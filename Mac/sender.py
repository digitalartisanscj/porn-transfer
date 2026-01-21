#!/usr/bin/env python3
"""
Pornhub Transfer - Sender
For Photographers (Mac/Windows)
"""

import customtkinter as ctk
import socket
import threading
import json
import os
import hashlib
from pathlib import Path
from zeroconf import ServiceBrowser, Zeroconf, ServiceListener
import struct
import time
from datetime import datetime
from tkinter import filedialog

# Try to import drag and drop support
try:
    from tkinterdnd2 import DND_FILES, TkinterDnD
    HAS_DND = True
except ImportError:
    HAS_DND = False
    print("Note: tkinterdnd2 not installed, drag & drop disabled. Install with: pip install tkinterdnd2")

# Config
PORT = 45678
SERVICE_TYPE = "_phototransfer._tcp.local."
CHUNK_SIZE = 1024 * 1024  # 1MB chunks


class ServiceDiscovery(ServiceListener):
    def __init__(self, callback):
        self.callback = callback
        self.services = {}
    
    def update_service(self, zc, type_, name):
        pass
    
    def remove_service(self, zc, type_, name):
        if name in self.services:
            del self.services[name]
            self.callback(self.services)
    
    def add_service(self, zc, type_, name):
        info = zc.get_service_info(type_, name)
        if info:
            ip = socket.inet_ntoa(info.addresses[0])
            role = info.properties.get(b'role', b'unknown').decode()
            self.services[name] = {
                'ip': ip,
                'port': info.port,
                'role': role,
                'name': name
            }
            self.callback(self.services)


