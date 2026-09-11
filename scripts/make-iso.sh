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

rm -rf "$ISO_ROOT"
mkdir -p "$ISO_ROOT/kernel"
cp "$KERNEL_ELF" "$ISO_ROOT/kernel/varix"
cp limine.conf "$ISO_ROOT/limine.conf"
cp tools/limine/limine-binary/limine-bios-cd.bin \
   tools/limine/limine-binary/limine-uefi-cd.bin \
   tools/limine/limine-binary/limine-bios.sys "$ISO_ROOT/"

xorriso -as mkisofs \
    -b limine-bios-cd.bin -no-emul-boot -boot-load-size 4 -boot-info-table \
    --efi-boot limine-uefi-cd.bin -efi-boot-part --efi-boot-image --protective-msdos-label \
    "$ISO_ROOT" -o varix.iso

echo "OK: varix.iso 已生成"
