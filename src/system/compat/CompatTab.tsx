/**
 * AI-12 兼容纵深组 — 设置页「兼容性」标签（Z-15…Z-21、M-37…M-45 面板）。
 * 红线：只观察、只提示、只降级；全屏检测统一接口（Z-18）输出在此可见。
 */
import { useEffect, useState } from "react";
import { useI18n } from "../../i18n";
import { ipc } from "../../lib/ipc";
import type { Settings } from "../../lib/settings";
import { COMPAT_MATRIX, MATRIX_VERSION, queryCompat, type MatrixEntry } from "./matrix";
import {
  currentSlowTier,
  detectAndApplyHostProfile,
  evaluateSlowTier,
  applyHighRefreshTiming,
} from "./compatBoot";
import { slowTierLabel, type HostKind, type SlowTier } from "./protocols";

type HostOverride = Settings["compatHostOverride"];

export function CompatTab(props: { settings: Settings; onPatch: (p: Partial<Settings>) => void }): React.ReactElement {
  const { t } = useI18n();
  const s = props.settings;
  const set = <K extends keyof Settings>(k: K, v: Settings[K]): void => props.onPatch({ [k]: v } as Partial<Settings>);

  const [query, setQuery] = useState("");
  const [driverProcs, setDriverProcs] = useState<string[] | null>(null);
  const [hostKind, setHostKind] = useState<HostKind | null>(null);
  const [shimStats, setShimStats] = useState<Record<string, number> | null>(null);
  const [uwpCount, setUwpCount] = useState<number | null>(null);

  useEffect(() => {
    void ipc.compatDriverScan().then(setDriverProcs).catch(() => setDriverProcs([]));
    void ipc.compatShimStats().then(setShimStats).catch(() => setShimStats({}));
    void ipc.compatUwpList().then((a) => setUwpCount(a.length)).catch(() => setUwpCount(null));
  }, []);

  const hit: MatrixEntry | null = query.trim() ? queryCompat(query) : null;
  const tierColor = (tier: string): string => (tier === "A" ? "var(--ok, #4ade80)" : tier === "B" ? "var(--warn, #facc15)" : "var(--bad, #f87171)");

  const reEvalSlow = (override: Settings["compatSlowOverride"]): void => {
    const tier: SlowTier = override === -1 ? currentSlowTier() : (override as SlowTier);
    evaluateSlowTier(tier, { hdd: false, lowMem: false, lowGpu: false });
  };

  return (
    <div className="cp-tab">
      <p className="cp-desc">{t("cpDesc")}</p>

      {/* Z-15 应用适配等级库（纯数据查询，不做自动修复） */}
      <section className="cp-group">
        <h4>{t("cpMatrixTitle")}</h4>
        <p className="cp-hint">{t("cpMatrixHint", { version: MATRIX_VERSION, count: String(COMPAT_MATRIX.length) })}</p>
        <input
          className="cp-query"
          type="text"
          value={query}
          placeholder={t("cpQueryPlaceholder")}
          onChange={(e) => setQuery(e.target.value)}
        />
        {hit && (
          <div className="cp-hit">
            <span className="cp-tier" style={{ color: tierColor(hit.tier) }}>{hit.tier}</span>
            <span className="cp-hit-name">{hit.name}</span>
            {hit.symptom && <span className="cp-hit-symptom">{t("cpSymptom")}: {hit.symptom}</span>}
            {hit.advice && <span className="cp-hit-advice">{t("cpAdvice")}: {hit.advice}</span>}
          </div>
        )}
        {query.trim() && !hit && <p className="cp-hint">{t("cpNoEntry")}</p>}
      </section>

      {/* Z-16 遗留协议 Shim / M-40 高刷 */}
      <section className="cp-group">
        <h4>{t("cpShimTitle")}</h4>
        <label className="cp-row">
          <input type="checkbox" checked={s.compatLegacyShim} onChange={(e) => set("compatLegacyShim", e.target.checked)} />
          <span>
            <span className="cp-label">{t("cpShimToggle")}</span>
            <span className="cp-hint">{t("cpShimHint")}</span>
          </span>
        </label>
        {shimStats && Object.keys(shimStats).length > 0 && (
          <p className="cp-hint">
            {t("cpShimHits")}: {Object.entries(shimStats).map(([k, v]) => `${k}×${v}`).join(", ")}
          </p>
        )}
        <label className="cp-row">
          <input
            type="checkbox"
            checked={s.compatHighRefresh}
            onChange={(e) => {
              set("compatHighRefresh", e.target.checked);
              applyHighRefreshTiming(144, e.target.checked);
            }}
          />
          <span>
            <span className="cp-label">{t("cpHighRefresh")}</span>
            <span className="cp-hint">{t("cpHighRefreshHint")}</span>
          </span>
        </label>
      </section>

      {/* Z-20 宿主档 / Z-21 慢速档（降级只降不升、可手动覆盖） */}
      <section className="cp-group">
        <h4>{t("cpHostTitle")}</h4>
        <div className="cp-row">
          <span>
            <span className="cp-label">{t("cpHostOverride")}</span>
            <span className="cp-hint">{hostKind ? `${t("cpHostDetected")}: ${hostKind}` : t("cpHostDetectHint")}</span>
          </span>
          <select
            value={s.compatHostOverride}
            onChange={(e) => {
              const v = e.target.value as HostOverride;
              set("compatHostOverride", v);
              void detectAndApplyHostProfile(v).then(setHostKind);
            }}
          >
            <option value="auto">Auto</option>
            <option value="native">Native</option>
            <option value="vm">VM</option>
            <option value="remote">Remote (RDP)</option>
          </select>
        </div>
        <div className="cp-row">
          <span>
            <span className="cp-label">{t("cpSlowOverride")}</span>
            <span className="cp-hint">{t("cpSlowHint")}</span>
          </span>
          <select
            value={String(s.compatSlowOverride)}
            onChange={(e) => {
              const v = Number(e.target.value) as Settings["compatSlowOverride"];
              set("compatSlowOverride", v);
              reEvalSlow(v);
            }}
          >
            <option value="-1">{t("cpSlowAuto")}</option>
            <option value="0">L0</option>
            <option value="1">L1</option>
            <option value="2">L2</option>
            <option value="3">L3</option>
          </select>
        </div>
        <p className="cp-hint">{slowTierLabel(currentSlowTier())}</p>
      </section>

      {/* M-38 UWP / M-41 驱动共存（只观察、只提示） */}
      <section className="cp-group">
        <h4>{t("cpAwareTitle")}</h4>
        <p className="cp-hint">
          {t("cpUwpCount", { count: uwpCount === null ? "—" : String(uwpCount) })}
        </p>
        <p className="cp-hint">
          {driverProcs && driverProcs.length > 0
            ? t("cpDriverRunning", { procs: driverProcs.join(", ") })
            : t("cpDriverNone")}
        </p>
      </section>
    </div>
  );
}
