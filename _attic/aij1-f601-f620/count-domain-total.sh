#!/usr/bin/env bash
# J1 域全部功能文件现值统计（非空非注释行；测试不计入口径）
cd "$(dirname "$0")/../.." || exit 1
filt() { awk '
  BEGIN { inblock=0 }
  /\/\*/ { if ($0 !~ /\*\//) { inblock=1; sub(/\/\*.*/, ""); } }
  inblock==1 && /\*\// { inblock=0; sub(/.*\*\//, ""); }
  inblock==1 { next }
  { line=$0; sub(/\/\/.*/, "", line); gsub(/^[ \t]+|[ \t]+$/, "", line); if (line != "") n++ }
  END { print n+0 }
'; }
total=0
for f in src/features/mouse/j1store.ts src/features/mouse/curve.ts src/features/mouse/filters.ts \
  src/features/mouse/wheel.ts src/features/mouse/screen.ts src/features/mouse/magnet.ts \
  src/features/mouse/autoscroll.ts src/features/mouse/hoverTiming.ts src/features/mouse/profiles.ts \
  src/features/mouse/sideButtons.ts src/features/mouse/gestures.ts src/features/mouse/overlay.ts \
  src/features/mouse/inertia.ts src/features/mouse/gestureRecorder.ts src/features/mouse/pack.ts \
  src/features/mouse/telemetry.ts src/features/mouse/evidence.ts src/features/mouse/actions.ts \
  src/features/mouse/shortcutRecorder.ts src/features/mouse/checklist.ts \
  src/features/mouse/windowRuntime.ts src/features/mouse/J1Runtime.tsx \
  src/features/settings/MouseJ1Tab.tsx src/features/settings/MouseJ1Panels.tsx src/styles/mouse-j1.css; do
  n=$(filt < "$f"); printf "%6d  %s\n" "$n" "$f"; total=$((total+n))
done
echo "------"; echo "J1 域功能文件现值合计: $total"
