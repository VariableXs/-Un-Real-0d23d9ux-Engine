/**
 * 文件安全四件（AI-U3 · F509 粉碎 / F510 单文件加密 / F511 剪贴板清空 / F512 截图历史）。
 *
 * 判据唯一源（主册摘文）：
 * - F509「确认对话框三重警示（文件名/数量/此操作不可恢复——不经过回收站）
 *   + 默认焦点取消（F207）；覆写执行或诚实标注两分支（介质不支持覆写时
 *   诚实标注「此介质无法保证覆写，建议启用全盘加密 F439」）；耗时提示」。
 * - F510「密码派生密钥；文件夹整体打包加密为 .vxcrypt 单文件；原文件可选
 *   保留（默认不保留）但提示一次；打开 .vxcrypt 双击输密码解出临时视图
 *   （关闭即隐——阅后即焚式浏览）；导出解密副本需明确选择；密码丢了就是
 *   真丢了（无后门——诚实告知）」。
 * - F511「双入口（快速设置磁贴 + Ctrl+Shift+Delete）；当前+历史全清；
 *   确认框列条数；密码后提示条 5 秒；清空后粘贴行为（空——应用得到诚实
 *   失败）」。
 * - F512「最近 20 张缩略条；临时/已保存两态；未保存的关机即失（F311 同源，
 *   通知一次）；历史持久仅含已保存项的引用」。
 */

import { u3Store } from "./u3store";

/* ------------------------------- F509 文件粉碎 ------------------------------- */

/** 覆写遍数（默认 3：全 0/全 1/随机—— gutmann 简化族）。 */
export const SHRED_DEFAULT_PASSES = 3;

export type ShredMediumCapability = "overwrite-ok" | "wear-leveling";

/**
 * 介质覆写能力分派（判据：覆写执行或诚实标注两分支）。
 * U 盘（wear-leveling 磨损均衡）无法保证覆写——诚实标注并指向 F439。
 */
export function shredPlan(medium: ShredMediumCapability, files: string[]): {
  mode: "overwrite" | "honest-label";
  passes: number;
  warning: string;
  estSecondsPerMB: number;
} {
  if (medium === "overwrite-ok") {
    return {
      mode: "overwrite",
      passes: SHRED_DEFAULT_PASSES,
      warning: "",
      estSecondsPerMB: 0.02 * SHRED_DEFAULT_PASSES, // 大文件覆写慢——耗时提示数据源
    };
  }
  return {
    mode: "honest-label",
    passes: 0,
    warning: "此介质无法保证覆写（磨损均衡），建议启用全盘加密（F439）后再使用粉碎",
    estSecondsPerMB: 0,
  };
}

/** 三重警示（判据：文件名/数量/不可恢复三要素 + 默认焦点在「取消」）。 */
export function shredWarningItems(files: string[]): { names: string; count: number; irrecoverable: string; defaultFocus: "cancel" } {
  return {
    names: files.slice(0, 5).join("、") + (files.length > 5 ? ` 等 ${files.length} 项` : ""),
    count: files.length,
    irrecoverable: "此操作不可恢复——不经过回收站",
    defaultFocus: "cancel", // F207 铁律：危险操作默认焦点取消
  };
}

/** 普通删除不受影响声明（判据：普通删除的后悔药 F261 不受影响）。 */
export const SHRED_ISOLATION_NOTE = "粉碎与普通删除完全隔离：普通删除仍走回收站（F261 后悔药保留）";

/* ------------------------------- F510 单文件加密 ------------------------------- */

export const VXCRYPT_EXT = ".vxcrypt";
/** 容器魔数与版本（诚实格式：可导出可迁移，十四章开放格式）。 */
export const VXCRYPT_MAGIC = "VXC1";
/** PBKDF2 迭代次数（OWASP 2023 建议 ≥600k；本域取 600,000——密码派生密钥）。 */
export const VXCRYPT_PBKDF2_ITERS = 600_000;
const PBKDF2_ITERS_TEST = 10_000; // 单测加速档（format 里登记 iters，读取自适应）

export interface VxCryptHeader {
  magic: typeof VXCRYPT_MAGIC;
  iters: number;
  saltB64: string;
  ivB64: string;
}

function b64(buf: ArrayBuffer): string {
  return btoa(String.fromCharCode(...new Uint8Array(buf)));
}
function unb64(s: string): Uint8Array {
  return Uint8Array.from(atob(s), (c) => c.charCodeAt(0));
}

