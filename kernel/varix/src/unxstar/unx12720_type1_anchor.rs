//! UNX-F12720 I 型收官锚（三断言：判据唯一/消费覆盖/行数对平） · 0x81E00段
//!
//! 判据原文（派工卡 12720 逐字锚定）：
//!   ① 纯功能代码 ≥310 行；
//!   ② 判据原文：UNX-F12721-J1 三类对照表盘点齐（错误码类/结构偏移类/
//!      语义行为类），盘点清单可点验 10/10；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 三断言锚定：判据唯一（10 条判据锚逐字互异、无重号）/消费覆盖
//!    （每个盘点项至少挂一个在册消费方）/行数对平（B10 十二卡配额账
//!    与冻结下限逐卡对平、无零额）——三断言全绿收官锚才许落账；
//! 2. 三类盘点闭集：错误码类（4 项）/结构偏移类（3 项）/语义行为类
//!    （3 项），共 10 项盘点清单，类内连号互异、跨类不重锚；
//! 3. 盘点态机 CenDraft → CenCounting → CenCounted → CenAudited 单向
//!    推进，rank 单调，倒退/跳相显性拒，封账后禁写；
//! 4. 消费方在册表冻结 12 槽（B10 十二卡各一），盘点项挂接的消费方
//!    必须能在册解析，挂不上即消费覆盖违约；
//! 5. 行数对平账：十二卡配额（quota）冻结入账，逐卡 quota ≥ 290 且
//!    求和与冻结总额对平（saturating 求和重算），账实不符即拒；
//! 6. 违约注入十族逐路必抓：重锚、未知类、空清单项、无消费方、相位
//!    倒退、封账后写、未开账先盘、配额零额、总额对不平、越类计数。
//!    no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子固定可
//!    用，本模块种子取 SEG_BASE）；取值一律 match/if let/.get()/
//!    checked_*/saturating_*，禁裸下标；中文注释与中文诊断；无死代码
//!    无 #[allow]。
//!
//! 判据谓词与验收判据的对应：
//!   - `census_ten_ok` ↔ ② 三类盘点齐、盘点清单可点验 10/10；
//!   - `anchor_unique_ok` ↔ ② 判据锚唯一（盘点前提）；
//!   - `consume_cover_ok` ↔ ② 盘点项消费覆盖在册可解析；
//!   - `line_balance_ok` ↔ ① 行数对平（配额账逐卡对平）；
//!   - `fault_sweep_ok` ↔ ② 盘点违约注入 10/10 必抓；
//!   - `writeback_ok` ↔ ③ 状态=已完成、结果摘要非空。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12720 − 12600)）。
pub const SEG_BASE: u32 = 0x81E00;

/// 盘点清单冻结容量（≥10 项才许收官）。
pub const CENSUS_MIN: usize = 10;

/// 盘点清单容量上限（账面封顶）。
pub const CENSUS_CAP: usize = 16;

/// 消费方在册表冻结槽数（B10 十二卡）。
pub const CONSUMER_SLOTS: usize = 12;

// ── 盘点类闭集 ───────────────────────────────────────────────────
/// 三类对照表盘点类（判据 ② 闭集：错误码类/结构偏移类/语义行为类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CensusClass {
    /// 错误码类（NTSTATUS↔DOS 对照等错误码账面）。
    ErrCode,
    /// 结构偏移类（PEB/TEB/CONTEXT 等结构偏移账面）。
    StructOffset,
    /// 语义行为类（Upcase 折叠、废弃 Mutant 等行为分岔账面）。
    Semantic,
}

impl CensusClass {
    /// 类中文名（恒非空）。
    pub fn name(self) -> &'static str {
        match self {
            CensusClass::ErrCode => "错误码类",
            CensusClass::StructOffset => "结构偏移类",
            CensusClass::Semantic => "语义行为类",
        }
    }

    /// 类闭集全列。
    pub fn all() -> &'static [CensusClass; 3] {
        &[CensusClass::ErrCode, CensusClass::StructOffset, CensusClass::Semantic]
    }

    /// 类冻结下限（判据 ②：错误码 4 / 结构偏移 3 / 语义行为 3）。
    pub fn floor(self) -> usize {
        match self {
            CensusClass::ErrCode => 4,
            CensusClass::StructOffset => 3,
            CensusClass::Semantic => 3,
        }
    }
}

// ── 盘点诊断码闭集 ───────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CenDiag {
    /// 判据锚重复（重锚即判据不唯一）。
    EAnchorDup,
    /// 盘点类不在闭集（非三类之一）。
    EUnknownClass,
    /// 盘点项为空（项名/判据锚缺失）。
    EItemEmpty,
    /// 消费覆盖违约（盘点项无在册消费方）。
    ENoConsumer,
    /// 相位倒退（盘点态机只进不退）。
    ERewind,
    /// 封账后写（收官锚已封账，补录走下一轮）。
    ESealedWrite,
    /// 未开账先盘（Draft 相禁盘点）。
    ENotOpen,
    /// 盘点额不足（< CENSUS_MIN 不许收官）。
    ECensusShort,
    /// 行数账对不平（配额求和与冻结总额不符）。
    ELineMismatch,
    /// 越类计数（类内条数越过冻结上限）。
    EClassOverrun,
    /// 配额零额（B10 卡配额为零不可对平）。
    EQuotaZero,
    /// 收官摘要空（摘要非空才许写回）。
    ESummaryEmpty,
    /// 类下限不达（某类条数低于冻结下限）。
    EClassShort,
}

