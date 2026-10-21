@echo off
setlocal
for /f "usebackq tokens=*" %%i in (`"%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe" -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath`) do (
    set "VS_DIR=%%i"
)
call "%VS_DIR%\VC\Auxiliary\Build\vcvars64.bat" >nul
cl.exe /O2 /EHsc /std:c++17 test_mano_p_init.cpp user32.lib /Fe:test_mano_p_init.exe
if errorlevel 1 (
    echo Compilation failed.
    exit /b 1
)
test_mano_p_init.exe
del test_mano_p_init.obj test_mano_p_init.exe
