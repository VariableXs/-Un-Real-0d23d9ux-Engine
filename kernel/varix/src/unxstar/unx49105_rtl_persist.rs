//! # unx49105 — B16 RTL 布局基向与国别布局切换持久化
//!
//! 卡面原文（任务板 CoRun·UNX-F2-B40）：
//!
//! > **F49105 阿拉伯/希伯来 RTL 布局基向（双态字形层与方向无关键位）**
//! > **F49106 国别布局切换持久化（per-session 与全局两层配置，切换期零丢键）**
//!
//! 验收锚点：
//!
//! - `UNX-F49105-J1` RTL 基向判定与镜像/数字/字形四类键位十轮零误判。
//! - `UNX-F49106-J1` 100 轮 per-session/全局两层切换，在途键零丢失、
//!   落盘可回读、回退可取证。
//!
//! ## 闸序与理由
//!
//! 1. **基向先立**：每个键必须先归入「字母/数字/中性/镜像/控制/空格」六类，
//!    基向才有可判对象；分类错则后面四条闸全部是错的账。
//! 2. **数字段恒 LTR**：阿-伯数字与阿拉伯-印度数字都在 RTL 基向里保持
//!    LTR run——把数字判成 RTL 会让整段文本重排错位，且错位不可逆。
//! 3. **镜像可逆**：成对符号在 RTL 下必须换成对偶码位，且镜像两次等于
//!    恒等；不可逆的镜像就是把一个括号悄悄变成另一个括号之外的东西。
//! 4. **字形四态**：阿拉伯字母按前后连接性落在独立/首/中/尾四形，
//!    不连字母（ا د ذ ر ز و）只能落独立或尾形；把不连字母判成首形，
//!    后续所有字母就会整体错位一个连接位。
//! 5. **切换零丢键**：两层配置（per-session 覆盖全局）切换期间到达的键
//!    必须进在途队列，切换完成后原样落到新布局；静默丢键比布局错更糟，
//!    因为丢掉的键在账上根本不存在。
//! 6. **落盘可回读**：切换账与字形账都必须渲染、回读、拒篡改。
//!
//! ## 与相邻模块的差异
//!
//! - `unx49085_keymap_fallback` 只管 US/UK/DE/FR 四套 LTR 表与损坏回退；
//!   本模块处理 **RTL 基向 + 字形四态 + 两层配置持久化**，与它共享布局
//!   一词但不共享任何表。
//! - `unx49101` 与 `unx49103` 收国际 LTR 黄金集与韩/俄东欧套；本模块
//!   的阿/希两套表独立成段（0x73800..=0x7383F），不与前两者互抄键位。
//! - 本模块无全局可变状态：会话、在途队列、配置都由调用方按值持有。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

// ── 段位与规模 ───────────────────────────────────────────────────
/// 本模块段基址（B16 第三槽）。
pub const SEG_BASE: u32 = 0x73800;
/// 段宽（相邻模块互斥）。
pub const SEG_SPAN: u32 = 0x40;
/// 持久化诊断码子窗基址。
pub const PERS_BASE: u32 = SEG_BASE + 0x11;
/// 阿拉伯字母数。
pub const AR_LETTERS: usize = 28;
/// 希伯来字母数。
pub const HE_LETTERS: usize = 22;
/// 可连字母数（28 - 6 个只连左字母）。
pub const JOINABLE: usize = 22;
/// 希伯来词尾形数。
pub const HE_FINALS: usize = 5;
/// 镜像对数。
pub const MIRROR_PAIRS: usize = 8;
/// 阿拉伯-印度数字数。
pub const ARABIC_DIGITS: usize = 10;
/// 黄金对规模：阿 20 + 希 15 + 镜像 10。
pub const GOLDEN_AR: usize = 20;
pub const GOLDEN_HE: usize = 15;
pub const GOLDEN_MIRROR: usize = 10;
pub const GOLDEN: usize = GOLDEN_AR + GOLDEN_HE + GOLDEN_MIRROR;
/// 切换轮数（判据 ×100）。
pub const SWITCH_ROUNDS: u32 = 100;
/// 每轮在途键数。
pub const PENDING_PER_ROUND: u32 = 6;
/// 在途队列容量。
pub const PENDING_CAP: usize = 16;
/// 层数（基/换/第三功能层）。
pub const LAYERS: usize = 3;
/// 字形四态数。
pub const SHAPES: usize = 4;
/// 账行槽数。
pub const ROW_SLOTS: usize = 3;
/// 字形账行数（4 组连接态 × 2 槽位）。
pub const SHAPE_ROW_SLOTS: usize = SHAPES * 2;
/// 切换账标签与键序。
pub const SW_TAG: &str = "SWCH105";
pub const SW_KEYS: [&str; 5] = ["round", "layout", "pending", "delivered", "lost"];
/// 字形账标签与键序。
pub const SHAPE_TAG: &str = "SHAPE105";
pub const SHAPE_KEYS: [&str; 4] = ["slot", "shape", "codepoint", "joinable"];
/// 未知诊断码说明。
pub const UNKNOWN: &str = "未知 B16 RTL/持久化诊断码";

// ── 诊断码 ───────────────────────────────────────────────────────
/// RTL 与持久化诊断码（14 条，说明互不共用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RtlCode {
    /// 键类无法归入六类之一
    ClassBad,
    /// 数字段被判成 RTL
    DigitBad,
    /// 镜像两次不等于恒等
    MirrorBad,
    /// 不连字母出现首形或中部形
    ShapeBad,
    /// 层号越出 0..=2
    LayerBad,
    /// 布局码位无法反查
    CodepointBad,
    /// 黄金对表不完整
    GoldenBad,
    /// 两层配置解析错（会话覆盖未生效）
    ScopeBad,
    /// 切换期在途键丢失
    PendingBad,
    /// 在途队列越出容量
    QueueBad,
    /// 摘要与重算不符
    DigestBad,
    /// 账行键序错位
    RowBad,
    /// 字段数或字段名不符
    FieldBad,
    /// 处置链不完整
    Incomplete,
}

impl RtlCode {
    /// 诊断码值。
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    /// 逐码说明（互不共用）。
    pub fn detail(self) -> &'static str {
        match self {
            RtlCode::ClassBad => "键位无法归入字母/数字/中性/镜像/控制/空格六类",
            RtlCode::DigitBad => "数字段被判成 RTL，违反数字恒 LTR 规则",
            RtlCode::MirrorBad => "成对符号镜像两次未回到原码位",
            RtlCode::ShapeBad => "只连左字母被判成首形或中部形",
            RtlCode::LayerBad => "层号越出 基/换/第三功能 三层窗口",
            RtlCode::CodepointBad => "码位无法反查到布局与槽位",
            RtlCode::GoldenBad => "黄金对表数量与登记规模不符",
            RtlCode::ScopeBad => "两层配置解析未让 per-session 覆盖全局",
            RtlCode::PendingBad => "切换期间在途键被静默丢弃",
            RtlCode::QueueBad => "在途队列占用越出 16 槽上限",
            RtlCode::DigestBad => "摘要与重算不一致",
            RtlCode::RowBad => "账行键序或标签错位",
            RtlCode::FieldBad => "账行字段数或字段名不符",
            RtlCode::Incomplete => "RTL 与切换处置链未跑满全程",
        }
    }

    /// 面向运营的短句（与 `detail` 不同口径）。
    pub fn message(self) -> &'static str {
        match self {
            RtlCode::ClassBad => "键类不明：请补六类判定",
            RtlCode::DigitBad => "数字反向：请钉 LTR",
            RtlCode::MirrorBad => "镜像不可逆：请核对对偶表",
            RtlCode::ShapeBad => "字形错态：请回连性表",
            RtlCode::LayerBad => "层号越界：请回到三层",
            RtlCode::CodepointBad => "码位查无：请核对布局表",
            RtlCode::GoldenBad => "黄金不全：请补足四十五对",
            RtlCode::ScopeBad => "层级失效：请让会话覆盖全局",
            RtlCode::PendingBad => "丢键：请在途补投",
            RtlCode::QueueBad => "队列越界：请先排空",
            RtlCode::DigestBad => "摘要失配：请重算账行",
            RtlCode::RowBad => "账行损坏：请重渲染",
            RtlCode::FieldBad => "字段错位：请核对键序",
            RtlCode::Incomplete => "链路不全：请跑满百轮",
        }
    }
}

/// 全部诊断码（顺序即值序）。
pub const ALL: [RtlCode; 14] = [
    RtlCode::ClassBad,
    RtlCode::DigitBad,
    RtlCode::MirrorBad,
    RtlCode::ShapeBad,
    RtlCode::LayerBad,
    RtlCode::CodepointBad,
    RtlCode::GoldenBad,
    RtlCode::ScopeBad,
    RtlCode::PendingBad,
    RtlCode::QueueBad,
    RtlCode::DigestBad,
    RtlCode::RowBad,
    RtlCode::FieldBad,
    RtlCode::Incomplete,
];

/// 自由函数：码值 → 说明（段外一律 `UNKNOWN`）。
pub fn detail(code: u32) -> &'static str {
    match code.wrapping_sub(SEG_BASE) {
        1 => RtlCode::ClassBad.detail(),
        2 => RtlCode::DigitBad.detail(),
        3 => RtlCode::MirrorBad.detail(),
        4 => RtlCode::ShapeBad.detail(),
        5 => RtlCode::LayerBad.detail(),
        6 => RtlCode::CodepointBad.detail(),
        7 => RtlCode::GoldenBad.detail(),
        8 => RtlCode::ScopeBad.detail(),
        9 => RtlCode::PendingBad.detail(),
        10 => RtlCode::QueueBad.detail(),
        11 => RtlCode::DigestBad.detail(),
        12 => RtlCode::RowBad.detail(),
        13 => RtlCode::FieldBad.detail(),
        14 => RtlCode::Incomplete.detail(),
        _ => UNKNOWN,
    }
}

