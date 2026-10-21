use crate::agent::state::AgentState;
use crate::checkpoint::{ErrorCategory, ErrorCheckpoint};
use crate::engine::ChatMessage;
use crate::tools::executor::{AgentMode, ToolAction, ToolExecutor};
use colored::*;
use std::io::{stdout, Write};

pub struct AgentLoop;

impl AgentLoop {
    pub async fn process_user_input(
        state: &mut AgentState,
        user_input: &str,
    ) -> Result<(), String> {
        let max_steps = state.settings.max_agent_steps;
        state.step_count = 0;

        let session_logger = crate::agent::TaskSessionLogger::init(user_input);

        // Parse @ directives (e.g. @os_navigation_vlm, @skill:metadata, @tool:capture_screen)
        let directives = crate::agent::PromptDirectives::parse_and_apply(
            user_input,
            &mut state.skill_manager,
            &mut state.tool_registry,
            &state.mcp_manager,
        );

        if state.static_system_prompt.is_empty() {
            state.refresh_static_prompt();
        }

        let directive_prompt = directives.build_directive_prompt();

        // Prepare user chat message (supporting independent image upload with default inspection prompt)
        let prompt_text = if !directives.clean_prompt.is_empty() {
            &directives.clean_prompt
        } else {
            user_input
        };

        let effective_prompt = if prompt_text.trim().is_empty() && state.pending_image.is_some() {
            "Describe and analyze this image in detail.".to_string()
        } else {
            prompt_text.to_string()
        };

        let user_content = if !directive_prompt.is_empty() {
            format!("{}\n\n{}", directive_prompt.trim(), effective_prompt)
        } else {
            effective_prompt
        };

        let mut initial_user_msg = ChatMessage {
            role: "user".to_string(),
            content: user_content,
            images: None,
        };

        let has_attached_image = state.pending_image.is_some();

        if let Some(ref img) = state.pending_image {
            initial_user_msg.images = Some(vec![img.to_data_url()]);
            println!(
                "{}",
                format!("📷 Attached image: {}", img.image_path.display()).cyan()
            );
            state.pending_image = None;
        } else {
            // Live Desktop Autonomous Perception: Capture current desktop frame directly to RAM (0 disk I/O, zero SSD wear)
            if let Ok((_name, b64)) = crate::tools::os_bridge_client::OsBridgeClient::capture_screen_ram() {
                println!("{}", "📷 Live Desktop In-RAM Perception captured (1920x1080 native)".cyan());
                initial_user_msg.images = Some(vec![b64]);
            }
        }

        // Dynamic Step (N-1) -> Step N Feedback Loop tracking
        let mut prev_assistant_reply: Option<String> = None;
        let mut prev_feedback_observation: Option<ChatMessage> = None;
        let mut last_generated_text: String = String::new();

        // Single-Slot In-Place RAM Frame Buffer (Zero disk writes, zero SSD wear)
        let mut active_ram_frame_b64: Option<String> = initial_user_msg.images.take().and_then(|mut imgs| imgs.pop());

        // Loop Repetition Circuit Breaker tracking (SHA-256 action signature)
        let mut last_action_signature: Option<String> = None;
        let mut action_repeat_count: usize = 0;

        // Track whether Agent-CLI was minimized by LLM Mode: task
        let mut terminal_minimized = false;

        // Multi-turn autonomous tool execution loop
        while state.step_count < max_steps {
            state.step_count += 1;

            // Build request messages for Step N:
            // 1. Static System Prompt (pre-cached static in engine VRAM prefix)
            // 2. Initial User Prompt (anchor goal)
            // 3. For Step N >= 2: Step (N-1) model response + dynamic Step (N-1) -> Step N Feedback Loop
            let sys_prompt = if has_attached_image {
                r#"You are J.A.R.V.I.S., an autonomous AI desktop assistant capable of multimodal perception and Windows desktop operation.
When an image or screenshot is provided:
1. Carefully observe the desktop, applications, and taskbar icons.
2. Directly and concisely fulfill the user's request.
3. If an action or tool is needed, use the standard format (mode: task / tool: <name>). Otherwise reply in chat."#.to_string()
            } else {
                state.static_system_prompt.clone()
            };

            let mut request_messages = vec![ChatMessage {
                role: "system".to_string(),
                content: sys_prompt,
                images: None,
            }];

            request_messages.push(initial_user_msg.clone());

            if state.step_count > 1 {
                if let Some(ref reply) = prev_assistant_reply {
                    request_messages.push(ChatMessage {
                        role: "assistant".to_string(),
                        content: reply.clone(),
                        images: None,
                    });
                }
                if let Some(ref obs) = prev_feedback_observation {
                    request_messages.push(obs.clone());
                }
            }

            // Attach active in-RAM frame ONLY to the latest message being evaluated (Zero history pipe duplication)
            if let Some(last_msg) = request_messages.last_mut() {
                if let Some(ref b64) = active_ram_frame_b64 {
                    last_msg.images = Some(vec![b64.clone()]);
                }
            }

            session_logger.log_step_start(state.step_count, &request_messages);

            // Maintain temporary loop input cache snapshot for step awareness and testing
            let input_snapshot = serde_json::json!({
                "step": state.step_count,
                "mode": if state.step_count == 1 { "chat" } else { "task" },
                "initial_user_prompt": initial_user_msg.content,
                "previous_assistant_reply": prev_assistant_reply,
                "previous_feedback_observation": prev_feedback_observation.as_ref().map(|o| &o.content),
                "messages_count": request_messages.len(),
                "messages_fed_to_llm": request_messages.iter().map(|m| {
                    serde_json::json!({
                        "role": m.role,
                        "content_length": m.content.len(),
                        "preview": m.content.chars().take(200).collect::<String>()
                    })
                }).collect::<Vec<_>>()
            });
            let _ = std::fs::create_dir_all("cache");
            let _ = std::fs::write("cache/loop_step_input.json", serde_json::to_string_pretty(&input_snapshot).unwrap_or_default());

            // Progress status while reasoning / computing prompt
            let thinking_msg = if state.step_count == 1 {
                if has_attached_image {
                    "Ingesting visual frames & evaluating prompt...".to_string()
                } else {
                    "Thinking & formulating response...".to_string()
                }
            } else {
                "Evaluating tool results & synthesizing next response...".to_string()
            };

            let spinner = std::sync::Arc::new(std::sync::Mutex::new(Some(
                crate::cli::ProgressSpinner::start(&thinking_msg),
            )));
            let sp_clone = std::sync::Arc::clone(&spinner);
            let filter = std::sync::Arc::new(std::sync::Mutex::new(StreamChatFilter::new()));
            let filter_clone = std::sync::Arc::clone(&filter);

            let rolling_reasoning = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
            let rr_clone = std::sync::Arc::clone(&rolling_reasoning);

            let response_result = state
                .engine
                .stream_chat(&request_messages, None, |event| {
                    match event {
                        crate::engine::StreamEvent::Thinking(thought) => {
                            if let Ok(mut text_guard) = rr_clone.lock() {
                                text_guard.push_str(thought);
                                let clean: String = text_guard
                                    .chars()
                                    .filter(|c| !c.is_control() && *c != '\n' && *c != '\r')
                                    .collect();
                                let last_chunk: String = clean
                                    .chars()
                                    .rev()
                                    .take(45)
                                    .collect::<Vec<_>>()
                                    .into_iter()
                                    .rev()
                                    .collect();
                                let clean_trimmed = last_chunk.trim();
                                if !clean_trimmed.is_empty() {
                                    if let Ok(guard) = sp_clone.lock() {
                                        if let Some(ref sp) = *guard {
                                            sp.update(&format!("Reasoning: {}...", clean_trimmed));
                                        }
                                    }
                                }
                            }
                        }
                        crate::engine::StreamEvent::Token(token) => {
                            if token.is_empty() {
                                return;
                            }
                            if let Ok(mut filt) = filter_clone.lock() {
                                filt.push_token(token, &sp_clone);
                            }
                        }
                    }
                })
                .await;

            // Ensure spinner is cleared if stream errored or finished without tokens
            if let Ok(mut lock) = spinner.lock() {
                if let Some(sp) = lock.take() {
                    sp.stop_and_clear();
                }
            }

            let (generated_text, time_taken) = match response_result {
                Ok((text, time)) => (text, time),
                Err(err) => {
                    session_logger.log_session_end(&format!("Stream error: {}", err), state.step_count);
                    if terminal_minimized {
                        crate::tools::os_metadata::OsMetadataTools::restore_terminal_window();
                    }
                    return Err(err);
                }
            };

            // Parse structured model output (Chat + Tool action)
            let parsed = ToolExecutor::parse_model_output(&generated_text);

            // Finalize chat streaming output
            if let Ok(mut filt) = filter.lock() {
                filt.finish(&parsed.chat);
            }

            // Display net inference time taken
            if time_taken > 0.0 {
                println!("{}", format!("⏱ Net time taken: {:.2}s", time_taken).dimmed());
            }

            session_logger.log_model_output(
                state.step_count,
                &generated_text,
                &parsed.chat,
                parsed.mode,
                &parsed.action,
                time_taken,
            );

            // Track assistant response for Step (N-1) context
            last_generated_text = generated_text.clone();

            // Update cache snapshot with resolved mode
            let _ = std::fs::write(
                "cache/loop_step_input.json",
                serde_json::to_string_pretty(&serde_json::json!({
                    "step": state.step_count,
                    "mode": match parsed.mode { AgentMode::Chat => "chat", AgentMode::Task => "task" },
                    "raw_model_output": generated_text,
                    "parsed_action": format!("{:?}", parsed.action),
                    "initial_user_prompt": initial_user_msg.content,
                    "previous_assistant_reply": prev_assistant_reply,
                    "previous_feedback_observation": prev_feedback_observation.as_ref().map(|o| &o.content),
                    "messages_count": request_messages.len(),
                    "messages_fed_to_llm": request_messages.iter().map(|m| {
                        serde_json::json!({
                            "role": m.role,
                            "content_length": m.content.len(),
                            "preview": m.content.chars().take(200).collect::<String>()
                        })
                    }).collect::<Vec<_>>()
                })).unwrap_or_default(),
            );

            // =========================================================================
            // AGENT-LEVEL GATEWAY: CHAT MODE vs. TASK MODE ARBITRATION
            // =========================================================================

            // Case 1: Chat Mode (Single Turn Only)
            // Terminal minimized: ❌ No. Step awareness: ❌ None.
            if parsed.mode == AgentMode::Chat || matches!(parsed.action, ToolAction::None) {
                session_logger.log_session_end("Concluded (Chat Mode single turn completed)", state.step_count);
                if terminal_minimized {
                    crate::tools::os_metadata::OsMetadataTools::restore_terminal_window();
                    terminal_minimized = false;
                }
                break; // Conclude immediately, return to >>>
            }

            // Case 2: Explicit Task Completion
            if matches!(parsed.action, ToolAction::TaskCompleted) {
                session_logger.log_session_end("Concluded (TaskCompleted explicitly signaled)", state.step_count);
                if terminal_minimized {
                    crate::tools::os_metadata::OsMetadataTools::restore_terminal_window();
                    terminal_minimized = false;
                }
                break; // Task complete, return to >>>
            }

            // Case 3: Task Mode Action Execution
            if let ToolAction::Execute(tool_call) = parsed.action {
                // Minimize terminal so target desktop/application is unobstructed
                if !terminal_minimized {
                    crate::tools::os_metadata::OsMetadataTools::minimize_terminal_window();
                    terminal_minimized = true;
                    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
                }

                // =========================================================================
                // LOOP REPETITION CIRCUIT BREAKER (Action Signature Tripwire)
                // =========================================================================
                let current_action_sig = format!("{}:{}", tool_call.tool_name, tool_call.arguments);
                if let Some(ref prev_sig) = last_action_signature {
                    if *prev_sig == current_action_sig {
                        action_repeat_count += 1;
                    } else {
                        action_repeat_count = 1;
                        last_action_signature = Some(current_action_sig.clone());
                    }
                } else {
                    action_repeat_count = 1;
                    last_action_signature = Some(current_action_sig.clone());
                }

                let completed_step = state.step_count;
                let current_step = state.step_count + 1;

                // HARD INTERCEPT at Repeat Count >= 3 (Halts infinite action loop deadlock)
                if action_repeat_count >= 3 {
                    session_logger.log_session_end("Terminated by Circuit Breaker (3 duplicate actions)", state.step_count);
                    println!(
                        "{}",
                        format!(
                            "🛑 [CIRCUIT BREAKER] Intercepted duplicate action '{}' after 3 identical attempts. Concluding task.",
                            tool_call.tool_name
                        )
                        .yellow()
                        .bold()
                    );
                    if terminal_minimized {
                        crate::tools::os_metadata::OsMetadataTools::restore_terminal_window();
                        terminal_minimized = false;
                    }
                    break;
                }

                println!(
                    "{}",
                    format!(
                        "⚡ [TOOL CALL] {} ({})",
                        tool_call.tool_name.bold(),
                        tool_call.arguments
                    )
                    .bright_blue()
                );

                let tool_spinner = crate::cli::ProgressSpinner::start(&format!(
                    "Executing tool '{}'...",
                    tool_call.tool_name
                ));

                // Execute tool
                let exec_result = ToolExecutor::execute(
                    &state.tool_registry,
                    &state.mcp_manager,
                    &tool_call,
                )
                .await;

                let (tool_output_str, is_err) = match exec_result {
                    Ok(out) => {
                        tool_spinner.stop_with_success(&format!(
                            "Tool '{}' execution completed",
                            tool_call.tool_name
                        ));
                        (out, false)
                    }
                    Err(err) => {
                        tool_spinner.stop_with_error(&format!(
                            "Tool '{}' execution failed",
                            tool_call.tool_name
                        ));
                        (err, true)
                    }
                };

                if is_err {
                    if tool_output_str.contains("[CHECKPOINT:") {
                        println!("{}", tool_output_str.red());
                    } else {
                        println!("{}", format!("  ↳ [FAILED] {}", tool_output_str).red());
                    }
                } else {
                    let preview: String = tool_output_str
                        .lines()
                        .take(6)
                        .collect::<Vec<&str>>()
                        .join("\n");
                    println!("{}", format!("  ↳ [RESULT]\n{}", preview).dimmed());
                    if tool_output_str.lines().count() > 6 {
                        println!("{}", "  ↳ ... (output truncated in preview)".dimmed());
                    }
                }

                let is_action_tool = matches!(
                    tool_call.tool_name.as_str(),
                    "click_coordinates" | "click_element" | "type_text" | "type_element" | "drag_mouse"
                );

                // Perception loop: Direct-to-RAM screen capture (Zero disk I/O, zero SSD wear)
                if !is_err && (is_action_tool || tool_call.tool_name == "capture_screen" || tool_call.tool_name == "get_screen_state") {
                    if is_action_tool {
                        // 1st wait: 2.0s UI settling phase
                        tokio::time::sleep(tokio::time::Duration::from_millis(2000)).await;
                    }

                    match crate::tools::os_bridge_client::OsBridgeClient::capture_screen_ram() {
                        Ok((name, b64)) => {
                            println!("{}", format!("  📷 [In-RAM Capture] Screen frame overwritten in RAM as '{}'", name).cyan());
                            // In-place overwrite: destructive replacement of previous frame in RAM (zero stale references, zero disk I/O)
                            active_ram_frame_b64 = Some(b64);
                        }
                        Err(e) => {
                            eprintln!("  ⚠️ In-RAM screen capture failed: {}", e);
                        }
                    }
                }

                session_logger.log_tool_result(
                    state.step_count,
                    &tool_call.tool_name,
                    &tool_call.arguments.to_string(),
                    is_err,
                    &tool_output_str,
                    active_ram_frame_b64.is_some(),
                );

                // Construct clean Step (N-1) -> Step (N) observation without heuristic status text
                let mut payload = format!("Tool Executed: {}({})", tool_call.tool_name, tool_call.arguments);
                if active_ram_frame_b64.is_some() {
                    payload.push_str("\n[Attached: current_screen.png (Freshly overwritten in RAM)]");
                }
                if action_repeat_count == 2 {
                    payload.push_str(&format!(
                        "\n⚠️ [CIRCUIT BREAKER WARNING]: You executed this identical action in Step ({}). If this action was an application launch (e.g. clicking Chrome or an app icon), it was already dispatched by Windows; emit 'tool: task_completed' immediately. Do NOT repeat this action.",
                        completed_step
                    ));
                }
                if is_err {
                    payload.push_str(&format!("\nError: {}", tool_output_str.trim()));
                } else if tool_call.tool_name == "get_screen_state" || tool_call.tool_name == "capture_screen" {
                    let clean_metadata = if tool_output_str.contains("\"base64\"") {
                        if let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&tool_output_str) {
                            if let Some(obj) = v.as_object_mut() {
                                obj.remove("base64");
                            }
                            serde_json::to_string_pretty(&v).unwrap_or_else(|_| tool_output_str.clone())
                        } else {
                            tool_output_str.clone()
                        }
                    } else {
                        tool_output_str.clone()
                    };
                    payload.push_str(&format!("\nMetadata:\n{}", clean_metadata.trim()));
                }

                let observation = format!(
                    "==================== Results of previous Step ({}) ====================\n{}\n========================================================================\n\n==================== Verify and Plan current step ({}) ====================\nUser request: {}\n=====================================================================",
                    completed_step,
                    payload,
                    current_step,
                    user_input
                );

                session_logger.log_step_observation(state.step_count, &observation);

                prev_assistant_reply = Some(generated_text);
                prev_feedback_observation = Some(ChatMessage {
                    role: "user".to_string(),
                    content: observation,
                    images: None, // Historical messages NEVER store or re-transmit giant base64!
                });

                // Continue autonomous loop!
                continue;
            }
        }

