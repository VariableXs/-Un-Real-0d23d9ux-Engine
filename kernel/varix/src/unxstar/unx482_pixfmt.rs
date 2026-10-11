//! UNX-F0482 · 像素格式判定（BGRX/XRGB 族）（CoRun·Unxreal · 帧缓冲深化之二）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal.md#UNX-F0482`
//!
//! **判据 J1**：格式判定四分发（32bpp: BGRX/XRGB/24bpp/16bpp）落账，
//! 判定依据账注（红蓝序实测抽样核）。
//!
//! ## 要点一：格式判定四分发
//!
//! 依据调用方声明的位深分四路：32bpp 进入 BGRX/XRGB 双假设裁决（红蓝
//! 序实测抽样核定夺）；24bpp 进入 BGR 字节序核验；16bpp 进入 RGB565/
//! RGB555 变体探测。声明位深不在 {32, 24, 16} 闭集内 → 诊断码立案，
//! 不猜、不回退、不 panic。
//!
//! ## 要点二：红蓝序实测抽样核——判定依据账注
//!
//! 已知颜色协议：探针区前三像素按通道语义写纯红/纯绿/纯蓝（写侧由
//! 调用方用本模块导出的编码函数完成，本模块零 IO 只做观察面分类）。
//! 裁决证据：纯红像素的 0xFF 落在字节 2 → BGRX；落在字节 1 → XRGB；
//! 纯蓝像素交叉定位（BGRX 字节 0 / XRGB 字节 3）；纯绿像素为锚
//! （BGRX 字节 1 / XRGB 字节 2）。实测命中假设的整幅字节像即判定
//! 成立，红蓝落点+绿锚全部写进账注——判定依据可审计不口头。
//!
//! 16bpp 变体探测用位 15 探针（CPU 原始 u16 写 0x8000，小端落盘字节
//! 序确定，无需显示通路配合）：读回纯黑 → 位 15 未用 → RGB555；读回
//! 红主导（R=132/255 整数定点展开）→ RGB565。红蓝序次核：原始
//! 0xF800/0x07E0/0x001F 读回主导通道核对。
//!
//! ## 要点三：账本守恒与重判定
//!
//! 判定历史逐条登记（序号/逻辑 tick/判定/依据账注/探针摘要），重判定
//! 结论必须与前判一致——不一致显性立案（REJUDGE_MISMATCH），原判定
//! 依旧站得住不改写历史。守恒检查：序号连续、tick 单调不降、账注
//! 非空、相态与末条记录双向对拍。
//!
//! ## 要点四：零 panic 面 + 诊断码独占 0x96xx 段
//!
//! 与 F0261 像素通道语义册（0x3a00 段）/F0481 帧缓冲台账（0x8400 段）
//! 等互不重叠；本模块零 panic 面——帧缓冲字节是观察值不是可信值，
//! 所有切片越界/变体不可判/坏帧形态均走诊断码闭集（0x9600..0x960F）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码闭集（独占 0x96xx 段）
// ---------------------------------------------------------------------------

/// 像素格式判定诊断码。独占 `0x96xx` 段（与 F0261 的 0x3a00 / F0481 的
/// 0x8400 段互不重叠——grep 同目录核实后占段）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PfCode(pub u16);

impl PfCode {
    /// 0x9600 帧缓冲空（零长度，观察面不存在）。
    pub const FB_EMPTY: PfCode = PfCode(0x9600);
    /// 0x9601 坏帧显性立案：全 0xFF（面板未点亮/通路悬空形态）。
    pub const BAD_FRAME_FF: PfCode = PfCode(0x9601);
    /// 0x9602 坏帧显性立案：全 0x00（通路接地/清屏未回写形态）。
    pub const BAD_FRAME_00: PfCode = PfCode(0x9602);
    /// 0x9603 声明位深不在四分发闭集（非 32/24/16）。
    pub const BITDEPTH_BAD: PfCode = PfCode(0x9603);
    /// 0x9604 抽样比对失配（无假设字节像与观察面相符——红蓝序核对失败）。
    pub const PROBE_MISMATCH: PfCode = PfCode(0x9604);
    /// 0x9605 数据面不足（探针区/转换源短于所需字节数）。
    pub const DATA_SHORT: PfCode = PfCode(0x9605);
    /// 0x9606 转换源目标同格式（转换矩阵非法参数——同格式不走矩阵）。
    pub const CONVERT_SAME: PfCode = PfCode(0x9606);
    /// 0x9607 位掩码/位移不一致（描述符自审失败——掩码不连续或位移错位）。
    pub const MASK_BAD: PfCode = PfCode(0x9607);
    /// 0x9608 行字节数溢出（宽度乘字节深越界/零行）。
    pub const STRIDE_OVERFLOW: PfCode = PfCode(0x9608);
    /// 0x9609 对齐参数非法（零对齐/非二次幂——钳制前就非法）。
    pub const ALIGN_BAD: PfCode = PfCode(0x9609);
    /// 0x960A 重判定不一致（与前判相左——原判定依旧站得住）。
    pub const REJUDGE_MISMATCH: PfCode = PfCode(0x960A);
    /// 0x960B 账本不守恒（序号/tick/账注/相态守恒恒等式破缺）。
    pub const LEDGER_BROKEN: PfCode = PfCode(0x960B);
    /// 0x960C 状态机相态违例（在错误相态调用了入口）。
    pub const PHASE_BAD: PfCode = PfCode(0x960C);
    /// 0x960D 16bpp 变体不可判（位 15 探针读回形态既非纯黑也非红主导）。
    pub const VARIANT_UNDECIDED: PfCode = PfCode(0x960D);
    /// 0x960E 清单比对失配（两份导出清单逐行对拍出现分歧）。
    pub const MANIFEST_MISMATCH: PfCode = PfCode(0x960E);
    /// 0x960F 段位守卫（诊断码越出本模块 0x96xx 段）。
    pub const SEG_GUARD: PfCode = PfCode(0x960F);

    /// wire 码。
    pub const fn code(self) -> u16 {
        self.0
    }

    /// 人话原因。
    pub fn reason(self) -> String {
        match self {
            PfCode::FB_EMPTY => "帧缓冲空：零长度观察面，抽样无从谈起".into(),
            PfCode::BAD_FRAME_FF => "坏帧立案：全 0xFF——面板未点亮或通路悬空形态".into(),
            PfCode::BAD_FRAME_00 => "坏帧立案：全 0x00——通路接地或清屏未回写形态".into(),
            PfCode::BITDEPTH_BAD => "声明位深不在四分发闭集：只认 32/24/16 三档".into(),
            PfCode::PROBE_MISMATCH => "抽样比对失配：无假设字节像与观察面相符，红蓝序核对失败".into(),
            PfCode::DATA_SHORT => "数据面不足：探针区或转换源短于所需字节数".into(),
            PfCode::CONVERT_SAME => "转换源目标同格式：同格式不走转换矩阵".into(),
            PfCode::MASK_BAD => "位掩码/位移不一致：描述符自审失败".into(),
            PfCode::STRIDE_OVERFLOW => "行字节数溢出：宽度乘字节深越界或零行".into(),
            PfCode::ALIGN_BAD => "对齐参数非法：零对齐或非二次幂".into(),
            PfCode::REJUDGE_MISMATCH => "重判定不一致：与前判相左，原判定依旧站得住不改写历史".into(),
            PfCode::LEDGER_BROKEN => "账本不守恒：序号/tick/账注/相态恒等式破缺".into(),
            PfCode::PHASE_BAD => "状态机相态违例：在错误相态调用了入口".into(),
            PfCode::VARIANT_UNDECIDED => "16bpp 变体不可判：位 15 探针读回形态无法区分 565/555".into(),
            PfCode::MANIFEST_MISMATCH => "清单比对失配：两份导出清单逐行对拍出现分歧".into(),
            PfCode::SEG_GUARD => "段位守卫：诊断码越出本模块 0x96xx 段".into(),
            PfCode(_) => "未知 UNX 像素格式判定域诊断码".into(),
        }
    }
}

/// 本模块诊断段基址。
pub const PF482_SEG_BASE: u16 = 0x9600;
/// 本模块诊断段顶（含）。
pub const PF482_SEG_TOP: u16 = 0x960F;

/// 段位守卫：全部诊断码必须落在本段 [0x9600, 0x960F]。
pub fn seg_guard(c: PfCode) -> bool {
    let v = c.code();
    v >= PF482_SEG_BASE && v <= PF482_SEG_TOP
}

/// 诊断码闭集全列（16 码——闭集显性，占段即全量）。
pub const ALL_CODES: [PfCode; 16] = [
    PfCode::FB_EMPTY,
    PfCode::BAD_FRAME_FF,
    PfCode::BAD_FRAME_00,
    PfCode::BITDEPTH_BAD,
    PfCode::PROBE_MISMATCH,
    PfCode::DATA_SHORT,
    PfCode::CONVERT_SAME,
    PfCode::MASK_BAD,
    PfCode::STRIDE_OVERFLOW,
    PfCode::ALIGN_BAD,
    PfCode::REJUDGE_MISMATCH,
    PfCode::LEDGER_BROKEN,
    PfCode::PHASE_BAD,
    PfCode::VARIANT_UNDECIDED,
    PfCode::MANIFEST_MISMATCH,
    PfCode::SEG_GUARD,
];

