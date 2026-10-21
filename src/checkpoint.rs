use colored::*;
use std::fmt;

#[derive(Debug, Clone)]
pub enum ErrorCategory {
    LlmUnavailable,
    ModelLoadFailed,
    SkillNotFound,
    SkillParseError,
    McpUnreachable,
    McpExecutionFailed,
    ToolNonFunctional,
    ToolDisabled,
    InvalidParameters,
    MultimodalError,
    SystemError,
}

impl fmt::Display for ErrorCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErrorCategory::LlmUnavailable => write!(f, "LLM_UNAVAILABLE"),
            ErrorCategory::ModelLoadFailed => write!(f, "MODEL_LOAD_FAILED"),
            ErrorCategory::SkillNotFound => write!(f, "SKILL_NOT_FOUND"),
            ErrorCategory::SkillParseError => write!(f, "SKILL_PARSE_ERROR"),
            ErrorCategory::McpUnreachable => write!(f, "MCP_UNREACHABLE"),
            ErrorCategory::McpExecutionFailed => write!(f, "MCP_EXECUTION_FAILED"),
            ErrorCategory::ToolNonFunctional => write!(f, "TOOL_NON_FUNCTIONAL"),
            ErrorCategory::ToolDisabled => write!(f, "TOOL_DISABLED"),
            ErrorCategory::InvalidParameters => write!(f, "INVALID_PARAMETERS"),
            ErrorCategory::MultimodalError => write!(f, "MULTIMODAL_ERROR"),
            ErrorCategory::SystemError => write!(f, "SYSTEM_ERROR"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ErrorCheckpoint {
    pub category: ErrorCategory,
    pub title: String,
    pub reason: String,
    pub details: Option<String>,
    pub resolution: String,
}

impl ErrorCheckpoint {
    pub fn new(
        category: ErrorCategory,
        title: &str,
        reason: &str,
        details: Option<&str>,
        resolution: &str,
    ) -> Self {
        Self {
            category,
            title: title.to_string(),
            reason: reason.to_string(),
            details: details.map(|d| d.to_string()),
            resolution: resolution.to_string(),
        }
    }

    pub fn format_terminal(&self) -> String {
        let tag = format!("[CHECKPOINT: {}]", self.category).red().bold();
        let title = self.title.bold();

        let mut out = format!("\n❌ {} {}\n", tag, title);
        out.push_str(&format!("   {:<12} {}\n", "Reason:".bright_white().bold(), self.reason));

        if let Some(ref d) = self.details {
            let trimmed = d.trim();
            if !trimmed.is_empty() {
                out.push_str(&format!("   {:<12} {}\n", "Details:".dimmed().bold(), trimmed.dimmed()));
            }
        }

        out.push_str(&format!("   {:<12} {}\n", "Resolution:".cyan().bold(), self.resolution.cyan()));
        out
    }

    pub fn to_plain_string(&self) -> String {
        let mut out = format!("[CHECKPOINT: {}] {}\n", self.category, self.title);
        out.push_str(&format!("Reason: {}\n", self.reason));
        if let Some(ref d) = self.details {
            out.push_str(&format!("Details: {}\n", d.trim()));
        }
        out.push_str(&format!("Resolution: {}\n", self.resolution));
        out
    }
}

impl fmt::Display for ErrorCheckpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.format_terminal())
    }
}
