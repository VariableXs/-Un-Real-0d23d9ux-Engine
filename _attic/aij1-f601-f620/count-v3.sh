#!/usr/bin/env bash
# AI-J1 v3 行数对账脚本（纯功能行 = 非空 + 非块注释行；与 v1/v2 口径一致）
cd "$(dirname "$0")/../.." || exit 1
count() {
  # 去掉 /* */ 块注释与 // 行注释、空行
  awk '
    BEGIN { inblock=0 }
    /\/\*/ { if ($0 !~ /\*\//) { inblock=1; sub(/\/\*.*/, ""); } }
    inblock==1 && /\*\// { inblock=0; sub(/.*\*\//, ""); }
    inblock==1 { next }
    { line=$0; sub(/\/\/.*/, "", line); gsub(/^[ \t]+|[ \t]+$/, "", line); if (line != "") n++ }
    END { print n+0 }
  ' "$1"
}
total=0
for f in \
  src/features/mouse/actions.ts \
  src/features/mouse/windowRuntime.ts \
  src/features/mouse/J1Runtime.tsx \
  src/features/mouse/profiles.ts \
  src/features/mouse/hoverTiming.ts \
  src/features/settings/MouseJ1Tab.tsx \
  src/features/settings/MouseJ1Panels.tsx \
  src/entries/explorer/main.tsx \
  src/entries/taskbar/main.tsx \
  src/entries/datavault/main.tsx \
  src/system/explorer/ExplorerWindow.tsx \
  src/features/settings/SettingsModal.tsx \
  src/App.tsx \
; do
  n=$(count "$f")
  printf "%6d  %s\n" "$n" "$f"
  total=$((total + n))
done
echo "------"
echo "total(全文件现值, 含 v1/v2 既有行): $total"
