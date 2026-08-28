use std::{
    collections::{HashMap, HashSet},
    path::Path,
    sync::{Arc, Barrier, Mutex},
    thread,
};

use serde_json::json;
use tempfile::tempdir;

use crate::{
    gateway_service::{gateway_source_hash, source_identity_key, GatewayService},
    models::{CapabilitySet, GatewayProfile, ManagedModel, TargetKind},
    secrets::{SecretStore, MISSING_SECRET_MESSAGE},
    store::Store,
    target_import::{get_target_model_states, TargetImportService},
};

#[derive(Default)]
struct MemorySecretStore {
    values: Mutex<HashMap<String, String>>,
}

impl SecretStore for MemorySecretStore {
    fn set(&self, key: &str, secret: &str) -> crate::error::CoreResult<()> {
        self.values
            .lock()
            .unwrap()
            .insert(key.to_string(), secret.to_string());
        Ok(())
    }

    fn get(&self, key: &str) -> crate::error::CoreResult<String> {
        self.values
            .lock()
            .unwrap()
            .get(key)
            .cloned()
            .ok_or_else(|| crate::error::CoreError::SecretStore(MISSING_SECRET_MESSAGE.to_string()))
    }

    fn delete(&self, key: &str) -> crate::error::CoreResult<()> {
        self.values.lock().unwrap().remove(key);
        Ok(())
    }
}

fn target_paths(directory: &Path) -> HashMap<TargetKind, String> {
    HashMap::from([
        (
            TargetKind::Workbuddy,
            directory
                .join("workbuddy-models.json")
                .to_string_lossy()
                .to_string(),
        ),
        (
            TargetKind::Codebuddy,
            directory
                .join("codebuddy-models.json")
                .to_string_lossy()
                .to_string(),
        ),
    ])
}

#[test]
fn concurrent_bootstrap_imports_one_gateway() {
    let directory = tempdir().unwrap();
    let paths = Arc::new(target_paths(directory.path()));
    std::fs::write(
        paths.get(&TargetKind::Workbuddy).unwrap(),
        r#"[{"id":"shared","url":"https://gateway.example/v1","apiKey":"shared-secret","useCustomProtocol":false}]"#,
    )
    .unwrap();
    let database_path = directory.path().join("everybuddy.db");
    let first_store = Store::open(&database_path).unwrap();
    let second_store = Store::open(&database_path).unwrap();
    let secrets = Arc::new(MemorySecretStore::default());
    let barrier = Arc::new(Barrier::new(2));

    let handles: Vec<_> = [first_store, second_store]
        .into_iter()
        .map(|store| {
            let paths = Arc::clone(&paths);
            let secrets = Arc::clone(&secrets);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                TargetImportService::new(&store, secrets, &paths).bootstrap_import()
            })
        })
        .collect();

    let results: Vec<_> = handles
        .into_iter()
        .map(|handle| handle.join().unwrap().unwrap())
        .collect();
    let stored = Store::open(&database_path).unwrap();

    assert_eq!(stored.list_gateways().unwrap().len(), 1);
    assert_eq!(stored.list_models().unwrap().len(), 1);
    assert_eq!(
        results
            .iter()
            .map(|result| result.report.imported_gateway_count)
            .sum::<usize>(),
        1
    );
}

#[test]
fn cleans_up_imported_credential_when_a_later_target_path_is_missing() {
    let directory = tempdir().unwrap();
    let workbuddy_path = directory.path().join("workbuddy-models.json");
    std::fs::write(
        &workbuddy_path,
        r#"[{"id":"shared","url":"https://gateway.example/v1","apiKey":"shared-secret","useCustomProtocol":false}]"#,
    )
    .unwrap();
    let paths = HashMap::from([(
        TargetKind::Workbuddy,
        workbuddy_path.to_string_lossy().to_string(),
    )]);
    let store = Store::open(&directory.path().join("everybuddy.db")).unwrap();
    let secrets = Arc::new(MemorySecretStore::default());

    let error = TargetImportService::new(&store, secrets.clone(), &paths)
        .bootstrap_import()
        .unwrap_err();
    let stored_keys: Vec<_> = secrets.values.lock().unwrap().keys().cloned().collect();

    assert!(error
        .to_string()
        .contains("CodeBuddy path is not configured"));
    assert_eq!(
        stored_keys,
        vec!["__everybuddy_source_identity_key_v1".to_string()]
    );
    assert!(store.list_gateways().unwrap().is_empty());
}

