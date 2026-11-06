//! # unx49103 — B16 韩文两态组合与俄文/东欧 QWERTZ 布局套
//!
//! 卡面原文（任务板 CoRun·UNX-F2-B40）：
//!
//! > **F49103 韩文布局语义（한/영 切换键与两态组合，黄金 20 组）** 行数 400
//! > **F49104 俄文 ЙЦУКЕН 与东欧 QWERTZ 布局套（黄金 30 组）** 行数 380
//!
//! 验收锚点：
//!
//! - `UNX-F49104-J1` 判据锚定 §AI-62 §2：한/영 两态键位表自洽、初/中/终声
//!   组合恒等式在全音节区零漂移、黄金 20 组逐条命中。
//! - `UNX-F49105-J1` 判据锚定 §AI-62 §2：俄文 33 字母与东欧三套 QWERTZ
//!   黄金 30 组逐条命中，账行可回读、复跑一致。
//!
//! ## 闸序与理由
//!
//! 1. **表先于组合**：韩文两态各 46 格先登记（数字行跨态同码、字母行按态
//!    分列），组合引擎才有一致的输入符号集。表错一格，拼出来的音节就是在
//!    给错键位盖章。
//! 2. **换档不变式**：韩文态只有首字母行在换档时变形（紧声 ㅃㅉㄸㄲㅆ 与
//!    ㅒㅖ），中/下两行换档必须与基层逐格相等——韩文无大小写，若换档层被
//!    写成第三套键位，两态机就退化成三态。
//! 3. **组合恒等式**：`compose(l, v, t)` 与 `decompose(code)` 在完整音节区
//!    0xAC00..=0xD7A3 共 11172 码上互为逆运算，逐码回算不等即为表崩。
//!    这一闸只认恒等式，不认任何厂商音节表。
//! 4. **合成对可回推**：初声叠字 5 对、复元音 7 对、双收音 11 对，逐对要求
//!    合成结果落在对应索引表内，且合成前两个成分仍在表内——否则引擎会在
//!    下一键上拼出一个查不到索引的字。
//! 5. **한/영 切换**：连按两次必须回原态且翻转计数为偶；在途队列必须排空
//!    后才允许换态，越出 8 槽的键要记账为丢弃而不是静默吞掉。
//! 6. **东欧先互异后黄金**：四套布局（俄/捷/斯洛伐克/匈牙利）签名两两不同，
//!    数字行换档逐格不等于基层，之后才允许黄金 30 组入账。
//! 7. **西里尔清点**：俄语 33 字母（含 ё）逐字母能在俄文表内反查到槽位，
//!    换档层大写逐格回读；缺一个字母就是内核认为不存在的键。
//! 8. **账行回读**：两族账行（4 套东欧 + 5 面韩文账）导出→回读→篡改→复跑
//!    四步全绿才算入账。
//!
//! ## 与相邻模块的差异
//!
//! - `unx49101_layouts` 只管六组拉丁国际布局与日文 JIS 假名层；本模块的
//!   韩文两态与东欧四套不共享任何表，段号互斥
//!   （0x737C0..=0x737FF 对 0x73780..=0x737BF）。
//! - `unx49105_rtl_persist` 管 RTL 基向与两层配置持久化；本模块的组合引擎
//!   是「同态内多键合成一码」，与 RTL 的方向重排是两件正交的事。
//! - 东欧三套 QWERTZ 与 `unx49101` 的德语 QWERTZ 键位独立登记，不互抄：
//!   本模块的 QWERTZ 行带各语言变音槽，德语模块的换档数字行是另一套。
//!
//! ## 表数据来源声明
//!
//! 本模块键位表为 **内核登记序**：逐槽显式手写、逐行显式记长，判据考察的是
//! 「表自洽 + 组合恒等 + 黄金对命中 + 账行可回读」，不宣称与任一厂商印刷
//! 或任一国家标准码位表完全一致。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

// ── 段位与规模 ───────────────────────────────────────────────────
/// 本模块段基址（B16 第二槽）。
pub const SEG_BASE: u32 = 0x737C0;
/// 段宽（与相邻模块互斥）。
pub const SEG_SPAN: u32 = 0x40;
/// 韩文组合诊断子窗基址。
pub const KOR_BASE: u32 = SEG_BASE + 0x11;
/// 韩文诊断子窗码数。
pub const KOR_CODES: usize = 8;
/// 东欧布局套数（俄/捷/斯洛伐克/匈牙利）。
pub const EAST: usize = 4;
/// 单套布局行数。
pub const ROWS: usize = 4;
/// 单行最大键数（数字行 13）。
pub const COLS: usize = 13;
/// 每行键数：数字 13 / 上字母 12 / 中字 11 / 下字母 10。
pub const ROW_LEN: [u8; ROWS] = [13, 12, 11, 10];
/// 各行在扁平表中的起始下标。
pub const ROW_START: [usize; ROWS] = [0, 13, 25, 36];
/// 单套布局键位格数。
pub const CELLS: usize = 46;
/// 数字行换档格数。
pub const SHIFT_COLS: usize = 13;
/// 层数（基/换两态）。
pub const LAYERS_TOTAL: usize = 2;
/// 俄文黄金条数。
pub const GOLD_RU: usize = 12;
/// 捷克黄金条数。
pub const GOLD_CS: usize = 6;
/// 斯洛伐克黄金条数。
pub const GOLD_SK: usize = 6;
/// 匈牙利黄金条数。
pub const GOLD_HU: usize = 6;
/// 东欧黄金总数。
pub const GOLD_EAST: usize = GOLD_RU + GOLD_CS + GOLD_SK + GOLD_HU;
/// 韩文黄金组合数。
pub const GOLD_KOR: usize = 20;
/// 全模块黄金总数。
pub const GOLD_TOTAL: usize = GOLD_EAST + GOLD_KOR;
/// 初声（领音）数。
pub const LEADS: usize = 19;
/// 中声（元音）数。
pub const VOWELS: usize = 21;
/// 终声（收音）种类数（不含空收音）。
pub const TAILS: usize = 27;
/// 终声槽数（含 0 号空收音）。
pub const TAIL_SLOTS: usize = 28;
/// 完整音节总数 19×21×28。
pub const SYLLABLES: usize = LEADS * VOWELS * TAIL_SLOTS;
/// 音节区首码。
pub const SYL_START: u32 = 0xAC00;
/// 音节区末码。
pub const SYL_LAST: u32 = 0xD7A3;
/// 初声叠字合成对数。
pub const LEAD_MERGE: usize = 5;
/// 复元音合成对数。
pub const VOWEL_MERGE: usize = 7;
/// 双收音合成对数。
pub const TAIL_MERGE: usize = 11;
/// 韩文态数（라틴/한글）。
pub const KOR_STATES: usize = 2;
/// 韩文账面数（两态 + 三张索引表）。
pub const KOR_FACES: usize = 5;
/// 한/영 在途队列容量。
pub const PENDING_CAP: usize = 8;
/// 韩文切换键物理位。
pub const HAN_YEONG_POS: u8 = 56;
/// 韩文组合键（한자）物理位。
pub const HANJA_POS: u8 = 57;
/// 西里尔字母清点规模。
pub const CYRILLIC: usize = 33;
/// 东欧账行槽数（一套一行）。
pub const EAST_ROW_SLOTS: usize = EAST;
/// 韩文账行槽数。
pub const KOR_ROW_SLOTS: usize = KOR_FACES;
/// 东欧账行标签。
pub const EAST_TAG: &str = "EAST46";
/// 东欧账行键序。
pub const EAST_KEYS: [&str; 5] = ["layout", "golden", "hit", "cells", "sig"];
/// 韩文账行标签。
pub const KOR_TAG: &str = "KOR2FACE";
/// 韩文账行键序。
pub const KOR_KEYS: [&str; 4] = ["face", "count", "cells", "sig"];

// ── 诊断码 ───────────────────────────────────────────────────────
/// 未知诊断码说明。
pub const UNKNOWN: &str = "未知 B16 韩文/东欧布局诊断码";

/// 东欧与俄文布局诊断码（14 条，说明互不共用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EastCode {
    /// 行几何越窗
    RowBad,
    /// 表规模或行内重复
    TableBad,
    /// 布局标签/下标无法反查
    LayoutBad,
    /// 两套布局签名撞车
    SignatureBad,
    /// 数字行换档与基层同码
    ShiftBad,
    /// 西里尔字母清点缺字
    CyrillicBad,
    /// 大写回读不符
    CaseBad,
    /// 变音槽未登记
    DiacriticBad,
    /// 黄金条数或归属不符
    GoldenBad,
    /// 黄金对未命中期望码位
    ExpectBad,
    /// 码位反查不到槽位
    CodepointBad,
    /// 账行字段数或字段名不符
    FieldBad,
    /// 摘要与重算不符
    DigestBad,
    /// 处置链不完整
    Incomplete,
}

impl EastCode {
    /// 诊断码值。
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    /// 逐码说明（互不共用）。
    pub fn detail(self) -> &'static str {
        match self {
            EastCode::RowBad => "行记长或行首下标越出 13/12/11/10 窗口",
            EastCode::TableBad => "键位表格数不等于 46 或同一行内出现重复码位",
            EastCode::LayoutBad => "布局下标或标签无法反查到已登记的四套之一",
            EastCode::SignatureBad => "两套东欧布局折叠出同一签名，分派会混用键位",
            EastCode::ShiftBad => "数字行某格换档码与基层码相同，方向语义丢失",
            EastCode::CyrillicBad => "俄文 33 字母中至少一字母在表内反查不到",
            EastCode::CaseBad => "字母格换档码不等于该字母的大写形式",
            EastCode::DiacriticBad => "变音字母槽位缺失或落进了非字母格",
            EastCode::GoldenBad => "黄金条数与四套布局登记规模不符",
            EastCode::ExpectBad => "黄金对正向查表结果与期望码位不等",
            EastCode::CodepointBad => "表内码位无法反查回任一行列槽",
            EastCode::FieldBad => "东欧账行字段数或字段名与登记键序不符",
            EastCode::DigestBad => "东欧账行摘要与重算值不一致",
            EastCode::Incomplete => "东欧布局登记链未跑满全程",
        }
    }

    /// 面向运营的短句（与 `detail` 不同口径）。
    pub fn message(self) -> &'static str {
        match self {
            EastCode::RowBad => "行界越窗：请回四行记长",
            EastCode::TableBad => "表不自洽：请补 46 格并去重",
            EastCode::LayoutBad => "布局查无：请核对四套标签",
            EastCode::SignatureBad => "签名撞车：请重排变音槽",
            EastCode::ShiftBad => "换档同码：请改数字行符号",
            EastCode::CyrillicBad => "缺西里尔字母：请补清点表",
            EastCode::CaseBad => "大小写错位：请回大写映射",
            EastCode::DiacriticBad => "变音缺槽：请登记附加字母",
            EastCode::GoldenBad => "黄金不全：请补足三十条",
            EastCode::ExpectBad => "黄金不中：请核对期望码",
            EastCode::CodepointBad => "反查失败：请核对行首下标",
            EastCode::FieldBad => "字段错位：请核对键序",
            EastCode::DigestBad => "摘要失配：请重算账行",
            EastCode::Incomplete => "链路不全：请跑满全程",
        }
    }
}

/// 韩文两态组合诊断码（8 条，落在子窗内）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KorCode {
    /// 初声表越界
    LeadBad,
    /// 中声表越界
    VowelBad,
    /// 终声表越界
    TailBad,
    /// 组合式回算不符
    ComposeBad,
    /// 合成对无法回推
    MergeBad,
    /// 한/영 切换态漂移
    ToggleBad,
    /// 在途键丢失
    PendingBad,
    /// 韩文黄金组未命中
    GoldenBad,
}

impl KorCode {
    /// 诊断码值（子窗内顺序编号）。
    pub fn val(self) -> u32 {
        KOR_BASE + self as u32
    }

    /// 逐码说明（互不共用）。
    pub fn detail(self) -> &'static str {
        match self {
            KorCode::LeadBad => "初声索引越出 19 格窗口或表内重复",
            KorCode::VowelBad => "中声索引越出 21 格窗口或表内重复",
            KorCode::TailBad => "终声索引越出 28 槽窗口或表内重复",
            KorCode::ComposeBad => "音节码回算与原码不等的组合恒等式破裂",
            KorCode::MergeBad => "叠字/复元音/双收音合成结果不在索引表内",
            KorCode::ToggleBad => "한/영 连按两次未回到起始态",
            KorCode::PendingBad => "切换期在途键越出 8 槽被丢弃却未记账",
            KorCode::GoldenBad => "韩文黄金 20 组至少一条未拼出期望音节",
        }
    }

    /// 面向运营的短句（与 `detail` 不同口径）。
    pub fn message(self) -> &'static str {
        match self {
            KorCode::LeadBad => "初声越界：请回十九格",
            KorCode::VowelBad => "中声越界：请回二十一格",
            KorCode::TailBad => "终声越界：请回二十八槽",
            KorCode::ComposeBad => "组合漂移：请核恒等式",
            KorCode::MergeBad => "合成失败：请核三张对表",
            KorCode::ToggleBad => "切换不回：请查态机",
            KorCode::PendingBad => "在途丢键：请记账补投",
            KorCode::GoldenBad => "韩金不中：请重跑二十组",
        }
    }
}

/// 全部东欧诊断码（顺序即值序）。
pub const ALL: [EastCode; 14] = [
    EastCode::RowBad,
    EastCode::TableBad,
    EastCode::LayoutBad,
    EastCode::SignatureBad,
    EastCode::ShiftBad,
    EastCode::CyrillicBad,
    EastCode::CaseBad,
    EastCode::DiacriticBad,
    EastCode::GoldenBad,
    EastCode::ExpectBad,
    EastCode::CodepointBad,
    EastCode::FieldBad,
    EastCode::DigestBad,
    EastCode::Incomplete,
];

/// 全部韩文诊断码（顺序即值序）。
pub const ALL_KOR: [KorCode; KOR_CODES] = [
    KorCode::LeadBad,
    KorCode::VowelBad,
    KorCode::TailBad,
    KorCode::ComposeBad,
    KorCode::MergeBad,
    KorCode::ToggleBad,
    KorCode::PendingBad,
    KorCode::GoldenBad,
];

/// 自由函数：码值 → 说明（段外一律 `UNKNOWN`）。
pub fn detail(code: u32) -> &'static str {
    match code.wrapping_sub(SEG_BASE) {
        1 => EastCode::RowBad.detail(),
        2 => EastCode::TableBad.detail(),
        3 => EastCode::LayoutBad.detail(),
        4 => EastCode::SignatureBad.detail(),
        5 => EastCode::ShiftBad.detail(),
        6 => EastCode::CyrillicBad.detail(),
        7 => EastCode::CaseBad.detail(),
        8 => EastCode::DiacriticBad.detail(),
        9 => EastCode::GoldenBad.detail(),
        10 => EastCode::ExpectBad.detail(),
        11 => EastCode::CodepointBad.detail(),
        12 => EastCode::FieldBad.detail(),
        13 => EastCode::DigestBad.detail(),
        14 => EastCode::Incomplete.detail(),
        17 => KorCode::LeadBad.detail(),
        18 => KorCode::VowelBad.detail(),
        19 => KorCode::TailBad.detail(),
        20 => KorCode::ComposeBad.detail(),
        21 => KorCode::MergeBad.detail(),
        22 => KorCode::ToggleBad.detail(),
        23 => KorCode::PendingBad.detail(),
        24 => KorCode::GoldenBad.detail(),
        _ => UNKNOWN,
    }
}

/// 段守卫：主段 14 码与子窗 8 码都不得越出本段窗口。
pub fn segment_guard() -> bool {
    if ALL.len() != 14 {
        return false;
    }
    if ALL_KOR.len() != KOR_CODES {
        return false;
    }
    if let (Some(first), Some(last)) = (ALL.first(), ALL.get(13)) {
        if first.val() != SEG_BASE + 1 || last.val() != SEG_BASE + 14 {
            return false;
        }
    } else {
        return false;
    }
    if let Some(k) = ALL_KOR.last() {
        if k.val() >= SEG_BASE + SEG_SPAN {
            return false;
        }
    } else {
        return false;
    }
    if KOR_BASE < SEG_BASE + 15 {
        return false;
    }
    let mut i = 1usize;
    while i < ALL.len() {
        if let (Some(p), Some(c)) = (ALL.get(i - 1), ALL.get(i)) {
            if c.val() != p.val() + 1 {
                return false;
            }
        } else {
            return false;
        }
        i += 1;
    }
    let mut j = 1usize;
    while j < ALL_KOR.len() {
        if let (Some(p), Some(c)) = (ALL_KOR.get(j - 1), ALL_KOR.get(j)) {
            if c.val() != p.val() + 1 {
                return false;
            }
        } else {
            return false;
        }
        j += 1;
    }
    true
}

/// 码面自检：每条码都能被 `detail` 反解，且说明互不共用。
pub fn code_face_ok() -> bool {
    let mut i = 0usize;
    let mut seen: Vec<u32> = Vec::new();
    while i < ALL.len() {
        let c = match ALL.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if detail(c.val()) != c.detail() || message_of(c.val()).is_none() {
            return false;
        }
        if c.detail() == c.message() {
            return false;
        }
        if seen.contains(&c.val()) {
            return false;
        }
        seen.push(c.val());
        i += 1;
    }
    let mut j = 0usize;
    while j < ALL_KOR.len() {
        let k = match ALL_KOR.get(j) {
            Some(v) => *v,
            None => return false,
        };
        if detail(k.val()) != k.detail() {
            return false;
        }
        if k.detail() == k.message() {
            return false;
        }
        if seen.contains(&k.val()) {
            return false;
        }
        seen.push(k.val());
        j += 1;
    }
    detail(SEG_BASE) == UNKNOWN && detail(SEG_BASE + SEG_SPAN) == UNKNOWN
}

/// 面向运营的短句取用（越窗返回 `None`）。
pub fn message_of(code: u32) -> Option<&'static str> {
    let off = code.wrapping_sub(SEG_BASE);
    if (1..=14).contains(&off) {
        if let Some(c) = ALL.get(off as usize - 1) {
            return Some(c.message());
        }
        return None;
    }
    if (0x11..=0x18).contains(&off) {
        if let Some(k) = ALL_KOR.get(off as usize - 0x11) {
            return Some(k.message());
        }
    }
    None
}

// ── 层与布局标识 ─────────────────────────────────────────────────
/// 键位层：基层与换档层。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layer {
    /// 基层（未按换档）。
    Base,
    /// 换档层。
    Shift,
}

impl Layer {
    /// 层号。
    pub fn index(self) -> usize {
        match self {
            Layer::Base => 0,
            Layer::Shift => 1,
        }
    }

    /// 层号 → 层（越窗返回 `None`）。
    pub fn from_index(v: usize) -> Option<Layer> {
        match v {
            0 => Some(Layer::Base),
            1 => Some(Layer::Shift),
            _ => None,
        }
    }

    /// 层标签。
    pub fn tag(self) -> &'static str {
        match self {
            Layer::Base => "base",
            Layer::Shift => "shift",
        }
    }
}

/// 东欧四套布局标识。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EastId {
    /// 俄文 ЙЦУКЕН。
    Russian,
    /// 捷克 QWERTZ。
    Czech,
    /// 斯洛伐克 QWERTZ。
    Slovak,
    /// 匈牙利 QWERTZ。
    Hungarian,
}

impl EastId {
    /// 下标。
    pub fn index(self) -> usize {
        match self {
            EastId::Russian => 0,
            EastId::Czech => 1,
            EastId::Slovak => 2,
            EastId::Hungarian => 3,
        }
    }

    /// 下标 → 布局（越窗返回 `None`）。
    pub fn from_index(v: usize) -> Option<EastId> {
        match v {
            0 => Some(EastId::Russian),
            1 => Some(EastId::Czech),
            2 => Some(EastId::Slovak),
            3 => Some(EastId::Hungarian),
            _ => None,
        }
    }

    /// 标签。
    pub fn tag(self) -> &'static str {
        match self {
            EastId::Russian => "ytsuen",
            EastId::Czech => "qwertz-cs",
            EastId::Slovak => "qwertz-sk",
            EastId::Hungarian => "qwertz-hu",
        }
    }

    /// 标签 → 布局（未知返回 `None`）。
    pub fn from_tag(s: &str) -> Option<EastId> {
        let mut i = 0usize;
        while i < ALL_EAST.len() {
            if let Some(id) = ALL_EAST.get(i) {
                if id.tag() == s {
                    return EastId::from_index(*id as usize);
                }
            }
            i += 1;
        }
        None
    }

    /// 基层表（46 格）。
    pub fn base_table(self) -> &'static [char; CELLS] {
        match self {
            EastId::Russian => &RU_BASE,
            EastId::Czech => &CS_BASE,
            EastId::Slovak => &SK_BASE,
            EastId::Hungarian => &HU_BASE,
        }
    }

    /// 数字行换档表（13 格）。
    pub fn shift_row(self) -> &'static [char; SHIFT_COLS] {
        match self {
            EastId::Russian => &RU_SHIFT,
            EastId::Czech => &CS_SHIFT,
            EastId::Slovak => &SK_SHIFT,
            EastId::Hungarian => &HU_SHIFT,
        }
    }

    /// 该套布局登记的黄金条数。
    pub fn golden_count(self) -> usize {
        match self {
            EastId::Russian => GOLD_RU,
            EastId::Czech => GOLD_CS,
            EastId::Slovak => GOLD_SK,
            EastId::Hungarian => GOLD_HU,
        }
    }

    /// 正向查表：层 + 行 + 列 → 码位。
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
                    Some(upper(*self.base_table().get(start + col)?))
                }
            }
        }
    }

    /// 表校验和：逐格折叠基层，数字行额外折叠换档层。
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

    /// 折叠签名（8 字节）。
    pub fn signature(self) -> [u8; 8] {
        let sum = self.checksum();
        let mut out = [0u8; 8];
        let mut i = 0usize;
        while i < 8 {
            if let Some(s) = out.get_mut(i) {
                *s = ((sum >> (i * 8)) & 0xff) as u8;
            }
            i += 1;
        }
        out[7] = out[7].wrapping_add(self.index() as u8);
        out
    }

    /// 签名的十六进制串（16 字符）。
    pub fn sig_hex(self) -> String {
        let sig = self.signature();
        let mut out = String::with_capacity(16);
        let mut i = 0usize;
        while i < sig.len() {
            if let Some(b) = sig.get(i) {
                out.push_str(&format!("{:02x}", b));
            }
            i += 1;
        }
        out
    }
}

/// 全部东欧布局（顺序即下标序）。
pub const ALL_EAST: [EastId; EAST] = [
    EastId::Russian,
    EastId::Czech,
    EastId::Slovak,
    EastId::Hungarian,
];

/// 字母大写化（非字母原样返回）。
pub fn upper(c: char) -> char {
    match c.to_uppercase().next() {
        Some(v) => v,
        None => c,
    }
}

// ── 东欧键位表（逐槽手写，行长按 ROW_LEN 登记） ──────────────────
/// 俄文 ЙЦУКЕН 基层：ё 占数字行首格，三行字母按 ЙЦУКЕН 铺放。
pub const RU_BASE: [char; CELLS] = [
    'ё', '1', '2', '3', '4', '5', '6', '7', '8', '9', '0', '-', '=',
    'й', 'ц', 'у', 'к', 'е', 'н', 'г', 'ш', 'щ', 'з', 'х', 'ъ',
    'ф', 'ы', 'в', 'а', 'п', 'р', 'о', 'л', 'д', 'ж', 'э',
    'я', 'ч', 'с', 'м', 'и', 'т', 'ь', 'б', 'ю', '.',
];

/// 俄文数字行换档：` ! " № ; % : ? * ( ) _ +
pub const RU_SHIFT: [char; SHIFT_COLS] = [
    '`', '!', '"', '№', ';', '%', ':', '?', '*', '(', ')', '_', '+',
];

/// 捷克 QWERTZ 基层：ě 占数字行首格，变音槽落在上字母行末两格与中字行末两格。
pub const CS_BASE: [char; CELLS] = [
    'ě', '1', '2', '3', '4', '5', '6', '7', '8', '9', '0', '%', ';',
    'q', 'w', 'e', 'r', 't', 'z', 'u', 'i', 'o', 'p', 'ú', 'ä',
    'a', 's', 'd', 'f', 'g', 'h', 'j', 'k', 'l', 'ř', 'č',
    'y', 'x', 'c', 'v', 'b', 'n', 'm', 'š', 'ž', '&',
];

