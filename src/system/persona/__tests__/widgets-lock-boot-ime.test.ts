import { beforeEach, describe, expect, it } from "vitest";
import {
  loadWidgetConfig, addWidget, removeWidget, moveWidget,
  updateWidget, clampOpacity, clockIsAccurate, perfGuardAction, REFRESH_MS, RENDER_BUDGET_MS, OPACITY_MIN,
} from "../widgets";
import {
  loadLockScreenConfig, saveLockScreenConfig, isZeroLoopAnimation,
  wakeWithinBudget, WAKE_TIMELINE, WAKE_BUDGET_MS, BIG_DISPLAY_PX, FOCUS_RETREAT, FALLBACK_COLOR,
} from "../lockcustom";
import {
  loadBootSkinConfig, validateBootSkinConfig, particlePalette,
  densityAuditTable, normalizeBackdropLuma, suggestDensity, luminance,
  PARTICLE_COUNTS, BACKDROP_DIM, READABLE_LUMA_MIN, READABLE_LUMA_MAX,
} from "../bootskin";
import {
  loadImeSkinConfig, saveImeSkinConfig, clampImeSkin, validateImeSkin,
  skinForPopup, ImeLatencySelfCheck, needsPagingHint,
  RENDER_BUDGET_MS as IME_BUDGET_MS, OPACITY_FLOOR, HIGHLIGHT_CONTRAST_MIN,
} from "../imeskin";
import { personaStore } from "../store";

beforeEach(() => {
  personaStore.reset();
});

describe("F163 桌面小组件", () => {
  it("三组件增删改位全链", () => {
    let cfg = loadWidgetConfig();
    cfg = addWidget(cfg, "clock", 10, 20);
    cfg = addWidget(cfg, "weather", 100, 20);
    expect(cfg.instances).toHaveLength(2);
    const id = cfg.instances[0]!.id;
    cfg = moveWidget(cfg, id, 50, 60);
    expect(cfg.instances.find((i) => i.id === id)).toMatchObject({ x: 50, y: 60 });
    cfg = updateWidget(cfg, id, { clockStyle: "analog", opacity: 0.5 });
    expect(cfg.instances.find((i) => i.id === id)?.clockStyle).toBe("analog");
    cfg = removeWidget(cfg, id);
    expect(cfg.instances).toHaveLength(1);
  });

  it("透明度 20% 下限护栏", () => {
    expect(OPACITY_MIN).toBe(0.2);
    expect(clampOpacity(0.05)).toBe(0.2);
    expect(clampOpacity(1.5)).toBe(1);
    expect(clampOpacity(0.666)).toBe(0.67);
  });

  it("时钟秒级准确（<1s 判定，F187 强一致）", () => {
    expect(clockIsAccurate(1000, 1200)).toBe(true);
    expect(clockIsAccurate(1000, 2600)).toBe(false);
  });

  it("数据刷新节律：天气 30min / 时钟 1s / 快照 10min", () => {
    expect(REFRESH_MS.clock).toBe(1000);
    expect(REFRESH_MS.weather).toBe(30 * 60 * 1000);
    expect(REFRESH_MS.snapshot).toBe(10 * 60 * 1000);
  });

  it("性能保护：超预算自动降透明 ×0.6；未超不动作", () => {
    let cfg = loadWidgetConfig();
    cfg = addWidget(cfg, "snapshot", 0, 0);
    const okR = perfGuardAction(cfg, 2);
    expect(okR.degrade).toBe(false);
    const badR = perfGuardAction(cfg, 5);
    expect(badR.degrade).toBe(true);
    expect(badR.next.instances[0]?.opacity).toBeCloseTo(0.6);
    expect(RENDER_BUDGET_MS).toBeCloseTo(3.3);
    // 关闭保护则不降
    const off = perfGuardAction({ ...cfg, perfGuard: false }, 99);
    expect(off.degrade).toBe(false);
  });
});

describe("F164 锁屏定制", () => {
  it("三式配置持久化 + 通知隐私默认开启", () => {
    const d = loadLockScreenConfig();
    expect(d.notifyPrivacy).toBe(true);
    expect(d.timeStyle).toBe("big-number");
    saveLockScreenConfig({ ...d, timeStyle: "analog" });
    expect(loadLockScreenConfig().timeStyle).toBe("analog");
  });

  it("零循环动画：5 秒窗口内多帧重绘即违规（跨窗重绘合法）", () => {
    expect(isZeroLoopAnimation([1000])).toBe(true);
    expect(isZeroLoopAnimation([0, 6000, 12000])).toBe(true); // 每 6s 一帧——无 5s 窗口内连续重绘
    expect(isZeroLoopAnimation([5000, 5500, 5900])).toBe(false); // 0.9s 内三帧
  });

  it("唤醒时间线：各段预算与整体 2s 预算", () => {
    expect(WAKE_BUDGET_MS).toBe(2000);
    expect(WAKE_TIMELINE.reduce((a, s) => a + s.budgetMs, 0)).toBe(2000);
    expect(wakeWithinBudget([{ stage: "ACPI 唤醒", elapsedMs: 500 }, { stage: "合成器恢复", elapsedMs: 700 }, { stage: "锁屏层首帧", elapsedMs: 500 }]).ok).toBe(true);
    const over = wakeWithinBudget([{ stage: "合成器恢复", elapsedMs: 1200 }]);
    expect(over.ok).toBe(false);
    expect(over.over).toEqual(["合成器恢复"]);
  });

  it("设计细节常量：大数字 96px / 让位布局 / 兜底色", () => {
    expect(BIG_DISPLAY_PX).toBe(96);
    expect(FOCUS_RETREAT.scale).toBe(0.7);
    expect(FALLBACK_COLOR).toBe("#101018");
  });
});

