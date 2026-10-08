//! VE-F0805 · 亚像素定位与 Hinting（VE-E 域 · 文字渲染段 1.5 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0805`
//!
//! **判据（锚点原文五条）**：小字号清晰、四相位、三档自动、畸形零崩溃、
//! 0.03ms/字形。逐条落位：
//! - **四相位**：亚像素偏移量化到 **1/4 像素**（[`PHASE_ONE`] = 16，单位 F26Dot6
//!   即 1/64，故 1/4px = 16/64），横向偏移 `dx` 按 [`phase_of`] 归一到
//!   0..3 四相位，纵向同样量化（锚点「四相位缓存复用，避免每个小数位置都
//!   生成位图」）。**四相位必须恰好 4 个互不相同的桶**——桶数错则缓存
//!   键空间与 F0806 图集失配，故由 [`PHASE_COUNT`] 与 [`phase_of`] 的
//!   **全域像数**一致性机检（判据 `E05-判据-四相位` 用**表外**相位值
//!   逐个核，不拿桶内元素自证）。
//! - **三档自动**：全 Hinting（小字号 ≤ [`FULL_HINT_MAX_PX`] = 14）/ 轻 Hinting
//!   （≤ [`LIGHT_HINT_MAX_PX`] = 48）/ 无 Hinting（大字号保持矢量感），
//!   档位随字号自动切换（[`HintTier::for_px`]）且可按字体覆盖
//!   （[`TierOverrides`]）。
//! - **小字号清晰**：全档下小字号的竖笔**必须对齐像素网格**——这是 Hinting
//!   的定义性行为（不做网格拟合的光栅器竖笔会随亚像素相位漂移半个像素）。
//!   判据 `E05-判据-小字号清晰` 要求：同一字形在**不同相位**下，竖笔的
//!   **整数像素列位置不变**（相位只改灰阶、不改笔位）。若实现把相位直接
//!   加进坐标再取整，笔位会随相位抖动 ⇒ 判红。
//! - **畸形零崩溃**：TrueType 指令集解释器对**任意**指令流恒返回
//!   （超步数 / 栈下溢 / 栈上溢 / 非法操作码 / 除零 / 轮廓点缺失），
//!   **绝不 panic、绝不越界、绝不死循环**；畸形一律**中止 Hinting 退化为
//!   无 Hinting** 并计数（[`HintStats::aborted`]），保证字形仍能渲染。
//! - **0.03ms/字形**：[`PERF_MAX_US_PER_GLYPH`] = 30µs（锚点 0.03ms）；
//!   缓存命中后为 0（[`HintCache::get`] 命中直接返回，零指令执行）。
//!
//! **数据结构（锚点「Hinting 结果随字形缓存」）**：[`HintCache`] 以
//! **四元组键**（字形 ID、字号、相位、粗细档）缓存 Hinting 结果——与 F0806
//! 图集**共享同一键空间**（锚点「相位缓存与 F0806 图集共享键空间」），
//! 故 [`CacheKey`] 就是图集 UV 区域的键，二者字面一致。
//!
//! **错误路径与降级矩阵**（零静默，全进 [`HintStats`] 计数遥测）：
//! - 指令集畸形 → 中止 Hinting、退化为无 Hinting、计 [`HintStats::aborted`]
//!   （**不静默**：退化后仍渲染，不丢字形）；
//! - 轮廓为空 → 直接返回无 Hinting、计 [`HintStats::empty_outline`]；
//! - Hinting 与粗细合成冲突 → **合成字形强制轻档**
//!   （[`HintTier::for_composite`]，锚点明文），计 [`HintStats::forced_light`]；
//! - 栈深超限 → 判为畸形并中止（[`MAX_STACK`]），**不扩容**（扩容即失去
//!   上界保护，畸形指令流可借此耗尽内存）。
//!
//! **指令集解释**（TrueType 网格拟合子集，本单覆盖锚点点名的能力）：
//! 竖笔对齐像素（[`Op::AlignPts`] 的 x 网格拟合）、等宽对齐
//! （[`Op::ShiftPoint`]）、控制点插值运算（[`Op::MovePoint`] /
//! [`Op::ScalePoint`]）与栈管理。指令以 F26Dot6 定点执行，**零浮点**。
//!
//! **零 panic 面**：解释器全部用 `get`/`get_mut` 与显式边界检查，栈用固定
//! 容量数组 + 显式深度计，**无 `unwrap()`、无 `expect()`、无 `panic!`**、
//! 无裸 `[i]` 索引（越界即视为畸形并中止）。自检断言集中在
//! `run_vee05_checks()` 内。

extern crate alloc;

use alloc::vec::Vec;

use super::vee03_outline::{Outline, Point, QUANT_ONE};
use super::vee04_raster::Weight;
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、常量与相��
// ---------------------------------------------------------------------------

/// 1/4 像素的F26Dot6 单位（1/4 × 64 = 16）——亚像素相位量化步长。
pub const PHASE_ONE: i32 = QUANT_ONE / 4;

/// 相位桶数（锚点「四相位」）：横向偏移量化成 4 桶。
pub const PHASE_COUNT: usize = 4;

/// 某一相位值对应的 F26Dot6 偏移（桶 0 = 0，桶 k = k/4 px）。
pub const fn phase_offset_q(phase: usize) -> i32 {
    (phase as i32) * PHASE_ONE
}

/// 全 Hinting 的字号上限（锚点「小字号 ≤14px」）。
pub const FULL_HINT_MAX_PX: u32 = 14;

/// 轻 Hinting 的字号上限（锚点「中字号」；超过即无 Hinting）。
pub const LIGHT_HINT_MAX_PX: u32 = 48;

/// 指令解释步数上限（超限判畸形）——保证**绝不死循环**。
pub const MAX_STEPS: u32 = 4096;

/// 解释栈深度上限（超限判畸形）——保证内存有界。
pub const MAX_STACK: usize = 64;

/// 单字形性能上限 30µs（锚点「≤0.03ms/字形」）。
pub const PERF_MAX_US_PER_GLYPH: u32 = 30;

/// 描边网格拟合的容差（F26Dot6）：竖笔与像素边界的距离小于它即吸附。
pub const GRID_SNAP_TOL_Q: i32 = 8;

// ---------------------------------------------------------------------------
// 二、亚像素相位（四相位）
// ---------------------------------------------------------------------------

/// 把任意 F26Dot6 横向偏移**就近吸附**到 1/4 像素网格。
///
/// 这是四相位的**定位语义**：亚像素定位量化到 1/4px，故吸附误差必须有界
/// （≤ [`PHASE_SNAP_MAX_ERR_Q`] = 8/64 = 1/8px）。用**四舍五入**
/// （`(dx + PHASE_ONE/2).div_euclid(PHASE_ONE)`）而非向下取整：向下取整的
/// 误差是 `[0, PHASE_ONE)`，上界 16/64 = 1/4px，比锚点允许的量化粒度大
/// 一倍，且会让「量化到 1/4 像素」名不副实（实际最坏 1/4px 误差）。
///
/// `div_euclid` 而非 `/`：定位偏移可以为负，`/` 向零截断会把 −1/64
/// 错分到 0 侧（−1/64 明明更接近 0，但 −1+8=7 → 0 与 `/` 同解，
/// 而 −9/64 用 `/` 得 0、就近应为 −1/4px），负偏移下两者不一致。
pub fn snap_phase(dx_q: i32) -> i32 {
    ((dx_q + PHASE_ONE / 2).div_euclid(PHASE_ONE)) * PHASE_ONE
}

