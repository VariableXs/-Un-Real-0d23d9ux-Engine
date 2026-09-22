# 提权只读诊断 v1：
#  1) bcdedit /enum firmware —— 固件引导项清单（Windows Boot Manager 是否还在）
#  2) GPT 主/备头 CRC —— PhysicalDrive0（内置）与 PhysicalDrive1（U 盘）
# 严格零写入。
import ctypes, os, struct, subprocess, sys, time, zlib

ROOT = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main"
LOG = ROOT + r"\_attic\vx-elev-diag.rpt"

def main():
    lines = []
    def say(m):
        lines.append(str(m))

    # 1) bcdedit firmware
    r = subprocess.run(["bcdedit", "/enum", "firmware"], capture_output=True)
    out = (r.stdout or b"") + (r.stderr or b"")
    for enc in ("gbk", "utf-8"):
        try:
            txt = out.decode(enc)
            break
        except UnicodeDecodeError:
            continue
    else:
        txt = out.decode("utf-8", "replace")
    say("== bcdedit /enum firmware ==")
    say(txt)
    say("windows-bootmgr present: " + str("windowsbootmgr" in txt.lower().replace(" ", "").replace("\u200b", "")))

    # 2) GPT headers
    def crc32(b):
        return zlib.crc32(b) & 0xFFFFFFFF

    for n in (0, 1):
        path = f"\\\\.\\PhysicalDrive{n}"
        say(f"== PhysicalDrive{n} GPT ==")
        try:
            f = open(path, "rb", buffering=0)
        except OSError as e:
            say(f"open failed: {e}")
            continue
        with f:
            fd = f.fileno()
            def pread(off, size):
                f.seek(off)
                return f.read(size)
            # 磁盘总大小（ioctl 不可用就跳过；用 GPT 头 alternate_lba 定位备份头）
            sec = pread(512, 512)
            if sec[:8] != b"EFI PART":
                say(f"LBA1 not GPT: {sec[:8]!r}")
                continue
            hdr = sec[:92]
            stored = struct.unpack_from("<I", hdr, 16)[0]
            zeroed = bytearray(hdr)
            zeroed[16:20] = bytes(4)
            calc = crc32(bytes(zeroed))
            alt_lba = struct.unpack_from("<Q", hdr, 32)[0]
            pe_lba = struct.unpack_from("<Q", hdr, 72)[0]
            pe_num = struct.unpack_from("<I", hdr, 80)[0]
            pe_size = struct.unpack_from("<I", hdr, 84)[0]
            pe_crc = struct.unpack_from("<I", hdr, 88)[0]
            say(f"primary hdr @LBA1: stored={stored:08x} calc={calc:08x} "
                f"{'OK' if stored == calc else '!! MISMATCH !!'}")
            arr = pread(pe_lba * 512, pe_num * pe_size)
            c = crc32(arr)
            say(f"entry array: stored={pe_crc:08x} calc={c:08x} "
                f"{'OK' if c == pe_crc else '!! MISMATCH !!'}")
            bh = pread(alt_lba * 512, 512)
            if bh[:8] == b"EFI PART":
                s2 = struct.unpack_from("<I", bh, 16)[0]
                zb = bytearray(bh[:92])
                zb[16:20] = bytes(4)
                c2 = crc32(bytes(zb))
                say(f"backup hdr @LBA{alt_lba}: stored={s2:08x} calc={c2:08x} "
                    f"{'OK' if s2 == c2 else '!! MISMATCH !!'}")
                pe2 = struct.unpack_from("<I", bh, 88)[0]
                pl2 = struct.unpack_from("<Q", bh, 72)[0]
                arr2 = pread(pl2 * 512, pe_num * pe_size)
                cc2 = crc32(arr2)
                say(f"backup entry array: stored={pe2:08x} calc={cc2:08x} "
                    f"{'OK' if cc2 == pe2 else '!! MISMATCH !!'}")
            else:
                say(f"backup hdr @LBA{alt_lba} invalid: {bh[:8]!r}")
    with open(LOG, "w", encoding="utf-8") as fh:
        fh.write("\n".join(lines) + "\nELEV-DIAG-DONE\n")

if __name__ == "__main__":
    # 提权自举：非管理员则 UAC 重启自己
    try:
        is_admin = ctypes.windll.shell32.IsUserAnAdmin()
    except Exception:
        is_admin = False
    if not is_admin:
        params = f'"{os.path.abspath(__file__)}" elevated'
        rc = ctypes.windll.shell32.ShellExecuteW(None, "runas", sys.executable, params, None, 0)
        if rc <= 32:
            print(f"UAC 被取消 (rc={rc})")
            sys.exit(2)
        print("已发起提权，等待报告…")
        deadline = time.time() + 120
        while time.time() < deadline:
            time.sleep(2)
            if os.path.exists(LOG):
                print(open(LOG, encoding="utf-8", errors="replace").read())
                break
        else:
            print("超时")
    else:
        try:
            main()
        except Exception as e:
            import traceback
            with open(LOG, "w", encoding="utf-8") as fh:
                fh.write("EXCEPTION:\n" + traceback.format_exc() + "\nELEV-DIAG-DONE\n")
        print("ELEV-DIAG-DONE")
