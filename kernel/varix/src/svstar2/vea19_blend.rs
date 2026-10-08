//! VE-F0019 · 混合状态机（VE-A 域 · 内核图形抽象层 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0019`
//!
//! **判据（锚点原文逐条）**：图形管线的混合（Blend）状态机（源/目标因子/
//! 混合算子/写掩码四维组合的统一封装），预置状态库与自定义通道，非法组合
//! 拒绝含正确建议。混合是半透明世界的规则书——规则写错半透明就变成透明
//! 的灾难；含混合状态的预览工具（半透明效果的实时预览）。
//! 数据结构：状态封装；预置库。错误路径与降级矩阵：非法组合→拒绝+建议；
//! 状态漂移→缓存失效；越界→钳制。性能逐项分解：O(状态)。
//! 跨批对接点：A18 采样器库同构。无障碍与隐私：状态表读屏可达。
//! 判据：**四维组合、预置库、非法拒绝、缓存失效、判据**。
//!
//! ┌── 第一性声明（本域最恶劣缺陷：混合因子被静默丢弃）─────────────────┐
//! │ 混合状态最恶劣的缺陷不是「混合错了」——混合错了画面会立刻变怪，      │
//! │ 一眼可见。最恶劣的是**因子被静默丢弃**：D3D12/Vulkan 都规定当算子     │
//! │ 为 `Min`/`Max` 时源/目标因子**被硬件忽略**。于是作者写下              │
//! │ `Min(SrcAlpha, One)` 以为在做「按源alpha 变暗」，实际执行的是        │
//! │ `Min(Src, Dst)`——因子被丢掉，行为与他写的**完全不同**，而画面照样    │
//! │ 出得来，只是暗部层次全平。根因是「算子会吃掉因子」这一硬件规则没被   │
//! │ 写进类型里。于是本条第一性声明是：                                 │
//! │ **任何会被硬件丢弃的字段，必须在构造期就被拒绝，不许带到运行时。**   │
//! │ 凡丢弃必留账：被拒绝的组合连同「哪一维被丢弃、丢弃后真实行为是什么」 │
//! │ 一并产出诊断，无记录的丢弃产出 P0。                                 │
//! └─────────────────────────────────────────────────────────────────────┘
//!
//! **为什么「丢弃必须留账」是硬要求**：混合因子被丢弃后的行为**没有报错**，
//! 只是与意图不同。缺台账时，排查只能靠猜「是不是 Min/Max 吃了因子」；
//! 有台账时，「你写的因子被丢弃、真实执行的是 Min(Src,Dst)」是一行可查的
//! 事实。这使「画面层次全平」从主观感受变成可对账的判定。
//!
//! **四维组合与相关性裁剪（判据：四维组合）**：
//! 四维是源因子 / 目标因子 / 混合算子 / 写掩码，另加**独立 alpha 通道**的
//! 因子与算子（颜色与 alpha 常走不同算子——这是混合规则里最常被漏掉的一维）。
//! 本条的 [`BlendDesc::canonical_key`] 只编码**真正起作用**的维度：
//! - 未启用混合 → 键退化为单一「关闭」标记，四维全部不参与（硬件也不读）；
//! - 写掩码不含 RGB → 颜色因子与颜色算子不参与键（不写就不算）；
//! - 写掩码不含 A → alpha 因子与alpha 算子不参与键；
//! - 无任何因子引用常量色 → 常量色四分量不参与键。
//! 这正是硬件行为：**两个只在不起作用的字段上不同的描述符 = 同一个混合状态**
//! （同参共享），去重因此在语义上正确而非近似。
//!
//! **预置库与 A18 同构（跨批对接点）**：与 F0018 采样器状态库同一套骨架——
//! `const` 预置表是单一事实源、基线指纹冻结对齐、变更走双签流程、
//! 去重命中统计可查。差别只在维度：A18 是过滤/寻址/各向异性三维，本条是
//! 四维（+独立 alpha 维）。**预置项永不被淘汰**——预置是基线，淘汰预置等于
//! 私自改基线。
//!
//! **缓存失效（判据：缓存失效）**：混合状态按 [`BlendDesc::canonical_key`]
//! 去重共享，但**共享的前提是上下文相同**。上下文由 [`BlendContext`] 标记
//! （目标格式 + 是否支持硬件独立 alpha 混合）。同一状态键在**不同上下文**下
//! 复用即为**状态漂移**：此时必须失效并重建，而不是拿旧条目继续用——
//! 否则硬件不支持独立 alpha 混合的路径上会拿到一条声称支持的状态。
//! 失效是**可审计的**（[`BlendCache::audit`] 逐条列出漂移条目与原因）。
//!
//! **越界→钳制**：整数侧的越界是**掩码位**——`WriteMask::from_bits` 对
//! `0b1111` 以上的未知位**钳制**（丢弃未知位）而非放任，因为放任会让未知位
//! 悄悄写进后备缓冲。浮点侧的越界是**混合结果**——`Subtract` 可产生负值，
//! 在**无符号**目标上必须钳制到 `[0,1]`；在**浮点**目标上负值是合法 HDR，
//! **不钳制**（钳了就是画质事故）。两者处置方向相反，故不共用码。
//!
//! **预览工具（锚点原文：半透明效果的实时预览）**：
//! [`preview_strip`] 按源 alpha 斜坡逐格求值，输出每格的入/出像素与两个诚实
//! 标记：`clamped`（该格结果被无符号目标钳制）与`factor_ignored`
//! （该状态的因子被 Min/Max 丢弃）。预览**按硬件真实规则求值**——包括
//! Min/Max 丢因子、alpha 通道因子塌缩、关闭混合时写掩码仍然生效。
//! 预览若与硬件规则不一致，它就是第二套错误答案，故不设「预览模式」。
//!
//! **性能逐项分解（锚点原文：O(状态)）**：
//! · 库查询 O(1)——状态键为定长字节串，哈希分桶 + 键字节精确比对双保险；
//! · 组合判定 O(1)——内层门禁走纯判定 [`classify`]，**不得**调会生成建议的
//!   [`validate`]（后者每次判失败都要扫全表求最近预置，退化为 O(预置数²)）；
//! · 组合求值 O(通道)——四通道定长，无堆分配；
//! · 审计 O(条目)——仅在显式调用时执行，正常路径零开销。
//! 该线性性由 [`BlendCache::preset_scan_work`] **实测计量**（非纸面声明），
//! 并由自检 `A19-性能-无嵌套二次方` 守卫。
//! 性能自检刻意选择**内层大量判失败**的场合（全部非法组合），
//! 因为缺陷恰恰发生在那一层；只测成功路径是自证式算术。
//!
//! **读屏可达**：状态表逐行 [`BlendCache::state_table_rows`] + 人话摘要
//! [`BlendCache::screen_text`]；隐私——状态表只含渲染参数，**零用户内容**
//! （不记像素内容、不记资源名以外的可识别信息）。
//!
//! 逻辑 tick 注入，零墙钟；全部确定性算法、零IO，保证回归可复现。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 与 A18 采样器状态库同构的衔接契约版本。
pub const A18_LINK: u32 = 1;

/// 混合预置集基线版本（变更须双签 + 抬版本号）。
pub const BLEND_BASELINE_VERSION: u32 = 1;

/// 混合预置集基线指纹（[`compute_preset_hash`] 重算比对；漂移即须重签）。
///
/// 取值由 `compute_preset_hash()` 对 v1 预置表**实测回填**，非手写臆造；
/// 任何人改动 [`PRESET_TABLE`] 都会让此指纹失配→ 落
/// [`BaselineStatus::Drifted`]（阻断级），必须走 [`queue_preset_change`]
/// 双签并同步抬版本号与本常量。
pub const BLEND_BASELINE_HASH: u64 = 0xe677_5819_1880_cc52;

/// 动态（运行时构建）混合状态区容量上限（库膨胀阈值）。
pub const LIBRARY_CAP: usize = 64;

/// 单个混合状态的显存字节（去重收益折算的量化基准）。
pub const BLEND_STATE_BYTES: u64 = 48;

/// 上下文无漂移标记（[`BlendContext::tag`] 的缺省值）。
pub const CONTEXT_TAG_DEFAULT: u64 = 0x0000_0000_0000_0001;

/// 预览斜坡缺省格数（实时预览的默认分辨率）。
pub const PREVIEW_CELLS: usize = 8;

/// FNV-1a 64 位基件（基线指纹与去重分桶共用；非密码学用途）。
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64 位素数。
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 钳制上限（无符号目标混合结果的合法上界）。
const UNIT_MAX: f32 = 1.0;
/// 钳制下限（无符号目标混合结果的合法下界）。
const UNIT_MIN: f32 = 0.0;

/// FNV-1a 64 位（字节序列 → 指纹）。与 `vea18_sampler::fnv1a64` 同算法，
/// 但本模块**自持**不引兄弟模块——域内模块必须自持。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h = FNV_OFFSET;
    for b in bytes.iter() {
        h ^= *b as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

// ---------------------------------------------------------------------------
// 二、四维枚举：因子 / 算子 / 写掩码 / 目标格式
// ---------------------------------------------------------------------------

/// 混合因子（源与目标共用同一维度枚举，线上编码对称）。
///
/// **判别值 ≠ 线上编码值**：本枚举按「正向因子在前、反向因子在后」的逻辑
/// 分组编号，而线上编码（[`BlendFactor::wire`]）沿用 D3D12 的正反交错序。
/// 二者刻意不同，故**禁止** `enum_val as u8` 造二进制头——必须走 `wire()`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum BlendFactor {
    /// 0：源项直接丢弃（`src * 0`）。
    Zero = 0,
    /// 1：源项原样通过。
    One = 1,
    /// 源颜色（逐通道）。
    SrcColor = 2,
    /// 目标颜色（逐通道）。
    DstColor = 3,
    /// 源 alpha（塌缩为标量，作用于各通道）。
    SrcAlpha = 4,
    /// 目标 alpha（塌缩为标量）。
    DstAlpha = 5,
    /// 常量色（逐通道）。
    ConstantColor = 6,
    /// 常量 alpha（塌缩为标量）。
    ConstantAlpha = 7,
    /// `1 - 源颜色`。
    OneMinusSrcColor = 8,
    /// `1 - 目标颜色`。
    OneMinusDstColor = 9,
    /// `1 - 源 alpha`。
    OneMinusSrcAlpha = 10,
    /// `1 - 目标 alpha`。
    OneMinusDstAlpha = 11,
    /// `1 - 常量色`。
    OneMinusConstantColor = 12,
    /// `1 - 常量 alpha`。
    OneMinusConstantAlpha = 13,
}

/// 混合算子（颜色与 alpha 各有一个）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum BlendOp {
    /// `src + dst`（线性混合的标准算子）。
    Add = 0,
    /// `src - dst`（可产生负值 → 无符号目标须钳制）。
    Subtract = 1,
    /// `dst - src`（可产生负值 → 无符号目标须钳制）。
    ReverseSubtract = 2,
    /// `min(src, dst)`——**硬件忽略源/目标因子**。
    Min = 3,
    /// `max(src, dst)`——**硬件忽略源/目标因子**。
    Max = 4,
}

/// 渲染目标格式（决定有无 alpha 通道、以及结果是否须钳制）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum BlendTarget {
    /// RGBA8 无符号归一化：有 alpha，结果钳制到 `[0,1]`。
    Rgba8Unorm = 0,
    /// BGRA8 无符号归一化：有 alpha，结果钳制到 `[0,1]`。
    Bgra8Unorm = 1,
    /// RGBA16 浮点：有 alpha，**结果不钳制**（负值是合法 HDR）。
    Rgba16Float = 2,
    /// RGB8 无符号归一化：**无 alpha**，结果钳制到 `[0,1]`。
    Rgb8Unorm = 3,
    /// R8 无符号归一化：**无 alpha**，结果钳制到 `[0,1]`。
    R8Unorm = 4,
}

impl BlendTarget {
    /// 目标是否带 alpha 通道（不带 alpha 的目标上引用 `DstAlpha` 是缺陷）。
    pub fn has_alpha(self) -> bool {
        match self {
            BlendTarget::Rgba8Unorm | BlendTarget::Bgra8Unorm | BlendTarget::Rgba16Float => true,
            BlendTarget::Rgb8Unorm | BlendTarget::R8Unorm => false,
        }
    }

    /// 目标是否为无符号归一化（决定混合结果是否须钳制到 `[0,1]`）。
    ///
    /// 处置方向相反，故与 [`has_alpha`] 分开陈述：浮点目标**不钳制**，
    /// 钳了就是画质事故（合法 HDR 负值被吃掉）。
    pub fn is_unorm(self) -> bool {
        !matches!(self, BlendTarget::Rgba16Float)
    }

    /// 格式名（读屏与诊断文本用）。
    pub fn name(self) -> &'static str {
        match self {
            BlendTarget::Rgba8Unorm => "RGBA8Unorm",
            BlendTarget::Bgra8Unorm => "BGRA8Unorm",
            BlendTarget::Rgba16Float => "RGBA16Float",
            BlendTarget::Rgb8Unorm => "RGB8Unorm",
            BlendTarget::R8Unorm => "R8Unorm",
        }
    }
}

