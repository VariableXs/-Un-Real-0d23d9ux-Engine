/**
 * F372 系统活动人话时间线（H 域 · AI-H4）：
 * 出了问题问「刚才发生了什么」：设置中心「关于」页的人话时间线——最近 24h 系统大事记
 * （「14:02 打开 记事本」「14:05 异常关机」「14:07 自检修复 3 处」），来源是审计日志（F194）
 * 的用户面转译（事件→人话映射表）；每条可展开技术详情；时间线只读不可改（哈希链完整性）。
 * 判据（主册 F372）：映射表覆盖率（系统事件大类全转译）；展开详情；24h 窗口与存储上限；
 * 只读+哈希链判据；时间准确性对账 F295。
 * 依赖锚点：F194 审计日志 / F295 时间源。
 */

/** 系统事件大类（判据「映射表覆盖率」——全类目必须有人话映射）。 */
export const EVENT_KINDS = ["appOpen", "appClose", "crash", "abnormalShutdown", "selfHeal", "update", "restore", "devicePlug"] as const;
export type EventKind = (typeof EVENT_KINDS)[number];

/** 审计日志条目（F194 的源面；本层只读消费）。 */
export interface AuditEvent {
  seq: number;
  kind: EventKind;
  at: number;
  /** 技术详情（展开面；只读透传）。 */
  techDetail: string;
  /** 动态参数（人话模板填充）。 */
  params?: Record<string, string>;
}

/** 人话映射表（判据核心——一处一事实，覆盖 EVENT_KINDS 全类目）。 */
export const HUMAN_TEMPLATES: Record<EventKind, string> = {
  appOpen: "打开 {app}",
  appClose: "关闭 {app}",
  crash: "{app} 意外退出（已隔离，其他窗口不受影响）",
  abnormalShutdown: "异常关机",
  selfHeal: "自检修复 {count} 处",
  update: "系统更新：{version}",
  restore: "恢复到还原点「{point}」",
  devicePlug: "接入设备 {device}",
};

/** 模板覆盖率审计（判据「系统事件大类全转译」）：缺一即不合格。 */
export function auditTemplateCoverage(): { pass: boolean; missing: EventKind[] } {
  const missing = EVENT_KINDS.filter((k) => !(k in HUMAN_TEMPLATES) || !HUMAN_TEMPLATES[k].trim());
  return { pass: missing.length === 0, missing };
}

/** 人话时间（F295 对齐口径：HH:mm）。 */
export function humanTime(at: number, base: Date): string {
  const d = new Date(at);
  void base;
  return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}

export interface TimelineRow {
  seq: number;
  time: string;
  /** 人话正文。 */
  text: string;
  /** 可展开的技术详情（判据「展开详情」）。 */
  detail: string;
}

/** 缺参数模板如实保留占位（零静默：不把缺参翻译成错的句子）。 */
export function fillTemplate(template: string, params?: Record<string, string>): string {
  if (!params) return template;
  return template.replace(/\{(\w+)\}/g, (_, key: string) => (key in params ? params[key]! : `{${key}}`));
}

/* ---------- 时间线窗口与哈希链 ---------- */

/** 24h 窗口（判据）。 */
export const WINDOW_MS = 24 * 60 * 60 * 1000;
/** 存储上限（判据「存储上限」）。 */
export const TIMELINE_CAP = 500;

/** FNV-1a 哈希（哈希链节点的轻量实现——只读完整性校验用）。 */
export function fnv1a(text: string): string {
  let h = 0x811c9dc5;
  for (let i = 0; i < text.length; i++) {
    h ^= text.charCodeAt(i);
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return h.toString(16).padStart(8, "0");
}

export interface ChainedRow extends TimelineRow {
  /** 链哈希 = H(上一节点哈希 + 本条内容)——改一条即断链（判据「只读+哈希链」）。 */
  chain: string;
}

/** 审计事件 → 时间线行（带哈希链；时间升序）。 */
export function buildTimeline(events: AuditEvent[], now: number): ChainedRow[] {
  const inWindow = events
    .filter((e) => e.at >= now - WINDOW_MS && e.at <= now)
    .sort((a, b) => a.at - b.at || a.seq - b.seq)
    .slice(-TIMELINE_CAP);
  let prev = "genesis";
  return inWindow.map((e) => {
    const row: TimelineRow = {
      seq: e.seq,
      time: humanTime(e.at, new Date(now)),
      text: fillTemplate(HUMAN_TEMPLATES[e.kind] ?? e.kind, e.params),
      detail: e.techDetail,
    };
    const chain = fnv1a(`${prev}|${row.seq}|${row.time}|${row.text}|${row.detail}`);
    prev = chain;
    return { ...row, chain };
  });
}

/** 哈希链完整性校验：重算全链，任一不匹配 → 被篡改（判据）。 */
export function verifyChain(rows: ChainedRow[]): { intact: boolean; brokenAt: number | null } {
  let prev = "genesis";
  for (const r of rows) {
    const expect = fnv1a(`${prev}|${r.seq}|${r.time}|${r.text}|${r.detail}`);
    if (expect !== r.chain) return { intact: false, brokenAt: r.seq };
    prev = r.chain;
  }
  return { intact: true, brokenAt: null };
}
