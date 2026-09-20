#!/usr/bin/env python3
"""UEFI 镜像交叉审计：用独立实现（pyfatfs）读我手写的 FAT32，判断是我写错
还是 OVMF 的引导项查找问题。全只读，不改镜像。

用法： python uefi-img-audit.py [镜像路径]
"""
import os
import struct
import sys
import tempfile

ESP_TYPE = bytes.fromhex("28732ac11ff8d211ba4b00a0c93ec93b")


def crc32(data: bytes) -> int:
    import zlib
    return zlib.crc32(data) & 0xFFFFFFFF


def read_gpt(path):
    with open(path, "rb") as f:
        mbr = f.read(512)
        f.seek(512)
        hdr = f.read(92)
        f.seek(512 * 2)
        ents = f.read(512 * 32)
    print("protective MBR type[0] = 0x%02x (期望 0xEE)" % mbr[446 + 4])
    sig, rev, hsz, hcrc, _res, my, alt, fu, lu = struct.unpack_from("<8sIIII QQQQ", hdr, 0)
    print("GPT sig      : %r (期望 b'EFI PART')" % sig)
    print("GPT rev      : 0x%x  hdrsize=%d" % (rev, hsz))
    print("MyLBA=%d AltLBA=%d first_usable=%d last_usable=%d" % (my, alt, fu, lu))
    elba, num, esz, ecrc = struct.unpack_from("<QIII", hdr, 72)
    print("entries LBA  : %d  num=%d size=%d crc=0x%08x" % (elba, num, esz, ecrc))
    # 头 CRC（置零后算）
    h = bytearray(hdr)
    struct.pack_into("<I", h, 16, 0)
    calc = crc32(bytes(h[:hsz]))
    print("header CRC   : stored=0x%08x calc=0x%08x -> %s"
          % (hcrc, calc, "OK" if hcrc == calc else "MISMATCH"))
    ents_calc = crc32(ents[:num * esz])
    print("entries CRC  : stored=0x%08x calc=0x%08x -> %s"
          % (ecrc, ents_calc, "OK" if ecrc == ents_calc else "MISMATCH"))

    parts = []
    for i in range(num):
        pe = ents[i * esz:(i + 1) * esz]
        if pe[0:16] == b"\0" * 16:
            continue
        tg = pe[0:16]
        first, last, attrs = struct.unpack_from("<QQQ", pe, 32)
        name = pe[56:128].decode("utf-16-le").rstrip("\0")
        parts.append((i, tg, first, last, attrs, name))
        print("part[%d] %s first=%d last=%d attrs=0x%x name=%r"
              % (i, "ESP" if tg == ESP_TYPE else tg.hex()[:8], first, last, attrs, name))
    return parts


def slice_part(path, first, last, outp):
    with open(path, "rb") as f:
        f.seek(first * 512)
        data = f.read((last - first + 1) * 512)
    with open(outp, "wb") as g:
        g.write(data)
    return len(data)


def bpb_report(buf):
    bps = struct.unpack_from("<H", buf, 11)[0]
    spc = buf[13]
    resv = struct.unpack_from("<H", buf, 14)[0]
    nfats = buf[16]
    media = buf[21]
    tot32 = struct.unpack_from("<I", buf, 32)[0]
    fsz32 = struct.unpack_from("<I", buf, 36)[0]
    rootclus = struct.unpack_from("<I", buf, 44)[0]
    fsinfo = struct.unpack_from("<H", buf, 48)[0]
    bkboot = struct.unpack_from("<H", buf, 50)[0]
    bootsig = buf[66]
    print("BPB: bps=%d spc=%d resv=%d nfats=%d media=0x%02x tot32=%d fatsz32=%d"
          % (bps, spc, resv, nfats, media, tot32, fsz32))
    print("     rootclus=%d fsinfo=%d bkboot=%d bootsig=0x%02x sig55aa=%r"
          % (rootclus, fsinfo, bkboot, bootsig, buf[510:512]))
    # FAT32 有效性判据
    ok = True
    if bps != 512:
        print("  !! bytes/sector 非 512"); ok = False
    if spc not in (1, 2, 4, 8, 16, 32, 64, 128):
        print("  !! sectors/cluster 非法"); ok = False
    if bootsig != 0x29:
        print("  !! 扩展引导签名非 0x29"); ok = False
    if buf[510:512] != b"\x55\xAA":
        print("  !! 无 0x55AA"); ok = False
    cluster_bytes = spc * bps
    data_start = resv + nfats * fsz32
    count = (tot32 - data_start) // spc
    print("     data_start=%d cluster_bytes=%d  cluster_count=%d (%s)"
          % (data_start, cluster_bytes, count,
             "FAT32 OK" if count >= 65525 else ("FAT16 区间(%d<65525)" % count)))
    if count < 65525:
        print("  !! 簇数不足 65525 —— 严格意义上这不是 FAT32，"
              "部分固件/驱动会按 FAT16 解析或拒绝")
        ok = False
    return ok, data_start, spc, bps


