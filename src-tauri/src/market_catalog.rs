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
    unique_leaf_ids: HashMap<String, Option<String>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MarketModel {
    pub id: String,
    #[serde(default)]
    pub context_length: Option<u64>,
    #[serde(default)]
    architecture: MarketArchitecture,
    #[serde(default)]
    top_provider: MarketTopProvider,
    #[serde(default)]
    supported_parameters: Vec<String>,
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
    max_completion_tokens: Option<u64>,
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

        Ok(Self {
            by_id,
            unique_leaf_ids,
        })
    }

    pub fn find(&self, model_id: &str, vendor: &str) -> Option<&MarketModel> {
        let normalized = normalize_model_id(model_id);
        if let Some(model) = self.by_id.get(&normalized) {
            return Some(model);
        }

        let leaf = model_leaf(&normalized);
        let normalized_vendor = normalize_vendor(vendor).unwrap_or(vendor);
        openrouter_namespaces(normalized_vendor)
            .iter()
            .find_map(|namespace| self.by_id.get(&format!("{namespace}/{leaf}")))
            .or_else(|| {
                self.unique_leaf_ids
                    .get(leaf)
                    .and_then(Option::as_ref)
                    .and_then(|id| self.by_id.get(id))
            })
    }
}

impl MarketModel {
    pub fn vendor(&self) -> Option<String> {
        self.id
            .split('/')
            .next()
            .and_then(normalize_vendor)
            .map(ToString::to_string)
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
            && self.supported_parameters.iter().any(|parameter| {
                ["reasoning", "reasoning_effort", "include_reasoning"]
                    .iter()
                    .any(|candidate| parameter.eq_ignore_ascii_case(candidate))
            })
    }

    pub fn max_output_tokens(&self) -> Option<u64> {
        self.top_provider.max_completion_tokens
    }

    fn outputs_text(&self) -> bool {
        self.architecture
            .output_modalities
            .iter()
            .any(|modality| modality.eq_ignore_ascii_case("text"))
    }
}

pub fn normalize_vendor(value: &str) -> Option<&'static str> {
    match value
        .trim()
        .to_ascii_lowercase()
        .replace([' ', '_'], "-")
        .as_str()
    {
        "openai" | "azure-openai" | "openai-codex" => Some("openai"),
        "anthropic" | "claude" => Some("anthropic"),
        "google" | "google-vertex" | "gemini" => Some("google"),
        "deepseek" => Some("deepseek"),
        "qwen" | "alibaba" | "dashscope" => Some("qwen"),
        "moonshot" | "moonshotai" | "moonshotai-cn" | "kimi-coding" => Some("moonshot"),
        "zhipu" | "zai" | "z-ai" | "bigmodel" | "chatglm" => Some("zhipu"),
        "minimax" | "minimax-cn" => Some("minimax"),
        "xai" | "x-ai" | "grok" => Some("xai"),
        "mistral" | "mistralai" => Some("mistral"),
        "meta" | "metaai" | "meta-llama" => Some("meta"),
        "cohere" => Some("cohere"),
        "tencent" | "hunyuan" => Some("tencent"),
        "bytedance" | "bytedance-seed" | "volcengine" | "volcengine-ark" | "doubao" => {
            Some("bytedance")
        }
        "baidu" | "qianfan" | "ernie" => Some("baidu"),
        "01ai" | "01-ai" | "zero-one-ai" => Some("01ai"),
        "amazon" | "amazon-bedrock" | "aws" => Some("amazon"),
        "ai21" => Some("ai21"),
        "nvidia" => Some("nvidia"),
        "perplexity" => Some("perplexity"),
        "groq" => Some("groq"),
        "cerebras" => Some("cerebras"),
        _ => None,
    }
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

fn openrouter_namespaces(vendor: &str) -> &'static [&'static str] {
    match vendor {
        "openai" => &["openai"],
        "anthropic" => &["anthropic"],
        "google" => &["google"],
        "deepseek" => &["deepseek"],
        "qwen" => &["qwen"],
        "moonshot" => &["moonshotai"],
        "zhipu" => &["z-ai"],
        "minimax" => &["minimax"],
        "xai" => &["x-ai"],
        "mistral" => &["mistralai"],
        "meta" => &["meta-llama"],
        "cohere" => &["cohere"],
        "tencent" => &["tencent"],
        "bytedance" => &["bytedance", "bytedance-seed"],
        "baidu" => &["baidu"],
        "amazon" => &["amazon"],
        "ai21" => &["ai21"],
        "nvidia" => &["nvidia"],
        "perplexity" => &["perplexity"],
        "groq" => &["groq"],
        "cerebras" => &["cerebras"],
        "01ai" => &["01-ai"],
        _ => &[],
    }
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
            context_length: Some(200_000),
            architecture: MarketArchitecture {
                input_modalities: input_modalities.iter().map(ToString::to_string).collect(),
                output_modalities: output_modalities.iter().map(ToString::to_string).collect(),
            },
            top_provider: MarketTopProvider {
                max_completion_tokens: Some(32_000),
            },
            supported_parameters: supported_parameters
                .iter()
                .map(ToString::to_string)
                .collect(),
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
            None
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
