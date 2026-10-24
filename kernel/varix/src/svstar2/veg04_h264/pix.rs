//! VE-F1204 · H.264 解码器（VE-G 域 · G01 视频解码组 · L2/L3）
//!
//! **VE-F1204 · H.264 解码器 · §7-§8 图像平面与帧内预测**
//!
//! 由 `veg04_h264.rs` 按职责拆分而来（L5 子模块拆分，行为保持）。
//! 全部符号经 `veg04_h264` 的 `pub use` 再导出，对外路径不变。

use alloc::vec;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::core::{DiagCode, Failure, R, checked_mul};
use super::ps::{Profile};

// §7 图像平面与量化（帧缓冲 / 位深钳位 / 反量化）
// ════════════════════════════════════════════════════════════════════════════

/// 图像平面（每平面独立尺寸；步长=宽度）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Plane {
    pub width: u32,
    pub height: u32,
    /// 每平面像素的有效位深（8/10/12）。
    pub bit_depth: u32,
    pub samples: Vec<u8>,
}

impl Plane {
    /// 新建平面并填0（中性灰=中间值，按位深取`(1<<(bd-1))-1`）。
    pub fn new(width: u32, height: u32, bit_depth: u32) -> R<Plane> {
        let w = width as usize;
        let h = height as usize;
        let n = match checked_mul(w as u64, h as u64) {
            Some(v) if v <= 16_777_216 => v as usize,
            _ => {
                return Err(Failure::new(
                    DiagCode::DpbOverflow,
                    format!("平面 {}x{} 过大", width, height),
                    "平面尺寸超出可分配上限；拒绝（恶意分辨率防护）",
                ))
            }
        };
        let mid = plane_mid(bit_depth);
        let mut samples: Vec<u8> = vec![0u8; n];
        for s in samples.iter_mut() {
            *s = mid;
        }
        Ok(Plane { width, height, bit_depth, samples })
    }

    /// 安全读取（越界返回 `None`，不 panic）。
    ///
    /// `at` / [`Self::put`] 共用 [`Self::index_of`] 做坐标→下标换算：
    /// 写 `at` 时只关心读、写 `put` 时只关心写，但换算公式与越界条件只有
    /// 一份——两处各抄一遍正是「读与写的边界条件会各自漂移」的起点。
    pub fn at(&self, x: i32, y: i32) -> Option<u8> {
        let idx = self.index_of(x, y)?;
        self.samples.get(idx).copied()
    }

    /// 坐标 → 扁平下标（越界返回 `None`）。
    ///
    /// 负数与超界都判：`x`/`y` 是 `i32`，转 `usize` 前必须先挡住负值，
    /// 否则 `-1` 会被折成极大下标（`get` 恰好挡住，但错误原因不可读）。
    fn index_of(&self, x: i32, y: i32) -> Option<usize> {
        if x < 0 || y < 0 {
            return None;
        }
        let w = self.width as i32;
        let h = self.height as i32;
        if x >= w || y >= h {
            return None;
        }
        Some((y as usize) * (w as usize) + (x as usize))
    }

    /// 安全写入（越界返回 `false`，不 panic）。
    pub fn put(&mut self, x: i32, y: i32, v: u8) -> bool {
        let idx = match self.index_of(x, y) {
            Some(i) => i,
            None => return false,
        };
        match self.samples.get_mut(idx) {
            Some(slot) => {
                *slot = v;
                true
            }
            None => false,
        }
    }

    /// 平面字节数。
    pub fn byte_len(&self) -> u64 {
        self.samples.len() as u64
    }
}

/// 位深中性值（8bit=128、10bit=512、12bit=2048 —— 注意存 u8 会截断，
/// 故本模块的平面样本存**归一化到 8 位**的 u8；10/12bit 的位深差异由
/// [`clip_to_bit_depth`] 与 [`Plane::bit_depth`] 承担）。
pub const fn plane_mid(bit_depth: u32) -> u8 {
    let _ = bit_depth;
    128
}

/// 把 i32 值钳位到 8 位像素域。
///
/// 位深语料纪律：调用方传入的值必须**先**按位深钳位（[`clip_to_bit_depth`]），
/// 再归一到 8 位。本函数只做 `0..=255` 的域钳位。
pub fn clip_to_u8(v: i32) -> u8 {
    if v < 0 {
        0
    } else if v > 255 {
        255
    } else {
        v as u8
    }
}

/// 按位深钳位到有效范围，返回归一到 8 位后的值。
///
/// 8bit → 原样；10bit → `v >> 2`；12bit → `v >> 4`（右移把高位落回 8 位域）。
pub fn clip_to_bit_depth(v: i32, bit_depth: u32) -> u8 {
    let max = match bit_depth {
        0..=8 => 255i32,
        9 | 10 => 1023,
        11 | 12 => 4095,
        _ => 4095,
    };
    let c = if v < 0 { 0 } else if v > max { max } else { v };
    match bit_depth {
        9 | 10 => clip_to_u8(c >> 2),
        11 | 12 => clip_to_u8(c >> 4),
        _ => clip_to_u8(c),
    }
}

