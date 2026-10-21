#![allow(unused_imports, dead_code)]

mod agent;
pub mod checkpoint;
pub mod cli;
mod config;
mod engine;
mod mcp;
mod skills;
mod tools;

use agent::{AgentLoop, AgentState};
use clap::Parser;
use cli::{Repl, UI};
use colored::Colorize;

#[derive(Parser, Debug)]
#[command(name = "jaa")]
#[command(author = "J.A.A (jarvis_assist_agent)")]
#[command(version = "1.0.0")]
#[command(about = "J.A.A (jarvis_assist_agent) — High-performance native CLI autonomous agent", long_about = None)]
struct Args {
    /// Direct prompt to execute autonomously and exit
    #[arg(short, long)]
    prompt: Option<String>,

    /// Override model to load (.gguf filename or model name)
    #[arg(short, long)]
    model: Option<String>,

    /// Optional image file to attach for multimodal visual perception
    #[arg(long)]
    image: Option<String>,

    /// Force interactive REPL mode (default)
    #[arg(short, long, default_value_t = true)]
    interactive: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    // Print welcome HUD banner before initializing state and loading models
    UI::print_banner();

    let mut state = AgentState::initialize(args.model).await;

    if let Some(ref img_path) = args.image {
        match crate::engine::MultimodalInput::from_file(img_path) {
            Ok(img) => {
                println!(
                    "{}",
                    format!("📷 Attached image for goal: {} (format: {})", img_path, img.mime_type).cyan()
                );
                state.pending_image = Some(img);
            }
            Err(e) => {
                eprintln!("Failed to load image '{}': {}", img_path, e);
            }
        }
    }

    // Check if one-shot prompt was supplied
    if let Some(prompt) = args.prompt {
        println!("⚡ Executing goal: {}\n", prompt);
        if let Err(e) = AgentLoop::process_user_input(&mut state, &prompt).await {
            eprintln!("Execution failed: {}", e);
        }
        return Ok(());
    }

    // Default to interactive REPL
    Repl::run(state).await;

    Ok(())
}
