use std::collections::BTreeMap;

use chrono::Utc;
use serde_json::Value;

use crate::models::{
    CapabilityEvidence, CapabilitySet, EvidenceSource, ManagedModel, ModelConfiguration,
    ReasoningConfiguration, ReasoningEffort, ReasoningSummary,
};

#[derive(Debug, Clone)]
pub struct CapabilityResolver;

#[derive(Debug, Clone, Copy)]
struct ReasoningPreset {
    default_effort: ReasoningEffort,
    supported_efforts: &'static [ReasoningEffort],
    can_disable_thinking: bool,
}

const DEEPSEEK_REASONING_EFFORTS: &[ReasoningEffort] =
    &[ReasoningEffort::High, ReasoningEffort::Max];
const KIMI_K3_REASONING_EFFORTS: &[ReasoningEffort] = &[
    ReasoningEffort::Low,
    ReasoningEffort::High,
    ReasoningEffort::Max,
];

impl CapabilityResolver {
    pub fn resolve(
        model_id: &str,
        metadata: &Value,
        existing: &[CapabilityEvidence],
    ) -> (CapabilitySet, Vec<CapabilityEvidence>) {
        let now = Utc::now().to_rfc3339();
        let mut evidence = vec![
            evidence(
                "toolCall",
                false,
                EvidenceSource::Default,
                "Conservative default",
                &now,
            ),
            evidence(
                "images",
                false,
                EvidenceSource::Default,
                "Conservative default",
                &now,
            ),
            evidence(
                "reasoning",
                false,
                EvidenceSource::Default,
                "Conservative default",
                &now,
            ),
        ];

        evidence.extend(catalog_evidence(model_id, &now));
        evidence.extend(metadata_evidence(metadata, &now));
        evidence.extend(existing.iter().cloned());

        let mut chosen: BTreeMap<&str, &CapabilityEvidence> = BTreeMap::new();
        for item in &evidence {
            let replace = chosen
                .get(item.capability.as_str())
                .map(|current| item.source >= current.source)
                .unwrap_or(true);
            if replace {
                chosen.insert(item.capability.as_str(), item);
            }
        }

        let supports_reasoning = chosen.get("reasoning").is_some_and(|item| item.value);
        let capabilities = CapabilitySet {
            supports_tool_call: chosen.get("toolCall").is_some_and(|item| item.value),
            supports_images: chosen.get("images").is_some_and(|item| item.value),
            supports_reasoning,
            reasoning_efforts: if supports_reasoning {
                reasoning_efforts(model_id, metadata)
            } else {
                Vec::new()
            },
        };

        (capabilities, evidence)
    }

    pub fn apply_manual(model: &mut ManagedModel, capabilities: CapabilitySet) {
        let now = Utc::now().to_rfc3339();
        model.evidence.retain(|item| {
            item.source != EvidenceSource::Manual
                || !matches!(
                    item.capability.as_str(),
                    "toolCall" | "images" | "reasoning"
                )
        });
        model.evidence.extend([
            evidence(
                "toolCall",
                capabilities.supports_tool_call,
                EvidenceSource::Manual,
                "User override",
                &now,
            ),
            evidence(
                "images",
                capabilities.supports_images,
                EvidenceSource::Manual,
                "User override",
                &now,
            ),
            evidence(
                "reasoning",
                capabilities.supports_reasoning,
                EvidenceSource::Manual,
                "User override",
                &now,
            ),
        ]);
        model.capabilities = capabilities;
        model.updated_at = now;
    }
}

pub fn evidence(
    capability: &str,
    value: bool,
    source: EvidenceSource,
    detail: &str,
    checked_at: &str,
) -> CapabilityEvidence {
    CapabilityEvidence {
        capability: capability.to_string(),
        value,
        source,
        detail: detail.to_string(),
        checked_at: checked_at.to_string(),
    }
}

