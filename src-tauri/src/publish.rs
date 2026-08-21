use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use chrono::Utc;
use uuid::Uuid;

use crate::{
    error::{CoreError, CoreResult},
    models::{
        BackupRecord, ExecutePublishRequest, ModelConflict, PreparePublishRequest, PublishPreview,
        PublishResult, TargetKind, TargetPreview, TargetPublishResult,
    },
    secrets::SecretStore,
    store::{Store, TargetStateUpdate},
    target::{
        atomic_write, fingerprint, model_config, read_target_file, target_path, ConfigDocument,
    },
};

const BACKUP_RETENTION: usize = 10;

pub struct PublishCoordinator<'a> {
    pub store: &'a Store,
    pub secrets: Arc<dyn SecretStore>,
    pub backup_root: &'a Path,
}

impl PublishCoordinator<'_> {
    pub fn preview(
        &self,
        request: &PreparePublishRequest,
        target_paths: &HashMap<TargetKind, String>,
    ) -> CoreResult<PublishPreview> {
        validate_request(&request.model_ids, &request.targets)?;
        let gateway = self.store.gateway(&request.gateway_id)?;
        let models = self
            .store
            .selected_models(&request.gateway_id, &request.model_ids)?;
        let token = self.secrets.get(&gateway.token_ref)?;
        let incoming: Vec<_> = models
            .iter()
            .map(|model| model_config(model, &gateway, &token))
            .collect();
        let selected_ids: HashSet<_> = request.model_ids.iter().map(String::as_str).collect();
        let mut targets = Vec::new();
        let mut conflicts = Vec::new();

        for kind in &request.targets {
            let path = target_path(*kind, target_paths)?;
            let (mut document, original) = ConfigDocument::read(&path)?;
            conflicts.extend(document.collisions(&selected_ids).into_iter().map(
                |(model_id, existing_name)| ModelConflict {
                    target: *kind,
                    model_id,
                    existing_name,
                },
            ));
            let summary = document.merge(&incoming);
            targets.push(TargetPreview {
                target: *kind,
                path: path.to_string_lossy().to_string(),
                fingerprint: original.as_deref().map(fingerprint),
                add_count: summary.add_count,
                update_count: summary.update_count,
                unchanged_count: summary.unchanged_count,
            });
        }

        Ok(PublishPreview {
            targets,
            conflicts,
            warnings: vec![
                "WorkBuddy and CodeBuddy require the API token in their local models.json file."
                    .to_string(),
            ],
        })
    }

    pub fn execute(
        &self,
        request: &ExecutePublishRequest,
        target_paths: &HashMap<TargetKind, String>,
    ) -> CoreResult<PublishResult> {
        validate_request(&request.model_ids, &request.targets)?;
        let gateway = self.store.gateway(&request.gateway_id)?;
        let models = self
            .store
            .selected_models(&request.gateway_id, &request.model_ids)?;
        let token = self.secrets.get(&gateway.token_ref)?;
        let incoming: Vec<_> = models
            .iter()
            .map(|model| model_config(model, &gateway, &token))
            .collect();
        let selected_ids: HashSet<_> = request.model_ids.iter().map(String::as_str).collect();
        let expectation_map: HashMap<_, _> = request
            .expectations
            .iter()
            .map(|item| (item.target, item.fingerprint.as_deref()))
            .collect();
        let mut prepared = Vec::new();

        for kind in &request.targets {
            let path = target_path(*kind, target_paths)?;
            let (mut document, original) = ConfigDocument::read(&path)?;
            let current_fingerprint = original.as_deref().map(fingerprint);
            let expected = expectation_map.get(kind).copied().flatten();
            if current_fingerprint.as_deref() != expected {
                return Err(CoreError::Drift(format!(
                    "{} configuration changed after preview; reload before publishing",
                    kind.display_name()
                )));
            }
            let collisions = document.collisions(&selected_ids);
            if !collisions.is_empty() && !request.accept_conflicts {
                return Err(CoreError::Conflict(
                    "Confirm model replacements before publishing".to_string(),
                ));
            }
            document.merge(&incoming);
            prepared.push(PreparedTarget {
                kind: *kind,
                path,
                original,
                output: document.to_bytes()?,
            });
        }

        for target in &prepared {
            if let Some(original) = &target.original {
                self.create_backup(target.kind, &target.path, original)?;
            }
        }

        let mut committed: Vec<&PreparedTarget> = Vec::new();
        let mut results = Vec::new();
        for target in &prepared {
            if let Err(error) = write_and_verify(target, &selected_ids) {
                let rollback =
                    (!matches!(error, CoreError::Drift(_))).then(|| rollback_target(target));
                results.push(TargetPublishResult {
                    target: target.kind,
                    success: false,
                    rollback_attempted: rollback.is_some(),
                    rolled_back: rollback.as_ref().is_some_and(Result::is_ok),
                    message: match rollback {
                        Some(Ok(())) => format!("{error}; changes were rolled back"),
                        Some(Err(rollback_error)) => {
                            format!("{error}; rollback failed: {rollback_error}")
                        }
                        None => error.to_string(),
                    },
                });
                rollback_committed(&committed, &mut results, "a target write failed");
                return Ok(PublishResult {
                    success: false,
                    results,
                });
            }
            committed.push(target);
            results.push(TargetPublishResult {
                target: target.kind,
                success: true,
                rollback_attempted: false,
                rolled_back: false,
                message: format!("Published to {}", target.kind.display_name()),
            });
        }

        let state_updates: Vec<_> = prepared
            .iter()
            .map(|target| {
                let hash = fingerprint(&target.output);
                TargetStateUpdate {
                    target: target.kind,
                    path: target.path.to_string_lossy().to_string(),
                    seen_hash: Some(hash.clone()),
                    published_hash: Some(hash),
                    schema: "managed".to_string(),
                }
            })
            .collect();
        if self.store.save_target_states(&state_updates).is_err() {
            rollback_committed(
                &committed,
                &mut results,
                "the local publish state could not be saved",
            );
            return Ok(PublishResult {
                success: false,
                results,
            });
        }

        Ok(PublishResult {
            success: true,
            results,
        })
    }

    pub fn restore(&self, backup_id: &str) -> CoreResult<()> {
        let backup = self.store.backup(backup_id)?;
        let backup_path = PathBuf::from(&backup.path);
        let source_path = PathBuf::from(&backup.source_path);
        let bytes = read_target_file(&backup_path)?;
        if fingerprint(&bytes) != backup.fingerprint {
            return Err(CoreError::Conflict(
                "The backup file no longer matches its recorded fingerprint".to_string(),
            ));
        }
        ConfigDocument::parse(&bytes)?;
        let original = source_path
            .exists()
            .then(|| read_target_file(&source_path))
            .transpose()?;
        if let Some(current) = original.as_deref() {
            self.create_backup(backup.target, &source_path, current)?;
        }
        atomic_write(&source_path, &bytes)?;
        let hash = fingerprint(&bytes);
        if let Err(error) = self.store.save_target_state(
            backup.target,
            &backup.source_path,
            Some(&hash),
            Some(&hash),
            "restored",
        ) {
            let restored = PreparedTarget {
                kind: backup.target,
                path: source_path,
                original,
                output: bytes,
            };
            return match rollback_target(&restored) {
                Ok(()) => Err(error),
                Err(_) => Err(CoreError::Storage(
                    "Could not save restore state, and file recovery also failed".to_string(),
                )),
            };
        }
        Ok(())
    }

    fn create_backup(
        &self,
        target: TargetKind,
        source_path: &Path,
        bytes: &[u8],
    ) -> CoreResult<BackupRecord> {
        let directory = self.backup_root.join(target.as_str());
        fs::create_dir_all(&directory)?;
        let id = Uuid::new_v4().to_string();
        let timestamp = Utc::now().format("%Y%m%dT%H%M%S%.3fZ");
        let backup_path = directory.join(format!("{timestamp}-{id}.json"));
        atomic_write(&backup_path, bytes)?;
        let backup = BackupRecord {
            id,
            target,
            path: backup_path.to_string_lossy().to_string(),
            source_path: source_path.to_string_lossy().to_string(),
            fingerprint: fingerprint(bytes),
            created_at: Utc::now().to_rfc3339(),
        };
        self.store.add_backup(&backup)?;
        self.prune_backups(target)?;
        Ok(backup)
    }

    fn prune_backups(&self, target: TargetKind) -> CoreResult<()> {
        for backup in self
            .store
            .list_backups(Some(target))?
            .into_iter()
            .skip(BACKUP_RETENTION)
        {
            let path = PathBuf::from(&backup.path);
            if path.exists() {
                fs::remove_file(path)?;
            }
            self.store.remove_backup_record(&backup.id)?;
        }
        Ok(())
    }
}

