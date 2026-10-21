import subprocess
import json
import os
import sys
import time
import threading
from datetime import datetime

def run_single_vlm_test(name, model_path, mmproj_path, vision_encoder, image_path, prompt, log_fn):
    log_fn(f"\n========================================================")
    log_fn(f"RUNNING VLM TEST: {name}")
    log_fn(f"Model Path: {model_path}")
    if mmproj_path:
        log_fn(f"Projector Path: {mmproj_path}")
    log_fn(f"ViT Architecture: {vision_encoder}")
    log_fn(f"========================================================")

    root_dir = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    engine_bin = os.path.join(root_dir, "jarvis-engine.exe")
    if not os.path.exists(engine_bin):
        engine_bin = os.path.join(root_dir, "target", "release", "jarvis-engine.exe")

    cmd = [engine_bin, "--model", model_path, "--ctx", "16384", "--threads", "12"]
    if mmproj_path:
        cmd.extend(["--mmproj", mmproj_path])

    log_fn(f"Launching socketless pipe command: {' '.join(cmd)}")
    start_boot = time.time()
    try:
        proc = subprocess.Popen(
            cmd,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            bufsize=1
        )
    except Exception as e:
        log_fn(f"Failed to spawn engine: {e}")
        return {"name": name, "success": False, "error": str(e)}

    # Drain stderr asynchronously to prevent 4KB pipe buffer deadlock
    stderr_lines = []
    def drain_stderr():
        for line in proc.stderr:
            stderr_lines.append(line)
    t = threading.Thread(target=drain_stderr, daemon=True)
    t.start()

    ready_line = proc.stdout.readline()
    boot_time = time.time() - start_boot
    log_fn(f"Engine Ready in {boot_time:.2f}s: {ready_line.strip()}")
    if not ready_line or '"event":"ready"' not in ready_line:
        log_fn(f"Engine failed to report ready. Stderr snippet: {''.join(stderr_lines[:10])}")
        proc.kill()
        return {"name": name, "success": False, "error": "Ready timeout or failure"}

    req = {
        "cmd": "chat",
        "messages": [
            {
                "role": "user",
                "content": prompt
            }
        ],
        "image_path": image_path,
        "vision_encoder": vision_encoder,
        "temperature": 0.1,
        "max_tokens": 256,
        "stop_tokens": ["<|im_end|>", "<|endoftext|>", "<end_of_turn>", "</s>"]
    }

    t0 = time.time()
    proc.stdin.write(json.dumps(req) + "\n")
    proc.stdin.flush()

    tokens = []
    time_taken = 0.0
    tokens_encoded = 0
    error_msg = None

    while True:
        line = proc.stdout.readline()
        if not line:
            break
        try:
            ev = json.loads(line.strip())
            ev_type = ev.get("event")
            if ev_type == "token":
                tok = ev.get("data", "")
                tokens.append(tok)
                # Print token in real-time
                sys.stdout.write(tok)
                sys.stdout.flush()
            elif ev_type == "thinking":
                thought = ev.get("data", "")
                log_fn(f"\n  [Thinking] {thought}")
                if "continuous multimodal tokens" in thought:
                    parts = thought.split("Encoded ")
                    if len(parts) > 1:
                        tokens_encoded = parts[1].split()[0]
            elif ev_type == "done":
                time_taken = ev.get("time_taken", time.time() - t0)
                log_fn(f"\n  [Done] Generation finished in {time_taken:.2f}s")
                break
            elif ev_type == "error":
                error_msg = ev.get("error")
                log_fn(f"\n  [Engine Error] {error_msg}")
                break
        except Exception as e:
            log_fn(f"Parse error: {e} for line: {line.strip()}")

    # Cleanup subprocess
    proc.stdin.write(json.dumps({"cmd": "exit"}) + "\n")
    proc.stdin.flush()
    try:
        proc.wait(timeout=5)
    except:
        proc.kill()

    full_output = "".join(tokens).strip()
    return {
        "name": name,
        "success": error_msg is None and len(full_output) > 0,
        "boot_time": boot_time,
        "inference_time": time_taken,
        "tokens_encoded": tokens_encoded,
        "output": full_output,
        "error": error_msg
    }