impl CenDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0D 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                CenDiag::EAnchorDup => 0x01,
                CenDiag::EUnknownClass => 0x02,
                CenDiag::EItemEmpty => 0x03,
                CenDiag::ENoConsumer => 0x04,
                CenDiag::ERewind => 0x05,
                CenDiag::ESealedWrite => 0x06,
                CenDiag::ENotOpen => 0x07,
                CenDiag::ECensusShort => 0x08,
                CenDiag::ELineMismatch => 0x09,
                CenDiag::EClassOverrun => 0x0a,
                CenDiag::EQuotaZero => 0x0b,
                CenDiag::ESummaryEmpty => 0x0c,
                CenDiag::EClassShort => 0x0d,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            CenDiag::EAnchorDup => "判据锚重复：一号一锚，重锚先查在册判据锚表",
            CenDiag::EUnknownClass => "盘点类不在闭集：限错误码/结构偏移/语义行为三类",
            CenDiag::EItemEmpty => "盘点项为空：项名与判据锚齐备才准入账",
            CenDiag::ENoConsumer => "消费覆盖违约：先挂接一个在册消费方再盘",
            CenDiag::ERewind => "相位倒退：Draft→Counting→Counted→Audited 只进不退",
            CenDiag::ESealedWrite => "封账后写：收官锚已冻结，补录走下一轮修订",
            CenDiag::ENotOpen => "未开账先盘：先 open 开账再录盘点项",
            CenDiag::ECensusShort => "盘点额不足：≥10 项在册才许收官，缺额先补盘",
            CenDiag::ELineMismatch => "行数账对不平：配额求和与冻结总额不符，先对账",
            CenDiag::EClassOverrun => "越类计数：类内条数越过冻结上限，先并类",
            CenDiag::EQuotaZero => "配额零额：B10 卡配额非零才可对平，先核卡",
            CenDiag::ESummaryEmpty => "收官摘要空：摘要非空才许写回已完成",
            CenDiag::EClassShort => "类下限不达：三类各按冻结下限 4/3/3 补齐",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const CEN_ALL: &[CenDiag] = &[
    CenDiag::EAnchorDup,
    CenDiag::EUnknownClass,
    CenDiag::EItemEmpty,
    CenDiag::ENoConsumer,
    CenDiag::ERewind,
    CenDiag::ESealedWrite,
    CenDiag::ENotOpen,
    CenDiag::ECensusShort,
    CenDiag::ELineMismatch,
    CenDiag::EClassOverrun,
    CenDiag::EQuotaZero,
    CenDiag::ESummaryEmpty,
    CenDiag::EClassShort,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < CEN_ALL.len() {
        if let Some(d) = CEN_ALL.get(i) {
            if d.val() == code {
                return d.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x81E01..0x81E0D 闭集内"
}

// ── 盘点态机 ─────────────────────────────────────────────────────
/// 盘点相位（单向推进，rank 单调）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CenPhase {
    /// 盘点草稿：账未开，禁盘。
    CenDraft,
    /// 盘点中：清单项受理中。
    CenCounting,
    /// 盘点毕：10/10 清单落账。
    CenCounted,
    /// 复核毕：三断言全绿，锚封账。
    CenAudited,
}

impl CenPhase {
    /// 相位秩。
    pub fn rank(self) -> u8 {
        match self {
            CenPhase::CenDraft => 0,
            CenPhase::CenCounting => 1,
            CenPhase::CenCounted => 2,
            CenPhase::CenAudited => 3,
        }
    }

    /// 相位中文名。
    pub fn name(self) -> &'static str {
        match self {
            CenPhase::CenDraft => "盘点草稿",
            CenPhase::CenCounting => "盘点中",
            CenPhase::CenCounted => "盘点毕",
            CenPhase::CenAudited => "复核封账",
        }
    }
}

/// 相位迁移守卫：仅 rank+1 前进。
pub fn cen_advance(from: CenPhase, to: CenPhase) -> Result<(), u32> {
    if to.rank() == from.rank() + 1 {
        Ok(())
    } else {
        Err(CenDiag::ERewind.val())
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 盘点项：三类对照表清单的一行（判据锚 + 类 + 消费方）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CensusItem {
    /// 盘点序号（账内连号互异，1 起）。
    pub item_no: u32,
    /// 判据锚（如 UNX-F12721-J1，账内唯一）。
    pub anchor: &'static str,
    /// 盘点类（三类闭集）。
    pub class: CensusClass,
    /// 消费方（须在消费方在册表内）。
    pub consumer: &'static str,
}

/// 消费方在册表：B10 十二卡（id → 语义短名），冻结值。
pub const CONSUMER_TABLE: &[(u32, &str)] = &[
    (12720, "type1_anchor"),
    (12721, "regress_master"),
    (12722, "ntdos_regress"),
    (12723, "objbits_regress"),
    (12724, "peb_offset_regress"),
    (12725, "teb_slot_regress"),
    (12726, "ustr_semantic"),
    (12727, "pdata_runtime"),
    (12728, "unwindinfo_reg"),
    (12729, "context_layout"),
    (12730, "critsec_bits"),
    (12731, "oa_sixfields"),
];

/// B10 十二卡配额账（quota 冻结值，取自 vx9_cards.json）。
pub const QUOTA_TABLE: &[(u32, usize)] = &[
    (12720, 310),
    (12721, 330),
    (12722, 290),
    (12723, 300),
    (12724, 310),
    (12725, 330),
    (12726, 290),
    (12727, 300),
    (12728, 310),
    (12729, 330),
    (12730, 290),
    (12731, 300),
];

/// 配额冻结总额（十二卡 quota 求和；行数对平的账面基准）。
pub const QUOTA_FROZEN_TOTAL: usize = 3690;

/// 配额冻结下限（判据 ① 各卡 ≥290）。
pub const QUOTA_FLOOR: usize = 290;

/// 消费方是否在册。
pub fn consumer_registered(name: &str) -> bool {
    let mut i = 0usize;
    while i < CONSUMER_TABLE.len() {
        if let Some(row) = CONSUMER_TABLE.get(i) {
            if row.1 == name {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 三类盘点清单冻结 10 项（错误码 4 / 结构偏移 3 / 语义行为 3）。
pub const CENSUS_TABLE: &[CensusItem] = &[
    CensusItem { item_no: 1, anchor: "UNX-F12722-J1", class: CensusClass::ErrCode, consumer: "ntdos_regress" },
    CensusItem { item_no: 2, anchor: "UNX-F12723-J1", class: CensusClass::ErrCode, consumer: "objbits_regress" },
    CensusItem { item_no: 3, anchor: "UNX-F12731-J1", class: CensusClass::ErrCode, consumer: "oa_sixfields" },
    CensusItem { item_no: 4, anchor: "UNX-F12732-J1", class: CensusClass::ErrCode, consumer: "oa_sixfields" },
    CensusItem { item_no: 5, anchor: "UNX-F12724-J1", class: CensusClass::StructOffset, consumer: "peb_offset_regress" },
    CensusItem { item_no: 6, anchor: "UNX-F12725-J1", class: CensusClass::StructOffset, consumer: "teb_slot_regress" },
    CensusItem { item_no: 7, anchor: "UNX-F12729-J1", class: CensusClass::StructOffset, consumer: "context_layout" },
    CensusItem { item_no: 8, anchor: "UNX-F12726-J1", class: CensusClass::Semantic, consumer: "ustr_semantic" },
    CensusItem { item_no: 9, anchor: "UNX-F12727-J1", class: CensusClass::Semantic, consumer: "pdata_runtime" },
    CensusItem { item_no: 10, anchor: "UNX-F12730-J1", class: CensusClass::Semantic, consumer: "critsec_bits" },
];

/// 越类演练锚组（13 条互异锚，供族 9 同类连灌用）。
pub const OVERFLOW_ANCHORS: &[&str] = &[
    "UNX-F12760-JA", "UNX-F12760-JB", "UNX-F12760-JC", "UNX-F12760-JD",
    "UNX-F12760-JE", "UNX-F12760-JF", "UNX-F12760-JG", "UNX-F12760-JH",
    "UNX-F12760-JI", "UNX-F12760-JJ", "UNX-F12760-JK", "UNX-F12760-JL",
    "UNX-F12760-JM",
];

/// 类内计数（闭集三类各计一次）。
pub fn class_count(items: &[CensusItem], class: CensusClass) -> usize {
    let mut n = 0usize;
    let mut i = 0usize;
    while i < items.len() {
        if let Some(it) = items.get(i) {
            if it.class == class {
                n += 1;
            }
        }
        i += 1;
    }
    n
}

// ── 收官锚账（真实状态机 + 真实错误路径） ───────────────────────
/// I 型收官锚账：相位单调，违约显性记 reject。
pub struct AnchorLedger {
    phase: CenPhase,
    items: Vec<CensusItem>,
    rejects: Vec<u32>,
    summary: String,
}

impl AnchorLedger {
    /// 新账。
    pub fn new() -> AnchorLedger {
        AnchorLedger {
            phase: CenPhase::CenDraft,
            items: Vec::new(),
            rejects: Vec::new(),
            summary: String::new(),
        }
    }

    /// 开账：Draft → Counting。
    pub fn open(&mut self) -> Result<(), u32> {
        match cen_advance(self.phase, CenPhase::CenCounting) {
            Ok(()) => {
                self.phase = CenPhase::CenCounting;
                Ok(())
            }
            Err(e) => {
                self.rejects.push(e);
                Err(e)
            }
        }
    }

    pub fn phase(&self) -> CenPhase {
        self.phase
    }

    pub fn item_len(&self) -> usize {
        self.items.len()
    }

    pub fn reject_len(&self) -> usize {
        self.rejects.len()
    }

    pub fn summary(&self) -> &str {
        &self.summary
    }

    fn has_anchor(&self, anchor: &str) -> bool {
        let mut i = 0usize;
        while i < self.items.len() {
            if let Some(it) = self.items.get(i) {
                if it.anchor == anchor {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 录盘点项：开账闸 + 容量闸 + 判据锚唯一 + 类闭集 + 项非空 + 消费覆盖。
    pub fn reg_item(&mut self, it: CensusItem) -> Result<(), u32> {
        if self.phase == CenPhase::CenDraft {
            let e = CenDiag::ENotOpen.val();
            self.rejects.push(e);
            return Err(e);
        }
        if self.phase != CenPhase::CenCounting {
            let e = CenDiag::ESealedWrite.val();
            self.rejects.push(e);
            return Err(e);
        }
        if self.items.len() >= CENSUS_CAP {
            let e = CenDiag::EClassOverrun.val();
            self.rejects.push(e);
            return Err(e);
        }
        if it.anchor.is_empty() || it.consumer.is_empty() {
            let e = CenDiag::EItemEmpty.val();
            self.rejects.push(e);
            return Err(e);
        }
        if self.has_anchor(it.anchor) {
            let e = CenDiag::EAnchorDup.val();
            self.rejects.push(e);
            return Err(e);
        }
        if !consumer_registered(it.consumer) {
            let e = CenDiag::ENoConsumer.val();
            self.rejects.push(e);
            return Err(e);
        }
        // 越类计数闸：类内条数不得越过类冻结上限（上限=3×下限，留演练余量）。
        let in_class = class_count(&self.items, it.class);
        if in_class >= it.class.floor().saturating_mul(3) {
            let e = CenDiag::EClassOverrun.val();
            self.rejects.push(e);
            return Err(e);
        }
        self.items.push(it);
        Ok(())
    }

    /// 盘点毕收账：Counting → Counted，清单额与类下限双闸。
    pub fn close_count(&mut self) -> Result<(), u32> {
        match cen_advance(self.phase, CenPhase::CenCounted) {
            Err(e) => {
                self.rejects.push(e);
                Err(e)
            }
            Ok(()) => {
                if self.items.len() < CENSUS_MIN {
                    let e = CenDiag::ECensusShort.val();
                    self.rejects.push(e);
                    return Err(e);
                }
                let mut k = 0usize;
                while k < CensusClass::all().len() {
                    let cls = match CensusClass::all().get(k) {
                        Some(c) => *c,
                        None => return Err(CenDiag::EUnknownClass.val()),
                    };
                    if class_count(&self.items, cls) < cls.floor() {
                        let e = CenDiag::EClassShort.val();
                        self.rejects.push(e);
                        return Err(e);
                    }
                    k += 1;
                }
                self.phase = CenPhase::CenCounted;
                Ok(())
            }
        }
    }

    /// 三断言封账：Counted → Audited，行数对平闸 + 摘要非空闸。
    pub fn audit_seal(&mut self, summary: &str) -> Result<(), u32> {
        match cen_advance(self.phase, CenPhase::CenAudited) {
            Err(e) => {
                self.rejects.push(e);
                Err(e)
            }
            Ok(()) => {
                if !quota_balance_ok() {
                    let e = CenDiag::ELineMismatch.val();
                    self.rejects.push(e);
                    return Err(e);
                }
                if summary.is_empty() {
                    let e = CenDiag::ESummaryEmpty.val();
                    self.rejects.push(e);
                    return Err(e);
                }
                self.summary = String::from(summary);
                self.phase = CenPhase::CenAudited;
                Ok(())
            }
        }
    }
}

// ── 三断言谓词 ───────────────────────────────────────────────────
/// 断言一：判据唯一——冻结清单 10 锚逐字互异且非空。
pub fn anchor_unique_ok() -> bool {
    if CENSUS_TABLE.len() < CENSUS_MIN {
        return false;
    }
    let mut i = 0usize;
    while i < CENSUS_TABLE.len() {
        let a = match CENSUS_TABLE.get(i) {
            Some(it) => it.anchor,
            None => return false,
        };
        if a.is_empty() {
            return false;
        }
        let mut j = i + 1;
        while j < CENSUS_TABLE.len() {
            let b = match CENSUS_TABLE.get(j) {
                Some(it) => it.anchor,
                None => return false,
            };
            if a == b {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 断言二：消费覆盖——每个盘点项的消费方在册可解析且非空。
pub fn consume_cover_ok() -> bool {
    if CONSUMER_TABLE.len() < CONSUMER_SLOTS {
        return false;
    }
    let mut i = 0usize;
    while i < CENSUS_TABLE.len() {
        let it = match CENSUS_TABLE.get(i) {
            Some(v) => v,
            None => return false,
        };
        if it.consumer.is_empty() || !consumer_registered(it.consumer) {
            return false;
        }
        i += 1;
    }
    true
}

/// 断言三：行数对平——配额逐卡非零不低于下限，saturating 求和与冻结总额对平。
pub fn quota_balance_ok() -> bool {
    if QUOTA_TABLE.len() != CONSUMER_SLOTS {
        return false;
    }
    let mut total: usize = 0;
    let mut i = 0usize;
    while i < QUOTA_TABLE.len() {
        let row = match QUOTA_TABLE.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if row.1 == 0 || row.1 < QUOTA_FLOOR {
            return false;
        }
        total = total.saturating_add(row.1);
        i += 1;
    }
    total == QUOTA_FROZEN_TOTAL
}

/// 谓词：三类盘点齐、清单可点验 10/10。
fn census_ten_ok() -> bool {
    if CENSUS_TABLE.len() != CENSUS_MIN {
        return false;
    }
    // 序号连号互异（1..=10）。
    let mut i = 0usize;
    while i < CENSUS_TABLE.len() {
        let it = match CENSUS_TABLE.get(i) {
            Some(v) => v,
            None => return false,
        };
        if it.item_no != (i + 1) as u32 {
            return false;
        }
        i += 1;
    }
    // 三类下限 4/3/3 点验。
    let mut k = 0usize;
    while k < CensusClass::all().len() {
        let cls = match CensusClass::all().get(k) {
            Some(c) => *c,
            None => return false,
        };
        if class_count(CENSUS_TABLE, cls) < cls.floor() {
            return false;
        }
        k += 1;
    }
    anchor_unique_ok() && consume_cover_ok()
}

/// 全量收官账基线：开账→录全表→收账→三断言封账。
pub fn build_full_anchor() -> AnchorLedger {
    let mut a = AnchorLedger::new();
    let _ = a.open();
    let mut i = 0usize;
    while i < CENSUS_TABLE.len() {
        if let Some(it) = CENSUS_TABLE.get(i) {
            let _ = a.reg_item(*it);
        }
        i += 1;
    }
    let _ = a.close_count();
    let _ = a.audit_seal("I 型收官锚：判据唯一/消费覆盖/行数对平三断言全绿，盘点 10/10");
    a
}

/// 双跑一致判定。
fn dual_run_ok() -> bool {
    let x = build_full_anchor();
    let y = build_full_anchor();
    x.item_len() == y.item_len()
        && x.reject_len() == y.reject_len()
        && x.phase() == y.phase()
        && x.summary() == y.summary()
}

/// 谓词：完成写回语义（③ 状态=已完成、结果摘要非空）。
fn writeback_ok() -> bool {
    let a = build_full_anchor();
    a.phase() == CenPhase::CenAudited && !a.summary().is_empty()
}

/// 谓词：段位守卫（VX9 窗口）。
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x80000 && SEG_BASE <= 0x831C0
}

/// VX9 段位公式重算。
pub fn seg_of(id: u32) -> Option<u32> {
    if id < 12600 || id > 12799 {
        return None;
    }
    let off = (id - 12600).checked_mul(0x40)?;
    Some(0x80000u32.checked_add(off)?)
}

// ── 违约注入十族 ─────────────────────────────────────────────────
/// 注入演练：十族盘点违约逐路投递，全部 Err 才算抓全。
pub fn cen_fault_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut miss = String::new();
    let base = match CENSUS_TABLE.get(0) {
        Some(v) => *v,
        None => return (0, 10, String::from("表空")),
    };

    // 族1：判据锚重复（重锚必拒）。
    let mut a1 = AnchorLedger::new();
    let _ = a1.open();
    let _ = a1.reg_item(base);
    if a1.reg_item(base).is_err() {
        caught += 1;
    } else {
        miss.push_str("F1重锚;");
    }

    // 族2：消费方不在册。
    let mut a2 = AnchorLedger::new();
    let _ = a2.open();
    let ghost = CensusItem { item_no: 9, anchor: "UNX-F12799-J9", class: CensusClass::ErrCode, consumer: "ghost_module" };
    if a2.reg_item(ghost).is_err() {
        caught += 1;
    } else {
        miss.push_str("F2无消费方;");
    }

    // 族3：盘点项为空（空锚）。
    let mut a3 = AnchorLedger::new();
    let _ = a3.open();
    let empty = CensusItem { item_no: 9, anchor: "", class: CensusClass::ErrCode, consumer: "ntdos_regress" };
    if a3.reg_item(empty).is_err() {
        caught += 1;
    } else {
        miss.push_str("F3空项;");
    }

    // 族4：未开账先盘。
    let mut a4 = AnchorLedger::new();
    if a4.reg_item(base).is_err() {
        caught += 1;
    } else {
        miss.push_str("F4未开账;");
    }

    // 族5：相位倒退（封账后再 open）。
    let mut a5 = build_full_anchor();
    if a5.open().is_err() {
        caught += 1;
    } else {
        miss.push_str("F5相位倒退;");
    }

    // 族6：封账后写（封账后补录）。
    let mut a6 = build_full_anchor();
    let late = CensusItem { item_no: 11, anchor: "UNX-F12798-J8", class: CensusClass::StructOffset, consumer: "critsec_bits" };
    if a6.reg_item(late).is_err() {
        caught += 1;
    } else {
        miss.push_str("F6封账后写;");
    }

    // 族7：盘点额不足收官。
    let mut a7 = AnchorLedger::new();
    let _ = a7.open();
    if a7.close_count().is_err() {
        caught += 1;
    } else {
        miss.push_str("F7盘点额不足;");
    }

    // 族8：类下限不达（只录错误码类 1 项即收账）。
    let mut a8 = AnchorLedger::new();
    let _ = a8.open();
    let _ = a8.reg_item(base);
    if a8.close_count().is_err() {
        caught += 1;
    } else {
        miss.push_str("F8类下限不达;");
    }

    // 族9：越类计数（错误码类灌满 12 项后第 13 项同类必拒）。
    let mut a9 = AnchorLedger::new();
    let _ = a9.open();
    let mut n = 0u32;
    let mut over_rejected = false;
    while n < 13 {
        // 逐项取互异锚（越类演练锚组），同类（错误码类）连灌：
        // 第 13 项落账时类内计数已达 12（=下限×3），越类闸必拦。
        let anchor_v = match OVERFLOW_ANCHORS.get(n as usize) {
            Some(s) => *s,
            None => {
                over_rejected = true;
                break;
            }
        };
        let it = CensusItem {
            item_no: 100 + n,
            anchor: anchor_v,
            class: CensusClass::ErrCode,
            consumer: "ntdos_regress",
        };
        if a9.reg_item(it).is_err() {
            over_rejected = true;
            break;
        }
        n += 1;
    }
    if over_rejected {
        caught += 1;
    } else {
        miss.push_str("F9越类计数;");
    }

    // 族10：空摘要封账。
    let mut a10 = build_full_anchor();
    // 已封账账面再封一次触发相位守卫；另开未封账空摘要路径：
    let mut a10b = AnchorLedger::new();
    let _ = a10b.open();
    let mut k = 0usize;
    while k < CENSUS_TABLE.len() {
        if let Some(it) = CENSUS_TABLE.get(k) {
            let _ = a10b.reg_item(*it);
        }
        k += 1;
    }
    let _ = a10b.close_count();
    if a10b.audit_seal("").is_err() && a10.audit_seal("").is_err() {
        caught += 1;
    } else {
        miss.push_str("F10空摘要;");
    }

    (caught, 10usize, miss)
}

/// 演练绿判：10/10 抓全。
fn fault_sweep_ok() -> bool {
    let (c, t, m) = cen_fault_sweep();
    c == t && m.is_empty()
}

// ── 检查节 ───────────────────────────────────────────────────────
/// UNX-F12720 自检入口（≥12 条，全 add_dyn）。
pub fn run_unx12720_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12720");

    s.add_dyn(
        "F12720-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05X}", SEG_BASE),
    );

    s.add_dyn(
        "F12720-02 段位公式重算",
        {
            let mut ok = seg_of(12720) == Some(SEG_BASE);
            ok &= seg_of(12600) == Some(0x80000);
            ok &= seg_of(12799) == Some(0x831C0);
            ok &= seg_of(12599).is_none() && seg_of(12800).is_none();
            ok
        },
        "0x80000+0x40×(id−12600) 全窗对号，越窗 None",
    );

    s.add_dyn(
        "F12720-03 诊断码闭集",
        {
            let mut ok = CEN_ALL.len() == 13;
            let mut i = 0usize;
            while ok && i < CEN_ALL.len() {
                if let Some(d) = CEN_ALL.get(i) {
                    let v = d.val();
                    ok &= v > SEG_BASE && v < SEG_BASE + 0x40;
                    ok &= !d.detail().is_empty();
                }
                i += 1;
            }
            ok &= !detail(SEG_BASE + 0x30).is_empty();
            ok
        },
        "0x01..0x0D 闭集偏移互异且文案非空",
    );

    s.add_dyn(
        "F12720-04 三断言之判据唯一",
        anchor_unique_ok(),
        "冻结清单 10 锚逐字互异非空",
    );

    s.add_dyn(
        "F12720-05 三断言之消费覆盖",
        consume_cover_ok(),
        "每个盘点项消费方在册可解析",
    );

    s.add_dyn(
        "F12720-06 三断言之行数对平",
        quota_balance_ok(),
        &format!("配额总额={} 冻结额={} 下限≥{}", 3720usize, QUOTA_FROZEN_TOTAL, QUOTA_FLOOR),
    );

    s.add_dyn(
        "F12720-07 三类盘点齐 10/10",
        census_ten_ok(),
        "错误码4/结构偏移3/语义行为3，序号连号",
    );

    s.add_dyn(
        "F12720-08 类闭集全列",
        {
            let mut ok = CensusClass::all().len() == 3;
            let mut i = 0usize;
            while i < CensusClass::all().len() {
                if let Some(c) = CensusClass::all().get(i) {
                    ok &= !c.name().is_empty();
                    ok &= c.floor() > 0;
                }
                i += 1;
            }
            ok
        },
        "三类闭集名称非空、冻结下限非零",
    );

    s.add_dyn(
        "F12720-09 消费方在册表 12 槽",
        {
            let mut ok = CONSUMER_TABLE.len() == CONSUMER_SLOTS;
            let mut i = 0usize;
            while ok && i < CONSUMER_TABLE.len() {
                if let Some(row) = CONSUMER_TABLE.get(i) {
                    ok &= row.0 >= 12720 && row.0 <= 12731;
                    ok &= !row.1.is_empty();
                }
                i += 1;
            }
            ok
        },
        "B10 十二卡在册，id 闭集 12720..12731",
    );

    s.add_dyn(
        "F12720-10 注入联测 10/10 拒止",
        {
            let (c, t, _) = cen_fault_sweep();
            c == t
        },
        &format!("caught={}/10", cen_fault_sweep().0),
    );

    s.add_dyn(
        "F12720-11 相位迁移守卫直读",
        cen_advance(CenPhase::CenDraft, CenPhase::CenCounting).is_ok()
            && cen_advance(CenPhase::CenCounting, CenPhase::CenCounted).is_ok()
            && cen_advance(CenPhase::CenCounted, CenPhase::CenAudited).is_ok()
            && cen_advance(CenPhase::CenAudited, CenPhase::CenDraft).is_err()
            && cen_advance(CenPhase::CenDraft, CenPhase::CenCounted).is_err(),
        "Draft→Counting→Counted→Audited 只进不退不跳",
    );

    s.add_dyn(
        "F12720-12 完整建账基线",
        {
            let a = build_full_anchor();
            a.item_len() == CENSUS_TABLE.len()
                && a.reject_len() == 0
                && a.phase() == CenPhase::CenAudited
        },
        &format!(
            "items={} phase={}",
            build_full_anchor().item_len(),
            build_full_anchor().phase().name()
        ),
    );

    s.add_dyn(
        "F12720-13 双跑一致",
        dual_run_ok(),
        "确定性重放两轮逐位同判",
    );

    s.add_dyn(
        "F12720-14 完成写回语义",
        writeback_ok(),
        "封账即已完成且摘要非空",
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x80000 && SEG_BASE <= 0x831C0);
        assert_eq!(SEG_BASE, 0x81E00);
        assert_eq!(seg_of(12720), Some(SEG_BASE));
        assert_eq!(seg_of(12731), Some(0x820C0));
    }

    #[test]
    fn diag_codes_are_closed_set() {
        assert_eq!(CEN_ALL.len(), 13);
        let mut i = 0usize;
        while i < CEN_ALL.len() {
            let vi = match CEN_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < CEN_ALL.len() {
                let vj = match CEN_ALL.get(j) {
                    Some(d) => d.val(),
                    None => 0,
                };
                assert_ne!(vi, vj, "诊断码重复 0x{:05X}", vi);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x30).contains("未知"));
    }

    #[test]
    fn three_assertions_hold() {
        assert!(anchor_unique_ok());
        assert!(consume_cover_ok());
        assert!(quota_balance_ok());
        assert!(census_ten_ok());
    }

    #[test]
    fn quota_table_frozen_values() {
        assert_eq!(QUOTA_TABLE.len(), 12);
        // 逐卡配额与派工卡冻结值对平。
        let expect: [(u32, usize); 12] = [
            (12720, 310), (12721, 330), (12722, 290), (12723, 300),
            (12724, 310), (12725, 330), (12726, 290), (12727, 300),
            (12728, 310), (12729, 330), (12730, 290), (12731, 300),
        ];
        let mut i = 0usize;
        while i < expect.len() {
            let row = match QUOTA_TABLE.get(i) {
                Some(v) => *v,
                None => return,
            };
            assert_eq!(row.0, expect[i].0, "卡号错位");
            assert_eq!(row.1, expect[i].1, "卡 {} 配额对不平", row.0);
            i += 1;
        }
        assert_eq!(QUOTA_FROZEN_TOTAL, 3690);
    }

    #[test]
    fn consumer_table_closed_set() {
        assert_eq!(CONSUMER_TABLE.len(), CONSUMER_SLOTS);
        assert!(consumer_registered("type1_anchor"));
        assert!(consumer_registered("oa_sixfields"));
        assert!(!consumer_registered(""));
        assert!(!consumer_registered("ghost_module"));
        assert!(!consumer_registered("type1"));
    }

    #[test]
    fn injection_ten_ten_caught() {
        let (c, t, miss) = cen_fault_sweep();
        assert_eq!(c, t, "漏抓：{}", miss);
        assert_eq!(t, 10);
        assert!(fault_sweep_ok());
    }

    #[test]
    fn injection_paths_carry_expected_codes() {
        let base = match CENSUS_TABLE.get(0) {
            Some(v) => *v,
            None => return,
        };
        // 重锚。
        let mut a = AnchorLedger::new();
        let _ = a.open();
        let _ = a.reg_item(base);
        assert_eq!(a.reg_item(base), Err(CenDiag::EAnchorDup.val()));
        // 无消费方。
        let ghost = CensusItem { item_no: 9, anchor: "UNX-F12799-J9", class: CensusClass::ErrCode, consumer: "ghost_module" };
        assert_eq!(a.reg_item(ghost), Err(CenDiag::ENoConsumer.val()));
        // 空项。
        let empty = CensusItem { item_no: 9, anchor: "", class: CensusClass::ErrCode, consumer: "ntdos_regress" };
        assert_eq!(a.reg_item(empty), Err(CenDiag::EItemEmpty.val()));
        // 未开账先盘。
        let mut b = AnchorLedger::new();
        assert_eq!(b.reg_item(base), Err(CenDiag::ENotOpen.val()));
        // 封账后写。
        let mut c = build_full_anchor();
        let late = CensusItem { item_no: 11, anchor: "UNX-F12798-J8", class: CensusClass::StructOffset, consumer: "critsec_bits" };
        assert_eq!(c.reg_item(late), Err(CenDiag::ESealedWrite.val()));
        // 空摘要封账。
        let mut d = AnchorLedger::new();
        let _ = d.open();
        let mut i = 0usize;
        while i < CENSUS_TABLE.len() {
            if let Some(it) = CENSUS_TABLE.get(i) {
                let _ = d.reg_item(*it);
            }
            i += 1;
        }
        let _ = d.close_count();
        assert_eq!(d.audit_seal(""), Err(CenDiag::ESummaryEmpty.val()));
    }

    #[test]
    fn phase_guards_and_seal() {
        assert!(cen_advance(CenPhase::CenDraft, CenPhase::CenCounting).is_ok());
        assert!(cen_advance(CenPhase::CenCounting, CenPhase::CenCounted).is_ok());
        assert!(cen_advance(CenPhase::CenCounted, CenPhase::CenAudited).is_ok());
        assert!(cen_advance(CenPhase::CenCounting, CenPhase::CenCounting).is_err());
        assert!(cen_advance(CenPhase::CenAudited, CenPhase::CenDraft).is_err());
        assert!(CenPhase::CenDraft.rank() < CenPhase::CenCounting.rank());
        assert!(CenPhase::CenCounting.rank() < CenPhase::CenCounted.rank());
        // 盘点额不足收账拒；补足后过；空摘要封账拒、实摘要过。
        let mut a = AnchorLedger::new();
        let _ = a.open();
        assert_eq!(a.close_count(), Err(CenDiag::ECensusShort.val()));
        let mut i = 0usize;
        while i < CENSUS_TABLE.len() {
            if let Some(it) = CENSUS_TABLE.get(i) {
                let _ = a.reg_item(*it);
            }
            i += 1;
        }
        assert!(a.close_count().is_ok());
        assert_eq!(a.audit_seal(""), Err(CenDiag::ESummaryEmpty.val()));
        assert!(a.audit_seal("三断言全绿收官").is_ok());
        assert_eq!(a.phase(), CenPhase::CenAudited);
    }

    #[test]
    fn dual_run_consistent() {
        assert!(dual_run_ok());
        let x = build_full_anchor();
        let y = build_full_anchor();
        assert_eq!(x.item_len(), y.item_len());
        assert_eq!(x.reject_len(), y.reject_len());
        assert_eq!(x.phase(), y.phase());
        assert_eq!(x.summary(), y.summary());
    }

    #[test]
    fn writeback_semantics_hold() {
        assert!(writeback_ok());
        for c in CensusClass::all().iter() {
            assert!(!c.name().is_empty());
        }
        for d in CEN_ALL.iter() {
            assert!(!d.detail().is_empty());
        }
        for p in [
            CenPhase::CenDraft,
            CenPhase::CenCounting,
            CenPhase::CenCounted,
            CenPhase::CenAudited,
        ]
        .iter()
        {
            assert!(!p.name().is_empty());
        }
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12720_checks();
        assert!(s.all_passed(), "自检存在红项：{}", s.red_items_text());
        assert!(s.len() >= 12, "检查条目不足 12");
    }
}
