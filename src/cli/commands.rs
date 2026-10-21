use crate::agent::state::AgentState;
use crate::checkpoint::{ErrorCategory, ErrorCheckpoint};
use crate::cli::ui::UI;
use crate::engine::multimodal::MultimodalInput;
use crate::tools::executor::ExtractedToolCall;
use crate::tools::executor::ToolExecutor;
use colored::*;
use serde_json::Value;
use std::path::{Path, PathBuf};

pub fn print_diagnostic_error(e: &str) {
    if e.contains("[CHECKPOINT:") {
        println!("{}", e.red());
    } else {
        println!("{}", format!("❌ {}", e).red());
    }
}

pub struct CommandHandler;

impl CommandHandler {
    pub async fn handle_command(state: &mut AgentState, input: &str) -> Result<bool, String> {
        let trimmed = input.trim();
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.is_empty() {
            return Ok(true);
        }

        let cmd = parts[0].to_lowercase();
        let args = &parts[1..];

        match cmd.as_str() {
            "/model" => {
                Self::handle_model_command(state, args).await?;
            }
            "/skill" => {
                Self::handle_skill_command(state, args)?;
            }
            "/tool" => {
                Self::handle_tool_command(state, args).await?;
            }
            "/mcp" => {
                Self::handle_mcp_command(state, args).await?;
            }
            "/status" => {
                Self::handle_status_command(state)?;
            }
            "/config" | "/settings" => {
                Self::handle_config_command(state, args)?;
            }
            "/image" | "/img" => {
                if args.is_empty() {
                    println!("{}", "Usage: /image <file_path> [optional prompt]".yellow());
                    println!("{}", "  Example 1 (together):     /image screen.png What is displayed on screen?".dimmed());
                    println!("{}", "  Example 2 (independent):  /image photo.jpg (then press Enter to analyze)".dimmed());
                } else {
                    let image_path = args[0];
                    let prompt_text = if args.len() > 1 {
                        args[1..].join(" ")
                    } else {
                        String::new()
                    };

                    match MultimodalInput::from_file(image_path) {
                        Ok(img) => {
                            println!(
                                "{}",
                                format!("✓ Loaded image: {} (format: {})", image_path, img.mime_type).green()
                            );
                            state.pending_image = Some(img);

                            if !prompt_text.is_empty() {
                                println!("{}", format!("Processing prompt with attached image: '{}'", prompt_text).cyan());
                                crate::agent::loop_engine::AgentLoop::process_user_input(state, &prompt_text).await?;
                            } else {
                                println!("{}", "📷 Image attached. Press Enter or type a question to analyze independently.".dimmed());
                            }
                        }
                        Err(e) => {
                            let cp = ErrorCheckpoint::new(
                                ErrorCategory::MultimodalError,
                                "Failed to Load Image",
                                &format!("Unable to open or encode image file at '{}'", image_path),
                                Some(&e),
                                "Ensure the file exists, has a valid image extension (.png, .jpg, .webp), and read permissions are granted.",
                            );
                            println!("{}", cp.format_terminal());
                        }
                    }
                }
            }
            "/clear" => {
                print!("\x1B[2J\x1B[1;1H");
                state.reset_chat();
                UI::print_banner();
                println!("{}", "Chat history cleared.".dimmed());
            }
            "/help" => {
                UI::print_help();
            }
            "/exit" | "/bye" | "/quit" => {
                state.sync_and_save();
                println!("{}", "✓ Configuration saved to config/config.json. Shutting down J.A.A (jarvis_assist_agent). Goodbye!".cyan().bold());
                return Ok(false);
            }
            "/navigation_vision" | "/nav_vision" | "/vision_mode" => {
                Self::handle_navigation_vision_command(state, &args).await?;
            }
            "@" | "@help" | "/directives" => {
                crate::agent::PromptDirectives::print_help(&state.skill_manager, &state.tool_registry, &state.mcp_manager);
            }
            _ => {
                println!(
                    "{}",
                    format!("Unknown command '{}'. Type /help for list of commands.", cmd).yellow()
                );
            }
        }

        Ok(true)
    }

