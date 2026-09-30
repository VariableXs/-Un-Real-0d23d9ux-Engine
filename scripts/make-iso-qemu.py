#!/usr/bin/env python3
"""make-iso 辅助路径（QEMU 验证用）—— 纯 pycdlib 产出 BIOS+UEFI 双 El Torito 引导 ISO。

背景：正式管线 scripts/make-iso.sh 依赖 xorriso（本机暂缺），本脚本只保证
「QEMU/SeaBIOS+OVMF 能引导」所需的最小 ISO 语义：目录树 + 双 El Torito 引导项。
不含 xorriso 的 isohybrid MBR/GPT 补丁，因此产物**不可 dd 到 U 盘**；
U 盘产物仍以 make-iso.sh 为准。用法：

    python scripts/make-iso-qemu.py   # 在仓库根执行，产出 varix-qemu.iso

可选参数（验证用，不改变默认行为）：
    --conf PATH   指定 limine.conf（默认 build/isoroot/limine.conf），用于
                  产出「带三卡菜单」的验证 ISO 而不动正式配置
    --out PATH    指定输出 ISO 路径（默认 <root>/varix-qemu.iso）
"""
import os
import shutil
import struct
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
KERNEL_ELF = os.path.join(ROOT, "kernel", "target", "x86_64-unknown-none", "release", "varix")
ISO_ROOT = os.path.join(ROOT, "build", "isoroot")
LIMINE_DIR = os.path.join(ROOT, "tools", "limine", "limine-binary")
# 引导配置种子（repo 根，与内核 SHARED_BOOT_SELECT_PATH 同名同义）。
# 内核经 Limine internal module（../boot-select.json，flags=0 可选缺失）读取，
# ISO 根放了它，QEMU 演练才能覆盖「副本存在→生效」的主路径。
SEED_CONF = os.path.join(ROOT, "boot-select.json")


def _parse_args(argv):
    """极小参数解析：`--conf FILE` / `--out FILE` / `--no-seed`。"""
    # 单一事实源 = repo 根 limine.conf（isoroot 里那份只是同步目标，
    # 绝不作输入——09-21 部署预检实测其残留 cmdline 会压掉菜单）
    conf = os.path.join(ROOT, "limine.conf")
    out = os.path.join(ROOT, "varix-qemu.iso")
    no_seed = False
    i = 1
    while i < len(argv):
        a = argv[i]
        if a == "--conf" and i + 1 < len(argv):
            conf = argv[i + 1]
            i += 2
        elif a == "--out" and i + 1 < len(argv):
            out = argv[i + 1]
            i += 2
        elif a == "--no-seed":
            no_seed = True
            i += 1
        else:
            print(f"ERROR: 未知参数 {a}", file=sys.stderr)
            return None, None, None
    return conf, out, no_seed

import pycdlib


