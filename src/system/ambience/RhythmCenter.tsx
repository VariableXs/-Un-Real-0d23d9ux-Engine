/**
 * AI-18 U-54 节律中心 — 四环进度卡片（久坐/用眼/喝水/站立）。
 *
 * 用法：AmbienceTab 与焦点舱内嵌；环 = 当前进度（0..1），满环 = 到期提醒。
 * 计时状态由 AmbienceRuntime 持有（本组件只读渲染）。
 */

import { useI18n } from "../../i18n";
import type { RhythmKind } from "./schema";
import { rhythmRings, RHYTHM_KINDS, type RhythmStateMap } from "./rhythm";
import type { AmbienceSettings } from "./schema";

const R = 14;
const CIRC = 2 * Math.PI * R;

export function RhythmCenter(props: {
  settings: AmbienceSettings;
  state: RhythmStateMap;
  nowMono: number;
}): React.ReactElement {
  const { t } = useI18n();
  const rings = rhythmRings(props.settings, props.nowMono, props.state);

  return (
    <div className="ai18-rhythm-center" data-testid="ai18-rhythm-center">
      {RHYTHM_KINDS.map((kind) => {
        const item = props.settings.rhythm[kind];
        const p = props.state[kind];
        const ring = rings[kind] ?? 0;
        return (
          <div key={kind} className="ai18-rhythm-ring" data-kind={kind}>
            <svg width="36" height="36" viewBox="0 0 36 36" aria-hidden>
              <circle cx="18" cy="18" r={R} fill="none" stroke="oklch(1 0 0 / 0.12)" strokeWidth="3" />
              <circle
                cx="18" cy="18" r={R} fill="none" stroke="var(--accent)" strokeWidth="3"
                strokeLinecap="round" strokeDasharray={CIRC}
                strokeDashoffset={CIRC * (1 - (item.enabled ? ring : 0))}
                transform="rotate(-90 18 18)"
              />
            </svg>
            <span className="ai18-rr-label">
              {t(`amb18Rhythm_${kind}`)}
              {!item.enabled && <span className="muted"> · {t("amb18RhythmOff")}</span>}
            </span>
            <span className="ai18-rr-count">
              {t("amb18RhythmCount", { n: p?.count ?? 0 })}
            </span>
          </div>
        );
      })}
    </div>
  );
}

export type { RhythmKind };