    async fn handle_model_command(state: &mut AgentState, args: &[&str]) -> Result<(), String> {
        if args.is_empty() {
            println!("\n{}", "================ DEDICATED MODEL FOLDER (models/) ================".cyan().bold());
            println!("  Path: {}", state.engine.models_dir.display().to_string().yellow());
            println!("  Drop your .gguf models and vision projectors (mmproj-*.gguf) into this folder.\n");

            let unified_models = state.engine.scan_unified_models();
            let mut cur_idx = 1;

            let local_ggufs: Vec<_> = unified_models.iter().filter(|m| matches!(m.source, crate::engine::model_sources::ModelOrigin::LocalGguf)).collect();
            let ollama_cache: Vec<_> = unified_models.iter().filter(|m| matches!(m.source, crate::engine::model_sources::ModelOrigin::OllamaCache)).collect();

            if !local_ggufs.is_empty() {
                println!("{}", "  Local GGUF Models (models/ - HuggingFace / Local):".bold());
                for m in &local_ggufs {
                    let is_active = state
                        .engine
                        .active_model
                        .as_ref()
                        .map(|cur| cur.file_name == m.display_name || cur.model_name == m.display_name)
                        .unwrap_or(false);

                    let active_marker = if is_active {
                        "● [ACTIVE]".green().bold()
                    } else {
                        "○ [GGUF]  ".cyan()
                    };

                    let vision_badge = if m.has_vision { " (Vision)".bright_magenta().to_string() } else { String::new() };

                    println!(
                        "  [{:>2}] {} {:<32} {:>8.1} MB  {:<8}  (arch: {}){}",
                        cur_idx,
                        active_marker,
                        m.display_name.bold(),
                        m.file_size_mb,
                        m.quant_type.yellow(),
                        m.architecture,
                        vision_badge
                    );
                    cur_idx += 1;
                }
                println!();
            }

            if !ollama_cache.is_empty() {
                println!("{}", "  Ollama Cached Models (Direct GPU Access via GGUF Blobs):".bold());
                for m in &ollama_cache {
                    let is_active = state
                        .engine
                        .active_model
                        .as_ref()
                        .map(|cur| cur.file_name == m.display_name || cur.model_name == m.display_name)
                        .unwrap_or(false);

                    let active_marker = if is_active {
                        "● [ACTIVE]".green().bold()
                    } else {
                        "○ [OLLAMA]".bright_yellow()
                    };

                    let vision_badge = if m.has_vision { " (Vision)".bright_magenta().to_string() } else { String::new() };

                    println!(
                        "  [{:>2}] {} {:<32} {:>8.1} MB  {:<8}  (arch: {}){}",
                        cur_idx,
                        active_marker,
                        m.display_name.bold(),
                        m.file_size_mb,
                        m.quant_type.dimmed(),
                        m.architecture,
                        vision_badge
                    );
                    cur_idx += 1;
                }
                println!();
            }

            if unified_models.is_empty() {
                println!("{}", "  No models currently detected in models/ folder or Ollama cache.".yellow());
                println!("{}", "  Drop a .gguf file into models/ or pull with '/model pull hf:<repo>/<file>'.".dimmed());
            }

            println!(
                "\n  {} /model <#> | /model <name> | /model pull hf:<repo>/<file> | /model import <ollama_name>\n",
                "Commands:".bold()
            );
        } else if args[0].eq_ignore_ascii_case("pull") {
            if args.len() < 2 {
                println!("{}", "Usage: /model pull <hf:owner/repo/file.gguf | direct_https_url>".yellow());
                println!("{}", "Example: /model pull hf:bartowski/Qwen2.5-VL-3B-Instruct-GGUF/Qwen2.5-VL-3B-Instruct-Q4_K_M.gguf".dimmed());
            } else {
                let spec = args[1];
                match crate::engine::model_sources::ModelSourceManager::pull_from_huggingface(spec, &state.engine.models_dir).await {
                    Ok(path) => {
                        println!("{}", format!("✓ Saved model to: {}", path.display()).green().bold());
                        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("model.gguf");
                        match state.engine.load_and_pin_model(file_name, true).await {
                            Ok(info) => {
                                state.sync_and_save();
                                state.refresh_static_prompt();
                                println!("{}", format!("✓ Successfully activated downloaded model '{}'", info.file_name).green().bold());
                            }
                            Err(e) => print_diagnostic_error(&e),
                        }
                    }
                    Err(e) => print_diagnostic_error(&e),
                }
            }
        } else if args[0].eq_ignore_ascii_case("import") {
            if args.len() < 2 {
                println!("{}", "Usage: /model import <ollama_model_tag>".yellow());
                println!("{}", "Example: /model import qwen3-vl:2b".dimmed());
            } else {
                let tag = args[1];
                match crate::engine::model_sources::ModelSourceManager::import_ollama_model(tag, &state.engine.models_dir) {
                    Ok(dest) => {
                        println!("{}", format!("✓ Imported model weights to: {}", dest.display()).green().bold());
                        let file_name = dest.file_name().and_then(|n| n.to_str()).unwrap_or("model.gguf");
                        match state.engine.load_and_pin_model(file_name, true).await {
                            Ok(info) => {
                                state.sync_and_save();
                                state.refresh_static_prompt();
                                println!("{}", format!("✓ Successfully activated imported model '{}'", info.file_name).green().bold());
                            }
                            Err(e) => print_diagnostic_error(&e),
                        }
                    }
                    Err(e) => print_diagnostic_error(&e),
                }
            }
        } else if args[0].eq_ignore_ascii_case("build") || args[0].eq_ignore_ascii_case("load") {
            if args.len() < 2 {
                println!("{}", "Usage: /model load <gguf_file_in_models>".yellow());
            } else {
                let gguf_name = args[1];
                match state.engine.load_and_pin_model(gguf_name, true).await {
                    Ok(info) => {
                        state.sync_and_save();
                        state.refresh_static_prompt();
                        println!(
                            "{}",
                            format!(
                                "✓ Successfully activated and pinned model '{}' (arch: {}, quant: {}, context: {} tokens) [Saved to config]",
                                info.model_name, info.architecture, info.quant_type, info.context_length
                            )
                            .green()
                            .bold()
                        );
                    }
                    Err(e) => {
                        print_diagnostic_error(&e);
                    }
                }
            }
        } else {
            let model_query = args.join(" ");
            match state.engine.load_and_pin_model(&model_query, true).await {
                Ok(info) => {
                    state.sync_and_save();
                    state.refresh_static_prompt();
                    println!(
                        "{}",
                        format!(
                            "✓ Successfully activated and pinned model '{}' (arch: {}, quant: {}, size: {:.1} MB, context: {} tokens) [Saved to config]",
                            info.file_name, info.architecture, info.quant_type, info.file_size_mb, info.context_length
                        )
                        .green()
                        .bold()
                    );
                }
                Err(e) => {
                    print_diagnostic_error(&e);
                }
            }
        }
        Ok(())
    }

