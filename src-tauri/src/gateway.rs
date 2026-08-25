use std::{collections::HashSet, path::PathBuf, time::Duration};

use chrono::Utc;
use reqwest::{redirect::Policy, Client, StatusCode};
use serde_json::{json, Value};
use url::{Host, Url};

use crate::{
    capability::{
        configuration_from_sources, evidence, infer_vendor_from_metadata, CapabilityResolver,
    },
    error::{CoreError, CoreResult},
    market_catalog::{MarketCatalogClient, MarketModel},
    models::{EvidenceSource, GatewayProfile, ManagedModel},
};

#[derive(Clone)]
pub struct GatewayClient {
    client: Client,
    market_catalog: MarketCatalogClient,
}

const MAX_GATEWAY_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
const MAX_DISCOVERED_MODELS: usize = 10_000;

impl GatewayClient {
    pub fn new(market_cache_path: Option<PathBuf>) -> CoreResult<Self> {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(45))
            .redirect(Policy::none())
            .user_agent(concat!("EveryBuddy/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| CoreError::Network(error.to_string()))?;
        let market_catalog =
            MarketCatalogClient::new(client.clone(), !cfg!(test), market_cache_path);
        Ok(Self {
            client,
            market_catalog,
        })
    }

    pub async fn discover(
        &self,
        profile: &GatewayProfile,
        token: &str,
        existing: &[ManagedModel],
    ) -> CoreResult<Vec<ManagedModel>> {
        let url = format!("{}/models", profile.api_root.trim_end_matches('/'));
        let response = self
            .client
            .get(url)
            .bearer_auth(token)
            .send()
            .await
            .map_err(map_reqwest_error)?;

        if response.status() == StatusCode::UNAUTHORIZED
            || response.status() == StatusCode::FORBIDDEN
        {
            return Err(CoreError::Authentication(
                "The gateway rejected the API token".to_string(),
            ));
        }
        if !response.status().is_success() {
            return Err(CoreError::Protocol(format!(
                "The models endpoint returned HTTP {}",
                response.status().as_u16()
            )));
        }

        let bytes = read_response_body(response, "The models response").await?;
        let body = parse_discovery_body(&bytes, token)?;
        let items = body["data"].as_array().expect("validated data array");
        let market_catalog = self.market_catalog.snapshot().await;
        let now = Utc::now().to_rfc3339();

        items
            .iter()
            .map(|item| {
                let id = item
                    .get("id")
                    .and_then(Value::as_str)
                    .filter(|id| !id.trim().is_empty())
                    .ok_or_else(|| {
                        CoreError::Protocol("A model entry is missing its id".to_string())
                    })?;
                let gateway_name = item
                    .get("name")
                    .or_else(|| item.get("display_name"))
                    .and_then(Value::as_str)
                    .filter(|name| !name.trim().is_empty());
                let key = format!("{}::{}", profile.id, id);
                let existing_model = existing.iter().find(|model| model.key == key);
                let preserved_evidence: Vec<_> = existing_model
                    .map(|model| {
                        model
                            .evidence
                            .iter()
                            .filter(|item| {
                                matches!(
                                    item.source,
                                    EvidenceSource::Imported
                                        | EvidenceSource::Probe
                                        | EvidenceSource::Manual
                                )
                            })
                            .cloned()
                            .collect()
                    })
                    .unwrap_or_default();
                let inferred_vendor = infer_vendor_from_metadata(id, item);
                let market_model = market_catalog
                    .as_deref()
                    .and_then(|catalog| catalog.find(id, &inferred_vendor));
                let discovered_name = gateway_name
                    .or_else(|| market_model.and_then(MarketModel::display_name))
                    .unwrap_or(id);
                let discovered_vendor = market_model
                    .and_then(MarketModel::vendor)
                    .unwrap_or(inferred_vendor);
                let identity_override = existing_model
                    .and_then(|model| model.metadata.get("everybuddyIdentityOverride"));
                let (name, vendor) =
                    resolved_identity(identity_override, discovered_name, discovered_vendor);
                let (capabilities, evidence) = CapabilityResolver::resolve_with_market(
                    id,
                    item,
                    market_model,
                    &preserved_evidence,
                );
                let configuration =
                    discovered_configuration(existing_model, id, item, market_model, &capabilities);
                let mut metadata = object_without_secret(item);
                if let Some(source) = existing_model
                    .and_then(|model| model.metadata.get("everybuddySource"))
                    .and_then(Value::as_str)
                {
                    if let Some(object) = metadata.as_object_mut() {
                        object.insert("everybuddySource".to_string(), json!(source));
                    }
                }
                if let Some(identity_override) = identity_override {
                    if let Some(object) = metadata.as_object_mut() {
                        object.insert(
                            "everybuddyIdentityOverride".to_string(),
                            identity_override.clone(),
                        );
                    }
                }
                if let (Some(object), Some(market_model)) = (metadata.as_object_mut(), market_model)
                {
                    object.insert(
                        "everybuddyOpenRouterMatch".to_string(),
                        json!({
                            "source": "openrouter",
                            "modelId": market_model.id,
                        }),
                    );
                }

                Ok(ManagedModel {
                    key,
                    gateway_id: profile.id.clone(),
                    id: id.to_string(),
                    name,
                    vendor,
                    capabilities,
                    configuration,
                    evidence,
                    metadata,
                    updated_at: now.clone(),
                })
            })
            .collect()
    }

    pub async fn market_model(&self, model_id: &str, vendor: &str) -> Option<MarketModel> {
        self.market_catalog
            .snapshot()
            .await?
            .find(model_id, vendor)
            .cloned()
    }

    pub async fn probe(
        &self,
        profile: &GatewayProfile,
        token: &str,
        model: &ManagedModel,
    ) -> CoreResult<(Vec<crate::models::CapabilityEvidence>, Vec<String>)> {
        let api_root = normalize_api_root(
            model
                .configuration
                .endpoint_override
                .as_deref()
                .unwrap_or(&profile.api_root),
        )?;
        let url = format!("{}/chat/completions", api_root.trim_end_matches('/'));
        let now = Utc::now().to_rfc3339();
        let mut results = Vec::new();
        let mut notes = Vec::new();

        let tool_body = json!({
            "model": model.id,
            "messages": [{"role": "user", "content": "Call the get_probe_value tool with value 1. Do not answer with text."}],
            "tools": [{
                "type": "function",
                "function": {
                    "name": "get_probe_value",
                    "description": "Capability probe",
                    "parameters": {
                        "type": "object",
                        "properties": {"value": {"type": "integer"}},
                        "required": ["value"]
                    }
                }
            }],
            "tool_choice": "auto",
            "max_tokens": 16
        });
        match self.probe_request(&url, token, tool_body).await {
            Ok(body) => {
                let supported = body
                    .pointer("/choices/0/message/tool_calls")
                    .and_then(Value::as_array)
                    .is_some_and(|calls| !calls.is_empty());
                if supported {
                    results.push(evidence(
                        "toolCall",
                        true,
                        EvidenceSource::Probe,
                        "Tool call returned",
                        &now,
                    ));
                } else {
                    notes.push(
                        "Tool probe completed without a tool call; existing evidence was kept"
                            .to_string(),
                    );
                }
            }
            Err(error) => notes.push(format!("Tool probe: {error}")),
        }

        let image_body = json!({
            "model": model.id,
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": "Reply with one word describing this image."},
                    {"type": "image_url", "image_url": {"url": "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII="}}
                ]
            }],
            "max_tokens": 4
        });
        match self.probe_request(&url, token, image_body).await {
            Ok(_) => results.push(evidence(
                "images",
                true,
                EvidenceSource::Probe,
                "Image input accepted",
                &now,
            )),
            Err(error) => notes.push(format!("Image probe: {error}")),
        }

        let reasoning_body = json!({
            "model": model.id,
            "messages": [{"role": "user", "content": "What is 1 + 1? Reply with the number only."}],
            "reasoning_effort": "low",
            "max_tokens": 8
        });
        match self.probe_request(&url, token, reasoning_body).await {
            Ok(body) => {
                let has_reasoning = body
                    .pointer("/usage/completion_tokens_details/reasoning_tokens")
                    .and_then(Value::as_u64)
                    .is_some_and(|tokens| tokens > 0)
                    || body
                        .pointer("/choices/0/message/reasoning_content")
                        .is_some();
                if has_reasoning {
                    results.push(evidence(
                        "reasoning",
                        true,
                        EvidenceSource::Probe,
                        "Reasoning output reported",
                        &now,
                    ));
                } else {
                    notes.push("Reasoning parameter was accepted without verifiable reasoning output; existing evidence was kept".to_string());
                }
            }
            Err(error) => notes.push(format!("Reasoning probe: {error}")),
        }

        Ok((results, notes))
    }

    async fn probe_request(&self, url: &str, token: &str, body: Value) -> CoreResult<Value> {
        let response = self
            .client
            .post(url)
            .bearer_auth(token)
            .json(&body)
            .send()
            .await
            .map_err(map_reqwest_error)?;
        if !response.status().is_success() {
            return Err(CoreError::Protocol(format!(
                "HTTP {}",
                response.status().as_u16()
            )));
        }
        let bytes = read_response_body(response, "The probe response").await?;
        serde_json::from_slice(&bytes)
            .map_err(|_| CoreError::Protocol("Response is not valid JSON".to_string()))
    }
}