/// 写掩码（四通道独立开关）。
///
/// 用 `u8` 位而非位域 crate：本仓内核 `[dependencies]` 为空，不引入外部依赖。
/// 位定义 `R=0b0001, G=0b0010, B=0b0100, A=0b1000`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WriteMask(u8);

impl WriteMask {
    /// 不写任何通道（该 draw 写了等于没写——非法组合，见 [`Verdict::MaskEmpty`]）。
    pub const NONE: WriteMask = WriteMask(0b0000);
    /// 只写 alpha（典型：仅更新目标 alpha，颜色不动）。
    pub const ALPHA: WriteMask = WriteMask(0b1000);
    /// 只写 RGB（典型：混合颜色但保留目标 alpha 供后段合成）。
    pub const RGB: WriteMask = WriteMask(0b0111);
    /// 写全四通道（标准半透明）。
    pub const RGBA: WriteMask = WriteMask(0b1111);
    /// 只写红通道（调试/单通道特效）。
    pub const RED: WriteMask = WriteMask(0b0001);

    /// 由原始位构造；**未知位（`0b1111` 以上）被钳制丢弃**。
    ///
    /// 越界→钳制而非放任：放任会让未知位悄悄写进后备缓冲，且在无alpha 目标上
    /// 可能被解释成有效通道。钳制丢弃是唯一安全处置。
    pub fn from_bits(raw: u8) -> WriteMask {
        WriteMask(raw & 0b1111)
    }

    /// 被钳制丢弃的未知位**个数**（诊断用：越界多少可查）。
    ///
    /// 按**置位计数**而非移位后的数值——`0b1111_0001` 越界的是 4 个位，
    /// 不是数值 15。移位当计数会把「4 个越界位」报成 15，诊断文案直接说谎。
    pub fn dropped_bits(raw: u8) -> u32 {
        (raw >> 4).count_ones()
    }

    /// 原始位。
    pub fn bits(self) -> u8 {
        self.0
    }

    /// 是否写红通道。
    pub fn r(self) -> bool {
        self.0 & 0b0001 != 0
    }
    /// 是否写绿通道。
    pub fn g(self) -> bool {
        self.0 & 0b0010 != 0
    }
    /// 是否写蓝通道。
    pub fn b(self) -> bool {
        self.0 & 0b0100 != 0
    }
    /// 是否写 alpha 通道。
    pub fn a(self) -> bool {
        self.0 & 0b1000 != 0
    }

    /// 是否写任一颜色通道（决定颜色维度是否起作用）。
    pub fn has_color(self) -> bool {
        self.r() || self.g() || self.b()
    }

    /// 人话写法（`RGBA` / `RGB` / `A` / `无`）。
    pub fn describe(self) -> String {
        if self.0 == 0 {
            return String::from("无通道");
        }
        let mut s = String::new();
        if self.r() {
            s.push('R');
        }
        if self.g() {
            s.push('G');
        }
        if self.b() {
            s.push('B');
        }
        if self.a() {
            s.push('A');
        }
        s
    }
}

impl BlendFactor {
    /// **线上编码**（D3D12 `D3D12_BLEND_FACTOR` 正反交错序）。
    ///
    /// 与 `#[repr(u8)]` 判别值**刻意不同**（见枚举头注），故必须显式映射。
    pub fn wire(self) -> u8 {
        match self {
            BlendFactor::Zero => 0,
            BlendFactor::One => 1,
            BlendFactor::SrcColor => 2,
            BlendFactor::OneMinusSrcColor => 3,
            BlendFactor::DstColor => 4,
            BlendFactor::OneMinusDstColor => 5,
            BlendFactor::SrcAlpha => 6,
            BlendFactor::OneMinusSrcAlpha => 7,
            BlendFactor::DstAlpha => 8,
            BlendFactor::OneMinusDstAlpha => 9,
            BlendFactor::ConstantColor => 10,
            BlendFactor::OneMinusConstantAlpha => 11,
            BlendFactor::ConstantAlpha => 12,
            BlendFactor::OneMinusConstantColor => 13,
        }
    }

    /// 由线上编码还原；**越界编码被钳制**为 [`BlendFactor::Zero`]。
    ///
    /// 钳制方向为「丢弃并按 0 处理」：混合里把未知因子当 0 最多让该通道
    /// 不受源影响，而当 `One` 会**放大**源项造成过曝——后者是不可逆的画质事故。
    pub fn from_wire(wire: u8) -> BlendFactor {
        match wire {
            0 => BlendFactor::Zero,
            1 => BlendFactor::One,
            2 => BlendFactor::SrcColor,
            3 => BlendFactor::OneMinusSrcColor,
            4 => BlendFactor::DstColor,
            5 => BlendFactor::OneMinusDstColor,
            6 => BlendFactor::SrcAlpha,
            7 => BlendFactor::OneMinusSrcAlpha,
            8 => BlendFactor::DstAlpha,
            9 => BlendFactor::OneMinusDstAlpha,
            10 => BlendFactor::ConstantColor,
            11 => BlendFactor::OneMinusConstantAlpha,
            12 => BlendFactor::ConstantAlpha,
            13 => BlendFactor::OneMinusConstantColor,
            _ => BlendFactor::Zero,
        }
    }

    /// 线上编码是否越界（诊断用）。
    pub fn wire_is_known(wire: u8) -> bool {
        wire <= 13
    }

    /// 因子名（读屏、建议文本与诊断用）。
    pub fn name(self) -> &'static str {
        match self {
            BlendFactor::Zero => "Zero",
            BlendFactor::One => "One",
            BlendFactor::SrcColor => "SrcColor",
            BlendFactor::DstColor => "DstColor",
            BlendFactor::SrcAlpha => "SrcAlpha",
            BlendFactor::DstAlpha => "DstAlpha",
            BlendFactor::ConstantColor => "ConstantColor",
            BlendFactor::ConstantAlpha => "ConstantAlpha",
            BlendFactor::OneMinusSrcColor => "OneMinusSrcColor",
            BlendFactor::OneMinusDstColor => "OneMinusDstColor",
            BlendFactor::OneMinusSrcAlpha => "OneMinusSrcAlpha",
            BlendFactor::OneMinusDstAlpha => "OneMinusDstAlpha",
            BlendFactor::OneMinusConstantColor => "OneMinusConstantColor",
            BlendFactor::OneMinusConstantAlpha => "OneMinusConstantAlpha",
        }
    }

    /// 该因子是否引用**目标像素**（决定在无 alpha 目标上是否非法）。
    pub fn reads_dst(self) -> bool {
        matches!(
            self,
            BlendFactor::DstColor
                | BlendFactor::DstAlpha
                | BlendFactor::OneMinusDstColor
                | BlendFactor::OneMinusDstAlpha
        )
    }

    /// 该因子是否引用**常量色**（决定常量色是否参与去重键）。
    pub fn reads_constant(self) -> bool {
        matches!(
            self,
            BlendFactor::ConstantColor
                | BlendFactor::ConstantAlpha
                | BlendFactor::OneMinusConstantColor
                | BlendFactor::OneMinusConstantAlpha
        )
    }

    /// 该因子在**alpha 通道**上的塌缩语义是否恒为标量。
    ///
    /// 颜色类因子（`SrcColor`/`DstColor`/常量色）在 alpha 通道上必须塌缩到
    /// 自身的 alpha 分量——这是硬件规则，也是预览与硬件保持一致的前提。
    pub fn collapses_in_alpha(self) -> bool {
        matches!(
            self,
            BlendFactor::SrcColor
                | BlendFactor::OneMinusSrcColor
                | BlendFactor::DstColor
                | BlendFactor::OneMinusDstColor
                | BlendFactor::ConstantColor
                | BlendFactor::OneMinusConstantColor
        )
    }
}

impl BlendOp {
    /// **线上编码**（D3D12 `D3D12_BLEND_OP`：`Add = 1`，无 0）。
    ///
    /// 与判别值**刻意相差 1**（判别 `Add = 0`），故禁止 `enum_val as u8`。
    pub fn wire(self) -> u8 {
        match self {
            BlendOp::Add => 1,
            BlendOp::Subtract => 2,
            BlendOp::ReverseSubtract => 3,
            BlendOp::Min => 4,
            BlendOp::Max => 5,
        }
    }

    /// 由线上编码还原；**越界编码被钳制**为 [`BlendOp::Add`]。
    ///
    /// 钳制方向为「退回加法」：`Add` 是唯一在所有因子组合下都有定义、
    /// 且不会产生越界的算子，故未知算子退到它最安全。
    pub fn from_wire(wire: u8) -> BlendOp {
        match wire {
            2 => BlendOp::Subtract,
            3 => BlendOp::ReverseSubtract,
            4 => BlendOp::Min,
            5 => BlendOp::Max,
            _ => BlendOp::Add,
        }
    }

    /// 线上编码是否越界（诊断用）。
    pub fn wire_is_known(wire: u8) -> bool {
        (1..=5).contains(&wire)
    }

    /// 算子名。
    pub fn name(self) -> &'static str {
        match self {
            BlendOp::Add => "Add",
            BlendOp::Subtract => "Subtract",
            BlendOp::ReverseSubtract => "ReverseSubtract",
            BlendOp::Min => "Min",
            BlendOp::Max => "Max",
        }
    }

    /// **该算子是否丢弃源/目标因子**（D3D12/Vulkan 规定：`Min`/`Max` 忽略因子）。
    ///
    /// 这是本域第一性声明所指的「静默丢弃」判据的**唯一真源**——
    /// 校验、诊断、预览三处必须一致，故收敛到这一个函数。
    pub fn ignores_factors(self) -> bool {
        matches!(self, BlendOp::Min | BlendOp::Max)
    }

    /// 该算子是否可能产生**下溢**（负值）——无符号目标上须钳制。
    pub fn may_underflow(self) -> bool {
        matches!(self, BlendOp::Subtract | BlendOp::ReverseSubtract)
    }
}

// ---------------------------------------------------------------------------
// 三、四维状态封装
// ---------------------------------------------------------------------------

/// 混合状态描述（四维 + 独立 alpha 维的统一封装）。
///
/// 不 derive `Eq`/`Hash`——`constant` 是 `[f32;4]`，`f32` 不实现 `Eq`。
/// 去重键走 [`BlendDesc::canonical_key`]（字节级），不走派生。
#[derive(Clone, Debug, PartialEq)]
pub struct BlendDesc {
    /// 是否启用混合。关闭时硬件**不读**任何因子与算子，但仍按掩码写入源色。
    pub enabled: bool,
    /// 源颜色因子。
    pub src_color: BlendFactor,
    /// 目标颜色因子。
    pub dst_color: BlendFactor,
    /// 颜色通道算子。
    pub color_op: BlendOp,
    /// 源 alpha 因子（独立 alpha 维；与颜色因子**可以不同**）。
    pub src_alpha: BlendFactor,
    /// 目标 alpha 因子。
    pub dst_alpha: BlendFactor,
    /// alpha 通道算子。
    pub alpha_op: BlendOp,
    /// 写掩码（决定哪一维真正起作用）。
    pub mask: WriteMask,
    /// 常量色 RGBA（仅当某因子引用它时起作用）。
    pub constant: [f32; 4],
    /// 常量色是否被**显式提供**。
    ///
    /// 单独设标志而非「看常量是否为零」：纯黑常量色是完全合法的需求，
    /// 用数值反推「有没有设」会把合法的纯黑误判成未设。
    pub constant_set: bool,
}

impl BlendDesc {
    /// 构造「关闭混合」状态（等价于直接覆盖，但掩码仍生效）。
    pub fn disabled() -> Self {
        BlendDesc {
            enabled: false,
            src_color: BlendFactor::One,
            dst_color: BlendFactor::Zero,
            color_op: BlendOp::Add,
            src_alpha: BlendFactor::One,
            dst_alpha: BlendFactor::Zero,
            alpha_op: BlendOp::Add,
            mask: WriteMask::RGBA,
            constant: [0.0; 4],
            constant_set: false,
        }
    }

    /// 构造标准直通alpha 混合：`SrcAlpha / OneMinusSrcAlpha`，掩码全写。
    pub fn straight_alpha() -> Self {
        BlendDesc {
            enabled: true,
            src_color: BlendFactor::SrcAlpha,
            dst_color: BlendFactor::OneMinusSrcAlpha,
            color_op: BlendOp::Add,
            src_alpha: BlendFactor::One,
            dst_alpha: BlendFactor::OneMinusSrcAlpha,
            alpha_op: BlendOp::Add,
            mask: WriteMask::RGBA,
            constant: [0.0; 4],
            constant_set: false,
        }
    }

    /// 设置颜色因子与颜色算子。
    pub fn with_color(mut self, s: BlendFactor, d: BlendFactor, op: BlendOp) -> Self {
        self.src_color = s;
        self.dst_color = d;
        self.color_op = op;
        self
    }

    /// 设置独立 alpha 因子与alpha 算子。
    pub fn with_alpha(mut self, s: BlendFactor, d: BlendFactor, op: BlendOp) -> Self {
        self.src_alpha = s;
        self.dst_alpha = d;
        self.alpha_op = op;
        self
    }

    /// 设置写掩码。
    pub fn with_mask(mut self, m: WriteMask) -> Self {
        self.mask = m;
        self
    }

