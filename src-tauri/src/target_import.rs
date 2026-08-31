use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    sync::Arc,
};

use chrono::Utc;
use serde::Serialize;
use url::Url;
use uuid::Uuid;

use crate::{
    error::{CoreError, CoreResult},
    gateway::{normalize_api_root, normalize_request_url},
    gateway_service::{gateway_source_hash, source_identity_key},
    models::{
        GatewayProfile, ManagedModel, TargetImportIssue, TargetImportReport, TargetKind,
        TargetModelState, TargetSnapshot,
    },
    secrets::{SecretStore, MISSING_SECRET_MESSAGE},
    store::Store,
    target::{target_inspections, TargetInspection},
    target_codec::{DecodedTargetModel as ParsedEntry, ModelIdentity},
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetImportResult {
    pub targets: Vec<crate::models::TargetStatus>,
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
        let inspections = target_inspections(self.store, self.paths)?;
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

                for inspection in &inspections {
                    let import =
                        self.import_target(inspection, &mut context, &mut baselines, &mut report);
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
        let context = ImportContext::load(self.store, Arc::clone(&self.secrets))?;
        let states = target_model_states_from_inspections(&inspections, &context)?;
        let targets = inspections
            .into_iter()
            .map(|inspection| inspection.status)
            .collect();
        Ok(TargetImportResult {
            targets,
            states,
            report,
        })
    }

    fn import_target(
        &self,
        inspection: &TargetInspection,
        context: &mut ImportContext,
        baselines: &mut HashMap<String, String>,
        report: &mut TargetImportReport,
    ) -> CoreResult<()> {
        let target = inspection.status.kind;
        if !inspection.status.file_exists {
            return Ok(());
        }
        let document = match inspection.document.as_ref() {
            Some(document) => document,
            None => {
                report.issues.push(issue(
                    target,
                    None,
                    "targetReadFailed",
                    inspection
                        .status
                        .error
                        .clone()
                        .unwrap_or_else(|| "Could not read target configuration".to_string()),
                ));
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
        context.add_model(model.clone());
        context.new_models.push(model);
        Ok(())
    }
}

#[cfg(test)]
pub fn get_target_model_states(
    store: &Store,
    secrets: Arc<dyn SecretStore>,
    paths: &HashMap<TargetKind, String>,
) -> CoreResult<Vec<TargetModelState>> {
    Ok(get_target_snapshot(store, secrets, paths)?.target_model_states)
}

pub fn get_target_snapshot(
    store: &Store,
    secrets: Arc<dyn SecretStore>,
    paths: &HashMap<TargetKind, String>,
) -> CoreResult<TargetSnapshot> {
    let inspections = target_inspections(store, paths)?;
    let context = ImportContext::load(store, secrets)?;
    let target_model_states = target_model_states_from_inspections(&inspections, &context)?;
    let targets = inspections
        .into_iter()
        .map(|inspection| inspection.status)
        .collect();
    Ok(TargetSnapshot {
        targets,
        target_model_states,
    })
}

fn target_model_states_from_inspections(
    inspections: &[TargetInspection],
    context: &ImportContext,
) -> CoreResult<Vec<TargetModelState>> {
    inspections
        .iter()
        .map(|inspection| match_target_state(inspection, context))
        .collect()
}

fn match_target_state(
    inspection: &TargetInspection,
    context: &ImportContext,
) -> CoreResult<TargetModelState> {
    let target = inspection.status.kind;
    let Some(document) = inspection.document.as_ref() else {
        let mut state = TargetModelState::empty(target);
        state.fingerprint = inspection.status.fingerprint.clone();
        state.skipped_count = usize::from(inspection.status.file_exists);
        return Ok(state);
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
        fingerprint: inspection.status.fingerprint.clone(),
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
    model_identity_index: HashMap<ModelIdentity, Vec<String>>,
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
        let mut context = Self {
            secrets,
            gateways,
            models,
            new_gateways: Vec::new(),
            new_models: Vec::new(),
            written_secret_refs: Vec::new(),
            deleted_sources,
            identity_key,
            source_identities: Vec::new(),
            model_identity_index: HashMap::new(),
        };
        context.rebuild_model_identity_index();
        Ok(context)
    }

    fn exact_model_keys(&self, entry: &ParsedEntry) -> Vec<String> {
        self.model_identity_index
            .get(&entry.model_identity())
            .cloned()
            .unwrap_or_default()
    }

    fn add_model(&mut self, model: ManagedModel) {
        let identity = self.model_identity(&model);
        let model_key = model.key.clone();
        self.models.push(model);
        if let Some(identity) = identity {
            self.model_identity_index
                .entry(identity)
                .or_default()
                .push(model_key);
        }
    }

    fn rebuild_model_identity_index(&mut self) {
        let mut index: HashMap<ModelIdentity, Vec<String>> = HashMap::new();
        for model in &self.models {
            let Some(identity) = self.model_identity(model) else {
                continue;
            };
            index.entry(identity).or_default().push(model.key.clone());
        }
        self.model_identity_index = index;
    }

    fn model_identity(&self, model: &ManagedModel) -> Option<ModelIdentity> {
        let gateway = self
            .gateways
            .iter()
            .find(|gateway| gateway.profile.id == model.gateway_id)?;
        let token = gateway.token.as_ref()?;
        let api_root = self.model_effective_root(model)?;
        Some(ModelIdentity::exact(
            model.id.clone(),
            api_root,
            token.clone(),
            model.configuration.use_custom_protocol,
        ))
    }

    fn model_effective_root(&self, model: &ManagedModel) -> Option<String> {
        let gateway = self
            .gateways
            .iter()
            .find(|gateway| gateway.profile.id == model.gateway_id)?;
        let endpoint = model
            .configuration
            .endpoint_override
            .as_deref()
            .unwrap_or(&gateway.profile.api_root);
        if model.configuration.use_custom_protocol {
            normalize_request_url(endpoint).ok()
        } else {
            normalize_api_root(endpoint).ok()
        }
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
            self.rebuild_model_identity_index();
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

#[cfg(test)]
#[path = "target_import_tests.rs"]
mod tests;
