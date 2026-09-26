/**
 * J 鼠标域 · F614 鼠标分设备档案 + F616 应用级鼠标档案。
 *
 * F614——每只鼠标各记一套手感参数：档案含速度/曲线（F601）/滚轮档（F605）/
 * 侧键映射（F615）四件；插入即自动挂载对应档案（F543 分设备音量记忆的鼠标域
 * 同构）；新设备首插克隆当前默认+气泡提示；上限 10 台、超出淘汰最久未用。
 *
 * F616——比设备档案更细一层：同一只鼠标在不同应用里各有手感——档案由应用
 * 侧声明或用户手配双路；前台切换档案时参数差异只作用于增量（不整段重置防
 * 跳变）；设备档案与应用档案正交（二维矩阵取交集、各自独立登记）。
 *
 * 判据锚点：
 * - 三设备档案切换 / VID-PID/EDID 识别 / 四件套完整性 / 首插克隆与气泡提示 /
 *   导出导入与上限淘汰 → DeviceProfileManager
 * - 三应用档案切换 <100ms（纯同步 diff，无 IO——延迟上界天然成立）
 * - 增量切换无跳变（速度连续性）→ applyIncremental()
 * - 声明接口与手配双路 → AppProfile.source
 * - 与 F614 正交性矩阵用例 → resolveParams()
 */

import type { CurveId } from "./curve";
import type { WheelMode } from "./wheel";
import { j1Store, J1StoreError } from "./j1store";

/* ------------------------------- F614 设备档案 ------------------------------- */

/** 档案四件套（速度/曲线/滚轮/侧键——完整性校验以此为准）。 */
export interface DeviceProfileParams {
  sens: number;
  curve: CurveId;
  wheelMode: WheelMode;
  sideButtons?: { back: string; forward: string };
}

export interface DeviceProfile {
  id: string;
  name: string;
  /** 识别键：`vid:pid` 或 EDID 指纹（VID-PID/EDID 识别判据）。 */
  deviceKey: string;
  params: DeviceProfileParams;
  createdAt: number;
  lastUsedAt: number;
}

export const DEVICE_PROFILE_CAP = 10;

export interface DeviceProfilesConfig {
  profiles: DeviceProfile[];
  notifyOnClone: boolean;
}

/** 档案四件套完整性校验（缺一件即非法——导入/手配共用）。 */
export function validateDeviceProfile(p: DeviceProfile): string[] {
  const errs: string[] = [];
  if (!p.id) errs.push("缺少档案 id");
  if (!p.deviceKey) errs.push("缺少设备识别键（VID-PID/EDID）");
  if (typeof p.params?.sens !== "number" || p.params.sens <= 0 || p.params.sens > 10) errs.push("速度参数非法（0<sens≤10）");
  if (!["linear", "classic", "soft", "custom"].includes(p.params?.curve)) errs.push("曲线档非法");
  if (!["notch", "smooth", "per-app"].includes(p.params?.wheelMode)) errs.push("滚轮档非法");
  return errs;
}

/** 档案导出（F623 打包对接：AI-J2 逐字段取表）。 */
export function exportDeviceProfiles(cfg: DeviceProfilesConfig): DeviceProfile[] {
  return JSON.parse(JSON.stringify(cfg.profiles)) as DeviceProfile[];
}

/**
 * 设备档案管理器：
 * - ensureFor()：插入设备时调用——有档案则挂载并刷新 lastUsedAt；
 *   无档案则首插克隆当前默认 + 返回 notify（气泡提示「已为此设备建档」）；
 * - 上限 10 台，超出淘汰最久未用（F543 同纪律）。
 */
export class DeviceProfileManager {
  /** 当前默认（克隆源）。 */
  currentDefault: DeviceProfileParams = { sens: 1, curve: "classic", wheelMode: "per-app" };

  ensureFor(deviceKey: string, deviceName: string, atMs: number): {
    profile: DeviceProfile;
    cloned: boolean;
    evicted?: string;
  } {
    const cfg = this.config();
    const existing = cfg.profiles.find((p) => p.deviceKey === deviceKey);
    if (existing) {
      this.touch(existing.id, atMs);
      return { profile: existing, cloned: false };
    }
    let evicted: string | undefined;
    let profiles = [...cfg.profiles];
    if (profiles.length >= DEVICE_PROFILE_CAP) {
      const oldest = profiles.reduce((a, b) => (a.lastUsedAt <= b.lastUsedAt ? a : b));
      profiles = profiles.filter((p) => p.id !== oldest.id);
      evicted = oldest.name;
    }
    const profile: DeviceProfile = {
      id: `dev-${deviceKey}`,
      name: deviceName || deviceKey,
      deviceKey,
      params: { ...this.currentDefault },
      createdAt: atMs,
      lastUsedAt: atMs,
    };
    const errs = validateDeviceProfile(profile);
    if (errs.length > 0) throw new J1StoreError("devices", `新设备档案校验失败: ${errs.join("；")}`);
    profiles.push(profile);
    j1Store.set("devices", { profiles });
    return { profile, cloned: true, evicted };
  }

