use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    sync::Arc,
};

use chrono::Utc;
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use url::Url;
use uuid::Uuid;

use crate::{
    capability::{configuration_from_metadata, evidence, infer_vendor, CapabilityResolver},
    error::{CoreError, CoreResult},
    gateway::{normalize_api_root, object_without_secret, value_contains_secret},
    gateway_service::{gateway_source_hash, source_identity_key},
    market_catalog,
    models::{
        CapabilitySet, EvidenceSource, GatewayProfile, ManagedModel, TargetImportIssue,
        TargetImportReport, TargetKind, TargetModelState,
    },
    secrets::{SecretStore, MISSING_SECRET_MESSAGE},
    store::Store,
    target::{fingerprint, read_target_file, target_path, ConfigDocument},
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetImportResult {
    pub states: Vec<TargetModelState>,
    pub report: TargetImportReport,
}

pub struct TargetImportService<'a> {
    store: &'a Store,
    secrets: Arc<dyn SecretStore>,
    paths: &'a HashMap<TargetKind, String>,
}

impl<'a> TargetImportService<'a> {
    pub fn new(
        store: &'a Store,
        secrets: Arc<dyn SecretStore>,
        paths: &'a HashMap<TargetKind, String>,
    ) -> Self {
        Self {
            store,
            secrets,
            paths,
        }
    }

    pub fn bootstrap_import(&self) -> CoreResult<TargetImportResult> {
        let written_secret_refs = RefCell::new(Vec::new());
        let import = self.store.import_missing_serialized(
            |gateways, models, deleted_sources, source_history_exists| {
                let identity_key =
                    source_identity_key(self.secrets.as_ref(), source_history_exists)?;
                let mut context = ImportContext::from_snapshots(
                    gateways,
                    models,
                    deleted_sources,
                    Arc::clone(&self.secrets),
                    identity_key,
                )?;
                let mut report = TargetImportReport::default();
                let mut baselines = HashMap::new();

                for target in [TargetKind::Workbuddy, TargetKind::Codebuddy] {
                    let import =
                        self.import_target(target, &mut context, &mut baselines, &mut report);
                    *written_secret_refs.borrow_mut() = context.written_secret_refs.clone();
                    import?;
                }

                report.imported_gateway_count = context.new_gateways.len();
                report.imported_model_count = context.new_models.len();
                Ok((
                    report,
                    context.new_gateways,
                    context.source_identities,
                    context.new_models,
                ))
            },
        );
        let report = match import {
            Ok(report) => report,
            Err(error) => {
                return Err(cleanup_import_credentials(
                    self.secrets.as_ref(),
                    &written_secret_refs.into_inner(),
                    error,
                ));
            }
        };
        let states = get_target_model_states(self.store, Arc::clone(&self.secrets), self.paths)?;
        Ok(TargetImportResult { states, report })
    }

    fn import_target(
        &self,
        target: TargetKind,
        context: &mut ImportContext,
        baselines: &mut HashMap<String, String>,
        report: &mut TargetImportReport,
    ) -> CoreResult<()> {
        let path = target_path(target, self.paths)?;
        if !path.exists() {
            return Ok(());
        }
        let document = match ConfigDocument::read(&path) {
            Ok((document, _)) => document,
            Err(error) => {
                report
                    .issues
                    .push(issue(target, None, "targetReadFailed", error.to_string()));
                return Ok(());
            }
        };

        for raw in document.models() {
            let entry = match ParsedEntry::parse_for_import(target, raw) {
                Ok(entry) => entry,
                Err(parse_issue) => {
                    report.issues.push(parse_issue);
                    continue;
                }
            };
            self.import_entry(entry, context, baselines, report)?;
        }
        Ok(())
    }

