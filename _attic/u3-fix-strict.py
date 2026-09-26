# -*- coding: utf-8 -*-
"""AI-U3 严格模式修复脚本（noUncheckedIndexedAccess 债清零）。"""

def patch(path, pairs):
    s = open(path, encoding='utf-8').read()
    for old, new in pairs:
        if old not in s:
            print("MISS %s: %s" % (path, old[:70]))
        s = s.replace(old, new, 1)
    open(path, 'w', encoding='utf-8', newline='\n').write(s)

# deskicons.ts
patch('src/features/u3/deskicons.ts', [
    ('    if (width(lines[line]) + width(word) <= budgetPerLine) {\n      lines[line] += word;',
     '    if (width(lines[line] ?? "") + width(word) <= budgetPerLine) {\n      lines[line] = (lines[line] ?? "") + word;'),
    ('      const space = budgetPerLine - width(lines[line]);',
     '      const space = budgetPerLine - width(lines[line] ?? "");'),
    ('      lines[line] += chars.slice(0, take).join("");',
     '      lines[line] = (lines[line] ?? "") + chars.slice(0, take).join("");'),
    ('  if (truncated) {\n    let tail = [...lines[1]];\n    while (width(tail.join("")) + width(ELLIPSIS) > budgetPerLine && tail.length > 0) tail.pop();\n    lines[1] = tail.join("") + ELLIPSIS;\n  } else if (width(lines[1]) > budgetPerLine) {',
     '  if (truncated) {\n    let tail = [...(lines[1] ?? "")];\n    while (width(tail.join("")) + width(ELLIPSIS) > budgetPerLine && tail.length > 0) tail.pop();\n    lines[1] = tail.join("") + ELLIPSIS;\n  } else if (width(lines[1] ?? "") > budgetPerLine) {'),
    ('    let tail = [...lines[1]];\n    while (width(tail.join("")) + width(ELLIPSIS) > budgetPerLine && tail.length > 0) tail.pop();\n    lines[1] = tail.join("") + ELLIPSIS;\n    truncated = true;',
     '    let tail = [...(lines[1] ?? "")];\n    while (width(tail.join("")) + width(ELLIPSIS) > budgetPerLine && tail.length > 0) tail.pop();\n    lines[1] = tail.join("") + ELLIPSIS;\n    truncated = true;'),
    ('  return { lines: [lines[0], lines[1]], truncated };',
     '  return { lines: [lines[0] ?? "", lines[1] ?? ""], truncated };'),
    ('  from: { colPx: number; rowPx: number }, // 语义参数：调用方记录换档前格距（算法只依赖 to）\n  to: { colPx: number; rowPx: number },\n): Array<{ x: number; y: number }> {',
     '  from: { colPx: number; rowPx: number }, // 语义参数：调用方记录换档前格距（算法只依赖 to）\n  to: { colPx: number; rowPx: number },\n): Array<{ x: number; y: number }> {\n  void from;'),
])

# explorerx.ts：h 已守卫（hits.length===0 早退）
patch('src/features/u3/explorerx.ts', [
    ('    firstContext: `${text.slice(Math.max(0, h.start - ctx), h.start)}【${text.slice(h.start, h.end)}】${text.slice(h.end, h.end + ctx)}`,',
     '    firstContext: `${text.slice(Math.max(0, h.start - ctx), h.start)}【${text.slice(h.start, h.end)}】${text.slice(h.end, h.end + ctx)}`, // h 由上方早退守卫'),
])

# sysdev.ts：LRU 淘汰循环索引
patch('src/features/u3/sysdev.ts', [
    ('    for (let i = 1; i < next.length; i++) if (next[i].lastUsed < next[oldest].lastUsed) oldest = i;',
     '    for (let i = 1; i < next.length; i++) if ((next[i]?.lastUsed ?? 0) < (next[oldest]?.lastUsed ?? 0)) oldest = i;'),
])

# anchor.ts
patch('src/features/u3/anchor.ts', [
    ('Object.keys(KERNEL_DOMAIN_CHECKS).length === 10 && KERNEL_DOMAIN_CHECKS.anchor.checks === 25',
     'Object.keys(KERNEL_DOMAIN_CHECKS).length === 10 && KERNEL_DOMAIN_CHECKS.anchor?.checks === 25'),
    ('  for (let i = 0; i < flat.length && picked.length < n; i += step) picked.push(flat[i]);',
     '  for (let i = 0; i < flat.length && picked.length < n; i += step) { const p = flat[i]; if (p) picked.push(p); }'),
])

