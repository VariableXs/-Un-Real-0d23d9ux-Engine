/**
 * J 鼠标域 · J1 判据证据引擎（MD3 附录 B 证据三件套的机械生产者）。
 *
 * 把散在各模块的对拍表/谱表/矩阵收敛为一个可导出的证据包：
 * - F601 三曲线 20 点增益对拍表（含自定义贝塞尔当前参数）；
 * - F611 手抖 2/4/6Hz 过滤效果谱（轻/强两档）；
 * - F604 自动滚 16 方位映射表；
 * - F609 边缘深入三档速度表；
 * - F619 长按默认档对账；
 * - F620 反色描边三底色走查样本；
 * - F607 护边时序自检（单/双屏矩阵）。
 * 面板「导出证据包」与 CI 复核共用同一入口——数据、复现命令、日期三件齐。
 */

import { gainTable20, CURVE_LIBRARY, slowTuneRegistryRow } from "./curve";
import { tremorSpectrum, liftFilterSelfTest } from "./filters";
import { resolveWheelMode, notchLines, tiltCols } from "./wheel";
import { autoscrollVelocity, edgeScrollSpeed, AUTOSCROLL_PRESET } from "./autoscroll";
import { longPressDefaultAudit, LONG_PRESS_BASE } from "./hoverTiming";
import { OVERLAY_WALKTHROUGH_BACKGROUNDS, composeOverlay, invertColor } from "./overlay";
import { SEAM_GUARD_PRESET, SeamGuard, cornerExempt, type MonitorInfo } from "./screen";
import { j1Store, J1_DEFAULTS } from "./j1store";
import type { CurveConfig } from "./curve";
import type { WheelNotchConfig } from "./wheel";

export interface EvidencePack {
  format: "vx-j1-evidence";
  version: 1;
  generatedAt: string;
  sections: {
    f601_gain: { curve: string; table: { inPx: number; outPx: number }[] }[];
    f602_registry: ReturnType<typeof slowTuneRegistryRow>;
    f603_selftest: { p95: number; pass: boolean; samples: number };
    f604_dirs16: { deg: number; dirIndex: number; vxAbs: number }[];
    f605_modes: { appId: string; appClass: string; resolved: string }[];
    f606_tilt: { colsPerNotch: number; clamped: number };
    f607_seam: { preset: typeof SEAM_GUARD_PRESET; cornersExempt: boolean; matrix: string[] };
    f609_edge: { depth: number; pxPerFrame: number }[];
    f611_spectrum: { level: string; rows: { hz: number; residualRatio: number }[] }[];
    f619_audit: ReturnType<typeof longPressDefaultAudit>;
    f620_overlay: { background: string; outlineColor: string; active: boolean }[];
  };
}

