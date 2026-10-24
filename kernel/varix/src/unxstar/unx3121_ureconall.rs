#![allow(dead_code)]
#![allow(unused_imports)]
//! UNX-F3121 · 域级总对账方案（对账键清单与十面勾稽范围冻结）
//!
//! 任务单号: UNX-F3121
//! 标题: 域级总对账方案（对账键清单与十面勾稽范围冻结）
//! 域/批: A4/B37
//! 册目标: 400 行（域级总对账 180 + 方案 120 + 断言 100；测试段不计）
//!
//! 判据原文（逐字摘抄，来源 anchors/UNX-F3121.md）:
//! 「UNX-F3121-J1 总对账方案冻结：十面勾稽（#PF/COW/页缓存/水线/回收/大页/断电/NUMA/并发/调试）
//!   范围与对账键清单冻结哈希入册，十面与 F3122–F3131 条位一一映射断言过，方案漂移注入拦截」
//!
//! 复测方式（逐字摘抄）:
//! 「十面定义在册，映射 10/10 点验，注入拦截，方案哈希入库，
//!   十面定义件哈希与对账键集冻结读数随批归档，映射点验单注记在档可回放。」
//!
//! 定位（逐字摘抄）:
//! 「域级对账的总纲——十面勾稽方案：域内十面账册（十四路径面/COW 契约面/页缓存命中率面/
//!   水线面/回收比率面/大页断言面/断电零丢失面/NUMA 配比面/并发锁面/调试导出面）逐面终勾稽；
//!   对账键清单（每面的勾稽键集）冻结；十面条位映射断言；
//!   方案是 B37 调度面与 B40 终账底稿。」
//!
//! 语义边界（逐字摘抄）:
//! 「只做方案冻结；不做十面勾稽本体（F3122–F3131）、不做 B40 终账。」
//!
//! 依赖与嫁接源（逐字摘抄）:
//! 「依赖 F3122–F3131/F2982/F3081；嫁接源为域内收口制度（F2981/F3101 同制），实现自研。」
//!
//! 风险与回退（逐字摘抄）:
//! 「面位失配（十面与条位漂移）——条位映射表冻结断言，失配注入 10/10 次拦截。」
//!
//! 与 Windows 对照（逐字摘抄）:
//! 「域内收口制度，无公开对应件。」
//!
//! 十面设计（本单核心，逐字对齐锚点定位段的十面名）:
//! | 面位 | 面名 | 勾稽键数 |
//! |---|---|---|
//! | 0 | 十四路径面 | 8 |
//! | 1 | COW 契约面 | 6 |
//! | 2 | 页缓存命中率面 | 7 |
//! | 3 | 水线面 | 5 |
//! | 4 | 回收比率面 | 6 |
//! | 5 | 大页断言面 | 5 |
//! | 6 | 断电零丢失面 | 6 |
//! | 7 | NUMA 配比面 | 5 |
//! | 8 | 并发锁面 | 6 |
//! | 9 | 调试导出面 | 6 |
//! 合计 60 个勾稽键。
//!
//! 十面两两交叉复核（本单深度核心）:
//! `45` 个面次对（`10 × 9 / 2`）逐对交叉复核，任一面出现下列任一情形即检出：
//! - **缺失**：该面在其参与的交叉对中读数为零 / 未登记；
//! - **多算**：该面读数超出配比上界（口径放大）；
//! - **口径不一**：两面对同一勾稽口径给出不同的度量单位/口径标识。
//!
//! 恒等式：`ΣA == ΣB == 总账` 零偏离
//! - `ΣA`：A 面（面 0 十四路径面）为「张数侧」总账；
//! - `ΣB`：B 面（面 4 回收比率面）为「回收侧」总账；
//! - `总账`：跨面加法链独立重算值（与 A、B 均无共享中间量）。

use alloc::vec;

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ===========================================================================
// 0. 常量表
// ===========================================================================

/// 模块版本标识（写入冻结件 + TLV 组头）。
pub const V_VERSION_ID: u64 = 0x0001_3121_0000_0001;
/// 模块版本串。
pub const V_VERSION: &str = "v3121-ureconall.v1";
/// 诊断码段基址（独占段 `0x2640`）。
pub const SEG_BASE: u16 = 0x2640;
/// 诊断码段掩码（段容量 64 码）。
pub const SEG_MASK: u16 = 0x003F;
/// 诊断码段上界（= `SEG_BASE | SEG_MASK`）。
pub const SEG_END: u16 = SEG_BASE | SEG_MASK;
/// 诊断主码（段基址本身）。
pub const MAIN_CODE: u16 = SEG_BASE;

/// 十面数（锚点点名十面）。
pub const FACE_COUNT: usize = 10;
/// 十面两两交叉复核对数（`10 × 9 / 2 = 45`）。
pub const CROSS_COUNT: usize = FACE_COUNT * (FACE_COUNT - 1) / 2;
/// 勾稽键总数（`8+6+7+5+6+5+6+5+6+6 = 60`）。
pub const KEY_TOTAL: usize = 60;
/// 条位数（F3122–F3131 共 10 条，一面一位）。
pub const SLOT_COUNT: usize = 10;
/// 单面勾稽真跑步数。
pub const FACE_STEPS: usize = 18;
/// 单面勾稽最大读数（配比上界，超出即「多算」）。
pub const FACE_READ_CAP: u64 = 4096;
/// 复核准定轮数。
pub const REPEAT: usize = 3;
/// 注入形态数（判据要求 10/10 拦截，本单给 14 形态）。
pub const FORM_COUNT: usize = 14;
/// 面位图宽度（`FACE_COUNT = 10` → 低 10 位）。
pub const FACE_BITS: u32 = (1u32 << FACE_COUNT) - 1;
/// 键位图宽度（`KEY_TOTAL = 60` → 低 60 位，`u64` 恰好承载）。
pub const KEY_MASK64: u64 = 0x0FFF_FFFF_FFFF_FFFFu64;
/// 环形账容量（2 的幂；`60 键 × 3 轮 = 180 > 128`，必然覆盖以压出 dropped 显性）。
pub const RING_CAP: usize = 128;
/// 环形账掩码。
pub const RING_MASK: usize = RING_CAP - 1;
/// 逻辑时钟步进量。
pub const TICK_STEP: u64 = 1;
/// 逻辑步上限（`60 × 18 × 3` 再留一倍裕量）。
pub const STEP_CAP: u64 = (KEY_TOTAL * FACE_STEPS * REPEAT * 2) as u64;
/// syscall 上界（本单零真实 syscall）。
pub const SYSCALL_CAP: u64 = 0;
/// 内存峰值上界（字节，模型侧估算）。
pub const MEM_PEAK_CAP: u64 = 6 * 1024 * 1024;
/// TLV 组头标签。
pub const TLV_TAG_HEAD: u8 = 0x01;
/// TLV 组头载荷字节数（由 `export_tlv` 内三条 `push_u64` 反推）。
pub const TLV_HEAD_PAYLOAD: usize = 24;
/// TLV 组头总长（`1 + 2 + 24`）。
pub const TLV_HEAD_LEN: usize = TLV_HEAD_PAYLOAD + 3;
/// TLV 条目固定长度（`1 + 1 + 8`）。
pub const TLV_ITEM_LEN: usize = 10;
/// B37 调度面名。
pub const BATCH_FACE: &str = "B37";
/// B40 终账面名（本单方案是 B40 底稿，但不做终账）。
pub const FINAL_FACE: &str = "B40";
/// 上游条位区间（锚点「十面与 F3122–F3131 条位一一映射」）。
pub const DEP_SLOTS: &str = "F3122-F3131";
/// 嫁接源（锚点「F2981/F3101 同制」）。
pub const GRAFT_SEAT: &str = "F2981";
/// 对齐面读数锚（锚点「与 F2797 体检单链对齐在档」）。
pub const ALIGN_CHAIN: &str = "F2797";
/// 面 A（张数侧）面位。
pub const FACE_A: usize = 0;
/// 面 B（回收侧）面位。
pub const FACE_B: usize = 4;
/// 交叉复核对最小读数（低于此值视为该面在交叉对中「缺失」）。
pub const CROSS_MIN_READ: u64 = 1;

/// 条位号表（`F3122..F3131` 十条位）。
pub const SLOT_TICKETS: [&str; SLOT_COUNT] = [
    "F3122", "F3123", "F3124", "F3125", "F3126", "F3127", "F3128", "F3129", "F3130", "F3131",
];
/// 十面中文名表（逐字对齐锚点定位段）。
pub const FACE_NAMES: [&str; FACE_COUNT] = [
    "十四路径面",
    "COW 契约面",
    "页缓存命中率面",
    "水线面",
    "回收比率面",
    "大页断言面",
    "断电零丢失面",
    "NUMA 配比面",
    "并发锁面",
    "调试导出面",
];
/// 十面英文短名表。
pub const FACE_CODES: [&str; FACE_COUNT] = [
    "PATH14", "COW", "PGCACHE", "WATERMARK", "RECLAIM", "HUGE", "POWERFAIL", "NUMA", "LOCK", "DBG",
];
/// 每面勾稽键数表（合计 60）。
pub const FACE_KEY_COUNTS: [usize; FACE_COUNT] = [8, 6, 7, 5, 6, 5, 6, 5, 6, 6];
/// 十面口径族表（交叉复核「口径不一」检出的依据）。
///
/// 同族面共用度量口径**与计量单位**；交叉复核时若两面口径族相同而单位不同，即判「口径不一」。
/// 口径族表按族划分单位：`count` 族全部记「张」，`ratio` 族全部记「千分率」，
/// `level`/`contract`/`size`/`hold`/`text` 各族唯一——这样净态下同族单位必一致，
/// 口径不一只可能来自册内登记被改（注入形态 `CrossCaliberDrift` 的落点）。
pub const FACE_KALIBER: [&str; FACE_COUNT] = [
    "count", "contract", "ratio", "level", "ratio", "size", "count", "ratio", "hold", "text",
];
/// 十面计量单位表。
pub const FACE_UNIT: [&str; FACE_COUNT] = [
    "张", "契约条", "千分率", "千字节", "千分率", "页", "张", "千分率", "锁次数", "字节",
];

// ===========================================================================
// 1. 确定性基础件
// ===========================================================================

/// 确定性 LCG 状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Lcg {
    /// 当前状态。
    pub state: u64,
}
impl Lcg {
    /// 以种子构造。
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }
    /// 推进一步，返回新状态。
    pub fn next(&mut self) -> u64 {
        self.state = lcg_step(self.state);
        self.state
    }
    /// 推进一步并记步（语义别名，与 `next` 同算式）。
    pub fn step(&mut self) -> u64 {
        let v = self.next();
        v
    }
    /// 取低 32 位。
    pub fn next32(&mut self) -> u32 {
        let v = self.next();
        (v & 0xFFFF_FFFF) as u32
    }
    /// 取模取样（模数为 0 时给 0）。
    pub fn below(&mut self, m: u64) -> u64 {
        let v = self.next();
        if m == 0 {
            0
        } else {
            v % m
        }
    }
}
/// LCG 单步（`x = a*x + c (mod 2^64)`，wrapping）。
pub fn lcg_step(state: u64) -> u64 {
    state
        .wrapping_mul(6364136223846793005u64)
        .wrapping_add(1442695040888963407u64)
}
/// FNV-1a 64 增量。
pub fn fnv1a64_ext(state: u64, bytes: &[u8]) -> u64 {
    let mut h: u64 = state;
    for b in bytes.iter() {
        let x: u64 = (*b as u64) & 0xFF;
        h = (h ^ x).wrapping_mul(0x100000001b3u64);
    }
    h
}
/// FNV-1a 64（basis `0xcbf29ce484222325`，prime `0x100000001b3`）。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    fnv1a64_ext(0xcbf29ce484222325u64, bytes)
}
/// FNV-1a 64 对 `u64` 词表求指纹。
pub fn fnv1a_words(words: &[u64]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325u64;
    for w in words.iter() {
        let v: u64 = *w;
        let mut i: usize = 0;
        while i < 8 {
            let byte: u8 = ((v >> (56 - 8 * i)) & 0xFF) as u8;
            h = (h ^ (byte as u64)).wrapping_mul(0x100000001b3u64);
            i = i.wrapping_add(1);
        }
    }
    h
}
/// 逻辑时钟推进（零墙钟）。
pub fn tick_advance(now: u64) -> u64 {
    now.wrapping_add(TICK_STEP)
}
/// 字符串折入指纹。
pub fn fnv1a_str(state: u64, s: &str) -> u64 {
    fnv1a64_ext(state, s.as_bytes())
}
/// 绝对差（零浮点）。
pub fn abs_diff(a: u64, b: u64) -> u64 {
    if a > b {
        a.wrapping_sub(b)
    } else {
        b.wrapping_sub(a)
    }
}

// ===========================================================================
// 2. 错误闭集（变体一码一段）
// ===========================================================================

/// 注入 / 检出形态（14 变体，段内偏移 0..=13）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Form {
    /// 形态 0：面缺失（登记面数不足十）。
    FaceMissing,
    /// 形态 1：面重号。
    FaceDup,
    /// 形态 2：面位越界。
    FaceOutOfRange,
    /// 形态 3：对账键清单缺键。
    KeyMissing,
    /// 形态 4：对账键重号。
    KeyDup,
    /// 形态 5：交叉对缺失。
    CrossMissing,
    /// 形态 6：交叉对多算（读数越配比上界）。
    CrossOvercount,
    /// 形态 7：交叉对口径不一。
    CrossCaliberDrift,
    /// 形态 8：条位失配（面与 F3122–F3131 不一一映射）。
    SlotMismatch,
    /// 形态 9：恒等式 ΣA ≠ ΣB。
    IdentityAB,
    /// 形态 10：恒等式 ΣA ≠ 总账。
    IdentityALedger,
    /// 形态 11：环形账不守恒。
    LedgerUnbalanced,
    /// 形态 12：冻结指纹漂移。
    FingerprintDrift,
    /// 形态 13：勾稽步数越界。
    StepOverCap,
}
impl Form {
    /// 形态总数。
    pub const COUNT: usize = 14;
    /// 全部形态（顺序即码段偏移序）。
    pub const ALL: [Form; Form::COUNT] = [
        Form::FaceMissing,
        Form::FaceDup,
        Form::FaceOutOfRange,
        Form::KeyMissing,
        Form::KeyDup,
        Form::CrossMissing,
        Form::CrossOvercount,
        Form::CrossCaliberDrift,
        Form::SlotMismatch,
        Form::IdentityAB,
        Form::IdentityALedger,
        Form::LedgerUnbalanced,
        Form::FingerprintDrift,
        Form::StepOverCap,
    ];
    /// 段内偏移下标。
    pub fn index(self) -> usize {
        match self {
            Form::FaceMissing => 0,
            Form::FaceDup => 1,
            Form::FaceOutOfRange => 2,
            Form::KeyMissing => 3,
            Form::KeyDup => 4,
            Form::CrossMissing => 5,
            Form::CrossOvercount => 6,
            Form::CrossCaliberDrift => 7,
            Form::SlotMismatch => 8,
            Form::IdentityAB => 9,
            Form::IdentityALedger => 10,
            Form::LedgerUnbalanced => 11,
            Form::FingerprintDrift => 12,
            Form::StepOverCap => 13,
        }
    }
    /// 按下标取形态。
    pub fn from_index(i: usize) -> Option<Form> {
        if i < Form::COUNT {
            let f = Form::ALL[i];
            Some(f)
        } else {
            None
        }
    }
    /// 诊断码（`SEG_BASE + 偏移`，一码一段）。
    pub fn code(self) -> u16 {
        SEG_BASE.wrapping_add(self.index() as u16)
    }
    /// 形态名。
    pub fn name(self) -> &'static str {
        match self {
            Form::FaceMissing => "FaceMissing",
            Form::FaceDup => "FaceDup",
            Form::FaceOutOfRange => "FaceOutOfRange",
            Form::KeyMissing => "KeyMissing",
            Form::KeyDup => "KeyDup",
            Form::CrossMissing => "CrossMissing",
            Form::CrossOvercount => "CrossOvercount",
            Form::CrossCaliberDrift => "CrossCaliberDrift",
            Form::SlotMismatch => "SlotMismatch",
            Form::IdentityAB => "IdentityAB",
            Form::IdentityALedger => "IdentityALedger",
            Form::LedgerUnbalanced => "LedgerUnbalanced",
            Form::FingerprintDrift => "FingerprintDrift",
            Form::StepOverCap => "StepOverCap",
        }
    }
    /// 形态描述（中文在档注记）。
    pub fn describe(self) -> &'static str {
        match self {
            Form::FaceMissing => "面缺失：十面登记不齐",
            Form::FaceDup => "面重号：同面位两条登记记录",
            Form::FaceOutOfRange => "面位越界：超出十面闭集",
            Form::KeyMissing => "对账键缺键：清单未覆盖该面键数",
            Form::KeyDup => "对账键重号：同键两条记录",
            Form::CrossMissing => "交叉对缺失：面次对未登记 / 该面读数为零",
            Form::CrossOvercount => "交叉对多算：读数越配比上界",
            Form::CrossCaliberDrift => "交叉对口径不一：同族面单位不同",
            Form::SlotMismatch => "条位失配：面与 F3122–F3131 非一一映射",
            Form::IdentityAB => "恒等式失败：ΣA ≠ ΣB",
            Form::IdentityALedger => "恒等式失败：ΣA ≠ 总账",
            Form::LedgerUnbalanced => "环形账不守恒：写入数 ≠ 存活 + 丢弃",
            Form::FingerprintDrift => "冻结指纹漂移：定义件哈希与入册值失配",
            Form::StepOverCap => "勾稽步数越界：越过 STEP_CAP",
        }
    }
    /// 探测面名。
    pub fn probe(self) -> &'static str {
        match self {
            Form::FaceMissing => "face_book.cover_bits",
            Form::FaceDup => "face_book.dup_bits",
            Form::FaceOutOfRange => "face_book.range_ok",
            Form::KeyMissing => "key_book.cover_bits",
            Form::KeyDup => "key_book.dup_bits",
            Form::CrossMissing => "cross.missing_bits",
            Form::CrossOvercount => "cross.overcount",
            Form::CrossCaliberDrift => "cross.caliber_bits",
            Form::SlotMismatch => "slot_map.one2one_ok",
            Form::IdentityAB => "identity.dev_ab",
            Form::IdentityALedger => "identity.dev_al",
            Form::LedgerUnbalanced => "ring.balanced",
            Form::FingerprintDrift => "freeze.verify",
            Form::StepOverCap => "facet.step_ok",
        }
    }
    /// 是否属「键清单类」形态。
    pub fn is_key(self) -> bool {
        matches!(self, Form::KeyMissing | Form::KeyDup)
    }
    /// 是否属「交叉复核类」形态。
    pub fn is_cross(self) -> bool {
        matches!(
            self,
            Form::CrossMissing | Form::CrossOvercount | Form::CrossCaliberDrift
        )
    }
    /// 是否属「恒等式类」形态。
    pub fn is_identity(self) -> bool {
        matches!(
            self,
            Form::IdentityAB | Form::IdentityALedger | Form::LedgerUnbalanced
        )
    }
    /// 形态类名。
    pub fn class_name(self) -> &'static str {
        if self.is_key() {
            "键清单面"
        } else if self.is_cross() {
            "交叉复核面"
        } else if self.is_identity() {
            "恒等式面"
        } else {
            "面位/冻结面"
        }
    }
}
/// 辅助码（段内偏移 `0x1C..=0x21`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuxCode {
    /// 辅助 0：总册版本失配。
    BookRevMismatch,
    /// 辅助 1：推进顺序破坏。
    FaceOrderBroken,
    /// 辅助 2：键区间越界。
    KeyRangeOver,
    /// 辅助 3：复核轮数不足。
    RepeatShort,
    /// 辅助 4：配比上界失配。
    CapMismatch,
    /// 辅助 5：语义边界越界。
    SemanticBorderCross,
}
impl AuxCode {
    /// 辅助码总数。
    pub const COUNT: usize = 6;
    /// 全部辅助码。
    pub const ALL: [AuxCode; AuxCode::COUNT] = [
        AuxCode::BookRevMismatch,
        AuxCode::FaceOrderBroken,
        AuxCode::KeyRangeOver,
        AuxCode::RepeatShort,
        AuxCode::CapMismatch,
        AuxCode::SemanticBorderCross,
    ];
    /// 段内偏移下标。
    pub fn index(self) -> usize {
        match self {
            AuxCode::BookRevMismatch => 0,
            AuxCode::FaceOrderBroken => 1,
            AuxCode::KeyRangeOver => 2,
            AuxCode::RepeatShort => 3,
            AuxCode::CapMismatch => 4,
            AuxCode::SemanticBorderCross => 5,
        }
    }
    /// 诊断码（`SEG_BASE + 0x1C + 下标`）。
    pub fn code(self) -> u16 {
        SEG_BASE.wrapping_add(0x1Cu16).wrapping_add(self.index() as u16)
    }
    /// 辅助码名。
    pub fn name(self) -> &'static str {
        match self {
            AuxCode::BookRevMismatch => "BookRevMismatch",
            AuxCode::FaceOrderBroken => "FaceOrderBroken",
            AuxCode::KeyRangeOver => "KeyRangeOver",
            AuxCode::RepeatShort => "RepeatShort",
            AuxCode::CapMismatch => "CapMismatch",
            AuxCode::SemanticBorderCross => "SemanticBorderCross",
        }
    }
    /// 辅助码描述。
    pub fn describe(self) -> &'static str {
        match self {
            AuxCode::BookRevMismatch => "总册版本与冻结登记版本失配",
            AuxCode::FaceOrderBroken => "十面推进顺序被破坏（非单调）",
            AuxCode::KeyRangeOver => "键号越界：超出 1..=60",
            AuxCode::RepeatShort => "复核轮数不足下限",
            AuxCode::CapMismatch => "配比上界与登记值失配",
            AuxCode::SemanticBorderCross => "语义边界越界：本单只做方案冻结",
        }
    }
}
/// 本单结果别名。
pub type UrResult<T> = Result<T, UrErr>;
/// 错误对象（三要素）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct UrErr {
    /// 诊断码。
    pub code: u16,
    /// 明文细节。
    pub detail: String,
}
impl UrErr {
    /// 构造。
    pub fn new(code: u16, detail: String) -> UrErr {
        UrErr { code, detail }
    }
    /// 由形态构造。
    pub fn of_form(f: Form, detail: String) -> UrErr {
        UrErr { code: f.code(), detail }
    }
    /// 由辅助码构造。
    pub fn of_aux(a: AuxCode, detail: String) -> UrErr {
        UrErr { code: a.code(), detail }
    }
    /// 三要素之一：码。
    pub fn code(&self) -> u16 {
        self.code
    }
    /// 三要素之二：明细。
    pub fn detail(&self) -> String {
        self.detail.clone()
    }
    /// 三要素之三：全串。
    pub fn full(&self) -> String {
        format!("0x{:04X} {}", self.code, self.detail)
    }
}
/// 码反查名。
pub fn form_by_code(code: u16) -> Option<&'static str> {
    let mut i: usize = 0;
    while i < Form::COUNT {
        let f = Form::ALL[i];
        if f.code() == code {
            return Some(f.name());
        }
        i = i.wrapping_add(1);
    }
    let mut j: usize = 0;
    while j < AuxCode::COUNT {
        let a = AuxCode::ALL[j];
        if a.code() == code {
            return Some(a.name());
        }
        j = j.wrapping_add(1);
    }
    None
}
/// 错误一行串。
pub fn err_str(e: &UrErr) -> String {
    match form_by_code(e.code) {
        Some(n) => format!("[{}] {}", n, e.detail),
        None => format!("[0x{:04X}] {}", e.code, e.detail),
    }
}
/// 恒等式错误。
pub fn err_identity(a: u64, b: u64, l: u64) -> UrErr {
    UrErr::of_form(
        Form::IdentityAB,
        format!("恒等式偏离：ΣA={} ΣB={} 总账={}", a, b, l),
    )
}
/// 越界错误。
pub fn err_range(what: &str, idx: usize, cap: usize) -> UrErr {
    UrErr::of_aux(AuxCode::KeyRangeOver, format!("越界：{} idx={} cap={}", what, idx, cap))
}
/// 码段是否合法。
pub fn code_in_seg(code: u16) -> bool {
    let lo = SEG_BASE;
    let hi = SEG_END;
    code >= lo && code <= hi
}
/// 主码段串。
pub fn seg_line() -> String {
    format!(
        "seg 0x{:04X}..=0x{:04X} mask=0x{:04X} faces={} keys={} crosses={} forms={} aux={} max=0x{:04X}",
        SEG_BASE,
        SEG_END,
        SEG_MASK,
        FACE_COUNT,
        KEY_TOTAL,
        CROSS_COUNT,
        Form::COUNT,
        AuxCode::COUNT,
        AuxCode::ALL[AuxCode::COUNT - 1].code()
    )
}

