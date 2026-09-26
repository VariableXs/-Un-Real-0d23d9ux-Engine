/**
 * C 桌面体验域·后段 · AI-D2 运行时（F106 键盘提示 HUD / F107 输入法浮窗 /
 * F110 屏幕键盘 浮层总装）。
 *
 * 生效面声明（诚实边界，与 J1Runtime 同纪律）：
 * - 本运行时在 Varix 桌面窗口内挂载三枚浮层；全部配置经 d2Store 订阅，
 *   即时生效免重启（F303 哲学同源）。
 * - 锁定键切换检测走 `vx-d2-key` 镜像桥（KeymapOverlays 收编漏斗——只
 *   镜像不消费，零拦截）；OS 输入法真值在壳层，桌面环境经 `vx-d2-ime`
 *   CustomEvent 注入（{chinese}），浏览器环境浮窗即开关（点击态位切换）。
 * - 全屏降级：document.fullscreenElement 变化即转角标/角标浮窗（F106/F107
 *   判据的降级路径），恢复即还原。
 * - 屏幕键盘（F110）按键 → 向当前焦点元素派发合成 KeyboardEvent（域内
 *   真实生效）；物理键学习模式经镜像桥同步高亮；密码框安全键盘等系统级
 *   限制为壳层职责，域内如实标注。
 */

import React, { useCallback, useEffect, useRef, useState } from "react";
import { d2Store } from "./d2store";
import { KeyHud, lockIconShape, lockLabel, type HudContent } from "./keyhud";
import { ImeFloat, imeLabel, type FloatMode } from "./imefloat";
import { OnScreenKb, WINDOW_H_PX, WINDOW_W_PX, keyLabel, type VKey } from "./osk";
import { useHotkey } from "../../lib/keymap/hooks";
import { useKeyMirrorBridge } from "../../components/KeymapOverlays";

/* ------------------------------- store 订阅 ------------------------------- */

interface KeyHudCfg { enabled: boolean; cornerMode: boolean }
interface ImeCfg { mode: FloatMode; hideOnPassword: boolean }
interface OskCfg { full: boolean; opacity: number; learning: boolean; clickThrough: boolean; alwaysOnTop: boolean; visible: boolean }

function section<T>(name: string, fallback: T): T {
  const s = d2Store.get(name as never);
  return { ...(fallback as unknown as Record<string, unknown>), ...s } as unknown as T;
}

/* ------------------------------ F106 HUD 层 ------------------------------ */

type HudSnap = { content: HudContent | null; phase: "hidden" | "showing" | "holding" | "fading" };

