#!/usr/bin/env python3
"""真正的 PowerShell 词法器 —— 不是正则计数器。

动机：ps_struct_check.py 是括号计数器，它对 CI 实际拒绝的文件给出了 26/26 通过。
这个实现按 PowerShell 的词法规则逐字符走：注释、单/双引号串、here-string、
反引号转义与续行、$( ) 子表达式，并在真实的 token 流上配平括号。

用法: python3 ps_lex_check.py <文件或目录>
"""
import sys
from pathlib import Path

NORMAL, SQ, DQ, BCMT, HDQ, HSQ = 'NORMAL', 'SQ', 'DQ', 'BCMT', 'HDQ', 'HSQ'
PAIRS = {')': '(', ']': '[', '}': '{'}
# PowerShell 把 Unicode 智能引号当作等价的字符串定界符。
# 这一点是本次 CI 故障的根因：BOM 缺失 + 按 ANSI 误读时，汉字末字节会变成 U+201D，
# 从而提前闭合字符串。词法器必须建模这个行为，否则抓不到该类缺陷。
DQUOTES = ('"', '\u201c', '\u201d')   # "  “  ”
SQUOTES = ("'", '\u2018', '\u2019')   # '  ‘  ’


def lex(text):
    """返回 (errors, line_no_at_eof)。errors 为 (line, col, msg) 列表。"""
    errs = []
    stack = []          # [(char, line, col, origin)]  origin: 'plain' | 'sub'
    mode = NORMAL
    modes = []          # 进入 $( 前的模式栈
    i, n = 0, len(text)
    line, col = 1, 1

    def adv(k=1):
        nonlocal i, line, col
        for _ in range(k):
            if i < n:
                if text[i] == '\n':
                    line += 1
                    col = 1
                else:
                    col += 1
                i += 1

    while i < n:
        c = text[i]
        nxt = text[i + 1] if i + 1 < n else ''

        # ---------- 块注释 ----------
        if mode == BCMT:
            if c == '#' and nxt == '>':
                adv(2)
                mode = modes.pop() if modes else NORMAL
            else:
                adv()
            continue

        # ---------- here-string ----------
        if mode in (HDQ, HSQ):
            term = '"@' if mode == HDQ else "'@"
            # 结束标记必须在行首（允许前置空白）
            if c == term[0] and nxt == '@':
                line_start = text.rfind('\n', 0, i) + 1
                if text[line_start:i].strip() == '':
                    adv(2)
                    mode = NORMAL
                    continue
            adv()
            continue

        # ---------- 单引号串：只有 '' 转义 ----------
        if mode == SQ:
            if c in SQUOTES:
                if nxt == "'":
                    adv(2)
                else:
                    adv()
                    mode = modes.pop() if modes else NORMAL
            else:
                adv()
            continue

        # ---------- 双引号串：反引号转义，$( 进入子表达式 ----------
        if mode == DQ:
            if c == '`':
                adv(2)
            elif c == '$' and nxt == '(':
                stack.append(('(', line, col, 'sub'))
                modes.append(DQ)
                mode = NORMAL
                adv(2)
            elif c in DQUOTES:
                adv()
                mode = modes.pop() if modes else NORMAL
            else:
                adv()
            continue

        # ---------- NORMAL ----------
        if c == '#':
            while i < n and text[i] != '\n':
                adv()
            continue
        if c == '<' and nxt == '#':
            modes.append(NORMAL)
            mode = BCMT
            adv(2)
            continue
        if c == '@' and nxt in ("'", '"') and i + 2 < n and text[i + 2] == '\n':
            mode = HDQ if nxt == '"' else HSQ
            adv(2)
            continue
        if c in SQUOTES:
            modes.append(NORMAL)
            mode = SQ
            adv()
            continue
        if c in DQUOTES:
            modes.append(NORMAL)
            mode = DQ
            adv()
            continue
        if c == '`':
            # 反引号续行：后面必须紧跟换行（CRLF 亦可），否则只是转义下一字符
            if nxt == '\r' or nxt == '\n':
                adv()
                continue
            if nxt == '':
                errs.append((line, col, '文件末尾是孤立的反引号续行符'))
                adv()
                continue
            adv(2)
            continue
        if c in '([{':
            stack.append((c, line, col, 'plain'))
            adv()
            continue
        if c in ')]}':
            if not stack:
                errs.append((line, col, f'多余的 {c}'))
            else:
                top, tl, tc, origin = stack.pop()
                if top != PAIRS[c]:
                    errs.append((line, col, f'{c} 与第 {tl} 行第 {tc} 列的 {top} 不匹配'))
                if origin == 'sub':
                    mode = modes.pop() if modes else DQ
            adv()
            continue
        adv()

    if mode == SQ:
        errs.append((line, col, "单引号串未闭合"))
    if mode == DQ:
        errs.append((line, col, '双引号串未闭合'))
    if mode == BCMT:
        errs.append((line, col, '块注释 <# 未闭合'))
    if mode in (HDQ, HSQ):
        errs.append((line, col, 'here-string 未闭合'))
    for ch, l, c0, origin in stack:
        tag = '$(' if origin == 'sub' else ch
        errs.append((l, c0, f'{tag} 未闭合 —— 缺少配对的 {")" if ch == "(" else ("]" if ch == "[" else "}")}'))
    return errs, line


def main():
    root = Path(sys.argv[1] if len(sys.argv) > 1 else 'portable')
    files = sorted([root] if root.is_file() else root.rglob('*.ps1'))
    bad = 0
    for f in files:
        raw = f.read_bytes()
        if raw.startswith(b'\xef\xbb\xbf'):
            raw = raw[3:]
        text = raw.decode('utf-8', errors='replace')
        errs, _ = lex(text)
        if errs:
            bad += 1
            print(f'[FAIL] {f}')
            for l, c, m in errs[:6]:
                print(f'   line {l} col {c}: {m}')
    print(f'== {len(files) - bad}/{len(files)} 个 .ps1 词法通过 ==')
    return 1 if bad else 0


if __name__ == '__main__':
    sys.exit(main())