        if state.step_count >= max_steps && !last_generated_text.is_empty() {
            session_logger.log_session_end("Terminated (Max reasoning steps exceeded)", state.step_count);
            let last_parsed = ToolExecutor::parse_model_output(&last_generated_text);
            if matches!(last_parsed.action, ToolAction::Execute(_)) {
                let cp = ErrorCheckpoint::new(
                    ErrorCategory::SystemError,
                    "Agent Max Reasoning Steps Exceeded",
                    &format!("The autonomous reasoning loop hit the maximum safety limit ({} steps).", max_steps),
                    None,
                    "Adjust step limit using '/config set max_steps <num>' or refine your prompt.",
                );
                println!("{}", cp.format_terminal());
            }
        }

        if terminal_minimized {
            crate::tools::os_metadata::OsMetadataTools::restore_terminal_window();
        }

        // Commit interaction turn to persistent chat history
        state.chat_history.push(initial_user_msg);
        if !last_generated_text.is_empty() {
            state.chat_history.push(ChatMessage {
                role: "assistant".to_string(),
                content: last_generated_text,
                images: None,
            });
        }

        Ok(())
    }
}

/// Helper filter for streaming Chat content while suppressing dotted lines and Tool blocks
struct StreamChatFilter {
    buffer: String,
    printed_len: usize,
    header_printed: bool,
    hit_delimiter: bool,
}

