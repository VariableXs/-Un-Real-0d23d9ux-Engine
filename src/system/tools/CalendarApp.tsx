import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useI18n, MONTHS, WEEKDAYS } from "../../i18n";
import { ipc } from "../../lib/ipc";
import { pushNotify } from "../../state/notifyStore";

/**
 * F-2.3 日历与时钟（VWM 虚拟窗口应用）：
 * 月历 / 世界时钟 / 秒表 / 计时器 / 闹钟。
 * 闹钟持久化（tools/calendar.json），触发 = 通知中心横幅 + 可选提示音。
 */

type CalTab = "month" | "world" | "stopwatch" | "timer" | "alarm";

const TABS: { id: CalTab; key: string }[] = [
  { id: "month", key: "calMonth" },
  { id: "world", key: "calWorld" },
  { id: "stopwatch", key: "calStopwatch" },
  { id: "timer", key: "calTimer" },
  { id: "alarm", key: "calAlarm" },
];

/** 时区偏移（分钟，相对 UTC；不含本地时区）。 */
const ZONES: { key: string; offset: number }[] = [
  { key: "北京", offset: 480 },
  { key: "伦敦", offset: 0 },
  { key: "纽约", offset: -300 },
  { key: "东京", offset: 540 },
  { key: "悉尼", offset: 600 },
  { key: "柏林", offset: 60 },
];

interface AlarmItem {
  id: string;
  /** HH:MM（本地时间）。 */
  time: string;
  enabled: boolean;
  label: string;
  /** 上次触发日期（ISO），防止同一分钟重复触发。 */
  lastFired: string | null;
}

function pad(n: number): string {
  return String(n).padStart(2, "0");
}