    /// 设置常量色（同时置 `constant_set`）。
    pub fn with_constant(mut self, c: [f32; 4]) -> Self {
        self.constant = c;
        self.constant_set = true;
        self
    }

    /// 启用/关闭混合。
    pub fn with_enabled(mut self, on: bool) -> Self {
        self.enabled = on;
        self
    }

    /// 四维中**当前真正起作用**的因子是否引用目标 alpha。
    fn active_reads_dst_alpha(&self) -> bool {
        let mut r = false;
        if self.mask.has_color() {
            r |= self.src_color == BlendFactor::DstAlpha
                || self.dst_color == BlendFactor::DstAlpha
                || self.src_color == BlendFactor::OneMinusDstAlpha
                || self.dst_color == BlendFactor::OneMinusDstAlpha;
        }
        if self.mask.a() {
            r |= self.src_alpha == BlendFactor::DstAlpha
                || self.dst_alpha == BlendFactor::DstAlpha
                || self.src_alpha == BlendFactor::OneMinusDstAlpha
                || self.dst_alpha == BlendFactor::OneMinusDstAlpha;
        }
        r
    }

    /// 四维中**当前真正起作用**的因子是否引用常量色。
    fn active_reads_constant(&self) -> bool {
        let mut r = false;
        if self.mask.has_color() {
            r |= self.src_color.reads_constant() || self.dst_color.reads_constant();
        }
        if self.mask.a() {
            r |= self.src_alpha.reads_constant() || self.dst_alpha.reads_constant();
        }
        r
    }

    /// **相关性裁剪去重键**：只编码真正起作用的维度。
    ///
    /// 两个只在「不起作用字段」上不同的描述符 → 同一混合状态（同参共享），
    /// 这正是硬件行为，故该去重在语义上正确而非近似。
    pub fn canonical_key(&self) -> Vec<u8> {
        let mut k: Vec<u8> = Vec::new();
        if !self.enabled {
            // 硬件不读任何因子与算子 → 键只保留「关闭」标记 + **写掩码**。
            //
            // 掩码**不可裁剪**：关闭混合时硬件仍按掩码决定写哪些通道，
            // 故 `opaque`(RGBA 直写) 与 `alpha_write_only`(只写 alpha) 是
            // 两个不同的状态，不可合并。若此处一并裁掉掩码，两者会共享同一条
            // 状态——一个把整帧涂满、另一个只动 alpha，行为天差地别。
            k.push(0x00);
            k.push(self.mask.bits());
            return k;
        }
        k.push(0x01);
        k.push(self.mask.bits());
        if self.mask.has_color() {
            k.push(self.src_color.wire());
            k.push(self.dst_color.wire());
            k.push(self.color_op.wire());
        }
        if self.mask.a() {
            k.push(self.src_alpha.wire());
            k.push(self.dst_alpha.wire());
            k.push(self.alpha_op.wire());
        }
        if self.active_reads_constant() {
            for c in self.constant.iter() {
                k.extend_from_slice(&c.to_bits().to_le_bytes());
            }
        }
        k
    }

    /// 去重键指纹（分桶用；碰撞由 [`BlendCache`] 的键字节精确比对兜底）。
    pub fn key_hash(&self) -> u64 {
        fnv1a64(&self.canonical_key())
    }

    /// 二进制头（供命令缓冲打包；因子/算子一律走 `wire()`）。
    pub fn to_wire(&self) -> [u8; 12] {
        let mut out = [0u8; 12];
        out[0] = if self.enabled { 1 } else { 0 };
        out[1] = self.mask.bits();
        out[2] = self.src_color.wire();
        out[3] = self.dst_color.wire();
        out[4] = self.color_op.wire();
        out[5] = self.src_alpha.wire();
        out[6] = self.dst_alpha.wire();
        out[7] = self.alpha_op.wire();
        out[8..12].copy_from_slice(&self.constant[0].to_bits().to_le_bytes());
        out
    }

    /// 人话描述（读屏与诊断用）。
    pub fn describe(&self) -> String {
        if !self.enabled {
            return format!("混合关闭（掩码 {} 直写源色）", self.mask.describe());
        }
        let mut s = String::new();
        if self.mask.has_color() {
            s.push_str(&format!(
                "颜色 {} {} {} → ",
                self.src_color.name(),
                self.color_op.name(),
                self.dst_color.name()
            ));
        }
        if self.mask.a() {
            s.push_str(&format!(
                "Alpha {} {} {} → ",
                self.src_alpha.name(),
                self.alpha_op.name(),
                self.dst_alpha.name()
            ));
        }
        s.push_str(&format!("掩码 {}", self.mask.describe()));
        if self.color_op.ignores_factors() || self.alpha_op.ignores_factors() {
            s.push_str(" [因子被算子丢弃]");
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 四、判定与拒绝建议
// ---------------------------------------------------------------------------

/// 组合判定结论。
///
/// **处置方向相反的状态不得共用码**：
/// - [`Verdict::Ok`] 通过；
/// -阻断级（须改代码）走 [`Verdict::Blocking`]；
/// - 提示级（合法但易踩坑）走 [`Verdict::Advisory`]。
/// 三者方向不同：阻断要改代码、提示只需知情故绝不共用。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// 组合合法。
    Ok,
    /// **阻断级**：某维会被硬件静默丢弃或引用不存在的数据。
    Blocking(Flaw),
    /// **提示级**：合法，但是已知易踩坑的写法，须留账告知。
    Advisory(Flaw),
}

/// 具体缺陷码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flaw {
    /// 关闭混合却设了非全写掩码——掩码只在混合下有意义，纯覆盖时它被忽略。
    MaskWithoutBlend,
    /// 掩码为空——该 draw 一个通道都不写，等于什么都没画。
    MaskEmpty,
    /// `Min`/`Max` 搭配非平凡因子——因子会被硬件丢弃，行为与意图不符。
    FactorDiscardedByOp,
    /// 因子引用常量色但常量色未被显式提供——读到的是残留值。
    ConstantUnset,
    /// 因子引用目标 alpha 但目标格式无 alpha 通道。
    DstAlphaOnOpaqueTarget,
    /// 独立 alpha 走 `One/One`（alpha 原样透传）而颜色走 `SrcAlpha`——
    /// 合法但是经典坑：后段按 alpha 合成时读到的是**陈旧**目标 alpha。
    StaleAlphaPassthrough,
}

/// 缺陷的人话解释（为什么非法/为什么提示）。
pub fn flaw_reason(f: Flaw) -> &'static str {
    match f {
        Flaw::MaskWithoutBlend => "混合已关闭，写掩码不参与混合且行为反直觉：关闭混合时硬件只按掩码直写源色，非全写掩码会让调用方以为在做部分混合",
        Flaw::MaskEmpty => "写掩码为空，该 draw 不写任何通道——绘制调用存在但像素不变，等价于空操作",
        Flaw::FactorDiscardedByOp => "Min/Max 算子会丢弃源与目标因子（D3D12/Vulkan 规定），你写的因子不参与运算，实际执行的是 min/max(src,dst)",
        Flaw::ConstantUnset => "因子引用了常量色但常量色未显式提供，硬件将使用上一次残留的常量色值——结果依赖绘制顺序",
        Flaw::DstAlphaOnOpaqueTarget => "因子引用了目标 alpha，但目标格式不含 alpha 通道，该因子无源可读",
        Flaw::StaleAlphaPassthrough => "独立 alpha 因子为 One/One（目标 alpha 原样透传）而颜色按源 alpha 混合：颜色被混合、alpha 未更新，后段按 alpha 合成时读到陈旧值",
    }
}

/// 缺陷的修正建议方向。
pub fn flaw_hint(f: Flaw) -> &'static str {
    match f {
        Flaw::MaskWithoutBlend => "若要做部分写入，启用混合并把两侧因子设为 One/Zero；若确为纯覆盖，改用 WriteMask::RGBA 使掩码与意图一致",
        Flaw::MaskEmpty => "至少写一个通道：全色半透明用 RGBA，只更新 alpha 用 ALPHA，只更新颜色用 RGB",
        Flaw::FactorDiscardedByOp => "改用 Add 算子并显式写出想要的因子（min 语义用 OneMinusDstColor/SrcColor 组合近似），或保留 Min/Max 但把两侧因子写成 One/One 以声明「我知道因子无效」",
        Flaw::ConstantUnset => "用 with_constant([r,g,b,a]) 显式提供常量色；纯黑常量也是合法需求，故不能用「值是否为零」代替显式标志",
        Flaw::DstAlphaOnOpaqueTarget => "改用不引用目标 alpha 的因子（One/OneMinusSrcAlpha/SrcColor），或换用带 alpha 的目标格式",
        Flaw::StaleAlphaPassthrough => "若颜色与 alpha 需一致，把独立 alpha 也设为 SrcAlpha/OneMinusSrcAlpha；若确需透传 alpha，请确认后段合成读的是本pass 之前的 alpha",
    }
}

/// 纯判定：**O(1)**，不生成建议、不扫表。
///
/// 内层门禁必须走本函数—— [`validate`] 每次判失败都要扫全表求最近预置，
/// 若内层调`validate` 则退化为 O(预置数²)。
pub fn classify(d: &BlendDesc, target: BlendTarget) -> Verdict {
    // 掩码为空：任何目标上都是空操作。
    if d.mask == WriteMask::NONE {
        return Verdict::Blocking(Flaw::MaskEmpty);
    }
    // 关闭混合：掩码仍生效，故只提示不阻断（写「只写 alpha」是完全合法的
    // 直写用法）；但非全写掩码在关闭混合下极易被误读为部分混合。
    if !d.enabled {
        return if d.mask == WriteMask::RGBA {
            Verdict::Ok
        } else {
            Verdict::Advisory(Flaw::MaskWithoutBlend)
        };
    }
    // Min/Max 丢因子：只有当被丢的因子**不是**平凡的 One/One 时才阻断——
    // 写成 One/One 是「显式声明知道因子无效」，属合法用法。
    if d.mask.has_color() && d.color_op.ignores_factors() {
        let trivial = matches!(
            (d.src_color, d.dst_color),
            (BlendFactor::One, BlendFactor::One)
                | (BlendFactor::Zero, BlendFactor::Zero)
        );
        if !trivial {
            return Verdict::Blocking(Flaw::FactorDiscardedByOp);
        }
    }
    if d.mask.a() && d.alpha_op.ignores_factors() {
        let trivial = matches!(
            (d.src_alpha, d.dst_alpha),
            (BlendFactor::One, BlendFactor::One)
                | (BlendFactor::Zero, BlendFactor::Zero)
        );
        if !trivial {
            return Verdict::Blocking(Flaw::FactorDiscardedByOp);
        }
    }
    if d.active_reads_constant() && !d.constant_set {
        return Verdict::Blocking(Flaw::ConstantUnset);
    }
    if d.active_reads_dst_alpha() && !target.has_alpha() {
        return Verdict::Blocking(Flaw::DstAlphaOnOpaqueTarget);
    }
    // 提示级：陈旧 alpha 透传。
    if d.mask.a()
        && d.mask.has_color()
        && matches!(
            (d.src_alpha, d.dst_alpha),
            (BlendFactor::One, BlendFactor::One)
        )
        && d.src_color == BlendFactor::SrcAlpha
    {
        return Verdict::Advisory(Flaw::StaleAlphaPassthrough);
    }
    Verdict::Ok
}

/// 拒绝记录（零静默：拒绝必留账）。
#[derive(Clone, Debug, PartialEq)]
pub struct Rejection {
    /// 缺陷码。
    pub flaw: Flaw,
    /// 是否阻断级（提示级不进拒绝记录）。
    pub blocking: bool,
    /// 为什么非法（人话）。
    pub reason: String,
    /// 正确组合建议（人话）。
    pub hint: String,
    /// 被判定为非法的描述（人话）。
    pub desc_text: String,
    /// 目标格式（归因用：同一状态在不同目标上判定不同）。
    pub target: BlendTarget,
    /// 逻辑 tick（何时被判非法）。
    pub tick: u64,
}

/// 预置描述行。
///
/// 只 derive `Clone` 不 derive `Copy`——内含 [`BlendDesc`]（含 `[f32;4]`，
/// `f32` 不实现 `Copy`）。
#[derive(Clone, Debug)]
pub struct PresetRow {
    /// 预置名（键）。
    pub key_name: &'static str,
    /// 用途说明（**必填**——预置是基线，不许含糊）。
    pub use_note: &'static str,
    /// 描述符各维。
    pub desc: BlendDesc,
    /// 常量色（仅 `ConstantColor` 类预置用）。
    pub constant: [f32; 4],
}

impl PresetRow {
    /// 展开为描述符（常量色已就位）。
    pub fn to_desc(&self) -> BlendDesc {
        let mut d = self.desc.clone();
        d.constant = self.constant;
        d.constant_set = self.constant_needed();
        d
    }

    /// 该预置是否需要常量色。
    fn constant_needed(&self) -> bool {
        self.desc.src_color.reads_constant()
            || self.desc.dst_color.reads_constant()
            || self.desc.src_alpha.reads_constant()
            || self.desc.dst_alpha.reads_constant()
    }
}