    fn import_entry(
        &self,
        entry: ParsedEntry,
        context: &mut ImportContext,
        baselines: &mut HashMap<String, String>,
        report: &mut TargetImportReport,
    ) -> CoreResult<()> {
        let identity = entry.identity_hash();
        if let Some(baseline) = baselines.get(&identity) {
            if baseline != &entry.signature {
                report.issues.push(issue(
                    entry.target,
                    Some(entry.model_id.clone()),
                    "targetConflict",
                    format!(
                        "{} parameters differ from the WorkBuddy import baseline",
                        entry.target.display_name()
                    ),
                ));
            }
        } else {
            baselines.insert(identity, entry.signature.clone());
        }

        match context.exact_model_keys(&entry) {
            keys if keys.len() == 1 => {
                context.record_source_identity(&keys[0], &entry);
                return Ok(());
            }
            keys if keys.len() > 1 => {
                report.issues.push(issue(
                    entry.target,
                    Some(entry.model_id),
                    "ambiguousModel",
                    "Multiple local models match this target configuration".to_string(),
                ));
                return Ok(());
            }
            _ => {}
        }

        let gateway_id = match context.resolve_gateway(&entry, report)? {
            Some(id) => id,
            None => return Ok(()),
        };
        let imports_models = context.is_new_gateway(&gateway_id);

        if let Some(existing) = context
            .models
            .iter()
            .find(|model| model.gateway_id == gateway_id && model.id == entry.model_id)
        {
            if context.model_effective_root(existing).as_deref() == Some(entry.api_root.as_str()) {
                return Ok(());
            }
            report.issues.push(issue(
                entry.target,
                Some(entry.model_id),
                "modelConflict",
                "A local model with this ID uses a different endpoint".to_string(),
            ));
            return Ok(());
        }

        if !imports_models {
            return Ok(());
        }

        let model = entry.into_model(&gateway_id);
        context.models.push(model.clone());
        context.new_models.push(model);
        Ok(())
    }
}

pub fn get_target_model_states(
    store: &Store,
    secrets: Arc<dyn SecretStore>,
    paths: &HashMap<TargetKind, String>,
) -> CoreResult<Vec<TargetModelState>> {
    let context = ImportContext::load(store, secrets)?;
    [TargetKind::Workbuddy, TargetKind::Codebuddy]
        .into_iter()
        .map(|target| match_target_state(target, paths, &context))
        .collect()
}

fn match_target_state(
    target: TargetKind,
    paths: &HashMap<TargetKind, String>,
    context: &ImportContext,
) -> CoreResult<TargetModelState> {
    let path = target_path(target, paths)?;
    if !path.exists() {
        return Ok(TargetModelState::empty(target));
    }
    let bytes = match read_target_file(&path) {
        Ok(bytes) => bytes,
        Err(_) => {
            let mut state = TargetModelState::empty(target);
            state.skipped_count = 1;
            return Ok(state);
        }
    };
    let fingerprint_value = Some(fingerprint(&bytes));
    let document = match ConfigDocument::parse(&bytes) {
        Ok(document) => document,
        Err(_) => {
            let mut state = TargetModelState::empty(target);
            state.fingerprint = fingerprint_value;
            state.skipped_count = 1;
            return Ok(state);
        }
    };
    let mut matched = HashSet::new();
    let mut unmatched_count = 0;
    let mut skipped_count = 0;
    for raw in document.models() {
        match ParsedEntry::parse_for_match(target, raw) {
            Ok(entry) => {
                let keys = context.exact_model_keys(&entry);
                if keys.len() == 1 {
                    matched.insert(keys[0].clone());
                } else {
                    unmatched_count += 1;
                }
            }
            Err(_) => skipped_count += 1,
        }
    }
    let mut matched_model_keys: Vec<_> = matched.into_iter().collect();
    matched_model_keys.sort();
    Ok(TargetModelState {
        target,
        fingerprint: fingerprint_value,
        matched_model_keys,
        unmatched_count,
        skipped_count,
    })
}

struct ImportContext {
    secrets: Arc<dyn SecretStore>,
    gateways: Vec<GatewaySnapshot>,
    models: Vec<ManagedModel>,
    new_gateways: Vec<(GatewayProfile, String)>,
    new_models: Vec<ManagedModel>,
    written_secret_refs: Vec<String>,
    deleted_sources: HashSet<String>,
    identity_key: String,
    source_identities: Vec<(String, String)>,
}

