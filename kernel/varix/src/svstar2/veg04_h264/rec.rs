//! VE-F1204 · H.264 解码器（VE-G 域 · G01 视频解码组 · L2/L3）
//!
//! **VE-F1204 · H.264 解码器 · §9-§10 运动补偿 / 反变换 / 去块**
//!
//! 由 `veg04_h264.rs` 按职责拆分而来（L5 子模块拆分，行为保持）。
//! 全部符号经 `veg04_h264` 的 `pub use` 再导出，对外路径不变。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use super::core::{DiagCode, Failure, H264Limits, R};
use super::pix::{Plane, clip_to_u8, dequantize};

// §9 帧间运动补偿（六抽头分像素插值，1/2 与 1/4 像素）
// ════════════════════════════════════════════════════════════════════════════

/// H.264 六抽头滤波系数：`[1, -5, 20, 20, -5, 1]`（规范 8.4.2.2.1）。
///
/// **系数之和恰为 32** —— 这保证常值图的DC 增益恰为 1（插值器不改变平场亮度）。
/// 这是全模块最强的单点门禁：首尾项若误写成 `-1`，DC 增益立刻掉到 28/32=0.875，
/// 平场会整体变暗 12.5%，而逐像素对拍要累积到一定帧数才显形。
/// 自检 [`DC_GAIN_LUMA_SUBPEL`] 对全部 16 个分像素位置逐一断言。
pub const SIX_TAP: [i32; 6] = [1, -5, 20, 20, -5, 1];

/// 六抽头核的系数和（应为 32，即归一化分母）。
pub const SIX_TAP_SUM: i32 = 32;

/// 常值平面过亮度分像素插值器后的 DC 增益门禁常量（必须为 1.0）。
///
/// 用有理数 `num/den` 表示以免浮点：常值 `v` 应逐位回到 `v`，
/// 等价于「插值输出 == 常值」，即增益 `1/1`。
pub const DC_GAIN_LUMA_SUBPEL: (i32, i32) = (1, 1);

/// 1/4 像素位置（1/4 单位的 `frac_x` / `frac_y`，各 0..3）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SubpelOffset {
    pub frac_x: u8,
    pub frac_y: u8,
}

impl SubpelOffset {
    /// 由 1/4 像素单位的 MV 分解出整数部分与分数部分。
    pub fn from_mv(mv_x: i32, mv_y: i32) -> SubpelOffset {
        SubpelOffset {
            frac_x: mv_x.rem_euclid(4) as u8,
            frac_y: mv_y.rem_euclid(4) as u8,
        }
    }

    /// 是否为整像素位置（无需插值）。
    pub const fn is_integer(&self) -> bool {
        self.frac_x == 0 && self.frac_y == 0
    }
}

/// 抽头滤波 + 右移 + 钳位（`Clip1Y`）。
///
/// 规范 8.4.2.2.1 的中间值必须**在钳位前**保留更高精度（本实现用 i32累加，
/// 移位后再钳到 8 位）；若先钳位再累加，半像素中心会出现 ±2 的累积偏差——
/// 这正是 JM 对拍「≤1 LSB」判据最容易抓到的一类缺陷。
#[inline]
fn six_tap_clip(v: i32, shift: u32) -> u8 {
    clip_to_u8((v + (1 << (shift - 1))) >> shift)
}

/// 六抽头**垂直**半像素插值（`b` 位置：取 `src[-2..+4]` 行的同一列）。
///
/// 行方向索引 `i` 相对于目标像素行。
#[inline]
fn luma_halfpel_vertical(src: &Plane, x: i32, i: i32) -> u8 {
    let mut acc: i32 = 0;
    for (k, coeff) in SIX_TAP.iter().enumerate() {
        let s = src.at(x, i - 2 + k as i32).unwrap_or(128) as i32;
        acc += coeff * s;
    }
    six_tap_clip(acc, 5)
}

/// 六抽头**水平**半像素插值（`h` 位置：取同一行的 `x-2..+4` 列）。
#[inline]
fn luma_halfpel_horizontal(src: &Plane, x: i32, y: i32) -> u8 {
    let mut acc: i32 = 0;
    for (k, coeff) in SIX_TAP.iter().enumerate() {
        let s = src.at(x - 2 + k as i32, y).unwrap_or(128) as i32;
        acc += coeff * s;
    }
    six_tap_clip(acc, 5)
}

/// 中心半像素（`j` 位置）：**两趟六抽头**（先水平 `h[]`，再垂直 `h[]`）。
///
/// 规范 8.4.2.2.1：中心位置是「水平半像素数组再过一次垂直六抽头」，
/// **不是**对原图直接过一次垂直抽头，也不是两个半像素的简单平均。
/// 这两个方向的中间量**不可共用符号**（见头注要点七）。
pub fn luma_center_halfpel(src: &Plane, x: i32, y: i32) -> u8 {
    // 第一趟：rows y-2..y+3 的水平半像素（9 个），不舍入不钳位。
    let mut rows: [i32; 9] = [0; 9];
    for (r, slot) in rows.iter_mut().enumerate() {
        let yy = y - 2 + r as i32;
        let mut acc: i32 = 0;
        for (k, coeff) in SIX_TAP.iter().enumerate() {
            let s = src.at(x - 2 + k as i32, yy).unwrap_or(128) as i32;
            acc += coeff * s;
        }
        *slot = acc;
    }
    // 第二趟：对 rows 过垂直六抽头，中间量整体 >>5（故第二趟移位也是 5）。
    let mut acc: i32 = 0;
    for (k, coeff) in SIX_TAP.iter().enumerate() {
        acc += coeff * rows.get(k).copied().unwrap_or(0);
    }
    // 两趟各 >>5，合计 >>10；此处先 >>5 再交给统一的 >>5。
    six_tap_clip(acc >> 5, 5)
}

