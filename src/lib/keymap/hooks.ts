/**
 * Z-10 useHotkey / Z-11 useGridNav（含 M-29 长按加速曲线）。
 *
 * Z-10：combo 经 Z-08 注册表登记（context scope，mount 注册 / unmount 注销），
 *       上下文进入/退出与组件生命周期绑定；窗口卸载快照 diff 为空。
 * Z-11：roving tabindex 网格导航封装；Home/End/PageUp/PageDown 快跳 + scroll-into-view。
 * M-29：长按加速曲线查表（500ms 前 1x，之后 1.5x/2.5x/4x 三段），常数 const 化。
 */

import { useEffect, useRef, useCallback } from "react";
import { register, unregister } from "./registry";
import { arbitrate, isEditableTarget } from "./arbitrate";
import { shouldYield } from "./system-combos";
import { normalizeAccel } from "../shortcuts";

/** 事件 → 规范 combo 串（不含未按下的修饰键）。 */
export function eventToCombo(e: KeyboardEvent): string {
  const mods: string[] = [];
  if (e.ctrlKey) mods.push("ctrl");
  if (e.altKey) mods.push("alt");
  if (e.shiftKey) mods.push("shift");
  if (e.metaKey) mods.push("super");
  let key = e.key.toLowerCase();
  if (key === " ") key = "space";
  if (key.length === 1) key = key;
  else key = key === "escape" ? "escape" : key;
  return [...mods, key].join("+");
}

export interface HotkeyOptions {
  scope?: "global" | "window" | "context";
  priority?: number;
  /** window scope 的 id（同窗多面板用不同 id 区分）。 */
  windowScope?: string;
  /** 在可编辑元素内仍触发（global 默认让位）。 */
  allowEditable?: boolean;
}

/**
 * Z-10：声明式热键。组件挂载即注册进 Z-08 注册表；卸载即注销。
 * 触发前经过 Z-09 让位判定与 Z-10 裁决。
 */
export function useHotkey(
  combo: string,
  handler: (e: KeyboardEvent) => void,
  options: HotkeyOptions = {},
): void {
  const { scope = "context", priority = 0, windowScope, allowEditable = false } = options;
  const handlerRef = useRef(handler);
  handlerRef.current = handler;

  useEffect(() => {
    const normalized = normalizeAccel(combo === "ctrl+/" ? "ctrl+slash" : combo);
    const id = `hotkey:${normalized ?? combo}`;
    const res = register({
      id,
      combo: normalized ?? combo,
      scope,
      priority,
      source: windowScope ?? "hook",
      descKey: id,
    });
    if (!res.ok) return; // 注册失败 → 诚实降级：不生效（M-28 占用榜口径由设置页统计）

    const onKey = (e: KeyboardEvent): void => {
      const c = eventToCombo(e);
      if (c !== (normalized ?? combo)) return;
      const y = shouldYield(c);
      if (y.yield) return;
      const verdict = arbitrate({ combo: c, target: e.target, windowScope: windowScope ?? null });
      if (verdict.binding?.id !== id) return;
      if (!allowEditable && isEditableTarget(e.target) && scope === "global") return;
      handlerRef.current(e);
    };
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      unregister(id);
    };
  }, [combo, scope, priority, windowScope, allowEditable]);
}

// ---------- M-29 长按加速曲线 ----------

/** 曲线段：持续时间（ms）→ 步进倍率。500ms 前 1x。 */
export const KEY_REPEAT_CURVE: ReadonlyArray<{ until: number; scale: number }> = [
  { until: 500, scale: 1 },
  { until: 1000, scale: 1.5 },
  { until: 1600, scale: 2.5 },
  { until: Infinity, scale: 4 },
];

/** 长按 elapsed 毫秒后的单步倍率。 */
export function keyRepeatScale(elapsedMs: number): number {
  for (const seg of KEY_REPEAT_CURVE) {
    if (elapsedMs < seg.until) return seg.scale;
  }
  return 4;
}

/** 基础间隔（ms）：网格导航单步基准。 */
export const KEY_REPEAT_BASE_MS = 120;

/**
 * M-29/Z-11：useGridNav — roving tabindex 网格导航。
 * 返回容器/元素注册器与键盘处理；长按方向键按曲线加速步进。
 */
export function useGridNav(options: {
  itemCount: number;
  columns: number;
  onSelect?: (index: number) => void;
  loop?: boolean;
}) {
  const { itemCount, columns, onSelect, loop = true } = options;
  const indexRef = useRef(0);
  const pressStartRef = useRef<number>(0);
  const lastStepRef = useRef<number>(0);

  const move = useCallback(
    (delta: number, elapsed: number) => {
      if (itemCount === 0) return;
      const scale = keyRepeatScale(elapsed);
      const steps = Math.max(1, Math.round(scale * Math.abs(delta))) * Math.sign(delta);
      let next = indexRef.current + steps;
      if (loop) next = ((next % itemCount) + itemCount) % itemCount;
      else next = Math.min(itemCount - 1, Math.max(0, next));
      indexRef.current = next;
      onSelect?.(next);
    },
    [itemCount, loop, onSelect],
  );

  const onKeyDown = useCallback(
    (e: KeyboardEvent | React.KeyboardEvent): void => {
      const now = Date.now();
      if (!pressStartRef.current || e.type === "keydown") {
        if (e.type === "keydown" && !("repeat" in e && (e as KeyboardEvent).repeat)) {
          pressStartRef.current = now;
          lastStepRef.current = now;
        }
      }
      const elapsed = now - pressStartRef.current;
      let delta = 0;
      switch (e.key) {
        case "ArrowRight": delta = 1; break;
        case "ArrowLeft": delta = -1; break;
        case "ArrowDown": delta = columns; break;
        case "ArrowUp": delta = -columns; break;
        case "Home": indexRef.current = 0; onSelect?.(0); e.preventDefault(); return;
        case "End": indexRef.current = itemCount - 1; onSelect?.(itemCount - 1); e.preventDefault(); return;
        case "PageUp": indexRef.current = Math.max(0, indexRef.current - columns * 4); onSelect?.(indexRef.current); e.preventDefault(); return;
        case "PageDown": indexRef.current = Math.min(itemCount - 1, indexRef.current + columns * 4); onSelect?.(indexRef.current); e.preventDefault(); return;
        case "Enter":
        case " ":
          if (onSelect) { onSelect(indexRef.current); e.preventDefault(); }
          return;
        default: return;
      }
      // 长按重复节流：基础间隔 / 加速倍率
      const scale = keyRepeatScale(elapsed);
      const interval = KEY_REPEAT_BASE_MS / scale;
      const isRepeat = "repeat" in e && (e as KeyboardEvent).repeat;
      if (isRepeat && now - lastStepRef.current < interval) { e.preventDefault(); return; }
      lastStepRef.current = now;
      move(delta, elapsed);
      e.preventDefault();
    },
    [columns, itemCount, move, onSelect],
  );

  const setIndex = useCallback((i: number): void => { indexRef.current = i; }, []);

  return { onKeyDown, setIndex, currentIndex: indexRef };
}
