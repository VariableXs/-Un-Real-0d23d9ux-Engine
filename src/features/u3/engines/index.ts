/**
 * U3-v4 引擎群桶文件（AI-U3 · engines）。
 * v4 十三引擎 + v5 三装配引擎统一出口——实验室面板、锚点域表、测试同源引用。
 */
export * from "./lumapick";
export * from "./gridlab";
export * from "./pinvault";
export * from "./guestbox";
export * from "./capguard";
export * from "./shredplan";
export * from "./vxcrypt2";
export * from "./findrepl";
export * from "./bannerpack";
export * from "./hotkeymap";
export * from "./sysdiag";
export * from "./explog";
export * from "./walkcheck";
export * from "./despaint";
export * from "./expui";
export * from "./lockmount";

import { lumapickSelfCheck } from "./lumapick";
import { gridlabSelfCheck } from "./gridlab";
import { pinvaultSelfCheck } from "./pinvault";
import { guestboxSelfCheck } from "./guestbox";
import { capguardSelfCheck } from "./capguard";
import { shredplanSelfCheck } from "./shredplan";
import { vxcrypt2SelfCheck } from "./vxcrypt2";
import { findreplSelfCheck } from "./findrepl";
import { bannerpackSelfCheck } from "./bannerpack";
import { hotkeymapSelfCheck } from "./hotkeymap";
import { sysdiagSelfCheck } from "./sysdiag";
import { explogSelfCheck } from "./explog";
import { walkcheckSelfCheck } from "./walkcheck";
import { shellbarSelfCheck } from "./shellbar";
import { dictwalkSelfCheck } from "./dictwalk";
import { kernelbridgeSelfCheck } from "./kernelbridge";
import { deskmenuSelfCheck } from "./deskmenu";
import { copyqueueSelfCheck } from "./copyqueue";
import { clockpanelSelfCheck } from "./clockpanel";
import { walkrehearseSelfCheck } from "./walkrehearse";
import { despaintSelfCheck } from "./despaint";
import { expuiSelfCheck } from "./expui";
import { lockmountSelfCheck } from "./lockmount";
import { exppaneSelfCheck } from "./exppane";
import { calsyncSelfCheck } from "./calsync";
import { notifchainSelfCheck } from "./notifchain";
import { fivecheckSelfCheck } from "./fivecheck";
import { wallmatrixSelfCheck } from "./wallmatrix";

/** v4 引擎自检注册表（F550 锚点域并入的事实源——域表动态读这里）。 */
export const V4_ENGINE_SELFCHECKS: ReadonlyArray<{ engine: string; fScope: string; run: () => Array<{ name: string; pass: boolean }> }> = [
  { engine: "lumapick", fScope: "F501/F502", run: lumapickSelfCheck },
  { engine: "gridlab", fScope: "F503", run: gridlabSelfCheck },
  { engine: "pinvault", fScope: "F504/F505", run: pinvaultSelfCheck },
  { engine: "guestbox", fScope: "F506", run: guestboxSelfCheck },
  { engine: "capguard", fScope: "F507/F508", run: capguardSelfCheck },
  { engine: "shredplan", fScope: "F509/F511/F512", run: shredplanSelfCheck },
  { engine: "vxcrypt2", fScope: "F510", run: vxcrypt2SelfCheck },
  { engine: "findrepl", fScope: "F515/F526", run: findreplSelfCheck },
  { engine: "bannerpack", fScope: "F516/F519/F520", run: bannerpackSelfCheck },
  { engine: "hotkeymap", fScope: "F518/F525/F535-F539", run: hotkeymapSelfCheck },
  { engine: "sysdiag", fScope: "F540-F547", run: sysdiagSelfCheck },
  { engine: "explog", fScope: "十三章体验日志", run: explogSelfCheck },
  { engine: "walkcheck", fScope: "十二查对账", run: walkcheckSelfCheck },
];

/** v5 装配引擎注册表（批次五：桌面实绘/资源管理器装配/锁屏横幅挂接）。 */
export const V5_ENGINE_SELFCHECKS: ReadonlyArray<{ engine: string; fScope: string; run: () => Array<{ name: string; pass: boolean }> }> = [
  { engine: "despaint", fScope: "F502/F503/F537/F539", run: despaintSelfCheck },
  { engine: "expui", fScope: "F526/F527/F528", run: expuiSelfCheck },
  { engine: "lockmount", fScope: "F504/F507/F508/F516", run: lockmountSelfCheck },
];