#[derive(Debug)]
struct PreparedTarget {
    kind: TargetKind,
    path: PathBuf,
    original: Option<Vec<u8>>,
    output: Vec<u8>,
}

fn validate_request(model_ids: &[String], targets: &[TargetKind]) -> CoreResult<()> {
    if model_ids.is_empty() {
        return Err(CoreError::Validation(
            "Select at least one model to publish".to_string(),
        ));
    }
    if targets.is_empty() {
        return Err(CoreError::Validation(
            "Select WorkBuddy, CodeBuddy, or both".to_string(),
        ));
    }
    let unique_targets: HashSet<_> = targets.iter().collect();
    if unique_targets.len() != targets.len() {
        return Err(CoreError::Validation(
            "A configuration target can only be selected once".to_string(),
        ));
    }
    Ok(())
}

fn write_and_verify(target: &PreparedTarget, selected_ids: &HashSet<&str>) -> CoreResult<()> {
    if current_target_bytes(&target.path)? != target.original {
        return Err(CoreError::Drift(format!(
            "{} configuration changed immediately before publishing",
            target.kind.display_name()
        )));
    }
    atomic_write(&target.path, &target.output)?;
    let written = read_target_file(&target.path)?;
    let document = ConfigDocument::parse(&written)?;
    let written_ids: HashSet<_> = document
        .models()
        .iter()
        .filter_map(|model| model.get("id").and_then(serde_json::Value::as_str))
        .collect();
    if !selected_ids.is_subset(&written_ids) {
        return Err(CoreError::Target(format!(
            "{} verification failed after writing",
            target.kind.display_name()
        )));
    }
    Ok(())
}

