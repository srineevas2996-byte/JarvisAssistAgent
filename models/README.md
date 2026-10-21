# 🧠 J.A.A (jarvis_assist_agent) Models Directory

Place your `.gguf` language models and multimodal vision projectors (`mmproj-*.gguf`) in this folder.

---

## 🏆 Optimal Tested Vision Model: `mai-ui:2b`

Based on our empirical benchmark tests (conducted in the *Vision Model Token Generation Plan* evaluation), the **optimal model for Windows desktop UI perception, OCR, and autonomous navigation** is:

### 🥇 **`mai-ui:2b` (`mai-ui-2b.gguf`)** — Qwen-VL UI Specialized
- **Architecture**: Qwen-VL ViT + Spatial Merger + Autoregressive Transformer
- **Context Length**: Up to 16,384 tokens
- **Size / VRAM**: ~2.2 GB (runs smoothly on virtually any GPU or modern CPU)
- **Why It Won the Benchmark**:
  - **Cold Start Time**: **4.56 seconds** to initialize and lock into GPU memory.
  - **Inference Latency**: **35.7s – 36.2s** end-to-end (over **2.5× faster** than `qwen3-vl:2b` and **3× faster** than `minicpm-v2.6:8b`).
  - **Grounding Precision**: **100% accuracy** on full-desktop taskbar icons, window titles, and application content without hallucination.
  - **Zero Infinite Loops**: Unlike larger general VLMs that can enter internal thought loops on dense screenshots, `mai-ui:2b` generates concise, deterministic UI observations.

---

## 📊 Benchmark Comparison Summary

| Model Spec | Architecture | Init Time | Inference Time | Desktop Grounding Accuracy | Hallucination Status | Recommendation |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **`mai-ui:2b`** | **Qwen-VL UI** | **4.56s** | **35.7s** | **100%** (Identified all 15 taskbar apps) | **Zero Hallucination** | 🌟 **Optimal Primary Choice** |
| `qwen3-vl:2b` | Qwen-VL General | 4.28s | 91.35s | Partial (Stuck in repeated thought loops) | High Repetition | Fallback |
| `minicpm-v2.6:8b` | CLIP ViT + mmproj | 11.64s | 109.04s | Moderate (Confused website buttons for OS icons) | Minor Hallucinations | Secondary Fallback |
| `gemma3:4b` | SigLIP ViT | 15.97s | 70.08s | Low (Invented non-existent apps) | Heavy Hallucination | Not Recommended |
| `gemma4:e2b` | SigLIP ViT | 4.90s | 24.93s | 0% (Described UI as a document grid of "m"s) | Critical Failure | Avoid for Desktop UI |

---

## 📥 How to Obtain `mai-ui:2b`

You have three convenient ways to load `mai-ui:2b` into J.A.A:

### Method 1: Via Ollama (Auto-Detected)
If you have [Ollama](https://ollama.com) installed, run:
```bash
ollama pull mai-ui:2b
```
J.A.A automatically discovers and reads GGUF model blobs directly from your `%USERPROFILE%\.ollama\models` directory with zero copying or network socket overhead.

### Method 2: Drop GGUF File Directly
Download `mai-ui-2b.gguf` (or any compatible Qwen-VL / LLaVA GGUF model) and drop it into this `models/` directory:
```text
models/
└── mai-ui-2b.gguf
```

### Method 3: Direct Download via J.A.A CLI
Inside the J.A.A terminal, pull directly from Hugging Face:
```text
>>> /model pull hf:bartowski/Qwen2.5-VL-3B-Instruct-GGUF/Qwen2.5-VL-3B-Instruct-Q4_K_M.gguf
```

---

## 🕹️ CLI Model Commands
```text
/model             # Lists all recognized models (.gguf and Ollama cache)
/model <#>         # Loads a model by index number (e.g. /model 1)
/model <filename>  # Loads a model by exact filename
/status            # Verifies active model, architecture, and memory pinning
```
