use crate::config::{save_history, ReceiverConfig, TransferRecord};
use crate::TransferProgress;
use chrono::Utc;
use mdns_sd::{ServiceDaemon, ServiceInfo};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::Emitter;

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


// Caută un folder existent pentru acest fotograf (pentru reluare transfer)
fn find_existing_folder(base_path: &std::path::Path, photographer: &str, day_folder: Option<&str>) -> Option<std::path::PathBuf> {
    let search_path = if let Some(day) = day_folder {
        base_path.join(day)
    } else {
        base_path.to_path_buf()
    };

    if !search_path.exists() {
        return None;
    }

    // Caută foldere care conțin numele fotografului
    if let Ok(entries) = std::fs::read_dir(&search_path) {
        let mut matching_folders: Vec<_> = entries
            .filter_map(|e| e.ok())
            .filter(|e| {
                let name = e.file_name().to_string_lossy().to_lowercase();
                name.contains(&photographer.to_lowercase()) && e.path().is_dir()
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
                println!("Connection from: {}", addr);

                let config = {
                    let c = config_state.lock().map_err(|e| e.to_string())?;
                    c.clone()
                };

                if let Err(e) = handle_connection(
                    stream,
                    config,
                    &config_state,
                    &history,
                    &window,
                ) {
                    eprintln!("Transfer error: {}", e);
                    let _ = window.emit("transfer-error", e.to_string());
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
    window: &tauri::Window,
) -> Result<(), String> {
    stream.set_nonblocking(false).map_err(|e| e.to_string())?;
    stream.set_nodelay(true).map_err(|e| e.to_string())?;

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

    // Caută folder existent pentru acest fotograf (pentru reluare transfer)
    let existing_folder = find_existing_folder(&search_base, &header.photographer, None);

    // Decide dacă reluăm în folderul existent sau vom crea unul nou
    // IMPORTANT: Nu creăm folderul încă - așteptăm confirmarea de la sender
    let (existing_resume_folder, is_resume) = if let Some(ref existing) = existing_folder {
        // Verifică dacă există fișiere parțial transferate
        let existing_files_count = std::fs::read_dir(existing)
            .map(|entries| entries.count())
            .unwrap_or(0);

        if existing_files_count > 0 && existing_files_count < header.files.len() {
            // Transfer parțial - vom relua în același folder
            (Some(existing.clone()), true)
        } else {
            // Folder complet sau gol - vom crea folder nou
            (None, false)
        }
    } else {
        // Nu există folder - vom crea unul nou
        (None, false)
    };

    // Pentru verificarea duplicatelor, folosim folderul existent dacă există
    // sau căutăm în toată ziua/categoria
    let mut all_duplicates = if let Some(ref resume_folder) = existing_resume_folder {
        check_duplicates_in_folder(resume_folder, &header.files)
    } else {
        Vec::new()
    };

    // Verifică duplicate în toată ziua (doar pentru taggeri sau în alte foldere pentru editori)
    let day_duplicates = if config.role == "tagger" {
        find_duplicates_in_day(&base_path, day_folder, &header.files, existing_resume_folder.as_deref())
    } else if config.role == "editor" {
        find_duplicates_in_day(&search_base, None, &header.files, existing_resume_folder.as_deref())
    } else {
        Vec::new()
    };

    // Adaugă duplicatele din ziuă (care nu sunt deja în lista din folderul curent)
    for dup in day_duplicates {
        if !all_duplicates.iter().any(|d| d.file_name == dup.file_name) {
            all_duplicates.push(dup);
        }
    }

    // Send ACK cu informații despre duplicate
    // Notă: folder-ul indicat este orientativ - va fi creat efectiv doar la transfer
    let ack = AckResponse {
        status: "ready".to_string(),
        folder: existing_resume_folder.as_ref().map(|p| p.to_string_lossy().to_string()),
        duplicates: all_duplicates.clone(),
        resume_folder: if is_resume { existing_resume_folder.as_ref().map(|p| p.to_string_lossy().to_string()) } else { None },
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
        return Ok(());
    }

    // ACUM creăm folderul - doar după ce senderul a confirmat că vrea să trimită fișiere
    let full_path = if let Some(resume_folder) = existing_resume_folder {
        // Folosim folderul existent pentru reluare
        resume_folder
    } else {
        // Creăm folder nou - ACUM incrementăm contorul
        let folder_name = {
            let mut cfg = config_state.lock().map_err(|e| e.to_string())?;
            cfg.generate_unique_folder_name(&header.photographer, &search_base)
        };
        let path = if config.role == "editor" {
            base_path.join(source_category).join(&folder_name)
        } else {
            config.get_full_path(&folder_name)
        };
        path
    };

    // Create directory - doar acum, când știm sigur că vom primi fișiere
    std::fs::create_dir_all(&full_path)
        .map_err(|e| format!("Eroare creare folder: {}", e))?;

    let files_to_receive: Vec<&FileMetadata> = header
        .files
        .iter()
        .filter(|f| files_to_send.contains(&f.name))
        .collect();

    // Emit transfer started
    let _ = window.emit("transfer-started", &header.photographer);

    // Receive files
    let total_bytes: u64 = files_to_receive.iter().map(|f| f.size).sum();
    let mut total_received: u64 = 0;
    let start_time = Instant::now();
    let total_files_count = files_to_receive.len();

    for (index, file_meta) in files_to_receive.iter().enumerate() {
        let file_path = full_path.join(&file_meta.name);
        let mut file = std::fs::File::create(&file_path)
            .map_err(|e| format!("Eroare creare fișier {}: {}", file_meta.name, e))?;

        let mut file_received: u64 = 0;
        let mut buffer = vec![0u8; CHUNK_SIZE];

        while file_received < file_meta.size {
            let to_read = std::cmp::min(CHUNK_SIZE, (file_meta.size - file_received) as usize);
            let bytes_read = stream
                .read(&mut buffer[..to_read])
                .map_err(|e| format!("Eroare citire date: {}", e))?;

            if bytes_read == 0 {
                return Err("Conexiune închisă prematur".to_string());
            }

            file.write_all(&buffer[..bytes_read])
                .map_err(|e| format!("Eroare scriere fișier: {}", e))?;

            file_received += bytes_read as u64;
            total_received += bytes_read as u64;

            // Calculate speed
            let elapsed = start_time.elapsed().as_secs_f64();
            let speed_mbps = if elapsed > 0.0 {
                (total_received as f64 / elapsed) / (1024.0 * 1024.0)
            } else {
                0.0
            };

            // Emit progress
            let progress = TransferProgress {
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
        stream
            .write_all(b"OK")
            .map_err(|e| format!("Eroare trimitere confirmare: {}", e))?;
    }

    // Record in history
    let record = TransferRecord {
        timestamp: Utc::now(),
        photographer: header.photographer.clone(),
        file_count: total_files_count,
        total_size: total_bytes,
        folder: full_path.to_string_lossy().to_string(),
        day: if config.use_day_folders {
            Some(config.current_day.clone())
        } else {
            None
        },
    };

    {
        let mut hist = history.lock().map_err(|e| e.to_string())?;
        hist.push(record.clone());
        let _ = save_history(&hist);
    }

    // Emit transfer complete
    let _ = window.emit("transfer-complete", &record);

    Ok(())
}
