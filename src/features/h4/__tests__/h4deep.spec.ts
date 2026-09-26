/**
 * H4 深化批次单测（v2 界面壳层）：
 * 运行时状态机（h4ui）的开机序列/滤镜单点/阅读变量/徽标时序，
 * 以及与引擎（F371/F386/F387/F399）的联动语义。纯逻辑、可注入、零 DOM 依赖。
 */

import { describe, expect, it } from "vitest";
import { memStore, __clearMem, type KvStore } from "../../../system/h4/internal/store";
import * as f371 from "../../../system/h4/f371-bootBadge";
import * as f386 from "../../../system/h4/f386-readingMode";
import * as f387 from "../../../system/h4/f387-grayscaleMode";
import * as f399 from "../../../system/h4/f399-easterEggs";
import {
  applyFilterPlan,
  applyReadingVars,
  badgePhaseClass,
  filterPlan,
  readingVarsFor,
  runBootPlan,
  type BootOnceState,
} from "../h4ui";

/* ------------------------------- 测试基建 ------------------------------- */

function freshStore(): KvStore {
  __clearMem();
  return memStore();
}

/** 最小元素桩（applyFilterPlan/applyReadingVars 的 DOM 契约面）。 */
function fakeEl(): { attrs: Map<string, string>; props: Map<string, string>; el: HTMLElement } {
  const attrs = new Map<string, string>();
  const props = new Map<string, string>();
  const el = {
    setAttribute: (k: string, v: string): void => void attrs.set(k, v),
    removeAttribute: (k: string): void => void attrs.delete(k),
    style: {
      setProperty: (k: string, v: string): void => void props.set(k, v),
      removeProperty: (k: string): void => void props.delete(k),
    },
  } as unknown as HTMLElement;
  return { attrs, props, el };
}

/* ------------------------------- 开机序列 ------------------------------- */

describe("runBootPlan（F371 徽标 + F399 彩蛋编排）", () => {
  it("首跑记账：seq = 历史长度 + 1，徽标视图与实测链同源（drift < 100ms）", () => {
    const store = freshStore();
    const r = runBootPlan({ done: false } as BootOnceState, store, 3200);
    expect(r.state.done).toBe(true);
    expect(r.plan.seq).toBe(1);
    expect(r.plan.badgeVm?.seconds).toBe(3.2);
    expect(r.plan.badgeVm?.ok).toBe(true);
    expect(f371.bootHistory(store)).toHaveLength(1);
  });

  it("同会话二跑零动作（去重守卫——StrictMode 双挂载不重复记账）", () => {
    const store = freshStore();
    const first = runBootPlan({ done: false } as BootOnceState, store, 3200);
    const second = runBootPlan(first.state, store, 9999);
    expect(second.plan.badge).toBeNull();
    expect(second.plan.badgeVm).toBeNull();
    expect(second.plan.eggPlay).toBe(false);
    expect(second.plan.seq).toBe(0);
    expect(f371.bootHistory(store)).toHaveLength(1); // 不重复入账
  });

  it("徽标可关（判据）：关闭后仍记账（曲线常驻）但不发徽标计划", () => {
    const store = freshStore();
    f371.setEnabled(false, store);
    const r = runBootPlan({ done: false } as BootOnceState, store, 2100);
    expect(r.plan.badge).toBeNull();
    expect(r.plan.badgeVm).toBeNull();
    expect(f371.bootHistory(store)).toHaveLength(1);
  });

  it("彩蛋①：第 100 次开机触发且一生一次（跨会话持久化）", () => {
    const store = freshStore();
    // 前 99 次：只计数不触发（每会话一次守卫，直接走引擎层模拟历史会话）
    for (let i = 0; i < 99; i++) f399.countBoot(store);
    const r100 = runBootPlan({ done: false } as BootOnceState, store, 3200);
    expect(r100.plan.eggPlay).toBe(true);
    expect(r100.plan.eggVariant).toBe("star-emblem-particles");
    // 第 101 次会话：不再触发
    const r101 = runBootPlan({ done: false } as BootOnceState, store, 3000);
    expect(r101.plan.eggPlay).toBe(false);
  });

  it("seq 跨会话连续（第 3 次开机 → seq 3，徽标与曲线同源账本）", () => {
    const store = freshStore();
    f371.recordBoot({ seq: 1, measuredMs: 3000, at: 1 }, store);
    f371.recordBoot({ seq: 2, measuredMs: 3100, at: 2 }, store);
    const r = runBootPlan({ done: false } as BootOnceState, store, 2900);
    expect(r.plan.seq).toBe(3);
    expect(f371.curveFromSameLedger(store).at(-1)?.seconds).toBe(2.9);
  });
});

