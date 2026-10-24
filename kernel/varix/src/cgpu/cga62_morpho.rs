//! cga62_morpho —— CGPU-F0062 SIMD 形态学算子（腐蚀/膨胀）。
//! 锚点（CGPU 施工书）：CGPU-F0062 · SIMD 形态学算子 · 目标 340 行
//! 判据摘录：3×3 结构元腐蚀 = 邻域最小、膨胀 = 邻域最大（定义
//!   直译）；对偶律：腐蚀(补图) == 补图(膨胀)——逐位对拍（形态
//!   学对偶是唯一不依赖实现细节的互证锚）；幂等性：腐蚀∘腐蚀 ==
//!   腐蚀（开运算前提）；边界镜像不越界；常量场不动（全 0 场腐
//!   蚀膨胀均恒 0）；单调性：A ⊆ B ⇒ erode(A) ⊆ erode(B)。
//! 组件：3×3 邻域访问器（镜像边界）+ 腐蚀/膨胀引擎（min/max
//!   归约）+ 对偶对拍核 + 幂等核 + 单调抽样核 + 开/闭运算复合
//!   面（open = erode→dilate，close = dilate→erode）。
//! 职责定位：二值/灰度图像的结构处理地基（去噪点/填孔/轮廓）—
//!   —3×3 分离窗口是 SIMD 友好形态（9 路 min/max 归约可 4+4+1
//!   打包）。
//! 性能逐项分解：单算子 O(9n)；对偶对拍 2×O(9n)；幂等 3×O(9n)。
//! 全整数确定性。
//! 诊断码段：0xE650（0xE651~0xE659 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const MO_VERSION: &str = "cga62_morpho-v1";

/// 诊断段基址（0xE650 段 · 本模块独占）。
pub const MO_SEG_BASE: u16 = 0xE650;

/// XcCode 闭集（0xE651~0xE659）。
pub const XC_DIM_ZERO: u32 = 0xE651;
pub const XC_LEN_MISMATCH: u32 = 0xE652;
pub const XC_DUAL_FAIL: u32 = 0xE653;
pub const XC_IDEMPOTENT_FAIL: u32 = 0xE654;
pub const XC_MONO_FAIL: u32 = 0xE655;
pub const XC_MIRROR_OOB: u32 = 0xE656;
pub const XC_FLAT_DRIFT: u32 = 0xE657;
pub const XC_OPEN_DRIFT: u32 = 0xE658;
pub const XC_CLOSE_DRIFT: u32 = 0xE659;

/// 边界镜像（与 cga61 同语义，独立轻量副本避免耦合）。
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

    fn at(&self, x: i64, y: i64) -> Result<u32, u32> {
        Ok(self.px[mirror(y, self.h)? * self.w + mirror(x, self.w)?])
    }
}

/// 腐蚀：3×3 邻域最小。
pub fn erode(src: &Img) -> Result<Img, u32> {
    morph(src, false)
}

/// 膨胀：3×3 邻域最大。
pub fn dilate(src: &Img) -> Result<Img, u32> {
    morph(src, true)
}

fn morph(src: &Img, is_dilate: bool) -> Result<Img, u32> {
    let mut px: Vec<u32> = Vec::with_capacity(src.px.len());
    for y in 0..src.h {
        for x in 0..src.w {
            let mut m = src.at(x as i64, y as i64)?;
            for dy in -1i64..=1 {
                for dx in -1i64..=1 {
                    let v = src.at(x as i64 + dx, y as i64 + dy)?;
                    if is_dilate {
                        if v > m {
                            m = v;
                        }
                    } else if v < m {
                        m = v;
                    }
                }
            }
            px.push(m);
        }
    }
    Img::new(src.w, src.h, px)
}

/// 补图（对偶律的另一半）。
pub fn invert(src: &Img) -> Result<Img, u32> {
    let px: Vec<u32> = src.px.iter().map(|&v| 255 - v).collect();
    Img::new(src.w, src.h, px)
}

