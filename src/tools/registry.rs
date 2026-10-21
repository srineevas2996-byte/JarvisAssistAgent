use crate::checkpoint::{ErrorCategory, ErrorCheckpoint};
use crate::mcp::protocol::McpTool;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ToolSource {
    Native,
    Mcp { server_name: String },
    CustomFile {
        file_path: PathBuf,
        command: Option<String>,
        args: Option<Vec<String>>,
    },
    Script { script_path: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomToolManifest {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Option<Vec<String>>,
    #[serde(default = "default_tool_schema")]
    pub parameters: Value,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_tool_schema() -> Value {
    json!({
        "type": "object",
        "properties": {}
    })
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value,
    pub enabled: bool,
    pub source: ToolSource,
    pub file_name: Option<String>,
}

pub struct ToolRegistry {
    pub tools_dir: PathBuf,
    pub tools: HashMap<String, ToolDefinition>,
    pub disabled_tools: Vec<String>,
}

impl ToolRegistry {
    pub fn new<P: AsRef<Path>>(tools_dir: P, disabled_tools: &[String]) -> Self {
        let mut registry = Self {
            tools_dir: tools_dir.as_ref().to_path_buf(),
            tools: HashMap::new(),
            disabled_tools: disabled_tools.to_vec(),
        };
        registry.register_builtin_tools();
        registry.scan_tools_dir();
        registry.apply_disabled_tools();
        registry
    }

    pub fn apply_disabled_tools(&mut self) {
        for (name, tool) in self.tools.iter_mut() {
            if self.disabled_tools.iter().any(|d| d.eq_ignore_ascii_case(name)) {
                tool.enabled = false;
            }
        }
    }

    pub fn get_disabled_tools(&self) -> Vec<String> {
        self.tools
            .values()
            .filter(|t| !t.enabled)
            .map(|t| t.name.clone())
            .collect()
    }

    fn register_builtin_tools(&mut self) {
        // ================= Vision & Screen Capture Tools =================
        self.register(ToolDefinition {
            name: "capture_screen".to_string(),
            description: "Captures full desktop screenshot and returns screen resolution (width, height), image file path, and coordinate normalization criteria (0.0, 0.0 at Bottom-Left, 1.0, 1.0 at Top-Right).".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {}
            }),
            enabled: true,
            source: ToolSource::Native,
            file_name: None,
        });

        // ================= Metadata & Accessibility Navigation Tools =================
        self.register(ToolDefinition {
            name: "get_screen_state".to_string(),
            description: "Perceives the layered desktop window stack and returns sequential Tag IDs [1], [2], [3]... with bounding boxes.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {}
            }),
            enabled: true,
            source: ToolSource::Native,
            file_name: None,
        });

        // ================= Mouse Control Tools =================
        self.register(ToolDefinition {
            name: "click_coordinates".to_string(),
            description: "Moves mouse and clicks normalized coordinates (X, Y) on the primary display where (0.0, 0.0) is Bottom-Left and (1.0, 1.0) is Top-Right. Supports double-click and right-click.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "x": { "type": "number", "description": "Normalized horizontal coordinate from 0.0 (left) to 1.0 (right)" },
                    "y": { "type": "number", "description": "Normalized vertical coordinate from 0.0 (bottom) to 1.0 (top)" },
                    "double": { "type": "boolean", "description": "Set to true to double-click", "default": false },
                    "right": { "type": "boolean", "description": "Set to true to right-click", "default": false }
                },
                "required": ["x", "y"]
            }),
            enabled: true,
            source: ToolSource::Native,
            file_name: None,
        });

        self.register(ToolDefinition {
            name: "click_element".to_string(),
            description: "Clicks an element on screen by its assigned Tag ID from get_screen_state.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "tag_id": { "type": "integer", "description": "The unique integer Tag_id from get_screen_state to click." },
                    "double": { "type": "boolean", "description": "Set to true to double-click", "default": false },
                    "right": { "type": "boolean", "description": "Set to true to right-click", "default": false }
                },
                "required": ["tag_id"]
            }),
            enabled: true,
            source: ToolSource::Native,
            file_name: None,
        });

        self.register(ToolDefinition {
            name: "drag_mouse".to_string(),
            description: "Drags the mouse smoothly between normalized coordinates (from_x, from_y) and (to_x, to_y) where (0.0, 0.0) is Bottom-Left and (1.0, 1.0) is Top-Right.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "from_x": { "type": "number", "description": "Starting normalized X coordinate (0.0=left to 1.0=right)" },
                    "from_y": { "type": "number", "description": "Starting normalized Y coordinate (0.0=bottom to 1.0=top)" },
                    "to_x": { "type": "number", "description": "Destination normalized X coordinate (0.0=left to 1.0=right)" },
                    "to_y": { "type": "number", "description": "Destination normalized Y coordinate (0.0=bottom to 1.0=top)" },
                    "duration": { "type": "number", "description": "Drag duration in seconds", "default": 0.3 }
                },
                "required": ["from_x", "from_y", "to_x", "to_y"]
            }),
            enabled: true,
            source: ToolSource::Native,
            file_name: None,
        });

        // ================= Keyboard Control Tools =================
        self.register(ToolDefinition {
            name: "type_text".to_string(),
            description: "Types text into active focus, or optionally clicks normalized coordinates (X, Y) first. Set press_enter=true to submit.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "text": { "type": "string", "description": "Text to type" },
                    "x": { "type": "number", "description": "Optional normalized X coordinate (0.0=left to 1.0=right) to click first" },
                    "y": { "type": "number", "description": "Optional normalized Y coordinate (0.0=bottom to 1.0=top) to click first" },
                    "press_enter": { "type": "boolean", "description": "Set to true to press Enter after typing", "default": false }
                },
                "required": ["text"]
            }),
            enabled: true,
            source: ToolSource::Native,
            file_name: None,
        });

        self.register(ToolDefinition {
            name: "type_element".to_string(),
            description: "Focuses an element on screen by Tag ID and types text into it. Set press_enter=true to submit.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "tag_id": { "type": "integer", "description": "The integer Tag_id to focus and type into." },
                    "text": { "type": "string", "description": "The text to type" },
                    "press_enter": { "type": "boolean", "description": "Set to true to press Enter after typing", "default": false }
                },
                "required": ["tag_id", "text"]
            }),
            enabled: true,
            source: ToolSource::Native,
            file_name: None,
        });


        // ================= Window Management Tools =================
        self.register(ToolDefinition {
            name: "window_control".to_string(),
            description: "Minimizes, maximizes, restores, or focuses a target window by title or process name.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "description": "Action: 'focus', 'minimize', or 'maximize'", "default": "focus" },
                    "title": { "type": "string", "description": "Target window title or substring (e.g. 'Paint', 'Notepad', 'Chrome')" }
                },
                "required": ["action"]
            }),
            enabled: true,
            source: ToolSource::Native,
            file_name: None,
        });
    }

    /// Scans the dedicated tools/ directory for custom tool files (*.json, *.ps1, *.py, *.bat)
    pub fn scan_tools_dir(&mut self) {
        if !self.tools_dir.exists() {
            let _ = fs::create_dir_all(&self.tools_dir);
            return;
        }

        for entry in WalkDir::new(&self.tools_dir)
            .max_depth(2)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if path.is_file() {
                let ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_lowercase();

                let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();

                if ext == "json" {
                    // Try parsing as CustomToolManifest
                    if let Ok(content) = fs::read_to_string(path) {
                        if let Ok(manifest) = serde_json::from_str::<CustomToolManifest>(&content) {
                            self.register(ToolDefinition {
                                name: manifest.name,
                                description: manifest.description,
                                parameters: manifest.parameters,
                                enabled: manifest.enabled,
                                source: ToolSource::CustomFile {
                                    file_path: path.to_path_buf(),
                                    command: manifest.command,
                                    args: manifest.args,
                                },
                                file_name: Some(file_name),
                            });
                        }
                    }
                } else if ext == "ps1" || ext == "bat" || ext == "py" {
                    let tool_name = path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("custom_script")
                        .to_string();

                    // Only add if not already defined via json
                    if !self.tools.contains_key(&tool_name) {
                        let (cmd, base_args) = match ext.as_str() {
                            "ps1" => ("powershell".to_string(), vec!["-NoProfile".to_string(), "-File".to_string(), path.display().to_string()]),
                            "bat" => ("cmd".to_string(), vec!["/c".to_string(), path.display().to_string()]),
                            "py" => ("python".to_string(), vec![path.display().to_string()]),
                            _ => (path.display().to_string(), vec![]),
                        };

                        self.register(ToolDefinition {
                            name: tool_name.clone(),
                            description: format!("Custom script tool from tools/{}", file_name),
                            parameters: json!({
                                "type": "object",
                                "properties": {
                                    "arguments": { "type": "string", "description": "Optional arguments passed to the script" }
                                }
                            }),
                            enabled: true,
                            source: ToolSource::CustomFile {
                                file_path: path.to_path_buf(),
                                command: Some(cmd),
                                args: Some(base_args),
                            },
                            file_name: Some(file_name),
                        });
                    }
                }
            }
        }
    }

    pub fn register(&mut self, tool: ToolDefinition) {
        self.tools.insert(tool.name.clone(), tool);
    }

    pub fn register_mcp_tools(&mut self, server_name: &str, mcp_tools: &[McpTool]) {
        for t in mcp_tools {
            if self.tools.contains_key(&t.name) {
                continue;
            }
            self.register(ToolDefinition {
                name: t.name.clone(),
                description: t.description.clone(),
                parameters: t.input_schema.clone(),
                enabled: true,
                source: ToolSource::Mcp {
                    server_name: server_name.to_string(),
                },
                file_name: None,
            });
        }
    }

    pub fn get_tool(&self, name: &str) -> Option<&ToolDefinition> {
        self.tools.get(name)
    }

    pub fn list_tools(&self) -> Vec<&ToolDefinition> {
        let mut list: Vec<&ToolDefinition> = self.tools.values().collect();
        list.sort_by(|a, b| a.name.cmp(&b.name));
        list
    }

    pub fn toggle_tool(&mut self, name_or_idx: &str, enable: Option<bool>) -> Result<(String, bool), String> {
        // Try numerical index first
        if let Ok(idx) = name_or_idx.parse::<usize>() {
            let (target_name, count) = {
                let tools = self.list_tools();
                if idx >= 1 && idx <= tools.len() {
                    (Some(tools[idx - 1].name.clone()), tools.len())
                } else {
                    (None, tools.len())
                }
            };

            if let Some(target_name) = target_name {
                if let Some(tool) = self.tools.get_mut(&target_name) {
                    let new_state = enable.unwrap_or(!tool.enabled);
                    tool.enabled = new_state;
                    if new_state {
                        self.disabled_tools.retain(|d| !d.eq_ignore_ascii_case(&target_name));
                    } else if !self.disabled_tools.iter().any(|d| d.eq_ignore_ascii_case(&target_name)) {
                        self.disabled_tools.push(target_name.clone());
                    }
                    return Ok((target_name, new_state));
                }
            }
            return Err(ErrorCheckpoint::new(
                ErrorCategory::ToolNonFunctional,
                "Invalid Tool Index",
                &format!("Selected tool index [{}] is out of bounds.", idx),
                Some(&format!("Total registered tools: {}", count)),
                "Type '/tool' to see available tools and pick a number between 1 and that total.",
            )
            .to_plain_string());
        }

        // Match by name or file name
        let target_key = self
            .tools
            .iter()
            .find(|(k, t)| {
                k.eq_ignore_ascii_case(name_or_idx)
                    || t.name.eq_ignore_ascii_case(name_or_idx)
                    || t.file_name.as_deref().map(|f| f.eq_ignore_ascii_case(name_or_idx)).unwrap_or(false)
            })
            .map(|(k, _)| k.clone());

        if let Some(k) = target_key {
            if let Some(tool) = self.tools.get_mut(&k) {
                let new_state = enable.unwrap_or(!tool.enabled);
                tool.enabled = new_state;
                let name = tool.name.clone();
                if new_state {
                    self.disabled_tools.retain(|d| !d.eq_ignore_ascii_case(&name));
                } else if !self.disabled_tools.iter().any(|d| d.eq_ignore_ascii_case(&name)) {
                    self.disabled_tools.push(name.clone());
                }
                return Ok((name, new_state));
            }
        }

        Err(ErrorCheckpoint::new(
            ErrorCategory::ToolNonFunctional,
            "Tool Not Registered",
            &format!("No tool matching identifier '{}' is registered.", name_or_idx),
            Some(&format!("Scanned tools directory: {}", self.tools_dir.display())),
            "Add a tool manifest (.json) or script (.ps1, .bat, .py) to tools/ folder, or type '/tool reload'.",
        )
        .to_plain_string())
    }

    pub fn build_tools_system_prompt(&self) -> String {
        let mut enabled_tools: Vec<&ToolDefinition> = self.tools.values().filter(|t| t.enabled).collect();
        if enabled_tools.is_empty() {
            return String::new();
        }
        enabled_tools.sort_by(|a, b| a.name.cmp(&b.name));

        let mut prompt = String::from("## REGISTERED TOOLS INDEX:\n\n");
        for (i, tool) in enabled_tools.iter().enumerate() {
            let params_summary = Self::format_compact_parameters(&tool.parameters);
            prompt.push_str(&format!(
                "[{}] `{}`{}\n    {}\n\n",
                i + 1,
                tool.name,
                params_summary,
                tool.description.trim()
            ));
        }

        prompt
    }

    fn format_compact_parameters(val: &Value) -> String {
        if let Some(props) = val.get("properties").and_then(|p| p.as_object()) {
            let required = val.get("required")
                .and_then(|r| r.as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
                .unwrap_or_default();

            let params: Vec<String> = props.iter().map(|(k, v)| {
                let typ = v.get("type").and_then(|t| t.as_str()).unwrap_or("any");
                let is_req = required.contains(&k.as_str());
                if is_req {
                    format!("{}: {}", k, typ)
                } else {
                    format!("{}?: {}", k, typ)
                }
            }).collect();
            format!("({})", params.join(", "))
        } else {
            "()".to_string()
        }
    }

    pub fn get_openai_tools_schema(&self) -> Vec<Value> {
        self.tools
            .values()
            .filter(|t| t.enabled)
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters
                    }
                })
            })
            .collect()
    }
}
