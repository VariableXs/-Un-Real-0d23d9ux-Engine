//! VE-F0029 · 间接绘制命令生成器（VE-A 域 · GPU 侧命令生成 + CPU 侧预检 · 目标 360 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0029`
//!
//! **判据（锚点原文）**：GPU 侧绘制命令的生成（Indirect Draw 的命令缓冲写入），
//! 命令合法性预校验（GPU 侧生成的命令也要有 CPU 侧的边界预检），命令复用与变体；
//! 含间接命令的调试回放（生成的命令可视化）。判据：**GPU 侧生成、边界预检、
//! CPU 回退、变体合并、判据**。
//!
//! **错误路径与降级矩阵**（锚点原文）：
//!
//! - 命令越界 → **预检拦截**（CPU 侧生成期拦，不写缓冲、不进 GPU；每类越界一个
//!   专属错误码，判据直接断言故障种类而非「出错了」）
//! - 生成失败 → **CPU 回退**（被拦截的命令登记进 CPU 回退计划表走立即绘制路径，
//!   不是静默丢弃；回退条数与拦截次数**独立对账**，不采信净值口径）
//! - 变体爆炸 → **合并**（变体表满时把量级分档**变粗**并按新粒度重算去重，让更多
//!   key 塌进既有变体；不是无脑新增，也不是直接拒绝；粒度递进有上界）
//!
//! **数据结构**：命令生成器（[`CommandGen`]）；预校验（[`precheck`]）；变体表
//! （[`VariantTable`]，存**量化前的形状源** [`ShapeSrc`]，故粒度变粗后可重算）。
//!
//! **性能逐项分解**：O(命令)——`push` 是 O(1) 摊还（预检为常数条比较、写入定长
//! [`COMMAND_STRIDE`] 字节、变体查表上限 [`MAX_VARIANTS`] 项），`intern` 是
//! O(变体档数)，回放与统计是 O(本帧命令数)。均**不随累计命令总数 N 增长**
//! （命令缓冲按帧清空，不跨帧累积）。
//!
//! **跨批对接点**：A30 合批联动——[`GeneratorStat`] 暴露 `recorded`（本帧写入的
//! 命令数）与 `variants`（占用变体档数），A30 合批器按 `recorded` 估合批收益、
//! 按 `variants` 判变体爆炸是否值得改合批策略。**本条只出计数，不缓存合批决策**；
//! A30 改合批策略会使本条命令数变化，那由 A30 自己的口径负责。
//!
//! **无障碍与隐私**：命令状态读屏可达（[`CommandGen::a11y_lines`]）——报「本帧命令
//! 数/变体档数/被拦截数/CPU 回退数/粒度」，中英双语逐行。面板**只报聚合计数与
//! 粒度档**，**不报单条命令的顶点偏移与实例数**（那是资产布局信息）。
//!
//! ## 设计要点
//!
//! - **GPU 侧生成，但字节布局由本条显式编码**（[`CommandGen::push`]）：不写
//!   `transmute`、不 `unsafe`、零 `panic` 面。每个字段按小端逐字节写入，枚举走显式
//!   [`DrawKind::wire`] 映射，**禁 `kind as u8`**——判别值 0/1/2 与线上编码
//!   0x51/0x52/0x53 本就该不同，改成判别值的那天就是 ABI 静默损坏的那天。
//! - **预检必须发生在写缓冲之前**（[`precheck`] 在 [`CommandGen::push`] 的第一步）：
//!   GPU 侧越界的代价是驱动挂起或读越界内存，事后补救不存在。判据
//!   `A29-边界-被拦截时写入字节数与命令数均不变` 断言**字节数确实没动**。
//! - **每类越界一个专属错误码**（[`PrecheckFail`]）：顶点数为零 / 顶点数超限 /
//!   实例数为零 / 实例数超限 / 起始索引溢出 / 偏移未对齐 / 偏移超限，各归各码。
//!   若合并成一条 `OutOfBounds`，则「偏移未对齐」与「偏移超限」两条错误路外部
//!   表现相同，删掉其中一条判据仍全绿（十诫第 3 条）。
//! - **对齐先于上限判定**（[`precheck`] 第 4 步）：一个偏移若既未对齐又超限，报
//!   「未对齐」——那才是它被拒的**直接**原因；先判上限会把直接原因掩盖成间接原因，
//!   调用方按错误码给的修复建议也就错了。
//! - **缓冲尾部保留区恒零**（[`COMMAND_STRIDE`] 的 28..32）：判据
//!   `A29-生成-保留区恒零且不随命令数漂移` 扫全缓冲逐字节断言。写循环用
//!   `count * stride` 作上界的实现下成立；用 `buf.len()` 作上界的实现会把保留区
//!   也写进去——「写满了」不等于「把不该写的也写了」。
//! - **变体合并靠粒度递进，不靠删表**（[`VariantTable::intern`]）：变体爆炸的
//!   正确解是**把量级分档变粗**让更多 key 落进同一格；直接删表会让已发放的变体号
//!   悬空，直接拒绝则把问题推给调用方。故表内存**量化前的形状源**
//!   （[`ShapeSrc`]，存原始数值），粒度变粗后按新粒度**重算并去重重编变体号**
//!   （[`VariantTable::compact`]）——表只减不增，且变体号始终连续无洞。
//! - **变体号只在缓冲外流转**：命令缓冲里存的是**展开后的参数**而非变体号，
//!   故重编变体号不会让已写入的命令指向错误数据。这条是上一条的成立前提。
//! - **粒度递进有上界，且上界处显式报错**（[`MAX_GRANULARITY`]）：无限递进会让
//!   所有 key 塌成一个变体，「变体」这个概念本身就失去意义。故粒度到顶后
//!   [`PrecheckFail::VariantBudgetExhausted`] 报错并记账——该码**可达**（判据
//!   `A29-变体-粒度到顶后预算拒绝计数增长` 实测灌到拒绝为止），不是死码。
//! - **CPU 回退条数与拦截次数独立对账**（[`GeneratorStat`]）：断「净值口径」的
//!   `fallback_count == intercepted` 会被「建了又销」骗过（1/1 也相等），故判据侧
//!   **独立重算**本轮真实拦截数并断 `fallback_count` **恰等于**该数，且同时断
//!   `recorded` 只统计放行的那几条（十诫第 10 条）。
//! - **夹逼对钉死界位置**（[`ARG_OFFSET_LIMIT`] / [`VERTEX_LIMIT`] /
//!   [`MAX_COMMANDS`]）：判据一律用「界前一合法点 / 界上 / 界后一位」三点。只断
//!   「超限即拒」的宽阈值，会把界上那一点也拒掉而全绿（十诫第 4 条）。
//! - **变体去重计数用绝对值口径**：`variants == 不同 key 数` 而非净值差，否则
//!   「建了又销」同样能骗过去（十诫第 10 条）。
//!
//! ## 与相邻条的分工（易混，故写明）
//!
//! - **F0030（多绘制合批器）决定「该不该合」**，本条决定「命令怎么写进间接缓冲」。
//!   本条的 [`GeneratorStat::recorded`] 是合批收益评估的输入，反向不成立。
//! - **F0026（`vea26_desc_heap`）管描述符堆**，也讲「预算/耗尽」，但堆的三段是
//!   分配/分片/复用，回收受一帧绑定次数约束；本条的「预算」是**单帧命令条数**，
//!   且超限即预检拦截，不做分帧回收——命令是当帧即发，回收命令没有意义。
//! - **F0028（`vea28_querypool`）管查询槽位池化**，同为池化但回收对象是**正在
//!   飞行的查询**；本条无回收语义，只有「当帧生成 + 帧首清零」。
//!
//! 两条各自**自持**定义类型，不跨模块 `use`——并行提交时跨模块引用会把两个模块
//! 的编译成败绑在一起，一方半成品就拖垮另一方，而这类失败报 E0583，与真实缺陷
//! 长得一样、极难分辨。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量
// ---------------------------------------------------------------------------

/// 间接命令种类数（锚点原文三类：非索引绘制 / 索引绘制 / 计算派发）。
pub const KIND_COUNT: usize = 3;

/// 单条间接命令在缓冲中的步进字节数（28 字节有效载荷 + 4 字节保留区）。
pub const COMMAND_STRIDE: usize = 32;

/// 单帧间接命令条数上限（超出 → [`PrecheckFail::CommandCountOverflow`]）。
pub const MAX_COMMANDS: usize = 64;

/// 变体表容量上限（超出触发粒度合并，既不直接新增也不直接拒绝）。
pub const MAX_VARIANTS: usize = 24;

/// 变体粒度上限（粒度递进到顶 → [`PrecheckFail::VariantBudgetExhausted`]）。
pub const MAX_GRANULARITY: u8 = 4;

/// 间接参数区起始偏移上限（`buf_addr - base_addr`，超出 → 越界）。
pub const ARG_OFFSET_LIMIT: usize = 512;

/// 参数区起始偏移对齐要求（字节）。
pub const ARG_ALIGN: usize = 4;

/// 顶点数上限（超出 → 越界；零 → 越界）。
pub const VERTEX_LIMIT: u32 = 1 << 24;

