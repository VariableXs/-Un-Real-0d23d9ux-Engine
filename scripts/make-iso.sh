#!/usr/bin/env bash
# AURORA-1000 步骤 0012 · Limine ISO 打包脚本（Limine v12 布局）
# 产出 varix.iso（Limine BIOS/UEFI 双引导 ISO + 防护 MBR）
# 依赖: xorriso（MSYS2: pacman -S xorriso；Windows 便携版放 tools/xorriso/ 或 PATH）
#       limine v12 binary release 解包到 tools/limine/limine-binary/
set -euo pipefail
cd "$(dirname "$0")/.."

KERNEL_ELF="kernel/target/x86_64-unknown-none/release/varix"
ISO_ROOT="build/isoroot"

command -v xorriso >/dev/null 2>&1 || { echo "ERROR: 缺少 xorriso（MSYS2: pacman -S xorriso）" >&2; exit 127; }
[ -f "$KERNEL_ELF" ] || { echo "ERROR: 内核 ELF 不存在，先运行 cargo kbuild（在 kernel/ 目录）" >&2; exit 1; }

LIMINE_DIR="tools/limine/limine-binary"
for f in limine-bios-cd.bin limine-uefi-cd.bin limine-bios.sys BOOTX64.EFI; do
    [ -f "$LIMINE_DIR/$f" ] || {
        echo "ERROR: 缺少 $LIMINE_DIR/$f —— 需解包 limine【二进制】发行包" >&2
        echo "       注意 limine-<ver>.tar.gz 是源码包（无 .bin），要用 limine-binary.tar.gz" >&2
        exit 1
    }
done

rm -rf "$ISO_ROOT"
mkdir -p "$ISO_ROOT/kernel"
cp "$KERNEL_ELF" "$ISO_ROOT/kernel/varix"
cp limine.conf "$ISO_ROOT/limine.conf"
cp "$LIMINE_DIR/limine-bios-cd.bin" \
   "$LIMINE_DIR/limine-uefi-cd.bin" \
   "$LIMINE_DIR/limine-bios.sys" "$ISO_ROOT/"

# F181 · UEFI 引导还需要 ESP 树里的 /EFI/BOOT/BOOTX64.EFI：
# 只有 El Torito 的 limine-uefi-cd.bin 时，ISO 内没有 EFI 系统分区树，
# dd 到 U 盘后在 UEFI-only 固件上会找不到引导体（xorriso 会就此告警）。
mkdir -p "$ISO_ROOT/EFI/BOOT"
cp "$LIMINE_DIR/BOOTX64.EFI" "$ISO_ROOT/EFI/BOOT/BOOTX64.EFI"
[ -f "$LIMINE_DIR/BOOTIA32.EFI" ] && cp "$LIMINE_DIR/BOOTIA32.EFI" "$ISO_ROOT/EFI/BOOT/BOOTIA32.EFI" || true

# F180 · initrd 打包流水线：用户程序/资产自动打进 ISO（一键）
# 源程序尚未构建时脚本会写入占位载荷，保证 ISO 仍可引导到内核。
PY_BIN="${PYTHON:-python3}"
command -v "$PY_BIN" >/dev/null 2>&1 || PY_BIN=python
"$PY_BIN" scripts/make-initfs.py --no-isoroot
cp build/initrd.img "$ISO_ROOT/initrd.img"
ls -l "$ISO_ROOT/initrd.img"

xorriso -as mkisofs \
    -b limine-bios-cd.bin -no-emul-boot -boot-load-size 4 -boot-info-table \
    --efi-boot limine-uefi-cd.bin -efi-boot-part --efi-boot-image --protective-msdos-label \
    -isohybrid-gpt-basdat \
    "$ISO_ROOT" -o varix.iso

# F182 验收：USB 产物 = 这个 ISO 本身（dd 可写），但必须实测「混合 MBR + 盘尾 GPT 备份表」
# 真的在，而不是只靠模型里的一个 true。GPT 解析用 python3（不依赖 fdisk/sgdisk）。
"$PY_BIN" - <<'PY'
import os, struct, sys

