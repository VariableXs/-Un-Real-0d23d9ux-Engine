# -*- coding: utf-8 -*-
"""vx-stability v4：利用残留挂载 HKLM\\VXMC（= U 盘 SYSTEM hive）直接写入，
再卸载验证 + 重载复核。不再 reg load。"""
import subprocess, datetime, time, sys, traceback

OUT = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\vx-stability4.rpt"
BS = chr(92)
Y = "Y:" + BS
_fh = open(OUT, "w", encoding="utf-8")

def _excepthook(t, v, tb):
    try:
        log("FATAL " + "".join(traceback.format_exception(t, v, tb)))
    except Exception:
        pass
sys.excepthook = _excepthook

def log(s):
    _fh.write(str(s) + "\n")
    _fh.flush()

def run(cmd):
    try:
        r = subprocess.run(cmd, capture_output=True, text=True, timeout=90)
        return (r.stdout or "") + (r.stderr or "")
    except Exception as ex:
        return f"EXC {type(ex).__name__}"

def getval(key, name):
    out = run(["reg", "query", key, "/v", name])
    for line in out.splitlines():
        if name in line and "REG" in line:
            return line.split()[-1]
    return "(missing)"

log(f"=== vx-stability v4 start {datetime.datetime.now()} ===")

root = "HKLM" + BS + "VXMC"
# 先确认身份
idv = getval(root + BS + "ControlSet001" + BS + "Services" + BS + "UASPStor", "Start")
log(f"  身份确认 UASPStor.Start = {idv}（应为 0x0 = U 盘 hive）")
if idv != "0x0":
    log("  FATAL: VXMC 不是 U 盘 hive 或已被改动，中止")
    _fh.close()
    sys.exit(1)

svcs = root + BS + "ControlSet001" + BS + "Services"

# 1. UASPStor ImagePath -> usbstor.sys（UAS 服务跑 BOT 本体）
k = svcs + BS + "UASPStor"
run(["reg", "add", k, "/v", "ImagePath", "/t", "REG_EXPAND_SZ",
     "/d", BS + "SystemRoot" + BS + "System32" + BS + "drivers" + BS + "usbstor.sys", "/f"])
log(f"  UASPStor.ImagePath -> {getval(k, 'ImagePath')}")

# 2. Start 复核
for d in ["usbstor", "USBXHCI", "USBHUB3", "UASPStor"]:
    kd = svcs + BS + d
    old = getval(kd, "Start")
    if old != "0x0":
        run(["reg", "add", kd, "/v", "Start", "/t", "REG_DWORD", "/d", "0", "/f"])
    log(f"  {d}.Start: {old} -> {getval(kd, 'Start')}")

# 3. 禁用选择性挂起
kusb = svcs + BS + "USB"
run(["reg", "add", kusb, "/v", "DisableSelectiveSuspend", "/t", "REG_DWORD", "/d", "1", "/f"])
log(f"  USB.DisableSelectiveSuspend = {getval(kusb, 'DisableSelectiveSuspend')}")

# 4. 放宽 UASPStor 超时 0xf -> 0x50
kp = svcs + BS + "UASPStor" + BS + "Parameters"
oldt = getval(kp, "IoTimeoutValue")
run(["reg", "add", kp, "/v", "IoTimeoutValue", "/t", "REG_DWORD", "/d", "80", "/f"])
log(f"  UASPStor.Parameters.IoTimeoutValue: {oldt} -> {getval(kp, 'IoTimeoutValue')}")

# 5. 便携标志/崩溃不复启/转储 复核
ctrl = root + BS + "ControlSet001" + BS + "Control"
for name, want, val in [("PortableOperatingSystem", "0x1", "1"),
                        ]:
    old = getval(ctrl, name)
    if old != want:
        run(["reg", "add", ctrl, "/v", name, "/t", "REG_DWORD", "/d", val, "/f"])
    log(f"  {name}: {old} -> {getval(ctrl, name)}")
cc = ctrl + BS + "CrashControl"
for name, want, val in [("AutoReboot", "0x0", "0"), ("DumpEnabled", "0x1", "1")]:
    old = getval(cc, name)
    if old != want:
        run(["reg", "add", cc, "/v", name, "/t", "REG_DWORD", "/d", val, "/f"])
    log(f"  CrashControl.{name}: {old} -> {getval(cc, name)}")

# 6. 卸载 VXMC（落盘），带重试
unloaded = False
for attempt in range(1, 4):
    ur = run(["reg", "unload", "HKLM" + BS + "VXMC"])
    ok = "SUCCESS" in ur.upper() or "操作成功" in ur or "ERROR" not in ur.upper()
    log(f"  unload 尝试{attempt}: {ur.strip()[:80]}")
    still = run(["reg", "query", "HKLM" + BS + "VXMC", "/v", ""])
    if "ERROR" in still.upper() or "错误" in still or "unable" in still.lower():
        unloaded = True
        log(f"  VXMC 已卸载（查询确认不存在）")
        break
    time.sleep(2)
log(f"  卸载结果: {unloaded}")

# 7. 重载复核（证明改动已落盘）
hive = "X:" + BS + "Windows" + BS + "System32" + BS + "config" + BS + "SYSTEM"
if unloaded:
    lr = run(["reg", "load", "HKLM" + BS + "VXSYS", hive])
    log(f"  复核重载: {lr.strip()[:80]}")
    if "SUCCESS" in lr.upper() or "操作成功" in lr:
        r2 = "HKLM" + BS + "VXSYS" + BS + "ControlSet001" + BS + "Services" + BS + "UASPStor"
        log(f"  落盘验证 UASPStor.ImagePath = {getval(r2, 'ImagePath')}")
        log(f"  落盘验证 UASPStor.Start = {getval(r2, 'Start')}")
        kusb2 = "HKLM" + BS + "VXSYS" + BS + "ControlSet001" + BS + "Services" + BS + "USB"
        log(f"  落盘验证 USB.DisableSelectiveSuspend = {getval(kusb2, 'DisableSelectiveSuspend')}")
        kp2 = r2 + BS + "Parameters"
        log(f"  落盘验证 IoTimeoutValue = {getval(kp2, 'IoTimeoutValue')}")
        ur2 = run(["reg", "unload", "HKLM" + BS + "VXSYS"])
        log(f"  复核卸载: {ur2.strip()[:60]}")

log("=== v4 done ===")
_fh.close()
print("done")
