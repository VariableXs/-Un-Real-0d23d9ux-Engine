/**
 * 复制链七件（AI-U3 · F524 后悔窗 / F529 空间预检 / F530 复制后校验 /
 * F531 队列化 / F532 人话诊断 / F533 只读提醒 / F534 长路径）。
 *
 * 判据唯一源（主册摘文）：
 * - F524「清空执行后通知条驻留 5 秒（已清空回收站 N 项——撤销）；清空动作
 *   内部先暂存、超时才真释放；长按撤销条可延寿（再给 10 秒）；5 秒后没有任何
 *   办法——诚实」。
 * - F529「目标卷剩余 <所需（含 10% 缓冲）时开跑前拦下；三选出路（仍要复制/
 *   换目标/取消）；预检在进度对话框出现前完成；多目标批逐盘预检」。
 * - F530「>1GB 自动开默认；源/目标哈希比对后台做；不符时明确告知；校验走
 *   空闲 IO；关闭开关」。
 * - F531「同时发起的多批复制自动排队（同盘串行、异盘可并行）；每批独立
 *   进度/暂停/取消/优先级；『先传这批』右键插队」。
 * - F532「四类人话归因（格式不支持/文件损坏/应用缺失/权限不足）+为什么+
 *   现在能做什么（每类配出路：F257/F294/Edge/F324）」。
 * - F533「只读卷写前置提醒（写保护开关/只读挂载）；角标可见；提示一次不重复」。
 * - F534「600+ 字符路径全操作；索引覆盖；显示保尾；复制路径完整」。
 */

import { u3Store } from "./u3store";

/* ------------------------------- F524 撤销清空回收站 ------------------------------- */

/** 后悔窗 5s（主册 F524 判据）；长按延寿 10s；延寿上限 2 次（偏差登记见内核 ustar3 v1 §4）。 */
export const UNDO_BIN_WINDOW_MS = 5000;
export const UNDO_BIN_EXTEND_MS = 10000;
export const UNDO_BIN_MAX_EXTENDS = 2;

export interface UndoBinRt {
  staged: string[];       // 暂存项（真释放前不删盘面账目）
  expiresAt: number;
  extends: number;
  released: boolean;
}

/** 清空 = 先暂存 + 开窗（判据：清空动作内部先暂存、超时才真释放）。 */
export function undoBinOpen(items: string[], now: number): UndoBinRt {
  return { staged: [...items], expiresAt: now + UNDO_BIN_WINDOW_MS, extends: 0, released: false };
}

/** 长按延寿（判据：长按撤销条可延寿再给 10 秒；上限防无限拖延）。 */
export function undoBinExtend(rt: UndoBinRt): boolean {
  if (rt.released || rt.extends >= UNDO_BIN_MAX_EXTENDS) return false;
  rt.extends += 1;
  rt.expiresAt += UNDO_BIN_EXTEND_MS;
  return true;
}

/** tick：超时真释放（判据：真释放时机与账目同步；超时后诚实不可恢复）。 */
export function undoBinTick(rt: UndoBinRt, now: number): { expired: boolean } {
  if (!rt.released && now >= rt.expiresAt) {
    rt.released = true;
    rt.staged = [];
    return { expired: true };
  }
  return { expired: false };
}

/** 撤销：窗口内全部还原（判据：撤销完整性 N 项全回）。 */
export function undoBinRestore(rt: UndoBinRt): string[] | null {
  if (rt.released) return null;
  const items = [...rt.staged];
  rt.released = true;
  rt.staged = [];
  return items;
}

/* ------------------------------- F529 空间预检 ------------------------------- */

/** 缓冲 10%（主册 F529 规格表：防止「刚好放下然后系统卡死」）。 */
export const SPACE_CHECK_BUFFER_PCT = 10;

export interface SpaceCheckInput {
  totalBytes: number;
  perTargetFree: Record<string, number>; // 目标卷 → 剩余字节
  perTargetNeed: Record<string, number>; // 目标卷 → 所需字节
}

export type SpaceDecision = "proceed" | "change-target" | "cancel";

