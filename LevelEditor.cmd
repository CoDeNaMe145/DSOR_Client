@echo off
setlocal
cd /d "%~dp0"
if not exist target\release\dsor-level-editor.exe (
  powershell -NoProfile -ExecutionPolicy Bypass -File tools\Build-LevelEditor.ps1
  if errorlevel 1 exit /b 1
)
target\release\dsor-level-editor.exe --editor %*
if errorlevel 1 pause