/// 诊断码闭集自审：全列皆在段内且人话原因非空（闭集完备性对拍锚）。
pub fn codes_audit() -> Result<(), PfCode> {
    let mut i = 0usize;
    while i < ALL_CODES.len() {
        let c = ALL_CODES[i];
        if !seg_guard(c) {
            return Err(PfCode::SEG_GUARD);
        }
        if c.reason().is_empty() {
            return Err(PfCode::SEG_GUARD);
        }
        i += 1;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 二、像素格式种类（四分发槽位）
// ---------------------------------------------------------------------------

/// 像素格式种类（判定产物）。四分发四个槽位、五种具体格式——16bpp
/// 槽位下 565/555 两变体由位 15 探针细分。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixFmtKind {
    /// 32bpp BGRX：内存字节序 B,G,R,X（小端 u32 下 R 在位 16..23）。
    Bgrx32,
    /// 32bpp XRGB：内存字节序 X,R,G,B（小端 u32 下 R 在位 8..15）。
    Xrgb32,
    /// 24bpp BGR：内存字节序 B,G,R（无保留字节）。
    Bgr24,
    /// 16bpp RGB565：R5/G6/B5，位 15..11/10..5/4..0。
    Rgb565,
    /// 16bpp RGB555：R5/G5/B5，位 14..10/9..5/4..0，位 15 未用。
    Rgb555,
    /// 表外格式（四分发不认——诊断立案占位）。
    Unknown,
}

impl PixFmtKind {
    /// 五种具体格式全列（矩阵/清单遍历序——枚举序即矩阵序）。
    pub const CONCRETE_KINDS: [PixFmtKind; 5] = [
        PixFmtKind::Bgrx32,
        PixFmtKind::Xrgb32,
        PixFmtKind::Bgr24,
        PixFmtKind::Rgb565,
        PixFmtKind::Rgb555,
    ];

    /// 格式名（快照/渲染单点）。
    pub const fn name(self) -> &'static str {
        match self {
            PixFmtKind::Bgrx32 => "BGRX32",
            PixFmtKind::Xrgb32 => "XRGB32",
            PixFmtKind::Bgr24 => "BGR24",
            PixFmtKind::Rgb565 => "RGB565",
            PixFmtKind::Rgb555 => "RGB555",
            PixFmtKind::Unknown => "UNKNOWN",
        }
    }

    /// 位深（Unknown 为 0——表外无位深）。
    pub const fn bits_per_pixel(self) -> u16 {
        match self {
            PixFmtKind::Bgrx32 => 32,
            PixFmtKind::Xrgb32 => 32,
            PixFmtKind::Bgr24 => 24,
            PixFmtKind::Rgb565 => 16,
            PixFmtKind::Rgb555 => 16,
            PixFmtKind::Unknown => 0,
        }
    }

    /// 每像素字节数（Unknown 为 0）。
    pub const fn bytes_per_pixel(self) -> u16 {
        match self {
            PixFmtKind::Bgrx32 => 4,
            PixFmtKind::Xrgb32 => 4,
            PixFmtKind::Bgr24 => 3,
            PixFmtKind::Rgb565 => 2,
            PixFmtKind::Rgb555 => 2,
            PixFmtKind::Unknown => 0,
        }
    }

    /// 四分发槽位号：0=BGRX32 / 1=XRGB32 / 2=BGR24 / 3=16bpp 族 /
    /// 4=表外（不属四分发任何槽）。
    pub const fn four_way_slot(self) -> u8 {
        match self {
            PixFmtKind::Bgrx32 => 0,
            PixFmtKind::Xrgb32 => 1,
            PixFmtKind::Bgr24 => 2,
            PixFmtKind::Rgb565 | PixFmtKind::Rgb555 => 3,
            PixFmtKind::Unknown => 4,
        }
    }

    /// 四分发槽位名（判据原文对拍面）。
    pub fn four_way_slot_name(self) -> &'static str {
        match self.four_way_slot() {
            0 => "32bpp-BGRX",
            1 => "32bpp-XRGB",
            2 => "24bpp",
            3 => "16bpp",
            _ => "表外",
        }
    }

    /// 矩阵下标（CONCRETE_KINDS 序；Unknown 无下标）。
    pub const fn matrix_index(self) -> Option<usize> {
        match self {
            PixFmtKind::Bgrx32 => Some(0),
            PixFmtKind::Xrgb32 => Some(1),
            PixFmtKind::Bgr24 => Some(2),
            PixFmtKind::Rgb565 => Some(3),
            PixFmtKind::Rgb555 => Some(4),
            PixFmtKind::Unknown => None,
        }
    }
}

/// 四分发槽位名全列（判据原文「32bpp: BGRX/XRGB/24bpp/16bpp」逐字对拍）。
pub const FOUR_WAY_SLOTS: [&str; 4] = ["32bpp-BGRX", "32bpp-XRGB", "24bpp", "16bpp"];

// ---------------------------------------------------------------------------
// 三、像素格式描述符（位深/掩码/位移/字节序）
// ---------------------------------------------------------------------------

/// 像素格式描述符：通道掩码+位移+字节序——判定成立后的一切换算
/// （编码/解码/转换/位型）都从描述符出发，不散落魔数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FmtDesc {
    /// 格式种类。
    pub kind: PixFmtKind,
    /// 位深。
    pub bpp: u16,
    /// 每像素字节数。
    pub bytes_pp: u16,
    /// 红通道掩码（在像素原语 u32 内）。
    pub r_mask: u32,
    /// 绿通道掩码。
    pub g_mask: u32,
    /// 蓝通道掩码。
    pub b_mask: u32,
    /// 红通道位移（掩码最低置位位）。
    pub r_shift: u8,
    /// 绿通道位移。
    pub g_shift: u8,
    /// 蓝通道位移。
    pub b_shift: u8,
    /// 保留/透明通道掩码（X 或 A——BGRX 高字节、XRGB 低字节、555 位 15）。
    pub aux_mask: u32,
    /// 内存字节序名（账注用）。
    pub byte_order: &'static str,
}

/// 掩码最低置位位（None = 掩码为零）。
pub fn mask_lowest_shift(mask: u32) -> Option<u8> {
    if mask == 0 {
        return None;
    }
    let s = mask.trailing_zeros();
    if s > 31 {
        return None;
    }
    Some(s as u8)
}

/// 掩码连续性：从最低置位位起连续 N 位（判定描述符合法性的硬条件
/// ——跳位掩码在本域四格式中不存在，出现即描述符被改坏）。
pub fn mask_contiguous(mask: u32) -> bool {
    if mask == 0 {
        return false;
    }
    let s = mask.trailing_zeros();
    let m = mask >> s;
    // m 应为 2^k - 1 形：m & (m+1) == 0。
    m.wrapping_add(1) & m == 0
}

/// 五种具体格式的描述符（唯一事实源——所有换算从此出发）。
pub const fn descriptor(kind: PixFmtKind) -> FmtDesc {
    match kind {
        PixFmtKind::Bgrx32 => FmtDesc {
            kind,
            bpp: 32,
            bytes_pp: 4,
            r_mask: 0x00FF_0000,
            g_mask: 0x0000_FF00,
            b_mask: 0x0000_00FF,
            r_shift: 16,
            g_shift: 8,
            b_shift: 0,
            aux_mask: 0xFF00_0000,
            byte_order: "B,G,R,X（小端字节序）",
        },
        PixFmtKind::Xrgb32 => FmtDesc {
            kind,
            bpp: 32,
            bytes_pp: 4,
            r_mask: 0x0000_FF00,
            g_mask: 0x00FF_0000,
            b_mask: 0xFF00_0000,
            r_shift: 8,
            g_shift: 16,
            b_shift: 24,
            aux_mask: 0x0000_00FF,
            byte_order: "X,R,G,B（小端字节序）",
        },
        PixFmtKind::Bgr24 => FmtDesc {
            kind,
            bpp: 24,
            bytes_pp: 3,
            r_mask: 0x00FF_0000,
            g_mask: 0x0000_FF00,
            b_mask: 0x0000_00FF,
            r_shift: 16,
            g_shift: 8,
            b_shift: 0,
            aux_mask: 0,
            byte_order: "B,G,R（字节序）",
        },
        PixFmtKind::Rgb565 => FmtDesc {
            kind,
            bpp: 16,
            bytes_pp: 2,
            r_mask: 0xF800,
            g_mask: 0x07E0,
            b_mask: 0x001F,
            r_shift: 11,
            g_shift: 5,
            b_shift: 0,
            aux_mask: 0,
            byte_order: "小端 u16",
        },
        PixFmtKind::Rgb555 => FmtDesc {
            kind,
            bpp: 16,
            bytes_pp: 2,
            r_mask: 0x7C00,
            g_mask: 0x03E0,
            b_mask: 0x001F,
            r_shift: 10,
            g_shift: 5,
            b_shift: 0,
            aux_mask: 0x8000,
            byte_order: "小端 u16",
        },
        PixFmtKind::Unknown => FmtDesc {
            kind,
            bpp: 0,
            bytes_pp: 0,
            r_mask: 0,
            g_mask: 0,
            b_mask: 0,
            r_shift: 0,
            g_shift: 0,
            b_shift: 0,
            aux_mask: 0,
            byte_order: "未声明",
        },
    }
}

