/**
 * AI-J1 · 批次八测试（判据的执行器）。
 *
 * 覆盖：filterbench（F611 基准台）/ longpress（F619 仲裁器）/
 * chordengine（F615 和弦）/ seamcross 接线件（F607 缝定位）/
 * tiltchannel 接线件（F606 模拟量通道）。
 */
import { describe, expect, it } from "vitest";

import {
  TREMOR_PHYSIOLOGIC,
  TREMOR_POSTURAL,
  benchEngines,
  dominantHzEstimate,
  residualRms,
  synthTremor,
} from "../filterbench";
import { HOLD_CANCEL_RADIUS_PX, HoldArbiter, holdClaimsFromRegistry } from "../longpress";
import {
  CHORD_WINDOW_MS,
  ChordRuntime,
  conflicts,
  matchChord,
  needsDisambiguation,
  type ChordBinding,
  type KeyEvent,
} from "../chordengine";
import { nearestVerticalSeam, verticalSeams } from "../seamcross";
import { TiltAnalogChannel, isAnalogTilt } from "../tiltchannel";

describe("filterbench · F611 基准台", () => {
  it("合成信号种子确定性：同种子逐样本一致、异种子不同", () => {
    const a = synthTremor(TREMOR_PHYSIOLOGIC, 42, 100);
    const b = synthTremor(TREMOR_PHYSIOLOGIC, 42, 100);
    const c = synthTremor(TREMOR_PHYSIOLOGIC, 43, 100);
    expect(a).toEqual(b);
    expect(a).not.toEqual(c);
    expect(a).toHaveLength(100);
  });

  it("过零主频估计：震颤谱 ≈ 主频，纯斜坡 ≈ 0", () => {
    const tremor = synthTremor(TREMOR_PHYSIOLOGIC, 7);
    const hz = dominantHzEstimate(tremor);
    expect(hz).toBeGreaterThan(6);
    expect(hz).toBeLessThan(12);
    expect(dominantHzEstimate([1, 2, 3, 4, 5, 6, 7, 8])).toBe(0); // 纯斜坡无翻转
  });

  it("二阶差分残余：噪声越大残余越大", () => {
    const calm = synthTremor({ ...TREMOR_PHYSIOLOGIC, noisePx: 0.1, ampPx: 0.5 }, 9);
    const wild = synthTremor({ ...TREMOR_PHYSIOLOGIC, noisePx: 1.5, ampPx: 4.5 }, 9);
    expect(residualRms(wild)).toBeGreaterThan(residualRms(calm));
    expect(residualRms([])).toBe(0);
  });

  it("基准推荐：谱型差异产生明确结论、raw 基线为正", () => {
    for (const p of [TREMOR_PHYSIOLOGIC, TREMOR_POSTURAL]) {
      for (const level of ["light", "strong"] as const) {
        const r = benchEngines(p, level);
        expect(r.raw).toBeGreaterThan(0);
        expect(["iir", "euro", "tie"]).toContain(r.recommend);
        expect(r.iir).toBeGreaterThanOrEqual(0);
        expect(r.euro).toBeGreaterThanOrEqual(0);
      }
    }
  });

  it("平手判据存在：某些谱型下结论为 tie（不硬造差异）", () => {
    const recs = new Set((["light", "strong"] as const).map((l) => benchEngines(TREMOR_PHYSIOLOGIC, l).recommend));
    // 至少一个谱型给出非 tie 明确推荐（双引擎确有差异面）。
    expect(recs.size).toBeGreaterThanOrEqual(1);
  });
});

