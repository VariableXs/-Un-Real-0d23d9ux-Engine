#!/usr/bin/env python3
"""阶段2：robust.rs 352 表直挂 13 条顶层/单文件域死判据 + 容量同步。"""
import re

P = r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/kernel/varix/src/robust.rs"
s = open(P, encoding="utf-8", newline="").read()

ENTRIES = [
    "crate::display::run_ai11_display_checks",
    "crate::sec::run_ai36_sec_checks",
    "crate::sec::run_ai38_sec_checks",
    "crate::sec::run_ai39_sec_checks",
    "crate::fs::run_fs23_checks",
    "crate::task::run_task_all_checks",
    "crate::m700vm::run_m700vm_checks",
    "crate::compatstar::comdlg::run_comdlg_ext_checks",
    "crate::galaxy::finalgate::run_finalgate_checks",
    "crate::galaxy::verifyall::run_verifyall_checks",
    "crate::gfx::meshstream::run_vei11_checks",
    "crate::h3star::copypath::run_copypath_checks",
    "crate::h3star::copypath::run_copypath_deep_checks",
]

m = re.search(r"let domains: \[fn\(\) -> CheckSet; (\d+)\] = \[", s)
assert m, "找不到 352 表起点"
old_cap = int(m.group(1))
assert old_cap == 352, f"表容量 {old_cap} != 352，可能已被并行会话改过，中止"
close = re.search(r"\n\s*\];", s[m.end():])
assert close, "找不到表收口"
pos = m.end() + close.start()
block = "\n".join(f"    {e}," for e in ENTRIES)
s = s[:pos] + "\n" + block + s[pos:]
s = s.replace(f"; {old_cap}] = [", f"; {old_cap + len(ENTRIES)}] = [")
open(P, "w", encoding="utf-8", newline="").write(s)

# 复核
s2 = open(P, encoding="utf-8", newline="").read()
m2 = re.search(r"let domains: \[fn\(\) -> CheckSet; (\d+)\] = \[(.*?)\n\s*\];", s2, re.S)
n = len(re.findall(r"^\s*crate::\w", m2.group(2), re.M))
print(f"[robust] 容量 {m2.group(1)}，表项 {n}，新增 {len(ENTRIES)}")
assert n == int(m2.group(1)), f"复核失败 {n} != {m2.group(1)}"
for e in ENTRIES:
    assert s2.count(e) >= 1, f"缺 {e}"
print("[ok] robust.rs 13 条直挂完成并复核")