class DropZone(ctk.CTkFrame):
    def __init__(self, master, title, icon, color, on_drop, on_click, target_name, **kwargs):
        super().__init__(master, **kwargs)
        
        self.on_drop_callback = on_drop
        self.on_click = on_click
        self.target_name = target_name
        self.files = []
        self.color = color
        self.default_color = color
        
        self.configure(fg_color=color, corner_radius=15, height=180)
        
        self.icon_label = ctk.CTkLabel(self, text=icon, font=("", 40))
        self.icon_label.pack(pady=(20, 5))
        
        self.title_label = ctk.CTkLabel(self, text=title, font=("", 16, "bold"))
        self.title_label.pack()
        
        self.status_label = ctk.CTkLabel(self, text="Click sau trage pozele", font=("", 12))
        self.status_label.pack(pady=5)
        
        self.progress_bar = ctk.CTkProgressBar(self, width=150)
        self.progress_bar.set(0)
        self.progress_bar.pack(pady=5)
        self.progress_bar.pack_forget()  # Hide initially
        
        # Bind click
        self.bind("<Button-1>", lambda e: self.on_click())
        self.icon_label.bind("<Button-1>", lambda e: self.on_click())
        self.title_label.bind("<Button-1>", lambda e: self.on_click())
        self.status_label.bind("<Button-1>", lambda e: self.on_click())
    
    def register_drop(self, root):
        """Register this zone for drag and drop"""
        if HAS_DND:
            self.drop_target_register(DND_FILES)
            self.dnd_bind('<<DropEnter>>', self.on_drag_enter)
            self.dnd_bind('<<DropLeave>>', self.on_drag_leave)
            self.dnd_bind('<<Drop>>', self.on_drop)
    
    def on_drag_enter(self, event):
        self.configure(fg_color="#2a5a3a" if "TAGGER" in self.title_label.cget("text") else "#6a2a2a")
        return event.action
    
    def on_drag_leave(self, event):
        self.configure(fg_color=self.default_color)
        return event.action
    
    def on_drop(self, event):
        self.configure(fg_color=self.default_color)
        # Parse dropped files
        files = self.parse_drop_data(event.data)
        if files:
            self.on_drop_callback(files)
        return event.action
    
    def parse_drop_data(self, data):
        """Parse dropped file/folder paths from event data"""
        files = []
        paths_to_check = []
        
        # Handle different formats
        if '{' in data:
            # Tcl list format with braces for paths with spaces
            import re
            # Find all paths in braces or standalone
            parts = re.findall(r'\{([^}]+)\}|(\S+)', data)
            for match in parts:
                path = match[0] if match[0] else match[1]
                if path:
                    paths_to_check.append(path)
        else:
            # Simple space-separated or newline-separated
            for path in data.replace('\r', '').split('\n'):
                path = path.strip()
                if path:
                    paths_to_check.append(path)
        
        # Process paths - extract files from folders
        for path in paths_to_check:
            if os.path.isfile(path):
                files.append(path)
            elif os.path.isdir(path):
                # Extract all media files from folder
                files.extend(self.get_files_from_folder(path))
        
        return files
    
    def get_files_from_folder(self, folder_path):
        """Extract all media files from a folder (recursively)"""
        media_extensions = {
            # RAW formats - Canon
            '.cr2', '.cr3', '.crw',
            # RAW formats - Nikon
            '.nef', '.nrw',
            # RAW formats - Sony
            '.arw', '.srf', '.sr2',
            # RAW formats - Fujifilm
            '.raf',
            # RAW formats - Panasonic/Leica
            '.rw2', '.rwl',
            # RAW formats - Olympus/OM System
            '.orf',
            # RAW formats - Pentax
            '.pef', '.ptx',
            # RAW formats - Samsung
            '.srw',
            # RAW formats - Hasselblad
            '.3fr', '.fff',
            # RAW formats - Phase One
            '.iiq',
            # RAW formats - Sigma
            '.x3f',
            # RAW formats - GoPro
            '.gpr',
            # RAW formats - Adobe/Generic
            '.dng', '.raw',
            # Image formats
            '.jpg', '.jpeg', '.png', '.tiff', '.tif', '.heic', '.heif', '.webp', '.bmp', '.gif',
            # Video formats
            '.mp4', '.mov', '.avi', '.mkv', '.mxf', '.m4v', '.wmv',
            # Professional video RAW
            '.braw', '.r3d', '.crm',
        }
        
        files = []
        for root, dirs, filenames in os.walk(folder_path):
            for filename in filenames:
                ext = os.path.splitext(filename)[1].lower()
                if ext in media_extensions:
                    files.append(os.path.join(root, filename))
        
        return sorted(files)
    
    def set_status(self, text):
        self.status_label.configure(text=text)
    
    def set_progress(self, value):
        if value > 0:
            self.progress_bar.pack(pady=5)
            self.progress_bar.set(value)
        else:
            self.progress_bar.pack_forget()
    
    def set_progress_with_status(self, value, status):
        if value > 0:
            self.progress_bar.pack(pady=5)
            self.progress_bar.set(value)
            self.status_label.configure(text=status)
        else:
            self.progress_bar.pack_forget()
    
    def set_done(self):
        self.status_label.configure(text="✅ Gata, șefu'!")
        self.progress_bar.pack_forget()
        self.after(3000, lambda: self.set_status("Click sau trage pozele"))
    
    def set_error(self, msg):
        self.status_label.configure(text=f"❌ {msg}")
        self.progress_bar.pack_forget()


# Create custom Tk class that supports DnD
if HAS_DND:
    class TkDnDCTk(ctk.CTk, TkinterDnD.DnDWrapper):
        def __init__(self, *args, **kwargs):
            super().__init__(*args, **kwargs)
            self.TkdndVersion = TkinterDnD._require(self)
else:
    TkDnDCTk = ctk.CTk


