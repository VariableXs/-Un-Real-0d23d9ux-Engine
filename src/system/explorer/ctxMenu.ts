/**
 * AI-09 M-19：文件管理器右键菜单注册表。
 * - 项集合固定（只提供注册表内安全动作，不支持任意 shell 命令——安全红线）
 * - 用户可显隐 / 排序（localStorage 持久），「恢复默认」一键还原
 * - ExplorerWindow 构建行菜单时按此配置过滤/排序
 */

export type CtxItemId =
  | "open"
  | "fav"
  | "copy"
  | "cut"
  | "sendto"
  | "wholocks"
  | "rename"
  | "delete"
  | "purge"
  | "shred"
  | "share"
  | "reveal";

/** 出厂顺序（与批次C/E 行为一致）。 */
export const CTX_DEFAULT_ORDER: CtxItemId[] = [
  "open",
  "fav",
  "copy",
  "cut",
  "sendto",
  "wholocks",
  "rename",
  "delete",
  "purge",
  "shred",
  "share",
  "reveal",
];

export interface CtxItemDef {
  id: CtxItemId;
  /** 词典 key（显示名）。 */
  labelKey: string;
  /** 适用范围：file / dir / 通用。 */
  scope: "all" | "file" | "dir";
}

/** 注册表（M-19：只提供注册表内安全动作）。 */
export const CTX_ITEMS: Record<CtxItemId, CtxItemDef> = {
  open: { id: "open", labelKey: "exOpen", scope: "all" },
  fav: { id: "fav", labelKey: "exFavAdd", scope: "dir" },
  copy: { id: "copy", labelKey: "exCopyAction", scope: "all" },
  cut: { id: "cut", labelKey: "exCut", scope: "all" },
  sendto: { id: "sendto", labelKey: "exSendTo", scope: "all" },
  wholocks: { id: "wholocks", labelKey: "exWhoLocks", scope: "file" },
  rename: { id: "rename", labelKey: "exRename", scope: "all" },
  delete: { id: "delete", labelKey: "exDelete", scope: "all" },
  purge: { id: "purge", labelKey: "exPurgeAction", scope: "all" },
  shred: { id: "shred", labelKey: "shredTitle", scope: "file" },
  share: { id: "share", labelKey: "xfShare", scope: "file" },
  reveal: { id: "reveal", labelKey: "showInExplorer", scope: "all" },
};

export interface CtxConfig {
  /** 显示顺序（含隐藏项，便于恢复排序记忆）。 */
  order: CtxItemId[];
  /** 隐藏项集合。 */
  hidden: CtxItemId[];
}

const KEY = "variable:explorer:ctxmenu:v1";

export function loadCtxConfig(): CtxConfig {
  try {
    const raw = JSON.parse(localStorage.getItem(KEY) ?? "null") as CtxConfig | null;
    if (!raw || !Array.isArray(raw.order) || !Array.isArray(raw.hidden)) return defaultCtxConfig();
    // 防御：未来新增项自动补尾，删除项剔除
    const known = new Set(CTX_DEFAULT_ORDER);
    const order = raw.order.filter((x) => known.has(x));
    for (const id of CTX_DEFAULT_ORDER) if (!order.includes(id)) order.push(id);
    return { order, hidden: raw.hidden.filter((x) => known.has(x)) };
  } catch {
    return defaultCtxConfig();
  }
}

export function saveCtxConfig(cfg: CtxConfig): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(cfg));
  } catch {
    /* storage blocked → 本次会话内生效但不持久 */
  }
}

export function defaultCtxConfig(): CtxConfig {
  return { order: [...CTX_DEFAULT_ORDER], hidden: [] };
}

export function resetCtxConfig(): CtxConfig {
  try {
    localStorage.removeItem(KEY);
  } catch {
    /* ignore */
  }
  return defaultCtxConfig();
}

// ---------- M-26：删除档位（默认 = 现状：环境回收站） ----------

export type DeleteTier = "variable" | "ask";

const TIER_KEY = "variable:explorer:deleteTier";

export function loadDeleteTier(): DeleteTier {
  try {
    const v = localStorage.getItem(TIER_KEY);
    return v === "ask" ? "ask" : "variable";
  } catch {
    return "variable";
  }
}

export function saveDeleteTier(tier: DeleteTier): void {
  try {
    localStorage.setItem(TIER_KEY, tier);
  } catch {
    /* ignore */
  }
}
