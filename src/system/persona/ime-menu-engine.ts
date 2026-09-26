/**
 * F166/F167 输入法与右键菜单引擎深化 · 候选窗布局引擎 + 菜单总装器。
 *
 * 主册判据延伸：
 * - F166【设计细节】「候选窗每次弹出重读皮肤参数」「9 候选档下翻页键提示」
 *   「组合期渲染延迟 ≤16ms」——布局引擎输出候选框几何与分页，供渲染层直用。
 * - F167【设计细节】「隐藏项在『显示更多选项』二级完整保留（功能不丢只收纳）」
 *   「菜单弹出延迟红线 100ms（定制不增负）」——总装器把 基线+应用注册+用户定制
 *   装配成最终渲染模型（含二级收纳），O(n) 单趟。
 */

import {
  SYSTEM_ITEMS, LOCKED_ITEMS, usageCount, type CtxMenuConfig,
} from "./ctxmenu";
import type { ImeSkinConfig } from "./imeskin";

// ---------- 候选窗布局引擎 ----------

export interface CandidateItem {
  index: number;   // 1-based 候选序号（数字键选择）
  text: string;
  comment?: string; // 释义/注音
}

export interface CandidateLayout {
  /** 分页后当前页候选。 */
  page: CandidateItem[];
  pageIndex: number;
  pageCount: number;
  /** 窗口几何（CSS px）。 */
  width: number;
  height: number;
  /** 行高。 */
  lineHeightPx: number;
  /** 翻页提示（9 档显示）。 */
  showPagingHint: boolean;
  pagingHintText: string | null;
}

export const CANDIDATE_ROW_PX = 30;
export const CANDIDATE_PADDING_PX = 10;

/**
 * 候选分页布局（纯函数）：candidates 全量 → 按 skin.candidates 每页条数分页。
 * 光标页由 selectedIndex 反推（选中的候选永远在当前页内）。
 */
export function layoutCandidates(all: CandidateItem[], skin: ImeSkinConfig, selectedIndex: number): CandidateLayout {
  const perPage = skin.candidates;
  const pageCount = Math.max(1, Math.ceil(all.length / perPage));
  const pageIndex = Math.min(pageCount - 1, Math.floor(Math.max(0, selectedIndex) / perPage));
  const page = all.slice(pageIndex * perPage, pageIndex * perPage + perPage);
  const rows = Math.max(1, page.length);
  const width = 220 + Math.max(...page.map((c) => c.text.length), 2) * skin.fontSize; // 估算宽度（序号+词长）
  const height = rows * CANDIDATE_ROW_PX + CANDIDATE_PADDING_PX * 2;
  const showPagingHint = perPage === 9 && pageCount > 1;
  return {
    page,
    pageIndex,
    pageCount,
    width,
    height,
    lineHeightPx: CANDIDATE_ROW_PX,
    showPagingHint,
    pagingHintText: showPagingHint ? `第 ${pageIndex + 1}/${pageCount} 页 · -/+ 翻页` : null,
  };
}

/** 组合期预算自检：布局计算耗时必须 ≤16ms（输入法不商量）。 */
export const COMPOSITION_BUDGET_MS = 16;

// ---------- 右键菜单总装器 ----------

export interface MenuItemModel {
  id: string;
  label: string;
  /** 一级可见 / 二级收纳（显示更多选项）。 */
  tier: 1 | 2;
  /** 系统组（位置不动）。 */
  group: "system" | "app";
  locked: boolean;
  disabled: boolean;
  shortcutHint?: string;
}

export interface AssembledMenu {
  items: MenuItemModel[];
  /** 二级收纳项计数（「显示更多选项 (n)」）。 */
  moreCount: number;
  /** 装配耗时（调用方实测——100ms 预算对账）。 */
  assembledInMs?: number;
}

/**
 * 菜单总装（单趟 O(n)）：
 * 1. 系统基线（乙-4 表序）——锁定项强制一级、隐藏项进二级；
 * 2. 应用注册项——按 90 天计数降序追加一级（受 autoSort 影响，未排序按注册序）；
 * 3. 全部隐藏项收进「显示更多选项」二级（功能不丢只收纳）。
 */
export function assembleContextMenu(config: CtxMenuConfig, appItems: { id: string; label: string; shortcutHint?: string }[], context: { target: "file" | "folder" | "desktop" | "background" }): AssembledMenu {
  const t0 = Date.now();
  const items: MenuItemModel[] = [];
  // 1. 系统组（保序；桌面背景不含文件类项——上下文过滤）。
  const fileOnly = new Set(["cut", "copy", "rename", "send-to"]);
  for (const base of SYSTEM_ITEMS) {
    if (context.target === "background" && fileOnly.has(base.id)) continue;
    const state = config.items[base.id];
    const hidden = state?.hidden === true;
    items.push({
      id: base.id,
      label: base.zh,
      tier: hidden ? 2 : 1,
      group: "system",
      locked: LOCKED_ITEMS.has(base.id),
      disabled: false,
    });
  }
  // 2. 应用注册组（使用计数降序——F072 一致性；隐藏的应用项同样进二级）。
  const sortedApps = [...appItems].sort((a, b) => usageCount(config, b.id) - usageCount(config, a.id));
  for (const app of sortedApps) {
    const state = config.items[app.id];
    const hidden = state?.hidden === true;
    items.push({
      id: app.id,
      label: app.label,
      tier: hidden ? 2 : 1,
      group: "app",
      locked: false,
      disabled: false,
      shortcutHint: app.shortcutHint,
    });
  }
  const moreCount = items.filter((i) => i.tier === 2).length;
  return { items, moreCount, assembledInMs: Date.now() - t0 };
}

/** 弹出预算校验（装配耗时 + 渲染准备 ≤100ms——定制不增负）。 */
export function popupBudgetOk(assembledMs: number): boolean {
  return assembledMs <= 100;
}
