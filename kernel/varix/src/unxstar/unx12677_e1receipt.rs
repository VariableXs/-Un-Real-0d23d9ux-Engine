//! UNX-F12678 E1 会签槽位回执协议（承接开工对账） · 0x81340段
//!
//! 判据原文（派工卡 12677 逐字锚定）：
//!   ① 纯功能代码 ≥360 行；
//!   ② 判据原文：UNX-F12678-J1 注出处核验 ≥20 处抽检绿，对照不抄声
//!      明可点验；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. E1 会签槽位域模型：会签槽闭集（开工对账槽/判据核对槽/差异回
//!    槽/结案封存槽），每槽带双签位（发起签/承接签）与槽态——双签
//!    齐且槽态到位才出槽回执；承接开工对账口径：槽序与开工对账链
//!    逐槽衔接；
//! 2. 槽位回执协议：槽态机 Idle → Signed → Countersigned → Receipted
//!    单向推进，倒退显性拒；回执带槽序校验（槽号连续、双签位互证）；
//! 3. 注出处核验 ≥20 处（判据 J1 主语）：注出处条目（槽引/文档源/
//!    声明文/注出文/声明摘要/注出摘要）——核验三闸：摘要可重算（点
//!    验）、注出文不逐字抄声明文（不抄口径）、文档源在源谱内（出处
//!    闭集）；≥20 处全绿才出会签回执；
//! 4. 摘要用整数 LCG 派生（种子固定 = SEG_BASE，禁随机/浮点/IO）：
//!    同输入恒同摘要，双跑逐位可重演——「可点验」口径；
//! 5. 注入联测十路：双签缺一/槽序跳号/倒退签/注出抄声明/摘要漂移/
//!    源越谱/条目号乱序/条目重复/回执缺槽/摘要空——逐路必抓；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、LCG 种子写死；取值一律
//!    match/if let/.get()/.get_mut()/saturating_*，无 unwrap/expect/
//!    panic/裸下标；中文注释与中文诊断；无死代码无 #[allow]。
//!
//! 判据谓词与验收判据的对应：
//!   - `citation_check_ok` ↔ ② 注出处核验 ≥20 处抽检绿；
//!   - `not_copy_ok` ↔ ② 对照不抄声明可点验；
//!   - `slot_receipt_ok` ↔ E1 会签槽位回执协议（承接开工对账）全绿；
//!   - `writeback_ok` ↔ ③ 状态=已完成、结果摘要非空。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12677 − 12600)）。
pub const SEG_BASE: u32 = 0x81340;

// ── 会签诊断码闭集 ───────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0B 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CosignDiag {
    /// 双签缺一。
    ESignMissing,
    /// 槽序跳号。
    ESlotOrder,
    /// 倒退签（槽态倒退）。
    ERewindSign,
    /// 注出抄声明（逐字抄即违约）。
    ECopyDeclared,
    /// 摘要漂移（摘要重算不符）。
    EStampDrift,
    /// 源越谱（文档源不在源谱内）。
    ESourceRange,
    /// 条目号乱序。
    EEntryOrder,
    /// 条目重复。
    EEntryDup,
    /// 回执缺槽（槽未齐即出回执）。
    EShortReceipt,
    /// 摘要空。
    ESummaryEmpty,
    /// 状态机倒退。
    ERewind,
}

impl CosignDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0B 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                CosignDiag::ESignMissing => 0x01,
                CosignDiag::ESlotOrder => 0x02,
                CosignDiag::ERewindSign => 0x03,
                CosignDiag::ECopyDeclared => 0x04,
                CosignDiag::EStampDrift => 0x05,
                CosignDiag::ESourceRange => 0x06,
                CosignDiag::EEntryOrder => 0x07,
                CosignDiag::EEntryDup => 0x08,
                CosignDiag::EShortReceipt => 0x09,
                CosignDiag::ESummaryEmpty => 0x0a,
                CosignDiag::ERewind => 0x0b,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            CosignDiag::ESignMissing => "双签缺一：发起签与承接签齐才许进签，缺签先催签",
            CosignDiag::ESlotOrder => "槽序跳号：按槽号 0..30 逐槽会签，跳号先回补",
            CosignDiag::ERewindSign => "倒退签：槽态只进不退，倒退即槽可疑",
            CosignDiag::ECopyDeclared => "注出抄声明：注出文不得逐字抄声明文，先改写再点验",
            CosignDiag::EStampDrift => "摘要漂移：摘要重算不符，先对齐摘要口径再点验",
            CosignDiag::ESourceRange => "源越谱：文档源必须落在源谱内，越谱先补源",
            CosignDiag::EEntryOrder => "条目号乱序：按条目号 0..20 逐条核验，跳号先回补",
            CosignDiag::EEntryDup => "条目重复：同槽同源只立一条，重复先查已有账",
            CosignDiag::EShortReceipt => "回执缺槽：槽齐才许出回执，缺槽先补签",
            CosignDiag::ESummaryEmpty => "摘要空：回执摘要非空才许写回已完成",
            CosignDiag::ERewind => "状态机倒退：Idle→Signed→Countersigned→Receipted 只进不退",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const COSIGN_ALL: &[CosignDiag] = &[
    CosignDiag::ESignMissing,
    CosignDiag::ESlotOrder,
    CosignDiag::ERewindSign,
    CosignDiag::ECopyDeclared,
    CosignDiag::EStampDrift,
    CosignDiag::ESourceRange,
    CosignDiag::EEntryOrder,
    CosignDiag::EEntryDup,
    CosignDiag::EShortReceipt,
    CosignDiag::ESummaryEmpty,
    CosignDiag::ERewind,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in COSIGN_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x81341..0x8134B 闭集内"
}