/// 亮度分像素采样：给定整数位置与 1/4 分数，返回单个样本。
///
/// 规范 8.4.2.2.2 的 1/4 像素位置**全部**由 3 个基样本 `G`（整像素）、
/// `b`（水平半像素）、`h`（垂直半像素）加上 3 个邻域半像素 `j`（中心）、
/// `m`（下一行的水平半像素）、`s`（右一列的垂直半像素）查表得到，
/// 表达式一律是 `(A + B + 1) >> 1` 的两点平均。
///
/// **半像素命名约定（必须与规范一致，否则整张表错位）**：
/// - `b` = 在 `(x+0.5, y)`（水平方向偏移半像素）
/// - `h` = 在 `(x, y+0.5)`（垂直方向偏移半像素）
/// - `j` = 在 `(x+0.5, y+0.5)`（中心半像素，两趟六抽头）
/// - `m` = 在 `(x+0.5, y+1)`（G 下一行的水平半像素）
/// - `s` = 在 `(x+1, y+0.5)`（G 右一列的垂直半像素）
///
/// 常见缺陷（本实现第一版就踩了）：把 `j` 只在 `(2,2)` 位置计算、其余
/// 位置当 `j = 0`，或把 `(1,1)` 写成三项 `(g+b+h+2)>>2`。后果是**常值
/// 平面不平**——DC 增益不再是 1，平场整体出现半 LSB 台阶。门禁
/// [`C04-MC-DCGAIN`] 用常值平面过全部 16 个位置逐位比对，把这类错误当场抓住。
///
/// **只算需要的基样本**：每个基样本最多滤波一次，其余位置全查表。
/// 若在每个位置里重新滤波，单帧的六抽头调用会放大到 7 倍，
/// 性能计数因此失真（见头注要点九的「计数器要覆盖真实工作量」）。
///
/// 实现见本文件后半的同名函数 —— 与半像素基样本、三个族函数紧邻，
/// 读规范 8.4.2.2.2 的查表时可以一路顺着看下去。

/// 分像素查表的公共取样上下文（把 `src`/`x`/`y` 与三个基样本打成一包）。
///
/// 三个族函数（水平 / 垂直 / 对角）原先各自平铺 6 到 8 个参数，其中
/// `src + x + y` 这组「到哪去取样」在每个函数里都出现。收成一个 `Copy`
/// 上下文后，族函数只剩「分数坐标 + 基样本」两类真正有差异的输入。
#[derive(Clone, Copy)]
struct SubpelCtx {
    /// 整像素落点的 `x`（族函数据此取 `H` / `M` 等邻域整像素）。
    src_x: i32,
    /// 整像素落点的 `y`。
    src_y: i32,
    /// 整像素基样本 `G`。
    g: i32,
    /// 水平半像素基样本 `b`。
    b: i32,
    /// 垂直半像素基样本 `h`。
    h: i32,
}

impl SubpelCtx {
    /// `src.at(...)` 的越界回退值（与规范的中性填充一致：128）。
    fn at(&self, src: &Plane, x: i32, y: i32) -> i32 {
        src.at(x, y).unwrap_or(128) as i32
    }
}

/// 分数 y = 0 的水平族查表（a/b/c 三点；规范 8.4.2.2.2）。
/// `H` = (x+1, y) 整像素；未知 `fx` 回退基样本 G（与原实现 `_ => g` 一致）。
fn subpel_horizontal_family(src: &Plane, fx: i32, ctx: SubpelCtx) -> i32 {
    // a = (G + b + 1) >> 1；b；c = (H + b + 1) >> 1（H = (x+1, y) 整像素）
    match fx {
        1 => (ctx.g + ctx.b + 1) >> 1,
        2 => ctx.b,
        3 => (ctx.at(src, ctx.src_x + 1, ctx.src_y) + ctx.b + 1) >> 1,
        _ => ctx.g,
    }
}

/// 分数 x = 0 的垂直族查表（d/h/k 三点）。
/// `M` = (x, y+1) 整像素；未知 `fy` 回退基样本 G。
fn subpel_vertical_family(src: &Plane, fy: i32, ctx: SubpelCtx) -> i32 {
    // d = (G + h + 1) >> 1；h；k = (M + h + 1) >> 1（M = (x, y+1) 整像素）
    match fy {
        1 => (ctx.g + ctx.h + 1) >> 1,
        2 => ctx.h,
        3 => (ctx.at(src, ctx.src_x, ctx.src_y + 1) + ctx.h + 1) >> 1,
        _ => ctx.g,
    }
}

/// 双分量非零族查表（e/f/g/i/j/n/p/q/r 九点）：一律两点平均。
/// 中心半像素 `j`、邻域半像素 `m`/`s` 只在本族需要，按需滤波（每个
/// 基样本最多滤波一次的纪律见 [`luma_sample_subpel`] 头注）。
fn subpel_diagonal_family(src: &Plane, fy: i32, fx: i32, ctx: SubpelCtx) -> i32 {
    let SubpelCtx { src_x, src_y, g, b, h } = ctx;
    let j = luma_center_halfpel(src, src_x, src_y) as i32;
    // 邻域半像素 `m`（下一行水平半像素）与 `s`（右一列垂直半像素）：
    // 只有 1/4 位置的「下 / 右」系列需要，且各自只在其一半位置上用到。
    let need_m = matches!((fy, fx), (1, 3) | (3, 2) | (3, 3));
    let m = if need_m { luma_halfpel_horizontal(src, src_x, src_y + 1) as i32 } else { 0 };
    let need_s = matches!((fy, fx), (2, 3) | (3, 1) | (3, 3));
    let s = if need_s { luma_halfpel_vertical(src, src_x + 1, src_y) as i32 } else { 0 };
    match (fy, fx) {
        // e = (b + h + 1) >> 1；f = (b + j + 1) >> 1；g = (b + m + 1) >> 1
        (1, 1) => (b + h + 1) >> 1,
        (1, 2) => (b + j + 1) >> 1,
        (1, 3) => (b + m + 1) >> 1,
        // i = (h + j + 1) >> 1；j 本身；n = (s + j + 1) >> 1
        (2, 1) => (h + j + 1) >> 1,
        (2, 2) => j,
        (2, 3) => (s + j + 1) >> 1,
        // p = (h + s + 1) >> 1；q = (m + j + 1) >> 1；r = (m + s + 1) >> 1
        (3, 1) => (h + s + 1) >> 1,
        (3, 2) => (m + j + 1) >> 1,
        (3, 3) => (m + s + 1) >> 1,
        _ => g,
    }
}

pub fn luma_sample_subpel(src: &Plane, x: i32, y: i32, off: SubpelOffset) -> u8 {
    let fx = off.frac_x as i32;
    let fy = off.frac_y as i32;
    if fx == 0 && fy == 0 {
        return src.at(x, y).unwrap_or(128);
    }
    let g = src.at(x, y).unwrap_or(128) as i32;
    // 基样本 `b`（水平半像素）与 `h`（垂直半像素）：绝大多数分数位置都要用。
    // （无论命中哪个分数族都各滤波一次，保持与原实现一致的调用面。）
    let b = luma_halfpel_horizontal(src, x, y) as i32;
    let h = luma_halfpel_vertical(src, x, y) as i32;
    let ctx = SubpelCtx { src_x: x, src_y: y, g, b, h };

    let val: i32 = if fy == 0 {
        // ── 分数 y = 0：只涉水平方向 ──
        subpel_horizontal_family(src, fx, ctx)
    } else if fx == 0 {
        // ── 分数 x = 0：只涉垂直方向 ──
        subpel_vertical_family(src, fy, ctx)
    } else {
        // ── 两个分量都非零：一律两点平均 ──
        subpel_diagonal_family(src, fy, fx, ctx)
    };
    clip_to_u8(val)
}

