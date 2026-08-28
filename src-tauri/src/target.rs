use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub const MAX_TARGET_CONFIG_BYTES: usize = 8 * 1024 * 1024;
const MAX_TARGET_MODELS: usize = 10_000;

use atomicwrites::{AtomicFile, OverwriteBehavior};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use url::Url;

use crate::{
    error::{CoreError, CoreResult},
    gateway::{normalize_api_root, normalize_request_url},
    models::{GatewayProfile, ManagedModel, TargetKind, TargetSchema, TargetStatus},
    store::Store,
};

pub trait TargetAdapter: Send + Sync {
    fn kind(&self) -> TargetKind;
    fn default_path(&self) -> CoreResult<PathBuf>;
}

#[derive(Debug, Default)]
pub struct WorkbuddyAdapter;

impl TargetAdapter for WorkbuddyAdapter {
    fn kind(&self) -> TargetKind {
        TargetKind::Workbuddy
    }

    fn default_path(&self) -> CoreResult<PathBuf> {
        home_config_path(".workbuddy")
    }
}

#[derive(Debug, Default)]
pub struct CodebuddyAdapter;

impl TargetAdapter for CodebuddyAdapter {
    fn kind(&self) -> TargetKind {
        TargetKind::Codebuddy
    }

    fn default_path(&self) -> CoreResult<PathBuf> {
        home_config_path(".codebuddy")
    }
}

pub fn adapters() -> [Box<dyn TargetAdapter>; 2] {
    [Box::new(WorkbuddyAdapter), Box::new(CodebuddyAdapter)]
}

pub fn default_target_paths() -> CoreResult<HashMap<TargetKind, String>> {
    adapters()
        .into_iter()
        .map(|adapter| {
            Ok((
                adapter.kind(),
                adapter.default_path()?.to_string_lossy().to_string(),
            ))
        })
        .collect()
}

pub fn target_path(kind: TargetKind, paths: &HashMap<TargetKind, String>) -> CoreResult<PathBuf> {
    let raw = paths.get(&kind).ok_or_else(|| {
        CoreError::Target(format!("{} path is not configured", kind.display_name()))
    })?;
    expand_home(raw)
}

pub fn target_statuses(
    store: &Store,
    paths: &HashMap<TargetKind, String>,
) -> CoreResult<Vec<TargetStatus>> {
    adapters()
        .into_iter()
        .map(|adapter| {
            let kind = adapter.kind();
            let path = target_path(kind, paths)?;
            target_status(store, kind, &path)
        })
        .collect()
}

fn target_status(store: &Store, kind: TargetKind, path: &Path) -> CoreResult<TargetStatus> {
    let parent_exists = path.parent().is_some_and(Path::exists);
    let file_exists = path.exists();
    let installed = fs::symlink_metadata(path).is_ok() || parent_exists;
    let write_path = target_write_path(path);
    let writable = write_path.as_deref().is_ok_and(is_writable);
    let mut schema = TargetSchema::Missing;
    let mut fingerprint_value = None;
    let mut error = write_path.err().map(|error| error.to_string());

    if file_exists {
        match read_target_file(path) {
            Ok(bytes) => {
                fingerprint_value = Some(fingerprint(&bytes));
                match ConfigDocument::parse(&bytes) {
                    Ok(document) => schema = document.schema(),
                    Err(parse_error) => {
                        schema = TargetSchema::Invalid;
                        error = Some(parse_error.to_string());
                    }
                }
            }
            Err(read_error) => {
                schema = TargetSchema::Invalid;
                error = Some(read_error.to_string());
            }
        }
    }

    let drifted = store
        .target_last_published_hash(kind)?
        .is_some_and(|published| fingerprint_value.as_deref() != Some(published.as_str()));

    Ok(TargetStatus {
        kind,
        display_name: kind.display_name().to_string(),
        path: path.to_string_lossy().to_string(),
        installed,
        file_exists,
        writable,
        schema,
        fingerprint: fingerprint_value,
        drifted,
        error,
    })
}

