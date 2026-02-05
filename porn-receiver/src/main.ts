import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { getVersion } from "@tauri-apps/api/app";

interface ReceiverConfig {
  role: string;
  name: string;
  base_path: string;
  folder_template: string;
  folder_counter: number;
  use_day_folders: boolean;
  current_day: string;
  reset_numbering_daily: boolean;
  port: number;
}

interface TransferProgress {
  transfer_id: string;
  photographer: string;
  file_name: string;
  file_index: number;
  total_files: number;
  bytes_received: number;
  total_bytes: number;
  speed_mbps: number;
}

interface TransferRecord {
  transfer_id: string;
  timestamp: string;
  photographer: string;
  file_count: number;
  total_size: number;
  folder: string;
  day: string | null;
  status?: "Complete" | "Partial" | "Error";
  source_role?: string | null; // "tagger", "editor", or null (fotograf)
}

interface DiscoveredEditor {
  name: string;
  role: string;
  host: string;
  port: number;
}

interface SentRecord {
  timestamp: string;
  target_name: string;
  file_count: number;
  total_size: number;
  folder_name: string | null;
}

interface SendProgress {
  send_id: string;
  file_name: string;
  file_index: number;
  total_files: number;
  bytes_sent: number;
  total_bytes: number;
  speed_mbps: number;
  target_name: string;
}

interface SendResult {
  send_id: string;
  target_name: string;
  file_count: number;
}

interface SendError {
  send_id: string;
  target_name: string;
  error: string;
}

interface TempFolderInfo {
  path: string;
  photographer: string;
  file_count: number;
  total_size: number;
  day: string | null;
}

let config: ReceiverConfig | null = null;
let activeTransfers: Map<string, TransferProgress> = new Map();
let activeSends: Map<string, SendProgress> = new Map();  // Pentru trimiteri simultane
let discoveredEditors: DiscoveredEditor[] = [];
let selectedEditor: DiscoveredEditor | null = null;
let pendingFiles: string[] = [];
let discoveryInterval: number | null = null;
let resetClickCount = 0;
let resetClickTimer: number | null = null;

// DOM Elements
let setupScreen: HTMLElement;
let mainContent: HTMLElement;
let statusIndicator: HTMLElement;
let statusText: HTMLElement;
let ipAddress: HTMLElement;
let daySelector: HTMLElement;
let taggerSettings: HTMLElement;
let toast: HTMLElement;
let toastMessage: HTMLElement;
let tabSendBtn: HTMLElement;
let editorsList: HTMLElement;
let sendDropZone: HTMLElement;
let activeSendsContainer: HTMLElement;

window.addEventListener("DOMContentLoaded", async () => {
  initElements();
  setupEventListeners();
  await setupTauriListeners(); // Must be before loadConfig to catch server-started event
  await loadConfig();
  await showLocalIP();
  await showAppVersion(); // Afișează versiunea aplicației
  await checkTempFolders(); // Verifică foldere temporare la pornire
  checkForUpdatesOnStartup(); // Verifică actualizări în background
});

function initElements() {
  setupScreen = document.getElementById("setup-screen")!;
  mainContent = document.getElementById("main-content")!;
  statusIndicator = document.getElementById("status-indicator")!;
  statusText = document.getElementById("status-text")!;
  ipAddress = document.getElementById("ip-address")!;
  daySelector = document.getElementById("day-selector")!;
  taggerSettings = document.getElementById("tagger-settings")!;
  toast = document.getElementById("toast")!;
  toastMessage = document.getElementById("toast-message")!;
  tabSendBtn = document.getElementById("tab-send-btn")!;
  editorsList = document.getElementById("editors-list")!;
  sendDropZone = document.getElementById("send-drop-zone")!;
  activeSendsContainer = document.getElementById("active-sends-container")!;
}

async function loadConfig() {
  try {
    config = await invoke<ReceiverConfig>("get_config");

    if (config.name && config.base_path) {
      // Already configured, show main content
      showMainContent();

      // Check if server is already running
      const isRunning = await invoke<boolean>("is_server_running");
      if (isRunning) {
        statusIndicator.classList.add("online");
        statusText.textContent = "Online";
      } else {
        await startServer();
      }
    }
  } catch (e) {
    console.error("Error loading config:", e);
  }
}

async function showLocalIP() {
  try {
    const ip = await invoke<string>("get_local_ip");
    ipAddress.textContent = `IP: ${ip}`;
  } catch (e) {
    ipAddress.textContent = "";
  }
}

async function showAppVersion() {
  try {
    const version = await getVersion();
    const versionEl = document.getElementById("current-version");
    if (versionEl) {
      versionEl.textContent = `Versiune: ${version}`;
    }
  } catch (e) {
    console.error("Error getting app version:", e);
  }
}

