import { useCallback, useEffect, useRef, useState } from "react";
import { Cloud, CloudFog, CloudLightning, CloudRain, CloudSnow, CloudSun, RefreshCw, Settings2, Sun } from "lucide-react";
import { useI18n } from "../../i18n";
import { ipc } from "../../lib/ipc";
import { requestNetConsent, type NetDecision } from "../../lib/netGuard";
import {
  cacheRead,
  cacheWrite,
  parseOpenMeteo,
  parseWttr,
  readConfig,
  weatherUrl,
  wmoIconKind,
  wmoText,
  writeConfig,
  WEATHER_TTL_MS,
  type StorageLike,
  type WeatherConfig,
  type WeatherData,
  type WeatherIconKind,
} from "./weatherService";
import "../../styles/ai08-weather.css";

/**
 * Z-23 天气信息卡（任务栏温度小组件）：
 * - 配置未启用 → 恒返回 null（不占任务栏、不挂任何定时器）；
 * - 取数流程：缓存命中且未过期（30 分钟）→ 用缓存；过期 → 先 requestNetConsent，
 *   用户同意（once/always）才 ipc.httpFetch；拒绝/断网/超时/限流 → 静默「暂无数据」，
 *   绝不弹错误窗；
 * - 零后台常驻：只有用户对该主机选过「始终允许」才挂 30 分钟静默续期定时器
 *   （后台绝不弹授权框）；其余场景只靠挂载/手动刷新取数；卸载清理全部定时器；
 * - 不做系统定位（wttr 城市名 / open-meteo 手动经纬度），不做逐小时预报。
 */

const ICONS: Record<WeatherIconKind, typeof Sun> = {
  sun: Sun,
  partly: CloudSun,
  cloud: Cloud,
  fog: CloudFog,
  rain: CloudRain,
  snow: CloudSnow,
  storm: CloudLightning,
};

const DAY_SHORT: Record<string, string[]> = {
  zh: ["周日", "周一", "周二", "周三", "周四", "周五", "周六"],
  "zh-TW": ["週日", "週一", "週二", "週三", "週四", "週五", "週六"],
  en: ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"],
};

function fmtDay(ds: string, lang: string): string {
  const d = new Date(`${ds}T00:00:00`);
  if (Number.isNaN(d.getTime())) return ds;
  const wd = DAY_SHORT[lang] ?? DAY_SHORT.zh ?? [];
  const p = (n: number) => String(n).padStart(2, "0");
  return `${p(d.getMonth() + 1)}-${p(d.getDate())} ${wd[d.getDay()] ?? ""}`;
}