    fn handle_skill_command(state: &mut AgentState, args: &[&str]) -> Result<(), String> {
        if args.is_empty() {
            println!("\n{}", "================ DEDICATED SKILLS FOLDER (skills/) ================".cyan().bold());
            println!("  Path: {}", state.skill_manager.skills_dir.display().to_string().yellow());
            println!("  Drop your *.md skill files (e.g. os_navigation.md, research.md) into this folder.\n");

            let skills = state.skill_manager.list_skills();
            if skills.is_empty() {
                println!("{}", "  No .md skill files recognized in skills/ directory.".dimmed());
            } else {
                for (i, s) in skills.iter().enumerate() {
                    let status = if s.is_active {
                        "[ACTIVE]".green().bold()
                    } else {
                        "[DISABLED]".dimmed()
                    };

                    let file_name = s.file_path.file_name().and_then(|n| n.to_str()).unwrap_or("skill.md");

                    println!(
                        "  [{:>2}] {} {:<20} ({:<18}) - {}",
                        i + 1,
                        status,
                        s.id.bold(),
                        file_name.cyan(),
                        s.description
                    );
                }
            }
            println!(
                "\n  {} /skill enable <# or name> | /skill disable <# or name> | /skill view <# or name>\n",
                "Configure Skill:".bold()
            );
        } else {
            let subcmd = args[0].to_lowercase();
            match subcmd.as_str() {
                "enable" => {
                    if args.len() < 2 {
                        println!("{}", "Usage: /skill enable <# or name>".yellow());
                    } else {
                        let id = args[1];
                        match state.skill_manager.toggle_skill(id, Some(true)) {
                            Ok((name, _)) => {
                                state.sync_and_save();
                                state.static_system_prompt = state.build_static_system_prompt();
                                println!("{}", format!("✓ Skill '{}' is now ACTIVE (saved to config).", name).green());
                            }
                            Err(e) => print_diagnostic_error(&e),
                        }
                    }
                }
                "disable" => {
                    if args.len() < 2 {
                        println!("{}", "Usage: /skill disable <# or name>".yellow());
                    } else {
                        let id = args[1];
                        match state.skill_manager.toggle_skill(id, Some(false)) {
                            Ok((name, _)) => {
                                state.sync_and_save();
                                state.static_system_prompt = state.build_static_system_prompt();
                                println!("{}", format!("✓ Skill '{}' is now DISABLED (saved to config).", name).yellow());
                            }
                            Err(e) => print_diagnostic_error(&e),
                        }
                    }
                }
                "view" => {
                    if args.len() < 2 {
                        println!("{}", "Usage: /skill view <# or name>".yellow());
                    } else {
                        let id = args[1];
                        let skill_opt = if let Ok(idx) = id.parse::<usize>() {
                            let list = state.skill_manager.list_skills();
                            if idx >= 1 && idx <= list.len() {
                                Some(list[idx - 1].clone())
                            } else {
                                None
                            }
                        } else {
                            state.skill_manager.get_skill(id).cloned()
                        };

                        if let Some(s) = skill_opt {
                            println!("\n=== SKILL FILE: {} ({}) ===", s.name.bold(), s.file_path.display());
                            println!("*Description*: {}", s.description);
                            println!("\n{}\n", s.instructions);
                        } else {
                            let cp = ErrorCheckpoint::new(
                                ErrorCategory::SkillNotFound,
                                "Skill Not Found",
                                &format!("No skill matching '{}' in skills/ folder.", id),
                                None,
                                "Use '/skill' to view recognized skills.",
                            );
                            println!("{}", cp.format_terminal());
                        }
                    }
                }
                "reload" => {
                    state.skill_manager.reload(&state.settings.active_skills);
                    println!("{}", "✓ Rescanned skills/ directory and recognized all skill files.".green());
                }
                other => {
                    // Quick toggle shorthand: /skill <# or name>
                    match state.skill_manager.toggle_skill(other, None) {
                        Ok((name, new_state)) => {
                            state.sync_and_save();
                            let label = if new_state { "ACTIVE" } else { "DISABLED" };
                            println!("{}", format!("✓ Skill '{}' is now {} (saved to config).", name, label).cyan());
                        }
                        Err(e) => print_diagnostic_error(&e),
                    }
                }
            }
        }
        Ok(())
    }

