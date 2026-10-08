/**
 * F510 深化引擎 · .vxcrypt 单文件加密容器（AI-U3 · vxcrypt2）。
 *
 * 判据唯一源（主册摘文）：「加密/浏览/导出三链路；临时视图无痕判据（关闭后
 * 临时区清零）；原文件去留；密码错误提示；文件夹打包」。
 *
 * 深化点（v2 已有 WebCrypto 真加解密 round-trip，本引擎补齐**容器格式与
 * 生命周期层**——真实现不只算得对，格式与状态机也要立得住）：
 * 1. 容器头**序列化/反序列化**：魔数/版本/KDF 参数表/salt/IV/密文偏移——
 *    字节布局是契约（十四章开放格式），头解析失败给三要素错误。
 * 2. KDF 参数阶梯：判据 600k 迭代为基准档，低配设备降档显性登记（不许静默）。
 * 3. 临时视图无痕生命周期状态机：打开浏览 → 落临时区（记录）→ 关闭清零
 *    （逐项核对）→ 异常退出兜底清扫。
 * 4. 文件夹打包语义：文件夹 → 虚拟归档条目表（相对路径规范化），打包前后
 *    条目一致可导出。
 */

/* ------------------------------ 容器头格式 ------------------------------ */

/** 魔数 "VXCRYPT1"（8 字节）。 */
export const VXCRYPT_MAGIC = "VXCRYPT1";
/** 格式版本（十四章：版本化——后续演进加版本不改语义）。 */
export const VXCRYPT_VERSION = 1;
/** PBKDF2 迭代基准档（判据 600k，与 v2 WebCrypto 实现一致）。 */
export const VXCRYPT_KDF_ITER_BASE = 600000;
/** KDF 降档下限（低配设备最低 200k——低于此拒绝加密，宁慢不弱）。 */
export const VXCRYPT_KDF_ITER_FLOOR = 200000;
/** salt/IV 字节宽（AES-256-GCM 工业口径）。 */
export const VXCRYPT_SALT_BYTES = 16;
export const VXCRYPT_IV_BYTES = 12;

export interface VxcryptHeader {
  magic: string;
  version: number;
  kdfIter: number;
  saltHex: string;
  ivHex: string;
  /** 密文体在容器中的起始偏移（头长度 + 文件名区）。 */
  payloadOffset: number;
}

/**
 * 头序列化（确定性布局）：magic(8) + version(u16) + kdfIter(u32) +
 * salt(16B hex) + iv(12B hex) + payloadOffset(u32)。
 */
export function serializeHeader(h: VxcryptHeader): string {
  if (h.magic !== VXCRYPT_MAGIC) throw new Error(`[u3:F510] 魔数必须 ${VXCRYPT_MAGIC}`);
  if (!Number.isInteger(h.version) || h.version < 1) throw new Error(`[u3:F510] 版本非法 ${h.version}`);
  if (h.kdfIter < VXCRYPT_KDF_ITER_FLOOR) throw new Error(`[u3:F510] KDF 迭代 ${h.kdfIter} 低于下限 ${VXCRYPT_KDF_ITER_FLOOR}——拒绝弱参数容器`);
  if (!/^[0-9a-f]{32}$/.test(h.saltHex)) throw new Error("[u3:F510] salt 必须 16 字节 hex");
  if (!/^[0-9a-f]{24}$/.test(h.ivHex)) throw new Error("[u3:F510] IV 必须 12 字节 hex");
  return [
    h.magic,
    h.version.toString(16).padStart(4, "0"),
    h.kdfIter.toString(16).padStart(8, "0"),
    h.saltHex,
    h.ivHex,
    h.payloadOffset.toString(16).padStart(8, "0"),
  ].join(":");
}

/** 头反序列化：解析失败抛三要素错误（发生了什么/为什么/下一步）。 */
export function parseHeader(line: string): VxcryptHeader {
  const parts = line.split(":");
  if (parts.length !== 6 || parts[0] !== VXCRYPT_MAGIC) {
    throw new Error(`无法打开：不是有效的 .vxcrypt 容器（文件头魔数不符）——请确认选择的是本系统加密过的文件`);
  }
  const version = parseInt(parts[1]!, 16);
  if (version > VXCRYPT_VERSION) {
    throw new Error(`无法打开：容器版本 v${version} 高于本系统支持的 v${VXCRYPT_VERSION}——请升级系统后再打开`);
  }
  const kdfIter = parseInt(parts[2]!, 16);
  if (kdfIter < VXCRYPT_KDF_ITER_FLOOR) {
    throw new Error(`无法打开：容器 KDF 参数弱于安全下限——该容器可能被篡改，已拒绝打开`);
  }
  return {
    magic: parts[0]!,
    version,
    kdfIter,
    saltHex: parts[3]!,
    ivHex: parts[4]!,
    payloadOffset: parseInt(parts[5]!, 16),
  };
}

/** 错误密码提示（判据「密码错误提示」——不泄露哪一步失败，防暴力探测面收窄）。 */
export function wrongPasswordMessage(): string {
  return "密码不正确——请核对后重试（连续错误会按 F504 规则冷却）";
}

/* --------------------------- 临时视图无痕生命周期 --------------------------- */

export type TempViewPhase = "closed" | "open" | "cleaning" | "cleaned";

