use std::sync::Arc;

use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use uuid::Uuid;

use crate::{
    error::{CoreError, CoreResult},
    models::GatewayProfile,
    secrets::{SecretStore, MISSING_SECRET_MESSAGE},
    store::Store,
};

pub trait GatewayRepository {
    fn find_gateway(&self, id: &str) -> CoreResult<Option<GatewayProfile>>;
    fn has_source_history(&self) -> CoreResult<bool>;
    fn persist_gateway(
        &self,
        profile: &GatewayProfile,
        invalidate_models: bool,
        source_hash: &str,
        previous_source_hash: Option<&str>,
    ) -> CoreResult<()>;
    fn gateway_source_roots(&self, id: &str) -> CoreResult<Vec<String>>;
    fn remove_gateway(&self, id: &str, source_hashes: &[String]) -> CoreResult<()>;
}

impl GatewayRepository for Store {
    fn find_gateway(&self, id: &str) -> CoreResult<Option<GatewayProfile>> {
        self.find_gateway(id)
    }

    fn has_source_history(&self) -> CoreResult<bool> {
        self.has_gateway_source_history()
    }

    fn persist_gateway(
        &self,
        profile: &GatewayProfile,
        invalidate_models: bool,
        source_hash: &str,
        previous_source_hash: Option<&str>,
    ) -> CoreResult<()> {
        self.save_gateway_with_provenance(
            profile,
            invalidate_models,
            Some(source_hash),
            previous_source_hash,
        )
    }

    fn gateway_source_roots(&self, id: &str) -> CoreResult<Vec<String>> {
        self.gateway_source_roots(id)
    }

    fn remove_gateway(&self, id: &str, source_hashes: &[String]) -> CoreResult<()> {
        self.delete_gateway_with_tombstone(id, source_hashes)
    }
}

pub struct GatewayService<'a, R: GatewayRepository + ?Sized> {
    repository: &'a R,
    secrets: Arc<dyn SecretStore>,
}

impl<'a, R: GatewayRepository + ?Sized> GatewayService<'a, R> {
    pub fn new(repository: &'a R, secrets: Arc<dyn SecretStore>) -> Self {
        Self {
            repository,
            secrets,
        }
    }

    pub fn save(&self, profile: &GatewayProfile, token: &str) -> CoreResult<bool> {
        let previous = self.repository.find_gateway(&profile.id)?;
        let previous_token = previous
            .as_ref()
            .map(|gateway| optional_secret(self.secrets.as_ref(), &gateway.token_ref))
            .transpose()?
            .flatten();
        let models_invalidated = previous.as_ref().is_some_and(|gateway| {
            gateway.api_root != profile.api_root || previous_token.as_deref() != Some(token)
        });
        let identity_key =
            source_identity_key(self.secrets.as_ref(), self.repository.has_source_history()?)?;
        let source_hash = gateway_source_hash(&identity_key, &profile.api_root, token);
        let previous_source_hash =
            previous
                .as_ref()
                .zip(previous_token.as_deref())
                .map(|(gateway, previous_token)| {
                    gateway_source_hash(&identity_key, &gateway.api_root, previous_token)
                });

        self.secrets.set(&profile.token_ref, token)?;
        if let Err(error) = self.repository.persist_gateway(
            profile,
            models_invalidated,
            &source_hash,
            previous_source_hash.as_deref(),
        ) {
            let compensation = match previous_token {
                Some(secret) => self.secrets.set(&profile.token_ref, &secret),
                None => self.secrets.delete(&profile.token_ref),
            };
            return Err(compensation_error("save", error, compensation));
        }
        Ok(models_invalidated)
    }

    pub fn delete(&self, id: &str) -> CoreResult<()> {
        let profile = self
            .repository
            .find_gateway(id)?
            .ok_or_else(|| CoreError::Validation("Gateway profile not found".to_string()))?;
        let previous_token = optional_secret(self.secrets.as_ref(), &profile.token_ref)?;
        let source_hashes = match previous_token.as_deref() {
            Some(token) => {
                let identity_key = source_identity_key(
                    self.secrets.as_ref(),
                    self.repository.has_source_history()?,
                )?;
                self.repository
                    .gateway_source_roots(id)?
                    .into_iter()
                    .map(|api_root| gateway_source_hash(&identity_key, &api_root, token))
                    .collect()
            }
            None => Vec::new(),
        };

        self.secrets.delete(&profile.token_ref)?;
        if let Err(error) = self.repository.remove_gateway(id, &source_hashes) {
            let compensation = match previous_token {
                Some(secret) => self.secrets.set(&profile.token_ref, &secret),
                None => Ok(()),
            };
            return Err(compensation_error("delete", error, compensation));
        }
        Ok(())
    }
}

