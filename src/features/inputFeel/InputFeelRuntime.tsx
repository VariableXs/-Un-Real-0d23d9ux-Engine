/**
 * AI-06 输入手感组 — 桌面层运行时（U-58/U-59、V-63…V-69）。
 *
 * 挂载点：DesktopShell（仅桌面窗口一次）。全部功能默认关闭或等于现状；
 * 设置变化即时生效（props.settings.inputFeel）。
 *
 * 诚实边界（验收口径）：
 * - V-63/64 轨迹与涟漪只覆盖桌面环境窗口（本 webview），四大内置应用
 *   窗口为独立 webview，跨窗口指针特效不在本轮（无越权注入）；
 * - V-66 重映射在桌面层 capture 翻译；宿主低级键盘钩子已被实测静默忽略
 *   （见 shell/kbdhook.rs 注释），故「对四大应用全局生效」登记待验清单；
 * - V-67 自然滚动的触控板/鼠标滚轮区分依赖「近期是否有鼠标移动」启发式
 *   （浏览器不暴露滚轮来源），连接外接鼠标并移动过则恒保持鼠标语义。
 */

import { useEffect, useRef, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { useI18n } from "../../i18n";
import { ipc } from "../../lib/ipc";
import type { Settings } from "../../lib/settings";
import { useDnd } from "../../state/notifyStore";
import {
  KEYBOARD_COVERAGE,
  LONG_PRESS_CONTEXT_MS,
  RIPPLE_MS,
  RIPPLE_REDUCED_MS,
  TYPING_SOUND_PROFILES,
  TRAIL_LEVELS,
  isDragStart,
  matchRemap,
  setLiveInputFeel,
  shouldInvertWheel,
  touchModeActive,
} from "../../lib/inputFeel";
import { SHORTCUT_ACTIONS, prettyAccel } from "../../lib/shortcuts";
import { register as registerKey, unregister as unregisterKey } from "../../lib/keymap/registry";
import "../../styles/input-feel.css";

/** 合成事件标记，避免 V-66 翻译事件再次进入 capture 翻译器（死循环防护）。 */
const remappedEvents = new WeakSet<Event>();

export function InputFeelRuntime(props: { settings: Settings }): React.ReactNode {
  const ife = props.settings.inputFeel;
  const reduceMotion = props.settings.reduceMotion;
  const dnd = useDnd();
  const { t, lang } = useI18n();
  const [cheatOpen, setCheatOpen] = useState(false);
  const [capsOn, setCapsOn] = useState(false);
  const [precisionOn, setPrecisionOn] = useState(false);

  // 快照：供 DesktopIcons / VirtualWindowFrame 等无 props 调用点读取阈值
  useEffect(() => {
    setLiveInputFeel(ife);
  }, [ife]);

  // -----------------------------------------------------------------------
  // U-58 速查浮层：注册到 Z-08 键位注册表（未经仲裁不得注册的红线口径）
  // -----------------------------------------------------------------------
  useEffect(() => {
    const r = registerKey({
      id: "u58-cheatsheet",
      combo: "ctrl+/",
      scope: "global",
      priority: 10,
      source: "U-58:cheatsheet",
      descKey: "kbCovCheatsheet",
    });
    return () => {
      if (r.ok) unregisterKey("u58-cheatsheet");
    };
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent): void => {
      if ((e.ctrlKey || e.metaKey) && !e.shiftKey && !e.altKey && e.key === "/") {
        e.preventDefault();
        setCheatOpen((v) => !v);
      } else if (e.key === "Escape" && cheatOpen) {
        setCheatOpen(false);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [cheatOpen]);

  // -----------------------------------------------------------------------
  // V-66 按键重映射：capture 层单键翻译（环境内；四大应用窗口见诚实边界）
  // -----------------------------------------------------------------------
  useEffect(() => {
    if (ife.keyRemaps.length === 0) return;
    const onKey = (e: KeyboardEvent): void => {
      if (remappedEvents.has(e)) return;
      const hit = matchRemap(e, ife.keyRemaps);
      if (!hit) return;
      e.preventDefault();
      e.stopImmediatePropagation();
      const target = e.target;
      if (!(target instanceof EventTarget)) return;
      // 仅翻译 key/keyCode 层（code 层合成事件浏览器不允许伪造 code 语义之外的行为）
      const synthetic = new KeyboardEvent("keydown", {
        key: hit.key,
        code: hit.code,
        bubbles: e.bubbles,
        cancelable: true,
        ctrlKey: e.ctrlKey,
        altKey: e.altKey,
        shiftKey: e.shiftKey,
        metaKey: e.metaKey,
        repeat: e.repeat,
      });
      remappedEvents.add(synthetic);
      target.dispatchEvent(synthetic);
    };
    window.addEventListener("keydown", onKey, { capture: true });
    return () => window.removeEventListener("keydown", onKey, { capture: true } as EventListenerOptions);
  }, [ife.keyRemaps]);

  // -----------------------------------------------------------------------
  // V-62 指针方案：环境内 cursor 层即时生效（system = 完全不动 = 现状）
  // -----------------------------------------------------------------------
  useEffect(() => {
    const styleId = "if-cursor-style";
    document.getElementById(styleId)?.remove();
    document.documentElement.classList.remove("if-cursor-custom", "if-cursor-large", "if-cursor-hc");
    if (ife.pointerScheme === "system") return;
    const style = document.createElement("style");
    style.id = styleId;
    if (ife.pointerScheme === "custom") {
      // 自定义方案：13 态中环境内可表达的态映射到 CSS cursor（文件路径走 asset 协议）
      const c = ife.customCursors;
      const u = (p?: string): string => (p ? `url("${convertFileSrc(p)}"), ` : "");
      document.documentElement.classList.add("if-cursor-custom");
      style.textContent = `
        .if-cursor-custom, .if-cursor-custom * { cursor: ${u(c.normalSelect)}auto; }
        .if-cursor-custom button, .if-cursor-custom a, .if-cursor-custom [role="button"] { cursor: ${u(c.handwriting) || u(c.normalSelect)}pointer; }
        .if-cursor-custom input, .if-cursor-custom textarea { cursor: ${u(c.textSelect)}text; }
        .if-cursor-custom [disabled], .if-cursor-custom [aria-disabled="true"] { cursor: ${u(c.unavailable)}not-allowed; }
      `;
    } else {
      // 内置大号 / 高对比：SVG 数据 URI 放大箭头（环境内即时预览）
      const large = ife.pointerScheme === "large";
      document.documentElement.classList.add(large ? "if-cursor-large" : "if-cursor-hc");
      const stroke = large ? "rgba(230,240,255,.95)" : "#000";
      const fill = large ? "rgba(90,130,220,.9)" : "#ffdd33";
      const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${large ? 40 : 40}" height="${large ? 40 : 40}"><path d="M6 2 L6 30 L13 23 L18 34 L23 31 L18 21 L28 20 Z" fill="${fill}" stroke="${stroke}" stroke-width="2"/></svg>`;
      const uri = `url("data:image/svg+xml,${encodeURIComponent(svg)}") 4 2, `;
      style.textContent = `
        .if-cursor-large, .if-cursor-large *, .if-cursor-hc, .if-cursor-hc * { cursor: ${uri}auto; }
      `;
    }
    document.head.appendChild(style);
    return () => {
      style.remove();
      document.documentElement.classList.remove("if-cursor-custom", "if-cursor-large", "if-cursor-hc");
    };
  }, [ife.pointerScheme, ife.customCursors]);

  // -----------------------------------------------------------------------
  // V-65 大写锁定提示：仅环境内键盘焦点（document.hasFocus）触发，800ms 淡出
  // -----------------------------------------------------------------------
  useEffect(() => {
    if (!ife.capsLockHint) return;
    let prev = false;
    let timer = 0;
    const check = (e: KeyboardEvent): void => {
      // 密码框场景 Windows 已有系统提示，不重复（克制边界）
      const el = e.target as HTMLElement | null;
      if (el instanceof HTMLInputElement && el.type === "password") return;
      const cur = e.getModifierState("CapsLock");
      if (cur !== prev && document.hasFocus()) {
        prev = cur;
        setCapsOn(cur);
        window.clearTimeout(timer);
        timer = window.setTimeout(() => setCapsOn(false), 800);
      }
    };
    window.addEventListener("keydown", check);
    window.addEventListener("keyup", check);
    return () => {
      window.removeEventListener("keydown", check);
      window.removeEventListener("keyup", check);
      window.clearTimeout(timer);
    };
  }, [ife.capsLockHint]);

  // -----------------------------------------------------------------------
  // V-67 自然滚动 + V-61 滚轮行数（环境内列表/面板；不 hook 系统）
  // -----------------------------------------------------------------------
  const wheelCfg = useRef({ natural: false, factor: 1 });
  wheelCfg.current = { natural: ife.naturalScroll, factor: ife.mouse.wheelLines / 3 };
  useEffect(() => {
    let lastMouseMoveMs = 0;
    let lastPointerType = "";
    const onPointerMove = (e: PointerEvent): void => {
      lastPointerType = e.pointerType;
      if (e.pointerType === "mouse") lastMouseMoveMs = Date.now();
    };
    const scrollableAncestor = (el: EventTarget | null): HTMLElement | null => {
      let n = el instanceof HTMLElement ? el : null;
      while (n) {
        const ov = getComputedStyle(n).overflowY;
        if (n.scrollHeight > n.clientHeight && (ov === "auto" || ov === "scroll" || ov === "overlay")) return n;
        n = n.parentElement;
      }
      return document.scrollingElement instanceof HTMLElement ? document.scrollingElement : null;
    };
    const onWheel = (e: WheelEvent): void => {
      const cfg = wheelCfg.current;
      if (!cfg.natural && cfg.factor === 1) return;
      const target = scrollableAncestor(e.target);
      if (!target) return;
      e.preventDefault();
      const invert = shouldInvertWheel(cfg.natural, lastPointerType, lastMouseMoveMs, Date.now()) ? -1 : 1;
      target.scrollBy({ top: e.deltaY * invert * cfg.factor, left: e.deltaX * invert * cfg.factor });
    };
    window.addEventListener("pointermove", onPointerMove, { passive: true });
    window.addEventListener("wheel", onWheel, { passive: false, capture: true });
    return () => {
      window.removeEventListener("pointermove", onPointerMove);
      window.removeEventListener("wheel", onWheel, { capture: true } as EventListenerOptions);
    };
  }, []);

  // -----------------------------------------------------------------------
  // V-63 指针轨迹（合成层 canvas 残影）+ V-64 点击涟漪 + V-69 十字标
  // -----------------------------------------------------------------------
  const overlayRef = useRef<HTMLCanvasElement | null>(null);
  useEffect(() => {
    const canvas = overlayRef.current;
    if (!canvas || !ife.trailEnabled || reduceMotion) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    const dpr = Math.min(2, window.devicePixelRatio || 1);
    const resize = (): void => {
      canvas.width = window.innerWidth * dpr;
      canvas.height = window.innerHeight * dpr;
    };
    resize();
    window.addEventListener("resize", resize);
    const pts: { x: number; y: number; t: number }[] = [];
    const maxPts = TRAIL_LEVELS[ife.trailLevel - 1] ?? TRAIL_LEVELS[0];
    const onMove = (e: PointerEvent): void => {
      pts.push({ x: e.clientX, y: e.clientY, t: performance.now() });
      if (pts.length > maxPts) pts.splice(0, pts.length - maxPts);
    };
    let raf = 0;
    const paint = (): void => {
      const now = performance.now();
      ctx.clearRect(0, 0, canvas.width, canvas.height);
      // 60ms 衰减的低透明度残影
      for (let i = pts.length - 1; i >= 0; i--) {
        const pt = pts[i];
        if (!pt) continue;
        const age = now - pt.t;
        if (age > 600) {
          pts.splice(i, 1);
          continue;
        }
        const alpha = Math.max(0, 0.35 * (1 - age / 600));
        ctx.fillStyle = `rgba(160,190,255,${alpha})`;
        ctx.beginPath();
        ctx.arc(pt.x * dpr, pt.y * dpr, 3.5 * dpr, 0, Math.PI * 2);
        ctx.fill();
      }
      raf = requestAnimationFrame(paint);
    };
    raf = requestAnimationFrame(paint);
    window.addEventListener("pointermove", onMove, { passive: true });
    return () => {
      cancelAnimationFrame(raf);
      window.removeEventListener("resize", resize);
      window.removeEventListener("pointermove", onMove);
      ctx.clearRect(0, 0, canvas.width, canvas.height); // 关闭后零渲染残留
    };
  }, [ife.trailEnabled, ife.trailLevel, reduceMotion]);

  // V-64 涟漪 / reduce-motion 退化十字标（DOM 层，零 canvas 常驻）
  useEffect(() => {
    if (!ife.rippleEnabled) return;
    const onDown = (e: PointerEvent): void => {
      const host = document.getElementById("if-ripple-host");
      if (!host) return;
      const left = e.button === 2 || (ife.mouse.swapButtons && e.button === 0);
      const el = document.createElement("div");
      el.className = reduceMotion ? "if-cross" : "if-ripple";
      el.style.left = `${e.clientX}px`;
      el.style.top = `${e.clientY}px`;
      if (!reduceMotion) el.classList.add(left ? "right" : "left");
      host.appendChild(el);
      window.setTimeout(() => el.remove(), reduceMotion ? RIPPLE_REDUCED_MS : RIPPLE_MS);
    };
    window.addEventListener("pointerdown", onDown, { passive: true });
    return () => {
      window.removeEventListener("pointerdown", onDown);
      document.getElementById("if-ripple-host")?.replaceChildren(); // 零状态残留
    };
  }, [ife.rippleEnabled, ife.mouse.swapButtons, reduceMotion]);

  // -----------------------------------------------------------------------
  // V-69 指针精确模式：按住修饰键 → SPI 临时降速，松开/失焦即还原
  // -----------------------------------------------------------------------
  const precisionCfg = useRef({ enabled: false, mod: "alt" as string, ratio: 0.4 });
  precisionCfg.current = { enabled: ife.precisionEnabled, mod: ife.precisionModifier, ratio: ife.precisionRatio };
  useEffect(() => {
    const onDown = (e: KeyboardEvent): void => {
      const c = precisionCfg.current;
      if (!c.enabled) return;
      if ((c.mod === "alt" && e.key === "Alt") || (c.mod === "ctrl" && e.key === "Control") || (c.mod === "shift" && e.key === "Shift")) {
        setPrecisionOn(true);
        void ipc.pointerSpeedTemp(c.ratio).catch(() => setPrecisionOn(false));
      }
    };
    const restore = (): void => {
      setPrecisionOn(false);
      void ipc.pointerSpeedRestore().catch(() => {});
    };
    const onUp = (e: KeyboardEvent): void => {
      const c = precisionCfg.current;
      if ((c.mod === "alt" && e.key === "Alt") || (c.mod === "ctrl" && e.key === "Control") || (c.mod === "shift" && e.key === "Shift")) {
        restore();
      }
    };
    window.addEventListener("keydown", onDown);
    window.addEventListener("keyup", onUp);
    window.addEventListener("blur", restore); // 失焦兜底（Alt+Tab 离开环境时必须还原）
    return () => {
      window.removeEventListener("keydown", onDown);
      window.removeEventListener("keyup", onUp);
      window.removeEventListener("blur", restore);
      restore(); // 卸载即还原（红线：退出必须还原宿主状态）
    };
  }, []);

  // -----------------------------------------------------------------------
  // V-68 打字音效：<60ms 程序化合成、随机 pitch、音量/静音/DND 全部联动既有引擎参数
  // -----------------------------------------------------------------------
  const soundCfg = useRef({ id: ife.typingSound, volume: props.settings.soundVolume, muted: props.settings.soundMuted, dnd });
  soundCfg.current = { id: ife.typingSound, volume: props.settings.soundVolume, muted: props.settings.soundMuted, dnd };
  useEffect(() => {
    let ac: AudioContext | null = null;
    const onKey = (e: KeyboardEvent): void => {
      const c = soundCfg.current;
      if (c.id === "off" || c.muted || c.dnd) return;
      if (e.ctrlKey || e.metaKey || e.altKey || e.key.length !== 1) return; // 仅单字符打字
      if (!document.hasFocus()) return; // 仅环境内焦点生效
      try {
        if (!ac) {
          const AC = window.AudioContext || (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
          if (!AC) return;
          ac = new AC();
        }
        if (ac.state === "suspended") void ac.resume().catch(() => {});
        const p = TYPING_SOUND_PROFILES[c.id];
        if (!p) return;
        const osc = ac.createOscillator();
        const env = ac.createGain();
        const t0 = ac.currentTime;
        // 随机 pitch 微变（防听觉疲劳）
        const f = p.base + (Math.random() * 2 - 1) * p.jitter;
        osc.type = "sine";
        osc.frequency.setValueAtTime(f, t0);
        env.gain.setValueAtTime(0.0001, t0);
        env.gain.exponentialRampToValueAtTime(c.volume * 0.08, t0 + 0.008);
        env.gain.exponentialRampToValueAtTime(0.0001, t0 + p.dur);
        osc.connect(env).connect(ac.destination);
        osc.start(t0);
        osc.stop(t0 + p.dur + 0.02);
        window.setTimeout(() => env.disconnect(), 120);
      } catch {
        /* 声音是增益不是依赖 */
      }
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
      void ac?.close().catch(() => {});
      ac = null; // 关闭后零钩子开销
    };
  }, []);

  // -----------------------------------------------------------------------
  // U-59 触控基础：首触切换 touch 模式（44px 目标 + 长按=右键菜单 450ms）
  // -----------------------------------------------------------------------
  useEffect(() => {
    let hasTouchSignal = false;
    const apply = (): void => {
      document.documentElement.classList.toggle("touch-ui", touchModeActive(ife.touchMode, hasTouchSignal));
    };
    apply();
    const onDown = (e: PointerEvent): void => {
      if (e.pointerType === "touch" || e.pointerType === "pen") {
        if (!hasTouchSignal) {
          hasTouchSignal = true;
          apply();
        }
      }
    };
    // 长按 = 右键菜单（任务栏图标/桌面图标 hover 信息的触控替代）
    let pressTimer = 0;
    let pressStart: { x: number; y: number } | null = null;
    const lpDown = (e: PointerEvent): void => {
      if (e.pointerType !== "touch" && e.pointerType !== "pen") return;
      pressStart = { x: e.clientX, y: e.clientY };
      window.clearTimeout(pressTimer);
      pressTimer = window.setTimeout(() => {
        if (!pressStart) return;
        const el = document.elementFromPoint(pressStart.x, pressStart.y);
        if (!el) return;
        el.dispatchEvent(
          new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: pressStart.x, clientY: pressStart.y, button: 2 }),
        );
        pressStart = null;
      }, LONG_PRESS_CONTEXT_MS);
    };
    const lpCancel = (e: PointerEvent): void => {
      // 移动超过拖拽阈值 → 不是长按（交给拖拽语义）
      if (pressStart && isDragStart(e.clientX - pressStart.x, e.clientY - pressStart.y, ife.dragThreshold, e.pointerType)) {
        window.clearTimeout(pressTimer);
        pressStart = null;
      }
    };
    const lpUp = (): void => {
      window.clearTimeout(pressTimer);
      pressStart = null;
    };
    window.addEventListener("pointerdown", onDown, { passive: true });
    window.addEventListener("pointerdown", lpDown, { passive: true });
    window.addEventListener("pointermove", lpCancel, { passive: true });
    window.addEventListener("pointerup", lpUp, { passive: true });
    window.addEventListener("pointercancel", lpUp, { passive: true });
    return () => {
      window.clearTimeout(pressTimer);
      window.removeEventListener("pointerdown", onDown);
      window.removeEventListener("pointerdown", lpDown);
      window.removeEventListener("pointermove", lpCancel);
      window.removeEventListener("pointerup", lpUp);
      window.removeEventListener("pointercancel", lpUp);
      document.documentElement.classList.remove("touch-ui");
    };
  }, [ife.touchMode, ife.dragThreshold]);

  // -----------------------------------------------------------------------
  // 渲染层：轨迹 canvas / 涟漪宿主 / CapsLock OSD / 精确模式十字标 / 速查浮层
  // -----------------------------------------------------------------------
  return (
    <>
      {ife.trailEnabled && !reduceMotion && <canvas ref={overlayRef} className="if-trail-canvas" aria-hidden />}
      {ife.rippleEnabled && <div id="if-ripple-host" className="if-ripple-host" aria-hidden />}
      {capsOn && (
        <div className="if-caps-osd" role="status" aria-live="polite">
          {t("ifCapsOn")}
        </div>
      )}
      {precisionOn && ife.precisionCross && <div className="if-precision-cross" aria-hidden />}
      {cheatOpen && (
        <div className="if-cheatsheet" role="dialog" aria-label={t("ifCheatTitle")}>
          <div className="if-cheatsheet-card">
            <div className="if-cheatsheet-head">
              <span>{t("ifCheatTitle")}</span>
              <span className="dim small">{t("ifCheatHint")}</span>
            </div>
            {(["system", "panel", "window", "launch"] as const).map((grp) => {
              const rows = SHORTCUT_ACTIONS.filter((a) => a.group === grp);
              if (rows.length === 0) return null;
              return (
                <div key={grp} className="if-cheatsheet-group">
                  <div className="if-cheatsheet-gtitle">{t(`ifCheatGroup_${grp}`)}</div>
                  {rows.map((a) => {
                    const accel = props.settings.shortcutBinds[a.id] ?? a.accel;
                    const label = a.labelKey === "scActLaunchN" ? t("scActLaunchN", { n: a.id.replace("launch", "") }) : t(a.labelKey);
                    return (
                      <div key={a.id} className="if-cheatsheet-row">
                        <span>{label}</span>
                        <kbd>{prettyAccel(accel, lang)}</kbd>
                      </div>
                    );
                  })}
                </div>
              );
            })}
            <div className="if-cheatsheet-group">
              <div className="if-cheatsheet-gtitle">{t("ifCheatGroup_more")}</div>
              {KEYBOARD_COVERAGE.slice(0, 6).map((r) => (
                <div key={r.opKey} className="if-cheatsheet-row">
                  <span>{t(r.opKey)}</span>
                  <kbd>{prettyAccel(r.path, lang)}</kbd>
                </div>
              ))}
            </div>
          </div>
        </div>
      )}
    </>
  );
}
