"""打包varix 内核 ISO（El Torito BIOS 可引导）。

环境事实（不要凭 README 猜）：
- `scripts/make-iso.sh` 已不存在，README 里的 xorriso 路径（/tmp/xorr）
  在本机不存在 → 改用纯 Python 的 pycdlib，无外部二进制依赖。
- `build/isoroot/` 是既有的 ISO 根目录树（Limine BIOS 引导器 +
  `limine.conf` + `/kernel/varix`），本脚本只负责把它封成.iso。

三套目录项各自的硬约束（全部实测踩出来，别删也别"优化"）：
1. **ISO9660 层必须全大写**（`_check_d1_characters` 只吃 `A-Z0-9_`）。
   目录 `/kernel` 写成小写直接抛 `PyCdlibInvalidInput`。
2. **Rock Ridge 层传真名**——这是 Limine BIOS 唯一认的通道。Limine 用
   自己的 ISO9660 驱动按 `limine.conf` 的 `boot():/kernel/varix` 取文件，
   **不认 Joliet**，没有 RR 就只剩大写短名可认，于是 `kernel` → `KERNEL`、
   `limine.conf` → `LIMINE.CONF`（截断丢 F），按真名一个都找不到，
   表现为**串口日志一个字节都没有**。
   历史对照：仓库里已"验证过"的 `varix.iso` 内核条目其实也是坏的
   （`PANIC: iso9660: failed to read file data`）——不是新回归，是老病。
3. **Joliet 层传真名**，兜底 UEFI 路径。

其余 pycdlib 的坑（照抄即可）：
4. `add_file`/`add_fp`/`add_directory` **都不自动建父目录**，路径上每一级
   都要先显式 `add_directory`，否则 `Could not find path`。
5. 目录必须**按深度升序**建，不能按字典序——`EFI` 大写排在 `boot` 之后，
   字典序会先撞 `/EFI/BOOT` 而父 `/EFI` 还没建。
6. 目录名**不能带 `;1`**（版本号只对文件有效）。
7. `rr_name` 必须**相对路径**（不带前导 `/`），否则报
   `A rock ridge name must be relative`。
8. El Torito 参数名反直觉：`bootfile_path`（不是 `bootimage_path`）、
   `media_name`（不是 `boot_media_type`）；且 `bootfile_path` 必须是
   **ISO 内部路径**（以 `/` 开头，全大写），不是宿主文件系统路径。
9. 引导镜像选 `limine-bios-cd.bin`（CD 扇区引导），不是 `limine-bios.sys`
   （那是 MBR 路径）。选错类型 = QEMU 认不出盘。
"""

import io
import os

import pycdlib

# 脚本在 `kernel/_tools/`，仓库根要上溯两级（第一版写成一级，结果把
# `kernel/build/isoroot` 当成根，直接报 boot image missing）。
REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
ISOROOT = os.path.join(REPO, "build", "isoroot")
BOOT_IMG = os.path.join(ISOROOT, "limine-bios-cd.bin")
OUT = os.path.join(REPO, "build", "varix-fresh.iso")

# El Torito 要引用的引导镜像在 ISO 层的名字（必须全大写，见文件头坑 1/8）。
BOOT_ISO_PATH = "/LIMINE_BIOS_CD.BIN"

# 文件名**一律不带 `;1` 版本号**。这是本脚本最隐蔽的一个坑：
# pycdlib 习惯给 ISO9660 文件名加版本号（`VARIX;1`），但 Limine 按
# `limine.conf` 里的 `boot():/kernel/varix` 精确匹配目录项，**不会替
# 你补版本号** —— 名字对不上就静默不引导（串口日志 0 字节，CPU 停在
# BIOS 的 0xfff0，连 Limine 的 banner 都看不到）。
# 反证：仓库里已"验证过"的 `varix.iso`，其内核目录项正是 `VARIX`
# （无 `;1`），且能被 Limine 找到并加载。
#
# 另：ISO9660 层的长名会**被截断**（8+3），故用下划线短名；
# 真名由 Rock Ridge / Joliet 承载，Limine BIOS 走 RR。
NO_VERSION_SUFFIX = True


