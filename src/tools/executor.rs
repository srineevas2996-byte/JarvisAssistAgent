use crate::checkpoint::{ErrorCategory, ErrorCheckpoint};
use crate::mcp::McpManager;
use crate::tools::registry::{ToolRegistry, ToolSource};
use regex::Regex;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentMode {
    Chat,
    Task,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ToolAction {
    Execute(ExtractedToolCall),
    TaskCompleted,
    None,
}

#[derive(Debug, Clone)]
pub struct ParsedModelOutput {
    pub mode: AgentMode,
    pub chat: String,
    pub action: ToolAction,
    pub raw_text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExtractedToolCall {
    pub tool_name: String,
    pub arguments: Value,
    pub raw_matched: String,
}

pub struct ToolExecutor;

impl ToolExecutor {
    /// Parses structured model output into Chat text and Tool action (YAML or JSON)
    pub fn parse_model_output(text: &str) -> ParsedModelOutput {
        let trimmed = text.trim();

        // 1. Separate Chat section and Tool section based on `Tool:` line boundary
        let mut chat_lines = Vec::new();
        let mut tool_lines = Vec::new();
        let mut in_tool = false;
        let mut explicit_mode: Option<AgentMode> = None;

        for line in trimmed.lines() {
            let line_trimmed = line.trim();
            let line_lower = line_trimmed.to_lowercase();

            if !in_tool {
                if line_lower.starts_with("mode:") {
                    let m_val = line_trimmed[5..].trim().to_lowercase();
                    if m_val.starts_with("task") {
                        explicit_mode = Some(AgentMode::Task);
                    } else if m_val.starts_with("chat") {
                        explicit_mode = Some(AgentMode::Chat);
                    }
                    continue;
                } else if line_lower.starts_with("tool:") {
                    in_tool = true;
                    let remainder = line_trimmed[5..].trim();
                    if !remainder.is_empty() {
                        let rem_lower = remainder.to_lowercase();
                        if !remainder.contains(':')
                            && rem_lower != "task_completed"
                            && rem_lower != "task completed"
                            && rem_lower != "none"
                            && rem_lower != "done"
                            && rem_lower != "finish"
                            && rem_lower != "finished"
                        {
                            tool_lines.push(format!("name: {}", remainder));
                        } else {
                            tool_lines.push(remainder.to_string());
                        }
                    }
                } else {
                    // Check if line is a separator line (e.g. ......... or --------- or =======)
                    let is_separator = line_trimmed.len() >= 3
                        && line_trimmed.chars().all(|c| c == '.' || c == '-' || c == '_' || c == '=');
                    if !is_separator {
                        chat_lines.push(line);
                    }
                }
            } else {
                tool_lines.push(line.to_string());
            }
        }

        let mut chat_content = chat_lines.join("\n").trim().to_string();

        // Strip leading "Chat:" or "chat:" prefix if present
        if let Some(stripped) = chat_content
            .strip_prefix("Chat:")
            .or_else(|| chat_content.strip_prefix("chat:"))
            .or_else(|| chat_content.strip_prefix("CHAT:"))
        {
            chat_content = stripped.trim().to_string();
        }

        let tool_block = if in_tool && !tool_lines.is_empty() {
            Some(tool_lines.join("\n").trim().to_string())
        } else {
            None
        };

        // If no explicit Tool: line was found, produce ToolAction::None and enforce Chat mode
        if tool_block.is_none() {
            return ParsedModelOutput {
                mode: AgentMode::Chat,
                chat: if chat_content.is_empty() { trimmed.to_string() } else { chat_content },
                action: ToolAction::None,
                raw_text: trimmed.to_string(),
            };
        }

        let mut action = if let Some(ref tb) = tool_block {
            let mut clean_tb = tb.trim();
            if clean_tb.starts_with("```") {
                if let Some(pos) = clean_tb.find('\n') {
                    clean_tb = &clean_tb[pos + 1..];
                }
                if clean_tb.ends_with("```") {
                    clean_tb = &clean_tb[..clean_tb.len() - 3];
                }
                clean_tb = clean_tb.trim();
            }
            let lower = clean_tb.to_lowercase();

            if lower == "task_completed"
                || lower == "task completed"
                || lower == "task_complete"
                || lower == "none"
                || lower == "done"
                || lower == "completed"
                || lower == "finish"
                || lower == "finished"
                || lower.starts_with("task_completed")
                || lower.starts_with("task completed")
            {
                ToolAction::TaskCompleted
            } else {
                Self::parse_yaml_tool_block(clean_tb)
                    .map(ToolAction::Execute)
                    .unwrap_or_else(|| {
                        if lower.contains("task_completed") || lower.contains("task completed") {
                            ToolAction::TaskCompleted
                        } else {
                            ToolAction::None
                        }
                    })
            }
        } else {
            ToolAction::None
        };

        // Gateway mode resolution:
        // - In chat mode, tools are never executed (action is forced to None).
        // - In task mode, an actionable tool or task_completed engages TaskMode; omitting tools falls back to ChatMode.
        let mode = match explicit_mode {
            Some(AgentMode::Chat) => {
                action = ToolAction::None;
                AgentMode::Chat
            }
            Some(AgentMode::Task) => {
                if matches!(action, ToolAction::Execute(_)) || matches!(action, ToolAction::TaskCompleted) {
                    AgentMode::Task
                } else {
                    AgentMode::Chat
                }
            }
            None => {
                if matches!(action, ToolAction::Execute(_)) || matches!(action, ToolAction::TaskCompleted) {
                    AgentMode::Task
                } else {
                    AgentMode::Chat
                }
            }
        };

        // Smart parameter recovery for 2B models when arguments are omitted:
        if let ToolAction::Execute(ref mut call) = action {
            if call.tool_name == "type_text" {
                let has_valid_text = call
                    .arguments
                    .as_object()
                    .and_then(|o| o.get("text"))
                    .and_then(|t| t.as_str())
                    .map(|s| !s.is_empty())
                    .unwrap_or(false);

                if !has_valid_text {
                    // Check if arguments was directly a string
                    if let Some(s) = call.arguments.as_str() {
                        call.arguments = serde_json::json!({
                            "text": s,
                            "press_enter": true
                        });
                    } else {
                        // Check if chat mentions: type "xyz" or type xyz
                        if let Ok(re) = Regex::new(r#"(?i)type\s+["']?([a-zA-Z0-9_\-\.]+?)["']?(?:\s+to|\s+into|\s+and|\.|\n|$)"#) {
                            if let Some(caps) = re.captures(&chat_content) {
                                if let Some(txt) = caps.get(1) {
                                    call.arguments = serde_json::json!({
                                        "text": txt.as_str().trim(),
                                        "press_enter": true
                                    });
                                }
                            }
                        }
                    }
                }
            } else if call.tool_name == "click_element" {
                let has_valid_tag = call
                    .arguments
                    .as_object()
                    .and_then(|o| o.get("tag_id").or_else(|| o.get("tag")).or_else(|| o.get("id")))
                    .is_some();

                if !has_valid_tag {
                    if let Some(num) = call.arguments.as_i64() {
                        call.arguments = serde_json::json!({ "tag_id": num });
                    } else if let Some(s) = call.arguments.as_str() {
                        let digits: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
                        if let Ok(num) = digits.parse::<i64>() {
                            call.arguments = serde_json::json!({ "tag_id": num });
                        }
                    }
                    if !call.arguments.is_object() || call.arguments.get("tag_id").is_none() {
                        if let Ok(re) = Regex::new(r#"(?i)(?:tag_id|tag|\[)\s*(\d+)\]?"#) {
                            if let Some(caps) = re.captures(&chat_content) {
                                if let Some(num_str) = caps.get(1) {
                                    if let Ok(num) = num_str.as_str().parse::<i64>() {
                                        call.arguments = serde_json::json!({ "tag_id": num });
                                    }
                                }
                            }
                        }
                    }
                } else if let Some(obj) = call.arguments.as_object_mut() {
                    if !obj.contains_key("tag_id") {
                        if let Some(val) = obj.remove("tag").or_else(|| obj.remove("id")) {
                            obj.insert("tag_id".to_string(), val);
                        }
                    }
                }
            } else if call.tool_name == "send_hotkey" {
                let has_keys = call
                    .arguments
                    .as_object()
                    .and_then(|o| o.get("keys"))
                    .and_then(|k| k.as_array())
                    .map(|a| !a.is_empty())
                    .unwrap_or(false);
                if !has_keys {
                    call.arguments = serde_json::json!({
                        "keys": ["win"]
                    });
                }
            }
        }

        ParsedModelOutput {
            mode,
            chat: chat_content,
            action,
            raw_text: trimmed.to_string(),
        }
    }

    /// Parses YAML tool call block e.g.:
    /// name: open_app
    /// arguments:
    ///   app_name: mspaint
    pub fn parse_yaml_tool_block(yaml_str: &str) -> Option<ExtractedToolCall> {
        let mut clean = yaml_str.trim();
        if clean.starts_with("```") {
            if let Some(pos) = clean.find('\n') {
                clean = &clean[pos + 1..];
            }
            if clean.ends_with("```") {
                clean = &clean[..clean.len() - 3];
            }
            clean = clean.trim();
        }
        let clean = clean.trim_matches('`').trim();

        // 1. Serde YAML deserialization directly into JSON Value
        if let Ok(val) = serde_yaml::from_str::<Value>(clean) {
            let mut tool_name = val
                .get("name")
                .or_else(|| val.get("tool"))
                .or_else(|| val.get("tool_name"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let mut args = Value::Null;

            if tool_name.is_some() {
                args = if let Some(a) = val.get("arguments").or_else(|| val.get("parameters")).or_else(|| val.get("args")) {
                    a.clone()
                } else if let Some(map) = val.as_object() {
                    let mut filtered = serde_json::Map::new();
                    for (k, v) in map {
                        if k != "name" && k != "tool" && k != "tool_name" {
                            filtered.insert(k.clone(), v.clone());
                        }
                    }
                    if filtered.is_empty() {
                        Value::Null
                    } else {
                        Value::Object(filtered)
                    }
                } else {
                    Value::Null
                };
            } else if let Some(map) = val.as_object() {
                if map.len() == 1 {
                    let (k, v) = map.iter().next().unwrap();
                    tool_name = Some(k.clone());
                    args = v.clone();
                }
            }

            if let Some(name) = tool_name {
                return Some(ExtractedToolCall {
                    tool_name: name,
                    arguments: args,
                    raw_matched: clean.to_string(),
                });
            }
        }

        // 2. Regex fallback for loose YAML lines:
        let name_re = Regex::new(r#"(?im)^\s*(?:name|tool|tool_name):\s*([a-zA-Z0-9_\-\.]+)"#).ok()?;
        if let Some(caps) = name_re.captures(clean) {
            let name = caps.get(1)?.as_str().to_string();
            let mut args = Value::Null;

            if let Some(arg_idx) = clean.find("arguments:") {
                let arg_substr = clean[arg_idx + 10..].trim();
                if let Ok(arg_val) = serde_yaml::from_str::<Value>(arg_substr) {
                    args = arg_val;
                }
            } else {
                let mut flat_map = serde_json::Map::new();
                for line in clean.lines() {
                    let l = line.trim();
                    if let Some(pos) = l.find(':') {
                        let k = l[..pos].trim();
                        let v = l[pos + 1..].trim().trim_matches('"').trim_matches('\'');
                        if k != "name" && k != "tool" && k != "tool_name" && !k.is_empty() {
                            flat_map.insert(k.to_string(), Value::String(v.to_string()));
                        }
                    }
                }
                if !flat_map.is_empty() {
                    args = Value::Object(flat_map);
                }
            }

            return Some(ExtractedToolCall {
                tool_name: name,
                arguments: args,
                raw_matched: clean.to_string(),
            });
        }

        None
    }

    /// Dispatches execution to Native tools or MCP servers
    pub async fn execute(
        registry: &ToolRegistry,
        mcp_manager: &McpManager,
        call: &ExtractedToolCall,
    ) -> Result<String, String> {
        let tool_def = registry
            .get_tool(&call.tool_name)
            .ok_or_else(|| {
                ErrorCheckpoint::new(
                    ErrorCategory::ToolNonFunctional,
                    "Unrecognized Tool Requested",
                    &format!("The tool '{}' is not registered in the system.", call.tool_name),
                    None,
                    "Type '/tool' to view available tools, or drop a tool definition into tools/ folder.",
                )
                .to_plain_string()
            })?;

        if !tool_def.enabled {
            return Err(ErrorCheckpoint::new(
                ErrorCategory::ToolDisabled,
                "Tool Currently Disabled",
                &format!("The tool '{}' is currently deactivated.", call.tool_name),
                None,
                &format!("Re-enable this tool by running '/tool enable {}'.", call.tool_name),
            )
            .to_plain_string());
        }

        match &tool_def.source {
            ToolSource::Native => match call.tool_name.as_str() {
                // Vision (Visual Navigation) Tools routed via C++ Subprocess Bridge (Option A)
                "capture_screen" => crate::tools::os_bridge_client::OsBridgeClient::capture_screen(&call.arguments),
                "click_coordinates" => crate::tools::os_bridge_client::OsBridgeClient::click_coordinates(&call.arguments),
                "type_text" => crate::tools::os_bridge_client::OsBridgeClient::type_text(&call.arguments),
                "drag_mouse" => crate::tools::os_bridge_client::OsBridgeClient::drag_mouse(&call.arguments),
                "send_hotkey" => crate::tools::os_bridge_client::OsBridgeClient::send_hotkey(&call.arguments),
                // Metadata (Accessibility & Window Tree) Tools routed via C++ Subprocess Bridge (Option A)
                "get_screen_state" | "get_desktop_metadata" => crate::tools::os_bridge_client::OsBridgeClient::get_screen_state(&call.arguments),
                "click_element" => crate::tools::os_bridge_client::OsBridgeClient::click_element(&call.arguments),
                "type_element" => crate::tools::os_bridge_client::OsBridgeClient::type_text(&call.arguments),
                "window_control" => crate::tools::os_bridge_client::OsBridgeClient::window_control(&call.arguments),
                other => Err(ErrorCheckpoint::new(
                    ErrorCategory::ToolNonFunctional,
                    "Native Tool Implementation Missing",
                    &format!("No built-in handler found for tool '{}'.", other),
                    None,
                    "Check tool registry registration.",
                )
                .to_plain_string()),
            },
            ToolSource::Mcp { .. } => {
                mcp_manager.call_mcp_tool(&call.tool_name, call.arguments.clone()).await
            }
            ToolSource::CustomFile { command, args, file_path } => {
                let cmd_to_run = command.as_deref().unwrap_or("powershell");
                let mut proc_cmd = std::process::Command::new(cmd_to_run);

                // Build args
                if let Some(base_args) = args {
                    for arg in base_args {
                        // Check if placeholder like {arg_name}
                        let mut resolved_arg = arg.clone();
                        if let Some(obj) = call.arguments.as_object() {
                            for (k, v) in obj {
                                let placeholder = format!("{{{}}}", k);
                                let val_str = match v {
                                    Value::String(s) => s.clone(),
                                    other => other.to_string(),
                                };
                                resolved_arg = resolved_arg.replace(&placeholder, &val_str);
                            }
                        }
                        proc_cmd.arg(resolved_arg);
                    }
                } else {
                    // Default to passing file path and JSON arguments
                    proc_cmd.arg(file_path);
                    let args_str = serde_json::to_string(&call.arguments).unwrap_or_default();
                    proc_cmd.arg(args_str);
                }

                let output = proc_cmd.output().map_err(|e| {
                    ErrorCheckpoint::new(
                        ErrorCategory::ToolNonFunctional,
                        "Custom Tool Process Spawn Failure",
                        &format!("Failed to spawn custom tool executable '{}'", cmd_to_run),
                        Some(&e.to_string()),
                        "Ensure the executable command is in PATH or configure tools/ manifest.",
                    )
                    .to_plain_string()
                })?;

                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                let exit_code = output.status.code().unwrap_or(-1);

                if !output.status.success() {
                    let details = if !stderr.trim().is_empty() {
                        stderr.trim().to_string()
                    } else {
                        stdout.trim().to_string()
                    };

                    return Err(ErrorCheckpoint::new(
                        ErrorCategory::ToolNonFunctional,
                        "Custom Tool Execution Failed",
                        &format!("Custom tool '{}' exited with non-zero exit code ({})", call.tool_name, exit_code),
                        Some(&details),
                        "Inspect custom tool script logs and verify arguments passed in tool call.",
                    )
                    .to_plain_string());
                }

                Ok(stdout.trim().to_string())
            }
            ToolSource::Script { script_path } => {
                let args_json = serde_json::to_string(&call.arguments).unwrap_or_default();
                let output = std::process::Command::new("python")
                    .args([script_path, "--args", &args_json])
                    .output()
                    .map_err(|e| {
                        ErrorCheckpoint::new(
                            ErrorCategory::ToolNonFunctional,
                            "Python Script Tool Process Launch Error",
                            &format!("Failed running Python tool script at '{}'", script_path),
                            Some(&e.to_string()),
                            "Ensure Python 3 is installed and accessible via 'python' in your system PATH.",
                        )
                        .to_plain_string()
                    })?;

                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                let exit_code = output.status.code().unwrap_or(-1);

                if !output.status.success() {
                    return Err(ErrorCheckpoint::new(
                        ErrorCategory::ToolNonFunctional,
                        "Python Script Tool Returned Error",
                        &format!("Script exited with code {}", exit_code),
                        Some(stderr.trim()),
                        "Check Python script errors and dependencies.",
                    )
                    .to_plain_string());
                }

                Ok(stdout.trim().to_string())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_model_output_task_completed() {
        let text = r#"Chat: Hello! I am ready to help you with anything you need.
................................................................
Tool: task_completed"#;

        let parsed = ToolExecutor::parse_model_output(text);
        assert_eq!(parsed.chat, "Hello! I am ready to help you with anything you need.");
        assert_eq!(parsed.action, ToolAction::TaskCompleted);
    }

    #[test]
    fn test_parse_model_output_yaml_tool_call() {
        let text = r#"Chat: Opening Notepad for you now.
................................................................
Tool:
  name: open_app
  arguments:
    app_name: notepad"#;

        let parsed = ToolExecutor::parse_model_output(text);
        assert_eq!(parsed.chat, "Opening Notepad for you now.");
        match parsed.action {
            ToolAction::Execute(call) => {
                assert_eq!(call.tool_name, "open_app");
                assert_eq!(call.arguments["app_name"], "notepad");
            }
            other => panic!("Expected ToolAction::Execute, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_model_output_fenced_yaml_tool_call() {
        let text = "Chat: Checking system status.\n................................................................\nTool:\n```yaml\nname: get_system_status\narguments: {}\n```";

        let parsed = ToolExecutor::parse_model_output(text);
        assert_eq!(parsed.chat, "Checking system status.");
        match parsed.action {
            ToolAction::Execute(call) => {
                assert_eq!(call.tool_name, "get_system_status");
            }
            other => panic!("Expected ToolAction::Execute, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_model_output_no_tool_yields_none() {
        let text = "The capital of France is Paris.";
        let parsed = ToolExecutor::parse_model_output(text);
        assert_eq!(parsed.chat, "The capital of France is Paris.");
        assert_eq!(parsed.action, ToolAction::None);
    }

    #[test]
    fn test_parse_model_output_done_alias() {
        let text = "Chat: All done!\n--------------------------------\nTool: done";
        let parsed = ToolExecutor::parse_model_output(text);
        assert_eq!(parsed.chat, "All done!");
        assert_eq!(parsed.action, ToolAction::TaskCompleted);
    }

    #[test]
    fn test_parse_model_output_single_line_tool_call() {
        let text = "Chat: Taking a screenshot.\n................................................................\nTool: capture_screen";
        let parsed = ToolExecutor::parse_model_output(text);
        assert_eq!(parsed.chat, "Taking a screenshot.");
        match parsed.action {
            ToolAction::Execute(call) => {
                assert_eq!(call.tool_name, "capture_screen");
            }
            other => panic!("Expected ToolAction::Execute, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_model_output_malformed_yaml_yields_none() {
        let text = "Chat: I am thinking.\n................................................................\nTool:\n  [invalid yaml: {";
        let parsed = ToolExecutor::parse_model_output(text);
        assert_eq!(parsed.chat, "I am thinking.");
        assert_eq!(parsed.action, ToolAction::None);
    }

    #[test]
    fn test_parse_model_output_with_mode() {
        let text = "Mode: task\nChat: Capturing the screen now.\n................................................................\nTool:\n  name: capture_screen\n  arguments: {}";
        let parsed = ToolExecutor::parse_model_output(text);
        assert_eq!(parsed.mode, AgentMode::Task);
        assert_eq!(parsed.chat, "Capturing the screen now.");
        match parsed.action {
            ToolAction::Execute(call) => {
                assert_eq!(call.tool_name, "capture_screen");
            }
            other => panic!("Expected Execute, got {:?}", other),
        }

        let chat_text = "Mode: chat\nChat: Hello there!";
        let parsed_chat = ToolExecutor::parse_model_output(chat_text);
        assert_eq!(parsed_chat.mode, AgentMode::Chat);
        assert_eq!(parsed_chat.chat, "Hello there!");
        assert_eq!(parsed_chat.action, ToolAction::None);

        let task_done_text = "Mode: task\nChat: Task finished.\n................................................................\nTool: task_completed";
        let parsed_task_done = ToolExecutor::parse_model_output(task_done_text);
        assert_eq!(parsed_task_done.mode, AgentMode::Task);
        assert_eq!(parsed_task_done.chat, "Task finished.");
        assert_eq!(parsed_task_done.action, ToolAction::TaskCompleted);
    }

    #[test]
    fn test_parse_model_output_smart_recovery_type_text() {
        let text = "Mode: task\nChat: Next, I will type \"edge\" to launch the Edge browser.\n................................................................\nTool: type_text";
        let parsed = ToolExecutor::parse_model_output(text);
        assert_eq!(parsed.mode, AgentMode::Task);
        match parsed.action {
            ToolAction::Execute(call) => {
                assert_eq!(call.tool_name, "type_text");
                assert_eq!(call.arguments.get("text").unwrap().as_str().unwrap(), "edge");
                assert_eq!(call.arguments.get("press_enter").unwrap().as_bool().unwrap(), true);
            }
            other => panic!("Expected Execute, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_model_output_no_arguments_block() {
        let text = "Mode: task\nChat: Inspecting desktop elements.\n................................................................\nTool:\n  name: get_screen_state";
        let parsed = ToolExecutor::parse_model_output(text);
        assert_eq!(parsed.mode, AgentMode::Task);
        assert_eq!(parsed.chat, "Inspecting desktop elements.");
        match parsed.action {
            ToolAction::Execute(call) => {
                assert_eq!(call.tool_name, "get_screen_state");
                assert!(call.arguments.is_null());
            }
            other => panic!("Expected Execute, got {:?}", other),
        }
    }
}