    async fn handle_tool_command(state: &mut AgentState, args: &[&str]) -> Result<(), String> {
        if args.is_empty() {
            println!("\n{}", "================ DEDICATED TOOLS FOLDER (tools/) ================".cyan().bold());
            println!("  Path: {}", state.tool_registry.tools_dir.display().to_string().yellow());
            println!("  Drop custom tool definitions (*.json) or scripts (*.ps1, *.bat, *.py) into this folder.\n");

            let tools = state.tool_registry.list_tools();
            for (i, t) in tools.iter().enumerate() {
                let status = if t.enabled {
                    "[READY]".green().bold()
                } else {
                    "[OFF]".dimmed()
                };

                let src = match &t.source {
                    crate::tools::ToolSource::Native => "Native Rust".bright_cyan(),
                    crate::tools::ToolSource::Mcp { server_name } => {
                        format!("MCP: {}", server_name).yellow()
                    }
                    crate::tools::ToolSource::CustomFile { file_path, .. } => {
                        format!("tools/{}", file_path.file_name().and_then(|n| n.to_str()).unwrap_or("custom")).bright_green()
                    }
                    crate::tools::ToolSource::Script { .. } => "Script".dimmed(),
                };

                println!("  [{:>2}] {} {:<24} [{}] - {}", i + 1, status, t.name.bold(), src, t.description);
            }
            println!(
                "\n  {} /tool enable <# or name> | /tool disable <# or name> | /tool test <# or name>\n",
                "Configure Tool:".bold()
            );
        } else {
            let subcmd = args[0].to_lowercase();
            match subcmd.as_str() {
                "enable" => {
                    if args.len() < 2 {
                        println!("{}", "Usage: /tool enable <# or name>".yellow());
                    } else {
                        let target = args[1];
                        match state.tool_registry.toggle_tool(target, Some(true)) {
                            Ok((name, _)) => {
                                state.sync_and_save();
                                state.static_system_prompt = state.build_static_system_prompt();
                                println!("{}", format!("✓ Tool '{}' is now READY (saved to config).", name).green());
                            }
                            Err(e) => print_diagnostic_error(&e),
                        }
                    }
                }
                "disable" => {
                    if args.len() < 2 {
                        println!("{}", "Usage: /tool disable <# or name>".yellow());
                    } else {
                        let target = args[1];
                        match state.tool_registry.toggle_tool(target, Some(false)) {
                            Ok((name, _)) => {
                                state.sync_and_save();
                                state.static_system_prompt = state.build_static_system_prompt();
                                println!("{}", format!("✓ Tool '{}' is now DISABLED (saved to config).", name).yellow());
                            }
                            Err(e) => print_diagnostic_error(&e),
                        }
                    }
                }
                "reload" => {
                    state.tool_registry.scan_tools_dir();
                    state.static_system_prompt = state.build_static_system_prompt();
                    state.sync_and_save();
                    println!("{}", "✓ Rescanned tools/ directory for new tool files.".green());
                }
                "test" => {
                    if args.len() < 2 {
                        println!("{}", "Usage: /tool test <# or name> [json_args]".yellow());
                    } else {
                        let target = args[1];
                        let tool_name = if let Ok(idx) = target.parse::<usize>() {
                            let list = state.tool_registry.list_tools();
                            if idx >= 1 && idx <= list.len() {
                                list[idx - 1].name.clone()
                            } else {
                                target.to_string()
                            }
                        } else {
                            target.to_string()
                        };

                        let json_str = if args.len() > 2 {
                            args[2..].join(" ")
                        } else {
                            "{}".to_string()
                        };

                        let parsed_args: Value = serde_json::from_str(&json_str)
                            .unwrap_or(serde_json::json!({}));

                        println!("{}", format!("Testing tool '{}' with arguments: {}", tool_name, parsed_args).dimmed());

                        let call = ExtractedToolCall {
                            tool_name: tool_name.clone(),
                            arguments: parsed_args,
                            raw_matched: String::new(),
                        };

                        match ToolExecutor::execute(&state.tool_registry, &state.mcp_manager, &call).await {
                            Ok(output) => {
                                println!("\n{}", "Tool Output:".green().bold());
                                println!("{}\n", output);
                            }
                            Err(err) => {
                                print_diagnostic_error(&err);
                            }
                        }
                    }
                }
                other => {
                    match state.tool_registry.toggle_tool(other, None) {
                        Ok((name, new_state)) => {
                            state.sync_and_save();
                            let label = if new_state { "READY" } else { "DISABLED" };
                            println!("{}", format!("✓ Tool '{}' is now {} (saved to config).", name, label).cyan());
                        }
                        Err(e) => print_diagnostic_error(&e),
                    }
                }
            }
        }
        Ok(())
    }

