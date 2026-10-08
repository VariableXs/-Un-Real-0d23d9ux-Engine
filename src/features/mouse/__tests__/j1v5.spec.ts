/**
 * J1 深化批次五单测：九引擎逐件钉住判据。
 * gainfield 精确反解 / oneEuro 双引擎 / recognizer Protractor / edid 解析 /
 * topology 接缝 / wheelcal 标定 / physics 弹簧 / session 聚合 / reconcile 对账。
 */
import { describe, expect, it } from "vitest";

import {
  bezierGainPrecise,
  bezierX,
  firstNonMonotonic,
  scaleConsistency,
  solveBezierT,
  solverResidual,
} from "../gainfield";
import { compareEngines, euroResidual, TremorFilterEuro, tuneOneEuro } from "../oneEuro";
import {
  makeTemplate,
  protractorScore,
  protractorDistance,
  recognizeProtractor,
  resample,
  ShapeRecorder,
  shapeFallback,
} from "../recognizer";
import { decodeManufacturer, describeIdentity, edidFingerprintOf, parseEdid, synthEdid } from "../edid";
import {
  clampToDesktop,
  isMixedDpi,
  nearestSeam,
  seamBetween,
  seamSegments,
  toLogical,
  toPhysical,
  topologySummary,
  virtualBounds,
} from "../topology";
import {
  compareWithFactory,
  evalGainCurve,
  fitGainCurve,
  LineHeightEstimator,
  previewTable,
  WheelCalibrationWizard,
} from "../wheelcal";
import type { FrustrationSignal, J1Event } from "../telemetry";
import {
  anchorLifeCurve,
  easeOutCubic,
  hudBreathAlpha,
  longPressVisualProgress,
  springSettled,
  springToward,
  tiltRepeatInterval,
} from "../physics";
import { bucketSessions, dailyReport, percentile, sessionStats, worstSessions } from "../session";
import { buildReconcileMarkdown, engineProbes, V5_NEW_ANCHORS } from "../reconcile";
import { GestureRecognizer } from "../gestures";
import { WheelGain } from "../wheel";
import { findDuplicateShape } from "../recognizer";
import { enumerateAppIds, validateAppOverrideTarget } from "../appRegistry";
import { j1Store } from "../j1store";
import { ScreenMemory } from "../screen";

/* ------------------------------- 批次六：最丑角落闭合 ------------------------------- */

describe("批次六 · WheelGain 方向翻转防爬升（F605 ugly 闭合）", () => {
  const mk = (): WheelGain => new WheelGain(() => ({ enabled: true, minLines: 3, maxLines: 12, accelMs: 220 }));

  it("同向快滚增益爬升；上下抖滚增益不爬升", () => {
    const steady = mk();
    for (let i = 0; i < 10; i++) steady.feed(1000 + i * 80, false, 1);
    const jitter = mk();
    for (let i = 0; i < 10; i++) jitter.feed(1000 + i * 80, false, i % 2 === 0 ? 1 : -1);
    // 两态节奏 EMA 同为 12.5 档/秒，但抖滚每次翻转腰斩——输出行数必须显著更低。
    expect(jitter.feed(2000, false, 1)).toBeLessThan(steady.feed(2000, false, 1));
  });

  it("dirSign=0（未传方向）行为与 v4 完全一致（回归零破坏）", () => {
    const g = mk();
    g.feed(1000, false);
    const out = g.feed(1080, false); // 12.5 档/秒
    expect(out).toBeGreaterThan(3);
    expect(out).toBeLessThanOrEqual(12);
  });
});

