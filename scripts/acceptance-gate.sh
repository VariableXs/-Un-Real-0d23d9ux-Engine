#!/usr/bin/env bash
# UNREAL-X-15000 真实验收总门禁
# 用法: scripts/acceptance-gate.sh [--full-kernel]
#   --full-kernel  额外跑内核全量 cargo ktest（已知有挂点，默认只跑 --lib）
# 产出:
#   _attic/acceptance/acceptance-raw/  各线原始结果(JSON/日志)
#     —— 非功能产物（日志/快照 JSON），按仓库约定统一归档到 _attic，不混入 docs/
#   stdout 汇总表：实际通过数 vs 文档宣称数
set -uo pipefail
cd "$(dirname "$0")/.."
ROOT="$(pwd)"
OUT="_attic/acceptance/acceptance-raw"
mkdir -p "$OUT"
FULL_KERNEL=0
[ "${1:-}" = "--full-kernel" ] && FULL_KERNEL=1

log() { printf '\n===== %s =====\n' "$*"; }

# ---------- 静态 + 运行时口径：三线 X-ID 覆盖 ----------
log "0. 三线 X-ID 覆盖（V 线运行时收集 + K/C 线静态扫描）"
npx vitest run src/test/collect-vline.test.ts > "$OUT/collect-vline.log" 2>&1
node -e "
const cp=require('child_process'),fs=require('fs');
const grab=(cmd)=>new Set(cp.execSync(cmd,{shell:'bash',maxBuffer:1e9}).toString().trim().split('\n').map(s=>parseInt(s.slice(1),10)).filter(n=>n>=1&&n<=15000));
const v=JSON.parse(fs.readFileSync('$OUT/vline-ids.json','utf8'));
const V=new Set(v.entries.map(e=>parseInt(e.id.slice(1),10)).filter(n=>n>=1&&n<=15000));
const K=grab('grep -rhoE \"X[0-9]{5}\" kernel/varix/src --include=*.rs');
const C=grab('grep -rhoE \"X[0-9]{5}\" code-analysis --include=*.rs');
const U=new Set([...V,...K,...C]);
let full=0,partial=0,zero=0;
for(let f=0;f<600;f++){let n=0;for(let i=0;i<25;i++) if(U.has(f*25+i+1))n++; if(n===25)full++;else if(n===0)zero++;else partial++;}
const summary={v_entries:v.total,v_unique:V.size,k_unique:K.size,c_unique:C.size,union:U.size,missing:15000-U.size,families:{full,partial,zero},v_failing:v.failing};
fs.writeFileSync('$OUT/coverage-summary.json',JSON.stringify(summary,null,1));
console.log('coverage',JSON.stringify(summary));
" | tee "$OUT/id-coverage.txt"

# ---------- V 线：vitest ----------
log "1. V 线 vitest（src/features 全量）"
npx vitest run --reporter=json --outputFile="$OUT/vitest.json" > "$OUT/vitest.log" 2>&1
VITEST_RC=$?
node -e "
const r=require('./$OUT/vitest.json');
console.log('vitest suites', r.numTotalTestSuites, 'tests', r.numTotalTests,
  'passed', r.numPassedTests, 'failed', r.numFailedTests, 'rc=$VITEST_RC');" | tee "$OUT/vitest-summary.txt"

# ---------- C 线：ca-core ----------
log "2. C 线 code-analysis/core cargo test"
cargo test --manifest-path code-analysis/core/Cargo.toml --locked > "$OUT/ca-core.log" 2>&1
CA_RC=$?
grep -h 'test result' "$OUT/ca-core.log" | tee "$OUT/ca-core-summary.txt"
echo "ca-core rc=$CA_RC" | tee -a "$OUT/ca-core-summary.txt"

# ---------- K 线：kernel ----------
log "3. K 线 kernel cargo ktest --lib（宿主侧）"
(cd kernel && cargo ktest --lib) > "$OUT/ktest-lib.log" 2>&1
K_RC=$?
(cd kernel && true)
grep -h 'test result' "$OUT/ktest-lib.log" | tee "$OUT/ktest-lib-summary.txt"
echo "ktest-lib rc=$K_RC" | tee -a "$OUT/ktest-lib-summary.txt"

if [ "$FULL_KERNEL" = 1 ]; then
  log "3b. K 线全量 cargo ktest（含已知挂点验证）"
  (cd kernel && cargo ktest) > "$OUT/ktest-full.log" 2>&1
  FULL_RC=$?
  grep -h 'test result' "$OUT/ktest-full.log" | tail -20 | tee "$OUT/ktest-full-summary.txt"
  echo "ktest-full rc=$FULL_RC" | tee -a "$OUT/ktest-full-summary.txt"
fi

# ---------- 汇总 ----------
log "4. 汇总"
node --input-type=module -e "
import fs from 'fs';
const out='$OUT';
const v=JSON.parse(fs.readFileSync(out+'/vitest.json','utf8'));
const g=(f)=>{try{return fs.readFileSync(out+'/'+f,'utf8')}catch{return ''}};
const sum=(txt)=>{const m=[...txt.matchAll(/test result: (ok|FAILED)\. (\d+) passed; (\d+) failed/g)];
  return m.reduce((a,x)=>({ok:a.ok+(x[1]==='ok'?1:0),passed:a.passed+ +x[2],failed:a.failed+ +x[3]}),{ok:0,passed:0,failed:0})};
const ca=sum(g('ca-core-summary.txt')||'');
const k=sum(g('ktest-lib-summary.txt')||'');
const kf=sum(g('ktest-full-summary.txt')||'');
console.log(JSON.stringify({vitest:{total:v.numTotalTests,passed:v.numPassedTests,failed:v.numFailedTests},caCore:ca,kernelLib:k,kernelFull:process.argv[1]==='1'?kf:null},null,1));
" "$FULL_KERNEL"

echo "原始结果已保存到 $OUT/"