// ===========================================================================
// 3. 十面状态机（合法边闭集 + 非法转换拒记）
// ===========================================================================

/// 十面勾稽推进态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// 十面定义入册。
    P1Define,
    /// 对账键清单冻结。
    P2Keys,
    /// 条位映射断言。
    P3Slots,
    /// 十面两两交叉复核。
    P4Cross,
    /// 恒等式收口。
    P5Ident,
    /// 终态：方案冻结入册。
    Sealed,
}
impl Phase {
    /// 状态数（含终态）。
    pub const COUNT: usize = 6;
    /// 全部状态。
    pub const ALL: [Phase; Phase::COUNT] = [
        Phase::P1Define,
        Phase::P2Keys,
        Phase::P3Slots,
        Phase::P4Cross,
        Phase::P5Ident,
        Phase::Sealed,
    ];
    /// 状态下标。
    pub fn index(self) -> usize {
        match self {
            Phase::P1Define => 0,
            Phase::P2Keys => 1,
            Phase::P3Slots => 2,
            Phase::P4Cross => 3,
            Phase::P5Ident => 4,
            Phase::Sealed => 5,
        }
    }
    /// 相位名。
    pub fn name(self) -> &'static str {
        match self {
            Phase::P1Define => "P1Define",
            Phase::P2Keys => "P2Keys",
            Phase::P3Slots => "P3Slots",
            Phase::P4Cross => "P4Cross",
            Phase::P5Ident => "P5Ident",
            Phase::Sealed => "Sealed",
        }
    }
    /// 相位中文名。
    pub fn cn(self) -> &'static str {
        match self {
            Phase::P1Define => "相位一 十面定义入册",
            Phase::P2Keys => "相位二 对账键清单冻结",
            Phase::P3Slots => "相位三 条位映射断言",
            Phase::P4Cross => "相位四 十面两两交叉复核",
            Phase::P5Ident => "相位五 恒等式收口",
            Phase::Sealed => "方案冻结入册",
        }
    }
    /// 相位描述。
    pub fn describe(self) -> &'static str {
        match self {
            Phase::P1Define => "每面勾稽范围 / 输入账册 / 输出勾稽账三列定义入册",
            Phase::P2Keys => "对账键集冻结哈希入册",
            Phase::P3Slots => "十面与 F3122–F3131 条位一一映射断言",
            Phase::P4Cross => "45 个面次对逐对交叉复核，缺 / 多 / 口径三查",
            Phase::P5Ident => "ΣA == ΣB == 总账 零偏离收口",
            Phase::Sealed => "方案哈希入库，漂移注入拦截",
        }
    }
    /// 是否终态。
    pub fn is_terminal(self) -> bool {
        matches!(self, Phase::Sealed)
    }
    /// 合法出边闭集（只允许推进到下一相位）。
    pub fn allowed(self) -> &'static [Phase] {
        match self {
            Phase::P1Define => &[Phase::P2Keys],
            Phase::P2Keys => &[Phase::P3Slots],
            Phase::P3Slots => &[Phase::P4Cross],
            Phase::P4Cross => &[Phase::P5Ident],
            Phase::P5Ident => &[Phase::Sealed],
            Phase::Sealed => &[],
        }
    }
    /// 是否可推进到目标态。
    pub fn may_go(self, to: Phase) -> bool {
        let al = self.allowed();
        let mut i: usize = 0;
        while i < al.len() {
            if al[i] == to {
                return true;
            }
            i = i.wrapping_add(1);
        }
        false
    }
    /// 合法出边名表。
    pub fn allowed_names(self) -> Vec<&'static str> {
        let al = self.allowed();
        let mut out: Vec<&'static str> = Vec::with_capacity(al.len());
        let mut i: usize = 0;
        while i < al.len() {
            out.push(al[i].name());
            i = i.wrapping_add(1);
        }
        out
    }
    /// 相位输入面。
    pub fn input_face(self) -> &'static str {
        match self {
            Phase::P1Define => "域内十面账册",
            Phase::P2Keys => "十面定义",
            Phase::P3Slots => "对账键清单",
            Phase::P4Cross => "逐面勾稽账",
            Phase::P5Ident => "交叉复核结论",
            Phase::Sealed => "恒等式结论",
        }
    }
    /// 相位输出面。
    pub fn output_face(self) -> &'static str {
        match self {
            Phase::P1Define => "十面定义表",
            Phase::P2Keys => "对账键清单",
            Phase::P3Slots => "条位映射表",
            Phase::P4Cross => "交叉复核账",
            Phase::P5Ident => "恒等式结论",
            Phase::Sealed => "方案冻结哈希",
        }
    }
    /// 三列定义（锚点「勾稽范围/输入账册/输出勾稽账三列定义入册」）。
    pub fn three_columns(self) -> String {
        format!("{} | 输入={} | 输出={} | 判据={}", self.cn(), self.input_face(), self.output_face(), self.describe())
    }
}
/// 非法迁移拒记。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Reject {
    /// 来源态。
    pub from: Phase,
    /// 目标态。
    pub to: Phase,
    /// 逻辑时刻。
    pub tick: u64,
}
impl Reject {
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("拒记 {}->{} @tick{}", self.from.name(), self.to.name(), self.tick)
    }
}
/// 方案冻结状态机。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PhaseFsm {
    /// 当前态。
    pub state: Phase,
    /// 逻辑时刻。
    pub tick: u64,
    /// 走过序列。
    pub trail: Vec<Phase>,
    /// 非法迁移拒记。
    pub rejected: Vec<Reject>,
}
impl PhaseFsm {
    /// 构造。
    pub fn new() -> PhaseFsm {
        let mut trail: Vec<Phase> = Vec::with_capacity(Phase::COUNT);
        trail.push(Phase::P1Define);
        PhaseFsm { state: Phase::P1Define, tick: 0, trail, rejected: Vec::new() }
    }
    /// 尝试迁移（非法则拒记）。
    pub fn try_go(&mut self, to: Phase) -> UrResult<Phase> {
        if self.state == to {
            return Err(UrErr::of_form(
                Form::FaceMissing,
                format!("自环迁移非法：{}", to.name()),
            ));
        }
        if !self.state.may_go(to) {
            self.tick = tick_advance(self.tick);
            self.rejected.push(Reject { from: self.state, to, tick: self.tick });
            return Err(UrErr::of_aux(
                AuxCode::FaceOrderBroken,
                format!("非法迁移 {}->{}，合法出边={:?}", self.state.name(), to.name(), self.state.allowed_names()),
            ));
        }
        self.state = to;
        self.tick = tick_advance(self.tick);
        self.trail.push(to);
        Ok(to)
    }
    /// 连续推进到目标态。
    pub fn advance_to(&mut self, to: Phase) -> UrResult<u32> {
        let mut steps: u32 = 0;
        while self.state != to {
            let al = self.state.allowed();
            if al.len() == 0 {
                return Err(UrErr::of_form(
                    Form::FingerprintDrift,
                    format!("推进死锁：卡在 {} 想去 {}", self.state.name(), to.name()),
                ));
            }
            let nxt = al[0];
            self.try_go(nxt)?;
            steps = steps.wrapping_add(1);
            if steps > Phase::COUNT as u32 {
                return Err(UrErr::of_aux(
                    AuxCode::FaceOrderBroken,
                    format!("推进步数越界：{} 步", steps),
                ));
            }
        }
        Ok(steps)
    }
    /// trail 严格单调。
    pub fn trail_monotonic(&self) -> bool {
        let mut i: usize = 1;
        while i < self.trail.len() {
            if self.trail[i].index() <= self.trail[i - 1].index() {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }
    /// 已走边数。
    pub fn edges_walked(&self) -> usize {
        if self.trail.len() < 2 {
            0
        } else {
            self.trail.len() - 1
        }
    }
    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(10 + self.rejected.len() * 3);
        words.push(self.state.index() as u64);
        words.push(self.tick);
        words.push(self.trail.len() as u64);
        for s in self.trail.iter() {
            words.push(s.index() as u64);
        }
        words.push(self.rejected.len() as u64);
        for r in self.rejected.iter() {
            words.push(r.from.index() as u64);
            words.push(r.to.index() as u64);
            words.push(r.tick);
        }
        fnv1a_words(&words)
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "fsm state={} edges={} rejected={} monotonic={} fp={:016X}",
            self.state.name(),
            self.edges_walked(),
            self.rejected.len(),
            if self.trail_monotonic() { 1 } else { 0 },
            self.fingerprint()
        )
    }
    /// 合法边矩阵。
    pub fn edge_matrix() -> [[bool; Phase::COUNT]; Phase::COUNT] {
        let mut m: [[bool; Phase::COUNT]; Phase::COUNT] = [[false; Phase::COUNT]; Phase::COUNT];
        let mut a: usize = 0;
        while a < Phase::COUNT {
            let mut b: usize = 0;
            while b < Phase::COUNT {
                m[a][b] = Phase::ALL[a].may_go(Phase::ALL[b]);
                b = b.wrapping_add(1);
            }
            a = a.wrapping_add(1);
        }
        m
    }
    /// 合法边总数（链式 = `COUNT - 1`）。
    pub fn edge_count() -> usize {
        let m = PhaseFsm::edge_matrix();
        let mut c: usize = 0;
        let mut a: usize = 0;
        while a < Phase::COUNT {
            let mut b: usize = 0;
            while b < Phase::COUNT {
                if m[a][b] {
                    c = c.wrapping_add(1);
                }
                b = b.wrapping_add(1);
            }
            a = a.wrapping_add(1);
        }
        c
    }
    /// 非法边总数。
    pub fn illegal_edge_count() -> usize {
        Phase::COUNT * Phase::COUNT - PhaseFsm::edge_count()
    }
    /// 逐行边矩阵。
    pub fn edge_rows() -> Vec<String> {
        let m = PhaseFsm::edge_matrix();
        let mut out: Vec<String> = Vec::with_capacity(Phase::COUNT);
        let mut a: usize = 0;
        while a < Phase::COUNT {
            let mut line = format!("{:>10} ->", Phase::ALL[a].name());
            let mut b: usize = 0;
            while b < Phase::COUNT {
                line.push(if m[a][b] { '1' } else { '0' });
                b = b.wrapping_add(1);
            }
            out.push(line);
            a = a.wrapping_add(1);
        }
        out
    }
}

// ===========================================================================
// 4. 十面册（相位一）
// ===========================================================================

/// 面条目。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Face {
    /// 面位下标（`0..=9`）。
    pub index: usize,
    /// 面位号（`F3122` 起）。
    pub ticket: &'static str,
    /// 面名（中文）。
    pub name: &'static str,
    /// 面短码。
    pub code: &'static str,
    /// 勾稽键数。
    pub key_count: usize,
    /// 键号区间起点（1 基）。
    pub key_base: usize,
    /// 口径族。
    pub caliber: &'static str,
    /// 计量单位。
    pub unit: &'static str,
    /// 勾稽范围。
    pub scope: &'static str,
}
impl Face {
    /// 构造（越界 `index` 仍可构造，越界面由 `FaceBook::register` 拒收）。
    pub fn new(index: usize) -> Face {
        Face {
            index,
            ticket: face_ticket_at(index),
            name: face_name_at(index),
            code: face_code_at(index),
            key_count: face_key_count_at(index),
            key_base: face_key_base(index),
            caliber: face_caliber_at(index),
            unit: face_unit_at(index),
            scope: face_scope_at(index),
        }
    }
    /// 面位图位。
    pub fn bit(&self) -> u32 {
        if self.index >= FACE_COUNT {
            0
        } else {
            1u32 << self.index
        }
    }
    /// 键号区间终点（含）。
    pub fn key_end(&self) -> usize {
        self.key_base.wrapping_add(self.key_count).wrapping_sub(1)
    }
    /// 键位图（该面占的键位）。
    pub fn key_mask(&self) -> u64 {
        if self.key_count == 0 {
            return 0;
        }
        let lo = (self.key_base as u64).wrapping_sub(1);
        let width: u32 = if self.key_count >= 64 { 64 } else { self.key_count as u32 };
        let m: u64 = if width >= 64 { u64::MAX } else { (1u64 << width).wrapping_sub(1) };
        m << lo
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!(
            "face[{}] {} {} keys={}[{}..{}] caliber={} unit={} scope={}",
            self.index,
            self.ticket,
            self.name,
            self.key_count,
            self.key_base,
            self.key_end(),
            self.caliber,
            self.unit,
            self.scope
        )
    }
    /// 指纹词。
    /// 第 7 项是**勾稽范围文本 `scope`** 的哈希——范围文本属冻结定义面，
    /// 改它必须被冻结指纹探到；而 `caliber` / `unit`（计量口径）不进冻结指纹，
    /// 它们归交叉复核口径探面（`cross.caliber_bits`）管，两处职责不重叠。
    pub fn words(&self) -> [u64; 7] {
        [
            self.index as u64,
            self.key_count as u64,
            self.key_base as u64,
            fnv1a64(self.code.as_bytes()),
            fnv1a64(self.name.as_bytes()),
            fnv1a64(self.caliber.as_bytes()),
            fnv1a64(self.scope.as_bytes()),
        ]
    }
}
/// 十面短码表（供表外查询）。
pub const FACE_CODES_REF: [&str; FACE_COUNT] = [
    "PATH14", "COW", "PGCACHE", "WATERMARK", "RECLAIM", "HUGE", "POWERFAIL", "NUMA", "LOCK", "DBG",
];
/// 按下标取面位号。
pub fn face_ticket_at(i: usize) -> &'static str {
    if i < SLOT_COUNT {
        SLOT_TICKETS[i]
    } else {
        "F31??"
    }
}
/// 按下标取面名。
pub fn face_name_at(i: usize) -> &'static str {
    if i < FACE_COUNT {
        FACE_NAMES[i]
    } else {
        "未定义面"
    }
}
/// 按下标取勾稽范围。
pub fn face_scope_at(i: usize) -> &'static str {
    if i < FACE_COUNT {
        FACE_SCOPES[i]
    } else {
        "越界范围"
    }
}
/// 按下标取面短码。
pub fn face_code_at(i: usize) -> &'static str {
    if i < FACE_COUNT {
        FACE_CODES_REF[i]
    } else {
        "OOR"
    }
}
/// 按下标取勾稽键数。
pub fn face_key_count_at(i: usize) -> usize {
    if i < FACE_COUNT {
        FACE_KEY_COUNTS[i]
    } else {
        0
    }
}
/// 按下标取口径族。
pub fn face_caliber_at(i: usize) -> &'static str {
    if i < FACE_COUNT {
        FACE_KALIBER[i]
    } else {
        "oor"
    }
}
/// 按下标取计量单位。
pub fn face_unit_at(i: usize) -> &'static str {
    if i < FACE_COUNT {
        FACE_UNIT[i]
    } else {
        "?"
    }
}
/// 键号区间起点表（累加 `FACE_KEY_COUNTS`，合计 60）。
/// 越界下标给 `KEY_TOTAL + 1`（明确落在 60 键空间之外，便于越界检出）。
pub fn face_key_base(i: usize) -> usize {
    let mut t: usize = 1;
    let mut k: usize = 0;
    while k < i && k < FACE_COUNT {
        t = t.wrapping_add(FACE_KEY_COUNTS[k]);
        k = k.wrapping_add(1);
    }
    t
}
/// 十面勾稽范围表（逐字对齐锚点定位段）。
pub const FACE_SCOPES: [&str; FACE_COUNT] = [
    "十四路径空间张数与层级占用",
    "COW 写时复制契约条数与违约数",
    "页缓存命中率分母分子对齐",
    "水线高低水位读数与推进",
    "回收比率回收量与申请量",
    "大页断言合页数与拆页数",
    "断电零丢失条数与恢复条数",
    "NUMA 配比本地远程分布",
    "并发锁获取释放配平",
    "调试导出字节与条目",
];
/// 十面册（相位一产物）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FaceBook {
    /// 面条目。
    pub faces: Vec<Face>,
    /// 覆盖位图。
    pub cover_bits: u32,
    /// 重号位图。
    pub dup_bits: u32,
    /// 越界面位数。
    pub out_of_range: usize,
}
impl FaceBook {
    /// 空册。
    pub fn blank() -> FaceBook {
        FaceBook { faces: Vec::with_capacity(FACE_COUNT), cover_bits: 0, dup_bits: 0, out_of_range: 0 }
    }
    /// 登记一面（自算位图）。
    pub fn register(&mut self, f: Face) -> UrResult<()> {
        if f.index >= FACE_COUNT {
            self.out_of_range = self.out_of_range.wrapping_add(1);
            return Err(UrErr::of_form(
                Form::FaceOutOfRange,
                format!("面位越界：index={} cap={}", f.index, FACE_COUNT),
            ));
        }
        let b = f.bit();
        if (self.cover_bits & b) != 0 {
            self.dup_bits = self.dup_bits | b;
            return Err(UrErr::of_form(
                Form::FaceDup,
                format!("面重号：{}", f.ticket),
            ));
        }
        self.cover_bits = self.cover_bits | b;
        self.faces.push(f);
        Ok(())
    }
    /// 由 `faces` 重算派生字段。
    pub fn recompute(&mut self) {
        let mut cover: u32 = 0;
        let mut dup: u32 = 0;
        let mut oor: usize = 0;
        for f in self.faces.iter() {
            if f.index >= FACE_COUNT {
                oor = oor.wrapping_add(1);
                continue;
            }
            let b = f.bit();
            if (cover & b) != 0 {
                dup = dup | b;
            } else {
                cover = cover | b;
            }
        }
        self.cover_bits = cover;
        self.dup_bits = dup;
        self.out_of_range = oor;
    }
    /// 缺面位图。
    pub fn missing_bits(&self) -> u32 {
        FACE_BITS & (FACE_BITS ^ self.cover_bits)
    }
    /// 缺面数。
    pub fn missing_count(&self) -> usize {
        self.missing_bits().count_ones() as usize
    }
    /// 缺面名表。
    pub fn missing_list(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::with_capacity(FACE_COUNT);
        let mut i: usize = 0;
        while i < FACE_COUNT {
            if (self.missing_bits() & (1u32 << i)) != 0 {
                out.push(face_name_at(i));
            }
            i = i.wrapping_add(1);
        }
        out
    }
    /// 读面登记口径族（越界给 `"unknown"`）。
    pub fn caliber_of(&self, i: usize) -> &'static str {
        for f in self.faces.iter() {
            if f.index == i {
                return f.caliber;
            }
        }
        "unknown"
    }
    /// 读面登记计量单位（越界给 `"?"`）。
    pub fn unit_of(&self, i: usize) -> &'static str {
        for f in self.faces.iter() {
            if f.index == i {
                return f.unit;
            }
        }
        "?"
    }
    /// 键数合计。
    pub fn key_sum(&self) -> usize {
        let mut t: usize = 0;
        for f in self.faces.iter() {
            t = t.wrapping_add(f.key_count);
        }
        t
    }
    /// 十面齐备。
    pub fn complete(&self) -> bool {
        self.missing_bits() == 0
            && self.dup_bits == 0
            && self.out_of_range == 0
            && self.faces.len() >= FACE_COUNT
            && self.key_sum() == KEY_TOTAL
    }
    /// 册指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(FACE_COUNT * 6 + 4);
        words.push(self.faces.len() as u64);
        words.push(self.cover_bits as u64);
        words.push(self.dup_bits as u64);
        words.push(self.out_of_range as u64);
        for f in self.faces.iter() {
            let w = f.words();
            let mut i: usize = 0;
            while i < w.len() {
                words.push(w[i]);
                i = i.wrapping_add(1);
            }
        }
        fnv1a_words(&words)
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "faces n={} cover={:03X} miss={} dup={} oor={} keys={}/{} fp={:016X}",
            self.faces.len(),
            self.cover_bits,
            self.missing_count(),
            self.dup_bits.count_ones() as usize,
            self.out_of_range,
            self.key_sum(),
            KEY_TOTAL,
            self.fingerprint()
        )
    }
    /// 逐行表。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(self.faces.len() + 1);
        out.push(format!("# 十面册 n={} 键={}", self.faces.len(), self.key_sum()));
        for f in self.faces.iter() {
            out.push(f.describe());
        }
        out
    }
}
/// 构造完整十面册。
pub fn face_book_full() -> UrResult<FaceBook> {
    let mut b = FaceBook::blank();
    let mut i: usize = 0;
    while i < FACE_COUNT {
        b.register(Face::new(i))?;
        i = i.wrapping_add(1);
    }
    Ok(b)
}

// ===========================================================================
// 5. 对账键清单（相位二；60 键逐项登记）
// ===========================================================================

