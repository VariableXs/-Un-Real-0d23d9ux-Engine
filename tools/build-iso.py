#!/usr/bin/env python3
"""AURORA-1000 · varix.iso 打包（xorriso 缺席的 Windows 替代实现）
用 pycdlib 生成 Limine BIOS/UEFI 双 El Torito 引导 ISO，
等价 scripts/make-iso.sh 的布局：/KERNEL/VARIX + /KERNEL/LIMINE.CFG + /LIMINE.CONF。
用法: python tools/build-iso.py  →  产出仓库根 varix.iso
"""
import os
import sys

import pycdlib

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
KERNEL_ELF = os.path.join(ROOT, "kernel", "target", "x86_64-unknown-none", "release", "varix")
LIMINE_DIR = os.path.join(ROOT, "tools", "limine", "limine-binary")
OUT = os.path.join(ROOT, "varix.iso")

ISO_PATH_KERNEL = "/KERNEL/VARIX"
ISO_PATH_CFG = "/KERNEL/LIMINE.CFG"
ISO_PATH_CONF = "/LIMINE.CONF"
ISO_PATH_BIOS_CD = "/LIMINE_B.CD"
ISO_PATH_UEFI_CD = "/LIMINE_U.CD"
ISO_PATH_BIOS_SYS = "/LIMINEBIOS.SYS"

# Limine v10+ 的 TOML 配置（limine.cfg 老格式对 v12 已不再支持，两份都带上）
LIMINE_CONF = """timeout: 0
serial: yes

/kernel/varix
    protocol: limine
    kernel_path: boot():/kernel/varix
    kernel_cmdline: desktop=1 boot_timeout=0
"""


def main() -> int:
    if not os.path.isfile(KERNEL_ELF):
        print("ERROR: 内核 ELF 不存在，先在 kernel/ 运行 cargo kbuild", file=sys.stderr)
        return 1
    bios_cd = os.path.join(LIMINE_DIR, "limine-bios-cd.bin")
    bios_sys = os.path.join(LIMINE_DIR, "limine-bios.sys")
    uefi_cd = os.path.join(LIMINE_DIR, "limine-uefi-cd.bin")
    for p in (bios_cd, uefi_cd, bios_sys):
        if not os.path.isfile(p):
            print(f"ERROR: 缺少 {p}（先解包 limine-binary.zip）", file=sys.stderr)
            return 1
    old_cfg_path = os.path.join(ROOT, "limine.cfg")

    # 内核 ELF 先做「Limine 净化」再打包：
    #   1) 截断到最后一个 PT_LOAD 段的文件末尾——节表/symtab 对 Limine 加载
    #      无用；QEMU 实证 ISO 内核文件 >7.69MB 会触发 Limine iso9660 驱动
    #      "failed to read file data"（未对齐只是表象，大小才是决定因素）。
    #   2) 补零对齐到 2048B 扇区整数倍。
    import struct as _struct
    with open(KERNEL_ELF, "rb") as f:
        data = f.read()
    e_phoff = _struct.unpack_from("<Q", data, 0x20)[0]
    e_phentsize = _struct.unpack_from("<H", data, 0x36)[0]
    e_phnum = _struct.unpack_from("<H", data, 0x38)[0]
    load_end = 0
    for i in range(e_phnum):
        off = e_phoff + i * e_phentsize
        p_type = _struct.unpack_from("<I", data, off)[0]
        if p_type == 1:  # PT_LOAD
            p_offset = _struct.unpack_from("<Q", data, off + 8)[0]
            p_filesz = _struct.unpack_from("<Q", data, off + 32)[0]
            load_end = max(load_end, p_offset + p_filesz)
    assert load_end > 0
    data = data[:load_end]
    data += b"\x00" * ((-len(data)) % 2048)
    kernel_padded = os.path.join(os.environ.get("TEMP", "/tmp"), "varix_padded.elf")
    with open(kernel_padded, "wb") as f:
        f.write(data)

    iso = pycdlib.PyCdlib()
    iso.new(interchange_level=3, joliet=3, rock_ridge="1.09")
    iso.add_directory("/KERNEL", rr_name="kernel")
    iso.add_file(kernel_padded, ISO_PATH_KERNEL, rr_name="varix")
    conf_tmp = os.path.join(os.environ.get("TEMP", "/tmp"), "limine.conf")
    with open(conf_tmp, "w", newline="\n") as f:
        f.write(LIMINE_CONF)
    iso.add_file(conf_tmp, ISO_PATH_CONF, rr_name="limine.conf")
    iso.add_file(bios_cd, ISO_PATH_BIOS_CD, rr_name="limine-bios-cd.bin")
    iso.add_file(uefi_cd, ISO_PATH_UEFI_CD, rr_name="limine-uefi-cd.bin")
    iso.add_file(bios_sys, ISO_PATH_BIOS_SYS, rr_name="limine-bios.sys")

    # BIOS El Torito（对应 xorriso: -b limine-cd.bin -no-emul-boot -boot-load-size 4 -boot-info-table）
    iso.add_eltorito(
        ISO_PATH_BIOS_CD,
        media_name="noemul",
        boot_info_table=True,
        boot_load_seg=0,
        boot_load_size=4,
    )
    # UEFI El Torito（对应 xorriso: --efi-boot limine-cd-efi.bin -efi-boot-part --efi-boot-image）
    iso.add_eltorito(
        ISO_PATH_UEFI_CD,
        media_name="noemul",
        boot_info_table=True,
        boot_load_seg=0,
        boot_load_size=4,
        efi=True,
    )
    # QEMU/ATAPI 实测：ISO 总扇区为奇数时 Limine 的大文件读取随机失败，
    # 垫一个尾文件把卷补到偶数扇区。
    pad_tmp = os.path.join(os.environ.get("TEMP", "/tmp"), "iso_pad.dat")
    with open(pad_tmp, "wb") as f:
        f.write(b" " * 2048)
    iso.add_file(pad_tmp, "/PAD.DAT;1", rr_name="pad.dat")
    iso.write(OUT)
    iso.close()
    print(f"OK: {OUT} ({os.path.getsize(OUT)} bytes)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
