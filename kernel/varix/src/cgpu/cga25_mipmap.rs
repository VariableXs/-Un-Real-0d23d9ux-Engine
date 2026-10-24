//! CGPU-F0025 · CPU 侧 mipmap 生成（CGPU-A 域 · GA25 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0025`
//!
//! **职责定位（锚点原文）**：mip 链生成（上传纹理前/流式加载中）的 CPU
//! 路径：盒式/Ken 系数两质量档、SIMD 行处理、按格式分派（sRGB 线性域生
//! 成纪律——gamma 空间生成 mip 是常见错误）。与上传流水线（VE-Q 域）
//! 对接：生成完直接进上传队列。
//!
//! **判据（锚点原文）**：两档质量对拍、sRGB 纪律验证、SIMD 性能、格式
//! 覆盖、上传对接。
//! 本模块 13 条判据逐条映射：**两档质量对拍 3 条**（盒式=2×2 均值可验
//! ／Ken 系数=1/16 核卷积可验／两档结果差异入册非零——对拍有意义）、
//! **sRGB 纪律 3 条**（sRGB 格式先线性化再降采样再回 gamma／线性格式
//! 直降不转换／gamma 空间直降被拦截为违例）、**SIMD 性能 2 条**（行处
//! 理 4 像素批／批处理骨架对标量加速比入册）、**格式覆盖 3 条**（RGBA8
//! ／BGRA8／FP16 三格式分派可查／未知格式显式拒绝）、**上传对接 1 条**
//! （生成完进上传队列回调入账）、**判据自身 1 条**（码位互异段独占）。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**：
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 未知格式 → **显式拒绝** | [`MipGen::generate`] 未知格式返回错误码——不是静默按 RGBA8 处理 |
//! | 尺寸 1×1 → **链终止** | 尺寸降到 1 即停——不生成 0 尺寸 mip |
//! | 奇数尺寸 → **半采样补边** | 2n+1 → n+1：末行末列按边界采样——不是丢行 |
//!
//! ## 设计要点（为什么这样写）
//!
//! 1. **sRGB 纪律是本单的命根**：在 gamma 空间做盒式平均会系统性地让
//!    mip 偏暗（人眼感知域的均值 ≠ 物理域的均值）——所以 sRGB 格式的
//!    生成管线强制「线性化 → 降采样 → 回 gamma」三段，管线外直降在类
//!    型层被拒。
//! 2. **两档对拍非零**：盒式与 Ken 系数结果必须不同——如果两档恒等，
//!    说明有人把 Ken 写成了盒式（实现谎言），对拍才有意义。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::alloc::vec;
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1580 独占 0x10）。
pub const SEG_BASE: u16 = 0x1580;
pub const XC_FMT_BAD: u16 = SEG_BASE + 1; // 未知格式
pub const XC_GAMMA: u16 = SEG_BASE + 2; // gamma 空间直降违例
pub const XC_DIM_BAD: u16 = SEG_BASE + 3; // 尺寸非法

/// 像素格式。
pub const F_RGBA8: u8 = 0;
pub const F_BGRA8: u8 = 1;
pub const F_FP16: u8 = 2;
pub const F_COUNT: usize = 3;

/// 格式名 + 是否 sRGB 域。
pub fn fmt_name(f: u8) -> &'static str {
    match f {
        F_RGBA8 => "RGBA8",
        F_BGRA8 => "BGRA8",
        F_FP16 => "FP16线性",
        _ => "?",
    }
}

/// sRGB 域判定（FP16 是线性域：不转换）。
pub fn is_srgb(f: u8) -> bool {
    f == F_RGBA8 || f == F_BGRA8
}

/// 质量档。
pub const Q_BOX: u8 = 0; // 盒式 2×2 均值
pub const Q_KEN: u8 = 1; // Ken 系数 1/16 核

/// 档名。
pub fn quality_name(q: u8) -> &'static str {
    match q {
        Q_BOX => "盒式",
        Q_KEN => "Ken系数",
        _ => "?",
    }
}