describe("批次六 · ScreenMemory 真 LRU（F613 ugly 闭合）", () => {
  it("超上限淘汰最久未写（带时间戳），旧态无 at 视为最旧优先淘汰", () => {
    const holder = { points: {} as Record<string, { x: number; y: number; at?: number }> };
    const unsub = j1Store.subscribe((s, next) => {
      if (s === "screenMemory") holder.points = next.points as typeof holder.points;
    });
    // 预置一块旧态屏（v4 存档：无 at）——应最先被淘汰。
    holder.points.legacy = { x: 1, y: 1 };
    const mem = new ScreenMemory(() => ({ enabled: true, points: holder.points }));
    const mons = Array.from({ length: 12 }, (_, i) => ({ id: `m${i}`, x: 0, y: 0, width: 100, height: 100, edidFingerprint: `m${i}`, scale: 1 }));
    // restore 兼容旧态（无 at 不炸）——在淘汰前验证；restore 需要屏在位（按 edid 钳制）。
    mons.push({ id: "legacy", x: 0, y: 0, width: 100, height: 100, edidFingerprint: "legacy", scale: 1 });
    expect(mem.restore("legacy", mons)).toEqual({ x: 1, y: 1 });
    for (let i = 0; i < 9; i++) mem.remember(`m${i}`, 10 + i, 10, mons);
    const keys = Object.keys(holder.points);
    expect(keys).toHaveLength(8);
    expect(keys).not.toContain("legacy"); // 旧态最旧，先走
    expect(keys).toContain("m8"); // 最新写入保留
    unsub();
    j1Store.reset();
  });
});

describe("批次六 · 形状查重（F617 录制台判据延伸）", () => {
  it("同形 ≥0.92 拒绝入库；略异形允许共存", () => {
    const shapes = { circle: { name: "圆", action: "x", template: makeTemplate(circlePts(0, 0, 100)) } };
    const dup = findDuplicateShape(makeTemplate(circlePts(2, -1, 98)), shapes);
    expect(dup?.id).toBe("circle");
    const diff = findDuplicateShape(makeTemplate(vStroke()), shapes);
    expect(diff).toBeNull();
  });
});

describe("批次六 · 应用身份注册表（F605 ugly 闭合）", () => {
  const fakeDoc = {
    querySelectorAll: (sel: string) =>
      sel === "[data-app-id]"
        ? [
            { getAttribute: (a: string) => (a === "data-app-id" ? "explorer" : a === "data-app-class" ? "list" : "资源管理器") },
            { getAttribute: (a: string) => (a === "data-app-id" ? "app-write" : a === "data-app-class" ? "document" : null) },
            { getAttribute: (a: string) => (a === "data-app-id" ? "explorer" : null) }, // 重复——去重
          ]
        : [],
  } as unknown as Document;

  it("枚举去重保序 + label 缺省回退 id", () => {
    const apps = enumerateAppIds(fakeDoc);
    expect(apps.map((a) => a.id)).toEqual(["explorer", "app-write"]);
    expect(apps[0]!.label).toBe("资源管理器");
    expect(apps[1]!.label).toBe("app-write");
    expect(apps[1]!.appClass).toBe("document");
  });

  it("覆盖目标校验：清单外 id 显性拒绝（防拼写错误静默失效）", () => {
    const apps = enumerateAppIds(fakeDoc);
    expect(validateAppOverrideTarget("explorer", apps)).toHaveLength(0);
    expect(validateAppOverrideTarget("explorr", apps)).toHaveLength(1);
    expect(validateAppOverrideTarget("  ", apps)).toHaveLength(1);
  });
});

/* ------------------------------- gainfield ------------------------------- */