impl ImportContext {
    fn load(store: &Store, secrets: Arc<dyn SecretStore>) -> CoreResult<Self> {
        let identity_key =
            source_identity_key(secrets.as_ref(), store.has_gateway_source_history()?)?;
        Self::from_snapshots(
            store.list_gateways()?,
            store.list_models()?,
            HashSet::new(),
            secrets,
            identity_key,
        )
    }

    fn from_snapshots(
        gateways: Vec<GatewayProfile>,
        models: Vec<ManagedModel>,
        deleted_sources: HashSet<String>,
        secrets: Arc<dyn SecretStore>,
        identity_key: String,
    ) -> CoreResult<Self> {
        let gateways = gateways
            .into_iter()
            .map(|profile| {
                let (token, credential_unavailable) = match secrets.get(&profile.token_ref) {
                    Ok(token) => (Some(token), false),
                    Err(CoreError::SecretStore(message)) if message == MISSING_SECRET_MESSAGE => {
                        (None, false)
                    }
                    Err(_) => (None, true),
                };
                GatewaySnapshot {
                    profile,
                    token,
                    credential_unavailable,
                }
            })
            .collect();
        Ok(Self {
            secrets,
            gateways,
            models,
            new_gateways: Vec::new(),
            new_models: Vec::new(),
            written_secret_refs: Vec::new(),
            deleted_sources,
            identity_key,
            source_identities: Vec::new(),
        })
    }

    fn exact_model_keys(&self, entry: &ParsedEntry) -> Vec<String> {
        self.models
            .iter()
            .filter(|model| model.id == entry.model_id)
            .filter_map(|model| {
                let gateway = self
                    .gateways
                    .iter()
                    .find(|gateway| gateway.profile.id == model.gateway_id)?;
                (gateway.token.as_deref() == Some(entry.token.as_str())
                    && self.model_effective_root(model).as_deref() == Some(entry.api_root.as_str())
                    && model.configuration.use_custom_protocol
                        == entry.configuration.use_custom_protocol)
                    .then(|| model.key.clone())
            })
            .collect()
    }

    fn model_effective_root(&self, model: &ManagedModel) -> Option<String> {
        let gateway = self
            .gateways
            .iter()
            .find(|gateway| gateway.profile.id == model.gateway_id)?;
        normalize_api_root(
            model
                .configuration
                .endpoint_override
                .as_deref()
                .unwrap_or(&gateway.profile.api_root),
        )
        .ok()
    }

    fn is_new_gateway(&self, gateway_id: &str) -> bool {
        self.new_gateways
            .iter()
            .any(|(gateway, _)| gateway.id == gateway_id)
    }

    fn record_source_identity(&mut self, model_key: &str, entry: &ParsedEntry) {
        let gateway_id = self
            .models
            .iter()
            .find(|model| model.key == model_key)
            .map(|model| model.gateway_id.clone());
        if let Some(gateway_id) = gateway_id {
            let source_hash =
                gateway_source_hash(&self.identity_key, &entry.api_root, &entry.token);
            self.source_identities.push((gateway_id, source_hash));
        }
    }

