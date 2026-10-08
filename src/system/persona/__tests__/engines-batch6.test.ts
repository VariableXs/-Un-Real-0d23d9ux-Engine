import { describe, expect, it } from "vitest";
import {
  flattenMenu, menuNavigate, auditMenuDepth, layoutTray, trayHitIndex,
} from "../ctxnav";
import {
  normalizeEvent, displayKey, isModifierOnly, ChordMatcher, SEQUENCE_TIMEOUT_MS,
  auditAllBindings, eventFromCombo,
} from "../chord-engine";
import {
  renderReport, diffRuns, regressionMessage, detectFlaky, auditEvidence,
  buildEvidencePackage, verifyEvidencePackage, fnv1a, EVIDENCE_MAX_AGE_MS,
} from "../verdict-report";
import {
  generateJsonSchema, toCssVariables, fromCssVariables, applyPatch,
  scaleFontTable, verifyVariableContract, CSS_VAR_EXPECTED_COUNT,
} from "../tokens-schema";
import {
  solarTimes, dayOfYear, compareWithCooper, polarStrategy, planPreload,
  preloadFallback, multiMonitorPreloadDuration,
} from "../wallpaper-solar";
import { defaultTokenTable, FONT_KEYS } from "../tokens";
import type { ItemVerdict } from "../verdict";

// ---------- ctxnav ----------

const MENU = [
  { id: "open", label: "打开", accelerator: "O" },
  { id: "sep", label: "-", disabled: true },
  { id: "rename", label: "重命名", accelerator: "R", children: [{ id: "f2", label: "F2 就地改名" }] },
  { id: "delete", label: "删除", locked: true },
  { id: "empty", label: "空子菜单", children: [{ id: "x", label: "x", disabled: true }] },
];

