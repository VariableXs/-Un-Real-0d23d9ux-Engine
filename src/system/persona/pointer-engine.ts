/**
 * F156 指针引擎深化 · 帧播放器模型 + vxpointer 格式 + .cur/.ani 互通评估 +
 * 渲染规划（DPR 感知）。
 *
 * 主册判据延伸：
 * - 【设计细节】「动效指针帧序列上限 16 帧」「导入兼容 .cur/.ani 评估（Windows
 *   生态互通）」「sprite 生成含 1x/2x 双套（乙节基线）」。
 * - 渲染规划按设备像素比换算（4K 管线——高分屏放大不糊）。
 */

import { ANIM_MAX_FRAMES, POINTER_ROLES, type PointerScheme, type PointerRoleAsset } from "./pointer";

// ---------- 帧播放器模型（纯时间驱动——渲染层消费） ----------

export interface PlayheadFrame {
  frameIndex: number;
  /** 距该帧开始的毫秒（供插值）。 */
  frameElapsedMs: number;
  loop: number;
}

/** 动效播放头：时间 → 帧序号（fps 驱动，循环计数）。 */
export function playheadAt(fps: number, frameCount: number, elapsedMs: number): PlayheadFrame {
  if (frameCount <= 0) return { frameIndex: 0, frameElapsedMs: 0, loop: 0 };
  const clampedFps = Math.min(60, Math.max(1, fps || 24));
  const frameMs = 1000 / clampedFps;
  const total = Math.floor(elapsedMs / frameMs);
  return {
    frameIndex: total % frameCount,
    frameElapsedMs: elapsedMs - total * frameMs,
    loop: Math.floor(total / frameCount),
  };
}

/** 播放一轮完整动效的时长（ms）。 */
export function cycleMs(fps: number, frameCount: number): number {
  const clampedFps = Math.min(60, Math.max(1, fps || 24));
  return Math.round((frameCount * 1000) / clampedFps);
}

// ---------- 渲染规划（DPR 感知 · 4K 管线） ----------

export interface RenderPlan {
  role: string;
  /** CSS 像素尺寸。 */
  cssSize: number;
  /** 物理像素尺寸（cssSize × dpr，向上取偶——纹理对齐）。 */
  physicalSize: number;
  /** 实际使用的 sprite 倍率档（1x/2x 中选 ≥物理尺寸的最小者）。 */
  spriteScale: 1 | 2;
  /** 热点物理坐标。 */
  hotspotPhysical: { x: number; y: number };
}

/** 按 DPR 选 sprite 档：物理尺寸 ≤ 基准 → 1x；否则 2x（放大检查不糊）。 */
export function renderPlan(asset: PointerRoleAsset, schemeScale: number, dpr: number): RenderPlan {
  const cssSize = Math.round(asset.size * schemeScale);
  const physicalSize = Math.ceil((cssSize * dpr) / 2) * 2;
  const spriteScale = physicalSize > asset.size ? 2 : 1;
  const hs = asset.hotspot ?? { x: 0, y: 0 };
  return {
    role: asset.role,
    cssSize,
    physicalSize,
    spriteScale,
    hotspotPhysical: {
      x: Math.round(hs.x * schemeScale * (spriteScale === 2 ? 2 : 1)),
      y: Math.round(hs.y * schemeScale * (spriteScale === 2 ? 2 : 1)),
    },
  };
}

/** 全方案 15 枚渲染规划（含 DPR 变化重算入口——分辨率热切换 F224 联动）。 */
export function renderPlansForScheme(scheme: PointerScheme, dpr: number): Record<string, RenderPlan> {
  const out: Record<string, RenderPlan> = {};
  for (const role of POINTER_ROLES) {
    const asset = scheme.roles[role.id];
    if (asset && asset.frames.length > 0) out[role.id] = renderPlan(asset, scheme.scale, dpr);
  }
  return out;
}

// ---------- vxpointer 格式（导出/导入 + 校验） ----------

export const VXPOINTER_FORMAT = "vxpointer";
export const VXPOINTER_VERSION = 1;

export interface VxPointerFile {
  format: typeof VXPOINTER_FORMAT;
  version: 1;
  name: string;
  scale: number;
  roles: Record<string, { size: number; hotspot: { x: number; y: number } | null; frames: string[]; fps: number; animated: boolean }>;
  checksum?: string;
}

export interface VxPointerParseResult {
  ok: boolean;
  issues: string[];
  data: VxPointerFile | null;
}

const KNOWN_ROLES = new Set(POINTER_ROLES.map((r) => r.id));

/** FNV-1a（方案完整性——导入损坏检出）。 */
export function vxpointerChecksum(file: Omit<VxPointerFile, "checksum">): string {
  const s = JSON.stringify(file);
  let h = 0x811c9dc5;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 0x01000193);
  }
  return (h >>> 0).toString(16).padStart(8, "0");
}

