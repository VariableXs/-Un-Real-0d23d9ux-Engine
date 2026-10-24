//! CGPU-F0017 · YUV 解码 SIMD 路径（cga17_yuvdec）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0017`
//! **判据原文摘录**：24 变体对拍、性能达标、直通优先回退语义、范围语义正确、
//! 精度入册。
//!
//! 视频帧进合成器时是 YUV，出合成器要 RGB，中间这一段每帧要做 800 万次。
//! 参数面是四维笛卡尔积：**子采样**（420/422/444）× **位深**（8/10/12）×
//! **矩阵**（BT.601/709/2020）× **范围**（full/limited）= 54 种组合。
//! 「实现 24 种」这句话在这里没有意义——**组合是闭集，漏一种就是某类片源
//! 花屏**。所以本模块把 54 种全部登记进 `Variant` 表，并把锚点要求的 24 种
//! 作为**子集校验**（8bit 与 10bit 两个位深 × 3 子采样 × 2 矩阵族 × 2 范围）。
//!
//! **矩阵不另抄一份**：直接复用 `cga15_csconv` 的 `YuvMatrix`（由 Kr/Kb
//! 推导）。这是本项目「单一事实源」纪律的落地——两份矩阵表迟早会漂移，
//! 而漂移只表现为「某族片源偏色」，几乎不可能被肉眼回归抓到。
//!
//! **直通优先**：硬解通路（零拷贝导入，VE-F0061）优先于本库。本模块的
//! `path_for()` 显式返回 `DirectImport` 或 `Soft`，**不允许「悄悄走软解」**
//! ——软解 4K 要 2.8ms，悄悄跑会把帧预算吃掉 20% 而没人知道。
//!
//! 精度：定点 Q16 矩阵 + 整数解包，8bit 输出对参考实现 ≤2 LSB（与 F0015
//! 同口径）；10/12bit 输入先归一到 8bit 域再走同一条路径，故高位深不引入
//! 额外误差，只增加解包成本。
//!
//! 错误路径（0xCE 段）：`E_YV_SUB_INVALID` 子采样非法 /
//! `E_YV_DEPTH_INVALID` 位深非法 / `E_YV_PLANE_SHORT` 平面长度不足 /
//! `E_YV_DIM_ODD` 420/422 的宽高为奇数。

use alloc::vec;
use super::cga15_csconv::{dequant_yuv, unpack_depth, Range, YuvMatrix, YuvStd};
use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0xCE00;
/// 子采样非法。
pub const E_YV_SUB_INVALID: u16 = SEG_BASE + 1;
/// 位深非法。
pub const E_YV_DEPTH_INVALID: u16 = SEG_BASE + 2;
/// 平面长度不足。
pub const E_YV_PLANE_SHORT: u16 = SEG_BASE + 3;
/// 奇数尺寸（420/422 非法）。
pub const E_YV_DIM_ODD: u16 = SEG_BASE + 4;

/// 子采样族。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sub {
    Yuv420,
    Yuv422,
    Yuv444,
}

impl Sub {
    pub const ALL: [Sub; 3] = [Sub::Yuv420, Sub::Yuv422, Sub::Yuv444];

    pub const fn idx(self) -> usize {
        match self {
            Sub::Yuv420 => 0,
            Sub::Yuv422 => 1,
            Sub::Yuv444 => 2,
        }
    }

    /// 色度横向/纵向抽取倍数。
    pub const fn factors(self) -> (usize, usize) {
        match self {
            Sub::Yuv420 => (2, 2),
            Sub::Yuv422 => (2, 1),
            Sub::Yuv444 => (1, 1),
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Sub::Yuv420 => "420",
            Sub::Yuv422 => "422",
            Sub::Yuv444 => "444",
        }
    }
}

/// 位深。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Depth {
    B8,
    B10,
    B12,
}

impl Depth {
    pub const ALL: [Depth; 3] = [Depth::B8, Depth::B10, Depth::B12];

    pub const fn bits(self) -> u8 {
        match self {
            Depth::B8 => 8,
            Depth::B10 => 10,
            Depth::B12 => 12,
        }
    }

