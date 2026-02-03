use crate::discovery::DiscoveredService;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tauri::Emitter;

/// Deschide un fișier pentru citire, cu suport pentru sharing pe Windows
fn open_file_for_read(path: &str) -> std::io::Result<std::fs::File> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE = 0x7
        std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0x7)
            .open(path)
    }

    #[cfg(not(target_os = "windows"))]
    {
        std::fs::File::open(path)
    }
}

const CHUNK_SIZE: usize = 4 * 1024 * 1024; // 4 MB chunks

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileInfo {
    pub path: String,          // Calea absolută pe sursă
    pub name: String,          // Numele fișierului
    pub relative_path: String, // Calea relativă (ex: "web/photo.jpg") pentru structura folder
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendProgress {
    pub file_name: String,
    pub file_index: usize,
    pub total_files: usize,
    pub bytes_sent: u64,
    pub total_bytes: u64,
    pub speed_mbps: f64,
    pub target_name: String,
}

#[derive(Serialize, Deserialize)]
struct TransferHeader {
    photographer: String,
    files: Vec<FileMetadata>,
    is_folder_transfer: bool,
    folder_name: Option<String>,
    #[serde(default)]
    sender_role: Option<String>, // "tagger" sau "editor"
}

#[derive(Serialize, Deserialize)]
struct FileMetadata {
    name: String,
    #[serde(default)]
    relative_path: String, // Calea relativă pentru structura folder
    size: u64,
    checksum: String,
}

#[derive(Serialize, Deserialize)]
struct AckResponse {
    status: String,
    folder: Option<String>,
}

pub async fn send_files_to_editor(
    service: &DiscoveredService,
    sender_name: &str,
    sender_role: &str,
    files: &[FileInfo],
    folder_name: Option<String>,  // Numele original al folderului (pentru receiver→receiver)
    window: tauri::Window,
    is_cancelled: Arc<AtomicBool>,
) -> Result<(), String> {
    // Resetează flagul de anulare la începutul transferului
    is_cancelled.store(false, Ordering::Relaxed);
    // Conectare la editor
    let addr = format!("{}:{}", service.host, service.port);
    println!("Conectare la editor: {}", addr);

    let mut stream = TcpStream::connect(&addr)
        .map_err(|e| format!("Nu m-am putut conecta la {}: {}", addr, e))?;

    // Setează timeout pentru operațiuni
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(60)))
        .map_err(|e| format!("Eroare setare read timeout: {}", e))?;
    stream
        .set_write_timeout(Some(std::time::Duration::from_secs(60)))
        .map_err(|e| format!("Eroare setare write timeout: {}", e))?;
    stream
        .set_nodelay(true)
        .map_err(|e| format!("Eroare setare TCP nodelay: {}", e))?;

    println!("Conectat cu succes la {}", addr);

    // Construiește metadata FĂRĂ checksum
    let file_metadata: Vec<FileMetadata> = files
        .iter()
        .map(|f| FileMetadata {
            name: f.name.clone(),
            relative_path: f.relative_path.clone(), // Include calea relativă pentru subfoldere
            size: f.size,
            checksum: String::new(), // Fără checksum
        })
        .collect();

    let total_bytes: u64 = files.iter().map(|f| f.size).sum();

    // Trimite header-ul
    let header = TransferHeader {
        photographer: sender_name.to_string(),
        files: file_metadata,
        is_folder_transfer: folder_name.is_some(),
        folder_name,  // Trimite numele original al folderului (pentru receiver→receiver)
        sender_role: Some(sender_role.to_string()),
    };

    let header_json = serde_json::to_string(&header).map_err(|e| e.to_string())?;
    let header_bytes = header_json.as_bytes();

    // Trimite lungimea header-ului (4 bytes, big endian)
    println!("Trimit header ({} bytes)...", header_bytes.len());
    stream
        .write_all(&(header_bytes.len() as u32).to_be_bytes())
        .map_err(|e| format!("Eroare trimitere lungime header: {}", e))?;

    // Trimite header-ul
    stream
        .write_all(header_bytes)
        .map_err(|e| format!("Eroare trimitere header: {}", e))?;
    stream.flush().map_err(|e| format!("Eroare flush header: {}", e))?;

    println!("Header trimis, aștept ACK...");

    // Așteaptă ACK
    let mut ack_len_buf = [0u8; 4];
    stream
        .read_exact(&mut ack_len_buf)
        .map_err(|e| format!("Eroare citire lungime ACK: {}", e))?;
    let ack_len = u32::from_be_bytes(ack_len_buf) as usize;

    let mut ack_buf = vec![0u8; ack_len];
    stream
        .read_exact(&mut ack_buf)
        .map_err(|e| format!("Eroare citire ACK: {}", e))?;

    let ack: AckResponse =
        serde_json::from_slice(&ack_buf).map_err(|e| format!("Eroare parsare ACK: {}", e))?;

    println!("ACK primit: status={}", ack.status);

    if ack.status != "ready" {
        return Err(format!("Receiver nu e gata: {}", ack.status));
    }

    // Trimite lista de fișiere de transferat (serverul așteaptă această listă)
    // Folosim relative_path pentru a identifica corect fișierele cu subfoldere
    println!("Trimit lista de {} fișiere...", files.len());
    let files_to_send: Vec<String> = files.iter().map(|f| f.relative_path.clone()).collect();
    let decision_json = serde_json::to_string(&files_to_send).map_err(|e| e.to_string())?;
    let decision_bytes = decision_json.as_bytes();

    stream
        .write_all(&(decision_bytes.len() as u32).to_be_bytes())
        .map_err(|e| format!("Eroare trimitere decizie len: {}", e))?;
    stream
        .write_all(decision_bytes)
        .map_err(|e| format!("Eroare trimitere decizie: {}", e))?;

    // Trimite fișierele
    let mut total_sent: u64 = 0;
    let start_time = Instant::now();

    for (index, file) in files.iter().enumerate() {
        println!("Încerc să deschid fișierul: {}", file.path);

        // Verifică dacă fișierul există
        let path = std::path::Path::new(&file.path);
        if !path.exists() {
            return Err(format!("Fișierul nu există: {}", file.path));
        }

        // Verifică metadatele
        match std::fs::metadata(&file.path) {
            Ok(meta) => {
                println!("Metadata: readonly={}, len={}", meta.permissions().readonly(), meta.len());
            }
            Err(e) => {
                println!("Nu pot citi metadata: {}", e);
            }
        }

        let mut file_handle = open_file_for_read(&file.path)
            .map_err(|e| format!("Nu pot deschide {}: {} (path: {})", file.name, e, file.path))?;

        let mut buffer = vec![0u8; CHUNK_SIZE];
        let mut file_sent: u64 = 0;

        while file_sent < file.size {
            // Verifică dacă transferul a fost anulat
            if is_cancelled.load(Ordering::Relaxed) {
                println!("Transfer anulat de utilizator");
                let _ = window.emit("send-cancelled", ());
                return Err("Transfer anulat".to_string());
            }

            let bytes_read = file_handle
                .read(&mut buffer)
                .map_err(|e| format!("Eroare citire {}: {}", file.name, e))?;

            if bytes_read == 0 {
                break;
            }

            stream
                .write_all(&buffer[..bytes_read])
                .map_err(|e| format!("Eroare trimitere {}: {}", file.name, e))?;

            file_sent += bytes_read as u64;
            total_sent += bytes_read as u64;

            // Calculează viteza
            let elapsed = start_time.elapsed().as_secs_f64();
            let speed_mbps = if elapsed > 0.0 {
                (total_sent as f64 / elapsed) / (1024.0 * 1024.0)
            } else {
                0.0
            };

            // Trimite progress la UI
            let progress = SendProgress {
                file_name: file.name.clone(),
                file_index: index,
                total_files: files.len(),
                bytes_sent: total_sent,
                total_bytes,
                speed_mbps,
                target_name: service.name.clone(),
            };

            let _ = window.emit("send-progress", &progress);
        }

        // Așteaptă confirmare pentru fișier (fără checksum)
        let mut response = [0u8; 32];
        let n = stream
            .read(&mut response)
            .map_err(|e| format!("Eroare citire confirmare: {}", e))?;

        let response_str = String::from_utf8_lossy(&response[..n]);
        if !response_str.contains("OK") {
            return Err(format!(
                "Eroare la fișierul {}: {}",
                file.name, response_str
            ));
        }
    }

    // Emite eveniment de finalizare
    let _ = window.emit("send-complete", files.len());

    Ok(())
}

