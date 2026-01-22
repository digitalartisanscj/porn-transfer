#!/bin/bash
# Porn Sender - First Run Script
# Run this once after installing to fix macOS security warning

APP_PATH="/Applications/Porn Sender.app"

if [ -d "$APP_PATH" ]; then
    echo "Fixing Porn Sender..."
    sudo xattr -rd com.apple.quarantine "$APP_PATH"
    echo "Done! Starting Porn Sender..."
    open "$APP_PATH"
else
    echo "Porn Sender.app not found in /Applications"
    echo "Please drag it to Applications first, then run this script again."
fi
