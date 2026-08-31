use std::sync::{Mutex, MutexGuard};

use chrono::Utc;
use serde_json::{json, Value};

use crate::{
    capability::{
        configuration_from_sources, evidence, infer_vendor, supports_chat_configuration,
        CapabilityResolver,
    },
    error::{CoreError, CoreResult},
    gateway::{normalize_api_root, normalize_request_url, GatewayClient},
    market_catalog::MarketModel,
    models::{
        EvidenceSource, GatewayProfile, ManagedModel, ManualModelInput, ModelConfiguration,
        ModelOrigin, ModelUpdateInput, ProbeSummary,
    },
    store::Store,
};

pub struct ModelLifecycle<'a> {
    store: &'a Store,
    gateway_client: &'a GatewayClient,
    app_mutation: &'a Mutex<()>,
}

pub(crate) fn apply_openrouter_detail(model: &mut ManagedModel, detail: &MarketModel) {
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

pub(crate) fn replace_probe_evidence(
    model: &mut ManagedModel,
    probe_evidence: Vec<crate::models::CapabilityEvidence>,
) {
    model
        .evidence
        .retain(|item| item.source != EvidenceSource::Probe);
    model.evidence.extend(probe_evidence);
}

pub(crate) fn apply_model_update(
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

pub(crate) fn ensure_gateway_snapshot_unchanged(
    expected_profile: &GatewayProfile,
    expected_token: &str,
    current_profile: &GatewayProfile,
    current_token: &str,
) -> CoreResult<()> {
    if expected_profile != current_profile || expected_token != current_token {
        return Err(CoreError::Conflict(
            "The API profile or credential changed while the request was running; reload and try again"
                .to_string(),
        ));
    }
    Ok(())
}

pub(crate) fn ensure_model_id_available(models: &[ManagedModel], model_id: &str) -> CoreResult<()> {
    if models.iter().any(|model| model.id == model_id) {
        return Err(CoreError::Validation(
            "This model ID already exists in the selected API source".to_string(),
        ));
    }
    Ok(())
}

pub(crate) fn normalize_model_configuration(
    mut configuration: ModelConfiguration,
    capabilities: &crate::models::CapabilitySet,
) -> CoreResult<ModelConfiguration> {
    let normalize_endpoint: fn(&str) -> CoreResult<String> = if configuration.use_custom_protocol {
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
        return Err(CoreError::Validation(
            "Custom protocol requires a complete request URL".to_string(),
        ));
    }
    if configuration
        .temperature
        .is_some_and(|temperature| !temperature.is_finite() || temperature < 0.0)
    {
        return Err(CoreError::Validation(
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
        return Err(CoreError::Validation(
            "Token limits must be positive safe integers".to_string(),
        ));
    }
    if configuration
        .reasoning
        .summary
        .is_some_and(|summary| !summary.is_supported_target_value())
    {
        return Err(CoreError::Validation(
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
            return Err(CoreError::Validation(
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

pub(crate) fn record_identity_override(model: &mut ManagedModel, name: &str, vendor: &str) {
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

pub(crate) fn build_manual_model(
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

pub(crate) fn preserve_local_models(discovered: &mut Vec<ManagedModel>, existing: &[ManagedModel]) {
    for model in existing.iter().filter(|model| model.is_locally_managed()) {
        if !discovered.iter().any(|item| item.key == model.key) {
            discovered.push(model.clone());
        }
    }
}

impl<'a> ModelLifecycle<'a> {
    pub fn new(
        store: &'a Store,
        gateway_client: &'a GatewayClient,
        app_mutation: &'a Mutex<()>,
    ) -> Self {
        Self {
            store,
            gateway_client,
            app_mutation,
        }
    }

    pub async fn discover(&self, gateway_id: String) -> CoreResult<Vec<ManagedModel>> {
        let (profile, token, existing) = {
            let _mutation = self.lock_mutation()?;
            let (profile, token) = self.store.gateway_with_token(&gateway_id)?;
            let existing = self.store.models_for_gateway_including_stale(&gateway_id)?;
            (profile, token, existing)
        };
        let mut models = self
            .gateway_client
            .discover(&profile, &token, &existing)
            .await?;
        preserve_local_models(&mut models, &existing);
        models.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));

        let _mutation = self.lock_mutation()?;
        let (current_profile, current_token) = self.store.gateway_with_token(&gateway_id)?;
        ensure_gateway_snapshot_unchanged(&profile, &token, &current_profile, &current_token)?;
        self.store
            .replace_gateway_models_if_unchanged(&profile, &existing, &models)?;
        Ok(models)
    }

    pub async fn add_manual(&self, input: ManualModelInput) -> CoreResult<ManagedModel> {
        let id = input.id.trim().to_string();
        if id.is_empty() {
            return Err(CoreError::Validation("Model ID is required".to_string()));
        }
        let gateway_snapshot = {
            let _mutation = self.lock_mutation()?;
            let gateway = self.store.gateway(&input.gateway_id)?;
            ensure_model_id_available(&self.store.models_for_gateway(&input.gateway_id)?, &id)?;
            gateway
        };

        let lookup_vendor = if input.vendor.trim().is_empty() {
            crate::capability::infer_vendor(&id)
        } else {
            input.vendor.trim().to_ascii_lowercase()
        };
        let market_model = self.gateway_client.market_model(&id, &lookup_vendor).await;
        let model = build_manual_model(
            &input.gateway_id,
            &id,
            &input.name,
            &input.vendor,
            market_model.as_ref(),
        );

        let _mutation = self.lock_mutation()?;
        let current_gateway = self.store.gateway(&input.gateway_id)?;
        if current_gateway != gateway_snapshot {
            return Err(CoreError::Conflict(
                "The API profile changed while the model was being added; reload and try again"
                    .to_string(),
            ));
        }
        ensure_model_id_available(&self.store.models_for_gateway(&input.gateway_id)?, &id)?;
        self.store.save_model(&model)?;
        Ok(model)
    }

    pub async fn probe(&self, model_key: String) -> CoreResult<ProbeSummary> {
        let (mut model, profile, token) = {
            let _mutation = self.lock_mutation()?;
            let model = self.store.model(&model_key)?;
            let (profile, token) = self.store.gateway_with_token(&model.gateway_id)?;
            (model, profile, token)
        };
        let model_snapshot = model.clone();
        let (probe_evidence, notes) = self.gateway_client.probe(&profile, &token, &model).await?;
        replace_probe_evidence(&mut model, probe_evidence);
        let (capabilities, evidence) =
            CapabilityResolver::resolve(&model.id, &model.metadata, &model.evidence);
        model.capabilities = capabilities;
        model.evidence = evidence;
        model.updated_at = Utc::now().to_rfc3339();

        let _mutation = self.lock_mutation()?;
        let (current_profile, current_token) = self.store.gateway_with_token(&model.gateway_id)?;
        let current_model = self.store.model(&model_key)?;
        ensure_gateway_snapshot_unchanged(&profile, &token, &current_profile, &current_token)?;
        if current_model != model_snapshot {
            return Err(CoreError::Conflict(
                "The model changed while it was being probed; reload and try again".to_string(),
            ));
        }
        self.store.save_model(&model)?;
        Ok(ProbeSummary {
            model,
            request_count: 3,
            notes,
        })
    }

    pub async fn apply_openrouter(&self, model_key: String) -> CoreResult<ManagedModel> {
        let model_snapshot = {
            let _mutation = self.lock_mutation()?;
            self.store.model(&model_key)?
        };
        let detail = self
            .gateway_client
            .market_model_detail(&model_snapshot.id, &model_snapshot.vendor)
            .await?;
        let mut model = model_snapshot.clone();
        apply_openrouter_detail(&mut model, &detail);

        let _mutation = self.lock_mutation()?;
        let current_model = self.store.model(&model_key)?;
        if current_model != model_snapshot {
            return Err(CoreError::Conflict(
                "The model changed while OpenRouter information was being loaded; reload and try again"
                    .to_string(),
            ));
        }
        self.store.save_model(&model)?;
        Ok(model)
    }

    pub async fn openrouter_match(&self, model_key: String) -> CoreResult<Option<String>> {
        let model = {
            let _mutation = self.lock_mutation()?;
            self.store.model(&model_key)?
        };
        Ok(self
            .gateway_client
            .market_model(&model.id, &model.vendor)
            .await
            .map(|matched| matched.id))
    }

    pub fn update(&self, input: ModelUpdateInput) -> CoreResult<ManagedModel> {
        let _mutation = self.lock_mutation()?;
        let mut model = self.store.model(&input.model_key)?;
        let name = input.name.trim();
        let vendor = input.vendor.trim();
        if name.is_empty() || vendor.is_empty() {
            return Err(CoreError::Validation(
                "Model name and vendor are required".to_string(),
            ));
        }
        let configuration =
            normalize_model_configuration(input.configuration, &input.capabilities)?;
        apply_model_update(
            &mut model,
            input.capabilities,
            configuration,
            name,
            &vendor.to_ascii_lowercase(),
        );
        self.store.save_model(&model)?;
        Ok(model)
    }

    fn lock_mutation(&self) -> CoreResult<MutexGuard<'_, ()>> {
        self.app_mutation
            .lock()
            .map_err(|_| CoreError::Storage("Application mutation lock is unavailable".to_string()))
    }
}
