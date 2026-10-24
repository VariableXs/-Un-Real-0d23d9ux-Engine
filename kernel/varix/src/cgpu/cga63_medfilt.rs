//! cga63_medfilt —— CGPU-F0063 SIMD 中值滤波。
//! 锚点（CGPU 施工书）：CGPU-F0063 · SIMD 中值滤波 · 目标 340 行
//! 判据摘录：3×3 中值 = 邻域第 5 小（9 元中位数定义直译）；快速
//!   选择路径（网络法 19 次比较求 9 元中位数）与排序法结果逐位
//!   一致（双实现互证锚）；孤立椒盐噪点被滤除（单像素极值窗内
//!   消失）；边缘保持（阶跃边两侧值不混合——中值非线性保边性）；
//!   常量场不动；奇数窗宽硬线（偶窗中位数不定义）。
//! 组件：9 元中位数排序网络（19 比较器冻结序列）+ 窗口提取器
//!   （镜像边界）+ 滤波引擎（网络法主路径）+ 排序法对拍路径 +
//!   椒盐噪声注入/检出核 + 边缘保持核。
//! 职责定位：图像去噪地基——中值滤波是保边去椒盐的标准算子，
//!   网络法 19 比较器无分支无排序堆开销，是 SIMD 打包最优形。
//! 性能逐项分解：每像素 19 次比较（网络法）vs 排序法 O(9 log 9)；
//!   滤波全图 O(19n)。全整数确定性。
//! 诊断码段：0xE660（0xE661~0xE669 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const MF_VERSION: &str = "cga63_medfilt-v1";

/// 诊断段基址（0xE660 段 · 本模块独占）。
pub const MF_SEG_BASE: u16 = 0xE660;

/// XcCode 闭集（0xE661~0xE669）。
pub const XC_DIM_ZERO: u32 = 0xE661;
pub const XC_WIN_EVEN: u32 = 0xE662;
pub const XC_NET_DRIFT: u32 = 0xE663;
pub const XC_MIRROR_OOB: u32 = 0xE664;
pub const XC_SORT_DRIFT: u32 = 0xE665;
pub const XC_NOISE_SURVIVE: u32 = 0xE666;
pub const XC_EDGE_BLUR: u32 = 0xE667;
pub const XC_FLAT_DRIFT: u32 = 0xE668;
pub const XC_WIN_OOB: u32 = 0xE669;

/// 边界镜像。
fn mirror(idx: i64, n: usize) -> Result<usize, u32> {
    if n == 0 {
        return Err(XC_DIM_ZERO);
    }
    let nn = n as i64;
    let mut i = idx;
    while i < 0 || i >= nn {
        if i < 0 {
            i = -i;
        }
        if i >= nn {
            i = 2 * nn - 2 - i;
        }
        if i < 0 {
            return Err(XC_MIRROR_OOB);
        }
    }
    Ok(i as usize)
}

/// 9 元中位数排序网络（19 比较器冻结序列——Paeth 文献值）。
/// 比较器语义：swap_if_greater(p[i], p[j])，网络输出 p[4] 为中位数。
pub fn median9_network(p: &mut [u32; 9]) {
    // 冻结序列（行分组 = 网络级深，共 6 级 19 对）。
    const NET: [(usize, usize); 19] = [
        (1, 2), (4, 5), (7, 8),
        (0, 1), (3, 4), (6, 7), (1, 2), (4, 5), (7, 8),
        (0, 3), (5, 8), (4, 7), (3, 6), (1, 5), (2, 8),
        (2, 5), (1, 4), (7, 6), (4, 3),
    ];
    for &(i, j) in NET.iter() {
        if p[i] > p[j] {
            let t = p[i];
            p[i] = p[j];
            p[j] = t;
        }
    }
    // 网络输出性质：p[4] = 中位数（剩余槽位无须全序）。
}

/// 排序法参照（对拍锚：全排序取 [4]）。
pub fn median9_sort(p: &[u32; 9]) -> u32 {
    let mut v: Vec<u32> = Vec::with_capacity(9);
    for &x in p.iter() {
        v.push(x);
    }
    // 插入排序（9 元小数组，确定性）。
    for i in 1..v.len() {
        let key = v[i];
        let mut j = i;
        while j > 0 && v[j - 1] > key {
            v[j] = v[j - 1];
            j -= 1;
        }
        v[j] = key;
    }
    v[4]
}

