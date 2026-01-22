#!/bin/bash
# Porn Receiver - First Run Script
# Run this once after installing to fix macOS security warning

APP_PATH="/Applications/Porn Receiver.app"

if [ -d "$APP_PATH" ]; then
    echo "Fixing Porn Receiver..."
    sudo xattr -rd com.apple.quarantine "$APP_PATH"
    echo "Done! Starting Porn Receiver..."
    open "$APP_PATH"
else
    echo "Porn Receiver.app not found in /Applications"
    echo "Please drag it to Applications first, then run this script again."
fi