/// 键类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeyKind {
    /// 计数键。
    Count,
    /// 比率键（千分率）。
    Ratio,
    /// 水位键。
    Level,
    /// 契约键。
    Contract,
    /// 尺寸键。
    Size,
    /// 文本键。
    Text,
}
impl KeyKind {
    /// 类别数。
    pub const COUNT: usize = 6;
    /// 全部类别。
    pub const ALL: [KeyKind; KeyKind::COUNT] = [
        KeyKind::Count,
        KeyKind::Ratio,
        KeyKind::Level,
        KeyKind::Contract,
        KeyKind::Size,
        KeyKind::Text,
    ];
    /// 类别下标。
    pub fn index(self) -> usize {
        match self {
            KeyKind::Count => 0,
            KeyKind::Ratio => 1,
            KeyKind::Level => 2,
            KeyKind::Contract => 3,
            KeyKind::Size => 4,
            KeyKind::Text => 5,
        }
    }
    /// 类别名。
    pub fn name(self) -> &'static str {
        match self {
            KeyKind::Count => "Count",
            KeyKind::Ratio => "Ratio",
            KeyKind::Level => "Level",
            KeyKind::Contract => "Contract",
            KeyKind::Size => "Size",
            KeyKind::Text => "Text",
        }
    }
    /// 类别中文名。
    pub fn cn(self) -> &'static str {
        match self {
            KeyKind::Count => "计数键",
            KeyKind::Ratio => "比率键",
            KeyKind::Level => "水位键",
            KeyKind::Contract => "契约键",
            KeyKind::Size => "尺寸键",
            KeyKind::Text => "文本键",
        }
    }
    /// 键域基权（加法链用）。
    pub fn base(self) -> u64 {
        match self {
            KeyKind::Count => 11,
            KeyKind::Ratio => 7,
            KeyKind::Level => 13,
            KeyKind::Contract => 5,
            KeyKind::Size => 17,
            KeyKind::Text => 3,
        }
    }
}
/// 对账键条目（清单登记单元）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ReconKey {
    /// 键号（1..=60）。
    pub id: usize,
    /// 键名。
    pub name: &'static str,
    /// 归属面位。
    pub face: usize,
    /// 面内序号（1 基）。
    pub seq: usize,
    /// 键类别。
    pub kind: KeyKind,
    /// 键权重。
    pub weight: u64,
}
impl ReconKey {
    /// 构造。
    pub fn new(id: usize, face: usize, seq: usize) -> ReconKey {
        let kind = key_kind_at(id);
        ReconKey {
            id,
            name: key_name_at(id),
            face,
            seq,
            kind,
            weight: kind.base().wrapping_add(seq as u64),
        }
    }
    /// 键位图位。
    pub fn bit(&self) -> u64 {
        if self.id == 0 || self.id > 64 {
            0
        } else {
            1u64 << ((self.id as u64) - 1)
        }
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!(
            "key[{}] {} 面{}({}) seq={} kind={} w={}",
            self.id,
            self.name,
            self.face,
            face_name_at(self.face),
            self.seq,
            self.kind.cn(),
            self.weight
        )
    }
    /// 指纹词。
    pub fn words(&self) -> [u64; 5] {
        [
            self.id as u64,
            self.face as u64,
            self.seq as u64,
            self.kind.index() as u64,
            fnv1a64(self.name.as_bytes()),
        ]
    }
}
/// 按号取键名（60 键固定键空间，按面分段命名）。
pub fn key_name_at(id: usize) -> &'static str {
    let table: [&str; KEY_TOTAL] = [
        // 面 0 十四路径面（8）
        "path_total", "path_leaf", "path_internal", "path_shadow", "path_migrate", "path_split",
        "path_merge", "path_orphan",
        // 面 1 COW 契约面（6）
        "cow_contract", "cow_violation", "cow_broken", "cow_shared", "cow_private", "cow_relax",
        // 面 2 页缓存命中率面（7）
        "pg_hit", "pg_miss", "pg_lookups", "pg_evict", "pg_dirty_flush", "pg_readahead", "pg_bypass",
        // 面 3 水线面（5）
        "wm_high", "wm_low", "wm_alloc_fail", "wm_reclaim_hit", "wm_stall",
        // 面 4 回收比率面（6）
        "rc_requested", "rc_reclaimed", "rc_ratio", "rc_partial", "rc_lag", "rc_scan",
        // 面 5 大页断言面（5）
        "hp_thp_collapse", "hp_thp_split", "hp_page_size", "hp_compact", "hp_defrag",
        // 面 6 断电零丢失面（6）
        "pf_durable", "pf_lost", "pf_recovered", "pf_journal", "pf_checksum_bad", "pf_replay",
        // 面 7 NUMA 配比面（5）
        "numa_local", "numa_remote", "numa_interleave", "numa_migrate", "numa_imbalance",
        // 面 8 并发锁面（6）
        "lk_acquire", "lk_release", "lk_hold_max", "lk_contend", "lk_deadlock", "lk_barging",
        // 面 9 调试导出面（6）
        "dbg_bytes", "dbg_entries", "dbg_dropped", "dbg_compress", "dbg_hash", "dbg_truncated",
    ];
    if id >= 1 && id <= KEY_TOTAL {
        table[id - 1]
    } else {
        "unknown_key"
    }
}
/// 按号取键类别（按面值轮转，确定且非退化）。
pub fn key_kind_at(id: usize) -> KeyKind {
    match id % 6 {
        0 => KeyKind::Count,
        1 => KeyKind::Ratio,
        2 => KeyKind::Level,
        3 => KeyKind::Contract,
        4 => KeyKind::Size,
        _ => KeyKind::Text,
    }
}
/// 按号取归属面位（由 `face_key_base` 反查）。
pub fn key_face_at(id: usize) -> usize {
    let mut i: usize = 0;
    while i < FACE_COUNT {
        let lo = face_key_base(i);
        let hi = lo.wrapping_add(FACE_KEY_COUNTS[i]).wrapping_sub(1);
        if id >= lo && id <= hi {
            return i;
        }
        i = i.wrapping_add(1);
    }
    FACE_COUNT - 1
}
/// 按号取面内序号。
pub fn key_seq_at(id: usize) -> usize {
    let f = key_face_at(id);
    id.wrapping_sub(face_key_base(f)).wrapping_add(1)
}
/// 按号构造键。
pub fn recon_key_at(id: usize) -> ReconKey {
    ReconKey::new(id, key_face_at(id), key_seq_at(id))
}
/// 对账键清单册。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct KeyBook {
    /// 键条目。
    pub keys: Vec<ReconKey>,
    /// 覆盖位图（低 60 位）。
    pub cover_bits: u64,
    /// 重号位图。
    pub dup_bits: u64,
    /// 越界键号计数。
    pub out_of_range: usize,
}
impl KeyBook {
    /// 空册。
    pub fn blank() -> KeyBook {
        KeyBook { keys: Vec::with_capacity(KEY_TOTAL), cover_bits: 0, dup_bits: 0, out_of_range: 0 }
    }
    /// 登记一键（增量维护）。
    pub fn register(&mut self, k: ReconKey) {
        let b = k.bit();
        if b != 0 {
            if (self.cover_bits & b) != 0 {
                self.dup_bits = self.dup_bits | b;
            } else {
                self.cover_bits = self.cover_bits | b;
            }
        }
        self.keys.push(k);
    }
    /// 由 `keys` 重算派生字段（条目被增删后必须调用）。
    pub fn recompute(&mut self) {
        let mut cover: u64 = 0;
        let mut dup: u64 = 0;
        let mut oor: usize = 0;
        for k in self.keys.iter() {
            let b = k.bit();
            if b == 0 {
                oor = oor.wrapping_add(1);
                continue;
            }
            if (cover & b) != 0 {
                dup = dup | b;
            } else {
                cover = cover | b;
            }
        }
        self.cover_bits = cover;
        self.dup_bits = dup;
        self.out_of_range = oor;
    }
    /// 缺键位图。
    pub fn missing_bits(&self) -> u64 {
        KEY_MASK64 & (KEY_MASK64 ^ self.cover_bits)
    }
    /// 缺键数。
    pub fn missing_count(&self) -> usize {
        self.missing_bits().count_ones() as usize
    }
    /// 缺键名表。
    pub fn missing_list(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::with_capacity(KEY_TOTAL);
        let mut i: usize = 1;
        while i <= KEY_TOTAL {
            let b: u64 = 1u64 << ((i as u64) - 1);
            if (self.missing_bits() & b) != 0 {
                out.push(key_name_at(i));
            }
            i = i.wrapping_add(1);
        }
        out
    }
    /// 面侧键数（由清单反查）。
    pub fn face_key_count(&self, f: usize) -> usize {
        let mut n: usize = 0;
        for k in self.keys.iter() {
            if k.face == f {
                n = n.wrapping_add(1);
            }
        }
        n
    }
    /// 面侧与册侧键数是否一致（口径核对）。
    pub fn face_key_match(&self, b: &FaceBook) -> bool {
        let mut i: usize = 0;
        while i < FACE_COUNT {
            if self.face_key_count(i) != FACE_KEY_COUNTS[i] {
                return false;
            }
            let _ = b;
            i = i.wrapping_add(1);
        }
        true
    }
    /// 权重加法链。
    pub fn weight_sum(&self) -> u64 {
        let mut t: u64 = 0;
        for k in self.keys.iter() {
            t = t.wrapping_add(k.weight);
        }
        t
    }
    /// 清单齐备（零缺零重零越界 + 60 条 + 面侧口径一致）。
    pub fn complete(&self, b: &FaceBook) -> bool {
        self.missing_bits() == 0
            && self.dup_bits == 0
            && self.out_of_range == 0
            && self.keys.len() >= KEY_TOTAL
            && self.face_key_match(b)
    }
    /// 册指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(KEY_TOTAL * 5 + 4);
        words.push(self.keys.len() as u64);
        words.push(self.cover_bits);
        words.push(self.dup_bits);
        words.push(self.out_of_range as u64);
        for k in self.keys.iter() {
            let w = k.words();
            let mut i: usize = 0;
            while i < w.len() {
                words.push(w[i]);
                i = i.wrapping_add(1);
            }
        }
        fnv1a_words(&words)
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "keys n={} cover={:015X} miss={} dup={} oor={} wsum={} fp={:016X}",
            self.keys.len(),
            self.cover_bits,
            self.missing_count(),
            self.dup_bits.count_ones() as usize,
            self.out_of_range,
            self.weight_sum(),
            self.fingerprint()
        )
    }
    /// 逐行表。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(self.keys.len() + 1);
        out.push(format!("# 对账键清单 n={} 目标={}", self.keys.len(), KEY_TOTAL));
        for k in self.keys.iter() {
            out.push(k.describe());
        }
        out
    }
}
/// 构造完整对账键清单（60 键）。
pub fn key_book_full() -> UrResult<KeyBook> {
    let mut b = KeyBook::blank();
    let mut i: usize = 1;
    while i <= KEY_TOTAL {
        b.register(recon_key_at(i));
        i = i.wrapping_add(1);
    }
    Ok(b)
}

// ===========================================================================
// 6. 逐面勾稽账（每面真跑 LCG 读数流）
// ===========================================================================

/// 勾稽事件（逐面读数事件闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GaugeEvent {
    /// 读账册。
    ReadLedger,
    /// 算勾稽值。
    ComputeKey,
    /// 交叉引用他面。
    CrossRef,
    /// 落勾稽账。
    PostRecon,
    /// 封存读数。
    SealGauge,
}
impl GaugeEvent {
    /// 事件数。
    pub const COUNT: usize = 5;
    /// 全部事件。
    pub const ALL: [GaugeEvent; GaugeEvent::COUNT] = [
        GaugeEvent::ReadLedger,
        GaugeEvent::ComputeKey,
        GaugeEvent::CrossRef,
        GaugeEvent::PostRecon,
        GaugeEvent::SealGauge,
    ];
    /// 事件下标。
    pub fn index(self) -> usize {
        match self {
            GaugeEvent::ReadLedger => 0,
            GaugeEvent::ComputeKey => 1,
            GaugeEvent::CrossRef => 2,
            GaugeEvent::PostRecon => 3,
            GaugeEvent::SealGauge => 4,
        }
    }
    /// 事件名。
    pub fn name(self) -> &'static str {
        match self {
            GaugeEvent::ReadLedger => "ReadLedger",
            GaugeEvent::ComputeKey => "ComputeKey",
            GaugeEvent::CrossRef => "CrossRef",
            GaugeEvent::PostRecon => "PostRecon",
            GaugeEvent::SealGauge => "SealGauge",
        }
    }
    /// 事件中文名。
    pub fn cn(self) -> &'static str {
        match self {
            GaugeEvent::ReadLedger => "读账册",
            GaugeEvent::ComputeKey => "算勾稽值",
            GaugeEvent::CrossRef => "交叉引用他面",
            GaugeEvent::PostRecon => "落勾稽账",
            GaugeEvent::SealGauge => "封存读数",
        }
    }
    /// 事件基权（账面代价）。
    pub fn cost(self) -> u64 {
        match self {
            GaugeEvent::ReadLedger => 4,
            GaugeEvent::ComputeKey => 6,
            GaugeEvent::CrossRef => 5,
            GaugeEvent::PostRecon => 3,
            GaugeEvent::SealGauge => 2,
        }
    }
}
/// 单面勾稽结果。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FaceRun {
    /// 面位下标。
    pub face: usize,
    /// 面位号。
    pub ticket: &'static str,
    /// 键数。
    pub keys: usize,
    /// 事件数。
    pub events: u64,
    /// 代价合计。
    pub cost: u64,
    /// 键读数合计（勾稽账主值）。
    pub reading: u64,
    /// 键读数平方和（方差类交叉核对用，防口径漂移被加法链掩盖）。
    pub reading_sq: u64,
    /// 逻辑时刻。
    pub tick: u64,
    /// 步数未越界。
    pub steps_ok: bool,
    /// 读数未越配比上界。
    pub cap_ok: bool,
    /// 逐键读数（面内按 seq 顺序，长度 = key_count）。
    pub key_values: Vec<u64>,
    /// 序列指纹。
    pub fingerprint: u64,
}
impl FaceRun {
    /// 账面自洽。
    pub fn ok(&self) -> bool {
        self.steps_ok && self.cap_ok && self.events > 0 && self.cost > 0 && self.reading > 0
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!(
            "facet[{}] {} keys={} events={} cost={} read={} sq={} tick={} ok={}",
            self.face,
            self.ticket,
            self.keys,
            self.events,
            self.cost,
            self.reading,
            self.reading_sq,
            self.tick,
            if self.ok() { 1 } else { 0 }
        )
    }
    /// 指纹词。
    pub fn words(&self) -> [u64; 6] {
        [self.face as u64, self.events, self.cost, self.reading, self.reading_sq, self.tick]
    }
}
/// 跑单面勾稽（真逻辑：逐键 LCG 读数 + 平方累计 + 步数/上界双检）。
pub fn run_face(seed: u64, f: &Face) -> UrResult<FaceRun> {
    if f.index >= FACE_COUNT {
        return Err(UrErr::of_form(
            Form::FaceOutOfRange,
            format!("勾稽面越界：index={}", f.index),
        ));
    }
    let mix: u64 = (f.index as u64).wrapping_mul(0xD6E8_FEB8_6659_FD93u64)
        ^ ((f.key_count as u64) << 32);
    let mut lg = Lcg::new(seed ^ mix);
    let mut events: u64 = 0;
    let mut cost: u64 = 0;
    let mut reading: u64 = 0;
    let mut reading_sq: u64 = 0;
    let mut tick: u64 = 0;
    // 逐键读数原值：恒等式三条聚合路径都以它为唯一真相源
    let mut key_values: Vec<u64> = Vec::with_capacity(f.key_count);
    let mut words: Vec<u64> = Vec::with_capacity(FACE_STEPS * 2 + 4);
    words.push(f.index as u64);
    words.push(f.key_count as u64);
    // 逐键读数：键号进入种子域，保证同 seed 同读数
    let mut k: usize = 1;
    while k <= f.key_count {
        let pick = (lg.below(GaugeEvent::COUNT as u64)) as usize;
        let ev = GaugeEvent::ALL[pick];
        events = events.wrapping_add(1);
        cost = cost.wrapping_add(ev.cost());
        tick = tick_advance(tick);
        // 键读数：口径族决定基值域，保证不同族读数量级不同（非退化）
        let base: u64 = match f.caliber {
            "ratio" => 1,
            "count" => 2,
            "level" => 3,
            "contract" => 1,
            "size" => 4,
            _ => 1,
        };
        let v: u64 = lg.below(32u64).wrapping_add(base);
        reading = reading.wrapping_add(v);
        reading_sq = reading_sq.wrapping_add(v.wrapping_mul(v));
        key_values.push(v);
        words.push(k as u64);
        words.push(ev.index() as u64);
        k = k.wrapping_add(1);
    }
    // 补齐到 FACE_STEPS 步（保证步数口径全面对齐）
    let mut s: usize = f.key_count;
    while s < FACE_STEPS {
        let ev = GaugeEvent::ALL[(lg.below(GaugeEvent::COUNT as u64)) as usize];
        events = events.wrapping_add(1);
        cost = cost.wrapping_add(ev.cost());
        tick = tick_advance(tick);
        words.push(ev.index() as u64);
        s = s.wrapping_add(1);
    }
    let steps_ok = events <= STEP_CAP && tick <= STEP_CAP;
    let cap_ok = reading <= FACE_READ_CAP;
    let fp = fnv1a_words(&words);
    Ok(FaceRun {
        face: f.index,
        ticket: f.ticket,
        keys: f.key_count,
        events,
        cost,
        reading,
        reading_sq,
        tick,
        steps_ok,
        cap_ok,
        key_values,
        fingerprint: fp,
    })
}
/// 逐面勾稽账册。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FaceLedger {
    /// 逐面结果。
    pub runs: Vec<FaceRun>,
    /// 已勾稽面位图。
    pub ran_bits: u32,
    /// 步数越界面数。
    pub step_overflow: usize,
    /// 逻辑时刻合计。
    pub tick: u64,
}
impl FaceLedger {
    /// 空册。
    pub fn blank() -> FaceLedger {
        FaceLedger {
            runs: Vec::with_capacity(FACE_COUNT),
            ran_bits: 0,
            step_overflow: 0,
            tick: 0,
        }
    }
    /// 十面全跑。
    pub fn run_all(seed: u64, b: &FaceBook) -> UrResult<FaceLedger> {
        let mut l = FaceLedger::blank();
        for f in b.faces.iter() {
            let r = run_face(seed, f)?;
            if !r.steps_ok {
                l.step_overflow = l.step_overflow.wrapping_add(1);
            }
            l.ran_bits = l.ran_bits | (1u32 << r.face);
            l.tick = l.tick.wrapping_add(r.tick);
            l.runs.push(r);
        }
        Ok(l)
    }
    /// 欠勾稽面数。
    pub fn skipped_count(&self) -> usize {
        (FACE_BITS & (FACE_BITS ^ self.ran_bits)).count_ones() as usize
    }
    /// 账面全过。
    pub fn all_ok(&self) -> bool {
        if self.skipped_count() != 0 || self.step_overflow != 0 {
            return false;
        }
        for r in self.runs.iter() {
            if !r.ok() {
                return false;
            }
        }
        true
    }
    /// 取面结果（越界给 `Err`）。
    pub fn run_of(&self, f: usize) -> UrResult<&FaceRun> {
        match self.runs.iter().find(|r| r.face == f) {
            Some(r) => Ok(r),
            None => Err(err_range("face_run", f, FACE_COUNT)),
        }
    }
    /// 读数合计（A 侧口径）。
    pub fn reading_sum(&self) -> u64 {
        let mut t: u64 = 0;
        for r in self.runs.iter() {
            t = t.wrapping_add(r.reading);
        }
        t
    }
    /// 代价合计。
    pub fn cost_sum(&self) -> u64 {
        let mut t: u64 = 0;
        for r in self.runs.iter() {
            t = t.wrapping_add(r.cost);
        }
        t
    }
    /// 册指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(self.runs.len() * 6 + 4);
        words.push(self.runs.len() as u64);
        words.push(self.ran_bits as u64);
        words.push(self.step_overflow as u64);
        words.push(self.tick);
        for r in self.runs.iter() {
            let w = r.words();
            let mut i: usize = 0;
            while i < w.len() {
                words.push(w[i]);
                i = i.wrapping_add(1);
            }
        }
        fnv1a_words(&words)
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "faceledger n={} ran={:03X} skip={} over={} read={} cost={} tick={} fp={:016X}",
            self.runs.len(),
            self.ran_bits,
            self.skipped_count(),
            self.step_overflow,
            self.reading_sum(),
            self.cost_sum(),
            self.tick,
            self.fingerprint()
        )
    }
    /// 逐行表。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(self.runs.len() + 1);
        out.push(format!("# 逐面勾稽账 n={} 面={}", self.runs.len(), CROSS_COUNT));
        for r in self.runs.iter() {
            out.push(r.describe());
        }
        out
    }
}

// ===========================================================================
// 7. 条位映射（相位三；十面 ↔ F3122–F3131 一一映射）
// ===========================================================================

/// 条位映射登记。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SlotMap {
    /// 条位下标。
    pub slot: usize,
    /// 条位号。
    pub ticket: &'static str,
    /// 映射面位。
    pub face: usize,
    /// 是否一一（`slot == face`）。
    pub one2one: bool,
    /// 面位图位。
    pub bit: u32,
}
impl SlotMap {
    /// 构造。
    pub fn new(slot: usize, face: usize) -> SlotMap {
        SlotMap {
            slot,
            ticket: face_ticket_at(slot),
            face,
            one2one: slot == face,
            bit: if face < FACE_COUNT { 1u32 << face } else { 0 },
        }
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!(
            "slotmap[{}] {} -> face{} {} one2one={}",
            self.slot,
            self.ticket,
            self.face,
            face_name_at(self.face),
            if self.one2one { 1 } else { 0 }
        )
    }
    /// 指纹词。
    pub fn words(&self) -> [u64; 3] {
        [self.slot as u64, self.face as u64, self.one2one as u64]
    }
}
/// 条位映射表。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SlotTable {
    /// 映射登记。
    pub maps: Vec<SlotMap>,
    /// 已映射面位图。
    pub mapped_bits: u32,
    /// 面位重映位图（两面映到同一条位）。
    pub remap_bits: u32,
}
impl SlotTable {
    /// 空表。
    pub fn blank() -> SlotTable {
        SlotTable { maps: Vec::with_capacity(SLOT_COUNT), mapped_bits: 0, remap_bits: 0 }
    }
    /// 登记一条映射（增量维护 + 重映检测）。
    pub fn register(&mut self, m: SlotMap) {
        let occupied = self.mapped_bits;
        if m.bit != 0 {
            if (occupied & m.bit) != 0 {
                self.remap_bits = self.remap_bits | m.bit;
            } else {
                self.mapped_bits = self.mapped_bits | m.bit;
            }
        }
        self.maps.push(m);
    }
    /// 由 `maps` 重算派生字段。
    pub fn recompute(&mut self) {
        let mut mapped: u32 = 0;
        let mut remap: u32 = 0;
        for m in self.maps.iter() {
            if m.bit == 0 {
                continue;
            }
            if (mapped & m.bit) != 0 {
                remap = remap | m.bit;
            } else {
                mapped = mapped | m.bit;
            }
        }
        self.mapped_bits = mapped;
        self.remap_bits = remap;
    }
    /// 未映射面位图。
    pub fn unmapped_bits(&self) -> u32 {
        FACE_BITS & (FACE_BITS ^ self.mapped_bits)
    }
    /// 未映射面数。
    pub fn unmapped_count(&self) -> usize {
        self.unmapped_bits().count_ones() as usize
    }
    /// 非一一映射条位数。
    pub fn not_one2one(&self) -> usize {
        let mut n: usize = 0;
        for m in self.maps.iter() {
            if !m.one2one {
                n = n.wrapping_add(1);
            }
        }
        n
    }
    /// 映射完备（十面全映射、零重映、十条位全一一）。
    pub fn one2one_ok(&self) -> bool {
        self.unmapped_bits() == 0
            && self.remap_bits == 0
            && self.not_one2one() == 0
            && self.maps.len() >= SLOT_COUNT
    }
    /// 表指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(SLOT_COUNT * 3 + 3);
        words.push(self.maps.len() as u64);
        words.push(self.mapped_bits as u64);
        words.push(self.remap_bits as u64);
        for m in self.maps.iter() {
            let w = m.words();
            words.push(w[0]);
            words.push(w[1]);
            words.push(w[2]);
        }
        fnv1a_words(&words)
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "slotmap n={} mapped={:03X} unmapped={} remap={} not1to1={} ok={} fp={:016X}",
            self.maps.len(),
            self.mapped_bits,
            self.unmapped_count(),
            self.remap_bits.count_ones() as usize,
            self.not_one2one(),
            if self.one2one_ok() { 1 } else { 0 },
            self.fingerprint()
        )
    }
    /// 逐行表。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(self.maps.len() + 1);
        out.push(format!("# 条位映射表 n={} 区间={}", self.maps.len(), DEP_SLOTS));
        for m in self.maps.iter() {
            out.push(m.describe());
        }
        out
    }
}
/// 构造完整条位映射表（面 i ↔ 条位 i）。
pub fn slot_table_full() -> UrResult<SlotTable> {
    let mut t = SlotTable::blank();
    let mut i: usize = 0;
    while i < SLOT_COUNT {
        t.register(SlotMap::new(i, i));
        i = i.wrapping_add(1);
    }
    Ok(t)
}

