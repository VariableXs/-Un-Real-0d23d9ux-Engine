/**
 * V-07 桌面敲字定位（纯状态机，供 vitest 与 DesktopIcons 共用）。
 * - 可打印字符（无 Ctrl/Alt/Meta 修饰）在 300ms 窗口内累积为查询串；
 * - 匹配由组件层用 matchPinyin（src/lib/pinyin.ts）完成，本模块只管缓冲；
 * - 焦点不在桌面时不产生键盘事件 → 天然零开销。
 */
export const TYPE_WINDOW_MS = 300;
/** 缓冲上限：防超长敲字拖垮匹配。 */
export const TYPE_BUFFER_MAX = 32;

export interface TypingState {
  buffer: string;
  lastAt: number;
}

/** 判断是否为「裸可打印字符」：单字符且无 ctrl/alt/meta 修饰。 */
export function isPrintableKey(key: string, ctrl: boolean, alt: boolean, meta: boolean): boolean {
  return key.length === 1 && !ctrl && !alt && !meta;
}

/**
 * 累积敲字：与上次敲字间隔超过 windowMs → 重新开始；否则追加。
 * 缓冲超上限时从尾部截断（保留最近输入）。
 */
export function collectTyping(prev: TypingState | null, key: string, now: number, windowMs: number = TYPE_WINDOW_MS): TypingState {
  const fresh = !prev || now - prev.lastAt > windowMs;
  const buffer = fresh ? key : (prev!.buffer + key).slice(-TYPE_BUFFER_MAX);
  return { buffer, lastAt: now };
}

/** 当前查询串（= 缓冲内容）。 */
export function typingQuery(state: TypingState | null): string {
  return state?.buffer ?? "";
}