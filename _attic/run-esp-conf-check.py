# 提权运行 vx-check-esp-conf.ps1 并轮询日志（复用 esp-deploy-switch.py 的 UAC 模式）
import ctypes, os, subprocess, sys, time

ROOT = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main"
SCRIPT = ROOT + r"\_attic\vx-check-esp-conf.ps1"
LOG = ROOT + r"\_attic\vx-check-esp-conf-r3.log"

raw = open(SCRIPT, "rb").read()
if not raw.startswith(b"\xef\xbb\xbf"):
    open(SCRIPT, "wb").write(b"\xef\xbb\xbf" + raw)

# {log} 占位符替换为本仓库内日志路径（避免落 %TEMP% 找不到）
body = open(SCRIPT, "rb").read().decode("utf-8-sig")
body = body.replace("{log}", LOG.replace("\\", "\\\\"))
tmp = SCRIPT  # 直接改临时副本路径执行
tmp = ROOT + r"\_attic\vx-check-esp-conf-run.ps1"
open(tmp, "wb").write(b"\xef\xbb\xbf" + body.encode("utf-8"))

if os.path.exists(LOG):
    os.remove(LOG)

params = (
    '-NoProfile -ExecutionPolicy Bypass -Command '
    f'try {{ & \'{tmp}\' *> \'{LOG}\' }} '
    f'catch {{ $_ | Out-String | Add-Content \'{LOG}\' }}'
)
rc = ctypes.windll.shell32.ShellExecuteW(None, "runas", "powershell.exe", params, None, 0)
if rc <= 32:
    print(f"UAC 被取消 (rc={rc})")
    sys.exit(2)

deadline = time.time() + 90
while time.time() < deadline:
    time.sleep(2)
    if os.path.exists(LOG):
        data = open(LOG, "rb").read().decode("utf-16", "replace")
        if "ESP-CONF-CHECK-DONE" in data or "Exception" in data or "throw" in data:
            print(data)
            break
else:
    print("超时")
