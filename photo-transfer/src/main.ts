import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";

interface DiscoveredService {
  name: string;
  role: string;
  host: string;
  port: number;
}

interface TransferProgress {
  file_name: string;
  file_index: number;
  total_files: number;
  bytes_sent: number;
  total_bytes: number;
  speed_mbps: number;
}

interface DuplicateInfo {
  file_name: string;
  existing_path: string;
  existing_size: number;
  new_size: number;
  same_checksum: boolean;
}

interface DuplicateCheckResult {
  duplicates: DuplicateInfo[];
  resume_folder: string | null;
  target_folder: string;
}

interface ChecksumProgress {
  current: number;
  total: number;
  file_name: string;
}

// State - now stores all services, keyed by unique identifier (name+host)
let allServices: DiscoveredService[] = [];
let pendingFiles: string[] = [];
let isTransferring = false;
let pendingDuplicateCheck: {
  receiver: DiscoveredService;
  paths: string[];
  result: DuplicateCheckResult;
} | null = null;

// DOM Elements
let photographerNameInput: HTMLInputElement;
let dropTagger: HTMLElement;
let dropEditor: HTMLElement;
let taggerStatus: HTMLElement;
let taggerStatusText: HTMLElement;
let editorStatus: HTMLElement;
let editorStatusText: HTMLElement;
let progressSection: HTMLElement;
let progressBar: HTMLElement;
let progressTitle: HTMLElement;
let progressStats: HTMLElement;
let progressFile: HTMLElement;
let progressSpeed: HTMLElement;
let btnCancel: HTMLElement;
let toast: HTMLElement;
let toastMessage: HTMLElement;

// Initialize
window.addEventListener("DOMContentLoaded", async () => {
  initElements();
  await loadConfig();
  setupEventListeners();
  setupTauriListeners();
  setupManualConnect();
  setupDuplicateModal();
  startServiceDiscovery();
  updateDropZonesState();
});

function initElements() {
  photographerNameInput = document.getElementById("photographer-name") as HTMLInputElement;
  dropTagger = document.getElementById("drop-tagger")!;
  dropEditor = document.getElementById("drop-editor")!;
  taggerStatus = document.getElementById("tagger-status")!;
  taggerStatusText = document.getElementById("tagger-status-text")!;
  editorStatus = document.getElementById("editor-status")!;
  editorStatusText = document.getElementById("editor-status-text")!;
  progressSection = document.getElementById("progress-section")!;
  progressBar = document.getElementById("progress-bar")!;
  progressTitle = document.getElementById("progress-title")!;
  progressStats = document.getElementById("progress-stats")!;
  progressFile = document.getElementById("progress-file")!;
  progressSpeed = document.getElementById("progress-speed")!;
  btnCancel = document.getElementById("btn-cancel-transfer")!;
  toast = document.getElementById("toast")!;
  toastMessage = document.getElementById("toast-message")!;
}

async function loadConfig() {
  try {
    const name = await invoke<string | null>("load_config");
    if (name) {
      photographerNameInput.value = name;
    }
  } catch (e) {
    console.error("Error loading config:", e);
  }
}

function hasValidName(): boolean {
  return photographerNameInput.value.trim().length > 0;
}

function updateDropZonesState() {
  const nameValid = hasValidName();

  if (!nameValid) {
    dropTagger.classList.add("disabled");
    dropEditor.classList.add("disabled");
    return;
  }

  updateServiceStatus();
}

