use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use chrono::Utc;
use uuid::Uuid;

use crate::{
    capability::{has_unverified_market_match, supports_chat_configuration},
    error::{CoreError, CoreResult},
    gateway_service::{gateway_source_hash, source_identity_key},
    models::{
        BackupRecord, ExecutePublishRequest, ModelConflict, ModelRevision, PreparePublishRequest,
        PublishPreview, PublishResult, TargetKind, TargetPreview, TargetPublishResult,
    },
    secrets::SecretStore,
    store::{Store, TargetStateUpdate},
    target::{
        atomic_write, fingerprint, model_config, read_target_file, target_path, target_write_path,
        ConfigDocument,
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
        let managed_models = self.store.models_for_gateway(&request.gateway_id)?;
        validate_model_configurations(&models)?;
        let token = self.secrets.get(&gateway.token_ref)?;
        let identity_key = source_identity_key(
            self.secrets.as_ref(),
            self.store.has_gateway_source_history()?,
        )?;
        let credential_revision = gateway_source_hash(&identity_key, &gateway.api_root, &token);
        let incoming: Vec<_> = models
            .iter()
            .map(|model| model_config(model, &gateway, &token))
            .collect();
        let managed: Vec<_> = managed_models
            .iter()
            .map(|model| model_config(model, &gateway, &token))
            .collect();
        let selected_ids: HashSet<_> = request.model_ids.iter().map(String::as_str).collect();
        let mut targets = Vec::new();
        let mut conflicts = Vec::new();

        for kind in &request.targets {
            let path = target_path(*kind, target_paths)?;
            let write_path = target_write_path(&path)?;
            let (mut document, original) = ConfigDocument::read(&write_path)?;
            conflicts.extend(document.collisions(&selected_ids).into_iter().map(
                |(model_id, existing_name)| ModelConflict {
                    target: *kind,
                    model_id,
                    existing_name,
                },
            ));
            let summary = document.sync(&incoming, &managed);
            targets.push(TargetPreview {
                target: *kind,
                path: path.to_string_lossy().to_string(),
                write_path: write_path.to_string_lossy().to_string(),
                fingerprint: original.as_deref().map(fingerprint),
                add_count: summary.add_count,
                update_count: summary.update_count,
                unchanged_count: summary.unchanged_count,
                remove_count: summary.remove_count,
            });
        }

        Ok(PublishPreview {
            targets,
            conflicts,
            warnings: vec![
                "WorkBuddy and CodeBuddy require the API token in their local models.json file."
                    .to_string(),
            ],
            gateway_revision: gateway.updated_at,
            credential_revision,
            model_revisions: model_revisions(&managed_models),
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
        let managed_models = self.store.models_for_gateway(&request.gateway_id)?;
        validate_model_configurations(&models)?;
        let token = self.secrets.get(&gateway.token_ref)?;
        let identity_key = source_identity_key(
            self.secrets.as_ref(),
            self.store.has_gateway_source_history()?,
        )?;
        let credential_revision = gateway_source_hash(&identity_key, &gateway.api_root, &token);
        validate_resource_revisions(
            request,
            &gateway.updated_at,
            &credential_revision,
            &managed_models,
        )?;
        let mut source_hashes: Vec<_> = models
            .iter()
            .map(|model| {
                let api_root = model
                    .configuration
                    .endpoint_override
                    .as_deref()
                    .unwrap_or(&gateway.api_root);
                gateway_source_hash(&identity_key, api_root, &token)
            })
            .collect();
        source_hashes.push(credential_revision.clone());
        source_hashes.sort_unstable();
        source_hashes.dedup();
        let incoming: Vec<_> = models
            .iter()
            .map(|model| model_config(model, &gateway, &token))
            .collect();
        let managed: Vec<_> = managed_models
            .iter()
            .map(|model| model_config(model, &gateway, &token))
            .collect();
        let selected_ids: HashSet<_> = request.model_ids.iter().map(String::as_str).collect();
        let expectation_map: HashMap<_, _> = request
            .expectations
            .iter()
            .map(|item| (item.target, item))
            .collect();
        if request.expectations.len() != request.targets.len()
            || expectation_map.len() != request.targets.len()
            || request
                .targets
                .iter()
                .any(|target| !expectation_map.contains_key(target))
        {
            return Err(CoreError::Conflict(
                "The publish preview is incomplete; create a new preview".to_string(),
            ));
        }
        let mut prepared = Vec::new();

        for kind in &request.targets {
            let path = target_path(*kind, target_paths)?;
            let expectation = expectation_map
                .get(kind)
                .expect("expectations were validated above");
            if path != Path::new(&expectation.path) {
                return Err(CoreError::Conflict(format!(
                    "{} path changed after preview; create a new preview",
                    kind.display_name()
                )));
            }
            let write_path = target_write_path(&path)?;
            if write_path != Path::new(&expectation.write_path) {
                return Err(CoreError::Conflict(format!(
                    "{} write destination changed after preview; create a new preview",
                    kind.display_name()
                )));
            }
            let (mut document, original) = ConfigDocument::read(&write_path)?;
            let current_fingerprint = original.as_deref().map(fingerprint);
            if current_fingerprint.as_deref() != expectation.fingerprint.as_deref() {
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
            document.sync(&incoming, &managed);
            prepared.push(PreparedTarget {
                kind: *kind,
                configured_path: path,
                write_path,
                original,
                output: document.to_bytes()?,
            });
        }

        let current_token = self.secrets.get(&gateway.token_ref)?;
        let current_credential_revision =
            gateway_source_hash(&identity_key, &gateway.api_root, &current_token);
        if current_credential_revision != request.credential_revision {
            return Err(CoreError::Conflict(
                "The API credential changed after preview; create a new preview".to_string(),
            ));
        }

        for target in &prepared {
            if let Some(original) = &target.original {
                self.create_backup(target.kind, &target.write_path, original)?;
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
                    path: target.configured_path.to_string_lossy().to_string(),
                    seen_hash: Some(hash.clone()),
                    published_hash: Some(hash),
                    schema: "managed".to_string(),
                }
            })
            .collect();
        if self
            .store
            .save_publish_state(&gateway.id, &source_hashes, &state_updates)
            .is_err()
        {
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
                configured_path: source_path.clone(),
                write_path: source_path,
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
        if let Err(error) = self.store.add_backup(&backup) {
            return match fs::remove_file(&backup_path) {
                Ok(()) => Err(error),
                Err(_) => Err(CoreError::Storage(
                    "Could not record the backup, and backup file cleanup also failed".to_string(),
                )),
            };
        }
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
            self.store.remove_backup_record(&backup.id)?;
            if path.exists() {
                if let Err(error) = fs::remove_file(&path) {
                    return match self.store.add_backup(&backup) {
                        Ok(()) => Err(CoreError::Storage(error.to_string())),
                        Err(_) => Err(CoreError::Storage(
                            "Could not remove an expired backup, and its database record could not be restored"
                                .to_string(),
                        )),
                    };
                }
            }
        }
        Ok(())
    }
}

fn model_revisions(models: &[crate::models::ManagedModel]) -> Vec<ModelRevision> {
    let mut revisions: Vec<_> = models
        .iter()
        .map(|model| ModelRevision {
            key: model.key.clone(),
            updated_at: model.updated_at.clone(),
        })
        .collect();
    revisions.sort_by(|left, right| left.key.cmp(&right.key));
    revisions
}

fn validate_resource_revisions(
    request: &ExecutePublishRequest,
    gateway_revision: &str,
    credential_revision: &str,
    models: &[crate::models::ManagedModel],
) -> CoreResult<()> {
    if request.gateway_revision != gateway_revision
        || request.credential_revision != credential_revision
        || request.model_revisions != model_revisions(models)
    {
        return Err(CoreError::Conflict(
            "The API profile or selected models changed after preview; create a new preview"
                .to_string(),
        ));
    }
    Ok(())
}

#[derive(Debug)]
struct PreparedTarget {
    kind: TargetKind,
    configured_path: PathBuf,
    write_path: PathBuf,
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
    let unique_models: HashSet<_> = model_ids.iter().collect();
    if unique_models.len() != model_ids.len() {
        return Err(CoreError::Validation(
            "A model can only be selected once".to_string(),
        ));
    }
    Ok(())
}

fn validate_model_configurations(models: &[crate::models::ManagedModel]) -> CoreResult<()> {
    if models
        .iter()
        .any(|model| !model.configuration.has_valid_numeric_values())
    {
        return Err(CoreError::Validation(
            "One or more selected models contain invalid numeric configuration".to_string(),
        ));
    }
    if models.iter().any(|model| {
        model
            .configuration
            .reasoning
            .summary
            .is_some_and(|summary| !summary.is_supported_target_value())
    }) {
        return Err(CoreError::Validation(
            "One or more selected models use an unsupported reasoning summary".to_string(),
        ));
    }
    if models.iter().any(|model| {
        model.configuration.use_custom_protocol && model.configuration.endpoint_override.is_none()
    }) {
        return Err(CoreError::Validation(
            "One or more custom protocol models are missing a complete request URL".to_string(),
        ));
    }
    if models
        .iter()
        .any(|model| has_unverified_market_match(&model.metadata))
    {
        return Err(CoreError::Validation(
            "One or more selected models need an OpenRouter capability refresh before publishing"
                .to_string(),
        ));
    }
    if models.iter().any(|model| {
        !supports_chat_configuration(&model.metadata)
            && (model.capabilities.supports_tool_call
                || model.capabilities.supports_images
                || model.capabilities.supports_reasoning
                || !model.capabilities.reasoning_efforts.is_empty()
                || model.configuration.max_input_tokens.is_some()
                || model.configuration.max_output_tokens.is_some()
                || model.configuration.temperature.is_some()
                || model.configuration.only_reasoning
                || model.configuration.reasoning != Default::default())
    }) {
        return Err(CoreError::Validation(
            "One or more non-text models contain unsupported chat capabilities or parameters"
                .to_string(),
        ));
    }
    Ok(())
}

fn write_and_verify(target: &PreparedTarget, selected_ids: &HashSet<&str>) -> CoreResult<()> {
    if current_target_bytes(&target.write_path)? != target.original {
        return Err(CoreError::Drift(format!(
            "{} configuration changed immediately before publishing",
            target.kind.display_name()
        )));
    }
    atomic_write(&target.write_path, &target.output)?;
    let written = read_target_file(&target.write_path)?;
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
    let current = current_target_bytes(&target.write_path)?;
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
        atomic_write(&target.write_path, original)
    } else if target.write_path.exists() {
        fs::remove_file(&target.write_path).map_err(|error| {
            CoreError::Target(format!(
                "Could not remove {}: {error}",
                target.write_path.display()
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
                    let write_path = target_write_path(&path).unwrap();
                    TargetExpectation {
                        target: *target,
                        path: path.to_string_lossy().to_string(),
                        write_path: write_path.to_string_lossy().to_string(),
                        fingerprint: write_path
                            .exists()
                            .then(|| fingerprint(&fs::read(write_path).unwrap())),
                    }
                })
                .collect();
            let gateway = self.store.gateway("gateway").unwrap();
            let token = self.secrets.get("gateway").unwrap();
            let identity_key = source_identity_key(
                self.secrets.as_ref(),
                self.store.has_gateway_source_history().unwrap(),
            )
            .unwrap();
            ExecutePublishRequest {
                gateway_id: "gateway".to_string(),
                model_ids: vec!["gpt-5".to_string()],
                targets,
                expectations,
                gateway_revision: gateway.updated_at,
                credential_revision: gateway_source_hash(&identity_key, &gateway.api_root, &token),
                model_revisions: model_revisions(
                    &self.store.models_for_gateway("gateway").unwrap(),
                ),
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
        assert!(validate_request(
            &["gpt-5".to_string(), "gpt-5".to_string()],
            &[TargetKind::Workbuddy]
        )
        .is_err());
    }

    #[test]
    fn preview_rejects_invalid_legacy_model_configuration() {
        let fixture = Fixture::new();
        let mut model = fixture.store.model("gateway::gpt-5").unwrap();
        model.configuration.temperature = Some(-0.1);
        fixture.store.save_model(&model).unwrap();
        let request = PreparePublishRequest {
            gateway_id: "gateway".to_string(),
            model_ids: vec!["gpt-5".to_string()],
            targets: vec![TargetKind::Workbuddy],
        };

        let error = fixture
            .coordinator()
            .preview(&request, &fixture.paths)
            .unwrap_err();

        assert!(matches!(error, CoreError::Validation(_)));
        assert!(!fixture.path(TargetKind::Workbuddy).exists());
    }

    #[test]
    fn preview_rejects_legacy_reasoning_summary_values() {
        let fixture = Fixture::new();
        let mut model = fixture.store.model("gateway::gpt-5").unwrap();
        model.capabilities.supports_reasoning = true;
        model.configuration.reasoning.summary = Some(crate::models::ReasoningSummary::Never);
        fixture.store.save_model(&model).unwrap();
        let request = PreparePublishRequest {
            gateway_id: "gateway".to_string(),
            model_ids: vec!["gpt-5".to_string()],
            targets: vec![TargetKind::Workbuddy],
        };

        let error = fixture
            .coordinator()
            .preview(&request, &fixture.paths)
            .unwrap_err();

        assert!(matches!(error, CoreError::Validation(_)));
        assert!(!fixture.path(TargetKind::Workbuddy).exists());
    }

    #[test]
    fn preview_rejects_custom_protocol_without_a_request_url() {
        let fixture = Fixture::new();
        let mut model = fixture.store.model("gateway::gpt-5").unwrap();
        model.configuration.use_custom_protocol = true;
        model.configuration.endpoint_override = None;
        fixture.store.save_model(&model).unwrap();
        let request = PreparePublishRequest {
            gateway_id: "gateway".to_string(),
            model_ids: vec!["gpt-5".to_string()],
            targets: vec![TargetKind::Workbuddy],
        };

        let error = fixture
            .coordinator()
            .preview(&request, &fixture.paths)
            .unwrap_err();

        assert!(matches!(error, CoreError::Validation(_)));
        assert!(!fixture.path(TargetKind::Workbuddy).exists());
    }

    #[test]
    fn preview_rejects_legacy_non_text_chat_projection() {
        let fixture = Fixture::new();
        let mut model = fixture.store.model("gateway::gpt-5").unwrap();
        model.metadata["everybuddyOpenRouterMatch"] = json!({
            "source": "openrouter",
            "modelId": "provider/image-model",
            "supportsTextOutput": false
        });
        model.capabilities.supports_images = true;
        model.configuration.max_output_tokens = Some(4_096);
        fixture.store.save_model(&model).unwrap();
        let request = PreparePublishRequest {
            gateway_id: "gateway".to_string(),
            model_ids: vec!["gpt-5".to_string()],
            targets: vec![TargetKind::Workbuddy],
        };

        let error = fixture
            .coordinator()
            .preview(&request, &fixture.paths)
            .unwrap_err();

        assert!(matches!(error, CoreError::Validation(_)));
        assert!(!fixture.path(TargetKind::Workbuddy).exists());
    }

    #[test]
    fn preview_rejects_unverified_legacy_market_matches() {
        let fixture = Fixture::new();
        let mut model = fixture.store.model("gateway::gpt-5").unwrap();
        model.metadata["everybuddyOpenRouterMatch"] = json!({
            "source": "openrouter",
            "modelId": "openai/gpt-5"
        });
        fixture.store.save_model(&model).unwrap();
        let request = PreparePublishRequest {
            gateway_id: "gateway".to_string(),
            model_ids: vec!["gpt-5".to_string()],
            targets: vec![TargetKind::Workbuddy],
        };

        let error = fixture
            .coordinator()
            .preview(&request, &fixture.paths)
            .unwrap_err();

        assert!(error.to_string().contains("capability refresh"));
        assert!(!fixture.path(TargetKind::Workbuddy).exists());
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
        assert_eq!(
            fs::read(fixture.path(TargetKind::Workbuddy)).unwrap(),
            fs::read(fixture.path(TargetKind::Codebuddy)).unwrap()
        );
        assert!(fixture.store.has_gateway_source_history().unwrap());
    }

    #[test]
    fn publishing_selection_removes_only_unselected_models_from_current_gateway() {
        let fixture = Fixture::new();
        let gateway = fixture.store.gateway("gateway").unwrap();
        let mut image_model = fixture.store.model("gateway::gpt-5").unwrap();
        image_model.key = "gateway::gpt-image-2".to_string();
        image_model.id = "gpt-image-2".to_string();
        image_model.name = "GPT Image 2".to_string();
        image_model.updated_at = "2026-08-20T00:00:01Z".to_string();
        fixture.store.save_model(&image_model).unwrap();

        let selected = model_config(
            &fixture.store.model("gateway::gpt-5").unwrap(),
            &gateway,
            "test-token",
        );
        let mut unselected = model_config(&image_model, &gateway, "test-token");
        unselected["url"] = json!("https://api.example.com/v1/images/generations");
        unselected["useCustomProtocol"] = json!(true);
        let external = json!({
            "id": "external-model",
            "name": "External model",
            "url": "https://other.example.com/v1",
            "apiKey": "other-token"
        });
        let unmanaged = json!({
            "id": "local-model",
            "name": "Local model",
            "url": "https://api.example.com/v1",
            "apiKey": "test-token"
        });
        let models = vec![selected, unselected, external, unmanaged];
        fs::write(
            fixture.path(TargetKind::Workbuddy),
            serde_json::to_vec_pretty(&models).unwrap(),
        )
        .unwrap();
        fs::write(
            fixture.path(TargetKind::Codebuddy),
            serde_json::to_vec_pretty(&json!({
                "models": models,
                "availableModels": ["preserved-root-field"]
            }))
            .unwrap(),
        )
        .unwrap();

        let preview = fixture
            .coordinator()
            .preview(
                &PreparePublishRequest {
                    gateway_id: "gateway".to_string(),
                    model_ids: vec!["gpt-5".to_string()],
                    targets: vec![TargetKind::Workbuddy, TargetKind::Codebuddy],
                },
                &fixture.paths,
            )
            .unwrap();
        assert!(preview
            .targets
            .iter()
            .all(|target| target.remove_count == 1));

        let request = fixture.request(vec![TargetKind::Workbuddy, TargetKind::Codebuddy]);
        let result = fixture
            .coordinator()
            .execute(&request, &fixture.paths)
            .unwrap();

        assert!(result.success);
        for target in [TargetKind::Workbuddy, TargetKind::Codebuddy] {
            assert_eq!(
                model_ids(&fixture.path(target)),
                vec!["gpt-5", "external-model", "local-model"]
            );
        }
        let codebuddy: Value =
            serde_json::from_slice(&fs::read(fixture.path(TargetKind::Codebuddy)).unwrap())
                .unwrap();
        assert_eq!(
            codebuddy["availableModels"],
            json!(["preserved-root-field"])
        );
    }

    #[cfg(unix)]
    #[test]
    fn rolls_back_first_target_when_second_write_fails() {
        use std::os::unix::fs::PermissionsExt;

        let mut fixture = Fixture::new();
        let original = b"[]\n";
        fs::write(fixture.path(TargetKind::Workbuddy), original).unwrap();
        let read_only_directory = fixture.directory.path().join("read-only");
        fs::create_dir(&read_only_directory).unwrap();
        let codebuddy_path = read_only_directory.join("models.json");
        fs::write(&codebuddy_path, original).unwrap();
        fixture.paths.insert(
            TargetKind::Codebuddy,
            codebuddy_path.to_string_lossy().to_string(),
        );
        let request = fixture.request(vec![TargetKind::Workbuddy, TargetKind::Codebuddy]);
        fs::set_permissions(&read_only_directory, fs::Permissions::from_mode(0o500)).unwrap();

        let result = fixture
            .coordinator()
            .execute(&request, &fixture.paths)
            .unwrap();
        fs::set_permissions(&read_only_directory, fs::Permissions::from_mode(0o700)).unwrap();

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
        use std::os::unix::fs::PermissionsExt;

        let mut fixture = Fixture::new();
        let read_only_directory = fixture.directory.path().join("read-only");
        fs::create_dir(&read_only_directory).unwrap();
        let codebuddy_path = read_only_directory.join("models.json");
        fs::write(&codebuddy_path, b"[]\n").unwrap();
        fixture.paths.insert(
            TargetKind::Codebuddy,
            codebuddy_path.to_string_lossy().to_string(),
        );
        let request = fixture.request(vec![TargetKind::Workbuddy, TargetKind::Codebuddy]);
        fs::set_permissions(&read_only_directory, fs::Permissions::from_mode(0o500)).unwrap();

        let result = fixture
            .coordinator()
            .execute(&request, &fixture.paths)
            .unwrap();
        fs::set_permissions(&read_only_directory, fs::Permissions::from_mode(0o700)).unwrap();

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
    fn extra_preview_expectation_stops_publish_before_any_write() {
        let fixture = Fixture::new();
        let path = fixture.path(TargetKind::Workbuddy);
        fs::write(&path, b"[]\n").unwrap();
        let mut request = fixture.request(vec![TargetKind::Workbuddy]);
        request.expectations.push(request.expectations[0].clone());

        let error = fixture
            .coordinator()
            .execute(&request, &fixture.paths)
            .unwrap_err();

        assert!(matches!(error, CoreError::Conflict(_)));
        assert_eq!(fs::read(path).unwrap(), b"[]\n");
        assert!(!fixture.store.has_gateway_source_history().unwrap());
    }

    #[test]
    fn resource_revision_change_stops_publish_before_any_write() {
        let fixture = Fixture::new();
        let path = fixture.path(TargetKind::Workbuddy);
        fs::write(&path, b"[]\n").unwrap();
        let mut request = fixture.request(vec![TargetKind::Workbuddy]);
        request.model_revisions[0].updated_at = "stale".to_string();

        let error = fixture
            .coordinator()
            .execute(&request, &fixture.paths)
            .unwrap_err();

        assert!(matches!(error, CoreError::Conflict(_)));
        assert_eq!(fs::read(path).unwrap(), b"[]\n");
    }

    #[test]
    fn credential_change_stops_publish_before_any_write() {
        let fixture = Fixture::new();
        let path = fixture.path(TargetKind::Workbuddy);
        fs::write(&path, b"[]\n").unwrap();
        let request = fixture.request(vec![TargetKind::Workbuddy]);
        fixture.secrets.set("gateway", "rotated-token").unwrap();

        let error = fixture
            .coordinator()
            .execute(&request, &fixture.paths)
            .unwrap_err();

        assert!(matches!(error, CoreError::Conflict(_)));
        assert_eq!(fs::read(path).unwrap(), b"[]\n");
    }

    #[test]
    fn settings_path_change_stops_publish_before_any_write() {
        let fixture = Fixture::new();
        let original_path = fixture.path(TargetKind::Workbuddy);
        let alternate_path = fixture.directory.path().join("alternate.json");
        fs::write(&original_path, b"[]\n").unwrap();
        fs::write(&alternate_path, b"[]\n").unwrap();
        let request = fixture.request(vec![TargetKind::Workbuddy]);
        let mut changed_paths = fixture.paths.clone();
        changed_paths.insert(
            TargetKind::Workbuddy,
            alternate_path.to_string_lossy().to_string(),
        );

        let error = fixture
            .coordinator()
            .execute(&request, &changed_paths)
            .unwrap_err();

        assert!(matches!(error, CoreError::Conflict(_)));
        assert_eq!(fs::read(original_path).unwrap(), b"[]\n");
        assert_eq!(fs::read(alternate_path).unwrap(), b"[]\n");
    }

    #[cfg(unix)]
    #[test]
    fn symlink_retarget_stops_publish_before_any_write() {
        use std::os::unix::fs::symlink;

        let mut fixture = Fixture::new();
        let first = fixture.directory.path().join("first.json");
        let second = fixture.directory.path().join("second.json");
        let link = fixture.directory.path().join("models-link.json");
        fs::write(&first, b"[]\n").unwrap();
        fs::write(&second, b"[]\n").unwrap();
        symlink(&first, &link).unwrap();
        fixture
            .paths
            .insert(TargetKind::Workbuddy, link.to_string_lossy().to_string());
        let request = fixture.request(vec![TargetKind::Workbuddy]);
        fs::remove_file(&link).unwrap();
        symlink(&second, &link).unwrap();

        let error = fixture
            .coordinator()
            .execute(&request, &fixture.paths)
            .unwrap_err();

        assert!(matches!(error, CoreError::Conflict(_)));
        assert_eq!(fs::read(first).unwrap(), b"[]\n");
        assert_eq!(fs::read(second).unwrap(), b"[]\n");
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
            configured_path: path.clone(),
            write_path: path.clone(),
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
            configured_path: path.clone(),
            write_path: path.clone(),
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
    fn keeps_backup_file_when_retention_record_delete_fails() {
        let fixture = Fixture::new();
        let path = fixture.path(TargetKind::Workbuddy);
        let coordinator = fixture.coordinator();
        for index in 0..BACKUP_RETENTION {
            coordinator
                .create_backup(
                    TargetKind::Workbuddy,
                    &path,
                    format!("[{{\"id\":\"model-{index}\"}}]\n").as_bytes(),
                )
                .unwrap();
        }
        fixture
            .store
            .execute_test_sql(
                r#"
                CREATE TRIGGER fail_backup_delete
                BEFORE DELETE ON backups
                BEGIN
                    SELECT RAISE(FAIL, 'injected backup delete failure');
                END;
                "#,
            )
            .unwrap();

        let error = coordinator
            .create_backup(TargetKind::Workbuddy, &path, b"[{\"id\":\"latest\"}]\n")
            .unwrap_err();
        let backups = fixture
            .store
            .list_backups(Some(TargetKind::Workbuddy))
            .unwrap();

        assert!(matches!(error, CoreError::Storage(_)));
        assert_eq!(backups.len(), BACKUP_RETENTION + 1);
        assert!(backups
            .iter()
            .all(|backup| Path::new(&backup.path).exists()));
    }

    #[test]
    fn restores_backup_record_when_retention_file_delete_fails() {
        let fixture = Fixture::new();
        let path = fixture.path(TargetKind::Workbuddy);
        let coordinator = fixture.coordinator();
        for index in 0..BACKUP_RETENTION {
            coordinator
                .create_backup(
                    TargetKind::Workbuddy,
                    &path,
                    format!("[{{\"id\":\"model-{index}\"}}]\n").as_bytes(),
                )
                .unwrap();
        }
        let oldest = fixture
            .store
            .list_backups(Some(TargetKind::Workbuddy))
            .unwrap()
            .pop()
            .unwrap();
        fs::remove_file(&oldest.path).unwrap();
        fs::create_dir(&oldest.path).unwrap();

        let error = coordinator
            .create_backup(TargetKind::Workbuddy, &path, b"[{\"id\":\"latest\"}]\n")
            .unwrap_err();
        let backups = fixture
            .store
            .list_backups(Some(TargetKind::Workbuddy))
            .unwrap();

        assert!(matches!(error, CoreError::Storage(_)));
        assert!(backups.iter().any(|backup| backup.id == oldest.id));
    }

    #[test]
    fn removes_backup_file_when_database_record_fails() {
        let fixture = Fixture::new();
        let path = fixture.path(TargetKind::Workbuddy);
        fixture
            .store
            .execute_test_sql(
                r#"
                CREATE TRIGGER fail_backup_record
                BEFORE INSERT ON backups
                BEGIN
                    SELECT RAISE(FAIL, 'injected backup failure');
                END;
                "#,
            )
            .unwrap();

        let error = fixture
            .coordinator()
            .create_backup(TargetKind::Workbuddy, &path, b"[]\n")
            .unwrap_err();

        assert!(matches!(error, CoreError::Storage(_)));
        assert_eq!(
            fs::read_dir(fixture.directory.path().join("backups/workbuddy"))
                .unwrap()
                .count(),
            0
        );
        assert!(fixture.store.list_backups(None).unwrap().is_empty());
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
        assert!(!fixture.store.has_gateway_source_history().unwrap());
    }
}
