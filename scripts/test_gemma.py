import subprocess
import json
import time
import threading
import os
from datetime import datetime

def main():
    root_dir = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    test_log_dir = os.path.join(root_dir, "test_log")
    os.makedirs(test_log_dir, exist_ok=True)

    timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")
    log_file_path = os.path.join(test_log_dir, f"{timestamp}_gemma_vlm_test.log")

    log_lines = []
    def log(msg):
        line = f"[{datetime.now().strftime('%Y-%m-%d %H:%M:%S.%f')[:-3]}] {msg}"
        print(line)
        log_lines.append(line)

    engine_bin = os.path.join(root_dir, "jarvis-engine.exe")
    blobs_dir = os.path.expanduser("~/.ollama/models/blobs")

    gemma_models = [
        {
            "name": "gemma3:4b (Google Gemma 3 Multimodal)",
            "path": os.path.join(blobs_dir, "sha256-aeda25e63ebd698fab8638ffb778e68bed908b960d39d0becc650fa981609d25"),
            "stops": ["<end_of_turn>", "<eos>"]
        },
        {
            "name": "gemma4:e2b",
            "path": os.path.join(blobs_dir, "sha256-4e30e2665218745ef463f722c0bf86be0cab6ee676320f1cfadf91e989107448"),
            "stops": ["<|im_end|>", "<|endoftext|>"]
        }
    ]

    image_path = os.path.join(root_dir, "cache", "vlm_test_input.png")
    prompt = "Describe in about 100 words all the icons in the task bar and the main content in the application shown in this screenshot."

    for gm in gemma_models:
        log(f"\n========================================================")
        log(f"EVALUATING: {gm['name']}")
        log(f"Model Path: {gm['path']}")
        if not os.path.exists(gm['path']):
            log("Model path does not exist on disk!")
            continue

        cmd = [engine_bin, "--model", gm["path"], "--ctx", "8192", "--threads", "12"]
        log(f"Command: {' '.join(cmd)}")

        t0 = time.time()
        p = subprocess.Popen(
            cmd,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            bufsize=1
        )

        stderr_captured = []
        def drain():
            for l in p.stderr:
                stderr_captured.append(l)
        threading.Thread(target=drain, daemon=True).start()

        ready_line = p.stdout.readline()
        log(f"Ready: {ready_line.strip()} in {time.time() - t0:.2f}s")

        req = {
            "cmd": "chat",
            "messages": [
                {"role": "user", "content": prompt}
            ],
            "image_path": image_path,
            "vision_encoder": "SigLIP ViT",
            "stop_tokens": gm["stops"],
            "temperature": 0.1,
            "max_tokens": 256
        }

        t_gen = time.time()
        p.stdin.write(json.dumps(req) + "\n")
        p.stdin.flush()

        tokens = []
        while True:
            line = p.stdout.readline()
            if not line:
                break
            try:
                ev = json.loads(line.strip())
                ev_type = ev.get("event")
                if ev_type == "token":
                    tokens.append(ev.get("data", ""))
                elif ev_type == "thinking":
                    log(f"  [Thinking] {ev.get('data')}")
                elif ev_type == "done":
                    log(f"  [Done] Generation finished in {ev.get('time_taken', time.time() - t_gen):.2f}s")
                    break
                elif ev_type == "error":
                    log(f"  [Error] {ev.get('error')}")
                    break
            except Exception as e:
                log(f"  [Raw] {line.strip()}")

        log(f"Result for {gm['name']}:")
        log(f"Output: {''.join(tokens).strip()}")

        p.stdin.write(json.dumps({"cmd": "exit"}) + "\n")
        p.stdin.flush()
        try:
            p.wait(timeout=5)
        except:
            p.kill()

    with open(log_file_path, "w", encoding="utf-8") as f:
        f.write("\n".join(log_lines) + "\n")
    log(f"\nSaved test log to: {log_file_path}")

if __name__ == "__main__":
    main()
