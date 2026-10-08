/**
 * AURORA-10000 · AI-11~AI-15 车道 · 运行时。
 * - applyDesignState：把生效 preset/switch 的 vars + 令牌调试覆写写入 <html>
 *   的 style（--w2-* 令牌 + [data-w2-theme] 主题覆盖），移除时无残留；
 * - wallpaper 引擎档（族0056）经既有 ai04:wallpaper-apply 事件驱动真实引擎；
 * - 剧场/仪式/健康/光标层由 runners.ts 消费 params。
 * 纯 DOM 写入集中在此文件，其余模块保持纯数据/纯函数。
 */
import { activeEntries, designStore } from "./state";
import type { DesignEntry } from "./types";

const THEME_ATTR = "data-w2-theme";

const THEME_TOKENS = new Set([
  "--bg-canvas",
  "--bg-surface",
  "--bg-raised",
  "--text-primary",
  "--text-secondary",
  "--accent",
]);

function applyVars(el: HTMLElement, vars: Record<string, string>): void {
  for (const [k, v] of Object.entries(vars)) {
    if (THEME_TOKENS.has(k)) {
      el.setAttribute(THEME_ATTR, "on");
      el.style.setProperty(k, v);
    } else {
      el.style.setProperty(k, v);
    }
  }
}

/** 汇总当前生效行 → 写入根节点；返回写入的令牌名（测试/调试可见）。 */
export function applyDesignState(root: HTMLElement | null = typeof document !== "undefined" ? document.documentElement : null): string[] {
  const applied: string[] = [];
  if (!root) return applied;
  const s = designStore.getState();
  const entries = activeEntries(s);

  // 1) 覆写类令牌（调试面板 F01646）
  for (const [k, v] of Object.entries(s.tokenOverrides)) {
    root.style.setProperty(k, v);
    applied.push(k);
  }

  // 2) 生效行
  for (const ent of entries) {
    if (ent.vars) {
      applyVars(root, ent.vars);
      applied.push(...Object.keys(ent.vars));
    }
  }

  // 3) 主题归零：无主题档生效时摘掉覆盖（既有主题恢复）
  const hasTheme = entries.some((e) => Object.keys(e.vars ?? {}).some((k) => THEME_TOKENS.has(k)))
    || Object.keys(s.tokenOverrides).some((k) => THEME_TOKENS.has(k));
  if (!hasTheme) root.removeAttribute(THEME_ATTR);
  return applied;
}

/** 壁纸引擎档：选中后驱动既有壁纸引擎（族0056）。 */
export function applyEngineEntry(ent: DesignEntry): boolean {
  const kind = ent.params?.engineKind;
  if (typeof kind !== "string") return false;
  if (typeof window === "undefined") return false;
  window.dispatchEvent(
    new CustomEvent("ai04:wallpaper-apply", {
      detail: { patch: { engine: kind, params: ent.params ?? {} }, source: "aurora-w2" },
    }),
  );
  return true;
}

/** 启动副作用：状态变化即重放令牌。幂等，activate.ts 调用一次。 */
export function initDesignRuntime(): void {
  applyDesignState();
  designStore.subscribe(() => applyDesignState());
}
