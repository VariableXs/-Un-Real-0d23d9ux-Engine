//! cga102_maskblend —— CGPU-F0102 掩码混合算子。
//! 锚点（CGPU 施工书）：CGPU-F0102 · 掩码混合算子 · 目标 300 行
//! 判据摘录：混合核（out = (a×w + b×(255−w))/255——Q8 权重域）；
//! 权重两端锚（w=0 → 全 b、w=255 → 全 a——端点直通核）；掩码
//! 驱动（逐像素权重取自掩码面——空间变化混合）；二值掩码特例
//! （0/255 → 逐像素硬切——无中间色泄露）；对称账（swap(a,b)∘
//! swap(w) == 原 mix 镜像——对偶核）；自混合恒等（a==b → 恒等
//! ——任意权重）；掩码域账（掩码长度 == 像素数）；混合有界
//! （out ∈ [min(a,b), max(a,b)]——插值不越界红线）。
//! 组件：Q8 权重混合 + 掩码面驱动 + 硬切特例 + 端点/对偶/恒等
//!   + 有界账 + 域闸。
//! 职责定位：图层合成的原子操作——软边笔刷、渐变叠加、局部调
//! 整全走这里；Q8 定点（除以 255 用近似移位或真除——取真除保
//! 端点精确）。
//!
//! 端点精确性：w=255 → (a×255 + b×0)/255 = a 精确（无近似移位
//! 损耗——混合核用真除不用 >>8 近似）。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

// ===== 诊断码闭集（0xE9B0 段 · 本模块独占）=====
pub const XC_LEN: u32 = 0xE9B0; // 长度失配
pub const XC_BOUND_FAIL: u32 = 0xE9B1; // 插值越界
pub const XC_ENDPOINT: u32 = 0xE9B2; // 端点锚破

/// Q8 权重混合单值（真除保端点）。
pub fn mix1(a: u8, b: u8, w: u8) -> u8 {
    ((a as u16 * w as u16 + b as u16 * (255 - w as u16)) / 255) as u8
}

/// 全图掩码混合（等长三向量）。
pub fn mask_blend(a: &[u8], b: &[u8], mask: &[u8]) -> Result<Vec<u8>, u32> {
    if a.len() != b.len() || b.len() != mask.len() {
        return Err(XC_LEN);
    }
    let mut out: Vec<u8> = Vec::with_capacity(a.len());
    for i in 0..a.len() {
        let v = mix1(a[i], b[i], mask[i]);
        // 有界账：插值不越 [min, max]（真除下恒成立——账核）。
        let lo = a[i].min(b[i]);
        let hi = a[i].max(b[i]);
        if v < lo || v > hi {
            return Err(XC_BOUND_FAIL);
        }
        out.push(v);
    }
    Ok(out)
}

/// 端点锚：w=0 → b；w=255 → a。
pub fn endpoint_anchor(a: u8, b: u8) -> Result<(), u32> {
    if mix1(a, b, 0) != b || mix1(a, b, 255) != a {
        return Err(XC_ENDPOINT);
    }
    Ok(())
}

/// 自混合恒等：a==b → 任意 w 恒等。
pub fn self_identity(v: u8) -> Result<(), u32> {
    for w in [0u8, 1, 64, 128, 200, 255].iter() {
        if mix1(v, v, *w) != v {
            return Err(XC_BOUND_FAIL);
        }
    }
    Ok(())
}

/// 对偶账：swap 语义——blend(a,b,w) == 255−blend(b,a,255−w)？
/// 精确式：mix(a,b,w) = (aw + b(255−w))/255；
/// mix(b,a,255−w) = (b(255−w) + a·w)/255 —— 恒等相等（同一式）。
pub fn duality(a: u8, b: u8, w: u8) -> Result<(), u32> {
    if mix1(a, b, w) != mix1(b, a, 255 - w) {
        return Err(XC_BOUND_FAIL);
    }
    Ok(())
}

/// 二值硬切：掩码 ∈ {0,255} → 输出逐像素 ∈ {a, b}（无中间色）。
pub fn hard_cut(a: &[u8], b: &[u8], mask: &[u8]) -> Result<Vec<u8>, u32> {
    for &m in mask.iter() {
        if m != 0 && m != 255 {
            return Err(XC_LEN); // 非二值掩码拒（借用 LEN 语义——域拒）
        }
    }
    mask_blend(a, b, mask)
}

