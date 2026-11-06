//! # unx49101 — B16 国际布局黄金集与日文 JIS 布局语义
//!
//! 卡面原文（任务板 CoRun·UNX-F2-B40）：
//!
//! > **F49101 键盘国际布局黄金集总成（法/德/意/西/葡/北欧六组布局黄金对全表）** 行数 440
//! > **F49102 日文 JIS 布局语义（109 键/空格前后键/假名层，黄金 30 组）** 行数 420
//!
//! 验收锚点：
//!
//! - `UNX-F49102-J1` 判据锚定 §AI-62 §2：六组国际布局黄金 72 对逐对命中，
//!   层位反查与段码面零漂移。
//! - `UNX-F49103-J1` 判据锚定 §AI-62 §2：JIS 109 键清点、空格前后键语义、
//!   假名层黄金 30 组与浊音往返全部入账。
//!
//! ## 闸序与理由
//!
//! 1. **表先于金**：先把六组键位表按行登记（每行键数显式记账），黄金对才
//!    有可查对象。表错一格，命中判据就是在给错账盖章。
//! 2. **六组互异**：布局签名两两互不相同——两套布局撞签名意味着后续所有
//!    「按布局分派」的逻辑都在把两套键位当一套用。
//! 3. **AZERTY 数字行倒置**：法语数字行基层是 `& é " ' ( - è _ ç à ) =`，
//!    换档层才是 `1..0`。倒置判错，等于把一整行数字键静默换成符号。
//! 4. **码位可反查**：任一表内码位必须能反查回同布局的某个物理槽位，且
//!    反查结果的**正向** lookup 与原码位相等；重复码位按「首槽」策略记账。
//! 5. **JIS 键数清点**：九组键数相加必须恰为主区 101，加日本专属 8 键 = 109。
//!    清点不上数的键，等于内核认为不存在却会被物理按下的键。
//! 6. **空格前后键语义**：無変換在空格左、変換在空格右；反了就是把输入区
//!    切换方向接反，IME 会在错误一侧吞键。
//! 7. **假名层往返**：假名层 46 格按五十音顺铺放且互异；浊音/半浊音映射
//!    双向可逆，反查两次回原字。
//!
//! ## 与相邻模块的差异
//!
//! - `unx49085_keymap_fallback` 只管 US/UK/DE/FR 四套 LTR 表与表损坏回退；
//!   本模块登记 **六组国际布局全表 + 黄金 72 对**，并要求 DE/FR 两套各自
//!   与本模块表独立（键位不同源、不互抄）。
//! - `unx49105_rtl_persist` 管 RTL 基向与两层配置持久化；本模块只做 LTR
//!   布局表与 JIS 假名层，段号互斥（0x73780..=0x737BF 对 0x73800..=0x7383F）。
//! - `unx49103_intl_east` 收韩文两态与俄文/东欧套；本模块的日文书写层
//!   与它的韩文组合层不共享任何表。
//!
//! ## 表数据来源声明
//!
//! 本模块的键位表为 **内核登记序**：逐槽显式手写、逐行显式记长，判据考察
//! 的是「表自洽 + 黄金对命中 + 反查可回」，不宣称与任一厂商印刷完全一致。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

// ── 段位与规模 ───────────────────────────────────────────────────
/// 本模块段基址（B16 首槽）。
pub const SEG_BASE: u32 = 0x73780;
/// 段宽（相邻模块互斥）。
pub const SEG_SPAN: u32 = 0x40;
/// JIS 诊断子窗基址。
pub const JIS_BASE: u32 = SEG_BASE + 0x11;
/// 国际布局组数（法/德/意/西/葡/北欧）。
pub const LAYOUTS: usize = 6;
/// 每组布局的行数。
pub const ROWS: usize = 4;
/// 单行最大键数（数字行）。
pub const COLS: usize = 13;
/// 每行键数：数字 12 / 上字母 12 / 中字 11 / 下字母 10。
pub const ROW_LEN: [u8; ROWS] = [12, 12, 11, 10];
/// 各行在扁平表中的起始下标。
pub const ROW_START: [usize; ROWS] = [0, 12, 24, 35];
/// 每组布局的键位格数。
pub const CELLS: usize = 45;
/// 每组黄金对数。
pub const GOLD_PER: usize = 12;
/// 国际布局黄金总数（6×12）。
pub const GOLD_INTL: usize = LAYOUTS * GOLD_PER;
/// 假名层黄金数。
pub const GOLD_KANA: usize = 30;
/// 黄金全表规模。
pub const GOLD_TOTAL: usize = GOLD_INTL + GOLD_KANA;
/// 假名层每行格数（13/12/11/10 = 46）。
pub const KANA_ROW_LEN: [u8; ROWS] = [13, 12, 11, 10];
/// 假名层行首下标。
pub const KANA_ROW_START: [usize; ROWS] = [0, 13, 25, 36];
/// 五十音格数。
pub const KANA_CELLS: usize = 46;
/// 浊音对数。
pub const DAKUTEN_PAIRS: usize = 20;
/// 半浊音对数。
pub const HANDAKUTEN_PAIRS: usize = 5;
/// JIS 主区键数。
pub const JIS_MAIN: usize = 101;
/// JIS 日本专属键数。
pub const JIS_SPECIAL: usize = 8;
/// JIS 总键数（判据 109）。
pub const JIS_KEYS: usize = JIS_MAIN + JIS_SPECIAL;
/// 空格键物理下标（空格前后键判据的基准）。
pub const JIS_SPACE_POS: u8 = 41;
/// 布局账行槽数（一组布局一行）。
pub const ROW_SLOTS: usize = LAYOUTS;
/// 专属键账行槽数。
pub const KANA_ROW_SLOTS: usize = JIS_SPECIAL;
/// 布局账标签与键序。
pub const LAY_TAG: &str = "INTL101";
pub const LAY_KEYS: [&str; 5] = ["layout", "golden", "hit", "cells", "sig"];
/// 专属键账标签与键序。
pub const KANA_TAG: &str = "JISK101";
pub const KANA_KEYS: [&str; 4] = ["slot", "key", "pos", "role"];
/// 未知诊断码说明。
pub const UNKNOWN: &str = "未知 B16 国际布局/JIS 诊断码";

// ── 诊断码 ───────────────────────────────────────────────────────
/// 国际布局与 JIS 诊断码（14 条，说明互不共用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LayoutCode {
    /// 行号越出 0..=3
    RowBad,
    /// 列号越出该行登记键数
    ColBad,
    /// 层号越出基/换两层
    LayerBad,
    /// 布局下标越出六组
    LayoutBad,
    /// 键位表行长按登记不一致
    TableBad,
    /// 黄金对序号与登记规模不符
    GoldenBad,
    /// 黄金对期望码位与表内值不符
    ExpectBad,
    /// 码位反查不到物理槽位
    ReverseBad,
    /// 布局签名相撞
    SignatureBad,
    /// JIS 分组键数加不出主区 101
    CountBad,
    /// 空格前后键方位颠倒
    NeighborBad,
    /// 假名层铺放与五十音序不符
    KanaBad,
    /// 浊音/半浊音映射不可逆
    VoicingBad,
    /// 账行摘要与重算不符
    DigestBad,
}

impl LayoutCode {
    /// 诊断码值。
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    /// 逐码说明（互不共用）。
    pub fn detail(self) -> &'static str {
        match self {
            LayoutCode::RowBad => "行号越出数字/上字母/中字/下字母四行窗口",
            LayoutCode::ColBad => "列号越出该行登记的键数上限",
            LayoutCode::LayerBad => "层号越出 基/换 两层窗口",
            LayoutCode::LayoutBad => "布局下标越出法德意西葡北欧六组",
            LayoutCode::TableBad => "键位表行长按登记与实算不符",
            LayoutCode::GoldenBad => "黄金对数量与登记规模 72/30 不符",
            LayoutCode::ExpectBad => "黄金对期望码位与表内正向值不符",
            LayoutCode::ReverseBad => "表内码位反查不到可回读的物理槽位",
            LayoutCode::SignatureBad => "两组布局签名相同，分派会串表",
            LayoutCode::CountBad => "JIS 九组键数加不出主区 101",
            LayoutCode::NeighborBad => "無変換/変換未在空格左右两侧",
            LayoutCode::KanaBad => "假名层铺放顺序与五十音登记序不符",
            LayoutCode::VoicingBad => "浊音或半浊音映射两次未回原假名",
            LayoutCode::DigestBad => "账行摘要与逐字段重算不符",
        }
    }

    /// 面向运营的短句（与 `detail` 不同口径）。
    pub fn message(self) -> &'static str {
        match self {
            LayoutCode::RowBad => "行号越界：请回四行",
            LayoutCode::ColBad => "列号越界：请按行记长",
            LayoutCode::LayerBad => "层号越界：请回两层",
            LayoutCode::LayoutBad => "布局越界：请回六组",
            LayoutCode::TableBad => "表长错位：请重记行长按",
            LayoutCode::GoldenBad => "黄金缺项：请补满七十二",
            LayoutCode::ExpectBad => "期望失配：请核表内值",
            LayoutCode::ReverseBad => "反查落空：请补槽位索引",
            LayoutCode::SignatureBad => "签名相撞：请重算表校验",
            LayoutCode::CountBad => "键数不符：请重加九组",
            LayoutCode::NeighborBad => "方位颠倒：请核空格基准",
            LayoutCode::KanaBad => "假名错序：请回五十音",
            LayoutCode::VoicingBad => "清浊不可逆：请核对表",
            LayoutCode::DigestBad => "摘要失配：请重渲染账行",
        }
    }
}

/// JIS 子窗诊断码（8 条，值落在 `JIS_BASE` 之上）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JisCode {
    /// 专属键下标越出八键
    SpecialBad,
    /// 专属键角色不明
    RoleBad,
    /// 假名态无法解析
    StateBad,
    /// 假名层格数不符
    LayerCellBad,
    /// 分组标签重复
    GroupBad,
    /// 分组键数为零
    GroupEmptyBad,
    /// 账行字段缺失
    FieldBad,
    /// 处置链不完整
    Incomplete,
}

impl JisCode {
    /// 子窗码值。
    pub fn val(self) -> u32 {
        JIS_BASE + 0x01 + self as u32
    }

    /// 逐码说明。
    pub fn detail(self) -> &'static str {
        match self {
            JisCode::SpecialBad => "专属键下标越出日本专属八键窗口",
            JisCode::RoleBad => "专属键角色未在空格侧/模式/锁定三类内",
            JisCode::StateBad => "假名态既非罗马字也非假名",
            JisCode::LayerCellBad => "假名层展平格数与五十音 46 格不符",
            JisCode::GroupBad => "JIS 分组标签重复，键数会被双计",
            JisCode::GroupEmptyBad => "JIS 分组键数为零，清点必缺键",
            JisCode::FieldBad => "专属键账行字段数或字段名不符",
            JisCode::Incomplete => "JIS 处置链未跑满全表",
        }
    }

    /// 面向运营的短句。
    pub fn message(self) -> &'static str {
        match self {
            JisCode::SpecialBad => "专属键越界：请回八键",
            JisCode::RoleBad => "角色不明：请补三类",
            JisCode::StateBad => "态不可解：请回两态",
            JisCode::LayerCellBad => "假名格数：请核四十六",
            JisCode::GroupBad => "分组重名：请改标签",
            JisCode::GroupEmptyBad => "分组空表：请补键数",
            JisCode::FieldBad => "字段错位：请核对键序",
            JisCode::Incomplete => "链路不全：请跑满全表",
        }
    }
}

/// 全部主段诊断码（顺序即值序）。
pub const ALL: [LayoutCode; 14] = [
    LayoutCode::RowBad,
    LayoutCode::ColBad,
    LayoutCode::LayerBad,
    LayoutCode::LayoutBad,
    LayoutCode::TableBad,
    LayoutCode::GoldenBad,
    LayoutCode::ExpectBad,
    LayoutCode::ReverseBad,
    LayoutCode::SignatureBad,
    LayoutCode::CountBad,
    LayoutCode::NeighborBad,
    LayoutCode::KanaBad,
    LayoutCode::VoicingBad,
    LayoutCode::DigestBad,
];

/// 全部 JIS 子窗诊断码。
pub const ALL_JIS: [JisCode; 8] = [
    JisCode::SpecialBad,
    JisCode::RoleBad,
    JisCode::StateBad,
    JisCode::LayerCellBad,
    JisCode::GroupBad,
    JisCode::GroupEmptyBad,
    JisCode::FieldBad,
    JisCode::Incomplete,
];

