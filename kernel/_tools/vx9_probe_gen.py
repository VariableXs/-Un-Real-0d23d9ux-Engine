# -*- coding: utf-8 -*-
"""VX9 探针生成器：只生成探针 include 清单与统计壳（工具脚本，不生成业务代码）。
用法：python - taskboard 回写用 vx9_close.py；本脚本只管探针。
用法：python -I vx9_probe_gen.py <start> <end>
扫描 varix/src/unxstar/ 中 unx<id>_*.rs（id ∈ [start,end]），生成 _tools/vx_sel_vx9/src/main.rs。"""
import io, os, re, sys

K = r"D:\2\14\-D9ux" if False else r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\kernel"
K = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\kernel"
STAR = os.path.join(K, "varix", "src", "unxstar")
OUT = os.path.join(K, "_Write" if False else "varix", "src", "unxstar")
OUT = os.path.join(K, "_tools", "vx_sel_vx9", "src", "main.rs")

start, end = int(sys.argv[1]), int(sys.argv[2])

entries = []
for fn in os.listdir(STAR):
    m = re.match(r"^unx(\d+)_[A-Za-z0-9_]+\.rs$", fn)
    if m:
        i = int(m.group(1))
        if start <= i <= end:
            entries.append((i, fn))
entries.sort()
assert entries, "no modules found in range"

lines = []
lines.append("#![allow(dead_code)]")
lines.append("extern crate alloc;")
lines.append("")
lines.append('#[path = "../../../varix/src/checks.rs"]')
lines.append("pub mod checks;")
lines.append("")
for (i, fn) in entries:
    lines.append('#[path = "../../../varix/src/unxstar/%s"]' % fn)
    lines.append("pub mod m%d;" % i)
    lines.append("")
lines.append("fn main() {")
lines.append("    let mut total = 0usize;")
lines.append("    let mut red = 0usize;")
for (i, fn) in entries:
    lines.append('    {')
    lines.append('        let cs = m%d::run_unx%d_checks();' % (i, i))
    lines.append('        total += 1;')
    red = '        if !cs.all_passed() { red += 1; println!("RED unx%d: {}", %d, cs.red_items_text()); }' % (i, i)
    lines.append(red)
    lines.append('    }')
lines.append('    println!("TOTAL=%d RED=%d PANIC=0", total, red);')
lines.append("}")
lines.append("")

with io.open(OUT, "w", encoding="utf-8", newline="") as g:
    _ = g
    g.write("\n".join(lines) + "\n")
print("probe main.rs written:", len(entries), "modules, range", start, "-", end)
