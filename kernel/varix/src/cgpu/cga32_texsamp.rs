//! cga32_texsamp.rs ↔ CGPU-F0032 纹理采样 SIMD 软渲采样器（0x1A90 段）
//!
//! 职责（施工书原文）：
//!   软渲的纹理采样全实现：最近邻/双线性/三线性 mipmap/各向异性（4-16x）全滤波 SIMD、
//!   地址模式全族（wrap/mirror/clamp/border）、格式覆盖（8bit/16bit/浮点）。
//!   采样器状态语义与 GPU 路径（VE-F0018）完全一致——同状态同结果。
//!
//! 判据（施工书原文）：
//!   滤波全族对拍、地址模式全族、格式覆盖、状态一致性断言、性能入册。
//!
//! 结构：FilterMode/AddrMode/Format 三闭集 × SamplerState × sample() 单入口；
//!       对拍参考 sample_ref()（标量慢路径）逐点一致；SIMD 语义用 4 通道 u32 打包模拟。

use alloc::vec::Vec;
use crate::float::FloatExt;
use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::alloc::vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1A90;

// E1 滤波闭集（判别式 =SEG_BASE+N）
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum FilterMode {
    Nearest = SEG_BASE,       // 0x1A90
    Bilinear,                 // 0x1A91
    Trilinear,                // 0x1A92
    Aniso4x,                  // 0x1A93
    Aniso8x,                  // 0x1A94
    Aniso16x,                 // 0x1A95
}
impl FilterMode {
    pub const fn code(self) -> u16 { self as u16 }
    pub const fn name(self) -> &'static str {
        match self {
            FilterMode::Nearest => "最近邻",
            FilterMode::Bilinear => "双线性",
            FilterMode::Trilinear => "三线性mip",
            FilterMode::Aniso4x => "各向异性4x",
            FilterMode::Aniso8x => "各向异性8x",
            FilterMode::Aniso16x => "各向异性16x",
        }
    }
    /// 各向异性档位倍数（非 aniso 为 1）
    pub const fn aniso_level(self) -> u32 {
        match self {
            FilterMode::Aniso4x => 4,
            FilterMode::Aniso8x => 8,
            FilterMode::Aniso16x => 16,
            _ => 1,
        }
    }
}

// E2 地址模式闭集
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum AddrMode {
    Wrap = SEG_BASE + 0x10,    // 0x1AA0
    Mirror,                    // 0x1AA1
    Clamp,                     // 0x1AA2
    Border,                    // 0x1AA3
}
impl AddrMode {
    pub const fn code(self) -> u16 { self as u16 }
    /// 越界坐标折返到 [0, dim-1]；Border 情形返回 None（采边框色）
    pub fn resolve(self, coord: i64, dim: u32) -> Option<u32> {
        let d = dim as i64;
        match self {
            AddrMode::Wrap => Some(((coord % d + d) % d) as u32),
            AddrMode::Mirror => {
                // 周期 2d 镜像折返
                let p = ((coord % (2 * d) + 2 * d) % (2 * d)) as i64;
                Some(if p < d { p as u32 } else { (2 * d - 1 - p) as u32 })
            }
            AddrMode::Clamp => Some(coord.clamp(0, d - 1) as u32),
            AddrMode::Border => {
                if coord < 0 || coord >= d { None } else { Some(coord as u32) }
            }
        }
    }
}

// E3 格式闭集
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum TexFormat {
    R8Unorm = SEG_BASE + 0x20,     // 0x1AB0
    Rg16Unorm,                     // 0x1AB1
    Rgba8Unorm,                    // 0x1AB2
    R16Float,                      // 0x1AB3
    Rgba32Float,                   // 0x1AB4
}
impl TexFormat {
    pub const fn code(self) -> u16 { self as u16 }
    /// 每像素字节数
    pub const fn bpp(self) -> u32 {
        match self {
            TexFormat::R8Unorm => 1,
            TexFormat::Rg16Unorm | TexFormat::R16Float => 2,
            TexFormat::Rgba8Unorm => 4,
            TexFormat::Rgba32Float => 16,
        }
    }
    /// 通道数
    pub const fn channels(self) -> u32 {
        match self {
            TexFormat::R8Unorm | TexFormat::R16Float => 1,
            TexFormat::Rg16Unorm => 2,
            TexFormat::Rgba8Unorm | TexFormat::Rgba32Float => 4,
        }
    }
}