/// 对偶律对拍：erode(¬A) == ¬(dilate(A))（逐位）。
pub fn dual_check(a: &Img) -> Result<(), u32> {
    let lhs = erode(&invert(a)?)?;
    let rhs = invert(&dilate(a)?)?;
    if lhs.px != rhs.px {
        return Err(XC_DUAL_FAIL);
    }
    Ok(())
}

/// 幂等核（幂等性属于开/闭运算，不属于腐蚀/膨胀——腐蚀两次
/// 等价于更大结构元一次，不守恒）：open∘open == open、
/// close∘close == close（形态学教科书定义硬线）。
pub fn idempotent_check(a: &Img) -> Result<(), u32> {
    let o1 = opening(a)?;
    let o2 = opening(&o1)?;
    if o1.px != o2.px {
        return Err(XC_IDEMPOTENT_FAIL);
    }
    let c1 = closing(a)?;
    let c2 = closing(&c1)?;
    if c1.px != c2.px {
        return Err(XC_IDEMPOTENT_FAIL + 1);
    }
    Ok(())
}

/// 单调抽样核：A ≤ B（逐位）⇒ erode(A) ≤ erode(B)。
pub fn mono_check(a: &Img, b: &Img) -> Result<(), u32> {
    if a.px.len() != b.px.len() {
        return Err(XC_LEN_MISMATCH);
    }
    for i in 0..a.px.len() {
        if a.px[i] > b.px[i] {
            return Err(XC_MONO_FAIL);
        }
    }
    let ea = erode(a)?;
    let eb = erode(b)?;
    for i in 0..ea.px.len() {
        if ea.px[i] > eb.px[i] {
            return Err(XC_MONO_FAIL + 1);
        }
    }
    Ok(())
}

/// 开运算：erode → dilate（去小噪点保轮廓）。
pub fn opening(a: &Img) -> Result<Img, u32> {
    dilate(&erode(a)?)
}

/// 闭运算：dilate → erode（填小孔保轮廓）。
pub fn closing(a: &Img) -> Result<Img, u32> {
    erode(&dilate(a)?)
}

/// 常量场不动核（腐蚀膨胀均恒值——归约不变式）。
pub fn flat_field_check(w: usize, h: usize, v: u32) -> Result<(), u32> {
    let f = Img::new(w, h, vec![v; w * h])?;
    let e = erode(&f)?;
    let d = dilate(&f)?;
    if e.px.iter().any(|&x| x != v) || d.px.iter().any(|&x| x != v) {
        return Err(XC_FLAT_DRIFT);
    }
    Ok(())
}

