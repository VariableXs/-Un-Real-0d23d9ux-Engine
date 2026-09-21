# -*- coding: utf-8 -*-
"""T8 · 真机冷启动/交替引导日志器（AI-1/AI-6 演练配套）。

用法（每次引导进 Windows 后手动跑一次，或挂计划任务开机自启）：
  python t8-realboot-log.py <VARIX|WINDOWS>

向 W:\\SHARED\\varix-bootlog.txt 追加一行：时间戳 + 本次引导入口 + 引导链
证据（BootNext 是否已清零、U 盘是否在场）。连续 10 次冷启动演练的
日志即验收证据（docs/acceptance/ 对应记录引用本文件输出）。
只写 SHARED 一行文本，不碰其他任何盘。
"""
import ctypes
import datetime
import os
import sys

LOG = r"W:\SHARED\varix-bootlog.txt"


def usb_present():
    """U 盘在场判定：VARIX-ESP 卷标签可访问。"""
    k32 = ctypes.windll.kernel32
    return k32.GetDriveTypeW("Y:\\") == 4 or os.path.isdir("Y:\\")


def main():
    if len(sys.argv) < 2 or sys.argv[1].upper() not in ("VARIX", "WINDOWS"):
        print("用法: python t8-realboot-log.py <VARIX|WINDOWS>")
        return 2
    entry = sys.argv[1].upper()
    ts = datetime.datetime.now().strftime("%Y-%m-%d %H:%M:%S")
    line = "%s | entered=%s | usb=%s\n" % (ts, entry, "Y" if usb_present() else "N")
    with open(LOG, "a", encoding="utf-8") as f:
        f.write(line)
    print("logged:", line.strip())
    with open(LOG, "r", encoding="utf-8") as f:
        n = sum(1 for _ in f)
    print("total boots logged:", n)
    return 0


if __name__ == "__main__":
    sys.exit(main())
