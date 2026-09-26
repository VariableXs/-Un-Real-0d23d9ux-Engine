/**
 * AI-J1 批次七测试 · 十引擎纵深（F601/602/603/606/607/609/613/614/618/620）。
 * 全部纯函数域内测试——vitest node 环境零 DOM 依赖（appRegistry 教训沿用）。
 */
import { describe, expect, it } from "vitest";

import {
  MAX_LIFT,
  PRESS_LIFT,
  REST_LIFT,
  atlasUV,
  groundShadowColor,
  liftFromSpeed,
  pressLift,
  respectReducedMotion,
  shadowFromLift,
} from "../shadowcast";
import {
  CORNER_ENTER_PX,
  CORNER_EXIT_PX,
  CORNER_ZONE_PX,
  SeamCrossMachine,
  cornerHysteresis,
  inCornerZone,
  predictCrossing,
  seamStickiness,
} from "../seamcross";
import {
  BOUNCE_WINDOW_MS,
  CLEAN_STREAK,
  PALM_RADIUS_PX,
  classifyContact,
  feedGuard,
  guardSummary,
  initialGuardState,
} from "../palmguard";
import {
  PRECISION_COOLDOWN_MS,
  PRECISION_RAMP_MS,
  composedGain,
  nudge,
  nudgeStep,
  rampFactor,
  transitionPrecision,
  type PrecisionState,
} from "../precisiontune";
import {
  TILT_COAST_MS,
  TILT_DEADZONE_DEG,
  arbiterTiltPress,
  discreteTiltRate,
  tiltCoast,
  tiltRate,
  tiltZoomStepsPerSec,
} from "../tiltchannel";
import {
  EDGE_COAST_MS,
  edgeReleaseCoast,
  resolveEdgeTarget,
  sameScreenBands,
  type EdgeContainer,
} from "../edgeramp";
import { AUDIT_LIMIT, WheelRuleAudit, resolveRule, tempOverride, tempRemainingSec, validateRules, type WheelRule } from "../wheelrules";
import {
  WIN_SLIDER_MAX,
  WIN_SLIDER_MIN,
  blendCurves,
  blendPreviewTable,
  inspectCurve,
  sensToWinSlider,
  translatorSelfTest,
  winSliderToSens,
} from "../speedspectrum";
import { displayCapability, identityConfidence, mayRestore, physicalRestorePoint, physicalSpeedNorm } from "../displayidentity";
import { PACK_VERSION, exportPack, mergeProfiles, migrateProfilePack, packChecksum, verifyPack, type ProfileSnapshot } from "../profilesync";
import { synthEdid, parseEdid } from "../edid";

/* ------------------------------- F601 纵深：曲线谱学 ------------------------------- */

