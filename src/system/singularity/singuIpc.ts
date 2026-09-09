/**
 * SINGULARITY-100 · Rust 命令的懒加载 invoke 封装。
 *
 * 与 lib/ipc.ts 同款纪律：动态 import @tauri-apps/api/core，vitest 纯逻辑
 * 测试永不加载 Tauri 运行时；任何失败如实返回 null / 空数组（不编造数据）。
 * 字段与 src-tauri/src/shell/singularity.rs 的 Serialize 结构严格一致。
 */

import type { SinguPulseLike } from "./shared";

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T | null> {
  try {
    const mod = await import("@tauri-apps/api/core");
    return await mod.invoke<T>(cmd, args);
  } catch {
    return null; // 非 Tauri 环境 / 命令缺失：诚实降级
  }
}

// ---- singu_pulse（Q-08/23/51/52/53/56/57/98）----
export const singuPulse = () => invoke<SinguPulseLike>("singu_pulse");

// ---- singu_temp_scan / singu_temp_clear（Q-37/95 临时文件账本与清理）----
export interface SinguTempEntry {
  path: string;
  kind: string; // temp | cache | logs | data
  bytes: number;
  files: number;
}
export const singuTempScan = () => invoke<SinguTempEntry[]>("singu_temp_scan");
export const singuTempClear = (kinds: string[]) => invoke<number>("singu_temp_clear", { kinds });

// ---- singu_zone_check（Q-67 下载检疫；Zone.Identifier 只在 NTFS 存在）----
export interface SinguZone {
  from_internet: boolean;
  zone_id: number | null;
  readable: boolean;
}
export const singuZoneCheck = (path: string) => invoke<SinguZone>("singu_zone_check", { path });

// ---- singu_journal_log/list/clear（Q-64 访问日志）----
export interface SinguJournalEntry {
  ts_ms: number;
  dir: string;
  action: string;
  actor: string;
}
export const singuJournalLog = (dir: string, actor: string, action: string) =>
  invoke<null>("singu_journal_log", { dir, actor, action });
export const singuJournalList = () => invoke<SinguJournalEntry[]>("singu_journal_list");
export const singuJournalClear = () => invoke<null>("singu_journal_clear");

// ---- singu_batch_attrs（Q-38 批量属性）----
export interface SinguBatchResult {
  ok: number;
  failed: Array<{ path: string; error: string }>;
}
export const singuBatchAttrs = (
  paths: string[],
  patch: { readonly?: boolean; hidden?: boolean; archive?: boolean; time_shift_days?: number },
) => invoke<SinguBatchResult>("singu_batch_attrs", { paths, patch });

// ---- singu_archive_check（Q-42 存档快检：zip 逐条目 CRC）----
export interface SinguArchiveReport {
  total: number;
  ok: number;
  failed: string[];
  readable: boolean;
}
export const singuArchiveCheck = (path: string) => invoke<SinguArchiveReport>("singu_archive_check", { path });

// ---- singu_data_profile（Q-70 冰山 / Q-95 配额：数据分区构成）----
export interface SinguDataZone {
  zone: string; // database | cache | logs | temp | media | workspace
  bytes: number;
}
export const singuDataProfile = () => invoke<SinguDataZone[]>("singu_data_profile");
