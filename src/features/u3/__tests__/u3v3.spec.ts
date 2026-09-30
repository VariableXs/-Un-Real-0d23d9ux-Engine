/**
 * AI-U3 深化批次（v3 接线面）单测——动作路由中心 / F550 三面对账 / 桌面接线件逻辑。
 *
 * 环境声明：vitest 全局为 node 环境（setup.ts 仅 shim localStorage/定时器）。
 * 本文件自装最小 DOM shim（CustomEvent/addEventListener/document 属性操作），
 * 使事件派发与动作路由路径在 node 下真实可测；不污染实现代码。
 */

import { describe, expect, it, beforeEach, afterEach, afterAll } from "vitest";

/* ------------------------- node 环境 DOM micro-shim ------------------------- */

type L = (e: unknown) => void;
const listeners = new Map<string, Set<L>>();
interface ShimWin {
  addEventListener(t: string, l: L): void;
  removeEventListener(t: string, l: L): void;
  dispatchEvent(e: { type: string; detail?: unknown }): boolean;
  setTimeout: typeof setTimeout;
  clearTimeout: typeof clearTimeout;
}
const shimWindow: ShimWin = {
  addEventListener: (t, l) => {
    const s = listeners.get(t) ?? new Set<L>();
    s.add(l);
    listeners.set(t, s);
  },
  removeEventListener: (t, l) => void listeners.get(t)?.delete(l),
  dispatchEvent: (e) => {
    const attrs = (documentShim.attrs as Record<string, Record<string, unknown>>)[e.type];
    const payload = { type: e.type, detail: attrs ?? (e as { detail?: unknown }).detail };
    for (const l of listeners.get(e.type) ?? []) l(payload);
    return true;
  },
  setTimeout,
  clearTimeout,
};
const documentShim = {
  attrs: {} as Record<string, Record<string, unknown>>,
  _appId: null as string | null,
  querySelector(_sel: string) {
    return this._appId ? { getAttribute: () => this._appId } : null;
  },
  documentElement: {
    setAttribute(k: string, v: string) {
      documentShim.attrs[k] = { k, v } as unknown as Record<string, unknown>;
    },
    getAttribute(k: string) {
      const a = documentShim.attrs[k] as { v?: string } | undefined;
      return a ? a.v : null;
    },
    removeAttribute(k: string) {
      delete documentShim.attrs[k];
    },
  },
  setAttribute(k: string, v: string) {
    documentShim.attrs[k] = { k, v } as unknown as Record<string, unknown>;
  },
  getAttribute(k: string) {
    const a = documentShim.attrs[k] as { v?: string } | undefined;
    return a ? a.v : null;
  },
  removeAttribute(k: string) {
    delete documentShim.attrs[k];
  },
};

const g = globalThis as Record<string, unknown>;
const hadWindow = "window" in g;
const hadCE = "CustomEvent" in g;
const hadDoc = "document" in g;
if (!hadCE) g.CustomEvent = class {
  type: string;
  detail: unknown;
  constructor(type: string, init?: { detail?: unknown }) {
    this.type = type;
    this.detail = init?.detail;
  }
};
g.window = shimWindow; // 覆盖 setup.ts 的定时器-only shim（保留其定时器转发语义）
g.document = documentShim;

afterAll(() => {
  if (!hadWindow) delete g.window;
  if (!hadCE) delete g.CustomEvent;
  if (!hadDoc) delete g.document;
});

import {
  U3_ACTION_REGISTRY, dispatchU3Action, findU3ActionMeta, registerU3ActionHandler,
  u3ActionHandlerSnapshot,
} from "../actions";
import {
  MASTER_F501_F550, kernelLedgerSelfCheck, reconcileF501F550,
  anchorSpaceConflictFree, anchorExecutabilitySpotCheck, FNO_TO_FRONTEND_DOMAIN,
  FNO_TO_KERNEL_DOMAIN,
} from "../reconcile";
import { resnapDesktopIcons, layoutLockGuard, DESK_SHAKE_MS } from "../deskiconLayer";
import { u3Store } from "../u3store";
import { GRID_DENSITY_PX, type GridDensity } from "../deskicons";

/* ------------------------------- 动作路由中心 ------------------------------- */

