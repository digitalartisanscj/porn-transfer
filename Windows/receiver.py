#!/usr/bin/env python3
"""
Pornhub Transfer - Receiver
For Tagger (Windows) and Editor (Mac)
"""

import customtkinter as ctk
import socket
import threading
import json
import os
import hashlib
import uuid
from pathlib import Path
from zeroconf import ServiceInfo, Zeroconf, ServiceBrowser, ServiceListener
import struct
import time
from datetime import datetime
from tkinter import filedialog
from collections import defaultdict

# Config
PORT = 45678
SERVICE_TYPE = "_phototransfer._tcp.local."
CHUNK_SIZE = 1024 * 1024  # 1MB chunks

# Default folder templates
DEFAULT_TEMPLATES = [
    "{num:02d} - {name}",
    "{name}_{num:03d}",
    "{num:02d}_{name}",
    "{date}_{num:02d} - {name}",
    "{name}_{date}_{time}",
    "{name}",
]


class PeerDiscovery(ServiceListener):
    """Discover peers on the network (editors for tagger, other editors for editors)"""
    def __init__(self, callback, my_role, my_ip=None):
        self.callback = callback
        self.my_role = my_role
        self.my_ip = my_ip
        self.peers = {}
    
    def update_service(self, zc, type_, name):
        pass
    
    def remove_service(self, zc, type_, name):
        if name in self.peers:
            del self.peers[name]
            self.callback(self.peers)
    
    def add_service(self, zc, type_, name):
        info = zc.get_service_info(type_, name)
        if info:
            role = info.properties.get(b'role', b'unknown').decode()
            peer_name = info.properties.get(b'name', b'').decode()
            ip = socket.inet_ntoa(info.addresses[0])
            
            # Skip self
            if ip == self.my_ip:
                return
            
            # Tagger sees editors, editors see other editors
            if role == 'editor':
                display_name = peer_name if peer_name else ip
                self.peers[name] = {
                    'ip': ip,
                    'port': info.port,
                    'name': display_name,
                    'role': role
                }
                self.callback(self.peers)