fn catalog_evidence(model_id: &str, now: &str) -> Vec<CapabilityEvidence> {
    let id = model_id.to_ascii_lowercase();
    let catalog_id = catalog_model_id(&id);
    let mut result = Vec::new();

    if id.contains("gpt-4o") || id.contains("gpt-4.1") || id.contains("gpt-5") {
        result.push(evidence(
            "toolCall",
            true,
            EvidenceSource::Catalog,
            "Known model family",
            now,
        ));
        result.push(evidence(
            "images",
            true,
            EvidenceSource::Catalog,
            "Known model family",
            now,
        ));
    }
    if id.contains("gpt-5")
        || catalog_id.starts_with("o1")
        || catalog_id.starts_with("o3")
        || catalog_id.starts_with("o4")
    {
        result.push(evidence(
            "reasoning",
            true,
            EvidenceSource::Catalog,
            "Known reasoning family",
            now,
        ));
    }
    if id.contains("claude-3") || id.contains("claude-4") {
        result.push(evidence(
            "toolCall",
            true,
            EvidenceSource::Catalog,
            "Known model family",
            now,
        ));
        result.push(evidence(
            "images",
            true,
            EvidenceSource::Catalog,
            "Known model family",
            now,
        ));
    }
    if reasoning_preset(model_id).is_some()
        || id.contains("thinking")
        || id.contains("reasoner")
        || id.contains("deepseek-r1")
    {
        result.push(evidence(
            "reasoning",
            true,
            EvidenceSource::Catalog,
            if reasoning_preset(model_id).is_some() {
                "Built-in reasoning preset"
            } else {
                "Model identifier hint"
            },
            now,
        ));
    }

    result
}

fn metadata_evidence(metadata: &Value, now: &str) -> Vec<CapabilityEvidence> {
    let mappings = [
        (
            "toolCall",
            ["supports_tool_call", "supportsToolCall", "tool_call"],
        ),
        ("images", ["supports_images", "supportsImages", "vision"]),
        (
            "reasoning",
            ["supports_reasoning", "supportsReasoning", "reasoning"],
        ),
    ];

    let mut result: Vec<_> = mappings
        .into_iter()
        .filter_map(|(capability, keys)| {
            keys.into_iter().find_map(|key| {
                metadata
                    .get(key)
                    .and_then(Value::as_bool)
                    .map(|value| evidence(capability, value, EvidenceSource::Metadata, key, now))
            })
        })
        .collect();

    let has_explicit_reasoning = result.iter().any(|item| item.capability == "reasoning");
    if !has_explicit_reasoning
        && metadata_reasoning_efforts(metadata).is_some_and(|efforts| !efforts.is_empty())
    {
        result.push(evidence(
            "reasoning",
            true,
            EvidenceSource::Metadata,
            "reasoning.supportedEfforts",
            now,
        ));
    }

    result
}

fn reasoning_effort_values(metadata: &Value) -> Option<&Vec<Value>> {
    metadata
        .pointer("/reasoning/supportedEfforts")
        .and_then(Value::as_array)
        .or_else(|| {
            [
                "reasoning_efforts",
                "supported_reasoning_efforts",
                "supportedEfforts",
            ]
            .into_iter()
            .find_map(|key| metadata.get(key).and_then(Value::as_array))
        })
}

fn metadata_reasoning_efforts(metadata: &Value) -> Option<Vec<ReasoningEffort>> {
    reasoning_effort_values(metadata).map(|values| {
        values
            .iter()
            .filter_map(Value::as_str)
            .filter_map(parse_reasoning_effort)
            .fold(Vec::new(), |mut efforts, effort| {
                if !efforts.contains(&effort) {
                    efforts.push(effort);
                }
                efforts
            })
    })
}

fn reasoning_efforts(model_id: &str, metadata: &Value) -> Vec<String> {
    metadata_reasoning_efforts(metadata)
        .unwrap_or_else(|| {
            reasoning_preset(model_id)
                .map(|preset| preset.supported_efforts.to_vec())
                .unwrap_or_default()
        })
        .into_iter()
        .map(reasoning_effort_name)
        .map(ToString::to_string)
        .collect()
}