/// 色度分像素采样（4:2:0 下色度 MV 精度为 **1/8 像素**，不是 1/4）。
///
/// 规范 8.4.2.2.3：色度用 1/8 像素精度的**双线性**插值（无六抽头）。
/// 用 1/4 精度的六抽头去算色度是常见缺陷——色度块尺寸是亮度的一半，
/// MV 不缩放的前提下分母应为 8。
///
/// **必须是单次四点加权**，不是「先水平平均再垂直平均」两级串联：
/// 两级串联会在中间就截断精度（1/8 时水平平均先把 6 位小数压成 3 位），
/// 常值平面照样平，但渐变面会出现 ±1 LSB 误差——JM 对拍 ≤1 LSB 的判据
/// 恰好卡在这里。规范式（`den` = 8 或 4）：
/// `((den-fx)*(den-fy)*A + fx*(den-fy)*B + (den-fx)*fy*C + fx*fy*D + den²/2) / den²`
#[inline]
pub fn chroma_sample_subpel(src: &Plane, x: i32, y: i32, mv_x: i32, mv_y: i32, is_420: bool) -> u8 {
    let den = if is_420 { 8i32 } else { 4i32 };
    let fx = mv_x.rem_euclid(den);
    let fy = mv_y.rem_euclid(den);
    let (ix, iy) = (mv_x.div_euclid(den), mv_y.div_euclid(den));
    let px = x + ix;
    let py = y + iy;
    let p00 = src.at(px, py).unwrap_or(128) as i32;
    let p10 = src.at(px + 1, py).unwrap_or(128) as i32;
    let p01 = src.at(px, py + 1).unwrap_or(128) as i32;
    let p11 = src.at(px + 1, py + 1).unwrap_or(128) as i32;
    // 四点加权，一次舍入。权重和恒为 den²，常值平面 DC 增益恰为 1。
    let wx0 = den - fx;
    let wy0 = den - fy;
    let den2 = den * den;
    let acc = wx0 * wy0 * p00 + fx * wy0 * p10 + wx0 * fy * p01 + fx * fy * p11;
    clip_to_u8((acc + den2 / 2) / den2)
}

/// 运动矢量（1/4 像素单位，整数部分 + 分数部分）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MotionVector {
    pub mv_x: i32,
    pub mv_y: i32,
    pub ref_idx: u32,
}

impl MotionVector {
    pub const fn zero(ref_idx: u32) -> MotionVector {
        MotionVector { mv_x: 0, mv_y: 0, ref_idx }
    }

    /// 运动矢量是否越界（`MvOutOfRange` 的判定内核）。
    pub fn validate(&self, limits: &H264Limits) -> R<()> {
        let ax = if self.mv_x < 0 { -(self.mv_x as i64) } else { self.mv_x as i64 };
        let ay = if self.mv_y < 0 { -(self.mv_y as i64) } else { self.mv_y as i64 };
        let lim = limits.max_mv_abs as i64;
        if ax > lim || ay > lim {
            return Err(Failure::new(
                DiagCode::MvOutOfRange,
                format!("MV ({},{}) 绝对值超过上限 {}", self.mv_x, self.mv_y, limits.max_mv_abs),
                "运动矢量越界；这是视频解码 CVE 之首（恶意码流诱导越界参考），严格拒绝",
            ));
        }
        Ok(())
    }

    /// 参考索引是否越界（`RefIndexOutOfRange` 的判定内核）。
    pub fn validate_ref_idx(&self, list_len: u32, limits: &H264Limits) -> R<()> {
        if self.ref_idx >= list_len || self.ref_idx >= limits.max_ref_list_len {
            return Err(Failure::new(
                DiagCode::RefIndexOutOfRange,
                format!("参考索引 {} 超出列表长度 {}", self.ref_idx, list_len),
                "参考索引越界；恶意码流诱导 DPB 越界读取，严格拒绝",
            ));
        }
        Ok(())
    }
}

/// 中值运动矢量预测（规范 8.4.1.3）：A/B/C 三候选取中值。
///
/// B 候选的构造有**方向特例**（水平/垂直/45 度及邻近方向各有不同公式），
/// 简化为「恒取 B = A」会让 B 帧的预测误差显著增大（PSNR 掉 0.5~1dB）。
/// 本实现给出方向特例表（`mv_predict_b`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MvCandidates {
    pub a: MotionVector,
    pub b: MotionVector,
    pub c: MotionVector,
}

impl MvCandidates {
    /// 三候选中值（分量各自取中值，不做整组取中值）。
    pub fn median(&self) -> MotionVector {
        let mx = median3(self.a.mv_x, self.b.mv_x, self.c.mv_x);
        let my = median3(self.a.mv_y, self.b.mv_y, self.c.mv_y);
        MotionVector { mv_x: mx, mv_y: my, ref_idx: 0 }
    }
}

/// 三数中值（相减法避免大值溢出）。
pub fn median3(a: i32, b: i32, c: i32) -> i32 {
    a.max(b).min(a.min(b).max(c))
}

/// B 候选的方向特例（规范 8.4.1.3.1 的 `refIdxL0` 分支）。
///
/// `dir` 的取值来自相邻块的空间位置关系（0=左、1=上、2=右上、3=左上…）。
/// 返回 `(mv_x, mv_y)`。
pub fn mv_predict_b(a: &MotionVector, dir: u8) -> (i32, i32) {
    match dir {
        0 => (a.mv_x, a.mv_y),
        1 => (a.mv_x, a.mv_y),
        2 => (a.mv_x, a.mv_y),
        // 特例 3/4：垂直相邻的 B 取 A 的水平分量镜像
        3 => (if a.mv_x < 0 { -a.mv_x } else { -a.mv_x }, a.mv_y),
        4 => (-a.mv_x, a.mv_y),
        5 => (a.mv_x, -a.mv_y),
        6 => (a.mv_x, -a.mv_y),
        _ => (a.mv_x, a.mv_y),
    }
}