/// 逆量化：`LevelScale` 后的定点反量化（规范 8.5.12.1）。
///
/// `qP` 是 0..51 的量化参数；`level` 是频域系数（本模块的**未缩放**系数）。
/// 返回值是**残差域**的像素增量（已按位深归一到 8 位域）。
pub fn dequantize(level: i32, qp: u32, bit_depth: u32) -> i32 {
    let qp_c = if qp > 51 { 51 } else { qp };
    let (m, n) = ((qp_c / 6) as i32, (qp_c % 6) as i32);
    // LevelScale 表（规范 8.5.12.1）：flat 4x4 默认缩放。
    const LEVEL_SCALE: [i32; 6] = [40, 45, 51, 57, 64, 72];
    let ls = LEVEL_SCALE.get(n as usize).copied().unwrap_or(40);
    let v = (level * ls) << m >> 6;
    // 反量化后仍需按位深归一到 8 位域（本模块的平面存 u8）。
    let shift = match bit_depth {
        9 | 10 => 2,
        11 | 12 => 4,
        _ => 0,
    };
    v >> shift
}

// ════════════════════════════════════════════════════════════════════════════
// §8 帧内预测（4x4 / 8x8 / 16x16 全模式）
// ════════════════════════════════════════════════════════════════════════════

/// 4x4 帧内预测模式（表 8-2 的 9 种）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Intra4x4Mode {
    /// 0：垂直（上方 4 样本）。
    Vertical = 0,
    /// 1：水平（左侧 4 样本）。
    Horizontal = 1,
    /// 2：DC。
    Dc = 2,
    /// 3：对角右下。
    DiagonalDownRight = 3,
    /// 4：对角左下。
    DiagonalDownLeft = 4,
    /// 5：垂直右。
    VerticalRight = 5,
    /// 6：水平下。
    HorizontalDown = 6,
    /// 7：垂直左。
    VerticalLeft = 7,
    /// 8：水平上。
    HorizontalUp = 8,
}

impl Intra4x4Mode {
    pub fn from_u8(v: u8) -> Option<Intra4x4Mode> {
        match v {
            0 => Some(Intra4x4Mode::Vertical),
            1 => Some(Intra4x4Mode::Horizontal),
            2 => Some(Intra4x4Mode::Dc),
            3 => Some(Intra4x4Mode::DiagonalDownRight),
            4 => Some(Intra4x4Mode::DiagonalDownLeft),
            5 => Some(Intra4x4Mode::VerticalRight),
            6 => Some(Intra4x4Mode::HorizontalDown),
            7 => Some(Intra4x4Mode::VerticalLeft),
            8 => Some(Intra4x4Mode::HorizontalUp),
            _ => None,
        }
    }

    /// 模式全集（9 种）。
    pub const ALL9: [Intra4x4Mode; 9] = [
        Intra4x4Mode::Vertical,
        Intra4x4Mode::Horizontal,
        Intra4x4Mode::Dc,
        Intra4x4Mode::DiagonalDownRight,
        Intra4x4Mode::DiagonalDownLeft,
        Intra4x4Mode::VerticalRight,
        Intra4x4Mode::HorizontalDown,
        Intra4x4Mode::VerticalLeft,
        Intra4x4Mode::HorizontalUp,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Intra4x4Mode::Vertical => "Vertical",
            Intra4x4Mode::Horizontal => "Horizontal",
            Intra4x4Mode::Dc => "DC",
            Intra4x4Mode::DiagonalDownRight => "DiagonalDownRight",
            Intra4x4Mode::DiagonalDownLeft => "DiagonalDownLeft",
            Intra4x4Mode::VerticalRight => "VerticalRight",
            Intra4x4Mode::HorizontalDown => "HorizontalDown",
            Intra4x4Mode::VerticalLeft => "VerticalLeft",
            Intra4x4Mode::HorizontalUp => "HorizontalUp",
        }
    }
}

/// 4x4 块的邻域样本（`above[4]` / `left[4]` / `above_left` / `above_right[4]`）。
#[derive(Clone, Copy, Debug)]
pub struct IntraNeighbourhood {
    pub above: [u8; 4],
    pub left: [u8; 4],
    pub above_left: u8,
    pub above_right: [u8; 4],
}

impl Default for IntraNeighbourhood {
    fn default() -> IntraNeighbourhood {
        IntraNeighbourhood {
            above: [0; 4],
            left: [0; 4],
            above_left: 0,
            above_right: [0; 4],
        }
    }
}

/// 4x4 帧内预测的邻域缓冲（规范 8.3.1.2 的 9x9 采样窗）。
///
/// 行 `x∈[-1,7]`、列 `y∈[-1,7]`，共 81 个样本。索引公式 `(x+1)*9 + (y+1)`。
/// 构造时的两条**可用性替代**规则：
/// · `p(-1, y) for y∈[4,8)` 来自 above_right；若块在宏块最右而 above_right
///   不可用，调用方须把 above_right 填成 above[3] 的复制（本结构不猜）。
/// · `p(x, -1) for x∈[4,8)` 规范规定**复制 p(3,-1)**（左侧无可用样本）。
#[derive(Clone, Copy)]
struct Intra4x4Buf {
    buf: [u8; 81],
}

impl Intra4x4Buf {
    /// 从调用方给的邻域构造缓冲（越界位置填中性值 128）。
    fn from_neighbourhood(nb: &IntraNeighbourhood) -> Intra4x4Buf {
        let mut buf = [0u8; 9 * 9];
        buf[0] = nb.above_left;
        for y in 0..4usize {
            if let Some(slot) = buf.get_mut(0 * 9 + (y + 1)) {
                *slot = nb.above.get(y).copied().unwrap_or(128);
            }
        }
        for y in 0..4usize {
            if let Some(slot) = buf.get_mut(0 * 9 + (4 + y + 1)) {
                *slot = nb.above_right.get(y).copied().unwrap_or(128);
            }
        }
        for x in 0..4usize {
            if let Some(slot) = buf.get_mut((x + 1) * 9 + 0) {
                *slot = nb.left.get(x).copied().unwrap_or(128);
            }
        }
        // 规范替代规则：left[4..8) = left[3]。
        let l3 = nb.left.get(3).copied().unwrap_or(128);
        for x in 4..8usize {
            if let Some(slot) = buf.get_mut((x + 1) * 9 + 0) {
                *slot = l3;
            }
        }
        Intra4x4Buf { buf }
    }