/// 捷克数字行换档。
pub const CS_SHIFT: [char; SHIFT_COLS] = [
    '+', '!', '@', '#', '$', '%', '^', '&', '*', '(', ')', '=', '\'',
];

/// 斯洛伐克 QWERTZ 基层：; 占数字行首格，y 归下字母行首，ň ô ť 落在下字母行。
pub const SK_BASE: [char; CELLS] = [
    ';', '1', '2', '3', '4', '5', '6', '7', '8', '9', '0', '%', '/',
    'q', 'w', 'e', 'r', 't', 'z', 'u', 'i', 'o', 'p', 'ú', 'ä',
    'a', 's', 'd', 'f', 'g', 'h', 'j', 'k', 'l', 'é', 'í',
    'y', 'x', 'c', 'v', 'b', 'n', 'm', 'ň', 'ô', 'ť',
];

/// 斯洛伐克数字行换档。
pub const SK_SHIFT: [char; SHIFT_COLS] = [
    '°', '!', '"', '#', '$', '%', '&', '/', '(', ')', '?', '_', ':',
];

/// 匈牙利 QWERTZ 基层：í 占数字行首格，ő ű 与 ö ó 分列上字母行与数字行末。
pub const HU_BASE: [char; CELLS] = [
    'í', '1', '2', '3', '4', '5', '6', '7', '8', '9', '0', 'ó', 'ö',
    'q', 'w', 'e', 'r', 't', 'z', 'u', 'i', 'o', 'p', 'ő', 'ü',
    'a', 's', 'd', 'f', 'g', 'h', 'j', 'k', 'l', 'á', 'ű',
    'y', 'x', 'c', 'v', 'b', 'n', 'm', ',', '.', '-',
];

/// 匈牙利数字行换档。
pub const HU_SHIFT: [char; SHIFT_COLS] = [
    '\'', '!', '"', '#', '$', '%', '&', '/', '(', ')', '=', '?', 'Ö',
];

/// 俄文 33 字母清点表（含 ё，逐字母必须在 RU 表内可反查）。
pub const CYRILLIC_LETTERS: [char; CYRILLIC] = [
    'а', 'б', 'в', 'г', 'д', 'е', 'ё', 'ж', 'з', 'и', 'й', 'к', 'л',
    'м', 'н', 'о', 'п', 'р', 'с', 'т', 'у', 'ф', 'х', 'ц', 'ч', 'ш',
    'щ', 'ъ', 'ы', 'ь', 'э', 'ю', 'я',
];

/// ё 的大写落点：数字行的换档层归符号行，故 Ё 不落在该槽，单独入账。
pub const CYRILLIC_YO_UPPER: char = 'Ё';

/// 捷克变音登记槽（行, 列, 码位）。
pub const CS_DIACRITICS: [(u8, u8, char); 6] = [
    (0, 0, 'ě'),
    (1, 10, 'ú'),
    (1, 11, 'ä'),
    (2, 9, 'ř'),
    (2, 10, 'č'),
    (3, 7, 'š'),
];

/// 斯洛伐克变音登记槽。
pub const SK_DIACRITICS: [(u8, u8, char); 7] = [
    (1, 10, 'ú'),
    (1, 11, 'ä'),
    (2, 9, 'é'),
    (2, 10, 'í'),
    (3, 7, 'ň'),
    (3, 8, 'ô'),
    (3, 9, 'ť'),
];

/// 匈牙利变音登记槽。
pub const HU_DIACRITICS: [(u8, u8, char); 7] = [
    (0, 0, 'í'),
    (0, 11, 'ó'),
    (0, 12, 'ö'),
    (1, 10, 'ő'),
    (1, 11, 'ü'),
    (2, 9, 'á'),
    (2, 10, 'ű'),
];

// ── 东欧黄金对（30 条，逐对手工核对） ────────────────────────────
/// 一条东欧黄金对：布局/层/行列 → 期望码位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EastGold {
    /// 序号 21..=50。
    pub id: u16,
    /// 布局。
    pub layout: EastId,
    /// 层。
    pub layer: Layer,
    /// 行。
    pub row: u8,
    /// 列。
    pub col: u8,
    /// 期望码位。
    pub expect: char,
    /// 该条考察的结构特征。
    pub note: &'static str,
}

/// 东欧黄金全表。
pub const EAST_GOLD: [EastGold; GOLD_EAST] = [
    EastGold { id: 21, layout: EastId::Russian, layer: Layer::Base, row: 1, col: 0, expect: 'й', note: "ЙЦУКЕН 首字母" },
    EastGold { id: 22, layout: EastId::Russian, layer: Layer::Base, row: 2, col: 0, expect: 'ф', note: "中字行首格" },
    EastGold { id: 23, layout: EastId::Russian, layer: Layer::Base, row: 2, col: 9, expect: 'ж', note: "变音槽位" },
    EastGold { id: 24, layout: EastId::Russian, layer: Layer::Base, row: 1, col: 11, expect: 'ъ', note: "硬音符号" },
    EastGold { id: 25, layout: EastId::Russian, layer: Layer::Shift, row: 2, col: 0, expect: 'Ф', note: "西里尔大写回读" },
    EastGold { id: 26, layout: EastId::Russian, layer: Layer::Shift, row: 1, col: 0, expect: 'Й', note: "短元音大写" },
    EastGold { id: 27, layout: EastId::Russian, layer: Layer::Shift, row: 0, col: 3, expect: '№', note: "序号符号换档" },
    EastGold { id: 28, layout: EastId::Russian, layer: Layer::Base, row: 0, col: 0, expect: 'ё', note: "点ё 占数字行首" },
    EastGold { id: 29, layout: EastId::Russian, layer: Layer::Shift, row: 0, col: 0, expect: '`', note: "换档反引号" },
    EastGold { id: 30, layout: EastId::Russian, layer: Layer::Base, row: 3, col: 0, expect: 'я', note: "下字母行首格" },
    EastGold { id: 31, layout: EastId::Russian, layer: Layer::Shift, row: 3, col: 0, expect: 'Я', note: "下字母行大写" },
    EastGold { id: 32, layout: EastId::Russian, layer: Layer::Base, row: 2, col: 10, expect: 'э', note: "反向 э 收尾" },
    EastGold { id: 33, layout: EastId::Czech, layer: Layer::Base, row: 1, col: 10, expect: 'ú', note: "捷克变音槽" },
    EastGold { id: 34, layout: EastId::Czech, layer: Layer::Shift, row: 1, col: 10, expect: 'Ú', note: "变音大写" },
    EastGold { id: 35, layout: EastId::Czech, layer: Layer::Base, row: 2, col: 9, expect: 'ř', note: "ř 落中字行" },
    EastGold { id: 36, layout: EastId::Czech, layer: Layer::Base, row: 2, col: 10, expect: 'č', note: "č 紧邻 ř" },
    EastGold { id: 37, layout: EastId::Czech, layer: Layer::Base, row: 0, col: 0, expect: 'ě', note: "ě 占数字行首" },
    EastGold { id: 38, layout: EastId::Czech, layer: Layer::Base, row: 3, col: 7, expect: 'š', note: "š 落下字母行" },
    EastGold { id: 39, layout: EastId::Slovak, layer: Layer::Base, row: 1, col: 10, expect: 'ú', note: "斯洛伐克 ú 与捷克同槽" },
    EastGold { id: 40, layout: EastId::Slovak, layer: Layer::Base, row: 2, col: 9, expect: 'é', note: "é 与捷克 ř 同槽" },
    EastGold { id: 41, layout: EastId::Slovak, layer: Layer::Base, row: 2, col: 10, expect: 'í', note: "í 收尾中字行" },
    EastGold { id: 42, layout: EastId::Slovak, layer: Layer::Base, row: 3, col: 7, expect: 'ň', note: "ň 落下字母行" },
    EastGold { id: 43, layout: EastId::Slovak, layer: Layer::Shift, row: 3, col: 7, expect: 'Ň', note: "变音大写回读" },
    EastGold { id: 44, layout: EastId::Slovak, layer: Layer::Base, row: 0, col: 0, expect: ';', note: "分号占数字行首" },
    EastGold { id: 45, layout: EastId::Hungarian, layer: Layer::Base, row: 1, col: 10, expect: 'ő', note: "双acute ő" },
    EastGold { id: 46, layout: EastId::Hungarian, layer: Layer::Shift, row: 1, col: 10, expect: 'Ő', note: "Ő 大写回读" },
    EastGold { id: 47, layout: EastId::Hungarian, layer: Layer::Base, row: 0, col: 12, expect: 'ö', note: "ö 占数字行末" },
    EastGold { id: 48, layout: EastId::Hungarian, layer: Layer::Shift, row: 0, col: 12, expect: 'Ö', note: "数字行末大写" },
    EastGold { id: 49, layout: EastId::Hungarian, layer: Layer::Base, row: 3, col: 0, expect: 'y', note: "匈牙利 y 移入下字母行" },
    EastGold { id: 50, layout: EastId::Hungarian, layer: Layer::Base, row: 2, col: 10, expect: 'ű', note: "ű 收尾中字行" },
];

// ── 东欧闸 ───────────────────────────────────────────────────────
/// 行几何：记长之和 = 格数，行首下标逐级累加。
pub fn east_row_len_ok() -> bool {
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
        if len > COLS as usize {
            return false;
        }
        acc += len;
        i += 1;
    }
    acc == CELLS && ROW_LEN.len() == ROWS && SHIFT_COLS == COLS as usize
}

/// 表自洽：四套表各 46 格、换档数字行 13 格、每行内码位互异。
/// 东欧四套表自洽：行记长合法，且整张卡面 46 格逐格互异、换档行齐整。
pub fn east_tables_ok() -> bool {
    if !east_row_len_ok() {
        return false;
    }
    let mut i = 0usize;
    while i < ALL_EAST.len() {
        let id = match ALL_EAST.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if id.base_table().len() != CELLS || id.shift_row().len() != SHIFT_COLS {
            return false;
        }
        let mut x = 0usize;
        while x < CELLS {
            let ca = match id.base_table().get(x) {
                Some(v) => *v,
                None => return false,
            };
            let mut y = x + 1;
            while y < CELLS {
                // 绝对槽号扫描到卡面末尾：跨行撞码会让反查歧义。
                match id.base_table().get(y) {
                    Some(cb) if *cb == ca => return false,
                    Some(_) => {}
                    None => return false,
                }
                y += 1;
            }
            x += 1;
        }
        let mut s = 0usize;
        while s < SHIFT_COLS {
            let sa = match id.shift_row().get(s) {
                Some(v) => *v,
                None => return false,
            };
            let mut t = s + 1;
            while t < SHIFT_COLS {
                if let Some(sb) = id.shift_row().get(t) {
                    if *sb == sa {
                        return false;
                    }
                } else {
                    return false;
                }
                t += 1;
            }
            s += 1;
        }
        i += 1;
    }
    true
}

/// 数字行换档逐格不同于基层（同码即方向语义丢失）。
pub fn east_shift_distinct_ok() -> bool {
    if !east_row_len_ok() {
        return false;
    }
    let mut i = 0usize;
    while i < ALL_EAST.len() {
        let id = match ALL_EAST.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let mut c = 0usize;
        while c < SHIFT_COLS {
            let b = match id.base_table().get(c) {
                Some(v) => *v,
                None => return false,
            };
            let s = match id.shift_row().get(c) {
                Some(v) => *v,
                None => return false,
            };
            if b == s {
                return false;
            }
            c += 1;
        }
        i += 1;
    }
    true
}

