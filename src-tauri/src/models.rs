use std::collections::HashMap;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GatewayProfile {
    pub id: String,
    pub name: String,
    pub api_root: String,
    pub token_ref: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayInput {
    pub id: Option<String>,
    pub name: String,
    pub base_url: String,
    pub token: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManualModelInput {
    pub gateway_id: String,
    pub id: String,
    pub name: String,
    pub vendor: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CapabilitySet {
    pub supports_tool_call: bool,
    pub supports_images: bool,
    pub supports_reasoning: bool,
    #[serde(default)]
    pub reasoning_efforts: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningEffort {
    Minimal,
    Low,
    Medium,
    High,
    Xhigh,
    Max,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningSummary {
    Auto,
    Always,
    Never,
    Concise,
    Detailed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct ReasoningConfiguration {
    pub effort: Option<ReasoningEffort>,
    pub default_effort: Option<ReasoningEffort>,
    pub supported_efforts: Vec<ReasoningEffort>,
    pub summary: Option<ReasoningSummary>,
    pub can_disable_thinking: bool,
}

impl Default for ReasoningConfiguration {
    fn default() -> Self {
        Self {
            effort: None,
            default_effort: None,
            supported_efforts: Vec::new(),
            summary: None,
            can_disable_thinking: true,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct ModelConfiguration {
    pub endpoint_override: Option<String>,
    pub max_input_tokens: Option<u64>,
    pub max_output_tokens: Option<u64>,
    pub temperature: Option<f64>,
    pub only_reasoning: bool,
    pub reasoning: ReasoningConfiguration,
    pub use_custom_protocol: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub enum EvidenceSource {
    #[serde(alias = "catalog")]
    Default,
    Metadata,
    OpenRouter,
    Imported,
    Probe,
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityEvidence {
    pub capability: String,
    pub value: bool,
    pub source: EvidenceSource,
    pub detail: String,
    pub checked_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedModel {
    pub key: String,
    pub gateway_id: String,
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub capabilities: CapabilitySet,
    #[serde(default)]
    pub configuration: ModelConfiguration,
    pub evidence: Vec<CapabilityEvidence>,
    pub metadata: Value,
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelUpdateInput {
    pub model_key: String,
    pub name: String,
    pub vendor: String,
    pub capabilities: CapabilitySet,
    pub configuration: ModelConfiguration,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum TargetKind {
    Workbuddy,
    Codebuddy,
}

impl TargetKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Workbuddy => "workbuddy",
            Self::Codebuddy => "codebuddy",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Workbuddy => "WorkBuddy",
            Self::Codebuddy => "CodeBuddy",
        }
    }
}

impl FromStr for TargetKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "workbuddy" => Ok(Self::Workbuddy),
            "codebuddy" => Ok(Self::Codebuddy),
            _ => Err(format!("Unknown target kind: {value}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TargetSchema {
    Missing,
    Array,
    Wrapped,
    Invalid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetStatus {
    pub kind: TargetKind,
    pub display_name: String,
    pub path: String,
    pub installed: bool,
    pub file_exists: bool,
    pub writable: bool,
    pub schema: TargetSchema,
    pub fingerprint: Option<String>,
    pub drifted: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TargetModelState {
    pub target: TargetKind,
    pub fingerprint: Option<String>,
    pub matched_model_keys: Vec<String>,
    pub unmatched_count: usize,
    pub skipped_count: usize,
}

impl TargetModelState {
    pub fn empty(target: TargetKind) -> Self {
        Self {
            target,
            fingerprint: None,
            matched_model_keys: Vec::new(),
            unmatched_count: 0,
            skipped_count: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TargetImportIssue {
    pub target: TargetKind,
    pub model_id: Option<String>,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TargetImportReport {
    pub imported_gateway_count: usize,
    pub imported_model_count: usize,
    pub issues: Vec<TargetImportIssue>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparePublishRequest {
    pub gateway_id: String,
    pub model_ids: Vec<String>,
    pub targets: Vec<TargetKind>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetExpectation {
    pub target: TargetKind,
    pub fingerprint: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutePublishRequest {
    pub gateway_id: String,
    pub model_ids: Vec<String>,
    pub targets: Vec<TargetKind>,
    pub expectations: Vec<TargetExpectation>,
    pub accept_conflicts: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelConflict {
    pub target: TargetKind,
    pub model_id: String,
    pub existing_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetPreview {
    pub target: TargetKind,
    pub path: String,
    pub fingerprint: Option<String>,
    pub add_count: usize,
    pub update_count: usize,
    pub unchanged_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishPreview {
    pub targets: Vec<TargetPreview>,
    pub conflicts: Vec<ModelConflict>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetPublishResult {
    pub target: TargetKind,
    pub success: bool,
    pub rollback_attempted: bool,
    pub rolled_back: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishResult {
    pub success: bool,
    pub results: Vec<TargetPublishResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupRecord {
    pub id: String,
    pub target: TargetKind,
    pub path: String,
    pub source_path: String,
    pub fingerprint: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub language: String,
    pub theme: String,
    pub selected_targets: Vec<TargetKind>,
    pub target_paths: HashMap<TargetKind, String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveSettingsInput {
    pub language: String,
    pub theme: String,
    pub selected_targets: Vec<TargetKind>,
    pub target_paths: HashMap<TargetKind, String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapData {
    pub gateways: Vec<GatewayProfile>,
    pub models: Vec<ManagedModel>,
    pub targets: Vec<TargetStatus>,
    pub target_model_states: Vec<TargetModelState>,
    pub import_report: TargetImportReport,
    pub settings: AppSettings,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeSummary {
    pub model: ManagedModel,
    pub request_count: usize,
    pub notes: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::EvidenceSource;

    #[test]
    fn legacy_catalog_evidence_deserializes_as_default() {
        let source: EvidenceSource = serde_json::from_str("\"catalog\"").unwrap();

        assert_eq!(source, EvidenceSource::Default);
        assert_eq!(serde_json::to_string(&source).unwrap(), "\"default\"");
    }
}