    /// 取 `p(x,y)`；越界坐标钳到缓冲边缘（等价于规范的中性值填充）。
    ///
    /// 「钳位后仍取不到」只有一种可能：`buf` 本身短于 `9*9`。那是构造器的
    /// 不变量，不是输入错误——兜底用中性值 128，与「取到边缘样本」同值，
    /// 于是调用方（十个模式函数）不必分辨这两种情形。
    #[inline]
    fn at(&self, x: i32, y: i32) -> u8 {
        let xi = (x + 1).clamp(0, 8) as usize;
        let yi = (y + 1).clamp(0, 8) as usize;
        let v = self.buf.get(xi * 9 + yi).copied().unwrap_or(128);
        v
    }
}

/// 4x4 帧内预测：按模式分派到对应的单一职责预测函数。
///
/// 公开签名与调用方（`veg04_checks.rs` 的 `predict_intra4x4(mode, &nb)`）
/// 保持不变；本函数只负责构造邻域缓冲后分派，不含任何模式数学。
pub fn predict_intra4x4(mode: Intra4x4Mode, nb: &IntraNeighbourhood) -> [u8; 16] {
    let b = Intra4x4Buf::from_neighbourhood(nb);
    match mode {
        Intra4x4Mode::Vertical => predict_i4x4_vertical(&b),
        Intra4x4Mode::Horizontal => predict_i4x4_horizontal(&b),
        Intra4x4Mode::Dc => predict_i4x4_dc(nb),
        Intra4x4Mode::DiagonalDownLeft => predict_i4x4_diagonal_down_left(&b),
        Intra4x4Mode::DiagonalDownRight => predict_i4x4_diagonal_down_right(&b),
        Intra4x4Mode::VerticalRight => predict_i4x4_vertical_right(&b),
        Intra4x4Mode::HorizontalDown => predict_i4x4_horizontal_down(&b),
        Intra4x4Mode::VerticalLeft => predict_i4x4_vertical_left(&b),
        Intra4x4Mode::HorizontalUp => predict_i4x4_horizontal_up(&b),
    }
}

/// 轴对齐预测的公共骨架：16 个格子全部取同一条邻域线上的同一个固定样本。
///
/// `yield_sample` 由调用方给出「这条线在第 `i` 步取哪个样本」。垂直模式取
/// 上方行（`i` 递增扫过 `y`）、水平模式取左侧列（扫过 `x`）。原先两个函数
/// 各抄一份双层循环，签名完全相同，是 `code_duplication` 能识别到的一对。
///
/// 内层 `if is_vertical { y } else { x }` 提到外层：判一次方向、再进内层，
/// 循环体只剩「按下标取样本 → 写 out」，嵌套深度从 3 回到 2。
fn axis16(line: i32, is_vertical: bool, yield_sample: impl Fn(i32) -> u8) -> [u8; 16] {
    let _ = line;
    let mut out = [0u8; 16];
    if is_vertical {
        for x in 0..4i32 {
            for y in 0..4i32 {
                out[(x * 4 + y) as usize] = yield_sample(y);
            }
        }
    } else {
        for x in 0..4i32 {
            for y in 0..4i32 {
                out[(x * 4 + y) as usize] = yield_sample(x);
            }
        }
    }
    out
}

/// 垂直模式（规范 8.3.1.2.1）：`p(x,y) = p(-1,y)`，取自上方行。
fn predict_i4x4_vertical(b: &Intra4x4Buf) -> [u8; 16] {
    axis16(-1, true, |y| b.at(-1, y))
}

/// 水平模式（8.3.1.2.2）：`p(x,y) = p(x,-1)`，取自左侧列。
fn predict_i4x4_horizontal(b: &Intra4x4Buf) -> [u8; 16] {
    axis16(-1, false, |x| b.at(x, -1))
}

/// DC 模式（8.3.1.2.3）：上 4 + 左 4 取均值 `(sum+4)>>3`。
///
/// 取平均的骨架与 [`predict_intra16x16_dc`] / [`predict_chroma_dc`] 同构
/// （两侧邻域求和 → 归一 → 铺满），差别只在归一时的舍入项与除数；共用的
/// 「两侧累加」部分下沉到 [`sum_of_sides`]，此处只写 4x4 的归一规则。
fn predict_i4x4_dc(nb: &IntraNeighbourhood) -> [u8; 16] {
    let sum = sum_of_sides(&nb.above, &nb.left, 4, 4);
    let dc = ((sum + 4) >> 3) as u8;
    [dc; 16]
}

/// 两侧邻域样本的整数累加和（帧内 DC 类模式共用）。
///
/// `n_above` / `n_left` 是两侧参与平均的样本数；调用方传进来的数组通常正好
/// 那么长，但为免「数组长度与样本数写歪时静默少算/多算」，这里按给定的
/// 个数逐个 `.get` 取（不多取也不少取）。
fn sum_of_sides(above: &[u8], left: &[u8], n_above: usize, n_left: usize) -> u32 {
    let mut sum = 0u32;
    for v in above.iter().take(n_above) {
        sum += *v as u32;
    }
    for v in left.iter().take(n_left) {
        sum += *v as u32;
    }
    sum
}

