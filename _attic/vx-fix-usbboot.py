# -*- coding: utf-8 -*-
"""U盘 Win11 USB 启动驱动修复（0x7B）：
1. 核对四个驱动 .sys 文件在位
2. 离线 reg: Start 3->0（usbstor/USBXHCI/USBHUB3/UASPStor）
3. 回读验证
4. 顺手重上 U盘 BCD 防自愈闸（recoveryenabled No + IgnoreAllFailures，
   U盘侧操作，符合引导红线）
"""
import os, subprocess, datetime

OUT = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\vx-fix-usbboot.rpt"
BS = chr(92)
X = "X:" + BS
Y = "Y:" + BS
DRIVERS = ["usbstor", "USBXHCI", "USBHUB3", "UASPStor"]
_fh = open(OUT, "w", encoding="utf-8")

def log(s):
    _fh.write(str(s) + "\n")
    _fh.flush()

def run(cmd):
    try:
        r = subprocess.run(cmd, capture_output=True, text=True, timeout=60)
        return (r.stdout or "") + (r.stderr or "")
    except Exception as ex:
        return f"EXC {type(ex).__name__}"

log(f"=== usb-boot fix start {datetime.datetime.now()} ===")

# 1. 驱动文件核对
log("=== 1. 驱动文件核对 ===")
drvdir = os.path.join(X, "Windows" + BS + "System32" + BS + "drivers")
missing = False
for d in DRIVERS:
    p = os.path.join(drvdir, d + ".sys")
    ok = os.path.isfile(p)
    sz = os.path.getsize(p) if ok else 0
    log(f"  {d}.sys: {'OK ' + str(sz) + 'B' if ok else 'MISSING!'}")
    if not ok:
        missing = True
# INF 存在性（保证 PnP 能匹配）
infx = os.path.join(X, "Windows" + BS + "INF")
for inf in ["usbstor.inf", "usbport.inf", "usbhub3.inf", "uaspstor.inf"]:
    p = os.path.join(infx, inf)
    log(f"  INF {inf}: {'OK' if os.path.isfile(p) else 'missing'}")

# 2. 离线注册表 Start 3->0
log("=== 2. Start 值修改 ===")
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
    for d in DRIVERS:
        key = "HKLM" + BS + "VXSYS" + BS + cs + BS + "Services" + BS + d
        before = run(["reg", "query", key, "/v", "Start"])
        old = "?"
        for line in before.splitlines():
            if "Start" in line and "REG" in line:
                old = line.split()[-1]
        ar = run(["reg", "add", key, "/v", "Start", "/t", "REG_DWORD", "/d", "0", "/f"])
        # 回读
        after = run(["reg", "query", key, "/v", "Start"])
        new = "?"
        for line in after.splitlines():
            if "Start" in line and "REG" in line:
                new = line.split()[-1]
        log(f"  {d}: {old} -> {new}  (add rc: {ar.strip()[:40]})")
    run(["reg", "unload", "HKLM" + BS + "VXSYS"])
    log("  hive 已卸载")
else:
    log(f"  hive 加载失败，中止注册表部分")

# 3. U盘 BCD 防自愈闸
log("=== 3. U盘 BCD 防自愈闸 ===")
bcd = os.path.join(Y, "EFI" + BS + "Microsoft" + BS + "Boot" + BS + "BCD")
if os.path.isfile(bcd):
    r1 = run(["bcdedit", "/store", bcd, "/set", "{default}", "recoveryenabled", "No"])
    r2 = run(["bcdedit", "/store", bcd, "/set", "{default}", "bootstatuspolicy", "IgnoreAllFailures"])
    log(f"  recoveryenabled No: {r1.strip()[:80]}")
    log(f"  bootstatuspolicy: {r2.strip()[:80]}")
    v = run(["bcdedit", "/store", bcd, "/enum", "{default}"])
    for line in v.splitlines():
        if "recoveryenabled" in line or "bootstatuspolicy" in line:
            log(f"  验证: {line.strip()}")

log("=== fix done ===")
_fh.close()
print("done")
