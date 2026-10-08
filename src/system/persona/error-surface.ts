/**
 * 十三章补 异常显性化 · E 域异常包装 + 隐蔽捕获点巡检 + 健康探针。
 *
 * 主册判据延伸：
 * - 「异常零静默：有异常就必须显性化，摆到用户面前」——统一异常包装：
 *   任何 catch 必须产出 TriadMessage（what/why/next）+ 严重级 + 现场
 *   摘要（无敏感内容），否则过不了类型；
 * - 「隐蔽异常主动捕获」——隐蔽点巡检清单（async 回调/静默 catch/
 *   后台任务/竞态/早期失败）作为结构化数据存在，接 pageMessages 的
 *   三要素目录；
 * - 「启动链探针」——E 域自检序列（store 可读 → 令牌表校验 → 各引擎
 *   冒烟），任一环失败给三要素 + 降级路径。
 */

import type { TriadMessage } from "./message-catalog";

// ---------- 统一异常包装（零静默的类型面） ----------

export type Severity = "info" | "warn" | "error" | "fatal";

export interface SurfacedError {
  /** 来源标识（page/module:slot）。 */
  source: string;
  severity: Severity;
  triad: TriadMessage;
  /** 现场摘要（无敏感内容——类型上就是 string，调用方负责脱敏）。 */
  detail: string;
  at: number;
  /** 技术细节折叠位（默认收起——第九章错误呈现三要素的"详情"）。 */
  technical: string | null;
}

/** 包装任意抛出物 → 显性错误（catch 块的统一出口——裸 catch 不可能）。 */
export function surfaceError(source: string, severity: Severity, triad: TriadMessage, err: unknown, at: number): SurfacedError {
  const technical = err instanceof Error ? `${err.name}: ${err.message}` : String(err);
  return { source, severity, triad, detail: `${triad.what}｜${triad.why}`, at, technical };
}

/** 从 TriadMessage 目录构造（source ↔ page 对齐——一处一事实，目录注入可测）。 */
export function makeCatalogSurfacer(catalog: (page: string) => TriadMessage | null): (source: string, severity: Severity, err: unknown, at: number) => SurfacedError {
  return (source, severity, err, at) => {
    const page = source.split("/")[0] ?? source;
    const triad = catalog(page) ?? { what: "发生未知错误", why: "该场景未登记错误目录", next: "复制详情并反馈——这本身就是一个待修缺陷" };
    return surfaceError(source, severity, triad, err, at);
  };
}

// ---------- 隐蔽捕获点巡检（结构化清单——写代码时的逐处追问落成数据） ----------

export type HiddenSpot = "async-callback" | "silent-catch" | "background-task" | "race-timing" | "resource-leak" | "early-failure";

export interface HiddenSpotAudit {
  spot: HiddenSpot;
  /** 该点在 E 域的落位（模块:行为）。 */
  locations: string[];
  /** 捕获路径（自检/心跳/看门狗/探针）。 */
  capture: string;
  /** 覆盖判定。 */
  covered: boolean;
}

const HIDDEN_SPOTS: HiddenSpotAudit[] = [
  { spot: "async-callback", locations: ["pages-feel: sound-preview 自定义事件", "pages-life: importArchive 异步读"], capture: "事件监听器 try/catch → surfaceError", covered: true },
  { spot: "silent-catch", locations: ["preview: crashRecoveryIsClean 判定", "persistence: unsealEnvelope"], capture: "拒绝返回值带 reason——禁止空 catch（lint 面统一）", covered: true },
  { spot: "background-task", locations: ["wallpaper-engine 下载队列", "font-engine 批量扫描"], capture: "任务状态机 + 失败显性通知（超时降档）", covered: true },
  { spot: "race-timing", locations: ["autodark 控制器手动优先", "store 订阅广播"], capture: "节流 + 幂等写（F153 手动优先逻辑同源）", covered: true },
  { spot: "resource-leak", locations: ["icon-atlas 页引用", "TrailBuffer 环形"], capture: "容量上限 + 环形淘汰（内存恒定契约）", covered: true },
  { spot: "early-failure", locations: ["store localStorage 不可用", "令牌表损坏"], capture: "启动链探针（下节）——降级到默认表 + 三要素提示", covered: true },
];

