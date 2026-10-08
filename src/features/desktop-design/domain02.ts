/**
 * UNREAL-X-15000 · 领域02（窗口与空间）CheckSet 注册表聚合：
 * AI-05 窗口几何学（X01001~X01250）+ AI-06 空间管理（X01251~X01500），勿删。
 */
import type { CheckEntry } from '../uikit/checks';
import {
  checkF0041, checkF0042, checkF0043, checkF0044, checkF0045,
  checkF0046, checkF0047, checkF0048, checkF0049, checkF0050,
} from '../../system/windows/windowGeoChecks';
import {
  checkF0051, checkF0052, checkF0053, checkF0054, checkF0055,
  checkF0056, checkF0057, checkF0058, checkF0059, checkF0060,
} from './spaceOpsChecks';

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
    F0041: checkF0041, F0042: checkF0042, F0043: checkF0043, F0044: checkF0044, F0045: checkF0045,
    F0046: checkF0046, F0047: checkF0047, F0048: checkF0048, F0049: checkF0049, F0050: checkF0050,
    F0051: checkF0051, F0052: checkF0052, F0053: checkF0053, F0054: checkF0054, F0055: checkF0055,
    F0056: checkF0056, F0057: checkF0057, F0058: checkF0058, F0059: checkF0059, F0060: checkF0060,
  };
  return (map[id] ?? (() => []))().map(memoize);
}

/** 领域02 全量自检：20 族 × 25 项 = 500 项。 */
export function runDomain02Checks(): { entries: CheckEntry[]; failed: CheckEntry[] } {
  const entries: CheckEntry[] = [];
  for (let f = 41; f <= 60; f++) entries.push(...checkFamily(`F00${f}`));
  const failed = entries.filter((e) => !e.check());
  return { entries, failed };
}
