use crate::checkpoint::{ErrorCategory, ErrorCheckpoint};
use serde_json::{json, Value};
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

#[cfg(target_os = "windows")]
#[repr(C)]
struct POINT {
    x: i32,
    y: i32,
}

#[cfg(target_os = "windows")]
unsafe extern "system" {
    fn SetCursorPos(x: i32, y: i32) -> i32;
    fn GetCursorPos(point: *mut POINT) -> i32;
    fn mouse_event(dwFlags: u32, dx: u32, dy: u32, dwData: u32, dwExtraInfo: usize);
    fn GetSystemMetrics(nIndex: i32) -> i32;
    fn ShowWindow(hWnd: usize, nCmdShow: i32) -> i32;
    fn GetForegroundWindow() -> usize;
}

#[cfg(target_os = "windows")]
const MOUSEEVENTF_LEFTDOWN: u32 = 0x0002;
#[cfg(target_os = "windows")]
const MOUSEEVENTF_LEFTUP: u32 = 0x0004;
#[cfg(target_os = "windows")]
const MOUSEEVENTF_RIGHTDOWN: u32 = 0x0008;
#[cfg(target_os = "windows")]
const MOUSEEVENTF_RIGHTUP: u32 = 0x0010;
#[cfg(target_os = "windows")]
const SM_CXSCREEN: i32 = 0;
#[cfg(target_os = "windows")]
const SM_CYSCREEN: i32 = 1;

pub struct OsVlmTools;

impl OsVlmTools {
    /// Captures full desktop screenshot to output path (defaulting to cache/current_screen.png)
    pub fn capture_screen(args: &Value) -> Result<String, String> {
        let default_cache = "cache/current_screen.png";
        let output_path = args
            .get("output_path")
            .or_else(|| args.get("output"))
            .and_then(|o| o.as_str())
            .unwrap_or(default_cache);

        let path = Path::new(output_path);
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }

        // Get primary display resolution
        let (width, height) = Self::get_resolution();

        // Native PowerShell GDI+ primary screen buffer capture
        let ps_cmd = format!(
            "Add-Type -AssemblyName System.Windows.Forms; Add-Type -AssemblyName System.Drawing; \
             $bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds; \
             $bmp = New-Object System.Drawing.Bitmap $bounds.Width, $bounds.Height; \
             $graphics = [System.Drawing.Graphics]::FromImage($bmp); \
             $graphics.CopyFromScreen($bounds.Location, [System.Drawing.Point]::Empty, $bounds.Size); \
             $bmp.Save('{}', [System.Drawing.Imaging.ImageFormat]::Png); \
             $graphics.Dispose(); $bmp.Dispose();",
            path.display().to_string().replace('\\', "/")
        );