/// 描述符自审：位深与字节数一致、通道掩码连续且位移等于最低置位位、
/// 通道互不重叠、掩码并集恰好覆盖位深全域（保留字节计入 aux）。
/// 表外格式（Unknown）不在自审面内——直接 MASK_BAD。
pub fn desc_audit(d: &FmtDesc) -> Result<(), PfCode> {
    if d.kind == PixFmtKind::Unknown {
        return Err(PfCode::MASK_BAD);
    }
    if d.bpp == 0 || d.bytes_pp == 0 {
        return Err(PfCode::MASK_BAD);
    }
    if d.bpp as u32 != d.bytes_pp as u32 * 8 {
        return Err(PfCode::MASK_BAD);
    }
    // 通道掩码连续性与位移对拍。
    let triples: [(u32, u8); 3] = [(d.r_mask, d.r_shift), (d.g_mask, d.g_shift), (d.b_mask, d.b_shift)];
    let mut k = 0usize;
    while k < triples.len() {
        let (mask, shift) = triples[k];
        if !mask_contiguous(mask) {
            return Err(PfCode::MASK_BAD);
        }
        match mask_lowest_shift(mask) {
            Some(s) => {
                if s != shift {
                    return Err(PfCode::MASK_BAD);
                }
            }
            None => return Err(PfCode::MASK_BAD),
        }
        k += 1;
    }
    // 互不重叠。
    if (d.r_mask & d.g_mask) != 0 || (d.r_mask & d.b_mask) != 0 || (d.g_mask & d.b_mask) != 0 {
        return Err(PfCode::MASK_BAD);
    }
    if (d.aux_mask & (d.r_mask | d.g_mask | d.b_mask)) != 0 {
        return Err(PfCode::MASK_BAD);
    }
    // 并集恰好覆盖位深全域（bpp=32 时全域为 u32::MAX，避免 1<<32 溢出）。
    let full: u32 = if d.bpp >= 32 { u32::MAX } else { (1u32 << d.bpp) - 1 };
    if (d.r_mask | d.g_mask | d.b_mask | d.aux_mask) != full {
        return Err(PfCode::MASK_BAD);
    }
    Ok(())
}

/// 五种具体格式描述符全量自审（一次跑遍——初始化期一次性对拍锚）。
pub fn all_desc_audit() -> Result<(), PfCode> {
    let mut i = 0usize;
    while i < PixFmtKind::CONCRETE_KINDS.len() {
        desc_audit(&descriptor(PixFmtKind::CONCRETE_KINDS[i]))?;
        i += 1;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 四、颜色常量与精确位型
// ---------------------------------------------------------------------------

/// 8 位 RGB 颜色值（通道语义层——与内存字节序无关的规范形态）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb8 {
    /// 红。
    pub r: u8,
    /// 绿。
    pub g: u8,
    /// 蓝。
    pub b: u8,
}

impl Rgb8 {
    /// 构造（const——探针协议常量表用）。
    pub const fn new(r: u8, g: u8, b: u8) -> Rgb8 {
        Rgb8 { r, g, b }
    }

    /// 名（五常量对拍面）。
    pub const fn name(self) -> &'static str {
        match self {
            Rgb8 { r: 0, g: 0, b: 0 } => "黑",
            Rgb8 { r: 255, g: 255, b: 255 } => "白",
            Rgb8 { r: 255, g: 0, b: 0 } => "红",
            Rgb8 { r: 0, g: 255, b: 0 } => "绿",
            Rgb8 { r: 0, g: 0, b: 255 } => "蓝",
            _ => "杂色",
        }
    }
}

/// 颜色五常量：黑/白/红/绿/蓝（通道语义层规范值）。
pub const COLOR_BLACK: Rgb8 = Rgb8::new(0, 0, 0);
/// 白。
pub const COLOR_WHITE: Rgb8 = Rgb8::new(255, 255, 255);
/// 红（红蓝序抽样核的正主角之一）。
pub const COLOR_RED: Rgb8 = Rgb8::new(255, 0, 0);
/// 绿（红蓝序抽样核的锚）。
pub const COLOR_GREEN: Rgb8 = Rgb8::new(0, 255, 0);
/// 蓝（红蓝序抽样核的正主角之二）。
pub const COLOR_BLUE: Rgb8 = Rgb8::new(0, 0, 255);

/// 探针协议通道序：红→绿→蓝（红蓝定夺、绿做锚——判据账注的依据序）。
pub const PROBE_SEQ: [Rgb8; 3] = [COLOR_RED, COLOR_GREEN, COLOR_BLUE];

/// 8 位 → N 位量化下行（整数定点：四舍五入 (v*max+127)/255；无浮点）。
pub fn quant_down(v8: u32, bits: u32) -> u32 {
    let max: u32 = (1u32 << bits) - 1;
    (v8 * max + 127) / 255
}

/// N 位 → 8 位量化上行（整数定点：半 LSB 补偿 (v*255+max/2)/max）。
pub fn quant_up(v: u32, bits: u32) -> u32 {
    let max: u32 = (1u32 << bits) - 1;
    (v * 255 + max / 2) / max
}

/// RGB565 打包（R5/G6/B5——位移取自描述符，不散魔数）。
pub fn pack565(c: Rgb8) -> u32 {
    let r = quant_down(c.r as u32, 5);
    let g = quant_down(c.g as u32, 6);
    let b = quant_down(c.b as u32, 5);
    (r << 11) | (g << 5) | b
}

/// RGB555 打包（R5/G5/B5，位 15 恒零）。
pub fn pack555(c: Rgb8) -> u32 {
    let r = quant_down(c.r as u32, 5);
    let g = quant_down(c.g as u32, 5);
    let b = quant_down(c.b as u32, 5);
    (r << 10) | (g << 5) | b
}

/// 颜色 → 像素原语位型（掩码/位移从描述符出发；Unknown → MASK_BAD）。
pub fn pixel_bits(kind: PixFmtKind, c: Rgb8) -> Result<u32, PfCode> {
    let d = descriptor(kind);
    if d.kind == PixFmtKind::Unknown {
        return Err(PfCode::MASK_BAD);
    }
    let bits: u32 = match kind {
        PixFmtKind::Rgb565 => pack565(c),
        PixFmtKind::Rgb555 => pack555(c),
        // 32/24 位族按掩码位移合成（BGRX/XRGB/BGR24 各归其位）。
        _ => {
            let r = ((c.r as u32) << d.r_shift) & d.r_mask;
            let g = ((c.g as u32) << d.g_shift) & d.g_mask;
            let b = ((c.b as u32) << d.b_shift) & d.b_mask;
            r | g | b
        }
    };
    Ok(bits)
}

/// 颜色 → 内存字节像（编码单点——写侧探针/转换矩阵共用）。
/// 尾部不足 4 字节的槽位补零（调用方按 bytes_per_pixel 截取）。
pub fn encode_pixel(kind: PixFmtKind, c: Rgb8) -> [u8; 4] {
    match kind {
        PixFmtKind::Bgrx32 => [c.b, c.g, c.r, 0],
        PixFmtKind::Xrgb32 => [0, c.r, c.g, c.b],
        PixFmtKind::Bgr24 => [c.b, c.g, c.r, 0],
        PixFmtKind::Rgb565 => {
            let v = pack565(c);
            [(v & 0xFF) as u8, ((v >> 8) & 0xFF) as u8, 0, 0]
        }
        PixFmtKind::Rgb555 => {
            let v = pack555(c);
            [(v & 0xFF) as u8, ((v >> 8) & 0xFF) as u8, 0, 0]
        }
        PixFmtKind::Unknown => [0, 0, 0, 0],
    }
}

/// 内存字节像 → 颜色（解码单点——观察面分类共用）。数据不足或表外
/// 格式 → None（不猜）。
pub fn decode_pixel(kind: PixFmtKind, bytes: &[u8]) -> Option<Rgb8> {
    let need = kind.bytes_per_pixel() as usize;
    if need == 0 || bytes.len() < need {
        return None;
    }
    match kind {
        PixFmtKind::Bgrx32 => Some(Rgb8::new(bytes[2], bytes[1], bytes[0])),
        PixFmtKind::Xrgb32 => Some(Rgb8::new(bytes[1], bytes[2], bytes[3])),
        PixFmtKind::Bgr24 => Some(Rgb8::new(bytes[2], bytes[1], bytes[0])),
        PixFmtKind::Rgb565 => {
            let v = u16::from_le_bytes([bytes[0], bytes[1]]) as u32;
            let r = quant_up((v >> 11) & 0x1F, 5);
            let g = quant_up((v >> 5) & 0x3F, 6);
            let b = quant_up(v & 0x1F, 5);
            Some(Rgb8::new(r as u8, g as u8, b as u8))
        }
        PixFmtKind::Rgb555 => {
            let v = u16::from_le_bytes([bytes[0], bytes[1]]) as u32;
            let r = quant_up((v >> 10) & 0x1F, 5);
            let g = quant_up((v >> 5) & 0x1F, 5);
            let b = quant_up(v & 0x1F, 5);
            Some(Rgb8::new(r as u8, g as u8, b as u8))
        }
        PixFmtKind::Unknown => None,
    }
}