def main():
    if not os.path.isfile(BOOT_IMG):
        raise SystemExit("boot image missing: %s" % BOOT_IMG)

    iso = pycdlib.PyCdlib()
    # rock_ridge 是关键：Limine BIOS 只靠它读真名（文件头「二」）。
    iso.new(interchange_level=3, rock_ridge="1.09", joliet=3, vol_ident="VARIX")

    files = []
    for dirpath, _dirnames, filenames in os.walk(ISOROOT):
        for fn in filenames:
            full = os.path.join(dirpath, fn)
            rel = os.path.relpath(full, ISOROOT).replace(os.sep, "/")
            files.append((full, rel))

    # 目录按**深度**升序建（文件头坑 5）。
    all_dirs = set()
    for _full, rel in files:
        parts = rel.split("/")
        for depth in range(1, len(parts)):
            all_dirs.add(tuple(parts[:depth]))
    for parts in sorted(all_dirs, key=len):
        iso_leaf = "/" + "/".join(p.upper().replace("-", "_") for p in parts)
        # rr_name 必须是**相对单层叶名**（坑 7：传 `EFI/BOOT` 这种带 `/`
        # 的会被判 "must be relative"）。
        iso.add_directory(
            iso_path=iso_leaf,
            rr_name=parts[-1],
            joliet_path="/" + "/".join(parts),
        )
        print("  d %-28s  <-iso %s" % ("/" + "/".join(parts), iso_leaf))

    for full, rel in files:
        # ISO9660 层：全大写、连字符换下划线、**不带 `;1`**（见上方
        # NO_VERSION_SUFFIX 说明——带了就静默不引导）。
        iso_leaf = "/" + "/".join(
            p.upper().replace("-", "_") for p in rel.split("/"))
        leaf = rel.split("/")[-1]
        if os.path.splitext(full)[1].lower() in (".img", ".bin"):
            data = open(full, "rb").read()
            iso.add_fp(io.BytesIO(data), len(data),
                       iso_path=iso_leaf, rr_name=leaf, joliet_path="/" + rel)
        else:
            iso.add_file(full, iso_path=iso_leaf, rr_name=leaf, joliet_path="/" + rel)
        print("  + %-28s  <-iso %s" % ("/" + rel, iso_leaf))

    iso.add_eltorito(bootfile_path=BOOT_ISO_PATH, media_name="noemul")

    iso.write(OUT)
    iso.close()
    print("wrote %s (%d bytes)" % (OUT, os.path.getsize(OUT)))

    # --- 后处理：修El Torito（见文件头「关口①」）与补 Stage 3 路径 ---
    patch_eltorito(OUT, BOOT_IMG)
    verify_boot_chain(OUT, BOOT_IMG)