describe("F165 开机动画个性化", () => {
  it("三档密度粒子数 1200/600/200（帧资产对拍口径）", () => {
    expect(PARTICLE_COUNTS).toEqual({ dense: 1200, standard: 600, minimal: 200 });
    const audit = densityAuditTable();
    expect(audit.map((a) => a.particleCount)).toEqual([1200, 600, 200]);
    expect(audit.every((a) => a.durationOk)).toBe(true);
  });

  it("粒子色映射：主色→基色（80%）/高光（125%）——预览与真播同源", () => {
    const p = particlePalette("#ff8000");
    expect(p.emblem).toBe("#ff8000");
    expect(p.base).toBe("#cc6600");
    expect(p.highlight).toBe("#ffa000");
  });

  it("底图亮度治理：过暗提亮/过亮压暗/区间内不动", () => {
    expect(READABLE_LUMA_MIN).toBeCloseTo(0.12);
    expect(READABLE_LUMA_MAX).toBeCloseTo(0.88);
    const dark = normalizeBackdropLuma("#101010");
    expect(dark.adjusted).toBe(true);
    expect(luminance(dark.hex)).toBeGreaterThanOrEqual(READABLE_LUMA_MIN);
    const bright = normalizeBackdropLuma("#ffffff");
    expect(bright.adjusted).toBe(true);
    expect(luminance(bright.hex)).toBeLessThanOrEqual(READABLE_LUMA_MAX);
    const mid = normalizeBackdropLuma("#808080");
    expect(mid.adjusted).toBe(false);
  });

  it("校验：非法强调色/密度档拒绝；底图暗化 65% 固定", () => {
    expect(validateBootSkinConfig({ ...loadBootSkinConfig(), accentOverride: "orange" }).ok).toBe(false);
    expect(validateBootSkinConfig({ ...loadBootSkinConfig(), density: "hyper" as never }).ok).toBe(false);
    expect(BACKDROP_DIM).toBe(0.65);
  });

  it("弱机降档建议：低电量建议极简；else 不建议", () => {
    expect(suggestDensity("low")).toBe("minimal");
    expect(suggestDensity("high")).toBeNull();
    expect(suggestDensity("medium")).toBeNull();
  });
});

describe("F166 输入法皮肤", () => {
  it("透明度 60% 下限与字号 12-16 护栏钳制", () => {
    expect(OPACITY_FLOOR).toBe(0.6);
    const c = clampImeSkin({ ...loadImeSkinConfig(), opacity: 0.2, fontSize: 30, candidates: 7 as never });
    expect(c.opacity).toBe(0.6);
    expect(c.fontSize).toBe(16);
    expect(c.candidates).toBe(5);
  });

  it("候选高亮对比度门禁 4.5:1（F141 公式复用）", () => {
    expect(HIGHLIGHT_CONTRAST_MIN).toBe(4.5);
    const good = validateImeSkin(loadImeSkinConfig());
    expect(good.highlightContrast).toBeGreaterThanOrEqual(4.5);
    const bad = validateImeSkin({ ...loadImeSkinConfig(), colors: { background: "#888888", text: "#888888", highlight: "#888888", highlightText: "#999999" } });
    expect(bad.ok).toBe(false);
    expect(bad.issues.some((i) => i.includes("4.5"))).toBe(true);
  });

  it("跟随主题：弹出时用令牌注入；独立模式用自配色", () => {
    const cfg = loadImeSkinConfig();
    const theme = { background: "#111111", text: "#eeeeee", highlight: "#ff0000", highlightText: "#ffffff" };
    expect(skinForPopup({ ...cfg, followTheme: true }, theme).colors).toEqual(theme);
    expect(skinForPopup({ ...cfg, followTheme: false, colors: { background: "#222222", text: "#dddddd", highlight: "#00ff00", highlightText: "#000000" } }, theme).colors.background).toBe("#222222");
  });

  it("16ms 红线自检：P99 超标且近期超标率 >10% → 自动回退建议", () => {
    expect(IME_BUDGET_MS).toBe(16);
    const okCheck = new ImeLatencySelfCheck();
    for (let i = 0; i < 60; i++) okCheck.record(10);
    expect(okCheck.verdict()).toMatchObject({ ok: true, shouldRevertToDefault: false });
    const badCheck = new ImeLatencySelfCheck();
    for (let i = 0; i < 55; i++) badCheck.record(30);
    for (let i = 0; i < 5; i++) badCheck.record(2);
    const v = badCheck.verdict();
    expect(v.ok).toBe(false);
    expect(v.shouldRevertToDefault).toBe(true);
  });

  it("9 候选档显示翻页键提示", () => {
    expect(needsPagingHint(9)).toBe(true);
    expect(needsPagingHint(5)).toBe(false);
  });

  it("saveImeSkinConfig 落盘 + 校验一体", () => {
    const r = saveImeSkinConfig({ ...loadImeSkinConfig(), candidates: 9 });
    expect(r.ok).toBe(true);
    expect(loadImeSkinConfig().candidates).toBe(9);
  });
});
