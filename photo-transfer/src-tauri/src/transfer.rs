use crate::{DiscoveredService, FileInfo, TransferProgress};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Instant;
use tauri::Emitter;

const CHUNK_SIZE: usize = 4 * 1024 * 1024; // 4 MB chunks pentru viteză maximă

#[derive(Serialize, Deserialize)]
struct TransferHeader {
    photographer: String,
    files: Vec<FileMetadata>,
    is_folder_transfer: bool,
    folder_name: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
struct FileMetadata {
    name: String,
    size: u64,
    checksum: String, // Păstrat pentru compatibilitate, dar gol
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateInfo {
    pub file_name: String,
    pub existing_path: String,
    pub existing_size: u64,
    pub new_size: u64,
    pub same_checksum: bool,
}

#[derive(Serialize, Deserialize)]
struct AckResponse {
    status: String,
    folder: Option<String>,
    #[serde(default)]
    duplicates: Vec<DuplicateInfo>,
    #[serde(default)]
    resume_folder: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateCheckResult {
    pub duplicates: Vec<DuplicateInfo>,
    pub resume_folder: Option<String>,
    pub target_folder: String,
}

/// Verifică duplicatele înainte de transfer - doar după nume (INSTANT)
pub fn check_duplicates(
    service: &DiscoveredService,
    photographer_name: &str,
    files: &[FileInfo],
    _window: Option<&tauri::Window>,
) -> Result<DuplicateCheckResult, String> {
    // Conectare la receiver
    let addr = format!("{}:{}", service.host, service.port);
    let mut stream = TcpStream::connect(&addr)
        .map_err(|e| format!("Nu m-am putut conecta la {}: {}", addr, e))?;

    stream
        .set_nodelay(true)
        .map_err(|e| format!("Eroare setare TCP nodelay: {}", e))?;

    // Construiește metadata FĂRĂ checksum
    let file_metadata: Vec<FileMetadata> = files
        .iter()
        .map(|f| FileMetadata {
            name: f.name.clone(),
            size: f.size,
            checksum: String::new(),
        })
        .collect();

    // Trimite header-ul
    let header = TransferHeader {
        photographer: photographer_name.to_string(),
        files: file_metadata,
        is_folder_transfer: false,
        folder_name: None,
    };

    let header_json = serde_json::to_string(&header).map_err(|e| e.to_string())?;
    let header_bytes = header_json.as_bytes();

    stream
        .write_all(&(header_bytes.len() as u32).to_be_bytes())
        .map_err(|e| format!("Eroare trimitere lungime header: {}", e))?;

    stream
        .write_all(header_bytes)
        .map_err(|e| format!("Eroare trimitere header: {}", e))?;

    // Așteaptă ACK cu informații despre duplicate
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

    if ack.status != "ready" {
        return Err(format!("Receiver nu e gata: {}", ack.status));
    }

    // Trimite lista goală pentru a închide conexiunea
    let empty_list: Vec<String> = Vec::new();
    let decision_json = serde_json::to_string(&empty_list).map_err(|e| e.to_string())?;
    let decision_bytes = decision_json.as_bytes();

    stream
        .write_all(&(decision_bytes.len() as u32).to_be_bytes())
        .ok();
    stream.write_all(decision_bytes).ok();

    Ok(DuplicateCheckResult {
        duplicates: ack.duplicates,
        resume_folder: ack.resume_folder,
        target_folder: ack.folder.unwrap_or_default(),
    })
}

pub async fn send_files_to_receiver(
    service: &DiscoveredService,
    photographer_name: &str,
    files: &[FileInfo],
    window: tauri::Window,
) -> Result<(), String> {
    send_files_with_selection(service, photographer_name, files, None, window).await
}

/// Trimite fișierele selectate - FĂRĂ checksum (transfer direct, rapid)
pub async fn send_files_with_selection(
    service: &DiscoveredService,
    photographer_name: &str,
    files: &[FileInfo],
    files_to_send: Option<Vec<String>>,
    window: tauri::Window,
) -> Result<(), String> {
    // Determină ce fișiere să trimită
    let files_filtered: Vec<&FileInfo> = if let Some(ref selected) = files_to_send {
        files.iter().filter(|f| selected.contains(&f.name)).collect()
    } else {
        files.iter().collect()
    };

    if files_filtered.is_empty() {
        let _ = window.emit("transfer-complete", 0);
        return Ok(());
    }

    // Conectare la receiver
    let addr = format!("{}:{}", service.host, service.port);
    let mut stream = TcpStream::connect(&addr)
        .map_err(|e| format!("Nu m-am putut conecta la {}: {}", addr, e))?;

    stream
        .set_nodelay(true)
        .map_err(|e| format!("Eroare setare TCP nodelay: {}", e))?;

    // Construiește metadata FĂRĂ checksum
    let file_metadata: Vec<FileMetadata> = files_filtered
        .iter()
        .map(|f| FileMetadata {
            name: f.name.clone(),
            size: f.size,
            checksum: String::new(),
        })
        .collect();

    // Trimite header-ul
    let header = TransferHeader {
        photographer: photographer_name.to_string(),
        files: file_metadata,
        is_folder_transfer: false,
        folder_name: None,
    };

    let header_json = serde_json::to_string(&header).map_err(|e| e.to_string())?;
    let header_bytes = header_json.as_bytes();

    stream
        .write_all(&(header_bytes.len() as u32).to_be_bytes())
        .map_err(|e| format!("Eroare trimitere lungime header: {}", e))?;

    stream
        .write_all(header_bytes)
        .map_err(|e| format!("Eroare trimitere header: {}", e))?;

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

    if ack.status != "ready" {
        return Err(format!("Receiver nu e gata: {}", ack.status));
    }

    // Trimite lista de fișiere de transferat
    let selected_names: Vec<String> = files_filtered.iter().map(|f| f.name.clone()).collect();
    let decision_json = serde_json::to_string(&selected_names).map_err(|e| e.to_string())?;
    let decision_bytes = decision_json.as_bytes();

    stream
        .write_all(&(decision_bytes.len() as u32).to_be_bytes())
        .map_err(|e| format!("Eroare trimitere decizie len: {}", e))?;
    stream
        .write_all(decision_bytes)
        .map_err(|e| format!("Eroare trimitere decizie: {}", e))?;

    let total_bytes: u64 = files_filtered.iter().map(|f| f.size).sum();

    // Trimite fișierele - direct, fără checksum
    let mut total_sent: u64 = 0;
    let start_time = Instant::now();

    for (index, file) in files_filtered.iter().enumerate() {
        let mut file_handle =
            std::fs::File::open(&file.path).map_err(|e| format!("Nu pot deschide {}: {}", file.name, e))?;

        let mut buffer = vec![0u8; CHUNK_SIZE];
        let mut file_sent: u64 = 0;

        while file_sent < file.size {
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
            let progress = TransferProgress {
                file_name: file.name.clone(),
                file_index: index,
                total_files: files_filtered.len(),
                bytes_sent: total_sent,
                total_bytes,
                speed_mbps,
            };

            let _ = window.emit("transfer-progress", &progress);
        }

        // Așteaptă confirmare pentru fișier (OK simplu, fără verificare checksum)
        let mut response = [0u8; 32];
        let n = stream
            .read(&mut response)
            .map_err(|e| format!("Eroare citire confirmare: {}", e))?;

        let response_str = String::from_utf8_lossy(&response[..n]);
        if !response_str.contains("OK") {
            return Err(format!("Eroare la fișierul {}: {}", file.name, response_str));
        }
    }

    // Emite eveniment de finalizare
    let _ = window.emit("transfer-complete", files_filtered.len());

    Ok(())
}