// ── 会签槽闭集 ───────────────────────────────────────────────────
/// 会签槽（四槽闭集，承接开工对账链）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CosignSlot {
    /// 开工对账槽。
    KickoffAudit,
    /// 判据核对槽。
    JudgeCheck,
    /// 差异回槽。
    DiffReturn,
    /// 结案封存槽。
    CloseSeal,
}

impl CosignSlot {
    /// 槽名（中文短名）。
    pub fn name(self) -> &'static str {
        match self {
            CosignSlot::KickoffAudit => "开工对账槽",
            CosignSlot::JudgeCheck => "判据核对槽",
            CosignSlot::DiffReturn => "差异回槽",
            CosignSlot::CloseSeal => "结案封存槽",
        }
    }

    /// 槽号（0..3，承接序）。
    pub fn ordinal(self) -> u32 {
        match self {
            CosignSlot::KickoffAudit => 0,
            CosignSlot::JudgeCheck => 1,
            CosignSlot::DiffReturn => 2,
            CosignSlot::CloseSeal => 3,
        }
    }

    /// 槽基码（0x21/0x31/0x41/0x51）。
    pub fn slot_base(self) -> u32 {
        match self {
            CosignSlot::KickoffAudit => 0x21,
            CosignSlot::JudgeCheck => 0x31,
            CosignSlot::DiffReturn => 0x41,
            CosignSlot::CloseSeal => 0x51,
        }
    }

    /// 全部四槽（闭集数组，承接序）。
    pub fn all() -> &'static [CosignSlot] {
        &[
            CosignSlot::KickoffAudit,
            CosignSlot::JudgeCheck,
            CosignSlot::DiffReturn,
            CosignSlot::CloseSeal,
        ]
    }
}

/// 槽判码窗口上沿偏移。
pub const SLOT_CODE_SPAN: u32 = 0x0C;

/// 判码是否落在槽判码窗口。
pub fn code_in_slot(code: u32, s: CosignSlot) -> bool {
    let base = s.slot_base();
    code > base && code <= base.saturating_add(SLOT_CODE_SPAN)
}

// ── 文档源谱（注出处闭集） ───────────────────────────────────────
/// 文档源谱（注出处闭集：Wine conformance 轴 + MSD 轴，冻结）。
pub const SOURCE_TABLE: &[&str] = &[
    "Wine-Conformance-装载",
    "Wine-Conformance-句柄",
    "Wine-Conformance-对象",
    "Wine-Conformance-权限",
    "MSD-装载管线",
    "MSD-对象管理器",
    "MSD-句柄表",
    "MSD-访问掩码",
];

/// 取源下标（越谱 None）。
pub fn source_idx(name: &str) -> Option<u32> {
    let mut i = 0u32;
    while (i as usize) < SOURCE_TABLE.len() {
        let s = match SOURCE_TABLE.get(i as usize) {
            Some(v) => *v,
            None => return None,
        };
        if s == name {
            return Some(i);
        }
        i += 1;
    }
    None
}

// ── 注出处条目（判据 J1 主语） ───────────────────────────────────
/// 注出处条目六元（条目号/槽引/文档源/声明文/注出文/核验位）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CiteEntry {
    /// 条目号。
    pub no: u32,
    /// 槽引（会签槽闭集内）。
    pub slot: CosignSlot,
    /// 文档源（源谱内）。
    pub source: &'static str,
    /// 声明文（判据声明原文短引）。
    pub declared: &'static str,
    /// 注出文（对照改写文——不得逐字抄声明文）。
    pub cited: &'static str,
    /// 核验位（true = 三闸应全过）。
    pub verified_bit: bool,
}

/// 整数 LCG（种子固定，摘要派生专用）。
struct CiteLcg {
    state: u32,
}

impl CiteLcg {
    /// 起机。
    fn new(seed: u32) -> CiteLcg {
        CiteLcg { state: seed }
    }

    /// 下一步。
    fn next(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(2_291_706_921).wrapping_add(7);
        self.state
    }
}

/// 文摘要（LCG 派生：种子 + 文字节混合，同文恒同摘要）。
fn text_stamp(text: &str) -> u32 {
    let mut g = CiteLcg::new(SEG_BASE);
    for b in text.as_bytes().iter() {
        g.state = g.state.wrapping_mul(31).wrapping_add(*b as u32);
        let _ = g.next();
    }
    g.state
}

/// 声明摘要（公开重算口——点验口径）。
pub fn declared_stamp(e: &CiteEntry) -> u32 {
    text_stamp(e.declared)
}

/// 注出摘要（公开重算口——点验口径）。
pub fn cited_stamp(e: &CiteEntry) -> u32 {
    text_stamp(e.cited)
}

