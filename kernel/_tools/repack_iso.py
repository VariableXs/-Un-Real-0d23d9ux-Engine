"""在既有 varix.iso 上原地替换内核，重新生成可引导 ISO。

为什么不用 `make_iso.py` 从 isoroot 重打包（实测记录，别再走回头路）：
从零打包的 ISO 无论目录项怎么摆，QEMU SeaBIOS 都**不引导**——CPU 停在
BIOS 的 `EIP=0xfff0`，串口日志 0 字节，连 Limine banner 都看不到。
已排除的原因（逐条实测）：El Torito 结构正确（validation `55aa`、
initial entry `boot=0x88`、media=0、sector_count 与镜像大小一致、
boot image LBA 指向的字节与 `limine-bios-cd.bin` 首字节完全相同）、
ISO 层目录项名与旧 ISO 一致（`KERNEL/VARIX`，无 `;1`）。

但反过来**从旧 ISO 出发替换文件是通的**——旧 ISO 能引导到 Limine 并
真的去读 `boot():/kernel/varix`（只是读到的是截断的旧内核，于是
`PANIC: iso9660: failed to read file data`）。所以本脚本走这条路：
拿现成的、结构已被真机验证过的 `varix.iso` 作底，只换掉内核内容。

换文件的坑：pycdlib **不能替换已存在的 extent**，必须
`rm_file` + `add_fp`，且新文件要放在 extent 尾部（会重新排布目录项）。
"""

import os
import sys

import pycdlib

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
BASE_ISO = os.path.join(REPO, "varix.iso")
NEW_KERNEL = os.path.join(REPO, "kernel", "target", "x86_64-unknown-none", "release", "varix")
OUT = os.path.join(REPO, "build", "varix-fresh.iso")

ISO_KERNEL_PATH = "/KERNEL/VARIX"


def main():
    if not os.path.isfile(BASE_ISO):
        raise SystemExit("base iso missing: %s" % BASE_ISO)
    if not os.path.isfile(NEW_KERNEL):
        raise SystemExit("new kernel missing: %s (先跑 cargo kbuild)" % NEW_KERNEL)

    iso = pycdlib.PyCdlib()
    iso.open(BASE_ISO)

    # 先记录旧内核大小，作为「确实换掉了」的可核对基线。
    old = None
    for c in iso.list_children(iso_path="/KERNEL"):
        n = c.file_identifier().decode("utf-8", "replace")
        if n not in (".", ".."):
            old = c.data_length
    print("old kernel bytes: %s" % old)

    # 只删一次：旧 ISO 的目录项本来就叫 `VARIX`（无 `;1`，见 make_iso.py
    # 里 NO_VERSION_SUFFIX 的说明）。多删一次会报 "Could not find path"。
    iso.rm_file("/KERNEL/VARIX")

    data = open(NEW_KERNEL, "rb").read()
    import io
    # **只写 ISO9660 + Rock Ridge 两层，不碰 Joliet**。
    # 旧 ISO 的 Joliet 层目录项是 UTF-16BE 裸字节（`list_children` 返回
    # `'\x00b\x00o\x00o\x00t'` 这种），pycdlib 按 utf-16 解析路径时找不到
    # 父目录 → `Could not find path`。反正 Limine BIOS 走 RR，Joliet 只
    # 服务 UEFI 路径；此处动不了它就绕开它，不为了"三套齐全"把活卡死。
    iso.add_fp(io.BytesIO(data), len(data), iso_path=ISO_KERNEL_PATH, rr_name="VARIX")
    print("new kernel bytes: %d" % len(data))

    iso.write(OUT)
    iso.close()
    print("wrote %s (%d bytes)" % (OUT, os.path.getsize(OUT)))


if __name__ == "__main__":
    main()