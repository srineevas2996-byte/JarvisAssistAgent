# 🔌 Dedicated MCP Folder (`mcp/`)

Drop individual Model Context Protocol (MCP) server configuration files (`*.json`) here.
JARVIS automatically detects each `.json` file in this folder and lists them inside the CLI via `/mcp`.

### Example: `mcp/my_server.json`
```json
{
  "name": "my-server",
  "command": "python",
  "args": ["path/to/server.py"],
  "enabled": true,
  "description": "Custom hardware peripheral or API MCP server"
}
```

### CLI Commands:
- `/mcp` : Displays all recognized MCP server files in this folder
- `/mcp 1` or `/mcp toggle <name>` : Toggles a server online or offline
- `/mcp reload` : Hot-reloads all server configurations