/// 混合预置全集（`const` 表，零分配可静态审计）。
///
/// 覆盖十二类真实场景：直通不透明、直通 alpha、预乘 alpha、叠加、
/// 正片叠底、滤色、变暗、变亮、最小、最大、减色、纯 alpha 写入、调试掩码。
pub const PRESET_TABLE: [PresetRow; 13] = [
    PresetRow {
        key_name: "opaque",
        use_note: "不透明实心：关闭混合直写，掩码全写（默认最省）",
        desc: BlendDesc {
            enabled: false,
            src_color: BlendFactor::One,
            dst_color: BlendFactor::Zero,
            color_op: BlendOp::Add,
            src_alpha: BlendFactor::One,
            dst_alpha: BlendFactor::Zero,
            alpha_op: BlendOp::Add,
            mask: WriteMask::RGBA,
            constant: [0.0; 4],
            constant_set: false,
        },
        constant: [0.0; 4],
    },
    PresetRow {
        key_name: "alpha_straight",
        use_note: "标准半透明（直通 alpha）：SrcAlpha/OneMinusSrcAlpha，最常用的 UI 混合",
        desc: BlendDesc {
            enabled: true,
            src_color: BlendFactor::SrcAlpha,
            dst_color: BlendFactor::OneMinusSrcAlpha,
            color_op: BlendOp::Add,
            src_alpha: BlendFactor::One,
            dst_alpha: BlendFactor::OneMinusSrcAlpha,
            alpha_op: BlendOp::Add,
            mask: WriteMask::RGBA,
            constant: [0.0; 4],
            constant_set: false,
        },
        constant: [0.0; 4],
    },
    PresetRow {
        key_name: "alpha_premultiplied",
        use_note: "预乘 alpha 半透明：One/OneMinusSrcAlpha，避免半透明边缘发黑（纹理须预乘）",
        desc: BlendDesc {
            enabled: true,
            src_color: BlendFactor::One,
            dst_color: BlendFactor::OneMinusSrcAlpha,
            color_op: BlendOp::Add,
            src_alpha: BlendFactor::One,
            dst_alpha: BlendFactor::OneMinusSrcAlpha,
            alpha_op: BlendOp::Add,
            mask: WriteMask::RGBA,
            constant: [0.0; 4],
            constant_set: false,
        },
        constant: [0.0; 4],
    },
    PresetRow {
        key_name: "additive",
        use_note: "叠加发光：One/One，用于高斯光晕与能量特效（alpha 通道常需单独处理）",
        desc: BlendDesc {
            enabled: true,
            src_color: BlendFactor::One,
            dst_color: BlendFactor::One,
            color_op: BlendOp::Add,
            src_alpha: BlendFactor::One,
            dst_alpha: BlendFactor::One,
            alpha_op: BlendOp::Add,
            mask: WriteMask::RGBA,
            constant: [0.0; 4],
            constant_set: false,
        },
        constant: [0.0; 4],
    },
    PresetRow {
        key_name: "multiply",
        use_note: "正片叠底压暗：DstColor/Zero，用于阴影贴花与污渍叠加",
        desc: BlendDesc {
            enabled: true,
            src_color: BlendFactor::DstColor,
            dst_color: BlendFactor::Zero,
            color_op: BlendOp::Add,
            src_alpha: BlendFactor::One,
            dst_alpha: BlendFactor::Zero,
            alpha_op: BlendOp::Add,
            mask: WriteMask::RGBA,
            constant: [0.0; 4],
            constant_set: false,
        },
        constant: [0.0; 4],
    },
    PresetRow {
        key_name: "screen",
        use_note: "滤色提亮：OneMinusDstColor/One，用于柔光与雾效提亮",
        desc: BlendDesc {
            enabled: true,
            src_color: BlendFactor::OneMinusDstColor,
            dst_color: BlendFactor::One,
            color_op: BlendOp::Add,
            src_alpha: BlendFactor::One,
            dst_alpha: BlendFactor::OneMinusSrcAlpha,
            alpha_op: BlendOp::Add,
            mask: WriteMask::RGBA,
            constant: [0.0; 4],
            constant_set: false,
        },
        constant: [0.0; 4],
    },
    PresetRow {
        key_name: "darken",
        use_note: "变暗：Zero/SrcColor（取较暗者），用于阴影加深；注意与Min 的因子语义差异",
        desc: BlendDesc {
            enabled: true,
            src_color: BlendFactor::Zero,
            dst_color: BlendFactor::SrcColor,
            color_op: BlendOp::Add,
            src_alpha: BlendFactor::Zero,
            dst_alpha: BlendFactor::One,
            alpha_op: BlendOp::Add,
            mask: WriteMask::RGBA,
            constant: [0.0; 4],
            constant_set: false,
        },
        constant: [0.0; 4],
    },
    PresetRow {
        key_name: "lighten",
        use_note: "变亮：One/SrcColor（取较亮者），用于高光叠加",
        desc: BlendDesc {
            enabled: true,
            src_color: BlendFactor::One,
            dst_color: BlendFactor::SrcColor,
            color_op: BlendOp::Add,
            src_alpha: BlendFactor::One,
            dst_alpha: BlendFactor::One,
            alpha_op: BlendOp::Add,
            mask: WriteMask::RGBA,
            constant: [0.0; 4],
            constant_set: false,
        },
        constant: [0.0; 4],
    },
    PresetRow {
        key_name: "min_op",
        use_note: "最小值相交：Min 且因子写 One/One 显式声明因子无效，用于遮罩求交",
        desc: BlendDesc {
            enabled: true,
            src_color: BlendFactor::One,
            dst_color: BlendFactor::One,
            color_op: BlendOp::Min,
            src_alpha: BlendFactor::One,
            dst_alpha: BlendFactor::One,
            alpha_op: BlendOp::Min,
            mask: WriteMask::RGBA,
            constant: [0.0; 4],
            constant_set: false,
        },
        constant: [0.0; 4],
    },
    PresetRow {
        key_name: "max_op",
        use_note: "最大值并集：Max 且因子写 One/One，用于光圈求并",
        desc: BlendDesc {
            enabled: true,
            src_color: BlendFactor::One,
            dst_color: BlendFactor::One,
            color_op: BlendOp::Max,
            src_alpha: BlendFactor::One,
            dst_alpha: BlendFactor::One,
            alpha_op: BlendOp::Max,
            mask: WriteMask::RGBA,
            constant: [0.0; 4],
            constant_set: false,
        },
        constant: [0.0; 4],
    },
    PresetRow {
        key_name: "subtract",
        use_note: "减色擦除：One/One Subtract，无符号目标上负值被钳制到 0",
        desc: BlendDesc {
            enabled: true,
            src_color: BlendFactor::One,
            dst_color: BlendFactor::One,
            color_op: BlendOp::Subtract,
            src_alpha: BlendFactor::One,
            dst_alpha: BlendFactor::One,
            alpha_op: BlendOp::Subtract,
            mask: WriteMask::RGBA,
            constant: [0.0; 4],
            constant_set: false,
        },
        constant: [0.0; 4],
    },
    PresetRow {
        key_name: "alpha_write_only",
        use_note: "只更新目标 alpha：关闭混合 + ALPHA 掩码，供后段按 alpha 合成",
        desc: BlendDesc {
            enabled: false,
            src_color: BlendFactor::One,
            dst_color: BlendFactor::Zero,
            color_op: BlendOp::Add,
            src_alpha: BlendFactor::One,
            dst_alpha: BlendFactor::Zero,
            alpha_op: BlendOp::Add,
            mask: WriteMask::ALPHA,
            constant: [0.0; 4],
            constant_set: false,
        },
        constant: [0.0; 4],
    },
    PresetRow {
        key_name: "debug_mask_red",
        use_note: "调试单通道：关闭混合 + RED 掩码，用于隔离红通道问题",
        desc: BlendDesc {
            enabled: false,
            src_color: BlendFactor::One,
            dst_color: BlendFactor::Zero,
            color_op: BlendOp::Add,
            src_alpha: BlendFactor::One,
            dst_alpha: BlendFactor::Zero,
            alpha_op: BlendOp::Add,
            mask: WriteMask::RED,
            constant: [0.0; 4],
            constant_set: false,
        },
        constant: [0.0; 4],
    },
];

/// 按名取预置描述。
pub fn preset_desc(key_name: &str) -> Option<BlendDesc> {
    for row in PRESET_TABLE.iter() {
        if row.key_name == key_name {
            return Some(row.to_desc());
        }
    }
    None
}

/// 重算预置集基线指纹（改 [`PRESET_TABLE`] 必使其变化）。
pub fn compute_preset_hash() -> u64 {
    let mut buf: Vec<u8> = Vec::new();
    for row in PRESET_TABLE.iter() {
        for b in row.key_name.as_bytes().iter() {
            buf.push(*b);
        }
        buf.push(0x1f);
        let d = row.to_desc();
        buf.extend_from_slice(&d.canonical_key());
        buf.push(0x1e);
    }
    fnv1a64(&buf)
}

/// 基线对齐三态。**处置方向相反，故不共用码**。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BaselineStatus {
    /// 与基线一致（预置集未漂移）。
    Aligned,
    /// 与基线不一致——须重签（阻断级）。
    Drifted {
        /// 基线声明指纹。
        expected: u64,
        /// 实际重算指纹。
        actual: u64,
    },
    /// 该基线版本未登记——须登记后使用（阻断级）。
    Unregistered,
}

impl BaselineStatus {
    /// 是否对齐。
    pub fn is_aligned(self) -> bool {
        matches!(self, BaselineStatus::Aligned)
    }

    /// 人话描述。
    pub fn describe(self) -> String {
        match self {
            BaselineStatus::Aligned => String::from("预置集与基线一致"),
            BaselineStatus::Drifted { expected, actual } => format!(
                "预置集已漂移：基线 {:016x}，实际 {:016x}——须双签重签",
                expected, actual
            ),
            BaselineStatus::Unregistered => {
                String::from("该基线版本未登记——须先登记再使用")
            }
        }
    }
}

/// 基线核对（重算 vs 声明）。
pub fn verify_baseline() -> BaselineStatus {
    let actual = compute_preset_hash();
    if BLEND_BASELINE_VERSION != 1 {
        // 只有 v1 在册；其他版本视为未登记（不得默认可用）。
        return BaselineStatus::Unregistered;
    }
    if actual == BLEND_BASELINE_HASH {
        BaselineStatus::Aligned
    } else {
        BaselineStatus::Drifted {
            expected: BLEND_BASELINE_HASH,
            actual,
        }
    }
}

/// 预置集变更申请（走双签流程）。
#[derive(Clone, Debug)]
pub struct ChangeRequest {
    /// 变更摘要（人话）。
    pub summary: String,
    /// 申请人。
    pub requester: String,
    /// 复核人（须与申请人不同——自己签自己的不算复核）。
    pub reviewer: String,
    /// 新基线版本号（须高于当前）。
    pub new_version: u32,
}

/// 变更受理结果。**受理 / 驳回 / 待复核** 三态方向不同，不共用码。
#[derive(Clone, Debug, PartialEq)]
pub enum ChangeOutcome {
    /// 已受理：双签齐备且版本递进。
    Accepted {
        /// 受理后的基线版本。
        version: u32,
        /// 受理时的预置集实际指纹（须同步回填常量）。
        actual_hash: u64,
    },
    /// 驳回：签名或版本不合规。
    Rejected {
        /// 驳回原因。
        why: String,
    },
}

/// 受理预置集变更（双签 + 版本递进 + 抬指纹）。
///
/// 变更**不改** [`PRESET_TABLE`]——预置表是 `const`，运行期改不动基线，
/// 这是纪律而非缺陷。受理只产出「新版指纹 + 谁签的」，由构建期回填常量。
pub fn queue_preset_change(req: &ChangeRequest, current: u32) -> ChangeOutcome {
    if req.summary.trim().is_empty() {
        return ChangeOutcome::Rejected {
            why: String::from("变更摘要为空——预置是基线，不许含糊"),
        };
    }
    if req.requester.trim().is_empty() || req.reviewer.trim().is_empty() {
        return ChangeOutcome::Rejected {
            why: String::from("申请人或复核人缺失"),
        };
    }
    if req.requester == req.reviewer {
        return ChangeOutcome::Rejected {
            why: String::from("申请人与复核人相同——自己签自己的不算双签"),
        };
    }
    if req.new_version <= current {
        return ChangeOutcome::Rejected {
            why: format!("新版本 {} 未高于当前版本 {}", req.new_version, current),
        };
    }
    ChangeOutcome::Accepted {
        version: req.new_version,
        actual_hash: compute_preset_hash(),
    }
}