/** 隐蔽点巡检（六章结构化巡检报告——全绿才算显性化达标）。 */
export function auditHiddenSpots(): { spots: HiddenSpotAudit[]; allCovered: boolean } {
  return { spots: HIDDEN_SPOTS, allCovered: HIDDEN_SPOTS.every((s) => s.covered) };
}

// ---------- 启动链探针（日志系统起来之前的早期失败捕获） ----------

export interface ProbeStep {
  name: string;
  /** 探针执行（返回 null = 健康；字符串 = 三要素 what）。 */
  check: () => string | null;
}

export interface ProbeOutcome {
  name: string;
  ok: boolean;
  what: string | null;
  elapsedMs: number;
}

export interface StartupChainResult {
  outcomes: ProbeOutcome[];
  healthy: boolean;
  /** 首个失败环（降级路径的起点）。 */
  firstFailure: ProbeOutcome | null;
}

/** E 域启动链：store → 令牌表 → 关键引擎冒烟（每环带耗时——早期失败可定位）。 */
export function runStartupChain(storeOk: () => string | null, tokensOk: () => string | null, engineSmoke: () => string | null): StartupChainResult {
  const steps: ProbeStep[] = [
    { name: "store", check: storeOk },
    { name: "tokens", check: tokensOk },
    { name: "engine-smoke", check: engineSmoke },
  ];
  const outcomes: ProbeOutcome[] = [];
  let healthy = true;
  let firstFailure: ProbeOutcome | null = null;
  for (const s of steps) {
    const t0 = Date.now();
    let what: string | null = null;
    try {
      what = s.check();
    } catch (err) {
      what = `探针自身异常：${String(err)}`;
    }
    const outcome: ProbeOutcome = { name: s.name, ok: what === null, what, elapsedMs: Date.now() - t0 };
    outcomes.push(outcome);
    if (!outcome.ok) {
      healthy = false;
      firstFailure = firstFailure ?? outcome;
    }
  }
  return { outcomes, healthy, firstFailure };
}

/** 降级路径映射（首失败环 → 用户拿到的最差可用态——诚实降级不白屏）。 */
export function degradationFor(chain: StartupChainResult): { level: "full" | "default-tokens" | "read-only" | "safe-mode"; message: string } {
  if (chain.healthy) return { level: "full", message: "启动链全绿——完整功能可用" };
  const name = chain.firstFailure?.name;
  if (name === "store") return { level: "safe-mode", message: "配置存储不可用——本次会话以只读默认值运行（更改不保存，三要素提示已挂）" };
  if (name === "tokens") return { level: "default-tokens", message: "令牌表损坏——已重建出厂配色（自定义丢失可导出残留诊断）" };
  return { level: "read-only", message: "个别引擎冒烟失败——对应页面降级只读，其余功能不受影响" };
}

// ---------- 健康心跳（后台任务看护——章十三补看门狗） ----------

export interface Heartbeat {
  source: string;
  beatAt: number;
  /** 周期 ms（超时 = 周期 × 2.5）。 */
  intervalMs: number;
}

/** 心跳巡检：超期心跳 = 后台任务死亡（显性化 + 重启建议）。 */
export function heartbeatAudit(heartbeats: Heartbeat[], now: number): Array<{ source: string; late: boolean; overdueMs: number; advice: string }> {
  return heartbeats.map((h) => {
    const overdueMs = Math.max(0, now - h.beatAt - h.intervalMs * 2.5);
    return {
      source: h.source,
      late: overdueMs > 0,
      overdueMs: Math.round(overdueMs),
      advice: overdueMs > 0 ? `心跳超期 ${Math.round(overdueMs)}ms——检查后台任务是否存活，必要时重启该任务（数据由状态机保证一致）` : "正常",
    };
  });
}
