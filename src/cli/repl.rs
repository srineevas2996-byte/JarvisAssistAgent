use crate::agent::loop_engine::AgentLoop;
use crate::agent::state::AgentState;
use crate::cli::commands::CommandHandler;
use crate::cli::ui::UI;
use colored::*;
use std::io::{stdin, stdout, Write};

pub struct Repl;

impl Repl {
    pub async fn run(mut state: AgentState) {
        let model_name = state
            .engine
            .active_model
            .as_ref()
            .map(|m| m.file_name.as_str())
            .unwrap_or("");

        let skills_count = state.skill_manager.get_active_ids().len();
        let tools_count = state.tool_registry.tools.values().filter(|t| t.enabled).count();
        let mcp_count = state.mcp_manager.clients.len();

        UI::print_status_badges(model_name, skills_count, tools_count, mcp_count);
        println!();

        let mut input_buffer = String::new();

        loop {
            print!("{}", ">>> ".cyan().bold());
            let _ = stdout().flush();

            input_buffer.clear();
            match stdin().read_line(&mut input_buffer) {
                Ok(0) => {
                    // EOF reached
                    state.sync_and_save();
                    println!("\nConfiguration saved. Exiting J.A.A (jarvis_assist_agent).");
                    break;
                }
                Ok(_) => {
                    let trimmed = input_buffer.trim();
                    if trimmed.is_empty() {
                        if state.pending_image.is_some() {
                            if let Err(e) = AgentLoop::process_user_input(&mut state, "").await {
                                if e.contains("[CHECKPOINT:") {
                                    eprintln!("{}", e.red());
                                } else {
                                    eprintln!("{}", format!("Processing failed: {}", e).red());
                                }
                            }
                            println!();
                        }
                        continue;
                    }

                    // Check for slash command or direct @ / @help
                    if trimmed.starts_with('/') || trimmed == "@" || trimmed == "@help" {
                        match CommandHandler::handle_command(&mut state, trimmed).await {
                            Ok(continue_running) => {
                                if !continue_running {
                                    break;
                                }
                            }
                            Err(e) => {
                                println!("{}", format!("Command error: {}", e).red());
                            }
                        }
                    } else {
                        // Regular user prompt -> Agentic ReAct loop
                        if let Err(e) = AgentLoop::process_user_input(&mut state, trimmed).await {
                            if e.contains("[CHECKPOINT:") {
                                eprintln!("{}", e.red());
                            } else {
                                eprintln!("{}", format!("Processing failed: {}", e).red());
                            }
                        }
                        println!();
                    }
                }
                Err(err) => {
                    eprintln!("Error reading input: {}", err);
                    break;
                }
            }
        }
    }
}