pub fn infer_vendor(model_id: &str) -> String {
    let id = model_id.to_ascii_lowercase();
    if id.contains("gpt") || id.starts_with('o') {
        "openai"
    } else if id.contains("claude") {
        "anthropic"
    } else if id.contains("gemini") {
        "google"
    } else if id.contains("deepseek") {
        "deepseek"
    } else if id.contains("qwen") {
        "qwen"
    } else {
        "custom"
    }
    .to_string()
}

pub fn configuration_from_metadata(
    model_id: &str,
    metadata: &Value,
    capabilities: &CapabilitySet,
) -> ModelConfiguration {
    let reasoning_metadata = metadata.get("reasoning").filter(|value| value.is_object());
    let metadata_supported_efforts = metadata_reasoning_efforts(metadata);
    let preset = capabilities
        .supports_reasoning
        .then(|| reasoning_preset(model_id))
        .flatten();
    let supported_efforts = metadata_supported_efforts.clone().unwrap_or_else(|| {
        preset.map_or_else(
            || {
                capabilities
                    .reasoning_efforts
                    .iter()
                    .filter_map(|value| parse_reasoning_effort(value))
                    .collect()
            },
            |value| value.supported_efforts.to_vec(),
        )
    });

    ModelConfiguration {
        endpoint_override: None,
        max_input_tokens: optional_u64(
            metadata,
            &["maxInputTokens", "max_input_tokens", "context_window"],
        ),
        max_output_tokens: optional_u64(
            metadata,
            &["maxOutputTokens", "max_output_tokens", "max_tokens"],
        ),
        temperature: optional_f64(metadata, &["temperature"]),
        only_reasoning: optional_bool(metadata, &["onlyReasoning", "only_reasoning"])
            .unwrap_or(false),
        reasoning: ReasoningConfiguration {
            effort: reasoning_metadata
                .and_then(|value| value.get("effort"))
                .and_then(Value::as_str)
                .and_then(parse_reasoning_effort),
            default_effort: reasoning_metadata
                .and_then(|value| value.get("defaultEffort"))
                .and_then(Value::as_str)
                .and_then(parse_reasoning_effort)
                .or_else(|| {
                    metadata_supported_efforts
                        .is_none()
                        .then(|| preset.map(|value| value.default_effort))
                        .flatten()
                }),
            supported_efforts,
            summary: reasoning_metadata
                .and_then(|value| value.get("summary"))
                .and_then(Value::as_str)
                .and_then(parse_reasoning_summary),
            can_disable_thinking: reasoning_metadata
                .and_then(|value| value.get("canDisableThinking"))
                .and_then(Value::as_bool)
                .or_else(|| preset.map(|value| value.can_disable_thinking))
                .unwrap_or(true),
        },
        use_custom_protocol: optional_bool(metadata, &["useCustomProtocol", "use_custom_protocol"])
            .unwrap_or(false),
    }
}

fn reasoning_preset(model_id: &str) -> Option<ReasoningPreset> {
    let id = model_id.to_ascii_lowercase();
    let id = catalog_model_id(&id);
    let deepseek = ["deepseek-v4-pro", "deepseek-v4-flash", "deepseek-reasoner"]
        .into_iter()
        .any(|family| matches_model_family(id, family));

    if deepseek {
        Some(ReasoningPreset {
            default_effort: ReasoningEffort::High,
            supported_efforts: DEEPSEEK_REASONING_EFFORTS,
            can_disable_thinking: true,
        })
    } else if id == "kimi-k3" {
        Some(ReasoningPreset {
            default_effort: ReasoningEffort::High,
            supported_efforts: KIMI_K3_REASONING_EFFORTS,
            can_disable_thinking: true,
        })
    } else {
        None
    }
}

fn catalog_model_id(model_id: &str) -> &str {
    model_id.rsplit('/').next().unwrap_or(model_id)
}

fn matches_model_family(model_id: &str, family: &str) -> bool {
    model_id == family
        || model_id
            .strip_prefix(family)
            .and_then(|suffix| suffix.strip_prefix('-'))
            .is_some_and(|suffix| {
                !suffix.is_empty() && suffix.chars().all(|character| character.is_ascii_digit())
            })
}

