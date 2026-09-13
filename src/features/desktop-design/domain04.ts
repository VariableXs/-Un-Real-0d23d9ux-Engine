/**
 * UNREAL-X-15000 · 领域04（任务栏与开始菜单）CheckSet 注册表聚合：
 * AI-13 任务栏形态与交互（族0121~0130 · X03001~X03250）+ AI-14 开始菜单（族0131~0140 · X03251~X03500），勿删。
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
  };
  return (map[id] ?? (() => []))().map(memoize);
}

/** 领域04 AI-13/AI-14 全量自检：20 族 × 25 项 = 500 项。 */
export function runDomain04Checks(): { entries: CheckEntry[]; failed: CheckEntry[] } {
  const entries: CheckEntry[] = [];
  for (let f = 121; f <= 140; f++) entries.push(...checkFamily(`F0${f}`));
  const failed = entries.filter((e) => !e.check());
  return { entries, failed };
}