describe("speedspectrum（F601 纵深）", () => {
  it("灵敏度换算双射 round-trip：11 档全过", () => {
    const t = translatorSelfTest();
    expect(t.pass).toBe(true);
    expect(winSliderToSens(6)).toBe(1); // 第 6 档 = 1x 基准
    expect(winSliderToSens(11)).toBeCloseTo(1.8, 2);
    expect(winSliderToSens(1)).toBeCloseTo(0.2, 2); // 全程为正（0.35 斜率初版缺陷已修）
    expect(sensToWinSlider(1)).toBe(6);
  });

  it("换算器越界显性化（不猜）", () => {
    expect(() => winSliderToSens(0)).toThrow(RangeError);
    expect(() => winSliderToSens(12)).toThrow(RangeError);
    expect(() => winSliderToSens(7.5)).toThrow(RangeError);
    expect(() => sensToWinSlider(0)).toThrow(RangeError);
    expect(() => sensToWinSlider(-5)).toThrow(RangeError); // 非正
    expect(sensToWinSlider(999)).toBe(WIN_SLIDER_MAX); // 反向钳制不抛
    expect(sensToWinSlider(0.01)).toBe(WIN_SLIDER_MIN);
  });

  it("混成端点可预测：t=0 恒等 A、t=1 恒等 B、越界钳制", () => {
    const a = (px: number) => 1 + px / 100;
    const b = () => 2;
    expect(blendCurves(a, b, 0)(50)).toBe(a(50));
    expect(blendCurves(a, b, 1)(50)).toBe(2);
    expect(blendCurves(a, b, -3)(50)).toBe(a(50)); // 钳到 0
    expect(blendCurves(a, b, 9)(50)).toBe(2); // 钳到 1
    const mid = blendCurves(a, b, 0.5)(50);
    expect(mid).toBeCloseTo((a(50) + 2) / 2, 10);
  });

  it("混成预览表三列并排", () => {
    const rows = blendPreviewTable(() => 1, () => 3, 0.5, 32, 96);
    expect(rows).toHaveLength(4); // 0,32,64,96
    expect(rows[2]).toEqual({ px: 64, a: 1, b: 3, mixed: 2 });
  });

  it("体检：单调曲线 pass、下坡曲线 fail 并定位", () => {
    const good = inspectCurve((px) => 0.5 + px / 128);
    expect(good.verdict).toBe("pass");
    expect(good.monotonic).toBe(true);
    expect(good.notes[0]).toContain("五项全过");
    // 真单调 fail：中途下坡（48px 处）后再创新高（峰值在行程末）。
    const bad = inspectCurve((px) => (px < 40 ? 1 + px / 50 : px < 80 ? 1.8 - (px - 40) / 100 : 1.4 + (px - 80) / 25));
    expect(bad.verdict).toBe("fail");
    expect(bad.monotonic).toBe(false);
    expect(bad.firstDip).not.toBeNull();
    expect(bad.notes[0]).toContain("发涩");
  });

  it("体检：零位过低 warn、增益失控 fail、过冲 warn", () => {
    expect(inspectCurve(() => 0.05).verdict).toBe("warn"); // 单调但零位过低
    const crazy = inspectCurve((px) => 1 + px * 0.2);
    expect(crazy.verdict).toBe("fail"); // 失控 >6x
    expect(crazy.gainMax).toBeGreaterThan(6);
    // 过冲曲线：连续（无跳变）、峰值后回落 >3%
    const over = inspectCurve((px) => (px <= 100 ? 0.5 + px / 100 : 1.5 - (px - 100) / 250));
    expect(over.overshoot).toBe(true);
    expect(over.monotonic).toBe(true); // 峰后回落不算中途下坡——两判据分离
    expect(over.verdict).toBe("warn");
  });
});

/* ------------------------------- F602 纵深：精密模式 ------------------------------- */

describe("precisiontune（F602 纵深）", () => {
  it("减速坡道端点精确与对称", () => {
    expect(rampFactor(0, 0.1, true)).toBe(1);
    expect(rampFactor(PRECISION_RAMP_MS, 0.1, true)).toBeCloseTo(0.1, 10);
    expect(rampFactor(0, 0.1, false)).toBeCloseTo(0.1, 10);
    expect(rampFactor(PRECISION_RAMP_MS, 0.1, false)).toBe(1);
    // 中段指数缓动 > 线性（easeOutCubic 前快后慢）
    expect(rampFactor(30, 0.1, true)).toBeLessThan(1 - (0.9 * 30) / PRECISION_RAMP_MS);
  });

  it("键盘微调三档步进与轴吸附", () => {
    expect(nudgeStep({})).toBe(1);
    expect(nudgeStep({ shift: true })).toBe(5);
    expect(nudgeStep({ ctrl: true })).toBe(10);
    expect(nudge({ x: 3, y: 7 }, "left", {})).toEqual({ x: 2, y: 7 });
    expect(nudge({ x: 0, y: 7 }, "left", { shift: true })).toEqual({ x: 0, y: 7 }); // 钳非负
    expect(nudge({ x: 12, y: 34 }, "home", {})).toEqual({ x: 0, y: 34 });
    expect(nudge({ x: 12, y: 34 }, "end", {})).toEqual({ x: 12, y: 0 });
    expect(nudge({ x: 1, y: 1 }, "down", { ctrl: true })).toEqual({ x: 1, y: 11 });
  });

  it("粘滞状态机全转移表", () => {
    let st: PrecisionState = { phase: "idle", sinceMs: null, stickyCount: 0 };
    st = transitionPrecision(st, { kind: "tap", atMs: 0 });
    expect(st.phase).toBe("active");
    st = transitionPrecision(st, { kind: "release", atMs: 50 });
    expect(st.phase).toBe("cooldown");
    // cooldown 防抖：窗内 tap 忽略
    const blocked = transitionPrecision(st, { kind: "tap", atMs: 100 });
    expect(blocked.phase).toBe("cooldown");
    // 窗外 tap 重新激活（cooldown 自 release 时刻起算：50 + 260 + 10）
    st = transitionPrecision(st, { kind: "tap", atMs: 50 + PRECISION_COOLDOWN_MS + 10 });
    expect(st.phase).toBe("active");
    // 三连击粘滞
    st = transitionPrecision(st, { kind: "tripleTap", atMs: 900 });
    expect(st.phase).toBe("sticky");
    expect(st.stickyCount).toBe(1);
    // 粘滞吞 release
    st = transitionPrecision(st, { kind: "release", atMs: 950 });
    expect(st.phase).toBe("sticky");
    // 再按一次退出
    st = transitionPrecision(st, { kind: "tap", atMs: 1000 });
    expect(st.phase).toBe("cooldown");
  });

  it("与 F601 正交：precision=1 时逐位等于原曲线", () => {
    expect(composedGain(1.37, 1)).toBe(1.37);
    expect(composedGain(2, 0.5)).toBe(1);
  });
});