function setupEventListeners() {
  // Setup form
  const setupForm = document.getElementById("setup-form") as HTMLFormElement;
  const setupBrowse = document.getElementById("setup-browse")!;

  setupBrowse.addEventListener("click", async () => {
    const folder = await selectFolder();
    if (folder) {
      (document.getElementById("setup-folder") as HTMLInputElement).value = folder;
    }
  });

  setupForm.addEventListener("submit", async (e) => {
    e.preventDefault();

    const role = (document.querySelector('input[name="setup-role"]:checked') as HTMLInputElement).value;
    const name = (document.getElementById("setup-name") as HTMLInputElement).value.trim();
    const folder = (document.getElementById("setup-folder") as HTMLInputElement).value;

    if (!name || !folder) {
      showToast("Completeaza toate campurile", "error");
      return;
    }

    config = {
      role,
      name,
      base_path: folder,
      folder_template: "{num:02d} - {name}",
      folder_counter: 1,
      use_day_folders: role === "tagger",
      current_day: "DAY 1",
      reset_numbering_daily: true,
      port: 45678,
    };

    try {
      await invoke("save_config", { config });
      showMainContent();
      await startServer();
    } catch (e) {
      showToast(`Eroare: ${e}`, "error");
    }
  });

  // Settings form
  const settingsForm = document.getElementById("settings-form") as HTMLFormElement;
  const settingsBrowse = document.getElementById("settings-browse")!;

  settingsBrowse.addEventListener("click", async () => {
    const folder = await selectFolder();
    if (folder) {
      (document.getElementById("settings-folder") as HTMLInputElement).value = folder;
    }
  });

  settingsForm.addEventListener("submit", async (e) => {
    e.preventDefault();

    if (!config) return;

    const newRole = (document.querySelector('input[name="settings-role"]:checked') as HTMLInputElement).value;
    const newPort = parseInt((document.getElementById("settings-port") as HTMLInputElement).value, 10) || 45678;
    const roleChanged = newRole !== config.role;
    const portChanged = newPort !== config.port;

    config.role = newRole;
    config.name = (document.getElementById("settings-name") as HTMLInputElement).value.trim();
    config.base_path = (document.getElementById("settings-folder") as HTMLInputElement).value;
    config.folder_template = (document.getElementById("settings-template") as HTMLSelectElement).value;
    config.use_day_folders = (document.getElementById("settings-day-folders") as HTMLInputElement).checked;
    config.reset_numbering_daily = (document.getElementById("settings-reset-daily") as HTMLInputElement).checked;
    config.port = newPort;

    try {
      await invoke("save_config", { config });

      // Dacă portul sau rolul s-au schimbat, restartează serverul automat
      if (roleChanged || portChanged) {
        await invoke("stop_server");
        await new Promise((resolve) => setTimeout(resolve, 500));
        await startServer();
        showToast(`Setări salvate - Server restartat pe portul ${config.port}`, "success");
      } else {
        showToast("Setări salvate", "success");
      }
      updateUIForRole();
    } catch (e) {
      showToast(`Eroare: ${e}`, "error");
    }
  });

  // Restart button
  const btnRestart = document.getElementById("btn-restart")!;
  btnRestart.addEventListener("click", async () => {
    try {
      await invoke("stop_server");
      // Small delay to ensure server stops
      await new Promise((resolve) => setTimeout(resolve, 500));
      await startServer();
      showToast("Server restartat cu succes", "success");
    } catch (e) {
      showToast(`Eroare restart: ${e}`, "error");
    }
  });

  // Day selector
  const currentDayInput = document.getElementById("current-day") as HTMLInputElement;
  const dayPrev = document.getElementById("day-prev")!;
  const dayNext = document.getElementById("day-next")!;

  currentDayInput.addEventListener("change", async () => {
    if (!config) return;
    config.current_day = currentDayInput.value;
    try {
      await invoke("save_config", { config });
      await loadDayCounter(); // Reload counter for new day
    } catch (e) {
      console.error("Error saving day:", e);
    }
  });

  // Day counter input
  const dayCounterInput = document.getElementById("day-counter") as HTMLInputElement;
  dayCounterInput.addEventListener("change", async () => {
    if (!config) return;
    const value = parseInt(dayCounterInput.value, 10);
    if (isNaN(value) || value < 1) {
      dayCounterInput.value = "1";
      return;
    }
    try {
      await invoke("set_day_counter", { day: config.current_day, value });
      showToast(`Următorul folder va fi ${value.toString().padStart(2, '0')}`, "success");
    } catch (e) {
      console.error("Error saving counter:", e);
      showToast(`Eroare: ${e}`, "error");
    }
  });

  dayPrev.addEventListener("click", async () => {
    if (!config) return;
    const { prefix, num } = extractDayParts(config.current_day);
    if (num > 1) {
      config.current_day = `${prefix}${num - 1}`;
      currentDayInput.value = config.current_day;
      try {
        await invoke("save_config", { config });
        await loadDayCounter();
      } catch (e) {
        console.error("Error saving day:", e);
      }
    }
  });

  dayNext.addEventListener("click", async () => {
    if (!config) return;
    const { prefix, num } = extractDayParts(config.current_day);
    config.current_day = `${prefix}${num + 1}`;
    currentDayInput.value = config.current_day;
    try {
      await invoke("save_config", { config });
      await loadDayCounter();
    } catch (e) {
      console.error("Error saving day:", e);
    }
  });

  // Tabs
  document.querySelectorAll(".tab").forEach((tab) => {
    tab.addEventListener("click", () => {
      const tabName = tab.getAttribute("data-tab")!;
      switchTab(tabName);
    });
  });

  // Server status click to start/stop
  const serverStatus = document.getElementById("server-status")!;
  serverStatus.addEventListener("click", async () => {
    if (!config || !config.name || !config.base_path) return;

    const isOnline = statusIndicator.classList.contains("online");
    if (!isOnline) {
      await startServer();
    }
  });

  // Send tab functionality
  setupSendFunctionality();

  // Modal cancel button
  const modalCancel = document.getElementById("modal-cancel")!;
  modalCancel.addEventListener("click", () => {
    document.getElementById("editor-select-modal")!.style.display = "none";
    pendingFiles = [];
  });

  // Check for updates button
  const btnCheckUpdate = document.getElementById("btn-check-update")!;
  btnCheckUpdate.addEventListener("click", checkForUpdates);
}

