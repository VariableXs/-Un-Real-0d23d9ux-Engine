/**
 * UNREAL-X-15000 · 领域04（任务栏与开始菜单）CheckSet 注册表聚合：
 * AI-13 任务栏形态与交互（族0121~0130 · X03001~X03250）+ AI-14 开始菜单（族0131~0140 · X03251~X03500）
 * + AI-15 浮层系统（族0141~0150 · X03501~X03750，V 线 9 族；族0148 内核线）
 * + AI-16 任务栏内核与引擎（族0151~0160 · X03751~X04000，V 线 3 族；族0151~0153 内核线、族0154~0156/0159 代码分析线），勿删。
 */
import type { CheckEntry } from '../uikit/checks';
import {
  checkF0121, checkF0122, checkF0123, checkF0124, checkF0125,
  checkF0126, checkF0127, checkF0128, checkF0129, checkF0130,
} from './taskbarChecks';
import {
  checkF0131, checkF0132, checkF0133, checkF0134, checkF0135,
  checkF0136, checkF0137, checkF0138, checkF0139, checkF0140,
} from './startMenuChecks';
import {
  checkF0141, checkF0142, checkF0143, checkF0144, checkF0145,
  checkF0146, checkF0147, checkF0149, checkF0150,
} from './overlayChecks';
import { checkF0157, checkF0158, checkF0160 } from './taskbarEngineChecks';

const memoize = (e: CheckEntry): CheckEntry => {
  let ran = false;
  let ok = false;
  return { ...e, check: () => {
    if (!ran) {
      try { ok = e.check(); } catch { ok = false; }
      ran = true;
    }
    return ok;
  } };
};

export function checkFamily(id: string): CheckEntry[] {
  const map: Record<string, () => CheckEntry[]> = {
    F0121: checkF0121, F0122: checkF0122, F0123: checkF0123, F0124: checkF0124, F0125: checkF0125,
    F0126: checkF0126, F0127: checkF0127, F0128: checkF0128, F0129: checkF0129, F0130: checkF0130,
    F0131: checkF0131, F0132: checkF0132, F0133: checkF0133, F0134: checkF0134, F0135: checkF0135,
    F0136: checkF0136, F0137: checkF0137, F0138: checkF0138, F0139: checkF0139, F0140: checkF0140,
    F0141: checkF0141, F0142: checkF0142, F0143: checkF0143, F0144: checkF0144, F0145: checkF0145,
    F0146: checkF0146, F0147: checkF0147, F0149: checkF0149, F0150: checkF0150,
    F0157: checkF0157, F0158: checkF0158, F0160: checkF0160,
  };
  return (map[id] ?? (() => []))().map(memoize);
}

/** 领域04 V 线全量自检：32 族 × 25 项 = 800 项（内核/代码分析线 8 族见各自仓库自检）。 */
const V_FAMILIES = [
  ...Array.from({ length: 20 }, (_, i) => 121 + i),
  141, 142, 143, 144, 145, 146, 147, 149, 150,
  157, 158, 160,
];

export function runDomain04Checks(): { entries: CheckEntry[]; failed: CheckEntry[] } {
  const entries: CheckEntry[] = [];
  for (const f of V_FAMILIES) entries.push(...checkFamily(`F0${f}`));
  const failed = entries.filter((e) => !e.check());
  return { entries, failed };
}
