use crate::checkpoint::{ErrorCategory, ErrorCheckpoint};
use crate::cli::ProgressSpinner;
use crate::engine::backend::{ChatMessage, StreamEvent};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

#[derive(Debug, Deserialize)]
struct PipeResponse {
    event: String,
    data: Option<String>,
    model: Option<String>,
    error: Option<String>,
    time_taken: Option<f32>,
}

pub struct PipeEngineClient {
    child: Child,
    stdin: BufWriter<ChildStdin>,
    lines: Lines<BufReader<ChildStdout>>,
    pub model_path: PathBuf,
    pub num_ctx: u64,
    pub stop_tokens: Vec<String>,
    pub vision_encoder_type: Option<String>,
}

impl PipeEngineClient {
    /// Locates the compiled jarvis-engine.exe binary
    pub fn locate_engine_binary() -> PathBuf {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .unwrap_or(PathBuf::from("."));

        // Check same directory as running binary (e.g. target/release or D:\JARVIS agent)
        let same_dir = exe_dir.join("jarvis-engine.exe");
        if same_dir.exists() {
            return same_dir;
        }

        // Check target/release
        let target_rel = PathBuf::from("target").join("release").join("jarvis-engine.exe");
        if target_rel.exists() {
            return target_rel;
        }

        // Check target/debug
        let target_deb = PathBuf::from("target").join("debug").join("jarvis-engine.exe");
        if target_deb.exists() {
            return target_deb;
        }

        PathBuf::from("jarvis-engine.exe")
    }

    /// Spawns jarvis-engine.exe as a dedicated pipe subprocess and waits for ready event
    pub async fn spawn(
        model_path: &Path,
        mmproj_path: Option<&Path>,
        num_ctx: u64,
        temp: f32,
        stop_tokens: Vec<String>,
        vision_encoder_type: Option<String>,
    ) -> Result<Self, String> {
        let engine_bin = Self::locate_engine_binary();

        let n_threads = std::thread::available_parallelism()
            .map(|p| p.get())
            .unwrap_or(8)
            .min(12);

        let spinner = ProgressSpinner::start(&format!(
            "Spawning jarvis-engine pipe subprocess for '{}' (zero network sockets, {} threads)...",
            model_path.file_name().and_then(|n| n.to_str()).unwrap_or("model"),
            n_threads
        ));

        let mut cmd = Command::new(&engine_bin);
        cmd.arg("--model")
            .arg(model_path.to_str().unwrap_or_default())
            .arg("--ctx")
            .arg(num_ctx.to_string())
            .arg("--temp")
            .arg(temp.to_string())
            .arg("--threads")
            .arg(n_threads.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        if let Some(mmproj) = mmproj_path {
            cmd.arg("--mmproj").arg(mmproj.to_str().unwrap_or_default());
        }

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                spinner.stop_with_error(&format!("Failed to spawn jarvis-engine: {}", e));
                return Err(ErrorCheckpoint::new(
                    ErrorCategory::ModelLoadFailed,
                    "Failed to Launch jarvis-engine Subprocess",
                    &format!("Cannot execute '{}'", engine_bin.display()),
                    Some(&e.to_string()),
                    "Ensure jarvis-engine.exe has been compiled via 'cargo build --release'.",
                ).to_plain_string());
            }
        };

        let child_stdin = child.stdin.take().ok_or("Failed to open child stdin pipe.")?;
        let child_stdout = child.stdout.take().ok_or("Failed to open child stdout pipe.")?;

        let writer = BufWriter::new(child_stdin);
        let reader = BufReader::new(child_stdout);
        let mut lines = reader.lines();

        // Wait up to 40 seconds for ready signal on stdout pipe
        let mut ready = false;
        let start = std::time::Instant::now();

        while start.elapsed() < Duration::from_secs(40) {
            match tokio::time::timeout(Duration::from_millis(500), lines.next_line()).await {
                Ok(Ok(Some(line))) => {
                    if let Ok(res) = serde_json::from_str::<PipeResponse>(&line) {
                        if res.event == "ready" {
                            ready = true;
                            break;
                        } else if res.event == "error" {
                            let err_msg = res.error.unwrap_or_else(|| "Unknown engine error".to_string());
                            spinner.stop_with_error(&err_msg);
                            let _ = child.kill().await;
                            return Err(format!("Engine reported error: {}", err_msg));
                        }
                    }
                }
                Ok(Ok(None)) => {
                    spinner.stop_with_error("Engine pipe closed prematurely (EOF)");
                    let _ = child.kill().await;
                    return Err("Engine subprocess pipe closed unexpectedly.".to_string());
                }
                Ok(Err(e)) => {
                    spinner.stop_with_error(&format!("Pipe read error: {}", e));
                    let _ = child.kill().await;
                    return Err(format!("Pipe communication failure: {}", e));
                }
                Err(_) => {
                    spinner.update(&format!(
                        "Locking model in GPU VRAM over pipe ({:.1}s)...",
                        start.elapsed().as_secs_f32()
                    ));
                }
            }
        }

        if !ready {
            spinner.stop_with_error("Subprocess timed out during GPU weight loading");
            let _ = child.kill().await;
            return Err("jarvis-engine startup timed out over pipe.".to_string());
        }

        spinner.stop_with_success(&format!(
            "jarvis-engine locked in memory via pipe (0 network sockets exposed)"
        ));