#[derive(Debug, Clone)]
pub struct ConfigDocument {
    root: Value,
    schema: TargetSchema,
}

impl ConfigDocument {
    pub fn empty() -> Self {
        Self {
            root: Value::Array(Vec::new()),
            schema: TargetSchema::Array,
        }
    }

    pub fn parse(bytes: &[u8]) -> CoreResult<Self> {
        if bytes.len() > MAX_TARGET_CONFIG_BYTES {
            return Err(CoreError::Target(
                "models.json is too large; the limit is 8 MiB".to_string(),
            ));
        }
        let root: Value = serde_json::from_slice(bytes)
            .map_err(|error| CoreError::Target(format!("Invalid models.json: {error}")))?;
        let schema = if root.is_array() {
            TargetSchema::Array
        } else if root.get("models").is_some_and(Value::is_array) {
            TargetSchema::Wrapped
        } else {
            return Err(CoreError::Target(
                "models.json must be an array or an object containing a models array".to_string(),
            ));
        };
        let models = match schema {
            TargetSchema::Array => root.as_array().expect("array schema"),
            TargetSchema::Wrapped => root["models"].as_array().expect("wrapped schema"),
            _ => unreachable!("schema is validated above"),
        };
        if models.len() > MAX_TARGET_MODELS {
            return Err(CoreError::Target(format!(
                "models.json exceeds the {MAX_TARGET_MODELS} model limit"
            )));
        }
        let mut ids = HashSet::new();
        for model in models {
            if let Some(id) = model.get("id").and_then(Value::as_str) {
                if !ids.insert(id) {
                    return Err(CoreError::Target(format!(
                        "models.json contains duplicate model ID {id}"
                    )));
                }
            }
        }
        Ok(Self { root, schema })
    }

    pub fn read(path: &Path) -> CoreResult<(Self, Option<Vec<u8>>)> {
        if !path.exists() {
            return Ok((Self::empty(), None));
        }
        let bytes = read_target_file(path)?;
        let document = Self::parse(&bytes)?;
        Ok((document, Some(bytes)))
    }

    pub fn schema(&self) -> TargetSchema {
        self.schema.clone()
    }

    pub fn models(&self) -> &[Value] {
        match self.schema {
            TargetSchema::Array => self.root.as_array().expect("array schema"),
            TargetSchema::Wrapped => self
                .root
                .get("models")
                .and_then(Value::as_array)
                .expect("wrapped schema"),
            _ => &[],
        }
    }

    pub fn merge(&mut self, incoming: &[Value]) -> MergeSummary {
        let models = match self.schema {
            TargetSchema::Array => self.root.as_array_mut().expect("array schema"),
            TargetSchema::Wrapped => self
                .root
                .get_mut("models")
                .and_then(Value::as_array_mut)
                .expect("wrapped schema"),
            _ => unreachable!("invalid schemas are rejected before merge"),
        };

        let mut summary = MergeSummary::default();
        for new_model in incoming {
            let id = new_model
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if let Some(index) = models
                .iter()
                .position(|model| model.get("id").and_then(Value::as_str) == Some(id))
            {
                let merged = merge_known_fields(&models[index], new_model);
                if models[index] == merged {
                    summary.unchanged_count += 1;
                } else {
                    models[index] = merged;
                    summary.update_count += 1;
                }
            } else {
                models.push(new_model.clone());
                summary.add_count += 1;
            }
        }
        summary
    }