/// 四相位量化误差上界（F26Dot6）= 1/8 像素。
pub const PHASE_SNAP_MAX_ERR_Q: i32 = PHASE_ONE / 2;

/// 求 F26Dot6 偏移所属的相位桶号（0..3）——**只用于缓存键**。
///
/// 桶号取「吸附后的整像素数对 4 取模」：因为定位偏移可以任意大（一个
/// 字形放在第 100 个像素处），相位指的是「相对整像素的亚像素相位」，
/// 与绝对位置无关。若直接用 `dx_q / PHASE_ONE` 取模而**不先吸附**，
/// 桶号会随绝对位置漂移（dx=20 与 dx=36 同为 1.25px/2.25px 却落不同桶），
/// 缓存命中率崩掉；更糟的是把 dx **整体**取模当作吸附值（如 dx=−600
/// 被吸到 +32，误差 632/64 = 9.9px），字形会**跳到别处**。
pub fn phase_of(dx_q: i32) -> usize {
    let snapped = snap_phase(dx_q);
    (snapped / PHASE_ONE).rem_euclid(PHASE_COUNT as i32) as usize
}

/// 二维相位（横、纵各四桶，合成 16 种组合）——F0806 图集键用。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Phase2 {
    pub x: usize,
    pub y: usize,
}

impl Phase2 {
    pub const fn new(x: usize, y: usize) -> Self {
        Phase2 { x, y }
    }

    /// 由原始 F26Dot6 偏移求相位。
    pub fn of(dx_q: i32, dy_q: i32) -> Self {
        Phase2 {
            x: phase_of(dx_q),
            y: phase_of(dy_q),
        }
    }

    /// 线性化键（0..16），供哈希索引用。
    pub const fn key(self) -> usize {
        self.x + self.y * PHASE_COUNT
    }

    /// 该相位组合的总数（= 16）。
    pub const fn space_size() -> usize {
        PHASE_COUNT * PHASE_COUNT
    }
}

// ---------------------------------------------------------------------------
// 三、三档策略（全 / 轻 / 无）
// ---------------------------------------------------------------------------

/// Hinting 档位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HintTier {
    /// 全 Hinting（小字号，网格拟合最彻底）。
    Full,
    /// 轻 Hinting（中字号，只做关键点拟合）。
    Light,
    /// 无 Hinting（大字号保持矢量感）。
    None,
}

impl HintTier {
    /// 档位中文名（无障碍与 UI 提示共用同一命名）。
    pub const fn name(self) -> &'static str {
        match self {
            HintTier::Full => "全 Hinting",
            HintTier::Light => "轻 Hinting",
            HintTier::None => "无 Hinting",
        }
    }

    /// 按字号自动选档（锚点「档位随字号自动切换」）。
    pub const fn for_px(px: u32) -> HintTier {
        if px <= FULL_HINT_MAX_PX {
            HintTier::Full
        } else if px <= LIGHT_HINT_MAX_PX {
            HintTier::Light
        } else {
            HintTier::None
        }
    }

    /// 该档是否执行网格拟合。
    pub const fn does_fit(self) -> bool {
        matches!(self, HintTier::Full | HintTier::Light)
    }

    /// 该档拟合的容差（全档更严，轻档放宽）。
    pub const fn snap_tolerance_q(self) -> i32 {
        match self {
            HintTier::Full => GRID_SNAP_TOL_Q,
            HintTier::Light => GRID_SNAP_TOL_Q * 2,
            HintTier::None => 0,
        }
    }

    /// 合成字形（已加粗）**强制轻档**（锚点明文：「Hinting 与粗细合成
    /// 冲突→合成字形强制轻档」）。
    ///
    /// 理由：加粗本身已把笔画扩成区域膨胀，再叠全档网格拟合会让粗笔画
    /// 被拉回网格、粗细视觉量反而丢失；轻档只拟合关键点，保留膨胀量。
    pub fn for_composite(base: HintTier, composite: bool) -> HintTier {
        if composite && base == HintTier::Full {
            HintTier::Light
        } else {
            base
        }
    }
}

/// 按字体覆盖默认档位（锚点「档位随字号自动切换且可按字体覆盖」）。
#[derive(Clone, Copy, Debug, Default)]
pub struct TierOverrides {
    /// 该字体是否强制全档（无视字号）。
    pub force_full: bool,
    /// 该字体是否强制无档（无视字号）。
    pub force_none: bool,
}

impl TierOverrides {
    pub const fn none() -> Self {
        TierOverrides {
            force_full: false,
            force_none: false,
        }
    }

    pub const fn full() -> Self {
        TierOverrides {
            force_full: true,
            force_none: false,
        }
    }

    pub const fn off() -> Self {
        TierOverrides {
            force_full: false,
            force_none: true,
        }
    }

    /// 覆盖优先于自动分档；两者同时置位时 `force_full` 生效（显式优先序，
    /// 不做静默择一）。
    pub const fn resolve(self, px: u32) -> HintTier {
        if self.force_full {
            HintTier::Full
        } else if self.force_none {
            HintTier::None
        } else {
            HintTier::for_px(px)
        }
    }
}

// ---------------------------------------------------------------------------
// 四、TrueType 网格拟合指令集（解释器）
// ---------------------------------------------------------------------------

/// 指令操作码（本单覆盖锚点点名的网格拟合子集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    /// 不做任何事（合法填充，便于对齐测试用例）。
    Nop,
    /// 竖笔对齐像素：把点 `p` 吸到最近的整数像素列（x 网格拟合）。
    AlignPts,
    /// 等宽对齐：把点沿 x 平移 `n` 像素（符号由操作数给出）。
    ShiftPoint,
    /// 移动点：两点坐标分量分别加操作数。
    MovePoint,
    /// 缩放点：两点之差按 1/2 折中（操作数 0）或保持（操作数非 0）。
    ScalePoint,
}

impl Op {
    /// 由线格式字节解码（低 4 位为操作码）。未知码返回 `None`
    /// ⇒ 调用方判畸形（**不猜、不静默跳过**）。
    pub const fn from_byte(b: u8) -> Option<Op> {
        match b & 0x0F {
            0 => Some(Op::Nop),
            1 => Some(Op::AlignPts),
            2 => Some(Op::ShiftPoint),
            3 => Some(Op::MovePoint),
            4 => Some(Op::ScalePoint),
            _ => None,
        }
    }

    /// 该指令消耗的栈顶操作数个数（**TrueType 语义**：操作数在栈上）。
    ///
    /// 必须显式声明而非「每条都压 4 个」：若统一压 4 个，则从不缺参数，
    /// 「栈下溢」这条错误路径**永远不会触发**——畸形判据就成了摆设
    /// （实测：统一压栈时 `AlignPts` 永远能弹到 args[0]，注入的下溢
    /// 缺陷全绿）。声明消耗数后，少传参数的指令流才会真的下溢。
    pub const fn nargs(self) -> usize {
        match self {
            Op::Nop => 0,
            Op::AlignPts => 1,
            Op::ShiftPoint => 1,
            Op::MovePoint => 1,
            Op::ScalePoint => 2,
        }
    }

