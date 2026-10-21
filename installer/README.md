# 📦 J.A.A (jarvis_assist_agent) Installer

> **Core Purpose**: Standalone, automated setup environment for **J.A.A (`jarvis_assist_agent`)** — the 100% non-HTTP socket-based, native Rust & C++ fast autonomous agent framework for local Vision-Language Models (VLMs) and LLMs.

---

## How to Install

### Option A: Double-Click
Double-click `install.bat` in the root project folder (or `setup.bat` in this folder).

### Option B: PowerShell
```powershell
powershell -ExecutionPolicy Bypass -File .\install.ps1
```

### Advanced Options:
```powershell
# Skip Rust compilation (if binaries are already present):
powershell -ExecutionPolicy Bypass -File .\install.ps1 -SkipRustBuild

# Skip C++ compilation:
powershell -ExecutionPolicy Bypass -File .\install.ps1 -SkipNativeBuild

# Skip desktop shortcut creation:
powershell -ExecutionPolicy Bypass -File .\install.ps1 -CreateDesktopShortcut:$false
```

## What the Installer Does:
1. **Directories**: Ensures `models/`, `skills/`, `tools/`, `mcp/`, `config/`, `cache/`, `test_log/`, and `native_lib/` exist.
2. **Configuration**: Initializes `config/config.json` and `config/mcp.json` with sensible defaults if missing.
3. **DLL Sync**: Copies the llama.cpp / ggml C++ runtime DLLs from `native_lib/` to the project root.
4. **Native C++ Build**: Detects Visual Studio MSVC 2022 and builds `jarvis-os-bridge.exe` and `jarvis-engine.exe`.
5. **Rust Core Build**: Compiles `jaa.exe` via `cargo build --release` and places it in the project root.
6. **Launchers & Shortcuts**: Creates `jaa.bat`, `jarvis.bat`, and `J.A.A CLI.lnk` desktop shortcut with custom icon.
