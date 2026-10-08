/**
 * AURORA-10000 · AI-11~AI-15 车道 · 持久化状态（本地，不出本机）。
 * - preset 族：同族单选（ selections[familyId] = entryId | null ）；
 * - switch 项：独立启停（ enabled[entryId] = true ）；
 * - 持久化 localStorage `variable:aurora:w2:v1`；档案导出/导入走 JSON。
 * 复用 src/lib/store.ts 的最小 Store（与既有 lane 一致，不引新依赖）。
 */
import { createStore, useStore } from "../../lib/store";
import { FAMILIES, findEntry } from "./catalog";
import type { DesignEntry } from "./types";

export interface DesignState {
  selections: Record<string, string | null>;
  enabled: Record<string, boolean>;
  /** 令牌调试面板（F01646）的实时覆写。 */
  tokenOverrides: Record<string, string>;
}

const LS_KEY = "variable:aurora:w2:v1";

export function defaultState(): DesignState {
  return { selections: {}, enabled: {}, tokenOverrides: {} };
}

export function loadState(): DesignState {
  try {
    const raw = localStorage.getItem(LS_KEY);
    if (!raw) return defaultState();
    const p = JSON.parse(raw) as Partial<DesignState>;
    return {
      selections: typeof p.selections === "object" && p.selections ? p.selections : {},
      enabled: typeof p.enabled === "object" && p.enabled ? p.enabled : {},
      tokenOverrides: typeof p.tokenOverrides === "object" && p.tokenOverrides ? p.tokenOverrides : {},
    };
  } catch {
    return defaultState();
  }
}

export function saveState(s: DesignState): void {
  try {
    localStorage.setItem(LS_KEY, JSON.stringify(s));
  } catch {
    /* 隐私模式等：内存态继续可用 */
  }
}

export const designStore = createStore<DesignState>(loadState());

designStore.subscribe(() => saveState(designStore.getState()));

/** preset 族单选（再次点击同档 = 取消选择）。 */
export function selectPreset(entryId: string): void {
  const ent = findEntry(entryId);
  if (!ent || ent.kind !== "preset") return;
  designStore.setState((s) => ({
    selections: {
      ...s.selections,
      [ent.family]: s.selections[ent.family] === entryId ? null : entryId,
    },
  }));
}

/** switch 项启停（reserved 项只允许打开「占位开关」，行为仍冻结）。 */
export function toggleSwitch(entryId: string): void {
  const ent = findEntry(entryId);
  if (!ent || ent.kind === "preset") return;
  designStore.setState((s) => {
    const enabled = { ...s.enabled };
    if (enabled[entryId]) delete enabled[entryId];
    else enabled[entryId] = true;
    return { enabled };
  });
}

export function resetFamily(familyId: string): void {
  designStore.setState((s) => {
    const selections = { ...s.selections };
    delete selections[familyId];
    const enabled = { ...s.enabled };
    for (const ent of FAMILIES.find((f) => f.id === familyId)?.entries ?? []) {
      delete enabled[ent.id];
    }
    return { selections, enabled };
  });
}

export function resetAll(): void {
  designStore.setState(defaultState());
}

/** 个性档案导出（F01800 等）：JSON 字符串。 */
export function exportProfile(): string {
  return JSON.stringify({ format: "variable-design", version: 1, ...designStore.getState() }, null, 2);
}

/** 个性档案导入：校验 format/version 后整体替换。 */
export function importProfile(json: string): { ok: boolean; error?: string } {
  try {
    const p = JSON.parse(json) as Record<string, unknown>;
    if (p.format !== "variable-design" || p.version !== 1) return { ok: false, error: "format/version mismatch" };
    designStore.setState({
      selections: (p.selections as DesignState["selections"]) ?? {},
      enabled: (p.enabled as DesignState["enabled"]) ?? {},
      tokenOverrides: (p.tokenOverrides as DesignState["tokenOverrides"]) ?? {},
    });
    return { ok: true };
  } catch {
    return { ok: false, error: "invalid json" };
  }
}

/* ---------------- 选择器 ---------------- */

export function useDesignState(): DesignState {
  return useStore(designStore, (s) => s);
}

/** 汇总：当前生效的 preset 行 + 开启的 switch 行。 */
export function activeEntries(s: DesignState = designStore.getState()): DesignEntry[] {
  const out: DesignEntry[] = [];
  for (const f of FAMILIES) {
    const sel = s.selections[f.id];
    if (sel) {
      const ent = findEntry(sel);
      if (ent) out.push(ent);
    }
    for (const ent of f.entries) {
      if (ent.kind !== "preset" && s.enabled[ent.id]) out.push(ent);
    }
  }
  return out;
}

/** 写入令牌调试面板的实时覆写（F01646）。 */
export function setTokenOverride(name: string, value: string): void {
  designStore.setState((s) => ({ tokenOverrides: { ...s.tokenOverrides, [name]: value } }));
}

export function clearTokenOverride(name: string): void {
  designStore.setState((s) => {
    const o = { ...s.tokenOverrides };
    delete o[name];
    return { tokenOverrides: o };
  });
}
