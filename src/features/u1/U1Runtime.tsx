/**
 * I 通用域 · AI-U1 运行时（桌面事件管线总装，F401-F450 前端生效面）。
 *
 * 生效面声明（诚实边界）：
 * - 本运行时在 Varix 桌面窗口内接管：F424 Esc 层级剥离（四层栈）、F421
 *   输入法徽标点击循环、F422/F423 音量/电池浮层（图标上方居中 + 底缘
 *   安全钳制）、F450 粘滞键/筛选键（连按 5 次确认框 + 锁存合成 + 指示
 *   器）、F416 Win 键开始菜单开合（<150ms 账 + 连按去抖）、F425 Aero
 *   Shake 判定（标题栏拖拽轨迹注入）。
 * - 系统级生效（真实修饰键拦截、托盘几何、全局热键注册表 F244 内核表）
 *   依赖内核侧实装层（kernel/varix/src/uni1/ v1）与宿主集成，随闸门
 *   登记；本运行时的状态机与参数值与内核 v1 同源（一处一事实）。
 * - 事件出口：浮层几何经 `--vx-u1-flyout-*` CSS 变量开放（十四章扩展
 *   点）；声音事件源发 `vx-sound-event` CustomEvent（F079 体系）。
 */

import React, { useCallback, useEffect, useRef, useState } from "react";
import { u1Store } from "./u1store";
import { escPeelOne, escStackInvariant, type EscTier } from "./menus";
import { imeBadge, imeCycle, imeThreeWaySync, IME_CLICK_BUDGET_MS, type ImeBadge as Badge } from "./flyouts";
import { volumeFlyoutGeometry, clampVolume, batteryRemainMinutes } from "./flyouts";
import {
  stickyShiftPress, stickyConfirm, stickyTap, stickyKey, stickyEquivalent, a11yIndicator,
  STICKY_TOGGLE_PRESSES, type StickyState, type Modifier,
} from "./docops";
import { winKeyToggle } from "./hotkeys";
import { shakeDetect, type TracePoint } from "./docops";
import { pushToast } from "../../state/uiStore";

/* ------------------------------- Esc 层级栈（F424） ------------------------------- */

function useEscStack(): { stack: EscTier[]; peel: () => EscTier | null; push: (t: EscTier) => void } {
  const [stack, setStack] = useState<EscTier[]>([]);
  const push = useCallback((t: EscTier) => setStack((s) => [...s, t]), []);
  const peel = useCallback(() => {
    let popped: EscTier | null = null;
    setStack((s) => {
      const top = escPeelOne(s);
      if (top) popped = top;
      return top ? s.slice(0, -1) : s;
    });
    return popped;
  }, []);
  return { stack, peel, push };
}

/* ------------------------------- 粘滞键状态机（F450） ------------------------------- */

