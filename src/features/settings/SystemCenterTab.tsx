/**
 * F-1 系统设置中心 —— 「环境系统」八节（自包含组件，不触碰 SettingsModal hook 敏感区）。
 * 口径（主计划 21.1）：每节带能力徽标 —— VM 档：完整 ｜ 直跑档：只读/降级，绝不混淆边界。
 * 数据面复用既有硬件命令（audio/wifi/battery），新增 sysenv_overview/sysenv_display_set。
 */
import { useEffect, useMemo, useState } from "react";
import { Monitor, Volume2, Wifi, User, Clock, AppWindow, BatteryCharging, Accessibility } from "lucide-react";
import { useI18n } from "../../i18n";
import type { Lang } from "../../i18n/dictionaries";
import { errMessage, ipc } from "../../lib/ipc";
import type { SysDisplay, SysEnvOverview, AudioState, AudioDeviceInfo, WifiState, BatteryState, FileAssoc } from "../../lib/ipc";
import type { Settings } from "../../lib/settings";
import { pushToast } from "../../state/uiStore";

export type SysSection =
  | "sys-display"
  | "sys-sound"
  | "sys-net"
  | "sys-account"
  | "sys-time"
  | "sys-apps"
  | "sys-power"
  | "sys-access";

export const SYS_SECTIONS: { id: SysSection; icon: React.ElementType }[] = [
  { id: "sys-display", icon: Monitor },
  { id: "sys-sound", icon: Volume2 },
  { id: "sys-net", icon: Wifi },
  { id: "sys-account", icon: User },
  { id: "sys-time", icon: Clock },
  { id: "sys-apps", icon: AppWindow },
  { id: "sys-power", icon: BatteryCharging },
  { id: "sys-access", icon: Accessibility },
];

/** 能力徽标：VM 档完整（可写）/ 直跑档只读，与主计划能力矩阵一一对应。 */
function CapBadge(props: { vm: boolean }): React.ReactElement {
  const { t } = useI18n();
  return (
    <span className={`cap-badge ${props.vm ? "full" : "ro"}`}>
      {props.vm ? t("sysCapFull") : t("sysCapRO")}
    </span>
  );
}

function Field(props: { label: string; children: React.ReactNode }): React.ReactElement {
  return (
    <label className="field">
      <span className="field-label">{props.label}</span>
      {props.children}
    </label>
  );
}

/**
 * D-2：直跑档用户级 Shell 覆盖开关（实验性，默认关闭）。
 * 前端负责三重警示（说明 → 再次确认 → 生效提示）；后端负责写 HKCU +
 * 一键还原脚本 + 崩溃自愈。VM 档无意义（登录即 Variable），隐藏。
 */
function BootIntoVariable(): React.ReactElement | null {
  const { t } = useI18n();
  const [status, setStatus] = useState<import("../../lib/ipc").Shell.DirectShellStatus | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    void ipc.directShellStatus().then(setStatus).catch(() => setStatus(null));
  }, []);

  async function toggle(): Promise<void> {
    if (busy) return;
    const enabling = !(status?.enabled ?? false);
    // 三重警示（D-2 规格）：1) 行为说明 2) 恢复方式确认 3) 主确认
    if (enabling) {
      if (!window.confirm(`${t("sysBootVarTitle")}\n\n${t("sysBootVarHint")}`)) return;
      if (!window.confirm(t("sysBootVarConfirm"))) return;
    }
    setBusy(true);
    try {
      await ipc.directShellSet(enabling);
      setStatus(await ipc.directShellStatus());
      pushToast(enabling ? "success" : "info", t("sysBootVarTitle"), enabling ? t("sysBootVarEnabled") : t("sysBootVarDisabled"));
    } catch (e) {
      pushToast("error", t("sysBootVarTitle"), errMessage(e).message);
    } finally {
      setBusy(false);
    }
  }

  return (
    <Field label={t("sysBootVarTitle")}>
      <button type="button" className="btn" disabled={busy} onClick={() => void toggle()}>
        {status?.enabled ? "ON" : "OFF"}
      </button>
      <span className="dim small">{t("sysBootVarHint")}</span>
    </Field>
  );
}