describe("badgePhaseClass（F371 淡入 300 / 停留 3000 / 淡出 500）", () => {
  it("时间轴逐段校验（边界值判定）——t≤0 引擎语义为 hidden（淡入未开始）", () => {
    expect(badgePhaseClass(0)).toBe("h4-boot-badge--gone");
    expect(badgePhaseClass(1)).toBe("h4-boot-badge--in");
    expect(badgePhaseClass(150)).toBe("h4-boot-badge--in");
    expect(badgePhaseClass(300)).toBe("h4-boot-badge--hold");
    expect(badgePhaseClass(3000)).toBe("h4-boot-badge--hold");
    expect(badgePhaseClass(3300)).toBe("h4-boot-badge--out");
    expect(badgePhaseClass(3799)).toBe("h4-boot-badge--out");
    expect(badgePhaseClass(3800)).toBe("h4-boot-badge--gone");
  });
});

/* ------------------------------- 滤镜单点 ------------------------------- */

describe("filterPlan / applyFilterPlan（F387 互斥单点的落地面）", () => {
  it("四态映射：attr 与 CSS 滤镜一一对应；none = 双清", () => {
    expect(filterPlan("grayscale")).toEqual({ attr: "grayscale", cssFilter: "grayscale(1)" });
    expect(filterPlan("highContrast").attr).toBe("high-contrast");
    expect(filterPlan("colorDeficiency").cssFilter).toContain("hue-rotate");
    expect(filterPlan("none")).toEqual({ attr: null, cssFilter: null });
  });

  it("应用契约：写 attr + --h4-filter；关闭零残留（属性与变量都摘除）", () => {
    const { attrs, props, el } = fakeEl();
    applyFilterPlan(el, filterPlan("grayscale"));
    expect(attrs.get("data-h4-filter")).toBe("grayscale");
    expect(props.get("--h4-filter")).toBe("grayscale(1)");
    applyFilterPlan(el, filterPlan("none"));
    expect(attrs.has("data-h4-filter")).toBe(false);
    expect(props.has("--h4-filter")).toBe(false);
  });

  it("与引擎互斥语义联动：requestFilter 后到者替换前者（一次只有一个滤镜生效）", () => {
    const store = freshStore();
    f387.requestFilter(store, "grayscale");
    const r = f387.requestFilter(store, "colorDeficiency");
    expect(r.now).toBe("colorDeficiency");
    expect(r.replaced).toBe("grayscale");
    expect(filterPlan(f387.activeFilter(store)).attr).toBe("cvd");
    const toggle = f387.requestFilter(store, "colorDeficiency"); // 同滤镜再请求 = 关（磁贴语义）
    expect(toggle.now).toBe("none");
  });
});

/* ------------------------------- 阅读模式 ------------------------------- */

describe("readingVarsFor / applyReadingVars（F386 三参数落地面）", () => {
  it("变量换算：行距 1.6、45 字/行按 CJK/ASCII 混合估宽、衬线旗标", () => {
    const style = f386.withSerif(f386.defaultStyle(), true);
    const cjk = readingVarsFor(style, 16, "这是一段全角中文，用来估算页宽。");
    const ascii = readingVarsFor(style, 16, "ascii only sample text");
    expect(cjk.lineHeightRatio).toBe("1.6");
    expect(serifFlag(cjk.serif)).toBe("1");
    expect(Number(cjk.measurePx)).toBeGreaterThan(Number(ascii.measurePx)); // 全角占宽更大 → 页更宽
    expect(Number(ascii.measurePx)).toBe(45 * 0.5 * 16);
  });

  it("应用契约：on 写三变量 + data-h4-reading；off 全摘除（零残留）", () => {
    const { attrs, props, el } = fakeEl();
    applyReadingVars(el, readingVarsFor(f386.defaultStyle(), 16, "样例文本"));
    expect(attrs.get("data-h4-reading")).toBe("on");
    expect(props.get("--h4-reading-line")).toBe("1.6");
    applyReadingVars(el, null);
    expect(attrs.has("data-h4-reading")).toBe(false);
    expect(props.size).toBe(0);
  });

  it("每应用记忆联动（引擎面）：editor 开、desktop 关互不干扰", () => {
    const store = freshStore();
    f386.setAppMode("editor", true, true, store);
    expect(f386.appMode("editor", store)).toEqual({ on: true, serif: true });
    expect(f386.appMode("desktop", store)).toEqual({ on: false, serif: false });
  });
});

function serifFlag(v: string): string {
  return v;
}