export function CalendarApp(): React.ReactElement {
  const { t, lang } = useI18n();
  const [tab, setTab] = useState<CalTab>("month");
  const [now, setNow] = useState(() => new Date());

  useEffect(() => {
    const id = window.setInterval(() => setNow(new Date()), 500);
    return () => window.clearInterval(id);
  }, []);

  // ---- 月历 ----
  const [cursor, setCursor] = useState(() => new Date());
  const monthGrid = useMemo(() => {
    const y = cursor.getFullYear();
    const m = cursor.getMonth();
    const first = new Date(y, m, 1).getDay();
    const days = new Date(y, m + 1, 0).getDate();
    const cells: (number | null)[] = Array.from({ length: first }, () => null);
    for (let d = 1; d <= days; d++) cells.push(d);
    while (cells.length % 7 !== 0) cells.push(null);
    return cells;
  }, [cursor]);

  // ---- 秒表 ----
  const [swRunning, setSwRunning] = useState(false);
  const [swMs, setSwMs] = useState(0);
  const swStart = useRef(0);
  useEffect(() => {
    if (!swRunning) return;
    swStart.current = performance.now() - swMs;
    const id = window.setInterval(() => setSwMs(performance.now() - swStart.current), 50);
    return () => window.clearInterval(id);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [swRunning]);

  // ---- 计时器 ----
  const [timerTotal, setTimerTotal] = useState(300); // 秒
  const [timerLeft, setTimerLeft] = useState(300);
  const [timerRunning, setTimerRunning] = useState(false);
  useEffect(() => {
    if (!timerRunning) return;
    const id = window.setInterval(() => {
      setTimerLeft((s) => {
        if (s <= 1) {
          setTimerRunning(false);
          pushNotify("system", t("calTimer"), t("calTimerDone"));
          return 0;
        }
        return s - 1;
      });
    }, 1000);
    return () => window.clearInterval(id);
  }, [timerRunning, t]);

  // ---- 闹钟 ----
  const [alarms, setAlarms] = useState<AlarmItem[]>([]);
  const [newTime, setNewTime] = useState("08:00");

  useEffect(() => {
    void (async () => {
      try {
        const raw = await ipc.toolDataRead("calendar");
        if (raw) setAlarms(JSON.parse(raw) as AlarmItem[]);
      } catch {
        /* 数据损坏按空表处理 */
      }
    })();
  }, []);

  const flushAlarms = useCallback((list: AlarmItem[]) => {
    void ipc.toolDataWrite("calendar", JSON.stringify(list)).catch(() => {});
  }, []);

  // 闹钟触发检查（半秒轮询共享 now）
  useEffect(() => {
    if (alarms.length === 0) return;
    const hhmm = `${pad(now.getHours())}:${pad(now.getMinutes())}`;
    const today = now.toISOString().slice(0, 10);
    let changed = false;
    for (const a of alarms) {
      if (!a.enabled || a.time !== hhmm || a.lastFired === today) continue;
      pushNotify("system", `${t("calAlarm")} · ${a.time}`, a.label || t("calAlarmFire"));
      a.lastFired = today;
      changed = true;
    }
    if (changed) flushAlarms(alarms);
  }, [now, alarms, t, flushAlarms]);

  const addAlarm = () => {
    const a: AlarmItem = {
      id: `a${Date.now().toString(36)}`,
      time: newTime,
      enabled: true,
      label: "",
      lastFired: null,
    };
    const next = [...alarms, a];
    setAlarms(next);
    flushAlarms(next);
  };

  const patchAlarm = (id: string, patch: Partial<AlarmItem>) => {
    const next = alarms.map((x) => (x.id === id ? { ...x, ...patch } : x));
    setAlarms(next);
    flushAlarms(next);
  };

  const removeAlarm = (id: string) => {
    const next = alarms.filter((x) => x.id !== id);
    setAlarms(next);
    flushAlarms(next);
  };

  const worldTime = (offset: number): string => {
    const d = new Date(now.getTime() + (offset - -now.getTimezoneOffset()) * 60000);
    return `${pad(d.getHours())}:${pad(d.getMinutes())}`;
  };

  const fmtMs = (ms: number): string => {
    const s = Math.floor(ms / 1000);
    return `${pad(Math.floor(s / 60))}:${pad(s % 60)}.${pad(Math.floor((ms % 1000) / 10))}`;
  };

  return (
    <div className="cal-app">
      <div className="calc-tabs" role="tablist">
        {TABS.map((x) => (
          <button key={x.id} role="tab" aria-selected={tab === x.id} className={`calc-tab${tab === x.id ? " active" : ""}`} onClick={() => setTab(x.id)}>
            {t(x.key)}
          </button>
        ))}
      </div>

      {tab === "month" && (
        <div className="cal-month">
          <div className="row gap8" style={{ justifyContent: "space-between" }}>
            <button type="button" className="icon-btn tiny" aria-label={t("calPrev")} onClick={() => setCursor(new Date(cursor.getFullYear(), cursor.getMonth() - 1, 1))}>‹</button>
            <span>{MONTHS[lang][cursor.getMonth()]} {cursor.getFullYear()}</span>
            <button type="button" className="icon-btn tiny" aria-label={t("calNext")} onClick={() => setCursor(new Date(cursor.getFullYear(), cursor.getMonth() + 1, 1))}>›</button>
          </div>
          <div className="cal-grid" role="grid">
            {WEEKDAYS[lang].map((w) => (
              <span key={w} className="cal-wd dim small">{w.slice(0, lang === "en" ? 2 : 3)}</span>
            ))}
            {monthGrid.map((d, i) => {
              const isToday = d === now.getDate() && cursor.getMonth() === now.getMonth() && cursor.getFullYear() === now.getFullYear();
              return (
                <span key={i} className={`cal-cell${isToday ? " cal-today" : ""}`} aria-current={isToday ? "date" : undefined}>
                  {d ?? ""}
                </span>
              );
            })}
          </div>
          <p className="dim small">{lang === "en" ? "Local time" : "本地时间"} {pad(now.getHours())}:{pad(now.getMinutes())}:{pad(now.getSeconds())}</p>
        </div>
      )}

      {tab === "world" && (
        <ul className="cal-world">
          {ZONES.map((z) => (
            <li key={z.key} className="cal-zone">
              <span>{z.key}</span>
              <span className="cal-zone-time">{worldTime(z.offset)}</span>
            </li>
          ))}
        </ul>
      )}

      {tab === "stopwatch" && (
        <div className="cal-center">
          <div className="cal-bigtime" role="timer">{fmtMs(swMs)}</div>
          <div className="row gap8" style={{ justifyContent: "center" }}>
            <button type="button" className="btn ghost" onClick={() => setSwRunning((r) => !r)}>
              {swRunning ? t("calPause") : t("calStart")}
            </button>
            <button type="button" className="btn ghost" onClick={() => { setSwRunning(false); setSwMs(0); }}>
              {t("calReset")}
            </button>
          </div>
        </div>
      )}

      {tab === "timer" && (
        <div className="cal-center">
          <div className="cal-bigtime" role="timer">{pad(Math.floor(timerLeft / 60))}:{pad(timerLeft % 60)}</div>
          {!timerRunning && (
            <div className="row gap4" style={{ justifyContent: "center" }}>
              {[60, 300, 600, 1500].map((s) => (
                <button key={s} type="button" className="btn ghost tiny" onClick={() => { setTimerTotal(s); setTimerLeft(s); }}>
                  {s / 60} min
                </button>
              ))}
            </div>
          )}
          <div className="row gap8" style={{ justifyContent: "center" }}>
            <button type="button" className="btn ghost" onClick={() => setTimerRunning((r) => !r)} disabled={timerLeft === 0}>
              {timerRunning ? t("calPause") : t("calStart")}
            </button>
            <button type="button" className="btn ghost" onClick={() => { setTimerRunning(false); setTimerLeft(timerTotal); }}>
              {t("calReset")}
            </button>
          </div>
        </div>
      )}

      {tab === "alarm" && (
        <div className="cal-alarm">
          <div className="row gap8">
            <input type="time" className="text-input" value={newTime} onChange={(e) => setNewTime(e.target.value)} aria-label={t("calAlarmTime")} />
            <button type="button" className="btn ghost tiny" onClick={addAlarm}>+ {t("calAlarmAdd")}</button>
          </div>
          {alarms.length === 0 ? (
            <p className="dim small">{t("calAlarmEmpty")}</p>
          ) : (
            <ul className="cal-alarm-list">
              {alarms.map((a) => (
                <li key={a.id} className="cal-zone">
                  <span>
                    <input
                      type="checkbox"
                      checked={a.enabled}
                      onChange={(e) => patchAlarm(a.id, { enabled: e.target.checked })}
                      aria-label={`${t("calAlarm")} ${a.time}`}
                    />{" "}
                    {a.time}
                    <input
                      className="text-input tiny"
                      style={{ width: 120, marginLeft: 8 }}
                      value={a.label}
                      placeholder={t("calAlarmLabel")}
                      onChange={(e) => patchAlarm(a.id, { label: e.target.value })}
                    />
                  </span>
                  <button type="button" className="icon-btn tiny danger-hover" aria-label={t("notesDelete")} onClick={() => removeAlarm(a.id)}>✕</button>
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
    </div>
  );
}
