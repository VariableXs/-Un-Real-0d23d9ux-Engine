#!/usr/bin/env python3
r"""ESP 镜像里的 FAT32 文件读写（自写最小实现，不依赖 pyfatfs）。

为什么自写：本机 `_attic/varix-uefi.img` 里的 FAT32 是手写生成的，pyfatfs
的**写入器**把它的簇链判成 "FREE_CLUSTER mark found in FAT cluster chain"
而拒绝写（读取却正常）。与其猜它错在哪，不如按微软 FAT32 规范自己走簇链：
完全掌控、写完立刻回读比对，不一致就报错退出。

用法：
  python esp-fat.py list  <img> [img_path]        列目录
  python esp-fat.py get   <img> <img_path> <out>  取文件到本地
  python esp-fat.py put   <img> <img_path> <src>  把本地文件写回镜像

镜像路径大小写不敏感（镜像里实为 /KERNEL/VARIX 全大写）；GPT 里按类型
GUID 自动找 ESP 分区。写入前自动备份到 _attic/esp-backup/。
"""
import hashlib
import os
import shutil
import struct
import sys
import time

ESP_TYPE = bytes.fromhex("28732ac11ff8d211ba4b00a0c93ec93b")
ATTIC = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BACKUP_DIR = os.path.join(ATTIC, "esp-backup")
EOF_MARK = 0x0FFFFFF8
BAD_MARK = 0x0FFFFFF7


def _lfn_slot(e):
    """从一个 LFN 槽（32B 目录项）取出它的 13 个 UTF-16LE 字符。

    布局：字节 1..10 = 字符 1..5；13 位保留；14..25 = 字符 6..11；
    26..27 保留；28..31 = 字符 12..13。0x0000/0xFFFF 是填充，截断即止。
    """
    raw = e[1:11] + e[14:26] + e[28:32]
    out = []
    for i in range(0, len(raw), 2):
        u = raw[i] | (raw[i + 1] << 8)
        if u in (0x0000, 0xFFFF):
            break
        out.append(chr(u))
    return "".join(out)


class Fat32:
    """最小 FAT32 读写器：定位文件 / 写簇 / 回读。"""

    def __init__(self, path, offset, writable=False):
        self.f = open(path, "r+b" if writable else "rb")
        self.base = offset
        b = self.read(0, 512)
        self.bps = struct.unpack_from("<H", b, 11)[0]
        self.spc = b[13]
        self.resv = struct.unpack_from("<H", b, 14)[0]
        self.nfats = b[16]
        self.fat_sz = struct.unpack_from("<I", b, 36)[0]
        self.root = struct.unpack_from("<I", b, 44)[0]
        self.fat0 = self.resv * self.bps
        self.data0 = self.fat0 + self.nfats * self.fat_sz * self.bps
        self.csz = self.spc * self.bps

    def read(self, off, n):
        self.f.seek(self.base + off)
        return self.f.read(n)

    def write(self, off, data):
        self.f.seek(self.base + off)
        self.f.write(data)

    def cluster_off(self, c):
        return self.data0 + (c - 2) * self.csz

    def fat_get(self, c):
        return struct.unpack_from("<I", self.read(self.fat0 + c * 4, 4), 0)[0] & 0x0FFFFFFF

    def fat_put(self, c, v):
        cur = struct.unpack_from("<I", self.read(self.fat0 + c * 4, 4), 0)[0]
        nv = (cur & 0xF0000000) | (v & 0x0FFFFFFF)
        self.write(self.fat0 + c * 4, struct.pack("<I", nv))
        if self.nfats > 1:  # 冗余表同步：不一致会让某些固件读到脏链
            self.write(self.fat0 + self.fat_sz * self.bps + c * 4, struct.pack("<I", nv))

    def chain(self, start):
        out, seen, c = [], set(), start
        while 2 <= c < BAD_MARK:
            if c in seen:
                raise SystemExit(f"FAT 簇链成环 @ {c}")
            seen.add(c)
            out.append(c)
            nxt = self.fat_get(c)
            if nxt >= EOF_MARK:
                break
            c = nxt
        return out

    def read_dir(self, start):
        """返回 [(long_name, short_name, attr, first_cluster, size, entry_off)]。

        LFN（长文件名）槽按微软规范重建：物理上第一条装名字**尾部**（序号最大、
        带 0x40 结束位），序号 1 那条紧跟在 8.3 目录项之前——所以拼接要按
        物理顺序**倒过来**。踩过的坑：不重建时 `limine.conf` 只能看到
        8.3 短名 `LIMINE~1CON`，按真实文件名查找会查不到。
        """
        items = []
        lfn = []
        for c in self.chain(start):
            buf = self.read(self.cluster_off(c), self.csz)
            for i in range(0, len(buf), 32):
                e = buf[i:i + 32]
                if len(e) < 32 or e[0] == 0x00:  # 0x00 = 本目录再没有条目
                    return items
                if e[0] == 0xE5:  # 已删除：连同其 LFN 一起丢
                    lfn = []
                    continue
                attr = e[11]
                if attr == 0x0F:  # LFN 槽
                    lfn.append(e)
                    continue
                short = e[0:11].decode("ascii", errors="replace").strip()
                long = "".join(_lfn_slot(s) for s in reversed(lfn)) if lfn else ""
                fc = struct.unpack_from("<H", e, 26)[0] | (struct.unpack_from("<H", e, 20)[0] << 16)
                size = struct.unpack_from("<I", e, 28)[0]
                items.append((long or short, short, attr, fc, size, self.cluster_off(c) + i))
                lfn = []
        return items

    def find(self, start, name):
        """按长名或短名查找（大小写不敏感）。"""
        want = name.upper()
        for it in self.read_dir(start):
            long, short = it[0], it[1]
            if long.upper() == want or short.upper().rstrip(". ") == want.rstrip(". "):
                return it
        return None

    def resolve(self, path):
        """把 /a/b 解析成目录项（大小写不敏感、长名/短名都认）。"""
        cur = self.root
        parts = [p for p in path.replace("\\", "/").split("/") if p]
        ent = None
        for i, p in enumerate(parts):
            ent = self.find(cur, p)
            if ent is None:
                raise SystemExit("镜像里找不到 " + path + f"（在 {p} 处断）")
            if i < len(parts) - 1:
                if ent[2] & 0x10 == 0:
                    raise SystemExit(p + " 不是目录")
                cur = ent[3]
        return ent

    def read_file(self, path):
        _l, _s, _a, fc, size, _o = self.resolve(path)
        data = b"".join(self.read(self.cluster_off(c), self.csz) for c in self.chain(fc))
        return data[:size]

    def write_file(self, path, data):
        """覆写已存在的文件（不够则从空闲 FAT 项追加分配）。"""
        _l, _s, _a, fclu, _osize, eoff = self.resolve(path)
        chain = self.chain(fclu)
        need = (len(data) + self.csz - 1) // self.csz
        if need == 0:
            need = 0
        if need > len(chain):
            taken = set(chain)
            scan = 2
            while len(chain) < need:
                while scan in taken or self.fat_get(scan) != 0:
                    scan += 1
                    if scan >= BAD_MARK:
                        raise SystemExit("FAT 里找不到足够的空闲簇")
                self.fat_put(chain[-1], scan)
                chain.append(scan)
                taken.add(scan)
                scan += 1
        for i, c in enumerate(chain):
            if i >= need:
                break
            chunk = data[i * self.csz:(i + 1) * self.csz]
            self.write(self.cluster_off(c), chunk + b"\x00" * (self.csz - len(chunk)))
        used = chain[:need]
        if need == 0:
            self.fat_put(fclu, EOF_MARK)
            used = [fclu]
        else:
            for c in chain[need:]:
                self.fat_put(c, 0)
            self.fat_put(used[-1], EOF_MARK)
        cur = bytearray(self.read(eoff, 32))
        struct.pack_into("<I", cur, 28, len(data))
        self.write(eoff, bytes(cur))
        self.f.flush()
        os.fsync(self.f.fileno())


