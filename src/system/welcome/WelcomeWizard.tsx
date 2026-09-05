import { useState } from "react";
import {
  Code2, Database, Network, PenLine, ShieldCheck, Sparkles, Usb, WifiOff,
} from "lucide-react";
import { useI18n } from "../../i18n";
import { CloseLight } from "../../components/CloseLight";
import type { Settings, WallpaperMode } from "../../lib/settings";

/**
 * 批次A：首次启动欢迎向导（仅桌面窗口，wizardDone=false 时显示）。
 * 四步：欢迎 → 隐私承诺 → 壁纸选择（实时预览：选完立即生效，向导卡片半透明可看到桌面）
 * → 就绪。随时可跳过；完成或跳过都会写入 wizardDone，之后不再出现。
 * 所有文案走词典（zh/en），零网络、数据仅本机。
 */

const WALL_MODES: WallpaperMode[] = ["solid", "gravity", "image", "video", "hybrid", "web"];
const WALL_LABEL_KEYS: Record<WallpaperMode, string> = {
  solid: "wpSolid",
  gravity: "wpGravity",
  image: "wpImage",
  video: "wpVideo",
  hybrid: "wpHybrid",
  web: "wpWeb",
  system: "wpSystem",
};

export function WelcomeWizard(props: {
  currentWallpaper: WallpaperMode;
  onPatch: (patch: Partial<Settings>) => void;
}): React.ReactElement {
  const { t } = useI18n();
  const [step, setStep] = useState(0);
  const last = step === 3;

  const finish = (): void => props.onPatch({ wizardDone: true });

  return (
    <div className="wizard-overlay" role="dialog" aria-modal aria-label={t("wizHello")}>
      <div className="wizard-card">
        <div className="wizard-lights">
          <CloseLight onClose={finish} />
        </div>
        <div className="wizard-dots" aria-hidden>
          {[0, 1, 2, 3].map((i) => (
            <span key={i} className={i === step ? "on" : ""} />
          ))}
        </div>

        {step === 0 && (
          <section className="wizard-step">
            <div className="wizard-brand">VARIABLE</div>
            <h2>{t("wizHello")}</h2>
            <p className="dim">{t("wizIntro")}</p>
            <ul className="wizard-list">
              <li><PenLine size={15} /> {t("wizAppWrite")}</li>
              <li><Network size={15} /> {t("wizAppMind")}</li>
              <li><Code2 size={15} /> {t("wizAppCode")}</li>
              <li><Sparkles size={15} /> {t("wizAppFate")}</li>
            </ul>
          </section>
        )}

        {step === 1 && (
          <section className="wizard-step">
            <h2>{t("wizPrivacyTitle")}</h2>
            <ul className="wizard-list">
              <li><WifiOff size={15} /> {t("wizP1")}</li>
              <li><Database size={15} /> {t("wizP2")}</li>
              <li><ShieldCheck size={15} /> {t("wizP3")}</li>
              <li><Usb size={15} /> {t("wizP4")}</li>
            </ul>
            <p className="dim small">{t("wizPrivacyNote")}</p>
          </section>
        )}

        {step === 2 && (
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
          </section>
        )}

        {step === 3 && (
          <section className="wizard-step">
            <div className="wizard-brand small">VARIABLE</div>
            <h2>{t("wizDoneTitle")}</h2>
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
              <button type="button" className="btn primary" autoFocus onClick={() => setStep((s) => s + 1)}>
                {t("wizNext")}
              </button>
            )}
          </div>
        </footer>
      </div>
    </div>
  );
}
