import { useEffect, useRef, useState } from "react";
import { Flag, Pause, Play, Plus, RotateCcw, Trash2, X } from "lucide-react";
import { useI18n } from "../../i18n";
import { ipc } from "../../lib/ipc";
import { pushToast } from "../../state/uiStore";
import {
  ALARM_LABEL_MAX,
  CITY_MAX,
  CITY_ZONES,
  cityOf,
  clockUid,
  createStopwatch,
  createTimer,
  defaultClockHubData,
  formatCountdown,
  formatHM,
  formatOffset,
  formatStopwatch,
  minuteKey,
  nextAlarmAt,
  alarmShouldFireNow,
  parseData,
  serializeData,
  stopwatchElapsed,
  stopwatchLap,
  stopwatchReset,
  stopwatchToggle,
  timerJustDone,
  timerMarkDone,
  timerRemainMs,
  timerReset,
  timerToggle,
  zoneOffsetMinutes,
  type Alarm,
  type ClockHubData,
  type StopwatchState,
  type TimerState,
} from "./clockhub";
import "../../styles/ai08-clock.css";

/**
 * Z-22 时钟中心（VWM 工具窗口应用）：
 * - 四标签页：世界时钟（多城市并排 / Intl 处理夏令时 / 可增删）/ 秒表（多实例计次）/
 *   计时器（多实例并发，到点 toast + WebAudio 短提示音）/ 闹钟（按星期重复，可启停）。
 * - 秒表与计时器全部走 clockhub.ts 的「系统时钟锚定」：这里的 250ms tick 只是渲染脉冲，
 *   读数永远由 Date.now() 与锚点差值推出，节流/挂起零漂移。
 * - 持久化：城市选择 + 闹钟 → ipc.toolDataWrite("clockhub.json")；
 *   秒表/计时器是瞬时任务，不跨会话恢复（设计取舍见 clockhub.ts 头注）。
 * - 规格红线：不做日历（任务栏时钟已有）、不做日程管理。
 * - 全键盘可达：原生 button/input/select（Tab/Enter），表单内 Esc 关闭，标签页支持左右方向键。
 */

const TABS = ["world", "stopwatch", "timer", "alarm"] as const;
type Tab = (typeof TABS)[number];

/** 星期短名（i18n 只提供全称，紧凑徽标用本地常量；随 lang 切换）。 */
const DAY_SHORT: Record<string, string[]> = {
  zh: ["日", "一", "二", "三", "四", "五", "六"],
  "zh-TW": ["日", "一", "二", "三", "四", "五", "六"],
  en: ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"],
};

// ---- Intl 格式化器缓存（formatter 构造贵，每个时区只建一次） ----
const timeFmtCache = new Map<string, Intl.DateTimeFormat>();
function zonedTimeFmt(zone: string): Intl.DateTimeFormat {
  let f = timeFmtCache.get(zone);
  if (!f) {
    // hourCycle h23 避免 en-GB 部分环境把 0 点显示成 24
    f = new Intl.DateTimeFormat("en-GB", {
      timeZone: zone,
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
      hourCycle: "h23",
    });
    timeFmtCache.set(zone, f);
  }
  return f;
}

const dateFmtCache = new Map<string, Intl.DateTimeFormat>();
function zonedDateFmt(zone: string, lang: string): Intl.DateTimeFormat {
  const key = `${zone}|${lang}`;
  let f = dateFmtCache.get(key);
  if (!f) {
    f = new Intl.DateTimeFormat(lang === "en" ? "en-US" : "zh-CN", {
      timeZone: zone,
      month: "numeric",
      day: "numeric",
      weekday: "short",
    });
    dateFmtCache.set(key, f);
  }
  return f;
}