/// 五常量在指定格式下的精确位型表（黑/白/红/绿/蓝逐项——账注引用面）。
/// Unknown → MASK_BAD。
pub fn color_bit_patterns(kind: PixFmtKind) -> Result<Vec<(String, u32)>, PfCode> {
    let colors: [(&str, Rgb8); 5] = [
        ("黑", COLOR_BLACK),
        ("白", COLOR_WHITE),
        ("红", COLOR_RED),
        ("绿", COLOR_GREEN),
        ("蓝", COLOR_BLUE),
    ];
    let mut out: Vec<(String, u32)> = Vec::new();
    let mut i = 0usize;
    while i < colors.len() {
        let (name, c) = colors[i];
        let bits = pixel_bits(kind, c)?;
        out.push((format!("{}({})", name, kind.name()), bits));
        i += 1;
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// 五、红蓝序实测抽样核（判定依据账注的证据面）
// ---------------------------------------------------------------------------

/// 32bpp 探针所需字节数（3 像素 × 4 字节：红/绿/蓝）。
pub const PROBE_32_BYTES: usize = 12;
/// 24bpp 探针所需字节数（3 像素 × 3 字节）。
pub const PROBE_24_BYTES: usize = 9;
/// 16bpp 探针所需字节数（4 像素 × 2 字节：位 15 探针+红/绿/蓝次核）。
pub const PROBE_16_BYTES: usize = 8;

/// 16bpp 位 15 探针原始写值（CPU 原始 u16 写——小端落盘字节序确定，
/// 无需显示通路配合，是 565/555 细分的裁决写）。
pub const PROBE_16_RAW_BIT15: u16 = 0x8000;
/// 16bpp 次核原始写值：红（565 规范 R5=31）。
pub const PROBE_16_RAW_RED: u16 = 0xF800;
/// 16bpp 次核原始写值：绿（565 规范 G6=63）。
pub const PROBE_16_RAW_GREEN: u16 = 0x07E0;
/// 16bpp 次核原始写值：蓝（565/555 通用 B5=31）。
pub const PROBE_16_RAW_BLUE: u16 = 0x001F;

/// 16bpp 探针写侧字节像（四像素原始写依次 [位15探针, 红, 绿, 蓝] 的
/// 小端落盘——调用方按序原始写入后，观察面交给 verdict_16 分类）。
pub const PROBE_16_WRITE: [u8; 8] = [0x00, 0x80, 0x00, 0xF8, 0xE0, 0x07, 0x1F, 0x00];

/// 抽样裁决产物：命中（格式+依据账注）/ 失配（账注）/ 变体不可判
/// （账注）/ 数据面不足（需/实）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeOutcome {
    /// 命中：格式 + 判定依据账注（红蓝落点实测记录）。
    Match(PixFmtKind, String),
    /// 失配：无假设字节像与观察面相符（账注记观察形态）。
    Mismatch(String),
    /// 16bpp 变体不可判：位 15 探针读回形态无法区分 565/555。
    Undecided(String),
    /// 数据面不足：需要字节数 / 实有字节数。
    Short(usize, usize),
}

/// 探针摘要（FNV-1a 32——观察面前 64 字节确定性指纹，账本引用面）。
pub fn probe_digest(fb: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let n = if fb.len() < 64 { fb.len() } else { 64 };
    let mut i = 0usize;
    while i < n {
        h ^= fb[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

/// 指定格式下探针协议的期望字节像（红/绿/蓝三像素编码拼接——假设
/// 预测面，与观察面逐字节对拍即「实测抽样核」）。
pub fn expected_probe_image(kind: PixFmtKind) -> Option<Vec<u8>> {
    match kind {
        PixFmtKind::Bgrx32 | PixFmtKind::Xrgb32 | PixFmtKind::Bgr24 => {
            let mut out = Vec::new();
            let mut i = 0usize;
            while i < PROBE_SEQ.len() {
                let enc = encode_pixel(kind, PROBE_SEQ[i]);
                let n = kind.bytes_per_pixel() as usize;
                let mut k = 0usize;
                while k < n {
                    out.push(enc[k]);
                    k += 1;
                }
                i += 1;
            }
            Some(out)
        }
        // 16bpp 族：写侧是 CPU 原始 u16 写，字节像与变体无关。
        PixFmtKind::Rgb565 | PixFmtKind::Rgb555 => Some(PROBE_16_WRITE.to_vec()),
        PixFmtKind::Unknown => None,
    }
}

/// 观察面与期望像逐字节对拍（实长恰好等于所需字节数为前提——调用方
/// 先行长度闸）。
fn image_matches(kind: PixFmtKind, observed: &[u8]) -> bool {
    match expected_probe_image(kind) {
        Some(expect) => {
            if expect.len() != observed.len() {
                return false;
            }
            let mut i = 0usize;
            while i < observed.len() {
                if expect[i] != observed[i] {
                    return false;
                }
                i += 1;
            }
            true
        }
        None => false,
    }
}

/// 某像素槽内 0xFF 落点全列（红蓝落点实测记录——账注证据）。
fn ff_positions(px: &[u8]) -> Vec<usize> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < px.len() {
        if px[i] == 0xFF {
            out.push(i);
        }
        i += 1;
    }
    out
}

/// 落点列渲染成账注片段（空列记「无」）。
fn ff_note(pos: &[usize]) -> String {
    if pos.is_empty() {
        return "无".to_string();
    }
    let mut s = String::new();
    let mut i = 0usize;
    while i < pos.len() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&format!("{}", pos[i]));
        i += 1;
    }
    s
}

/// 32bpp 双假设裁决：BGRX 与 XRGB 的期望字节像逐一与观察面对拍，
/// 恰一命中即判（红蓝落点+绿锚写入账注）。双假设皆不中 → 失配。
pub fn verdict_32(fb: &[u8]) -> ProbeOutcome {
    if fb.len() < PROBE_32_BYTES {
        return ProbeOutcome::Short(PROBE_32_BYTES, fb.len());
    }
    let obs = &fb[..PROBE_32_BYTES];
    // 恰一命中语义：先数命中数，再取命中者——防多命中静默取先。
    let mut hits: Vec<PixFmtKind> = Vec::new();
    let mut i = 0usize;
    while i < PixFmtKind::CONCRETE_KINDS.len() {
        let k = PixFmtKind::CONCRETE_KINDS[i];
        if k.bits_per_pixel() == 32 && image_matches(k, obs) {
            hits.push(k);
        }
        i += 1;
    }
    if hits.len() == 1 {
        let k = hits[0];
        let red_ff = ff_positions(&obs[0..4]);
        let green_ff = ff_positions(&obs[4..8]);
        let blue_ff = ff_positions(&obs[8..12]);
        let note = format!(
            "红蓝序实测抽样核：纯红 0xFF 落点字节[{}]、纯绿锚落点字节[{}]、纯蓝 0xFF 落点字节[{}]——整幅字节像与 {} 假设逐字节相符（{}），判定依据落账",
            ff_note(&red_ff),
            ff_note(&green_ff),
            ff_note(&blue_ff),
            k.name(),
            descriptor(k).byte_order
        );
        return ProbeOutcome::Match(k, note);
    }
    if hits.len() > 1 {
        return ProbeOutcome::Mismatch(
            "红蓝序抽样核异常：BGRX/XRGB 双假设同时命中（协议写侧被破坏）——失配立案".to_string(),
        );
    }
    ProbeOutcome::Mismatch(
        "红蓝序抽样核失配：观察面 12 字节与 BGRX/XRGB 两假设期望字节像均不相符".to_string(),
    )
}

/// 24bpp 核验：BGR 字节序期望像对拍（24bpp 无红蓝歧义度——歧义在
/// 「是不是 24bpp」，由声明位深与整幅字节像共同核）。
pub fn verdict_24(fb: &[u8]) -> ProbeOutcome {
    if fb.len() < PROBE_24_BYTES {
        return ProbeOutcome::Short(PROBE_24_BYTES, fb.len());
    }
    let obs = &fb[..PROBE_24_BYTES];
    if image_matches(PixFmtKind::Bgr24, obs) {
        let red_ff = ff_positions(&obs[0..3]);
        let green_ff = ff_positions(&obs[3..6]);
        let blue_ff = ff_positions(&obs[6..9]);
        let note = format!(
            "红蓝序实测抽样核（24bpp）：纯红 0xFF 落点字节[{}]、纯绿锚落点字节[{}]、纯蓝 0xFF 落点字节[{}]——整幅字节像与 BGR24 假设逐字节相符（B,G,R 字节序），判定依据落账",
            ff_note(&red_ff),
            ff_note(&green_ff),
            ff_note(&blue_ff)
        );
        return ProbeOutcome::Match(PixFmtKind::Bgr24, note);
    }
    // 反序观察（红 FF 在字节 0 / 蓝 FF 在字节 2）单独立账注便于人审。
    if obs[0] == 0xFF && obs[1] == 0x00 && obs[2] == 0x00 && obs[6] == 0x00 && obs[7] == 0x00 && obs[8] == 0xFF {
        return ProbeOutcome::Mismatch(
            "红蓝序抽样核失配（24bpp）：观察面呈 RGB 反序（红 FF 在字节 0、蓝 FF 在字节 2）——与四分发 24bpp BGR 假设不符".to_string(),
        );
    }
    ProbeOutcome::Mismatch("红蓝序抽样核失配（24bpp）：观察面 9 字节与 BGR24 假设期望字节像不符".to_string())
}

/// 主导通道核对：解码色恰为「单通道非零、余通道为零」的单色形态。
fn dominant_is(c: Option<Rgb8>, chan: u8) -> bool {
    match c {
        Some(v) => match chan {
            0 => v.r > 0 && v.g == 0 && v.b == 0,
            1 => v.g > 0 && v.r == 0 && v.b == 0,
            2 => v.b > 0 && v.r == 0 && v.g == 0,
            _ => false,
        },
        None => false,
    }
}

/// 16bpp 变体探测（565/555 细分）：位 15 探针裁决 + 红蓝序次核。
///
/// 裁决逻辑：观察面像素 0 必须是原始 u16 写 0x8000 的小端落盘字节像
/// [00,80]（CPU 原始写字节序确定——写侧协议闸）。解码对拍：
/// RGB555 假设下位 15 未用 → 纯黑；RGB565 假设下位 15 属 R5 → 红主导
/// （R=132 整数定点展开）。恰一成立即判；两不成立 → 变体不可判。
pub fn verdict_16(fb: &[u8]) -> ProbeOutcome {
    if fb.len() < PROBE_16_BYTES {
        return ProbeOutcome::Short(PROBE_16_BYTES, fb.len());
    }
    if fb[0] != PROBE_16_WRITE[0] || fb[1] != PROBE_16_WRITE[1] {
        return ProbeOutcome::Mismatch(
            "位 15 探针协议闸失配：像素 0 观察字节非原始写 0x8000 的小端落盘像 [00,80]".to_string(),
        );
    }
    let d565 = decode_pixel(PixFmtKind::Rgb565, &fb[0..2]);
    let d555 = decode_pixel(PixFmtKind::Rgb555, &fb[0..2]);
    let black555 = d555 == Some(COLOR_BLACK);
    let red565 = dominant_is(d565, 0);
    let kind = if black555 && !red565 {
        PixFmtKind::Rgb555
    } else if red565 && !black555 {
        PixFmtKind::Rgb565
    } else {
        return ProbeOutcome::Undecided(
            "位 15 探针读回形态既非纯黑（555 预测）也非红主导（565 预测）——565/555 变体不可判".to_string(),
        );
    };
    // 红蓝序次核：像素 1/2/3 原始写 0xF800/0x07E0/0x001F 在两变体下
    // 均应读回红/绿/蓝主导单色（F800 在 555 下 R=247 仍红主导——
    // 主导核与变体无关，是红蓝序的跨变体稳定证据）。
    let mut k = 0usize;
    while k < 3 {
        let base = 2 + k * 2;
        let c = decode_pixel(kind, &fb[base..base + 2]);
        if !dominant_is(c, k as u8) {
            return ProbeOutcome::Mismatch(format!(
                "红蓝序次核失配（16bpp）：次核像素 {} 在 {} 假设下未读回主导单色",
                k,
                kind.name()
            ));
        }
        k += 1;
    }
    let verdict_note = if kind == PixFmtKind::Rgb555 {
        "位 15 写 0x8000 读回纯黑 → 位 15 未用 → RGB555".to_string()
    } else {
        format!(
            "位 15 写 0x8000 读回红主导（R={} 整数定点展开）→ 位 15 属红通道 → RGB565",
            match d565 {
                Some(c) => c.r,
                None => 0,
            }
        )
    };
    let note = format!(
        "16bpp 变体探测：{}；红蓝序次核 0xF800/0x07E0/0x001F 读回红/绿/蓝主导相符（{}），判定依据落账",
        verdict_note,
        descriptor(kind).byte_order
    );
    ProbeOutcome::Match(kind, note)
}

/// 四分发单点（无状态一次性判定——状态机复用同核）。
pub fn judge_once(claim_bpp: u32, fb: &[u8], tick: u64) -> Result<(PixFmtKind, String), PfCode> {
    let _ = tick; // 一次性判定不带账本，tick 仅入参签名对齐（无墙钟纪律）
    let audit = bad_frame_audit(fb)?;
    if let Some(code) = bad_frame_case(&audit) {
        return Err(code);
    }
    let outcome = match claim_bpp {
        32 => verdict_32(fb),
        24 => verdict_24(fb),
        16 => verdict_16(fb),
        _ => return Err(PfCode::BITDEPTH_BAD),
    };
    match outcome {
        ProbeOutcome::Match(kind, note) => Ok((kind, note)),
        ProbeOutcome::Mismatch(_) => Err(PfCode::PROBE_MISMATCH),
        ProbeOutcome::Undecided(_) => Err(PfCode::VARIANT_UNDECIDED),
        ProbeOutcome::Short(_, _) => Err(PfCode::DATA_SHORT),
    }
}

// ---------------------------------------------------------------------------
// 六、行 Stride 计算（含对齐钳制）
// ---------------------------------------------------------------------------

/// 缺省对齐（调用方不声明时使用——8 字节对齐是帧缓冲通行值）。
pub const DEFAULT_ALIGN: u64 = 8;
/// 对齐钳制上限（超过即钳回——防调用方传天文数字拖爆行宽）。
pub const MAX_ALIGN: u64 = 256;
/// 单行字节数硬上限（u64 域内的防御闸——真实帧缓冲远小于此）。
pub const MAX_ROW_BYTES: u64 = 1 << 40;

/// Stride 计算账（原始行字节/对齐后 stride/填充字节/有效对齐/是否被钳）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StrideInfo {
    /// 原始行字节数（宽 × 每像素字节）。
    pub raw_row_bytes: u64,
    /// 对齐后行距（stride）。
    pub stride: u64,
    /// 行尾填充字节数（stride - raw）。
    pub padding: u64,
    /// 有效对齐（钳制后）。
    pub align_effective: u64,
    /// 对齐是否被钳制（请求值超上限被钳回 MAX_ALIGN）。
    pub clamped: bool,
}