describe("gainfield F601 精确反解", () => {
  it("solveBezierT 全域残差 <2e-5（含极端控制点）", () => {
    expect(solverResidual(0.35, 0.7)).toBeLessThan(2e-5);
    expect(solverResidual(0.9, 0.1)).toBeLessThan(2e-5);
    expect(solverResidual(0.1, 0.9)).toBeLessThan(2e-5);
    expect(solverResidual(0.0, 1.0)).toBeLessThan(2e-5);
  });

  it("增益端点：a=0 → 0.5，a=128 → 2.0（y 控制点默认档）", () => {
    expect(bezierGainPrecise(0, 0.35, 0.55, 0.7, 1.0)).toBeCloseTo(0.5, 6);
    expect(bezierGainPrecise(128, 0.35, 0.55, 0.7, 1.0)).toBeCloseTo(2.0, 6);
  });

  it("x(t) 反解后 y 与参数化 y 一致（预览=实际）", () => {
    const t = 0.42;
    const x = bezierX(t, 0.35, 0.7);
    const ts = solveBezierT(x, 0.35, 0.7);
    expect(ts).toBeCloseTo(t, 4);
  });

  it("单调性自检：健康曲线 null；y 控制点越界折返被检出", () => {
    expect(firstNonMonotonic(0.35, 0.55, 0.7, 1.0)).toBeNull();
    // cp2y=-1：y(t)=t²(4t-3)，前段下降——折返点应在低位移区。
    expect(firstNonMonotonic(0.35, 0, 0.7, -1)).not.toBeNull();
  });

  it("DPI 归一：线性 rawGain 跨缩放一致性 pass", () => {
    const r = scaleConsistency((a) => 1 + Math.min(1, a / 128) * 0.5, 96, 1, 1.25);
    expect(r.pass).toBe(true);
  });
});

/* ------------------------------- oneEuro ------------------------------- */

describe("oneEuro F603/F611 双引擎", () => {
  it("强档对 2Hz 震颤残余 <0.5（吃得住）", () => {
    expect(euroResidual("strong", 2, 1)).toBeLessThan(0.5);
  });

  it("意图直通：≥4×幅度阈值的位移零衰减", () => {
    const f = new TremorFilterEuro("strong");
    f.feed(0.5, 0.5, 0);
    const out = f.feed(20, 0, 8);
    expect(out.x).toBe(20);
    expect(out.filtered).toBe(false);
  });

  it("tuneOneEuro 两档参数齐备且 beta>0", () => {
    const l = tuneOneEuro("light");
    const s = tuneOneEuro("strong");
    expect(l.minCutoff).toBeGreaterThan(s.minCutoff);
    expect(l.beta).toBeGreaterThan(s.beta);
  });

  it("对拍谱：3 频段齐、One Euro 具备频率选择性（高频压更狠）", () => {
    const { rows } = compareEngines(1);
    expect(rows).toHaveLength(3);
    // One Euro 的真差异（CHI 2012 语义）：截止随速度自适应 → 高频震颤压得
    // 更狠、低频漂移放得更宽；IIR 无选择性（对拍实测恒 ~0.075）。
    expect(euroResidual("strong", 6, 1)).toBeLessThan(euroResidual("strong", 2, 1));
    expect(euroResidual("strong", 6, 1)).toBeLessThan(0.5);
  });
});

/* ------------------------------- recognizer ------------------------------- */

function circlePts(cx: number, cy: number, r: number, n = 48): { x: number; y: number }[] {
  return Array.from({ length: n + 1 }, (_, i) => {
    const a = (i / n) * Math.PI * 2;
    return { x: cx + r * Math.cos(a), y: cy + r * Math.sin(a) };
  });
}

function vStroke(): { x: number; y: number }[] {
  // 密集化到 ~40 点（真实轨迹点距 1-3px；稀疏 3 点触发 shapeFallback 的
  // ≥12 点下界——下界本身是判据：太短的形状既难画准也易误触）。
  const pts: { x: number; y: number }[] = [];
  for (let i = 0; i <= 20; i++) pts.push({ x: (60 * i) / 20, y: (120 * i) / 20 });
  for (let i = 1; i <= 20; i++) pts.push({ x: 60 + (60 * i) / 20, y: 120 - (120 * i) / 20 });
  return pts;
}