describe("ctxnav · 菜单键盘导航（F167/F168 深化）", () => {
  it("扁平化保序 + 深度标注", () => {
    const flat = flattenMenu(MENU);
    expect(flat.map((f) => f.item.id)).toEqual(["open", "sep", "rename", "f2", "delete", "empty", "x"]);
    expect(flat.find((f) => f.item.id === "f2")!.depth).toBe(1);
  });

  it("down/up 回绕且跳过禁用项；home/end 直达", () => {
    let focus: string[] | null = null;
    let r = menuNavigate(MENU, focus, { type: "down" });
    expect(r).toEqual({ type: "focus", path: ["open"] });
    focus = r.type === "focus" ? r.path : null;
    r = menuNavigate(MENU, focus, { type: "down" });
    expect(r.type === "focus" && r.path).toEqual(["rename"]); // sep 被跳过。
    focus = r.type === "focus" ? r.path : null;
    r = menuNavigate(MENU, focus, { type: "up" });
    expect(r.type === "focus" && r.path).toEqual(["open"]);
    r = menuNavigate(MENU, focus, { type: "end" });
    expect(r.type === "focus" && r.path).toEqual(["empty"]);
    // 回绕：从 empty 再 down 回到 open。
    focus = r.type === "focus" ? r.path : null;
    r = menuNavigate(MENU, focus, { type: "down" });
    expect(r.type === "focus" && r.path).toEqual(["open"]);
  });

  it("right 开子菜单、left 回父级、Enter 激活叶子", () => {
    let r = menuNavigate(MENU, ["rename"], { type: "right" });
    expect(r.type).toBe("submenu-open");
    expect(r.type === "submenu-open" && r.path).toEqual(["rename", "f2"]);
    r = menuNavigate(MENU, ["rename", "f2"], { type: "left" });
    expect(r.type).toBe("submenu-close");
    r = menuNavigate(MENU, ["open"], { type: "enter" });
    expect(r).toEqual({ type: "activate", path: ["open"] });
  });

  it("锁定项：可聚焦、Enter 不激活（防自残）；禁用项 Enter 忽略", () => {
    expect(menuNavigate(MENU, ["delete"], { type: "enter" }).type).toBe("ignored");
    expect(menuNavigate(MENU, ["empty"], { type: "enter" }).type).toBe("ignored"); // 全禁用子菜单。
  });

  it("type-ahead：加速器跳转、同字母循环轮转、无命中 ignored", () => {
    const r1 = menuNavigate(MENU, ["open"], { type: "type-ahead", ch: "r" });
    expect(r1.type === "focus" && r1.path).toEqual(["rename"]);
    const r2 = menuNavigate(MENU, ["open"], { type: "type-ahead", ch: "z" });
    expect(r2.type).toBe("ignored");
    // 循环：焦点在 rename，按 o 回绕到 open。
    const r3 = menuNavigate(MENU, ["rename"], { type: "type-ahead", ch: "o" });
    expect(r3.type === "focus" && r3.path).toEqual(["open"]);
  });

  it("F215 层级审计：超两级路径显性列出", () => {
    const deep = auditMenuDepth([{ id: "a", label: "A", children: [{ id: "b", label: "B", children: [{ id: "c", label: "C" }] }] }]);
    expect(deep).toHaveLength(1);
    expect(deep[0]!.depth).toBe(3);
    expect(auditMenuDepth(MENU)).toHaveLength(0);
  });

  it("托盘溢出：钉选优先、容量外最少使用进折叠、钉选挤出显性报告", () => {
    const icons = [
      { id: "p1", w: 32, lastUsedAt: 100, pinned: true },
      { id: "u1", w: 32, lastUsedAt: 900, pinned: false },
      { id: "u2", w: 32, lastUsedAt: 500, pinned: false },
      { id: "u3", w: 32, lastUsedAt: 100, pinned: false },
      { id: "u4", w: 32, lastUsedAt: 10, pinned: false },
    ];
    const lay = layoutTray(icons, 160); // 容量约 (160-20+6)/38 ≈ 3。
    expect(lay.visible.map((i) => i.id)).toContain("p1"); // 钉选在可见区。
    expect(lay.visible.length).toBeLessThanOrEqual(4);
    expect(lay.overflow.length).toBeGreaterThan(0);
    expect(lay.chevronX).toBeGreaterThan(0);
    expect(lay.panel.cols).toBeLessThanOrEqual(4);
    // 钉选挤出场景：容量被钉选占满。
    const tight = layoutTray([{ id: "a", w: 32, lastUsedAt: 1, pinned: true }, { id: "b", w: 32, lastUsedAt: 2, pinned: true }], 40);
    expect(tight.pinnedOverflow.length).toBeGreaterThan(0);
  });

  it("托盘热区：图标命中、间隙不命中（C-6 响应的几何面）", () => {
    expect(trayHitIndex(10, 3)).toBe(0);
    expect(trayHitIndex(40, 3)).toBe(1); // 32+6=38 周期 → 图标 1 起于 38。
    expect(trayHitIndex(34, 3)).toBeNull(); // 间隙（32..38 之间）。
    expect(trayHitIndex(-1, 3)).toBeNull();
    expect(trayHitIndex(100, 2)).toBeNull(); // 越界。
  });
});

// ---------- chord-engine ----------