/* ------------------------------- F603 纵深：手掌守门 ------------------------------- */

describe("palmguard（F603 纵深）", () => {
  const cleanSample = { radiusPx: 3, speedPxMs: 0.8, pressure: 0.9, sinceTouchMs: 500, angleDelta: 0 };

  it("手掌签名 reject / 大接触但鲜活 suspect / 正常 clean", () => {
    expect(classifyContact({ ...cleanSample, radiusPx: PALM_RADIUS_PX, speedPxMs: 0.1, pressure: 0.3 })).toBe("reject");
    expect(classifyContact({ ...cleanSample, radiusPx: PALM_RADIUS_PX + 4, speedPxMs: 1.2, pressure: 0.9 })).toBe("suspect");
    expect(classifyContact(cleanSample)).toBe("clean");
  });

  it("落笔弹跳窗：30ms 内反转 reject、窗外同信号 clean", () => {
    const bounce = { ...cleanSample, sinceTouchMs: BOUNCE_WINDOW_MS, angleDelta: Math.PI, speedPxMs: 0.2 };
    expect(classifyContact(bounce)).toBe("reject");
    expect(classifyContact({ ...bounce, sinceTouchMs: BOUNCE_WINDOW_MS + 1 })).toBe("clean");
    // 反转但速度极低（几乎没动）不算弹跳
    expect(classifyContact({ ...bounce, speedPxMs: 0.01 })).toBe("clean");
  });

  it("守门状态机：reject 进入、连续 3 帧 clean 恢复、suspect 不改相位", () => {
    let st = initialGuardState();
    const r = feedGuard(st, { ...cleanSample, radiusPx: PALM_RADIUS_PX + 2, speedPxMs: 0.1, pressure: 0.2 });
    expect(r.verdict).toBe("reject");
    expect(r.next.rejecting).toBe(true);
    expect(r.next.lastReason).toBe("palm-signature");
    st = r.next;
    for (let i = 1; i < CLEAN_STREAK; i++) {
      const step = feedGuard(st, cleanSample);
      expect(step.verdict).toBe("clean");
      expect(step.next.rejecting).toBe(true); // 未满连击仍守门
      st = step.next;
    }
    const done = feedGuard(st, cleanSample);
    expect(done.next.rejecting).toBe(false);
    expect(done.next.lastReason).toBe("recovered");
    // suspect 不改相位
    const before = done.next;
    const sus = feedGuard(before, { ...cleanSample, radiusPx: 30, speedPxMs: 2, pressure: 1 });
    expect(sus.verdict).toBe("suspect");
    expect(sus.next.rejecting).toBe(before.rejecting);
  });

  it("守门摘要带拦截计数", () => {
    let st = initialGuardState();
    expect(guardSummary(st)).toContain("无拦截记录");
    st = feedGuard(st, { ...cleanSample, radiusPx: 20, speedPxMs: 0.1, pressure: 0.2 }).next;
    expect(guardSummary(st)).toContain("已拦 1 帧");
  });
});

/* ------------------------------- F606 纵深：倾斜通道 ------------------------------- */