#[test]
fn reports_credential_cleanup_failure_without_exposing_secrets() {
    struct FailingDeleteStore {
        calls: Mutex<usize>,
    }

    impl SecretStore for FailingDeleteStore {
        fn set(&self, _key: &str, _secret: &str) -> crate::error::CoreResult<()> {
            unreachable!()
        }

        fn get(&self, _key: &str) -> crate::error::CoreResult<String> {
            unreachable!()
        }

        fn delete(&self, _key: &str) -> crate::error::CoreResult<()> {
            *self.calls.lock().unwrap() += 1;
            Err(crate::error::CoreError::SecretStore(
                "injected delete failure".to_string(),
            ))
        }
    }

    let secrets = FailingDeleteStore {
        calls: Mutex::new(0),
    };
    let error = super::cleanup_import_credentials(
        &secrets,
        &["secret-ref-a".to_string(), "secret-ref-b".to_string()],
        crate::error::CoreError::Storage("injected import failure".to_string()),
    );

    assert_eq!(*secrets.calls.lock().unwrap(), 2);
    assert!(error.to_string().contains("credential cleanup also failed"));
    assert!(!error.to_string().contains("secret-ref"));
}

#[test]
fn imports_target_models_without_exposing_the_token() {
    let directory = tempdir().unwrap();
    let workbuddy_path = directory.path().join("workbuddy-models.json");
    let database_path = directory.path().join("everybuddy.db");
    std::fs::write(
            &workbuddy_path,
            r#"[{"id":"gpt-5.6","name":"GPT-5.6","vendor":"openai","url":"https://gateway.example/v1","apiKey":"target-secret","supportsToolCall":true,"supportsImages":true,"supportsReasoning":true,"maxInputTokens":200000,"reasoning":{"supportedEfforts":["low","high"],"defaultEffort":"high","canDisableThinking":false},"useCustomProtocol":false}]"#,
        )
        .unwrap();
    let store = Store::open(&database_path).unwrap();
    let secrets = Arc::new(MemorySecretStore::default());
    let paths = HashMap::from([
        (
            TargetKind::Workbuddy,
            workbuddy_path.to_string_lossy().to_string(),
        ),
        (
            TargetKind::Codebuddy,
            directory
                .path()
                .join("missing-codebuddy.json")
                .to_string_lossy()
                .to_string(),
        ),
    ]);

    let result = TargetImportService::new(&store, secrets, &paths)
        .bootstrap_import()
        .unwrap();

    assert_eq!(result.report.imported_gateway_count, 1);
    assert_eq!(result.report.imported_model_count, 1);
    let gateways = store.list_gateways().unwrap();
    let models = store.list_models().unwrap();
    assert_eq!(gateways.len(), 1);
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].configuration.max_input_tokens, Some(200_000));
    assert_eq!(models[0].configuration.reasoning.supported_efforts.len(), 2);
    assert_eq!(models[0].metadata["everybuddySource"], "targetImport");
    assert_eq!(
        models[0].metadata["everybuddyIdentityOverride"],
        json!({"name": "GPT-5.6", "vendor": "openai"})
    );
    assert!(!models[0].metadata.to_string().contains("target-secret"));
    for path in [
        database_path.clone(),
        database_path.with_extension("db-wal"),
    ] {
        if path.exists() {
            assert!(
                !String::from_utf8_lossy(&std::fs::read(path).unwrap()).contains("target-secret")
            );
        }
    }
    assert_eq!(
        result.states[0].matched_model_keys,
        vec![models[0].key.clone()]
    );
    assert!(!serde_json::to_string(&result)
        .unwrap()
        .contains("target-secret"));
}

