#[cfg(windows)]
fn main() {
    let mut res = winres::WindowsResource::new();
    res.set_icon("assets/jarvis.ico");
    res.set("ProductName", "J.A.A (jarvis_assist_agent)");
    res.set("FileDescription", "J.A.A - Jarvis Assist Agent Autonomous CLI");
    res.set("LegalCopyright", "Copyright (C) 2026");
    if let Err(e) = res.compile() {
        eprintln!("Warning: Failed to compile Windows resource: {}", e);
    }

    // Ensure native jarvis-engine.exe and jarvis-os-bridge.exe are present alongside binary in target dir
    if let Ok(out_dir) = std::env::var("OUT_DIR") {
        let out_path = std::path::PathBuf::from(out_dir);
        if let Some(target_dir) = out_path.ancestors().nth(3) {
            for bin in &["jarvis-engine.exe", "jarvis-os-bridge.exe"] {
                let src = std::path::Path::new(bin);
                if src.exists() {
                    let dst = target_dir.join(bin);
                    let _ = std::fs::copy(src, dst);
                }
            }
        }
    }
}

#[cfg(not(windows))]
fn main() {}
