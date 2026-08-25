use std::sync::Arc;

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
        SaveSettingsInput, TargetKind, TargetModelState, TargetStatus,
    },
    publish::PublishCoordinator,
    target::{default_target_paths, target_statuses},
    target_import::{get_target_model_states as read_target_model_states, TargetImportService},
    AppState,
};

type CommandResult<T> = Result<T, CommandError>;

#[tauri::command]
pub fn bootstrap(state: State<'_, AppState>) -> CommandResult<BootstrapData> {
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
) -> CommandResult<GatewayProfile> {
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

    GatewayService::new(&state.store, Arc::clone(&state.secrets))
        .save(&profile, input.token.trim())
        .map_err(CommandError::from)?;
    Ok(profile)
}

#[tauri::command]
pub fn get_gateway_token(id: String, state: State<'_, AppState>) -> CommandResult<String> {
    let profile = state.store.gateway(&id).map_err(CommandError::from)?;
    state
        .secrets
        .get(&profile.token_ref)
        .map_err(CommandError::from)
}

#[tauri::command]
pub fn delete_gateway(id: String, state: State<'_, AppState>) -> CommandResult<()> {
    GatewayService::new(&state.store, Arc::clone(&state.secrets))
        .delete(&id)
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn discover_models(
    gateway_id: String,
    state: State<'_, AppState>,
) -> CommandResult<Vec<ManagedModel>> {
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
        .models_for_gateway(&gateway_id)
        .map_err(CommandError::from)?;
    let mut models = state
        .gateway_client
        .discover(&profile, &token, &existing)
        .await
        .map_err(CommandError::from)?;
    preserve_local_models(&mut models, &existing);
    models.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
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
    state
        .store
        .gateway(&input.gateway_id)
        .map_err(CommandError::from)?;

    let id = input.id.trim();
    if id.is_empty() {
        return Err(crate::error::CoreError::Validation("Model ID is required".to_string()).into());
    }
    let existing = state
        .store
        .models_for_gateway(&input.gateway_id)
        .map_err(CommandError::from)?;
    if existing.iter().any(|model| model.id == id) {
        return Err(crate::error::CoreError::Validation(
            "This model ID already exists in the selected API source".to_string(),
        )
        .into());
    }

    let lookup_vendor = if input.vendor.trim().is_empty() {
        infer_vendor(id)
    } else {
        input.vendor.trim().to_ascii_lowercase()
    };
    let market_model = state.gateway_client.market_model(id, &lookup_vendor).await;
    let model = build_manual_model(
        &input.gateway_id,
        id,
        &input.name,
        &input.vendor,
        market_model.as_ref(),
    );
    state.store.save_model(&model).map_err(CommandError::from)?;
    Ok(model)
}

#[tauri::command]
pub async fn probe_model(
    model_key: String,
    state: State<'_, AppState>,
) -> CommandResult<ProbeSummary> {
    let mut model = state.store.model(&model_key).map_err(CommandError::from)?;
    let profile = state
        .store
        .gateway(&model.gateway_id)
        .map_err(CommandError::from)?;
    let token = state
        .secrets
        .get(&profile.token_ref)
        .map_err(CommandError::from)?;
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
    model.vendor = vendor.to_ascii_lowercase();
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
    coordinator(state.inner())
        .restore(&id)
        .map_err(CommandError::from)
}

#[tauri::command]
pub fn save_settings(
    input: SaveSettingsInput,
    state: State<'_, AppState>,
) -> CommandResult<AppSettings> {
    if !matches!(input.language.as_str(), "zh-CN" | "en") {
        return Err(crate::error::CoreError::Validation(
            "Unsupported interface language".to_string(),
        )
        .into());
    }
    if !matches!(input.theme.as_str(), "light" | "dark" | "system") {
        return Err(crate::error::CoreError::Validation("Unsupported theme".to_string()).into());
    }
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
    let settings = AppSettings {
        language: input.language,
        theme: input.theme,
        selected_targets: input.selected_targets,
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
        .is_some_and(|temperature| !temperature.is_finite())
    {
        return Err(crate::error::CoreError::Validation(
            "Temperature must be a finite number".to_string(),
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

fn build_manual_model(
    gateway_id: &str,
    id: &str,
    name: &str,
    vendor: &str,
    market_model: Option<&MarketModel>,
) -> ManagedModel {
    let fallback_vendor = if vendor.trim().is_empty() {
        infer_vendor(id)
    } else {
        vendor.trim().to_ascii_lowercase()
    };
    let vendor = market_model
        .and_then(MarketModel::vendor)
        .unwrap_or(fallback_vendor);
    let metadata = json!({
        "id": id,
        "owned_by": vendor,
        "everybuddySource": "manual"
    });
    let (capabilities, evidence) =
        CapabilityResolver::resolve_with_market(id, &metadata, market_model, &[]);
    let configuration = configuration_from_sources(id, &metadata, market_model, &capabilities);

    ManagedModel {
        key: format!("{gateway_id}::{id}"),
        gateway_id: gateway_id.to_string(),
        id: id.to_string(),
        name: if name.trim().is_empty() {
            market_model
                .and_then(MarketModel::display_name)
                .unwrap_or(id)
                .to_string()
        } else {
            name.trim().to_string()
        },
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
