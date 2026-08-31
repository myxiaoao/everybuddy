mod capability;
mod commands;
mod conditional_write;
mod error;
mod file_permissions;
mod gateway;
mod gateway_service;
mod market_catalog;
mod model_lifecycle;
mod models;
mod publish;
mod secrets;
mod store;
mod target;
mod target_codec;
mod target_import;

use std::{path::PathBuf, sync::Mutex};

use gateway::GatewayClient;
use store::Store;
use tauri::Manager;
use tauri_plugin_log::{RotationStrategy, Target, TargetKind};

pub struct AppState {
    store: Store,
    gateway_client: GatewayClient,
    backup_root: PathBuf,
    app_mutation: Mutex<()>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
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
                GatewayClient::new(Some(data_dir.join("openrouter-models-cache.json")))
                    .map_err(|error| std::io::Error::other(error.to_string()))?;
            app.manage(AppState {
                store,
                gateway_client,
                backup_root: data_dir.join("backups"),
                app_mutation: Mutex::new(()),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::bootstrap,
            commands::get_gateway_token,
            commands::save_gateway,
            commands::delete_gateway,
            commands::discover_models,
            commands::add_manual_model,
            commands::probe_model,
            commands::get_openrouter_model_match,
            commands::apply_openrouter_model,
            commands::update_model,
            commands::get_target_snapshot,
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
                target_selection_initialized: false,
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