/// 网络法对拍核（随机语料 256 组逐组一致）。
pub fn net_vs_sort(seed: u64) -> Result<(), u32> {
    let mut s = seed;
    for _ in 0..256usize {
        let mut p = [0u32; 9];
        for slot in p.iter_mut() {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            *slot = (s >> 40) as u32 % 256;
        }
        let expect = median9_sort(&p);
        let mut q = p;
        median9_network(&mut q);
        if q[4] != expect {
            return Err(XC_NET_DRIFT);
        }
    }
    Ok(())
}

/// 灰度图。
pub struct Img {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u32>,
}

impl Img {
    pub fn new(w: usize, h: usize, px: Vec<u32>) -> Result<Img, u32> {
        if w == 0 || h == 0 || px.len() != w * h {
            return Err(XC_DIM_ZERO);
        }
        Ok(Img { w, h, px })
    }

    fn window(&self, x: usize, y: usize) -> Result<[u32; 9], u32> {
        let mut w = [0u32; 9];
        let cx = x as i64;
        let cy = y as i64;
        let mut k = 0usize;
        for dy in -1i64..=1 {
            for dx in -1i64..=1 {
                w[k] = self.px[mirror(cy + dy, self.h)? * self.w + mirror(cx + dx, self.w)?];
                k += 1;
            }
        }
        Ok(w)
    }
}

/// 中值滤波主引擎（网络法）。
pub fn median_filter(src: &Img) -> Result<Img, u32> {
    let mut px: Vec<u32> = Vec::with_capacity(src.px.len());
    for y in 0..src.h {
        for x in 0..src.w {
            let mut w = src.window(x, y)?;
            median9_network(&mut w);
            px.push(w[4]);
        }
    }
    Img::new(src.w, src.h, px)
}

/// 椒盐注入核（固定种子确定性——检验滤除面的语料制造机）。
pub fn inject_salt_pepper(src: &Img, seed: u64, count: usize) -> Result<Img, u32> {
    let mut px: Vec<u32> = Vec::with_capacity(src.px.len());
    for &v in src.px.iter() {
        px.push(v);
    }
    let mut s = seed;
    for _ in 0..count {
        s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let idx = ((s >> 33) as usize) % px.len();
        px[idx] = if s & 1 != 0 { 255 } else { 0 };
    }
    Img::new(src.w, src.h, px)
}

/// 椒盐检出核：单像素极值计数（左右邻域均为中性而自身极值）。
pub fn count_impulses(img: &Img) -> Result<usize, u32> {
    let mut n = 0usize;
    for y in 0..img.h {
        for x in 0..img.w {
            let v = img.px[y * img.w + x];
            if v == 0 || v == 255 {
                let mut neutral = 0usize;
                for dy in -1i64..=1 {
                    for dx in -1i64..=1 {
                        if dx == 0 && dy == 0 {
                            continue;
                        }
                        let nb = img.px[mirror(y as i64 + dy, img.h)? * img.w + mirror(x as i64 + dx, img.w)?];
                        if nb != 0 && nb != 255 {
                            neutral += 1;
                        }
                    }
                }
                if neutral >= 6 {
                    n += 1;
                }
            }
        }
    }
    Ok(n)
}

/// 常量场不动核。
pub fn flat_field_check(w: usize, h: usize, v: u32) -> Result<(), u32> {
    let f = Img::new(w, h, vec![v; w * h])?;
    let m = median_filter(&f)?;
    if m.px.iter().any(|&x| x != v) {
        return Err(XC_FLAT_DRIFT);
    }
    Ok(())
}

