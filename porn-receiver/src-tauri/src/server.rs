use crate::config::{save_history, ReceiverConfig, TransferRecord, TransferStatus};
use crate::TransferProgress;
use chrono::Utc;
use mdns_sd::{ServiceDaemon, ServiceInfo};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::Emitter;

const TCP_TIMEOUT_SECS: u64 = 30;

const SERVICE_TYPE: &str = "_phototransfer._tcp.local.";
const CHUNK_SIZE: usize = 4 * 1024 * 1024; // 4 MB

#[derive(Serialize, Deserialize)]
struct TransferHeader {
    photographer: String,
    files: Vec<FileMetadata>,
    is_folder_transfer: bool,
    folder_name: Option<String>,
    #[serde(default)]
    sender_role: Option<String>, // "tagger", "editor", sau None pentru fotografii
}

#[derive(Serialize, Deserialize)]
struct FileMetadata {
    name: String,
    #[serde(default)]
    relative_path: String, // Calea relativă pentru structura subfolder
    size: u64,
    checksum: String,
}

#[derive(Serialize, Deserialize)]
struct AckResponse {
    status: String,
    folder: Option<String>,
    #[serde(default)]
    duplicates: Vec<DuplicateInfo>,
    #[serde(default)]
    resume_folder: Option<String>, // Folderul existent pentru reluare transfer
}

#[derive(Serialize, Deserialize, Clone)]
struct DuplicateInfo {
    file_name: String,
    existing_path: String,  // Unde există deja fișierul
    existing_size: u64,
    new_size: u64,
    same_checksum: bool,    // True dacă checksumul e identic
}

fn get_local_ip() -> Result<String, String> {
    use std::net::UdpSocket;

    let socket = UdpSocket::bind("0.0.0.0:0").map_err(|e| e.to_string())?;
    socket.connect("8.8.8.8:80").map_err(|e| e.to_string())?;
    let addr = socket.local_addr().map_err(|e| e.to_string())?;
    Ok(addr.ip().to_string())
}


// Caută un folder temporar existent pentru acest fotograf (pentru reluare transfer)
// Folderele temporare au format: .tmp_{photographer}_{timestamp}
fn find_temp_folder(base_path: &std::path::Path, photographer: &str) -> Option<std::path::PathBuf> {
    if !base_path.exists() {
        return None;
    }

    let prefix = format!(".tmp_{}_", photographer.to_lowercase().replace(' ', "_"));

    if let Ok(entries) = std::fs::read_dir(base_path) {
        let mut matching_folders: Vec<_> = entries
            .filter_map(|e| e.ok())
            .filter(|e| {
                let name = e.file_name().to_string_lossy().to_lowercase();
                name.starts_with(&prefix) && e.path().is_dir()
            })
            .collect();

        // Sortează după timp modificare (cel mai recent primul)
        matching_folders.sort_by(|a, b| {
            let a_time = a.metadata().and_then(|m| m.modified()).ok();
            let b_time = b.metadata().and_then(|m| m.modified()).ok();
            b_time.cmp(&a_time)
        });

        matching_folders.first().map(|e| e.path())
    } else {
        None
    }
}

// Generează un nume de folder temporar unic
fn generate_temp_folder_name(photographer: &str) -> String {
    let sanitized = photographer.to_lowercase().replace(' ', "_");
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!(".tmp_{}_{}", sanitized, timestamp)
}

// Găsește toate folderele temporare din calea specificată
pub fn find_all_temp_folders(base_path: &std::path::Path) -> Vec<TempFolderInfo> {
    let mut temp_folders = Vec::new();

    if !base_path.exists() {
        return temp_folders;
    }

    // Caută în folderul de bază
    scan_for_temp_folders(base_path, &mut temp_folders, None);

    // Caută și în subfolderele DAY (pentru taggeri)
    if let Ok(entries) = std::fs::read_dir(base_path) {
        for entry in entries.filter_map(|e| e.ok()) {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("DAY ") && entry.path().is_dir() {
                scan_for_temp_folders(&entry.path(), &mut temp_folders, Some(name));
            }
        }
    }

    temp_folders
}

