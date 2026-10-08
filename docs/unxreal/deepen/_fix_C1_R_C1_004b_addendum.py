# -*- coding: utf-8 -*-
"""R-C1-004b 补遗——UNX-C1-B28 骨架 F8545 性能收官批数修正。

机检发现：F8545 判据行"三批计时账合并——F8494/F8514/F8534 承接"沿旧计数。
真值：E 型性能核账条目共六批（B22 F8434/B23 F8454/B24 F8474/B25 F8494
/B26 F8514/B27 F8534），收官汇总应为六批计时账合并。
勘误纪律同 R-C1-004 主脚本（脚本执行+全链自检+留痕备查）。
"""

import sys
from pathlib import Path

BATCH = Path(__file__).resolve().parent.parent / "batches" / "UNX-C1-B28.md"

OLD = "UNX-F8545-J1 全族性能断言汇总（三批计时账合并——F8494/F8514/F8534 承接），O(1)/O(段数) 断言逐族全绿，无隐藏线性扫描"
NEW = "UNX-F8545-J1 全族性能断言汇总（六批计时账合并——F8434/F8454/F8474/F8494/F8514/F8534 承接——R-C1-004b 补遗勘误后真值），O(1)/O(段数) 断言逐族全绿，无隐藏线性扫描"


def main() -> int:
    if not BATCH.exists():
        print(f"[FAIL] 骨架文件不存在：{BATCH}")
        return 1
    text = BATCH.read_text(encoding="utf-8")
    n = text.count(OLD)
    if n != 1:
        print(f"[FAIL] 旧串命中 {n} 次（应为 1）——中止，零写入。")
        return 1
    text = text.replace(OLD, NEW)
    if text.count(NEW) != 1 or "三批计时账" in text:
        print("[FAIL] 自检失败：新值未在位或旧值残留。")
        return 1
    assert len(["F8434", "F8454", "F8474", "F8494", "F8514", "F8534"]) == 6
    BATCH.write_text(text, encoding="utf-8", newline="\n")
    print("[DONE] R-C1-004b 补遗完成：F8545 三批→六批（F8434/F8454/F8474/F8494/F8514/F8534），旧值零残留。")
    return 0


if __name__ == "__main__":
    sys.exit(main())