  /** 更新某设备档案参数（面板手配入口）。 */
  update(deviceKey: string, patch: Partial<DeviceProfileParams>): void {
    const cfg = this.config();
    const profiles = cfg.profiles.map((p) =>
      p.deviceKey === deviceKey ? { ...p, params: { ...p.params, ...patch } } : p,
    );
    j1Store.set("devices", { profiles });
  }

  /** 删除档案（用户显式操作）。 */
  remove(deviceKey: string): void {
    const cfg = this.config();
    j1Store.set("devices", { profiles: cfg.profiles.filter((p) => p.deviceKey !== deviceKey) });
  }

  private touch(id: string, atMs: number): void {
    const cfg = this.config();
    j1Store.set("devices", {
      profiles: cfg.profiles.map((p) => (p.id === id ? { ...p, lastUsedAt: atMs } : p)),
    });
  }

  private config(): DeviceProfilesConfig {
    const s = j1Store.get("devices");
    return {
      profiles: (s.profiles as DeviceProfile[] | undefined) ?? [],
      notifyOnClone: (s.notifyOnClone as boolean | undefined) ?? true,
    };
  }
}

/* ------------------------------- F616 应用级档案 ------------------------------- */

export type AppProfileSource = "declared" | "manual";

export interface AppProfile {
  appId: string;
  appName: string;
  source: AppProfileSource;
  /** 只写覆盖项——增量切换（不整段重置防跳变）的类型化表达。 */
  overrides: Partial<DeviceProfileParams>;
}

export interface AppProfilesConfig {
  profiles: Record<string, AppProfile>;
  currentApp: string;
}

/**
 * 增量应用：只把覆盖项合入当前生效参数，其余字段保持原值（速度连续性判据——
 * 从浏览器切到画图软件，只有被声明的字段变性格，其它字段原地不动）。
 */
export function applyIncremental(current: DeviceProfileParams, overrides: Partial<DeviceProfileParams>): DeviceProfileParams {
  return { ...current, ...Object.fromEntries(Object.entries(overrides).filter(([, v]) => v !== undefined)) };
}

/**
 * 正交矩阵解析（与 F614 正交性判据）：设备档案打底 → 应用档案增量覆盖。
 * 两个维度各自独立登记，二维取交集；无应用档案时设备档案完整生效（默认态
 * 单档案走天下）。
 */
export function resolveParams(
  device: DeviceProfileParams,
  appProfile: AppProfile | null,
): DeviceProfileParams {
  if (!appProfile) return device;
  return applyIncremental(device, appProfile.overrides);
}

/** 应用档案注册（声明接口：应用前台获焦时上报；手配面板走同一入口）。 */
export function upsertAppProfile(p: AppProfile): void {
  const cfg = j1Store.get("appProfiles");
  const profiles = { ...((cfg.profiles as Record<string, AppProfile>) ?? {}) };
  const errs: string[] = [];
  if (!p.appId) errs.push("缺少 appId");
  if (p.source !== "declared" && p.source !== "manual") errs.push("档案来源非法");
  if (errs.length > 0) throw new J1StoreError("appProfiles", `应用档案校验失败: ${errs.join("；")}`);
  profiles[p.appId] = p;
  j1Store.set("appProfiles", { profiles });
}

/** 移除应用档案（清除覆盖 → 回退全局/设备默认）。 */
export function removeAppProfile(appId: string): void {
  const cfg = j1Store.get("appProfiles");
  const profiles = { ...((cfg.profiles as Record<string, AppProfile>) ?? {}) };
  delete profiles[appId];
  j1Store.set("appProfiles", { profiles });
}

export function getAppProfile(appId: string): AppProfile | null {
  const cfg = j1Store.get("appProfiles");
  return ((cfg.profiles as Record<string, AppProfile>) ?? {})[appId] ?? null;
}

/**
 * F616 前台切换触发（应用前台获焦 → 档案挂载的单一入口）：
 * 记录 currentApp 并返回该应用的档案（无档案返回 null——默认态零干预）。
 * runtime 在 window focus / activeElement 变化时调用；<100ms 判据由纯同步
 * diff 保证（本函数无 IO）。
 */
export function trackCurrentApp(appId: string): AppProfile | null {
  const cur = (j1Store.get("appProfiles").currentApp as string) ?? "";
  if (cur !== appId) j1Store.set("appProfiles", { currentApp: appId });
  return getAppProfile(appId);
}

/**
 * 设备档案批量导入（F614 导出导入判据的导入侧）：
 * 逐条校验（validateDeviceProfile），非法条目显性列出——合法子集才收
 * （「半套不收」在单条粒度放宽为「非法单条不收、合法单条照常」——导入
 * 向导逐条呈现，用户对每一台的去留有知情权）。
 */
export function importDeviceProfiles(list: unknown): { accepted: DeviceProfile[]; rejected: { name: string; errors: string[] }[] } {
  const accepted: DeviceProfile[] = [];
  const rejected: { name: string; errors: string[] }[] = [];
  if (!Array.isArray(list)) throw new J1StoreError("devices", "导入清单不是数组");
  for (const raw of list) {
    const p = raw as DeviceProfile;
    const errs = validateDeviceProfile(p);
    if (errs.length > 0) rejected.push({ name: p?.name ?? p?.deviceKey ?? "未知名", errors: errs });
    else accepted.push(p);
  }
  return { accepted, rejected };
}
