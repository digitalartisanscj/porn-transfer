use crate::discovery::DiscoveredService;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::time::Instant;
use tauri::Emitter;

const CHUNK_SIZE: usize = 4 * 1024 * 1024; // 4 MB chunks

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileInfo {
    pub path: String,
    pub name: String,
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
    window: tauri::Window,
) -> Result<(), String> {
    // Conectare la editor
    let addr = format!("{}:{}", service.host, service.port);
    let mut stream = TcpStream::connect(&addr)
        .map_err(|e| format!("Nu m-am putut conecta la {}: {}", addr, e))?;

    stream
        .set_nodelay(true)
        .map_err(|e| format!("Eroare setare TCP nodelay: {}", e))?;

    // Calculează checksum-urile
    let file_metadata: Vec<FileMetadata> = files
        .iter()
        .map(|f| {
            let checksum = calculate_md5(&f.path).unwrap_or_default();
            FileMetadata {
                name: f.name.clone(),
                size: f.size,
                checksum,
            }
        })
        .collect();

    let total_bytes: u64 = files.iter().map(|f| f.size).sum();

    // Trimite header-ul
    let header = TransferHeader {
        photographer: sender_name.to_string(),
        files: file_metadata,
        is_folder_transfer: false,
        folder_name: None,
        sender_role: Some(sender_role.to_string()),
    };

    let header_json = serde_json::to_string(&header).map_err(|e| e.to_string())?;
    let header_bytes = header_json.as_bytes();

    // Trimite lungimea header-ului (4 bytes, big endian)
    stream
        .write_all(&(header_bytes.len() as u32).to_be_bytes())
        .map_err(|e| format!("Eroare trimitere lungime header: {}", e))?;

    // Trimite header-ul
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

    // Trimite fișierele
    let mut total_sent: u64 = 0;
    let start_time = Instant::now();

    for (index, file) in files.iter().enumerate() {
        let mut file_handle = std::fs::File::open(&file.path)
            .map_err(|e| format!("Nu pot deschide {}: {}", file.name, e))?;

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

        // Așteaptă confirmare pentru fișier
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

fn calculate_md5(path: &str) -> Result<String, std::io::Error> {
    let mut file = std::fs::File::open(path)?;
    let mut context = md5::Context::new();
    let mut buffer = [0u8; 8192];

    loop {
        let bytes_read = file.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        context.consume(&buffer[..bytes_read]);
    }

    Ok(format!("{:x}", context.compute()))
}

pub fn prepare_files(paths: &[String]) -> Vec<FileInfo> {
    paths
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
        .collect()
}