#[test]
fn repeated_import_is_idempotent_and_workbuddy_wins_target_conflicts() {
    let directory = tempdir().unwrap();
    let paths = target_paths(directory.path());
    std::fs::write(
            paths.get(&TargetKind::Workbuddy).unwrap(),
            r#"[{"id":"shared","name":"Work baseline","url":"https://gateway.example/v1","apiKey":"shared-secret","supportsToolCall":true,"useCustomProtocol":false}]"#,
        )
        .unwrap();
    std::fs::write(
            paths.get(&TargetKind::Codebuddy).unwrap(),
            r#"{"models":[{"id":"shared","name":"Code variant","url":"https://gateway.example/v1","apiKey":"shared-secret","supportsToolCall":false,"useCustomProtocol":false}],"keep":true}"#,
        )
        .unwrap();
    let store = Store::open(&directory.path().join("everybuddy.db")).unwrap();
    let secrets = Arc::new(MemorySecretStore::default());

    let first = TargetImportService::new(&store, secrets.clone(), &paths)
        .bootstrap_import()
        .unwrap();
    let second = TargetImportService::new(&store, secrets, &paths)
        .bootstrap_import()
        .unwrap();

    assert_eq!(first.report.imported_gateway_count, 1);
    assert_eq!(first.report.imported_model_count, 1);
    assert!(first
        .report
        .issues
        .iter()
        .any(|item| item.target == TargetKind::Codebuddy && item.code == "targetConflict"));
    assert_eq!(store.list_models().unwrap()[0].name, "Work baseline");
    assert_eq!(second.report.imported_gateway_count, 0);
    assert_eq!(second.report.imported_model_count, 0);
    assert_eq!(
        second.states[0].matched_model_keys,
        second.states[1].matched_model_keys
    );
}

#[test]
fn restart_reconciles_models_removed_from_target_configuration() {
    let directory = tempdir().unwrap();
    let paths = target_paths(directory.path());
    let database_path = directory.path().join("everybuddy.db");
    std::fs::write(
        paths.get(&TargetKind::Workbuddy).unwrap(),
        r#"[{"id":"configured-model","url":"https://gateway.example/v1","apiKey":"shared-secret","useCustomProtocol":false}]"#,
    )
    .unwrap();
    let secrets = Arc::new(MemorySecretStore::default());

    {
        let store = Store::open(&database_path).unwrap();
        let first = TargetImportService::new(&store, secrets.clone(), &paths)
            .bootstrap_import()
            .unwrap();
        assert_eq!(first.states[0].matched_model_keys.len(), 1);
    }

    std::fs::write(paths.get(&TargetKind::Workbuddy).unwrap(), "[]").unwrap();
    let store = Store::open(&database_path).unwrap();
    let restarted = TargetImportService::new(&store, secrets, &paths)
        .bootstrap_import()
        .unwrap();

    assert!(restarted.states[0].matched_model_keys.is_empty());
    assert_eq!(store.list_models().unwrap().len(), 1);
}

#[test]
fn existing_gateway_only_matches_models_without_importing_missing_ones() {
    let directory = tempdir().unwrap();
    let paths = target_paths(directory.path());
    std::fs::write(
            paths.get(&TargetKind::Workbuddy).unwrap(),
            r#"[{"id":"target-only","url":"https://gateway.example/v1","apiKey":"shared-secret","useCustomProtocol":false}]"#,
        )
        .unwrap();
    let store = Store::open(&directory.path().join("everybuddy.db")).unwrap();
    let profile = GatewayProfile {
        id: "existing-gateway".to_string(),
        name: "Existing".to_string(),
        api_root: "https://gateway.example/v1".to_string(),
        token_ref: "existing-gateway".to_string(),
        created_at: "2026-08-20T00:00:00Z".to_string(),
        updated_at: "2026-08-20T00:00:00Z".to_string(),
    };
    store.save_gateway(&profile).unwrap();
    let secrets = Arc::new(MemorySecretStore::default());
    secrets.set(&profile.token_ref, "shared-secret").unwrap();

    let result = TargetImportService::new(&store, secrets, &paths)
        .bootstrap_import()
        .unwrap();

    assert_eq!(result.report.imported_gateway_count, 0);
    assert_eq!(result.report.imported_model_count, 0);
    assert!(store.list_models().unwrap().is_empty());
    assert!(result.states[0].matched_model_keys.is_empty());
    assert_eq!(result.states[0].unmatched_count, 1);
}

