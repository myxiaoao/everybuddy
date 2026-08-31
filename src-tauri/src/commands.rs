use std::{collections::HashSet, sync::MutexGuard};

use chrono::Utc;
use tauri::State;
use uuid::Uuid;

use crate::{
    error::CommandError,
    gateway::normalize_api_root,
    gateway_service::GatewayService,
    model_lifecycle::ModelLifecycle,
    models::{
        AppSettings, BackupRecord, BootstrapData, ExecutePublishRequest, GatewayInput,
        GatewayProfile, ManagedModel, ManualModelInput, ModelUpdateInput, PreparePublishRequest,
        ProbeSummary, PublishPreview, PublishResult, SaveGatewayResult, SaveSettingsInput,
        TargetKind, TargetSnapshot,
    },
    publish::PublishCoordinator,
    target::{default_target_paths, target_path, target_write_path},
    target_import::{get_target_snapshot as read_target_snapshot, TargetImportService},
    AppState,
};

type CommandResult<T> = Result<T, CommandError>;

#[tauri::command]
pub fn bootstrap(state: State<'_, AppState>) -> CommandResult<BootstrapData> {
    let _mutation = lock_app_mutation(state.inner())?;
    let settings = state
        .store
        .settings(default_target_paths().map_err(CommandError::from)?)
        .map_err(CommandError::from)?;
    let import = TargetImportService::new(&state.store, &settings.target_paths)
        .bootstrap_import()
        .map_err(CommandError::from)?;
    Ok(BootstrapData {
        gateways: state.store.list_gateways().map_err(CommandError::from)?,
        models: state.store.list_models().map_err(CommandError::from)?,
        targets: import.targets,
        target_model_states: import.states,
        import_report: import.report,
        settings,
    })
}

#[tauri::command]
pub fn get_gateway_token(id: String, state: State<'_, AppState>) -> CommandResult<Option<String>> {
    state.store.gateway(&id).map_err(CommandError::from)?;
    state
        .store
        .optional_gateway_token(&id)
        .map_err(CommandError::from)
}

#[tauri::command]
pub fn save_gateway(
    input: GatewayInput,
    state: State<'_, AppState>,
) -> CommandResult<SaveGatewayResult> {
    let _mutation = lock_app_mutation(state.inner())?;
    let name = input.name.trim();
    if name.is_empty() {
        return Err(
            crate::error::CoreError::Validation("Gateway name is required".to_string()).into(),
        );
    }
    let api_root = normalize_api_root(&input.base_url).map_err(CommandError::from)?;
    let now = Utc::now().to_rfc3339();
    let existing = input
        .id
        .as_deref()
        .map(|id| state.store.gateway(id))
        .transpose()
        .map_err(CommandError::from)?;
    let id = input.id.unwrap_or_else(|| Uuid::new_v4().to_string());
    let replacement_token = input.token.as_deref();
    let profile = GatewayProfile {
        id: id.clone(),
        name: name.to_string(),
        api_root,
        created_at: existing
            .map(|profile| profile.created_at)
            .unwrap_or_else(|| now.clone()),
        updated_at: now,
    };

    let models_invalidated = GatewayService::new(&state.store)
        .save_optional(&profile, replacement_token)
        .map_err(CommandError::from)?;
    Ok(SaveGatewayResult {
        profile,
        models_invalidated,
    })
}

