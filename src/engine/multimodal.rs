use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct MultimodalInput {
    pub image_path: PathBuf,
    pub mime_type: String,
    pub base64_data: String,
}

impl MultimodalInput {
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let p = path.as_ref();
        if !p.exists() {
            return Err(format!("Image file does not exist: {}", p.display()));
        }

        let extension = p
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        let mime_type = match extension.as_str() {
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "webp" => "image/webp",
            "gif" => "image/gif",
            "bmp" => "image/bmp",
            _ => return Err(format!("Unsupported image format: .{}", extension)),
        }
        .to_string();

        let bytes = fs::read(p).map_err(|e| format!("Failed to read image file: {}", e))?;
        let base64_data = BASE64.encode(bytes);

        Ok(Self {
            image_path: p.to_path_buf(),
            mime_type,
            base64_data,
        })
    }

    pub fn raw_base64(&self) -> &str {
        &self.base64_data
    }

    pub fn to_data_url(&self) -> String {
        format!("data:{};base64,{}", self.mime_type, self.base64_data)
    }

    /// Strips any 'data:...;base64,' prefix to get pure base64
    pub fn clean_base64(input: &str) -> String {
        if let Some(pos) = input.find(";base64,") {
            input[pos + 8..].to_string()
        } else {
            input.to_string()
        }
    }

    /// Ensures a base64 string has the 'data:image/...;base64,' URI prefix
    pub fn ensure_data_url(input: &str, default_mime: &str) -> String {
        if input.starts_with("data:") {
            input.to_string()
        } else {
            format!("data:{};base64,{}", default_mime, input)
        }
    }
}