/// 行 stride 计算：raw = 宽 × 每像素字节；stride = raw 向上取整到
/// 有效对齐。对齐请求为 0 → 用缺省 8；非二次幂 → ALIGN_BAD；超上限
/// → 钳回 MAX_ALIGN 并如实标注 clamped（钳制不静默）。
pub fn stride_calc(kind: PixFmtKind, width: u64, align_req: u64) -> Result<StrideInfo, PfCode> {
    let bytes_pp = kind.bytes_per_pixel() as u64;
    if bytes_pp == 0 {
        return Err(PfCode::MASK_BAD);
    }
    if width == 0 {
        return Err(PfCode::STRIDE_OVERFLOW);
    }
    let raw = match width.checked_mul(bytes_pp) {
        Some(v) => v,
        None => return Err(PfCode::STRIDE_OVERFLOW),
    };
    if raw > MAX_ROW_BYTES {
        return Err(PfCode::STRIDE_OVERFLOW);
    }
    let (align_effective, clamped) = if align_req == 0 {
        (DEFAULT_ALIGN, false)
    } else if align_req > MAX_ALIGN {
        (MAX_ALIGN, true)
    } else {
        (align_req, false)
    };
    // 二次幂闸（零与非二次幂都非法——对齐语义要求掩码式取整）。
    if align_effective == 0 || (align_effective & (align_effective - 1)) != 0 {
        return Err(PfCode::ALIGN_BAD);
    }
    // 向上取整：raw ≤ 2^40、align ≤ 2^8，中间量不溢 u64。
    let stride = ((raw + align_effective - 1) / align_effective) * align_effective;
    if stride < raw {
        return Err(PfCode::STRIDE_OVERFLOW);
    }
    Ok(StrideInfo {
        raw_row_bytes: raw,
        stride,
        padding: stride - raw,
        align_effective,
        clamped,
    })
}

/// 整帧字节数（stride × 行数——帧长防越界账）。
pub fn frame_bytes(info: &StrideInfo, height: u64) -> Result<u64, PfCode> {
    if height == 0 {
        return Err(PfCode::STRIDE_OVERFLOW);
    }
    match info.stride.checked_mul(height) {
        Some(v) => {
            if v > MAX_ROW_BYTES * 1024 {
                return Err(PfCode::STRIDE_OVERFLOW);
            }
            Ok(v)
        }
        None => Err(PfCode::STRIDE_OVERFLOW),
    }
}

// ---------------------------------------------------------------------------
// 七、格式间转换矩阵（含 32↔24↔16 有损降级账注）
// ---------------------------------------------------------------------------

/// 矩阵值：同格式。
pub const CONV_SAME: u8 = 0;
/// 矩阵值：无损转换。
pub const CONV_LOSSLESS: u8 = 1;
/// 矩阵值：有损降级（通道位深收缩）。
pub const CONV_LOSSY: u8 = 2;

/// 转换矩阵（行=源、列=目标，序同 CONCRETE_KINDS）：
/// 32↔32、32↔24 全 8 位通道互转无损；32/24 → 16 族有损（8 位通道
/// 量化收缩到 5/6 位）；16 族 → 32/24 无损（上行展开）；565→555 有损
/// （绿 6→5 位收缩）；555→565 无损（绿 5→6 位上行）。
pub const CONVERT_MATRIX: [[u8; 5]; 5] = [
    // → BGRX32  XRGB32  BGR24   565     555
    [CONV_SAME, CONV_LOSSLESS, CONV_LOSSLESS, CONV_LOSSY, CONV_LOSSY], // BGRX32
    [CONV_LOSSLESS, CONV_SAME, CONV_LOSSLESS, CONV_LOSSY, CONV_LOSSY], // XRGB32
    [CONV_LOSSLESS, CONV_LOSSLESS, CONV_SAME, CONV_LOSSY, CONV_LOSSY], // BGR24
    [CONV_LOSSLESS, CONV_LOSSLESS, CONV_LOSSLESS, CONV_SAME, CONV_LOSSY], // RGB565
    [CONV_LOSSLESS, CONV_LOSSLESS, CONV_LOSSLESS, CONV_LOSSLESS, CONV_SAME], // RGB555
];

/// 转换类别查询（源/目标任一为表外格式 → MASK_BAD）。
pub fn conv_class(from: PixFmtKind, to: PixFmtKind) -> Result<u8, PfCode> {
    let fi = match from.matrix_index() {
        Some(i) => i,
        None => return Err(PfCode::MASK_BAD),
    };
    let ti = match to.matrix_index() {
        Some(i) => i,
        None => return Err(PfCode::MASK_BAD),
    };
    Ok(CONVERT_MATRIX[fi][ti])
}

