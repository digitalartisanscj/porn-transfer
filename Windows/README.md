# 🔥 Pornhub Transfer

Aplicație simplă pentru transfer rapid de fișiere RAW între fotografi, tagger și editor la evenimente.

> *Numele e pentru moralul echipei, nu pentru ce credeți voi.*

## Instalare

### Cerințe
- Python 3.9+
- pip

### Setup (pe toate mașinile)

```bash
# 1. Clonează sau copiază folderul
# 2. Instalează dependențele:
pip install -r requirements.txt
```

**Pe Mac**, dacă primești erori:
```bash
pip3 install -r requirements.txt
```

**Notă pentru drag & drop:** Librăria `tkinterdnd2` e necesară pentru drag & drop. Pe Mac poate necesita instalare specială:
```bash
pip3 install tkinterdnd2
```
Dacă drag & drop nu merge, click pe zone funcționează întotdeauna.

## Utilizare

### 1. Pornește Receiver-ul (Tagger + Editor)

**Pe PC-ul Tagger-ului (Windows):**
```bash
python receiver.py
```

**Pe Mac-ul Editorului:**
```bash
python3 receiver.py
```

La prima pornire:
1. Selectează rolul (Tagger sau Editor)
2. Alege folderul unde să salveze fișierele

### 2. Configurare foldere (Tagger)

Click pe ⚙️ pentru a configura:

**Template-uri disponibile:**
- `{num:02d} - {name}` → "01 - Toni"
- `{name}_{num:03d}` → "Toni_001"  
- `{date}_{num:02d} - {name}` → "2024-01-15_01 - Toni"
- `{name}` → "Toni" (fără număr)
- Sau template custom

**Variabile:**
- `{name}` - numele fotografului
- `{num}` sau `{num:02d}` - numărul folderului (01, 02...)
- `{num:03d}` - număr cu 3 cifre (001, 002...)
- `{date}` - data (2024-01-15)
- `{time}` - ora (14-30)

**Opțiuni suplimentare:**
- Use day subfolders - organizare pe zile (DAY 1, DAY 2...)
- Reset numbering each day - numerotare de la 1 în fiecare zi
- Day prefix - prefixul zilelor (DAY, ZIUA, D...)

### 3. Pornește Sender-ul (Fotografi)

```bash
python sender.py
# sau pe Mac:
python3 sender.py
```

La prima pornire:
1. Introdu-ți numele

Aplicația găsește automat Tagger-ul și Editorul în rețea.

### 4. Trimite fișiere

- Click pe zona **TAGGER** sau **EDITOR**
- Selectează fișierele RAW
- Așteaptă să se termine transferul

## Structura folderelor

### La Tagger (exemplu cu template implicit):
```
RAWs/
├── DAY 1/
│   ├── 01 - Toni/
│   ├── 02 - Alex/
│   └── 03 - Toni/      (a doua sesiune)
├── DAY 2/
│   └── 01 - Maria/
```

### La Editor:
```
URGENT/
├── Toni_001/
├── Alex_001/
└── Toni_002/
```

## Networking

- Toate dispozitivele trebuie să fie în aceeași rețea locală
- Portul folosit: **45678** (TCP)
- Serviciul mDNS: `_phototransfer._tcp.local.`

### Dacă discovery-ul nu funcționează:

Pe Windows, verifică firewall-ul:
```
Windows Defender Firewall > Allow an app > Python
```

Pe Mac, permite conexiuni incoming când apare prompt-ul.

## Butoane UI

- ⚙️ - Setări foldere
- 🔄 - Reset configurație (reconfigurează de la zero)

## Istoric Transferuri (Tagger)

Tagger-ul are un tab "📋 History" care arată:
- Toate transferurile completate
- Timestamp, fotograf, număr fișiere, size
- Ziua în care s-a făcut transferul
- Buton de clear pentru a șterge istoricul

Istoricul se salvează în `~/.photo_transfer_history.json` și persistă între sesiuni.

## Raport Statistici (Tagger)

Tab-ul "📊 Report" arată statistici grupate:
- Per zi (DAY 1, DAY 2, etc.)
- Per fotograf
- Număr de transferuri, fișiere totale, size total

Butonul "📄 Export" salvează raportul într-un fișier text.

## Tagger → Editor

Tagger-ul poate trimite **foldere** direct la editori:
- Click "Select Folder" pentru a alege un folder
- Denumirea folderului se păstrează la destinație
- Dacă sunt 2+ editori, poți alege la care să trimiți
- Opțiune "Send to ALL" pentru a trimite la toți

## Editor → Editor

Editorii pot trimite foldere între ei:
- La setup, fiecare editor își pune un nume (ex: "Ana", "Mihai")
- Editorii se văd automat în rețea
- Denumirea folderului se păstrează

**Diferența față de fotografi:**
- Fotografii trimit **fișiere** → se creează folder nou cu template
- Tagger/Editori trimit **foldere** → denumirea se păstrează

## Transfer Tab (Simultaneous)

Tab-ul "📥 Transfers" arată ambele direcții:

```
┌─────────────────────────────────────────┐
│ 📤 Sending:                             │
│ ┌─────────────────────────────────────┐ │
│ │ 01 - Toni → Ana                     │ │
│ │ ████████░░░░ 156/240 MB (65%)       │ │
│ ├─────────────────────────────────────┤ │
│ │ 02 - Maria → Mihai                  │ │
│ │ ████░░░░░░░░ 45/180 MB (25%)        │ │
│ └─────────────────────────────────────┘ │
│                                         │
│ 📥 Receiving:                           │
│ ┌─────────────────────────────────────┐ │
│ │ 📷 Alex                             │ │
│ │ ██████████░░ 89/120 MB (74%)        │ │
│ └─────────────────────────────────────┘ │
└─────────────────────────────────────────┘
```

- Poți primi și trimite **simultan**
- Mai multe transferuri în paralel
- Progress bar pentru fiecare transfer

## Resetare configurație

Șterge fișierele de configurare pentru a reseta:

**Receiver:**
- Windows: `%USERPROFILE%\.photo_transfer_receiver.json`
- Mac: `~/.photo_transfer_receiver.json`

**Sender:**
- Windows: `%USERPROFILE%\.photo_transfer_sender.json`  
- Mac: `~/.photo_transfer_sender.json`

## Detectare Duplicate (Sender)

Sender-ul ține un log al fișierelor trimise în ziua respectivă.

Dacă fotograful încearcă să trimită fișiere deja trimise:
```
┌─────────────────────────────────────────┐
│  ⚠️                                      │
│  5 files already sent today!            │
│  12 new files to send                   │
│                                         │
│  • IMG_1234.ARW                         │
│  • IMG_1235.ARW                         │
│  • ...                                  │
│                                         │
│  [📤 Send only new (12)] [📤 Send ALL] │
│                              [Cancel]   │
└─────────────────────────────────────────┘
```

**Opțiuni:**
- **Send only new** - trimite doar fișierele care nu au fost trimise
- **Send ALL** - trimite tot, inclusiv duplicatele
- **Cancel** - nu trimite nimic

Log-ul se resetează automat la miezul nopții (fișier nou per zi).

## Troubleshooting

### "Not connected"
- Verifică că receiver-ul rulează
- Verifică că ești în aceeași rețea
- Verifică firewall-ul

### Transfer lent
- Folosește cablu în loc de WiFi
- Verifică că ai switch gigabit sau 2.5G

### Aplicația nu pornește
```bash
# Verifică versiunea Python
python --version  # trebuie 3.9+

# Reinstalează dependențele
pip install --upgrade -r requirements.txt
```