    async fn handle_mcp_command(state: &mut AgentState, args: &[&str]) -> Result<(), String> {
        if args.is_empty() || args[0].eq_ignore_ascii_case("list") {
            println!("\n{}", "================ DEDICATED MCP FOLDER (mcp/) ================".cyan().bold());
            println!("  Path: {}", state.mcp_manager.mcp_dir.display().to_string().yellow());
            println!("  Drop your individual MCP server config files (*.json) into this folder.\n");

            let servers = state.mcp_manager.list_servers();
            if servers.is_empty() {
                println!("{}", "  No MCP server configuration files recognized in mcp/ directory.".yellow());
            } else {
                for (i, (name, cfg, is_online)) in servers.iter().enumerate() {
                    let status = if *is_online {
                        "[ONLINE]".green().bold()
                    } else if cfg.enabled {
                        "[CONNECTING]".yellow()
                    } else {
                        "[DISABLED]".dimmed()
                    };

                    let tools_count = state
                        .mcp_manager
                        .server_tools
                        .get(*name)
                        .map(|t| t.len())
                        .unwrap_or(0);

                    let file_label = cfg
                        .file_path
                        .as_ref()
                        .and_then(|p| p.file_name())
                        .and_then(|n| n.to_str())
                        .unwrap_or("config.json");

                    println!(
                        "  [{:>2}] ● {} {:<18} ({}) ({} tools) - {}",
                        i + 1,
                        status,
                        name.bold(),
                        file_label.cyan(),
                        tools_count,
                        cfg.description
                    );

                    // Print exposed tools
                    if let Some(tools) = state.mcp_manager.server_tools.get(*name) {
                        for tool in tools {
                            println!("        ↳ `{}`: {}", tool.name.cyan(), tool.description.dimmed());
                        }
                    }
                }
            }
            println!(
                "\n  {} /mcp toggle <# or name> | /mcp enable <# or name> | /mcp reload\n",
                "Configure MCP:".bold()
            );
        } else {
            let subcmd = args[0].to_lowercase();
            match subcmd.as_str() {
                "toggle" | "enable" | "disable" => {
                    if args.len() < 2 {
                        println!("{}", "Usage: /mcp toggle <# or name>".yellow());
                    } else {
                        let target = args[1];
                        let enable_flag = if subcmd == "enable" {
                            Some(true)
                        } else if subcmd == "disable" {
                            Some(false)
                        } else {
                            None
                        };

                        match state.mcp_manager.toggle_server(target, enable_flag).await {
                            Ok((name, new_state)) => {
                                let label = if new_state { "ONLINE" } else { "DISABLED" };
                                println!("{}", format!("✓ MCP Server '{}' is now {}.", name, label).green());

                                // Resync tools with registry
                                if let Some(tools) = state.mcp_manager.server_tools.get(&name) {
                                    state.tool_registry.register_mcp_tools(&name, tools);
                                }
                            }
                            Err(e) => print_diagnostic_error(&e),
                        }
                    }
                }
                "reload" => {
                    state.mcp_manager.start_enabled_servers().await;
                    for (srv_name, tools) in &state.mcp_manager.server_tools {
                        state.tool_registry.register_mcp_tools(srv_name, tools);
                    }
                    println!("{}", "✓ Rescanned mcp/ directory and reloaded all MCP server files.".green());
                }
                other => {
                    // Quick toggle shorthand: /mcp <# or name>
                    match state.mcp_manager.toggle_server(other, None).await {
                        Ok((name, new_state)) => {
                            let label = if new_state { "ONLINE" } else { "DISABLED" };
                            println!("{}", format!("✓ MCP Server '{}' is now {}.", name, label).cyan());
                            if let Some(tools) = state.mcp_manager.server_tools.get(&name) {
                                state.tool_registry.register_mcp_tools(&name, tools);
                            }
                        }
                        Err(e) => print_diagnostic_error(&e),
                    }
                }
            }
        }
        Ok(())
    }