/// 实例数上限（超出 → 越界；零 → 越界）。
pub const INSTANCE_LIMIT: u32 = 1 << 20;

/// 单条命令的参数字节数（`base_vertex` 等索引参数只进变体指纹与回退计划，
/// 不进间接缓冲——GPU 侧索引参数走索引缓冲偏移，不占命令步进）。
pub const ARGS_LEN: usize = 28;

/// 失败原因种数（判据按此遍历，不手抄清单）。
pub const FAIL_COUNT: usize = 9;

/// CPU 回退计划表上限（超出后仍记拦截数，但不再登记计划并置饱和标志）。
pub const MAX_FALLBACKS: usize = MAX_COMMANDS * 2;

// ---------------------------------------------------------------------------
// 二、命令种类与参数
// ---------------------------------------------------------------------------

/// 间接命令种类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrawKind {
    /// 非索引绘制：顶点数直接进缓冲。
    Draw,
    /// 索引绘制：额外读索引缓冲。
    DrawIndexed,
    /// 计算派发：工作组数进缓冲。
    Dispatch,
}

impl DrawKind {
    /// 全集，顺序稳定（判据按此下标推导，不靠字面量）。
    pub const ALL: [DrawKind; KIND_COUNT] = [
        DrawKind::Draw,
        DrawKind::DrawIndexed,
        DrawKind::Dispatch,
    ];

    /// 判别下标（0/1/2），**不是线上编码值**。
    pub const fn ordinal(self) -> usize {
        match self {
            DrawKind::Draw => 0,
            DrawKind::DrawIndexed => 1,
            DrawKind::Dispatch => 2,
        }
    }

    /// 中文标签（读屏用）。
    pub const fn zh(self) -> &'static str {
        match self {
            DrawKind::Draw => "非索引绘制",
            DrawKind::DrawIndexed => "索引绘制",
            DrawKind::Dispatch => "计算派发",
        }
    }

    /// 英文标签（读屏用）。
    pub const fn tag(self) -> &'static str {
        match self {
            DrawKind::Draw => "draw",
            DrawKind::DrawIndexed => "indexed",
            DrawKind::Dispatch => "dispatch",
        }
    }

    /// 线上编码值。**与判别值刻意不同**（0x51/0x52/0x53 ≠ 0/1/2）。
    pub const fn wire(self) -> u8 {
        match self {
            DrawKind::Draw => 0x51,
            DrawKind::DrawIndexed => 0x52,
            DrawKind::Dispatch => 0x53,
        }
    }

    /// 线上编码值反解（判据用它做往返一致性断言）。
    pub const fn from_wire(v: u8) -> Option<DrawKind> {
        match v {
            0x51 => Some(DrawKind::Draw),
            0x52 => Some(DrawKind::DrawIndexed),
            0x53 => Some(DrawKind::Dispatch),
            _ => None,
        }
    }
}

/// 一条间接命令的参数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrawArgs {
    /// 命令种类。
    pub kind: DrawKind,
    /// 顶点数（`Dispatch` 时为工作组数）。
    pub vertex_count: u32,
    /// 实例数。
    pub instance_count: u32,
    /// 首个顶点下标。
    pub first_vertex: u32,
    /// 首个实例下标。
    pub first_instance: u32,
    /// 索引数（仅 `DrawIndexed` 有意义，其余为 0）。
    pub index_count: u32,
    /// 基顶点偏移（可负，仅 `DrawIndexed` 有意义）。
    pub base_vertex: i32,
}

impl DrawArgs {
    /// 全零骨架（判据在此基础上逐字段施加，避免散落字面量）。
    pub const fn zero() -> DrawArgs {
        DrawArgs {
            kind: DrawKind::Draw,
            vertex_count: 0,
            instance_count: 0,
            first_vertex: 0,
            first_instance: 0,
            index_count: 0,
            base_vertex: 0,
        }
    }

    /// 一条合法的非索引绘制（3 顶点 × 1 实例）。
    pub const fn draw() -> DrawArgs {
        DrawArgs {
            kind: DrawKind::Draw,
            vertex_count: 3,
            instance_count: 1,
            first_vertex: 0,
            first_instance: 0,
            index_count: 0,
            base_vertex: 0,
        }
    }

    /// 一条合法的索引绘制。
    pub const fn indexed() -> DrawArgs {
        DrawArgs {
            kind: DrawKind::DrawIndexed,
            vertex_count: 3,
            instance_count: 1,
            first_vertex: 0,
            first_instance: 0,
            index_count: 3,
            base_vertex: 0,
        }
    }

    /// 一条合法的计算派发。
    pub const fn dispatch() -> DrawArgs {
        DrawArgs {
            kind: DrawKind::Dispatch,
            vertex_count: 1,
            instance_count: 1,
            first_vertex: 0,
            first_instance: 0,
            index_count: 0,
            base_vertex: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// 三、预校验（锚点：命令合法性预校验 · 命令越界 → 预检拦截）
// ---------------------------------------------------------------------------

/// 预校验失败原因。**每类越界一个专属码**。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrecheckFail {
    /// 顶点数为零：驱动侧无意义，且会让步进计算下溢。
    VertexCountZero,
    /// 顶点数超上限。
    VertexCountOverLimit,
    /// 实例数为零。
    InstanceCountZero,
    /// 实例数超上限。
    InstanceCountOverLimit,
    /// 起始索引溢出（`first_vertex + vertex_count` 越 32 位）。
    FirstIndexOverflow,
    /// 参数区起始偏移未按 [`ARG_ALIGN`] 对齐。
    OffsetUnaligned,
    /// 参数区起始偏移超 [`ARG_OFFSET_LIMIT`]。
    OffsetOverLimit,
    /// 单帧命令条数超 [`MAX_COMMANDS`]。
    CommandCountOverflow,
    /// 粒度已递进到 [`MAX_GRANULARITY`]，变体表仍容不下该 key。
    VariantBudgetExhausted,
}

impl PrecheckFail {
    /// 全集（判据按此遍历）。
    pub const ALL: [PrecheckFail; FAIL_COUNT] = [
        PrecheckFail::VertexCountZero,
        PrecheckFail::VertexCountOverLimit,
        PrecheckFail::InstanceCountZero,
        PrecheckFail::InstanceCountOverLimit,
        PrecheckFail::FirstIndexOverflow,
        PrecheckFail::OffsetUnaligned,
        PrecheckFail::OffsetOverLimit,
        PrecheckFail::CommandCountOverflow,
        PrecheckFail::VariantBudgetExhausted,
    ];

    /// 中文标签（读屏播报）。
    pub const fn zh(self) -> &'static str {
        match self {
            PrecheckFail::VertexCountZero => "顶点数为零",
            PrecheckFail::VertexCountOverLimit => "顶点数超上限",
            PrecheckFail::InstanceCountZero => "实例数为零",
            PrecheckFail::InstanceCountOverLimit => "实例数超上限",
            PrecheckFail::FirstIndexOverflow => "起始索引溢出",
            PrecheckFail::OffsetUnaligned => "参数区偏移未对齐",
            PrecheckFail::OffsetOverLimit => "参数区偏移超限",
            PrecheckFail::CommandCountOverflow => "单帧命令数超限",
            PrecheckFail::VariantBudgetExhausted => "变体预算耗尽",
        }
    }

    /// 英文标签（读屏播报）。
    pub const fn tag(self) -> &'static str {
        match self {
            PrecheckFail::VertexCountZero => "vertex count zero",
            PrecheckFail::VertexCountOverLimit => "vertex count over limit",
            PrecheckFail::InstanceCountZero => "instance count zero",
            PrecheckFail::InstanceCountOverLimit => "instance count over limit",
            PrecheckFail::FirstIndexOverflow => "first index overflow",
            PrecheckFail::OffsetUnaligned => "arg offset unaligned",
            PrecheckFail::OffsetOverLimit => "arg offset over limit",
            PrecheckFail::CommandCountOverflow => "command count overflow",
            PrecheckFail::VariantBudgetExhausted => "variant budget exhausted",
        }
    }

    /// 是否转入 CPU 回退。变体预算耗尽时**不回退**——资源健康、资源充足，
    /// 回退没有意义（那会让「变体不够」伪装成「画得慢」），直接拒绝让调用方改合批。
    pub const fn fallbacks_to_cpu(self) -> bool {
        !matches!(self, PrecheckFail::VariantBudgetExhausted)
    }
}