describe("chord-engine · 组合匹配（F169 深化）", () => {
  it("事件归一：规范序 Ctrl>Alt>Shift>Meta>键、显示键名映射", () => {
    expect(normalizeEvent({ key: "a", ctrl: true, alt: false, shift: true, meta: false })).toBe("Ctrl+Shift+A");
    expect(normalizeEvent({ key: "ArrowUp", ctrl: false, alt: false, shift: false, meta: false })).toBe("↑");
    expect(displayKey("Escape")).toBe("Esc");
    expect(displayKey(" ")).toBe("Space");
    expect(displayKey("q")).toBe("Q");
  });

  it("纯修饰键不触发；Esc 清缓冲", () => {
    expect(isModifierOnly({ key: "Control", ctrl: true, alt: false, shift: false, meta: false })).toBe(true);
    expect(isModifierOnly({ key: "k", ctrl: true, alt: false, shift: false, meta: false })).toBe(false);
  });

  it("单段直配；两段序列 pending → match；超时作废", () => {
    const m = new ChordMatcher([
      { actionId: "save", sequence: ["Ctrl+S"], enabled: true },
      { actionId: "copy-down", sequence: ["Ctrl+K", "Ctrl+C"], enabled: true },
    ]);
    const ev = eventFromCombo("Ctrl+S")!;
    expect(m.feed(ev, 0).type).toBe("match");
    const k = eventFromCombo("Ctrl+K")!;
    const p = m.feed(k, 100);
    expect(p.type).toBe("pending");
    expect(m.pending).toEqual(["Ctrl+K"]);
    const c = eventFromCombo("Ctrl+C")!;
    const hit = m.feed(c, 300);
    expect(hit.type).toBe("match");
    expect(hit.type === "match" && hit.actionId).toBe("copy-down");
    // 超时：序列首段后 >1s。
    m.feed(k, 1000);
    expect(m.feed(eventFromCombo("Ctrl+C")!, 1000 + SEQUENCE_TIMEOUT_MS + 10).type).toBe("none");
    expect(m.pending).toEqual([]);
  });

  it("序列失败：第二段不匹配 → 缓冲清空（诚实不串段）", () => {
    const m = new ChordMatcher([{ actionId: "x", sequence: ["Ctrl+K", "Ctrl+C"], enabled: true }]);
    m.feed(eventFromCombo("Ctrl+K")!, 0);
    expect(m.feed(eventFromCombo("Ctrl+S")!, 100).type).toBe("none");
    expect(m.pending).toEqual([]);
  });

  it("冲突绑定谁也不触发（conflict 显性化）；组合期让位（B-904）", () => {
    const m = new ChordMatcher([
      { actionId: "a1", sequence: ["Ctrl+S"], enabled: true },
      { actionId: "a2", sequence: ["Ctrl+S"], enabled: true },
    ]);
    const r = m.feed(eventFromCombo("Ctrl+S")!, 0);
    expect(r.type).toBe("conflict");
    expect(r.type === "conflict" && r.actionIds).toEqual(["a2"]); // 首个被挤出的 a1 + 登记的 a2。
    // 组合期：不匹配不清缓冲。
    const m2 = new ChordMatcher([{ actionId: "seq", sequence: ["Ctrl+K", "Ctrl+C"], enabled: true }]);
    m2.feed(eventFromCombo("Ctrl+K")!, 0);
    expect(m2.feed(eventFromCombo("Ctrl+C")!, 50, true).type).toBe("none");
    expect(m2.pending).toEqual(["Ctrl+K"]); // 缓冲保持。
  });

  it("全表一致性对拍：正常绑定通过、冲突被抓、非法组合被拒", () => {
    const bindings = [
      { actionId: "save", sequence: ["Ctrl+S"], enabled: true },
      { actionId: "copy", sequence: ["Ctrl+K", "Ctrl+C"], enabled: true },
      { actionId: "dup", sequence: ["Ctrl+S"], enabled: true },
      { actionId: "off", sequence: ["Ctrl+Q"], enabled: false },
    ];
    const audit = auditAllBindings(bindings);
    expect(audit.total).toBe(3); // 停用项不参与。
    expect(audit.checks.find((c) => c.binding.actionId === "save")!.consistent).toBe(false);
    expect(audit.checks.find((c) => c.binding.actionId === "save")!.reason).toContain("冲突");
    expect(audit.checks.find((c) => c.binding.actionId === "copy")!.consistent).toBe(true);
    expect(eventFromCombo("Ctrl+坏")).toBeNull(); // 不规范组合拒绝。
  });
});

// ---------- verdict-report ----------

function fakeItem(id: string, pass: boolean): ItemVerdict {
  return {
    id,
    name: `项${id}`,
    probe: "三步",
    steps: [
      { step: "mutate", ok: pass, detail: pass ? "ok" : "写入后值未变化", evidence: "h" },
      { step: "take-effect", ok: pass, detail: "ok", evidence: "h" },
      { step: "rollback", ok: pass, detail: "ok", evidence: "h" },
    ],
    pass,
  };
}
function fakeRun(runId: string, passIds: string[], finishedAt: number): import("../verdict-report").VerdictRun {
  const items = ["F151", "F152", "F170"].map((id) => fakeItem(id, passIds.includes(id)));
  return {
    runId, startedAt: finishedAt - 6000, finishedAt,
    verdict: { items, passed: items.filter((i) => i.pass).length, total: 3, allGreen: passIds.length === 3, elapsedMinutes: 0.1, withinBudget: true, evidenceComplete: true },
  };
}

