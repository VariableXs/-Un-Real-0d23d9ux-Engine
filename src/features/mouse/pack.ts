/**
 * J 鼠标域 · F614/F616 档案打包导出导入（F623 vxtheme 打包的对接面）。
 *
 * AI-J2 的 F623「鼠标档案入 vxtheme」四件打包以本模块的 pack/unpack 为
 * 单一事实源：设备档案（F614）+ 应用档案（F616）+ J1 全配置快照打成一包；
 * 合法性校验三例判据（超速/超量/非法轨迹各一）在 validatePack 内逐条执法。
 * 中断原子性：先全部校验，再统一切换——任一非法整包拒绝（不留半套）。
 */

import { j1Store, J1_FORMAT, J1_VERSION, type J1Config, type J1Section } from "./j1store";
import { validateDeviceProfile, type DeviceProfile } from "./profiles";

export const MOUSE_PACK_FORMAT = "vx-mouse-pack";
export const MOUSE_PACK_VERSION = 1;

export interface MousePack {
  format: typeof MOUSE_PACK_FORMAT;
  version: typeof MOUSE_PACK_VERSION;
  /** 打包时间戳（迁移对账用）。 */
  createdAt: number;
  /** J1 全配置分节快照（20 节）。 */
  config: J1Config;
  /** 设备档案（冗余存一份以便跨版本迁移校验）。 */
  devices: DeviceProfile[];
}

export interface PackValidation {
  ok: boolean;
  errors: string[];
  /** 降级清单（可修但需用户知情——如实呈现不静默）。 */
  warnings: string[];
}

/**
 * 打包导出：J1 全配置 + 设备档案一包带走。
 */
export function exportPack(): MousePack {
  const config = j1Store.exportAll();
  const devices = (j1Store.get("devices").profiles as DeviceProfile[] | undefined) ?? [];
  return {
    format: MOUSE_PACK_FORMAT,
    version: MOUSE_PACK_VERSION,
    createdAt: Date.now(),
    config,
    devices: JSON.parse(JSON.stringify(devices)) as DeviceProfile[],
  };
}

/**
 * 打包校验（三例判据的执法点）：
 * - 超速：sens > 10 → 拒绝（该条档案非法）；
 * - 超量：设备档案 > 上限 10 → 警告降级（保留最近 10 台，淘汰最久未用）；
 * - 非法轨迹：自定义手势 dirs 含 0..7 外值 → 该条拒绝。
 * 其余结构错误（格式头/版本/缺节）一律拒绝。
 */
export function validatePack(pack: unknown): PackValidation {
  const errors: string[] = [];
  const warnings: string[] = [];
  const p = pack as MousePack | null;
  if (!p || typeof p !== "object") return { ok: false, errors: ["包不是对象"], warnings };
  if (p.format !== MOUSE_PACK_FORMAT) errors.push(`格式头不匹配（期望 ${MOUSE_PACK_FORMAT}）`);
  if (p.version !== MOUSE_PACK_VERSION) errors.push(`版本不匹配（期望 ${MOUSE_PACK_VERSION}）`);
  if (!p.config || typeof p.config !== "object") errors.push("缺少 config 分节");
  if (Array.isArray(p?.devices)) {
    for (const d of p.devices) {
      const errs = validateDeviceProfile(d);
      if (errs.length > 0) errors.push(`设备档案「${d?.name ?? d?.deviceKey ?? "?"}」: ${errs.join("；")}`);
    }
    if (p.devices.length > 10) {
      warnings.push(`设备档案 ${p.devices.length} 台超上限——导入时保留最近使用 10 台，其余淘汰（将列出名单）`);
    }
  } else {
    warnings.push("包内无设备档案清单（旧版包）——按仅配置导入");
  }
  // 非法轨迹：自定义手势 dirs 值域执法。
  const gestures = p.config?.gestures?.custom as Record<string, { dirs?: unknown }> | undefined;
  if (gestures) {
    for (const [id, g] of Object.entries(gestures)) {
      const dirs = g?.dirs;
      if (!Array.isArray(dirs) || dirs.length === 0 || !dirs.every((d) => typeof d === "number" && Number.isInteger(d) && d >= 0 && d <= 7)) {
        errors.push(`自定义手势「${id}」: 轨迹非法（需非空的 0..7 方向数组）`);
      }
    }
  }
  return { ok: errors.length === 0, errors, warnings };
}

/**
 * 打包导入：validatePack 全绿才落盘（中断原子性）；设备超量降级保留最近
 * 10 台（lastUsedAt 排序）并在返回值里如实列出淘汰名单。
 */
export function importPack(pack: unknown): { applied: boolean; evicted: string[] } {
  const v = validatePack(pack);
  if (!v.ok) {
    throw new Error(`[mouse-j1:F623] 打包校验失败，整包拒绝: ${v.errors.join("；")}`);
  }
  const p = pack as MousePack;
  let devices = p.devices ?? [];
  const evicted: string[] = [];
  if (devices.length > 10) {
    const sorted = [...devices].sort((a, b) => b.lastUsedAt - a.lastUsedAt);
    devices = sorted.slice(0, 10);
    for (const d of sorted.slice(10)) evicted.push(d.name);
  }
  // 先全部写入内存再统一 emit（j1Store.importAll 自带原子切换）。
  j1Store.importAll(p.config as Record<string, unknown>);
  j1Store.set("devices", { profiles: devices });
  return { applied: true, evicted };
}

/** 包摘要（分享/详情页一句话）：几台设备、几个自定义手势、打包时间。 */
export function packSummary(pack: MousePack): string {
  const gestures = Object.keys(pack.config.gestures?.custom ?? {}).length;
  return `${pack.devices.length} 台设备档案 · ${gestures} 条自定义手势 · ${new Date(pack.createdAt).toLocaleString()} 打包`;
}

/** J1 配置节名（导出完整性自检用）。 */
export function packSections(pack: MousePack): J1Section[] {
  void J1_FORMAT;
  void J1_VERSION;
  return Object.keys(pack.config) as J1Section[];
}
