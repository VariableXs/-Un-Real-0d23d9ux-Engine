import { useState } from "react";
import { Database, ShieldCheck, Usb, WifiOff } from "lucide-react";
import { useI18n } from "../../i18n";
import { CloseLight } from "../../components/CloseLight";
import type { Settings, TaskbarPos, ThemeId, WallpaperMode } from "../../lib/settings";

/**
 * A-5 首次体验精修（蓝图 23.5）：向导收敛为三步，总时长 ≤ 90s——
 *   0 隐私契约卡逐条确认 → 1 壁纸/主题选择（即时预览 + 5 套一键换装预设）
 *   → 2 布局偏好 + 就绪。随时可跳过；完成或跳过都会写入 wizardDone。
 * 换装资源全部本地化（零联网）；删减说明：星图叙事/多主题启动动画已弃用。
 */

const WALL_MODES: WallpaperMode[] = ["solid", "gravity", "image", "video", "hybrid", "web"];
const WALL_LABEL_KEYS: Record<WallpaperMode, string> = {
  solid: "wpSolid",
  gravity: "wpGravity",
  image: "wpImage",
  video: "wpVideo",
  hybrid: "wpHybrid",
  web: "wpWeb",
  shader: "wpShader",
  system: "wpSystem",
};

/** A-5 五套一键换装预设 = 壁纸 + 主题 + 字号 + 任务栏方位（资源全本地）。 */
export interface DressPreset {
  id: string;
  theme: ThemeId;
  wallpaperMode: WallpaperMode;
  fontSize: number;
  taskbarPos: TaskbarPos;
}

export const DRESS_PRESETS: DressPreset[] = [
  { id: "nebula", theme: "deep-space", wallpaperMode: "hybrid", fontSize: 16, taskbarPos: "bottom" },
  { id: "sunlight", theme: "paper", wallpaperMode: "solid", fontSize: 17, taskbarPos: "bottom" },
  { id: "inkstone", theme: "minimal-black", wallpaperMode: "solid", fontSize: 16, taskbarPos: "left" },
  { id: "clarity", theme: "high-contrast", wallpaperMode: "solid", fontSize: 17, taskbarPos: "bottom" },
  { id: "focus", theme: "deep-space", wallpaperMode: "web", fontSize: 15, taskbarPos: "top" },
];

const POSITIONS: TaskbarPos[] = ["bottom", "left", "right", "top"];
const POS_KEYS: Record<TaskbarPos, string> = {
  bottom: "tbPosBottom",
  left: "tbPosLeft",
  right: "tbPosRight",
  top: "tbPosTop",
};

export function WelcomeWizard(props: {
  currentWallpaper: WallpaperMode;
  onPatch: (patch: Partial<Settings>) => void;
}): React.ReactElement {
  const { t } = useI18n();
  const [step, setStep] = useState(0);
  // 隐私契约逐条确认（步骤 0 的四条全部勾选才可继续）
  const [ok, setOk] = useState<boolean[]>([false, false, false, false]);
  const allOk = ok.every(Boolean);
  const last = step === 2;

  const finish = (): void => props.onPatch({ wizardDone: true });

  return (
    <div className="wizard-overlay" role="dialog" aria-modal aria-label={t("wizHello")}>
      <div className="wizard-card">
        <div className="wizard-lights">
          <CloseLight onClose={finish} />
        </div>
        <div className="wizard-dots" aria-hidden>
          {[0, 1, 2].map((i) => (
            <span key={i} className={i === step ? "on" : ""} />
          ))}
        </div>

        {step === 0 && (
          <section className="wizard-step">
            <div className="wizard-brand">VARIABLE</div>
            <h2>{t("wizPrivacyTitle")}</h2>
            <ul className="wizard-list" style={{ listStyle: "none", paddingLeft: 0 }}>
              {[
                { icon: <WifiOff size={15} />, text: t("wizP1") },
                { icon: <Database size={15} />, text: t("wizP2") },
                { icon: <ShieldCheck size={15} />, text: t("wizP3") },
                { icon: <Usb size={15} />, text: t("wizP4") },
              ].map((item, i) => (
                <li key={i}>
                  <label className="wizard-contract">
                    <input
                      type="checkbox"
                      checked={ok[i]}
                      onChange={(e) => setOk((p) => p.map((v, j) => (j === i ? e.target.checked : v)))}
                    />
                    {item.icon}
                    <span>{item.text}</span>
                  </label>
                </li>
              ))}
            </ul>
            <p className="dim small">{t("wizPrivacyNote")}</p>
          </section>
        )}

        {step === 1 && (
          <section className="wizard-step">
            <h2>{t("wizWallTitle")}</h2>
            <p className="dim small">{t("wizWallHint")}</p>
            <div className="wizard-walls">
              {WALL_MODES.map((m) => (
                <button
                  key={m}
                  type="button"
                  className={`wizard-wall${props.currentWallpaper === m ? " on" : ""}`}
                  onClick={() => props.onPatch({ wallpaperMode: m })}
                >
                  <span className={`wizard-wall-chip wp-${m}`} aria-hidden />
                  {t(WALL_LABEL_KEYS[m])}
                </button>
              ))}
            </div>
            {/* A-5：5 套一键换装（即时预览，一键应用全套） */}
            <h3 className="wizard-sub">{t("wizDressTitle")}</h3>
            <div className="wizard-walls">
              {DRESS_PRESETS.map((p) => (
                <button
                  key={p.id}
                  type="button"
                  className={`wizard-wall${props.currentWallpaper === p.wallpaperMode ? " on" : ""}`}
                  title={t(`dress_${p.id}`)}
                  onClick={() =>
                    props.onPatch({ theme: p.theme, wallpaperMode: p.wallpaperMode, fontSize: p.fontSize, taskbarPos: p.taskbarPos })
                  }
                >
                  <span className={`wizard-wall-chip wp-${p.wallpaperMode}`} aria-hidden />
                  {t(`dress_${p.id}`)}
                </button>
              ))}
            </div>
          </section>
        )}

        {step === 2 && (
          <section className="wizard-step">
            <h2>{t("wizLayoutTitle")}</h2>
            <p className="dim small">{t("wizLayoutHint")}</p>
            <div className="wizard-walls" role="group" aria-label={t("taskbarPos")}>
              {POSITIONS.map((p) => (
                <button key={p} type="button" className="wizard-wall" onClick={() => props.onPatch({ taskbarPos: p })}>
                  {t(POS_KEYS[p])}
                </button>
              ))}
            </div>
            <div className="wizard-brand small" style={{ marginTop: 18 }}>VARIABLE</div>
            <p className="dim">{t("wizDoneBody")}</p>
          </section>
        )}

        <footer className="wizard-foot">
          <button type="button" className="btn ghost" onClick={finish}>
            {t("wizSkip")}
          </button>
          <div className="row gap8">
            {step > 0 && (
              <button type="button" className="btn ghost" onClick={() => setStep((s) => s - 1)}>
                {t("wizBack")}
              </button>
            )}
            {last ? (
              <button type="button" className="btn primary" autoFocus onClick={finish}>
                {t("wizEnter")}
              </button>
            ) : (
              <button
                type="button"
                className="btn primary"
                autoFocus
                disabled={step === 0 && !allOk}
                onClick={() => setStep((s) => s + 1)}
              >
                {step === 0 ? t("wizAgreeNext") : t("wizNext")}
              </button>
            )}
          </div>
        </footer>
      </div>
    </div>
  );
}