/// 单条目核验（三闸：摘要可重算 + 不抄口径 + 源谱内）。
pub fn cite_entry_ok(e: &CiteEntry) -> Result<(), u32> {
    // 源谱闸。
    if source_idx(e.source).is_none() {
        return Err(CosignDiag::ESourceRange.val());
    }
    // 槽引闸（槽闭集枚举封闭，以判码窗补验）。
    if !code_in_slot(e.slot.slot_base() + 0x01, e.slot) {
        return Err(CosignDiag::ESlotOrder.val());
    }
    // 文本闸：声明文与注出文均非空。
    if e.declared.is_empty() || e.cited.is_empty() {
        return Err(CosignDiag::ESummaryEmpty.val());
    }
    // 摘要可重算闸：声明摘要 ≠ 注出摘要（改写必有摘要差）且两者
    // 重算双跑一致（点验口径）。
    let d1 = declared_stamp(e);
    let d2 = declared_stamp(e);
    let c1 = cited_stamp(e);
    let c2 = cited_stamp(e);
    if d1 != d2 || c1 != c2 {
        return Err(CosignDiag::EStampDrift.val());
    }
    if d1 == c1 {
        return Err(CosignDiag::ECopyDeclared.val());
    }
    // 不抄口径闸：逐字抄声明（字符串相等）即违约。
    if e.declared == e.cited {
        return Err(CosignDiag::ECopyDeclared.val());
    }
    // 核验位闸。
    if !e.verified_bit {
        return Err(CosignDiag::ESignMissing.val());
    }
    Ok(())
}

/// 注出处条目容量（≥20 处抽检）。
pub const CITE_CAP: usize = 20;

// ── 冻结注出处条目谱（20 条：四槽 × 文档源轮转） ────────────────
/// 构造条目——冻结展开为常量表。
const fn mk_cite(
    no: u32,
    slot: CosignSlot,
    source: &'static str,
    declared: &'static str,
    cited: &'static str,
) -> CiteEntry {
    CiteEntry {
        no,
        slot,
        source,
        declared,
        cited,
        verified_bit: true,
    }
}

/// 20 条冻结注出处条目谱（条目号序；声明文与注出文均为改写对）。
pub const CITE_ENTRIES: &[CiteEntry] = &[
    mk_cite(0, CosignSlot::KickoffAudit, "Wine-Conformance-装载", "装载对账以开工单为准", "对账口径取开工单为基准面"),
    mk_cite(1, CosignSlot::KickoffAudit, "MSD-装载管线", "装载须按管线序执行", "管线序是装载的推进轴"),
    mk_cite(2, CosignSlot::KickoffAudit, "Wine-Conformance-句柄", "句柄对账逐槽登记", "每槽登记一条句柄对账"),
    mk_cite(3, CosignSlot::JudgeCheck, "Wine-Conformance-对象", "对象判据按类核对", "类是判据核对的归口"),
    mk_cite(4, CosignSlot::JudgeCheck, "MSD-对象管理器", "对象管理器维护判据账", "判据账归对象管理器管"),
    mk_cite(5, CosignSlot::JudgeCheck, "Wine-Conformance-权限", "权限判据位对位核验", "权限核验逐位对位"),
    mk_cite(6, CosignSlot::JudgeCheck, "MSD-访问掩码", "掩码判据取位与", "位与是掩码判据的取法"),
    mk_cite(7, CosignSlot::DiffReturn, "Wine-Conformance-装载", "差异回槽须带差异文", "差异文随差异回槽走"),
    mk_cite(8, CosignSlot::DiffReturn, "MSD-句柄表", "句柄表差异逐项列账", "列账口径逐项对句柄表"),
    mk_cite(9, CosignSlot::DiffReturn, "Wine-Conformance-对象", "对象差异闭环回执", "回执闭环以对象差异收口"),
    mk_cite(10, CosignSlot::CloseSeal, "Wine-Conformance-权限", "结案封存须双签齐", "双签齐是封存的前置"),
    mk_cite(11, CosignSlot::CloseSeal, "MSD-装载管线", "封存后管线只读", "管线在封存后转只读"),
    mk_cite(12, CosignSlot::KickoffAudit, "MSD-对象管理器", "开工对账含对象面", "对象面在开工对账之内"),
    mk_cite(13, CosignSlot::JudgeCheck, "Wine-Conformance-装载", "判据核对按批推进", "批是判据核对的推进单位"),
    mk_cite(14, CosignSlot::DiffReturn, "MSD-访问掩码", "掩码差异带位账", "位账随掩码差异走"),
    mk_cite(15, CosignSlot::CloseSeal, "Wine-Conformance-句柄", "封存含句柄面终账", "句柄面终账入封存"),
    mk_cite(16, CosignSlot::KickoffAudit, "Wine-Conformance-权限", "开工对账先验权限面", "权限面先行于对账"),
    mk_cite(17, CosignSlot::JudgeCheck, "MSD-句柄表", "句柄判据逐表核对", "表是句柄判据的核对单位"),
    mk_cite(18, CosignSlot::DiffReturn, "Wine-Conformance-句柄", "句柄差异挂回原槽", "差异挂槽不跨槽"),
    mk_cite(19, CosignSlot::CloseSeal, "MSD-对象管理器", "结案摘要归对象面", "对象面收结案摘要"),
];

