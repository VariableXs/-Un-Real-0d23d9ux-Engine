
import os, sys, subprocess
LOG = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\vx-bcdraw.rpt"
def log(m=""):
    with open(LOG, "a", encoding="utf-8") as f: f.write(str(m) + "\n")
def run(cmd):
    try:
        r = subprocess.run(cmd, capture_output=True, text=True, errors="replace", timeout=60)
        return (r.stdout or "") + (("\nERR:" + r.stderr) if r.stderr.strip() else "")
    except Exception as ex:
        return "EXC " + repr(ex)
log("== BCD full dump")
k32 = ctypes.WinDLL("kernel32")
Y = None
for L in "CDEFGHIJKLMNOPQRSTUVWXYZ":
    p = L + ":\\"
    nm = ctypes.create_unicode_buffer(300)
    if k32.GetVolumeInformationW(p, nm, 300, None, None, None, None, 0) and nm.value == "VARIX-ESP":
        Y = L + ":"
log("Y=" + str(Y))
if Y:
    bcd = Y + chr(92) + "Boot" + chr(92) + "BCD"
    out = run(["bcdedit", "/store", bcd, "/enum", "/v"])
    log(out[:6000])
log("done")
