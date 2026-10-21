@echo off
setlocal
cd /d "%~dp0.."
call "%~dp0..\install.bat" %*
endlocal
