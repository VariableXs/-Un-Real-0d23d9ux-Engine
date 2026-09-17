@echo off
rem VARIX usb unplug drill - elevated wrapper (pure ASCII)
cd /d "D:\2\14\-Un-Real-0d23d9ux-Engine-main"
python _attic\usb-observe-run.py > _attic\usb63-observe-out.txt 2>&1
exit /b %ERRORLEVEL%