#[test]
fn deleted_gateway_source_is_not_recreated_during_bootstrap_import() {
    let directory = tempdir().unwrap();
    let paths = target_paths(directory.path());
    std::fs::write(
        paths.get(&TargetKind::Workbuddy).unwrap(),
        r#"[{"id":"shared","url":"https://gateway.example/v1","apiKey":"shared-secret","useCustomProtocol":false}]"#,
    )
    .unwrap();
    let store = Store::open(&directory.path().join("everybuddy.db")).unwrap();
    let secrets = Arc::new(MemorySecretStore::default());
    TargetImportService::new(&store, secrets.clone(), &paths)
        .bootstrap_import()
        .unwrap();
    let gateway_id = store.list_gateways().unwrap()[0].id.clone();

    GatewayService::new(&store, secrets.clone())
        .delete(&gateway_id)
        .unwrap();
    let result = TargetImportService::new(&store, secrets, &paths)
        .bootstrap_import()
        .unwrap();

    assert_eq!(result.report.imported_gateway_count, 0);
    assert_eq!(result.report.imported_model_count, 0);
    assert!(store.list_gateways().unwrap().is_empty());
    assert!(store.list_models().unwrap().is_empty());
}

#[test]
fn deleted_gateway_with_a_missing_credential_uses_its_stored_tombstone() {
    let directory = tempdir().unwrap();
    let paths = target_paths(directory.path());
    std::fs::write(
        paths.get(&TargetKind::Workbuddy).unwrap(),
        r#"[{"id":"shared","url":"https://gateway.example/v1","apiKey":"shared-secret","useCustomProtocol":false}]"#,
    )
    .unwrap();
    let store = Store::open(&directory.path().join("everybuddy.db")).unwrap();
    let secrets = Arc::new(MemorySecretStore::default());
    TargetImportService::new(&store, secrets.clone(), &paths)
        .bootstrap_import()
        .unwrap();
    let gateway_id = store.list_gateways().unwrap()[0].id.clone();
    secrets.delete(&gateway_id).unwrap();

    GatewayService::new(&store, secrets.clone())
        .delete(&gateway_id)
        .unwrap();
    let result = TargetImportService::new(&store, secrets, &paths)
        .bootstrap_import()
        .unwrap();

    assert_eq!(result.report.imported_gateway_count, 0);
    assert!(store.list_gateways().unwrap().is_empty());
}

#[test]
fn deleted_gateway_tombstones_published_endpoint_overrides() {
    let directory = tempdir().unwrap();
    let paths = target_paths(directory.path());
    std::fs::write(
        paths.get(&TargetKind::Workbuddy).unwrap(),
        r#"[{"id":"shared","url":"https://override.example/v1","apiKey":"shared-secret","useCustomProtocol":false}]"#,
    )
    .unwrap();
    let store = Store::open(&directory.path().join("everybuddy.db")).unwrap();
    let secrets = Arc::new(MemorySecretStore::default());
    let profile = GatewayProfile {
        id: "gateway".to_string(),
        name: "Gateway".to_string(),
        api_root: "https://gateway.example/v1".to_string(),
        token_ref: "gateway".to_string(),
        created_at: "2026-08-20T00:00:00Z".to_string(),
        updated_at: "2026-08-20T00:00:00Z".to_string(),
    };
    GatewayService::new(&store, secrets.clone())
        .save(&profile, "shared-secret")
        .unwrap();
    let identity_key = source_identity_key(
        secrets.as_ref(),
        store.has_gateway_source_history().unwrap(),
    )
    .unwrap();
    store
        .record_gateway_source_identities(
            &profile.id,
            &[gateway_source_hash(
                &identity_key,
                "https://override.example/v1",
                "shared-secret",
            )],
        )
        .unwrap();
    secrets.delete(&profile.token_ref).unwrap();

    GatewayService::new(&store, secrets.clone())
        .delete(&profile.id)
        .unwrap();
    let result = TargetImportService::new(&store, secrets, &paths)
        .bootstrap_import()
        .unwrap();

    assert_eq!(result.report.imported_gateway_count, 0);
    assert!(store.list_gateways().unwrap().is_empty());
}

