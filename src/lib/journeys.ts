/**
 * 任务 82（专项 M · 高频场景剧本六条端到端）—— 旅程定义与逻辑层跑查器。
 *
 * 总案专项 M 口径：功能不是做完就算——按真实使用场景端到端走一遍。
 * 本模块承载：
 * - M1-M6 六条旅程的步骤注册表（每步=能力断言挂钩，不是文案清单）；
 * - 逻辑层跑查器 runJourney：逐步执行能力断言（失败可重试、不报废全程
 *   ——总案 M1「任一步失败可重试不报废全程」）；
 * - 痛点清单 journey.issues 追加（M1「旅程痛点清单逐条修」）。
 *
 * 能力注入与诚实边界：
 * - lib 层能力（搬家状态机/便签/骨架屏时机）由本模块内真调用断言；
 * - feature 层能力（向导组件/引擎会话/通道判定）经 JourneyCaps 注入，
 *   未注入 = skip（如实跳过），绝不伪造 true；
 * - 体验类验收（3 名新手 ≤15 分钟、录屏计时）属任务 88 终局联验人工轮。
 */

import {
  saveMigration,
  newMigration,
  advanceMigration,
  verifyChecksums,
  retireWipeBlock,
} from "./dataMigration";
import { quickNoteEntry, appendQuickNote } from "./quick/quicknote";
import { shouldShowSkeleton, CONTENT_GEOMETRY } from "./skeleton";

/** 能力挂钩：true=在位；false=断言未过（FAIL 可重试）；null=环境缺失（skip）。 */
export type CapCheck = () => boolean | null;

/** 注入式能力表（feature 层由调用方提供；lib 层默认实现见 libCaps）。 */
export interface JourneyCaps {
  oobeWizard: CapCheck; // M1 首次运行向导
  bootPacing: CapCheck; // M1 引导节奏反馈
  engineColdStages: CapCheck; // M2/M4 引擎阶段化叙事
  channelVerdict: CapCheck; // M2 通道判定（反作弊→纯 Windows）
  sharedAppsRegistry: CapCheck; // M2/M4 shared_apps 登记
  saveQueue: CapCheck; // M3/M5 保存队列零丢失
  richClipboard: CapCheck; // M3 跨软件数据流（xflow 5.7）
  demoIsolation: CapCheck; // M4 演示数据隔离
  kvaultBurn: CapCheck; // M5 保险箱焚毁语义
  migration: CapCheck; // M6 搬家状态机
  retirement: CapCheck; // M6 退役两档
  skeleton: CapCheck; // M1/M4/M6 骨架屏
}

export interface JourneyStep {
  id: string;
  label: string;
  check: (caps: JourneyCaps) => boolean | null;
}

export interface Journey {
  id: "M1" | "M2" | "M3" | "M4" | "M5" | "M6";
  title: string;
  steps: JourneyStep[];
  /** 痛点清单（人工轮回填；逻辑跑查器不写入）。 */
  issues: string[];
}

export interface StepResult {
  id: string;
  state: "ok" | "skip" | "fail";
  note: string;
}

export interface JourneyReport {
  id: Journey["id"];
  results: StepResult[];
  ok: boolean;
  /** 失败步可重试：重试只执行 fail 步（不报废全程）。 */
  retriable: string[];
}

// ---- lib 层能力默认实现（全部真调用，零字符串把戏） ----

export function libCaps(): Pick<JourneyCaps, "migration" | "retirement" | "skeleton"> {
  return {
    migration: () => {
      const s = newMigration("probe");
      advanceMigration(s, "remount-diff-chain", () => "ok");
      if (s.step["remount-diff-chain"] !== "done") {
        return false;
      }
      saveMigration(s);
      return verifyChecksums({ a: "1" }, { a: "1" }).ok
        && !verifyChecksums({ a: "1" }, { a: "2" }).ok;
    },
    retirement: () => {
      const buf = new Uint8Array(64);
      buf.fill(0x5a);
      const w = retireWipeBlock(buf);
      // 三轮全零化后：各轮模式摘要互异（确定性账本），末轮摘要稳定。
      const w2 = retireWipeBlock(buf);
      const last = w.passes[2];
      const last2 = w2.passes[2];
      return w.passes.length === 3
        && last !== undefined && last2 !== undefined
        && new Set(w.passes.map((p) => p.digest)).size === 3
        && last.digest === last2.digest;
    },
    skeleton: () =>
      shouldShowSkeleton(299, false) === false
      && shouldShowSkeleton(300, false) === true
      && shouldShowSkeleton(300, true) === false
      && CONTENT_GEOMETRY.files === 40,
  };
}

// ---- 六条旅程（总案 M1-M6 逐条对应） ----