pub fn source_identity_key(
    store: &dyn SecretStore,
    source_history_exists: bool,
) -> CoreResult<String> {
    const KEY_REF: &str = "__everybuddy_source_identity_key_v1";

    if let Some(key) = optional_secret(store, KEY_REF)? {
        return Ok(key);
    }
    if source_history_exists {
        return Err(CoreError::SecretStore(
            "The source identity key is missing from the system credential store".to_string(),
        ));
    }
    let key = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    store.set(KEY_REF, &key)?;
    Ok(key)
}

pub fn gateway_source_hash(identity_key: &str, api_root: &str, token: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(identity_key.as_bytes())
        .expect("HMAC accepts keys of any length");
    mac.update(b"everybuddy.gateway-source.v1\0");
    mac.update(&(api_root.len() as u64).to_be_bytes());
    mac.update(api_root.as_bytes());
    mac.update(&(token.len() as u64).to_be_bytes());
    mac.update(token.as_bytes());
    format!("v1:{}", hex::encode(mac.finalize().into_bytes()))
}

fn optional_secret(store: &dyn SecretStore, key: &str) -> CoreResult<Option<String>> {
    match store.get(key) {
        Ok(secret) => Ok(Some(secret)),
        Err(CoreError::SecretStore(message)) if message == MISSING_SECRET_MESSAGE => Ok(None),
        Err(error) => Err(error),
    }
}