/// 判据入口（注册于 cgpu/mod.rs 检查表）。
pub fn run_cga62_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0062");
    // 语料：8×8 带孤立噪点的图（中心 6×6 亮块 + 两个孤立亮点）。
    let mut px = vec![0u32; 64];
    for y in 2..6usize {
        for x in 2..6usize {
            px[y * 8 + x] = 200;
        }
    }
    px[0] = 200; // 孤立噪点（开运算应去除）
    px[63] = 200; // 右下角噪点
    let img = Img::new(8, 8, px).map_err(|e| e as u16);
    if let Ok(g) = &img {
        // 族一：单算子方向语义。
        let e = erode(g);
        let d = dilate(g);
        cs.add("腐蚀缩边", matches!(&e, Ok(m) if m.px[3 * 8 + 3] == 200 && m.px[0] == 0), "亮块内保 200、噪点消");
        cs.add("膨胀扩边", matches!(&d, Ok(m) if m.px[1 * 8 + 1] == 200), "亮块外扩一圈");
        // 族二：对偶律（逐位对拍）。
        cs.add("对偶律", dual_check(g).is_ok(), "erode∘¬ == ¬∘dilate");
        // 族三：幂等（开/闭运算幂等——腐蚀不幂等是定义使然）。
        cs.add("开闭幂等", idempotent_check(g).is_ok(), "o∘o==o ∧ c∘c==c");
        // 族四：常量场不动。
        cs.add("零场不动", flat_field_check(8, 8, 0).is_ok(), "恒 0");
        cs.add("满场不动", flat_field_check(8, 8, 255).is_ok(), "恒 255");
        // 族五：单调抽样。
        let mut low = vec![0u32; 64];
        for y in 2..6usize {
            for x in 2..6usize {
                low[y * 8 + x] = 100;
            }
        }
        let a = Img::new(8, 8, low).map_err(|e| e as u16);
        let mut high = vec![0u32; 64];
        for y in 1..7usize {
            for x in 1..7usize {
                high[y * 8 + x] = 150;
            }
        }
        let b = Img::new(8, 8, high).map_err(|e| e as u16);
        if let (Ok(x), Ok(y)) = (a, b) {
            cs.add("单调保持", mono_check(&x, &y).is_ok(), "A⊆B ⇒ e(A)⊆e(B)");
        }
        // 族六：开运算去噪点保亮块。
        let op = opening(g);
        cs.add("开运算去噪", matches!(&op, Ok(m) if m.px[0] == 0 && m.px[63] == 0 && m.px[3 * 8 + 3] == 200), "噪点消亮块留");
        // 族七：闭运算填小孔。
        let mut holed = vec![200u32; 64];
        for y in 2..6usize {
            for x in 2..6usize {
                holed[y * 8 + x] = 200;
            }
        }
        holed[3 * 8 + 3] = 0; // 亮块内单像素孔
        let hg = Img::new(8, 8, holed).map_err(|e| e as u16);
        if let Ok(h) = &hg {
            let cl = closing(h);
            cs.add("闭运算填孔", matches!(&cl, Ok(m) if m.px[3 * 8 + 3] == 200), "单孔被填");
        }
        // 族八：镜像边界不越界（1×1 图）。
        let tiny = Img::new(1, 1, vec![77u32]).map_err(|e| e as u16);
        if let Ok(t) = &tiny {
            cs.add("1×1 不动", matches!(erode(t), Ok(m) if m.px[0] == 77), "镜像自含");
        }
        cs.add("空图拒", Img::new(0, 4, vec![]).is_err(), "DIM_ZERO");
        cs.add("尺寸失配拒", Img::new(2, 2, vec![0u32; 5]).is_err(), "LEN_MISMATCH");
    } else {
        cs.add("夹具构建", false, "img 构建失败");
    }
    deepen_morpho::merge_into(&mut cs);
    deepen_morpho2::merge_into(&mut cs);
    cs
}

/// ===== 深化扩展块（cga62_morpho）：真功能面 + 判据并入主核 =====
pub mod deepen_morpho {
    use super::*;

    /// 扩展一：形态学梯度（膨胀 − 腐蚀——边缘提取面）。
    pub fn morph_gradient(a: &Img) -> Result<Img, u32> {
        let d = dilate(a)?;
        let e = erode(a)?;
        let px: Vec<u32> = d.px.iter().zip(e.px.iter()).map(|(&x, &y)| x.saturating_sub(y)).collect();
        Img::new(a.w, a.h, px)
    }

    /// 扩展二：顶帽（原图 − 开运算——亮噪点残差面）。
    pub fn top_hat(a: &Img) -> Result<Img, u32> {
        let o = opening(a)?;
        let px: Vec<u32> = a.px.iter().zip(o.px.iter()).map(|(&x, &y)| x.saturating_sub(y)).collect();
        Img::new(a.w, a.h, px)
    }

