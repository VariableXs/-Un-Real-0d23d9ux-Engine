#!/usr/bin/env bash
# chaos-drill · 混沌演练例行化（AI-S 任务 79 / 专项 J）
# 两段：1) 内核 fs23_journal 掉电注入族（torn/replay）全量重跑；
#       2) SQLite WAL 崩溃持久性演练（合成临时库，不触碰用户数据）。
# 用法: scripts/chaos-drill.sh
set -uo pipefail
cd "$(dirname "$0")/.."
OUT="_attic/verify"
mkdir -p "$OUT"
FAILED=0

step() { printf '\n===== [%s] =====\n' "$1"; }

step "1. 内核 fs23_journal 掉电注入族"
( cd kernel && cargo ktest --lib -- fs:: ) > "$OUT/chaos-fs.log" 2>&1
FS_RC=$?
grep -h 'test result' "$OUT/chaos-fs.log" | tail -2
if [ "$FS_RC" -ne 0 ]; then
  echo "FAIL: fs:: 族测试未通过（rc=$FS_RC，见 $OUT/chaos-fs.log）"; FAILED=1
else
  if ! grep -qE 'torn_entry_halts_replay|append_and_replay' "$OUT/chaos-fs.log"; then
    echo "FAIL: 掉电注入用例未被实际执行（filter 未命中），拒绝假绿"; FAILED=1
  else
    echo "OK   torn/replay 掉电注入族全绿"
  fi
fi

step "2. SQLite WAL 崩溃持久性演练（合成临时库）"
python - "$OUT" <<'PY' || FAILED=1
import os, sqlite3, subprocess, sys, tempfile
out = sys.argv[1]
tmp = tempfile.mkdtemp(prefix="chaos-wal-")
db = os.path.join(tmp, "drill.db")
# 子进程：WAL 模式写 50 笔已提交事务，随后在未提交事务中途硬崩（os._exit）
child = r'''
import os, sqlite3, sys
db = sys.argv[1]
c = sqlite3.connect(db, isolation_level=None)
c.execute("PRAGMA journal_mode=WAL")
c.execute("CREATE TABLE t(x INTEGER)")
for i in range(50):
    c.execute("BEGIN")
    c.execute("INSERT INTO t VALUES (?)", (i,))
    c.execute("COMMIT")
c.execute("BEGIN")
c.execute("INSERT INTO t VALUES (999)")   # 未提交
os._exit(1)                               # 硬崩，模拟掉电
'''
r = subprocess.run([sys.executable, "-c", child, db])
assert r.returncode != 0, "子进程应硬崩"
# 演练断言：50 笔已提交事务全部持久；崩溃中的未提交事务不得出现
c = sqlite3.connect(db)
n = c.execute("SELECT COUNT(*) FROM t").fetchone()[0]
bad = c.execute("SELECT COUNT(*) FROM t WHERE x=999").fetchone()[0]
c.close()
with open(os.path.join(out, "chaos-wal.txt"), "w") as f:
    f.write(f"committed={n} leaked_uncommitted={bad}\n")
if n != 50 or bad != 0:
    print(f"FAIL: WAL 崩溃持久性被破坏 committed={n} (期望 50) leaked={bad}")
    sys.exit(1)
print(f"OK   WAL 硬崩后已提交 50/50 持久，未提交事务零泄漏")
PY

[ "$FAILED" -eq 0 ] && echo "CHAOS: ALL PASS" || { echo "CHAOS: FAILED"; exit 1; }