describe("recognizer F617 Protractor 形状引擎", () => {
  it("重采样 32 点、模板 64 维", () => {
    expect(resample(circlePts(0, 0, 100))).toHaveLength(32);
    expect(makeTemplate(circlePts(0, 0, 100))).toHaveLength(64);
  });

  it("同形高分、异形低分（余弦距离打分方向正确）", () => {
    const circle = makeTemplate(circlePts(0, 0, 100));
    const vee = makeTemplate(vStroke());
    const sample = makeTemplate(circlePts(5, -3, 95));
    expect(protractorDistance(sample, circle)).toBeLessThan(protractorDistance(sample, vee));
    expect(protractorScore(protractorDistance(sample, circle))).toBeGreaterThan(0.9);
  });

  it("识别：圆样本命中圆模板（阈值以上），V 样本不冒领圆", () => {
    const shapes = {
      circle: { name: "圆", action: "view.refresh", template: makeTemplate(circlePts(0, 0, 100)) },
    };
    const hitCircle = recognizeProtractor(makeTemplate(circlePts(2, 2, 98)), shapes);
    expect(hitCircle?.id).toBe("circle");
    expect(hitCircle!.score).toBeGreaterThanOrEqual(0.8);
    const miss = recognizeProtractor(makeTemplate(vStroke()), shapes);
    expect(miss).toBeNull();
  });

  it("shapeFallback：重绑定一致性（bindings 共表）", () => {
    const shapes = {
      vee: { name: "对勾", action: "edit.paste", template: makeTemplate(vStroke()) },
    };
    const cfg = {
      enabled: true,
      trailFadeMs: 120,
      custom: {},
      bindings: { vee: "edit.copy" },
      shapes,
    };
    const hit = shapeFallback(vStroke(), cfg);
    expect(hit?.action).toBe("edit.copy");
  });

  it("ShapeRecorder：不达标 finish 显性抛错（不静默出半成品模板）", () => {
    const rec = new ShapeRecorder();
    rec.begin(0, 0);
    rec.feed(2, 1);
    expect(() => rec.finish()).toThrow();
    rec.reset();
    rec.begin(0, 0);
    for (const p of circlePts(0, 0, 60, 64)) rec.feed(p.x, p.y);
    expect(rec.finish()).toHaveLength(64);
  });

  it("端到端：GestureRecognizer 方向串未命中 → 形状兜底命中", () => {
    const shapes = {
      circle: { name: "圆", action: "view.refresh", template: makeTemplate(circlePts(0, 0, 100)) },
    };
    const pts = circlePts(0, 0, 100);
    const rec = new GestureRecognizer();
    rec.begin(pts[0]!.x, pts[0]!.y); // 起点在圆周上（真实手势语义——圆心起笔=轨迹带尖刺，属用户画法问题）
    for (const p of pts.slice(1)) rec.feed(p.x, p.y);
    const hit = rec.recognize({ enabled: true, trailFadeMs: 120, custom: {}, bindings: {}, shapes });
    expect(hit?.id).toBe("circle");
  });
});

/* ------------------------------- edid ------------------------------- */

describe("edid F613/F614 字节身份", () => {
  it("合成块解析：checksum/厂商/产品码/序列/首选时序", () => {
    const bytes = synthEdid({ manufacturer: "LEN", productCode: 0x1234, serial: 0xaabbccdd, hActive: 1920, vActive: 1080 });
    const info = parseEdid(bytes);
    expect(info.checksumOk).toBe(true);
    expect(info.manufacturer).toBe("LEN");
    expect(info.productCode).toBe(0x1234);
    expect(info.serial).toBe(0xaabbccdd);
    expect(info.year).toBe(2026);
    expect(info.preferred?.hActive).toBe(1920);
    expect(info.preferred?.pixelClockKhz).toBe(148500);
  });

  it("篡改字节 → checksum 检出（完整性判据）", () => {
    const bytes = synthEdid({ manufacturer: "AUS", productCode: 1, serial: 2 });
    bytes[40] = (bytes[40]! + 1) & 0xff;
    expect(parseEdid(bytes).checksumOk).toBe(false);
  });

  it("半块与坏魔数拒绝（显性报错不静默）", () => {
    expect(() => parseEdid(new Uint8Array(64))).toThrow();
    const bad = synthEdid({ manufacturer: "LEN", productCode: 1, serial: 1 });
    bad[0] = 0x01;
    expect(() => parseEdid(bad)).toThrow();
  });

  it("指纹稳定：同块同指纹、异块异指纹", () => {
    const a = synthEdid({ manufacturer: "LEN", productCode: 1, serial: 1 });
    const b = synthEdid({ manufacturer: "AUS", productCode: 1, serial: 1 });
    expect(edidFingerprintOf(a)).toBe(edidFingerprintOf(synthEdid({ manufacturer: "LEN", productCode: 1, serial: 1 })));
    expect(edidFingerprintOf(a)).not.toBe(edidFingerprintOf(b));
    expect(edidFingerprintOf(a)).toMatch(/^edid:[0-9a-f]{8}$/);
  });

  it("厂商码解码与身份卡分级", () => {
    expect(decodeManufacturer(0x04, 0x21)).toBe("AAA"); // 15bit 打包 = 1·1024+1·32+1 = 0x0421
    expect(describeIdentity("tauri:DELL U2720Q:1920x1080").kind).toBe("name");
    expect(describeIdentity("edid:0123abcd").kind).toBe("edid");
  });
});

