/**
 * F400 H 域收官登记（H 域 · AI-H4）：
 * H 域 200 项（F201-F400）收官三件事：①全项标题与判据锚点汇入全域总检（F200/F375 双层
 * 扩展——总检脚本新增 200 个检查点）；②F 清单增补区新增 F-H 节（200 项一行账，由报告
 * 正文自动提取生成，一处一事实）；③季度审视范围扩为 F001-F400+I 域（F200 条款同步修订）。
 * 判据（主册 F400）：一行账与正文标题 100% 一致（脚本生成保证）；总检检查点新增 200 个
 * 全绿基线；三处同源审计；F200 条款修订完成。
 * 边界纪律：本模块由 AI-H4 落地——F351-F400 的 50 项登记册内置（本队产出，一处一事实）；
 * F201-F350 的 150 项标题由 H1-H3 各自的登记册注入（不越队代写）。
 * 存储键：variable:h4:f400（一行账快照）。
 */

import { defaultStore, h4Key, readJson, type KvStore, writeJson } from "./internal/store";

/** H4 分队 50 项一行账（与《Varix STAR I start.md》第 6 部 F351-F400 正文标题逐字一致——
 * 判据「一行账与正文标题 100% 一致」的生成源）。 */
export const H4_TITLES: ReadonlyArray<{ item: string; title: string }> = [
  { item: "F351", title: "工作区快照" }, { item: "F352", title: "缩略图窗上直接操作" }, { item: "F353", title: "跨屏拖拽位置记忆" }, { item: "F354", title: "任务栏资源摘要" }, { item: "F355", title: "Edge 深度协同" },
  { item: "F356", title: "PWA 应用化" }, { item: "F357", title: "下载收口体验" }, { item: "F358", title: "全局画中画" }, { item: "F359", title: "全局屏幕拾色器" }, { item: "F360", title: "像素标尺与网格叠加" },
  { item: "F361", title: "屏幕录制" }, { item: "F362", title: "录屏产物管理" }, { item: "F363", title: "专注计时器" }, { item: "F364", title: "下载文件夹一键整理" }, { item: "F365", title: "重复文件查找" },
  { item: "F366", title: "托盘电池显示选项" }, { item: "F367", title: "桌面分区吸附" }, { item: "F368", title: "最小化到托盘" }, { item: "F369", title: "后台任务中心" }, { item: "F370", title: "后台不惊扰承诺" },
  { item: "F371", title: "开机时长徽标" }, { item: "F372", title: "系统活动人话时间线" }, { item: "F373", title: "键盘布局管理" }, { item: "F374", title: "快捷键速查浮层" }, { item: "F375", title: "H 域总判据" },
  { item: "F376", title: "标题栏系统菜单" }, { item: "F377", title: "键盘移动与调整窗口" }, { item: "F378", title: "列宽双击自适应" }, { item: "F379", title: "表头排序指示" }, { item: "F380", title: "空行点击清除选择" },
  { item: "F381", title: "树形控件半选与记忆" }, { item: "F382", title: "对话框位置记忆" }, { item: "F383", title: "弹窗排队不叠罗汉" }, { item: "F384", title: "焦点陷阱（模态完整性）" }, { item: "F385", title: "无障碍语义树" },
  { item: "F386", title: "阅读模式" }, { item: "F387", title: "灰度模式" }, { item: "F388", title: "竖屏与异形屏适配" }, { item: "F389", title: "滚动长截图" }, { item: "F390", title: "选中文本查词" },
  { item: "F391", title: "选中文本翻译" }, { item: "F392", title: "文件夹大小列" }, { item: "F393", title: "存储热点图" }, { item: "F394", title: "清理建议收口页" }, { item: "F395", title: "U 盘健康监控" },
  { item: "F396", title: "备份向导" }, { item: "F397", title: "还原演练" }, { item: "F398", title: "界面语言热切" }, { item: "F399", title: "彩蛋总谱" }, { item: "F400", title: "H 域收官登记" },
];

export interface LedgerLine {
  item: string;
  title: string;
}