export function WeatherBadge(): React.ReactElement | null {
  const { t, lang } = useI18n();
  // cfg === null：首帧尚未加载 → 不渲染
  const [cfg, setCfg] = useState<WeatherConfig | null>(null);
  const [data, setData] = useState<WeatherData | null>(null);
  const [busy, setBusy] = useState(false);
  const [open, setOpen] = useState(false);
  const [settings, setSettings] = useState(false);
  const [draft, setDraft] = useState("");
  const mountedRef = useRef(false);
  const cfgRef = useRef<WeatherConfig | null>(null);
  const allowTimerRef = useRef(false); // 用户是否对该主机选过「始终允许」
  const hoverTimer = useRef<number | null>(null);
  const closeTimer = useRef<number | null>(null);
  const refreshTimer = useRef<number | null>(null);
  const fetchRef = useRef<(c: WeatherConfig) => Promise<void>>(() => Promise.resolve());

  /** 30 分钟静默续期：仅当用户选过「始终允许」（后台绝不弹授权框）。 */
  const scheduleRefresh = useCallback((): void => {
    if (!allowTimerRef.current) return;
    if (refreshTimer.current !== null) window.clearTimeout(refreshTimer.current);
    refreshTimer.current = window.setTimeout(() => {
      refreshTimer.current = null;
      const c = cfgRef.current;
      if (mountedRef.current && c?.enabled) void fetchRef.current(c);
    }, WEATHER_TTL_MS + 5_000);
  }, []);

  /** 取数全流程：缓存过期时由挂载/保存配置/手动刷新触发；拒绝与失败一律静默降级。 */
  const fetchFlow = useCallback(
    async (c: WeatherConfig): Promise<void> => {
      const url = weatherUrl(c);
      let decision: NetDecision;
      try {
        decision = await requestNetConsent(url, t("wxPurpose"));
      } catch {
        return; // 授权通道自身异常 → 静默
      }
      if (!mountedRef.current) return;
      if (decision !== "once" && decision !== "always") return; // 拒绝 → 静默显示「暂无数据」
      allowTimerRef.current = decision === "always";
      setBusy(true);
      try {
        const text = await ipc.httpFetch(url);
        if (!mountedRef.current) return;
        const parsed = c.provider === "openmeteo" ? parseOpenMeteo(text) : parseWttr(text);
        cacheWrite(window.localStorage, parsed);
        setData(parsed);
        scheduleRefresh();
      } catch {
        /* 断网/超时/限流/坏响应 → 占位文本，绝不弹错误窗 */
      } finally {
        if (mountedRef.current) setBusy(false);
      }
    },
    [scheduleRefresh, t],
  );

  // fetchFlow 经 ref 供定时器回调使用（避免闭包循环依赖）
  useEffect(() => {
    fetchRef.current = fetchFlow;
  }, [fetchFlow]);

  // 挂载：读配置 → 未启用直接空渲染；启用 → 缓存优先，过期才走授权取数。卸载清全部定时器。
  useEffect(() => {
    mountedRef.current = true;
    const c = readConfig(window.localStorage);
    setCfg(c);
    if (c.enabled) {
      const cached = cacheRead(window.localStorage);
      if (cached) setData(cached);
      else void fetchRef.current(c);
    }
    return () => {
      mountedRef.current = false;
      if (hoverTimer.current !== null) window.clearTimeout(hoverTimer.current);
      if (closeTimer.current !== null) window.clearTimeout(closeTimer.current);
      if (refreshTimer.current !== null) window.clearTimeout(refreshTimer.current);
    };
  }, []);

  // 配置变化同步 ref；停用即掐掉续期定时器
  useEffect(() => {
    cfgRef.current = cfg;
    if (cfg && !cfg.enabled && refreshTimer.current !== null) {
      window.clearTimeout(refreshTimer.current);
      refreshTimer.current = null;
    }
  }, [cfg]);

  if (!cfg || !cfg.enabled) return null;

  const ls: StorageLike = window.localStorage;

  const openCard = (): void => {
    if (closeTimer.current !== null) {
      window.clearTimeout(closeTimer.current);
      closeTimer.current = null;
    }
    if (open) return;
    setDraft(cfg.location);
    setOpen(true);
  };
  const closeCard = (): void => {
    if (hoverTimer.current !== null) window.clearTimeout(hoverTimer.current);
    closeTimer.current = window.setTimeout(() => {
      closeTimer.current = null;
      setOpen(false);
      setSettings(false);
    }, 220);
  };
  const toggleCard = (): void => {
    if (open) {
      setOpen(false);
      setSettings(false);
    } else {
      openCard();
    }
  };

  const saveConfig = (next: WeatherConfig): void => {
    writeConfig(ls, next);
    setCfg(next);
    setData(null); // 位置/供应商变了，旧数据立即失效
    setSettings(false);
    void fetchFlow(next); // 保存即刷新（用户主动操作，此时弹授权框不突兀）
  };

  const CurIcon = data ? ICONS[wmoIconKind(data.code)] : Cloud;
  const temp = data ? `${Math.round(data.tempC)}°` : t("wxNoData");

  return (
    <div
      className="wx-wrap"
      onMouseEnter={() => {
        if (hoverTimer.current !== null) window.clearTimeout(hoverTimer.current);
        hoverTimer.current = window.setTimeout(() => openCard(), 160);
      }}
      onMouseLeave={closeCard}
      onKeyDown={(e) => {
        if (e.key === "Escape" && open) {
          e.stopPropagation();
          setOpen(false);
          setSettings(false);
        }
      }}
    >
      <button
        type="button"
        className="wx-badge"
        aria-label={`${t("wxTitle")} ${temp}`}
        aria-expanded={open}
        onClick={toggleCard}
      >
        {busy && !data ? (
          <RefreshCw size={14} className="wx-spin" aria-hidden />
        ) : (
          <CurIcon size={14} strokeWidth={1.8} aria-hidden />
        )}
        <span className="wx-temp">{temp}</span>
      </button>

      {open && (
        <div className="wx-pop" role="dialog" aria-label={t("wxTitle")}>
          <div className="wx-now">
            <CurIcon size={26} strokeWidth={1.6} className="wx-now-icon" aria-hidden />
            <div className="wx-now-main">
              <span className="wx-now-temp">{data ? `${Math.round(data.tempC)}°C` : t("wxNoData")}</span>
              <span className="wx-now-desc">{data ? wmoText(data.code) : t("wxHintDeny")}</span>
            </div>
            <button
              type="button"
              className="icon-btn tiny wx-gear"
              aria-pressed={settings}
              aria-label={t("wxSettings")}
              onClick={() => setSettings((v) => !v)}
            >
              <Settings2 size={13} />
            </button>
          </div>

          {data ? (
            <div className="wx-days">
              {data.days.map((d) => {
                const Ico = ICONS[wmoIconKind(d.code)];
                return (
                  <div key={d.date} className="wx-day">
                    <span className="wx-day-date">{fmtDay(d.date, lang)}</span>
                    <span className="wx-day-ico">
                      <Ico size={14} strokeWidth={1.7} aria-hidden />
                    </span>
                    <span className="wx-day-desc">{wmoText(d.code)}</span>
                    <span className="wx-day-temp">
                      {Math.round(d.minC)}° <span className="dim">/</span> {Math.round(d.maxC)}°
                    </span>
                  </div>
                );
              })}
            </div>
          ) : (
            <div className="wx-empty">{busy ? t("wxFetching") : t("wxNoData")}</div>
          )}

          {settings && (
            <div className="wx-settings">
              <label>
                {t("wxProvider")}
                <select
                  className="text-input"
                  value={cfg.provider}
                  onChange={(e) => {
                    // 切供应商时把草稿位置重置为新供应商的默认形态，避免格式错配
                    setDraft(e.target.value === "openmeteo" ? "39.9,116.4" : "Beijing");
                    setCfg({ ...cfg, provider: e.target.value === "openmeteo" ? "openmeteo" : "wttr" });
                  }}
                >
                  <option value="wttr">wttr.in</option>
                  <option value="openmeteo">open-meteo</option>
                </select>
              </label>
              <label>
                {t("wxLocation")}
                <input
                  type="text"
                  className="text-input"
                  value={draft}
                  maxLength={64}
                  placeholder={cfg.provider === "openmeteo" ? "39.9,116.4" : "Beijing"}
                  onChange={(e) => setDraft(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") saveConfig({ ...cfg, location: draft });
                  }}
                />
              </label>
              <p className="wx-hint">{t("wxLocationHint")}</p>
              <div className="wx-settings-actions">
                <button type="button" className="btn ghost tiny" onClick={() => void fetchFlow(cfg)} disabled={busy}>
                  <RefreshCw size={12} /> {t("wxRefresh")}
                </button>
                <button type="button" className="btn ghost tiny" onClick={() => saveConfig({ ...cfg, location: draft })}>
                  {t("wxSave")}
                </button>
                <button
                  type="button"
                  className="btn ghost tiny danger-hover"
                  onClick={() => saveConfig({ ...cfg, enabled: false })}
                >
                  {t("wxDisable")}
                </button>
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
