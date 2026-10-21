use byteorder::{LittleEndian, ReadBytesExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

const GGUF_MAGIC: u32 = 0x46554747; // "GGUF" in little endian

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GgufModelInfo {
    pub file_name: String,
    pub file_path: PathBuf,
    pub file_size_bytes: u64,
    pub file_size_mb: f64,
    pub architecture: String,
    pub model_name: String,
    pub context_length: u64,
    pub tensor_count: u64,
    pub quant_type: String,
    pub is_multimodal_projector: bool,
    pub embedding_length: Option<u64>,
    pub block_count: Option<u64>,
    pub head_count: Option<u64>,
    pub vision_projector_type: Option<String>,
    pub vision_image_size: Option<u32>,
    pub vision_patch_size: Option<u32>,
    pub tokenizer_model: Option<String>,
    pub has_vision: bool,
    pub vision_encoder_type: Option<String>,
}

pub struct GgufParser;

impl GgufParser {
    pub fn parse_file(path: &Path) -> Result<GgufModelInfo, String> {
        let file = File::open(path).map_err(|e| format!("Failed to open file: {}", e))?;
        let metadata = file.metadata().map_err(|e| format!("Failed to read metadata: {}", e))?;
        let file_size_bytes = metadata.len();
        let file_size_mb = (file_size_bytes as f64) / (1024.0 * 1024.0);

        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        let is_multimodal_projector = file_name.to_lowercase().starts_with("mmproj")
            || file_name.to_lowercase().contains("projector");

        let mut reader = BufReader::new(file);

        // Read magic
        let magic = reader
            .read_u32::<LittleEndian>()
            .map_err(|e| format!("Failed to read magic: {}", e))?;
        if magic != GGUF_MAGIC {
            return Err("Not a valid GGUF file (magic header mismatch)".to_string());
        }

        // Read version
        let version = reader
            .read_u32::<LittleEndian>()
            .map_err(|e| format!("Failed to read version: {}", e))?;
        if version != 2 && version != 3 {
            return Err(format!("Unsupported GGUF version: {}", version));
        }

        let tensor_count = reader
            .read_u64::<LittleEndian>()
            .map_err(|e| format!("Failed to read tensor count: {}", e))?;
        let metadata_kv_count = reader
            .read_u64::<LittleEndian>()
            .map_err(|e| format!("Failed to read metadata count: {}", e))?;

        let mut kv_map: HashMap<String, String> = HashMap::new();

        // Read up to first 1000 metadata entries directly from binary GGUF header
        let entries_to_read = metadata_kv_count.min(1000);
        for _ in 0..entries_to_read {
            if let Ok(key) = Self::read_string(&mut reader) {
                if let Ok(val_type) = reader.read_u32::<LittleEndian>() {
                    if let Ok(val_str) = Self::read_value(&mut reader, val_type) {
                        kv_map.insert(key, val_str);
                    }
                }
            } else {
                break;
            }
        }

        let architecture = kv_map
            .get("general.architecture")
            .cloned()
            .unwrap_or_else(|| "unknown".to_string());

        let model_name = kv_map
            .get("general.name")
            .cloned()
            .unwrap_or_else(|| {
                file_name
                    .strip_suffix(".gguf")
                    .unwrap_or(&file_name)
                    .to_string()
            });

        // Context length (look for <arch>.context_length or general context length)
        let ctx_key = format!("{}.context_length", architecture);
        let context_length = kv_map
            .get(&ctx_key)
            .or_else(|| kv_map.get("llama.context_length"))
            .or_else(|| kv_map.get("qwen2.context_length"))
            .or_else(|| kv_map.get("context_length"))
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(8192);

        // Architecture dimensions & tensor structural metadata
        let embd_key = format!("{}.embedding_length", architecture);
        let embedding_length = kv_map.get(&embd_key).and_then(|v| v.parse::<u64>().ok());

        let block_key = format!("{}.block_count", architecture);
        let block_count = kv_map.get(&block_key).and_then(|v| v.parse::<u64>().ok());

        let head_key = format!("{}.attention.head_count", architecture);
        let head_count = kv_map.get(&head_key).and_then(|v| v.parse::<u64>().ok());

        // Multimodal vision & projection layer metadata
        let vision_projector_type = kv_map
            .get("clip.projector_type")
            .cloned()
            .or_else(|| {
                if kv_map.get("clip.has_llava_projector").map(|v| v == "true" || v == "1").unwrap_or(false) {
                    Some("mlp".to_string())
                } else if kv_map.get("clip.vision_encoder").is_some() {
                    kv_map.get("clip.vision_encoder").cloned()
                } else {
                    None
                }
            });

        let vision_image_size = kv_map
            .get("clip.vision_encoder.image_size")
            .or_else(|| kv_map.get("clip.image_size"))
            .and_then(|v| v.parse::<u32>().ok());

        let vision_patch_size = kv_map
            .get("clip.vision_encoder.patch_size")
            .or_else(|| kv_map.get("clip.patch_size"))
            .and_then(|v| v.parse::<u32>().ok());

        let tokenizer_model = kv_map.get("tokenizer.ggml.model").cloned();

        let vision_encoder_type = Self::detect_vision_encoder(&architecture, &file_name, &kv_map);

        let has_vision = is_multimodal_projector
            || vision_encoder_type.is_some()
            || vision_projector_type.is_some()
            || architecture.to_lowercase().contains("vl")
            || architecture.to_lowercase().contains("vision")
            || file_name.to_lowercase().contains("vl")
            || file_name.to_lowercase().contains("mai-ui");

        // Quantization info from file_type or filename
        let quant_type = Self::detect_quant_type(&file_name, &kv_map);

        Ok(GgufModelInfo {
            file_name,
            file_path: path.to_path_buf(),
            file_size_bytes,
            file_size_mb,
            architecture,
            model_name,
            context_length,
            tensor_count,
            quant_type,
            is_multimodal_projector,
            embedding_length,
            block_count,
            head_count,
            vision_projector_type,
            vision_image_size,
            vision_patch_size,
            tokenizer_model,
            has_vision,
            vision_encoder_type,
        })
    }

    pub fn detect_vision_encoder(
        architecture: &str,
        file_name: &str,
        kv_map: &HashMap<String, String>,
    ) -> Option<String> {
        let arch_lower = architecture.to_lowercase();
        let fname_lower = file_name.to_lowercase();

        // 1. Qwen-VL family (Qwen2-VL, Qwen3-VL, mai-ui)
        if arch_lower.contains("qwen2vl")
            || arch_lower.contains("qwen3vl")
            || arch_lower.contains("qwen2_vl")
            || fname_lower.contains("qwen3-vl")
            || fname_lower.contains("mai-ui")
        {
            return Some("Qwen-VL ViT".to_string());
        }

        // 2. MiniCPM-V family
        if arch_lower.contains("minicpm") || fname_lower.contains("minicpm") {
            return Some("MiniCPM-V ViT".to_string());
        }

        // 3. InternVL2 family
        if arch_lower.contains("internvl") || fname_lower.contains("internvl") {
            return Some("InternVL2 ViT".to_string());
        }

        // 4. LLaVA family (LLaVA 1.5 vs LLaVA-NeXT)
        if arch_lower.contains("llava") || fname_lower.contains("llava") {
            let has_dynamic = kv_map
                .get("clip.has_dynamic_tiling")
                .map(|v| v == "true" || v == "1")
                .unwrap_or(false);
            if has_dynamic {
                return Some("LLaVA-NeXT ViT".to_string());
            } else {
                return Some("LLaVA 1.5 ViT".to_string());
            }
        }

        // 5. MobileVLM family
        if let Some(proj) = kv_map.get("clip.projector_type") {
            if proj.to_lowercase().contains("ldp") {
                return Some("MobileVLM ViT".to_string());
            }
        }

        // 6. SigLIP / PaliGemma / Gemma 3 family
        if let Some(enc) = kv_map.get("clip.vision_encoder") {
            let enc_lower = enc.to_lowercase();
            if enc_lower.contains("siglip") {
                return Some("SigLIP ViT".to_string());
            } else if enc_lower.contains("qwen") {
                return Some("Qwen-VL ViT".to_string());
            } else {
                return Some(format!("{} ViT", enc));
            }
        }

        // 7. Generic CLIP projector
        if kv_map.contains_key("clip.projector_type")
            || kv_map.contains_key("clip.has_llava_projector")
            || arch_lower.contains("clip")
        {
            return Some("CLIP ViT".to_string());
        }

        if arch_lower.contains("vl") || arch_lower.contains("vision") {
            return Some(format!("{} ViT", architecture));
        }

        None
    }

    fn read_string<R: Read>(reader: &mut R) -> Result<String, std::io::Error> {
        let len = reader.read_u64::<LittleEndian>()? as usize;
        if len > 1024 * 64 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "String too long in GGUF metadata",
            ));
        }
        let mut buf = vec![0u8; len];
        reader.read_exact(&mut buf)?;
        String::from_utf8(buf).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    fn read_value<R: Read + Seek>(reader: &mut R, val_type: u32) -> Result<String, std::io::Error> {
        match val_type {
            0 => Ok(reader.read_u8()?.to_string()),
            1 => Ok(reader.read_i8()?.to_string()),
            2 => Ok(reader.read_u16::<LittleEndian>()?.to_string()),
            3 => Ok(reader.read_i16::<LittleEndian>()?.to_string()),
            4 => Ok(reader.read_u32::<LittleEndian>()?.to_string()),
            5 => Ok(reader.read_i32::<LittleEndian>()?.to_string()),
            6 => Ok(reader.read_f32::<LittleEndian>()?.to_string()),
            7 => Ok((reader.read_u8()? != 0).to_string()),
            8 => Self::read_string(reader),
            9 => {
                // Array
                let item_type = reader.read_u32::<LittleEndian>()?;
                let count = reader.read_u64::<LittleEndian>()? as usize;
                // For simplicity, skip or summarize array
                if item_type == 8 && count <= 5 {
                    let mut items = Vec::new();
                    for _ in 0..count {
                        if let Ok(s) = Self::read_string(reader) {
                            items.push(s);
                        }
                    }
                    Ok(items.join(", "))
                } else {
                    // Skip remaining array elements based on item type
                    Self::skip_array_items(reader, item_type, count)?;
                    Ok(format!("[Array of {} items]", count))
                }
            }
            10 => Ok(reader.read_u64::<LittleEndian>()?.to_string()),
            11 => Ok(reader.read_i64::<LittleEndian>()?.to_string()),
            12 => Ok(reader.read_f64::<LittleEndian>()?.to_string()),
            _ => Ok(format!("[Unknown type {}]", val_type)),
        }
    }

    fn skip_array_items<R: Read + Seek>(
        reader: &mut R,
        item_type: u32,
        count: usize,
    ) -> Result<(), std::io::Error> {
        let item_size = match item_type {
            0 | 1 | 7 => 1,
            2 | 3 => 2,
            4 | 5 | 6 => 4,
            10 | 11 | 12 => 8,
            8 => {
                for _ in 0..count {
                    let len = reader.read_u64::<LittleEndian>()?;
                    reader.seek(SeekFrom::Current(len as i64))?;
                }
                return Ok(());
            }
            _ => return Ok(()),
        };
        reader.seek(SeekFrom::Current((item_size * count) as i64))?;
        Ok(())
    }

    fn detect_quant_type(filename: &str, kv: &HashMap<String, String>) -> String {
        let upper = filename.to_uppercase();
        for q in &[
            "Q4_K_M", "Q4_K_S", "Q5_K_M", "Q5_K_S", "Q8_0", "Q4_0", "Q4_1", "Q5_0", "Q5_1",
            "Q2_K", "Q3_K_M", "Q3_K_L", "Q6_K", "F16", "BF16", "F32",
        ] {
            if upper.contains(q) {
                return q.to_string();
            }
        }
        if let Some(ft) = kv.get("general.file_type") {
            return format!("GGUF_FT_{}", ft);
        }
        "Standard Quant".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_parse_real_gguf_if_present() {
        let path = Path::new("C:\\models\\mai-ui-2b.gguf");
        if path.exists() {
            let res = GgufParser::parse_file(path);
            assert!(res.is_ok(), "Failed to parse real GGUF file: {:?}", res.err());
            let info = res.unwrap();
            println!("=== Parsed GGUF Model Info ===");
            println!("Architecture: {}", info.architecture);
            println!("Model Name: {}", info.model_name);
            println!("Context Length: {}", info.context_length);
            println!("Tensor Count: {}", info.tensor_count);
            println!("Has Vision: {}", info.has_vision);
            println!("Vision Encoder Type: {:?}", info.vision_encoder_type);
            println!("Vision Projector: {:?}", info.vision_projector_type);
            println!("Embedding Length: {:?}", info.embedding_length);
            println!("Block Count: {:?}", info.block_count);
            println!("Head Count: {:?}", info.head_count);
            println!("Tokenizer Model: {:?}", info.tokenizer_model);
            println!("==============================\n");
            assert!(!info.architecture.is_empty());
            assert_eq!(info.vision_encoder_type, Some("Qwen-VL ViT".to_string()));
        }
    }
}
