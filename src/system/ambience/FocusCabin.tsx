/**
 * AI-18 U-50 焦点舱 2.0 — 全屏专注模式与干扰拦截。
 *
 * 口径：
 * - 全屏深色舱体（壁纸压暗 60% + 中央当前任务大字）；
 * - 仅显示：番茄计时、今日专注累计、退出（双 Esc，独立于环境切换）；
 * - 舱内通知静默队列：新通知照常入通知中心（纯净 ≠ 禁用），舱内实时记
 *   「舱外事件」计数，出舱后汇总提醒；
 * - 专注账本：舱内时长入账（localStorage 7 天滚动；可导出 JSON）；
 * - 音景联动（U-49）：专注时段自动播放所选场景，休息停。
 */

import { useEffect, useMemo, useRef, useState } from "react";
import { useI18n } from "../../i18n";
import { pushNotify, notifyStore } from "../../state/notifyStore";
import { pushToast } from "../../state/uiStore";
import type { Settings } from "../../lib/settings";
import { startScene, stopScene } from "../../lib/soundscape";
import type { SoundscapeScene } from "./schema";
import { localDayKey } from "./briefing";

/** 番茄默认 25/5（分钟）。 */
export const POMODORO_FOCUS_MIN = 25;
export const POMODORO_BREAK_MIN = 5;

const LEDGER_KEY = "ai18.focusLedger";
const ESC_DOUBLE_MS = 400;

export interface LedgerDay {
  day: string;
  minutes: number;
  sessions: { start: number; minutes: number }[];
}

export function loadLedger(): LedgerDay[] {
  try {
    const raw = localStorage.getItem(LEDGER_KEY);
    if (!raw) return [];
    const arr = JSON.parse(raw);
    if (!Array.isArray(arr)) return [];
    const today = localDayKey(new Date());
    return arr
      .filter((d): d is LedgerDay => typeof d?.day === "string" && typeof d?.minutes === "number" && Array.isArray(d.sessions))
      .filter((d) => d.day > shiftDay(today, -7)); // 7 天滚动
  } catch {
    return [];
  }
}

export function saveLedgerDay(entry: LedgerDay): void {
  const all = loadLedger().filter((d) => d.day !== entry.day);
  all.push(entry);
  try {
    localStorage.setItem(LEDGER_KEY, JSON.stringify(all.slice(-8)));
  } catch { /* quota：忽略 */ }
}

export function todayFocusMinutes(): number {
  return loadLedger().find((d) => d.day === localDayKey(new Date()))?.minutes ?? 0;
}

function shiftDay(key: string, delta: number): string {
  const parts = key.split("-").map(Number);
  const dt = new Date(parts[0] ?? 0, (parts[1] ?? 1) - 1, parts[2] ?? 1);
  dt.setDate(dt.getDate() + delta);
  return localDayKey(dt);
}

