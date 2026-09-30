#!/usr/bin/env bash
# verify · 单命令总门禁（AI-S 任务 69）
# 收敛：V 线(tsc+vitest) + C 线(ca-core) + K 线(kcheck+ktest --lib) + 桌面后端(variable --lib)
#       + tools/audit.cjs + 基准回归(>10% 回归即拦)
# 用法:
#   scripts/verify.sh                # 全量门禁（每次交付必跑）
#   scripts/verify.sh --no-bench     # 跳过基准回归段
#   scripts/verify.sh --rebaseline   # 以本次结果重写基准（仅指标改善时允许人工使用）
# 原始结果: _attic/verify/（非功能产物，按仓库约定不入 docs/）
set -uo pipefail
cd "$(dirname "$0")/.."
ROOT="$(pwd)"
OUT="_attic/verify"
BENCH_DIR="_attic/bench"
mkdir -p "$OUT" "$BENCH_DIR"
BENCH=1; REBASE=0
for a in "$@"; do
  case "$a" in
    --no-bench) BENCH=0 ;;
    --rebaseline) REBASE=1 ;;
    *) echo "未知参数: $a"; exit 2 ;;
  esac
done

FAILED=0
declare -a ROWS
row() { ROWS+=("$1"); [ "${2:-0}" -ne 0 ] && FAILED=1; }
step() { printf '\n===== [%s] %s =====\n' "$1" "$2"; }

# ---------- 1. V 线 ----------
step 1 "V 线 tsc --noEmit"
if npx tsc --noEmit > "$OUT/tsc.log" 2>&1; then
  row "V tsc            PASS" 0
else
  row "V tsc            FAIL (见 $OUT/tsc.log)" 1
fi

step 1 "V 线 vitest 全量"
npx vitest run --reporter=json --outputFile="$OUT/vitest.json" > "$OUT/vitest.log" 2>&1
V_RC=$?
node -e "
const r=require('./$OUT/vitest.json');
console.log('suites',r.numTotalTestSuites,'tests',r.numTotalTests,'passed',r.numPassedTests,'failed',r.numFailedTests,'skipped',r.numSkippedTests);" \
  > "$OUT/vitest-summary.txt" 2>&1
cat "$OUT/vitest-summary.txt"
row "V vitest         $( [ $V_RC -eq 0 ] && echo PASS || echo FAIL ) (rc=$V_RC)" "$V_RC"

# ---------- 2. C 线 ----------
step 2 "C 线 ca-core cargo test --locked"
( cd code-analysis && cargo test --locked -p ca-core ) > "$OUT/ca-core.log" 2>&1
C_RC=$?
grep -h 'test result' "$OUT/ca-core.log" | tail -3
row "C ca-core        $( [ $C_RC -eq 0 ] && echo PASS || echo FAIL ) (rc=$C_RC)" "$C_RC"

# ---------- 3. K 线 ----------
step 3 "K 线 cargo kcheck"
( cd kernel && cargo kcheck ) > "$OUT/kcheck.log" 2>&1
KC_RC=$?
if grep -qE '^error' "$OUT/kcheck.log"; then KC_RC=1; fi
row "K kcheck         $( [ $KC_RC -eq 0 ] && echo PASS || echo FAIL ) (rc=$KC_RC)" "$KC_RC"

step 3 "K 线 cargo ktest --lib"
( cd kernel && cargo ktest --lib ) > "$OUT/ktest-lib.log" 2>&1
KT_RC=$?
grep -h 'test result' "$OUT/ktest-lib.log" | tail -3
row "K ktest --lib    $( [ $KT_RC -eq 0 ] && echo PASS || echo FAIL ) (rc=$KT_RC)" "$KT_RC"

# ---------- 4. 桌面后端 ----------
step 4 "后端 cargo test -p variable --lib"
( cd src-tauri && cargo test --locked -p variable --lib ) > "$OUT/variable-lib.log" 2>&1
B_RC=$?
grep -h 'test result' "$OUT/variable-lib.log" | tail -3
row "B variable --lib $( [ $B_RC -eq 0 ] && echo PASS || echo FAIL ) (rc=$B_RC)" "$B_RC"

