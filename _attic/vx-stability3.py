# -*- coding: utf-8 -*-
"""vx-stability v3：ImagePath 重定向方案（不碰驱动文件）。
- UASPStor ImagePath -> \\SystemRoot\\System32\\drivers\\usbstor.sys（UAS 服务跑 BOT 本体）
- hive load 带重试（v2 报被占用）
- BCD 已在 v2 完成，只复核。"""
import os, subprocess, datetime, time, sys, traceback

OUT = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\vx-stability3.rpt"
BS = chr(92)
X = "X:" + BS
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

log(f"=== vx-stability v3 start {datetime.datetime.now()} ===")

hive = os.path.join(X, "Windows" + BS + "System32" + BS + "config" + BS + "SYSTEM")
loaded = False
for attempt in range(1, 4):
    run(["reg", "unload", "HKLM" + BS + "VXSYS"])
    time.sleep(1.5 * attempt)
    lr = run(["reg", "load", "HKLM" + BS + "VXSYS", hive])
    log(f"  load 尝试{attempt}: {lr.strip()[:100]}")
    if "SUCCESS" in lr.upper() or "操作成功" in lr:
        loaded = True
        break
    time.sleep(3)

if loaded:
    sel = run(["reg", "query", "HKLM" + BS + "VXSYS" + BS + "Select"])
    cur = None
    for line in sel.splitlines():
        if "Default" in line:
            tok = line.split()[-1]
            try:
                cur = int(tok, 16) if tok.lower().startswith("0x") else int(tok)
            except Exception:
                pass
    cs = f"ControlSet{cur:03d}" if cur else "ControlSet001"
    log(f"  控制集: {cs}")
    svcs = "HKLM" + BS + "VXSYS" + BS + cs + BS + "Services"

    # 1. UASPStor ImagePath 重定向
    k = svcs + BS + "UASPStor"
    oldip = run(["reg", "query", k, "/v", "ImagePath"])
    log(f"  UASPStor.ImagePath 旧值: {[l.strip() for l in oldip.splitlines() if 'ImagePath' in l or 'REG_' in l]}")
    run(["reg", "add", k, "/v", "ImagePath", "/t", "REG_EXPAND_SZ",
         "/d", BS + "SystemRoot" + BS + "System32" + BS + "drivers" + BS + "usbstor.sys", "/f"])
    log(f"  UASPStor.ImagePath 新值: {getval(k, 'ImagePath')}")

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

    # 4. 放宽超时
    kp = svcs + BS + "UASPStor" + BS + "Parameters"
    oldt = getval(kp, "IoTimeoutValue")
    run(["reg", "add", kp, "/v", "IoTimeoutValue", "/t", "REG_DWORD", "/d", "80", "/f"])
    log(f"  UASPStor.Parameters.IoTimeoutValue: {oldt} -> {getval(kp, 'IoTimeoutValue')}")

    # 5. 便携标志 / 崩溃不复启 / 转储 复核
    ctrl = "HKLM" + BS + "VXSYS" + BS + cs + BS + "Control"
    oldp = getval(ctrl, "PortableOperatingSystem")
    if oldp != "0x1":
        run(["reg", "add", ctrl, "/v", "PortableOperatingSystem", "/t", "REG_DWORD", "/d", "1", "/f"])
    log(f"  PortableOperatingSystem: {oldp} -> {getval(ctrl, 'PortableOperatingSystem')}")
    cc = ctrl + BS + "CrashControl"
    olda = getval(cc, "AutoReboot")
    if olda != "0x0":
        run(["reg", "add", cc, "/v", "AutoReboot", "/t", "REG_DWORD", "/d", "0", "/f"])
    log(f"  CrashControl.AutoReboot: {olda} -> {getval(cc, 'AutoReboot')}")
    oldd = getval(cc, "DumpEnabled")
    if oldd != "0x1":
        run(["reg", "add", cc, "/v", "DumpEnabled", "/t", "REG_DWORD", "/d", "1", "/f"])
    log(f"  CrashControl.DumpEnabled: {oldd} -> {getval(cc, 'DumpEnabled')}")

    ur = run(["reg", "unload", "HKLM" + BS + "VXSYS"])
    log(f"  hive 卸载: {ur.strip()[:60]}")
else:
    log("  FATAL: hive 三次加载均失败")

bcd = os.path.join(Y, "EFI" + BS + "Microsoft" + BS + "Boot" + BS + "BCD")
if os.path.isfile(bcd):
    v = run(["bcdedit", "/store", bcd, "/enum", "{default}"])
    for line in v.splitlines():
        l = line.strip()
        if "recoveryenabled" in l.lower() or "bootstatuspolicy" in l.lower():
            log(f"  BCD 复核: {l}")
else:
    log("  BCD: Y 盘不可达（v2 已写入过，跳过）")

log("=== v3 done ===")
_fh.close()
print("done")
