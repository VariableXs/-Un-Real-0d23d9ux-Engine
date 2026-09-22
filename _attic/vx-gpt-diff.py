# 提权只读诊断 v2：内置盘 PhysicalDrive0 主/备 GPT 分区表数组逐项 diff。
# 严格零写入。输出到 _attic\vx-gpt-diff.rpt
import ctypes, os, struct, sys, time

ROOT = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main"
LOG = ROOT + r"\_attic\vx-gpt-diff.rpt"

def main():
    lines = []
    f = open(r"\\.\PhysicalDrive0", "rb", buffering=0)
    with f:
        def pread(off, size):
            f.seek(off)
            return f.read(size)
        hdr = pread(512, 512)[:92]
        pe_lba = struct.unpack_from("<Q", hdr, 72)[0]
        pe_num = struct.unpack_from("<I", hdr, 80)[0]
        pe_size = struct.unpack_from("<I", hdr, 84)[0]
        alt_lba = struct.unpack_from("<Q", hdr, 32)[0]
        bh = pread(alt_lba * 512, 512)[:92]
        pl2 = struct.unpack_from("<Q", bh, 72)[0]
        arr_p = pread(pe_lba * 512, pe_num * pe_size)
        arr_b = pread(pl2 * 512, pe_num * pe_size)
        lines.append(f"pe_lba={pe_lba} n={pe_num} size={pe_size} backup_pe_lba={pl2}")
        for i in range(pe_num):
            e_p = arr_p[i*pe_size:(i+1)*pe_size]
            e_b = arr_b[i*pe_size:(i+1)*pe_size]
            if e_p != e_b:
                lines.append(f"--- entry {i} DIFFERS ---")
                lines.append(f"primary : typeGUID={e_p[:16].hex()} partGUID={e_p[16:32].hex()} "
                             f"first={struct.unpack_from('<Q', e_p, 32)[0]} last={struct.unpack_from('<Q', e_p, 40)[0]} "
                             f"attr={struct.unpack_from('<Q', e_p, 48)[0]}")
                lines.append(f"backup  : typeGUID={e_b[:16].hex()} partGUID={e_b[16:32].hex()} "
                             f"first={struct.unpack_from('<Q', e_b, 32)[0]} last={struct.unpack_from('<Q', e_b, 40)[0]} "
                             f"attr={struct.unpack_from('<Q', e_b, 48)[0]}")
            else:
                t = e_p[:16].hex()
                if t != "0" * 32:
                    lines.append(f"entry {i} same: type={t} "
                                 f"first={struct.unpack_from('<Q', e_p, 32)[0]} last={struct.unpack_from('<Q', e_p, 40)[0]}")
        # 主头与备头其它字段差异
        for off, name in ((72, "pe_lba"), (32, "alt_lba")):
            pass
        lines.append(f"hdr primary pe_lba={pe_lba} alt={alt_lba}")
        lines.append(f"hdr backup  pe_lba={pl2} alt={struct.unpack_from('<Q', bh, 32)[0]}")
    with open(LOG, "w", encoding="utf-8") as fh:
        fh.write("\n".join(lines) + "\nGPT-DIFF-DONE\n")

if __name__ == "__main__":
    try:
        is_admin = ctypes.windll.shell32.IsUserAnAdmin()
    except Exception:
        is_admin = False
    if not is_admin:
        rc = ctypes.windll.shell32.ShellExecuteW(None, "runas", sys.executable, f'"{os.path.abspath(__file__)}" elevated', None, 0)
        if rc <= 32:
            print("UAC 被取消"); sys.exit(2)
        deadline = time.time() + 120
        while time.time() < deadline:
            time.sleep(2)
            if os.path.exists(LOG):
                print(open(LOG, encoding="utf-8", errors="replace").read()); break
        else:
            print("超时")
    else:
        try:
            main()
        except Exception:
            import traceback
            open(LOG, "w", encoding="utf-8").write("EXCEPTION:\n" + traceback.format_exc())
        print("done")