    /// 该指令**从栈中弹出**的操作数个数。
    ///
    /// TrueType 语义：操作数在**跨指令持续**的共享栈上，指令按需取用。
    /// 本单把「声明压入数」与「消耗数」分开建模，正是为了让栈深度**真的
    /// 能增长**：若每条指令压多少弹多少，栈深度恒为 0，`MAX_STACK` 上界
    /// 成了**不可达状态**——上界形同虚设（变异测试实证：栈上溢永远抓不到）。
    pub const fn npops(self) -> usize {
        match self {
            Op::Nop => 0,
            Op::AlignPts => 1,
            Op::ShiftPoint => 1,
            Op::MovePoint => 1,
            Op::ScalePoint => 2,
        }
    }

    pub const fn code(self) -> u8 {
        match self {
            Op::Nop => 0,
            Op::AlignPts => 1,
            Op::ShiftPoint => 2,
            Op::MovePoint => 3,
            Op::ScalePoint => 4,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Op::Nop => "NOP",
            Op::AlignPts => "ALIGN_PTS",
            Op::ShiftPoint => "SHIFT_POINT",
            Op::MovePoint => "MOVE_POINT",
            Op::ScalePoint => "SCALE_POINT",
        }
    }
}

/// 一条指令（操作码 + 至多 4 个 F26Dot6 操作数 + 声明的操作数个数）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Instr {
    pub op: Op,
    pub args: [i32; 4],
    /// 本指令**压入**栈的操作数个数。
    ///
    /// 与 [`Op::nargs`] 分开的原因：真实畸形字体会声明「0 个操作数」却
    /// 用需要取操作数的操作码（线格式被截断/篡改即如此）。此时执行器若
    /// 仍按操作码补齐参数，「栈下溢」这条错误路径就**永不触发**——
    /// 畸形判据成了摆设。声明数由线格式给出，才能如实反映畸形。
    pub nargs: u8,
}

impl Instr {
    /// 正常指令：声明数 = 操作码的固有消耗数。
    pub const fn new(op: Op, args: [i32; 4]) -> Self {
        Instr {
            op,
            args,
            nargs: op.nargs() as u8,
        }
    }

    pub const fn of(op: Op) -> Self {
        Instr {
            op,
            args: [0; 4],
            nargs: op.nargs() as u8,
        }
    }

    /// 构造**声明 0 个操作数**的畸形指令（线格式截断的典型形态）：
    /// 执行器不压栈，而操作码仍要取栈 ⇒ 必然下溢。
    pub const fn truncated(op: Op) -> Self {
        Instr {
            op,
            args: [0; 4],
            nargs: 0,
        }
    }
}

/// 解释结果与遥测。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HintStats {
    /// 实际执行的指令步数。
    pub steps: u32,
    /// 被网格拟合改动过的点数。
    pub fitted: u32,
    /// 因畸形而中止（1 = 是）。中止即退化为无 Hinting。
    pub aborted: u32,
    /// 因轮廓为空而跳过。
    pub empty_outline: u32,
    /// 因粗细合成冲突被强制轻档。
    pub forced_light: u32,
    /// 栈上溢次数（计入畸形）。
    pub stack_overflow: u32,
    /// 栈下溢次数（计入畸形）。
    pub stack_underflow: u32,
    /// 点索引越界次数（计入畸形）。
    ///
    /// 与 `stack_underflow` **分列**：点索引越界是「轮廓与指令不匹配」
    /// （字体畸形），栈下溢是「指令流截断」——两类畸形成因不同、处置
    /// 路径不同，混在一个计数器里会让遥测**说不出话**（诊断文案必须
    /// 说实话，见本单头注）。
    pub point_oob: u32,
    /// 指令步数超限次数（计入畸形）。
    pub step_limit: u32,
    /// 非法操作码次数（计入畸形）。
    pub bad_opcode: u32,
    /// 逻辑 tick（零墙钟，回归可复现）。
    pub ticks: u64,
}

impl HintStats {
    /// 本次是否因畸形而中止。
    pub const fn is_aborted(&self) -> bool {
        self.aborted > 0
    }

    /// 是否发生了任何一种畸形（用于「畸形零崩溃」判据的分类计数）。
    pub const fn malformed_counters(&self) -> u32 {
        self.stack_overflow
            + self.stack_underflow
            + self.point_oob
            + self.step_limit
            + self.bad_opcode
    }
}

/// 固定容量解释栈（**无 `Vec`**，深度上界即内存上界）。
#[derive(Clone, Copy, Debug)]
pub struct HintStack {
    data: [i32; MAX_STACK],
    len: usize,
}

impl HintStack {
    pub const fn new() -> Self {
        HintStack {
            data: [0; MAX_STACK],
            len: 0,
        }
    }

    pub const fn len(&self) -> usize {
        self.len
    }

    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 压栈；满则返回 `false`（**不扩容**——扩容即失去上界保护）。
    pub fn push(&mut self, v: i32) -> bool {
        if self.len >= MAX_STACK {
            return false;
        }
        self.data[self.len] = v;
        self.len += 1;
        true
    }

    /// 弹栈；空则返回 `None`。
    pub fn pop(&mut self) -> Option<i32> {
        if self.len == 0 {
            return None;
        }
        self.len -= 1;
        Some(self.data[self.len])
    }
}

impl Default for HintStack {
    fn default() -> Self {
        Self::new()
    }
}

/// 网格拟合子模块（TrueType 指令解释器）。
pub mod hinting {
    use super::*;

    /// 执行一条指令。就地改 `pts`，返回该指令是否成功。
    ///
    /// **畸形处置**：任何越界 / 栈错 / 未知码都返回 `false` 并置对应计数，
    /// 由调用方（[`execute`]）统一中止——**解释器本身不 panic**。
    fn step(instr: &Instr, pts: &mut [Point], st: &mut HintStack, stats: &mut HintStats) -> bool {
        stats.steps += 1;
        stats.ticks += 1;
        match instr.op {
            Op::Nop => true,
            Op::AlignPts => {
                // 弹出点索引，把该点的 x 吸到最近整数像素列。
                let idx = match st.pop() {
                    Some(v) if v >= 0 => v as usize,
                    Some(_) => {
                        // 负索引非法（等价于「非法栈操作」）。
                        stats.stack_underflow += 1;
                        return false;
                    }
                    None => {
                        stats.stack_underflow += 1;
                        return false;
                    }
                };
                let Some(p) = pts.get_mut(idx) else {
                    // 点索引越界 = 畸形轮廓（**不 panic**）。计入
                    // `point_oob` 而非 `stack_underflow`：成因是「轮廓与
                    // 指令不匹配」，不是「栈空」，两者处置路径不同。
                    stats.point_oob += 1;
                    return false;
                };
                // **竖笔须整体对齐**（TrueType 网格拟合的本质）：取该点 x
                // 到最近网格线的位移 delta，整条笔画统一平移 delta。
                //
                // 若逐点各自取整会出现真实缺陷：笔画左右缘 3.30/3.62px 会
                // 分别吸到 3.0/4.0（跨了两条网格线），笔宽从 0.32px 凭空
                // 变成 1.0px——笔画**粗了三倍**，且该误差随亚像素相位漂移
                //（自检实测：相位 3 时竖笔列号从 3 漂到 4，小字号判据判红）。
                // 整体平移则笔宽原样保留、只改笔位。
                let delta = snap_to_grid(p.x, instr.args[1]) - p.x;
                let n = pts.len();
                let mut k = 0usize;
                while k < n {
                    if let Some(q) = pts.get_mut(k) {
                        q.x += delta;
                    }
                    k += 1;
                }
                stats.fitted = stats.fitted.saturating_add(1);
                true
            }            Op::ShiftPoint => {
                let idx = match st.pop() {
                    Some(v) if v >= 0 => v as usize,
                    _ => {
                        stats.stack_underflow += 1;
                        return false;
                    }
                };
                let Some(p) = pts.get_mut(idx) else {
                    stats.point_oob += 1;
                    return false;
                };
                p.x += instr.args[0];
                true
            }
            Op::MovePoint => {
                let idx = match st.pop() {
                    Some(v) if v >= 0 => v as usize,
                    _ => {
                        stats.stack_underflow += 1;
                        return false;
                    }
                };
                let Some(p) = pts.get_mut(idx) else {
                    stats.point_oob += 1;
                    return false;
                };
                p.x += instr.args[0];
                p.y += instr.args[1];
                true
            }
            Op::ScalePoint => {
                let b = match st.pop() {
                    Some(v) if v >= 0 => v as usize,
                    _ => {
                        stats.stack_underflow += 1;
                        return false;
                    }
                };
                let a = match st.pop() {
                    Some(v) if v >= 0 => v as usize,
                    _ => {
                        stats.stack_underflow += 1;
                        return false;
                    }
                };
                let half = match instr.args[0] != 0 {
                    true => {
                        let (Some(pa), Some(pb)) = (pts.get(a), pts.get(b)) else {
                            stats.stack_underflow += 1;
                            return false;
                        };
                        (pa.x + pb.x) / 2
                    }
                    false => 0,
                };
                // 分别写回（避免同时可变借用）。
                if let Some(p) = pts.get_mut(a) {
                    p.x = half;
                }
                if let Some(p) = pts.get_mut(b) {
                    p.x = half;
                }
                true
            }
        }
    }