async function deriveKey(password: string, salt: Uint8Array, iters: number): Promise<CryptoKey> {
  const base = await crypto.subtle.importKey("raw", new TextEncoder().encode(password), "PBKDF2", false, ["deriveKey"]);
  return crypto.subtle.deriveKey(
    { name: "PBKDF2", salt: salt as unknown as BufferSource, iterations: iters, hash: "SHA-256" },
    base,
    { name: "AES-GCM", length: 256 },
    false,
    ["encrypt", "decrypt"],
  );
}

/**
 * 加密字节流 → .vxcrypt 容器（AES-256-GCM 认证加密；密码派生走 PBKDF2-SHA256）。
 * iters 可注入以加速单测；真实路径默认 600k。密码错误 = GCM 认证失败（不泄露信息）。
 */
export async function vxcryptEncryptBytes(plain: Uint8Array, password: string, iters = VXCRYPT_PBKDF2_ITERS): Promise<Uint8Array> {
  if (!password) throw new Error("[u3:F510] 密码不能为空——空密码等于没加密");
  const salt = crypto.getRandomValues(new Uint8Array(16));
  const iv = crypto.getRandomValues(new Uint8Array(12));
  const key = await deriveKey(password, salt, iters);
  const cipher = await crypto.subtle.encrypt({ name: "AES-GCM", iv: iv as unknown as BufferSource }, key, plain as unknown as BufferSource);
  const header: VxCryptHeader = { magic: VXCRYPT_MAGIC, iters, saltB64: b64(salt.buffer as ArrayBuffer), ivB64: b64(iv.buffer as ArrayBuffer) };
  const head = new TextEncoder().encode(JSON.stringify(header));
  const len = new Uint8Array(4);
  new DataView(len.buffer).setUint32(0, head.length, true);
  const out = new Uint8Array(VXCRYPT_MAGIC.length + 4 + head.length + cipher.byteLength);
  out.set(new TextEncoder().encode(VXCRYPT_MAGIC), 0);
  out.set(len, VXCRYPT_MAGIC.length);
  out.set(head, VXCRYPT_MAGIC.length + 4);
  out.set(new Uint8Array(cipher), VXCRYPT_MAGIC.length + 4 + head.length);
  return out;
}

/** 解密（判据：密码错误提示——GCM 认证失败给出人话而非裸异常码）。 */
export async function vxcryptDecryptBytes(container: Uint8Array, password: string): Promise<Uint8Array> {
  const magic = new TextDecoder().decode(container.slice(0, VXCRYPT_MAGIC.length));
  if (magic !== VXCRYPT_MAGIC) {
    throw new Error("[u3:F510] 不是有效的 .vxcrypt 文件（魔数不符）");
  }
  const dv = new DataView(container.buffer, container.byteOffset, container.byteLength);
  const headLen = dv.getUint32(VXCRYPT_MAGIC.length, true);
  const headStart = VXCRYPT_MAGIC.length + 4;
  const header = JSON.parse(new TextDecoder().decode(container.slice(headStart, headStart + headLen))) as VxCryptHeader;
  const iters = header.iters >= 10_000 ? header.iters : PBKDF2_ITERS_TEST; // 容器内登记值优先
  const key = await deriveKey(password, unb64(header.saltB64), iters);
  try {
    const plain = await crypto.subtle.decrypt(
      { name: "AES-GCM", iv: unb64(header.ivB64) as unknown as BufferSource },
      key,
      container.slice(headStart + headLen) as unknown as BufferSource,
    );
    return new Uint8Array(plain);
  } catch {
    throw new Error("[u3:F510] 密码错误或文件已损坏——密码丢了就是真丢了（本格式无后门）");
  }
}

/** 原文件去留默认值与一次性提示语义（判据：默认不保留但提示一次）。 */
export const ONE_CRYPT_KEEP_DEFAULT = false;
export const ONE_CRYPT_KEEP_NOTE = "加密后原文件默认不保留（加密的意图就是藏）——如需保留请在本次勾选（仅提示这一次）";

/** 临时视图无痕判据（判据：关闭后临时区清零——阅后即焚）。 */
export const TEMP_VIEW_TARGETS = ["temp-view-blob", "temp-view-thumbnail", "temp-view-export-cache"] as const;
export function tempViewCleanupPlan(): readonly string[] {
  return TEMP_VIEW_TARGETS;
}

/* ------------------------------- F511 剪贴板一键清空 ------------------------------- */

/** 快捷键（主册：Ctrl+Shift+Delete VARIX 组合）。 */
export const CLIP_WIPE_HOTKEY = "Ctrl+Shift+Delete";
/** 密码后提示条驻留 5s（主册 F511 规格表）。 */
export const CLIP_SECRET_PROMPT_MS = 5000;

/**
 * 清空执行（判据：当前+历史全清；确认框列条数）。
 * clipboard 引用注入（测试不碰真实剪贴板）；历史条目数组就地清空。
 */
