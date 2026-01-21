import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";

interface ReceiverConfig {
  role: string;
  name: string;
  base_path: string;
  folder_template: string;
  folder_counter: number;
  use_day_folders: boolean;
  current_day: string;
  reset_numbering_daily: boolean;
}

interface TransferProgress {
  photographer: string;
  file_name: string;
  file_index: number;
  total_files: number;
  bytes_received: number;
  total_bytes: number;
  speed_mbps: number;
}

interface TransferRecord {
  timestamp: string;
  photographer: string;
  file_count: number;
  total_size: number;
  folder: string;
  day: string | null;
}

interface DiscoveredEditor {
  name: string;
  role: string;
  host: string;
  port: number;
}

interface SendProgress {
  file_name: string;
  file_index: number;
  total_files: number;
  bytes_sent: number;
  total_bytes: number;
  speed_mbps: number;
  target_name: string;
}

let config: ReceiverConfig | null = null;
let activeTransfers: Map<string, TransferProgress> = new Map();
let discoveredEditors: DiscoveredEditor[] = [];
let selectedEditor: DiscoveredEditor | null = null;
let pendingFiles: string[] = [];
let discoveryInterval: number | null = null;

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
let sendProgress: HTMLElement;

window.addEventListener("DOMContentLoaded", async () => {
  initElements();
  setupEventListeners();
  await setupTauriListeners(); // Must be before loadConfig to catch server-started event
  await loadConfig();
  await showLocalIP();
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
  sendProgress = document.getElementById("send-progress")!;
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
    const roleChanged = newRole !== config.role;

    config.role = newRole;
    config.name = (document.getElementById("settings-name") as HTMLInputElement).value.trim();
    config.base_path = (document.getElementById("settings-folder") as HTMLInputElement).value;
    config.folder_template = (document.getElementById("settings-template") as HTMLSelectElement).value;
    config.use_day_folders = (document.getElementById("settings-day-folders") as HTMLInputElement).checked;
    config.reset_numbering_daily = (document.getElementById("settings-reset-daily") as HTMLInputElement).checked;

    try {
      await invoke("save_config", { config });
      showToast(roleChanged ? "Setari salvate - Apasa Restart Server" : "Setari salvate", "success");
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
    } catch (e) {
      console.error("Error saving day:", e);
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

  await listen<string>("transfer-started", (event) => {
    const photographer = event.payload;
    showToast(`Transfer de la ${photographer}...`, "success");
    document.getElementById("transfers-empty")!.style.display = "none";
  });

  await listen<TransferProgress>("transfer-progress", (event) => {
    const p = event.payload;
    activeTransfers.set(p.photographer, p);
    updateTransfersUI();
  });

  await listen<TransferRecord>("transfer-complete", (event) => {
    const record = event.payload;
    activeTransfers.delete(record.photographer);
    updateTransfersUI();
    showToast(`Transfer complet: ${record.file_count} fisiere de la ${record.photographer}`, "success");
    loadHistory();
  });

  await listen<string>("transfer-error", (event) => {
    showToast(`Eroare transfer: ${event.payload}`, "error");
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

    // Set role radio button
    if (config.role === "tagger") {
      (document.getElementById("settings-role-tagger") as HTMLInputElement).checked = true;
    } else {
      (document.getElementById("settings-role-editor") as HTMLInputElement).checked = true;
    }

    updateUIForRole();
    loadHistory();
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

  activeTransfers.forEach((p) => {
    const percent = (p.bytes_received / p.total_bytes) * 100;
    const item = document.createElement("div");
    item.className = "transfer-item";
    item.innerHTML = `
      <div class="transfer-header">
        <span class="transfer-name">${p.photographer}</span>
        <span class="transfer-stats">${p.file_index + 1}/${p.total_files} - ${p.speed_mbps.toFixed(1)} MB/s</span>
      </div>
      <div class="progress-bar">
        <div class="progress-bar-fill" style="width: ${percent}%"></div>
      </div>
      <div class="transfer-stats">${p.file_name}</div>
    `;
    list.appendChild(item);
  });
}

async function loadHistory() {
  try {
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
      <div class="stat-row">
        <span class="stat-label">${day}</span>
        <span class="stat-value">${stats.files} poze (${stats.photographers.size} fotografi)</span>
      </div>
    `).join('');

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
    item.innerHTML = `
      <div class="editor-info">
        <span class="editor-name">${editor.name}</span>
        <span class="editor-ip">${editor.host}:${editor.port}</span>
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

  try {
    sendProgress.style.display = "block";
    document.getElementById("send-target-name")!.textContent = editor.name;

    await invoke("send_to_editor", {
      targetHost: editor.host,
      targetPort: editor.port,
      targetName: editor.name,
      filePaths: paths,
    });
  } catch (e) {
    showToast(`Eroare transfer: ${e}`, "error");
    sendProgress.style.display = "none";
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
  await listen<SendProgress>("send-progress", (event) => {
    const p = event.payload;
    const percent = (p.bytes_sent / p.total_bytes) * 100;

    document.getElementById("send-stats")!.textContent = `${p.file_index + 1}/${p.total_files} - ${p.speed_mbps.toFixed(1)} MB/s`;
    (document.getElementById("send-progress-bar") as HTMLElement).style.width = `${percent}%`;
    document.getElementById("send-file-name")!.textContent = p.file_name;
  });

  await listen<number>("send-complete", (event) => {
    sendProgress.style.display = "none";
    showToast(`Transfer complet: ${event.payload} fisiere trimise`, "success");
  });
}
