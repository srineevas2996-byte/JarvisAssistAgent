use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub default_model: String,
    pub models_dir: String,
    pub skills_dir: String,
    pub tools_dir: String,
    pub mcp_dir: String,
    pub temperature: f32,
    pub max_context_tokens: usize,
    #[serde(default = "default_num_ctx")]
    pub num_ctx: u64,
    pub max_agent_steps: usize,
    pub stream_output: bool,
    pub active_skills: Vec<String>,
    pub disabled_tools: Vec<String>,
}

fn default_num_ctx() -> u64 {
    16384
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            default_model: "mai-ui:2b".to_string(),
            models_dir: "models".to_string(),
            skills_dir: "skills".to_string(),
            tools_dir: "tools".to_string(),
            mcp_dir: "mcp".to_string(),
            temperature: 0.2,
            max_context_tokens: 16384,
            num_ctx: 16384,
            max_agent_steps: 15,
            stream_output: true,
            active_skills: vec!["jarvis_assistant".to_string()],
            disabled_tools: Vec::new(),
        }
    }
}

impl Settings {
    pub fn config_path() -> PathBuf {
        Path::new("config").join("config.json")
    }

    pub fn load() -> Self {
        let path = Self::config_path();
        if path.exists() {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(settings) = serde_json::from_str::<Settings>(&content) {
                    return settings;
                }
            }
        }
        let default_settings = Self::default();
        let _ = default_settings.save();
        default_settings
    }

    pub fn save(&self) -> Result<(), std::io::Error> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        fs::write(path, json)?;
        Ok(())
    }
}
