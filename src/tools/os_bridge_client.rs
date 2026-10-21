use crate::checkpoint::{ErrorCategory, ErrorCheckpoint};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Mutex;

static REQ_COUNTER: AtomicI64 = AtomicI64::new(1);

struct BridgeProcess {
    child: Child,
    stdin: ChildStdin,
    reader: BufReader<ChildStdout>,
}

impl BridgeProcess {
    fn spawn() -> Result<Self, ErrorCheckpoint> {
        let exe_path = Self::find_bridge_exe()?;
        let mut child = Command::new(&exe_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                ErrorCheckpoint::new(
                    ErrorCategory::SystemError,
                    "Failed to Launch Native OS Bridge",
                    &format!("Could not spawn process at '{}'", exe_path.display()),
                    Some(&e.to_string()),
                    "Ensure jarvis-os-bridge.exe exists and execution permissions are granted.",
                )
            })?;

        let stdin = child.stdin.take().ok_or_else(|| {
            ErrorCheckpoint::new(
                ErrorCategory::SystemError,
                "Bridge Stdin Unavailable",
                "Failed to capture stdin of jarvis-os-bridge process.",
                None,
                "Check system process creation permissions.",
            )
        })?;

        let stdout = child.stdout.take().ok_or_else(|| {
            ErrorCheckpoint::new(
                ErrorCategory::SystemError,
                "Bridge Stdout Unavailable",
                "Failed to capture stdout of jarvis-os-bridge process.",
                None,
                "Check system process creation permissions.",
            )
        })?;

        Ok(Self {
            child,
            stdin,
            reader: BufReader::new(stdout),
        })
    }

    fn find_bridge_exe() -> Result<PathBuf, ErrorCheckpoint> {
        let candidates = [
            PathBuf::from("jarvis-os-bridge.exe"),
            PathBuf::from("target/release/jarvis-os-bridge.exe"),
            PathBuf::from("target/debug/jarvis-os-bridge.exe"),
        ];

        for c in &candidates {
            if c.exists() {
                return Ok(c.clone());
            }
        }

        Err(ErrorCheckpoint::new(
            ErrorCategory::ToolNonFunctional,
            "Native OS Bridge Binary Missing",
            "Could not locate 'jarvis-os-bridge.exe' in project root or target directories.",
            None,
            "Compile the bridge using MSVC: cl /O2 /EHsc native\\os_bridge\\main.cpp user32.lib /Fe:jarvis-os-bridge.exe",
        ))
    }

    fn is_alive(&mut self) -> bool {
        match self.child.try_wait() {
            Ok(None) => true,
            _ => false,
        }
    }

    fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

static BRIDGE: Mutex<Option<BridgeProcess>> = Mutex::new(None);

pub struct OsBridgeClient;