fn resolved_identity(
    identity_override: Option<&Value>,
    discovered_name: &str,
    discovered_vendor: String,
) -> (String, String) {
    let name = identity_override
        .and_then(|value| value.get("name"))
        .and_then(Value::as_str)
        .unwrap_or(discovered_name)
        .to_string();
    let vendor = identity_override
        .and_then(|value| value.get("vendor"))
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .unwrap_or(discovered_vendor);
    (name, vendor)
}

fn should_preserve_configuration(model: &ManagedModel) -> bool {
    let local_source = matches!(
        model
            .metadata
            .get("everybuddySource")
            .and_then(Value::as_str),
        Some("manual" | "targetImport")
    );
    local_source
        || model
            .evidence
            .iter()
            .any(|item| item.source == EvidenceSource::Manual)
}

fn discovered_configuration(
    existing: Option<&ManagedModel>,
    model_id: &str,
    metadata: &Value,
    market_model: Option<&crate::market_catalog::MarketModel>,
    capabilities: &crate::models::CapabilitySet,
) -> crate::models::ModelConfiguration {
    existing
        .filter(|model| should_preserve_configuration(model))
        .map(|model| model.configuration.clone())
        .unwrap_or_else(|| {
            configuration_from_sources(model_id, metadata, market_model, capabilities)
        })
}

