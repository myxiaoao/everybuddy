use std::{
    collections::HashSet,
    sync::{Arc, MutexGuard},
};

use chrono::Utc;
use serde_json::{json, Value};
use tauri::State;
use uuid::Uuid;

use crate::{
    capability::{configuration_from_sources, evidence, infer_vendor, CapabilityResolver},
    error::CommandError,
    gateway::normalize_api_root,
    gateway_service::GatewayService,
    market_catalog::MarketModel,
    models::{
        AppSettings, BackupRecord, BootstrapData, EvidenceSource, ExecutePublishRequest,
        GatewayInput, GatewayProfile, ManagedModel, ManualModelInput, ModelConfiguration,
        ModelUpdateInput, PreparePublishRequest, ProbeSummary, PublishPreview, PublishResult,
        SaveGatewayResult, SaveSettingsInput, TargetKind, TargetModelState, TargetStatus,
    },
    publish::PublishCoordinator,
    target::{default_target_paths, target_path, target_statuses, target_write_path},
    target_import::{get_target_model_states as read_target_model_states, TargetImportService},
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
    let import = TargetImportService::new(
        &state.store,
        Arc::clone(&state.secrets),
        &settings.target_paths,
    )
    .bootstrap_import()
    .map_err(CommandError::from)?;
    let targets =
        target_statuses(&state.store, &settings.target_paths).map_err(CommandError::from)?;
    Ok(BootstrapData {
        gateways: state.store.list_gateways().map_err(CommandError::from)?,
        models: state.store.list_models().map_err(CommandError::from)?,
        targets,
        target_model_states: import.states,
        import_report: import.report,
        settings,
    })
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
    if input.token.trim().is_empty() {
        return Err(
            crate::error::CoreError::Validation("API token is required".to_string()).into(),
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
    let token_ref = existing
        .as_ref()
        .map(|profile| profile.token_ref.clone())
        .unwrap_or_else(|| id.clone());
    let profile = GatewayProfile {
        id: id.clone(),
        name: name.to_string(),
        api_root,
        token_ref,
        created_at: existing
            .map(|profile| profile.created_at)
            .unwrap_or_else(|| now.clone()),
        updated_at: now,
    };

    let models_invalidated = GatewayService::new(&state.store, Arc::clone(&state.secrets))
        .save(&profile, input.token.trim())
        .map_err(CommandError::from)?;
    Ok(SaveGatewayResult {
        profile,
        models_invalidated,
    })
}

#[tauri::command]
pub fn get_gateway_token(id: String, state: State<'_, AppState>) -> CommandResult<String> {
    let _mutation = lock_app_mutation(state.inner())?;
    let profile = state.store.gateway(&id).map_err(CommandError::from)?;
    state
        .secrets
        .get(&profile.token_ref)
        .map_err(CommandError::from)
}

#[tauri::command]
pub fn delete_gateway(id: String, state: State<'_, AppState>) -> CommandResult<()> {
    let _mutation = lock_app_mutation(state.inner())?;
    GatewayService::new(&state.store, Arc::clone(&state.secrets))
        .delete(&id)
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn discover_models(
    gateway_id: String,
    state: State<'_, AppState>,
) -> CommandResult<Vec<ManagedModel>> {
    let (profile, token, existing) = {
        let _mutation = lock_app_mutation(state.inner())?;
        let profile = state
            .store
            .gateway(&gateway_id)
            .map_err(CommandError::from)?;
        let token = state
            .secrets
            .get(&profile.token_ref)
            .map_err(CommandError::from)?;
        let existing = state
            .store
            .models_for_gateway_including_stale(&gateway_id)
            .map_err(CommandError::from)?;
        (profile, token, existing)
    };
    let mut models = state
        .gateway_client
        .discover(&profile, &token, &existing)
        .await
        .map_err(CommandError::from)?;
    preserve_local_models(&mut models, &existing);
    models.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
    let _mutation = lock_app_mutation(state.inner())?;
    let current_profile = state
        .store
        .gateway(&gateway_id)
        .map_err(CommandError::from)?;
    let current_token = state
        .secrets
        .get(&current_profile.token_ref)
        .map_err(CommandError::from)?;
    ensure_gateway_snapshot_unchanged(&profile, &token, &current_profile, &current_token)
        .map_err(CommandError::from)?;
    state
        .store
        .replace_gateway_models_if_unchanged(&profile, &existing, &models)
        .map_err(CommandError::from)?;
    Ok(models)
}

#[tauri::command]
pub async fn add_manual_model(
    input: ManualModelInput,
    state: State<'_, AppState>,
) -> CommandResult<ManagedModel> {
    let id = input.id.trim().to_string();
    if id.is_empty() {
        return Err(crate::error::CoreError::Validation("Model ID is required".to_string()).into());
    }
    let gateway_snapshot = {
        let _mutation = lock_app_mutation(state.inner())?;
        let gateway = state
            .store
            .gateway(&input.gateway_id)
            .map_err(CommandError::from)?;
        ensure_model_id_available(
            &state
                .store
                .models_for_gateway(&input.gateway_id)
                .map_err(CommandError::from)?,
            &id,
        )
        .map_err(CommandError::from)?;
        gateway
    };

    let lookup_vendor = if input.vendor.trim().is_empty() {
        infer_vendor(&id)
    } else {
        input.vendor.trim().to_ascii_lowercase()
    };
    let market_model = state.gateway_client.market_model(&id, &lookup_vendor).await;
    let model = build_manual_model(
        &input.gateway_id,
        &id,
        &input.name,
        &input.vendor,
        market_model.as_ref(),
    );
    let _mutation = lock_app_mutation(state.inner())?;
    let current_gateway = state
        .store
        .gateway(&input.gateway_id)
        .map_err(CommandError::from)?;
    if current_gateway != gateway_snapshot {
        return Err(crate::error::CoreError::Conflict(
            "The API profile changed while the model was being added; reload and try again"
                .to_string(),
        )
        .into());
    }
    ensure_model_id_available(
        &state
            .store
            .models_for_gateway(&input.gateway_id)
            .map_err(CommandError::from)?,
        &id,
    )
    .map_err(CommandError::from)?;
    state.store.save_model(&model).map_err(CommandError::from)?;
    Ok(model)
}

#[tauri::command]
pub async fn probe_model(
    model_key: String,
    state: State<'_, AppState>,
) -> CommandResult<ProbeSummary> {
    let (mut model, profile, token) = {
        let _mutation = lock_app_mutation(state.inner())?;
        let model = state.store.model(&model_key).map_err(CommandError::from)?;
        let profile = state
            .store
            .gateway(&model.gateway_id)
            .map_err(CommandError::from)?;
        let token = state
            .secrets
            .get(&profile.token_ref)
            .map_err(CommandError::from)?;
        (model, profile, token)
    };
    let model_snapshot = model.clone();
    let (probe_evidence, notes) = state
        .gateway_client
        .probe(&profile, &token, &model)
        .await
        .map_err(CommandError::from)?;
    model.evidence.extend(probe_evidence);
    let (capabilities, evidence) =
        CapabilityResolver::resolve(&model.id, &model.metadata, &model.evidence);
    model.capabilities = capabilities;
    model.evidence = evidence;
    model.updated_at = Utc::now().to_rfc3339();
    let _mutation = lock_app_mutation(state.inner())?;
    let current_profile = state
        .store
        .gateway(&model.gateway_id)
        .map_err(CommandError::from)?;
    let current_token = state
        .secrets
        .get(&current_profile.token_ref)
        .map_err(CommandError::from)?;
    let current_model = state.store.model(&model_key).map_err(CommandError::from)?;
    ensure_gateway_snapshot_unchanged(&profile, &token, &current_profile, &current_token)
        .map_err(CommandError::from)?;
    if current_model != model_snapshot {
        return Err(crate::error::CoreError::Conflict(
            "The model changed while it was being probed; reload and try again".to_string(),
        )
        .into());
    }
    state.store.save_model(&model).map_err(CommandError::from)?;
    Ok(ProbeSummary {
        model,
        request_count: 3,
        notes,
    })
}

#[tauri::command]
pub fn update_model(
    input: ModelUpdateInput,
    state: State<'_, AppState>,
) -> CommandResult<ManagedModel> {
    let _mutation = lock_app_mutation(state.inner())?;
    let mut model = state
        .store
        .model(&input.model_key)
        .map_err(CommandError::from)?;
    let name = input.name.trim();
    let vendor = input.vendor.trim();
    if name.is_empty() || vendor.is_empty() {
        return Err(crate::error::CoreError::Validation(
            "Model name and vendor are required".to_string(),
        )
        .into());
    }
    let configuration = normalize_model_configuration(input.configuration, &input.capabilities)
        .map_err(CommandError::from)?;
    let normalized_vendor = vendor.to_ascii_lowercase();
    record_identity_override(&mut model, name, &normalized_vendor);
    let reasoning_configuration_changed = model.configuration.only_reasoning
        != configuration.only_reasoning
        || model.configuration.reasoning != configuration.reasoning;
    let configuration_changed = model.configuration != configuration;
    if model.capabilities != input.capabilities {
        CapabilityResolver::apply_manual(&mut model, input.capabilities);
    } else {
        model.updated_at = Utc::now().to_rfc3339();
    }
    model.name = name.to_string();
    model.vendor = normalized_vendor;
    if configuration_changed {
        let capability = if reasoning_configuration_changed {
            "reasoningConfiguration"
        } else {
            "configuration"
        };
        model
            .evidence
            .retain(|item| item.source != EvidenceSource::Manual || item.capability != capability);
        model.evidence.push(evidence(
            capability,
            true,
            EvidenceSource::Manual,
            "User override",
            &Utc::now().to_rfc3339(),
        ));
    }
    model.configuration = configuration;
    state.store.save_model(&model).map_err(CommandError::from)?;
    Ok(model)
}

#[tauri::command]
pub fn get_target_statuses(state: State<'_, AppState>) -> CommandResult<Vec<TargetStatus>> {
    let settings = state
        .store
        .settings(default_target_paths().map_err(CommandError::from)?)
        .map_err(CommandError::from)?;
    target_statuses(&state.store, &settings.target_paths).map_err(CommandError::from)
}

#[tauri::command]
pub fn get_target_model_states(state: State<'_, AppState>) -> CommandResult<Vec<TargetModelState>> {
    let settings = state
        .store
        .settings(default_target_paths().map_err(CommandError::from)?)
        .map_err(CommandError::from)?;
    read_target_model_states(
        &state.store,
        Arc::clone(&state.secrets),
        &settings.target_paths,
    )
    .map_err(CommandError::from)
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
        secrets: Arc::clone(&state.secrets),
        backup_root: &state.backup_root,
    }
}

fn lock_app_mutation(state: &AppState) -> CommandResult<MutexGuard<'_, ()>> {
    state.app_mutation.lock().map_err(|_| {
        CommandError::from(crate::error::CoreError::Storage(
            "Application mutation lock is unavailable".to_string(),
        ))
    })
}

fn ensure_gateway_snapshot_unchanged(
    expected_profile: &GatewayProfile,
    expected_token: &str,
    current_profile: &GatewayProfile,
    current_token: &str,
) -> crate::error::CoreResult<()> {
    if expected_profile != current_profile || expected_token != current_token {
        return Err(crate::error::CoreError::Conflict(
            "The API profile or credential changed while the request was running; reload and try again"
                .to_string(),
        ));
    }
    Ok(())
}

fn ensure_model_id_available(
    models: &[ManagedModel],
    model_id: &str,
) -> crate::error::CoreResult<()> {
    if models.iter().any(|model| model.id == model_id) {
        return Err(crate::error::CoreError::Validation(
            "This model ID already exists in the selected API source".to_string(),
        ));
    }
    Ok(())
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

fn normalize_model_configuration(
    mut configuration: ModelConfiguration,
    capabilities: &crate::models::CapabilitySet,
) -> crate::error::CoreResult<ModelConfiguration> {
    configuration.endpoint_override = configuration
        .endpoint_override
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(|endpoint| normalize_api_root(&endpoint))
        .transpose()?;
    if configuration
        .temperature
        .is_some_and(|temperature| !temperature.is_finite() || temperature < 0.0)
    {
        return Err(crate::error::CoreError::Validation(
            "Temperature must be a finite non-negative number".to_string(),
        ));
    }
    if [
        configuration.max_input_tokens,
        configuration.max_output_tokens,
    ]
    .into_iter()
    .flatten()
    .any(|value| value == 0 || value > crate::models::MAX_SAFE_INTEGER)
    {
        return Err(crate::error::CoreError::Validation(
            "Token limits must be positive safe integers".to_string(),
        ));
    }

    let mut efforts = Vec::new();
    for effort in configuration.reasoning.supported_efforts {
        if !efforts.contains(&effort) {
            efforts.push(effort);
        }
    }
    configuration.reasoning.supported_efforts = efforts;

    for selected in [
        configuration.reasoning.effort,
        configuration.reasoning.default_effort,
    ]
    .into_iter()
    .flatten()
    {
        if !configuration.reasoning.supported_efforts.is_empty()
            && !configuration
                .reasoning
                .supported_efforts
                .contains(&selected)
        {
            return Err(crate::error::CoreError::Validation(
                "Reasoning effort and default effort must be included in supported efforts"
                    .to_string(),
            ));
        }
    }

    if !capabilities.supports_reasoning {
        configuration.only_reasoning = false;
        configuration.reasoning = Default::default();
    } else if configuration.only_reasoning {
        configuration.reasoning.can_disable_thinking = false;
    }
    Ok(configuration)
}

fn record_identity_override(model: &mut ManagedModel, name: &str, vendor: &str) {
    if model.name == name && model.vendor == vendor {
        return;
    }
    if !model.metadata.is_object() {
        model.metadata = json!({});
    }
    let mut identity_override = model
        .metadata
        .get("everybuddyIdentityOverride")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    if model.name != name {
        identity_override.insert("name".to_string(), json!(name));
    }
    if model.vendor != vendor {
        identity_override.insert("vendor".to_string(), json!(vendor));
    }
    model.metadata["everybuddyIdentityOverride"] = Value::Object(identity_override);
}

fn build_manual_model(
    gateway_id: &str,
    id: &str,
    name: &str,
    vendor: &str,
    market_model: Option<&MarketModel>,
) -> ManagedModel {
    let explicit_name = name.trim();
    let explicit_vendor = vendor.trim();
    let vendor = if explicit_vendor.is_empty() {
        market_model
            .and_then(MarketModel::vendor)
            .unwrap_or_else(|| infer_vendor(id))
    } else {
        explicit_vendor.to_ascii_lowercase()
    };
    let resolved_name = if explicit_name.is_empty() {
        market_model
            .and_then(MarketModel::display_name)
            .unwrap_or(id)
            .to_string()
    } else {
        explicit_name.to_string()
    };
    let mut metadata = json!({
        "id": id,
        "owned_by": vendor,
        "everybuddySource": "manual"
    });
    let mut identity_override = serde_json::Map::new();
    if !explicit_name.is_empty() {
        identity_override.insert("name".to_string(), json!(resolved_name.clone()));
    }
    if !explicit_vendor.is_empty() {
        identity_override.insert("vendor".to_string(), json!(vendor.clone()));
    }
    if !identity_override.is_empty() {
        metadata["everybuddyIdentityOverride"] = Value::Object(identity_override);
    }
    let (capabilities, evidence) =
        CapabilityResolver::resolve_with_market(id, &metadata, market_model, &[]);
    let configuration = configuration_from_sources(id, &metadata, market_model, &capabilities);

    ManagedModel {
        key: format!("{gateway_id}::{id}"),
        gateway_id: gateway_id.to_string(),
        id: id.to_string(),
        name: resolved_name,
        vendor,
        capabilities,
        configuration,
        evidence,
        metadata,
        updated_at: Utc::now().to_rfc3339(),
    }
}

fn preserve_local_models(discovered: &mut Vec<ManagedModel>, existing: &[ManagedModel]) {
    for model in existing.iter().filter(|model| is_local_model(model)) {
        if !discovered.iter().any(|item| item.key == model.key) {
            discovered.push(model.clone());
        }
    }
}

fn is_local_model(model: &ManagedModel) -> bool {
    matches!(
        model
            .metadata
            .get("everybuddySource")
            .and_then(Value::as_str),
        Some("manual" | "targetImport")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

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
        imported.metadata["everybuddySource"] = json!("targetImport");
        imported.evidence[0].source = crate::models::EvidenceSource::Imported;
        let mut refreshed = Vec::new();

        preserve_local_models(&mut refreshed, &[imported.clone()]);

        assert_eq!(refreshed.len(), 1);
        assert_eq!(refreshed[0].key, imported.key);
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
            token_ref: "gateway".to_string(),
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
