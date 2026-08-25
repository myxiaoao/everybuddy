use std::path::Path;

use chrono::Utc;
use rusqlite::{Connection, DatabaseName};
use uuid::Uuid;

use crate::error::{CoreError, CoreResult};

pub(super) const SCHEMA_VERSION: i64 = 2;

pub(super) fn migrate(
    connection: &mut Connection,
    database_path: &Path,
    current_version: i64,
    database_existed: bool,
) -> CoreResult<()> {
    if current_version == SCHEMA_VERSION {
        return Ok(());
    }

    let has_user_tables: bool = connection.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM sqlite_master
            WHERE type = 'table' AND name NOT LIKE 'sqlite_%'
        )",
        [],
        |row| row.get(0),
    )?;
    if database_existed && has_user_tables {
        backup_before_migration(connection, database_path, current_version)?;
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

        CREATE TABLE IF NOT EXISTS deleted_gateway_sources (
            source_hash TEXT PRIMARY KEY,
            deleted_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS gateway_source_identities (
            gateway_id TEXT NOT NULL,
            source_hash TEXT NOT NULL,
            PRIMARY KEY(gateway_id, source_hash),
            FOREIGN KEY(gateway_id) REFERENCES gateway_profiles(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS stale_gateway_models (
            gateway_id TEXT PRIMARY KEY,
            FOREIGN KEY(gateway_id) REFERENCES gateway_profiles(id) ON DELETE CASCADE
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
    connection: &Connection,
    database_path: &Path,
    current_version: i64,
) -> CoreResult<()> {
    let parent = database_path.parent().ok_or_else(|| {
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