async fn read_response_body(mut response: reqwest::Response, label: &str) -> CoreResult<Vec<u8>> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_GATEWAY_RESPONSE_BYTES as u64)
    {
        return Err(CoreError::Protocol(format!("{label} is too large")));
    }

    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(map_reqwest_error)? {
        if bytes.len().saturating_add(chunk.len()) > MAX_GATEWAY_RESPONSE_BYTES {
            return Err(CoreError::Protocol(format!("{label} is too large")));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn parse_discovery_body(bytes: &[u8], token: &str) -> CoreResult<Value> {
    if bytes.len() > MAX_GATEWAY_RESPONSE_BYTES {
        return Err(CoreError::Protocol(
            "The models response is too large".to_string(),
        ));
    }
    let body: Value = serde_json::from_slice(bytes)
        .map_err(|_| CoreError::Protocol("The models response is not valid JSON".to_string()))?;
    if value_contains_secret(&body, token) {
        return Err(CoreError::Protocol(
            "The models response contains sensitive credential data".to_string(),
        ));
    }
    let items = body.get("data").and_then(Value::as_array).ok_or_else(|| {
        CoreError::Protocol("The models response must contain a data array".to_string())
    })?;
    if items.len() > MAX_DISCOVERED_MODELS {
        return Err(CoreError::Protocol(format!(
            "The models response exceeds the {MAX_DISCOVERED_MODELS} model limit"
        )));
    }
    let mut ids = HashSet::new();
    for item in items {
        if let Some(id) = item.get("id").and_then(Value::as_str) {
            if !ids.insert(id) {
                return Err(CoreError::Protocol(format!(
                    "The models response contains duplicate model ID {id}"
                )));
            }
        }
    }
    Ok(body)
}

#[cfg(test)]
fn discover_from_body(body: &str, token: &str) -> CoreResult<Value> {
    parse_discovery_body(body.as_bytes(), token)
}

pub fn normalize_api_root(input: &str) -> CoreResult<String> {
    let trimmed = input.trim().trim_end_matches('/');
    let mut url = Url::parse(trimmed)
        .map_err(|_| CoreError::Validation("Enter a valid HTTP or HTTPS API URL".to_string()))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(CoreError::Validation(
            "Only HTTP and HTTPS gateway URLs are supported".to_string(),
        ));
    }
    if url.scheme() == "http" {
        let loopback = match url.host() {
            Some(Host::Domain(host)) => host.eq_ignore_ascii_case("localhost"),
            Some(Host::Ipv4(address)) => address.is_loopback(),
            Some(Host::Ipv6(address)) => address.is_loopback(),
            None => false,
        };
        if !loopback {
            return Err(CoreError::Validation(
                "Remote gateway URLs must use HTTPS".to_string(),
            ));
        }
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(CoreError::Validation(
            "Credentials are not allowed inside the gateway URL".to_string(),
        ));
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(CoreError::Validation(
            "Gateway URLs cannot contain a query or fragment".to_string(),
        ));
    }

    let path = url.path().trim_end_matches('/');
    let normalized_path = if path.ends_with("/v1/models") {
        path.trim_end_matches("/models").to_string()
    } else if path.ends_with("/v1") {
        path.to_string()
    } else if path.is_empty() || path == "/" {
        "/v1".to_string()
    } else {
        format!("{path}/v1")
    };
    url.set_path(&normalized_path);

    Ok(url.to_string().trim_end_matches('/').to_string())
}

fn map_reqwest_error(error: reqwest::Error) -> CoreError {
    if error.is_timeout() {
        CoreError::Network("The gateway request timed out".to_string())
    } else if error.is_connect() {
        CoreError::Network("Could not connect to the gateway".to_string())
    } else {
        CoreError::Network(error.to_string())
    }
}

pub fn object_without_secret(value: &Value) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .iter()
                .filter(|(key, _)| !is_secret_key(key))
                .map(|(key, value)| (key.clone(), object_without_secret(value)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(object_without_secret).collect()),
        _ => value.clone(),
    }
}

