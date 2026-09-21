/**
 * 需求 2 界面逻辑测试：钉住「不撒谎」的三条契约。
 *
 * 为什么值得写：这一整条链路（内核交接 → Windows 自启 Variable）跨两个系统，
 * 出了问题用户只会看到"点进了一个不是 Variable 的桌面"或"停在 ushell"，很难
 * 反推原因。界面必须把「哪一半没开」说清楚，而不是给一个笼统的"正常"。
 */
import { describe, expect, it } from "vitest";
import type { Shell } from "../ipc";
import {
  HANDOFF_FALLBACK,
  autostartDisabled,
  chainComplete,
  chainHint,
  handoffDisabled,
  sharedLabel,
  sharedState,
} from "../handoffView";

function cfg(patch: Partial<Shell.BootCfgView> = {}): Shell.BootCfgView {
  return {
    found: true,
    sharedRoot: "/mnt/shared",
    path: "/mnt/shared/boot-select.json",
    handoff: true,
    handoffExplicit: true,
    timeoutSec: 5,
    defaultEntry: "variable",
    showMenu: true,
    ...patch,
  };
}

describe("共享盘四态", () => {
  it("未读到既不是就绪也不是失败，而是「加载中」", () => {
    expect(sharedState(null)).toBe("loading");
    expect(sharedLabel(null)).toContain("正在读取");
  });

  it("没有共享盘要说「没找到」，不能说「正常」", () => {
    const c = cfg({ sharedRoot: "", path: "", found: false });
    expect(sharedState(c)).toBe("no-shared");
    expect(sharedLabel(c)).toContain("未找到共享盘");
  });

  it("有盘但还没有配置文件，要如实说缺哪一份", () => {
    const c = cfg({ found: false });
    expect(sharedState(c)).toBe("no-config");
    expect(sharedLabel(c)).toContain("还没有 boot-select.json");
    expect(sharedLabel(c)).toContain(c.path);
  });

  it("就绪时要说清 handoff 是否跟随内核默认值", () => {
    expect(sharedLabel(cfg({ handoffExplicit: true }))).not.toContain("内核默认值");
    expect(sharedLabel(cfg({ handoffExplicit: false }))).toContain("内核默认值");
  });
});

describe("开关可用性", () => {
  it("配置没读到前不许点交接开关（盲写会覆盖用户原有设置）", () => {
    expect(handoffDisabled(null, false)).toBe(true);
    expect(handoffDisabled(cfg(), false)).toBe(false);
  });

  it("写入中一律禁用（防连点造成两次落盘）", () => {
    expect(handoffDisabled(cfg(), true)).toBe(true);
    expect(autostartDisabled({ on: true }, true)).toBe(true);
  });

  it("自启状态读不到时禁用自启开关", () => {
    expect(autostartDisabled(null, false)).toBe(true);
    expect(autostartDisabled({ on: false }, false)).toBe(false);
  });
});

describe("链路完整性提示（把缺口说清楚）", () => {
  it("两半都开才算完整", () => {
    expect(chainComplete(cfg(), { on: true })).toBe(true);
    expect(chainComplete(cfg(), { on: false })).toBe(false);
    expect(chainComplete(cfg({ handoff: false }), { on: true })).toBe(false);
  });

  it("两半都没开 → 明确说停在 ushell", () => {
    expect(chainHint(cfg({ handoff: false }), { on: false })).toContain("ushell");
  });

  it("只缺内核侧 → 指向 ushell", () => {
    expect(chainHint(cfg({ handoff: false }), { on: true })).toContain("内核侧交接关着");
  });

  it("只缺 Windows 侧 → 指向「只会看到普通 Windows 桌面」，这才是最容易被忽略的坑", () => {
    const h = chainHint(cfg(), { on: false });
    expect(h).toContain("普通 Windows 桌面");
    expect(h).toContain("不会自动进 Variable");
  });

  it("完整时不画多余提示", () => {
    expect(chainHint(cfg(), { on: true })).toBe("");
  });

  it("状态未读到时不给结论（宁可不提示，也不猜）", () => {
    expect(chainHint(null, { on: true })).toBe("");
    expect(chainHint(cfg(), null)).toBe("");
  });
});

describe("空态锚点", () => {
  it("HANDOFF_FALLBACK 的 handoff 必须与内核默认值 true 一致", () => {
    // 内核 bootcfg::defaults() 里 handoff 缺键 → true；界面若显示 false，
    // 用户会以为关着而实际开着。
    expect(HANDOFF_FALLBACK.handoff).toBe(true);
    expect(HANDOFF_FALLBACK.handoffExplicit).toBe(false);
    expect(HANDOFF_FALLBACK.found).toBe(false);
  });
});