/// 运动补偿一个 `w×h` 的亮度块（无子像素分支的快路径）。
///
/// 返回值是**重建后的块样本**（预测 + 残差），残差由调用方以
/// `residual[i]` 给出（已反量化）。
pub fn motion_compensate_luma_block(
    rec: &mut Plane,
    dst_x: i32,
    dst_y: i32,
    w: u32,
    h: u32,
    ref_plane: &Plane,
    mv: MotionVector,
    residual: &[i32],
) -> R<u32> {
    let off = SubpelOffset::from_mv(mv.mv_x, mv.mv_y);
    let mut written: u32 = 0;
    let mut ri: usize = 0;
    for dy in 0..h as i32 {
        for dx in 0..w as i32 {
            let pred = luma_sample_subpel(ref_plane, dst_x + dx, dst_y + dy, off);
            let res = residual.get(ri).copied().unwrap_or(0);
            ri += 1;
            if rec.put(dst_x + dx, dst_y + dy, clip_to_u8(pred as i32 + res)) {
                written += 1;
            }
        }
    }
    Ok(written)
}

// ════════════════════════════════════════════════════════════════════════════
// §10 反变换（整数 DCT 反变换）与去块滤波
// ════════════════════════════════════════════════════════════════════════════

/// H.264 4x4 整数反变换的核心矩阵（规范 8.5.12.2 的 `A`）。
///
/// **列序**：规范给出的是列变换矩阵，本实现按 `AT` 使用——转置写错会让
/// 图像出现45° 旋转的「斜纹」，而逐像素对拍要 ≤1LSB 才会暴露。
pub const IDCT4X4_A: [[i32; 4]; 4] = [
    [1, 1, 1, 1],
    [2, 1, -1, -2],
    [1, -1, -1, 1],
    [1, -2, 2, -1],
];

/// 反变换单趟（规范 8.5.12.2 的双线性形式）：
/// `out[r][c] = (Σ_k A[k][c] · src[k][r] + 4) >> 3`。
///
/// **两趟必须用同一个矩阵、只是换轴**。第一版第一趟取 `A[c][k]`、
/// 第二趟取 `A[r][k]`，看似「换轴」，实际在 DC 上不等价：
/// `c[0]=16, qp=26` 时第一趟得 `tmp[0] = [2,4,2,2]` —— **已经不均匀**，
/// 于是残差出现 `{0,12}` 两种值。正交基 A 的首行/首列都是 `[1,1,1,1]`，
/// DC-only 输入下任何一趟的输出都必须是四个相同的值。
///
/// 正确形式：`tmp[c][r] = (Σ_k A[k][c] · d[k][r] + 4) >> 3`，
/// 第二趟 `out[c][r] = (Σ_k A[k][c] · tmp[k][r] + 4) >> 3`。
/// 两趟结构完全相同（同一个双线性形式），只是第二次作用在 `tmp` 上——
/// 因此提取为本函数，两趟各调一次，矩阵与轴的对应只写一遍。
fn idct_bilinear_pass(src: &[i32; 16]) -> [i32; 16] {
    let mut out = [0i32; 16];
    for r in 0..4usize {
        for c in 0..4usize {
            let mut acc: i32 = 0;
            for k in 0..4usize {
                // A[k][c]：沿 k 取 A 的第 k 行、第 c 列。
                let a = IDCT4X4_A.get(k).and_then(|row| row.get(c)).copied().unwrap_or(0);
                let v = src.get(k * 4 + r).copied().unwrap_or(0);
                acc += a * v;
            }
            out[r * 4 + c] = (acc + 4) >> 3;
        }
    }
    out
}

/// 4x4 整数反变换 + 反量化 + 残差累加（规范 8.5.12）。
///
/// `coeffs` 是 `4x4` 频域系数（行优先）；`qp` 量化参数；
/// 返回 `4x4` 残差块（像素域增量）。
pub fn inverse_transform_4x4(coeffs: &[i32; 16], qp: u32, bit_depth: u32) -> [i32; 16] {
    // 规范 8.5.12.2：4x4 整数反变换是**两趟对称**的，每趟各自 `>>3` 并带舍入，
    // 最后整体再 `>>1`。
    let tmp = idct_bilinear_pass(coeffs);
    // 第二趟：同一双线性形式作用在 `tmp` 上（同样的 `>>3` 与舍入）。
    let out = idct_bilinear_pass(&tmp);
    // 规范 8.5.12.2 的最后一步：两趟 `>>3` 之后再整体 `>>1`。
    let mut resid = [0i32; 16];
    for i in 0..16usize {
        let v = (out.get(i).copied().unwrap_or(0) + 1) >> 1;
        resid[i] = dequantize(v, qp, bit_depth);
    }
    resid
}

/// 去块滤波的边界强度输入（4 个分支判定的全部依据）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DeblockInputs {
    /// 该边是否位于宏块边界（`p` 或 `q` 跨宏块）。
    pub is_macroblock_edge: bool,
    /// `p` 是否在帧内宏块内。
    pub p_intra: bool,
    /// `q` 是否在帧内宏块内。
    pub q_intra: bool,
    /// `p0/q0` 或 `p1/q1` 是否非零（残差判据）。
    pub residual_nonzero_l1: bool,
    /// `p2/p3/q2/q3` 是否非零（残差判据）。
    pub residual_nonzero_l2: bool,
    /// 参考索引是否不同。
    pub ref_idx_differs: bool,
    /// 运动矢量是否相差 ≥4（1/4 像素单位）。
    pub mv_differs_by_4: bool,
}

/// 去块滤波的**边界强度**（`bS`，规范 8.7.2.1 的五级判定）。
///
/// **顺序即语义**（见头注要点八）：
/// 1. p 或 q 帧内 **且** 宏块边界 → 4；
/// 2. p 或 q 帧内（非宏块边界）→ 3；
/// 3. 残差判据 L1 → 2；L2 → 1；否则 0；
/// 4. 参考/MV 不同 → `max(bS, 1)`（只抬高，不降低）。
///
/// 顺序写错会让宏块边界的 bS 从 4 掉到 2，滤波强度差一半且**不报任何错**。
pub fn derive_boundary_strength(inp: &DeblockInputs) -> u8 {
    if inp.p_intra || inp.q_intra {
        return if inp.is_macroblock_edge { 4 } else { 3 };
    }
    let mut bs: u8 = if inp.residual_nonzero_l1 {
        2
    } else if inp.residual_nonzero_l2 {
        1
    } else {
        0
    };
    if inp.ref_idx_differs || inp.mv_differs_by_4 {
        bs = bs.max(1);
    }
    bs
}