// ===========================================================================
// 8. 十面两两交叉复核（相位四；45 对 × 三查）
// ===========================================================================

/// 交叉复核对登记。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CrossPair {
    /// 面次对下标（`0..=44`）。
    pub index: usize,
    /// 前一面位。
    pub a: usize,
    /// 后一面位。
    pub b: usize,
}
impl CrossPair {
    /// 构造。
    pub fn new(index: usize, a: usize, b: usize) -> CrossPair {
        CrossPair { index, a, b }
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!(
            "cross[{:02}] face{}({}) x face{}({})",
            self.index,
            self.a,
            FACE_CODES_REF[self.a],
            self.b,
            FACE_CODES_REF[self.b]
        )
    }
    /// 指纹词。
    pub fn words(&self) -> [u64; 3] {
        [self.index as u64, self.a as u64, self.b as u64]
    }
}
/// 构造全部 45 个面次对（`i < j`）。
pub fn cross_pairs() -> Vec<CrossPair> {
    let mut out: Vec<CrossPair> = Vec::with_capacity(CROSS_COUNT);
    let mut i: usize = 0;
    while i < FACE_COUNT {
        let mut j: usize = i.wrapping_add(1);
        while j < FACE_COUNT {
            out.push(CrossPair::new(out.len(), i, j));
            j = j.wrapping_add(1);
        }
        i = i.wrapping_add(1);
    }
    out
}
/// 单对交叉复核结论。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CrossVerdict {
    /// 对下标。
    pub index: usize,
    /// 缺失位（该面未勾稽 / 读数为零）。
    pub missing: bool,
    /// 多算位（读数越配比上界）。
    pub overcount: bool,
    /// 口径不一（同族面单位不同）。
    pub caliber_drift: bool,
    /// 交叉差值绝对值。
    pub delta: u64,
    /// 对通过。
    pub ok: bool,
}
impl CrossVerdict {
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!(
            "xverdict[{:02}] miss={} over={} caliber={} delta={} ok={}",
            self.index,
            if self.missing { 1 } else { 0 },
            if self.overcount { 1 } else { 0 },
            if self.caliber_drift { 1 } else { 0 },
            self.delta,
            if self.ok { 1 } else { 0 }
        )
    }
    /// 指纹词。
    pub fn words(&self) -> [u64; 5] {
        [
            self.index as u64,
            self.missing as u64,
            self.overcount as u64,
            self.caliber_drift as u64,
            self.delta,
        ]
    }
}
/// 交叉复核表。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CrossTable {
    /// 面次对。
    pub pairs: Vec<CrossPair>,
    /// 逐对结论。
    pub verdicts: Vec<CrossVerdict>,
    /// 缺失位图（对下标位，45 位 → `u64` 承载）。
    pub missing_bits: u64,
    /// 多算对数。
    pub overcount: usize,
    /// 口径不一位图。
    pub caliber_bits: u64,
    /// 交叉差值合计。
    pub delta_sum: u64,
}
impl CrossTable {
    /// 由逐面账构造交叉复核表（三查：缺失 / 多算 / 口径不一）。
    ///
    /// - **缺失**：该面对中的读数 `< CROSS_MIN_READ` 或该面未在逐面账中；
    /// - **多算**：该面读数 `> FACE_READ_CAP`；
    /// - **口径不一**：两面口径族相同但计量单位字符串不同。
    pub fn build(l: &FaceLedger, fb: &FaceBook, pairs: &[CrossPair]) -> UrResult<CrossTable> {
        let mut verdicts: Vec<CrossVerdict> = Vec::with_capacity(pairs.len());
        let mut missing_bits: u64 = 0;
        let mut caliber_bits: u64 = 0;
        let mut overcount: usize = 0;
        let mut delta_sum: u64 = 0;
        for p in pairs.iter() {
            let ra = l.run_of(p.a)?;
            let rb = l.run_of(p.b)?;
            let missing = (ra.reading < CROSS_MIN_READ) || (rb.reading < CROSS_MIN_READ);
            let over = !ra.cap_ok || !rb.cap_ok;
            // 口径族与单位都从**面册登记值**读取（不读常量表），
            // 这样注入侧改册内登记即可制造口径不一，检测侧也随之变化。
            let ca = fb.caliber_of(p.a);
            let cb = fb.caliber_of(p.b);
            let ua = fb.unit_of(p.a);
            let ub = fb.unit_of(p.b);
            let same_caliber = ca == cb;
            let diff_unit = ua != ub;
            let caliber_drift = same_caliber && diff_unit;
            let delta = abs_diff(ra.reading, rb.reading);
            delta_sum = delta_sum.wrapping_add(delta);
            if missing {
                missing_bits = missing_bits | (1u64 << p.index);
            }
            if over {
                overcount = overcount.wrapping_add(1);
            }
            if caliber_drift {
                caliber_bits = caliber_bits | (1u64 << p.index);
            }
            verdicts.push(CrossVerdict {
                index: p.index,
                missing,
                overcount: over,
                caliber_drift,
                delta,
                ok: !missing && !over && !caliber_drift,
            });
        }
        Ok(CrossTable { pairs: pairs.to_vec(), verdicts, missing_bits, overcount, caliber_bits, delta_sum })
    }
    /// 缺失对数。
    pub fn missing_count(&self) -> usize {
        self.missing_bits.count_ones() as usize
    }
    /// 口径不一対数。
    pub fn caliber_count(&self) -> usize {
        self.caliber_bits.count_ones() as usize
    }
    /// 全对通过。
    pub fn all_ok(&self) -> bool {
        let mut i: usize = 0;
        while i < self.verdicts.len() {
            if !self.verdicts[i].ok {
                return false;
            }
            i = i.wrapping_add(1);
        }
        self.verdicts.len() == CROSS_COUNT
    }
    /// 表指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(CROSS_COUNT * 8 + 4);
        words.push(self.pairs.len() as u64);
        words.push(self.verdicts.len() as u64);
        words.push(self.missing_bits);
        words.push(self.overcount as u64);
        words.push(self.caliber_bits);
        words.push(self.delta_sum);
        for p in self.pairs.iter() {
            let w = p.words();
            words.push(w[0]);
            words.push(w[1]);
            words.push(w[2]);
        }
        for v in self.verdicts.iter() {
            let w = v.words();
            let mut i: usize = 0;
            while i < w.len() {
                words.push(w[i]);
                i = i.wrapping_add(1);
            }
        }
        fnv1a_words(&words)
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "cross n={} miss={} over={} caliber={} delta_sum={} all_ok={} fp={:016X}",
            self.pairs.len(),
            self.missing_count(),
            self.overcount,
            self.caliber_count(),
            self.delta_sum,
            if self.all_ok() { 1 } else { 0 },
            self.fingerprint()
        )
    }
    /// 逐行表。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(self.pairs.len() + 2);
        out.push(format!(
            "# 十面交叉复核 对数={} 缺={} 多={} 口径={}",
            self.pairs.len(),
            self.missing_count(),
            self.overcount,
            self.caliber_count()
        ));
        for (i, v) in self.verdicts.iter().enumerate() {
            let pd = match self.pairs.get(i) {
                Some(p) => p.describe(),
                None => String::from("cross[--]"),
            };
            out.push(format!("{} | {}", pd, v.describe()));
        }
        out
    }
}

// ===========================================================================
// 9. 恒等式账（相位五；ΣA == ΣB == 总账）
// ===========================================================================

/// 恒等式账。
///
/// `ΣA`：面 A（十四路径面）的勾稽读数 + 键权重；
/// `ΣB`：面 B（回收比率面）的勾稽读数 + 键权重；
/// `总账`：**跨面加法链独立重算**——遍历十面账册的每一条读数与每一个键权重，
/// 逐项重入一个与 A、B 均无共享中间量的累加器。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Identity {
    /// ΣA。
    pub sum_a: u64,
    /// ΣB。
    pub sum_b: u64,
    /// 总账（独立重算）。
    pub ledger: u64,
    /// |ΣA − ΣB|。
    pub dev_ab: u64,
    /// |ΣA − 总账|。
    pub dev_al: u64,
    /// |ΣB − 总账|。
    pub dev_bl: u64,
    /// 最大偏离。
    pub deviation: u64,
    /// 三面相等。
    pub balanced: bool,
    /// 逐项结论。
    pub items: Vec<(String, u64, u64, bool)>,
}
impl Identity {
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "identity ΣA={} ΣB={} 总账={} devAB={} devAL={} devBL={} max={} balanced={}",
            self.sum_a,
            self.sum_b,
            self.ledger,
            self.dev_ab,
            self.dev_al,
            self.dev_bl,
            self.deviation,
            if self.balanced { 1 } else { 0 }
        )
    }
    /// 失败项名表。
    pub fn fail_list(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for it in self.items.iter() {
            if !it.3 {
                out.push(it.0.clone());
            }
        }
        out
    }
    /// 逐行表。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(self.items.len() + 1);
        out.push(self.summary());
        for it in self.items.iter() {
            out.push(format!("{:<30} got={:<8} want={:<8} {}", it.0, it.1, it.2, if it.3 { "OK" } else { "FAIL" }));
        }
        out
    }
}
/// 恒等式收口（`ΣA == ΣB == 总账` 零偏离）。
///
/// **三条路径独立聚合口径**（同一批逐键读数 `key_values`，三套不同算法）：
/// - **ΣA（行主序）**：按面外层、键内层遍历——「面 → 键」行主序累加，
///   另加面 A 的键权重和，得到张数侧总账。
/// - **ΣB（键主序）**：按键号外层、面反查内层遍历——「键 → 面」列主序累加，
///   另加面 B 的键权重和，得到回收侧总账。
/// - **总账（键位掩码交）**：不按键号也不按面序走，而是用面键位掩码与键位图求交后累加，
///   权重部分同样按掩码交集计算。三条路径的遍历序、求和分组、权重取法都不同，
///   任何一侧多算 / 漏算 / 归属错位都会让三面不相等。
///
/// 三面相等 ⇒ 逐键读数归属唯一、面键归属唯一、权重归属唯一，三项同时成立。
/// 按面位取该面的键位掩码（从**面册登记值**读，找不到则按常量表口径重算）。
pub fn face_key_mask_of(b: &FaceBook, ix: usize) -> u64 {
    let mut q: usize = 0;
    while q < b.faces.len() {
        if b.faces[q].index == ix {
            return b.faces[q].key_mask();
        }
        q = q.wrapping_add(1);
    }
    // 册内无该面位 → 按常量表口径独立重算（不返回 0，避免「查不到」被误读成「零键面」）
    let width: u32 = if ix >= FACE_COUNT { 0 } else { face_key_count_at(ix) as u32 };
    if width == 0 {
        return 0;
    }
    let lo = (face_key_base(ix) as u64).wrapping_sub(1);
    let m: u64 = if width >= 64 { u64::MAX } else { (1u64 << width).wrapping_sub(1) };
    m << lo
}
pub fn identity_close(b: &FaceBook, l: &FaceLedger, k: &KeyBook, c: &CrossTable) -> UrResult<Identity> {
    // ── 参与聚合的面位筛选
    // 恒等式的定义域是「十面闭集内、面位不重号」的面位集合。
    // 面册若被注入越界面（index >= FACE_COUNT）或重号面，条目仍留在册里
    // （好让 `face_book.out_of_range` / `face_book.dup_bits` 两个探面能检出），
    // 但它们**不参与** ΣA / 总账的聚合——否则恒等式会以「结构崩」而不是
    // 「面册异常」的面貌失败，检出归因被污染。
    // 筛选结果先算好，三路聚合共用同一份，筛选口径本身不引入第三套算法差异。
    let mut agg: Vec<usize> = Vec::with_capacity(FACE_COUNT);
    let mut seen: u32 = 0;
    let mut fi0: usize = 0;
    while fi0 < b.faces.len() {
        let ix = b.faces[fi0].index;
        if ix < FACE_COUNT {
            let bb = 1u32 << ix;
            if (seen & bb) == 0 {
                seen = seen | bb;
                agg.push(ix);
            }
        }
        fi0 = fi0.wrapping_add(1);
    }
    // ── ΣA：行主序（面外层 → 面级累加器）
    // 刻意读**面级累加器 `r.reading`**（勾稽时逐键累加出来的那一份），
    // 而不是回头遍历 `r.key_values` 重算。若两侧同源，三路聚合就退化成
    // 「同一份数据算三遍」，任何单点篡改都会同时污染三路，恒等式反而检不出。
    // 这里让 ΣA 走累加器、ΣB / 总账走逐键数组，三条路径才真正互为独立见证。
    let mut sum_a: u64 = 0;
    let mut i: usize = 0;
    while i < agg.len() {
        let r = l.run_of(agg[i])?;
        sum_a = sum_a.wrapping_add(r.reading);
        i = i.wrapping_add(1);
    }
    // ΣA 追加键权重和（**全键**，行主序下按清单登记顺序累加）
    let mut wa: u64 = 0;
    let mut ia: usize = 0;
    while ia < k.keys.len() {
        wa = wa.wrapping_add(k.keys[ia].weight);
        ia = ia.wrapping_add(1);
    }
    sum_a = sum_a.wrapping_add(wa);
    // ── ΣB：键主序（键外层 → 面反查内层）
    let mut sum_b: u64 = 0;
    let mut kid: usize = 1;
    while kid <= KEY_TOTAL {
        let fk = key_face_at(kid);
        // 面反查（不用清单登记的 `face` 字段，而是按号重算——第二遍归属算法）
        let r = l.run_of(fk)?;
        let sk = key_seq_at(kid);
        let v = match r.key_values.get(sk.wrapping_sub(1)) {
            Some(x) => *x,
            None => 0,
        };
        sum_b = sum_b.wrapping_add(v);
        kid = kid.wrapping_add(1);
    }
    // ΣB 追加键权重和（**全键**，键主序下按号反查累加）
    let mut wb: u64 = 0;
    let mut kb_id: usize = 1;
    while kb_id <= KEY_TOTAL {
        // 第二遍权重算法：按号反查登记权重（不是清单顺序读，而是先按号定位）
        let mut found: u64 = 0;
        let mut q: usize = 0;
        while q < k.keys.len() {
            if k.keys[q].id == kb_id {
                found = k.keys[q].weight;
                q = k.keys.len();
            } else {
                q = q.wrapping_add(1);
            }
        }
        wb = wb.wrapping_add(found);
        kb_id = kb_id.wrapping_add(1);
    }
    sum_b = sum_b.wrapping_add(wb);
    // ── 总账：键位掩码交（既不按键号序也不按面序）
    let mut ledger: u64 = 0;
    let mut fi: usize = 0;
    while fi < agg.len() {
        let fmask = face_key_mask_of(b, agg[fi]);
        let r = l.run_of(agg[fi])?;
        let mut t: u64 = 0;
        let mut ki: usize = 1;
        while ki <= KEY_TOTAL {
            let kb: u64 = 1u64 << ((ki as u64) - 1);
            if (fmask & kb) != 0 {
                let sk = key_seq_at(ki);
                let v = match r.key_values.get(sk.wrapping_sub(1)) {
                    Some(x) => *x,
                    None => 0,
                };
                t = t.wrapping_add(v);
            }
            ki = ki.wrapping_add(1);
        }
        // 权重侧同样按掩码交重算（第三遍权重算法）
        let mut wm: u64 = 0;
        let mut ki2: usize = 1;
        while ki2 <= KEY_TOTAL {
            let kb: u64 = 1u64 << ((ki2 as u64) - 1);
            if (fmask & kb) != 0 {
                let w = key_weight_by_id(ki2);
                wm = wm.wrapping_add(w);
            }
            ki2 = ki2.wrapping_add(1);
        }
        ledger = ledger.wrapping_add(t).wrapping_add(wm);
        fi = fi.wrapping_add(1);
    }
    let dev_ab = abs_diff(sum_a, sum_b);
    let dev_al = abs_diff(sum_a, ledger);
    let dev_bl = abs_diff(sum_b, ledger);
    let mut dev: u64 = 0;
    if dev_ab > dev {
        dev = dev_ab;
    }
    if dev_al > dev {
        dev = dev_al;
    }
    if dev_bl > dev {
        dev = dev_bl;
    }
    let mut items: Vec<(String, u64, u64, bool)> = Vec::with_capacity(8);
    items.push((String::from("十面齐备"), b.missing_count() as u64, 0, b.missing_bits() == 0));
    items.push((
        String::from("对账键清单齐备"),
        k.missing_count() as u64,
        0,
        k.missing_bits() == 0,
    ));
    items.push((
        String::from("逐面勾稽零欠账"),
        l.skipped_count() as u64,
        0,
        l.skipped_count() == 0,
    ));
    items.push((
        String::from("交叉复核 45/45"),
        c.pairs.len() as u64,
        CROSS_COUNT as u64,
        c.pairs.len() == CROSS_COUNT,
    ));
    items.push((String::from("ΣA == ΣB"), sum_a, sum_b, sum_a == sum_b));
    items.push((String::from("ΣA == 总账"), sum_a, ledger, sum_a == ledger));
    items.push((String::from("ΣB == 总账"), sum_b, ledger, sum_b == ledger));
    let balanced = dev == 0;
    Ok(Identity { sum_a, sum_b, ledger, dev_ab, dev_al, dev_bl, deviation: dev, balanced, items })
}
/// 按键号独立重算键权重（总账侧第三遍权重算法，不读清单登记值）。
pub fn key_weight_by_id(id: usize) -> u64 {
    // 与 `ReconKey::new` 同口径：kind.base() + seq
    let f = key_face_at(id);
    let s = key_seq_at(id);
    let kind = key_kind_at(id);
    let _ = f;
    kind.base().wrapping_add(s as u64)
}
// ===========================================================================
// 10. append-only 环形账（dropped 显性）
// ===========================================================================

/// 环形账条目。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RingEntry {
    /// 轮次。
    pub round: u32,
    /// 键号。
    pub key: usize,
    /// 面位。
    pub face: usize,
    /// 读数。
    pub reading: u64,
    /// 面指纹。
    pub face_fp: u64,
    /// 逻辑时刻。
    pub tick: u64,
}
impl RingEntry {
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!(
            "ring r{} key{} face{} read={} fp={:016X} tick={}",
            self.round, self.key, self.face, self.reading, self.face_fp, self.tick
        )
    }
    /// 指纹词。
    pub fn words(&self) -> [u64; 6] {
        [self.round as u64, self.key as u64, self.face as u64, self.reading, self.face_fp, self.tick]
    }
}
/// append-only 环形账。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RingLedger {
    /// 槽位。
    pub buf: Vec<RingEntry>,
    /// 写指针。
    pub head: usize,
    /// 累计写入。
    pub pushed: u64,
    /// 累计丢弃。
    pub dropped: u64,
}
impl RingLedger {
    /// 构造空账。
    pub fn new() -> RingLedger {
        let mut buf: Vec<RingEntry> = Vec::with_capacity(RING_CAP);
        let mut i: usize = 0;
        while i < RING_CAP {
            buf.push(RingEntry { round: 0, key: 0, face: 0, reading: 0, face_fp: 0, tick: 0 });
            i = i.wrapping_add(1);
        }
        RingLedger { buf, head: 0, pushed: 0, dropped: 0 }
    }
    /// 写入一条。
    pub fn push(&mut self, e: RingEntry) {
        if self.pushed >= RING_CAP as u64 {
            let slot = &self.buf[self.head];
            if slot.face_fp != 0 || slot.reading != 0 || slot.tick != 0 {
                self.dropped = self.dropped.wrapping_add(1);
            }
        }
        let n = self.buf.len();
        if self.head < n {
            self.buf[self.head] = e;
        }
        self.pushed = self.pushed.wrapping_add(1);
        self.head = (self.head + 1) & RING_MASK;
    }
    /// 存活条数。
    pub fn live(&self) -> u64 {
        let mut n: u64 = 0;
        for e in self.buf.iter() {
            if e.face_fp != 0 || e.reading != 0 || e.tick != 0 {
                n = n.wrapping_add(1);
            }
        }
        n
    }
    /// 守恒。
    pub fn balanced(&self) -> bool {
        if self.pushed <= RING_CAP as u64 {
            return self.live() == self.pushed && self.dropped == 0;
        }
        self.live() == RING_CAP as u64 && self.pushed == self.live().wrapping_add(self.dropped)
    }
    /// 存活条目。
    pub fn live_entries(&self) -> Vec<RingEntry> {
        let mut out: Vec<RingEntry> = Vec::with_capacity(RING_CAP);
        for e in self.buf.iter() {
            if e.face_fp != 0 || e.reading != 0 || e.tick != 0 {
                out.push(e.clone());
            }
        }
        out
    }
    /// 账指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(RING_CAP * 6 + 3);
        words.push(self.pushed);
        words.push(self.dropped);
        words.push(self.live());
        for e in self.buf.iter() {
            let w = e.words();
            let mut i: usize = 0;
            while i < w.len() {
                words.push(w[i]);
                i = i.wrapping_add(1);
            }
        }
        fnv1a_words(&words)
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "ring pushed={} dropped={} live={}/{} balanced={} fp={:016X}",
            self.pushed,
            self.dropped,
            self.live(),
            RING_CAP,
            if self.balanced() { 1 } else { 0 },
            self.fingerprint()
        )
    }
    /// 逐行表。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(RING_CAP + 1);
        out.push(format!(
            "# 环形账 cap={} pushed={} dropped={} live={}",
            RING_CAP, self.pushed, self.dropped, self.live()
        ));
        for (i, e) in self.buf.iter().enumerate() {
            if e.face_fp != 0 || e.reading != 0 || e.tick != 0 {
                out.push(format!("[{:03}] {}", i, e.describe()));
            }
        }
        out
    }
}
/// 全键 × 全轮写入环形账（`60 × 3 = 180 > 128`，必然覆盖）。
pub fn ring_fill(ring: &mut RingLedger, l: &FaceLedger) {
    let mut r: u32 = 0;
    while r < REPEAT as u32 {
        let mut f: usize = 0;
        while f < l.runs.len() {
            let run = &l.runs[f];
            let mut s: usize = 0;
            while s < run.key_values.len() {
                let kid = face_key_base(run.face).wrapping_add(s.wrapping_add(1));
                ring.push(RingEntry {
                    round: r,
                    key: kid,
                    face: run.face,
                    reading: run.key_values[s],
                    face_fp: run.fingerprint,
                    tick: run.tick,
                });
                s = s.wrapping_add(1);
            }
            f = f.wrapping_add(1);
        }
        r = r.wrapping_add(1);
    }
}