fn optional_u64(metadata: &Value, keys: &[&str]) -> Option<u64> {
    keys.iter()
        .find_map(|key| metadata.get(*key).and_then(Value::as_u64))
}

fn optional_f64(metadata: &Value, keys: &[&str]) -> Option<f64> {
    keys.iter()
        .find_map(|key| metadata.get(*key).and_then(Value::as_f64))
}

fn optional_bool(metadata: &Value, keys: &[&str]) -> Option<bool> {
    keys.iter()
        .find_map(|key| metadata.get(*key).and_then(Value::as_bool))
}

fn parse_reasoning_effort(value: &str) -> Option<ReasoningEffort> {
    match value {
        "minimal" => Some(ReasoningEffort::Minimal),
        "low" => Some(ReasoningEffort::Low),
        "medium" => Some(ReasoningEffort::Medium),
        "high" => Some(ReasoningEffort::High),
        "xhigh" => Some(ReasoningEffort::Xhigh),
        "max" => Some(ReasoningEffort::Max),
        _ => None,
    }
}

fn reasoning_effort_name(value: ReasoningEffort) -> &'static str {
    match value {
        ReasoningEffort::Minimal => "minimal",
        ReasoningEffort::Low => "low",
        ReasoningEffort::Medium => "medium",
        ReasoningEffort::High => "high",
        ReasoningEffort::Xhigh => "xhigh",
        ReasoningEffort::Max => "max",
    }
}