    fn resolve_gateway(
        &mut self,
        entry: &ParsedEntry,
        report: &mut TargetImportReport,
    ) -> CoreResult<Option<String>> {
        let exact: Vec<_> = self
            .gateways
            .iter()
            .filter(|gateway| {
                gateway.profile.api_root == entry.api_root
                    && gateway.token.as_deref() == Some(entry.token.as_str())
            })
            .map(|gateway| gateway.profile.id.clone())
            .collect();
        if exact.len() == 1 {
            let gateway_id = exact.into_iter().next().expect("one exact gateway");
            let source_hash =
                gateway_source_hash(&self.identity_key, &entry.api_root, &entry.token);
            self.source_identities
                .push((gateway_id.clone(), source_hash));
            return Ok(Some(gateway_id));
        }
        if exact.len() > 1 {
            report.issues.push(issue(
                entry.target,
                Some(entry.model_id.clone()),
                "ambiguousGateway",
                "Multiple API profiles match this endpoint and credential".to_string(),
            ));
            return Ok(None);
        }

        let missing: Vec<_> = self
            .gateways
            .iter()
            .enumerate()
            .filter(|(_, gateway)| {
                gateway.profile.api_root == entry.api_root
                    && gateway.token.is_none()
                    && !gateway.credential_unavailable
            })
            .map(|(index, _)| index)
            .collect();
        if missing.len() > 1 {
            report.issues.push(issue(
                entry.target,
                Some(entry.model_id.clone()),
                "ambiguousGateway",
                "Multiple API profiles use this endpoint but have no stored credential".to_string(),
            ));
            return Ok(None);
        }
        if let Some(index) = missing.first().copied() {
            let token_ref = self.gateways[index].profile.token_ref.clone();
            if self.secrets.set(&token_ref, &entry.token).is_err() {
                report.issues.push(issue(
                    entry.target,
                    Some(entry.model_id.clone()),
                    "credentialImportFailed",
                    "Could not save the imported credential".to_string(),
                ));
                return Ok(None);
            }
            self.written_secret_refs.push(token_ref);
            self.gateways[index].token = Some(entry.token.clone());
            let gateway_id = self.gateways[index].profile.id.clone();
            let source_hash =
                gateway_source_hash(&self.identity_key, &entry.api_root, &entry.token);
            self.source_identities
                .push((gateway_id.clone(), source_hash));
            return Ok(Some(gateway_id));
        }

        if self.gateways.iter().any(|gateway| {
            gateway.profile.api_root == entry.api_root && gateway.credential_unavailable
        }) {
            report.issues.push(issue(
                entry.target,
                Some(entry.model_id.clone()),
                "credentialUnavailable",
                "The system credential store is unavailable for a matching API profile".to_string(),
            ));
            return Ok(None);
        }

        if self.deleted_sources.contains(&gateway_source_hash(
            &self.identity_key,
            &entry.api_root,
            &entry.token,
        )) {
            return Ok(None);
        }

        let now = Utc::now().to_rfc3339();
        let id = Uuid::new_v4().to_string();
        let profile = GatewayProfile {
            id: id.clone(),
            name: imported_gateway_name(entry.target, &entry.api_root),
            api_root: entry.api_root.clone(),
            token_ref: id.clone(),
            created_at: now.clone(),
            updated_at: now,
        };
        if self.secrets.set(&profile.token_ref, &entry.token).is_err() {
            report.issues.push(issue(
                entry.target,
                Some(entry.model_id.clone()),
                "credentialImportFailed",
                "Could not save the imported credential".to_string(),
            ));
            return Ok(None);
        }
        self.written_secret_refs.push(profile.token_ref.clone());
        self.gateways.push(GatewaySnapshot {
            profile: profile.clone(),
            token: Some(entry.token.clone()),
            credential_unavailable: false,
        });
        let source_hash = gateway_source_hash(&self.identity_key, &entry.api_root, &entry.token);
        self.new_gateways.push((profile, source_hash));
        Ok(Some(id))
    }
}

fn cleanup_import_credentials(
    secrets: &dyn SecretStore,
    keys: &[String],
    primary: CoreError,
) -> CoreError {
    let mut cleanup_failed = false;
    for key in keys.iter().rev() {
        if secrets.delete(key).is_err() {
            cleanup_failed = true;
        }
    }
    if cleanup_failed {
        CoreError::SecretStore(
            "Could not import target configuration, and credential cleanup also failed".to_string(),
        )
    } else {
        primary
    }
}

struct GatewaySnapshot {
    profile: GatewayProfile,
    token: Option<String>,
    credential_unavailable: bool,
}

