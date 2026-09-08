/**
 * AI-17 · 视觉语言设置页（VisionTab）
 * Z-67 动效速度滑杆（即时预览）/ Z-69 边缘热区配置 / U-57 引导重置 /
 * Z-70 帮助中心入口 / U-08 材质档位说明 / U-10 图标语言说明。
 */
import { useState } from "react";
import { useI18n } from "../../i18n";
import {
  loadMotionSpeed, saveMotionSpeed,
} from "../vision/VisionRuntime";
import {
  loadHotspotConfig, saveHotspotConfig, resetHotspotConfig,
  type EdgePosition, type HotspotAction,
} from "../../lib/hotspots";
import { resetTour, resetAllCoaches } from "../onboarding/onboarding";
import { resolveIcon } from "../../lib/iconRegistry";

const EDGES: { id: EdgePosition; labelKey: string }[] = [
  { id: "tl", labelKey: "hsCornerTL" },
  { id: "tr", labelKey: "hsCornerTR" },
  { id: "bl", labelKey: "hsCornerBL" },
  { id: "br", labelKey: "hsCornerBR" },
  { id: "top", labelKey: "hsEdgeTop" },
  { id: "left", labelKey: "hsEdgeLeft" },
  { id: "right", labelKey: "hsEdgeRight" },
];

const ACTIONS: { id: HotspotAction; labelKey: string }[] = [
  { id: "none", labelKey: "hsActionNone" },
  { id: "start-menu", labelKey: "hsActionStart" },
  { id: "quick-panel", labelKey: "hsActionQuick" },
];

export function VisionTab(): React.ReactElement {
  const { t } = useI18n();
  const [motionSpeed, setMotionSpeedState] = useState(loadMotionSpeed());
  const [hotspots, setHotspots] = useState(loadHotspotConfig());
  const Check = resolveIcon("confirm");
  const Refresh = resolveIcon("refresh");

  const patchEdge = (edge: EdgePosition, action: HotspotAction): void => {
    const next = { ...hotspots, positions: { ...hotspots.positions, [edge]: action } };
    setHotspots(next);
    saveHotspotConfig(next);
  };

  const toggleEnabled = (): void => {
    const next = { ...hotspots, enabled: !hotspots.enabled };
    setHotspots(next);
    saveHotspotConfig(next);
  };

  return (
    <div className="vision-tab" style={{ display: "flex", flexDirection: "column", gap: 20 }}>
      {/* Z-67 动效速度 */}
      <section>
        <h3 style={{ margin: "0 0 6px" }}>{t("vtMotionTitle")}</h3>
        <div className="dim" style={{ fontSize: "var(--fs-13)", marginBottom: 8 }}>{t("vtMotionDesc")}</div>
        {(["0.5", "1", "1.5"] as const).map((v) => (
          <label key={v} style={{ display: "inline-flex", alignItems: "center", gap: 6, marginRight: 16, cursor: "pointer" }}>
            <input
              type="radio"
              name="motion-speed"
              checked={motionSpeed === v}
              onChange={() => { setMotionSpeedState(v); saveMotionSpeed(v); }}
            />
            {v === "0.5" ? t("vtMotionSlow") : v === "1" ? t("vtMotionNormal") : t("vtMotionFast")}
          </label>
        ))}
      </section>

      {/* Z-69 边缘热区 */}
      <section>
        <h3 style={{ margin: "0 0 6px" }}>{t("vtHotspotTitle")}</h3>
        <div className="dim" style={{ fontSize: "var(--fs-13)", marginBottom: 8 }}>{t("vtHotspotDesc")}</div>
        <label style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 10, cursor: "pointer" }}>
          <input type="checkbox" checked={hotspots.enabled} onChange={toggleEnabled} />
          {t("vtHotspotEnable")}
        </label>
        <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fill, minmax(220px, 1fr))", gap: 8 }}>
          {EDGES.map(({ id, labelKey }) => (
            <label key={id} style={{ display: "flex", alignItems: "center", gap: 8 }}>
              <span style={{ minWidth: 84, fontSize: "var(--fs-13)" }}>{t(labelKey)}</span>
              <select
                className="mi-ctl"
                value={hotspots.positions[id] ?? "none"}
                onChange={(e) => patchEdge(id, e.target.value as HotspotAction)}
                style={{ height: "var(--ctl-input)", flex: 1 }}
              >
                {ACTIONS.map((a) => (
                  <option key={a.id} value={a.id}>{t(a.labelKey)}</option>
                ))}
              </select>
            </label>
          ))}
        </div>
        <button
          type="button"
          className="mi-ctl"
          style={{ marginTop: 10, display: "inline-flex", alignItems: "center", gap: 6, minHeight: 30, padding: "0 10px" }}
          onClick={() => { const cfg = resetHotspotConfig(); setHotspots(cfg); }}
        >
          <Refresh size={14} />{t("vtHotspotReset")}
        </button>
      </section>

      {/* U-57 引导重置 */}
      <section>
        <h3 style={{ margin: "0 0 6px" }}>{t("vtOnboardTitle")}</h3>
        <div className="dim" style={{ fontSize: "var(--fs-13)", marginBottom: 8 }}>{t("vtOnboardDesc")}</div>
        <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
          <button
            type="button"
            className="mi-ctl mi-hover-rise"
            style={{ display: "inline-flex", alignItems: "center", gap: 6, minHeight: 32, padding: "0 12px" }}
            onClick={() => { resetTour(); }}
          >
            <Check size={14} />{t("vtOnboardRetour")}
          </button>
          <button
            type="button"
            className="mi-ctl mi-hover-rise"
            style={{ display: "inline-flex", alignItems: "center", gap: 6, minHeight: 32, padding: "0 12px" }}
            onClick={() => { resetAllCoaches(); }}
          >
            <Refresh size={14} />{t("vtOnboardResetCoach")}
          </button>
        </div>
      </section>

      {/* 材质与图标说明（U-08/U-10 只读说明段） */}
      <section>
        <h3 style={{ margin: "0 0 6px" }}>{t("vtMaterialTitle")}</h3>
        <div className="dim" style={{ fontSize: "var(--fs-13)" }}>{t("vtMaterialDesc")}</div>
      </section>
    </div>
  );
}