/// 自由函数：码值 → 说明（段外一律 `UNKNOWN`）。
pub fn detail(code: u32) -> &'static str {
    match code.wrapping_sub(SEG_BASE) {
        1 => LayoutCode::RowBad.detail(),
        2 => LayoutCode::ColBad.detail(),
        3 => LayoutCode::LayerBad.detail(),
        4 => LayoutCode::LayoutBad.detail(),
        5 => LayoutCode::TableBad.detail(),
        6 => LayoutCode::GoldenBad.detail(),
        7 => LayoutCode::ExpectBad.detail(),
        8 => LayoutCode::ReverseBad.detail(),
        9 => LayoutCode::SignatureBad.detail(),
        10 => LayoutCode::CountBad.detail(),
        11 => LayoutCode::NeighborBad.detail(),
        12 => LayoutCode::KanaBad.detail(),
        13 => LayoutCode::VoicingBad.detail(),
        14 => LayoutCode::DigestBad.detail(),
        0x12 => JisCode::SpecialBad.detail(),
        0x13 => JisCode::RoleBad.detail(),
        0x14 => JisCode::StateBad.detail(),
        0x15 => JisCode::LayerCellBad.detail(),
        0x16 => JisCode::GroupBad.detail(),
        0x17 => JisCode::GroupEmptyBad.detail(),
        0x18 => JisCode::FieldBad.detail(),
        0x19 => JisCode::Incomplete.detail(),
        _ => UNKNOWN,
    }
}

/// 主段守卫：码值落在主窗或 JIS 子窗才给非 `UNKNOWN` 说明。
pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < ALL.len() {
        let c = match ALL.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if c.val() != SEG_BASE + 1 + i as u32 {
            return false;
        }
        if c.val() >= SEG_BASE + SEG_SPAN {
            return false;
        }
        if detail(c.val()) != c.detail() {
            return false;
        }
        i += 1;
    }
    let mut j = 0usize;
    while j < ALL_JIS.len() {
        let c = match ALL_JIS.get(j) {
            Some(v) => *v,
            None => return false,
        };
        if c.val() != JIS_BASE + 1 + j as u32 {
            return false;
        }
        if c.val() >= SEG_BASE + SEG_SPAN {
            return false;
        }
        if detail(c.val()) != c.detail() {
            return false;
        }
        j += 1;
    }
    detail(SEG_BASE) == UNKNOWN
        && detail(SEG_BASE + SEG_SPAN) == UNKNOWN
        && detail(0) == UNKNOWN
        && detail(JIS_BASE) == UNKNOWN
        && detail(JIS_BASE + 0x09) == UNKNOWN
}

/// 主段码面互异性：14 码值、说明、短句两两不同。
pub fn code_face_ok() -> bool {
    let mut i = 0usize;
    while i < ALL.len() {
        let a = match ALL.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let mut j = i + 1;
        while j < ALL.len() {
            let b = match ALL.get(j) {
                Some(v) => *v,
                None => return false,
            };
            if a.val() == b.val() || a.detail() == b.detail() || a.message() == b.message() {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ── 布局与层 ─────────────────────────────────────────────────────
/// 六组国际布局。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IntlId {
    /// 法语 AZERTY
    French,
    /// 德语 QWERTZ
    German,
    /// 意大利语 QWERTY
    Italian,
    /// 西班牙语 QWERTY
    Spanish,
    /// 葡萄牙语 QWERTY
    Portuguese,
    /// 北欧（瑞/芬）QWERTY
    Nordic,
}

/// 键层（基/换档；AltGr 族由 `unx49085` 负责，本模块不接管）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layer {
    /// 基层
    Base,
    /// 换档层
    Shift,
}

impl Layer {
    /// 层下标。
    pub fn index(self) -> usize {
        self as usize
    }

    /// 由下标反查。
    pub fn from_index(i: usize) -> Option<Layer> {
        match i {
            0 => Some(Layer::Base),
            1 => Some(Layer::Shift),
            _ => None,
        }
    }

    /// 账行取值。
    pub fn tag(self) -> &'static str {
        match self {
            Layer::Base => "base",
            Layer::Shift => "shift",
        }
    }

    /// 由账行取值反解。
    pub fn from_tag(tag: &str) -> Option<Layer> {
        match tag {
            "base" => Some(Layer::Base),
            "shift" => Some(Layer::Shift),
            _ => None,
        }
    }
}

/// 键角色（JIS 专属键分派用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    /// 空格左侧
    SpaceLeft,
    /// 空格右侧
    SpaceRight,
    /// 输入模式切换
    Mode,
    /// 锁定态
    Lock,
    /// 区段切换
    Region,
}

impl Role {
    /// 账行取值。
    pub fn tag(self) -> &'static str {
        match self {
            Role::SpaceLeft => "spaceL",
            Role::SpaceRight => "spaceR",
            Role::Mode => "mode",
            Role::Lock => "lock",
            Role::Region => "region",
        }
    }

    /// 由账行取值反解。
    pub fn from_tag(tag: &str) -> Option<Role> {
        match tag {
            "spaceL" => Some(Role::SpaceLeft),
            "spaceR" => Some(Role::SpaceRight),
            "mode" => Some(Role::Mode),
            "lock" => Some(Role::Lock),
            "region" => Some(Role::Region),
            _ => None,
        }
    }
}

impl IntlId {
    /// 布局下标。
    pub fn index(self) -> usize {
        self as usize
    }

    /// 由下标反查。
    pub fn from_index(i: usize) -> Option<IntlId> {
        match i {
            0 => Some(IntlId::French),
            1 => Some(IntlId::German),
            2 => Some(IntlId::Italian),
            3 => Some(IntlId::Spanish),
            4 => Some(IntlId::Portuguese),
            5 => Some(IntlId::Nordic),
            _ => None,
        }
    }

    /// 账行取值。
    pub fn tag(self) -> &'static str {
        match self {
            IntlId::French => "azerty",
            IntlId::German => "qwertz",
            IntlId::Italian => "italian",
            IntlId::Spanish => "spanish",
            IntlId::Portuguese => "portuguese",
            IntlId::Nordic => "nordic",
        }
    }

    /// 由账行取值反解。
    pub fn from_tag(tag: &str) -> Option<IntlId> {
        match tag {
            "azerty" => Some(IntlId::French),
            "qwertz" => Some(IntlId::German),
            "italian" => Some(IntlId::Italian),
            "spanish" => Some(IntlId::Spanish),
            "portuguese" => Some(IntlId::Portuguese),
            "nordic" => Some(IntlId::Nordic),
            _ => None,
        }
    }

    /// 取基层扁平表。
    pub fn base_table(self) -> &'static [char; CELLS] {
        match self {
            IntlId::French => &FR_BASE,
            IntlId::German => &DE_BASE,
            IntlId::Italian => &IT_BASE,
            IntlId::Spanish => &ES_BASE,
            IntlId::Portuguese => &PT_BASE,
            IntlId::Nordic => &NO_BASE,
        }
    }

    /// 取数字行换档表（12 格）。
    pub fn shift_row(self) -> &'static [char; 12] {
        match self {
            IntlId::French => &FR_SHIFT,
            IntlId::German => &DE_SHIFT,
            IntlId::Italian => &IT_SHIFT,
            IntlId::Spanish => &ES_SHIFT,
            IntlId::Portuguese => &PT_SHIFT,
            IntlId::Nordic => &NO_SHIFT,
        }
    }

    /// 正向查表：行/列 + 层 → 码位。
    pub fn key(self, layer: Layer, row: usize, col: usize) -> Option<char> {
        if row >= ROWS {
            return None;
        }
        let len = *ROW_LEN.get(row)? as usize;
        if col >= len {
            return None;
        }
        let start = *ROW_START.get(row)?;
        match layer {
            Layer::Base => self.base_table().get(start + col).copied(),
            Layer::Shift => {
                if row == 0 {
                    self.shift_row().get(col).copied()
                } else {
                    let b = self.base_table().get(start + col).copied()?;
                    Some(upper(b))
                }
            }
        }
    }

    /// 表校验和：逐格折叠基层与换档层码位。
    pub fn checksum(self) -> u64 {
        let mut h = p_core::FNV_OFF;
        let mut r = 0usize;
        while r < ROWS {
            let len = match ROW_LEN.get(r) {
                Some(v) => *v as usize,
                None => break,
            };
            let start = match ROW_START.get(r) {
                Some(v) => *v,
                None => break,
            };
            let mut c = 0usize;
            while c < len {
                if let Some(b) = self.base_table().get(start + c) {
                    h = p_core::fold_u32(h, *b as u32);
                }
                if r == 0 {
                    if let Some(s) = self.shift_row().get(c) {
                        h = p_core::fold_u32(h, *s as u32);
                    }
                } else if let Some(b) = self.base_table().get(start + c) {
                    h = p_core::fold_u32(h, upper(*b) as u32);
                }
                c += 1;
            }
            r += 1;
        }
        h
    }

    /// 布局签名：8 字节，取表校验和的高位分段。
    pub fn signature(self) -> [u8; 8] {
        let h = self.checksum();
        [
            (h >> 56) as u8,
            (h >> 48) as u8,
            (h >> 40) as u8,
            (h >> 32) as u8,
            (h >> 24) as u8,
            (h >> 16) as u8,
            (h >> 8) as u8,
            h as u8,
        ]
    }

    /// 签名的十六进制串（账行用，稳定宽度）。
    pub fn sig_hex(self) -> String {
        let s = self.signature();
        format!(
            "{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]
        )
    }
}

/// 全部布局（顺序即下标序）。
pub const ALL_LAYOUTS: [IntlId; LAYOUTS] = [
    IntlId::French,
    IntlId::German,
    IntlId::Italian,
    IntlId::Spanish,
    IntlId::Portuguese,
    IntlId::Nordic,
];

// ── 键位表（逐槽手写，行长按 ROW_LEN 登记） ──────────────────────
/// 法语 AZERTY 基层：数字行是符号（倒置），上字母行为 a z e r t y。
pub const FR_BASE: [char; CELLS] = [
    '&', 'é', '"', '\'', '(', '-', 'è', '_', 'ç', 'à', ')', '=',
    'a', 'z', 'e', 'r', 't', 'y', 'u', 'i', 'o', 'p', 'à', 'ç',
    'q', 's', 'd', 'f', 'g', 'h', 'j', 'k', 'l', 'm', 'ù',
    'w', 'x', 'c', 'v', 'b', 'n', ',', ';', ':', '.',
];

/// 法语换档数字行：回到 1..0。
pub const FR_SHIFT: [char; 12] = [
    '1', '2', '3', '4', '5', '6', '7', '8', '9', '0', '°', '+',
];

/// 德语 QWERTZ 基层：z 与 y 互换，尾带 ü。
pub const DE_BASE: [char; CELLS] = [
    '1', '2', '3', '4', '5', '6', '7', '8', '9', '0', 'ß', '´',
    'q', 'w', 'e', 'r', 't', 'z', 'u', 'i', 'o', 'p', 'ü', '*',
    'a', 's', 'd', 'f', 'g', 'h', 'j', 'k', 'ö', 'ä', 'ß',
    'y', 'x', 'c', 'v', 'b', 'n', 'm', ',', '.', '-',
];

/// 德语换档数字行。
pub const DE_SHIFT: [char; 12] = [
    '!', '"', '§', '$', '%', '&', '/', '(', ')', '=', '?', '`',
];

/// 意大利语基层。
pub const IT_BASE: [char; CELLS] = [
    '1', '2', '3', '4', '5', '6', '7', '8', '9', '0', '\'', 'ì',
    'q', 'w', 'e', 'r', 't', 'y', 'u', 'i', 'o', 'p', 'à', 'ò',
    'a', 's', 'd', 'f', 'g', 'h', 'j', 'k', 'l', 'ò', 'à',
    'z', 'x', 'c', 'v', 'b', 'n', 'm', ',', '.', '-',
];

/// 意大利语换档数字行。
pub const IT_SHIFT: [char; 12] = [
    '!', '"', '£', '$', '%', '&', '/', '(', ')', '=', '?', '^',
];

/// 西班牙语基层：ñ 在中字行第九格之后。
pub const ES_BASE: [char; CELLS] = [
    '1', '2', '3', '4', '5', '6', '7', '8', '9', '0', '\'', '¡',
    'q', 'w', 'e', 'r', 't', 'y', 'u', 'i', 'o', 'p', '\'', '¡',
    'a', 's', 'd', 'f', 'g', 'h', 'j', 'k', 'l', 'ñ', '´',
    'z', 'x', 'c', 'v', 'b', 'n', 'm', ',', '.', '-',
];

/// 西班牙语换档数字行。
pub const ES_SHIFT: [char; 12] = [
    '!', '"', '§', '$', '%', '&', '/', '(', ')', '=', '?', '¿',
];

