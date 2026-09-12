/**
 * AURORA-10000 领域04 · 族0092 剪贴板管理（AI-19 批次，勿删）。
 * 历史/置顶/收藏/去重/过期/容量/脱敏/拦截/审计/合并粘贴/导出。
 */

export type ClipKind = "text" | "image" | "file";

export interface ClipEntry {
  id: string;
  kind: ClipKind;
  text: string;
  ts: number;
  pinned: boolean;
  favorite: boolean;
  /** 敏感项（脱敏显示，F02296）。 */
  sensitive?: boolean;
  /** 读取审计（F02294）。 */
  readers?: string[];
}

/** 敏感探测：密码/卡号/身份证样式。 */
const SENSITIVE_RE = [
  /^(?:\d{16,19})$/, // 卡号
  /^\d{17}[\dXx]$/, // 身份证
  /^[!-~]{12,64}$/, // 高熵长串（保守：含大小写+数字+符号）
];

export function detectSensitive(text: string): boolean {
  const t = text.trim();
  if (/password|passwd|secret/i.test(t)) return true;
  if (/\d{16,19}/.test(t.replace(/\s/g, "")) && /^[\d\s]+$/.test(t)) return true;
  return SENSITIVE_RE.some((re) => re.test(t)) && /[a-z]/i.test(t) && /\d/.test(t);
}

/** 脱敏显示（F02296）。 */
export function mask(text: string): string {
  if (text.length <= 4) return "••••";
  return text.slice(0, 2) + "••••" + text.slice(-2);
}

/** 追加历史：去重合并（F02287）、容量与过期清理（F02285/86）。 */
export function pushClip(
  history: readonly ClipEntry[], entry: Omit<ClipEntry, "id" | "ts" | "pinned" | "favorite">,
  opts: { capacity: number; expireDays: number; dedupe: boolean; autoSensitive: boolean; now?: number },
): ClipEntry[] {
  const now = opts.now ?? Date.now();
  const withMeta: ClipEntry = {
    ...entry,
    id: `clip-${now}-${Math.random().toString(36).slice(2, 8)}`,
    ts: now, pinned: false, favorite: false,
    sensitive: opts.autoSensitive ? detectSensitive(entry.text) : entry.sensitive,
  };
  let list: ClipEntry[];
  if (opts.dedupe) {
    const prev = history.find((c) => c.text === entry.text && c.kind === entry.kind);
    list = prev
      ? history.map((c) => (c.id === prev.id ? { ...c, ts: now } : c))
      : [withMeta, ...history];
  } else {
    list = [withMeta, ...history];
  }
  const expireAt = now - opts.expireDays * 86_400_000;
  list = list.filter((c) => c.pinned || c.favorite || c.ts >= expireAt);
  const keep = list.filter((c) => c.pinned || c.favorite);
  const rest = list.filter((c) => !c.pinned && !c.favorite).slice(0, Math.max(0, opts.capacity - keep.length));
  return [...keep, ...rest].sort((a, b) => b.ts - a.ts);
}

/** 类型过滤（F02279）。 */
export function filterKind(list: readonly ClipEntry[], kind: ClipKind | "all"): ClipEntry[] {
  return kind === "all" ? [...list] : list.filter((c) => c.kind === kind);
}

/** 搜索（F02278）。 */
export function searchClips(list: readonly ClipEntry[], q: string): ClipEntry[] {
  const s = q.trim().toLowerCase();
  return s ? list.filter((c) => c.text.toLowerCase().includes(s)) : [...list];
}

/** 合并粘贴（F02288）：按序拼接选中项文本。 */
export function mergePaste(list: readonly ClipEntry[], sep = "\n"): string {
  return list.filter((c) => c.kind === "text").map((c) => c.text).join(sep);
}

/** 纯文本粘贴策略（F02289/90）：plain 时剥富文本标记。 */
export function pasteStrategy(text: string, plain: boolean): string {
  return plain ? text.replace(/<[^>]+>/g, "") : text;
}

/** 读取审计登记（F02294）：谁读了哪条。 */
export function recordReader(list: readonly ClipEntry[], id: string, reader: string): ClipEntry[] {
  return list.map((c) => (c.id === id ? { ...c, readers: [...(c.readers ?? []), reader] } : c));
}

/** 导出备份（F02298）。 */
export function exportClips(list: readonly ClipEntry[]): string {
  return JSON.stringify(list.map((c) => ({ ...c, text: c.sensitive ? mask(c.text) : c.text })), null, 2);
}
