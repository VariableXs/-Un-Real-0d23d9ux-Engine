/**
 * AI-13 Z-61 增量更新通道（Update Channels & Delta）
 *
 * 诚实口径：本版实现「通道元数据 + 双版本回滚点记录 + 用户确认制」前端骨架；
 * 实际 diff 下载（<30% 目标）依赖 sysmaint::update_apply 既有通道，此处不做下载。
 */

export type UpdateChannel = "stable" | "beta";

export interface ChannelInfo {
  channel: UpdateChannel;
  /** 当前版本 */
  currentVersion: string;
  /** 回滚点（升级前记录的上一版本标记） */
  rollbackPoint: string | null;
  /** 是否已征得用户确认（用户确认制红线） */
  confirmed: boolean;
}

const ROLLBACK_KEY = "vxs:update-rollback";
let info: ChannelInfo | null = null;

export function initChannel(currentVersion: string, channel: UpdateChannel): ChannelInfo {
  info = { channel, currentVersion, rollbackPoint: loadRollback(), confirmed: false };
  return info;
}

export function channelInfo(): ChannelInfo {
  return info ?? { channel: "stable", currentVersion: "?", rollbackPoint: null, confirmed: false };
}

/** 切换通道（需用户确认后调用；切换本身不触发下载）。 */
export function setChannel(channel: UpdateChannel): ChannelInfo {
  info = { ...channelInfo(), channel, confirmed: false };
  return info;
}

/** 升级前登记回滚点（双版本回滚）。 */
export function markRollback(previousVersion: string): void {
  try {
    localStorage.setItem(ROLLBACK_KEY, JSON.stringify({ version: previousVersion, ts: Date.now() }));
    if (info) info.rollbackPoint = previousVersion;
  } catch { /* ignore */ }
}

function loadRollback(): string | null {
  try {
    const raw = localStorage.getItem(ROLLBACK_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as { version?: string };
    return typeof parsed.version === "string" ? parsed.version : null;
  } catch {
    return null;
  }
}

/** 通道清单描述（设置页呈现；stable=默认 / beta=先行体验，均本地无外发）。 */
export const CHANNEL_LABELS: Record<UpdateChannel, { zh: string; en: string }> = {
  stable: { zh: "稳定版（默认）", en: "Stable (default)" },
  beta: { zh: "先行体验（可能不稳定）", en: "Beta (may be unstable)" },
};

/** 增量包预算校验（工具函数，供 sysmaint 侧对照）：diff 大于目标 30% 判不合格。 */
export function deltaBudgetOk(diffBytes: number, fullBytes: number): boolean {
  return fullBytes > 0 && diffBytes / fullBytes < 0.3;
}

/** 测试辅助：读回滚点。 */
export function loadRollbackTestHelper(): string | null {
  return loadRollback();
}
