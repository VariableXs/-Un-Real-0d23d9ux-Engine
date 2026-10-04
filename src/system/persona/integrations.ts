/**
 * E 域跨域集成点层 · F151-F170 与兄弟域的接驳面（集成点在位，等待对端注入）。
 *
 * 主册判据延伸（逐条钉源）：
 * - F153【数据与存储】「日落时间引用 F116 城市数据」→ SunProvider 注入点：
 *   F116 就绪前用天文公式兜底（autodark.sunTimesMinutes），就绪后由 F116 注册
 *   provider——本域不等待，先跑起来（Schema 先行纪律）。
 * - F167【设计细节】「计数引擎复用 F072」→ F072 使用计数灌入口：两源对账，
 *   差异如实报告（一处一事实的机械审计）。
 * - F170【设计细节】「与 F121 还原点联动验证（回退可走还原点路径复测——双路
 *   保险）」→ restorePointPathReverify：第二路复测与第一路逐位对拍。
 * - F161【数据与存储】「包签名（F127）」「导出走 F127 打包」→ vxappPackDescriptor：
 *   打包直通的声明性入口（F127 就绪后直连）。
 * - F163【状态与异常】「时钟与系统时间强一致（F187）」→ clockDriftOk 判定。
 *
 * 集成纪律：本文件只做「接驳面 + 兜底」，不实现对端域的功能——边界诚实。
 */

// ---------- F116 日落日出数据注入点（F153 sunset 模式） ----------

export interface SunTimes {
  /** 日落（当日分钟数 0..1439）。 */
  sunset: number;
  /** 日出（当日分钟数 0..1439）。 */
  sunrise: number;
  /** 数据来源标注（诊断与体验日志用）。 */
  source: string;
}

/** F116 城市数据提供者（F116 就绪后注册；返回 null = 今日数据不可用）。 */
export type SunProvider = (dateISO: string) => SunTimes | null;

let sunProvider: SunProvider | null = null;

/** 注册 F116 数据源（幂等——后注册者生效，返回是否覆盖了前者）。 */
export function setSunProvider(p: SunProvider | null): boolean {
  const had = sunProvider !== null;
  sunProvider = p;
  return had;
}

export function hasSunProvider(): boolean {
  return sunProvider !== null;
}

export interface SunInjectionResult {
  /** 是否注入成功（provider 缺席/返回 null = 未注入——调用方走天文兜底）。 */
  injected: boolean;
  times: SunTimes | null;
  detail: string;
}

/** F116 数据注入：写进 AutoDarkConfig 的 sunsetMinutes/sunriseMinutes 的上游。 */
export function injectSunTimes(dateISO: string): SunInjectionResult {
  if (!sunProvider) {
    return { injected: false, times: null, detail: "F116 未接入——使用天文公式兜底（autodark.sunTimesMinutes）" };
  }
  const t = sunProvider(dateISO);
  if (!t) {
    return { injected: false, times: null, detail: "F116 已接入但今日数据不可用（极昼极夜或数据缺失）——走兜底" };
  }
  return { injected: true, times: t, detail: `F116 注入成功（${t.source}）` };
}

/** F116 判据「日落触发时刻准确（±5 分钟）」的机械口径。 */
export const SUN_ACCURACY_TOLERANCE_MIN = 5;

export function sunAccuracyVerdict(injected: SunTimes, actual: SunTimes): { ok: boolean; sunsetDeltaMin: number; sunriseDeltaMin: number } {
  const sunsetDeltaMin = Math.abs(injected.sunset - actual.sunset);
  const sunriseDeltaMin = Math.abs(injected.sunrise - actual.sunrise);
  return {
    ok: sunsetDeltaMin <= SUN_ACCURACY_TOLERANCE_MIN && sunriseDeltaMin <= SUN_ACCURACY_TOLERANCE_MIN,
    sunsetDeltaMin, sunriseDeltaMin,
  };
}

// ---------- F072 最近使用引擎计数直连（F167 自动排序同源） ----------

export interface UsageFeedEntry {
  itemId: string;
  /** F072 引擎统计的次数（窗口期内）。 */
  count: number;
  /** 计数所属日（YYYY-MM-DD；缺省=今天）。 */
  day?: string;
}

export interface UsageFeedResult {
  accepted: number;
  rejected: number;
  /** 拒绝原因逐条（负数/超窗——外部输入全清洗）。 */
  rejectedReasons: string[];
}

/** 单日计数上限护栏（防灌入异常数据撑爆环形——F167 90 天环形同窗）。 */
export const FEED_MAX_PER_DAY = 10_000;