        Ok(Self {
            child,
            stdin: writer,
            lines,
            model_path: model_path.to_path_buf(),
            num_ctx,
            stop_tokens,
            vision_encoder_type,
        })
    }

    /// Sends a prompt and streams tokens over the pipe
    pub async fn stream_chat<F>(
        &mut self,
        messages: &[ChatMessage],
        tools_schema: Option<&[Value]>,
        temperature: f32,
        mut on_event: F,
    ) -> Result<(String, f32), String>
    where
        F: FnMut(StreamEvent),
    {
        let mut msg_values = Vec::new();
        let mut latest_image_path: Option<String> = None;

        for m in messages {
            let val = json!({
                "role": m.role,
                "content": m.content,
            });

            if let Some(ref imgs) = m.images {
                if !imgs.is_empty() {
                    if let Some(first_img) = imgs.first() {
                        latest_image_path = Some(first_img.clone());
                    }
                }
            }

            msg_values.push(val);
        }

        let max_turn_tokens = 512;
        let mut req_obj = json!({
            "cmd": "chat",
            "messages": msg_values,
            "temperature": temperature,
            "max_tokens": max_turn_tokens,
            "stop_tokens": self.stop_tokens,
        });

        if let Some(ref enc) = self.vision_encoder_type {
            req_obj["vision_encoder"] = json!(enc);
        }

        if let Some(ref img_p) = latest_image_path {
            if img_p.contains(";base64,") || img_p.len() > 500 {
                let clean_b64 = crate::engine::MultimodalInput::clean_base64(img_p);
                req_obj["image_name"] = json!("current_screen.png");
                req_obj["image_base64"] = json!(clean_b64);
            } else {
                req_obj["image_path"] = json!(img_p);
                req_obj["image_name"] = json!("current_screen.png");
            }
        }

        if let Some(tools) = tools_schema {
            if !tools.is_empty() {
                req_obj["tools"] = json!(tools);
            }
        }

        let serialized = serde_json::to_string(&req_obj).map_err(|e| e.to_string())?;

        // Write command to child stdin pipe
        self.stdin
            .write_all(serialized.as_bytes())
            .await
            .map_err(|e| format!("Failed to write to engine pipe: {}", e))?;
        self.stdin
            .write_all(b"\n")
            .await
            .map_err(|e| format!("Failed to flush newline to pipe: {}", e))?;
        self.stdin
            .flush()
            .await
            .map_err(|e| format!("Failed to flush pipe stdin: {}", e))?;

        // Read streaming lines from child stdout pipe
        let mut full_response = String::new();
        let mut net_time_taken: f32 = 0.0;

        loop {
            let line_opt = self.lines.next_line().await.map_err(|e| {
                format!("Error reading from engine stdout pipe: {}", e)
            })?;

            let line = match line_opt {
                Some(l) => l,
                None => {
                    let mut err_detail = String::new();
                    if let Ok(Some(status)) = self.child.try_wait() {
                        err_detail.push_str(&format!(" (child process terminated with status {})", status));
                    }
                    return Err(format!("Engine pipe closed while streaming tokens{}.", err_detail));
                }
            };

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            if let Ok(res) = serde_json::from_str::<PipeResponse>(trimmed) {
                match res.event.as_str() {
                    "thinking" => {
                        if let Some(ref thought) = res.data {
                            on_event(StreamEvent::Thinking(thought));
                        }
                    }
                    "token" => {
                        if let Some(ref token) = res.data {
                            full_response.push_str(token);
                            on_event(StreamEvent::Token(token));
                        }
                    }
                    "done" => {
                        if let Some(final_text) = res.data {
                            if full_response.is_empty() {
                                full_response = final_text;
                            }
                        }
                        if let Some(time) = res.time_taken {
                            net_time_taken = time;
                        }
                        break;
                    }
                    "error" => {
                        let err_msg = res.error.unwrap_or_else(|| "Unknown engine error".to_string());
                        return Err(format!("Engine error over pipe: {}", err_msg));
                    }
                    _ => {}
                }
            }
        }

        Ok((full_response, net_time_taken))
    }

    /// Primes and pre-warms the KV cache with the static system prompt + skills + tools prefix
    pub async fn prewarm_kv_cache(&mut self, system_prompt: &str) -> Result<(), String> {
        let req_obj = json!({
            "cmd": "warmup",
            "messages": [
                {
                    "role": "system",
                    "content": system_prompt
                }
            ]
        });

        let serialized = serde_json::to_string(&req_obj).map_err(|e| e.to_string())?;

        self.stdin
            .write_all(serialized.as_bytes())
            .await
            .map_err(|e| format!("Failed to write warmup to pipe: {}", e))?;
        self.stdin
            .write_all(b"\n")
            .await
            .map_err(|e| format!("Failed to flush newline: {}", e))?;
        self.stdin
            .flush()
            .await
            .map_err(|e| format!("Failed to flush stdin: {}", e))?;

        // Read until warmed or timeout
        let start = std::time::Instant::now();
        while start.elapsed() < Duration::from_secs(45) {
            match tokio::time::timeout(Duration::from_millis(500), self.lines.next_line()).await {
                Ok(Ok(Some(line))) => {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    if let Ok(res) = serde_json::from_str::<PipeResponse>(trimmed) {
                        if res.event == "warmed" || res.event == "ready" {
                            break;
                        }
                    }
                }
                Ok(Ok(None)) => break,
                Ok(Err(_)) => break,
                Err(_) => {}
            }
        }

        Ok(())
    }

    /// Stops the child process
    pub async fn stop(&mut self) {
        let _ = self.stdin.write_all(b"{\"cmd\":\"exit\"}\n").await;
        let _ = self.stdin.flush().await;
        tokio::time::sleep(Duration::from_millis(200)).await;
        let _ = self.child.kill().await;
    }
}

impl Drop for PipeEngineClient {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}
