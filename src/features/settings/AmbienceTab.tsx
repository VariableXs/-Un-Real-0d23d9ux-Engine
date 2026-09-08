/**
 * AI-18 氛围与个性化组 — 设置页「氛围」标签（U-49/50/53/54、N-33、M-64..M-72、V-71..V-80）。
 *
 * 红线：
 * - 全部默认关闭或等于现状；档位改动即时预览（AmbienceRuntime 注入 CSS 变量）；
 * - 音景试听即点即停（Web Audio 程序化生成，零音频资产）；
 * - M-64 主色采样 / V-80 对比度守护内嵌主题工坊逻辑（本地计算）。
 */

import { useEffect, useState } from "react";
import { useI18n } from "../../i18n";
import type { Settings } from "../../lib/settings";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import {
  SOUNDSCAPE_SCENES,
  startScene,
  stopScene,
  activeScenes,
} from "../../lib/soundscape";
import type {
  AmbienceSettings,
  DaySlot,
  GlowMode,
  HoverLatency,
  IconSizeTier,
  RhythmKind,
  ScreensaverMode,
  UiDensity,
  UiRadius,
  FocusRingStyle,
} from "../../system/ambience/schema";
import { RhythmCenter } from "../../system/ambience/RhythmCenter";
import { rhythmStore } from "../../system/ambience/AmbienceRuntime";
import { DAY_SLOTS } from "../../system/ambience/dayAround";
import { CURATED_WALLPAPERS, nextIndex } from "../../system/ambience/curated";
import { sampleAccentCandidates, oklchToCss, oklchToHex, applyAccent } from "../../system/ambience/wallpaperAccent";
import { guardAccent } from "../../lib/contrastGuard";
import { toAssetUrl } from "../background/CosmicBackground";
import { useStore } from "../../lib/store";