describe("U3 动作路由中心（actions.ts）", () => {
  const offs: Array<() => void> = [];
  afterEach(() => {
    for (const off of offs.splice(0)) off();
  });

  beforeEach(() => {
    u3Store.reset();
    documentShim._appId = null;
  });

  it("登记表 12 条动作、F 编号语义齐全", () => {
    expect(U3_ACTION_REGISTRY.length).toBe(12);
    for (const a of U3_ACTION_REGISTRY) {
      expect(a.fno).toMatch(/^F\d{3}/);
      expect(a.name).toBeTruthy();
      expect(a.desc).toBeTruthy();
      expect(findU3ActionMeta(a.action)).not.toBeNull();
    }
  });

  it("返回 false 显式未接住 → 继续下探下一个全局处理器", async () => {
    const order: string[] = [];
    offs.push(registerU3ActionHandler("clip.wipe", () => {
      order.push("first");
      return false;
    }));
    offs.push(registerU3ActionHandler("clip.wipe", () => {
      order.push("second");
      return true;
    }));
    const r = await dispatchU3Action("clip.wipe", "api");
    expect(order).toEqual(["first", "second"]);
    expect(r).toEqual({ handled: true, via: "global" });
  });

  it("应用作用域优先于全局（appScope 有声明时先走 app 级）", async () => {
    documentShim._appId = "notes";
    const order: string[] = [];
    offs.push(registerU3ActionHandler("clip.wipe", () => {
      order.push("app");
      return true;
    }, { appId: "notes" }));
    offs.push(registerU3ActionHandler("clip.wipe", () => {
      order.push("global");
      return true;
    }));
    const r = await dispatchU3Action("clip.wipe", "api");
    expect(order).toEqual(["app"]);
    expect(r).toEqual({ handled: true, via: "app" });
  });

  it("handler 抛错不拖垮路由（扩展隔离），继续命中下一处理器", async () => {
    offs.push(registerU3ActionHandler("desktop.resnap", () => {
      throw new Error("扩展崩了");
    }));
    let ok = 0;
    offs.push(registerU3ActionHandler("desktop.resnap", () => void ok++));
    const r = await dispatchU3Action("desktop.resnap", "api");
    expect(ok).toBe(1);
    expect(r.handled).toBe(true);
  });

  it("别名动作：winnum-N 与 lockscreen:reason 派发系统事件", async () => {
    const seen: Array<{ name: string; detail: unknown }> = [];
    const listener = (e: unknown) => seen.push({ name: (e as { type: string }).type, detail: (e as { detail: unknown }).detail });
    shimWindow.addEventListener("vx-u3-winnum", listener);
    shimWindow.addEventListener("vx-u3-lock-screen", listener);
    const r1 = await dispatchU3Action("winnum-3", "runtime");
    const r2 = await dispatchU3Action("lockscreen:bt-away", "runtime");
    expect(r1).toMatchObject({ handled: true, via: "alias" });
    expect(r2).toMatchObject({ handled: true, via: "alias" });
    expect(seen.map((s) => ({ name: s.name, detail: s.detail }))).toEqual([
      { name: "vx-u3-winnum", detail: { n: 3, source: "runtime" } },
      { name: "vx-u3-lock-screen", detail: { reason: "bt-away", source: "runtime" } },
    ]);
    shimWindow.removeEventListener("vx-u3-winnum", listener);
    shimWindow.removeEventListener("vx-u3-lock-screen", listener);
  });

  it("登记快照与退订同步；空 action 注册拒绝", () => {
    expect(u3ActionHandlerSnapshot().length).toBe(0);
    const off = registerU3ActionHandler("window.peek", () => true);
    expect(u3ActionHandlerSnapshot().length).toBe(1);
    off();
    expect(u3ActionHandlerSnapshot().length).toBe(0);
    expect(() => registerU3ActionHandler("", () => true)).toThrow(/非空/);
  });
});

/* ------------------------------- F550 三面对账 ------------------------------- */

