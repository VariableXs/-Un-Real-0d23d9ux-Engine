# -*- coding: utf-8 -*-
"""0xED v6：①备份并清空 MountedDevices（内核重建映射）②审计非微软 Boot 驱动并禁用可疑项。
U盘 SYSTEM hive 手术，逐行落盘+回读验证。输出 vx-fix-0xed-v6.rpt
"""
import ctypes
import os
import subprocess
import traceback
import winreg

LOG = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\vx-fix-0xed-v6.rpt"
logf = None

def log(m):
    print(m, flush=True); logf.write(m + "\n"); logf.flush()

def run(cmd, timeout=300):
    log("$ " + " ".join(cmd))
    try:
        r = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout, errors="replace")
        for line in ((r.stdout or "") + (r.stderr or "")).splitlines():
            if line.strip():
                log("  | " + line.strip())
        return r.returncode
    except Exception as ex:
        log(f"  | EXC {ex!r}"); return -2

def find_vol(label):
    k32 = ctypes.WinDLL("kernel32")
    k32.FindFirstVolumeW.restype = ctypes.c_void_p
    k32.FindFirstVolumeW.argtypes = [ctypes.c_wchar_p, ctypes.c_uint32]
    k32.FindNextVolumeW.restype = ctypes.c_int
    k32.FindNextVolumeW.argtypes = [ctypes.c_void_p, ctypes.c_wchar_p, ctypes.c_uint32]
    k32.FindVolumeClose.argtypes = [ctypes.c_void_p]
    k32.GetVolumeInformationW.restype = ctypes.c_int
    k32.GetVolumeInformationW.argtypes = [ctypes.c_wchar_p, ctypes.c_wchar_p, ctypes.c_uint32,
                                          ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p,
                                          ctypes.c_wchar_p, ctypes.c_uint32]
    buf = ctypes.create_unicode_buffer(300)
    h = k32.FindFirstVolumeW(buf, 300)
    if not h:
        return None
    found = None
    while True:
        root = buf.value.rstrip("\\") + "\\"
        name = ctypes.create_unicode_buffer(261)
        if k32.GetVolumeInformationW(root, name, 261, None, None, None, None, 0) and name.value == label:
            found = buf.value
            break
        if not k32.FindNextVolumeW(h, buf, 300):
            break
    k32.FindVolumeClose(h)
    return found

def mount(g, letter):
    k32 = ctypes.WinDLL("kernel32")
    k32.SetVolumeMountPointW.restype = ctypes.c_int
    k32.SetVolumeMountPointW.argtypes = [ctypes.c_wchar_p, ctypes.c_wchar_p]
    k32.DeleteVolumeMountPointW.argtypes = [ctypes.c_wchar_p]
    k32.DeleteVolumeMountPointW(letter + "\\")
    return bool(k32.SetVolumeMountPointW(letter + "\\", g))

# 微软标准引导驱动白名单（Start 可为 0 的）
MS_BOOT_OK = {
    "acpi", "afd", "amdxata", "atapi", "battery", "beep", "bfs", "bowser", "cdrom",
    "classpnp", "clfs", "cng", "crashdmp", "disk", "drmkaud", "dump_diskdump",
    "dump_dumpfve", "dump_storage", "ew", "exfat", "fastfat", "fileinfo", "filecrypt",
    "fltmgr", "fs_rec", "fvevol", "fvolop", "fwpkclnt", "hvloader", "http", "intelide",
    "isapnp", "kbdclass", "kbdhid", "kdcom", "ks", "kbdclass", "luafv", "modem",
    "mountmgr", "mrxsmb", "msfs", "msisadrv", "msrpc", "mup", "ndis", "ndu", "netbios",
    "netbt", "npcap", "npfs", "npsvctrig", "ntfs", "null", "nwifi", "partmgr", "pci",
    "pcw", "pdc", "peauth", "processr", "rdbss", "rdpbus", "rdpvideominiport",
    "raspp", "refs", "rspndr", "sdbus", "storahci", "stornvme", "storport", "swenum",
    "tbs", "tcpip", "tdx", "tpm", "tsusbflt", "tunnel", "udfs", "umbus", "usbccgp",
    "usbhub", "usbhub3", "usbstor", "usbxhci", "uaspstor", "vhf", "volmgr", "volsnap",
    "volume", "vdrvroot", "vmbus", "vsm", "wacompen", "wanarp", "wd", "wdf01000",
    "wdfldr", "wfplwfs", "winhvr", "wmilib", "ws2ifsl", "xinputhid", "hidclass",
    "hidparse", "hidusb", "mshidkmdf", "mshidumdf", "cuvid", "ndiswan", "qwavedrv",
    "sffdisk", "sffp_sd", "sfloppy", "acpiex", "condrv", "cimfs", "bam", "iorate",
    "mmcss", "dxgkrnl", "dxgmms1", "dxgmms2", "monitor", "mouclass", "mouhid",
}