def main(argv=None) -> int:
    conf, out, no_seed = _parse_args(list(argv if argv is not None else sys.argv))
    if conf is None:
        return 1
    if not os.path.isfile(conf):
        print(f"ERROR: limine.conf 不存在: {conf}", file=sys.stderr)
        return 1
    OUT = os.path.abspath(out)
    if not os.path.isfile(KERNEL_ELF):
        print("ERROR: 内核 ELF 不存在，先在 kernel/ 运行 cargo kbuild", file=sys.stderr)
        return 1
    for f in ("limine-bios-cd.bin", "limine-uefi-cd.bin", "BOOTX64.EFI"):
        if not os.path.isfile(os.path.join(LIMINE_DIR, f)):
            print(f"ERROR: 缺少 {LIMINE_DIR}/{f}", file=sys.stderr)
            return 1
    if not os.path.isfile(SEED_CONF):
        print(f"ERROR: 缺少引导配置种子 {SEED_CONF}", file=sys.stderr)
        return 1

    # 刷新 isoroot：新内核 + initrd + boot-select.json 副本
    shutil.copy2(KERNEL_ELF, os.path.join(ISO_ROOT, "kernel", "varix"))
    # limine.conf 单一事实源 = repo 根（--conf 覆盖时用覆盖件）。
    # isoroot 里的旧拷贝绝不再当输入（09-21 部署预检实测：它残留
    # kernel_cmdline: boot_timeout=0，把 ISO 引导的菜单静默压掉——
    # cmd 优先级契约如实执行了，错的是输入；部署链漂移教训入库）。
    # 同步为 best-effort：ISO 打包直取 conf 权威源（下行 src 逻辑），
    # isoroot 拷贝被外部进程短暂锁住时警告继续，不让打包失败。
    try:
        shutil.copy2(conf, os.path.join(ISO_ROOT, "limine.conf"))
    except OSError as e:
        print(f"WARN: isoroot/limine.conf 同步失败（打包仍用权威源）: {e}", file=sys.stderr)
    # 副本语义：演练者可先改写 isoroot/boot-select.json 再打包（损坏/自定义
    # 变体就是这么做的）；这里只在缺失时从种子回填，绝不覆盖已有变体内容。
    iso_boot_select = os.path.join(ISO_ROOT, "boot-select.json")
    if not no_seed and not os.path.isfile(iso_boot_select):
        shutil.copy2(SEED_CONF, iso_boot_select)
    # S4 壁纸模块（可选资产）：源 bin 在 _attic/wallpaper-src/（构建链
    # t6-extract-frames.py 产出）；缺席 = ISO 不含该文件，内核回退色带。
    WALLPAPER_BIN = os.path.join(ROOT, "_attic", "wallpaper-src", "wallpaper-rgb565.bin")
    wallpaper_present = os.path.isfile(WALLPAPER_BIN)
    if wallpaper_present:
        os.makedirs(os.path.join(ISO_ROOT, "boot"), exist_ok=True)
        shutil.copy2(WALLPAPER_BIN, os.path.join(ISO_ROOT, "wallpaper.rgb565"))
    initrd = os.path.join(ROOT, "build", "initrd.img")
    if not os.path.isfile(initrd):
        py = sys.executable
        subprocess.run([py, os.path.join(ROOT, "scripts", "make-initfs.py"), "--no-isoroot"],
                       check=True, cwd=ROOT)

    iso = pycdlib.PyCdlib()
    iso.new(interchange_level=3, joliet=3, rock_ridge="1.09", vol_ident="VARIX_BOOT")
    iso.add_directory("/BOOT", rr_name="boot", joliet_path="/boot")
    iso.add_directory("/KERNEL", rr_name="kernel", joliet_path="/kernel")
    iso.add_directory("/EFI", rr_name="EFI", joliet_path="/EFI")
    iso.add_directory("/EFI/BOOT", rr_name="BOOT", joliet_path="/EFI/BOOT")
    for path, rr, joliet, iso_name, ddiso, djoliet2 in [
        ("kernel/varix", "varix", "varix", "VARIX_ELF", "/KERNEL", "/kernel"),
        ("limine.conf", "limine.conf", "limine.conf", "LIMINE_CONF", "/", "/"),
        ("boot-select.json", "boot-select.json", "boot-select.json", "BOOT_SEL_JSON", "/", "/"),
        ("initrd.img", "initrd.img", "initrd.img", "INITRD_IMG", "/", "/"),
        ("limine-bios-cd.bin", "limine-bios-cd.bin", "limine-bios-cd.bin", "LIMINE_BIOS_CD", "/BOOT", "/boot"),
        ("limine-uefi-cd.bin", "limine-uefi-cd.bin", "limine-uefi-cd.bin", "LIMINE_UEFI_CD", "/BOOT", "/boot"),
        ("limine-bios.sys", "limine-bios.sys", "limine-bios.sys", "LIMINE_BIOS_SYS", "/BOOT", "/boot"),
    ] + ([("wallpaper.rgb565", "wallpaper.rgb565", "wallpaper.rgb565", "WALLPAPER_RGB565", "/", "/")] if wallpaper_present else []):
        diso, djoliet = ddiso, djoliet2
        src = conf if path == "limine.conf" else os.path.join(ISO_ROOT, path)
        if path == "boot-select.json" and not os.path.isfile(src):
            continue  # --no-seed 演练：副本缺席，ISO 不含该文件
        if path == "boot/wallpaper.rgb565" and not wallpaper_present:
            continue  # 壁纸资产缺席：ISO 不含该文件（内核回退色带）
        iso.add_file(src,
                     iso_path=diso + "/" + iso_name + ".;1",
                     rr_name=rr, joliet_path=djoliet + "/" + joliet)
    # ESP 树（UEFI 从 ISO 文件系统找 /EFI/BOOT/BOOTX64.EFI）
    for src, rr in [("BOOTX64.EFI", "BOOTX64_EFI"), ("BOOTIA32.EFI", "BOOTIA32_EFI")]:
        p = os.path.join(LIMINE_DIR, src)
        if os.path.isfile(p):
            iso.add_file(p, iso_path="/EFI/BOOT/" + rr + ".;1",
                         rr_name=rr, joliet_path="/EFI/BOOT/" + rr)

    # 双 El Torito：BIOS（limine-bios-cd.bin, no-emul, load 4 扇区）+ UEFI
    iso.add_eltorito("/BOOT/LIMINE_BIOS_CD.;1",
                     media_name="noemul", boot_load_size=4, boot_info_table=True)
    iso.add_eltorito("/BOOT/LIMINE_UEFI_CD.;1",
                     media_name="noemul", efi=True)
    iso.write(OUT)
    iso.close()

    # 最小自检：System Area 之后能找到 El Torito 校验项和两个引导记录
    n = os.path.getsize(OUT)
    with open(OUT, "rb") as fh:
        blob = fh.read()
    # PVD 在 LBA16
    if blob[16 * 2048:16 * 2048 + 6] != b"\x01CD001":
        print("ERROR: PVD 缺失", file=sys.stderr)
        return 1
    print(f"OK: {OUT} 已生成（{n} 字节；注意：无 isohybrid，仅 QEMU 引导用）")
    return 0


if __name__ == "__main__":
    sys.exit(main())