impl StreamChatFilter {
    fn new() -> Self {
        Self {
            buffer: String::new(),
            printed_len: 0,
            header_printed: false,
            hit_delimiter: false,
        }
    }

    fn push_token(
        &mut self,
        token: &str,
        spinner: &std::sync::Arc<std::sync::Mutex<Option<crate::cli::ProgressSpinner>>>,
    ) {
        if self.hit_delimiter {
            return;
        }

        self.buffer.push_str(token);

        // Check if buffer contains a tool delimiter line
        if let Some(pos) = Self::find_delimiter_pos(&self.buffer) {
            self.hit_delimiter = true;
            self.flush_up_to(pos, spinner);
            return;
        }

        // Determine safe length to print (holding back potential delimiter at end)
        let safe_len = Self::safe_print_len(&self.buffer);
        self.flush_up_to(safe_len, spinner);
    }

    fn find_delimiter_pos(buf: &str) -> Option<usize> {
        for (idx, _) in buf.match_indices('\n') {
            let after = &buf[idx + 1..];
            let line = after.lines().next().unwrap_or("").trim();
            let lower = line.to_lowercase();
            if lower.starts_with("tool:") {
                return Some(idx);
            }
            if line.len() >= 3 && line.chars().all(|c| c == '.' || c == '-' || c == '_' || c == '=') {
                return Some(idx);
            }
        }
        let first_line = buf.lines().next().unwrap_or("").trim();
        let first_lower = first_line.to_lowercase();
        if first_lower.starts_with("tool:") {
            return Some(0);
        }
        if first_line.len() >= 3 && first_line.chars().all(|c| c == '.' || c == '-' || c == '_' || c == '=') {
            return Some(0);
        }
        None
    }

