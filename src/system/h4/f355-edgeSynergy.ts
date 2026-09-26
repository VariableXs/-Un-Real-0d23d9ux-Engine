/**
 * F355 Edge 深度协同（H 域 · AI-H4）：
 * 浏览器与系统的三根管子 + 一条诚实边界：
 * ① 下载管：下载完成直达通知（点击定位下载目录，F357 收口体验承接）；
 * ② 拖拽管：网页文本拖拽全语义（F255 四落点同表）；
 * ③ 待遇管：Edge 窗口完全享受系统级待遇（贴靠 F276 / 快捷键族 F309 / 多标签互通 F271 形制）；
 * ④ 诚实边界：UA 与窗口形制不伪装——以 VARIX 身份访问；兼容性问题走 A 域判例工厂（回退路径），
 *    绝不靠伪装混过去（A-3 诚实边界纪律）。
 * 判据（主册 F355）：三管子用例；系统待遇清单逐项验证；下载通知联动；兼容性回退路径。
 */

import { defaultStore, h4Key, readJson, writeJson, type KvStore } from "./internal/store";

/* ---------- ① 下载管 ---------- */

export interface DownloadDoneEvent {
  /** 下载项 id（Edge 会话内）。 */
  id: string;
  fileName: string;
  sizeBytes: number;
  /** 目标完整路径（S: 下载目录语义由调用方保证）。 */
  targetPath: string;
  /** 完整性校验结果（F357 承接）。 */
  hashOk: boolean | null;
}

export interface DownloadNotifyPlan {
  eventId: string;
  /** 通知主文案（人话）。 */
  title: string;
  body: string;
  /** 双动作：打开 / 所在文件夹（判据「下载通知联动」）。 */
  actions: Array<{ id: "open" | "reveal"; label: string }>;
  /** 联动 F357 收口体验：true = 进入其下载收口链路。 */
  handoffToF357: boolean;
}

export function downloadNotifyPlan(ev: DownloadDoneEvent): DownloadNotifyPlan {
  const sizeMb = ev.sizeBytes / (1024 * 1024);
  const sizeText = sizeMb >= 1 ? `${sizeMb.toFixed(1)} MB` : `${Math.max(1, Math.round(ev.sizeBytes / 1024))} KB`;
  return {
    eventId: ev.id,
    title: `下载完成：${ev.fileName}`,
    body: `${sizeText} · ${ev.targetPath}`,
    actions: [
      { id: "open", label: "打开" },
      { id: "reveal", label: "所在文件夹" },
    ],
    handoffToF357: ev.hashOk !== false,
  };
}

/* ---------- ② 拖拽管（F255 四落点） ---------- */

/** F255 四落点语义（本表为 Edge 拖拽管的对齐源）。 */
export type DropTarget = "editor" | "desktop" | "explorer" | "taskbar";

export interface DragPayload {
  kind: "text" | "link" | "file";
  text: string;
}

export interface DropOutcome {
  target: DropTarget;
  accepted: boolean;
  /** 落点语义（人话，供测试与走查对照）。 */
  semantic: string;
}

export function classifyDrop(target: DropTarget, payload: DragPayload): DropOutcome {
  switch (target) {
    case "editor":
      return { target, accepted: payload.kind === "text", semantic: "文本插入光标处；链接转纯文本 URL；文件拒绝" };
    case "desktop":
      return { target, accepted: payload.kind === "link" || payload.kind === "file", semantic: "链接/文件生成快捷方式；纯文本拒绝" };
    case "explorer":
      return { target, accepted: payload.kind === "link" || payload.kind === "file", semantic: "链接下载落盘、文件复制进目录；纯文本拒绝" };
    case "taskbar":
      return { target, accepted: false, semantic: "任务栏不是文本/文件落点（明确拒绝，不吞不糊）" };
  }
}

/* ---------- ③ 待遇管（系统待遇清单） ---------- */

export type TreatmentId = "snap" | "hotkeys" | "tabInterop" | "taskbarPreview" | "altTab";

export interface TreatmentItem {
  id: TreatmentId;
  name: string;
  /** 锚点 F 编号（一处一事实）。 */
  anchor: string;
  entitled: boolean;
  /** 未享受待遇时的人话原因（审计面）。 */
  reason: string;
}

export const SYSTEM_TREATMENT_CHECKLIST: Array<Omit<TreatmentItem, "entitled" | "reason">> = [
  { id: "snap", name: "窗口贴靠（四区+四角）", anchor: "F276" },
  { id: "hotkeys", name: "窗口快捷键族", anchor: "F309" },
  { id: "tabInterop", name: "多标签互通", anchor: "F271" },
  { id: "taskbarPreview", name: "任务栏缩略图预览", anchor: "F073" },
  { id: "altTab", name: "Alt+Tab 独立窗口项", anchor: "F082" },
];

/** 待遇清单逐项验证：任何一项失格即整体不合格（浏览器=一等公民判据）。 */
export function auditTreatment(failures: Partial<Record<TreatmentId, string>>): { pass: boolean; items: TreatmentItem[] } {
  const items = SYSTEM_TREATMENT_CHECKLIST.map((t) => {
    const fail = failures[t.id];
    return { ...t, entitled: !fail, reason: fail ?? "系统级待遇正常享受" };
  });
  return { pass: items.every((i) => i.entitled), items };
}

/* ---------- ④ 诚实边界（UA 与回退路径） ---------- */

export interface UaPolicy {
  /** UA 产品令牌：诚实身份，永不伪装。 */
  productToken: string;
  /** 允许伪装浏览器身份 = false（判据红线）。 */
  spoofingAllowed: false;
}

export function uaPolicy(): UaPolicy {
  return { productToken: "VARIX", spoofingAllowed: false };
}

export interface CompatIssueRecord {
  url: string;
  issue: string;
  at: number;
  /** 回退路径：A 域判例工厂引用号。 */
  caseFactoryRef: string;
}

/** 兼容性问题登记 → 回退路径（A2 判例工厂引用）；伪装不是选项。 */
export function recordCompatIssue(url: string, issue: string, at: number, store: KvStore = defaultStore()): CompatIssueRecord {
  const rec: CompatIssueRecord = { url, issue, at, caseFactoryRef: "A2/case-factory" };
  const all = readJson<CompatIssueRecord[]>(store, h4Key("f355", "compat"), [], Array.isArray);
  writeJson(store, h4Key("f355", "compat"), [...all.slice(-49), rec]);
  return rec;
}

export function listCompatIssues(store: KvStore = defaultStore()): CompatIssueRecord[] {
  return readJson<CompatIssueRecord[]>(store, h4Key("f355", "compat"), [], Array.isArray);
}
