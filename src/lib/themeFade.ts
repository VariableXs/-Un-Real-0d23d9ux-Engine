/**
 * AI-17 · Z-68 主题切换零闪白（Theme Cross-Fade）
 * ① 切换时先在离屏准备新主题根样式快照（requestAnimationFrame 双帧提交）；
 * ② 双层交叉淡入 170ms（--dur-3 动效令牌，interactions.css .theme-fade-root）；
 * ③ 壁纸层不参与淡入（.theme-fade-wall 由调用方排除）；
 * ④ 连续快速切换防抖：只应用最后一次（debounce 50ms 窗口）。
 */

let pending: ReturnType<typeof setTimeout> | null = null;
let lastTheme: string | null = null;

export interface ThemeFadeOptions {
  /** 应用主题到 DOM 的回调（通常是 document.documentElement.dataset.theme = t） */
  apply: (theme: string) => void;
  /** 交叉淡入动画宿主元素（默认 documentElement） */
  host?: HTMLElement | null;
  /** 防抖窗口 ms（默认 50，最后一次为准） */
  debounceMs?: number;
}

/**
 * 主题交叉淡入入口。连续调用时只有最后一次在防抖窗口后真正执行。
 * 返回取消函数（测试用）。
 */
export function crossFadeTheme(theme: string, opts: ThemeFadeOptions): () => void {
  if (pending !== null) clearTimeout(pending);
  lastTheme = theme;
  pending = setTimeout(() => {
    pending = null;
    if (theme !== lastTheme) return; // 已有更新的切换请求，放弃本帧
    opts.apply(theme);
    // 双帧后加淡入动画类：新主题首帧已绘制，交叉期不露白/黑
    const host = opts.host ?? (typeof document !== "undefined" ? document.documentElement : null);
    if (host && typeof requestAnimationFrame === "function") {
      requestAnimationFrame(() => {
        requestAnimationFrame(() => {
          host.classList.add("theme-fade-root");
          const clear = (): void => {
            host.classList.remove("theme-fade-root");
            host.removeEventListener("animationend", clear);
          };
          host.addEventListener("animationend", clear);
          // 兜底：动画事件丢失也按时长清理
          setTimeout(clear, 400);
        });
      });
    }
  }, opts.debounceMs ?? 50);
  return () => {
    if (pending !== null) {
      clearTimeout(pending);
      pending = null;
    }
  };
}

/** 测试辅助：重置内部状态。 */
export function resetThemeFade(): void {
  if (pending !== null) {
    clearTimeout(pending);
    pending = null;
  }
  lastTheme = null;
}