async function setupTauriListeners() {
  await listen("server-started", () => {
    statusIndicator.classList.add("online");
    statusText.textContent = "Online";
  });

  await listen("server-stopped", () => {
    statusIndicator.classList.remove("online");
    statusText.textContent = "Oprit";
  });

  await listen<{transfer_id: string, photographer: string}>("transfer-started", (event) => {
    const { photographer } = event.payload;
    showToast(`Transfer de la ${photographer}...`, "success");
    document.getElementById("transfers-empty")!.style.display = "none";
  });

  await listen<TransferProgress>("transfer-progress", (event) => {
    const p = event.payload;
    // Folosește transfer_id ca cheie pentru a suporta transferuri simultane
    activeTransfers.set(p.transfer_id, p);
    updateTransfersUI();
  });

  await listen<TransferRecord>("transfer-complete", async (event) => {
    const record = event.payload;
    // Folosește transfer_id pentru a șterge transferul corect
    activeTransfers.delete(record.transfer_id);
    updateTransfersUI();
    showToast(`Transfer complet: ${record.file_count} fisiere de la ${record.photographer}`, "success");
    loadHistory();
    loadDayCounter(); // Actualizează counter-ul după transfer

    // Notificare cu sunet integrat (rămâne mai mult în Notification Center)
    const isFromPhotographer = !record.source_role || record.source_role === null;
    const sound = isFromPhotographer ? "Glass" : "Ping";
    await showOSNotification(
      `Transfer de la ${record.photographer}`,
      `${record.file_count} fișiere primite`,
      sound
    );
  });

  await listen<string>("transfer-error", async (event) => {
    // La eroare, curăță toate transferurile active (nu știm care a eșuat)
    // În viitor am putea trimite transfer_id pentru a fi mai specific
    console.error("Transfer error:", event.payload);
    showToast(`Eroare transfer: ${event.payload}`, "error");

    // Notificare cu sunet eroare
    await showOSNotification("Transfer eșuat", event.payload, "Basso");
  });

  await listen<TransferRecord>("transfer-partial", async (event) => {
    const record = event.payload;
    activeTransfers.delete(record.transfer_id);
    updateTransfersUI();
    showToast(`Transfer întrerupt: ${record.file_count} fișiere salvate de la ${record.photographer}`, "error");
    loadHistory();

    // Notificare cu sunet eroare
    await showOSNotification(
      "Transfer întrerupt",
      `${record.file_count} fișiere salvate de la ${record.photographer}`,
      "Basso"
    );
  });

  await listen<TransferRecord>("transfer-cancelled", (event) => {
    const record = event.payload;
    activeTransfers.delete(record.transfer_id);
    updateTransfersUI();
    showToast(`Transfer anulat: ${record.file_count} fișiere salvate de la ${record.photographer}`, "error");
    loadHistory();
  });

  // Send listeners
  await setupSendListeners();
}

function showMainContent() {
  setupScreen.style.display = "none";
  mainContent.style.display = "flex";

  if (config) {
    // Populate settings
    (document.getElementById("settings-name") as HTMLInputElement).value = config.name;
    (document.getElementById("settings-folder") as HTMLInputElement).value = config.base_path;
    (document.getElementById("settings-template") as HTMLSelectElement).value = config.folder_template;
    (document.getElementById("settings-day-folders") as HTMLInputElement).checked = config.use_day_folders;
    (document.getElementById("settings-reset-daily") as HTMLInputElement).checked = config.reset_numbering_daily;
    (document.getElementById("current-day") as HTMLInputElement).value = config.current_day;
    (document.getElementById("settings-port") as HTMLInputElement).value = (config.port || 45678).toString();

    // Set role radio button
    if (config.role === "tagger") {
      (document.getElementById("settings-role-tagger") as HTMLInputElement).checked = true;
    } else {
      (document.getElementById("settings-role-editor") as HTMLInputElement).checked = true;
    }

    updateUIForRole();
    loadHistory();
    loadSentHistory();
  }
}

function updateUIForRole() {
  if (!config) return;

  if (config.role === "tagger") {
    daySelector.style.display = "flex";
    taggerSettings.style.display = "block";
    // Tagger poate trimite către editori
    tabSendBtn.style.display = "block";
    startDiscovery();
    loadDayCounter(); // Load counter for current day
  } else if (config.role === "editor") {
    daySelector.style.display = "none";
    taggerSettings.style.display = "none";
    // Editor poate trimite către alți editori
    tabSendBtn.style.display = "block";
    startDiscovery();
  } else {
    daySelector.style.display = "none";
    taggerSettings.style.display = "none";
    tabSendBtn.style.display = "none";
  }
}

async function startServer() {
  try {
    await invoke("start_server");
  } catch (e) {
    showToast(`Eroare pornire server: ${e}`, "error");
  }
}

async function selectFolder(): Promise<string | null> {
  try {
    const selected = await open({
      directory: true,
      multiple: false,
    });
    return selected as string | null;
  } catch (e) {
    console.error("Error selecting folder:", e);
    return null;
  }
}

function switchTab(tabName: string) {
  // Update tab buttons
  document.querySelectorAll(".tab").forEach((t) => {
    t.classList.toggle("active", t.getAttribute("data-tab") === tabName);
  });

  // Update tab content
  document.querySelectorAll(".tab-content").forEach((c) => {
    c.classList.toggle("active", c.id === `tab-${tabName}`);
  });

  if (tabName === "history") {
    loadHistory();
  }
}

