import re, sys, io
sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8")
p = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\docs\Varix\CoRun Varix STAR II · Unxreal\AI-63 · M3 · 500项新功能增补册（B16–B40 · F49901–F50400）.md"
t = open(p, encoding="utf-8").read()
rows = re.findall(r"^\| UNX-(F\d+) \| (.+?) \| (\d+) \| 增补 \| UNX-F\d+-J1 (.+?) \|$", t, re.M)
ids = [r[0] for r in rows]
checks = {}
checks["1_条数500"] = len(rows) == 500
checks["2_ID连续零跳号"] = ids == [f"F{49901+i}" for i in range(500)]
checks["3_判据唯一"] = len(set(ids)) == 500
batches = re.split(r"^## 批 UNX-M3-B(\d+)", t, flags=re.M)[1:]
sums, counts = [], []
for i in range(0, len(batches), 2):
    body = batches[i+1]
    rws = re.findall(r"^\| UNX-F\d+ \| .+? \| (\d+) \| 增补 \|", body, re.M)
    counts.append(len(rws)); sums.append(sum(map(int, rws)))
checks["4_批数25"] = len(counts) == 25
checks["5_每批20条"] = all(c == 20 for c in counts)
checks["6_每批6000行"] = all(s == 6000 for s in sums)
checks["7_状态全增补"] = t.count("| 增补 |") == 500
if not all(c == 20 for c in counts):
    checks["5b_批条数明细"] = counts
if not all(s == 6000 for s in sums):
    checks["6b_批行数明细"] = sums
fails = [k for k, v in checks.items() if v is False]
print("批行数:", sums)
print("批条数:", counts)
print("ALL PASS" if not fails else f"FAIL: {fails}")
sys.exit(0 if not fails else 1)