def main():
    root_dir = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    test_log_dir = os.path.join(root_dir, "test_log")
    os.makedirs(test_log_dir, exist_ok=True)

    timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")
    log_file_path = os.path.join(test_log_dir, f"{timestamp}_VLM_test.log")

    log_lines = []
    def log(msg):
        line = f"[{datetime.now().strftime('%Y-%m-%d %H:%M:%S.%f')[:-3]}] {msg}"
        print(line)
        log_lines.append(line)

    image_path = os.path.join(root_dir, "cache", "vlm_test_input.png")
    prompt = "Describe in about 100 words all the icons in the task bar and the main content in the application shown in this screenshot."

    log("=================================================================")
    log("  JARVIS VLM CAPACITY & MULTI-ARCHITECTURE TOKENIZATION EVALUATION ")
    log("=================================================================")
    log(f"Test Name: VLM_test")
    log(f"Test Log File: {log_file_path}")
    log(f"Input Image: {image_path} (1024x576)")
    log(f"Prompt: {prompt}")
    log(f"Mode: Pure VLM (Metadata & Accessibility Tree Temporarily Blocked)")
    log(f"Pipeline: JARVIS Native Stdio Anonymous Pipe (Zero Sockets, Zero HTTP)")

    # Define candidate models
    user_home = os.path.expanduser("~")
    ollama_blobs = os.path.join(user_home, ".ollama", "models", "blobs")

    candidate_models = []

    # 1. mai-ui:2b
    mai_path = r"C:\models\mai-ui-2b.gguf"
    if not os.path.exists(mai_path):
        mai_path = os.path.join(ollama_blobs, "sha256-f50ea57f3eabe99b718c67bfc80e8e14733980ce9adde2cc6d5c36e43da76f85")
    if os.path.exists(mai_path):
        candidate_models.append({
            "name": "mai-ui:2b (Qwen-VL UI Specialized)",
            "model_path": mai_path,
            "mmproj_path": None,
            "vision_encoder": "Qwen-VL ViT"
        })

    # 2. qwen3-vl:2b
    qwen3vl_path = os.path.join(ollama_blobs, "sha256-ebabfa59b71a5b96e0281ec2994977e785284e0939807a99fc340dec3c6f10de")
    if os.path.exists(qwen3vl_path):
        candidate_models.append({
            "name": "qwen3-vl:2b (General Multimodal VLM)",
            "model_path": qwen3vl_path,
            "mmproj_path": None,
            "vision_encoder": "Qwen-VL ViT"
        })

    # 3. minicpm-v2.6:8b
    minicpm_model = os.path.join(ollama_blobs, "sha256-52a31a07991467109aa7b85e3122b4eb2e9fe58b0dc5501c25f45c230551636d")
    minicpm_proj = os.path.join(ollama_blobs, "sha256-4485f68a0f1aa404c391e788ea88ea653c100d8e98fe572698f701e5809711fd")
    if os.path.exists(minicpm_model) and os.path.exists(minicpm_proj):
        candidate_models.append({
            "name": "minicpm-v2.6:8b (CLIP ViT with Separate mmproj)",
            "model_path": minicpm_model,
            "mmproj_path": minicpm_proj,
            "vision_encoder": "CLIP ViT"
        })

    results = []
    for cand in candidate_models:
        res = run_single_vlm_test(
            cand["name"],
            cand["model_path"],
            cand["mmproj_path"],
            cand["vision_encoder"],
            image_path,
            prompt,
            log
        )
        results.append(res)

    log("\n=================================================================")
    log("  VLM_test COMPREHENSIVE RESULTS & CAPACITY COMPARISON           ")
    log("=================================================================")
    for r in results:
        status_str = "PASS" if r["success"] else "FAIL"
        log(f"\nModel: {r['name']} [{status_str}]")
        if r["success"]:
            log(f"  • ViT Continuous Tokens Encoded: {r['tokens_encoded']}")
            log(f"  • Initialization Time: {r['boot_time']:.2f}s")
            log(f"  • Inference & Token Generation: {r['inference_time']:.2f}s")
            log(f"  • Model Generated Description:\n    \"{r['output']}\"")
        else:
            log(f"  • Error: {r.get('error')}")

    with open(log_file_path, "w", encoding="utf-8") as f:
        f.write("\n".join(log_lines) + "\n")
    log(f"\nExecution log written to: {log_file_path}")

if __name__ == "__main__":
    main()
