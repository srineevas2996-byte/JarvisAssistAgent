@echo off
call "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat"
cl /O2 /EHsc native\os_bridge\main.cpp /Fe:jarvis-os-bridge.exe
del main.obj
