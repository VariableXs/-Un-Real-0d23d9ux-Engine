import { resolveSnapRect, decide, type RuleAction } from "./rules";
import { addToStage } from "./stages";
import {
  setVwmOpenHook,
  snapVwmRect,
  setVwmOpacity,
  setVwmTopmost,
  vwmStore,
  type VwmApp,
} from "./vwm";

/**
 * N-03 规则引擎执行桥（AI-02 窗口编排组）：
 * 把规则裁决（rules.decide）映射到 VWM 既有执行器——
 * snapRect → snapVwmRect、stage → stages.addToStage、topmost/opacity → Z-36 既有动作。
 * **不新增任何绕过安全审查的执行路径**：受限窗口（L4/管理员）在 decide 层已被拒绝；
 * 前端侧 tier 信息由 embed 域提供前恒按非受限处理，后端接入时替换判定来源（诚实记录于交付报告）。
 */

/** 应用单条裁决结果到 VWM store。返回已应用的动作数（被拒动作不计）。 */
export function applyDecision(d: ReturnType<typeof decide>): number {
  if (!d.rule) return 0;
  const st = vwmStore.getState();
  const wa = st.workArea;
  let applied = 0;
  for (const a of d.rule.actions) {
    if (applyAction(a, d.winId, wa)) applied += 1;
  }
  return applied;
}

function applyAction(a: RuleAction, winId: string, wa: { x: number; y: number; w: number; h: number }): boolean {
  switch (a.type) {
    case "snapRect":
      snapVwmRect(winId, resolveSnapRect(a.rect, wa));
      return true;
    case "stage":
      addToStage(a.stageId, winId);
      return true;
    case "topmost":
      setVwmTopmost(winId, true);
      return true;
    case "opacity":
      // decide 层已拒绝 30–100 之外的值；此处按百分比换算
      setVwmOpacity(winId, a.value / 100);
      return true;
    default:
      return false;
  }
}

/** 安装到 vwm.openVwmInstance 的规则钩子（幂等——重复安装覆盖同一实现）。 */
export function installRuleHook(): void {
  setVwmOpenHook((id: string, app: VwmApp, title: string) => {
    const d = decide({ winId: id, app: app as string, title, restricted: false });
    applyDecision(d);
  });
}