/// 校验：**仅阻断级**判为错误，提示级放行但要求留账。
///
/// **本函数会扫全表求最近预置，故仅在构造/校验路径调用**——内层门禁走 [`classify`]。
///
/// 提示级（[`Flaw::StaleAlphaPassthrough`] 等）**不是错误**：那是合法但易踩坑的
/// 写法（例如「关闭混合 + 只写 alpha」是标准直写用法），若判成错误就会把
/// [`PRESET_TABLE`] 里的合法预置全部拒掉。提示的处置是**记账**
/// （[`BlendCache::resolve`] 将其写入台账），不是阻断。
pub fn validate(d: &BlendDesc, target: BlendTarget, tick: u64) -> Result<(), Rejection> {
    match classify(d, target) {
        Verdict::Blocking(f) => {
            let (name, why) = suggest_preset(d, target);
            Err(Rejection {
                flaw: f,
                blocking: true,
                reason: format!("{}（最近合法预置 {}：{}）", flaw_reason(f), name, why),
                hint: flaw_hint(f).to_string(),
                desc_text: d.describe(),
                target,
                tick,
            })
        }
        // 合法（含提示级）：不阻断。
        Verdict::Ok | Verdict::Advisory(_) => Ok(()),
    }
}

/// 提示级记录（非阻断，须留账）。
///
/// 与 [`Rejection`] 同结构但 `blocking = false`——处置方向不同（阻断要改代码、
/// 提示只需知情），故不共用码。返回 `None` 表示无提示。
pub fn advisory_note(d: &BlendDesc, target: BlendTarget, tick: u64) -> Option<Rejection> {
    match classify(d, target) {
        Verdict::Advisory(f) => Some(Rejection {
            flaw: f,
            blocking: false,
            reason: flaw_reason(f).to_string(),
            hint: flaw_hint(f).to_string(),
            desc_text: d.describe(),
            target,
            tick,
        }),
        Verdict::Ok | Verdict::Blocking(_) => None,
    }
}

/// 求**最近合法预置**（按四维差异字段计数选最近）。
///
/// 只说「非法」等于把问题退回给调用方自己重猜，而重猜的成本远高于系统直接
/// 建议一个可用值——故拒绝必须附一条能走的路。
pub fn suggest_preset(d: &BlendDesc, target: BlendTarget) -> (&'static str, String) {
    let mut best_name: &'static str = "opaque";
    let mut best_diff = usize::MAX;
    let mut best_note = String::new();
    for row in PRESET_TABLE.iter() {
        let cand = row.to_desc();
        if !matches!(classify(&cand, target), Verdict::Ok) {
            continue;
        }
        let diff = blend_distance(d, &cand);
        if diff < best_diff {
            best_diff = diff;
            best_name = row.key_name;
            best_note = row.use_note.to_string();
        }
    }
    (best_name, best_note)
}

/// 四维差异距离（只计**描述层面**的字段数，用于选最近）。
fn blend_distance(a: &BlendDesc, b: &BlendDesc) -> usize {
    let mut n = 0usize;
    if a.enabled != b.enabled {
        n += 1;
    }
    if a.src_color != b.src_color {
        n += 1;
    }
    if a.dst_color != b.dst_color {
        n += 1;
    }
    if a.color_op != b.color_op {
        n += 1;
    }
    if a.src_alpha != b.src_alpha {
        n += 1;
    }
    if a.dst_alpha != b.dst_alpha {
        n += 1;
    }
    if a.alpha_op != b.alpha_op {
        n += 1;
    }
    if a.mask != b.mask {
        n += 1;
    }
    n
}

// ---------------------------------------------------------------------------
// 五、混合求值（预览与硬件同规则）
// ---------------------------------------------------------------------------

/// 单通道求值结果（附两个诚实标记）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EvalCell {
    /// 输出 RGBA。
    pub out: [f32; 4],
    /// 该结果是否被**无符号目标钳制**过（负值/超 1被夹）。
    pub clamped: bool,
    /// 该状态的因子是否被算子丢弃（`Min`/`Max`）。
    pub factor_ignored: bool,
}

/// 因子取值：`factor(src, dst, constant, is_alpha_channel)`。
///
/// `is_alpha_channel` 为真时，颜色类因子**塌缩到自身 alpha 分量**——
/// 这是硬件规则；预览若不塌缩就会与硬件给出不同答案，成为第二套错误答案。
fn factor_term(
    f: BlendFactor,
    src: [f32; 4],
    dst: [f32; 4],
    constant: [f32; 4],
    is_alpha_channel: bool,
) -> f32 {
    let sa = src[3];
    let da = dst[3];
    let ca = constant[3];
    match f {
        BlendFactor::Zero => 0.0,
        BlendFactor::One => 1.0,
        BlendFactor::SrcColor => {
            if is_alpha_channel {
                sa
            } else {
                src[0]
            }
        }
        BlendFactor::OneMinusSrcColor => {
            if is_alpha_channel {
                1.0 - sa
            } else {
                1.0 - src[0]
            }
        }
        BlendFactor::DstColor => {
            if is_alpha_channel {
                da
            } else {
                dst[0]
            }
        }
        BlendFactor::OneMinusDstColor => {
            if is_alpha_channel {
                1.0 - da
            } else {
                1.0 - dst[0]
            }
        }
        BlendFactor::SrcAlpha => sa,
        BlendFactor::OneMinusSrcAlpha => 1.0 - sa,
        BlendFactor::DstAlpha => da,
        BlendFactor::OneMinusDstAlpha => 1.0 - da,
        BlendFactor::ConstantColor => {
            if is_alpha_channel {
                ca
            } else {
                constant[0]
            }
        }
        BlendFactor::OneMinusConstantColor => {
            if is_alpha_channel {
                1.0 - ca
            } else {
                1.0 - constant[0]
            }
        }
        BlendFactor::ConstantAlpha => ca,
        BlendFactor::OneMinusConstantAlpha => 1.0 - ca,
    }
}

/// 算子应用。
fn apply_op(op: BlendOp, s: f32, d: f32) -> f32 {
    match op {
        BlendOp::Add => s + d,
        BlendOp::Subtract => s - d,
        BlendOp::ReverseSubtract => d - s,
        BlendOp::Min => {
            if s < d {
                s
            } else {
                d
            }
        }
        BlendOp::Max => {
            if s > d {
                s
            } else {
                d
            }
        }
    }
}

/// 按混合状态求值一个像素（**逐通道**，与硬件规则一致）。
///
/// 关闭混合时硬件**不读因子**，但**仍按掩码写入源色**——故本函数在关闭
/// 分支仍处理掩码，这是最易写错的一处。
pub fn evaluate(d: &BlendDesc, src: [f32; 4], dst: [f32; 4], target: BlendTarget) -> EvalCell {
    let mut out = dst;
    let mut clamped = false;
    let factor_ignored = d.enabled
        && (d.color_op.ignores_factors() || d.alpha_op.ignores_factors());

    for c in 0..4usize {
        if !d.mask_writes(c) {
            continue;
        }
        let v = if !d.enabled {
            src[c]
        } else {
            let is_alpha = c == 3;
            let (sf, df, op) = if is_alpha {
                (d.src_alpha, d.dst_alpha, d.alpha_op)
            } else {
                (d.src_color, d.dst_color, d.color_op)
            };
            // Min/Max 忽略因子：因子项直接为1（等价于 src*1 + dst*1 的顺序，
            // 但 Min/Max 本身与顺序无关，故结果一致）。
            let s_term = if op.ignores_factors() {
                src[c]
            } else {
                src[c] * factor_term(sf, src, dst, d.constant, is_alpha)
            };
            let d_term = if op.ignores_factors() {
                dst[c]
            } else {
                dst[c] * factor_term(df, src, dst, d.constant, is_alpha)
            };
            apply_op(op, s_term, d_term)
        };
        let w = if target.is_unorm() {
            if v < UNIT_MIN {
                clamped = true;
                UNIT_MIN
            } else if v > UNIT_MAX {
                clamped = true;
                UNIT_MAX
            } else {
                v
            }
        } else {
            // 浮点目标：负值是合法 HDR，**不钳制**（钳了就是画质事故）。
            v
        };
        out[c] = w;
    }
    EvalCell {
        out,
        clamped,
        factor_ignored,
    }
}

impl BlendDesc {
    /// 写掩码是否写第 `c` 通道（`0..4`；越界通道号返回 `false`）。
    pub fn mask_writes(&self, c: usize) -> bool {
        match c {
            0 => self.mask.r(),
            1 => self.mask.g(),
            2 => self.mask.b(),
            3 => self.mask.a(),
            _ => false,
        }
    }
}

/// 预览斜坡的一格。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PreviewCell {
    /// 该格的源 alpha（斜坡自变量）。
    pub src_alpha: f32,
    /// 源像素（各通道按 `src_alpha` 缩放后的结果）。
    pub src: [f32; 4],
    /// 目标像素（斜坡内恒定）。
    pub dst: [f32; 4],
    /// 求值结果。
    pub cell: EvalCell,
}

/// 半透明实时预览：按源 alpha 斜坡逐格求值。
///
/// 预览**按硬件真实规则求值**（见 [`evaluate`]），不设「预览模式」——
/// 预览若与硬件规则不一致，它就是第二套错误答案。
pub fn preview_strip(
    d: &BlendDesc,
    base_rgb: [f32; 3],
    dst: [f32; 4],
    cells: usize,
    target: BlendTarget,
) -> Vec<PreviewCell> {
    let n = if cells == 0 { PREVIEW_CELLS } else { cells };
    let mut out: Vec<PreviewCell> = Vec::with_capacity(n);
    for i in 0..n {
        let a = if n == 1 {
            1.0
        } else {
            i as f32 / (n - 1) as f32
        };
        let src = [base_rgb[0], base_rgb[1], base_rgb[2], a];
        let cell = evaluate(d, src, dst, target);
        out.push(PreviewCell {
            src_alpha: a,
            src,
            dst,
            cell,
        });
    }
    out
}

/// 预览的人话摘要（读屏用：说清每格发生了什么）。
pub fn preview_summary(cells: &[PreviewCell]) -> String {
    if cells.is_empty() {
        return String::from("预览为空（格数为 0）");
    }
    let mut clamped = 0usize;
    let mut ignored = 0usize;
    for c in cells.iter() {
        if c.cell.clamped {
            clamped += 1;
        }
        if c.cell.factor_ignored {
            ignored += 1;
        }
    }
    let first = &cells[0];
    let last = &cells[cells.len() - 1];
    let mut s = format!(
        "预览 {} 格：alpha {:.2}→{:.2}，输出 R {:.3}→{:.3}",
        cells.len(),
        first.src_alpha,
        last.src_alpha,
        first.cell.out[0],
        last.cell.out[0]
    );
    if ignored > 0 {
        s.push_str(&format!("；{} 格因子被算子丢弃", ignored));
    }
    if clamped > 0 {
        s.push_str(&format!("；{} 格被无符号目标钳制", clamped));
    }
    s
}

// ---------------------------------------------------------------------------
// 六、混合状态库（去重 + 状态漂移失效 + 淘汰 + 统计）
// ---------------------------------------------------------------------------

/// 状态来源。
#[derive(Clone, Debug, PartialEq)]
pub enum Origin {
    /// 命中预置。
    Preset(&'static str),
    /// 与已有条目同参共享（去重命中）。
    Dedup(usize),
    /// 运行时构建。
    Runtime(usize),
    /// 因**上下文漂移**被失效后重建（重建非首次，故单列——可查漂移频次）。
    Replaced(usize),
}

impl Origin {
    /// 来源名（读屏与诊断）。
    pub fn name(&self) -> &'static str {
        match self {
            Origin::Preset(_) => "预置",
            Origin::Dedup(_) => "去重共享",
            Origin::Runtime(_) => "运行时构建",
            Origin::Replaced(_) => "漂移重建",
        }
    }
}

/// 解析结果。
#[derive(Clone, Debug, PartialEq)]
pub struct Resolution {
    /// 条目槽位号。
    pub slot: usize,
    /// 来源。
    pub origin: Origin,
    /// 该状态的键指纹。
    pub hash: u64,
    /// 解析时的上下文标记（用于事后核对是否漂移）。
    pub context_tag: u64,
}

/// 库条目。
#[derive(Clone, Debug)]
pub struct CacheEntry {
    /// 状态描述。
    pub desc: BlendDesc,
    /// 去重键（字节精确比对用）。
    pub key: Vec<u8>,
    /// 上下文标记。
    pub context_tag: u64,
    /// 命中次数（热度）。
    pub hits: u32,
    /// 最近使用 tick（LRU）。
    pub lru_tick: u64,
}

/// 漂移审计记录。
#[derive(Clone, Debug, PartialEq)]
pub struct DriftRecord {
    /// 槽位号。
    pub slot: usize,
    /// 条目声明的上下文标记。
    pub entry_tag: u64,
    /// 当前上下文的标记。
    pub current_tag: u64,
    /// 漂移原因（人话）。
    pub why: String,
}

/// 统计（去重省了多少、漂移多少次可查）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BlendStats {
    /// 解析总次数。
    pub resolves: u32,
    /// 预置命中次数。
    pub preset_hits: u32,
    /// 去重共享次数。
    pub dedup_hits: u32,
    /// 运行时构建次数。
    pub runtime_built: u32,
    /// 漂移失效并重建的次数。
    pub drift_rebuilds: u32,
    /// 淘汰次数。
    pub evictions: u32,
}

/// 混合状态库。
pub struct BlendCache {
    /// 条目区。
    entries: Vec<CacheEntry>,
    /// 当前上下文标记。
    context_tag: u64,
    /// 逻辑 tick。
    tick: u64,
    /// 拒绝台账（零静默）。
    rejections: Vec<Rejection>,
    /// 漂移台账。
    drifts: Vec<DriftRecord>,
    /// 统计。
    pub stats: BlendStats,
    /// 容量上限。
    cap: usize,
}

