import { askNetConsent, type NetConsentDecision } from "../components/Modal";
import { ipc, type NetPolicy } from "./ipc";

/**
 * 联网守门（批次0，规格 12.2.2）：
 * Variable 默认零联网。任何功能要发起网络请求前必须调用 requestNetConsent，
 * 用户明确同意后才会继续。"始终允许/始终拒绝"持久化到 <dataDir>/net_consent.json，
 * "仅此一次/拒绝"只在本次会话内生效，不落盘。
 * 本模块只做授权判定，自身不发起任何网络请求。
 */

export type NetDecision = NetConsentDecision;

/** 从目标（URL 或裸主机）提取主机名；无法解析时返回空串。 */
export function hostOf(target: string): string {
  try {
    const u = new URL(target);
    if (u.hostname) return u.hostname.toLowerCase();
  } catch {
    /* 非完整 URL → 按裸主机形式处理 */
  }
  const raw = (target.split("://").pop() ?? target).split(/[/?#:]/)[0] ?? "";
  return raw.trim().toLowerCase();
}

/**
 * 请求联网授权：
 * - 命中"始终允许" → 直接返回 "always"（不打扰）
 * - 命中"始终拒绝" → 直接返回 "deny"（不打扰）
 * - 从未决定 → 弹三键对话框（拒绝 / 仅此一次 / 始终允许）
 */
export async function requestNetConsent(target: string, purpose: string): Promise<NetDecision> {
  const host = hostOf(target);
  if (!host) return "deny";
  let saved: NetPolicy | null = null;
  try {
    saved = await ipc.netConsentCheck(host);
  } catch {
    saved = null; // 策略存储不可用 → 仍走弹窗询问，不静默放行
  }
  if (saved === "allow") return "always";
  if (saved === "deny") return "deny";
  const decision = await askNetConsent({ target, purpose });
  if (decision === "always") {
    await ipc.netConsentSet(host, "allow").catch(() => {});
  }
  return decision;
}