/** 一行账生成（判据「由报告正文自动提取生成」）：输入标题表 → 逐行账（不改字——
 * 生成器不做任何改写，100% 一致性由「同一数据源喂正文与账本」保证）。 */
export function generateOneLineLedger(titles: ReadonlyArray<{ item: string; title: string }>): LedgerLine[] {
  return titles.map((t) => ({ item: t.item, title: t.title }));
}

/** 一行账与标题表一致性校验（判据 100% 一致）：逐项逐字比对。 */
export function auditLedgerMatchesTitles(ledger: LedgerLine[], titles: ReadonlyArray<{ item: string; title: string }>): { pass: boolean; mismatch: string[] } {
  const mismatch: string[] = [];
  if (ledger.length !== titles.length) mismatch.push(`行数 ${ledger.length} ≠ ${titles.length}`);
  for (let i = 0; i < Math.min(ledger.length, titles.length); i++) {
    if (ledger[i]!.item !== titles[i]!.item || ledger[i]!.title !== titles[i]!.title) {
      mismatch.push(`${ledger[i]!.item}: "${ledger[i]!.title}" ≠ "${titles[i]!.title}"`);
    }
  }
  return { pass: mismatch.length === 0, mismatch };
}

/* ---------- 总检检查点（F200/F375 双层扩展——判据①） ---------- */

export interface MasterCheckpoint {
  item: string;
  name: string;
  passed: boolean;
}

/** H 域总检基座：200 项 × 1 检查点（F201-F350 由各队登记册注入，F351-F400 内置）。 */
export function buildHDomainCheckpoints(
  h1ToH3Titles: ReadonlyArray<{ item: string; title: string }>,
  results: ReadonlyArray<{ item: string; passed: boolean }>,
): { total: number; passed: number; allGreen: boolean; missing: string[] } {
  const all = [...h1ToH3Titles, ...H4_TITLES].map((t) => t.item);
  const resultMap = new Map(results.map((r) => [r.item, r.passed]));
  const missing = all.filter((item) => !resultMap.has(item));
  const passed = all.filter((item) => resultMap.get(item) === true).length;
  return { total: all.length, passed, allGreen: missing.length === 0 && passed === all.length, missing };
}

/* ---------- 三处同源审计（判据「三处同源」：正文/一行账/总检脚本） ---------- */

export interface ThreeSourceAudit {
  pass: boolean;
  sources: { docTitles: number; ledgerLines: number; checkpoints: number };
  detail: string;
}

/** 三处同源：正文标题数 = 一行账行数 = 总检检查点数（200）。 */
export function auditThreeSources(docTitleCount: number, ledger: LedgerLine[], checkpointTotal: number): ThreeSourceAudit {
  const pass = docTitleCount === ledger.length && ledger.length === checkpointTotal && checkpointTotal === 200;
  return {
    pass,
    sources: { docTitles: docTitleCount, ledgerLines: ledger.length, checkpoints: checkpointTotal },
    detail: pass ? "正文/一行账/总检三处同源（各 200 项）" : "三处数目不一致——收官登记不通过（一处一事实被破坏）",
  };
}

/* ---------- F200 条款修订登记（判据③：季度审视范围扩为 F001-F400+I 域） ---------- */

export interface QuarterlyScopeRevision {
  /** 修订后季度审视范围。 */
  scope: string;
  revisedAt: string;
  /** F200 条款版本号（同步修订的留痕）。 */
  clauseRevision: string;
}

export function quarterlyScopeRevision(revisedAt: string): QuarterlyScopeRevision {
  return { scope: "F001-F400 + I 域（F401-F600）", revisedAt, clauseRevision: "F200-r2" };
}

/* ---------- 一行账快照持久化（收官账入库） ---------- */

const KEY = h4Key("f400", "ledger");

export function persistLedger(ledger: LedgerLine[], store: KvStore = defaultStore()): boolean {
  return writeJson(store, KEY, ledger);
}

export function loadLedger(store: KvStore = defaultStore()): LedgerLine[] {
  return readJson<LedgerLine[]>(store, KEY, [], Array.isArray);
}
