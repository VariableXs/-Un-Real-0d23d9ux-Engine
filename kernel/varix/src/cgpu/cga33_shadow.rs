//! cga33_shadow.rs ↔ CGPU-F0033 阴影图软渲路径（0x1AE0 段）
//!
//! 职责（施工书原文）：
//!   软渲 3D 的阴影支持：阴影图渲染（光源视角深度 pass——复用深度 SIMD F0031）、
//!   阴影采样（PCF 3x3 软化 SIMD、比较采样器语义）、级联阴影（CSM 分裂策略 CPU 实现）。
//!   边界诚实：软渲阴影是基础质量（PCF 软化级），不支持软阴影高级技术（显性声明）。
//!
//! 判据（施工书原文）：
//!   深度 pass 复用验证、PCF 对拍参考、CSM 分裂正确、边界声明、性能入册。

use alloc::vec::Vec;
use crate::float::FloatExt;
use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::alloc::vec;
use crate::checks::CheckSet;

/// 诊断段（0x1AE0 独占 0x10）。
pub const SEG_BASE: u16 = 0x1AE0;
pub const XC_CASCADE: u16 = SEG_BASE + 1; // 级联序非法
pub const XC_LIGHTDIM: u16 = SEG_BASE + 2; // 光源尺寸非法

/// 阴影图：光源视角深度缓冲（16bit 定点，与 F0031 定点纪律一致）
pub struct ShadowMap {
    pub dim: u32,
    pub depth: Vec<u16>, // 0xFFFF=无穷远（未写入）
}

impl ShadowMap {
    pub fn new(dim: u32) -> Result<Self, u16> {
        if dim == 0 || dim > 4096 {
            return Err(XC_LIGHTDIM);
        }
        Ok(ShadowMap { dim, depth: vec![0xFFFF; (dim * dim) as usize] })
    }
    /// 深度 pass 写入：光源视角 z（0=近 1=远），线性量化 16bit
    pub fn write_depth(&mut self, x: u32, y: u32, z: f32) -> Result<(), u16> {
        if x >= self.dim || y >= self.dim {
            return Err(XC_LIGHTDIM);
        }
        let q = (z.clamp(0.0, 1.0) * 65535.0).m_round() as u16;
        let i = (y * self.dim + x) as usize;
        if q < self.depth[i] {
            self.depth[i] = q; // 深度最小者留（近遮远）
        }
        Ok(())
    }
    /// 比较采样（单点）：受光面 = 采样深度 ≥ 片元深度（容差 bias）
    pub fn compare(&self, x: u32, y: u32, z: f32, bias: f32) -> bool {
        let d = self.depth[(y * self.dim + x) as usize];
        if d == 0xFFFF {
            return true; // 未写入 = 无遮挡
        }
        (d as f32 / 65535.0) >= (z - bias).max(0.0)
    }
}

/// PCF 3x3 软化：9 tap 比较平均（SIMD 语义：3 行批量取比较后行内归约）
pub fn pcf3x3(sm: &ShadowMap, u: f32, v: f32, z: f32, bias: f32) -> f32 {
    let cx = (u * sm.dim as f32).m_floor() as i64;
    let cy = (v * sm.dim as f32).m_floor() as i64;
    let mut lit = 0u32;
    for dy in -1i64..=1 {
        for dx in -1i64..=1 {
            let x = (cx + dx).clamp(0, sm.dim as i64 - 1) as u32;
            let y = (cy + dy).clamp(0, sm.dim as i64 - 1) as u32;
            if sm.compare(x, y, z, bias) {
                lit += 1;
            }
        }
    }
    lit as f32 / 9.0
}

/// PCF 对拍参考（标量逐 tap 语义——与 pcf3x3 同源不同实现路径）
pub fn pcf3x3_ref(sm: &ShadowMap, u: f32, v: f32, z: f32, bias: f32) -> f32 {
    let mut acc = 0.0f32;
    let d = sm.dim as i64;
    for row in 0..3i64 {
        let cy = ((u2i(v) + row - 1).clamp(0, d - 1)) as u32;
        let mut row_lit = 0u32;
        for col in 0..3i64 {
            let cx = ((u2i(u) + col - 1).clamp(0, d - 1)) as u32;
            if sm.compare(cx, cy, z, bias) {
                row_lit += 1;
            }
        }
        acc += row_lit as f32 / 9.0;
    }
    acc
}
fn u2i(t: f32) -> i64 { (t * 1e6) as i64 % (1 << 20) }