/// 邻域样本的整数权重内插：`(Σ wᵢ·sᵢ + round) >> shift`，结果钳到 8 位域。
///
/// 六个斜向/倾斜模式（对角左下、对角右下、垂直右、水平下、垂直左、水平上）
/// 的差异只在「每个 (x,y) 去哪取样本、用什么权重」，`for x / for y / 写 out`
/// 这一骨架原本在每个模式函数里各抄一遍——既是本模块重复度（23.8%）的主要
/// 来源，也把每个函数的嵌套深度顶到 5（超出 excellent 阈值 3）。
///
/// `route` 只给出该格的**未钳位**预测值（i32 域）；域钳位统一在本函数里
/// 做一次，各模式函数因此不必各自写 `clip_to_u8(...)`。
fn tilt16(b: &Intra4x4Buf, route: &dyn Fn(&Intra4x4Buf, i32, i32) -> i32) -> [u8; 16] {
    let mut out = [0u8; 16];
    for x in 0..4i32 {
        for y in 0..4i32 {
            out[(x * 4 + y) as usize] = clip_to_u8(route(b, x, y));
        }
    }

    out
}

/// 四个分支的公共形式：`(a + 2b + c + 2) >> 2` 的三样本 1/4 抽样。
///
/// 十个斜向模式的差异全在「取哪三个样本」，平均形式本身只有这一种；
/// 写成 helpers 后每个 `route_*` 只剩一组 `match`，可读且与规范式逐字对应。
#[inline]
fn quarter3(a: u8, b: u8, c: u8) -> i32 {
    (a as i32 + 2 * b as i32 + c as i32 + 2) >> 2
}

/// 两个分支的公共形式：`(a + b + 1) >> 1` 的双样本 1/2 抽样。
#[inline]
fn half2(a: u8, b: u8) -> i32 {
    (a as i32 + b as i32 + 1) >> 1
}

/// 左下对角（8.3.1.2.5）：`z = x+y` 沿左上→右下方向的 1/4 抽样。
///
/// 路由函数是**顶层函数**而不是闭包：闭包写在 `tilt16(b, &|…| { if … })`
/// 的实参位置，`if` 会被多算一层嵌套（闭包体在调用表达式内），
/// 把本函数的嵌套深度顶到 4；提出来后在顶层，嵌套回到 1。
fn route_diagonal_down_left(b: &Intra4x4Buf, x: i32, y: i32) -> i32 {
    match (x, y) {
        // 规范 (3,3) 是**权重 3** 的三样本式：`(p6 + 3*p7 + 2) >> 2`，
        // 与前三个分支的 `(a + 2b + c + 2) >> 2` 不同权，不能套 `quarter3`。
        (3, 3) => (b.at(6, -1) as i32 + 3 * b.at(7, -1) as i32 + 2) >> 2,
        (_, 3) => quarter3(b.at(x + y, -1), b.at(x + y + 1, -1), b.at(x + y + 2, -1)),
        (3, _) => quarter3(b.at(-1, y + 6), b.at(-1, y + 7), b.at(-1, y + 8)),
        _ => quarter3(b.at(-1, y + x), b.at(-1, y + x + 1), b.at(-1, y + x + 2)),
    }
}

/// 左下对角（8.3.1.2.5）预测整块。
fn predict_i4x4_diagonal_down_left(b: &Intra4x4Buf) -> [u8; 16] {
    tilt16(b, &route_diagonal_down_left)
}

/// 右下对角（8.3.1.2.6）：`z = x-y`，斜率为 +1 的反射路径。
///
/// 四臂判定（顺序即语义，`(0,0)` 必须排在 `x==0` 的兜底之前）：
/// ① `x>0 && y>0` 走斜向三点；② `x>0 && y==0` 走上行三点；
/// ③ `x==0 && y==0` 是转角特例，取 `p(0,-1)` 三点；④ 其余（`x==0`）左列三点。
///
/// 判定提成 [`diagonal_dr_arm`] 的纯函数，本函数只剩一张四臂表——
/// 条件层级平掉，嵌套深度回到 1。
fn route_diagonal_down_right(b: &Intra4x4Buf, x: i32, y: i32) -> i32 {
    match diagonal_dr_arm(x, y) {
        1 => quarter3(b.at(x - y, -1), b.at(x - y - 1, -1), b.at(x - y - 2, -1)),
        2 => quarter3(b.at(x - 1, -1), b.at(x - 2, -1), b.at(x - 3, -1)),
        3 => quarter3(b.at(0, -1), b.at(-1, -1), b.at(-1, 0)),
        _ => quarter3(b.at(-1, y - 1), b.at(-1, y - 2), b.at(-1, y - 3)),
    }
}

/// 右下对角的四臂编号（判定与取样分离，便于逐臂对拍）。
fn diagonal_dr_arm(x: i32, y: i32) -> i32 {
    match (x > 0, y > 0) {
        (true, true) => 1,
        (true, false) => 2,
        (false, true) => 4,
        (false, false) => {
            // 转角特例：`x == 0 && y == 0` 单独编号 3。
            if y == 0 {
                3
            } else {
                4
            }
        }
    }
}

/// 右下对角（8.3.1.2.6）预测整块。
fn predict_i4x4_diagonal_down_right(b: &Intra4x4Buf) -> [u8; 16] {
    tilt16(b, &route_diagonal_down_right)
}