#[test]
fn rotating_a_gateway_credential_tombstones_the_previous_source() {
    let directory = tempdir().unwrap();
    let paths = target_paths(directory.path());
    std::fs::write(
        paths.get(&TargetKind::Workbuddy).unwrap(),
        r#"[{"id":"shared","url":"https://gateway.example/v1","apiKey":"old-secret","useCustomProtocol":false}]"#,
    )
    .unwrap();
    let store = Store::open(&directory.path().join("everybuddy.db")).unwrap();
    let secrets = Arc::new(MemorySecretStore::default());
    let mut profile = GatewayProfile {
        id: "gateway".to_string(),
        name: "Gateway".to_string(),
        api_root: "https://gateway.example/v1".to_string(),
        token_ref: "gateway".to_string(),
        created_at: "2026-08-20T00:00:00Z".to_string(),
        updated_at: "2026-08-20T00:00:00Z".to_string(),
    };
    let service = GatewayService::new(&store, secrets.clone());
    service.save(&profile, "old-secret").unwrap();
    profile.updated_at = "2026-08-21T00:00:00Z".to_string();
    service.save(&profile, "new-secret").unwrap();

    let result = TargetImportService::new(&store, secrets, &paths)
        .bootstrap_import()
        .unwrap();

    assert_eq!(result.report.imported_gateway_count, 0);
    assert_eq!(store.list_gateways().unwrap().len(), 1);
}

#[test]
fn missing_source_identity_key_does_not_silently_reset_tombstones() {
    let directory = tempdir().unwrap();
    let paths = target_paths(directory.path());
    std::fs::write(
        paths.get(&TargetKind::Workbuddy).unwrap(),
        r#"[{"id":"shared","url":"https://gateway.example/v1","apiKey":"shared-secret","useCustomProtocol":false}]"#,
    )
    .unwrap();
    let store = Store::open(&directory.path().join("everybuddy.db")).unwrap();
    let secrets = Arc::new(MemorySecretStore::default());
    TargetImportService::new(&store, secrets.clone(), &paths)
        .bootstrap_import()
        .unwrap();
    secrets
        .delete("__everybuddy_source_identity_key_v1")
        .unwrap();

    let error = TargetImportService::new(&store, secrets, &paths)
        .bootstrap_import()
        .unwrap_err();

    assert!(error.to_string().contains("source identity key is missing"));
}

#[test]
fn new_gateway_imports_all_models_from_the_same_api_source() {
    let directory = tempdir().unwrap();
    let paths = target_paths(directory.path());
    std::fs::write(
            paths.get(&TargetKind::Workbuddy).unwrap(),
            r#"[
              {"id":"model-a","url":"https://gateway.example/v1","apiKey":"shared-secret","useCustomProtocol":false},
              {"id":"model-b","url":"https://gateway.example/v1","apiKey":"shared-secret","useCustomProtocol":false}
            ]"#,
        )
        .unwrap();
    let store = Store::open(&directory.path().join("everybuddy.db")).unwrap();
    let secrets = Arc::new(MemorySecretStore::default());

    let result = TargetImportService::new(&store, secrets, &paths)
        .bootstrap_import()
        .unwrap();

    assert_eq!(result.report.imported_gateway_count, 1);
    assert_eq!(result.report.imported_model_count, 2);
    assert_eq!(store.list_models().unwrap().len(), 2);
    assert_eq!(result.states[0].matched_model_keys.len(), 2);
}

