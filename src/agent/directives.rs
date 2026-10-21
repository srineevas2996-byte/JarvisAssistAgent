use crate::mcp::McpManager;
use crate::skills::SkillManager;
use crate::tools::ToolRegistry;
use colored::*;
use regex::Regex;

#[derive(Debug, Default, Clone)]
pub struct PromptDirectives {
    pub clean_prompt: String,
    pub targeted_skills: Vec<String>,
    pub targeted_tools: Vec<String>,
    pub targeted_mcp_servers: Vec<String>,
}

impl PromptDirectives {
    /// Parses '@' directives from user prompt (e.g. @os_navigation_vlm, @skill:metadata, @tool:capture_screen)
    pub fn parse_and_apply(
        raw_input: &str,
        skill_manager: &mut SkillManager,
        tool_registry: &mut ToolRegistry,
        mcp_manager: &McpManager,
    ) -> Self {
        let tag_regex = Regex::new(r#"(?i)@([a-zA-Z0-9_\-:/]+)"#).unwrap();

        let mut targeted_skills = Vec::new();
        let mut targeted_tools = Vec::new();
        let mut targeted_mcp = Vec::new();

        for cap in tag_regex.captures_iter(raw_input) {
            let full_tag = cap.get(1).map(|m| m.as_str()).unwrap_or("");
            let lower = full_tag.to_lowercase();

            if lower == "help" || lower.is_empty() {
                continue;
            }

            if lower.starts_with("skill:") || lower.starts_with("skill/") {
                let name = &full_tag[6..];
                if let Some(skill) = skill_manager.skills.get_mut(name) {
                    skill.is_active = true;
                    targeted_skills.push(skill.name.clone());
                } else if let Some((_, skill)) = skill_manager.skills.iter_mut().find(|(_, s)| s.name.eq_ignore_ascii_case(name)) {
                    skill.is_active = true;
                    targeted_skills.push(skill.name.clone());
                } else {
                    targeted_skills.push(name.to_string());
                }
            } else if lower.starts_with("tool:") || lower.starts_with("tool/") {
                let name = &full_tag[5..];
                if let Some(tool) = tool_registry.tools.get_mut(name) {
                    tool.enabled = true;
                    targeted_tools.push(tool.name.clone());
                } else if let Some((_, tool)) = tool_registry.tools.iter_mut().find(|(k, _)| k.eq_ignore_ascii_case(name)) {
                    tool.enabled = true;
                    targeted_tools.push(tool.name.clone());
                } else {
                    targeted_tools.push(name.to_string());
                }
            } else if lower.starts_with("mcp:") || lower.starts_with("mcp/") {
                let name = &full_tag[4..];
                targeted_mcp.push(name.to_string());
            } else {
                // Smart auto-resolution: skill -> tool -> mcp
                let mut matched = false;

                // 1. Check skill match
                if let Some(skill) = skill_manager.skills.get_mut(full_tag) {
                    skill.is_active = true;
                    targeted_skills.push(skill.name.clone());
                    matched = true;
                } else if let Some((_, skill)) = skill_manager.skills.iter_mut().find(|(k, s)| k.eq_ignore_ascii_case(full_tag) || s.name.eq_ignore_ascii_case(full_tag)) {
                    skill.is_active = true;
                    targeted_skills.push(skill.name.clone());
                    matched = true;
                }

                // 2. Check tool match
                if !matched {
                    if let Some(tool) = tool_registry.tools.get_mut(full_tag) {
                        tool.enabled = true;
                        targeted_tools.push(tool.name.clone());
                        matched = true;
                    } else if let Some((_, tool)) = tool_registry.tools.iter_mut().find(|(k, _)| k.eq_ignore_ascii_case(full_tag)) {
                        tool.enabled = true;
                        targeted_tools.push(tool.name.clone());
                        matched = true;
                    }
                }

                // 3. Check MCP server match
                if !matched {
                    if mcp_manager.clients.contains_key(full_tag) {
                        targeted_mcp.push(full_tag.to_string());
                    }
                }
            }
        }

        // Clean user prompt by stripping @ tags
        let clean = tag_regex.replace_all(raw_input, "").trim().to_string();
        let clean_prompt = if clean.is_empty() {
            raw_input.to_string()
        } else {
            clean
        };

        // Print visual confirmation badge if directives were found
        let mut badges = Vec::new();
        for s in &targeted_skills {
            badges.push(format!("Skill: {}", s).bright_cyan().bold().to_string());
        }
        for t in &targeted_tools {
            badges.push(format!("Tool: {}", t).bright_green().bold().to_string());
        }
        for m in &targeted_mcp {
            badges.push(format!("MCP: {}", m).bright_magenta().bold().to_string());
        }

        if !badges.is_empty() {
            println!(
                "{}",
                format!("📌 Target Focus: [{}]", badges.join(" • ")).bold()
            );
        }

        Self {
            clean_prompt,
            targeted_skills,
            targeted_tools,
            targeted_mcp_servers: targeted_mcp,
        }
    }

    /// Injects mandatory execution directives into the prompt
    pub fn build_directive_prompt(&self) -> String {
        let mut injection = String::new();

        if !self.targeted_skills.is_empty() {
            injection.push_str(&format!(
                "\n[USER DIRECTIVE: Prioritize and follow the methodology defined in skill(s): {}]\n",
                self.targeted_skills.join(", ")
            ));
        }

        if !self.targeted_tools.is_empty() {
            injection.push_str(&format!(
                "\n[MANDATORY TOOL DIRECTIVE: The user explicitly specified tool(s): {}. You MUST invoke the appropriate tool(s) from this list to fulfill the request.]\n",
                self.targeted_tools.join(", ")
            ));
        }

        if !self.targeted_mcp_servers.is_empty() {
            injection.push_str(&format!(
                "\n[MANDATORY MCP DIRECTIVE: Prioritize tools from MCP server(s): {}]\n",
                self.targeted_mcp_servers.join(", ")
            ));
        }

        injection
    }

    /// Displays interactive help for all available @ directive mentions
    pub fn print_help(skill_manager: &SkillManager, tool_registry: &ToolRegistry, mcp_manager: &McpManager) {
        println!("\n{}", "================ @ DIRECTIVES QUICK REFERENCE ================".cyan().bold());
        println!("  You can prefix or include '@' in any prompt to target a specific Skill, Tool, or MCP server.\n");

        println!("{}", "  Available @ Skills:".bold());
        for s in skill_manager.list_skills() {
            let active = if s.is_active { "[ACTIVE]".green() } else { "[DISABLED]".dimmed() };
            println!("    @{:<24} {} - {}", s.id.cyan().bold(), active, s.description.dimmed());
        }

        println!("\n{}", "  Available @ Tools:".bold());
        for t in tool_registry.tools.values() {
            let enabled = if t.enabled { "[READY]".green() } else { "[DISABLED]".dimmed() };
            println!("    @{:<24} {} - {}", t.name.green().bold(), enabled, t.description.dimmed());
        }

        println!("\n{}", "  Available @ MCP Servers:".bold());
        for (name, _, online) in mcp_manager.list_servers() {
            let badge = if online { "[ONLINE]".green() } else { "[OFFLINE]".dimmed() };
            println!("    @mcp:{:<20} {}", name.magenta().bold(), badge);
        }

        println!("\n  {} @os_navigation_vlm take screenshot and click start\n", "Example:".bold());
    }
}
