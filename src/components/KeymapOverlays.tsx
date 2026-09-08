/**
 * AI-05 键位纪律组 — 浮层三件套 + 全局 Esc 消费。
 *
 * Z-12 上下文键位速查浮层：Ctrl+/ 呼出（自身经 Z-08 注册，自举验证）；
 *      实时读注册表激活键位；毛玻璃样式，不抢焦点。
 * Z-13 命令提示条：注册表派生，设置开关（默认开）；容器高度 > 600px 才显示。
 * M-33 按键回显：监听 keycast://show 事件（仅功能组合，打字内容永不出现）。
 * M-34 全局 Esc：栈顶消费一层；双击 Esc 切环境由 kbdhook 独立判定（不动）。
 */

import { useEffect, useState } from "react";
import type { Settings } from "../lib/settings";
import { snapshot, register, unregister } from "../lib/keymap/registry";
import { arbitrate } from "../lib/keymap/arbitrate";
import { shouldYield } from "../lib/keymap/system-combos";
import { eventToCombo } from "../lib/keymap/hooks";
import { prettyAccel } from "../lib/shortcuts";
import { useI18n } from "../i18n";
import { consumeOverlayOnEsc } from "../state/uiStore";

/** M-34：全局 Esc handler——栈顶消费一层；栈空放行（不拦截）。 */
export function useEscOverlayStack(): void {
  useEffect(() => {
    const onKey = (e: KeyboardEvent): void => {
      if (e.key !== "Escape" || e.repeat) return;
      const closed = consumeOverlayOnEsc();
      if (closed !== null) {
        e.preventDefault();
        e.stopPropagation();
        window.dispatchEvent(new CustomEvent("variable:overlay-close", { detail: closed }));
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, []);
}

/** Z-12：速查浮层。Ctrl+/ 呼出；展示注册表当前激活键位（global/window）。 */
export function KeymapOverlay(): JSX.Element | null {
  const { lang } = useI18n();
  const [open, setOpen] = useState(false);
  const [tick, setTick] = useState(0);

  useEffect(() => {
    const id = "overlay:ctrl-slash";
    const res = register({
      id,
      combo: "ctrl+slash",
      scope: "global",
      priority: 10,
      source: "KeymapOverlay",
      descKey: "kmToggleOverlay",
    });
    if (!res.ok) return;
    const onKey = (e: KeyboardEvent): void => {
      if (eventToCombo(e) !== "ctrl+slash") return;
      if (shouldYield("ctrl+slash").yield) return;
      if (arbitrate({ combo: "ctrl+slash", target: e.target }).binding?.id !== id) return;
      e.preventDefault();
      setOpen((v) => !v);
      setTick((t) => t + 1);
    };
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      unregister(id);
    };
  }, []);

  if (!open) return null;
  const bindings = snapshot().filter((b) => b.scope !== "context");
  return (
    <div
      key={tick}
      style={{
        position: "fixed", inset: 0, zIndex: 9998,
        display: "flex", alignItems: "center", justifyContent: "center",
        background: "rgba(0,0,0,0.25)", backdropFilter: "blur(10px)",
        pointerEvents: "none",
      }}
      onClick={() => setOpen(false)}
    >
      <div
        style={{
          minWidth: 320, maxHeight: "60vh", overflow: "auto", borderRadius: 12, padding: 16,
          background: "rgba(28,32,44,0.72)", backdropFilter: "blur(24px)",
          border: "1px solid rgba(255,255,255,0.12)", color: "var(--fg, #e8eaf0)",
          boxShadow: "0 12px 40px rgba(0,0,0,0.4)",
        }}
      >
        <div style={{ fontWeight: 600, marginBottom: 8 }}>
          {lang !== "en" ? "可用键位（Ctrl+/ 关闭）" : "Active keys (Ctrl+/ to close)"}
        </div>
        {bindings.map((b) => (
          <div key={b.id} style={{ display: "flex", justifyContent: "space-between", gap: 24, padding: "3px 0", fontSize: 13 }}>
            <span className="dim">{b.descKey}</span>
            <code>{prettyAccel(b.combo, lang)}</code>
          </div>
        ))}
        {bindings.length === 0 && <div className="dim small">{lang !== "en" ? "暂无登记键位" : "No registered bindings"}</div>}
      </div>
    </div>
  );
}

/** Z-13：命令提示条（底部条，注册表派生；容器高度 ≤600px 不显示）。 */
export function CommandHintBar(props: { settings: Settings }): JSX.Element | null {
  const { lang } = useI18n();
  const [tall, setTall] = useState(false);
  const [items, setItems] = useState<{ id: string; combo: string; desc: string }[]>([]);

  useEffect(() => {
    const update = (): void => setTall(window.innerHeight > 600);
    update();
    window.addEventListener("resize", update);
    return () => window.removeEventListener("resize", update);
  }, []);

  useEffect(() => {
    const update = (): void => {
      setItems(
        snapshot()
          .filter((b) => b.scope !== "context")
          .slice(0, 8)
          .map((b) => ({ id: b.id, combo: b.combo, desc: b.descKey })),
      );
    };
    update();
    const timer = window.setInterval(update, 2000); // 注册表变更低频轮询（快照 diff）
    return () => window.clearInterval(timer);
  }, []);

  if (!props.settings.commandHintBar || !tall || items.length === 0) return null;
  return (
    <div
      style={{
        position: "fixed", left: 0, right: 0, bottom: 0, zIndex: 50,
        display: "flex", gap: 16, justifyContent: "center", alignItems: "center",
        height: 26, fontSize: 11, pointerEvents: "none",
        background: "rgba(0,0,0,0.18)", backdropFilter: "blur(8px)",
        color: "var(--fg-muted, #9aa0ae)", borderTop: "1px solid rgba(255,255,255,0.06)",
      }}
    >
      {items.map((it) => (
        <span key={it.id} style={{ whiteSpace: "nowrap" }}>
          <code>{prettyAccel(it.combo, lang)}</code> {it.desc}
        </span>
      ))}
    </div>
  );
}

export interface KeycastEntry {
  id: number;
  text: string;
  born: number;
}

/** M-33：按键回显浮层——仅展示 keycast://show 事件携带的功能组合。 */
export function KeycastOverlay(props: { settings: Settings }): JSX.Element | null {
  const [entries, setEntries] = useState<KeycastEntry[]>([]);

  useEffect(() => {
    if (!props.settings.keycast) return;
    let nextId = 1;
    const onShow = (e: Event): void => {
      const text = (e as CustomEvent<string>).detail;
      if (typeof text !== "string" || text === "") return;
      const id = nextId++;
      setEntries((prev) => [...prev.slice(-3), { id, text, born: Date.now() }]);
      window.setTimeout(() => {
        setEntries((prev) => prev.filter((x) => x.id !== id));
      }, 2000); // 2s 淡出
    };
    window.addEventListener("keycast://show", onShow);
    return () => window.removeEventListener("keycast://show", onShow);
  }, [props.settings.keycast]);

  if (!props.settings.keycast || entries.length === 0) return null;
  const corner = props.settings.keycastCorner;
  const pos: React.CSSProperties =
    corner === "tl" ? { top: 12, left: 12 } :
    corner === "tr" ? { top: 12, right: 12 } :
    corner === "bl" ? { bottom: 36, left: 12 } : { bottom: 36, right: 12 };
  return (
    <div style={{ position: "fixed", ...pos, zIndex: 9997, display: "flex", flexDirection: "column", gap: 4, alignItems: "flex-end", pointerEvents: "none" }}>
      {entries.map((en) => (
        <div
          key={en.id}
          style={{
            padding: "4px 10px", borderRadius: 8, fontSize: 12,
            background: "rgba(28,32,44,0.78)", color: "var(--fg, #e8eaf0)",
            border: "1px solid rgba(255,255,255,0.12)", backdropFilter: "blur(12px)",
            fontFamily: "ui-monospace, Consolas, monospace",
          }}
        >
          {en.text}
        </div>
      ))}
    </div>
  );
}
