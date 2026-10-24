//! VE-F0790 噪声滤镜
//! 职责：确定性 LCG 伪随机噪声（同参数同结果），颗粒强度可控，
//!       支持单色（luma-only）与彩色两种模式。
//! 契约：seed 相同 → 输出逐位相同；强度 0 → 恒等。

use crate::checks::CheckSet;
use super::vec381_fbase::{FBitmap, Params, Pixel};

/// LCG 状态（确定性伪随机）
#[derive(Clone, Debug, PartialEq)]
pub struct Lcg {
    pub state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407) }
    }
    /// 下一个 [0,1) 值
    pub fn next01(&mut self) -> f64 {
        self.state = self.state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.state >> 33) as f64 / (1u64 << 31) as f64
    }
    /// 下一个 [-1,1) 值
    pub fn next_sym(&mut self) -> f64 {
        self.next01() * 2.0 - 1.0
    }
}

fn u8c(v: f64) -> u8 {
    v.round().clamp(0.0, 255.0) as u8
}

/// 单像素加噪
pub fn noise_px(p: Pixel, rng: &mut Lcg, amount: f64, mono: bool) -> Pixel {
    if mono {
        let n = rng.next_sym() * amount * 255.0;
        let add = |c: u8| u8c(c as f64 + n);
        [add(p[0]), add(p[1]), add(p[2]), p[3]]
    } else {
        let n0 = rng.next_sym() * amount * 255.0;
        let n1 = rng.next_sym() * amount * 255.0;
        let n2 = rng.next_sym() * amount * 255.0;
        [u8c(p[0] as f64 + n0), u8c(p[1] as f64 + n1), u8c(p[2] as f64 + n2), p[3]]
    }
}

/// 全图噪声（逐像素顺序推进 RNG）
pub fn grain(src: &FBitmap, seed: u64, amount: f64, mono: bool) -> FBitmap {
    let mut out = FBitmap::new(src.w, src.h);
    let mut rng = Lcg::new(seed);
    for y in 0..src.h {
        for x in 0..src.w {
            out.set(x, y, noise_px(src.get(x, y), &mut rng, amount, mono));
        }
    }
    out
}

/// 算子包装：seed / amount / mono 参数
pub fn op_noise(b: &FBitmap, p: &Params) -> Result<FBitmap, &'static str> {
    let seed = p.get_or("seed", 42.0) as i64 as u64;
    let amount = p.get_or("amount", 0.0);
    let mono = p.get_or("mono", 1.0) != 0.0;
    Ok(grain(b, seed, amount, mono))
}

pub fn run_vec390_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec390");
    // 族一：LCG 确定性
    let mut a = Lcg::new(7);
    let mut b = Lcg::new(7);
    let av: alloc::vec::Vec<f64> = (0..8).map(|_| a.next01()).collect();
    let bv: alloc::vec::Vec<f64> = (0..8).map(|_| b.next01()).collect();
    s.add("噪声-同种子同序列", av == bv, "determinism");
    s.add("噪声-值域01", av.iter().all(|&v| v >= 0.0 && v < 1.0), "range [0,1)");
    // 族二：对称值域
    let mut c = Lcg::new(9);
    let cv: alloc::vec::Vec<f64> = (0..64).map(|_| c.next_sym()).collect();
    s.add("噪声-对称值域", cv.iter().all(|&v| v >= -1.0 && v < 1.0), "range [-1,1)");
    s.add("噪声-非恒零", cv.iter().any(|&v| v > 0.5) && cv.iter().any(|&v| v < -0.5), "spread");
    // 族三：种子不同序列不同
    let mut d = Lcg::new(8);
    let dv: alloc::vec::Vec<f64> = (0..8).map(|_| d.next01()).collect();
    s.add("噪声-异种子异序列", av != dv, "different seeds");
    // 族四：强度 0 恒等
    let src = FBitmap::solid(4, 4, [77, 88, 99, 255]);
    let g0 = grain(&src, 1, 0.0, true);
    s.add("噪声-零强度恒等", g0.px == src.px, "amount 0");
    // 族五：确定性整图
    let g1 = grain(&src, 5, 0.2, true);
    let g2 = grain(&src, 5, 0.2, true);
    s.add("噪声-整图确定", g1.px == g2.px, "whole deterministic");
    // 族六：alpha 直通
    s.add("噪声-alpha直通", g1.get(0, 0)[3] == 255, "alpha keep");
    // 族七：单色扰动同 delta
    let mut e = Lcg::new(5);
    let np = noise_px([100, 100, 100, 255], &mut e, 0.1, true);
    s.add("噪声-单色等偏移", np[0] == np[1] && np[1] == np[2], "mono equal");
    // 族八：彩色扰动独立（大概率三通道不全等）
    let mut f = Lcg::new(11);
    let cp = noise_px([100, 100, 100, 255], &mut f, 0.3, false);
    let independent = !(cp[0] == cp[1] && cp[1] == cp[2]);
    s.add("噪声-彩色独立", independent, "color spread");
    // 族九：算子包装
    let mut p = Params::new();
    p.set("amount", 0.0);
    let oo = op_noise(&src, &p).expect("op");
    s.add("噪声-算子零恒等", oo.px == src.px, "op amount 0");
    let mut p2 = Params::new();
    p2.set("seed", 3.0).set("amount", 0.15);
    s.add("噪声-算子有响应", op_noise(&src, &p2).is_ok(), "op ok");
    s
}
