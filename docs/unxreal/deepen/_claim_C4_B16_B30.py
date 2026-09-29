# -*- coding: utf-8 -*-
"""AI-14 认领写锁（§6.2 规则二）：总纲 §7.3-C4 B16–B30 翻 [骨架]/AI-14 + 预算批注 + 域账守恒校验"""
import io

F = "docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal · 总纲与施工书.md"
text = io.open(F, encoding="utf-8").read()

ROWS = {
    "B16": ("F10701–F10720", 6500), "B17": ("F10721–F10740", 6400),
    "B18": ("F10741–F10760", 6300), "B19": ("F10761–F10780", 6400),
    "B20": ("F10781–F10800", 6400), "B21": ("F10801–F10820", 6900),
    "B22": ("F10821–F10840", 6800), "B23": ("F10841–F10860", 6900),
    "B24": ("F10861–F10880", 6900), "B25": ("F10881–F10900", 7000),
    "B26": ("F10901–F10920", 6800), "B27": ("F10921–F10940", 6900),
    "B28": ("F10941–F10960", 6800), "B29": ("F10961–F10980", 7200),
    "B30": ("F10981–F11000", 7300),
}
n = 0
for b, (frange, rows) in ROWS.items():
    old = "| UNX-C4-%s | %s | 20 | [未动] | 0 | 待领 |" % (b, frange)
    new = "| UNX-C4-%s | %s | 20 | [骨架] | —（骨架 %s 行） | AI-14 |" % (b, frange, format(rows, ","))
    assert text.count(old) == 1, "锚点不唯一 B%s: %d" % (b, text.count(old))
    text = text.replace(old, new, 1)
    n += 1
print("B16–B30 十五行翻转 [骨架]/AI-14: %d/15" % n)

# 引言尾批注：本会话续领 + 收官预算 + 域账守恒
anchor = "B21–B28（E 型正反双判据）→ B29–B36（I 型集成：vim 全屏联测/多进程 IPC 压测/挂号协议联签）→ B37–B40（C 型收官：LTP ipc/pty 组回归与压测核账）。"
assert text.count(anchor) == 1
addendum = ("【波08-M10 续领批注（AI-14）】本会话续领 B16–B30（F10701–F11000，300 条）：B16–B20 M 型机制收尾（socket 连接建立深水/地址与关闭/选项族/epoll 深水/M 型总收口，32,000 行）"
            "+ B21–B28 E 型正反双判据错误矩阵（管道 FIFO/信号/pty termios/unix socket 两批/复用器/SysV/POSIX，55,000 行）+ B29–B30 I 型集成前段（vim 全屏联测/多进程 IPC 压测场，14,500 行），"
            "骨架预算合计 101,500 行；B31–B40 收官预算一次批注（B31–B36 I 型后段各 5,330 合计 31,980 + B37–B40 C 型收官各 5,210 合计 20,840 = 52,820 行）。"
            "域账守恒：B01–B15 深化 85,680 + B16–B30 骨架 101,500 + B31–B40 收官 52,820 = 240,000 ✓（域累计至 B30 = 187,180/240,000）。")
text = text.replace(anchor, anchor + addendum, 1)
print("引言续领批注 + 域账守恒声明: applied")

io.open(F, "w", encoding="utf-8", newline="").write(text)
print("总纲写锁落盘完成")
