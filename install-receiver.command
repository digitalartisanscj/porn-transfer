#!/bin/bash
# Porn Receiver - Install Script
# Extracts the app and fixes macOS security

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ZIP_FILE="$SCRIPT_DIR/Porn Receiver.app.zip"
APP_PATH="/Applications/Porn Receiver.app"

echo "Installing Porn Receiver..."

# Remove old version if exists
if [ -d "$APP_PATH" ]; then
    echo "Removing old version..."
    rm -rf "$APP_PATH"
fi

# Extract zip to Applications
if [ -f "$ZIP_FILE" ]; then
    echo "Extracting to Applications..."
    unzip -q "$ZIP_FILE" -d /Applications/

    # Fix quarantine
    echo "Fixing security (may ask for password)..."
    sudo xattr -rd com.apple.quarantine "$APP_PATH"

    echo "Done! Starting Porn Receiver..."
    open "$APP_PATH"
else
    # Maybe app is already in Applications
    if [ -d "$APP_PATH" ]; then
        echo "Fixing security (may ask for password)..."
        sudo xattr -rd com.apple.quarantine "$APP_PATH"
        echo "Done! Starting Porn Receiver..."
        open "$APP_PATH"
    else
        echo "Error: Could not find Porn Receiver.app.zip"
        echo "Make sure this script is in the same folder as the zip file."
    fi
fi