/** F072 引擎数据批量灌入（对齐 usageCount 的环形结构——不重复造存储）。 */
export function feedUsageFromRecentEngine(
  ring: Record<string, { day: string; count: number }[]>,
  entries: UsageFeedEntry[],
  todayISO: string,
): UsageFeedResult {
  const result: UsageFeedResult = { accepted: 0, rejected: 0, rejectedReasons: [] };
  for (const e of entries) {
    if (!e.itemId || typeof e.count !== "number" || !Number.isFinite(e.count) || e.count < 0) {
      result.rejected++;
      result.rejectedReasons.push(`${e.itemId || "(空 id)"}: 计数非法`);
      continue;
    }
    if (e.count > FEED_MAX_PER_DAY) {
      result.rejected++;
      result.rejectedReasons.push(`${e.itemId}: 单日 ${e.count} 超护栏 ${FEED_MAX_PER_DAY}`);
      continue;
    }
    const day = e.day ?? todayISO;
    const list = ring[e.itemId] ?? [];
    const slot = list.find((s) => s.day === day);
    if (slot) slot.count = e.count; // F072 是权威源——覆盖本地同日计数（一处一事实）
    else list.push({ day, count: e.count });
    ring[e.itemId] = list;
    result.accepted++;
  }
  return result;
}

/** 两源对账：F072 权威值 vs 本地环形值——差异清单如实返回（不静默合并）。 */
export function reconcileCounts(
  ring: Record<string, { day: string; count: number }[]>,
  authoritative: Record<string, number>,
): { consistent: boolean; diffs: { itemId: string; local: number; remote: number }[] } {
  const localTotals: Record<string, number> = {};
  for (const [id, list] of Object.entries(ring)) {
    localTotals[id] = list.reduce((acc, s) => acc + s.count, 0);
  }
  const diffs: { itemId: string; local: number; remote: number }[] = [];
  const ids = new Set([...Object.keys(localTotals), ...Object.keys(authoritative)]);
  for (const id of ids) {
    const l = localTotals[id] ?? 0;
    const r = authoritative[id] ?? 0;
    if (l !== r) diffs.push({ itemId: id, local: l, remote: r });
  }
  return { consistent: diffs.length === 0, diffs };
}

// ---------- F121 还原点双路保险（F170 回退复测） ----------

export interface DualPathResult {
  /** 第一路：直接回退（E 域原生 undo）。 */
  directPathOk: boolean;
  /** 第二路：还原点路径复测（F121 快照恢复）。 */
  restorePointPathOk: boolean;
  /** 双路结论一致才算过——单路绿单路红 = 回退实现有分叉，回炉。 */
  bothPathsAgree: boolean;
  detail: string;
}

/**
 * 双路保险：E 域回退后，把回退结果与 F121 还原点记录的快照逐位对拍——
 * 两路都达标且结论一致，「随时可退」才算双保险（主册 F170 设计细节）。
 */
export function restorePointPathReverify(
  beforeSnapshot: unknown,
  afterDirectRollback: unknown,
  restorePointSnapshot: unknown,
): DualPathResult {
  const directPathOk = JSON.stringify(afterDirectRollback) === JSON.stringify(beforeSnapshot);
  const restorePointPathOk = JSON.stringify(restorePointSnapshot) === JSON.stringify(beforeSnapshot);
  const agree = directPathOk === restorePointPathOk;
  return {
    directPathOk,
    restorePointPathOk,
    bothPathsAgree: directPathOk && restorePointPathOk && agree,
    detail: directPathOk && restorePointPathOk
      ? "双路一致：直接回退与还原点路径均与变更前快照逐位等值"
      : `分叉检出：直接回退${directPathOk ? "✓" : "✗"} / 还原点路径${restorePointPathOk ? "✓" : "✗"}——回退实现需回炉`,
  };
}

// ---------- F127 vxapp 打包直通（F161 导出管线） ----------

export interface VxappPackDescriptor {
  /** F127 打包器消费的声明（F127 就绪后直接传入）。 */
  packId: string;
  displayName: string;
  /** 包内容物类型（E 域只产出配置与资产引用，不含可执行——F126 开放格式宪法）。 */
  contents: "vxtheme-profile";
  /** 预期产物（.vxapp 包内布局）。 */
  layout: readonly { path: string; kind: "config" | "asset-ref" }[];
}

/** 从档案包生成 F127 打包声明（直通接口——F127 缺席时导出仍走独立 JSON 通道）。 */
export function vxappPackDescriptor(pkgName: string): VxappPackDescriptor {
  return {
    packId: `persona.${pkgName || "profile"}`,
    displayName: pkgName || "个性化档案",
    contents: "vxtheme-profile",
    layout: [
      { path: "profile.vxtheme.json", kind: "config" },
      { path: "assets/", kind: "asset-ref" },
    ],
  };
}

// ---------- F187 时钟强一致（F163 时钟组件秒级准确的上游判定） ----------

/** 容差：秒级准确 = 与系统时钟偏差 ≤1s（F187 守护后的残差预算）。 */
export const CLOCK_DRIFT_TOLERANCE_MS = 1000;

export function clockDriftOk(wallClockMs: number, systemClockMs: number, toleranceMs = CLOCK_DRIFT_TOLERANCE_MS): boolean {
  return Math.abs(wallClockMs - systemClockMs) <= toleranceMs;
}