export function SystemCenterTab(props: {
  section: SysSection;
  settings: Settings;
  onChange: (patch: Partial<Settings>) => void;
}): React.ReactElement {
  const { t, lang, setLang } = useI18n();
  const [ov, setOv] = useState<SysEnvOverview | null>(null);
  const [ovErr, setOvErr] = useState<string | null>(null);
  const [wifi, setWifi] = useState<WifiState | null>(null);
  const [battery, setBattery] = useState<BatteryState | null>(null);
  const [audio, setAudio] = useState<AudioState | null>(null);
  const [devices, setDevices] = useState<AudioDeviceInfo[]>([]);
  const [assoc, setAssoc] = useState<FileAssoc[] | null>(null);

  useEffect(() => {
    void ipc
      .sysenvOverview()
      .then(setOv)
      .catch((e) => setOvErr(errMessage(e).message));
    void ipc.wifiGet().then(setWifi).catch(() => setWifi(null));
    void ipc.batteryGet().then(setBattery).catch(() => setBattery(null));
    void ipc.audioGet().then(setAudio).catch(() => setAudio(null));
    void ipc.audioDevices().then(setDevices).catch(() => setDevices([]));
    void ipc.fileAssocList().then(setAssoc).catch(() => setAssoc(null));
  }, []);

  const vm = ov?.vm ?? false;
  const offsetLabel = useMemo(() => {
    if (!ov) return "…";
    const m = ov.utc_offset_minutes;
    const sign = m >= 0 ? "+" : "-";
    const h = Math.floor(Math.abs(m) / 60);
    const min = Math.abs(m) % 60;
    return `UTC${sign}${String(h).padStart(2, "0")}:${String(min).padStart(2, "0")}`;
  }, [ov]);

  // ---- 声音：主音量/静音（IAudioEndpointVolume，已有命令） ----
  async function setVolume(v: number): Promise<void> {
    setAudio((cur) => (cur ? { ...cur, volume: v } : cur));
    try {
      setAudio(await ipc.audioSet(v));
    } catch (e) {
      pushToast("error", t("sysSound"), errMessage(e).message);
    }
  }
  async function setMuted(muted: boolean): Promise<void> {
    try {
      setAudio(await ipc.audioSet(audio?.volume ?? 0.5, muted));
    } catch (e) {
      pushToast("error", t("sysSound"), errMessage(e).message);
    }
  }
  async function setDefaultDevice(id: string): Promise<void> {
    try {
      await ipc.audioSetDefault(id);
      setDevices(await ipc.audioDevices());
      pushToast("success", t("sysSoundDefaultSet"));
    } catch (e) {
      pushToast("error", t("sysSound"), errMessage(e).message);
    }
  }

  // ---- 显示：模式切换（VM 档真实生效，直跑档后端如实拒绝） ----
  async function applyMode(d: SysDisplay, width: number, height: number, hz: number): Promise<void> {
    try {
      const applied = await ipc.sysenvDisplaySet(d.device, width, height, hz);
      pushToast("success", t("sysDispApplied"), applied);
      setOv(await ipc.sysenvOverview());
    } catch (e) {
      pushToast("error", t("sysDispApply"), errMessage(e).message);
    }
  }

  const nightSlider = (
    <Field label={t("sysNightLight")}>
      <input
        type="range" min={0} max={70} step={5}
        value={props.settings.nightLight}
        onChange={(e) => props.onChange({ nightLight: Number(e.target.value) })}
      />
      <span className="dim small">{t("sysNightLightHint")}</span>
    </Field>
  );

  return (
    <>
      <p className="dim small sys-ovline">
        {ov === null
          ? ovErr ?? "…"
          : `${t("sysRunMode")}: ${vm ? t("sysModeVM") : t("sysModeDirect")} · ${ov.vm_reason}`}
      </p>

      {props.section === "sys-display" && (
        <>
          <h4>{t("sysDispTitle")} <CapBadge vm={vm} /></h4>
          {(ov?.displays ?? []).map((d) => {
            const cur = d.current;
            return (
              <Field key={d.device} label={`${d.primary ? t("sysMonitorPrimary") : t("sysMonitor")} · ${d.name}`}>
                <div className="row gap8 wrap">
                  <select
                    defaultValue=""
                    onChange={(e) => {
                      const [w, h] = e.target.value.split("x").map(Number);
                      if (w && h && cur) void applyMode(d, w, h, cur.hz);
                    }}
                  >
                    <option value="">{cur ? `${cur.width}×${cur.height}` : t("sysDispPick")}</option>
                    {d.resolutions.map(([w, h]) => (
                      <option key={`${w}x${h}`} value={`${w}x${h}`}>{w}×{h}</option>
                    ))}
                  </select>
                  {cur && d.refresh_rates.length > 1 && (
                    <select
                      value={String(cur.hz)}
                      onChange={(e) => void applyMode(d, cur.width, cur.height, Number(e.target.value))}
                    >
                      {d.refresh_rates.map((r) => (
                        <option key={r} value={r}>{r} Hz</option>
                      ))}
                    </select>
                  )}
                </div>
                <span className="dim small">{t("sysDispDevHint")}</span>
              </Field>
            );
          })}
          {ov !== null && ov.displays.length === 0 && <p className="dim small">{t("sysDispNone")}</p>}
          {nightSlider}
        </>
      )}

      {props.section === "sys-sound" && (
        <>
          <h4>{t("sysSound")} <CapBadge vm /></h4>
          {audio && (
            <Field label={`${t("sysVolume")}: ${Math.round(audio.volume * 100)}%`}>
              <div className="row gap8">
                <input
                  type="range" min={0} max={100}
                  value={Math.round(audio.volume * 100)}
                  onChange={(e) => void setVolume(Number(e.target.value) / 100)}
                />
                <label className="check-line">
                  <input type="checkbox" checked={audio.muted} onChange={(e) => void setMuted(e.target.checked)} />
                  {t("sysMute")}
                </label>
              </div>
            </Field>
          )}
          <Field label={t("sysOutDevice")}>
            <select
              value={devices.find((d) => d.kind === "render" && d.default)?.id ?? ""}
              onChange={(e) => void setDefaultDevice(e.target.value)}
            >
              {devices.filter((d) => d.kind === "render").map((d) => (
                <option key={d.id} value={d.id}>{d.name}{d.default ? " ✓" : ""}</option>
              ))}
              {devices.filter((d) => d.kind === "render").length === 0 && <option value="">—</option>}
            </select>
          </Field>
          <p className="dim small">{t("sysMixerHint")}</p>
        </>
      )}

      {props.section === "sys-net" && (
        <>
          <h4>{t("sysNet")} <CapBadge vm={false} /></h4>
          <Field label={t("sysWifiState")}>
            <span className="small">
              {wifi?.connected ? `${wifi.ssid ?? "?"}${wifi.signal != null ? ` · ${wifi.signal}%` : ""}` : t("sysWifiOff")}
            </span>
          </Field>
          <p className="dim small">{t("sysNetHint")}</p>
        </>
      )}

      {props.section === "sys-account" && (
        <>
          <h4>{t("sysAccount")} <CapBadge vm={false} /></h4>
          <Field label={t("sysLocalUser")}>
            <code className="small">{ov?.username ?? "…"}</code>
          </Field>
          <p className="dim small">{t("sysAccountHint")}</p>
        </>
      )}

      {props.section === "sys-time" && (
        <>
          <h4>{t("sysTime")} <CapBadge vm={false} /></h4>
          <Field label={t("sysTimezone")}>
            <span className="small">{ov ? `${ov.timezone || "?"} · ${offsetLabel}` : "…"}</span>
          </Field>
          <Field label={t("language")}>
            <select value={lang} onChange={(e) => setLang(e.target.value as Lang)}>
              <option value="zh">简体中文</option>
              <option value="zh-TW">繁體中文</option>
              <option value="en">English</option>
            </select>
          </Field>
        </>
      )}

      {props.section === "sys-apps" && (
        <>
          <h4>{t("sysApps")} <CapBadge vm={false} /></h4>
          {assoc !== null && assoc.length > 0 ? (
            <div className="backup-list">
              {assoc.map((a) => (
                <div key={a.ext} className="backup-row">
                  <code className="small">.{a.ext}</code>
                  <span className="flex-1" />
                  <span className="dim small">{a.appName || t("sysAppHost")}</span>
                </div>
              ))}
            </div>
          ) : (
            <p className="dim small">{t("sysAppsEmpty")}</p>
          )}
          <p className="dim small">{t("sysAppsHint")}</p>
        </>
      )}

      {props.section === "sys-power" && (
        <>
          <h4>{t("sysPower")} <CapBadge vm={vm} /></h4>
          <Field label={t("sysPowerScheme")}>
            <span className="small">{ov?.power_scheme_name ?? "…"}</span>
          </Field>
          {battery && battery.hasBattery && (
            <Field label={t("sysBattery")}>
              <span className="small">
                {battery.percent != null ? `${battery.percent}%` : "…"} · {battery.acOnline ? t("sysACOn") : t("sysACOff")}
              </span>
            </Field>
          )}
          <p className="dim small">{t("sysPowerHint")}</p>
          <BootIntoVariable />
        </>
      )}

      {props.section === "sys-access" && (
        <>
          <h4>{t("sysAccess")} <CapBadge vm /></h4>
          <p className="dim small">{t("sysAccessHint")}</p>
          {nightSlider}
        </>
      )}
    </>
  );
}