function updateTransfersUI() {
  const list = document.getElementById("transfers-list")!;
  const empty = document.getElementById("transfers-empty")!;

  if (activeTransfers.size === 0) {
    empty.style.display = "flex";
    list.innerHTML = "";
    return;
  }

  empty.style.display = "none";
  list.innerHTML = "";

  activeTransfers.forEach((p, transferId) => {
    const percent = (p.bytes_received / p.total_bytes) * 100;
    const item = document.createElement("div");
    item.className = "transfer-item";
    item.id = `transfer-${transferId}`;
    item.innerHTML = `
      <div class="transfer-header">
        <span class="transfer-name">${p.photographer}</span>
        <span class="transfer-stats">${p.file_index + 1}/${p.total_files} - ${p.speed_mbps.toFixed(1)} MB/s</span>
      </div>
      <div class="progress-bar">
        <div class="progress-bar-fill" style="width: ${percent}%"></div>
      </div>
      <div class="transfer-footer">
        <span class="transfer-file">${p.file_name}</span>
        <button class="btn-cancel-transfer" data-transfer-id="${transferId}">Anuleaza</button>
      </div>
    `;

    // Add cancel button listener
    const cancelBtn = item.querySelector('.btn-cancel-transfer')!;
    cancelBtn.addEventListener('click', async () => {
      try {
        await invoke("cancel_current_transfer");
        activeTransfers.delete(transferId);
        updateTransfersUI();
        showToast(`Transfer de la ${p.photographer} anulat`, "error");
      } catch (e) {
        console.error("Error cancelling transfer:", e);
      }
    });

    list.appendChild(item);
  });
}

// Actualizează UI-ul pentru trimiteri simultane
function updateSendsUI() {
  activeSendsContainer.innerHTML = "";

  activeSends.forEach((p, sendId) => {
    const percent = (p.bytes_sent / p.total_bytes) * 100;
    const item = document.createElement("div");
    item.className = "send-progress";
    item.id = `send-${sendId}`;
    item.innerHTML = `
      <div class="send-progress-header">
        <span class="send-progress-target">${p.target_name}</span>
        <span class="send-progress-stats">${p.file_index + 1}/${p.total_files} - ${p.speed_mbps.toFixed(1)} MB/s</span>
      </div>
      <div class="progress-bar">
        <div class="progress-bar-fill" style="width: ${percent}%"></div>
      </div>
      <div class="send-progress-footer">
        <p class="send-progress-file">${p.file_name}</p>
        <button type="button" class="btn btn-cancel btn-cancel-send" data-send-id="${sendId}">Anuleaza</button>
      </div>
    `;

    // Add cancel button listener
    const cancelBtn = item.querySelector('.btn-cancel-send')!;
    cancelBtn.addEventListener('click', async () => {
      try {
        await invoke("cancel_send_transfer");
        activeSends.delete(sendId);
        updateSendsUI();
        showToast(`Trimitere către ${p.target_name} anulată`, "error");
      } catch (e) {
        console.error("Error cancelling send:", e);
      }
    });

    activeSendsContainer.appendChild(item);
  });
}