#[test]
fn keeps_same_model_id_isolated_by_endpoint_and_token() {
    let directory = tempdir().unwrap();
    let paths = target_paths(directory.path());
    std::fs::write(
            paths.get(&TargetKind::Workbuddy).unwrap(),
            r#"[{"id":"shared","url":"https://gateway.example/v1","apiKey":"work-secret","useCustomProtocol":false}]"#,
        )
        .unwrap();
    std::fs::write(
            paths.get(&TargetKind::Codebuddy).unwrap(),
            r#"[{"id":"shared","url":"https://gateway.example/v1","apiKey":"code-secret","useCustomProtocol":false}]"#,
        )
        .unwrap();
    let store = Store::open(&directory.path().join("everybuddy.db")).unwrap();
    let secrets = Arc::new(MemorySecretStore::default());

    let result = TargetImportService::new(&store, secrets, &paths)
        .bootstrap_import()
        .unwrap();

    assert_eq!(result.report.imported_gateway_count, 2);
    assert_eq!(result.report.imported_model_count, 2);
    assert_eq!(store.list_models().unwrap().len(), 2);
    assert_ne!(
        result.states[0].matched_model_keys,
        result.states[1].matched_model_keys
    );
}

#[test]
fn skips_unsupported_or_incomplete_target_entries_with_structured_issues() {
    let directory = tempdir().unwrap();
    let paths = target_paths(directory.path());
    std::fs::write(
            paths.get(&TargetKind::Workbuddy).unwrap(),
            r#"[
              {"id":"custom","url":"https://gateway.example/v1","apiKey":"secret","useCustomProtocol":true},
              {"id":"missing-token","url":"https://gateway.example/v1","useCustomProtocol":false},
              {"id":"invalid-parameters","url":"https://gateway.example/v1","apiKey":"secret","maxInputTokens":"many","useCustomProtocol":false},
              {"id":"leaked-token","url":"https://gateway.example/v1","apiKey":"target-secret-value","note":"Bearer target-secret-value","useCustomProtocol":false}
            ]"#,
        )
        .unwrap();
    let store = Store::open(&directory.path().join("everybuddy.db")).unwrap();
    let secrets = Arc::new(MemorySecretStore::default());

    let result = TargetImportService::new(&store, secrets, &paths)
        .bootstrap_import()
        .unwrap();

    let codes: HashSet<_> = result
        .report
        .issues
        .iter()
        .map(|item| item.code.as_str())
        .collect();
    assert!(codes.contains("customProtocol"));
    assert!(codes.contains("missingToken"));
    assert!(codes.contains("invalidParameters"));
    assert_eq!(result.states[0].unmatched_count, 1);
    assert_eq!(result.states[0].skipped_count, 3);
    assert!(store.list_gateways().unwrap().is_empty());
}

#[test]
fn rejects_unsafe_numeric_values_from_target_configuration() {
    let directory = tempdir().unwrap();
    let paths = target_paths(directory.path());
    std::fs::write(
        paths.get(&TargetKind::Workbuddy).unwrap(),
        r#"[
          {"id":"zero-limit","url":"https://gateway.example/v1","apiKey":"secret","maxInputTokens":0},
          {"id":"unsafe-limit","url":"https://gateway.example/v1","apiKey":"secret","maxOutputTokens":9007199254740992},
          {"id":"negative-temperature","url":"https://gateway.example/v1","apiKey":"secret","temperature":-0.1}
        ]"#,
    )
    .unwrap();
    let store = Store::open(&directory.path().join("everybuddy.db")).unwrap();
    let secrets = Arc::new(MemorySecretStore::default());

    let result = TargetImportService::new(&store, secrets, &paths)
        .bootstrap_import()
        .unwrap();

    assert_eq!(result.report.issues.len(), 3);
    assert!(result
        .report
        .issues
        .iter()
        .all(|issue| issue.code == "invalidParameters"));
    assert!(store.list_gateways().unwrap().is_empty());
    assert!(store.list_models().unwrap().is_empty());
}

