# Quick Build Guide

## Step 1: Build on Windows PC

```cmd
pip install -r requirements.txt
build_windows.bat
```

Copiază din `dist/`:
- ✅ `PhotoSender/` folder → pentru fotografi Windows
- ✅ `PhotoReceiver/` folder → pentru tagger

---

## Step 2: Build on Mac

```bash
pip3 install -r requirements.txt
chmod +x build_mac.sh
./build_mac.sh
```

Copiază din `dist/`:
- ✅ `PhotoSender/PhotoSender.app` → pentru fotografi Mac
- ✅ `PhotoReceiver/PhotoReceiver.app` → pentru editori

---

## Step 3: Organizează pe USB

```
USB Stick/
├── Fotografi/
│   ├── Windows/
│   │   └── PhotoSender/        (întreg folderul)
│   │       └── PhotoSender.exe
│   └── Mac/
│       └── PhotoSender.app
├── Tagger/
│   └── PhotoReceiver/          (întreg folderul)
│       └── PhotoReceiver.exe
└── Editori/
    └── PhotoReceiver.app
```

**Notă:** Pentru Windows trebuie copiat **întreg folderul**, nu doar .exe-ul!

---

## La eveniment

1. **Tagger** rulează `PhotoReceiver.exe` din folder
2. **Editori** rulează `PhotoReceiver.app`
3. **Fotografi** rulează `PhotoSender` (.exe din folder sau .app)
4. Totul se găsește automat în rețea! 🎉

---

## De ce foldere în loc de fișier unic?

- **Pornire rapidă** (~1-2 secunde vs 10+ secunde)
- Fișierele sunt deja extrase, nu mai așteaptă