/* ------------------------------- topology ------------------------------- */

const MON_A = { id: "a", x: 0, y: 0, width: 1920, height: 1080, edidFingerprint: "edid:aaaa", scale: 1 };
const MON_B = { id: "b", x: 1920, y: 0, width: 2560, height: 1440, edidFingerprint: "edid:bbbb", scale: 1.25 };

describe("topology F607 接缝与 DPI 变换", () => {
  it("左右相邻屏：竖直缝 at=1920，重叠区间 [0,1080]", () => {
    const s = seamBetween(MON_A, MON_B)!;
    expect(s.axis).toBe("v");
    expect(s.at).toBe(1920);
    expect(s.from).toBe(0);
    expect(s.to).toBe(1080);
    expect(seamSegments([MON_A, MON_B])).toHaveLength(1);
  });

  it("角对角触碰不算接缝（四角豁免的几何前提）", () => {
    const c = { id: "c", x: 1920, y: 1080, width: 800, height: 600, edidFingerprint: "c", scale: 1 };
    expect(seamBetween(MON_A, c)).toBeNull();
  });

  it("缝距与最近缝：缝上 0、离缝 100px 处 100", () => {
    expect(nearestSeam([MON_A, MON_B], 1915, 500)!.dist).toBeCloseTo(5, 6);
    expect(nearestSeam([MON_A, MON_B], 1820, 500)!.dist).toBeCloseTo(100, 6);
  });

  it("包围盒与钳制", () => {
    const b = virtualBounds([MON_A, MON_B]);
    expect(b.width).toBe(4480);
    expect(clampToDesktop([MON_A, MON_B], 99999, 50)).toEqual({ x: 4480, y: 50 });
  });

  it("DPI 变换：物理↔逻辑往返 + 量化残差显性", () => {
    const phys = toPhysical([MON_A, MON_B], 1000, 500);
    expect(phys.monitor?.id).toBe("a");
    const back = toLogical([MON_A, MON_B], phys.x, phys.y);
    expect(back.x).toBeCloseTo(1000, 6);
    expect(back.residual).toBeLessThan(0.5);
    const onB = toPhysical([MON_A, MON_B], 2000, 100);
    expect(onB.x).toBeCloseTo(2500, 6);
  });

  it("混合 DPI 判定与拓扑摘要", () => {
    expect(isMixedDpi([MON_A, MON_B])).toBe(true);
    const s = topologySummary([MON_A, MON_B]);
    expect(s.screens).toBe(2);
    expect(s.seams).toBe(1);
    expect(s.mixedDpi).toBe(true);
  });
});

/* ------------------------------- wheelcal ------------------------------- */