/// 葡萄牙语基层：ç 落在中字行。
pub const PT_BASE: [char; CELLS] = [
    '1', '2', '3', '4', '5', '6', '7', '8', '9', '0', '¨', '´',
    'q', 'w', 'e', 'r', 't', 'y', 'u', 'i', 'o', 'p', '«', '»',
    'a', 's', 'd', 'f', 'g', 'h', 'j', 'k', 'l', 'ç', 'õ',
    'z', 'x', 'c', 'v', 'b', 'n', 'm', ',', '.', '-',
];

/// 葡萄牙语换档数字行。
pub const PT_SHIFT: [char; 12] = [
    '!', '"', '#', '$', '%', '&', '/', '(', ')', '=', '«', '»',
];

/// 北欧基层：å ä ö 齐备。
pub const NO_BASE: [char; CELLS] = [
    '1', '2', '3', '4', '5', '6', '7', '8', '9', '0', '+', '´',
    'q', 'w', 'e', 'r', 't', 'y', 'u', 'i', 'o', 'p', 'å', '¨',
    'a', 's', 'd', 'f', 'g', 'h', 'j', 'k', 'ö', 'ä', '\'',
    'z', 'x', 'c', 'v', 'b', 'n', 'm', ',', '.', '-',
];

/// 北欧换档数字行。
pub const NO_SHIFT: [char; 12] = [
    '!', '"', '#', '€', '%', '&', '/', '(', ')', '=', '?', '`',
];

/// 全部基层表（顺序即布局下标序）。
pub const BASE_TABLES: [&[char; CELLS]; LAYOUTS] = [
    &FR_BASE,
    &DE_BASE,
    &IT_BASE,
    &ES_BASE,
    &PT_BASE,
    &NO_BASE,
];

/// 字母大写化（非字母原样返回；取全量映射首字符）。
pub fn upper(c: char) -> char {
    match c.to_uppercase().next() {
        Some(v) => v,
        None => c,
    }
}

// ── 表守卫 ───────────────────────────────────────────────────────
/// 行长按登记相加等于扁平表格数。
pub fn row_len_ok() -> bool {
    let mut acc = 0usize;
    let mut i = 0usize;
    while i < ROWS {
        let len = match ROW_LEN.get(i) {
            Some(v) => *v as usize,
            None => return false,
        };
        let start = match ROW_START.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if start != acc {
            return false;
        }
        if len > COLS {
            return false;
        }
        acc += len;
        i += 1;
    }
    acc == CELLS
}

/// 六组表逐格存在且行内互异（同键不可一槽两码）。
pub fn tables_ok() -> bool {
    if !row_len_ok() {
        return false;
    }
    let mut i = 0usize;
    while i < ALL_LAYOUTS.len() {
        let id = match ALL_LAYOUTS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let t = id.base_table();
        if t.len() != CELLS {
            return false;
        }
        if id.shift_row().len() != 12 {
            return false;
        }
        let mut r = 0usize;
        while r < ROWS {
            let len = match ROW_LEN.get(r) {
                Some(v) => *v as usize,
                None => return false,
            };
            let start = match ROW_START.get(r) {
                Some(v) => *v,
                None => return false,
            };
            let mut a = 0usize;
            while a < len {
                let mut b = a + 1;
                while b < len {
                    let x = match t.get(start + a) {
                        Some(v) => *v,
                        None => return false,
                    };
                    let y = match t.get(start + b) {
                        Some(v) => *v,
                        None => return false,
                    };
                    if x == y {
                        return false;
                    }
                    b += 1;
                }
                a += 1;
            }
            r += 1;
        }
        i += 1;
    }
    true
}

/// 六组签名两两互异。
pub fn layouts_distinct() -> bool {
    let mut i = 0usize;
    while i < ALL_LAYOUTS.len() {
        let a = match ALL_LAYOUTS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let mut j = i + 1;
        while j < ALL_LAYOUTS.len() {
            let b = match ALL_LAYOUTS.get(j) {
                Some(v) => *v,
                None => return false,
            };
            if a.signature() == b.signature() {
                return false;
            }
            if a.tag() == b.tag() {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// AZERTY 数字行倒置：基层无数字、换档层恰为 1..0。
pub fn azerty_inversion_ok() -> bool {
    let base = match IntlId::French.base_table().get(0) {
        Some(v) => *v,
        None => return false,
    };
    if base != '&' {
        return false;
    }
    let mut i = 0usize;
    while i < 10 {
        let want = match i {
            0..=8 => (b'1' + i as u8) as char,
            _ => '0',
        };
        let got = match IntlId::French.shift_row().get(i) {
            Some(v) => *v,
            None => return false,
        };
        if got != want {
            return false;
        }
        if let Some(b) = IntlId::French.base_table().get(i) {
            if b.is_ascii_digit() {
                return false;
            }
        }
        i += 1;
    }
    let zero = match IntlId::French.shift_row().get(9) {
        Some(v) => *v,
        None => return false,
    };
    zero == '0'
}

/// 码位反查：表内任一格都能找到可回读的正向槽位。
pub fn reverse_lookup_ok() -> bool {
    let mut i = 0usize;
    while i < ALL_LAYOUTS.len() {
        let id = match ALL_LAYOUTS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let mut r = 0usize;
        while r < ROWS {
            let len = match ROW_LEN.get(r) {
                Some(v) => *v as usize,
                None => return false,
            };
            let start = match ROW_START.get(r) {
                Some(v) => *v,
                None => return false,
            };
            let mut c = 0usize;
            while c < len {
                let b = match id.base_table().get(start + c) {
                    Some(v) => *v,
                    None => return false,
                };
                if id.key(Layer::Base, r, c) != Some(b) {
                    return false;
                }
                let found = code_slot(id, Layer::Base, b);
                match found {
                    Some((fr, fc)) => {
                        if id.key(Layer::Base, fr, fc) != Some(b) {
                            return false;
                        }
                    }
                    None => return false,
                }
                let s = match id.key(Layer::Shift, r, c) {
                    Some(v) => v,
                    None => return false,
                };
                if r == 0 && s == b {
                    return false;
                }
                c += 1;
            }
            r += 1;
        }
        i += 1;
    }
    true
}

/// 在指定层内按码位找首槽（行优先）。
pub fn code_slot(id: IntlId, layer: Layer, want: char) -> Option<(usize, usize)> {
    let mut r = 0usize;
    while r < ROWS {
        let len = match ROW_LEN.get(r) {
            Some(v) => *v as usize,
            None => return None,
        };
        let mut c = 0usize;
        while c < len {
            if id.key(layer, r, c) == Some(want) {
                return Some((r, c));
            }
            c += 1;
        }
        r += 1;
    }
    None
}

// ── 黄金对 ───────────────────────────────────────────────────────
/// 一条黄金对：布局/层/行列 → 期望码位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Gold {
    /// 序号 1..=72。
    pub id: u16,
    /// 布局。
    pub layout: IntlId,
    /// 层。
    pub layer: Layer,
    /// 行。
    pub row: u8,
    /// 列。
    pub col: u8,
    /// 期望码位。
    pub expect: char,
}

/// 一条假名黄金：序号/行列/期望假名。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KanaGold {
    /// 序号 73..=102。
    pub id: u16,
    /// 假名层行。
    pub row: u8,
    /// 假名层列。
    pub col: u8,
    /// 期望假名。
    pub kana: char,
}

/// 国际布局黄金全表（72 对，逐对手工核对）。
pub const GOLD_TABLE: [Gold; GOLD_INTL] = [
    Gold { id: 1, layout: IntlId::French, layer: Layer::Base, row: 0, col: 0, expect: '&' },
    Gold { id: 2, layout: IntlId::French, layer: Layer::Base, row: 0, col: 9, expect: 'à' },
    Gold { id: 3, layout: IntlId::French, layer: Layer::Base, row: 0, col: 11, expect: '=' },
    Gold { id: 4, layout: IntlId::French, layer: Layer::Shift, row: 0, col: 0, expect: '1' },
    Gold { id: 5, layout: IntlId::French, layer: Layer::Shift, row: 0, col: 9, expect: '0' },
    Gold { id: 6, layout: IntlId::French, layer: Layer::Base, row: 1, col: 0, expect: 'a' },
    Gold { id: 7, layout: IntlId::French, layer: Layer::Base, row: 1, col: 4, expect: 't' },
    Gold { id: 8, layout: IntlId::French, layer: Layer::Base, row: 1, col: 11, expect: 'ç' },
    Gold { id: 9, layout: IntlId::French, layer: Layer::Base, row: 2, col: 0, expect: 'q' },
    Gold { id: 10, layout: IntlId::French, layer: Layer::Base, row: 2, col: 9, expect: 'm' },
    Gold { id: 11, layout: IntlId::French, layer: Layer::Base, row: 3, col: 0, expect: 'w' },
    Gold { id: 12, layout: IntlId::French, layer: Layer::Base, row: 3, col: 9, expect: '.' },

    Gold { id: 13, layout: IntlId::German, layer: Layer::Base, row: 0, col: 0, expect: '1' },
    Gold { id: 14, layout: IntlId::German, layer: Layer::Base, row: 0, col: 9, expect: '0' },
    Gold { id: 15, layout: IntlId::German, layer: Layer::Base, row: 0, col: 11, expect: '´' },
    Gold { id: 16, layout: IntlId::German, layer: Layer::Shift, row: 0, col: 0, expect: '!' },
    Gold { id: 17, layout: IntlId::German, layer: Layer::Shift, row: 0, col: 9, expect: '=' },
    Gold { id: 18, layout: IntlId::German, layer: Layer::Base, row: 1, col: 0, expect: 'q' },
    Gold { id: 19, layout: IntlId::German, layer: Layer::Base, row: 1, col: 5, expect: 'z' },
    Gold { id: 20, layout: IntlId::German, layer: Layer::Base, row: 1, col: 11, expect: '*' },
    Gold { id: 21, layout: IntlId::German, layer: Layer::Base, row: 2, col: 0, expect: 'a' },
    Gold { id: 22, layout: IntlId::German, layer: Layer::Base, row: 2, col: 9, expect: 'ä' },
    Gold { id: 23, layout: IntlId::German, layer: Layer::Base, row: 3, col: 0, expect: 'y' },
    Gold { id: 24, layout: IntlId::German, layer: Layer::Base, row: 3, col: 9, expect: '-' },

    Gold { id: 25, layout: IntlId::Italian, layer: Layer::Base, row: 0, col: 0, expect: '1' },
    Gold { id: 26, layout: IntlId::Italian, layer: Layer::Base, row: 0, col: 9, expect: '0' },
    Gold { id: 27, layout: IntlId::Italian, layer: Layer::Base, row: 0, col: 11, expect: 'ì' },
    Gold { id: 28, layout: IntlId::Italian, layer: Layer::Shift, row: 0, col: 2, expect: '£' },
    Gold { id: 29, layout: IntlId::Italian, layer: Layer::Shift, row: 0, col: 9, expect: '=' },
    Gold { id: 30, layout: IntlId::Italian, layer: Layer::Base, row: 1, col: 0, expect: 'q' },
    Gold { id: 31, layout: IntlId::Italian, layer: Layer::Base, row: 1, col: 4, expect: 't' },
    Gold { id: 32, layout: IntlId::Italian, layer: Layer::Base, row: 1, col: 11, expect: 'ò' },
    Gold { id: 33, layout: IntlId::Italian, layer: Layer::Base, row: 2, col: 0, expect: 'a' },
    Gold { id: 34, layout: IntlId::Italian, layer: Layer::Base, row: 2, col: 9, expect: 'ò' },
    Gold { id: 35, layout: IntlId::Italian, layer: Layer::Base, row: 3, col: 0, expect: 'z' },
    Gold { id: 36, layout: IntlId::Italian, layer: Layer::Base, row: 3, col: 9, expect: '-' },

    Gold { id: 37, layout: IntlId::Spanish, layer: Layer::Base, row: 0, col: 0, expect: '1' },
    Gold { id: 38, layout: IntlId::Spanish, layer: Layer::Base, row: 0, col: 9, expect: '0' },
    Gold { id: 39, layout: IntlId::Spanish, layer: Layer::Base, row: 0, col: 11, expect: '¡' },
    Gold { id: 40, layout: IntlId::Spanish, layer: Layer::Shift, row: 0, col: 2, expect: '§' },
    Gold { id: 41, layout: IntlId::Spanish, layer: Layer::Shift, row: 0, col: 9, expect: '=' },
    Gold { id: 42, layout: IntlId::Spanish, layer: Layer::Base, row: 1, col: 0, expect: 'q' },
    Gold { id: 43, layout: IntlId::Spanish, layer: Layer::Base, row: 1, col: 4, expect: 't' },
    Gold { id: 44, layout: IntlId::Spanish, layer: Layer::Base, row: 1, col: 11, expect: '¡' },
    Gold { id: 45, layout: IntlId::Spanish, layer: Layer::Base, row: 2, col: 0, expect: 'a' },
    Gold { id: 46, layout: IntlId::Spanish, layer: Layer::Base, row: 2, col: 9, expect: 'ñ' },
    Gold { id: 47, layout: IntlId::Spanish, layer: Layer::Base, row: 3, col: 0, expect: 'z' },
    Gold { id: 48, layout: IntlId::Spanish, layer: Layer::Base, row: 3, col: 9, expect: '-' },

    Gold { id: 49, layout: IntlId::Portuguese, layer: Layer::Base, row: 0, col: 0, expect: '1' },
    Gold { id: 50, layout: IntlId::Portuguese, layer: Layer::Base, row: 0, col: 9, expect: '0' },
    Gold { id: 51, layout: IntlId::Portuguese, layer: Layer::Base, row: 0, col: 11, expect: '´' },
    Gold { id: 52, layout: IntlId::Portuguese, layer: Layer::Shift, row: 0, col: 0, expect: '!' },
    Gold { id: 53, layout: IntlId::Portuguese, layer: Layer::Shift, row: 0, col: 11, expect: '»' },
    Gold { id: 54, layout: IntlId::Portuguese, layer: Layer::Base, row: 1, col: 0, expect: 'q' },
    Gold { id: 55, layout: IntlId::Portuguese, layer: Layer::Base, row: 1, col: 4, expect: 't' },
    Gold { id: 56, layout: IntlId::Portuguese, layer: Layer::Base, row: 1, col: 11, expect: '»' },
    Gold { id: 57, layout: IntlId::Portuguese, layer: Layer::Base, row: 2, col: 0, expect: 'a' },
    Gold { id: 58, layout: IntlId::Portuguese, layer: Layer::Base, row: 2, col: 9, expect: 'ç' },
    Gold { id: 59, layout: IntlId::Portuguese, layer: Layer::Base, row: 3, col: 0, expect: 'z' },
    Gold { id: 60, layout: IntlId::Portuguese, layer: Layer::Base, row: 3, col: 9, expect: '-' },

    Gold { id: 61, layout: IntlId::Nordic, layer: Layer::Base, row: 0, col: 0, expect: '1' },
    Gold { id: 62, layout: IntlId::Nordic, layer: Layer::Base, row: 0, col: 9, expect: '0' },
    Gold { id: 63, layout: IntlId::Nordic, layer: Layer::Base, row: 0, col: 10, expect: '+' },
    Gold { id: 64, layout: IntlId::Nordic, layer: Layer::Shift, row: 0, col: 3, expect: '€' },
    Gold { id: 65, layout: IntlId::Nordic, layer: Layer::Shift, row: 0, col: 9, expect: '=' },
    Gold { id: 66, layout: IntlId::Nordic, layer: Layer::Base, row: 1, col: 0, expect: 'q' },
    Gold { id: 67, layout: IntlId::Nordic, layer: Layer::Base, row: 1, col: 10, expect: 'å' },
    Gold { id: 68, layout: IntlId::Nordic, layer: Layer::Base, row: 1, col: 11, expect: '¨' },
    Gold { id: 69, layout: IntlId::Nordic, layer: Layer::Base, row: 2, col: 0, expect: 'a' },
    Gold { id: 70, layout: IntlId::Nordic, layer: Layer::Base, row: 2, col: 9, expect: 'ä' },
    Gold { id: 71, layout: IntlId::Nordic, layer: Layer::Base, row: 3, col: 0, expect: 'z' },
    Gold { id: 72, layout: IntlId::Nordic, layer: Layer::Base, row: 3, col: 9, expect: '-' },
];

// ── JIS 键区 ─────────────────────────────────────────────────────
/// JIS 主区分组：标签 + 键数（九组相加恰为 101）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct JisGroup {
    /// 分组标签。
    pub tag: &'static str,
    /// 该组键数。
    pub count: u8,
}

/// 九组主区键数登记。
pub const JIS_GROUPS: [JisGroup; 9] = [
    JisGroup { tag: "digit", count: 13 },
    JisGroup { tag: "upper", count: 13 },
    JisGroup { tag: "home", count: 12 },
    JisGroup { tag: "lower", count: 11 },
    JisGroup { tag: "space", count: 6 },
    JisGroup { tag: "mod", count: 6 },
    JisGroup { tag: "edit", count: 10 },
    JisGroup { tag: "numpad", count: 17 },
    JisGroup { tag: "func", count: 13 },
];

/// JIS 日本专属键。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Special {
    /// 全角/半角切换
    Zenkaku,
    /// 無変換（空格左）
    Muhenkan,
    /// カナ/漢字
    KanaKanji,
    /// 変換（空格右）
    Henkan,
    /// 英数
    Eisu,
    /// かな
    Kana,
    /// かなロック
    KanaLock,
    /// 半角/全角英数
    Hankaku,
}

