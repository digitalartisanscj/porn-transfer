import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";

interface ServerState {
  running: boolean;
  port: number;
  local_ip: string;
  releases: Record<string, AppRelease>;
}

interface AppRelease {
  app_type: string;
  version: string;
  notes: string;
  aarch64_path: string | null;
  aarch64_sig: string | null;
  x64_path: string | null;
  x64_sig: string | null;
  windows_path: string | null;
  windows_sig: string | null;
  pub_date: string;
}

let serverState: ServerState | null = null;

async function updateUI() {
  try {
    serverState = await invoke<ServerState>("get_server_state");

    const statusEl = document.getElementById("server-status")!;
    const infoEl = document.getElementById("server-info")!;
    const startBtn = document.getElementById("btn-start-server")!;
    const stopBtn = document.getElementById("btn-stop-server")!;
    const ipEl = document.getElementById("server-ip")!;
    const portEl = document.getElementById("server-port")!;

    if (serverState.running) {
      statusEl.innerHTML = `<div class="status-indicator online"></div><span>Server activ</span>`;
      infoEl.style.display = "block";
      ipEl.textContent = serverState.local_ip;
      portEl.textContent = serverState.port.toString();
      startBtn.style.display = "none";
      stopBtn.style.display = "inline-block";
    } else {
      statusEl.innerHTML = `<div class="status-indicator offline"></div><span>Server oprit</span>`;
      infoEl.style.display = "none";
      startBtn.style.display = "inline-block";
      stopBtn.style.display = "none";
    }

    updateReleasesList();
  } catch (e) {
    console.error("Failed to get server state:", e);
  }
}

function updateReleasesList() {
  const listEl = document.getElementById("releases-list")!;

  if (!serverState || Object.keys(serverState.releases).length === 0) {
    listEl.innerHTML = `<p class="hint">Niciun release adăugat.</p>`;
    return;
  }

  let html = "";
  for (const [type, release] of Object.entries(serverState.releases)) {
    html += `
      <div class="release-item">
        <h3>${type.charAt(0).toUpperCase() + type.slice(1)} v${release.version}</h3>
        <p>${release.notes || "Fără descriere"}</p>
        <p>ARM64: ${release.aarch64_path ? "✅" : "❌"} | Intel: ${release.x64_path ? "✅" : "❌"} | Windows: ${release.windows_path ? "✅" : "❌"}</p>
        <p style="font-size: 0.75rem; color: #999;">Publicat: ${new Date(release.pub_date).toLocaleString()}</p>
      </div>
    `;
  }

  listEl.innerHTML = html;
}

function showMessage(text: string, type: "success" | "error") {
  const msgEl = document.getElementById("status-message")!;
  msgEl.textContent = text;
  msgEl.className = `status-message ${type}`;

  setTimeout(() => {
    msgEl.className = "status-message";
  }, 5000);
}

async function startServer() {
  try {
    await invoke("start_server");
    showMessage("Server pornit cu succes!", "success");
    updateUI();
  } catch (e) {
    showMessage(`Eroare la pornirea serverului: ${e}`, "error");
  }
}

async function stopServer() {
  try {
    await invoke("stop_server");
    showMessage("Server oprit.", "success");
    updateUI();
  } catch (e) {
    showMessage(`Eroare la oprirea serverului: ${e}`, "error");
  }
}

async function selectFile(target: "aarch64" | "x64" | "windows") {
  try {
    const filters = target === "windows"
      ? [{ name: "Windows Bundle", extensions: ["zip", "nsis.zip", "msi"] }]
      : [{ name: "App Bundle", extensions: ["app", "tar.gz", "gz"] }];

    const selected = await open({
      multiple: false,
      filters,
      directory: false
    });

    if (selected) {
      let inputId: string;
      if (target === "aarch64") inputId = "aarch64-path";
      else if (target === "x64") inputId = "x64-path";
      else inputId = "windows-path";

      const input = document.getElementById(inputId) as HTMLInputElement;
      input.value = selected as string;
    }
  } catch (e) {
    console.error("File selection error:", e);
  }
}

async function addRelease() {
  const appType = (document.getElementById("app-type") as HTMLSelectElement).value;
  const version = (document.getElementById("version") as HTMLInputElement).value;
  const notes = (document.getElementById("notes") as HTMLTextAreaElement).value;
  const aarch64Path = (document.getElementById("aarch64-path") as HTMLInputElement).value;
  const x64Path = (document.getElementById("x64-path") as HTMLInputElement).value;
  const windowsPath = (document.getElementById("windows-path") as HTMLInputElement).value;

  if (!version) {
    showMessage("Te rog introdu versiunea!", "error");
    return;
  }

  if (!aarch64Path && !x64Path && !windowsPath) {
    showMessage("Te rog selectează cel puțin un fișier!", "error");
    return;
  }

  const btn = document.getElementById("btn-add-release") as HTMLButtonElement;
  btn.textContent = "Se procesează...";
  btn.disabled = true;

  try {
    await invoke("add_release", {
      appType,
      version,
      notes,
      aarch64File: aarch64Path || null,
      x64File: x64Path || null,
      windowsFile: windowsPath || null
    });

    showMessage(`Release ${appType} v${version} adăugat cu succes!`, "success");

    // Clear form
    (document.getElementById("version") as HTMLInputElement).value = "";
    (document.getElementById("notes") as HTMLTextAreaElement).value = "";
    (document.getElementById("aarch64-path") as HTMLInputElement).value = "";
    (document.getElementById("x64-path") as HTMLInputElement).value = "";
    (document.getElementById("windows-path") as HTMLInputElement).value = "";

    updateUI();
  } catch (e) {
    showMessage(`Eroare: ${e}`, "error");
  } finally {
    btn.textContent = "Adaugă Release";
    btn.disabled = false;
  }
}

window.addEventListener("DOMContentLoaded", async () => {
  // Setup event listeners
  document.getElementById("btn-start-server")!.addEventListener("click", startServer);
  document.getElementById("btn-stop-server")!.addEventListener("click", stopServer);
  document.getElementById("btn-select-aarch64")!.addEventListener("click", () => selectFile("aarch64"));
  document.getElementById("btn-select-x64")!.addEventListener("click", () => selectFile("x64"));
  document.getElementById("btn-select-windows")!.addEventListener("click", () => selectFile("windows"));
  document.getElementById("btn-add-release")!.addEventListener("click", addRelease);

  // Listen for events
  await listen("server-started", () => {
    updateUI();
  });

  await listen("release-added", () => {
    updateUI();
  });

  // Initial state
  updateUI();

  // Auto-start server
  startServer();
});
