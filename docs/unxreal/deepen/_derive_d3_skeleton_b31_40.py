# -*- coding: utf-8 -*-
"""波08-M28 · D3 域收官轮：骨架索引账 B31–B40 单源派生器（AI-18）。

判例依据（AI-09 B4 收官轮"数据脚本管线"+AI-16 D1 M26"双同步"）：
骨架册自深化册判据单源派生，判据文本零改写，杜绝人工双写漂移。
运行：python docs/unxreal/deepen/_derive_d3_skeleton_b31_40.py
"""
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
DEEPEN = ROOT / 'docs' / 'unxreal' / 'deepen'
BATCH = ROOT / 'docs' / 'unxreal' / 'batches'

# 十批主题（骨架册标题用；与总纲 §7.3-D3 收官轮规划一致）
TITLES = {
    31: 'D3×D1 同步族对象真联签（一）· 冻结接口对接',
    32: 'D3×D2 进程句柄继承真联签（二）· 继承链挂载',
    33: 'D3×J1 安全描述联签（三）· SD/SDT 引用与审计',
    34: 'D3×K1/K5 消费面预演 · Reg* API 面与导出工具',
    35: '句柄账本×O4 长稳对接 · I 段收口',
    36: '双向互读 S4 闭环全量回归矩阵（C 型收官一）',
    37: '句柄账清账与泄漏扫描终验（C 型收官二）',
    38: '万键 hive 读写延迟性能核账（C 型收官三）',
    39: '文档对齐与下游冻结协议移交终验（C 型收官四）',
    40: '域收官闭账 · 满账 240,000 声明（C 型收官五）',
}

META_RE = re.compile(
    r'### (UNX-F(\d+)) · (.+?)\n'
    r'- 域/批：D3/B(\d+)｜判据：(UNX-F\d+-J1 .*?)｜纯功能行数：(\d+) 行（.*?）｜状态：\[已深化\]')


def derive(b: int) -> str:
    src = DEEPEN / ('D3-B%d.md' % b)
    if not src.exists():
        raise SystemExit('深化册缺失: %s' % src)
    txt = src.read_text(encoding='utf-8')
    entries = []
    for m in META_RE.finditer(txt):
        fid, num, title, bb, crit, rows = m.group(1), int(m.group(2)), m.group(3).strip(), int(m.group(4)), m.group(5).strip(), int(m.group(6))
        if bb != b:
            raise SystemExit('%s: 元行批号 B%d 与册号不符' % (fid, bb))
        if not crit.startswith(fid + '-J1'):
            raise SystemExit('%s: 判据号不自指' % fid)
        entries.append((fid, title, crit, rows))
    if len(entries) != 20:
        raise SystemExit('D3-B%d: 提取条数 %d != 20' % (b, len(entries)))
    lo = 13600 + (b - 30) * 20 + 1
    hi = lo + 19
    ids = [int(e[0][6:]) for e in entries]
    if ids != list(range(lo, hi + 1)):
        raise SystemExit('D3-B%d: ID 区间不符（期望 F%d–F%d，实得 %d 条 首%d 末%d）'
                         % (b, lo, hi, len(ids), ids[0], ids[-1]))
    rows_sum = sum(e[3] for e in entries)
    if rows_sum != 6000:
        raise SystemExit('D3-B%d: 行数和 %d != 6000' % (b, rows_sum))
    domain_rows = 180000 + (b - 30) * 6000
    closing = ('域满账收官' if b == 40 else '域累计 %s/240,000' % format(domain_rows, ','))
    head = ('# UNX-D3-B%d · %s（UNX-F%d–UNX-F%d · 20 条）\n\n'
            '> AI-18 承办｜波08-M28 · D3 域收官轮｜派生声明：本骨架账自深化册 '
            'docs/unxreal/deepen/D3-B%d.md 判据单源派生（判据文本零改写，派生器回算），'
            '深化已收口故条目状态标 [已深化]｜批累计行数锁定 6,000（= 本册 20 条之和，脚本回算），%s\n'
            % (b, TITLES[b], lo, hi, b, closing))
    body_lines = []
    for fid, title, crit, rows in entries:
        body_lines.append('### %s · %s' % (fid, title))
        body_lines.append('- 域/批：D3/B%d｜纯功能行数：%d｜状态：[已深化]｜判据：%s' % (b, rows, crit))
    return head + '\n' + '\n\n'.join(body_lines) + '\n'


def main():
    drift_probe = 0
    for b in range(31, 41):
        out = derive(b)
        dst = BATCH / ('UNX-D3-B%d.md' % b)
        dst.write_text(out, encoding='utf-8')
        # 即时回读比对：派生文本中的判据必须与深化册逐字一致（防写入态漂移）
        src = (DEEPEN / ('D3-B%d.md' % b)).read_text(encoding='utf-8')
        src_crits = set(m.group(5).strip() for m in META_RE.finditer(src))
        dst_crits = set(re.findall(r'｜判据：(UNX-F\d+-J1 .+)', out))
        dst_crits = set(c.strip() for c in dst_crits)
        if src_crits != dst_crits:
            drift_probe += 1
            print('D3-B%d: 回读判据集不一致' % b)
        print('D3-B%d -> batches/UNX-D3-B%d.md（20 条 / 6,000 行）OK' % (b, b))
    print('回读漂移探针：%d 处' % drift_probe)
    return 1 if drift_probe else 0


if __name__ == '__main__':
    sys.exit(main())
