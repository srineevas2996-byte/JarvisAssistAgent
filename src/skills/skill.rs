use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillMetadata {
    pub name: String,
    pub description: String,
    #[serde(default = "default_version")]
    pub version: String,
}

fn default_version() -> String {
    "1.0.0".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub instructions: String,
    pub file_path: PathBuf,
    pub is_active: bool,
}

impl Skill {
    pub fn parse_file<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let p = path.as_ref();
        let content = fs::read_to_string(p).map_err(|e| format!("Failed to read skill file: {}", e))?;

        let id = p
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("skill")
            .to_string();

        let (metadata, instructions) = Self::extract_frontmatter(&content)?;

        Ok(Self {
            id,
            name: metadata.name,
            description: metadata.description,
            version: metadata.version,
            instructions,
            file_path: p.to_path_buf(),
            is_active: false,
        })
    }

    fn extract_frontmatter(content: &str) -> Result<(SkillMetadata, String), String> {
        let trimmed = content.trim_start();
        if trimmed.starts_with("---") {
            let rest = &trimmed[3..];
            if let Some(end_idx) = rest.find("---") {
                let yaml_str = &rest[..end_idx];
                let body = rest[end_idx + 3..].trim().to_string();

                let metadata: SkillMetadata = serde_yaml::from_str(yaml_str)
                    .map_err(|e| format!("YAML frontmatter parsing error: {}", e))?;

                return Ok((metadata, body));
            }
        }

        // Fallback if no frontmatter
        let metadata = SkillMetadata {
            name: "Custom Skill".to_string(),
            description: "No description provided".to_string(),
            version: "1.0.0".to_string(),
        };
        Ok((metadata, content.to_string()))
    }
}
