@echo off
call "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat"
cl /O2 /EHsc inspect_fg.cpp /Fe:inspect_fg.exe
inspect_fg.exe
del inspect_fg.exe inspect_fg.obj inspect_fg.cpp
