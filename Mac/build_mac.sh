#!/bin/bash
echo "========================================"
echo "Building Pornhub Transfer for Mac"
echo "========================================"

# Set minimum macOS version for compatibility
# 10.15 = Catalina, works on Ventura and newer
export MACOSX_DEPLOYMENT_TARGET=10.15

# Install PyInstaller if not present
pip3 install pyinstaller

# Clean previous builds
rm -rf build dist

# Build Sender (for photographers) - using onedir for fast startup
echo ""
echo "Building Sender..."
pyinstaller --onedir --windowed --name "PhotoSender" --icon=icon.ico sender.py

# Build Receiver (for tagger/editor)
echo ""
echo "Building Receiver..."
pyinstaller --onedir --windowed --name "PhotoReceiver" --icon=icon.icns receiver.py

echo ""
echo "========================================"
echo "Done! Apps are in dist/ folder"
echo ""
echo "  dist/PhotoSender/PhotoSender.app"
echo "  dist/PhotoReceiver/PhotoReceiver.app"
echo "========================================"
