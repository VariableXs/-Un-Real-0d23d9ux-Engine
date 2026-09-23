# -*- coding: utf-8 -*-
"""vx-stability v2：断点续跑 + 每步遗言。v1 在改名后因 U 盘瞬时掉线崩掉。
未完成项：usbstor.sys -> uaspstor.sys 复制、注册表、BCD。"""
import os, shutil, subprocess, datetime, hashlib, sys, traceback

OUT = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\vx-stability2.rpt"
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

def sha(p):
    h = hashlib.sha256()
    with open(p, "rb") as f:
        for blk in iter(lambda: f.read(1 << 20), b""):
            h.update(blk)
    return h.hexdigest()[:16]

log(f"=== vx-stability v2 start {datetime.datetime.now()} ===")

drvdir = os.path.join(X, "Windows" + BS + "System32" + BS + "drivers")
uas = os.path.join(drvdir, "uaspstor.sys")
bot = os.path.join(drvdir, "usbstor.sys")
bak = os.path.join(drvdir, "uaspstor.sys.uas-bak")

# ---------- 1. 完成 UAS -> BOT 复制 ----------
try:
    log(f"  pre: uaspstor={os.path.getsize(uas) if os.path.isfile(uas) else 'NA'} "
        f"usbstor={os.path.getsize(bot) if os.path.isfile(bot) else 'NA'} "
        f"bak={os.path.getsize(bak) if os.path.isfile(bak) else 'NA'}")
    if os.path.isfile(uas) and os.path.isfile(bot) and os.path.getsize(uas) != os.path.getsize(bot):
        if not os.path.isfile(bak):
            shutil.move(uas, bak)
            log("  重新改名备份完成")
        os.remove(uas)
        log("  旧 uaspstor.sys 已删")
        shutil.copyfile(bot, uas)
        log("  usbstor.sys -> uaspstor.sys 复制完成")
    same = os.path.isfile(uas) and os.path.getsize(uas) == os.path.getsize(bot)
    log(f"  校验: uaspstor 现为 BOT 本体 = {same} (sha uas={sha(uas) if os.path.isfile(uas) else 'NA'} bot={sha(bot)})")
except Exception:
    log("STEP1 EXC " + traceback.format_exc())

# ---------- 2. 注册表 ----------
try:
    hive = os.path.join(X, "Windows" + BS + "System32" + BS + "config" + BS + "SYSTEM")
    run(["reg", "unload", "HKLM" + BS + "VXSYS"])
    lr = run(["reg", "load", "HKLM" + BS + "VXSYS", hive])
    log(f"  reg load: {lr.strip()[:100]}")
    if "SUCCESS" in lr.upper() or "操作成功" in lr:
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

        def getval(key, name):
            out = run(["reg", "query", key, "/v", name])
            for line in out.splitlines():
                if name in line and "REG" in line:
                    return line.split()[-1]
            return "(missing)"

        for d in ["usbstor", "USBXHCI", "USBHUB3", "UASPStor"]:
            k = svcs + BS + d
            old = getval(k, "Start")
            if old != "0x0":
                run(["reg", "add", k, "/v", "Start", "/t", "REG_DWORD", "/d", "0", "/f"])
            log(f"  {d}.Start: {old} -> {getval(k, 'Start')}")

        kusb = svcs + BS + "USB"
        run(["reg", "add", kusb, "/v", "DisableSelectiveSuspend", "/t", "REG_DWORD", "/d", "1", "/f"])
        log(f"  USB.DisableSelectiveSuspend = {getval(kusb, 'DisableSelectiveSuspend')}")

        kp = svcs + BS + "UASPStor" + BS + "Parameters"
        oldt = getval(kp, "IoTimeoutValue")
        run(["reg", "add", kp, "/v", "IoTimeoutValue", "/t", "REG_DWORD", "/d", "80", "/f"])
        log(f"  UASPStor.Parameters.IoTimeoutValue: {oldt} -> {getval(kp, 'IoTimeoutValue')}")

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

        run(["reg", "unload", "HKLM" + BS + "VXSYS"])
        log("  hive 已卸载")
    else:
        log("  FATAL: hive 加载失败")
except Exception:
    log("STEP2 EXC " + traceback.format_exc())

# ---------- 3. BCD 防自愈闸 ----------
try:
    bcd = os.path.join(Y, "EFI" + BS + "Microsoft" + BS + "Boot" + BS + "BCD")
    if os.path.isfile(bcd):
        r1 = run(["bcdedit", "/store", bcd, "/set", "{default}", "recoveryenabled", "No"])
        r2 = run(["bcdedit", "/store", bcd, "/set", "{default}", "bootstatuspolicy", "IgnoreAllFailures"])
        log(f"  recoveryenabled No: {(r1 or 'OK').strip()[:60]}")
        log(f"  bootstatuspolicy: {(r2 or 'OK').strip()[:60]}")
        v = run(["bcdedit", "/store", bcd, "/enum", "{default}"])
        for line in v.splitlines():
            l = line.strip()
            if "recoveryenabled" in l.lower() or "bootstatuspolicy" in l.lower():
                log(f"  验证: {l}")
    else:
        log(f"  Y 盘 BCD 不存在（盘符可能重排），列出当前卷:")
        log(run(["wmic", "logicaldisk", "get", "caption,volumename"]).strip())
except Exception:
    log("STEP3 EXC " + traceback.format_exc())

log("=== v2 done ===")
_fh.close()
print("done")
