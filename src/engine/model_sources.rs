use crate::checkpoint::{ErrorCategory, ErrorCheckpoint};
use crate::cli::ProgressSpinner;
use crate::engine::gguf_parser::{GgufModelInfo, GgufParser};
use futures_util::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;
use walkdir::WalkDir;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ModelOrigin {
    HuggingFace,
    OllamaCache,
    LocalGguf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedModel {
    pub display_name: String,
    pub source: ModelOrigin,
    pub gguf_path: PathBuf,
    pub mmproj_path: Option<PathBuf>,
    pub file_size_mb: f64,
    pub architecture: String,
    pub quant_type: String,
    pub context_length: u64,
    pub has_vision: bool,
    pub model_family: Option<String>,
    pub model_families: Vec<String>,
    pub template: Option<String>,
    pub stop_tokens: Vec<String>,
    pub system_prompt: Option<String>,
    pub vision_encoder_type: Option<String>,
}

pub struct ModelSourceManager;

impl ModelSourceManager {
    /// Discovers all models cached by Ollama in %USERPROFILE%\.ollama\models\manifests,
    /// extracting complete manifest layers (config JSON, prompt template, stop params, and vision projectors)
    pub fn scan_ollama_cache() -> Vec<UnifiedModel> {
        let mut models = Vec::new();

        let user_profile = match std::env::var("USERPROFILE") {
            Ok(p) => PathBuf::from(p),
            Err(_) => return models,
        };

        let manifests_dir = user_profile.join(".ollama").join("models").join("manifests");
        let blobs_dir = user_profile.join(".ollama").join("models").join("blobs");

        if !manifests_dir.exists() || !blobs_dir.exists() {
            return models;
        }

        for entry in WalkDir::new(&manifests_dir)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if path.is_file() {
                // Determine model name from relative path under manifests (e.g. registry.ollama.ai/library/qwen3-vl/2b -> qwen3-vl:2b)
                if let Ok(rel) = path.strip_prefix(&manifests_dir) {
                    let comps: Vec<String> = rel.iter().map(|c| c.to_string_lossy().to_string()).collect();
                    if comps.len() >= 2 {
                        let name_part = &comps[comps.len() - 2];
                        let tag_part = &comps[comps.len() - 1];
                        let model_tag = format!("{}:{}", name_part, tag_part);

                        if let Ok(manifest_content) = std::fs::read_to_string(path) {
                            if let Ok(json_val) = serde_json::from_str::<Value>(&manifest_content) {
                                // 1. Read config blob for model_family, model_families, etc.
                                let mut model_family: Option<String> = None;
                                let mut model_families: Vec<String> = Vec::new();

                                if let Some(cdigest) = json_val.get("config").and_then(|c| c.get("digest")).and_then(|d| d.as_str()) {
                                    let clean_c = cdigest.replace(':', "-");
                                    let cpath = blobs_dir.join(&clean_c);
                                    if let Ok(ccontent) = std::fs::read_to_string(&cpath) {
                                        if let Ok(cjson) = serde_json::from_str::<Value>(&ccontent) {
                                            model_family = cjson.get("model_family").and_then(|f| f.as_str()).map(|s| s.to_string());
                                            if let Some(fams) = cjson.get("model_families").and_then(|f| f.as_array()) {
                                                model_families = fams.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect();
                                            }
                                        }
                                    }
                                }

                                // 2. Parse layers
                                let mut model_digest: Option<String> = None;
                                let mut mmproj_digest: Option<String> = None;
                                let mut template_digest: Option<String> = None;
                                let mut params_digest: Option<String> = None;
                                let mut system_digest: Option<String> = None;

                                if let Some(layers) = json_val.get("layers").and_then(|l| l.as_array()) {
                                    for layer in layers {
                                        let media_type = layer.get("mediaType").and_then(|m| m.as_str()).unwrap_or("");
                                        let digest = layer.get("digest").and_then(|d| d.as_str()).unwrap_or("");

                                        if media_type.contains("model") {
                                            model_digest = Some(digest.to_string());
                                        } else if media_type.contains("projector") {
                                            mmproj_digest = Some(digest.to_string());
                                        } else if media_type.contains("template") {
                                            template_digest = Some(digest.to_string());
                                        } else if media_type.contains("params") {
                                            params_digest = Some(digest.to_string());
                                        } else if media_type.contains("system") {
                                            system_digest = Some(digest.to_string());
                                        }
                                    }
                                }

                                // Read template blob
                                let template = template_digest.and_then(|d| {
                                    let clean = d.replace(':', "-");
                                    std::fs::read_to_string(blobs_dir.join(&clean)).ok()
                                });

                                // Read stop tokens and parameters
                                let mut stop_tokens: Vec<String> = Vec::new();
                                if let Some(d) = params_digest {
                                    let clean = d.replace(':', "-");
                                    if let Ok(pcontent) = std::fs::read_to_string(blobs_dir.join(&clean)) {
                                        if let Ok(pjson) = serde_json::from_str::<Value>(&pcontent) {
                                            if let Some(stops) = pjson.get("stop").and_then(|s| s.as_array()) {
                                                for v in stops {
                                                    if let Some(s) = v.as_str() {
                                                        if s.starts_with('[') && s.ends_with(']') {
                                                            if let Ok(sub) = serde_json::from_str::<Vec<String>>(s) {
                                                                stop_tokens.extend(sub);
                                                                continue;
                                                            }
                                                        }
                                                        stop_tokens.push(s.to_string());
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }

                                // Read default system prompt
                                let system_prompt = system_digest.and_then(|d| {
                                    let clean = d.replace(':', "-");
                                    std::fs::read_to_string(blobs_dir.join(&clean)).ok()
                                });

                                if let Some(digest) = model_digest {
                                    let clean_digest = digest.replace(':', "-");
                                    let blob_path = blobs_dir.join(&clean_digest);

                                    if blob_path.exists() {
                                        let mmproj_path = mmproj_digest.map(|d| {
                                            blobs_dir.join(d.replace(':', "-"))
                                        }).filter(|p| p.exists());

                                        let is_vision_tag = model_tag.contains("-vl") || model_tag.contains("vision") || model_tag.contains("mai-ui");
                                        let has_vision = mmproj_path.is_some() || is_vision_tag;

                                        // Try reading GGUF header from the blob
                                        if let Ok(info) = GgufParser::parse_file(&blob_path) {
                                            let arch_lower = info.architecture.to_lowercase();
                                            let is_vl_arch = arch_lower.contains("vl") || arch_lower.contains("vision");

                                            // Determine vision encoder type: GGUF header or Ollama manifest family fallback
                                            let mut vision_encoder_type = info.vision_encoder_type.clone();
                                            if vision_encoder_type.is_none() {
                                                let fams_lower: Vec<String> = model_families.iter().map(|f| f.to_lowercase()).collect();
                                                if fams_lower.iter().any(|f| f.contains("qwen3vl") || f.contains("qwen2vl")) {
                                                    vision_encoder_type = Some("Qwen-VL ViT".to_string());
                                                } else if fams_lower.iter().any(|f| f.contains("minicpm")) {
                                                    vision_encoder_type = Some("MiniCPM-V ViT".to_string());
                                                } else if fams_lower.iter().any(|f| f.contains("clip")) {
                                                    vision_encoder_type = Some("CLIP ViT".to_string());
                                                }
                                            }

                                            // Fallback stop tokens if empty in params
                                            if stop_tokens.is_empty() {
                                                stop_tokens = vec![
                                                    "<|im_end|>".to_string(),
                                                    "<|endoftext|>".to_string(),
                                                ];
                                            }

                                            models.push(UnifiedModel {
                                                display_name: model_tag,
                                                source: ModelOrigin::OllamaCache,
                                                gguf_path: blob_path,
                                                mmproj_path,
                                                file_size_mb: info.file_size_mb,
                                                architecture: info.architecture,
                                                quant_type: info.quant_type,
                                                context_length: info.context_length,
                                                has_vision: has_vision || info.is_multimodal_projector || is_vl_arch,
                                                model_family,
                                                model_families,
                                                template,
                                                stop_tokens,
                                                system_prompt,
                                                vision_encoder_type,
                                            });
                                        } else {
                                            let meta = std::fs::metadata(&blob_path);
                                            let size_mb = meta.map(|m| (m.len() as f64) / (1024.0 * 1024.0)).unwrap_or(0.0);
                                            models.push(UnifiedModel {
                                                display_name: model_tag,
                                                source: ModelOrigin::OllamaCache,
                                                gguf_path: blob_path,
                                                mmproj_path,
                                                file_size_mb: size_mb,
                                                architecture: model_family.clone().unwrap_or_else(|| "gguf".to_string()),
                                                quant_type: "quantized".to_string(),
                                                context_length: 4096,
                                                has_vision,
                                                model_family,
                                                model_families,
                                                template,
                                                stop_tokens,
                                                system_prompt,
                                                vision_encoder_type: None,
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        models
    }

    /// Pulls / downloads a GGUF model directly from HuggingFace with live progress spinner
    pub async fn pull_from_huggingface(
        hf_spec_or_url: &str,
        dest_dir: &Path,
    ) -> Result<PathBuf, String> {
        let (download_url, file_name) = if hf_spec_or_url.starts_with("http://") || hf_spec_or_url.starts_with("https://") {
            let parsed_url = reqwest::Url::parse(hf_spec_or_url)
                .map_err(|e| format!("Invalid URL: {}", e))?;
            let fname = parsed_url
                .path_segments()
                .and_then(|mut s| s.next_back())
                .unwrap_or("model.gguf")
                .to_string();
            (hf_spec_or_url.to_string(), fname)
        } else {
            // Assume format: hf:<owner>/<repo>/<filename> or <owner>/<repo>/<filename>
            let clean = hf_spec_or_url.trim_start_matches("hf:");
            let parts: Vec<&str> = clean.split('/').collect();
            if parts.len() < 3 {
                return Err("HuggingFace spec must be 'hf:<owner>/<repo>/<filename.gguf>' or a direct HTTPS URL.".to_string());
            }
            let owner = parts[0];
            let repo = parts[1];
            let fname = parts[2..].join("/");
            let url = format!("https://huggingface.co/{}/{}/resolve/main/{}", owner, repo, fname);
            (url, parts.last().unwrap_or(&"model.gguf").to_string())
        };

        if !dest_dir.exists() {
            let _ = std::fs::create_dir_all(dest_dir);
        }

        let target_file = dest_dir.join(&file_name);
        if target_file.exists() {
            return Ok(target_file);
        }

        let client = Client::builder()
            .timeout(Duration::from_secs(3600))
            .build()
            .unwrap_or_default();

        let spinner = ProgressSpinner::start(&format!("Connecting to HuggingFace: {}...", file_name));

        let res = match client.get(&download_url).send().await {
            Ok(r) => r,
            Err(e) => {
                spinner.stop_with_error(&format!("Connection failed: {}", e));
                return Err(format!("Failed to connect to HuggingFace at {}: {}", download_url, e));
            }
        };

        if !res.status().is_success() {
            let status = res.status();
            spinner.stop_with_error(&format!("Download failed with HTTP {}", status));
            return Err(format!("HuggingFace returned HTTP status {}: {}", status, download_url));
        }

        let total_size = res.content_length().unwrap_or(0);
        let mut stream = res.bytes_stream();

        let mut file = tokio::fs::File::create(&target_file)
            .await
            .map_err(|e| format!("Failed to create destination file: {}", e))?;

        use tokio::io::AsyncWriteExt;
        let mut downloaded: u64 = 0;
        let mut last_update = std::time::Instant::now();

        while let Some(chunk_res) = stream.next().await {
            let chunk = chunk_res.map_err(|e| format!("Download stream error: {}", e))?;
            file.write_all(&chunk)
                .await
                .map_err(|e| format!("Failed to write chunk to file: {}", e))?;

            downloaded += chunk.len() as u64;

            if last_update.elapsed() >= Duration::from_millis(400) {
                last_update = std::time::Instant::now();
                let dl_mb = (downloaded as f64) / (1024.0 * 1024.0);
                if total_size > 0 {
                    let total_mb = (total_size as f64) / (1024.0 * 1024.0);
                    let pct = ((downloaded as f64) / (total_size as f64)) * 100.0;
                    spinner.update(&format!(
                        "Downloading {:.1} MB / {:.1} MB ({:.1}%) - {}...",
                        dl_mb, total_mb, pct, file_name
                    ));
                } else {
                    spinner.update(&format!("Downloading {:.1} MB - {}...", dl_mb, file_name));
                }
            }
        }

        file.flush().await.map_err(|e| format!("Failed to flush file: {}", e))?;
        drop(file);

        spinner.stop_with_success(&format!(
            "Successfully downloaded '{file_name}' ({:.1} MB)",
            (downloaded as f64) / (1024.0 * 1024.0)
        ));

        // Validate GGUF header
        if let Err(e) = GgufParser::parse_file(&target_file) {
            println!("  ⚠️ Warning: Downloaded file header validation: {}", e);
        }

        Ok(target_file)
    }

    /// Imports / links an Ollama cached model directly into the local models/ folder
    pub fn import_ollama_model(model_tag: &str, dest_dir: &Path) -> Result<PathBuf, String> {
        let cached = Self::scan_ollama_cache();
        let target = cached.iter().find(|m| m.display_name.eq_ignore_ascii_case(model_tag))
            .ok_or_else(|| format!("Model '{}' was not found in local Ollama cache.", model_tag))?;

        let clean_name = target.display_name.replace(':', "_").replace('/', "_");
        let dest_path = dest_dir.join(format!("{}.gguf", clean_name));

        if !dest_dir.exists() {
            let _ = std::fs::create_dir_all(dest_dir);
        }

        if dest_path.exists() {
            return Ok(dest_path);
        }

        // Try creating a hard link first (instant, 0 disk space used)
        if std::fs::hard_link(&target.gguf_path, &dest_path).is_ok() {
            return Ok(dest_path);
        }

        // Fall back to copy
        std::fs::copy(&target.gguf_path, &dest_path)
            .map_err(|e| format!("Failed to copy model weights from Ollama cache: {}", e))?;

        Ok(dest_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scan_ollama_cache_metadata() {
        let models = ModelSourceManager::scan_ollama_cache();
        println!("\n=== Discovered Ollama Cache Models ({}) ===", models.len());
        for m in &models {
            println!("- Display Name: {}", m.display_name);
            println!("  Architecture: {}", m.architecture);
            println!("  Family: {:?}", m.model_family);
            println!("  Families: {:?}", m.model_families);
            println!("  Has Vision: {}", m.has_vision);
            println!("  ViT Encoder: {:?}", m.vision_encoder_type);
            println!("  Stop Tokens: {:?}", m.stop_tokens);
            println!("  Projector Path: {:?}", m.mmproj_path);
            println!("  Size MB: {:.1}", m.file_size_mb);
        }
        println!("===========================================\n");
        assert!(!models.is_empty(), "Expected at least one Ollama model in local cache");
    }
}