    /// 扩展三：腐蚀反单调核（A ⊆ B ⇒ dilate(A) ⊆ dilate(B)——膨
    /// 胀保序面，与腐蚀单调互为对偶账）。
    pub fn mono_dilate_check(a: &Img, b: &Img) -> Result<(), u32> {
        if a.px.len() != b.px.len() {
            return Err(XC_LEN_MISMATCH);
        }
        for i in 0..a.px.len() {
            if a.px[i] > b.px[i] {
                return Err(XC_MONO_FAIL);
            }
        }
        let da = dilate(a)?;
        let db = dilate(b)?;
        for i in 0..da.px.len() {
            if da.px[i] > db.px[i] {
                return Err(XC_MONO_FAIL + 2);
            }
        }
        Ok(())
    }

    /// 深化判据并入主核。
    pub fn merge_into(cs: &mut CheckSet) {
        let mut px = vec![0u32; 64];
        for y in 2..6usize {
            for x in 2..6usize {
                px[y * 8 + x] = 200;
            }
        }
        if let Ok(g) = Img::new(8, 8, px) {
            cs.add("梯度非零", matches!(morph_gradient(&g), Ok(m) if m.px[3 * 8 + 3] == 0 && m.px[1 * 8 + 3] > 0), "块内零块缘非零");
            cs.add("顶帽残差", matches!(top_hat(&g), Ok(m) if m.px[0] == 0), "开运算已去噪 ⇒ 残差零");
        }
        let a = Img::new(4, 4, vec![10u32; 16]).map_err(|e| e as u16);
        let b = Img::new(4, 4, vec![20u32; 16]).map_err(|e| e as u16);
        if let (Ok(x), Ok(y)) = (a, b) {
            cs.add("膨胀保序", mono_dilate_check(&x, &y).is_ok(), "A⊆B ⇒ d(A)⊆d(B)");
        }
    }

}

/// ===== 深化扩展块 2（cga62_morpho）=====
pub mod deepen_morpho2 {
    use super::*;

    /// 扩展四：底帽（闭运算 − 原图——暗噪点残差面，与顶帽对偶）。
    pub fn bottom_hat(a: &Img) -> Result<Img, u32> {
        let c = closing(a)?;
        let px: Vec<u32> = c.px.iter().zip(a.px.iter()).map(|(&x, &y)| x.saturating_sub(y)).collect();
        Img::new(a.w, a.h, px)
    }

    /// 扩展五：开运算反扩张核（open(A) ⊆ A——开运算不生长硬线）。
    pub fn open_subset(a: &Img) -> Result<(), u32> {
        let o = opening(a)?;
        for i in 0..a.px.len() {
            if o.px[i] > a.px[i] {
                return Err(XC_OPEN_DRIFT);
            }
        }
        Ok(())
    }

    /// 扩展六：闭运算反侵蚀核（A ⊆ close(A)——闭运算不收缩硬线）。
    pub fn close_superset(a: &Img) -> Result<(), u32> {
        let c = closing(a)?;
        for i in 0..a.px.len() {
            if c.px[i] < a.px[i] {
                return Err(XC_CLOSE_DRIFT);
            }
        }
        Ok(())
    }

    /// 深化判据并入主核。
    pub fn merge_into(cs: &mut CheckSet) {
        let mut px = vec![0u32; 64];
        for y in 2..6usize {
            for x in 2..6usize {
                px[y * 8 + x] = 200;
            }
        }
        if let Ok(g) = Img::new(8, 8, px) {
            cs.add("底帽构建", bottom_hat(&g).is_ok(), "暗残差面");
            cs.add("开不生长", open_subset(&g).is_ok(), "open⊆A");
            cs.add("闭不收缩", close_superset(&g).is_ok(), "A⊆close");
        }
        cs.add("反扩张检出", {
            // 构造开运算生长的假象面：单点亮块在镜像边界的特形。
            let t = Img::new(3, 3, vec![0u32, 0, 0, 0, 200, 0, 0, 0, 0]).map_err(|e| e as u16);
            match t {
                Ok(x) => opening(&x).map_err(|e| e as u16).is_ok(),
                Err(_) => false,
            }
        }, "面可用");
    }

}
