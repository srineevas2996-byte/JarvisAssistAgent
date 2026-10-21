use colored::*;

pub struct UI;

impl UI {
    pub fn print_banner() {
        println!("{}", r#"
      ___       ___       ___  
     |_  |     /   \     /   \ 
       | |    / /_\ \   / /_\ \
       | | _  |  _  |   |  _  |
   /\__/ // | | | | | _ | | | |
   \____/ \_| \_| |_/(_)\_| |_/  J.A.A (jarvis_assist_agent) v1.0
"#.cyan().bold());
        println!("{}", "⚡ High-Performance Native CLI Autonomous Agent".dimmed());
        println!("{}", "Type your request or use slash commands (/help, /status, /model, /skill, /tool, /mcp)\n".dimmed());
    }

    pub fn print_status_badges(model: &str, skills_count: usize, tools_count: usize, mcp_count: usize) {
        let model_badge = if model.is_empty() {
            "[No Model Loaded]".red()
        } else {
            format!("[Model: {}]", model).green()
        };

        let skills_badge = format!("[Skills: {} active]", skills_count).blue();
        let tools_badge = format!("[Tools: {} ready]", tools_count).magenta();
        let mcp_badge = format!("[MCP: {} online]", mcp_count).yellow();

        println!("{} {} {} {}", model_badge, skills_badge, tools_badge, mcp_badge);
    }

    pub fn print_help() {
        println!("\n{}", "AVAILABLE SLASH COMMANDS:".cyan().bold());
        println!("  {:<22} {}", "/model".bold(), "List available .gguf files or switch active model (/model <name>)");
        println!("  {:<22} {}", "/skill".bold(), "List and toggle skills (/skill enable <id>, /skill disable <id>, /skill view <id>)");
        println!("  {:<22} {}", "/tool".bold(), "Inspect plugged tools and toggle availability (/tool enable <name>, /tool test <name>)");
        println!("  {:<22} {}", "/mcp".bold(), "Manage Model Context Protocol servers (/mcp list, /mcp toggle <name>, /mcp reload)");
        println!("  {:<22} {}", "/status".bold(), "Display full configuration status (model, context, skills, tools, MCP, system)");
        println!("  {:<22} {}", "/config".bold(), "View or tweak saved settings (/config set num_ctx, temp, steps)");
        println!("  {:<22} {}", "/navigation_vision".bold(), "Choose OS navigation method: 1 (metadata Tag IDs) vs 2 (VLM vision)");
        println!("  {:<22} {}", "/image <path>".bold(), "Attach an image for multimodal vision inspection");
        println!("  {:<22} {}", "/clear".bold(), "Clear terminal screen and conversation history");
        println!("  {:<22} {}", "/help".bold(), "Show this help screen");
        println!("  {:<22} {}", "/exit or /bye".bold(), "Exit J.A.A CLI\n");

        println!("{}", "INLINE '@' TARGETING DIRECTIVES:".magenta().bold());
        println!("  {:<22} {}", "@<name>".bold(), "Smart auto-resolve: activates skill, tool, or MCP server for this prompt");
        println!("  {:<22} {}", "@skill:<name>".bold(), "Enforces execution runbook of specific skill (e.g. @skill:os_navigation_vlm)");
        println!("  {:<22} {}", "@tool:<name>".bold(), "Enforces priority usage of specific tool (e.g. @tool:capture_screen)");
        println!("  {:<22} {}", "@mcp:<name>".bold(), "Routes request to specific MCP server");
        println!("  {:<22} {}", "@help".bold(), "Show dedicated guide and available targets for @ directives\n");
    }
}
