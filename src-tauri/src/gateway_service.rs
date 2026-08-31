use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

use crate::{
    error::{CoreError, CoreResult},
    models::GatewayProfile,
    secrets::MISSING_CREDENTIAL_MESSAGE,
    store::Store,
};

pub trait GatewayRepository {
    fn find_gateway(&self, id: &str) -> CoreResult<Option<GatewayProfile>>;
    fn gateway_token(&self, id: &str) -> CoreResult<Option<String>>;
    fn source_identity_key(&self) -> CoreResult<String>;
    fn persist_gateway(
        &self,
        profile: &GatewayProfile,
        token: &str,
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

    fn gateway_token(&self, id: &str) -> CoreResult<Option<String>> {
        self.optional_gateway_token(id)
    }

    fn source_identity_key(&self) -> CoreResult<String> {
        Store::source_identity_key(self)
    }

    fn persist_gateway(
        &self,
        profile: &GatewayProfile,
        token: &str,
        invalidate_models: bool,
        source_hash: &str,
        previous_source_hash: Option<&str>,
    ) -> CoreResult<()> {
        self.save_gateway_with_provenance(
            profile,
            token,
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
}

impl<'a, R: GatewayRepository + ?Sized> GatewayService<'a, R> {
    pub fn new(repository: &'a R) -> Self {
        Self { repository }
    }

    #[cfg(test)]
    pub fn save(&self, profile: &GatewayProfile, token: &str) -> CoreResult<bool> {
        self.save_optional(profile, Some(token))
    }

    pub fn save_optional(
        &self,
        profile: &GatewayProfile,
        replacement_token: Option<&str>,
    ) -> CoreResult<bool> {
        let previous = self.repository.find_gateway(&profile.id)?;
        let previous_token = previous
            .as_ref()
            .map(|gateway| self.repository.gateway_token(&gateway.id))
            .transpose()?
            .flatten();
        let token = replacement_token
            .map(str::trim)
            .filter(|token| !token.is_empty())
            .map(ToString::to_string)
            .or_else(|| previous_token.clone())
            .ok_or_else(|| {
                if previous.is_some() {
                    CoreError::Credential(MISSING_CREDENTIAL_MESSAGE.to_string())
                } else {
                    CoreError::Validation("API token is required".to_string())
                }
            })?;
        let models_invalidated = previous.as_ref().is_some_and(|gateway| {
            gateway.api_root != profile.api_root || previous_token.as_deref() != Some(&token)
        });
        let identity_key = self.repository.source_identity_key()?;
        let source_hash = gateway_source_hash(&identity_key, &profile.api_root, &token);
        let previous_source_hash =
            previous
                .as_ref()
                .zip(previous_token.as_deref())
                .map(|(gateway, previous_token)| {
                    gateway_source_hash(&identity_key, &gateway.api_root, previous_token)
                });

        self.repository.persist_gateway(
            profile,
            &token,
            models_invalidated,
            &source_hash,
            previous_source_hash.as_deref(),
        )?;
        Ok(models_invalidated)
    }

    pub fn delete(&self, id: &str) -> CoreResult<()> {
        let profile = self
            .repository
            .find_gateway(id)?
            .ok_or_else(|| CoreError::Validation("Gateway profile not found".to_string()))?;
        let previous_token = self.repository.gateway_token(&profile.id)?;
        let source_hashes = match previous_token.as_deref() {
            Some(token) => {
                let identity_key = self.repository.source_identity_key()?;
                self.repository
                    .gateway_source_roots(id)?
                    .into_iter()
                    .map(|api_root| gateway_source_hash(&identity_key, &api_root, token))
                    .collect()
            }
            None => Vec::new(),
        };

        self.repository.remove_gateway(id, &source_hashes)
    }
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

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Mutex};

    use super::*;

    #[derive(Default)]
    struct FakeRepository {
        gateways: Mutex<HashMap<String, GatewayProfile>>,
        tokens: Mutex<HashMap<String, String>>,
        invalidated: Mutex<Vec<String>>,
        fail_save: bool,
        fail_delete: bool,
    }

    impl GatewayRepository for FakeRepository {
        fn find_gateway(&self, id: &str) -> CoreResult<Option<GatewayProfile>> {
            Ok(self.gateways.lock().unwrap().get(id).cloned())
        }

        fn gateway_token(&self, id: &str) -> CoreResult<Option<String>> {
            Ok(self.tokens.lock().unwrap().get(id).cloned())
        }

        fn source_identity_key(&self) -> CoreResult<String> {
            Ok("identity-key".to_string())
        }

        fn persist_gateway(
            &self,
            profile: &GatewayProfile,
            token: &str,
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
            self.tokens
                .lock()
                .unwrap()
                .insert(profile.id.clone(), token.to_string());
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
            self.tokens.lock().unwrap().remove(id);
            Ok(())
        }
    }

    fn profile(name: &str) -> GatewayProfile {
        GatewayProfile {
            id: "gateway".to_string(),
            name: name.to_string(),
            api_root: "https://api.example.com/v1".to_string(),
            created_at: "2026-08-20T00:00:00Z".to_string(),
            updated_at: "2026-08-20T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn failed_save_keeps_the_previous_profile_and_token() {
        let repository = FakeRepository {
            gateways: Mutex::new(HashMap::from([(
                "gateway".to_string(),
                profile("Existing"),
            )])),
            tokens: Mutex::new(HashMap::from([(
                "gateway".to_string(),
                "old-token".to_string(),
            )])),
            fail_save: true,
            ..Default::default()
        };
        let service = GatewayService::new(&repository);

        assert!(service.save(&profile("Updated"), "new-token").is_err());
        assert_eq!(
            repository.gateways.lock().unwrap()["gateway"].name,
            "Existing"
        );
        assert_eq!(repository.tokens.lock().unwrap()["gateway"], "old-token");
    }

    #[test]
    fn failed_new_profile_save_does_not_store_a_token() {
        let repository = FakeRepository {
            fail_save: true,
            ..Default::default()
        };
        let service = GatewayService::new(&repository);

        assert!(service.save(&profile("New"), "new-token").is_err());
        assert!(repository.tokens.lock().unwrap().is_empty());
    }

    #[test]
    fn failed_delete_keeps_the_profile_and_token() {
        let repository = FakeRepository {
            gateways: Mutex::new(HashMap::from([(
                "gateway".to_string(),
                profile("Existing"),
            )])),
            tokens: Mutex::new(HashMap::from([(
                "gateway".to_string(),
                "saved-token".to_string(),
            )])),
            fail_delete: true,
            ..Default::default()
        };
        let service = GatewayService::new(&repository);

        assert!(service.delete("gateway").is_err());
        assert!(repository.gateways.lock().unwrap().contains_key("gateway"));
        assert_eq!(repository.tokens.lock().unwrap()["gateway"], "saved-token");
    }

    #[test]
    fn invalidates_models_only_when_gateway_provenance_changes() {
        let repository = FakeRepository {
            gateways: Mutex::new(HashMap::from([(
                "gateway".to_string(),
                profile("Existing"),
            )])),
            tokens: Mutex::new(HashMap::from([(
                "gateway".to_string(),
                "old-token".to_string(),
            )])),
            ..Default::default()
        };
        let service = GatewayService::new(&repository);

        assert!(!service.save(&profile("Renamed"), "old-token").unwrap());

        let mut moved = profile("Moved");
        moved.api_root = "https://other.example.com/v1".to_string();
        assert!(service.save(&moved, "old-token").unwrap());
        assert_eq!(
            repository.invalidated.lock().unwrap().as_slice(),
            ["gateway"]
        );
    }

    #[test]
    fn omitted_replacement_keeps_the_existing_credential() {
        let repository = FakeRepository {
            gateways: Mutex::new(HashMap::from([(
                "gateway".to_string(),
                profile("Existing"),
            )])),
            tokens: Mutex::new(HashMap::from([(
                "gateway".to_string(),
                "saved-token".to_string(),
            )])),
            ..Default::default()
        };
        let service = GatewayService::new(&repository);

        assert!(!service.save_optional(&profile("Renamed"), None).unwrap());
        assert_eq!(repository.tokens.lock().unwrap()["gateway"], "saved-token");
        assert_eq!(
            repository
                .gateways
                .lock()
                .unwrap()
                .get("gateway")
                .unwrap()
                .name,
            "Renamed"
        );
    }

    #[test]
    fn new_gateway_requires_a_credential() {
        let repository = FakeRepository::default();
        let service = GatewayService::new(&repository);

        let error = service.save_optional(&profile("New"), None).unwrap_err();

        assert!(matches!(error, CoreError::Validation(_)));
        assert!(repository.gateways.lock().unwrap().is_empty());
        assert!(repository.tokens.lock().unwrap().is_empty());
    }

    #[test]
    fn missing_saved_credential_fails_before_profile_mutation() {
        let original = profile("Existing");
        let repository = FakeRepository {
            gateways: Mutex::new(HashMap::from([("gateway".to_string(), original.clone())])),
            ..Default::default()
        };
        let service = GatewayService::new(&repository);

        let error = service
            .save_optional(&profile("Must not persist"), None)
            .unwrap_err();

        assert!(matches!(error, CoreError::Credential(_)));
        assert_eq!(
            repository.gateways.lock().unwrap().get("gateway").unwrap(),
            &original
        );
        assert!(repository.tokens.lock().unwrap().is_empty());
    }
}