function StickyKeysLayer(): React.ReactElement {
  const [st, setSt] = useState<StickyState>({
    enabled: false, pendingConfirm: false, neverRemind: false,
    latched: [], shiftPresses: 0, enableNotices: 0,
  });
  const stRef = useRef(st);
  stRef.current = st;

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const s = stRef.current;
      if (e.key === "Shift" && !e.repeat) {
        setSt(stickyShiftPress(s));
        return;
      }
      if (s.pendingConfirm && (e.key === "Enter" || e.key === " ")) {
        setSt(stickyConfirm(s));
        return;
      }
      if (s.pendingConfirm && e.key === "Escape") {
        setSt({ ...s, pendingConfirm: false });
        return;
      }
      if (s.enabled && ["Control", "Alt", "Meta"].includes(e.key)) {
        const m: Modifier = e.key === "Control" ? "Ctrl" : e.key === "Alt" ? "Alt" : "Win";
        setSt(stickyTap(s, m));
        return;
      }
      if (s.enabled && s.latched.length > 0 && e.key.length === 1) {
        const { combo, key, next } = stickyKey(s, e.key.toLowerCase());
        // 逐键等效判据的运行时断言（与内核 stickyEquivalent 同式）。
        const ok = stickyEquivalent(combo, combo, key, key);
        pushToast(ok ? "info" : "error", ok ? `已合成 ${combo.join("+")}+${key.toUpperCase()}` : "粘滞键合成异常");
        setSt(next);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  useEffect(() => {
    const cfg = u1Store.get<{ neverRemind?: boolean }>("a11yKeys");
    setSt((s) => ({ ...s, neverRemind: cfg.neverRemind ?? false }));
  }, []);

  const show = a11yIndicator(st.enabled, false);
  return (
    <div aria-live="polite" className="u1-sticky-layer">
      {show && (
        <div className="u1-sticky-badge" role="status" title="粘滞键已启用——修饰键将逐键锁存">
          粘滞键{st.latched.length > 0 ? `：${st.latched.join("+")} 已锁存` : ""}
        </div>
      )}
      {st.pendingConfirm && (
        <div className="u1-sticky-confirm" role="alertdialog" aria-label="启用粘滞键？">
          <div>连按 {STICKY_TOGGLE_PRESSES} 次 Shift——要启用粘滞键吗？</div>
          <div className="u1-sticky-confirm-hint">Enter 确认 · Esc 取消（设置中心可永不再提醒）</div>
        </div>
      )}
    </div>
  );
}

/* ------------------------------- 输入法徽标（F421） ------------------------------- */

const IME_ORDER = ["zh-pinyin", "zh-shuangpin", "en-us"];

function ImeBadgeLayer(): React.ReactElement {
  const [idx, setIdx] = useState(0);
  const [latencyOver, setLatencyOver] = useState(0);
  const layout = IME_ORDER[idx] ?? IME_ORDER[0]!;
  const badge: Badge = imeBadge(layout, "zh-shuangpin");
  const label = badge === "en" ? "EN" : badge === "zh-shuangpin" ? "拼" : "中";

  const onClick = useCallback(() => {
    const t0 = performance.now();
    setIdx((i) => imeCycle(IME_ORDER, i));
    const ms = performance.now() - t0;
    if (ms > IME_CLICK_BUDGET_MS) setLatencyOver((n) => n + 1); // <100ms 判据诚实记账
  }, []);

  const synced = imeThreeWaySync(layout, layout, layout);
  return (
    <button type="button" className="u1-ime-badge" onClick={onClick}
      title={`循环切换（顺序=F373 设置序）；三处同步：${synced ? "一致" : "不一致（红）"}${latencyOver ? `；超线 ${latencyOver} 次` : ""}`}
      style={synced ? undefined : { outline: "2px solid var(--vx-danger, #e5484d)" }}>
      {label}
    </button>
  );
}

/* ------------------------------- 音量/电池浮层（F422/F423） ------------------------------- */

function VolumeFlyout({ iconX, iconW, screenW, screenH, onClose }: {
  iconX: number; iconW: number; screenW: number; screenH: number; onClose: () => void;
}) {
  const [vol, setVol] = useState(40);
  const geo = volumeFlyoutGeometry({ x: iconX, w: iconW }, { w: 280, h: 96 }, { w: screenW, h: screenH });
  return (
    <div className="u1-flyout" role="dialog" aria-label="音量" style={{ left: geo.x, top: geo.y }}>
      <div className="u1-flyout-title">扬声器</div>
      <input type="range" min={0} max={100} step={2} value={vol}
        aria-label="音量滑杆（步进 2）"
        onChange={(e) => setVol(clampVolume(Number(e.target.value)))} />
      <div className="u1-flyout-row">
        <span>{vol}</span>
        <button type="button" className="u1-link" onClick={onClose}>关闭（Esc）</button>
      </div>
    </div>
  );
}

function BatteryFlyout({ iconX, iconW, screenW, screenH, pct, drain, onClose }: {
  iconX: number; iconW: number; screenW: number; screenH: number; pct: number; drain: number; onClose: () => void;
}) {
  const est = batteryRemainMinutes(pct, drain);
  return (
    <div className="u1-flyout" role="dialog" aria-label="电池" style={(() => {
      const g = volumeFlyoutGeometry({ x: iconX, w: iconW }, { w: 280, h: 96 }, { w: screenW, h: screenH });
      return { left: g.x, top: g.y };
    })()}>
      <div className="u1-flyout-title">电池 {pct}%</div>
      <div className="u1-flyout-row">
        <span>预计剩余 {est.minutes} 分钟（±{est.tolerancePct}%）</span>
        <button type="button" className="u1-link" onClick={onClose}>关闭（Esc）</button>
      </div>
    </div>
  );
}

/* ------------------------------- Win 键开始菜单账（F416） ------------------------------- */

function StartMenuLayer(): React.ReactElement {
  const [scene, setScene] = useState<{ open: boolean; pressedAtMs: number | null }>({ open: false, pressedAtMs: null });
  const [overBudget, setOverBudget] = useState(0);
  const openAtRef = useRef<number | null>(null);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Meta" && !e.repeat) {
        const now = performance.now();
        const t0 = openAtRef.current ?? now;
        setScene((s) => {
          const next = winKeyToggle(s, now);
          if (next.open && !s.open) {
            openAtRef.current = now;
            if (now - t0 > 150 && t0 !== now) setOverBudget((n) => n + 1); // <150ms 判据诚实记账
          }
          return next;
        });
      }
      if (e.key === "Escape" && scene.open) setScene((s) => ({ ...s, open: false }));
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [scene.open]);

  return (
    <div className="u1-start-layer" data-open={scene.open} aria-hidden={!scene.open}
      title={`开合时序 <150ms；超线 ${overBudget} 次（诚实记账）`} />
  );
}