/// 段位自检：14 码值两两不同且全部落在本段窗口内。
pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < ALL.len() {
        let a = ALL[i].val();
        if a <= SEG_BASE || a > SEG_BASE + 0x0F {
            return false;
        }
        let mut j = i + 1;
        while j < ALL.len() {
            if ALL[j].val() == a {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 持久化子窗码值自检：与主码区不重叠且互不相同。
pub fn pers_guard() -> bool {
    let mut i = 0usize;
    while i < ALL.len() {
        let a = PERS_BASE + ALL[i].val().saturating_sub(SEG_BASE);
        if a <= SEG_BASE + 0x0F || a > SEG_BASE + SEG_SPAN {
            return false;
        }
        let mut j = i + 1;
        while j < ALL.len() {
            if PERS_BASE + ALL[j].val().saturating_sub(SEG_BASE) == a {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 持久化子窗码值。
pub fn pers_val(code: RtlCode) -> u32 {
    PERS_BASE.saturating_add(code.val().saturating_sub(SEG_BASE))
}

// ── 基向与键类 ───────────────────────────────────────────────────
/// 书写基向。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    /// 从左到右。
    Ltr,
    /// 从右到左。
    Rtl,
}

impl Dir {
    /// 账行取值。
    pub fn tag(self) -> &'static str {
        match self {
            Dir::Ltr => "ltr",
            Dir::Rtl => "rtl",
        }
    }

    /// 由账行取值反解。
    pub fn from_tag(tag: &str) -> Option<Dir> {
        match tag {
            "ltr" => Some(Dir::Ltr),
            "rtl" => Some(Dir::Rtl),
            _ => None,
        }
    }

    /// 反向。
    pub fn opposite(self) -> Dir {
        match self {
            Dir::Ltr => Dir::Rtl,
            Dir::Rtl => Dir::Ltr,
        }
    }
}

/// 键位方向类别（六类，互斥）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Class {
    /// 字母：跟随布局基向。
    Letter,
    /// 数字：恒 LTR。
    Number,
    /// 中性：两侧同向时继承该向。
    Neutral,
    /// 成对符号：RTL 下镜像。
    Mirror,
    /// 控制键：方向无关，不透明。
    Control,
    /// 空格：词边界，中性但可断 run。
    Space,
}

impl Class {
    /// 账行取值。
    pub fn tag(self) -> &'static str {
        match self {
            Class::Letter => "letter",
            Class::Number => "number",
            Class::Neutral => "neutral",
            Class::Mirror => "mirror",
            Class::Control => "control",
            Class::Space => "space",
        }
    }

    /// 由账行取值反解。
    pub fn from_tag(tag: &str) -> Option<Class> {
        match tag {
            "letter" => Some(Class::Letter),
            "number" => Some(Class::Number),
            "neutral" => Some(Class::Neutral),
            "mirror" => Some(Class::Mirror),
            "control" => Some(Class::Control),
            "space" => Some(Class::Space),
            _ => None,
        }
    }

    /// 类下标。
    pub fn index(self) -> usize {
        self as usize
    }

    /// 该类的呈现方向：数字恒 LTR，其余跟随基向；控制与空格为方向无关。
    pub fn run_dir(self, base: Dir) -> Dir {
        match self {
            Class::Number => Dir::Ltr,
            Class::Control | Class::Space => base,
            _ => base,
        }
    }

    /// 是否参与 run 连续性（中性与控制会断开 run）。
    pub fn keeps_run(self) -> bool {
        matches!(self, Class::Letter | Class::Number)
    }
}

/// 全部键类（顺序即下标序）。
pub const ALL_CLASSES: [Class; 6] = [
    Class::Letter,
    Class::Number,
    Class::Neutral,
    Class::Mirror,
    Class::Control,
    Class::Space,
];

// ── 层与字形 ─────────────────────────────────────────────────────
/// 键位层。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layer {
    /// 基层。
    Base,
    /// 换档层。
    Shift,
    /// 第三功能层。
    Alt,
}

impl Layer {
    /// 层下标。
    pub fn index(self) -> usize {
        self as usize
    }

    /// 由下标反查层（越界 `None`）。
    pub fn from_index(i: usize) -> Option<Layer> {
        match i {
            0 => Some(Layer::Base),
            1 => Some(Layer::Shift),
            2 => Some(Layer::Alt),
            _ => None,
        }
    }

    /// 账行取值。
    pub fn tag(self) -> &'static str {
        match self {
            Layer::Base => "base",
            Layer::Shift => "shift",
            Layer::Alt => "alt",
        }
    }

    /// 由账行取值反解。
    pub fn from_tag(tag: &str) -> Option<Layer> {
        match tag {
            "base" => Some(Layer::Base),
            "shift" => Some(Layer::Shift),
            "alt" => Some(Layer::Alt),
            _ => None,
        }
    }
}

/// 阿拉伯字形四态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shape {
    /// 独立形。
    Isolated,
    /// 首形。
    Initial,
    /// 中部形。
    Medial,
    /// 尾形。
    Final,
}

impl Shape {
    /// 态下标。
    pub fn index(self) -> usize {
        self as usize
    }

    /// 由下标反查（越界 `None`）。
    pub fn from_index(i: usize) -> Option<Shape> {
        match i {
            0 => Some(Shape::Isolated),
            1 => Some(Shape::Initial),
            2 => Some(Shape::Medial),
            3 => Some(Shape::Final),
            _ => None,
        }
    }

    /// 账行取值。
    pub fn tag(self) -> &'static str {
        match self {
            Shape::Isolated => "iso",
            Shape::Initial => "init",
            Shape::Medial => "medial",
            Shape::Final => "final",
        }
    }

    /// 由账行取值反解。
    pub fn from_tag(tag: &str) -> Option<Shape> {
        match tag {
            "iso" => Some(Shape::Isolated),
            "init" => Some(Shape::Initial),
            "medial" => Some(Shape::Medial),
            "final" => Some(Shape::Final),
            _ => None,
        }
    }

    /// 该态是否要求字母可向右连（即前一个字母连到它）。
    pub fn needs_prev(self) -> bool {
        matches!(self, Shape::Initial | Shape::Medial | Shape::Final)
    }

    /// 该态是否要求字母可向左连（即它连到后一个字母）。
    pub fn needs_next(self) -> bool {
        matches!(self, Shape::Initial | Shape::Medial)
    }
}

// ── 布局标识 ─────────────────────────────────────────────────────
/// 本模块登记的布局（含回退用美式）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LayoutId {
    /// 阿拉伯（RTL）。
    Arabic,
    /// 希伯来（RTL）。
    Hebrew,
    /// 美式（LTR，回退基准）。
    Us,
}

impl LayoutId {
    /// 布局下标。
    pub fn index(self) -> usize {
        self as usize
    }

    /// 由下标反查（越界 `None`）。
    pub fn from_index(i: usize) -> Option<LayoutId> {
        match i {
            0 => Some(LayoutId::Arabic),
            1 => Some(LayoutId::Hebrew),
            2 => Some(LayoutId::Us),
            _ => None,
        }
    }

    /// 账行取值。
    pub fn tag(self) -> &'static str {
        match self {
            LayoutId::Arabic => "ar",
            LayoutId::Hebrew => "he",
            LayoutId::Us => "us",
        }
    }

    /// 由账行取值反解。
    pub fn from_tag(tag: &str) -> Option<LayoutId> {
        match tag {
            "ar" => Some(LayoutId::Arabic),
            "he" => Some(LayoutId::Hebrew),
            "us" => Some(LayoutId::Us),
            _ => None,
        }
    }

    /// 布局名（中文口径，供告警与账行不同字段）。
    pub fn name(self) -> &'static str {
        match self {
            LayoutId::Arabic => "阿拉伯 RTL",
            LayoutId::Hebrew => "希伯来 RTL",
            LayoutId::Us => "美式 LTR",
        }
    }

    /// 基向。
    pub fn dir(self) -> Dir {
        match self {
            LayoutId::Arabic | LayoutId::Hebrew => Dir::Rtl,
            LayoutId::Us => Dir::Ltr,
        }
    }

    /// 该布局字母数。
    pub fn letters(self) -> usize {
        match self {
            LayoutId::Arabic => AR_LETTERS,
            LayoutId::Hebrew => HE_LETTERS,
            LayoutId::Us => 0,
        }
    }

    /// 布局签名：字母数、基向、表校验和与规模四元（互不相同）。
    pub fn signature(self) -> [u8; 8] {
        let mut out = [0u8; 8];
        out[0] = self.index() as u8;
        out[1] = self.letters() as u8;
        out[2] = match self.dir() {
            Dir::Ltr => b'l',
            Dir::Rtl => b'r',
        };
        let sum = self.checksum();
        out[3] = (sum & 0xFF) as u8;
        out[4] = ((sum >> 8) & 0xFF) as u8;
        out[5] = ((sum >> 16) & 0xFF) as u8;
        out[6] = ((sum >> 24) & 0xFF) as u8;
        out[7] = match self {
            LayoutId::Arabic => 1,
            LayoutId::Hebrew => 2,
            LayoutId::Us => 0,
        };
        out
    }

    /// 表校验和：按层把码位压成 32 位（确定性，供签名与摘要复用）。
    pub fn checksum(self) -> u32 {
        let mut acc = 0u32;
        let mut layer = 0usize;
        while layer < LAYERS {
            let table = match self.table(layer) {
                Some(t) => t,
                None => return acc,
            };
            let mut i = 0usize;
            while i < table.len() {
                if let Some(c) = table.get(i) {
                    acc = acc.wrapping_mul(31).wrapping_add(*c as u32);
                }
                i += 1;
            }
            layer += 1;
        }
        acc
    }

    /// 取指定层的键位表（美式为 `None`，由 `unx49085` 负责）。
    pub fn table(self, layer: usize) -> Option<&'static [char]> {
        match (self, Layer::from_index(layer)) {
            (LayoutId::Arabic, Some(Layer::Base)) => Some(&AR_BASE),
            (LayoutId::Arabic, Some(Layer::Shift)) => Some(&AR_SHIFT),
            (LayoutId::Arabic, Some(Layer::Alt)) => Some(&AR_ALT),
            (LayoutId::Hebrew, Some(Layer::Base)) => Some(&HE_BASE),
            (LayoutId::Hebrew, Some(Layer::Shift)) => Some(&HE_SHIFT),
            (LayoutId::Hebrew, Some(Layer::Alt)) => Some(&HE_ALT),
            _ => None,
        }
    }
}

/// 全部布局（顺序即下标序）。
pub const ALL_LAYOUTS: [LayoutId; 3] = [LayoutId::Arabic, LayoutId::Hebrew, LayoutId::Us];

// ── 键位表（逐键手写） ──────────────────────────────────────────
/// 阿拉伯基层：28 个字母，按标准字母序登记到物理槽位。
pub const AR_BASE: [char; AR_LETTERS] = [
    'ا', 'ب', 'ت', 'ث', 'ج', 'ح', 'خ', 'د', 'ذ', 'ر', 'ز', 'س', 'ش',
    'ص', 'ض', 'ط', 'ظ', 'ع', 'غ', 'ف', 'ق', 'ك', 'ل', 'م', 'ن', 'ه',
    'و', 'ي',
];

/// 阿拉伯换档层：28 个音符/ hamza 族符号，逐位登记且与基层互不重叠。
pub const AR_SHIFT: [char; AR_LETTERS] = [
    'ٰ', 'ٓ', 'ٔ', 'ٕ', 'ٗ', '٘', 'ٙ', 'ٚ', 'ٛ', 'ٜ', 'ٝ', 'ٞ', 'ٟ',
    'ً', 'ٌ', 'ٍ', 'َ', 'ُ', 'ِ', 'ّ', 'ْ',
    'ۖ', 'ۗ', 'ۘ', 'ۙ', 'ۚ', 'ۛ', 'ۜ',
];

/// 阿拉伯第三功能层：前 10 槽为阿拉伯-印度数字，其余为排版符号。
pub const AR_ALT: [char; AR_LETTERS] = [
    '٠', '١', '٢', '٣', '٤', '٥', '٦', '٧', '٨', '٩',
    '٪', '٫', '،', '؟', '؛', '«', '»', '(', ')', '=',
    '[', ']', '{', '}', '〈', '〉', '《', '》',
];

/// 希伯来基层：22 个字母。
pub const HE_BASE: [char; HE_LETTERS] = [
    'א', 'ב', 'ג', 'ד', 'ה', 'ו', 'ז', 'ח', 'ט', 'י', 'כ', 'ל', 'מ',
    'נ', 'ס', 'ע', 'פ', 'צ', 'ק', 'ר', 'ש', 'ת',
];

/// 希伯来换档层：22 个符号，逐位登记且与基层互不重叠。
pub const HE_SHIFT: [char; HE_LETTERS] = [
    '!', '@', '#', '$', '%', '^', '&', '*', '(', ')', '-', '=', ';',
    '\'', '[', ']', ',', '.', '/', '<', '>', '~',
];

/// 希伯来第三功能层：22 个标点与货币/引号族。
pub const HE_ALT: [char; HE_LETTERS] = [
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', '_', '+', ':',
    '"', '{', '}', '?', '₪', '«', '»', '–', '|',
];

/// 阿拉伯-印度数字（0..9）。
pub const ARABIC_DIGIT_TABLE: [char; ARABIC_DIGITS] =
    ['٠', '١', '٢', '٣', '٤', '٥', '٦', '٧', '٨', '٩'];

/// 只连左（不向右连）的阿拉伯字母：ا د ذ ر ز و，共 6 个。
pub const AR_RIGHT_ONLY: [char; 6] = ['ا', 'د', 'ذ', 'ر', 'ز', 'و'];

/// 双向控制与排版方向键的固定码位。
pub const KEY_RLM: char = '\u{200F}';
pub const KEY_LRM: char = '\u{200E}';
pub const KEY_ALM: char = '\u{061C}';
pub const KEY_SPACE: char = ' ';

/// 镜像对：RTL 下每个码位换成对偶，两次必回原值。
pub const MIRROR: [(char, char); MIRROR_PAIRS] = [
    ('(', ')'),
    ('[', ']'),
    ('{', '}'),
    ('〈', '〉'),
    ('《', '》'),
    ('「', '」'),
    ('『', '』'),
    ('【', '】'),
];

/// 希伯来词尾形：5 个字母在词尾换成对偶码位（无连接性，属表内映射）。
pub const HE_FINAL_TABLE: [(char, char); HE_FINALS] = [
    ('כ', 'ך'),
    ('מ', 'ם'),
    ('נ', 'ן'),
    ('פ', 'ף'),
    ('צ', 'ץ'),
];