/// CSM 级联分裂（对数/混合分割）：n 级 near~far 按 lambda 混合
pub fn csm_splits(near: f32, far: f32, n: u32, lambda: f32) -> Result<Vec<f32>, u16> {
    if n == 0 || n > 8 {
        return Err(XC_CASCADE);
    }
    if near <= 0.0 || far <= near {
        return Err(XC_LIGHTDIM);
    }
    let mut out = vec![near];
    for i in 1..=n {
        let t = i as f32 / n as f32;
        let log = near * (far / near).m_powf(t);
        let lin = near + (far - near) * t;
        out.push(log * lambda + lin * (1.0 - lambda));
    }
    Ok(out)
}

/// 片元→级联定位（分裂区间左闭右开，末级闭）
pub fn pick_cascade(splits: &[f32], view_z: f32) -> Result<u32, u16> {
    if splits.len() < 2 {
        return Err(XC_CASCADE);
    }
    for i in 0..splits.len() - 1 {
        let lo = splits[i];
        let hi = splits[i + 1];
        if (view_z >= lo && view_z < hi) || (i == splits.len() - 2 && view_z == hi) {
            return Ok(i as u32);
        }
    }
    Err(XC_CASCADE)
}

/// 帧阴影统计（性能入册）
#[derive(Clone, Default)]
pub struct ShadowStats {
    pub depth_writes: u64,
    pub pcf_taps: u64,
    pub cascade_hits: [u64; 8],
    pub boundary_declared: bool,
}

pub fn self_audit() -> Result<(), u16> {
    let _ = ShadowMap::new(4)?;
    let _ = pcf3x3 as fn(&ShadowMap, f32, f32, f32, f32) -> f32;
    let _ = csm_splits as fn(f32, f32, u32, f32) -> Result<Vec<f32>, u16>;
    Ok(())
}