/** 预检（判据：所需含 10% 缓冲；跨盘批逐盘检；三选出路）。 */
export function spaceCheck(input: SpaceCheckInput): { ok: boolean; shortfalls: Array<{ volume: string; freeGB: number; needGB: number }> } {
  const shortfalls: Array<{ volume: string; freeGB: number; needGB: number }> = [];
  for (const vol of Object.keys(input.perTargetNeed)) {
    const need = (input.perTargetNeed[vol] ?? 0) * (1 + SPACE_CHECK_BUFFER_PCT / 100);
    const free = input.perTargetFree[vol] ?? 0;
    if (free < need) shortfalls.push({ volume: vol, freeGB: free / 1024 ** 3, needGB: need / 1024 ** 3 });
  }
  return { ok: shortfalls.length === 0, shortfalls };
}

/** 预检人话文案（判据：「目标盘只剩 2.1GB，这批要 3.4GB——放不下」）。 */
export function shortfallMessage(s: { volume: string; freeGB: number; needGB: number }): string {
  return `目标盘（${s.volume}）只剩 ${s.freeGB.toFixed(1)}GB，这批要 ${s.needGB.toFixed(1)}GB（含 10% 缓冲）——放不下`;
}

/* ------------------------------- F530 复制后校验 ------------------------------- */

/** >1GB 自动开默认（主册 F530 判据）。 */
export const COPY_VERIFY_AUTO_ABOVE = 1024 ** 3;

export interface VerifyRt { enabled: boolean; autoAboveBytes: number }

/** 校验决策（判据：>1GB 自动开；关闭开关；小文件不校验）。 */
export function shouldVerify(rt: VerifyRt, sizeBytes: number, userForced?: boolean): boolean {
  if (userForced !== undefined) return userForced;
  if (!rt.enabled) return false;
  return sizeBytes >= rt.autoAboveBytes;
}

/** 校验结果（判据：失败报告完整性——源/目标路径+人话建议）。 */
export function verifyReport(ok: boolean, src: string, dst: string): { ok: boolean; message: string } {
  return ok
    ? { ok: true, message: "校验一致" }
    : { ok: false, message: `校验失败——建议重新复制该文件（源：${src} / 目标：${dst}）` };
}

/* ------------------------------- F531 复制任务队列化 ------------------------------- */

export type CopyTaskState = "queued" | "running" | "paused" | "done" | "canceled";

export interface CopyTask {
  id: string;
  volume: string;
  state: CopyTaskState;
  priority: number; // 小者先
}

/**
 * 队列调度（判据：同盘串行/异盘并行判定；插队优先级；暂停取消独立）。
 * parallel 每盘 1、异盘上限由配置决定（默认 2 盘并行）。
 */
export function scheduleQueue(tasks: CopyTask[], runningVolumeSet: Set<string>, crossDiskParallel: number): CopyTask[] {
  const activeByVolume = new Set(runningVolumeSet);
  const picks: CopyTask[] = [];
  const slots = Math.max(0, crossDiskParallel - activeByVolume.size);
  const eligible = tasks
    .filter((t) => t.state === "queued")
    .sort((a, b) => a.priority - b.priority);
  for (const t of eligible) {
    if (picks.length >= slots) break;
    if (activeByVolume.has(t.volume)) continue; // 同盘串行：该盘已有任务在跑
    picks.push(t);
    activeByVolume.add(t.volume);
  }
  return picks;
}

/** 插队（判据：「先传这批」右键插队——优先级调到队首）。 */
export function jumpQueue(tasks: CopyTask[], id: string): CopyTask[] {
  const minPriority = tasks.reduce((m, t) => Math.min(m, t.priority), Infinity);
  return tasks.map((t) => (t.id === id ? { ...t, priority: minPriority - 1 } : t));
}

/* ------------------------------- F532 打开失败人话诊断 ------------------------------- */

export type OpenFailKind = "format" | "corrupt" | "app-missing" | "permission";

export interface OpenFailDiagnosis {
  kind: OpenFailKind;
  what: string;   // 三问之一：是什么问题
  why: string;    // 三问之二：为什么
  next: string;   // 三问之三：现在能做什么（配出路链接）
  route: "f257" | "f294" | "edge-search" | "f324" | "f325";
}

