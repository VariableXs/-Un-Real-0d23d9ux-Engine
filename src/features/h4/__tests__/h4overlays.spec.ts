/// <reference types="node" />
/**
 * H4 真实浮层逻辑层单测（v3）：
 * 取样引擎（颜色解析/祖先回退链）、拾色会话、标尺宿主面、
 * 速查键位适配（N-17 真源消费）、长按 Win 状态机、专注芯片。
 */

import { describe, expect, it } from "vitest";
import * as f359 from "../../../system/h4/f359-colorPicker";
import {
  entriesFromKeymap,
  focusChipPlacement,
  focusChipTick,
  parseColorString,
  pickerHostEscape,
  pickerHostPick,
  pickerHostStart,
  rulerGridPlan,
  rulerHostEscape,
  rulerHostMove,
  rulerHostReadout,
  rulerHostStart,
  sampleColorChain,
  winHoldDown,
  winHoldTick,
  winHoldUp,
} from "../overlays";

/* ---------------- 取样引擎 ---------------- */

describe("parseColorString（rgb/rgba/hex 三格式 + 非法显式 null）", () => {
  it("全格式解析与钳制", () => {
    expect(parseColorString("rgb(1, 2, 3)")).toEqual({ r: 1, g: 2, b: 3 });
    expect(parseColorString("rgba(10, 20, 30, 0.5)")).toEqual({ r: 10, g: 20, b: 30 });
    expect(parseColorString("#4F7CFF")).toEqual({ r: 79, g: 124, b: 255 });
    expect(parseColorString("  #ff8000 ")).toEqual({ r: 255, g: 128, b: 0 });
    expect(parseColorString("300, -5, 7")).toBeNull();
    expect(parseColorString("transparent")).toBeNull();
    expect(parseColorString("")).toBeNull();
  });
});

describe("sampleColorChain（祖先回退链——判据「取色准确性」落地面）", () => {
  const make = (styles: Record<string, string>): { el: Element; parent: Element | null } => {
    // 轻量桩：只需满足 sampleColorChain 对 Element 的最小用法（parentElement 链）
    const fake = { parentElement: null } as unknown as Element;
    (fake as unknown as Record<string, unknown>).__styles = styles;
    return { el: fake, parent: null };
  };

  it("目标 backgroundColor 命中即返回（链路留痕）", () => {
    const el = make({ "background-color": "rgb(1, 2, 3)" });
    const r = sampleColorChain(el.el, (_node, prop) => (prop === "background-color" ? "rgb(1, 2, 3)" : "none"));
    expect(r.color).toEqual({ r: 1, g: 2, b: 3 });
    expect(r.chain[0]!.parsed).toEqual({ r: 1, g: 2, b: 3 });
  });

  it("背景透明 → 回退 color → 再回退祖先背景（跳过 transparent）", () => {
    const parent = make({ "background-color": "#102030" });
    const child = make({ "background-color": "transparent", color: "rgb(9, 8, 7)" });
    (child.el as unknown as { parentElement: Element }).parentElement = parent.el;
    const read = (node: Element, prop: string): string => {
      const styles = (node as unknown as { __styles: Record<string, string> }).__styles;
      return styles[prop] ?? "";
    };
    const r = sampleColorChain(child.el, read);
    expect(r.color).toEqual({ r: 9, g: 8, b: 7 }); // 第一跳 color 命中

    // 全透明子层 → 祖先背景命中（跳过 transparent）
    const child2 = make({ "background-color": "transparent", color: "transparent" });
    (child2.el as unknown as { parentElement: Element }).parentElement = parent.el;
    const r2 = sampleColorChain(child2.el, read);
    expect(r2.color).toEqual({ r: 0x10, g: 0x20, b: 0x30 }); // 祖先回退命中
  });

  it("全链透明 = null（诚实：取不到不编色）", () => {
    const el = make({ "background-color": "transparent", color: "transparent" });
    const r = sampleColorChain(el.el, (_node, prop) => (prop === "background-color" ? "transparent" : "transparent"));
    expect(r.color).toBeNull();
    expect(r.chain.length).toBeGreaterThan(0);
  });
});

/* ---------------- 拾色会话 ---------------- */