describe("verdict-report · 总检报告（F170 深化）", () => {
  it("报告渲染：红项清单 + 明细表 + 预算标注", () => {
    const run = fakeRun("r1", ["F151", "F152"], Date.now());
    const rep = renderReport(run);
    expect(rep.passCount).toBe(2);
    expect(rep.failCount).toBe(1);
    expect(rep.markdown).toContain("## 红项");
    expect(rep.markdown).toContain("F170");
    expect(rep.markdown).toContain("预算 1800s");
  });

  it("跨轮回归：新红/修复/持续红/稳定四分法", () => {
    const prev = fakeRun("a", ["F151", "F152"], 1000);
    const curr = fakeRun("b", ["F151", "F170"], 2000);
    const d = diffRuns(prev, curr);
    expect(d.regressed).toEqual(["F152"]);
    expect(d.fixed).toEqual(["F170"]);
    expect(d.persistentlyFailing).toEqual([]);
    expect(d.stable).toEqual(["F151"]);
    expect(regressionMessage(d)).toContain("新增红");
    const clean = fakeRun("c1", ["F151", "F152", "F170"], 1000);
    expect(regressionMessage(diffRuns(clean, clean))).toContain("可归档");
  });

  it("抖动检测：红绿交替 = flaky、全绿稳定、全红持续红", () => {
    const runs = [fakeRun("a", ["F151"], 1), fakeRun("b", [], 2), fakeRun("c", ["F151"], 3)];
    const flaky = detectFlaky(runs, "F151");
    expect(flaky.flaky).toBe(true);
    expect(flaky.message).toContain("2 绿");
    expect(detectFlaky(runs, "F151").history).toEqual([true, false, true]);
    const stable = detectFlaky([fakeRun("a", ["F152"], 1), fakeRun("b", ["F152"], 2)], "F152");
    expect(stable.flaky).toBe(false);
  });

  it("证据审计：三步缺一报缺、过期证据报 freshness", () => {
    const now = Date.now();
    const fresh = fakeRun("r", ["F151", "F152", "F170"], now);
    expect(auditEvidence(fresh, now).complete).toBe(true);
    const staleRun = fakeRun("old", ["F151", "F152", "F170"], now - EVIDENCE_MAX_AGE_MS - 1000);
    const audit = auditEvidence(staleRun, now);
    expect(audit.complete).toBe(false);
    expect(audit.gaps.every((g) => g.missing.includes("freshness"))).toBe(true);
    expect(audit.gaps[0]!.human).toContain("过期");
  });

  it("证据包：哈希验真通过、篡改拒绝（FNV-1a 对拍）", () => {
    const run = fakeRun("r", ["F151", "F152", "F170"], Date.now());
    const pkg = buildEvidencePackage(run);
    expect(verifyEvidencePackage(pkg).ok).toBe(true);
    const tampered = { ...pkg, items: pkg.items.map((i) => (i.id === "F151" ? { ...i, pass: !i.pass } : i)) };
    const v = verifyEvidencePackage(tampered);
    expect(v.ok).toBe(false);
    expect(v.reason).toContain("篡改");
    expect(verifyEvidencePackage({ ...pkg, format: "bogus" as never }).ok).toBe(false);
    expect(fnv1a("a")).not.toBe(fnv1a("b")); // 不同输入不同哈希。
    expect(fnv1a("a")).toBe(fnv1a("a")); // 确定性。
  });
});

// ---------- tokens-schema ----------

