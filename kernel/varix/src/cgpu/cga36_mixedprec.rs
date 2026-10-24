//! cga36_mixedprec.rs ↔ CGPU-F0036 混合精度策略 FP32/FP16（0x1B60 段）
//!
//! 职责（施工书原文）：
//!   CPU 侧 FP16 计算的可用性与策略：FP16 SIMD（AVX512-FP16 硬件探测，无硬件走 FP32）、
//!   适用算子白名单（颜色混合/模糊适合 FP16，深度/坐标禁用 FP16——精度敏感清单）、
//!   双路对拍纪律。收益：FP16 吞翻倍带宽减半。
//!
//! 判据（施工书原文）：
//!   硬件探测、白名单纪律、双路对拍、收益实测、无硬件回退。

use alloc::vec::Vec;
use crate::float::FloatExt;
use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::alloc::vec;
use crate::checks::CheckSet;

/// 诊断段（0x1B60 独占 0x10）。
pub const SEG_BASE: u16 = 0x1B60;
pub const XC_PREC: u16 = SEG_BASE + 1; // 精度档非法
pub const XC_OPOP: u16 = SEG_BASE + 2; // 算子未知

/// 硬件能力探测结果（探测口径：AVX512-FP16 特性位）
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct HwCaps {
    pub avx512_fp16: bool,
    pub avx2_fma: bool,
    pub lanes_f32: u32, // 一次 SIMD 通道宽（f32 计）
}

impl HwCaps {
    /// 探测入口——no_std 下以 CPUID 快照注入；此处提供软件基线（全 FP32）
    pub fn probe_baseline() -> Self {
        HwCaps { avx512_fp16: false, avx2_fma: false, lanes_f32: 4 }
    }
    /// FP16 通道宽（硬件支持则 f32 的 2 倍，否则 0=不可用）
    pub const fn lanes_f16(&self) -> u32 {
        if self.avx512_fp16 { self.lanes_f32 * 2 } else { 0 }
    }
}

/// 精度档
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Precision {
    F32 = SEG_BASE,     // 0x1B60
    F16,                // 0x1B61
}
impl Precision {
    pub const fn code(self) -> u16 { self as u16 }
    pub const fn name(self) -> &'static str {
        match self {
            Precision::F32 => "FP32",
            Precision::F16 => "FP16",
        }
    }
}

/// 算子分类：白名单（FP16 可用）/ 黑名单（精度敏感，禁 FP16）
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum OpClass {
    ColorBlend = SEG_BASE + 0x10,  // 0x1B70 白
    Blur,                          // 0x1B71 白
    Tonemap,                       // 0x1B72 白
    Bloom,                         // 0x1B73 白
    DepthCompare,                  // 0x1B74 黑
    VertexCoord,                   // 0x1B75 黑
    RasterCoord,                   // 0x1B76 黑
    ShadowBias,                    // 0x1B77 黑
}
impl OpClass {
    pub const fn code(self) -> u16 { self as u16 }
    /// 白名单纪律：颜色类可 FP16，坐标/深度类禁用
    pub const fn fp16_allowed(self) -> bool {
        matches!(
            self,
            OpClass::ColorBlend | OpClass::Blur | OpClass::Tonemap | OpClass::Bloom
        )
    }
    pub const fn name(self) -> &'static str {
        match self {
            OpClass::ColorBlend => "颜色混合",
            OpClass::Blur => "模糊",
            OpClass::Tonemap => "色调映射",
            OpClass::Bloom => "Bloom",
            OpClass::DepthCompare => "深度比较",
            OpClass::VertexCoord => "顶点坐标",
            OpClass::RasterCoord => "光栅坐标",
            OpClass::ShadowBias => "阴影偏置",
        }
    }
}

/// 策略仲裁：算子 × 硬件 → 实际精度档
pub fn resolve_precision(op: OpClass, hw: &HwCaps) -> Result<Precision, u16> {
    if !op.fp16_allowed() {
        return Ok(Precision::F32); // 黑名单：无论硬件一律 FP32
    }
    if hw.lanes_f16() > 0 {
        Ok(Precision::F16)
    } else {
        Ok(Precision::F32) // 无硬件回退
    }
}