fn rollback_target(target: &PreparedTarget) -> CoreResult<()> {
    let current = current_target_bytes(&target.path)?;
    if current == target.original {
        return Ok(());
    }
    if current.as_deref() != Some(target.output.as_slice()) {
        return Err(CoreError::Drift(format!(
            "{} configuration changed after publishing; external changes were preserved",
            target.kind.display_name()
        )));
    }
    if let Some(original) = &target.original {
        atomic_write(&target.path, original)
    } else if target.path.exists() {
        fs::remove_file(&target.path).map_err(|error| {
            CoreError::Target(format!(
                "Could not remove {}: {error}",
                target.path.display()
            ))
        })
    } else {
        Ok(())
    }
}

fn current_target_bytes(path: &Path) -> CoreResult<Option<Vec<u8>>> {
    path.exists().then(|| read_target_file(path)).transpose()
}

fn rollback_committed(
    committed: &[&PreparedTarget],
    results: &mut [TargetPublishResult],
    reason: &str,
) {
    for target in committed.iter().rev() {
        let rollback = rollback_target(target);
        if let Some(result) = results
            .iter_mut()
            .find(|result| result.target == target.kind)
        {
            result.success = false;
            result.rollback_attempted = true;
            result.rolled_back = rollback.is_ok();
            result.message = rollback
                .map(|_| format!("Published changes were rolled back because {reason}"))
                .unwrap_or_else(|error| format!("Rollback failed after {reason}: {error}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Mutex};

    use serde_json::{json, Value};
    use tempfile::TempDir;

    use super::*;
    use crate::{
        models::{CapabilitySet, GatewayProfile, ManagedModel, TargetExpectation},
        secrets::MISSING_SECRET_MESSAGE,
    };

    #[derive(Default)]
    struct MemorySecretStore {
        values: Mutex<HashMap<String, String>>,
    }

    impl SecretStore for MemorySecretStore {
        fn set(&self, key: &str, secret: &str) -> CoreResult<()> {
            self.values
                .lock()
                .unwrap()
                .insert(key.to_string(), secret.to_string());
            Ok(())
        }

        fn get(&self, key: &str) -> CoreResult<String> {
            self.values
                .lock()
                .unwrap()
                .get(key)
                .cloned()
                .ok_or_else(|| CoreError::SecretStore(MISSING_SECRET_MESSAGE.to_string()))
        }

        fn delete(&self, key: &str) -> CoreResult<()> {
            self.values.lock().unwrap().remove(key);
            Ok(())
        }
    }

    struct Fixture {
        directory: TempDir,
        store: Store,
        secrets: Arc<MemorySecretStore>,
        paths: HashMap<TargetKind, String>,
        backup_root: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let directory = tempfile::tempdir().unwrap();
            let store = Store::open(&directory.path().join("everybuddy.db")).unwrap();
            let profile = GatewayProfile {
                id: "gateway".to_string(),
                name: "Gateway".to_string(),
                api_root: "https://api.example.com/v1".to_string(),
                token_ref: "gateway".to_string(),
                created_at: "2026-08-20T00:00:00Z".to_string(),
                updated_at: "2026-08-20T00:00:00Z".to_string(),
            };
            store.save_gateway(&profile).unwrap();
            store
                .save_model(&ManagedModel {
                    key: "gateway::gpt-5".to_string(),
                    gateway_id: "gateway".to_string(),
                    id: "gpt-5".to_string(),
                    name: "GPT-5".to_string(),
                    vendor: "openai".to_string(),
                    capabilities: CapabilitySet::default(),
                    configuration: Default::default(),
                    evidence: Vec::new(),
                    metadata: json!({"id": "gpt-5"}),
                    updated_at: "2026-08-20T00:00:00Z".to_string(),
                })
                .unwrap();
            let secrets = Arc::new(MemorySecretStore::default());
            secrets.set("gateway", "test-token").unwrap();
            let paths = HashMap::from([
                (
                    TargetKind::Workbuddy,
                    directory
                        .path()
                        .join("workbuddy-models.json")
                        .to_string_lossy()
                        .to_string(),
                ),
                (
                    TargetKind::Codebuddy,
                    directory
                        .path()
                        .join("codebuddy-models.json")
                        .to_string_lossy()
                        .to_string(),
                ),
            ]);
            let backup_root = directory.path().join("backups");
            Self {
                directory,
                store,
                secrets,
                paths,
                backup_root,
            }
        }

        fn coordinator(&self) -> PublishCoordinator<'_> {
            PublishCoordinator {
                store: &self.store,
                secrets: self.secrets.clone(),
                backup_root: &self.backup_root,
            }
        }

        fn path(&self, target: TargetKind) -> PathBuf {
            PathBuf::from(self.paths.get(&target).unwrap())
        }

        fn request(&self, targets: Vec<TargetKind>) -> ExecutePublishRequest {
            let expectations = targets
                .iter()
                .map(|target| {
                    let path = self.path(*target);
                    TargetExpectation {
                        target: *target,
                        fingerprint: path.exists().then(|| fingerprint(&fs::read(path).unwrap())),
                    }
                })
                .collect();
            ExecutePublishRequest {
                gateway_id: "gateway".to_string(),
                model_ids: vec!["gpt-5".to_string()],
                targets,
                expectations,
                accept_conflicts: true,
            }
        }
    }

    fn model_ids(path: &Path) -> Vec<String> {
        let bytes = fs::read(path).unwrap();
        ConfigDocument::parse(&bytes)
            .unwrap()
            .models()
            .iter()
            .filter_map(|model| model.get("id").and_then(Value::as_str))
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn rejects_empty_publish_selection() {
        assert!(validate_request(&[], &[TargetKind::Workbuddy]).is_err());
        assert!(validate_request(&["gpt-5".to_string()], &[]).is_err());
    }

    #[test]
    fn publishes_to_each_single_target() {
        for target in [TargetKind::Workbuddy, TargetKind::Codebuddy] {
            let fixture = Fixture::new();
            fs::write(fixture.path(target), b"[]\n").unwrap();
            let request = fixture.request(vec![target]);

            let result = fixture
                .coordinator()
                .execute(&request, &fixture.paths)
                .unwrap();

            assert!(result.success);
            assert_eq!(result.results.len(), 1);
            assert_eq!(model_ids(&fixture.path(target)), vec!["gpt-5"]);
        }
    }

    #[test]
    fn publishes_to_both_targets() {
        let fixture = Fixture::new();
        for target in [TargetKind::Workbuddy, TargetKind::Codebuddy] {
            fs::write(fixture.path(target), b"[]\n").unwrap();
        }
        let request = fixture.request(vec![TargetKind::Workbuddy, TargetKind::Codebuddy]);

        let result = fixture
            .coordinator()
            .execute(&request, &fixture.paths)
            .unwrap();

        assert!(result.success);
        assert_eq!(
            model_ids(&fixture.path(TargetKind::Workbuddy)),
            vec!["gpt-5"]
        );
        assert_eq!(
            model_ids(&fixture.path(TargetKind::Codebuddy)),
            vec!["gpt-5"]
        );
    }

    #[cfg(unix)]
    #[test]
    fn rolls_back_first_target_when_second_write_fails() {
        use std::os::unix::fs::symlink;

        let fixture = Fixture::new();
        let original = b"[]\n";
        fs::write(fixture.path(TargetKind::Workbuddy), original).unwrap();
        symlink(
            fixture.directory.path().join("missing.json"),
            fixture.path(TargetKind::Codebuddy),
        )
        .unwrap();
        let request = fixture.request(vec![TargetKind::Workbuddy, TargetKind::Codebuddy]);

        let result = fixture
            .coordinator()
            .execute(&request, &fixture.paths)
            .unwrap();

        assert!(!result.success);
        assert_eq!(
            fs::read(fixture.path(TargetKind::Workbuddy)).unwrap(),
            original
        );
        assert!(result.results.iter().all(|item| item.rolled_back));
    }

    #[cfg(unix)]
    #[test]
    fn removes_new_first_target_during_compensation() {
        use std::os::unix::fs::symlink;

        let fixture = Fixture::new();
        symlink(
            fixture.directory.path().join("missing.json"),
            fixture.path(TargetKind::Codebuddy),
        )
        .unwrap();
        let request = fixture.request(vec![TargetKind::Workbuddy, TargetKind::Codebuddy]);

        let result = fixture
            .coordinator()
            .execute(&request, &fixture.paths)
            .unwrap();

        assert!(!result.success);
        assert!(!fixture.path(TargetKind::Workbuddy).exists());
    }

    #[test]
    fn drift_stops_publish_before_any_write() {
        let fixture = Fixture::new();
        let path = fixture.path(TargetKind::Workbuddy);
        fs::write(&path, b"[]\n").unwrap();
        let mut request = fixture.request(vec![TargetKind::Workbuddy]);
        request.expectations[0].fingerprint = Some("stale".to_string());

        let error = fixture
            .coordinator()
            .execute(&request, &fixture.paths)
            .unwrap_err();

        assert!(matches!(error, CoreError::Drift(_)));
        assert_eq!(fs::read(path).unwrap(), b"[]\n");
    }

    #[test]
    fn write_rechecks_drift_immediately_before_replacing_the_file() {
        let fixture = Fixture::new();
        let path = fixture.path(TargetKind::Workbuddy);
        let original = b"[]\n".to_vec();
        let external = b"[{\"id\":\"external\"}]\n".to_vec();
        fs::write(&path, &external).unwrap();
        let target = PreparedTarget {
            kind: TargetKind::Workbuddy,
            path: path.clone(),
            original: Some(original),
            output: b"[{\"id\":\"gpt-5\"}]\n".to_vec(),
        };

        let error = write_and_verify(&target, &HashSet::from(["gpt-5"])).unwrap_err();

        assert!(matches!(error, CoreError::Drift(_)));
        assert_eq!(fs::read(path).unwrap(), external);
    }

    #[test]
    fn rollback_preserves_changes_made_after_publish() {
        let fixture = Fixture::new();
        let path = fixture.path(TargetKind::Workbuddy);
        let external = b"[{\"id\":\"external\"}]\n".to_vec();
        fs::write(&path, &external).unwrap();
        let target = PreparedTarget {
            kind: TargetKind::Workbuddy,
            path: path.clone(),
            original: Some(b"[]\n".to_vec()),
            output: b"[{\"id\":\"gpt-5\"}]\n".to_vec(),
        };

        let error = rollback_target(&target).unwrap_err();

        assert!(matches!(error, CoreError::Drift(_)));
        assert_eq!(fs::read(path).unwrap(), external);
    }

    #[test]
    fn restores_a_verified_backup() {
        let fixture = Fixture::new();
        let path = fixture.path(TargetKind::Workbuddy);
        let original = b"[{\"id\":\"legacy\",\"name\":\"Legacy\"}]\n";
        fs::write(&path, original).unwrap();
        let request = fixture.request(vec![TargetKind::Workbuddy]);
        fixture
            .coordinator()
            .execute(&request, &fixture.paths)
            .unwrap();
        let backup = fixture
            .store
            .list_backups(Some(TargetKind::Workbuddy))
            .unwrap()
            .into_iter()
            .find(|backup| fingerprint(original) == backup.fingerprint)
            .unwrap();

        fixture.coordinator().restore(&backup.id).unwrap();

        assert_eq!(fs::read(path).unwrap(), original);
    }

    #[test]
    fn rolls_back_restore_when_target_state_save_fails() {
        let fixture = Fixture::new();
        let path = fixture.path(TargetKind::Workbuddy);
        let original = b"[{\"id\":\"legacy\",\"name\":\"Legacy\"}]\n";
        fs::write(&path, original).unwrap();
        let request = fixture.request(vec![TargetKind::Workbuddy]);
        fixture
            .coordinator()
            .execute(&request, &fixture.paths)
            .unwrap();
        let published = fs::read(&path).unwrap();
        let backup = fixture
            .store
            .list_backups(Some(TargetKind::Workbuddy))
            .unwrap()
            .into_iter()
            .find(|backup| fingerprint(original) == backup.fingerprint)
            .unwrap();
        fixture
            .store
            .execute_test_sql(
                r#"
                CREATE TRIGGER fail_restore_state
                BEFORE INSERT ON target_states
                BEGIN
                    SELECT RAISE(FAIL, 'injected restore state failure');
                END;
                "#,
            )
            .unwrap();

        let error = fixture.coordinator().restore(&backup.id).unwrap_err();

        assert!(matches!(error, CoreError::Storage(_)));
        assert_eq!(fs::read(path).unwrap(), published);
    }

    #[test]
    fn retains_only_ten_backups_per_target() {
        let fixture = Fixture::new();
        let path = fixture.path(TargetKind::Workbuddy);
        let coordinator = fixture.coordinator();

        for index in 0..11 {
            coordinator
                .create_backup(
                    TargetKind::Workbuddy,
                    &path,
                    format!("[{{\"id\":\"model-{index}\"}}]\n").as_bytes(),
                )
                .unwrap();
        }

        assert_eq!(
            fixture
                .store
                .list_backups(Some(TargetKind::Workbuddy))
                .unwrap()
                .len(),
            10
        );
        assert_eq!(
            fs::read_dir(fixture.directory.path().join("backups/workbuddy"))
                .unwrap()
                .count(),
            10
        );
    }

    #[test]
    fn rolls_back_files_when_target_state_transaction_fails() {
        let fixture = Fixture::new();
        for target in [TargetKind::Workbuddy, TargetKind::Codebuddy] {
            fs::write(fixture.path(target), b"[]\n").unwrap();
        }
        fixture
            .store
            .execute_test_sql(
                r#"
                CREATE TRIGGER fail_codebuddy_state
                BEFORE INSERT ON target_states
                WHEN NEW.target = 'codebuddy'
                BEGIN
                    SELECT RAISE(FAIL, 'injected target state failure');
                END;
                "#,
            )
            .unwrap();
        let request = fixture.request(vec![TargetKind::Workbuddy, TargetKind::Codebuddy]);

        let result = fixture
            .coordinator()
            .execute(&request, &fixture.paths)
            .unwrap();

        assert!(!result.success);
        assert!(result.results.iter().all(|item| item.rolled_back));
        assert_eq!(
            fs::read(fixture.path(TargetKind::Workbuddy)).unwrap(),
            b"[]\n"
        );
        assert_eq!(
            fs::read(fixture.path(TargetKind::Codebuddy)).unwrap(),
            b"[]\n"
        );
        assert!(fixture
            .store
            .target_last_published_hash(TargetKind::Workbuddy)
            .unwrap()
            .is_none());
    }
}