pub fn value_contains_secret(value: &Value, secret: &str) -> bool {
    if secret.is_empty() {
        return false;
    }
    match value {
        Value::Object(object) => object.iter().any(|(key, value)| {
            (!is_structural_response_key(key) && string_contains_secret(key, secret))
                || value_contains_secret(value, secret)
        }),
        Value::Array(items) => items.iter().any(|item| value_contains_secret(item, secret)),
        Value::String(value) => string_contains_secret(value, secret),
        _ => false,
    }
}

fn is_structural_response_key(key: &str) -> bool {
    matches!(
        key.to_ascii_lowercase().as_str(),
        "data" | "id" | "object" | "name" | "display_name" | "owned_by" | "created"
    )
}

fn string_contains_secret(value: &str, secret: &str) -> bool {
    if value == secret || secret.len() >= 12 && value.contains(secret) {
        return true;
    }

    value.match_indices(secret).any(|(start, _)| {
        let before = value[..start].chars().next_back();
        let after = value[start + secret.len()..].chars().next();
        before.is_none_or(|character| !is_credential_character(character))
            && after.is_none_or(|character| !is_credential_character(character))
    })
}

fn is_credential_character(character: char) -> bool {
    character.is_ascii_alphanumeric()
        || matches!(character, '.' | '_' | '~' | '+' | '/' | '-' | '=')
}

fn is_secret_key(key: &str) -> bool {
    let normalized: String = key
        .chars()
        .filter(|character| !matches!(character, '_' | '-'))
        .flat_map(char::to_lowercase)
        .collect();
    matches!(
        normalized.as_str(),
        "apikey"
            | "token"
            | "accesstoken"
            | "refreshtoken"
            | "authorization"
            | "password"
            | "secret"
            | "clientsecret"
            | "credential"
            | "credentials"
    )
}

#[cfg(test)]
mod tests {
    use std::thread;

