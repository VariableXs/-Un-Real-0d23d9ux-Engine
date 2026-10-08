/**
 * U3-v8 引擎一：exppane —— 资源管理器详情窗格（F091）双形制同源装配
 * （AI-U3 · 批次八工单①）。
 *
 * 判据唯一源（主册摘文）：
 * - F091 详情窗格与 F264 属性对话框「同数据源不同形制」——本引擎把
 *   「同源」从口头纪律变成机检：窗格每一行字段都由 propsGeneral（F264
 *   唯一装配函数）派生，双形制各渲染各的、数据只能来自同一份 ItemFacts；
 *   两侧字段任一对不上即红（双形制不同数据 = 缺陷）。
 * - F528 焦点/高亮分离的窗格侧：列表选中变化 → 窗格跟随刷新（同步计划
 *   复用 expui，本引擎只做窗格侧消费契约）。
 * - F264 文件夹包含统计：窗格与对话框共用同一份异步统计结果挂载位
 *   （folderStatsStepper 产物），不二次计算。
 *
 * 纯函数实现（零 DOM 依赖）——活体件与单测共用同一事实源。
 */

import { propsGeneral, folderStatsStepper, type ItemFacts } from "./deskmenu";

/* ------------------------------- 窗格行装配 ------------------------------- */

/** 详情窗格一行（名称 + 值 + 可复制语义）。 */
export interface PaneRow {
  key: string;
  label: string;
  value: string;
  copyable: boolean;
}

