use crate::error::{CoreError, CoreResult};

const SERVICE_NAME: &str = "com.everybuddy.desktop.gateway";
const LEGACY_SERVICE_NAME: &str = "com.everybuddy.app.gateway";
pub const MISSING_SECRET_MESSAGE: &str =
    "The gateway token is missing from the system credential store";

pub trait SecretStore: Send + Sync {
    fn set(&self, key: &str, secret: &str) -> CoreResult<()>;
    fn get(&self, key: &str) -> CoreResult<String>;
    fn delete(&self, key: &str) -> CoreResult<()>;
}

#[derive(Debug, Default)]
pub struct SystemSecretStore;

impl SecretStore for SystemSecretStore {
    fn set(&self, key: &str, secret: &str) -> CoreResult<()> {
        KeyringBackend.set(SERVICE_NAME, key, secret)
    }

    fn get(&self, key: &str) -> CoreResult<String> {
        get_with_legacy_migration(&KeyringBackend, key)
    }

    fn delete(&self, key: &str) -> CoreResult<()> {
        delete_all_credentials(&KeyringBackend, key)
    }
}

trait CredentialBackend {
    fn set(&self, service: &str, key: &str, secret: &str) -> CoreResult<()>;
    fn get(&self, service: &str, key: &str) -> CoreResult<Option<String>>;
    fn delete(&self, service: &str, key: &str) -> CoreResult<()>;
}

#[derive(Debug, Default)]
struct KeyringBackend;

impl CredentialBackend for KeyringBackend {
    fn set(&self, service: &str, key: &str, secret: &str) -> CoreResult<()> {
        let entry = keyring::Entry::new(service, key)
            .map_err(|error| CoreError::SecretStore(error.to_string()))?;
        entry
            .set_password(secret)
            .map_err(|_| credential_store_unavailable())
    }

    fn get(&self, service: &str, key: &str) -> CoreResult<Option<String>> {
        let entry = keyring::Entry::new(service, key)
            .map_err(|error| CoreError::SecretStore(error.to_string()))?;
        match entry.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(credential_store_unavailable()),
        }
    }

    fn delete(&self, service: &str, key: &str) -> CoreResult<()> {
        let entry = keyring::Entry::new(service, key)
            .map_err(|error| CoreError::SecretStore(error.to_string()))?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(credential_store_unavailable()),
        }
    }
}

fn get_with_legacy_migration(backend: &dyn CredentialBackend, key: &str) -> CoreResult<String> {
    if let Some(secret) = backend.get(SERVICE_NAME, key)? {
        return Ok(secret);
    }
    if let Some(secret) = backend.get(LEGACY_SERVICE_NAME, key)? {
        backend.set(SERVICE_NAME, key, &secret)?;
        backend.delete(LEGACY_SERVICE_NAME, key)?;
        return Ok(secret);
    }
    Err(CoreError::SecretStore(MISSING_SECRET_MESSAGE.to_string()))
}

fn delete_all_credentials(backend: &dyn CredentialBackend, key: &str) -> CoreResult<()> {
    let current = backend.delete(SERVICE_NAME, key);
    let legacy = backend.delete(LEGACY_SERVICE_NAME, key);
    current.and(legacy)
}

fn credential_store_unavailable() -> CoreError {
    CoreError::SecretStore("The system credential store is unavailable".to_string())
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Mutex};

    use super::*;

    #[derive(Default)]
    struct MemoryBackend {
        values: Mutex<HashMap<(String, String), String>>,
    }

    impl CredentialBackend for MemoryBackend {
        fn set(&self, service: &str, key: &str, secret: &str) -> CoreResult<()> {
            self.values
                .lock()
                .unwrap()
                .insert((service.to_string(), key.to_string()), secret.to_string());
            Ok(())
        }

        fn get(&self, service: &str, key: &str) -> CoreResult<Option<String>> {
            Ok(self
                .values
                .lock()
                .unwrap()
                .get(&(service.to_string(), key.to_string()))
                .cloned())
        }

        fn delete(&self, service: &str, key: &str) -> CoreResult<()> {
            self.values
                .lock()
                .unwrap()
                .remove(&(service.to_string(), key.to_string()));
            Ok(())
        }
    }

    #[test]
    fn migrates_legacy_secret_and_deletes_the_original() {
        let backend = MemoryBackend::default();
        backend
            .set(LEGACY_SERVICE_NAME, "gateway", "legacy-token")
            .unwrap();

        let token = get_with_legacy_migration(&backend, "gateway").unwrap();

        assert_eq!(token, "legacy-token");
        assert_eq!(
            backend.get(SERVICE_NAME, "gateway").unwrap().as_deref(),
            Some("legacy-token")
        );
        assert!(backend
            .get(LEGACY_SERVICE_NAME, "gateway")
            .unwrap()
            .is_none());
    }

    #[test]
    fn deletes_current_and_legacy_credentials() {
        let backend = MemoryBackend::default();
        backend
            .set(SERVICE_NAME, "gateway", "current-token")
            .unwrap();
        backend
            .set(LEGACY_SERVICE_NAME, "gateway", "legacy-token")
            .unwrap();

        delete_all_credentials(&backend, "gateway").unwrap();

        assert!(backend.get(SERVICE_NAME, "gateway").unwrap().is_none());
        assert!(backend
            .get(LEGACY_SERVICE_NAME, "gateway")
            .unwrap()
            .is_none());
    }

    #[test]
    fn prefers_the_current_service_secret() {
        let backend = MemoryBackend::default();
        backend
            .set(LEGACY_SERVICE_NAME, "gateway", "legacy-token")
            .unwrap();
        backend
            .set(SERVICE_NAME, "gateway", "current-token")
            .unwrap();

        assert_eq!(
            get_with_legacy_migration(&backend, "gateway").unwrap(),
            "current-token"
        );
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[test]
    fn system_secret_store_uses_a_native_backend() {
        let entry = keyring::Entry::new(SERVICE_NAME, "backend-check").unwrap();

        assert!(!entry.get_credential().is::<keyring::mock::MockCredential>());
    }
}