    pub fn sync(&mut self, incoming: &[Value], managed: &[Value]) -> MergeSummary {
        let selected_ids: HashSet<_> = incoming
            .iter()
            .filter_map(|model| model.get("id").and_then(Value::as_str))
            .collect();
        let managed_identities: HashMap<_, _> = managed
            .iter()
            .filter_map(model_identity)
            .map(|identity| (identity.key.clone(), identity))
            .collect();
        let mut remove_count = 0;

        match self.schema {
            TargetSchema::Array => self.root.as_array_mut().expect("array schema"),
            TargetSchema::Wrapped => self
                .root
                .get_mut("models")
                .and_then(Value::as_array_mut)
                .expect("wrapped schema"),
            _ => unreachable!("invalid schemas are rejected before sync"),
        }
        .retain(|model| {
            let is_selected = model
                .get("id")
                .and_then(Value::as_str)
                .is_some_and(|id| selected_ids.contains(id));
            let should_remove = !is_selected
                && model_identity(model).is_some_and(|identity| {
                    managed_identities
                        .get(&identity.key)
                        .is_some_and(|managed| identity.belongs_to(managed))
                });
            if should_remove {
                remove_count += 1;
            }
            !should_remove
        });

        let mut summary = self.merge(incoming);
        summary.remove_count = remove_count;
        summary
    }

    pub fn collisions(&self, ids: &HashSet<&str>) -> Vec<(String, String)> {
        self.models()
            .iter()
            .filter_map(|model| {
                let id = model.get("id")?.as_str()?;
                ids.contains(id).then(|| {
                    (
                        id.to_string(),
                        model
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or(id)
                            .to_string(),
                    )
                })
            })
            .collect()
    }

    pub fn to_bytes(&self) -> CoreResult<Vec<u8>> {
        let mut bytes = serde_json::to_vec_pretty(&self.root)
            .map_err(|error| CoreError::Target(error.to_string()))?;
        bytes.push(b'\n');
        Ok(bytes)
    }
}

pub fn read_target_file(path: &Path) -> CoreResult<Vec<u8>> {
    let file = fs::File::open(path).map_err(|error| {
        CoreError::Target(format!("Could not read {}: {error}", path.display()))
    })?;
    let mut bytes = Vec::new();
    file.take((MAX_TARGET_CONFIG_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            CoreError::Target(format!("Could not read {}: {error}", path.display()))
        })?;
    if bytes.len() > MAX_TARGET_CONFIG_BYTES {
        return Err(CoreError::Target(format!(
            "{} is too large; the limit is 8 MiB",
            path.display()
        )));
    }
    Ok(bytes)
}

#[derive(Debug, Clone, Default)]
pub struct MergeSummary {
    pub add_count: usize,
    pub update_count: usize,
    pub unchanged_count: usize,
    pub remove_count: usize,
}

#[derive(Clone, Hash, PartialEq, Eq)]
struct ModelIdentityKey {
    id: String,
    api_key: String,
}

struct ModelIdentity {
    key: ModelIdentityKey,
    url: String,
    use_custom_protocol: bool,
}