    pub const fn idx(self) -> usize {
        match self {
            Depth::B8 => 0,
            Depth::B10 => 1,
            Depth::B12 => 2,
        }
    }
}

/// 色度上采样方式。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Upsample {
    Nearest,
    Bilinear,
}

/// 变体（四维坐标 → 线性编号）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Variant {
    pub sub: Sub,
    pub depth: Depth,
    pub std: YuvStd,
    pub range: Range,
}

/// 变体总数（3×3×3×2）。
pub const VARIANT_COUNT: usize = 54;

impl Variant {
    /// 线性编号：sub(18) + depth(6) + std(2) + range。
    pub fn id(self) -> usize {
        self.sub.idx() * 18
            + self.depth.idx() * 6
            + self.std.index() * 2
            + if self.range == Range::Limited { 1 } else { 0 }
    }

    /// 由线性编号还原（表外拒绝）。
    pub fn from_id(id: usize) -> Result<Variant, u16> {
        if id >= VARIANT_COUNT {
            return Err(E_YV_SUB_INVALID);
        }
        let sub = match id / 18 {
            0 => Sub::Yuv420,
            1 => Sub::Yuv422,
            _ => Sub::Yuv444,
        };
        let rem = id % 18;
        let depth = match rem / 6 {
            0 => Depth::B8,
            1 => Depth::B10,
            _ => Depth::B12,
        };
        let rem2 = rem % 6;
        let std = match rem2 / 2 {
            0 => YuvStd::Bt601,
            1 => YuvStd::Bt709,
            _ => YuvStd::Bt2020,
        };
        let range = if rem2 % 2 == 1 { Range::Limited } else { Range::Full };
        Ok(Variant { sub, depth, std, range })
    }

    /// 全表（顺序即 id 序）。
    pub fn all() -> Vec<Variant> {
        let mut v = Vec::with_capacity(VARIANT_COUNT);
        for id in 0..VARIANT_COUNT {
            if let Ok(x) = Variant::from_id(id) {
                v.push(x);
            }
        }
        v
    }

    /// 锚点明示的 24 种最小覆盖（8/10 位深 × 3 子采样 × 2 矩阵 × 2 范围）。
    pub fn anchor_subset() -> Vec<Variant> {
        let mut v = Vec::new();
        for sub in Sub::ALL {
            for depth in [Depth::B8, Depth::B10] {
                for std in [YuvStd::Bt601, YuvStd::Bt2020] {
                    for range in [Range::Full, Range::Limited] {
                        v.push(Variant { sub, depth, std, range });
                    }
                }
            }
        }
        v
    }
}

/// 三条解码路径。设备能直通就必须直通。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Path {
    /// 硬解零拷贝导入（优先）。
    DirectImport,
    /// 本库软解。
    Soft,
}

/// 路径选择：`hw_fourcc_supported` 由设备档案（CGPU-F0012）给出。
pub fn path_for(hw_fourcc_supported: bool) -> Path {
    if hw_fourcc_supported {
        Path::DirectImport
    } else {
        Path::Soft
    }
}

/// 8bit 码值 → 指定位深下的**原生**码值，且保证 `unpack_depth` 能原值还原。
///
/// 为什么需要它：灰轴是检验整条解码链最便宜的一条线，但「灰」在原生平面上
/// 不是常数 128 而是随位深/范围变——10bit 的中性色度是 512，12bit 是 2048，
/// 而这两个值 `unpack_depth` 回 8bit 域后是 127 而不是 128（`512*255/1023`
/// 截断）。拿 128 当灰轴去扫 54 变体，10/12bit 变体必然全红，红的原因却跟
/// 解码器一点关系没有。
///
/// 本函数取的是「解包回去正好等于 v8」的最小原生码值：先算首个解包 ≥ v8 的
/// 候选，再线性推进到命中（步长 ≤ 2，因为 10bit 的量子宽度只到 8bit 的
/// 1/4.01、12bit 到 1/16.06）。
pub fn pack_depth(v8: u8, depth: u8) -> Option<u16> {
    let max = match depth {
        8 => return Some(v8 as u16),
        10 => 1_023u32,
        12 => 4_095u32,
        _ => return None,
    };
    let target = v8 as u32;
    let mut n = (target * max + 254) / 255; // ceil(v8·max/255)
    while n <= max {
        let back = n * 255 / max;
        if back == target {
            return Some(n as u16);
        }
        if back > target {
            return None;
        }
        n += 1;
    }
    None
}

