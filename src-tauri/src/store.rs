use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    str::FromStr,
    sync::{Mutex, MutexGuard},
    time::Duration,
};

use chrono::Utc;
use rusqlite::{params, Connection, DatabaseName, OptionalExtension, TransactionBehavior};
use uuid::Uuid;

use crate::{
    error::{CoreError, CoreResult},
    models::{AppSettings, BackupRecord, GatewayProfile, ManagedModel, TargetKind},
};

pub struct Store {
    connection: Mutex<Connection>,
    database_path: PathBuf,
}

const SCHEMA_VERSION: i64 = 1;

pub struct TargetStateUpdate {
    pub target: TargetKind,
    pub path: String,
    pub seen_hash: Option<String>,
    pub published_hash: Option<String>,
    pub schema: String,
}

impl Store {
    pub fn open(path: &Path) -> CoreResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let database_existed = path.exists() && path.metadata()?.len() > 0;
        let connection = Connection::open(path)?;
        let current_version: i64 =
            connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if current_version > SCHEMA_VERSION {
            return Err(CoreError::Storage(format!(
                "The database schema version {current_version} is newer than this app supports ({SCHEMA_VERSION})"
            )));
        }
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.busy_timeout(Duration::from_secs(5))?;
        let store = Self {
            connection: Mutex::new(connection),
            database_path: path.to_path_buf(),
        };
        store.migrate(current_version, database_existed)?;
        Ok(store)
    }

    fn connection(&self) -> CoreResult<MutexGuard<'_, Connection>> {
        self.connection
            .lock()
            .map_err(|_| CoreError::Storage("The local database lock is unavailable".to_string()))
    }

    fn migrate(&self, current_version: i64, database_existed: bool) -> CoreResult<()> {
        if current_version == SCHEMA_VERSION {
            return Ok(());
        }

        let mut connection = self.connection()?;
        let has_user_tables: bool = connection.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM sqlite_master
                WHERE type = 'table' AND name NOT LIKE 'sqlite_%'
            )",
            [],
            |row| row.get(0),
        )?;
        if database_existed && has_user_tables {
            self.backup_before_migration(&connection, current_version)?;
        }

        let transaction = connection.transaction()?;
        transaction.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS gateway_profiles (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                api_root TEXT NOT NULL,
                token_ref TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS models (
                model_key TEXT PRIMARY KEY,
                gateway_id TEXT NOT NULL,
                upstream_id TEXT NOT NULL,
                name TEXT NOT NULL,
                vendor TEXT NOT NULL,
                capabilities_json TEXT NOT NULL,
                configuration_json TEXT NOT NULL DEFAULT '{}',
                evidence_json TEXT NOT NULL,
                metadata_json TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                FOREIGN KEY(gateway_id) REFERENCES gateway_profiles(id) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS models_gateway_idx ON models(gateway_id);

            CREATE TABLE IF NOT EXISTS target_states (
                target TEXT PRIMARY KEY,
                path TEXT NOT NULL,
                last_seen_hash TEXT,
                last_published_hash TEXT,
                schema_name TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS backups (
                id TEXT PRIMARY KEY,
                target TEXT NOT NULL,
                path TEXT NOT NULL,
                source_path TEXT NOT NULL,
                fingerprint TEXT NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS backups_target_created_idx
                ON backups(target, created_at DESC);

            CREATE TABLE IF NOT EXISTS app_settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            "#,
        )?;
        let has_configuration_column = {
            let mut statement = transaction.prepare("PRAGMA table_info(models)")?;
            let columns = statement
                .query_map([], |row| row.get::<_, String>(1))?
                .collect::<Result<Vec<_>, _>>()?;
            columns.iter().any(|column| column == "configuration_json")
        };
        if !has_configuration_column {
            transaction.execute(
                "ALTER TABLE models ADD COLUMN configuration_json TEXT NOT NULL DEFAULT '{}'",
                [],
            )?;
        }
        transaction.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        transaction.commit()?;
        Ok(())
    }

    fn backup_before_migration(
        &self,
        connection: &Connection,
        current_version: i64,
    ) -> CoreResult<()> {
        let parent = self.database_path.parent().ok_or_else(|| {
            CoreError::Storage("The database path has no parent directory".to_string())
        })?;
        let backup_directory = parent.join("migration-backups");
        std::fs::create_dir_all(&backup_directory)?;
        let timestamp = Utc::now().format("%Y%m%dT%H%M%S%.3fZ");
        let backup_path = backup_directory.join(format!(
            "everybuddy-v{current_version}-{timestamp}-{}.db",
            Uuid::new_v4()
        ));
        connection.backup(DatabaseName::Main, &backup_path, None)?;
        Ok(())
    }

    pub fn list_gateways(&self) -> CoreResult<Vec<GatewayProfile>> {
        let connection = self.connection()?;
        query_gateways(&connection)
    }

    pub fn gateway(&self, id: &str) -> CoreResult<GatewayProfile> {
        self.find_gateway(id)?
            .ok_or_else(|| CoreError::Validation("Gateway profile not found".to_string()))
    }

    pub fn find_gateway(&self, id: &str) -> CoreResult<Option<GatewayProfile>> {
        let profile = self
            .connection()?
            .query_row(
                "SELECT id, name, api_root, token_ref, created_at, updated_at
                 FROM gateway_profiles WHERE id = ?1",
                [id],
                |row| {
                    Ok(GatewayProfile {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        api_root: row.get(2)?,
                        token_ref: row.get(3)?,
                        created_at: row.get(4)?,
                        updated_at: row.get(5)?,
                    })
                },
            )
            .optional()?;
        Ok(profile)
    }

    pub fn save_gateway(&self, profile: &GatewayProfile) -> CoreResult<()> {
        self.connection()?.execute(
            r#"INSERT INTO gateway_profiles
               (id, name, api_root, token_ref, created_at, updated_at)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6)
               ON CONFLICT(id) DO UPDATE SET
                 name = excluded.name,
                 api_root = excluded.api_root,
                 token_ref = excluded.token_ref,
                 updated_at = excluded.updated_at"#,
            params![
                profile.id,
                profile.name,
                profile.api_root,
                profile.token_ref,
                profile.created_at,
                profile.updated_at
            ],
        )?;
        Ok(())
    }

    pub fn delete_gateway(&self, id: &str) -> CoreResult<()> {
        self.connection()?
            .execute("DELETE FROM gateway_profiles WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn list_models(&self) -> CoreResult<Vec<ManagedModel>> {
        self.query_models(None)
    }

    pub fn models_for_gateway(&self, gateway_id: &str) -> CoreResult<Vec<ManagedModel>> {
        self.query_models(Some(gateway_id))
    }

    fn query_models(&self, gateway_id: Option<&str>) -> CoreResult<Vec<ManagedModel>> {
        let connection = self.connection()?;
        query_models(&connection, gateway_id)
    }

    pub fn import_missing_serialized<T, F>(&self, operation: F) -> CoreResult<T>
    where
        F: FnOnce(
            Vec<GatewayProfile>,
            Vec<ManagedModel>,
        ) -> CoreResult<(T, Vec<GatewayProfile>, Vec<ManagedModel>)>,
    {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let gateways = query_gateways(&transaction)?;
        let models = query_models(&transaction, None)?;
        let (result, new_gateways, new_models) = operation(gateways, models)?;
        for gateway in &new_gateways {
            transaction.execute(
                r#"INSERT OR IGNORE INTO gateway_profiles
                   (id, name, api_root, token_ref, created_at, updated_at)
                   VALUES (?1, ?2, ?3, ?4, ?5, ?6)"#,
                params![
                    gateway.id,
                    gateway.name,
                    gateway.api_root,
                    gateway.token_ref,
                    gateway.created_at,
                    gateway.updated_at
                ],
            )?;
        }
        for model in &new_models {
            insert_missing_model(&transaction, model)?;
        }
        transaction.commit()?;
        Ok(result)
    }

    pub fn model(&self, key: &str) -> CoreResult<ManagedModel> {
        self.list_models()?
            .into_iter()
            .find(|model| model.key == key)
            .ok_or_else(|| CoreError::Validation("Model not found".to_string()))
    }

    pub fn selected_models(
        &self,
        gateway_id: &str,
        ids: &[String],
    ) -> CoreResult<Vec<ManagedModel>> {
        let models = self.models_for_gateway(gateway_id)?;
        let selected: Vec<_> = models
            .into_iter()
            .filter(|model| ids.contains(&model.id))
            .collect();
        if selected.len() != ids.len() {
            return Err(CoreError::Validation(
                "One or more selected models no longer exist in this gateway".to_string(),
            ));
        }
        Ok(selected)
    }

    pub fn replace_gateway_models_if_unchanged(
        &self,
        expected_gateway: &GatewayProfile,
        expected: &[ManagedModel],
        models: &[ManagedModel],
    ) -> CoreResult<()> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let current_gateway = query_gateway(&transaction, &expected_gateway.id)?;
        if current_gateway.as_ref() != Some(expected_gateway) {
            return Err(CoreError::Conflict(
                "The API profile changed while its models were refreshing; reload and try again"
                    .to_string(),
            ));
        }
        let current = model_versions(&transaction, &expected_gateway.id)?;
        let mut expected_versions: Vec<_> = expected
            .iter()
            .map(|model| (model.key.clone(), model.updated_at.clone()))
            .collect();
        expected_versions.sort_unstable();
        if current != expected_versions {
            return Err(CoreError::Conflict(
                "Models changed while the gateway was refreshing; reload and try again".to_string(),
            ));
        }
        transaction.execute(
            "DELETE FROM models WHERE gateway_id = ?1",
            [&expected_gateway.id],
        )?;
        for model in models {
            insert_model(&transaction, model)?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn save_model(&self, model: &ManagedModel) -> CoreResult<()> {
        let connection = self.connection()?;
        insert_model(&connection, model)
    }

    pub fn target_last_published_hash(&self, target: TargetKind) -> CoreResult<Option<String>> {
        Ok(self
            .connection()?
            .query_row(
                "SELECT last_published_hash FROM target_states WHERE target = ?1",
                [target.as_str()],
                |row| row.get(0),
            )
            .optional()?
            .flatten())
    }

    pub fn save_target_state(
        &self,
        target: TargetKind,
        path: &str,
        seen_hash: Option<&str>,
        published_hash: Option<&str>,
        schema: &str,
    ) -> CoreResult<()> {
        self.save_target_states(&[TargetStateUpdate {
            target,
            path: path.to_string(),
            seen_hash: seen_hash.map(str::to_string),
            published_hash: published_hash.map(str::to_string),
            schema: schema.to_string(),
        }])
    }

    pub fn save_target_states(&self, updates: &[TargetStateUpdate]) -> CoreResult<()> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        for update in updates {
            transaction.execute(
                r#"INSERT INTO target_states
                   (target, path, last_seen_hash, last_published_hash, schema_name, updated_at)
                   VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                   ON CONFLICT(target) DO UPDATE SET
                     path = excluded.path,
                     last_seen_hash = excluded.last_seen_hash,
                     last_published_hash = COALESCE(excluded.last_published_hash, target_states.last_published_hash),
                     schema_name = excluded.schema_name,
                     updated_at = excluded.updated_at"#,
                params![
                    update.target.as_str(),
                    update.path,
                    update.seen_hash,
                    update.published_hash,
                    update.schema,
                    Utc::now().to_rfc3339()
                ],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn execute_test_sql(&self, sql: &str) -> CoreResult<()> {
        self.connection()?.execute_batch(sql)?;
        Ok(())
    }

    pub fn add_backup(&self, backup: &BackupRecord) -> CoreResult<()> {
        self.connection()?.execute(
            "INSERT INTO backups (id, target, path, source_path, fingerprint, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                backup.id,
                backup.target.as_str(),
                backup.path,
                backup.source_path,
                backup.fingerprint,
                backup.created_at
            ],
        )?;
        Ok(())
    }

    pub fn list_backups(&self, target: Option<TargetKind>) -> CoreResult<Vec<BackupRecord>> {
        let connection = self.connection()?;
        let sql = if target.is_some() {
            "SELECT id, target, path, source_path, fingerprint, created_at
             FROM backups WHERE target = ?1 ORDER BY created_at DESC"
        } else {
            "SELECT id, target, path, source_path, fingerprint, created_at
             FROM backups ORDER BY created_at DESC"
        };
        let mut statement = connection.prepare(sql)?;
        let mut rows = if let Some(kind) = target {
            statement.query([kind.as_str()])?
        } else {
            statement.query([])?
        };
        let mut backups = Vec::new();
        while let Some(row) = rows.next()? {
            let kind_value: String = row.get(1)?;
            backups.push(BackupRecord {
                id: row.get(0)?,
                target: TargetKind::from_str(&kind_value).map_err(CoreError::Storage)?,
                path: row.get(2)?,
                source_path: row.get(3)?,
                fingerprint: row.get(4)?,
                created_at: row.get(5)?,
            });
        }
        Ok(backups)
    }

    pub fn backup(&self, id: &str) -> CoreResult<BackupRecord> {
        self.list_backups(None)?
            .into_iter()
            .find(|backup| backup.id == id)
            .ok_or_else(|| CoreError::Validation("Backup not found".to_string()))
    }

    pub fn remove_backup_record(&self, id: &str) -> CoreResult<()> {
        self.connection()?
            .execute("DELETE FROM backups WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn settings(&self, default_paths: HashMap<TargetKind, String>) -> CoreResult<AppSettings> {
        let connection = self.connection()?;
        let mut statement = connection.prepare("SELECT key, value FROM app_settings")?;
        let values = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<HashMap<_, _>, _>>()?;

        let selected_targets = values
            .get("selected_targets")
            .and_then(|value| serde_json::from_str(value).ok())
            .unwrap_or_default();
        let target_paths = values
            .get("target_paths")
            .and_then(|value| serde_json::from_str(value).ok())
            .unwrap_or(default_paths);

        Ok(AppSettings {
            language: values
                .get("language")
                .cloned()
                .unwrap_or_else(|| "zh-CN".to_string()),
            theme: values
                .get("theme")
                .cloned()
                .unwrap_or_else(|| "system".to_string()),
            selected_targets,
            target_paths,
        })
    }

    pub fn save_settings(&self, settings: &AppSettings) -> CoreResult<()> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let values = [
            ("language", settings.language.clone()),
            ("theme", settings.theme.clone()),
            ("selected_targets", to_json(&settings.selected_targets)?),
            ("target_paths", to_json(&settings.target_paths)?),
        ];
        for (key, value) in values {
            transaction.execute(
                "INSERT INTO app_settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }
}

fn query_gateways(connection: &Connection) -> CoreResult<Vec<GatewayProfile>> {
    let mut statement = connection.prepare(
        "SELECT id, name, api_root, token_ref, created_at, updated_at
         FROM gateway_profiles ORDER BY name COLLATE NOCASE",
    )?;
    let rows = statement.query_map([], |row| {
        Ok(GatewayProfile {
            id: row.get(0)?,
            name: row.get(1)?,
            api_root: row.get(2)?,
            token_ref: row.get(3)?,
            created_at: row.get(4)?,
            updated_at: row.get(5)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn query_gateway(connection: &Connection, id: &str) -> CoreResult<Option<GatewayProfile>> {
    connection
        .query_row(
            "SELECT id, name, api_root, token_ref, created_at, updated_at
             FROM gateway_profiles WHERE id = ?1",
            [id],
            |row| {
                Ok(GatewayProfile {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    api_root: row.get(2)?,
                    token_ref: row.get(3)?,
                    created_at: row.get(4)?,
                    updated_at: row.get(5)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
}

fn query_models(
    connection: &Connection,
    gateway_id: Option<&str>,
) -> CoreResult<Vec<ManagedModel>> {
    let sql = if gateway_id.is_some() {
        "SELECT model_key, gateway_id, upstream_id, name, vendor, capabilities_json,
                configuration_json, evidence_json, metadata_json, updated_at
         FROM models WHERE gateway_id = ?1 ORDER BY name COLLATE NOCASE"
    } else {
        "SELECT model_key, gateway_id, upstream_id, name, vendor, capabilities_json,
                configuration_json, evidence_json, metadata_json, updated_at
         FROM models ORDER BY name COLLATE NOCASE"
    };
    let mut statement = connection.prepare(sql)?;
    let mut rows = if let Some(id) = gateway_id {
        statement.query([id])?
    } else {
        statement.query([])?
    };
    let mut models = Vec::new();
    while let Some(row) = rows.next()? {
        let capabilities = parse_json(&row.get::<_, String>(5)?)?;
        let raw_configuration = row.get::<_, String>(6)?;
        let metadata = parse_json(&row.get::<_, String>(8)?)?;
        let configuration = if raw_configuration.trim() == "{}" {
            crate::capability::configuration_from_metadata(&metadata, &capabilities)
        } else {
            parse_json(&raw_configuration)?
        };
        models.push(ManagedModel {
            key: row.get(0)?,
            gateway_id: row.get(1)?,
            id: row.get(2)?,
            name: row.get(3)?,
            vendor: row.get(4)?,
            capabilities,
            configuration,
            evidence: parse_json(&row.get::<_, String>(7)?)?,
            metadata,
            updated_at: row.get(9)?,
        });
    }
    Ok(models)
}

fn model_versions(connection: &Connection, gateway_id: &str) -> CoreResult<Vec<(String, String)>> {
    let mut statement = connection.prepare(
        "SELECT model_key, updated_at FROM models WHERE gateway_id = ?1 ORDER BY model_key",
    )?;
    let rows = statement.query_map([gateway_id], |row| Ok((row.get(0)?, row.get(1)?)))?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn insert_model(connection: &Connection, model: &ManagedModel) -> CoreResult<()> {
    connection.execute(
        r#"INSERT INTO models
           (model_key, gateway_id, upstream_id, name, vendor, capabilities_json,
            configuration_json, evidence_json, metadata_json, updated_at)
           VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
           ON CONFLICT(model_key) DO UPDATE SET
             name = excluded.name,
             vendor = excluded.vendor,
             capabilities_json = excluded.capabilities_json,
             configuration_json = excluded.configuration_json,
             evidence_json = excluded.evidence_json,
             metadata_json = excluded.metadata_json,
             updated_at = excluded.updated_at"#,
        params![
            model.key,
            model.gateway_id,
            model.id,
            model.name,
            model.vendor,
            to_json(&model.capabilities)?,
            to_json(&model.configuration)?,
            to_json(&model.evidence)?,
            to_json(&model.metadata)?,
            model.updated_at
        ],
    )?;
    Ok(())
}

fn insert_missing_model(connection: &Connection, model: &ManagedModel) -> CoreResult<()> {
    connection.execute(
        r#"INSERT OR IGNORE INTO models
           (model_key, gateway_id, upstream_id, name, vendor, capabilities_json,
            configuration_json, evidence_json, metadata_json, updated_at)
           VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)"#,
        params![
            model.key,
            model.gateway_id,
            model.id,
            model.name,
            model.vendor,
            to_json(&model.capabilities)?,
            to_json(&model.configuration)?,
            to_json(&model.evidence)?,
            to_json(&model.metadata)?,
            model.updated_at
        ],
    )?;
    Ok(())
}

fn to_json<T: serde::Serialize>(value: &T) -> CoreResult<String> {
    serde_json::to_string(value).map_err(|error| CoreError::Storage(error.to_string()))
}

fn parse_json<T: serde::de::DeserializeOwned>(value: &str) -> CoreResult<T> {
    serde_json::from_str(value).map_err(|error| CoreError::Storage(error.to_string()))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::json;
    use tempfile::tempdir;

    use super::*;
    use crate::models::CapabilitySet;

    #[test]
    fn initializes_new_database_at_current_schema_version() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("everybuddy.db");

        let store = Store::open(&path).unwrap();
        let version: i64 = store
            .connection()
            .unwrap()
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();

        assert_eq!(version, SCHEMA_VERSION);
        assert!(!directory.path().join("migration-backups").exists());
    }

    #[test]
    fn multiple_gateways_keep_same_upstream_model_id_isolated() {
        let directory = tempdir().unwrap();
        let store = Store::open(&directory.path().join("everybuddy.db")).unwrap();
        let profile = GatewayProfile {
            id: "gateway-1".to_string(),
            name: "Local gateway".to_string(),
            api_root: "http://127.0.0.1:3000/v1".to_string(),
            token_ref: "gateway-1".to_string(),
            created_at: "2026-08-20T00:00:00Z".to_string(),
            updated_at: "2026-08-20T00:00:00Z".to_string(),
        };
        store.save_gateway(&profile).unwrap();
        assert_eq!(store.gateway("gateway-1").unwrap(), profile);
        store
            .save_gateway(&GatewayProfile {
                id: "gateway-2".to_string(),
                name: "Second gateway".to_string(),
                api_root: "http://127.0.0.1:4000/v1".to_string(),
                token_ref: "gateway-2".to_string(),
                created_at: "2026-08-20T00:00:00Z".to_string(),
                updated_at: "2026-08-20T00:00:00Z".to_string(),
            })
            .unwrap();

        for gateway_id in ["gateway-1", "gateway-2"] {
            store
                .save_model(&ManagedModel {
                    key: format!("{gateway_id}::shared-model"),
                    gateway_id: gateway_id.to_string(),
                    id: "shared-model".to_string(),
                    name: "Shared model".to_string(),
                    vendor: "custom".to_string(),
                    capabilities: CapabilitySet::default(),
                    configuration: Default::default(),
                    evidence: Vec::new(),
                    metadata: json!({"id": "shared-model"}),
                    updated_at: "2026-08-20T00:00:00Z".to_string(),
                })
                .unwrap();
        }

        assert_eq!(store.list_gateways().unwrap().len(), 2);
        assert_eq!(store.list_models().unwrap().len(), 2);
        assert_eq!(store.models_for_gateway("gateway-1").unwrap().len(), 1);
        assert_eq!(store.models_for_gateway("gateway-2").unwrap().len(), 1);
    }

    #[test]
    fn refresh_rejects_a_stale_model_snapshot() {
        let directory = tempdir().unwrap();
        let store = Store::open(&directory.path().join("everybuddy.db")).unwrap();
        let profile = GatewayProfile {
            id: "gateway".to_string(),
            name: "Gateway".to_string(),
            api_root: "https://api.example.com/v1".to_string(),
            token_ref: "gateway".to_string(),
            created_at: "2026-08-20T00:00:00Z".to_string(),
            updated_at: "2026-08-20T00:00:00Z".to_string(),
        };
        store.save_gateway(&profile).unwrap();
        let mut original = ManagedModel {
            key: "gateway::model".to_string(),
            gateway_id: "gateway".to_string(),
            id: "model".to_string(),
            name: "Original".to_string(),
            vendor: "custom".to_string(),
            capabilities: CapabilitySet::default(),
            configuration: Default::default(),
            evidence: Vec::new(),
            metadata: json!({"id": "model"}),
            updated_at: "2026-08-20T00:00:00Z".to_string(),
        };
        store.save_model(&original).unwrap();
        let snapshot = store.models_for_gateway("gateway").unwrap();
        original.name = "Edited while refreshing".to_string();
        original.updated_at = "2026-08-21T00:00:00Z".to_string();
        store.save_model(&original).unwrap();

        let error = store
            .replace_gateway_models_if_unchanged(&profile, &snapshot, &snapshot)
            .unwrap_err();

        assert!(matches!(error, CoreError::Conflict(_)));
        assert_eq!(
            store.model("gateway::model").unwrap().name,
            "Edited while refreshing"
        );
    }

    #[test]
    fn refresh_rejects_a_stale_gateway_snapshot() {
        let directory = tempdir().unwrap();
        let store = Store::open(&directory.path().join("everybuddy.db")).unwrap();
        let profile = GatewayProfile {
            id: "gateway".to_string(),
            name: "Gateway".to_string(),
            api_root: "https://api.example.com/v1".to_string(),
            token_ref: "gateway".to_string(),
            created_at: "2026-08-20T00:00:00Z".to_string(),
            updated_at: "2026-08-20T00:00:00Z".to_string(),
        };
        store.save_gateway(&profile).unwrap();
        let mut edited = profile.clone();
        edited.api_root = "https://api.changed.example/v1".to_string();
        edited.updated_at = "2026-08-21T00:00:00Z".to_string();
        store.save_gateway(&edited).unwrap();

        let error = store
            .replace_gateway_models_if_unchanged(&profile, &[], &[])
            .unwrap_err();

        assert!(matches!(error, CoreError::Conflict(_)));
    }

    #[test]
    fn migrates_existing_model_table_without_configuration_column() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("legacy.db");
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                r#"
                CREATE TABLE models (
                    model_key TEXT PRIMARY KEY,
                    gateway_id TEXT NOT NULL,
                    upstream_id TEXT NOT NULL,
                    name TEXT NOT NULL,
                    vendor TEXT NOT NULL,
                    capabilities_json TEXT NOT NULL,
                    evidence_json TEXT NOT NULL,
                    metadata_json TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );
                "#,
            )
            .unwrap();
        drop(connection);

        let store = Store::open(&path).unwrap();
        let connection = store.connection().unwrap();
        let mut statement = connection.prepare("PRAGMA table_info(models)").unwrap();
        let columns = statement
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert!(columns.iter().any(|column| column == "configuration_json"));
        drop(statement);
        drop(connection);

        let backup_directory = directory.path().join("migration-backups");
        let backups = fs::read_dir(&backup_directory)
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(backups.len(), 1);

        let backup = Connection::open(backups[0].path()).unwrap();
        let backup_columns = backup
            .prepare("PRAGMA table_info(models)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(!backup_columns
            .iter()
            .any(|column| column == "configuration_json"));
    }

    #[test]
    fn rejects_database_from_a_newer_app_version() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("future.db");
        let connection = Connection::open(&path).unwrap();
        connection
            .pragma_update(None, "user_version", SCHEMA_VERSION + 1)
            .unwrap();
        drop(connection);

        let error = Store::open(&path).err().expect("newer schema must fail");

        assert!(error.to_string().contains("newer than this app supports"));
    }
}