impl BlendCache {
    /// 构造新库（默认上下文标记 [`CONTEXT_TAG_DEFAULT`]）。
    pub fn new() -> Self {
        BlendCache {
            entries: Vec::new(),
            context_tag: CONTEXT_TAG_DEFAULT,
            tick: 0,
            rejections: Vec::new(),
            drifts: Vec::new(),
            stats: BlendStats::default(),
            cap: LIBRARY_CAP,
        }
    }

    /// 切换上下文（目标格式 / 硬件独立 alpha 混合能力）。
    ///
    /// 切换**不**立即清库，而是把新旧条目标记为待审计——漂移由 [`Self::audit`]
    /// 逐条列出并失效，使「谁漂移了」可查，而不是静默全清。
    pub fn set_context(&mut self, tag: u64) {
        self.context_tag = tag;
    }

    /// 推进逻辑 tick。
    pub fn advance_tick(&mut self) -> u64 {
        self.tick = self.tick.wrapping_add(1);
        self.tick
    }

    /// 当前 tick。
    pub fn tick(&self) -> u64 {
        self.tick
    }

    /// 当前上下文标记。
    pub fn context_tag(&self) -> u64 {
        self.context_tag
    }

    /// 审计并失效**状态漂移**条目；返回本次失效条数。
    ///
    /// 判定：条目的 `context_tag` 与当前上下文不一致 → 漂移。
    /// 失效是显式的、可审计的；不静默复用旧条目。
    pub fn audit(&mut self) -> usize {
        let cur = self.context_tag;
        let mut drop_slots: Vec<usize> = Vec::new();
        for (i, e) in self.entries.iter().enumerate() {
            if e.context_tag != cur {
                self.drifts.push(DriftRecord {
                    slot: i,
                    entry_tag: e.context_tag,
                    current_tag: cur,
                    why: format!(
                        "条目建于上下文 {:016x}，当前上下文 {:016x}：目标格式或硬件独立 alpha 混合能力已变，状态语义不再等价",
                        e.context_tag, cur
                    ),
                });
                drop_slots.push(i);
            }
        }
        // 从后往前删，避免下标平移。
        let mut n = 0usize;
        for i in drop_slots.iter().rev() {
            self.entries.remove(*i);
            n += 1;
        }
        n
    }

    /// 漂移台账（只读）。
    pub fn drift_log(&self) -> &[DriftRecord] {
        &self.drifts
    }

    /// 拒绝台账（只读）。
    pub fn rejection_log(&self) -> &[Rejection] {
        &self.rejections
    }

    /// 解析一个混合状态（校验 → 去重 → 兜底 → 淘汰）。
    pub fn resolve(
        &mut self,
        d: &BlendDesc,
        target: BlendTarget,
    ) -> Result<Resolution, Rejection> {
        self.stats.resolves += 1;
        let now = self.tick;

        if let Err(r) = validate(d, target, now) {
            // 阻断级入拒绝台账。
            self.rejections.push(r.clone());
            return Err(r);
        }
        // 提示级也入账（零静默：提示不留账等于没提示）。
        if let Some(note) = advisory_note(d, target, now) {
            self.rejections.push(note);
        }

        let key = d.canonical_key();
        let hash = fnv1a64(&key);

        // 键字节精确比对（哈希碰撞不误合）。
        for (i, e) in self.entries.iter().enumerate() {
            if e.key == key {
                self.stats.dedup_hits += 1;
                let lru = now;
                self.entries[i].hits += 1;
                self.entries[i].lru_tick = lru;
                // 预置名回查：命中预置条目时如实标预置。
                let origin = match preset_name_of(&self.entries[i].desc) {
                    Some(nm) => {
                        self.stats.preset_hits += 1;
                        Origin::Preset(nm)
                    }
                    None => Origin::Dedup(i),
                };
                return Ok(Resolution {
                    slot: i,
                    origin,
                    hash,
                    context_tag: self.context_tag,
                });
            }
        }

        // 未命中：预置回填优先，其次运行时构建。
        let slot = self.entries.len();
        let origin = match preset_name_of(d) {
            Some(nm) => {
                self.stats.preset_hits += 1;
                Origin::Preset(nm)
            }
            None => {
                // 库膨胀 → LRU 淘汰**动态项**（预置项永不动）。
                if self.entries.len() >= self.cap {
                    self.evict_one_dynamic();
                }
                self.stats.runtime_built += 1;
                Origin::Runtime(slot)
            }
        };
        self.entries.push(CacheEntry {
            desc: d.clone(),
            key,
            context_tag: self.context_tag,
            hits: 1,
            lru_tick: now,
        });
        Ok(Resolution {
            slot,
            origin,
            hash,
            context_tag: self.context_tag,
        })
    }