#[test]
fn matches_existing_custom_protocol_models_without_importing_them() {
    let directory = tempdir().unwrap();
    let paths = target_paths(directory.path());
    std::fs::write(
        paths.get(&TargetKind::Workbuddy).unwrap(),
        r#"[{"id":"custom","url":"https://gateway.example/v1/images/generations","apiKey":"secret","useCustomProtocol":true}]"#,
    )
    .unwrap();
    let store = Store::open(&directory.path().join("everybuddy.db")).unwrap();
    let profile = GatewayProfile {
        id: "gateway".to_string(),
        name: "Gateway".to_string(),
        api_root: "https://gateway.example/v1".to_string(),
        token_ref: "gateway".to_string(),
        created_at: "2026-08-20T00:00:00Z".to_string(),
        updated_at: "2026-08-20T00:00:00Z".to_string(),
    };
    store.save_gateway(&profile).unwrap();
    let configuration = crate::models::ModelConfiguration {
        endpoint_override: Some("https://gateway.example/v1/images/generations".to_string()),
        use_custom_protocol: true,
        ..Default::default()
    };
    store
        .save_model(&ManagedModel {
            key: "gateway::custom".to_string(),
            gateway_id: profile.id.clone(),
            id: "custom".to_string(),
            name: "Custom".to_string(),
            vendor: "custom".to_string(),
            capabilities: CapabilitySet::default(),
            configuration,
            evidence: Vec::new(),
            metadata: json!({"everybuddySource": "manual"}),
            updated_at: "2026-08-20T00:00:00Z".to_string(),
        })
        .unwrap();
    let secrets = Arc::new(MemorySecretStore::default());
    secrets.set(&profile.token_ref, "secret").unwrap();

    let states = get_target_model_states(&store, secrets, &paths).unwrap();

    assert_eq!(states[0].matched_model_keys, vec!["gateway::custom"]);
    assert_eq!(states[0].skipped_count, 0);
}

#[test]
fn does_not_match_a_custom_protocol_target_to_a_standard_model() {
    let directory = tempdir().unwrap();
    let paths = target_paths(directory.path());
    std::fs::write(
        paths.get(&TargetKind::Workbuddy).unwrap(),
        r#"[{"id":"custom","url":"https://gateway.example/v1","apiKey":"secret","useCustomProtocol":true}]"#,
    )
    .unwrap();
    let store = Store::open(&directory.path().join("everybuddy.db")).unwrap();
    let profile = GatewayProfile {
        id: "gateway".to_string(),
        name: "Gateway".to_string(),
        api_root: "https://gateway.example/v1".to_string(),
        token_ref: "gateway".to_string(),
        created_at: "2026-08-20T00:00:00Z".to_string(),
        updated_at: "2026-08-20T00:00:00Z".to_string(),
    };
    store.save_gateway(&profile).unwrap();
    store
        .save_model(&ManagedModel {
            key: "gateway::custom".to_string(),
            gateway_id: profile.id.clone(),
            id: "custom".to_string(),
            name: "Custom".to_string(),
            vendor: "custom".to_string(),
            capabilities: CapabilitySet::default(),
            configuration: Default::default(),
            evidence: Vec::new(),
            metadata: json!({"everybuddySource": "manual"}),
            updated_at: "2026-08-20T00:00:00Z".to_string(),
        })
        .unwrap();
    let secrets = Arc::new(MemorySecretStore::default());
    secrets.set(&profile.token_ref, "secret").unwrap();

    let states = get_target_model_states(&store, secrets, &paths).unwrap();

    assert!(states[0].matched_model_keys.is_empty());
    assert_eq!(states[0].unmatched_count, 1);
}

