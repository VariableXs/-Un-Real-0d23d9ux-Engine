#!/usr/bin/env python3
r"""把新编译的内核 ELF 刷进 UEFI 测试盘的 ESP（/_attic/varix-uefi.img）。

为什么不用 pyfatfs：本机镜像里的 FAT32 是手写生成的，pyfatfs 的**写入器**
把它的簇链判成 "FREE_CLUSTER mark found in FAT cluster chain" 并拒绝写
（读取却正常——两者的簇链遍历口径不一致）。与其猜它错在哪，不如自己按
微软 FAT32 规范做外科式原地写入：完全掌控簇链，写完立刻按同一套逻辑回读
比对，不一致就报错退出。

流程：
  1. 备份镜像到 _attic/esp-backup/（失败可回滚）；
  2. 解析 GPT → 按类型 GUID 找 ESP → 算分区字节偏移；
  3. 解析 BPB → 定位 FAT 表与数据区；
  4. 遍历根目录 → KERNEL 目录 → VARIX 目录项，取出首簇与文件大小；
  5. 沿 FAT 走簇链，把新内核按簇写入（不够则从未使用的 FAT 项里追加分配，
     多余则截断并把尾巴标回空闲）；
  6. 回写 FAT + 目录项文件大小，**重新读一遍做 sha256 逐字节比对**。

用法：python _attic/tools/uefi-img-refresh-kernel.py [--dry-run] [内核路径]
默认内核：kernel/target/x86_64-unknown-none/release/varix
"""
import hashlib
import os
import shutil
import struct
import sys
import time

ESP_TYPE = bytes.fromhex("28732ac11ff8d211ba4b00a0c93ec93b")
ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
IMG = os.path.join(ROOT, "_attic", "varix-uefi.img")
BACKUP_DIR = os.path.join(ROOT, "_attic", "esp-backup")
KERNEL = os.path.join(ROOT, "kernel", "target", "x86_64-unknown-none", "release", "varix")

EOF_MARK = 0x0FFFFFF8
BAD_MARK = 0x0FFFFFF7


class Fat32:
    """最小 FAT32 读写器：只做本任务需要的三件事（定位文件/写簇/回读）。"""

    def __init__(self, path, offset):
        self.f = open(path, "r+b")
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
        off = self.fat0 + c * 4
        return struct.unpack_from("<I", self.read(off, 4), 0)[0] & 0x0FFFFFFF

    def fat_put(self, c, v):
        # 只改低 28 位，高 4 位保留（规范：FAT32 高 4 位保留）
        cur = struct.unpack_from("<I", self.read(self.fat0 + c * 4, 4), 0)[0]
        nv = (cur & 0xF0000000) | (v & 0x0FFFFFFF)
        self.write(self.fat0 + c * 4, struct.pack("<I", nv))
        # 第二份 FAT 同步（冗余表不一致会让某些固件读到脏链）
        if self.nfats > 1:
            second = self.fat0 + self.fat_sz * self.bps
            self.write(second + c * 4, struct.pack("<I", nv))

    def chain(self, start):
        out = []
        c = start
        seen = set()
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
        """返回 [(name, attr, first_cluster, size, entry_off)]（跳过 LFN 槽）。"""
        items = []
        for c in self.chain(start):
            buf = self.read(self.cluster_off(c), self.csz)
            for i in range(0, len(buf), 32):
                e = buf[i:i + 32]
                if len(e) < 32 or e[0] == 0x00:
                    continue
                attr = e[11]
                if attr == 0x0F:  # LFN
                    continue
                if e[0] == 0xE5:  # 已删除
                    continue
                name = e[0:11].decode("ascii", errors="replace").strip()
                fc = struct.unpack_from("<H", e, 26)[0] | (struct.unpack_from("<H", e, 20)[0] << 16)
                size = struct.unpack_from("<I", e, 28)[0]
                items.append((name, attr, fc, size, self.cluster_off(c) + i))
        return items

    def find(self, start, name):
        for it in self.read_dir(start):
            if it[0].upper() == name.upper():
                return it
        return None


def find_esp(path):
    with open(path, "rb") as f:
        f.seek(512)
        hdr = f.read(92)
        sig, _rev, hsz, _c, _r, _m, _a, _fu, _lu = struct.unpack_from("<8sIIII QQQQ", hdr, 0)
        if sig != b"EFI PART":
            raise SystemExit("不是 GPT 镜像: %r" % sig)
        elba, num, esz, _e = struct.unpack_from("<QIII", hdr, 72)
        f.seek(elba * 512)
        ents = f.read(num * esz)
    for i in range(num):
        pe = ents[i * esz:(i + 1) * esz]
        if pe[0:16] == b"\0" * 16:
            continue
        if pe[0:16] == ESP_TYPE:
            first, last = struct.unpack_from("<QQ", pe, 32)
            return first, last
    raise SystemExit("镜像里没有 ESP 分区")