class SenderApp(TkDnDCTk):
    def __init__(self):
        super().__init__()
        
        self.title("🔥 Pornhub Transfer - Expeditor")
        self.geometry("550x500")
        self.minsize(450, 450)
        
        # State
        self.photographer_name = ""
        self.tagger_service = None
        self.editor_service = None
        self.services = {}
        self.sending = False
        
        # Sent files log (per day)
        self.sent_log = {}  # {filename+size: timestamp}
        self.sent_log_path = None  # Set after loading config
        
        # Config
        self.config_path = Path.home() / ".photo_transfer_sender.json"
        self.load_config()
        self.load_sent_log()
        
        # Start discovery
        self.zeroconf = Zeroconf()
        self.listener = ServiceDiscovery(self.on_services_updated)
        self.browser = ServiceBrowser(self.zeroconf, SERVICE_TYPE, self.listener)
        
        self.build_ui()
    
    def load_config(self):
        if self.config_path.exists():
            try:
                with open(self.config_path) as f:
                    config = json.load(f)
                    self.photographer_name = config.get('name', '')
            except:
                pass
    
    def save_config(self):
        with open(self.config_path, 'w') as f:
            json.dump({'name': self.photographer_name}, f)
    
    def get_sent_log_path(self):
        """Get path for today's sent log"""
        today = datetime.now().strftime("%Y-%m-%d")
        return Path.home() / f".photo_transfer_sent_{today}.json"
    
    def load_sent_log(self):
        """Load sent files log for today"""
        self.sent_log_path = self.get_sent_log_path()
        if self.sent_log_path.exists():
            try:
                with open(self.sent_log_path) as f:
                    self.sent_log = json.load(f)
            except:
                self.sent_log = {}
        else:
            self.sent_log = {}
    
    def save_sent_log(self):
        """Save sent files log"""
        with open(self.sent_log_path, 'w') as f:
            json.dump(self.sent_log, f)
    
    def get_file_key(self, filepath):
        """Generate unique key for a file (name + size)"""
        path = Path(filepath)
        size = path.stat().st_size
        return f"{path.name}|{size}"
    
    def mark_file_sent(self, filepath, target):
        """Mark a file as sent"""
        key = self.get_file_key(filepath)
        self.sent_log[key] = {
            'name': Path(filepath).name,
            'target': target,
            'timestamp': datetime.now().isoformat()
        }
        self.save_sent_log()
    
    def check_duplicates(self, files):
        """Check which files have already been sent today"""
        duplicates = []
        new_files = []
        
        for filepath in files:
            key = self.get_file_key(filepath)
            if key in self.sent_log:
                duplicates.append({
                    'path': filepath,
                    'name': Path(filepath).name,
                    'sent_at': self.sent_log[key].get('timestamp', 'earlier')
                })
            else:
                new_files.append(filepath)
        
        return duplicates, new_files
    
    def show_duplicate_dialog(self, duplicates, new_files, all_files, target, zone, service):
        """Show dialog asking what to do with duplicates - uses callbacks"""
        dialog = ctk.CTkToplevel(self)
        dialog.title("🤔 Stai puțin...")
        dialog.geometry("450x450")
        dialog.transient(self)
        dialog.grab_set()
        
        # Center on parent
        dialog.update_idletasks()
        x = self.winfo_x() + (self.winfo_width() - 450) // 2
        y = self.winfo_y() + (self.winfo_height() - 450) // 2
        dialog.geometry(f"+{x}+{y}")
        
        # Warning icon and message
        ctk.CTkLabel(dialog, text="🤨", font=("", 40)).pack(pady=(15, 5))
        ctk.CTkLabel(dialog, text=f"{len(duplicates)} fișiere deja trimise azi!", 
                    font=("", 16, "bold")).pack()
        
        ctk.CTkLabel(dialog, text=f"{len(new_files)} fișiere noi de trimis",
                    font=("", 12), text_color="gray").pack(pady=(5, 10))
        
        # List duplicates (scrollable)
        list_frame = ctk.CTkScrollableFrame(dialog, height=120)
        list_frame.pack(fill="x", padx=20, pady=10)
        
        for dup in duplicates[:10]:  # Show first 10
            ctk.CTkLabel(list_frame, text=f"• {dup['name']}", 
                        font=("", 11), anchor="w").pack(anchor="w")
        
        if len(duplicates) > 10:
            ctk.CTkLabel(list_frame, text=f"... și încă {len(duplicates) - 10}",
                        font=("", 11), text_color="gray").pack(anchor="w")
        
        # Buttons frame at bottom
        btn_frame = ctk.CTkFrame(dialog, fg_color="transparent")
        btn_frame.pack(fill="x", padx=20, pady=20, side="bottom")
        
        def on_skip():
            dialog.destroy()
            if new_files:
                self.sending = True
                threading.Thread(target=self._send_files_thread, args=(new_files, service, zone, target), daemon=True).start()
            else:
                zone.set_status("Nimic nou de trimis")
                self.after(2000, lambda: zone.set_status("Click sau trage pozele"))
        
        def on_send_all():
            dialog.destroy()
            self.sending = True
            threading.Thread(target=self._send_files_thread, args=(all_files, service, zone, target), daemon=True).start()
        
        def on_cancel():
            dialog.destroy()
            zone.set_status("Anulat")
            self.after(2000, lambda: zone.set_status("Click sau trage pozele"))
        
        # Cancel button on the right
        ctk.CTkButton(btn_frame, text="Renunț", fg_color="gray", width=100,
                     command=on_cancel).pack(side="right", padx=5)
        
        # Send ALL button
        ctk.CTkButton(btn_frame, text=f"📤 Trimite TOATE ({len(all_files)})", width=150,
                     command=on_send_all).pack(side="right", padx=5)
        
        # Send only new button (if there are new files)
        if new_files:
            ctk.CTkButton(btn_frame, text=f"📤 Doar noi ({len(new_files)})", 
                         fg_color="green", width=150, command=on_skip).pack(side="right", padx=5)
    
    def build_ui(self):
        # Header
        header = ctk.CTkFrame(self, fg_color="transparent")
        header.pack(fill="x", padx=20, pady=15)
        
        ctk.CTkLabel(header, text="🔥 Pornhub Transfer", font=("", 22, "bold")).pack(side="left")
        
        # Name input
        name_frame = ctk.CTkFrame(self, fg_color="transparent")
        name_frame.pack(fill="x", padx=20, pady=10)
        
        ctk.CTkLabel(name_frame, text="Numele tău:", font=("", 14)).pack(side="left")
        
        self.name_entry = ctk.CTkEntry(name_frame, width=200, font=("", 14))
        self.name_entry.insert(0, self.photographer_name)
        self.name_entry.pack(side="left", padx=10)
        self.name_entry.bind("<KeyRelease>", self.on_name_change)
        
        # Status
        self.status_frame = ctk.CTkFrame(self, fg_color="transparent")
        self.status_frame.pack(fill="x", padx=20, pady=5)
        
        self.tagger_status = ctk.CTkLabel(self.status_frame, text="🏷️ Tagger: caut...", font=("", 12))
        self.tagger_status.pack(side="left", padx=10)
        
        self.editor_status = ctk.CTkLabel(self.status_frame, text="🎨 Editor: caut...", font=("", 12))
        self.editor_status.pack(side="right", padx=10)
        
        # Drop zones
        zones_frame = ctk.CTkFrame(self, fg_color="transparent")
        zones_frame.pack(fill="both", expand=True, padx=20, pady=20)
        
        zones_frame.columnconfigure(0, weight=1)
        zones_frame.columnconfigure(1, weight=1)
        zones_frame.rowconfigure(0, weight=1)
        
        self.tagger_zone = DropZone(
            zones_frame, 
            "TAGGER", "📁", "#1a472a",
            on_drop=lambda f: self.send_files(f, 'tagger'),
            on_click=lambda: self.select_files('tagger'),
            target_name='tagger'
        )
        self.tagger_zone.grid(row=0, column=0, sticky="nsew", padx=(0, 10))
        
        self.editor_zone = DropZone(
            zones_frame,
            "EDITOR", "🚨", "#4a1a1a", 
            on_drop=lambda f: self.send_files(f, 'editor'),
            on_click=lambda: self.select_files('editor'),
            target_name='editor'
        )
        self.editor_zone.grid(row=0, column=1, sticky="nsew", padx=(10, 0))
        
        # Register drag and drop
        if HAS_DND:
            self.tagger_zone.register_drop(self)
            self.editor_zone.register_drop(self)
            info_text = "Click pe zonă sau trage fișiere/foldere"
        else:
            info_text = "Click pe zonă pentru a selecta fișiere"
        
        # Info
        info_label = ctk.CTkLabel(self, text=info_text, font=("", 11), text_color="gray")
        info_label.pack(pady=10)
    
    def on_name_change(self, event=None):
        self.photographer_name = self.name_entry.get().strip()
        self.save_config()
    
    def on_services_updated(self, services):
        self.services = services
        self.after(0, self.update_status)
    
    def update_status(self):
        self.tagger_service = None
        self.editor_service = None
        
        for name, info in self.services.items():
            if info['role'] == 'tagger':
                self.tagger_service = info
            elif info['role'] == 'editor':
                self.editor_service = info
        
        if self.tagger_service:
            self.tagger_status.configure(text=f"🏷️ Tagger: ✅ {self.tagger_service['ip']}")
            self.tagger_zone.set_status("Click sau trage pozele")
        else:
            self.tagger_status.configure(text="🏷️ Tagger: ⏳ caut...")
            self.tagger_zone.set_status("Neconectat")
        
        if self.editor_service:
            self.editor_status.configure(text=f"🎨 Editor: ✅ {self.editor_service['ip']}")
            self.editor_zone.set_status("Click sau trage pozele")
        else:
            self.editor_status.configure(text="🎨 Editor: ⏳ caut...")
            self.editor_zone.set_status("Neconectat")
    
    def select_files(self, target):
        if self.sending:
            return
        
        if target == 'tagger' and not self.tagger_service:
            return
        if target == 'editor' and not self.editor_service:
            return
        
        if not self.photographer_name:
            self.name_entry.focus()
            self.name_entry.configure(border_color="red")
            self.after(2000, lambda: self.name_entry.configure(border_color="gray"))
            return
        
        # File dialog
        files = filedialog.askopenfilenames(
            title="Selectează fișiere",
            filetypes=[
                ("All media", "*.arw *.cr2 *.cr3 *.crw *.nef *.nrw *.raf *.rw2 *.rwl *.orf *.dng *.raw *.pef *.ptx *.srw *.srf *.sr2 *.3fr *.fff *.iiq *.x3f *.gpr *.jpg *.jpeg *.png *.tiff *.tif *.heic *.heif *.webp *.bmp *.gif *.mp4 *.mov *.avi *.mkv *.mxf *.m4v *.wmv *.braw *.r3d *.crm *.ARW *.CR2 *.CR3 *.CRW *.NEF *.NRW *.RAF *.RW2 *.RWL *.ORF *.DNG *.RAW *.PEF *.PTX *.SRW *.SRF *.SR2 *.3FR *.FFF *.IIQ *.X3F *.GPR *.JPG *.JPEG *.PNG *.TIFF *.TIF *.HEIC *.HEIF *.WEBP *.BMP *.GIF *.MP4 *.MOV *.AVI *.MKV *.MXF *.M4V *.WMV *.BRAW *.R3D *.CRM"),
                ("All files", "*.*")
            ]
        )
        
        if files:
            self.send_files(list(files), target)
    
    def send_files(self, files, target):
        if self.sending:
            return
        
        if not files:
            return
        
        if not self.photographer_name:
            self.name_entry.focus()
            self.name_entry.configure(border_color="red")
            self.after(2000, lambda: self.name_entry.configure(border_color="gray"))
            return
        
        service = self.tagger_service if target == 'tagger' else self.editor_service
        zone = self.tagger_zone if target == 'tagger' else self.editor_zone
        
        if not service:
            zone.set_error("Not connected")
            return
        
        # Check for duplicates
        duplicates, new_files = self.check_duplicates(files)
        
        if duplicates:
            # Show dialog - it handles sending via callbacks
            self.show_duplicate_dialog(duplicates, new_files, files, target, zone, service)
        else:
            # No duplicates, send directly
            self.sending = True
            threading.Thread(target=self._send_files_thread, args=(files, service, zone, target), daemon=True).start()
    
    def _send_files_thread(self, files, service, zone, target):
        try:
            # Prepare file info
            file_infos = []
            self.after(0, lambda: zone.set_status(f"Pregătesc {len(files)} fișiere..."))
            
            for file_path in files:
                path = Path(file_path)
                size = path.stat().st_size
                
                # Calculate checksum
                hasher = hashlib.md5()
                with open(path, 'rb') as f:
                    while chunk := f.read(CHUNK_SIZE):
                        hasher.update(chunk)
                
                file_infos.append({
                    'name': path.name,
                    'size': size,
                    'checksum': hasher.hexdigest(),
                    'path': str(path)
                })
            
            total_size = sum(f['size'] for f in file_infos)
            
            self.after(0, lambda: zone.set_status(f"Mă conectez..."))
            
            # Connect
            sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
            sock.connect((service['ip'], service['port']))
            
            # Send header
            header = {
                'photographer': self.photographer_name,
                'files': [{'name': f['name'], 'size': f['size'], 'checksum': f['checksum']} for f in file_infos]
            }
            header_data = json.dumps(header).encode('utf-8')
            sock.send(struct.pack('!I', len(header_data)))
            sock.send(header_data)
            
            # Wait for ACK
            ack_size = struct.unpack('!I', sock.recv(4))[0]
            ack = json.loads(sock.recv(ack_size).decode('utf-8'))
            
            if ack.get('status') != 'ready':
                raise Exception("Server not ready")
            
            self.after(0, lambda: zone.set_status(f"Trimit {len(files)} fișiere..."))
            
            # Send files with speed tracking
            sent_total = 0
            start_time = time.time()
            last_update_time = start_time
            last_sent = 0
            
            for file_info in file_infos:
                with open(file_info['path'], 'rb') as f:
                    remaining = file_info['size']
                    while remaining > 0:
                        chunk = f.read(min(CHUNK_SIZE, remaining))
                        sock.send(chunk)
                        remaining -= len(chunk)
                        sent_total += len(chunk)
                        
                        # Update progress with speed and ETA
                        now = time.time()
                        if now - last_update_time >= 0.3:  # Update every 300ms
                            progress = sent_total / total_size
                            
                            # Calculate speed
                            elapsed = now - start_time
                            if elapsed > 0:
                                speed = sent_total / elapsed  # bytes per second
                                speed_mb = speed / (1024 * 1024)
                                
                                # Calculate ETA
                                remaining_bytes = total_size - sent_total
                                if speed > 0:
                                    eta_seconds = remaining_bytes / speed
                                    if eta_seconds < 60:
                                        eta_str = f"{int(eta_seconds)}s"
                                    else:
                                        eta_str = f"{int(eta_seconds // 60)}m {int(eta_seconds % 60)}s"
                                else:
                                    eta_str = "..."
                                
                                status = f"{speed_mb:.1f} MB/s • {eta_str} rămas"
                                self.after(0, lambda s=status, p=progress: zone.set_progress_with_status(p, s))
                            
                            last_update_time = now
                            last_sent = sent_total
                
                # Wait for confirmation
                response = sock.recv(16)
                if response != b'OK':
                    raise Exception(f"Transfer error: {response.decode()}")
                
                # Mark file as sent
                self.mark_file_sent(file_info['path'], target)
            
            sock.close()
            self.after(0, zone.set_done)
            
        except Exception as e:
            self.after(0, lambda: zone.set_error(str(e)[:30]))
        finally:
            self.sending = False
    
    def on_closing(self):
        self.zeroconf.close()
        self.destroy()


if __name__ == "__main__":
    ctk.set_appearance_mode("dark")
    ctk.set_default_color_theme("blue")
    
    app = SenderApp()
    app.protocol("WM_DELETE_WINDOW", app.on_closing)
    app.mainloop()