def main():
    path = sys.argv[1] if len(sys.argv) > 1 else os.path.join(
        os.path.dirname(__file__), "..", "varix-uefi.img")
    path = os.path.abspath(path)
    print("=== 镜像: %s (%d bytes) ===" % (path, os.path.getsize(path)))
    if os.path.getsize(path) % 512:
        print("!! 镜像大小不是 512 的整数倍 —— 备用 GPT 头位置可能错位")
    parts = read_gpt(path)
    esp = [p for p in parts if p[1] == ESP_TYPE]
    if not esp:
        print("!! 没找到 ESP 分区，放弃")
        return 1
    i, _tg, first, last, attrs, name = esp[0]
    print("\n=== ESP: LBA %d..%d (%d 扇区, %d MB) ==="
          % (first, last, last - first + 1, (last - first + 1) * 512 // 1024 // 1024))

    tmp = os.path.join(tempfile.gettempdir(), "esp-slice.bin")
    n = slice_part(path, first, last, tmp)
    print("切片落盘: %s (%d bytes)" % (tmp, n))

    with open(tmp, "rb") as f:
        head = f.read(4096)
    ok, data_start, spc, bps = bpb_report(head)

    # ---- 独立实现交叉验证 ----
    print("\n=== 独立实现（pyfatfs）交叉验证 ===")
    try:
        from pyfatfs.PyFatFS import PyFatFS
        # PyFatFS 是 PyFilesystem2 的 FS 子类：构造即挂载，不需要 .open()，
        # 也不接受 BytesIO（只吃路径）。切片落盘正是为了绕开这个限制。
        fs = PyFatFS(tmp, offset=0, read_only=True)
        try:
            def walk(d, depth=0):
                for e in fs.listdir(d):
                    full = (d.rstrip("/") + "/" + e) if d != "/" else "/" + e
                    info = ""
                    try:
                        st = fs.stat(full)
                        info = " (%d bytes)" % getattr(st, "st_size", -1)
                    except Exception:
                        pass
                    print("  " + "  " * depth + full + info)
                    try:
                        if fs.isdir(full) and depth < 3:
                            walk(full, depth + 1)
                    except Exception:
                        pass
            walk("/")
        finally:
            try:
                fs.close()
            except Exception:
                pass
        print("pyfatfs: 挂载成功 —— FAT32 结构是合规的")
        print("结论：问题不在 FAT32，而在 OVMF 的引导项/设备路径枚举")
    except Exception as ex:
        print("pyfatfs 失败: %r" % (ex,))
        print("结论：FAT32 可能不合规 —— 需修 build-hdd.py")

    # ---- 根目录原始目录项转储（判断 LFN 是否装反）----
    print("\n=== 根目录前 8 个目录项（原始）===")
    with open(tmp, "rb") as f:
        f.seek(data_start * bps)
        raw = f.read(8 * 32)
    for k in range(8):
        e = raw[k * 32:(k + 1) * 32]
        if e[0] == 0x00:
            print("  [%d] <空/结束>" % k)
            continue
        attr = e[11]
        if attr == 0x0F:
            seq = e[0] & 0x3F
            last = bool(e[0] & 0x40)
            part = e[1:11].decode("utf-16-le", "replace") + \
                e[14:26].decode("utf-16-le", "replace") + \
                e[28:32].decode("utf-16-le", "replace")
            print("  [%d] LFN seq=%d last=%s chk=0x%02x : %r"
                  % (k, seq, last, e[13], part.replace("\uffff", "~").rstrip("\x00")))
        else:
            print("  [%d] 8.3 name=%r attr=0x%02x clus=%d size=%d"
                  % (k, e[0:11], attr,
                     struct.unpack_from("<H", e, 26)[0],
                     struct.unpack_from("<I", e, 28)[0]))
    return 0


if __name__ == "__main__":
    sys.exit(main())