describe("tokens-schema · Schema 与变量桥（F151 深化）", () => {
  it("JSON Schema：24 色枚举、曲线/时长 enum、version const", () => {
    const { schema } = generateJsonSchema();
    const colors = (schema as { properties: { colors: { propertyNames: { enum: string[] } } } }).properties.colors.propertyNames.enum;
    expect(colors).toHaveLength(24);
    const motion = (schema as { properties: { motion: { properties: Record<string, { properties: { curve: { enum: string[] } } }> } } }).properties.motion;
    expect(motion.properties!.enter!.properties.curve.enum).toEqual(["enter", "exit", "emphasized", "spring", "linear"]);
    expect((schema as { properties: { version: { const: number } } }).properties.version.const).toBe(1);
  });

  it("CSS 变量桥：45 变量契约 + 反向回收 round-trip", () => {
    const base = defaultTokenTable();
    const vars = toCssVariables(base);
    expect(Object.keys(vars)).toHaveLength(CSS_VAR_EXPECTED_COUNT);
    expect(vars["--p-r-card"]).toBe("12px"); // 键名对齐 tokens.css 既有约定（--p-r-*）。
    expect(vars["--p-motion-micro"]).toBe("120ms");
    expect(vars["--p-sp-1"]).toBe("4px");
    const back = fromCssVariables(vars);
    expect(back.rejected).toHaveLength(0);
    expect(back.patch.colors!["--p-accent"]).toBe(base.colors["--p-accent"]);
    const patched = applyPatch(base, back.patch);
    expect(patched.colors).toEqual(base.colors);
    expect(patched.radius).toEqual(base.radius);
    expect(patched.font).toEqual(base.font);
  });

  it("反向清洗：非法值逐条拒绝、派生变量跳过、未知 --p-* 键显性化", () => {
    const r = fromCssVariables({ "--p-accent": "red", "--p-r-card": "5.5px", "--p-fs-body": "3px", "--p-ease-enter": "cubic-bezier(0.1,1,0.3,1)", "--p-motion-micro": "120ms", "--p-sp-1": "4px", "--p-unknown": "1", "--other": "2" });
    const reasons = r.rejected.map((x) => x.reason).join("|");
    expect(reasons).toContain("hex");
    expect(reasons).toContain("0-48");
    expect(reasons).toContain("9-96");
    expect(reasons).toContain("未知 --p-*");
    // 派生/只读变量（ease/motion/sp）跳过不拒绝——它们不是用户可改面。
    expect(r.rejected.some((x) => x.key.startsWith("--p-ease-"))).toBe(false);
    expect(r.rejected.some((x) => x.key.startsWith("--p-motion-"))).toBe(false);
    expect(r.rejected.some((x) => x.key.startsWith("--p-sp-"))).toBe(false);
    expect(r.rejected.some((x) => x.key === "--other")).toBe(false); // 非 --p- 前缀不管（不是本域面）。
    expect(r.patch.motion).toBeUndefined(); // 动效补丁不来自 CSS 变量（宪法值派生）。
  });

  it("补丁应用：default 为底、逐键覆盖、版本回写", () => {
    const base = defaultTokenTable();
    const patched = applyPatch(base, { colors: { "--p-accent": "#123456" } });
    expect(patched.colors["--p-accent"]).toBe("#123456");
    expect(patched.colors["--p-bg-canvas"]).toBe(base.colors["--p-bg-canvas"]);
    expect(patched.version).toBe(1);
  });

  it("字号缩放：1.25× 等比、caption 底线钳制、非法系数拒绝", () => {
    const base = defaultTokenTable();
    const r = scaleFontTable(base, 1.25);
    expect(r.scaled.body).toBe(Math.round(base.font.body * 1.25));
    expect(r.clamped).toHaveLength(0);
    const tiny = scaleFontTable({ ...base, font: { ...base.font, caption: 7 } }, 1);
    expect(tiny.scaled.caption).toBe(9);
    expect(tiny.clamped).toContain("caption");
    expect(() => scaleFontTable(base, 0)).toThrow();
    expect(() => scaleFontTable(base, Number.NaN)).toThrow();
    expect(FONT_KEYS).toHaveLength(4);
  });

  it("变量契约自证：数量与缺键双检", () => {
    const v = verifyVariableContract(defaultTokenTable());
    expect(v.ok).toBe(true);
    expect(v.actual).toBe(v.expected);
    expect(v.missing).toEqual([]);
  });
});

// ---------- wallpaper-solar ----------