describe("tiltchannel（F606 纵深）", () => {
  it("死区严格零、线性映射端点、越界钳制", () => {
    expect(tiltRate(0)).toBe(0);
    expect(tiltRate(TILT_DEADZONE_DEG)).toBe(0);
    expect(tiltRate(-TILT_DEADZONE_DEG)).toBe(0); // 死区边界（±）严格零
    expect(tiltRate(10)).toBe(1200);
    expect(tiltRate(-10)).toBe(-1200);
    expect(tiltRate(99)).toBe(1200); // 钳制
    const mid = tiltRate((TILT_DEADZONE_DEG + 10) / 2);
    expect(mid).toBeGreaterThan(200);
    expect(mid).toBeLessThan(1200);
  });

  it("离散降级 = 6° 等效、回中零", () => {
    expect(discreteTiltRate("center")).toBe(0);
    expect(discreteTiltRate("right")).toBe(tiltRate(6));
    expect(discreteTiltRate("left")).toBe(-tiltRate(6));
  });

  it("余韵与 F204 同常数：300ms 处归零、指数中段", () => {
    expect(tiltCoast(0)).toBe(1);
    expect(tiltCoast(TILT_COAST_MS)).toBe(0);
    expect(tiltCoast(100)).toBeGreaterThan(0.2);
    expect(tiltCoast(200)).toBeLessThan(0.5);
  });

  it("先到先得仲裁", () => {
    expect(arbiterTiltPress(null, null)).toBe("none");
    expect(arbiterTiltPress(100, null)).toBe("autoscroll");
    expect(arbiterTiltPress(100, 200)).toBe("autoscroll");
    expect(arbiterTiltPress(200, 100)).toBe("zoom");
    expect(arbiterTiltPress(null, 100)).toBe("zoom");
    expect(arbiterTiltPress(100, 100)).toBe("autoscroll"); // 同帧按压制胜
  });

  it("缩放步频：档位感封顶 12 步/s", () => {
    expect(tiltZoomStepsPerSec(0)).toBe(0);
    expect(tiltZoomStepsPerSec(10)).toBe(12);
    expect(tiltZoomStepsPerSec(-10)).toBe(12); // 对称
    expect(tiltZoomStepsPerSec(5)).toBeLessThan(12);
    expect(tiltZoomStepsPerSec(5)).toBeGreaterThan(0);
  });
});

/* ------------------------------- F607 纵深：接缝状态机 ------------------------------- */

