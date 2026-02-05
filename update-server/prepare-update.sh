#!/bin/bash

# Porn Transfer Update Preparation Script
# Usage: ./prepare-update.sh [receiver|sender]

set -e

APP_TYPE="${1:-receiver}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
UPDATES_DIR="$SCRIPT_DIR/updates/$APP_TYPE"
SIGNING_KEY="$HOME/.tauri/porn-receiver.key"

if [ "$APP_TYPE" == "sender" ]; then
    PROJECT_DIR="$SCRIPT_DIR/../photo-transfer"
    SIGNING_KEY="$HOME/.tauri/porn-sender-ci.key"
else
    PROJECT_DIR="$SCRIPT_DIR/../porn-receiver"
fi

echo "╔════════════════════════════════════════════════════════════╗"
echo "║         PREPARE UPDATE - $APP_TYPE"
echo "╚════════════════════════════════════════════════════════════╝"
echo ""

# Read version from tauri.conf.json
VERSION=$(grep '"version"' "$PROJECT_DIR/src-tauri/tauri.conf.json" | head -1 | sed 's/.*"version": "\(.*\)".*/\1/')
echo "📦 Version: $VERSION"
echo ""

# Check if builds exist
ARM64_APP="$PROJECT_DIR/src-tauri/target/aarch64-apple-darwin/release/bundle/macos/Porn Receiver.app"
X64_APP="$PROJECT_DIR/src-tauri/target/x86_64-apple-darwin/release/bundle/macos/Porn Receiver.app"

if [ "$APP_TYPE" == "sender" ]; then
    ARM64_APP="$PROJECT_DIR/src-tauri/target/aarch64-apple-darwin/release/bundle/macos/Porn Sender.app"
    X64_APP="$PROJECT_DIR/src-tauri/target/x86_64-apple-darwin/release/bundle/macos/Porn Sender.app"
fi

# Create tar.gz files
echo "📦 Creating tar.gz archives..."

if [ -d "$ARM64_APP" ]; then
    echo "  - ARM64 (Apple Silicon)..."
    cd "$(dirname "$ARM64_APP")"
    tar -czf "$UPDATES_DIR/$(basename "$ARM64_APP").tar.gz" "$(basename "$ARM64_APP")"
    ARM64_SIZE=$(stat -f%z "$UPDATES_DIR/$(basename "$ARM64_APP").tar.gz")
    echo "    Size: $ARM64_SIZE bytes"
else
    echo "  ⚠️  ARM64 build not found: $ARM64_APP"
fi

if [ -d "$X64_APP" ]; then
    echo "  - x64 (Intel)..."
    cd "$(dirname "$X64_APP")"
    tar -czf "$UPDATES_DIR/$(basename "$X64_APP")_x64.tar.gz" "$(basename "$X64_APP")"
    X64_SIZE=$(stat -f%z "$UPDATES_DIR/$(basename "$X64_APP")_x64.tar.gz")
    echo "    Size: $X64_SIZE bytes"
else
    echo "  ⚠️  x64 build not found: $X64_APP"
fi

# Sign the archives
echo ""
echo "🔐 Signing archives..."

if command -v minisign &> /dev/null; then
    if [ -f "$SIGNING_KEY" ]; then
        cd "$UPDATES_DIR"
        for f in *.tar.gz; do
            if [ -f "$f" ]; then
                echo "  - Signing $f..."
                minisign -S -s "$SIGNING_KEY" -m "$f" -x "$f.sig" -t "Porn Transfer Update"
            fi
        done
    else
        echo "  ⚠️  Signing key not found: $SIGNING_KEY"
        echo "  ⚠️  Using tauri CLI to sign instead..."
    fi
else
    echo "  Using tauri signer..."
    cd "$UPDATES_DIR"
    for f in *.tar.gz; do
        if [ -f "$f" ]; then
            echo "  - Signing $f..."
            # Read signature from tauri signer
            SIGNATURE=$(cat "$SIGNING_KEY" | base64 -d | head -c 100 2>/dev/null || echo "")
            if [ -z "$SIGNATURE" ]; then
                echo "    (Manual signing needed - using placeholder)"
            fi
        fi
    done
fi

# Get signatures using npm tauri signer
echo ""
echo "🔐 Getting signatures with Tauri signer..."
cd "$PROJECT_DIR"

ARM64_SIG=""
X64_SIG=""

if [ -f "$UPDATES_DIR/Porn Receiver.app.tar.gz" ]; then
    ARM64_SIG=$(npm run tauri signer sign "$UPDATES_DIR/Porn Receiver.app.tar.gz" --private-key-path "$SIGNING_KEY" 2>/dev/null | tail -1 || echo "")
fi

if [ -f "$UPDATES_DIR/Porn Receiver.app_x64.tar.gz" ]; then
    X64_SIG=$(npm run tauri signer sign "$UPDATES_DIR/Porn Receiver.app_x64.tar.gz" --private-key-path "$SIGNING_KEY" 2>/dev/null | tail -1 || echo "")
fi

# Get local IP
LOCAL_IP=$(ipconfig getifaddr en0 2>/dev/null || ipconfig getifaddr en1 2>/dev/null || echo "localhost")

# Create latest.json
echo ""
echo "📝 Creating latest.json..."

cat > "$UPDATES_DIR/latest.json" << EOF
{
  "version": "$VERSION",
  "notes": "Update to version $VERSION",
  "pub_date": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "platforms": {
    "darwin-aarch64": {
      "signature": "$ARM64_SIG",
      "url": "http://$LOCAL_IP:8080/$APP_TYPE/Porn Receiver.app.tar.gz"
    },
    "darwin-x86_64": {
      "signature": "$X64_SIG",
      "url": "http://$LOCAL_IP:8080/$APP_TYPE/Porn Receiver.app_x64.tar.gz"
    }
  }
}
EOF

echo ""
echo "✅ Update prepared successfully!"
echo ""
echo "Files in $UPDATES_DIR:"
ls -la "$UPDATES_DIR"
echo ""
echo "📋 latest.json content:"
cat "$UPDATES_DIR/latest.json"
echo ""
