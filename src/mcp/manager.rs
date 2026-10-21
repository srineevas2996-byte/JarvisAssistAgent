use crate::checkpoint::{ErrorCategory, ErrorCheckpoint};
use crate::mcp::client::McpClient;
use crate::mcp::protocol::McpTool;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use walkdir::WalkDir;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    #[serde(default)]
    pub name: Option<String>,
    pub command: String,
    pub args: Vec<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub description: String,
    #[serde(skip)]
    pub file_path: Option<PathBuf>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpConfigFile {
    #[serde(rename = "mcpServers")]
    pub mcp_servers: HashMap<String, McpServerConfig>,
}

pub struct McpManager {
    pub mcp_dir: PathBuf,
    pub legacy_config_path: PathBuf,
    pub configs: HashMap<String, McpServerConfig>,
    pub clients: HashMap<String, Arc<McpClient>>,
    pub server_tools: HashMap<String, Vec<McpTool>>,
}

impl McpManager {
    pub fn new<P1: AsRef<Path>, P2: AsRef<Path>>(mcp_dir: P1, legacy_config_path: P2) -> Self {
        Self {
            mcp_dir: mcp_dir.as_ref().to_path_buf(),
            legacy_config_path: legacy_config_path.as_ref().to_path_buf(),
            configs: HashMap::new(),
            clients: HashMap::new(),
            server_tools: HashMap::new(),
        }
    }

    pub fn load_all_configs(&mut self) -> Result<(), String> {
        self.configs.clear();

        // 1. Scan dedicated mcp/ directory for individual *.json server files
        if !self.mcp_dir.exists() {
            let _ = fs::create_dir_all(&self.mcp_dir);
        } else {
            for entry in WalkDir::new(&self.mcp_dir)
                .max_depth(2)
                .into_iter()
                .filter_map(|e| e.ok())
            {
                let path = entry.path();
                if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("json") {
                    if let Ok(content) = fs::read_to_string(path) {
                        // Case A: File contains single server definition
                        if let Ok(mut single_cfg) = serde_json::from_str::<McpServerConfig>(&content) {
                            let srv_name = single_cfg
                                .name
                                .clone()
                                .unwrap_or_else(|| {
                                    path.file_stem()
                                        .and_then(|s| s.to_str())
                                        .unwrap_or("mcp_server")
                                        .to_string()
                                });
                            single_cfg.name = Some(srv_name.clone());
                            single_cfg.file_path = Some(path.to_path_buf());
                            self.configs.insert(srv_name, single_cfg);
                        } else if let Ok(multi_cfg) = serde_json::from_str::<McpConfigFile>(&content) {
                            // Case B: File contains { "mcpServers": { ... } }
                            for (name, mut cfg) in multi_cfg.mcp_servers {
                                cfg.name = Some(name.clone());
                                cfg.file_path = Some(path.to_path_buf());
                                self.configs.insert(name, cfg);
                            }
                        }
                    }
                }
            }
        }

        // 2. Fallback to legacy config/mcp.json if present
        if self.legacy_config_path.exists() {
            if let Ok(content) = fs::read_to_string(&self.legacy_config_path) {
                if let Ok(multi_cfg) = serde_json::from_str::<McpConfigFile>(&content) {
                    for (name, mut cfg) in multi_cfg.mcp_servers {
                        if !self.configs.contains_key(&name) {
                            cfg.name = Some(name.clone());
                            cfg.file_path = Some(self.legacy_config_path.clone());
                            self.configs.insert(name, cfg);
                        }
                    }
                }
            }
        }

        Ok(())
    }

    pub async fn start_enabled_servers(&mut self) {
        let _ = self.load_all_configs();
        for (name, cfg) in &self.configs {
            if cfg.enabled && !self.clients.contains_key(name) {
                match McpClient::spawn(name, &cfg.command, &cfg.args).await {
                    Ok(client) => {
                        let tools = client.list_tools().await.unwrap_or_default();
                        self.server_tools.insert(name.clone(), tools);
                        self.clients.insert(name.clone(), Arc::new(client));
                    }
                    Err(e) => {
                        eprintln!("Warning: Failed to start MCP server '{}': {}", name, e);
                    }
                }
            }
        }
    }

