use crate::checkpoint::{ErrorCategory, ErrorCheckpoint};
use serde_json::{json, Value};
use std::fs;
use std::path::Path;
use std::process::Command;

pub struct BuiltinTools;

impl BuiltinTools {
    pub fn execute_command(args: &Value) -> Result<String, String> {
        let cmd_str = args
            .get("command")
            .and_then(|c| c.as_str())
            .ok_or_else(|| {
                ErrorCheckpoint::new(
                    ErrorCategory::InvalidParameters,
                    "Missing Parameter 'command'",
                    "The tool 'execute_command' requires a 'command' string argument.",
                    None,
                    "Provide a valid command string, e.g. {\"command\": \"dir\"}",
                )
                .to_plain_string()
            })?;

        let working_dir = args
            .get("cwd")
            .and_then(|d| d.as_str())
            .unwrap_or(".");

        #[cfg(target_os = "windows")]
        let output = Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", cmd_str])
            .current_dir(working_dir)
            .output()
            .map_err(|e| {
                ErrorCheckpoint::new(
                    ErrorCategory::ToolNonFunctional,
                    "Command Process Spawn Failure",
                    "Failed to launch PowerShell process on the host OS.",
                    Some(&e.to_string()),
                    "Ensure powershell.exe is accessible in system PATH and permissions allow process execution.",
                )
                .to_plain_string()
            })?;

        #[cfg(not(target_os = "windows"))]
        let output = Command::new("sh")
            .args(["-c", cmd_str])
            .current_dir(working_dir)
            .output()
            .map_err(|e| {
                ErrorCheckpoint::new(
                    ErrorCategory::ToolNonFunctional,
                    "Command Process Spawn Failure",
                    "Failed to launch sh process on the host OS.",
                    Some(&e.to_string()),
                    "Ensure /bin/sh is accessible.",
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
                "Command Execution Failed",
                &format!("Command '{}' terminated with non-zero exit code ({})", cmd_str, exit_code),
                Some(&details),
                "Verify command arguments and check if any required files or dependencies are missing.",
            )
            .to_plain_string());
        }

        let mut res = String::new();
        if !stdout.is_empty() {
            res.push_str(&stdout);
        }
        if res.trim().is_empty() {
            res.push_str("[Command completed successfully with no stdout output]");
        }

        Ok(res.trim().to_string())
    }

    pub fn read_file(args: &Value) -> Result<String, String> {
        let path_str = args
            .get("path")
            .and_then(|p| p.as_str())
            .ok_or_else(|| {
                ErrorCheckpoint::new(
                    ErrorCategory::InvalidParameters,
                    "Missing Parameter 'path'",
                    "The tool 'read_file' requires a 'path' argument.",
                    None,
                    "Provide a valid file path, e.g. {\"path\": \"README.md\"}",
                )
                .to_plain_string()
            })?;

        let path = Path::new(path_str);
        if !path.exists() {
            return Err(ErrorCheckpoint::new(
                ErrorCategory::ToolNonFunctional,
                "File Not Found",
                &format!("The requested file '{}' does not exist on disk.", path_str),
                None,
                "Check file path spelling or use 'list_directory' to see available files in the current directory.",
            )
            .to_plain_string());
        }

        let content = fs::read_to_string(path)
            .map_err(|e| {
                ErrorCheckpoint::new(
                    ErrorCategory::ToolNonFunctional,
                    "File Read Access Error",
                    &format!("Failed to read content of '{}'", path_str),
                    Some(&e.to_string()),
                    "Verify file permissions and ensure the file is not locked by another process.",
                )
                .to_plain_string()
            })?;

        // Support optional start_line and end_line
        let start_line = args.get("start_line").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
        let end_line = args.get("end_line").and_then(|v| v.as_u64()).map(|v| v as usize);

        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();

        let s_idx = if start_line > 0 { start_line - 1 } else { 0 };
        let e_idx = match end_line {
            Some(e) => e.min(total_lines),
            None => total_lines,
        };

        if s_idx >= total_lines {
            return Ok(format!("File has {} lines; requested start line is {}.", total_lines, start_line));
        }

        let selected = &lines[s_idx..e_idx];
        let mut numbered = String::new();
        for (i, line) in selected.iter().enumerate() {
            numbered.push_str(&format!("{:4}: {}\n", s_idx + i + 1, line));
        }

        Ok(numbered.trim_end().to_string())
    }

