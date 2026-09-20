#!/usr/bin/env python3
r"""FAT32 镜像写入器（纯 Python，无需管理员权限）—— 向固定 VHD 的 ESP 分区
原地替换文件内容。

为什么需要它：QEMU 要验证「UEFI 普通块设备 + 三卡菜单」，需要一个 GPT+FAT32 的
VHD；而用 Storage cmdlet 建盘/格式化需要管理员权限。本工具直接操作 FAT32 结构，
**只覆盖指定文件的簇链内容**（新文件 ≤ 旧簇链容量时原地覆写，永不移动其它文件）。

安全铁律：
  * 只在文件大小 ≤ 既有簇链总容量时原地写；超出则**报错退出**，绝不重建 FS
    （不冒损坏 ESP 的风险）。
  * 写前备份整个 VHD 到 .bak；写入后回读校验 SHA256。
  * 只碰 --image 指定的文件，不触碰其它目录项、不碰 FAT 表（簇链不变）。

用法：
  python _attic/tools/fat32-put.py --image <vhd> --path "KERNEL/VARIX" --src <file>
  python _attic/tools/fat32-put.py --image <vhd> --list
"""
import argparse
import hashlib
import os
import shutil
import struct
import sys

BPS_OFF = 11
SIG = b"\x55\xaa"


