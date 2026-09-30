import re, sys, io
sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8")
p = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\docs\Varix\CoRun Varix STAR II · Unxreal\AI-63 · M3 · 300项新功能增补册（B01–B15 · F49601–F49900）.md"
t = open(p, encoding="utf-8").read()
rows = re.findall(r"^\| UNX-(F\d+) \| (.+?) \| (\d+) \| 增补 \| UNX-F\d+-J1 (.+?) \|$", t, re.M)
ids = [r[0] for r in rows]
checks = {}
checks["1_条数300"] = len(rows) == 300
checks["2_ID连续零跳号"] = ids == [f"F{49601+i}" for i in range(300)]
checks["3_判据唯一"] = len(set(ids)) == 300
batches = re.split(r"^## 批 UNX-M3-B(\d+)", t, flags=re.M)[1:]
sums, counts = [], []
for i in range(0, len(batches), 2):
    body = batches[i+1]
    rws = re.findall(r"^\| UNX-F\d+ \| .+? \| (\d+) \| 增补 \|", body, re.M)
    counts.append(len(rws)); sums.append(sum(map(int, rws)))
checks["4_批数15"] = len(counts) == 15
checks["5_每批20条"] = all(c == 20 for c in counts)
checks["6_每批6000行"] = all(s == 6000 for s in sums)
checks["7_状态全增补"] = t.count("| 增补 |") == 300
anchors = ["F49601", "F49625", "F49641", "F49663", "F49687"]
anch_rows = {r[0]: int(r[2]) for r in rows if r[0] in anchors}
checks["8_示例条目行数保真"] = anch_rows == {"F49601":420,"F49625":520,"F49641":460,"F49663":380,"F49687":340}
fails = [k for k,v in checks.items() if not v]
print("批行数:", sums)
print("批条数:", counts)
print("示例锚:", anch_rows)
print("ALL PASS" if not fails else f"FAIL: {fails}")
sys.exit(0 if not fails else 1)