export async function wipeClipboard(clipboard: { writeText(t: string): Promise<void> } | null, history: unknown[]): Promise<number> {
  let wiped = history.length;
  history.length = 0;
  if (clipboard) {
    try {
      await clipboard.writeText(""); // 清空后粘贴行为 = 空（应用得到诚实失败）
    } catch (e) {
      console.error("[u3:F511] 剪贴板写入失败（历史已清，当前板可能残留）", e);
      throw e; // 异常显性化（十三章），但历史清理不回滚
    }
  }
  return wiped;
}

/** 敏感复制后的自觉提示（判据：输完密码后系统提示条 5 秒）。 */
export function secretCopyPrompt(now: number): { show: boolean; expireAt: number } {
  return { show: true, expireAt: now + CLIP_SECRET_PROMPT_MS };
}

/* ------------------------------- F512 截图历史 ------------------------------- */

/** 20 条上限（主册 F512 判据）；淘汰最旧（未保存优先淘汰）。 */
export const SHOT_HISTORY_CAP = 20;

export interface ShotEntry {
  id: string;
  createdAt: number;
  saved: boolean;       // 临时/已保存两态
  path: string;         // 已保存项 = 真实路径；临时项 = 空
  thumbDataUrl: string;
}

/** 入架：满 20 条先淘汰最旧的临时项，再淘汰最旧的已保存项（判据：上限与淘汰）。
 *  淘汰候选不含本次新入架条目自身——新图永远先入架再参与后续淘汰。 */
export function pushShot(entries: ShotEntry[], entry: ShotEntry): ShotEntry[] {
  const next = [...entries, entry];
  if (next.length <= SHOT_HISTORY_CAP) return next;
  const candidates = next.slice(0, next.length - 1); // 排除新条目自身
  const tmpIdx = candidates.findIndex((e) => !e.saved);
  next.splice(tmpIdx >= 0 ? tmpIdx : 0, 1);
  return next;
}

/** 关机清理：未保存项即失（判据：关机清理与通知）；历史持久仅含已保存项引用。 */
export function shutdownShotPlan(entries: ShotEntry[]): { dropped: number; kept: ShotEntry[]; notifyOnce: string } {
  const kept = entries.filter((e) => e.saved);
  return {
    dropped: entries.length - kept.length,
    kept,
    notifyOnce: `有 ${entries.length - kept.length} 张临时截图未保存，关机后将被清除（仅提醒这一次）`,
  };
}

/* ------------------------------- 自检 ------------------------------- */

/** F509-F512 判据自检（前端面）。 */
export function filesecSelfCheck(): Array<{ name: string; pass: boolean }> {
  const checks: Array<{ name: string; pass: boolean }> = [];
  const plan = shredPlan("wear-leveling", ["a.txt"]);
  checks.push({ name: "F509 诚实标注分支", pass: plan.mode === "honest-label" && plan.warning.includes("F439") });
  checks.push({ name: "F509 三重警示+焦点取消", pass: shredWarningItems(["a", "b"]).defaultFocus === "cancel" });
  checks.push({ name: "F510 阅后即焚清单", pass: tempViewCleanupPlan().length === 3 });
  // 容器魔数常量在位（round-trip 与错误密码路径由单测覆盖——WebCrypto 异步）
  checks.push({ name: "F510 容器魔数+派生参数", pass: VXCRYPT_MAGIC === "VXC1" && VXCRYPT_PBKDF2_ITERS >= 600_000 });
  // 截图历史：上限淘汰优先临时项
  const entries: ShotEntry[] = Array.from({ length: SHOT_HISTORY_CAP }, (_, i) => ({ id: `s${i}`, createdAt: i, saved: true, path: "", thumbDataUrl: "" }));
  const withTemp = pushShot(entries, { id: "new", createdAt: 999, saved: false, path: "", thumbDataUrl: "" });
  checks.push({ name: "F512 20 条上限", pass: withTemp.length === SHOT_HISTORY_CAP });
  const fullSaved = Array.from({ length: SHOT_HISTORY_CAP }, (_, i) => ({ id: `s${i}`, createdAt: i, saved: true, path: "", thumbDataUrl: "" }));
  const pushSaved = pushShot(fullSaved, { id: "new", createdAt: 999, saved: true, path: "", thumbDataUrl: "" });
  checks.push({ name: "F512 淘汰最旧", pass: pushSaved.length === SHOT_HISTORY_CAP && !pushSaved.some((e) => e.id === "s0") });
  // 关机清理只留已保存
  const sd = shutdownShotPlan([...withTemp]);
  checks.push({ name: "F512 关机清理两态", pass: sd.dropped === 1 && sd.kept.every((e) => e.saved) });
  return checks;
}