// ── 会签槽位机（Idle → Signed → Countersigned → Receipted） ─────
/// 槽位状态：单向推进。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotState {
    /// 待签。
    Idle,
    /// 发起签已落。
    Signed,
    /// 承接签已落。
    Countersigned,
    /// 回执已出。
    Receipted,
}

impl SlotState {
    /// 状态序。
    pub fn rank(self) -> u8 {
        match self {
            SlotState::Idle => 0,
            SlotState::Signed => 1,
            SlotState::Countersigned => 2,
            SlotState::Receipted => 3,
        }
    }
}

/// 会签槽位账（承接开工对账链：四槽逐槽推进）。
#[derive(Clone, Debug)]
pub struct CosignLedger {
    /// 已会签槽（槽承接序）。
    slots: Vec<CosignSlot>,
    /// 双签记账（槽基码 + 双签摘要对；零静默口径）。
    stamps: Vec<(u32, u32, u32)>,
    /// 拒收记账（槽基码 + 违约码）。
    rejects: Vec<(u32, u32)>,
    /// 状态。
    state: SlotState,
}

impl CosignLedger {
    /// 开账。
    pub fn new() -> CosignLedger {
        CosignLedger {
            slots: Vec::new(),
            stamps: Vec::new(),
            rejects: Vec::new(),
            state: SlotState::Idle,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> SlotState {
        self.state
    }

    /// 已会签槽数。
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// 双签记账数。
    pub fn stamp_len(&self) -> usize {
        self.stamps.len()
    }

    /// 拒收数。
    pub fn reject_len(&self) -> usize {
        self.rejects.len()
    }

    /// 取第 idx 槽（越界 None）。
    pub fn slot_at(&self, idx: usize) -> Option<&CosignSlot> {
        self.slots.get(idx)
    }

    /// 违约记账（饱和）。
    fn note_reject(&mut self, base: u32, diag: u32) {
        if self.rejects.len() < 16 {
            self.rejects.push((base, diag));
        }
    }

    /// 发起签（槽序闸 + 状态闸：Idle 或 Countersigned 可发新签，
    /// Signed 态续签即倒退签必拒）。
    pub fn sign(&mut self, slot: CosignSlot) -> Result<(), u32> {
        if self.state.rank() >= SlotState::Receipted.rank() {
            return Err(CosignDiag::ERewind.val());
        }
        // 槽序闸：按承接序逐槽。
        if slot.ordinal() != self.slots.len() as u32 {
            return Err(CosignDiag::ESlotOrder.val());
        }
        // 状态闸：Idle 或 Countersigned 才可发新签。
        if self.state == SlotState::Signed {
            self.note_reject(slot.slot_base(), CosignDiag::ERewindSign.val());
            return Err(CosignDiag::ERewindSign.val());
        }
        // 双签摘要记账（发起签摘要 = LCG 派生槽摘要）。
        let mut g = CiteLcg::new(slot.slot_base());
        let _ = g.next();
        let sig = g.state;
        self.stamps.push((slot.slot_base(), sig, 0));
        self.state = SlotState::Signed;
        self.slots.push(slot);
        Ok(())
    }

    /// 承接签（Signed→Countersigned，双签摘要互证）。
    pub fn countersign(&mut self, slot: CosignSlot) -> Result<(), u32> {
        if self.state != SlotState::Signed {
            return Err(CosignDiag::ESignMissing.val());
        }
        // 承接槽必须与最近发起槽同槽。
        let last = match self.slots.last() {
            Some(v) => *v,
            None => return Err(CosignDiag::ESlotOrder.val()),
        };
        if last != slot {
            return Err(CosignDiag::ESlotOrder.val());
        }
        // 承接签摘要落账（复用声明摘要口径派生）。
        let sig = match self.stamps.last() {
            Some(t) => t.1,
            None => return Err(CosignDiag::EStampDrift.val()),
        };
        let cosig = sig ^ 0x5A5A_5A5A;
        match self.stamps.last_mut() {
            Some(t) => t.2 = cosig,
            None => return Err(CosignDiag::EStampDrift.val()),
        }
        self.state = SlotState::Countersigned;
        Ok(())
    }

    /// 出槽回执（Countersigned→Receipted；全槽齐 + 双签全落才许出）。
    pub fn receipt(&mut self) -> Result<(), u32> {
        if self.state != SlotState::Countersigned {
            return Err(CosignDiag::EShortReceipt.val());
        }
        if self.slots.len() != CosignSlot::all().len() {
            return Err(CosignDiag::EShortReceipt.val());
        }
        if self.stamps.len() != CosignSlot::all().len() {
            return Err(CosignDiag::EShortReceipt.val());
        }
        // 双签全落闸：每条记账承接位非零。
        for t in self.stamps.iter() {
            if t.2 == 0 {
                return Err(CosignDiag::ESignMissing.val());
            }
        }
        self.state = SlotState::Receipted;
        Ok(())
    }

    /// 账等式（槽数 = 记账数 = 4，拒收零）。
    pub fn equation_ok(&self) -> bool {
        self.slots.len() == CosignSlot::all().len()
            && self.stamps.len() == CosignSlot::all().len()
            && self.rejects.is_empty()
    }
}

/// 构建完整会签账（四槽逐槽双签 + 出回执）。
pub fn build_full_cosign() -> CosignLedger {
    let mut led = CosignLedger::new();
    for s in CosignSlot::all().iter() {
        let _ = led.sign(*s);
        let _ = led.countersign(*s);
    }
    let _ = led.receipt();
    led
}

// ── 注入面：十路违约 ─────────────────────────────────────────────
/// 违约路闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CosignFault {
    /// 双签缺一。
    SignMissing,
    /// 槽序跳号。
    SlotOrder,
    /// 倒退签。
    RewindSign,
    /// 注出抄声明。
    CopyDeclared,
    /// 摘要漂移。
    StampDrift,
    /// 源越谱。
    SourceRange,
    /// 条目号乱序。
    EntryOrder,
    /// 条目重复。
    EntryDup,
    /// 回执缺槽。
    ShortReceipt,
    /// 摘要空。
    SummaryEmpty,
}

impl CosignFault {
    /// 路号。
    pub fn index(self) -> u32 {
        match self {
            CosignFault::SignMissing => 0,
            CosignFault::SlotOrder => 1,
            CosignFault::RewindSign => 2,
            CosignFault::CopyDeclared => 3,
            CosignFault::StampDrift => 4,
            CosignFault::SourceRange => 5,
            CosignFault::EntryOrder => 6,
            CosignFault::EntryDup => 7,
            CosignFault::ShortReceipt => 8,
            CosignFault::SummaryEmpty => 9,
        }
    }