function setupEventListeners() {
  // Save name on change and update UI
  photographerNameInput.addEventListener("input", () => {
    updateDropZonesState();
  });

  photographerNameInput.addEventListener("change", async () => {
    try {
      await invoke("save_config", { name: photographerNameInput.value });
    } catch (e) {
      console.error("Error saving config:", e);
    }
  });

  // Drop zones click handlers (for files)
  dropTagger.addEventListener("click", (e) => {
    // Don't trigger if clicking on the folder button
    if ((e.target as HTMLElement).classList.contains("btn-folder")) return;
    selectFiles("tagger");
  });
  dropEditor.addEventListener("click", (e) => {
    if ((e.target as HTMLElement).classList.contains("btn-folder")) return;
    selectFiles("editor");
  });

  // Folder buttons
  document.getElementById("btn-folder-tagger")!.addEventListener("click", (e) => {
    e.stopPropagation();
    selectFolder("tagger");
  });
  document.getElementById("btn-folder-editor")!.addEventListener("click", (e) => {
    e.stopPropagation();
    selectFolder("editor");
  });

  // Drag and drop visual feedback only (actual drop handled by Tauri)
  [dropTagger, dropEditor].forEach((zone) => {
    zone.addEventListener("dragover", (e) => {
      e.preventDefault();
      if (!zone.classList.contains("disabled")) {
        zone.classList.add("drag-over");
      }
    });

    zone.addEventListener("dragleave", () => {
      zone.classList.remove("drag-over");
    });

    zone.addEventListener("drop", (e) => {
      e.preventDefault();
      zone.classList.remove("drag-over");
    });
  });

  // Setup Tauri drag-drop listener
  setupTauriDragDrop();
}

let currentDropTarget: "tagger" | "editor" | null = null;

async function setupTauriDragDrop() {
  const webviewWindow = getCurrentWebviewWindow();

  await webviewWindow.onDragDropEvent(async (event) => {
    if (event.payload.type === "over") {
      // Detect which zone we're over based on position
      const pos = event.payload.position;
      const taggerRect = dropTagger.getBoundingClientRect();
      const editorRect = dropEditor.getBoundingClientRect();

      if (pos.x >= taggerRect.left && pos.x <= taggerRect.right &&
          pos.y >= taggerRect.top && pos.y <= taggerRect.bottom) {
        currentDropTarget = "tagger";
        if (!dropTagger.classList.contains("disabled")) {
          dropTagger.classList.add("drag-over");
        }
        dropEditor.classList.remove("drag-over");
      } else if (pos.x >= editorRect.left && pos.x <= editorRect.right &&
                 pos.y >= editorRect.top && pos.y <= editorRect.bottom) {
        currentDropTarget = "editor";
        if (!dropEditor.classList.contains("disabled")) {
          dropEditor.classList.add("drag-over");
        }
        dropTagger.classList.remove("drag-over");
      } else {
        currentDropTarget = null;
        dropTagger.classList.remove("drag-over");
        dropEditor.classList.remove("drag-over");
      }
    } else if (event.payload.type === "drop") {
      dropTagger.classList.remove("drag-over");
      dropEditor.classList.remove("drag-over");

      if (isTransferring || !currentDropTarget) return;

      const zone = currentDropTarget === "tagger" ? dropTagger : dropEditor;
      if (zone.classList.contains("disabled")) return;

      const paths = event.payload.paths;
      if (paths && paths.length > 0) {
        await sendFilesToRole(currentDropTarget, paths);
      }
      currentDropTarget = null;
    } else if (event.payload.type === "leave") {
      dropTagger.classList.remove("drag-over");
      dropEditor.classList.remove("drag-over");
      currentDropTarget = null;
    }
  });
}

async function setupTauriListeners() {
  // Checksum calculation progress
  await listen<ChecksumProgress>("checksum-progress", (event) => {
    const p = event.payload;
    const percent = (p.current / p.total) * 100;
    progressBar.style.width = `${percent}%`;
    progressStats.textContent = `${p.current} / ${p.total}`;
    progressFile.textContent = p.file_name;
    progressSpeed.textContent = "";
  });

  // Checksums were cached - skip calculation display
  await listen<number>("checksums-cached", (event) => {
    progressTitle.textContent = "Se conectează...";
    progressFile.textContent = `Se folosesc ${event.payload} checksums din cache`;
  });

  // Transfer progress
  await listen<TransferProgress>("transfer-progress", (event) => {
    const p = event.payload;
    const percent = (p.bytes_sent / p.total_bytes) * 100;

    progressBar.style.width = `${percent}%`;
    progressStats.textContent = `${p.file_index + 1} / ${p.total_files} fișiere`;
    progressFile.textContent = p.file_name;
    progressSpeed.textContent = `${p.speed_mbps.toFixed(1)} MB/s`;
  });

  // Transfer complete
  await listen<number>("transfer-complete", (event) => {
    isTransferring = false;
    progressSection.classList.remove("active");
    showToast(`Transfer complet: ${event.payload} fișiere`, "success");
    enableDropZones();
  });

  // Cancel button
  btnCancel.addEventListener("click", () => {
    if (isTransferring || progressSection.classList.contains("active")) {
      isTransferring = false;
      progressSection.classList.remove("active");
      showToast("Transfer anulat", "error");
      enableDropZones();
    }
  });
}