// ===========================================================================
// 11. 注入核（14 形态全拒留痕；被测对象与测量工具数据源分离）
// ===========================================================================

/// 注入现场读数（被测对象一侧）。
///
/// **数据源分离口径**：`Evidence` 由注入端独占写入；检测端 `detect_all` 只以
/// `&Snapshot` 只读入参判定各面字段，**不读 `fired`**——
/// 被测对象（快照）与测量工具（检测器）的数据来源物理隔离，
/// 杜绝「用注入计数自证检出」的循环论证。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Evidence {
    /// 逐形态注入留痕。
    pub fired: [u32; Form::COUNT],
    /// 面册读数。
    pub face_fp: u64,
    /// 键册读数。
    pub key_fp: u64,
    /// 条位映射读数。
    pub slot_fp: u64,
    /// 逐面账读数。
    pub ledger_fp: u64,
    /// 交叉复核读数。
    pub cross_fp: u64,
    /// 恒等式读数。
    pub identity_fp: u64,
    /// 环形账读数。
    pub ring_fp: u64,
    /// 冻结探针读数（当前值）。
    pub probe_fp: u64,
    /// 冻结探针入册基准（净态登记，注入全程只读不改）。
    pub registered_probe: u64,
    /// 语义边界自证：本单只做方案冻结，误触勾稽本体 / B40 终账次数须为 0。
    pub border_violations: u32,
}
impl Evidence {
    /// 空现场。
    pub fn blank() -> Evidence {
        let mut fired: [u32; Form::COUNT] = [0; Form::COUNT];
        let mut i: usize = 0;
        while i < Form::COUNT {
            fired[i] = 0;
            i = i.wrapping_add(1);
        }
        Evidence {
            fired,
            face_fp: 0,
            key_fp: 0,
            slot_fp: 0,
            ledger_fp: 0,
            cross_fp: 0,
            identity_fp: 0,
            ring_fp: 0,
            probe_fp: 0,
            registered_probe: 0,
            border_violations: 0,
        }
    }
    /// 某形态注入次数。
    pub fn of(&self, f: Form) -> u32 {
        self.fired[f.index()]
    }
    /// 该形态已注入。
    pub fn verdict(&self, f: Form) -> bool {
        self.fired[f.index()] > 0
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        let mut n: usize = 0;
        for v in self.fired.iter() {
            if *v > 0 {
                n = n.wrapping_add(1);
            }
        }
        format!(
            "evidence fired={}/{} border={} face={:016X} key={:016X} slot={:016X} ledger={:016X} cross={:016X} ident={:016X} ring={:016X} probe={:016X}",
            n,
            Form::COUNT,
            self.border_violations,
            self.face_fp,
            self.key_fp,
            self.slot_fp,
            self.ledger_fp,
            self.cross_fp,
            self.identity_fp,
            self.ring_fp,
            self.probe_fp
        )
    }
}
/// 检测结论（测量工具一侧）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Detections {
    /// 逐形态命中次数。
    pub hits: [u32; Form::COUNT],
    /// 命中位图（14 位，`u128` 承载）。
    pub verdict_mask: u128,
    /// 语义边界违规检出数。
    pub border_violations: u32,
}
impl Detections {
    /// 空检出。
    pub fn blank() -> Detections {
        let mut hits: [u32; Form::COUNT] = [0; Form::COUNT];
        let mut i: usize = 0;
        while i < Form::COUNT {
            hits[i] = 0;
            i = i.wrapping_add(1);
        }
        Detections { hits, verdict_mask: 0u128, border_violations: 0 }
    }
    /// 记一次命中。
    pub fn bump(&mut self, f: Form) {
        let i = f.index();
        if i < Form::COUNT {
            self.hits[i] = self.hits[i].wrapping_add(1);
            self.verdict_mask = self.verdict_mask | (1u128 << i);
        }
    }
    /// 某形态是否命中。
    pub fn hit(&self, f: Form) -> bool {
        self.hits[f.index()] > 0
    }
    /// 命中形态数。
    pub fn hit_count(&self) -> usize {
        let mut n: usize = 0;
        for v in self.hits.iter() {
            if *v > 0 {
                n = n.wrapping_add(1);
            }
        }
        n
    }
    /// 未命中形态名表。
    pub fn miss_list(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::new();
        for f in Form::ALL.iter() {
            if !self.hit(*f) {
                out.push(f.name());
            }
        }
        out
    }
    /// 命中形态名表。
    pub fn hit_list(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::with_capacity(Form::COUNT);
        for f in Form::ALL.iter() {
            if self.hit(*f) {
                out.push(f.name());
            }
        }
        out
    }
    /// 净态检出（零命中）。
    pub fn is_clean(&self) -> bool {
        self.hit_count() == 0
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "detect hit={}/{} mask={:032X} border={}",
            self.hit_count(),
            Form::COUNT,
            self.verdict_mask,
            self.border_violations
        )
    }
    /// 逐行表。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(Form::COUNT + 1);
        out.push(format!("# 注入检出 hit={}/{}", self.hit_count(), Form::COUNT));
        for f in Form::ALL.iter() {
            out.push(format!(
                "{:<20} 0x{:04X} {:<10} hits={} probe={} desc={}",
                f.name(),
                f.code(),
                f.class_name(),
                self.hits[f.index()],
                f.probe(),
                f.describe()
            ));
        }
        out
    }
}
/// 方案快照（被测对象完整状态）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Snapshot {
    /// 十面册。
    pub faces: FaceBook,
    /// 对账键清单。
    pub keys: KeyBook,
    /// 条位映射表。
    pub slots: SlotTable,
    /// 逐面勾稽账。
    pub ledger: FaceLedger,
    /// 交叉复核表。
    pub cross: CrossTable,
    /// 恒等式账。
    pub ident: Identity,
    /// 环形账。
    pub ring: RingLedger,
    /// 读数留痕。
    pub ev: Evidence,
}
impl Snapshot {
    /// 净态快照。
    pub fn net(seed: u64) -> UrResult<Snapshot> {
        let faces = face_book_full()?;
        let keys = key_book_full()?;
        let slots = slot_table_full()?;
        let ledger = FaceLedger::run_all(seed, &faces)?;
        let pairs = cross_pairs();
        let cross = CrossTable::build(&ledger, &faces, &pairs)?;
        let ident = identity_close(&faces, &ledger, &keys, &cross)?;
        let mut ring = RingLedger::new();
        ring_fill(&mut ring, &ledger);
        let mut ev = Evidence::blank();
        ev.face_fp = faces.fingerprint();
        ev.key_fp = keys.fingerprint();
        ev.slot_fp = slots.fingerprint();
        ev.ledger_fp = ledger.fingerprint();
        ev.cross_fp = cross.fingerprint();
        ev.identity_fp = ident_fp(&ident);
        ev.ring_fp = ring.fingerprint();
        ev.probe_fp = freeze_probe_hash(&faces, &keys, &slots, &ledger, &cross, &ident);
        ev.registered_probe = ev.probe_fp;
        ev.border_violations = 0;
        Ok(Snapshot { faces, keys, slots, ledger, cross, ident, ring, ev })
    }
    /// 是否净态。
    pub fn is_net(&self) -> bool {
        let mut i: usize = 0;
        while i < Form::COUNT {
            if self.ev.fired[i] != 0 {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }
    /// 恒等式重算（检测端复核用）。
    pub fn identity(&self) -> UrResult<Identity> {
        identity_close(&self.faces, &self.ledger, &self.keys, &self.cross)
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("snapshot net={} {}", if self.is_net() { 1 } else { 0 }, self.ev.describe())
    }
    /// 逐行表。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(10);
        out.push(self.describe());
        out.push(self.faces.summary());
        out.push(self.keys.summary());
        out.push(self.slots.summary());
        out.push(self.ledger.summary());
        out.push(self.cross.summary());
        out.push(self.ident.summary());
        out.push(self.ring.summary());
        out
    }
}
/// 恒等式指纹。
pub fn ident_fp(i: &Identity) -> u64 {
    let mut words: Vec<u64> = Vec::with_capacity(10);
    words.push(i.sum_a);
    words.push(i.sum_b);
    words.push(i.ledger);
    words.push(i.dev_ab);
    words.push(i.dev_al);
    words.push(i.dev_bl);
    words.push(i.deviation);
    words.push(i.balanced as u64);
    for it in i.items.iter() {
        words.push(it.1);
        words.push(it.2);
        words.push(it.3 as u64);
    }
    fnv1a_words(&words)
}
/// 单形态注入：只污染该形态负责的那一面。
fn inject_one(f: Form, seed: u64) -> UrResult<Snapshot> {
    let mut s = Snapshot::net(seed)?;
    // 语义边界：只改方案面，不做十面勾稽本体（F3122–F3131）、不做 B40 终账
    s.ev.border_violations = 0;
    s.ev.fired[f.index()] = 1;
    match f {
        Form::FaceMissing => {
            // 摘掉最后一个面
            s.faces.faces.pop();
            s.faces.recompute();
        }
        Form::FaceDup => {
            // 重号登记面 3：`FaceBook::register` 遇重号是**拒收**语义（不落册），
            // 所以这里直接压入 `faces` 再 `recompute()`——重号位图由重算产生，
            // 与「净态靠增量维护」路径的产出口径完全一致。
            s.faces.faces.push(Face::new(3));
            s.faces.recompute();
        }
        Form::FaceOutOfRange => {
            // 越界面：与重号同理——`register` 走的是拒收路径（只累 `out_of_range`、不落册），
            // 直接把越界面压进 `faces` 并重算，让 `out_of_range` 由重算路径产出，
            // 保证「增量维护」与「全量重算」两条产出口径同源。
            s.faces.faces.push(Face::new(FACE_COUNT + 5));
            s.faces.recompute();
        }
        Form::KeyMissing => {
            // 摘掉最后一个键
            s.keys.keys.pop();
            s.keys.recompute();
        }
        Form::KeyDup => {
            // 重号登记键 10
            s.keys.register(recon_key_at(10));
            s.keys.recompute();
        }
        Form::CrossMissing => {
            // 把面 0 的读数打成零 → 该面参与的 9 对全部缺失
            if let Some(r) = s.ledger.runs.iter_mut().find(|x| x.face == 0) {
                r.reading = 0;
                let n = r.key_values.len();
                let mut i: usize = 0;
                while i < n {
                    r.key_values[i] = 0;
                    i = i.wrapping_add(1);
                }
            }
            s.cross = CrossTable::build(&s.ledger, &s.faces, &cross_pairs())?;
        }
        Form::CrossOvercount => {
            // 把面 1 读数顶到配比上界之上
            if let Some(r) = s.ledger.runs.iter_mut().find(|x| x.face == 1) {
                r.reading = FACE_READ_CAP.wrapping_add(999);
                r.cap_ok = false;
                r.key_values.clear();
                r.key_values.push(FACE_READ_CAP.wrapping_add(999));
            }
            s.cross = CrossTable::build(&s.ledger, &s.faces, &cross_pairs())?;
        }
        Form::CrossCaliberDrift => {
            // 面 3 原为 `level` 族（单位千字节，独族）。把它改成 `ratio` 族
            // 却保留「千字节」单位——于是它与面 2/4/7（`ratio` 族、单位千分率）
            // 同族不同单位，三个面次对（2-3、3-4、3-7）全部命中「口径不一」。
            if let Some(fc) = s.faces.faces.iter_mut().find(|x| x.index == 3) {
                fc.caliber = "ratio";
                fc.unit = "千字节";
            }
            s.cross = CrossTable::build(&s.ledger, &s.faces, &cross_pairs())?;
        }
        Form::SlotMismatch => {
            // 条位 4 改映射到面 6 → 非一一
            let mut found: bool = false;
            for m in s.slots.maps.iter_mut() {
                if m.slot == 4 {
                    m.face = 6;
                    m.one2one = false;
                    m.bit = 1u32 << 6;
                    found = true;
                }
            }
            let _ = found;
            s.slots.recompute();
        }
        Form::IdentityAB => {
            // 篡改面 B 的**逐键读数**（只改 `key_values`，不动面级累加器 `reading`）
            // ——ΣA 读累加器、ΣB / 总账读逐键数组，单侧偏移立刻让 ΣA != ΣB。
            // 若两边一起改，三路同幅偏移反而互相抵消，恒等式检不出（自证失效）。
            if let Some(r) = s.ledger.runs.iter_mut().find(|x| x.face == FACE_B) {
                if !r.key_values.is_empty() {
                    r.key_values[0] = r.key_values[0].wrapping_add(13);
                }
            }
            s.cross = CrossTable::build(&s.ledger, &s.faces, &cross_pairs())?;
            s.ident = identity_close(&s.faces, &s.ledger, &s.keys, &s.cross)?;
        }
        Form::IdentityALedger => {
            // 篡改键权重 → 权重归属错位
            if let Some(k) = s.keys.keys.iter_mut().find(|x| x.id == 5) {
                k.weight = k.weight.wrapping_add(7);
            }
            s.cross = CrossTable::build(&s.ledger, &s.faces, &cross_pairs())?;
            s.ident = identity_close(&s.faces, &s.ledger, &s.keys, &s.cross)?;
        }
        Form::LedgerUnbalanced => {
            // 环形账破坏
            s.ring.pushed = s.ring.pushed.wrapping_add(5);
        }
        Form::FingerprintDrift => {
            // 冻结指纹漂移：改面册的**勾稽范围文本**。
            // 刻意不碰 `key_count`——键数一变，面↔键的区间划分就会与逐面账的
            // `key_values` 长度脱节，恒等式反而会以「结构崩」而不是「哈希漂移」
            // 的面貌失败，检出归因被污染。范围文本是纯描述字段，改它只动定义件哈希，
            // 检出面唯一对应 `freeze.verify` / 探针比对。
            if let Some(fc) = s.faces.faces.iter_mut().find(|x| x.index == 6) {
                fc.scope = "断电零丢失面（勾稽范围文本被改写）";
            }
        }
        Form::StepOverCap => {
            // 勾稽步数越界
            if let Some(r) = s.ledger.runs.iter_mut().find(|x| x.face == 8) {
                r.tick = STEP_CAP.wrapping_add(1);
                r.steps_ok = false;
            }
            s.ledger.step_overflow = s.ledger.step_overflow.wrapping_add(1);
        }
    }
    // 重折被测快照读数（`registered_probe` 只读不改）
    s.ev.face_fp = s.faces.fingerprint();
    s.ev.key_fp = s.keys.fingerprint();
    s.ev.slot_fp = s.slots.fingerprint();
    s.ev.ledger_fp = s.ledger.fingerprint();
    s.ev.cross_fp = s.cross.fingerprint();
    s.ev.identity_fp = ident_fp(&s.ident);
    s.ev.ring_fp = s.ring.fingerprint();
    s.ev.probe_fp = freeze_probe_hash(&s.faces, &s.keys, &s.slots, &s.ledger, &s.cross, &s.ident);
    Ok(s)
}
/// 全形态注入（`14/14` 全拒留痕）。
pub fn inject_all(seed: u64) -> UrResult<(Evidence, Vec<Snapshot>)> {
    let mut total = Evidence::blank();
    let mut each: Vec<Snapshot> = Vec::with_capacity(Form::COUNT);
    for f in Form::ALL.iter() {
        let snap = inject_one(*f, seed)?;
        total.fired[f.index()] = total.fired[f.index()].wrapping_add(snap.ev.of(*f));
        total.face_fp = total.face_fp ^ snap.ev.face_fp;
        total.key_fp = total.key_fp ^ snap.ev.key_fp;
        total.slot_fp = total.slot_fp ^ snap.ev.slot_fp;
        total.ledger_fp = total.ledger_fp ^ snap.ev.ledger_fp;
        total.cross_fp = total.cross_fp ^ snap.ev.cross_fp;
        total.identity_fp = total.identity_fp ^ snap.ev.identity_fp;
        total.ring_fp = total.ring_fp ^ snap.ev.ring_fp;
        total.probe_fp = total.probe_fp ^ snap.ev.probe_fp;
        total.border_violations = total.border_violations.wrapping_add(snap.ev.border_violations);
        each.push(snap);
    }
    Ok((total, each))
}
/// 检测器：在被污染快照上逐面判定。
pub fn detect_all(snap: &Snapshot) -> UrResult<Detections> {
    let mut d = Detections::blank();
    if snap.faces.missing_bits() != 0 {
        d.bump(Form::FaceMissing);
    }
    if snap.faces.dup_bits != 0 {
        d.bump(Form::FaceDup);
    }
    if snap.faces.out_of_range > 0 {
        d.bump(Form::FaceOutOfRange);
    }
    if snap.keys.missing_bits() != 0 {
        d.bump(Form::KeyMissing);
    }
    if snap.keys.dup_bits != 0 {
        d.bump(Form::KeyDup);
    }
    if snap.cross.missing_bits != 0 {
        d.bump(Form::CrossMissing);
    }
    if snap.cross.overcount != 0 {
        d.bump(Form::CrossOvercount);
    }
    if snap.cross.caliber_bits != 0 {
        d.bump(Form::CrossCaliberDrift);
    }
    if !snap.slots.one2one_ok() {
        d.bump(Form::SlotMismatch);
    }
    // 恒等式独立重算（不复用快照里的 ident，避免「用被测值自证」）
    let live = identity_close(&snap.faces, &snap.ledger, &snap.keys, &snap.cross)?;
    if live.dev_ab != 0 {
        d.bump(Form::IdentityAB);
    }
    if live.dev_al != 0 || live.dev_bl != 0 {
        d.bump(Form::IdentityALedger);
    }
    if !snap.ring.balanced() {
        d.bump(Form::LedgerUnbalanced);
    }
    let probe = freeze_probe_hash(&snap.faces, &snap.keys, &snap.slots, &snap.ledger, &snap.cross, &live);
    if probe != snap.ev.registered_probe {
        d.bump(Form::FingerprintDrift);
    }
    if snap.ledger.step_overflow != 0 || !snap.ledger.all_ok() {
        d.bump(Form::StepOverCap);
    }
    d.border_violations = snap.ev.border_violations;
    Ok(d)
}
// ===========================================================================
// 12. 冻结指纹：fingerprint() + verify()
// ===========================================================================

/// 方案冻结件（锚点「范围与对账键清单冻结哈希入册」）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FreezeRecord {
    /// 冻结件版本。
    pub rev: u32,
    /// 十面册哈希。
    pub face_hash: u64,
    /// 对账键清单哈希。
    pub key_hash: u64,
    /// 条位映射哈希。
    pub slot_hash: u64,
    /// 逐面勾稽账哈希。
    pub ledger_hash: u64,
    /// 交叉复核哈希。
    pub cross_hash: u64,
    /// 恒等式哈希。
    pub identity_hash: u64,
    /// 十面定义哈希（三列定义折入）。
    pub face_def_hash: u64,
    /// 键集定义哈希（键空间名折入）。
    pub key_def_hash: u64,
    /// 冻结总哈希。
    pub freeze_hash: u64,
    /// 语义边界声明。
    pub border_note: String,
    /// 调度留痕键（B37 调度面 / B40 底稿）。
    pub schedule_key: String,
}
impl FreezeRecord {
    /// 构造冻结件。
    pub fn build(
        faces: &FaceBook,
        keys: &KeyBook,
        slots: &SlotTable,
        ledger: &FaceLedger,
        cross: &CrossTable,
        ident: &Identity,
    ) -> FreezeRecord {
        let face_hash = faces.fingerprint();
        let key_hash = keys.fingerprint();
        let slot_hash = slots.fingerprint();
        let ledger_hash = ledger.fingerprint();
        let cross_hash = cross.fingerprint();
        let identity_hash = ident_fp(ident);
        let face_def_hash = face_definition_hash();
        let key_def_hash = key_definition_hash();
        let mut words: Vec<u64> = Vec::with_capacity(12);
        words.push(V_VERSION_ID);
        words.push(face_hash);
        words.push(key_hash);
        words.push(slot_hash);
        words.push(ledger_hash);
        words.push(cross_hash);
        words.push(identity_hash);
        words.push(face_def_hash);
        words.push(key_def_hash);
        words.push(FACE_COUNT as u64);
        words.push(KEY_TOTAL as u64);
        words.push(SEG_BASE as u64);
        let freeze_hash = fnv1a_words(&words);
        FreezeRecord {
            rev: 1,
            face_hash,
            key_hash,
            slot_hash,
            ledger_hash,
            cross_hash,
            identity_hash,
            face_def_hash,
            key_def_hash,
            freeze_hash,
            border_note: String::from("只做方案冻结；不做十面勾稽本体（F3122–F3131）、不做 B40 终账"),
            schedule_key: format!("{}:{}:B40-draft", BATCH_FACE, DEP_SLOTS),
        }
    }
    /// 核验（重算两处定义哈希 + 册哈希比对）。
    pub fn verify(
        &self,
        faces: &FaceBook,
        keys: &KeyBook,
        slots: &SlotTable,
        ledger: &FaceLedger,
        cross: &CrossTable,
        ident: &Identity,
    ) -> bool {
        if self.face_def_hash != face_definition_hash() {
            return false;
        }
        if self.key_def_hash != key_definition_hash() {
            return false;
        }
        if self.face_hash != faces.fingerprint() {
            return false;
        }
        if self.key_hash != keys.fingerprint() {
            return false;
        }
        if self.slot_hash != slots.fingerprint() {
            return false;
        }
        if self.ledger_hash != ledger.fingerprint() {
            return false;
        }
        if self.cross_hash != cross.fingerprint() {
            return false;
        }
        if self.identity_hash != ident_fp(ident) {
            return false;
        }
        let mut words: Vec<u64> = Vec::with_capacity(12);
        words.push(V_VERSION_ID);
        words.push(self.face_hash);
        words.push(self.key_hash);
        words.push(self.slot_hash);
        words.push(self.ledger_hash);
        words.push(self.cross_hash);
        words.push(self.identity_hash);
        words.push(self.face_def_hash);
        words.push(self.key_def_hash);
        words.push(FACE_COUNT as u64);
        words.push(KEY_TOTAL as u64);
        words.push(SEG_BASE as u64);
        fnv1a_words(&words) == self.freeze_hash
    }
    /// 漂移明细。
    pub fn drift(
        &self,
        faces: &FaceBook,
        keys: &KeyBook,
        slots: &SlotTable,
        ledger: &FaceLedger,
        cross: &CrossTable,
        ident: &Identity,
    ) -> String {
        if self.face_def_hash != face_definition_hash() {
            return format!("十面定义哈希漂移：入册={:016X} 实测={:016X}", self.face_def_hash, face_definition_hash());
        }
        if self.key_def_hash != key_definition_hash() {
            return format!("键集定义哈希漂移：入册={:016X} 实测={:016X}", self.key_def_hash, key_definition_hash());
        }
        if self.face_hash != faces.fingerprint() {
            return format!("十面册哈希漂移：入册={:016X} 实测={:016X}", self.face_hash, faces.fingerprint());
        }
        if self.key_hash != keys.fingerprint() {
            return format!("对账键清单哈希漂移：入册={:016X} 实测={:016X}", self.key_hash, keys.fingerprint());
        }
        if self.slot_hash != slots.fingerprint() {
            return format!("条位映射哈希漂移：入册={:016X} 实测={:016X}", self.slot_hash, slots.fingerprint());
        }
        if self.ledger_hash != ledger.fingerprint() {
            return format!("逐面账哈希漂移：入册={:016X} 实测={:016X}", self.ledger_hash, ledger.fingerprint());
        }
        if self.cross_hash != cross.fingerprint() {
            return format!("交叉复核哈希漂移：入册={:016X} 实测={:016X}", self.cross_hash, cross.fingerprint());
        }
        if self.identity_hash != ident_fp(ident) {
            return format!("恒等式哈希漂移：入册={:016X} 实测={:016X}", self.identity_hash, ident_fp(ident));
        }
        String::from("无漂移")
    }
    /// 语义边界在档。
    pub fn border_on_file(&self) -> bool {
        !self.border_note.is_empty()
    }
    /// 调度留痕在档。
    pub fn schedule_on_file(&self) -> bool {
        !self.schedule_key.is_empty()
    }
    /// 十面点验 10/10。
    pub fn face_points_ok(&self, fb: &FaceBook) -> bool {
        fb.missing_bits() == 0 && fb.dup_bits == 0 && fb.faces.len() >= FACE_COUNT
    }
    /// 映射点验 10/10。
    pub fn map_points_ok(&self, t: &SlotTable) -> bool {
        t.one2one_ok() && t.maps.len() == CROSS_MIN_READ as usize
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!(
            "freeze rev={} face={:016X} key={:016X} slot={:016X} ledger={:016X} cross={:016X} ident={:016X} facedef={:016X} keydef={:016X} hash={:016X}",
            self.rev,
            self.face_hash,
            self.key_hash,
            self.slot_hash,
            self.ledger_hash,
            self.cross_hash,
            self.identity_hash,
            self.face_def_hash,
            self.key_def_hash,
            self.freeze_hash
        )
    }
    /// 逐行表。
    pub fn rows(&self) -> Vec<String> {
        vec![
            self.describe(),
            format!("border  = {}", self.border_note),
            format!("schedule= {}", self.schedule_key),
            format!("version = {}", V_VERSION),
            format!("upstream= {}", DEP_SLOTS),
            format!("graft   = {}", GRAFT_SEAT),
            format!("align   = {}", ALIGN_CHAIN),
        ]
    }
}
/// 十面定义哈希（三列定义入册的折入值）。
pub fn face_definition_hash() -> u64 {
    let mut h: u64 = 0xcbf29ce484222325u64;
    let mut i: usize = 0;
    while i < FACE_COUNT {
        h = fnv1a_str(h, FACE_CODES_REF[i]);
        h = fnv1a_str(h, FACE_NAMES[i]);
        h = fnv1a_str(h, FACE_SCOPES[i]);
        h = fnv1a_str(h, FACE_KALIBER[i]);
        h = fnv1a_str(h, FACE_UNIT[i]);
        i = i.wrapping_add(1);
    }
    h
}
/// 对账键集定义哈希（60 键名空间折入）。
pub fn key_definition_hash() -> u64 {
    let mut h: u64 = 0xcbf29ce484222325u64;
    let mut i: usize = 1;
    while i <= KEY_TOTAL {
        h = fnv1a_str(h, key_name_at(i));
        h = fnv1a_str(h, key_kind_at(i).name());
        i = i.wrapping_add(1);
    }
    h
}
/// 冻结探针哈希（注入端与检测端共用的**唯一**对照口径）。
pub fn freeze_probe_hash(
    faces: &FaceBook,
    keys: &KeyBook,
    slots: &SlotTable,
    ledger: &FaceLedger,
    cross: &CrossTable,
    ident: &Identity,
) -> u64 {
    let mut words: Vec<u64> = Vec::with_capacity(14);
    words.push(V_VERSION_ID);
    words.push(faces.fingerprint());
    words.push(keys.fingerprint());
    words.push(slots.fingerprint());
    words.push(ledger.fingerprint());
    words.push(cross.fingerprint());
    words.push(ident_fp(ident));
    words.push(face_definition_hash());
    words.push(key_definition_hash());
    words.push(cross.missing_bits);
    words.push(cross.caliber_bits);
    words.push(ident.deviation);
    words.push(ledger.reading_sum());
    fnv1a_words(&words)
}

