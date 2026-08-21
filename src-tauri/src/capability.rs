use std::collections::BTreeMap;

use chrono::Utc;
use serde_json::Value;

use crate::models::{
    CapabilityEvidence, CapabilitySet, EvidenceSource, ManagedModel, ModelConfiguration,
    ReasoningConfiguration, ReasoningEffort, ReasoningSummary,
};

#[derive(Debug, Clone)]
pub struct CapabilityResolver;

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

        let capabilities = CapabilitySet {
            supports_tool_call: chosen.get("toolCall").is_some_and(|item| item.value),
            supports_images: chosen.get("images").is_some_and(|item| item.value),
            supports_reasoning: chosen.get("reasoning").is_some_and(|item| item.value),
            reasoning_efforts: reasoning_efforts(metadata),
        };

        (capabilities, evidence)
    }

    pub fn apply_manual(model: &mut ManagedModel, capabilities: CapabilitySet) {
        let now = Utc::now().to_rfc3339();
        model
            .evidence
            .retain(|item| item.source != EvidenceSource::Manual);
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
    if id.contains("gpt-5") || id.starts_with("o1") || id.starts_with("o3") || id.starts_with("o4")
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
    if id.contains("thinking") || id.contains("reasoner") || id.contains("deepseek-r1") {
        result.push(evidence(
            "reasoning",
            true,
            EvidenceSource::Catalog,
            "Model identifier hint",
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

    mappings
        .into_iter()
        .filter_map(|(capability, keys)| {
            keys.into_iter().find_map(|key| {
                metadata
                    .get(key)
                    .and_then(Value::as_bool)
                    .map(|value| evidence(capability, value, EvidenceSource::Metadata, key, now))
            })
        })
        .collect()
}

fn reasoning_efforts(metadata: &Value) -> Vec<String> {
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
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .filter(|value| parse_reasoning_effort(value).is_some())
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
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
    metadata: &Value,
    capabilities: &CapabilitySet,
) -> ModelConfiguration {
    let reasoning_metadata = metadata.get("reasoning").filter(|value| value.is_object());
    let supported_efforts = reasoning_metadata
        .and_then(|reasoning| reasoning.get("supportedEfforts"))
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
        .map(|values| {
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
        .unwrap_or_else(|| {
            capabilities
                .reasoning_efforts
                .iter()
                .filter_map(|value| parse_reasoning_effort(value))
                .collect()
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
                .and_then(parse_reasoning_effort),
            supported_efforts,
            summary: reasoning_metadata
                .and_then(|value| value.get("summary"))
                .and_then(Value::as_str)
                .and_then(parse_reasoning_summary),
            can_disable_thinking: reasoning_metadata
                .and_then(|value| value.get("canDisableThinking"))
                .and_then(Value::as_bool)
                .unwrap_or(true),
        },
        use_custom_protocol: optional_bool(metadata, &["useCustomProtocol", "use_custom_protocol"])
            .unwrap_or(false),
    }
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

        let configuration = configuration_from_metadata(&metadata, &capabilities);

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
}
