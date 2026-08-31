use rusqlite::{params, Connection, OptionalExtension};

use crate::{
    error::{CoreError, CoreResult},
    models::{GatewayProfile, ManagedModel},
};

pub(super) fn query_gateways(connection: &Connection) -> CoreResult<Vec<GatewayProfile>> {
    let mut statement = connection.prepare(
        "SELECT id, name, api_root, created_at, updated_at
         FROM gateway_profiles ORDER BY name COLLATE NOCASE",
    )?;
    let rows = statement.query_map([], |row| {
        Ok(GatewayProfile {
            id: row.get(0)?,
            name: row.get(1)?,
            api_root: row.get(2)?,
            created_at: row.get(3)?,
            updated_at: row.get(4)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub(super) fn query_gateway(
    connection: &Connection,
    id: &str,
) -> CoreResult<Option<GatewayProfile>> {
    connection
        .query_row(
            "SELECT id, name, api_root, created_at, updated_at
             FROM gateway_profiles WHERE id = ?1",
            [id],
            |row| {
                Ok(GatewayProfile {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    api_root: row.get(2)?,
                    created_at: row.get(3)?,
                    updated_at: row.get(4)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
}

pub(super) fn query_models(
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
        let model_id: String = row.get(2)?;
        let configuration = if raw_configuration.trim() == "{}" {
            crate::capability::configuration_from_metadata(&model_id, &metadata, &capabilities)
        } else {
            parse_json(&raw_configuration)?
        };
        models.push(ManagedModel {
            key: row.get(0)?,
            gateway_id: row.get(1)?,
            id: model_id,
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

pub(super) fn model_versions(
    connection: &Connection,
    gateway_id: &str,
) -> CoreResult<Vec<(String, String)>> {
    let mut statement = connection.prepare(
        "SELECT model_key, updated_at FROM models WHERE gateway_id = ?1 ORDER BY model_key",
    )?;
    let rows = statement.query_map([gateway_id], |row| Ok((row.get(0)?, row.get(1)?)))?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub(super) fn insert_model(connection: &Connection, model: &ManagedModel) -> CoreResult<()> {
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

pub(super) fn insert_missing_model(
    connection: &Connection,
    model: &ManagedModel,
) -> CoreResult<()> {
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

pub(super) fn to_json<T: serde::Serialize>(value: &T) -> CoreResult<String> {
    serde_json::to_string(value).map_err(|error| CoreError::Storage(error.to_string()))
}

fn parse_json<T: serde::de::DeserializeOwned>(value: &str) -> CoreResult<T> {
    serde_json::from_str(value).map_err(|error| CoreError::Storage(error.to_string()))
}
