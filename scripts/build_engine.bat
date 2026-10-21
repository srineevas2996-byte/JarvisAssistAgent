@echo off
setlocal

echo [JARVIS Native C++ Inference Engine Compiler]
echo Detecting Visual Studio 2022 / MSVC environment...

for /f "usebackq tokens=*" %%i in (`"%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe" -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath`) do (
    set "VS_DIR=%%i"
)

if not defined VS_DIR (
    echo [ERROR] Visual Studio C++ build tools not found.
    exit /b 1
)

call "%VS_DIR%\VC\Auxiliary\Build\vcvars64.bat" >nul
if errorlevel 1 (
    echo [ERROR] Failed initializing vcvars64.bat environment.
    exit /b 1
)

echo Compiling native\jarvis_engine\main.cpp to jarvis-engine.exe...
cl.exe /O2 /EHsc /std:c++17 native\jarvis_engine\main.cpp user32.lib bcrypt.lib /Fe:jarvis-engine.exe /link /SUBSYSTEM:CONSOLE
if errorlevel 1 (
    echo [ERROR] MSVC compilation failed.
    exit /b 1
)

if exist main.obj del main.obj

echo.
echo [SUCCESS] jarvis-engine.exe compiled and ready.
endlocal
