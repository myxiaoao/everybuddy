use crate::{
    error::{CoreError, CoreResult},
    models::ManagedModel,
};

pub const ID_BYTES: usize = 512;
pub const NAME_BYTES: usize = 1024;
pub const VENDOR_BYTES: usize = 128;
pub const URL_BYTES: usize = 8192;
pub const TOKEN_BYTES: usize = 16 * 1024;
pub const MODEL_BYTES: usize = 256 * 1024;

pub fn text(value: &str, limit: usize, label: &str) -> CoreResult<()> {
    if value.len() > limit {
        return Err(CoreError::Validation(format!(
            "{label} exceeds the {limit} byte limit; shorten the value and retry"
        )));
    }
    Ok(())
}

pub fn identity(id: &str, name: &str, vendor: &str) -> CoreResult<()> {
    text(id, ID_BYTES, "Model ID")?;
    text(name, NAME_BYTES, "Model name")?;
    text(vendor, VENDOR_BYTES, "Model vendor")
}

pub fn model(model: &ManagedModel) -> CoreResult<()> {
    identity(&model.id, &model.name, &model.vendor)?;
    let bytes =
        serde_json::to_vec(model).map_err(|error| CoreError::Validation(error.to_string()))?;
    if bytes.len() > MODEL_BYTES {
        return Err(CoreError::Validation(
            "Model data exceeds 256 KiB; reduce metadata or configuration and retry".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_utf8_bytes_without_echoing_input() {
        let secret = "secret".repeat(TOKEN_BYTES);
        let error = text(&secret, TOKEN_BYTES, "API token").unwrap_err();
        assert!(!error.to_string().contains(&secret));
        assert!(identity("model", &"模".repeat(342), "vendor").is_err());
        assert!(identity(&"a".repeat(ID_BYTES), "name", "vendor").is_ok());
    }
}