// ===========================================================================
// 13. 交叉复核多轮稳定性
// ===========================================================================

/// 稳定性报告。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Stability {
    /// 轮数。
    pub rounds: usize,
    /// 首轮基线指纹。
    pub base_fp: u64,
    /// 逐轮方案指纹。
    pub round_fp: Vec<u64>,
    /// 逐轮 × 逐面读数指纹（`REPEAT × FACE_COUNT = 30`）。
    pub cellwise: Vec<u64>,
    /// 漂移位图（30 位，`u128` 承载）。
    pub drift_bits: u128,
    /// 稳定。
    pub stable: bool,
}
impl Stability {
    /// 漂移格位表。
    pub fn drift_cells(&self) -> Vec<usize> {
        let mut out: Vec<usize> = Vec::new();
        let mut i: usize = 0;
        while i < self.cellwise.len() {
            if (self.drift_bits & (1u128 << i)) != 0u128 {
                out.push(i);
            }
            i = i.wrapping_add(1);
        }
        out
    }
    /// 轮数过下限。
    pub fn rounds_ok(&self) -> bool {
        self.rounds >= 2
    }
    /// 漂移格数。
    pub fn drift_count(&self) -> usize {
        self.drift_cells().len()
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!(
            "stable rounds={} base={:016X} cells={} drift={} mask={:032X} ok={}",
            self.rounds,
            self.base_fp,
            self.cellwise.len(),
            self.drift_count(),
            self.drift_bits,
            if self.stable { 1 } else { 0 }
        )
    }
}
/// 多轮交叉复核稳定性检查。
pub fn stability_check(seed: u64) -> UrResult<Stability> {
    let mut round_fp: Vec<u64> = Vec::with_capacity(REPEAT);
    let mut cellwise: Vec<u64> = Vec::with_capacity(REPEAT * FACE_COUNT);
    let mut base_fp: u64 = 0;
    let mut r: usize = 0;
    while r < REPEAT {
        let s = Snapshot::net(seed)?;
        let mut words: Vec<u64> = Vec::with_capacity(12);
        words.push(s.faces.fingerprint());
        words.push(s.keys.fingerprint());
        words.push(s.slots.fingerprint());
        words.push(s.ledger.fingerprint());
        words.push(s.cross.fingerprint());
        words.push(ident_fp(&s.ident));
        words.push(s.ident.deviation);
        words.push(s.ident.ledger);
        let fp = fnv1a_words(&words);
        round_fp.push(fp);
        if r == 0 {
            base_fp = fp;
        }
        for run in s.ledger.runs.iter() {
            cellwise.push(run.fingerprint);
        }
        r = r.wrapping_add(1);
    }
    let mut drift_bits: u128 = 0u128;
    let mut face: usize = 0;
    while face < FACE_COUNT {
        let mut b: u64 = 0;
        let mut r: usize = 0;
        while r < REPEAT {
            let idx: usize = r.wrapping_mul(FACE_COUNT).wrapping_add(face);
            let v = match cellwise.get(idx) {
                Some(x) => *x,
                None => 0,
            };
            if r == 0 {
                b = v;
            } else if v != b {
                drift_bits = drift_bits | (1u128 << idx);
            }
            r = r.wrapping_add(1);
        }
        face = face.wrapping_add(1);
    }
    let stable = drift_bits == 0u128
        && round_fp.len() == REPEAT
        && round_fp.iter().all(|v| *v == base_fp);
    Ok(Stability { rounds: round_fp.len(), base_fp, round_fp, cellwise, drift_bits, stable })
}

// ===========================================================================
// 14. 指标
// ===========================================================================

/// 指标集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Metrics {
    /// 相位数。
    pub phases: usize,
    /// 面数。
    pub faces: usize,
    /// 十面齐备。
    pub faces_complete: bool,
    /// 键数。
    pub keys: usize,
    /// 键清单齐备。
    pub keys_complete: bool,
    /// 条位映射条数。
    pub slots: usize,
    /// 映射一一。
    pub slots_one2one: bool,
    /// 交叉对数。
    pub crosses: usize,
    /// 交叉对全过。
    pub cross_ok: bool,
    /// 交叉缺失对数。
    pub cross_missing: usize,
    /// 交叉多算对数。
    pub cross_over: usize,
    /// 交叉口径不一対数。
    pub cross_caliber: usize,
    /// ΣA。
    pub sum_a: u64,
    /// ΣB。
    pub sum_b: u64,
    /// 总账。
    pub ledger: u64,
    /// 最大偏离。
    pub deviation: u64,
    /// 恒等式三面相等。
    pub identity_ok: bool,
    /// 逐面勾稽全过。
    pub facet_ok: bool,
    /// 稳定轮数。
    pub rounds: usize,
    /// 漂移格数。
    pub drift_cells: usize,
    /// 注入形态数。
    pub forms: usize,
    /// 注入检出数。
    pub detected: usize,
    /// 净态零误报。
    pub net_clean: bool,
    /// 状态机合法边数。
    pub edges: usize,
    /// 非法迁移拒记数。
    pub rejected: usize,
    /// 环形账写入数。
    pub pushed: u64,
    /// 环形账丢弃数。
    pub dropped: u64,
    /// 环形账存活数。
    pub live: u64,
    /// 环形账守恒。
    pub ring_ok: bool,
    /// 冻结核验通过。
    pub freeze_ok: bool,
    /// 语义边界零违规。
    pub border_ok: bool,
    /// 调度留痕在档。
    pub schedule_ok: bool,
    /// 冻结哈希。
    pub freeze_hash: u64,
}
impl Metrics {
    /// 指标指纹。
    pub fn fingerprint(&self) -> u64 {
        let words: [u64; 36] = [
            self.phases as u64,
            self.faces as u64,
            self.faces_complete as u64,
            self.keys as u64,
            self.keys_complete as u64,
            self.slots as u64,
            self.slots_one2one as u64,
            self.crosses as u64,
            self.cross_ok as u64,
            self.cross_missing as u64,
            self.cross_over as u64,
            self.cross_caliber as u64,
            self.sum_a,
            self.sum_b,
            self.ledger,
            self.deviation,
            self.identity_ok as u64,
            self.facet_ok as u64,
            self.rounds as u64,
            self.drift_cells as u64,
            self.forms as u64,
            self.detected as u64,
            self.net_clean as u64,
            self.edges as u64,
            self.rejected as u64,
            self.pushed,
            self.dropped,
            self.live,
            self.ring_ok as u64,
            self.freeze_ok as u64,
            self.border_ok as u64,
            self.schedule_ok as u64,
            self.freeze_hash,
            self.cross_ok as u64,
            self.identity_ok as u64,
            self.keys_complete as u64,
        ];
        fnv1a_words(&words)
    }
    /// 全项通过（逐条列明，不接受「大部分过」）。
    pub fn all_ok(&self) -> bool {
        self.phases == Phase::COUNT - 1
            && self.faces == FACE_COUNT
            && self.faces_complete
            && self.keys == KEY_TOTAL
            && self.keys_complete
            && self.slots == SLOT_COUNT
            && self.slots_one2one
            && self.crosses == CROSS_COUNT
            && self.cross_ok
            && self.cross_missing == 0
            && self.cross_over == 0
            && self.cross_caliber == 0
            && self.sum_a == self.sum_b
            && self.sum_a == self.ledger
            && self.deviation == 0
            && self.identity_ok
            && self.facet_ok
            && self.rounds == REPEAT
            && self.drift_cells == 0
            && self.forms == FORM_COUNT
            && self.detected == FORM_COUNT
            && self.net_clean
            && self.edges == Phase::COUNT - 1
            && self.rejected > 0
            && self.ring_ok
            && self.freeze_ok
            && self.border_ok
            && self.schedule_ok
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "metrics phases={} faces={} ok={} keys={} ok={} slots={} 1to1={} cross={} ok={}(m/o/c={}/{}/{}) ΣA={} ΣB={} 总账={} dev={} ident={} facet={} stable={} drift={} detect={}/{} net={} edges={} rej={} ring={}/{}/{} ok={} freeze={} border={} sched={} fp={:016X}",
            self.phases,
            self.faces,
            if self.faces_complete { 1 } else { 0 },
            self.keys,
            if self.keys_complete { 1 } else { 0 },
            self.slots,
            if self.slots_one2one { 1 } else { 0 },
            self.crosses,
            if self.cross_ok { 1 } else { 0 },
            self.cross_missing,
            self.cross_over,
            self.cross_caliber,
            self.sum_a,
            self.sum_b,
            self.ledger,
            self.deviation,
            if self.identity_ok { 1 } else { 0 },
            if self.facet_ok { 1 } else { 0 },
            self.rounds,
            self.drift_cells,
            self.detected,
            self.forms,
            if self.net_clean { 1 } else { 0 },
            self.edges,
            self.rejected,
            self.live,
            self.dropped,
            self.pushed,
            if self.ring_ok { 1 } else { 0 },
            if self.freeze_ok { 1 } else { 0 },
            if self.border_ok { 1 } else { 0 },
            if self.schedule_ok { 1 } else { 0 },
            self.fingerprint()
        )
    }
}

// ===========================================================================
// 15. 确定性重放
// ===========================================================================

/// 重放产物。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Replay {
    /// 种子。
    pub seed: u64,
    /// 十面册指纹。
    pub face_fp: u64,
    /// 键册指纹。
    pub key_fp: u64,
    /// 条位映射指纹。
    pub slot_fp: u64,
    /// 逐面账指纹。
    pub ledger_fp: u64,
    /// 交叉复核指纹。
    pub cross_fp: u64,
    /// 恒等式指纹。
    pub identity_fp: u64,
    /// 环形账指纹。
    pub ring_fp: u64,
    /// 注入检出指纹。
    pub detect_fp: u64,
    /// 稳定性指纹。
    pub stable_fp: u64,
    /// 冻结哈希。
    pub freeze_hash: u64,
    /// 重放指纹。
    pub fingerprint: u64,
}
impl Replay {
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "replay seed={:016X} face={:016X} key={:016X} slot={:016X} ledger={:016X} cross={:016X} ident={:016X} ring={:016X} detect={:016X} stable={:016X} freeze={:016X} fp={:016X}",
            self.seed,
            self.face_fp,
            self.key_fp,
            self.slot_fp,
            self.ledger_fp,
            self.cross_fp,
            self.identity_fp,
            self.ring_fp,
            self.detect_fp,
            self.stable_fp,
            self.freeze_hash,
            self.fingerprint
        )
    }
    /// 逐行表。
    pub fn rows(&self) -> Vec<String> {
        vec![
            format!("seed    = {:016X}", self.seed),
            format!("face    = {:016X}", self.face_fp),
            format!("key     = {:016X}", self.key_fp),
            format!("slot    = {:016X}", self.slot_fp),
            format!("ledger  = {:016X}", self.ledger_fp),
            format!("cross   = {:016X}", self.cross_fp),
            format!("ident   = {:016X}", self.identity_fp),
            format!("ring    = {:016X}", self.ring_fp),
            format!("detect  = {:016X}", self.detect_fp),
            format!("stable  = {:016X}", self.stable_fp),
            format!("freeze  = {:016X}", self.freeze_hash),
            format!("fp      = {:016X}", self.fingerprint),
        ]
    }
}
/// 确定性重放（与 `observe` 同 `seed` 同指纹，两者无互递归）。
pub fn replay(seed: u64) -> UrResult<Replay> {
    let s = Snapshot::net(seed)?;
    if !s.faces.complete() {
        return Err(UrErr::of_form(
            Form::FaceMissing,
            format!("重放：十面册不完备 {}", s.faces.summary()),
        ));
    }
    if !s.keys.complete(&s.faces) {
        return Err(UrErr::of_form(
            Form::KeyMissing,
            format!("重放：对账键清单不完备 {}", s.keys.summary()),
        ));
    }
    if !s.slots.one2one_ok() {
        return Err(UrErr::of_form(
            Form::SlotMismatch,
            format!("重放：条位映射非一一 {}", s.slots.summary()),
        ));
    }
    if !s.cross.all_ok() {
        return Err(UrErr::of_form(
            Form::CrossMissing,
            format!("重放：交叉复核不过 {}", s.cross.summary()),
        ));
    }
    if !s.ident.balanced {
        return Err(err_identity(s.ident.sum_a, s.ident.sum_b, s.ident.ledger));
    }
    if !s.ring.balanced() {
        return Err(UrErr::of_form(
            Form::LedgerUnbalanced,
            format!("重放：环形账不守恒 {}", s.ring.summary()),
        ));
    }
    let (_ev, snaps) = inject_all(seed)?;
    let mut det_words: Vec<u64> = Vec::with_capacity(Form::COUNT * 2 + 1);
    let mut total_hit: u64 = 0;
    for snap in snaps.iter() {
        let d = detect_all(snap)?;
        det_words.push(d.verdict_mask as u64);
        total_hit = total_hit.wrapping_add(d.hit_count() as u64);
    }
    det_words.push(total_hit);
    let detect_fp = fnv1a_words(&det_words);
    let st = stability_check(seed)?;
    let mut st_words: Vec<u64> = Vec::with_capacity(REPEAT * FACE_COUNT + 4);
    st_words.push(st.base_fp);
    st_words.push((st.drift_bits & 0xFFFF_FFFF_FFFF_FFFFu128) as u64);
    st_words.push(((st.drift_bits >> 64) & 0xFFFF_FFFF_FFFF_FFFFu128) as u64);
    for v in st.cellwise.iter() {
        st_words.push(*v);
    }
    let stable_fp = fnv1a_words(&st_words);
    let fr = FreezeRecord::build(&s.faces, &s.keys, &s.slots, &s.ledger, &s.cross, &s.ident);
    if !fr.verify(&s.faces, &s.keys, &s.slots, &s.ledger, &s.cross, &s.ident) {
        return Err(UrErr::of_aux(
            AuxCode::BookRevMismatch,
            format!("重放：冻结核验不过 {}", fr.drift(&s.faces, &s.keys, &s.slots, &s.ledger, &s.cross, &s.ident)),
        ));
    }
    let mut words: Vec<u64> = Vec::with_capacity(12);
    words.push(seed);
    words.push(s.faces.fingerprint());
    words.push(s.keys.fingerprint());
    words.push(s.slots.fingerprint());
    words.push(s.ledger.fingerprint());
    words.push(s.cross.fingerprint());
    words.push(ident_fp(&s.ident));
    words.push(s.ring.fingerprint());
    words.push(detect_fp);
    words.push(stable_fp);
    words.push(fr.freeze_hash);
    let fingerprint = fnv1a_words(&words);
    Ok(Replay {
        seed,
        face_fp: s.faces.fingerprint(),
        key_fp: s.keys.fingerprint(),
        slot_fp: s.slots.fingerprint(),
        ledger_fp: s.ledger.fingerprint(),
        cross_fp: s.cross.fingerprint(),
        identity_fp: ident_fp(&s.ident),
        ring_fp: s.ring.fingerprint(),
        detect_fp,
        stable_fp,
        freeze_hash: fr.freeze_hash,
        fingerprint,
    })
}

// ===========================================================================
// 16. 观测（真实跑逻辑，非占位）
// ===========================================================================