    use tiny_http::{Header, Response, Server};

    use super::*;

    #[test]
    fn normalizes_supported_gateway_url_shapes() {
        assert_eq!(
            normalize_api_root("https://example.com").unwrap(),
            "https://example.com/v1"
        );
        assert_eq!(
            normalize_api_root("https://example.com/v1/").unwrap(),
            "https://example.com/v1"
        );
        assert_eq!(
            normalize_api_root("https://example.com/v1/models").unwrap(),
            "https://example.com/v1"
        );
        assert_eq!(
            normalize_api_root("https://example.com/api").unwrap(),
            "https://example.com/api/v1"
        );
    }

    #[test]
    fn rejects_credentials_in_url() {
        assert!(normalize_api_root("https://token@example.com/v1").is_err());
    }

    #[test]
    fn rejects_plain_http_for_remote_gateways() {
        assert!(normalize_api_root("http://api.example.com/v1").is_err());
        assert!(normalize_api_root("http://127.0.0.1:8080/v1").is_ok());
        assert!(normalize_api_root("http://[::1]:8080/v1").is_ok());
    }

    #[test]
    fn recursively_removes_secret_metadata() {
        let metadata = json!({
            "id": "safe-model",
            "Authorization": "Bearer hidden",
            "nested": {
                "access_token": "hidden",
                "safe": true,
                "items": [
                    {"client-secret": "hidden", "label": "keep"},
                    {"CREDENTIALS": "hidden"}
                ]
            }
        });

        let sanitized = object_without_secret(&metadata);

        assert_eq!(sanitized["id"], "safe-model");
        assert_eq!(sanitized["nested"]["safe"], true);
        assert_eq!(sanitized["nested"]["items"][0]["label"], "keep");
        assert!(sanitized.get("Authorization").is_none());
        assert!(sanitized["nested"].get("access_token").is_none());
        assert!(sanitized["nested"]["items"][0]
            .get("client-secret")
            .is_none());
        assert!(sanitized["nested"]["items"][1].get("CREDENTIALS").is_none());
    }

    #[test]
    fn detects_secret_values_under_unrecognized_metadata_keys() {
        let metadata = json!({
            "nested": {
                "note": "Bearer target-secret-value",
                "items": ["safe"]
            }
        });

        assert!(value_contains_secret(&metadata, "target-secret-value"));
        assert!(!value_contains_secret(&metadata, "different-secret"));
        assert!(value_contains_secret(&json!({"value": "x"}), "x"));
        assert!(value_contains_secret(&json!({"note": "Bearer abc"}), "abc"));
        assert!(!value_contains_secret(&json!({"id": "safe-model"}), "id"));
    }

    #[test]
    fn detects_credentials_in_json_keys_without_short_substring_false_positives() {
        assert!(value_contains_secret(
            &json!({"target-secret-value": true}),
            "target-secret-value"
        ));
        assert!(value_contains_secret(&json!({"note": "Bearer pro"}), "pro"));
        assert!(value_contains_secret(&json!({"note": "pro"}), "pro"));
        assert!(!value_contains_secret(
            &json!({"note": "provider unavailable"}),
            "pro"
        ));
        assert!(value_contains_secret(&json!({"sk": true}), "sk"));
        assert!(!value_contains_secret(&json!({"data": []}), "data"));
    }

    #[test]
    fn rejects_duplicate_model_ids() {
        let body = r#"{"data":[{"id":"duplicate"},{"id":"duplicate"}]}"#;
        let error = discover_from_body(body, "test-token").unwrap_err();

        assert!(error.to_string().contains("duplicate model ID"));
    }

    #[test]
    fn rejects_oversized_gateway_responses() {
        let body = format!(
            r#"{{"data":[{{"id":"large","note":"{}"}}]}}"#,
            "x".repeat(5 * 1024 * 1024)
        );
        let error = discover_from_body(&body, "test-token").unwrap_err();

        assert!(error.to_string().contains("response is too large"));
    }

    #[test]
    fn rejects_too_many_discovered_models() {
        let data: Vec<_> = (0..=MAX_DISCOVERED_MODELS)
            .map(|index| json!({"id": format!("model-{index}")}))
            .collect();
        let body = json!({"data": data}).to_string();

        let error = discover_from_body(&body, "test-token").unwrap_err();

        assert!(error.to_string().contains("model limit"));
    }

