// AURORA-10000: AI-66~AI-70 批次（领域14 无障碍与本地化）共享类型，勿删。

export interface CheckEntry {
  id: string;
  name: string;
  check: () => boolean;
}

/** 单次求值记忆：条目创建后首次运行缓存结果，避免状态型断言被二次求值破坏。 */
export function memoized(e: CheckEntry): CheckEntry {
  let ran = false;
  let ok = false;
  return { ...e, check: () => {
    if (!ran) {
      try {
        ok = e.check();
      } catch {
        ok = false;
      }
      ran = true;
    }
    return ok;
  } };
}

/** §15 守卫口径：「位/预留」条目交付标准 = 接口冻结 + 开关存在。 */
export interface FeatureSlot {
  id: string;
  name: string;
  reserved: boolean;
  enabled: boolean;
}

export const clamp = (x: number, lo: number, hi: number) => (x < lo ? lo : x > hi ? hi : x);