function startServiceDiscovery() {
  // Poll for services every 2 seconds
  setInterval(async () => {
    try {
      const foundServices = await invoke<DiscoveredService[]>("get_services");
      allServices = foundServices;
      updateServiceStatus();
    } catch (e) {
      console.error("Error getting services:", e);
    }
  }, 2000);
}

function getServicesByRole(role: string): DiscoveredService[] {
  return allServices.filter((s) => s.role === role);
}

function updateServiceStatus() {
  const taggers = getServicesByRole("tagger");
  const editors = getServicesByRole("editor");
  const nameValid = hasValidName();

  // Update tagger status
  if (taggers.length > 0) {
    taggerStatus.classList.add("online");
    if (taggers.length === 1) {
      taggerStatusText.textContent = taggers[0].name;
    } else {
      taggerStatusText.textContent = `${taggers.length} online`;
    }
    if (nameValid) {
      dropTagger.classList.remove("disabled");
    }
  } else {
    taggerStatus.classList.remove("online");
    taggerStatusText.textContent = "Se cauta...";
    dropTagger.classList.add("disabled");
  }

  // Update editor status
  if (editors.length > 0) {
    editorStatus.classList.add("online");
    if (editors.length === 1) {
      editorStatusText.textContent = editors[0].name;
    } else {
      editorStatusText.textContent = `${editors.length} online`;
    }
    if (nameValid) {
      dropEditor.classList.remove("disabled");
    }
  } else {
    editorStatus.classList.remove("online");
    editorStatusText.textContent = "Se cauta...";
    dropEditor.classList.add("disabled");
  }

  // If no name, keep disabled regardless of service status
  if (!nameValid) {
    dropTagger.classList.add("disabled");
    dropEditor.classList.add("disabled");
  }
}

async function selectFiles(role: string) {
  if (isTransferring) return;

  const zone = role === "tagger" ? dropTagger : dropEditor;
  if (zone.classList.contains("disabled")) return;

  try {
    const extensions = await invoke<string[]>("get_media_extensions");

    const selected = await open({
      multiple: true,
      directory: false,
      filters: [
        {
          name: "Media",
          extensions: extensions,
        },
      ],
    });

    if (selected && Array.isArray(selected) && selected.length > 0) {
      await sendFilesToRole(role, selected);
    } else if (selected && typeof selected === "string") {
      await sendFilesToRole(role, [selected]);
    }
  } catch (e) {
    console.error("Error selecting files:", e);
    showToast("Eroare la selectarea fișierelor", "error");
  }
}

async function selectFolder(role: string) {
  if (isTransferring) return;

  const zone = role === "tagger" ? dropTagger : dropEditor;
  if (zone.classList.contains("disabled")) return;

  try {
    const selected = await open({
      multiple: false,
      directory: true,
    });

    if (selected && typeof selected === "string") {
      await sendFilesToRole(role, [selected]);
    }
  } catch (e) {
    console.error("Error selecting folder:", e);
    showToast("Eroare la selectarea folderului", "error");
  }
}

async function sendFilesToRole(role: string, paths: string[]) {
  const receivers = getServicesByRole(role);

  if (receivers.length === 0) {
    showToast(`Nu există ${role} online`, "error");
    return;
  }

  if (receivers.length === 1) {
    // Only one receiver, send directly
    await sendFilesToReceiver(receivers[0], paths);
  } else {
    // Multiple receivers, show selection modal
    pendingFiles = paths;
    showReceiverSelectionModal(role, receivers);
  }
}