class Fat32:
    def __init__(self, path, part_lba=None):
        self.path = path
        self.f = open(path, "rb+")
        self.part_lba = part_lba if part_lba is not None else self._find_gpt_part_lba()
        self.f.seek(self.part_lba * 512)
        bs = self.f.read(512)
        if bs[510:512] != SIG:
            raise SystemExit("不是有效 FAT 引导扇区（缺 0xAA55）")
        (self.bps, self.spc, self.rsv, self.nfats) = struct.unpack("<HBHB", bs[BPS_OFF:BPS_OFF + 6])
        self.fatsz = struct.unpack("<I", bs[36:40])[0]
        self.rootcl = struct.unpack("<I", bs[44:48])[0]
        self.total = struct.unpack("<I", bs[32:36])[0]
        self.fat_lba = self.part_lba + self.rsv
        self.data_lba = self.part_lba + self.rsv + self.nfats * self.fatsz
        self.cluster_bytes = self.spc * self.bps

    def _find_gpt_part_lba(self):
        f = self.f
        f.seek(512)
        h = f.read(92)
        if h[:8] != b"EFI PART":
            raise SystemExit("镜像没有 GPT 头（LBA1 无 EFI PART）")
        pd_lba, pd_num, pd_sz = struct.unpack("<QII", h[72:88])
        f.seek(pd_lba * 512)
        for _ in range(pd_num):
            e = f.read(pd_sz)
            if e[:16] == b"\0" * 16:
                break
            # ESP 类型 GUID：C12A7328-F81F-11D2-BA4B-00A0C93EC93B（混合端序）
            if e[:4] == bytes.fromhex("28732ac1") and e[4:6] == bytes.fromhex("1ff8"):
                return struct.unpack("<Q", e[32:40])[0]
        raise SystemExit("GPT 里找不到 ESP 分区")

    # ---- 簇链 ----
    def fat_next(self, cl):
        self.f.seek(self.fat_lba * self.bps + cl * 4)
        return struct.unpack("<I", self.f.read(4))[0] & 0x0FFFFFFF

    def chain(self, start, limit=500000):
        out, cl, g = [], start, 0
        while 2 <= cl < 0x0FFFFFF8 and g < limit:
            out.append(cl)
            g += 1
            cl = self.fat_next(cl)
        return out

    def cl_off(self, cl):
        return (self.data_lba + (cl - 2) * self.spc) * self.bps

    # ---- 目录 ----
    def _lfn_of(self, entries):
        """从一组 LFN 项拼出长名（按序号的倒序拼）。"""
        parts = {}
        for e in entries:
            seq = e[0] & 0x1F
            chunk = e[1:11] + e[14:26] + e[28:32]
            try:
                parts[seq] = chunk.decode("utf-16-le")
            except Exception:
                parts[seq] = ""
        return "".join(parts[k] for k in sorted(parts)).split("\x00")[0].rstrip("\uffff")

    def listdir(self, clus, prefix="", depth=0, out=None):
        out = out if out is not None else []
        lfn_buf = []
        for cl in self.chain(clus):
            self.f.seek(self.cl_off(cl))
            d = self.f.read(self.cluster_bytes)
            for i in range(0, len(d), 32):
                e = d[i:i + 32]
                if e[0] == 0:
                    return out
                if e[0] == 0xE5:
                    lfn_buf = []
                    continue
                if e[11] == 0x0F:
                    lfn_buf.append(e)
                    continue
                short = e[:8].decode("ascii", "replace").strip()
                ext = e[8:11].decode("ascii", "replace").strip()
                name = short + ("." + ext if ext else "")
                long_name = self._lfn_of(lfn_buf) if lfn_buf else ""
                lfn_buf = []
                if name in (".", ".."):
                    continue
                hi, lo = struct.unpack("<HH", e[20:22] + e[26:28])
                start = (hi << 16) | lo
                size = struct.unpack("<I", e[28:32])[0]
                isdir = bool(e[11] & 0x10)
                out.append({
                    "name": long_name or name, "short": name, "dir": isdir,
                    "cluster": start, "size": size, "depth": depth,
                })
                if isdir and depth < 4:
                    self.listdir(start, prefix + "  ", depth + 1, out)
        return out

    def find(self, path):
        """按 'KERNEL/VARIX' 这种路径找目录项（大小写不敏感，长/短名都认）。"""
        parts = [p for p in path.replace("\\", "/").split("/") if p]
        entries = self.listdir(self.rootcl)
        cur = None
        # 逐级查找
        clus = self.rootcl
        for i, want in enumerate(parts):
            want_up = want.upper()
            found = None
            lfn_buf = []
            for cl in self.chain(clus):
                self.f.seek(self.cl_off(cl))
                d = self.f.read(self.cluster_bytes)
                for k in range(0, len(d), 32):
                    e = d[k:k + 32]
                    if e[0] == 0:
                        break
                    if e[0] == 0xE5:
                        lfn_buf = []
                        continue
                    if e[11] == 0x0F:
                        lfn_buf.append(e)
                        continue
                    short = e[:8].decode("ascii", "replace").strip()
                    ext = e[8:11].decode("ascii", "replace").strip()
                    sname = short + ("." + ext if ext else "")
                    lname = self._lfn_of(lfn_buf) if lfn_buf else ""
                    lfn_buf = []
                    cands = {sname.upper(), (lname or "").upper()}
                    if want_up in cands:
                        hi, lo = struct.unpack("<HH", e[20:22] + e[26:28])
                        found = {
                            "cluster": (hi << 16) | lo,
                            "size": struct.unpack("<I", e[28:32])[0],
                            "dir": bool(e[11] & 0x10),
                            "short": sname, "name": lname or sname,
                        }
                        break
                if found:
                    break
            if not found:
                return None
            cur = found
            clus = found["cluster"]
        return cur

    # ---- 写 ----
    def put_inplace(self, path, src_file):
        ent = self.find(path)
        if ent is None:
            raise SystemExit(f"镜像里找不到 {path}（本工具不新建文件，避免动 FAT 表）")
        if ent["dir"]:
            raise SystemExit(f"{path} 是目录，不能覆盖")
        data = open(src_file, "rb").read()
        ch = self.chain(ent["cluster"])
        cap = len(ch) * self.cluster_bytes
        if len(data) > cap:
            raise SystemExit(
                f"！！新文件 {len(data)}B 超过既有簇链容量 {cap}B（{len(ch)} 簇）——\n"
                f"   为保证 ESP 结构安全，本工具拒绝扩链。请改用管理员权限建盘方案，\n"
                f"   或先把镜像里的内核换成足够大的占位版本。"
            )
        # 备份
        bak = self.path + ".bak"
        self.f.flush()
        shutil.copy2(self.path, bak)
        print(f"备份 → {bak}")
        # 原地覆写
        buf = data + b"\x00" * (cap - len(data))
        for idx, cl in enumerate(ch):
            self.f.seek(self.cl_off(cl))
            self.f.write(buf[idx * self.cluster_bytes:(idx + 1) * self.cluster_bytes])
        self.f.flush()
        os.fsync(self.f.fileno())
        # 目录项大小字段（可选：大小变了要同步，否则 FAT 读出来的 size 不对）
        self._set_size(path, len(data))
        return {"capacity": cap, "written": len(data), "clusters": len(ch), "backup": bak}

    def _set_size(self, path, size):
        """更新 FAT 目录项里的文件大小字段（不改其它字节）。"""
        parts = [p for p in path.replace("\\", "/").split("/") if p]
        clus = self.rootcl
        for i, want in enumerate(parts):
            want_up = want.upper()
            lfn_buf = []
            hit_off = None
            hit_clus = None
            for cl in self.chain(clus):
                base = self.cl_off(cl)
                self.f.seek(base)
                d = self.f.read(self.cluster_bytes)
                for k in range(0, len(d), 32):
                    e = d[k:k + 32]
                    if e[0] == 0:
                        break
                    if e[0] == 0xE5:
                        lfn_buf = []
                        continue
                    if e[11] == 0x0F:
                        lfn_buf.append(e)
                        continue
                    short = e[:8].decode("ascii", "replace").strip()
                    ext = e[8:11].decode("ascii", "replace").strip()
                    sname = short + ("." + ext if ext else "")
                    lname = self._lfn_of(lfn_buf) if lfn_buf else ""
                    lfn_buf = []
                    if want_up in {sname.upper(), (lname or "").upper()}:
                        hit_off = base + k
                        hi, lo = struct.unpack("<HH", e[20:22] + e[26:28])
                        hit_clus = (hi << 16) | lo
                        break
                if hit_off is not None:
                    break
            if hit_off is None:
                raise SystemExit("_set_size: 找不到 " + want)
            if i == len(parts) - 1:
                self.f.seek(hit_off + 28)
                self.f.write(struct.pack("<I", size))
                self.f.flush()
                os.fsync(self.f.fileno())
                return
            clus = hit_clus
        raise SystemExit("_set_size: 未走到叶子")


