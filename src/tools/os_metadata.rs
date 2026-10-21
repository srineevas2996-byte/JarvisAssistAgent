use crate::checkpoint::{ErrorCategory, ErrorCheckpoint};
use crate::tools::os_vlm::OsVlmTools;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagElement {
    pub tag_id: usize,
    pub title: String,
    pub process: String,
    pub class_name: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub center_x: i32,
    pub center_y: i32,
}

static LATEST_TAG_MAP: Mutex<Option<Vec<TagElement>>> = Mutex::new(None);

pub struct OsMetadataTools;

impl OsMetadataTools {
    /// Scans the desktop window hierarchy and returns structured Tag_ID tree
    pub fn get_screen_state(_args: &Value) -> Result<String, String> {
        // Enumerate visible top-level windows with coordinates and process names via native PowerShell
        let ps_script = r#"
Add-Type @"
using System;
using System.Runtime.InteropServices;
using System.Text;
using System.Collections.Generic;

public class WinEnumerator {
    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static extern bool EnumWindows(EnumWindowsProc lpEnumFunc, IntPtr lParam);

    public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);

    [DllImport("user32.dll", SetLastError = true, CharSet = CharSet.Auto)]
    public static extern int GetWindowText(IntPtr hWnd, StringBuilder lpString, int nMaxCount);

    [DllImport("user32.dll", SetLastError = true, CharSet = CharSet.Auto)]
    public static extern int GetClassName(IntPtr hWnd, StringBuilder lpClassName, int nMaxCount);

    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static extern bool IsWindowVisible(IntPtr hWnd);

    [DllImport("user32.dll")]
    public static extern bool GetWindowRect(IntPtr hWnd, out RECT lpRect);

    [DllImport("user32.dll", SetLastError=true)]
    public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint lpdwProcessId);

    [StructLayout(LayoutKind.Sequential)]
    public struct RECT {
        public int Left;
        public int Top;
        public int Right;
        public int Bottom;
    }

    public static List<string> GetVisibleWindows() {
        var list = new List<string>();
        EnumWindows((hWnd, lParam) => {
            if (IsWindowVisible(hWnd)) {
                var sb = new StringBuilder(256);
                GetWindowText(hWnd, sb, 256);
                string title = sb.ToString().Trim();
                if (!string.IsNullOrEmpty(title) && title != "Program Manager") {
                    RECT r;
                    GetWindowRect(hWnd, out r);
                    int w = r.Right - r.Left;
                    int h = r.Bottom - r.Top;
                    if (w > 20 && h > 20) {
                        uint pid;
                        GetWindowThreadProcessId(hWnd, out pid);
                        string procName = "unknown";
                        try {
                            procName = System.Diagnostics.Process.GetProcessById((int)pid).ProcessName;
                        } catch {}

                        var classSb = new StringBuilder(128);
                        GetClassName(hWnd, classSb, 128);

                        list.Add(string.Format("{0}|{1}|{2}|{3}|{4}|{5}|{6}", title, procName, classSb.ToString(), r.Left, r.Top, w, h));
                    }
                }
            }
            return true;
        }, IntPtr.Zero);
        return list;
    }
}
"@ -ErrorAction SilentlyContinue

