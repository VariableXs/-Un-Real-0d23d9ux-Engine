# -*- coding: utf-8 -*-
"""U盘 Win11 启动稳定性加固（0xED 竞态缓解）：
1. UAS -> BOT 降级：uaspstor.sys 更名备份，用 usbstor.sys 本体顶替。
   BOT 协议无 streams/NCQ，砍掉廉价 U 盘 UAS 固件命令卡死的整条路径。
   （UASPStor 服务 Start 已为 0，PnP 加载该服务时实际运行 BOT 代码）
2. 禁用 USB 选择性挂起（Services\\USB\\DisableSelectiveSuspend=1）
3. 放宽 UASPStor 超时 IoTimeoutValue 0xf -> 0x50
4. 复核 PortableOperatingSystem / AutoReboot / DumpEnabled / BCD 防自愈闸
全部在 U 盘侧离线操作（X: 系统 hive + Y: BCD），符合引导红线。
"""
import os, shutil, subprocess, datetime, hashlib

OUT = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\vx-stability.rpt"
BS = chr(92)
X = "X:" + BS
Y = "Y:" + BS
_fh = open(OUT, "w", encoding="utf-8")

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

log(f"=== vx-stability start {datetime.datetime.now()} ===")

drvdir = os.path.join(X, "Windows" + BS + "System32" + BS + "drivers")
uas = os.path.join(drvdir, "uaspstor.sys")
bot = os.path.join(drvdir, "usbstor.sys")
bak = os.path.join(drvdir, "uaspstor.sys.uas-bak")

# ---------- 1. UAS -> BOT 降级 ----------
log("=== 1. UAS -> BOT ===")
try:
    if not os.path.isfile(bot):
        log("  FATAL: usbstor.sys 不在位，中止")
        raise SystemExit
    if os.path.isfile(bak):
        log(f"  备份已存在: {bak} ({os.path.getsize(bak)}B) 不覆盖")
    elif os.path.isfile(uas):
        shutil.move(uas, bak)
        log(f"  uaspstor.sys 已改名备份 ({os.path.getsize(bak)}B sha={sha(bak)})")
    else:
        log("  uaspstor.sys 不存在（可能已降级过），跳过改名")
    shutil.copyfile(bot, uas)
    log(f"  usbstor.sys -> uaspstor.sys 完成 ({os.path.getsize(uas)}B sha={sha(uas)})")
    log(f"  原始 usbstor.sys sha={sha(bot)}")
except SystemExit:
    _fh.close()
    raise

# ---------- 2/3. 注册表 ----------
log("=== 2. 离线注册表 ===")
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

    # 2a. 驱动 Start 复核（应为 0）
    for d in ["usbstor", "USBXHCI", "USBHUB3", "UASPStor"]:
        k = svcs + BS + d
        old = getval(k, "Start")
        if old != "0x0":
            run(["reg", "add", k, "/v", "Start", "/t", "REG_DWORD", "/d", "0", "/f"])
        log(f"  {d}.Start: {old} -> {getval(k, 'Start')}")

    # 2b. 禁用 USB 选择性挂起
    kusb = svcs + BS + "USB"
    run(["reg", "add", kusb, "/v", "DisableSelectiveSuspend", "/t", "REG_DWORD", "/d", "1", "/f"])
    log(f"  USB.DisableSelectiveSuspend = {getval(kusb, 'DisableSelectiveSuspend')}")

    # 2c. 放宽 UAS 超时（0xf 秒 -> 0x50 秒）
    kp = svcs + BS + "UASPStor" + BS + "Parameters"
    oldt = getval(kp, "IoTimeoutValue")
    run(["reg", "add", kp, "/v", "IoTimeoutValue", "/t", "REG_DWORD", "/d", "80", "/f"])
    log(f"  UASPStor.Parameters.IoTimeoutValue: {oldt} -> {getval(kp, 'IoTimeoutValue')}")

    # 2d. 便携系统标志 / 崩溃不复启 / 全量转储 复核
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

# ---------- 4. BCD 防自愈闸复核 ----------
log("=== 3. BCD 防自愈闸 ===")
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
    log(f"  FATAL: BCD 不存在 {bcd}")

log("=== vx-stability done ===")
_fh.close()
print("done")
