# 只读体检 v2：固件引导项（bcdedit）+ 两块盘 GPT 主/备头 CRC
# 全程只读。GPT 读取 \\.\PhysicalDriveN，无写入路径。
import subprocess, sys, uuid

def ps(cmd):
    r = subprocess.run(["powershell", "-NoProfile", "-c", cmd],
                       capture_output=True)
    out = (r.stdout or b"") + (r.stderr or b"")
    for enc in ("gbk", "utf-8"):
        try:
            return out.decode(enc)
        except UnicodeDecodeError:
            continue
    return out.decode("utf-8", "replace")

print("== firmware boot entries (bcdedit) ==")
print(ps("bcdedit /enum firmware"))

def crc32(data: bytes) -> int:
    import zlib
    return zlib.crc32(data) & 0xFFFFFFFF

# GPT header: LBA1 (primary) 与最后 LBA (backup)，各 92 字节有效
# 校验：header CRC32 覆盖 [0:92]，且 partition-entry-array CRC 在 offset 88
for n in (0, 1):
    path = f"\\\\.\\PhysicalDrive{n}"
    try:
        # 需要 admin；无权限则报错退出
        import win32file  # noqa
    except ImportError:
        pass
    try:
        h = open(path, "rb", buffering=0)
    except OSError as e:
        print(f"[disk {n}] open failed: {e}")
        continue
    with h:
        def pread(off, size):
            import msvcrt, ctypes
            ov = ctypes.create_string_buffer(8)
            # python 3.13 file read at offset via seek works with buffering=0 on win32? use os pread
            return os.pread(h.fileno(), size, off)
        sec = pread(512, 512)
        if sec[:8] != b"EFI PART":
            print(f"[disk {n}] LBA1 不是 GPT 头: {sec[:8]!r}")
            continue
        hdr = sec[:92]
        stored = struct.unpack_from("<I", hdr, 16)[0]
        calc = crc32(hdr[:92])
        backup_lba = struct.unpack_from("<Q", hdr, 32)[0]
        pe_lba = struct.unpack_from("<Q", hdr, 72)[0]
        pe_num = struct.unpack_from("<I", hdr, 80)[0]
        pe_size = struct.unpack_from("<I", hdr, 84)[0]
        pe_crc_stored = struct.unpack_from("<I", hdr, 88)[0]
        print(f"[disk {n}] primary header @LBA1: hdrCRC stored={stored:08x} calc={calc:08x} "
              f"{'OK' if stored==calc else '!! CRC MISMATCH !!'}")
        print(f"[disk {n}]   backup_lba={backup_lba} entries@LBA{pe_lba} n={pe_num} size={pe_size}")
        # 读分区表数组校验 CRC
        arr = pread(pe_lba*512, pe_num*pe_size)
        c = crc32(arr)
        print(f"[disk {n}]   entry-array CRC stored={pe_crc_stored:08x} calc={c:08x} "
              f"{'OK' if c==pe_crc_stored else '!! MISMATCH !!'}")
        # backup header
        total = pread(0, 512)  # MBR LBA0
        # 磁盘大小：从 volume 层拿不到 LBA 数，用 backup_lba 定位
        bh = pread(backup_lba*512, 512)
        if bh[:8] == b"EFI PART":
            stored2 = struct.unpack_from("<I", bh, 16)[0]
            calc2 = crc32(bh[:92])
            print(f"[disk {n}] backup header @LBA{backup_lba}: hdrCRC stored={stored2:08x} calc={calc2:08x} "
                  f"{'OK' if stored2==calc2 else '!! CRC MISMATCH !!'}")
            pe_lba2 = struct.unpack_from("<Q", bh, 72)[0]
            pe_crc_stored2 = struct.unpack_from("<I", bh, 88)[0]
            arr2 = pread(pe_lba2*512, pe_num*pe_size)
            c2 = crc32(arr2)
            print(f"[disk {n}]   backup entry-array CRC stored={pe_crc_stored2:08x} calc={c2:08x} "
                  f"{'OK' if c2==pe_crc_stored2 else '!! MISMATCH !!'}")
        else:
            print(f"[disk {n}] backup header 读取异常: {bh[:8]!r}")