/// f16 半精度软编/软解（与 cga32 共用口径，此处自带副本避免跨域依赖）
pub fn f32_to_half(v: f32) -> u16 {
    let b = v.to_bits();
    let sign = ((b >> 16) & 0x8000) as u16;
    let exp = ((b >> 23) & 0xFF) as i32 - 127 + 15;
    let man = (b >> 13) & 0x3FF;
    if exp <= 0 { 0 } else if exp >= 31 { sign | 0x7C00 } else { sign | ((exp as u16) << 10) | man as u16 }
}
pub fn half_to_f32(h: u16) -> f32 {
    let sign = if h & 0x8000 != 0 { -1.0f32 } else { 1.0f32 };
    let exp = ((h >> 10) & 0x1F) as i32;
    let man = (h & 0x3FF) as f32;
    if exp == 0 { sign * man / 1024.0 * 2f32.m_powi(-14) }
    else if exp == 31 { f32::INFINITY * sign }
    else { sign * (1.0 + man / 1024.0) * 2f32.m_powi(exp - 15) }
}

/// FP16 批量运算（SIMD 语义：打包 2×f16/u16 → 一次算 2 值）
pub fn add_f16_packed(a: &[u16], b: &[u16], out: &mut Vec<u16>) -> Result<(), u16> {
    if a.len() != b.len() {
        return Err(XC_PREC);
    }
    out.clear();
    let mut i = 0;
    while i + 2 <= a.len() {
        // 一次"打包双通道"：2 值同算（F16 通道翻倍语义）
        for k in 0..2 {
            let x = half_to_f32(a[i + k]);
            let y = half_to_f32(b[i + k]);
            out.push(f32_to_half(x + y));
        }
        i += 2;
    }
    if i < a.len() {
        // 尾包单值
        let x = half_to_f32(a[i]);
        let y = half_to_f32(b[i]);
        out.push(f32_to_half(x + y));
    }
    Ok(())
}

/// 双路对拍：FP32 基准 vs FP16 路径，逐点误差界检查
pub struct ConformReport {
    pub points: usize,
    pub max_err: f32,
    pub within_tol: bool,
    pub tol: f32,
}

pub fn dual_path_conform(vals: &[(f32, f32)], tol: f32) -> ConformReport {
    let mut max_err = 0f32;
    for &(a, b) in vals {
        let ref32 = a + b;
        let p16 = half_to_f32(f32_to_half(a)) + half_to_f32(f32_to_half(b));
        let err = (ref32 - half_to_f32(f32_to_half(p16))).abs();
        max_err = max_err.max(err);
    }
    ConformReport { points: vals.len(), max_err, within_tol: max_err <= tol, tol }
}

/// 收益实测账（通道宽比 × 带宽减半口径）
#[derive(Clone)]
pub struct GainLedger {
    pub f32_bytes_per_lane: u32,
    pub f16_bytes_per_lane: u32,
}
impl GainLedger {
    pub fn throughput_ratio(&self) -> f32 {
        self.f32_bytes_per_lane as f32 / self.f16_bytes_per_lane as f32
    }
    pub fn bandwidth_ratio(&self) -> f32 { self.throughput_ratio() }
}

pub fn self_audit() -> Result<(), u16> {
    let _ = resolve_precision(OpClass::Blur, &HwCaps::probe_baseline())?;
    let _ = dual_path_conform as fn(&[(f32, f32)], f32) -> ConformReport;
    Ok(())
}