struct ParsedEntry {
    target: TargetKind,
    model_id: String,
    name: String,
    vendor: String,
    api_root: String,
    token: String,
    capabilities: CapabilitySet,
    configuration: crate::models::ModelConfiguration,
    metadata: Value,
    evidence: Vec<crate::models::CapabilityEvidence>,
    signature: String,
}

impl ParsedEntry {
    fn parse_for_import(target: TargetKind, raw: &Value) -> Result<Self, TargetImportIssue> {
        Self::parse(target, raw, false)
    }

    fn parse_for_match(target: TargetKind, raw: &Value) -> Result<Self, TargetImportIssue> {
        Self::parse(target, raw, true)
    }

    fn parse(
        target: TargetKind,
        raw: &Value,
        allow_custom_protocol: bool,
    ) -> Result<Self, TargetImportIssue> {
        let object = raw.as_object().ok_or_else(|| {
            issue(
                target,
                None,
                "invalidParameters",
                "The model entry must be a JSON object".to_string(),
            )
        })?;
        let model_id = required_string(object.get("id"), target, None, "missingModelId")?;
        let model_ref = Some(model_id.clone());
        let raw_url = required_string(object.get("url"), target, model_ref.clone(), "missingUrl")?;
        let api_root = normalize_api_root(&raw_url).map_err(|_| {
            issue(
                target,
                model_ref.clone(),
                "invalidUrl",
                "The target model URL is not a valid HTTP or HTTPS API root".to_string(),
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
            return Err(issue(
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
                return Err(issue(
                    target,
                    model_ref,
                    "invalidParameters",
                    format!("{field} must be a boolean"),
                ));
            }
        }
        let configuration: crate::models::ModelConfiguration = serde_json::from_value(raw.clone())
            .map_err(|_| {
                issue(
                    target,
                    model_ref.clone(),
                    "invalidParameters",
                    "The target model contains invalid advanced parameters".to_string(),
                )
            })?;
        if !configuration.has_valid_numeric_values() {
            return Err(issue(
                target,
                model_ref.clone(),
                "invalidParameters",
                "Token limits and Temperature contain invalid numeric values".to_string(),
            ));
        }
        if configuration.use_custom_protocol && !allow_custom_protocol {
            return Err(issue(
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
            metadata_object.insert("everybuddySource".to_string(), json!("targetImport"));
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
                .collect::<serde_json::Map<_, _>>();
            if !identity_override.is_empty() {
                metadata_object.insert(
                    "everybuddyIdentityOverride".to_string(),
                    Value::Object(identity_override),
                );
            }
        }
        let (mut capabilities, evidence) =
            CapabilityResolver::resolve(&model_id, &metadata, &imported_evidence);
        capabilities.reasoning_efforts = imported_capabilities.reasoning_efforts;
        let configuration = configuration_from_metadata(&model_id, raw, &capabilities);
        let name = explicit_name.unwrap_or_else(|| model_id.clone());
        let vendor = explicit_vendor.unwrap_or_else(|| infer_vendor(&model_id));
        let signature = fingerprint(
            serde_json::to_vec(&json!({
                "name": name,
                "vendor": vendor,
                "capabilities": capabilities,
                "configuration": configuration,
            }))
            .expect("import signature is serializable")
            .as_slice(),
        );
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

    fn identity_hash(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.api_root.as_bytes());
        hasher.update([0]);
        hasher.update(self.token.as_bytes());
        hasher.update([0]);
        hasher.update(self.model_id.as_bytes());
        hex::encode(hasher.finalize())
    }

    fn into_model(self, gateway_id: &str) -> ManagedModel {
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
            issue(
                target,
                model_id,
                code,
                format!("Required field for {code} is missing"),
            )
        })
}

fn issue(
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

fn imported_gateway_name(target: TargetKind, api_root: &str) -> String {
    Url::parse(api_root)
        .ok()
        .and_then(|url| url.host_str().map(ToString::to_string))
        .map(|host| format!("{host} (Imported)"))
        .unwrap_or_else(|| format!("{} Import", target.display_name()))
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

#[cfg(test)]
#[path = "target_import_tests.rs"]
mod tests;