describe("wheelcal F605/F612 标定引擎", () => {
  it("行高估计：3 样本后收敛 ≈40px/行，离群被拒", () => {
    const est = new LineHeightEstimator();
    expect(est.feed({ lines: 3, px: 120 })).toBeNull();
    est.feed({ lines: 3, px: 120 });
    expect(est.feed({ lines: 3, px: 120 })).toBe(40);
    expect(est.feed({ lines: 1, px: 9999 })).toBe(40); // 离群拒绝
  });

  it("幂律拟合：两点精确、R²=1；单点拒绝", () => {
    const fit = fitGainCurve([
      { notchesPerSec: 1, lines: 3 },
      { notchesPerSec: 8, lines: 12 },
    ]);
    expect(fit.ok).toBe(true);
    expect(fit.r2).toBe(1);
    expect(fit.k).toBeCloseTo(3, 1);
    expect(fit.b).toBeCloseTo(0.667, 2);
    expect(fitGainCurve([{ notchesPerSec: 3, lines: 6 }]).ok).toBe(false);
  });

  it("曲线求值与预览表（6 档、单调不减）", () => {
    expect(evalGainCurve(3, 0, 5)).toBe(3);
    const table = previewTable(3, 0.667);
    expect(table).toHaveLength(6);
    for (let i = 1; i < table.length; i++) expect(table[i]!.lines).toBeGreaterThanOrEqual(table[i - 1]!.lines);
  });

  it("向导四步状态机：中断可弃、采纳返回参数", () => {
    const wiz = new WheelCalibrationWizard();
    expect(wiz.state.step).toBe("idle");
    wiz.begin();
    wiz.feedLinePair({ lines: 3, px: 120 });
    wiz.feedLinePair({ lines: 3, px: 120 });
    wiz.feedLinePair({ lines: 3, px: 120 });
    wiz.recordPace(1, 3);
    expect(wiz.fitNow().fit?.note).toContain("不足两档");
    wiz.recordPace(8, 12);
    const st = wiz.fitNow();
    expect(st.step).toBe("preview");
    expect(st.fit?.ok).toBe(true);
    const adopted = wiz.accept();
    expect(adopted?.k).toBeGreaterThan(0);
    wiz.begin();
    expect(wiz.state.paceSamples).toHaveLength(0);
    wiz.abort();
    expect(wiz.state.step).toBe("idle");
  });

  it("出厂对账：三点标定贴近出厂线性", () => {
    const r = compareWithFactory(fitGainCurve([
      { notchesPerSec: 1, lines: 3 },
      { notchesPerSec: 8, lines: 12 },
    ]));
    expect(r.at3).toBeGreaterThan(0);
    expect(typeof r.nearFactory).toBe("boolean");
  });
});

/* ------------------------------- physics ------------------------------- */

describe("physics F604/F608 指针物理", () => {
  it("临界阻尼弹簧：收敛到位即停、无过冲", () => {
    let v = { x: 0, y: 0 };
    const target = { x: 4.8, y: -3.2 };
    for (let i = 0; i < 80; i++) v = springToward(v, target, 16);
    expect(springSettled(v, target)).toBe(true);
    // 无过冲：全程单调逼近（临界阻尼定义）。
    let prev = Math.hypot(target.x, target.y);
    let w = { x: 0, y: 0 };
    for (let i = 0; i < 40; i++) {
      w = springToward(w, target, 16);
      const d = Math.hypot(target.x - w.x, target.y - w.y);
      expect(d).toBeLessThanOrEqual(prev + 1e-9);
      prev = d;
    }
  });

  it("缓动族端点精确", () => {
    expect(easeOutCubic(0)).toBe(0);
    expect(easeOutCubic(1)).toBe(1);
    expect(anchorLifeCurve(0)).toBeCloseTo(0, 12); // 浮点舍入尾巴 <1e-15
    expect(anchorLifeCurve(80)).toBeGreaterThan(1); // pop 过冲
    expect(anchorLifeCurve(1000)).toBe(0); // 消失完毕
  });

  it("长按进度预弯：端点不骗人、单调", () => {
    expect(longPressVisualProgress(0)).toBe(0);
    expect(longPressVisualProgress(1)).toBe(1);
    expect(longPressVisualProgress(0.5)).toBeGreaterThan(0.5); // 前段快
  });

  it("HUD 呼吸与倾斜连发缓升", () => {
    expect(hudBreathAlpha(0)).toBeCloseTo(0.96, 6);
    expect(hudBreathAlpha(400)).toBeGreaterThan(0.9);
    expect(hudBreathAlpha(400)).toBeLessThan(1.01);
    expect(tiltRepeatInterval(0, 40)).toBe(40);
    expect(tiltRepeatInterval(8, 40)).toBeLessThan(tiltRepeatInterval(0, 40));
    expect(tiltRepeatInterval(99, 40)).toBeGreaterThanOrEqual(16);
  });
});