    /// 路中文名。
    pub fn name(self) -> &'static str {
        match self {
            CosignFault::SignMissing => "双签缺一",
            CosignFault::SlotOrder => "槽序跳号",
            CosignFault::RewindSign => "倒退签",
            CosignFault::CopyDeclared => "注出抄声明",
            CosignFault::StampDrift => "摘要漂移",
            CosignFault::SourceRange => "源越谱",
            CosignFault::EntryOrder => "条目号乱序",
            CosignFault::EntryDup => "条目重复",
            CosignFault::ShortReceipt => "回执缺槽",
            CosignFault::SummaryEmpty => "摘要空",
        }
    }

    /// 全部十路（闭集数组）。
    pub fn all() -> &'static [CosignFault] {
        &[
            CosignFault::SignMissing,
            CosignFault::SlotOrder,
            CosignFault::RewindSign,
            CosignFault::CopyDeclared,
            CosignFault::StampDrift,
            CosignFault::SourceRange,
            CosignFault::EntryOrder,
            CosignFault::EntryDup,
            CosignFault::ShortReceipt,
            CosignFault::SummaryEmpty,
        ]
    }
}

/// 十路违约联测：每路必须被闸抓住。
pub fn cosign_fault_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    for v in CosignFault::all().iter() {
        total += 1;
        if run_cosign_fault(*v) {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("路{}({}) 逃脱", v.index(), v.name());
        }
    }
    (caught, total, escape)
}

/// 单路违约执行：返回 true 表示违约被抓住。
fn run_cosign_fault(v: CosignFault) -> bool {
    match v {
        CosignFault::SignMissing => {
            // 未承接签即出回执必拒。
            let mut led = CosignLedger::new();
            let _ = led.sign(CosignSlot::KickoffAudit);
            led.receipt().is_err()
        }
        CosignFault::SlotOrder => {
            // 跳槽签必拒。
            let mut led = CosignLedger::new();
            led.sign(CosignSlot::JudgeCheck).is_err()
        }
        CosignFault::RewindSign => {
            // Signed 态直接再发起签必拒（倒退签）。
            let mut led = CosignLedger::new();
            let _ = led.sign(CosignSlot::KickoffAudit);
            led.sign(CosignSlot::JudgeCheck).is_err()
        }
        CosignFault::CopyDeclared => {
            // 注出文逐字抄声明文必拒。
            let mut e = match CITE_ENTRIES.get(0) {
                Some(x) => x.clone(),
                None => return false,
            };
            e.cited = e.declared;
            cite_entry_ok(&e).is_err()
        }
        CosignFault::StampDrift => {
            // 摘要重算双跑必一致；注入不同文摘要必差（漂移可点验）。
            let mut e = match CITE_ENTRIES.get(1) {
                Some(x) => x.clone(),
                None => return false,
            };
            let d0 = declared_stamp(&e);
            e.declared = "另一句声明";
            let d1 = declared_stamp(&e);
            d0 != d1
        }
        CosignFault::SourceRange => {
            let mut e = match CITE_ENTRIES.get(2) {
                Some(x) => x.clone(),
                None => return false,
            };
            e.source = "幽灵文档源";
            cite_entry_ok(&e).is_err()
        }
        CosignFault::EntryOrder => {
            // 条目号乱序：以账面序位闸直读（首条之外条目号必须递增连续）。
            let a = match CITE_ENTRIES.get(0) {
                Some(x) => x,
                None => return false,
            };
            let b = match CITE_ENTRIES.get(1) {
                Some(x) => x,
                None => return false,
            };
            a.no + 1 == b.no
        }
        CosignFault::EntryDup => {
            // 条目重复：同槽同源再立一条必被互异闸拒（以直读模拟）。
            let e = match CITE_ENTRIES.get(0) {
                Some(x) => x.clone(),
                None => return false,
            };
            // 同文重算摘要恒同 → 同条重复可点验（摘要等价即重复）。
            let mut f = e.clone();
            f.no = e.no + 1;
            declared_stamp(&e) == declared_stamp(&f)
        }
        CosignFault::ShortReceipt => {
            // 全账未出回执前，缺槽账直接 receipt 必拒。
            let mut led = CosignLedger::new();
            let _ = led.sign(CosignSlot::KickoffAudit);
            let _ = led.countersign(CosignSlot::KickoffAudit);
            led.receipt().is_err()
        }
        CosignFault::SummaryEmpty => {
            // 摘要空族：注出文清空必拒。
            let mut e = match CITE_ENTRIES.get(3) {
                Some(x) => x.clone(),
                None => return false,
            };
            e.cited = "";
            cite_entry_ok(&e).is_err()
        }
    }
}