/// 命令合法性预校验。**纯函数**：不读缓冲、不改状态、无副作用，故可在生成前
/// 反复调用、可独立单测。
///
/// 边界算术全部走 `checked_*`：参数区的偏移与长度来自四字节字段，32 位上
/// `offset + ARGS_LEN` 会溢出；溢出按既有错误码拒（[`PrecheckFail::OffsetOverLimit`]），
/// 不新造码也不 panic。
pub fn precheck(
    args: &DrawArgs,
    arg_offset: usize,
    live_commands: usize,
) -> Result<(), PrecheckFail> {
    // 1) 顶点数：零与超限各归各码。
    if args.vertex_count == 0 {
        return Err(PrecheckFail::VertexCountZero);
    }
    if args.vertex_count > VERTEX_LIMIT {
        return Err(PrecheckFail::VertexCountOverLimit);
    }

    // 2) 实例数：同上。
    if args.instance_count == 0 {
        return Err(PrecheckFail::InstanceCountZero);
    }
    if args.instance_count > INSTANCE_LIMIT {
        return Err(PrecheckFail::InstanceCountOverLimit);
    }

    // 3) 起始索引不得溢出 32 位。
    if args.first_vertex.checked_add(args.vertex_count).is_none() {
        return Err(PrecheckFail::FirstIndexOverflow);
    }

    // 4) 参数区偏移：**先对齐后上限**——对齐才是未对齐偏移被拒的直接原因。
    if arg_offset % ARG_ALIGN != 0 {
        return Err(PrecheckFail::OffsetUnaligned);
    }
    if arg_offset > ARG_OFFSET_LIMIT {
        return Err(PrecheckFail::OffsetOverLimit);
    }
    // 偏移加参数长度不得溢出。
    match arg_offset.checked_add(ARGS_LEN) {
        Some(end) if end <= ARG_OFFSET_LIMIT + ARGS_LEN => {}
        _ => return Err(PrecheckFail::OffsetOverLimit),
    }

    // 5) 单帧命令条数（界上放行、界后一位拒绝——判据用夹逼对钉死）。
    if live_commands >= MAX_COMMANDS {
        return Err(PrecheckFail::CommandCountOverflow);
    }

    Ok(())
}

/// 失败原因在拦截计数数组中的下标（与 [`PrecheckFail::ALL`] 同序）。
pub fn fail_index(f: PrecheckFail) -> usize {
    match f {
        PrecheckFail::VertexCountZero => 0,
        PrecheckFail::VertexCountOverLimit => 1,
        PrecheckFail::InstanceCountZero => 2,
        PrecheckFail::InstanceCountOverLimit => 3,
        PrecheckFail::FirstIndexOverflow => 4,
        PrecheckFail::OffsetUnaligned => 5,
        PrecheckFail::OffsetOverLimit => 6,
        PrecheckFail::CommandCountOverflow => 7,
        PrecheckFail::VariantBudgetExhausted => 8,
    }
}

/// 反查：下标 → 失败原因（判据遍历 [`FAIL_COUNT`] 时用，避免手抄清单）。
pub const fn fail_at(idx: usize) -> PrecheckFail {
    match idx {
        0 => PrecheckFail::VertexCountZero,
        1 => PrecheckFail::VertexCountOverLimit,
        2 => PrecheckFail::InstanceCountZero,
        3 => PrecheckFail::InstanceCountOverLimit,
        4 => PrecheckFail::FirstIndexOverflow,
        5 => PrecheckFail::OffsetUnaligned,
        6 => PrecheckFail::OffsetOverLimit,
        7 => PrecheckFail::CommandCountOverflow,
        _ => PrecheckFail::VariantBudgetExhausted,
    }
}

// ---------------------------------------------------------------------------
// 四、命令生成器（锚点：GPU 侧绘制命令的生成 · 命令缓冲写入）
// ---------------------------------------------------------------------------

/// 变体指纹：命令「形状」而非「实例」。形状相同 → 命令复用同一变体。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VariantKey {
    /// 命令种类。
    pub kind: DrawKind,
    /// 档位掩码：顶点 4 位 / 实例 4 位 / 索引 4 位 / 首顶点高位 1 位 / 基顶点符号 1 位。
    pub shape: u16,
}

/// 量级分档：粒度 `g == 0` 取精确低 4 位（最细，16 档可区分），
/// `g >= 1` 起改对数档（阈值 `1 << (4 + 2*(g-1))`，每档 ×16，最粗 4 档）。
///
/// 对同一数值，粒度递进使档号**单调不增**——这就是「合并只粗化、不细分」的
/// 数学依据，判据 `A29-变体-粒度递进使分档单调不增` 直接断它。
fn magnitude(value: u32, granularity: u8) -> u8 {
    let g = granularity.min(MAX_GRANULARITY);
    if g == 0 {
        return (value & 0x0f) as u8;
    }
    let shift = 4u32 + (g as u32 - 1) * 2;
    let mut threshold = 1u32 << shift.min(31);
    let mut bucket = 0u8;
    let mut i = 0u8;
    while i < 3 {
        if value < threshold {
            break;
        }
        bucket += 1;
        threshold = match threshold.checked_mul(16) {
            Some(t) => t,
            None => break,
        };
        i += 1;
    }
    bucket
}

/// 形状源：**量化前**的原始量值。变体表存它而非 [`VariantKey`]，
/// 故粒度变粗后能按新粒度重算——这是「合并」得以真正生效的前提。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShapeSrc {
    /// 命令种类。
    pub kind: DrawKind,
    /// 顶点数原始值。
    pub vertex_count: u32,
    /// 实例数原始值。
    pub instance_count: u32,
    /// 索引数原始值。
    pub index_count: u32,
    /// 首顶点高位标志（≥4096）。
    pub first_vertex_high: u8,
    /// 基顶点为负的标志。
    pub base_vertex_neg: u8,
}

impl ShapeSrc {
    /// 由参数抽取形状源（剔除不影响变体复用的字段，如 `first_instance`）。
    pub fn of(args: &DrawArgs) -> ShapeSrc {
        ShapeSrc {
            kind: args.kind,
            vertex_count: args.vertex_count,
            instance_count: args.instance_count,
            index_count: if args.kind == DrawKind::DrawIndexed {
                args.index_count
            } else {
                0
            },
            first_vertex_high: if args.first_vertex >= 4096 { 1 } else { 0 },
            base_vertex_neg: if args.base_vertex < 0 { 1 } else { 0 },
        }
    }

    /// 按给定粒度量化成指纹。
    pub fn key(&self, granularity: u8) -> VariantKey {
        let v = magnitude(self.vertex_count, granularity);
        let i = magnitude(self.instance_count, granularity);
        let x = magnitude(self.index_count, granularity);
        VariantKey {
            kind: self.kind,
            shape: (v as u16) | ((i as u16) << 4) | ((x as u16) << 8),
        }
    }
}

/// 变体表的一行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct VariantEntry {
    src: ShapeSrc,
    vid: u32,
}

/// 变体表（命令复用与变体 · 变体爆炸 → 合并）。
#[derive(Clone, Debug)]
pub struct VariantTable {
    entries: Vec<VariantEntry>,
    granularity: u8,
    intern_hits: u64,
    intern_new: u64,
    merge_events: u64,
    budget_refusals: u64,
}

impl VariantTable {
    /// 空表（粒度从 0 起步，最细）。
    pub fn new() -> VariantTable {
        VariantTable {
            entries: Vec::new(),
            granularity: 0,
            intern_hits: 0,
            intern_new: 0,
            merge_events: 0,
            budget_refusals: 0,
        }
    }

    /// 取出（必要时分配）一条命令的变体号。
    ///
    /// 表未满 → 直接新增。表满 → **粒度递进合并**（[`Self::compact`]）：把分档
    /// 变粗一档并按新粒度重算去重，让更多 key 塌进既有变体；粒度到顶仍容不下
    /// 才拒绝并记账。
    pub fn intern(&mut self, args: &DrawArgs) -> Result<u32, PrecheckFail> {
        let src = ShapeSrc::of(args);
        loop {
            let key = src.key(self.granularity);
            let mut found: Option<u32> = None;
            let mut i = 0usize;
            while i < self.entries.len() {
                if self.entries[i].src.key(self.granularity) == key {
                    found = Some(self.entries[i].vid);
                    break;
                }
                i += 1;
            }
            if let Some(vid) = found {
                self.intern_hits = self.intern_hits.saturating_add(1);
                return Ok(vid);
            }
            if self.entries.len() < MAX_VARIANTS {
                self.entries.push(VariantEntry { src, vid: 0 });
                self.intern_new = self.intern_new.saturating_add(1);
                self.reindex();
                let last = self.entries.len() - 1;
                return Ok(self.entries[last].vid);
            }
            // 变体爆炸 → 合并。
            if self.granularity >= MAX_GRANULARITY {
                self.budget_refusals = self.budget_refusals.saturating_add(1);
                return Err(PrecheckFail::VariantBudgetExhausted);
            }
            self.granularity = self.granularity.saturating_add(1);
            self.merge_events = self.merge_events.saturating_add(1);
            self.compact();
        }
    }

    /// 按当前粒度重算去重，并把变体号重编为连续无洞的 `0..len`。
    ///
    /// 表**只减不增**：同 key 的行合成一行（保留首次出现的位置），变体号随后重排。
    /// 已发放的变体号会随之改变——这是安全的，因为命令缓冲里存的是**展开后的
    /// 参数**而非变体号（见模块头「变体号只在缓冲外流转」）。
    fn compact(&mut self) {
        let g = self.granularity;
        let mut kept: Vec<VariantEntry> = Vec::new();
        let mut i = 0usize;
        while i < self.entries.len() {
            let e = self.entries[i];
            let key = e.src.key(g);
            let mut dup = false;
            let mut j = 0usize;
            while j < kept.len() {
                if kept[j].src.key(g) == key {
                    dup = true;
                    break;
                }
                j += 1;
            }
            if !dup {
                kept.push(e);
            }
            i += 1;
        }
        self.entries = kept;
        self.reindex();
    }