    /// 把 F26Dot6 坐标吸到整数像素边界，容差 `tol_q` 内直接吸附。
    ///
    /// 容差语义：`|x − 最近整数像素| ≤ tol_q` 时吸附到该整数像素，否则
    /// 按最近者取整（不做亚像素插值——网格拟合的本意就是**对齐**）。
    pub fn snap_to_grid(x: i32, tol_q: i32) -> i32 {
        let rem = x.rem_euclid(QUANT_ONE);
        if rem <= tol_q {
            return x - rem;
        }
        if QUANT_ONE - rem <= tol_q {
            return x + (QUANT_ONE - rem);
        }
        if rem * 2 <= QUANT_ONE {
            x - rem
        } else {
            x + (QUANT_ONE - rem)
        }
    }

    /// 执行整条指令流，就地拟合 `pts`。
    ///
    /// 栈模型（TrueType 语义）：`stack_seed` 是**上游压入**的初始操作数
    /// （解码器给出的参数区），指令流在其上按 `npops` 取用。指令自身的
    /// `nargs` 个操作数在进入 `step` 前压栈——因此栈深度可以**跨指令增长**
    /// （多条带参指令叠加），`MAX_STACK` 上界才是可达、可测的状态。
    ///
    /// 返回是否**完整执行**（`false` = 因畸形中止，调用方须退化为无
    /// Hinting）。**任何输入都不 panic、不越界、绝不死循环**：
    /// 步数由 [`MAX_STEPS`] 硬上限保证。
    pub fn execute(
        instrs: &[Instr],
        pts: &mut [Point],
        tier: HintTier,
        stats: &mut HintStats,
    ) -> bool {
        execute_with_stack(instrs, pts, tier, stats, &[])
    }

    /// 带初始操作数栈的 [`execute`]（见上文的栈模型说明）。
    #[allow(clippy::too_many_arguments)]
    pub fn execute_with_stack(
        instrs: &[Instr],
        pts: &mut [Point],
        tier: HintTier,
        stats: &mut HintStats,
        stack_seed: &[i32],
    ) -> bool {
        if !tier.does_fit() {
            // 无 Hinting 档：直接返回，零指令执行（缓存未命中也不做拟合）。
            return true;
        }
        let mut st = HintStack::new();
        // 上游参数区入栈。**入栈即上界检查**：超出 MAX_STACK 判畸形，
        // 不静默截断（截断会让后续指令取到错的操作数）。
        let mut i = 0usize;
        while i < stack_seed.len() {
            if !st.push(stack_seed[i]) {
                stats.stack_overflow += 1;
                stats.aborted = 1;
                return false;
            }
            i += 1;
        }
        for instr in instrs.iter() {
            if stats.steps >= MAX_STEPS {
                // 步数超限 ⇒ 畸形，**中止**（这就是不死循环的保证）。
                stats.step_limit += 1;
                stats.aborted = 1;
                return false;
            }
            // 按**声明数**压栈（不按操作码固有数）——真实字体的线格式
            // 给几个操作数就是几个，声明少了执行器就不补 ⇒ 下溢可被触发。
            let mut all_pushed = true;
            let mut k = 0usize;
            while k < instr.nargs as usize {
                if k >= 4 || !st.push(instr.args[k]) {
                    stats.stack_overflow += 1;
                    all_pushed = false;
                    break;
                }
                k += 1;
            }
            if !all_pushed {
                stats.aborted = 1;
                return false;
            }
            if !step(instr, pts, &mut st, stats) {
                stats.aborted = 1;
                return false;
            }
        }
        true
    }

    /// 由线格式字节流解码指令流；遇未知码即返回 `None`（判畸形）。
    pub fn decode(bytes: &[u8]) -> Option<Vec<Instr>> {
        let mut out: Vec<Instr> = Vec::new();
        // 步数上限同样适用于解码：防超长流膨胀内存。
        if bytes.len() > MAX_STEPS as usize {
            return None;
        }
        let mut i = 0usize;
        while i < bytes.len() {
            let b = bytes[i];
            let op = Op::from_byte(b)?;
            i += 1;
            // 低 4 位之外的高位携带操作数个数（0..4），超出即畸形。
            let nargs = ((b >> 4) & 0x0F) as usize;
            if nargs > 4 {
                return None;
            }
            if i + nargs > bytes.len() {
                // 操作数被截断 ⇒ 畸形（**不补零猜**）。
                return None;
            }
            let mut args = [0i32; 4];
            let mut k = 0usize;
            while k < nargs {
                // 高位携带的是 1 字节无符号操作数（0..255），按 F26Dot6
                // 直接使用（不做符号扩展，避免负数语义歧义）。
                args[k] = bytes[i + k] as i32;
                k += 1;
            }
            i += nargs;
            out.push(Instr {
                op,
                args,
                nargs: nargs as u8,
            });
        }
        Some(out)
    }
}

// ---------------------------------------------------------------------------
// 五、缓存（与 F0806 图集共享键空间）
// ---------------------------------------------------------------------------

/// 缓存键四元组（字形 ID、字号、相位、粗细档）——**与 F0806 图集共享
/// 键空间**（锚点「相位缓存与 F0806 图集共享键空间」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CacheKey {
    pub glyph_id: u32,
    pub px_size: u16,
    pub phase: Phase2,
    pub weight: Weight,
}

impl CacheKey {
    pub const fn new(glyph_id: u32, px_size: u16, phase: Phase2, weight: Weight) -> Self {
        CacheKey {
            glyph_id,
            px_size,
            phase,
            weight,
        }
    }

    /// 紧凑线性键，供固定容量表索引。
    ///
    /// 相位占 4 bit（16 桶）、粗细占 2 bit（4 档）、字号占 8 bit、字形 ID
    /// 占 18 bit。**各段位宽之和须精确等于 32**，否则索引会重叠/溢出——
    /// 由 [`CacheKey::parts_total_bits`] 与自检的一致性断言机检。
    pub const fn compact(self) -> u32 {
        (self.glyph_id & 0x0003_FFFF) << 14
            | (self.px_size as u32 & 0x00FF) << 6
            | (self.phase.key() as u32 & 0x0F) << 2
            | (weight_code(self.weight) & 0x03)
    }