    fn safe_print_len(buf: &str) -> usize {
        if let Some(idx) = buf.rfind('\n') {
            let tail = buf[idx + 1..].trim_start();
            let lower = tail.to_lowercase();
            if "tool:".starts_with(&lower)
                || (!tail.is_empty() && tail.chars().all(|c| c == '.' || c == '-' || c == '_' || c == '='))
            {
                return idx;
            }
        }
        buf.len()
    }

    fn flush_up_to(
        &mut self,
        end_idx: usize,
        spinner: &std::sync::Arc<std::sync::Mutex<Option<crate::cli::ProgressSpinner>>>,
    ) {
        if end_idx <= self.buffer.len() {
            let slice = &self.buffer[..end_idx];
            let clean = Self::strip_chat_prefix(slice);
            if clean.len() > self.printed_len {
                let to_print = &clean[self.printed_len..];
                if !to_print.is_empty() {
                    if !self.header_printed {
                        if let Ok(mut lock) = spinner.lock() {
                            if let Some(sp) = lock.take() {
                                sp.stop_and_clear();
                            }
                        }
                        print!("{}", "🤖 J.A.A: ".green().bold());
                        self.header_printed = true;
                    }
                    print!("{}", to_print);
                    let _ = stdout().flush();
                    self.printed_len = clean.len();
                }
            }
        }
    }