#[test]
fn repairs_a_unique_gateway_with_a_missing_credential_without_overwriting_the_model() {
    let directory = tempdir().unwrap();
    let paths = target_paths(directory.path());
    std::fs::write(
            paths.get(&TargetKind::Workbuddy).unwrap(),
            r#"[{"id":"existing","name":"Target name","url":"https://gateway.example/v1","apiKey":"recovered-secret","useCustomProtocol":false}]"#,
        )
        .unwrap();
    let store = Store::open(&directory.path().join("everybuddy.db")).unwrap();
    let profile = GatewayProfile {
        id: "existing-gateway".to_string(),
        name: "Existing".to_string(),
        api_root: "https://gateway.example/v1".to_string(),
        token_ref: "existing-gateway".to_string(),
        created_at: "2026-08-20T00:00:00Z".to_string(),
        updated_at: "2026-08-20T00:00:00Z".to_string(),
    };
    store.save_gateway(&profile).unwrap();
    store
        .save_model(&ManagedModel {
            key: "existing-gateway::existing".to_string(),
            gateway_id: profile.id.clone(),
            id: "existing".to_string(),
            name: "Keep local name".to_string(),
            vendor: "custom".to_string(),
            capabilities: CapabilitySet::default(),
            configuration: Default::default(),
            evidence: Vec::new(),
            metadata: json!({"everybuddySource": "manual"}),
            updated_at: "2026-08-20T00:00:00Z".to_string(),
        })
        .unwrap();
    let secrets = Arc::new(MemorySecretStore::default());

    let result = TargetImportService::new(&store, secrets.clone(), &paths)
        .bootstrap_import()
        .unwrap();

    assert_eq!(secrets.get("existing-gateway").unwrap(), "recovered-secret");
    assert_eq!(result.report.imported_gateway_count, 0);
    assert_eq!(result.report.imported_model_count, 0);
    assert_eq!(store.list_models().unwrap()[0].name, "Keep local name");
    assert_eq!(
        result.states[0].matched_model_keys,
        vec!["existing-gateway::existing"]
    );
}

#[test]
fn reports_ambiguous_gateways_without_importing_or_repairing_credentials() {
    let directory = tempdir().unwrap();
    let paths = target_paths(directory.path());
    std::fs::write(
            paths.get(&TargetKind::Workbuddy).unwrap(),
            r#"[{"id":"ambiguous","url":"https://gateway.example/v1","apiKey":"target-secret","useCustomProtocol":false}]"#,
        )
        .unwrap();
    let store = Store::open(&directory.path().join("everybuddy.db")).unwrap();
    for id in ["gateway-a", "gateway-b"] {
        store
            .save_gateway(&GatewayProfile {
                id: id.to_string(),
                name: id.to_string(),
                api_root: "https://gateway.example/v1".to_string(),
                token_ref: id.to_string(),
                created_at: "2026-08-20T00:00:00Z".to_string(),
                updated_at: "2026-08-20T00:00:00Z".to_string(),
            })
            .unwrap();
    }
    let secrets = Arc::new(MemorySecretStore::default());

    let result = TargetImportService::new(&store, secrets.clone(), &paths)
        .bootstrap_import()
        .unwrap();

    assert!(result
        .report
        .issues
        .iter()
        .any(|item| item.code == "ambiguousGateway"));
    assert!(secrets.get("gateway-a").is_err());
    assert!(secrets.get("gateway-b").is_err());
    assert_eq!(result.states[0].unmatched_count, 1);
    assert!(store.list_models().unwrap().is_empty());
}

#[test]
fn reports_damaged_target_json_without_blocking_other_targets() {
    let directory = tempdir().unwrap();
    let paths = target_paths(directory.path());
    std::fs::write(paths.get(&TargetKind::Workbuddy).unwrap(), "{not-json").unwrap();
    std::fs::write(
            paths.get(&TargetKind::Codebuddy).unwrap(),
            r#"[{"id":"valid","url":"https://gateway.example/v1","apiKey":"secret","useCustomProtocol":false}]"#,
        )
        .unwrap();
    let store = Store::open(&directory.path().join("everybuddy.db")).unwrap();
    let secrets = Arc::new(MemorySecretStore::default());

    let result = TargetImportService::new(&store, secrets, &paths)
        .bootstrap_import()
        .unwrap();

    assert!(result
        .report
        .issues
        .iter()
        .any(|item| { item.target == TargetKind::Workbuddy && item.code == "targetReadFailed" }));
    assert_eq!(result.report.imported_model_count, 1);
    assert_eq!(result.states[0].skipped_count, 1);
    assert_eq!(result.states[1].matched_model_keys.len(), 1);
}