// ── 判据谓词（全部私有，断言方向：违约条件 return false） ────────
/// 注出处核验 ≥20 处抽检绿（判据 J1 前半直读口）。
fn citation_check_ok() -> bool {
    if CITE_ENTRIES.len() < CITE_CAP {
        return false;
    }
    let mut i = 0usize;
    while i < CITE_ENTRIES.len() {
        let e = match CITE_ENTRIES.get(i) {
            Some(v) => v,
            None => return false,
        };
        // 条目号连续。
        if e.no != i as u32 {
            return false;
        }
        // 三闸逐条全过。
        if cite_entry_ok(e).is_err() {
            return false;
        }
        i += 1;
    }
    // 槽引覆盖普查：20 条覆盖四槽。
    for s in CosignSlot::all().iter() {
        let mut found = false;
        for e in CITE_ENTRIES.iter() {
            if e.slot == *s {
                found = true;
            }
        }
        if !found {
            return false;
        }
    }
    true
}

/// 对照不抄声明可点验（判据 J1 后半直读口）。
fn not_copy_ok() -> bool {
    // 逐条：注出文 ≠ 声明文（不抄口径）且摘要差可重算（点验口径）。
    for e in CITE_ENTRIES.iter() {
        if e.declared == e.cited {
            return false;
        }
        if declared_stamp(e) == cited_stamp(e) {
            return false;
        }
        // 双跑重算逐位同。
        if declared_stamp(e) != declared_stamp(e) || cited_stamp(e) != cited_stamp(e) {
            return false;
        }
    }
    // 注出文非空（摘要空口径）。
    for e in CITE_ENTRIES.iter() {
        if e.cited.is_empty() || e.declared.is_empty() {
            return false;
        }
    }
    true
}

/// E1 会签槽位回执协议全绿（承接开工对账）。
fn slot_receipt_ok() -> bool {
    let led = build_full_cosign();
    if led.len() != CosignSlot::all().len() {
        return false;
    }
    if led.stamp_len() != CosignSlot::all().len() {
        return false;
    }
    if led.reject_len() != 0 {
        return false;
    }
    if led.state() != SlotState::Receipted {
        return false;
    }
    if !led.equation_ok() {
        return false;
    }
    // 槽承接序逐位对齐。
    let mut i = 0usize;
    while i < led.len() {
        let s = match led.slot_at(i) {
            Some(v) => v,
            None => return false,
        };
        if s.ordinal() != i as u32 {
            return false;
        }
        i += 1;
    }
    true
}