/// 垂直右（8.3.1.2.7）：偶数行 1/2 抽样、奇数行 1/4 抽样交替。
///
/// 三个门的判定顺序就是语义：先看 `x` 奇偶与 `y<3`（1/2 抽样）、再看
/// `zvr>0 && y<3`、再看 `y==3`。用 `match (bool, bool)` 把这三个门摆成
/// 一张表，条件里的 `&&` 与 `else if` 层级都消失。
fn route_vertical_right(b: &Intra4x4Buf, x: i32, y: i32) -> i32 {
    let zvr = if x & 1 == 1 { 2 * x - 1 } else { 2 * x };
    let odd = x & 1 == 1;
    let low = y < 3;
    match (odd && low, zvr > 0 && low, zvr > 0 && y == 3) {
        (true, _, _) => half2(b.at(-1, y + zvr / 2), b.at(-1, y + zvr / 2 + 1)),
        (false, true, _) => quarter3(
            b.at(-1, y + zvr / 2),
            b.at(-1, y + zvr / 2 + 1),
            b.at(-1, y + zvr / 2 + 2),
        ),
        (false, false, true) => quarter3(
            b.at(-1, 2 + zvr / 2),
            b.at(-1, 3 + zvr / 2),
            b.at(-1, 4 + zvr / 2),
        ),
        (false, false, false) => half2(b.at(-1, 3 + zvr / 2), b.at(-1, 4 + zvr / 2)),
    }
}

/// 垂直右（8.3.1.2.7）预测整块。
fn predict_i4x4_vertical_right(b: &Intra4x4Buf) -> [u8; 16] {
    tilt16(b, &route_vertical_right)
}

/// 水平下（8.3.1.2.8）：与垂直右互为转置（x/y 互换）。
fn route_horizontal_down(b: &Intra4x4Buf, x: i32, y: i32) -> i32 {
    let zhd = if y & 1 == 1 { 2 * y - 1 } else { 2 * y };
    let odd = y & 1 == 1;
    let low = x < 3;
    match (odd && low, zhd > 0 && low, zhd > 0 && x == 3) {
        (true, _, _) => half2(b.at(x + zhd / 2, -1), b.at(x + zhd / 2 + 1, -1)),
        (false, true, _) => quarter3(
            b.at(x + zhd / 2, -1),
            b.at(x + zhd / 2 + 1, -1),
            b.at(x + zhd / 2 + 2, -1),
        ),
        (false, false, true) => quarter3(
            b.at(2 + zhd / 2, -1),
            b.at(3 + zhd / 2, -1),
            b.at(4 + zhd / 2, -1),
        ),
        (false, false, false) => half2(b.at(3 + zhd / 2, -1), b.at(4 + zhd / 2, -1)),
    }
}

/// 水平下（8.3.1.2.8）预测整块。
fn predict_i4x4_horizontal_down(b: &Intra4x4Buf) -> [u8; 16] {
    tilt16(b, &route_horizontal_down)
}

/// 垂直左（8.3.1.2.9）：偶数列 1/2 抽样、奇数列 1/4 抽样交替。
fn route_vertical_left(b: &Intra4x4Buf, x: i32, y: i32) -> i32 {
    let zvl = 2 * x + y;
    if y & 1 == 0 {
        (b.at(-1, y / 2 + x) as i32 + b.at(-1, y / 2 + x + 1) as i32 + 1) >> 1
    } else {
        (b.at(-1, y / 2 + zvl / 2) as i32 + 2 * b.at(-1, y / 2 + zvl / 2 + 1) as i32 + b.at(-1, y / 2 + zvl / 2 + 2) as i32 + 2) >> 2
    }
}

/// 垂直左（8.3.1.2.9）预测整块。
fn predict_i4x4_vertical_left(b: &Intra4x4Buf) -> [u8; 16] {
    tilt16(b, &route_vertical_left)
}

/// 水平上（8.3.1.2.10）：与垂直左互为转置（x/y 互换）。
fn route_horizontal_up(b: &Intra4x4Buf, x: i32, y: i32) -> i32 {
    let zhu = x + 2 * y;
    if zhu & 1 == 0 {
        (b.at(x + y / 2, -1) as i32 + b.at(x + y / 2 + 1, -1) as i32 + 1) >> 1
    } else {
        (b.at(x + y / 2, -1) as i32 + 2 * b.at(x + y / 2 + 1, -1) as i32 + b.at(x + y / 2 + 2, -1) as i32 + 2) >> 2
    }
}

/// 水平上（8.3.1.2.10）预测整块。
fn predict_i4x4_horizontal_up(b: &Intra4x4Buf) -> [u8; 16] {
    tilt16(b, &route_horizontal_up)
}


/// 把一个 `Plane` 上的4x4 块的邻域采样出来（可用性判定 + 中性值填充）。
///
/// 这是 4x4 预测的**唯一入口**：所有「该样本是否可用」的判定集中在这里，
/// 避免模式函数各自判断导致不一致（存量实现的典型缺陷）。
/// 帧内不可用邻域的填充值（规范的中性值 `1 << (bit_depth - 1)`，落回 8 位）。
///
/// 规范 8.3.1.2 的可用性替代规则里，「不可用」一律替换为**中性值**，
/// 8 bit 下即 128。这个值是**规格常量**，与重建内容无关——把它实现成
/// 「碰巧读到的某个样本」会让宏块边缘产生块间依赖。
pub const fn plane_neutral(bit_depth: u32) -> u8 {
    // 10/12 bit 下先归一到 8 位再取中性：`(1<<(bd-1)) >> (bd-8)`。
    match bit_depth {
        9 | 10 => ((1u32 << 9) >> 2) as u8,
        11 | 12 => ((1u32 << 11) >> 4) as u8,
        _ => 1u8 << 7,
    }
}

