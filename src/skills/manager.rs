use crate::checkpoint::{ErrorCategory, ErrorCheckpoint};
use crate::skills::skill::Skill;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub struct SkillManager {
    pub skills_dir: PathBuf,
    pub skills: HashMap<String, Skill>,
}

impl SkillManager {
    pub fn new<P: AsRef<Path>>(skills_dir: P, active_skill_ids: &[String]) -> Self {
        let mut mgr = Self {
            skills_dir: skills_dir.as_ref().to_path_buf(),
            skills: HashMap::new(),
        };
        mgr.reload(active_skill_ids);
        mgr
    }

    pub fn reload(&mut self, active_skill_ids: &[String]) {
        self.skills.clear();
        if !self.skills_dir.exists() {
            let _ = std::fs::create_dir_all(&self.skills_dir);
            return;
        }

        for entry in WalkDir::new(&self.skills_dir)
            .max_depth(2)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if path.is_file() {
                if let Some(ext) = path.extension() {
                    if ext.to_string_lossy().to_lowercase() == "md" {
                        if let Ok(mut skill) = Skill::parse_file(path) {
                            if active_skill_ids.iter().any(|id| id.eq_ignore_ascii_case(&skill.id) || id.eq_ignore_ascii_case(&skill.name)) {
                                skill.is_active = true;
                            }
                            self.skills.insert(skill.id.clone(), skill);
                        }
                    }
                }
            }
        }
    }

    pub fn list_skills(&self) -> Vec<&Skill> {
        let mut list: Vec<&Skill> = self.skills.values().collect();
        list.sort_by(|a, b| a.name.cmp(&b.name));
        list
    }

    pub fn get_skill(&self, id_or_name: &str) -> Option<&Skill> {
        self.skills.get(id_or_name).or_else(|| {
            self.skills
                .values()
                .find(|s| s.name.eq_ignore_ascii_case(id_or_name))
        })
    }

    pub fn toggle_skill(&mut self, id_or_idx: &str, enable: Option<bool>) -> Result<(String, bool), String> {
        // Try numerical index
        if let Ok(idx) = id_or_idx.parse::<usize>() {
            let (target_id, count) = {
                let list = self.list_skills();
                if idx >= 1 && idx <= list.len() {
                    (Some(list[idx - 1].id.clone()), list.len())
                } else {
                    (None, list.len())
                }
            };

            if let Some(target_id) = target_id {
                if let Some(skill) = self.skills.get_mut(&target_id) {
                    let new_state = enable.unwrap_or(!skill.is_active);
                    skill.is_active = new_state;
                    return Ok((skill.name.clone(), new_state));
                }
            }
            return Err(ErrorCheckpoint::new(
                ErrorCategory::SkillNotFound,
                "Invalid Skill Index",
                &format!("Specified skill index [{}] is out of bounds.", idx),
                Some(&format!("Total available skills: {}", count)),
                "Type '/skill' to list recognized skills and select an index within range.",
            )
            .to_plain_string());
        }

        // Match by id, name, or filename
        let key = self
            .skills
            .iter()
            .find(|(k, s)| {
                k.eq_ignore_ascii_case(id_or_idx)
                    || s.name.eq_ignore_ascii_case(id_or_idx)
                    || s.file_path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .map(|f| f.eq_ignore_ascii_case(id_or_idx))
                        .unwrap_or(false)
            })
            .map(|(k, _)| k.clone());

        if let Some(k) = key {
            if let Some(skill) = self.skills.get_mut(&k) {
                let new_state = enable.unwrap_or(!skill.is_active);
                skill.is_active = new_state;
                return Ok((skill.name.clone(), new_state));
            }
        }
        Err(ErrorCheckpoint::new(
            ErrorCategory::SkillNotFound,
            "Skill File Not Found",
            &format!("No skill matching '{}' exists in skills/ folder.", id_or_idx),
            Some(&format!("Scanned directory: {}", self.skills_dir.display())),
            "Drop your custom *.md skill runbook into the skills/ folder, then type '/skill reload'.",
        )
        .to_plain_string())
    }

    pub fn get_active_ids(&self) -> Vec<String> {
        self.skills
            .values()
            .filter(|s| s.is_active)
            .map(|s| s.id.clone())
            .collect()
    }

    pub fn build_system_prompt(&self) -> String {
        let mut active: Vec<&Skill> = self.skills.values().filter(|s| s.is_active).collect();
        active.sort_by(|a, b| a.name.cmp(&b.name));

        if active.is_empty() {
            return String::new();
        }

        let mut prompt = String::from("## ACTIVE AGENT SKILLS & RUNBOOKS (INDEX & DESCRIPTIONS):\n\n");
        for (i, skill) in active.iter().enumerate() {
            prompt.push_str(&format!("[{}] Skill: {} (v{})\n", i + 1, skill.name, skill.version));
            prompt.push_str(&format!("    Description: {}\n", skill.description));
            prompt.push_str("    Runbook & Guidelines:\n");
            for line in skill.instructions.lines() {
                prompt.push_str(&format!("    {}\n", line));
            }
            prompt.push_str("\n---\n\n");
        }
        prompt
    }
}
