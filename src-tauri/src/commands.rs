use std::{
    collections::HashSet,
    sync::{Arc, MutexGuard},
};

use chrono::Utc;
use serde_json::{json, Value};
use tauri::State;
use uuid::Uuid;

use crate::{
    capability::{
        configuration_from_sources, evidence, infer_vendor, supports_chat_configuration,
        CapabilityResolver,
    },
    error::CommandError,
    gateway::{normalize_api_root, normalize_request_url},
    gateway_service::GatewayService,
    market_catalog::MarketModel,
    models::{
        AppSettings, BackupRecord, BootstrapData, EvidenceSource, ExecutePublishRequest,
        GatewayInput, GatewayProfile, ManagedModel, ManualModelInput, ModelConfiguration,
        ModelOrigin, ModelUpdateInput, PreparePublishRequest, ProbeSummary, PublishPreview,
        PublishResult, SaveGatewayResult, SaveSettingsInput, TargetKind, TargetModelState,
        TargetStatus,
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
    replace_probe_evidence(&mut model, probe_evidence);
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
pub async fn apply_openrouter_model(
    model_key: String,
    state: State<'_, AppState>,
) -> CommandResult<ManagedModel> {
    let model_snapshot = {
        let _mutation = lock_app_mutation(state.inner())?;
        state.store.model(&model_key).map_err(CommandError::from)?
    };
    let detail = state
        .gateway_client
        .market_model_detail(&model_snapshot.id, &model_snapshot.vendor)
        .await
        .map_err(CommandError::from)?;
    let mut model = model_snapshot.clone();
    apply_openrouter_detail(&mut model, &detail);

    let _mutation = lock_app_mutation(state.inner())?;
    let current_model = state.store.model(&model_key).map_err(CommandError::from)?;
    if current_model != model_snapshot {
        return Err(crate::error::CoreError::Conflict(
            "The model changed while OpenRouter information was being loaded; reload and try again"
                .to_string(),
        )
        .into());
    }
    state.store.save_model(&model).map_err(CommandError::from)?;
    Ok(model)
}

#[tauri::command]
pub async fn get_openrouter_model_match(
    model_key: String,
    state: State<'_, AppState>,
) -> CommandResult<Option<String>> {
    let model = {
        let _mutation = lock_app_mutation(state.inner())?;
        state.store.model(&model_key).map_err(CommandError::from)?
    };
    Ok(state
        .gateway_client
        .market_model(&model.id, &model.vendor)
        .await
        .map(|matched| matched.id))
}

fn apply_openrouter_detail(model: &mut ManagedModel, detail: &MarketModel) {
    let metadata = Value::Null;
    let (capabilities, evidence) =
        CapabilityResolver::resolve_with_market(&model.id, &metadata, Some(detail), &[]);
    let mut configuration =
        configuration_from_sources(&model.id, &metadata, Some(detail), &capabilities);
    configuration.endpoint_override = model.configuration.endpoint_override.clone();
    configuration.use_custom_protocol = model.configuration.use_custom_protocol;

    if !model.metadata.is_object() {
        model.metadata = json!({});
    }
    model.metadata["everybuddyOpenRouterMatch"] = json!({
        "source": "openrouter",
        "modelId": detail.id,
        "supportsTextOutput": detail.supports_chat_configuration(),
    });
    model.capabilities = capabilities;
    model.configuration = configuration;
    model.evidence = evidence;
    model.updated_at = Utc::now().to_rfc3339();
}

fn replace_probe_evidence(
    model: &mut ManagedModel,
    probe_evidence: Vec<crate::models::CapabilityEvidence>,
) {
    model
        .evidence
        .retain(|item| item.source != EvidenceSource::Probe);
    model.evidence.extend(probe_evidence);
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
    apply_model_update(
        &mut model,
        input.capabilities,
        configuration,
        name,
        &normalized_vendor,
    );
    state.store.save_model(&model).map_err(CommandError::from)?;
    Ok(model)
}

fn apply_model_update(
    model: &mut ManagedModel,
    mut capabilities: crate::models::CapabilitySet,
    mut configuration: ModelConfiguration,
    name: &str,
    normalized_vendor: &str,
) {
    let supports_chat = supports_chat_configuration(&model.metadata);
    if !supports_chat {
        capabilities = Default::default();
        configuration.max_input_tokens = None;
        configuration.max_output_tokens = None;
        configuration.temperature = None;
        configuration.only_reasoning = false;
        configuration.reasoning = Default::default();
        model.evidence.retain(|item| {
            item.source != EvidenceSource::Manual
                || !matches!(
                    item.capability.as_str(),
                    "toolCall" | "images" | "reasoning"
                )
        });
    }
    let previous_capabilities = model.capabilities.clone();
    let request_target_changed = model.configuration.endpoint_override
        != configuration.endpoint_override
        || model.configuration.use_custom_protocol != configuration.use_custom_protocol;
    record_identity_override(model, name, normalized_vendor);
    if request_target_changed {
        model
            .evidence
            .retain(|item| item.source != EvidenceSource::Probe);
        let (mut resolved, evidence) =
            CapabilityResolver::resolve(&model.id, &model.metadata, &model.evidence);
        if resolved.supports_reasoning && resolved.reasoning_efforts.is_empty() {
            resolved.reasoning_efforts = previous_capabilities.reasoning_efforts.clone();
        }
        model.capabilities = resolved;
        model.evidence = evidence;
        if supports_chat {
            apply_manual_capability_changes(model, &previous_capabilities, &capabilities);
        }
    } else if previous_capabilities != capabilities && supports_chat {
        CapabilityResolver::apply_manual(model, capabilities);
    } else if previous_capabilities != capabilities {
        model.capabilities = capabilities;
    }
    if !model.capabilities.supports_reasoning {
        configuration.only_reasoning = false;
        configuration.reasoning = Default::default();
    }
    let reasoning_configuration_changed = model.configuration.only_reasoning
        != configuration.only_reasoning
        || model.configuration.reasoning != configuration.reasoning;
    let configuration_changed = model.configuration != configuration;
    model.updated_at = Utc::now().to_rfc3339();
    model.name = name.to_string();
    model.vendor = normalized_vendor.to_string();
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
}

fn apply_manual_capability_changes(
    model: &mut ManagedModel,
    previous: &crate::models::CapabilitySet,
    submitted: &crate::models::CapabilitySet,
) {
    let now = Utc::now().to_rfc3339();
    let changes = [
        (
            "toolCall",
            previous.supports_tool_call != submitted.supports_tool_call,
            submitted.supports_tool_call,
        ),
        (
            "images",
            previous.supports_images != submitted.supports_images,
            submitted.supports_images,
        ),
        (
            "reasoning",
            previous.supports_reasoning != submitted.supports_reasoning,
            submitted.supports_reasoning,
        ),
    ];
    let mut applied = false;
    for (capability, changed, value) in changes {
        if !changed {
            continue;
        }
        applied = true;
        model
            .evidence
            .retain(|item| item.source != EvidenceSource::Manual || item.capability != capability);
        model.evidence.push(evidence(
            capability,
            value,
            EvidenceSource::Manual,
            "User override",
            &now,
        ));
    }
    if !applied {
        return;
    }
    let reasoning_efforts = model.capabilities.reasoning_efforts.clone();
    let (mut capabilities, evidence) =
        CapabilityResolver::resolve(&model.id, &model.metadata, &model.evidence);
    if capabilities.supports_reasoning && capabilities.reasoning_efforts.is_empty() {
        capabilities.reasoning_efforts = reasoning_efforts;
    }
    model.capabilities = capabilities;
    model.evidence = evidence;
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
    let normalize_endpoint: fn(&str) -> crate::error::CoreResult<String> =
        if configuration.use_custom_protocol {
            normalize_request_url
        } else {
            normalize_api_root
        };
    configuration.endpoint_override = configuration
        .endpoint_override
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(|endpoint| normalize_endpoint(&endpoint))
        .transpose()?;
    if configuration.use_custom_protocol && configuration.endpoint_override.is_none() {
        return Err(crate::error::CoreError::Validation(
            "Custom protocol requires a complete request URL".to_string(),
        ));
    }
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
    if configuration
        .reasoning
        .summary
        .is_some_and(|summary| !summary.is_supported_target_value())
    {
        return Err(crate::error::CoreError::Validation(
            "Reasoning summary must be auto, concise, or detailed".to_string(),
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
        "owned_by": vendor
    });
    ModelOrigin::Manual.write_to_metadata(&mut metadata);
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
    if let Some(market_model) = market_model {
        metadata["everybuddyOpenRouterMatch"] = json!({
            "source": "openrouter",
            "modelId": market_model.id,
            "supportsTextOutput": market_model.supports_chat_configuration(),
        });
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
    for model in existing.iter().filter(|model| model.is_locally_managed()) {
        if !discovered.iter().any(|item| item.key == model.key) {
            discovered.push(model.clone());
        }
    }
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