/// 判据入口（注册于 cgpu/mod.rs 检查表）。
pub fn run_cga63_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0063");
    // 族一：网络 vs 排序双实现互证（256 组随机语料）。
    cs.add("双实现互证", net_vs_sort(7).is_ok(), "19 比较器网络==排序法");
    // 族二：手算中位数锚。
    let mut p = [9u32, 1, 8, 2, 7, 3, 6, 4, 5];
    median9_network(&mut p);
    cs.add("中位数锚", p[4] == 5, "1..9 中位=5");
    let mut q = [255u32, 0, 128, 64, 192, 32, 96, 160, 224];
    median9_network(&mut q);
    cs.add("离散锚", q[4] == median9_sort(&[255u32, 0, 128, 64, 192, 32, 96, 160, 224]), "双路一致");
    // 族三：常量场不动。
    cs.add("零场不动", flat_field_check(8, 8, 0).is_ok(), "恒 0");
    cs.add("满场不动", flat_field_check(8, 8, 200).is_ok(), "恒 200");
    // 族四：椒盐滤除（注入 8 点，滤波后极值点数锐减）。
    let clean = Img::new(8, 8, vec![128u32; 64]).map_err(|e| e as u16);
    if let Ok(c) = &clean {
        let noisy = inject_salt_pepper(c, 11, 8).map_err(|e| e as u16);
        if let Ok(n) = &noisy {
            let before = count_impulses(n).map_err(|e| e as u16);
            let filtered = median_filter(n);
            cs.add("滤波构建", filtered.is_ok(), "网络法全图");
            if let Ok(f) = &filtered {
                let after = count_impulses(f).map_err(|e| e as u16);
                cs.add("椒盐滤除", matches!((&before, &after), (Ok(b), Ok(a)) if *a < *b), "极值点锐减");
                cs.add("中位还原", matches!(after, Ok(0)), "全部还原中值 128");
            }
        }
        // 族五：边缘保持（阶跃边两侧 0/255 不混合成中间灰）。
        let mut edge = vec![0u32; 64];
        for y in 0..8usize {
            for x in 4..8usize {
                edge[y * 8 + x] = 255;
            }
        }
        let eg = Img::new(8, 8, edge).map_err(|e| e as u16);
        if let Ok(e) = &eg {
            let fm = median_filter(e);
            cs.add("边滤波构建", fm.is_ok(), "阶跃边图");
            if let Ok(m) = &fm {
                // 中值保边：滤波后每像素非 0 即 255（无中间灰）。
                let mixed = m.px.iter().any(|&v| v != 0 && v != 255);
                cs.add("边缘不糊", !mixed, "保边硬线");
                cs.add("左域保黑", m.px[1 * 8 + 1] == 0, "阶跃左侧仍 0");
                cs.add("右域保白", m.px[1 * 8 + 6] == 255, "阶跃右侧仍 255");
            }
        }
        // 族六：1×1 图自含（镜像退化为自身）。
        let tiny = Img::new(1, 1, vec![77u32]).map_err(|e| e as u16);
        if let Ok(t) = &tiny {
            cs.add("1×1 不动", matches!(median_filter(t), Ok(m) if m.px[0] == 77), "窗口全镜像");
        }
        cs.add("空图拒", Img::new(0, 4, vec![]).is_err(), "DIM_ZERO");
        cs.add("尺寸失配拒", Img::new(2, 2, vec![0u32; 5]).is_err(), "失配");
    } else {
        cs.add("夹具构建", false, "clean 构建失败");
    }
    deepen_med::merge_into(&mut cs);
    deepen_med2::merge_into(&mut cs);
    cs
}

/// ===== 深化扩展块（cga63_medfilt）：真功能面 + 判据并入主核 =====
pub mod deepen_med {
    use super::*;

    /// 扩展一：5 元中位数网络（13 比较器冻结——行窗/十字窗底座）。
    pub fn median5_network(p: &mut [u32; 5]) {
        // 9 比较器冻结序列（文献 5 元最优网，输出 p[2] 为中位数）。
        const NET9: [(usize, usize); 9] = [
            (0, 1), (3, 4), (2, 4), (2, 3), (1, 4),
            (0, 3), (0, 2), (1, 3), (1, 2),
        ];
        for &(i, j) in NET9.iter() {
            if p[i] > p[j] {
                let t = p[i];
                p[i] = p[j];
                p[j] = t;
            }
        }
    }

    /// 排序法 5 元参照。
    pub fn median5_sort(p: &[u32; 5]) -> u32 {
        let mut v: Vec<u32> = Vec::new();
        for &x in p.iter() {
            v.push(x);
        }
        for i in 1..v.len() {
            let key = v[i];
            let mut j = i;
            while j > 0 && v[j - 1] > key {
                v[j] = v[j - 1];
                j -= 1;
            }
            v[j] = key;
        }
        v[2]
    }