describe("wallpaper-solar · 天文历（F153/F154 深化）", () => {
  it("NOAA 高精度：北京夏至日出≈04:45、日落≈19:46（±10min 天文历对照）", () => {
    const t = solarTimes(new Date(2026, 5, 21), 39.9, 116.4, 480)!;
    expect(t).not.toBeNull();
    expect(Math.abs(t.sunrise - 285)).toBeLessThanOrEqual(10); // 04:45 ±10min。
    expect(Math.abs(t.sunset - 1186)).toBeLessThanOrEqual(10); // 19:46 ±10min。
    expect(t.source).toBe("noaa-high");
  });

  it("极昼极夜显性化：北纬 80° 夏至 null、参数越界抛错", () => {
    expect(solarTimes(new Date(2026, 5, 21), 80, 0, 0)).toBeNull();
    expect(solarTimes(new Date(2026, 11, 21), -80, 0, 0)).toBeNull();
    expect(() => solarTimes(new Date(), 95, 0, 0)).toThrow();
    expect(() => solarTimes(new Date(), 0, 200, 0)).toThrow();
  });

  it("与 Cooper 兜底对拍：天文日内禀一致性 + 兜底差值有界", () => {
    const t = solarTimes(new Date(2026, 8, 1), 39.9, 116.4, 480)!;
    // 内禀一致性：正午 = (日出+日落)/2 = 720 - 4×经度 - 均时差 + 时区（NOAA 公式恒等式）。
    const noonExpect = 720 - 4 * 116.4 - t.eqTimeMin + 480;
    const noonActual = (t.sunrise + t.sunset) / 2;
    expect(Math.abs(noonActual - noonExpect)).toBeLessThanOrEqual(1);
    expect(t.sunrise).toBeLessThan(t.sunset);
    // 与 Cooper 简化式的差值有界（Cooper 赤纬误差 ~1.5° 在北京纬度 ≤ ~20 分钟）。
    const cooper = { sunrise: 345, sunset: 1120 }; // 9 月 1 日北京近似值。
    const cmp = compareWithCooper(t, cooper);
    expect(cmp.sunriseDelta).toBeLessThanOrEqual(20);
    expect(cmp.sunsetDelta).toBeLessThanOrEqual(20);
  });

  it("极昼策略：null 归宿显性（不静默）", () => {
    const mid = solarTimes(new Date(2026, 5, 21), 39.9, 116.4, 480);
    expect(polarStrategy(mid, 39.9)).toBeNull(); // 有数据 → 无策略。
    const polar = polarStrategy(null, 80);
    expect(polar).not.toBeNull();
    expect(["dark", "light"]).toContain(polar!.side);
  });

  it("dayOfYear：闰年/平年正确", () => {
    expect(dayOfYear(new Date(2026, 0, 1))).toBe(1);
    expect(dayOfYear(new Date(2026, 11, 31))).toBe(365);
    expect(dayOfYear(new Date(2028, 11, 31))).toBe(366);
  });

  it("预载规划：窗尾落点、跨午夜窗按明日轴求交、装不下显性", () => {
    // 跨天：now 15:00、换图次日 08:00、空闲窗 22:00-06:00 → 明日段窗尾落点 05:48。
    const p = planPreload({ idleWindows: [{ fromMin: 1320, toMin: 360 }], durationMin: 12, changeAtMin: 480, nowMin: 900 });
    expect(p).not.toBeNull();
    expect(p!.atMin).toBe(1440 + 360 - 12); // 1788 = 明日 05:48（窗尾 06:00 前留满 12min）。
    expect(p!.fits).toBe(true);
    expect(p!.retryAtMin).toBeLessThan(1440 + 360); // 重试点仍在窗内。
    // 同日窗：窗尾落点。
    const sameDay = planPreload({ idleWindows: [{ fromMin: 600, toMin: 1200 }], durationMin: 30, changeAtMin: 1300, nowMin: 900 });
    expect(sameDay!.atMin).toBe(1170);
    // 无可用窗：时长超过任何交段 → 显性 null。
    const none = planPreload({ idleWindows: [{ fromMin: 1000, toMin: 1100 }], durationMin: 120, changeAtMin: 480, nowMin: 900 });
    expect(none).toBeNull();
    expect(preloadFallback(null).action).toBe("reuse-yesterday");
    const tight = planPreload({ idleWindows: [{ fromMin: 1000, toMin: 1200 }], durationMin: 90, changeAtMin: 1300, nowMin: 1000 });
    expect(tight!.fits).toBe(true);
    expect(preloadFallback(tight).message).toContain("零等待");
  });

  it("多屏预载预算：并行度 2 封顶（带宽诚实）", () => {
    expect(multiMonitorPreloadDuration(10, 1)).toBe(10);
    expect(multiMonitorPreloadDuration(10, 2)).toBe(10);
    expect(multiMonitorPreloadDuration(10, 4)).toBe(20);
    expect(() => multiMonitorPreloadDuration(0, 1)).toThrow();
  });
});