export function serializeVxPointer(scheme: PointerScheme): VxPointerFile {
  const roles: VxPointerFile["roles"] = {};
  for (const r of POINTER_ROLES) {
    const a = scheme.roles[r.id];
    if (a) {
      roles[r.id] = {
        size: a.size,
        hotspot: a.hotspot ? { x: a.hotspot.x, y: a.hotspot.y } : null,
        frames: a.frames.slice(0, ANIM_MAX_FRAMES),
        fps: Math.min(60, Math.max(1, a.fps || (a.animated ? 24 : 0))),
        animated: a.animated,
      };
    }
  }
  const withoutChecksum: Omit<VxPointerFile, "checksum"> = {
    format: VXPOINTER_FORMAT,
    version: 1,
    name: scheme.name,
    scale: scheme.scale,
    roles,
  };
  return { ...withoutChecksum, checksum: vxpointerChecksum(withoutChecksum) };
}

export function parseVxPointer(raw: unknown): VxPointerParseResult {
  const issues: string[] = [];
  if (typeof raw !== "object" || raw === null) {
    return { ok: false, issues: ["不是 JSON 对象"], data: null };
  }
  const p = raw as Partial<VxPointerFile>;
  if (p.format !== VXPOINTER_FORMAT) issues.push(`format 须为 "${VXPOINTER_FORMAT}"`);
  if (p.version !== VXPOINTER_VERSION) issues.push(`version 须为 ${VXPOINTER_VERSION}`);
  if (typeof p.name !== "string" || p.name.length === 0 || p.name.length > 60) issues.push("name 须为 1..60 字符");
  if (typeof p.scale !== "number" || p.scale < 0.5 || p.scale > 2) issues.push("scale 须在 0.5-2");
  if (typeof p.roles !== "object" || p.roles === null) issues.push("roles 缺失");
  if (!p.checksum) issues.push("缺少完整性校验值");
  if (typeof p.name === "string" && typeof p.scale === "number" && p.roles) {
    const body: Omit<VxPointerFile, "checksum"> = { format: VXPOINTER_FORMAT, version: 1, name: p.name, scale: p.scale, roles: p.roles };
    if (p.checksum && vxpointerChecksum(body) !== p.checksum) {
      issues.push("完整性校验不匹配——方案已损坏");
    }
  }
  // 逐角色校验（未知角色 conflict 不阻断；帧数超限钳制提示）。
  if (p.roles) {
    for (const [role, asset] of Object.entries(p.roles)) {
      if (!KNOWN_ROLES.has(role)) {
        issues.push(`未知指针角色 ${role}（导入时忽略）`);
        continue;
      }
      if (asset.frames && asset.frames.length > ANIM_MAX_FRAMES) {
        issues.push(`${role}: 动效帧 ${asset.frames.length} 超上限 ${ANIM_MAX_FRAMES}（导入时裁剪）`);
      }
    }
  }
  const ok = !issues.some((i) => !i.includes("忽略") && !i.includes("裁剪"));
  return { ok, issues, data: ok ? (raw as VxPointerFile) : null };
}

// ---------- .cur/.ani 互通评估（评估件以报告为交付——F063 同法） ----------

export interface CurAniCompatibilityReport {
  format: ".cur" | ".ani";
  /** 静态/动效能力映射可行性。 */
  supported: boolean;
  limitations: string[];
}

/**
 * .cur：单帧静态位图 + 热点（与 vxpointer 静态角色一一映射）——可行。
 * .ani：RIFF 容器帧序列 + JIF 块（帧率在元数据）——可映射动效角色，
 * 但 Windows .ani 无 4K 双倍率、热点按帧可能漂移，列为限制项。
 */
export function evaluateCurAniCompatibility(kind: ".cur" | ".ani"): CurAniCompatibilityReport {
  if (kind === ".cur") {
    return {
      format: ".cur",
      supported: true,
      limitations: ["仅单帧静态位图（映射静态角色）", "位深按 32bpp ARGB 读取", "热点随文件自带，1px 精度"],
    };
  }
  return {
    format: ".ani",
    supported: true,
    limitations: [
      "帧率取自 RIFF 元数据（超 60fps 降采样至 60）",
      "无双倍率 sprite——4K 下按 2x 最近邻放大（诚实标注非原生）",
      "逐帧热点可能漂移——取首帧热点为准",
      "帧序列上限 16 帧（超出裁剪并提示）",
    ],
  };
}

/** RIFF 容器嗅探（.ani 魔数 RIFF....ACON）。 */
export function isAniContainer(buf: ArrayBuffer): boolean {
  if (buf.byteLength < 12) return false;
  const v = new DataView(buf);
  const riff = v.getUint32(0) === 0x52494646;
  const acon = v.getUint32(8) === 0x41434f4e;
  return riff && acon;
}

/** .cur 魔数嗅探（ICO 族：00 00 01 00 / CUR：00 00 02 00）。 */
export function isCurFile(buf: ArrayBuffer): boolean {
  if (buf.byteLength < 4) return false;
  const v = new DataView(buf);
  return v.getUint16(0) === 0 && v.getUint16(2) === 2;
}
