import { useEffect, useRef, useState } from "react";
import { Pause, Play, RotateCcw } from "lucide-react";
import { useI18n } from "../../i18n";
import { pushToast } from "../../state/uiStore";

/**
 * U-18 迷你应用：番茄计时。
 * - 25/5 分钟预设（专注/休息切换即重置）
 * - 开始/暂停/重置 + SVG 进度环（r=44 周长 2πr）
 * - 结束轻提示：pushToast（勿扰/静音矩阵由通知层既有逻辑处理）
 * - 设计取舍：
 *   ① 到点后自动切换到另一模式（暂停态），符合「完成→休息→完成」的自然
 *     节律，用户一键继续；不做自动连跑（避免无人值守时无限循环）
 *   ② 计时读数经 ref 镜像读取、副作用（toast/模式切换）在 interval 回调
 *     里执行而非 setState 更新器里——StrictMode 下更新器可能双调，
 *     纯更新器才能保证 toast 只响一次
 */

const WORK_SEC = 25 * 60;
const BREAK_SEC = 5 * 60;
/** SVG 环几何：viewBox 100×100，r=44。 */
const RING_C = 2 * Math.PI * 44;

function fmt(sec: number): string {
  const m = Math.floor(sec / 60);
  const s = sec % 60;
  return `${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
}

export function MiniPomodoro(): React.ReactElement {
  const { t } = useI18n();
  const [mode, setMode] = useState<"work" | "break">("work");
  const [remaining, setRemaining] = useState(WORK_SEC);
  const [running, setRunning] = useState(false);

  // interval 闭包读取的最新值镜像（渲染期同步，StrictMode 双渲染幂等）
  const liveRef = useRef({ remaining, mode });
  liveRef.current = { remaining, mode };

  useEffect(() => {
    if (!running) return undefined;
    const id = window.setInterval(() => {
      const { remaining: r, mode: m } = liveRef.current;
      if (r > 1) {
        setRemaining(r - 1);
        return;
      }
      // 到点：停表 + 轻提示 + 预装另一模式（暂停态）
      setRemaining(0);
      setRunning(false);
      const wasWork = m === "work";
      pushToast(
        "info",
        t("miniPomodoroTitle"),
        t(wasWork ? "miniPomodoroWorkDone" : "miniPomodoroBreakDone"),
      );
      setMode(wasWork ? "break" : "work");
      setRemaining(wasWork ? BREAK_SEC : WORK_SEC);
    }, 1000);
    return () => window.clearInterval(id);
  }, [running, t]);

  const switchMode = (m: "work" | "break"): void => {
    setMode(m);
    setRemaining(m === "work" ? WORK_SEC : BREAK_SEC);
    setRunning(false);
  };

  const reset = (): void => {
    setRemaining(mode === "work" ? WORK_SEC : BREAK_SEC);
    setRunning(false);
  };

  const total = mode === "work" ? WORK_SEC : BREAK_SEC;
  const pct = Math.max(0, Math.min(1, 1 - remaining / total));

  return (
    <div className="mini-pomo">
      <div className="mini-pomo-modes" role="tablist" aria-label={t("miniPomodoroTitle")}>
        <button
          type="button"
          className={`mini-pomo-mode${mode === "work" ? " on" : ""}`}
          onClick={() => switchMode("work")}
        >
          {t("miniPomodoroWork")}
        </button>
        <button
          type="button"
          className={`mini-pomo-mode${mode === "break" ? " on" : ""}`}
          onClick={() => switchMode("break")}
        >
          {t("miniPomodoroBreak")}
        </button>
      </div>

      <div className="mini-pomo-ring">
        <svg viewBox="0 0 100 100" aria-hidden>
          <circle className="mini-pomo-track" cx="50" cy="50" r="44" />
          <circle
            className={`mini-pomo-arc${mode === "break" ? " break" : ""}`}
            cx="50"
            cy="50"
            r="44"
            style={{ strokeDasharray: RING_C, strokeDashoffset: RING_C * (1 - pct) }}
          />
        </svg>
        <div className="mini-pomo-time">{fmt(remaining)}</div>
      </div>

      <div className="mini-pomo-ctrl">
        <button type="button" className="btn tiny" onClick={() => setRunning((r) => !r)}>
          {running ? <Pause size={12} /> : <Play size={12} />}
          {running ? t("miniPomodoroPause") : t("miniPomodoroStart")}
        </button>
        <button type="button" className="btn ghost tiny" onClick={reset}>
          <RotateCcw size={12} />
          {t("miniPomodoroReset")}
        </button>
      </div>
    </div>
  );
}