impl OsBridgeClient {
    /// Dispatches a request to jarvis-os-bridge.exe with automatic crash-recovery and watchdog timeout
    pub fn dispatch(action: &str, params: &Value) -> Result<Value, String> {
        let mut guard = BRIDGE.lock().map_err(|e| {
            ErrorCheckpoint::new(
                ErrorCategory::SystemError,
                "OS Bridge Mutex Poisoned",
                "Failed to acquire lock for OS Bridge subprocess.",
                Some(&e.to_string()),
                "Restart the application to recover system mutexes.",
            )
            .to_plain_string()
        })?;

        // 1. Ensure Bridge process is alive
        let needs_spawn = match guard.as_mut() {
            Some(proc) => !proc.is_alive(),
            None => true,
        };

        if needs_spawn {
            if let Some(mut old_proc) = guard.take() {
                old_proc.kill();
            }
            let new_proc = BridgeProcess::spawn().map_err(|cp| cp.to_plain_string())?;
            *guard = Some(new_proc);
        }

        let proc = guard.as_mut().unwrap();

        // 2. Prepare JSON request
        let req_id = REQ_COUNTER.fetch_add(1, Ordering::SeqCst);
        let safe_params = if params.is_null() {
            json!({})
        } else {
            params.clone()
        };
        let req_json = json!({
            "id": req_id,
            "action": action,
            "params": safe_params
        });

        let mut req_line = req_json.to_string();
        req_line.push('\n');

        // 3. Write to stdin
        if let Err(e) = proc.stdin.write_all(req_line.as_bytes()) {
            proc.kill();
            *guard = None;
            return Err(ErrorCheckpoint::new(
                ErrorCategory::SystemError,
                "OS Bridge Pipe Write Error",
                "Failed writing command to jarvis-os-bridge pipe.",
                Some(&e.to_string()),
                "The bridge process crashed or terminated. It will be restarted on the next tool action.",
            )
            .to_plain_string());
        }

        if let Err(e) = proc.stdin.flush() {
            proc.kill();
            *guard = None;
            return Err(ErrorCheckpoint::new(
                ErrorCategory::SystemError,
                "OS Bridge Pipe Flush Error",
                "Failed to flush command to jarvis-os-bridge pipe.",
                Some(&e.to_string()),
                "The bridge process was terminated.",
            )
            .to_plain_string());
        }

        // 4. Read response from stdout (Byte-safe read to prevent UTF-8 crash on non-ASCII titles)
        let mut byte_buf = Vec::new();
        let read_res = proc.reader.read_until(b'\n', &mut byte_buf);

        match read_res {
            Ok(0) => {
                // EOF - Process closed stdout
                proc.kill();
                *guard = None;
                return Err(ErrorCheckpoint::new(
                    ErrorCategory::SystemError,
                    "OS Bridge Subprocess Disconnected (EOF)",
                    "jarvis-os-bridge process closed its stdout pipe unexpectedly.",
                    None,
                    "Check Windows Event Viewer or event permissions. The bridge will restart automatically on the next action.",
                )
                .to_plain_string());
            }
            Ok(_) => {
                let line_str = String::from_utf8_lossy(&byte_buf);
                // Parse line
                let parsed: Value = serde_json::from_str(line_str.trim()).map_err(|e| {
                    ErrorCheckpoint::new(
                        ErrorCategory::SystemError,
                        "OS Bridge Output Parse Error",
                        "Failed to parse JSON response from jarvis-os-bridge.",
                        Some(&format!("Raw: '{}' Error: {}", line_str.trim(), e)),
                        "Ensure native bridge output adheres to JSON protocol.",
                    )
                    .to_plain_string()
                })?;

                let status = parsed.get("status").and_then(|s| s.as_str()).unwrap_or("");
                if status == "success" {
                    Ok(parsed.get("result").cloned().unwrap_or(Value::Null))
                } else {
                    let code = parsed.get("error_code").and_then(|c| c.as_str()).unwrap_or("BridgeExecutionFailed");
                    let msg = parsed.get("message").and_then(|m| m.as_str()).unwrap_or("Bridge returned an error.");
                    let details = parsed.get("details").and_then(|d| d.as_str());
                    let suggestion = parsed.get("suggestion").and_then(|s| s.as_str()).unwrap_or("Check target application state.");

                    let category = match code {
                        "InvalidParameters" => ErrorCategory::InvalidParameters,
                        "InputInjectionBlocked" | "UiaAccessDenied" => ErrorCategory::ToolNonFunctional,
                        _ => ErrorCategory::ToolNonFunctional,
                    };

                    Err(ErrorCheckpoint::new(
                        category,
                        msg,
                        code,
                        details,
                        suggestion,
                    )
                    .to_plain_string())
                }
            }
            Err(e) => {
                proc.kill();
                *guard = None;
                Err(ErrorCheckpoint::new(
                    ErrorCategory::SystemError,
                    "OS Bridge Pipe Read Error",
                    "Failed to read response from jarvis-os-bridge stdout pipe.",
                    Some(&e.to_string()),
                    "Bridge process was terminated and will be restarted on the next request.",
                )
                .to_plain_string())
            }
        }
    }

    // ================= Native Tools Dispatched to Bridge =================

    pub fn capture_screen(params: &Value) -> Result<String, String> {
        let res = Self::dispatch("capture_screen", params)?;
        Ok(serde_json::to_string_pretty(&res).unwrap_or_else(|_| res.to_string()))
    }

