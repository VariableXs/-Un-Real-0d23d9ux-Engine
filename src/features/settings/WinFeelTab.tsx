/**
 * AI-01 窗口手感组 — 设置页「窗口手感」标签（Z-36…Z-42、M-01…M-09 面板）。
 *
 * 红线：
 * - 全部开关默认关闭（对宿主行为零打扰）；每项注明生效范围与快捷键；
 * - 不做「记住全部窗口」类激进默认，仅提供用户显式开启的手感增强。
 */

import { useI18n } from "../../i18n";
import type { Settings } from "../../lib/settings";

export function WinFeelTab(props: { settings: Settings; onPatch: (p: Partial<Settings>) => void }): React.ReactElement {
  const { t } = useI18n();
  const s = props.settings;
  const set = <K extends keyof Settings>(k: K, v: Settings[K]): void => props.onPatch({ [k]: v } as Partial<Settings>);

  const toggle = (k: "winFeelOpacity" | "winShake" | "winGuides" | "winGestures" | "altWheelTopmost", label: string, hint: string): React.ReactElement => (
    <label className="wf-feel-row" key={k}>
      <input type="checkbox" checked={s[k]} onChange={(e) => set(k, e.target.checked)} />
      <span className="wf-feel-text">
        <span className="wf-feel-label">{label}</span>
        <span className="wf-feel-hint">{hint}</span>
      </span>
    </label>
  );

  return (
    <div className="wf-feel-tab">
      <p className="wf-feel-desc">{t("wfTabDesc")}</p>

      <section className="wf-feel-group">
        <h4>{t("wfGroupZ")}</h4>
        {toggle("winFeelOpacity", t("wfOpacityLabel"), t("wfOpacityHint"))}
        {toggle("altWheelTopmost", t("wfAltWheelLabel"), t("wfAltWheelHint"))}
        <div className="wf-feel-row">
          <span className="wf-feel-text">
            <span className="wf-feel-label">{t("wfHotzoneLabel")}</span>
            <span className="wf-feel-hint">{t("wfHotzoneHint")}</span>
          </span>
          <select
            value={String(s.desktopHotzone)}
            onChange={(e) => set("desktopHotzone", Number(e.target.value))}
          >
            <option value="0">{t("wfHotzoneOff")}</option>
            <option value="8">8 px</option>
            <option value="16">16 px</option>
            <option value="24">24 px</option>
            <option value="32">32 px</option>
          </select>
        </div>
      </section>

      <section className="wf-feel-group">
        <h4>{t("wfGroupM")}</h4>
        {toggle("winShake", t("wfShakeLabel"), t("wfShakeHint"))}
        {toggle("winGuides", t("wfGuidesLabel"), t("wfGuidesHint"))}
        {toggle("winGestures", t("wfGesturesLabel"), t("wfGesturesHint"))}
        <div className="wf-feel-row">
          <span className="wf-feel-text">
            <span className="wf-feel-label">{t("wfAltTabLabel")}</span>
            <span className="wf-feel-hint">{t("wfAltTabHint")}</span>
          </span>
          <select
            value={s.altTabFilter}
            onChange={(e) => set("altTabFilter", e.target.value as Settings["altTabFilter"])}
          >
            <option value="off">{t("wfAltTabOff")}</option>
            <option value="app">{t("wfAltTabApp")}</option>
            <option value="monitor">{t("wfAltTabMonitor")}</option>
          </select>
        </div>
        <div className="wf-feel-row">
          <span className="wf-feel-text">
            <span className="wf-feel-label">{t("wfXmouseLabel")}</span>
            <span className="wf-feel-hint">{t("wfXmouseHint")}</span>
          </span>
          <select
            value={String(s.xmouse)}
            onChange={(e) => set("xmouse", Number(e.target.value) as Settings["xmouse"])}
          >
            <option value="0">{t("wfXmouseOff")}</option>
            <option value="1">{t("wfXmouseFocus")}</option>
            <option value="2">{t("wfXmouseRaise")}</option>
          </select>
        </div>
      </section>
    </div>
  );
}
