# -*- coding: utf-8 -*-
"""AI-110 主汇编册登记块追加器（纯追加零删除）。
用法：项目根目录执行
    python _ai110_append.py
前置：python _ai110_gen.py 已生成 _ai110_reg_block.md。
防重：追加前主册 A110-### 段（A110-001–A110-300）零命中断言。
幂等保护：若主册已含 A110-300 行则拒绝重复追加。
"""
import hashlib, sys

MAIN = "docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal.md"
BLK = "_ai110_reg_block.md"

def main():
    data = open(MAIN, "rb").read()
    pre = hashlib.sha256(data).hexdigest()
    text = data.decode("utf-8")
    if "| A110-300 |" in text:
        sys.exit("主册已含 A110-300——拒绝重复追加（幂等保护）")
    # 防重断言：A110-### 独立 ID 段零命中（GOV84-A110 类无连字符序号不算）
    import re
    hits = re.findall(r"A110-\d{3}", text)
    assert not hits, f"A110-### 段已有命中：{hits[:5]}"
    blk = open(BLK, encoding="utf-8").read()
    header = "### 增补登记 · AI-110 内核工程链增补线（B01–B15 · A110-001–A110-300 · 15批×20条×6,000行=90,000行 · 增补卷独立账不占域账）\n\n"
    with open(MAIN, "ab") as f:
        f.write(b"\n\n---\n\n")
        f.write(header.encode("utf-8"))
        f.write(blk.encode("utf-8"))
    post = hashlib.sha256(open(MAIN, "rb").read()).hexdigest()
    assert post != pre
    print(f"appended. pre ={pre}\n         post={post}")

if __name__ == "__main__":
    main()