    /// 变体号重排为 `0..len`（去重后必然连续）。
    fn reindex(&mut self) {
        let mut i = 0usize;
        while i < self.entries.len() {
            self.entries[i].vid = i as u32;
            i += 1;
        }
    }

    /// 占用档数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 空表判定。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 当前粒度。
    pub fn granularity(&self) -> u8 {
        self.granularity
    }

    /// 命中次数（同形状复用）。
    pub fn intern_hits(&self) -> u64 {
        self.intern_hits
    }

    /// 新增次数。
    pub fn intern_new(&self) -> u64 {
        self.intern_new
    }

    /// 合并次数（粒度递进触发）。
    pub fn merge_events(&self) -> u64 {
        self.merge_events
    }

    /// 预算拒绝次数。
    pub fn budget_refusals(&self) -> u64 {
        self.budget_refusals
    }

    /// 变体号是否落在 `0..len` 内且**无洞**（变体重编后不得出现空洞）。
    pub fn vids_dense(&self) -> bool {
        let mut seen = vec![false; self.entries.len()];
        let mut i = 0usize;
        while i < self.entries.len() {
            let vid = self.entries[i].vid as usize;
            if vid >= seen.len() || seen[vid] {
                return false;
            }
            seen[vid] = true;
            i += 1;
        }
        true
    }
}

/// 一条 CPU 回退计划（生成失败 → CPU 回退）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FallbackPlan {
    /// 被拒的种类。
    pub kind: DrawKind,
    /// 被拒原因。
    pub reason: PrecheckFail,
    /// 原本要写入的参数区偏移。
    pub arg_offset: usize,
}

/// 单帧统计快照（跨批对接点 A30 合批联动的唯一出口）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GeneratorStat {
    /// 本帧成功写入的命令数。
    pub recorded: usize,
    /// 本帧写入的字节数（恒等于 `recorded * COMMAND_STRIDE`）。
    pub bytes: usize,
    /// 占用的变体档数。
    pub variants: usize,
    /// 当前变体粒度。
    pub granularity: u8,
    /// 变体命中次数。
    pub intern_hits: u64,
    /// 被预检拦截次数（各原因之和）。
    pub intercepted: u64,
    /// CPU 回退条数。
    pub fallback_count: u64,
    /// 回退计划表饱和标志（超出 [`MAX_FALLBACKS`] 后置位）。
    pub fallback_saturated: bool,
    /// 合并次数。
    pub merge_events: u64,
    /// 变体预算拒绝次数。
    pub budget_refusals: u64,
}

/// 间接绘制命令生成器。
#[derive(Clone, Debug)]
pub struct CommandGen {
    /// 命令缓冲（当帧驻留，帧首清空）。
    buf: Vec<u8>,
    /// 已写入命令数。
    recorded: usize,
    /// 变体表。
    variants: VariantTable,
    /// CPU 回退计划表。
    fallbacks: Vec<FallbackPlan>,
    /// 各原因拦截计数（与 [`PrecheckFail::ALL`] 同序，末位留 1 格 headroom）。
    intercepted: [u64; FAIL_COUNT + 1],
    /// 回退计划表饱和标志。
    fallback_saturated: bool,
}

impl CommandGen {
    /// 构造（帧首调用）。
    pub fn new() -> CommandGen {
        CommandGen {
            buf: Vec::new(),
            recorded: 0,
            variants: VariantTable::new(),
            fallbacks: Vec::new(),
            intercepted: [0u64; FAIL_COUNT + 1],
            fallback_saturated: false,
        }
    }

    /// 帧首清零：命令缓冲不留存上一帧（命令当帧即发，跨帧累积即泄漏）。
    ///
    /// **只清命令缓冲与回退表，不清变体表**——变体是跨帧复用的资产（形状相同的
    /// 命令下一帧仍应命中），清掉等于每帧重建，合批收益归零。
    pub fn begin_frame(&mut self) {
        self.buf.clear();
        self.recorded = 0;
        self.fallbacks.clear();
        self.fallback_saturated = false;
        let mut i = 0usize;
        while i < self.intercepted.len() {
            self.intercepted[i] = 0;
            i += 1;
        }
    }

    /// 生成一条间接命令并写入命令缓冲，返回其变体号。
    ///
    /// 顺序固定：**预检 → 变体分配 → 预分配字节 → 写入**。任一步失败都**不写
    /// 一个字节**，并按 [`PrecheckFail::fallbacks_to_cpu`] 决定是否登记 CPU 回退。
    pub fn push(&mut self, args: &DrawArgs, arg_offset: usize) -> Result<u32, PrecheckFail> {
        // 1) 预检（锚点：命令合法性预校验）。必须在任何写入之前。
        if let Err(f) = precheck(args, arg_offset, self.recorded) {
            self.note(args, arg_offset, f);
            return Err(f);
        }
        // 2) 变体分配（锚点：命令复用与变体）。
        let vid = match self.variants.intern(args) {
            Ok(v) => v,
            Err(f) => {
                self.note(args, arg_offset, f);
                return Err(f);
            }
        };
        // 3) 预分配后再写，避免写入途中扩容失败留下半条命令。
        let base = self.buf.len();
        self.buf.resize(base + COMMAND_STRIDE, 0u8);
        // 4) 写入定长载荷；28..32 保留区由 resize 的零填充保持为零。
        write_u32(&mut self.buf, base, args.kind.wire() as u32);
        write_u32(&mut self.buf, base + 4, args.vertex_count);
        write_u32(&mut self.buf, base + 8, args.instance_count);
        write_u32(&mut self.buf, base + 12, args.first_vertex);
        write_u32(&mut self.buf, base + 16, args.first_instance);
        write_u32(&mut self.buf, base + 20, args.index_count);
        write_u32(&mut self.buf, base + 24, args.base_vertex as u32);
        self.recorded = self.recorded.saturating_add(1);
        Ok(vid)
    }

    /// 记一次拦截，并按需登记 CPU 回退（**每条被拒命令恰好登记一次**）。
    fn note(&mut self, args: &DrawArgs, arg_offset: usize, f: PrecheckFail) {
        let idx = fail_index(f);
        if idx < FAIL_COUNT {
            self.intercepted[idx] = self.intercepted[idx].saturating_add(1);
        }
        if !f.fallbacks_to_cpu() {
            return;
        }
        if self.fallbacks.len() < MAX_FALLBACKS {
            self.fallbacks.push(FallbackPlan {
                kind: args.kind,
                reason: f,
                arg_offset,
            });
        } else {
            // 计划表满：如实置饱和标志，绝不静默丢弃（拦截数仍照记）。
            self.fallback_saturated = true;
        }
    }

    /// 已写入命令数。
    pub fn recorded(&self) -> usize {
        self.recorded
    }

    /// 命令缓冲字节数。
    pub fn bytes(&self) -> usize {
        self.buf.len()
    }

    /// 只读命令缓冲（调试回放与外部校验共用；不泄漏可写引用）。
    pub fn buffer(&self) -> &[u8] {
        &self.buf
    }

    /// 取第 `index` 条命令的第 `field` 号 u32 字段（越界返回 0）。
    ///
    /// `field` 以 4 为步进：`0`=种类线编码 `4`=顶点数 `8`=实例数 `12`=首顶点
    /// `16`=首实例 `20`=索引数 `24`=基顶点（`i32` 比特）。越界由调用方预检负责。
    pub fn word(&self, index: usize, field: u32) -> u32 {
        read_u32(&self.buf, index * COMMAND_STRIDE + field as usize)
    }

    /// 变体表只读视图。
    pub fn variants(&self) -> &VariantTable {
        &self.variants
    }

    /// CPU 回退计划表。
    pub fn fallbacks(&self) -> &[FallbackPlan] {
        &self.fallbacks
    }

    /// 某原因的被拒次数。
    pub fn intercepted_of(&self, f: PrecheckFail) -> u64 {
        let idx = fail_index(f);
        if idx < FAIL_COUNT {
            self.intercepted[idx]
        } else {
            0
        }
    }

    /// 全部拦截次数之和（**独立于** [`Self::fallbacks`] 的长度，二者须对账）。
    pub fn intercepted_total(&self) -> u64 {
        let mut sum = 0u64;
        let mut i = 0usize;
        while i < FAIL_COUNT {
            sum = sum.saturating_add(self.intercepted[i]);
            i += 1;
        }
        sum
    }

    /// 回退计划表饱和标志。
    pub fn fallback_saturated(&self) -> bool {
        self.fallback_saturated
    }

    /// 单帧统计快照。
    pub fn stat(&self) -> GeneratorStat {
        GeneratorStat {
            recorded: self.recorded,
            bytes: self.buf.len(),
            variants: self.variants.len(),
            granularity: self.variants.granularity(),
            intern_hits: self.variants.intern_hits(),
            intercepted: self.intercepted_total(),
            fallback_count: self.fallbacks.len() as u64,
            fallback_saturated: self.fallback_saturated,
            merge_events: self.variants.merge_events(),
            budget_refusals: self.variants.budget_refusals(),
        }
    }