function KeyHudLayer(): React.ReactElement | null {
  const cfg = section<KeyHudCfg>("keyhud", { enabled: true, cornerMode: false });
  const hudRef = useRef<KeyHud>(new KeyHud());
  const [snap, setSnap] = useState<HudSnap>({ content: null, phase: "hidden" });
  const [rect, setRect] = useState({ x: 0, y: 0, w: 0, h: 0 });

  // 锁定键切换：镜像桥状态差分（注册表不收裸锁定键——镜像桥为收编漏斗）。
  useEffect(() => {
    const prev = { caps: false, num: false, scroll: false, primed: false };
    const onMirror = (e: Event): void => {
      const d = (e as CustomEvent<{ capsLock: boolean; numLock: boolean; scrollLock: boolean }>).detail;
      if (!d) return;
      const now = performance.now();
      if (prev.primed) {
        if (d.capsLock !== prev.caps) hudRef.current.toggle("caps", d.capsLock, now);
        if (d.numLock !== prev.num) hudRef.current.toggle("num", d.numLock, now);
        if (d.scrollLock !== prev.scroll) hudRef.current.toggle("scroll", d.scrollLock, now);
      }
      prev.caps = d.capsLock;
      prev.num = d.numLock;
      prev.scroll = d.scrollLock;
      prev.primed = true;
    };
    // 输入法切换复用（壳层注入 {chinese}——F107 联动同组件）。
    const onIme = (e: Event): void => {
      const d = (e as CustomEvent<{ chinese?: boolean }>).detail;
      hudRef.current.imeSwitch(d?.chinese !== false, performance.now());
    };
    const onFs = (): void => hudRef.current.setFullscreen(document.fullscreenElement !== null);
    document.addEventListener("fullscreenchange", onFs);
    window.addEventListener("vx-d2-key", onMirror);
    window.addEventListener("vx-d2-ime", onIme);
    onFs();
    return () => {
      document.removeEventListener("fullscreenchange", onFs);
      window.removeEventListener("vx-d2-key", onMirror);
      window.removeEventListener("vx-d2-ime", onIme);
    };
  }, []);

  // 状态机推进（rAF——只在可见时跑，空闲零开销）。
  useEffect(() => {
    if (!cfg.enabled) return;
    let raf = 0;
    const loop = (): void => {
      const hud = hudRef.current;
      hud.tick(performance.now());
      const phase = hud.state.kind;
      setSnap((s) => (s.phase !== phase || s.content !== hud.content ? { content: hud.content, phase } : s));
      raf = requestAnimationFrame(loop);
    };
    raf = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(raf);
  }, [cfg.enabled]);

  useEffect(() => {
    hudRef.current.enabled = cfg.enabled;
    hudRef.current.setFullscreen(cfg.cornerMode || document.fullscreenElement !== null);
    setRect(hudRef.current.rect(window.innerWidth, window.innerHeight));
    const onResize = (): void => setRect(hudRef.current.rect(window.innerWidth, window.innerHeight));
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, [cfg.enabled, cfg.cornerMode]);

  if (!cfg.enabled || snap.phase === "hidden" || !snap.content) return null;
  const c = snap.content;
  const label = c.kind === "lock" ? lockLabel(c.key, c.on) : c.chinese ? "中文输入" : "英文输入";
  const shape = c.kind === "lock" ? lockIconShape(c.key) : "circle";
  const on = c.kind === "lock" ? c.on : c.chinese;
  const corner = cfg.cornerMode || document.fullscreenElement !== null;
  return (
    <div
      className={`d2-keyhud${corner ? " d2-keyhud--corner" : ""}${snap.phase === "showing" ? " d2-keyhud--in" : ""}${snap.phase === "fading" ? " d2-keyhud--out" : ""}`}
      style={corner ? { right: 16, bottom: 16 } : { left: rect.x, width: rect.w }}
      role="status"
      aria-live="polite"
    >
      <span className={`d2-keyhud-shape d2-keyhud-shape--${shape}${on ? "" : " d2-keyhud-shape--off"}`} aria-hidden />
      <span className="d2-keyhud-text">{label}</span>
    </div>
  );
}

/* ---------------------------- F107 输入法浮窗 ---------------------------- */

function ImeFloatLayer(): React.ReactElement | null {
  const cfg = section<ImeCfg>("imefloat", { mode: "follow", hideOnPassword: true });
  const fRef = useRef<ImeFloat>(new ImeFloat());
  const [, force] = useState(0);
  const [anchor, setAnchor] = useState<{ x: number; y: number } | null>(null);

  useEffect(() => {
    const f = fRef.current;
    const rerender = (): void => force((v) => v + 1);
    // 光标锚：焦点元素底缘（域内近似锚——精确插入点由壳层经 vx-d2-caret 注入）。
    const reanchor = (): void => {
      const el = document.activeElement as HTMLElement | null;
      if (!el || el === document.body) return;
      const editable = el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement || el.isContentEditable;
      if (!editable) return;
      const r = el.getBoundingClientRect();
      setAnchor({ x: r.left, y: r.bottom });
    };
    const onFocusIn = (): void => {
      const el = document.activeElement as HTMLElement | null;
      const isPassword = el instanceof HTMLInputElement && el.type === "password";
      if (cfg.hideOnPassword) f.setPasswordFocus(isPassword);
      reanchor();
      rerender();
    };
    const onCaret = (e: Event): void => {
      const d = (e as CustomEvent<{ x: number; y: number }>).detail;
      if (d && typeof d.x === "number") {
        f.cursorMoved(d.x, d.y, performance.now());
        setAnchor({ x: d.x, y: d.y });
        rerender();
      }
    };
    const onFs = (): void => {
      f.setFullscreen(document.fullscreenElement !== null);
      rerender();
    };
    document.addEventListener("focusin", onFocusIn);
    document.addEventListener("selectionchange", reanchor);
    document.addEventListener("fullscreenchange", onFs);
    window.addEventListener("vx-d2-caret", onCaret);
    onFs();
    return () => {
      document.removeEventListener("focusin", onFocusIn);
      document.removeEventListener("selectionchange", reanchor);
      document.removeEventListener("fullscreenchange", onFs);
      window.removeEventListener("vx-d2-caret", onCaret);
    };
  }, [cfg.hideOnPassword]);

  const f = fRef.current;
  f.mode = cfg.mode;

  const toggleState = (which: 0 | 1 | 2): void => {
    f.toggleState(which);
    // 三态广播（term2 等消费面同源）。
    window.dispatchEvent(new CustomEvent("vx-d2-ime-state", { detail: { ...f.states } }));
    force((v) => v + 1);
  };

  if (cfg.mode === "hidden" || cfg.mode === "taskbarChip") {
    if (cfg.mode !== "taskbarChip") return null;
    return (
      <button type="button" className="d2-ime-chip" onClick={() => { f.collapse(false); force((v) => v + 1); }} aria-label="展开输入法状态浮窗">
        {f.states.chinese ? "中" : "英"}
      </button>
    );
  }
  if (cfg.mode === "cornerBadge" || document.fullscreenElement !== null) {
    return <div className="d2-ime-cornerbadge" aria-hidden>{f.states.chinese ? "中" : "英"}</div>;
  }

  const a = anchor ?? { x: Math.round(window.innerWidth / 2), y: Math.round(window.innerHeight / 2) };
  f.cursorMoved(a.x, a.y, f.appliedMoves === 0 ? 0 : performance.now()); // 首移必应用（D14 同源）
  const r = f.rect(window.innerWidth, window.innerHeight);
  const segs: Array<{ which: 0 | 1 | 2; label: string }> = [
    { which: 0, label: f.states.chinese ? "中" : "英" },
    { which: 1, label: f.states.fullwidth ? "全" : "半" },
    { which: 2, label: f.states.cnPunct ? "。" : "." },
  ];
  return (
    <div className="d2-imefloat" style={{ left: r.x, top: r.y, width: r.w, height: r.h }} role="group" aria-label={`输入法状态：${imeLabel(f.states)}`}>
      {segs.map((s, i) => (
        <React.Fragment key={s.which}>
          {i > 0 && <span className="d2-imefloat-dot" aria-hidden>·</span>}
          <button type="button" className="d2-imefloat-seg" onClick={() => toggleState(s.which)} aria-label={`切换${["中英", "全半角", "中英标点"][i]}`}>
            {s.label}
          </button>
        </React.Fragment>
      ))}
      <button type="button" className="d2-imefloat-collapse" onClick={() => { f.collapse(true); force((v) => v + 1); }} aria-label="收起为任务栏小标">–</button>
    </div>
  );
}

/* ------------------------------ F110 屏幕键盘 ------------------------------ */

function OskLayer(): React.ReactElement | null {
  const cfg = section<OskCfg>("osk", { full: true, opacity: 90, learning: true, clickThrough: false, alwaysOnTop: true, visible: false });
  const kbRef = useRef<OnScreenKb>(new OnScreenKb());
  const [pos, setPos] = useState({ x: 48, y: 140 });
  const dragRef = useRef<{ dx: number; dy: number } | null>(null);
  const [flash, setFlash] = useState<{ code: number; at: number } | null>(null);
  const [, force] = useState(0);

  useHotkey(
    "ctrl+shift+o",
    () => {
      d2Store.set("osk", { visible: !d2Store.getWith("osk", "visible", false) });
    },
    { scope: "global", priority: 5, allowEditable: true },
  );

  // 学习模式：物理键按下 → 虚拟键同步高亮（镜像桥）。
  useEffect(() => {
    kbRef.current.learning = cfg.learning;
    kbRef.current.full = cfg.full;
    kbRef.current.opacity = cfg.opacity;
    kbRef.current.clickThrough = cfg.clickThrough;
    kbRef.current.alwaysOnTop = cfg.alwaysOnTop;
    const onMirror = (e: Event): void => {
      const d = (e as CustomEvent<{ code: string; key: string; repeat: boolean }>).detail;
      if (!d || d.repeat || !cfg.learning) return;
      const code = kbRef.current.highlightFromPhysical(d);
      if (code !== null) setFlash({ code, at: Date.now() });
    };
    window.addEventListener("vx-d2-key", onMirror);
    return () => window.removeEventListener("vx-d2-key", onMirror);
  }, [cfg.learning, cfg.full, cfg.opacity, cfg.clickThrough, cfg.alwaysOnTop]);

  const press = useCallback((k: VKey): void => {
    const kb = kbRef.current;
    const out = kb.press(k.code);
    setFlash({ code: k.code, at: Date.now() });
    if (out !== "") {
      const target = document.activeElement as HTMLElement | null;
      if (target && (target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement || target.isContentEditable)) {
        if (out === "\b") {
          // 退格：对可编辑目标模拟删除（真实 keydown 派发 + 受控回删兜底）。
          target.dispatchEvent(new KeyboardEvent("keydown", { key: "Backspace", code: "Backspace", bubbles: true, cancelable: true }));
        } else {
          target.dispatchEvent(new KeyboardEvent("keydown", { key: out, bubbles: true, cancelable: true }));
          target.dispatchEvent(new KeyboardEvent("keyup", { key: out, bubbles: true }));
        }
      } else {
        window.dispatchEvent(new KeyboardEvent("keydown", { key: out === " " ? " " : out, bubbles: true, cancelable: true }));
      }
    }
    force((v) => v + 1);
  }, []);

  if (!cfg.visible) return null;
  const kb = kbRef.current;
  const keys = kb.keys();
  // 布局按行排（full：功能排/主区四排/底排/导航区/小键盘按 w 加权流式布局）。
  const scale = 0.62;
  const winW = Math.round(WINDOW_W_PX * scale);
  const winH = Math.round(WINDOW_H_PX * scale);
  return (
    <div
      className="d2-osk"
      style={{
        left: pos.x, top: pos.y, width: winW,
        opacity: cfg.opacity / 100,
        pointerEvents: cfg.clickThrough ? "none" : "auto",
        zIndex: cfg.alwaysOnTop ? 2147482000 : 900,
      }}
      role="group"
      aria-label="屏幕键盘"
    >
      <div
        className="d2-osk-head"
        onPointerDown={(e) => {
          dragRef.current = { dx: e.clientX - pos.x, dy: e.clientY - pos.y };
          (e.target as Element).setPointerCapture?.(e.pointerId);
        }}
        onPointerMove={(e) => {
          if (!dragRef.current) return;
          setPos({ x: e.clientX - dragRef.current.dx, y: e.clientY - dragRef.current.dy });
        }}
        onPointerUp={() => {
          if (!dragRef.current) return;
          dragRef.current = null;
          setPos((p) => {
            const s = OnScreenKb.snapToEdge(p.x, p.y, window.innerWidth, window.innerHeight, winW, winH);
            return { x: s.x, y: s.y };
          });
        }}
      >
        <span className="d2-osk-title">屏幕键盘</span>
        <span className="d2-osk-head-actions">
          <button type="button" onClick={() => { d2Store.set("osk", { full: !cfg.full }); }} aria-label={cfg.full ? "切换紧凑布局" : "切换全布局"}>{cfg.full ? "紧凑" : "全布局"}</button>
          <button type="button" onClick={() => { d2Store.set("osk", { visible: false }); }} aria-label="关闭屏幕键盘">×</button>
        </span>
      </div>
      <div className="d2-osk-body" style={{ height: winH - 36 }}>
        {keys.map((k) => (
          <button
            key={k.code}
            type="button"
            className={`d2-osk-key d2-osk-key--${k.kind}${kb.shift && (k.kind === "shift" || k.shifted !== "") ? " d2-osk-key--lit" : ""}${kb.caps && k.kind === "capsLock" ? " d2-osk-key--lit" : ""}${kb.numLock && k.kind === "numLock" ? " d2-osk-key--lit" : ""}${kb.ctrl && k.kind === "ctrl" ? " d2-osk-key--lit" : ""}${kb.alt && k.kind === "alt" ? " d2-osk-key--lit" : ""}${kb.held.includes(k.code) ? " d2-osk-key--held" : ""}${flash && flash.code === k.code ? " d2-osk-key--flash" : ""}`}
            style={{ width: Math.max(28, k.w * scale), minWidth: 28 }}
            onPointerDown={(e) => { e.preventDefault(); press(k); }}
            onPointerUp={() => { kb.release(k.code); force((v) => v + 1); }}
            onPointerLeave={() => { if (kb.held.includes(k.code)) { kb.release(k.code); force((v) => v + 1); } }}
            aria-label={`${k.base}${k.shifted ? ` / ${k.shifted}` : ""}`}
          >
            {keyLabel(k, kb.shift, kb.caps) || (k.kind === "space" ? "␣" : "")}
          </button>
        ))}
      </div>
    </div>
  );
}

/* ------------------------------- 运行时总装 ------------------------------- */

export function D2Runtime(): React.ReactElement {
  useKeyMirrorBridge();
  return (
    <>
      <KeyHudLayer />
      <ImeFloatLayer />
      <OskLayer />
    </>
  );
}