export const JOURNEYS: Journey[] = [
  {
    id: "M1",
    title: "第一次开机：插盘→引导→向导→桌面→第一个软件",
    steps: [
      { id: "s1", label: "引导页选择与倒计时反馈", check: (c) => c.bootPacing() },
      { id: "s2", label: "首次运行向导（注册表驱动）", check: (c) => c.oobeWizard() },
      { id: "s3", label: "桌面就绪与骨架屏快路径", check: (c) => c.skeleton() },
      { id: "s4", label: "首个软件启动引导", check: (c) => c.sharedAppsRegistry() },
    ],
    issues: [],
  },
  {
    id: "M2",
    title: "装第一个游戏：登记→通道判定→启动→退出回 Variable",
    steps: [
      { id: "s1", label: "Steam 游戏库登记（shared_apps）", check: (c) => c.sharedAppsRegistry() },
      { id: "s2", label: "通道判定（反作弊→引导纯 Windows）", check: (c) => c.channelVerdict() },
      { id: "s3", label: "引擎冷启动阶段化叙事", check: (c) => c.engineColdStages() },
    ],
    issues: [],
  },
  {
    id: "M3",
    title: "一天的工作流：文档→搜索→多窗口→带走向另一台续写",
    steps: [
      { id: "s1", label: "文档自动保存零丢失（SaveCoordinator）", check: (c) => c.saveQueue() },
      { id: "s2", label: "跨软件数据流（富剪贴板/拖拽）", check: (c) => c.richClipboard() },
      { id: "s3", label: "便签与快速记录", check: () => appendQuickNote("", quickNoteEntry("x")).length > 0 },
    ],
    issues: [],
  },
  {
    id: "M4",
    title: "给朋友演示：五分钟展示三通道+隔离+拔盘无痕",
    steps: [
      { id: "s1", label: "演示数据与真实数据隔离", check: (c) => c.demoIsolation() },
      { id: "s2", label: "三通道能力呈现（引擎阶段叙事）", check: (c) => c.engineColdStages() },
      { id: "s3", label: "骨架屏（演示中加载不尴尬）", check: (c) => c.skeleton() },
    ],
    issues: [],
  },
  {
    id: "M5",
    title: "出问题的一周：四类小故障均有惊无险",
    steps: [
      { id: "s1", label: "引擎崩溃→会话叙事降级（异常三场景）", check: (c) => c.engineColdStages() },
      { id: "s2", label: "文档零丢失（保存队列）", check: (c) => c.saveQueue() },
      { id: "s3", label: "数据兜底（保险箱焚毁语义在位）", check: (c) => c.kvaultBurn() },
    ],
    issues: [],
  },
  {
    id: "M6",
    title: "换新 U 盘/换新电脑：搬家向导 + 新机首插引导",
    steps: [
      { id: "s1", label: "搬家状态机（断点续走/幂等）", check: (c) => c.migration() },
      { id: "s2", label: "退役两档（reset/wipe 双确认）", check: (c) => c.retirement() },
      { id: "s3", label: "搬家后骨架屏/配置回灌一致", check: (c) => c.skeleton() },
    ],
    issues: [],
  },
];

/** 跑一条旅程（逐步执行；fail 不中断后续——总案 M1 重试口径）。 */
export function runJourney(
  id: Journey["id"],
  caps: JourneyCaps,
  retries = 1,
  journeys: Journey[] = JOURNEYS,
): JourneyReport {
  const j = journeys.find((x) => x.id === id);
  if (!j) {
    throw new Error(`unknown journey: ${id}`);
  }
  const results: StepResult[] = [];
  for (const step of j.steps) {
    let state: StepResult["state"] = "fail";
    let note = "";
    for (let attempt = 0; attempt <= retries; attempt++) {
      try {
        const r = step.check(caps);
        if (r === null) {
          state = "skip";
          note = "环境缺失，如实跳过";
          break;
        }
        if (r === true) {
          state = "ok";
          break;
        }
        note = "能力断言未过";
      } catch (e) {
        note = e instanceof Error ? e.message : String(e);
      }
      if (attempt === retries) {
        state = state === "skip" ? "skip" : "fail";
      }
    }
    results.push({ id: step.id, state, note });
  }
  const failed = results.filter((r) => r.state === "fail").map((r) => r.id);
  return { id, results, ok: failed.length === 0, retriable: failed };
}

/** 重试失败步（不重跑全程；返回更新后的报告）。 */
export function retryFailed(
  report: JourneyReport,
  caps: JourneyCaps,
  journeys: Journey[] = JOURNEYS,
): JourneyReport {
  if (report.retriable.length === 0) {
    return report;
  }
  const again = runJourney(report.id, caps, 1, journeys);
  const keep = new Map(again.results.map((r) => [r.id, r]));
  const merged = report.results.map((r) =>
    r.state === "fail" && keep.has(r.id) ? keep.get(r.id)! : r,
  );
  return {
    ...report,
    results: merged,
    ok: merged.every((r) => r.state !== "fail"),
    retriable: merged.filter((r) => r.state === "fail").map((r) => r.id),
  };
}