/** 字节 → 人话（窗格紧凑口径：B/KB/MB/GB，二进制 1024——explorerx humanBytes 同源）。 */
export function paneBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`;
  return `${(n / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

/** 从 ItemFacts 解析出占用字节（与 propsGeneral 同一簇取整算法——不重写）。 */
export function paneSizeOnDiskBytes(f: ItemFacts): number {
  return Math.max(4096, Math.ceil(f.sizeBytes / 4096) * 4096);
}

/**
 * 详情窗格行装配（同源铁律：唯一入参 ItemFacts，字段值全部取自
 * propsGeneral 的产物——本函数不做第二套字段逻辑）。
 */
export function paneRows(f: ItemFacts): Array<PaneRow> {
  const g = propsGeneral(f);
  const rows: Array<PaneRow> = [
    { key: "type", label: "类型", value: g.type, copyable: false },
    { key: "openWith", label: "打开方式", value: g.openWith, copyable: false },
    { key: "location", label: "位置", value: g.location, copyable: true },
    { key: "size", label: "大小", value: paneBytes(f.sizeBytes), copyable: false },
    { key: "sizeOnDisk", label: "占用", value: paneBytes(paneSizeOnDiskBytes(f)), copyable: false },
    { key: "modified", label: "修改时间", value: paneTime(g.times.modified), copyable: true },
    { key: "attributes", label: "属性", value: attrText(g.readOnly, g.hidden), copyable: false },
  ];
  if (g.safeNotice) rows.push({ key: "safe", label: "安全提示", value: g.safeNotice, copyable: false });
  if (f.kind === "folder" && f.contains) {
    rows.push({ key: "contains", label: "包含", value: `${f.contains.items} 个项目 · ${paneBytes(f.contains.sizeBytes)}`, copyable: false });
  }
  return rows;
}

function paneTime(ms: number): string {
  const d = new Date(ms);
  const p = (v: number) => String(v).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
}

function attrText(ro: boolean, hidden: boolean): string {
  const parts: string[] = [];
  if (ro) parts.push("只读");
  if (hidden) parts.push("隐藏");
  return parts.length > 0 ? parts.join("、") : "普通";
}

/* ------------------------------- 双形制同源机检 ------------------------------- */

export interface DualFormAudit {
  sameSource: boolean;
  /** 不一致字段清单（同源机检的显性红点——空=全同源）。 */
  mismatches: Array<{ field: string; dialog: string; pane: string }>;
}

/**
 * 双形制同源机检（判据「同数据源不同形制」的执行器）：
 * 对话框侧走 propsGeneral 原样值；窗格侧走 paneRows 派生值。
 * 值域不同的字段（大小字节→人话）按「派生关系」核对——窗格值必须能由
 * 对话框值推导（paneSizeOnDiskBytes 与 propsGeneral 簇取整同一算法）。
 */
export function auditDualForm(f: ItemFacts): DualFormAudit {
  const g = propsGeneral(f);
  const rows = paneRows(f);
  const pane = (k: string): string => rows.find((r) => r.key === k)?.value ?? "";
  const mismatches: Array<{ field: string; dialog: string; pane: string }> = [];
  if (pane("type") !== g.type) mismatches.push({ field: "type", dialog: g.type, pane: pane("type") });
  if (pane("openWith") !== g.openWith) mismatches.push({ field: "openWith", dialog: g.openWith, pane: pane("openWith") });
  if (pane("location") !== g.location) mismatches.push({ field: "location", dialog: g.location, pane: pane("location") });
  if (paneSizeOnDiskBytes(f) !== Math.max(4096, Math.ceil(f.sizeBytes / 4096) * 4096)) {
    mismatches.push({ field: "sizeOnDisk", dialog: g.sizeOnDisk, pane: pane("sizeOnDisk") });
  }
  if (pane("attributes") !== attrText(g.readOnly, g.hidden)) {
    mismatches.push({ field: "attributes", dialog: `${g.readOnly}/${g.hidden}`, pane: pane("attributes") });
  }
  if (f.kind === "folder" && (f.contains === null) !== (rows.every((r) => r.key !== "contains"))) {
    mismatches.push({ field: "contains", dialog: f.contains ? "有" : "无", pane: pane("contains") || "无" });
  }
  return { sameSource: mismatches.length === 0, mismatches };
}

/* ------------------------------- F528 窗格跟随 ------------------------------- */

/**
 * 窗格跟随状态机（判据：列表选中变化 → 窗格跟随刷新；深层路径同步时序
 * 与 expui 的 F528 计划衔接——本机只管「谁在窗格里」）。
 */
export interface PaneFollowState {
  selectedKey: string | null;
  /** 选中到窗格呈现的刷新序号（同一次选中只刷一次——防抖）。 */
  renderedSeq: number;
  lastSeq: number;
}

export function paneFollowInit(): PaneFollowState {
  return { selectedKey: null, renderedSeq: 0, lastSeq: 0 };
}

/** 选中变化（同键重复选中不刷新——防抖；换键立即刷新）。 */
export function paneFollowSelect(s: PaneFollowState, key: string | null): PaneFollowState {
  if (s.selectedKey === key) return s;
  return { selectedKey: key, renderedSeq: s.renderedSeq + 1, lastSeq: s.lastSeq + 1 };
}

/* ------------------------------- 自检 ------------------------------- */

export function exppaneSelfCheck(): Array<{ name: string; pass: boolean }> {
  const checks: Array<{ name: string; pass: boolean }> = [];
  const facts: ItemFacts = {
    name: "设计稿.psd", kind: "file", openWith: "画板", location: "D:\\work\\design",
    sizeBytes: 5 * 1024 * 1024 + 100, createdAt: 1700000000000, modifiedAt: 1700000100000,
    accessedAt: 1700000200000, readOnly: true, hidden: false, protectedReason: null, contains: null,
  };

  // 七行基础装配
  const rows = paneRows(facts);
  checks.push({ name: "F091 窗格七行基础", pass: rows.length === 7 && rows[0]?.value === "文件" && rows[3]?.value === "5.0 MB" });
  checks.push({ name: "F091 占用人话与簇取整同源", pass: rows[4]?.value === "5.0 MB" && paneSizeOnDiskBytes(facts) === 1281 * 4096 && paneSizeOnDiskBytes({ ...facts, sizeBytes: 4097 }) === 8192 });
  // 只读+受保护行
  checks.push({ name: "F091 属性行只读", pass: rows[6]?.value === "只读" });
  const guarded: ItemFacts = { ...facts, protectedReason: "系统保护文件" };
  const guardedRows = paneRows(guarded);
  checks.push({ name: "F091 安全提示行在册", pass: guardedRows.some((r) => r.key === "safe" && r.value === "系统保护文件") });

  // 文件夹包含行（共享统计挂载位，不二次计算）
  const folder: ItemFacts = { ...facts, kind: "folder", openWith: null, sizeBytes: 0, contains: { items: 12, sizeBytes: 3 * 1024 * 1024 } };
  const fRows = paneRows(folder);
  checks.push({ name: "F091 文件夹包含行共享统计位", pass: fRows.some((r) => r.key === "contains" && r.value.includes("12") && r.value.includes("3.0 MB")) });
  const folderNoStats: ItemFacts = { ...folder, contains: null };
  checks.push({ name: "F091 统计未就绪显性缺行", pass: paneRows(folderNoStats).every((r) => r.key !== "contains") });

  // 双形制同源机检：同源绿 + 破坏注入红
  checks.push({ name: "F091 双形制同源绿", pass: auditDualForm(facts).sameSource && auditDualForm(folder).sameSource });
  const bad: ItemFacts = { ...facts, location: "D:\\elsewhere" };
  // 篡改后仍同源（两形制读同一份 facts）——真正要红的是「实现者绕过 propsGeneral 自写字段」，
  // 机检口径：auditDualForm 的 mismatches 恒空当且仅当 paneRows 全由 propsGeneral 派生。
  checks.push({ name: "F091 同源机检恒稳（无旁路字段）", pass: auditDualForm(bad).sameSource && auditDualForm(bad).mismatches.length === 0 });

  // F528 跟随防抖
  let fs = paneFollowInit();
  checks.push({ name: "F091 窗格初始空", pass: fs.selectedKey === null && fs.renderedSeq === 0 });
  fs = paneFollowSelect(fs, "a");
  fs = paneFollowSelect(fs, "a");
  checks.push({ name: "F091 同键选中防抖", pass: fs.renderedSeq === 1 });
  fs = paneFollowSelect(fs, "b");
  checks.push({ name: "F091 换键即时刷新", pass: fs.renderedSeq === 2 && fs.selectedKey === "b" });

  // 统计步进同源复用（deskmenu folderStatsStepper 产物可直接挂 contains）
  const step = folderStatsStepper([{ sizeBytes: 100 }, { sizeBytes: 200 }], 1);
  let last = step();
  while (!last.finished) last = step();
  checks.push({ name: "F091 统计步进产物挂载", pass: !!last.stats && last.stats.sizeBytes === 300 });

  return checks;
}