    pub fn write_file(args: &Value) -> Result<String, String> {
        let path_str = args
            .get("path")
            .and_then(|p| p.as_str())
            .ok_or_else(|| {
                ErrorCheckpoint::new(
                    ErrorCategory::InvalidParameters,
                    "Missing Parameter 'path'",
                    "The tool 'write_file' requires a 'path' parameter.",
                    None,
                    "Provide a valid file path, e.g. {\"path\": \"output.txt\", \"content\": \"...\"}",
                )
                .to_plain_string()
            })?;

        let content = args
            .get("content")
            .and_then(|c| c.as_str())
            .ok_or_else(|| {
                ErrorCheckpoint::new(
                    ErrorCategory::InvalidParameters,
                    "Missing Parameter 'content'",
                    "The tool 'write_file' requires a 'content' string parameter.",
                    None,
                    "Provide text content to write.",
                )
                .to_plain_string()
            })?;

        let path = Path::new(path_str);
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }

        fs::write(path, content).map_err(|e| {
            ErrorCheckpoint::new(
                ErrorCategory::ToolNonFunctional,
                "File Write Access Denied",
                &format!("Unable to write {} bytes to '{}'", content.len(), path_str),
                Some(&e.to_string()),
                "Verify directory write permissions and ensure the path is not read-only.",
            )
            .to_plain_string()
        })?;
        Ok(format!("Successfully written {} bytes to {}", content.len(), path_str))
    }

    pub fn list_directory(args: &Value) -> Result<String, String> {
        let dir_str = args.get("path").and_then(|p| p.as_str()).unwrap_or(".");
        let path = Path::new(dir_str);
        if !path.exists() {
            return Err(ErrorCheckpoint::new(
                ErrorCategory::ToolNonFunctional,
                "Directory Not Found",
                &format!("Directory '{}' does not exist.", dir_str),
                None,
                "Verify directory path spelling or use '.' to inspect current working directory.",
            )
            .to_plain_string());
        }

        let entries = fs::read_dir(path).map_err(|e| {
            ErrorCheckpoint::new(
                ErrorCategory::ToolNonFunctional,
                "Directory Read Access Denied",
                &format!("Failed to read contents of directory '{}'", dir_str),
                Some(&e.to_string()),
                "Check folder read permissions.",
            )
            .to_plain_string()
        })?;
        let mut list = Vec::new();

        for entry in entries.filter_map(|e| e.ok()) {
            let file_name = entry.file_name().to_string_lossy().to_string();
            let file_type = if entry.path().is_dir() { "DIR" } else { "FILE" };
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            list.push(format!("{:<6} {:>10} bytes  {}", file_type, size, file_name));
        }

        Ok(list.join("\n"))
    }

    pub fn screen_capture(args: &Value) -> Result<String, String> {
        let output_path = args
            .get("output")
            .and_then(|o| o.as_str())
            .unwrap_or("cache/screen.png");

        let path = Path::new(output_path);
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }

        // PowerShell native screen grab command (zero external Python dependency)
        let ps_cmd = format!(
            "Add-Type -AssemblyName System.Windows.Forms; Add-Type -AssemblyName System.Drawing; \
             $bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds; \
             $bmp = New-Object System.Drawing.Bitmap $bounds.Width, $bounds.Height; \
             $graphics = [System.Drawing.Graphics]::FromImage($bmp); \
             $graphics.CopyFromScreen($bounds.Location, [System.Drawing.Point]::Empty, $bounds.Size); \
             $bmp.Save('{}', [System.Drawing.Imaging.ImageFormat]::Png); \
             $graphics.Dispose(); $bmp.Dispose();",
            path.display()
        );

        let output = Command::new("powershell.exe")
            .args(["-NoProfile", "-Command", &ps_cmd])
            .output()
            .map_err(|e| {
                ErrorCheckpoint::new(
                    ErrorCategory::ToolNonFunctional,
                    "Screen Capture Process Spawn Failed",
                    "Failed to launch PowerShell screenshot process.",
                    Some(&e.to_string()),
                    "Verify powershell.exe is accessible in PATH.",
                )
                .to_plain_string()
            })?;

        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr);
            return Err(ErrorCheckpoint::new(
                ErrorCategory::ToolNonFunctional,
                "Screen Capture Graphics Failure",
                "Primary display buffer capture failed.",
                Some(&err),
                "Verify desktop session is active and user session is not locked or headless.",
            )
            .to_plain_string());
        }

        Ok(format!("Screen captured successfully to '{}'", path.display()))
    }

    pub fn system_status(_args: &Value) -> Result<String, String> {
        let os_desc = std::env::consts::OS;
        let arch = std::env::consts::ARCH;

        // Windows system info query
        let host_name = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "localhost".to_string());
        let username = std::env::var("USERNAME").unwrap_or_else(|_| "user".to_string());

        let res = json!({
            "hostname": host_name,
            "username": username,
            "os": os_desc,
            "arch": arch,
            "working_directory": std::env::current_dir().unwrap_or_default().display().to_string(),
        });

        Ok(serde_json::to_string_pretty(&res).unwrap_or_default())
    }
}