p = "varix.iso"
n = os.path.getsize(p)
with open(p, "rb") as fh:
    blob = fh.read()

problems = []

# 1) 混合 MBR：偏移 510 处必须是 0x55AA 引导签名。
if blob[510:512] != b"\x55\xaa":
    problems.append("缺少 MBR 引导签名 0x55AA（不是 dd 可写镜像）")

# 2) GPT 主表：LBA1 处签名 "EFI PART"，且 header CRC32 自洽。
def gpt_crc32(buf):
    poly, crc = 0xEDB88320, 0xFFFFFFFF
    for b in buf:
        crc ^= b
        for _ in range(8):
            crc = (crc >> 1) ^ (poly if crc & 1 else 0)
    return crc ^ 0xFFFFFFFF

def check_gpt_header(hdr, label):
    """校验一个 GPT header：签名 / header_size / header CRC（CRC 字段需先置零）。"""
    if hdr[:8] != b"EFI PART":
        return f"{label}: 签名不是 EFI PART"
    hsize = struct.unpack_from("<I", hdr, 12)[0]   # offset 12 = header_size
    if hsize != 92:
        return f"{label}: header_size={hsize} 异常"
    stored = struct.unpack_from("<I", hdr, 16)[0]  # offset 16 = header CRC32
    # 规范要求：计算 CRC 时先把 CRC 字段本身清零。
    zeroed = hdr[:hsize][:16] + b"\x00\x00\x00\x00" + hdr[:hsize][20:]
    if gpt_crc32(zeroed) != stored:
        return f"{label}: header CRC 不符"
    return None

primary = blob[512:512 + 512]
err = check_gpt_header(primary, "主 GPT")
if err:
    problems.append(err + "（未产出 GPT）")

# 3) GPT 备份表：必须位于最后一个扇区（防截断损坏）。
lba = n // 512
backup = blob[-512:]
if backup[:8] != b"EFI PART":
    problems.append("盘尾缺少 GPT 备份表（备份表必须在最后一个 LBA）")
else:
    # 备份表 header 的 current_lba 应指向最后一个 LBA，且 CRC 自洽。
    err = check_gpt_header(backup, "备份 GPT")
    if err:
        problems.append(err)
    elif struct.unpack_from("<Q", backup, 24)[0] != lba - 1:
        problems.append("备份 GPT 的 current_lba 不等于最后一个 LBA")

if problems:
    print("ISO/USB 产物校验失败:", file=sys.stderr)
    for x in problems:
        print("  -", x, file=sys.stderr)
    sys.exit(1)
print(f"OK: 混合 MBR(0x55AA) + 主/备 GPT 均在位（{n} 字节 / {lba} 扇区）")
PY

# F181 验收：双引导必须真的挂在 ISO 上 —— El Torito 至少两个引导项（BIOS + UEFI），
# 且 ISO 文件系统里要有 ESP 树 /EFI/BOOT/BOOTX64.EFI（U 盘 UEFI 引导靠它）。
ET="$(xorriso -indev varix.iso -report_el_torito plain 2>/dev/null || true)"
BOOT_IMGS="$(printf '%s\n' "$ET" | grep -c 'El Torito boot img' || true)"
[ "${BOOT_IMGS:-0}" -ge 2 ] || { echo "ERROR: El Torito 引导项只有 ${BOOT_IMGS:-0} 个（需 BIOS + UEFI 两个）" >&2; exit 1; }
printf '%s\n' "$ET" | grep -qi 'UEFI' || { echo "ERROR: El Torito 里没有 UEFI 引导项" >&2; exit 1; }
xorriso -indev varix.iso -find /EFI/BOOT -type f 2>/dev/null | grep -q 'BOOTX64.EFI' \
    || { echo "ERROR: ISO 内缺少 /EFI/BOOT/BOOTX64.EFI（UEFI 引导体缺失）" >&2; exit 1; }

echo "OK: varix.iso 已生成"
