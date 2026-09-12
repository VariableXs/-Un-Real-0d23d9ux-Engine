/**
 * AURORA-10000：AI-01~AI-05 批次，勿删。
 * BootTheaterTab.tsx — 领域01 启动与品牌剧场设置页（族0001~族0025，F00001~F00625）。
 * 25 个族各一个档位选择器：每一项 = 独立参数档（F 编号 + 名称来自 registry，与全景图逐字对齐）；
 * 默认全部「关闭」= 行为与现状分毫不差。音景/声音ID 支持本地合成试听（尊重全局静音与音量）。
 */
import type { Settings } from "../../lib/settings";
import { THEATER_FAMILIES, findTheaterItem } from "../../system/boot/theater/registry";
import { playTheaterSound } from "../../system/boot/theater/theaterSound";

export function BootTheaterTab(props: {
  settings: Settings;
  onPatch: (patch: Partial<Settings>) => void;
}): React.ReactElement {
  const sel = props.settings.bootTheater ?? {};
  const setKind = (kind: string, id: string): void => {
    props.onPatch({ bootTheater: { ...sel, [kind]: id } });
  };
  const preview = (kind: string): void => {
    const id = sel[kind];
    if (id) playTheaterSound(id, props.settings.soundVolume, props.settings.soundMuted);
  };

  return (
    <div className="tab-body">
      <p className="dim small">
        启动与品牌剧场：25 族 × 25 档（F00001~F00625），每项为独立参数档；默认全部关闭，选择后下次启动生效。
      </p>
      {THEATER_FAMILIES.map((f) => {
        const current = sel[f.kind] ?? "";
        const canPreview = f.kind === "soundscape" || f.kind === "soundId";
        return (
          <div key={f.kind} className="field">
            <span className="field-label">
              {`族${String(f.num).padStart(4, "0")} ${f.name}`}
              <span className="dim small"> · {f.attr}</span>
            </span>
            <div className="row" style={{ display: "flex", gap: "var(--sp-2)" }}>
              <select
                value={current}
                onChange={(e) => setKind(f.kind, e.target.value)}
                aria-label={`选择${f.name}档位`}
                style={{ flex: 1 }}
              >
                <option value="">关闭（默认）</option>
                {f.items.map((it) => (
                  <option key={it.id} value={it.id}>
                    {`${it.id} ${it.name}`}
                  </option>
                ))}
              </select>
              {canPreview && (
                <button type="button" onClick={() => preview(f.kind)} disabled={!current}>
                  试听
                </button>
              )}
            </div>
            {current && findTheaterItem(current) ? (
              <span className="dim small">{findTheaterItem(current)?.desc}</span>
            ) : undefined}
          </div>
        );
      })}
    </div>
  );
}