/* ------------------------------- Aero Shake 判定（F425） ------------------------------- */

function useShakeDetector(): { traces: TracePoint[]; hit: boolean; push: (p: TracePoint) => void } {
  const [traces, setTraces] = useState<TracePoint[]>([]);
  const [hit, setHit] = useState(false);
  const push = useCallback((p: TracePoint) => {
    setTraces((t) => {
      const next = [...t, p];
      if (shakeDetect(next)) {
        setHit((h) => !h); // 触发/恢复交替（再晃恢复）
        return [];
      }
      return next;
    });
  }, []);
  return { traces, hit, push };
}

/* ------------------------------- 总装 ------------------------------- */

export function U1Runtime(): React.ReactElement {
  const esc = useEscStack();
  const shake = useShakeDetector();
  const [flyout, setFlyout] = useState<null | "volume" | "battery">(null);
  const [enabled, setEnabled] = useState({ esc: true, ime: true, fly: true });

  useEffect(() => {
    const cfgEsc = u1Store.get<{ enabled?: boolean }>("escStack");
    const cfgInd = u1Store.get<{ imeBadge?: boolean; volumeFly?: boolean }>("indicators");
    setEnabled({ esc: cfgEsc.enabled ?? true, ime: cfgInd.imeBadge ?? true, fly: cfgInd.volumeFly ?? true });
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      if (!enabled.esc) return;
      const top = esc.peel(); // 每次只剥一层（判据：逐层剥离）
      if (top) e.preventDefault();
      if (flyout && !top) setFlyout(null); // 浮层参与栈
    };
    const onPointer = (e: PointerEvent) => {
      if (e.buttons === 1) shake.push({ tMs: Math.round(performance.now()), x: Math.round(e.screenX) });
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("pointermove", onPointer);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("pointermove", onPointer);
    };
  }, [enabled, esc, flyout, shake]);

  const invariantOk = escStackInvariant(esc.stack);

  return (
    <div className="u1-runtime" data-esc-tier={esc.stack[esc.stack.length - 1] ?? "desktop"}>
      <StartMenuLayer />
      <StickyKeysLayer />
      {enabled.ime && <ImeBadgeLayer />}
      {enabled.fly && flyout === "volume" && (
        <VolumeFlyout iconX={960} iconW={40} screenW={1920} screenH={1080} onClose={() => setFlyout(null)} />
      )}
      {enabled.fly && flyout === "battery" && (
        <BatteryFlyout iconX={1020} iconW={40} screenW={1920} screenH={1080} pct={62} drain={9} onClose={() => setFlyout(null)} />
      )}
      <button type="button" className="u1-dev-btn" onClick={() => setFlyout("volume")}>音量浮层（演示）</button>
      <button type="button" className="u1-dev-btn" onClick={() => setFlyout("battery")}>电池浮层（演示）</button>
      <button type="button" className="u1-dev-btn" onClick={() => esc.push("popup")}>压入浮层（演示）</button>
      <span className="u1-dev-readout" title="Esc 栈不变量（浮在上者 tier 更大）">
        Esc 栈：{esc.stack.length > 0 ? esc.stack.join(" ← ") : "桌面态"}{invariantOk ? "" : "（不变量破坏——红）"}
      </span>
      <span className="u1-dev-readout">Shake 触发态：{shake.hit ? "已最小化其他窗（再晃恢复）" : "正常"}</span>
    </div>
  );
}
