#!/bin/bash
echo "========================================"
echo "Building Pornhub Transfer for Mac"
echo "========================================"

# Install PyInstaller if not present
pip3 install pyinstaller

# Clean previous builds
rm -rf build dist

# Build Sender (for photographers) - using onedir for fast startup
echo ""
echo "Building Sender..."
pyinstaller --onedir --windowed --name "PhotoSender" sender.py

# Build Receiver (for tagger/editor)
echo ""
echo "Building Receiver..."
pyinstaller --onedir --windowed --name "PhotoReceiver" receiver.py

echo ""
echo "========================================"
echo "Done! Apps are in dist/ folder"
echo ""
echo "  dist/PhotoSender/PhotoSender.app"
echo "  dist/PhotoReceiver/PhotoReceiver.app"
echo "========================================"