async function loadHistory() {
  try {
    // Sincronizează istoricul cu fișierele reale de pe disc
    await invoke("sync_history_from_disk");
    const history = await invoke<TransferRecord[]>("get_history");
    const list = document.getElementById("history-list")!;
    const statsTotal = document.getElementById("stats-total")!;
    const statsByDay = document.getElementById("stats-by-day")!;
    const statsByPhotographer = document.getElementById("stats-by-photographer")!;

    if (history.length === 0) {
      list.innerHTML = '<p class="history-empty">Niciun transfer inca</p>';
      statsTotal.innerHTML = '<p class="stat-row"><span class="stat-label">Nicio inregistrare</span></p>';
      statsByDay.innerHTML = '';
      statsByPhotographer.innerHTML = '';
      return;
    }

    // Calculate statistics
    let totalFiles = 0;
    let totalSize = 0;
    const byDay: Map<string, { files: number; size: number; photographers: Set<string> }> = new Map();
    const byPhotographer: Map<string, { files: number; size: number }> = new Map();
    const byPhotographerByDay: Map<string, Map<string, { files: number; size: number }>> = new Map();

    history.forEach((record) => {
      const day = record.day || "Fara zi";

      // Total
      totalFiles += record.file_count;
      totalSize += record.total_size;

      // By day
      if (!byDay.has(day)) {
        byDay.set(day, { files: 0, size: 0, photographers: new Set() });
      }
      const dayStats = byDay.get(day)!;
      dayStats.files += record.file_count;
      dayStats.size += record.total_size;
      dayStats.photographers.add(record.photographer);

      // By photographer
      if (!byPhotographer.has(record.photographer)) {
        byPhotographer.set(record.photographer, { files: 0, size: 0 });
      }
      const photoStats = byPhotographer.get(record.photographer)!;
      photoStats.files += record.file_count;
      photoStats.size += record.total_size;

      // By photographer by day
      if (!byPhotographerByDay.has(record.photographer)) {
        byPhotographerByDay.set(record.photographer, new Map());
      }
      const photoDays = byPhotographerByDay.get(record.photographer)!;
      if (!photoDays.has(day)) {
        photoDays.set(day, { files: 0, size: 0 });
      }
      const photoDayStats = photoDays.get(day)!;
      photoDayStats.files += record.file_count;
      photoDayStats.size += record.total_size;
    });

    // Render total stats
    statsTotal.innerHTML = `
      <div class="stat-row">
        <span class="stat-label">Total poze</span>
        <span class="stat-value highlight">${totalFiles}</span>
      </div>
      <div class="stat-row">
        <span class="stat-label">Total marime</span>
        <span class="stat-value">${formatSize(totalSize)}</span>
      </div>
      <div class="stat-row">
        <span class="stat-label">Transferuri</span>
        <span class="stat-value">${history.length}</span>
      </div>
    `;

    // Render stats by day (sorted naturally: DAY 1, DAY 2, etc.)
    const sortedDays = Array.from(byDay.entries()).sort((a, b) => {
      const numA = extractDayParts(a[0]).num;
      const numB = extractDayParts(b[0]).num;
      return numA - numB;
    });

    statsByDay.innerHTML = sortedDays.map(([day, stats]) => `
      <div class="stat-row day-stat-row" data-day="${day}">
        <span class="stat-label">${day}</span>
        <span class="stat-value">${stats.files} poze (${stats.photographers.size} fotografi)</span>
      </div>
    `).join('');

    // Adaugă click handler pentru reset pe ziua curentă (triple-click)
    statsByDay.querySelectorAll('.day-stat-row').forEach((row) => {
      row.addEventListener('click', () => handleDayResetClick(row.getAttribute('data-day')!));
    });

    // Render stats by photographer
    const sortedPhotographers = Array.from(byPhotographer.entries()).sort((a, b) => b[1].files - a[1].files);

    statsByPhotographer.innerHTML = sortedPhotographers.map(([name, stats]) => {
      const photoDays = byPhotographerByDay.get(name)!;
      const daysDetail = Array.from(photoDays.entries())
        .sort((a, b) => extractDayParts(a[0]).num - extractDayParts(b[0]).num)
        .map(([day, dayStats]) => `${day}: ${dayStats.files}`)
        .join(', ');

      return `
        <div class="stat-row">
          <span class="stat-label">${name}</span>
          <span class="stat-value">${stats.files} poze</span>
        </div>
        <div class="stat-row" style="margin-left: 12px; font-size: 11px;">
          <span class="stat-label">${daysDetail}</span>
        </div>
      `;
    }).join('');

    // Group history by day
    const groupedByDay: Map<string, TransferRecord[]> = new Map();
    history.forEach((record) => {
      const day = record.day || "Fara zi";
      if (!groupedByDay.has(day)) {
        groupedByDay.set(day, []);
      }
      groupedByDay.get(day)!.push(record);
    });

    // Sort days naturally and render
    const sortedDayGroups = Array.from(groupedByDay.entries()).sort((a, b) => {
      const numA = extractDayParts(a[0]).num;
      const numB = extractDayParts(b[0]).num;
      return numB - numA; // Most recent day first
    });

    list.innerHTML = "";
    sortedDayGroups.forEach(([day, records]) => {
      const dayTotalFiles = records.reduce((sum, r) => sum + r.file_count, 0);
      const dayTotalSize = records.reduce((sum, r) => sum + r.total_size, 0);
      const photographers = new Set(records.map(r => r.photographer)).size;

      // Sort records within day by timestamp (newest first)
      records.sort((a, b) => new Date(b.timestamp).getTime() - new Date(a.timestamp).getTime());

      const section = document.createElement("div");
      section.className = "history-day-section";
      section.innerHTML = `
        <div class="history-day-header">
          <span class="history-day-title">${day}</span>
          <span class="history-day-stats">${dayTotalFiles} poze | ${formatSize(dayTotalSize)} | ${photographers} fotografi</span>
        </div>
        <div class="history-day-items">
          ${records.map((record) => {
            const date = new Date(record.timestamp);
            const timeStr = date.toLocaleTimeString("ro-RO", { hour: "2-digit", minute: "2-digit" });
            const dateStr = date.toLocaleDateString("ro-RO");
            return `
              <div class="history-item">
                <div class="history-info">
                  <span class="history-name">${record.photographer}</span>
                  <span class="history-folder">${record.folder}</span>
                  <span class="history-details">${record.file_count} fisiere - ${formatSize(record.total_size)}</span>
                </div>
                <div class="history-right">
                  <div class="history-time">${timeStr}</div>
                  <div class="history-details">${dateStr}</div>
                </div>
              </div>
            `;
          }).join('')}
        </div>
      `;
      list.appendChild(section);
    });
  } catch (e) {
    console.error("Error loading history:", e);
  }
}

function extractDayParts(day: string): { prefix: string; num: number } {
  const match = day.match(/^(.*?)(\d+)\s*$/);
  if (match) {
    return { prefix: match[1], num: parseInt(match[2], 10) };
  }
  return { prefix: "DAY ", num: 1 };
}

