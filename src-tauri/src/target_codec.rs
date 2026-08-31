use chrono::Utc;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use url::Url;

use crate::{
    capability::{configuration_from_metadata, evidence, infer_vendor, CapabilityResolver},
    gateway::{
        normalize_api_root, normalize_request_url, object_without_secret, value_contains_secret,
    },
    market_catalog,
    models::{
        CapabilitySet, EvidenceSource, GatewayProfile, ManagedModel, ModelOrigin,
        TargetImportIssue, TargetKind,
    },
};

#[derive(Clone, Hash, PartialEq, Eq)]
pub(crate) struct ModelIdentityKey {
    pub id: String,
    pub api_key: String,
}

#[derive(Clone, Hash, PartialEq, Eq)]
pub(crate) struct ModelIdentity {
    pub key: ModelIdentityKey,
    url: String,
    use_custom_protocol: bool,
}

pub(crate) struct DecodedTargetModel {
    pub target: TargetKind,
    pub model_id: String,
    name: String,
    vendor: String,
    pub api_root: String,
    pub token: String,
    capabilities: CapabilitySet,
    configuration: crate::models::ModelConfiguration,
    metadata: Value,
    evidence: Vec<crate::models::CapabilityEvidence>,
    pub signature: String,
}

impl DecodedTargetModel {
    pub fn parse_for_import(target: TargetKind, raw: &Value) -> Result<Self, TargetImportIssue> {
        Self::parse(target, raw, false)
    }

    pub fn parse_for_match(target: TargetKind, raw: &Value) -> Result<Self, TargetImportIssue> {
        Self::parse(target, raw, true)
    }

    fn parse(
        target: TargetKind,
        raw: &Value,
        allow_custom_protocol: bool,
    ) -> Result<Self, TargetImportIssue> {
        let object = raw.as_object().ok_or_else(|| {
            decode_issue(
                target,
                None,
                "invalidParameters",
                "The model entry must be a JSON object".to_string(),
            )
        })?;
        let model_id = required_string(object.get("id"), target, None, "missingModelId")?;
        let model_ref = Some(model_id.clone());
        let raw_url = required_string(object.get("url"), target, model_ref.clone(), "missingUrl")?;
        let use_custom_protocol = object
            .get("useCustomProtocol")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let normalized_url = if use_custom_protocol {
            normalize_request_url(&raw_url)
        } else {
            normalize_api_root(&raw_url)
        };
        let api_root = normalized_url.map_err(|_| {
            decode_issue(
                target,
                model_ref.clone(),
                "invalidUrl",
                "The target model URL is not a valid HTTP or HTTPS endpoint".to_string(),
            )
        })?;
        let token = required_string(
            object.get("apiKey"),
            target,
            model_ref.clone(),
            "missingToken",
        )?;
        let explicit_name = object
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);
        let explicit_vendor = object
            .get("vendor")
            .and_then(Value::as_str)
            .and_then(market_catalog::normalize_vendor);
        let mut metadata = object_without_secret(raw);
        if value_contains_secret(&metadata, &token) {
            return Err(decode_issue(
                target,
                model_ref.clone(),
                "invalidParameters",
                "The target model metadata contains credential data".to_string(),
            ));
        }
        for field in [
            "supportsToolCall",
            "supportsImages",
            "supportsReasoning",
            "onlyReasoning",
            "useCustomProtocol",
        ] {
            if object.get(field).is_some_and(|value| !value.is_boolean()) {
                return Err(decode_issue(
                    target,
                    model_ref,
                    "invalidParameters",
                    format!("{field} must be a boolean"),
                ));
            }
        }
        let configuration: crate::models::ModelConfiguration = serde_json::from_value(raw.clone())
            .map_err(|_| {
                decode_issue(
                    target,
                    model_ref.clone(),
                    "invalidParameters",
                    "The target model contains invalid advanced parameters".to_string(),
                )
            })?;
        if !configuration.has_valid_numeric_values() {
            return Err(decode_issue(
                target,
                model_ref.clone(),
                "invalidParameters",
                "Token limits and Temperature contain invalid numeric values".to_string(),
            ));
        }
        if configuration.use_custom_protocol && !allow_custom_protocol {
            return Err(decode_issue(
                target,
                model_ref,
                "customProtocol",
                "Custom protocol models are not imported automatically".to_string(),
            ));
        }
        let imported_capabilities = CapabilitySet {
            supports_tool_call: object
                .get("supportsToolCall")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            supports_images: object
                .get("supportsImages")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            supports_reasoning: object
                .get("supportsReasoning")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            reasoning_efforts: configuration
                .reasoning
                .supported_efforts
                .iter()
                .map(reasoning_effort_name)
                .map(ToString::to_string)
                .collect(),
        };
        let now = Utc::now().to_rfc3339();
        let imported_evidence = vec![
            evidence(
                "toolCall",
                imported_capabilities.supports_tool_call,
                EvidenceSource::Imported,
                "Imported from target configuration",
                &now,
            ),
            evidence(
                "images",
                imported_capabilities.supports_images,
                EvidenceSource::Imported,
                "Imported from target configuration",
                &now,
            ),
            evidence(
                "reasoning",
                imported_capabilities.supports_reasoning,
                EvidenceSource::Imported,
                "Imported from target configuration",
                &now,
            ),
        ];
        if let Some(metadata_object) = metadata.as_object_mut() {
            let identity_override = json!({
                "name": explicit_name.clone(),
                "vendor": explicit_vendor.clone(),
            });
            let identity_override = identity_override
                .as_object()
                .expect("identity override is an object")
                .iter()
                .filter(|(_, value)| !value.is_null())
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect::<Map<_, _>>();
            if !identity_override.is_empty() {
                metadata_object.insert(
                    "everybuddyIdentityOverride".to_string(),
                    Value::Object(identity_override),
                );
            }
        }
        ModelOrigin::Target.write_to_metadata(&mut metadata);
        let (mut capabilities, evidence) =
            CapabilityResolver::resolve(&model_id, &metadata, &imported_evidence);
        capabilities.reasoning_efforts = imported_capabilities.reasoning_efforts;
        let configuration = configuration_from_metadata(&model_id, raw, &capabilities);
        let name = explicit_name.unwrap_or_else(|| model_id.clone());
        let vendor = explicit_vendor.unwrap_or_else(|| infer_vendor(&model_id));
        let signature = fingerprint_value(&json!({
            "name": name,
            "vendor": vendor,
            "capabilities": capabilities,
            "configuration": configuration,
        }));
        Ok(Self {
            target,
            model_id,
            name,
            vendor,
            api_root,
            token,
            capabilities,
            configuration,
            metadata,
            evidence,
            signature,
        })
    }

    pub fn identity_hash(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.api_root.as_bytes());
        hasher.update([0]);
        hasher.update(self.token.as_bytes());
        hasher.update([0]);
        hasher.update(self.model_id.as_bytes());
        hex::encode(hasher.finalize())
    }

    pub fn model_identity(&self) -> ModelIdentity {
        ModelIdentity {
            key: ModelIdentityKey {
                id: self.model_id.clone(),
                api_key: self.token.clone(),
            },
            url: self.api_root.clone(),
            use_custom_protocol: self.configuration.use_custom_protocol,
        }
    }

    pub fn into_model(self, gateway_id: &str) -> ManagedModel {
        ManagedModel {
            key: format!("{gateway_id}::{}", self.model_id),
            gateway_id: gateway_id.to_string(),
            id: self.model_id,
            name: self.name,
            vendor: self.vendor,
            capabilities: self.capabilities,
            configuration: self.configuration,
            evidence: self.evidence,
            metadata: self.metadata,
            updated_at: Utc::now().to_rfc3339(),
        }
    }
}

