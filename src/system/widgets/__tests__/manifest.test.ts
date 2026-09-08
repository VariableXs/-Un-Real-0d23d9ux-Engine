import { describe, expect, it } from "vitest";
import {
  bumpFailure, canSendOutbound, hasPermission, isInboundAllowed, resetFailures,
  shouldCollapse, validateManifest, WIDGET_FAILURE_LIMIT, type WidgetFailureDoc,
  type WidgetManifest, type WidgetOutboundMessage,
} from "../manifest";

const good: WidgetManifest = {
  format: "widget",
  version: 1,
  id: "tp.example.clock",
  name: "Example Clock",
  size: "1x1",
  permissions: ["clock", "refresh"],
  refresh: 30,
};

describe("validateManifest", () => {
  it("合法通过", () => {
    const r = validateManifest({ ...good });
    expect(r.ok).toBe(true);
    expect(r.data?.refresh).toBe(30);
  });
  it("refresh 下限 5s", () => {
    const r = validateManifest({ ...good, refresh: 1 });
    expect(r.ok).toBe(false);
  });
  it("权限不在白名单 → 拒绝", () => {
    const r = validateManifest({ ...good, permissions: ["fs:write"] });
    expect(r.ok).toBe(false);
    expect(r.errors.join()).toContain("permissions");
  });
  it("size/id/format 校验", () => {
    expect(validateManifest({ ...good, size: "3x3" }).ok).toBe(false);
    expect(validateManifest({ ...good, id: "" }).ok).toBe(false);
    expect(validateManifest({ ...good, format: "other" }).ok).toBe(false);
    expect(validateManifest(null).ok).toBe(false);
  });
});

describe("hasPermission / 消息白名单", () => {
  it("权限命中", () => {
    expect(hasPermission(good, "clock")).toBe(true);
    expect(hasPermission(good, "todo:read")).toBe(false);
  });
  it("入站仅 widget:refresh", () => {
    expect(isInboundAllowed({ type: "widget:refresh" })).toBe(true);
    expect(isInboundAllowed({ type: "eval", code: "…" })).toBe(false);
    expect(isInboundAllowed(null)).toBe(false);
    expect(isInboundAllowed("widget:refresh")).toBe(false);
  });
  it("出站按权限放行", () => {
    const tick: WidgetOutboundMessage = { type: "widget:tick", time: new Date().toISOString() };
    const todos: WidgetOutboundMessage = { type: "widget:todos", items: [] };
    expect(canSendOutbound(good, tick)).toBe(true);
    expect(canSendOutbound(good, todos)).toBe(false);
    const noPerm = { ...good, permissions: [] as never[] };
    expect(canSendOutbound(noPerm, tick)).toBe(false);
  });
});

describe("生命周期：连续失败 3 次自动收起", () => {
  it("计数/重置/阈值", () => {
    let doc: WidgetFailureDoc = { counts: {} };
    doc = bumpFailure(doc, "tp.a");
    doc = bumpFailure(doc, "tp.a");
    expect(shouldCollapse(doc, "tp.a")).toBe(false);
    doc = bumpFailure(doc, "tp.a");
    expect(shouldCollapse(doc, "tp.a")).toBe(true);
    expect(WIDGET_FAILURE_LIMIT).toBe(3);
    doc = resetFailures(doc, "tp.a");
    expect(shouldCollapse(doc, "tp.a")).toBe(false);
    expect(doc.counts["tp.b"]).toBeUndefined();
  });
  it("不同组件独立计数", () => {
    let doc: WidgetFailureDoc = { counts: {} };
    doc = bumpFailure(doc, "a");
    doc = bumpFailure(doc, "a");
    doc = bumpFailure(doc, "a");
    expect(shouldCollapse(doc, "a")).toBe(true);
    expect(shouldCollapse(doc, "b")).toBe(false);
  });
});