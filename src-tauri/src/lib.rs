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

pub struct AppState {
    store: Store,
    secrets: Arc<dyn SecretStore>,
    gateway_client: GatewayClient,
    backup_root: PathBuf,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
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

    #[test]
    fn base_config_initializes_the_updater_plugin() {
        let config: Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let updater = &config["plugins"]["updater"];

        assert!(updater["endpoints"].is_array());
        assert!(updater["pubkey"].is_string());
    }
}
