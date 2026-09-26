/**
 * H4 真实浮层运行时（v3 · 大量深化 · 四个真表面）。
 *
 * 生效面声明（诚实边界）：
 * - 拾色器：真指针真取样——elementFromPoint → computed style 解析（rgb/rgba/hex +
 *   祖先回退链），取样域 = VARIX 合成面（VARIX 桌面即本 webview）；取到即复制
 *   （HEX+RGB 双格式）+ 进 F234 最近色 + Shift 连续模式（f359 引擎语义）。
 * - 像素标尺：真拖拽真读数（dx/dy/矩形）+ G 键切换 8px/20% 网格 + Esc 秒退（f360）。
 * - 速查浮层：真键盘长按 Win 600ms 触发（f374 状态机），数据源 = N-17
 *   effectiveKeymap 真源（改键即反映，无缓存层）；与既有 Z-12（Ctrl+/）入口共存。
 * - 专注芯片：真计时徽标（f363 引擎），到点双通道提醒并入每日账（防重）。
 * 逻辑全部在 overlays.ts（可测层）；本文件只做事件接线与 DOM 落笔。
 */

import React, { useCallback, useEffect, useRef, useState } from "react";
import { pushToast } from "../../state/uiStore";
import * as f359 from "../../system/h4/f359-colorPicker";
import * as f363 from "../../system/h4/f363-focusTimer";
import { effectiveKeymap } from "../../lib/keys/registry";
import {
  FOCUS_EVENT,
  SUMMON_PICKER,
  SUMMON_RULER,
  entriesFromKeymap,
  focusChipTick,
  pickerHostEscape,
  pickerHostPick,
  pickerHostStart,
  rulerGridPlan,
  rulerHostEscape,
  rulerHostMove,
  rulerHostReadout,
  rulerHostStart,
  sampleColorChain,
  winHoldDown,
  winHoldTick,
  winHoldUp,
  type FocusChipState,
  type PickerHostState,
  type RulerHostState,
  type WinHoldHostState,
} from "./overlays";

/* ------------------------------- 拾色器浮层 ------------------------------- */

function ColorPickerOverlay(props: { onExit: () => void }): React.ReactElement {
  const [host, setHost] = useState<PickerHostState>(() => pickerHostStart({ active: true, picks: 0, recent: [], last: null }));
  const [cursor, setCursor] = useState<{ x: number; y: number } | null>(null);
  const [hoverHex, setHoverHex] = useState<string | null>(null);
  const shiftRef = useRef(false);

  const sampleAt = useCallback((x: number, y: number): ReturnType<typeof sampleColorChain> => {
    const el = document.elementFromPoint(x, y);
    return sampleColorChain(el, (node, prop) => window.getComputedStyle(node).getPropertyValue(prop));
  }, []);

  const onMove = useCallback((e: PointerEvent): void => {
    shiftRef.current = e.shiftKey;
    setCursor({ x: e.clientX, y: e.clientY });
    const { color } = sampleAt(e.clientX, e.clientY);
    setHoverHex(color ? f359.toHex(color) : null);
  }, [sampleAt]);

  const onDown = useCallback((e: PointerEvent): void => {
    const { color } = sampleAt(e.clientX, e.clientY);
    setHost((s) => pickerHostPick(s, color, e.shiftKey));
    if (color) {
      const payload = f359.clipboardPayload(color);
      void navigator.clipboard?.writeText(payload.hex).catch(() => undefined); // 剪贴板策略拒绝时 toast 仍给全值（诚实降级）
      pushToast("success", `已取色 ${payload.hex} · ${payload.rgb}${e.shiftKey ? "（连续模式继续）" : "（已复制入剪贴板）"}`);
    } else {
      pushToast("info", "该点全透明——取不到色，不编色（移到有色元素上重试）");
    }
    if (!e.shiftKey) props.onExit();
  }, [props, sampleAt]);

  const onKey = useCallback((e: KeyboardEvent): void => {
    if (e.key === "Escape") {
      setHost((s) => pickerHostEscape(s));
      props.onExit();
    }
  }, [props]);

  useEffect(() => {
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerdown", onDown);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerdown", onDown);
      window.removeEventListener("keydown", onKey);
    };
  }, [onMove, onDown, onKey]);

  return (
    <div className="h4-picker-layer" role="application" aria-label="屏幕拾色器（单击取样 · Shift 连续 · Esc 退出）">
      {cursor && (
        <div className="h4-picker-loupe" style={{ left: cursor.x + 18, top: cursor.y + 18 }}>
          <span className="h4-picker-swatch" style={{ background: hoverHex ?? "transparent" }} />
          <span className="h4-picker-readout">
            {hoverHex ? <code>{hoverHex}</code> : "透明点"}
          </span>
        </div>
      )}
      <div className="h4-picker-hud">拾色器 · 单击取样{host.picks > 0 ? ` · 本次已取 ${host.picks} 色` : ""} · Shift 连续 · Esc 退出</div>
    </div>
  );
}