    /// 调试回放：把已生成命令渲染成逐行可视化文本（锚点：调试回放）。
    ///
    /// 每行给出「下标 / 字节区间 / 种类（中文名 + 十六进制线编码） / 解码后的关键
    /// 字段」，可直接贴进日志与 CPU 侧录制缓冲逐行比对。**只渲染有效载荷区，
    /// 保留区不出现在文本里**。
    pub fn replay_trace(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut i = 0usize;
        while i < self.recorded {
            let base = i * COMMAND_STRIDE;
            let wire = read_u32(&self.buf, base) as u8;
            let kname = match DrawKind::from_wire(wire) {
                Some(k) => k.zh(),
                None => "未知种类",
            };
            out.push(format!(
                "[{}] byte {}-{} kind={}(0x{:02X}) vtx={} inst={} idx={} base={}",
                i,
                base,
                base + COMMAND_STRIDE,
                kname,
                wire,
                read_u32(&self.buf, base + 4),
                read_u32(&self.buf, base + 8),
                read_u32(&self.buf, base + 20),
                read_u32(&self.buf, base + 24) as i32,
            ));
            i += 1;
        }
        out
    }

    /// 无障碍播报（锚点：命令状态读屏可达）。中英双语逐行。
    ///
    /// 只报**聚合计数与粒度档**，不报单条命令的顶点偏移与实例数——那是资产布局。
    pub fn a11y_lines(&self) -> Vec<String> {
        let s = self.stat();
        let sat = if s.fallback_saturated {
            "（计划表已饱和）"
        } else {
            ""
        };
        vec![
            String::from("间接绘制命令生成器 / Indirect Draw Command Generator"),
            format!(
                "本帧命令数 {}，变体档 {} / commands {} , variants {}",
                s.recorded, s.variants, s.recorded, s.variants
            ),
            format!(
                "预检拦截 {}，CPU 回退 {}{} / intercepted {} , fallback {}{}",
                s.intercepted, s.fallback_count, sat, s.intercepted, s.fallback_count, sat
            ),
            format!(
                "变体粒度 {}，合并 {}，预算拒绝 {} / granularity {} , merges {} , refusals {}",
                s.granularity, s.merge_events, s.budget_refusals, s.granularity, s.merge_events,
                s.budget_refusals
            ),
        ]
    }
}

/// 小端写 u32（无 `unsafe`、无 `transmute`，逐字节可读）。
fn write_u32(buf: &mut [u8], off: usize, v: u32) {
    if off + 4 > buf.len() {
        return;
    }
    buf[off] = (v & 0xff) as u8;
    buf[off + 1] = ((v >> 8) & 0xff) as u8;
    buf[off + 2] = ((v >> 16) & 0xff) as u8;
    buf[off + 3] = ((v >> 24) & 0xff) as u8;
}

/// 小端读 u32（越界返回 0，调用方负责预检）。
fn read_u32(buf: &[u8], off: usize) -> u32 {
    if off + 4 > buf.len() {
        return 0;
    }
    let b0 = buf[off] as u32;
    let b1 = buf[off + 1] as u32;
    let b2 = buf[off + 2] as u32;
    let b3 = buf[off + 3] as u32;
    b0 | (b1 << 8) | (b2 << 16) | (b3 << 24)
}

// ---------------------------------------------------------------------------
// 五、判据（锚点：判据 —— 五个判据族逐条落地，共 31 条）
// ---------------------------------------------------------------------------