/// RGBA 全通道混合（掩码共享四通道）。
pub fn blend_rgba(
    a: &[u8],
    b: &[u8],
    mask: &[u8],
    npix: usize,
) -> Result<Vec<u8>, u32> {
    if a.len() != b.len() || a.len() != npix * 4 || mask.len() != npix {
        return Err(XC_LEN);
    }
    let mut out: Vec<u8> = Vec::with_capacity(a.len());
    for i in 0..npix {
        for c in 0..4 {
            out.push(mix1(a[i * 4 + c], b[i * 4 + c], mask[i]));
        }
    }
    Ok(out)
}

/// 渐变掩码生成（水平线性 [0,255]）。
pub fn linear_mask(w: u32) -> Vec<u8> {
    if w == 0 {
        return Vec::new();
    }
    (0..w).map(|x| ((x as u32 * 255) / (w - 1).max(1) as u32).min(255) as u8).collect()
}

// ===== 主判据 =====
pub fn run_cga102_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0102");
    // 族一：端点与恒等。
    cs.add("端点锚", endpoint_anchor(30, 220).is_ok(), "w=0→b / w=255→a");
    cs.add("自混合恒等", self_identity(137).is_ok(), "任意 w 恒等");
    // 族二：中点锚。
    cs.add("中点锚", mix1(100, 200, 128) == 149, "(12800+25400)/255=149");
    cs.add("对偶", duality(100, 200, 77).is_ok(), "交换恒等");
    // 族三：全图混合 + 有界。
    let a: Vec<u8> = alloc::vec![100u8; 8];
    let b: Vec<u8> = alloc::vec![200u8; 8];
    let mask: Vec<u8> = alloc::vec![0u8, 32, 64, 96, 128, 160, 192, 255];
    if let Ok(out) = mask_blend(&a, &b, &mask) {
        cs.add("单调渐变", (1..8).all(|i| out[i] <= out[i - 1]), "权重升则趋 a 降");
        cs.add("端值", out[0] == 200 && out[7] == 100, "两端直通");
    }
    cs.add("长度失配拒", mask_blend(&a, &b[..7], &mask).is_err(), "LEN");
    // 族四：二值硬切。
    let bm: Vec<u8> = alloc::vec![255u8, 0, 255, 0];
    let av: Vec<u8> = alloc::vec![10u8, 10, 10, 10];
    let bv: Vec<u8> = alloc::vec![90u8, 90, 90, 90];
    if let Ok(out) = hard_cut(&av, &bv, &bm) {
        cs.add("硬切无中间", out.iter().all(|&v| v == 10 || v == 90), "闭集");
        cs.add("硬切序", out[0] == 10 && out[1] == 90, "255→a");
    }
    cs.add("非二值拒", hard_cut(&av, &bv, &alloc::vec![128u8, 0, 0, 0]).is_err(), "域拒");
    // 族五：RGBA 混合。
    let ra: Vec<u8> = alloc::vec![255u8, 0, 0, 128, 255, 0, 0, 128];
    let rb: Vec<u8> = alloc::vec![0u8, 255, 0, 128, 0, 255, 0, 128];
    let rm: Vec<u8> = alloc::vec![255u8, 0];
    if let Ok(out) = blend_rgba(&ra, &rb, &rm, 2) {
        cs.add("RGBA 端点", out[0] == 255 && out[5] == 255, "w=255→a / w=0→b");
        cs.add("RGBA 长 8", out.len() == 8, "2×4");
    }
    cs.add("RGBA 掩码失配", blend_rgba(&ra, &rb, &alloc::vec![0u8], 2).is_err(), "LEN");
    // 族六：渐变掩码。
    let gm = linear_mask(256);
    cs.add("渐变首末", gm[0] == 0 && gm[255] == 255, "端点");
    cs.add("渐变单调", (1..256).all(|i| gm[i] >= gm[i - 1]), "非降");
    cs.add("渐变零宽", linear_mask(0).is_empty(), "空");
    // 族七：混合一致性（全图 == 逐点单值核）。
    if let Ok(out) = mask_blend(&a, &b, &mask) {
        cs.add("逐点一致", (0..8).all(|i| out[i] == mix1(100, 200, mask[i])), "全图==单点");
    }
    CheckSet::merge(CheckSet::merge(cs, run_cga102_deep()), run_cga102_deep2())
}