        let output = Command::new("powershell.exe")
            .args(["-NoProfile", "-Command", &ps_cmd])
            .output()
            .map_err(|e| {
                ErrorCheckpoint::new(
                    ErrorCategory::ToolNonFunctional,
                    "Screen Capture Process Failed",
                    "Failed to launch PowerShell screenshot process.",
                    Some(&e.to_string()),
                    "Ensure powershell.exe is accessible in PATH.",
                )
                .to_plain_string()
            })?;

        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr);
            return Err(ErrorCheckpoint::new(
                ErrorCategory::ToolNonFunctional,
                "Screen Capture Graphics Error",
                "Desktop screen buffer grab failed.",
                Some(&err),
                "Verify active desktop display session.",
            )
            .to_plain_string());
        }

        let abs_path = path.canonicalize().unwrap_or(path.to_path_buf());
        let res = json!({
            "status": "success",
            "width": width,
            "height": height,
            "file_path": abs_path.display().to_string(),
            "message": format!("Screenshot captured ({}x{}) to '{}'", width, height, abs_path.display())
        });

        Ok(serde_json::to_string_pretty(&res).unwrap_or_default())
    }

    /// Converts normalized coordinates [0.0, 1.0] where (0,0) is bottom-left and (1,1) is top-right to Windows screen pixels.
    fn normalize_to_pixel(x: f64, y: f64) -> (i32, i32) {
        #[cfg(target_os = "windows")]
        unsafe {
            let mut sw = GetSystemMetrics(SM_CXSCREEN);
            let mut sh = GetSystemMetrics(SM_CYSCREEN);
            if sw <= 0 { sw = 1920; }
            if sh <= 0 { sh = 1080; }

            if x >= 0.0 && x <= 1.0 && y >= 0.0 && y <= 1.0 {
                let px = (x * (sw - 1) as f64).round() as i32;
                let py = ((1.0 - y) * (sh - 1) as f64).round() as i32;
                (px.clamp(0, sw - 1), py.clamp(0, sh - 1))
            } else {
                (x.round() as i32, y.round() as i32)
            }
        }
        #[cfg(not(target_os = "windows"))]
        {
            (x.round() as i32, y.round() as i32)
        }
    }

    /// Clicks on normalized coordinates (x, y) with optional double or right click
    pub fn click_coordinates(args: &Value) -> Result<String, String> {
        let x = args.get("x").and_then(|v| v.as_f64().or_else(|| v.as_i64().map(|i| i as f64))).ok_or_else(|| {
            ErrorCheckpoint::new(
                ErrorCategory::InvalidParameters,
                "Missing 'x' Coordinate",
                "The tool 'click_coordinates' requires a number 'x' parameter.",
                None,
                "Provide normalized 'x' coordinate (0.0 to 1.0), e.g. {\"x\": 0.5, \"y\": 0.5}",
            )
            .to_plain_string()
        })?;

        let y = args.get("y").and_then(|v| v.as_f64().or_else(|| v.as_i64().map(|i| i as f64))).ok_or_else(|| {
            ErrorCheckpoint::new(
                ErrorCategory::InvalidParameters,
                "Missing 'y' Coordinate",
                "The tool 'click_coordinates' requires a number 'y' parameter.",
                None,
                "Provide normalized 'y' coordinate (0.0 to 1.0), e.g. {\"x\": 0.5, \"y\": 0.5}",
            )
            .to_plain_string()
        })?;

        let double = args.get("double").and_then(|v| v.as_bool()).unwrap_or(false);
        let right = args.get("right").and_then(|v| v.as_bool()).unwrap_or(false);

        let (pixel_x, pixel_y) = Self::normalize_to_pixel(x, y);

        #[cfg(target_os = "windows")]
        unsafe {
            SetCursorPos(pixel_x, pixel_y);
            std::thread::sleep(Duration::from_millis(40));

            if right {
                mouse_event(MOUSEEVENTF_RIGHTDOWN, 0, 0, 0, 0);
                std::thread::sleep(Duration::from_millis(30));
                mouse_event(MOUSEEVENTF_RIGHTUP, 0, 0, 0, 0);
            } else if double {
                mouse_event(MOUSEEVENTF_LEFTDOWN, 0, 0, 0, 0);
                std::thread::sleep(Duration::from_millis(30));
                mouse_event(MOUSEEVENTF_LEFTUP, 0, 0, 0, 0);
                std::thread::sleep(Duration::from_millis(60));
                mouse_event(MOUSEEVENTF_LEFTDOWN, 0, 0, 0, 0);
                std::thread::sleep(Duration::from_millis(30));
                mouse_event(MOUSEEVENTF_LEFTUP, 0, 0, 0, 0);
            } else {
                mouse_event(MOUSEEVENTF_LEFTDOWN, 0, 0, 0, 0);
                std::thread::sleep(Duration::from_millis(30));
                mouse_event(MOUSEEVENTF_LEFTUP, 0, 0, 0, 0);
            }
        }

        let action = if right { "Right-clicked" } else if double { "Double-clicked" } else { "Clicked" };
        Ok(format!("{} at normalized coordinates ({:.3}, {:.3}) [pixel ({}, {})]", action, x, y, pixel_x, pixel_y))
    }

    /// Types text into active focus, optionally clicking normalized coordinates (x, y) first
    pub fn type_text(args: &Value) -> Result<String, String> {
        let text = args.get("text").and_then(|t| t.as_str()).ok_or_else(|| {
            ErrorCheckpoint::new(
                ErrorCategory::InvalidParameters,
                "Missing 'text' Parameter",
                "The tool 'type_text' requires a string 'text' to type.",
                None,
                "Provide text argument, e.g. {\"text\": \"hello world\"}",
            )
            .to_plain_string()
        })?;

        let press_enter = args.get("press_enter").and_then(|v| v.as_bool()).unwrap_or(false);

        // Click coordinates first if specified
        let x_opt = args.get("x").and_then(|v| v.as_f64().or_else(|| v.as_i64().map(|i| i as f64)));
        let y_opt = args.get("y").and_then(|v| v.as_f64().or_else(|| v.as_i64().map(|i| i as f64)));
        if let (Some(x_val), Some(y_val)) = (x_opt, y_opt) {
            let _ = Self::click_coordinates(&json!({ "x": x_val, "y": y_val }));
            std::thread::sleep(Duration::from_millis(80));
        }

        // PowerShell SendKeys with SendWait handles standard text and hotkeys
        let escaped = text
            .replace('\'', "''")
            .replace('+', "{+}")
            .replace('^', "{^}")
            .replace('%', "{%}")
            .replace('~', "{~}")
            .replace('(', "{(}")
            .replace(')', "{)}")
            .replace('{', "{{}")
            .replace('}', "{}}");

        let enter_suffix = if press_enter { "; [System.Windows.Forms.SendKeys]::SendWait('{ENTER}')" } else { "" };
        let ps_cmd = format!(
            "Add-Type -AssemblyName System.Windows.Forms; [System.Windows.Forms.SendKeys]::SendWait('{}'){}",
            escaped, enter_suffix
        );

        let _ = Command::new("powershell.exe")
            .args(["-NoProfile", "-Command", &ps_cmd])
            .output();

        Ok(format!("Typed '{}' (press_enter: {})", text, press_enter))
    }

    /// Drags mouse from normalized (from_x, from_y) to (to_x, to_y) over specified duration
    pub fn drag_mouse(args: &Value) -> Result<String, String> {
        let from_x = args.get("from_x").and_then(|v| v.as_f64().or_else(|| v.as_i64().map(|i| i as f64))).unwrap_or(0.0);
        let from_y = args.get("from_y").and_then(|v| v.as_f64().or_else(|| v.as_i64().map(|i| i as f64))).unwrap_or(0.0);
        let to_x = args.get("to_x").and_then(|v| v.as_f64().or_else(|| v.as_i64().map(|i| i as f64))).unwrap_or(0.0);
        let to_y = args.get("to_y").and_then(|v| v.as_f64().or_else(|| v.as_i64().map(|i| i as f64))).unwrap_or(0.0);
        let duration = args.get("duration").and_then(|v| v.as_f64()).unwrap_or(0.3) as f32;

        let (pixel_from_x, pixel_from_y) = Self::normalize_to_pixel(from_x, from_y);
        let (pixel_to_x, pixel_to_y) = Self::normalize_to_pixel(to_x, to_y);

        #[cfg(target_os = "windows")]
        unsafe {
            SetCursorPos(pixel_from_x, pixel_from_y);
            std::thread::sleep(Duration::from_millis(50));
            mouse_event(MOUSEEVENTF_LEFTDOWN, 0, 0, 0, 0);
            std::thread::sleep(Duration::from_millis(30));

            let steps = ((duration * 60.0).max(10.0)) as i32;
            let step_delay = Duration::from_millis(((duration * 1000.0) / steps as f32).max(5.0) as u64);

            for i in 1..=steps {
                let t = i as f32 / steps as f32;
                let cur_x = pixel_from_x as f32 + (pixel_to_x - pixel_from_x) as f32 * t;
                let cur_y = pixel_from_y as f32 + (pixel_to_y - pixel_from_y) as f32 * t;
                SetCursorPos(cur_x as i32, cur_y as i32);
                std::thread::sleep(step_delay);
            }

            std::thread::sleep(Duration::from_millis(30));
            mouse_event(MOUSEEVENTF_LEFTUP, 0, 0, 0, 0);
        }

        Ok(format!("Dragged mouse from normalized ({:.3}, {:.3}) to ({:.3}, {:.3}) in {:.2}s", from_x, from_y, to_x, to_y, duration))
    }

    /// Sends hotkey combinations, e.g. ["ctrl", "s"], ["alt", "tab"], ["enter"]
    pub fn send_hotkey(args: &Value) -> Result<String, String> {
        let keys = args.get("keys").and_then(|k| k.as_array()).ok_or_else(|| {
            ErrorCheckpoint::new(
                ErrorCategory::InvalidParameters,
                "Missing 'keys' Array",
                "The tool 'send_hotkey' requires an array of key strings.",
                None,
                "Example: {\"keys\": [\"ctrl\", \"s\"]}",
            )
            .to_plain_string()
        })?;

        let key_strings: Vec<String> = keys
            .iter()
            .filter_map(|k| k.as_str())
            .map(|s| s.to_lowercase())
            .collect();

        if key_strings.is_empty() {
            return Err("No keys specified in hotkey array.".to_string());
        }

        // Construct SendKeys syntax: ctrl = ^, shift = +, alt = %
        let mut prefix = String::new();
        let mut final_key = String::new();

        for k in &key_strings {
            match k.as_str() {
                "ctrl" | "control" => prefix.push('^'),
                "shift" => prefix.push('+'),
                "alt" => prefix.push('%'),
                "enter" | "return" => final_key = "{ENTER}".to_string(),
                "esc" | "escape" => final_key = "{ESC}".to_string(),
                "tab" => final_key = "{TAB}".to_string(),
                "space" => final_key = " ".to_string(),
                "backspace" => final_key = "{BACKSPACE}".to_string(),
                "delete" => final_key = "{DELETE}".to_string(),
                "up" => final_key = "{UP}".to_string(),
                "down" => final_key = "{DOWN}".to_string(),
                "left" => final_key = "{LEFT}".to_string(),
                "right" => final_key = "{RIGHT}".to_string(),
                other => final_key = other.to_string(),
            }
        }

        let sendkeys_str = format!("{}{}", prefix, final_key);
        let ps_cmd = format!(
            "Add-Type -AssemblyName System.Windows.Forms; [System.Windows.Forms.SendKeys]::SendWait('{}')",
            sendkeys_str
        );

        let _ = Command::new("powershell.exe")
            .args(["-NoProfile", "-Command", &ps_cmd])
            .output();

        Ok(format!("Sent hotkey combination: [{}]", key_strings.join("+")))
    }

    /// Erases a rectangular region [x1, y1, x2, y2] using mouse drag brush rubbing
    pub fn erase_region(args: &Value) -> Result<String, String> {
        let x1 = args.get("x1").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
        let y1 = args.get("y1").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
        let x2 = args.get("x2").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
        let y2 = args.get("y2").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
        let step = args.get("step").and_then(|v| v.as_i64()).unwrap_or(6) as i32;

        let top = y1.min(y2);
        let bottom = y1.max(y2);
        let left = x1.min(x2);
        let right = x1.max(x2);

        #[cfg(target_os = "windows")]
        unsafe {
            SetCursorPos(left, top);
            std::thread::sleep(Duration::from_millis(50));
            mouse_event(MOUSEEVENTF_LEFTDOWN, 0, 0, 0, 0);

            let mut cur_y = top;
            let mut going_right = true;

            while cur_y <= bottom {
                let target_x = if going_right { right } else { left };
                SetCursorPos(target_x, cur_y);
                std::thread::sleep(Duration::from_millis(15));
                going_right = !going_right;
                cur_y += step;
            }

            std::thread::sleep(Duration::from_millis(30));
            mouse_event(MOUSEEVENTF_LEFTUP, 0, 0, 0, 0);
        }

        Ok(format!("Erased region [{}, {}, {}, {}] with step {}", left, top, right, bottom, step))
    }

    /// Draws freehand brush text on canvas at (start_x, start_y)
    pub fn draw_text(args: &Value) -> Result<String, String> {
        let text = args.get("text").and_then(|t| t.as_str()).unwrap_or("J.A.A");
        let start_x = args.get("start_x").and_then(|v| v.as_i64()).unwrap_or(300) as i32;
        let start_y = args.get("start_y").and_then(|v| v.as_i64()).unwrap_or(300) as i32;

        #[cfg(target_os = "windows")]
        unsafe {
            let char_width = 25;
            let char_height = 40;
            let mut cursor_x = start_x;

            for ch in text.chars() {
                if ch.is_whitespace() {
                    cursor_x += 18;
                    continue;
                }

                // Draw a simple brush stroke glyph for the character
                SetCursorPos(cursor_x, start_y);
                std::thread::sleep(Duration::from_millis(30));
                mouse_event(MOUSEEVENTF_LEFTDOWN, 0, 0, 0, 0);
                std::thread::sleep(Duration::from_millis(15));

                SetCursorPos(cursor_x, start_y + char_height);
                std::thread::sleep(Duration::from_millis(30));
                SetCursorPos(cursor_x + char_width, start_y + char_height);
                std::thread::sleep(Duration::from_millis(30));
                SetCursorPos(cursor_x + char_width, start_y);
                std::thread::sleep(Duration::from_millis(30));

                mouse_event(MOUSEEVENTF_LEFTUP, 0, 0, 0, 0);
                std::thread::sleep(Duration::from_millis(40));

                cursor_x += char_width + 8;
            }
        }

        Ok(format!("Rendered brush text '{}' starting at ({}, {})", text, start_x, start_y))
    }

    /// Returns primary screen resolution (width, height)
    pub fn get_resolution() -> (i32, i32) {
        #[cfg(target_os = "windows")]
        unsafe {
            let w = GetSystemMetrics(SM_CXSCREEN);
            let h = GetSystemMetrics(SM_CYSCREEN);
            if w > 0 && h > 0 {
                return (w, h);
            }
        }
        (1920, 1080)
    }
}
