mod config;
mod discovery;
mod server;
mod transfer;

use config::{ReceiverConfig, TransferRecord};
use discovery::{DiscoveredService, ServiceDiscovery};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use tauri::State;

const PORT: u16 = 45678;

pub struct AppState {
    pub config: Arc<Mutex<ReceiverConfig>>,
    pub is_running: Arc<Mutex<bool>>,
    pub history: Arc<Mutex<Vec<TransferRecord>>>,
    pub discovery: Arc<Mutex<Option<ServiceDiscovery>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferProgress {
    pub photographer: String,
    pub file_name: String,
    pub file_index: usize,
    pub total_files: usize,
    pub bytes_received: u64,
    pub total_bytes: u64,
    pub speed_mbps: f64,
}

// Commands

#[tauri::command]
async fn get_config(state: State<'_, AppState>) -> Result<ReceiverConfig, String> {
    let config = state.config.lock().map_err(|e| e.to_string())?;
    Ok(config.clone())
}

#[tauri::command]
async fn save_config(state: State<'_, AppState>, config: ReceiverConfig) -> Result<(), String> {
    let mut current = state.config.lock().map_err(|e| e.to_string())?;
    *current = config.clone();
    current.save()?;
    Ok(())
}

#[tauri::command]
async fn start_server(
    state: State<'_, AppState>,
    window: tauri::Window,
) -> Result<(), String> {
    let config = {
        let c = state.config.lock().map_err(|e| e.to_string())?;
        c.clone()
    };

    {
        let mut running = state.is_running.lock().map_err(|e| e.to_string())?;
        if *running {
            return Err("Server deja pornit".to_string());
        }
        *running = true;
    }

    let is_running = Arc::clone(&state.is_running);
    let config_state = Arc::clone(&state.config);
    let history = Arc::clone(&state.history);

    std::thread::spawn(move || {
        if let Err(e) = server::run_server(PORT, config, config_state, history, is_running, window) {
            eprintln!("Server error: {}", e);
        }
    });

    Ok(())
}

#[tauri::command]
async fn stop_server(state: State<'_, AppState>) -> Result<(), String> {
    let mut running = state.is_running.lock().map_err(|e| e.to_string())?;
    *running = false;
    Ok(())
}

#[tauri::command]
async fn is_server_running(state: State<'_, AppState>) -> Result<bool, String> {
    let running = state.is_running.lock().map_err(|e| e.to_string())?;
    Ok(*running)
}

#[tauri::command]
async fn get_history(state: State<'_, AppState>) -> Result<Vec<TransferRecord>, String> {
    let history = state.history.lock().map_err(|e| e.to_string())?;
    Ok(history.clone())
}

#[tauri::command]
async fn clear_history(state: State<'_, AppState>, day: Option<String>) -> Result<(), String> {
    let mut history = state.history.lock().map_err(|e| e.to_string())?;

    if let Some(day_to_clear) = day {
        // Șterge doar intrările din ziua specificată
        history.retain(|record| {
            record.day.as_ref() != Some(&day_to_clear)
        });
    } else {
        // Șterge tot istoricul
        history.clear();
    }

    config::save_history(&history)?;
    Ok(())
}

#[tauri::command]
async fn get_local_ip() -> Result<String, String> {
    use std::net::UdpSocket;

    let socket = UdpSocket::bind("0.0.0.0:0").map_err(|e| e.to_string())?;
    socket.connect("8.8.8.8:80").map_err(|e| e.to_string())?;
    let addr = socket.local_addr().map_err(|e| e.to_string())?;
    Ok(addr.ip().to_string())
}

#[tauri::command]
async fn start_discovery(state: State<'_, AppState>) -> Result<(), String> {
    let config = {
        let c = state.config.lock().map_err(|e| e.to_string())?;
        c.clone()
    };

    let mut discovery_guard = state.discovery.lock().map_err(|e| e.to_string())?;

    // Dacă există deja, nu facem nimic
    if discovery_guard.is_some() {
        return Ok(());
    }

    let discovery = ServiceDiscovery::new(config.name.clone());
    *discovery_guard = Some(discovery);

    Ok(())
}

#[tauri::command]
async fn get_editors(state: State<'_, AppState>) -> Result<Vec<DiscoveredService>, String> {
    let discovery_guard = state.discovery.lock().map_err(|e| e.to_string())?;

    if let Some(ref discovery) = *discovery_guard {
        Ok(discovery.get_editors())
    } else {
        Ok(vec![])
    }
}

#[tauri::command]
async fn send_to_editor(
    state: State<'_, AppState>,
    target_host: String,
    target_port: u16,
    target_name: String,
    file_paths: Vec<String>,
    window: tauri::Window,
) -> Result<(), String> {
    let config = {
        let c = state.config.lock().map_err(|e| e.to_string())?;
        c.clone()
    };

    let service = DiscoveredService {
        name: target_name,
        role: "editor".to_string(),
        host: target_host,
        port: target_port,
    };

    let files = transfer::prepare_files(&file_paths);

    if files.is_empty() {
        return Err("Nu s-au găsit fișiere valide".to_string());
    }

    transfer::send_files_to_editor(&service, &config.name, &config.role, &files, window).await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let config = ReceiverConfig::load().unwrap_or_default();
    let history = config::load_history().unwrap_or_default();

    let app_state = AppState {
        config: Arc::new(Mutex::new(config)),
        is_running: Arc::new(Mutex::new(false)),
        history: Arc::new(Mutex::new(history)),
        discovery: Arc::new(Mutex::new(None)),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_config,
            start_server,
            stop_server,
            is_server_running,
            get_history,
            clear_history,
            get_local_ip,
            start_discovery,
            get_editors,
            send_to_editor,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