describe("F550 三面对账引擎（reconcile.ts）", () => {
  it("内核账册自证：616 检查项 / 307 单测（对源码实测核对）", () => {
    const led = kernelLedgerSelfCheck();
    expect(led.checksTotal).toBe(616);
    expect(led.testsTotal).toBe(307);
    expect(led.ledgerOk).toBe(true);
  });

  it("主册面 50 判据编号齐全且首尾正确", () => {
    expect(MASTER_F501_F550.length).toBe(50);
    expect(MASTER_F501_F550[0]).toMatchObject({ fno: "F501", name: "桌面图标文字可读性" });
    expect(MASTER_F501_F550[49]).toMatchObject({ fno: "F550", name: "I 域批次六验收锚点" });
  });

  it("编号映射：F501→deskicons、F535→winkeys、F549→clockcal 三面同域", () => {
    expect(FNO_TO_FRONTEND_DOMAIN["F501"]).toBe("deskicons");
    expect(FNO_TO_KERNEL_DOMAIN["F501"]).toBe("deskicons");
    expect(FNO_TO_FRONTEND_DOMAIN["F535"]).toBe("winkeys");
    expect(FNO_TO_KERNEL_DOMAIN["F535"]).toBe("winkeys");
    expect(FNO_TO_FRONTEND_DOMAIN["F549"]).toBe("clockcal");
    expect(FNO_TO_KERNEL_DOMAIN["F549"]).toBe("clockcal");
  });

  it("三面对账全绿：50 项逐项三面在位 + 前端自检全绿 + 内核账册可信", () => {
    const rec = reconcileF501F550();
    expect(rec.total).toBe(50);
    expect(rec.ok).toBe(50);
    expect(rec.missing).toEqual([]);
    expect(rec.kernelLedger.ledgerOk).toBe(true);
    expect(rec.frontendRuntime.allGreen).toBe(true);
    expect(rec.allGreen).toBe(true);
  });

  it("对账规则自我验证：人为抽掉一个前端域 → 对账红且缺面显性", () => {
    const saved = FNO_TO_FRONTEND_DOMAIN["F505"];
    delete FNO_TO_FRONTEND_DOMAIN["F505"];
    try {
      const rec = reconcileF501F550();
      expect(rec.ok).toBe(49);
      expect(rec.allGreen).toBe(false);
      const row = rec.rows.find((r) => r.fno === "F505");
      expect(row?.ok).toBe(false);
    } finally {
      FNO_TO_FRONTEND_DOMAIN["F505"] = saved as string; // 还原（中断原子性）
    }
  });

  it("合并无冲突：F501-550 与 F400/F575 区间不相交（F600 超集登记豁免）", () => {
    const v = anchorSpaceConflictFree();
    expect(v.conflictFree).toBe(true);
    expect(v.overlaps).toEqual([]);
  });

  it("锚点可执行性抽查：5 条全部双面可执行", () => {
    const spot = anchorExecutabilitySpotCheck(5);
    expect(spot.length).toBe(5);
    expect(spot.every((s) => s.executable)).toBe(true);
  });
});

/* ------------------------------- 桌面接线件逻辑 ------------------------------- */

describe("桌面接线件（deskiconLayer 逻辑面）", () => {
  beforeEach(() => u3Store.reset());
  afterEach(() => {
    documentShim.documentElement.removeAttribute("data-u3-clicklock-grabbed");
    documentShim.documentElement.removeAttribute("data-u3-clicklock-pending");
  });

  it("resnapDesktopIcons：按当前密度档重排并派发 vx-u3-resnap", () => {
    const seen: Array<{ to: { colPx: number; rowPx: number }; points: unknown }> = [];
    const listener = (e: unknown) => seen.push((e as { detail: { to: { colPx: number; rowPx: number }; points: unknown } }).detail);
    shimWindow.addEventListener("vx-u3-resnap", listener);
    const out = resnapDesktopIcons({
      points: [{ x: 8, y: 3 }, { x: 60, y: 70 }],
      from: { colPx: 96, rowPx: 96 },
    });
    expect(seen.length).toBe(1);
    expect(seen[0]!.to).toEqual({ colPx: GRID_DENSITY_PX.standard, rowPx: GRID_DENSITY_PX.standard });
    expect(out[0]).toEqual({ x: 0, y: 0 });
    expect(out[1]).toEqual({ x: 80, y: 80 });
    shimWindow.removeEventListener("vx-u3-resnap", listener);
  });

  it("layoutLockGuard：未锁放行；锁定拒绝+派发抖动反馈（120ms 同源）", () => {
    u3Store.set("layoutLock", { locked: false });
    expect(layoutLockGuard()).toMatchObject({ allowed: true, shakeMs: 0 });
    const seen: Array<{ ms: number; message: string }> = [];
    const listener = (e: unknown) => seen.push((e as { detail: { ms: number; message: string } }).detail);
    shimWindow.addEventListener("vx-u3-shake", listener);
    u3Store.set("layoutLock", { locked: true });
    const v = layoutLockGuard();
    expect(v.allowed).toBe(false);
    expect(v.shakeMs).toBe(DESK_SHAKE_MS);
    expect(v.statusbar).toContain("已锁定");
    expect(seen.length).toBe(1);
    expect(seen[0]!.ms).toBe(120);
    shimWindow.removeEventListener("vx-u3-shake", listener);
  });

  it("网格密度档切换后 resnap 用新档（loose 96）", () => {
    u3Store.set("gridDensity", { density: "loose" as GridDensity });
    const seen: Array<{ to: { colPx: number; rowPx: number } }> = [];
    const listener = (e: unknown) => seen.push((e as { detail: { to: { colPx: number; rowPx: number } } }).detail);
    shimWindow.addEventListener("vx-u3-resnap", listener);
    resnapDesktopIcons({ points: [{ x: 10, y: 10 }], from: { colPx: 80, rowPx: 80 } });
    expect(seen[0]!.to).toEqual({ colPx: 96, rowPx: 96 });
    shimWindow.removeEventListener("vx-u3-resnap", listener);
  });
});