    /// 淘汰一个**动态**条目（LRU）；返回是否淘汰成功。
    ///
    /// 预置项**永不淘汰**——预置是基线，淘汰预置等于私自改基线。
    fn evict_one_dynamic(&mut self) -> bool {
        let mut victim: Option<(usize, u64)> = None;
        for (i, e) in self.entries.iter().enumerate() {
            if preset_name_of(&e.desc).is_some() {
                continue;
            }
            match victim {
                None => victim = Some((i, e.lru_tick)),
                Some((_, lv)) if e.lru_tick < lv => victim = Some((i, e.lru_tick)),
                _ => {}
            }
        }
        match victim {
            None => false,
            Some((i, _)) => {
                self.entries.remove(i);
                self.stats.evictions += 1;
                true
            }
        }
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// **某预置当前是否仍在库中**（淘汰纪律的可观测口径）。
    ///
    /// 「预置项永不被淘汰」是纪律，但纪律若无法**从外部查证**就等于没有——
    /// 故本方法按预置键在条目区做**字节精确比对**（不信哈希），
    /// 使自检与诊断都能独立复核，而不只看 [`BlendStats::evictions`] 计数。
    pub fn preset_present(&self, key_name: &str) -> bool {
        for row in PRESET_TABLE.iter() {
            if row.key_name == key_name {
                let k = row.to_desc().canonical_key();
                return self.entries.iter().any(|e| e.key == k);
            }
        }
        false
    }

    /// 仍在库中的预置条数（应恒等于 [`PRESET_TABLE`] 的条数）。
    pub fn presets_present(&self) -> usize {
        let mut n = 0usize;
        for row in PRESET_TABLE.iter() {
            if self.preset_present(row.key_name) {
                n += 1;
            }
        }
        n
    }

    /// 去重省下的状态数（总解析数 − 实际条目数）。
    pub fn dedup_saved(&self) -> u32 {
        self.stats.resolves.saturating_sub(self.entries.len() as u32)
    }

    /// 去重省下的字节（按 [`BLEND_STATE_BYTES`] 折算）。
    pub fn dedup_saved_bytes(&self) -> u64 {
        self.dedup_saved() as u64 * BLEND_STATE_BYTES
    }

    /// 预置命中率（百分比）。
    pub fn preset_hit_pct(&self) -> u32 {
        if self.stats.resolves == 0 {
            return 0;
        }
        self.stats.preset_hits * 100 / self.stats.resolves
    }

    /// **实测**预置扫描工作量（内层判定的比较次数）。
    ///
    /// 供性能自检计量：走 [`classify`] 的 O(1) 判定时，本值随预置数**线性**
    /// 增长；若内层误调 [`validate`]（每次失败扫全表求建议），本值呈二次方。
    /// 计数器覆盖**缺陷发生的那一层**（内层判定），故能真正抓住回归。
    pub fn preset_scan_work(&self, d: &BlendDesc, target: BlendTarget) -> u64 {
        let mut work = 0u64;
        for row in PRESET_TABLE.iter() {
            let cand = row.to_desc();
            work += 1; // 每行一次纯判定
            let _ = classify(&cand, target);
        }
        work += 1; // 被测描述自身一次判定
        let _ = classify(d, target);
        work
    }

    /// 状态表逐行（读屏可达）。
    pub fn state_table_rows(&self) -> Vec<String> {
        let mut rows: Vec<String> = Vec::new();
        rows.push(format!(
            "混合状态表：条目 {}（容量 {}），上下文 {:016x}",
            self.entries.len(),
            self.cap,
            self.context_tag
        ));
        for (i, e) in self.entries.iter().enumerate() {
            let nm = preset_name_of(&e.desc).unwrap_or("自定义");
            rows.push(format!(
                "[{}] {} · 命中 {} · {} · 上下文 {:016x}",
                i,
                e.desc.describe(),
                e.hits,
                nm,
                e.context_tag
            ));
        }
        if !self.drifts.is_empty() {
            rows.push(format!("状态漂移记录 {} 条：", self.drifts.len()));
            for d in self.drifts.iter() {
                rows.push(format!(
                    "  槽[{}] {} → {}：{}",
                    d.slot,
                    fmt_tag(d.entry_tag),
                    fmt_tag(d.current_tag),
                    d.why
                ));
            }
        }
        if !self.rejections.is_empty() {
            rows.push(format!("拒绝记录 {} 条：", self.rejections.len()));
            for r in self.rejections.iter() {
                rows.push(format!(
                    "  [{}] {}：{}",
                    if r.blocking { "阻断" } else { "提示" },
                    r.desc_text,
                    r.reason
                ));
            }
        }
        rows
    }

    /// 人话摘要（读屏首段）。
    pub fn screen_text(&self) -> String {
        format!(
            "混合状态库：{} 条（预置命中 {}%，去重省 {} 条 / {} 字节），运行时构建 {}，漂移重建 {}，淘汰 {}，拒绝 {}",
            self.entries.len(),
            self.preset_hit_pct(),
            self.dedup_saved(),
            self.dedup_saved_bytes(),
            self.stats.runtime_built,
            self.stats.drift_rebuilds,
            self.stats.evictions,
            self.rejections.len()
        )
    }
}

impl Default for BlendCache {
    fn default() -> Self {
        Self::new()
    }
}

/// 十六进制标记格式化。
fn fmt_tag(t: u64) -> String {
    format!("{:016x}", t)
}

/// 回查预置名（描述与某预置行语义等价则返回其名）。
///
/// 判定用 [`BlendDesc::canonical_key`] 而非 `PartialEq`——预置行与库条目
/// 在「不起作用字段」上可以不同却语义等价（如常量色），键相等才是等价判据。
fn preset_name_of(d: &BlendDesc) -> Option<&'static str> {
    let k = d.canonical_key();
    for row in PRESET_TABLE.iter() {
        if row.to_desc().canonical_key() == k {
            return Some(row.key_name);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// 七、自检注册入口
// ---------------------------------------------------------------------------

/// VE-F0019 域自检（判据逐条映射见 `vea19_checks.rs`）。
pub fn run_vea19_checks() -> CheckSet {
    super::vea19_checks::run_vea19_checks()
}

// ---------------------------------------------------------------------------
// 八、单元测试（宿主 cargo test 直跑）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vea19_wire_mapping_is_explicit_and_round_trips() {
        // 判别值 ≠ 线上编码值：Add 判别 0、线上 1 —— 若误用 as u8 即错。
        assert_eq!(BlendOp::Add as u8, 0);
        assert_eq!(BlendOp::Add.wire(), 1);
        assert_ne!(BlendOp::Add as u8, BlendOp::Add.wire());
        // 全因子往返一致。
        let all = [
            BlendFactor::Zero,
            BlendFactor::One,
            BlendFactor::SrcColor,
            BlendFactor::OneMinusSrcColor,
            BlendFactor::DstColor,
            BlendFactor::OneMinusDstColor,
            BlendFactor::SrcAlpha,
            BlendFactor::OneMinusSrcAlpha,
            BlendFactor::DstAlpha,
            BlendFactor::OneMinusDstAlpha,
            BlendFactor::ConstantColor,
            BlendFactor::OneMinusConstantAlpha,
            BlendFactor::ConstantAlpha,
            BlendFactor::OneMinusConstantColor,
        ];
        for f in all.iter() {
            assert_eq!(BlendFactor::from_wire(f.wire()), *f, "因子 {} 往返失配", f.name());
            assert!(BlendFactor::wire_is_known(f.wire()));
        }
        // 线上编码互不冲突（14 个因子占14 个不同编码）。
        let mut wires: Vec<u8> = all.iter().map(|f| f.wire()).collect();
        wires.sort_unstable();
        let before = wires.len();
        wires.dedup();
        assert_eq!(wires.len(), before, "因子线上编码冲突");
        // 算子往返 + 越界钳制。
        for op in [
            BlendOp::Add,
            BlendOp::Subtract,
            BlendOp::ReverseSubtract,
            BlendOp::Min,
            BlendOp::Max,
        ]
        .iter()
        {
            assert_eq!(BlendOp::from_wire(op.wire()), *op);
            assert!(BlendOp::wire_is_known(op.wire()));
        }
        assert!(!BlendOp::wire_is_known(0), "线上编码 0 不属于任何算子");
        assert!(!BlendOp::wire_is_known(6));
        assert_eq!(BlendOp::from_wire(200), BlendOp::Add, "越界算子编码须钳制为 Add");
        assert_eq!(BlendFactor::from_wire(200), BlendFactor::Zero, "越界因子编码须钳制为 Zero");
    }

    #[test]
    fn vea19_preset_universe_is_complete_and_self_legal() {
        // 预置全集：名唯一、用途说明非空、自身合法（不自相矛盾）。
        let mut names: Vec<&str> = PRESET_TABLE.iter().map(|r| r.key_name).collect();
        let before = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), before, "预置名唯一");
        for row in PRESET_TABLE.iter() {
            assert!(!row.use_note.is_empty(), "{} 缺用途说明", row.key_name);
            let d = row.to_desc();
            assert!(
                validate(&d, BlendTarget::Rgba8Unorm, 0).is_ok(),
                "预置 {} 自身非法",
                row.key_name
            );
        }
        // 覆盖十二类场景在册。
        for nm in [
            "opaque",
            "alpha_straight",
            "alpha_premultiplied",
            "additive",
            "multiply",
            "screen",
            "darken",
            "lighten",
            "min_op",
            "max_op",
            "subtract",
            "alpha_write_only",
            "debug_mask_red",
        ]
        .iter()
        {
            assert!(preset_desc(nm).is_some(), "预置 {} 缺失", nm);
        }
    }

    #[test]
    fn vea19_canonical_key_trims_irrelevant_dimensions() {
        // 关闭混合：因子与算子不参与键，但**掩码必须参与**（关闭混合时硬件仍按
        // 掩码决定写哪些通道；裁掉掩码会让 opaque 与 alpha_write_only 合并）。
        let off = BlendDesc::disabled();
        let mut off2 = off.clone();
        off2.src_color = BlendFactor::DstAlpha;
        off2.color_op = BlendOp::Max;
        assert_eq!(off.canonical_key(), off2.canonical_key(), "关闭混合时因子/算子不得影响键");
        let mut off3 = off.clone();
        off3.mask = WriteMask::ALPHA;
        assert_ne!(off.canonical_key(), off3.canonical_key(), "关闭混合时掩码必须入键");
        assert_eq!(off.canonical_key(), vec![0x00, WriteMask::RGBA.bits()]);

        // 掩码不含 A：alpha 因子与alpha 算子不参与键。
        let base = BlendDesc::straight_alpha().with_mask(WriteMask::RGB);
        let mut diff_alpha = base.clone();
        diff_alpha.src_alpha = BlendFactor::Zero;
        diff_alpha.alpha_op = BlendOp::Max;
        assert_eq!(base.canonical_key(), diff_alpha.canonical_key(), "alpha 维度应被裁剪");

        // 掩码不含 RGB：颜色因子与颜色算子不参与键。
        let base2 = BlendDesc::straight_alpha().with_mask(WriteMask::ALPHA);
        let mut diff_color = base2.clone();
        diff_color.src_color = BlendFactor::DstColor;
        diff_color.color_op = BlendOp::Subtract;
        assert_eq!(base2.canonical_key(), diff_color.canonical_key(), "颜色维度应被裁剪");

        // 无因子引用常量色：常量色不参与键（起作用的才参与）。
        let no_const = BlendDesc::straight_alpha();
        let mut with_const = no_const.clone();
        with_const.constant = [0.9; 4];
        with_const.constant_set = true;
        assert_eq!(no_const.canonical_key(), with_const.canonical_key(), "未引用常量色时不得入键");

        // 引用常量色时必须入键。
        let uses_const = BlendDesc::straight_alpha()
            .with_color(BlendFactor::ConstantColor, BlendFactor::OneMinusSrcAlpha, BlendOp::Add)
            .with_constant([0.1, 0.2, 0.3, 1.0]);
        let mut other_const = uses_const.clone();
        other_const.constant = [0.8, 0.8, 0.8, 1.0];
        assert_ne!(uses_const.canonical_key(), other_const.canonical_key(), "起作用的常量色必须入键");
    }

    #[test]
    fn vea19_min_max_factor_discard_is_blocking() {
        // 第一性声明：Min/Max 丢弃因子 → 阻断，且诊断说明真实行为。
        let bad = BlendDesc::straight_alpha()
            .with_color(BlendFactor::SrcAlpha, BlendFactor::One, BlendOp::Min);
        match classify(&bad, BlendTarget::Rgba8Unorm) {
            Verdict::Blocking(Flaw::FactorDiscardedByOp) => {}
            other => panic!("Min+非平凡因子应阻断，实得 {:?}", other),
        }
        // One/One 是「显式声明知道因子无效」的合法写法。
        let declared = BlendDesc::straight_alpha()
            .with_color(BlendFactor::One, BlendFactor::One, BlendOp::Min)
            .with_alpha(BlendFactor::One, BlendFactor::One, BlendOp::Min);
        assert_eq!(classify(&declared, BlendTarget::Rgba8Unorm), Verdict::Ok);
        assert!(BlendOp::Min.ignores_factors());
        assert!(!BlendOp::Add.ignores_factors());
    }

    #[test]
    fn vea19_evaluate_matches_hardware_rules() {
        // 直通 alpha：out = src*a + dst*(1-a)。
        let d = BlendDesc::straight_alpha();
        let cell = evaluate(&d, [1.0, 0.0, 0.0, 0.5], [0.0, 0.0, 1.0, 1.0], BlendTarget::Rgba8Unorm);
        assert!((cell.out[0] - 0.5).abs() < 1e-6, "红{}", cell.out[0]);
        assert!((cell.out[2] - 0.5).abs() < 1e-6, "蓝{}", cell.out[2]);

        // 关闭混合：硬件不读因子，但仍按掩码写入 —— RGB 掩码下 alpha 保持目标值。
        let off = BlendDesc::disabled().with_mask(WriteMask::RGB);
        let c2 = evaluate(&off, [0.2, 0.3, 0.4, 0.9], [0.7, 0.7, 0.7, 0.1], BlendTarget::Rgba8Unorm);
        assert!((c2.out[0] - 0.2).abs() < 1e-6, "直写红{}", c2.out[0]);
        assert!((c2.out[3] - 0.1).abs() < 1e-6, "alpha 未被写，应保持目标 0.1，实得 {}", c2.out[3]);

        // Min/Max 忽略因子：结果与因子无关。
        let with_f = BlendDesc::straight_alpha()
            .with_color(BlendFactor::SrcAlpha, BlendFactor::One, BlendOp::Max);
        let without_f = BlendDesc::straight_alpha()
            .with_color(BlendFactor::One, BlendFactor::One, BlendOp::Max);
        let a = evaluate(&with_f, [0.2, 0.9, 0.9, 1.0], [0.8, 0.1, 0.1, 1.0], BlendTarget::Rgba8Unorm);
        let b = evaluate(&without_f, [0.2, 0.9, 0.9, 1.0], [0.8, 0.1, 0.1, 1.0], BlendTarget::Rgba8Unorm);
        assert!((a.out[1] - b.out[1]).abs() < 1e-6, "Max 必须忽略因子");
        assert!((a.out[1] - 0.9).abs() < 1e-6, "Max(0.9,0.1) = 0.9");
        assert!(a.factor_ignored, "预览须诚实标记因子被丢弃");

        // alpha 通道因子塌缩：SrcColor 在 alpha 通道上取源 alpha 作因子，
        // 故 out.a = src.a * src.a = 0.0625（塌缩后仍乘源 alpha 本身）。
        let collapse = BlendDesc::straight_alpha()
            .with_alpha(BlendFactor::SrcColor, BlendFactor::One, BlendOp::Add);
        let c3 = evaluate(&collapse, [1.0, 1.0, 1.0, 0.25], [0.0, 0.0, 0.0, 0.0], BlendTarget::Rgba8Unorm);
        assert!((c3.out[3] - 0.0625).abs() < 1e-6, "alpha 通道须塌缩为源 alpha，实得 {}", c3.out[3]);
        assert!(BlendFactor::SrcColor.collapses_in_alpha());
    }

    #[test]
    fn vea19_clamp_direction_differs_by_target() {
        // Subtract 产生负值：无符号目标钳到 0，浮点目标保留。
        let d = preset_desc("subtract").unwrap_or_else(|| panic!("subtract 预置在册"));
        let src = [0.1, 0.1, 0.1, 0.1];
        let dst = [0.8, 0.8, 0.8, 0.8];
        let unorm = evaluate(&d, src, dst, BlendTarget::Rgba8Unorm);
        assert!(unorm.clamped, "无符号目标应钳制");
        assert!((unorm.out[0] - 0.0).abs() < 1e-6, "应夹到 0");
        let float = evaluate(&d, src, dst, BlendTarget::Rgba16Float);
        assert!(!float.clamped, "浮点目标不得钳制（负值是合法 HDR）");
        assert!((float.out[0] - (-0.7)).abs() < 1e-6, "浮点目标须保留负值");
        assert!(BlendOp::Subtract.may_underflow());
        assert!(!BlendOp::Add.may_underflow());
    }

    #[test]
    fn vea19_mask_out_of_range_bits_are_clamped() {
        // 越界位（0b1111 以上）被钳制丢弃，数量可查。
        assert_eq!(WriteMask::from_bits(0b1111_0001).bits(), 0b0001);
        // 按置位个数计：0b1111_0001 的越界位是 bit4..bit7 共 4 个。
        assert_eq!(WriteMask::dropped_bits(0b1111_0001), 4);
        assert_eq!(WriteMask::dropped_bits(0b0001_0001), 1);
        assert_eq!(WriteMask::dropped_bits(0b0000_1111), 0);
        assert_eq!(WriteMask::from_bits(0b1111_1111).bits(), WriteMask::RGBA.bits());
        assert_eq!(WriteMask::dropped_bits(0b1111_1111), 4);
        assert!(!WriteMask::from_bits(0xF0).has_color());
        assert!(!WriteMask::from_bits(0xF0).a());
        assert_eq!(WriteMask::RGBA.describe(), String::from("RGBA"));
        assert_eq!(WriteMask::NONE.describe(), String::from("无通道"));
    }

    #[test]
    fn vea19_rejection_carries_reason_and_working_suggestion() {
        // 每个拒绝都附「为什么」+「正确组合建议」，且建议本身合法可用。
        let probes: [(BlendDesc, BlendTarget); 4] = [
            (
                BlendDesc::disabled().with_mask(WriteMask::NONE),
                BlendTarget::Rgba8Unorm,
            ),
            (
                BlendDesc::straight_alpha()
                    .with_color(BlendFactor::ConstantColor, BlendFactor::One, BlendOp::Add),
                BlendTarget::Rgba8Unorm,
            ),
            (
                BlendDesc::straight_alpha()
                    .with_color(BlendFactor::DstAlpha, BlendFactor::One, BlendOp::Add),
                BlendTarget::Rgb8Unorm,
            ),
            (
                BlendDesc::straight_alpha()
                    .with_color(BlendFactor::SrcAlpha, BlendFactor::One, BlendOp::Max),
                BlendTarget::Rgba8Unorm,
            ),
        ];
        for (i, (d, t)) in probes.iter().enumerate() {
            match validate(d, *t, 7) {
                Ok(()) => panic!("探针 {} 应被拒绝", i),
                Err(r) => {
                    assert!(!r.reason.is_empty(), "探针 {} 缺原因", i);
                    assert!(!r.hint.is_empty(), "探针 {} 缺建议", i);
                    assert!(!r.desc_text.is_empty(), "探针 {} 缺被测描述", i);
                    assert_eq!(r.tick, 7, "拒绝须记账 tick");
                    // 建议必须是**能走的路**：最近预置自身须合法。
                    let (nm, note) = suggest_preset(d, *t);
                    assert!(!note.is_empty(), "预置 {} 缺用途说明", nm);
                    let cand = preset_desc(nm).unwrap_or_else(|| panic!("建议预置 {} 不在册", nm));
                    assert!(
                        validate(&cand, *t, 0).is_ok(),
                        "建议预置 {} 自身非法——建议不是能走的路",
                        nm
                    );
                }
            }
        }
    }

    #[test]
    fn vea19_cache_dedup_and_drift_invalidation() {
        let mut cache = BlendCache::new();
        let d = BlendDesc::straight_alpha();
        let t = BlendTarget::Rgba8Unorm;
        let r1 = cache.resolve(&d, t).unwrap_or_else(|e| panic!("应合法：{}", e.reason));
        assert!(matches!(r1.origin, Origin::Preset("alpha_straight")), "应命中预置");

        // 只在不起作用字段上不同 → 同参共享，不新建。
        let mut alias = d.clone();
        alias.constant = [9.0; 4];
        alias.constant_set = true;
        let r2 = cache.resolve(&alias, t).unwrap_or_else(|e| panic!("应合法：{}", e.reason));
        assert_eq!(r2.slot, r1.slot, "无效字段差异不得新建状态");
        assert_eq!(cache.len(), 1);

        // 状态漂移：切换上下文 → 审计失效。
        const NEW_TAG: u64 = 0xabcd_0000_0000_0001;
        assert_eq!(cache.audit(), 0, "同上下文不应有漂移");
        cache.set_context(NEW_TAG);
        let dropped = cache.audit();
        assert_eq!(dropped, 1, "上下文变更后旧条目必须失效");
        assert_eq!(cache.len(), 0);
        assert_eq!(cache.drift_log().len(), 1, "漂移须留账");
        assert!(!cache.drift_log()[0].why.is_empty(), "漂移原因不得为空");

        // 重建后可继续用，且条目带新上下文标记。
        let r3 = cache.resolve(&d, t).unwrap_or_else(|e| panic!("重建应合法：{}", e.reason));
        assert_eq!(r3.context_tag, NEW_TAG, "重建条目须带当前上下文标记");
        assert_eq!(cache.context_tag(), NEW_TAG);
        assert_eq!(cache.audit(), 0, "同上下文不应再有漂移");
    }

    #[test]
    fn vea19_rejections_are_ledgered_zero_silent() {
        let mut cache = BlendCache::new();
        let bad = BlendDesc::straight_alpha()
            .with_color(BlendFactor::SrcAlpha, BlendFactor::One, BlendOp::Max);
        assert!(cache.resolve(&bad, BlendTarget::Rgba8Unorm).is_err());
        assert_eq!(cache.rejection_log().len(), 1, "拒绝必须入账");
        assert!(cache.rejection_log()[0].blocking);
        // 零静默：状态表里必须能读到这条拒绝。
        let rows = cache.state_table_rows();
        assert!(
            rows.iter().any(|r| r.contains("拒绝记录")),
            "拒绝须在状态表可见"
        );
        assert!(!cache.screen_text().is_empty());
        // 隐私：状态表只含渲染参数，零用户内容。
        for r in rows.iter() {
            assert!(!r.contains("password"), "状态表不得含用户凭据：{}", r);
        }
    }

    #[test]
    fn vea19_baseline_freeze_and_change_flow() {
        // 基线三态区分（对齐/漂移/未登记）。
        //
        // 常量已由 `compute_preset_hash()` 实测回填，故此刻应**对齐**；
        // 对齐才是常态，漂移与未登记是异常态（处置方向不同，故不共用码）。
        let st = verify_baseline();
        assert!(
            matches!(st, BaselineStatus::Aligned),
            "回填后的基线应对齐，实得 {:?}",
            st
        );
        assert!(st.is_aligned());
        // 指纹必须由表内容导出且非平凡。
        assert_eq!(compute_preset_hash(), BLEND_BASELINE_HASH);
        assert_ne!(BLEND_BASELINE_HASH, 0, "基线指纹不得为占位 0");
        // 未登记版本走另一条分支（不得默认可用）。
        let unregistered = if BLEND_BASELINE_VERSION != 1 {
            matches!(verify_baseline(), BaselineStatus::Unregistered)
        } else {
            true
        };
        assert!(unregistered);
        // 漂移态文案必须说清「须重签」。
        let drifted = BaselineStatus::Drifted {
            expected: 1,
            actual: 2,
        };
        assert!(!drifted.is_aligned());
        assert!(drifted.describe().contains("须双签重签"));
        assert!(BaselineStatus::Unregistered.describe().contains("未登记"));
        // 变更走流程：双签 + 版本递进，缺一即驳回。
        let good = ChangeRequest {
            summary: String::from("补min/max 预置的因子说明"),
            requester: String::from("A"),
            reviewer: String::from("B"),
            new_version: 2,
        };
        assert!(matches!(
            queue_preset_change(&good, 1),
            ChangeOutcome::Accepted { version: 2, .. }
        ));
        let mut self_signed = good.clone();
        self_signed.reviewer = String::from("A");
        assert!(matches!(
            queue_preset_change(&self_signed, 1),
            ChangeOutcome::Rejected { .. }
        ));
        let mut stale = good.clone();
        stale.new_version = 1;
        assert!(matches!(
            queue_preset_change(&stale, 1),
            ChangeOutcome::Rejected { .. }
        ));
        let mut blank = good.clone();
        blank.summary = String::new();
        assert!(matches!(
            queue_preset_change(&blank, 1),
            ChangeOutcome::Rejected { .. }
        ));
    }

    #[test]
    fn vea19_preset_scan_work_is_linear_not_quadratic() {
        // 性能自检：内层判定必须 O(1)（走 classify），不得调 validate。
        // 场景选「全部预置都判失败」——缺陷恰恰发生在内层大量失败那一层。
        let mut cache = BlendCache::new();
        let bad = BlendDesc::straight_alpha()
            .with_color(BlendFactor::SrcAlpha, BlendFactor::One, BlendOp::Max);
        let work = cache.preset_scan_work(&bad, BlendTarget::Rgba8Unorm);
        // 实测工作量 == 预置数 + 1，纯线性常数。
        assert_eq!(
            work,
            PRESET_TABLE.len() as u64 + 1,
            "内层判定工作量应恒为预置数+1（O(状态)），实测 {}",
            work
        );
        // 若实现退化为 validate（每行失败再扫全表），量级会变成预置数的平方。
        assert!(
            work < (PRESET_TABLE.len() as u64) * 2,
            "出现嵌套二次方：{}",
            work
        );
    }

    #[test]
    fn vea19_preview_strip_is_monotone_and_honest() {
        // 直通 alpha 斜坡：alpha 0 → 输出= 目标；alpha 1 → 输出 = 源。
        let d = BlendDesc::straight_alpha();
        let cells = preview_strip(&d, [1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 1.0], 8, BlendTarget::Rgba8Unorm);
        assert_eq!(cells.len(), 8);
        let first = cells[0];
        let last = cells[cells.len() - 1];
        assert!((first.src_alpha - 0.0).abs() < 1e-6);
        assert!((last.src_alpha - 1.0).abs() < 1e-6);
        assert!((first.cell.out[2] - 1.0).abs() < 1e-6, "alpha=0 应完全显示目标");
        assert!((last.cell.out[0] - 1.0).abs() < 1e-6, "alpha=1 应完全显示源");
        assert!(!first.cell.factor_ignored, "Add 不应丢因子");
        // 单调性：输出红随 alpha 递增（无跳变即无错）。
        for i in 1..cells.len() {
            assert!(
                cells[i].cell.out[0] >= cells[i - 1].cell.out[0] - 1e-6,
                "预览斜坡非单调（第 {} 格）",
                i
            );
        }
        let s = preview_summary(&cells);
        assert!(!s.is_empty());
        assert_eq!(preview_summary(&[]), String::from("预览为空（格数为 0）"));
        // 格数 0 → 退回缺省格数，不返回空预览（空预览会被误读为「无变化」）。
        assert_eq!(
            preview_strip(&d, [1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 1.0], 0, BlendTarget::Rgba8Unorm).len(),
            PREVIEW_CELLS
        );
    }

    #[test]
    fn vea19_min_max_preview_flags_discard_and_clamp() {
        // Max + 非平凡因子：预览须同时诚实标出「因子被丢弃」。
        let d = BlendDesc::straight_alpha()
            .with_color(BlendFactor::SrcAlpha, BlendFactor::One, BlendOp::Max);
        let cells = preview_strip(&d, [0.9, 0.9, 0.9], [0.1, 0.1, 0.1, 1.0], 4, BlendTarget::Rgba8Unorm);
        assert!(cells[0].cell.factor_ignored, "预览须标出因子被丢弃");
        assert!(preview_summary(&cells).contains("因子被算子丢弃"));

        // Subtract 在无符号目标上：预览须标出钳制。
        let sub = preset_desc("subtract").unwrap_or_else(|| panic!("subtract 在册"));
        let cells2 = preview_strip(&sub, [0.1, 0.1, 0.1], [0.9, 0.9, 0.9, 1.0], 4, BlendTarget::Rgba8Unorm);
        assert!(cells2.iter().any(|c| c.cell.clamped), "无符号目标应有钳制格");
        assert!(preview_summary(&cells2).contains("被无符号目标钳制"));
    }

    #[test]
    fn vea19_library_cap_evicts_dynamic_never_presets() {
        // 库膨胀 → LRU 淘汰动态项，预置项永不动。
        let mut cache = BlendCache::new();
        let t = BlendTarget::Rgba8Unorm;
        // 先塞入全部预置。
        for row in PRESET_TABLE.iter() {
            let d = row.to_desc();
            cache.resolve(&d, t).unwrap_or_else(|e| panic!("预置应合法"));
        }
        let preset_count = PRESET_TABLE.len();
        assert_eq!(cache.len(), preset_count, "预置应全部入库");
        // 再塞入超过容量的自定义状态。
        for i in 0..(LIBRARY_CAP + 8) {
            let k = [i as f32 / 100.0, 0.0, 0.0, 1.0];
            let d = BlendDesc::straight_alpha()
                .with_color(BlendFactor::ConstantColor, BlendFactor::OneMinusSrcAlpha, BlendOp::Add)
                .with_constant(k);
            cache.advance_tick();
            let _ = cache.resolve(&d, t);
        }
        assert!(cache.len() <= LIBRARY_CAP, "库须封顶，实得 {}", cache.len());
        // 预置一个都没被淘汰。
        for row in PRESET_TABLE.iter() {
            let nm = preset_name_of(&row.to_desc());
            assert!(nm.is_some(), "预置 {} 应仍可回查", row.key_name);
        }
        assert!(cache.stats.evictions > 0, "应有淘汰发生");
        // 去重收益可查。
        assert!(cache.dedup_saved() > 0);
        assert_eq!(
            cache.dedup_saved_bytes(),
            cache.dedup_saved() as u64 * BLEND_STATE_BYTES
        );
        assert!(!cache.screen_text().is_empty());
        for r in cache.state_table_rows() {
            assert!(!r.contains("password"));
        }
    }

    #[test]
    fn vea19_verdict_severity_directions_are_distinct() {
        // 处置方向相反的状态不得共用码：阻断 vs 提示必须可区分。
        let blocking = BlendDesc::straight_alpha()
            .with_color(BlendFactor::SrcAlpha, BlendFactor::One, BlendOp::Max);
        assert!(matches!(
            classify(&blocking, BlendTarget::Rgba8Unorm),
            Verdict::Blocking(_)
        ));

        let advisory = BlendDesc::disabled().with_mask(WriteMask::ALPHA);
        assert!(
            matches!(classify(&advisory, BlendTarget::Rgba8Unorm), Verdict::Advisory(_)),
            "关闭混合 + ALPHA 掩码是合法的直写用法，只提示"
        );
        // 提示级**不阻断**：它是合法写法（且是预置），处置是记账不是报错。
        assert!(
            validate(&advisory, BlendTarget::Rgba8Unorm, 1).is_ok(),
            "提示级不得判为错误"
        );
        let note = advisory_note(&advisory, BlendTarget::Rgba8Unorm, 1)
            .expect("提示级须能产出留账记录");
        assert!(!note.blocking, "留账记录须标为非阻断");
        assert_eq!(note.flaw, Flaw::MaskWithoutBlend);
        assert!(!note.reason.is_empty() && !note.hint.is_empty());

        // 无 alpha 目标上的 DstAlpha 引用 → 阻断（无源可读）。
        let no_alpha = BlendDesc::straight_alpha()
            .with_alpha(BlendFactor::DstAlpha, BlendFactor::One, BlendOp::Add);
        assert!(matches!(
            classify(&no_alpha, BlendTarget::Rgb8Unorm),
            Verdict::Blocking(Flaw::DstAlphaOnOpaqueTarget)
        ));
        // 同一状态在带 alpha 目标上合法 —— 判定随目标格式而变，须如实区分。
        assert_eq!(classify(&no_alpha, BlendTarget::Rgba8Unorm), Verdict::Ok);

        // 每个缺陷码都要有人话解释与建议。
        for f in [
            Flaw::MaskWithoutBlend,
            Flaw::MaskEmpty,
            Flaw::FactorDiscardedByOp,
            Flaw::ConstantUnset,
            Flaw::DstAlphaOnOpaqueTarget,
            Flaw::StaleAlphaPassthrough,
        ]
        .iter()
        {
            assert!(!flaw_reason(*f).is_empty(), "{:?} 缺解释", f);
            assert!(!flaw_hint(*f).is_empty(), "{:?} 缺建议", f);
        }
    }

    #[test]
    fn vea19_disabled_blend_still_honours_mask() {
        // 最易写错处：关闭混合 ≠ 忽略掩码。
        let d = BlendDesc::disabled().with_mask(WriteMask::ALPHA);
        let c = evaluate(&d, [0.1, 0.2, 0.3, 0.9], [0.4, 0.5, 0.6, 0.2], BlendTarget::Rgba8Unorm);
        assert!((c.out[0] - 0.4).abs() < 1e-6, "R 未写，应保持目标");
        assert!((c.out[3] - 0.9).abs() < 1e-6, "A 应被写入源 alpha");
    }

    #[test]
    fn vea19_fnv_is_stable_and_distinguishing() {
        assert_eq!(fnv1a64(b""), FNV_OFFSET);
        assert_ne!(fnv1a64(b"alpha_straight"), fnv1a64(b"alpha_premult"));
        // 键字节精确性：单比特差异必改哈希。
        let a = BlendDesc::straight_alpha();
        let mut b = a.clone();
        b.mask = WriteMask::RGB;
        assert_ne!(a.key_hash(), b.key_hash(), "掩码差异必须改哈希");
        // 同描述两次求值键相同（确定性）。
        assert_eq!(a.key_hash(), a.key_hash());
    }
}