/** 头部签名快判（判据：损坏检测判据——头 512 字节签名）。 */
const MAGIC_TABLE: Array<{ kind: OpenFailKind; test: (head: Uint8Array) => boolean; label: string }> = [
  { kind: "corrupt", test: (h) => h.length >= 4 && h[0] === 0x50 && h[1] === 0x4b && !(h[2] === 0x03 && h[3] === 0x04) && !(h[2] === 0x05 && h[3] === 0x06) && !(h[2] === 0x07 && h[3] === 0x08), label: "ZIP" },
  { kind: "corrupt", test: (h) => h.length >= 4 && h[0] === 0x25 && h[1] === 0x50 && h[2] === 0x44 && h[3] === 0x46 !== true && h[0] === 0x25, label: "PDF" },
];

/** 四类归因（判据：四类归因准确率——注入用例；三问结构 F209 同规）。 */
export function diagnoseOpenFail(head: Uint8Array | null, appName: string | null, permDenied: boolean): OpenFailDiagnosis {
  if (permDenied) {
    return {
      kind: "permission",
      what: "权限不足",
      why: "当前账户没有读取该文件的授权",
      next: "到权限中心查看与申请授权",
      route: "f324",
    };
  }
  if (head && MAGIC_TABLE.some((m) => m.test(head))) {
    return {
      kind: "corrupt",
      what: "文件可能已损坏",
      why: "文件头签名与格式规范不符",
      next: "从备份或文件历史版本恢复（F325/F396）",
      route: "f325",
    };
  }
  if (!appName) {
    return {
      kind: "app-missing",
      what: "没有能打开这种文件的应用",
      why: "系统里没有注册处理此格式的应用",
      next: "用「打开方式」选择器挑选应用，或去 Edge 搜索合适的应用",
      route: "f257",
    };
  }
  return {
    kind: "format",
    what: "格式不受支持",
    why: `${appName} 不支持此格式`,
    next: "换一个应用打开（打开方式选择器）",
    route: "f257",
  };
}

/* ------------------------------- F533 只读介质提醒 ------------------------------- */

export interface VolumeRo { volume: string; readOnly: boolean; cause: "switch" | "mount" | "policy" }

/** 前置提醒（判据：写保护检测+前置时机+提示一次不重复）。 */
export function readOnlyReminder(vol: VolumeRo, alreadyReminded: Set<string>): { remind: boolean; message: string } {
  if (!vol.readOnly || alreadyReminded.has(vol.volume)) return { remind: false, message: "" };
  alreadyReminded.add(vol.volume);
  const causeText = vol.cause === "switch" ? "物理写保护开关" : vol.cause === "mount" ? "以只读方式挂载" : "系统策略只读";
  return {
    remind: true,
    message: `此盘处于只读状态（${causeText}）——检查物理写保护开关或以可写方式重新挂载`,
  };
}

/* ------------------------------- F534 长路径全程支持 ------------------------------- */

/** 显示保尾：保尾字符数（判据：地址栏与 Tooltip 截断保尾保文件名）。 */
export const LONG_PATH_TAIL_KEEP = 24;

export function longPathDisplay(path: string, tailKeep = LONG_PATH_TAIL_KEEP): string {
  if (path.length <= 60) return path;
  return `${path.slice(0, 24)}…${path.slice(-tailKeep)}`;
}

/** 长路径合法性：>260 字符仍可操作（判据：600+ 字符路径全操作）。 */
export function longPathOk(path: string): boolean {
  return path.length >= 260 ? !path.split(/[\\/]/).some((seg) => seg.length > 255) : true;
}

/* ------------------------------- 运行时读取 ------------------------------- */

export function spaceCheckConfig() {
  const s = u3Store.get("spaceCheck");
  return { bufferPct: (s.bufferPct as number) ?? SPACE_CHECK_BUFFER_PCT };
}
export function copyVerifyConfig() {
  const s = u3Store.get("copyVerify");
  return { enabled: (s.enabled as boolean) ?? true, autoAboveBytes: (s.autoAboveBytes as number) ?? COPY_VERIFY_AUTO_ABOVE };
}

/* ------------------------------- 自检 ------------------------------- */