# clockcal.ts：锚点元组守卫
patch('src/features/u3/clockcal.ts', [
    ('  const [sy, sm, sd] = CNY_ANCHORS[i];\n  const [ny, nm, nd] = CNY_ANCHORS[i + 1];\n  return daysFromCivil(sy, sm, sd) + lunarYearDays(LUNAR_INFO[i]) === daysFromCivil(ny, nm, nd);',
     '  const cur = CNY_ANCHORS[i];\n  const nxt = CNY_ANCHORS[i + 1];\n  const info = LUNAR_INFO[i];\n  if (!cur || !nxt || info === undefined) return false;\n  const [sy, sm, sd] = cur;\n  const [ny, nm, nd] = nxt;\n  return daysFromCivil(sy, sm, sd) + lunarYearDays(info) === daysFromCivil(ny, nm, nd);'),
    ('    const [sy, sm, sd] = CNY_ANCHORS[i];\n    const [ny, nm, nd] = CNY_ANCHORS[i + 1];\n    const start = daysFromCivil(sy, sm, sd);\n    const next = daysFromCivil(ny, nm, nd);',
     '    const cur = CNY_ANCHORS[i];\n    const nxt = CNY_ANCHORS[i + 1];\n    const info0 = LUNAR_INFO[i];\n    if (!cur || !nxt || info0 === undefined) return null;\n    const [sy, sm, sd] = cur;\n    const [ny, nm, nd] = nxt;\n    const start = daysFromCivil(sy, sm, sd);\n    const next = daysFromCivil(ny, nm, nd);'),
    ('    const info = LUNAR_INFO[i];\n    const leap = leapMonthOf(info);',
     '    const info = info0;\n    const leap = leapMonthOf(info);'),
])

# winkeys.ts
patch('src/features/u3/winkeys.ts', [
    ('  const slot = slots[digit];\n  if (shift) return { action: "new-instance", appId: slot.appId };',
     '  const slot = slots[digit];\n  if (!slot) return { action: "none" }; // 防御（上方越界守卫已覆盖）\n  if (shift) return { action: "new-instance", appId: slot.appId };'),
    ('  const next = zOrder[zOrder.length - 1]; // Z 序最后 = 最久未用 → 循环后退\n  return { focusId: next, minimizedRestored: true };',
     '  const next = zOrder[zOrder.length - 1] ?? null; // Z 序最后 = 最久未用 → 循环后退\n  if (!next) return { focusId: null, minimizedRestored: false };\n  return { focusId: next, minimizedRestored: true };'),
])

# ledger.ts
patch('src/features/u3/ledger.ts', [
    ('    checksTotal += KERNEL_DOMAIN_CHECKS[d].checks;\n    testsTotal += KERNEL_DOMAIN_CHECKS[d].unitTests;',
     '    checksTotal += KERNEL_DOMAIN_CHECKS[d]?.checks ?? 0;\n    testsTotal += KERNEL_DOMAIN_CHECKS[d]?.unitTests ?? 0;'),
])

# reconcile.ts
patch('src/features/u3/reconcile.ts', [
    ('  const tokens = KERNEL_DOMAIN_CHECKS[d].fRange.split(/[/,]+/);',
     '  const tokens = (KERNEL_DOMAIN_CHECKS[d]?.fRange ?? "").split(/[/,]+/);'),
])

# copyops.ts
patch('src/features/u3/copyops.ts', [
    ('    const need = input.perTargetNeed[vol] * (1 + SPACE_CHECK_BUFFER_PCT / 100);',
     '    const need = (input.perTargetNeed[vol] ?? 0) * (1 + SPACE_CHECK_BUFFER_PCT / 100);'),
    ('  checks.push({ name: "F529 预检拦截", pass: !sc.ok && sc.shortfalls[0].volume === "D:" });',
     '  checks.push({ name: "F529 预检拦截", pass: !sc.ok && sc.shortfalls[0]?.volume === "D:" });'),
    ('  checks.push({ name: "F529 人话文案", pass: shortfallMessage(sc.shortfalls[0]).includes("10% 缓冲") });',
     '  checks.push({ name: "F529 人话文案", pass: !sc.ok && shortfallMessage(sc.shortfalls[0]!).includes("10% 缓冲") });'),
    ('  checks.push({ name: "F531 插队", pass: scheduleQueue(jumped, new Set(), 2)[0].id === "3" });',
     '  checks.push({ name: "F531 插队", pass: scheduleQueue(jumped, new Set(), 2)[0]?.id === "3" });'),
])

# locksec.ts
patch('src/features/u3/locksec.ts', [
    ('    const m = markers[id];\n    const x1 = Math.max(m.x, shot.x);',
     '    const m = markers[id];\n    if (!m) continue;\n    const x1 = Math.max(m.x, shot.x);'),
    ('  checks.push({ name: "F508 黑块几何对齐", pass: rects.length === 1 && rects[0].x === 150 && rects[0].y === 120 && rects[0].w === 150 && rects[0].h === 60 });',
     '  checks.push({ name: "F508 黑块几何对齐", pass: rects.length === 1 && rects[0]?.x === 150 && rects[0]?.y === 120 && rects[0]?.w === 150 && rects[0]?.h === 60 });'),
])

# filesec.ts：未使用 import
patch('src/features/u3/filesec.ts', [
    ('import { u3Store } from "./u3store";\n\n', ''),
])

# pointerfx.ts：kind 参数语义说明
patch('src/features/u3/pointerfx.ts', [
    ('export function soundLightPolicy(kind: SoundEventKind, mode: DisturbMode, perEventEnabled: boolean): { flash: boolean; sound: boolean; logToCenter: boolean } {\n  const evOn = perEventEnabled;',
     'export function soundLightPolicy(kind: SoundEventKind, mode: DisturbMode, perEventEnabled: boolean): { flash: boolean; sound: boolean; logToCenter: boolean } {\n  void kind; // 逐事件开关由调用方读配置后传入（perEventEnabled）——本函数只做档位裁决\n  const evOn = perEventEnabled;'),
])
print("all patched")