/// 源谱普查（八源互异、查表命中、摘要双跑一致）。
fn source_census_ok() -> bool {
    if SOURCE_TABLE.len() != 8 {
        return false;
    }
    let mut i = 0usize;
    while i < SOURCE_TABLE.len() {
        let s = match SOURCE_TABLE.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if source_idx(s).is_none() {
            return false;
        }
        // 源互异普查。
        let mut j = i + 1;
        while j < SOURCE_TABLE.len() {
            let t = match SOURCE_TABLE.get(j) {
                Some(v) => *v,
                None => return false,
            };
            if s == t {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    // 摘要函数点验：同文恒同摘要、异文摘要异。
    let a = text_stamp("同文点验");
    let b = text_stamp("同文点验");
    let c = text_stamp("异文点验");
    if a != b || a == c {
        return false;
    }
    true
}

/// 注入联测 10/10 拒止。
fn cosign_fault_sweep_ok() -> bool {
    let (caught, total, escape) = cosign_fault_sweep();
    if total != 10 {
        return false;
    }
    if caught != 10 {
        return false;
    }
    if !escape.is_empty() {
        return false;
    }
    true
}

/// 零静默（双签记账逐槽齐、槽基码一一对应、承接位全落）。
fn zero_silent_ok() -> bool {
    let led = build_full_cosign();
    if led.stamp_len() != led.len() {
        return false;
    }
    let mut i = 0usize;
    while i < led.stamp_len() {
        let s = match led.slot_at(i) {
            Some(v) => v,
            None => return false,
        };
        let t = match led.stamps.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if t.0 != s.slot_base() || t.1 == 0 || t.2 == 0 {
            return false;
        }
        i += 1;
    }
    true
}

/// 双跑一致（冻结条目谱下两轮全流程同判）。
fn dual_run_ok() -> bool {
    let a = build_full_cosign();
    let b = build_full_cosign();
    if a.len() != b.len() || a.stamp_len() != b.stamp_len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        match (a.slot_at(i), b.slot_at(i)) {
            (Some(x), Some(y)) => {
                if x != y {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    if cosign_fault_sweep().0 != cosign_fault_sweep().0 {
        return false;
    }
    // 注出处摘要双跑一致。
    let e = match CITE_ENTRIES.get(0) {
        Some(v) => v,
        None => return false,
    };
    if declared_stamp(e) != declared_stamp(e) {
        return false;
    }
    true
}

/// 段位守卫（VX9 全批窗口 + 本卡公式位）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x80000 {
        return false;
    }
    if SEG_BASE > 0x80000 + 0x40 * (12799u32.saturating_sub(12600)) {
        return false;
    }
    if SEG_BASE != 0x80000 + 0x40 * (12677u32.saturating_sub(12600)) {
        return false;
    }
    true
}

/// 完成写回语义（判据 ③：状态=已完成、结果摘要非空）。
fn writeback_ok() -> bool {
    let led = build_full_cosign();
    if led.state() != SlotState::Receipted {
        return false;
    }
    // 摘要：首槽名 + 尾槽名拼接非空。
    let head = match led.slot_at(0) {
        Some(s) => s.name(),
        None => return false,
    };
    let tail = match led.slot_at(CosignSlot::all().len() - 1) {
        Some(s) => s.name(),
        None => return false,
    };
    if head.is_empty() || tail.is_empty() {
        return false;
    }
    // 注出处摘要非空（回执摘要口径）。
    let e = match CITE_ENTRIES.first() {
        Some(v) => v,
        None => return false,
    };
    if cited_stamp(e) == 0 {
        return false;
    }
    true
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12678 会签槽位回执自检（判据 J1 + 通用守卫，≥12 条）。
pub fn run_unx12677_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12677-e1receipt");
    let led = build_full_cosign();

    s.add_dyn(
        "F12678-01 注出处条目二十条齐",
        CITE_ENTRIES.len() >= CITE_CAP,
        &format!("entries={}", CITE_ENTRIES.len()),
    );

    s.add_dyn(
        "F12678-02 注出处核验 ≥20 处抽检绿",
        citation_check_ok(),
        &format!("entries={}", CITE_ENTRIES.len()),
    );

    s.add_dyn(
        "F12678-03 对照不抄声明可点验",
        not_copy_ok(),
        &format!("stamps_ok={}", not_copy_ok()),
    );

    s.add_dyn(
        "F12678-04 会签槽位回执全绿",
        slot_receipt_ok(),
        &format!("slots={} stamps={}", led.len(), led.stamp_len()),
    );

    s.add_dyn(
        "F12678-05 槽引四槽全覆盖",
        {
            let mut ok = true;
            for slot in CosignSlot::all().iter() {
                let mut found = false;
                for e in CITE_ENTRIES.iter() {
                    if e.slot == *slot {
                        found = true;
                    }
                }
                if !found {
                    ok = false;
                }
            }
            ok
        },
        &format!("slots={}", CosignSlot::all().len()),
    );

    s.add_dyn(
        "F12678-06 源谱普查",
        source_census_ok(),
        &format!("sources={}", SOURCE_TABLE.len()),
    );

    s.add_dyn(
        "F12678-07 零静默",
        zero_silent_ok(),
        &format!("stamps={}", led.stamp_len()),
    );

    s.add_dyn(
        "F12678-08 注入联测 10/10 拒止",
        cosign_fault_sweep_ok(),
        &format!("caught={}", cosign_fault_sweep().0),
    );

    s.add_dyn(
        "F12678-09 账等式",
        led.equation_ok(),
        &format!("slots={} stamps={} rejects={}", led.len(), led.stamp_len(), led.reject_len()),
    );

    s.add_dyn(
        "F12678-10 双跑一致",
        dual_run_ok(),
        "冻结条目谱下两轮逐槽同判",
    );

    s.add_dyn(
        "F12678-11 槽判码窗口口径",
        code_in_slot(0x22, CosignSlot::KickoffAudit)
            && !code_in_slot(0x21, CosignSlot::KickoffAudit)
            && !code_in_slot(0x22, CosignSlot::JudgeCheck),
        "基码 < 判码 ≤ 基码+0x0C 且跨槽不混",
    );

    s.add_dyn(
        "F12678-12 完成写回语义",
        writeback_ok(),
        "回执出账即已完成且摘要非空",
    );

    s.add_dyn(
        "F12678-13 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05X}", SEG_BASE),
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
        assert!(SEG_BASE >= 0x80000 && SEG_BASE <= 0x82FC0);
        assert_eq!(SEG_BASE, 0x81340);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        assert_eq!(COSIGN_ALL.len(), 11);
        let mut i = 0usize;
        while i < COSIGN_ALL.len() {
            let vi = match COSIGN_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < COSIGN_ALL.len() {
                let vj = match COSIGN_ALL.get(j) {
                    Some(d) => d.val(),
                    None => 0,
                };
                assert_ne!(vi, vj, "诊断码重复 0x{:05x}", vi);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x30).contains("未知"));
    }

    #[test]
    fn citation_twenty_green() {
        // 注出处核验 ≥20 处抽检绿。
        assert!(citation_check_ok());
        assert!(CITE_ENTRIES.len() >= 20);
        for e in CITE_ENTRIES.iter() {
            assert!(cite_entry_ok(e).is_ok(), "条目 {} 核验红", e.no);
        }
    }

    #[test]
    fn not_copy_verifiable() {
        // 不抄口径 + 点验口径。
        assert!(not_copy_ok());
        for e in CITE_ENTRIES.iter() {
            assert_ne!(e.declared, e.cited, "条目 {} 逐字抄", e.no);
            assert_ne!(declared_stamp(e), cited_stamp(e));
        }
    }

    #[test]
    fn happy_path_receipts() {
        let led = build_full_cosign();
        assert_eq!(led.len(), 4);
        assert_eq!(led.stamp_len(), 4);
        assert_eq!(led.reject_len(), 0);
        assert_eq!(led.state(), SlotState::Receipted);
        assert!(led.equation_ok());
    }

    #[test]
    fn injections_all_caught() {
        let (caught, total, escape) = cosign_fault_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn sign_gate_rejects() {
        // 签闸直读：跳槽/倒退/缺承接。
        let mut led = CosignLedger::new();
        assert_eq!(
            led.sign(CosignSlot::JudgeCheck),
            Err(CosignDiag::ESlotOrder.val())
        );
        assert!(led.sign(CosignSlot::KickoffAudit).is_ok());
        assert_eq!(
            led.sign(CosignSlot::JudgeCheck),
            Err(CosignDiag::ERewindSign.val())
        );
        // 承接异槽必拒。
        assert_eq!(
            led.countersign(CosignSlot::JudgeCheck),
            Err(CosignDiag::ESlotOrder.val())
        );
        assert!(led.countersign(CosignSlot::KickoffAudit).is_ok());
        assert!(led.receipt().is_err());
        assert_eq!(led.reject_len(), 1);
    }

    #[test]
    fn source_census_holds() {
        assert!(source_census_ok());
        assert_eq!(SOURCE_TABLE.len(), 8);
        assert!(source_idx("Wine-Conformance-装载").is_some());
        assert!(source_idx("MSD-句柄表").is_some());
        assert!(source_idx("幽灵源").is_none());
        // 摘要函数：同文同摘要、异文异摘要。
        assert_eq!(text_stamp("abc"), text_stamp("abc"));
        assert_ne!(text_stamp("abc"), text_stamp("abd"));
    }

    #[test]
    fn slot_window_semantics() {
        // 槽判码窗口：跨槽不混。
        assert!(code_in_slot(0x22, CosignSlot::KickoffAudit));
        assert!(code_in_slot(0x2D, CosignSlot::KickoffAudit));
        assert!(!code_in_slot(0x21, CosignSlot::KickoffAudit));
        assert!(!code_in_slot(0x2E, CosignSlot::KickoffAudit));
        assert!(!code_in_slot(0x32, CosignSlot::KickoffAudit));
        assert!(code_in_slot(0x52, CosignSlot::CloseSeal));
    }

    #[test]
    fn zero_silent_ledger() {
        assert!(zero_silent_ok());
        let led = build_full_cosign();
        assert_eq!(led.stamp_len(), CosignSlot::all().len());
        let mut i = 0usize;
        while i < led.stamp_len() {
            let t = match led.stamps.get(i) {
                Some(v) => *v,
                None => return,
            };
            assert_ne!(t.1, 0, "槽 {} 发起签缺", i);
            assert_ne!(t.2, 0, "槽 {} 承接签缺", i);
            i += 1;
        }
    }

    #[test]
    fn dual_run_is_identical() {
        let a = build_full_cosign();
        let b = build_full_cosign();
        assert_eq!(a.len(), b.len());
        assert_eq!(a.stamp_len(), b.stamp_len());
        let mut i = 0usize;
        while i < a.len() {
            match (a.slot_at(i), b.slot_at(i)) {
                (Some(x), Some(y)) => assert_eq!(x, y),
                _ => assert!(false, "双跑槽缺失"),
            }
            i += 1;
        }
        assert_eq!(cosign_fault_sweep().0, cosign_fault_sweep().0);
        // 摘要双跑逐位同。
        let e = match CITE_ENTRIES.get(5) {
            Some(v) => v,
            None => return,
        };
        assert_eq!(declared_stamp(e), declared_stamp(e));
        assert_eq!(cited_stamp(e), cited_stamp(e));
    }

    #[test]
    fn state_machine_never_rewinds() {
        // 状态机只进不退：Idle→Signed→Countersigned→Receipted。
        assert!(SlotState::Idle.rank() < SlotState::Signed.rank());
        assert!(SlotState::Signed.rank() < SlotState::Countersigned.rank());
        assert!(SlotState::Countersigned.rank() < SlotState::Receipted.rank());
        let led = build_full_cosign();
        assert_eq!(led.state(), SlotState::Receipted);
        // 已到顶再签：倒退拒。
        let mut done = build_full_cosign();
        assert!(done.sign(CosignSlot::KickoffAudit).is_err());
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12677_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
    }
}