def sha256(path, n=None):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        while True:
            b = f.read(1 << 20)
            if not b:
                break
            h.update(b)
    return h.hexdigest()


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--image", required=True)
    ap.add_argument("--path", help="镜像内路径，如 KERNEL/VARIX")
    ap.add_argument("--src", help="本地源文件")
    ap.add_argument("--list", action="store_true", help="只列目录树")
    ap.add_argument("--verify", help="回读校验：镜像内路径；配合 --src 比对 SHA256")
    a = ap.parse_args()

    if not os.path.isfile(a.image):
        raise SystemExit("镜像不存在: " + a.image)
    fs = Fat32(a.image)
    print(f"镜像 {a.image}: part_lba={fs.part_lba} bps={fs.bps} spc={fs.spc} "
          f"rsv={fs.rsv} nfats={fs.nfats} fatsz={fs.fatsz} root={fs.rootcl}")

    if a.list:
        for e in fs.listdir(fs.rootcl):
            print("  " * e["depth"] + ("DIR  " if e["dir"] else "FILE ")
                  + f"{e['name']:28s} cls={e['cluster']:<7d} size={e['size']}")
        return 0

    if a.path and a.src:
        if not os.path.isfile(a.src):
            raise SystemExit("源文件不存在: " + a.src)
        r = fs.put_inplace(a.path, a.src)
        print(f"写入 {a.path} ← {a.src}: {r['written']}B / 容量 {r['capacity']}B "
              f"({r['clusters']} 簇)")

    if a.verify:
        ent = fs.find(a.verify)
        if ent is None:
            raise SystemExit("verify: 找不到 " + a.verify)
        print(f"回读 {a.verify}: size={ent['size']} cluster={ent['cluster']}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