[WinEnumerator]::GetVisibleWindows()
"#;

        let output = Command::new("powershell.exe")
            .args(["-NoProfile", "-Command", ps_script])
            .output()
            .map_err(|e| {
                ErrorCheckpoint::new(
                    ErrorCategory::ToolNonFunctional,
                    "Desktop Perception Process Spawn Failed",
                    "Failed to launch PowerShell window enumerator.",
                    Some(&e.to_string()),
                    "Ensure PowerShell is accessible.",
                )
                .to_plain_string()
            })?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut elements = Vec::new();
        let mut cur_tag = 1;

        for line in stdout.lines() {
            let parts: Vec<&str> = line.trim().split('|').collect();
            if parts.len() >= 7 {
                let title = parts[0].to_string();
                let process = parts[1].to_string();
                let class_name = parts[2].to_string();
                let x = parts[3].parse::<i32>().unwrap_or(0);
                let y = parts[4].parse::<i32>().unwrap_or(0);
                let width = parts[5].parse::<i32>().unwrap_or(0);
                let height = parts[6].parse::<i32>().unwrap_or(0);

                let center_x = x + width / 2;
                let center_y = y + height / 2;

                elements.push(TagElement {
                    tag_id: cur_tag,
                    title,
                    process,
                    class_name,
                    x,
                    y,
                    width,
                    height,
                    center_x,
                    center_y,
                });
                cur_tag += 1;
            }
        }

        // Cache into memory and cache file
        let cache_file = "cache/latest_tag_map.json";
        if let Some(parent) = Path::new(cache_file).parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(cache_file, serde_json::to_string_pretty(&elements).unwrap_or_default());

        if let Ok(mut lock) = LATEST_TAG_MAP.lock() {
            *lock = Some(elements.clone());
        }

        // Build human/LLM-friendly relational tree format
        let mut out = String::new();
        out.push_str("================ DESKTOP ACCESSIBILITY & WINDOW TREE ================\n");
        out.push_str("  [Tag_ID]  [Process]            [Bounding Box (x, y, w, h)]  [Title / Element]\n");
        out.push_str("----------------------------------------------------------------------\n");

        for elem in &elements {
            out.push_str(&format!(
                "  [{:>2}]      {:<18} ({:>4}, {:>4}, {:>4}, {:>4})    \"{}\" (center: [{}, {}])\n",
                elem.tag_id, elem.process, elem.x, elem.y, elem.width, elem.height, elem.title, elem.center_x, elem.center_y
            ));
        }

        out.push_str("======================================================================\n");
        out.push_str("To interact, call 'click_element' with {\"tag_id\": <num>} or 'type_element' with {\"tag_id\": <num>, \"text\": \"...\"}\n");

        Ok(out)
    }

    /// Clicks an element by its assigned Tag_ID from get_screen_state
    pub fn click_element(args: &Value) -> Result<String, String> {
        let tag_id = args.get("tag_id").and_then(|v| v.as_u64()).ok_or_else(|| {
            ErrorCheckpoint::new(
                ErrorCategory::InvalidParameters,
                "Missing 'tag_id' Parameter",
                "The tool 'click_element' requires integer 'tag_id'.",
                None,
                "Run 'get_screen_state' first, then pass target tag_id, e.g. {\"tag_id\": 2}",
            )
            .to_plain_string()
        })? as usize;

        let double = args.get("double").and_then(|v| v.as_bool()).unwrap_or(false);
        let right = args.get("right").and_then(|v| v.as_bool()).unwrap_or(false);

        // Find element in cache
        let elem = Self::find_tag(tag_id)?;

        let click_res = OsVlmTools::click_coordinates(&json!({
            "x": elem.center_x,
            "y": elem.center_y,
            "double": double,
            "right": right
        }))?;

        Ok(format!(
            "Clicked Tag [{}] \"{}\" ({}) at center ({}, {}): {}",
            tag_id, elem.title, elem.process, elem.center_x, elem.center_y, click_res
        ))
    }

    /// Focuses an element by Tag_ID and types text
    pub fn type_element(args: &Value) -> Result<String, String> {
        let tag_id = args.get("tag_id").and_then(|v| v.as_u64()).ok_or_else(|| {
            ErrorCheckpoint::new(
                ErrorCategory::InvalidParameters,
                "Missing 'tag_id' Parameter",
                "The tool 'type_element' requires integer 'tag_id'.",
                None,
                "Example: {\"tag_id\": 3, \"text\": \"hello\"}",
            )
            .to_plain_string()
        })? as usize;

        let text = args.get("text").and_then(|t| t.as_str()).ok_or_else(|| {
            ErrorCheckpoint::new(
                ErrorCategory::InvalidParameters,
                "Missing 'text' Parameter",
                "The tool 'type_element' requires 'text' to type.",
                None,
                "Example: {\"tag_id\": 3, \"text\": \"hello\"}",
            )
            .to_plain_string()
        })?;

        let press_enter = args.get("press_enter").and_then(|v| v.as_bool()).unwrap_or(false);

        let elem = Self::find_tag(tag_id)?;

        // Focus by clicking center
        let _ = OsVlmTools::click_coordinates(&json!({
            "x": elem.center_x,
            "y": elem.center_y,
        }));
        std::thread::sleep(std::time::Duration::from_millis(60));

        // Type text
        let type_res = OsVlmTools::type_text(&json!({
            "text": text,
            "press_enter": press_enter
        }))?;

        Ok(format!(
            "Focused Tag [{}] \"{}\" and typed '{}' (result: {})",
            tag_id, elem.title, text, type_res
        ))
    }

    /// Minimizes, maximizes, restores, or focuses a target window
    pub fn window_control(args: &Value) -> Result<String, String> {
        let action = args.get("action").and_then(|a| a.as_str()).unwrap_or("focus").to_lowercase();
        let title_query = args.get("title").or_else(|| args.get("window_name")).and_then(|t| t.as_str()).unwrap_or("");

        #[cfg(target_os = "windows")]
        {
            if title_query.is_empty()
                || title_query.eq_ignore_ascii_case("terminal")
                || title_query.eq_ignore_ascii_case("cli")
                || title_query.eq_ignore_ascii_case("jarvis")
                || title_query.eq_ignore_ascii_case("jaa")
                || title_query.eq_ignore_ascii_case("jarvis_assist_agent")
                || title_query.eq_ignore_ascii_case("console")
            {
                if action == "minimize" {
                    Self::minimize_terminal_window();
                    return Ok("Agent-CLI (terminal) window minimized.".to_string());
                } else if action == "restore" || action == "focus" || action == "maximize" {
                    Self::restore_terminal_window();
                    return Ok("Agent-CLI (terminal) window restored to foreground.".to_string());
                }
            }
        }

        let ps_cmd = match action.as_str() {
            "minimize" => format!(
                "$w = Get-Process | Where-Object {{ $_.MainWindowTitle -like '*{0}*' }} | Select-Object -First 1; \
                 if ($w) {{ (New-Object -ComObject WScript.Shell).AppActivate($w.Id); \
                 Add-Type @'
                 [System.Runtime.InteropServices.DllImport(\"user32.dll\")]
                 public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
'@; [Win32ShowWindow]::ShowWindow($w.MainWindowHandle, 6); Write-Output 'Minimized window.' }} else {{ Write-Output 'Window not found.' }}",
                title_query
            ),
            "maximize" => format!(
                "$w = Get-Process | Where-Object {{ $_.MainWindowTitle -like '*{0}*' }} | Select-Object -First 1; \
                 if ($w) {{ (New-Object -ComObject WScript.Shell).AppActivate($w.Id); \
                 Add-Type @'
                 [System.Runtime.InteropServices.DllImport(\"user32.dll\")]
                 public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
'@; [Win32ShowWindow]::ShowWindow($w.MainWindowHandle, 3); Write-Output 'Maximized window.' }} else {{ Write-Output 'Window not found.' }}",
                title_query
            ),
            "restore" => format!(
                "$w = Get-Process | Where-Object {{ $_.MainWindowTitle -like '*{0}*' }} | Select-Object -First 1; \
                 if ($w) {{ (New-Object -ComObject WScript.Shell).AppActivate($w.Id); \
                 Add-Type @'
                 [System.Runtime.InteropServices.DllImport(\"user32.dll\")]
                 public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
'@; [Win32ShowWindow]::ShowWindow($w.MainWindowHandle, 9); Write-Output 'Restored window.' }} else {{ Write-Output 'Window not found.' }}",
                title_query
            ),
            _ => format!(
                "$w = Get-Process | Where-Object {{ $_.MainWindowTitle -like '*{0}*' }} | Select-Object -First 1; \
                 if ($w) {{ (New-Object -ComObject WScript.Shell).AppActivate($w.Id); Write-Output 'Focused window.' }} else {{ Write-Output 'Window not found.' }}",
                title_query
            ),
        };

        let output = Command::new("powershell.exe")
            .args(["-NoProfile", "-Command", &ps_cmd])
            .output()
            .map_err(|e| format!("Window control failed: {}", e))?;

        let res = String::from_utf8_lossy(&output.stdout);
        Ok(res.trim().to_string())
    }

    fn find_tag(tag_id: usize) -> Result<TagElement, String> {
        // First check memory cache
        if let Ok(lock) = LATEST_TAG_MAP.lock() {
            if let Some(ref list) = *lock {
                if let Some(elem) = list.iter().find(|e| e.tag_id == tag_id) {
                    return Ok(elem.clone());
                }
            }
        }

        // Second check disk cache
        let cache_file = "cache/latest_tag_map.json";
        if Path::new(cache_file).exists() {
            if let Ok(content) = fs::read_to_string(cache_file) {
                if let Ok(list) = serde_json::from_str::<Vec<TagElement>>(&content) {
                    if let Some(elem) = list.iter().find(|e| e.tag_id == tag_id) {
                        return Ok(elem.clone());
                    }
                }
            }
        }

        // If not found, refresh screen state and re-search
        let _ = Self::get_screen_state(&json!({}));
        if let Ok(lock) = LATEST_TAG_MAP.lock() {
            if let Some(ref list) = *lock {
                if let Some(elem) = list.iter().find(|e| e.tag_id == tag_id) {
                    return Ok(elem.clone());
                }
            }
        }

        Err(ErrorCheckpoint::new(
            ErrorCategory::InvalidParameters,
            "Tag ID Not Found",
            &format!("Element with Tag [{}] was not found in desktop perception map.", tag_id),
            Some("Active windows or positions may have changed."),
            "Call 'get_screen_state' again to refresh active window tag IDs.",
        )
        .to_plain_string())
    }

    /// Minimizes the Agent-CLI (console/terminal) window via native Win32 FFI
    pub fn minimize_terminal_window() {
        #[cfg(windows)]
        {
            #[link(name = "kernel32")]
            unsafe extern "system" {
                fn GetConsoleWindow() -> usize;
            }
            #[link(name = "user32")]
            unsafe extern "system" {
                fn ShowWindow(hWnd: usize, nCmdShow: i32) -> i32;
            }

            let hwnd = unsafe { GetConsoleWindow() };
            if hwnd != 0 {
                unsafe {
                    ShowWindow(hwnd, 6); // 6 = SW_MINIMIZE
                }
            }
        }
    }

    /// Restores the Agent-CLI (console/terminal) window and brings it to foreground
    pub fn restore_terminal_window() {
        #[cfg(windows)]
        {
            #[link(name = "kernel32")]
            unsafe extern "system" {
                fn GetConsoleWindow() -> usize;
            }
            #[link(name = "user32")]
            unsafe extern "system" {
                fn ShowWindow(hWnd: usize, nCmdShow: i32) -> i32;
                fn SetForegroundWindow(hWnd: usize) -> i32;
            }

            let hwnd = unsafe { GetConsoleWindow() };
            if hwnd != 0 {
                unsafe {
                    ShowWindow(hwnd, 9); // 9 = SW_RESTORE
                    SetForegroundWindow(hwnd);
                }
            }
        }
    }
}
