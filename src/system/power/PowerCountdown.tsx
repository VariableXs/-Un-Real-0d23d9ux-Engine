import { useEffect } from "react";
import { useI18n } from "../../i18n";
import { pushOverlay, popOverlay } from "../../state/uiStore";
import { useStore } from "../../lib/store";
import { cancelPowerCountdown, powerGateStore } from "./powerGate";

/**
 * AI-20 V-92：关机倒计时全屏轻遮罩（不吓人但不可忽视）。
 * 环形进度 + 「取消」大按钮；Esc = 取消（M-34 浮层栈协议）。
 * 零渲染负担：无倒计时时返回 null（不挂任何 DOM）。
 */
const OVERLAY_ID = "power-countdown";

export function PowerCountdown(): React.ReactElement | null {
  const { t } = useI18n();
  const pending = useStore(powerGateStore, (s) => s.pending);

  useEffect(() => {
    if (pending) pushOverlay(OVERLAY_ID);
    else popOverlay(OVERLAY_ID);
    return () => popOverlay(OVERLAY_ID);
  }, [pending]);

  useEffect(() => {
    if (!pending) return;
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === "Escape") {
        e.stopPropagation();
        cancelPowerCountdown();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [pending]);

  if (!pending) return null;
  const pct = pending.remainSec / 10;
  const R = 54;
  const C = 2 * Math.PI * R;
  return (
    <div className="power-gate-overlay" data-testid="power-gate" role="alertdialog" aria-live="assertive">
      <div className="power-gate-card">
        <svg width={132} height={132} viewBox="0 0 132 132" aria-hidden className="power-gate-ring">
          <circle cx={66} cy={66} r={R} className="power-gate-track" />
          <circle
            cx={66}
            cy={66}
            r={R}
            className="power-gate-arc"
            strokeDasharray={C}
            strokeDashoffset={C * (1 - pct)}
            transform="rotate(-90 66 66)"
          />
          <text x={66} y={74} className="power-gate-num">{pending.remainSec}</text>
        </svg>
        <h3>{pending.action === "shutdown" ? t("v92ShutdownTitle") : t("v92RebootTitle")}</h3>
        <p className="dim small">{t("v92Body")}</p>
        <button type="button" className="btn primary power-gate-cancel" data-testid="power-gate-cancel" onClick={() => cancelPowerCountdown()}>
          {t("v92Cancel")}
        </button>
      </div>
    </div>
  );
}