#[tauri::command]
pub fn delete_gateway(id: String, state: State<'_, AppState>) -> CommandResult<()> {
    let _mutation = lock_app_mutation(state.inner())?;
    GatewayService::new(&state.store)
        .delete(&id)
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn discover_models(
    gateway_id: String,
    state: State<'_, AppState>,
) -> CommandResult<Vec<ManagedModel>> {
    model_lifecycle(state.inner())
        .discover(gateway_id)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn add_manual_model(
    input: ManualModelInput,
    state: State<'_, AppState>,
) -> CommandResult<ManagedModel> {
    model_lifecycle(state.inner())
        .add_manual(input)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn probe_model(
    model_key: String,
    state: State<'_, AppState>,
) -> CommandResult<ProbeSummary> {
    model_lifecycle(state.inner())
        .probe(model_key)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn apply_openrouter_model(
    model_key: String,
    state: State<'_, AppState>,
) -> CommandResult<ManagedModel> {
    model_lifecycle(state.inner())
        .apply_openrouter(model_key)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn get_openrouter_model_match(
    model_key: String,
    state: State<'_, AppState>,
) -> CommandResult<Option<String>> {
    model_lifecycle(state.inner())
        .openrouter_match(model_key)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
pub fn update_model(
    input: ModelUpdateInput,
    state: State<'_, AppState>,
) -> CommandResult<ManagedModel> {
    model_lifecycle(state.inner())
        .update(input)
        .map_err(CommandError::from)
}

#[tauri::command]
pub fn get_target_snapshot(state: State<'_, AppState>) -> CommandResult<TargetSnapshot> {
    let settings = state
        .store
        .settings(default_target_paths().map_err(CommandError::from)?)
        .map_err(CommandError::from)?;
    read_target_snapshot(&state.store, &settings.target_paths).map_err(CommandError::from)
}

#[tauri::command]
pub fn prepare_publish(
    request: PreparePublishRequest,
    state: State<'_, AppState>,
) -> CommandResult<PublishPreview> {
    let _mutation = lock_app_mutation(state.inner())?;
    let settings = state
        .store
        .settings(default_target_paths().map_err(CommandError::from)?)
        .map_err(CommandError::from)?;
    coordinator(state.inner())
        .preview(&request, &settings.target_paths)
        .map_err(CommandError::from)
}

#[tauri::command]
pub fn execute_publish(
    request: ExecutePublishRequest,
    state: State<'_, AppState>,
) -> CommandResult<PublishResult> {
    let _mutation = lock_app_mutation(state.inner())?;
    let settings = state
        .store
        .settings(default_target_paths().map_err(CommandError::from)?)
        .map_err(CommandError::from)?;
    coordinator(state.inner())
        .execute(&request, &settings.target_paths)
        .map_err(CommandError::from)
}

#[tauri::command]
pub fn list_backups(
    target: Option<TargetKind>,
    state: State<'_, AppState>,
) -> CommandResult<Vec<BackupRecord>> {
    state.store.list_backups(target).map_err(CommandError::from)
}

#[tauri::command]
pub fn restore_backup(id: String, state: State<'_, AppState>) -> CommandResult<()> {
    let _mutation = lock_app_mutation(state.inner())?;
    coordinator(state.inner())
        .restore(&id)
        .map_err(CommandError::from)
}

#[tauri::command]
pub fn save_settings(
    input: SaveSettingsInput,
    state: State<'_, AppState>,
) -> CommandResult<AppSettings> {
    let _mutation = lock_app_mutation(state.inner())?;
    if !matches!(input.language.as_str(), "zh-CN" | "en") {
        return Err(crate::error::CoreError::Validation(
            "Unsupported interface language".to_string(),
        )
        .into());
    }
    if !matches!(input.theme.as_str(), "light" | "dark" | "system") {
        return Err(crate::error::CoreError::Validation("Unsupported theme".to_string()).into());
    }
    validate_selected_targets(&input.selected_targets).map_err(CommandError::from)?;
    if input
        .target_paths
        .values()
        .any(|path| path.trim().is_empty())
    {
        return Err(crate::error::CoreError::Validation(
            "Configuration target paths cannot be empty".to_string(),
        )
        .into());
    }
    validate_target_paths(&input.target_paths).map_err(CommandError::from)?;
    let settings = AppSettings {
        language: input.language,
        theme: input.theme,
        selected_targets: input.selected_targets,
        target_selection_initialized: input.target_selection_initialized,
        target_paths: input.target_paths,
    };
    state
        .store
        .save_settings(&settings)
        .map_err(CommandError::from)?;
    Ok(settings)
}

fn coordinator(state: &AppState) -> PublishCoordinator<'_> {
    PublishCoordinator {
        store: &state.store,
        backup_root: &state.backup_root,
    }
}

fn model_lifecycle(state: &AppState) -> ModelLifecycle<'_> {
    ModelLifecycle::new(&state.store, &state.gateway_client, &state.app_mutation)
}

fn lock_app_mutation(state: &AppState) -> CommandResult<MutexGuard<'_, ()>> {
    state.app_mutation.lock().map_err(|_| {
        CommandError::from(crate::error::CoreError::Storage(
            "Application mutation lock is unavailable".to_string(),
        ))
    })
}

fn validate_target_paths(
    paths: &std::collections::HashMap<TargetKind, String>,
) -> crate::error::CoreResult<()> {
    let workbuddy_path = target_write_path(&target_path(TargetKind::Workbuddy, paths)?)?;
    let codebuddy_path = target_write_path(&target_path(TargetKind::Codebuddy, paths)?)?;
    if workbuddy_path == codebuddy_path {
        return Err(crate::error::CoreError::Validation(
            "WorkBuddy and CodeBuddy must use different configuration files".to_string(),
        ));
    }
    Ok(())
}

fn validate_selected_targets(targets: &[TargetKind]) -> crate::error::CoreResult<()> {
    if targets.iter().collect::<HashSet<_>>().len() != targets.len() {
        return Err(crate::error::CoreError::Validation(
            "A configuration target can only be selected once".to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        capability::{evidence, CapabilityResolver},
        market_catalog::MarketModel,
        model_lifecycle::{
            apply_model_update, apply_openrouter_detail, build_manual_model,
            ensure_gateway_snapshot_unchanged, ensure_model_id_available,
            normalize_model_configuration, preserve_local_models, record_identity_override,
            replace_probe_evidence,
        },
        models::{EvidenceSource, GatewayProfile, ModelConfiguration, ModelOrigin},
    };
    use serde_json::json;

    #[test]
    fn manual_model_uses_openrouter_capabilities_and_source_marker() {
        let market_model: MarketModel = serde_json::from_value(serde_json::json!({
            "id": "openai/gpt-5.6",
            "architecture": {
                "input_modalities": ["text", "image"],
                "output_modalities": ["text"]
            },
            "supported_parameters": ["tools", "reasoning_effort"]
        }))
        .unwrap();
        let model = build_manual_model(
            "gateway-1",
            "gpt-5.6",
            "Private GPT",
            "",
            Some(&market_model),
        );

        assert_eq!(model.key, "gateway-1::gpt-5.6");
        assert_eq!(model.name, "Private GPT");
        assert_eq!(model.vendor, "openai");
        assert!(model.capabilities.supports_tool_call);
        assert!(model.capabilities.supports_images);
        assert!(model.capabilities.supports_reasoning);
        assert!(model.capabilities.reasoning_efforts.is_empty());
        assert_eq!(model.metadata["everybuddySource"], "manual");
        assert_eq!(
            model.metadata["everybuddyIdentityOverride"],
            json!({"name": "Private GPT"})
        );
    }

    #[test]
    fn manual_model_without_openrouter_match_uses_conservative_defaults() {
        let model = build_manual_model("gateway-1", "private-model", "", "", None);

        assert_eq!(model.vendor, "custom");
        assert_eq!(model.capabilities, Default::default());
        assert!(model.configuration.reasoning.supported_efforts.is_empty());
    }

    #[test]
    fn manual_model_uses_openrouter_name_and_dynamic_provider_as_fallbacks() {
        let market_model: MarketModel = serde_json::from_value(serde_json::json!({
            "id": "future-lab/new-model",
            "name": "Future Lab: New Model",
            "architecture": {
                "input_modalities": ["text"],
                "output_modalities": ["text"]
            }
        }))
        .unwrap();

        let model = build_manual_model("gateway-1", "new-model", "", "", Some(&market_model));

        assert_eq!(model.name, "Future Lab: New Model");
        assert_eq!(model.vendor, "future-lab");
    }

    #[test]
    fn refresh_preserves_only_manual_models_missing_from_discovery() {
        let manual = build_manual_model("gateway-1", "private-model", "Private", "custom", None);
        let discovered =
            build_manual_model("gateway-1", "upstream-model", "Upstream", "custom", None);
        let mut refreshed = vec![discovered.clone()];

        preserve_local_models(&mut refreshed, &[manual.clone(), discovered]);

        assert_eq!(refreshed.len(), 2);
        assert!(refreshed.iter().any(|model| model.key == manual.key));
    }

    #[test]
    fn refresh_preserves_imported_models_missing_from_discovery() {
        let mut imported =
            build_manual_model("gateway-1", "target-model", "Imported", "custom", None);
        ModelOrigin::Target.write_to_metadata(&mut imported.metadata);
        imported.evidence[0].source = crate::models::EvidenceSource::Imported;
        let mut refreshed = Vec::new();

        preserve_local_models(&mut refreshed, &[imported.clone()]);

        assert_eq!(refreshed.len(), 1);
        assert_eq!(refreshed[0].key, imported.key);
    }

    #[test]
    fn openrouter_detail_replaces_model_facts_and_preserves_local_routing() {
        let mut model = build_manual_model("gateway-1", "gpt-test", "Private GPT", "openai", None);
        model.configuration.endpoint_override =
            Some("https://gateway.example/v1/chat/completions".to_string());
        model.configuration.use_custom_protocol = true;
        CapabilityResolver::apply_manual(
            &mut model,
            crate::models::CapabilitySet {
                supports_tool_call: false,
                supports_images: false,
                supports_reasoning: false,
                reasoning_efforts: Vec::new(),
            },
        );
        model.evidence.push(evidence(
            "images",
            false,
            EvidenceSource::Probe,
            "Old probe",
            "2026-08-20T00:00:00Z",
        ));
        let detail: MarketModel = serde_json::from_value(json!({
            "id": "openai/gpt-test",
            "context_length": 128_000,
            "architecture": {
                "input_modalities": ["text", "image"],
                "output_modalities": ["text"]
            },
            "top_provider": { "max_completion_tokens": 16_384 },
            "supported_parameters": ["tools", "reasoning_effort"]
        }))
        .unwrap();

        apply_openrouter_detail(&mut model, &detail);

        assert_eq!(model.name, "Private GPT");
        assert_eq!(model.vendor, "openai");
        assert_eq!(model.origin(), Some(ModelOrigin::Manual));
        assert!(model.capabilities.supports_tool_call);
        assert!(model.capabilities.supports_images);
        assert!(model.capabilities.supports_reasoning);
        assert_eq!(model.configuration.max_input_tokens, Some(128_000));
        assert_eq!(model.configuration.max_output_tokens, Some(16_384));
        assert_eq!(
            model.configuration.endpoint_override.as_deref(),
            Some("https://gateway.example/v1/chat/completions")
        );
        assert!(model.configuration.use_custom_protocol);
        assert_eq!(
            model.metadata["everybuddyOpenRouterMatch"]["modelId"],
            "openai/gpt-test"
        );
        assert!(model.evidence.iter().any(|item| {
            item.capability == "toolCall" && item.source == EvidenceSource::OpenRouter
        }));
        assert!(!model.evidence.iter().any(|item| {
            matches!(
                item.source,
                EvidenceSource::Manual | EvidenceSource::Probe | EvidenceSource::Imported
            )
        }));
    }

    #[test]
    fn rejects_default_effort_outside_supported_efforts() {
        let mut configuration = ModelConfiguration::default();
        configuration.reasoning.supported_efforts = vec![crate::models::ReasoningEffort::Low];
        configuration.reasoning.default_effort = Some(crate::models::ReasoningEffort::High);
        let capabilities = crate::models::CapabilitySet {
            supports_reasoning: true,
            ..Default::default()
        };

        assert!(normalize_model_configuration(configuration, &capabilities).is_err());
    }

    #[test]
    fn rejects_non_positive_token_limits_and_negative_temperature() {
        let configurations = [
            ModelConfiguration {
                max_input_tokens: Some(0),
                ..Default::default()
            },
            ModelConfiguration {
                max_output_tokens: Some(0),
                ..Default::default()
            },
            ModelConfiguration {
                temperature: Some(-0.1),
                ..Default::default()
            },
            ModelConfiguration {
                max_input_tokens: Some(9_007_199_254_740_992),
                ..Default::default()
            },
        ];

        for configuration in configurations {
            assert!(normalize_model_configuration(
                configuration,
                &crate::models::CapabilitySet::default()
            )
            .is_err());
        }
    }

    #[test]
    fn rejects_legacy_reasoning_summaries_when_saving() {
        let capabilities = crate::models::CapabilitySet {
            supports_reasoning: true,
            ..Default::default()
        };
        for summary in [
            crate::models::ReasoningSummary::Always,
            crate::models::ReasoningSummary::Never,
        ] {
            let mut configuration = ModelConfiguration::default();
            configuration.reasoning.summary = Some(summary);

            assert!(normalize_model_configuration(configuration, &capabilities).is_err());
        }
    }

    #[test]
    fn custom_protocol_preserves_the_complete_request_url() {
        assert!(normalize_model_configuration(
            ModelConfiguration {
                use_custom_protocol: true,
                ..Default::default()
            },
            &Default::default(),
        )
        .is_err());

        let mut configuration = ModelConfiguration {
            endpoint_override: Some("https://gateway.example/v1/images/generations/".to_string()),
            use_custom_protocol: true,
            ..Default::default()
        };

        configuration = normalize_model_configuration(configuration, &Default::default()).unwrap();

        assert_eq!(
            configuration.endpoint_override.as_deref(),
            Some("https://gateway.example/v1/images/generations")
        );
    }

    #[test]
    fn replacing_probe_evidence_removes_stale_positive_results() {
        let mut model = build_manual_model("gateway", "model", "Model", "custom", None);
        model.evidence.push(evidence(
            "images",
            true,
            EvidenceSource::Probe,
            "Old endpoint probe",
            "2026-08-20T00:00:00Z",
        ));

        replace_probe_evidence(&mut model, Vec::new());

        assert!(!model
            .evidence
            .iter()
            .any(|item| item.source == EvidenceSource::Probe));
    }

    #[test]
    fn non_text_models_ignore_manual_chat_configuration() {
        let mut model = build_manual_model("gateway", "image-model", "Image", "custom", None);
        model.metadata = json!({
            "architecture": { "output_modalities": ["image"] }
        });
        let capabilities = crate::models::CapabilitySet {
            supports_tool_call: true,
            supports_images: true,
            supports_reasoning: true,
            reasoning_efforts: vec!["high".to_string()],
        };
        let configuration = ModelConfiguration {
            max_input_tokens: Some(32_000),
            max_output_tokens: Some(4_000),
            temperature: Some(0.7),
            only_reasoning: true,
            ..Default::default()
        };

        apply_model_update(&mut model, capabilities, configuration, "Image", "custom");

        assert_eq!(model.capabilities, Default::default());
        assert_eq!(model.configuration, Default::default());
        assert!(!model.evidence.iter().any(|item| {
            item.source == EvidenceSource::Manual
                && matches!(
                    item.capability.as_str(),
                    "toolCall" | "images" | "reasoning"
                )
        }));
    }

    #[test]
    fn persisted_catalog_non_text_guard_survives_endpoint_changes() {
        let mut model = build_manual_model("gateway", "image-model", "Image", "custom", None);
        model.metadata["everybuddyOpenRouterMatch"] = json!({
            "source": "openrouter",
            "modelId": "provider/image-model",
            "supportsTextOutput": false
        });
        model.evidence.push(evidence(
            "images",
            true,
            EvidenceSource::Imported,
            "Old target import",
            "2026-08-20T00:00:00Z",
        ));
        let mut configuration = model.configuration.clone();
        configuration.endpoint_override = Some("https://new.example.com/v1".to_string());

        apply_model_update(
            &mut model,
            Default::default(),
            configuration,
            "Image",
            "custom",
        );

        assert_eq!(model.capabilities, Default::default());
    }

    #[test]
    fn endpoint_change_drops_probe_evidence_without_turning_old_values_into_manual_overrides() {
        let mut model = build_manual_model("gateway", "model", "Model", "custom", None);
        model
            .evidence
            .retain(|item| item.source != EvidenceSource::Manual);
        model.evidence.push(evidence(
            "images",
            true,
            EvidenceSource::Probe,
            "Old endpoint probe",
            "2026-08-20T00:00:00Z",
        ));
        model.capabilities.supports_images = true;
        model.evidence.push(evidence(
            "reasoning",
            true,
            EvidenceSource::Probe,
            "Old endpoint probe",
            "2026-08-20T00:00:00Z",
        ));
        model.capabilities.supports_reasoning = true;
        model.configuration.only_reasoning = true;
        model.configuration.reasoning.summary = Some(crate::models::ReasoningSummary::Auto);
        let mut submitted_capabilities = model.capabilities.clone();
        submitted_capabilities.supports_tool_call = true;
        let mut configuration = model.configuration.clone();
        configuration.endpoint_override = Some("https://new.example.com/v1".to_string());

        apply_model_update(
            &mut model,
            submitted_capabilities,
            configuration,
            "Model",
            "custom",
        );

        assert!(model.capabilities.supports_tool_call);
        assert!(!model.capabilities.supports_images);
        assert!(!model.capabilities.supports_reasoning);
        assert!(!model.configuration.only_reasoning);
        assert_eq!(model.configuration.reasoning, Default::default());
        assert!(!model
            .evidence
            .iter()
            .any(|item| item.source == EvidenceSource::Probe));
        assert!(!model
            .evidence
            .iter()
            .any(|item| { item.source == EvidenceSource::Manual && item.capability == "images" }));
        assert!(model.evidence.iter().any(|item| {
            item.source == EvidenceSource::Manual && item.capability == "toolCall" && item.value
        }));
    }

    #[test]
    fn records_manual_identity_overrides_for_future_refreshes() {
        let mut model = build_manual_model("gateway", "model", "Original", "custom", None);

        record_identity_override(&mut model, "Renamed", "private");

        assert_eq!(
            model.metadata["everybuddyIdentityOverride"],
            json!({"name": "Renamed", "vendor": "private"})
        );
    }

    #[test]
    fn records_only_the_identity_field_that_changed() {
        let mut model = build_manual_model("gateway", "model", "Original", "custom", None);
        model
            .metadata
            .as_object_mut()
            .unwrap()
            .remove("everybuddyIdentityOverride");

        record_identity_override(&mut model, "Renamed", "custom");

        assert_eq!(
            model.metadata["everybuddyIdentityOverride"],
            json!({"name": "Renamed"})
        );
    }

    #[test]
    fn rejects_targets_that_resolve_to_the_same_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("models.json");
        let paths = std::collections::HashMap::from([
            (TargetKind::Workbuddy, path.to_string_lossy().to_string()),
            (TargetKind::Codebuddy, path.to_string_lossy().to_string()),
        ]);

        assert!(validate_target_paths(&paths).is_err());
    }

    #[test]
    fn rejects_duplicate_target_preferences() {
        assert!(
            validate_selected_targets(&[TargetKind::Workbuddy, TargetKind::Workbuddy,]).is_err()
        );
    }

    #[test]
    fn rejects_changed_gateway_snapshot_after_a_remote_request() {
        let expected = GatewayProfile {
            id: "gateway".to_string(),
            name: "Gateway".to_string(),
            api_root: "https://api.example.com/v1".to_string(),
            created_at: "2026-08-20T00:00:00Z".to_string(),
            updated_at: "2026-08-20T00:00:00Z".to_string(),
        };
        let mut changed = expected.clone();
        changed.updated_at = "2026-08-21T00:00:00Z".to_string();

        assert!(
            ensure_gateway_snapshot_unchanged(&expected, "old-token", &changed, "old-token")
                .is_err()
        );
        assert!(
            ensure_gateway_snapshot_unchanged(&expected, "old-token", &expected, "new-token")
                .is_err()
        );
    }

    #[test]
    fn rejects_a_model_id_added_while_market_lookup_is_running() {
        let existing = build_manual_model("gateway", "duplicate", "Duplicate", "custom", None);

        assert!(ensure_model_id_available(&[existing], "duplicate").is_err());
    }

    #[test]
    fn clears_reasoning_configuration_when_reasoning_is_disabled() {
        let mut configuration = ModelConfiguration {
            only_reasoning: true,
            ..Default::default()
        };
        configuration.reasoning.default_effort = Some(crate::models::ReasoningEffort::High);

        let normalized =
            normalize_model_configuration(configuration, &crate::models::CapabilitySet::default())
                .unwrap();

        assert!(!normalized.only_reasoning);
        assert_eq!(normalized.reasoning, Default::default());
    }
}
