@echo off
echo ========================================
echo Organizing distribution folder
echo ========================================

mkdir distribution 2>nul
mkdir "distribution\Fotografi Windows" 2>nul
mkdir "distribution\Tagger Windows" 2>nul

copy dist\PhotoSender.exe "distribution\Fotografi Windows\" 2>nul
copy dist\PhotoReceiver.exe "distribution\Tagger Windows\" 2>nul

echo.
echo Done! Check the 'distribution' folder
echo.
echo Now copy the Mac apps from a Mac build:
echo   - PhotoSender.app   -> distribution\Fotografi Mac\
echo   - PhotoReceiver.app -> distribution\Editori Mac\
echo.
pause
