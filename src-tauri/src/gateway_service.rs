use std::sync::Arc;

use crate::{
    error::{CoreError, CoreResult},
    models::GatewayProfile,
    secrets::{SecretStore, MISSING_SECRET_MESSAGE},
    store::Store,
};

pub trait GatewayRepository {
    fn find_gateway(&self, id: &str) -> CoreResult<Option<GatewayProfile>>;
    fn persist_gateway(&self, profile: &GatewayProfile) -> CoreResult<()>;
    fn remove_gateway(&self, id: &str) -> CoreResult<()>;
}

impl GatewayRepository for Store {
    fn find_gateway(&self, id: &str) -> CoreResult<Option<GatewayProfile>> {
        self.find_gateway(id)
    }

    fn persist_gateway(&self, profile: &GatewayProfile) -> CoreResult<()> {
        self.save_gateway(profile)
    }

    fn remove_gateway(&self, id: &str) -> CoreResult<()> {
        self.delete_gateway(id)
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

    pub fn save(&self, profile: &GatewayProfile, token: &str) -> CoreResult<()> {
        let previous = self.repository.find_gateway(&profile.id)?;
        let previous_token = previous
            .as_ref()
            .map(|gateway| optional_secret(self.secrets.as_ref(), &gateway.token_ref))
            .transpose()?
            .flatten();

        self.secrets.set(&profile.token_ref, token)?;
        if let Err(error) = self.repository.persist_gateway(profile) {
            let compensation = match previous_token {
                Some(secret) => self.secrets.set(&profile.token_ref, &secret),
                None => self.secrets.delete(&profile.token_ref),
            };
            return Err(compensation_error("save", error, compensation));
        }
        Ok(())
    }

    pub fn delete(&self, id: &str) -> CoreResult<()> {
        let profile = self
            .repository
            .find_gateway(id)?
            .ok_or_else(|| CoreError::Validation("Gateway profile not found".to_string()))?;
        let previous_token = optional_secret(self.secrets.as_ref(), &profile.token_ref)?;

        self.secrets.delete(&profile.token_ref)?;
        if let Err(error) = self.repository.remove_gateway(id) {
            let compensation = match previous_token {
                Some(secret) => self.secrets.set(&profile.token_ref, &secret),
                None => Ok(()),
            };
            return Err(compensation_error("delete", error, compensation));
        }
        Ok(())
    }
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
        fail_save: bool,
        fail_delete: bool,
    }

    impl GatewayRepository for FakeRepository {
        fn find_gateway(&self, id: &str) -> CoreResult<Option<GatewayProfile>> {
            Ok(self.gateways.lock().unwrap().get(id).cloned())
        }

        fn persist_gateway(&self, profile: &GatewayProfile) -> CoreResult<()> {
            if self.fail_save {
                return Err(CoreError::Storage("injected save failure".to_string()));
            }
            self.gateways
                .lock()
                .unwrap()
                .insert(profile.id.clone(), profile.clone());
            Ok(())
        }

        fn remove_gateway(&self, id: &str) -> CoreResult<()> {
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
        };
        let secrets = Arc::new(FakeSecretStore::default());
        secrets.set("gateway", "old-token").unwrap();
        *secrets.fail_set_on_call.lock().unwrap() = Some(3);
        let service = GatewayService::new(&repository, secrets);

        let error = service.save(&profile("Updated"), "new-token").unwrap_err();

        assert!(error
            .to_string()
            .contains("credential recovery also failed"));
        assert!(!error.to_string().contains("old-token"));
        assert!(!error.to_string().contains("new-token"));
    }
}