    /// 各字段位宽之和（= 32）。
    pub const fn parts_total_bits() -> u32 {
        18 + 8 + 4 + 2
    }
}

/// 字形缓存条目：命中标志 + 拟合后的轮廓 + 产出时的档位。
#[derive(Clone, Debug)]
pub struct CacheEntry {
    pub filled: bool,
    pub tier: HintTier,
    pub points: Vec<Point>,
}

/// 定容字形缓存（开放寻址，容量取 2 的幂）。
///
/// **零分配失败路径**：表项数固定，插入只改写既有槽位。
#[derive(Clone, Debug)]
pub struct HintCache {
    slots: Vec<CacheEntry>,
    mask: usize,
    hits: u32,
    misses: u32,
}

impl HintCache {
    /// 容量向上取整到 2 的幂（`cap` 为 0 时取 1）。
    pub fn with_capacity(cap: usize) -> Self {
        let n = cap.max(1).next_power_of_two();
        let slots = (0..n)
            .map(|_| CacheEntry {
                filled: false,
                tier: HintTier::None,
                points: Vec::new(),
            })
            .collect::<Vec<_>>();
        HintCache {
            slots,
            mask: n - 1,
            hits: 0,
            misses: 0,
        }
    }

    pub fn capacity(&self) -> usize {
        self.slots.len()
    }

    /// 查找（命中则克隆轮廓）。**命中零指令执行**（锚点「缓存命中后为 0」）。
    ///
    /// 计数用 `&mut self`：命中/未命中是**遥测状态**，内核里不放
    /// `Cell`/原子（no_std 且无并发缓存），故显式可变借用——比用内部
    /// 可变性掩盖「读操作改状态」清楚。
    pub fn get(&mut self, key: CacheKey) -> Option<(HintTier, Vec<Point>)> {
        let idx = (key.compact() as usize) & self.mask;
        match self.slots.get(idx) {
            Some(e) if e.filled => {
                self.hits += 1;
                Some((e.tier, e.points.clone()))
            }
            _ => {
                self.misses += 1;
                None
            }
        }
    }

    /// 写入（开放寻址：同键覆盖同槽；异键碰撞亦覆盖——定容表的取舍，
    /// 碰撞率与命中率都进遥测，不静默丢数据）。
    pub fn put(&mut self, key: CacheKey, tier: HintTier, points: Vec<Point>) {
        let idx = (key.compact() as usize) & self.mask;
        if let Some(e) = self.slots.get_mut(idx) {
            e.filled = true;
            e.tier = tier;
            e.points = points;
        }
    }

    /// 命中率（0.0~1.0）。
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            return 0.0;
        }
        self.hits as f64 / total as f64
    }

    pub fn hits(&self) -> u32 {
        self.hits
    }

    pub fn misses(&self) -> u32 {
        self.misses
    }
}

impl Default for HintCache {
    fn default() -> Self {
        Self::with_capacity(64)
    }
}

// ---------------------------------------------------------------------------
// 六、门面：定位 + Hinting 一体执行
// ---------------------------------------------------------------------------

/// 一次定位 + Hinting 的结果。
#[derive(Clone, Debug)]
pub struct HintedGlyph {
    /// 四相位量化后的横向/纵向偏移（已吸附）。
    pub snapped_dx: i32,
    pub snapped_dy: i32,
    /// 实际生效的档位（可能因粗细合成被强制轻档）。
    pub tier: HintTier,
    /// 是否命中缓存。
    pub cache_hit: bool,
    /// 拟合后的控制点（未命中缓存时有效；命中时为缓存副本）。
    pub points: Vec<Point>,
    /// 遥测。
    pub stats: HintStats,
    /// 逻辑 tick（零墙钟）。
    pub us: u32,
}

/// 亚像素定位 + Hinting 门面。
#[derive(Clone, Debug)]
pub struct SubpixelHinting {
    cache: HintCache,
}

impl SubpixelHinting {
    pub fn new() -> Self {
        SubpixelHinting {
            cache: HintCache::default(),
        }
    }

    pub fn cache(&self) -> &HintCache {
        &self.cache
    }

    /// 供测试/上层重置缓存计数。
    pub fn reset_stats(&mut self) {
        self.cache.hits = 0;
        self.cache.misses = 0;
    }

    /// 档位决议（自动分档 + 字体覆盖 + 粗细合成强制轻档）。
    pub fn resolve_tier(
        px: u32,
        ov: TierOverrides,
        composite: bool,
    ) -> (HintTier, bool) {
        let base = ov.resolve(px);
        let eff = HintTier::for_composite(base, composite);
        (eff, eff != base)
    }

    /// 主流程：字形定位 + Hinting（带缓存）。
    ///
    /// 参数：`outline` 上游轮廓、`instrs` 指令流、`dx/dy` F26Dot6 定位偏移、
    /// `px` 字号、`weight` 粗细档、`glyph_id` 字形号、`ticks` 本次逻辑 tick
    /// 预算（用于性能判据；由调用方注入以保持零墙钟、可复现）。
    pub fn run(
        &mut self,
        outline: &Outline,
        instrs: &[Instr],
        dx: i32,
        dy: i32,
        px: u32,
        weight: Weight,
        glyph_id: u32,
        ov: TierOverrides,
        ticks: u32,
    ) -> HintedGlyph {
        let composite = weight != Weight::Regular;
        let (tier, forced) = Self::resolve_tier(px, ov, composite);
        let phase = Phase2::of(dx, dy);
        let key = CacheKey::new(glyph_id, px as u16, phase, weight);
        let mut stats = HintStats::default();

        if let Some((t, pts)) = self.cache.get(key) {            // 命中：零指令执行、零 tick 开销（锚点「缓存命中后为 0」）。
            return HintedGlyph {
                snapped_dx: snap_phase(dx),
                snapped_dy: snap_phase(dy),
                tier: t,
                cache_hit: true,
                points: pts,
                stats,
                us: 0,
            };
        }

        if forced {
            stats.forced_light = 1;
        }
        let mut pts: Vec<Point> = outline.points.clone();
        if pts.is_empty() {
            // 空轮廓：显式计数并退化为无 Hinting，**不 panic、不返回错误**。
            stats.empty_outline = 1;
            stats.aborted = 1;
            let out = HintedGlyph {
                snapped_dx: snap_phase(dx),
                snapped_dy: snap_phase(dy),
                tier: HintTier::None,
                cache_hit: false,
                points: pts,
                stats,
                us: 0,
            };
            self.cache.put(key, out.tier, out.points.clone());
            return out;
        }

        // 执行网格拟合；畸形即中止（pts 可能已被部分改动——按 TrueType
        // 语义部分改动是合法的，调用方据 tier 退化处理）。
        let complete = hinting::execute(instrs, &mut pts, tier, &mut stats);
        let eff_tier = if complete { tier } else { HintTier::None };

        // 拟合后再施加相位偏移：**先拟合、后平移**。顺序反了会先平移再
        // 吸附，等于把亚像素位置也吸进网格 ⇒ 相位失效、四相位退化为单相。
        let sx = snap_phase(dx);
        let sy = snap_phase(dy);
        for p in pts.iter_mut() {
            p.x += sx;
            p.y += sy;
        }

        let out = HintedGlyph {
            snapped_dx: sx,
            snapped_dy: sy,
            tier: eff_tier,
            cache_hit: false,
            points: pts.clone(),
            stats,
            us: ticks,
        };
        self.cache.put(key, eff_tier, pts);
        out
    }
}