function showReceiverSelectionModal(role: string, receivers: DiscoveredService[]) {
  const modal = document.getElementById("receiver-select-modal")!;
  const list = document.getElementById("receiver-list")!;
  const title = document.getElementById("receiver-select-title")!;

  title.textContent = `Selecteaza ${role === "tagger" ? "Tagger" : "Editor"}`;
  list.innerHTML = "";

  receivers.forEach((receiver) => {
    const btn = document.createElement("button");
    btn.className = "receiver-option";
    btn.innerHTML = `
      <span class="receiver-name">${receiver.name}</span>
      <span class="receiver-ip">${receiver.host}</span>
    `;
    btn.addEventListener("click", async () => {
      modal.style.display = "none";
      await sendFilesToReceiver(receiver, pendingFiles);
      pendingFiles = [];
    });
    list.appendChild(btn);
  });

  modal.style.display = "flex";
}

async function sendFilesToReceiver(receiver: DiscoveredService, paths: string[]) {
  if (isTransferring) return;

  const name = photographerNameInput.value.trim();
  if (!name) {
    showToast("Introdu numele tău mai întâi", "error");
    photographerNameInput.focus();
    return;
  }

  if (paths.length === 0) {
    showToast("Nu s-au selectat fișiere", "error");
    return;
  }

  // Show expanding status
  progressSection.classList.add("active");
  progressBar.style.width = "0%";
  progressTitle.textContent = "Se procesează...";
  progressStats.textContent = "";
  progressFile.textContent = "Se scanează fișierele și folderele...";
  progressSpeed.textContent = "";

  try {
    // First expand paths (folders -> list of files inside)
    const expandedPaths = await invoke<string[]>("expand_paths", { paths });

    if (expandedPaths.length === 0) {
      progressSection.classList.remove("active");
      showToast("Nu s-au găsit fișiere media în selecție", "error");
      return;
    }

    // Update status for checksum progress
    progressTitle.textContent = "Se calculează checksums...";
    progressFile.textContent = `${expandedPaths.length} fișiere găsite.`;

    // Now check for duplicates with expanded paths
    const result = await invoke<DuplicateCheckResult>("check_duplicates_before_send", {
      targetHost: receiver.host,
      targetPort: receiver.port,
      photographerName: name,
      filePaths: expandedPaths,
      window: null, // Tauri will use current window
    });

    progressSection.classList.remove("active");

    if (result.duplicates.length > 0) {
      // Show duplicate dialog
      pendingDuplicateCheck = { receiver, paths: expandedPaths, result };
      showDuplicateModal(expandedPaths, result);
    } else {
      // No duplicates, proceed with transfer
      await startTransfer(receiver, expandedPaths, name);
    }
  } catch (e) {
    console.error("Error:", e);
    progressSection.classList.remove("active");
    showToast(`Eroare: ${e}`, "error");
  }
}

function showDuplicateModal(allPaths: string[], result: DuplicateCheckResult) {
  const modal = document.getElementById("duplicate-modal")!;
  const list = document.getElementById("duplicate-list")!;
  const info = document.getElementById("duplicate-info")!;
  const title = document.getElementById("duplicate-title")!;

  // Set info text
  if (result.resume_folder) {
    title.textContent = "Transfer întrerupt detectat";
    info.textContent = `Se reia transferul în folderul existent. ${result.duplicates.length} fișiere există deja.`;
  } else {
    title.textContent = "Fișiere duplicate detectate";
    info.textContent = `${result.duplicates.length} din ${allPaths.length} fișiere există deja pe receiver.`;
  }

  // Build list
  list.innerHTML = "";
  const duplicateNames = new Set(result.duplicates.map(d => d.file_name));

  allPaths.forEach((path) => {
    const fileName = path.split("/").pop() || path.split("\\").pop() || path;
    const duplicate = result.duplicates.find(d => d.file_name === fileName);
    const isDuplicate = duplicateNames.has(fileName);

    const item = document.createElement("div");
    item.className = `duplicate-item ${isDuplicate ? "is-duplicate" : ""} ${duplicate?.same_checksum ? "same-checksum" : ""}`;

    const checkbox = document.createElement("input");
    checkbox.type = "checkbox";
    checkbox.checked = !isDuplicate; // Pre-select non-duplicates
    checkbox.dataset.fileName = fileName;
    checkbox.dataset.path = path;

    const itemInfo = document.createElement("div");
    itemInfo.className = "duplicate-item-info";

    const itemName = document.createElement("div");
    itemName.className = "duplicate-item-name";
    itemName.textContent = fileName;

    const itemDetails = document.createElement("div");
    itemDetails.className = "duplicate-item-details";

    if (duplicate) {
      if (duplicate.same_checksum) {
        itemDetails.textContent = `Identic cu cel existent în ${duplicate.existing_path.split("/").pop() || "folder"}`;
      } else {
        const sizeDiff = duplicate.new_size - duplicate.existing_size;
        const sizeDiffStr = sizeDiff > 0 ? `+${formatSize(sizeDiff)}` : formatSize(sizeDiff);
        itemDetails.textContent = `Diferit (${sizeDiffStr}) - există în ${duplicate.existing_path.split("/").pop() || "folder"}`;
      }
    } else {
      itemDetails.textContent = "Fișier nou";
    }

    itemInfo.appendChild(itemName);
    itemInfo.appendChild(itemDetails);

    const badge = document.createElement("span");
    badge.className = `duplicate-item-badge ${isDuplicate ? (duplicate?.same_checksum ? "identical" : "duplicate") : "new"}`;
    badge.textContent = isDuplicate ? (duplicate?.same_checksum ? "Identic" : "Diferit") : "Nou";

    item.appendChild(checkbox);
    item.appendChild(itemInfo);
    item.appendChild(badge);
    list.appendChild(item);
  });

  modal.style.display = "flex";
}