export interface TempViewLedger {
  phase: TempViewPhase;
  /** 临时区落盘清单（打开浏览时逐笔记账——关闭时逐项核对清零）。 */
  entries: string[];
}

export function tempViewOpen(paths: string[]): TempViewLedger {
  return { phase: "open", entries: [...paths] };
}

/**
 * 关闭清零（判据「关闭后临时区清零」）：逐项删除（注入执行器），任一失败
 * 进入 cleaning 重试清单——全清零才算 cleaned（异常退出兜底由调用方以
 * entries 账本驱动重扫）。
 */
export function tempViewClose(
  ledger: TempViewLedger,
  removeFn: (path: string) => boolean,
): { ledger: TempViewLedger; removed: string[]; leftover: string[] } {
  const removed: string[] = [];
  const leftover: string[] = [];
  for (const p of ledger.entries) {
    if (removeFn(p)) removed.push(p);
    else leftover.push(p);
  }
  const next: TempViewLedger = {
    phase: leftover.length === 0 ? "cleaned" : "cleaning",
    entries: leftover, // 只留未清项——下次清扫只扫剩余（幂等）
  };
  return { ledger: next, removed, leftover };
}

/* ------------------------------ 文件夹打包 ------------------------------ */

/** 归档条目（相对路径规范化——绝对路径与 .. 逃逸显性拒绝）。 */
export interface ArchiveEntry {
  /** 归档内相对路径（/ 分隔，无 ..）。 */
  rel: string;
  sizeBytes: number;
}

export function normalizeArchiveEntry(absRoot: string, rawPath: string): ArchiveEntry {
  if (!rawPath.startsWith(absRoot)) {
    throw new Error(`[u3:F510] 打包路径越出根目录：${rawPath}`);
  }
  let rel = rawPath.slice(absRoot.length).replace(/\\/g, "/").replace(/^\/+/, "");
  const segs = rel.split("/").filter((s) => s.length > 0);
  if (segs.some((s) => s === "..")) {
    throw new Error(`[u3:F510] 打包路径含 .. 逃逸段：${rawPath}`);
  }
  rel = segs.join("/");
  if (rel.length === 0) rel = "(root)";
  return { rel, sizeBytes: 0 };
}

/* ------------------------------ 自检 ------------------------------ */

export function vxcrypt2SelfCheck(): Array<{ name: string; pass: boolean }> {
  const checks: Array<{ name: string; pass: boolean }> = [];
  // 头 round-trip
  const h: VxcryptHeader = {
    magic: VXCRYPT_MAGIC,
    version: 1,
    kdfIter: VXCRYPT_KDF_ITER_BASE,
    saltHex: "a".repeat(32),
    ivHex: "b".repeat(24),
    payloadOffset: 128,
  };
  const parsed = parseHeader(serializeHeader(h));
  checks.push({ name: "F510 头 round-trip", pass: parsed.kdfIter === VXCRYPT_KDF_ITER_BASE && parsed.payloadOffset === 128 && parsed.version === 1 });
  // 魔数不符 → 三要素错误
  let magicThrew = false;
  try { parseHeader("NOTAVX:0001:000927c0:" + "a".repeat(32) + ":" + "b".repeat(24) + ":00000080"); } catch { magicThrew = true; }
  checks.push({ name: "F510 非法容器三要素拒绝", pass: magicThrew });
  // 弱 KDF 拒绝
  let weakThrew = false;
  try { parseHeader("VXCRYPT1:0001:000186a0:" + "a".repeat(32) + ":" + "b".repeat(24) + ":00000080"); } catch { weakThrew = true; }
  checks.push({ name: "F510 弱 KDF 拒绝", pass: weakThrew });
  // 序列化侧弱参数拒绝
  let serThrew = false;
  try { serializeHeader({ ...h, kdfIter: 100000 }); } catch { serThrew = true; }
  checks.push({ name: "F510 弱参数序列化拒绝", pass: serThrew });
  // 临时视图：开→落账→关→清零；一笔回删失败 → cleaning 且账本只留残余
  const lv = tempViewOpen(["t1.bin", "t2.bin", "t3.bin"]);
  const c1 = tempViewClose(lv, (p) => p !== "t2.bin");
  checks.push({ name: "F510 临时区清零账本", pass: c1.removed.length === 2 && c1.leftover.length === 1 && c1.ledger.phase === "cleaning" && c1.ledger.entries.length === 1 });
  const c2 = tempViewClose(c1.ledger, () => true);
  checks.push({ name: "F510 清扫幂等到 cleaned", pass: c2.ledger.phase === "cleaned" && c2.leftover.length === 0 });
  // 打包路径：规范化 + .. 逃逸拒绝 + 越根拒绝
  const e1 = normalizeArchiveEntry("C:/docs", "C:/docs\\a\\b\\c.txt");
  let dotdotThrew = false;
  let outThrew = false;
  try { normalizeArchiveEntry("C:/docs", "C:/docs/../../etc/passwd"); } catch { dotdotThrew = true; }
  try { normalizeArchiveEntry("C:/docs", "D:/elsewhere/x.txt"); } catch { outThrew = true; }
  checks.push({ name: "F510 打包路径规范化", pass: e1.rel === "a/b/c.txt" && dotdotThrew && outThrew });
  return checks;
}