/// 有损降级账注（转换依据可审计——逐对给出语义说明）。
pub fn conv_note(from: PixFmtKind, to: PixFmtKind) -> String {
    let class = match conv_class(from, to) {
        Ok(c) => c,
        Err(_) => return "转换涉及表外格式——矩阵无此边".to_string(),
    };
    if from == to {
        return format!("{}→{} 同格式：无需转换", from.name(), to.name());
    }
    match class {
        CONV_LOSSY => {
            if from == PixFmtKind::Rgb565 && to == PixFmtKind::Rgb555 {
                "RGB565→RGB555：绿通道 6 位→5 位量化收缩——有损降级账注".to_string()
            } else {
                format!(
                    "{}→{}：8 位通道量化收缩至 5/6 位——有损降级账注（下行量化 round-to-nearest，整数定点）",
                    from.name(),
                    to.name()
                )
            }
        }
        CONV_LOSSLESS => {
            if (from == PixFmtKind::Rgb565 || from == PixFmtKind::Rgb555)
                && (to.bits_per_pixel() == 32 || to.bits_per_pixel() == 24)
            {
                format!("{}→{}：5/6 位通道上行无损展开至 8 位", from.name(), to.name())
            } else if from == PixFmtKind::Rgb555 && to == PixFmtKind::Rgb565 {
                "RGB555→RGB565：绿通道 5 位→6 位上行无损展开".to_string()
            } else {
                format!("{}→{}：8 位通道全保留——无损转换（{} 字节序重排）", from.name(), to.name(), descriptor(from).byte_order)
            }
        }
        _ => format!("{}→{}：同格式", from.name(), to.name()),
    }
}

/// 单像素转换账（解码→再编码；目标字节像按目标每像素字节数截取）。
pub fn convert_pixel(from: PixFmtKind, to: PixFmtKind, src: &[u8]) -> Result<Vec<u8>, PfCode> {
    if from == to {
        return Err(PfCode::CONVERT_SAME);
    }
    let c = match decode_pixel(from, src) {
        Some(v) => v,
        None => return Err(PfCode::DATA_SHORT),
    };
    let enc = encode_pixel(to, c);
    let n = to.bytes_per_pixel() as usize;
    if n == 0 {
        return Err(PfCode::MASK_BAD);
    }
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < n {
        out.push(enc[i]);
        i += 1;
    }
    Ok(out)
}

/// 批量转换账（逐像素走矩阵；源数据面不足 → DATA_SHORT；同格式 →
/// CONVERT_SAME；账注随账返回——有损降级显性不静默）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConvertBill {
    /// 目标格式字节流（count × 目标每像素字节）。
    pub out: Vec<u8>,
    /// 转换类别（CONV_*）。
    pub class: u8,
    /// 转换账注（有损降级账注/无损说明）。
    pub note: String,
    /// 转换像素数。
    pub pixels: usize,
}

/// 批量转换单点。
pub fn convert_buffer(from: PixFmtKind, to: PixFmtKind, fb: &[u8], count: usize) -> Result<ConvertBill, PfCode> {
    if from == to {
        return Err(PfCode::CONVERT_SAME);
    }
    let fb_pp = from.bytes_per_pixel() as usize;
    let tg_pp = to.bytes_per_pixel() as usize;
    if fb_pp == 0 || tg_pp == 0 {
        return Err(PfCode::MASK_BAD);
    }
    let need = match count.checked_mul(fb_pp) {
        Some(v) => v,
        None => return Err(PfCode::DATA_SHORT),
    };
    if fb.len() < need {
        return Err(PfCode::DATA_SHORT);
    }
    let class = conv_class(from, to)?;
    let mut out: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < count {
        let chunk = &fb[i * fb_pp..i * fb_pp + fb_pp];
        let mut px = convert_pixel(from, to, chunk)?;
        let mut k = 0usize;
        while k < px.len() {
            out.push(px[k]);
            k += 1;
        }
        px.clear();
        i += 1;
    }
    Ok(ConvertBill {
        out,
        class,
        note: conv_note(from, to),
        pixels: count,
    })
}

// ---------------------------------------------------------------------------
// 八、坏帧缓冲处置（全 0xFF / 全 0x00 显性立案）
// ---------------------------------------------------------------------------

/// 坏帧形态审计账（观察面普查——全 FF/全 00/均匀色/首个分歧点）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BadFrameAudit {
    /// 观察面字节数。
    pub len: usize,
    /// 全 0xFF 形态。
    pub all_ff: bool,
    /// 全 0x00 形态。
    pub all_zero: bool,
    /// 均匀形态（全同字节——含 FF/00 两特例）。
    pub uniform: bool,
    /// 均匀形态下的字节值。
    pub uniform_byte: Option<u8>,
    /// 首个与首字节分歧的下标（均匀面为 None）。
    pub first_diverge: Option<usize>,
}

/// 坏帧审计单点（空面 → FB_EMPTY——观察面不存在是立案前置条件）。
pub fn bad_frame_audit(fb: &[u8]) -> Result<BadFrameAudit, PfCode> {
    if fb.is_empty() {
        return Err(PfCode::FB_EMPTY);
    }
    let first = fb[0];
    let mut all_same = true;
    let mut diverge: Option<usize> = None;
    let mut i = 1usize;
    while i < fb.len() {
        if fb[i] != first {
            all_same = false;
            diverge = Some(i);
            break;
        }
        i += 1;
    }
    Ok(BadFrameAudit {
        len: fb.len(),
        all_ff: all_same && first == 0xFF,
        all_zero: all_same && first == 0x00,
        uniform: all_same,
        uniform_byte: if all_same { Some(first) } else { None },
        first_diverge: diverge,
    })
}

/// 坏帧显性立案：全 FF / 全 00 两形态回诊断码（其余形态不立案——
/// 均匀杂色面仍可走抽样核，由红蓝序对拍自然裁决）。
pub fn bad_frame_case(a: &BadFrameAudit) -> Option<PfCode> {
    if a.all_ff {
        return Some(PfCode::BAD_FRAME_FF);
    }
    if a.all_zero {
        return Some(PfCode::BAD_FRAME_00);
    }
    None
}

/// 坏帧账注（立案/放行皆留痕——观察形态写进账）。
pub fn bad_frame_note(a: &BadFrameAudit) -> String {
    if a.all_ff {
        return format!("坏帧形态：全 0xFF（{} 字节）——面板未点亮/通路悬空形态，显性立案", a.len);
    }
    if a.all_zero {
        return format!("坏帧形态：全 0x00（{} 字节）——通路接地/清屏未回写形态，显性立案", a.len);
    }
    if a.uniform {
        return format!(
            "观察面均匀非坏帧：全 0x{:02X}（{} 字节）——放行抽样核",
            match a.uniform_byte {
                Some(v) => v,
                None => 0,
            },
            a.len
        );
    }
    format!(
        "观察面非均匀（{} 字节，首分歧于下标 {}）——放行抽样核",
        a.len,
        match a.first_diverge {
            Some(v) => v,
            None => 0,
        }
    )
}

// ---------------------------------------------------------------------------
// 九、判定状态机（四分发判定）
// ---------------------------------------------------------------------------

/// 状态机相态（Idle → Decided/Rejected；Decided 可进重判定）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JudgePhase {
    /// 起步：未判定。
    Idle,
    /// 判定成立：verdict 有值、末条记录无诊断。
    Decided,
    /// 判定拒绝：末条记录带诊断码。
    Rejected,
}

impl JudgePhase {
    /// 相态名（账注/清单渲染单点）。
    pub const fn name(self) -> &'static str {
        match self {
            JudgePhase::Idle => "idle",
            JudgePhase::Decided => "decided",
            JudgePhase::Rejected => "rejected",
        }
    }
}

/// 判定登记账条目（判定历史+依据+时间戳逻辑 tick——全部落账可审计）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JudgeRecord {
    /// 序号（从 1 起严格递增——守恒检查锚）。
    pub seq: u64,
    /// 逻辑 tick（调用方喂入——本模块无墙钟，tick 单调性由守恒检查把关）。
    pub tick: u64,
    /// 判定产物（拒绝条目为 None）。
    pub kind: Option<PixFmtKind>,
    /// 声明位深（判定入口的原始声明——留痕）。
    pub claim_bpp: u32,
    /// 观察面字节数。
    pub fb_len: usize,
    /// 观察面探针摘要（FNV-1a 前 64 字节——证据指纹）。
    pub probe_digest: u32,
    /// 判定依据账注（红蓝落点实测/失配形态/立案原因——非空守恒）。
    pub basis: String,
    /// 诊断码（拒绝/重判失配条目有值）。
    pub diag: Option<PfCode>,
}

/// 像素格式判定状态机（四分发判定落账全流程）。
#[derive(Debug, Clone)]
pub struct PixFmtJudge {
    phase: JudgePhase,
    verdict: Option<PixFmtKind>,
    diag: Option<PfCode>,
    claim_bpp: u32,
    fb_len: usize,
    begin_tick: u64,
    records: Vec<JudgeRecord>,
}

impl PixFmtJudge {
    /// 空状态机（Idle 相态起步）。
    pub fn new() -> PixFmtJudge {
        PixFmtJudge {
            phase: JudgePhase::Idle,
            verdict: None,
            diag: None,
            claim_bpp: 0,
            fb_len: 0,
            begin_tick: 0,
            records: Vec::new(),
        }
    }