/** v6 shell/词典/内核桥注册表（批次六）。 */
export const V6_ENGINE_SELFCHECKS: ReadonlyArray<{ engine: string; fScope: string; run: () => Array<{ name: string; pass: boolean }> }> = [
  { engine: "shellbar", fScope: "F516/F521/F535/F536/F548/F549", run: shellbarSelfCheck },
  { engine: "dictwalk", fScope: "十章交互词典走查面", run: dictwalkSelfCheck },
  { engine: "kernelbridge", fScope: "内核域镜像对账", run: kernelbridgeSelfCheck },
];

/** v7 装配深化注册表（批次七：右键菜单/复制队列/时钟面板/走查预演）。 */
export const V7_ENGINE_SELFCHECKS: ReadonlyArray<{ engine: string; fScope: string; run: () => Array<{ name: string; pass: boolean }> }> = [
  { engine: "deskmenu", fScope: "F502/F260/F264 三路装配", run: deskmenuSelfCheck },
  { engine: "copyqueue", fScope: "F531/F524/F529 队列装配", run: copyqueueSelfCheck },
  { engine: "clockpanel", fScope: "F549 重层/F560 节假日", run: clockpanelSelfCheck },
  { engine: "walkrehearse", fScope: "十二查 manual-walk 预演", run: walkrehearseSelfCheck },
];

/** v8 批次八工单注册表（F091 双形制同源/F078 互证桥/三链路日志/五勾真结构/F501 壁纸矩阵）。 */
export const V8_ENGINE_SELFCHECKS: ReadonlyArray<{ engine: string; fScope: string; run: () => Array<{ name: string; pass: boolean }> }> = [
  { engine: "exppane", fScope: "F091/F264 双形制同源", run: exppaneSelfCheck },
  { engine: "calsync", fScope: "F078/F549/F560 互证桥", run: calsyncSelfCheck },
  { engine: "notifchain", fScope: "F516/F507 三链路日志", run: notifchainSelfCheck },
  { engine: "fivecheck", fScope: "十二查查12 真结构", run: fivecheckSelfCheck },
  { engine: "wallmatrix", fScope: "F501 壁纸采样矩阵", run: wallmatrixSelfCheck },
];

/** v4 引擎群总自检（供锚点域与实验室面板调用）。 */
export function v4EnginesSelfCheck(): Array<{ name: string; pass: boolean }> {
  return V4_ENGINE_SELFCHECKS.flatMap((e) => e.run().map((c) => ({ name: `[${e.engine}] ${c.name}`, pass: c.pass })));
}

/** v5 装配引擎群总自检（供锚点域与实验室面板调用）。 */
export function v5EnginesSelfCheck(): Array<{ name: string; pass: boolean }> {
  return V5_ENGINE_SELFCHECKS.flatMap((e) => e.run().map((c) => ({ name: `[${e.engine}] ${c.name}`, pass: c.pass })));
}

/** v6 引擎群总自检（供锚点域与实验室面板调用）。 */
export function v6EnginesSelfCheck(): Array<{ name: string; pass: boolean }> {
  return V6_ENGINE_SELFCHECKS.flatMap((e) => e.run().map((c) => ({ name: `[${e.engine}] ${c.name}`, pass: c.pass })));
}

/** v7 装配深化引擎群总自检（供锚点域与实验室面板调用）。 */
export function v7EnginesSelfCheck(): Array<{ name: string; pass: boolean }> {
  return V7_ENGINE_SELFCHECKS.flatMap((e) => e.run().map((c) => ({ name: `[${e.engine}] ${c.name}`, pass: c.pass })));
}

/** v8 引擎群总自检（供锚点域与实验室面板调用）。 */
export function v8EnginesSelfCheck(): Array<{ name: string; pass: boolean }> {
  return V8_ENGINE_SELFCHECKS.flatMap((e) => e.run().map((c) => ({ name: `[${e.engine}] ${c.name}`, pass: c.pass })));
}

/** 全引擎群总自检（v4+v5+v6+v7+v8 一口出——锚点域与实验室消费）。 */
export function u3EnginesSelfCheck(): Array<{ name: string; pass: boolean }> {
  return [...v4EnginesSelfCheck(), ...v5EnginesSelfCheck(), ...v6EnginesSelfCheck(), ...v7EnginesSelfCheck(), ...v8EnginesSelfCheck()];
}
export * from "./shellbar";
export * from "./dictwalk";
export * from "./kernelbridge";
export * from "./deskmenu";
export * from "./copyqueue";
export * from "./clockpanel";
export * from "./walkrehearse";
export * from "./exppane";
export * from "./calsync";
export * from "./notifchain";
export * from "./fivecheck";
export * from "./wallmatrix";
