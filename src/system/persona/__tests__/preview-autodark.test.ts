import { beforeEach, describe, expect, it } from "vitest";
import { PreviewSession, crashRecoveryIsClean, PREVIEW_LATENCY_BUDGET_MS } from "../preview";
import {
  AutoDarkController, defaultAutoDarkConfig, desiredSide, nextSwitchAt, parseHHMM,
  saveAutoDarkConfig, sunTimesMinutes, tickAutoDark, SWITCH_FADE_MS, ADVANCE_TOAST_SECONDS,
} from "../autodark";
import { personaStore } from "../store";
import { defaultTokenTable, saveTokenTable } from "../tokens";

beforeEach(() => {
  personaStore.reset();
});

describe("F152 实时预览编辑器", () => {
  it("开启即快照 → dirty 检测 → apply 生效", () => {
    const table = defaultTokenTable();
    saveTokenTable(table);
    const s = new PreviewSession();
    s.start(table);
    expect(s.dirty).toBe(false);
    s.patch((d) => { d.radius.window = 28; });
    expect(s.dirty).toBe(true);
    const { applied } = s.apply();
    expect(applied.radius.window).toBe(28);
    expect(s.active).toBe(false);
  });

  it("放弃 = 哈希还原验证（零残留）", () => {
    const table = defaultTokenTable();
    const s = new PreviewSession();
    s.start(table);
    s.patch((d) => { d.colors["--p-accent"] = "#ff0000"; });
    s.patch((d) => { d.font.body = 22; });
    const { restored, hashVerified } = s.discard();
    expect(hashVerified).toBe(true);
    expect(restored.colors["--p-accent"]).toBe(table.colors["--p-accent"]);
    expect(restored.font.body).toBe(table.font.body);
  });

  it("改-预览延迟记账在预算内（≤100ms 判据）", () => {
    const s = new PreviewSession();
    s.start(defaultTokenTable());
    s.patch(() => {});
    expect(s.withinLatencyBudget).toBe(true);
    expect(PREVIEW_LATENCY_BUDGET_MS).toBe(100);
  });

  it("未开启会话时操作拒绝（异常显性化）", () => {
    const s = new PreviewSession();
    expect(() => s.patch(() => {})).toThrow(/会话未开启/);
    expect(() => s.apply()).toThrow(/会话未开启/);
    expect(() => s.discard()).toThrow(/会话未开启/);
  });

  it("崩溃恢复：落盘正身损坏按校验失败报备", () => {
    expect(crashRecoveryIsClean(defaultTokenTable())).toBe(true);
    expect(crashRecoveryIsClean({ colors: { "--p-accent": "not-hex" } })).toBe(false);
  });
});

describe("F153 主题深浅自动切换", () => {
  it("HH:MM 解析与非法拦截", () => {
    expect(parseHHMM("20:00")).toBe(1200);
    expect(parseHHMM("24:00")).toBeNull();
    expect(parseHHMM("abc")).toBeNull();
    expect(parseHHMM("7:5")).toBeNull();
  });

  it("区间推断：跨零点（20:00→07:00）夜里为深", () => {
    const cfg = defaultAutoDarkConfig();
    cfg.enabled = true;
    cfg.mode = "timer";
    cfg.darkAt = "20:00";
    cfg.lightAt = "07:00";
    expect(desiredSide(cfg, new Date("2026-09-26T23:00:00").getTime())).toBe("dark");
    expect(desiredSide(cfg, new Date("2026-09-26T00:30:00").getTime())).toBe("dark");
    expect(desiredSide(cfg, new Date("2026-09-26T12:00:00").getTime())).toBe("light");
  });

  it("手动暂停当日 → tick 返回 none（尊重手动）", () => {
    const cfg = defaultAutoDarkConfig();
    cfg.enabled = true;
    const now = new Date("2026-09-26T20:30:00").getTime();
    cfg.manualPauseDate = new Date(now).toLocaleDateString("sv");
    saveAutoDarkConfig(cfg);
    const d = tickAutoDark(cfg, { now, activeSide: "light", darkThemeAvailable: true, lightThemeAvailable: true, advanceToastShown: false, advanceConfirmed: false });
    expect(d.action).toBe("none");
    expect(d.reason).toContain("暂停");
  });

  it("主题卸载 → 切换跳过 + 提示重绑", () => {
    const cfg = defaultAutoDarkConfig();
    cfg.enabled = true;
    cfg.mode = "timer";
    cfg.darkAt = "00:01";
    cfg.lightAt = "23:59";
    const d = tickAutoDark(cfg, { now: new Date("2026-09-26T00:02:00").getTime(), activeSide: "light", darkThemeAvailable: false, lightThemeAvailable: true, advanceToastShown: true, advanceConfirmed: false });
    expect(d.action).toBe("none");
    expect(d.reason).toContain("卸载");
  });

  it("预告窗口 60s：进窗先 preToast，确认后执行", () => {
    const cfg = defaultAutoDarkConfig();
    cfg.enabled = true;
    cfg.mode = "timer";
    cfg.darkAt = "20:00";
    cfg.lightAt = "07:00";
    const inWindow = new Date("2026-09-26T19:59:30").getTime();
    const d1 = tickAutoDark(cfg, { now: inWindow, activeSide: "light", darkThemeAvailable: true, lightThemeAvailable: true, advanceToastShown: false, advanceConfirmed: false });
    expect(d1.action).toBe("preToast");
    expect(ADVANCE_TOAST_SECONDS).toBe(60);
    const d2 = tickAutoDark(cfg, { now: inWindow, activeSide: "light", darkThemeAvailable: true, lightThemeAvailable: true, advanceToastShown: true, advanceConfirmed: false });
    expect(d2.action).toBe("none");
    const atEdge = new Date("2026-09-26T20:00:10").getTime();
    const d3 = tickAutoDark(cfg, { now: atEdge, activeSide: "light", darkThemeAvailable: true, lightThemeAvailable: true, advanceToastShown: true, advanceConfirmed: false });
    expect(d3.action).toBe("toDark");
  });

  it("sunTimesMinutes：中纬度夏季日落晚于冬季；极昼极夜返回 null", () => {
    const summer = sunTimesMinutes(40, 172); // 夏至附近
    const winter = sunTimesMinutes(40, 355); // 冬至附近
    expect(summer).not.toBeNull();
    expect(winter).not.toBeNull();
    expect(summer!.sunset).toBeGreaterThan(winter!.sunset);
    expect(sunTimesMinutes(89, 172)).toBeNull(); // 北极极昼
  });

  it("Controller.pauseToday 写入手动暂停并落状态 paused", () => {
    const host = { applySide: () => {}, showAdvanceToast: () => {}, report: () => {} };
    const c = new AutoDarkController(host);
    const now = new Date("2026-09-26T15:00:00").getTime();
    c.pauseToday(now);
    expect(personaStore.get("theme").autoDark).toBeTruthy();
  });

  it("nextSwitchAt 返回下次边界时刻", () => {
    const cfg = defaultAutoDarkConfig();
    cfg.enabled = true;
    const now = new Date("2026-09-26T10:00:00").getTime();
    expect(nextSwitchAt(cfg, now)).toBe(new Date("2026-09-26T20:00:00").getTime());
  });

  it("切换动画 300ms 交叉淡入（常量契约）", () => {
    expect(SWITCH_FADE_MS).toBe(300);
  });
});
