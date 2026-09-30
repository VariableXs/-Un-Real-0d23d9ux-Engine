/**
 * N-09 内置组件共用工具：刷新 gate 钩子 + 待办本地存储（供第三方 todo:read 只读）。
 */
import { useEffect } from "react";

/** paused 时暂停轮询；恢复时立即补一次刷新。 */
export function useRefresh(fn: () => void, intervalMs: number, paused: boolean): void {
  useEffect(() => {
    if (paused) return;
    fn();
    const id = window.setInterval(fn, intervalMs);
    return () => window.clearInterval(id);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [paused, intervalMs]);
}

export interface WgtTodoItem {
  id: number;
  text: string;
  done: boolean;
}

export const TODOS_LS_KEY = "variable:widgets:todos:v1";

export function loadTodos(): WgtTodoItem[] {
  try {
    const raw = localStorage.getItem(TODOS_LS_KEY);
    const arr = raw ? (JSON.parse(raw) as WgtTodoItem[]) : [];
    return Array.isArray(arr) ? arr : [];
  } catch {
    return [];
  }
}

export function saveTodos(items: WgtTodoItem[]): void {
  try {
    localStorage.setItem(TODOS_LS_KEY, JSON.stringify(items.slice(0, 100)));
  } catch {
    /* storage full */
  }
}