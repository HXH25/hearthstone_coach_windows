@echo off
setlocal
cd /d "%~dp0"

powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%~dp0tools\bootstrap_launcher.ps1" %*
set "HC_EXIT=%ERRORLEVEL%"

if not "%HC_EXIT%"=="0" (
    echo.
    echo [HearthCoach] Launcher failed with exit code %HC_EXIT%.
    echo Please keep this window open and send the error text if you need help.
    pause
)

exit /b %HC_EXIT%