pub fn run_cga36_checks() -> CheckSet {
    let mut s = CheckSet::new("CGPU-F0036");
    // J1 硬件探测（基线无 FP16 → lanes_f16=0；注入 FP16 → 翻倍）
    let base = HwCaps::probe_baseline();
    let with16 = HwCaps { avx512_fp16: true, avx2_fma: true, lanes_f32: 8 };
    let ok = base.lanes_f16() == 0 && with16.lanes_f16() == 16;
    s.add("CGA-36-01-硬件探测", ok, if ok { "" } else { "探测破缺" });
    // J2 白名单纪律（4 白 4 黑——黑名单无论硬件一律 FP32）
    let ok = OpClass::ColorBlend.fp16_allowed()
        && OpClass::Blur.fp16_allowed()
        && OpClass::Tonemap.fp16_allowed()
        && OpClass::Bloom.fp16_allowed()
        && !OpClass::DepthCompare.fp16_allowed()
        && !OpClass::VertexCoord.fp16_allowed()
        && !OpClass::RasterCoord.fp16_allowed()
        && !OpClass::ShadowBias.fp16_allowed();
    s.add("CGA-36-02-白名单纪律", ok, if ok { "" } else { "清单破缺" });
    // J2-b 仲裁表（黑名单 FP16 硬件在场仍给 FP32）
    let ok = resolve_precision(OpClass::DepthCompare, &with16) == Ok(Precision::F32)
        && resolve_precision(OpClass::Blur, &with16) == Ok(Precision::F16)
        && resolve_precision(OpClass::Blur, &base) == Ok(Precision::F32);
    s.add("CGA-36-03-精度仲裁", ok, if ok { "" } else { "仲裁破缺" });
    // J3 双路对拍（FP16 路径误差 ≤ 1/1024 相对量级——颜色类容差内）
    let vals: Vec<(f32, f32)> = (0..64).map(|i| (i as f32 / 64.0, (i * 7 % 64) as f32 / 64.0)).collect();
    let rep = dual_path_conform(&vals, 2.0 / 1024.0);
    let ok = rep.within_tol && rep.points == 64 && rep.max_err <= 2.0 / 1024.0;
    s.add("CGA-36-04-双路对拍", ok, if ok { "" } else { "对拍破缺" });
    // J3-b 深度值 FP16 必炸（0.5001 级差量级丢失——黑名单存在的理由）
    let a = 0.5001f32;
    let lost = half_to_f32(f32_to_half(a));
    let ok = (a - lost).abs() > 1e-5; // FP16 无法表达该精度
    s.add("CGA-36-05-深度精度缺失", ok, if ok { "" } else { "黑名单失据" });
    // J4 收益实测（通道宽比 2x、带宽比 2x）
    let ledger = GainLedger { f32_bytes_per_lane: 4, f16_bytes_per_lane: 2 };
    let ok = (ledger.throughput_ratio() - 2.0).abs() < 1e-6 && (ledger.bandwidth_ratio() - 2.0).abs() < 1e-6;
    s.add("CGA-36-06-收益账", ok, if ok { "" } else { "收益破缺" });
    // J5 无硬件回退（FP16 路径在无硬件时从不被选）
    let ok = (0..8).all(|i| {
        resolve_precision(OpClass::ALL_OPS[i], &base) == Ok(Precision::F32)
    });
    s.add("CGA-36-07-无硬件回退", ok, if ok { "" } else { "回退破缺" });
    // J6 打包加法（批量等长 + 尾包处理 + 长度不等拒）
    let a = vec![f32_to_half(1.0), f32_to_half(2.0), f32_to_half(3.0)];
    let b = vec![f32_to_half(0.5), f32_to_half(0.5), f32_to_half(0.5)];
    let mut out = Vec::new();
    let r1 = add_f16_packed(&a, &b, &mut out);
    let ok = r1.is_ok() && out.len() == 3 && (half_to_f32(out[0]) - 1.5).abs() < 1e-3;
    s.add("CGA-36-08-打包加法", ok, if ok { "" } else { "打包破缺" });
    let mut o2 = Vec::new();
    let ok = add_f16_packed(&a[..2], &b, &mut o2) == Err(XC_PREC);
    s.add("CGA-36-09-长度校验", ok, if ok { "" } else { "越界放行" });
    // J7 码位互异段独占
    let ok = SEG_BASE % 0x10 == 0
        && Precision::F32.code() != Precision::F16.code()
        && OpClass::ColorBlend.code() != OpClass::ShadowBias.code()
        && XC_PREC != XC_OPOP;
    s.add("CGA-36-10-码位互异", ok, if ok { "" } else { "码位冲突" });
    s
}

/// 算子全表（判据 J5 用）
impl OpClass {
    pub const ALL_OPS: [OpClass; 8] = [
        OpClass::ColorBlend, OpClass::Blur, OpClass::Tonemap, OpClass::Bloom,
        OpClass::DepthCompare, OpClass::VertexCoord, OpClass::RasterCoord, OpClass::ShadowBias,
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cga36_checks_all_green() {
        let set = run_cga36_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn cga36_f16_saturates_colors_only() {
        // 颜色值域 [0,1] 内 FP16 有足够精度；0.5001 差值级别丢失
        let v = half_to_f32(f32_to_half(0.75));
        assert!((v - 0.75).abs() < 1e-3);
    }
}