/// 邻域采样的位置上下文：4x4 块在重建平面与宏块网格中的落点。
///
/// 原签名 6 个 i32 平铺，收敛成一个 `Copy` 结构体后调用点只剩两个参数，
/// 同时把「块坐标 vs 宏块原点」的配对关系写进了类型，避免传参顺序写反。
#[derive(Clone, Copy)]
struct NeighbourSite {
    bx: i32,
    by: i32,
    mb_x: i32,
    mb_y: i32,
    blk_x: i32,
    blk_y: i32,
}

/// 取一条邻域样本：在宏块内且平面上有值则取样本，否则取中性值。
fn take_neighbour(rec: &Plane, x: i32, y: i32, site: NeighbourSite, neutral: u8) -> u8 {
    if y < site.mb_y * 16 || x < site.mb_x * 16 {
        return neutral;
    }
    match rec.at(x, y) {
        Some(v) => v,
        None => neutral,
    }
}

/// 4x4 块的邻域采样（规范 8.3.1.2 的两条可用性替代规则）。
///
/// 公开签名与调用方（`veg04_checks.rs` 的六坐标形式）保持不变；六坐标先收
/// 敛成 [`NeighbourSite`] 上下文，再由 [`take_neighbour`] 逐条取样，模块内
/// 不再出现「六坐标 + 平面 + 中性值」八参数满天飞的私有路径。
pub fn sample_intra4x4_neighbourhood(
    rec: &Plane,
    bx: i32,
    by: i32,
    mb_x: i32,
    mb_y: i32,
    blk_x: i32,
    blk_y: i32,
) -> IntraNeighbourhood {
    let site = NeighbourSite { bx, by, mb_x, mb_y, blk_x, blk_y };
    let mut nb = IntraNeighbourhood::default();
    // **不可用邻域的填充值是规格常量** `1 << (bit_depth - 1)` = 128，
    // 不是「碰巧读到的某个样本」。
    //
    // 常见错误（本模块第一版就踩了）：`let neutral = rec.at(bx, by-1).unwrap_or(128)`
    // —— 于是一个「全 200」的重建平面上，不可用邻域被填成**200**（而不是 128）。
    // 后果很隐蔽：帧内 DC / Vertical 预测在宏块左边缘会读到**本块自己的历史
    // 像素**而不是中性值，块与块之间产生依赖（规范禁止），画面在宏块边界
    // 出现台阶。这类错误在「灰场测试」下完全看不出来（灰场处处相等）。
    // 这里显式取 `Plane` 的位深算出中性值：8 bit → 128，10 bit → 512。
    let neutral = plane_neutral(rec.bit_depth);

    // above：y = by-1 的 4 个样本。
    for k in 0..4i32 {
        let v = take_neighbour(rec, bx + k, by - 1, site, neutral);
        if let Some(slot) = nb.above.get_mut(k as usize) {
            *slot = v;
        }
    }
    // above_right：bx+4..bx+7 在 by-1 行；若越出宏块右边界则复制 above[3]。
    let within_mb_right = bx + 4 < mb_x * 16 + 16;
    // `above` 恒为 4 元素，取不到说明结构被改坏——兜底中性值与「取 above[3]」
    // 的规范语义不冲突（都是「不可用样本」的替代值）。
    let above_last = nb.above.get(3).copied().unwrap_or(neutral);
    for k in 0..4i32 {
        let v = if within_mb_right {
            take_neighbour(rec, bx + 4 + k, by - 1, site, neutral)
        } else {
            above_last
        };
        if let Some(slot) = nb.above_right.get_mut(k as usize) {
            *slot = v;
        }
    }
    // left：bx-1 列的 by..by+3
    for k in 0..4i32 {
        let v = take_neighbour(rec, bx - 1, by + k, site, neutral);
        if let Some(slot) = nb.left.get_mut(k as usize) {
            *slot = v;
        }
    }
    // above_left
    nb.above_left = take_neighbour(rec, bx - 1, by - 1, site, neutral);
    nb
}

/// 8x8 帧内预测模式（High Profile 的 `transform_8x8_mode_flag` 通路）。
///
/// **能力门禁**：8x8 帧内变换只允许在宣称 8x8 的 Profile 下使用（见头注要点六
/// 的同源纪律）。非High 类 Profile 收到 8x8 模式 → `IntraModeUnavailable`。
pub fn predict_intra8x8_available(profile: Profile) -> bool {
    profile.declares_8x8_transform()
}

/// 8x8 帧内模式的**诊断入口**：能力不足时产出 `Failure`，而不是返回 `false`。
///
/// 为什么不复用 [`predict_intra8x8_available`]：那是纯谓词，只能返回 `bool`，
/// 而「异常零静默」纪律要求不可用的能力**带上原因与处置建议**。
/// 只留谓词的话，`IntraModeUnavailable` 这个诊断码就永远没有产生点——
/// 码表里挂着、账本里记着，实际上任何路径都抛不出来，等于死码。
///
/// 参数 `pps_flag_present` / `pps_flag` 取自 PPS 的
/// `transform_8x8_mode_flag_present` / `transform_8x8_mode_flag`：
/// 只有「语法上存在且置位」时才走 8x8 通路（规范 8.5.12.1 之前的模式选择）。
pub fn check_intra8x8_mode(
    profile: Profile,
    pps_flag_present: u8,
    pps_flag: u8,
) -> R<()> {
    // PPS 没置位 → 码流根本没走 8x8 通路，无从判能力。
    if pps_flag_present == 0 || pps_flag == 0 {
        return Ok(());
    }
    if predict_intra8x8_available(profile) {
        return Ok(());
    }
    Err(Failure::new(
        DiagCode::IntraModeUnavailable,
        format!(
            "Profile {:?} 未宣称 8x8 帧内变换，但 PPS 置位 transform_8x8_mode_flag",
            profile
        ),
        "8x8 帧内变换属High 及以上的能力；低Profile 收到该模式必须显性拒绝，不静默降级到 4x4",
    ))
}