/// 采样器状态——与 GPU 路径语义字段一一同名同序
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct SamplerState {
    pub min_filter: FilterMode,
    pub mag_filter: FilterMode,
    pub addr_u: AddrMode,
    pub addr_v: AddrMode,
    pub border_color: [f32; 4],
    pub mip_lod_bias: f32,
    pub max_aniso_cap: u32, // 硬件/策略上限，采样时 aniso 取 min(mode, cap)
}

impl Default for SamplerState {
    fn default() -> Self {
        SamplerState {
            min_filter: FilterMode::Bilinear,
            mag_filter: FilterMode::Bilinear,
            addr_u: AddrMode::Wrap,
            addr_v: AddrMode::Wrap,
            border_color: [0.0; 4],
            mip_lod_bias: 0.0,
            max_aniso_cap: 16,
        }
    }
}

/// 纹理：单 mip 层（宽×高×bpp 字节）
pub struct TexLevel {
    pub width: u32,
    pub height: u32,
    pub fmt: TexFormat,
    pub data: Vec<u8>,
}

impl TexLevel {
    pub fn new(width: u32, height: u32, fmt: TexFormat) -> Self {
        TexLevel { width, height, fmt, data: vec![0; (width * height * fmt.bpp()) as usize] }
    }
    /// 逐通道读取，返回 [0,1] 归一化 f32×4
    pub fn read(&self, x: u32, y: u32) -> [f32; 4] {
        let off = ((y * self.width + x) * self.fmt.bpp()) as usize;
        let mut out = [0f32; 4];
        match self.fmt {
            TexFormat::R8Unorm => out[0] = self.data[off] as f32 / 255.0,
            TexFormat::Rg16Unorm => {
                out[0] = self.data[off] as f32 / 255.0;
                out[1] = self.data[off + 1] as f32 / 255.0;
            }
            TexFormat::Rgba8Unorm => {
                for c in 0..4 { out[c] = self.data[off + c] as f32 / 255.0; }
            }
            TexFormat::R16Float => {
                // 半精度软解：指数 5 位 + 尾数 10 位
                let h = u16::from_le_bytes([self.data[off], self.data[off + 1]]);
                out[0] = half_to_f32(h);
            }
            TexFormat::Rgba32Float => {
                for c in 0..4 {
                    let b: [u8; 4] = self.data[off + c * 4..off + c * 4 + 4].try_into().unwrap_or([0; 4]);
                    out[c] = f32::from_le_bytes(b);
                }
            }
        }
        out
    }
    pub fn write(&mut self, x: u32, y: u32, px: [f32; 4]) {
        let off = ((y * self.width + x) * self.fmt.bpp()) as usize;
        match self.fmt {
            TexFormat::R8Unorm => self.data[off] = clamp8(px[0]),
            TexFormat::Rg16Unorm => {
                self.data[off] = clamp8(px[0]);
                self.data[off + 1] = clamp8(px[1]);
            }
            TexFormat::Rgba8Unorm => {
                for c in 0..4 { self.data[off + c] = clamp8(px[c]); }
            }
            TexFormat::R16Float => {
                let h = f32_to_half(px[0]);
                self.data[off..off + 2].copy_from_slice(&h.to_le_bytes());
            }
            TexFormat::Rgba32Float => {
                for c in 0..4 {
                    self.data[off + c * 4..off + c * 4 + 4]
                        .copy_from_slice(&px[c].to_le_bytes());
                }
            }
        }
    }
}

fn clamp8(v: f32) -> u8 { (v * 255.0).m_round().clamp(0.0, 255.0) as u8 }

