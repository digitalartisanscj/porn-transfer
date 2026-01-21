# Building Executables

## Requirements

1. Python 3.9+ installed
2. All dependencies installed: `pip install -r requirements.txt`
3. PyInstaller: `pip install pyinstaller`

---

## Windows (.exe)

### Quick build:
```cmd
build_windows.bat
```

### Manual build:
```cmd
pip install pyinstaller
pyinstaller --onefile --windowed --name "PhotoSender" sender.py
pyinstaller --onefile --windowed --name "PhotoReceiver" receiver.py
```

### Output:
- `dist/PhotoSender.exe` - pentru fotografi
- `dist/PhotoReceiver.exe` - pentru tagger/editor

---

## Mac (.app)

### Quick build:
```bash
chmod +x build_mac.sh
./build_mac.sh
```

### Manual build:
```bash
pip3 install pyinstaller
pyinstaller --onefile --windowed --name "PhotoSender" sender.py
pyinstaller --onefile --windowed --name "PhotoReceiver" receiver.py
```

### Output:
- `dist/PhotoSender.app` - pentru fotografi
- `dist/PhotoReceiver.app` - pentru tagger/editor

---

## Using spec files (advanced)

For more control:
```bash
pyinstaller PhotoSender.spec
pyinstaller PhotoReceiver.spec
```

---

## Troubleshooting

### "App is damaged" on Mac
Mac blochează aplicațiile nesemnate. Fix:
```bash
xattr -cr /path/to/PhotoSender.app
xattr -cr /path/to/PhotoReceiver.app
```

Sau: System Preferences → Security → "Open Anyway"

### Missing modules
Dacă lipsesc module, adaugă în spec file:
```python
hiddenimports=['module_name'],
```

### Antivirus on Windows
Unele antivirusuri blochează PyInstaller executables.
Adaugă excepție sau semnează executabilul.

### Large file size
Executabilele sunt ~50-100MB (includ Python + toate dependențele).
Normal pentru PyInstaller.

---

## Distribution

Trebuie compilat pe **ambele platforme** (Windows + Mac):

### Compilezi pe Windows:
- `PhotoSender.exe` → pentru fotografi Windows
- `PhotoReceiver.exe` → pentru tagger

### Compilezi pe Mac:
- `PhotoSender.app` → pentru fotografi Mac  
- `PhotoReceiver.app` → pentru editori

### Cine primește ce:

| Rol | Fișier | Platformă |
|-----|--------|-----------|
| Fotograf (Windows) | `PhotoSender.exe` | Windows |
| Fotograf (Mac) | `PhotoSender.app` | Mac |
| Tagger | `PhotoReceiver.exe` | Windows |
| Editor | `PhotoReceiver.app` | Mac |

### Cum distribui:
- USB stick cu toate executabilele
- Network share / Google Drive
- AirDrop pentru Mac-uri

---

## Code Signing (optional, pentru distribuție profesională)

### Mac:
```bash
codesign --deep --force --sign "Developer ID Application: Your Name" PhotoSender.app
```

### Windows:
Folosește `signtool.exe` cu un certificat code signing.