describe("seamcross（F607 纵深）", () => {
  it("角落滞回双阈值：进 +2 退 -6 中间带保持", () => {
    expect(cornerHysteresis(false, CORNER_ENTER_PX + 0.5)).toBe(true);
    expect(cornerHysteresis(false, CORNER_ENTER_PX - 0.5)).toBe(false);
    expect(cornerHysteresis(true, -CORNER_EXIT_PX + 0.5)).toBe(true); // 滞回带保持
    expect(cornerHysteresis(true, -CORNER_EXIT_PX - 0.5)).toBe(false);
    expect(cornerHysteresis(false, 1)).toBe(false);
  });

  it("角落区只在缝端 ±48px", () => {
    expect(inCornerZone(10, 1000)).toBe(true);
    expect(inCornerZone(980, 1000)).toBe(true);
    expect(inCornerZone(500, 1000)).toBe(false);
    expect(inCornerZone(CORNER_ZONE_PX + 1, 1000)).toBe(false);
    expect(CORNER_EXIT_PX).toBeGreaterThan(CORNER_ENTER_PX); // 滞回成立的结构保证
  });

  it("粘滞三档：off 全放行 / light 0.55 / strong 0.12 且 Shift 豁免", () => {
    expect(seamStickiness("off", 3, 1, false)).toBe(1);
    expect(seamStickiness("light", 3, 1, false)).toBeCloseTo(0.55, 10);
    expect(seamStickiness("strong", 3, 1, false)).toBeCloseTo(0.12, 10);
    expect(seamStickiness("strong", 3, 1, true)).toBe(1); // Shift 放行
    expect(seamStickiness("light", 10, 1, false)).toBe(1); // 离缝不放
    expect(seamStickiness("light", 3, 0, false)).toBe(1); // 不朝缝不放
    expect(seamStickiness("light", 3, -1, false)).toBe(1); // 远离缝
  });

  it("交叉预测：高速朝缝才预热、慢速不猜", () => {
    const seam = { nearestX: 100, nearestY: 50, normalX: 1, normalY: 0, targetScale: 1.5 };
    const fast = predictCrossing({ x: 60, y: 50 }, { vx: 2, vy: 0 }, seam);
    expect(fast.willCross).toBe(true);
    expect(fast.targetScale).toBe(1.5);
    expect(fast.framesToSeam).toBe(20); // 40px / 2px per frame
    expect(predictCrossing({ x: 60, y: 50 }, { vx: 0.2, vy: 0 }, seam).willCross).toBe(false);
    expect(predictCrossing({ x: 60, y: 50 }, { vx: -2, vy: 0 }, seam).willCross).toBe(false); // 背向
    expect(predictCrossing({ x: 100, y: 50 }, { vx: 2, vy: 0 }, seam).framesToSeam).toBe(0);
  });

  it("状态机端到端：角落跨入 → 滞回保持 → 退出；长直缝顺滑", () => {
    const m = new SeamCrossMachine();
    // 角落区：深度 +3 → 跨
    const f1 = m.frame({ alongPx: 20, seamLenPx: 1000, depthPx: 3, speedPxMs: 0.5, shiftHeld: false, stickiness: "off" });
    expect(f1.crossed).toBe(true);
    expect(f1.reason).toContain("corner-hysteresis");
    // 深度 -3（滞回带）→ 保持跨
    const f2 = m.frame({ alongPx: 20, seamLenPx: 1000, depthPx: -3, speedPxMs: 0.5, shiftHeld: false, stickiness: "off" });
    expect(f2.crossed).toBe(true);
    // 深度 -10 → 退出
    const f3 = m.frame({ alongPx: 20, seamLenPx: 1000, depthPx: -10, speedPxMs: 0.5, shiftHeld: false, stickiness: "off" });
    expect(f3.crossed).toBe(false);
    // 长直缝：深度 +1 即跨（无滞回拖累）
    const f4 = m.frame({ alongPx: 500, seamLenPx: 1000, depthPx: 1, speedPxMs: 0.5, shiftHeld: false, stickiness: "off" });
    expect(f4.crossed).toBe(true);
    expect(f4.reason).toContain("straight");
  });

  it("粘滞层在未跨贴缝时施压", () => {
    const m = new SeamCrossMachine();
    // 深度 -2 = 近侧贴缝未跨（正深度是对面侧）。
    const f = m.frame({ alongPx: 500, seamLenPx: 1000, depthPx: -2, speedPxMs: 0.5, shiftHeld: false, stickiness: "light" });
    expect(f.crossed).toBe(false);
    expect(f.vxScale).toBeCloseTo(0.55, 10);
  });
});

/* ------------------------------- F609 纵深：边缘滚纵深 ------------------------------- */

describe("edgeramp（F609 纵深）", () => {
  const mk = (id: string, depth: number, remainingPx: number): EdgeContainer => ({ id, depth, remainingPx });

  it("嵌套命中：最内层有余量者胜", () => {
    const r = resolveEdgeTarget([mk("outer", 0, 500), mk("inner", 1, 100)]);
    expect(r.target?.id).toBe("inner");
    expect(r.carried).toBe(false);
  });

  it("内层耗尽接力外层（carried 标记）", () => {
    const r = resolveEdgeTarget([mk("outer", 0, 500), mk("inner", 1, 0)]);
    expect(r.target?.id).toBe("outer");
    expect(r.carried).toBe(true);
  });

  it("全部耗尽显式 null（不猜）", () => {
    expect(resolveEdgeTarget([mk("a", 0, 0)]).target).toBeNull();
    expect(resolveEdgeTarget([]).target).toBeNull();
  });

  it("松手处置：drop 余韵 / 离带即停", () => {
    expect(edgeReleaseCoast("drop", 0)).toBe(1);
    expect(edgeReleaseCoast("drop", EDGE_COAST_MS)).toBe(0);
    expect(edgeReleaseCoast("drop", 100)).toBeGreaterThan(0.2);
    expect(edgeReleaseCoast("pointer-left-band", 0)).toBe(0);
    expect(edgeReleaseCoast("pointer-left-band", 999)).toBe(0);
  });

  it("跨屏边缘：双带唯一归属不双滚", () => {
    const bands = [
      { screenId: "A", bandStart: 1870, bandEnd: 1920, stack: [mk("a", 0, 300)] },
      { screenId: "B", bandStart: 1920, bandEnd: 1970, stack: [mk("b", 0, 300)] },
    ];
    expect(sameScreenBands(bands, 1890)?.screenId).toBe("A");
    expect(sameScreenBands(bands, 1930)?.screenId).toBe("B");
    expect(sameScreenBands(bands, 1920)?.screenId).toBe("B"); // 边界归属右带（左闭右开）
    expect(sameScreenBands(bands, 1000)).toBeNull();
  });
});