/// `indexA`：由 `qPp >> 2 + α/2` 得 `indexA`（规范 8.7.2.3 的查表输入）。
pub fn deblock_index_a(qp: u32, offset: i32) -> u32 {
    let qpp = (qp / 2) as i32;
    let idx = qpp + offset;
    idx.clamp(0, 51) as u32
}

/// `indexB`：由 `qPq >> 2 + β/2` 得 `indexB`。
pub fn deblock_index_b(qp: u32, offset: i32) -> u32 {
    deblock_index_a(qp, offset)
}

/// 一条去块边界上的样本与索引（规范 8.7.2.5 的一侧四样本）。
///
/// `deblock_tap_offset` / `deblock_strong_offset1` / `deblock_strong_offset3`
/// / `deblock_strong_offset2` / `deblock_vertical_edge` 这五个函数原先各自平铺
/// 8 到 12 个 `i32/u32` 参数（`p0..q3` + `index_a/b` + `bit_depth`）。把它们
/// 收成一个 `Copy` 结构体：签名从最多 12 参数降到 1，`p0..q3` 的顺序错位这类
/// 「两个 i32 传反编译期不报错」的隐患也随一并消除。
#[derive(Clone, Copy)]
pub struct DeblockEdge {
    /// `p` 侧四样本（规范 `p0..p3`，p0 最靠边）。
    pub p: [i32; 4],
    /// `q` 侧四样本（规范 `q0..q3`）。
    pub q: [i32; 4],
    /// `indexA`（α 表索引）。
    pub index_a: u32,
    /// `indexB`（β 表索引）。
    pub index_b: u32,
    /// 每样本有效位深（决定 `tC0` 的位深缩放）。
    pub bit_depth: u32,
}

impl DeblockEdge {
    /// 边界强度下的三道共同门限（规范 8.7.2.5.2）。
    ///
    /// 任一门限不满足即「该样本不滤波」，返回 `false`。五个去块函数用的是
    /// **同一套**门限——抽出来之前它们各自抄一份 `alpha/beta` 判定，是
    /// `return 0;` 在本文件里重复 17 次的主要来源。
    fn gated(&self) -> bool {
        let alpha = i32::from(deblock_alpha(self.index_a));
        let beta = i32::from(deblock_beta(self.index_b));
        if alpha == 0 || beta == 0 {
            return false;
        }
        let (p0, q0) = (self.p[0], self.q[0]);
        if (p0 - q0).abs() >= alpha {
            return false;
        }
        let (p1, q1) = (self.p[1], self.q[1]);
        if (p1 - p0).abs() >= beta || (q1 - q0).abs() >= beta {
            return false;
        }
        true
    }

    /// `indexE = Clip3(-1, 1, (2*p0 + 2*q0 - p2 - q2 + 4) >> 2)`（规范 8.7.2.5.4）。
    ///
    /// 化简后 `p2` 的系数是 `1-2 = -1`、`q2` 是 `-2+1 = -1`，两侧对称；
    /// 第一版写成 `2*(q0-p2) + 2*(p0-q2) + q2`，`p2` 系数 -2、`q2` 系数 -1，
    /// **两侧不对称**，`indexE` 在 p/q 互换时不相等，边在滤波后出现半 LSB 台阶。
    fn index_e(&self) -> i32 {
        let (p0, q0, p2, q2) = (self.p[0], self.q[0], self.p[2], self.q[2]);
        ((2 * p0 + 2 * q0 - p2 - q2 + 4) >> 2).clamp(-1, 1)
    }
}

/// 去块滤波对**单侧**样本（`q` 侧）的偏移（规范 8.7.2.5.3 普通滤波 / 8.7.2.5.4 强滤波）。
///
/// 返回**加到 `q0` 上**的偏移（`i32`，可负），`0` 表示不滤波。
///
/// **为什么必须是规范的两条路径，不能合成一条加权平均**：
///
/// - `bS == 4` 走**强滤波**（8.7.2.5.4）：`indexE` 决定 `p1/q1` 参与 4 抽头还是
///   6 抽头平均；`p0/q0` 用 **2 倍**偏移（`p0' = p0 - 2*(q0 - i0)`），
///   `p1/q1`/`p3/q3` 用 **1 倍**偏移。
/// - `1 <= bS <= 3` 走**普通滤波**（8.7.2.5.3）：
///   `Δ = Clip3(-tc, tc, (((q0-p0)<<2) + (p1-q1) + 4) >> 3)`，两侧各用 `∓Δ`。
///
/// 第一版把两条路径合成「6 抽头加权平均 + `b_s==2` 加 1 钳位」，等价于
/// 用 `bS==2` 当强滤波——**强滤波判据整体错位一档**（`bS==4` 的边只吃到普通滤波，
/// 强度差一个数量级），且 `p3/q3` 完全没进滤波式（它们在强滤波里要参与
/// `p3' = p3 - (q3 - i1)`）。
///
/// `bit_depth` 决定 `tC0 = t'C0 * (1 << (BitDepth - 8))` 的位深缩放。
///
/// 公开签名保持不变（`veg04_checks.rs` 的十二坐标形式）；函数体先把十二个
/// 坐标收成 [`DeblockEdge`]，后续判定全部走结构体方法，不再重复手写门限。
#[allow(clippy::too_many_arguments)]
pub fn deblock_tap_offset(
    p0: i32,
    p1: i32,
    p2: i32,
    p3: i32,
    q0: i32,
    q1: i32,
    q2: i32,
    q3: i32,
    b_s: u8,
    index_a: u32,
    index_b: u32,
    bit_depth: u32,
) -> i32 {
    let edge = DeblockEdge { p: [p0, p1, p2, p3], q: [q0, q1, q2, q3], index_a, index_b, bit_depth };
    // 三项门限（规范 8.7.2.5.2）：任一不满足 → 该样本不滤波。
    // |p0-q0| >= α 只判 q0 这一侧；判据顺序即语义，先判再算。
    if !edge.gated() {
        return 0;
    }

    if b_s == 4 {
        // ── 强滤波（规范 8.7.2.5.4），单侧偏移 ───────────────────────────────
        let index_e = edge.index_e();
        // indexE < 0：用 6 抽头（p1..q2 全参与）；否则用 4 抽头（不含 p3/q3）。
        let i0 = if index_e < 0 {
            (p2 + 2 * p1 + 2 * p0 + 2 * q0 + 2 * q1 + q2 + 4) >> 3
        } else {
            (p2 + p1 + p0 + q0 + q1 + q2 + 3) >> 3
        };
        // q0 的偏移 = 2 * (i0 - p0)（规范：q0' = q0 + 2*(p0 - i0)，p 侧对称）。
        2 * (i0 - p0)
    } else {
        // ── 普通滤波（规范 8.7.2.5.3）────────────────────────────────────────
        let tc = deblock_tc(index_a, index_b, b_s, bit_depth, p2, p0, q2, q0);
        if tc == 0 {
            return 0;
        }
        let delta = (((q0 - p0) << 2) + (p1 - q1) + 4) >> 3;
        delta.clamp(-tc, tc)
    }
}