impl ModelIdentity {
    fn belongs_to(&self, managed: &Self) -> bool {
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

fn model_identity(model: &Value) -> Option<ModelIdentity> {
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

pub fn model_config(model: &ManagedModel, gateway: &GatewayProfile, token: &str) -> Value {
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

fn prefixed_model_name(gateway_name: &str, model_name: &str) -> String {
    let prefix = format!("{gateway_name} · ");
    if model_name.starts_with(&prefix) {
        model_name.to_string()
    } else {
        format!("{prefix}{model_name}")
    }
}

pub fn fingerprint(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> CoreResult<()> {
    let write_path = target_write_path(path)?;
    let parent = write_path
        .parent()
        .ok_or_else(|| CoreError::Target("Target path has no parent directory".to_string()))?;
    fs::create_dir_all(parent).map_err(|error| {
        CoreError::Target(format!("Could not create {}: {error}", parent.display()))
    })?;

    AtomicFile::new(&write_path, OverwriteBehavior::AllowOverwrite)
        .write(|file| {
            file.write_all(bytes)?;
            file.sync_all()
        })
        .map_err(|error| {
            CoreError::Target(format!("Could not write {}: {error}", path.display()))
        })?;
    secure_permissions(&write_path)?;
    Ok(())
}

pub fn target_write_path(path: &Path) -> CoreResult<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().map_err(CoreError::from)?.join(path)
    };
    match fs::symlink_metadata(&absolute) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            fs::canonicalize(&absolute).map_err(|_| {
                CoreError::Target(format!(
                    "Target path is a dangling symlink: {}",
                    path.display()
                ))
            })
        }
        Ok(_) => fs::canonicalize(&absolute).map_err(|error| {
            CoreError::Target(format!("Could not resolve {}: {error}", path.display(),))
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            resolve_missing_path(&absolute)
        }
        Err(error) => Err(CoreError::Target(format!(
            "Could not inspect {}: {error}",
            path.display()
        ))),
    }
}

fn resolve_missing_path(path: &Path) -> CoreResult<PathBuf> {
    let mut cursor = path;
    let mut missing = Vec::new();
    while !cursor.exists() {
        let file_name = cursor.file_name().ok_or_else(|| {
            CoreError::Target(format!("Could not resolve target path: {}", path.display()))
        })?;
        missing.push(file_name.to_os_string());
        cursor = cursor.parent().ok_or_else(|| {
            CoreError::Target(format!("Could not resolve target path: {}", path.display()))
        })?;
    }
    let mut resolved = fs::canonicalize(cursor).map_err(|error| {
        CoreError::Target(format!("Could not resolve {}: {error}", path.display()))
    })?;
    for component in missing.iter().rev() {
        resolved.push(component);
    }
    Ok(resolved)
}

pub fn secure_permissions(path: &Path) -> CoreResult<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|error| {
            CoreError::Target(format!("Could not secure {}: {error}", path.display()))
        })?;
    }

    #[cfg(windows)]
    secure_windows_permissions(path)?;

    Ok(())
}

#[cfg(windows)]
fn secure_windows_permissions(path: &Path) -> CoreResult<()> {
    use std::{ffi::c_void, mem::size_of, os::windows::ffi::OsStrExt, ptr};

    use windows_sys::Win32::{
        Foundation::{CloseHandle, GENERIC_ALL, HANDLE},
        Security::{
            AddAccessAllowedAce,
            Authorization::{SetNamedSecurityInfoW, SE_FILE_OBJECT},
            GetLengthSid, GetTokenInformation, InitializeAcl, TokenUser, ACCESS_ALLOWED_ACE, ACL,
            ACL_REVISION, DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
            TOKEN_QUERY, TOKEN_USER,
        },
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };

    let mut token: HANDLE = ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(windows_permission_error(path, None));
    }

    let result = (|| {
        let mut token_info_size = 0;
        unsafe { GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut token_info_size) };
        if token_info_size == 0 {
            return Err(windows_permission_error(path, None));
        }

        // usize storage keeps the buffer aligned for TOKEN_USER.
        let mut token_info = vec![0usize; (token_info_size as usize).div_ceil(size_of::<usize>())];
        if unsafe {
            GetTokenInformation(
                token,
                TokenUser,
                token_info.as_mut_ptr().cast::<c_void>(),
                token_info_size,
                &mut token_info_size,
            )
        } == 0
        {
            return Err(windows_permission_error(path, None));
        }

        let user = unsafe { &*token_info.as_ptr().cast::<TOKEN_USER>() };
        let sid_length = unsafe { GetLengthSid(user.User.Sid) } as usize;
        if sid_length == 0 {
            return Err(windows_permission_error(path, None));
        }

        let acl_size =
            size_of::<ACL>() + size_of::<ACCESS_ALLOWED_ACE>() + sid_length - size_of::<u32>();
        let mut acl_storage = vec![0u32; acl_size.div_ceil(size_of::<u32>())];
        let acl = acl_storage.as_mut_ptr().cast::<ACL>();
        if unsafe { InitializeAcl(acl, acl_size as u32, ACL_REVISION) } == 0
            || unsafe { AddAccessAllowedAce(acl, ACL_REVISION, GENERIC_ALL, user.User.Sid) } == 0
        {
            return Err(windows_permission_error(path, None));
        }

        let wide_path: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let status = unsafe {
            SetNamedSecurityInfoW(
                wide_path.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                acl,
                ptr::null_mut(),
            )
        };
        if status != 0 {
            return Err(windows_permission_error(path, Some(status)));
        }
        Ok(())
    })();

    unsafe { CloseHandle(token) };
    result
}

