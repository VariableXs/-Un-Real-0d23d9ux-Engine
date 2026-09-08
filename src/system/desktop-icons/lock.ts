/**
 * V-03 桌面图标锁定（纯状态机，供 vitest 与 DesktopIcons 共用）。
 *
 * 语义（规格 V-03）：锁定 = 只锁「位置与删除」，防手滑而非安全功能。
 * - 拦截：拖动（drag）、删除/移除（delete）、剪切（cut）、重排（reorder =
 *   排序/自动排列/归架/出架/剪切粘贴等改变位置的操作）；
 * - 放行：新建（create）、重命名（rename）、复制（copy）——与 Windows 一致。
 */
export type LockAction =
  | "drag"
  | "delete"
  | "cut"
  | "reorder"
  | "create"
  | "rename"
  | "copy";

/** 锁定期间被拦截的动作集合。 */
const BLOCKED: readonly LockAction[] = ["drag", "delete", "cut", "reorder"];

/** 判断某动作在当前锁定态下是否应被拦截。未锁定时一律放行。 */
export function isBlocked(action: LockAction, locked: boolean): boolean {
  return locked && BLOCKED.includes(action);
}