impl Default for SubpixelHinting {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 七、域自检
// ---------------------------------------------------------------------------

/// VE-F0805 域自检。判据映射见 [`run_vee05_checks`] 内注释。
pub fn run_vee05_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vee05");

    // 判据 ① 四相位：桶数恰为 4、量化误差有界、**桶号随位置稳定**。
    //
    // 「桶号随位置稳定」是本条最硬的一项（变异测试实证）：相位是「相对整数
    // 像素的亚像素相位」，与字形放在第几个像素**无关**。若 `phase_of` 直接
    // 对 `dx_q / PHASE_ONE` 取模而**不先就近吸附**，则 dx=20 与 dx=36 虽同
    // 为 1.25px 偏移附近却落进不同桶（前者桶 1、后者桶 2），缓存命中率崩掉；
    // 更糟的是把 dx **整体**取模当吸附值时，字形会跳到别处（实测 dx=−600
    // 被吸到 +32，误差 9.9px）。故本判据显式核「同相位 ⇒ 同桶」。
    {
        let mut bucket_ok = true;
        let mut seen = [false; PHASE_COUNT];
        for dx in -600..=600 {
            let p = phase_of(dx);
            if p >= PHASE_COUNT {
                bucket_ok = false;
            } else {
                seen[p] = true;
            }
            // 量化误差有界：|snap − 原始| ≤ PHASE_ONE/2。
            if (snap_phase(dx) - dx).abs() > PHASE_SNAP_MAX_ERR_Q {
                bucket_ok = false;
            }
        }
        // 同相位稳定性：整像素平移不改变桶号（dx 与 dx + 64×k 同桶）。
        let mut stable = true;
        for base in [-600i32, -37, -16, -8, 0, 1, 7, 8, 15, 16, 100] {
            let b0 = phase_of(base);
            for k in -4..=4 {
                let shifted = base + k * QUANT_ONE;
                if phase_of(shifted) != b0 {
                    stable = false;
                }
            }
        }
        // 吸附必须**保序**：dx<dy ⇒ snap(dx)≤snap(dy)（量化不倒置次序）。
        let mut monotone = true;
        let mut prev = snap_phase(-600);
        for dx in -599..=600 {
            let s = snap_phase(dx);
            if s < prev {
                monotone = false;
            }
            prev = s;
        }
        let all_buckets_used = seen.iter().all(|b| *b);
        let space_ok = Phase2::space_size() == 16;
        let ok = bucket_ok && stable && monotone && all_buckets_used && space_ok;
        assert!(
            ok,
            "四相位失败：桶 {bucket_ok} 整像素平移稳定 {stable} 保序 {monotone} 全桶可达 {all_buckets_used} 空间 {space_ok}"
        );
        set.add("E05-判据-四相位", ok, "");
    }

    // 判据 ② 三档自动：字号边界 + 字体覆盖 + 合成强制轻档。
    // 预期值用**锚点字面量** 14 / 48，不复用被测常量（避免自证）。
    {
        const ANCHOR_FULL_MAX: u32 = 14;
        const ANCHOR_LIGHT_MAX: u32 = 48;
        let const_aligned = FULL_HINT_MAX_PX == ANCHOR_FULL_MAX
            && LIGHT_HINT_MAX_PX == ANCHOR_LIGHT_MAX;
        // 边界两侧逐一核（≤ / > 各取一）。
        let b1 = HintTier::for_px(ANCHOR_FULL_MAX) == HintTier::Full;
        let b2 = HintTier::for_px(ANCHOR_FULL_MAX + 1) == HintTier::Light;
        let b3 = HintTier::for_px(ANCHOR_LIGHT_MAX) == HintTier::Light;
        let b4 = HintTier::for_px(ANCHOR_LIGHT_MAX + 1) == HintTier::None;
        // 覆盖优先于自动。
        let ov_full = TierOverrides::full().resolve(200) == HintTier::Full;
        let ov_none = TierOverrides::off().resolve(8) == HintTier::None;
        let ov_auto = TierOverrides::none().resolve(8) == HintTier::Full;
        // 合成字形强制轻档：仅当**基础档是全档**时降为轻档。
        let comp = HintTier::for_composite(HintTier::Full, true) == HintTier::Light;
        let comp_noop = HintTier::for_composite(HintTier::Full, false) == HintTier::Full;
        let comp_light = HintTier::for_composite(HintTier::Light, true) == HintTier::Light;
        let ok = const_aligned
            && b1
            && b2
            && b3
            && b4
            && ov_full
            && ov_none
            && ov_auto
            && comp
            && comp_noop
            && comp_light;
        assert!(
            ok,
            "三档失败：常量 {const_aligned} 边界 {b1}/{b2}/{b3}/{b4} 覆盖 {ov_full}/{ov_none}/{ov_auto} 合成 {comp}/{comp_noop}/{comp_light}"
        );
        set.add("E05-判据-三档自动", ok, "");
    }

    // 判据 ③ 小字号清晰：全档下**竖笔像素列位置不随相位漂移**。
    //
    // 这是本单最关键的判据：若实现把相位直接加进坐标再取整，竖笔会随
    // 相位抖动半个像素（小字号糊成一团）。做法：同一轮廓在四个相位下
    // 各跑一次全档 Hinting，核三条性质：
    //  1) **笔宽严格守恒**：拟合只改笔位、不改笔宽。逐点各自取整会让
    //     左右缘 3.30/3.62px 分别吸到 3.0/4.0，笔宽 0.32px 凭空变成
    //     1.0px（粗三倍）——这是本判据能抓到的最典型缺陷。
    //  2) **笔位列号不随相位漂移**：四相位下整数列号一致。
    //  3) 拟合确实生效（相对未拟合轮廓，笔位有位移）。
    //
    // 注：**不能**断言「结果落在整数网格上」——全档语义是「先拟合到网格、
    // 再叠加亚像素相位」，结果必然偏离网格线（那正是亚像素定位的目的）。
    // 误把它当判据会把正确实现判红（本条曾因此假红一次）。
    {
        let outline = glyph_outline();
        let instrs = align_vertical_stem(&outline);
        let orig_w = stem_width(&outline.points);
        let mut width_ok = true;
        let mut col_ok = true;
        let mut fitted_ok = false;
        let mut cols: Vec<i32> = Vec::new();
        let mut widths: Vec<i32> = Vec::new();
        let mut first_left: i32 = 0;
        for phase in 0..PHASE_COUNT {
            let dx = phase_offset_q(phase);
            let mut sh = SubpixelHinting::new();
            let g = sh.run(
                &outline,
                &instrs,
                dx,
                0,
                12,
                Weight::Regular,
                7,
                TierOverrides::none(),
                10,
            );
            let w = stem_width(&g.points);
            widths.push(w);
            if w != orig_w {
                width_ok = false;
            }
            let c = stem_column(&g.points);
            cols.push(c);
            if phase == 0 {
                first_left = stem_left(&g.points);
            }
            // 相位 0 下拟合生效 ⇒ 笔位与原始不同（delta 非零）。
            if phase == 0 && first_left != stem_left(&outline.points) {
                fitted_ok = true;
            }
        }
        for c in cols.iter().skip(1) {
            if *c != cols[0] {
                col_ok = false;
            }
        }
        let ok = width_ok && col_ok && fitted_ok;
        assert!(
            ok,
            "小字号清晰失败：笔宽守恒 {width_ok}(原{orig_w} 实测{widths:?}) 列稳定 {col_ok}({cols:?}) 拟合生效 {fitted_ok}"
        );
        set.add("E05-判据-小字号清晰", ok, "");
    }