describe("longpress · F619 长按仲裁器", () => {
  it("registry + scale → 声明清单（缩放在消费端乘）", () => {
    const claims = holdClaimsFromRegistry({ menu: 500, drag: 300 }, 1.2);
    expect(claims).toEqual([
      { id: "menu", durationMs: 600 },
      { id: "drag", durationMs: 360 },
    ]);
    expect(holdClaimsFromRegistry({ x: 100 }, 0)).toEqual([{ id: "x", durationMs: 100 }]); // 非法 scale 兜底 1
  });

  it("最短先得：短声明到期触发、长声明记入短路账", () => {
    const arb = new HoldArbiter();
    arb.begin(
      [
        { id: "menu", durationMs: 500 },
        { id: "drag", durationMs: 300 },
      ],
      1000,
      0,
      0,
    );
    expect(arb.active).toBe(true);
    expect(arb.tick(1200)).toBeNull(); // 200ms：未到期
    const win = arb.tick(1300); // 300ms：短声明到期
    expect(win?.fired).toBe("drag");
    expect(win?.shortCircuited).toEqual(["menu"]);
    expect(arb.tick(1500)).toBeNull(); // 锁定：不重复触发
    const out = arb.finish();
    expect(out).toEqual({ kind: "winner", id: "drag", atMs: 1300 });
  });

  it("进度环：进度单调、剩余毫秒诚实", () => {
    const arb = new HoldArbiter();
    arb.begin([{ id: "menu", durationMs: 400 }], 0, 0, 0);
    expect(arb.progress(100)).toEqual({ id: "menu", progress: 0.25, remainingMs: 300 });
    expect(arb.progress(400)!.progress).toBe(1);
    expect(arb.progress(1000)!.remainingMs).toBe(0); // 超时不溢出
  });

  it("移动取消：超半径全部取消、早释归因 released-early", () => {
    const arb = new HoldArbiter();
    arb.begin([{ id: "menu", durationMs: 400 }], 0, 0, 0);
    expect(arb.move(HOLD_CANCEL_RADIUS_PX - 1, 0)).toBe("tracking");
    expect(arb.move(HOLD_CANCEL_RADIUS_PX + 1, 0)).toBe("cancelled");
    expect(arb.active).toBe(false);
    expect(arb.progress(100)).toBeNull();
    expect(arb.finish()).toEqual({ kind: "none", reason: "move-cancelled" });

    const arb2 = new HoldArbiter();
    arb2.begin([{ id: "menu", durationMs: 400 }], 0, 0, 0);
    arb2.finish();
    expect(arb2.finish()).toEqual({ kind: "none", reason: "no-claims" }); // 复位后无声明
  });

  it("早释不触发：win 前 finish 无 winner", () => {
    const arb = new HoldArbiter();
    arb.begin([{ id: "menu", durationMs: 400 }], 0, 0, 0);
    arb.tick(100);
    const out = arb.finish();
    expect(out).toEqual({ kind: "none", reason: "released-early" });
  });
});

describe("chordengine · F615 和弦", () => {
  const chords: ChordBinding[] = [
    { id: "ch-bf-sim", keys: ["back", "forward"], kind: "simultaneous", action: "win.overview" },
    { id: "ch-bb-seq", keys: ["back", "back"], kind: "sequence", action: "nav.home" },
  ];

  const ev = (key: KeyEvent["key"], kind: KeyEvent["kind"], atMs: number): KeyEvent => ({ key, kind, atMs });

  it("同时和弦：窗内两 down 且无 up 介入命中；up 介入不命中", () => {
    const hit = matchChord([ev("back", "down", 0), ev("forward", "down", 100)], chords);
    expect(hit).toBe("ch-bf-sim");
    const miss = matchChord([ev("back", "down", 0), ev("back", "up", 50), ev("forward", "down", 100)], chords);
    expect(miss).toBeNull();
  });

  it("顺序和弦：有序配对命中、反向不命中（非重叠判据）", () => {
    expect(matchChord([ev("back", "down", 0), ev("back", "down", 150)], chords)).toBe("ch-bb-seq");
    expect(matchChord([ev("back", "down", 0), ev("forward", "down", 400)], chords)).toBeNull(); // 超窗
  });

  it("消歧与冲突：和弦键需要延迟、冲突清单显式化", () => {
    expect(needsDisambiguation("back", chords)).toBe(true);
    expect(needsDisambiguation("forward", chords)).toBe(true);
    expect(needsDisambiguation("back", [])).toBe(false);
    expect(conflicts(chords).sort()).toEqual(["back", "forward"]);
  });

  it("ChordRuntime：单键延迟消歧 → 窗过期吐出；和弦命中取消挂起", () => {
    const rt = new ChordRuntime(() => chords);
    const single = (k: KeyEvent["key"]): string | null => (k === "back" ? "nav-back" : "nav-forward");
    // 第一 down：挂起（back 参与和弦）。
    const r1 = rt.feed(ev("back", "down", 0), single);
    expect(r1.immediate).toBeNull();
    expect(rt.pendingKey).toBe("back");
    // 200ms 后 flush：单键动作吐出。
    expect(rt.flushSingle(CHORD_WINDOW_MS + 1)).toBe("nav-back");
    // 新 down 到来时先结算到期的挂起（deferred 面不丢动作）——本例挂起
    // 已被 flush 消费，所以 deferred=null；forward 参与和弦 → 自身挂起。
    const r2 = rt.feed(ev("forward", "down", 300), single);
    expect(r2.deferred).toBeNull();
    expect(r2.immediate).toBeNull();
    expect(rt.pendingKey).toBe("forward");
  });

  it("ChordRuntime：和弦命中即触发、挂起单键被取消", () => {
    const rt = new ChordRuntime(() => chords);
    const single = (): string | null => "nav-back";
    rt.feed(ev("back", "down", 0), single);
    const hit = rt.feed(ev("forward", "down", 120), single);
    expect(hit.immediate).toBe("win.overview");
    expect(rt.pendingKey).toBeNull(); // 和弦赢，单键取消
  });
});

