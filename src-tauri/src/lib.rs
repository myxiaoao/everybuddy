mod capability;
mod commands;
mod error;
mod gateway;
mod gateway_service;
mod models;
mod publish;
mod secrets;
mod store;
mod target;
mod target_import;

use std::{path::PathBuf, sync::Arc};

use gateway::GatewayClient;
use secrets::{SecretStore, SystemSecretStore};
use store::Store;
use tauri::Manager;
use tauri_plugin_log::{RotationStrategy, Target, TargetKind};

pub struct AppState {
    store: Store,
    secrets: Arc<dyn SecretStore>,
    gateway_client: GatewayClient,
    backup_root: PathBuf,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Warn)
                .targets([Target::new(TargetKind::LogDir {
                    file_name: Some("everybuddy".into()),
                })])
                .rotation_strategy(RotationStrategy::KeepSome(3))
                .max_file_size(2 * 1024 * 1024)
                .build(),
        )
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let store = Store::open(&data_dir.join("everybuddy.db"))
                .map_err(|error| std::io::Error::other(error.to_string()))?;
            let gateway_client =
                GatewayClient::new().map_err(|error| std::io::Error::other(error.to_string()))?;
            app.manage(AppState {
                store,
                secrets: Arc::new(SystemSecretStore),
                gateway_client,
                backup_root: data_dir.join("backups"),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::bootstrap,
            commands::save_gateway,
            commands::get_gateway_token,
            commands::delete_gateway,
            commands::discover_models,
            commands::add_manual_model,
            commands::probe_model,
            commands::update_model,
            commands::get_target_statuses,
            commands::get_target_model_states,
            commands::prepare_publish,
            commands::execute_publish,
            commands::list_backups,
            commands::restore_backup,
            commands::save_settings,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run EveryBuddy");
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use crate::models::{AppSettings, BootstrapData, TargetImportReport, TargetKind};

    #[test]
    fn base_config_initializes_the_updater_plugin() {
        let config: Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let updater = &config["plugins"]["updater"];

        assert!(updater["endpoints"].is_array());
        assert!(updater["pubkey"].is_string());
    }

    #[test]
    fn bootstrap_serialization_matches_the_frontend_contract() {
        let expected: Value =
            serde_json::from_str(include_str!("../tests/fixtures/bootstrap-contract.json"))
                .unwrap();
        let actual = serde_json::to_value(BootstrapData {
            gateways: Vec::new(),
            models: Vec::new(),
            targets: Vec::new(),
            target_model_states: Vec::new(),
            import_report: TargetImportReport::default(),
            settings: AppSettings {
                language: "zh-CN".to_string(),
                theme: "system".to_string(),
                selected_targets: Vec::new(),
                target_paths: [
                    (
                        TargetKind::Workbuddy,
                        "/home/test/.workbuddy/models.json".to_string(),
                    ),
                    (
                        TargetKind::Codebuddy,
                        "/home/test/.codebuddy/models.json".to_string(),
                    ),
                ]
                .into_iter()
                .collect(),
            },
        })
        .unwrap();

        assert_eq!(actual, expected);
    }
}
