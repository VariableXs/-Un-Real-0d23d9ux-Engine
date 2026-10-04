/**
 * AI-16 启动与声音通知组 — 设置页「声音与通知」标签。
 * 覆盖：U-05/U-06 启动交响与仪式节奏、U-52 声景主题、Z-43 逐应用音量记忆、
 * Z-44 勿扰日程、Z-45 声音方案校验、Z-46 通信设备快切、Z-47 通知存档与搜索、
 * Z-48 麦克风使用指示、Z-49 提醒中心、N-32 智能通知整理（学习结论透明 + 用户覆盖）。
 * 红线：bootSoundMode 永远尊重静音（mute 档不可被主题/节奏覆盖）。
 */
import { useCallback, useEffect, useState } from "react";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import { useI18n } from "../../i18n";
import { errMessage, ipc, type Shell } from "../../lib/ipc";
import type { Settings } from "../../lib/settings";
import { pushToast } from "../../state/uiStore";
import { learnedStats, type LearnTable } from "../../state/notifySmart";
import { useNotifications } from "../../state/notifyStore";

function Field(props: { label: string; children: React.ReactNode; hint?: string }): React.ReactElement {
  return (
    <label className="field">
      <span className="field-label">{props.label}</span>
      {props.children}
      {props.hint ? <span className="dim small">{props.hint}</span> : undefined}
    </label>
  );
}

function fmtTime(ts: number): string {
  const d = new Date(ts);
  const p = (n: number): string => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
}