    fn strip_chat_prefix(s: &str) -> &str {
        let mut trimmed_start = s.trim_start();
        let lower = trimmed_start.to_lowercase();
        if lower.starts_with("mode:") {
            if let Some(nl) = trimmed_start.find('\n') {
                trimmed_start = trimmed_start[nl + 1..].trim_start();
            } else {
                return ""; // still buffering leading mode: line
            }
        } else if trimmed_start.len() <= 6 && "mode:".starts_with(&lower) {
            return ""; // still buffering "mode:"
        }

        let lower = trimmed_start.to_lowercase();
        if lower.starts_with("chat:") {
            let mut remainder = &trimmed_start[5..];
            if remainder.starts_with(' ') {
                remainder = &remainder[1..];
            }
            remainder
        } else if trimmed_start.len() <= 6 && "chat:".starts_with(&lower) {
            "" // still buffering "chat:"
        } else if lower.starts_with("tool:") || lower.starts_with("...") || lower.starts_with("---") {
            ""
        } else {
            trimmed_start
        }
    }

    fn finish(&mut self, final_chat: &str) {
        let clean = final_chat.trim();
        if !clean.is_empty() {
            if !self.header_printed {
                print!("{}", "🤖 J.A.A: ".green().bold());
                println!("{}", clean);
                self.header_printed = true;
            } else if clean.len() > self.printed_len {
                let remaining = &clean[self.printed_len..];
                println!("{}", remaining);
            } else {
                println!();
            }
        } else if self.header_printed {
            println!();
        }
    }
}
