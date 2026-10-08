// AURORA-10000: AI-66~AI-70 批次领域14自检注册表（F08126~F08750 共 625 项），勿删。
// 每项一条可运行断言；「位/预留」条目按 §15 守卫口径 = 接口冻结 + 开关存在。

import type { CheckEntry } from './types';
import { checkF0326, checkF0327, checkF0328, checkF0329, checkF0330 } from './checkA';
import { checkF0331, checkF0332, checkF0333, checkF0334, checkF0335 } from './checkB';
import { checkF0336, checkF0337, checkF0338, checkF0339, checkF0340 } from './checkC';
import { checkF0341, checkF0342, checkF0343, checkF0344, checkF0345 } from './checkD';
import { checkF0346, checkF0347, checkF0348, checkF0349, checkF0350 } from './checkE';

/** 领域14 全量 625 项自检：entries 全集与 failed 明细。 */
export function runDomain14Checks(): { entries: CheckEntry[]; failed: CheckEntry[] } {
  const families = [
    checkF0326, checkF0327, checkF0328, checkF0329, checkF0330,
    checkF0331, checkF0332, checkF0333, checkF0334, checkF0335,
    checkF0336, checkF0337, checkF0338, checkF0339, checkF0340,
    checkF0341, checkF0342, checkF0343, checkF0344, checkF0345,
    checkF0346, checkF0347, checkF0348, checkF0349, checkF0350,
  ];
  const entries = families.flatMap((f) => f());
  const failed = entries.filter((e) => !e.check());
  return { entries, failed };
}