fn scan_for_temp_folders(path: &std::path::Path, results: &mut Vec<TempFolderInfo>, day: Option<String>) {
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.filter_map(|e| e.ok()) {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with(".tmp_") && entry.path().is_dir() {
                // Extrage numele fotografului din .tmp_{photographer}_{timestamp}
                let parts: Vec<&str> = name.trim_start_matches(".tmp_").rsplitn(2, '_').collect();
                let photographer = if parts.len() == 2 {
                    parts[1].replace('_', " ")
                } else {
                    "Unknown".to_string()
                };

                // Numără fișierele
                let (file_count, total_size) = if let Ok(files) = std::fs::read_dir(entry.path()) {
                    let mut count = 0usize;
                    let mut size = 0u64;
                    for f in files.filter_map(|e| e.ok()) {
                        if f.path().is_file() {
                            count += 1;
                            size += f.metadata().map(|m| m.len()).unwrap_or(0);
                        }
                    }
                    (count, size)
                } else {
                    (0, 0)
                };

                results.push(TempFolderInfo {
                    path: entry.path().to_string_lossy().to_string(),
                    photographer,
                    file_count,
                    total_size,
                    day: day.clone(),
                });
            }
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TempFolderInfo {
    pub path: String,
    pub photographer: String,
    pub file_count: usize,
    pub total_size: u64,
    pub day: Option<String>,
}

// Caută duplicate în toată ziua curentă (toate folderele) - doar după nume
fn find_duplicates_in_day(
    base_path: &std::path::Path,
    day_folder: Option<&str>,
    files: &[FileMetadata],
    exclude_folder: Option<&std::path::Path>,
) -> Vec<DuplicateInfo> {
    let mut duplicates = Vec::new();

    let search_path = if let Some(day) = day_folder {
        base_path.join(day)
    } else {
        base_path.to_path_buf()
    };

    if !search_path.exists() {
        return duplicates;
    }

    // Parcurge toate folderele din ziua curentă
    if let Ok(entries) = std::fs::read_dir(&search_path) {
        for entry in entries.filter_map(|e| e.ok()) {
            let folder_path = entry.path();

            // Skip folderul curent (exclude_folder)
            if let Some(exclude) = exclude_folder {
                if folder_path == exclude {
                    continue;
                }
            }

            if folder_path.is_dir() {
                // Verifică fiecare fișier din transfer - doar după nume
                for file_meta in files {
                    let potential_duplicate = folder_path.join(&file_meta.name);
                    if potential_duplicate.exists() {
                        if let Ok(metadata) = std::fs::metadata(&potential_duplicate) {
                            // Verificare doar după nume - same_checksum = true dacă dimensiunea e aceeași
                            let same_size = metadata.len() == file_meta.size;

                            duplicates.push(DuplicateInfo {
                                file_name: file_meta.name.clone(),
                                existing_path: folder_path.to_string_lossy().to_string(),
                                existing_size: metadata.len(),
                                new_size: file_meta.size,
                                same_checksum: same_size, // Folosim dimensiunea ca aproximare
                            });
                        }
                    }
                }
            }
        }
    }

    duplicates
}

// Verifică duplicate în folderul curent de transfer - doar după nume (instant)
fn check_duplicates_in_folder(
    folder_path: &std::path::Path,
    files: &[FileMetadata],
) -> Vec<DuplicateInfo> {
    let folder_path_str = folder_path.to_string_lossy().to_string();

    files
        .iter()
        .filter_map(|file_meta| {
            let file_path = std::path::Path::new(&folder_path_str).join(&file_meta.name);
            if file_path.exists() {
                if let Ok(metadata) = std::fs::metadata(&file_path) {
                    // Verificare doar după nume - same_checksum = true dacă dimensiunea e aceeași
                    let same_size = metadata.len() == file_meta.size;

                    return Some(DuplicateInfo {
                        file_name: file_meta.name.clone(),
                        existing_path: folder_path_str.clone(),
                        existing_size: metadata.len(),
                        new_size: file_meta.size,
                        same_checksum: same_size,
                    });
                }
            }
            None
        })
        .collect()
}

pub fn run_server(
    port: u16,
    config: ReceiverConfig,
    config_state: Arc<Mutex<ReceiverConfig>>,
    history: Arc<Mutex<Vec<TransferRecord>>>,
    is_running: Arc<Mutex<bool>>,
    is_cancelled: Arc<std::sync::atomic::AtomicBool>,
    window: tauri::Window,
) -> Result<(), String> {
    // Start mDNS registration
    let mdns = ServiceDaemon::new().map_err(|e| e.to_string())?;

    let service_name = format!("porn-receiver-{}", uuid::Uuid::new_v4());
    let mut properties = std::collections::HashMap::new();
    properties.insert("role".to_string(), config.role.clone());
    properties.insert("name".to_string(), config.name.clone());

    // Get local IP address
    let local_ip = get_local_ip().unwrap_or_else(|_| "0.0.0.0".to_string());
    println!("mDNS: Registering service with IP: {}", local_ip);

    let service_info = ServiceInfo::new(
        SERVICE_TYPE,
        &service_name,
        &format!("{}.local.", hostname::get().unwrap_or_default().to_string_lossy()),
        &local_ip,
        port,
        properties,
    )
    .map_err(|e| e.to_string())?;

    println!("mDNS: Service registered as {} on port {}", service_name, port);

    mdns.register(service_info).map_err(|e| e.to_string())?;

    // Emit server started event
    let _ = window.emit("server-started", port);

    // Start TCP listener
    let listener = TcpListener::bind(format!("0.0.0.0:{}", port)).map_err(|e| e.to_string())?;
    listener
        .set_nonblocking(true)
        .map_err(|e| e.to_string())?;

    loop {
        // Check if should stop
        {
            let running = is_running.lock().map_err(|e| e.to_string())?;
            if !*running {
                break;
            }
        }

        // Try to accept connection
        match listener.accept() {
            Ok((stream, addr)) => {
                println!("=== Conexiune nouă de la: {} ===", addr);

                let config = {
                    let c = config_state.lock().map_err(|e| e.to_string())?;
                    c.clone()
                };

                // Reset flag la începutul fiecărui conexiuni
                // NOTĂ: Acest flag este global - cancel va afecta toate transferurile active
                is_cancelled.store(false, std::sync::atomic::Ordering::Relaxed);

                match handle_connection(
                    stream,
                    config,
                    &config_state,
                    &history,
                    &is_cancelled,
                    &window,
                ) {
                    Ok(()) => {
                        println!("=== Conexiune finalizată cu succes de la {} ===", addr);
                    }
                    Err(e) => {
                        eprintln!("!!! Eroare conexiune de la {}: {} !!!", addr, e);
                        let _ = window.emit("transfer-error", e.to_string());
                    }
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // No connection waiting, sleep a bit
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(e) => {
                eprintln!("Accept error: {}", e);
            }
        }
    }

    // Cleanup
    let _ = mdns.shutdown();
    let _ = window.emit("server-stopped", ());

    Ok(())
}

fn handle_connection(
    mut stream: TcpStream,
    config: ReceiverConfig,
    config_state: &Arc<Mutex<ReceiverConfig>>,
    history: &Arc<Mutex<Vec<TransferRecord>>>,
    is_cancelled: &Arc<std::sync::atomic::AtomicBool>,
    window: &tauri::Window,
) -> Result<(), String> {
    stream.set_nonblocking(false).map_err(|e| e.to_string())?;
    stream.set_nodelay(true).map_err(|e| e.to_string())?;

    // Set timeout pentru a detecta deconectări
    stream.set_read_timeout(Some(Duration::from_secs(TCP_TIMEOUT_SECS))).map_err(|e| e.to_string())?;
    stream.set_write_timeout(Some(Duration::from_secs(TCP_TIMEOUT_SECS))).map_err(|e| e.to_string())?;

    // Read header length
    let mut len_buf = [0u8; 4];
    stream
        .read_exact(&mut len_buf)
        .map_err(|e| format!("Eroare citire lungime header: {}", e))?;
    let header_len = u32::from_be_bytes(len_buf) as usize;

    // If header_len is 0, this is an INFO request
    if header_len == 0 {
        let info = serde_json::json!({
            "name": config.name,
            "role": config.role,
        });
        let info_json = serde_json::to_string(&info).map_err(|e| e.to_string())?;
        let info_bytes = info_json.as_bytes();
        stream
            .write_all(&(info_bytes.len() as u32).to_be_bytes())
            .map_err(|e| format!("Eroare trimitere info len: {}", e))?;
        stream
            .write_all(info_bytes)
            .map_err(|e| format!("Eroare trimitere info: {}", e))?;
        return Ok(());
    }

    // Read header
    let mut header_buf = vec![0u8; header_len];
    stream
        .read_exact(&mut header_buf)
        .map_err(|e| format!("Eroare citire header: {}", e))?;

    let header: TransferHeader =
        serde_json::from_slice(&header_buf).map_err(|e| format!("Eroare parsare header: {}", e))?;

    // Determină categoria în funcție de sender_role
    // Pentru editori: organizare în subfoldere Fotograf/Tagger/Editor
    let source_category = match header.sender_role.as_deref() {
        Some("tagger") => "Tagger",
        Some("editor") => "Editor",
        _ => "Fotograf", // fotografii sau oricine altcineva
    };

    // Determină base_path și day_folder pentru căutarea duplicatelor
    let base_path = std::path::PathBuf::from(&config.base_path);
    let day_folder = if config.use_day_folders && config.role == "tagger" {
        Some(config.current_day.as_str())
    } else {
        None
    };

    // Calculează calea de căutare pentru foldere existente
    let search_base = if config.role == "editor" {
        base_path.join(source_category)
    } else if let Some(day) = day_folder {
        base_path.join(day)
    } else {
        base_path.clone()
    };

    // Caută folder TEMPORAR existent pentru acest fotograf (pentru reluare transfer)
    // Nu căutăm foldere finalizate - doar temporare pentru a relua transferul întrerupt
    let existing_temp_folder = find_temp_folder(&search_base, &header.photographer);

    // Dacă există folder temporar, îl folosim pentru reluare
    let (resume_temp_folder, is_resume) = if let Some(ref temp) = existing_temp_folder {
        (Some(temp.clone()), true)
    } else {
        (None, false)
    };

    // Pentru verificarea duplicatelor, folosim folderul temporar dacă există
    let mut all_duplicates = if let Some(ref temp_folder) = resume_temp_folder {
        check_duplicates_in_folder(temp_folder, &header.files)
    } else {
        Vec::new()
    };

    // Verifică duplicate în toată ziua (doar pentru taggeri sau în alte foldere pentru editori)
    // IMPORTANT: Nu căutăm duplicate în folderele finalizate - doar în folderul temporar curent
    let day_duplicates = if config.role == "tagger" {
        find_duplicates_in_day(&base_path, day_folder, &header.files, resume_temp_folder.as_deref())
    } else if config.role == "editor" {
        find_duplicates_in_day(&search_base, None, &header.files, resume_temp_folder.as_deref())
    } else {
        Vec::new()
    };

    // Adaugă duplicatele din ziuă (care nu sunt deja în lista din folderul curent)
    for dup in day_duplicates {
        if !all_duplicates.iter().any(|d| d.file_name == dup.file_name) {
            all_duplicates.push(dup);
        }
    }

    // Send ACK cu informații despre duplicate și folder temporar de reluare
    let ack = AckResponse {
        status: "ready".to_string(),
        folder: resume_temp_folder.as_ref().map(|p| p.to_string_lossy().to_string()),
        duplicates: all_duplicates.clone(),
        resume_folder: if is_resume { resume_temp_folder.as_ref().map(|p| p.to_string_lossy().to_string()) } else { None },
    };
    let ack_json = serde_json::to_string(&ack).map_err(|e| e.to_string())?;
    let ack_bytes = ack_json.as_bytes();

    stream
        .write_all(&(ack_bytes.len() as u32).to_be_bytes())
        .map_err(|e| format!("Eroare trimitere ACK len: {}", e))?;
    stream
        .write_all(ack_bytes)
        .map_err(|e| format!("Eroare trimitere ACK: {}", e))?;

    // Întotdeauna așteaptă răspunsul senderului cu lista de fișiere de trimis
    // (pentru a permite check_duplicates să funcționeze corect)
    let mut decision_len_buf = [0u8; 4];
    stream
        .read_exact(&mut decision_len_buf)
        .map_err(|e| format!("Eroare citire decizie duplicate: {}", e))?;
    let decision_len = u32::from_be_bytes(decision_len_buf) as usize;

    let mut decision_buf = vec![0u8; decision_len];
    stream
        .read_exact(&mut decision_buf)
        .map_err(|e| format!("Eroare citire date decizie: {}", e))?;

    let files_to_send: Vec<String> = serde_json::from_slice(&decision_buf)
        .map_err(|e| format!("Eroare parsare decizie: {}", e))?;

    // Dacă lista e goală, senderul a anulat (check_duplicates only)
    // Nu am creat niciun folder, deci nu trebuie să curățăm nimic
    if files_to_send.is_empty() {
        println!("--- Verificare duplicate finalizată (lista goală, fără transfer) ---");
        return Ok(());
    }

    println!("--- Se vor primi {} fișiere de la {} ---", files_to_send.len(), header.photographer);

    // ACUM creăm folderul TEMPORAR - sau folosim cel existent pentru reluare
    // Folderul va fi redenumit la final cu numele numerotat
    let (temp_path, _is_new_transfer) = if let Some(temp_folder) = resume_temp_folder {
        // Folosim folderul temporar existent pentru reluare
        (temp_folder, false)
    } else {
        // Creăm folder TEMPORAR nou - NU incrementăm contorul încă
        let temp_folder_name = generate_temp_folder_name(&header.photographer);
        let path = if config.role == "editor" {
            base_path.join(source_category).join(&temp_folder_name)
        } else if let Some(day) = day_folder {
            base_path.join(day).join(&temp_folder_name)
        } else {
            base_path.join(&temp_folder_name)
        };
        (path, true)
    };

    let full_path = temp_path.clone();

    // Create directory - doar acum, când știm sigur că vom primi fișiere
    std::fs::create_dir_all(&full_path)
        .map_err(|e| format!("Eroare creare folder: {}", e))?;

    // Filtrează fișierele de primit folosind relative_path (sau name dacă relative_path e gol)
    let files_to_receive: Vec<&FileMetadata> = header
        .files
        .iter()
        .filter(|f| {
            let key = if f.relative_path.is_empty() { &f.name } else { &f.relative_path };
            files_to_send.contains(key)
        })
        .collect();

    // Generează un transfer_id unic pentru acest transfer
    let transfer_id = format!(
        "{}_{}",
        header.photographer.replace(' ', "_"),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    );

    // Emit transfer started cu transfer_id
    let _ = window.emit("transfer-started", serde_json::json!({
        "transfer_id": transfer_id,
        "photographer": header.photographer
    }));

    // Receive files
    let total_bytes: u64 = files_to_receive.iter().map(|f| f.size).sum();
    let mut total_received: u64 = 0;
    let start_time = Instant::now();
    let total_files_count = files_to_receive.len();
    let mut files_completed: usize = 0;

    // Numără fișierele reale din folder
    let count_real_files = || -> (usize, u64) {
        if let Ok(entries) = std::fs::read_dir(&full_path) {
            let mut count = 0usize;
            let mut size = 0u64;
            for entry in entries.filter_map(|e| e.ok()) {
                if entry.path().is_file() {
                    count += 1;
                    size += entry.metadata().map(|m| m.len()).unwrap_or(0);
                }
            }
            (count, size)
        } else {
            (0, 0)
        }
    };

    let folder_path_str = full_path.to_string_lossy().to_string();

    // Helper function pentru salvare/actualizare în istoric
    let save_to_history = |hist: &Arc<Mutex<Vec<TransferRecord>>>, status: TransferStatus, tid: &str| {
        // Numără fișierele reale din folder
        let (real_file_count, real_total_size) = count_real_files();

        let record = TransferRecord {
            transfer_id: tid.to_string(),
            timestamp: Utc::now(),
            photographer: header.photographer.clone(),
            file_count: real_file_count,
            total_size: real_total_size,
            folder: folder_path_str.clone(),
            day: if config.use_day_folders {
                Some(config.current_day.clone())
            } else {
                None
            },
            status,
        };

        if let Ok(mut h) = hist.lock() {
            // Caută înregistrare existentă pentru acest folder și actualizează-o
            if let Some(existing) = h.iter_mut().find(|r| r.folder == folder_path_str) {
                existing.timestamp = record.timestamp;
                existing.file_count = record.file_count;
                existing.total_size = record.total_size;
                existing.status = record.status.clone();
            } else {
                // Nu există - adaugă nouă
                h.push(record.clone());
            }
            let _ = save_history(&h);
        }

        record
    };

    for (index, file_meta) in files_to_receive.iter().enumerate() {
        // Verifică dacă transferul a fost anulat
        if is_cancelled.load(std::sync::atomic::Ordering::Relaxed) {
            let record = save_to_history(history, TransferStatus::Partial, &transfer_id);
            let _ = window.emit("transfer-cancelled", &record);
            return Err("Transfer anulat de utilizator".to_string());
        }

        // Folosește relative_path pentru a păstra structura de subfoldere
        // Dacă relative_path e gol, folosește name
        let relative = if file_meta.relative_path.is_empty() {
            &file_meta.name
        } else {
            &file_meta.relative_path
        };
        let file_path = full_path.join(relative);

        // Creează subfoldere dacă e necesar
        if let Some(parent) = file_path.parent() {
            if parent != full_path {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    let record = save_to_history(history, TransferStatus::Error, &transfer_id);
                    let _ = window.emit("transfer-error", format!("Eroare creare subfolder: {}", e));
                    let _ = window.emit("transfer-partial", &record);
                    return Err(format!("Eroare creare subfolder: {}", e));
                }
            }
        }

        let mut file = match std::fs::File::create(&file_path) {
            Ok(f) => f,
            Err(e) => {
                // Salvează în istoric ca eroare
                let record = save_to_history(history, TransferStatus::Error, &transfer_id);
                let _ = window.emit("transfer-error", format!("Eroare creare fișier {}: {}", file_meta.name, e));
                let _ = window.emit("transfer-partial", &record);
                return Err(format!("Eroare creare fișier {}: {}", file_meta.name, e));
            }
        };

        let mut file_received: u64 = 0;
        let mut buffer = vec![0u8; CHUNK_SIZE];

        while file_received < file_meta.size {
            let to_read = std::cmp::min(CHUNK_SIZE, (file_meta.size - file_received) as usize);
            let bytes_read = match stream.read(&mut buffer[..to_read]) {
                Ok(0) => {
                    // Conexiune închisă - salvează transferul parțial în istoric
                    let record = save_to_history(history, TransferStatus::Partial, &transfer_id);
                    let _ = window.emit("transfer-partial", &record);
                    return Err("Conexiune închisă prematur".to_string());
                }
                Ok(n) => n,
                Err(e) => {
                    // Eroare de citire (inclusiv timeout) - salvează transferul parțial
                    let record = save_to_history(history, TransferStatus::Partial, &transfer_id);
                    let _ = window.emit("transfer-partial", &record);
                    return Err(format!("Eroare citire date: {}", e));
                }
            };

            if let Err(e) = file.write_all(&buffer[..bytes_read]) {
                let record = save_to_history(history, TransferStatus::Error, &transfer_id);
                let _ = window.emit("transfer-partial", &record);
                return Err(format!("Eroare scriere fișier: {}", e));
            }

            file_received += bytes_read as u64;
            total_received += bytes_read as u64;

            // Verifică dacă transferul a fost anulat după fiecare chunk
            if is_cancelled.load(std::sync::atomic::Ordering::Relaxed) {
                let record = save_to_history(history, TransferStatus::Partial, &transfer_id);
                let _ = window.emit("transfer-cancelled", &record);
                return Err("Transfer anulat de utilizator".to_string());
            }

            // Calculate speed
            let elapsed = start_time.elapsed().as_secs_f64();
            let speed_mbps = if elapsed > 0.0 {
                (total_received as f64 / elapsed) / (1024.0 * 1024.0)
            } else {
                0.0
            };

            // Emit progress
            let progress = TransferProgress {
                transfer_id: transfer_id.clone(),
                photographer: header.photographer.clone(),
                file_name: file_meta.name.clone(),
                file_index: index,
                total_files: total_files_count,
                bytes_received: total_received,
                total_bytes,
                speed_mbps,
            };
            let _ = window.emit("transfer-progress", &progress);
        }

        // Trimite OK - fără verificare checksum
        if let Err(e) = stream.write_all(b"OK") {
            let record = save_to_history(history, TransferStatus::Partial, &transfer_id);
            let _ = window.emit("transfer-partial", &record);
            return Err(format!("Eroare trimitere confirmare: {}", e));
        }

        files_completed += 1;
    }

    // Transfer complet! Acum redenumim folderul temporar la numele final
    // Pentru receiver→receiver: păstrează numele original
    // Pentru fotograf→receiver: generează nume nou cu counter
    let final_folder_name = if header.sender_role.is_some() && header.folder_name.is_some() {
        // Transfer receiver→receiver: păstrează numele original al folderului
        let original_name = header.folder_name.as_ref().unwrap().clone();

        // Verifică dacă folderul cu acest nume există deja, adaugă suffix dacă da
        let mut name = original_name.clone();
        let mut suffix = 1;
        loop {
            let check_path = if config.role == "editor" {
                base_path.join(source_category).join(&name)
            } else {
                search_base.join(&name)
            };

            if !check_path.exists() || check_path == temp_path {
                break;
            }
            suffix += 1;
            name = format!("{}_{}", original_name, suffix);
        }
        name
    } else {
        // Transfer fotograf→receiver: generează nume nou cu counter
        let mut cfg = config_state.lock().map_err(|e| e.to_string())?;
        cfg.generate_unique_folder_name(&header.photographer, &search_base)
    };

    let final_path = if config.role == "editor" {
        base_path.join(source_category).join(&final_folder_name)
    } else {
        config.get_full_path(&final_folder_name)
    };

    // Redenumește folderul temporar la numele final
    std::fs::rename(&temp_path, &final_path)
        .map_err(|e| format!("Eroare redenumire folder: {}", e))?;

    // Actualizează folder_path_str pentru salvarea în istoric
    let final_folder_path_str = final_path.to_string_lossy().to_string();

    // Numără fișierele reale din folderul final
    let (final_file_count, final_total_size) = {
        if let Ok(entries) = std::fs::read_dir(&final_path) {
            let mut count = 0usize;
            let mut size = 0u64;
            for entry in entries.filter_map(|e| e.ok()) {
                if entry.path().is_file() {
                    count += 1;
                    size += entry.metadata().map(|m| m.len()).unwrap_or(0);
                }
            }
            (count, size)
        } else {
            (files_completed, total_bytes)
        }
    };

    // Salvează în istoric cu calea finală
    let record = TransferRecord {
        transfer_id: transfer_id.clone(),
        timestamp: Utc::now(),
        photographer: header.photographer.clone(),
        file_count: final_file_count,
        total_size: final_total_size,
        folder: final_folder_path_str.clone(),
        day: if config.use_day_folders {
            Some(config.current_day.clone())
        } else {
            None
        },
        status: TransferStatus::Complete,
    };

    if let Ok(mut h) = history.lock() {
        // Elimină orice înregistrare anterioară pentru folderul temporar
        h.retain(|r| r.folder != folder_path_str);
        // Adaugă înregistrarea cu folderul final
        h.push(record.clone());
        let _ = save_history(&h);
    }

    // Emit transfer complete cu folderul final
    let _ = window.emit("transfer-complete", &record);

    Ok(())
}
