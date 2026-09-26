/**
 * U3-v4 引擎群桶文件（AI-U3 · engines）。
 * 十三引擎统一出口——实验室面板、锚点域表、测试同源引用。
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

/** v4 引擎群总自检（供锚点域与实验室面板调用）。 */
export function v4EnginesSelfCheck(): Array<{ name: string; pass: boolean }> {
  return V4_ENGINE_SELFCHECKS.flatMap((e) => e.run().map((c) => ({ name: `[${e.engine}] ${c.name}`, pass: c.pass })));
}
