use std::collections::BTreeMap;

use chrono::Utc;
use serde_json::Value;

use crate::{
    market_catalog::{normalize_vendor, MarketModel},
    models::{
        CapabilityEvidence, CapabilitySet, EvidenceSource, ManagedModel, ModelConfiguration,
        ReasoningConfiguration, ReasoningEffort, ReasoningSummary,
    },
};

#[derive(Debug, Clone)]
pub struct CapabilityResolver;

impl CapabilityResolver {
    pub fn resolve(
        model_id: &str,
        metadata: &Value,
        existing: &[CapabilityEvidence],
    ) -> (CapabilitySet, Vec<CapabilityEvidence>) {
        Self::resolve_with_market(model_id, metadata, None, existing)
    }

    pub fn resolve_with_market(
        _model_id: &str,
        metadata: &Value,
        market_model: Option<&MarketModel>,
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

        evidence.extend(metadata_evidence(metadata, &now));
        if let Some(market_model) = market_model {
            evidence.extend(market_evidence(market_model, &now));
        }
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
                market_model
                    .and_then(market_reasoning_efforts)
                    .map(|efforts| {
                        efforts
                            .into_iter()
                            .map(reasoning_effort_name)
                            .map(ToString::to_string)
                            .collect()
                    })
                    .unwrap_or_else(|| reasoning_efforts(metadata))
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

fn market_evidence(model: &MarketModel, now: &str) -> Vec<CapabilityEvidence> {
    vec![
        evidence(
            "toolCall",
            model.supports_tool_call(),
            EvidenceSource::OpenRouter,
            "OpenRouter supported_parameters",
            now,
        ),
        evidence(
            "images",
            model.supports_images(),
            EvidenceSource::OpenRouter,
            "OpenRouter input_modalities",
            now,
        ),
        evidence(
            "reasoning",
            model.supports_reasoning(),
            EvidenceSource::OpenRouter,
            "OpenRouter supported_parameters",
            now,
        ),
    ]
}

fn metadata_evidence(metadata: &Value, now: &str) -> Vec<CapabilityEvidence> {
    let mut result: Vec<_> = ["toolCall", "images", "reasoning"]
        .into_iter()
        .filter_map(|capability| {
            metadata_capability(metadata, capability).map(|(value, detail)| {
                evidence(capability, value, EvidenceSource::Metadata, detail, now)
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

fn metadata_capability(metadata: &Value, capability: &str) -> Option<(bool, &'static str)> {
    let (bool_paths, array_terms): (&[&str], &[&str]) = match capability {
        "toolCall" => (
            &[
                "/supports_tool_call",
                "/supportsToolCall",
                "/tool_call",
                "/capabilities/tool_call",
                "/capabilities/toolCall",
                "/capabilities/tools",
            ],
            &["tools", "tool_choice", "tool_call", "function_calling"],
        ),
        "images" => (
            &[
                "/supports_images",
                "/supportsImages",
                "/vision",
                "/capabilities/images",
                "/capabilities/vision",
            ],
            &["image", "images", "vision", "image_url"],
        ),
        "reasoning" => (
            &[
                "/supports_reasoning",
                "/supportsReasoning",
                "/capabilities/reasoning",
                "/capabilities/thinking",
            ],
            &[
                "reasoning",
                "reasoning_effort",
                "include_reasoning",
                "thinking",
            ],
        ),
        _ => return None,
    };

    if let Some((value, path)) = bool_paths.iter().find_map(|path| {
        metadata
            .pointer(path)
            .and_then(Value::as_bool)
            .map(|value| (value, *path))
    }) {
        return Some((value, path));
    }

    if capability == "reasoning"
        && metadata
            .get("reasoning")
            .and_then(Value::as_object)
            .is_some_and(|reasoning| {
                [
                    "effort",
                    "defaultEffort",
                    "supportedEfforts",
                    "summary",
                    "canDisableThinking",
                ]
                .iter()
                .any(|key| reasoning.contains_key(*key))
            })
    {
        return Some((true, "/reasoning"));
    }

    let array_paths: &[&str] = match capability {
        "images" => &[
            "/input_modalities",
            "/inputModalities",
            "/architecture/input_modalities",
            "/capabilities",
            "/features",
        ],
        _ => &[
            "/supported_parameters",
            "/supportedParameters",
            "/capabilities",
            "/features",
        ],
    };
    array_paths.iter().find_map(|path| {
        metadata
            .pointer(path)
            .and_then(Value::as_array)
            .and_then(|values| {
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .any(|value| {
                        array_terms
                            .iter()
                            .any(|term| value.eq_ignore_ascii_case(term))
                    })
                    .then_some((true, *path))
            })
    })
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

fn market_reasoning_efforts(model: &MarketModel) -> Option<Vec<ReasoningEffort>> {
    model.supported_reasoning_efforts().map(|efforts| {
        efforts
            .iter()
            .filter_map(|effort| parse_reasoning_effort(effort))
            .fold(Vec::new(), |mut efforts, effort| {
                if !efforts.contains(&effort) {
                    efforts.push(effort);
                }
                efforts
            })
    })
}

fn reasoning_efforts(metadata: &Value) -> Vec<String> {
    metadata_reasoning_efforts(metadata)
        .unwrap_or_default()
        .into_iter()
        .map(reasoning_effort_name)
        .map(ToString::to_string)
        .collect()
}

pub fn infer_vendor(model_id: &str) -> String {
    model_id
        .split_once('/')
        .and_then(|(namespace, _)| normalize_vendor(namespace))
        .unwrap_or_else(|| "custom".to_string())
}

pub fn infer_vendor_from_metadata(model_id: &str, metadata: &Value) -> String {
    ["vendor", "provider", "owned_by", "ownedBy", "organization"]
        .into_iter()
        .find_map(|key| metadata_vendor(metadata.get(key)))
        .and_then(normalize_vendor)
        .unwrap_or_else(|| infer_vendor(model_id))
}

fn metadata_vendor(value: Option<&Value>) -> Option<&str> {
    match value {
        Some(Value::String(value)) => Some(value),
        Some(Value::Object(value)) => value
            .get("id")
            .or_else(|| value.get("name"))
            .and_then(Value::as_str),
        _ => None,
    }
}

pub fn configuration_from_metadata(
    model_id: &str,
    metadata: &Value,
    capabilities: &CapabilitySet,
) -> ModelConfiguration {
    configuration_from_sources(model_id, metadata, None, capabilities)
}

pub fn configuration_from_sources(
    _model_id: &str,
    metadata: &Value,
    market_model: Option<&MarketModel>,
    capabilities: &CapabilitySet,
) -> ModelConfiguration {
    if market_model.is_some_and(|model| !model.supports_chat_configuration()) {
        return ModelConfiguration::default();
    }

    let reasoning_metadata = metadata.get("reasoning").filter(|value| value.is_object());
    let metadata_supported_efforts = metadata_reasoning_efforts(metadata);
    let market_supported_efforts = market_model.and_then(market_reasoning_efforts);
    let supported_efforts = market_supported_efforts
        .or(metadata_supported_efforts)
        .unwrap_or_else(|| {
            capabilities
                .reasoning_efforts
                .iter()
                .filter_map(|value| parse_reasoning_effort(value))
                .collect()
        });
    let default_effort = market_model
        .and_then(MarketModel::default_reasoning_effort)
        .map(parse_reasoning_effort)
        .unwrap_or_else(|| {
            reasoning_metadata
                .and_then(|value| value.get("defaultEffort"))
                .and_then(Value::as_str)
                .and_then(parse_reasoning_effort)
        })
        .filter(|effort| supported_efforts.is_empty() || supported_efforts.contains(effort));

    ModelConfiguration {
        endpoint_override: None,
        max_input_tokens: market_model
            .and_then(MarketModel::max_input_tokens)
            .or_else(|| {
                optional_u64(
                    metadata,
                    &["maxInputTokens", "max_input_tokens", "context_window"],
                )
            }),
        max_output_tokens: market_model
            .and_then(MarketModel::max_output_tokens)
            .or_else(|| {
                optional_u64(
                    metadata,
                    &["maxOutputTokens", "max_output_tokens", "max_tokens"],
                )
            }),
        temperature: market_model
            .and_then(MarketModel::temperature)
            .or_else(|| optional_f64(metadata, &["temperature"])),
        only_reasoning: optional_bool(metadata, &["onlyReasoning", "only_reasoning"])
            .unwrap_or(false),
        reasoning: ReasoningConfiguration {
            effort: reasoning_metadata
                .and_then(|value| value.get("effort"))
                .and_then(Value::as_str)
                .and_then(parse_reasoning_effort),
            default_effort,
            supported_efforts,
            summary: reasoning_metadata
                .and_then(|value| value.get("summary"))
                .and_then(Value::as_str)
                .and_then(parse_reasoning_summary),
            can_disable_thinking: market_model
                .and_then(MarketModel::can_disable_thinking)
                .or_else(|| {
                    reasoning_metadata
                        .and_then(|value| value.get("canDisableThinking"))
                        .and_then(Value::as_bool)
                })
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
    let normalized = value.trim().to_ascii_lowercase().replace(['-', ' '], "_");
    match normalized.as_str() {
        "minimal" => Some(ReasoningEffort::Minimal),
        "low" => Some(ReasoningEffort::Low),
        "medium" => Some(ReasoningEffort::Medium),
        "high" => Some(ReasoningEffort::High),
        "xhigh" | "x_high" | "extra_high" => Some(ReasoningEffort::Xhigh),
        "max" | "maximum" => Some(ReasoningEffort::Max),
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
        let metadata = serde_json::json!({
            "supports_tool_call": true,
            "supports_images": true,
            "supports_reasoning": true
        });
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
    fn resolves_openrouter_style_gateway_metadata() {
        let metadata = serde_json::json!({
            "supported_parameters": ["tools", "reasoning_effort"],
            "architecture": {"input_modalities": ["text", "image"]}
        });

        let (capabilities, evidence) = CapabilityResolver::resolve("private-model", &metadata, &[]);

        assert!(capabilities.supports_tool_call);
        assert!(capabilities.supports_images);
        assert!(capabilities.supports_reasoning);
        assert!(evidence.iter().any(|item| {
            item.capability == "images"
                && item.source == EvidenceSource::Metadata
                && item.detail == "/architecture/input_modalities"
        }));
    }

    #[test]
    fn openrouter_capabilities_override_gateway_metadata() {
        let metadata = serde_json::json!({
            "supportsImages": true,
            "supportsToolCall": true,
            "supportsReasoning": true
        });
        let market_model: MarketModel = serde_json::from_value(serde_json::json!({
            "id": "openai/gpt-5.6",
            "architecture": {
                "input_modalities": ["text"],
                "output_modalities": ["text"]
            },
            "supported_parameters": []
        }))
        .unwrap();

        let (capabilities, _) =
            CapabilityResolver::resolve_with_market("gpt-5.6", &metadata, Some(&market_model), &[]);

        assert!(!capabilities.supports_tool_call);
        assert!(!capabilities.supports_images);
        assert!(!capabilities.supports_reasoning);
    }

    #[test]
    fn maps_openrouter_reasoning_metadata_to_workbuddy_configuration() {
        let market_model: MarketModel = serde_json::from_value(serde_json::json!({
            "id": "openai/gpt-5.6-sol",
            "context_length": 1_050_000,
            "architecture": {
                "input_modalities": ["file", "image", "text"],
                "output_modalities": ["text"]
            },
            "top_provider": {
                "context_length": 1_050_000,
                "max_completion_tokens": 128_000
            },
            "supported_parameters": [
                "include_reasoning",
                "reasoning",
                "reasoning_effort",
                "tool_choice",
                "tools"
            ],
            "default_parameters": {
                "temperature": null
            },
            "reasoning": {
                "mandatory": false,
                "default_enabled": true,
                "supported_efforts": ["max", "xhigh", "high", "medium", "low", "none"],
                "default_effort": "medium"
            }
        }))
        .unwrap();
        let (capabilities, _) = CapabilityResolver::resolve_with_market(
            "gpt-5.6-sol",
            &Value::Null,
            Some(&market_model),
            &[],
        );

        let configuration = configuration_from_sources(
            "gpt-5.6-sol",
            &Value::Null,
            Some(&market_model),
            &capabilities,
        );

        assert!(capabilities.supports_tool_call);
        assert!(capabilities.supports_images);
        assert!(capabilities.supports_reasoning);
        assert_eq!(
            capabilities.reasoning_efforts,
            vec!["max", "xhigh", "high", "medium", "low"]
        );
        assert_eq!(configuration.max_input_tokens, Some(1_050_000));
        assert_eq!(configuration.max_output_tokens, Some(128_000));
        assert_eq!(configuration.temperature, None);
        assert_eq!(
            configuration.reasoning.supported_efforts,
            vec![
                ReasoningEffort::Max,
                ReasoningEffort::Xhigh,
                ReasoningEffort::High,
                ReasoningEffort::Medium,
                ReasoningEffort::Low,
            ]
        );
        assert_eq!(
            configuration.reasoning.default_effort,
            Some(ReasoningEffort::Medium)
        );
        assert!(configuration.reasoning.can_disable_thinking);
    }

    #[test]
    fn openrouter_configuration_overrides_gateway_metadata_with_fallbacks() {
        let metadata = serde_json::json!({
            "maxInputTokens": 8_192,
            "maxOutputTokens": 2_048,
            "temperature": 0.2,
            "reasoning": {
                "defaultEffort": "low",
                "supportedEfforts": ["low"],
                "canDisableThinking": true
            }
        });
        let market_model: MarketModel = serde_json::from_value(serde_json::json!({
            "id": "provider/reasoning-model",
            "architecture": {
                "input_modalities": ["text"],
                "output_modalities": ["text"]
            },
            "top_provider": {
                "context_length": 262_144,
                "max_completion_tokens": 32_768
            },
            "supported_parameters": ["reasoning_effort"],
            "default_parameters": {
                "temperature": 0.7
            },
            "reasoning": {
                "mandatory": true,
                "supported_efforts": ["medium", "high"],
                "default_effort": "high"
            }
        }))
        .unwrap();
        let capabilities = CapabilitySet {
            supports_reasoning: true,
            ..Default::default()
        };

        let configuration = configuration_from_sources(
            "reasoning-model",
            &metadata,
            Some(&market_model),
            &capabilities,
        );

        assert_eq!(configuration.max_input_tokens, Some(262_144));
        assert_eq!(configuration.max_output_tokens, Some(32_768));
        assert_eq!(configuration.temperature, Some(0.7));
        assert_eq!(
            configuration.reasoning.supported_efforts,
            vec![ReasoningEffort::Medium, ReasoningEffort::High]
        );
        assert_eq!(
            configuration.reasoning.default_effort,
            Some(ReasoningEffort::High)
        );
        assert!(!configuration.reasoning.can_disable_thinking);
    }

    #[test]
    fn maps_none_default_effort_to_disabled_reasoning() {
        let market_model: MarketModel = serde_json::from_value(serde_json::json!({
            "id": "provider/optional-reasoning-model",
            "architecture": {
                "input_modalities": ["text"],
                "output_modalities": ["text"]
            },
            "reasoning": {
                "mandatory": false,
                "supported_efforts": ["high", "none"],
                "default_effort": "none"
            }
        }))
        .unwrap();
        let capabilities = CapabilitySet {
            supports_reasoning: true,
            ..Default::default()
        };

        let configuration = configuration_from_sources(
            "optional-reasoning-model",
            &Value::Null,
            Some(&market_model),
            &capabilities,
        );

        assert_eq!(
            configuration.reasoning.supported_efforts,
            vec![ReasoningEffort::High]
        );
        assert_eq!(configuration.reasoning.default_effort, None);
        assert!(configuration.reasoning.can_disable_thinking);
    }

    #[test]
    fn does_not_project_non_text_catalog_parameters_to_chat_configuration() {
        let market_model: MarketModel = serde_json::from_value(serde_json::json!({
            "id": "sourceful/riverflow-v2.5-pro",
            "context_length": 0,
            "architecture": {
                "input_modalities": ["text", "image"],
                "output_modalities": ["image"]
            },
            "top_provider": {
                "context_length": 0,
                "max_completion_tokens": 0
            },
            "supported_parameters": ["include_reasoning", "reasoning", "reasoning_effort"],
            "default_parameters": {
                "temperature": 0.7
            },
            "reasoning": {
                "mandatory": true,
                "supported_efforts": ["xhigh", "high", "medium", "low"],
                "default_effort": "medium"
            }
        }))
        .unwrap();
        let (capabilities, _) = CapabilityResolver::resolve_with_market(
            "riverflow-v2.5-pro",
            &Value::Null,
            Some(&market_model),
            &[],
        );
        let configuration = configuration_from_sources(
            "riverflow-v2.5-pro",
            &Value::Null,
            Some(&market_model),
            &capabilities,
        );

        assert_eq!(capabilities, CapabilitySet::default());
        assert_eq!(configuration, ModelConfiguration::default());
    }

    #[test]
    fn records_openrouter_as_the_capability_source() {
        let market_model: MarketModel = serde_json::from_value(serde_json::json!({
            "id": "openai/gpt-5.6",
            "architecture": {
                "input_modalities": ["text"],
                "output_modalities": ["text"]
            },
            "supported_parameters": []
        }))
        .unwrap();

        let (capabilities, evidence) = CapabilityResolver::resolve_with_market(
            "gpt-5.6",
            &Value::Null,
            Some(&market_model),
            &[],
        );

        assert!(!capabilities.supports_tool_call);
        assert!(!capabilities.supports_images);
        assert!(!capabilities.supports_reasoning);
        assert!(evidence.iter().any(|item| {
            item.capability == "reasoning"
                && item.source == EvidenceSource::OpenRouter
                && item.detail == "OpenRouter supported_parameters"
                && !item.value
        }));
    }

    #[test]
    fn normalizes_reasoning_effort_aliases_from_metadata() {
        let metadata = serde_json::json!({
            "reasoning": {
                "supportedEfforts": ["x-high", "extra_high", "maximum"]
            }
        });
        let capabilities = CapabilitySet {
            supports_reasoning: true,
            ..Default::default()
        };

        let configuration = configuration_from_metadata("private-model", &metadata, &capabilities);

        assert_eq!(
            configuration.reasoning.supported_efforts,
            vec![ReasoningEffort::Xhigh, ReasoningEffort::Max]
        );
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
    fn metadata_supplies_explicit_reasoning_efforts() {
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
    fn explicit_empty_metadata_efforts_stay_empty() {
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