def find_esp(path):
    with open(path, "rb") as f:
        f.seek(512)
        hdr = f.read(92)
        sig, _r, _hs, _hc, _rs, _my, _al, _fu, _lu = struct.unpack_from("<8sIIII QQQQ", hdr, 0)
        if sig != b"EFI PART":
            raise SystemExit("不是 GPT 镜像: %r" % sig)
        elba, num, esz, _e = struct.unpack_from("<QIII", hdr, 72)
        f.seek(elba * 512)
        ents = f.read(num * esz)
    for i in range(num):
        pe = ents[i * esz:(i + 1) * esz]
        if pe[0:16] == ESP_TYPE:
            first, _last = struct.unpack_from("<QQ", pe, 32)
            return first
    raise SystemExit("镜像里没有 ESP 分区")


def sha(b):
    return hashlib.sha256(b).hexdigest()


def main() -> int:
    if len(sys.argv) < 3:
        print(__doc__)
        return 2
    cmd, img = sys.argv[1], sys.argv[2]
    off = find_esp(img) * 512

    if cmd == "list":
        fs = Fat32(img, off)
        base = sys.argv[3] if len(sys.argv) > 3 else "/"
        ent = None if base == "/" else fs.resolve(base)
        cur = fs.root if ent is None else ent[3]
        for long, short, a, _fc, size, _o in fs.read_dir(cur):
            tag = "  [D] " if a & 0x10 else "      "
            extra = "" if long == short else f"  (短名 {short})"
            print(tag + long + ("" if a & 0x10 else f"  ({size} B)") + extra)
        fs.f.close()
        return 0

    if cmd == "get":
        if len(sys.argv) < 5:
            print("用法: get <img> <img_path> <out>")
            return 2
        fs = Fat32(img, off)
        data = fs.read_file(sys.argv[3])
        fs.f.close()
        with open(sys.argv[4], "wb") as f:
            f.write(data)
        print(f"GET {sys.argv[3]} -> {sys.argv[4]}  {len(data)} B  sha256={sha(data)[:16]}…")
        return 0

    if cmd == "put":
        if len(sys.argv) < 5:
            print("用法: put <img> <img_path> <src>")
            return 2
        with open(sys.argv[4], "rb") as f:
            data = f.read()
        os.makedirs(BACKUP_DIR, exist_ok=True)
        bak = os.path.join(BACKUP_DIR, os.path.basename(img) + "-" + time.strftime("%Y%m%d-%H%M%S") + ".img")
        shutil.copy2(img, bak)
        print(f"备份: {bak}")
        fs = Fat32(img, off, writable=True)
        old = fs.read_file(sys.argv[3])
        fs.write_file(sys.argv[3], data)
        fs.f.close()
        v = Fat32(img, off)
        back = v.read_file(sys.argv[3])
        v.f.close()
        print(f"PUT {sys.argv[4]} -> {sys.argv[3]}")
        print(f"  旧 {len(old)} B -> 新 {len(back)} B  sha256={sha(back)[:16]}…")
        if sha(back) != sha(data) or len(back) != len(data):
            print("!!! 校验失败：写入内容与源不一致 —— 请从备份回滚", file=sys.stderr)
            return 1
        print("OK: 逐字节一致")
        return 0

    print("未知命令 " + cmd)
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
