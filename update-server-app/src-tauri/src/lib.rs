use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use tokio::sync::RwLock;
use warp::Filter;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppRelease {
    pub app_type: String,
    pub version: String,
    pub notes: String,
    pub aarch64_path: Option<String>,
    pub aarch64_sig: Option<String>,
    pub x64_path: Option<String>,
    pub x64_sig: Option<String>,
    pub pub_date: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerState {
    pub running: bool,
    pub port: u16,
    pub local_ip: String,
    pub releases: HashMap<String, AppRelease>,
}

pub struct AppState {
    pub server_state: Arc<RwLock<ServerState>>,
    pub updates_dir: PathBuf,
    pub signing_key_path: PathBuf,
}

fn get_local_ip() -> String {
    let output = Command::new("ipconfig")
        .args(["getifaddr", "en0"])
        .output()
        .or_else(|_| {
            Command::new("ipconfig")
                .args(["getifaddr", "en1"])
                .output()
        });

    match output {
        Ok(o) => {
            let ip = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if ip.is_empty() { "127.0.0.1".to_string() } else { ip }
        }
        Err(_) => "127.0.0.1".to_string(),
    }
}

#[tauri::command]
async fn get_server_state(state: State<'_, AppState>) -> Result<ServerState, String> {
    let s = state.server_state.read().await;
    Ok(s.clone())
}

#[tauri::command]
async fn add_release(
    state: State<'_, AppState>,
    app_type: String,
    version: String,
    notes: String,
    aarch64_file: Option<String>,
    x64_file: Option<String>,
    app_handle: AppHandle,
) -> Result<AppRelease, String> {
    let updates_dir = state.updates_dir.clone();
    let app_dir = updates_dir.join(&app_type);
    fs::create_dir_all(&app_dir).map_err(|e| e.to_string())?;

    let signing_key = state.signing_key_path.clone();
    let mut aarch64_sig = None;
    let mut x64_sig = None;
    let mut aarch64_dest = None;
    let mut x64_dest = None;

    // Process ARM64 file
    if let Some(ref path) = aarch64_file {
        let src = PathBuf::from(path);
        let filename = format!("{}_aarch64.app.tar.gz", app_type);
        let dest = app_dir.join(&filename);

        if path.ends_with(".app") {
            let tar_output = Command::new("tar")
                .args(["-czf", dest.to_str().unwrap(), "-C", src.parent().unwrap().to_str().unwrap(), src.file_name().unwrap().to_str().unwrap()])
                .output()
                .map_err(|e| e.to_string())?;

            if !tar_output.status.success() {
                return Err(format!("Failed to create tar.gz: {}", String::from_utf8_lossy(&tar_output.stderr)));
            }
        } else {
            fs::copy(&src, &dest).map_err(|e| e.to_string())?;
        }

        aarch64_sig = sign_file(&dest, &signing_key).ok();
        aarch64_dest = Some(filename);
    }

    // Process x64 file
    if let Some(ref path) = x64_file {
        let src = PathBuf::from(path);
        let filename = format!("{}_x64.app.tar.gz", app_type);
        let dest = app_dir.join(&filename);

        if path.ends_with(".app") {
            let tar_output = Command::new("tar")
                .args(["-czf", dest.to_str().unwrap(), "-C", src.parent().unwrap().to_str().unwrap(), src.file_name().unwrap().to_str().unwrap()])
                .output()
                .map_err(|e| e.to_string())?;

            if !tar_output.status.success() {
                return Err(format!("Failed to create tar.gz: {}", String::from_utf8_lossy(&tar_output.stderr)));
            }
        } else {
            fs::copy(&src, &dest).map_err(|e| e.to_string())?;
        }

        x64_sig = sign_file(&dest, &signing_key).ok();
        x64_dest = Some(filename);
    }

    let local_ip = get_local_ip();
    let release = AppRelease {
        app_type: app_type.clone(),
        version: version.clone(),
        notes: notes.clone(),
        aarch64_path: aarch64_dest,
        aarch64_sig,
        x64_path: x64_dest,
        x64_sig,
        pub_date: chrono::Utc::now().to_rfc3339(),
    };

    {
        let mut s = state.server_state.write().await;
        s.releases.insert(app_type.clone(), release.clone());
    }

    generate_latest_json(&app_dir, &release, &local_ip, 8080)?;

    let _ = app_handle.emit("release-added", &release);

    Ok(release)
}

fn sign_file(file_path: &PathBuf, key_path: &PathBuf) -> Result<String, String> {
    let sig_path = format!("{}.sig", file_path.to_str().unwrap());

    let output = Command::new("minisign")
        .args([
            "-S", "-s", key_path.to_str().unwrap(),
            "-m", file_path.to_str().unwrap(),
            "-x", &sig_path,
        ])
        .output();

    match output {
        Ok(o) if o.status.success() => {
            let sig = fs::read_to_string(&sig_path)
                .map_err(|e| format!("Failed to read signature: {}", e))?;
            Ok(sig.trim().to_string())
        }
        Ok(o) => Err(format!("Signing failed: {}", String::from_utf8_lossy(&o.stderr))),
        Err(e) => Err(format!("Failed to run minisign: {}", e)),
    }
}

fn generate_latest_json(app_dir: &PathBuf, release: &AppRelease, local_ip: &str, port: u16) -> Result<(), String> {
    let mut platforms = serde_json::Map::new();

    if let (Some(ref path), Some(ref sig)) = (&release.aarch64_path, &release.aarch64_sig) {
        let mut platform = serde_json::Map::new();
        platform.insert("signature".to_string(), serde_json::Value::String(sig.clone()));
        platform.insert("url".to_string(), serde_json::Value::String(
            format!("http://{}:{}/{}/{}", local_ip, port, release.app_type, path)
        ));
        platforms.insert("darwin-aarch64".to_string(), serde_json::Value::Object(platform));
    }

    if let (Some(ref path), Some(ref sig)) = (&release.x64_path, &release.x64_sig) {
        let mut platform = serde_json::Map::new();
        platform.insert("signature".to_string(), serde_json::Value::String(sig.clone()));
        platform.insert("url".to_string(), serde_json::Value::String(
            format!("http://{}:{}/{}/{}", local_ip, port, release.app_type, path)
        ));
        platforms.insert("darwin-x86_64".to_string(), serde_json::Value::Object(platform));
    }

    let latest = serde_json::json!({
        "version": release.version,
        "notes": release.notes,
        "pub_date": release.pub_date,
        "platforms": platforms
    });

    let json_path = app_dir.join("latest.json");
    fs::write(&json_path, serde_json::to_string_pretty(&latest).unwrap())
        .map_err(|e| format!("Failed to write latest.json: {}", e))?;

    Ok(())
}

#[tauri::command]
async fn start_server(state: State<'_, AppState>, app_handle: AppHandle) -> Result<(), String> {
    let updates_dir = state.updates_dir.clone();
    let server_state = state.server_state.clone();

    {
        let s = server_state.read().await;
        if s.running {
            return Ok(());
        }
    }

    let local_ip = get_local_ip();
    let port = 8080u16;

    {
        let mut s = server_state.write().await;
        s.running = true;
        s.port = port;
        s.local_ip = local_ip.clone();
    }

    let updates_dir_clone = updates_dir.clone();
    let app_handle_clone = app_handle.clone();

    tokio::spawn(async move {
        let receiver_dir = warp::path("receiver")
            .and(warp::fs::dir(updates_dir_clone.join("receiver")));

        let sender_dir = warp::path("sender")
            .and(warp::fs::dir(updates_dir_clone.join("sender")));

        let health = warp::path("health")
            .map(|| warp::reply::json(&serde_json::json!({"status": "ok"})));

        let routes = receiver_dir
            .or(sender_dir)
            .or(health)
            .with(warp::cors().allow_any_origin());

        let addr: std::net::SocketAddr = ([0, 0, 0, 0], 8080).into();

        let _ = app_handle_clone.emit("server-started", serde_json::json!({
            "port": 8080,
            "ip": get_local_ip()
        }));

        warp::serve(routes).run(addr).await;
    });

    tokio::spawn(async move {
        use mdns_sd::{ServiceDaemon, ServiceInfo};

        if let Ok(mdns) = ServiceDaemon::new() {
            let service_type = "_http._tcp.local.";
            let instance_name = "porn-transfer-updates";
            let host_name = hostname::get().unwrap_or_default().to_string_lossy().to_string();
            let host = format!("{}.local.", host_name);

            if let Ok(service_info) = ServiceInfo::new(
                service_type,
                instance_name,
                &host,
                &local_ip,
                port,
                None,
            ) {
                let _ = mdns.register(service_info);
            }
        }
    });

    let _ = app_handle.emit("server-started", ());
    Ok(())
}

#[tauri::command]
async fn stop_server(state: State<'_, AppState>) -> Result<(), String> {
    let mut s = state.server_state.write().await;
    s.running = false;
    Ok(())
}

#[tauri::command]
fn get_updates_dir(state: State<'_, AppState>) -> String {
    state.updates_dir.to_string_lossy().to_string()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let home = dirs::home_dir().unwrap_or_default();
    let updates_dir = home.join("PornTransferUpdates");
    fs::create_dir_all(&updates_dir).ok();
    fs::create_dir_all(updates_dir.join("receiver")).ok();
    fs::create_dir_all(updates_dir.join("sender")).ok();

    let signing_key_path = home.join(".tauri").join("porn-receiver.key");

    let app_state = AppState {
        server_state: Arc::new(RwLock::new(ServerState {
            running: false,
            port: 8080,
            local_ip: get_local_ip(),
            releases: HashMap::new(),
        })),
        updates_dir,
        signing_key_path,
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_opener::init())
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            get_server_state,
            add_release,
            start_server,
            stop_server,
            get_updates_dir,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
