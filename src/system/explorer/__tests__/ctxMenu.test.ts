import { describe, expect, it, beforeEach } from "vitest";
import {
  CTX_DEFAULT_ORDER,
  CTX_ITEMS,
  defaultCtxConfig,
  loadCtxConfig,
  loadDeleteTier,
  resetCtxConfig,
  saveCtxConfig,
  saveDeleteTier,
} from "../ctxMenu";

/**
 * AI-09 M-19/M-26：右键菜单注册表与删除档位 ——
 * - 默认配置 = 出厂顺序、全可见（默认即现状红线）
 * - 显隐/排序持久化 + 防御性合并（未知项剔除、缺项补尾）
 * - 恢复默认一键还原
 */

const store = () => globalThis.localStorage as Storage;

describe("ctxMenu 注册表（M-19）", () => {
  beforeEach(() => {
    store().clear();
  });

  it("默认配置：出厂顺序 + 无隐藏项", () => {
    const cfg = loadCtxConfig();
    expect(cfg.order).toEqual(CTX_DEFAULT_ORDER);
    expect(cfg.hidden).toEqual([]);
    expect(defaultCtxConfig().order).toHaveLength(CTX_DEFAULT_ORDER.length);
  });

  it("注册表项均有词典 key 且 id 自洽", () => {
    for (const id of CTX_DEFAULT_ORDER) {
      expect(CTX_ITEMS[id]).toBeDefined();
      expect(CTX_ITEMS[id].id).toBe(id);
      expect(CTX_ITEMS[id].labelKey.length).toBeGreaterThan(0);
    }
  });

  it("显隐 + 排序持久化并可读回", () => {
    const cfg = {
      order: ["rename", "open", ...CTX_DEFAULT_ORDER.filter((x) => x !== "rename" && x !== "open")],
      hidden: ["purge" as const],
    };
    saveCtxConfig(cfg);
    const loaded = loadCtxConfig();
    expect(loaded.order.slice(0, 2)).toEqual(["rename", "open"]);
    expect(loaded.hidden).toEqual(["purge"]);
  });

  it("防御：存储中的未知项剔除、缺失项补尾、坏 JSON 回落默认", () => {
    store().setItem("variable:explorer:ctxmenu:v1", "not-json{");
    expect(loadCtxConfig().order).toEqual(CTX_DEFAULT_ORDER);

    store().setItem(
      "variable:explorer:ctxmenu:v1",
      JSON.stringify({ order: ["open", "nope"], hidden: ["also-nope"] }),
    );
    const cfg = loadCtxConfig();
    expect(cfg.order).toEqual(CTX_DEFAULT_ORDER); // nope 剔除、缺失补尾 → 恢复出厂序列
    expect(cfg.hidden).toEqual([]);
  });

  it("恢复默认：清键并返回出厂配置", () => {
    saveCtxConfig({ order: ["open"], hidden: ["copy" as const] });
    const cfg = resetCtxConfig();
    expect(cfg).toEqual(defaultCtxConfig());
    expect(loadCtxConfig()).toEqual(defaultCtxConfig());
  });
});

describe("删除档位（M-26）", () => {
  beforeEach(() => {
    store().clear();
  });

  it("默认 = variable（等于现状：环境回收站）", () => {
    expect(loadDeleteTier()).toBe("variable");
  });

  it("档位读写持久化；未知值回落默认", () => {
    saveDeleteTier("ask");
    expect(loadDeleteTier()).toBe("ask");
    saveDeleteTier("variable");
    expect(loadDeleteTier()).toBe("variable");

    store().setItem("variable:explorer:deleteTier", "system-unknown");
    expect(loadDeleteTier()).toBe("variable");
  });
});
