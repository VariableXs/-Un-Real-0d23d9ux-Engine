/**
 * J 鼠标域 · F607/F613 纵深 · EDID 字节通道消费（批次七）。
 *
 * edid.ts（v5）把 128 字节解析成 EdidInfo。本引擎把解析产物变成
 * 「显示器能力档案」——F607 屏对面板与 F613 落点记忆的真实受益者：
 *
 * 1. 能力档案——从首选时序读物理分辨率、从尺寸描述读物理宽高，算出
 *    PPI 与点距。跨屏手感参数（F607 缝速、F613 记忆点钳制）都该以
 *    物理口径对齐，而不是逻辑口径——两台 27" 4K 与 27" 1080p 在
 *    逻辑坐标系里差 4 倍，在物理坐标系里是同一只手。
 *
 * 2. 身份置信度——三档身份来源：EDID 全指纹（checksum 过）> 序列号
 *    描述符 > 型号名。同型号双显示器（办公室常态）只有序列号能区分；
 *    F613 记忆点错屏 = 灾难（指针出现在错误的屏上），所以身份匹配
 *    必须显式报告置信度，低置信度时落点记忆拒绝恢复（宁可丢记忆，
 *    不可丢确定性——章十二红线）。
 *
 * 3. 物理口径钳制——记忆点恢复时以物理尺寸比例换算（分辨率变化后
 *    记忆点仍在「同一物理位置」，而不是同一逻辑坐标）。
 *
 * 判据锚点：
 * - PPI/点距 → displayCapability()
 * - 三档置信度与低置信拒绝 → identityConfidence() / mayRestore()
 * - 物理口径记忆点恢复 → physicalRestorePoint()
 */

import type { EdidInfo } from "./edid";

/* ------------------------------- 能力档案 ------------------------------- */

export interface DisplayCapability {
  /** 首选时序分辨率（EDID 权威口径——非当前模式）。 */
  nativeW: number;
  nativeH: number;
  /** 物理宽高（cm；EDID 1.4 不含此字段时由宿主链路补报，缺失为 null）。 */
  physWcm: number | null;
  physHcm: number | null;
  /** 对角英寸（尺寸缺失时 null）。 */
  diagonalIn: number | null;
  /** PPI（物理口径；尺寸缺失时 null——显式缺失不猜）。 */
  ppi: number | null;
  /** 点距 mm（PPI 的倒数口径；null 同上）。 */
  dotPitchMm: number | null;
}

/**
 * 显示器能力档案推导。物理尺寸不走 EDID 1.4 基本块（标准里就没有），
 * 由宿主 WMI/显示器描述符链路补报——缺失时 PPI/点距如实为 null，
 * 下游按「无物理口径」路径运行（F607 缝速不归一、F613 按逻辑口径钳制）。
 */
export function displayCapability(e: EdidInfo, dims?: { wCm: number; hCm: number } | null): DisplayCapability {
  const w = e.preferred?.hActive ?? 0;
  const h = e.preferred?.vActive ?? 0;
  const wc = dims?.wCm ?? null;
  const hc = dims?.hCm ?? null;
  const diag = wc && hc && wc > 0 && hc > 0 ? Math.hypot(wc, hc) / 2.54 : null;
  let ppi: number | null = null;
  if (diag && w > 0 && h > 0) {
    ppi = Math.round((Math.hypot(w, h) / diag) * 10) / 10;
  }
  return {
    nativeW: w,
    nativeH: h,
    physWcm: wc,
    physHcm: hc,
    diagonalIn: diag ? Math.round(diag * 10) / 10 : null,
    ppi,
    dotPitchMm: ppi ? Math.round((25.4 / ppi) * 1000) / 1000 : null,
  };
}

/* ------------------------------- 身份置信度 ------------------------------- */

export type Confidence = "full" | "serial" | "name";

export interface IdentityMatch {
  confidence: Confidence;
  /** 人话描述（面板直接展示）。 */
  describe: string;
}

/**
 * 三档置信度判定：
 * - full：checksum 通过 → 全指纹可信（唯一物理件级）；
 * - serial：checksum 失败但有序列号字符串 → 同型号可区分；
 * - name：只有型号名 → 同型号双屏不可区分（低置信）。
 */
export function identityConfidence(e: EdidInfo): IdentityMatch {
  if (e.checksumOk) return { confidence: "full", describe: `EDID 全指纹（校验和通过 · ${e.manufacturer} ${e.productCode} · SN ${e.serial}）` };
  if (e.serial > 0) return { confidence: "serial", describe: `序列号匹配（校验和未过 · SN ${e.serial}）` };
  return { confidence: "name", describe: `仅型号名（${e.manufacturer}）——同型号多屏不可区分` };
}

/**
 * F613 落点恢复授权：低置信（name）身份的记忆点**禁止恢复**。
 * 返回拒绝理由（三要素口径：发生了什么/为什么/怎么办）。
 */
export function mayRestore(e: EdidInfo): { allowed: boolean; reason?: string } {
  const c = identityConfidence(e).confidence;
  if (c === "name") {
    return {
      allowed: false,
      reason: "这台显示器只报了型号名、没有可靠序列号——恢复记忆点有落到另一块同型号屏上的风险。已跳过恢复；接入 EDID 完整读取后自动恢复此功能。",
    };
  }
  return { allowed: true };
}

/* ------------------------------- 物理口径记忆点 ------------------------------- */

/**
 * 物理口径恢复：记忆点存于旧分辨率，显示器现在换了新分辨率。
 * 按「物理位置不变」换算：x 物理比例 = 旧 x / 旧宽 → 新 x = 比例 × 新宽。
 * 物理尺寸（cm）参与约束：宽高比变化（旋转屏！）时按两轴独立比例。
 */
export function physicalRestorePoint(
  point: { x: number; y: number },
  from: { w: number; h: number },
  to: { w: number; h: number },
): { x: number; y: number } {
  if (from.w <= 0 || from.h <= 0) return { ...point };
  const rx = point.x / from.w;
  const ry = point.y / from.h;
  return {
    x: Math.round(rx * to.w),
    y: Math.round(ry * to.h),
  };
}

/**
 * 双屏 PPI 差对跨屏手感的预告：PPI 差 >40% 时，F607 缝速应按物理口径
 * 归一（否则跨缝瞬间视觉速度跳变）。返回归一系数（相对 A 屏）。
 */
export function physicalSpeedNorm(ppiA: number | null, ppiB: number | null): number {
  if (!ppiA || !ppiB) return 1; // 无物理口径不归一（显式缺失）
  return Math.round((ppiB / ppiA) * 100) / 100;
}
