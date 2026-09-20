# -*- coding: utf-8 -*-
"""把 i18n 的英文词条从 dictionaries.ts 拆成独立模块（性能：按需加载）。

为什么拆：dictionaries.ts 284KB 源码里，en 独自占 3122 行（约一半）。中文用户
首屏并不需要英文，却要为它付出下载与解析成本。中文/繁体保留静态（zh-TW 由 zh
经 convertDict 转换而来，增量很小且必须同步可用），英文改为按需加载。

只做机械切分与最小改写，**不改动任何词条内容**：
  1. 抽出 en 定义（3289..6410 行）写成 src/i18n/dict-en.ts
  2. 从 dictionaries.ts 删除这段（含其后空行）
  3. 打印切点前后各 3 行供人工核对
"""
import io
import os

ROOT = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main"
SRC = os.path.join(ROOT, "src", "i18n", "dictionaries.ts")
DST = os.path.join(ROOT, "src", "i18n", "dict-en.ts")

with io.open(SRC, "r", encoding="utf-8") as f:
    lines = f.readlines()

# 定位 en 定义（用内容定位，不硬信行号）
start = None
end = None
for i, ln in enumerate(lines):
    if ln.startswith("const en: Dict = {"):
        start = i
        break
if start is None:
    raise SystemExit("未找到 `const en: Dict = {` 起点")

# 从起点向下找第一个仅由 `};` 组成的行
for j in range(start, len(lines)):
    if lines[j].rstrip("\r\n") == "};":
        end = j
        break
if end is None:
    raise SystemExit("未找到 en 定义的结束 `};`")

print(f"en 定义：第 {start + 1}..{end + 1} 行（共 {end - start + 1} 行）")
print("--- 切点前 ---")
for k in range(max(0, start - 3), start):
    print(f"{k + 1}: {lines[k].rstrip()}")
print("--- 切点后 ---")
for k in range(end + 1, min(len(lines), end + 4)):
    print(f"{k + 1}: {lines[k].rstrip()}")

block = lines[start:end + 1]
# 原定义在 dictionaries.ts 里是模块内私有 `const en`，拆出来后必须加 export，
# 否则 dict-en.ts 不含任何导出 → TS 报 "File is not a module"。
if block and block[0].startswith("const en:"):
    block[0] = "export " + block[0]
else:
    raise SystemExit(f"en 定义首行不是预期的 `const en:`，实为：{block[0]!r}")

# 写出独立模块
header = (
    "// 英文词条（性能：从 dictionaries.ts 拆出，按需加载）。\n"
    "//\n"
    "// 本文件由 _attic/tools/split-i18n-en.py 机械切分生成，**词条内容与拆分前逐字一致**。\n"
    "// 中文用户首屏不需要英文，把它留在主包里纯属浪费下载与解析；改成需要时（切到 en\n"
    "// 或空闲预取）才加载。未加载时 translate() 会按既有回退链落到 zh，不会显示空白或 key。\n"
    "//\n"
    "// 注意：这里重新声明 Dict 而不是从 dictionaries.ts 导入 —— 反向导入会造成循环依赖。\n"
    "\n"
    "type Dict = Record<string, string>;\n"
    "\n"
)
with io.open(DST, "w", encoding="utf-8", newline="\n") as f:
    f.write(header)
    f.writelines(block)

# 从原文件删除 en 定义 + 其后紧跟的空白行
cut = end + 1
while cut < len(lines) and lines[cut].strip() == "":
    cut += 1
rest = lines[:start] + lines[cut:]

with io.open(SRC, "w", encoding="utf-8", newline="\n") as f:
    f.writelines(rest)

print(f"\n已写出 {DST}（{len(block)} 行）")
print(f"dictionaries.ts: {len(lines)} 行 -> {len(rest)} 行")