fn parse_reasoning_summary(value: &str) -> Option<ReasoningSummary> {
    match value {
        "auto" => Some(ReasoningSummary::Auto),
        "always" => Some(ReasoningSummary::Always),
        "never" => Some(ReasoningSummary::Never),
        "concise" => Some(ReasoningSummary::Concise),
        "detailed" => Some(ReasoningSummary::Detailed),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_evidence_wins_over_all_other_sources() {
        let metadata = serde_json::json!({ "supports_tool_call": true });
        let manual = evidence(
            "toolCall",
            false,
            EvidenceSource::Manual,
            "User override",
            "2026-08-20T00:00:00Z",
        );

        let (capabilities, _) = CapabilityResolver::resolve("gpt-5", &metadata, &[manual]);

        assert!(!capabilities.supports_tool_call);
        assert!(capabilities.supports_images);
        assert!(capabilities.supports_reasoning);
    }

    #[test]
    fn imported_evidence_wins_over_metadata_but_not_probe() {
        let metadata = serde_json::json!({ "supports_tool_call": true });
        let imported = evidence(
            "toolCall",
            false,
            EvidenceSource::Imported,
            "Target import",
            "2026-08-20T00:00:00Z",
        );
        let probe = evidence(
            "toolCall",
            true,
            EvidenceSource::Probe,
            "Probe",
            "2026-08-20T00:00:01Z",
        );

        let (imported_capabilities, _) = CapabilityResolver::resolve(
            "private-model",
            &metadata,
            std::slice::from_ref(&imported),
        );
        let (probed_capabilities, _) =
            CapabilityResolver::resolve("private-model", &metadata, &[imported, probe]);

        assert!(!imported_capabilities.supports_tool_call);
        assert!(probed_capabilities.supports_tool_call);
    }

    #[test]
    fn unknown_model_uses_conservative_defaults() {
        let (capabilities, _) = CapabilityResolver::resolve("private-model", &Value::Null, &[]);
        assert_eq!(capabilities, CapabilitySet::default());
    }

    #[test]
    fn resolves_complete_model_configuration_from_metadata() {
        let metadata = serde_json::json!({
            "max_input_tokens": 262144,
            "max_output_tokens": 32768,
            "temperature": 0.6,
            "only_reasoning": true,
            "use_custom_protocol": true,
            "reasoning": {
                "effort": "low",
                "defaultEffort": "high",
                "supportedEfforts": ["low", "high", "invalid"],
                "summary": "detailed",
                "canDisableThinking": false
            }
        });
        let capabilities = CapabilitySet {
            supports_reasoning: true,
            ..Default::default()
        };

        let configuration = configuration_from_metadata("custom-model", &metadata, &capabilities);

        assert_eq!(configuration.max_input_tokens, Some(262_144));
        assert_eq!(configuration.max_output_tokens, Some(32_768));
        assert_eq!(configuration.temperature, Some(0.6));
        assert!(configuration.only_reasoning);
        assert!(configuration.use_custom_protocol);
        assert_eq!(
            configuration.reasoning.supported_efforts,
            vec![ReasoningEffort::Low, ReasoningEffort::High]
        );
        assert_eq!(
            configuration.reasoning.summary,
            Some(ReasoningSummary::Detailed)
        );
        assert!(!configuration.reasoning.can_disable_thinking);
    }

    #[test]
    fn applies_deepseek_reasoning_preset_to_namespaced_and_versioned_models() {
        let model_id = "deepseek/deepseek-v4-pro-202606";
        let (capabilities, _) = CapabilityResolver::resolve(model_id, &Value::Null, &[]);
        let configuration = configuration_from_metadata(model_id, &Value::Null, &capabilities);

        assert!(capabilities.supports_reasoning);
        assert_eq!(capabilities.reasoning_efforts, vec!["high", "max"]);
        assert_eq!(
            configuration.reasoning.default_effort,
            Some(ReasoningEffort::High)
        );
        assert_eq!(
            configuration.reasoning.supported_efforts,
            vec![ReasoningEffort::High, ReasoningEffort::Max]
        );
        assert!(configuration.reasoning.can_disable_thinking);
    }

    #[test]
    fn applies_kimi_k3_reasoning_preset() {
        let model_id = "moonshotai/kimi-k3";
        let (capabilities, _) = CapabilityResolver::resolve(model_id, &Value::Null, &[]);
        let configuration = configuration_from_metadata(model_id, &Value::Null, &capabilities);

        assert!(capabilities.supports_reasoning);
        assert_eq!(
            configuration.reasoning.supported_efforts,
            vec![
                ReasoningEffort::Low,
                ReasoningEffort::High,
                ReasoningEffort::Max
            ]
        );
        assert_eq!(
            configuration.reasoning.default_effort,
            Some(ReasoningEffort::High)
        );
    }

    #[test]
    fn does_not_apply_kimi_k3_preset_to_other_versions() {
        let (capabilities, _) =
            CapabilityResolver::resolve("moonshotai/kimi-k3-1", &Value::Null, &[]);

        assert!(!capabilities.supports_reasoning);
        assert!(capabilities.reasoning_efforts.is_empty());
    }

    #[test]
    fn metadata_overrides_catalog_reasoning_preset() {
        let model_id = "deepseek-v4-pro";
        let metadata = serde_json::json!({
            "supportsReasoning": true,
            "reasoning": {
                "defaultEffort": "low",
                "supportedEfforts": ["minimal", "low"],
                "canDisableThinking": false
            }
        });
        let (capabilities, _) = CapabilityResolver::resolve(model_id, &metadata, &[]);
        let configuration = configuration_from_metadata(model_id, &metadata, &capabilities);

        assert_eq!(capabilities.reasoning_efforts, vec!["minimal", "low"]);
        assert_eq!(
            configuration.reasoning.supported_efforts,
            vec![ReasoningEffort::Minimal, ReasoningEffort::Low]
        );
        assert_eq!(
            configuration.reasoning.default_effort,
            Some(ReasoningEffort::Low)
        );
        assert!(!configuration.reasoning.can_disable_thinking);
    }

    #[test]
    fn explicit_empty_metadata_efforts_do_not_fall_back_to_catalog() {
        let model_id = "deepseek-v4-flash";
        let metadata = serde_json::json!({
            "reasoning": {"supportedEfforts": []}
        });
        let (capabilities, _) = CapabilityResolver::resolve(model_id, &metadata, &[]);
        let configuration = configuration_from_metadata(model_id, &metadata, &capabilities);

        assert!(capabilities.supports_reasoning);
        assert!(capabilities.reasoning_efforts.is_empty());
        assert!(configuration.reasoning.supported_efforts.is_empty());
        assert_eq!(configuration.reasoning.default_effort, None);
    }
}