class ReceiverApp(ctk.CTk):
    def __init__(self):
        super().__init__()
        
        self.title("🔥 Pornhub Transfer - Receptor")
        self.geometry("650x850")
        self.minsize(550, 750)
        
        # State
        self.role = None  # 'tagger' or 'editor'
        self.editor_name = ""  # Name for editors
        self.current_day = "DAY 1"
        self.folder_counter = 0
        self.base_path = None
        self.server_socket = None
        self.zeroconf = None
        self.service_info = None
        self.running = False
        self.transfers = {}  # Incoming: {client_id: {name, progress, total, status}}
        self.outgoing = {}   # Outgoing: {transfer_id: {folder, destination, sent, total, status}}
        
        # Folder settings
        self.folder_template = "{num:02d} - {name}"
        self.use_day_folders = True
        self.reset_numbering_daily = True
        self.custom_day_prefix = "DAY"
        
        # Transfer history
        self.history = []
        self.history_path = Path.home() / ".photo_transfer_history.json"
        
        # Peer discovery (for tagger/editor to send to editors)
        self.peers = {}  # Other editors (and tagger for editors)
        self.peer_browser = None
        self.local_ip = None  # Set in start_server
        
        # Initialize zeroconf early (needed for discovery)
        self.zeroconf = Zeroconf()
        
        # Load config
        self.config_path = Path.home() / ".photo_transfer_receiver.json"
        self.load_config()
        self.load_history()
        
        if self.role:
            self.show_main_ui()
        else:
            self.show_role_selection()
    
    def load_config(self):
        if self.config_path.exists():
            try:
                with open(self.config_path) as f:
                    config = json.load(f)
                    self.role = config.get('role')
                    self.editor_name = config.get('editor_name', '')
                    self.base_path = config.get('base_path')
                    self.folder_counter = config.get('folder_counter', 0)
                    self.folder_template = config.get('folder_template', "{num:02d} - {name}")
                    self.use_day_folders = config.get('use_day_folders', True)
                    self.reset_numbering_daily = config.get('reset_numbering_daily', True)
                    self.custom_day_prefix = config.get('custom_day_prefix', "DAY")
                    self.current_day = config.get('current_day', "DAY 1")
            except:
                pass
    
    def save_config(self):
        config = {
            'role': self.role,
            'editor_name': self.editor_name,
            'base_path': self.base_path,
            'folder_counter': self.folder_counter,
            'folder_template': self.folder_template,
            'use_day_folders': self.use_day_folders,
            'reset_numbering_daily': self.reset_numbering_daily,
            'custom_day_prefix': self.custom_day_prefix,
            'current_day': self.current_day
        }
        with open(self.config_path, 'w') as f:
            json.dump(config, f)
    
    def load_history(self):
        """Load transfer history from file"""
        if self.history_path.exists():
            try:
                with open(self.history_path) as f:
                    self.history = json.load(f)
            except:
                self.history = []
    
    def save_history(self):
        """Save transfer history to file"""
        with open(self.history_path, 'w') as f:
            json.dump(self.history, f, indent=2)
    
    def add_to_history(self, photographer, file_count, total_size, folder, day=None):
        """Add a completed transfer to history"""
        entry = {
            'timestamp': datetime.now().isoformat(),
            'photographer': photographer,
            'file_count': file_count,
            'total_size': total_size,
            'folder': str(folder),
            'day': day
        }
        self.history.insert(0, entry)  # Most recent first
        # Keep last 500 entries
        self.history = self.history[:500]
        self.save_history()
        # Update UI
        self.after(0, self.refresh_history_ui)
        self.after(0, self.refresh_report_ui)
    
    def show_role_selection(self):
        self.clear_window()
        
        frame = ctk.CTkFrame(self)
        frame.pack(expand=True, fill="both", padx=40, pady=40)
        
        ctk.CTkLabel(frame, text="🔥 Pornhub Transfer", font=("", 24, "bold")).pack(pady=20)
        ctk.CTkLabel(frame, text="Tu ce învârți?", font=("", 16)).pack(pady=10)
        
        ctk.CTkButton(frame, text="🏷️  TAGGER", font=("", 18), height=60, width=250,
                      command=lambda: self.set_role('tagger')).pack(pady=15)
        
        ctk.CTkButton(frame, text="🎨  EDITOR", font=("", 18), height=60, width=250,
                      command=lambda: self.set_role('editor')).pack(pady=15)
    
    def set_role(self, role):
        self.role = role
        if role == 'editor':
            self.ask_editor_name()
        else:
            self.select_folder()
    
    def ask_editor_name(self):
        """Ask editor for their name"""
        self.clear_window()
        
        frame = ctk.CTkFrame(self)
        frame.pack(expand=True, fill="both", padx=40, pady=40)
        
        ctk.CTkLabel(frame, text="🎨 Setup Editor", font=("", 24, "bold")).pack(pady=20)
        ctk.CTkLabel(frame, text="Cum te cheamă?", font=("", 16)).pack(pady=10)
        
        self.name_entry = ctk.CTkEntry(frame, width=250, font=("", 16), 
                                        placeholder_text="ex: Ana, Mihai...")
        self.name_entry.pack(pady=10)
        if self.editor_name:
            self.name_entry.insert(0, self.editor_name)
        
        ctk.CTkButton(frame, text="Continuă", font=("", 16), height=50, width=200,
                      command=self.save_editor_name).pack(pady=20)
    
    def save_editor_name(self):
        name = self.name_entry.get().strip()
        if name:
            self.editor_name = name
            self.select_folder()
        else:
            self.name_entry.configure(border_color="red")
    
    def select_folder(self):
        from tkinter import filedialog
        
        title = "Alege folderul pentru RAW-uri" if self.role == 'tagger' else "Alege folderul URGENT"
        folder = filedialog.askdirectory(title=title)
        
        if folder:
            self.base_path = folder
            self.save_config()
            self.show_main_ui()
        else:
            self.show_role_selection()
    
    def clear_window(self):
        for widget in self.winfo_children():
            widget.destroy()
    
    def show_main_ui(self):
        self.clear_window()
        
        # Header
        header = ctk.CTkFrame(self)
        header.pack(fill="x", padx=20, pady=10)
        
        role_icon = "🏷️" if self.role == 'tagger' else "🎨"
        role_name = "TAGGER" if self.role == 'tagger' else f"EDITOR: {self.editor_name}"
        ctk.CTkLabel(header, text=f"{role_icon} {role_name}", font=("", 20, "bold")).pack(side="left")
        
        ctk.CTkButton(header, text="🔄", width=40, command=self.reset_config).pack(side="right")
        ctk.CTkButton(header, text="⚙️", width=40, command=self.show_settings).pack(side="right", padx=5)
        
        # Day selector (tagger only, if day folders enabled)
        if self.role == 'tagger' and self.use_day_folders:
            day_frame = ctk.CTkFrame(self)
            day_frame.pack(fill="x", padx=20, pady=10)
            
            ctk.CTkLabel(day_frame, text="Ziua curentă:", font=("", 14)).pack(side="left", padx=10)
            
            # Generate day options based on prefix
            day_options = [f"{self.custom_day_prefix} {i}" for i in range(1, 6)]
            if self.current_day not in day_options:
                self.current_day = day_options[0]
            
            self.day_var = ctk.StringVar(value=self.current_day)
            self.day_menu = ctk.CTkOptionMenu(day_frame, values=day_options,
                                               variable=self.day_var, command=self.change_day,
                                               font=("", 14), width=150)
            self.day_menu.pack(side="left", padx=10)
            
            ctk.CTkButton(day_frame, text="+ Zi nouă", width=100, 
                         command=self.add_day).pack(side="right", padx=10)
        
        # Folder info & preview
        folder_frame = ctk.CTkFrame(self)
        folder_frame.pack(fill="x", padx=20, pady=10)
        
        ctk.CTkLabel(folder_frame, text="📁 " + str(self.base_path), font=("", 11),
                    wraplength=400).pack(padx=10, pady=(5,2))
        
        # Show folder preview
        preview = self.generate_folder_preview("ExampleName")
        self.preview_label = ctk.CTkLabel(folder_frame, text=f"Următorul folder: {preview}", 
                                          font=("", 10), text_color="gray")
        self.preview_label.pack(padx=10, pady=(0,5))
        
        # Status
        self.status_label = ctk.CTkLabel(self, text="⏳ Pornesc...", font=("", 14))
        self.status_label.pack(pady=5)
        
        # Send to Editor section (for both tagger and editor)
        send_frame = ctk.CTkFrame(self)
        send_frame.pack(fill="x", padx=20, pady=5)
        
        send_label = "📤 Trimite la Editori:" if self.role == 'tagger' else "📤 Trimite la alți Editori:"
        ctk.CTkLabel(send_frame, text=send_label, font=("", 12)).pack(side="left", padx=10)
        
        self.peer_status_label = ctk.CTkLabel(send_frame, text="⏳ caut...", 
                                                 font=("", 11), text_color="gray")
        self.peer_status_label.pack(side="left", padx=5)
        
        self.send_button = ctk.CTkButton(send_frame, text="Alege Folder", width=110,
                                         command=self.select_folder_for_send, state="disabled")
        self.send_button.pack(side="right", padx=10)
        
        # Tabview for Transfers, History, and Report
        self.tabview = ctk.CTkTabview(self, height=280)
        self.tabview.pack(fill="both", expand=True, padx=20, pady=10)
        
        self.tabview.add("📥 Transferuri")
        self.tabview.add("📋 Istoric")
        if self.role == 'tagger':
            self.tabview.add("📊 Raport")
        
        # Transfers tab - split into outgoing and incoming
        transfers_container = self.tabview.tab("📥 Transferuri")
        
        # Outgoing section
        ctk.CTkLabel(transfers_container, text="📤 Se trimit:", font=("", 12, "bold")).pack(anchor="w", pady=(5,2))
        self.outgoing_frame = ctk.CTkScrollableFrame(transfers_container, height=100)
        self.outgoing_frame.pack(fill="x", pady=(0,10))
        
        self.no_outgoing_label = ctk.CTkLabel(self.outgoing_frame, 
                                               text="Nimic în curs",
                                               font=("", 11), text_color="gray")
        self.no_outgoing_label.pack(pady=15)
        
        # Incoming section
        ctk.CTkLabel(transfers_container, text="📥 Se primesc:", font=("", 12, "bold")).pack(anchor="w", pady=(5,2))
        self.transfers_frame = ctk.CTkScrollableFrame(transfers_container)
        self.transfers_frame.pack(fill="both", expand=True)
        
        self.no_transfers_label = ctk.CTkLabel(self.transfers_frame, 
                                                text="Niciun transfer activ\nAștept...",
                                                font=("", 11), text_color="gray")
        self.no_transfers_label.pack(pady=30)
        
        # History tab
        history_header = ctk.CTkFrame(self.tabview.tab("📋 Istoric"), fg_color="transparent")
        history_header.pack(fill="x", pady=(5,5))
        
        ctk.CTkLabel(history_header, text=f"Total transferuri: {len(self.history)}", 
                    font=("", 12)).pack(side="left")
        
        ctk.CTkButton(history_header, text="🗑️ Șterge", width=80, 
                     command=self.clear_history).pack(side="right")
        
        self.history_frame = ctk.CTkScrollableFrame(self.tabview.tab("📋 Istoric"))
        self.history_frame.pack(fill="both", expand=True)
        
        self.refresh_history_ui()
        
        # Report tab (tagger only)
        if self.role == 'tagger':
            report_header = ctk.CTkFrame(self.tabview.tab("📊 Raport"), fg_color="transparent")
            report_header.pack(fill="x", pady=(5,5))
            
            ctk.CTkLabel(report_header, text="Statistici pe zi și fotograf", 
                        font=("", 12)).pack(side="left")
            
            ctk.CTkButton(report_header, text="📄 Export", width=80, 
                         command=self.export_report).pack(side="right")
            
            self.report_frame = ctk.CTkScrollableFrame(self.tabview.tab("📊 Raport"))
            self.report_frame.pack(fill="both", expand=True)
            
            self.refresh_report_ui()
        
        # Start server in thread to avoid blocking UI
        threading.Thread(target=self.start_server, daemon=True).start()
    
    def show_settings(self):
        """Show settings popup"""
        settings_window = ctk.CTkToplevel(self)
        settings_window.title("⚙️ Setări Folder")
        settings_window.geometry("450x500")
        settings_window.transient(self)
        settings_window.grab_set()
        
        # Template selection
        ctk.CTkLabel(settings_window, text="Template nume folder:", font=("", 14, "bold")).pack(anchor="w", padx=20, pady=(20,5))
        
        template_var = ctk.StringVar(value=self.folder_template)
        
        for template in DEFAULT_TEMPLATES:
            preview = self.format_folder_name(template, "Toni", 1)
            rb = ctk.CTkRadioButton(settings_window, text=f"{template}  →  {preview}", 
                                    variable=template_var, value=template)
            rb.pack(anchor="w", padx=30, pady=2)
        
        # Custom template
        ctk.CTkLabel(settings_window, text="Or custom template:", font=("", 12)).pack(anchor="w", padx=20, pady=(15,5))
        
        custom_frame = ctk.CTkFrame(settings_window, fg_color="transparent")
        custom_frame.pack(fill="x", padx=20, pady=5)
        
        custom_entry = ctk.CTkEntry(custom_frame, width=250, placeholder_text="{num:02d} - {name}")
        custom_entry.pack(side="left")
        
        def use_custom():
            if custom_entry.get().strip():
                template_var.set(custom_entry.get().strip())
        
        ctk.CTkButton(custom_frame, text="Use", width=60, command=use_custom).pack(side="left", padx=10)
        
        # Variables help
        help_text = """Variables:
  {name} - photographer name
  {num} or {num:02d} - folder number (01, 02...)
  {num:03d} - folder number 3 digits (001, 002...)
  {date} - date (2024-01-15)
  {time} - time (14-30)"""
        
        ctk.CTkLabel(settings_window, text=help_text, font=("", 11), justify="left",
                    text_color="gray").pack(anchor="w", padx=20, pady=10)
        
        # Day folders option (tagger only)
        if self.role == 'tagger':
            ctk.CTkLabel(settings_window, text="Day Organization:", font=("", 14, "bold")).pack(anchor="w", padx=20, pady=(15,5))
            
            use_days_var = ctk.BooleanVar(value=self.use_day_folders)
            ctk.CTkCheckBox(settings_window, text="Use day subfolders (DAY 1, DAY 2...)", 
                           variable=use_days_var).pack(anchor="w", padx=30, pady=2)
            
            reset_num_var = ctk.BooleanVar(value=self.reset_numbering_daily)
            ctk.CTkCheckBox(settings_window, text="Reset numbering each day", 
                           variable=reset_num_var).pack(anchor="w", padx=30, pady=2)
            
            # Day prefix
            prefix_frame = ctk.CTkFrame(settings_window, fg_color="transparent")
            prefix_frame.pack(fill="x", padx=20, pady=10)
            
            ctk.CTkLabel(prefix_frame, text="Day prefix:").pack(side="left")
            prefix_entry = ctk.CTkEntry(prefix_frame, width=100)
            prefix_entry.insert(0, self.custom_day_prefix)
            prefix_entry.pack(side="left", padx=10)
            ctk.CTkLabel(prefix_frame, text="(e.g., DAY, ZIUA, D)", text_color="gray").pack(side="left")
        
        # Save button
        def save_settings():
            self.folder_template = template_var.get()
            if self.role == 'tagger':
                self.use_day_folders = use_days_var.get()
                self.reset_numbering_daily = reset_num_var.get()
                self.custom_day_prefix = prefix_entry.get().strip() or "DAY"
            self.save_config()
            settings_window.destroy()
            self.show_main_ui()  # Refresh UI
        
        ctk.CTkButton(settings_window, text="💾 Salvează", font=("", 14), 
                     height=40, command=save_settings).pack(pady=20)
    
    def format_folder_name(self, template, name, num):
        """Format folder name using template"""
        now = datetime.now()
        
        # Replace variables
        result = template
        result = result.replace("{name}", name)
        result = result.replace("{date}", now.strftime("%Y-%m-%d"))
        result = result.replace("{time}", now.strftime("%H-%M"))
        
        # Handle {num} with optional formatting
        import re
        num_pattern = r'\{num(?::(\d+)d)?\}'
        match = re.search(num_pattern, result)
        if match:
            if match.group(1):
                digits = int(match.group(1))
                num_str = str(num).zfill(digits)
            else:
                num_str = str(num)
            result = re.sub(num_pattern, num_str, result)
        
        return result
    
    def generate_folder_preview(self, name):
        """Generate preview of next folder name"""
        next_num = self.get_next_folder_number()
        folder_name = self.format_folder_name(self.folder_template, name, next_num)
        
        if self.role == 'tagger' and self.use_day_folders:
            return f"{self.current_day}/{folder_name}"
        return folder_name
    
    def change_day(self, day):
        self.current_day = day
        self.save_config()
        # Update preview
        if hasattr(self, 'preview_label'):
            preview = self.generate_folder_preview("ExampleName")
            self.preview_label.configure(text=f"Next folder: {preview}")
    
    def add_day(self):
        current_days = list(self.day_menu.cget("values"))
        new_num = len(current_days) + 1
        new_day = f"{self.custom_day_prefix} {new_num}"
        current_days.append(new_day)
        self.day_menu.configure(values=current_days)
        self.day_var.set(new_day)
        self.change_day(new_day)
    
    def get_next_folder_number(self):
        """Get next folder number for current day/location"""
        if self.role == 'tagger' and self.use_day_folders:
            day_path = Path(self.base_path) / self.current_day
        else:
            day_path = Path(self.base_path)
        
        if not day_path.exists():
            return 1
        
        max_num = 0
        for folder in day_path.iterdir():
            if folder.is_dir():
                # Try to extract number from folder name
                name = folder.name
                import re
                # Find any number sequence in the name
                numbers = re.findall(r'\d+', name)
                for num_str in numbers:
                    try:
                        num = int(num_str)
                        max_num = max(max_num, num)
                    except:
                        pass
        
        return max_num + 1
    
    def reset_config(self):
        if self.config_path.exists():
            os.remove(self.config_path)
        self.role = None
        self.editor_name = ""
        self.base_path = None
        self.transfers = {}
        self.outgoing = {}
        self.peers = {}
        # Cancel peer browser if active
        if self.peer_browser:
            try:
                self.peer_browser.cancel()
            except:
                pass
            self.peer_browser = None
        self.stop_server()
        # Small delay to let zeroconf fully cleanup, then show role selection
        self.after(300, self._finish_reset)
    
    def _finish_reset(self):
        """Finish reset after cleanup delay"""
        self.zeroconf = Zeroconf()
        self.show_role_selection()
    
    def start_server(self):
        try:
            self.running = True
            
            # Start TCP server
            self.server_socket = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
            self.server_socket.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
            self.server_socket.bind(('0.0.0.0', PORT))
            self.server_socket.listen(10)
            
            # Get local IP
            s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
            try:
                s.connect(('8.8.8.8', 80))
                self.local_ip = s.getsockname()[0]
            finally:
                s.close()
            
            # Ensure zeroconf is initialized
            if not self.zeroconf:
                self.zeroconf = Zeroconf()
            
            # Register mDNS service with name (use timestamp to make unique)
            timestamp = int(time.time() * 1000) % 100000
            service_name = f"PhotoTransfer-{self.role.upper()}-{timestamp}._phototransfer._tcp.local."
            
            # Include editor name in properties
            props = {'role': self.role}
            if self.role == 'editor' and self.editor_name:
                props['name'] = self.editor_name
            
            self.service_info = ServiceInfo(
                SERVICE_TYPE,
                service_name,
                addresses=[socket.inet_aton(self.local_ip)],
                port=PORT,
                properties=props
            )
            
            self.zeroconf.register_service(self.service_info)
            
            # Update UI from main thread
            self.after(0, lambda: self.status_label.configure(text=f"✅ Online: {self.local_ip}:{PORT}"))
            
            # Start peer discovery (for tagger and editor)
            self.start_peer_discovery()
            
            # Accept connections in thread
            threading.Thread(target=self.accept_connections, daemon=True).start()
        
        except Exception as e:
            self.after(0, lambda: self.status_label.configure(text=f"❌ Error: {str(e)[:40]}"))
            print(f"Server error: {e}")
    
    def stop_server(self):
        self.running = False
        if self.zeroconf and self.service_info:
            try:
                self.zeroconf.unregister_service(self.service_info)
            except:
                pass
            try:
                self.zeroconf.close()
            except:
                pass
            self.zeroconf = None
            self.service_info = None
        if self.server_socket:
            try:
                self.server_socket.close()
            except:
                pass
            self.server_socket = None
    
    def accept_connections(self):
        while self.running:
            try:
                client_socket, addr = self.server_socket.accept()
                threading.Thread(target=self.handle_client, args=(client_socket, addr), daemon=True).start()
            except:
                break
    
    def handle_client(self, client_socket, addr):
        client_id = f"{addr[0]}:{addr[1]}"
        
        try:
            # Receive header
            header_size = struct.unpack('!I', client_socket.recv(4))[0]
            header_data = client_socket.recv(header_size).decode('utf-8')
            header = json.loads(header_data)
            
            photographer_name = header['photographer']
            files = header['files']  # [{name, size, checksum}, ...]
            total_size = sum(f['size'] for f in files)
            
            # Check if it's a folder transfer (from tagger/editor)
            is_folder_transfer = header.get('is_folder_transfer', False)
            folder_name = header.get('folder_name', None)
            
            # Create destination folder
            if is_folder_transfer and folder_name:
                # Preserve the original folder name
                dest_folder = Path(self.base_path) / folder_name
                dest_folder.mkdir(parents=True, exist_ok=True)
            else:
                # Regular transfer from photographer - use template
                dest_folder = self.create_destination_folder(photographer_name)
            
            # Send ACK with folder info
            ack = json.dumps({'status': 'ready', 'folder': str(dest_folder)}).encode('utf-8')
            client_socket.send(struct.pack('!I', len(ack)))
            client_socket.send(ack)
            
            # Update UI
            self.add_transfer(client_id, photographer_name, len(files), total_size, dest_folder)
            
            # Receive files
            received_size = 0
            for file_info in files:
                file_name = file_info['name']  # May contain subdirectory for folder transfers
                file_size = file_info['size']
                expected_checksum = file_info['checksum']
                
                file_path = dest_folder / file_name
                # Create subdirectory if needed
                file_path.parent.mkdir(parents=True, exist_ok=True)
                
                hasher = hashlib.md5()
                
                with open(file_path, 'wb') as f:
                    remaining = file_size
                    while remaining > 0:
                        chunk = client_socket.recv(min(CHUNK_SIZE, remaining))
                        if not chunk:
                            raise Exception("Connection lost")
                        f.write(chunk)
                        hasher.update(chunk)
                        remaining -= len(chunk)
                        received_size += len(chunk)
                        
                        # Update progress
                        self.update_transfer(client_id, received_size, total_size)
                
                # Verify checksum
                if hasher.hexdigest() != expected_checksum:
                    client_socket.send(b'CHECKSUM_ERROR')
                    raise Exception(f"Checksum mismatch: {file_name}")
                else:
                    client_socket.send(b'OK')
            
            # Done
            self.complete_transfer(client_id, photographer_name, len(files))
            
        except Exception as e:
            self.fail_transfer(client_id, str(e))
        finally:
            client_socket.close()
    
    def create_destination_folder(self, photographer_name):
        folder_num = self.get_next_folder_number()
        folder_name = self.format_folder_name(self.folder_template, photographer_name, folder_num)
        
        if self.role == 'tagger' and self.use_day_folders:
            dest = Path(self.base_path) / self.current_day / folder_name
        else:
            dest = Path(self.base_path) / folder_name
        
        dest.mkdir(parents=True, exist_ok=True)
        
        # Update preview after creating folder
        self.after(0, self.update_preview)
        
        return dest
    
    def update_preview(self):
        if hasattr(self, 'preview_label'):
            preview = self.generate_folder_preview("ExampleName")
            self.preview_label.configure(text=f"Next folder: {preview}")
    
    def add_transfer(self, client_id, name, file_count, total_size, folder):
        self.transfers[client_id] = {
            'name': name,
            'file_count': file_count,
            'total_size': total_size,
            'received': 0,
            'status': 'transferring',
            'folder': folder,
            'start_time': time.time(),
            'speed': 0,
            'eta': ''
        }
        self.after(0, self.refresh_transfers_ui)
    
    def update_transfer(self, client_id, received, total):
        if client_id in self.transfers:
            transfer = self.transfers[client_id]
            transfer['received'] = received
            
            # Calculate speed and ETA
            elapsed = time.time() - transfer['start_time']
            if elapsed > 0:
                speed = received / elapsed
                transfer['speed'] = speed
                
                remaining_bytes = total - received
                if speed > 0:
                    eta_seconds = remaining_bytes / speed
                    if eta_seconds < 60:
                        transfer['eta'] = f"{int(eta_seconds)}s"
                    else:
                        transfer['eta'] = f"{int(eta_seconds // 60)}m {int(eta_seconds % 60)}s"
            
            self.after(0, self.refresh_transfers_ui)
    
    def complete_transfer(self, client_id, name, file_count):
        if client_id in self.transfers:
            transfer = self.transfers[client_id]
            transfer['status'] = 'done'
            
            # Add to history
            day = self.current_day if (self.role == 'tagger' and self.use_day_folders) else None
            self.add_to_history(
                photographer=name,
                file_count=file_count,
                total_size=transfer['total_size'],
                folder=transfer['folder'],
                day=day
            )
            
            self.after(0, self.refresh_transfers_ui)
            # Remove after 10 seconds
            self.after(10000, lambda: self.remove_transfer(client_id))
    
    def fail_transfer(self, client_id, error):
        if client_id in self.transfers:
            self.transfers[client_id]['status'] = f'error: {error}'
            self.after(0, self.refresh_transfers_ui)
    
    def remove_transfer(self, client_id):
        if client_id in self.transfers:
            del self.transfers[client_id]
            self.refresh_transfers_ui()
    
    def refresh_transfers_ui(self):
        # Clear frame
        for widget in self.transfers_frame.winfo_children():
            widget.destroy()
        
        if not self.transfers:
            self.no_transfers_label = ctk.CTkLabel(self.transfers_frame, 
                                                    text="Niciun transfer activ\nAștept fotografi...",
                                                    font=("", 12), text_color="gray")
            self.no_transfers_label.pack(pady=50)
            return
        
        for client_id, info in self.transfers.items():
            frame = ctk.CTkFrame(self.transfers_frame)
            frame.pack(fill="x", pady=5, padx=5)
            
            # Name
            ctk.CTkLabel(frame, text=f"📷 {info['name']}", font=("", 14, "bold")).pack(anchor="w", padx=10, pady=(5,0))
            
            # Progress
            if info['status'] == 'done':
                ctk.CTkLabel(frame, text=f"✅ Gata - {info['file_count']} fișiere", 
                           text_color="green").pack(anchor="w", padx=10)
            elif info['status'].startswith('error'):
                ctk.CTkLabel(frame, text=f"❌ {info['status']}", 
                           text_color="red").pack(anchor="w", padx=10)
            else:
                progress = info['received'] / info['total_size'] if info['total_size'] > 0 else 0
                size_mb = info['total_size'] / (1024*1024)
                received_mb = info['received'] / (1024*1024)
                speed_mb = info.get('speed', 0) / (1024*1024)
                eta = info.get('eta', '')
                
                progress_bar = ctk.CTkProgressBar(frame, width=300)
                progress_bar.set(progress)
                progress_bar.pack(padx=10, pady=2)
                
                status_text = f"{received_mb:.0f} / {size_mb:.0f} MB ({progress*100:.0f}%)"
                if speed_mb > 0:
                    status_text += f"  •  {speed_mb:.1f} MB/s"
                if eta:
                    status_text += f"  •  {eta} rămas"
                
                ctk.CTkLabel(frame, text=status_text,
                           font=("", 11)).pack(anchor="w", padx=10, pady=(0,5))
    
    def refresh_history_ui(self):
        """Refresh the history list UI"""
        if not hasattr(self, 'history_frame'):
            return
            
        # Clear frame
        for widget in self.history_frame.winfo_children():
            widget.destroy()
        
        if not self.history:
            ctk.CTkLabel(self.history_frame, 
                        text="Niciun transfer încă",
                        font=("", 12), text_color="gray").pack(pady=50)
            return
        
        # Show last 50 entries in UI - compact format
        for entry in self.history[:50]:
            frame = ctk.CTkFrame(self.history_frame, height=36)
            frame.pack(fill="x", pady=2, padx=5)
            frame.pack_propagate(False)
            
            # Parse timestamp
            try:
                dt = datetime.fromisoformat(entry['timestamp'])
                time_str = dt.strftime("%H:%M")
                date_str = dt.strftime("%d/%m")
            except:
                time_str = ""
                date_str = ""
            
            # Single row with all info
            size_mb = entry['total_size'] / (1024*1024)
            folder_name = Path(entry['folder']).name if entry.get('folder') else ""
            
            # Left side: photographer + folder
            left = ctk.CTkFrame(frame, fg_color="transparent")
            left.pack(side="left", fill="y", padx=10)
            
            ctk.CTkLabel(left, text=f"📷 {entry['photographer']}", 
                        font=("", 12, "bold")).pack(side="left")
            
            ctk.CTkLabel(left, text=f"  →  📁 {folder_name}", 
                        font=("", 11), text_color="gray").pack(side="left")
            
            # Right side: stats + date
            right = ctk.CTkFrame(frame, fg_color="transparent")
            right.pack(side="right", fill="y", padx=10)
            
            stats = f"{entry['file_count']} fișiere • {size_mb:.1f} MB"
            ctk.CTkLabel(right, text=f"{date_str} {time_str}", 
                        font=("", 10), text_color="gray").pack(side="right")
            
            ctk.CTkLabel(right, text=stats, 
                        font=("", 10), text_color="gray").pack(side="right", padx=(0, 15))
    
    def clear_history(self):
        """Clear all history"""
        if self.history:
            self.history = []
            self.save_history()
            self.refresh_history_ui()
            if hasattr(self, 'report_frame'):
                self.refresh_report_ui()
    
    # ========== Peer Discovery and Folder Sending ==========
    
    def start_peer_discovery(self):
        """Start discovering other editors on the network"""
        self.peer_listener = PeerDiscovery(self.on_peers_updated, self.role, self.local_ip)
        self.peer_browser = ServiceBrowser(self.zeroconf, SERVICE_TYPE, self.peer_listener)
    
    def on_peers_updated(self, peers):
        """Called when peers are found/lost"""
        self.peers = peers
        self.after(0, self.update_peer_status)
    
    def update_peer_status(self):
        """Update peer status in UI"""
        if not hasattr(self, 'peer_status_label'):
            return
        
        if self.peers:
            peer_count = len(self.peers)
            names = ", ".join([f"{p['name']}" for p in self.peers.values()])
            self.peer_status_label.configure(text=f"✅ {peer_count}: {names}")
            self.send_button.configure(state="normal")
        else:
            self.peer_status_label.configure(text="⏳ caut...")
            self.send_button.configure(state="disabled")
    
    def select_folder_for_send(self):
        """Open folder dialog to select folder to send"""
        if not self.peers:
            return
        
        folder = filedialog.askdirectory(title="Alege folderul de trimis")
        
        if folder:
            # If multiple peers, let user choose
            if len(self.peers) > 1:
                self.show_peer_selection(folder)
            else:
                # Send to the only peer
                peer = list(self.peers.values())[0]
                self.send_folder_to_peer(folder, peer)
    
    def show_peer_selection(self, folder):
        """Show dialog to select which peer(s) to send to"""
        select_window = ctk.CTkToplevel(self)
        select_window.title("Alege destinația")
        select_window.geometry("350x250")
        select_window.transient(self)
        select_window.grab_set()
        
        folder_name = Path(folder).name
        ctk.CTkLabel(select_window, text=f"Send '{folder_name}' to:", 
                    font=("", 14, "bold")).pack(pady=15)
        
        for name, peer in self.peers.items():
            display = f"🎨 {peer['name']} ({peer['ip']})"
            btn = ctk.CTkButton(select_window, text=display, width=280,
                               command=lambda p=peer, w=select_window: self.on_peer_selected(folder, p, w))
            btn.pack(pady=5)
        
        # Send to all button
        ctk.CTkButton(select_window, text="📤 Trimite la TOȚI", fg_color="green", width=280,
                     command=lambda: self.send_to_all_peers(folder, select_window)).pack(pady=15)
    
    def on_peer_selected(self, folder, peer, window):
        window.destroy()
        self.send_folder_to_peer(folder, peer)
    
    def send_to_all_peers(self, folder, window):
        window.destroy()
        for peer in self.peers.values():
            self.send_folder_to_peer(folder, peer)
    
    def send_folder_to_peer(self, folder, peer):
        """Send folder to a peer"""
        transfer_id = str(uuid.uuid4())[:8]
        folder_name = Path(folder).name
        
        # Add to outgoing transfers
        self.outgoing[transfer_id] = {
            'folder': folder_name,
            'destination': peer['name'],
            'sent': 0,
            'total': 0,
            'status': 'preparing'
        }
        self.after(0, self.refresh_outgoing_ui)
        
        threading.Thread(target=self._send_folder_thread, args=(folder, peer, transfer_id), daemon=True).start()
    
    def _send_folder_thread(self, folder, peer, transfer_id):
        """Thread to send folder to peer"""
        try:
            folder_path = Path(folder)
            folder_name = folder_path.name
            
            # Collect all files in folder
            file_infos = []
            for file_path in folder_path.rglob('*'):
                if file_path.is_file():
                    size = file_path.stat().st_size
                    rel_path = file_path.relative_to(folder_path)
                    
                    hasher = hashlib.md5()
                    with open(file_path, 'rb') as f:
                        while chunk := f.read(CHUNK_SIZE):
                            hasher.update(chunk)
                    
                    file_infos.append({
                        'name': str(rel_path),  # Relative path to preserve structure
                        'size': size,
                        'checksum': hasher.hexdigest(),
                        'path': str(file_path)
                    })
            
            if not file_infos:
                raise Exception("Folder is empty")
            
            total_size = sum(f['size'] for f in file_infos)
            
            # Update total size
            self.outgoing[transfer_id]['total'] = total_size
            self.outgoing[transfer_id]['status'] = 'connecting'
            self.after(0, self.refresh_outgoing_ui)
            
            # Connect
            sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
            sock.connect((peer['ip'], peer['port']))
            
            # Send header with folder_name to preserve it
            sender_name = 'TAGGER' if self.role == 'tagger' else self.editor_name
            header = {
                'photographer': sender_name,
                'folder_name': folder_name,  # Preserve folder name!
                'is_folder_transfer': True,
                'files': [{'name': f['name'], 'size': f['size'], 'checksum': f['checksum']} for f in file_infos]
            }
            header_data = json.dumps(header).encode('utf-8')
            sock.send(struct.pack('!I', len(header_data)))
            sock.send(header_data)
            
            # Wait for ACK
            ack_size = struct.unpack('!I', sock.recv(4))[0]
            ack = json.loads(sock.recv(ack_size).decode('utf-8'))
            
            if ack.get('status') != 'ready':
                raise Exception("Peer not ready")
            
            # Update status
            self.outgoing[transfer_id]['status'] = 'sending'
            self.after(0, self.refresh_outgoing_ui)
            
            # Send files
            sent_size = 0
            for file_info in file_infos:
                with open(file_info['path'], 'rb') as f:
                    remaining = file_info['size']
                    while remaining > 0:
                        chunk = f.read(min(CHUNK_SIZE, remaining))
                        sock.send(chunk)
                        remaining -= len(chunk)
                        sent_size += len(chunk)
                        
                        # Update progress
                        self.outgoing[transfer_id]['sent'] = sent_size
                        self.after(0, self.refresh_outgoing_ui)
                
                # Wait for confirmation
                response = sock.recv(16)
                if response != b'OK':
                    raise Exception(f"Transfer error")
            
            sock.close()
            
            # Mark as done
            self.outgoing[transfer_id]['status'] = 'done'
            self.after(0, self.refresh_outgoing_ui)
            
            # Remove after 5 seconds
            self.after(5000, lambda: self.remove_outgoing(transfer_id))
            
        except Exception as e:
            if transfer_id in self.outgoing:
                self.outgoing[transfer_id]['status'] = f'error: {str(e)[:20]}'
                self.after(0, self.refresh_outgoing_ui)
                self.after(5000, lambda: self.remove_outgoing(transfer_id))
    
    def remove_outgoing(self, transfer_id):
        """Remove completed outgoing transfer"""
        if transfer_id in self.outgoing:
            del self.outgoing[transfer_id]
            self.refresh_outgoing_ui()
    
    def refresh_outgoing_ui(self):
        """Refresh outgoing transfers UI"""
        if not hasattr(self, 'outgoing_frame'):
            return
        
        # Clear frame
        for widget in self.outgoing_frame.winfo_children():
            widget.destroy()
        
        if not self.outgoing:
            self.no_outgoing_label = ctk.CTkLabel(self.outgoing_frame, 
                                                   text="Nimic în curs",
                                                   font=("", 11), text_color="gray")
            self.no_outgoing_label.pack(pady=15)
            return
        
        for transfer_id, info in self.outgoing.items():
            frame = ctk.CTkFrame(self.outgoing_frame, fg_color="transparent")
            frame.pack(fill="x", pady=2, padx=10)
            
            # Header: folder → destination
            header_text = f"{info['folder']} → {info['destination']}"
            ctk.CTkLabel(frame, text=header_text, font=("", 11, "bold")).pack(anchor="w")
            
            if info['status'] == 'done':
                ctk.CTkLabel(frame, text="✅ Sent!", text_color="green", 
                           font=("", 10)).pack(anchor="w")
            elif info['status'].startswith('error'):
                ctk.CTkLabel(frame, text=f"❌ {info['status']}", text_color="red",
                           font=("", 10)).pack(anchor="w")
            elif info['status'] in ('preparing', 'connecting'):
                ctk.CTkLabel(frame, text=f"⏳ {info['status']}...", text_color="gray",
                           font=("", 10)).pack(anchor="w")
            else:
                # Show progress
                progress = info['sent'] / info['total'] if info['total'] > 0 else 0
                sent_mb = info['sent'] / (1024*1024)
                total_mb = info['total'] / (1024*1024)
                
                progress_frame = ctk.CTkFrame(frame, fg_color="transparent")
                progress_frame.pack(fill="x")
                
                progress_bar = ctk.CTkProgressBar(progress_frame, width=200, height=12)
                progress_bar.set(progress)
                progress_bar.pack(side="left")
                
                ctk.CTkLabel(progress_frame, text=f" {sent_mb:.0f}/{total_mb:.0f} MB ({progress*100:.0f}%)",
                           font=("", 10)).pack(side="left")
    
    # ========== Report Generation ==========
    
    def generate_report_data(self):
        """Generate report data grouped by day and photographer"""
        report = defaultdict(lambda: defaultdict(lambda: {'transfers': 0, 'files': 0, 'size': 0}))
        
        for entry in self.history:
            day = entry.get('day') or 'No Day'
            photographer = entry['photographer']
            
            report[day][photographer]['transfers'] += 1
            report[day][photographer]['files'] += entry['file_count']
            report[day][photographer]['size'] += entry['total_size']
        
        return report
    
    def refresh_report_ui(self):
        """Refresh the report UI"""
        if not hasattr(self, 'report_frame'):
            return
        
        # Clear frame
        for widget in self.report_frame.winfo_children():
            widget.destroy()
        
        report = self.generate_report_data()
        
        if not report:
            ctk.CTkLabel(self.report_frame, 
                        text="No data yet",
                        font=("", 12), text_color="gray").pack(pady=50)
            return
        
        # Sort days
        days = sorted(report.keys(), key=lambda x: x if x != 'No Day' else 'ZZZ')
        
        for day in days:
            # Day header
            day_frame = ctk.CTkFrame(self.report_frame)
            day_frame.pack(fill="x", pady=5, padx=5)
            
            day_total_files = sum(p['files'] for p in report[day].values())
            day_total_size = sum(p['size'] for p in report[day].values()) / (1024*1024*1024)  # GB
            
            ctk.CTkLabel(day_frame, text=f"📅 {day}", 
                        font=("", 14, "bold")).pack(anchor="w", padx=10, pady=(5,2))
            
            ctk.CTkLabel(day_frame, text=f"Total: {day_total_files} files • {day_total_size:.2f} GB", 
                        font=("", 11), text_color="gray").pack(anchor="w", padx=10, pady=(0,5))
            
            # Photographers
            for photographer, stats in sorted(report[day].items()):
                p_frame = ctk.CTkFrame(day_frame, fg_color="transparent")
                p_frame.pack(fill="x", padx=20, pady=2)
                
                size_mb = stats['size'] / (1024*1024)
                
                ctk.CTkLabel(p_frame, text=f"📷 {photographer}", 
                            font=("", 12)).pack(side="left")
                
                ctk.CTkLabel(p_frame, text=f"{stats['transfers']}x • {stats['files']} files • {size_mb:.1f} MB", 
                            font=("", 11), text_color="gray").pack(side="right")
    
    def export_report(self):
        """Export report to a text file"""
        report = self.generate_report_data()
        
        if not report:
            return
        
        # Build report text
        lines = ["=" * 50]
        lines.append("TRANSFER REPORT")
        lines.append(f"Generated: {datetime.now().strftime('%Y-%m-%d %H:%M')}")
        lines.append("=" * 50)
        lines.append("")
        
        days = sorted(report.keys(), key=lambda x: x if x != 'No Day' else 'ZZZ')
        
        grand_total_files = 0
        grand_total_size = 0
        
        for day in days:
            day_total_files = sum(p['files'] for p in report[day].values())
            day_total_size = sum(p['size'] for p in report[day].values())
            grand_total_files += day_total_files
            grand_total_size += day_total_size
            
            lines.append(f"📅 {day}")
            lines.append("-" * 30)
            
            for photographer, stats in sorted(report[day].items()):
                size_mb = stats['size'] / (1024*1024)
                lines.append(f"  {photographer}: {stats['transfers']} transfers, {stats['files']} files, {size_mb:.1f} MB")
            
            lines.append(f"  TOTAL: {day_total_files} files, {day_total_size/(1024*1024*1024):.2f} GB")
            lines.append("")
        
        lines.append("=" * 50)
        lines.append(f"GRAND TOTAL: {grand_total_files} files, {grand_total_size/(1024*1024*1024):.2f} GB")
        lines.append("=" * 50)
        
        # Save to file
        file_path = filedialog.asksaveasfilename(
            title="Salvează Raport",
            defaultextension=".txt",
            filetypes=[("Text files", "*.txt")],
            initialfilename=f"transfer_report_{datetime.now().strftime('%Y%m%d_%H%M')}.txt"
        )
        
        if file_path:
            with open(file_path, 'w', encoding='utf-8') as f:
                f.write("\n".join(lines))
    
    def on_closing(self):
        self.stop_server()
        if self.peer_browser:
            try:
                self.peer_browser.cancel()
            except:
                pass
        self.destroy()


if __name__ == "__main__":
    ctk.set_appearance_mode("dark")
    ctk.set_default_color_theme("blue")
    
    app = ReceiverApp()
    app.protocol("WM_DELETE_WINDOW", app.on_closing)
    app.mainloop()