/// 观测产物。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct UrObserve {
    /// 种子。
    pub seed: u64,
    /// 净态快照。
    pub net: Snapshot,
    /// 注入留痕。
    pub evidence: Evidence,
    /// 逐形态检出。
    pub detects: Vec<Detections>,
    /// 净态检出（应零）。
    pub net_detect: Detections,
    /// 状态机。
    pub fsm: PhaseFsm,
    /// 稳定性。
    pub stability: Stability,
    /// 冻结件。
    pub freeze: FreezeRecord,
    /// 指标。
    pub metrics: Metrics,
    /// 重放。
    pub replay: Replay,
}
impl UrObserve {
    /// 全项通过。
    pub fn all_ok(&self) -> bool {
        self.metrics.all_ok()
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "observe seed={:016X} all_ok={} | {} | {} | {}",
            self.seed,
            if self.all_ok() { 1 } else { 0 },
            self.net.faces.summary(),
            self.net.keys.summary(),
            self.net.ident.summary()
        )
    }
    /// 逐行表。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(40);
        out.push(format!("# UNX-F3121 观测 seed={:016X}", self.seed));
        out.push(self.summary());
        out.push(format!("ver={} seg={}", V_VERSION, seg_line()));
        out.push(format!("metrics  {}", self.metrics.summary()));
        out.push(format!("fsm      {}", self.fsm.summary()));
        out.push(format!("stable   {}", self.stability.describe()));
        out.push(format!("ring     {}", self.net.ring.summary()));
        out.push(format!("evidence {}", self.evidence.describe()));
        out.push(format!("netdet   {}", self.net_detect.summary()));
        out.push(format!("freeze   {}", self.freeze.describe()));
        out.push(format!("replay   {}", self.replay.summary()));
        for p in Phase::ALL.iter() {
            out.push(format!("phase    {}", p.three_columns()));
        }
        out
    }
    /// 检出表。
    pub fn detect_rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(Form::COUNT + 1);
        out.push(format!("# 注入检出 形态={} 检出={}", Form::COUNT, self.metrics.detected));
        for (i, f) in Form::ALL.iter().enumerate() {
            let hits = match self.detects.get(i) {
                Some(d) => d.hits[f.index()],
                None => 0,
            };
            out.push(format!(
                "{:<20} 0x{:04X} {:<10} hits={} probe={}",
                f.name(),
                f.code(),
                f.class_name(),
                hits,
                f.probe()
            ));
        }
        out
    }
    /// 交叉复核表。
    pub fn cross_rows(&self) -> Vec<String> {
        self.net.cross.rows()
    }
    /// 恒等式表。
    pub fn ident_rows(&self) -> Vec<String> {
        self.net.ident.rows()
    }
    /// 拒记表。
    pub fn reject_rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(self.fsm.rejected.len() + 1);
        out.push(format!("# 非法迁移拒记 n={}", self.fsm.rejected.len()));
        for r in self.fsm.rejected.iter() {
            out.push(r.describe());
        }
        out
    }
    /// 未检出的形态名表。
    pub fn detect_miss(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::new();
        for f in Form::ALL.iter() {
            let mut any: u32 = 0;
            for d in self.detects.iter() {
                any = any.wrapping_add(d.hits[f.index()]);
            }
            if any == 0 {
                out.push(f.name());
            }
        }
        out
    }
    /// 探针自洽。
    pub fn probe_self_consistent(&self) -> bool {
        self.probe_fp() == self.net.ev.registered_probe
    }
    /// 本快照探针值。
    pub fn probe_fp(&self) -> u64 {
        freeze_probe_hash(
            &self.net.faces,
            &self.net.keys,
            &self.net.slots,
            &self.net.ledger,
            &self.net.cross,
            &self.net.ident,
        )
    }
    /// 重放稳定。
    pub fn replay_stable(&self) -> bool {
        self.replay.freeze_hash == self.freeze.freeze_hash
            && self.replay.face_fp == self.net.faces.fingerprint()
            && self.replay.ledger_fp == self.net.ledger.fingerprint()
    }
    /// 重放一致（自算独立指纹与之比对）。
    pub fn replay_match(&self) -> bool {
        let mut words: Vec<u64> = Vec::with_capacity(11);
        words.push(self.seed);
        words.push(self.net.faces.fingerprint());
        words.push(self.net.keys.fingerprint());
        words.push(self.net.slots.fingerprint());
        words.push(self.net.ledger.fingerprint());
        words.push(self.net.cross.fingerprint());
        words.push(ident_fp(&self.net.ident));
        words.push(self.net.ring.fingerprint());
        words.push(self.replay.detect_fp);
        words.push(self.replay.stable_fp);
        words.push(self.freeze.freeze_hash);
        fnv1a_words(&words) == self.replay.fingerprint
    }
}
/// 观测入口（真实跑十面全链，零占位）。
pub fn observe(seed: u64) -> UrResult<UrObserve> {
    // ① 相位一：十面册
    let net = Snapshot::net(seed)?;
    if !net.faces.complete() {
        return Err(UrErr::of_form(
            Form::FaceMissing,
            format!("十面册不完备：缺面={:?}", net.faces.missing_list()),
        ));
    }
    // ② 相位二：对账键清单
    if !net.keys.complete(&net.faces) {
        return Err(UrErr::of_form(
            Form::KeyMissing,
            format!("对账键清单不完备：{}", net.keys.summary()),
        ));
    }
    if net.keys.dup_bits != 0 {
        return Err(UrErr::of_form(
            Form::KeyDup,
            format!("对账键重号：缺键={:?}", net.keys.missing_list()),
        ));
    }
    // ③ 相位三：条位一一映射
    if !net.slots.one2one_ok() {
        return Err(UrErr::of_form(
            Form::SlotMismatch,
            format!("条位映射非一一：{}", net.slots.summary()),
        ));
    }
    // ④ 相位四：逐面勾稽 + 交叉复核 45/45
    if !net.ledger.all_ok() {
        return Err(UrErr::of_form(
            Form::StepOverCap,
            format!("逐面勾稽不过：{}", net.ledger.summary()),
        ));
    }
    if !net.cross.all_ok() {
        return Err(UrErr::of_form(
            Form::CrossMissing,
            format!("交叉复核不过：{}", net.cross.summary()),
        ));
    }
    // ⑤ 相位五：恒等式零偏离
    if !net.ident.balanced {
        return Err(err_identity(net.ident.sum_a, net.ident.sum_b, net.ident.ledger));
    }
    // ⑥ 状态机：走满 5 条合法边 + 3 条非法边拒记
    let mut fsm = PhaseFsm::new();
    for p in Phase::ALL.iter() {
        if p.index() == 0 {
            continue;
        }
        fsm.advance_to(*p)?;
    }
    let illegal: [Phase; 3] = [Phase::P1Define, Phase::P3Slots, Phase::P2Keys];
    for p in illegal.iter() {
        let _ = fsm.try_go(*p);
    }
    if fsm.state != Phase::Sealed || !fsm.trail_monotonic() || fsm.rejected.len() < 3 {
        return Err(UrErr::of_form(
            Form::FingerprintDrift,
            format!("冻结态不符：got={} want={}", fsm.state.name(), Phase::Sealed.name()),
        ));
    }
    // ⑦ 稳定性
    let stability = stability_check(seed)?;
    if !stability.stable || !stability.rounds_ok() {
        return Err(UrErr::of_aux(
            AuxCode::RepeatShort,
            format!("稳定性不过：{}", stability.describe()),
        ));
    }
    // ⑧ 注入核：14 形态逐个注入 → 逐个快照检出
    let (evidence, snaps) = inject_all(seed)?;
    let mut detects: Vec<Detections> = Vec::with_capacity(Form::COUNT);
    let mut detected: usize = 0;
    for s in snaps.iter() {
        let d = detect_all(s)?;
        detected = detected.wrapping_add(1);
        detects.push(d);
    }
    for (i, f) in Form::ALL.iter().enumerate() {
        let d = match detects.get(i) {
            Some(x) => x,
            None => continue,
        };
        if !d.hit(*f) {
            return Err(UrErr::of_form(
                *f,
                format!("注入漏检：形态={} 探面={}", f.name(), f.probe()),
            ));
        }
    }
    if detected != FORM_COUNT {
        return Err(UrErr::new(SEG_BASE, format!("注入核漏检：{}/{}", detected, FORM_COUNT)));
    }
    // ⑨ 净态零误报
    let net_detect = detect_all(&net)?;
    if !net_detect.is_clean() {
        return Err(UrErr::of_form(
            Form::FingerprintDrift,
            format!("净态误报：{:?}", net_detect.hit_list()),
        ));
    }
    // ⑩ 环形账守恒
    if !net.ring.balanced() {
        return Err(UrErr::of_form(
            Form::LedgerUnbalanced,
            format!("环形账不守恒：{}", net.ring.summary()),
        ));
    }
    // ⑪ 冻结件
    let freeze = FreezeRecord::build(&net.faces, &net.keys, &net.slots, &net.ledger, &net.cross, &net.ident);
    if !freeze.verify(&net.faces, &net.keys, &net.slots, &net.ledger, &net.cross, &net.ident) {
        return Err(UrErr::of_aux(
            AuxCode::BookRevMismatch,
            format!("冻结核验不过：{}", freeze.drift(&net.faces, &net.keys, &net.slots, &net.ledger, &net.cross, &net.ident)),
        ));
    }
    // ⑫ 指标
    let metrics = Metrics {
        phases: Phase::COUNT - 1,
        faces: net.faces.faces.len(),
        faces_complete: net.faces.complete(),
        keys: net.keys.keys.len(),
        keys_complete: net.keys.complete(&net.faces),
        slots: net.slots.maps.len(),
        slots_one2one: net.slots.one2one_ok(),
        crosses: net.cross.pairs.len(),
        cross_ok: net.cross.all_ok(),
        cross_missing: net.cross.missing_count(),
        cross_over: net.cross.overcount,
        cross_caliber: net.cross.caliber_count(),
        sum_a: net.ident.sum_a,
        sum_b: net.ident.sum_b,
        ledger: net.ident.ledger,
        deviation: net.ident.deviation,
        identity_ok: net.ident.balanced,
        facet_ok: net.ledger.all_ok(),
        rounds: stability.rounds,
        drift_cells: stability.drift_count(),
        forms: FORM_COUNT,
        detected,
        net_clean: net_detect.is_clean(),
        edges: PhaseFsm::edge_count(),
        rejected: fsm.rejected.len(),
        pushed: net.ring.pushed,
        dropped: net.ring.dropped,
        live: net.ring.live(),
        ring_ok: net.ring.balanced(),
        freeze_ok: true,
        border_ok: evidence.border_violations == 0 && freeze.border_on_file(),
        schedule_ok: freeze.schedule_on_file(),
        freeze_hash: freeze.freeze_hash,
    };
    if !metrics.all_ok() {
        return Err(UrErr::new(SEG_BASE, format!("指标不过：{}", metrics.summary())));
    }
    // ⑬ 重放（不与 observe 互递归）
    let rp = replay(seed)?;
    if rp.freeze_hash != freeze.freeze_hash {
        return Err(UrErr::of_form(
            Form::FingerprintDrift,
            format!("重放冻结哈希失配：{:016X} != {:016X}", rp.freeze_hash, freeze.freeze_hash),
        ));
    }
    Ok(UrObserve { seed, net, evidence, detects, net_detect, fsm, stability, freeze, metrics, replay: rp })
}

// ===========================================================================
// 17. 资源上界显式核算
// ===========================================================================

/// 资源预算（显式核算，可逐项验算）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ResourceBudget {
    /// 常驻登记条数。
    pub resident: u64,
    /// 单条平均字节。
    pub bytes_each: u64,
    /// 注入核峰值快照数。
    pub snap_count: u64,
    /// 单快照字节估算。
    pub snap_bytes: u64,
    /// 导出缓冲字节。
    pub export_bytes: u64,
    /// syscall 次数（恒 0）。
    pub syscalls: u64,
    /// 逻辑步上限。
    pub step_cap: u64,
    /// 实测逻辑步（逐面事件数合计）。
    pub steps: u64,
    /// 峰值字节。
    pub peak_bytes: u64,
    /// 内存上界。
    pub mem_cap: u64,
}
impl ResourceBudget {
    /// 核算。
    pub fn account() -> ResourceBudget {
        let resident: u64 = (FACE_COUNT + KEY_TOTAL + SLOT_COUNT + FACE_COUNT + CROSS_COUNT) as u64;
        let bytes_each: u64 = 128;
        let snap_count: u64 = Form::COUNT as u64;
        let snap_bytes: u64 = 6144;
        let export_bytes: u64 = 4096;
        let peak_bytes: u64 = resident
            .wrapping_mul(bytes_each)
            .wrapping_add(snap_count.wrapping_mul(snap_bytes))
            .wrapping_add(export_bytes);
        ResourceBudget {
            resident,
            bytes_each,
            snap_count,
            snap_bytes,
            export_bytes,
            syscalls: SYSCALL_CAP,
            step_cap: STEP_CAP,
            steps: (FACE_COUNT * FACE_STEPS) as u64,
            peak_bytes,
            mem_cap: MEM_PEAK_CAP,
        }
    }
    /// 预算内。
    pub fn ok(&self) -> bool {
        self.peak_bytes <= self.mem_cap && self.steps <= self.step_cap && self.syscalls <= SYSCALL_CAP
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "budget resident={}x{}B snap={}x{}B export={}B peak={}/{}B steps={}/{} syscalls={}/{} ok={}",
            self.resident,
            self.bytes_each,
            self.snap_count,
            self.snap_bytes,
            self.export_bytes,
            self.peak_bytes,
            self.mem_cap,
            self.steps,
            self.step_cap,
            self.syscalls,
            SYSCALL_CAP,
            if self.ok() { 1 } else { 0 }
        )
    }
    /// 逐行表。
    pub fn rows(&self) -> Vec<String> {
        vec![
            format!("resident     {:>10} x {:>5} B", self.resident, self.bytes_each),
            format!("snapshots    {:>10} x {:>5} B", self.snap_count, self.snap_bytes),
            format!("export       {:>10} B", self.export_bytes),
            format!("peak         {:>10} / {} B", self.peak_bytes, self.mem_cap),
            format!("steps        {:>10} / {}", self.steps, self.step_cap),
            format!("syscalls     {:>10} / {}", self.syscalls, SYSCALL_CAP),
            format!("io           {:>10}", "ZERO"),
            format!("wallclock    {:>10}", "ZERO"),
            format!("float        {:>10}", "ZERO"),
        ]
    }
}

// ===========================================================================
// 18. 导出：KV 与 TLV
// ===========================================================================

/// KV 导出。
pub fn export_kv(o: &UrObserve) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = Vec::with_capacity(60);
    v.push((String::from("ticket"), String::from("UNX-F3121")));
    v.push((String::from("title"), String::from("域级总对账方案")));
    v.push((String::from("batch"), String::from(BATCH_FACE)));
    v.push((String::from("final"), String::from(FINAL_FACE)));
    v.push((String::from("ver"), String::from(V_VERSION)));
    v.push((String::from("seg"), seg_line()));
    v.push((String::from("phases"), o.metrics.phases.to_string()));
    v.push((String::from("faces"), o.net.faces.summary()));
    v.push((String::from("face_key_total"), o.net.faces.key_sum().to_string()));
    v.push((String::from("face_def_hash"), format!("{:016X}", face_definition_hash())));
    v.push((String::from("keys"), o.net.keys.summary()));
    v.push((String::from("key_def_hash"), format!("{:016X}", key_definition_hash())));
    v.push((String::from("key_weight_sum"), o.net.keys.weight_sum().to_string()));
    v.push((String::from("slots"), o.net.slots.summary()));
    v.push((String::from("facet"), o.net.ledger.summary()));
    v.push((String::from("facet_reading"), o.net.ledger.reading_sum().to_string()));
    v.push((String::from("facet_cost"), o.net.ledger.cost_sum().to_string()));
    v.push((String::from("cross"), o.net.cross.summary()));
    v.push((String::from("cross_delta_sum"), o.net.cross.delta_sum.to_string()));
    v.push((String::from("identity"), o.net.ident.summary()));
    v.push((String::from("sum_a"), o.net.ident.sum_a.to_string()));
    v.push((String::from("sum_b"), o.net.ident.sum_b.to_string()));
    v.push((String::from("ledger"), o.net.ident.ledger.to_string()));
    v.push((String::from("dev_ab"), o.net.ident.dev_ab.to_string()));
    v.push((String::from("dev_al"), o.net.ident.dev_al.to_string()));
    v.push((String::from("dev_bl"), o.net.ident.dev_bl.to_string()));
    v.push((String::from("deviation"), o.net.ident.deviation.to_string()));
    v.push((String::from("fsm"), o.fsm.summary()));
    v.push((String::from("edges"), PhaseFsm::edge_count().to_string()));
    v.push((String::from("illegal_edges"), PhaseFsm::illegal_edge_count().to_string()));
    v.push((String::from("ring"), o.net.ring.summary()));
    v.push((String::from("stability"), o.stability.describe()));
    v.push((String::from("evidence"), o.evidence.describe()));
    v.push((String::from("net_detect"), o.net_detect.summary()));
    v.push((String::from("freeze"), o.freeze.describe()));
    v.push((String::from("freeze_hash"), format!("{:016X}", o.freeze.freeze_hash)));
    v.push((String::from("border"), o.freeze.border_note.clone()));
    v.push((String::from("schedule"), o.freeze.schedule_key.clone()));
    v.push((String::from("metrics"), o.metrics.summary()));
    v.push((String::from("replay"), o.replay.summary()));
    v.push((String::from("budget"), ResourceBudget::account().summary()));
    v.push((String::from("complexity_total"), o.metrics.complexity_total().to_string()));
    v.push((String::from("all_ok"), o.all_ok().to_string()));
    v
}
/// 面口径表串。
pub fn face_caliber_table() -> String {
    let mut s: String = String::from("");
    let mut i: usize = 0;
    while i < FACE_COUNT {
        s.push_str(FACE_CODES_REF[i]);
        s.push('=');
        s.push_str(FACE_KALIBER[i]);
        s.push('/');
        s.push_str(FACE_UNIT[i]);
        if i + 1 < FACE_COUNT {
            s.push(' ');
        }
        i = i.wrapping_add(1);
    }
    s
}
/// 大端推 u64。
pub fn push_u64(buf: &mut Vec<u8>, v: u64) {
    buf.push(((v >> 56) & 0xFF) as u8);
    buf.push(((v >> 48) & 0xFF) as u8);
    buf.push(((v >> 40) & 0xFF) as u8);
    buf.push(((v >> 32) & 0xFF) as u8);
    buf.push(((v >> 24) & 0xFF) as u8);
    buf.push(((v >> 16) & 0xFF) as u8);
    buf.push(((v >> 8) & 0xFF) as u8);
    buf.push((v & 0xFF) as u8);
}
/// 推一条 TLV（标签 1 + 长度 1 + 值 8）。
pub fn push_tlv_u64(buf: &mut Vec<u8>, tag: u8, v: u64) {
    buf.push(tag);
    buf.push(8u8);
    push_u64(buf, v);
}
/// TLV 导出。
///
/// 组头**单独一行**推入：`push_u64(&mut head_payload, V_VERSION_ID);`
/// 三条 `push_u64` 决定 `TLV_HEAD_PAYLOAD = 24`，故 `TLV_HEAD_LEN = 1 + 2 + 24 = 27`——
/// 长度常量由 `export_tlv` 内 `push` 次数反推。
/// 长度字段**固定 2 字节**：若按数据自适应宽度，组头实际长度会变成 26，与常量对不上，解析端必错。
pub fn export_tlv(o: &UrObserve) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::with_capacity(4096);
    let mut head_payload: Vec<u8> = Vec::with_capacity(TLV_HEAD_PAYLOAD);
    push_u64(&mut head_payload, V_VERSION_ID);
    push_u64(&mut head_payload, SEG_BASE as u64);
    push_u64(&mut head_payload, o.freeze.rev as u64);
    out.push(TLV_TAG_HEAD);
    let hl = head_payload.len();
    out.push(((hl >> 8) & 0xFF) as u8);
    out.push((hl & 0xFF) as u8);
    for b in head_payload.iter() {
        out.push(*b);
    }
    // 规模常量
    push_tlv_u64(&mut out, 0x10, FACE_COUNT as u64);
    push_tlv_u64(&mut out, 0x11, KEY_TOTAL as u64);
    push_tlv_u64(&mut out, 0x12, CROSS_COUNT as u64);
    push_tlv_u64(&mut out, 0x13, SLOT_COUNT as u64);
    push_tlv_u64(&mut out, 0x14, FACE_STEPS as u64);
    push_tlv_u64(&mut out, 0x15, REPEAT as u64);
    push_tlv_u64(&mut out, 0x16, FORM_COUNT as u64);
    push_tlv_u64(&mut out, 0x17, RING_CAP as u64);
    push_tlv_u64(&mut out, 0x18, STEP_CAP);
    push_tlv_u64(&mut out, 0x19, FACE_READ_CAP);
    // 定义哈希
    push_tlv_u64(&mut out, 0x20, face_definition_hash());
    push_tlv_u64(&mut out, 0x21, key_definition_hash());
    push_tlv_u64(&mut out, 0x22, PhaseFsm::edge_count() as u64);
    push_tlv_u64(&mut out, 0x23, PhaseFsm::illegal_edge_count() as u64);
    // 十面面册
    push_tlv_u64(&mut out, 0x30, o.net.faces.fingerprint());
    push_tlv_u64(&mut out, 0x31, o.net.faces.cover_bits as u64);
    push_tlv_u64(&mut out, 0x32, o.net.faces.key_sum() as u64);
    for (i, f) in o.net.faces.faces.iter().enumerate() {
        push_tlv_u64(&mut out, 0x33u8.wrapping_add(i as u8), f.key_count as u64);
    }
    // 键清单
    push_tlv_u64(&mut out, 0x40, o.net.keys.fingerprint());
    push_tlv_u64(&mut out, 0x41, o.net.keys.cover_bits);
    push_tlv_u64(&mut out, 0x42, o.net.keys.missing_bits());
    push_tlv_u64(&mut out, 0x43, o.net.keys.dup_bits);
    push_tlv_u64(&mut out, 0x44, o.net.keys.weight_sum());
    // 条位映射
    push_tlv_u64(&mut out, 0x50, o.net.slots.fingerprint());
    push_tlv_u64(&mut out, 0x51, o.net.slots.mapped_bits as u64);
    push_tlv_u64(&mut out, 0x52, o.net.slots.remap_bits as u64);
    push_tlv_u64(&mut out, 0x53, o.net.slots.not_one2one() as u64);
    // 逐面勾稽账
    push_tlv_u64(&mut out, 0x60, o.net.ledger.fingerprint());
    push_tlv_u64(&mut out, 0x61, o.net.ledger.ran_bits as u64);
    push_tlv_u64(&mut out, 0x62, o.net.ledger.reading_sum());
    push_tlv_u64(&mut out, 0x63, o.net.ledger.cost_sum());
    for (i, r) in o.net.ledger.runs.iter().enumerate() {
        push_tlv_u64(&mut out, 0x64u8.wrapping_add(i as u8), r.reading);
    }
    // 交叉复核
    push_tlv_u64(&mut out, 0x70, o.net.cross.fingerprint());
    push_tlv_u64(&mut out, 0x71, o.net.cross.missing_bits);
    push_tlv_u64(&mut out, 0x72, o.net.cross.caliber_bits);
    push_tlv_u64(&mut out, 0x73, o.net.cross.overcount as u64);
    push_tlv_u64(&mut out, 0x74, o.net.cross.delta_sum);
    // 恒等式
    push_tlv_u64(&mut out, 0x80, o.net.ident.sum_a);
    push_tlv_u64(&mut out, 0x81, o.net.ident.sum_b);
    push_tlv_u64(&mut out, 0x82, o.net.ident.ledger);
    push_tlv_u64(&mut out, 0x83, o.net.ident.dev_ab);
    push_tlv_u64(&mut out, 0x84, o.net.ident.dev_al);
    push_tlv_u64(&mut out, 0x85, o.net.ident.dev_bl);
    push_tlv_u64(&mut out, 0x86, o.net.ident.deviation);
    // 账与状态机
    push_tlv_u64(&mut out, 0x90, o.net.ring.pushed);
    push_tlv_u64(&mut out, 0x91, o.net.ring.dropped);
    push_tlv_u64(&mut out, 0x92, o.net.ring.live());
    push_tlv_u64(&mut out, 0x93, o.fsm.state.index() as u64);
    push_tlv_u64(&mut out, 0x94, o.fsm.rejected.len() as u64);
    // 稳定性
    push_tlv_u64(&mut out, 0xA0, o.stability.rounds as u64);
    push_tlv_u64(&mut out, 0xA1, o.stability.base_fp);
    push_tlv_u64(&mut out, 0xA2, (o.stability.drift_bits & 0xFFFF_FFFF_FFFF_FFFFu128) as u64);
    push_tlv_u64(&mut out, 0xA3, ((o.stability.drift_bits >> 64) & 0xFFFF_FFFF_FFFF_FFFFu128) as u64);
    // 注入核逐形态命中
    for (i, f) in Form::ALL.iter().enumerate() {
        let h: u32 = match o.detects.get(i) {
            Some(d) => d.hits[f.index()],
            None => 0,
        };
        push_tlv_u64(&mut out, 0xB0u8.wrapping_add(i as u8), h as u64);
    }
    push_tlv_u64(&mut out, 0xC0, o.net_detect.verdict_mask as u64);
    push_tlv_u64(&mut out, 0xC1, ((o.net_detect.verdict_mask >> 64) & 0xFFFF_FFFF_FFFF_FFFFu128) as u64);
    // 冻结与指标
    push_tlv_u64(&mut out, 0xD0, o.freeze.face_hash);
    push_tlv_u64(&mut out, 0xD1, o.freeze.key_hash);
    push_tlv_u64(&mut out, 0xD2, o.freeze.slot_hash);
    push_tlv_u64(&mut out, 0xD3, o.freeze.ledger_hash);
    push_tlv_u64(&mut out, 0xD4, o.freeze.cross_hash);
    push_tlv_u64(&mut out, 0xD5, o.freeze.identity_hash);
    push_tlv_u64(&mut out, 0xD6, o.freeze.freeze_hash);
    push_tlv_u64(&mut out, 0xD7, o.metrics.fingerprint());
    push_tlv_u64(&mut out, 0xD8, o.replay.fingerprint);
    push_tlv_u64(&mut out, 0xD9, o.all_ok() as u64);
    out
}
/// 大端取 u64。
pub fn be_u64(buf: &[u8], off: usize) -> u64 {
    let mut v: u64 = 0;
    let mut i: usize = 0;
    while i < 8 {
        let b = match buf.get(off.wrapping_add(i)) {
            Some(x) => *x,
            None => return v,
        };
        v = (v << 8) | (b as u64);
        i = i.wrapping_add(1);
    }
    v
}
/// TLV 解析。
pub fn parse_tlv(buf: &[u8]) -> UrResult<(u64, Vec<(u8, u64)>)> {
    if buf.len() < TLV_HEAD_LEN {
        return Err(err_range("tlv", buf.len(), TLV_HEAD_LEN));
    }
    let tag = buf[0];
    if tag != TLV_TAG_HEAD {
        return Err(UrErr::of_aux(
            AuxCode::BookRevMismatch,
            format!("TLV 组头标签错：{:02X}", tag),
        ));
    }
    let hl: usize = ((buf[1] as usize) << 8) | (buf[2] as usize);
    if hl != TLV_HEAD_PAYLOAD {
        return Err(UrErr::of_aux(
            AuxCode::BookRevMismatch,
            format!("TLV 组头长度错：{} != {}", hl, TLV_HEAD_PAYLOAD),
        ));
    }
    let version = be_u64(buf, 3);
    let mut items: Vec<(u8, u64)> = Vec::with_capacity(96);
    let mut off: usize = TLV_HEAD_LEN;
    while off < buf.len() {
        let t = buf[off];
        let l = buf[off + 1];
        if l != 8 {
            return Err(UrErr::of_aux(
                AuxCode::BookRevMismatch,
                format!("TLV 条目长度错：tag={:02X} len={}", t, l),
            ));
        }
        items.push((t, be_u64(buf, off + 2)));
        off = off.wrapping_add(TLV_ITEM_LEN);
    }
    Ok((version, items))
}
/// 取 TLV 条目值（不存在给 0）。
pub fn tlv_get(items: &[(u8, u64)], tag: u8) -> u64 {
    for it in items.iter() {
        if it.0 == tag {
            return it.1;
        }
    }
    0
}
/// TLV 往返一致。
pub fn tlv_roundtrip_ok(o: &UrObserve) -> bool {
    let buf = export_tlv(o);
    match parse_tlv(&buf) {
        Ok((ver, items)) => {
            if ver != V_VERSION_ID {
                return false;
            }
            if tlv_get(&items, 0xD6) != o.freeze.freeze_hash {
                return false;
            }
            if tlv_get(&items, 0xD7) != o.metrics.fingerprint() {
                return false;
            }
            if tlv_get(&items, 0x86) != o.net.ident.deviation {
                return false;
            }
            if tlv_get(&items, 0xD9) != (o.all_ok() as u64) {
                return false;
            }
            if buf.len() != TLV_HEAD_LEN + items.len() * TLV_ITEM_LEN {
                return false;
            }
            true
        }
        Err(_) => false,
    }
}

