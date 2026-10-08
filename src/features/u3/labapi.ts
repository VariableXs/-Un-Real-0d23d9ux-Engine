/**
 * U3 实验室装配层（AI-U3 · labapi）——引擎群与域件对实验室面板的冻结出口。
 * 单一出口点：U3Lab 只从这里取件（依赖方向单向：lab → labapi → engines/domains，
 * 不反向），避免面板与引擎散点耦合。
 */
export {
  V4_ENGINE_SELFCHECKS,
  V5_ENGINE_SELFCHECKS,
  V7_ENGINE_SELFCHECKS,
  V8_ENGINE_SELFCHECKS,
  v4EnginesSelfCheck,
  v5EnginesSelfCheck,
  v7EnginesSelfCheck,
  v8EnginesSelfCheck,
  u3EnginesSelfCheck,
  buildDomainChecklist,
  reconcileTwelveQueries,
  u3ItemCoverage,
  ExpLog,
  toImprovementItems,
} from "./engines";
export { paneRows, auditDualForm, paneFollowSelect, paneFollowInit, type ItemFacts as PaneItemFacts, type PaneRow, type DualFormAudit, type PaneFollowState } from "./engines";
export { F078_CALENDAR_BRIDGE, crossVerifySources, CANONICAL_2026, BRIDGE_VERSION, type CrossVerifyReport, type AnchorEntry, type FlyoutCalendarData } from "./engines";
export { NotifChainLog, routeFor, chainNodeToExpLog, type NotifChain, type ChainNode, type NotifLayer } from "./engines";
export { fiveCheckStructural, U3TAB_FIVECHECK_FACTS, PATH_CHAIN_MAX, type GroupFiveCheckFacts, type FiveCheckStructuralReport } from "./engines";
export { buildWallMatrix, scanLuma, makeWallpaper, pickWithHysteresis, WALL_TARGET_LUMA_255, HYSTERESIS_HALF, type MatrixCell, type WallTexture, type SampleZone, type TextColorRt } from "./engines";
export { wrapIconLabel as wrapIconLabelForLab } from "./deskicons";
export { pendingLedger, recordWalk, blockedWorkorder, fiveCheck, MANUAL_WALK_QUERIES, DPI_SCALE_TIERS, type WalkRecord, type FiveCheckFacts } from "./engines";
export { iconContextMenu, propsGeneral, renameEnter, renameSubmit, renameEscape, renameIdle, initialSelection, splitNameExt, applyAttribute, fullTextReachable, type ItemFacts, type RenameRt, type RenameVerdict, type MenuItem } from "./engines";
export { newJob, jobRow, jobProgress, jobPause, jobResume, jobCancel, jobDone, jobStateText, undoBannerAssemble, undoBannerTick, undoBannerExtend, undoBannerRestore, preflightGate, scheduleQueue, jumpQueue, hoverDateLine, GRID_COLS, type CopyJob, type JobRow, type UndoBanner, type GateVerdict } from "./engines";
export { buildMonthGrid, panelInit, panelToggle, panelShiftMonth, panelSelect, panelTitle, holidayCoverage, type CalendarCell, type PanelState } from "./engines";