    /// 相态只读。
    pub fn phase(&self) -> JudgePhase {
        self.phase
    }

    /// 判定产物只读。
    pub fn verdict(&self) -> Option<PixFmtKind> {
        self.verdict
    }

    /// 当前诊断只读。
    pub fn diag(&self) -> Option<PfCode> {
        self.diag
    }

    /// 声明位深只读。
    pub fn claim_bpp(&self) -> u32 {
        self.claim_bpp
    }

    /// 观察面字节数只读。
    pub fn fb_len(&self) -> usize {
        self.fb_len
    }

    /// 起判 tick 只读。
    pub fn begin_tick(&self) -> u64 {
        self.begin_tick
    }

    /// 登记账条数只读。
    pub fn record_count(&self) -> usize {
        self.records.len()
    }

    /// 登记账条目只读（越界 → None）。
    pub fn record_at(&self, idx: usize) -> Option<&JudgeRecord> {
        if idx < self.records.len() {
            Some(&self.records[idx])
        } else {
            None
        }
    }

    /// 诊断 wire 码渲染（无诊断记 "none"）。
    pub fn diag_wire(&self) -> String {
        match self.diag {
            Some(c) => format!("0x{:04X}", c.code()),
            None => "none".to_string(),
        }
    }

    /// 判定名渲染（未判定记 "n/a"）。
    pub fn verdict_name(&self) -> String {
        match self.verdict {
            Some(k) => k.name().to_string(),
            None => "n/a".to_string(),
        }
    }

    /// 登记账推入（seq 自动续——内部单点）。
    fn record_push(&mut self, tick: u64, kind: Option<PixFmtKind>, diag: Option<PfCode>, basis: String, digest: u32) {
        let seq = (self.records.len() as u64) + 1;
        self.records.push(JudgeRecord {
            seq,
            tick,
            kind,
            claim_bpp: self.claim_bpp,
            fb_len: self.fb_len,
            probe_digest: digest,
            basis,
            diag,
        });
    }

    /// 拒绝收尾（相态/诊断/账注三写——内部单点）。
    fn reject(&mut self, tick: u64, code: PfCode, basis: String, digest: u32) {
        self.phase = JudgePhase::Rejected;
        self.diag = Some(code);
        self.record_push(tick, None, Some(code), basis, digest);
    }

    /// 首判入口：坏帧处置 → 四分发 → 抽样核 → 落账。
    ///
    /// 规则：① 相态必须 Idle（违者 PHASE_BAD——重判走 re_judge）；
    /// ② 坏帧全 FF/全 00 显性立案（BAD_FRAME_FF/BAD_FRAME_00）不进
    /// 抽样核；③ 声明位深不在 {32,24,16} → BITDEPTH_BAD；④ 抽样核
    /// 命中 → Decided，红蓝落点账注入账；失配/不可判/面短 → Rejected。
    pub fn begin(&mut self, claim_bpp: u32, fb: &[u8], tick: u64) -> Result<PixFmtKind, PfCode> {
        if self.phase != JudgePhase::Idle {
            return Err(PfCode::PHASE_BAD);
        }
        self.claim_bpp = claim_bpp;
        self.fb_len = fb.len();
        self.begin_tick = tick;
        // 坏帧处置前置。
        match bad_frame_audit(fb) {
            Ok(a) => {
                if let Some(code) = bad_frame_case(&a) {
                    self.reject(tick, code, bad_frame_note(&a), probe_digest(fb));
                    return Err(code);
                }
            }
            Err(code) => {
                self.reject(tick, code, "帧缓冲空（零长度）：抽样无从谈起——显性立案".to_string(), 0);
                return Err(code);
            }
        }
        // 四分发。
        let outcome = match claim_bpp {
            32 => verdict_32(fb),
            24 => verdict_24(fb),
            16 => verdict_16(fb),
            _ => {
                self.reject(
                    tick,
                    PfCode::BITDEPTH_BAD,
                    format!("声明位深 {} 不在四分发闭集（32/24/16）——不猜不回退", claim_bpp),
                    probe_digest(fb),
                );
                return Err(PfCode::BITDEPTH_BAD);
            }
        };
        match outcome {
            ProbeOutcome::Match(kind, basis) => {
                self.verdict = Some(kind);
                self.phase = JudgePhase::Decided;
                self.diag = None;
                self.record_push(tick, Some(kind), None, basis, probe_digest(fb));
                Ok(kind)
            }
            ProbeOutcome::Mismatch(note) => {
                self.reject(tick, PfCode::PROBE_MISMATCH, note, probe_digest(fb));
                Err(PfCode::PROBE_MISMATCH)
            }
            ProbeOutcome::Undecided(note) => {
                self.reject(tick, PfCode::VARIANT_UNDECIDED, note, probe_digest(fb));
                Err(PfCode::VARIANT_UNDECIDED)
            }
            ProbeOutcome::Short(need, have) => {
                self.reject(
                    tick,
                    PfCode::DATA_SHORT,
                    format!("探针区数据面不足：需要 {} 字节实有 {} 字节", need, have),
                    probe_digest(fb),
                );
                Err(PfCode::DATA_SHORT)
            }
        }
    }

    /// 重判定入口：仅 Decided 相态可进；新观察面重走抽样核，结论必须
    /// 与前判一致。
    ///
    /// 规则：① 相态非 Decided → PHASE_BAD；② 一致 → 新账注入账（含
    /// 「重判定一致」标记），原判定依旧；③ 不一致 → REJUDGE_MISMATCH
    /// 立案，账注记双侧结论，原判定不改写（历史站得住）；④ 重判抽样
    /// 核失败（失配/不可判/面短/坏帧）→ 对应诊断立案，原判定不改写。
    pub fn re_judge(&mut self, claim_bpp: u32, fb: &[u8], tick: u64) -> Result<PixFmtKind, PfCode> {
        if self.phase != JudgePhase::Decided {
            return Err(PfCode::PHASE_BAD);
        }
        self.claim_bpp = claim_bpp;
        self.fb_len = fb.len();
        let digest = probe_digest(fb);
        // 重判同样先过坏帧闸。
        match bad_frame_audit(fb) {
            Ok(a) => {
                if let Some(code) = bad_frame_case(&a) {
                    let note = format!("重判定坏帧立案：{}（原判定 {} 依旧站得住）", bad_frame_note(&a), self.verdict_name());
                    self.diag = Some(code);
                    self.record_push(tick, None, Some(code), note, digest);
                    return Err(code);
                }
            }
            Err(code) => {
                let note = format!("重判定观察面空（零长度）：原判定 {} 依旧站得住", self.verdict_name());
                self.diag = Some(code);
                self.record_push(tick, None, Some(code), note, 0);
                return Err(code);
            }
        }
        let outcome = match claim_bpp {
            32 => verdict_32(fb),
            24 => verdict_24(fb),
            16 => verdict_16(fb),
            _ => {
                let note = format!(
                    "重判定声明位深 {} 不在四分发闭集（原判定 {} 依旧站得住）",
                    claim_bpp,
                    self.verdict_name()
                );
                self.diag = Some(PfCode::BITDEPTH_BAD);
                self.record_push(tick, None, Some(PfCode::BITDEPTH_BAD), note, digest);
                return Err(PfCode::BITDEPTH_BAD);
            }
        };
        match outcome {
            ProbeOutcome::Match(kind, basis) => {
                match self.verdict {
                    Some(prev) if prev == kind => {
                        let note = format!("重判定一致：{} —— {}", kind.name(), basis);
                        self.diag = None;
                        self.record_push(tick, Some(kind), None, note, digest);
                        Ok(kind)
                    }
                    Some(prev) => {
                        let note = format!(
                            "重判定不一致：前判 {} 本次 {} —— {}; 原判定依旧站得住不改写历史",
                            prev.name(),
                            kind.name(),
                            basis
                        );
                        self.diag = Some(PfCode::REJUDGE_MISMATCH);
                        self.record_push(tick, Some(prev), Some(PfCode::REJUDGE_MISMATCH), note, digest);
                        Err(PfCode::REJUDGE_MISMATCH)
                    }
                    None => {
                        // Decided 相态 verdict 为空属账本破缺——守恒检查会立案。
                        self.diag = Some(PfCode::LEDGER_BROKEN);
                        self.record_push(
                            tick,
                            None,
                            Some(PfCode::LEDGER_BROKEN),
                            "重判定发现 Decided 相态 verdict 为空——账本破缺".to_string(),
                            digest,
                        );
                        Err(PfCode::LEDGER_BROKEN)
                    }
                }
            }
            ProbeOutcome::Mismatch(note) => {
                let note2 = format!("重判定抽样核失配：{}（原判定 {} 依旧站得住）", note, self.verdict_name());
                self.diag = Some(PfCode::PROBE_MISMATCH);
                self.record_push(tick, None, Some(PfCode::PROBE_MISMATCH), note2, digest);
                Err(PfCode::PROBE_MISMATCH)
            }
            ProbeOutcome::Undecided(note) => {
                let note2 = format!("重判定变体不可判：{}（原判定 {} 依旧站得住）", note, self.verdict_name());
                self.diag = Some(PfCode::VARIANT_UNDECIDED);
                self.record_push(tick, None, Some(PfCode::VARIANT_UNDECIDED), note2, digest);
                Err(PfCode::VARIANT_UNDECIDED)
            }
            ProbeOutcome::Short(need, have) => {
                let note2 = format!(
                    "重判定探针区数据面不足：需要 {} 字节实有 {} 字节（原判定 {} 依旧站得住）",
                    need,
                    have,
                    self.verdict_name()
                );
                self.diag = Some(PfCode::DATA_SHORT);
                self.record_push(tick, None, Some(PfCode::DATA_SHORT), note2, digest);
                Err(PfCode::DATA_SHORT)
            }
        }
    }