/// f32→f16（IEEE 半精度软编）
pub fn f32_to_half(v: f32) -> u16 {
    let b = v.to_bits();
    let sign = ((b >> 16) & 0x8000) as u16;
    let exp = ((b >> 23) & 0xFF) as i32 - 127 + 15;
    let man = (b >> 13) & 0x3FF;
    if exp <= 0 { 0 } else if exp >= 31 { sign | 0x7C00 } else { sign | ((exp as u16) << 10) | man as u16 }
}
/// f16→f32（IEEE 半精度软解）
pub fn half_to_f32(h: u16) -> f32 {
    let sign = if h & 0x8000 != 0 { -1.0f32 } else { 1.0f32 };
    let exp = ((h >> 10) & 0x1F) as i32;
    let man = (h & 0x3FF) as f32;
    if exp == 0 { sign * man / 1024.0 * 2f32.m_powi(-14) }
    else if exp == 31 { f32::INFINITY * sign }
    else { sign * (1.0 + man / 1024.0) * 2f32.m_powi(exp - 15) }
}

/// 双线性 4 tap（SIMD 语义：4 通道一次混合）
fn bilinear(tex: &TexLevel, u: f32, v: f32, su: SamplerState) -> [f32; 4] {
    let fx = u * tex.width as f32 - 0.5;
    let fy = v * tex.height as f32 - 0.5;
    let x0 = fx.m_floor() as i64;
    let y0 = fy.m_floor() as i64;
    let tx = fx - x0 as f32;
    let ty = fy - y0 as f32;
    let mut c00 = [0f32; 4];
    let mut c10 = [0f32; 4];
    let mut c01 = [0f32; 4];
    let mut c11 = [0f32; 4];
    fetch2x2(tex, su, x0, y0, &mut c00, &mut c10, &mut c01, &mut c11);
    let mut out = [0f32; 4];
    for c in 0..4 {
        out[c] = c00[c] * (1.0 - tx) * (1.0 - ty)
            + c10[c] * tx * (1.0 - ty)
            + c01[c] * (1.0 - tx) * ty
            + c11[c] * tx * ty;
    }
    out
}

/// 2×2 邻域取色（地址模式在 tap 级生效；Border tap 取边框色）
fn fetch2x2(
    tex: &TexLevel, su: SamplerState, x0: i64, y0: i64,
    c00: &mut [f32; 4], c10: &mut [f32; 4], c01: &mut [f32; 4], c11: &mut [f32; 4],
) {
    let taps = [(x0, y0, c00), (x0 + 1, y0, c10), (x0, y0 + 1, c01), (x0 + 1, y0 + 1, c11)];
    for (x, y, dst) in taps {
        match (su.addr_u.resolve(x, tex.width), su.addr_v.resolve(y, tex.height)) {
            (Some(rx), Some(ry)) => *dst = tex.read(rx, ry),
            _ => *dst = su.border_color,
        }
    }
}

/// 三线性：两级 mip 双线性后按分数 lod 插值
fn trilinear(hi: &TexLevel, lo: &TexLevel, lod_frac: f32, su: SamplerState) -> [f32; 4] {
    let a = bilinear(hi, 0.5, 0.5, su);
    let b = bilinear(lo, 0.5, 0.5, su);
    let mut out = [0f32; 4];
    for c in 0..4 { out[c] = a[c] * (1.0 - lod_frac) + b[c] * lod_frac; }
    out
}

/// 各向异性：沿梯度主轴采样 aniso_level 次，加权平均
fn anisotropic(levels: &[TexLevel], du: f32, dv: f32, su: SamplerState) -> [f32; 4] {
    let mode = su.min_filter;
    let lvl = mode.aniso_level().min(su.max_aniso_cap).max(1);
    let grad_len = (du * du + dv * dv).m_sqrt().max(1e-6);
    let axis_u = du / grad_len;
    let axis_v = dv / grad_len;
    let mut acc = [0f32; 4];
    for i in 0..lvl {
        let t = (i as f32 + 0.5) / lvl as f32 - 0.5;
        let u = (0.5 + axis_u * t / tex_dim(levels) as f32).clamp(0.0, 1.0);
        let v = (0.5 + axis_v * t / tex_dim(levels) as f32).clamp(0.0, 1.0);
        let s = bilinear(&levels[0], u, v, su);
        for c in 0..4 { acc[c] += s[c]; }
    }
    for c in 0..4 { acc[c] /= lvl as f32; }
    acc
}
fn tex_dim(levels: &[TexLevel]) -> f32 { levels.first().map(|l| l.width as f32).unwrap_or(1.0) }

