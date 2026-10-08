/// <reference types="node" />
/**
 * H4 消费面接线契约舱（v6 · 深化批次六）：
 * consumers.ts 五十项接线表的完整性机检——四道审计（命名/唯一/覆盖/对齐）+ 查询面 +
 * 表面词表自洽。接线契约是「功能做出来就要让人找得到」的入口面，本舱保证它零漏项。
 */
import { describe, expect, it } from "vitest";
import {
  BINDINGS,
  SURFACES,
  INPUT_SOURCES,
  auditEventNaming,
  auditMountUniqueness,
  auditSurfaceCoverage,
  auditRegistryAlignment,
  wiringReport,
  bindingFor,
  bindingsOn,
  type ConsumerBinding,
} from "../consumers";
import { H4_REGISTRY } from "../../../system/h4/registry";

describe("v6 接线契约：四道审计", () => {
  it("事件命名规范：出向挂功能域 f<XXX>.、入向挂已注册源域（双向两域零裸奔）", () => {
    const a = auditEventNaming();
    expect(a.pass).toBe(true);
    expect(a.problems).toEqual([]);
    // 域外来源必须先进册：伪造未注册源域即红灯
    const rogue: ConsumerBinding[] = [{ item: "F001", surfaces: ["taskbar"], mountId: "x", eventsIn: ["widget.poke"], eventsOut: ["f001.poked"], note: "x" }];
    expect(auditEventNaming(rogue).pass).toBe(false);
  });

  it("入向源词表自洽：表面全部入册、词表无重复", () => {
    expect(INPUT_SOURCES.length).toBe(new Set(INPUT_SOURCES).size);
    for (const s of SURFACES) expect(INPUT_SOURCES.includes(s)).toBe(true);
  });

  it("挂载点唯一：mountId 全域唯一（同点位双挂 = 路由歧义）", () => {
    const a = auditMountUniqueness();
    expect(a.pass).toBe(true);
  });

  it("表面覆盖：每项 ≥1 表面、14 表面全被使用（零空表面）", () => {
    const a = auditSurfaceCoverage();
    expect(a.pass).toBe(true);
  });

  it("登记册对齐：绑定表与 H4_REGISTRY 50 项逐项一一对应（不漏项不越项）", () => {
    const a = auditRegistryAlignment();
    expect(a.pass).toBe(true);
    expect(BINDINGS.length).toBe(H4_REGISTRY.length);
  });

  it("审计器能抓真违规（审计器自身的有效性自证）：坏事件名/重复挂载/零表面均红灯", () => {
    const bad: ConsumerBinding[] = [
      { item: "F001", surfaces: [], mountId: "taskbar.thumb-actions", eventsIn: ["naked-event"], eventsOut: [], note: "x" },
      { item: "F002", surfaces: ["taskbar"], mountId: "taskbar.thumb-actions", eventsIn: [], eventsOut: [], note: "x" },
    ];
    expect(auditEventNaming(bad).pass).toBe(false);
    expect(auditMountUniqueness(bad).pass).toBe(false);
    expect(auditSurfaceCoverage(bad).pass).toBe(false);
    // 登记册对齐对任意非 50 项表恒红（绑定表 ≠ 登记册即不通过）
    expect(auditRegistryAlignment(bad).pass).toBe(false);
  });
});

describe("v6 接线契约：接线报告与查询面", () => {
  it("接线报告：50 项、事件计数与逐项之和一致、四道审计全绿", () => {
    const r = wiringReport();
    expect(r.totalItems).toBe(50);
    expect(r.allGreen).toBe(true);
    expect(r.totalEventsIn).toBe(BINDINGS.reduce((s, b) => s + b.eventsIn.length, 0));
    expect(r.totalEventsOut).toBe(BINDINGS.reduce((s, b) => s + b.eventsOut.length, 0));
    expect(r.perSurface.find((p) => p.surface === "taskbar")!.count).toBeGreaterThan(0);
  });

  it("单项查询：bindingFor 命中与未命中（未命中如实 null——不编造契约）", () => {
    const b = bindingFor("F351");
    expect(b!.mountId).toBe("task-view.workspace-snapshot");
    expect(b!.surfaces).toContain("settings");
    expect(bindingFor("F999")).toBeNull();
  });

  it("表面视角：任务栏/托盘/资源管理器都是多功能集散面；恢复环境至少挂演练", () => {
    expect(bindingsOn("taskbar").length).toBeGreaterThanOrEqual(5);
    expect(bindingsOn("tray").length).toBeGreaterThanOrEqual(4);
    expect(bindingsOn("explorer").length).toBeGreaterThanOrEqual(5);
    expect(bindingsOn("recovery-env").map((b) => b.item)).toEqual(["F397"]);
  });

  it("表面词表自洽：SURFACES 14 项、绑定表引用无词表外表面", () => {
    expect(SURFACES.length).toBe(14);
    for (const b of BINDINGS) {
      for (const s of b.surfaces) expect(SURFACES.includes(s)).toBe(true);
    }
  });

  it("关键接线抽查：判据点名的入口面逐项在位", () => {
    // F370 后台失败唯一出口在通知中心（横幅=0）
    expect(bindingFor("F370")!.surfaces).toEqual(["notify-center"]);
    // F399 彩蛋三触发面（终端/关于页/桌面）——不进设置/搜索/引导
    const egg = bindingFor("F399")!;
    expect(egg.surfaces).toContain("terminal");
    expect(egg.surfaces).not.toContain("settings");
    // F397 演练挂恢复环境（F198 三卡联动）
    expect(bindingFor("F397")!.surfaces).toContain("recovery-env");
    // F364 整理建议条在资源管理器（下载目录常驻条）
    expect(bindingFor("F364")!.surfaces).toContain("explorer");
    // F361 录制挂快速设置与托盘（红框/隐私指示与浮条）
    expect(bindingFor("F361")!.surfaces).toContain("quick-settings");
    // F385 语义树全局面（窗口框 + 通知中心播报）
    expect(bindingFor("F385")!.surfaces).toContain("window-frame");
  });
});