describe("pickerHost 会话（f359.pickOnce 宿主面）", () => {
  it("普通取样：取一色即退（active=false）；Shift 连续：保持 active", () => {
    const started = pickerHostStart({ active: true, picks: 0, recent: [], last: null });
    const one = pickerHostPick(started, { r: 1, g: 2, b: 3 }, false);
    expect(one.active).toBe(false);
    expect(one.picks).toBe(1);
    expect(one.last).toEqual({ r: 1, g: 2, b: 3 });
    expect(one.recent).toHaveLength(1);
    const cont = pickerHostPick(pickerHostStart(started), { r: 5, g: 5, b: 5 }, true);
    expect(cont.active).toBe(true);
  });

  it("最近色去重提队首且上限 8（F234 同源）", () => {
    let s = pickerHostStart({ active: true, picks: 0, recent: [], last: null });
    for (let i = 0; i < 10; i++) s = pickerHostPick(s, { r: i, g: i, b: i }, true);
    expect(s.recent).toHaveLength(f359.RECENT_COLORS_CAP);
    s = pickerHostPick(s, { r: 9, g: 9, b: 9 }, true); // 已在队内的重复色
    expect(s.recent[0]).toEqual({ r: 9, g: 9, b: 9 });
    expect(s.recent).toHaveLength(f359.RECENT_COLORS_CAP);
  });

  it("取不到色不编色不计数；Esc 显式退", () => {
    const s = pickerHostPick(pickerHostStart({ active: true, picks: 2, recent: [], last: null }), null, false);
    expect(s.picks).toBe(2);
    expect(s.active).toBe(false);
    expect(pickerHostEscape(s).active).toBe(false);
  });
});

/* ---------------- 标尺宿主面 ---------------- */

describe("rulerHost（f360 宿主面）", () => {
  it("起点→移动→读数（dx/dy + 归一化矩形）", () => {
    let s = rulerHostStart({ active: false, origin: null, current: null }, { x: 100, y: 200 });
    s = rulerHostMove(s, { x: 340, y: 120 });
    const r = rulerHostReadout(s)!;
    expect(r.dx).toBe(240);
    expect(r.dy).toBe(80);
    expect(r.rect).toEqual({ x: 100, y: 120, w: 240, h: 80 });
  });

  it("无起点时移动不产生读数；Esc 清场（零残留）", () => {
    const s = rulerHostMove({ active: true, origin: null, current: null }, { x: 5, y: 5 });
    expect(rulerHostReadout(s)).toBeNull();
    expect(rulerHostEscape(s)).toEqual({ active: false, origin: null, current: null });
  });

  it("网格计划走 f360.gridPaintSpec 一处定义（8px / 0.2）", () => {
    const p = rulerGridPlan(64, 40);
    expect(p.step).toBe(8);
    expect(p.opacity).toBe(0.2);
    expect(p.lines).toBe(8 + 5);
  });
});

/* ---------------- 速查键位适配 ---------------- */

describe("entriesFromKeymap（N-17/F244 真源 → F374 视图）", () => {
  it("分组映射与 combo 规范化", () => {
    const rows = [
      { action: "a", labelKey: "显示桌面", accel: "win+d", group: "window" },
      { action: "b", labelKey: "灰度模式", accel: "F387", group: "system" },
      { action: "c", labelKey: "打开应用", accel: "ctrl+1", group: "launch" },
      { action: "d", labelKey: "保存", accel: "ctrl+s", group: "panel" },
    ];
    const entries = entriesFromKeymap(rows);
    expect(entries.map((e) => e.combo)).toEqual(["WIN+D", "F387", "CTRL+1", "CTRL+S"]);
    expect(entries.map((e) => e.group)).toEqual(["window", "system", "system", "appGeneric"]);
  });
});

describe("长按 Win 会话（f374 状态机宿主面）", () => {
  it("600ms 边界：599 不可见 / 600+ 可见；松开即隐", () => {
    let host = winHoldDown(0);
    host = winHoldTick(host, 599);
    expect(host.sheet.visible).toBe(false);
    host = winHoldTick(host, 600);
    expect(host.sheet.visible).toBe(true);
    host = winHoldUp(host);
    expect(host.sheet.visible).toBe(false);
    expect(host.holdTimer).toBeNull();
  });
});

/* ---------------- 专注芯片 ---------------- */

describe("focusChipTick（f363 宿主面）", () => {
  const mkRun = (minutes: number, startedAt: number) => ({ day: "2026-09-26", plannedMinutes: minutes, startedAt, endedAt: null as number | null, outcome: "running" as const });

  it("运行中出徽标；到点记一次账（防重）并出 due", () => {
    const started = Date.now();
    const st = { run: mkRun(25, started), recorded: false };
    const mid = focusChipTick(st, started + 60_000);
    expect(mid.badge).toBe("24:00");
    expect(mid.due).toBe(false);
    const due = focusChipTick(st, started + 25 * 60_000 + 1);
    expect(due.due).toBe(true);
    expect(due.badge).toBeNull();
    expect(due.state.recorded).toBe(true); // 已入账
    const again = focusChipTick(due.state, started + 26 * 60_000);
    expect(again.due).toBe(false); // 完成态不再重复提醒
  });

  it("空状态零动作；摆放避开任务栏", () => {
    expect(focusChipTick({ run: null, recorded: false }, Date.now()).badge).toBeNull();
    expect(focusChipPlacement(1080)).toEqual({ bottom: 64, left: 16 });
  });
});