def main():
    global logf
    logf = open(LOG, "w", encoding="utf-8")
    try:
        log("=== 0xED v6 ===")
        eng = find_vol("WIN_ENGINE")
        log(f"eng={bool(eng)}")
        if not eng:
            log("FATAL no WIN_ENGINE"); return
        mount(eng, "X:"); log("X: mounted")
        run(["reg", "load", "HKLM\\VXS", r"X:\Windows\System32\config\SYSTEM"])
        # ① MountedDevices：备份全部值，然后清空
        md = winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, r"VXS\MountedDevices", 0,
                            winreg.KEY_READ | winreg.KEY_SET_VALUE)
        vals = []
        i = 0
        while True:
            try:
                n, v, t = winreg.EnumValue(md, i); i += 1
                vals.append((n, v, t))
            except OSError:
                break
        log(f"MountedDevices has {len(vals)} values (backing up)")
        with open(r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\mounteddevices-backup-20260923.txt", "w") as f:
            for n, v, t in vals:
                f.write(f"{n}\t{v.hex()}\t{t}\n")
        while vals:
            n = vals[0][0]
            try:
                winreg.DeleteValue(md, n)
                log(f"  deleted {n}")
            except OSError as ex:
                log(f"  del {n} err {ex!r}")
            vals = vals[1:]
        winreg.CloseKey(md)
        log("MountedDevices cleared (backup saved)")
        # ② 审计 Services：Start=0 且不在白名单的
        srv = winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, r"VXS\ControlSet001\Services", 0, winreg.KEY_READ)
        suspects = []
        i = 0
        while True:
            try:
                name = winreg.EnumKey(srv, i); i += 1
            except OSError:
                break
            try:
                sk = winreg.OpenKey(srv, name)
                try:
                    start = winreg.QueryValueEx(sk, "Start")[0]
                except OSError:
                    continue
                try:
                    grp = winreg.QueryValueEx(sk, "Group")[0]
                except OSError:
                    grp = ""
                try:
                    image = winreg.QueryValueEx(sk, "ImagePath")[0]
                except OSError:
                    image = ""
                if start == 0 and name.lower() not in MS_BOOT_OK:
                    suspects.append((name, grp, image))
                winreg.CloseKey(sk)
            except OSError:
                continue
        winreg.CloseKey(srv)
        log(f"non-MS Start=0 drivers: {len(suspects)}")
        for n, g, img in suspects:
            log(f"  SUSPECT {n} grp={g} img={img}")
        # 禁用可疑项（Start=4 disabled），白名单外一律谨慎——先只禁 image 路径含 hasleo/vhd 相关
        for n, g, img in suspects:
            low = (n + " " + img).lower()
            if any(k in low for k in ("hasleo", "w2g", "wtg", "vhd", " portable", "easyuefi", "ewf", "fbwf", "cascadecode")):
                sk = winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, r"VXS\ControlSet001\Services\\" + n, 0, winreg.KEY_SET_VALUE)
                winreg.SetValueEx(sk, "Start", 0, winreg.REG_DWORD, 4)
                winreg.CloseKey(sk)
                log(f"  DISABLED {n}")
        # 回读验证
        k = winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, r"VXS\MountedDevices", 0, winreg.KEY_READ)
        cnt = 0
        while True:
            try:
                winreg.EnumValue(k, cnt); cnt += 1
            except OSError:
                break
        winreg.CloseKey(k)
        log(f"verify: MountedDevices now has {cnt} values")
        run(["reg", "unload", "HKLM\\VXS"])
        log("=== fix done v6 ===")
    except Exception:
        log("EXC-PATH:\n" + traceback.format_exc())
    finally:
        try: logf.close()
        except Exception: pass

if __name__ == "__main__":
    if not ctypes.windll.shell32.IsUserAnAdmin():
        print("need admin"); raise SystemExit(1)
    main()
