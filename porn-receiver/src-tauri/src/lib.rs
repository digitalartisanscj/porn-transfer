mod config;
mod discovery;
mod server;
mod transfer;

use config::{ReceiverConfig, TransferRecord, SentRecord, load_sent_history, add_sent_record};
use discovery::{DiscoveredService, ServiceDiscovery};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::State;

const DEFAULT_PORT: u16 = 45678;

pub struct AppState {
    pub config: Arc<Mutex<ReceiverConfig>>,
    pub is_running: Arc<Mutex<bool>>,
    pub history: Arc<Mutex<Vec<TransferRecord>>>,
    pub discovery: Arc<Mutex<Option<ServiceDiscovery>>>,
    pub is_transfer_cancelled: Arc<AtomicBool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferProgress {
    pub transfer_id: String, // ID unic pentru fiecare transfer (pentru transferuri simultane)
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

    // Păstrează counter-urile existente (frontend nu le trimite)
    let day_counters = current.day_counters.clone();
    let folder_counter = current.folder_counter;

    *current = config;

    // Restaurează counter-urile
    current.day_counters = day_counters;
    current.folder_counter = folder_counter;

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
    let is_cancelled = Arc::clone(&state.is_transfer_cancelled);

    // Reset flag la pornirea serverului
    is_cancelled.store(false, Ordering::Relaxed);

    // Folosește portul din config sau default
    let port = if config.port > 0 { config.port } else { DEFAULT_PORT };

    std::thread::spawn(move || {
        if let Err(e) = server::run_server(port, config, config_state, history, is_running, is_cancelled, window) {
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

    if let Some(ref day_to_clear) = day {
        // Șterge doar intrările din ziua specificată
        history.retain(|record| {
            record.day.as_ref() != Some(day_to_clear)
        });

        // Resetează counter-ul pentru ziua respectivă
        let mut config = state.config.lock().map_err(|e| e.to_string())?;
        config.day_counters.remove(day_to_clear);
        config.save()?;
    } else {
        // Șterge tot istoricul
        history.clear();

        // Resetează toate counter-urile
        let mut config = state.config.lock().map_err(|e| e.to_string())?;
        config.day_counters.clear();
        config.folder_counter = 1;
        config.save()?;
    }

    config::save_history(&history)?;
    Ok(())
}

#[tauri::command]
async fn sync_history_from_disk(state: State<'_, AppState>) -> Result<(), String> {
    use config::TransferStatus;
    use std::path::Path;

    let mut history = state.history.lock().map_err(|e| e.to_string())?;

    // Actualizează fiecare înregistrare pe baza fișierelor reale din folder
    for record in history.iter_mut() {
        let folder_path = Path::new(&record.folder);

        if folder_path.exists() {
            // Numără fișierele reale
            if let Ok(entries) = std::fs::read_dir(folder_path) {
                let mut count = 0usize;
                let mut size = 0u64;
                for entry in entries.filter_map(|e| e.ok()) {
                    if entry.path().is_file() {
                        count += 1;
                        size += entry.metadata().map(|m| m.len()).unwrap_or(0);
                    }
                }
                record.file_count = count;
                record.total_size = size;

                // Dacă folderul există și are fișiere, marchează ca Complete
                if count > 0 && record.status != TransferStatus::Complete {
                    record.status = TransferStatus::Complete;
                }
            }
        } else {
            // Folderul nu mai există - marchează cu 0
            record.file_count = 0;
            record.total_size = 0;
        }
    }

    // Elimină înregistrările cu 0 fișiere (foldere șterse)
    history.retain(|r| r.file_count > 0);

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
    folder_name: Option<String>,  // Numele original al folderului (pentru receiver→receiver)
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

    let total_size: u64 = files.iter().map(|f| f.size).sum();
    let file_count = files.len();
    let target = service.name.clone();
    let folder = folder_name.clone();

    // Trimite fișierele
    transfer::send_files_to_editor(&service, &config.name, &config.role, &files, folder_name, window).await?;

    // Salvează în istoricul de trimiteri
    let sent_record = SentRecord {
        timestamp: chrono::Utc::now(),
        target_name: target,
        file_count,
        total_size,
        folder_name: folder,
    };
    let _ = add_sent_record(sent_record);

    Ok(())
}

#[tauri::command]
async fn get_temp_folders(state: State<'_, AppState>) -> Result<Vec<server::TempFolderInfo>, String> {
    let config = state.config.lock().map_err(|e| e.to_string())?;
    let base_path = std::path::PathBuf::from(&config.base_path);
    Ok(server::find_all_temp_folders(&base_path))
}

#[tauri::command]
async fn cancel_current_transfer(state: State<'_, AppState>) -> Result<(), String> {
    state.is_transfer_cancelled.store(true, Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
async fn get_sent_history() -> Result<Vec<SentRecord>, String> {
    load_sent_history()
}

#[tauri::command]
async fn get_day_counter(state: State<'_, AppState>, day: String) -> Result<u32, String> {
    let config = state.config.lock().map_err(|e| e.to_string())?;
    Ok(*config.day_counters.get(&day).unwrap_or(&1))
}

#[tauri::command]
async fn set_day_counter(state: State<'_, AppState>, day: String, value: u32) -> Result<(), String> {
    let mut config = state.config.lock().map_err(|e| e.to_string())?;
    if value < 1 {
        return Err("Counter-ul trebuie să fie minim 1".to_string());
    }
    config.day_counters.insert(day, value);
    config.save()?;
    Ok(())
}

#[tauri::command]
async fn show_notification(title: String, body: String, sound: Option<String>) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        // Redă sunetul
        if let Some(ref sound_name) = sound {
            let _ = std::process::Command::new("afplay")
                .args([&format!("/System/Library/Sounds/{}.aiff", sound_name)])
                .spawn();
        }

        // Folosește display alert care rămâne pe ecran până e dat dismiss
        let script = format!(
            r#"display alert "{}" message "{}""#,
            title.replace("\"", "\\\""),
            body.replace("\"", "\\\"")
        );
        std::process::Command::new("osascript")
            .args(["-e", &script])
            .spawn()  // spawn pentru a nu bloca
            .map_err(|e| e.to_string())?;
    }

    #[cfg(target_os = "windows")]
    {
        // Pe Windows folosește PowerShell pentru a afișa un MessageBox
        // și redă sunetul system
        if sound.is_some() {
            let _ = std::process::Command::new("powershell")
                .args(["-Command", "[System.Media.SystemSounds]::Asterisk.Play()"])
                .spawn();
        }

        let script = format!(
            r#"Add-Type -AssemblyName System.Windows.Forms; [System.Windows.Forms.MessageBox]::Show('{}', '{}', 'OK', 'Information')"#,
            body.replace("'", "''"),
            title.replace("'", "''")
        );
        std::process::Command::new("powershell")
            .args(["-Command", &script])
            .spawn()
            .map_err(|e| e.to_string())?;
    }

    Ok(())
}

#[tauri::command]
async fn play_sound(sound_name: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        // Folosește system sounds pentru macOS
        let sound = match sound_name.as_str() {
            "receive-photographer" => "Glass",
            "receive-editor" => "Ping",
            "send-complete" => "Hero",
            "error" => "Basso",
            _ => "Pop",
        };
        std::process::Command::new("afplay")
            .args([&format!("/System/Library/Sounds/{}.aiff", sound)])
            .spawn()
            .map_err(|e| e.to_string())?;
    }

    #[cfg(target_os = "windows")]
    {
        // Pe Windows folosește PowerShell pentru system sounds
        let sound_type = match sound_name.as_str() {
            "receive-photographer" => "Asterisk",
            "receive-editor" => "Exclamation",
            "send-complete" => "Hand",
            "error" => "Beep",
            _ => "Asterisk",
        };
        let script = format!("[System.Media.SystemSounds]::{}::Play()", sound_type);
        std::process::Command::new("powershell")
            .args(["-Command", &script])
            .spawn()
            .map_err(|e| e.to_string())?;
    }

    Ok(())
}

#[tauri::command]
async fn delete_temp_folder(path: String) -> Result<(), String> {
    let folder_path = std::path::Path::new(&path);
    if folder_path.exists() && folder_path.is_dir() {
        // Verifică că este un folder temporar (începe cu .tmp_)
        if let Some(name) = folder_path.file_name() {
            if name.to_string_lossy().starts_with(".tmp_") {
                std::fs::remove_dir_all(folder_path)
                    .map_err(|e| format!("Eroare ștergere folder: {}", e))?;
                return Ok(());
            }
        }
    }
    Err("Nu este un folder temporar valid".to_string())
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
        is_transfer_cancelled: Arc::new(AtomicBool::new(false)),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_notification::init())
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_config,
            start_server,
            stop_server,
            is_server_running,
            get_history,
            clear_history,
            sync_history_from_disk,
            get_local_ip,
            start_discovery,
            get_editors,
            send_to_editor,
            get_temp_folders,
            delete_temp_folder,
            cancel_current_transfer,
            get_day_counter,
            set_day_counter,
            get_sent_history,
            show_notification,
            play_sound,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
