use crate::checkpoint::{ErrorCategory, ErrorCheckpoint};
use crate::cli::ProgressSpinner;
use crate::engine::gguf_parser::{GgufModelInfo, GgufParser};
use crate::engine::model_sources::{ModelOrigin, ModelSourceManager, UnifiedModel};
use crate::engine::pipe_client::PipeEngineClient;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub images: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
pub enum StreamEvent<'a> {
    Thinking(&'a str),
    Token(&'a str),
}

pub struct LLMEngine {
    pub active_model: Option<GgufModelInfo>,
    pub models_dir: PathBuf,
    pub temperature: f32,
    pub num_ctx: u64,
    pub pipe_engine: Option<PipeEngineClient>,
}

impl LLMEngine {
    pub fn new(models_dir: &Path, temperature: f32, num_ctx: u64) -> Self {
        Self {
            active_model: None,
            models_dir: models_dir.to_path_buf(),
            temperature,
            num_ctx,
            pipe_engine: None,
        }
    }

    /// Scan models/ and C:\models directories for all local .gguf files
    pub fn scan_models(&self) -> Vec<GgufModelInfo> {
        let mut list = Vec::new();
        let mut scan_dirs = vec![self.models_dir.clone()];
        let c_models = PathBuf::from("C:\\models");
        if c_models.exists() && !scan_dirs.contains(&c_models) {
            scan_dirs.push(c_models);
        }

        for dir in scan_dirs {
            if !dir.exists() {
                let _ = std::fs::create_dir_all(&dir);
                continue;
            }

            for entry in WalkDir::new(&dir)
                .max_depth(2)
                .into_iter()
                .filter_map(|e| e.ok())
            {
                let path = entry.path();
                if path.is_file() {
                    if let Some(ext) = path.extension() {
                        if ext.to_string_lossy().to_lowercase() == "gguf" {
                            if let Ok(info) = GgufParser::parse_file(path) {
                                if !list.iter().any(|m: &GgufModelInfo| m.file_path == info.file_path || m.file_name == info.file_name) {
                                    list.push(info);
                                }
                            }
                        }
                    }
                }
            }
        }
        list
    }

    /// Scans both local GGUFs in models/ (HuggingFace) and standalone Ollama cache GGUF blobs
    pub fn scan_unified_models(&self) -> Vec<UnifiedModel> {
        let mut list = Vec::new();

        // 1. Local GGUF models in models/ folder
        for g in self.scan_models() {
            let arch_lower = g.architecture.to_lowercase();
            let has_vision = g.is_multimodal_projector
                || arch_lower.contains("vl")
                || arch_lower.contains("vision")
                || g.file_name.to_lowercase().contains("vl")
                || g.file_name.to_lowercase().contains("mai-ui");
            list.push(UnifiedModel {
                display_name: g.file_name.clone(),
                source: ModelOrigin::LocalGguf,
                gguf_path: g.file_path.clone(),
                mmproj_path: None,
                file_size_mb: g.file_size_mb,
                architecture: g.architecture.clone(),
                quant_type: g.quant_type.clone(),
                context_length: g.context_length,
                has_vision,
                model_family: Some(g.architecture.clone()),
                model_families: vec![g.architecture.clone()],
                template: None,
                stop_tokens: vec!["<|im_end|>".to_string(), "<|endoftext|>".to_string()],
                system_prompt: None,
                vision_encoder_type: g.vision_encoder_type.clone(),
            });
        }

        // 2. Ollama local cache models (read directly from disk GGUF blobs, no daemon needed)
        for om in ModelSourceManager::scan_ollama_cache() {
            if !list.iter().any(|m| m.display_name.eq_ignore_ascii_case(&om.display_name)) {
                list.push(om);
            }
        }

        list
    }

    /// Activates a unified model by spawning the jarvis-engine pipe subprocess (Zero HTTP sockets)
    pub async fn activate_unified_model(&mut self, target: &UnifiedModel) -> Result<GgufModelInfo, String> {
        // Stop any running pipe engine first
        if let Some(mut existing) = self.pipe_engine.take() {
            existing.stop().await;
        }

        let ctx = if self.num_ctx > 0 { self.num_ctx } else { 16384 };

        let mmproj_target = target.mmproj_path.as_deref().or_else(|| {
            if target.has_vision {
                Some(&target.gguf_path)
            } else {
                None
            }
        });

        let pipe_client = PipeEngineClient::spawn(
            &target.gguf_path,
            mmproj_target,
            ctx,
            self.temperature,
            target.stop_tokens.clone(),
            target.vision_encoder_type.clone(),
        ).await?;

        let info = if let Ok(parsed) = GgufParser::parse_file(&target.gguf_path) {
            parsed
        } else {
            GgufModelInfo {
                file_name: target.display_name.clone(),
                file_path: target.gguf_path.clone(),
                file_size_bytes: (target.file_size_mb * 1024.0 * 1024.0) as u64,
                file_size_mb: target.file_size_mb,
                architecture: target.architecture.clone(),
                model_name: target.display_name.clone(),
                context_length: target.context_length,
                tensor_count: 0,
                quant_type: target.quant_type.clone(),
                is_multimodal_projector: target.has_vision,
                embedding_length: None,
                block_count: None,
                head_count: None,
                vision_projector_type: None,
                vision_image_size: None,
                vision_patch_size: None,
                tokenizer_model: None,
                has_vision: target.has_vision,
                vision_encoder_type: target.vision_encoder_type.clone(),
            }
        };

        self.pipe_engine = Some(pipe_client);
        self.active_model = Some(info.clone());

        Ok(info)
    }

    /// Select and load a model by index, filename, or model tag
    pub async fn load_model(&mut self, model_name_or_file: &str) -> Result<GgufModelInfo, String> {
        let unified_models = self.scan_unified_models();

        // 0. Numerical index match
        if let Ok(idx) = model_name_or_file.parse::<usize>() {
            if idx >= 1 && idx <= unified_models.len() {
                let target = &unified_models[idx - 1];
                return self.activate_unified_model(target).await;
            }

            return Err(ErrorCheckpoint::new(
                ErrorCategory::ModelLoadFailed,
                "Invalid Model Index",
                &format!("Requested model index [{}] is out of bounds.", idx),
                Some(&format!("Total available models: {}", unified_models.len())),
                "Type '/model' to view available models and choose a valid index.",
            ).to_plain_string());
        }

        // 1. Direct match on unified models (Local GGUF or Ollama cache)
        let query_clean = model_name_or_file.trim();
        let query_norm = query_clean.replace(':', "-").replace(".gguf", "").to_lowercase();
        if let Some(target) = unified_models.iter().find(|m| {
            let d_clean = m.display_name.trim();
            let d_norm = d_clean.replace(':', "-").replace(".gguf", "").to_lowercase();
            m.display_name.eq_ignore_ascii_case(query_clean)
                || m.display_name.to_lowercase().starts_with(&query_clean.to_lowercase())
                || d_norm == query_norm
                || d_norm.starts_with(&query_norm)
        }) {
            return self.activate_unified_model(target).await;
        }

        // 2. Direct path to GGUF file
        let path = Path::new(model_name_or_file);
        if path.exists() && path.is_file() {
            let info = GgufParser::parse_file(path).map_err(|e| e.to_string())?;
            let arch_lower = info.architecture.to_lowercase();
            let has_vision = info.is_multimodal_projector
                || arch_lower.contains("vl")
                || arch_lower.contains("vision")
                || info.file_name.to_lowercase().contains("vl")
                || info.file_name.to_lowercase().contains("mai-ui");
            let unified = UnifiedModel {
                display_name: info.file_name.clone(),
                source: ModelOrigin::LocalGguf,
                gguf_path: path.to_path_buf(),
                mmproj_path: None,
                file_size_mb: info.file_size_mb,
                architecture: info.architecture.clone(),
                quant_type: info.quant_type.clone(),
                context_length: info.context_length,
                has_vision,
                model_family: Some(info.architecture.clone()),
                model_families: vec![info.architecture.clone()],
                template: None,
                stop_tokens: vec!["<|im_end|>".to_string(), "<|endoftext|>".to_string()],
                system_prompt: None,
                vision_encoder_type: info.vision_encoder_type.clone(),
            };
            return self.activate_unified_model(&unified).await;
        }

        Err(ErrorCheckpoint::new(
            ErrorCategory::ModelLoadFailed,
            "Model Not Found",
            &format!("No GGUF file matching '{}' was found.", model_name_or_file),
            Some(&format!("Drop .gguf into '{}' or pull with '/model pull hf:<repo>/<file>'.", self.models_dir.display())),
            "Type '/model' to view recognized models and options.",
        ).to_plain_string())
    }

    /// Pre-warms and pins the model in memory/VRAM (handled automatically by jarvis-engine pipe subprocess)
    pub async fn preload_model(&self, _model_name: &str) -> Result<(), String> {
        Ok(())
    }

    /// Loads a model and permanently locks it into GPU VRAM/RAM with interactive progress spinner
    pub async fn load_and_pin_model(
        &mut self,
        model_name_or_file: &str,
        show_progress: bool,
    ) -> Result<GgufModelInfo, String> {
        let spinner = if show_progress {
            Some(ProgressSpinner::start(&format!(
                "Locking model '{}' into GPU memory via pipe subprocess...",
                model_name_or_file
            )))
        } else {
            None
        };

        let res = self.load_model(model_name_or_file).await;
        match res {
            Ok(info) => {
                if let Some(sp) = spinner {
                    sp.stop_with_success(&format!(
                        "Model '{}' permanently locked in GPU memory (pipe subprocess, 0 network sockets)",
                        info.file_name
                    ));
                }
                Ok(info)
            }
            Err(e) => {
                if let Some(sp) = spinner {
                    sp.stop_with_error(&format!("Failed to load model '{}'", model_name_or_file));
                }
                Err(e)
            }
        }
    }

    /// Stream chat completion token-by-token over the pipe subprocess supporting text, reasoning thoughts, and images
    pub async fn stream_chat<F>(
        &mut self,
        messages: &[ChatMessage],
        tools_schema: Option<&[Value]>,
        on_event: F,
    ) -> Result<(String, f32), String>
    where
        F: FnMut(StreamEvent),
    {
        // If pipe engine is not currently running, attempt loading active model or first available model
        if self.pipe_engine.is_none() {
            let target_name = self.active_model.as_ref()
                .map(|m| m.file_name.clone())
                .unwrap_or_default();

            if !target_name.is_empty() {
                self.load_model(&target_name).await?;
            } else {
                let unified = self.scan_unified_models();
                if let Some(first) = unified.first() {
                    self.activate_unified_model(first).await?;
                } else {
                    return Err(ErrorCheckpoint::new(
                        ErrorCategory::ModelLoadFailed,
                        "No Active Model Loaded",
                        "No GGUF models found in models/ directory or Ollama cache.",
                        None,
                        "Drop a .gguf into models/ or pull with '/model pull hf:<repo>/<file>'.",
                    ).to_plain_string());
                }
            }
        }

        let pipe = self.pipe_engine.as_mut().ok_or("Inference pipe engine is unavailable.")?;

        pipe.stream_chat(messages, tools_schema, self.temperature, on_event).await
    }

    /// Primes and pre-warms the KV cache with the static system prompt prefix
    pub async fn prewarm_kv_cache(&mut self, system_prompt: &str) -> Result<(), String> {
        if let Some(ref mut pipe) = self.pipe_engine {
            pipe.prewarm_kv_cache(system_prompt).await
        } else {
            Ok(())
        }
    }
}
