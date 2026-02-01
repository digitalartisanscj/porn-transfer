use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Istoric trimiteri pentru sender
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendRecord {
    pub timestamp: DateTime<Utc>,
    pub target_name: String,     // Numele destinatarului
    pub target_role: String,     // "tagger" sau "editor"
    pub file_count: usize,
    pub total_size: u64,
    #[serde(default)]
    pub status: SendStatus,
    #[serde(default)]
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub enum SendStatus {
    #[default]
    Success,
    Error,
    Cancelled,
}

fn send_history_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".photo_transfer_send_history.json")
}

pub fn load_send_history() -> Result<Vec<SendRecord>, String> {
    let path = send_history_path();
    if !path.exists() {
        return Ok(Vec::new());
    }

    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    serde_json::from_str(&content).map_err(|e| e.to_string())
}

pub fn save_send_history(history: &[SendRecord]) -> Result<(), String> {
    let path = send_history_path();
    // Keep only last 500 records
    let to_save: Vec<_> = history.iter().rev().take(500).cloned().collect();
    let content = serde_json::to_string_pretty(&to_save).map_err(|e| e.to_string())?;
    std::fs::write(&path, content).map_err(|e| e.to_string())
}

pub fn add_send_record(record: SendRecord) -> Result<(), String> {
    let mut history = load_send_history().unwrap_or_default();
    history.push(record);
    save_send_history(&history)
}

pub fn clear_send_history() -> Result<(), String> {
    let path = send_history_path();
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| e.to_string())?;
    }
    Ok(())
}