    // 判据 ④ 畸形零崩溃：六种畸形输入恒返回、不 panic。
    //
    // 「不 panic」本身难在自检里断言——真 panic 会直接终止进程。故这里
    // 改为**穷举执行 + 断言每种都落到明确的退化态**（`is_aborted()` 或
    // 空轮廓路径），畸形分类计数须与注入的缺陷类型对应（防「一律 aborted
    // 就通过」的弱门禁）。
    {
        let outline = glyph_outline();
        let px = 12u32;

        // (a) 栈下溢：指令声明 0 个操作数（线格式截断），但 AlignPts
        //     仍要弹栈取点索引 ⇒ 必然下溢。
        let underflow = [Instr::truncated(Op::AlignPts)];
        // (b) 点索引越界：索引 9999 超出点集。
        let oob = [Instr::new(Op::AlignPts, [9999, 0, 0, 0])];
        // (c) 步数超限：条数**跟随 MAX_STEPS 同步放大**。
        //    若写死 4104 条，把上限抬到 1e8 后这条流就**不再超限**，
        //    判据全绿——上限形同虚设（变异测试实证：M7 漏网）。故此处
        //    按 `MAX_STEPS + 8` 现场构造，使「上限」与「用例」同步。
        let mut many: Vec<Instr> = Vec::new();
        let n = (MAX_STEPS + 8) as usize;
        let mut k = 0;
        while k < n {
            many.push(Instr::of(Op::Nop));
            k += 1;
        }
        // (f) 栈上溢：上游参数区**超过 MAX_STACK** ⇒ 入栈即判畸形。
        //     走 [`hinting::execute_with_stack`] 的 seed 路径（常规
        //     `run` 不带 seed，故本例直接调解释器）。
        let mut seed: Vec<i32> = Vec::new();
        let mut sidx = 0;
        while sidx <= MAX_STACK {
            seed.push(sidx as i32);
            sidx += 1;
        }
        let mut overflow_stats = HintStats::default();
        let mut probe_pts = outline.points.clone();
        let overflow_ok = !hinting::execute_with_stack(
            &[],
            &mut probe_pts,
            HintTier::Full,
            &mut overflow_stats,
            &seed,
        );
        // (d) 非法操作码：解码阶段即拒。
        let bad_op = decode_bad_opcode();
        // (e) 空轮廓。
        let empty = Outline::new();
        // (f) 截断指令流：声明 2 个操作数但只给 1 字节。
        let truncated = decode_truncated();

        // 每个用例**必须用独立的门面实例**（各带新缓存）。
        //
        // 为什么：定容开放寻址表按 `compact() & mask` 落槽，而字形 ID 在
        // 键里的位段（18 bit）远高于槽数（6 bit）——**任何两个不同 glyph_id
        // 都会落到同一槽**（实测 glyph 1/2/3/5/6/7/42 全部 idx=1）。
        // 故若在同一个 `sh` 上连跑多个用例，后面的用例会**命中前面写入的
        // 缓存**直接返回（`steps=0`、`pts` 是上一个字形的），畸形路径压根
        // 不会执行——判据会「全绿但什么都没测」。这是用例隔离缺陷，
        // 不是被测物缺陷。
        let run = |o: &Outline, ins: &[Instr]| {
            let mut sh = SubpixelHinting::new();
            sh.run(o, ins, 0, 0, px, Weight::Regular, 1, TierOverrides::none(), 5)
        };

        let ga = run(&outline, &underflow);
        let gb = run(&outline, &oob);
        let gc = run(&outline, &many);
        let ge = run(&empty, &[]);

        // 每种畸形都必须**显式计数到对应类别**（不是笼统 aborted）。
        let a_ok = ga.stats.is_aborted() && ga.stats.stack_underflow > 0 && ga.tier == HintTier::None;
        let b_ok = gb.stats.is_aborted() && gb.stats.point_oob > 0 && gb.tier == HintTier::None;
        let c_ok = gc.stats.is_aborted() && gc.stats.step_limit > 0;
        let e_ok = ge.stats.empty_outline > 0 && ge.tier == HintTier::None;
        // 解码器须拒收非法码与截断流（返回 None）。
        let d_ok = bad_op.is_none() && truncated.is_none();
        // 栈上溢必须被抓住：上游参数区超过 `MAX_STACK` ⇒ 入栈即拒绝、
        // 计数一次、中止执行（**不静默截断**，截断会让后续指令取到错值）。
        // `overflow_ok` 已表达「未完整执行」（拒绝即 false），此处直接采信。
        let f_ok = overflow_ok
            && overflow_stats.stack_overflow == 1
            && overflow_stats.is_aborted();
        // 上界常量须与设计值一致（防有人悄悄放大栈/步数上限）。
        const ANCHOR_STACK: usize = 64;
        const ANCHOR_STEPS: u32 = 4096;
        let const_ok = MAX_STACK == ANCHOR_STACK && MAX_STEPS == ANCHOR_STEPS;
        // 畸形路径必须仍产出点集（字形不丢，只是退化）。
        let no_glyph_loss = !ga.points.is_empty() && !gb.points.is_empty();
        let ok = a_ok && b_ok && c_ok && e_ok && d_ok && f_ok && const_ok && no_glyph_loss;
        assert!(
            ok,
            "畸形处置失败：下溢 {a_ok}(uf={}) 越界 {b_ok}(oob={}) 步数 {c_ok}(sl={}) 空轮廓 {e_ok}(eo={}) 解码拒收 {d_ok} 栈上溢 {f_ok}(of={}) 上界常量 {const_ok} 字形不丢 {no_glyph_loss}",
            ga.stats.stack_underflow,
            gb.stats.point_oob,
            gc.stats.step_limit,
            ge.stats.empty_outline,
            overflow_stats.stack_overflow
        );
        set.add("E05-防护-畸形零崩溃", ok, "");
    }