export function AmbienceTab(props: { settings: Settings; onPatch: (p: Partial<Settings>) => void }): React.ReactElement {
  const { t, lang } = useI18n();
  const amb = props.settings.ambience;
  const set = (patch: Partial<AmbienceSettings>): void =>
    props.onPatch({ ambience: { ...amb, ...patch } });

  // U-49 试听状态
  const [playing, setPlaying] = useState<string[]>([]);
  useEffect(() => {
    const id = window.setInterval(() => setPlaying(activeScenes()), 500);
    return () => window.clearInterval(id);
  }, []);

  // U-54 节律环预览（读共享 store）
  const rhythmTick = useStore(rhythmStore, (s) => s.tick);
  const rhythmState = useStore(rhythmStore, (s) => s.state);
  const [nowMono] = useState(() => performance.now());

  // M-64 主色采样候选（OKLCH 元组；渲染时转 CSS）
  const [candidates, setCandidates] = useState<[number, number, number][]>([]);
  useEffect(() => {
    const path = props.settings.wallpaperMode === "image" || props.settings.wallpaperMode === "hybrid"
      ? props.settings.customBg.imagePath
      : "";
    if (!path) {
      setCandidates([]);
      return;
    }
    let alive = true;
    void sampleAccentCandidates(toAssetUrl(path)).then((cands) => {
      if (alive) setCandidates(cands);
    }).catch(() => {
      if (alive) setCandidates([]);
    });
    return () => {
      alive = false;
    };
  }, [props.settings.wallpaperMode, props.settings.customBg.imagePath]);

  const pickDir = (slot: DaySlot) =>
    void (async () => {
      const p = await openFileDialog({ multiple: false });
      if (typeof p !== "string") return;
      set({ dayAround: { ...amb.dayAround, dirs: { ...amb.dayAround.dirs, [slot]: p } } });
    })();

  const sel = <T extends string>(value: T, onChange: (v: T) => void, options: { id: T; label: string }[]): React.ReactElement => (
    <select className="text-input" value={value} onChange={(e) => onChange(e.target.value as T)}>
      {options.map((o) => (
        <option key={o.id} value={o.id}>{o.label}</option>
      ))}
    </select>
  );

  return (
    <div className="ai18-tab">
      {/* ---------------- U-49 氛围音景引擎 ---------------- */}
      <section>
        <h3>{t("amb18ScapeTitle")}</h3>
        <p className="dim small">{t("amb18ScapeHint")}</p>
        <div className="ai18-grid2">
          {SOUNDSCAPE_SCENES.map((sc) => (
            <div key={sc} className="row gap8">
              <span style={{ minWidth: 72 }}>{t(`amb18Scene_${sc}`)}</span>
              <input
                type="range"
                min={0}
                max={1}
                step={0.05}
                value={amb.soundscapeVolumes[sc]}
                onChange={(e) => {
                  const v = Number(e.target.value);
                  set({ soundscapeVolumes: { ...amb.soundscapeVolumes, [sc]: v } });
                  if (playing.includes(sc)) startScene(sc, v);
                }}
              />
              <button
                type="button"
                className={`btn tiny${playing.includes(sc) ? " primary" : " ghost"}`}
                onClick={() => {
                  if (playing.includes(sc)) stopScene(sc, amb.soundscapeVolumes[sc]);
                  else startScene(sc, amb.soundscapeVolumes[sc]);
                }}
              >
                {playing.includes(sc) ? t("amb18ScapeStop") : t("amb18ScapePlay")}
              </button>
            </div>
          ))}
        </div>
        <label className="row gap8" style={{ marginTop: 8 }}>
          <span>{t("amb18ScapeSleep")}</span>
          <input
            type="number"
            min={0}
            max={480}
            className="text-input"
            style={{ width: 80 }}
            value={amb.soundscapeSleepMin}
            onChange={(e) => set({ soundscapeSleepMin: Math.max(0, Math.min(480, Number(e.target.value) || 0)) })}
          />
          <span className="dim small">{t("amb18ScapeSleepUnit")}</span>
        </label>
      </section>

      {/* ---------------- U-53 环境辉光 ---------------- */}
      <section>
        <h3>{t("amb18GlowTitle")}</h3>
        <div className="row gap8 wrap">
          {(["off", "static", "breath"] as const).map((m: GlowMode) => (
            <label key={m} className="row gap8">
              <input type="radio" name="ai18-glow" checked={amb.glow === m} onChange={() => set({ glow: m })} />
              <span>{t(`amb18Glow_${m}`)}</span>
            </label>
          ))}
        </div>
        <p className="dim small">{t("amb18GlowHint")}</p>
      </section>

      {/* ---------------- U-54 节律助手 ---------------- */}
      <section>
        <h3>{t("amb18RhythmTitle")}</h3>
        <div className="ai18-grid2">
          {(["sitting", "eye", "drink", "stretch"] as const).map((k: RhythmKind) => {
            const item = amb.rhythm[k];
            return (
              <div key={k} className="row gap8 wrap">
                <label className="row gap8">
                  <input
                    type="checkbox"
                    checked={item.enabled}
                    onChange={(e) => set({ rhythm: { ...amb.rhythm, [k]: { ...item, enabled: e.target.checked } } })}
                  />
                  <span>{t(`amb18Rhythm_${k}`)}</span>
                </label>
                <input
                  type="number"
                  min={1}
                  max={240}
                  className="text-input"
                  style={{ width: 70 }}
                  value={item.intervalMin}
                  onChange={(e) => set({ rhythm: { ...amb.rhythm, [k]: { ...item, intervalMin: Math.max(1, Math.min(240, Number(e.target.value) || 1)) } } })}
                />
                {sel(item.notify, (v) => set({ rhythm: { ...amb.rhythm, [k]: { ...item, notify: v } } }), [
                  { id: "count" as const, label: t("amb18RhythmNCount") },
                  { id: "badge" as const, label: t("amb18RhythmNBadge") },
                  { id: "notify" as const, label: t("amb18RhythmNNotify") },
                ])}
              </div>
            );
          })}
        </div>
        <RhythmCenter key={rhythmTick} settings={amb} state={rhythmState} nowMono={nowMono} />
      </section>

      {/* ---------------- N-33 情绪引擎 ---------------- */}
      <section>
        <h3>{t("amb18MoodTitle")}</h3>
        <label className="row gap8">
          <input type="checkbox" checked={amb.mood.enabled} onChange={(e) => set({ mood: { ...amb.mood, enabled: e.target.checked } })} />
          <span>{t("amb18MoodEnable")}</span>
        </label>
        {amb.mood.enabled && (
          <div className="ai18-grid2" style={{ marginTop: 8 }}>
            <label className="row gap8">
              <span>{t("amb18MoodArousal")}</span>
              <input
                type="range"
                min={-1}
                max={1}
                step={0.1}
                value={amb.mood.manualArousal}
                onChange={(e) => set({ mood: { ...amb.mood, manualArousal: Number(e.target.value) } })}
              />
            </label>
            <label className="row gap8">
              <span>{t("amb18MoodFocus")}</span>
              <input
                type="range"
                min={0}
                max={1}
                step={0.05}
                value={amb.mood.manualFocus}
                onChange={(e) => set({ mood: { ...amb.mood, manualFocus: Number(e.target.value) } })}
              />
            </label>
          </div>
        )}
        <p className="dim small">{t("amb18MoodHint")}</p>
      </section>

      {/* ---------------- M-64 壁纸主色采样 + V-80 对比度守护 ---------------- */}
      <section>
        <h3>{t("amb18AccentTitle")}</h3>
        {candidates.length > 0 ? (
          <div className="ai18-accent-candidates">
            {candidates.map((c) => {
              const css = oklchToCss(c[0], c[1], c[2]);
              const verdict = guardAccent(oklchToHex(c[0], c[1], c[2]), "#f2f2f2", "#1a1a22");
              return (
                <button
                  key={css}
                  type="button"
                  className="ai18-accent-chip"
                  style={{ background: css }}
                  title={verdict.pass ? t("amb18AccentOk") : t("amb18AccentFail")}
                  onClick={() => {
                    if (!verdict.pass) return;
                    applyAccent(c);
                  }}
                />
              );
            })}
          </div>
        ) : (
          <p className="dim small">{t("amb18AccentNone")}</p>
        )}
        <p className="dim small">{t("amb18AccentHint")}</p>
      </section>

      {/* ---------------- M-65 昼夜壁纸组 ---------------- */}
      <section>
        <h3>{t("amb18DayTitle")}</h3>
        <label className="row gap8">
          <input type="checkbox" checked={amb.dayAround.enabled} onChange={(e) => set({ dayAround: { ...amb.dayAround, enabled: e.target.checked } })} />
          <span>{t("amb18DayEnable")}</span>
        </label>
        {amb.dayAround.enabled && (
          <div style={{ marginTop: 8 }}>
            {DAY_SLOTS.map((slot) => (
              <div key={slot} className="row gap8" style={{ marginBottom: 6 }}>
                <span className="small" style={{ minWidth: 64 }}>{t(`amb18Day_${slot}`)}</span>
                <input className="text-input flex-1" readOnly value={amb.dayAround.dirs[slot]} placeholder={t("amb18DirPlaceholder")} />
                <button type="button" className="btn ghost tiny" onClick={() => pickDir(slot)}>{t("chooseFile")}</button>
              </div>
            ))}
            <p className="dim small">{t("amb18DayBoundaries")}: {amb.dayAround.boundaries.join(" / ")} {t("amb18DayHour")}</p>
          </div>
        )}
      </section>

      {/* ---------------- M-68 屏保 / M-69 简报 / M-71 悬停 / M-72 会话恢复 ---------------- */}
      <section>
        <h3>{t("amb18IdleTitle")}</h3>
        <div className="ai18-grid2">
          <div className="row gap8">
            <span>{t("amb18Screensaver")}</span>
            {sel(amb.screensaver, (v: ScreensaverMode) => set({ screensaver: v }), [
              { id: "off" as const, label: t("amb18SsOff") },
              { id: "clock" as const, label: t("amb18SsClock") },
              { id: "black" as const, label: t("amb18SsBlack") },
            ])}
          </div>
          <label className="row gap8">
            <span>{t("amb18SsIdle")}</span>
            <input
              type="number"
              min={1}
              max={120}
              className="text-input"
              style={{ width: 70 }}
              value={amb.screensaverIdleMin}
              onChange={(e) => set({ screensaverIdleMin: Math.max(1, Math.min(120, Number(e.target.value) || 10)) })}
            />
            <span className="dim small">{t("amb18ScapeSleepUnit")}</span>
          </label>
          <label className="row gap8">
            <input type="checkbox" checked={amb.briefing} onChange={(e) => set({ briefing: e.target.checked })} />
            <span>{t("amb18BriefingEnable")}</span>
          </label>
          <div className="row gap8">
            <span>{t("amb18Hover")}</span>
            {sel(amb.hoverLatency, (v: HoverLatency) => set({ hoverLatency: v }), [
              { id: "fast" as const, label: t("amb18HoverFast") },
              { id: "std" as const, label: t("amb18HoverStd") },
              { id: "slow" as const, label: t("amb18HoverSlow") },
            ])}
          </div>
          <label className="row gap8">
            <input type="checkbox" checked={amb.sessionRestore} onChange={(e) => set({ sessionRestore: e.target.checked })} />
            <span>{t("amb18SessionEnable")}</span>
          </label>
        </div>
      </section>

      {/* ---------------- V-71 精选轮换 / V-72 壁纸滤镜 / V-73 农历 ---------------- */}
      <section>
        <h3>{t("amb18WallpaperTitle")}</h3>
        <div className="ai18-grid2">
          <label className="row gap8">
            <input
              type="checkbox"
              checked={amb.curated.on}
              onChange={(e) => set({ curated: { ...amb.curated, on: e.target.checked } })}
            />
            <span>{t("amb18CuratedEnable")}</span>
          </label>
          {amb.curated.on && (
            <button
              type="button"
              className="btn ghost tiny"
              onClick={() => set({ curated: { ...amb.curated, index: nextIndex(amb.curated.index, CURATED_WALLPAPERS.length) } })}
            >
              {t("amb18CuratedNext")}
            </button>
          )}
          <label className="row gap8">
            <span>{t("amb18WpSat")}（{amb.wallpaperFilter.saturation}%）</span>
            <input
              type="range"
              min={60}
              max={100}
              value={amb.wallpaperFilter.saturation}
              onChange={(e) => set({ wallpaperFilter: { ...amb.wallpaperFilter, saturation: Number(e.target.value) } })}
            />
          </label>
          <label className="row gap8">
            <span>{t("amb18WpBright")}（{amb.wallpaperFilter.brightness}%）</span>
            <input
              type="range"
              min={80}
              max={100}
              value={amb.wallpaperFilter.brightness}
              onChange={(e) => set({ wallpaperFilter: { ...amb.wallpaperFilter, brightness: Number(e.target.value) } })}
            />
          </label>
          <label className="row gap8">
            <input type="checkbox" checked={amb.lunarCalendar} onChange={(e) => set({ lunarCalendar: e.target.checked })} />
            <span>{t("amb18LunarEnable")}</span>
          </label>
        </div>
        {amb.curated.on &&
          (() => {
            const wp = CURATED_WALLPAPERS[amb.curated.index];
            if (!wp) return null;
            return (
              <p className="dim small">
                {lang === "en" ? wp.en : wp.zh}
                {" — "}
                {lang === "en" ? wp.storyEn : wp.storyZh}
              </p>
            );
          })()}
      </section>

      {/* ---------------- V-74..V-78 个性化档位 ---------------- */}
      <section>
        <h3>{t("amb18TierTitle")}</h3>
        <div className="ai18-grid2">
          <div className="row gap8">
            <span>{t("amb18Density")}</span>
            {sel(amb.uiDensity, (v: UiDensity) => set({ uiDensity: v }), [
              { id: "comfort" as const, label: t("amb18DensityComfort") },
              { id: "compact" as const, label: t("amb18DensityCompact") },
            ])}
          </div>
          <div className="row gap8">
            <span>{t("amb18Radius")}</span>
            {sel(amb.uiRadius, (v: UiRadius) => set({ uiRadius: v }), [
              { id: "round" as const, label: t("amb18RadiusRound") },
              { id: "small" as const, label: t("amb18RadiusSmall") },
              { id: "sharp" as const, label: t("amb18RadiusSharp") },
            ])}
          </div>
          <label className="row gap8">
            <span>{t("amb18Font")}</span>
            <input
              className="text-input flex-1"
              value={amb.uiFont}
              placeholder={t("amb18FontPlaceholder")}
              onChange={(e) => set({ uiFont: e.target.value.slice(0, 120) })}
            />
          </label>
          <div className="row gap8">
            <span>{t("amb18FocusRing")}</span>
            {sel(amb.focusRing, (v: FocusRingStyle) => set({ focusRing: v }), [
              { id: "system" as const, label: t("amb18RingSystem") },
              { id: "box" as const, label: t("amb18RingBox") },
              { id: "underline" as const, label: t("amb18RingUnderline") },
            ])}
          </div>
          <div className="row gap8">
            <span>{t("amb18IconTier")}</span>
            {sel(String(amb.iconTier), (v: string) => set({ iconTier: (Number(v) as IconSizeTier) }), [
              { id: "16", label: "16px" },
              { id: "20", label: "20px" },
              { id: "24", label: "24px" },
            ])}
          </div>
        </div>
        <p className="dim small">{t("amb18TierHint")}</p>
      </section>
    </div>
  );
}