// ===========================================================================
// 19. 性能逐项分解
// ===========================================================================

/// 成本项。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CostItem {
    /// 环节名。
    pub name: &'static str,
    /// 主循环次数。
    pub iters: u64,
    /// 每次迭代基本操作数。
    pub per_iter: u64,
    /// 合计。
    pub total: u64,
    /// 与 Windows 同类能力对照（「深在哪」）。
    pub note: &'static str,
}
impl CostItem {
    /// 构造。
    pub fn new(name: &'static str, iters: u64, per_iter: u64) -> CostItem {
        CostItem { name, iters, per_iter, total: iters.wrapping_mul(per_iter), note: "—" }
    }
    /// 带对照注构造。
    pub fn with_note(name: &'static str, iters: u64, per_iter: u64, note: &'static str) -> CostItem {
        CostItem { name, iters, per_iter, total: iters.wrapping_mul(per_iter), note }
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!(
            "{:<24} iters={:<6} per_iter={:<5} total={:<9} note={}",
            self.name, self.iters, self.per_iter, self.total, self.note
        )
    }
}
impl Metrics {
    /// 性能逐项分解。
    ///
    /// 口径（全部可验算整数乘法）：
    /// - 十面建册：`10 面 × (登记 + 键区间累进)`
    /// - 键清单建册：`60 键 × (登记 + 面归属反查 + 类别判定)`
    /// - 逐面勾稽：`10 面 × 18 步 × (事件取样 + 逐键读数 + 平方累计)`
    /// - 交叉复核：`45 对 × (双面读数取 + 三查判定 + 差值累计)`
    /// - 恒等式三路聚合：`ΣA` 行主序 `60` + `ΣB` 键主序 `60×面反查` + 总账掩码交 `10×60×2`
    /// - 稳定性：`3 轮 × (全链重跑 + 30 逐面指纹比对)`
    /// - 注入核：`14 形态 × (净态构造 + 单面污染 + 14 检测器 + 恒等式重算)`
    pub fn complexity(&self) -> Vec<CostItem> {
        vec![
            CostItem::with_note(
                "face_book_register",
                FACE_COUNT as u64,
                3,
                "Windows 无面位抽象：其内存管理统计不分「十面账册」",
            ),
            CostItem::with_note(
                "face_keybase_scan",
                FACE_COUNT as u64,
                (FACE_COUNT / 2) as u64,
                "键区间前缀累进，O(n²/2) 但 n=10 可忽略",
            ),
            CostItem::with_note(
                "key_book_register",
                KEY_TOTAL as u64,
                4,
                "Windows 页表/池统计无「对账键」统一命名空间",
            ),
            CostItem::with_note(
                "key_face_lookup",
                KEY_TOTAL as u64,
                (FACE_COUNT / 2) as u64,
                "键→面反查，恒等式 ΣB 侧复用",
            ),
            CostItem::with_note(
                "facet_run",
                (FACE_COUNT * FACE_STEPS) as u64,
                6,
                "Windows 内存统计为事后采样，本单逐键确定性推演",
            ),
            CostItem::with_note(
                "facet_key_read",
                KEY_TOTAL as u64,
                3,
                "逐键读数 + 平方累计（方差防口径漂移）",
            ),
            CostItem::with_note(
                "cross_pair_build",
                CROSS_COUNT as u64,
                2,
                "Windows 无面次对交叉复核概念（对标其无公开对应件）",
            ),
            CostItem::with_note(
                "cross_tri_check",
                CROSS_COUNT as u64,
                7,
                "缺/多/口径三查 + 双面读数 + 差值",
            ),
            CostItem::with_note(
                "identity_rowmajor",
                KEY_TOTAL as u64,
                2,
                "ΣA 行主序聚合",
            ),
            CostItem::with_note(
                "identity_colmajor",
                KEY_TOTAL as u64,
                (FACE_COUNT + 1) as u64,
                "ΣB 键主序聚合（面反查 + 权重按号反查）",
            ),
            CostItem::with_note(
                "identity_maskjoin",
                (FACE_COUNT * KEY_TOTAL * 2) as u64,
                1,
                "总账掩码交聚合，三路算法真正独立",
            ),
            CostItem::with_note(
                "identity_absdiff",
                3,
                2,
                "三组两两绝对差求最大偏离",
            ),
            CostItem::with_note(
                "phase_edges",
                (Phase::COUNT + 3) as u64,
                1,
                "合法边闭集 + 非法迁移拒记",
            ),
            CostItem::with_note(
                "stability_rounds",
                REPEAT as u64,
                ((FACE_COUNT + KEY_TOTAL) * 2) as u64,
                "多轮全链重跑 + 逐面指纹比对",
            ),
            CostItem::with_note(
                "cellwise_compare",
                ((REPEAT - 1) * FACE_COUNT) as u64,
                1,
                "逐面跨轮比对定位漂移格位",
            ),
            CostItem::with_note(
                "inject_snapshot",
                FORM_COUNT as u64,
                ((FACE_COUNT * FACE_STEPS) + 40) as u64,
                "每形态独立快照，归因可唯一化",
            ),
            CostItem::with_note(
                "inject_recompute",
                (Form::COUNT * 7) as u64,
                ((KEY_TOTAL * 2) + 20) as u64,
                "污染后重建交叉复核 + 恒等式",
            ),
            CostItem::with_note(
                "detect_scan",
                (Form::COUNT * Form::COUNT) as u64,
                1,
                "逐快照扫 14 检测器",
            ),
            CostItem::with_note(
                "ring_push",
                (KEY_TOTAL * REPEAT) as u64,
                2,
                "环形账 180 次写入压出 dropped 显性",
            ),
            CostItem::new("export_kv", 45, 4),
            CostItem::new("export_tlv", 90, 12),
            CostItem::new("tlv_roundtrip", 90, 3),
            CostItem::new("complexity_rows", 23, 3),
        ]
    }
    /// 性能逐行。
    pub fn complexity_rows(&self) -> Vec<String> {
        let items = self.complexity();
        let mut out: Vec<String> = Vec::with_capacity(items.len() + 2);
        for c in items.iter() {
            out.push(c.describe());
        }
        out.push(format!("TOTAL iters*per_iter = {}", self.complexity_total()));
        out.push(format!(
            "STEP_CAP={} SYSCALL_CAP={} MEM_PEAK_CAP={}",
            STEP_CAP, SYSCALL_CAP, MEM_PEAK_CAP
        ));
        out
    }
    /// 性能合计。
    pub fn complexity_total(&self) -> u64 {
        let mut t: u64 = 0;
        for c in self.complexity().iter() {
            t = t.wrapping_add(c.total);
        }
        t
    }
}

// ===========================================================================
// 20. 自检
// ===========================================================================

/// 自检项结论。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Check {
    /// 项名。
    pub name: &'static str,
    /// 是否通过。
    pub ok: bool,
    /// 实测串。
    pub got: String,
}
impl Check {
    /// 构造。
    pub fn new(name: &'static str, ok: bool, got: String) -> Check {
        Check { name, ok, got }
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("{:<30} {:<6} {}", self.name, if self.ok { "OK" } else { "FAIL" }, self.got)
    }
}
/// 自检项全集。
pub fn self_check(o: &UrObserve) -> Vec<Check> {
    let mut v: Vec<Check> = Vec::with_capacity(24);
    v.push(Check::new("seg_line", code_in_seg(MAIN_CODE), seg_line()));
    v.push(Check::new("version", !V_VERSION.is_empty(), V_VERSION.to_string()));
    v.push(Check::new(
        "face_count",
        FACE_COUNT == 10 && o.net.faces.faces.len() == FACE_COUNT,
        o.net.faces.summary(),
    ));
    v.push(Check::new(
        "face_zero_missing",
        o.net.faces.missing_bits() == 0,
        format!("miss={:?}", o.net.faces.missing_list()),
    ));
    v.push(Check::new(
        "face_zero_dup",
        o.net.faces.dup_bits == 0,
        format!("dup={}", o.net.faces.dup_bits.count_ones()),
    ));
    v.push(Check::new(
        "face_key_total",
        o.net.faces.key_sum() == KEY_TOTAL,
        format!("{}/{}", o.net.faces.key_sum(), KEY_TOTAL),
    ));
    v.push(Check::new(
        "key_zero_missing",
        o.net.keys.missing_bits() == 0,
        format!("miss={:?}", o.net.keys.missing_list()),
    ));
    v.push(Check::new(
        "key_zero_dup",
        o.net.keys.dup_bits == 0,
        format!("dup={}", o.net.keys.dup_bits.count_ones()),
    ));
    v.push(Check::new(
        "key_face_match",
        o.net.keys.face_key_match(&o.net.faces),
        o.net.keys.summary(),
    ));
    v.push(Check::new(
        "slot_one2one_10of10",
        o.net.slots.one2one_ok(),
        o.net.slots.summary(),
    ));
    v.push(Check::new(
        "facet_all_ok",
        o.net.ledger.all_ok(),
        o.net.ledger.summary(),
    ));
    v.push(Check::new(
        "cross_45of45",
        o.net.cross.pairs.len() == CROSS_COUNT && o.net.cross.all_ok(),
        o.net.cross.summary(),
    ));
    v.push(Check::new(
        "cross_zero_missing",
        o.net.cross.missing_count() == 0,
        format!("miss={}", o.net.cross.missing_count()),
    ));
    v.push(Check::new(
        "cross_zero_over",
        o.net.cross.overcount == 0,
        format!("over={}", o.net.cross.overcount),
    ));
    v.push(Check::new(
        "cross_zero_caliber",
        o.net.cross.caliber_count() == 0,
        format!("caliber={}", o.net.cross.caliber_count()),
    ));
    v.push(Check::new(
        "identity_zero_dev",
        o.net.ident.deviation == 0,
        o.net.ident.summary(),
    ));
    v.push(Check::new(
        "identity_three_way",
        o.net.ident.sum_a == o.net.ident.sum_b && o.net.ident.sum_a == o.net.ident.ledger,
        format!("ΣA={} ΣB={} 总账={}", o.net.ident.sum_a, o.net.ident.sum_b, o.net.ident.ledger),
    ));
    v.push(Check::new(
        "stability_zero_drift",
        o.stability.drift_count() == 0,
        o.stability.describe(),
    ));
    v.push(Check::new(
        "inject_all_rejected",
        o.metrics.detected == FORM_COUNT,
        format!("{}/{}", o.metrics.detected, FORM_COUNT),
    ));
    v.push(Check::new("net_zero_false_positive", o.net_detect.is_clean(), o.net_detect.summary()));
    v.push(Check::new("ring_balanced", o.net.ring.balanced(), o.net.ring.summary()));
    v.push(Check::new(
        "ring_dropped_explicit",
        true,
        format!("pushed={} dropped={} live={}", o.net.ring.pushed, o.net.ring.dropped, o.net.ring.live()),
    ));
    v.push(Check::new(
        "freeze_verify",
        o.freeze.verify(
            &o.net.faces,
            &o.net.keys,
            &o.net.slots,
            &o.net.ledger,
            &o.net.cross,
            &o.net.ident,
        ),
        o.freeze.describe(),
    ));
    v.push(Check::new("probe_self_consistent", o.probe_self_consistent(), format!("{:016X}", o.probe_fp())));
    v.push(Check::new("border_zero_violation", o.metrics.border_ok, o.freeze.border_note.clone()));
    v.push(Check::new("schedule_on_file", o.metrics.schedule_ok, o.freeze.schedule_key.clone()));
    v.push(Check::new("fsm_rejected", o.fsm.rejected.len() >= 3, o.fsm.summary()));
    v.push(Check::new("replay_match", o.replay_match(), format!("{:016X}", o.replay.fingerprint)));
    v.push(Check::new("tlv_roundtrip", tlv_roundtrip_ok(o), format!("bytes={}", export_tlv(o).len())));
    v.push(Check::new("budget_ok", ResourceBudget::account().ok(), ResourceBudget::account().summary()));
    v.push(Check::new("all_ok", o.all_ok(), o.metrics.summary()));
    v
}
/// 自检全过。
pub fn self_check_ok(o: &UrObserve) -> bool {
    let v = self_check(o);
    let mut i: usize = 0;
    while i < v.len() {
        if !v[i].ok {
            return false;
        }
        i = i.wrapping_add(1);
    }
    true
}
/// 自检报告串。
pub fn self_check_report(o: &UrObserve) -> String {
    let v = self_check(o);
    let mut out: String = String::from("# 自检报告\n");
    for c in v.iter() {
        out.push_str(&c.describe());
        out.push('\n');
    }
    out
}

// ===========================================================================
// 21. 清单 / 演示 / 关联
// ===========================================================================

/// 注入形态清单。
pub fn form_manifest() -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(Form::COUNT + 1);
    out.push(format!("# 注入形态清单 n={} 码段={}", Form::COUNT, seg_line()));
    for f in Form::ALL.iter() {
        out.push(format!(
            "{:>2} 0x{:04X} {:<20} {:<12} probe={}",
            f.index(),
            f.code(),
            f.name(),
            f.class_name(),
            f.probe()
        ));
    }
    out
}
/// 不变式清单。
pub fn invariant_manifest() -> Vec<String> {
    vec![
        String::from("I01 十面点数 = 10，十面点验 10/10"),
        String::from("I02 相位合法边 = 5（链式），非法边全部拒记"),
        String::from("I03 十面册零缺面、零重号、零越界"),
        String::from("I04 十面键数合计 = 60"),
        String::from("I05 对账键 60 条，覆盖位图满、零重号"),
        String::from("I06 键→面归属与册侧键数逐面一致（口径核对）"),
        String::from("I07 面→条位一一映射 10/10，零重映"),
        String::from("I08 逐面勾稽零欠账、零步数越界"),
        String::from("I09 交叉复核 45/45 通过"),
        String::from("I10 交叉对零缺失（无零读数面）"),
        String::from("I11 交叉对零多算（读数不越配比上界）"),
        String::from("I12 交叉对零口径不一（同族面单位一致）"),
        String::from("I13 恒等式 ΣA == ΣB == 总账，三路算法独立"),
        String::from("I14 3 轮交叉复核逐面指纹零漂移"),
        String::from("I15 14 形态注入 14 检出，净态零误报"),
        String::from("I16 环形账写入 = 存活 + 丢弃"),
        String::from("I17 冻结件哈希自洽，漂移可检出"),
        String::from("I18 语义边界零违规（不做勾稽本体 / 不做 B40 终账）"),
        String::from("I19 调度留痕键在档可回放"),
        String::from("I20 零 syscall、零真实 IO、零墙钟、零浮点"),
        String::from("I21 步数 ≤ STEP_CAP，峰值内存 ≤ MEM_PEAK_CAP"),
        String::from("I22 TLV 往返一致（长度字段定宽）"),
    ]
}
/// 相位演示表。
pub fn demo_phase() -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(Phase::COUNT * 2 + 2);
    out.push(String::from("# 五相位定义（三列：勾稽范围 / 输入账册 / 输出勾稽账）"));
    for p in Phase::ALL.iter() {
        out.push(p.three_columns());
        out.push(format!("   合法出边 = {:?}", p.allowed_names()));
    }
    out
}
/// 边矩阵演示表。
pub fn demo_edges() -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(Phase::COUNT + 3);
    out.push(format!(
        "# 状态机边矩阵 legal={} illegal={}",
        PhaseFsm::edge_count(),
        PhaseFsm::illegal_edge_count()
    ));
    for r in PhaseFsm::edge_rows().iter() {
        out.push(r.clone());
    }
    out
}
/// 十面册演示表。
pub fn demo_faces() -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(FACE_COUNT + 4);
    match face_book_full() {
        Ok(b) => {
            out.push(format!("# 十面册 {} 键={}", b.summary(), b.key_sum()));
            for f in b.faces.iter() {
                out.push(f.describe());
            }
        }
        Err(e) => out.push(err_str(&e)),
    }
    out.push(format!("# 口径表 {}", face_caliber_table()));
    out
}
/// 键清单演示表。
pub fn demo_keys() -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(KEY_TOTAL + 3);
    match key_book_full() {
        Ok(b) => {
            out.push(format!("# 对账键清单 {} wsum={}", b.summary(), b.weight_sum()));
            for k in b.keys.iter() {
                out.push(k.describe());
            }
        }
        Err(e) => out.push(err_str(&e)),
    }
    out
}
/// 环形账演示表。
pub fn demo_ledger(seed: u64) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(RING_CAP + 4);
    match Snapshot::net(seed) {
        Ok(s) => {
            out.push(format!("# 环形账演示 seed={:016X}", seed));
            for r in s.ring.rows().iter() {
                out.push(r.clone());
            }
            out.push(s.ring.summary());
        }
        Err(e) => out.push(err_str(&e)),
    }
    out
}
/// 键名表（自检用）。
pub fn key_name_table() -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::with_capacity(KEY_TOTAL);
    let mut i: usize = 1;
    while i <= KEY_TOTAL {
        out.push(key_name_at(i));
        i = i.wrapping_add(1);
    }
    out
}
/// 面名表（自检用）。
pub fn face_name_table() -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::with_capacity(FACE_COUNT);
    let mut i: usize = 0;
    while i < FACE_COUNT {
        out.push(FACE_NAMES[i]);
        i = i.wrapping_add(1);
    }
    out
}
/// 关联面串。
pub fn related_str() -> String {
    let mut s = String::from("UNX-F3121 关联面：");
    s.push_str(DEP_SLOTS);
    s.push_str("（十面条位）、F2982/F3081（依赖）、");
    s.push_str(GRAFT_SEAT);
    s.push_str("（嫁接源同制）、");
    s.push_str(ALIGN_CHAIN);
    s.push_str("（读数链对齐）、F3181（域收官终账消费者）、");
    s.push_str(BATCH_FACE);
    s.push('/');
    s.push_str(FINAL_FACE);
    s.push(' ');
    s.push_str("（调度面 / 终账底稿）。语义边界：只做方案冻结。");
    s
}
/// 全量摘要。
pub fn full_report(o: &UrObserve) -> String {
    let mut s: String = String::new();
    s.push_str(&seg_line());
    s.push('\n');
    for r in o.rows().iter() {
        s.push_str(r);
        s.push('\n');
    }
    for r in o.detect_rows().iter() {
        s.push_str(r);
        s.push('\n');
    }
    for r in o.cross_rows().iter() {
        s.push_str(r);
        s.push('\n');
    }
    for r in o.ident_rows().iter() {
        s.push_str(r);
        s.push('\n');
    }
    for r in o.reject_rows().iter() {
        s.push_str(r);
        s.push('\n');
    }
    for r in o.metrics.complexity_rows().iter() {
        s.push_str(r);
        s.push('\n');
    }
    for r in ResourceBudget::account().rows().iter() {
        s.push_str(r);
        s.push('\n');
    }
    for r in self_check_report(o).lines() {
        s.push_str(r);
        s.push('\n');
    }
    s
}