    fn handle_status_command(state: &AgentState) -> Result<(), String> {
        println!("\n{}", "================ J.A.A (jarvis_assist_agent) STATUS ================".cyan().bold());

        // 1. Models Folder
        let models = state.engine.scan_models();
        println!("\n{}", "1. DEDICATED MODEL FOLDER (models/):".bold());
        println!("   Path:         {}", state.engine.models_dir.display());
        println!("   Models Found: {}", models.len().to_string().cyan());
        if let Some(ref m) = state.engine.active_model {
            println!("   Active Model: {} (arch: {}, quant: {}, {:.1} MB, {} ctx)",
                m.file_name.green().bold(), m.architecture, m.quant_type.yellow(), m.file_size_mb, m.context_length
            );
        } else {
            println!("   Active Model: {}", "None loaded. Use /model to select from models/".red());
        }
        println!("   Engine Mode:  {}", "Direct Subprocess Pipe (jarvis-engine.exe - 0 network sockets)".green().bold());
        println!("   Context Limit:{}", if state.engine.num_ctx > 0 { format!(" {} tokens (num_ctx)", state.engine.num_ctx).green().to_string() } else { " Auto / Model default".dimmed().to_string() });

        // 2. Skills Folder
        let all_skills = state.skill_manager.list_skills();
        let active_skills = state.skill_manager.get_active_ids();
        println!("\n{}", "2. DEDICATED SKILLS FOLDER (skills/):".bold());
        println!("   Path:         {}", state.skill_manager.skills_dir.display());
        println!("   Files Found:  {}", all_skills.len().to_string().cyan());
        println!("   Active Count: {}", active_skills.len().to_string().green());
        for s in &all_skills {
            let status = if s.is_active { "[ACTIVE]".green() } else { "[DISABLED]".dimmed() };
            let file_name = s.file_path.file_name().and_then(|n| n.to_str()).unwrap_or("skill.md");
            println!("   - {:<18} {} ({})", s.name.bold(), status, file_name.dimmed());
        }

        // 3. Tools Folder & Plugged Tools
        let tools = state.tool_registry.list_tools();
        let ready_tools: Vec<_> = tools.iter().filter(|t| t.enabled).collect();
        println!("\n{}", "3. DEDICATED TOOLS FOLDER & PLUGGED TOOLS (tools/):".bold());
        println!("   Path:         {}", state.tool_registry.tools_dir.display());
        println!("   Total / Ready: {} / {}", tools.len(), ready_tools.len().to_string().green());
        for t in &ready_tools {
            let src_label = match &t.source {
                crate::tools::ToolSource::Native => "Native Rust",
                crate::tools::ToolSource::Mcp { server_name } => server_name,
                crate::tools::ToolSource::CustomFile { file_path, .. } => {
                    file_path.file_name().and_then(|n| n.to_str()).unwrap_or("tools/custom")
                }
                crate::tools::ToolSource::Script { .. } => "Script",
            };
            println!("   - {:<22} [{}]", t.name.bold(), src_label.dimmed());
        }

        // 4. MCP Folder & Running Servers
        println!("\n{}", "4. DEDICATED MCP FOLDER & SERVERS (mcp/):".bold());
        println!("   Path:         {}", state.mcp_manager.mcp_dir.display());
        for (name, cfg, online) in state.mcp_manager.list_servers() {
            let badge = if online { "ONLINE".green() } else { "OFFLINE".dimmed() };
            let file_name = cfg.file_path.as_ref().and_then(|p| p.file_name()).and_then(|n| n.to_str()).unwrap_or("mcp.json");
            println!("   - {:<18} [{}] ({}) -> {}", name.bold(), badge, file_name.cyan(), cfg.command);
        }

        // 5. Host Info
        println!("\n{}", "5. HOST ENVIRONMENT:".bold());
        println!("   OS / Arch:    {} / {}", std::env::consts::OS, std::env::consts::ARCH);
        println!("   Working Dir:  {}", std::env::current_dir().unwrap_or_default().display());
        if let Some(ref img) = state.pending_image {
            println!("   Pending Img:  {}", img.image_path.display().to_string().cyan());
        }

        println!("{}\n", "===================================================================".cyan().bold());
        Ok(())
    }

