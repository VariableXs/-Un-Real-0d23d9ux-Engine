# R3 persistent launcher: start variable.exe and keep alive (run in background)
import ctypes, os, subprocess, sys, time
try: ctypes.windll.shcore.SetProcessDpiAwareness(2)
except Exception: ctypes.windll.user32.SetProcessDPIAware()
BASE = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main"
QA = BASE + r"/_attic/qa"
EXE = BASE + r"/src-tauri/target/release/variable.exe"
LOG = QA + r"/r3_run2.log"
logf = open(LOG, "a", encoding="utf-8", errors="replace")
env = dict(os.environ); env["RUST_LOG"] = "info"
p = subprocess.Popen([EXE], stdout=logf, stderr=subprocess.STDOUT, cwd=BASE, env=env)
print("variable pid", p.pid, flush=True)
while True:
    rc = p.poll()
    if rc is not None:
        print("variable exited rc=", rc, flush=True)
        break
    time.sleep(5)