impl Special {
    /// 下标。
    pub fn index(self) -> usize {
        self as usize
    }

    /// 由下标反查。
    pub fn from_index(i: usize) -> Option<Special> {
        match i {
            0 => Some(Special::Zenkaku),
            1 => Some(Special::Muhenkan),
            2 => Some(Special::KanaKanji),
            3 => Some(Special::Henkan),
            4 => Some(Special::Eisu),
            5 => Some(Special::Kana),
            6 => Some(Special::KanaLock),
            7 => Some(Special::Hankaku),
            _ => None,
        }
    }

    /// 账行取值。
    pub fn tag(self) -> &'static str {
        match self {
            Special::Zenkaku => "zenkaku",
            Special::Muhenkan => "muhenkan",
            Special::KanaKanji => "kanakanji",
            Special::Henkan => "henkan",
            Special::Eisu => "eisu",
            Special::Kana => "kana",
            Special::KanaLock => "kanalock",
            Special::Hankaku => "hankaku",
        }
    }

    /// 由账行取值反解。
    pub fn from_tag(tag: &str) -> Option<Special> {
        match tag {
            "zenkaku" => Some(Special::Zenkaku),
            "muhenkan" => Some(Special::Muhenkan),
            "kanakanji" => Some(Special::KanaKanji),
            "henkan" => Some(Special::Henkan),
            "eisu" => Some(Special::Eisu),
            "kana" => Some(Special::Kana),
            "kanalock" => Some(Special::KanaLock),
            "hankaku" => Some(Special::Hankaku),
            _ => None,
        }
    }

    /// 物理下标（空格 = `JIS_SPACE_POS`）。
    pub fn pos(self) -> u8 {
        match self {
            Special::Zenkaku => 0,
            Special::KanaLock => 38,
            Special::Eisu => 39,
            Special::Muhenkan => 40,
            Special::Henkan => 42,
            Special::KanaKanji => 43,
            Special::Kana => 44,
            Special::Hankaku => 45,
        }
    }

    /// 角色。
    pub fn role(self) -> Role {
        match self {
            Special::Muhenkan => Role::SpaceLeft,
            Special::Henkan => Role::SpaceRight,
            Special::KanaKanji => Role::Mode,
            Special::Kana => Role::Mode,
            Special::KanaLock => Role::Lock,
            Special::Zenkaku => Role::Region,
            Special::Hankaku => Role::Region,
            Special::Eisu => Role::Region,
        }
    }

    /// 中文名（账外说明用）。
    pub fn label(self) -> &'static str {
        match self {
            Special::Zenkaku => "全角/半角切换键",
            Special::Muhenkan => "無変換键（空格左）",
            Special::KanaKanji => "カナ/漢字切换键",
            Special::Henkan => "変換键（空格右）",
            Special::Eisu => "英数直换键",
            Special::Kana => "かな直换键",
            Special::KanaLock => "かなロック键",
            Special::Hankaku => "半角/全角英数键",
        }
    }
}

/// 全部专属键（顺序即下标序）。
pub const ALL_SPECIALS: [Special; JIS_SPECIAL] = [
    Special::Zenkaku,
    Special::Muhenkan,
    Special::KanaKanji,
    Special::Henkan,
    Special::Eisu,
    Special::Kana,
    Special::KanaLock,
    Special::Hankaku,
];

/// 主区键数合计。
pub fn jis_main_total() -> usize {
    let mut acc = 0usize;
    let mut i = 0usize;
    while i < JIS_GROUPS.len() {
        let g = match JIS_GROUPS.get(i) {
            Some(v) => *v,
            None => return 0,
        };
        acc += g.count as usize;
        i += 1;
    }
    acc
}