/// 在指定层内按码位找首槽（行优先）。
pub fn east_code_slot(id: EastId, layer: Layer, want: char) -> Option<(usize, usize)> {
    let mut r = 0usize;
    while r < ROWS {
        let len = *ROW_LEN.get(r)? as usize;
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

/// 表内任一格都能反查回可读槽位。
pub fn east_reverse_ok() -> bool {
    let mut i = 0usize;
    while i < ALL_EAST.len() {
        let id = match ALL_EAST.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let mut r = 0usize;
        while r < ROWS {
            let len = row_len_or_zero(r);
            let start = row_start_or_zero(r);
            let mut c = 0usize;
            while c < len {
                let b = match id.base_table().get(start + c) {
                    Some(v) => *v,
                    None => return false,
                };
                let (fr, fc) = match east_code_slot(id, Layer::Base, b) {
                    Some(v) => v,
                    None => return false,
                };
                if id.key(Layer::Base, fr, fc) != Some(b) {
                    return false;
                }
                if id.key(Layer::Shift, r, c).is_none() {
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

/// 四套布局签名与标签两两互异。
pub fn east_layouts_distinct() -> bool {
    let mut i = 0usize;
    while i < ALL_EAST.len() {
        let a = match ALL_EAST.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if EastId::from_index(a.index()) != Some(a) {
            return false;
        }
        if EastId::from_tag(a.tag()) != Some(a) {
            return false;
        }
        let mut j = i + 1;
        while j < ALL_EAST.len() {
            let b = match ALL_EAST.get(j) {
                Some(v) => *v,
                None => return false,
            };
            if a.signature() == b.signature() || a.tag() == b.tag() {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    EastId::from_index(EAST).is_none()
}

/// 西里尔 33 字母逐字母可反查，且大写层逐格回读。
pub fn cyrillic_inventory_ok() -> bool {
    let ru = EastId::Russian;
    if CYRILLIC_LETTERS.len() != CYRILLIC {
        return false;
    }
    let mut i = 0usize;
    let mut seen: Vec<char> = Vec::new();
    while i < CYRILLIC_LETTERS.len() {
        let c = match CYRILLIC_LETTERS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if seen.contains(&c) {
            return false;
        }
        seen.push(c);
        let (r, col) = match east_code_slot(ru, Layer::Base, c) {
            Some(v) => v,
            None => return false,
        };
        if ru.key(Layer::Base, r, col) != Some(c) {
            return false;
        }
        if r == 0 {
            // 数字行换档层整行归符号，ё 的大写只能按独立入账的 Ё 核对。
            if c != 'ё' || upper(c) != CYRILLIC_YO_UPPER {
                return false;
            }
        } else if c.is_alphabetic() && ru.key(Layer::Shift, r, col) != Some(upper(c)) {
            return false;
        }
        i += 1;
    }
    true
}

/// 字母格大小写逐格回读（非字母格原样）。
pub fn uppercase_pair_ok() -> bool {
    let mut i = 0usize;
    while i < ALL_EAST.len() {
        let id = match ALL_EAST.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let mut r = 1usize;
        while r < ROWS {
            let len = row_len_or_zero(r);
            let start = row_start_or_zero(r);
            let mut c = 0usize;
            while c < len {
                let b = match id.base_table().get(start + c) {
                    Some(v) => *v,
                    None => return false,
                };
                let s = match id.key(Layer::Shift, r, c) {
                    Some(v) => v,
                    None => return false,
                };
                if b.is_alphabetic() && s != upper(b) {
                    return false;
                }
                if !b.is_alphabetic() && s != b {
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

/// 变音槽登记：每套非俄语布局的变音字母逐格落在登记位置。
pub fn diacritic_slots_ok() -> bool {
    if CS_DIACRITICS.len() != 6 || SK_DIACRITICS.len() != 7 || HU_DIACRITICS.len() != 7 {
        return false;
    }
    let mut a = 0usize;
    while a < CS_DIACRITICS.len() {
        let (r, c, ch) = match CS_DIACRITICS.get(a) {
            Some(v) => *v,
            None => return false,
        };
        if EastId::Czech.key(Layer::Base, r as usize, c as usize) != Some(ch) {
            return false;
        }
        a += 1;
    }
    let mut b = 0usize;
    while b < SK_DIACRITICS.len() {
        let (r, c, ch) = match SK_DIACRITICS.get(b) {
            Some(v) => *v,
            None => return false,
        };
        if EastId::Slovak.key(Layer::Base, r as usize, c as usize) != Some(ch) {
            return false;
        }
        b += 1;
    }
    let mut d = 0usize;
    while d < HU_DIACRITICS.len() {
        let (r, c, ch) = match HU_DIACRITICS.get(d) {
            Some(v) => *v,
            None => return false,
        };
        if EastId::Hungarian.key(Layer::Base, r as usize, c as usize) != Some(ch) {
            return false;
        }
        d += 1;
    }
    true
}

/// 单条东欧黄金是否命中。
pub fn east_gold_check(g: &EastGold) -> bool {
    g.layout.key(g.layer, g.row as usize, g.col as usize) == Some(g.expect)
        && g.id >= 21
        && g.id <= GOLD_TOTAL as u16
        && !g.note.is_empty()
}

/// 某套布局的黄金命中数。
pub fn east_gold_hits(id: EastId) -> usize {
    let mut acc = 0usize;
    let mut i = 0usize;
    while i < EAST_GOLD.len() {
        if let Some(g) = EAST_GOLD.get(i) {
            if g.layout == id && east_gold_check(g) {
                acc += 1;
            }
        }
        i += 1;
    }
    acc
}

/// 东欧黄金总闸：条数、序号连续、逐套规模、逐条命中。
pub fn golden_east_ok() -> bool {
    if EAST_GOLD.len() != GOLD_EAST {
        return false;
    }
    let mut i = 0usize;
    while i < EAST_GOLD.len() {
        let g = match EAST_GOLD.get(i) {
            Some(v) => v,
            None => return false,
        };
        if g.id as usize != 21 + i {
            return false;
        }
        if !east_gold_check(g) {
            return false;
        }
        if g.layout.golden_count() == 0 {
            return false;
        }
        i += 1;
    }
    let mut k = 0usize;
    while k < ALL_EAST.len() {
        let id = match ALL_EAST.get(k) {
            Some(v) => *v,
            None => return false,
        };
        if east_gold_hits(id) != id.golden_count() {
            return false;
        }
        k += 1;
    }
    east_gold_total_hits() == GOLD_EAST
}

/// 全部东欧黄金命中数（含未命中项计数）。
pub fn east_gold_total_hits() -> usize {
    let mut acc = 0usize;
    let mut i = 0usize;
    while i < EAST_GOLD.len() {
        if let Some(g) = EAST_GOLD.get(i) {
            if east_gold_check(g) {
                acc += 1;
            }
        }
        i += 1;
    }
    acc
}

// ── 韩文两态键位 ─────────────────────────────────────────────────
/// 韩文输入态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    /// 라틴（英文）态。
    Latin,
    /// 한글（韩文）态。
    Hangul,
}

impl State {
    /// 态号。
    pub fn index(self) -> usize {
        match self {
            State::Latin => 0,
            State::Hangul => 1,
        }
    }

    /// 态号 → 态。
    pub fn from_index(v: usize) -> Option<State> {
        match v {
            0 => Some(State::Latin),
            1 => Some(State::Hangul),
            _ => None,
        }
    }

    /// 态标签。
    pub fn tag(self) -> &'static str {
        match self {
            State::Latin => "latin",
            State::Hangul => "hangul",
        }
    }

    /// 取反态（한/영 键语义）。
    pub fn flipped(self) -> State {
        match self {
            State::Latin => State::Hangul,
            State::Hangul => State::Latin,
        }
    }
}

/// 全部韩文态。
pub const ALL_STATES: [State; KOR_STATES] = [State::Latin, State::Hangul];

/// 라틴态基层：US 字母排布。
pub const KR_LATIN_BASE: [char; CELLS] = [
    '`', '1', '2', '3', '4', '5', '6', '7', '8', '9', '0', '-', '=',
    'q', 'w', 'e', 'r', 't', 'y', 'u', 'i', 'o', 'p', '[', ']',
    'a', 's', 'd', 'f', 'g', 'h', 'j', 'k', 'l', ';', '\'',
    'z', 'x', 'c', 'v', 'b', 'n', 'm', ',', '.', '/',
];

/// 라틴态换档：数字行符号 + 字母大写。
pub const KR_LATIN_SHIFT: [char; CELLS] = [
    '~', '!', '@', '#', '$', '%', '^', '&', '*', '(', ')', '_', '+',
    'Q', 'W', 'E', 'R', 'T', 'Y', 'U', 'I', 'O', 'P', '{', '}',
    'A', 'S', 'D', 'F', 'G', 'H', 'J', 'K', 'L', ':', '"',
    'Z', 'X', 'C', 'V', 'B', 'N', 'M', '<', '>', '?',
];

/// 한글态基层：数字行跨态同码，三行字母按 26 母字铺放，末三格落标点。
pub const KR_HANGUL_BASE: [char; CELLS] = [
    '`', '1', '2', '3', '4', '5', '6', '7', '8', '9', '0', '-', '=',
    'ㅂ', 'ㅈ', 'ㄷ', 'ㄱ', 'ㅅ', 'ㅛ', 'ㅕ', 'ㅑ', 'ㅐ', 'ㅔ', 'ㅋ', 'ㅌ',
    'ㅁ', 'ㄴ', 'ㅇ', 'ㄹ', 'ㅎ', 'ㅗ', 'ㅓ', 'ㅏ', 'ㅣ', 'ㅊ', 'ㅍ',
    'ㅠ', 'ㅜ', 'ㅡ', ',', '.', '/', '₩', '?', ';', ':',
];

/// 한글态换档：仅首字母行变形（紧声 ㅃㅉㄸㄲㅆ 与 ㅒㅖ），中/下两行逐格等于基层。
pub const KR_HANGUL_SHIFT: [char; CELLS] = [
    '~', '!', '@', '#', '$', '%', '^', '&', '*', '(', ')', '_', '+',
    'ㅃ', 'ㅉ', 'ㄸ', 'ㄲ', 'ㅆ', 'ㅛ', 'ㅕ', 'ㅑ', 'ㅒ', 'ㅖ', 'ㅋ', 'ㅌ',
    'ㅁ', 'ㄴ', 'ㅇ', 'ㄹ', 'ㅎ', 'ㅗ', 'ㅓ', 'ㅏ', 'ㅣ', 'ㅊ', 'ㅍ',
    'ㅠ', 'ㅜ', 'ㅡ', ',', '.', '/', '₩', '?', ';', ':',
];

/// 按态与层取键位。
pub fn kor_key(state: State, layer: Layer, row: usize, col: usize) -> Option<char> {
    if row >= ROWS {
        return None;
    }
    let len = *ROW_LEN.get(row)? as usize;
    if col >= len {
        return None;
    }
    let start = *ROW_START.get(row)?;
    let table = match (state, layer) {
        (State::Latin, Layer::Base) => &KR_LATIN_BASE,
        (State::Latin, Layer::Shift) => &KR_LATIN_SHIFT,
        (State::Hangul, Layer::Base) => &KR_HANGUL_BASE,
        (State::Hangul, Layer::Shift) => &KR_HANGUL_SHIFT,
    };
    table.get(start + col).copied()
}

/// 按态取整表。
pub fn kor_table(state: State, layer: Layer) -> &'static [char; CELLS] {
    match (state, layer) {
        (State::Latin, Layer::Base) => &KR_LATIN_BASE,
        (State::Latin, Layer::Shift) => &KR_LATIN_SHIFT,
        (State::Hangul, Layer::Base) => &KR_HANGUL_BASE,
        (State::Hangul, Layer::Shift) => &KR_HANGUL_SHIFT,
    }
}

// ── 初声/中声/终声索引表 ──────────────────────────────────────────
/// 初声（领音）19 母，顺序即音节公式索引序。
pub const LEAD_JAMO: [char; LEADS] = [
    'ㄱ', 'ㄲ', 'ㄴ', 'ㄷ', 'ㄸ', 'ㄹ', 'ㅁ', 'ㅂ', 'ㅃ', 'ㅅ', 'ㅆ', 'ㅇ',
    'ㅈ', 'ㅉ', 'ㅊ', 'ㅋ', 'ㅌ', 'ㅍ', 'ㅎ',
];

/// 中声（元音）21 母，顺序即音节公式索引序。
pub const VOWEL_JAMO: [char; VOWELS] = [
    'ㅏ', 'ㅐ', 'ㅑ', 'ㅒ', 'ㅓ', 'ㅔ', 'ㅕ', 'ㅖ', 'ㅗ', 'ㅘ', 'ㅙ', 'ㅚ',
    'ㅛ', 'ㅜ', 'ㅝ', 'ㅞ', 'ㅟ', 'ㅠ', 'ㅡ', 'ㅢ', 'ㅣ',
];

/// 终声（收音）27 母；索引 0 号留给「无收音」。
pub const TAIL_JAMO: [char; TAILS] = [
    'ㄱ', 'ㄲ', 'ㄳ', 'ㄴ', 'ㄵ', 'ㄶ', 'ㄷ', 'ㄹ', 'ㄺ', 'ㄻ', 'ㄼ', 'ㄽ',
    'ㄾ', 'ㄿ', 'ㅀ', 'ㅁ', 'ㅂ', 'ㅄ', 'ㅅ', 'ㅆ', 'ㅇ', 'ㅈ', 'ㅊ', 'ㅋ',
    'ㅌ', 'ㅍ', 'ㅎ',
];

/// 初声叠字合成对（前键, 后键, 合成结果）。
pub const LEAD_MERGE_PAIRS: [(char, char, char); LEAD_MERGE] = [
    ('ㄱ', 'ㄱ', 'ㄲ'),
    ('ㅈ', 'ㅈ', 'ㅉ'),
    ('ㄷ', 'ㄷ', 'ㄸ'),
    ('ㅂ', 'ㅂ', 'ㅃ'),
    ('ㅅ', 'ㅅ', 'ㅆ'),
];

/// 复元音合成对。
pub const VOWEL_MERGE_PAIRS: [(char, char, char); VOWEL_MERGE] = [
    ('ㅗ', 'ㅏ', 'ㅘ'),
    ('ㅗ', 'ㅐ', 'ㅙ'),
    ('ㅗ', 'ㅣ', 'ㅚ'),
    ('ㅜ', 'ㅓ', 'ㅝ'),
    ('ㅜ', 'ㅔ', 'ㅞ'),
    ('ㅜ', 'ㅣ', 'ㅟ'),
    ('ㅡ', 'ㅣ', 'ㅢ'),
];

/// 双收音合成对。
pub const TAIL_MERGE_PAIRS: [(char, char, char); TAIL_MERGE] = [
    ('ㄱ', 'ㅅ', 'ㄳ'),
    ('ㄴ', 'ㅈ', 'ㄵ'),
    ('ㄴ', 'ㅎ', 'ㄶ'),
    ('ㄹ', 'ㄱ', 'ㄺ'),
    ('ㄹ', 'ㅁ', 'ㄻ'),
    ('ㄹ', 'ㅂ', 'ㄼ'),
    ('ㄹ', 'ㅅ', 'ㄽ'),
    ('ㄹ', 'ㅌ', 'ㄾ'),
    ('ㄹ', 'ㅍ', 'ㄿ'),
    ('ㄹ', 'ㅎ', 'ㅀ'),
    ('ㅂ', 'ㅅ', 'ㅄ'),
];

/// 可直接键入的初声（全部 19 枚均有物理键）。
pub const ATOMIC_LEADS: [char; LEADS] = LEAD_JAMO;

/// 可直接键入的中声（复元音 7 枚只能合成）。
pub const ATOMIC_VOWELS: [char; 14] = [
    'ㅏ', 'ㅐ', 'ㅑ', 'ㅒ', 'ㅓ', 'ㅔ', 'ㅕ', 'ㅖ', 'ㅗ', 'ㅛ', 'ㅜ', 'ㅠ',
    'ㅡ', 'ㅣ',
];

/// 可直接键入的终声（双收音 11 枚只能合成）。
pub const ATOMIC_TAILS: [char; 16] = [
    'ㄱ', 'ㄲ', 'ㄴ', 'ㄷ', 'ㄹ', 'ㅁ', 'ㅂ', 'ㅅ', 'ㅆ', 'ㅇ', 'ㅈ', 'ㅊ',
    'ㅋ', 'ㅌ', 'ㅍ', 'ㅎ',
];

/// 复元音（只能由合成得到）。
pub const COMPOSITE_VOWELS: [char; VOWEL_MERGE] = [
    'ㅘ', 'ㅙ', 'ㅚ', 'ㅝ', 'ㅞ', 'ㅟ', 'ㅢ',
];

/// 双收音（只能由合成得到）。
pub const COMPOSITE_TAILS: [char; TAIL_MERGE] = [
    'ㄳ', 'ㄵ', 'ㄶ', 'ㄺ', 'ㄻ', 'ㄼ', 'ㄽ', 'ㄾ', 'ㄿ', 'ㅀ', 'ㅄ',
];

// ── 组合索引与恒等式 ─────────────────────────────────────────────
/// 初声码位 → 索引。
pub fn lead_index(c: char) -> Option<usize> {
    let mut i = 0usize;
    while i < LEAD_JAMO.len() {
        if let Some(v) = LEAD_JAMO.get(i) {
            if *v == c {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// 中声码位 → 索引。
pub fn vowel_index(c: char) -> Option<usize> {
    let mut i = 0usize;
    while i < VOWEL_JAMO.len() {
        if let Some(v) = VOWEL_JAMO.get(i) {
            if *v == c {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// 终声码位 → 索引（1..=27；无收音为 0）。
pub fn tail_index(c: char) -> Option<usize> {
    let mut i = 0usize;
    while i < TAIL_JAMO.len() {
        if let Some(v) = TAIL_JAMO.get(i) {
            if *v == c {
                return Some(i + 1);
            }
        }
        i += 1;
    }
    None
}

/// 索引 → 码位（三表合一取字）。
pub fn jamo_at(kind: JamoKind, idx: usize) -> Option<char> {
    match kind {
        JamoKind::Lead => Some(*LEAD_JAMO.get(idx)?),
        JamoKind::Vowel => Some(*VOWEL_JAMO.get(idx)?),
        JamoKind::Tail => {
            if idx == 0 {
                return None;
            }
            Some(*TAIL_JAMO.get(idx - 1)?)
        }
    }
}

/// 母字类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JamoKind {
    /// 初声。
    Lead,
    /// 中声。
    Vowel,
    /// 终声。
    Tail,
}

/// 组合：索引三元 → 音节码。
pub fn compose_syllable(l: usize, v: usize, t: usize) -> Option<char> {
    if l >= LEADS || v >= VOWELS || t >= TAIL_SLOTS {
        return None;
    }
    let code = SYL_START + ((l * VOWELS + v) * TAIL_SLOTS + t) as u32;
    if code > SYL_LAST {
        return None;
    }
    char::from_u32(code)
}

/// 分解：音节码 → 索引三元。
pub fn decompose_syllable(c: char) -> Option<(usize, usize, usize)> {
    let code = c as u32;
    if code < SYL_START || code > SYL_LAST {
        return None;
    }
    let off = code - SYL_START;
    let t = off % TAIL_SLOTS as u32;
    let rest = off / TAIL_SLOTS as u32;
    let v = rest % VOWELS as u32;
    let l = rest / VOWELS as u32;
    if l >= LEADS as u32 {
        return None;
    }
    Some((l as usize, v as usize, t as usize))
}

/// 叠字/合成查对：返回合成结果码位。
pub fn merge_pair(pairs: &[(char, char, char)], a: char, b: char) -> Option<char> {
    let mut i = 0usize;
    while i < pairs.len() {
        let (x, y, z) = match pairs.get(i) {
            Some(v) => *v,
            None => return None,
        };
        if x == a && y == b {
            return Some(z);
        }
        i += 1;
    }
    None
}

/// 初声叠字合成。
pub fn merge_lead(a: char, b: char) -> Option<char> {
    merge_pair(&LEAD_MERGE_PAIRS, a, b)
}

/// 复元音合成。
pub fn merge_vowel(a: char, b: char) -> Option<char> {
    merge_pair(&VOWEL_MERGE_PAIRS, a, b)
}

/// 双收音合成。
pub fn merge_tail(a: char, b: char) -> Option<char> {
    merge_pair(&TAIL_MERGE_PAIRS, a, b)
}

/// 判定一个码位属于哪一类母字（初声优先）。
pub fn jamo_class(c: char) -> Option<JamoKind> {
    if lead_index(c).is_some() {
        return Some(JamoKind::Lead);
    }
    if vowel_index(c).is_some() {
        return Some(JamoKind::Vowel);
    }
    if tail_index(c).is_some() {
        return Some(JamoKind::Tail);
    }
    None
}

// ── 两态组合引擎 ─────────────────────────────────────────────────
/// 组合中的当前音节。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SyllableBuilder {
    /// 初声码位。
    pub lead: Option<char>,
    /// 中声码位。
    pub vowel: Option<char>,
    /// 终声码位。
    pub tail: Option<char>,
}

impl SyllableBuilder {
    /// 空构造。
    pub fn new() -> Self {
        SyllableBuilder { lead: None, vowel: None, tail: None }
    }

    /// 是否空槽。
    pub fn is_empty(&self) -> bool {
        self.lead.is_none() && self.vowel.is_none() && self.tail.is_none()
    }

    /// 结算当前音节：完整三元组走公式，残缺组按母字原样落账。
    pub fn finish(&mut self, out: &mut String) {
        match (self.lead, self.vowel, self.tail) {
            (Some(l), Some(v), _) => {
                let li = match lead_index(l) {
                    Some(x) => x,
                    None => {
                        out.push(l);
                        self.reset();
                        return;
                    }
                };
                let vi = match vowel_index(v) {
                    Some(x) => x,
                    None => {
                        out.push(l);
                        out.push(v);
                        self.reset();
                        return;
                    }
                };
                let ti = match self.tail {
                    Some(t) => match tail_index(t) {
                        // tail_index 给的已经是收音槽号（槽 0 表示无收音）。
                        Some(x) => x,
                        None => {
                            // 查不到索引的终声按母字原样落账，不并入音节。
                            out.push(l);
                            out.push(v);
                            out.push(t);
                            self.reset();
                            return;
                        }
                    },
                    None => 0,
                };
                if let Some(c) = compose_syllable(li, vi, ti) {
                    out.push(c);
                } else {
                    out.push(l);
                    out.push(v);
                }
            }
            (Some(l), None, t) => {
                out.push(l);
                if let Some(x) = t {
                    out.push(x);
                }
            }
            (None, Some(v), _) => {
                out.push(v);
            }
            (None, None, _) => {}
        }
        self.reset();
    }

    /// 清空槽位。
    pub fn reset(&mut self) {
        self.lead = None;
        self.vowel = None;
        self.tail = None;
    }
}

impl Default for SyllableBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// 逐键组合：输入一串母字，输出组合后的音节串。
pub fn compose_stream(seq: &str) -> String {
    let mut out = String::new();
    let mut b = SyllableBuilder::new();
    let mut i = 0usize;
    let chars: Vec<char> = seq.chars().collect();
    while i < chars.len() {
        let c = match chars.get(i) {
            Some(v) => *v,
            None => break,
        };
        if jamo_class(c).is_none() {
            b.finish(&mut out);
            out.push(c);
            i += 1;
            continue;
        }
        push_jamo(&mut b, c, &mut out);
        i += 1;
    }
    b.finish(&mut out);
    out
}

/// 把一枚母字压进构造器（含必要的结算）。
pub fn push_jamo(b: &mut SyllableBuilder, c: char, out: &mut String) {
    if vowel_index(c).is_some() {
        match (b.lead, b.vowel, b.tail) {
            (None, None, None) => {
                b.lead = Some('ㅇ');
                b.vowel = Some(c);
            }
            (Some(_), None, None) => {
                b.vowel = Some(c);
            }
            (Some(l), Some(v), None) => match merge_vowel(v, c) {
                Some(m) => b.vowel = Some(m),
                None => {
                    b.lead = Some(l);
                    b.vowel = Some(v);
                    b.finish(out);
                    b.lead = Some('ㅇ');
                    b.vowel = Some(c);
                }
            },
            _ => {
                b.finish(out);
                b.lead = Some('ㅇ');
                b.vowel = Some(c);
            }
        }
        return;
    }
    // 辅音：可作初声或终声。
    match (b.lead, b.vowel, b.tail) {
        (None, None, None) => {
            b.lead = Some(c);
        }
        (Some(l), None, None) => match merge_lead(l, c) {
            Some(m) => b.lead = Some(m),
            None => {
                b.lead = Some(l);
                b.finish(out);
                b.lead = Some(c);
            }
        },
        (Some(l), Some(v), None) => {
            if tail_index(c).is_some() {
                b.lead = Some(l);
                b.vowel = Some(v);
                b.tail = Some(c);
            } else {
                b.finish(out);
                b.lead = Some(c);
            }
        }
        (Some(_), _, Some(t)) => match merge_tail(t, c) {
            Some(m) => b.tail = Some(m),
            None => {
                b.finish(out);
                b.lead = Some(c);
            }
        },
        (None, _, _) => {
            b.lead = Some(c);
        }
    }
}

/// 한/영 切换与在途队列。
#[derive(Clone)]
pub struct Switch {
    /// 当前态。
    pub mode: State,
    /// 翻转次数。
    pub flips: u32,
    /// 入队成功计数。
    pub queued: u32,
    /// 越窗丢弃计数。
    pub drops: u32,
    /// 已排空（切换时结算）计数。
    pub flushes: u32,
    queue: Vec<char>,
    last: Option<char>,
}

impl Switch {
    /// 英文态起步。
    pub fn new() -> Self {
        Switch {
            mode: State::Latin,
            flips: 0,
            queued: 0,
            drops: 0,
            flushes: 0,
            queue: Vec::new(),
            last: None,
        }
    }

    /// 指定态起步。
    pub fn with_mode(mode: State) -> Self {
        let mut s = Switch::new();
        s.mode = mode;
        s
    }

    /// 当前态。
    pub fn mode(&self) -> State {
        self.mode
    }

    /// 在途键数。
    pub fn pending(&self) -> usize {
        self.queue.len()
    }

    /// 最后一枚入队键。
    pub fn last_key(&self) -> Option<char> {
        self.last
    }

    /// 한/영 键：先结算在途，再翻态。
    pub fn toggle(&mut self) -> State {
        self.flushes += 1;
        self.queue.clear();
        self.mode = self.mode.flipped();
        self.flips += 1;
        self.mode
    }

    /// 压入一枚键；越出容量即记为丢弃并返回 `false`。
    pub fn press(&mut self, c: char) -> bool {
        if self.queue.len() >= PENDING_CAP {
            self.drops += 1;
            return false;
        }
        self.queue.push(c);
        self.queued += 1;
        self.last = Some(c);
        true
    }

    /// 排空在途队列（返回压入顺序）。
    pub fn drain(&mut self) -> Vec<char> {
        self.flushes += 1;
        let out = self.queue.clone();
        self.queue.clear();
        out
    }

    /// 按当前态解析物理槽位（行列 + 层）得到码位。
    pub fn resolve(&self, layer: Layer, row: usize, col: usize) -> Option<char> {
        kor_key(self.mode, layer, row, col)
    }
}

impl Default for Switch {
    fn default() -> Self {
        Self::new()
    }
}

// ── 韩文黄金组合（20 组） ─────────────────────────────────────────
/// 一条韩文黄金：母字序列 → 期望音节串。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KorGold {
    /// 序号 1..=20。
    pub id: u16,
    /// 输入母字序列。
    pub seq: &'static str,
    /// 期望输出。
    pub expect: &'static str,
    /// 该条考察的结构特征。
    pub note: &'static str,
}

/// 韩文黄金全表。
pub const KOR_GOLD: [KorGold; GOLD_KOR] = [
    KorGold { id: 1, seq: "ㄱㅏ", expect: "가", note: "初+中，无收音" },
    KorGold { id: 2, seq: "ㄴㅏ", expect: "나", note: "次初声索引" },
    KorGold { id: 3, seq: "ㅎㅏㄴ", expect: "한", note: "全收音 ㄴ，卡面同名音节" },
    KorGold { id: 4, seq: "ㅇㅓ", expect: "어", note: "零声母 ㅇ 起头" },
    KorGold { id: 5, seq: "ㅈㅓㅁ", expect: "점", note: "收音 ㅁ 占十六号槽" },
    KorGold { id: 6, seq: "ㅅㅣ", expect: "시", note: "末位中声 ㅣ" },
    KorGold { id: 7, seq: "ㄱㅓ", expect: "거", note: "初声索引 0 配 ㅓ" },
    KorGold { id: 8, seq: "ㅂㅗㅏ", expect: "봐", note: "复元音合成 ㅗ+ㅏ" },
    KorGold { id: 9, seq: "ㄱㄱㅏ", expect: "까", note: "初声叠字 ㄱ+ㄱ→ㄲ" },
    KorGold { id: 10, seq: "ㅇㅏㄹㅁ", expect: "앎", note: "双收音合成 ㄹ+ㅁ→ㄻ，占十号槽" },
    KorGold { id: 11, seq: "ㅎㅏㄴㄱㅜㄱ", expect: "한국", note: "跨音节续打" },
    KorGold { id: 12, seq: "ㅌㅏ", expect: "타", note: "初声 ㅌ 索引 16" },
    KorGold { id: 13, seq: "ㅍㅏ", expect: "파", note: "初声 ㅍ 索引 17" },
    KorGold { id: 14, seq: "ㅊㅣㅂ", expect: "칩", note: "ㅂ 先占收音槽而非另起初声" },
    KorGold { id: 15, seq: "ㄹㅏㅁ", expect: "람", note: "ㄹ 初声配 ㅁ 收音" },
    KorGold { id: 16, seq: "ㅇㅟ", expect: "위", note: "单键复元音 ㅟ" },
    KorGold { id: 17, seq: "ㅇㅜㅣ", expect: "위", note: "合成路径与单键同值" },
    KorGold { id: 18, seq: "ㅇㅓㅂㅅ", expect: "없", note: "双收音 ㅂ+ㅅ→ㅄ 占十八号槽" },
    KorGold { id: 19, seq: "ㄷㅚ", expect: "되", note: "复元音 ㅚ 索引 11" },
    KorGold { id: 20, seq: "ㄷㅚㄴ", expect: "된", note: "在 ㅚ 上叠收音" },
];

// ── 韩文闸：两态表自洽 ───────────────────────────────────────────
/// 行记长（越窗记为 0，供闸内循环使用，不产生 panic）。
pub fn row_len_or_zero(row: usize) -> usize {
    match ROW_LEN.get(row) {
        Some(v) => *v as usize,
        None => 0,
    }
}

/// 行首下标（越窗记为 0，同上）。
pub fn row_start_or_zero(row: usize) -> usize {
    match ROW_START.get(row) {
        Some(v) => *v,
        None => 0,
    }
}

/// 取一张 46 格表的某一行（按登记行记长切出）。
pub fn table_row(t: &[char; CELLS], row: usize) -> Option<Vec<char>> {
    let len = *ROW_LEN.get(row)? as usize;
    let start = *ROW_START.get(row)?;
    let mut out = Vec::with_capacity(len);
    let mut i = 0usize;
    while i < len {
        let c = match t.get(start + i) {
            Some(v) => *v,
            None => return None,
        };
        out.push(c);
        i += 1;
    }
    Some(out)
}

/// 序列内无重复码位。
pub fn row_unique(chars: &[char]) -> bool {
    let mut i = 0usize;
    while i < chars.len() {
        let a = match chars.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let mut j = i + 1;
        while j < chars.len() {
            if let Some(b) = chars.get(j) {
                if *b == a {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 码位是否落在本模块登记的韩文母字窗口（U+3131..=U+3163）。
pub fn jamo_window_ok(c: char) -> bool {
    let v = c as u32;
    v >= 0x3131 && v <= 0x3163
}

/// 码位是否为非字母（标点/数字/符号）。
pub fn non_alpha(c: char) -> bool {
    !c.is_alphabetic()
}

/// 韩文四张表自洽：逐行记长、行内互异、母字与标点各归其位。
pub fn kor_tables_ok() -> bool {
    if !east_row_len_ok() {
        return false;
    }
    let faces = [
        State::Latin,
        State::Hangul,
    ];
    let layers = [Layer::Base, Layer::Shift];
    let mut si = 0usize;
    while si < faces.len() {
        let st = match faces.get(si) {
            Some(v) => *v,
            None => return false,
        };
        let mut li = 0usize;
        while li < layers.len() {
            let ly = match layers.get(li) {
                Some(v) => *v,
                None => return false,
            };
            let t = kor_table(st, ly);
            if t.len() != CELLS {
                return false;
            }
            let mut r = 0usize;
            while r < ROWS {
                let chars = match table_row(t, r) {
                    Some(v) => v,
                    None => return false,
                };
                if chars.len() != row_len_or_zero(r) {
                    return false;
                }
                if !row_unique(&chars) {
                    return false;
                }
                if st == State::Hangul && r > 0 {
                    // 한글态的字母行只允许母字或非字母标点，且至少三枚母字。
                    let mut jamo_seen = 0usize;
                    let mut k = 0usize;
                    while k < chars.len() {
                        let c = match chars.get(k) {
                            Some(v) => *v,
                            None => return false,
                        };
                        if jamo_class(c).is_some() {
                            jamo_seen += 1;
                        } else if !non_alpha(c) {
                            return false;
                        }
                        k += 1;
                    }
                    if jamo_seen < 3 {
                        return false;
                    }
                }
                if st == State::Latin && !chars.iter().all(|c| (*c as u32) < 128) {
                    return false;
                }
                r += 1;
            }
            li += 1;
        }
        si += 1;
    }
    true
}

/// 数字行跨态同码：两态的基层与换档层数字行逐格相等，且不含母字。
pub fn kor_number_row_shared_ok() -> bool {
    let mut i = 0usize;
    while i < SHIFT_COLS {
        let lb = match kor_key(State::Latin, Layer::Base, 0, i) {
            Some(v) => v,
            None => return false,
        };
        let hb = match kor_key(State::Hangul, Layer::Base, 0, i) {
            Some(v) => v,
            None => return false,
        };
        let ls = match kor_key(State::Latin, Layer::Shift, 0, i) {
            Some(v) => v,
            None => return false,
        };
        let hs = match kor_key(State::Hangul, Layer::Shift, 0, i) {
            Some(v) => v,
            None => return false,
        };
        if lb != hb || ls != hs {
            return false;
        }
        if lb == ls || jamo_window_ok(lb) || jamo_window_ok(ls) {
            return false;
        }
        if ls.is_alphabetic() {
            // 数字行换档不得产出字母（符号行语义）。
            return false;
        }
        i += 1;
    }
    let digits: Vec<char> = match table_row(kor_table(State::Hangul, Layer::Base), 0) {
        Some(v) => v,
        None => return false,
    };
    digits.len() == SHIFT_COLS && row_unique(&digits)
}

/// 换档不变式：한글态中/下两行换档与基层逐格相等，首字母行只在七个紧声槽变形。
pub fn kor_shift_invariant_ok() -> bool {
    // 中、下两行逐格相等——韩文无大小写，换档不能造出第三套键位。
    let mut r = 2usize;
    while r < ROWS {
        let b = match table_row(kor_table(State::Hangul, Layer::Base), r) {
            Some(v) => v,
            None => return false,
        };
        let s = match table_row(kor_table(State::Hangul, Layer::Shift), r) {
            Some(v) => v,
            None => return false,
        };
        if b.len() != s.len() {
            return false;
        }
        let mut i = 0usize;
        while i < b.len() {
            match (b.get(i), s.get(i)) {
                (Some(x), Some(y)) if *x == *y => {}
                _ => return false,
            }
            i += 1;
        }
        r += 1;
    }
    // 首字母行：变形槽必须恰为 0..=4（紧声）与 8..=9（ㅒㅖ），其余逐格相等。
    let base = match table_row(kor_table(State::Hangul, Layer::Base), 1) {
        Some(v) => v,
        None => return false,
    };
    let shift = match table_row(kor_table(State::Hangul, Layer::Shift), 1) {
        Some(v) => v,
        None => return false,
    };
    if base.len() != 12 || shift.len() != 12 {
        return false;
    }
    let mut diff = 0usize;
    let mut i = 0usize;
    while i < base.len() {
        let b = match base.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let s = match shift.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let should_diff = i <= 4 || i == 8 || i == 9;
        if should_diff {
            if b == s {
                return false;
            }
            diff += 1;
            // 前四组变形必须能被初声叠字式回推（ㅂ+ㅂ→ㅃ 等）。
            if i <= 4 && merge_lead(b, b) != Some(s) {
                return false;
            }
            if (i == 8 || i == 9) && !vowel_index(s).is_some() {
                return false;
            }
        } else if b != s {
            return false;
        }
        i += 1;
    }
    diff == 7
}

/// 라틴态换档语义：字母格等于大写，非字母格不得同码。
pub fn latin_shift_case_ok() -> bool {
    let mut r = 0usize;
    while r < ROWS {
        let len = row_len_or_zero(r);
        let mut c = 0usize;
        while c < len {
            let b = match kor_key(State::Latin, Layer::Base, r, c) {
                Some(v) => v,
                None => return false,
            };
            let s = match kor_key(State::Latin, Layer::Shift, r, c) {
                Some(v) => v,
                None => return false,
            };
            if s == b {
                return false;
            }
            if b.is_alphabetic() {
                if s != upper(b) || !s.is_alphabetic() {
                    return false;
                }
            } else if s.is_alphabetic() {
                return false;
            }
            c += 1;
        }
        r += 1;
    }
    true
}

/// 正向槽位取字后再反查，验证某面表内码位唯一可寻。
pub fn kor_code_slot(state: State, layer: Layer, want: char) -> Option<(usize, usize)> {
    let mut r = 0usize;
    while r < ROWS {
        let len = row_len_or_zero(r);
        let mut c = 0usize;
        while c < len {
            if kor_key(state, layer, r, c) == Some(want) {
                return Some((r, c));
            }
            c += 1;
        }
        r += 1;
    }
    None
}

/// 某面内 `want` 出现的槽位数。
pub fn kor_slot_hits(state: State, layer: Layer, want: char) -> usize {
    let mut acc = 0usize;
    let mut r = 0usize;
    while r < ROWS {
        let len = row_len_or_zero(r);
        let mut c = 0usize;
        while c < len {
            if kor_key(state, layer, r, c) == Some(want) {
                acc += 1;
            }
            c += 1;
        }
        r += 1;
    }
    acc
}

/// 反查闸：四面表内每一格都能反查回唯一槽位。
pub fn kor_reverse_ok() -> bool {
    let combos = [
        (State::Latin, Layer::Base),
        (State::Latin, Layer::Shift),
        (State::Hangul, Layer::Base),
        (State::Hangul, Layer::Shift),
    ];
    let mut i = 0usize;
    while i < combos.len() {
        let (st, ly) = match combos.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let t = kor_table(st, ly);
        let mut k = 0usize;
        while k < CELLS {
            let c = match t.get(k) {
                Some(v) => *v,
                None => return false,
            };
            if kor_slot_hits(st, ly, c) != 1 {
                return false;
            }
            let (r, col) = match kor_code_slot(st, ly, c) {
                Some(v) => v,
                None => return false,
            };
            if kor_key(st, ly, r, col) != Some(c) {
                return false;
            }
            // 行列与表下标的换算必须与行首下标表一致。
            if row_start_or_zero(r) + col != k {
                return false;
            }
            k += 1;
        }
        i += 1;
    }
    true
}

/// 键位边界：越出记长的列与越出的行一律取不到字。
pub fn kor_key_bounds_ok() -> bool {
    let mut i = 0usize;
    while i < KOR_STATES {
        let st = match ALL_STATES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let mut j = 0usize;
        while j < LAYERS_TOTAL {
            let ly = match Layer::from_index(j) {
                Some(v) => v,
                None => return false,
            };
            let mut r = 0usize;
            while r < ROWS {
                let len = row_len_or_zero(r);
                if kor_key(st, ly, r, len).is_some() {
                    return false;
                }
                if kor_key(st, ly, r, len - 1).is_none() {
                    return false;
                }
                r += 1;
            }
            if kor_key(st, ly, ROWS, 0).is_some() {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ── 韩文闸：母字表与组合恒等式 ───────────────────────────────────
/// 三张母字索引表自洽：规模、互异、窗口、索引往返。
pub fn jamo_tables_ok() -> bool {
    if LEAD_JAMO.len() != LEADS || VOWEL_JAMO.len() != VOWELS || TAIL_JAMO.len() != TAILS {
        return false;
    }
    if !row_unique(&LEAD_JAMO.to_vec()) {
        return false;
    }
    if !row_unique(&VOWEL_JAMO.to_vec()) {
        return false;
    }
    if !row_unique(&TAIL_JAMO.to_vec()) {
        return false;
    }
    let mut i = 0usize;
    while i < LEADS {
        let c = match LEAD_JAMO.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if !jamo_window_ok(c) || lead_index(c) != Some(i) {
            return false;
        }
        if jamo_at(JamoKind::Lead, i) != Some(c) {
            return false;
        }
        i += 1;
    }
    let mut j = 0usize;
    while j < VOWELS {
        let c = match VOWEL_JAMO.get(j) {
            Some(v) => *v,
            None => return false,
        };
        if !jamo_window_ok(c) || vowel_index(c) != Some(j) {
            return false;
        }
        if jamo_at(JamoKind::Vowel, j) != Some(c) {
            return false;
        }
        j += 1;
    }
    // 终声槽位 0 代表「无收音」，码位表整体右移一格。
    if jamo_at(JamoKind::Tail, 0).is_some() {
        return false;
    }
    let mut k = 0usize;
    while k < TAILS {
        let c = match TAIL_JAMO.get(k) {
            Some(v) => *v,
            None => return false,
        };
        if !jamo_window_ok(c) || tail_index(c) != Some(k + 1) {
            return false;
        }
        if jamo_at(JamoKind::Tail, k + 1) != Some(c) {
            return false;
        }
        k += 1;
    }
    if jamo_at(JamoKind::Tail, TAIL_SLOTS).is_some() {
        return false;
    }
    if jamo_at(JamoKind::Lead, LEADS).is_some() || jamo_at(JamoKind::Vowel, VOWELS).is_some() {
        return false;
    }
    true
}

/// 组合恒等式：完整音节区 11172 码逐码 `compose(decompose(x)) == x`。
pub fn compose_identity_ok() -> bool {
    if SYL_START + (SYLLABLES as u32) - 1 != SYL_LAST {
        return false;
    }
    if LEADS * VOWELS * TAIL_SLOTS != SYLLABLES {
        return false;
    }
    let mut i = 0usize;
    while i < SYLLABLES {
        let code = SYL_START + i as u32;
        let c = match char::from_u32(code) {
            Some(v) => v,
            None => return false,
        };
        let (l, v, t) = match decompose_syllable(c) {
            Some(x) => x,
            None => return false,
        };
        if l >= LEADS || v >= VOWELS || t >= TAIL_SLOTS {
            return false;
        }
        match compose_syllable(l, v, t) {
            Some(back) => {
                if back != c {
                    return false;
                }
            }
            None => return false,
        }
        // 母字取用与索引一致：终声槽 0 必须查不到码位。
        if t != 0 {
            if jamo_at(JamoKind::Tail, t).is_none() {
                return false;
            }
        } else if jamo_at(JamoKind::Tail, 0).is_some() {
            return false;
        }
        if jamo_at(JamoKind::Lead, l).is_none() || jamo_at(JamoKind::Vowel, v).is_none() {
            return false;
        }
        i += 1;
    }
    true
}

/// 组合式边界：任一脚趾越窗即拒绝，音节区外的码位不可分解。
pub fn compose_bounds_ok() -> bool {
    if compose_syllable(LEADS, 0, 0).is_some() {
        return false;
    }
    if compose_syllable(0, VOWELS, 0).is_some() {
        return false;
    }
    if compose_syllable(0, 0, TAIL_SLOTS).is_some() {
        return false;
    }
    if compose_syllable(LEADS - 1, VOWELS - 1, TAIL_SLOTS - 1).is_none() {
        return false;
    }
    if decompose_syllable('가').is_none() {
        return false;
    }
    match char::from_u32(SYL_START - 1) {
        Some(c) => {
            if decompose_syllable(c).is_some() {
                return false;
            }
        }
        None => return false,
    }
    match char::from_u32(SYL_LAST + 1) {
        Some(c) => {
            if decompose_syllable(c).is_some() {
                return false;
            }
        }
        None => return false,
    }
    if decompose_syllable('a').is_some() || decompose_syllable('한').is_none() {
        return false;
    }
    true
}

/// 合成对可回推：成分与结果都在索引表内，且对内键唯一、结果唯一。
pub fn merge_pairs_ok() -> bool {
    if LEAD_MERGE_PAIRS.len() != LEAD_MERGE
        || VOWEL_MERGE_PAIRS.len() != VOWEL_MERGE
        || TAIL_MERGE_PAIRS.len() != TAIL_MERGE
    {
        return false;
    }
    let mut seen_lead: Vec<char> = Vec::new();
    let mut i = 0usize;
    while i < LEAD_MERGE_PAIRS.len() {
        let (a, b, z) = match LEAD_MERGE_PAIRS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if lead_index(a).is_none()
            || lead_index(b).is_none()
            || lead_index(z).is_none()
            || a != b
        {
            return false;
        }
        if merge_lead(a, b) != Some(z) {
            return false;
        }
        if seen_lead.contains(&z) {
            return false;
        }
        seen_lead.push(z);
        i += 1;
    }
    let mut seen_vowel: Vec<char> = Vec::new();
    let mut j = 0usize;
    while j < VOWEL_MERGE_PAIRS.len() {
        let (a, b, z) = match VOWEL_MERGE_PAIRS.get(j) {
            Some(v) => *v,
            None => return false,
        };
        if vowel_index(a).is_none()
            || vowel_index(b).is_none()
            || vowel_index(z).is_none()
        {
            return false;
        }
        if a == b {
            return false;
        }
        if merge_vowel(a, b) != Some(z) || merge_vowel(b, a).is_some() {
            // 复元音合成只认「前键 + 后键」方向，反向不得成对。
            return false;
        }
        if seen_vowel.contains(&z) {
            return false;
        }
        seen_vowel.push(z);
        j += 1;
    }
    let mut seen_tail: Vec<char> = Vec::new();
    let mut k = 0usize;
    while k < TAIL_MERGE_PAIRS.len() {
        let (a, b, z) = match TAIL_MERGE_PAIRS.get(k) {
            Some(v) => *v,
            None => return false,
        };
        if tail_index(a).is_none() || tail_index(b).is_none() || tail_index(z).is_none() {
            return false;
        }
        if a == b {
            return false;
        }
        if merge_tail(a, b) != Some(z) || merge_tail(b, a).is_some() {
            return false;
        }
        if seen_tail.contains(&z) {
            return false;
        }
        seen_tail.push(z);
        k += 1;
    }
    // 结果必须落在「只能合成」清单里，不能是可直键的母字。
    let mut m = 0usize;
    while m < seen_vowel.len() {
        if let Some(c) = seen_vowel.get(m) {
            if !COMPOSITE_VOWELS.contains(c) {
                return false;
            }
        }
        m += 1;
    }
    let mut n = 0usize;
    while n < seen_tail.len() {
        if let Some(c) = seen_tail.get(n) {
            if !COMPOSITE_TAILS.contains(c) {
                return false;
            }
        }
        n += 1;
    }
    // 双向都查不到的组合不得被误判为可合成。
    if merge_lead('ㄱ', 'ㄴ').is_some()
        || merge_vowel('ㅏ', 'ㅣ').is_some()
        || merge_tail('ㄱ', 'ㄴ').is_some()
    {
        return false;
    }
    true
}

/// 可直键母字全部落在한글态两面上（基层或换档层任一）。
pub fn atomic_jamo_reachable_ok() -> bool {
    if ATOMIC_LEADS.len() != LEADS {
        return false;
    }
    if ATOMIC_VOWELS.len() + COMPOSITE_VOWELS.len() != VOWELS {
        return false;
    }
    if ATOMIC_TAILS.len() + COMPOSITE_TAILS.len() != TAILS {
        return false;
    }
    let mut i = 0usize;
    while i < ATOMIC_LEADS.len() {
        if let Some(c) = ATOMIC_LEADS.get(i) {
            if kor_code_slot(State::Hangul, Layer::Base, *c).is_none()
                && kor_code_slot(State::Hangul, Layer::Shift, *c).is_none()
            {
                return false;
            }
        }
        i += 1;
    }
    let mut j = 0usize;
    while j < ATOMIC_VOWELS.len() {
        if let Some(c) = ATOMIC_VOWELS.get(j) {
            if kor_code_slot(State::Hangul, Layer::Base, *c).is_none()
                && kor_code_slot(State::Hangul, Layer::Shift, *c).is_none()
            {
                return false;
            }
        }
        j += 1;
    }
    let mut k = 0usize;
    while k < ATOMIC_TAILS.len() {
        if let Some(c) = ATOMIC_TAILS.get(k) {
            if kor_code_slot(State::Hangul, Layer::Base, *c).is_none()
                && kor_code_slot(State::Hangul, Layer::Shift, *c).is_none()
            {
                return false;
            }
        }
        k += 1;
    }
    true
}

/// 只能合成的母字绝不出现在物理面上。
pub fn composite_not_keyable_ok() -> bool {
    let mut i = 0usize;
    while i < COMPOSITE_VOWELS.len() {
        let c = match COMPOSITE_VOWELS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if kor_code_slot(State::Hangul, Layer::Base, c).is_some() {
            return false;
        }
        if kor_code_slot(State::Hangul, Layer::Shift, c).is_some() {
            return false;
        }
        i += 1;
    }
    let mut j = 0usize;
    while j < COMPOSITE_TAILS.len() {
        let c = match COMPOSITE_TAILS.get(j) {
            Some(v) => *v,
            None => return false,
        };
        if kor_code_slot(State::Hangul, Layer::Base, c).is_some() {
            return false;
        }
        if kor_code_slot(State::Hangul, Layer::Shift, c).is_some() {
            return false;
        }
        j += 1;
    }
    true
}

// ── 韩文闸：黄金 20 组与切换态机 ─────────────────────────────────
/// 单条韩文黄金是否拼出期望串。
pub fn kor_gold_check(g: &KorGold) -> bool {
    if g.id == 0 || g.id as usize > GOLD_KOR {
        return false;
    }
    if g.seq.is_empty() || g.expect.is_empty() || g.note.is_empty() {
        return false;
    }
    if compose_stream(g.seq) != g.expect {
        return false;
    }
    // 期望串的每个码位都必须落在音节区且可回推索引。
    let mut chars = g.expect.chars();
    loop {
        match chars.next() {
            Some(c) => {
                if decompose_syllable(c).is_none() {
                    return false;
                }
            }
            None => break,
        }
    }
    true
}

/// 韩文黄金命中数。
pub fn kor_gold_hits() -> usize {
    let mut acc = 0usize;
    let mut i = 0usize;
    while i < KOR_GOLD.len() {
        if let Some(g) = KOR_GOLD.get(i) {
            if kor_gold_check(g) {
                acc += 1;
            }
        }
        i += 1;
    }
    acc
}

/// 韩文黄金总闸：条数、序号连续、逐条命中、说明互异、音节数与母字数关系。
pub fn golden_kor_ok() -> bool {
    if KOR_GOLD.len() != GOLD_KOR {
        return false;
    }
    let mut notes: Vec<&'static str> = Vec::new();
    let mut i = 0usize;
    while i < KOR_GOLD.len() {
        let g = match KOR_GOLD.get(i) {
            Some(v) => v,
            None => return false,
        };
        if g.id as usize != i + 1 {
            return false;
        }
        if notes.contains(&g.note) {
            return false;
        }
        notes.push(g.note);
        if !kor_gold_check(g) {
            return false;
        }
        i += 1;
    }
    if kor_gold_hits() != GOLD_KOR {
        return false;
    }
    // 卡面口径：韩文 20 组 + 东欧 30 组 = 五十组黄金。
    GOLD_KOR + GOLD_EAST == GOLD_TOTAL
}

/// 两态切换：连按两次必回起始态，且切换前一定排空在途。
pub fn switch_cycle_ok() -> bool {
    let mut si = 0usize;
    while si < ALL_STATES.len() {
        let st = match ALL_STATES.get(si) {
            Some(v) => *v,
            None => return false,
        };
        let mut sw = Switch::with_mode(st);
        if sw.mode() != st {
            return false;
        }
        if sw.toggle() != st.flipped() {
            return false;
        }
        if sw.toggle() != st {
            return false;
        }
        if sw.flips != 2 || sw.mode() != st {
            return false;
        }
        // 切换必须排空在途：先压三键，再切换，队列归零且记账一次。
        let mut sw2 = Switch::with_mode(st);
        if !sw2.press('ㄱ') || !sw2.press('ㅏ') || !sw2.press('ㄴ') {
            return false;
        }
        if sw2.pending() != 3 {
            return false;
        }
        let before = sw2.flushes;
        sw2.toggle();
        if sw2.pending() != 0 || sw2.flushes != before + 1 {
            return false;
        }
        if sw2.queued != 3 || sw2.drops != 0 {
            return false;
        }
        let q = sw2.drain();
        if !q.is_empty() {
            return false;
        }
        si += 1;
    }
    true
}

/// 在途队列：满槽后丢键必须记账，不得静默吞键；排空顺序即压入顺序。
pub fn pending_capacity_ok() -> bool {
    let mut sw = Switch::new();
    if sw.pending() != 0 || sw.last_key().is_some() {
        return false;
    }
    let mut pressed: Vec<char> = Vec::new();
    let mut i = 0usize;
    while i < PENDING_CAP {
        let c = match ALL_STATES.get(0) {
            Some(_) => match LEAD_JAMO.get(i % LEADS) {
                Some(v) => *v,
                None => return false,
            },
            None => return false,
        };
        if !sw.press(c) {
            return false;
        }
        pressed.push(c);
        if sw.last_key() != Some(c) {
            return false;
        }
        i += 1;
    }
    if sw.pending() != PENDING_CAP || sw.drops != 0 {
        return false;
    }
    if sw.press('ㅎ') {
        return false;
    }
    if sw.drops != 1 || sw.pending() != PENDING_CAP {
        return false;
    }
    let drained = sw.drain();
    if drained.len() != pressed.len() {
        return false;
    }
    let mut k = 0usize;
    while k < pressed.len() {
        match (pressed.get(k), drained.get(k)) {
            (Some(a), Some(b)) if *a == *b => {}
            _ => return false,
        }
        k += 1;
    }
    if sw.pending() != 0 || sw.flushes != 1 {
        return false;
    }
    // 排空后可再压入，容量不是一次性资源。
    if !sw.press('ㅏ') || sw.pending() != 1 {
        return false;
    }
    true
}

/// 态机解析与键位表一致：两态两层逐格回读同一字。
pub fn switch_resolve_ok() -> bool {
    let mut si = 0usize;
    while si < ALL_STATES.len() {
        let st = match ALL_STATES.get(si) {
            Some(v) => *v,
            None => return false,
        };
        let sw = Switch::with_mode(st);
        let mut li = 0usize;
        while li < LAYERS_TOTAL {
            let ly = match Layer::from_index(li) {
                Some(v) => v,
                None => return false,
            };
            let mut r = 0usize;
            while r < ROWS {
                let len = row_len_or_zero(r);
                let mut c = 0usize;
                while c < len {
                    let direct = match kor_key(st, ly, r, c) {
                        Some(v) => v,
                        None => return false,
                    };
                    if sw.resolve(ly, r, c) != Some(direct) {
                        return false;
                    }
                    c += 1;
                }
                r += 1;
            }
            li += 1;
        }
        if sw.mode() != st {
            return false;
        }
        si += 1;
    }
    true
}

/// 韩文专属键位登记（한/영 与 한자 落在 46 格窗口之外）。
pub const KOR_EXTRA_KEYS: [(u8, &'static str); 2] = [
    (HAN_YEONG_POS, "han-yeong"),
    (HANJA_POS, "hanja"),
];

/// 按物理槽取专属键标签（窗口内或未知返回 `None`）。
pub fn kor_extra_tag(pos: u8) -> Option<&'static str> {
    if (pos as usize) < CELLS {
        return None;
    }
    let mut i = 0usize;
    while i < KOR_EXTRA_KEYS.len() {
        let (p, tag) = match KOR_EXTRA_KEYS.get(i) {
            Some(v) => *v,
            None => return None,
        };
        if p == pos {
            return Some(tag);
        }
        i += 1;
    }
    None
}

/// 专属键槽自洽：两键都在 46 格外、互不相同，且窗口内取不到标签。
pub fn han_key_slots_ok() -> bool {
    if HAN_YEONG_POS >= HANJA_POS {
        return false;
    }
    if (HAN_YEONG_POS as usize) < CELLS || (HANJA_POS as usize) < CELLS {
        return false;
    }
    if kor_extra_tag(HAN_YEONG_POS) != Some("han-yeong")
        || kor_extra_tag(HANJA_POS) != Some("hanja")
    {
        return false;
    }
    let mut i = 0usize;
    while i < CELLS {
        if kor_extra_tag(i as u8).is_some() {
            return false;
        }
        i += 1;
    }
    if kor_extra_tag(99).is_some() {
        return false;
    }
    // 한/영 键驱动态机，한자 键不翻态。
    let mut sw = Switch::new();
    let start = sw.mode();
    sw.toggle();
    if sw.mode() == start || sw.flips != 1 {
        return false;
    }
    true
}

/// 组合流语义抽查：空串、非母字直通、跨音节续打。
pub fn compose_stream_edge_ok() -> bool {
    if !compose_stream("").is_empty() {
        return false;
    }
    if compose_stream("a") != "a" {
        return false;
    }
    if compose_stream("ㅏ") != "아" {
        // 缺初声时以零声母 ㅇ 补位，这是组合引擎的既定语义。
        return false;
    }
    if compose_stream("ㄱ") != "ㄱ" {
        return false;
    }
    if compose_stream("ㅎㅏㄴㄱㅜㄱ") != "한국" {
        return false;
    }
    if compose_stream("ㄱㅏㄴ") != "간" {
        return false;
    }
    if compose_stream("ㄱㅏㄴㅏ") != "간아" {
        // 收音后的元音只能另起零声母音节，不能把 ㄴ 抢作初声。
        return false;
    }
    if compose_stream("ㅂㅏㄱㅏ") != "박아" {
        return false;
    }
    if compose_stream("ㄱㅏㅂㅏ") != "갑아" {
        // ㅂ 本身是合法收音，先占终声槽，随后的元音另起音节。
        return false;
    }
    if compose_stream("ㅎㅏㄴ") != "한" {
        return false;
    }
    true
}

// ── 韩文键面账（五面） ───────────────────────────────────────────
/// 一个键面：态 × 层的一个 46 格窗，或两态共用的 13 格数字行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Face {
    /// 라틴态基层。
    LatinBase,
    /// 라틴态换档。
    LatinShift,
    /// 한글态基层。
    HangulBase,
    /// 한글态换档。
    HangulShift,
    /// 两态共用的数字行。
    SharedRow,
}

impl Face {
    /// 面号。
    pub fn index(self) -> usize {
        match self {
            Face::LatinBase => 0,
            Face::LatinShift => 1,
            Face::HangulBase => 2,
            Face::HangulShift => 3,
            Face::SharedRow => 4,
        }
    }

    /// 面号 → 面（越窗返回 `None`）。
    pub fn from_index(v: usize) -> Option<Face> {
        match v {
            0 => Some(Face::LatinBase),
            1 => Some(Face::LatinShift),
            2 => Some(Face::HangulBase),
            3 => Some(Face::HangulShift),
            4 => Some(Face::SharedRow),
            _ => None,
        }
    }

    /// 面标签。
    pub fn tag(self) -> &'static str {
        match self {
            Face::LatinBase => "latin-base",
            Face::LatinShift => "latin-shift",
            Face::HangulBase => "hangul-base",
            Face::HangulShift => "hangul-shift",
            Face::SharedRow => "shared-row",
        }
    }

    /// 标签 → 面（未知返回 `None`）。
    pub fn from_tag(s: &str) -> Option<Face> {
        let mut i = 0usize;
        while i < ALL_FACES.len() {
            if let Some(f) = ALL_FACES.get(i) {
                if f.tag() == s {
                    return Face::from_index(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 态与层（共用数字行不属于单一层，返回 `None`）。
    pub fn state_layer(self) -> Option<(State, Layer)> {
        match self {
            Face::LatinBase => Some((State::Latin, Layer::Base)),
            Face::LatinShift => Some((State::Latin, Layer::Shift)),
            Face::HangulBase => Some((State::Hangul, Layer::Base)),
            Face::HangulShift => Some((State::Hangul, Layer::Shift)),
            Face::SharedRow => None,
        }
    }

    /// 该面的格数。
    pub fn cells(self) -> usize {
        match self {
            Face::SharedRow => SHIFT_COLS,
            _ => CELLS,
        }
    }

    /// 该面的码位序列（按行序铺放）。
    pub fn slots(self) -> Vec<char> {
        match self.state_layer() {
            Some((st, ly)) => {
                let mut out = Vec::with_capacity(CELLS);
                let mut r = 0usize;
                while r < ROWS {
                    let len = row_len_or_zero(r);
                    let start = row_start_or_zero(r);
                    let mut c = 0usize;
                    while c < len {
                        if let Some(v) = kor_table(st, ly).get(start + c) {
                            out.push(*v);
                        }
                        c += 1;
                    }
                    r += 1;
                }
                out
            }
            None => match table_row(kor_table(State::Hangul, Layer::Base), 0) {
                Some(v) => v,
                None => Vec::new(),
            },
        }
    }

    /// 该面的互异码位数。
    pub fn distinct(self) -> usize {
        let s = self.slots();
        let mut seen: Vec<char> = Vec::new();
        let mut i = 0usize;
        while i < s.len() {
            if let Some(c) = s.get(i) {
                if !seen.contains(c) {
                    seen.push(*c);
                }
            }
            i += 1;
        }
        seen.len()
    }

    /// 折叠校验和：逐码折叠，共用数字行只折 13 格。
    pub fn checksum(self) -> u64 {
        let mut h = p_core::FNV_OFF;
        let s = self.slots();
        let mut i = 0usize;
        while i < s.len() {
            if let Some(c) = s.get(i) {
                h = p_core::fold_u32(h, *c as u32);
            }
            i += 1;
        }
        h = p_core::fold_u32(h, self.cells() as u32);
        h = p_core::fold_byte(h, self.index() as u8);
        h
    }

    /// 折叠签名（8 字节，末字节带面号）。
    pub fn signature(self) -> [u8; 8] {
        let sum = self.checksum();
        let mut out = [0u8; 8];
        let mut i = 0usize;
        while i < 8 {
            if let Some(s) = out.get_mut(i) {
                *s = ((sum >> (i * 8)) & 0xff) as u8;
            }
            i += 1;
        }
        out[7] = out[7].wrapping_add(self.index() as u8);
        out
    }

    /// 签名十六进制串（16 字符）。
    pub fn sig_hex(self) -> String {
        let sig = self.signature();
        let mut out = String::with_capacity(16);
        let mut i = 0usize;
        while i < sig.len() {
            if let Some(b) = sig.get(i) {
                out.push_str(&format!("{:02x}", b));
            }
            i += 1;
        }
        out
    }
}

/// 全部键面（顺序即面号序）。
pub const ALL_FACES: [Face; KOR_FACES] = [
    Face::LatinBase,
    Face::LatinShift,
    Face::HangulBase,
    Face::HangulShift,
    Face::SharedRow,
];

/// 面内互异：每一面的码位数都等于该面格数（面内零重复）。
pub fn face_cells_unique_ok() -> bool {
    let mut i = 0usize;
    while i < ALL_FACES.len() {
        let f = match ALL_FACES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if f.slots().len() != f.cells() {
            return false;
        }
        if f.distinct() != f.cells() {
            return false;
        }
        i += 1;
    }
    if Face::SharedRow.cells() != SHIFT_COLS || Face::HangulBase.cells() != CELLS {
        return false;
    }
    true
}

/// 五面互异：签名两两不同，标签可反查，面号连续。
pub fn faces_distinct_ok() -> bool {
    if ALL_FACES.len() != KOR_FACES {
        return false;
    }
    let mut i = 0usize;
    while i < ALL_FACES.len() {
        let f = match ALL_FACES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if f.index() != i || Face::from_index(i) != Some(f) || Face::from_tag(f.tag()) != Some(f)
        {
            return false;
        }
        if Face::from_index(KOR_FACES).is_some() {
            return false;
        }
        let mut j = i + 1;
        while j < ALL_FACES.len() {
            if let Some(g) = ALL_FACES.get(j) {
                if f.signature() == g.signature() {
                    return false;
                }
                if f.tag() == g.tag() {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 共用数字行语义：该面的 13 格在两态基层逐格相同。
pub fn shared_row_face_ok() -> bool {
    let s = Face::SharedRow.slots();
    if s.len() != SHIFT_COLS {
        return false;
    }
    let mut i = 0usize;
    while i < s.len() {
        let c = match s.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if kor_key(State::Latin, Layer::Base, 0, i) != Some(c) {
            return false;
        }
        if kor_key(State::Hangul, Layer::Base, 0, i) != Some(c) {
            return false;
        }
        i += 1;
    }
    true
}

// ── 账行：摘要、渲染、回读 ───────────────────────────────────────
/// 东欧布局账摘要。
pub fn east_digest(id: EastId, golden: u32, hit: u32, cells: u32) -> u64 {
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

/// 韩文键面账摘要。
pub fn face_digest(face: Face, count: u32, cells: u32) -> u64 {
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_byte(h, face.index() as u8);
    h = p_core::fold_u32(h, count);
    h = p_core::fold_u32(h, cells);
    let s = face.signature();
    let mut i = 0usize;
    while i < s.len() {
        if let Some(b) = s.get(i) {
            h = p_core::fold_byte(h, *b);
        }
        i += 1;
    }
    h
}

/// 十进制串解析（越界或非数字返回 `None`）。
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
        if !(b'0'..=b'9').contains(&b) {
            return None;
        }
        acc = acc.checked_mul(10)?;
        acc = acc.checked_add((b - b'0') as u32)?;
        i += 1;
    }
    Some(acc)
}

/// 十六进制摘要串解析（必须恰好 16 位）。
pub fn parse_hex16(s: &str) -> Option<u64> {
    let bytes = s.as_bytes();
    if bytes.len() != 16 {
        return None;
    }
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

/// 渲染东欧布局账行（每套一行）。
pub fn render_east_row(id: EastId) -> String {
    let hit = east_gold_hits(id) as u32;
    format!(
        "{}|layout={}|golden={}|hit={}|cells={}|sig={}|digest={:016x}",
        EAST_TAG,
        id.tag(),
        id.golden_count() as u32,
        hit,
        CELLS as u32,
        id.sig_hex(),
        east_digest(id, id.golden_count() as u32, hit, CELLS as u32)
    )
}

/// 回读东欧账行：重算命中数、签名与摘要。
pub fn audit_east_row(row: &str) -> bool {
    let f = match row_fields(row, EAST_TAG, &EAST_KEYS) {
        Some(v) if v.len() == EAST_KEYS.len() + 1 => v,
        _ => return false,
    };
    let id = match f.first().copied().and_then(EastId::from_tag) {
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
    if golden != id.golden_count() as u32 || cells != CELLS as u32 {
        return false;
    }
    if hit != east_gold_hits(id) as u32 {
        return false;
    }
    if f.get(4).copied() != Some(id.sig_hex().as_str()) {
        return false;
    }
    match f.get(5).copied().and_then(parse_hex16) {
        Some(v) => v == east_digest(id, golden, hit, cells),
        None => false,
    }
}

/// 渲染韩文键面账行（每面一行）。
pub fn render_kor_row(face: Face) -> String {
    let count = face.distinct() as u32;
    let cells = face.cells() as u32;
    format!(
        "{}|face={}|count={}|cells={}|sig={}|digest={:016x}",
        KOR_TAG,
        face.tag(),
        count,
        cells,
        face.sig_hex(),
        face_digest(face, count, cells)
    )
}

/// 回读韩文键面账行：重算互异数、格数与摘要。
pub fn audit_kor_row(row: &str) -> bool {
    let f = match row_fields(row, KOR_TAG, &KOR_KEYS) {
        Some(v) if v.len() == KOR_KEYS.len() + 1 => v,
        _ => return false,
    };
    let face = match f.first().copied().and_then(Face::from_tag) {
        Some(v) => v,
        None => return false,
    };
    let count = match f.get(1).copied().and_then(parse_u32) {
        Some(v) => v,
        None => return false,
    };
    let cells = match f.get(2).copied().and_then(parse_u32) {
        Some(v) => v,
        None => return false,
    };
    if count != face.distinct() as u32 || cells != face.cells() as u32 {
        return false;
    }
    if f.get(3).copied() != Some(face.sig_hex().as_str()) {
        return false;
    }
    match f.get(4).copied().and_then(parse_hex16) {
        Some(v) => v == face_digest(face, count, cells),
        None => false,
    }
}

/// 导出东欧账行（四套各一行）。
pub fn export_east_rows() -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < EAST_ROW_SLOTS {
        if let Some(id) = EastId::from_index(i) {
            out.push(render_east_row(id));
        }
        i += 1;
    }
    out
}

/// 导出韩文键面账行（五面各一行）。
pub fn export_kor_rows() -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < KOR_ROW_SLOTS {
        if let Some(f) = Face::from_index(i) {
            out.push(render_kor_row(f));
        }
        i += 1;
    }
    out
}

/// 篡改账行末位（用于回读拒伪）。
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

/// 复跑一致：两族账行导出两次逐行相等、逐行可回读、行间不撞。
pub fn rows_stable() -> bool {
    let ea = export_east_rows();
    let eb = export_east_rows();
    let ka = export_kor_rows();
    let kb = export_kor_rows();
    if ea.len() != EAST_ROW_SLOTS || eb.len() != EAST_ROW_SLOTS {
        return false;
    }
    if ka.len() != KOR_ROW_SLOTS || kb.len() != KOR_ROW_SLOTS {
        return false;
    }
    let mut i = 0usize;
    while i < ea.len() {
        match (ea.get(i), eb.get(i)) {
            (Some(x), Some(y)) if *x == *y => {}
            _ => return false,
        }
        let row = match ea.get(i) {
            Some(v) => v.as_str(),
            None => return false,
        };
        if !audit_east_row(row) {
            return false;
        }
        let mut j = i + 1;
        while j < ea.len() {
            if ea.get(j) == ea.get(i) {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    let mut k = 0usize;
    while k < ka.len() {
        match (ka.get(k), kb.get(k)) {
            (Some(x), Some(y)) if *x == *y => {}
            _ => return false,
        }
        let row = match ka.get(k) {
            Some(v) => v.as_str(),
            None => return false,
        };
        if !audit_kor_row(row) {
            return false;
        }
        let mut m = k + 1;
        while m < ka.len() {
            if ka.get(m) == ka.get(k) {
                return false;
            }
            m += 1;
        }
        k += 1;
    }
    true
}

/// 账行族际隔离：东欧行不得被韩文回读接受，反之亦然。
pub fn row_family_isolation_ok() -> bool {
    let east_rows = export_east_rows();
    let kor_rows = export_kor_rows();
    let east = match east_rows.first() {
        Some(v) => v.as_str(),
        None => return false,
    };
    let kor = match kor_rows.first() {
        Some(v) => v.as_str(),
        None => return false,
    };
    if audit_east_row(kor) || audit_kor_row(east) {
        return false;
    }
    if audit_east_row("") || audit_kor_row("") {
        return false;
    }
    // 缺字段、错键序、错标签一律拒收。
    if audit_east_row("EAST46|layout=ytsuen|golden=12|hit=12|cells=46|sig=0000000000000000") {
        return false;
    }
    if audit_kor_row("KOR2FACE|face=latin-base|count=46") {
        return false;
    }
    if audit_east_row("EAST46|lay=ytsuen|golden=12|hit=12|cells=46|sig=0000000000000000|digest=0000000000000000")
    {
        return false;
    }
    true
}

// ── 判据出口 ─────────────────────────────────────────────────────
/// 八项判据（每卡四条），全部为纯函数计算，无副作用。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out = Vec::new();

    // 1) F49103 两态表自洽：46 格 × 四面、数字行跨态同码、换档不造第三态。
    let tbl = kor_tables_ok();
    let shared = kor_number_row_shared_ok();
    let inv = kor_shift_invariant_ok();
    out.push((
        "F49103/两态表自洽",
        tbl && shared && inv,
        format!("tables={} shared_row={} shift_invariant={}", tbl, shared, inv),
    ));

    // 2) F49103 组合恒等式：11172 码逐码回算，母字索引与边界全绿。
    let ident = compose_identity_ok();
    let jamo = jamo_tables_ok();
    let bounds = compose_bounds_ok();
    out.push((
        "F49103/组合恒等式",
        ident && jamo && bounds,
        format!(
            "identity={} jamo={} bounds={} span={}",
            ident, jamo, bounds, SYLLABLES
        ),
    ));

    // 3) F49103 한/영 切换态：连按回态、在途排空记账、态机与键位一致。
    let cyc = switch_cycle_ok();
    let pend = pending_capacity_ok();
    let res = switch_resolve_ok();
    let slot = han_key_slots_ok();
    out.push((
        "F49103/한영切换态",
        cyc && pend && res && slot,
        format!(
            "cycle={} pending={} resolve={} extra_slot={}",
            cyc, pend, res, slot
        ),
    ));

    // 4) F49103 韩文黄金对账：20 组逐条拼出、合成对可回推、直键与合成互斥。
    let gold = golden_kor_ok();
    let merge = merge_pairs_ok();
    let reach = atomic_jamo_reachable_ok();
    let comp = composite_not_keyable_ok();
    let edge = compose_stream_edge_ok();
    out.push((
        "F49103/韩文黄金对账",
        gold && merge && reach && comp && edge,
        format!(
            "kor_gold={} hits={}/{} merge={} reachable={}",
            gold,
            kor_gold_hits(),
            GOLD_KOR,
            merge,
            reach && comp && edge
        ),
    ));

    // 5) F49104 四套布局互异：表自洽、签名两两不同、标签可反查。
    let tbl_e = east_tables_ok();
    let dist = east_layouts_distinct();
    let len_e = east_row_len_ok();
    out.push((
        "F49104/四套布局互异",
        tbl_e && dist && len_e,
        format!("tables={} distinct={} row_len={}", tbl_e, dist, len_e),
    ));

    // 6) F49104 东欧黄金对账：30 组逐条命中，逐套规模与总命中一致。
    let ge = golden_east_ok();
    let hits_e = east_gold_total_hits();
    let per_ok = EastId::from_index(EAST).is_none()
        && EastId::from_tag("ytsuen") == Some(EastId::Russian)
        && EastId::from_tag("qwertz-hu") == Some(EastId::Hungarian);
    out.push((
        "F49104/东欧黄金对账",
        ge && hits_e == GOLD_EAST && per_ok,
        format!("east_gold={} hits={}/{} face={}", ge, hits_e, GOLD_EAST, per_ok),
    ));

    // 7) F49104 换档不倒置与西里尔清点：33 字母逐字反查、大写逐格回读。
    let shift = east_shift_distinct_ok();
    let cyr = cyrillic_inventory_ok();
    let case_ok = uppercase_pair_ok();
    let dia = diacritic_slots_ok();
    let rev = east_reverse_ok();
    out.push((
        "F49104/换档与西里尔",
        shift && cyr && case_ok && dia && rev,
        format!(
            "shift={} cyrillic={} upper={} diacritic={} reverse={}",
            shift, cyr, case_ok, dia, rev
        ),
    ));

    // 8) F49104 账行回读：两族账行可回读、拒篡改、复跑一致、族际不互认。
    let er = export_east_rows();
    let kr = export_kor_rows();
    let mut audit_ok = er.len() == EAST_ROW_SLOTS && kr.len() == KOR_ROW_SLOTS;
    let mut i = 0usize;
    while i < er.len() {
        let row = match er.get(i) {
            Some(v) => v.as_str(),
            None => {
                audit_ok = false;
                break;
            }
        };
        if !audit_east_row(row) || audit_east_row(&tamper_tail(row)) {
            audit_ok = false;
        }
        i += 1;
    }
    let mut k = 0usize;
    while k < kr.len() {
        let row = match kr.get(k) {
            Some(v) => v.as_str(),
            None => {
                audit_ok = false;
                break;
            }
        };
        if !audit_kor_row(row) || audit_kor_row(&tamper_tail(row)) {
            audit_ok = false;
        }
        k += 1;
    }
    let guard = segment_guard();
    let face = code_face_ok();
    let face_ok = face_cells_unique_ok() && faces_distinct_ok() && shared_row_face_ok();
    let latin_ok = latin_shift_case_ok() && kor_reverse_ok() && kor_key_bounds_ok();
    out.push((
        "F49104/账行回读",
        audit_ok
            && rows_stable()
            && row_family_isolation_ok()
            && guard
            && face
            && face_ok
            && latin_ok,
        format!(
            "east_rows={} kor_rows={} stable={} isolation={} seg=0x{:05x}",
            er.len(),
            kr.len(),
            rows_stable(),
            row_family_isolation_ok(),
            SEG_BASE
        ),
    ));

    out
}

/// 入口：八项判据写入 CheckSet。
pub fn run_unx49103_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F49103 韩文两态组合与东欧布局套");
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
    use alloc::vec;

    // ── 段位与规模 ───────────────────────────────────────────────
    #[test]
    fn seg_base_and_span() {
        assert_eq!(SEG_BASE, 0x737C0);
        assert_eq!(SEG_SPAN, 0x40);
        assert_eq!(KOR_BASE, SEG_BASE + 0x11);
        assert_eq!(KOR_CODES, 8);
    }

    #[test]
    fn segment_window_does_not_reach_neighbors() {
        assert_eq!(SEG_BASE + SEG_SPAN, 0x73800);
        assert!(KOR_BASE + KOR_CODES as u32 <= SEG_BASE + SEG_SPAN);
    }

    #[test]
    fn row_geometry_is_four_rows_of_declared_length() {
        assert_eq!(ROWS, 4);
        assert_eq!(COLS, 13);
        assert_eq!(ROW_LEN.len(), ROWS);
        assert_eq!(ROW_START.len(), ROWS);
        assert_eq!(ROW_LEN[0], 13);
        assert_eq!(ROW_LEN[1], 12);
        assert_eq!(ROW_LEN[2], 11);
        assert_eq!(ROW_LEN[3], 10);
        let mut acc = 0usize;
        let mut i = 0usize;
        while i < ROWS {
            assert_eq!(ROW_START[i], acc);
            acc += ROW_LEN[i] as usize;
            i += 1;
        }
        assert_eq!(acc, CELLS);
        assert_eq!(SHIFT_COLS, COLS);
        assert_eq!(LAYERS_TOTAL, 2);
    }

    #[test]
    fn syllable_block_scale_and_edges() {
        assert_eq!(LEADS, 19);
        assert_eq!(VOWELS, 21);
        assert_eq!(TAILS, 27);
        assert_eq!(TAIL_SLOTS, 28);
        assert_eq!(SYLLABLES, 19 * 21 * 28);
        assert_eq!(SYLLABLES, 11172);
        assert_eq!(SYL_START, 0xAC00);
        assert_eq!(SYL_START + SYLLABLES as u32 - 1, SYL_LAST);
        assert_eq!(SYL_LAST, 0xD7A3);
    }

    #[test]
    fn golden_counts_add_up() {
        assert_eq!(GOLD_RU, 12);
        assert_eq!(GOLD_CS, 6);
        assert_eq!(GOLD_SK, 6);
        assert_eq!(GOLD_HU, 6);
        assert_eq!(GOLD_EAST, 30);
        assert_eq!(GOLD_KOR, 20);
        assert_eq!(GOLD_TOTAL, 50);
        assert_eq!(GOLD_KOR + GOLD_EAST, GOLD_TOTAL);
        assert_eq!(EAST_GOLD.len(), GOLD_EAST);
        assert_eq!(KOR_GOLD.len(), GOLD_KOR);
    }

    #[test]
    fn merge_pair_tables_declared_size() {
        assert_eq!(LEAD_MERGE, 5);
        assert_eq!(VOWEL_MERGE, 7);
        assert_eq!(TAIL_MERGE, 11);
        assert_eq!(LEAD_MERGE_PAIRS.len(), LEAD_MERGE);
        assert_eq!(VOWEL_MERGE_PAIRS.len(), VOWEL_MERGE);
        assert_eq!(TAIL_MERGE_PAIRS.len(), TAIL_MERGE);
        assert_eq!(CYRILLIC, 33);
        assert_eq!(CYRILLIC_LETTERS.len(), CYRILLIC);
    }

    #[test]
    fn face_and_row_slot_counts() {
        assert_eq!(KOR_STATES, 2);
        assert_eq!(KOR_FACES, 5);
        assert_eq!(PENDING_CAP, 8);
        assert_eq!(EAST_ROW_SLOTS, EAST);
        assert_eq!(KOR_ROW_SLOTS, KOR_FACES);
        assert_eq!(ALL_STATES.len(), KOR_STATES);
        assert_eq!(ALL_FACES.len(), KOR_FACES);
        assert_eq!(ALL_EAST.len(), EAST);
    }

    // ── 诊断码面 ─────────────────────────────────────────────────
    #[test]
    fn east_code_values_are_contiguous() {
        assert_eq!(ALL.len(), 14);
        let mut i = 0usize;
        while i < ALL.len() {
            let c = match ALL.get(i) {
                Some(v) => *v,
                None => panic!("缺码"),
            };
            assert_eq!(c.val(), SEG_BASE + 1 + i as u32);
            i += 1;
        }
        assert_eq!(EastCode::RowBad.val(), SEG_BASE + 1);
        assert_eq!(EastCode::Incomplete.val(), SEG_BASE + 14);
    }

    #[test]
    fn kor_code_values_are_contiguous_in_subwindow() {
        assert_eq!(ALL_KOR.len(), KOR_CODES);
        let mut i = 0usize;
        while i < ALL_KOR.len() {
            let k = match ALL_KOR.get(i) {
                Some(v) => *v,
                None => panic!("缺码"),
            };
            assert_eq!(k.val(), KOR_BASE + i as u32);
            i += 1;
        }
        assert_eq!(KorCode::LeadBad.val(), SEG_BASE + 0x11);
        assert_eq!(KorCode::GoldenBad.val(), SEG_BASE + 0x18);
    }

    #[test]
    fn code_details_pairwise_distinct_within_each_family() {
        let mut i = 0usize;
        while i < ALL.len() {
            let a = match ALL.get(i) {
                Some(v) => *v,
                None => panic!("缺码"),
            };
            assert!(!a.detail().is_empty());
            assert!(!a.message().is_empty());
            assert_ne!(a.detail(), a.message());
            let mut j = i + 1;
            while j < ALL.len() {
                if let Some(b) = ALL.get(j) {
                    assert_ne!(a.detail(), b.detail());
                    assert_ne!(a.message(), b.message());
                }
                j += 1;
            }
            i += 1;
        }
        let mut k = 0usize;
        while k < ALL_KOR.len() {
            let a = match ALL_KOR.get(k) {
                Some(v) => *v,
                None => panic!("缺码"),
            };
            assert!(!a.detail().is_empty());
            assert_ne!(a.detail(), a.message());
            let mut m = k + 1;
            while m < ALL_KOR.len() {
                if let Some(b) = ALL_KOR.get(m) {
                    assert_ne!(a.detail(), b.detail());
                    assert_ne!(a.message(), b.message());
                }
                m += 1;
            }
            k += 1;
        }
    }

    #[test]
    fn free_detail_covers_both_windows_and_gap_is_unknown() {
        let mut i = 1u32;
        while i <= 14 {
            assert_ne!(detail(SEG_BASE + i), UNKNOWN);
            i += 1;
        }
        assert_eq!(detail(SEG_BASE), UNKNOWN);
        assert_eq!(detail(SEG_BASE + 15), UNKNOWN);
        assert_eq!(detail(SEG_BASE + 16), UNKNOWN);
        let mut j = 0x11u32;
        while j <= 0x18 {
            assert_ne!(detail(SEG_BASE + j), UNKNOWN);
            j += 1;
        }
        assert_eq!(detail(SEG_BASE + 0x19), UNKNOWN);
        assert_eq!(detail(SEG_BASE + SEG_SPAN), UNKNOWN);
        assert_eq!(detail(0), UNKNOWN);
        assert_eq!(detail(u32::MAX), UNKNOWN);
    }

    #[test]
    fn detail_agrees_with_enum_for_every_code() {
        let mut i = 0usize;
        while i < ALL.len() {
            if let Some(c) = ALL.get(i) {
                assert_eq!(detail(c.val()), c.detail());
            }
            i += 1;
        }
        let mut k = 0usize;
        while k < ALL_KOR.len() {
            if let Some(c) = ALL_KOR.get(k) {
                assert_eq!(detail(c.val()), c.detail());
            }
            k += 1;
        }
    }

    #[test]
    fn message_of_covers_windows_only() {
        let mut i = 1u32;
        while i <= 14 {
            assert!(message_of(SEG_BASE + i).is_some());
            i += 1;
        }
        assert!(message_of(SEG_BASE).is_none());
        assert!(message_of(SEG_BASE + 15).is_none());
        assert!(message_of(SEG_BASE + 16).is_none());
        let mut j = 0x11u32;
        while j <= 0x18 {
            assert!(message_of(SEG_BASE + j).is_some());
            j += 1;
        }
        assert!(message_of(SEG_BASE + 0x19).is_none());
        assert!(message_of(SEG_BASE + SEG_SPAN).is_none());
    }

    #[test]
    fn segment_guard_and_code_face_pass() {
        assert!(segment_guard());
        assert!(code_face_ok());
    }

    #[test]
    fn unknown_text_is_stable_and_nonempty() {
        assert!(!UNKNOWN.is_empty());
        assert_eq!(UNKNOWN, detail(SEG_BASE + 15));
    }

    // ── 层与布局标识 ─────────────────────────────────────────────
    #[test]
    fn layer_roundtrip_and_window() {
        assert_eq!(Layer::Base.index(), 0);
        assert_eq!(Layer::Shift.index(), 1);
        assert_eq!(Layer::from_index(0), Some(Layer::Base));
        assert_eq!(Layer::from_index(1), Some(Layer::Shift));
        assert_eq!(Layer::from_index(2), None);
        assert_eq!(Layer::Base.tag(), "base");
        assert_eq!(Layer::Shift.tag(), "shift");
    }

    #[test]
    fn east_id_index_and_tag_roundtrip() {
        let mut i = 0usize;
        while i < ALL_EAST.len() {
            let id = match ALL_EAST.get(i) {
                Some(v) => *v,
                None => panic!("缺布局"),
            };
            assert_eq!(id.index(), i);
            assert_eq!(EastId::from_index(i), Some(id));
            assert_eq!(EastId::from_tag(id.tag()), Some(id));
            assert!(!id.tag().is_empty());
            i += 1;
        }
        assert_eq!(EastId::from_index(EAST), None);
        assert_eq!(EastId::from_index(99), None);
        assert_eq!(EastId::from_tag("qwerty"), None);
        assert_eq!(EastId::from_tag(""), None);
        assert_eq!(EastId::Russian.tag(), "ytsuen");
        assert_eq!(EastId::Czech.tag(), "qwertz-cs");
        assert_eq!(EastId::Slovak.tag(), "qwertz-sk");
        assert_eq!(EastId::Hungarian.tag(), "qwertz-hu");
    }

    #[test]
    fn east_table_lengths_are_declared() {
        let mut i = 0usize;
        while i < ALL_EAST.len() {
            let id = match ALL_EAST.get(i) {
                Some(v) => *v,
                None => panic!("缺布局"),
            };
            assert_eq!(id.base_table().len(), CELLS);
            assert_eq!(id.shift_row().len(), SHIFT_COLS);
            assert!(id.golden_count() > 0);
            assert_eq!(id.golden_count(), east_gold_hits(id));
            i += 1;
        }
        assert_eq!(EastId::Russian.golden_count(), GOLD_RU);
        assert_eq!(EastId::Czech.golden_count(), GOLD_CS);
        assert_eq!(EastId::Slovak.golden_count(), GOLD_SK);
        assert_eq!(EastId::Hungarian.golden_count(), GOLD_HU);
    }

    #[test]
    fn east_key_window_bounds() {
        let id = EastId::Russian;
        assert!(id.key(Layer::Base, ROWS, 0).is_none());
        assert!(id.key(Layer::Shift, ROWS, 0).is_none());
        assert!(id.key(Layer::Base, 0, COLS).is_none());
        assert!(id.key(Layer::Base, 1, 12).is_none());
        assert!(id.key(Layer::Base, 2, 11).is_none());
        assert!(id.key(Layer::Base, 3, 10).is_none());
        assert!(id.key(Layer::Base, 3, 9).is_some());
        assert!(id.key(Layer::Shift, 0, 12).is_some());
        assert!(id.key(Layer::Shift, 0, 13).is_none());
    }

    #[test]
    fn east_key_matches_flat_tables() {
        let mut si = 0usize;
        while si < ALL_EAST.len() {
            let id = match ALL_EAST.get(si) {
                Some(v) => *v,
                None => panic!("缺布局"),
            };
            let mut r = 0usize;
            while r < ROWS {
                let len = row_len_or_zero(r);
                let start = row_start_or_zero(r);
                let mut c = 0usize;
                while c < len {
                    assert_eq!(
                        id.key(Layer::Base, r, c),
                        id.base_table().get(start + c).copied()
                    );
                    if r == 0 {
                        assert_eq!(id.key(Layer::Shift, r, c), id.shift_row().get(c).copied());
                    } else if let Some(b) = id.base_table().get(start + c) {
                        assert_eq!(id.key(Layer::Shift, r, c), Some(upper(*b)));
                    }
                    c += 1;
                }
                r += 1;
            }
            si += 1;
        }
    }

    #[test]
    fn upper_maps_letters_and_keeps_symbols() {
        assert_eq!(upper('а'), 'А');
        assert_eq!(upper('ш'), 'Ш');
        assert_eq!(upper('ě'), 'Ě');
        assert_eq!(upper('ő'), 'Ő');
        assert_eq!(upper('q'), 'Q');
        assert_eq!(upper('.'), '.');
        assert_eq!(upper('1'), '1');
        assert_eq!(upper('ㄱ'), 'ㄱ');
    }

    #[test]
    fn east_russian_anchors() {
        let ru = EastId::Russian;
        assert_eq!(ru.key(Layer::Base, 0, 0), Some('ё'));
        assert_eq!(ru.key(Layer::Base, 1, 0), Some('й'));
        assert_eq!(ru.key(Layer::Base, 1, 11), Some('ъ'));
        assert_eq!(ru.key(Layer::Base, 2, 0), Some('ф'));
        assert_eq!(ru.key(Layer::Base, 2, 10), Some('э'));
        assert_eq!(ru.key(Layer::Base, 3, 0), Some('я'));
        assert_eq!(ru.key(Layer::Shift, 0, 3), Some('№'));
        assert_eq!(ru.key(Layer::Shift, 1, 0), Some('Й'));
        assert_eq!(ru.key(Layer::Shift, 2, 0), Some('Ф'));
        assert_eq!(ru.key(Layer::Shift, 3, 0), Some('Я'));
    }

    #[test]
    fn east_qwertz_diacritic_anchors() {
        assert_eq!(EastId::Czech.key(Layer::Base, 0, 0), Some('ě'));
        assert_eq!(EastId::Czech.key(Layer::Base, 1, 10), Some('ú'));
        assert_eq!(EastId::Czech.key(Layer::Base, 2, 9), Some('ř'));
        assert_eq!(EastId::Czech.key(Layer::Base, 3, 7), Some('š'));
        assert_eq!(EastId::Slovak.key(Layer::Base, 0, 0), Some(';'));
        assert_eq!(EastId::Slovak.key(Layer::Base, 3, 7), Some('ň'));
        assert_eq!(EastId::Slovak.key(Layer::Base, 3, 9), Some('ť'));
        assert_eq!(EastId::Hungarian.key(Layer::Base, 0, 11), Some('ó'));
        assert_eq!(EastId::Hungarian.key(Layer::Base, 1, 10), Some('ő'));
        assert_eq!(EastId::Hungarian.key(Layer::Base, 2, 10), Some('ű'));
        assert_eq!(EastId::Hungarian.key(Layer::Shift, 0, 12), Some('Ö'));
    }

    #[test]
    fn east_signatures_are_pairwise_distinct() {
        assert!(east_layouts_distinct());
        let mut i = 0usize;
        while i < ALL_EAST.len() {
            let a = match ALL_EAST.get(i) {
                Some(v) => *v,
                None => panic!("缺布局"),
            };
            assert_eq!(a.sig_hex().len(), 16);
            assert!(a
                .sig_hex()
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
            let mut j = i + 1;
            while j < ALL_EAST.len() {
                if let Some(b) = ALL_EAST.get(j) {
                    assert_ne!(a.signature(), b.signature());
                    assert_ne!(a.tag(), b.tag());
                }
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn east_checksum_is_deterministic() {
        let mut i = 0usize;
        while i < ALL_EAST.len() {
            let id = match ALL_EAST.get(i) {
                Some(v) => *v,
                None => panic!("缺布局"),
            };
            assert_eq!(id.checksum(), id.checksum());
            assert_ne!(id.checksum(), p_core::FNV_OFF);
            i += 1;
        }
    }

    // ── 东欧闸 ───────────────────────────────────────────────────
    #[test]
    fn east_geometry_gates() {
        assert!(east_row_len_ok());
        assert!(east_tables_ok());
        assert!(east_shift_distinct_ok());
        assert!(east_reverse_ok());
    }

    #[test]
    fn each_east_face_has_forty_six_distinct_slots() {
        let mut i = 0usize;
        while i < ALL_EAST.len() {
            let id = match ALL_EAST.get(i) {
                Some(v) => *v,
                None => panic!("缺布局"),
            };
            let mut seen: Vec<char> = Vec::new();
            let mut s = 0usize;
            while s < CELLS {
                let c = match id.base_table().get(s) {
                    Some(v) => *v,
                    None => panic!("缺格"),
                };
                // 跨行撞码会让反查歧义，逐格入账前先验重。
                assert!(!seen.contains(&c));
                seen.push(c);
                s += 1;
            }
            assert_eq!(seen.len(), CELLS);
            i += 1;
        }
    }

    #[test]
    fn row_len_and_start_helpers_clamp_out_of_window() {
        assert_eq!(row_len_or_zero(0), 13);
        assert_eq!(row_len_or_zero(1), 12);
        assert_eq!(row_len_or_zero(2), 11);
        assert_eq!(row_len_or_zero(3), 10);
        assert_eq!(row_len_or_zero(4), 0);
        assert_eq!(row_start_or_zero(0), 0);
        assert_eq!(row_start_or_zero(1), 13);
        assert_eq!(row_start_or_zero(2), 25);
        assert_eq!(row_start_or_zero(3), 36);
        assert_eq!(row_start_or_zero(9), 0);
    }

    #[test]
    fn table_row_slices_each_row() {
        let t = EastId::Russian.base_table();
        assert_eq!(table_row(t, 0).map(|v| v.len()), Some(13));
        assert_eq!(table_row(t, 1).map(|v| v.len()), Some(12));
        assert_eq!(table_row(t, 2).map(|v| v.len()), Some(11));
        assert_eq!(table_row(t, 3).map(|v| v.len()), Some(10));
        assert_eq!(table_row(t, 4), None);
        if let Some(row) = table_row(t, 1) {
            assert_eq!(row.first().copied(), Some('й'));
            assert_eq!(row.last().copied(), Some('ъ'));
        }
    }

    #[test]
    fn row_unique_accepts_distinct_and_rejects_repeat() {
        assert!(row_unique(&[]));
        assert!(row_unique(&['가']));
        assert!(row_unique(&['ㄱ', 'ㅏ', 'ㄴ']));
        assert!(!row_unique(&['ㄱ', 'ㄱ']));
        assert!(!row_unique(&['ㄱ', 'ㅏ', 'ㄱ']));
    }

    #[test]
    fn jamo_window_covers_declared_range() {
        assert!(!jamo_window_ok(char::from_u32(0x3130).unwrap_or('a')));
        assert!(jamo_window_ok('ㄱ'));
        assert!(jamo_window_ok('ㅣ'));
        // 窗口上界 U+3163 入账，U+3164 填补符与更晚的古字一律出局。
        assert!(jamo_window_ok(char::from_u32(0x3163).unwrap_or('ㅣ')));
        assert!(!jamo_window_ok(char::from_u32(0x3165).unwrap_or('ㅡ')));
        assert!(!jamo_window_ok('한'));
        assert!(!jamo_window_ok('a'));
        assert!(!jamo_window_ok('ㅤ'));
        assert_eq!(jamo_window_ok(char::from_u32(0x3164).unwrap_or('a')), false);
    }

    #[test]
    fn non_alpha_classifies_punctuation_only() {
        assert!(non_alpha(','));
        assert!(non_alpha('1'));
        assert!(non_alpha('₩'));
        assert!(non_alpha('№'));
        assert!(!non_alpha('q'));
        assert!(!non_alpha('ㄱ'));
        assert!(!non_alpha('й'));
    }

    #[test]
    fn cyrillic_inventory_is_complete() {
        assert!(cyrillic_inventory_ok());
        assert_eq!(CYRILLIC_LETTERS.len(), CYRILLIC);
        let mut seen: Vec<char> = Vec::new();
        let mut i = 0usize;
        while i < CYRILLIC_LETTERS.len() {
            let c = match CYRILLIC_LETTERS.get(i) {
                Some(v) => *v,
                None => panic!("缺字母"),
            };
            assert!(!seen.contains(&c));
            seen.push(c);
            assert!(c.is_alphabetic());
            assert!(east_code_slot(EastId::Russian, Layer::Base, c).is_some());
            i += 1;
        }
        assert_eq!(seen.len(), 33);
        assert!(seen.contains(&'ё'));
        assert!(seen.contains(&'щ'));
        assert!(seen.contains(&'ы'));
    }

    #[test]
    fn cyrillic_yo_upper_is_booked_separately() {
        assert_eq!(CYRILLIC_YO_UPPER, 'Ё');
        assert_eq!(upper('ё'), CYRILLIC_YO_UPPER);
        // 数字行换档整行归符号，ё 的大写不落在 (0,0)。
        assert_eq!(EastId::Russian.key(Layer::Shift, 0, 0), Some('`'));
        assert_ne!(EastId::Russian.key(Layer::Shift, 0, 0), Some('Ё'));
    }

    #[test]
    fn uppercase_pair_gate_and_layer_readback() {
        assert!(uppercase_pair_ok());
        let mut si = 0usize;
        while si < ALL_EAST.len() {
            let id = match ALL_EAST.get(si) {
                Some(v) => *v,
                None => panic!("缺布局"),
            };
            let mut r = 1usize;
            while r < ROWS {
                let len = row_len_or_zero(r);
                let mut c = 0usize;
                while c < len {
                    let b = match id.key(Layer::Base, r, c) {
                        Some(v) => v,
                        None => panic!("缺格"),
                    };
                    let s = match id.key(Layer::Shift, r, c) {
                        Some(v) => v,
                        None => panic!("缺格"),
                    };
                    if b.is_alphabetic() {
                        assert_eq!(s, upper(b));
                    } else {
                        assert_eq!(s, b);
                    }
                    c += 1;
                }
                r += 1;
            }
            si += 1;
        }
    }

    #[test]
    fn diacritic_slots_are_registered_and_readable() {
        assert!(diacritic_slots_ok());
        assert_eq!(CS_DIACRITICS.len(), 6);
        assert_eq!(SK_DIACRITICS.len(), 7);
        assert_eq!(HU_DIACRITICS.len(), 7);
        let sets: [(&[(u8, u8, char)], EastId); 3] = [
            (&CS_DIACRITICS, EastId::Czech),
            (&SK_DIACRITICS, EastId::Slovak),
            (&HU_DIACRITICS, EastId::Hungarian),
        ];
        let mut si = 0usize;
        while si < sets.len() {
            let (tbl, id) = match sets.get(si) {
                Some(v) => *v,
                None => panic!("缺表"),
            };
            let mut k = 0usize;
            while k < tbl.len() {
                let (r, c, ch) = match tbl.get(k) {
                    Some(v) => *v,
                    None => panic!("缺槽"),
                };
                assert_eq!(id.key(Layer::Base, r as usize, c as usize), Some(ch));
                assert_eq!(east_code_slot(id, Layer::Base, ch), Some((r as usize, c as usize)));
                k += 1;
            }
            si += 1;
        }
    }

    #[test]
    fn east_reverse_lookup_covers_every_cell() {
        let mut si = 0usize;
        while si < ALL_EAST.len() {
            let id = match ALL_EAST.get(si) {
                Some(v) => *v,
                None => panic!("缺布局"),
            };
            let mut r = 0usize;
            while r < ROWS {
                let len = row_len_or_zero(r);
                let start = row_start_or_zero(r);
                let mut c = 0usize;
                while c < len {
                    let b = match id.base_table().get(start + c) {
                        Some(v) => *v,
                        None => panic!("缺格"),
                    };
                    assert_eq!(east_code_slot(id, Layer::Base, b), Some((r, c)));
                    if r > 0 && b.is_alphabetic() {
                        assert_eq!(
                            east_code_slot(id, Layer::Shift, upper(b)),
                            Some((r, c))
                        );
                    }
                    c += 1;
                }
                r += 1;
            }
            si += 1;
        }
    }

    #[test]
    fn east_code_slot_rejects_unlisted_code() {
        assert_eq!(east_code_slot(EastId::Russian, Layer::Base, 'ä'), None);
        assert_eq!(east_code_slot(EastId::Czech, Layer::Base, 'ŕ'), None);
        assert_eq!(east_code_slot(EastId::Hungarian, Layer::Base, 'ㄱ'), None);
        assert_eq!(east_code_slot(EastId::Slovak, Layer::Shift, 'Ď'), None);
    }

    #[test]
    fn east_golden_gate_and_ids() {
        assert!(golden_east_ok());
        assert_eq!(east_gold_total_hits(), GOLD_EAST);
        let mut i = 0usize;
        while i < EAST_GOLD.len() {
            let g = match EAST_GOLD.get(i) {
                Some(v) => v,
                None => panic!("缺条"),
            };
            assert_eq!(g.id as usize, 21 + i);
            assert!(!g.note.is_empty());
            assert!(east_gold_check(g));
            assert!(g.layout.golden_count() > 0);
            i += 1;
        }
    }

    #[test]
    fn east_golden_per_layout_hit_counts() {
        let mut acc = 0usize;
        let mut i = 0usize;
        while i < ALL_EAST.len() {
            let id = match ALL_EAST.get(i) {
                Some(v) => *v,
                None => panic!("缺布局"),
            };
            let mut listed = 0usize;
            let mut k = 0usize;
            while k < EAST_GOLD.len() {
                if let Some(g) = EAST_GOLD.get(k) {
                    if g.layout == id {
                        listed += 1;
                    }
                }
                k += 1;
            }
            assert_eq!(listed, id.golden_count());
            assert_eq!(east_gold_hits(id), listed);
            acc += listed;
            i += 1;
        }
        assert_eq!(acc, GOLD_EAST);
    }

    #[test]
    fn east_golden_check_rejects_wrong_expect() {
        let mut g = match EAST_GOLD.first() {
            Some(v) => *v,
            None => panic!("缺条"),
        };
        g.expect = 'x';
        assert!(!east_gold_check(&g));
        g.expect = 'q';
        assert!(!east_gold_check(&g));
        g.note = "";
        assert!(!east_gold_check(&g));
    }

    // ── 韩文两态表 ───────────────────────────────────────────────
    #[test]
    fn state_index_tag_and_flip() {
        assert_eq!(State::Latin.index(), 0);
        assert_eq!(State::Hangul.index(), 1);
        assert_eq!(State::from_index(0), Some(State::Latin));
        assert_eq!(State::from_index(1), Some(State::Hangul));
        assert_eq!(State::from_index(2), None);
        assert_eq!(State::Latin.tag(), "latin");
        assert_eq!(State::Hangul.tag(), "hangul");
        assert_eq!(State::Latin.flipped(), State::Hangul);
        assert_eq!(State::Hangul.flipped(), State::Latin);
        assert_eq!(State::Hangul.flipped().flipped(), State::Hangul);
    }

    #[test]
    fn kor_tables_are_forty_six_cells() {
        assert_eq!(KR_LATIN_BASE.len(), CELLS);
        assert_eq!(KR_LATIN_SHIFT.len(), CELLS);
        assert_eq!(KR_HANGUL_BASE.len(), CELLS);
        assert_eq!(KR_HANGUL_SHIFT.len(), CELLS);
        assert!(kor_tables_ok());
        let mut si = 0usize;
        while si < ALL_STATES.len() {
            let st = match ALL_STATES.get(si) {
                Some(v) => *v,
                None => panic!("缺态"),
            };
            assert_eq!(kor_table(st, Layer::Base).len(), CELLS);
            assert_eq!(kor_table(st, Layer::Shift).len(), CELLS);
            si += 1;
        }
    }

    #[test]
    fn kor_key_window_bounds() {
        assert!(kor_key(State::Hangul, Layer::Base, ROWS, 0).is_none());
        assert!(kor_key(State::Latin, Layer::Base, 0, COLS).is_none());
        assert!(kor_key(State::Latin, Layer::Shift, 3, 10).is_none());
        assert!(kor_key(State::Hangul, Layer::Base, 3, 9).is_some());
        assert!(kor_key(State::Hangul, Layer::Shift, 2, 10).is_some());
        assert!(kor_key(State::Latin, Layer::Base, 0, 12).is_some());
        assert!(kor_key_bounds_ok());
    }

    #[test]
    fn kor_key_matches_flat_tables_everywhere() {
        let mut si = 0usize;
        while si < ALL_STATES.len() {
            let st = match ALL_STATES.get(si) {
                Some(v) => *v,
                None => panic!("缺态"),
            };
            let mut li = 0usize;
            while li < LAYERS_TOTAL {
                let ly = match Layer::from_index(li) {
                    Some(v) => v,
                    None => panic!("缺层"),
                };
                let mut r = 0usize;
                while r < ROWS {
                    let len = row_len_or_zero(r);
                    let start = row_start_or_zero(r);
                    let mut c = 0usize;
                    while c < len {
                        assert_eq!(
                            kor_key(st, ly, r, c),
                            kor_table(st, ly).get(start + c).copied()
                        );
                        c += 1;
                    }
                    r += 1;
                }
                li += 1;
            }
            si += 1;
        }
    }

    #[test]
    fn hangul_keyface_anchors() {
        assert_eq!(kor_key(State::Hangul, Layer::Base, 0, 0), Some('`'));
        assert_eq!(kor_key(State::Hangul, Layer::Base, 1, 0), Some('ㅂ'));
        assert_eq!(kor_key(State::Hangul, Layer::Base, 1, 3), Some('ㄱ'));
        assert_eq!(kor_key(State::Hangul, Layer::Base, 2, 2), Some('ㅇ'));
        assert_eq!(kor_key(State::Hangul, Layer::Base, 2, 7), Some('ㅏ'));
        assert_eq!(kor_key(State::Hangul, Layer::Base, 3, 0), Some('ㅠ'));
        assert_eq!(kor_key(State::Hangul, Layer::Base, 3, 6), Some('₩'));
        assert_eq!(kor_key(State::Hangul, Layer::Shift, 1, 0), Some('ㅃ'));
        assert_eq!(kor_key(State::Hangul, Layer::Shift, 1, 2), Some('ㄸ'));
        assert_eq!(kor_key(State::Hangul, Layer::Shift, 1, 4), Some('ㅆ'));
        assert_eq!(kor_key(State::Hangul, Layer::Shift, 1, 8), Some('ㅒ'));
        assert_eq!(kor_key(State::Hangul, Layer::Shift, 1, 9), Some('ㅖ'));
        assert_eq!(kor_key(State::Hangul, Layer::Shift, 3, 6), Some('₩'));
    }

    #[test]
    fn latin_keyface_anchors() {
        assert_eq!(kor_key(State::Latin, Layer::Base, 1, 0), Some('q'));
        assert_eq!(kor_key(State::Latin, Layer::Base, 3, 0), Some('z'));
        assert_eq!(kor_key(State::Latin, Layer::Shift, 1, 0), Some('Q'));
        assert_eq!(kor_key(State::Latin, Layer::Shift, 0, 0), Some('~'));
        assert_eq!(kor_key(State::Latin, Layer::Shift, 0, 3), Some('#'));
        assert_eq!(kor_key(State::Latin, Layer::Base, 0, 3), Some('3'));
    }

    #[test]
    fn number_row_is_shared_and_symbol_only() {
        assert!(kor_number_row_shared_ok());
        let mut i = 0usize;
        while i < SHIFT_COLS {
            let lb = match kor_key(State::Latin, Layer::Base, 0, i) {
                Some(v) => v,
                None => panic!("缺格"),
            };
            let hb = match kor_key(State::Hangul, Layer::Base, 0, i) {
                Some(v) => v,
                None => panic!("缺格"),
            };
            let ls = match kor_key(State::Latin, Layer::Shift, 0, i) {
                Some(v) => v,
                None => panic!("缺格"),
            };
            let hs = match kor_key(State::Hangul, Layer::Shift, 0, i) {
                Some(v) => v,
                None => panic!("缺格"),
            };
            assert_eq!(lb, hb);
            assert_eq!(ls, hs);
            assert_ne!(lb, ls);
            assert!(!jamo_window_ok(lb));
            assert!(!ls.is_alphabetic());
            i += 1;
        }
    }

    #[test]
    fn hangul_shift_invariant_is_seven_tense_slots() {
        assert!(kor_shift_invariant_ok());
        let b = match table_row(kor_table(State::Hangul, Layer::Base), 1) {
            Some(v) => v,
            None => panic!("缺行"),
        };
        let s = match table_row(kor_table(State::Hangul, Layer::Shift), 1) {
            Some(v) => v,
            None => panic!("缺行"),
        };
        assert_eq!(b.len(), 12);
        assert_eq!(s.len(), 12);
        let mut diff: Vec<usize> = Vec::new();
        let mut i = 0usize;
        while i < b.len() {
            if b.get(i) != s.get(i) {
                diff.push(i);
            }
            i += 1;
        }
        assert_eq!(diff.len(), 7);
        assert_eq!(diff, vec![0usize, 1, 2, 3, 4, 8, 9]);
        // 中/下两行逐格相等
        let mut r = 2usize;
        while r < ROWS {
            assert_eq!(
                table_row(kor_table(State::Hangul, Layer::Base), r),
                table_row(kor_table(State::Hangul, Layer::Shift), r)
            );
            r += 1;
        }
    }

    #[test]
    fn latin_shift_case_gate() {
        assert!(latin_shift_case_ok());
        assert!(kor_shift_invariant_ok());
    }

    #[test]
    fn kor_slot_hits_is_unique_per_face() {
        assert!(kor_reverse_ok());
        let mut si = 0usize;
        while si < ALL_STATES.len() {
            let st = match ALL_STATES.get(si) {
                Some(v) => *v,
                None => panic!("缺态"),
            };
            let mut li = 0usize;
            while li < LAYERS_TOTAL {
                let ly = match Layer::from_index(li) {
                    Some(v) => v,
                    None => panic!("缺层"),
                };
                let mut r = 0usize;
                while r < ROWS {
                    let len = row_len_or_zero(r);
                    let mut c = 0usize;
                    while c < len {
                        let ch = match kor_key(st, ly, r, c) {
                            Some(v) => v,
                            None => panic!("缺格"),
                        };
                        assert_eq!(kor_slot_hits(st, ly, ch), 1);
                        assert_eq!(kor_code_slot(st, ly, ch), Some((r, c)));
                        c += 1;
                    }
                    r += 1;
                }
                li += 1;
            }
            si += 1;
        }
        assert_eq!(kor_code_slot(State::Hangul, Layer::Base, 'ä'), None);
        assert_eq!(kor_slot_hits(State::Hangul, Layer::Base, 'ä'), 0);
    }

    // ── 母字索引表 ───────────────────────────────────────────────
    #[test]
    fn jamo_tables_gate_and_sizes() {
        assert!(jamo_tables_ok());
        assert_eq!(LEAD_JAMO.len(), LEADS);
        assert_eq!(VOWEL_JAMO.len(), VOWELS);
        assert_eq!(TAIL_JAMO.len(), TAILS);
        assert_eq!(ATOMIC_LEADS.len(), LEADS);
        assert_eq!(ATOMIC_VOWELS.len(), 14);
        assert_eq!(ATOMIC_TAILS.len(), 16);
        assert_eq!(COMPOSITE_VOWELS.len(), VOWEL_MERGE);
        assert_eq!(COMPOSITE_TAILS.len(), TAIL_MERGE);
        assert_eq!(ATOMIC_VOWELS.len() + COMPOSITE_VOWELS.len(), VOWELS);
        assert_eq!(ATOMIC_TAILS.len() + COMPOSITE_TAILS.len(), TAILS);
    }

    #[test]
    fn jamo_tables_are_unique_and_in_window() {
        assert!(row_unique(&LEAD_JAMO.to_vec()));
        assert!(row_unique(&VOWEL_JAMO.to_vec()));
        assert!(row_unique(&TAIL_JAMO.to_vec()));
        let mut i = 0usize;
        while i < LEAD_JAMO.len() {
            if let Some(c) = LEAD_JAMO.get(i) {
                assert!(jamo_window_ok(*c));
                assert_eq!(lead_index(*c), Some(i));
                assert_eq!(jamo_at(JamoKind::Lead, i), Some(*c));
            }
            i += 1;
        }
        let mut j = 0usize;
        while j < VOWEL_JAMO.len() {
            if let Some(c) = VOWEL_JAMO.get(j) {
                assert!(jamo_window_ok(*c));
                assert_eq!(vowel_index(*c), Some(j));
                assert_eq!(jamo_at(JamoKind::Vowel, j), Some(*c));
            }
            j += 1;
        }
        let mut k = 0usize;
        while k < TAIL_JAMO.len() {
            if let Some(c) = TAIL_JAMO.get(k) {
                assert!(jamo_window_ok(*c));
                assert_eq!(tail_index(*c), Some(k + 1));
                assert_eq!(jamo_at(JamoKind::Tail, k + 1), Some(*c));
            }
            k += 1;
        }
    }

    #[test]
    fn index_lookups_reject_foreign_codes() {
        assert_eq!(lead_index('ㅏ'), None);
        assert_eq!(vowel_index('ㄱ'), None);
        assert_eq!(tail_index('ㅣ'), None);
        assert_eq!(lead_index('a'), None);
        assert_eq!(vowel_index('ㅤ'), None);
        assert_eq!(tail_index('한'), None);
    }

    #[test]
    fn jamo_at_window_edges() {
        assert_eq!(jamo_at(JamoKind::Lead, 0), Some('ㄱ'));
        assert_eq!(jamo_at(JamoKind::Lead, LEADS - 1), Some('ㅎ'));
        assert_eq!(jamo_at(JamoKind::Lead, LEADS), None);
        assert_eq!(jamo_at(JamoKind::Vowel, 0), Some('ㅏ'));
        assert_eq!(jamo_at(JamoKind::Vowel, VOWELS - 1), Some('ㅣ'));
        assert_eq!(jamo_at(JamoKind::Vowel, VOWELS), None);
        assert_eq!(jamo_at(JamoKind::Tail, 0), None);
        assert_eq!(jamo_at(JamoKind::Tail, 1), Some('ㄱ'));
        assert_eq!(jamo_at(JamoKind::Tail, TAILS), Some('ㅎ'));
        assert_eq!(jamo_at(JamoKind::Tail, TAIL_SLOTS), None);
    }

    #[test]
    fn tail_slots_are_shifted_by_one() {
        assert_eq!(tail_index('ㄱ'), Some(1));
        assert_eq!(tail_index('ㄲ'), Some(2));
        assert_eq!(tail_index('ㄴ'), Some(4));
        assert_eq!(tail_index('ㄻ'), Some(10));
        assert_eq!(tail_index('ㅁ'), Some(16));
        assert_eq!(tail_index('ㅂ'), Some(17));
        assert_eq!(tail_index('ㅄ'), Some(18));
        assert_eq!(tail_index('ㅇ'), Some(21));
        assert_eq!(tail_index('ㅎ'), Some(27));
        assert_eq!(tail_index('ㅏ'), None);
    }

    #[test]
    fn jamo_class_prefers_lead_and_covers_tail() {
        assert_eq!(jamo_class('ㄱ'), Some(JamoKind::Lead));
        assert_eq!(jamo_class('ㅏ'), Some(JamoKind::Vowel));
        assert_eq!(jamo_class('ㅄ'), Some(JamoKind::Tail));
        assert_eq!(jamo_class('ㄻ'), Some(JamoKind::Tail));
        assert_eq!(jamo_class('a'), None);
        assert_eq!(jamo_class('한'), None);
        assert_eq!(jamo_class('₩'), None);
    }

    #[test]
    fn compose_formula_anchors() {
        assert_eq!(compose_syllable(0, 0, 0), Some('가'));
        assert_eq!(compose_syllable(11, 0, 0), Some('아'));
        assert_eq!(compose_syllable(18, 0, 4), Some('한'));
        assert_eq!(compose_syllable(18, 17, 0), Some('휴'));
        assert_eq!(compose_syllable(0, 6, 0), Some('겨'));
        assert_eq!(compose_syllable(5, 0, 16), Some('람'));
        assert_eq!(compose_syllable(14, 20, 17), Some('칩'));
        assert_eq!(compose_syllable(18, 20, 27), Some('힣'));
        assert_eq!(compose_syllable(3, 11, 4), Some('된'));
        assert_eq!(compose_syllable(11, 4, 18), Some('없'));
    }

    #[test]
    fn compose_rejects_out_of_range_indices() {
        assert_eq!(compose_syllable(LEADS, 0, 0), None);
        assert_eq!(compose_syllable(0, VOWELS, 0), None);
        assert_eq!(compose_syllable(0, 0, TAIL_SLOTS), None);
        assert_eq!(compose_syllable(usize::MAX, 0, 0), None);
        assert_eq!(compose_syllable(0, 0, usize::MAX), None);
    }

    #[test]
    fn decompose_rejects_non_syllable_codes() {
        assert_eq!(decompose_syllable('a'), None);
        assert_eq!(decompose_syllable('ㄱ'), None);
        assert_eq!(decompose_syllable('한'), Some((18, 0, 4)));
        assert_eq!(decompose_syllable('가'), Some((0, 0, 0)));
        assert_eq!(decompose_syllable('힣'), Some((18, 20, 27)));
    }

    #[test]
    fn compose_identity_holds_over_whole_block() {
        assert!(compose_identity_ok());
        let mut probe = 0usize;
        let mut code = SYL_START;
        while code <= SYL_LAST && probe < 200 {
            let c = match char::from_u32(code) {
                Some(v) => v,
                None => panic!("非法码"),
            };
            let (l, v, t) = match decompose_syllable(c) {
                Some(x) => x,
                None => panic!("回推失败"),
            };
            assert_eq!(compose_syllable(l, v, t), Some(c));
            code += 53;
            probe += 1;
        }
        assert_eq!(probe, 200);
    }

    #[test]
    fn compose_bounds_gate_and_edges() {
        assert!(compose_bounds_ok());
        assert_eq!(decompose_syllable(char::from_u32(SYL_START - 1).unwrap_or('a')), None);
        assert_eq!(compose_syllable(0, 0, 0), Some('가'));
    }

    // ── 合成对 ───────────────────────────────────────────────────
    #[test]
    fn merge_lead_pairs_read_back() {
        assert_eq!(merge_lead('ㄱ', 'ㄱ'), Some('ㄲ'));
        assert_eq!(merge_lead('ㅈ', 'ㅈ'), Some('ㅉ'));
        assert_eq!(merge_lead('ㄷ', 'ㄷ'), Some('ㄸ'));
        assert_eq!(merge_lead('ㅂ', 'ㅂ'), Some('ㅃ'));
        assert_eq!(merge_lead('ㅅ', 'ㅅ'), Some('ㅆ'));
        assert_eq!(merge_lead('ㄱ', 'ㄴ'), None);
        assert_eq!(merge_lead('ㅎ', 'ㅎ'), None);
    }

    #[test]
    fn merge_vowel_pairs_read_back_and_are_directional() {
        assert_eq!(merge_vowel('ㅗ', 'ㅏ'), Some('ㅘ'));
        assert_eq!(merge_vowel('ㅗ', 'ㅐ'), Some('ㅙ'));
        assert_eq!(merge_vowel('ㅗ', 'ㅣ'), Some('ㅚ'));
        assert_eq!(merge_vowel('ㅜ', 'ㅓ'), Some('ㅝ'));
        assert_eq!(merge_vowel('ㅜ', 'ㅔ'), Some('ㅞ'));
        assert_eq!(merge_vowel('ㅜ', 'ㅣ'), Some('ㅟ'));
        assert_eq!(merge_vowel('ㅡ', 'ㅣ'), Some('ㅢ'));
        assert_eq!(merge_vowel('ㅏ', 'ㅗ'), None);
        assert_eq!(merge_vowel('ㅣ', 'ㅜ'), None);
        assert_eq!(merge_vowel('ㅏ', 'ㅣ'), None);
    }

    #[test]
    fn merge_tail_pairs_read_back_and_are_directional() {
        assert_eq!(merge_tail('ㄱ', 'ㅅ'), Some('ㄳ'));
        assert_eq!(merge_tail('ㄴ', 'ㅈ'), Some('ㄵ'));
        assert_eq!(merge_tail('ㄴ', 'ㅎ'), Some('ㄶ'));
        assert_eq!(merge_tail('ㄹ', 'ㄱ'), Some('ㄺ'));
        assert_eq!(merge_tail('ㄹ', 'ㅁ'), Some('ㄻ'));
        assert_eq!(merge_tail('ㄹ', 'ㅂ'), Some('ㄼ'));
        assert_eq!(merge_tail('ㄹ', 'ㅅ'), Some('ㄽ'));
        assert_eq!(merge_tail('ㄹ', 'ㅌ'), Some('ㄾ'));
        assert_eq!(merge_tail('ㄹ', 'ㅍ'), Some('ㄿ'));
        assert_eq!(merge_tail('ㄹ', 'ㅎ'), Some('ㅀ'));
        assert_eq!(merge_tail('ㅂ', 'ㅅ'), Some('ㅄ'));
        assert_eq!(merge_tail('ㅅ', 'ㄱ'), None);
        assert_eq!(merge_tail('ㄱ', 'ㄴ'), None);
    }

    #[test]
    fn merge_pair_on_empty_slice_is_none() {
        let empty: [(char, char, char); 0] = [];
        assert_eq!(merge_pair(&empty, 'ㄱ', 'ㄱ'), None);
        assert_eq!(merge_pair(&LEAD_MERGE_PAIRS, 'ㄱ', 'ㅏ'), None);
    }

    #[test]
    fn merge_pair_results_land_in_composite_lists() {
        assert!(merge_pairs_ok());
        let mut i = 0usize;
        while i < VOWEL_MERGE_PAIRS.len() {
            if let Some((_, _, z)) = VOWEL_MERGE_PAIRS.get(i) {
                assert!(COMPOSITE_VOWELS.contains(z));
                assert!(vowel_index(*z).is_some());
            }
            i += 1;
        }
        let mut k = 0usize;
        while k < TAIL_MERGE_PAIRS.len() {
            if let Some((_, _, z)) = TAIL_MERGE_PAIRS.get(k) {
                assert!(COMPOSITE_TAILS.contains(z));
                assert!(tail_index(*z).is_some());
            }
            k += 1;
        }
        let mut m = 0usize;
        while m < LEAD_MERGE_PAIRS.len() {
            if let Some((a, b, z)) = LEAD_MERGE_PAIRS.get(m) {
                assert_eq!(a, b);
                assert!(lead_index(*z).is_some());
                assert_ne!(a, z);
            }
            m += 1;
        }
    }

    #[test]
    fn atomic_and_composite_sets_are_disjoint() {
        assert!(atomic_jamo_reachable_ok());
        assert!(composite_not_keyable_ok());
        let mut i = 0usize;
        while i < COMPOSITE_VOWELS.len() {
            if let Some(c) = COMPOSITE_VOWELS.get(i) {
                assert!(!ATOMIC_VOWELS.contains(c));
            }
            i += 1;
        }
        let mut k = 0usize;
        while k < COMPOSITE_TAILS.len() {
            if let Some(c) = COMPOSITE_TAILS.get(k) {
                assert!(!ATOMIC_TAILS.contains(c));
                assert!(tail_index(*c).is_some());
            }
            k += 1;
        }
        for c in ATOMIC_VOWELS.iter() {
            assert!(vowel_index(*c).is_some());
        }
        for c in ATOMIC_TAILS.iter() {
            assert!(tail_index(*c).is_some());
        }
    }

    // ── 组合引擎 ─────────────────────────────────────────────────
    #[test]
    fn builder_starts_empty_and_default_matches_new() {
        let b = SyllableBuilder::new();
        assert!(b.is_empty());
        assert_eq!(b.lead, None);
        assert_eq!(b.vowel, None);
        assert_eq!(b.tail, None);
        assert_eq!(SyllableBuilder::default(), b);
    }

    #[test]
    fn builder_finish_on_empty_pushes_nothing() {
        let mut b = SyllableBuilder::new();
        let mut out = String::new();
        b.finish(&mut out);
        assert!(out.is_empty());
        assert!(b.is_empty());
    }

    #[test]
    fn builder_finish_lead_only_emits_raw_jamo() {
        let mut b = SyllableBuilder::new();
        b.lead = Some('ㄱ');
        let mut out = String::new();
        b.finish(&mut out);
        assert_eq!(out, "ㄱ");
        assert!(b.is_empty());
    }

    #[test]
    fn builder_finish_vowel_only_emits_raw_vowel() {
        let mut b = SyllableBuilder::new();
        b.vowel = Some('ㅏ');
        let mut out = String::new();
        b.finish(&mut out);
        // 直接 finish 不补零声母——补位是 push_jamo 的语义。
        assert_eq!(out, "ㅏ");
        assert!(b.is_empty());
    }

    #[test]
    fn builder_finish_lead_and_tail_without_vowel_keeps_both() {
        let mut b = SyllableBuilder::new();
        b.lead = Some('ㄱ');
        b.tail = Some('ㄴ');
        let mut out = String::new();
        b.finish(&mut out);
        assert_eq!(out, "ㄱㄴ");
    }

    #[test]
    fn builder_finish_full_triple_uses_formula() {
        let mut b = SyllableBuilder::new();
        b.lead = Some('ㅎ');
        b.vowel = Some('ㅏ');
        b.tail = Some('ㄴ');
        let mut out = String::new();
        b.finish(&mut out);
        assert_eq!(out, "한");
        assert!(b.is_empty());
    }

    #[test]
    fn builder_finish_unindexable_tail_falls_back_to_raw() {
        let mut b = SyllableBuilder::new();
        b.lead = Some('ㄱ');
        b.vowel = Some('ㅏ');
        b.tail = Some('x');
        let mut out = String::new();
        b.finish(&mut out);
        assert_eq!(out, "ㄱㅏx");
        assert!(b.is_empty());
    }

    #[test]
    fn builder_finish_unindexable_lead_or_vowel_falls_back() {
        let mut b = SyllableBuilder::new();
        b.lead = Some('x');
        b.vowel = Some('ㅏ');
        let mut out = String::new();
        b.finish(&mut out);
        assert_eq!(out, "x");

        let mut c = SyllableBuilder::new();
        c.lead = Some('ㄱ');
        c.vowel = Some('x');
        let mut out2 = String::new();
        c.finish(&mut out2);
        assert_eq!(out2, "ㄱx");
    }

    #[test]
    fn builder_reset_clears_every_slot() {
        let mut b = SyllableBuilder::new();
        b.lead = Some('ㄱ');
        b.vowel = Some('ㅏ');
        b.tail = Some('ㄴ');
        b.reset();
        assert!(b.is_empty());
        assert_eq!(b.lead, None);
        assert_eq!(b.vowel, None);
        assert_eq!(b.tail, None);
    }

    #[test]
    fn push_jamo_inserts_zero_lead_for_bare_vowel() {
        let mut b = SyllableBuilder::new();
        let mut out = String::new();
        push_jamo(&mut b, 'ㅏ', &mut out);
        assert_eq!(b.lead, Some('ㅇ'));
        assert_eq!(b.vowel, Some('ㅏ'));
        assert_eq!(b.tail, None);
        assert!(out.is_empty());
        b.finish(&mut out);
        assert_eq!(out, "아");
    }

    #[test]
    fn push_jamo_merges_tense_lead() {
        let mut b = SyllableBuilder::new();
        let mut out = String::new();
        push_jamo(&mut b, 'ㄱ', &mut out);
        push_jamo(&mut b, 'ㄱ', &mut out);
        assert_eq!(b.lead, Some('ㄲ'));
        assert!(out.is_empty());
        push_jamo(&mut b, 'ㅏ', &mut out);
        b.finish(&mut out);
        assert_eq!(out, "까");
    }

    #[test]
    fn push_jamo_non_mergeable_second_consonant_starts_new_syllable() {
        let mut b = SyllableBuilder::new();
        let mut out = String::new();
        push_jamo(&mut b, 'ㄱ', &mut out);
        push_jamo(&mut b, 'ㄴ', &mut out);
        assert_eq!(b.lead, Some('ㄴ'));
        assert_eq!(out, "ㄱ");
    }

    #[test]
    fn push_jamo_consonant_after_vowel_takes_tail_slot() {
        let mut b = SyllableBuilder::new();
        let mut out = String::new();
        push_jamo(&mut b, 'ㅎ', &mut out);
        push_jamo(&mut b, 'ㅏ', &mut out);
        push_jamo(&mut b, 'ㄴ', &mut out);
        assert_eq!(b.tail, Some('ㄴ'));
        b.finish(&mut out);
        assert_eq!(out, "한");
    }

    #[test]
    fn push_jamo_merges_double_tail() {
        let mut b = SyllableBuilder::new();
        let mut out = String::new();
        push_jamo(&mut b, 'ㅇ', &mut out);
        push_jamo(&mut b, 'ㅓ', &mut out);
        push_jamo(&mut b, 'ㅂ', &mut out);
        push_jamo(&mut b, 'ㅅ', &mut out);
        assert_eq!(b.tail, Some('ㅄ'));
        b.finish(&mut out);
        assert_eq!(out, "없");
    }

    #[test]
    fn push_jamo_vowel_after_tail_starts_next_syllable() {
        let mut b = SyllableBuilder::new();
        let mut out = String::new();
        push_jamo(&mut b, 'ㄱ', &mut out);
        push_jamo(&mut b, 'ㅏ', &mut out);
        push_jamo(&mut b, 'ㄴ', &mut out);
        push_jamo(&mut b, 'ㅏ', &mut out);
        assert_eq!(out, "간");
        assert_eq!(b.lead, Some('ㅇ'));
        assert_eq!(b.vowel, Some('ㅏ'));
        b.finish(&mut out);
        assert_eq!(out, "간아");
    }

    #[test]
    fn push_jamo_second_vowel_merges_or_restarts() {
        let mut b = SyllableBuilder::new();
        let mut out = String::new();
        push_jamo(&mut b, 'ㅂ', &mut out);
        push_jamo(&mut b, 'ㅗ', &mut out);
        push_jamo(&mut b, 'ㅏ', &mut out);
        assert_eq!(b.vowel, Some('ㅘ'));
        b.finish(&mut out);
        assert_eq!(out, "봐");

        let mut c = SyllableBuilder::new();
        let mut out2 = String::new();
        push_jamo(&mut c, 'ㅂ', &mut out2);
        push_jamo(&mut c, 'ㅏ', &mut out2);
        push_jamo(&mut c, 'ㅗ', &mut out2);
        // ㅏ+ㅗ 不是登记合成对，前一音节先结算，再另起零声母音节。
        assert_eq!(out2, "바");
        assert_eq!(c.vowel, Some('ㅗ'));
        c.finish(&mut out2);
        assert_eq!(out2, "바오");
    }

    #[test]
    fn compose_stream_edges() {
        assert!(compose_stream_edge_ok());
        assert_eq!(compose_stream(""), "");
        assert_eq!(compose_stream("a"), "a");
        assert_eq!(compose_stream("ㅏ"), "아");
        assert_eq!(compose_stream("ㄱ"), "ㄱ");
        assert_eq!(compose_stream("ㅎㅏㄴㄱㅜㄱ"), "한국");
        assert_eq!(compose_stream("ㄱㅏㄴㅏ"), "간아");
        assert_eq!(compose_stream("ㅂㅏㄱㅏ"), "박아");
        assert_eq!(compose_stream("ㄱㅏㅂㅏ"), "갑아");
    }

    #[test]
    fn compose_stream_passes_through_precomposed_text() {
        // 已组合的音节不是母字，按原样落账。
        assert_eq!(compose_stream("한"), "한");
        assert_eq!(compose_stream("한국"), "한국");
        assert_eq!(compose_stream("₩"), "₩");
        assert_eq!(compose_stream("가나다"), "가나다");
    }

    #[test]
    fn compose_stream_mixed_runs() {
        assert_eq!(compose_stream("ㄱㅏ"), "가");
        assert_eq!(compose_stream("ㄱㅕ"), "겨");
        assert_eq!(compose_stream("ㅈㅓㅁ"), "점");
        assert_eq!(compose_stream("ㅇㅏㄹㅁ"), "앎");
        assert_eq!(compose_stream("ㅇㅟ"), "위");
        assert_eq!(compose_stream("ㅇㅜㅣ"), "위");
        assert_eq!(compose_stream("ㄷㅚ"), "되");
        assert_eq!(compose_stream("ㄷㅚㄴ"), "된");
        assert_eq!(compose_stream("ㅊㅣㅂ"), "칩");
        assert_eq!(compose_stream("ㄹㅏㅁ"), "람");
        assert_eq!(compose_stream("ㅌㅏ"), "타");
        assert_eq!(compose_stream("ㅍㅏ"), "파");
        assert_eq!(compose_stream("ㅅㅣ"), "시");
        assert_eq!(compose_stream("ㅇㅓ"), "어");
        assert_eq!(compose_stream("ㄱㅓ"), "거");
        assert_eq!(compose_stream("ㄴㅏ"), "나");
    }

    // ── 韩文黄金组 ───────────────────────────────────────────────
    #[test]
    fn kor_golden_gate_and_hits() {
        assert!(golden_kor_ok());
        assert_eq!(kor_gold_hits(), GOLD_KOR);
        let mut i = 0usize;
        while i < KOR_GOLD.len() {
            let g = match KOR_GOLD.get(i) {
                Some(v) => v,
                None => panic!("缺条"),
            };
            assert_eq!(g.id as usize, i + 1);
            assert!(!g.seq.is_empty());
            assert!(!g.expect.is_empty());
            assert!(!g.note.is_empty());
            assert!(kor_gold_check(g));
            i += 1;
        }
    }

    #[test]
    fn kor_golden_notes_are_pairwise_distinct() {
        let mut i = 0usize;
        while i < KOR_GOLD.len() {
            let a = match KOR_GOLD.get(i) {
                Some(v) => v,
                None => panic!("缺条"),
            };
            let mut j = i + 1;
            while j < KOR_GOLD.len() {
                if let Some(b) = KOR_GOLD.get(j) {
                    assert_ne!(a.note, b.note);
                }
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn kor_golden_inputs_are_all_jamo() {
        let mut i = 0usize;
        while i < KOR_GOLD.len() {
            let g = match KOR_GOLD.get(i) {
                Some(v) => v,
                None => panic!("缺条"),
            };
            for c in g.seq.chars() {
                assert!(jamo_class(c).is_some());
            }
            for c in g.expect.chars() {
                assert!(decompose_syllable(c).is_some());
            }
            i += 1;
        }
    }

    #[test]
    fn kor_golden_check_rejects_bad_rows() {
        let mut g = match KOR_GOLD.first() {
            Some(v) => *v,
            None => panic!("缺条"),
        };
        assert!(kor_gold_check(&g));
        g.expect = "나";
        assert!(!kor_gold_check(&g));
        g.expect = "가";
        g.id = 0;
        assert!(!kor_gold_check(&g));
        g.id = GOLD_KOR as u16 + 1;
        assert!(!kor_gold_check(&g));
        g.id = 1;
        g.seq = "";
        assert!(!kor_gold_check(&g));
    }

    // ── 한/영 切换 ───────────────────────────────────────────────
    #[test]
    fn switch_starts_in_latin_and_counts_flips() {
        let sw = Switch::new();
        assert_eq!(sw.mode(), State::Latin);
        assert_eq!(sw.flips, 0);
        assert_eq!(sw.queued, 0);
        assert_eq!(sw.drops, 0);
        assert_eq!(sw.flushes, 0);
        assert_eq!(sw.pending(), 0);
        assert_eq!(sw.last_key(), None);
        assert_eq!(Switch::default().mode(), State::Latin);
    }

    #[test]
    fn switch_toggle_flips_both_ways() {
        let mut sw = Switch::with_mode(State::Latin);
        assert_eq!(sw.toggle(), State::Hangul);
        assert_eq!(sw.flips, 1);
        assert_eq!(sw.toggle(), State::Latin);
        assert_eq!(sw.flips, 2);
        let mut h = Switch::with_mode(State::Hangul);
        assert_eq!(h.toggle(), State::Latin);
        assert_eq!(h.toggle(), State::Hangul);
        assert_eq!(h.flips, 2);
    }

    #[test]
    fn switch_toggle_drains_pending_queue() {
        let mut sw = Switch::with_mode(State::Hangul);
        assert!(sw.press('ㄱ'));
        assert!(sw.press('ㅏ'));
        assert!(sw.press('ㄴ'));
        assert_eq!(sw.pending(), 3);
        assert_eq!(sw.last_key(), Some('ㄴ'));
        let before = sw.flushes;
        sw.toggle();
        assert_eq!(sw.pending(), 0);
        assert_eq!(sw.flushes, before + 1);
        assert_eq!(sw.queued, 3);
        assert_eq!(sw.drops, 0);
        assert!(sw.drain().is_empty());
    }

    #[test]
    fn switch_press_honours_capacity_and_books_drops() {
        let mut sw = Switch::new();
        let mut i = 0usize;
        while i < PENDING_CAP {
            let c = match LEAD_JAMO.get(i % LEADS) {
                Some(v) => *v,
                None => panic!("缺母字"),
            };
            assert!(sw.press(c));
            assert_eq!(sw.last_key(), Some(c));
            i += 1;
        }
        assert_eq!(sw.pending(), PENDING_CAP);
        assert_eq!(sw.drops, 0);
        assert!(!sw.press('ㅎ'));
        assert_eq!(sw.drops, 1);
        assert_eq!(sw.pending(), PENDING_CAP);
        assert!(!sw.press('ㅏ'));
        assert_eq!(sw.drops, 2);
        assert!(pending_capacity_ok());
    }

    #[test]
    fn switch_drain_returns_press_order_and_frees_slots() {
        let mut sw = Switch::new();
        for c in ['ㄱ', 'ㅏ', 'ㄴ', 'ㄱ', 'ㅕ'].iter() {
            assert!(sw.press(*c));
        }
        let got = sw.drain();
        assert_eq!(got, vec!['ㄱ', 'ㅏ', 'ㄴ', 'ㄱ', 'ㅕ']);
        assert_eq!(sw.pending(), 0);
        assert_eq!(sw.flushes, 1);
        assert!(sw.press('ㅏ'));
        assert_eq!(sw.pending(), 1);
    }

    #[test]
    fn switch_resolve_matches_kor_key() {
        assert!(switch_resolve_ok());
        let sw = Switch::with_mode(State::Hangul);
        assert_eq!(sw.resolve(Layer::Base, 1, 3), Some('ㄱ'));
        assert_eq!(sw.resolve(Layer::Shift, 1, 0), Some('ㅃ'));
        assert_eq!(sw.resolve(Layer::Base, 3, 6), Some('₩'));
        assert_eq!(sw.mode(), State::Hangul);
        let en = Switch::with_mode(State::Latin);
        assert_eq!(en.resolve(Layer::Shift, 1, 0), Some('Q'));
        assert_eq!(en.resolve(Layer::Base, 0, 0), Some('`'));
        assert_eq!(en.resolve(Layer::Base, ROWS, 0), None);
    }

    #[test]
    fn switch_cycle_gate() {
        assert!(switch_cycle_ok());
        assert!(han_key_slots_ok());
    }

    #[test]
    fn extra_key_slots_live_outside_the_face() {
        assert_eq!(KOR_EXTRA_KEYS.len(), 2);
        assert_eq!(HAN_YEONG_POS, 56);
        assert_eq!(HANJA_POS, 57);
        assert!((HAN_YEONG_POS as usize) >= CELLS);
        assert!((HANJA_POS as usize) >= CELLS);
        assert_eq!(kor_extra_tag(HAN_YEONG_POS), Some("han-yeong"));
        assert_eq!(kor_extra_tag(HANJA_POS), Some("hanja"));
        assert_eq!(kor_extra_tag(0), None);
        assert_eq!(kor_extra_tag(CELLS as u8 - 1), None);
        assert_eq!(kor_extra_tag(99), None);
        let mut i = 0usize;
        while i < CELLS {
            assert!(kor_extra_tag(i as u8).is_none());
            i += 1;
        }
    }

    // ── 键面账 ───────────────────────────────────────────────────
    #[test]
    fn face_index_and_tag_roundtrip() {
        let mut i = 0usize;
        while i < ALL_FACES.len() {
            let f = match ALL_FACES.get(i) {
                Some(v) => *v,
                None => panic!("缺面"),
            };
            assert_eq!(f.index(), i);
            assert_eq!(Face::from_index(i), Some(f));
            assert_eq!(Face::from_tag(f.tag()), Some(f));
            assert!(!f.tag().is_empty());
            i += 1;
        }
        assert_eq!(Face::from_index(KOR_FACES), None);
        assert_eq!(Face::from_tag("nope"), None);
        assert_eq!(Face::LatinBase.tag(), "latin-base");
        assert_eq!(Face::LatinShift.tag(), "latin-shift");
        assert_eq!(Face::HangulBase.tag(), "hangul-base");
        assert_eq!(Face::HangulShift.tag(), "hangul-shift");
        assert_eq!(Face::SharedRow.tag(), "shared-row");
    }

    #[test]
    fn face_state_layer_mapping() {
        assert_eq!(Face::LatinBase.state_layer(), Some((State::Latin, Layer::Base)));
        assert_eq!(Face::LatinShift.state_layer(), Some((State::Latin, Layer::Shift)));
        assert_eq!(Face::HangulBase.state_layer(), Some((State::Hangul, Layer::Base)));
        assert_eq!(Face::HangulShift.state_layer(), Some((State::Hangul, Layer::Shift)));
        assert_eq!(Face::SharedRow.state_layer(), None);
    }

    #[test]
    fn face_cells_and_slots_agree() {
        let mut i = 0usize;
        while i < ALL_FACES.len() {
            let f = match ALL_FACES.get(i) {
                Some(v) => *v,
                None => panic!("缺面"),
            };
            let want = if f == Face::SharedRow { SHIFT_COLS } else { CELLS };
            assert_eq!(f.cells(), want);
            let s = f.slots();
            assert_eq!(s.len(), want);
            assert_eq!(f.distinct(), want);
            i += 1;
        }
        assert!(face_cells_unique_ok());
    }

    #[test]
    fn face_slots_follow_the_keyface_tables() {
        let hb = Face::HangulBase.slots();
        assert_eq!(hb.first(), Some(&'`'));
        assert_eq!(hb.get(ROW_START[1]), Some(&'ㅂ'));
        assert_eq!(hb.get(ROW_START[3] + 6), Some(&'₩'));
        let ls = Face::LatinShift.slots();
        assert_eq!(ls.get(ROW_START[1]), Some(&'Q'));
        let shared = Face::SharedRow.slots();
        assert_eq!(shared.len(), 13);
        assert_eq!(shared.get(12), Some(&'='));
        assert_eq!(shared.get(13), None);
    }

    #[test]
    fn faces_are_pairwise_distinct_by_signature() {
        assert!(faces_distinct_ok());
        assert!(shared_row_face_ok());
        let mut i = 0usize;
        while i < ALL_FACES.len() {
            let a = match ALL_FACES.get(i) {
                Some(v) => *v,
                None => panic!("缺面"),
            };
            assert_eq!(a.sig_hex().len(), 16);
            let mut j = i + 1;
            while j < ALL_FACES.len() {
                if let Some(b) = ALL_FACES.get(j) {
                    assert_ne!(a.signature(), b.signature());
                    assert_ne!(a.tag(), b.tag());
                }
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn face_checksum_is_deterministic() {
        let mut i = 0usize;
        while i < ALL_FACES.len() {
            let f = match ALL_FACES.get(i) {
                Some(v) => *v,
                None => panic!("缺面"),
            };
            assert_eq!(f.checksum(), f.checksum());
            i += 1;
        }
    }

    // ── 摘要与解析 ───────────────────────────────────────────────
    #[test]
    fn east_digest_is_deterministic_and_input_sensitive() {
        let a = east_digest(EastId::Russian, 12, 12, 46);
        let b = east_digest(EastId::Russian, 12, 12, 46);
        assert_eq!(a, b);
        assert_ne!(a, east_digest(EastId::Russian, 12, 11, 46));
        assert_ne!(a, east_digest(EastId::Russian, 11, 12, 46));
        assert_ne!(a, east_digest(EastId::Russian, 12, 12, 45));
        assert_ne!(a, east_digest(EastId::Czech, 12, 12, 46));
        assert_ne!(a, p_core::FNV_OFF);
    }

    #[test]
    fn face_digest_is_deterministic_and_input_sensitive() {
        let a = face_digest(Face::HangulBase, 46, 46);
        assert_eq!(a, face_digest(Face::HangulBase, 46, 46));
        assert_ne!(a, face_digest(Face::HangulBase, 45, 46));
        assert_ne!(a, face_digest(Face::HangulBase, 46, 45));
        assert_ne!(a, face_digest(Face::HangulShift, 46, 46));
        assert_ne!(a, face_digest(Face::SharedRow, 13, 13));
    }

    #[test]
    fn parse_u32_accepts_digits_only() {
        assert_eq!(parse_u32("0"), Some(0));
        assert_eq!(parse_u32("46"), Some(46));
        assert_eq!(parse_u32("4294967295"), Some(u32::MAX));
        assert_eq!(parse_u32(""), None);
        assert_eq!(parse_u32("-1"), None);
        assert_eq!(parse_u32("1a"), None);
        assert_eq!(parse_u32(" 1"), None);
        assert_eq!(parse_u32("4294967296"), None);
        assert_eq!(parse_u32("99999999999"), None);
    }

    #[test]
    fn parse_hex16_needs_exactly_sixteen_digits() {
        assert_eq!(parse_hex16("0000000000000000"), Some(0));
        assert_eq!(parse_hex16("ffffffffffffffff"), Some(u64::MAX));
        assert_eq!(parse_hex16("FFFFFFFFFFFFFFFF"), Some(u64::MAX));
        assert_eq!(parse_hex16("000000000000000"), None);
        assert_eq!(parse_hex16("00000000000000000"), None);
        assert_eq!(parse_hex16("000000000000000g"), None);
        assert_eq!(parse_hex16(""), None);
        assert_eq!(parse_hex16("1234567890abcdef"), Some(0x1234567890abcdef));
    }

    #[test]
    fn row_fields_splits_declared_key_order() {
        let row = render_east_row(EastId::Russian);
        let f = match row_fields(&row, EAST_TAG, &EAST_KEYS) {
            Some(v) => v,
            None => panic!("拆行失败"),
        };
        assert_eq!(f.len(), EAST_KEYS.len() + 1);
        assert_eq!(f.get(0).copied(), Some("ytsuen"));
        assert_eq!(f.get(1).copied(), Some("12"));
        assert_eq!(f.get(3).copied(), Some("46"));
        assert_eq!(f.get(4).copied(), Some(EastId::Russian.sig_hex().as_str()));
        assert_eq!(f.get(5).map(|v| v.len()), Some(16));

        let krow = render_kor_row(Face::SharedRow);
        let kf = match row_fields(&krow, KOR_TAG, &KOR_KEYS) {
            Some(v) => v,
            None => panic!("拆行失败"),
        };
        assert_eq!(kf.len(), KOR_KEYS.len() + 1);
        assert_eq!(kf.get(0).copied(), Some("shared-row"));
        assert_eq!(kf.get(1).copied(), Some("13"));
        assert_eq!(kf.get(2).copied(), Some("13"));
    }

    #[test]
    fn row_fields_rejects_malformed_rows() {
        assert_eq!(row_fields("", EAST_TAG, &EAST_KEYS), None);
        assert_eq!(
            row_fields("EAST46|layout=ytsuen", EAST_TAG, &EAST_KEYS),
            None
        );
        assert_eq!(
            row_fields(
                "KOR2FACE|layout=ytsuen|golden=12|hit=12|cells=46|sig=0123456789abcdef|digest=0000000000000000",
                EAST_TAG,
                &EAST_KEYS
            ),
            None
        );
        assert_eq!(
            row_fields(
                "EAST46|lay=ytsuen|golden=12|hit=12|cells=46|sig=0123456789abcdef|digest=0000000000000000",
                EAST_TAG,
                &EAST_KEYS
            ),
            None
        );
        assert_eq!(
            row_fields(
                "EAST46|layout=|golden=12|hit=12|cells=46|sig=0123456789abcdef|digest=0000000000000000",
                EAST_TAG,
                &EAST_KEYS
            ),
            None
        );
        assert_eq!(
            row_fields(
                "EAST46|layout=ytsuen|golden=12|hit=12|cells=46|sig=0123456789abcdef|digest=000",
                EAST_TAG,
                &EAST_KEYS
            ),
            None
        );
        assert_eq!(
            row_fields(
                "EAST46|layout=ytsuen|golden=12|hit=12|cells=46|sig=0123456789abcdef|digest=0000000000000000|extra=1",
                EAST_TAG,
                &EAST_KEYS
            ),
            None
        );
    }

    #[test]
    fn render_rows_have_declared_shape() {
        let mut i = 0usize;
        while i < ALL_EAST.len() {
            let id = match ALL_EAST.get(i) {
                Some(v) => *v,
                None => panic!("缺布局"),
            };
            let row = render_east_row(id);
            assert!(row.starts_with("EAST46|layout="));
            assert!(row.contains("|golden="));
            assert!(row.contains("|hit="));
            assert!(row.contains("|cells=46|"));
            assert!(row.contains("|sig="));
            assert!(row.contains("|digest="));
            assert!(audit_east_row(&row));
            i += 1;
        }
        let mut k = 0usize;
        while k < ALL_FACES.len() {
            let f = match ALL_FACES.get(k) {
                Some(v) => *v,
                None => panic!("缺面"),
            };
            let row = render_kor_row(f);
            assert!(row.starts_with("KOR2FACE|face="));
            assert!(row.contains("|count="));
            assert!(row.contains("|cells="));
            assert!(row.contains("|sig="));
            assert!(row.contains("|digest="));
            assert!(audit_kor_row(&row));
            k += 1;
        }
    }

    #[test]
    fn audit_rows_reject_every_field_mutation() {
        let row = render_east_row(EastId::Czech);
        assert!(audit_east_row(&row));
        assert!(!audit_east_row(&tamper_tail(&row)));
        assert!(!audit_east_row(""));
        assert!(!audit_east_row(&row.replace("golden=6", "golden=5")));
        assert!(!audit_east_row(&row.replace("hit=6", "hit=5")));
        assert!(!audit_east_row(&row.replace("cells=46", "cells=45")));
        assert!(!audit_east_row(&row.replace("qwertz-cs", "qwertz-sk")));

        let krow = render_kor_row(Face::HangulShift);
        assert!(audit_kor_row(&krow));
        assert!(!audit_kor_row(&tamper_tail(&krow)));
        assert!(!audit_kor_row(""));
        assert!(!audit_kor_row(&krow.replace("count=46", "count=45")));
        assert!(!audit_kor_row(&krow.replace("hangul-shift", "hangul-base")));
    }

    #[test]
    fn tamper_tail_flips_last_byte_only() {
        let row = render_east_row(EastId::Hungarian);
        let bad = tamper_tail(&row);
        let rb = row.as_bytes();
        let bb = bad.as_bytes();
        assert_eq!(rb.len(), bb.len());
        assert_ne!(bad, row);
        let mut i = 0usize;
        while i + 1 < rb.len() {
            assert_eq!(rb[i], bb[i]);
            i += 1;
        }
        let tail = rb[rb.len() - 1];
        assert_ne!(tail, bb[bb.len() - 1]);
        // 末位是 0/1 时两次篡改互为逆；其余末位先归 0、再归 1，回不到原文。
        if tail == b'0' || tail == b'1' {
            assert_eq!(tamper_tail(&bad), row);
        } else {
            assert_ne!(tamper_tail(&bad), row);
        }
        assert_eq!(tamper_tail(""), "");
        assert_eq!(tamper_tail("abc0"), "abc1");
        assert_eq!(tamper_tail("abc1"), "abc0");
        assert_eq!(tamper_tail("abcx"), "abc0");
    }

    #[test]
    fn export_row_counts_and_stability() {
        let e = export_east_rows();
        let k = export_kor_rows();
        assert_eq!(e.len(), EAST_ROW_SLOTS);
        assert_eq!(k.len(), KOR_ROW_SLOTS);
        let mut i = 0usize;
        while i < e.len() {
            assert!(audit_east_row(match e.get(i) {
                Some(v) => v.as_str(),
                None => panic!("缺行"),
            }));
            i += 1;
        }
        let mut j = 0usize;
        while j < k.len() {
            assert!(audit_kor_row(match k.get(j) {
                Some(v) => v.as_str(),
                None => panic!("缺行"),
            }));
            j += 1;
        }
        assert_eq!(export_east_rows(), e);
        assert_eq!(export_kor_rows(), k);
        assert!(rows_stable());
    }

    #[test]
    fn exported_rows_never_collide_within_a_family() {
        let e = export_east_rows();
        let mut i = 0usize;
        while i < e.len() {
            let mut j = i + 1;
            while j < e.len() {
                assert_ne!(e.get(i), e.get(j));
                j += 1;
            }
            i += 1;
        }
        let k = export_kor_rows();
        let mut m = 0usize;
        while m < k.len() {
            let mut n = m + 1;
            while n < k.len() {
                assert_ne!(k.get(m), k.get(n));
                n += 1;
            }
            m += 1;
        }
    }

    #[test]
    fn row_families_are_isolated() {
        assert!(row_family_isolation_ok());
        let e = match export_east_rows().first() {
            Some(v) => v.clone(),
            None => panic!("缺行"),
        };
        let k = match export_kor_rows().first() {
            Some(v) => v.clone(),
            None => panic!("缺行"),
        };
        assert!(!audit_east_row(&k));
        assert!(!audit_kor_row(&e));
        assert!(!audit_east_row(&e.replace(EAST_TAG, KOR_TAG)));
        assert!(!audit_kor_row(&k.replace(KOR_TAG, EAST_TAG)));
    }

    #[test]
    fn east_and_kor_row_tags_are_declared() {
        assert_eq!(EAST_TAG, "EAST46");
        assert_eq!(KOR_TAG, "KOR2FACE");
        assert_eq!(EAST_KEYS.len(), 5);
        assert_eq!(KOR_KEYS.len(), 4);
        assert_eq!(EAST_KEYS[0], "layout");
        assert_eq!(EAST_KEYS[4], "sig");
        assert_eq!(KOR_KEYS[0], "face");
        assert_eq!(KOR_KEYS[3], "sig");
    }

    // ── 判据出口 ─────────────────────────────────────────────────
    #[test]
    fn extra_checks_are_exactly_eight_all_green() {
        let c = extra_checks();
        assert_eq!(c.len(), 8);
        let mut i = 0usize;
        while i < c.len() {
            let (name, ok, detail) = match c.get(i) {
                Some(v) => v,
                None => panic!("缺判据"),
            };
            assert!(ok);
            assert!(!name.is_empty());
            assert!(!detail.is_empty());
            assert!(name.starts_with("F49103/") || name.starts_with("F49104/"));
            i += 1;
        }
        assert!(c.iter().all(|x| x.1));
    }

    #[test]
    fn extra_check_names_are_unique_and_split_per_card() {
        let c = extra_checks();
        let mut i = 0usize;
        while i < c.len() {
            let a = match c.get(i) {
                Some(v) => v.0,
                None => panic!("缺判据"),
            };
            let mut j = i + 1;
            while j < c.len() {
                if let Some(b) = c.get(j) {
                    assert_ne!(a, b.0);
                }
                j += 1;
            }
            i += 1;
        }
        assert_eq!(c.iter().filter(|x| x.0.starts_with("F49103/")).count(), 4);
        assert_eq!(c.iter().filter(|x| x.0.starts_with("F49104/")).count(), 4);
    }

    #[test]
    fn run_checkset_is_green_with_eight_dyn_slots() {
        let s = run_unx49103_checks();
        assert!(s.all_passed());
        assert_eq!(s.failed_count(), 0);
        assert_eq!(s.passed_count(), 8);
        assert_eq!(s.dyn_used(), 8);
        assert_eq!(s.dyn_dropped(), 0);
        assert_eq!(s.len(), 8);
        assert!(s.red_items_text().is_empty());
        let (g, b) = s.tally();
        assert_eq!((g, b), (8, 0));
    }

    #[test]
    fn run_checkset_is_repeatable() {
        let a = run_unx49103_checks();
        let b = run_unx49103_checks();
        assert_eq!(a.passed_count(), b.passed_count());
        assert_eq!(a.len(), b.len());
    }
}
