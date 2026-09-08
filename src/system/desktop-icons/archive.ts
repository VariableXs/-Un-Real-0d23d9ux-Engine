/**
 * V-96 桌面归档：图标过多时建议归档进文件架（复用 moveToShelf 语义，
 * 天然可逆、零文件操作；架在桌面即「一键可达」）。
 * - 阈值 localStorage 可调（variable:desktop:archive:threshold，默认 80）；
 * - 提示节流：30 天内不重复提示（variable:desktop:archive:lastHint）；
 * - 候选排除系统图标（sys-*：此电脑/回收站等不入架，与 moveToShelf 一致）。
 */
const LS_THRESHOLD = "variable:desktop:archive:threshold";
const LS_LASTHINT = "variable:desktop:archive:lastHint";

export const DEFAULT_ARCHIVE_THRESHOLD = 80;
/** 30 天（ms）。 */
export const ARCHIVE_HINT_COOLDOWN_MS = 30 * 24 * 60 * 60 * 1000;

export function archiveThreshold(): number {
  try {
    const n = Number(localStorage.getItem(LS_THRESHOLD));
    return Number.isFinite(n) && n > 0 ? Math.floor(n) : DEFAULT_ARCHIVE_THRESHOLD;
  } catch {
    return DEFAULT_ARCHIVE_THRESHOLD;
  }
}

export function setArchiveThreshold(n: number): void {
  try {
    localStorage.setItem(LS_THRESHOLD, String(Math.max(1, Math.floor(n))));
  } catch {
    /* storage blocked → 不持久化 */
  }
}

/** 上次提示时间（ms）；从未提示过返回 0。 */
export function lastArchiveHint(): number {
  try {
    const n = Number(localStorage.getItem(LS_LASTHINT));
    return Number.isFinite(n) ? n : 0;
  } catch {
    return 0;
  }
}

/**
 * 是否应当提示归档：可见图标数（排除系统图标后由调用方传入）超过阈值，
 * 且距上次提示超过 30 天冷却期。
 */
export function shouldSuggestArchive(visibleCount: number, now: number, threshold: number = archiveThreshold()): boolean {
  if (visibleCount <= threshold) return false;
  return now - lastArchiveHint() > ARCHIVE_HINT_COOLDOWN_MS;
}

/** 记录「本次已提示」，进入 30 天冷却。 */
export function markArchiveHinted(now: number): void {
  try {
    localStorage.setItem(LS_LASTHINT, String(now));
  } catch {
    /* storage blocked → 不持久化 */
  }
}

export interface ArchiveCandidate {
  id: string;
}

/** 归档候选：排除系统图标（sys-* 前缀），其余（软件/第三方/文件架）皆可入架。 */
export function archiveCandidates<T extends ArchiveCandidate>(defs: T[]): T[] {
  return defs.filter((d) => !d.id.startsWith("sys-"));
}

/** 归档架名：桌面归档 YYYY-MM（en：Desktop archive YYYY-MM）。 */
export function archiveShelfName(d: Date, lang: "zh" | "en" = "zh"): string {
  const mm = String(d.getMonth() + 1).padStart(2, "0");
  const ym = `${d.getFullYear()}-${mm}`;
  return lang === "en" ? `Desktop archive ${ym}` : `桌面归档 ${ym}`;
}