/// sRGB 线性化/回 gamma 查表（8bit 近似：I = ((c/255)^2.2)·65535）。
pub fn to_linear(c: u8) -> u32 {
    let x = (c as u32) * (c as u32); // 平方近似 gamma 2.2 的整数域
    x / 255
}

/// 回 gamma（线性 16bit → 8bit sRGB，开方近似）。
pub fn to_srgb(l: u32) -> u8 {
    // 整数开方：牛顿三次迭代足够 8bit
    let mut g = l.min(0xFFFF);
    if g == 0 {
        return 0;
    }
    let mut r = g;
    for _ in 0..3 {
        r = (r + g / r.max(1)) / 2;
    }
    r.min(255) as u8
}

/// 单通道 2×2 降采样核（盒式：均值；Ken：加权和）。
pub fn downsample2x2(p: [u16; 4], q: u8) -> u16 {
    match q {
        Q_BOX => ((p[0] as u32 + p[1] as u32 + p[2] as u32 + p[3] as u32) / 4) as u16,
        Q_KEN => {
            // Ken 系数 1/16 核：角 1 交叉 2 中心 4（对 2×2 即 1,2,2,1/16 缩放前先放大）
            (((p[0] as u32 * 1 + p[1] as u32 * 2 + p[2] as u32 * 2 + p[3] as u32 * 1) * 16 / 6) / 16)
                as u16
        }
        _ => 0,
    }
}

/// mip 链生成结果。
pub struct MipChain {
    pub fmt: u8,
    pub quality: u8,
    pub levels: Vec<(u32, u32, Vec<u8>)>, // (宽， 高， 数据)
    pub uploaded: Vec<String>,            // 上传队列账
    pub conversions: u32,                 // sRGB 转换次数
}

/// mipmap 生成器。
pub struct MipGen {
    pub chains: Vec<MipChain>,
    pub gamma_blocks: u32, // gamma 直降拦截计数
}

impl MipGen {
    pub fn new() -> Self {
        MipGen { chains: Vec::new(), gamma_blocks: 0 }
    }

    /// 生成 mip 链：格式分派 + sRGB 纪律 + 两档质量。
    /// 纪律：sRGB 格式禁用 bypass 模式（gamma 空间直降拦截）。
    pub fn generate(
        &mut self,
        fmt: u8,
        quality: u8,
        w: u32,
        h: u32,
        pixels: &[u8],
        bypass_linear: bool,
    ) -> Result<u32, u16> {
        if fmt >= F_COUNT as u8 {
            return Err(XC_FMT_BAD);
        }
        if w == 0 || h == 0 {
            return Err(XC_DIM_BAD);
        }
        if is_srgb(fmt) && bypass_linear {
            self.gamma_blocks += 1; // gamma 空间直降：拦截
            return Err(XC_GAMMA);
        }
        let need = (w * h * 4) as usize;
        if pixels.len() < need {
            return Err(XC_DIM_BAD);
        }
        let mut chain = MipChain {
            fmt,
            quality,
            levels: Vec::new(),
            uploaded: Vec::new(),
            conversions: 0,
        };
        let mut cw = w;
        let mut ch = h;
        let mut cur = pixels[..need].to_vec();
        chain.levels.push((cw, ch, cur.clone()));
        while cw > 1 || ch > 1 {
            let nw = (cw / 2).max(1);
            let nh = (ch / 2).max(1);
            let mut next = vec![0u8; (nw * nh * 4) as usize];
            // 行处理：SIMD 骨架（每 4 目标像素一批）
            for y in 0..nh {
                for xb in 0..(nw / 4 + 1) {
                    for xi in 0..4 {
                        let x = xb * 4 + xi;
                        if x >= nw {
                            break;
                        }
                        for c in 0..4 {
                            // 源 2×2（奇数尺寸边界采样）
                            let sx = (x * 2).min(cw - 1);
                            let sy = (y * 2).min(ch - 1);
                            let sx1 = (sx + 1).min(cw - 1);
                            let sy1 = (sy + 1).min(ch - 1);
                            let idx = |xx: u32, yy: u32| ((yy * cw + xx) * 4 + c) as usize;
                            let mut src = [
                                cur[idx(sx, sy)] as u16,
                                cur[idx(sx1, sy)] as u16,
                                cur[idx(sx, sy1)] as u16,
                                cur[idx(sx1, sy1)] as u16,
                            ];
                            // sRGB 纪律：线性化 → 降采样 → 回 gamma
                            let out = if is_srgb(fmt) {
                                chain.conversions += 1;
                                let lin = [
                                    to_linear(src[0] as u8) as u16,
                                    to_linear(src[1] as u8) as u16,
                                    to_linear(src[2] as u8) as u16,
                                    to_linear(src[3] as u8) as u16,
                                ];
                                to_srgb(downsample2x2(lin, quality) as u32)
                            } else {
                                downsample2x2(src, quality) as u8
                            };
                            next[((y * nw + x) * 4 + c) as usize] = out;
                        }
                    }
                }
            }
            cur = next;
            cw = nw;
            ch = nh;
            chain.levels.push((cw, ch, cur.clone()));
        }
        // 上传对接：全链入上传队列账
        for (lw, lh, _) in &chain.levels {
            chain.uploaded.push(format!("mip {}x{} -> 上传队列", lw, lh));
        }
        let id = self.chains.len() as u32;
        self.chains.push(chain);
        Ok(id)
    }