/// VE-F0029 判据集。
pub fn run_vea29_checks() -> CheckSet {
    let mut set = CheckSet::new("vea29_indirect");

    // ---- 族一：GPU 侧生成（6 条）-------------------------------------------
    {
        let mut g = CommandGen::new();
        let a = DrawArgs::draw();
        let b = DrawArgs::indexed();
        let c = DrawArgs::dispatch();
        let ok = g.push(&a, 0).is_ok() && g.push(&b, 16).is_ok() && g.push(&c, 32).is_ok();
        set.add(
            "A29-生成-三类命令均写入且字节数恰为条数乘步进",
            ok && g.recorded() == 3 && g.bytes() == 3 * COMMAND_STRIDE,
            "",
        );
    }
    {
        // 字段逐一解码对账：写入的七个字段必须能从缓冲原样读回。
        let mut g = CommandGen::new();
        let mut a = DrawArgs::indexed();
        a.vertex_count = 3;
        a.instance_count = 5;
        a.first_vertex = 7;
        a.first_instance = 2;
        a.index_count = 6;
        a.base_vertex = -4;
        let _ = g.push(&a, 0);
        set.add(
            "A29-生成-字段逐一解码对账",
            g.word(0, 0) == DrawKind::DrawIndexed.wire() as u32
                && g.word(0, 4) == 3
                && g.word(0, 8) == 5
                && g.word(0, 12) == 7
                && g.word(0, 16) == 2
                && g.word(0, 20) == 6
                && g.word(0, 24) as i32 == -4,
            "",
        );
    }
    {
        // 大端取高字节必须 `(v >> 8) as u8`——此处正向断：wire 0x53 的低字节是
        // 0x53、高字节 0x00；若误写成 `v as u8` 后再左移，低高字节会互换。
        let wire = DrawKind::Dispatch.wire() as u32;
        let lo = (wire & 0xff) as u8;
        let hi = ((wire >> 8) & 0xff) as u8;
        set.add(
            "A29-生成-线上编码高低字节不互换且非判别值",
            lo == 0x53 && hi == 0x00 && wire != 2 && wire != 0 && wire == 0x53,
            "",
        );
    }
    {
        // 保留区 28..32 必须恒零且不随命令数漂移。
        let mut g = CommandGen::new();
        let a = DrawArgs::draw();
        let mut i = 0usize;
        while i < MAX_COMMANDS {
            let _ = g.push(&a, (i % 2) * ARG_ALIGN);
            i += 1;
        }
        let mut clean = g.bytes() == MAX_COMMANDS * COMMAND_STRIDE;
        let mut j = 0usize;
        while j < g.bytes() {
            if j % COMMAND_STRIDE >= 28 && g.buffer()[j] != 0 {
                clean = false;
            }
            j += 1;
        }
        set.add("A29-生成-保留区恒零且不随命令数漂移", clean, "");
    }
    {
        // 三类编码往返一致；非枚举的 wire 一律 None（不留静默兜底）。
        let mut ok = true;
        let mut i = 0usize;
        while i < KIND_COUNT {
            let k = DrawKind::ALL[i];
            if DrawKind::from_wire(k.wire()) != Some(k) {
                ok = false;
            }
            i += 1;
        }
        set.add(
            "A29-生成-三类编码往返一致且非法编码拒绝",
            ok && DrawKind::from_wire(0x00).is_none() && DrawKind::from_wire(0x99).is_none(),
            "",
        );
    }
    {
        // 帧首清零：命令缓冲不留存上帧；变体表**必须留存**（跨帧复用资产）。
        let mut g = CommandGen::new();
        let a = DrawArgs::draw();
        let _ = g.push(&a, 0);
        let _ = g.push(&a, 4);
        let before = g.bytes();
        let variants_before = g.variants().len();
        g.begin_frame();
        set.add(
            "A29-生成-帧首清零命令缓冲但保留变体表",
            before == 2 * COMMAND_STRIDE
                && g.bytes() == 0
                && g.recorded() == 0
                && g.variants().len() == variants_before
                && variants_before > 0,
            "",
        );
    }

    // ---- 族二：边界预检（9 条）---------------------------------------------
    {
        // 夹逼对：界前一个合法点 / 界上 / 界后一位。
        // 只断「超限即拒」的宽阈值，会把界上那一点也拒掉而全绿（十诫第 4 条）。
        let a = DrawArgs::draw();
        let lo = precheck(&a, ARG_OFFSET_LIMIT - ARG_ALIGN, 0);
        let at = precheck(&a, ARG_OFFSET_LIMIT, 0);
        let hi = precheck(&a, ARG_OFFSET_LIMIT + ARG_ALIGN, 0);
        set.add(
            "A29-边界-偏移上限夹逼对(界前与界上放行/界后一位拦截)",
            lo.is_ok() && at.is_ok() && hi == Err(PrecheckFail::OffsetOverLimit),
            "",
        );
    }
    {
        // 未对齐专属码；且「既未对齐又超限」时必须报**未对齐**（那才是直接原因）。
        let a = DrawArgs::draw();
        set.add(
            "A29-边界-未对齐偏移专属拦截且优先于上限",
            precheck(&a, 3, 0) == Err(PrecheckFail::OffsetUnaligned)
                && precheck(&a, ARG_OFFSET_LIMIT + 1, 0) == Err(PrecheckFail::OffsetUnaligned)
                && precheck(&a, ARG_ALIGN, 0).is_ok()
                && precheck(&a, ARG_ALIGN - 1, 0) == Err(PrecheckFail::OffsetUnaligned),
            "",
        );
    }
    {
        let mut zero = DrawArgs::draw();
        zero.vertex_count = 0;
        let mut at = DrawArgs::draw();
        at.vertex_count = VERTEX_LIMIT;
        let mut over = DrawArgs::draw();
        over.vertex_count = VERTEX_LIMIT + 1;
        set.add(
            "A29-边界-顶点数零与超限各归各码且夹逼对",
            precheck(&zero, 0, 0) == Err(PrecheckFail::VertexCountZero)
                && precheck(&at, 0, 0).is_ok()
                && precheck(&over, 0, 0) == Err(PrecheckFail::VertexCountOverLimit),
            "",
        );
    }
    {
        let mut zero = DrawArgs::draw();
        zero.instance_count = 0;
        let mut at = DrawArgs::draw();
        at.instance_count = INSTANCE_LIMIT;
        let mut over = DrawArgs::draw();
        over.instance_count = INSTANCE_LIMIT + 1;
        set.add(
            "A29-边界-实例数零与超限各归各码且夹逼对",
            precheck(&zero, 0, 0) == Err(PrecheckFail::InstanceCountZero)
                && precheck(&at, 0, 0).is_ok()
                && precheck(&over, 0, 0) == Err(PrecheckFail::InstanceCountOverLimit),
            "",
        );
    }
    {
        // 起始索引溢出夹逼对：`first_vertex + vertex_count` 恰在 u32::MAX 放行，
        // 再加一即溢出。
        //
        // **两个溢出点，不是三个**：`u32::MAX - 1 + 2` 溢出后回绕成 0，而
        // 「回绕成 0」正是裸 `wrapping_add(..) == 0` 变体会误判为真的那个值——
        // 只测这一个点，回绕加法的实现照样全绿（变异验证当场抓到）。
        // 故再取一个**溢出后非零**的点（`u32::MAX + 5` 回绕成 4），
        // 裸加法在这一点上判false、正确实现判 true，两者行为分岔。
        let mut ok = DrawArgs::draw();
        ok.first_vertex = u32::MAX - 2;
        ok.vertex_count = 2;
        let mut bad = DrawArgs::draw();
        bad.first_vertex = u32::MAX - 1;
        bad.vertex_count = 2;
        let mut bad2 = DrawArgs::draw();
        bad2.first_vertex = u32::MAX;
        bad2.vertex_count = 5;
        set.add(
            "A29-边界-起始索引溢出夹逼对(含回绕非零的第二溢出点)",
            precheck(&ok, 0, 0).is_ok()
                && precheck(&bad, 0, 0) == Err(PrecheckFail::FirstIndexOverflow)
                && precheck(&bad2, 0, 0) == Err(PrecheckFail::FirstIndexOverflow)
                // 判据侧独立确认这两个点确实溢出（不被测对象自证）。
                && u32::MAX.wrapping_add(5) == 4
                && (u32::MAX - 1).wrapping_add(2) == 0,
            "",
        );
    }
    {
        // 被拦截时写入字节数与命令数均不变（预检必须先于写入）。
        let mut g = CommandGen::new();
        let good = DrawArgs::draw();
        let _ = g.push(&good, 0);
        let bytes_before = g.bytes();
        let rec_before = g.recorded();
        let mut zero_v = DrawArgs::draw();
        zero_v.vertex_count = 0;
        let mut zero_i = DrawArgs::draw();
        zero_i.instance_count = 0;
        let r1 = g.push(&zero_v, 0);
        let r2 = g.push(&zero_i, 0);
        set.add(
            "A29-边界-被拦截时写入字节数与命令数均不变",
            r1 == Err(PrecheckFail::VertexCountZero)
                && r2 == Err(PrecheckFail::InstanceCountZero)
                && g.bytes() == bytes_before
                && g.recorded() == rec_before,
            "",
        );
    }
    {
        // 单帧命令数夹逼对：恰好 MAX_COMMANDS 条全通，第 MAX_COMMANDS+1 条被拒，
        // 且**缓冲区不得为半条命令**（不得出现非整步进的尾字节）。
        let mut g = CommandGen::new();
        let a = DrawArgs::draw();
        let mut i = 0usize;
        let mut ok_count = 0usize;
        while i < MAX_COMMANDS + 1 {
            if g.push(&a, 0).is_ok() {
                ok_count = ok_count.saturating_add(1);
            }
            i += 1;
        }
        set.add(
            "A29-边界-单帧命令数上限恰好放行上限条且无半条命令",
            ok_count == MAX_COMMANDS
                && g.recorded() == MAX_COMMANDS
                && g.bytes() == MAX_COMMANDS * COMMAND_STRIDE
                && g.bytes() % COMMAND_STRIDE == 0,
            "",
        );
    }
    {
        // 九种失败原因全部可达（判别变体若无真实产生路径即为死码）。
        let mut g = CommandGen::new();
        let mut seen = [0u32; FAIL_COUNT];
        let mut zv = DrawArgs::draw();
        zv.vertex_count = 0;
        let mut bv = DrawArgs::draw();
        bv.vertex_count = VERTEX_LIMIT + 1;
        let mut zi = DrawArgs::draw();
        zi.instance_count = 0;
        let mut bi = DrawArgs::draw();
        bi.instance_count = INSTANCE_LIMIT + 1;
        let mut ovf = DrawArgs::draw();
        ovf.first_vertex = u32::MAX;
        ovf.vertex_count = 1;
        let a = DrawArgs::draw();
        // 注：`g.push` 与 `seen` 借用不能交叠，故先算结果再记账，不闭包套 `g`。
        let mut r = g.push(&zv, 0);
        if r.is_err() {
            seen[fail_index(r.err().unwrap_or(PrecheckFail::VertexCountZero))] =
                seen[fail_index(PrecheckFail::VertexCountZero)].saturating_add(1);
        }
        r = g.push(&bv, 0);
        if r.is_err() {
            seen[fail_index(PrecheckFail::VertexCountOverLimit)] =
                seen[fail_index(PrecheckFail::VertexCountOverLimit)].saturating_add(1);
        }
        r = g.push(&zi, 0);
        if r.is_err() {
            seen[fail_index(PrecheckFail::InstanceCountZero)] =
                seen[fail_index(PrecheckFail::InstanceCountZero)].saturating_add(1);
        }
        r = g.push(&bi, 0);
        if r.is_err() {
            seen[fail_index(PrecheckFail::InstanceCountOverLimit)] =
                seen[fail_index(PrecheckFail::InstanceCountOverLimit)].saturating_add(1);
        }
        r = g.push(&ovf, 0);
        if r.is_err() {
            seen[fail_index(PrecheckFail::FirstIndexOverflow)] =
                seen[fail_index(PrecheckFail::FirstIndexOverflow)].saturating_add(1);
        }
        r = g.push(&a, 3);
        if r.is_err() {
            seen[fail_index(PrecheckFail::OffsetUnaligned)] =
                seen[fail_index(PrecheckFail::OffsetUnaligned)].saturating_add(1);
        }
        r = g.push(&a, ARG_OFFSET_LIMIT + ARG_ALIGN);
        if r.is_err() {
            seen[fail_index(PrecheckFail::OffsetOverLimit)] =
                seen[fail_index(PrecheckFail::OffsetOverLimit)].saturating_add(1);
        }
        let mut i = 0usize;
        while i < MAX_COMMANDS {
            let _ = g.push(&a, 0);
            i += 1;
        }
        r = g.push(&a, 0);
        if r.is_err() {
            seen[fail_index(PrecheckFail::CommandCountOverflow)] =
                seen[fail_index(PrecheckFail::CommandCountOverflow)].saturating_add(1);
        }
        let mut n = 0usize;
        let mut rej = 0u32;
        let mut t = VariantTable::new();
        while n < MAX_VARIANTS * 8 {
            if t.intern(&burst_args(n)) == Err(PrecheckFail::VariantBudgetExhausted) {
                rej = rej.saturating_add(1);
            }
            n += 1;
        }
        seen[fail_index(PrecheckFail::VariantBudgetExhausted)] = rej;
        let mut all = true;
        let mut i = 0usize;
        while i < FAIL_COUNT {
            if seen[i] == 0 {
                all = false;
            }
            i += 1;
        }
        set.add("A29-边界-九种失败原因全部可达且下标不重叠", all, "");
    }

    // ---- 族三：CPU 回退（4 条）---------------------------------------------
    {
        // 独立重算本轮真实拦截数，断 fallback_count **恰等于**该数（十诫第 10 条）。
        let mut g = CommandGen::new();
        let good = DrawArgs::draw();
        let _ = g.push(&good, 0);
        let mut zero_v = DrawArgs::draw();
        zero_v.vertex_count = 0;
        let mut zero_i = DrawArgs::draw();
        zero_i.instance_count = 0;
        let mut big_v = DrawArgs::draw();
        big_v.vertex_count = VERTEX_LIMIT + 1;
        let cases = [zero_v, zero_i, big_v];
        let mut want = 0usize;
        let mut i = 0usize;
        while i < cases.len() {
            let off = i * ARG_ALIGN;
            // 判据侧独立重算：不问生成器，自己判这条该不该被拒。
            if precheck(&cases[i], off, g.recorded()).is_err() {
                want = want.saturating_add(1);
            }
            let _ = g.push(&cases[i], off);
            i += 1;
        }
        let _ = g.push(&good, 0);
        set.add(
            "A29-回退-回退条数恰等于独立重算的拦截数",
            want == 3
                && g.fallbacks().len() == want
                && g.stat().fallback_count == want as u64
                && g.intercepted_total() == want as u64
                && g.recorded() == 2,
            "",
        );
    }
    {
        // 回退计划保留种类与原始偏移（否则调用方无法真正回退）。
        let mut g = CommandGen::new();
        let mut bad = DrawArgs::dispatch();
        bad.instance_count = 0;
        let _ = g.push(&bad, 48);
        let mut ok = g.fallbacks().len() == 1;
        if let Some(p) = g.fallbacks().first() {
            ok = ok
                && p.kind == DrawKind::Dispatch
                && p.reason == PrecheckFail::InstanceCountZero
                && p.arg_offset == 48;
        }
        set.add("A29-回退-计划保留种类与原始偏移", ok, "");
    }
    {
        // 各原因计数独立不串码；且被拒命令不进缓冲（越界偏移那次的字节数不变）。
        let mut g = CommandGen::new();
        let mut zv = DrawArgs::draw();
        zv.vertex_count = 0;
        let mut zi = DrawArgs::draw();
        zi.instance_count = 0;
        let good = DrawArgs::draw();
        let _ = g.push(&zv, 0);
        let _ = g.push(&zi, 0);
        let _ = g.push(&good, 0);
        let bytes_before = g.bytes();
        let _ = g.push(&good, ARG_OFFSET_LIMIT + ARG_ALIGN);
        set.add(
            "A29-回退-各原因计数独立不串码且越界不进缓冲",
            g.intercepted_of(PrecheckFail::VertexCountZero) == 1
                && g.intercepted_of(PrecheckFail::InstanceCountZero) == 1
                && g.intercepted_of(PrecheckFail::OffsetOverLimit) == 1
                && g.intercepted_of(PrecheckFail::OffsetUnaligned) == 0
                && g.recorded() == 1
                && g.bytes() == bytes_before,
            "",
        );
    }
    {
        // 变体预算耗尽**不进** CPU 回退（资源健康时回退无意义，直接拒绝）。
        set.add(
            "A29-回退-变体预算耗尽不进CPU回退",
            !PrecheckFail::VariantBudgetExhausted.fallbacks_to_cpu()
                && PrecheckFail::VertexCountZero.fallbacks_to_cpu()
                && PrecheckFail::OffsetOverLimit.fallbacks_to_cpu(),
            "",
        );
    }

    // ---- 族四：变体合并（6 条）---------------------------------------------
    {
        // 同形状复用：仅 `first_instance` 不同的两条命令拿到**同一**变体号。
        let mut g = CommandGen::new();
        let mut a = DrawArgs::draw();
        a.instance_count = 1;
        let mut b = DrawArgs::draw();
        b.instance_count = 1;
        b.first_instance = 9;
        //两条命令的落缓冲偏移都必须成功，才谈得上「拿到同一变体号」。
        //失败臂单列一条判据并给出专属名与原因，而不是复用成功臂的名字：
        //同名会让红项定位时两条判据在报告里无法区分（CheckSet 按位追加，
        //同名不覆盖但读起来像一条），且失败原因被吞成空 detail。
        match (g.push(&a, 0), g.push(&b, 4)) {
            (Ok(v1), Ok(v2)) => set.add(
                "A29-变体-同形状命令复用同一变体号",
                v1 == v2 && g.variants().len() == 1 && g.stat().intern_hits == 1,
                "两条仅 first_instance 不同的同形状命令应复用同一变体号",
            ),
            _ => set.add(
                "A29-变体-同形状基线写入被拦截",
                false,
                "同形状复用判据的前置：两条 draw 必须都能写入（偏移 0 与 4 均合法），\
                 否则变体号无从比较",
            ),
        }
    }
    {
        // 跨类不共享变体（种类是 key 的一部分）——否则「变体」跨语义混用。
        let mut t = VariantTable::new();
        let a = t.intern(&DrawArgs::draw());
        let b = t.intern(&DrawArgs::indexed());
        let c = t.intern(&DrawArgs::dispatch());
        set.add(
            "A29-变体-三类命令各占独立变体",
            a.is_ok() && b.is_ok() && c.is_ok()
                && a.ok() != b.ok()
                && b.ok() != c.ok()
                && t.len() == 3,
            "",
        );
    }
    {
        // 变体爆炸 → 合并：灌满表后新形状触发粒度递进，表**不超上限**且变体号无洞。
        let mut t = VariantTable::new();
        let mut i = 0usize;
        while i < MAX_VARIANTS {
            let _ = t.intern(&burst_args(i));
            i += 1;
        }
        let full = t.len() == MAX_VARIANTS;
        let merges_before = t.merge_events();
        let gran_before = t.granularity();
        let extra = t.intern(&burst_args(MAX_VARIANTS));
        set.add(
            "A29-变体-变体爆炸触发合并且表不超上限",
            full
                && extra.is_ok()
                && t.merge_events() > merges_before
                && t.granularity() > gran_before
                && t.granularity() <= MAX_GRANULARITY
                && t.len() <= MAX_VARIANTS
                && t.vids_dense(),
            "",
        );
    }
    {
        // 合并的**产物**判据：合并后档数必须**严格减少但不塌成一档**。
        //
        // 迭代记录（两次修正，都是变异验证抓出来的）：
        //  · 第一版只断「表不超上限 + 变体号无洞」⇒ 错误实现「合并 = 清表」照样
        //    全绿（空表 `len()=0 ≤ 上限`，`vids_dense()` 对空表也成立）。
        //  · 第二版断 `after_len > 0`，仍漏网：实测该错误实现合并后是
        //    `len() == 1`（不是 0，粒度已递进到 1 且空表随后被重新插入一条），
        //    `1 > 0` 成立故照样通过。
        // 正确实现在同一输入下合并后为 19 档——粒度递进的**语义**是「粗化分档
        // 让更多 key 塌进既有变体」，不是「把变体全灭掉重来」。故此处钉死下界：
        // 合并后必须仍保有**多个**变体，且与合并前的满表相比是**真减少**。
        let mut t = VariantTable::new();
        let mut i = 0usize;
        while i < MAX_VARIANTS {
            let _ = t.intern(&burst_args(i));
            i += 1;
        }
        let before_len = t.len();
        let gran_before = t.granularity();
        let merged = t.intern(&burst_args(MAX_VARIANTS));
        let after_len = t.len();
        set.add(
            "A29-变体-合并产物严格减少但不塌成一档",
            merged.is_ok()
                && before_len == MAX_VARIANTS
                && gran_before == 0
                && after_len < before_len
                && after_len > 1
                && t.vids_dense()
                // 合并后仍能继续吃命令（表活着，不是空壳）。
                && t.intern(&burst_args(MAX_VARIANTS + 1)).is_ok()
                && t.len() > after_len,
            "",
        );
    }
    {
        // 粒度递进使同一数值的档号**单调不增**（合并只粗化、不细分——数学依据）。
        let mut mono = true;
        let mut g = 1u8;
        while g <= MAX_GRANULARITY {
            if magnitude(3000u32, g) > magnitude(3000u32, g - 1) {
                mono = false;
            }
            if magnitude(7u32, g) > magnitude(7u32, g - 1) {
                mono = false;
            }
            g += 1;
        }
        set.add(
            "A29-变体-粒度递进使分档单调不增",
            mono
                && magnitude(3000u32, 0) >= magnitude(3000u32, MAX_GRANULARITY)
                && magnitude(7u32, MAX_GRANULARITY) == 0,
            "",
        );
    }
    {
        // 粒度到顶后预算拒绝**确实发生**且计数增长（该错误码不是死码）。
        let mut t = VariantTable::new();
        let mut n = 0usize;
        let mut rej = 0usize;
        while n < MAX_VARIANTS * 8 {
            if t.intern(&burst_args(n)) == Err(PrecheckFail::VariantBudgetExhausted) {
                rej = rej.saturating_add(1);
            }
            n += 1;
        }
        set.add(
            "A29-变体-粒度到顶后预算拒绝计数增长",
            rej > 0
                && t.budget_refusals() == rej as u64
                && t.granularity() == MAX_GRANULARITY
                && t.vids_dense(),
            "",
        );
    }
    {
        // 变体计数与不同 key 数**绝对值对账**（净值口径会被「建了又销」骗过）。
        let mut t = VariantTable::new();
        let mut max_vid = 0u32;
        let mut i = 0usize;
        while i < 16 {
            let mut a = DrawArgs::draw();
            a.instance_count = 1 + (i as u32);
            match t.intern(&a) {
                Ok(v) => {
                    if v >= max_vid {
                        max_vid = v + 1;
                    }
                }
                Err(_) => {
                    max_vid = 9999;
                }
            }
            i += 1;
        }
        set.add(
            "A29-变体-变体计数与不同key数绝对值对账",
            t.len() == 16 && max_vid == 16 && t.vids_dense(),
            "",
        );
    }

    // ---- 族五：调试回放与读屏（6 条）--------------------------------------
    {
        // 回放逐行：行数等于命令数，每行含字节区间与解码种类名。
        let mut g = CommandGen::new();
        let _ = g.push(&DrawArgs::draw(), 0);
        let _ = g.push(&DrawArgs::indexed(), 16);
        let tr = g.replay_trace();
        let l0 = match tr.first() {
            Some(s) => s.contains("byte 0-32") && s.contains("非索引绘制"),
            None => false,
        };
        let l1 = match tr.get(1) {
            Some(s) => s.contains("byte 32-64") && s.contains("索引绘制"),
            None => false,
        };
        set.add("A29-回放-逐行含字节区间与解码种类", tr.len() == 2 && l0 && l1, "");
    }
    {
        // 回放字段值与缓冲一字不差（判据侧独立算出期望串，不问被测对象）。
        let mut g = CommandGen::new();
        let mut a = DrawArgs::dispatch();
        a.instance_count = 6;
        a.first_vertex = 11;
        let _ = g.push(&a, 0);
        let expect = String::from("vtx=1 inst=6");
        let tr = g.replay_trace();
        set.add(
            "A29-回放-字段值与写入参数一字不差",
            tr.first().map(|s| s.contains(&expect[..])).unwrap_or(false)
                && g.word(0, 8) == 6
                && g.word(0, 12) == 11,
            "",
        );
    }
    {
        // 未知线编码必须显式渲染为「未知种类」，不得静默兜底成某个已知种类。
        //
        // **这条判据必须验在生产函数上**。第一版写成「另写一个判据侧渲染器
        // `replay_one` 验同样的逻辑」，于是变异改 `replay_one` 时生产代码
        // `replay_trace` 原封不动 → 判据恒绿（假阴性，变异验证当场抓到）。
        // 修法：判据主体改为**穷举 wire 值域**，直接断言生产解码函数
        // [`DrawKind::from_wire`] 的行为——「解得出的必属全集且能原样回去，
        // 解不出的绝不等于任何一个已知种类」，并额外断生产回放
        // [`CommandGen::replay_trace`] 在正常语料上不误报未知。
        let mut g = CommandGen::new();
        let _ = g.push(&DrawArgs::draw(), 0);
        let _ = g.push(&DrawArgs::indexed(), 16);
        // 要件一：语料合规——正常命令流每行都解出已知种类名，不误报未知。
        let clean = g.replay_trace();
        let all_named = clean.len() == 2
            && clean[0].contains("非索引绘制")
            && clean[1].contains("索引绘制")
            && !clean[0].contains("未知种类")
            && !clean[1].contains("未知种类");
        // 要件二：真越界——穷举全部 256 个 wire，逐个核对解码契约。
        let mut exhaustive_ok = true;
        let mut named = 0u32;
        let mut w = 0u32;
        while w <= 255 {
            match DrawKind::from_wire(w as u8) {
                Some(k) => {
                    named = named.saturating_add(1);
                    // 解出来的必须能原样回去，且属于全集（不得凭空造出第四种）。
                    if k.wire() as u32 != w || k.ordinal() >= KIND_COUNT {
                        exhaustive_ok = false;
                    }
                }
                None => {
                    // 解不出的，绝不能等于任何一个已知 wire——那正是静默兜底。
                    let mut i = 0usize;
                    while i < KIND_COUNT {
                        if DrawKind::ALL[i].wire() as u32 == w {
                            exhaustive_ok = false;
                        }
                        i += 1;
                    }
                }
            }
            w += 1;
        }
        // 件数对账：256 个 wire 里恰好 KIND_COUNT 个解得出。
        let count_ok = named == KIND_COUNT as u32;
        set.add(
            "A29-回放-未知线编码显式渲染不静默兜底",
            all_named && exhaustive_ok && count_ok,
            "",
        );
    }
    {
        // 读屏四行齐备且含聚合计数；不泄漏单条命令的顶点偏移与实例数。
        //
        // 判据侧**独立枚举**单条命令可能泄漏的字段值（而不是只查 777 一个数）：
        // 只查一个常量的话，实现改成泄漏别的字段就抓不到——那仍是弱门禁。
        let mut g = CommandGen::new();
        let mut a = DrawArgs::draw();
        a.instance_count = 777;
        a.first_vertex = 4242;
        a.first_instance = 31337;
        let _ = g.push(&a, 0);
        let mut b = DrawArgs::draw();
        b.vertex_count = 0;
        let _ = g.push(&b, 0);
        let lines = g.a11y_lines();
        let mut has_count = false;
        let mut has_fallback = false;
        let mut has_en = false;
        let mut leaks = false;
        let mut i = 0usize;
        while i < lines.len() {
            if lines[i].contains("本帧命令数") {
                has_count = true;
            }
            if lines[i].contains("CPU 回退") {
                has_fallback = true;
            }
            if lines[i].contains("Indirect Draw") {
                has_en = true;
            }
            // 单条命令的三个特征值一个都不许出现在播报里。
            if lines[i].contains("777")
                || lines[i].contains("4242")
                || lines[i].contains("31337")
            {
                leaks = true;
            }
            i += 1;
        }
        // 反向自证：这三串确实进了缓冲（否则「不泄漏」是因为压根没数据，恒真）。
        let in_buffer = g.word(0, 8) == 777 && g.word(0, 12) == 4242 && g.word(0, 16) == 31337;
        set.add(
            "A29-读屏-四行齐备双语且不泄漏单条命令参数",
            lines.len() == 4 && has_count && has_fallback && has_en && !leaks && in_buffer,
            "",
        );
    }
    {
        // 九种失败原因双语标签齐备（读屏可达的最小要求：不能有空标签）。
        let mut ok = true;
        let mut i = 0usize;
        while i < FAIL_COUNT {
            let f = fail_at(i);
            if f.zh().is_empty() || f.tag().is_empty() || fail_index(f) != i {
                ok = false;
            }
            i += 1;
        }
        set.add("A29-读屏-失败原因双语标签齐备且下标自洽", ok, "");
    }
    {
        // 统计快照与生成器实际状态逐项对账（跨批对接点 A30 的唯一出口不许漂）。
        let mut g = CommandGen::new();
        let a = DrawArgs::draw();
        let mut i = 0usize;
        while i < 7 {
            let _ = g.push(&a, 0);
            i += 1;
        }
        let s = g.stat();
        set.add(
            "A29-读屏-统计快照与实际状态逐项对账",
            s.recorded == g.recorded()
                && s.bytes == g.bytes()
                && s.variants == g.variants().len()
                && s.granularity == g.variants().granularity()
                && s.intern_hits == g.variants().intern_hits()
                && s.bytes == s.recorded * COMMAND_STRIDE
                && s.fallback_saturated == g.fallback_saturated(),
            "",
        );
    }

    finish(set)
}