/* ------------------------------- F618 纵深：穿透规则 ------------------------------- */

describe("wheelrules（F618 纵深）", () => {
  const r = (layer: WheelRule["layer"], mode: WheelRule["mode"], extra: Partial<WheelRule> = {}): WheelRule => ({
    id: `${layer}-${mode}`,
    layer,
    mode,
    ...extra,
  });

  it("specificity 仲裁：container > app > global、后声明同层胜", () => {
    expect(resolveRule([r("global", "pass"), r("app", "intercept")]).mode).toBe("intercept");
    expect(resolveRule([r("app", "intercept"), r("container", "pass", { appId: "a", container: "menu" })]).mode).toBe("pass");
    expect(resolveRule([r("app", "pass"), r("app", "intercept")]).mode).toBe("intercept");
    expect(resolveRule([r("global", "inherit"), r("app", "inherit")]).mode).toBe("intercept"); // 全 inherit → 安全缺省
    expect(resolveRule([]).winner).toBeNull();
  });

  it("规则校验一次报全", () => {
    const errs = validateRules([
      r("global", "pass", { appId: "x" }), // 全局带应用
      r("app", "pass"), // 应用缺 id
      r("container", "inherit", { appId: "a", container: "menu" }), // 容器 inherit
      { id: "", layer: "app", mode: "pass", appId: "a" }, // 空 id
      r("app", "pass", { appId: "a" }),
      r("app", "pass", { appId: "a" }), // 重复 id
    ]);
    expect(errs.length).toBeGreaterThanOrEqual(5);
    expect(validateRules([r("global", "pass"), r("app", "pass", { appId: "a" })])).toEqual([]);
  });

  it("临时态叠加：优先于规则、过期即失效、倒计时向上取整", () => {
    expect(tempOverride("pass", { expiresAt: 1000, pass: false }, 500)).toBe("intercept");
    expect(tempOverride("intercept", { expiresAt: 1000, pass: true }, 500)).toBe("pass");
    expect(tempOverride("pass", { expiresAt: 1000, pass: false }, 1500)).toBe("pass"); // 过期
    expect(tempOverride("pass", null, 0)).toBe("pass");
    expect(tempRemainingSec({ expiresAt: 10500, pass: true }, 10000)).toBe(1); // 0.5s → 1
    expect(tempRemainingSec({ expiresAt: 9000, pass: true }, 10000)).toBe(0);
    expect(tempRemainingSec({ expiresAt: Infinity, pass: true }, 0)).toBeNull(); // 按住型无倒计时
  });

  it("审计环形 200 条 + lastVerdict", () => {
    const a = new WheelRuleAudit();
    for (let i = 0; i < AUDIT_LIMIT + 30; i++) a.record({ atMs: i, layer: "global", ruleId: null, mode: "intercept", ctx: `c${i}` });
    expect(a.all()).toHaveLength(AUDIT_LIMIT);
    expect(a.lastVerdict()?.ctx).toBe(`c${AUDIT_LIMIT + 29}`); // 最老被挤掉
    a.clear();
    expect(a.lastVerdict()).toBeNull();
  });
});

/* ------------------------------- F620 纵深：影子物理 ------------------------------- */

