/**
 * H4 域共享哈希底座（v4 深化批次四 · internal 底座层）：
 * FNV-1a 32 位——H4 域唯一的哈希实现（一处一事实，零冗余纪律）：
 * f372 时间线哈希链 / f351 快照完整性 / f362 分节账链 / f365 内容采样哈希 /
 * f370 确定性抖动 / f396 备份链完整性 全部委托本实现，各模块不得私抄第二份。
 */

/** FNV-1a 32 位哈希（十六进制 8 位定宽输出）。 */
export function fnv1a32(text: string): string {
  let h = 0x811c9dc5;
  for (let i = 0; i < text.length; i++) {
    h ^= text.charCodeAt(i);
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return h.toString(16).padStart(8, "0");
}

/** 规范化 JSON：键排序递归序列化——同一结构不同键序得同一序列化串（完整性校验的前提）。 */
export function canonicalJson(value: unknown): string {
  if (value === null || typeof value !== "object") return JSON.stringify(value) ?? "null";
  if (Array.isArray(value)) return `[${value.map(canonicalJson).join(",")}]`;
  const obj = value as Record<string, unknown>;
  return `{${Object.keys(obj)
    .sort()
    .map((k) => `${JSON.stringify(k)}:${canonicalJson(obj[k])}`)
    .join(",")}}`;
}

/** 结构校验和：对任意可序列化结构给出确定性指纹（快照/清单/账本的防篡改面）。 */
export function checksumOf(value: unknown): string {
  return fnv1a32(canonicalJson(value));
}