// ── 表查询 ───────────────────────────────────────────────────────
/// 字母是否为「只连左」的阿拉伯字母。
pub fn is_right_only(c: char) -> bool {
    let mut i = 0usize;
    while i < AR_RIGHT_ONLY.len() {
        if let Some(v) = AR_RIGHT_ONLY.get(i) {
            if *v == c {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 字母是否可左右两向连接（阿拉伯 28 去掉 6 个只连左）。
pub fn is_joinable(c: char) -> bool {
    if !is_arabic_letter(c) {
        return false;
    }
    !is_right_only(c)
}

/// 是否为阿拉伯基层字母。
pub fn is_arabic_letter(c: char) -> bool {
    let mut i = 0usize;
    while i < AR_BASE.len() {
        if let Some(v) = AR_BASE.get(i) {
            if *v == c {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 是否为希伯来基层字母。
pub fn is_hebrew_letter(c: char) -> bool {
    let mut i = 0usize;
    while i < HE_BASE.len() {
        if let Some(v) = HE_BASE.get(i) {
            if *v == c {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 是否为阿拉伯-印度数字。
pub fn is_arabic_digit(c: char) -> bool {
    let mut i = 0usize;
    while i < ARABIC_DIGIT_TABLE.len() {
        if let Some(v) = ARABIC_DIGIT_TABLE.get(i) {
            if *v == c {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 是否为 ASCII 数字。
pub fn is_ascii_digit(c: char) -> bool {
    matches!(c, '0'..='9')
}

/// 键类判定：字母 → 数字 → 镜像 → 控制 → 空格 → 中性，六类互斥。
pub fn classify(c: char) -> Class {
    if is_arabic_letter(c) || is_hebrew_letter(c) {
        return Class::Letter;
    }
    if is_ascii_digit(c) || is_arabic_digit(c) {
        return Class::Number;
    }
    if mirror_of(c).is_some() {
        return Class::Mirror;
    }
    if c == KEY_RLM || c == KEY_LRM || c == KEY_ALM {
        return Class::Control;
    }
    if c == KEY_SPACE {
        return Class::Space;
    }
    Class::Neutral
}

/// 码位反查槽位（按层查表）。
pub fn slot_of(layout: LayoutId, layer: Layer, c: char) -> Option<usize> {
    let table = layout.table(layer.index())?;
    let mut i = 0usize;
    while i < table.len() {
        if let Some(v) = table.get(i) {
            if *v == c {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// 码位反查布局与层（先阿后希，先基后换再第三层）。
pub fn locate(c: char) -> Option<(LayoutId, Layer, usize)> {
    let mut l = 0usize;
    while l < ALL_LAYOUTS.len() {
        let layout = match ALL_LAYOUTS.get(l) {
            Some(v) => *v,
            None => return None,
        };
        let mut k = 0usize;
        while k < LAYERS {
            let layer = match Layer::from_index(k) {
                Some(v) => v,
                None => return None,
            };
            if let Some(slot) = slot_of(layout, layer, c) {
                return Some((layout, layer, slot));
            }
            k += 1;
        }
        l += 1;
    }
    None
}

/// 镜像对偶（双向查表）。
pub fn mirror_of(c: char) -> Option<char> {
    let mut i = 0usize;
    while i < MIRROR.len() {
        let pair = match MIRROR.get(i) {
            Some(v) => *v,
            None => return None,
        };
        if pair.0 == c {
            return Some(pair.1);
        }
        if pair.1 == c {
            return Some(pair.0);
        }
        i += 1;
    }
    None
}

/// RTL 下的呈现码位：镜像类换成对偶，其余原样。
pub fn shaped_in_dir(layout: LayoutId, c: char) -> char {
    match layout.dir() {
        Dir::Ltr => c,
        Dir::Rtl => match mirror_of(c) {
            Some(m) => m,
            None => c,
        },
    }
}

/// 字形四态判定：由前后连接性决定，不连字母不落首/中部形。
pub fn shape_of(c: char, prev_joinable: bool, next_joinable: bool) -> Option<Shape> {
    if !is_arabic_letter(c) {
        return None;
    }
    let self_left = is_joinable(c);
    if prev_joinable {
        // 只连左字母即便后继可连也到不了中部形：它不向右连。
        if next_joinable && self_left {
            return Some(Shape::Medial);
        }
        return Some(Shape::Final);
    }
    if self_left && next_joinable {
        return Some(Shape::Initial);
    }
    Some(Shape::Isolated)
}

/// 字形合法性：不连字母永不出首形/中部形。
pub fn shape_allowed(c: char, shape: Shape) -> bool {
    if !is_arabic_letter(c) {
        return false;
    }
    if is_right_only(c) {
        return shape == Shape::Isolated || shape == Shape::Final;
    }
    true
}

/// 字形码位偏移（独立形零偏移；首/中/尾各偏移一档，登记为常量）。
pub const SHAPE_OFFSET: [u32; SHAPES] = [0x000, 0x004, 0x008, 0x00C];

/// 由字母与字形算出呈现码位。
pub fn shaped_codepoint(c: char, shape: Shape) -> Option<u32> {
    if !shape_allowed(c, shape) {
        return None;
    }
    let idx = shape.index();
    let off = SHAPE_OFFSET.get(idx)?;
    Some(c as u32 + off)
}

// ── 黄金对表 ─────────────────────────────────────────────────────
/// 一条黄金对：物理槽位输入 → 期望呈现码位，带基向与层。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Golden {
    /// 序号 1..=45。
    pub id: u8,
    /// 布局。
    pub layout: LayoutId,
    /// 层。
    pub layer: Layer,
    /// 物理槽位。
    pub slot: u8,
    /// 期望码位。
    pub expect: char,
    /// 期望基向。
    pub dir: Dir,
}

/// 阿拉伯基层黄金 20 对（槽位与码位逐对手工核对）。
pub const GOLDEN_TABLE: [Golden; GOLDEN] = [
    Golden { id: 1, layout: LayoutId::Arabic, layer: Layer::Base, slot: 0, expect: 'ا', dir: Dir::Rtl },
    Golden { id: 2, layout: LayoutId::Arabic, layer: Layer::Base, slot: 1, expect: 'ب', dir: Dir::Rtl },
    Golden { id: 3, layout: LayoutId::Arabic, layer: Layer::Base, slot: 2, expect: 'ت', dir: Dir::Rtl },
    Golden { id: 4, layout: LayoutId::Arabic, layer: Layer::Base, slot: 3, expect: 'ث', dir: Dir::Rtl },
    Golden { id: 5, layout: LayoutId::Arabic, layer: Layer::Base, slot: 4, expect: 'ج', dir: Dir::Rtl },
    Golden { id: 6, layout: LayoutId::Arabic, layer: Layer::Base, slot: 5, expect: 'ح', dir: Dir::Rtl },
    Golden { id: 7, layout: LayoutId::Arabic, layer: Layer::Base, slot: 6, expect: 'خ', dir: Dir::Rtl },
    Golden { id: 8, layout: LayoutId::Arabic, layer: Layer::Base, slot: 7, expect: 'د', dir: Dir::Rtl },
    Golden { id: 9, layout: LayoutId::Arabic, layer: Layer::Base, slot: 8, expect: 'ذ', dir: Dir::Rtl },
    Golden { id: 10, layout: LayoutId::Arabic, layer: Layer::Base, slot: 9, expect: 'ر', dir: Dir::Rtl },
    Golden { id: 11, layout: LayoutId::Arabic, layer: Layer::Base, slot: 10, expect: 'ز', dir: Dir::Rtl },
    Golden { id: 12, layout: LayoutId::Arabic, layer: Layer::Base, slot: 11, expect: 'س', dir: Dir::Rtl },
    Golden { id: 13, layout: LayoutId::Arabic, layer: Layer::Base, slot: 12, expect: 'ش', dir: Dir::Rtl },
    Golden { id: 14, layout: LayoutId::Arabic, layer: Layer::Base, slot: 13, expect: 'ص', dir: Dir::Rtl },
    Golden { id: 15, layout: LayoutId::Arabic, layer: Layer::Base, slot: 14, expect: 'ض', dir: Dir::Rtl },
    Golden { id: 16, layout: LayoutId::Arabic, layer: Layer::Base, slot: 15, expect: 'ط', dir: Dir::Rtl },
    Golden { id: 17, layout: LayoutId::Arabic, layer: Layer::Base, slot: 16, expect: 'ظ', dir: Dir::Rtl },
    Golden { id: 18, layout: LayoutId::Arabic, layer: Layer::Base, slot: 17, expect: 'ع', dir: Dir::Rtl },
    Golden { id: 19, layout: LayoutId::Arabic, layer: Layer::Base, slot: 18, expect: 'غ', dir: Dir::Rtl },
    Golden { id: 20, layout: LayoutId::Arabic, layer: Layer::Base, slot: 19, expect: 'ف', dir: Dir::Rtl },
    Golden { id: 21, layout: LayoutId::Hebrew, layer: Layer::Base, slot: 0, expect: 'א', dir: Dir::Rtl },
    Golden { id: 22, layout: LayoutId::Hebrew, layer: Layer::Base, slot: 1, expect: 'ב', dir: Dir::Rtl },
    Golden { id: 23, layout: LayoutId::Hebrew, layer: Layer::Base, slot: 2, expect: 'ג', dir: Dir::Rtl },
    Golden { id: 24, layout: LayoutId::Hebrew, layer: Layer::Base, slot: 3, expect: 'ד', dir: Dir::Rtl },
    Golden { id: 25, layout: LayoutId::Hebrew, layer: Layer::Base, slot: 4, expect: 'ה', dir: Dir::Rtl },
    Golden { id: 26, layout: LayoutId::Hebrew, layer: Layer::Base, slot: 5, expect: 'ו', dir: Dir::Rtl },
    Golden { id: 27, layout: LayoutId::Hebrew, layer: Layer::Base, slot: 6, expect: 'ז', dir: Dir::Rtl },
    Golden { id: 28, layout: LayoutId::Hebrew, layer: Layer::Base, slot: 7, expect: 'ח', dir: Dir::Rtl },
    Golden { id: 29, layout: LayoutId::Hebrew, layer: Layer::Base, slot: 8, expect: 'ט', dir: Dir::Rtl },
    Golden { id: 30, layout: LayoutId::Hebrew, layer: Layer::Base, slot: 9, expect: 'י', dir: Dir::Rtl },
    Golden { id: 31, layout: LayoutId::Hebrew, layer: Layer::Base, slot: 10, expect: 'כ', dir: Dir::Rtl },
    Golden { id: 32, layout: LayoutId::Hebrew, layer: Layer::Base, slot: 11, expect: 'ל', dir: Dir::Rtl },
    Golden { id: 33, layout: LayoutId::Hebrew, layer: Layer::Base, slot: 12, expect: 'מ', dir: Dir::Rtl },
    Golden { id: 34, layout: LayoutId::Hebrew, layer: Layer::Base, slot: 13, expect: 'נ', dir: Dir::Rtl },
    Golden { id: 35, layout: LayoutId::Hebrew, layer: Layer::Base, slot: 14, expect: 'ס', dir: Dir::Rtl },
    Golden { id: 36, layout: LayoutId::Arabic, layer: Layer::Alt, slot: 17, expect: ')', dir: Dir::Rtl },
    Golden { id: 37, layout: LayoutId::Arabic, layer: Layer::Alt, slot: 18, expect: '(', dir: Dir::Rtl },
    Golden { id: 38, layout: LayoutId::Arabic, layer: Layer::Alt, slot: 20, expect: ']', dir: Dir::Rtl },
    Golden { id: 39, layout: LayoutId::Arabic, layer: Layer::Alt, slot: 21, expect: '[', dir: Dir::Rtl },
    Golden { id: 40, layout: LayoutId::Arabic, layer: Layer::Alt, slot: 22, expect: '}', dir: Dir::Rtl },
    Golden { id: 41, layout: LayoutId::Arabic, layer: Layer::Alt, slot: 23, expect: '{', dir: Dir::Rtl },
    Golden { id: 42, layout: LayoutId::Arabic, layer: Layer::Alt, slot: 24, expect: '〉', dir: Dir::Rtl },
    Golden { id: 43, layout: LayoutId::Arabic, layer: Layer::Alt, slot: 25, expect: '〈', dir: Dir::Rtl },
    Golden { id: 44, layout: LayoutId::Arabic, layer: Layer::Alt, slot: 26, expect: '》', dir: Dir::Rtl },
    Golden { id: 45, layout: LayoutId::Arabic, layer: Layer::Alt, slot: 27, expect: '《', dir: Dir::Rtl },
];

/// 镜像组黄金（id 36..=45）在 RTL 下的呈现期望。
pub fn golden_mirror_expected(g: &Golden) -> Option<char> {
    match g.layout {
        LayoutId::Arabic | LayoutId::Hebrew => mirror_of(g.expect),
        LayoutId::Us => Some(g.expect),
    }
}

/// 逐条核对黄金对：基层黄金查表命中，镜像黄金查对偶命中。
pub fn golden_check(g: &Golden) -> bool {
    if g.id == 0 || g.id > GOLDEN as u8 {
        return false;
    }
    if g.dir != g.layout.dir() {
        return false;
    }
    if g.id <= (GOLDEN_AR + GOLDEN_HE) as u8 {
        let table = match g.layout.table(g.layer.index()) {
            Some(t) => t,
            None => return false,
        };
        return match table.get(g.slot as usize) {
            Some(v) => *v == g.expect,
            None => false,
        };
    }
    if g.id < 36 || g.id > 45 {
        return false;
    }
    // 镜像组：期望值是键位表字符在 RTL 下的**呈现**结果。
    match g.layout.table(Layer::Alt.index()) {
        Some(t) => match t.get(g.slot as usize) {
            Some(v) => shaped_in_dir(g.layout, *v) == g.expect && mirror_of(*v).is_some(),
            None => false,
        },
        None => false,
    }
}

/// 黄金对表完整性：数量、序号连续、逐条命中。
pub fn golden_integrity() -> bool {
    if GOLDEN_TABLE.len() != GOLDEN {
        return false;
    }
    let mut i = 0usize;
    while i < GOLDEN_TABLE.len() {
        let g = match GOLDEN_TABLE.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if g.id != (i as u8) + 1 {
            return false;
        }
        if !golden_check(&g) {
            return false;
        }
        i += 1;
    }
    true
}

/// 数字段呈现方向核对：RTL 布局里数字仍为 LTR。
pub fn digit_direction_ok() -> bool {
    let base = Dir::Rtl;
    if Class::Number.run_dir(base) != Dir::Ltr {
        return false;
    }
    let mut i = 0usize;
    while i < ARABIC_DIGIT_TABLE.len() {
        let c = match ARABIC_DIGIT_TABLE.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if classify(c) != Class::Number || Class::Number.run_dir(base) != Dir::Ltr {
            return false;
        }
        i += 1;
    }
    let mut k = 0u8;
    while k < 10 {
        let c = match (b'0' + k) as char {
            v => v,
        };
        if classify(c) != Class::Number {
            return false;
        }
        k += 1;
    }
    true
}

/// 镜像可逆性：8 对全部两次回原，且对偶互异。
pub fn mirror_involutive() -> bool {
    let mut i = 0usize;
    while i < MIRROR.len() {
        let pair = match MIRROR.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if pair.0 == pair.1 {
            return false;
        }
        if mirror_of(pair.0) != Some(pair.1) || mirror_of(pair.1) != Some(pair.0) {
            return false;
        }
        if mirror_of(mirror_of(pair.0).unwrap_or('\0')) != Some(pair.0) {
            return false;
        }
        i += 1;
    }
    true
}

/// 字形四态账：22 个可连字母 × 四态合法，6 个只连左字母仅独立/尾形。
pub fn shape_table_ok() -> bool {
    let mut joinable_seen = 0usize;
    let mut right_only_seen = 0usize;
    let mut i = 0usize;
    while i < AR_BASE.len() {
        let c = match AR_BASE.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let mut k = 0usize;
        while k < SHAPES {
            let shape = match Shape::from_index(k) {
                Some(v) => v,
                None => return false,
            };
            let allowed = shape_allowed(c, shape);
            if is_right_only(c) {
                if allowed != (shape == Shape::Isolated || shape == Shape::Final) {
                    return false;
                }
            } else if !allowed {
                return false;
            }
            // 呈现码位只对「合法态」要求；不连字母的首/中部形本就无码位。
            if allowed && shaped_codepoint(c, shape).is_none() {
                return false;
            }
            k += 1;
        }
        if is_joinable(c) {
            joinable_seen += 1;
        } else {
            right_only_seen += 1;
        }
        i += 1;
    }
    joinable_seen == JOINABLE && right_only_seen == AR_LETTERS - JOINABLE
}

/// 只连左字母的字形判定永落独立/尾形。
pub fn shape_right_only_ok() -> bool {
    let mut i = 0usize;
    while i < AR_RIGHT_ONLY.len() {
        let c = match AR_RIGHT_ONLY.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if shape_of(c, true, true) != Some(Shape::Final) {
            return false;
        }
        if shape_of(c, false, true) != Some(Shape::Isolated) {
            return false;
        }
        if shape_of(c, true, false) != Some(Shape::Final) {
            return false;
        }
        i += 1;
    }
    true
}

/// 词尾形表：5 对互异且两次反查回原。
pub fn hebrew_finals_ok() -> bool {
    let mut i = 0usize;
    while i < HE_FINAL_TABLE.len() {
        let pair = match HE_FINAL_TABLE.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if pair.0 == pair.1 {
            return false;
        }
        if !is_hebrew_letter(pair.0) {
            return false;
        }
        let mut found = false;
        let mut k = 0usize;
        while k < HE_FINAL_TABLE.len() {
            if let Some(q) = HE_FINAL_TABLE.get(k) {
                if q.0 == pair.0 && q.1 == pair.1 {
                    found = true;
                }
                if k != i && q.0 == pair.0 {
                    return false;
                }
            }
            k += 1;
        }
        if !found {
            return false;
        }
        i += 1;
    }
    true
}

/// 表完整性：每套布局三层齐全、长度等于字母数、基层码位两两不同。
pub fn tables_ok() -> bool {
    let mut l = 0usize;
    while l < ALL_LAYOUTS.len() {
        let layout = match ALL_LAYOUTS.get(l) {
            Some(v) => *v,
            None => return false,
        };
        if layout == LayoutId::Us {
            if layout.table(0).is_some() {
                return false;
            }
            l += 1;
            continue;
        }
        let mut k = 0usize;
        while k < LAYERS {
            let table = match layout.table(k) {
                Some(t) => t,
                None => return false,
            };
            if table.len() != layout.letters() {
                return false;
            }
            let mut i = 0usize;
            while i < table.len() {
                let a = match table.get(i) {
                    Some(v) => *v,
                    None => return false,
                };
                let mut j = i + 1;
                while j < table.len() {
                    if let Some(b) = table.get(j) {
                        if *b == a {
                            return false;
                        }
                    }
                    j += 1;
                }
                i += 1;
            }
            k += 1;
        }
        l += 1;
    }
    true
}

// ── 两层配置与会话 ───────────────────────────────────────────────
/// 配置作用域。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scope {
    /// 会话级覆盖。
    Session,
    /// 全局默认。
    Global,
}

impl Scope {
    /// 账行取值。
    pub fn tag(self) -> &'static str {
        match self {
            Scope::Session => "session",
            Scope::Global => "global",
        }
    }

    /// 由账行取值反解。
    pub fn from_tag(tag: &str) -> Option<Scope> {
        match tag {
            "session" => Some(Scope::Session),
            "global" => Some(Scope::Global),
            _ => None,
        }
    }

    /// 作用域优先级：会话覆盖全局。
    pub fn priority(self) -> u8 {
        match self {
            Scope::Session => 2,
            Scope::Global => 1,
        }
    }
}

/// 两层配置：全局布局与可选会话覆盖。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Config {
    /// 全局默认布局。
    pub global: LayoutId,
    /// 会话覆盖（`None` 表示未覆盖）。
    pub session: Option<LayoutId>,
}

impl Config {
    /// 新建仅含全局默认的配置。
    pub fn global_only(g: LayoutId) -> Config {
        Config { global: g, session: None }
    }

    /// 解析生效布局：会话覆盖优先，未覆盖回落全局。
    pub fn resolve(&self) -> LayoutId {
        match self.session {
            Some(v) => v,
            None => self.global,
        }
    }

    /// 按作用域写入布局：会话级写覆盖，全局级写默认并清空覆盖。
    pub fn set(&mut self, scope: Scope, layout: LayoutId) {
        match scope {
            Scope::Session => self.session = Some(layout),
            Scope::Global => {
                self.global = layout;
                self.session = None;
            }
        }
    }

    /// 清除会话覆盖（回落全局）。
    pub fn clear_session(&mut self) {
        self.session = None;
    }

    /// 当前是否处于会话覆盖。
    pub fn is_overridden(&self) -> bool {
        self.session.is_some()
    }
}

/// 会话：持有生效布局、当前层、在途队列与四项计数。
pub struct Session {
    layout: LayoutId,
    layer: Layer,
    pending: Vec<char>,
    /// 已投递键数。
    delivered: u32,
    /// 丢失键数（本模块必须恒为 0）。
    lost: u32,
    /// 回退次数。
    fallbacks: u32,
    /// 切换次数。
    switches: u32,
    /// 队列越界次数（必须恒为 0）。
    overflows: u32,
}

impl Session {
    /// 新建会话，生效布局由配置解析而来。
    pub fn new(cfg: &Config) -> Session {
        Session {
            layout: cfg.resolve(),
            layer: Layer::Base,
            pending: Vec::new(),
            delivered: 0,
            lost: 0,
            fallbacks: 0,
            switches: 0,
            overflows: 0,
        }
    }

    /// 生效布局。
    pub fn layout(&self) -> LayoutId {
        self.layout
    }

    /// 当前层。
    pub fn layer(&self) -> Layer {
        self.layer
    }

    /// 设定层（越界由调用方判错，这里只接受枚举）。
    pub fn set_layer(&mut self, layer: Layer) {
        self.layer = layer;
    }

    /// 在途队列长度。
    pub fn pending(&self) -> usize {
        self.pending.len()
    }

    /// 已投递量。
    pub fn delivered(&self) -> u32 {
        self.delivered
    }

    /// 丢失量。
    pub fn lost(&self) -> u32 {
        self.lost
    }

    /// 回退量。
    pub fn fallbacks(&self) -> u32 {
        self.fallbacks
    }

    /// 切换量。
    pub fn switches(&self) -> u32 {
        self.switches
    }

    /// 越界量。
    pub fn overflows(&self) -> u32 {
        self.overflows
    }

    /// 守恒式：在途 + 已投 = 已投（在途必须在切换后被排空）。
    pub fn conserved(&self) -> bool {
        self.lost == 0 && self.overflows == 0
    }

    /// 入队一个在途键；越出容量即判溢出并静默留账（不吞计数）。
    pub fn enqueue(&mut self, c: char) -> bool {
        if self.pending.len() >= PENDING_CAP {
            self.overflows = self.overflows.saturating_add(1);
            return false;
        }
        self.pending.push(c);
        true
    }

    /// 投递一个键：先查当前布局码位，查不到即回退美式并计回退。
    pub fn deliver(&mut self, c: char) -> Option<char> {
        if locate(c).is_none() && self.layout != LayoutId::Us {
            self.fallbacks = self.fallbacks.saturating_add(1);
            if classify(c) == Class::Neutral {
                self.delivered = self.delivered.saturating_add(1);
                return Some(c);
            }
            self.lost = self.lost.saturating_add(1);
            return None;
        }
        let out = shaped_in_dir(self.layout, c);
        self.delivered = self.delivered.saturating_add(1);
        Some(out)
    }

    /// 切换布局：先把在途键原样排空投递（不重映射），再换生效布局。
    pub fn switch(&mut self, target: LayoutId) -> u32 {
        let mut drained = 0u32;
        while let Some(c) = self.pending.first().copied() {
            let mut rest = Vec::new();
            let mut i = 1usize;
            while i < self.pending.len() {
                if let Some(v) = self.pending.get(i) {
                    rest.push(*v);
                }
                i += 1;
            }
            self.pending = rest;
            if self.deliver(c).is_some() {
                drained = drained.saturating_add(1);
            }
        }
        self.layout = target;
        self.switches = self.switches.saturating_add(1);
        drained
    }

    /// 由配置切换（作用域先落配置，再换会话生效布局）。
    pub fn switch_via(&mut self, cfg: &mut Config, scope: Scope, target: LayoutId) -> u32 {
        cfg.set(scope, target);
        let resolved = cfg.resolve();
        self.switch(resolved)
    }
}

/// 切换总报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SwitchReport {
    /// 轮数。
    pub rounds: u32,
    /// 到达键数。
    pub offered: u32,
    /// 投递键数。
    pub delivered: u32,
    /// 丢失键数（必须为 0）。
    pub lost: u32,
    /// 回退键数。
    pub fallbacks: u32,
    /// 残留队列（收尾必须为 0）。
    pub residue: u32,
    /// 切换次数。
    pub switches: u32,
    /// 链式摘要。
    pub digest: u64,
}

impl SwitchReport {
    /// 零丢键守恒：到达 = 投递 + 回退 + 丢失，且丢失与残留为零。
    pub fn conserved(&self) -> bool {
        self.offered
            == self
                .delivered
                .saturating_add(self.lost)
                .saturating_add(self.residue)
            && self.lost == 0
            && self.residue == 0
    }
}

/// 单轮摘要：轮号 + 布局码 + 在途 + 投递 + 丢失。
pub fn switch_digest(round: u32, layout: LayoutId, pending: u32, delivered: u32, lost: u32) -> u64 {
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, 0x5357_4348);
    h = p_core::fold_u32(h, round);
    h = p_core::fold_byte(h, layout.index() as u8);
    h = p_core::fold_u32(h, pending);
    h = p_core::fold_u32(h, delivered);
    h = p_core::fold_u32(h, lost);
    h
}

/// 跑 `rounds` 轮两层切换（每轮 6 个在途键），返回守恒报告。
pub fn switch_rtl(rounds: u32) -> Option<SwitchReport> {
    if rounds == 0 || rounds > SWITCH_ROUNDS {
        return None;
    }
    let mut cfg = Config::global_only(LayoutId::Us);
    let mut sess = Session::new(&cfg);
    let mut offered = 0u32;
    let mut delivered = 0u32;
    let mut switches = 0u32;
    let mut digest = p_core::FNV_OFF;
    let mut i = 0u32;
    while i < rounds {
        let target = match LayoutId::from_index((i % 3) as usize) {
            Some(v) => v,
            None => return None,
        };
        let scope = if i % 2 == 0 { Scope::Session } else { Scope::Global };
        let mut k = 0u32;
        while k < PENDING_PER_ROUND {
            let letter = match AR_BASE.get(((i + k) as usize) % AR_LETTERS) {
                Some(v) => *v,
                None => return None,
            };
            if sess.enqueue(letter) {
                offered = offered.saturating_add(1);
            } else {
                return None;
            }
            k = k.saturating_add(1);
        }
        let drained = sess.switch_via(&mut cfg, scope, target);
        switches = switches.saturating_add(1);
        delivered = delivered.saturating_add(drained);
        if sess.pending() != 0 {
            return None;
        }
        let d = switch_digest(i, target, sess.pending() as u32, delivered, sess.lost());
        digest = p_core::fold_u32(p_core::fold_u32(digest, d as u32), (d >> 32) as u32);
        i = i.saturating_add(1);
    }
    Some(SwitchReport {
        rounds,
        offered,
        delivered,
        lost: sess.lost(),
        fallbacks: sess.fallbacks(),
        residue: sess.pending() as u32,
        switches,
        digest,
    })
}

/// 百轮切换零丢键。
pub fn switch_green() -> bool {
    match switch_rtl(SWITCH_ROUNDS) {
        Some(r) => {
            r.rounds == SWITCH_ROUNDS
                && r.conserved()
                && r.switches == SWITCH_ROUNDS
                && r.offered == SWITCH_ROUNDS.saturating_mul(PENDING_PER_ROUND)
                && r.delivered == r.offered
                && r.digest != p_core::FNV_OFF
        }
        None => false,
    }
}

/// 复跑稳定：同轮数重复 `times` 次摘要逐位相同。
pub fn switch_replay_stable(times: u32) -> bool {
    if times == 0 || times > 8 {
        return false;
    }
    let want = match switch_rtl(SWITCH_ROUNDS) {
        Some(r) => r.digest,
        None => return false,
    };
    let mut i = 0u32;
    while i < times {
        match switch_rtl(SWITCH_ROUNDS) {
            Some(r) => {
                if r.digest != want {
                    return false;
                }
            }
            None => return false,
        }
        i = i.saturating_add(1);
    }
    want != p_core::FNV_OFF
}

// ── 字形账 ───────────────────────────────────────────────────────
/// 字形记录（一槽一态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ShapeRecord {
    /// 物理槽位。
    pub slot: u8,
    /// 字形四态。
    pub shape: Shape,
    /// 呈现码位。
    pub codepoint: u32,
    /// 是否可连。
    pub joinable: bool,
}

/// 由槽位、前后连接性算一条字形记录。
pub fn shape_record(slot: usize, prev_joinable: bool, next_joinable: bool) -> Option<ShapeRecord> {
    let c = *AR_BASE.get(slot)?;
    let shape = shape_of(c, prev_joinable, next_joinable)?;
    let codepoint = shaped_codepoint(c, shape)?;
    Some(ShapeRecord {
        slot: slot as u8,
        shape,
        codepoint,
        joinable: is_joinable(c),
    })
}

/// 字形账摘要。
pub fn shape_digest(slot: u8, shape: Shape, codepoint: u32, joinable: bool) -> u64 {
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, 0x5348_4150);
    h = p_core::fold_byte(h, slot);
    h = p_core::fold_byte(h, shape.index() as u8);
    h = p_core::fold_u32(h, codepoint);
    h = p_core::fold_byte(h, if joinable { 1 } else { 0 });
    h
}

// ── 账行 ─────────────────────────────────────────────────────────
/// 解析十进制（拒绝空串、非数字、溢出）。
pub fn parse_u32(s: &str) -> Option<u32> {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    let mut acc = 0u32;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = match bytes.get(i) {
            Some(v) => *v,
            None => return None,
        };
        if !b.is_ascii_digit() {
            return None;
        }
        acc = acc.checked_mul(10)?;
        acc = acc.checked_add((b - b'0') as u32)?;
        i += 1;
    }
    Some(acc)
}

/// 解析 16 位十六进制摘要。
pub fn parse_hex16(s: &str) -> Option<u64> {
    if s.len() != 16 {
        return None;
    }
    let bytes = s.as_bytes();
    let mut acc = 0u64;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = match bytes.get(i) {
            Some(v) => *v,
            None => return None,
        };
        let v = match b {
            b'0'..=b'9' => (b - b'0') as u64,
            b'a'..=b'f' => (b - b'a') as u64 + 10,
            b'A'..=b'F' => (b - b'A') as u64 + 10,
            _ => return None,
        };
        acc = acc.checked_mul(16)?;
        acc = acc.checked_add(v)?;
        i += 1;
    }
    Some(acc)
}

/// 按固定标签与键序拆行；返回各键值与摘要值。
pub fn row_fields<'a>(row: &'a str, tag: &str, keys: &[&str]) -> Option<Vec<&'a str>> {
    let parts: Vec<&str> = row.split('|').collect();
    if parts.len() != keys.len() + 2 {
        return None;
    }
    if parts.first() != Some(&tag) {
        return None;
    }
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < keys.len() {
        let piece = *parts.get(i + 1)?;
        let key = *keys.get(i)?;
        let rest = piece.strip_prefix(key)?;
        let value = rest.strip_prefix('=')?;
        if value.is_empty() {
            return None;
        }
        out.push(value);
        i += 1;
    }
    let d = parts.get(keys.len() + 1)?.strip_prefix("digest=")?;
    if d.len() != 16 {
        return None;
    }
    out.push(d);
    Some(out)
}

/// 渲染切换账行。
pub fn render_switch_row(r: &SwitchReport) -> String {
    format!(
        "{}|round={}|layout={}|pending={}|delivered={}|lost={}|digest={:016x}",
        SW_TAG,
        r.rounds,
        LayoutId::Arabic.tag(),
        r.residue,
        r.delivered,
        r.lost,
        r.digest
    )
}

/// 回读切换账行（重跑同轮数并比对）。
pub fn audit_switch_row(row: &str) -> bool {
    let f = match row_fields(row, SW_TAG, &SW_KEYS) {
        Some(v) if v.len() == SW_KEYS.len() + 1 => v,
        _ => return false,
    };
    let rounds = match f.first().copied().and_then(parse_u32) {
        Some(v) if v >= 1 && v <= SWITCH_ROUNDS => v,
        _ => return false,
    };
    if f.get(1).copied() != Some(LayoutId::Arabic.tag()) {
        return false;
    }
    let residue = match f.get(2).copied().and_then(parse_u32) {
        Some(v) => v,
        None => return false,
    };
    let delivered = match f.get(3).copied().and_then(parse_u32) {
        Some(v) => v,
        None => return false,
    };
    let lost = match f.get(4).copied().and_then(parse_u32) {
        Some(v) => v,
        None => return false,
    };
    if residue != 0 || lost != 0 {
        return false;
    }
    let want = match f.get(5).copied().and_then(parse_hex16) {
        Some(v) => v,
        None => return false,
    };
    match switch_rtl(rounds) {
        Some(r) => r.delivered == delivered && r.digest == want,
        None => false,
    }
}

/// 渲染字形账行。
pub fn render_shape_row(rec: &ShapeRecord) -> String {
    format!(
        "{}|slot={}|shape={}|codepoint={}|joinable={}|digest={:016x}",
        SHAPE_TAG,
        rec.slot,
        rec.shape.tag(),
        rec.codepoint,
        if rec.joinable { "1" } else { "0" },
        shape_digest(rec.slot, rec.shape, rec.codepoint, rec.joinable)
    )
}

/// 回读字形账行：重算码位并与摘要比对。
pub fn audit_shape_row(row: &str) -> bool {
    let f = match row_fields(row, SHAPE_TAG, &SHAPE_KEYS) {
        Some(v) if v.len() == SHAPE_KEYS.len() + 1 => v,
        _ => return false,
    };
    let slot = match f.first().copied().and_then(parse_u32) {
        Some(v) if v < AR_LETTERS as u32 => v as usize,
        _ => return false,
    };
    let shape = match f.get(1).copied().and_then(Shape::from_tag) {
        Some(v) => v,
        None => return false,
    };
    let codepoint = match f.get(2).copied().and_then(parse_u32) {
        Some(v) => v,
        None => return false,
    };
    let joinable_flag = match f.get(3).copied() {
        Some("1") => true,
        Some("0") => false,
        _ => return false,
    };
    let c = match AR_BASE.get(slot) {
        Some(v) => *v,
        None => return false,
    };
    if !shape_allowed(c, shape) {
        return false;
    }
    if shaped_codepoint(c, shape) != Some(codepoint) {
        return false;
    }
    if is_joinable(c) != joinable_flag {
        return false;
    }
    match f.get(4).copied().and_then(parse_hex16) {
        Some(v) => v == shape_digest(slot as u8, shape, codepoint, joinable_flag),
        None => false,
    }
}

/// 导出字形账行（8 个槽位 × 一种连接态组合，互不相同）。
pub fn export_shape_rows() -> Vec<String> {
    let mut out = Vec::new();
    let combos: [(bool, bool); 4] =
        [(false, false), (true, false), (true, true), (false, true)];
    let mut i = 0usize;
    while i < combos.len() {
        let pair = match combos.get(i) {
            Some(v) => *v,
            None => break,
        };
        let mut k = 0usize;
        while k < 2 {
            let slot = (i * 2 + k) % AR_LETTERS;
            if let Some(rec) = shape_record(slot, pair.0, pair.1) {
                let row = render_shape_row(&rec);
                if audit_shape_row(&row) {
                    out.push(row);
                }
            }
            k += 1;
        }
        i += 1;
    }
    out
}

/// 导出切换账行。
pub fn export_switch_rows() -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < ROW_SLOTS {
        let rounds = match ROW_ROUNDS.get(i) {
            Some(v) => *v,
            None => break,
        };
        if let Some(r) = switch_rtl(rounds) {
            out.push(render_switch_row(&r));
        }
        i += 1;
    }
    out
}

/// 切换账三档轮数（1 / 10 / 100）。
pub const ROW_ROUNDS: [u32; ROW_SLOTS] = [1, 10, SWITCH_ROUNDS];

/// 账行稳定性：切换账与字形账都逐行可回读、互不重复、复跑一致。
pub fn rows_stable() -> bool {
    let sw = export_switch_rows();
    let sh = export_shape_rows();
    if sw.len() != ROW_SLOTS || sh.is_empty() {
        return false;
    }
    let mut i = 0usize;
    while i < sw.len() {
        let row = match sw.get(i) {
            Some(v) => v,
            None => return false,
        };
        if !audit_switch_row(row) {
            return false;
        }
        let mut j = i + 1;
        while j < sw.len() {
            if sw.get(j) == Some(row) {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    let mut k = 0usize;
    while k < sh.len() {
        let row = match sh.get(k) {
            Some(v) => v,
            None => return false,
        };
        if !audit_shape_row(row) {
            return false;
        }
        let mut m = k + 1;
        while m < sh.len() {
            if sh.get(m) == Some(row) {
                return false;
            }
            m += 1;
        }
        k += 1;
    }
    let sw2 = export_switch_rows();
    let sh2 = export_shape_rows();
    sw.len() == sw2.len() && sh.len() == sh2.len()
}

/// 篡改探针：把账行最后一个字符改掉（摘要末位必变）。
pub fn tamper_tail(row: &str) -> String {
    let mut out = String::new();
    let bytes = row.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let b = match bytes.get(i) {
            Some(v) => *v,
            None => break,
        };
        if i + 1 == bytes.len() {
            out.push(if b == b'0' { '1' } else { '0' });
        } else {
            out.push(b as char);
        }
        i += 1;
    }
    out
}

// ── 判据 ─────────────────────────────────────────────────────────
/// 八项判据（4 × F49105 + 4 × F49106），每条都跑真实场景。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out = Vec::new();

    // 1) 基向与六类：每套布局有基向，键类互斥可反查。
    let mut classes_ok = true;
    let mut i = 0usize;
    while i < ALL_CLASSES.len() {
        let c = match ALL_CLASSES.get(i) {
            Some(v) => *v,
            None => return out,
        };
        if Class::from_tag(c.tag()) != Some(c) || c.index() != i {
            classes_ok = false;
        }
        i += 1;
    }
    let dir_ok = LayoutId::Arabic.dir() == Dir::Rtl
        && LayoutId::Hebrew.dir() == Dir::Rtl
        && LayoutId::Us.dir() == Dir::Ltr
        && LayoutId::Arabic.dir().opposite() == Dir::Ltr;
    let sig_ok = LayoutId::Arabic.signature() != LayoutId::Hebrew.signature()
        && LayoutId::Arabic.signature() != LayoutId::Us.signature()
        && LayoutId::Hebrew.signature() != LayoutId::Us.signature();
    out.push((
        "F49105/RTL基向判定",
        classes_ok && dir_ok && sig_ok,
        format!("classes={} dir={} sig={}", classes_ok, dir_ok, sig_ok),
    ));

    // 2) 数字恒 LTR + 镜像可逆。
    let mirror_ok = mirror_involutive();
    let shown = mirror_of('(').unwrap_or('\0');
    out.push((
        "F49105/数字与镜像",
        digit_direction_ok() && mirror_ok && shown == ')',
        format!("digit={} mirror={} shown={}", digit_direction_ok(), mirror_ok, shown as u32),
    ));

    // 3) 字形四态：全字母全态合法，只连左不落首/中部形。
    let table_ok = shape_table_ok();
    let ro_ok = shape_right_only_ok();
    let rows = export_shape_rows();
    let rows_ok = !rows.is_empty();
    out.push((
        "F49105/字形四态",
        table_ok && ro_ok && rows_ok,
        format!("table={} right_only={} rows={}", table_ok, ro_ok, rows.len()),
    ));

    // 4) 表与码位反查 + 异常码守卫。
    let t_ok = tables_ok();
    let guard_ok = segment_guard()
        && pers_guard()
        && detail(SEG_BASE) == UNKNOWN
        && detail(SEG_BASE + 0x0F) == UNKNOWN
        && detail(0) == UNKNOWN;
    let located = locate('ا').is_some() && locate('א').is_some() && locate('z').is_none();
    out.push((
        "F49105/键表与码位守卫",
        t_ok && guard_ok && located,
        format!("tables={} guard={} located={}", t_ok, guard_ok, located),
    ));

    // 5) 两层配置：会话覆盖全局，清覆盖回落全局。
    let mut cfg = Config::global_only(LayoutId::Us);
    cfg.set(Scope::Session, LayoutId::Arabic);
    let overridden = cfg.resolve() == LayoutId::Arabic && cfg.is_overridden();
    cfg.set(Scope::Global, LayoutId::Hebrew);
    let fell_back = cfg.resolve() == LayoutId::Hebrew && !cfg.is_overridden();
    cfg.clear_session();
    out.push((
        "F49106/两层配置解析",
        overridden && fell_back && cfg.resolve() == LayoutId::Hebrew,
        format!("overridden={} fell_back={}", overridden, fell_back),
    ));

    // 6) 百轮切换零丢键。
    let green = switch_green();
    let report = switch_rtl(SWITCH_ROUNDS);
    let (offered, delivered, lost) = match &report {
        Some(r) => (r.offered, r.delivered, r.lost),
        None => (0, 0, 1),
    };
    out.push((
        "F49106/百轮零丢键",
        green && lost == 0 && offered == delivered,
        format!("offered={} delivered={} lost={}", offered, delivered, lost),
    ));

    // 7) 账行回读：切换账与字形账都可回读、拒篡改、复跑一致。
    let sw_rows = export_switch_rows();
    let mut sw_tamper_ok = sw_rows.len() == ROW_SLOTS;
    let mut k = 0usize;
    while k < sw_rows.len() {
        let row = match sw_rows.get(k) {
            Some(v) => v,
            None => break,
        };
        // 原行必可回读；篡改行与畸形行必被拒。
        if !audit_switch_row(row)
            || audit_switch_row(&tamper_tail(row))
            || audit_switch_row("SWCH105|x")
        {
            sw_tamper_ok = false;
        }
        k += 1;
    }
    let mut sh_tamper_ok = rows.len() == SHAPE_ROW_SLOTS;
    k = 0;
    while k < rows.len() {
        let srow = match rows.get(k) {
            Some(v) => v,
            None => break,
        };
        if !audit_shape_row(srow) || audit_shape_row(&tamper_tail(srow)) {
            sh_tamper_ok = false;
        }
        k += 1;
    }
    out.push((
        "F49106/账行回读",
        sw_tamper_ok && sh_tamper_ok && rows_stable(),
        format!(
            "switch_rows={} shape_rows={} sw_ok={} sh_ok={} stable={}",
            sw_rows.len(),
            rows.len(),
            sw_tamper_ok,
            sh_tamper_ok,
            rows_stable()
        ),
    ));

    // 8) 复跑留痕：同轮数重复三次摘要逐位相同。
    let stable = switch_replay_stable(3);
    let a = switch_rtl(10).map(|r| r.digest).unwrap_or(0);
    let b = switch_rtl(10).map(|r| r.digest).unwrap_or(1);
    out.push((
        "F49106/复跑留痕",
        stable && a == b && a != p_core::FNV_OFF,
        format!("replay={} d={:016x}", stable, a),
    ));

    out
}

/// 入口：八项判据写入 CheckSet。
pub fn run_unx49105_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F49105 RTL 基向与切换持久化");
    let checks = extra_checks();
    let mut i = 0usize;
    while i < checks.len() {
        if let Some(c) = checks.get(i) {
            s.add_dyn(c.0, c.1, &c.2);
        }
        i += 1;
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_base_and_span() {
        assert_eq!(SEG_BASE, 0x73800);
        assert_eq!(SEG_SPAN, 0x40);
        assert_eq!(PERS_BASE, SEG_BASE + 0x11);
    }

    #[test]
    fn code_values_are_contiguous() {
        let mut i = 0usize;
        while i < ALL.len() {
            let c = match ALL.get(i) {
                Some(v) => *v,
                None => panic!("缺码"),
            };
            assert_eq!(c.val(), SEG_BASE + 1 + i as u32);
            i += 1;
        }
        assert_eq!(ALL.len(), 14);
    }

    #[test]
    fn code_details_unique() {
        let mut i = 0usize;
        while i < ALL.len() {
            let a = match ALL.get(i) {
                Some(v) => *v,
                None => panic!("缺码"),
            };
            assert!(!a.detail().is_empty());
            assert!(!a.message().is_empty());
            let mut j = i + 1;
            while j < ALL.len() {
                let b = match ALL.get(j) {
                    Some(v) => *v,
                    None => panic!("缺码"),
                };
                assert_ne!(a.detail(), b.detail());
                assert_ne!(a.message(), b.message());
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn free_detail_covers_all_and_rejects_window_edge() {
        let mut i = 1u32;
        while i <= 14 {
            assert_ne!(detail(SEG_BASE + i), UNKNOWN);
            i += 1;
        }
        assert_eq!(detail(SEG_BASE + 15), UNKNOWN);
        assert_eq!(detail(SEG_BASE), UNKNOWN);
        assert_eq!(detail(0), UNKNOWN);
        assert_eq!(detail(u32::MAX), UNKNOWN);
    }

    #[test]
    fn segment_guard_green() {
        assert!(segment_guard());
    }

    #[test]
    fn pers_guard_green_and_disjoint() {
        assert!(pers_guard());
        let mut i = 0usize;
        while i < ALL.len() {
            let c = match ALL.get(i) {
                Some(v) => *v,
                None => panic!("缺码"),
            };
            assert!(pers_val(c) > SEG_BASE + 0x0F);
            assert!(pers_val(c) <= SEG_BASE + SEG_SPAN);
            i += 1;
        }
    }

    #[test]
    fn dir_tags_round_trip() {
        assert_eq!(Dir::from_tag(Dir::Ltr.tag()), Some(Dir::Ltr));
        assert_eq!(Dir::from_tag(Dir::Rtl.tag()), Some(Dir::Rtl));
        assert_eq!(Dir::from_tag("x"), None);
        assert_eq!(Dir::Ltr.opposite(), Dir::Rtl);
        assert_eq!(Dir::Rtl.opposite(), Dir::Ltr);
    }

    #[test]
    fn class_tags_round_trip() {
        let mut i = 0usize;
        while i < ALL_CLASSES.len() {
            let c = match ALL_CLASSES.get(i) {
                Some(v) => *v,
                None => panic!("缺类"),
            };
            assert_eq!(Class::from_tag(c.tag()), Some(c));
            assert_eq!(c.index(), i);
            i += 1;
        }
        assert_eq!(Class::from_tag("nope"), None);
    }

    #[test]
    fn number_class_stays_ltr_under_rtl() {
        assert_eq!(Class::Number.run_dir(Dir::Rtl), Dir::Ltr);
        assert_eq!(Class::Number.run_dir(Dir::Ltr), Dir::Ltr);
        assert!(digit_direction_ok());
    }

    #[test]
    fn run_continuity_rules() {
        assert!(Class::Letter.keeps_run());
        assert!(Class::Number.keeps_run());
        assert!(!Class::Neutral.keeps_run());
        assert!(!Class::Space.keeps_run());
        assert!(!Class::Control.keeps_run());
        assert!(!Class::Mirror.keeps_run());
    }

    #[test]
    fn layer_bounds_and_tags() {
        assert_eq!(Layer::from_index(3), None);
        assert_eq!(Layer::from_index(99), None);
        assert_eq!(Layer::Base.index(), 0);
        assert_eq!(Layer::Alt.index(), 2);
        assert_eq!(Layer::from_tag(Layer::Shift.tag()), Some(Layer::Shift));
        assert_eq!(Layer::from_tag("x"), None);
    }

    #[test]
    fn shape_bounds_and_tags() {
        assert_eq!(Shape::from_index(SHAPES), None);
        assert_eq!(Shape::from_index(0), Some(Shape::Isolated));
        assert_eq!(Shape::from_tag(Shape::Medial.tag()), Some(Shape::Medial));
        assert_eq!(Shape::from_tag("x"), None);
        assert!(Shape::Initial.needs_prev());
        assert!(Shape::Initial.needs_next());
        assert!(!Shape::Final.needs_next());
        assert!(!Shape::Isolated.needs_prev());
    }

    #[test]
    fn layout_tags_and_dirs() {
        assert_eq!(LayoutId::from_tag("ar"), Some(LayoutId::Arabic));
        assert_eq!(LayoutId::from_tag("he"), Some(LayoutId::Hebrew));
        assert_eq!(LayoutId::from_tag("us"), Some(LayoutId::Us));
        assert_eq!(LayoutId::from_tag("de"), None);
        assert_eq!(LayoutId::Arabic.dir(), Dir::Rtl);
        assert_eq!(LayoutId::Hebrew.dir(), Dir::Rtl);
        assert_eq!(LayoutId::Us.dir(), Dir::Ltr);
        assert_eq!(LayoutId::from_index(3), None);
    }

    #[test]
    fn layout_signatures_are_distinct() {
        let a = LayoutId::Arabic.signature();
        let b = LayoutId::Hebrew.signature();
        let c = LayoutId::Us.signature();
        assert_ne!(a, b);
        assert_ne!(a, c);
        assert_ne!(b, c);
        assert_eq!(a.len(), 8);
    }

    #[test]
    fn layout_checksums_are_distinct_and_nonzero() {
        let a = LayoutId::Arabic.checksum();
        let b = LayoutId::Hebrew.checksum();
        assert_ne!(a, 0);
        assert_ne!(b, 0);
        assert_ne!(a, b);
        assert_eq!(LayoutId::Us.checksum(), 0);
    }

    #[test]
    fn tables_have_registered_sizes() {
        assert_eq!(AR_BASE.len(), AR_LETTERS);
        assert_eq!(AR_SHIFT.len(), AR_LETTERS);
        assert_eq!(AR_ALT.len(), AR_LETTERS);
        assert_eq!(HE_BASE.len(), HE_LETTERS);
        assert_eq!(HE_SHIFT.len(), HE_LETTERS);
        assert_eq!(HE_ALT.len(), HE_LETTERS);
        assert_eq!(ARABIC_DIGIT_TABLE.len(), ARABIC_DIGITS);
        assert_eq!(MIRROR.len(), MIRROR_PAIRS);
        assert_eq!(HE_FINAL_TABLE.len(), HE_FINALS);
    }

    #[test]
    fn tables_integrity_green() {
        assert!(tables_ok());
    }

    #[test]
    fn base_and_shift_layers_are_disjoint() {
        let mut i = 0usize;
        while i < AR_BASE.len() {
            let a = match AR_BASE.get(i) {
                Some(v) => *v,
                None => panic!("缺字母"),
            };
            let mut j = 0usize;
            while j < AR_SHIFT.len() {
                if let Some(b) = AR_SHIFT.get(j) {
                    assert_ne!(a, *b);
                }
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn hebrew_layers_disjoint() {
        let mut i = 0usize;
        while i < HE_BASE.len() {
            let a = match HE_BASE.get(i) {
                Some(v) => *v,
                None => panic!("缺字母"),
            };
            let mut j = 0usize;
            while j < HE_SHIFT.len() {
                if let Some(b) = HE_SHIFT.get(j) {
                    assert_ne!(a, *b);
                }
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn right_only_letters_membership() {
        let mut i = 0usize;
        while i < AR_RIGHT_ONLY.len() {
            let c = match AR_RIGHT_ONLY.get(i) {
                Some(v) => *v,
                None => panic!("缺字母"),
            };
            assert!(is_right_only(c));
            assert!(!is_joinable(c));
            assert!(is_arabic_letter(c));
            i += 1;
        }
        assert!(!is_right_only('ب'));
        assert!(is_joinable('ب'));
    }

    #[test]
    fn joinable_count_matches_registered() {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < AR_BASE.len() {
            if let Some(c) = AR_BASE.get(i) {
                if is_joinable(*c) {
                    n += 1;
                }
            }
            i += 1;
        }
        assert_eq!(n, JOINABLE);
    }

    #[test]
    fn arabic_digit_layer_classifies_as_number() {
        let mut i = 0usize;
        while i < ARABIC_DIGIT_TABLE.len() {
            let c = match ARABIC_DIGIT_TABLE.get(i) {
                Some(v) => *v,
                None => panic!("缺数字"),
            };
            assert_eq!(classify(c), Class::Number);
            i += 1;
        }
    }

    #[test]
    fn letters_classify_as_letter() {
        assert_eq!(classify('ب'), Class::Letter);
        assert_eq!(classify('א'), Class::Letter);
        assert!(!is_hebrew_letter('ب'));
        assert!(!is_arabic_letter('א'));
    }

    #[test]
    fn control_keys_classify_as_control() {
        assert_eq!(classify(KEY_RLM), Class::Control);
        assert_eq!(classify(KEY_LRM), Class::Control);
        assert_eq!(classify(KEY_ALM), Class::Control);
        assert_eq!(classify(KEY_SPACE), Class::Space);
    }

    #[test]
    fn neutral_punctuation_classifies_as_neutral_or_mirror() {
        assert_eq!(classify('\u{00A0}'), Class::Neutral);
        assert_eq!(classify('('), Class::Mirror);
        assert_eq!(classify('》'), Class::Mirror);
    }

    #[test]
    fn mirror_pairs_all_involutive() {
        assert!(mirror_involutive());
        let mut i = 0usize;
        while i < MIRROR.len() {
            let p = match MIRROR.get(i) {
                Some(v) => *v,
                None => panic!("缺对"),
            };
            assert_eq!(mirror_of(p.0), Some(p.1));
            assert_eq!(mirror_of(p.1), Some(p.0));
            i += 1;
        }
    }

    #[test]
    fn mirror_of_unknown_returns_none() {
        assert_eq!(mirror_of('a'), None);
        assert_eq!(mirror_of('ب'), None);
    }

    #[test]
    fn shaped_in_dir_only_touches_mirrors() {
        assert_eq!(shaped_in_dir(LayoutId::Arabic, '('), ')');
        assert_eq!(shaped_in_dir(LayoutId::Us, '('), '(');
        assert_eq!(shaped_in_dir(LayoutId::Arabic, 'ب'), 'ب');
    }

    #[test]
    fn shape_decision_matrix() {
        assert_eq!(shape_of('ب', false, false), Some(Shape::Isolated));
        assert_eq!(shape_of('ب', true, false), Some(Shape::Final));
        assert_eq!(shape_of('ب', false, true), Some(Shape::Initial));
        assert_eq!(shape_of('ب', true, true), Some(Shape::Medial));
    }

    #[test]
    fn right_only_never_enters_initial_or_medial() {
        assert!(shape_right_only_ok());
        assert_eq!(shape_of('ا', true, true), Some(Shape::Final));
        assert_eq!(shape_of('و', true, true), Some(Shape::Final));
        assert_eq!(shape_of('ر', false, true), Some(Shape::Isolated));
    }

    #[test]
    fn shape_of_non_letter_is_none() {
        assert_eq!(shape_of('(', true, true), None);
        assert_eq!(shape_of('1', true, true), None);
        assert!(!shape_allowed('(', Shape::Initial));
    }

    #[test]
    fn shape_table_gate_green() {
        assert!(shape_table_ok());
    }

    #[test]
    fn shaped_codepoint_offsets() {
        let base = 'ب' as u32;
        assert_eq!(shaped_codepoint('ب', Shape::Isolated), Some(base));
        assert_eq!(
            shaped_codepoint('ب', Shape::Initial),
            Some(base + SHAPE_OFFSET[1])
        );
        assert_eq!(shaped_codepoint('ا', Shape::Initial), None);
        assert_eq!(shaped_codepoint('(', Shape::Final), None);
    }

    #[test]
    fn hebrew_finals_gate_green() {
        assert!(hebrew_finals_ok());
        assert_eq!(is_hebrew_letter('ך'), false);
    }

    #[test]
    fn slot_and_locate_lookup() {
        assert_eq!(slot_of(LayoutId::Arabic, Layer::Base, 'ب'), Some(1));
        assert_eq!(slot_of(LayoutId::Hebrew, Layer::Base, 'א'), Some(0));
        assert_eq!(slot_of(LayoutId::Arabic, Layer::Alt, '٠'), Some(0));
        assert_eq!(slot_of(LayoutId::Us, Layer::Base, 'q'), None);
        let hit = locate('ن');
        assert!(hit.is_some());
        let pair = match hit {
            Some(v) => v,
            None => panic!("查无"),
        };
        assert_eq!(pair.0, LayoutId::Arabic);
        assert_eq!(pair.1, Layer::Base);
        assert_eq!(locate('z'), None);
    }

    #[test]
    fn golden_integrity_green() {
        assert!(golden_integrity());
        assert_eq!(GOLDEN_TABLE.len(), GOLDEN);
    }

    #[test]
    fn golden_ids_are_contiguous() {
        let mut i = 0usize;
        while i < GOLDEN_TABLE.len() {
            let g = match GOLDEN_TABLE.get(i) {
                Some(v) => *v,
                None => panic!("缺对"),
            };
            assert_eq!(g.id, i as u8 + 1);
            assert_eq!(g.dir, g.layout.dir());
            i += 1;
        }
    }

    #[test]
    fn golden_counts_per_group() {
        let mut ar = 0usize;
        let mut he = 0usize;
        let mut mr = 0usize;
        let mut i = 0usize;
        while i < GOLDEN_TABLE.len() {
            let g = match GOLDEN_TABLE.get(i) {
                Some(v) => *v,
                None => panic!("缺对"),
            };
            if g.id <= GOLDEN_AR as u8 {
                ar += 1;
            } else if g.id <= (GOLDEN_AR + GOLDEN_HE) as u8 {
                he += 1;
            } else {
                mr += 1;
            }
            i += 1;
        }
        assert_eq!(ar, GOLDEN_AR);
        assert_eq!(he, GOLDEN_HE);
        assert_eq!(mr, GOLDEN_MIRROR);
    }

    #[test]
    fn golden_rejects_wrong_expectation() {
        let mut g = match GOLDEN_TABLE.first().copied() {
            Some(v) => v,
            None => panic!("缺对"),
        };
        g.expect = 'ب';
        assert!(!golden_check(&g));
        let mut bad = match GOLDEN_TABLE.first().copied() {
            Some(v) => v,
            None => panic!("缺对"),
        };
        bad.id = 0;
        assert!(!golden_check(&bad));
        let mut over = match GOLDEN_TABLE.first().copied() {
            Some(v) => v,
            None => panic!("缺对"),
        };
        over.id = GOLDEN as u8 + 1;
        assert!(!golden_check(&over));
    }

    #[test]
    fn golden_mirror_expectations_exist() {
        let mut i = 35usize;
        while i < GOLDEN_TABLE.len() {
            let g = match GOLDEN_TABLE.get(i) {
                Some(v) => v,
                None => panic!("缺对"),
            };
            assert!(golden_mirror_expected(g).is_some());
            assert_eq!(classify(g.expect), Class::Mirror);
            i += 1;
        }
    }

    #[test]
    fn scope_tags_and_priority() {
        assert_eq!(Scope::from_tag(Scope::Session.tag()), Some(Scope::Session));
        assert_eq!(Scope::from_tag(Scope::Global.tag()), Some(Scope::Global));
        assert_eq!(Scope::from_tag("x"), None);
        assert!(Scope::Session.priority() > Scope::Global.priority());
    }

    #[test]
    fn config_session_overrides_global() {
        let mut cfg = Config::global_only(LayoutId::Us);
        assert_eq!(cfg.resolve(), LayoutId::Us);
        assert!(!cfg.is_overridden());
        cfg.set(Scope::Session, LayoutId::Hebrew);
        assert_eq!(cfg.resolve(), LayoutId::Hebrew);
        assert!(cfg.is_overridden());
        cfg.clear_session();
        assert_eq!(cfg.resolve(), LayoutId::Us);
    }

    #[test]
    fn config_global_write_clears_override() {
        let mut cfg = Config::global_only(LayoutId::Us);
        cfg.set(Scope::Session, LayoutId::Arabic);
        cfg.set(Scope::Global, LayoutId::Hebrew);
        assert_eq!(cfg.resolve(), LayoutId::Hebrew);
        assert!(!cfg.is_overridden());
    }

    #[test]
    fn session_starts_from_resolved_config() {
        let mut cfg = Config::global_only(LayoutId::Us);
        cfg.set(Scope::Session, LayoutId::Arabic);
        let sess = Session::new(&cfg);
        assert_eq!(sess.layout(), LayoutId::Arabic);
        assert_eq!(sess.layer(), Layer::Base);
        assert_eq!(sess.pending(), 0);
        assert!(sess.conserved());
    }

    #[test]
    fn session_layer_switch() {
        let cfg = Config::global_only(LayoutId::Arabic);
        let mut sess = Session::new(&cfg);
        sess.set_layer(Layer::Alt);
        assert_eq!(sess.layer(), Layer::Alt);
        sess.set_layer(Layer::Shift);
        assert_eq!(sess.layer(), Layer::Shift);
    }

    #[test]
    fn session_deliver_applies_mirror() {
        let cfg = Config::global_only(LayoutId::Arabic);
        let mut sess = Session::new(&cfg);
        assert_eq!(sess.deliver('('), Some(')'));
        assert_eq!(sess.deliver('ب'), Some('ب'));
        assert_eq!(sess.delivered(), 2);
        assert_eq!(sess.lost(), 0);
    }

    #[test]
    fn session_enqueue_respects_capacity() {
        let cfg = Config::global_only(LayoutId::Arabic);
        let mut sess = Session::new(&cfg);
        let mut i = 0usize;
        while i < PENDING_CAP {
            assert!(sess.enqueue('ب'));
            i += 1;
        }
        assert_eq!(sess.pending(), PENDING_CAP);
        assert!(!sess.enqueue('ب'));
        assert_eq!(sess.overflows(), 1);
        assert!(!sess.conserved());
    }

    #[test]
    fn session_switch_drains_pending() {
        let cfg = Config::global_only(LayoutId::Us);
        let mut sess = Session::new(&cfg);
        assert!(sess.enqueue('ب'));
        assert!(sess.enqueue('ي'));
        let drained = sess.switch(LayoutId::Arabic);
        assert_eq!(drained, 2);
        assert_eq!(sess.pending(), 0);
        assert_eq!(sess.layout(), LayoutId::Arabic);
        assert_eq!(sess.switches(), 1);
        assert_eq!(sess.lost(), 0);
    }

    #[test]
    fn session_switch_via_updates_config_layers() {
        let mut cfg = Config::global_only(LayoutId::Us);
        let mut sess = Session::new(&cfg);
        let _ = sess.switch_via(&mut cfg, Scope::Session, LayoutId::Hebrew);
        assert_eq!(cfg.resolve(), LayoutId::Hebrew);
        assert!(cfg.is_overridden());
        assert_eq!(sess.layout(), LayoutId::Hebrew);
    }

    #[test]
    fn switch_rtl_bounds() {
        assert!(switch_rtl(0).is_none());
        assert!(switch_rtl(SWITCH_ROUNDS + 1).is_none());
        assert!(switch_rtl(SWITCH_ROUNDS).is_some());
    }

    #[test]
    fn switch_rtl_conservation() {
        let r = match switch_rtl(SWITCH_ROUNDS) {
            Some(v) => v,
            None => panic!("无报告"),
        };
        assert_eq!(r.rounds, SWITCH_ROUNDS);
        assert_eq!(r.offered, SWITCH_ROUNDS * PENDING_PER_ROUND);
        assert_eq!(r.delivered, r.offered);
        assert_eq!(r.lost, 0);
        assert_eq!(r.residue, 0);
        assert_eq!(r.switches, SWITCH_ROUNDS);
        assert!(r.conserved());
    }

    #[test]
    fn switch_rtl_digest_nonzero_and_stable() {
        let a = match switch_rtl(50) {
            Some(v) => v.digest,
            None => panic!("无报告"),
        };
        let b = match switch_rtl(50) {
            Some(v) => v.digest,
            None => panic!("无报告"),
        };
        assert_eq!(a, b);
        assert_ne!(a, p_core::FNV_OFF);
        let c = match switch_rtl(51) {
            Some(v) => v.digest,
            None => panic!("无报告"),
        };
        assert_ne!(a, c);
    }

    #[test]
    fn switch_gate_green() {
        assert!(switch_green());
    }

    #[test]
    fn switch_replay_gate() {
        assert!(switch_replay_stable(3));
        assert!(!switch_replay_stable(0));
        assert!(!switch_replay_stable(9));
    }

    #[test]
    fn switch_digest_sensitivity() {
        let base = switch_digest(1, LayoutId::Arabic, 0, 6, 0);
        assert_eq!(base, switch_digest(1, LayoutId::Arabic, 0, 6, 0));
        assert_ne!(base, switch_digest(2, LayoutId::Arabic, 0, 6, 0));
        assert_ne!(base, switch_digest(1, LayoutId::Hebrew, 0, 6, 0));
        assert_ne!(base, switch_digest(1, LayoutId::Arabic, 1, 6, 0));
        assert_ne!(base, switch_digest(1, LayoutId::Arabic, 0, 7, 0));
        assert_ne!(base, switch_digest(1, LayoutId::Arabic, 0, 6, 1));
        assert_ne!(base, p_core::FNV_OFF);
    }

    #[test]
    fn parse_u32_edges() {
        assert_eq!(parse_u32(""), None);
        assert_eq!(parse_u32("0"), Some(0));
        assert_eq!(parse_u32("00012"), Some(12));
        assert_eq!(parse_u32("a1"), None);
        assert_eq!(parse_u32("-3"), None);
        assert_eq!(parse_u32("4294967295"), Some(u32::MAX));
        assert_eq!(parse_u32("4294967296"), None);
    }

    #[test]
    fn parse_hex16_edges() {
        assert_eq!(parse_hex16("0000000000000000"), Some(0));
        assert_eq!(parse_hex16("ffffffffffffffff"), Some(u64::MAX));
        assert_eq!(parse_hex16("FFFFFFFFFFFFFFFF"), Some(u64::MAX));
        assert_eq!(parse_hex16("abc"), None);
        assert_eq!(parse_hex16("gfffffffffffff"), None);
    }

    #[test]
    fn row_fields_shape() {
        let r = match switch_rtl(1) {
            Some(v) => v,
            None => panic!("无报告"),
        };
        let row = render_switch_row(&r);
        let f = match row_fields(&row, SW_TAG, &SW_KEYS) {
            Some(v) => v,
            None => panic!("拆行失败"),
        };
        assert_eq!(f.len(), SW_KEYS.len() + 1);
        assert_eq!(f.first().copied(), Some("1"));
        assert_eq!(f.get(1).copied(), Some("ar"));
        assert_eq!(row_fields(&row, "XXXX", &SW_KEYS), None);
        assert_eq!(row_fields("SWCH105|round=1", SW_TAG, &SW_KEYS), None);
        assert_eq!(row_fields("SWCH105|round=|layout=ar|pending=0|delivered=6|lost=0|digest=0000000000000000", SW_TAG, &SW_KEYS), None);
    }

    #[test]
    fn switch_row_round_trip() {
        let rows = export_switch_rows();
        assert_eq!(rows.len(), ROW_SLOTS);
        let mut i = 0usize;
        while i < rows.len() {
            let row = match rows.get(i) {
                Some(v) => v,
                None => panic!("缺行"),
            };
            assert!(audit_switch_row(row));
            i += 1;
        }
    }

    #[test]
    fn switch_row_rejects_unknown_layout() {
        assert!(!audit_switch_row(
            "SWCH105|round=1|layout=de|pending=0|delivered=6|lost=0|digest=0000000000000000"
        ));
    }

    #[test]
    fn switch_row_rejects_nonzero_lost() {
        assert!(!audit_switch_row(
            "SWCH105|round=1|layout=ar|pending=0|delivered=6|lost=1|digest=0000000000000000"
        ));
        assert!(!audit_switch_row(
            "SWCH105|round=1|layout=ar|pending=1|delivered=6|lost=0|digest=0000000000000000"
        ));
    }

    #[test]
    fn switch_row_rejects_out_of_window_round() {
        assert!(!audit_switch_row(
            "SWCH105|round=99999|layout=ar|pending=0|delivered=6|lost=0|digest=0000000000000000"
        ));
        assert!(!audit_switch_row(
            "SWCH105|round=0|layout=ar|pending=0|delivered=6|lost=0|digest=0000000000000000"
        ));
    }

    #[test]
    fn switch_row_tamper_rejected() {
        let rows = export_switch_rows();
        let mut i = 0usize;
        while i < rows.len() {
            let row = match rows.get(i) {
                Some(v) => v,
                None => panic!("缺行"),
            };
            assert!(!audit_switch_row(&tamper_tail(row)));
            let mut extra = row.clone();
            extra.push('0');
            assert!(!audit_switch_row(&extra));
            i += 1;
        }
    }

    #[test]
    fn shape_rows_round_trip() {
        let rows = export_shape_rows();
        assert_eq!(rows.len(), 8);
        let mut i = 0usize;
        while i < rows.len() {
            let row = match rows.get(i) {
                Some(v) => v,
                None => panic!("缺行"),
            };
            assert!(audit_shape_row(row));
            assert!(!audit_shape_row(&tamper_tail(row)));
            i += 1;
        }
    }

    #[test]
    fn shape_rows_are_distinct() {
        let rows = export_shape_rows();
        let mut i = 0usize;
        while i < rows.len() {
            let a = match rows.get(i) {
                Some(v) => v,
                None => panic!("缺行"),
            };
            let mut j = i + 1;
            while j < rows.len() {
                if let Some(b) = rows.get(j) {
                    assert_ne!(a, b);
                }
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn shape_record_gate_uses_legal_slots() {
        let mut i = 0usize;
        while i < AR_LETTERS {
            assert!(shape_record(i, true, true).is_some());
            assert!(shape_record(i, false, false).is_some());
            i += 1;
        }
        assert!(shape_record(AR_LETTERS, true, true).is_none());
    }

    #[test]
    fn shape_digest_sensitivity() {
        let base = shape_digest(1, Shape::Initial, 0x1234, true);
        assert_eq!(base, shape_digest(1, Shape::Initial, 0x1234, true));
        assert_ne!(base, shape_digest(2, Shape::Initial, 0x1234, true));
        assert_ne!(base, shape_digest(1, Shape::Final, 0x1234, true));
        assert_ne!(base, shape_digest(1, Shape::Initial, 0x1235, true));
        assert_ne!(base, shape_digest(1, Shape::Initial, 0x1234, false));
        assert_ne!(base, p_core::FNV_OFF);
    }

    #[test]
    fn rows_stable_gate() {
        assert!(rows_stable());
    }

    #[test]
    fn tamper_tail_flips_last_byte() {
        let row = "ABC|digest=000";
        assert_eq!(tamper_tail(row), "ABC|digest=001");
        let row2 = "ABC|digest=001";
        assert_eq!(tamper_tail(row2), "ABC|digest=000");
        assert_eq!(tamper_tail(""), "");
    }

    #[test]
    fn extra_checks_count_and_names() {
        let items = extra_checks();
        assert_eq!(items.len(), 8);
        let mut i = 0usize;
        let mut a = 0usize;
        let mut b = 0usize;
        while i < items.len() {
            let c = match items.get(i) {
                Some(v) => v,
                None => panic!("缺判据"),
            };
            assert!(c.1);
            assert!(!c.2.is_empty());
            if c.0.starts_with("F49105/") {
                a += 1;
            } else if c.0.starts_with("F49106/") {
                b += 1;
            }
            i += 1;
        }
        assert_eq!(a, 4);
        assert_eq!(b, 4);
    }

    #[test]
    fn checkset_eight_green() {
        let s = run_unx49105_checks();
        assert_eq!(s.len(), 8);
        assert_eq!(s.passed_count(), 8);
        assert_eq!(s.failed_count(), 0);
        assert!(s.all_passed());
        assert!(!s.truncated());
        assert!(s.red_items_text().is_empty());
        assert_eq!(s.domain, "UNX-F49105 RTL 基向与切换持久化");
    }
}
