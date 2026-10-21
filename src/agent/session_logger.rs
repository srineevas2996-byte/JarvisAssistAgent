use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use crate::engine::ChatMessage;
use crate::tools::executor::{AgentMode, ToolAction};

pub struct TaskSessionLogger {
    path: PathBuf,
}

impl TaskSessionLogger {
    /// Creates or truncates the session log file for a new task session
    pub fn init(user_prompt: &str) -> Self {
        let _ = std::fs::create_dir_all("cache");
        let path = PathBuf::from("cache/task_session.log");
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

        let header = format!(
            "================================================================================\n\
             J.A.A (jarvis_assist_agent) TASK SESSION LOG (Single-Task Active Trace)\n\
             TIMESTAMP: {}\n\
             USER GOAL: {}\n\
             ================================================================================\n\n",
            now, user_prompt
        );

        if let Ok(mut f) = File::create(&path) {
            let _ = f.write_all(header.as_bytes());
        }

        Self { path }
    }

    pub fn log_step_start(&self, step: usize, messages: &[ChatMessage]) {
        let mut entry = format!(
            "--------------------------------------------------------------------------------\n\
             [STEP {} START]\n\
             Messages fed to LLM (count: {}):\n\
             --------------------------------------------------------------------------------\n",
            step, messages.len()
        );

        for (i, m) in messages.iter().enumerate() {
            entry.push_str(&format!(
                ">> Message [{}]: role = {}\n{}\n\n",
                i, m.role, m.content
            ));
            if let Some(ref imgs) = m.images {
                entry.push_str(&format!("   [Attached in-RAM images count: {}]\n\n", imgs.len()));
            }
        }

        self.append(&entry);
    }

    pub fn log_model_output(
        &self,
        step: usize,
        raw_output: &str,
        chat: &str,
        mode: AgentMode,
        action: &ToolAction,
        time_taken: f32,
    ) {
        let mode_str = match mode {
            AgentMode::Chat => "chat",
            AgentMode::Task => "task",
        };

        let action_str = match action {
            ToolAction::None => "None (Chat response)".to_string(),
            ToolAction::TaskCompleted => "TaskCompleted".to_string(),
            ToolAction::Execute(call) => format!(
                "Execute tool '{}'\n   Arguments: {}\n   Raw Block:\n{}",
                call.tool_name,
                serde_json::to_string_pretty(&call.arguments).unwrap_or_else(|_| call.arguments.to_string()),
                call.raw_matched
            ),
        };

        let entry = format!(
            "--------------------------------------------------------------------------------\n\
             [STEP {} MODEL EVALUATION] (Net Inference Time: {:.2}s)\n\
             Raw Model Generation:\n\
             {}\n\n\
             Resolved Mode: {}\n\
             Chat Output: {}\n\
             Resolved Action:\n\
             {}\n\
             --------------------------------------------------------------------------------\n\n",
            step, time_taken, raw_output, mode_str, chat, action_str
        );

        self.append(&entry);
    }

    pub fn log_tool_result(
        &self,
        step: usize,
        tool_name: &str,
        arguments: &str,
        is_err: bool,
        output: &str,
        ram_frame_captured: bool,
    ) {
        let status = if is_err { "FAILED (Error)" } else { "SUCCESS" };
        let frame_status = if ram_frame_captured {
            "Yes (1920x1080 overwritten in RAM)"
        } else {
            "None"
        };

        let entry = format!(
            "--------------------------------------------------------------------------------\n\
             [STEP {} TOOL EXECUTION]\n\
             Tool: {}({})\n\
             Status: {}\n\
             Output:\n\
             {}\n\
             In-RAM Perception Frame: {}\n\
             --------------------------------------------------------------------------------\n\n",
            step, tool_name, arguments, status, output.trim(), frame_status
        );

        self.append(&entry);
    }

    pub fn log_step_observation(&self, step: usize, observation: &str) {
        let entry = format!(
            "[STEP {} NEXT OBSERVATION SYNTHESIS]\n\
             {}\n\
             --------------------------------------------------------------------------------\n\n",
            step, observation
        );
        self.append(&entry);
    }

    pub fn log_session_end(&self, reason: &str, total_steps: usize) {
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let entry = format!(
            "================================================================================\n\
             [SESSION CONCLUDED]\n\
             REASON: {}\n\
             TOTAL STEPS EXECUTED: {}\n\
             FINISHED AT: {}\n\
             ================================================================================\n",
            reason, total_steps, now
        );
        self.append(&entry);
    }

    fn append(&self, text: &str) {
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&self.path) {
            let _ = f.write_all(text.as_bytes());
        }
    }
}