pub fn prepare_files(paths: &[String]) -> Vec<FileInfo> {
    let mut files = Vec::new();

    for p in paths {
        let path = PathBuf::from(p);

        if path.is_dir() {
            // Dacă e folder, parcurge RECURSIV toate fișierele și subfolderele
            collect_files_recursive(&path, &path, &mut files);
        } else if path.is_file() {
            // Dacă e fișier individual, adaugă-l direct
            if let Ok(metadata) = std::fs::metadata(&path) {
                if let Some(name) = path.file_name() {
                    let name_str = name.to_string_lossy().to_string();
                    files.push(FileInfo {
                        path: p.clone(),
                        name: name_str.clone(),
                        relative_path: name_str, // Fișier individual - relative_path = name
                        size: metadata.len(),
                    });
                }
            }
        }
    }

    files
}

/// Parcurge recursiv un folder și colectează toate fișierele
/// `current` = folderul curent de parcurs
/// `root` = folderul rădăcină (pentru a calcula calea relativă)
/// Ignoră fișierele și folderele ascunse (care încep cu `.`)
fn collect_files_recursive(current: &Path, root: &Path, files: &mut Vec<FileInfo>) {
    if let Ok(entries) = std::fs::read_dir(current) {
        for entry in entries.filter_map(|e| e.ok()) {
            let entry_path = entry.path();

            // Ignoră fișierele și folderele ascunse (care încep cu `.`)
            if let Some(name) = entry_path.file_name() {
                let name_str = name.to_string_lossy();
                if name_str.starts_with('.') {
                    continue; // Skip .DS_Store, .hidden, etc.
                }
            }

            if entry_path.is_dir() {
                // Recursiv în subfolder
                collect_files_recursive(&entry_path, root, files);
            } else if entry_path.is_file() {
                // Calculează calea relativă față de root
                let relative = entry_path
                    .strip_prefix(root)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| {
                        entry_path
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default()
                    });

                if let Ok(metadata) = std::fs::metadata(&entry_path) {
                    files.push(FileInfo {
                        path: entry_path.to_string_lossy().to_string(),
                        name: entry_path
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default(),
                        relative_path: relative,
                        size: metadata.len(),
                    });
                }
            }
        }
    }
}
