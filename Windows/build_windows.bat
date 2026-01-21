@echo off
echo ========================================
echo Building Pornhub Transfer for Windows
echo ========================================

REM Install PyInstaller if not present
pip install pyinstaller

REM Clean previous builds
rmdir /s /q build 2>nul
rmdir /s /q dist 2>nul

REM Build Sender (for photographers) - using onedir for fast startup
echo.
echo Building Sender...
pyinstaller --onedir --windowed --name "PhotoSender" --icon "sender.ico" sender.py

REM Build Receiver (for tagger/editor)  
echo.
echo Building Receiver...
pyinstaller --onedir --windowed --name "PhotoReceiver" --icon "receiver.ico" receiver.py

echo.
echo ========================================
echo Done! Executables are in dist/ folder
echo.
echo   dist\PhotoSender\PhotoSender.exe
echo   dist\PhotoReceiver\PhotoReceiver.exe
echo ========================================
pause