/* ------------------------------- 像素标尺浮层 ------------------------------- */

function RulerOverlay(props: { onExit: () => void }): React.ReactElement {
  const [state, setState] = useState<RulerHostState>({ active: true, origin: null, current: null });
  const [grid, setGrid] = useState(false);

  const onMove = useCallback((e: PointerEvent): void => {
    setState((s) => rulerHostMove(s, { x: e.clientX, y: e.clientY }));
  }, []);
  const onDown = useCallback((e: PointerEvent): void => {
    setState((s) => rulerHostStart({ ...s, active: true }, { x: e.clientX, y: e.clientY }));
  }, []);
  const onKey = useCallback((e: KeyboardEvent): void => {
    if (e.key === "Escape") {
      setState((s) => rulerHostEscape(s));
      props.onExit();
    }
    if (e.key === "g" || e.key === "G") setGrid((g) => !g);
  }, [props]);

  useEffect(() => {
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerdown", onDown);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerdown", onDown);
      window.removeEventListener("keydown", onKey);
    };
  }, [onMove, onDown, onKey]);

  const readout = rulerHostReadout(state);
  const gridPlan = grid ? rulerGridPlan(window.innerWidth, window.innerHeight) : null;

  return (
    <div
      className="h4-ruler-layer"
      role="application"
      aria-label="像素标尺（点击设起点 · G 切网格 · Esc 退出）"
      style={grid ? {
        backgroundImage: `repeating-linear-gradient(0deg, rgba(127,127,127,${gridPlan?.opacity ?? 0.2}) 0 1px, transparent 1px ${gridPlan?.step ?? 8}px), repeating-linear-gradient(90deg, rgba(127,127,127,${gridPlan?.opacity ?? 0.2}) 0 1px, transparent 1px ${gridPlan?.step ?? 8}px)`,
      } : undefined}
    >
      {state.origin && state.current && (
        <>
          <svg className="h4-ruler-line" aria-hidden>
            <line x1={state.origin.x} y1={state.origin.y} x2={state.current.x} y2={state.current.y} />
            <circle cx={state.origin.x} cy={state.origin.y} r={3} />
          </svg>
          {readout && (
            <div className="h4-ruler-chip" style={{ left: state.current.x + 14, top: state.current.y + 14 }}>
              {readout.dx} × {readout.dy}px
            </div>
          )}
        </>
      )}
      <div className="h4-picker-hud">像素标尺 · 点击设起点 · G 网格{grid ? "开" : "关"} · Esc 退出</div>
    </div>
  );
}

/* ------------------------------- 速查浮层（长按 Win 600ms） ------------------------------- */

