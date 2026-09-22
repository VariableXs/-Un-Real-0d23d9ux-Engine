# 内置盘 GPT 主分区表数组修复（2026-09-22，Variable 已拍板）。
# 范围：仅 PhysicalDrive0 LBA2..33（16KB），内容 = 备份数组（LBA 1953525135..1953525166）原样。
# 安全闸门：写前校验主/备数组各自 5 个真实分区项完全一致；写后回读 + CRC 复验。
# 严格不触碰：分区数据、ESP/BCD、备份 GPT 区、任何其他 LBA。
import ctypes, os, struct, sys, time, zlib

ROOT = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main"
LOG = ROOT + r"\_attic\vx-gpt-repair.rpt"
PD = r"\\.\PhysicalDrive0"
SECTOR = 512
PE_LBA, PE_NUM, PE_SIZE = 2, 128, 128
BK_PE_LBA = 1953525135
EXPECTED_PART_COUNT = 5
EXPECTED_CRC = 0xC4AE8CA4  # 头里登记的 entry-array CRC（备份区已实证匹配）

def crc32(b):
    return zlib.crc32(b) & 0xFFFFFFFF

def main():
    lines = []
    def say(m):
        lines.append(str(m)); print(m)

    # 与只读诊断同款通道：open()（内部 FILE_SHARE_READ|WRITE，不独占）
    f = open(PD, "r+b", buffering=0)
    try:
        with f:
            def pread(off, size):
                f.seek(off); return f.read(size)
            def pwrite(off, data):
                f.seek(off); n = f.write(data); f.flush()
                return n

            hdr = pread(SECTOR, SECTOR)[:92]
            pe_lba = struct.unpack_from("<Q", hdr, 72)[0]
            pe_num = struct.unpack_from("<I", hdr, 80)[0]
            pe_size = struct.unpack_from("<I", hdr, 84)[0]
            alt_lba = struct.unpack_from("<Q", hdr, 32)[0]
            say(f"pe_lba={pe_lba} n={pe_num} size={pe_size} alt={alt_lba}")
            if (pe_lba, pe_num, pe_size) != (PE_LBA, PE_NUM, PE_SIZE):
                raise SystemExit("!! GPT 参数与预期不符——中止")
            arr_p = pread(pe_lba * SECTOR, pe_num * pe_size)
            arr_b = pread(BK_PE_LBA * SECTOR, pe_num * pe_size)
            # 闸门1：两数组中真实分区项（type GUID 非零）必须逐字节一致
            real_p = [i for i in range(pe_num) if arr_p[i*pe_size:i*pe_size+16] != bytes(16)]
            real_b = [i for i in range(pe_num) if arr_b[i*pe_size:i*pe_size+16] != bytes(16)]
            say(f"real entries primary={real_p} backup={real_b}")
            if real_p != real_b or len(real_p) != EXPECTED_PART_COUNT:
                raise SystemExit("!! 真实分区项数量/位置不一致——中止")
            for i in real_p:
                if arr_p[i*pe_size:(i+1)*pe_size] != arr_b[i*pe_size:(i+1)*pe_size]:
                    raise SystemExit(f"!! entry {i} 主备不一致——中止")
            # 闸门2：备份区 CRC 必须等于头里登记值
            if crc32(arr_b) != EXPECTED_CRC:
                raise SystemExit("!! 备份区 CRC 与登记值不符——中止")
            say("gates ok — writing backup array -> primary array (LBA2..33)")
            n = pwrite(pe_lba * SECTOR, arr_b)
            say(f"written {n} bytes")
            # 回读复验
            arr_p2 = pread(pe_lba * SECTOR, pe_num * pe_size)
            say("post-write CRC calc=%08x stored=%08x %s" %
                (crc32(arr_p2), EXPECTED_CRC, "OK" if crc32(arr_p2) == EXPECTED_CRC else "!! FAIL"))
            if crc32(arr_p2) != EXPECTED_CRC:
                raise SystemExit("!! 写后复验失败——需人工介入")
            # 主备一致性终验
            say("primary==backup: " + str(arr_p2 == arr_b))
        say("GPT-REPAIR-DONE")
    finally:
        pass
    with open(LOG, "w", encoding="utf-8") as fh:
        fh.write("\n".join(lines) + "\n")

if __name__ == "__main__":
    if not ctypes.windll.shell32.IsUserAnAdmin():
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
        except BaseException:
            import traceback
            lines.append("EXCEPTION/ABORT:\n" + traceback.format_exc())
            with open(LOG, "w", encoding="utf-8") as fh:
                fh.write("\n".join(lines) + "\n")
        print("script-end")
