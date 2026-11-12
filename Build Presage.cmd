@echo off
rem Double-click to build Presage.exe and put shortcuts to it on the Desktop and in the Start menu.
rem Run it again after pulling changes to update the app (close Presage first).
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\build-desktop.ps1" %*
echo.
pause