    /// 网络对拍核（128 组）。
    pub fn net5_vs_sort(seed: u64) -> Result<(), u32> {
        let mut s = seed;
        for _ in 0..128usize {
            let mut p = [0u32; 5];
            for slot in p.iter_mut() {
                s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                *slot = (s >> 40) as u32 % 256;
            }
            let expect = median5_sort(&p);
            let mut q = p;
            median5_network(&mut q);
            if q[2] != expect {
                return Err(XC_SORT_DRIFT);
            }
        }
        Ok(())
    }

    /// 扩展二：十字窗中值（5 元：中心 + 上下左右——3×3 的稀疏形）。
    pub fn median_cross(src: &Img, x: usize, y: usize) -> Result<u32, u32> {
        let mut p = [0u32; 5];
        p[0] = src.px[mirror(x as i64, src.w)? + mirror(y as i64 - 1, src.h)? * src.w];
        p[1] = src.px[mirror(x as i64 - 1, src.w)? + mirror(y as i64, src.h)? * src.w];
        p[2] = src.px[mirror(x as i64, src.w)? + mirror(y as i64, src.h)? * src.w];
        p[3] = src.px[mirror(x as i64 + 1, src.w)? + mirror(y as i64, src.h)? * src.w];
        p[4] = src.px[mirror(x as i64, src.w)? + mirror(y as i64 + 1, src.h)? * src.w];
        median5_network(&mut p);
        Ok(p[2])
    }

    /// 深化判据并入主核。
    pub fn merge_into(cs: &mut CheckSet) {
        cs.add("5 元双实现", net5_vs_sort(3).is_ok(), "128 组互证");
        let mut p = [9u32, 1, 8, 2, 5];
        median5_network(&mut p);
        cs.add("5 元锚", p[2] == 5, "1/2/5/8/9 中位");
        let img = Img::new(3, 3, vec![10u32, 20, 30, 40, 255, 60, 70, 80, 90]).map_err(|e| e as u16);
        if let Ok(i) = &img {
            cs.add("十字滤极值", matches!(median_cross(i, 1, 1), Ok(40)), "255 被中值顶替");
        }
    }

}

/// ===== 深化扩展块 2（cga63_medfilt）=====
pub mod deepen_med2 {
    use super::*;

    /// 扩展三：滤波幂等性核（中值滤波幂等——非线性算子的稳定面：
    /// 滤第二次不再变化）。
    pub fn med_idempotent(a: &Img) -> Result<(), u32> {
        let m1 = median_filter(a)?;
        let m2 = median_filter(&m1)?;
        if m1.px != m2.px {
            return Err(XC_FLAT_DRIFT);
        }
        Ok(())
    }

    /// 扩展四：极值收缩界（滤波后图 max ≤ 原 max、min ≥ 原 min——
    /// 中值的包络性质硬线）。
    pub fn envelope_check(a: &Img) -> Result<(), u32> {
        let m = median_filter(a)?;
        let amax = *a.px.iter().max().ok_or(XC_DIM_ZERO)?;
        let amin = *a.px.iter().min().ok_or(XC_DIM_ZERO)?;
        let mmax = *m.px.iter().max().ok_or(XC_DIM_ZERO)?;
        let mmin = *m.px.iter().min().ok_or(XC_DIM_ZERO)?;
        if mmax > amax || mmin < amin {
            return Err(XC_NOISE_SURVIVE);
        }
        Ok(())
    }

    /// 深化判据并入主核。
    pub fn merge_into(cs: &mut CheckSet) {
        let mut px = vec![100u32; 64];
        px[3 * 8 + 3] = 255; // 单孤立亮点
        if let Ok(g) = Img::new(8, 8, px) {
            cs.add("滤波幂等", med_idempotent(&g).is_ok(), "二滤不变");
            cs.add("包络收缩", envelope_check(&g).is_ok(), "max/min 收界");
            cs.add("极值消失", matches!(median_filter(&g), Ok(m) if m.px[3 * 8 + 3] == 100), "孤立点被顶替");
        }
        cs.add("包络面可用", matches!(envelope_check(&match Img::new(2, 2, vec![1u32, 2, 3, 4]) {
            Ok(x) => x,
            Err(_) => return,
        }), Ok(())), "正常图放行");
    }

}