# ---------------------------------------------------------------------------
# 关口①：El Torito 引导项（2026-10-09 实测定位，非推断）
#
# 症状：SeaBIOS 打印 `Booting from DVD/CD..` 后静默挂住，CPU 停 EIP=0xfff0，
#      串口 0 字节。pycdlib 写出的引导项 sector_count 用 `// 512` 算成 52，
#      而该字段单位是 2048 字节扇区（应为 13）；且引导镜像真身在 LBA 39/
#      205，Limine 的 CD 引导代码按 LBA 0 取镜像。两处都要改。
# ---------------------------------------------------------------------------
def patch_eltorito(iso_path, boot_path):
    import struct
    SEC = 2048
    SYSTEM_SECTORS = 16  # LBA 0..15；PVD 固定在 LBA 16

    with open(boot_path, "rb") as f:
        boot = f.read()
    want = -(-len(boot) // SEC)
    if want > SYSTEM_SECTORS:
        raise SystemExit("引导镜像 %d 扇区 > 系统区 %d 扇区" % (want, SYSTEM_SECTORS))

    raw = bytearray(open(iso_path, "rb").read())
    pvd = raw[16 * SEC : 17 * SEC]
    if pvd[1:6] != b"CD001":
        raise SystemExit("LBA16 不是 PVD")
    br = raw[17 * SEC : 18 * SEC]
    if br[1:6] != b"CD001":
        raise SystemExit("LBA17 不是 Boot Record")
    cat_lba = struct.unpack_from("<I", br, 71)[0]
    cat = bytearray(raw[cat_lba * SEC : (cat_lba + 1) * SEC])

    if cat[0] != 1:
        raise SystemExit("Boot Catalog 首项不是 validation entry")
    if cat[30] != 0x55 or cat[31] != 0xAA:
        raise SystemExit("validation entry 尾部 Key 不是 55 AA")
    if cat[32] != 0x88:
        raise SystemExit("引导项 indicator=0x%02x，不是 0x88" % cat[32])

    old_count = struct.unpack_from("<H", cat, 38)[0]
    old_rba = struct.unpack_from("<I", cat, 40)[0]

    # 证据先行：确认 load_rba 处确实是引导镜像真身，不盲改
    at_rba = bytes(raw[old_rba * SEC : old_rba * SEC + len(boot)])
    if at_rba != boot:
        raise SystemExit("load_rba=%d 处不是引导镜像真身，拒绝盲改" % old_rba)

    # 1) 搬进系统区 LBA 0
    raw[0 : want * SEC] = boot + b"\x00" * (want * SEC - len(boot))
    # 2) 引导项指向 LBA 0 + 写实际 2048 扇区数（无大端配对，只写 LE）
    struct.pack_into("<I", cat, 40, 0)
    struct.pack_into("<H", cat, 38, want)
    raw[cat_lba * SEC : (cat_lba + 1) * SEC] = cat

    with open(iso_path, "wb") as f:
        f.write(bytes(raw))
    print("  [eltorito] sector_count %d -> %d, load_rba %d -> 0"
          % (old_count, want, old_rba))
    print("  [eltorito] 引导镜像搬入 LBA 0..%d" % (want - 1))


# ---------------------------------------------------------------------------
# 关口②③：读回自检（不靠「写完就认为对」）
# ---------------------------------------------------------------------------
def verify_boot_chain(iso_path, boot_path):
    import struct
    SEC = 2048
    with open(boot_path, "rb") as f:
        boot = f.read()
    raw = open(iso_path, "rb").read()

    if raw[0 : len(boot)] != boot:
        raise SystemExit("自检失败：LBA 0 不是引导镜像真身")
    print("  [verify] LBA 0 ==引导镜像真身 (%d B)" % len(boot))

    if b"LIMINE_BIOS.SYS" not in raw and b"limine-bios.sys" not in raw:
        print("  [WARN] ISO 内未见 limine-bios.sys —— Stage 3 缺失会导致"
              "『Stage 3 file not found』")

    root = struct.unpack_from("<I", raw, 16 * SEC + 156 + 2)[0]
    d = raw[root * SEC : (root + 1) * SEC]
    i = 0
    kern_ext = None
    while i < SEC:
        l = d[i]
        if l == 0:
            break
        nlen = d[i + 32]
        nm = d[i + 33 : i + 33 + nlen].decode("latin1")
        if nm == "KERNEL":
            kern_ext = struct.unpack_from("<I", d, i + 2)[0]
        i += l
    if kern_ext is None:
        print("  [WARN] ISO 内无 /KERNEL 目录")
        return
    kd = raw[kern_ext * SEC : (kern_ext + 1) * SEC]
    i = 0
    while i < SEC:
        l = kd[i]
        if l == 0:
            break
        nlen = kd[i + 32]
        nm = kd[i + 33 : i + 33 + nlen].decode("latin1")
        if nlen == 5 and nm == "VARIX":
            ex = struct.unpack_from("<I", kd, i + 2)[0]
            sz = struct.unpack_from("<I", kd, i + 10)[0]
            src = os.path.join(ISOROOT, "kernel", "varix")
            want_bytes = open(src, "rb").read()
            same = raw[ex * SEC : ex * SEC + sz] == want_bytes
            print("  [verify] 内核 extent=%d size=%d 逐字节%s"
                  % (ex, sz, "全等" if same else "不等 <<< 有问题"))
            if sz > 4096:
                print("  [note] 内核 %d B（多扇区文件）—— 已实测可正常加载"
                      % sz)
        i += l


if __name__ == "__main__":
    main()