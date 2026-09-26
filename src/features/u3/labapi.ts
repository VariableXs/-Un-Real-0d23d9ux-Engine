/**
 * U3 实验室装配层（AI-U3 · labapi）——引擎群与域件对实验室面板的冻结出口。
 * 单一出口点：U3Lab 只从这里取件（依赖方向单向：lab → labapi → engines/domains，
 * 不反向），避免面板与引擎散点耦合。
 */
export {
  V4_ENGINE_SELFCHECKS,
  v4EnginesSelfCheck,
  buildDomainChecklist,
  reconcileTwelveQueries,
  u3ItemCoverage,
  ExpLog,
  toImprovementItems,
} from "./engines";
export { wrapIconLabel as wrapIconLabelForLab } from "./deskicons";