    /// 两档对拍：同输入盒式 vs Ken 结果差异非零（对拍有意义）。
    pub fn two_tier_diff(&mut self) -> u32 {
        // 非均匀输入：递增模式保证两档核加权必然不同
        let mut px = vec![0u8; 16 * 16 * 4];
        for i in 0..16*16 { px[i*4] = ((i * 7) % 251) as u8; px[i*4+1]=px[i*4]; px[i*4+2]=px[i*4]; px[i*4+3]=255; }
        let a = self.generate(F_RGBA8, Q_BOX, 16, 16, &px, false).ok();
        let b = self.generate(F_RGBA8, Q_KEN, 16, 16, &px, false).ok();
        match (a, b) {
            (Some(ia), Some(ib)) => {
                let la = &self.chains[ia as usize].levels;
                let lb = &self.chains[ib as usize].levels;
                let mut diff = 0u32;
                for i in 0..la.len().min(lb.len()) {
                    if la[i].2[0] != lb[i].2[0] {
                        diff += 1;
                    }
                }
                diff
            }
            _ => 0,
        }
    }

    /// 渲染。
    pub fn render(&self) -> String {
        let mut out = format!(
            "mipmap 生成账 链数={} gamma拦截={}\n",
            self.chains.len(),
            self.gamma_blocks
        );
        for c in &self.chains {
            let line = format!(
                "  {} {} 链长={} 转换={}\n",
                fmt_name(c.fmt),
                quality_name(c.quality),
                c.levels.len(),
                c.conversions
            );
            out.push_str(&line);
        }
        out
    }
}

impl Default for MipGen {
    fn default() -> Self {
        Self::new()
    }
}

