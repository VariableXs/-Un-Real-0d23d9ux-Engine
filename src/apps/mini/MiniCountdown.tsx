import { useEffect, useState } from "react";
import { formatLongDate, useI18n } from "../../i18n";

/**
 * U-18 迷你应用：倒计时日历。
 * - 目标日期选择（date input，本地时区零点）→ D+N 天计数 + 进度条
 * - 进度条基准：选定目标当天为起点（会话态记录，不持久化）；
 *   设计取舍：进度语义 =「从设定之日起已走过多少」，简单直觉且无需存储
 * - 每分钟刷新一次（天数跨零点变化、进度条缓慢推进都覆盖到了）
 */

const DAY_MS = 86400000;

function toISO(d: Date): string {
  const p = (n: number): string => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

function midnight(iso: string): Date {
  return new Date(`${iso}T00:00:00`);
}

export function MiniCountdown(): React.ReactElement {
  const { t, lang } = useI18n();
  // 默认目标：100 天后（选一个有意义的初始值，避免空态）
  const [target, setTarget] = useState(() => toISO(new Date(Date.now() + 100 * DAY_MS)));
  const [startISO, setStartISO] = useState(() => toISO(new Date()));
  const [now, setNow] = useState(() => new Date());

  useEffect(() => {
    const id = window.setInterval(() => setNow(new Date()), 60000);
    return () => window.clearInterval(id);
  }, []);

  const onPick = (iso: string): void => {
    if (!iso) return;
    setTarget(iso);
    setStartISO(toISO(new Date())); // 进度从设定之日起算
  };

  const targetMs = midnight(target).getTime();
  const startMs = midnight(startISO).getTime();
  // D+N：目标零点与当前时刻差值向上取整（当天 → D-Day；已过 → D-N）
  const days = Math.ceil((targetMs - now.getTime()) / DAY_MS);

  let pct = 0;
  if (targetMs > startMs) {
    pct = Math.max(0, Math.min(1, (now.getTime() - startMs) / (targetMs - startMs)));
  } else {
    pct = now.getTime() >= targetMs ? 1 : 0;
  }

  return (
    <div className="mini-cd">
      <label className="mini-cd-row">
        <span className="small dim">{t("miniCountdownTarget")}</span>
        <input
          className="text-input tiny mini-cd-date-input"
          type="date"
          value={target}
          onChange={(e) => onPick(e.target.value)}
        />
      </label>

      <div className={`mini-cd-badge${days < 0 ? " past" : days === 0 ? " today" : ""}`} role="status">
        {days > 0 ? `D+${days}` : days === 0 ? t("miniCountdownToday") : `D${days}`}
      </div>
      <div className="mini-cd-when small dim">
        {formatLongDate(midnight(target), lang)}
        {days < 0 ? ` · ${t("miniCountdownPast")}` : ""}
      </div>

      <div className="mini-cd-progress" role="progressbar" aria-valuenow={Math.round(pct * 100)} aria-valuemin={0} aria-valuemax={100}>
        <div className="mini-cd-fill" style={{ width: `${Math.round(pct * 100)}%` }} />
      </div>
    </div>
  );
}