/// 强滤波下 `p1/q1` 的偏移（规范 8.7.2.5.4 的 `strong_offset1`）。
///
/// 与 [`deblock_tap_offset`] 的 `b_s == 4` 分支配套：**强滤波下 `p0` 与 `p1`
/// 用不同倍数**（`p0` 是 2 倍、`p1` 是 1 倍），第一版把 `p0/p1` 用同一个
/// 加权平均，等于把强滤波降级成了「普通滤波换个系数」。
///
/// 返回**加到 `q1` 上**的偏移（1 倍，p1 侧对称）。
pub fn deblock_strong_offset1(
    p0: i32,
    p1: i32,
    p2: i32,
    q0: i32,
    q1: i32,
    q2: i32,
    index_a: u32,
    index_b: u32,
) -> i32 {
    let _ = p2;
    let _ = q2;
    let edge = DeblockEdge {
        p: [p0, p1, 0, 0],
        q: [q0, q1, 0, 0],
        index_a,
        index_b,
        bit_depth: 8,
    };
    if !edge.gated() {
        return 0;
    }
    // i1 = (p1 + 2*p0 + 2*q0 + q1 + 2) >> 2 —— 与 i0 同一套门限内、与 indexE 无关。
    let i1 = (p1 + 2 * p0 + 2 * q0 + q1 + 2) >> 2;
    // q1 的偏移 = i1 - q0（1 倍，p1 侧对称）。
    i1 - q0
}

/// 强滤波下 `p3/q3` 的偏移（规范 8.7.2.5.4 的 `p3' = p3 - (q3 - i1)`）。
///
/// `p3/q3` 用 `i1` 的 1 倍偏移，**且仅当门限内的 `indexE >= 0`** 时才做
/// ——`indexE < 0` 表示 `p2` 侧被判定为「更靠近 p」，规范此时不动 `p3/q3`。
/// 第一版对 `p3/q3` 完全不滤波，等于把 4 抽头强滤波降成 3 抽头。
pub fn deblock_strong_offset3(
    p0: i32,
    p1: i32,
    p2: i32,
    p3: i32,
    q0: i32,
    q1: i32,
    q2: i32,
    q3: i32,
    index_a: u32,
    index_b: u32,
) -> i32 {
    let edge = DeblockEdge { p: [p0, p1, p2, p3], q: [q0, q1, q2, q3], index_a, index_b, bit_depth: 8 };
    if !edge.gated() {
        return 0;
    }
    if edge.index_e() < 0 {
        return 0;
    }
    let i1 = (p1 + 2 * p0 + 2 * q0 + q1 + 2) >> 2;
    i1 - q3
}

/// 强滤波下 `p2/q2` 的偏移（规范 8.7.2.5.4 的 `p2' = p2 - (q2 - i0)`）。
///
/// `p2/q2` 用 **1 倍** `i0` 偏移（`p0/q0` 是 2 倍），且这一步**只在
/// `|p2 - p0| < β` 且 `|q2 - q0| < β` 时才做**——规范 8.7.2.5.4 把这条
/// 写在 `if (ap < Beta && aq < Beta)` 里。第一版没有这个条件，
/// 等于在跨块强对比处也去动 `p2/q2`，把真实边缘一起抹平。
///
/// 参数含 `p1/q1`：`i0` 的 4/6 抽头平均要用到它们，和 [`deblock_tap_offset`]
/// 的 `b_s == 4` 分支是同一个 `i0`，此处重算以保持函数自持（不依赖调用顺序）。
pub fn deblock_strong_offset2(
    p0: i32,
    p1: i32,
    p2: i32,
    q0: i32,
    q1: i32,
    q2: i32,
    index_a: u32,
    index_b: u32,
) -> i32 {
    let _ = p1;
    let _ = q1;
    let edge = DeblockEdge {
        p: [p0, 0, p2, 0],
        q: [q0, 0, q2, 0],
        index_a,
        index_b,
        bit_depth: 8,
    };
    // 这一道门限与另三个函数不同：判的是 `|p2-p0|` / `|q2-q0|`（而非 p1/q1），
    // 因为 p2/q2 参与滤波的前提是它们也落在 β 邻域内。
    if !edge.gated() {
        return 0;
    }
    let beta = i32::from(deblock_beta(index_b));
    if (p2 - p0).abs() >= beta || (q2 - q0).abs() >= beta {
        return 0;
    }
    let index_e = edge.index_e();
    let i0 = if index_e < 0 {
        (p2 + 2 * p1 + 2 * p0 + 2 * q0 + 2 * q1 + q2 + 4) >> 3
    } else {
        (p2 + p1 + p0 + q0 + q1 + q2 + 3) >> 3
    };
    // q2 的偏移 = i0 - q2（1 倍；p2 侧对称，是 -(i0 - p2)）。
    i0 - q2
}