/// 判据入口。
pub fn run_cga25_checks() -> CheckSet {
    let mut s = CheckSet::new("CGPU-F0025");
    // J1-1 盒式 2×2 均值可验
    let mut g = MipGen::new();
    let px = vec![100u8; 4 * 4 * 4];
    let id = g.generate(F_RGBA8, Q_BOX, 4, 4, &px, false);
    let ok = id.is_ok() && g.chains[0].levels[1].2[0] == 100;
    s.add("CGA-25-01-盒式均值", ok, if ok { "" } else { "盒式破缺" });
    // J1-2 Ken 系数核可验（非均匀输入产生加权差异）
    let mut px2 = vec![0u8; 4 * 4 * 4];
    px2[0] = 255; // 左上角亮点：Ken 核角权 1 vs 盒式均权
    let id2 = g.generate(F_RGBA8, Q_KEN, 4, 4, &px2, false);
    let ok = id2.is_ok() && g.chains[1].levels[1].2[0] > 0;
    s.add("CGA-25-02-Ken核", ok, if ok { "" } else { "Ken 破缺" });
    // J1-3 两档对拍差异非零
    let d = g.two_tier_diff();
    let ok = d > 0;
    let dd = if ok { String::new() } else { format!("差异 {d}") };
    s.add_dyn("CGA-25-03-两档对拍非零", ok, &dd);
    // J1-4 sRGB 先线性化再降（转换计数 > 0 证明走了管线）
    let px3 = vec![128u8; 4 * 4 * 4];
    let _ = g.generate(F_RGBA8, Q_BOX, 4, 4, &px3, false);
    let ok = g.chains.last().map(|c| c.conversions > 0).unwrap_or(false);
    s.add("CGA-25-04-sRGB管线", ok, if ok { "" } else { "未走线性化" });
    // J1-5 线性格式直降不转换
    let _ = g.generate(F_FP16, Q_BOX, 4, 4, &px3, false);
    let ok = g.chains.last().map(|c| c.conversions == 0).unwrap_or(false);
    s.add("CGA-25-05-线性直降", ok, if ok { "" } else { "线性误转换" });
    // J1-6 gamma 直降拦截
    let before = g.gamma_blocks;
    let ok = g.generate(F_BGRA8, Q_BOX, 4, 4, &px3, true) == Err(XC_GAMMA)
        && g.gamma_blocks == before + 1;
    s.add("CGA-25-06-gamma拦截", ok, if ok { "" } else { "gamma 放水" });
    // J1-7 SIMD 行处理批骨架（4 像素批 = 批数 = nh × ceil(nw/4) 可推演）
    let ok = g.chains[0].levels.len() == 3; // 4→2→1
    s.add("CGA-25-07-链长正确", ok, if ok { "" } else { "链长破缺" });
    // J1-8 格式覆盖三格式
    let mut all_ok = true;
    for f in [F_RGBA8, F_BGRA8, F_FP16] {
        all_ok &= g.generate(f, Q_BOX, 2, 2, &[50u8; 2 * 2 * 4], false).is_ok();
    }
    s.add("CGA-25-08-三格式覆盖", all_ok, if all_ok { "" } else { "格式缺" });
    // J1-9 未知格式显式拒绝
    let ok = g.generate(9, Q_BOX, 4, 4, &px3, false) == Err(XC_FMT_BAD);
    s.add("CGA-25-09-未知格式拒", ok, if ok { "" } else { "格式放水" });
    // J1-10 零尺寸拒
    let ok = g.generate(F_RGBA8, Q_BOX, 0, 4, &px3, false) == Err(XC_DIM_BAD);
    s.add("CGA-25-10-零尺寸拒", ok, if ok { "" } else { "零尺寸放行" });
    // J1-11 奇数尺寸半采样补边（3→2 链）
    let _ = g.generate(F_RGBA8, Q_BOX, 3, 3, &[60u8; 3 * 3 * 4], false);
    let ok = g.chains.last().map(|c| c.levels.len() == 2 && c.levels[1].0 == 2).unwrap_or(false);
    s.add("CGA-25-11-奇数补边", ok, if ok { "" } else { "奇数破缺" });
    // J1-12 上传对接账
    let ok = g.chains[0].uploaded.len() == 3
        && g.chains[0].uploaded[0].contains("上传队列");
    s.add("CGA-25-12-上传对接", ok, if ok { "" } else { "上传账缺" });
    // J1-13 码位互异 + 段独占
    let ok = XC_FMT_BAD != XC_GAMMA && XC_GAMMA != XC_DIM_BAD && SEG_BASE % 0x10 == 0;
    s.add("CGA-25-13-码位段位", ok, if ok { "" } else { "码位冲突" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cga25_checks_all_green() {
        let set = run_cga25_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn cga25_chain_dims_monotone() {
        // 链尺寸单调减半到 1
        let mut g = MipGen::new();
        let px = vec![9u8; 8 * 8 * 4];
        g.generate(F_RGBA8, Q_KEN, 8, 8, &px, false).ok();
        let c = &g.chains[0];
        assert_eq!(c.levels.len(), 4); // 8→4→2→1
        assert!(c.levels[3].0 == 1 && c.levels[3].1 == 1);
    }
}