fn compensation_error(
    operation: &str,
    primary: CoreError,
    compensation: CoreResult<()>,
) -> CoreError {
    match compensation {
        Ok(()) => primary,
        Err(_) => CoreError::Storage(format!(
            "Could not {operation} the gateway, and credential recovery also failed"
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
    };

    use super::*;

    #[derive(Default)]
    struct FakeRepository {
        gateways: Mutex<HashMap<String, GatewayProfile>>,
        invalidated: Mutex<Vec<String>>,
        fail_save: bool,
        fail_delete: bool,
    }

    impl GatewayRepository for FakeRepository {
        fn find_gateway(&self, id: &str) -> CoreResult<Option<GatewayProfile>> {
            Ok(self.gateways.lock().unwrap().get(id).cloned())
        }

        fn has_source_history(&self) -> CoreResult<bool> {
            Ok(false)
        }

        fn persist_gateway(
            &self,
            profile: &GatewayProfile,
            invalidate_models: bool,
            _source_hash: &str,
            _previous_source_hash: Option<&str>,
        ) -> CoreResult<()> {
            if self.fail_save {
                return Err(CoreError::Storage("injected save failure".to_string()));
            }
            self.gateways
                .lock()
                .unwrap()
                .insert(profile.id.clone(), profile.clone());
            if invalidate_models {
                self.invalidated.lock().unwrap().push(profile.id.clone());
            }
            Ok(())
        }

        fn gateway_source_roots(&self, id: &str) -> CoreResult<Vec<String>> {
            Ok(self
                .gateways
                .lock()
                .unwrap()
                .get(id)
                .map(|gateway| vec![gateway.api_root.clone()])
                .unwrap_or_default())
        }

        fn remove_gateway(&self, id: &str, _source_hashes: &[String]) -> CoreResult<()> {
            if self.fail_delete {
                return Err(CoreError::Storage("injected delete failure".to_string()));
            }
            self.gateways.lock().unwrap().remove(id);
            Ok(())
        }
    }

    #[derive(Default)]
    struct FakeSecretStore {
        values: Mutex<HashMap<String, String>>,
        set_calls: Mutex<usize>,
        fail_set_on_call: Mutex<Option<usize>>,
    }

    impl SecretStore for FakeSecretStore {
        fn set(&self, key: &str, secret: &str) -> CoreResult<()> {
            let mut calls = self.set_calls.lock().unwrap();
            *calls += 1;
            if *self.fail_set_on_call.lock().unwrap() == Some(*calls) {
                return Err(CoreError::SecretStore("injected set failure".to_string()));
            }
            self.values
                .lock()
                .unwrap()
                .insert(key.to_string(), secret.to_string());
            Ok(())
        }

        fn get(&self, key: &str) -> CoreResult<String> {
            self.values
                .lock()
                .unwrap()
                .get(key)
                .cloned()
                .ok_or_else(|| CoreError::SecretStore(MISSING_SECRET_MESSAGE.to_string()))
        }

        fn delete(&self, key: &str) -> CoreResult<()> {
            self.values.lock().unwrap().remove(key);
            Ok(())
        }
    }

    fn profile(name: &str) -> GatewayProfile {
        GatewayProfile {
            id: "gateway".to_string(),
            name: name.to_string(),
            api_root: "https://api.example.com/v1".to_string(),
            token_ref: "gateway".to_string(),
            created_at: "2026-08-20T00:00:00Z".to_string(),
            updated_at: "2026-08-20T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn restores_previous_token_when_profile_save_fails() {
        let repository = FakeRepository {
            gateways: Mutex::new(HashMap::from([(
                "gateway".to_string(),
                profile("Existing"),
            )])),
            fail_save: true,
            fail_delete: false,
            ..Default::default()
        };
        let secrets = Arc::new(FakeSecretStore::default());
        secrets.set("gateway", "old-token").unwrap();
        let service = GatewayService::new(&repository, secrets.clone());

        assert!(service.save(&profile("Updated"), "new-token").is_err());
        assert_eq!(secrets.get("gateway").unwrap(), "old-token");
    }

    #[test]
    fn removes_new_token_when_new_profile_save_fails() {
        let repository = FakeRepository {
            fail_save: true,
            ..Default::default()
        };
        let secrets = Arc::new(FakeSecretStore::default());
        let service = GatewayService::new(&repository, secrets.clone());

        assert!(service.save(&profile("New"), "new-token").is_err());
        assert!(secrets.get("gateway").is_err());
    }

    #[test]
    fn restores_token_when_profile_delete_fails() {
        let repository = FakeRepository {
            gateways: Mutex::new(HashMap::from([(
                "gateway".to_string(),
                profile("Existing"),
            )])),
            fail_save: false,
            fail_delete: true,
            ..Default::default()
        };
        let secrets = Arc::new(FakeSecretStore::default());
        secrets.set("gateway", "saved-token").unwrap();
        let service = GatewayService::new(&repository, secrets.clone());

        assert!(service.delete("gateway").is_err());
        assert_eq!(secrets.get("gateway").unwrap(), "saved-token");
    }

    #[test]
    fn reports_failed_credential_compensation_without_exposing_token() {
        let repository = FakeRepository {
            gateways: Mutex::new(HashMap::from([(
                "gateway".to_string(),
                profile("Existing"),
            )])),
            fail_save: true,
            fail_delete: false,
            ..Default::default()
        };
        let secrets = Arc::new(FakeSecretStore::default());
        secrets.set("gateway", "old-token").unwrap();
        *secrets.fail_set_on_call.lock().unwrap() = Some(4);
        let service = GatewayService::new(&repository, secrets);

        let error = service.save(&profile("Updated"), "new-token").unwrap_err();

        assert!(error
            .to_string()
            .contains("credential recovery also failed"));
        assert!(!error.to_string().contains("old-token"));
        assert!(!error.to_string().contains("new-token"));
    }

    #[test]
    fn invalidates_models_only_when_gateway_provenance_changes() {
        let repository = FakeRepository {
            gateways: Mutex::new(HashMap::from([(
                "gateway".to_string(),
                profile("Existing"),
            )])),
            ..Default::default()
        };
        let secrets = Arc::new(FakeSecretStore::default());
        secrets.set("gateway", "old-token").unwrap();
        let service = GatewayService::new(&repository, secrets);

        assert!(!service.save(&profile("Renamed"), "old-token").unwrap());

        let mut moved = profile("Moved");
        moved.api_root = "https://other.example.com/v1".to_string();
        assert!(service.save(&moved, "old-token").unwrap());
        assert_eq!(
            repository.invalidated.lock().unwrap().as_slice(),
            ["gateway"]
        );
    }
}
