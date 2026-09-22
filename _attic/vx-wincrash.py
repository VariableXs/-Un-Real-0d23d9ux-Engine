# -*- coding: utf-8 -*-
"""U盘 Win11 崩溃取证（只读 + 拷贝转储文件到工作区）：
1. 列出/复制 Minidump 与 MEMORY.DMP，解析 bugcheck 代码
2. 离线加载 SYSTEM 配置单元，查启动关键驱动 Start 值
3. 查 U 盘 BCD 引导恢复设置
"""
import os, struct, shutil, subprocess, datetime

OUT = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\vx-wincrash.rpt"
DUMPDIR = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\dumps"
X = "X:" + chr(92)
out = []

def log(s):
    out.append(str(s))

now = datetime.datetime.now()

# ---------- 1. 转储文件 ----------
log("=== 1. 崩溃转储 ===")
cands = []
for d in [os.path.join(X, r"Windows\Minidump"), os.path.join(X, r"Windows"), os.path.join(X, r"Windows\LiveKernelReports")]:
    try:
        for f in os.listdir(d):
            p = os.path.join(d, f)
            try:
                st = os.stat(p)
                if f.lower().endswith(".dmp") and st.st_size > 0:
                    cands.append((st.st_mtime, p, st.st_size))
            except Exception:
                pass
    except Exception as ex:
        log(f"  {d}: {ex!r}")
cands.sort(reverse=True)
for mt, p, sz in cands[:8]:
    log(f"  {p}  {sz/2**20:.1f}MB  {datetime.datetime.fromtimestamp(mt)}")

os.makedirs(DUMPDIR, exist_ok=True)
parsed = 0
for mt, p, sz in cands[:3]:
    try:
        dst = os.path.join(DUMPDIR, os.path.basename(p))
        shutil.copy2(p, dst)
        with open(dst, "rb") as f:
            head = f.read(0x60)
        if head[:8] == b"PAGEDU64":
            code = struct.unpack_from("<I", head, 0x38)[0]
            p1, p2, p3, p4 = struct.unpack_from("<4Q", head, 0x40)
            log(f"  [bugcheck] {os.path.basename(p)}: 代码=0x{code:08X} 参数=({p1:#x},{p2:#x},{p3:#x},{p4:#x})")
            if code == 0x7B:
                log("    -> 0x7B INACCESSIBLE_BOOT_DEVICE：内核找不到启动盘（USB 存储驱动未随引导加载）")
            elif code == 0xEF:
                log("    -> 0xEF CRITICAL_PROCESS_DIED：关键进程死亡")
            elif code == 0x21A:
                log("    -> 0x21A WINLOGON_CSRSS 崩溃")
            parsed += 1
        else:
            log(f"  {os.path.basename(p)}: 非标准转储头 {head[:8]!r}")
    except Exception as ex:
        log(f"  复制/解析 {p} 失败: {ex!r}")
if not cands:
    log("  无任何 .dmp 转储——崩溃发生在内核起来之前（winload 阶段）或转储被禁用")

# ---------- 2. 离线注册表：启动驱动 ----------
log("")
log("=== 2. 启动关键驱动 Start 值（0=引导启动 3=按需） ===")
hive = r"X:\Windows\System32\config\SYSTEM"
def run(cmd):
    r = subprocess.run(cmd, capture_output=True, text=True, timeout=60)
    return (r.stdout or "") + (r.stderr or "")

r = run(["reg", "query", r"HKLM\VXSYS" if False else "HKLM\\VXSYS", "/v", "x-nothing"])
if "系统找不到" in r or "ERROR" in r.upper():
    try:
        # 先确保卸载
        run(["reg", "unload", "HKLM\\VXSYS"])
    except Exception:
        pass
lr = run(["reg", "load", "HKLM\\VXSYS", hive])
log(f"  reg load: {(lr or '(空)').strip()[:120]}")
if "SUCCESS" in lr.upper() or "操作成功" in lr:
    sel = run(["reg", "query", "HKLM\\VXSYS\\Select"])
    cur = None
    for line in sel.splitlines():
        if "Default" in line:
            try:
                cur = int(line.split()[-1], 16) if line.strip().split()[-1].startswith("0x") else int(line.split()[-1])
            except Exception:
                pass
    cs = f"ControlSet{cur:03d}" if cur else "ControlSet001"
    log(f"  当前控制集: {cs}")
    for drv in ["usbstor", "USBXHCI", "USBHUB3", "UASPStor", "stornvme", "storahci",
                "partmgr", "volume", "vdrvroot", "mountmgr", "fs_rec", "pci", "acpi",
                "WdBoot", "WERHEALTH" ]:
        q = run(["reg", "query", f"HKLM\\VXSYS\\{cs}\\Services\\{drv}", "/v", "Start"])
        val = "?"
        for line in q.splitlines():
            if "Start" in line and "REG" in line:
                val = line.split()[-1]
        log(f"  {drv}: Start={val}")
    # 启动盘信息：MountedDevices / PortHint 略
    run(["reg", "unload", "HKLM\\VXSYS"])
else:
    log(f"  hive 加载失败：{lr[:200]}")

# ---------- 3. U盘 BCD 引导恢复设置 ----------
log("")
log("=== 3. U盘 BCD 恢复设置 ===")
bcd = r"Y:\EFI\Microsoft\Boot\BCD"
q = run(["bcdedit", "/store", bcd, "/enum", "{default}"])
for line in q.splitlines():
    if any(k in line for k in ["recoveryenabled", "bootstatuspolicy", "device", "osdevice", "identifier", "device"]):
        log(f"  {line.strip()}")

with open(OUT, "w", encoding="utf-8") as f:
    f.write("\n".join(out) + "\n")
print("\n".join(out))
