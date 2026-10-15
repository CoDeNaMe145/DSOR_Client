@echo off
setlocal
cd /d "%~dp0"
start "" http://localhost:8091
python tools\vfx_editor\server.py %*
if errorlevel 1 pause