/** F524/F529-F534 判据自检（前端面）。 */
export function copyopsSelfCheck(): Array<{ name: string; pass: boolean }> {
  const checks: Array<{ name: string; pass: boolean }> = [];
  // 后悔窗：暂存→撤销全回 / 超时真释放诚实不可恢复 / 延寿上限
  let now = 0;
  const rt = undoBinOpen(["a", "b", "c"], now);
  now += 4000;
  undoBinExtend(rt);
  now += 9000; // 4s+10s=14s 未超时
  const restored = undoBinRestore(rt);
  checks.push({ name: "F524 延寿+全量还原", pass: !!restored && restored.length === 3 });
  const rt2 = undoBinOpen(["x"], 0);
  undoBinExtend(rt2); undoBinExtend(rt2);
  checks.push({ name: "F524 延寿上限 2 次", pass: !undoBinExtend(rt2) && rt2.expiresAt === 5000 + 20000 });
  const rt3 = undoBinOpen(["y"], 0);
  undoBinTick(rt3, 5001);
  checks.push({ name: "F524 超时真释放", pass: rt3.released && undoBinRestore(rt3) === null });
  // 空间预检
  const sc = spaceCheck({ totalBytes: 0, perTargetFree: { "D:": 2.1 * 1024 ** 3 }, perTargetNeed: { "D:": 3.4 * 1024 ** 3 } });
  checks.push({ name: "F529 预检拦截", pass: !sc.ok && sc.shortfalls[0]?.volume === "D:" });
  checks.push({ name: "F529 人话文案", pass: !sc.ok && shortfallMessage(sc.shortfalls[0]!).includes("10% 缓冲") });
  // 校验决策
  const vr: VerifyRt = { enabled: true, autoAboveBytes: COPY_VERIFY_AUTO_ABOVE };
  checks.push({ name: "F530 >1GB 自动开", pass: shouldVerify(vr, 2 * 1024 ** 3) && !shouldVerify(vr, 100 * 1024 * 1024) && !shouldVerify({ ...vr, enabled: false }, 2 * 1024 ** 3) });
  // 队列：同盘串行/异盘并行/插队
  const q: CopyTask[] = [
    { id: "1", volume: "C:", state: "queued", priority: 5 },
    { id: "2", volume: "C:", state: "queued", priority: 1 },
    { id: "3", volume: "D:", state: "queued", priority: 9 },
  ];
  const picks = scheduleQueue(q, new Set(), 2);
  checks.push({ name: "F531 同盘串行异盘并行", pass: picks.length === 2 && picks.every((p) => p.volume !== "C:" || picks.filter((x) => x.volume === "C:").length === 1) });
  const jumped = jumpQueue(q, "3");
  checks.push({ name: "F531 插队", pass: scheduleQueue(jumped, new Set(), 2)[0]?.id === "3" });
  // 诊断四类
  const d1 = diagnoseOpenFail(new Uint8Array([0x50, 0x4b, 0x00, 0x00]), "编辑器", false);
  const d2 = diagnoseOpenFail(null, null, false);
  const d3 = diagnoseOpenFail(null, "编辑器", true);
  checks.push({ name: "F532 四类归因+三问", pass: d1.kind === "corrupt" && d2.kind === "app-missing" && d3.kind === "permission" && d1.what !== "" && d1.why !== "" && d1.next !== "" });
  // 只读提醒一次
  const seen = new Set<string>();
  const u = readOnlyReminder({ volume: "E:", readOnly: true, cause: "switch" }, seen);
  const again = readOnlyReminder({ volume: "E:", readOnly: true, cause: "switch" }, seen);
  checks.push({ name: "F533 提示一次不重复", pass: u.remind && !again.remind && u.message.includes("写保护开关") });
  // 长路径（100 层嵌套 ≈1900 字符——判据「600+ 字符路径全操作」）
  const long = "C:\\" + Array.from({ length: 100 }, (_, i) => `directory-level-${i}`).join("\\");
  checks.push({ name: "F534 长路径保尾+合法", pass: long.length > 600 && longPathDisplay(long).includes("…") && longPathOk(long) });
  return checks;
}
