@echo off
setlocal
echo ========================================================================
echo         J.A.A (jarvis_assist_agent) - Windows Installer
echo ========================================================================
echo.
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0install.ps1" %*
if errorlevel 1 (
    echo.
    echo [ERROR] Installation failed or was interrupted.
    pause
    exit /b 1
)
echo.
pause
endlocal