#[cfg(windows)]
fn windows_permission_error(path: &Path, status: Option<u32>) -> CoreError {
    let error = status
        .map(|code| std::io::Error::from_raw_os_error(code as i32))
        .unwrap_or_else(std::io::Error::last_os_error);
    CoreError::Target(format!("Could not secure {}: {error}", path.display()))
}

fn merge_known_fields(existing: &Value, incoming: &Value) -> Value {
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

fn home_config_path(directory: &str) -> CoreResult<PathBuf> {
    dirs::home_dir()
        .map(|home| home.join(directory).join("models.json"))
        .ok_or_else(|| CoreError::Target("Could not locate the user home directory".to_string()))
}

fn expand_home(raw: &str) -> CoreResult<PathBuf> {
    if raw == "~" || raw.starts_with("~/") || raw.starts_with("~\\") {
        let home = dirs::home_dir().ok_or_else(|| {
            CoreError::Target("Could not locate the user home directory".to_string())
        })?;
        let suffix = raw.trim_start_matches('~').trim_start_matches(['/', '\\']);
        Ok(home.join(suffix))
    } else {
        Ok(PathBuf::from(raw))
    }
}

fn is_writable(path: &Path) -> bool {
    let check_path = if path.exists() {
        path
    } else if let Some(parent) = path.parent() {
        parent
    } else {
        return false;
    };
    if !check_path.exists() {
        return check_path.parent().is_some_and(is_writable);
    }
    let Ok(metadata) = fs::metadata(check_path) else {
        return false;
    };
    if metadata.permissions().readonly() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o222 != 0
    }
    #[cfg(not(unix))]
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{
        CapabilitySet, ModelConfiguration, ReasoningConfiguration, ReasoningEffort,
        ReasoningSummary,
    };

    #[cfg(unix)]
    #[test]
    fn atomic_write_preserves_a_valid_symlink() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("actual.json");
        let link = directory.path().join("models.json");
        fs::write(&destination, b"[]\n").unwrap();
        symlink(&destination, &link).unwrap();

        atomic_write(&link, b"[{\"id\":\"gpt-5\"}]\n").unwrap();

        assert!(fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(fs::read(&destination).unwrap(), b"[{\"id\":\"gpt-5\"}]\n");
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_rejects_a_dangling_symlink() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().unwrap();
        let link = directory.path().join("models.json");
        symlink(directory.path().join("missing.json"), &link).unwrap();

        let error = atomic_write(&link, b"[]\n").unwrap_err();

        assert!(error.to_string().contains("dangling symlink"));
        assert!(fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
    }

    #[cfg(unix)]
    #[test]
    fn target_status_reports_a_dangling_symlink_as_unwritable() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(&directory.path().join("everybuddy.db")).unwrap();
        let link = directory.path().join("models.json");
        symlink(directory.path().join("missing.json"), &link).unwrap();

        let status = target_status(&store, TargetKind::Workbuddy, &link).unwrap();

        assert!(status.installed);
        assert!(!status.file_exists);
        assert!(!status.writable);
        assert!(status
            .error
            .is_some_and(|error| error.contains("dangling symlink")));
    }

    #[test]
    fn preserves_array_shape_and_unknown_model_fields() {
        let bytes = br#"[{"id":"gpt-5","name":"Old","custom":"keep"}]"#;
        let mut document = ConfigDocument::parse(bytes).unwrap();
        let summary = document.merge(&[json!({"id":"gpt-5","name":"New"})]);
        let output: Value = serde_json::from_slice(&document.to_bytes().unwrap()).unwrap();

        assert_eq!(summary.update_count, 1);
        assert_eq!(output[0]["name"], "New");
        assert_eq!(output[0]["custom"], "keep");
    }

    #[test]
    fn preserves_wrapped_shape_and_unknown_top_level_fields() {
        let bytes = br#"{"models":[],"availableModels":["existing"],"custom":true}"#;
        let mut document = ConfigDocument::parse(bytes).unwrap();
        document.merge(&[json!({"id":"gpt-5","name":"GPT-5"})]);
        let output: Value = serde_json::from_slice(&document.to_bytes().unwrap()).unwrap();

        assert!(output.is_object());
        assert_eq!(output["models"][0]["id"], "gpt-5");
        assert_eq!(output["availableModels"][0], "existing");
        assert_eq!(output["custom"], true);
    }

    #[test]
    fn sync_preserves_same_credentials_outside_the_managed_gateway_root() {
        let mut document = ConfigDocument::parse(
            br#"[
                {"id":"outside-path","url":"https://api.example.com/v2/images","apiKey":"token","useCustomProtocol":true},
                {"id":"outside-origin","url":"https://other.example.com/v1/images","apiKey":"token","useCustomProtocol":true}
            ]"#,
        )
        .unwrap();
        let managed = [
            json!({"id":"outside-path","url":"https://api.example.com/v1","apiKey":"token","useCustomProtocol":false}),
            json!({"id":"outside-origin","url":"https://api.example.com/v1","apiKey":"token","useCustomProtocol":false}),
        ];

        let summary = document.sync(&[], &managed);

        assert_eq!(summary.remove_count, 0);
        assert_eq!(document.models().len(), 2);
    }

    #[test]
    fn rejects_unrecognized_schema() {
        assert!(ConfigDocument::parse(br#"{"items":[]}"#).is_err());
    }

    #[test]
    fn writes_the_complete_workbuddy_model_contract() {
        let model = ManagedModel {
            key: "gateway::gpt-5".to_string(),
            gateway_id: "gateway".to_string(),
            id: "gpt-5".to_string(),
            name: "GPT-5".to_string(),
            vendor: "openai".to_string(),
            capabilities: CapabilitySet {
                supports_tool_call: true,
                supports_images: true,
                supports_reasoning: true,
                reasoning_efforts: Vec::new(),
            },
            configuration: ModelConfiguration {
                endpoint_override: Some("https://proxy.example.com/route".to_string()),
                max_input_tokens: Some(262_144),
                max_output_tokens: Some(32_768),
                temperature: Some(0.7),
                only_reasoning: true,
                reasoning: ReasoningConfiguration {
                    effort: Some(ReasoningEffort::Low),
                    default_effort: Some(ReasoningEffort::High),
                    supported_efforts: vec![
                        ReasoningEffort::Minimal,
                        ReasoningEffort::Low,
                        ReasoningEffort::Medium,
                        ReasoningEffort::High,
                        ReasoningEffort::Xhigh,
                        ReasoningEffort::Max,
                    ],
                    summary: Some(ReasoningSummary::Concise),
                    can_disable_thinking: false,
                },
                use_custom_protocol: true,
            },
            evidence: Vec::new(),
            metadata: Value::Null,
            updated_at: "2026-08-20T00:00:00Z".to_string(),
        };
        let gateway = GatewayProfile {
            id: "gateway".to_string(),
            name: "Gateway".to_string(),
            api_root: "https://api.example.com/v1".to_string(),
            token_ref: "gateway".to_string(),
            created_at: "2026-08-20T00:00:00Z".to_string(),
            updated_at: "2026-08-20T00:00:00Z".to_string(),
        };

        let output = model_config(&model, &gateway, "secret");

        assert_eq!(output["id"], "gpt-5");
        assert_eq!(output["name"], "Gateway · GPT-5");
        assert_eq!(output["vendor"], "openai");
        assert_eq!(output["url"], "https://proxy.example.com/route");
        assert_eq!(output["apiKey"], "secret");
        assert_eq!(output["maxInputTokens"], 262_144);
        assert_eq!(output["maxOutputTokens"], 32_768);
        assert_eq!(output["temperature"], 0.7);
        assert_eq!(output["supportsToolCall"], true);
        assert_eq!(output["supportsImages"], true);
        assert_eq!(output["supportsReasoning"], true);
        assert_eq!(output["onlyReasoning"], true);
        assert_eq!(output["useCustomProtocol"], true);
        assert_eq!(output["reasoning"]["effort"], "low");
        assert_eq!(output["reasoning"]["defaultEffort"], "high");
        assert_eq!(output["reasoning"]["summary"], "concise");
        assert_eq!(output["reasoning"]["canDisableThinking"], false);
        assert_eq!(output["reasoning"]["supportedEfforts"][5], "max");
    }

    #[test]
    fn does_not_duplicate_an_existing_gateway_name_prefix() {
        assert_eq!(
            prefixed_model_name("Gateway", "Gateway · GPT-5"),
            "Gateway · GPT-5"
        );
    }

    #[test]
    fn rejects_duplicate_model_ids_in_target_config() {
        let error =
            ConfigDocument::parse(br#"[{"id":"duplicate"},{"id":"duplicate"}]"#).unwrap_err();

        assert!(error.to_string().contains("duplicate model ID"));
    }

    #[test]
    fn rejects_oversized_target_config() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("models.json");
        let bytes = format!(
            r#"[{{"id":"large","note":"{}"}}]"#,
            "x".repeat(9 * 1024 * 1024)
        );
        fs::write(&path, bytes).unwrap();

        let error = ConfigDocument::read(&path).unwrap_err();

        assert!(error.to_string().contains("too large"));
    }

    #[test]
    fn rejects_too_many_target_models() {
        let models: Vec<_> = (0..=MAX_TARGET_MODELS)
            .map(|index| json!({"id": format!("model-{index}")}))
            .collect();
        let bytes = serde_json::to_vec(&models).unwrap();

        let error = ConfigDocument::parse(&bytes).unwrap_err();

        assert!(error.to_string().contains("model limit"));
    }

    #[test]
    fn target_status_fails_closed_when_drift_state_cannot_be_read() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(&directory.path().join("everybuddy.db")).unwrap();
        store.execute_test_sql("DROP TABLE target_states").unwrap();
        let paths = HashMap::from([
            (
                TargetKind::Workbuddy,
                directory
                    .path()
                    .join("work.json")
                    .to_string_lossy()
                    .to_string(),
            ),
            (
                TargetKind::Codebuddy,
                directory
                    .path()
                    .join("code.json")
                    .to_string_lossy()
                    .to_string(),
            ),
        ]);

        assert!(target_statuses(&store, &paths).is_err());
    }

    #[test]
    fn clears_managed_optional_fields_and_preserves_unknown_fields() {
        let existing = json!({
            "id": "plain-model",
            "name": "Old",
            "maxInputTokens": 100,
            "maxOutputTokens": 50,
            "temperature": 0.5,
            "reasoning": {"effort": "high", "providerOption": true},
            "providerOption": "keep"
        });
        let incoming = json!({
            "id": "plain-model",
            "name": "New",
            "onlyReasoning": false,
            "useCustomProtocol": false
        });

        let merged = merge_known_fields(&existing, &incoming);

        assert!(merged.get("maxInputTokens").is_none());
        assert!(merged.get("maxOutputTokens").is_none());
        assert!(merged.get("temperature").is_none());
        assert!(merged["reasoning"].get("effort").is_none());
        assert_eq!(merged["reasoning"]["providerOption"], true);
        assert_eq!(merged["providerOption"], "keep");
    }

    #[test]
    fn preserves_unknown_reasoning_fields_while_replacing_known_values() {
        let existing = json!({
            "id": "reasoner",
            "reasoning": {"effort": "low", "providerOption": true}
        });
        let incoming = json!({
            "id": "reasoner",
            "reasoning": {"supportedEfforts": ["high"], "canDisableThinking": false}
        });

        let merged = merge_known_fields(&existing, &incoming);

        assert!(merged["reasoning"].get("effort").is_none());
        assert_eq!(merged["reasoning"]["supportedEfforts"][0], "high");
        assert_eq!(merged["reasoning"]["providerOption"], true);
    }
}