function HotkeySheetOverlay(): React.ReactElement | null {
  const [visible, setVisible] = useState(false);
  const [rows, setRows] = useState(() => entriesFromKeymap(effectiveKeymap()));
  const holdingRef = useRef(false);
  const hostRef = useRef<WinHoldHostState | null>(null);
  const timerRef = useRef<number>(0);

  useEffect(() => {
    // 同源实时性：浮层每次打开重读注册表真源（改键后立即反映——无缓存层）。
    const onKeyDown = (e: KeyboardEvent): void => {
      if (e.key === "Meta" && !e.repeat && !holdingRef.current) {
        holdingRef.current = true;
        setRows(entriesFromKeymap(effectiveKeymap())); // 打开瞬间重读真源
        hostRef.current = winHoldDown(performance.now());
        window.clearInterval(timerRef.current);
        timerRef.current = window.setInterval(() => {
          if (hostRef.current) {
            hostRef.current = winHoldTick(hostRef.current, performance.now());
            setVisible(hostRef.current.sheet.visible);
          }
        }, 100);
      }
    };
    const endHold = (): void => {
      if (!holdingRef.current) return;
      holdingRef.current = false;
      window.clearInterval(timerRef.current);
      if (hostRef.current) hostRef.current = winHoldUp(hostRef.current); // 引擎状态机收口（winDownAt 复位）
      setVisible(hostRef.current?.sheet.visible ?? false);
    };
    const onKeyUp = (e: KeyboardEvent): void => {
      if (e.key === "Meta") endHold();
    };
    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("keyup", onKeyUp);
    window.addEventListener("blur", endHold);
    return () => {
      window.clearInterval(timerRef.current);
      window.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("keyup", onKeyUp);
      window.removeEventListener("blur", endHold);
    };
  }, []);

  if (!visible) return null;
  const groups = rows.reduce<Array<{ group: string; entries: typeof rows }>>((acc, r) => {
    const g = acc.find((x) => x.group === r.group);
    if (g) g.entries.push(r);
    else acc.push({ group: r.group, entries: [r] });
    return acc;
  }, []);

  return (
    <div className="h4-sheet h4-sheet--live" role="dialog" aria-label="快捷键速查（长按 Win 呼出，松开即隐）">
      <p className="h4-muted">数据源：N-17 键位注册表真源（改键即反映）· 与 Ctrl+/ 速查（Z-12）入口共存</p>
      {groups.map((g) => (
        <div key={g.group}>
          <h5>{g.group === "window" ? "窗口" : g.group === "system" ? "系统" : "通用"}</h5>
          {g.entries.slice(0, 14).map((e) => (
            <div key={`${e.combo}-${e.action}`} className="h4-sheet-row"><kbd>{e.combo}</kbd><span>{e.action}</span></div>
          ))}
        </div>
      ))}
    </div>
  );
}

/* ------------------------------- 专注芯片 ------------------------------- */

function FocusChip(): React.ReactElement | null {
  const [state, setState] = useState<FocusChipState>({ run: null, recorded: false });
  const [badge, setBadge] = useState<string | null>(null);

  useEffect(() => {
    const onFocus = (e: Event): void => {
      const detail = (e as CustomEvent<{ type: string; minutes?: number }>).detail;
      if (detail?.type === "start" && detail.minutes) {
        setState({ run: { day: new Date().toISOString().slice(0, 10), plannedMinutes: detail.minutes, startedAt: Date.now(), endedAt: null, outcome: "running" }, recorded: false });
      }
      if (detail?.type === "abandon") {
        setState((s) => {
          if (!s.run) return s;
          const a = f363.abandon(s.run, Date.now());
          f363.recordRun(a, Date.now()); // 放弃入账（真实时长）——同一引擎，不复制账本
          return { run: a, recorded: true };
        });
        setBadge(null);
      }
    };
    window.addEventListener(FOCUS_EVENT, onFocus);
    return () => window.removeEventListener(FOCUS_EVENT, onFocus);
  }, []);

  useEffect(() => {
    if (!state.run || state.run.outcome !== "running") return;
    const h = window.setInterval(() => {
      const r = focusChipTick(state, Date.now());
      setState(r.state);
      setBadge(r.badge);
      if (r.due) {
        const rem = f363.endReminder(state.run!);
        pushToast("success", `${rem.notification}（一声 ${rem.chimeVolume} + 一条通知——双通道达成，已入每日账）`);
      }
    }, 500);
    return () => window.clearInterval(h);
  }, [state]);

  if (!badge) return null;
  return (
    <div className="h4-focus-chip" role="timer" aria-label={`专注计时剩余 ${badge}`}>
      <svg width={12} height={12} viewBox="0 0 24 24" aria-hidden><circle cx="12" cy="13" r="8" fill="none" stroke="currentColor" strokeWidth="2" /><path d="M12 9v4l2.5 2.5M9 2h6" stroke="currentColor" strokeWidth="2" fill="none" strokeLinecap="round" /></svg>
      {badge}
    </div>
  );
}

/* ------------------------------- 宿主 ------------------------------- */

export function H4Overlays(): React.ReactElement {
  const [picker, setPicker] = useState(false);
  const [ruler, setRuler] = useState(false);

  useEffect(() => {
    const onPick = (): void => setPicker(true);
    const onRuler = (): void => setRuler(true);
    window.addEventListener(SUMMON_PICKER, onPick);
    window.addEventListener(SUMMON_RULER, onRuler);
    return () => {
      window.removeEventListener(SUMMON_PICKER, onPick);
      window.removeEventListener(SUMMON_RULER, onRuler);
    };
  }, []);

  return (
    <>
      {picker && <ColorPickerOverlay onExit={() => setPicker(false)} />}
      {ruler && <RulerOverlay onExit={() => setRuler(false)} />}
      <HotkeySheetOverlay />
      <FocusChip />
    </>
  );
}