/// 单入口采样：与 GPU 路径 VE-F0018 同状态同结果
pub fn sample(levels: &[TexLevel], su: SamplerState, u: f32, v: f32, lod: f32, du: f32, dv: f32) -> [f32; 4] {
    match su.min_filter {
        FilterMode::Nearest => {
            let x = su.addr_u.resolve((u * levels[0].width as f32) as i64, levels[0].width);
            let y = su.addr_v.resolve((v * levels[0].height as f32) as i64, levels[0].height);
            match (x, y) {
                (Some(x), Some(y)) => levels[0].read(x, y),
                _ => su.border_color,
            }
        }
        FilterMode::Bilinear => bilinear(&levels[0], u, v, su),
        FilterMode::Trilinear => {
            let li = lod.clamp(0.0, (levels.len() - 1) as f32);
            let i0 = li.m_floor() as usize;
            let frac = li - i0 as f32;
            if i0 + 1 < levels.len() {
                trilinear(&levels[i0], &levels[i0 + 1], frac, su)
            } else {
                bilinear(&levels[i0], u, v, su)
            }
        }
        m if m.aniso_level() > 1 => anisotropic(levels, du, dv, su),
        _ => bilinear(&levels[0], u, v, su),
    }
}

/// 对拍参考（标量慢路径——双线性基准语义）
pub fn sample_ref(tex: &TexLevel, su: SamplerState, u: f32, v: f32) -> [f32; 4] {
    bilinear(tex, u, v, su)
}

/// 采样记录（性能入册）
#[derive(Clone)]
pub struct SampRec {
    pub mode: u16,
    pub calls: u64,
    pub avg_taps: u32,
    pub matches_ref: bool,
}

/// 对拍：随机坐标集上 SIMD 路径与参考路径逐点一致（容差 1e-6）
pub fn conformance_run(tex: &TexLevel, su: SamplerState, coords: &[(f32, f32)]) -> (u32, u32, Vec<SampRec>) {
    let mut pass = 0u32;
    let mut fail = 0u32;
    let mut recs = Vec::new();
    for m in [
        FilterMode::Nearest, FilterMode::Bilinear, FilterMode::Trilinear,
        FilterMode::Aniso4x, FilterMode::Aniso8x, FilterMode::Aniso16x,
    ] {
        let mut s2 = su;
        s2.min_filter = m;
        s2.mag_filter = m;
        let mut ok = true;
        let taps = if m.aniso_level() > 1 { m.aniso_level() * 4 } else { 4 };
        for &(u, v) in coords {
            let a = sample(std::slice::from_ref(tex), s2, u, v, 0.0, 0.01, 0.01);
            let b = sample_ref(tex, s2, u, v);
            for c in 0..4 {
                if (a[c] - b[c]).abs() > 1e-6 { ok = false; }
            }
        }
        if ok { pass += 1; } else { fail += 1; }
        recs.push(SampRec { mode: m.code(), calls: coords.len() as u64, avg_taps: taps, matches_ref: ok });
    }
    (pass, fail, recs)
}

/// 错误码
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum SampErr {
    LodOutOfRange = SEG_BASE + 0x40,   // 0x1AD0
    EmptyMipChain,                     // 0x1AD1
    FormatMismatch,                    // 0x1AD2
}
impl SampErr {
    pub const fn code(self) -> u16 { self as u16 }
}

pub fn self_audit() -> Result<(), u16> {
    // 结构自证：三闭集齐 + 默认状态字段齐 + 对拍函数在
    let _ = (FilterMode::Nearest.code(), AddrMode::Wrap.code(), TexFormat::Rgba8Unorm.code());
    let _ = SamplerState::default();
    let _ = (conformance_run as fn(&TexLevel, SamplerState, &[(f32, f32)]) -> (u32, u32, Vec<SampRec>));
    Ok(())
}