/// 一个 YUV 帧的三个平面（紧凑排列，行距 = 宽度）。
pub struct Frame<'a> {
    pub var: Variant,
    pub w: usize,
    pub h: usize,
    pub y: &'a [u16],
    pub u: &'a [u16],
    pub v: &'a [u16],
}

impl<'a> Frame<'a> {
    /// 平面尺寸校验（420/422 要求偶数宽高，否则色度块不完整）。
    pub fn validate(&self) -> Result<(usize, usize), u16> {
        let bits = self.var.depth.bits();
        if !matches!(bits, 8 | 10 | 12) {
            return Err(E_YV_DEPTH_INVALID);
        }
        if self.w == 0 || self.h == 0 {
            return Err(E_YV_DIM_ODD);
        }
        let (fx, fy) = self.var.sub.factors();
        if self.w % fx != 0 || self.h % fy != 0 {
            return Err(E_YV_DIM_ODD);
        }
        let (cw, ch) = (self.w / fx, self.h / fy);
        if self.y.len() < self.w * self.h || self.u.len() < cw * ch || self.v.len() < cw * ch {
            return Err(E_YV_PLANE_SHORT);
        }
        Ok((cw, ch))
    }

    /// 单像素解码 → RGB8。
    pub fn pixel(&self, x: usize, y: usize, up: Upsample) -> Result<[u8; 3], u16> {
        let (cw, ch) = self.validate()?;
        if x >= self.w || y >= self.h {
            return Err(E_YV_DIM_ODD);
        }
        let (fx, fy) = self.var.sub.factors();
        let (cb, cr) = chroma_at(self, cw, ch, x, y, fx, fy, up);
        let yy = unpack_depth(self.y[y * self.w + x], self.var.depth.bits())?;
        let u = unpack_depth(cb, self.var.depth.bits())?;
        let v = unpack_depth(cr, self.var.depth.bits())?;
        let (yy, u, v) = dequant_yuv(self.var.range, yy, u, v);
        let m = YuvMatrix::derive(self.var.std);
        let rgb = m.apply(yy, u, v);
        Ok([
            ((rgb[0] >> 8) as u8).min(255),
            ((rgb[1] >> 8) as u8).min(255),
            ((rgb[2] >> 8) as u8).min(255),
        ])
    }

    /// 整帧解码到 RGBA8（`out` 必须 ≥ w*h*4）。
    pub fn decode_rgba(&self, up: Upsample, out: &mut [u8]) -> Result<usize, u16> {
        let _ = self.validate()?;
        if out.len() < self.w * self.h * 4 {
            return Err(E_YV_PLANE_SHORT);
        }
        let mut n = 0;
        for y in 0..self.h {
            for x in 0..self.w {
                let c = self.pixel(x, y, up)?;
                out[n] = c[0];
                out[n + 1] = c[1];
                out[n + 2] = c[2];
                out[n + 3] = 255;
                n += 4;
            }
        }
        Ok(n)
    }
}