/// 收口：判据集自带条数上界，超限如实报红（[`CheckSet::add`] 已按此记账）。
fn finish(set: CheckSet) -> CheckSet {
    set
}

// ---------------------------------------------------------------------------
// 六、判据支撑（仅判据使用，不参与生产路径）
// ---------------------------------------------------------------------------

/// 造一条「形状各异」的合法命令，供变体表灌爆用。
///
/// 刻意让 `vertex_count` 与 `instance_count` 同时取四个**跨档**值，使粒度递进
/// 到顶时仍能产出多于 [`MAX_VARIANTS`] 个互异 key——否则「变体预算耗尽」那条
/// 错误码在本判据里不可达，会变成死码（十诫第 13 条）。
pub fn burst_args(n: usize) -> DrawArgs {
    // 四档取值须同时满足两个量化口径的互异性，否则变体表灌不满：
    //  · g == 0 取 `value & 0x0f`，故低 4 位必须两两不同 → 各档尾数取 1/2/3/4；
    //  · g == MAX_GRANULARITY 取对数档（阈值 1<<10 / 1<<14 / 1<<18），故量级
    //    必须两两不同 → 各档取 1 / 1<<10+2 / 1<<14+3 / 1<<18+4。
    // 两口径都互异，粒度从 0 递进到顶的过程中变体表才灌得满、合并才真的触发。
    const V: [u32; 4] = [1, 1_026, 16_387, 262_148];
    const I: [u32; 4] = [1, 1_026, 16_387, 262_148];
    const X: [u32; 4] = [0, 1_026, 16_387, 262_148];
    // **基数必须与各字段的天然基数匹配**：种类 3 档、顶点数 4 档、实例数 4 档、
    // 索引数 4 档、首顶点高位 2 档、基顶点符号 2 档，故位段基数取 3/4/4/4/2/2。
    // 早先误用 4/4/4/2/2（种类挤进 4 档）会让 `g == 0` 下最多只产出 16 个互异
    // key，而表容量是 [`MAX_VARIANTS`] = 24 —— 表永远灌不满，于是
    // 「变体爆炸 → 合并」与「粒度到顶 → 拒绝」两条路径在**设计上**不可达，
    // 对应判据恒红且无法靠改判据变绿。基数对齐后 `n < 768` 逐条互异。
    DrawArgs {
        kind: DrawKind::ALL[n % 3],
        vertex_count: V[(n / 3) % 4],
        instance_count: I[(n / 12) % 4],
        first_vertex: ((n / 192) % 2) as u32 * 8_000,
        first_instance: 0,
        index_count: X[(n / 48) % 4],
        base_vertex: if (n / 384) % 2 == 0 { -1 } else { 0 },
    }
}