    #[test]
    fn probe_uses_the_model_endpoint_override() {
        let server = Server::http("127.0.0.1:0").unwrap();
        let address = server.server_addr().to_string();
        let server_thread = thread::spawn(move || {
            for _ in 0..3 {
                let request = server.recv().unwrap();
                assert_eq!(request.url(), "/v1/chat/completions");
                request
                    .respond(
                        Response::from_string(r#"{"choices":[{"message":{}}]}"#).with_header(
                            Header::from_bytes("content-type", "application/json").unwrap(),
                        ),
                    )
                    .unwrap();
            }
        });
        let profile = GatewayProfile {
            id: "remote".to_string(),
            name: "Remote".to_string(),
            api_root: "https://unused.example/v1".to_string(),
            token_ref: "remote".to_string(),
            created_at: "2026-08-20T00:00:00Z".to_string(),
            updated_at: "2026-08-20T00:00:00Z".to_string(),
        };
        let model = ManagedModel {
            key: "remote::model".to_string(),
            gateway_id: profile.id.clone(),
            id: "model".to_string(),
            name: "Model".to_string(),
            vendor: "custom".to_string(),
            capabilities: Default::default(),
            configuration: crate::models::ModelConfiguration {
                endpoint_override: Some(format!("http://{address}/v1/models")),
                ..Default::default()
            },
            evidence: Vec::new(),
            metadata: json!({"id": "model"}),
            updated_at: "2026-08-20T00:00:00Z".to_string(),
        };

        tauri::async_runtime::block_on(GatewayClient::new(None).unwrap().probe(
            &profile,
            "test-token",
            &model,
        ))
        .unwrap();

        server_thread.join().unwrap();
    }