describe("shadowcast（F620 纵深）", () => {
  it("三联物理一致性：单一 lift 派生三量", () => {
    const rest = shadowFromLift(REST_LIFT);
    expect(rest.offsetPx).toBe(0);
    expect(rest.blurPx).toBe(9);
    expect(rest.opacity).toBeCloseTo(0.38, 3);
    const high = shadowFromLift(MAX_LIFT);
    expect(high.offsetPx).toBeGreaterThan(0);
    expect(high.blurPx).toBeGreaterThan(rest.blurPx);
    expect(high.opacity).toBeLessThan(rest.opacity); // 越高越淡
    expect(shadowFromLift(0.1).offsetPx).toBeLessThan(0); // 越界钳到下限仍物理
  });

  it("速度→高度：静止静息、封顶 MAX", () => {
    expect(liftFromSpeed(0)).toBe(REST_LIFT);
    expect(liftFromSpeed(99)).toBe(MAX_LIFT);
    expect(liftFromSpeed(0.6)).toBeGreaterThan(REST_LIFT);
    expect(liftFromSpeed(0.6)).toBeLessThan(MAX_LIFT);
  });

  it("地面反光：吸收系数与明度差兜底 18%", () => {
    const wall = groundShadowColor({ r: 200, g: 200, b: 200 });
    expect(wall.shadow.r).toBe(164); // 200×0.82
    expect(wall.lumDeltaPct).toBeGreaterThanOrEqual(0.18);
    const dark = groundShadowColor({ r: 20, g: 20, b: 22 });
    expect(dark.lumDeltaPct).toBeGreaterThanOrEqual(0.18); // 暗地面也保明度差
  });

  it("按压呼吸：按下骤降、松开欠阻尼一拍、结束后静息", () => {
    expect(pressLift(0, true)).toBe(PRESS_LIFT);
    const mid = pressLift(45, false);
    expect(mid).toBeGreaterThan(PRESS_LIFT);
    expect(mid).toBeLessThan(REST_LIFT + 0.01);
    expect(pressLift(90, false)).toBeCloseTo(REST_LIFT, 5);
    expect(pressLift(500, false)).toBe(REST_LIFT);
  });

  it("弱动效归零：投影钉静息、动画层关", () => {
    const f = respectReducedMotion(true, 5, true, 10);
    expect(f.offsetPx).toBe(0);
    expect(f.animated).toBe(false);
    const live = respectReducedMotion(false, 1.2, false, 5);
    expect(live.animated).toBe(true);
  });

  it("图集 UV：4 帧等分、序 3 边界", () => {
    expect(atlasUV(0).u0).toBe(0);
    expect(atlasUV(1).u0).toBeCloseTo(0.25, 10);
    expect(atlasUV(3).u1).toBeCloseTo(1, 10);
  });
});

/* ------------------------------- F607/F613 纵深：EDID 消费 ------------------------------- */

describe("displayidentity（EDID 字节通道消费）", () => {
  it("能力档案：PPI/点距实算、尺寸缺失显式 null", () => {
    const e = parseEdid(synthEdid({ manufacturer: "AUS", productCode: 0x1234, serial: 77, hActive: 3840, vActive: 2160 }));
    const withDims = displayCapability(e, { wCm: 61, hCm: 34 });
    expect(withDims.nativeW).toBe(3840);
    expect(withDims.ppi).not.toBeNull();
    expect(withDims.dotPitchMm).not.toBeNull();
    expect(withDims.diagonalIn).toBeGreaterThan(26);
    const noDims = displayCapability(e, null);
    expect(noDims.ppi).toBeNull();
    expect(noDims.dotPitchMm).toBeNull();
  });

  it("三档置信度：full / serial / name", () => {
    const good = parseEdid(synthEdid({ manufacturer: "AUS", productCode: 0x1234, serial: 77 }));
    expect(identityConfidence(good).confidence).toBe("full");
    const broken = synthEdid({ manufacturer: "AUS", productCode: 0x1234, serial: 77 });
    broken[127] = (broken[127]! + 1) % 256; // 破坏校验和
    const bad = parseEdid(broken);
    expect(identityConfidence(bad).confidence).toBe("serial"); // SN 仍在
    const broken2 = synthEdid({ manufacturer: "AUS", productCode: 0x1234, serial: 0 });
    broken2[127] = (broken2[127]! + 1) % 256;
    expect(identityConfidence(parseEdid(broken2)).confidence).toBe("name");
  });

  it("低置信拒绝恢复（F613 宁可丢记忆不可丢确定性）", () => {
    const broken = synthEdid({ manufacturer: "AUS", productCode: 0x1234, serial: 0 });
    broken[127] = (broken[127]! + 1) % 256;
    const denied = mayRestore(parseEdid(broken));
    expect(denied.allowed).toBe(false);
    expect(denied.reason).toContain("序列号");
    expect(mayRestore(parseEdid(synthEdid({ manufacturer: "AUS", productCode: 0x1234, serial: 77 }))).allowed).toBe(true);
  });

  it("物理口径恢复：比例换算与旋转屏双轴", () => {
    expect(physicalRestorePoint({ x: 960, y: 540 }, { w: 1920, h: 1080 }, { w: 3840, h: 2160 })).toEqual({ x: 1920, y: 1080 });
    // 旋转屏：横→竖
    const rot = physicalRestorePoint({ x: 1800, y: 500 }, { w: 1920, h: 1080 }, { w: 1080, h: 1920 });
    expect(rot.x).toBeCloseTo(1013, -1);
    expect(rot.y).toBeCloseTo(889, -1);
    expect(physicalRestorePoint({ x: 5, y: 5 }, { w: 0, h: 0 }, { w: 100, h: 100 })).toEqual({ x: 5, y: 5 }); // 退化输入不炸
  });

  it("PPI 差归一：无物理口径不猜", () => {
    expect(physicalSpeedNorm(null, 160)).toBe(1);
    expect(physicalSpeedNorm(80, 160)).toBe(2);
    expect(physicalSpeedNorm(160, 80)).toBe(0.5);
  });
});