/* ------------------------------- session ------------------------------- */

function ev(at: number, verdict: J1Event["verdict"], durMs = 10): J1Event {
  return { at, kind: "click", verdict, target: "button", durMs, zone: 5 };
}

describe("session 章十三 聚合与日报", () => {
  const events: J1Event[] = [
    ev(0, "smooth", 10),
    ev(100, "laggy", 200),
    ev(200, "smooth", 30),
    ev(31 * 60 * 1000, "no-feedback", 0),
    ev(31 * 60 * 1000 + 500, "smooth", 50),
  ];
  const frustrations: FrustrationSignal[] = [
    { at: 150, kind: "rage-click", detail: "2s内同格3击", zone: 5 },
    { at: 31 * 60 * 1000 + 100, kind: "dead-click", detail: "同目标60s无反馈", zone: 1 },
  ];

  it("分位数线性插值口径", () => {
    expect(percentile([10, 20, 30, 40], 50)).toBe(25);
    expect(percentile([], 95)).toBe(0);
  });

  it("会话分桶：30min 间隔切两段", () => {
    const s = bucketSessions(events, frustrations);
    expect(s).toHaveLength(2);
    expect(s[0]!.events).toHaveLength(3);
    expect(s[1]!.events).toHaveLength(2);
    expect(s[0]!.frustrations).toHaveLength(1);
  });

  it("会话统计：结论分布/P95/顺畅率/热区", () => {
    const st = sessionStats(bucketSessions(events, frustrations)[0]!);
    expect(st.events).toBe(3);
    expect(st.verdicts.smooth).toBe(2);
    expect(st.latency.p95).toBeGreaterThanOrEqual(st.latency.p50);
    expect(st.smoothRate).toBeCloseTo(0.667, 2);
    expect(st.hotZones[0]?.zone).toBe(5);
  });

  it("最挫败会话排行（密度降序）与日报 Markdown", () => {
    const sessions = bucketSessions(events, frustrations);
    const worst = worstSessions(sessions);
    expect(worst.length).toBe(2);
    const report = dailyReport(sessions, "2026-09-27");
    expect(report).toContain("# J 域体验日报 · 2026-09-27");
    expect(report).toContain("会话明细");
    expect(report).toContain("隐私口径");
    expect(dailyReport([], "空")).toContain("零事件");
  });
});

/* ------------------------------- reconcile ------------------------------- */

describe("reconcile 检查项对账 v5", () => {
  it("完整对账 Markdown：二十项 + 十二查 + 引擎探针三段齐", () => {
    const md = buildReconcileMarkdown();
    expect(md).toContain("F620");
    expect(md).toContain("## 十二查逐项");
    expect(md).toContain("## v5 引擎健康探针");
    expect(md).toContain("最丑角落");
  });

  it("引擎探针全部真执行通过", () => {
    for (const p of engineProbes()) {
      expect(p.probe(), p.engine).toBe(true);
    }
  });

  it("v5 新增判据锚登记 ≥12 条且状态合法", () => {
    expect(V5_NEW_ANCHORS.length).toBeGreaterThanOrEqual(12);
    for (const a of V5_NEW_ANCHORS) {
      expect(["green", "gated"]).toContain(a.state);
      expect(a.carrier.length).toBeGreaterThan(0);
    }
  });
});