    /// 账本守恒检查（恒等式闭集逐条对拍）：
    /// ① Idle 相态 ↔ 账空；② Decided ↔ verdict 有值且账非空；③
    /// Rejected ↔ 末条带诊断且账非空；④ 序号从 1 起严格连续；⑤ tick
    /// 单调不降；⑥ 账注全非空；⑦ 末条记录与相态/判定双向对拍。
    pub fn conservation_check(&self) -> Result<(), PfCode> {
        match self.phase {
            JudgePhase::Idle => {
                if !self.records.is_empty() {
                    return Err(PfCode::LEDGER_BROKEN);
                }
            }
            JudgePhase::Decided => {
                if self.verdict.is_none() || self.records.is_empty() {
                    return Err(PfCode::LEDGER_BROKEN);
                }
            }
            JudgePhase::Rejected => {
                if self.diag.is_none() || self.records.is_empty() {
                    return Err(PfCode::LEDGER_BROKEN);
                }
            }
        }
        let mut i = 0usize;
        while i < self.records.len() {
            let r = &self.records[i];
            if r.seq != (i as u64) + 1 {
                return Err(PfCode::LEDGER_BROKEN);
            }
            if r.basis.is_empty() {
                return Err(PfCode::LEDGER_BROKEN);
            }
            if i > 0 && r.tick < self.records[i - 1].tick {
                return Err(PfCode::LEDGER_BROKEN);
            }
            i += 1;
        }
        if let Some(last) = self.records.last() {
            match self.phase {
                JudgePhase::Decided => {
                    if last.kind != self.verdict || last.diag.is_some() {
                        return Err(PfCode::LEDGER_BROKEN);
                    }
                }
                JudgePhase::Rejected => {
                    if last.diag.is_none() {
                        return Err(PfCode::LEDGER_BROKEN);
                    }
                }
                JudgePhase::Idle => {
                    return Err(PfCode::LEDGER_BROKEN);
                }
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 十、清单导出比对
// ---------------------------------------------------------------------------

/// 判定登记账清单导出（kv 行形态——与 F0201 报告协议同构；行数闭式：
/// 头 1 + 概览 7 + 逐条 1 行/条目）。
pub fn export_manifest(j: &PixFmtJudge) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    out.push("# varix-pixfmt482-ledger".to_string());
    out.push(format!("phase={}", j.phase().name()));
    out.push(format!("verdict={}", j.verdict_name()));
    out.push(format!("claim_bpp={}", j.claim_bpp()));
    out.push(format!("fb_len={}", j.fb_len()));
    out.push(format!("begin_tick={}", j.begin_tick()));
    out.push(format!("diag={}", j.diag_wire()));
    out.push(format!("count={}", j.record_count()));
    let mut i = 0usize;
    while i < j.record_count() {
        if let Some(r) = j.record_at(i) {
            let kind_name = match r.kind {
                Some(k) => k.name().to_string(),
                None => "n/a".to_string(),
            };
            let diag_name = match r.diag {
                Some(c) => format!("0x{:04X}", c.code()),
                None => "none".to_string(),
            };
            out.push(format!(
                "rec.{} tick={} seq={} kind={} claim_bpp={} fb_len={} digest={:#010x} diag={} basis={}",
                i, r.tick, r.seq, kind_name, r.claim_bpp, r.fb_len, r.probe_digest, diag_name, r.basis
            ));
        }
        i += 1;
    }
    out
}

/// 清单期望行数闸（kv 行数 == 结构闭式——渲染确定性对拍锚）。
pub fn export_manifest_expected_len(j: &PixFmtJudge) -> usize {
    8 + j.record_count()
}

/// 清单比对账。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ManifestDiff {
    /// 逐行全等。
    pub same: bool,
    /// 首个分歧行下标（全等为 None）。
    pub first_diff: Option<usize>,
    /// a 侧行数。
    pub lines_a: usize,
    /// b 侧行数。
    pub lines_b: usize,
}

/// 清单逐行对拍（同长同文 → same；分歧 → 首分歧下标显性）。
pub fn compare_manifest(a: &[String], b: &[String]) -> ManifestDiff {
    let n = if a.len() < b.len() { a.len() } else { b.len() };
    let mut i = 0usize;
    while i < n {
        if a[i] != b[i] {
            return ManifestDiff {
                same: false,
                first_diff: Some(i),
                lines_a: a.len(),
                lines_b: b.len(),
            };
        }
        i += 1;
    }
    let same = a.len() == b.len();
    ManifestDiff {
        same,
        first_diff: if same { None } else { Some(n) },
        lines_a: a.len(),
        lines_b: b.len(),
    }
}

// ---------------------------------------------------------------------------
// 十一、深化区：判定依据账注常量与四分发对拍
// ---------------------------------------------------------------------------

/// 判定依据账注（BGRX 命中形态——判据「判定依据账注」的固定语料）。
pub const BASIS_NOTE_BGRX: &str =
    "判定依据账注：红蓝序实测抽样核——纯红 0xFF 落字节 2、纯蓝 0xFF 落字节 0、绿锚落字节 1，BGRX 假设整幅字节像相符";

/// 判定依据账注（XRGB 命中形态）。
pub const BASIS_NOTE_XRGB: &str =
    "判定依据账注：红蓝序实测抽样核——纯红 0xFF 落字节 1、纯蓝 0xFF 落字节 3、绿锚落字节 2，XRGB 假设整幅字节像相符";

/// 判定依据账注（24bpp 命中形态）。
pub const BASIS_NOTE_BGR24: &str =
    "判定依据账注：红蓝序实测抽样核（24bpp）——纯红 0xFF 落字节 2、纯蓝 0xFF 落字节 0、绿锚落字节 1，BGR24 假设整幅字节像相符";

/// 判定依据账注（16bpp 两变体共用形态——变体细分语料见 verdict_16）。
pub const BASIS_NOTE_16: &str =
    "判定依据账注：16bpp 变体探测——位 15 写 0x8000 读回裁决 565/555，红蓝序次核 0xF800/0x07E0/0x001F 主导通道相符";

/// 有损降级总账注（32↔24↔16 转换面——判据「含 32↔24↔16 有损降级账注」）。
pub const LOSSY_NOTE: &str =
    "32↔24↔16 转换账注：32↔24 双向无损（8 位通道全保留）；32/24→16 有损降级（通道量化收缩，下行 round-to-nearest 整数定点）；16→32/24 无损上行展开；565→555 有损（绿 6→5 位）、555→565 无损";

/// 四分发槽位对拍（判据原文逐字对拍——「32bpp: BGRX/XRGB/24bpp/16bpp」）。
pub fn four_way_slots_note() -> String {
    let mut s = String::from("四分发槽位：");
    let mut i = 0usize;
    while i < FOUR_WAY_SLOTS.len() {
        if i > 0 {
            s.push('/');
        }
        s.push_str(FOUR_WAY_SLOTS[i]);
        i += 1;
    }
    s
}

/// 判定产物 → 固定账注对拍（判定结果与语料账注一致——防语料与判定
/// 逻辑漂移）。
pub fn basis_note_matches(kind: PixFmtKind, basis: &str) -> bool {
    match kind {
        PixFmtKind::Bgrx32 => basis.contains("BGRX") && basis.contains("红蓝序"),
        PixFmtKind::Xrgb32 => basis.contains("XRGB") && basis.contains("红蓝序"),
        PixFmtKind::Bgr24 => basis.contains("BGR24") && basis.contains("红蓝序"),
        PixFmtKind::Rgb565 | PixFmtKind::Rgb555 => basis.contains("位 15") && basis.contains("红蓝序"),
        PixFmtKind::Unknown => false,
    }
}

// ---------------------------------------------------------------------------
// 十二、深化区：全链合成账（判定+stride+位型表一次出账）
// ---------------------------------------------------------------------------

/// 全链合成账：判定产物+描述符+行距账+五常量位型表+依据账注——
/// 调用方一次拿到全部换算事实（全部纯函数零 IO）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JudgeBundle {
    /// 判定产物。
    pub kind: PixFmtKind,
    /// 描述符。
    pub desc: FmtDesc,
    /// 行距账。
    pub stride: StrideInfo,
    /// 五常量位型表（黑/白/红/绿/蓝）。
    pub colors: Vec<(String, u32)>,
    /// 判定依据账注。
    pub basis: String,
}

/// 全链合成单点：判定 → 描述符自审 → stride → 位型表 → 依据账注，
/// 任一环失败回诊断码（合成账只在全绿时出）。
pub fn judge_bundle(claim_bpp: u32, fb: &[u8], tick: u64, width: u64, align_req: u64) -> Result<JudgeBundle, PfCode> {
    let (kind, basis) = judge_once(claim_bpp, fb, tick)?;
    let desc = descriptor(kind);
    desc_audit(&desc)?;
    let stride = stride_calc(kind, width, align_req)?;
    let colors = color_bit_patterns(kind)?;
    Ok(JudgeBundle {
        kind,
        desc,
        stride,
        colors,
        basis,
    })
}