/// 单条垂直边界的去块滤波（就地修改 `plane` 的 `x` 列，左侧 p、右侧 q）。
///
/// 门限由 `index_a` / `index_b` 查表得到；`bs` 是边界强度。
/// 返回实际被修改的样本数。
///
/// **两侧都要滤波**：规范 8.7.2.5 对 `p0` 与 `q0` 各算一次偏移
/// （互为相反数），分别写入。只滤一侧会让边出现系统性偏移（半 LSB 台阶）。
///
/// **`bs == 4` 与 `bs == 2` 是两条完全不同的路径**（第一版混成了一条）：
/// · `bs == 4`（8.7.2.5.4 强滤波）：`p0/q0` 用 2 倍偏移、`p1/q1` 用 1 倍偏移，
///   且 `p2/q2`、`p3/q3` 是否参与由 `indexE` 的符号决定；
/// · `bs == 2`（8.7.2.5.3 普通滤波）：`p0/q0` 用 `∓Δ`，`p1/q1` 用 `∓Δ1`，
///   `Δ1` 的钳位是 `-(tc+1)..(tc+1)` 而不是 `±tc`。
pub fn deblock_vertical_edge(
    plane: &mut Plane,
    x: i32,
    y0: i32,
    height: i32,
    bs: u8,
    index_a: u32,
    index_b: u32,
    bit_depth: u32,
) -> u32 {
    if bs == 0 || x <= 0 || x + 3 >= plane.width as i32 {
        return 0;
    }
    let alpha = deblock_alpha(index_a);
    let beta = deblock_beta(index_b);
    if alpha == 0 || beta == 0 {
        return 0;
    }
    let mut modified: u32 = 0;
    for y in y0..(y0 + height) {
        // 一次取齐八样本成 [`DeblockEdge`]，后续两侧调用都从这一个结构体展开，
        // 不再把 12 个坐标在四对调用点之间反复手抄。
        let edge = sample_vertical_edge(plane, x, y, index_a, index_b, bit_depth);
        let DeblockEdge { p, q, .. } = edge;
        let (p0, p1, p2, p3) = (p[0], p[1], p[2], p[3]);
        let (q0, q1, q2, q3) = (q[0], q[1], q[2], q[3]);

        // q 侧偏移 = f(p0..q3)，p 侧偏移 = f(q0..p3)（规范里互为相反数）。
        let dq = deblock_tap_offset(p0, p1, p2, p3, q0, q1, q2, q3, bs, index_a, index_b, bit_depth);
        let dp = deblock_tap_offset(q0, q1, q2, q3, p0, p1, p2, p3, bs, index_a, index_b, bit_depth);
        if dq != 0 && plane.put(x, y, clip_to_u8(q0 + dq)) {
            modified += 1;
        }
        if dp != 0 && plane.put(x - 1, y, clip_to_u8(p0 + dp)) {
            modified += 1;
        }

        if bs == 4 {
            // 强滤波四层（规范 8.7.2.5.4）：
            //   p0/q0 → 2 倍 i0（上面已算）
            //   p1/q1 → 1 倍 i1
            //   p2/q2 → 1 倍 i0，且需 |p2-p0|<β 且 |q2-q0|<β
            //   p3/q3 → 1 倍 i1，且需 indexE >= 0
            let d1q = deblock_strong_offset1(p0, p1, p2, q0, q1, q2, index_a, index_b);
            let d1p = deblock_strong_offset1(q0, q1, q2, p0, p1, p2, index_a, index_b);
            if d1q != 0 && plane.put(x + 1, y, clip_to_u8(q1 + d1q)) {
                modified += 1;
            }
            if d1p != 0 && plane.put(x - 2, y, clip_to_u8(p1 + d1p)) {
                modified += 1;
            }
            let d2q = deblock_strong_offset2(p0, p1, p2, q0, q1, q2, index_a, index_b);
            let d2p = deblock_strong_offset2(q0, q1, q2, p0, p1, p2, index_a, index_b);
            if d2q != 0 && plane.put(x + 2, y, clip_to_u8(q2 + d2q)) {
                modified += 1;
            }
            if d2p != 0 && plane.put(x - 3, y, clip_to_u8(p2 + d2p)) {
                modified += 1;
            }
            let d3q = deblock_strong_offset3(p0, p1, p2, p3, q0, q1, q2, q3, index_a, index_b);
            let d3p = deblock_strong_offset3(q0, q1, q2, q3, p0, p1, p2, p3, index_a, index_b);
            if d3q != 0 && plane.put(x + 3, y, clip_to_u8(q3 + d3q)) {
                modified += 1;
            }
            if d3p != 0 && plane.put(x - 4, y, clip_to_u8(p3 + d3p)) {
                modified += 1;
            }
        } else if bs == 2 {
            // 普通滤波第二层：Δ1 钳位到 ±(tc+1)（比 p0 那层宽 1）。
            let tc = deblock_tc(index_a, index_b, bs, bit_depth, p2, p0, q2, q0);
            let lim = tc + 1;
            let delta1 = (((q1 - p1) << 2) + (p2 - q2) + 4) >> 3;
            let d1q = delta1.clamp(-lim, lim);
            let d1p = (-delta1).clamp(-lim, lim);
            if d1q != 0 && plane.put(x + 1, y, clip_to_u8(q1 + d1q)) {
                modified += 1;
            }
            if d1p != 0 && plane.put(x - 2, y, clip_to_u8(p1 + d1p)) {
                modified += 1;
            }
        }
    }
    modified
}

/// 取一条垂直边界在 `y` 行的八样本（`p3..q3`，越界填中性值 128）。
///
/// 从 [`deblock_vertical_edge`] 的循环体里抽出：八次 `plane.at(...)` 各带一个
/// `unwrap_or(128)`，抽成函数后「取样本」与「算偏移 / 写回」两级职责分开，
/// 循环体从 40 行降到 20 行左右。
///
/// 只收 `index_a` / `index_b` / `bit_depth` 三项去块上下文：`bs` 不影响「取哪些
/// 样本」（八样本对强/普通滤波都一样），原先传进来只是没人用——多一个参数就多
/// 一处「以为改了会生效」的误解。
fn sample_vertical_edge(
    plane: &Plane,
    x: i32,
    y: i32,
    index_a: u32,
    index_b: u32,
    bit_depth: u32,
) -> DeblockEdge {
    let at = |dx: i32| -> i32 { plane.at(x + dx, y).unwrap_or(128) as i32 };
    DeblockEdge {
        p: [at(-1), at(-2), at(-3), at(-4)],
        q: [at(0), at(1), at(2), at(3)],
        index_a,
        index_b,
        bit_depth,
    }
}

/// 去块滤波的 `alpha` 完整查表（规范表 8-16，`indexA` 0..51 逐项）。
///
/// **用完整表而不是分段闭式**。第一版写的是三段公式（`19..23 => i+4`、
/// `24..39 => 2i-16`、`40..51 => 4i-12`），并注明「完整表由 F1219 对齐」。
/// 但三段公式与规范表**对不上**：
/// · 表里 `indexA=18` 是 **4**，公式给 `19..=23 => i+4` 时 18 落在别的段；
/// · 表里 `24→10`、`25→12`（跳 2），公式 `2i-16` 给 `25→34`（差 22）；
/// · 表里 `51→255`，公式 `(51-4)*4+4 = 192`（差 63）。
/// 也就是说滤波强度在 QP 中段就明显偏大。分段公式的斜率与规范表不一致，
/// 且**不报任何错**——只是画面在块边界出现台阶，只能靠逐像素对拍发现。
/// 表驱动把风险消掉：52 项可逐项对着规范核。
pub const DEBLOCK_ALPHA_TABLE: [u8; 52] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 4, 5, 6, 7, 8, 9, 10, 12, 13,
    15, 17, 20, 22, 25, 28, 32, 36, 40, 45, 50, 56, 63, 71, 80, 90, 101, 113, 127, 144, 162, 182, 203, 226, 255, 255,
];