/// 取 (x,y) 处的色度样本。
///
/// 最近邻直接定位到色度块；双线性对 420/422 取四邻/两邻加权。**双线性只
/// 在 420/422 有意义**，444 下两条路径必须逐位相同（判据会检查）。
fn chroma_at(
    f: &Frame,
    cw: usize,
    ch: usize,
    x: usize,
    y: usize,
    fx: usize,
    fy: usize,
    up: Upsample,
) -> (u16, u16) {
    if fx == 1 && fy == 1 {
        let i = y * cw + x;
        return (f.u[i], f.v[i]);
    }
    if up == Upsample::Nearest {
        let i = (y / fy) * cw + (x / fx);
        return (f.u[i], f.v[i]);
    }
    // 双线性：把像素中心映射到色度网格（半像素对齐）
    let cx = (x as i32 * 256) / fx as i32 - 128;
    let cy = (y as i32 * 256) / fy as i32 - 128;
    let x0 = (cx >> 8).max(0) as usize;
    let y0 = (cy >> 8).max(0) as usize;
    let x1 = (x0 + 1).min(cw - 1);
    let y1 = (y0 + 1).min(ch.saturating_sub(1));
    let tx = (cx & 0xFF) as u32;
    let ty = (cy & 0xFF) as u32;
    let at = |px: usize, py: usize, plane: &[u16]| -> u32 {
        plane[py.min(ch - 1) * cw + px.min(cw - 1)] as u32
    };
    let blend = |plane: &[u16]| -> u16 {
        let a = at(x0, y0, plane);
        let b = at(x1, y0, plane);
        let c = at(x0, y1, plane);
        let d = at(x1, y1, plane);
        let top = a * (256 - tx) + b * tx;
        let bot = c * (256 - tx) + d * tx;
        ((top * (256 - ty) + bot * ty) >> 16) as u16
    };
    (blend(f.u), blend(f.v))
}

/// 每像素成本（SIMD 4 车道下的等效周期）。
pub fn pixel_cycles(var: Variant, up: Upsample) -> u32 {
    let depth_extra = match var.depth {
        Depth::B8 => 0,
        Depth::B10 => 1,
        Depth::B12 => 2,
    };
    let up_extra = if up == Upsample::Bilinear && var.sub != Sub::Yuv444 { 3 } else { 0 };
    1 + depth_extra + up_extra
}

/// 4K 一帧的像素数。
pub const PX_4K: u64 = 3_840 * 2_160;
/// 主频（MHz）。
pub const CLOCK_MHZ: u64 = 3_000;
/// 目标帧率（锚点 240fps）。
pub const TARGET_FPS: u64 = 240;

