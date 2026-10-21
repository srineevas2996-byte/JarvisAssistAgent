# 🛠️ Dedicated Tools Folder (`tools/`)

Drop your custom tool definition files (`*.json`) or custom scripts (`*.ps1`, `*.bat`, `*.py`) into this folder.
JARVIS automatically discovers each file in this folder and registers it as a plugged tool!

### File Formats Recognized:

#### 1. Manifest Tools (`*.json`)
```json
{
  "name": "my_custom_tool",
  "description": "Performs custom action or hardware interaction",
  "command": "powershell",
  "args": ["-NoProfile", "-Command", "echo 'Input was {param}'"],
  "parameters": {
    "type": "object",
    "properties": {
      "param": { "type": "string", "description": "Input parameter" }
    },
    "required": ["param"]
  },
  "enabled": true
}
```

#### 2. Standalone Script Tools (`*.ps1`, `*.bat`, `*.py`)
Drop any script file (e.g. `restart_service.ps1`, `clean_temp.bat`).
JARVIS automatically exposes it as a runnable tool named after the file!

### CLI Commands:
- `/tool` : Displays all plugged tools (Native, `tools/`, MCP)
- `/tool 1` or `/tool enable <name>` : Enables or disables a tool
- `/tool test <name_or_number> '{"param": "val"}'` : Executes an immediate test run
- `/tool reload` : Rescans the folder for newly added files
