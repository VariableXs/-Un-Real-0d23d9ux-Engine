/**
 * N-08 应用主题的常驻同步：
 * - 应用 = 三变体整体写入 localStorage（variable:theme-studio:applied:v1），
 *   并把当前场景（documentElement[data-theme] 信号）对应变体的 token 以内联
 *   变量写到 documentElement（内联优先级高于 tokens.css 亮度层）。
 * - data-theme 变化（settings 切换亮/暗/高对比）→ MutationObserver 自动换用
 *   对应变体（验收③「高对比变体随系统信号正确切换」）。
 * - 由 src/system/widgets/bus.ts 在桌面环境启动时初始化（车道 E 自挂载，不改 DesktopShell）。
 */
import {
  applyVariantToDOM, clearVariantFromDOM, currentVariant, type VariantKind,
} from "./tokens";
import { validateVtheme, type VthemeFile, type VthemeVariant } from "./vtheme";

export const APPLIED_LS_KEY = "variable:theme-studio:applied:v1";

export function loadAppliedVtheme(): VthemeFile | null {
  try {
    const raw = localStorage.getItem(APPLIED_LS_KEY);
    if (!raw) return null;
    const r = validateVtheme(JSON.parse(raw) as unknown);
    return r.ok && r.data ? r.data : null;
  } catch {
    return null;
  }
}

export function storeAppliedVtheme(file: VthemeFile): void {
  try {
    localStorage.setItem(APPLIED_LS_KEY, JSON.stringify(file));
  } catch {
    /* storage full —— 会话内仍然生效 */
  }
}

export function clearAppliedVtheme(): void {
  try {
    localStorage.removeItem(APPLIED_LS_KEY);
  } catch {
    /* ignore */
  }
  clearVariantFromDOM();
}

function variantFor(file: VthemeFile, kind: VariantKind): VthemeVariant | null {
  return file.variants[kind] ?? file.variants.dark ?? null;
}

/** 立即应用当前场景变体（工坊「应用」与启动同步共用）。 */
export function applyVthemeNow(file: VthemeFile): void {
  const variant = variantFor(file, currentVariant());
  if (variant) applyVariantToDOM(variant.colors, variant.shape);
}

let started = false;

export function initAppliedThemeSync(): void {
  if (started || typeof document === "undefined") return;
  started = true;
  const reapply = (): void => {
    const file = loadAppliedVtheme();
    if (file) applyVthemeNow(file);
  };
  // data-theme 变化 → 换变体；工坊取消应用（localStorage 清空）→ 下次信号变化自然不再写。
  new MutationObserver(reapply).observe(document.documentElement, {
    attributes: true,
    attributeFilter: ["data-theme"],
  });
  reapply();
}