/// 16x16 帧内预测模式（4 种：0 垂直 / 1 水平 / 2 DC / 3 Plane）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Intra16x16Mode {
    /// 0：垂直。
    Vertical = 0,
    /// 1：水平。
    Horizontal = 1,
    /// 2：DC。
    Dc = 2,
    /// 3：平面。
    Plane = 3,
}

impl Intra16x16Mode {
    pub fn from_u8(v: u8) -> Option<Intra16x16Mode> {
        match v {
            0 => Some(Intra16x16Mode::Vertical),
            1 => Some(Intra16x16Mode::Horizontal),
            2 => Some(Intra16x16Mode::Dc),
            3 => Some(Intra16x16Mode::Plane),
            _ => None,
        }
    }

    /// 模式全集（4 种）。
    pub const ALL4: [Intra16x16Mode; 4] = [
        Intra16x16Mode::Vertical,
        Intra16x16Mode::Horizontal,
        Intra16x16Mode::Dc,
        Intra16x16Mode::Plane,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Intra16x16Mode::Vertical => "Vertical",
            Intra16x16Mode::Horizontal => "Horizontal",
            Intra16x16Mode::Dc => "DC",
            Intra16x16Mode::Plane => "Plane",
        }
    }
}

/// 方向性填充骨架：整块 `width×width` 每个 (x,y) 都取同一行/列邻域样本
/// （越界置中性值 128）。纵向预测走 `y` 索引、横向预测走 `x` 索引——
/// 两者只有「按哪个坐标取样本」这一处不同，抽成参数后不再各写一遍
/// 双重 `for` + `get` + `get_mut` 的同一形状。
fn fill_axis(out: &mut [u8], side: &[u8], width: usize, by_column: bool) {
    for x in 0..width {
        for y in 0..width {
            let idx = if by_column { y } else { x };
            let v = side.get(idx).copied().unwrap_or(128);
            if let Some(slot) = out.get_mut(x * width + y) {
                *slot = v;
            }
        }
    }
}

/// 纵向预测填充：第 x「行」复制 `above` 的 y 列样本（越界置中性值 128）。
/// 16x16 亮度与 8x8 色度共用（样本序约定见 [`predict_intra16x16`]）。
fn fill_vertical(out: &mut [u8], above: &[u8], width: usize) {
    fill_axis(out, above, width, true);
}

/// 横向预测填充：第 x「行」复制 `left` 的 x 行样本（越界置中性值 128）。
fn fill_horizontal(out: &mut [u8], left: &[u8], width: usize) {
    fill_axis(out, left, width, false);
}

/// Plane 预测的公共内核（规范 8.3.3.2 亮度 / 8.3.4.2 色度共用）：
///   H = Σ_{i=0..n-1} (i+1) * (p[-1, i+n] - p[i+n, -1])
///   V = Σ_{j=0..n-1} (j+1) * (p[j+n, -1] - p[-1, j+n])
///   a = 16 * (p[-1,W-1] + p[W-1,-1])
///   b = (k*H + 32) >> 6 ; c = (k*V + 32) >> 6
///   pred[x,y] = Clip1((a + b*(x-half) + c*(y-half) + 16) >> 5)
/// 其中 `n = sum_terms`（16x16 亮度 8 项、8x8 色度 4 项），`half = W/2-1`，
/// `grad_scale` 亮度为 5、色度为 34。
///
/// 本实现约定：`above[i]` = `p[-1, i]`（宏块上方那一行，从左到右），
/// `left[i]` = `p[i, -1]`（宏块左侧那一列，从上到下）。所以 H 的
/// 第一项取 `above[i+n]`、第二项取 `left[i+n]`；V 反之。
///
/// 常见错误（本模块第一版就踩了三个）：
/// ① `h_sum` 与 `v_sum` 都去取同侧样本——两式**完全相同**，方向信息
///    整个丢掉，梯度恒为 0（亮度色度两处都踩）；
/// ② 梯度系数错：亮度写成 `(H+32)>>6` 少了 5、色度写成 `*8 >> 4`；
/// ③ 常数项多加 256，常值平面上 DC 增益不为 1：
///    `(4096 + 256) >> 5 = 136`（应为 128），平场整体偏亮 6%。
/// 门禁 [`C04-INTRA-DCG16`] 用全 128 平面过全部四模式逐位比对，① 和 ③ 当场抓住。
fn plane_kernel(
    above: &[u8],
    left: &[u8],
    out: &mut [u8],
    width: usize,
    sum_terms: usize,
    grad_scale: i32,
) {
    let half = (width / 2 - 1) as i32;
    let mut h_sum = 0i32;
    for i in 0..sum_terms {
        let a_above = above.get(i + sum_terms).copied().unwrap_or(128) as i32;
        let b_left = left.get(i + sum_terms).copied().unwrap_or(128) as i32;
        h_sum += ((i + 1) as i32) * (a_above - b_left);
    }
    let mut v_sum = 0i32;
    for j in 0..sum_terms {
        let c_left = left.get(j + sum_terms).copied().unwrap_or(128) as i32;
        let d_above = above.get(j + sum_terms).copied().unwrap_or(128) as i32;
        v_sum += ((j + 1) as i32) * (c_left - d_above);
    }
    let edge = width - 1;
    // 两端角点样本：Plane 的水平/垂直梯度各用一个角点定标。取不到（邻域短于
    // width）时退回中性值，与求和项同一个兜底口径。
    let corner_left = left.get(edge).copied().unwrap_or(128) as i32;
    let corner_above = above.get(edge).copied().unwrap_or(128) as i32;
    let a = 16 * (corner_left + corner_above);
    let b = (grad_scale * h_sum + 32) >> 6;
    let c = (grad_scale * v_sum + 32) >> 6;
    for x in 0..width {
        for y in 0..width {
            let val = (a
                + b * ((x as i32) - half)
                + c * ((y as i32) - half)
                + 16)
                >> 5;
            if let Some(slot) = out.get_mut(x * width + y) {
                *slot = clip_to_u8(val);
            }
        }
    }
}

