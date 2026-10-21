import subprocess
import os
import sys
import time
import threading
from datetime import datetime

if sys.platform == "win32":
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")

def main():
    root_dir = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    test_log_dir = os.path.join(root_dir, "test_log")
    os.makedirs(test_log_dir, exist_ok=True)

    timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")
    log_file_path = os.path.join(test_log_dir, f"{timestamp}_agent_e2e_mai_ui.log")

    log_lines = []
    def log(msg):
        line = f"[{datetime.now().strftime('%Y-%m-%d %H:%M:%S.%f')[:-3]}] {msg}"
        print(line)
        log_lines.append(line)

    jarvis_bin = os.path.join(root_dir, "target", "release", "jarvis.exe")
    if not os.path.exists(jarvis_bin):
        jarvis_bin = os.path.join(root_dir, "jarvis.exe")

    image_path = os.path.join(root_dir, "cache", "vlm_test_input.png")
    prompt = "Describe in about 100 words all the icons in the task bar and the main content in the application shown in this screenshot."

    log("=================================================================")
    log("  JARVIS AGENT END-TO-END MULTIMODAL PIPELINE TEST (mai-ui:2b)   ")
    log("=================================================================")
    log(f"Agent Binary: {jarvis_bin}")
    log(f"Test Log File: {log_file_path}")
    log(f"Model: mai-ui-2b.gguf (Qwen-VL UI Specialized)")
    log(f"Attached Screenshot: {image_path} (1024x576)")
    log(f"User Goal: {prompt}")
    log(f"Pipeline: Complete AgentLoop -> Backend -> PipeEngineClient -> jarvis-engine.exe")

    cmd = [
        jarvis_bin,
        "--model", "mai-ui-2b.gguf",
        "--image", image_path,
        "--prompt", prompt
    ]

    log(f"Executing: {' '.join(cmd)}")
    start_time = time.time()

    proc = subprocess.Popen(
        cmd,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
        bufsize=1,
        cwd=root_dir
    )

    stderr_captured = []
    def drain_stderr():
        for line in proc.stderr:
            stderr_captured.append(line)
    t = threading.Thread(target=drain_stderr, daemon=True)
    t.start()

    TIMEOUT_SECONDS = 120  # Strict 2-minute time limit
    log(f"Time Limit: {TIMEOUT_SECONDS}s (2 minutes maximum)")

    agent_output_lines = []
    timed_out = False

    def reader():
        for line in proc.stdout:
            clean = line.rstrip()
            log(clean)
            agent_output_lines.append(clean)

    reader_thread = threading.Thread(target=reader, daemon=True)
    reader_thread.start()

    reader_thread.join(timeout=TIMEOUT_SECONDS)
    if reader_thread.is_alive():
        timed_out = True
        log(f"\n[TIMEOUT ERROR] Test exceeded the 2-minute limit ({TIMEOUT_SECONDS}s)! Terminating process...")
        proc.kill()
        try:
            subprocess.run(["taskkill", "/F", "/T", "/PID", str(proc.pid)], capture_output=True)
        except Exception:
            pass
        reader_thread.join(timeout=2)

    total_time = time.time() - start_time
    returncode = proc.poll() if proc.poll() is not None else -1

    log("\n=================================================================")
    if timed_out:
        log(f"Test Status: FAILED - TIMEOUT (Exceeded {TIMEOUT_SECONDS}s limit)")
    else:
        log(f"Agent Process Finished with Exit Code: {returncode}")
    log(f"Total End-to-End Elapsed Time: {total_time:.2f}s")
    log("=================================================================")

    if stderr_captured:
        log("\n--- Captured Stderr ---")
        for sl in stderr_captured:
            log(f"  {sl.strip()}")

    with open(log_file_path, "w", encoding="utf-8") as f:
        f.write("\n".join(log_lines) + "\n")
    log(f"\nExecution log written to: {log_file_path}")

if __name__ == "__main__":
    main()