function formatSize(bytes: number): string {
  const abs = Math.abs(bytes);
  if (abs < 1024) return `${bytes} B`;
  if (abs < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (abs < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
}

function setupDuplicateModal() {
  const modal = document.getElementById("duplicate-modal")!;
  const btnCancel = document.getElementById("duplicate-cancel")!;
  const btnConfirm = document.getElementById("duplicate-confirm")!;
  const btnSelectAll = document.getElementById("dup-select-all")!;
  const btnDeselectAll = document.getElementById("dup-deselect-all")!;
  const btnSelectNew = document.getElementById("dup-select-new")!;

  btnCancel.addEventListener("click", () => {
    modal.style.display = "none";
    pendingDuplicateCheck = null;
  });

  modal.addEventListener("click", (e) => {
    if (e.target === modal) {
      modal.style.display = "none";
      pendingDuplicateCheck = null;
    }
  });

  btnSelectAll.addEventListener("click", () => {
    const checkboxes = document.querySelectorAll("#duplicate-list input[type=checkbox]") as NodeListOf<HTMLInputElement>;
    checkboxes.forEach(cb => cb.checked = true);
  });

  btnDeselectAll.addEventListener("click", () => {
    const checkboxes = document.querySelectorAll("#duplicate-list input[type=checkbox]") as NodeListOf<HTMLInputElement>;
    checkboxes.forEach(cb => cb.checked = false);
  });

  btnSelectNew.addEventListener("click", () => {
    if (!pendingDuplicateCheck) return;
    const duplicateNames = new Set(pendingDuplicateCheck.result.duplicates.map(d => d.file_name));
    const checkboxes = document.querySelectorAll("#duplicate-list input[type=checkbox]") as NodeListOf<HTMLInputElement>;
    checkboxes.forEach(cb => {
      cb.checked = !duplicateNames.has(cb.dataset.fileName || "");
    });
  });

  btnConfirm.addEventListener("click", async () => {
    if (!pendingDuplicateCheck) return;

    const checkboxes = document.querySelectorAll("#duplicate-list input[type=checkbox]:checked") as NodeListOf<HTMLInputElement>;
    const selectedPaths: string[] = [];
    const selectedNames: string[] = [];

    checkboxes.forEach(cb => {
      if (cb.dataset.path) selectedPaths.push(cb.dataset.path);
      if (cb.dataset.fileName) selectedNames.push(cb.dataset.fileName);
    });

    modal.style.display = "none";

    if (selectedPaths.length === 0) {
      showToast("Nu ai selectat niciun fișier", "error");
      pendingDuplicateCheck = null;
      return;
    }

    const { receiver, paths } = pendingDuplicateCheck;
    const name = photographerNameInput.value.trim();
    pendingDuplicateCheck = null;

    await startTransferWithSelection(receiver, paths, name, selectedNames);
  });
}

async function startTransfer(receiver: DiscoveredService, paths: string[], name: string) {
  isTransferring = true;
    disableDropZones();

  // Show progress
  progressSection.classList.add("active");
  progressBar.style.width = "0%";
  progressTitle.textContent = `Se trimite la ${receiver.name}...`;
  progressStats.textContent = `0 / ${paths.length} fișiere`;
  progressFile.textContent = "Se pregătește...";
  progressSpeed.textContent = "- MB/s";

  try {
    await invoke("send_files_to_host", {
      targetHost: receiver.host,
      targetPort: receiver.port,
      photographerName: name,
      filePaths: paths,
    });
  } catch (e) {
    console.error("Transfer error:", e);
    showToast(`Eroare transfer: ${e}`, "error");
    isTransferring = false;
    progressSection.classList.remove("active");
    enableDropZones();
  }
}

async function startTransferWithSelection(
  receiver: DiscoveredService,
  paths: string[],
  name: string,
  filesToSend: string[]
) {
  isTransferring = true;
    disableDropZones();

  // Show progress
  progressSection.classList.add("active");
  progressBar.style.width = "0%";
  progressTitle.textContent = `Se trimite la ${receiver.name}...`;
  progressStats.textContent = `0 / ${filesToSend.length} fișiere`;
  progressFile.textContent = "Se pregătește...";
  progressSpeed.textContent = "- MB/s";

  try {
    await invoke("send_files_with_selection", {
      targetHost: receiver.host,
      targetPort: receiver.port,
      photographerName: name,
      filePaths: paths,
      filesToSend: filesToSend,
    });
  } catch (e) {
    console.error("Transfer error:", e);
    showToast(`Eroare transfer: ${e}`, "error");
    isTransferring = false;
    progressSection.classList.remove("active");
    enableDropZones();
  }
}

function disableDropZones() {
  dropTagger.classList.add("disabled");
  dropEditor.classList.add("disabled");
}

function enableDropZones() {
  updateServiceStatus();
}

function showToast(message: string, type: "success" | "error") {
  toastMessage.textContent = message;
  toast.className = `toast ${type} show`;

  setTimeout(() => {
    toast.classList.remove("show");
  }, 3000);
}

interface ReceiverInfo {
  name: string;
  role: string;
}

// Manual connect functionality
function setupManualConnect() {
  const btnManual = document.getElementById("btn-manual-connect")!;
  const modal = document.getElementById("manual-modal")!;
  const btnCancel = document.getElementById("manual-cancel")!;
  const btnConfirm = document.getElementById("manual-confirm")!;
  const ipInput = document.getElementById("manual-ip") as HTMLInputElement;

  // Receiver selection modal
  const receiverModal = document.getElementById("receiver-select-modal")!;
  const receiverCancel = document.getElementById("receiver-select-cancel")!;

  receiverCancel.addEventListener("click", () => {
    receiverModal.style.display = "none";
    pendingFiles = [];
  });

  receiverModal.addEventListener("click", (e) => {
    if (e.target === receiverModal) {
      receiverModal.style.display = "none";
      pendingFiles = [];
    }
  });

  btnManual.addEventListener("click", () => {
    modal.style.display = "flex";
    ipInput.focus();
  });

  btnCancel.addEventListener("click", () => {
    modal.style.display = "none";
  });

  modal.addEventListener("click", (e) => {
    if (e.target === modal) {
      modal.style.display = "none";
    }
  });

  btnConfirm.addEventListener("click", async () => {
    const ip = ipInput.value.trim();

    if (!ip) {
      showToast("Introdu adresa IP", "error");
      return;
    }

    try {
      // Get receiver info from the receiver
      const info = await invoke<ReceiverInfo>("get_receiver_info", {
        ip,
        port: 45678,
      });

      // Add to services
      await invoke("add_manual_service", {
        ip,
        port: 45678,
        role: info.role,
        name: info.name,
      });

      modal.style.display = "none";
      showToast(`Conectat la ${info.name}`, "success");

      // Force update service status
      allServices = await invoke<DiscoveredService[]>("get_services");
      updateServiceStatus();
    } catch (e) {
      showToast(`Eroare: ${e}`, "error");
    }
  });
}
