mod discovery;
mod transfer;

use discovery::ServiceDiscovery;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::State;

// State pentru serviciile descoperite
pub struct AppState {
    pub discovery: Arc<Mutex<ServiceDiscovery>>,
    pub discovered_services: Arc<Mutex<HashMap<String, DiscoveredService>>>,
    pub is_transfer_cancelled: Arc<AtomicBool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredService {
    pub name: String,
    pub role: String, // "tagger" sau "editor"
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileInfo {
    pub path: String,
    pub name: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferProgress {
    pub file_name: String,
    pub file_index: usize,
    pub total_files: usize,
    pub bytes_sent: u64,
    pub total_bytes: u64,
    pub speed_mbps: f64,
}

// Comenzi Tauri

#[tauri::command]
async fn get_services(state: State<'_, AppState>) -> Result<Vec<DiscoveredService>, String> {
    let services = state.discovered_services.lock().map_err(|e| e.to_string())?;
    Ok(services.values().cloned().collect())
}

#[tauri::command]
async fn send_files(
    state: State<'_, AppState>,
    target_role: String,
    photographer_name: String,
    file_paths: Vec<String>,
    window: tauri::Window,
) -> Result<(), String> {
    // Găsește serviciul țintă
    let service = {
        let services = state.discovered_services.lock().map_err(|e| e.to_string())?;
        services
            .values()
            .find(|s| s.role == target_role)
            .cloned()
            .ok_or_else(|| format!("Nu s-a găsit serviciul: {}", target_role))?
    };

    // Pregătește fișierele
    let files: Vec<FileInfo> = file_paths
        .iter()
        .filter_map(|p| {
            let path = PathBuf::from(p);
            let metadata = std::fs::metadata(&path).ok()?;
            Some(FileInfo {
                path: p.clone(),
                name: path.file_name()?.to_string_lossy().to_string(),
                size: metadata.len(),
            })
        })
        .collect();

    if files.is_empty() {
        return Err("Nu s-au găsit fișiere valide".to_string());
    }

    // Reset flag și trimite fișierele
    state.is_transfer_cancelled.store(false, Ordering::Relaxed);
    let is_cancelled = Arc::clone(&state.is_transfer_cancelled);
    transfer::send_files_to_receiver(&service, &photographer_name, &files, is_cancelled, window).await
}

#[tauri::command]
async fn send_files_to_host(
    state: State<'_, AppState>,
    target_host: String,
    target_port: u16,
    photographer_name: String,
    file_paths: Vec<String>,
    window: tauri::Window,
) -> Result<(), String> {
    // Create a temporary service for the specified host
    let service = DiscoveredService {
        name: format!("{}:{}", target_host, target_port),
        role: "direct".to_string(),
        host: target_host,
        port: target_port,
    };

    // Pregătește fișierele
    let files: Vec<FileInfo> = file_paths
        .iter()
        .filter_map(|p| {
            let path = PathBuf::from(p);
            let metadata = std::fs::metadata(&path).ok()?;
            Some(FileInfo {
                path: p.clone(),
                name: path.file_name()?.to_string_lossy().to_string(),
                size: metadata.len(),
            })
        })
        .collect();

    if files.is_empty() {
        return Err("Nu s-au găsit fișiere valide".to_string());
    }

    // Reset flag și trimite fișierele
    state.is_transfer_cancelled.store(false, Ordering::Relaxed);
    let is_cancelled = Arc::clone(&state.is_transfer_cancelled);
    transfer::send_files_to_receiver(&service, &photographer_name, &files, is_cancelled, window).await
}

fn get_media_extensions_list() -> Vec<&'static str> {
    vec![
        // RAW
        "cr2", "cr3", "crw", "nef", "nrw", "arw", "srf", "sr2", "raf", "rw2", "rwl", "orf", "pef",
        "ptx", "srw", "3fr", "fff", "iiq", "x3f", "gpr", "dng", "raw",
        // Image
        "jpg", "jpeg", "png", "tiff", "tif", "heic", "heif", "webp", "bmp", "gif",
        // Video
        "mp4", "mov", "avi", "mkv", "mxf", "m4v", "wmv", "braw", "r3d", "crm",
    ]
}

#[tauri::command]
async fn get_media_extensions() -> Vec<String> {
    get_media_extensions_list()
        .into_iter()
        .map(String::from)
        .collect()
}

/// Expand paths - if a path is a folder, recursively find all media files inside
/// Returns a flat list of file paths (no folders)
#[tauri::command]
async fn expand_paths(paths: Vec<String>) -> Result<Vec<String>, String> {
    use std::collections::HashSet;

    let extensions: HashSet<&str> = get_media_extensions_list().into_iter().collect();
    let mut result: Vec<String> = Vec::new();

    fn collect_files(
        path: &std::path::Path,
        extensions: &HashSet<&str>,
        result: &mut Vec<String>,
    ) {
        if path.is_file() {
            // Check if it's a media file
            if let Some(ext) = path.extension() {
                let ext_lower = ext.to_string_lossy().to_lowercase();
                if extensions.contains(ext_lower.as_str()) {
                    result.push(path.to_string_lossy().to_string());
                }
            }
        } else if path.is_dir() {
            // Recursively process directory
            if let Ok(entries) = std::fs::read_dir(path) {
                let mut entries: Vec<_> = entries.filter_map(|e| e.ok()).collect();
                // Sort by name for consistent ordering
                entries.sort_by_key(|e| e.file_name());

                for entry in entries {
                    let entry_path = entry.path();
                    // Skip hidden files/folders (starting with .)
                    if let Some(name) = entry_path.file_name() {
                        if !name.to_string_lossy().starts_with('.') {
                            collect_files(&entry_path, extensions, result);
                        }
                    }
                }
            }
        }
    }

    for path_str in paths {
        let path = std::path::Path::new(&path_str);
        collect_files(path, &extensions, &mut result);
    }

    Ok(result)
}

#[tauri::command]
async fn save_config(name: String) -> Result<(), String> {
    let config_path = dirs::home_dir()
        .ok_or("Nu s-a găsit directorul home")?
        .join(".photo_transfer_sender_tauri.json");

    let config = serde_json::json!({ "name": name });
    std::fs::write(&config_path, serde_json::to_string_pretty(&config).unwrap())
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn load_config() -> Result<Option<String>, String> {
    let config_path = dirs::home_dir()
        .ok_or("Nu s-a găsit directorul home")?
        .join(".photo_transfer_sender_tauri.json");

    if !config_path.exists() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(&config_path).map_err(|e| e.to_string())?;
    let config: serde_json::Value = serde_json::from_str(&content).map_err(|e| e.to_string())?;
    Ok(config.get("name").and_then(|n| n.as_str()).map(String::from))
}

#[tauri::command]
async fn add_manual_service(
    state: State<'_, AppState>,
    ip: String,
    port: u16,
    role: String,
    name: String,
) -> Result<(), String> {
    let mut services = state.discovered_services.lock().map_err(|e| e.to_string())?;
    services.insert(
        name.clone(),
        DiscoveredService {
            name,
            role,
            host: ip,
            port,
        },
    );
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReceiverInfo {
    pub name: String,
    pub role: String,
}

#[tauri::command]
async fn check_duplicates_before_send(
    target_host: String,
    target_port: u16,
    photographer_name: String,
    file_paths: Vec<String>,
    window: tauri::Window,
) -> Result<transfer::DuplicateCheckResult, String> {
    let service = DiscoveredService {
        name: format!("{}:{}", target_host, target_port),
        role: "direct".to_string(),
        host: target_host,
        port: target_port,
    };

    let files: Vec<FileInfo> = file_paths
        .iter()
        .filter_map(|p| {
            let path = PathBuf::from(p);
            let metadata = std::fs::metadata(&path).ok()?;
            Some(FileInfo {
                path: p.clone(),
                name: path.file_name()?.to_string_lossy().to_string(),
                size: metadata.len(),
            })
        })
        .collect();

    if files.is_empty() {
        return Err("Nu s-au găsit fișiere valide".to_string());
    }

    // Verificare duplicate doar după nume (instant, fără checksum)
    transfer::check_duplicates(&service, &photographer_name, &files, Some(&window))
}

#[tauri::command]
async fn send_files_with_selection(
    state: State<'_, AppState>,
    target_host: String,
    target_port: u16,
    photographer_name: String,
    file_paths: Vec<String>,
    files_to_send: Vec<String>,
    window: tauri::Window,
) -> Result<(), String> {
    let service = DiscoveredService {
        name: format!("{}:{}", target_host, target_port),
        role: "direct".to_string(),
        host: target_host,
        port: target_port,
    };

    let files: Vec<FileInfo> = file_paths
        .iter()
        .filter_map(|p| {
            let path = PathBuf::from(p);
            let metadata = std::fs::metadata(&path).ok()?;
            Some(FileInfo {
                path: p.clone(),
                name: path.file_name()?.to_string_lossy().to_string(),
                size: metadata.len(),
            })
        })
        .collect();

    if files.is_empty() {
        return Err("Nu s-au găsit fișiere valide".to_string());
    }

    // Reset flag și trimite fișierele selectate
    state.is_transfer_cancelled.store(false, Ordering::Relaxed);
    let is_cancelled = Arc::clone(&state.is_transfer_cancelled);
    transfer::send_files_with_selection(
        &service,
        &photographer_name,
        &files,
        Some(files_to_send),
        is_cancelled,
        window,
    )
    .await
}

#[tauri::command]
async fn cancel_transfer(state: State<'_, AppState>) -> Result<(), String> {
    state.is_transfer_cancelled.store(true, Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
async fn get_receiver_info(ip: String, port: u16) -> Result<ReceiverInfo, String> {
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::time::Duration;

    // Connect to receiver
    let addr = format!("{}:{}", ip, port);
    let mut stream = TcpStream::connect_timeout(
        &addr.parse().map_err(|e: std::net::AddrParseError| e.to_string())?,
        Duration::from_secs(5),
    )
    .map_err(|e| format!("Nu s-a putut conecta la {}: {}", addr, e))?;

    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream.set_write_timeout(Some(Duration::from_secs(5))).ok();

    // Send INFO request (header_len = 0)
    stream
        .write_all(&0u32.to_be_bytes())
        .map_err(|e| format!("Eroare trimitere request: {}", e))?;

    // Read response length
    let mut len_buf = [0u8; 4];
    stream
        .read_exact(&mut len_buf)
        .map_err(|e| format!("Eroare citire răspuns: {}", e))?;
    let response_len = u32::from_be_bytes(len_buf) as usize;

    // Read response
    let mut response_buf = vec![0u8; response_len];
    stream
        .read_exact(&mut response_buf)
        .map_err(|e| format!("Eroare citire date: {}", e))?;

    // Parse JSON
    let info: ReceiverInfo =
        serde_json::from_slice(&response_buf).map_err(|e| format!("Eroare parsare: {}", e))?;

    Ok(info)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let discovered_services = Arc::new(Mutex::new(HashMap::new()));
    let services_clone = Arc::clone(&discovered_services);
    let services_clone_remove = Arc::clone(&discovered_services);

    // Pornește service discovery
    let discovery = ServiceDiscovery::new(
        move |service| {
            let mut services = services_clone.lock().unwrap();
            services.insert(service.name.clone(), service);
        },
        move |fullname| {
            let mut services = services_clone_remove.lock().unwrap();
            // Remove service by matching fullname pattern in the stored services
            services.retain(|name, _| !fullname.contains(name) && !name.contains(&fullname.split('.').next().unwrap_or("")));
            println!("mDNS: Services after removal: {:?}", services.keys().collect::<Vec<_>>());
        },
    );

    let app_state = AppState {
        discovery: Arc::new(Mutex::new(discovery)),
        discovered_services,
        is_transfer_cancelled: Arc::new(AtomicBool::new(false)),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            get_services,
            send_files,
            send_files_to_host,
            get_media_extensions,
            expand_paths,
            save_config,
            load_config,
            add_manual_service,
            get_receiver_info,
            check_duplicates_before_send,
            send_files_with_selection,
            cancel_transfer,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