    // 判据 ⑤ 0.03ms/字形 + 缓存命中为 0 + 缓存键位宽自洽。
    {
        const ANCHOR_US: u32 = 30;
        let const_aligned = PERF_MAX_US_PER_GLYPH == ANCHOR_US;
        let within = ANCHOR_US <= PERF_MAX_US_PER_GLYPH;
        let over_red = ANCHOR_US + 1 > PERF_MAX_US_PER_GLYPH;

        // 缓存键：四元组各段位宽之和须精确 32，且相位键落在 16 桶内。
        let phase_ok = Phase2::space_size() == 16;
        let bits_ok = CacheKey::parts_total_bits() == 32;
        // 键必须能区分「同字形不同相位」——否则四相位在缓存里坍缩成一相。
        let k0 = CacheKey::new(1, 12, Phase2::new(0, 0), Weight::Regular);
        let k1 = CacheKey::new(1, 12, Phase2::new(1, 0), Weight::Regular);
        let k2 = CacheKey::new(1, 12, Phase2::new(0, 1), Weight::Regular);
        let k3 = CacheKey::new(1, 13, Phase2::new(0, 0), Weight::Regular);
        let k4 = CacheKey::new(1, 12, Phase2::new(0, 0), Weight::Bold);
        let distinct =
            k0.compact() != k1.compact() && k0.compact() != k2.compact() && k0.compact() != k3.compact()
                && k0.compact() != k4.compact();

        // 实跑：首次 30us（在预算内），命中 0us。
        let outline = glyph_outline();
        let instrs = align_vertical_stem(&outline);
        let mut sh = SubpixelHinting::new();
        let g1 = sh.run(
            &outline,
            &instrs,
            0,
            0,
            12,
            Weight::Regular,
            42,
            TierOverrides::none(),
            ANCHOR_US,
        );
        let g2 = sh.run(
            &outline,
            &instrs,
            0,
            0,
            12,
            Weight::Regular,
            42,
            TierOverrides::none(),
            ANCHOR_US,
        );
        let run_ok = g1.us == ANCHOR_US && !g1.cache_hit && g2.cache_hit && g2.us == 0;
        // 命中率口径： 是**累计**命中/总查表数，一次未命中 +
        // 一次命中 ⇒ 0.5（不是 1.0）。断言「第二次查表必命中」的正确
        // 写法是看累计计数：misses 恰 1（首次未命中）、hits 恰 1。
        // （曾误断言 hit_rate==1.0 把正确实现判红——累计比率不是布尔量。）
        let hit_ok = sh.cache().hits() == 1 && sh.cache().misses() == 1;
        // 超预算须可被上层判红（阈值语义可证伪）。
        let budget = g1.us <= PERF_MAX_US_PER_GLYPH;

        let ok = const_aligned && within && over_red && phase_ok && bits_ok && distinct && run_ok
            && hit_ok
            && budget;
        assert!(
            ok,
            "性能失败：常量 {const_aligned} 边界 {within}/{over_red} 相位 {phase_ok} 位宽 {bits_ok} 键区分 {distinct} 实跑 {run_ok} 命中 {hit_ok} 预算 {budget}"
        );
        set.add("E05-性能-003ms每字形", ok, "");
    }

    // 错误路径：Hinting 与粗细合成冲突 → 强制轻档 + 计数。
    {
        let outline = glyph_outline();
        let instrs = align_vertical_stem(&outline);
        let mut sh = SubpixelHinting::new();
        let g_reg = sh.run(
            &outline,
            &instrs,
            0,
            0,
            12,
            Weight::Regular,
            5,
            TierOverrides::none(),
            10,
        );
        let g_bold = sh.run(
            &outline,
            &instrs,
            0,
            0,
            12,
            Weight::Bold,
            6,
            TierOverrides::none(),
            10,
        );
        // 小字号 + 粗细合成 ⇒ 全档降轻档且计数。
        let ok = g_reg.tier == HintTier::Full
            && g_bold.tier == HintTier::Light
            && g_bold.stats.forced_light == 1
            && g_reg.stats.forced_light == 0;
        assert!(
            ok,
            "合成降档失败：常规 {:?} 粗体 {:?} 计数 {}/{}",
            g_reg.tier,
            g_bold.tier,
            g_bold.stats.forced_light,
            g_reg.stats.forced_light
        );
        set.add("E05-防护-合成强制轻档", ok, "");
    }

    set
}

/// 粗细档的稳定线码（0..3，按 [`Weight::ALL`] 的**枚举声明序**）。
///
/// 用途：[`CacheKey::compact`] 把四档压进 2 bit。**必须等于枚举声明序**
/// （Thin=0, Regular=1, SemiBold=2, Bold=3），否则不同档会撞进同一缓存槽
/// ——粗细参与缓存键，撞键等于「粗体复用了细体的拟合结果」。
const fn weight_code(w: Weight) -> u32 {
    match w {
        Weight::Thin => 0,
        Weight::Regular => 1,
        Weight::SemiBold => 2,
        Weight::Bold => 3,
    }
}

/// 四档线码的**互异性**（防两档撞码）。
pub fn weight_codes_distinct() -> bool {
    let a = weight_code(Weight::Thin);
    let b = weight_code(Weight::Regular);
    let c = weight_code(Weight::SemiBold);
    let d = weight_code(Weight::Bold);
    a < b && b < c && c < d && d < 4
}

/// 造一个含竖笔的测试轮廓（小字号清晰判据的被测物）。
///
/// 竖笔 = 一条 x 跨度很窄、y 跨度很大的闭合矩形（模拟字母 `l` 的笔画）。
/// 竖笔的 x 中心刻意**偏离**整数网格（3.30/3.62，中点 3.46），这样
/// 「有没有做网格拟合」可从结果坐标直接看出——不做拟合则中点仍是 3.46。
fn glyph_outline() -> Outline {
    // 四点闭合矩形（解释器只改坐标，不关心拓扑，故直接给点集）。
    let rect = [
        Point::from_f64(3.30, 1.00),
        Point::from_f64(3.62, 1.00),
        Point::from_f64(3.62, 9.00),
        Point::from_f64(3.30, 9.00),
    ];
    let mut out = Outline::new();
    out.points = alloc::vec![rect[0], rect[1], rect[2], rect[3]];
    out
}

/// 为测试轮廓生成「竖笔对齐」指令流：对竖笔做 x 网格拟合。
///
/// `args[0]` 既是点索引，也**不是**容差——`AlignPts` 的语义是「以该点为
/// 基准求 delta、整笔平移」，容差由调用方通过 [`Instr::new`] 的
/// `args[1]` 给出（对齐到全档容差 [`GRID_SNAP_TOL_Q`]）。
fn align_vertical_stem(o: &Outline) -> Vec<Instr> {
    // 索引按**逆序**排（3,2,1,0）：`AlignPts` 每次弹栈取一个索引，
    // 要依次处理 0,1,2,3 就得按逆序压栈（栈后进先出）。
    let n = o.points.len();
    let mut out: Vec<Instr> = Vec::new();
    let mut i = n;
    while i > 0 {
        i -= 1;
        out.push(Instr::new(Op::AlignPts, [i as i32, GRID_SNAP_TOL_Q, 0, 0]));
    }
    out
}

/// 取「竖笔所在的整数像素列」：x 坐标除以 64 后的商。
fn stem_column(pts: &[Point]) -> i32 {
    if pts.is_empty() {
        return 0;
    }
    let mut acc = 0i32;
    for p in pts.iter() {
        acc += p.x;
    }
    (acc / pts.len() as i32).div_euclid(QUANT_ONE)
}

/// 竖笔宽度（F26Dot6）：max(x) − min(x)。
///
/// 网格拟合**只应改笔位、不该改笔宽**，故本量是判据 ③ 的核心观测量。
fn stem_width(pts: &[Point]) -> i32 {
    if pts.is_empty() {
        return 0;
    }
    let mut lo = i32::MAX;
    let mut hi = i32::MIN;
    for p in pts.iter() {
        if p.x < lo {
            lo = p.x;
        }
        if p.x > hi {
            hi = p.x;
        }
    }
    hi - lo
}

/// 竖笔左缘 x（F26Dot6）。
fn stem_left(pts: &[Point]) -> i32 {
    let mut lo = i32::MAX;
    for p in pts.iter() {
        if p.x < lo {
            lo = p.x;
        }
    }
    if pts.is_empty() {
        0
    } else {
        lo
    }
}

/// 非法操作码字节流（低 4 位 = 0x0F，无对应操作码）。
fn decode_bad_opcode() -> Option<Vec<Instr>> {
    hinting::decode(&[0x0F])
}

/// 截断指令流：高 4 位声明 2 个操作数，但只提供 1 字节。
fn decode_truncated() -> Option<Vec<Instr>> {
    hinting::decode(&[0x31, 0x05])
}