    fn handle_config_command(state: &mut AgentState, args: &[&str]) -> Result<(), String> {
        if args.is_empty() {
            println!("\n{}", "================ PERSISTENT USER CONFIGURATION ================".cyan().bold());
            println!("  Config File:        {}", "config/config.json".yellow());
            println!("  Default Model:      {}", if state.settings.default_model.is_empty() { "None (auto-picks first found)".dimmed() } else { state.settings.default_model.green() });
            println!("  Engine Mode:        {}", "Direct Subprocess Pipe (jarvis-engine.exe - 0 network sockets)".green());
            println!("  Context Limit:      {} (num_ctx)", if state.settings.num_ctx > 0 { format!("{} tokens", state.settings.num_ctx).green().to_string() } else { "Auto (model default)".dimmed().to_string() });
            println!("  Temperature:        {}", state.settings.temperature);
            println!("  Max Agent Steps:    {}", state.settings.max_agent_steps);
            println!("  Active Skills:      {:?}", state.settings.active_skills);
            println!("  Disabled Tools:     {:?}", state.settings.disabled_tools);
            println!("\n  {} /config set num_ctx <tokens> | /config set temp <float> | /config set steps <int> | /config save\n", "Modify Config:".bold());
        } else {
            let sub = args[0].to_lowercase();
            match sub.as_str() {
                "set" => {
                    if args.len() < 3 {
                        println!("{}", "Usage: /config set <num_ctx|temp|steps> <value>".yellow());
                    } else {
                        let key = args[1].to_lowercase();
                        let val = args[2];
                        match key.as_str() {
                            "num_ctx" | "ctx" | "context" => {
                                if val.eq_ignore_ascii_case("auto") || val == "0" {
                                    state.settings.num_ctx = 0;
                                    state.engine.num_ctx = 0;
                                    state.sync_and_save();
                                    println!("{}", "✓ Saved num_ctx to '0' (Auto / Model Native Default).".green());
                                } else if let Ok(n) = val.parse::<u64>() {
                                    state.settings.num_ctx = n;
                                    state.engine.num_ctx = n;
                                    state.sync_and_save();
                                    println!("{}", format!("✓ Saved num_ctx '{}' tokens to config.json (active on next prompt/load).", n).green());
                                } else {
                                    println!("{}", "Invalid integer for num_ctx (e.g. 2048, 4096, 8192, 16384, or 'auto').".red());
                                }
                            }
                            "temp" | "temperature" => {
                                if let Ok(t) = val.parse::<f32>() {
                                    state.settings.temperature = t;
                                    state.engine.temperature = t;
                                    state.sync_and_save();
                                    println!("{}", format!("✓ Saved temperature '{}' to config.", t).green());
                                } else {
                                    println!("{}", "Invalid float for temperature.".red());
                                }
                            }
                            "steps" | "max_steps" => {
                                if let Ok(s) = val.parse::<usize>() {
                                    state.settings.max_agent_steps = s;
                                    state.sync_and_save();
                                    println!("{}", format!("✓ Saved max steps '{}' to config.", s).green());
                                } else {
                                    println!("{}", "Invalid integer for steps.".red());
                                }
                            }
                            other => {
                                println!("{}", format!("Unknown config key '{}'. Use: num_ctx, temp, steps", other).red());
                            }
                        }
                    }
                }
                "save" => {
                    state.sync_and_save();
                    println!("{}", "✓ All configuration settings, active model, skills, and tools saved to config/config.json.".green());
                }
                _ => {
                    println!("{}", "Usage: /config | /config set <key> <val> | /config save".yellow());
                }
            }
        }
        Ok(())
    }

