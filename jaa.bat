@echo off
setlocal
set "ROOT=%~dp0"

if exist "%ROOT%jaa.exe" (
    "%ROOT%jaa.exe" %*
) else if exist "%ROOT%target\release\jaa.exe" (
    "%ROOT%target\release\jaa.exe" %*
) else if exist "%ROOT%target\debug\jaa.exe" (
    "%ROOT%target\debug\jaa.exe" %*
) else (
    echo [J.A.A] Binary not found. Running via cargo...
    cargo run --release --bin jaa -- %*
)
endlocal
