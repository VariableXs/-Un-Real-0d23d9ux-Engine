# -*- coding: utf-8 -*-
"""一体化取证 v2（提权运行）：逐行落盘，崩溃也留全量遗言。
1. 按卷标重挂盘符（ESP->Y:, WIN_ENGINE->X:, SHARED->S:）
2. 崩溃转储收集 + bugcheck 解析
3. 离线注册表查启动驱动
4. U盘 BCD 恢复设置
"""
import ctypes, ctypes.wintypes as wt, os, struct, shutil, subprocess, datetime, sys, traceback

OUT = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\vx-wincrash.rpt"
DUMPDIR = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\dumps"
BS = chr(92)
_fh = open(OUT, "w", encoding="utf-8")

def log(s):
    _fh.write(str(s) + "\n")
    _fh.flush()

def dump_exc(tag):
    log(f"!!!! EXC@{tag} !!!!")
    log(traceback.format_exc())

sys.excepthook = lambda t, v, tb: log("FATAL:\n" + "".join(traceback.format_exception(t, v, tb)))

log(f"=== forensic v2 start {datetime.datetime.now()} ===")

try:
    k32 = ctypes.WinDLL("kernel32", use_last_error=True)
    k32.FindFirstVolumeW.restype = wt.HANDLE
    k32.FindFirstVolumeW.argtypes = [ctypes.c_wchar_p, wt.DWORD]
    k32.FindNextVolumeW.restype = wt.BOOL
    k32.FindNextVolumeW.argtypes = [wt.HANDLE, ctypes.c_wchar_p, wt.DWORD]
    k32.FindVolumeClose.restype = wt.BOOL
    k32.GetVolumeInformationW.restype = wt.BOOL
    k32.SetVolumeMountPointW.restype = wt.BOOL

    # ---------- 0. 重挂盘符 ----------
    log("=== 0. 盘符重挂 ===")
    buf = ctypes.create_unicode_buffer(100)
    h = k32.FindFirstVolumeW(buf, 100)
    log(f"  FindFirstVolumeW handle={h}")
    vols = []
    while h:
        vols.append(ctypes.wstring_at(buf, 100).rstrip(chr(0)))
        if not k32.FindNextVolumeW(h, buf, 100):
            break
    try:
        k32.FindVolumeClose(h)
    except Exception:
        pass
    log(f"  卷数={len(vols)}")

    for vol in vols:
        try:
            name = ctypes.create_unicode_buffer(261)
            ok = k32.GetVolumeInformationW(vol, name, 261, None, None, None, None, 0)
            label = name.value if ok else ""
            total = ctypes.c_ulonglong()
            free = ctypes.c_ulonglong()
            k32.GetDiskFreeSpaceExW(vol, ctypes.byref(free), ctypes.byref(total), None)
            size_gb = total.value // 2**30 if total.value else 0
            log(f"  卷 {vol[:52]} label='{label}' size={size_gb}GB")
            target = None
            if label == "WIN_ENGINE":
                target = "X:"
            elif label == "SHARED":
                target = "S:"
            if target:
                mp = target + BS
                if k32.SetVolumeMountPointW(ctypes.create_unicode_buffer(mp), ctypes.create_unicode_buffer(vol)):
                    log(f"    已挂载 {target}")
                else:
                    log(f"    挂 {target} 失败 gle={k32.GetLastError()}（可能已占用，无妨）")
            # ESP 探测：小 FAT 卷 + 根有 limine.conf
            try:
                with open(vol[:-1] + BS + "limine.conf", "r", encoding="utf-8", errors="replace") as f:
                    if "varix" in f.read().lower() and size_gb == 0:
                        if k32.SetVolumeMountPointW(ctypes.create_unicode_buffer("Y:" + BS), ctypes.create_unicode_buffer(vol)):
                            log(f"    ESP 已挂载 Y:")
            except Exception:
                pass
        except Exception:
            dump_exc("vol-loop")

    X = "X:" + BS
    Y = "Y:" + BS
    have_x = os.path.isdir(os.path.join(X, "Windows"))
    have_y = os.path.isfile(os.path.join(Y, "limine.conf"))
    log(f"  X(Win11)={have_x} Y(ESP)={have_y}")

    # ---------- 1. 崩溃转储 ----------
    log("=== 1. 崩溃转储 ===")
    cands = []
    for d in [os.path.join(X, "Windows" + BS + "Minidump"), os.path.join(X, "Windows"),
              os.path.join(X, "Windows" + BS + "LiveKernelReports")]:
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
            log(f"  {d}: {type(ex).__name__}")
    cands.sort(reverse=True)
    for mt, p, sz in cands[:8]:
        log(f"  {p}  {sz/2**20:.1f}MB  {datetime.datetime.fromtimestamp(mt)}")

    os.makedirs(DUMPDIR, exist_ok=True)
    for mt, p, sz in cands[:3]:
        try:
            dst = os.path.join(DUMPDIR, os.path.basename(p))
            shutil.copy2(p, dst)
            with open(dst, "rb") as f:
                head = f.read(0x60)
            if head[:8] == b"PAGEDU64":
                code = struct.unpack_from("<I", head, 0x38)[0]
                p1, p2, p3, p4 = struct.unpack_from("<4Q", head, 0x40)
                log(f"  [bugcheck] {os.path.basename(p)}: 0x{code:08X} ({p1:#x},{p2:#x},{p3:#x},{p4:#x})")
                named = {0x7B: "INACCESSIBLE_BOOT_DEVICE",
                         0xEF: "CRITICAL_PROCESS_DIED",
                         0x21A: "WINLOGON_FATAL", 0x139: "KERNEL_SECURITY_CHECK_FAILURE",
                         0x50: "PAGE_FAULT_IN_NONPAGED_AREA"}.get(code, "")
                if named:
                    log(f"    -> {named}")
        except Exception:
            dump_exc("dmp-copy")
    if not cands:
        log("  无 .dmp —— 崩溃在 winload 阶段（内核未起）或转储被禁用")

    # ---------- 2. 离线注册表 ----------
    log("=== 2. 启动关键驱动 Start（0=引导启动） ===")
    hive = os.path.join(X, "Windows" + BS + "System32" + BS + "config" + BS + "SYSTEM")

    def run(cmd):
        try:
            r = subprocess.run(cmd, capture_output=True, text=True, timeout=60)
            return (r.stdout or "") + (r.stderr or "")
        except Exception as ex:
            return f"EXC {type(ex).__name__}"

    log(f"  hive 存在: {os.path.isfile(hive)}")
    run(["reg", "unload", "HKLM" + BS + "VXSYS"])
    lr = run(["reg", "load", "HKLM" + BS + "VXSYS", hive])
    log(f"  reg load: {lr.strip()[:150]}")
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
        for drv in ["usbstor", "USBXHCI", "USBHUB3", "UASPStor", "stornvme", "storahci",
                    "partmgr", "volume", "vdrvroot", "mountmgr", "pci", "acpi"]:
            q = run(["reg", "query", "HKLM" + BS + "VXSYS" + BS + cs + BS + "Services" + BS + drv, "/v", "Start"])
            val = "ABSENT"
            for line in q.splitlines():
                if "Start" in line and "REG" in line:
                    val = line.split()[-1]
            log(f"  {drv}: Start={val}")
        run(["reg", "unload", "HKLM" + BS + "VXSYS"])
        log("  hive 已卸载")

    # ---------- 3. U盘 BCD ----------
    log("=== 3. U盘 BCD 恢复设置 ===")
    bcd = os.path.join(Y, "EFI" + BS + "Microsoft" + BS + "Boot" + BS + "BCD")
    log(f"  BCD 存在: {os.path.isfile(bcd)}")
    q = run(["bcdedit", "/store", bcd, "/enum", "{default}"])
    log("  " + "\n  ".join(l.strip() for l in q.splitlines() if l.strip())[:900])

except Exception:
    dump_exc("top")

log("=== forensic v2 done ===")
_fh.close()
print("done")
