use std::{
    collections::HashMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use atomicwrites::{AtomicFile, OverwriteBehavior};
use reqwest::{Client, StatusCode};
use serde::Deserialize;
use tokio::sync::Mutex;

use crate::error::{CoreError, CoreResult};

const OPENROUTER_MODELS_URL: &str = "https://openrouter.ai/api/v1/models?output_modalities=all";
const MARKET_CATALOG_TIMEOUT: Duration = Duration::from_secs(5);
const MARKET_CATALOG_TTL: Duration = Duration::from_secs(6 * 60 * 60);
const MARKET_CATALOG_RETRY_DELAY: Duration = Duration::from_secs(15 * 60);
const MAX_MARKET_CATALOG_BYTES: usize = 8 * 1024 * 1024;
const MAX_MARKET_MODELS: usize = 10_000;

#[derive(Clone)]
pub struct MarketCatalogClient {
    client: Client,
    state: Arc<Mutex<CatalogState>>,
    enabled: bool,
    endpoint: String,
    cache_path: Option<PathBuf>,
}

struct CachedCatalog {
    expires_at: Instant,
    snapshot: Arc<MarketCatalogSnapshot>,
}

#[derive(Default)]
struct CatalogState {
    cached: Option<CachedCatalog>,
    disk_loaded: bool,
    last_attempt: Option<Instant>,
}

#[derive(Debug)]
pub struct MarketCatalogSnapshot {
    by_id: HashMap<String, MarketModel>,
    canonical_sources: HashMap<String, String>,
    unique_leaf_ids: HashMap<String, Option<String>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MarketModel {
    pub id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    canonical_slug: Option<String>,
    #[serde(default)]
    alias_target: Option<MarketAliasTarget>,
    #[serde(default)]
    context_length: Option<u64>,
    #[serde(default)]
    architecture: MarketArchitecture,
    #[serde(default)]
    top_provider: MarketTopProvider,
    #[serde(default)]
    supported_parameters: Vec<String>,
    #[serde(default)]
    default_parameters: MarketDefaultParameters,
    #[serde(default)]
    reasoning: Option<MarketReasoning>,
}

#[derive(Debug, Clone, Deserialize)]
struct MarketAliasTarget {
    #[serde(default)]
    name: Option<String>,
    slug: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct MarketArchitecture {
    #[serde(default)]
    input_modalities: Vec<String>,
    #[serde(default)]
    output_modalities: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct MarketTopProvider {
    #[serde(default)]
    context_length: Option<u64>,
    #[serde(default)]
    max_completion_tokens: Option<u64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct MarketDefaultParameters {
    #[serde(default)]
    temperature: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
struct MarketReasoning {
    #[serde(default)]
    mandatory: Option<bool>,
    #[serde(default)]
    default_enabled: Option<bool>,
    #[serde(default)]
    supported_efforts: Option<Vec<String>>,
    #[serde(default)]
    default_effort: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MarketCatalogResponse {
    data: Vec<MarketModel>,
}

impl MarketCatalogClient {
    pub fn new(client: Client, enabled: bool, cache_path: Option<PathBuf>) -> Self {
        Self {
            client,
            state: Arc::new(Mutex::new(CatalogState::default())),
            enabled,
            endpoint: OPENROUTER_MODELS_URL.to_string(),
            cache_path,
        }
    }

    pub async fn snapshot(&self) -> Option<Arc<MarketCatalogSnapshot>> {
        if !self.enabled {
            return None;
        }

        let mut state = self.state.lock().await;
        if !state.disk_loaded {
            state.disk_loaded = true;
            state.cached = self.cache_path.as_deref().and_then(load_disk_cache);
        }
        if let Some(cached) = &state.cached {
            if Instant::now() < cached.expires_at {
                return Some(Arc::clone(&cached.snapshot));
            }
        }
        if state
            .last_attempt
            .is_some_and(|attempt| attempt.elapsed() < MARKET_CATALOG_RETRY_DELAY)
        {
            return state
                .cached
                .as_ref()
                .map(|cached| Arc::clone(&cached.snapshot));
        }
        state.last_attempt = Some(Instant::now());

        match self.fetch().await {
            Ok((snapshot, bytes)) => {
                if let Some(path) = &self.cache_path {
                    let _ = save_disk_cache(path, &bytes);
                }
                state.cached = Some(CachedCatalog {
                    expires_at: Instant::now() + MARKET_CATALOG_TTL,
                    snapshot: Arc::clone(&snapshot),
                });
                Some(snapshot)
            }
            Err(_) => state
                .cached
                .as_ref()
                .map(|cached| Arc::clone(&cached.snapshot)),
        }
    }

    async fn fetch(&self) -> CoreResult<(Arc<MarketCatalogSnapshot>, Vec<u8>)> {
        let response = self
            .client
            .get(&self.endpoint)
            .timeout(MARKET_CATALOG_TIMEOUT)
            .send()
            .await
            .map_err(|error| CoreError::Network(error.to_string()))?;
        if response.status() != StatusCode::OK {
            return Err(CoreError::Protocol(format!(
                "The OpenRouter models endpoint returned HTTP {}",
                response.status().as_u16()
            )));
        }

        let bytes = read_limited_body(response).await?;
        let payload: MarketCatalogResponse = serde_json::from_slice(&bytes).map_err(|_| {
            CoreError::Protocol("The OpenRouter models endpoint returned invalid JSON".into())
        })?;
        let snapshot = Arc::new(MarketCatalogSnapshot::new(payload.data)?);
        Ok((snapshot, bytes))
    }

    #[cfg(test)]
    fn for_test(client: Client, endpoint: String, cache_path: Option<PathBuf>) -> Self {
        Self {
            client,
            state: Arc::new(Mutex::new(CatalogState::default())),
            enabled: true,
            endpoint,
            cache_path,
        }
    }
}

impl MarketCatalogSnapshot {
    fn new(models: Vec<MarketModel>) -> CoreResult<Self> {
        if models.len() > MAX_MARKET_MODELS {
            return Err(CoreError::Protocol(format!(
                "The OpenRouter models endpoint contains more than {MAX_MARKET_MODELS} models"
            )));
        }

        let mut by_id = HashMap::with_capacity(models.len());
        let mut unique_leaf_ids = HashMap::new();
        for model in models {
            let id = normalize_model_id(&model.id);
            if id.is_empty() || by_id.contains_key(&id) {
                continue;
            }
            let leaf = model_leaf(&id).to_string();
            unique_leaf_ids
                .entry(leaf)
                .and_modify(|entry| *entry = None)
                .or_insert_with(|| Some(id.clone()));
            by_id.insert(id, model);
        }

        let mut canonical_sources = HashMap::new();
        for (id, model) in &by_id {
            let Some(canonical_slug) = model
                .canonical_slug
                .as_deref()
                .map(normalize_model_id)
                .filter(|slug| !slug.is_empty())
            else {
                continue;
            };
            canonical_sources
                .entry(canonical_slug)
                .and_modify(|current: &mut String| {
                    if is_preferred_source(id, current) {
                        current.clone_from(id);
                    }
                })
                .or_insert_with(|| id.clone());
        }

        Ok(Self {
            by_id,
            canonical_sources,
            unique_leaf_ids,
        })
    }

    pub fn find(&self, model_id: &str, vendor: &str) -> Option<&MarketModel> {
        let normalized = normalize_model_id(model_id);
        if let Some(model) = self.find_candidate(&normalized) {
            return Some(model);
        }

        let leaf = model_leaf(&normalized);
        let normalized_vendor = normalize_vendor(vendor).unwrap_or_else(|| vendor.to_string());
        openrouter_namespaces(&normalized_vendor)
            .iter()
            .find_map(|namespace| self.find_candidate(&format!("{namespace}/{leaf}")))
            .or_else(|| {
                self.unique_leaf_ids
                    .get(leaf)
                    .and_then(Option::as_ref)
                    .and_then(|id| self.find_candidate(id))
            })
    }

    fn find_candidate(&self, id: &str) -> Option<&MarketModel> {
        if let Some(model) = self.by_id.get(id) {
            return Some(self.capability_source(model));
        }
        self.canonical_sources
            .get(id)
            .and_then(|source_id| self.by_id.get(source_id))
    }

    fn capability_source<'a>(&'a self, model: &'a MarketModel) -> &'a MarketModel {
        let target = model
            .alias_target
            .as_ref()
            .map(|target| normalize_model_id(&target.slug))
            .and_then(|target| {
                self.by_id.get(&target).or_else(|| {
                    self.canonical_sources
                        .get(&target)
                        .and_then(|source_id| self.by_id.get(source_id))
                })
            })
            .unwrap_or(model);

        target
            .canonical_slug
            .as_deref()
            .map(normalize_model_id)
            .and_then(|slug| self.canonical_sources.get(&slug))
            .and_then(|source_id| self.by_id.get(source_id))
            .or_else(|| {
                delivery_variant_base(&target.id)
                    .and_then(|base_id| self.by_id.get(&normalize_model_id(base_id)))
            })
            .unwrap_or(target)
    }
}

impl MarketModel {
    pub fn vendor(&self) -> Option<String> {
        self.id.split('/').next().and_then(normalize_vendor)
    }

    pub fn display_name(&self) -> Option<&str> {
        self.name
            .as_deref()
            .filter(|name| !name.trim().is_empty())
            .or_else(|| {
                self.alias_target
                    .as_ref()
                    .and_then(|target| target.name.as_deref())
                    .filter(|name| !name.trim().is_empty())
            })
    }

    pub fn supports_tool_call(&self) -> bool {
        self.outputs_text()
            && self.supported_parameters.iter().any(|parameter| {
                parameter.eq_ignore_ascii_case("tools")
                    || parameter.eq_ignore_ascii_case("tool_choice")
            })
    }

    pub fn supports_images(&self) -> bool {
        self.outputs_text()
            && self
                .architecture
                .input_modalities
                .iter()
                .any(|modality| modality.eq_ignore_ascii_case("image"))
    }

    pub fn supports_reasoning(&self) -> bool {
        self.outputs_text()
            && (self.has_reasoning_metadata()
                || self.supported_parameters.iter().any(|parameter| {
                    ["reasoning", "reasoning_effort", "include_reasoning"]
                        .iter()
                        .any(|candidate| parameter.eq_ignore_ascii_case(candidate))
                }))
    }

    pub fn max_input_tokens(&self) -> Option<u64> {
        self.outputs_text()
            .then(|| self.context_length.or(self.top_provider.context_length))
            .flatten()
            .filter(|value| *value > 0)
    }

    pub fn max_output_tokens(&self) -> Option<u64> {
        self.outputs_text()
            .then_some(self.top_provider.max_completion_tokens)
            .flatten()
            .filter(|value| *value > 0)
    }

    pub fn temperature(&self) -> Option<f64> {
        self.outputs_text()
            .then_some(self.default_parameters.temperature)
            .flatten()
    }

    pub fn supported_reasoning_efforts(&self) -> Option<&[String]> {
        if !self.outputs_text() {
            return None;
        }
        self.reasoning
            .as_ref()
            .and_then(|reasoning| reasoning.supported_efforts.as_deref())
    }

    pub fn default_reasoning_effort(&self) -> Option<&str> {
        if !self.outputs_text() {
            return None;
        }
        self.reasoning
            .as_ref()
            .and_then(|reasoning| reasoning.default_effort.as_deref())
    }

    pub fn can_disable_thinking(&self) -> Option<bool> {
        if !self.outputs_text() {
            return None;
        }
        let reasoning = self.reasoning.as_ref()?;
        reasoning.mandatory.map(|mandatory| !mandatory).or_else(|| {
            reasoning
                .supported_efforts
                .as_ref()
                .is_some_and(|efforts| {
                    efforts
                        .iter()
                        .any(|effort| effort.eq_ignore_ascii_case("none"))
                })
                .then_some(true)
        })
    }

    fn has_reasoning_metadata(&self) -> bool {
        self.reasoning.as_ref().is_some_and(|reasoning| {
            reasoning.mandatory.is_some()
                || reasoning.default_enabled.is_some()
                || reasoning.supported_efforts.is_some()
                || reasoning.default_effort.is_some()
        })
    }

    pub fn supports_chat_configuration(&self) -> bool {
        self.outputs_text()
    }

    fn outputs_text(&self) -> bool {
        self.architecture
            .output_modalities
            .iter()
            .any(|modality| modality.eq_ignore_ascii_case("text"))
    }
}

pub fn normalize_vendor(value: &str) -> Option<String> {
    let normalized = value
        .trim()
        .trim_start_matches('~')
        .to_ascii_lowercase()
        .replace([' ', '_'], "-");
    let canonical = match normalized.as_str() {
        "openai" | "azure-openai" | "openai-codex" => "openai",
        "anthropic" | "claude" => "anthropic",
        "google" | "google-vertex" | "gemini" => "google",
        "deepseek" => "deepseek",
        "qwen" | "alibaba" | "dashscope" => "qwen",
        "moonshot" | "moonshotai" | "moonshotai-cn" | "kimi-coding" => "moonshot",
        "zhipu" | "zai" | "z-ai" | "bigmodel" | "chatglm" => "zhipu",
        "minimax" | "minimax-cn" => "minimax",
        "xai" | "x-ai" | "grok" => "xai",
        "mistral" | "mistralai" => "mistral",
        "meta" | "metaai" | "meta-llama" => "meta",
        "cohere" => "cohere",
        "tencent" | "hunyuan" => "tencent",
        "bytedance" | "bytedance-seed" | "volcengine" | "volcengine-ark" | "doubao" => "bytedance",
        "baidu" | "qianfan" | "ernie" => "baidu",
        "01ai" | "01-ai" | "zero-one-ai" => "01ai",
        "amazon" | "amazon-bedrock" | "aws" => "amazon",
        "ai21" => "ai21",
        "nvidia" => "nvidia",
        "perplexity" => "perplexity",
        "groq" => "groq",
        "cerebras" => "cerebras",
        _ => normalized.as_str(),
    };
    valid_vendor_namespace(canonical).then(|| canonical.to_string())
}

fn load_disk_cache(path: &Path) -> Option<CachedCatalog> {
    let metadata = fs::metadata(path).ok()?;
    let modified = metadata.modified().ok()?;
    let age = modified.elapsed().unwrap_or_default();
    let bytes = fs::read(path).ok()?;
    if bytes.len() > MAX_MARKET_CATALOG_BYTES {
        return None;
    }
    let payload: MarketCatalogResponse = serde_json::from_slice(&bytes).ok()?;
    let snapshot = Arc::new(MarketCatalogSnapshot::new(payload.data).ok()?);
    let remaining = MARKET_CATALOG_TTL.saturating_sub(age);
    Some(CachedCatalog {
        expires_at: Instant::now() + remaining,
        snapshot,
    })
}

fn save_disk_cache(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(
        AtomicFile::new(path, OverwriteBehavior::AllowOverwrite).write(|file| {
            file.write_all(bytes)?;
            file.sync_all()
        })?,
    )
}

async fn read_limited_body(mut response: reqwest::Response) -> CoreResult<Vec<u8>> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_MARKET_CATALOG_BYTES as u64)
    {
        return Err(CoreError::Protocol(
            "The OpenRouter models response exceeds the 8 MiB limit".into(),
        ));
    }

    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| CoreError::Network(error.to_string()))?
    {
        if bytes.len().saturating_add(chunk.len()) > MAX_MARKET_CATALOG_BYTES {
            return Err(CoreError::Protocol(
                "The OpenRouter models response exceeds the 8 MiB limit".into(),
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn normalize_model_id(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn model_leaf(model_id: &str) -> &str {
    model_id.rsplit('/').next().unwrap_or(model_id)
}

fn is_preferred_source(candidate: &str, current: &str) -> bool {
    source_rank(candidate) < source_rank(current)
}

fn source_rank(id: &str) -> (bool, bool, &str) {
    (id.starts_with('~'), id.contains(':'), id)
}

fn delivery_variant_base(id: &str) -> Option<&str> {
    [":batch", ":free"]
        .into_iter()
        .find_map(|suffix| id.strip_suffix(suffix))
}

fn valid_vendor_namespace(value: &str) -> bool {
    value.len() <= 128
        && value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && value
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.'))
}

fn openrouter_namespaces(vendor: &str) -> Vec<String> {
    let namespaces: &[&str] = match vendor {
        "qwen" => &["qwen", "alibaba"],
        "moonshot" => &["moonshotai", "moonshot"],
        "zhipu" => &["z-ai"],
        "xai" => &["x-ai"],
        "mistral" => &["mistralai"],
        "meta" => &["meta-llama", "meta"],
        "bytedance" => &["bytedance", "bytedance-seed"],
        "01ai" => &["01-ai"],
        _ => return vec![vendor.to_string()],
    };
    namespaces
        .iter()
        .map(|namespace| (*namespace).to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    fn model(
        id: &str,
        input_modalities: &[&str],
        output_modalities: &[&str],
        supported_parameters: &[&str],
    ) -> MarketModel {
        MarketModel {
            id: id.to_string(),
            name: None,
            canonical_slug: None,
            alias_target: None,
            context_length: Some(200_000),
            architecture: MarketArchitecture {
                input_modalities: input_modalities.iter().map(ToString::to_string).collect(),
                output_modalities: output_modalities.iter().map(ToString::to_string).collect(),
            },
            top_provider: MarketTopProvider {
                context_length: None,
                max_completion_tokens: Some(32_000),
            },
            supported_parameters: supported_parameters
                .iter()
                .map(ToString::to_string)
                .collect(),
            default_parameters: MarketDefaultParameters::default(),
            reasoning: None,
        }
    }

    #[test]
    fn matches_namespaced_and_unique_leaf_model_ids() {
        let snapshot = MarketCatalogSnapshot::new(vec![model(
            "openai/gpt-5.6-sol",
            &["text", "image"],
            &["text"],
            &["reasoning_effort", "tools"],
        )])
        .unwrap();

        assert_eq!(
            snapshot.find("openai/gpt-5.6-sol", "openai").unwrap().id,
            "openai/gpt-5.6-sol"
        );
        assert_eq!(
            snapshot.find("gpt-5.6-sol", "openai").unwrap().id,
            "openai/gpt-5.6-sol"
        );
    }

    #[test]
    fn resolves_model_variants_aliases_and_canonical_slugs_to_the_base_record() {
        let models: Vec<MarketModel> = serde_json::from_value(serde_json::json!([
            {
                "id": "openai/gpt-5.6-sol",
                "name": "OpenAI: GPT-5.6 Sol",
                "canonical_slug": "openai/gpt-5.6-sol-20260709",
                "architecture": { "output_modalities": ["text"] }
            },
            {
                "id": "openai/gpt-5.6-sol:batch",
                "name": "OpenAI: GPT-5.6 Sol (batch)",
                "canonical_slug": "openai/gpt-5.6-sol-20260709",
                "architecture": { "output_modalities": ["text"] }
            },
            {
                "id": "openai/gpt-5.6-sol-pro",
                "name": "OpenAI: GPT-5.6 Sol Pro",
                "canonical_slug": "openai/gpt-5.6-sol-pro-20260709",
                "architecture": { "output_modalities": ["text"] }
            },
            {
                "id": "~openai/gpt-latest",
                "canonical_slug": "~openai/gpt-latest",
                "alias_target": {
                    "name": "OpenAI: GPT-5.6 Sol",
                    "slug": "openai/gpt-5.6-sol"
                },
                "architecture": { "output_modalities": ["text"] }
            }
        ]))
        .unwrap();
        let snapshot = MarketCatalogSnapshot::new(models).unwrap();

        for id in [
            "gpt-5.6-sol",
            "openai/gpt-5.6-sol",
            "openai/gpt-5.6-sol:batch",
            "openai/gpt-5.6-sol-20260709",
            "~openai/gpt-latest",
        ] {
            assert_eq!(
                snapshot.find(id, "openai").unwrap().id,
                "openai/gpt-5.6-sol",
                "unexpected capability source for {id}"
            );
        }
        assert_eq!(
            snapshot
                .find("openai/gpt-5.6-sol-pro", "openai")
                .unwrap()
                .id,
            "openai/gpt-5.6-sol-pro"
        );
    }

    #[test]
    fn does_not_use_ambiguous_leaf_ids() {
        let snapshot = MarketCatalogSnapshot::new(vec![
            model("vendor-a/shared", &["text"], &["text"], &[]),
            model("vendor-b/shared", &["text"], &["text"], &[]),
        ])
        .unwrap();

        assert!(snapshot.find("shared", "custom").is_none());
    }

    #[test]
    fn maps_structured_modalities_and_parameters_to_capabilities() {
        let chat = model(
            "openai/gpt-5.6-sol",
            &["text", "image", "file"],
            &["text"],
            &["reasoning_effort", "tools"],
        );
        let image_generator = model(
            "openai/gpt-image-2",
            &["text", "image"],
            &["image"],
            &["max_tokens"],
        );

        assert!(chat.supports_tool_call());
        assert!(chat.supports_images());
        assert!(chat.supports_reasoning());
        assert!(!image_generator.supports_images());
    }

    #[test]
    fn maps_openrouter_namespace_to_catalog_vendor() {
        assert_eq!(
            model("bytedance-seed/doubao-1.6", &["text"], &["text"], &[]).vendor(),
            Some("bytedance".to_string())
        );
        assert_eq!(
            model("future-lab/new-model", &["text"], &["text"], &[]).vendor(),
            Some("future-lab".to_string())
        );
        assert_eq!(
            model("~openai/gpt-latest", &["text"], &["text"], &[]).vendor(),
            Some("openai".to_string())
        );
    }

    #[test]
    fn reuses_memory_and_disk_cache_without_repeating_the_request() {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/models", server.server_addr());
        let requests = Arc::new(AtomicUsize::new(0));
        let server_requests = Arc::clone(&requests);
        let server_thread = std::thread::spawn(move || {
            let request = server.recv().unwrap();
            server_requests.fetch_add(1, Ordering::SeqCst);
            request
                .respond(tiny_http::Response::from_string(
                    serde_json::json!({
                        "data": [{
                            "id": "openai/gpt-test",
                            "architecture": {
                                "input_modalities": ["text"],
                                "output_modalities": ["text"]
                            },
                            "supported_parameters": ["tools"]
                        }]
                    })
                    .to_string(),
                ))
                .unwrap();
        });
        let directory = tempfile::tempdir().unwrap();
        let cache_path = directory.path().join("openrouter-models.json");
        let client = Client::builder().build().unwrap();
        let catalog =
            MarketCatalogClient::for_test(client.clone(), endpoint, Some(cache_path.clone()));

        tauri::async_runtime::block_on(async {
            assert!(catalog.snapshot().await.is_some());
            assert!(catalog.snapshot().await.is_some());
        });
        server_thread.join().unwrap();
        assert_eq!(requests.load(Ordering::SeqCst), 1);

        let restored = MarketCatalogClient::for_test(
            client,
            "http://127.0.0.1:1/models".to_string(),
            Some(cache_path),
        );
        let snapshot = tauri::async_runtime::block_on(restored.snapshot()).unwrap();
        assert!(snapshot.find("openai/gpt-test", "openai").is_some());
    }

    #[test]
    fn backs_off_after_a_failed_catalog_request() {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/models", server.server_addr());
        let requests = Arc::new(AtomicUsize::new(0));
        let server_requests = Arc::clone(&requests);
        let server_thread = std::thread::spawn(move || {
            let request = server.recv().unwrap();
            server_requests.fetch_add(1, Ordering::SeqCst);
            request.respond(tiny_http::Response::empty(503)).unwrap();
        });
        let catalog =
            MarketCatalogClient::for_test(Client::builder().build().unwrap(), endpoint, None);

        tauri::async_runtime::block_on(async {
            assert!(catalog.snapshot().await.is_none());
            assert!(catalog.snapshot().await.is_none());
        });
        server_thread.join().unwrap();
        assert_eq!(requests.load(Ordering::SeqCst), 1);
    }
}