def sha(b):
    return hashlib.sha256(b).hexdigest()


def main() -> int:
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    dry = "--dry-run" in sys.argv[1:]
    src = args[0] if args else KERNEL
    if not os.path.isfile(src):
        raise SystemExit("内核不存在: " + src)

    with open(src, "rb") as f:
        data = f.read()
    print(f"源内核: {len(data)} bytes sha256={sha(data)[:16]}…")

    first, last = find_esp(IMG)
    off = first * 512
    print(f"ESP: lba {first}..{last} offset={off}")
    fs = Fat32(IMG, off)
    print(f"FAT32: bps={fs.bps} spc={fs.spc} 簇={fs.csz}B resv={fs.resv} "
          f"nfats={fs.nfats} fat_sz={fs.fat_sz} root={fs.root}")

    d = fs.find(fs.root, "KERNEL")
    if d is None:
        raise SystemExit("ESP 根目录里没有 KERNEL（根: %r）" % [i[0] for i in fs.read_dir(fs.root)])
    entry = fs.find(d[2], "VARIX")
    if entry is None:
        raise SystemExit("KERNEL 目录里没有 VARIX（内容: %r）" % [i[0] for i in fs.read_dir(d[2])])
    _n, _a, fclu, osize, eoff = entry
    chain = fs.chain(fclu)
    cap = len(chain) * fs.csz
    print(f"目标 /KERNEL/VARIX: 首簇={fclu} 簇数={len(chain)} 容量={cap}B 现大小={osize}B")

    need = (len(data) + fs.csz - 1) // fs.csz
    if dry:
        print(f"[dry-run] 需要簇数={need} 现有={len(chain)} → "
              f"{'够，原地覆写' if need <= len(chain) else '不够，需追加分配'}")
        return 0

    # 备份
    os.makedirs(BACKUP_DIR, exist_ok=True)
    stamp = time.strftime("%Y%m%d-%H%M%S")
    bak = os.path.join(BACKUP_DIR, f"varix-uefi-{stamp}.img")
    fs.f.close()
    shutil.copy2(IMG, bak)
    print(f"备份: {bak}")
    fs = Fat32(IMG, off)

    # 补足簇：从 FAT 里找全 0 的空闲项
    if need > len(chain):
        taken = set(chain)
        scan = 2
        while len(chain) < need:
            while scan in taken or fs.fat_get(scan) != 0:
                scan += 1
                if scan >= BAD_MARK:
                    raise SystemExit("FAT 里找不到足够的空闲簇")
            prev = chain[-1]
            fs.fat_put(prev, scan)
            chain.append(scan)
            taken.add(scan)
            scan += 1
        fs.fat_put(chain[-1], EOF_MARK)
        print(f"追加簇到 {len(chain)} 个")

    # 写数据（按簇对齐铺开）
    for i, c in enumerate(chain):
        chunk = data[i * fs.csz:(i + 1) * fs.csz]
        if not chunk and i * fs.csz >= len(data):
            break
        fs.write(fs.cluster_off(c), chunk + b"\x00" * (fs.csz - len(chunk)))

    # 截断多余簇：新尾簇标 EOF，其余标空闲
    used = chain[:need]
    if len(used) < len(chain):
        for c in chain[len(used):]:
            fs.fat_put(c, 0)
    fs.fat_put(used[-1], EOF_MARK)

    # 更新目录项里的文件大小
    cur = bytearray(fs.read(eoff, 32))
    struct.pack_into("<I", cur, 28, len(data))
    fs.write(eoff, bytes(cur))
    fs.f.flush()
    os.fsync(fs.f.fileno())

    # 回读校验（重新打开，走同一套簇链逻辑）
    fs.f.close()
    v = Fat32(IMG, off)
    e2 = v.find(v.find(v.root, "KERNEL")[2], "VARIX")
    back = b"".join(v.read(v.cluster_off(c), v.csz) for c in v.chain(e2[2]))[: len(data)]
    v.f.close()
    print(f"回读: {len(back)} bytes sha256={sha(back)[:16]}…")
    if sha(back) != sha(data):
        print("!!! 校验失败：镜像内容与源内核不一致 —— 请从备份回滚", file=sys.stderr)
        return 1
    print("OK: 镜像内内核已更新并与源逐字节一致")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
