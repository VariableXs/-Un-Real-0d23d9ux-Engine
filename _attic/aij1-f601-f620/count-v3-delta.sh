#!/usr/bin/env bash
# v3 增量对账：新文件取全量功能行；改动文件取 git diff 新增行经同一过滤器
cd "$(dirname "$0")/../.." || exit 1
filt() { awk '
  BEGIN { inblock=0 }
  /\/\*/ { if ($0 !~ /\*\//) { inblock=1; sub(/\/\*.*/, ""); } }
  inblock==1 && /\*\// { inblock=0; sub(/.*\*\//, ""); }
  inblock==1 { next }
  { line=$0; sub(/\/\/.*/, "", line); gsub(/^[ \t]+|[ \t]+$/, "", line); if (line != "") n++ }
  END { print n+0 }
'; }
echo "== 新文件（全量功能行） =="
new_total=0
for f in src/features/mouse/actions.ts src/features/mouse/windowRuntime.ts src/features/mouse/__tests__/j1v3.spec.ts; do
  n=$(filt < "$f"); printf "%6d  %s\n" "$n" "$f"
  case "$f" in *__tests__*) ;; *) new_total=$((new_total+n));; esac
done
echo "== 改动文件（diff 新增功能行） =="
mod_total=0
for f in src/features/mouse/J1Runtime.tsx src/features/mouse/profiles.ts src/features/mouse/hoverTiming.ts src/features/settings/MouseJ1Tab.tsx src/features/settings/MouseJ1Panels.tsx src/entries/explorer/main.tsx src/entries/taskbar/main.tsx src/entries/datavault/main.tsx src/system/explorer/ExplorerWindow.tsx src/features/settings/SettingsModal.tsx src/App.tsx; do
  n=$(git diff -U0 -- "$f" | grep '^+' | grep -v '^+++' | filt); printf "%6d  %s\n" "$n" "$f"; mod_total=$((mod_total+n))
done
echo "------"
echo "新文件功能行合计: $new_total"
echo "改动文件新增功能行合计: $mod_total"
echo "v3 增量功能行总计: $((new_total+mod_total))"