/* ------------------------------- F614/F623 纵深：档案包 ------------------------------- */

describe("profilesync（档案数据生命周期）", () => {
  const mkProfile = (id: string, over: Partial<ProfileSnapshot> = {}): ProfileSnapshot => ({
    id,
    name: `档案 ${id}`,
    sens: 1.2,
    curve: "balanced",
    ...over,
  });

  it("导出快照与运行时隔离", () => {
    const src = [mkProfile("a")];
    const pack = exportPack(src, "2026-09-28T10:00:00");
    src[0]!.sens = 9;
    expect(pack.profiles[0]!.sens).toBe(1.2); // 快照不动
    expect(pack.formatVersion).toBe(PACK_VERSION);
    expect(pack.checksum).toBe(packChecksum(pack.profiles));
  });

  it("校验三关：结构/版本/校验和", () => {
    const pack = exportPack([mkProfile("a")], "2026-09-28T10:00:00");
    expect(verifyPack(pack).ok).toBe(true);
    expect(verifyPack(null).ok).toBe(false);
    expect(verifyPack({ formatVersion: 2 }).ok).toBe(false); // 缺 profiles
    expect(verifyPack({ ...pack, formatVersion: 99 }).ok).toBe(false);
    expect(verifyPack({ ...pack, checksum: "deadbeef" }).ok).toBe(false);
    const noChecksum = { ...pack };
    delete (noChecksum as { checksum?: string }).checksum;
    expect(verifyPack(noChecksum).ok).toBe(true); // 无校验和可收（宽容旧生产者）
  });

  it("v1→v2 迁移链：补 appOverrides、curve 归一", () => {
    const v1 = { formatVersion: 1, exportedAt: "old", checksum: "x", profiles: [mkProfile("a", { curve: "Custom", appOverrides: undefined })] };
    const migrated = migrateProfilePack(v1 as Parameters<typeof migrateProfilePack>[0]);
    expect(migrated.formatVersion).toBe(2);
    expect(migrated.profiles[0]!.curve).toBe("custom");
    expect(migrated.profiles[0]!.appOverrides).toEqual({});
  });

  it("冲突三选：skip/replace/merge 默认字段级补入", () => {
    const existing = [mkProfile("a", { sens: 1.5, wheelMode: "notch" })];
    const incoming = [mkProfile("a", { sens: 9, appOverrides: { ide: "ide-profile" } })];
    // merge：冲突字段保留现有、缺失字段补入、身份字段永不覆盖
    const m = mergeProfiles(existing, incoming, "merge");
    expect(m.report.merged).toBe(1);
    expect(m.result[0]!.sens).toBe(1.5);
    expect(m.result[0]!.wheelMode).toBe("notch");
    expect(m.result[0]!.appOverrides).toEqual({ ide: "ide-profile" });
    expect(m.report.mergedFields[0]!.fields).toContain("appOverrides");
    // skip
    expect(mergeProfiles(existing, incoming, "skip").report.skipped).toBe(1);
    // replace
    const rep = mergeProfiles(existing, incoming, "replace");
    expect(rep.report.replaced).toBe(1);
    expect(rep.result[0]!.sens).toBe(9);
    // 全新入库
    const add = mergeProfiles([], incoming, "merge");
    expect(add.report.added).toBe(1);
  });
});