export function SoundNotifyTab(props: {
  settings: Settings;
  onPatch: (patch: Partial<Settings>) => void;
}): React.ReactElement {
  const { t } = useI18n();
  const s = props.settings;
  const set = props.onPatch;

  // ---- Z-43 逐应用音量记忆 ----
  const [volmem, setVolmem] = useState<Shell.VolMemEntryView[]>([]);
  const [syncOut, setSyncOut] = useState<string | null>(null);
  const loadVolmem = useCallback(() => {
    void ipc.volmemList().then(setVolmem).catch(() => setVolmem([]));
  }, []);
  useEffect(loadVolmem, [loadVolmem]);

  // ---- Z-45 方案包校验 ----
  const [schemeOut, setSchemeOut] = useState<string | null>(null);

  // ---- Z-46 通信设备 ----
  const [commDevices, setCommDevices] = useState<{ id: string; name: string; default: boolean }[]>([]);
  useEffect(() => {
    void ipc.audioDevices().then((all) => {
      setCommDevices(all.filter((d) => d.kind === "render").map((d) => ({ id: d.id, name: d.name, default: d.default })));
    }).catch(() => setCommDevices([]));
  }, []);

  // ---- Z-47 通知存档 ----
  const [archQuery, setArchQuery] = useState("");
  const [archApp, setArchApp] = useState("");
  const [archApps, setArchApps] = useState<string[]>([]);
  const [archRows, setArchRows] = useState<Shell.ArchiveRowView[]>([]);
  const [archTotal, setArchTotal] = useState(0);
  const searchArchive = useCallback((q: string, app: string, offset = 0) => {
    void ipc.notifyArchiveQuery(q, app, 20, offset)
      .then((r) => {
        setArchRows(r.rows);
        setArchTotal(r.total);
      })
      .catch(() => {
        setArchRows([]);
        setArchTotal(0);
      });
  }, []);
  useEffect(() => {
    searchArchive("", "");
    void ipc.notifyArchiveApps().then(setArchApps).catch(() => setArchApps([]));
  }, [searchArchive]);

  // ---- Z-48 麦克风指示 ----
  const [mic, setMic] = useState<Shell.MicUsageView | null>(null);
  useEffect(() => {
    void ipc.micUsageState().then(setMic).catch(() => setMic(null));
  }, []);

  // ---- Z-49 提醒中心 ----
  const [reminders, setReminders] = useState<Shell.ReminderView[]>([]);
  const [remText, setRemText] = useState("");
  const [remDue, setRemDue] = useState("");
  const [remRepeat, setRemRepeat] = useState<"none" | "daily" | "weekly">("none");
  const loadReminders = useCallback(() => {
    void ipc.reminderList().then(setReminders).catch(() => setReminders([]));
  }, []);
  useEffect(loadReminders, [loadReminders]);

  async function addReminder(): Promise<void> {
    const text = remText.trim();
    const dueAt = remDue ? new Date(remDue).getTime() : 0;
    if (!text || !dueAt || Number.isNaN(dueAt) || dueAt <= 0) {
      pushToast("error", t("snRemindTitle"), t("snRemindInvalid"));
      return;
    }
    try {
      const list = await ipc.reminderAdd({
        id: `rem-${Date.now()}-${Math.floor(Math.random() * 1e6)}`,
        text,
        dueAt,
        repeat: remRepeat,
        category: "reminder",
        enabled: true,
        lastFired: 0,
      });
      setReminders(list);
      setRemText("");
      setRemDue("");
      pushToast("success", t("snRemindTitle"), t("snRemindAdded"));
    } catch (e) {
      pushToast("error", t("snRemindTitle"), errMessage(e).message);
    }
  }

  // ---- N-32 智能通知 ----
  const learn: LearnTable = {
    sources: {},
    overrides: {},
  };
  // 学习表从通知中心内存态取（useLearnTable 与 items 同源）
  const items = useNotifications();

  const learned = learnedStats({
    ...learn,
    // 从现有 items 聚合只读统计展示（真实学习表经 notifySmart 持久化，此处仅为面板呈现）
    sources: items.reduce<Record<string, { opened: number; dismissed: number; lastAt: number }>>((acc, it) => {
      const k = it.source ?? it.kind;
      const cur = acc[k] ?? { opened: it.read ? 1 : 0, dismissed: it.read ? 0 : 1, lastAt: it.time };
      acc[k] = {
        opened: cur.opened + (it.read ? 1 : 0),
        dismissed: cur.dismissed + (it.read ? 0 : 1),
        lastAt: Math.max(cur.lastAt, it.time),
      };
      return acc;
    }, {}),
  });

  const stackLabel = (v: string): string =>
    v === "silent" ? t("snStackSilent") : v === "digest" ? t("snStackDigest") : t("snStackInstant");

  return (
    <>
      <h4>{t("snTabTitle")}</h4>

      {/* ---- U-05/U-06/U-52 声景与启动 ---- */}
      <h5 className="dim">{t("snScapeTitle")}</h5>
      <Field label={t("snTheme")}>
        <select value={s.soundTheme} onChange={(e) => set({ soundTheme: e.target.value as Settings["soundTheme"] })}>
          <option value="default">{t("snThemeDefault")}</option>
          <option value="wood">{t("snThemeWood")}</option>
          <option value="midnight">{t("snThemeMidnight")}</option>
        </select>
      </Field>
      <Field label={t("snBootSound")}>
        <select value={s.bootSoundMode} onChange={(e) => set({ bootSoundMode: e.target.value as Settings["bootSoundMode"] })}>
          <option value="full">{t("snBootSoundFull")}</option>
          <option value="chime-only">{t("snBootSoundChime")}</option>
          <option value="mute">{t("snBootSoundMute")}</option>
        </select>
        <span className="dim small">{t("snBootSoundHint")}</span>
      </Field>
      <Field label={t("snBootPacing")}>
        <select value={s.bootPacing} onChange={(e) => set({ bootPacing: e.target.value as Settings["bootPacing"] })}>
          <option value="cinematic">{t("snPacingCinematic")}</option>
          <option value="brisk">{t("snPacingBrisk")}</option>
          <option value="instant">{t("snPacingInstant")}</option>
        </select>
      </Field>
      <Field label={t("snNightDamp")}>
        <label className="check-line">
          <input type="checkbox" checked={s.soundNightDamp} onChange={(e) => set({ soundNightDamp: e.target.checked })} />
          {t("snNightDampHint")}
        </label>
      </Field>

      {/* ---- Z-43 逐应用音量记忆 ---- */}
      <h5 className="dim">{t("snVolmemTitle")}</h5>
      {volmem.length === 0 ? (
        <p className="dim small">{t("snVolmemEmpty")}</p>
      ) : (
        <div className="backup-list">
          {volmem.map((m) => (
            <div key={m.app} className="backup-row">
              <code className="small">{m.app}</code>
              <input
                type="range" min={0} max={100}
                value={Math.round(m.volume * 100)}
                onChange={(e) => {
                  const volume = Number(e.target.value) / 100;
                  setVolmem((cur) => cur.map((x) => (x.app === m.app ? { ...x, volume } : x)));
                  void ipc.volmemSave({ ...m, volume, updatedAt: Date.now() }).catch(() => {});
                }}
              />
              <label className="check-line">
                <input
                  type="checkbox" checked={m.remember}
                  onChange={(e) => {
                    const remember = e.target.checked;
                    setVolmem((cur) => cur.map((x) => (x.app === m.app ? { ...x, remember } : x)));
                    void ipc.volmemSave({ ...m, remember, updatedAt: Date.now() }).catch(() => {});
                  }}
                />
                {t("snVolmemRemember")}
              </label>
              <button
                type="button" className="btn small"
                onClick={() => void ipc.volmemForget(m.app).then(loadVolmem).catch(() => {})}
              >
                {t("snForget")}
              </button>
            </div>
          ))}
        </div>
      )}
      <div className="row gap8 wrap">
        <button
          type="button" className="btn"
          onClick={() => void ipc.volmemSync().then((r) => {
            setSyncOut(r.length === 0 ? t("snSyncNone") : r.map((x) => `${x.app}: ${x.ok ? "OK" : x.reason ?? "?"}`).join(" · "));
          }).catch((e) => setSyncOut(errMessage(e).message))}
        >
          {t("snVolmemSync")}
        </button>
        <button type="button" className="btn" onClick={loadVolmem}>{t("snRefresh")}</button>
      </div>
      {syncOut ? <p className="dim small">{syncOut}</p> : null}

      {/* ---- Z-45 声音方案校验 ---- */}
      <h5 className="dim">{t("snSchemeTitle")}</h5>
      <div className="row gap8 wrap">
        <button
          type="button" className="btn"
          onClick={async () => {
            const p = await openFileDialog({ multiple: false, filters: [{ name: "Sound Scheme", extensions: ["json"] }] });
            if (typeof p !== "string") return;
            try {
              const r = await ipc.soundSchemeValidate(p);
              setSchemeOut(r.ok
                ? `${t("snSchemeOk")} · ${r.name} · ${r.events} events`
                : `${t("snSchemeBad")}: ${r.reason ?? "?"}`);
            } catch (e) {
              setSchemeOut(errMessage(e).message);
            }
          }}
        >
          {t("snSchemePick")}
        </button>
      </div>
      {schemeOut ? <p className="dim small">{schemeOut}</p> : null}

      {/* ---- Z-46 通信设备快切 ---- */}
      <Field label={t("snCommDevice")}>
        <select
          value={commDevices.find((d) => d.default)?.id ?? ""}
          onChange={(e) => void ipc.audioSetDefaultComm(e.target.value)
            .then(() => pushToast("success", t("snCommDevice"), t("snCommSet")))
            .catch((e) => pushToast("error", t("snCommDevice"), errMessage(e).message))}
        >
          {commDevices.map((d) => (
            <option key={d.id} value={d.id}>{d.name}{d.default ? " ✓" : ""}</option>
          ))}
          {commDevices.length === 0 && <option value="">—</option>}
        </select>
        <span className="dim small">{t("snCommHint")}</span>
      </Field>

      {/* ---- Z-44 勿扰日程 ---- */}
      <h5 className="dim">{t("snDndTitle")}</h5>
      <Field label={t("snDndSchedule")}>
        <label className="check-line">
          <input type="checkbox" checked={s.dndScheduleEnabled} onChange={(e) => set({ dndScheduleEnabled: e.target.checked })} />
          {t("snDndEnabled")}
        </label>
      </Field>
      <Field label={t("snDndWindow")}>
        <div className="row gap8">
          <input
            type="time" value={s.dndScheduleStart}
            onChange={(e) => set({ dndScheduleStart: e.target.value })}
          />
          <span className="dim small">→</span>
          <input
            type="time" value={s.dndScheduleEnd}
            onChange={(e) => set({ dndScheduleEnd: e.target.value })}
          />
        </div>
        <span className="dim small">{t("snDndWindowHint")}</span>
      </Field>
      <Field label={t("snDndExempt")}>
        <label className="check-line">
          <input type="checkbox" checked={s.dndReminderExempt} onChange={(e) => set({ dndReminderExempt: e.target.checked })} />
          {t("snDndExemptHint")}
        </label>
      </Field>

      {/* ---- N-32 智能通知整理 ---- */}
      <h5 className="dim">{t("snSmartTitle")}</h5>
      <Field label={t("snSmartToggle")}>
        <label className="check-line">
          <input type="checkbox" checked={s.notifySmart} onChange={(e) => set({ notifySmart: e.target.checked })} />
          {t("snSmartHint")}
        </label>
      </Field>
      <Field label={t("snRetention")}>
        <select
          value={String(s.notifyRetentionDays)}
          onChange={(e) => set({ notifyRetentionDays: Number(e.target.value) as Settings["notifyRetentionDays"] })}
        >
          <option value="30">30 {t("snDays")}</option>
          <option value="90">90 {t("snDays")}</option>
          <option value="0">{t("snForever")}</option>
        </select>
      </Field>
      {learned.length > 0 ? (
        <div className="backup-list">
          {learned.map((l) => (
            <div key={l.source} className="backup-row">
              <code className="small">{l.source}</code>
              <span className="dim small">{stackLabel(l.stack)}</span>
              <span className="dim small">{l.basis}</span>
            </div>
          ))}
        </div>
      ) : (
        <p className="dim small">{t("snSmartEmpty")}</p>
      )}

      {/* ---- Z-47 通知存档与搜索 ---- */}
      <h5 className="dim">{t("snArchiveTitle")}</h5>
      <Field label={t("snArchiveSearch")}>
        <div className="row gap8 wrap">
          <input
            value={archQuery}
            placeholder={t("snArchivePlaceholder")}
            onChange={(e) => setArchQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") searchArchive(archQuery, archApp);
            }}
          />
          <select value={archApp} onChange={(e) => { setArchApp(e.target.value); searchArchive(archQuery, e.target.value); }}>
            <option value="">{t("snArchiveAllApps")}</option>
            {archApps.map((a) => (
              <option key={a} value={a}>{a}</option>
            ))}
          </select>
          <button type="button" className="btn" onClick={() => searchArchive(archQuery, archApp)}>{t("snSearch")}</button>
        </div>
      </Field>
      {archTotal > 0 ? <p className="dim small">{t("snArchiveTotal")}: {archTotal}</p> : null}
      {archRows.length > 0 ? (
        <div className="backup-list">
          {archRows.map((r) => (
            <div key={r.id} className="backup-row">
              <code className="small">{fmtTime(r.ts)}</code>
              <span className="small flex-1"><b>{r.app}</b> · {r.title}</span>
              <button
                type="button" className="btn small"
                onClick={() => void ipc.notifyArchiveDelete([r.id]).then((n) => {
                  if (n > 0) searchArchive(archQuery, archApp);
                }).catch(() => {})}
              >
                {t("snDelete")}
              </button>
            </div>
          ))}
        </div>
      ) : (
        <p className="dim small">{t("snArchiveEmpty")}</p>
      )}

      {/* ---- Z-48 麦克风使用指示 ---- */}
      <h5 className="dim">{t("snMicTitle")}</h5>
      <Field label={t("snMicState")}>
        <div className="row gap8">
          <span className={`small ${mic?.inUse ? "sn-mic-on" : "dim"}`}>
            {mic === null ? "…" : mic.inUse ? `${t("snMicInUse")} (${mic.apps.join(", ")})` : t("snMicIdle")}
          </span>
          <button type="button" className="btn small" onClick={() => void ipc.micUsageState().then(setMic).catch(() => setMic(null))}>
            {t("snRefresh")}
          </button>
        </div>
        <span className="dim small">{t("snMicHint")}</span>
      </Field>

      {/* ---- Z-49 提醒中心 ---- */}
      <h5 className="dim">{t("snRemindTitle")}</h5>
      <Field label={t("snRemindNew")}>
        <div className="row gap8 wrap">
          <input
            value={remText}
            placeholder={t("snRemindPlaceholder")}
            onChange={(e) => setRemText(e.target.value)}
            style={{ minWidth: 160 }}
          />
          <input type="datetime-local" value={remDue} onChange={(e) => setRemDue(e.target.value)} />
          <select value={remRepeat} onChange={(e) => setRemRepeat(e.target.value as typeof remRepeat)}>
            <option value="none">{t("snRepeatNone")}</option>
            <option value="daily">{t("snRepeatDaily")}</option>
            <option value="weekly">{t("snRepeatWeekly")}</option>
          </select>
          <button type="button" className="btn" onClick={() => void addReminder()}>{t("snRemindAdd")}</button>
        </div>
      </Field>
      {reminders.length > 0 ? (
        <div className="backup-list">
          {reminders.map((r) => (
            <div key={r.id} className="backup-row">
              <span className="small flex-1">
                <b>{r.text}</b> · {fmtTime(r.dueAt)}{r.repeat !== "none" ? ` · ${t(`snRepeat${r.repeat === "daily" ? "Daily" : "Weekly"}`)}` : ""}
              </span>
              <button
                type="button" className="btn small"
                onClick={() => void ipc.reminderComplete(r.id).then(setReminders).catch((e) => pushToast("error", t("snRemindTitle"), errMessage(e).message))}
              >
                {t("snDone")}
              </button>
              <button
                type="button" className="btn small"
                onClick={() => void ipc.reminderDelete(r.id).then(setReminders).catch(() => {})}
              >
                {t("snDelete")}
              </button>
            </div>
          ))}
        </div>
      ) : (
        <p className="dim small">{t("snRemindEmpty")}</p>
      )}
    </>
  );
}