# ---------- 5. audit ----------
step 5 "tools/audit.cjs"
node tools/audit.cjs > "$OUT/audit.log" 2>&1
A_RC=$?
tail -3 "$OUT/audit.log"
row "audit            $( [ $A_RC -eq 0 ] && echo PASS || echo FAIL ) (rc=$A_RC)" "$A_RC"

# ---------- 6. 基准回归 ----------
if [ "$BENCH" = 1 ]; then
  step 6 "基准回归（3 次中位 vs 基线，>10% 回归拦截）"
  med3() { # med3 <outfile> <cmd...>
    local t=() s ms i
    for i in 1 2 3; do
      s=$(date +%s%N)
      ("${@:2}") >/dev/null 2>&1
      t+=( $(( ($(date +%s%N) - s) / 1000000 )) )
    done
    printf '%s\n' "${t[@]}" | sort -n | sed -n 2p > "$1"
  }
  med3 "$OUT/b.ktest-lib.ms" bash -c 'cd kernel && cargo ktest --lib'
  med3 "$OUT/b.kcheck.ms"   bash -c 'cd kernel && cargo kcheck'
  med3 "$OUT/b.tsc.ms"      bash -c 'npx tsc --noEmit'
  cur=$(node -e "
const fs=require('fs');
const rd=f=>{try{return +fs.readFileSync(f,'utf8').trim()||null}catch{return null}};
console.log(JSON.stringify({ktest_lib_ms:rd('$OUT/b.ktest-lib.ms'),kcheck_ms:rd('$OUT/b.kcheck.ms'),tsc_ms:rd('$OUT/b.tsc.ms')}))")
  echo "current: $cur"
  if [ "$REBASE" = 1 ]; then
    node -e "
const fs=require('fs');
const cur=JSON.parse(process.argv[1]);
const base=JSON.parse(fs.readFileSync('$BENCH_DIR/baseline.json','utf8'));
let worse=false;
for(const k of Object.keys(cur)){ if(cur[k]!=null&&base.budget[k]!=null&&cur[k]>base.budget[k]*1.02) worse=true; }
if(worse){ console.error('拒绝 rebaseline：本次存在劣化指标'); process.exit(1); }
fs.writeFileSync('$BENCH_DIR/baseline.json',JSON.stringify({date:new Date().toISOString().slice(0,10),budget:cur},null,1));
console.log('baseline 已更新');" "$cur"
    row "bench rebaseline PASS" 0
  else
    BENCH_RC=0
  if ! node -e "
const fs=require('fs');
const cur=JSON.parse(process.argv[1]);
let base=null;
try{base=JSON.parse(fs.readFileSync('$BENCH_DIR/baseline.json','utf8'))}catch{}
if(!base||!base.budget){
  fs.writeFileSync('$BENCH_DIR/baseline.json',JSON.stringify({date:new Date().toISOString().slice(0,10),budget:cur},null,1));
  console.log('无基线，本次结果已写入 $BENCH_DIR/baseline.json（首跑建线）');
  process.exit(0);
}
let fail=0;
for(const k of Object.keys(cur)){
  const b=base.budget[k];
  if(cur[k]==null||b==null) continue;
  const pct=((cur[k]-b)/b*100).toFixed(1);
  const bad=(cur[k]-b)/b>0.10;
  console.log(k+': '+b+'ms -> '+cur[k]+'ms ('+(pct>0?'+':'')+pct+'%)'+(bad?'  REGRESSION':''));
  if(bad) fail=1;
}
process.exit(fail);" "$cur"; then BENCH_RC=1; fi
  row "bench 回归       $( [ $BENCH_RC -eq 0 ] && echo PASS || echo FAIL )" "$BENCH_RC"
  fi
fi

# ---------- 汇总 ----------
step "SUMMARY" "verify 汇总"
rc_total=0
for r in "${ROWS[@]}"; do echo "  $r"; case "$r" in *" FAIL "*) rc_total=1;; esac
done
echo "原始结果: $OUT/"
if [ "$FAILED" -ne 0 ]; then
  echo "VERIFY: FAILED"; exit 1
fi
echo "VERIFY: ALL PASS"
exit 0