    #[test]
    fn discovers_models_from_fake_openai_server() {
        let server = Server::http("127.0.0.1:0").unwrap();
        let address = server.server_addr().to_string();
        let server_thread = thread::spawn(move || {
            let request = server.recv().unwrap();
            assert_eq!(request.url(), "/v1/models");
            assert_eq!(
                request
                    .headers()
                    .iter()
                    .find(|header| header.field.equiv("authorization"))
                    .map(|header| header.value.as_str()),
                Some("Bearer test-token")
            );
            request
                .respond(
                    Response::from_string(
                        r#"{"data":[{"id":"gpt-5.6","name":"GPT-5.6","supports_tool_call":true}]}"#,
                    )
                    .with_header(Header::from_bytes("content-type", "application/json").unwrap()),
                )
                .unwrap();
        });
        let profile = GatewayProfile {
            id: "fake".to_string(),
            name: "Fake gateway".to_string(),
            api_root: format!("http://{address}/v1"),
            token_ref: "fake".to_string(),
            created_at: "2026-08-20T00:00:00Z".to_string(),
            updated_at: "2026-08-20T00:00:00Z".to_string(),
        };

        let models = tauri::async_runtime::block_on(GatewayClient::new(None).unwrap().discover(
            &profile,
            "test-token",
            &[],
        ))
        .unwrap();

        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "gpt-5.6");
        assert!(models[0].capabilities.supports_tool_call);
        server_thread.join().unwrap();
    }

    #[test]
    fn refresh_reapplies_automatic_configuration_but_preserves_manual_configuration() {
        let mut automatic = ManagedModel {
            key: "gateway::deepseek-v4-pro".to_string(),
            gateway_id: "gateway".to_string(),
            id: "deepseek-v4-pro".to_string(),
            name: "DeepSeek V4 Pro".to_string(),
            vendor: "deepseek".to_string(),
            capabilities: crate::models::CapabilitySet {
                supports_reasoning: true,
                reasoning_efforts: vec!["high".to_string(), "max".to_string()],
                ..Default::default()
            },
            configuration: crate::models::ModelConfiguration {
                reasoning: crate::models::ReasoningConfiguration {
                    default_effort: Some(crate::models::ReasoningEffort::High),
                    supported_efforts: vec![
                        crate::models::ReasoningEffort::High,
                        crate::models::ReasoningEffort::Max,
                    ],
                    ..Default::default()
                },
                ..Default::default()
            },
            evidence: Vec::new(),
            metadata: json!({"id": "deepseek-v4-pro"}),
            updated_at: "2026-08-20T00:00:00Z".to_string(),
        };

        let metadata = json!({
            "supportsReasoning": true,
            "reasoning": {
                "defaultEffort": "low",
                "supportedEfforts": ["low"]
            }
        });
        let (capabilities, _) =
            CapabilityResolver::resolve(&automatic.id, &metadata, &automatic.evidence);
        let refreshed = discovered_configuration(
            Some(&automatic),
            &automatic.id,
            &metadata,
            None,
            &capabilities,
        );

        assert_eq!(
            refreshed.reasoning.supported_efforts,
            vec![crate::models::ReasoningEffort::Low]
        );

        automatic.evidence.push(evidence(
            "configuration",
            true,
            EvidenceSource::Manual,
            "User override",
            "2026-08-20T00:00:00Z",
        ));
        let preserved = discovered_configuration(
            Some(&automatic),
            &automatic.id,
            &metadata,
            None,
            &capabilities,
        );
        assert_eq!(preserved, automatic.configuration);

        automatic.evidence.clear();
        automatic.metadata["everybuddySource"] = json!("targetImport");
        let imported = discovered_configuration(
            Some(&automatic),
            &automatic.id,
            &metadata,
            None,
            &capabilities,
        );
        assert_eq!(imported, automatic.configuration);
    }

    #[test]
    fn refresh_preserves_manual_model_identity() {
        let identity_override = json!({
            "name": "Private GPT",
            "vendor": "private"
        });

        let identity = resolved_identity(
            Some(&identity_override),
            "Gateway GPT",
            "openai".to_string(),
        );

        assert_eq!(identity, ("Private GPT".to_string(), "private".to_string()));
    }

    #[test]
    fn rejects_gateway_response_that_echoes_the_token() {
        let server = Server::http("127.0.0.1:0").unwrap();
        let address = server.server_addr().to_string();
        let server_thread = thread::spawn(move || {
            let request = server.recv().unwrap();
            request
                .respond(
                    Response::from_string(
                        r#"{"data":[{"id":"safe-model","note":"Bearer target-secret-value"}]}"#,
                    )
                    .with_header(Header::from_bytes("content-type", "application/json").unwrap()),
                )
                .unwrap();
        });
        let profile = GatewayProfile {
            id: "fake".to_string(),
            name: "Fake gateway".to_string(),
            api_root: format!("http://{address}/v1"),
            token_ref: "fake".to_string(),
            created_at: "2026-08-20T00:00:00Z".to_string(),
            updated_at: "2026-08-20T00:00:00Z".to_string(),
        };

        let error = tauri::async_runtime::block_on(GatewayClient::new(None).unwrap().discover(
            &profile,
            "target-secret-value",
            &[],
        ))
        .unwrap_err();

        assert!(error.to_string().contains("sensitive credential data"));
        assert!(!error.to_string().contains("target-secret-value"));
        server_thread.join().unwrap();
    }

    #[test]
    fn does_not_follow_gateway_redirects() {
        let server = Server::http("127.0.0.1:0").unwrap();
        let address = server.server_addr();
        let server_thread = thread::spawn(move || {
            let request = server.recv().unwrap();
            request
                .respond(Response::empty(302).with_header(
                    Header::from_bytes("location", "http://127.0.0.1:9/v1/models").unwrap(),
                ))
                .unwrap();
        });
        let profile = GatewayProfile {
            id: "redirect".to_string(),
            name: "Redirect".to_string(),
            api_root: format!("http://{address}/v1"),
            token_ref: "redirect".to_string(),
            created_at: Utc::now().to_rfc3339(),
            updated_at: Utc::now().to_rfc3339(),
        };

        let error = tauri::async_runtime::block_on(GatewayClient::new(None).unwrap().discover(
            &profile,
            "test-token",
            &[],
        ))
        .unwrap_err();

        assert!(error.to_string().contains("HTTP 302"));
        server_thread.join().unwrap();
    }
}
