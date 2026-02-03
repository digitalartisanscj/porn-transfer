use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReceiverConfig {
    pub role: String,           // "tagger" sau "editor"
    pub name: String,           // Numele editorului/taggerului
    pub base_path: String,      // Folder-ul de bază pentru salvare
    pub folder_template: String, // Template pentru nume folder
    pub folder_counter: u32,    // Contorul curent pentru foldere (pentru editori sau când nu se folosesc zile)
    pub use_day_folders: bool,  // Organizare pe zile (doar tagger)
    pub current_day: String,    // Ziua curentă (DAY 1, DAY 2, etc.)
    pub reset_numbering_daily: bool, // Reset contor zilnic
    #[serde(default)]
    pub day_counters: HashMap<String, u32>, // Contoare separate pentru fiecare zi
    #[serde(default = "default_port")]
    pub port: u16,              // Portul TCP (diferit pentru tagger și editor pe același Mac)
}

fn default_port() -> u16 {
    45678
}

impl Default for ReceiverConfig {
    fn default() -> Self {
        let default_path = dirs::home_dir()
            .map(|p| p.join("PornTransfer"))
            .unwrap_or_else(|| PathBuf::from("~/PornTransfer"))
            .to_string_lossy()
            .to_string();

        Self {
            role: "tagger".to_string(),
            name: "".to_string(),
            base_path: default_path,
            folder_template: "{num:02d} - {name}".to_string(),
            folder_counter: 1,
            use_day_folders: true,
            current_day: "DAY 1".to_string(),
            reset_numbering_daily: true,
            day_counters: HashMap::new(),
            port: 45678,
        }
    }
}

impl ReceiverConfig {
    pub fn config_path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".porn_transfer_receiver.json")
    }

    pub fn load() -> Result<Self, String> {
        let path = Self::config_path();
        if !path.exists() {
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        serde_json::from_str(&content).map_err(|e| e.to_string())
    }

    pub fn save(&self) -> Result<(), String> {
        let path = Self::config_path();
        let content = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(&path, content).map_err(|e| e.to_string())
    }

    pub fn generate_folder_name(&mut self, photographer: &str) -> String {
        let counter = self.get_current_counter();
        let name = self.folder_template
            .replace("{name}", photographer)
            .replace("{num:02d}", &format!("{:02}", counter))
            .replace("{num:03d}", &format!("{:03}", counter))
            .replace("{num}", &counter.to_string())
            .replace("{date}", &chrono::Local::now().format("%Y-%m-%d").to_string())
            .replace("{time}", &chrono::Local::now().format("%H-%M").to_string());

        self.increment_counter();
        let _ = self.save();

        name
    }

    /// Obține contorul pentru ziua curentă (sau contorul global dacă nu se folosesc zile)
    fn get_current_counter(&self) -> u32 {
        if self.use_day_folders && self.role == "tagger" {
            *self.day_counters.get(&self.current_day).unwrap_or(&1)
        } else {
            self.folder_counter
        }
    }

    /// Incrementează contorul pentru ziua curentă
    fn increment_counter(&mut self) {
        if self.use_day_folders && self.role == "tagger" {
            let counter = self.day_counters.entry(self.current_day.clone()).or_insert(1);
            *counter += 1;
        } else {
            self.folder_counter += 1;
        }
    }

    /// Generează un nume de folder unic, verificând pe disc dacă există deja
    /// Această funcție trebuie apelată cu lock pe config
    /// Pentru taggeri cu zile: folosește contorul specific zilei curente
    pub fn generate_unique_folder_name(&mut self, photographer: &str, base_check_path: &std::path::Path) -> String {
        loop {
            let counter = self.get_current_counter();
            let name = self.folder_template
                .replace("{name}", photographer)
                .replace("{num:02d}", &format!("{:02}", counter))
                .replace("{num:03d}", &format!("{:03}", counter))
                .replace("{num}", &counter.to_string())
                .replace("{date}", &chrono::Local::now().format("%Y-%m-%d").to_string())
                .replace("{time}", &chrono::Local::now().format("%H-%M").to_string());

            let full_path = base_check_path.join(&name);

            self.increment_counter();

            // Verifică dacă folderul există deja pe disc
            if !full_path.exists() {
                let _ = self.save();
                return name;
            }

            // Dacă există, continuă să incrementeze până găsim un număr liber
            // (bucla va genera automat următorul număr)
        }
    }

    pub fn get_full_path(&self, folder_name: &str) -> PathBuf {
        let base = PathBuf::from(&self.base_path);

        if self.use_day_folders && self.role == "tagger" {
            base.join(&self.current_day).join(folder_name)
        } else {
            base.join(folder_name)
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferRecord {
    #[serde(default)]
    pub transfer_id: String, // ID unic pentru transferuri simultane
    pub timestamp: DateTime<Utc>,
    pub photographer: String,
    pub file_count: usize,
    pub total_size: u64,
    pub folder: String,
    pub day: Option<String>,
    #[serde(default)]
    pub status: TransferStatus,
    #[serde(default)]
    pub source_role: Option<String>, // "tagger", "editor", or None (fotograf)
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub enum TransferStatus {
    #[default]
    Complete,
    Partial,
    Error,
}

fn history_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".porn_transfer_history.json")
}

pub fn load_history() -> Result<Vec<TransferRecord>, String> {
    let path = history_path();
    if !path.exists() {
        return Ok(Vec::new());
    }

    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    serde_json::from_str(&content).map_err(|e| e.to_string())
}

pub fn save_history(history: &[TransferRecord]) -> Result<(), String> {
    let path = history_path();
    // Keep only last 500 records
    let to_save: Vec<_> = history.iter().rev().take(500).cloned().collect();
    let content = serde_json::to_string_pretty(&to_save).map_err(|e| e.to_string())?;
    std::fs::write(&path, content).map_err(|e| e.to_string())
}

pub fn clear_history() -> Result<(), String> {
    let path = history_path();
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

// ========== ISTORIC TRIMITERI (SENT HISTORY) ==========

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SentRecord {
    pub timestamp: DateTime<Utc>,
    pub target_name: String,     // Numele destinatarului (editor)
    pub file_count: usize,
    pub total_size: u64,
    pub folder_name: Option<String>, // Numele folderului trimis (dacă e folder)
}

fn sent_history_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".porn_transfer_sent_history.json")
}

pub fn load_sent_history() -> Result<Vec<SentRecord>, String> {
    let path = sent_history_path();
    if !path.exists() {
        return Ok(Vec::new());
    }

    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    serde_json::from_str(&content).map_err(|e| e.to_string())
}

pub fn save_sent_history(history: &[SentRecord]) -> Result<(), String> {
    let path = sent_history_path();
    // Keep only last 500 records
    let to_save: Vec<_> = history.iter().rev().take(500).cloned().collect();
    let content = serde_json::to_string_pretty(&to_save).map_err(|e| e.to_string())?;
    std::fs::write(&path, content).map_err(|e| e.to_string())
}

pub fn add_sent_record(record: SentRecord) -> Result<(), String> {
    let mut history = load_sent_history().unwrap_or_default();
    history.push(record);
    save_sent_history(&history)
}
