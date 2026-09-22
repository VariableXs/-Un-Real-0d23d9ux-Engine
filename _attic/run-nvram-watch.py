# 提权运行 nvram-watch-readonly.ps1（严格只读 NVRAM 体检）
import ctypes, os, sys, time

ROOT = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main"
SRC = ROOT + r"\_attic\tools\nvram-watch-readonly.ps1"
RUN = ROOT + r"\_attic\vx-nvram-watch-run.ps1"
LOG = ROOT + r"\_attic\vx-nvram-watch-r2.log"

body = open(SRC, "rb").read().decode("utf-8-sig")
body = body.replace("Set-Content -Path $log", "Set-Content -Path ($log + '.rpt')").replace("{log}", LOG.replace("\\", "\\\\"))
open(RUN, "wb").write(b"\xef\xbb\xbf" + body.encode("utf-8"))

if os.path.exists(LOG):
    os.remove(LOG)

params = (
    '-NoProfile -ExecutionPolicy Bypass -Command '
    f'try {{ & \'{RUN}\' *> \'{LOG}\' }} '
    f'catch {{ $_ | Out-String | Add-Content \'{LOG}\' }}'
)
rc = ctypes.windll.shell32.ShellExecuteW(None, "runas", "powershell.exe", params, None, 0)
if rc <= 32:
    print(f"UAC 被取消 (rc={rc})")
    sys.exit(2)

deadline = time.time() + 120
while time.time() < deadline:
    time.sleep(2)
    if os.path.exists(LOG):
        data = open(LOG, "rb").read().decode("utf-16", "replace")
        if "NVRAM-WATCH-DONE" in data or "Exception" in data:
            print(data[-4000:])
            break
else:
    print("超时")