// ---- WebAudio 短提示音（无第三方资源；无音频环境/自动播放限制 → 静默降级） ----
let audioCtx: AudioContext | null = null;
function ensureAudio(): void {
  try {
    if (!audioCtx) {
      const Ctor =
        window.AudioContext ?? (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
      if (!Ctor) return;
      audioCtx = new Ctor();
    }
    if (audioCtx.state === "suspended") void audioCtx.resume().catch(() => {});
  } catch {
    audioCtx = null;
  }
}
function playBeeps(count: number): void {
  ensureAudio();
  const ctx = audioCtx;
  if (!ctx || ctx.state !== "running") return;
  const t0 = ctx.currentTime + 0.02;
  for (let i = 0; i < count; i++) {
    const osc = ctx.createOscillator();
    const gain = ctx.createGain();
    osc.type = "sine";
    osc.frequency.value = 880;
    const start = t0 + i * 0.28;
    gain.gain.setValueAtTime(0.0001, start);
    gain.gain.exponentialRampToValueAtTime(0.07, start + 0.02);
    gain.gain.exponentialRampToValueAtTime(0.0001, start + 0.16);
    osc.connect(gain).connect(ctx.destination);
    osc.start(start);
    osc.stop(start + 0.2);
  }
}

function fmtDateTime(ts: number): string {
  const d = new Date(ts);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
}

export function ClockHubApp(_props: { winId: string }): React.ReactElement {
  const { t, lang } = useI18n();
  const [tab, setTab] = useState<Tab>("world");
  const [data, setData] = useState<ClockHubData>(() => defaultClockHubData());
  const [stopwatches, setStopwatches] = useState<StopwatchState[]>([]);
  const [timers, setTimers] = useState<TimerState[]>([]);
  const [now, setNow] = useState(() => Date.now());
  const loadedRef = useRef(false);
  const firedRef = useRef<Set<string>>(new Set());
  const tabRefs = useRef<(HTMLButtonElement | null)[]>([]);

  // 渲染脉冲：只负责让读数跟随系统时钟重算，本身不参与任何计时（锚定算法在 clockhub.ts）
  useEffect(() => {
    const iv = window.setInterval(() => setNow(Date.now()), 250);
    return () => window.clearInterval(iv);
  }, []);

  // 持久化：挂载读 tools/clockhub.json（读失败 → 默认，非 Tauri 环境同理）；之后每次变更写回
  useEffect(() => {
    void ipc
      .toolDataRead("clockhub.json")
      .then((j) => setData(parseData(j)))
      .catch(() => {})
      .finally(() => {
        loadedRef.current = true;
      });
  }, []);

  useEffect(() => {
    if (!loadedRef.current) return;
    void ipc.toolDataWrite("clockhub.json", serializeData(data)).catch(() => {});
  }, [data]);

  // 闹钟触发：分钟匹配 + 同分钟去重；单次闹钟响完自动停用
  useEffect(() => {
    const hit: Alarm[] = [];
    for (const a of data.alarms) {
      if (!alarmShouldFireNow(a, now)) continue;
      const key = `${a.id}|${minuteKey(now)}`;
      if (firedRef.current.has(key)) continue;
      firedRef.current.add(key);
      hit.push(a);
    }
    if (hit.length === 0) return;
    if (firedRef.current.size > 128) firedRef.current = new Set([...firedRef.current].slice(-64));
    playBeeps(3);
    for (const a of hit) {
      pushToast("info", t("chTitle"), `${a.label ? `${a.label} · ` : ""}${formatHM(a.hour, a.minute)}`);
    }
    const onceIds = new Set(hit.filter((a) => a.days.length === 0).map((a) => a.id));
    if (onceIds.size > 0) {
      setData((d) => ({ ...d, alarms: d.alarms.map((a) => (onceIds.has(a.id) ? { ...a, enabled: false } : a)) }));
    }
  }, [now, data.alarms, t]);

  // 计时器到点：锚点判定 → 标记 done + toast + 提示音（done 标记保证只报一次）
  useEffect(() => {
    const doneIds = new Set<string>();
    const next = timers.map((tm) => {
      if (timerJustDone(tm, now)) {
        doneIds.add(tm.id);
        return timerMarkDone(tm);
      }
      return tm;
    });
    if (doneIds.size === 0) return;
    setTimers(next);
    playBeeps(3);
    for (const tm of timers) {
      if (doneIds.has(tm.id)) pushToast("info", t("chTitle"), `${tm.label ? `${tm.label} · ` : ""}${t("chTimerDone")}`);
    }
  }, [now, timers, t]);

  const onTabsKeyDown = (e: React.KeyboardEvent): void => {
    if (e.key !== "ArrowRight" && e.key !== "ArrowLeft") return;
    e.preventDefault();
    const idx = TABS.indexOf(tab);
    const next = e.key === "ArrowRight" ? (idx + 1) % TABS.length : (idx + TABS.length - 1) % TABS.length;
    setTab(TABS[next] ?? tab);
    tabRefs.current[next]?.focus();
  };

  // ---- 世界时钟 ----
  const cities = data.cities.map((id) => cityOf(id)).filter((c): c is NonNullable<typeof c> => !!c);
  const available = CITY_ZONES.filter((c) => !data.cities.includes(c.id));
  const [pickCity, setPickCity] = useState("");
  const addCity = (): void => {
    const id = pickCity || available[0]?.id || "";
    if (!id) return;
    if (data.cities.length >= CITY_MAX) {
      pushToast("info", t("chTitle"), t("chCityLimit"));
      return;
    }
    setData((d) => (d.cities.includes(id) ? d : { ...d, cities: [...d.cities, id] }));
    setPickCity("");
  };
  const removeCity = (id: string): void => {
    setData((d) => ({ ...d, cities: d.cities.filter((c) => c !== id) }));
  };

  // ---- 秒表 ----
  const addStopwatch = (): void => {
    ensureAudio(); // 借用户手势解锁音频，保证到点提示音可播
    setStopwatches((ss) => [...ss, createStopwatch(clockUid("sw"), `${t("chTabStopwatch")} ${ss.length + 1}`)]);
  };
  const patchSw = (id: string, fn: (s: StopwatchState) => StopwatchState): void => {
    setStopwatches((ss) => ss.map((s) => (s.id === id ? fn(s) : s)));
  };

  // ---- 计时器 ----
  const [timerFormOpen, setTimerFormOpen] = useState(false);
  const [tf, setTf] = useState({ label: "", h: "0", m: "5", s: "0" });
  const [tfErr, setTfErr] = useState("");
  const addTimer = (): void => {
    const h = Math.floor(Number(tf.h));
    const m = Math.floor(Number(tf.m));
    const s = Math.floor(Number(tf.s));
    if (![h, m, s].every((v) => Number.isFinite(v) && v >= 0 && v <= 99)) {
      setTfErr(t("chTimerRange"));
      return;
    }
    const ms = (h * 3600 + m * 60 + s) * 1000;
    if (ms <= 0) {
      setTfErr(t("chTimerNeedPositive"));
      return;
    }
    ensureAudio();
    setTimers((ts) => [
      ...ts,
      createTimer(clockUid("tm"), tf.label.trim().slice(0, ALARM_LABEL_MAX) || t("chTabTimer"), ms),
    ]);
    setTimerFormOpen(false);
    setTf({ label: "", h: "0", m: "5", s: "0" });
    setTfErr("");
  };
  const patchTm = (id: string, fn: (x: TimerState) => TimerState): void => {
    setTimers((ts) => ts.map((x) => (x.id === id ? fn(x) : x)));
  };

  // ---- 闹钟 ----
  const [alarmFormOpen, setAlarmFormOpen] = useState(false);
  const [af, setAf] = useState({ label: "", hour: "8", minute: "30", days: [1, 2, 3, 4, 5] as number[] });
  const [afErr, setAfErr] = useState("");
  const dayShort = DAY_SHORT[lang] ?? DAY_SHORT.zh ?? [];
  const addAlarm = (): void => {
    const hour = Math.floor(Number(af.hour));
    const minute = Math.floor(Number(af.minute));
    if (!Number.isFinite(hour) || hour < 0 || hour > 23 || !Number.isFinite(minute) || minute < 0 || minute > 59) {
      setAfErr(t("chAlarmRange"));
      return;
    }
    setData((d) => ({
      ...d,
      alarms: [
        ...d.alarms,
        {
          id: clockUid("al"),
          hour,
          minute,
          days: [...af.days].sort((a, b) => a - b),
          enabled: true,
          label: af.label.trim().slice(0, ALARM_LABEL_MAX),
        },
      ],
    }));
    setAlarmFormOpen(false);
    setAfErr("");
  };
  const toggleAlarm = (id: string): void => {
    setData((d) => ({ ...d, alarms: d.alarms.map((a) => (a.id === id ? { ...a, enabled: !a.enabled } : a)) }));
  };
  const removeAlarm = (id: string): void => {
    setData((d) => ({ ...d, alarms: d.alarms.filter((a) => a.id !== id) }));
  };

  return (
    <div className="ch-app">
      <div className="ch-tabs" role="tablist" aria-label={t("chTitle")} onKeyDown={onTabsKeyDown}>
        {TABS.map((x, i) => (
          <button
            key={x}
            ref={(el) => {
              tabRefs.current[i] = el;
            }}
            type="button"
            role="tab"
            aria-selected={tab === x}
            className={`ch-tab${tab === x ? " active" : ""}`}
            onClick={() => setTab(x)}
          >
            {t(`chTab_${x}`)}
          </button>
        ))}
      </div>

      <div className="ch-body" role="tabpanel">
        {tab === "world" && (
          <>
            <div className="ch-bar">
              {data.cities.length < CITY_MAX && available.length > 0 && (
                <>
                  <select
                    className="text-input"
                    value={pickCity || available[0]?.id || ""}
                    onChange={(e) => setPickCity(e.target.value)}
                    aria-label={t("chCityPick")}
                  >
                    {available.map((c) => (
                      <option key={c.id} value={c.id}>
                        {c.city}
                      </option>
                    ))}
                  </select>
                  <button type="button" className="btn ghost tiny" onClick={addCity}>
                    <Plus size={12} /> {t("chAdd")}
                  </button>
                </>
              )}
              {available.length === 0 && data.cities.length > 0 && (
                <span className="dim small">{t("chAllCitiesAdded")}</span>
              )}
            </div>
            {cities.length === 0 ? (
              <p className="ch-empty">{t("chNoCityData")}</p>
            ) : (
              <div className="ch-world-grid">
                {cities.map((c) => {
                  const at = new Date(now);
                  const off = zoneOffsetMinutes(c.iana, at);
                  return (
                    <div key={c.id} className="ch-city-card">
                      <button
                        type="button"
                        className="icon-btn tiny ch-city-x"
                        aria-label={`${t("chRemove")} ${c.city}`}
                        onClick={() => removeCity(c.id)}
                      >
                        <X size={12} />
                      </button>
                      <span className="ch-city-name" title={`${c.city} · ${c.iana}`}>
                        {c.city}
                      </span>
                      <span className="ch-city-time">{zonedTimeFmt(c.iana).format(at)}</span>
                      <span className="ch-city-meta">
                        {zonedDateFmt(c.iana, lang).format(at)} · {formatOffset(off)}
                      </span>
                    </div>
                  );
                })}
              </div>
            )}
          </>
        )}

        {tab === "stopwatch" && (
          <>
            <div className="ch-bar">
              <button type="button" className="btn primary tiny" onClick={addStopwatch}>
                <Plus size={12} /> {t("chNewStopwatch")}
              </button>
            </div>
            {stopwatches.length === 0 ? (
              <p className="ch-empty">{t("chSwEmpty")}</p>
            ) : (
              <div className="ch-cards">
                {stopwatches.map((sw) => {
                  const el = stopwatchElapsed(sw, now);
                  return (
                    <div key={sw.id} className="ch-card">
                      <div className="ch-card-title">
                        <span className="ellipsis">{sw.label}</span>
                        <span className="flex-1" />
                        <button
                          type="button"
                          className="icon-btn tiny danger-hover"
                          aria-label={t("chDelete")}
                          onClick={() => setStopwatches((ss) => ss.filter((x) => x.id !== sw.id))}
                        >
                          <Trash2 size={12} />
                        </button>
                      </div>
                      <div className={`ch-big${sw.running ? " running" : ""}`}>{formatStopwatch(el)}</div>
                      <div className="ch-btnrow">
                        <button type="button" className="btn ghost tiny" onClick={() => patchSw(sw.id, (s) => stopwatchToggle(s))}>
                          {sw.running ? <Pause size={12} /> : <Play size={12} />}
                          {sw.running ? t("chPause") : el > 0 ? t("chResume") : t("chStart")}
                        </button>
                        <button
                          type="button"
                          className="btn ghost tiny"
                          disabled={!sw.running}
                          onClick={() => patchSw(sw.id, (s) => stopwatchLap(s).sw)}
                        >
                          <Flag size={12} /> {t("chLap")}
                        </button>
                        <button
                          type="button"
                          className="btn ghost tiny"
                          disabled={el === 0 && sw.laps.length === 0}
                          onClick={() => patchSw(sw.id, (s) => stopwatchReset(s))}
                        >
                          <RotateCcw size={12} /> {t("chReset")}
                        </button>
                      </div>
                      {sw.laps.length > 0 && (
                        <ul className="ch-laps">
                          {[...sw.laps].reverse().map((lap, ri) => {
                            const idx = sw.laps.length - 1 - ri;
                            const prev = idx > 0 ? sw.laps[idx - 1] ?? 0 : 0;
                            return (
                              <li key={idx}>
                                <span>
                                  {t("chLapN", { n: idx + 1 })}
                                </span>
                                <span>+{formatStopwatch(lap - prev)}</span>
                                <span>{formatStopwatch(lap)}</span>
                              </li>
                            );
                          })}
                        </ul>
                      )}
                    </div>
                  );
                })}
              </div>
            )}
          </>
        )}

        {tab === "timer" && (
          <>
            <div className="ch-bar">
              <button type="button" className="btn primary tiny" onClick={() => setTimerFormOpen(true)}>
                <Plus size={12} /> {t("chNewTimer")}
              </button>
            </div>
            {timerFormOpen && (
              <form
                className="ch-form"
                onSubmit={(e) => {
                  e.preventDefault();
                  addTimer();
                }}
                onKeyDown={(e) => {
                  if (e.key === "Escape") setTimerFormOpen(false);
                }}
              >
                <div className="ch-form-row">
                  <label className="ch-fld" htmlFor="ch-timer-label">
                    {t("chLabel")}
                  </label>
                  <input
                    id="ch-timer-label"
                    type="text"
                    className="text-input"
                    style={{ maxWidth: 200 }}
                    value={tf.label}
                    maxLength={ALARM_LABEL_MAX}
                    onChange={(e) => setTf({ ...tf, label: e.target.value })}
                  />
                </div>
                <div className="ch-form-row">
                  <label className="ch-fld" htmlFor="ch-timer-h">
                    {t("chHour")}
                  </label>
                  <input
                    id="ch-timer-h"
                    type="number"
                    min={0}
                    max={99}
                    className="text-input ch-num"
                    value={tf.h}
                    onChange={(e) => setTf({ ...tf, h: e.target.value })}
                  />
                  <label className="ch-fld" htmlFor="ch-timer-m">
                    {t("chMin")}
                  </label>
                  <input
                    id="ch-timer-m"
                    type="number"
                    min={0}
                    max={99}
                    className="text-input ch-num"
                    value={tf.m}
                    onChange={(e) => setTf({ ...tf, m: e.target.value })}
                  />
                  <label className="ch-fld" htmlFor="ch-timer-s">
                    {t("chSec")}
                  </label>
                  <input
                    id="ch-timer-s"
                    type="number"
                    min={0}
                    max={99}
                    className="text-input ch-num"
                    value={tf.s}
                    onChange={(e) => setTf({ ...tf, s: e.target.value })}
                  />
                  <span className="flex-1" />
                  <button type="submit" className="btn primary tiny">
                    {t("chTimerBegin")}
                  </button>
                  <button type="button" className="btn ghost tiny" onClick={() => setTimerFormOpen(false)}>
                    {t("chCancel")}
                  </button>
                </div>
                {tfErr && <span className="ch-err">{tfErr}</span>}
              </form>
            )}
            {timers.length === 0 ? (
              <p className="ch-empty">{t("chTimerEmpty")}</p>
            ) : (
              <div className="ch-cards">
                {timers.map((tm) => {
                  const remain = timerRemainMs(tm, now);
                  const pct = tm.durationMs > 0 ? Math.min(100, ((tm.durationMs - remain) / tm.durationMs) * 100) : 100;
                  return (
                    <div key={tm.id} className="ch-card">
                      <div className="ch-card-title">
                        <span className="ellipsis">{tm.label}</span>
                        <span className="flex-1" />
                        <button
                          type="button"
                          className="icon-btn tiny danger-hover"
                          aria-label={t("chDelete")}
                          onClick={() => setTimers((ts) => ts.filter((x) => x.id !== tm.id))}
                        >
                          <Trash2 size={12} />
                        </button>
                      </div>
                      <div className={`ch-big${tm.done ? " done" : ""}`}>
                        {tm.done ? t("chTimerFinished") : formatCountdown(remain)}
                      </div>
                      <div className="ch-timer-track" role="progressbar" aria-valuenow={Math.round(pct)} aria-valuemin={0} aria-valuemax={100}>
                        <div className={`ch-timer-fill${tm.done ? " done" : ""}`} style={{ width: `${pct}%` }} />
                      </div>
                      <div className="ch-btnrow">
                        {!tm.done && (
                          <button type="button" className="btn ghost tiny" onClick={() => patchTm(tm.id, (x) => timerToggle(x))}>
                            {tm.running ? <Pause size={12} /> : <Play size={12} />}
                            {tm.running ? t("chPause") : t("chResume")}
                          </button>
                        )}
                        <button type="button" className="btn ghost tiny" onClick={() => patchTm(tm.id, (x) => timerReset(x))}>
                          <RotateCcw size={12} /> {t("chReset")}
                        </button>
                      </div>
                    </div>
                  );
                })}
              </div>
            )}
          </>
        )}

        {tab === "alarm" && (
          <>
            <div className="ch-bar">
              <button type="button" className="btn primary tiny" onClick={() => setAlarmFormOpen((v) => !v)}>
                <Plus size={12} /> {t("chNewAlarm")}
              </button>
            </div>
            {alarmFormOpen && (
              <form
                className="ch-form"
                onSubmit={(e) => {
                  e.preventDefault();
                  addAlarm();
                }}
                onKeyDown={(e) => {
                  if (e.key === "Escape") setAlarmFormOpen(false);
                }}
              >
                <div className="ch-form-row">
                  <label className="ch-fld" htmlFor="ch-alarm-h">
                    {t("chTime")}
                  </label>
                  <input
                    id="ch-alarm-h"
                    type="number"
                    min={0}
                    max={23}
                    className="text-input ch-num"
                    value={af.hour}
                    onChange={(e) => setAf({ ...af, hour: e.target.value })}
                    aria-label={t("chHour")}
                  />
                  <span className="ch-fld">:</span>
                  <input
                    id="ch-alarm-m"
                    type="number"
                    min={0}
                    max={59}
                    className="text-input ch-num"
                    value={af.minute}
                    onChange={(e) => setAf({ ...af, minute: e.target.value })}
                    aria-label={t("chMin")}
                  />
                  <label className="ch-fld" htmlFor="ch-alarm-label" style={{ marginLeft: 10 }}>
                    {t("chAlarmLabel")}
                  </label>
                  <input
                    id="ch-alarm-label"
                    type="text"
                    className="text-input"
                    style={{ maxWidth: 180 }}
                    value={af.label}
                    maxLength={ALARM_LABEL_MAX}
                    onChange={(e) => setAf({ ...af, label: e.target.value })}
                  />
                </div>
                <div className="ch-form-row">
                  <span className="ch-fld">{t("chRepeat")}</span>
                  {dayShort.map((d, i) => (
                    <button
                      key={i}
                      type="button"
                      className={`ch-day-chip${af.days.includes(i) ? " on" : ""}`}
                      aria-pressed={af.days.includes(i)}
                      aria-label={d}
                      onClick={() =>
                        setAf((f) => ({
                          ...f,
                          days: f.days.includes(i) ? f.days.filter((x) => x !== i) : [...f.days, i],
                        }))
                      }
                    >
                      {d}
                    </button>
                  ))}
                  <span className="dim small">{t("chOnceHint")}</span>
                  <span className="flex-1" />
                  <button type="submit" className="btn primary tiny">
                    {t("chAdd")}
                  </button>
                  <button type="button" className="btn ghost tiny" onClick={() => setAlarmFormOpen(false)}>
                    {t("chCancel")}
                  </button>
                </div>
                {afErr && <span className="ch-err">{afErr}</span>}
              </form>
            )}
            {data.alarms.length === 0 ? (
              <p className="ch-empty">{t("chNoAlarms")}</p>
            ) : (
              <div className="ch-alarm-list">
                {data.alarms.map((a) => (
                  <div key={a.id} className={`ch-alarm-row${a.enabled ? "" : " off"}`}>
                    <span className="ch-alarm-time">{formatHM(a.hour, a.minute)}</span>
                    <div className="ch-alarm-mid">
                      <span className="ch-alarm-label">{a.label || t("chAlarmDefault")}</span>
                      <span className="ch-alarm-sub">
                        {a.days.length === 0 ? (
                          <span className="ch-day-chip on">{t("chOnce")}</span>
                        ) : (
                          dayShort.map((d, i) => (
                            <span key={i} className={`ch-day-chip${a.days.includes(i) ? " on" : ""}`} aria-hidden>
                              {d}
                            </span>
                          ))
                        )}
                        <span>
                          {t("chNextRing")} {fmtDateTime(nextAlarmAt(a, now))}
                        </span>
                      </span>
                    </div>
                    <button
                      type="button"
                      className={`btn ghost tiny ch-onoff${a.enabled ? " active" : ""}`}
                      aria-pressed={a.enabled}
                      onClick={() => toggleAlarm(a.id)}
                    >
                      {a.enabled ? t("chAlarmOn") : t("chAlarmOff")}
                    </button>
                    <button
                      type="button"
                      className="icon-btn tiny danger-hover"
                      aria-label={t("chDelete")}
                      onClick={() => removeAlarm(a.id)}
                    >
                      <Trash2 size={12} />
                    </button>
                  </div>
                ))}
              </div>
            )}
          </>
        )}
      </div>
    </div>
  );
}