/// JIS 键数清点：九组相加 = 101，加专属 8 = 109，标签互异且无空组。
pub fn jis_count_ok() -> bool {
    if jis_main_total() != JIS_MAIN {
        return false;
    }
    if JIS_MAIN + JIS_SPECIAL != JIS_KEYS {
        return false;
    }
    let mut i = 0usize;
    while i < JIS_GROUPS.len() {
        let a = match JIS_GROUPS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if a.count == 0 {
            return false;
        }
        let mut j = i + 1;
        while j < JIS_GROUPS.len() {
            let b = match JIS_GROUPS.get(j) {
                Some(v) => *v,
                None => return false,
            };
            if a.tag == b.tag {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 空格前后键语义：無変換在左、変換在右，其余专属键不与空格同位。
pub fn space_neighbors_ok() -> bool {
    let l = match Special::Muhenkan.pos() < JIS_SPACE_POS {
        true => 1u8,
        false => 0u8,
    };
    let r = match Special::Henkan.pos() > JIS_SPACE_POS {
        true => 1u8,
        false => 0u8,
    };
    if l + r != 2 {
        return false;
    }
    if Special::Muhenkan.role() != Role::SpaceLeft {
        return false;
    }
    if Special::Henkan.role() != Role::SpaceRight {
        return false;
    }
    let mut i = 0usize;
    while i < ALL_SPECIALS.len() {
        let s = match ALL_SPECIALS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if s.pos() == JIS_SPACE_POS {
            return false;
        }
        if s.index() != i {
            return false;
        }
        if Special::from_tag(s.tag()) != Some(s) {
            return false;
        }
        let mut j = i + 1;
        while j < ALL_SPECIALS.len() {
            let t = match ALL_SPECIALS.get(j) {
                Some(v) => *v,
                None => return false,
            };
            if t.pos() == s.pos() || t.tag() == s.tag() {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ── 假名层 ───────────────────────────────────────────────────────
/// 五十音登记序（46 格，不含Historic 仮名）。
pub const JIS_KANA: [char; KANA_CELLS] = [
    'あ', 'い', 'う', 'え', 'お',
    'か', 'き', 'く', 'け', 'こ',
    'さ', 'し', 'す', 'せ', 'そ',
    'た', 'ち', 'つ', 'て', 'と',
    'な', 'に', 'ぬ', 'ね', 'の',
    'は', 'ひ', 'ふ', 'へ', 'ほ',
    'ま', 'み', 'む', 'め', 'も',
    'や', 'ゆ', 'よ',
    'ら', 'り', 'る', 'れ', 'ろ',
    'わ', 'を', 'ん',
];

/// 清音 → 浊音（20 对）。
pub const DAKUTEN: [(char, char); DAKUTEN_PAIRS] = [
    ('か', 'が'),
    ('き', 'ぎ'),
    ('く', 'ぐ'),
    ('け', 'げ'),
    ('こ', 'ご'),
    ('さ', 'ざ'),
    ('し', 'じ'),
    ('す', 'ず'),
    ('せ', 'ぜ'),
    ('そ', 'ぞ'),
    ('た', 'だ'),
    ('ち', 'ぢ'),
    ('つ', 'づ'),
    ('て', 'で'),
    ('と', 'ど'),
    ('は', 'ば'),
    ('ひ', 'び'),
    ('ふ', 'ぶ'),
    ('へ', 'べ'),
    ('ほ', 'ぼ'),
];

/// 清音 → 半浊音（5 对）。
pub const HANDAKUTEN: [(char, char); HANDAKUTEN_PAIRS] = [
    ('は', 'ぱ'),
    ('ひ', 'ぴ'),
    ('ふ', 'ぷ'),
    ('へ', 'ぺ'),
    ('ほ', 'ぽ'),
];

/// 假名输入态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KanaState {
    /// 罗马字输入
    Roman,
    /// 假名直打
    Kana,
}

impl KanaState {
    /// 账行取值。
    pub fn tag(self) -> &'static str {
        match self {
            KanaState::Roman => "roman",
            KanaState::Kana => "kana",
        }
    }

    /// 由账行取值反解。
    pub fn from_tag(tag: &str) -> Option<KanaState> {
        match tag {
            "roman" => Some(KanaState::Roman),
            "kana" => Some(KanaState::Kana),
            _ => None,
        }
    }

    /// 取反态。
    pub fn toggle(self) -> KanaState {
        match self {
            KanaState::Roman => KanaState::Kana,
            KanaState::Kana => KanaState::Roman,
        }
    }
}

/// 假名层展平下标。
pub fn kana_flat(row: usize, col: usize) -> Option<usize> {
    if row >= ROWS {
        return None;
    }
    let len = *KANA_ROW_LEN.get(row)? as usize;
    if col >= len {
        return None;
    }
    let start = *KANA_ROW_START.get(row)?;
    Some(start + col)
}

/// 假名层取格（行优先铺放，与五十音登记序一致）。
pub fn kana_at(row: usize, col: usize) -> Option<char> {
    let idx = kana_flat(row, col)?;
    JIS_KANA.get(idx).copied()
}

/// 假名层行长按登记相加 = 46。
pub fn kana_rows_ok() -> bool {
    let mut acc = 0usize;
    let mut i = 0usize;
    while i < ROWS {
        let len = match KANA_ROW_LEN.get(i) {
            Some(v) => *v as usize,
            None => return false,
        };
        let start = match KANA_ROW_START.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if start != acc {
            return false;
        }
        if len > COLS {
            return false;
        }
        acc += len;
        i += 1;
    }
    acc == KANA_CELLS
}

/// 假名层互异性：46 格两两不同。
pub fn kana_unique_ok() -> bool {
    if JIS_KANA.len() != KANA_CELLS {
        return false;
    }
    let mut i = 0usize;
    while i < JIS_KANA.len() {
        let a = match JIS_KANA.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if kana_at(0, 0) == None {
            return false;
        }
        let mut j = i + 1;
        while j < JIS_KANA.len() {
            let b = match JIS_KANA.get(j) {
                Some(v) => *v,
                None => return false,
            };
            if a == b {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    // 展平逐格与登记序逐格相同。
    let mut k = 0usize;
    while k < KANA_CELLS {
        let mut r = 0usize;
        let mut found = None;
        while r < ROWS {
            let len = match KANA_ROW_LEN.get(r) {
                Some(v) => *v as usize,
                None => return false,
            };
            let mut c = 0usize;
            while c < len {
                if kana_flat(r, c) == Some(k) {
                    found = kana_at(r, c);
                }
                c += 1;
            }
            r += 1;
        }
        if found != JIS_KANA.get(k).copied() {
            return false;
        }
        k += 1;
    }
    true
}

/// 清音 → 浊音。
pub fn dakuten_of(c: char) -> Option<char> {
    let mut i = 0usize;
    while i < DAKUTEN.len() {
        let p = match DAKUTEN.get(i) {
            Some(v) => *v,
            None => return None,
        };
        if p.0 == c {
            return Some(p.1);
        }
        i += 1;
    }
    None
}

/// 浊音 → 清音（反查）。
pub fn dakuten_inv(c: char) -> Option<char> {
    let mut i = 0usize;
    while i < DAKUTEN.len() {
        let p = match DAKUTEN.get(i) {
            Some(v) => *v,
            None => return None,
        };
        if p.1 == c {
            return Some(p.0);
        }
        i += 1;
    }
    None
}

/// 清音 → 半浊音。
pub fn handakuuten_of(c: char) -> Option<char> {
    let mut i = 0usize;
    while i < HANDAKUTEN.len() {
        let p = match HANDAKUTEN.get(i) {
            Some(v) => *v,
            None => return None,
        };
        if p.0 == c {
            return Some(p.1);
        }
        i += 1;
    }
    None
}

/// 半浊音 → 清音（反查）。
pub fn handakuuten_inv(c: char) -> Option<char> {
    let mut i = 0usize;
    while i < HANDAKUTEN.len() {
        let p = match HANDAKUTEN.get(i) {
            Some(v) => *v,
            None => return None,
        };
        if p.1 == c {
            return Some(p.0);
        }
        i += 1;
    }
    None
}

/// 清浊往返：20 对 + 5 对全部两次回原，且目标互不重叠。
pub fn voicing_roundtrip_ok() -> bool {
    let mut i = 0usize;
    while i < DAKUTEN.len() {
        let p = match DAKUTEN.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if p.0 == p.1 {
            return false;
        }
        if dakuten_of(p.0) != Some(p.1) || dakuten_inv(p.1) != Some(p.0) {
            return false;
        }
        if dakuten_inv(dakuten_of(p.0).unwrap_or('\0')) != Some(p.0) {
            return false;
        }
        let mut j = i + 1;
        while j < DAKUTEN.len() {
            let q = match DAKUTEN.get(j) {
                Some(v) => *v,
                None => return false,
            };
            if q.0 == p.0 || q.1 == p.1 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    let mut k = 0usize;
    while k < HANDAKUTEN.len() {
        let p = match HANDAKUTEN.get(k) {
            Some(v) => *v,
            None => return false,
        };
        if handakuuten_of(p.0) != Some(p.1) || handakuuten_inv(p.1) != Some(p.0) {
            return false;
        }
        // は 行同时有浊音与半浊音，但目标码位必须互不相同。
        if let Some(d) = dakuten_of(p.0) {
            if d == p.1 {
                return false;
            }
        }
        let mut m = k + 1;
        while m < HANDAKUTEN.len() {
            let q = match HANDAKUTEN.get(m) {
                Some(v) => *v,
                None => return false,
            };
            if q.0 == p.0 || q.1 == p.1 {
                return false;
            }
            m += 1;
        }
        k += 1;
    }
    true
}

/// 假名态往返：切换两次回原态，且罗马字态不产生假名直打。
pub fn kana_state_ok() -> bool {
    if !kana_rows_ok() || !kana_unique_ok() || !voicing_roundtrip_ok() {
        return false;
    }
    let mut s = KanaState::Roman;
    let mut i = 0usize;
    while i < 8 {
        s = s.toggle();
        if i % 2 == 1 && s != KanaState::Roman {
            return false;
        }
        if i % 2 == 0 && s != KanaState::Kana {
            return false;
        }
        i += 1;
    }
    if KanaState::from_tag(KanaState::Roman.tag()) != Some(KanaState::Roman) {
        return false;
    }
    if KanaState::from_tag("kana") != Some(KanaState::Kana) {
        return false;
    }
    if KanaState::from_tag("katakana").is_some() {
        return false;
    }
    true
}

// ── 黄金全表核对 ─────────────────────────────────────────────────
/// 假名黄金 30 组（id 73..=102，逐组手工核对）。
pub const KANA_GOLD_TABLE: [KanaGold; GOLD_KANA] = [
    KanaGold { id: 73, row: 0, col: 0, kana: 'あ' },
    KanaGold { id: 74, row: 0, col: 1, kana: 'い' },
    KanaGold { id: 75, row: 0, col: 2, kana: 'う' },
    KanaGold { id: 76, row: 0, col: 3, kana: 'え' },
    KanaGold { id: 77, row: 0, col: 4, kana: 'お' },
    KanaGold { id: 78, row: 0, col: 5, kana: 'か' },
    KanaGold { id: 79, row: 0, col: 6, kana: 'き' },
    KanaGold { id: 80, row: 0, col: 7, kana: 'く' },
    KanaGold { id: 81, row: 0, col: 8, kana: 'け' },
    KanaGold { id: 82, row: 0, col: 9, kana: 'こ' },
    KanaGold { id: 83, row: 0, col: 10, kana: 'さ' },
    KanaGold { id: 84, row: 0, col: 11, kana: 'し' },
    KanaGold { id: 85, row: 0, col: 12, kana: 'す' },
    KanaGold { id: 86, row: 1, col: 0, kana: 'せ' },
    KanaGold { id: 87, row: 1, col: 1, kana: 'そ' },
    KanaGold { id: 88, row: 1, col: 2, kana: 'た' },
    KanaGold { id: 89, row: 1, col: 3, kana: 'ち' },
    KanaGold { id: 90, row: 1, col: 4, kana: 'つ' },
    KanaGold { id: 91, row: 1, col: 5, kana: 'て' },
    KanaGold { id: 92, row: 1, col: 6, kana: 'と' },
    KanaGold { id: 93, row: 1, col: 7, kana: 'な' },
    KanaGold { id: 94, row: 1, col: 8, kana: 'に' },
    KanaGold { id: 95, row: 2, col: 0, kana: 'は' },
    KanaGold { id: 96, row: 2, col: 1, kana: 'ひ' },
    KanaGold { id: 97, row: 2, col: 2, kana: 'ふ' },
    KanaGold { id: 98, row: 2, col: 3, kana: 'へ' },
    KanaGold { id: 99, row: 2, col: 4, kana: 'ほ' },
    KanaGold { id: 100, row: 2, col: 5, kana: 'ま' },
    KanaGold { id: 101, row: 2, col: 6, kana: 'み' },
    KanaGold { id: 102, row: 2, col: 7, kana: 'む' },
];

/// 单条国际黄金核对。
pub fn gold_check(g: &Gold) -> bool {
    if g.id == 0 || g.id > GOLD_INTL as u16 {
        return false;
    }
    if g.layout.index() >= LAYOUTS {
        return false;
    }
    match g.key() {
        Some(v) => v == g.expect,
        None => false,
    }
}

impl Gold {
    /// 正向取值（行列越界即 `None`）。
    pub fn key(&self) -> Option<char> {
        self.layout.key(self.layer, self.row as usize, self.col as usize)
    }

    /// 摘要：把布局、层、行列与期望码位折叠成 64 位。
    pub fn digest(&self) -> u64 {
        let mut h = p_core::FNV_OFF;
        h = p_core::fold_u32(h, self.id as u32);
        h = p_core::fold_byte(h, self.layout.index() as u8);
        h = p_core::fold_byte(h, self.layer.index() as u8);
        h = p_core::fold_byte(h, self.row);
        h = p_core::fold_byte(h, self.col);
        p_core::fold_u32(h, self.expect as u32)
    }
}

impl KanaGold {
    /// 正向取假名。
    pub fn kana(&self) -> Option<char> {
        kana_at(self.row as usize, self.col as usize)
    }

    /// 摘要：序号 + 行列 + 假名码位。
    pub fn digest(&self) -> u64 {
        let mut h = p_core::FNV_OFF;
        h = p_core::fold_u32(h, self.id as u32);
        h = p_core::fold_byte(h, self.row);
        h = p_core::fold_byte(h, self.col);
        p_core::fold_u32(h, self.kana as u32)
    }
}

/// 单条假名黄金核对。
pub fn kana_gold_check(g: &KanaGold) -> bool {
    if g.id <= GOLD_INTL as u16 || g.id > GOLD_TOTAL as u16 {
        return false;
    }
    match g.kana() {
        Some(v) => v == g.kana,
        None => false,
    }
}

/// 某布局的黄金命中数。
pub fn gold_hits(id: IntlId) -> usize {
    let mut n = 0usize;
    let mut i = 0usize;
    while i < GOLD_TABLE.len() {
        let g = match GOLD_TABLE.get(i) {
            Some(v) => *v,
            None => break,
        };
        if g.layout == id && gold_check(&g) {
            n += 1;
        }
        i += 1;
    }
    n
}

/// 黄金全表完整性：数量、序号连续、位置互异、逐条命中。
pub fn golden_intl_ok() -> bool {
    if GOLD_TABLE.len() != GOLD_INTL {
        return false;
    }
    let mut i = 0usize;
    while i < GOLD_TABLE.len() {
        let g = match GOLD_TABLE.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if g.id != i as u16 + 1 {
            return false;
        }
        if !gold_check(&g) {
            return false;
        }
        if g.row as usize >= ROWS {
            return false;
        }
        if g.col as usize >= COLS {
            return false;
        }
        let mut j = i + 1;
        while j < GOLD_TABLE.len() {
            let q = match GOLD_TABLE.get(j) {
                Some(v) => *v,
                None => return false,
            };
            if q.id == g.id {
                return false;
            }
            if q.layout == g.layout
                && q.layer == g.layer
                && q.row == g.row
                && q.col == g.col
            {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    // 每组布局恰 12 条。
    let mut k = 0usize;
    while k < ALL_LAYOUTS.len() {
        let id = match ALL_LAYOUTS.get(k) {
            Some(v) => *v,
            None => return false,
        };
        if gold_hits(id) != GOLD_PER {
            return false;
        }
        k += 1;
    }
    true
}

/// 假名黄金完整性：数量、序号续接国际段、逐条命中、位置互异。
pub fn golden_kana_ok() -> bool {
    if KANA_GOLD_TABLE.len() != GOLD_KANA {
        return false;
    }
    let mut i = 0usize;
    while i < KANA_GOLD_TABLE.len() {
        let g = match KANA_GOLD_TABLE.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if g.id != GOLD_INTL as u16 + i as u16 + 1 {
            return false;
        }
        if !kana_gold_check(&g) {
            return false;
        }
        let mut j = i + 1;
        while j < KANA_GOLD_TABLE.len() {
            let q = match KANA_GOLD_TABLE.get(j) {
                Some(v) => *v,
                None => return false,
            };
            if q.id == g.id || (q.row == g.row && q.col == g.col) {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    GOLD_INTL + GOLD_KANA == GOLD_TOTAL
}

/// 黄金全表（两段合看）逐条命中。
pub fn golden_all_ok() -> bool {
    if !golden_intl_ok() || !golden_kana_ok() {
        return false;
    }
    let mut n = 0usize;
    let mut i = 0usize;
    while i < GOLD_TABLE.len() {
        if GOLD_TABLE.get(i).map(gold_check) == Some(true) {
            n += 1;
        }
        i += 1;
    }
    let mut k = 0usize;
    while k < KANA_GOLD_TABLE.len() {
        if KANA_GOLD_TABLE.get(k).map(kana_gold_check) == Some(true) {
            n += 1;
        }
        k += 1;
    }
    n == GOLD_TOTAL
}

// ── 解析与账行 ───────────────────────────────────────────────────
/// 解析无符号十进制。
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

/// 布局账摘要。
pub fn layout_digest(id: IntlId, golden: u32, hit: u32, cells: u32) -> u64 {
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_byte(h, id.index() as u8);
    h = p_core::fold_u32(h, golden);
    h = p_core::fold_u32(h, hit);
    h = p_core::fold_u32(h, cells);
    let s = id.signature();
    let mut i = 0usize;
    while i < s.len() {
        if let Some(b) = s.get(i) {
            h = p_core::fold_byte(h, *b);
        }
        i += 1;
    }
    h
}

/// 专属键账摘要。
pub fn special_digest(slot: u32, s: Special, role: Role) -> u64 {
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, slot);
    h = p_core::fold_byte(h, s.index() as u8);
    h = p_core::fold_byte(h, s.pos());
    h = p_core::fold_u32(h, s.label().len() as u32);
    let rb = role.tag();
    let mut i = 0usize;
    while i < rb.len() {
        if let Some(b) = rb.as_bytes().get(i) {
            h = p_core::fold_byte(h, *b);
        }
        i += 1;
    }
    h
}

/// 渲染布局账行。
pub fn render_layout_row(id: IntlId) -> String {
    let hit = gold_hits(id) as u32;
    format!(
        "{}|layout={}|golden={}|hit={}|cells={}|sig={}|digest={:016x}",
        LAY_TAG,
        id.tag(),
        GOLD_PER as u32,
        hit,
        CELLS as u32,
        id.sig_hex(),
        layout_digest(id, GOLD_PER as u32, hit, CELLS as u32)
    )
}

/// 回读布局账行：重算命中数、签名与摘要。
pub fn audit_layout_row(row: &str) -> bool {
    let f = match row_fields(row, LAY_TAG, &LAY_KEYS) {
        Some(v) if v.len() == LAY_KEYS.len() + 1 => v,
        _ => return false,
    };
    let id = match f.first().copied().and_then(IntlId::from_tag) {
        Some(v) => v,
        None => return false,
    };
    let golden = match f.get(1).copied().and_then(parse_u32) {
        Some(v) => v,
        None => return false,
    };
    let hit = match f.get(2).copied().and_then(parse_u32) {
        Some(v) => v,
        None => return false,
    };
    let cells = match f.get(3).copied().and_then(parse_u32) {
        Some(v) => v,
        None => return false,
    };
    if golden != GOLD_PER as u32 || cells != CELLS as u32 {
        return false;
    }
    if hit != gold_hits(id) as u32 {
        return false;
    }
    if f.get(4).copied() != Some(id.sig_hex().as_str()) {
        return false;
    }
    match f.get(5).copied().and_then(parse_hex16) {
        Some(v) => v == layout_digest(id, golden, hit, cells),
        None => false,
    }
}

/// 渲染专属键账行。
pub fn render_special_row(slot: usize, s: Special) -> String {
    format!(
        "{}|slot={}|key={}|pos={}|role={}|digest={:016x}",
        KANA_TAG,
        slot as u32,
        s.tag(),
        s.pos(),
        s.role().tag(),
        special_digest(slot as u32, s, s.role())
    )
}

/// 回读专属键账行：重算位置、角色与摘要。
pub fn audit_special_row(row: &str) -> bool {
    let f = match row_fields(row, KANA_TAG, &KANA_KEYS) {
        Some(v) if v.len() == KANA_KEYS.len() + 1 => v,
        _ => return false,
    };
    let slot = match f.first().copied().and_then(parse_u32) {
        Some(v) if (v as usize) < KANA_ROW_SLOTS => v,
        _ => return false,
    };
    let s = match f.get(1).copied().and_then(Special::from_tag) {
        Some(v) => v,
        None => return false,
    };
    let pos = match f.get(2).copied().and_then(parse_u32) {
        Some(v) => v,
        None => return false,
    };
    let role = match f.get(3).copied().and_then(Role::from_tag) {
        Some(v) => v,
        None => return false,
    };
    if pos != s.pos() as u32 || role != s.role() {
        return false;
    }
    if slot as usize != s.index() {
        return false;
    }
    match f.get(4).copied().and_then(parse_hex16) {
        Some(v) => v == special_digest(slot, s, role),
        None => false,
    }
}

/// 导出布局账行（六组各一行）。
pub fn export_rows() -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < ROW_SLOTS {
        if let Some(id) = ALL_LAYOUTS.get(i) {
            out.push(render_layout_row(*id));
        }
        i += 1;
    }
    out
}

/// 导出专属键账行（八键各一行）。
pub fn export_special_rows() -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < KANA_ROW_SLOTS {
        if let Some(s) = ALL_SPECIALS.get(i) {
            out.push(render_special_row(i, *s));
        }
        i += 1;
    }
    out
}

/// 账行稳定性：两族账行逐行可回读、槽位互异、复跑一致。
pub fn rows_stable() -> bool {
    let a = export_rows();
    let b = export_rows();
    if a.len() != ROW_SLOTS || b.len() != ROW_SLOTS {
        return false;
    }
    let sa = export_special_rows();
    let sb = export_special_rows();
    if sa.len() != KANA_ROW_SLOTS || sb.len() != KANA_ROW_SLOTS {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        match b.get(i) {
            Some(x) if x == &a[i] => {}
            _ => return false,
        }
        let row = match a.get(i) {
            Some(v) => v.as_str(),
            None => return false,
        };
        if !audit_layout_row(row) {
            return false;
        }
        let mut j = i + 1;
        while j < a.len() {
            if a.get(j) == Some(&a[i]) {
                return false;
            }
            j += 1;
        }
        match (sa.get(i), sb.get(i)) {
            (Some(x), Some(y)) if *x == *y => {}
            _ => return false,
        }
        if let Some(srow) = sa.get(i) {
            if !audit_special_row(srow.as_str()) {
                return false;
            }
        }
        i += 1;
    }
    let mut k = ROW_SLOTS;
    while k < sa.len() {
        match (sa.get(k), sb.get(k)) {
            (Some(x), Some(y)) if *x == *y => {}
            _ => return false,
        }
        if let Some(srow) = sa.get(k) {
            if !audit_special_row(srow.as_str()) {
                return false;
            }
        }
        k += 1;
    }
    true
}

/// 篡改探针：把账行最后一个字符改掉（摘要末位必变）。
pub fn tamper_tail(row: &str) -> String {
    let mut out = String::new();
    let bytes = row.as_bytes();
    let mut i = 0usize;
    while let Some(b) = bytes.get(i) {
        if i + 1 == bytes.len() {
            out.push(if *b == b'0' { '1' } else { '0' });
        } else {
            out.push(*b as char);
        }
        i += 1;
    }
    out
}

// ── 判据 ─────────────────────────────────────────────────────────
/// 八项判据（F49101 四项 + F49102 四项）。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out = Vec::new();

    // 1) 六组布局互异：签名与标签两两不同，表长按登记自洽。
    let distinct = layouts_distinct();
    let tbl = tables_ok();
    let len_ok = row_len_ok();
    out.push((
        "F49101/六组布局互异",
        distinct && tbl && len_ok,
        format!("distinct={} tables={} row_len={}", distinct, tbl, len_ok),
    ));

    // 2) 黄金集对账：72 对逐条命中且每组恰 12 条。
    let gi = golden_intl_ok();
    let hits: usize = (0..LAYOUTS)
        .filter_map(IntlId::from_index)
        .map(gold_hits)
        .sum();
    out.push((
        "F49101/黄金集对账",
        gi && hits == GOLD_INTL,
        format!("intl={} hits={}/{}", gi, hits, GOLD_INTL),
    ));

    // 3) 层位反查：表内码位都能反查回正向槽位；AZERTY 倒置成立。
    let rev = reverse_lookup_ok();
    let inv = azerty_inversion_ok();
    let slot_ok = IntlId::from_index(LAYOUTS).is_none()
        && Layer::from_index(LAYERS_TOTAL).is_none()
        && IntlId::from_tag("azerty") == Some(IntlId::French);
    out.push((
        "F49101/层位反查",
        rev && inv && slot_ok,
        format!("reverse={} inversion={} bounds={}", rev, inv, slot_ok),
    ));

    // 4) 段位与码面：主段 14 码 + JIS 子窗 8 码全部可反解。
    let guard = segment_guard();
    let face = code_face_ok();
    let span_ok = SEG_BASE + SEG_SPAN > JIS_BASE + 0x09;
    out.push((
        "F49101/段位与码面",
        guard && face && span_ok,
        format!("guard={} face={} seg=0x{:05x}", guard, face, SEG_BASE),
    ));

    // 5) JIS 键数清点：九组 101 + 专属 8 = 109。
    let cnt = jis_count_ok();
    let total = jis_main_total() + JIS_SPECIAL;
    out.push((
        "F49102/JIS键数清点",
        cnt && total == JIS_KEYS,
        format!("count={} total={}", cnt, total),
    ));

    // 6) 空格前后键与假名黄金。
    let nb = space_neighbors_ok();
    let kg = golden_kana_ok();
    out.push((
        "F49102/空格前后键",
        nb && kg,
        format!("neighbor={} kana_gold={}", nb, kg),
    ));

    // 7) 假名态与清浊往返。
    let st = kana_state_ok();
    let vc = voicing_roundtrip_ok();
    let all = golden_all_ok();
    out.push((
        "F49102/假名态往返",
        st && vc && all,
        format!("state={} voicing={} all={}", st, vc, all),
    ));

    // 8) 账行回读与复跑：两族账行可回读、拒篡改、复跑一致。
    let lay = export_rows();
    let sp = export_special_rows();
    let mut tamper_ok = lay.len() == ROW_SLOTS && sp.len() == KANA_ROW_SLOTS;
    let mut i = 0usize;
    while i < lay.len() {
        let row = match lay.get(i) {
            Some(v) => v.as_str(),
            None => break,
        };
        if !audit_layout_row(row)
            || audit_layout_row(&tamper_tail(row))
            || audit_layout_row("INTL101|x")
        {
            tamper_ok = false;
        }
        i += 1;
    }
    let mut k = 0usize;
    while k < sp.len() {
        let row = match sp.get(k) {
            Some(v) => v.as_str(),
            None => break,
        };
        if !audit_special_row(row) || audit_special_row(&tamper_tail(row)) {
            tamper_ok = false;
        }
        k += 1;
    }
    out.push((
        "F49102/账行回读",
        tamper_ok && rows_stable(),
        format!(
            "layout_rows={} special_rows={} stable={}",
            lay.len(),
            sp.len(),
            rows_stable()
        ),
    ));

    out
}

/// 层数（与 `Layer` 变体数一致，供边界判据用）。
pub const LAYERS_TOTAL: usize = 2;

/// 入口：八项判据写入 CheckSet。
pub fn run_unx49101_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F49101 国际布局黄金集与 JIS 语义");
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
        assert_eq!(SEG_BASE, 0x73780);
        assert_eq!(SEG_SPAN, 0x40);
        assert_eq!(JIS_BASE, SEG_BASE + 0x11);
        assert_eq!(JIS_BASE + 0x08, SEG_BASE + 0x19);
        assert!(JIS_BASE + 0x08 < SEG_BASE + SEG_SPAN);
    }

    #[test]
    fn row_geometry_adds_up() {
        assert_eq!(ROWS, 4);
        assert_eq!(ROW_LEN.iter().map(|v| *v as usize).sum::<usize>(), CELLS);
        assert_eq!(ROW_START[0], 0);
        assert_eq!(ROW_START[1], 12);
        assert_eq!(ROW_START[2], 24);
        assert_eq!(ROW_START[3], 35);
        assert!(row_len_ok());
    }

    #[test]
    fn kana_row_geometry_adds_up() {
        assert_eq!(
            KANA_ROW_LEN.iter().map(|v| *v as usize).sum::<usize>(),
            KANA_CELLS
        );
        assert!(kana_rows_ok());
    }

    #[test]
    fn code_values_are_sequential() {
        let mut i = 0usize;
        while i < ALL.len() {
            let c = match ALL.get(i) {
                Some(v) => *v,
                None => panic!("缺码"),
            };
            assert_eq!(c.val(), SEG_BASE + 1 + i as u32);
            i += 1;
        }
        let mut j = 0usize;
        while j < ALL_JIS.len() {
            let c = match ALL_JIS.get(j) {
                Some(v) => *v,
                None => panic!("缺子码"),
            };
            assert_eq!(c.val(), JIS_BASE + 1 + j as u32);
            j += 1;
        }
    }

    #[test]
    fn code_details_are_distinct() {
        assert!(code_face_ok());
        assert_eq!(ALL.len(), 14);
        assert_eq!(ALL_JIS.len(), 8);
    }

    #[test]
    fn detail_resolves_every_registered_code() {
        let mut i = 0usize;
        while i < ALL.len() {
            let c = match ALL.get(i) {
                Some(v) => *v,
                None => panic!("缺码"),
            };
            assert_eq!(detail(c.val()), c.detail());
            i += 1;
        }
        let mut j = 0usize;
        while j < ALL_JIS.len() {
            let c = match ALL_JIS.get(j) {
                Some(v) => *v,
                None => panic!("缺子码"),
            };
            assert_eq!(detail(c.val()), c.detail());
            j += 1;
        }
    }

    #[test]
    fn detail_rejects_out_of_segment() {
        assert_eq!(detail(SEG_BASE), UNKNOWN);
        assert_eq!(detail(SEG_BASE + SEG_SPAN), UNKNOWN);
        assert_eq!(detail(0), UNKNOWN);
        assert_eq!(detail(JIS_BASE), UNKNOWN);
        assert_eq!(detail(JIS_BASE + 0x09), UNKNOWN);
        assert_eq!(detail(SEG_BASE + 0x0F), UNKNOWN);
    }

    #[test]
    fn segment_guard_gate() {
        assert!(segment_guard());
    }

    #[test]
    fn layout_index_and_tag_roundtrip() {
        let mut i = 0usize;
        while i < ALL_LAYOUTS.len() {
            let id = match ALL_LAYOUTS.get(i) {
                Some(v) => *v,
                None => panic!("缺布局"),
            };
            assert_eq!(id.index(), i);
            assert_eq!(IntlId::from_index(i), Some(id));
            assert_eq!(IntlId::from_tag(id.tag()), Some(id));
            i += 1;
        }
        assert_eq!(IntlId::from_index(LAYOUTS), None);
        assert_eq!(IntlId::from_tag("us"), None);
        assert_eq!(IntlId::from_tag(""), None);
    }

    #[test]
    fn layer_index_and_tag_roundtrip() {
        assert_eq!(Layer::Base.index(), 0);
        assert_eq!(Layer::Shift.index(), 1);
        assert_eq!(Layer::from_index(0), Some(Layer::Base));
        assert_eq!(Layer::from_index(1), Some(Layer::Shift));
        assert_eq!(Layer::from_index(2), None);
        assert_eq!(Layer::from_tag("base"), Some(Layer::Base));
        assert_eq!(Layer::from_tag("shift"), Some(Layer::Shift));
        assert_eq!(Layer::from_tag("altgr"), None);
        assert_eq!(LAYERS_TOTAL, 2);
    }

    #[test]
    fn tables_gate() {
        assert!(tables_ok());
        assert!(row_len_ok());
    }

    #[test]
    fn base_table_sizes_match_constant() {
        assert_eq!(FR_BASE.len(), CELLS);
        assert_eq!(DE_BASE.len(), CELLS);
        assert_eq!(IT_BASE.len(), CELLS);
        assert_eq!(ES_BASE.len(), CELLS);
        assert_eq!(PT_BASE.len(), CELLS);
        assert_eq!(NO_BASE.len(), CELLS);
        assert_eq!(FR_SHIFT.len(), 12);
        assert_eq!(NO_SHIFT.len(), 12);
    }

    #[test]
    fn base_tables_are_registered_in_index_order() {
        let mut i = 0usize;
        while i < ALL_LAYOUTS.len() {
            let id = match ALL_LAYOUTS.get(i) {
                Some(v) => *v,
                None => panic!("缺布局"),
            };
            let a = id.base_table();
            let b = match BASE_TABLES.get(i) {
                Some(v) => *v,
                None => panic!("缺表"),
            };
            assert_eq!(a, b);
            i += 1;
        }
    }

    #[test]
    fn layout_signatures_are_pairwise_distinct() {
        assert!(layouts_distinct());
        let mut i = 0usize;
        while i < ALL_LAYOUTS.len() {
            let a = match ALL_LAYOUTS.get(i) {
                Some(v) => *v,
                None => panic!("缺布局"),
            };
            assert_ne!(a.signature(), [0u8; 8]);
            let mut j = i + 1;
            while j < ALL_LAYOUTS.len() {
                let b = match ALL_LAYOUTS.get(j) {
                    Some(v) => *v,
                    None => panic!("缺布局"),
                };
                assert_ne!(a.signature(), b.signature());
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn checksum_is_reproducible() {
        let mut i = 0usize;
        while i < ALL_LAYOUTS.len() {
            let id = match ALL_LAYOUTS.get(i) {
                Some(v) => *v,
                None => panic!("缺布局"),
            };
            assert_eq!(id.checksum(), id.checksum());
            assert_ne!(id.checksum(), p_core::FNV_OFF);
            i += 1;
        }
    }

    #[test]
    fn sig_hex_has_stable_width() {
        let s = match IntlId::French.sig_hex().chars().count() {
            v => v,
        };
        assert_eq!(s, 16);
        assert_eq!(IntlId::Nordic.sig_hex().len(), 16);
    }

    #[test]
    fn azerty_digit_row_is_inverted() {
        assert!(azerty_inversion_ok());
        assert_eq!(IntlId::French.key(Layer::Base, 0, 0), Some('&'));
        assert_eq!(IntlId::French.key(Layer::Shift, 0, 0), Some('1'));
        assert_eq!(IntlId::French.key(Layer::Shift, 0, 10), Some('°'));
        assert_eq!(IntlId::French.key(Layer::Shift, 0, 11), Some('+'));
    }

    #[test]
    fn qwertz_swaps_y_and_z() {
        assert_eq!(IntlId::German.key(Layer::Base, 1, 5), Some('z'));
        assert_eq!(IntlId::German.key(Layer::Base, 3, 0), Some('y'));
        assert_eq!(IntlId::French.key(Layer::Base, 1, 5), Some('y'));
    }

    #[test]
    fn letter_rows_uppercase_on_shift() {
        assert_eq!(IntlId::German.key(Layer::Shift, 1, 0), Some('Q'));
        assert_eq!(IntlId::Nordic.key(Layer::Shift, 3, 0), Some('Z'));
        assert_eq!(IntlId::Italian.key(Layer::Shift, 2, 0), Some('A'));
        // 非字母格在换档层原样保留。
        assert_eq!(IntlId::Spanish.key(Layer::Shift, 3, 7), Some(','));
    }

    #[test]
    fn bounds_reject_out_of_range_slots() {
        assert_eq!(IntlId::French.key(Layer::Base, ROWS, 0), None);
        assert_eq!(IntlId::French.key(Layer::Base, 3, 10), None);
        assert_eq!(IntlId::French.key(Layer::Base, 1, 12), None);
        assert_eq!(IntlId::French.key(Layer::Shift, 0, 12), None);
        assert_eq!(FR_BASE.get(ROWS * COLS), None);
    }

    #[test]
    fn reverse_lookup_gate() {
        assert!(reverse_lookup_ok());
    }

    #[test]
    fn code_slot_finds_first_match() {
        assert_eq!(code_slot(IntlId::French, Layer::Base, '&'), Some((0, 0)));
        assert_eq!(code_slot(IntlId::German, Layer::Base, 'ñ'), None);
        assert_eq!(code_slot(IntlId::Spanish, Layer::Base, 'ñ'), Some((2, 9)));
        assert_eq!(code_slot(IntlId::French, Layer::Shift, '1'), Some((0, 0)));
    }

    #[test]
    fn duplicate_codepoints_resolve_to_readable_slot() {
        // FR 数字行第 10 格与上字母行第 11 格同为 'à'：反查必须给出可回读槽位。
        let s = match code_slot(IntlId::French, Layer::Base, 'à') {
            Some(v) => v,
            None => panic!("反查落空"),
        };
        assert_eq!(IntlId::French.key(Layer::Base, s.0, s.1), Some('à'));
    }

    #[test]
    fn gold_table_size_matches_registered() {
        assert_eq!(GOLD_TABLE.len(), GOLD_INTL);
        assert_eq!(GOLD_INTL, 72);
        assert_eq!(KANA_GOLD_TABLE.len(), GOLD_KANA);
        assert_eq!(GOLD_TOTAL, 102);
    }

    #[test]
    fn golden_intl_gate() {
        assert!(golden_intl_ok());
    }

    #[test]
    fn golden_kana_gate() {
        assert!(golden_kana_ok());
    }

    #[test]
    fn golden_all_gate() {
        assert!(golden_all_ok());
    }

    #[test]
    fn each_layout_has_twelve_goldens() {
        let mut i = 0usize;
        while i < ALL_LAYOUTS.len() {
            let id = match ALL_LAYOUTS.get(i) {
                Some(v) => *v,
                None => panic!("缺布局"),
            };
            assert_eq!(gold_hits(id), GOLD_PER);
            i += 1;
        }
    }

    #[test]
    fn gold_ids_are_contiguous_and_unique() {
        let mut i = 0usize;
        while i < GOLD_TABLE.len() {
            let g = match GOLD_TABLE.get(i) {
                Some(v) => *v,
                None => panic!("缺黄金"),
            };
            assert_eq!(g.id, i as u16 + 1);
            let mut j = i + 1;
            while j < GOLD_TABLE.len() {
                if let Some(q) = GOLD_TABLE.get(j) {
                    assert_ne!(q.id, g.id);
                    if q.layout == g.layout && q.layer == g.layer && q.row == g.row && q.col == g.col {
                        panic!("位置撞槽");
                    }
                }
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn gold_check_rejects_unknown_ids() {
        let bad = Gold {
            id: 0,
            layout: IntlId::French,
            layer: Layer::Base,
            row: 0,
            col: 0,
            expect: '&',
        };
        assert!(!gold_check(&bad));
        let over = Gold {
            id: GOLD_INTL as u16 + 1,
            layout: IntlId::French,
            layer: Layer::Base,
            row: 0,
            col: 0,
            expect: '&',
        };
        assert!(!gold_check(&over));
    }

    #[test]
    fn gold_check_rejects_wrong_expectation() {
        let mut g = match GOLD_TABLE.first() {
            Some(v) => *v,
            None => panic!("缺黄金"),
        };
        g.expect = 'Z';
        assert!(!gold_check(&g));
    }

    #[test]
    fn gold_digest_is_reproducible() {
        let g = match GOLD_TABLE.get(7) {
            Some(v) => *v,
            None => panic!("缺黄金"),
        };
        assert_eq!(g.digest(), g.digest());
        assert_ne!(g.digest(), p_core::FNV_OFF);
    }

    #[test]
    fn gold_digest_varies_across_table() {
        let mut seen: Vec<u64> = Vec::new();
        let mut i = 0usize;
        while i < GOLD_TABLE.len() {
            let g = match GOLD_TABLE.get(i) {
                Some(v) => *v,
                None => panic!("缺黄金"),
            };
            let d = g.digest();
            let mut j = 0usize;
            let mut dup = false;
            while j < seen.len() {
                if seen[j] == d {
                    dup = true;
                }
                j += 1;
            }
            assert!(!dup, "黄金摘要互异");
            seen.push(d);
            i += 1;
        }
        assert_eq!(seen.len(), GOLD_INTL);
    }

    #[test]
    fn kana_gold_ids_continue_after_intl() {
        let first = match KANA_GOLD_TABLE.first() {
            Some(v) => *v,
            None => panic!("缺假名黄金"),
        };
        assert_eq!(first.id, (GOLD_INTL + 1) as u16);
        let last = match KANA_GOLD_TABLE.last() {
            Some(v) => *v,
            None => panic!("缺假名黄金"),
        };
        assert_eq!(last.id, GOLD_TOTAL as u16);
    }

    #[test]
    fn kana_gold_rejects_misranged_ids() {
        let mut g = match KANA_GOLD_TABLE.first() {
            Some(v) => *v,
            None => panic!("缺假名黄金"),
        };
        g.id = 1;
        assert!(!kana_gold_check(&g));
        g.id = GOLD_TOTAL as u16 + 1;
        assert!(!kana_gold_check(&g));
    }

    #[test]
    fn kana_placement_follows_gojuon() {
        assert_eq!(kana_at(0, 0), Some('あ'));
        assert_eq!(kana_at(0, 12), Some('す'));
        assert_eq!(kana_at(1, 0), Some('せ'));
        assert_eq!(kana_at(2, 0), Some('は'));
        assert_eq!(kana_at(3, 9), Some('ん'));
        assert_eq!(kana_at(3, 10), None);
        assert_eq!(kana_at(ROWS, 0), None);
        assert_eq!(kana_flat(2, 11), None);
    }

    #[test]
    fn kana_table_is_unique() {
        assert!(kana_unique_ok());
        assert_eq!(JIS_KANA.len(), 46);
    }

    #[test]
    fn voicing_maps_roundtrip() {
        assert!(voicing_roundtrip_ok());
        assert_eq!(dakuten_of('か'), Some('が'));
        assert_eq!(dakuten_of('ぱ'), None);
        assert_eq!(dakuten_inv('が'), Some('か'));
        assert_eq!(dakuten_inv('か'), None);
        assert_eq!(handakuuten_of('は'), Some('ぱ'));
        assert_eq!(handakuuten_inv('ぱ'), Some('は'));
        assert_eq!(handakuuten_inv('ば'), None);
    }

    #[test]
    fn voicing_targets_do_not_collide() {
        let mut i = 0usize;
        while i < DAKUTEN.len() {
            let d = match DAKUTEN.get(i) {
                Some(v) => *v,
                None => panic!("缺浊音对"),
            };
            let mut j = 0usize;
            while j < HANDAKUTEN.len() {
                let h = match HANDAKUTEN.get(j) {
                    Some(v) => *v,
                    None => panic!("缺半浊音对"),
                };
                assert_ne!(d.1, h.1);
                j += 1;
            }
            i += 1;
        }
        assert_eq!(DAKUTEN.len(), 20);
        assert_eq!(HANDAKUTEN.len(), 5);
    }

    #[test]
    fn kana_state_toggle_returns_home() {
        assert!(kana_state_ok());
        let mut s = KanaState::Roman;
        let mut i = 0usize;
        while i < 16 {
            s = s.toggle();
            i += 1;
        }
        assert_eq!(s, KanaState::Roman);
        assert_eq!(KanaState::Roman.tag(), "roman");
        assert_eq!(KanaState::from_tag("kana"), Some(KanaState::Kana));
    }

    #[test]
    fn jis_group_sum_is_main_block() {
        assert_eq!(jis_main_total(), JIS_MAIN);
        assert_eq!(JIS_MAIN + JIS_SPECIAL, JIS_KEYS);
        assert!(jis_count_ok());
    }

    #[test]
    fn jis_groups_are_labeled_uniquely() {
        let mut i = 0usize;
        while i < JIS_GROUPS.len() {
            let a = match JIS_GROUPS.get(i) {
                Some(v) => *v,
                None => panic!("缺分组"),
            };
            assert_ne!(a.count, 0);
            let mut j = i + 1;
            while j < JIS_GROUPS.len() {
                let b = match JIS_GROUPS.get(j) {
                    Some(v) => *v,
                    None => panic!("缺分组"),
                };
                assert_ne!(a.tag, b.tag);
                j += 1;
            }
            i += 1;
        }
        assert_eq!(JIS_GROUPS.len(), 9);
    }

    #[test]
    fn space_flanked_by_conversion_keys() {
        assert!(space_neighbors_ok());
        assert!(Special::Muhenkan.pos() < JIS_SPACE_POS);
        assert!(Special::Henkan.pos() > JIS_SPACE_POS);
        assert_eq!(Special::Muhenkan.role(), Role::SpaceLeft);
        assert_eq!(Special::Henkan.role(), Role::SpaceRight);
    }

    #[test]
    fn special_index_tag_and_pos_roundtrip() {
        let mut i = 0usize;
        while i < ALL_SPECIALS.len() {
            let s = match ALL_SPECIALS.get(i) {
                Some(v) => *v,
                None => panic!("缺专属键"),
            };
            assert_eq!(s.index(), i);
            assert_eq!(Special::from_index(i), Some(s));
            assert_eq!(Special::from_tag(s.tag()), Some(s));
            assert!(!s.label().is_empty());
            let mut j = i + 1;
            while j < ALL_SPECIALS.len() {
                if let Some(t) = ALL_SPECIALS.get(j) {
                    assert_ne!(t.pos(), s.pos());
                    assert_ne!(t.tag(), s.tag());
                }
                j += 1;
            }
            i += 1;
        }
        assert_eq!(Special::from_index(JIS_SPECIAL), None);
        assert_eq!(Special::from_tag("enter"), None);
    }

    #[test]
    fn role_tag_roundtrip() {
        let all = [
            Role::SpaceLeft,
            Role::SpaceRight,
            Role::Mode,
            Role::Lock,
            Role::Region,
        ];
        let mut i = 0usize;
        while i < all.len() {
            let r = match all.get(i) {
                Some(v) => *v,
                None => panic!("缺角色"),
            };
            assert_eq!(Role::from_tag(r.tag()), Some(r));
            i += 1;
        }
        assert_eq!(Role::from_tag("none"), None);
    }

    #[test]
    fn upper_helper_maps_letters() {
        assert_eq!(upper('a'), 'A');
        assert_eq!(upper('é'), 'É');
        assert_eq!(upper('ï'), 'Ï');
        assert_eq!(upper(','), ',');
        assert_eq!(upper('1'), '1');
    }

    #[test]
    fn layout_row_roundtrip() {
        let mut i = 0usize;
        while i < ALL_LAYOUTS.len() {
            let id = match ALL_LAYOUTS.get(i) {
                Some(v) => *v,
                None => panic!("缺布局"),
            };
            let row = render_layout_row(id);
            assert!(audit_layout_row(&row));
            assert!(row.starts_with(LAY_TAG));
            i += 1;
        }
    }

    #[test]
    fn layout_row_rejects_tamper_and_malformed() {
        let row = render_layout_row(IntlId::Italian);
        assert!(!audit_layout_row(&tamper_tail(&row)));
        assert!(!audit_layout_row(""));
        assert!(!audit_layout_row("INTL101|x"));
        assert!(!audit_layout_row("JISK101|slot=0|key=eisu|pos=39|role=region|digest=0000000000000000"));
    }

    #[test]
    fn layout_row_rejects_wrong_hit_count() {
        let row = render_layout_row(IntlId::Nordic);
        let bad = row.replace("|hit=12|", "|hit=11|");
        assert!(!audit_layout_row(&bad));
        let bad2 = row.replace("|cells=45|", "|cells=46|");
        assert!(!audit_layout_row(&bad2));
    }

    #[test]
    fn special_row_roundtrip() {
        let mut i = 0usize;
        while i < ALL_SPECIALS.len() {
            let s = match ALL_SPECIALS.get(i) {
                Some(v) => *v,
                None => panic!("缺专属键"),
            };
            let row = render_special_row(i, s);
            assert!(audit_special_row(&row));
            i += 1;
        }
    }

    #[test]
    fn special_row_rejects_tamper_and_mismatch() {
        let row = render_special_row(1, Special::Muhenkan);
        assert!(!audit_special_row(&tamper_tail(&row)));
        assert!(!audit_special_row("JISK101|x"));
        let wrong = row.replace("|pos=40|", "|pos=41|");
        assert!(!audit_special_row(&wrong));
        let swapped = row.replace("|slot=1|", "|slot=2|");
        assert!(!audit_special_row(&swapped));
    }

    #[test]
    fn export_counts_match_slots() {
        assert_eq!(export_rows().len(), ROW_SLOTS);
        assert_eq!(export_special_rows().len(), KANA_ROW_SLOTS);
    }

    #[test]
    fn exported_rows_are_pairwise_distinct() {
        let rows = export_rows();
        let mut i = 0usize;
        while i < rows.len() {
            let mut j = i + 1;
            while j < rows.len() {
                assert_ne!(rows[i], rows[j]);
                j += 1;
            }
            i += 1;
        }
        let sp = export_special_rows();
        let mut k = 0usize;
        while k < sp.len() {
            let mut m = k + 1;
            while m < sp.len() {
                assert_ne!(sp[k], sp[m]);
                m += 1;
            }
            k += 1;
        }
    }

    #[test]
    fn rows_stable_gate() {
        assert!(rows_stable());
    }

    #[test]
    fn tamper_tail_flips_only_tail() {
        assert_eq!(tamper_tail("ABC|1"), "ABC|0");
        assert_eq!(tamper_tail("ABC|0"), "ABC|1");
        assert_eq!(tamper_tail(""), "");
    }

    #[test]
    fn parse_u32_edges() {
        assert_eq!(parse_u32(""), None);
        assert_eq!(parse_u32("0"), Some(0));
        assert_eq!(parse_u32("0012"), Some(12));
        assert_eq!(parse_u32("a"), None);
        assert_eq!(parse_u32("-1"), None);
    }

    #[test]
    fn parse_hex16_edges() {
        assert_eq!(parse_hex16("0000000000000000"), Some(0));
        assert_eq!(parse_hex16("ffffffffffffffff"), Some(u64::MAX));
        assert_eq!(parse_hex16("FFFFFFFFFFFFFFFF"), Some(u64::MAX));
        assert_eq!(parse_hex16("ffff"), None);
        assert_eq!(parse_hex16("gggggggggggggggg"), None);
    }

    #[test]
    fn row_fields_splits_by_registered_keys() {
        let row = render_layout_row(IntlId::German);
        let f = match row_fields(&row, LAY_TAG, &LAY_KEYS) {
            Some(v) => v,
            None => panic!("拆行失败"),
        };
        assert_eq!(f.len(), LAY_KEYS.len() + 1);
        assert_eq!(f.first().copied(), Some("qwertz"));
        assert_eq!(f.get(1).copied(), Some("12"));
    }

    #[test]
    fn row_fields_rejects_bad_prefix() {
        assert!(row_fields("X|layout=a|golden=1|hit=1|cells=1|sig=x|digest=0000000000000000", LAY_TAG, &LAY_KEYS).is_none());
        assert!(row_fields("INTL101|layout=a", LAY_TAG, &LAY_KEYS).is_none());
    }

    #[test]
    fn digest_helpers_are_deterministic() {
        let a = layout_digest(IntlId::French, 12, 12, 45);
        let b = layout_digest(IntlId::French, 12, 12, 45);
        assert_eq!(a, b);
        assert_ne!(a, layout_digest(IntlId::French, 12, 11, 45));
        let c = special_digest(3, Special::Henkan, Role::SpaceRight);
        assert_eq!(c, special_digest(3, Special::Henkan, Role::SpaceRight));
        assert_ne!(c, special_digest(4, Special::Henkan, Role::SpaceRight));
    }

    #[test]
    fn extra_checks_count_and_names() {
        let items = extra_checks();
        assert_eq!(items.len(), 8);
        let mut a = 0usize;
        let mut b = 0usize;
        let mut i = 0usize;
        while i < items.len() {
            let c = match items.get(i) {
                Some(v) => v,
                None => panic!("缺判据"),
            };
            assert!(c.1);
            assert!(!c.2.is_empty());
            if c.0.starts_with("F49101/") {
                a += 1;
            } else if c.0.starts_with("F49102/") {
                b += 1;
            }
            i += 1;
        }
        assert_eq!(a, 4);
        assert_eq!(b, 4);
    }

    #[test]
    fn checkset_eight_green() {
        let s = run_unx49101_checks();
        assert_eq!(s.len(), 8);
        assert_eq!(s.passed_count(), 8);
        assert_eq!(s.failed_count(), 0);
        assert!(s.all_passed());
        assert!(!s.truncated());
        assert!(s.red_items_text().is_empty());
        assert_eq!(s.domain, "UNX-F49101 国际布局黄金集与 JIS 语义");
    }
}
