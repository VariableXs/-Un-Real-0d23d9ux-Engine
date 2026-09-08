/**
 * N-10 锁屏纯逻辑（功能全景 L871-889）。
 *
 * - 口令仅本地哈希存储（WebCrypto SHA-256），明文永不落盘（L879 红线）；
 * - 口令错误 5 次起指数退避（backoffMs）；
 * - 恢复码 8 位数字，生成时仅显示一次，同样只存哈希（L886）；
 * - 通知聚合只输出「来源 + 数量」，绝不携带标题/正文（L878 隐私红线）。
 *
 * 诚实边界：应用层锁屏，不替代 Windows 锁屏；全局热键（Win+L）注册属
 * 后端 kbdhook 领地，本车道不做 —— 入口为菜单 / 快捷面板事件。
 */

export const FAIL_THRESHOLD = 5;
export const BACKOFF_BASE_MS = 30_000;
export const BACKOFF_MAX_MS = 30 * 60_000;

/**
 * 口令错误指数退避：第 5 次失败起 30s 翻倍，封顶 30 分钟。
 * backoffMs(4)=0, backoffMs(5)=30s, backoffMs(6)=60s …
 */
export function backoffMs(fails: number, base = BACKOFF_BASE_MS, max = BACKOFF_MAX_MS): number {
  if (!Number.isFinite(fails) || fails < FAIL_THRESHOLD) return 0;
  const exp = Math.floor(fails) - FAIL_THRESHOLD;
  return Math.min(max, base * 2 ** exp);
}

/** SHA-256 十六进制摘要（WebCrypto；Node ≥18 与浏览器均可用）。 */
export async function sha256Hex(text: string): Promise<string> {
  const data = new TextEncoder().encode(text);
  const digest = await crypto.subtle.digest("SHA-256", data);
  return Array.from(new Uint8Array(digest))
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}

/** 校验口令/恢复码：比对哈希（每次错误计入 fails，由调用方持久化）。 */
export async function verifySecret(input: string, hash: string): Promise<boolean> {
  return (await sha256Hex(input)) === hash;
}

/** 8 位数字恢复码（可注入随机源便于测试）。 */
export function randomRecoveryCode(rand: () => number = Math.random): string {
  let code = "";
  for (let i = 0; i < 8; i++) code += Math.floor(rand() * 10).toString();
  return code;
}

/* ---------------- 通知聚合（只来源 + 数量，红线：不带内容） ---------------- */

export interface LockNotifySource {
  /** 通知来源类别：privacy / hardware / system（对应 notifyStore.kind）。 */
  kind: string;
  count: number;
}

/**
 * 未读通知按来源聚合。输入刻意只取 kind/read 两个字段 ——
 * 类型层面就杜绝内容（title/body）进入锁屏渲染路径。
 */
export function aggregateByKind(items: ReadonlyArray<{ kind: string; read: boolean }>): LockNotifySource[] {
  const counts = new Map<string, number>();
  for (const it of items) {
    if (it.read) continue;
    counts.set(it.kind, (counts.get(it.kind) ?? 0) + 1);
  }
  return Array.from(counts, ([kind, count]) => ({ kind, count }));
}

/* ---------------- 本地持久化（localStorage 安全封装） ---------------- */

export const LS_MODE = "variable:lockscreen:mode";
export const LS_HASH = "variable:lockscreen:hash";
export const LS_RECOVERY_HASH = "variable:lockscreen:recovery-hash";
export const LS_FAILS = "variable:lockscreen:fails";
export const LS_CLOCK_SIZE = "variable:lockscreen:clock-size";
export const LS_FOCUS_UNTIL = "variable:lockscreen:focus-until";

export type LockMode = "real" | "ritual";

export function lsGet(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

export function lsSet(key: string, value: string): void {
  try {
    localStorage.setItem(key, value);
  } catch {
    /* 隐私模式等：锁屏状态退化为会话内，功能不崩 */
  }
}

/** 锁屏模式：variable:lockscreen:mode，默认 ritual（默认行为 = 现状）。 */
export function loadMode(): LockMode {
  return lsGet(LS_MODE) === "real" ? "real" : "ritual";
}

export function loadFails(): number {
  const n = Number(lsGet(LS_FAILS));
  return Number.isFinite(n) && n > 0 ? Math.floor(n) : 0;
}

export function clockSizeClass(): string {
  return `lockscr-clock-${lsGet(LS_CLOCK_SIZE) === "s" ? "s" : lsGet(LS_CLOCK_SIZE) === "l" ? "l" : "m"}`;
}