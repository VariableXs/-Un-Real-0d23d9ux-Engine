import { useEffect, useState } from "react";
import { useI18n } from "../../i18n";
import { addToStage, createStage, deleteStage, loadStages, planStageActivate, type StageGroup } from "./stages";
import { focusVwmWin, minimizeVwmWin, vwmStore } from "./vwm";
import { multiSelected } from "./multiselect";

/**
 * N-02 舞台管理器侧幕（AI-02 窗口编排组）：
 * 左缘竖排「侧幕气泡」——每个舞台组一枚，点击整组上台（成员聚焦、其余收拢）。
 * - 热区与贴靠热区错开 24px（气泡 left=28px）；
 * - 全屏窗口激活时侧幕自动隐藏由 CSS 层承担；
 * - 多选（V-24）≥2 窗口时出现「存为舞台组」快捷入口。
 */

/** 应用「整组上台」计划到 VWM store（侧幕点击 / Ctrl+Shift+←/→ 轮换共用）。 */
export function activateStage(stageId: string): void {
  const s = vwmStore.getState();
  const plan = planStageActivate(stageId, s.wins);
  for (const id of plan.sideline) minimizeVwmWin(id);
  for (const id of plan.focusOrder) focusVwmWin(id);
}

export function StageRail(props: { activeStageId: string | null; onActivate: (id: string) => void }): React.ReactElement | null {
  const { t } = useI18n();
  const [version, setVersion] = useState(0);
  const selCount = multiSelected().length;
  useEffect(() => vwmStore.subscribe(() => setVersion((v) => v + 1)), []);
  const groups: StageGroup[] = loadStages();
  if (groups.length === 0 && selCount < 2) return null;
  void version;

  const saveSelectionAsStage = (): void => {
    const ids = multiSelected();
    if (ids.length < 2) return;
    const g = createStage(`${t("stageDefaultName")} ${groups.length + 1}`);
    for (const id of ids) addToStage(g.id, id);
    setVersion((v) => v + 1);
  };

  return (
    <div className="vwm-stage-rail" role="navigation" aria-label={t("orchTabStages")}>
      {groups.map((g) => (
        <div key={g.id} className={`vwm-stage-bubble${props.activeStageId === g.id ? " on" : ""}`}>
          <button
            type="button"
            title={g.name}
            onClick={() => {
              activateStage(g.id);
              props.onActivate(g.id);
            }}
          >
            <span className="vwm-stage-name">{g.name}</span>
            <span className="vwm-stage-count">{g.members.length}</span>
          </button>
          <button
            type="button"
            className="vwm-stage-del"
            aria-label={`${t("stageDelete")} ${g.name}`}
            onClick={() => {
              deleteStage(g.id);
              setVersion((v) => v + 1);
            }}
          >
            ×
          </button>
        </div>
      ))}
      {selCount >= 2 && (
        <button type="button" className="vwm-stage-new" onClick={saveSelectionAsStage}>
          {t("stageSaveSelection")}
        </button>
      )}
    </div>
  );
}
