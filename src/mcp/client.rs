use crate::checkpoint::{ErrorCategory, ErrorCheckpoint};
use crate::mcp::protocol::{JsonRpcRequest, JsonRpcResponse, McpTool};
use serde_json::{json, Value};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;

pub struct McpClient {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    stdin: Arc<Mutex<ChildStdin>>,
    stdout_reader: Arc<Mutex<BufReader<ChildStdout>>>,
    request_id: AtomicU64,
    child: Arc<Mutex<Child>>,
}

impl McpClient {
    pub async fn spawn(name: &str, command: &str, args: &[String]) -> Result<Self, String> {
        let mut cmd = Command::new(command);
        cmd.args(args);
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::inherit());

        let mut child = cmd.spawn().map_err(|e| {
            ErrorCheckpoint::new(
                ErrorCategory::McpUnreachable,
                "MCP Server Process Spawn Failure",
                &format!("Failed to start child process for MCP server '{}' using command '{}'", name, command),
                Some(&e.to_string()),
                "Verify the command executable (e.g. python, uv, node) is installed on PATH and script arguments are valid.",
            )
            .to_plain_string()
        })?;

        let stdin = child.stdin.take().ok_or_else(|| {
            ErrorCheckpoint::new(
                ErrorCategory::McpUnreachable,
                "MCP Stdin Pipe Failure",
                "Failed to capture standard input stream for MCP server child process.",
                None,
                "Check process execution privileges and environment.",
            )
            .to_plain_string()
        })?;

        let stdout = child.stdout.take().ok_or_else(|| {
            ErrorCheckpoint::new(
                ErrorCategory::McpUnreachable,
                "MCP Stdout Pipe Failure",
                "Failed to capture standard output stream for MCP server child process.",
                None,
                "Check process execution privileges and environment.",
            )
            .to_plain_string()
        })?;

        let client = Self {
            name: name.to_string(),
            command: command.to_string(),
            args: args.to_vec(),
            stdin: Arc::new(Mutex::new(stdin)),
            stdout_reader: Arc::new(Mutex::new(BufReader::new(stdout))),
            request_id: AtomicU64::new(1),
            child: Arc::new(Mutex::new(child)),
        };

        // Initialize MCP handshake
        client.initialize().await.map_err(|e| {
            ErrorCheckpoint::new(
                ErrorCategory::McpUnreachable,
                "MCP Handshake Initialization Failed",
                &format!("MCP server '{}' failed during JSON-RPC initialize handshake.", name),
                Some(&e),
                "Verify that the target server implements the MCP stdio protocol correctly.",
            )
            .to_plain_string()
        })?;