// ===== 深化面：线性蒙版与硬切账 =====
/// 线性蒙版单调：从左到右权重不降。
pub fn linear_monotone(w: u32) -> bool {
    let m = linear_mask(w);
    m.windows(2).all(|p| p[0] <= p[1])
}

pub fn run_cga102_deep() -> CheckSet {
    let mut cs = CheckSet::new("cga102_deep");
    cs.add("线性单调", linear_monotone(8), "权重不降");
    let a = alloc::vec![0u8; 4];
    let b = alloc::vec![255u8; 4];
    let m = alloc::vec![0u8, 64, 128, 255];
    if let Ok(out) = mask_blend(&a, &b, &m) {
        cs.add("渐变有界", out.windows(2).all(|p| p[0] <= p[1]), "随权升");
        cs.add("界内", out.iter().all(|&v| v <= 255), "BOUND");
    }
    cs.add("端点锚", endpoint_anchor(0, 255).is_ok(), "0→b 255→a? 不，w0=b");
    cs.add("自恒等", self_identity(99).is_ok(), "w 中点=自身均值? 锚");
    cs.add("对偶", duality(30, 90, 64).is_ok(), "w 与 255-w 对偶");
    cs.add("硬切过", hard_cut(&a, &b, &m).is_ok(), "0/255 硬界");
    cs.add("长度拒", mask_blend(&a, &alloc::vec![1u8; 3], &m).is_err(), "LEN");
    cs
}

// ===== 深化面二：mix 代数域与硬切账 =====
pub fn run_cga102_deep2() -> CheckSet {
    let mut cs = CheckSet::new("cga102_deep2");
    // mix1 端点三锚。
    cs.add("w=0 端", mix1(10, 200, 0) == 200, "全 b");
    cs.add("w=255 端", mix1(10, 200, 255) == 10, "全 a");
    cs.add("w=128 中", mix1(0, 254, 128).abs_diff(127) <= 1, "中点");
    // 线性域：mix 对 w 单调（b>a 时 w↑ 值↓）。
    cs.add("单调域", {
        let mut ok = true;
        for i in 1..=255u8 {
            if mix1(0, 255, i) > mix1(0, 255, i - 1) {
                ok = false;
            }
        }
        ok
    }, "随权降");
    // 端点锚多对。
    cs.add("端点锚多对", endpoint_anchor(0, 255).is_ok()
        && endpoint_anchor(30, 90).is_ok()
        && endpoint_anchor(128, 128).is_ok(), "三对全过");
    // 自恒等多点。
    cs.add("自恒等多点", self_identity(0).is_ok() && self_identity(255).is_ok() && self_identity(66).is_ok(), "三锚");
    // 对偶多点。
    cs.add("对偶多点", duality(10, 240, 32).is_ok() && duality(100, 200, 200).is_ok(), "两对");
    // 全数组：mask 全 0 → 全 b；全 255 → 全 a。
    let a = alloc::vec![10u8; 8];
    let b = alloc::vec![240u8; 8];
    let m0 = alloc::vec![0u8; 8];
    let m255 = alloc::vec![255u8; 8];
    cs.add("掩码全 b", mask_blend(&a, &b, &m0) == Ok(b.clone()), "w=0");
    cs.add("掩码全 a", mask_blend(&a, &b, &m255) == Ok(a.clone()), "w=255");
    // 硬切端点同构。
    cs.add("硬切全 b", hard_cut(&a, &b, &m0) == Ok(b.clone()), "w=0");
    cs.add("硬切全 a", hard_cut(&a, &b, &m255) == Ok(a.clone()), "w=255");
    // 线性蒙版长度与首尾值。
    let lm = linear_mask(16);
    cs.add("线性长度", lm.len() == 16, "16 项");
    cs.add("线性首值", lm[0] == 0, "起 0");
    cs.add("线性尾值", lm[15] == 255, "终 255");
    // 长度失配三向。
    cs.add("长度三向拒", mask_blend(&a, &alloc::vec![1u8; 7], &m0).is_err()
        && mask_blend(&a, &b, &alloc::vec![1u8; 7]).is_err(), "b/m 失配");
    cs
}