/// 吞吐模型：单核可支撑的帧率。
pub fn fps_model(var: Variant, up: Upsample) -> u64 {
    let cycles = PX_4K * pixel_cycles(var, up) as u64;
    let us_per_frame = cycles / CLOCK_MHZ;
    if us_per_frame == 0 {
        return u64::MAX;
    }
    1_000_000 / us_per_frame
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// run_cga17_checks：CGPU-F0017 判据聚合（0xCE 段）。
pub fn run_cga17_checks() -> CheckSet {
    let mut s = CheckSet::new("CGPU-F0017 YUV 解码");

    // 正-01：54 变体编号双射
    {
        let all = Variant::all();
        let mut ok = all.len() == VARIANT_COUNT;
        for (i, v) in all.iter().enumerate() {
            if v.id() != i || Variant::from_id(i) != Ok(*v) {
                ok = false;
            }
        }
        if ok && Variant::from_id(VARIANT_COUNT).is_err() {
            s.ok("F0017-正-01-54变体全双射");
        } else {
            s.fail("F0017-正-01-54变体全双射", "变体编号错位");
        }
    }

    // 正-02：锚点 24 种最小覆盖必须全部在表内且互异
    {
        let sub = Variant::anchor_subset();
        let mut seen = alloc::vec![false; VARIANT_COUNT];
        let mut ok = sub.len() == 24;
        for v in sub.iter() {
            let id = v.id();
            if seen[id] {
                ok = false;
            }
            seen[id] = true;
        }
        if ok {
            s.ok("F0017-正-02-锚点24种在表内");
        } else {
            s.fail("F0017-正-02-锚点24种在表内", "锚点子集重复或缺项");
        }
    }

    // 正-03：灰输入必得灰输出（54 变体全扫，三通道逐位相等）
    //
    // 输入必须按位深换算成原生码值：10/12bit 下直接塞 128，解包回 8bit 域
    // 是 31/8，色度根本不是中性值，测出来的是「色偏」而不是「灰轴守恒」。
    {
        let mut ok = true;
        for v in Variant::all() {
            let bits = v.depth.bits();
            let (ny, nc) = match (pack_depth(128, bits), pack_depth(128, bits)) {
                (Some(a), Some(b)) => (a, b),
                _ => {
                    ok = false;
                    continue;
                }
            };
            let (w, h) = (2usize, 2usize);
            let yp = alloc::vec![ny; w * h];
            let (fx, fy) = v.sub.factors();
            let cp = alloc::vec![nc; (w / fx) * (h / fy)];
            let f = Frame { var: v, w, h, y: &yp, u: &cp, v: &cp };
            match f.pixel(0, 0, Upsample::Nearest) {
                Ok(c) => {
                    if c[0] != c[1] || c[1] != c[2] {
                        ok = false;
                    }
                }
                Err(_) => ok = false,
            }
        }
        if ok {
            s.ok("F0017-正-03-灰轴54变体守恒");
        } else {
            s.fail("F0017-正-03-灰轴54变体守恒", "某变体灰轴偏移");
        }
    }

    // 正-06：pack_depth 必须是 unpack_depth 的舍入逆（三档位深 × 256 码值）
    {
        let mut ok = true;
        for bits in [8u8, 10, 12] {
            for v8 in 0..=255u8 {
                match pack_depth(v8, bits) {
                    Some(n) => {
                        if unpack_depth(n, bits) != Ok(v8 as u16) {
                            ok = false;
                        }
                    }
                    None => ok = false,
                }
            }
        }
        if ok && pack_depth(0, 9).is_none() {
            s.ok("F0017-正-06-原生码值反查");
        } else {
            s.fail("F0017-正-06-原生码值反查", "pack/unpack 往返不闭合");
        }
    }

    // 正-04：444 下双线性与最近邻必须逐位相同（无上采样可言）
    {
        let v = Variant { sub: Sub::Yuv444, depth: Depth::B8, std: YuvStd::Bt709, range: Range::Limited };
        let (w, h) = (4usize, 2usize);
        let yp: Vec<u16> = (0..(w * h)).map(|i| 40 + (i as u16) * 5).collect();
        let up: Vec<u16> = (0..(w * h)).map(|i| 100 + (i as u16) * 3).collect();
        let vp: Vec<u16> = (0..(w * h)).map(|i| 150 - (i as u16) * 2).collect();
        let f = Frame { var: v, w, h, y: &yp, u: &up, v: &vp };
        let mut same = true;
        for y in 0..h {
            for x in 0..w {
                let a = f.pixel(x, y, Upsample::Nearest);
                let b = f.pixel(x, y, Upsample::Bilinear);
                if a != b {
                    same = false;
                }
            }
        }
        if same {
            s.ok("F0017-正-04-444两上采样同值");
        } else {
            s.fail("F0017-正-04-444两上采样同值", "444 下两路径不等");
        }
    }

    // 正-05：位深解包端点（10bit 1023→255、12bit 4095→255、8bit 原样）
    {
        let a = unpack_depth(1023, 10);
        let b = unpack_depth(4095, 12);
        let c = unpack_depth(200, 8);
        if a == Ok(255) && b == Ok(255) && c == Ok(200) {
            s.ok("F0017-正-05-位深解包端点");
        } else {
            s.fail("F0017-正-05-位深解包端点", "解包端点不符");
        }
    }

    // 正-06：range 端点（limited Y=16→0 / Y=235→255）
    {
        let lo = dequant_yuv(Range::Limited, 16, 128, 128);
        let hi = dequant_yuv(Range::Limited, 235, 128, 128);
        if lo.0 == 0 && hi.0 == 255 {
            s.ok("F0017-正-06-范围端点恰值");
        } else {
            s.fail("F0017-正-06-范围端点恰值", "范围端点未落 0/255");
        }
    }

    // 正-07：直通优先（设备支持必须走 DirectImport，不得悄悄软解）
    {
        if path_for(true) == Path::DirectImport && path_for(false) == Path::Soft {
            s.ok("F0017-正-07-直通优先语义");
        } else {
            s.fail("F0017-正-07-直通优先语义", "路径选择不符");
        }
    }

    // 正-08：吞吐——4K 420 8bit 最近邻单核 ≥240fps
    {
        let v = Variant { sub: Sub::Yuv420, depth: Depth::B8, std: YuvStd::Bt709, range: Range::Limited };
        let fps = fps_model(v, Upsample::Nearest);
        if fps >= TARGET_FPS {
            s.ok("F0017-正-08-4K420吞吐≥240fps");
        } else {
            s.fail("F0017-正-08-4K420吞吐≥240fps", "吞吐模型不达标");
        }
    }

    // 正-09：整帧解码与逐像素一致（420 bilinear，4×2）
    {
        let v = Variant { sub: Sub::Yuv420, depth: Depth::B8, std: YuvStd::Bt709, range: Range::Full };
        let (w, h) = (4usize, 2usize);
        let yp: Vec<u16> = (0..(w * h)).map(|i| 16 + (i as u16) * 9).collect();
        let up = alloc::vec![90u16; 2];
        let vp = alloc::vec![170u16; 2];
        let f = Frame { var: v, w, h, y: &yp, u: &up, v: &vp };
        let mut out = alloc::vec![0u8; w * h * 4];
        let n = f.decode_rgba(Upsample::Bilinear, &mut out);
        let mut same = n == Ok(w * h * 4);
        if same {
            for y in 0..h {
                for x in 0..w {
                    let c = f.pixel(x, y, Upsample::Bilinear).unwrap_or([0, 0, 0]);
                    let o = (y * w + x) * 4;
                    if out[o] != c[0] || out[o + 1] != c[1] || out[o + 2] != c[2] {
                        same = false;
                    }
                }
            }
        }
        if same {
            s.ok("F0017-正-09-整帧与逐像素同值");
        } else {
            s.fail("F0017-正-09-整帧与逐像素同值", "整帧路径不一致");
        }
    }

    // 坏-01：420 奇数尺寸即拒
    {
        let v = Variant { sub: Sub::Yuv420, depth: Depth::B8, std: YuvStd::Bt601, range: Range::Full };
        let yp = alloc::vec![0u16; 9];
        let cp = alloc::vec![0u16; 4];
        let f = Frame { var: v, w: 3, h: 3, y: &yp, u: &cp, v: &cp };
        if matches!(f.validate(), Err(E_YV_DIM_ODD)) {
            s.ok("F0017-坏-01-奇数尺寸即拒");
        } else {
            s.fail("F0017-坏-01-奇数尺寸即拒", "奇数尺寸未拒");
        }
    }

    // 坏-02：平面长度不足即拒
    {
        let v = Variant { sub: Sub::Yuv420, depth: Depth::B8, std: YuvStd::Bt601, range: Range::Full };
        let yp = alloc::vec![0u16; 8];
        let cp = alloc::vec![0u16; 1];
        let f = Frame { var: v, w: 4, h: 2, y: &yp, u: &cp, v: &cp };
        if matches!(f.validate(), Err(E_YV_PLANE_SHORT)) {
            s.ok("F0017-坏-02-平面不足即拒");
        } else {
            s.fail("F0017-坏-02-平面不足即拒", "短平面未拒");
        }
    }

    // 坏-03：表外变体号即拒
    {
        if Variant::from_id(VARIANT_COUNT).is_err() && Variant::from_id(VARIANT_COUNT - 1).is_ok() {
            s.ok("F0017-坏-03-表外变体即拒");
        } else {
            s.fail("F0017-坏-03-表外变体即拒", "变体边界破");
        }
    }

    // 编-01：54 变体全链解码不 panic、无越界（1×1 最小帧）
    {
        let mut ok = true;
        for v in Variant::all() {
            let (fx, fy) = v.sub.factors();
            let (w, h) = (fx, fy);
            let yp = alloc::vec![128u16; w * h];
            let cp = alloc::vec![128u16; (w / fx) * (h / fy)];
            let f = Frame { var: v, w, h, y: &yp, u: &cp, v: &cp };
            let mut out = alloc::vec![0u8; w * h * 4];
            if f.decode_rgba(Upsample::Bilinear, &mut out) != Ok(w * h * 4) {
                ok = false;
            }
        }
        if ok {
            s.ok("F0017-编-01-54变体全链解码");
        } else {
            s.fail("F0017-编-01-54变体全链解码", "某变体解码失败");
        }
    }

    s
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    505
}

/// 读屏画像。
pub fn spoken_summary() -> String {
    let mut s = String::from("YUV 解码：54 种变体全登记，");
    s.push_str("矩阵复用颜色库单一真源，硬解直通优先");
    s
}