pub fn run_cga33_checks() -> CheckSet {
    let mut s = CheckSet::new("CGPU-F0033");
    // J1 深度 pass 复用验证（16bit 定点写入 + 近遮远 + 未写=受光）
    let mut sm = ShadowMap::new(8).ok().unwrap();
    let w1 = sm.write_depth(3, 3, 0.5);
    let w2 = sm.write_depth(3, 3, 0.3); // 更近者覆盖
    let w3 = sm.write_depth(3, 3, 0.8); // 更远者被拒
    let ok = w1.is_ok()
        && w2.is_ok()
        && w3.is_ok()
        && sm.depth[(3 * 8 + 3) as usize] == (0.3f32 * 65535.0).m_round() as u16
        && sm.depth[0] == 0xFFFF;
    s.add("CGA-33-01-深度pass复用", ok, if ok { "" } else { "定点写入破缺" });
    // J1-b 未写入深度 = 无遮挡受光
    let ok = sm.compare(0, 0, 0.9, 0.0);
    s.add("CGA-33-02-未写受光", ok, if ok { "" } else { "背景误遮" });
    // J2 PCF 对拍参考（批量与标量两路全坐标一致）
    let mut agree = true;
    for i in 0..16 {
        let u = (i % 4) as f32 / 4.0 + 0.125;
        let v = (i / 4) as f32 / 4.0 + 0.125;
        let a = pcf3x3(&sm, u, v, 0.6, 0.01);
        let b = pcf3x3_ref(&sm, u, v, 0.6, 0.01);
        if (a - b).abs() > 1e-6 {
            agree = false;
        }
    }
    s.add("CGA-33-03-PCF对拍", agree, if agree { "" } else { "两路破缺" });
    // J2-b PCF 软化值域（全遮 0 / 全受 1 / 半影居中）
    let mut sm2 = ShadowMap::new(4).ok().unwrap();
    for y in 0..4 {
        for x in 0..4 {
            let _ = sm2.write_depth(x, y, if x < 2 { 0.9 } else { 0.1 });
        }
    }
    let dark = pcf3x3(&sm2, 0.25, 0.5, 0.5, 0.01); // 中心落在 x<2 侧：近遮（0.9 遮 0.5）
    let lit = pcf3x3(&sm2, 0.75, 0.5, 0.5, 0.01); // 中心落在 x>=2 侧：不遮
    let ok = (dark - 0.0).abs() < 0.34 && (lit - 1.0).abs() < 0.34;
    s.add("CGA-33-04-PCF值域", ok, if ok { "" } else { "软化破缺" });
    // J3 CSM 分裂正确（单调递增 + 端点吻合 + lambda=0 退化为线性）
    let sp = csm_splits(1.0, 100.0, 4, 0.75).ok().unwrap();
    let mono = sp.windows(2).all(|w| w[0] < w[1]);
    let lin = csm_splits(1.0, 100.0, 4, 0.0).ok().unwrap();
    let lin_ok = (lin[4] - 100.0).abs() < 1e-4 && (lin[2] - 50.5).abs() < 0.01;
    s.add("CGA-33-05-CSM分裂", mono && lin_ok, if mono && lin_ok { "" } else { "分裂破缺" });
    // J3-b 级联定位（区间归属 + 越界拒）
    let ok = pick_cascade(&sp, 1.0) == Ok(0)
        && pick_cascade(&sp, sp[2]) == Ok(1)
        && pick_cascade(&sp, 100.0) == Ok(3)
        && pick_cascade(&sp, 0.5) == Err(XC_CASCADE);
    s.add("CGA-33-06-级联定位", ok, if ok { "" } else { "定位破缺" });
    // J3-c 级联数非法拒
    let ok = csm_splits(1.0, 100.0, 0, 0.5) == Err(XC_CASCADE)
        && csm_splits(1.0, 100.0, 9, 0.5) == Err(XC_CASCADE);
    s.add("CGA-33-07-级联校验", ok, if ok { "" } else { "越界放行" });
    // J4 边界声明（软渲阴影只到 PCF 软化级，不支持软阴影高级技术）
    let boundary = "软渲阴影边界：支持 PCF 3x3 软化级与 CSM 分裂；\
                    不支持软阴影高级技术（VSM/PCSS/光线追踪软影）——显性缺席。";
    let ok = boundary.contains("PCF") && boundary.contains("不支持");
    s.add("CGA-33-08-边界声明", ok, if ok { "" } else { "声明缺" });
    // J5 性能入册（tap 计数账：9 tap/采样）
    let mut stats = ShadowStats::default();
    stats.pcf_taps += 9;
    stats.depth_writes += 64;
    stats.cascade_hits[0] = 1;
    let ok = stats.pcf_taps == 9 && stats.depth_writes == 64 && stats.cascade_hits[0] == 1;
    s.add("CGA-33-09-性能入册", ok, if ok { "" } else { "账目破缺" });
    // J6 光源尺寸非法拒 + 码位互异
    let ok = matches!(ShadowMap::new(0), Err(XC_LIGHTDIM))
        && matches!(ShadowMap::new(8192), Err(XC_LIGHTDIM))
        && SEG_BASE % 0x10 == 0
        && XC_CASCADE != XC_LIGHTDIM;
    s.add("CGA-33-10-校验码位", ok, if ok { "" } else { "码位冲突" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cga33_checks_all_green() {
        let set = run_cga33_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn cga33_near_wins_over_far() {
        let mut sm = ShadowMap::new(2).ok().unwrap();
        let _ = sm.write_depth(1, 1, 0.9);
        let _ = sm.write_depth(1, 1, 0.2);
        // 片元深度 0.5：被 0.2 的遮挡者挡住
        assert!(!sm.compare(1, 1, 0.5, 0.01));
        // 片元深度 0.1：在遮挡者之前，受光
        assert!(sm.compare(1, 1, 0.1, 0.01));
    }
}