fn required_string(
    value: Option<&Value>,
    target: TargetKind,
    model_id: Option<String>,
    code: &str,
) -> Result<String, TargetImportIssue> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .ok_or_else(|| {
            decode_issue(
                target,
                model_id,
                code,
                format!("Required field for {code} is missing"),
            )
        })
}

fn decode_issue(
    target: TargetKind,
    model_id: Option<String>,
    code: &str,
    message: String,
) -> TargetImportIssue {
    TargetImportIssue {
        target,
        model_id,
        code: code.to_string(),
        message,
    }
}

fn reasoning_effort_name(value: &crate::models::ReasoningEffort) -> &'static str {
    use crate::models::ReasoningEffort;
    match value {
        ReasoningEffort::Minimal => "minimal",
        ReasoningEffort::Low => "low",
        ReasoningEffort::Medium => "medium",
        ReasoningEffort::High => "high",
        ReasoningEffort::Xhigh => "xhigh",
        ReasoningEffort::Max => "max",
    }
}

fn fingerprint_value(value: &Value) -> String {
    let bytes = serde_json::to_vec(value).expect("target model signature is serializable");
    hex::encode(Sha256::digest(bytes))
}

impl ModelIdentity {
    pub fn exact(id: String, url: String, api_key: String, use_custom_protocol: bool) -> Self {
        Self {
            key: ModelIdentityKey { id, api_key },
            url,
            use_custom_protocol,
        }
    }

    pub fn belongs_to(&self, managed: &Self) -> bool {
        if self.url == managed.url && self.use_custom_protocol == managed.use_custom_protocol {
            return true;
        }
        if self.use_custom_protocol == managed.use_custom_protocol {
            return false;
        }

        let (standard, custom) = if self.use_custom_protocol {
            (managed, self)
        } else {
            (self, managed)
        };
        let (Ok(standard_url), Ok(custom_url)) =
            (Url::parse(&standard.url), Url::parse(&custom.url))
        else {
            return false;
        };
        if standard_url.origin() != custom_url.origin() {
            return false;
        }
        custom_url
            .path()
            .strip_prefix(standard_url.path())
            .is_some_and(|suffix| suffix.is_empty() || suffix.starts_with('/'))
    }
}