    async fn handle_navigation_vision_command(state: &mut AgentState, args: &[&str]) -> Result<(), String> {
        let meta_active = state.skill_manager.get_skill("os_navigation_metadata").map(|s| s.is_active).unwrap_or(false);
        let vlm_active = state.skill_manager.get_skill("os_navigation_vlm").map(|s| s.is_active).unwrap_or(false);

        if args.is_empty() {
            println!("\n{}", "==================== OS NAVIGATION VISION SELECTOR ====================".cyan().bold());
            println!("  J.A.R.V.I.S. provides two high-performance navigation architectures for Windows desktop control:");
            println!();

            // Mode 1: Metadata & Accessibility
            let m_badge = if meta_active && !vlm_active {
                "● [ACTIVE]".green().bold()
            } else if meta_active {
                "○ [ENABLED]".cyan()
            } else {
                "○ [DISABLED]".dimmed()
            };
            println!("  [1] {} {}", m_badge, "Metadata & Accessibility Tree Mode (os_navigation_metadata)".bold());
            println!("      {} Reads Win32 UI Automation tree & hierarchical Tag IDs [1], [2], [3]", "• Mechanism:".dimmed());
            println!("      {} Ultra-fast (0 ms vision latency, 0 vision tokens evaluated)", "• Performance:".dimmed());
            println!("      {} 100% deterministic pixel coordinates directly from OS window stack", "• Precision:".dimmed());
            println!("      {} Native apps, Settings, File Explorer, Notepad, dialogs, form filling", "• Best For:".dimmed());
            println!("      {} Cannot see canvas drawings, flat pixel graphics, or games", "• Limitations:".dimmed());
            println!();

            // Mode 2: Visual / VLM
            let v_badge = if vlm_active && !meta_active {
                "● [ACTIVE]".green().bold()
            } else if vlm_active {
                "○ [ENABLED]".cyan()
            } else {
                "○ [DISABLED]".dimmed()
            };
            println!("  [2] {} {}", v_badge, "Visual Language Model (VLM) Mode (os_navigation_vlm)".bold());
            println!("      {} Full desktop screenshot capture via capture_screen + visual tokens", "• Mechanism:".dimmed());
            println!("      {} Moderate (evaluates visual tokens through multimodal projector)", "• Performance:".dimmed());
            println!("      {} Visual layout comprehension, icon recognition, spatial reasoning", "• Precision:".dimmed());
            println!("      {} MS Paint, Photoshop, Figma, web canvas, video players, games", "• Best For:".dimmed());
            println!("      {} Higher token overhead and compute latency per turn", "• Limitations:".dimmed());
            println!();

            // Mode 3: Hybrid
            let h_badge = if meta_active && vlm_active {
                "● [ACTIVE]".green().bold()
            } else {
                "○ [OPTIONAL]".dimmed()
            };
            println!("  [3] {} {}", h_badge, "Hybrid Adaptive Mode (Both Enabled)".bold());
            println!("      {} Employs metadata Tag IDs first for instant clicks, with VLM screenshot fallback", "• Mechanism:".dimmed());
            println!();

            let cur_mode = if meta_active && vlm_active {
                "Hybrid (Metadata + VLM)".cyan().bold()
            } else if meta_active {
                "Metadata-Only (Tag IDs, Ultra-Fast)".green().bold()
            } else if vlm_active {
                "Image-Based VLM (Screenshots, Visual Reasoning)".magenta().bold()
            } else {
                "None (Default General Assistant)".yellow()
            };

            println!("  Current Active Mode: {}", cur_mode);
            println!("\n  {} /navigation_vision <1 | 2 | 3> or /navigation_vision <metadata | image | hybrid>\n", "Switch Mode:".bold());
            return Ok(());
        }

        let choice = args[0].to_lowercase();
        match choice.as_str() {
            "1" | "metadata" | "meta" | "tag" | "tree" => {
                let _ = state.skill_manager.toggle_skill("os_navigation_metadata", Some(true));
                let _ = state.skill_manager.toggle_skill("os_navigation_vlm", Some(false));
                state.sync_and_save();
                state.refresh_static_prompt();
                println!("{}", "✓ Switched to Metadata & Accessibility Navigation Mode:".green().bold());
                println!("  • Active Skill: os_navigation_metadata");
                println!("  • Perception: Win32 Tag IDs [1], [2], [3] via get_screen_state");
                println!("  • Latency: Ultra-fast (0 vision tokens, instant response)");
                println!("  • Saved to config/config.json");
            }
            "2" | "image" | "vlm" | "vision" | "visual" => {
                let has_vision = state
                    .engine
                    .active_model
                    .as_ref()
                    .map(|m| m.is_multimodal_projector)
                    .unwrap_or(false);

                if !has_vision {
                    println!("{}", "⚠️  Notice: Current active model does not have vision projector (mmproj) loaded.".yellow());
                    println!("{}", "    For visual desktop navigation, ensure a vision model (e.g. qwen3-vl:2b) is loaded via '/model'.".dimmed());
                }

                let _ = state.skill_manager.toggle_skill("os_navigation_vlm", Some(true));
                let _ = state.skill_manager.toggle_skill("os_navigation_metadata", Some(false));
                state.sync_and_save();
                state.refresh_static_prompt();
                println!("{}", "✓ Switched to Image-Based (VLM) Desktop Navigation Mode:".green().bold());
                println!("  • Active Skill: os_navigation_vlm");
                println!("  • Perception: Full desktop screenshot capture via capture_screen");
                println!("  • Actuation: Physical (X, Y) pixel clicks, drag, and hotkeys");
                println!("  • Saved to config/config.json");
            }
            "3" | "hybrid" | "both" | "auto" => {
                let _ = state.skill_manager.toggle_skill("os_navigation_metadata", Some(true));
                let _ = state.skill_manager.toggle_skill("os_navigation_vlm", Some(true));
                state.sync_and_save();
                state.refresh_static_prompt();
                println!("{}", "✓ Switched to Hybrid Adaptive Navigation Mode:".green().bold());
                println!("  • Active Skills: os_navigation_metadata + os_navigation_vlm");
                println!("  • Perception: Metadata Tag IDs for controls + visual inspection fallback");
                println!("  • Saved to config/config.json");
            }
            other => {
                println!("{}", format!("Unknown option '{}'. Use: 1 (metadata), 2 (image/vlm), or 3 (hybrid).", other).yellow());
            }
        }

        Ok(())
    }
}