export function FocusCabin(props: {
  open: boolean;
  onClose: () => void;
  settings: Settings;
}): React.ReactElement | null {
  const { t } = useI18n();
  const { open } = props;
  const amb = props.settings.ambience;

  // ---- 中央任务（进入时输入；空 = 「专注」）----
  const [task, setTask] = useState("");
  const [taskLocked, setTaskLocked] = useState(false);

  // ---- 番茄计时 ----
  const [phase, setPhase] = useState<"focus" | "break">("focus");
  const [remainSec, setRemainSec] = useState(POMODORO_FOCUS_MIN * 60);
  const [running, setRunning] = useState(true);
  const phaseRef = useRef(phase);
  phaseRef.current = phase;

  // ---- 音景联动 ----
  const [scene, setScene] = useState<SoundscapeScene | null>(null);
  const sceneRef = useRef<SoundscapeScene | null>(null);
  sceneRef.current = scene;

  // ---- 舱外事件（舱内到达的通知计数；出舱汇总）----
  const [outsideEvents, setOutsideEvents] = useState(0);
  const enterTime = useRef(0);
  const enterId = useRef(0);

  // 双 Esc 退出
  const lastEsc = useRef(0);

  useEffect(() => {
    if (!open) return;
    setTask("");
    setTaskLocked(false);
    setPhase("focus");
    setRemainSec(POMODORO_FOCUS_MIN * 60);
    setRunning(true);
    setOutsideEvents(0);
    enterTime.current = Date.now();
    enterId.current = notifyStore.getState().nextId;
    const onKey = (e: KeyboardEvent): void => {
      if (e.key !== "Escape") return;
      const now = Date.now();
      if (now - lastEsc.current <= ESC_DOUBLE_MS) props.onClose();
      lastEsc.current = now;
    };
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      // 出舱：停音景（淡出）+ 账本入账 + 舱外事件汇总
      if (sceneRef.current) stopScene(sceneRef.current, amb.soundscapeVolumes[sceneRef.current] ?? 0.5);
      const minutes = Math.max(0, Math.round((Date.now() - enterTime.current) / 60_000));
      if (minutes >= 1) {
        const day = localDayKey(new Date());
        const ledger = loadLedger();
        const entry = ledger.find((d) => d.day === day) ?? { day, minutes: 0, sessions: [] };
        entry.minutes += minutes;
        entry.sessions.push({ start: enterTime.current, minutes });
        saveLedgerDay(entry);
      }
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  // 舱内到达的新通知 = 舱外事件（照常入中心；舱内不弹横幅打扰由 DND 语义兜底）
  useEffect(() => {
    if (!open) return;
    const count = (): void => {
      const arrived = notifyStore.getState().items.filter((it) => it.id >= enterId.current).length;
      setOutsideEvents(arrived);
    };
    count();
    return notifyStore.subscribe(count);
  }, [open]);

  // 番茄计时 + 音景联动（专注播 / 休息停）
  useEffect(() => {
    if (!open || !running) return undefined;
    const id = window.setInterval(() => {
      setRemainSec((r) => {
        if (r > 1) return r - 1;
        // 相位切换
        const nextPhase = phaseRef.current === "focus" ? "break" : "focus";
        setPhase(nextPhase);
        if (nextPhase === "focus" && sceneRef.current) {
          startScene(sceneRef.current, amb.soundscapeVolumes[sceneRef.current] ?? 0.5);
        } else if (nextPhase === "break" && sceneRef.current) {
          stopScene(sceneRef.current, amb.soundscapeVolumes[sceneRef.current] ?? 0.5);
        }
        return (nextPhase === "focus" ? POMODORO_FOCUS_MIN : POMODORO_BREAK_MIN) * 60;
      });
    }, 1000);
    return () => window.clearInterval(id);
  }, [open, running, amb.soundscapeVolumes]);

  // 场景切换即时生效（专注时段内）
  useEffect(() => {
    if (!open) return;
    if (phase === "focus" && scene) startScene(scene, amb.soundscapeVolumes[scene] ?? 0.5);
    else if (scene) stopScene(scene, amb.soundscapeVolumes[scene] ?? 0.5);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [scene]);

  // 出舱汇总（舱外事件 ≥1 才提醒；不偷跑）
  const outsideRef = useRef(0);
  outsideRef.current = outsideEvents;
  useEffect(() => {
    if (open) return;
    const n = outsideRef.current;
    if (n > 0) {
      pushToast("info", t("amb18CabinTitle"), t("amb18CabinSummary", { n }));
      pushNotify("system", t("amb18CabinTitle"), t("amb18CabinSummary", { n }));
    }
    outsideRef.current = 0;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  const todayMin = useMemo(() => todayFocusMinutes(), [open]);
  if (!open) return null;

  const mm = String(Math.floor(remainSec / 60)).padStart(2, "0");
  const ss = String(remainSec % 60).padStart(2, "0");

  return (
    <div className="ai18-cabin" data-testid="ai18-cabin" role="dialog" aria-label={t("amb18CabinTitle")}>
      {!taskLocked ? (
        <input
          className="text-input"
          style={{ maxWidth: 420, fontSize: 18, textAlign: "center" }}
          value={task}
          placeholder={t("amb18CabinTaskPlaceholder")}
          autoFocus
          onChange={(e) => setTask(e.target.value.slice(0, 60))}
          onKeyDown={(e) => {
            if (e.key === "Enter") setTaskLocked(true);
          }}
          onBlur={() => setTaskLocked(true)}
        />
      ) : (
        <div className="ai18-cabin-task">{task || t("amb18CabinTaskDefault")}</div>
      )}
      <div className="ai18-cabin-timer" data-phase={phase}>
        {phase === "focus" ? t("amb18CabinFocus") : t("amb18CabinBreak")} · {mm}:{ss}
      </div>
      <div className="ai18-cabin-stats">
        <span>{t("amb18CabinToday", { n: todayMin })}</span>
        <span>{t("amb18CabinOutside", { n: outsideEvents })}</span>
      </div>
      <div className="ai18-cabin-actions">
        <button type="button" className="btn ghost" onClick={() => setRunning((v) => !v)}>
          {running ? t("amb18CabinPause") : t("amb18CabinResume")}
        </button>
        <button type="button" className="btn" onClick={props.onClose}>
          {t("amb18CabinExit")}
        </button>
      </div>
      {/* U-49 联动：场景选择（专注播/休息停） */}
      <div className="row gap8 wrap" style={{ justifyContent: "center" }}>
        {(["rain", "forest", "white", "pink", "night"] as const).map((sc) => (
          <button
            key={sc}
            type="button"
            className={`btn tiny${scene === sc ? " primary" : " ghost"}`}
            onClick={() => setScene((cur) => (cur === sc ? null : sc))}
          >
            {t(`amb18Scene_${sc}`)}
          </button>
        ))}
      </div>
      <div className="ai18-cabin-exit">{t("amb18CabinEscHint")}</div>
    </div>
  );
}