        Ok(client)
    }

    pub async fn send_request(&self, method: &str, params: Option<Value>) -> Result<Value, String> {
        let id = self.request_id.fetch_add(1, Ordering::SeqCst);
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(id),
            method: method.to_string(),
            params,
        };

        let mut json_str = serde_json::to_string(&req).map_err(|e| {
            ErrorCheckpoint::new(
                ErrorCategory::McpExecutionFailed,
                "MCP JSON Serialization Failure",
                "Failed to serialize JSON-RPC request to string.",
                Some(&e.to_string()),
                "Verify that tool arguments are JSON-serializable.",
            )
            .to_plain_string()
        })?;
        json_str.push('\n');

        // Write line to stdin
        {
            let mut stdin = self.stdin.lock().await;
            stdin
                .write_all(json_str.as_bytes())
                .await
                .map_err(|e| {
                    ErrorCheckpoint::new(
                        ErrorCategory::McpUnreachable,
                        "MCP Stdin Write Failed",
                        &format!("Failed writing to stdin of MCP server '{}'", self.name),
                        Some(&e.to_string()),
                        "Check if the server process crashed or closed its input channel.",
                    )
                    .to_plain_string()
                })?;
            stdin
                .flush()
                .await
                .map_err(|e| {
                    ErrorCheckpoint::new(
                        ErrorCategory::McpUnreachable,
                        "MCP Stdin Flush Failed",
                        &format!("Failed flushing stdin stream for MCP server '{}'", self.name),
                        Some(&e.to_string()),
                        "Ensure the server process is responsive and not hanging.",
                    )
                    .to_plain_string()
                })?;
        }

        // Read response from stdout
        let mut line = String::new();
        let mut reader = self.stdout_reader.lock().await;

        loop {
            line.clear();
            let n = reader
                .read_line(&mut line)
                .await
                .map_err(|e| {
                    ErrorCheckpoint::new(
                        ErrorCategory::McpExecutionFailed,
                        "MCP Stdout Read Failed",
                        &format!("Error reading response line from MCP server '{}'", self.name),
                        Some(&e.to_string()),
                        "Check server output for unexpected exceptions or non-JSON messages.",
                    )
                    .to_plain_string()
                })?;
            if n == 0 {
                return Err(ErrorCheckpoint::new(
                    ErrorCategory::McpUnreachable,
                    "MCP Connection Terminated Prematurely",
                    &format!("MCP server '{}' closed connection unexpectedly (EOF on stdout).", self.name),
                    None,
                    "Check server terminal / stderr output to diagnose fatal crash or missing dependencies.",
                )
                .to_plain_string());
            }

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            if let Ok(resp) = serde_json::from_str::<JsonRpcResponse>(trimmed) {
                if resp.id == Some(id) {
                    if let Some(err) = resp.error {
                        return Err(ErrorCheckpoint::new(
                            ErrorCategory::McpExecutionFailed,
                            "MCP Protocol Error",
                            &format!("MCP server returned JSON-RPC error (Code {})", err.code),
                            Some(&err.message),
                            "Check tool input arguments and ensure they conform to the tool parameter schema.",
                        )
                        .to_plain_string());
                    }
                    return Ok(resp.result.unwrap_or(Value::Null));
                }
            }
        }
    }

    pub async fn send_notification(&self, method: &str, params: Option<Value>) -> Result<(), String> {
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: None,
            method: method.to_string(),
            params,
        };

        let mut json_str = serde_json::to_string(&req).map_err(|e| e.to_string())?;
        json_str.push('\n');

        let mut stdin = self.stdin.lock().await;
        stdin
            .write_all(json_str.as_bytes())
            .await
            .map_err(|e| format!("MCP notification write failed: {}", e))?;
        stdin
            .flush()
            .await
            .map_err(|e| format!("MCP notification flush failed: {}", e))?;
        Ok(())
    }

    async fn initialize(&self) -> Result<(), String> {
        let params = json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "roots": { "listChanged": true },
                "sampling": {}
            },
            "clientInfo": {
                "name": "jaa-cli-agent",
                "version": "1.0.0"
            }
        });

        self.send_request("initialize", Some(params)).await?;
        self.send_notification("notifications/initialized", None).await?;
        Ok(())
    }

    pub async fn list_tools(&self) -> Result<Vec<McpTool>, String> {
        let res = self.send_request("tools/list", None).await?;
        let tools_val = res.get("tools").ok_or_else(|| {
            ErrorCheckpoint::new(
                ErrorCategory::McpExecutionFailed,
                "Malformed MCP tools/list Response",
                "Missing 'tools' array in response from MCP server.",
                Some(&res.to_string()),
                "Verify that server supports standard MCP tools/list method.",
            )
            .to_plain_string()
        })?;
        let tools: Vec<McpTool> = serde_json::from_value(tools_val.clone()).map_err(|e| {
            ErrorCheckpoint::new(
                ErrorCategory::McpExecutionFailed,
                "Invalid Tool Schema in MCP Response",
                "Failed to deserialize tool definition array from MCP server.",
                Some(&e.to_string()),
                "Ensure tool definitions conform to the MCP protocol schema.",
            )
            .to_plain_string()
        })?;
        Ok(tools)
    }

    pub async fn call_tool(&self, tool_name: &str, arguments: Value) -> Result<String, String> {
        let safe_arguments = if arguments.is_null() {
            json!({})
        } else {
            arguments
        };
        let params = json!({
            "name": tool_name,
            "arguments": safe_arguments
        });

        let res = self.send_request("tools/call", Some(params)).await?;

        // Check for MCP isError flag
        let is_error = res.get("isError").and_then(|v| v.as_bool()).unwrap_or(false);

        // Format content array or text
        let mut content_out = String::new();
        if let Some(content_arr) = res.get("content").and_then(|c| c.as_array()) {
            for item in content_arr {
                if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                    content_out.push_str(text);
                    content_out.push('\n');
                }
            }
        }

        let output_str = if !content_out.is_empty() {
            content_out.trim().to_string()
        } else {
            res.to_string()
        };

        if is_error {
            return Err(ErrorCheckpoint::new(
                ErrorCategory::McpExecutionFailed,
                "MCP Tool Reported Execution Failure",
                &format!("Tool '{}' returned an error state.", tool_name),
                Some(&output_str),
                "Review tool inputs and host OS prerequisites (e.g. windows accessibility, screen permissions).",
            )
            .to_plain_string());
        }

        Ok(output_str)
    }

    pub async fn terminate(&self) {
        let mut child = self.child.lock().await;
        let _ = child.kill().await;
    }
}
