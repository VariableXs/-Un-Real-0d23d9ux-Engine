# -*- coding: utf-8 -*-
"""T1 环境盘点（只读）：物理磁盘/卷枚举 + QEMU 定位 + WHPX 可用性探测。

只读诊断，不写任何盘、不改任何配置。输出供本会话施工决策用。
"""
import ctypes
import ctypes.wintypes as wt
import os
import shutil
import subprocess
import sys

ATTIC = os.path.dirname(os.path.abspath(__file__))


def enum_volumes():
    """枚举盘符卷：标签/文件系统/容量/总线类型线索。"""
    k32 = ctypes.windll.kernel32
    drives = []
    bitmask = k32.GetLogicalDrives()
    for i in range(26):
        if not (bitmask >> i) & 1:
            continue
        letter = chr(ord("A") + i) + ":\\"
        vol = ctypes.create_unicode_buffer(261)
        fs = ctypes.create_unicode_buffer(64)
        serno = wt.DWORD(0)
        maxlen = wt.DWORD(0)
        flags = wt.DWORD(0)
        ok = k32.GetVolumeInformationW(letter, vol, 261, ctypes.byref(serno),
                                       ctypes.byref(maxlen), ctypes.byref(flags), fs, 64)
        if not ok:
            drives.append((letter[:-1], "(不可读)", "?", -1, -1))
            continue
        free = ctypes.c_ulonglong(0)
        total = ctypes.c_ulonglong(0)
        k32.GetDiskFreeSpaceExW(letter, ctypes.byref(free), ctypes.byref(total), None)
        drives.append((letter[:-1], vol.value, fs.value,
                       round(total.value / 2**30, 1), round(free.value / 2**30, 1)))
    return drives


def enum_disks():
    """枚举物理磁盘（index/model/size/bus），经 wmic 兼容通道（PowerShell CIM）。"""
    ps = ("Get-CimInstance Win32_DiskDrive | ForEach-Object { "
          "\"{0}|{1}|{2}|{3}\" -f $_.Index, $_.Model, [math]::Round($_.Size/1GB,1), $_.InterfaceType }")
    try:
        out = subprocess.run(["powershell", "-NoProfile", "-Command", ps],
                             capture_output=True, text=True, timeout=60)
        return [ln.strip() for ln in out.stdout.splitlines() if "|" in ln]
    except Exception as e:  # noqa: BLE001
        return ["(枚举失败: %s)" % e]


def find_qemu():
    """定位 qemu-system-x86_64.exe：PATH + 常见安装根。"""
    hits = []
    p = shutil.which("qemu-system-x86_64")
    if p:
        hits.append(p)
    roots = [
        r"C:\Program Files\qemu", r"C:\Program Files (x86)\qemu",
        r"D:\Program Files\qemu", r"D:\qemu",
        os.path.expanduser(r"~\qemu"), r"C:\msys64\ucrt64\bin",
        r"C:\msys64\mingw64\bin",
    ]
    for root in roots:
        cand = os.path.join(root, "qemu-system-x86_64.exe")
        if os.path.isfile(cand):
            hits.append(cand)
    # _attic 既有脚本里出现过的绝对路径
    try:
        import glob as _g
        for py in _g.glob(os.path.join(ATTIC, "*.py")):
            with open(py, "r", encoding="utf-8", errors="ignore") as f:
                for line in f:
                    if "qemu-system-x86_64" in line and ("\\" in line or "/" in line):
                        for tok in line.replace('"', " ").replace("'", " ").split():
                            if tok.lower().endswith("qemu-system-x86_64.exe") and os.path.isfile(tok):
                                hits.append(tok)
    except Exception:  # noqa: BLE001
        pass
    return sorted(set(hits))


def probe_whpx(qemu):
    """WHPX 可用性：-accel whpx 快速探针（-display none, 立即退出）。"""
    if not qemu:
        return "NO-QEMU"
    try:
        r = subprocess.run(
            [qemu, "-accel", "whpx", "-machine", "q35", "-smp", "1", "-m", "128",
             "-display", "none", "-serial", "none", "-monitor", "none"],
            capture_output=True, text=True, timeout=45)
        blob = (r.stdout + r.stderr).lower()
        if "whpx" in blob and ("not" in blob or "fail" in blob or "不支持" in blob):
            return "UNAVAILABLE: " + blob.strip().splitlines()[0][:160] if blob.strip() else "UNAVAILABLE"
        if r.returncode in (0, 1) and "whpx is not" not in blob:
            # qemu 正常启动后我们未喂命令, rc=0/1 都算探通（WHPX 初始化成功时无 whpx 报错）
            if "whpx" not in blob:
                return "OK (no whpx complaint)"
            return "CHECK: " + " | ".join(blob.strip().splitlines()[:3])[:300]
        return "RC=%d %s" % (r.returncode, blob.strip()[:200])
    except subprocess.TimeoutExpired:
        return "TIMEOUT(视为可用: 未报 WHPX 错误)"


def main():
    print("=== 卷枚举 ===")
    for d in enum_volumes():
        print("  %s [%s] fs=%s total=%.1fGB free=%.1fGB" % d)
    print("=== 物理磁盘 ===")
    for d in enum_disks():
        print("  " + d)
    print("=== QEMU ===")
    qs = find_qemu()
    for q in qs:
        print("  " + q)
    if qs:
        print("=== WHPX 探针 ===")
        print("  " + probe_whpx(qs[0]))
    else:
        print("=== WHPX 探针 === 未找到 QEMU，跳过")
    # 关键目录存在性
    print("=== 关键物 ===")
    we = r"W:\SteamLibrary\steamapps\workshop\content\431960"
    print("  WE workshop dir:", "EXISTS" if os.path.isdir(we) else "missing", we)
    iso = os.path.join(os.path.dirname(ATTIC), "varix-qemu.iso")
    print("  varix-qemu.iso:", os.path.getsize(iso) if os.path.isfile(iso) else "missing")


if __name__ == "__main__":
    sys.exit(main())