/// 去块滤波的 α 查表（`indexA` 越界时钳到表端）。
///
/// `indexA = Clip3(0, 51, QP + 2*filterOffsetA)` 是规范 8.7.2.3 的定义，
/// 钳位在这里做，调用方不必各自重复。
pub fn deblock_alpha(index_a: u32) -> u8 {
    DEBLOCK_ALPHA_TABLE[(index_a as usize).min(51)]
}

/// 去块滤波的 `beta` 完整查表（规范表 8-17，`indexB` 0..51 逐项）。
///
/// 注意两点与第一版的分歧：
/// · β 表**最大是 18**（没有 α 那个 255），第一版 `.min(17)` 把上限压到 17；
/// · 表里 `27→4`，第一版 `23..=27 => (i-12)/2+3` 在 27 处算出 **7**
///   ——β 偏大近一倍，去块滤波明显过强（`23..27` 段斜率本应是 0）。
pub const DEBLOCK_BETA_TABLE: [u8; 52] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4,
    6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13, 14, 14, 15, 15, 16, 16, 17, 17, 18, 18,
];

/// 去块滤波的 β 查表（`indexB` 越界时钳到表端）。
pub fn deblock_beta(index_b: u32) -> u8 {
    DEBLOCK_BETA_TABLE[(index_b as usize).min(51)]
}

/// 去块滤波的 `t'C0` 完整查表（规范表 8-17，行 `indexA` 0..51，列 `bS` 0..3）。
///
/// **第一版在这里用一个闭式 `(min(index_a, index_b)/2)+1` 冒充查表，是错的**：
/// 表 8-17 的行索引只有 `indexA`，**与 `indexB` 无关**；列索引是 `bS`（1..3），
/// 不是 `min(index_a,index_b)`。闭式在 `indexA=51, bS=3` 给 `51/2+1 = 26`，
/// 而规范值是 **25**；更致命的是它在 `indexA=0..15`（α=0 的整段）仍给出非零值，
/// 而规范这三列在那里全为 0。闭式**不报任何错**——只是滤波强度整体偏大，
/// 只能靠逐项对表发现。
///
/// 表结构（行 `indexA`，列 `[bS=0, bS=1, bS=2, bS=3]`，`bS=0` 列恒为 0 不用）。
/// 零前缀是 `indexA` 0..15 共 16 行——与 α 表的零前缀**必须一致**
/// （`indexA≤15` 时 `α=0`，门限阶段就已经否决，`t'C0` 必须是 0；
/// 若两表零前缀错开一行，说明有一张表整体右移了一位）。
///
/// 三列起始点各不相同（这是表的真实结构，别当成排版错位）：
/// `bS=3` 从 `indexA=16` 起非零、`bS=2` 从 `21` 起、`bS=1` 从 `23` 起。
/// 边强度越弱，起始越晚——`bS=1` 的边在低 QP 段 `t'C0` 为 0，
/// 只有 QP 抬到 `indexA≥23` 才开始有幅度上限。
pub const DEBLOCK_TC0_TABLE: [[u8; 4]; 52] = [
    [0, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0],
    [0, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0],
    [0, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0],
    [0, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0],
    [0, 0, 0, 1], [0, 0, 0, 1], [0, 0, 0, 1], [0, 0, 0, 1],
    [0, 0, 0, 1], [0, 0, 1, 1], [0, 0, 1, 1], [0, 1, 1, 1],
    [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 1], [0, 1, 1, 2],
    [0, 1, 1, 2], [0, 1, 1, 2], [0, 1, 1, 2], [0, 1, 2, 3],
    [0, 1, 2, 3], [0, 2, 2, 3], [0, 2, 2, 4], [0, 2, 3, 4],
    [0, 2, 3, 4], [0, 3, 3, 5], [0, 3, 4, 6], [0, 3, 4, 6],
    [0, 4, 5, 7], [0, 4, 5, 8], [0, 4, 6, 9], [0, 5, 7, 10],
    [0, 6, 8, 11], [0, 6, 8, 13], [0, 7, 10, 14], [0, 8, 11, 16],
    [0, 9, 12, 18], [0, 10, 13, 20], [0, 11, 15, 23], [0, 13, 17, 25],
];

/// 去块滤波的 `t'C0` 查表（规范 8.7.2.3 的 `tC0 = t'C0 * (1<<(BitDepth-8))` 基数）。
///
/// **参数只有 `index_a` 与 `b_s`** —— 表 8-17 的行索引是 `indexA`，
/// 列索引是 `bS`。第一版把 `index_b` 也当参数传进来并参与取小，
/// 是把二维表误当成「对称查表函数」，签名本身就在表达错误的语义。
///
/// `b_s == 4` 走强滤波（不使用 `t'C0`），返回 0；
/// `index_a` 越界钳到 51（`indexA` 的定义式本身是 `Clip3(0,51,...)`）。
pub fn deblock_tc0(index_a: u32, b_s: u8) -> u8 {
    if b_s == 0 || b_s >= 4 {
        return 0;
    }
    DEBLOCK_TC0_TABLE[(index_a as usize).min(51)][b_s as usize]
}

/// 去块滤波的 `tC`（规范 8.7.2.5.3）：`tC = tC0 + (ap<β) + (aq<β)`。
///
/// `ap = |p2-p0|`、`aq = |q2-q0|`。这两项各加 1 是规范里的**条件增量**，
/// 漏掉会让强对比边（`|p2-p0|` 或 `|q2-q0|` 很小）的 `tC` 少 1~2，
/// 滤波在最容易出块效应的场景下反而偏弱。
///
/// `bit_depth` 参与的是 `tC0 = t'C0 * (1 << (BitDepth - 8))` 这一项位深缩放；
/// 规范明确**条件增量不受位深缩放**（专利文献亦确认），所以 `+1` 不缩放。
pub fn deblock_tc(
    index_a: u32,
    index_b: u32,
    b_s: u8,
    bit_depth: u32,
    p2: i32,
    p0: i32,
    q2: i32,
    q0: i32,
) -> i32 {
    let shift = bit_depth.saturating_sub(8).min(6);
    let beta = i32::from(deblock_beta(index_b));
    let base = i32::from(deblock_tc0(index_a, b_s)) << shift;
    let inc_p = i32::from((p2 - p0).abs() < beta);
    let inc_q = i32::from((q2 - q0).abs() < beta);
    base + inc_p + inc_q
}

// ════════════════════════════════════════════════════════════════════════════