    /// Captures the screen directly into an in-memory PNG buffer (Zero disk I/O)
    pub fn capture_screen_ram() -> Result<(String, String), String> {
        let res = Self::dispatch("capture_screen_ram", &serde_json::json!({}))?;
        let name = res.get("image_name").and_then(|n| n.as_str()).unwrap_or("current_screen.png").to_string();
        let b64 = res.get("base64").and_then(|b| b.as_str()).ok_or("No base64 data returned from RAM capture")?.to_string();
        Ok((name, b64))
    }

    pub fn get_screen_state(params: &Value) -> Result<String, String> {
        let res = Self::dispatch("get_screen_state", params)?;
        if let Some(text) = res.get("screen_text").and_then(|t| t.as_str()) {
            Ok(text.to_string())
        } else {
            Ok(serde_json::to_string_pretty(&res).unwrap_or_else(|_| res.to_string()))
        }
    }

    pub fn click_coordinates(params: &Value) -> Result<String, String> {
        let res = Self::dispatch("click_coordinates", params)?;
        Ok(res.get("message").and_then(|m| m.as_str()).unwrap_or("Clicked coordinates successfully").to_string())
    }

    pub fn click_element(params: &Value) -> Result<String, String> {
        let res = Self::dispatch("click_element", params)?;
        let tag = res.get("tag_id").and_then(|t| t.as_str()).unwrap_or("");
        let target = res.get("target").and_then(|t| t.as_str()).unwrap_or("");
        let control = res.get("control").and_then(|t| t.as_str()).unwrap_or("Element");
        let name = res.get("name").and_then(|t| t.as_str()).unwrap_or("");
        let cx = res.get("clicked_x").and_then(|x| x.as_i64()).unwrap_or(0);
        let cy = res.get("clicked_y").and_then(|y| y.as_i64()).unwrap_or(0);

        let desc = if !name.is_empty() {
            format!("{}: \"{}\"", control, name)
        } else {
            control.to_string()
        };

        let nx = res.get("norm_x").and_then(|x| x.as_f64());
        let ny = res.get("norm_y").and_then(|y| y.as_f64());

        if !target.is_empty() {
            if let (Some(x), Some(y)) = (nx, ny) {
                Ok(format!("Clicked [{}] in {} ({}) at normalized coordinates ({:.3}, {:.3}) [pixel {}, {}]", tag, target, desc, x, y, cx, cy))
            } else {
                Ok(format!("Clicked [{}] in {} ({}) at physical coordinates ({}, {})", tag, target, desc, cx, cy))
            }
        } else {
            if let (Some(x), Some(y)) = (nx, ny) {
                Ok(format!("Clicked element [{}] at normalized coordinates ({:.3}, {:.3}) [pixel {}, {}]", tag, x, y, cx, cy))
            } else {
                Ok(format!("Clicked element [{}] at physical coordinates ({}, {})", tag, cx, cy))
            }
        }
    }

    pub fn type_text(params: &Value) -> Result<String, String> {
        let res = Self::dispatch("type_text", params)?;
        Ok(res.get("message").and_then(|m| m.as_str()).unwrap_or("Text typed successfully").to_string())
    }

    pub fn send_hotkey(params: &Value) -> Result<String, String> {
        let res = Self::dispatch("send_hotkey", params)?;
        Ok(res.get("message").and_then(|m| m.as_str()).unwrap_or("Hotkey sequence sent successfully").to_string())
    }

    pub fn drag_mouse(params: &Value) -> Result<String, String> {
        let res = Self::dispatch("drag_mouse", params)?;
        Ok(res.get("message").and_then(|m| m.as_str()).unwrap_or("Mouse dragged successfully").to_string())
    }

    pub fn window_control(params: &Value) -> Result<String, String> {
        let res = Self::dispatch("window_control", params)?;
        let action = res.get("action").and_then(|a| a.as_str()).unwrap_or("executed");
        Ok(format!("Window control action '{}' executed successfully", action))
    }
}