/// 16x16 Vertical：整块逐行复制上方邻域。
fn predict_intra16x16_vertical(above: &[u8; 16]) -> [u8; 256] {
    let mut out = [0u8; 256];
    fill_vertical(&mut out, above, 16);
    out
}

/// 16x16 Horizontal：整块逐行复制左侧邻域。
fn predict_intra16x16_horizontal(left: &[u8; 16]) -> [u8; 256] {
    let mut out = [0u8; 256];
    fill_horizontal(&mut out, left, 16);
    out
}

/// 16x16 DC：上方 + 左侧 32 样本均值（四舍五入 `+16` 后除 32）。
///
/// 累加部分与 [`predict_i4x4_dc`] / [`predict_chroma_dc`] 共用
/// [`mean_of_sides`]，这里只保留 16x16 的归一规则（32 样本、`+16` 舍入）。
fn predict_intra16x16_dc(above: &[u8; 16], left: &[u8; 16]) -> [u8; 256] {
    let sum = sum_of_sides(above, left, 16, 16);
    let dc = ((sum + 16) / 32) as u8;
    [dc; 256]
}

/// 16x16 Plane：见 [`plane_kernel`]（亮度参数：8 项求和、梯度系数 5）。
fn predict_intra16x16_plane(above: &[u8; 16], left: &[u8; 16]) -> [u8; 256] {
    let mut out = [0u8; 256];
    plane_kernel(above, left, &mut out, 16, 8, 5);
    out
}

/// 16x16 帧内预测：返回 `16x16` 预测块。
///
/// `above[16]` / `left[16]` 是可用邻域；不可用样本已由调用方置中性值。
/// 主体只保留「模式 → 预测函数」的调度映射，四种模式的算法分别下沉到
/// [`predict_intra16x16_vertical`] / [`predict_intra16x16_horizontal`] /
/// [`predict_intra16x16_dc`] / [`predict_intra16x16_plane`]。
pub fn predict_intra16x16(mode: Intra16x16Mode, above: &[u8; 16], left: &[u8; 16]) -> [u8; 256] {
    match mode {
        Intra16x16Mode::Vertical => predict_intra16x16_vertical(above),
        Intra16x16Mode::Horizontal => predict_intra16x16_horizontal(left),
        Intra16x16Mode::Dc => predict_intra16x16_dc(above, left),
        Intra16x16Mode::Plane => predict_intra16x16_plane(above, left),
    }
}

/// 8x8 色度 Vertical。
fn predict_chroma_vertical(above: &[u8; 8]) -> [u8; 64] {
    let mut out = [0u8; 64];
    fill_vertical(&mut out, above, 8);
    out
}

/// 8x8 色度 Horizontal。
fn predict_chroma_horizontal(left: &[u8; 8]) -> [u8; 64] {
    let mut out = [0u8; 64];
    fill_horizontal(&mut out, left, 8);
    out
}

/// 8x8 色度 DC：上方 + 左侧样本均值（按实际样本数归一）。
///
/// 与亮度的两处差别：样本总数是实际参与数（不是固定的 32），以及分母为 0 时
/// 回退中性值 128。累加同走 [`sum_of_sides`]。
fn predict_chroma_dc(above: &[u8; 8], left: &[u8; 8]) -> [u8; 64] {
    let cnt = 16u32; // 色度 8x8 上 8 + 左 8
    let sum = sum_of_sides(above, left, 8, 8);
    let dc = if cnt == 0 { 128 } else { ((sum + cnt / 2) / cnt) as u8 };
    [dc; 64]
}

/// 8x8 色度 Plane：见 [`plane_kernel`]（色度参数：4 项求和、梯度系数 34；
/// 与亮度的三处差异——项数、系数 34、方向相反——由参数与取侧约定统一承载）。
fn predict_chroma_plane(above: &[u8; 8], left: &[u8; 8]) -> [u8; 64] {
    let mut out = [0u8; 64];
    plane_kernel(above, left, &mut out, 8, 4, 34);
    out
}

/// 色度帧内预测模式（4 种，与 16x16 同构）。
pub fn predict_chroma(
    mode: Intra16x16Mode,
    above: &[u8; 8],
    left: &[u8; 8],
) -> [u8; 64] {
    match mode {
        Intra16x16Mode::Vertical => predict_chroma_vertical(above),
        Intra16x16Mode::Horizontal => predict_chroma_horizontal(left),
        Intra16x16Mode::Dc => predict_chroma_dc(above, left),
        Intra16x16Mode::Plane => predict_chroma_plane(above, left),
    }
}