pub(crate) fn model_identity(model: &Value) -> Option<ModelIdentity> {
    let id = model.get("id")?.as_str()?.trim();
    let raw_url = model.get("url")?.as_str()?.trim();
    let api_key = model.get("apiKey")?.as_str()?.trim();
    if id.is_empty() || raw_url.is_empty() || api_key.is_empty() {
        return None;
    }
    let use_custom_protocol = model
        .get("useCustomProtocol")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let url = if use_custom_protocol {
        normalize_request_url(raw_url)
    } else {
        normalize_api_root(raw_url)
    }
    .ok()?;

    Some(ModelIdentity {
        key: ModelIdentityKey {
            id: id.to_string(),
            api_key: api_key.to_string(),
        },
        url,
        use_custom_protocol,
    })
}

pub(crate) fn encode_model(model: &ManagedModel, gateway: &GatewayProfile, token: &str) -> Value {
    let mut object = Map::new();
    object.insert("id".to_string(), json!(model.id));
    object.insert(
        "name".to_string(),
        json!(prefixed_model_name(&gateway.name, &model.name)),
    );
    object.insert("vendor".to_string(), json!(model.vendor));
    object.insert(
        "url".to_string(),
        json!(model
            .configuration
            .endpoint_override
            .as_deref()
            .unwrap_or(&gateway.api_root)),
    );
    object.insert("apiKey".to_string(), json!(token));
    if let Some(value) = model.configuration.max_input_tokens {
        object.insert("maxInputTokens".to_string(), json!(value));
    }
    if let Some(value) = model.configuration.max_output_tokens {
        object.insert("maxOutputTokens".to_string(), json!(value));
    }
    if let Some(value) = model.configuration.temperature {
        object.insert("temperature".to_string(), json!(value));
    }
    object.insert(
        "supportsToolCall".to_string(),
        json!(model.capabilities.supports_tool_call),
    );
    object.insert(
        "supportsImages".to_string(),
        json!(model.capabilities.supports_images),
    );
    object.insert(
        "supportsReasoning".to_string(),
        json!(model.capabilities.supports_reasoning),
    );
    object.insert(
        "onlyReasoning".to_string(),
        json!(model.capabilities.supports_reasoning && model.configuration.only_reasoning),
    );
    object.insert(
        "useCustomProtocol".to_string(),
        json!(model.configuration.use_custom_protocol),
    );
    if model.capabilities.supports_reasoning {
        let mut reasoning = Map::new();
        if let Some(value) = model.configuration.reasoning.effort {
            reasoning.insert("effort".to_string(), json!(value));
        }
        if let Some(value) = model.configuration.reasoning.default_effort {
            reasoning.insert("defaultEffort".to_string(), json!(value));
        }
        reasoning.insert(
            "supportedEfforts".to_string(),
            json!(model.configuration.reasoning.supported_efforts),
        );
        if let Some(value) = model.configuration.reasoning.summary {
            reasoning.insert("summary".to_string(), json!(value));
        }
        reasoning.insert(
            "canDisableThinking".to_string(),
            json!(model.configuration.reasoning.can_disable_thinking),
        );
        object.insert("reasoning".to_string(), Value::Object(reasoning));
    }
    Value::Object(object)
}

pub(crate) fn merge_known_fields(existing: &Value, incoming: &Value) -> Value {
    let mut merged = existing.as_object().cloned().unwrap_or_default();
    if let Some(fields) = incoming.as_object() {
        for (key, value) in fields {
            if key != "reasoning" {
                merged.insert(key.clone(), value.clone());
            }
        }
    }

    for field in ["maxInputTokens", "maxOutputTokens", "temperature"] {
        if incoming.get(field).is_none() {
            merged.remove(field);
        }
    }

    if let Some(incoming_reasoning) = incoming.get("reasoning").and_then(Value::as_object) {
        let mut reasoning = merged
            .get("reasoning")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        for field in ["effort", "defaultEffort", "summary"] {
            if incoming_reasoning.get(field).is_none() {
                reasoning.remove(field);
            }
        }
        for (key, value) in incoming_reasoning {
            reasoning.insert(key.clone(), value.clone());
        }
        merged.insert("reasoning".to_string(), Value::Object(reasoning));
    } else {
        let mut reasoning = merged
            .get("reasoning")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        for field in [
            "effort",
            "defaultEffort",
            "supportedEfforts",
            "summary",
            "canDisableThinking",
        ] {
            reasoning.remove(field);
        }
        if reasoning.is_empty() {
            merged.remove("reasoning");
        } else {
            merged.insert("reasoning".to_string(), Value::Object(reasoning));
        }
    }
    Value::Object(merged)
}

pub(crate) fn prefixed_model_name(gateway_name: &str, model_name: &str) -> String {
    let prefix = format!("{gateway_name} · ");
    if model_name.starts_with(&prefix) {
        model_name.to_string()
    } else {
        format!("{prefix}{model_name}")
    }
}
