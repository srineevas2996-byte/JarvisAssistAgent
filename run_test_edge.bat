@echo off
call "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat"
cl /O2 /EHsc test_edge.cpp /Fe:test_edge.exe
test_edge.exe
del test_edge.exe test_edge.obj test_edge.cpp
