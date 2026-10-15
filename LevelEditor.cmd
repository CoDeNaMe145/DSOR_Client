@echo off
setlocal
cd /d "%~dp0"
python tools\level_editor.py %*
if errorlevel 1 pause