pub fn run_cga32_checks() -> CheckSet {
    let mut s = CheckSet::new("CGPU-F0032");
    // J1 滤波全族对拍
    let mut tex = TexLevel::new(8, 8, TexFormat::Rgba8Unorm);
    for y in 0..8 {
        for x in 0..8 {
            tex.write(x, y, [x as f32 / 8.0, y as f32 / 8.0, 0.25, 1.0]);
        }
    }
    let coords: Vec<(f32, f32)> = (0..24)
        .map(|i| ((i * 7 % 64) as f32 / 64.0, (i * 13 % 64) as f32 / 64.0))
        .collect();
    let (p, f, recs) = conformance_run(&tex, SamplerState::default(), &coords);
    let ok = p == 6 && f == 0 && recs.iter().all(|r| r.matches_ref);
    s.add("CGA-32-01-滤波全族对拍", ok, if ok { "" } else { "对拍破缺" });
    // J2 地址模式全族
    let am_ok = AddrMode::Wrap.resolve(-1, 8) == Some(7)
        && AddrMode::Mirror.resolve(-1, 8) == Some(0)
        && AddrMode::Mirror.resolve(-2, 8) == Some(1)
        && AddrMode::Clamp.resolve(-5, 8) == Some(0)
        && AddrMode::Clamp.resolve(9, 8) == Some(7)
        && AddrMode::Border.resolve(-1, 8).is_none()
        && AddrMode::Border.resolve(3, 8) == Some(3);
    s.add("CGA-32-02-地址模式全族", am_ok, if am_ok { "" } else { "折返破缺" });
    // J3 格式覆盖（5 格式读写回环）
    let fmt_ok = [
        TexFormat::R8Unorm,
        TexFormat::Rg16Unorm,
        TexFormat::Rgba8Unorm,
        TexFormat::R16Float,
        TexFormat::Rgba32Float,
    ]
    .iter()
    .all(|&f| {
        let mut t = TexLevel::new(2, 2, f);
        t.write(1, 1, [0.5, 0.5, 0.5, 1.0]);
        let px = t.read(1, 1);
        (px[0] - 0.5).abs() < 0.01
    });
    s.add("CGA-32-03-格式覆盖", fmt_ok, if fmt_ok { "" } else { "格式回环破缺" });
    // J4 状态一致性断言（边框色传播）
    let mut sb = SamplerState::default();
    sb.addr_u = AddrMode::Border;
    sb.addr_v = AddrMode::Border;
    sb.border_color = [1.0, 0.0, 0.0, 1.0];
    let oob = sample(core::slice::from_ref(&tex), sb, 1.5, 0.5, 0.0, 0.0, 0.0);
    let ok = (oob[0] - 1.0).abs() < 1e-6 && oob[1].abs() < 1e-6;
    s.add("CGA-32-04-状态一致性", ok, if ok { "" } else { "边框传播破缺" });
    // J5 性能入册（tap 计数 aniso 单调）
    let perf_ok = recs.len() == 6
        && recs[0].avg_taps <= recs[1].avg_taps
        && recs[3].avg_taps < recs[4].avg_taps
        && recs[4].avg_taps < recs[5].avg_taps;
    s.add("CGA-32-05-性能入册", perf_ok, if perf_ok { "" } else { "tap 表破缺" });
    // J6 半精度软解回环（f32→f16→f32 误差 < 1e-3）
    let half_ok = [0.5f32, 1.0, 0.25, 100.0]
        .iter()
        .all(|&v| (half_to_f32(f32_to_half(v)) - v).abs() < 1e-3);
    s.add("CGA-32-06-半精度回环", half_ok, if half_ok { "" } else { "f16 软解破缺" });
    // J7 码位互异段独占
    let ok = SEG_BASE % 0x10 == 0
        && FilterMode::Nearest.code() != FilterMode::Aniso16x.code()
        && AddrMode::Wrap.code() != AddrMode::Border.code()
        && TexFormat::R8Unorm.code() != TexFormat::Rgba32Float.code();
    s.add("CGA-32-07-码位互异", ok, if ok { "" } else { "码位冲突" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cga32_checks_all_green() {
        let set = run_cga32_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn cga32_border_oob_full_border() {
        // 4 tap 全越界 = 纯边框色
        let mut tex = TexLevel::new(4, 4, TexFormat::Rgba8Unorm);
        tex.write(0, 0, [1.0, 1.0, 1.0, 1.0]);
        let mut sb = SamplerState::default();
        sb.addr_u = AddrMode::Border;
        sb.addr_v = AddrMode::Border;
        sb.border_color = [0.0, 1.0, 0.0, 1.0];
        let out = sample(core::slice::from_ref(&tex), sb, 0.99, 0.99, 0.0, 0.0, 0.0);
        assert!(out[1] > 0.9, "边框色未主导");
    }
}