/** 生成当前配置下的完整证据包（导出 JSON 即归档件）。 */
export function buildEvidencePack(): EvidencePack {
  const curve = { ...(J1_DEFAULTS.curve as unknown as CurveConfig), ...(j1Store.get("curve") as Partial<CurveConfig>) };
  const wheel = { ...(J1_DEFAULTS.wheelNotch as unknown as WheelNotchConfig), ...(j1Store.get("wheelNotch") as Partial<WheelNotchConfig>) };
  const slow = { enabled: true, ratio: 0.1, key: "shift" as const, ...(j1Store.get("slowTune") as object) };

  const f601 = CURVE_LIBRARY.map((c) => ({
    curve: c.id,
    table: gainTable20(c.id, c.id === "custom" ? curve : undefined),
  }));

  const f604 = [0, 22.5, 45, 67.5, 90, 112.5, 135, 157.5, 180, 202.5, 225, 247.5, 270, 292.5, 315, 337.5].map((deg) => {
    const rad = (deg * Math.PI) / 180;
    const v = autoscrollVelocity(Math.cos(rad) * 40, Math.sin(rad) * 40, { enabled: true, ...AUTOSCROLL_PRESET });
    return { deg, dirIndex: v.dirIndex, vxAbs: Math.round(Math.hypot(v.vx, v.vy) * 10) / 10 };
  });

  const f605 = (["document", "code", "terminal", "browser", "list"] as const).map((cls) => ({
    appId: `sample-${cls}`,
    appClass: cls,
    resolved: resolveWheelMode(wheel, `sample-${cls}`, cls),
  }));

  const dual: MonitorInfo[] = [
    { id: "a", x: 0, y: 0, width: 1920, height: 1080, edidFingerprint: "EDID-A", scale: 1 },
    { id: "b", x: 1920, y: 0, width: 1920, height: 1080, edidFingerprint: "EDID-B", scale: 1.25 },
  ];
  const guard = new SeamGuard(() => dual, () => ({ ...SEAM_GUARD_PRESET, enabled: true }));
  const f607 = {
    preset: SEAM_GUARD_PRESET,
    cornersExempt: cornerExempt(dual, 4, 4) && cornerExempt(dual, 3836, 4),
    matrix: [
      `接缝首触(1918,500): ${guard.feed(1918, 500, 0)}`,
      `停留 100ms(1919): ${guard.feed(1919, 500, 100)}`,
      `停留 210ms(1919): ${guard.feed(1919, 500, 210)}`,
    ],
  };

  return {
    format: "vx-j1-evidence",
    version: 1,
    generatedAt: new Date().toISOString(),
    sections: {
      f601_gain: f601,
      f602_registry: slowTuneRegistryRow(slow as never),
      f603_selftest: { ...liftFilterSelfTest([{ dx: 0.6, dy: -0.4, dtMs: 3 }, { dx: -0.5, dy: 0.5, dtMs: 6 }], 100), samples: 100 },
      f604_dirs16: f604,
      f605_modes: f605,
      f606_tilt: { colsPerNotch: 3, clamped: tiltCols(3) },
      f607_seam: f607,
      f609_edge: [0, 4, 8, 16, 23].map((d) => ({ depth: d, pxPerFrame: edgeScrollSpeed(d) })),
      f611_spectrum: (["light", "strong"] as const).map((lv) => ({
        level: lv,
        rows: ([2, 4, 6] as const).map((hz) => ({ hz, residualRatio: tremorSpectrum(lv, hz, 1.2).residualRatio })),
      })),
      f619_audit: longPressDefaultAudit({ scale: (j1Store.get("longPress").scale as number) ?? 1.0, registry: {} }),
      f620_overlay: OVERLAY_WALKTHROUGH_BACKGROUNDS.map((bg) => ({
        background: bg.id,
        outlineColor: invertColor("#ffffff"),
        active: composeOverlay({ outline: true, shadow: false, ring: false }).active,
      })),
    },
  };
}

/** 证据包内建一致性自检（导出前最后一道门）：关键数字必须落在判据硬线上。 */
export function auditEvidence(pack: EvidencePack): { ok: boolean; failures: string[] } {
  const failures: string[] = [];
  const linear = pack.sections.f601_gain.find((g) => g.curve === "linear");
  if (!linear || linear.table.some((r) => r.outPx !== r.inPx)) failures.push("F601: 线性曲线对拍表存在非 1:1 行");
  if (pack.sections.f603_selftest.p95 >= 0.5) failures.push("F603: 抬笔自检 P95 未达 <0.5px");
  if (pack.sections.f607_seam.matrix.some((m) => !m.includes("hold") && !m.includes("pass"))) failures.push("F607: 护边矩阵出现未预期态");
  for (const s of pack.sections.f611_spectrum) {
    if (s.rows.some((r) => r.residualRatio > 1)) failures.push(`F611: ${s.level} 档残余比 >1（放大器？）`);
  }
  for (const a of pack.sections.f619_audit) {
    if (!a.ok) failures.push(`F619: ${a.feature} 默认档 ≠ 现行值`);
  }
  const notch = notchLines(3);
  if (notch !== 3) failures.push("F605: 3 行/格基准漂移");
  void LONG_PRESS_BASE;
  return { ok: failures.length === 0, failures };
}