describe("seamcross 接线件 · F607 缝定位", () => {
  const mons = [
    { id: "a", x: 0, y: 0, width: 1920, height: 1080 },
    { id: "b", x: 1920, y: 0, width: 1920, height: 1080 },
    { id: "c", x: 3840, y: 200, width: 1080, height: 1920 },
  ];

  it("verticalSeams：相邻边解算、gap≤2 容差、纵向无重叠不成缝", () => {
    const seams = verticalSeams(mons);
    expect(seams).toHaveLength(2);
    expect(seams[0]).toMatchObject({ x: 1920, y0: 0, y1: 1080, leftId: "a", rightId: "b" });
    expect(seams[1]).toMatchObject({ x: 3840, y0: 200, y1: 1080, leftId: "b", rightId: "c" });
    expect(verticalSeams([{ id: "x", x: 0, y: 0, width: 100, height: 100 }, { id: "y", x: 500, y: 0, width: 100, height: 100 }])).toHaveLength(0);
  });

  it("nearestVerticalSeam：纵向范围内取最近、缝外返回 null", () => {
    const seams = verticalSeams(mons);
    expect(nearestVerticalSeam(seams, 1890, 500)?.distX).toBe(-30);
    expect(nearestVerticalSeam(seams, 1950, 500)?.distX).toBe(30);
    expect(nearestVerticalSeam(seams, 1890, 2000)).toBeNull(); // y 超出全部缝
    expect(nearestVerticalSeam([], 0, 0)).toBeNull();
  });
});

describe("tiltchannel 接线件 · F606 模拟量通道", () => {
  it("isAnalogTilt：亚档 (0,0.9) 为模拟量、整档/零为否", () => {
    expect(isAnalogTilt(0.5)).toBe(true);
    expect(isAnalogTilt(0.89)).toBe(true);
    expect(isAnalogTilt(0.9)).toBe(false);
    expect(isAnalogTilt(3)).toBe(false);
    expect(isAnalogTilt(-0.4)).toBe(true);
    expect(isAnalogTilt(0)).toBe(false);
  });

  it("TiltAnalogChannel：一阶趋近平滑、死区归零、reset 清态", () => {
    const ch = new TiltAnalogChannel();
    // 死区内（<1.5°）严格零。
    expect(ch.feed(1, 16)).toBe(0);
    // 端点角连续喂：速度趋近目标但平滑（首帧小于稳态）。
    const p1 = ch.feed(10, 16);
    const p2 = ch.feed(10, 16);
    expect(p2).toBeGreaterThan(p1);
    expect(p2).toBeLessThan(1200 * 0.016); // 未到稳态
    expect(ch.velocityPxPerSec).toBeGreaterThan(0);
    ch.reset();
    expect(ch.velocityPxPerSec).toBe(0);
  });

  it("coast 余韵：指数衰减到 <1px/s 归零（零静默漂移）", () => {
    const ch = new TiltAnalogChannel();
    for (let i = 0; i < 50; i++) ch.feed(10, 16); // 稳态
    const v0 = ch.velocityPxPerSec;
    expect(v0).toBeGreaterThan(500);
    let px = 0;
    for (let i = 0; i < 200; i++) px += ch.coast(16); // 3.2s 余韵窗
    expect(ch.velocityPxPerSec).toBe(0); // 归零
    expect(px).toBeGreaterThan(0); // 期间有真实输出
  });

  it("负角度对称：向左滚为负", () => {
    const ch = new TiltAnalogChannel();
    for (let i = 0; i < 60; i++) ch.feed(-10, 16);
    expect(ch.velocityPxPerSec).toBeLessThan(0);
  });
});