async function loadDayCounter() {
  if (!config) return;
  try {
    const counter = await invoke<number>("get_day_counter", { day: config.current_day });
    const dayCounterInput = document.getElementById("day-counter") as HTMLInputElement;
    dayCounterInput.value = counter.toString();
  } catch (e) {
    console.error("Error loading day counter:", e);
  }
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

function showToast(message: string, type: "success" | "error") {
  toastMessage.textContent = message;
  toast.className = `toast ${type} show`;

  setTimeout(() => {
    toast.classList.remove("show");
  }, 3000);
}

// ==================== NOTIFICATIONS & SOUNDS ====================

async function showOSNotification(title: string, body: string, sound?: string) {
  // Folosește comanda Rust cu osascript pentru notificări native macOS
  // sound: "Glass", "Ping", "Hero", "Basso", etc.
  invoke("show_notification", { title, body, sound: sound || null }).catch((e) => {
    console.error("Notification error:", e);
  });
}

// Reset history pentru o zi specifică - necesită triple-click + confirmare
function handleDayResetClick(day: string) {
  resetClickCount++;

  if (resetClickTimer) {
    clearTimeout(resetClickTimer);
  }

  if (resetClickCount === 3) {
    // Triple-click detectat - arată confirmare
    resetClickCount = 0;
    const dayToReset = day; // Captură valoarea

    // setTimeout pentru a permite UI-ului să se stabilizeze
    setTimeout(() => {
      if (confirm(`Ești sigur că vrei să ștergi istoricul pentru ${dayToReset}?\n\nAceastă acțiune nu poate fi anulată!`)) {
        clearDayHistory(dayToReset);
      }
    }, 100);
  } else {
    // Reset counter după 500ms
    resetClickTimer = setTimeout(() => {
      resetClickCount = 0;
    }, 500) as unknown as number;
  }
}

async function clearDayHistory(day: string) {
  try {
    await invoke("clear_history", { day });
    showToast(`Istoricul pentru ${day} a fost șters`, "success");
    loadHistory();
  } catch (e) {
    showToast(`Eroare: ${e}`, "error");
  }
}

// Verifică foldere temporare la pornirea aplicației
async function checkTempFolders() {
  try {
    const tempFolders = await invoke<TempFolderInfo[]>("get_temp_folders");

    if (tempFolders.length === 0) {
      return; // Nu există foldere temporare
    }

    // Afișează dialog pentru fiecare folder temporar
    const modal = document.createElement("div");
    modal.className = "temp-folders-modal";
    modal.innerHTML = `
      <div class="temp-folders-content">
        <h3>Transferuri întrerupte găsite</h3>
        <p>S-au găsit ${tempFolders.length} folder(e) cu transferuri nefinalizate:</p>
        <div class="temp-folders-list">
          ${tempFolders.map((folder, index) => `
            <div class="temp-folder-item" data-path="${folder.path}" data-index="${index}">
              <div class="temp-folder-info">
                <span class="temp-folder-photographer">${folder.photographer}</span>
                <span class="temp-folder-details">${folder.file_count} fișiere - ${formatSize(folder.total_size)}</span>
                ${folder.day ? `<span class="temp-folder-day">${folder.day}</span>` : ''}
              </div>
              <div class="temp-folder-actions">
                <button class="btn-temp-keep" data-path="${folder.path}" title="Păstrează pentru reluare">
                  Păstrează
                </button>
                <button class="btn-temp-delete" data-path="${folder.path}" title="Șterge folderul">
                  Șterge
                </button>
              </div>
            </div>
          `).join('')}
        </div>
        <div class="temp-folders-footer">
          <button class="btn-temp-close">Închide</button>
        </div>
      </div>
    `;

    document.body.appendChild(modal);

    // Handler pentru butonul de păstrare
    modal.querySelectorAll(".btn-temp-keep").forEach((btn) => {
      btn.addEventListener("click", () => {
        const item = btn.closest(".temp-folder-item");
        if (item) {
          item.classList.add("temp-folder-kept");
          showToast("Folderul va fi reluat la următorul transfer", "success");
        }
      });
    });

    // Handler pentru butonul de ștergere
    modal.querySelectorAll(".btn-temp-delete").forEach((btn) => {
      btn.addEventListener("click", async () => {
        const path = btn.getAttribute("data-path");
        if (path && confirm("Ești sigur că vrei să ștergi acest folder?\n\nFișierele vor fi pierdute definitiv!")) {
          try {
            await invoke("delete_temp_folder", { path });
            const item = btn.closest(".temp-folder-item");
            if (item) {
              item.remove();
            }
            showToast("Folder șters cu succes", "success");

            // Dacă nu mai sunt foldere, închide modalul
            if (modal.querySelectorAll(".temp-folder-item").length === 0) {
              modal.remove();
            }
          } catch (e) {
            showToast(`Eroare: ${e}`, "error");
          }
        }
      });
    });

    // Handler pentru închidere
    modal.querySelector(".btn-temp-close")?.addEventListener("click", () => {
      modal.remove();
    });

    // Închide la click pe fundal
    modal.addEventListener("click", (e) => {
      if (e.target === modal) {
        modal.remove();
      }
    });

  } catch (e) {
    console.error("Error checking temp folders:", e);
  }
}

// ==================== SEND FUNCTIONALITY ====================

function setupSendFunctionality() {
  // Drop zone click to select files
  sendDropZone.addEventListener("click", async () => {
    if (sendDropZone.classList.contains("disabled")) return;

    const files = await open({
      multiple: true,
      filters: [
        {
          name: "Media",
          extensions: [
            "jpg",
            "jpeg",
            "png",
            "cr2",
            "cr3",
            "nef",
            "arw",
            "raw",
            "dng",
            "mp4",
            "mov",
            "avi",
          ],
        },
      ],
    });

    if (files && files.length > 0) {
      const paths = Array.isArray(files) ? files : [files];
      await sendFilesToEditor(paths as string[]);
    }
  });

  // Drop zone drag and drop
  sendDropZone.addEventListener("dragover", (e) => {
    e.preventDefault();
    if (!sendDropZone.classList.contains("disabled")) {
      sendDropZone.classList.add("active");
    }
  });

  sendDropZone.addEventListener("dragleave", () => {
    sendDropZone.classList.remove("active");
  });

  sendDropZone.addEventListener("drop", async (e) => {
    e.preventDefault();
    sendDropZone.classList.remove("active");

    if (sendDropZone.classList.contains("disabled")) return;

    // Note: Tauri handles file drops differently - we need to use the file drop event
    showToast("Foloseste click pentru a selecta fisiere", "error");
  });

  // Setup Tauri file drop listener
  setupFileDrop();
}

async function setupFileDrop() {
  const webviewWindow = getCurrentWebviewWindow();

  await webviewWindow.onDragDropEvent(async (event) => {
    if (event.payload.type === "drop") {
      const paths = event.payload.paths;
      if (paths.length > 0 && selectedEditor) {
        await sendFilesToEditor(paths);
      }
    }
  });
}

async function startDiscovery() {
  try {
    await invoke("start_discovery");
    // Start polling for editors
    if (discoveryInterval) {
      clearInterval(discoveryInterval);
    }
    discoveryInterval = setInterval(refreshEditors, 2000) as unknown as number;
    await refreshEditors();
  } catch (e) {
    console.error("Error starting discovery:", e);
  }
}

async function refreshEditors() {
  try {
    discoveredEditors = await invoke<DiscoveredEditor[]>("get_editors");
    updateEditorsUI();
  } catch (e) {
    console.error("Error refreshing editors:", e);
  }
}

function updateEditorsUI() {
  const noEditors = document.getElementById("no-editors")!;

  if (discoveredEditors.length === 0) {
    noEditors.style.display = "block";
    noEditors.textContent = "Niciun editor online";
    editorsList.innerHTML = "";
    editorsList.appendChild(noEditors);
    sendDropZone.classList.add("disabled");
    selectedEditor = null;
    return;
  }

  noEditors.style.display = "none";
  editorsList.innerHTML = "";

  discoveredEditors.forEach((editor) => {
    const item = document.createElement("div");
    item.className = "editor-item";
    if (selectedEditor && selectedEditor.name === editor.name) {
      item.classList.add("selected");
    }
    const roleLabel = editor.role === "editor" ? "Editor" : editor.role === "tagger" ? "Tagger" : editor.role;
    item.innerHTML = `
      <div class="editor-info">
        <span class="editor-name">${editor.name}</span>
        <span class="editor-ip">${editor.host}:${editor.port} (${roleLabel})</span>
      </div>
      <span class="editor-status"></span>
    `;
    item.addEventListener("click", () => {
      selectEditor(editor);
    });
    editorsList.appendChild(item);
  });

  // If only one editor, auto-select it
  if (discoveredEditors.length === 1 && !selectedEditor) {
    selectEditor(discoveredEditors[0]);
  }
}

function selectEditor(editor: DiscoveredEditor) {
  selectedEditor = editor;

  // Update UI
  document.querySelectorAll(".editor-item").forEach((el) => {
    el.classList.remove("selected");
  });

  const items = editorsList.querySelectorAll(".editor-item");
  items.forEach((item) => {
    if (item.querySelector(".editor-name")?.textContent === editor.name) {
      item.classList.add("selected");
    }
  });

  // Enable drop zone
  sendDropZone.classList.remove("disabled");
  const dropText = sendDropZone.querySelector(".drop-zone-text")!;
  dropText.textContent = `Trimite catre ${editor.name}`;
}

async function sendFilesToEditor(paths: string[]) {
  if (!selectedEditor) {
    // If multiple editors, show selection modal
    if (discoveredEditors.length > 1) {
      pendingFiles = paths;
      showEditorSelectionModal();
      return;
    } else if (discoveredEditors.length === 1) {
      selectedEditor = discoveredEditors[0];
    } else {
      showToast("Niciun editor disponibil", "error");
      return;
    }
  }

  const editor = selectedEditor;

  // Separă path-urile în foldere și fișiere individuale
  const folders: string[] = [];
  const files: string[] = [];

  for (const path of paths) {
    const lastPart = path.split(/[/\\]/).pop() || "";
    // Dacă ultimul segment nu are extensie de fișier comun, presupunem că e folder
    if (lastPart && !lastPart.match(/\.(jpg|jpeg|png|gif|raw|cr2|cr3|nef|arw|dng|tif|tiff|psd|mp4|mov|avi|mkv|heic|heif|webp)$/i)) {
      folders.push(path);
    } else {
      files.push(path);
    }
  }

  try {
    // Dacă avem mai multe foldere, trimite-le în paralel (fiecare are propriul send_id)
    if (folders.length > 1) {
      showToast(`Se trimit ${folders.length} foldere...`, "success");

      // Lansează toate transferurile în paralel
      const promises = folders.map(folder => {
        const folderName = folder.split(/[/\\]/).pop() || null;
        return invoke<string>("send_to_editor", {
          targetHost: editor.host,
          targetPort: editor.port,
          targetName: editor.name,
          filePaths: [folder],
          folderName: folderName,
        });
      });

      // Trimite și fișierele individuale în paralel (dacă există)
      if (files.length > 0) {
        promises.push(invoke<string>("send_to_editor", {
          targetHost: editor.host,
          targetPort: editor.port,
          targetName: editor.name,
          filePaths: files,
          folderName: null,
        }));
      }

      // Așteaptă toate transferurile (erorile sunt gestionate individual prin events)
      await Promise.allSettled(promises);
    } else {
      // Caz normal: un singur folder sau fișiere
      let folderName: string | null = null;
      if (folders.length === 1) {
        folderName = folders[0].split(/[/\\]/).pop() || null;
      }

      await invoke<string>("send_to_editor", {
        targetHost: editor.host,
        targetPort: editor.port,
        targetName: editor.name,
        filePaths: paths,
        folderName: folderName,
      });
    }
  } catch (e) {
    // Eroarea este deja gestionată prin send-error event
    console.error("Send error:", e);
  }
}

function showEditorSelectionModal() {
  const modal = document.getElementById("editor-select-modal")!;
  const list = document.getElementById("modal-editor-list")!;

  list.innerHTML = "";
  discoveredEditors.forEach((editor) => {
    const btn = document.createElement("button");
    btn.className = "modal-editor-btn";
    btn.innerHTML = `
      <span class="editor-name">${editor.name}</span>
      <span class="editor-ip">${editor.host}</span>
    `;
    btn.addEventListener("click", async () => {
      modal.style.display = "none";
      selectedEditor = editor;
      await sendFilesToEditor(pendingFiles);
      pendingFiles = [];
    });
    list.appendChild(btn);
  });

  modal.style.display = "flex";
}

async function setupSendListeners() {
  // Listener pentru start transfer (adaugă în Map)
  await listen<SendResult>("send-started", (event) => {
    const { send_id, target_name, file_count } = event.payload;
    // Inițializează cu valori placeholder
    activeSends.set(send_id, {
      send_id,
      file_name: "Se conectează...",
      file_index: 0,
      total_files: file_count,
      bytes_sent: 0,
      total_bytes: 1,
      speed_mbps: 0,
      target_name,
    });
    updateSendsUI();
  });

  // Listener pentru progres transfer
  await listen<SendProgress>("send-progress", (event) => {
    const p = event.payload;
    activeSends.set(p.send_id, p);
    updateSendsUI();
  });

  // Listener pentru transfer complet
  await listen<SendResult>("send-complete", async (event) => {
    const { send_id, target_name, file_count } = event.payload;
    activeSends.delete(send_id);
    updateSendsUI();
    showToast(`Transfer complet: ${file_count} fișiere trimise către ${target_name}`, "success");
    loadSentHistory();

    // Notificare cu sunet pentru transfer trimis
    await showOSNotification(
      "Transfer trimis",
      `${file_count} fișiere trimise către ${target_name}`,
      "Hero"
    );
  });

  // Listener pentru erori la trimitere
  await listen<SendError>("send-error", async (event) => {
    const { send_id, target_name, error } = event.payload;
    console.error("Send error:", error);
    activeSends.delete(send_id);
    updateSendsUI();
    showToast(`Eroare trimitere către ${target_name}: ${error}`, "error");
    await showOSNotification("Trimitere eșuată", `${target_name}: ${error}`, "Basso");
  });

  // Listener pentru anulare trimitere
  await listen<SendResult>("send-cancelled", async (event) => {
    const { send_id, target_name } = event.payload;
    activeSends.delete(send_id);
    updateSendsUI();
    showToast(`Trimitere către ${target_name} anulată`, "error");
  });
}

// ==================== ISTORIC TRIMITERI ====================

async function loadSentHistory() {
  try {
    const sentHistory = await invoke<SentRecord[]>("get_sent_history");
    const container = document.getElementById("sent-history-list");
    if (!container) return;

    if (sentHistory.length === 0) {
      container.innerHTML = '<div class="empty-state">Nicio trimitere încă</div>';
      return;
    }

    // Sortează după timestamp descrescător (cele mai recente primele)
    const sorted = [...sentHistory].sort((a, b) =>
      new Date(b.timestamp).getTime() - new Date(a.timestamp).getTime()
    );

    container.innerHTML = sorted.slice(0, 50).map(record => {
      const date = new Date(record.timestamp);
      const timeStr = date.toLocaleTimeString("ro-RO", { hour: "2-digit", minute: "2-digit" });
      const dateStr = date.toLocaleDateString("ro-RO", { day: "2-digit", month: "short" });

      return `
        <div class="sent-item">
          <div class="sent-header">
            <span class="sent-target">→ ${record.target_name}</span>
            <span class="sent-time">${dateStr} ${timeStr}</span>
          </div>
          <div class="sent-details">
            <span>${record.file_count} fișiere</span>
            <span>${formatSize(record.total_size)}</span>
            ${record.folder_name ? `<span class="sent-folder">${record.folder_name}</span>` : ''}
          </div>
        </div>
      `;
    }).join('');
  } catch (e) {
    console.error("Error loading sent history:", e);
  }
}

// ==================== AUTO-UPDATE ====================

async function checkForUpdates() {
  const updateStatus = document.getElementById("update-status")!;
  const btnCheckUpdate = document.getElementById("btn-check-update")!;

  try {
    btnCheckUpdate.textContent = "Se verifică...";
    btnCheckUpdate.setAttribute("disabled", "true");
    updateStatus.textContent = "";
    updateStatus.className = "hint";

    const update = await check();

    if (update) {
      updateStatus.textContent = `Versiune nouă disponibilă: ${update.version}`;
      updateStatus.className = "hint success";

      if (confirm(`Este disponibilă versiunea ${update.version}. Vrei să actualizezi acum?`)) {
        updateStatus.textContent = "Se descarcă actualizarea...";

        let totalSize = 0;
        let downloaded = 0;
        await update.downloadAndInstall((progress) => {
          if (progress.event === "Started") {
            totalSize = (progress.data as any).contentLength || 0;
            updateStatus.textContent = `Se descarcă... 0%`;
          } else if (progress.event === "Progress") {
            downloaded += (progress.data as any).chunkLength || 0;
            const percent = totalSize > 0 ? Math.round((downloaded / totalSize) * 100) : 0;
            updateStatus.textContent = `Se descarcă... ${percent}%`;
          } else if (progress.event === "Finished") {
            updateStatus.textContent = "Se instalează...";
          }
        });

        updateStatus.textContent = "Actualizare completă! Se repornește...";
        await relaunch();
      }
    } else {
      updateStatus.textContent = "Ai ultima versiune instalată.";
      updateStatus.className = "hint success";
    }
  } catch (e) {
    console.error("Update check error:", e);
    updateStatus.textContent = `Eroare verificare: ${e}`;
    updateStatus.className = "hint error";
  } finally {
    btnCheckUpdate.textContent = "Verifică actualizări";
    btnCheckUpdate.removeAttribute("disabled");
  }
}

// Check for updates on startup (silent)
async function checkForUpdatesOnStartup() {
  try {
    const update = await check();
    if (update) {
      showToast(`Actualizare disponibilă: v${update.version}`, "success");
    }
  } catch (e) {
    console.error("Startup update check failed:", e);
  }
}
