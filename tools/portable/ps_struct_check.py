#!/usr/bin/env python3
"""ps1 结构校验器（本地，无 PowerShell 可用时的兜底）。

能力边界（如实声明）：
  * 能查：括号/花括号/方括号配对、字符串与 here-string 未闭合、行尾续行 `` ` ``、
          参数块 param() 是否闭合、if/foreach/while/function/try 的块是否闭合。
  * 不能查：完整 PowerShell 语法（那需要真正的 PowerShell AST，见 CI 的
          `portable/tests/Run-PortableTests.ps1`）。
因此本工具是「结构完整性」门禁，不是「语法正确性」门禁。
"""
import re
import sys
from pathlib import Path

PAIRS = {'(': ')', '[': ']', '{': '}'}
CLOSERS = set(PAIRS.values())


def scan(text):
    """返回 (errors, stats)。逐字符状态机，正确处理注释/字符串/here-string/转义。"""
    errors = []
    stack = []            # (char, line, col)
    i, n = 0, len(text)
    line, col = 1, 1
    here = None           # 当前 here-string 定界符: '@ 或 "@
    stats = {'lines': text.count('\n') + 1, 'chars': n}

    def err(msg, ln=None, cl=None):
        errors.append(f"L{ln or line}:C{cl or col} {msg}")

    while i < n:
        ch = text[i]
        nxt = text[i + 1] if i + 1 < n else ''

        # --- here-string 内部：只找定界符（必须在行首） ---
        if here is not None:
            if col == 1 and text.startswith(here, i):
                here = None
                i += 2
                col += 2
                continue
            if ch == '\n':
                line, col = line + 1, 1
            else:
                col += 1
            i += 1
            continue

        # --- 注释 ---
        if ch == '#':
            j = text.find('\n', i)
            if j < 0:
                break
            i, line, col = j, line + 1, 1
            continue
        if ch == '<' and nxt == '#':
            j = text.find('#>', i)
            if j < 0:
                err('块注释 <# 未闭合')
                break
            line += text.count('\n', i, j)
            i, col = j + 2, 1
            continue

        # --- here-string 起点 ---
        if ch == '@' and (nxt == '"' or nxt == "'"):
            rest = text[i + 2:text.find('\n', i) if text.find('\n', i) >= 0 else n]
            if rest.strip() != '':
                err(f"here-string {ch}{nxt} 定界符后不能有其他内容")
            else:
                here = nxt + '@'
            i += 2
            col += 2
            continue

        # --- 单/双引号字符串 ---
        if ch in ('"', "'"):
            quote = ch
            j = i + 1
            closed = False
            while j < n:
                c = text[j]
                if c == '`' and quote == '"':
                    j += 2
                    continue
                if c == quote:
                    if quote == "'" and j + 1 < n and text[j + 1] == "'":
                        j += 2
                        continue
                    if quote == '"' and j + 1 < n and text[j + 1] == '"':
                        j += 2
                        continue
                    closed = True
                    break
                if c == '\n':
                    break
                j += 1
            if not closed:
                err(f'{quote} 字符串未闭合', line, col)
                i = text.find('\n', i)
                if i < 0:
                    break
                line, col = line + 1, 1
                i += 1
                continue
            line += text.count('\n', i, j)
            i, col = j + 1, col + (j + 1 - i)
            continue

        # --- 反引号续行 ---
        if ch == '`':
            if nxt == '\r':
                if text[i + 2:i + 3] == '\n':
                    err('续行符 ` 后跟 CRLF：PowerShell 只在 ` 直接接 LF 时续行，CRLF 会失效')
                i += 1
                col += 1
                continue
            if nxt != '\n':
                col += 2
                i += 2
                continue
            i += 1
            line, col = line + 1, 1
            continue

        if ch == '\n':
            line, col = line + 1, 1
            i += 1
            continue

        if ch in PAIRS:
            stack.append((ch, line, col))
        elif ch in CLOSERS:
            if not stack:
                err(f'多余的闭合符 {ch}')
            else:
                op, ol, oc = stack.pop()
                if PAIRS[op] != ch:
                    err(f'{ch} 与 L{ol}:C{oc} 的 {op} 不匹配')
        col += 1
        i += 1

    if here is not None:
        errors.append(f'here-string {here} 未闭合')
    for op, ol, oc in stack:
        errors.append(f'L{ol}:C{oc} 的 {op} 未闭合')
    return errors, stats


def main(argv):
    if not argv:
        print(__doc__)
        return 2
    files = []
    for a in argv:
        p = Path(a)
        if p.is_dir():
            files += sorted(x for x in p.rglob('*.ps1') if '.git' not in x.parts)
        elif p.is_file():
            files.append(p)
        else:
            print(f'跳过（不存在）: {a}')
    bad = 0
    for f in files:
        text = f.read_text(encoding='utf-8-sig', errors='replace')
        errs, st = scan(text)
        if errs:
            bad += 1
            print(f'FAIL {f} ({len(errs)})')
            for e in errs[:10]:
                print(f'  {e}')
        else:
            print(f'ok   {f}  ({st["lines"]}行/{st["chars"]}字)')
    print(f'\n== {len(files) - bad}/{len(files)} 个 .ps1 结构完整 ==')
    return 1 if bad else 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