    pub fn list_servers(&self) -> Vec<(&String, &McpServerConfig, bool)> {
        let mut list = Vec::new();
        for (name, cfg) in &self.configs {
            let is_online = self.clients.contains_key(name);
            list.push((name, cfg, is_online));
        }
        list.sort_by(|a, b| a.0.cmp(b.0));
        list
    }

    pub async fn toggle_server(
        &mut self,
        name_or_idx: &str,
        enable: Option<bool>,
    ) -> Result<(String, bool), String> {
        // Support numerical index selection
        let target_name = if let Ok(idx) = name_or_idx.parse::<usize>() {
            let servers = self.list_servers();
            if idx >= 1 && idx <= servers.len() {
                servers[idx - 1].0.clone()
            } else {
                return Err(ErrorCheckpoint::new(
                    ErrorCategory::McpUnreachable,
                    "Invalid MCP Server Index",
                    &format!("Specified index [{}] is out of bounds.", idx),
                    Some(&format!("Total registered MCP servers: {}", servers.len())),
                    "Type '/mcp' to list registered servers and select an index within range.",
                )
                .to_plain_string());
            }
        } else {
            // Find by name or file name
            let found = self.configs.iter().find(|(k, c)| {
                k.eq_ignore_ascii_case(name_or_idx)
                    || c.file_path
                        .as_ref()
                        .and_then(|p| p.file_name())
                        .and_then(|n| n.to_str())
                        .map(|f| f.eq_ignore_ascii_case(name_or_idx))
                        .unwrap_or(false)
            });

            if let Some((k, _)) = found {
                k.clone()
            } else {
                return Err(ErrorCheckpoint::new(
                    ErrorCategory::McpUnreachable,
                    "MCP Server Configuration Not Found",
                    &format!("No MCP server matching '{}' found in mcp/ folder or config.", name_or_idx),
                    Some(&format!("Scanned folder: {}", self.mcp_dir.display())),
                    "Create or place an MCP configuration JSON file in the mcp/ folder, then run '/mcp reload'.",
                )
                .to_plain_string());
            }
        };

        let current_state = self
            .configs
            .get(&target_name)
            .map(|c| c.enabled)
            .unwrap_or(false);

        let new_state = enable.unwrap_or(!current_state);

        // Update in-memory config and persist to its file
        if let Some(cfg) = self.configs.get_mut(&target_name) {
            cfg.enabled = new_state;

            if let Some(ref file_path) = cfg.file_path {
                let single_cfg = cfg.clone();
                if let Ok(json) = serde_json::to_string_pretty(&single_cfg) {
                    let _ = fs::write(file_path, json);
                }
            }
        }

        if new_state {
            if let Some(cfg) = self.configs.get(&target_name) {
                let client = McpClient::spawn(&target_name, &cfg.command, &cfg.args).await?;
                let tools = client.list_tools().await.unwrap_or_default();
                self.server_tools.insert(target_name.clone(), tools);
                self.clients.insert(target_name.clone(), Arc::new(client));
            }
        } else {
            if let Some(client) = self.clients.remove(&target_name) {
                client.terminate().await;
            }
            self.server_tools.remove(&target_name);
        }

        Ok((target_name, new_state))
    }

    pub fn find_tool_server(&self, tool_name: &str) -> Option<(String, Arc<McpClient>)> {
        for (srv_name, tools) in &self.server_tools {
            if tools.iter().any(|t| t.name == tool_name) {
                if let Some(client) = self.clients.get(srv_name) {
                    return Some((srv_name.clone(), Arc::clone(client)));
                }
            }
        }
        None
    }

    pub async fn call_mcp_tool(&self, tool_name: &str, args: Value) -> Result<String, String> {
        if let Some((_srv_name, client)) = self.find_tool_server(tool_name) {
            client.call_tool(tool_name, args).await
        } else {
            Err(ErrorCheckpoint::new(
                ErrorCategory::McpUnreachable,
                "MCP Server Offline or Missing For Tool",
                &format!("No running or connected MCP server currently provides the tool '{}'.", tool_name),
                None,
                "Check '/mcp' to ensure the corresponding server is online and its tools are registered.",
            )
            .to_plain_string())
        }
    }
}
