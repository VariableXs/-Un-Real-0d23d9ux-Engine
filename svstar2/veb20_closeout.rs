//! VE-F0220 · virtio 组收口与 Intel 组移交（VE-B 域 · virtio 组收口 · 目标 360 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0220`
//!
//! **判据（锚点原文五条）**：十件套、双签、经验包、缺陷清零、判据。
//!
//! 本单是 B01 组（VE-F0201~VE-F0219）的**收口闸门**，向下游 F0221 移交。
//! 它不做新的驱动功能，只把「这个组可以收了」这件事做成**可核验的账**。
//!
//! # 头注要点（每条都是判据的反面，写在这里供判据引用）
//!
//! ## 要点一：收口不是「我觉得差不多了」
//!
//! 收口的判定物是**十件套清单**：十个槽位逐个点名，缺项即阻断。
//! 「大概齐了」不可判定，「十槽全绿且每槽有证据指针」才可判定。
//!
//! ## 要点二：性能基线必须诚实分档
//!
//! 三档（QEMU 软件渲染 / virgl 硬件穿透 / Venus）**不是三种高低**，
//! 是**三种跑在不同渲染路径上的不同东西**，数字之间不可比。
//! 因此每档必须带 `comparable`（是否可与其它档比）与 `sample_count`
//! （样本数）。**样本不足的档标`Experimental`（实验档）而不是删掉或
//! 悄悄按低值算平均**——删掉等于假装没这档，按低值算等于伪造精度。
//!
//! ## 要点三：双签是两个不同角色的独立确认
//!
//! 一签（收口人）说「我交了」，一签（复核人）说「我查了」。**两个签必须
//! 来自不同角色**（`signer` 字段），同一人签两次不构成双签——那只是一个人
//! 点了两下。双签缺任一方 ⇒ `Blocked`（阻断收口），不降级、不代签。
//!
//! ## 要点四：缺陷总账三色各有归宿
//!
//! 🔴红（`Blocking`）必须**清零**才能收口；🟡黄（`Closed`）必须
//! **有闭环证据**（关闭注记 + 复验记录）；🟢绿（`Registered`）只需
//! **登记入册**。「已关闭」不等于「有闭环证据」——只写一句「已修」
//! 而没有复验记录的，算`Opened` 不是 `Closed`。
//!
//! ## 要点五：移交后缺陷要能按总账回溯
//!
//! 缺陷总账的每一条都带**归属单号**（`owner`）。移交后若 Intel 组
//! 发现 virtio 侧缺陷，凭`owner` 就能定位回是哪一单留下的，
//! 而不是「反正 virtio 组已收口，无处可查」。
//!
//! ## 要点六：经验包是引用不是复制
//!
//! 移交经验包含「virtio 标准寄存器语义」与「能力探测方法论」两条
//! 方法论，每条引用**具体条目**（`ItemRef` = 层 + 单号 + 槽位）。
//! **复制粘贴整段实现不叫经验包**——下游要的是「去哪看」，不是
//! 「抄一份，改起来就是两份」。
//!
//! # 范围红线
//!
//! 本单**只读**上游数据，不改上游状态（收口是记账不是施工）。
//! 移交包**不含**任何 virtio 寄存器写路径——本单零 IO、零硬件访问。

// ---------------------------------------------------------------------------
// 导入（**三件套齐全**：`vec!` 宏与 `Vec` 类型是两个命名空间，
// `use alloc::vec::Vec;` 不导入 `vec!`；`String` 还需 `ToString`）
// ---------------------------------------------------------------------------

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ===========================================================================
// §0 诊断层（零静默的物质基础：错误与告警分两条独立通道）
// ===========================================================================

/// 收口阶段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// 尚未开始收口。
    NotStarted,
    /// 十件套清点中。
    Checklist,
    /// 缺陷总账清点中。
    Defects,
    /// 性能基线分档标注中。
    Baseline,
    /// 双签等待中。
    Signing,
    /// 已移交。
    HandedOver,
}

impl Phase {
    pub const COUNT: usize = 6;

    pub fn index(self) -> usize {
        match self {
            Phase::NotStarted => 0,
            Phase::Checklist => 1,
            Phase::Defects => 2,
            Phase::Baseline => 3,
            Phase::Signing => 4,
            Phase::HandedOver => 5,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Phase::NotStarted => "not_started",
            Phase::Checklist => "checklist",
            Phase::Defects => "defects",
            Phase::Baseline => "baseline",
            Phase::Signing => "signing",
            Phase::HandedOver => "handed_over",
        }
    }
}

/// 收口诊断码（B 域自有段，**不复用** 其他域的码型）。
///
/// 码段 `0x2Cxx`：VE-F2408（VE-M 域）占 `0x2Bxx`，F0220 顺延取 `0x2C`。
///
/// `Copy` 是必需的（不是可选派生）：`Diag` 按值进出、判据按码比对，
/// 不派生 `Copy`/`PartialEq` 就得处处 `&` 或靠 `eq` 方法绕，
/// 而「码相等」在收口账里是最基础的操作，不该靠自定义方法绕。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct CloseoutDiag(pub u16);

impl CloseoutDiag {
    /// 十件套有缺项 ⇒ 阻断收口，须回补。
    pub const CHECKLIST_INCOMPLETE: CloseoutDiag = CloseoutDiag(0x2C01);
    /// 缺陷总账仍有红项 ⇒ 不得收口。
    pub const RED_DEFECT_OPEN: CloseoutDiag = CloseoutDiag(0x2C02);
    /// 黄项标为已关闭但无闭环证据 ⇒ 判据拒绝该标注。
    pub const CLOSED_WITHOUT_EVIDENCE: CloseoutDiag = CloseoutDiag(0x2C03);
    /// 缺双签任一方。
    pub const SIGN_MISSING: CloseoutDiag = CloseoutDiag(0x2C04);
    /// 双签两签同角色 ⇒ 不构成双签。
    pub const SIGN_SAME_ROLE: CloseoutDiag = CloseoutDiag(0x2C05);
    /// 性能档样本不足却标为已认证档。
    pub const BASELINE_UNDER_SAMPLED: CloseoutDiag = CloseoutDiag(0x2C06);
    /// 经验包引用了不存在的条目（悬空引用）。
    pub const REF_DANGLING: CloseoutDiag = CloseoutDiag(0x2C07);
    /// 阶段跃迁非法（未过清点直接进双签）。
    pub const PHASE_SKIPPED: CloseoutDiag = CloseoutDiag(0x2C08);
}

/// 诊断严重度（**告警与错误分两条独立通道**，不合并成一位 flag）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    /// 告警（P1）：不阻断收口，但必须留在报告里。
    Warn,
    /// 错误（P0）：阻断收口。
    Block,
}

/// 一条诊断记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Diag {
    pub code: CloseoutDiag,
    pub severity: Severity,
}

/// 诊断袋（定容，覆写最旧；**覆写本身记诊断**，不静默丢证据）。
#[derive(Clone, Debug)]
pub struct DiagBag {
    items: Vec<Diag>,
    pub overwritten: u32,
}

impl DiagBag {
    pub const CAPACITY: usize = 32;

    pub fn new() -> DiagBag {
        DiagBag { items: alloc::vec::Vec::new(), overwritten: 0 }
    }

    pub fn push(&mut self, code: CloseoutDiag, severity: Severity) {
        if self.items.len() >= DiagBag::CAPACITY {
            // **覆写最旧并记账**：静默丢会让「报告里没有阻断项」与
            // 「阻断项被挤掉了」长得一模一样。
            self.items.remove(0);
            self.overwritten += 1;
        }
        self.items.push(Diag { code, severity });
    }

    pub fn block(&mut self, code: CloseoutDiag) {
        self.push(code, Severity::Block);
    }

    pub fn warn(&mut self, code: CloseoutDiag) {
        self.push(code, Severity::Warn);
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn has_block(&self) -> bool {
        self.items.iter().any(|d| d.severity == Severity::Block)
    }

    /// 是否存在**告警**项（与 [`DiagBag::has_block`] 分列两条通道）。
    ///
    /// 刻意不提供「has_anything」：合并成一个 flag 后，「只有告警」
    /// 与「有阻断」就分辨不出来了——而这两者对收口的后果完全不同
    /// （前者可放行，后者必须阻断）。
    pub fn has_warn(&self) -> bool {
        self.items.iter().any(|d| d.severity == Severity::Warn)
    }

    /// 告警项条数（与阻断项条数分列，避免二者相混）。
    pub fn warn_count(&self) -> u32 {
        self.items.iter().filter(|d| d.severity == Severity::Warn).count() as u32
    }

    /// 阻断项条数。
    pub fn block_count(&self) -> u32 {
        self.items.iter().filter(|d| d.severity == Severity::Block).count() as u32
    }

    /// 该码是否出现过（**跨两种严重度查**：同一码先告警后阻断也算出现）。
    pub fn has(&self, code: CloseoutDiag) -> bool {
        self.items.iter().any(|d| d.code == code)
    }

    pub fn count_of(&self, code: CloseoutDiag) -> usize {
        self.items.iter().filter(|d| d.code == code).count()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Diag> {
        self.items.iter()
    }
}

// ===========================================================================
// §1 十件套清单（收口判定的物质基础）
// ===========================================================================

/// 证据层：锚点「协议层、功能层、恢复层测试证据齐备」。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvidenceLayer {
    /// 协议层：状态机序与特性协商。
    Protocol,
    /// 功能层：资源/2D/多头。
    Functional,
    /// 恢复层：注入错误验证 F0212 处置。
    Recovery,
}

impl EvidenceLayer {
    pub const COUNT: usize = 3;

    pub fn index(self) -> usize {
        match self {
            EvidenceLayer::Protocol => 0,
            EvidenceLayer::Functional => 1,
            EvidenceLayer::Recovery => 2,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            EvidenceLayer::Protocol => "protocol",
            EvidenceLayer::Functional => "functional",
            EvidenceLayer::Recovery => "recovery",
        }
    }
}

/// 十件套槽位（**十个槽位，缺一即阻断**，顺序即判定顺序）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    /// 协议层测试证据。
    ProtocolEvidence,
    /// 功能层测试证据。
    FunctionalEvidence,
    /// 恢复层测试证据。
    RecoveryEvidence,
    /// 兼容矩阵覆盖确认。
    CompatCoverage,
    /// 参考驱动宣告核验。
    DeclarationProof,
    /// 联调通道验证。
    DebugChannelProof,
    /// 缺陷总账清点。
    DefectLedger,
    /// 性能基线三档标注。
    BaselineTiers,
    /// 无障碍承诺核验行。
    AccessibilityRow,
    /// 移交经验包齐备。
    HandoverPack,
}

impl Slot {
    pub const COUNT: usize = 10;

    pub fn index(self) -> usize {
        match self {
            Slot::ProtocolEvidence => 0,
            Slot::FunctionalEvidence => 1,
            Slot::RecoveryEvidence => 2,
            Slot::CompatCoverage => 3,
            Slot::DeclarationProof => 4,
            Slot::DebugChannelProof => 5,
            Slot::DefectLedger => 6,
            Slot::BaselineTiers => 7,
            Slot::AccessibilityRow => 8,
            Slot::HandoverPack => 9,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Slot::ProtocolEvidence => "protocol_evidence",
            Slot::FunctionalEvidence => "functional_evidence",
            Slot::RecoveryEvidence => "recovery_evidence",
            Slot::CompatCoverage => "compat_coverage",
            Slot::DeclarationProof => "declaration_proof",
            Slot::DebugChannelProof => "debug_channel_proof",
            Slot::DefectLedger => "defect_ledger",
            Slot::BaselineTiers => "baseline_tiers",
            Slot::AccessibilityRow => "accessibility_row",
            Slot::HandoverPack => "handover_pack",
        }
    }

    /// 该槽位要求的证据层（`None` = 不限层，只看指针非空）。
    pub fn layer(self) -> Option<EvidenceLayer> {
        match self {
            Slot::ProtocolEvidence => Some(EvidenceLayer::Protocol),
            Slot::FunctionalEvidence => Some(EvidenceLayer::Functional),
            Slot::RecoveryEvidence => Some(EvidenceLayer::Recovery),
            _ => None,
        }
    }

    pub const ALL: [Slot; Slot::COUNT] = [
        Slot::ProtocolEvidence,
        Slot::FunctionalEvidence,
        Slot::RecoveryEvidence,
        Slot::CompatCoverage,
        Slot::DeclarationProof,
        Slot::DebugChannelProof,
        Slot::DefectLedger,
        Slot::BaselineTiers,
        Slot::AccessibilityRow,
        Slot::HandoverPack,
    ];
}

/// 证据指针（**引用而非拷贝**：下游按指针回查上游，不复制实现）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EvidenceRef {
    /// 归属单号（如 `VE-F0215`）。
    pub task: u32,
    /// 槽位下标（指向该单内的一个可回查位置）。
    pub slot: u16,
}

impl EvidenceRef {
    pub const fn new(task: u32, slot: u16) -> EvidenceRef {
        EvidenceRef { task, slot }
    }

    pub fn is_valid(self) -> bool {
        self.task != 0
    }
}

/// 一个槽位的登记内容。
#[derive(Clone, Debug)]
pub struct SlotEntry {
    pub slot: Slot,
    /// 证据指针（**空指针 = 缺项**，判为阻断）。
    pub evidence: Option<EvidenceRef>,
    /// 通过用例数（0 且无指针 ⇒ 缺项）。
    pub passed: u32,
}

impl SlotEntry {
    pub fn new(slot: Slot, evidence: Option<EvidenceRef>, passed: u32) -> SlotEntry {
        SlotEntry { slot, evidence, passed }
    }

    /// 该槽是否已填。
    ///
    /// **填 = 有非空证据指针**。`passed` 为 0 但有指针算填（用例全 N/A
    /// 也是一种结论）；无指针一律不填——不然「跑了个寂寞」会被算成已填。
    pub fn is_filled(&self) -> bool {
        match self.evidence {
            Some(e) => e.is_valid(),
            None => false,
        }
    }
}

/// 十件套清单。
#[derive(Clone, Debug)]
pub struct Checklist {
    entries: Vec<SlotEntry>,
}

impl Checklist {
    pub fn new() -> Checklist {
        Checklist { entries: alloc::vec::Vec::new() }
    }

    /// 全空清单（十槽皆未填）。
    pub fn blank() -> Checklist {
        let mut c = Checklist::new();
        for s in Slot::ALL {
            c.entries.push(SlotEntry::new(s, None, 0));
        }
        c
    }

    /// 追加一条登记（**允许重复登记同槽位**：清点器要能记录
    /// 「同槽位被登记两次」这种脏输入，而不是悄悄去重把问题藏起来）。
    pub fn push_entry(&mut self, e: SlotEntry) {
        self.entries.push(e);
    }

    pub fn get(&self, slot: Slot) -> Option<&SlotEntry> {
        self.entries.iter().find(|e| e.slot == slot)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn filled(&self) -> u32 {
        self.entries.iter().filter(|e| e.is_filled()).count() as u32
    }

    pub fn missing(&self) -> u32 {
        (Slot::COUNT as u32).saturating_sub(self.filled())
    }

    /// 缺项槽位清单（**逐个点名**，不只给个数）。
    pub fn missing_slots(&self) -> Vec<Slot> {
        let mut out = alloc::vec::Vec::new();
        for s in Slot::ALL {
            match self.get(s) {
                Some(e) if e.is_filled() => {}
                _ => out.push(s),
            }
        }
        out
    }

    pub fn entries(&self) -> impl Iterator<Item = &SlotEntry> {
        self.entries.iter()
    }
}

impl Default for Checklist {
    fn default() -> Checklist {
        Checklist::blank()
    }
}

// ===========================================================================
// §2 缺陷总账（锚点「🔴清零、🟡闭环、🟢登记」）
// ===========================================================================

/// 缺陷三色。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity3 {
    /// 🔴红：阻断收口，必须清零。
    Blocking,
    /// 🟡黄：须有闭环证据。
    Closed,
    /// 🟢绿：登记入册即可。
    Registered,
}

impl Severity3 {
    pub const COUNT: usize = 3;

    pub fn index(self) -> usize {
        match self {
            Severity3::Blocking => 0,
            Severity3::Closed => 1,
            Severity3::Registered => 2,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Severity3::Blocking => "blocking",
            Severity3::Closed => "closed",
            Severity3::Registered => "registered",
        }
    }
}

/// 一条缺陷记录。
///
/// `Copy` 不可加：`closure_note` 是 [`String`]，非 `Copy`——
/// 按值传递缺陷记录会诱使人随手克隆，账本一大就成倍分配。
/// 这里只 `Clone`。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Defect {
    /// 缺陷编号（组内唯一，从 1 起）。
    pub id: u32,
    /// 归属单号（**移交后回溯定位的钥匙**）。
    pub owner: u32,
    /// 归档颜色。
    pub severity: Severity3,
    /// 闭环证据注记（**空串 = 无闭环证据**）。
    pub closure_note: String,
    /// 是否已复验（**「已修」不等于「已复验」**）。
    pub reverified: bool,
}

impl Defect {
    pub const fn new(id: u32, owner: u32, severity: Severity3) -> Defect {
        Defect { id, owner, severity, closure_note: String::new(), reverified: false }
    }

    /// 附闭环证据（**必须同时给注记与复验**，只给其一如实判未闭环）。
    pub fn close(&mut self, note: &str, reverified: bool) {
        self.closure_note = String::from(note);
        self.reverified = reverified;
    }

    /// 是否**真闭环**（黄项的唯一过关口径）。
    ///
    /// 三要件：颜色是黄、注记非空、**已复验**。缺任一即未闭环——
    /// 只写「已修」没复验记录，等于把「改完了」当成「改对了」。
    pub fn is_closed(&self) -> bool {
        self.severity == Severity3::Closed
            && !self.closure_note.is_empty()
            && self.reverified
    }

    /// 绿项只需入册（**不要求闭环证据**：登记不是修复）。
    pub fn is_registered(&self) -> bool {
        self.severity == Severity3::Registered && self.owner != 0
    }
}

/// 缺陷总账清点结论。
#[derive(Clone, Debug)]
pub struct DefectTally {
    pub blocking_open: u32,
    pub closed_ok: u32,
    pub closed_missing_evidence: u32,
    pub registered_ok: u32,
    pub registered_bad_owner: u32,
    pub total: u32,
}

impl DefectTally {
    /// 红项是否已清零（**清零 = 没有仍在阻断的红项**）。
    pub fn red_cleared(&self) -> bool {
        self.blocking_open == 0
    }

    /// 全账是否可收口（红清零 且 黄全闭环 且 绿全入册）。
    pub fn clean(&self) -> bool {
        self.red_cleared()
            && self.closed_missing_evidence == 0
            && self.registered_bad_owner == 0
    }
}

/// 清点缺陷总账。
///
/// **逐条独立判定**：一条黄项缺证据不影响另一条的结论——
/// 「合并成一条『有无黄项问题』」会让「20 条里19 条有证据」被算成
/// 「有黄项问题」，计数就失真了。
pub fn tally_defects(defects: &[Defect]) -> DefectTally {
    let mut t = DefectTally {
        blocking_open: 0,
        closed_ok: 0,
        closed_missing_evidence: 0,
        registered_ok: 0,
        registered_bad_owner: 0,
        total: defects.len() as u32,
    };
    for d in defects {
        match d.severity {
            // **红项只有两种归宿**：闭环（调用方先改 `severity` 降为黄）
            // 或清零（从账上移除）。**不存在「红项已接受」**，所以只要
            // 它还在账上就一律计未清零——原先这里写成两臂相同的 if/else，
            // 那个分支不表达任何判断，纯属噪音。
            Severity3::Blocking => {
                t.blocking_open += 1;
            }
            Severity3::Closed => {
                if d.is_closed() {
                    t.closed_ok += 1;
                } else {
                    t.closed_missing_evidence += 1;
                }
            }
            Severity3::Registered => {
                if d.is_registered() {
                    t.registered_ok += 1;
                } else {
                    t.registered_bad_owner += 1;
                }
            }
        }
    }
    t
}

// ===========================================================================
// §3 性能基线三档（锚点「诚实标注」）
// ===========================================================================

/// 基线档位（**三种渲染路径，不是三种高低**）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BaselineTier {
    /// QEMU 软件渲染。
    QemuSoftware,
    /// virgl 硬件穿透。
    VirglPassthrough,
    /// Venus。
    Venus,
}

impl BaselineTier {
    pub const COUNT: usize = 3;

    pub fn index(self) -> usize {
        match self {
            BaselineTier::QemuSoftware => 0,
            BaselineTier::VirglPassthrough => 1,
            BaselineTier::Venus => 2,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            BaselineTier::QemuSoftware => "qemu_software",
            BaselineTier::VirglPassthrough => "virgl_passthrough",
            BaselineTier::Venus => "venus",
        }
    }

    /// **基线数据是否够格进对比**（样本数门槛）。
    ///
    /// 门槛 5：少于 5 个样本的均值波动会大于档间差异，拿它比高低
    /// 是在比噪声。**不够格的档不是删掉，而是标实验档**。
    pub const MIN_SAMPLES: u32 = 5;

    pub fn enough(self, samples: u32) -> bool {
        samples >= BaselineTier::MIN_SAMPLES
    }
}
/// 认证状态（**够格/ 不够格两态，没有中间态**）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BaselineGrade {
    /// 已认证：可进对比。
    Certified,
    /// 实验档：数据在，但**不参与档间高低比较**。
    Experimental,
}

/// 一档基线。
#[derive(Clone, Debug)]
pub struct BaselineEntry {
    pub tier: BaselineTier,
    /// 帧时间（毫秒）。
    pub frame_ms: f32,
    /// 样本数。
    pub samples: u32,
    /// 认证状态。
    pub grade: BaselineGrade,
    /// **跨档可比**（不同渲染路径的数字本就不可比，见要点二）。
    pub comparable: bool,
}

impl BaselineEntry {
    /// **非 `const`**：`enough` 是普通 `fn`（含 trait 调用），在 `const`
    /// 上下文里调用会E0015。认证状态是构造期计算，不要求编译期求值。
    pub fn new(tier: BaselineTier, frame_ms: f32, samples: u32) -> BaselineEntry {
        // **认证状态由样本数推导，不由构造者自报**：
        // 自报就等于「我可以宣称这档可信」，那是自证式。
        let grade = if tier.enough(samples) {
            BaselineGrade::Certified
        } else {
            BaselineGrade::Experimental
        };
        // **可比性恒为 false**：三档跑在不同渲染路径上。把它做成
        // 「由参数决定」是给下游留一个「挑一个能比的档」的假选项。
        BaselineEntry { tier, frame_ms, samples, grade, comparable: false }
    }

    pub fn is_certified(&self) -> bool {
        self.grade == BaselineGrade::Certified
    }

    /// 样本是否够格（**判据侧独立重算**，不信 `grade` 字段）。
    pub fn samples_ok(&self) -> bool {
        self.tier.enough(self.samples)
    }
}

/// 校验三档标注（**逐档独立判定**，并对「样本不足却标认证」记码）。
pub fn audit_baselines(entries: &[BaselineEntry], bag: &mut DiagBag) {
    for e in entries {
        if e.is_certified() && !e.samples_ok() {
            // **够格与自报冲突**：这是必须暴露的分歧，不能悄悄按自报走。
            bag.warn(CloseoutDiag::BASELINE_UNDER_SAMPLED);
        }
        if e.comparable {
            // 跨渲染路径的数字不可比，标可比即产生错误比较。
            bag.warn(CloseoutDiag::BASELINE_UNDER_SAMPLED);
        }
    }
}

// ===========================================================================
// §4 双签（锚点「双签后移交」）
// ===========================================================================

/// 签署角色。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignerRole {
    /// 收口人：负责「我交了」。
    Submitter,
    /// 复核人：负责「我查了」。
    Reviewer,
}

impl SignerRole {
    pub const COUNT: usize = 2;

    pub fn index(self) -> usize {
        match self {
            SignerRole::Submitter => 0,
            SignerRole::Reviewer => 1,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            SignerRole::Submitter => "submitter",
            SignerRole::Reviewer => "reviewer",
        }
    }
}

/// 一枚签。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Signature {
    pub role: SignerRole,
    /// 签署人标识（工号/名字的稳定散列，非 0 有效）。
    pub signer: u32,
}

impl Signature {
    pub const fn new(role: SignerRole, signer: u32) -> Signature {
        Signature { role, signer }
    }
}

/// 双签结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignState {
    /// 双签齐备且两签不同人。
    DoubleSigned,
    /// 缺任一签。
    Incomplete,
    /// 两签同一人（同人签两次不构成双签）。
    SameSigner,
}

/// 判定双签（**三态互斥**，不给「差不多」的口子）。
pub fn judge_signs(sigs: &[Signature]) -> SignState {
    let submitter = sigs.iter().find(|s| s.role == SignerRole::Submitter);
    let reviewer = sigs.iter().find(|s| s.role == SignerRole::Reviewer);
    match (submitter, reviewer) {
        (Some(a), Some(b)) => {
            if a.signer == b.signer {
                SignState::SameSigner
            } else {
                SignState::DoubleSigned
            }
        }
        _ => SignState::Incomplete,
    }
}

// ===========================================================================
// §5 移交经验包（锚点「经验包含 virtio 标准寄存器语义与能力探测方法论」）
// ===========================================================================

/// 方法论条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Methodology {
    /// virtio 标准寄存器语义。
    RegisterSemantics,
    /// 能力探测方法论。
    CapabilityProbe,
}

impl Methodology {
    pub const COUNT: usize = 2;

    pub fn index(self) -> usize {
        match self {
            Methodology::RegisterSemantics => 0,
            Methodology::CapabilityProbe => 1,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Methodology::RegisterSemantics => "virtio_register_semantics",
            Methodology::CapabilityProbe => "capability_probe",
        }
    }

    pub const ALL: [Methodology; Methodology::COUNT] =
        [Methodology::RegisterSemantics, Methodology::CapabilityProbe];
}

/// 条目引用（**层 + 单号 + 槽位**，三段齐全才算有效引用）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ItemRef {
    /// 证据层（`None` = 不限层，如方法论条目挂在单号而非层上）。
    pub layer: Option<EvidenceLayer>,
    /// 归属单号。
    pub task: u32,
    /// 槽位下标。
    pub slot: u16,
}

impl ItemRef {
    pub const fn new(layer: Option<EvidenceLayer>, task: u32, slot: u16) -> ItemRef {
        ItemRef { layer, task, slot }
    }

    /// 引用是否有效（**单号非 0**；`slot == 0` 合法——第 0 槽也是槽）。
    pub fn is_dangling(self) -> bool {
        self.task == 0
    }
}

/// 一条方法论及其引用集。
#[derive(Clone, Debug)]
pub struct MethodNote {
    pub methodology: Methodology,
    /// 引用条目（**至少一条**）。
    pub refs: Vec<ItemRef>,
}

impl MethodNote {
    pub fn new(methodology: Methodology) -> MethodNote {
        MethodNote { methodology, refs: alloc::vec::Vec::new() }
    }

    pub fn push(&mut self, r: ItemRef) {
        self.refs.push(r);
    }

    /// 该方法论条目是否完备（**有引用 且 无悬空**）。
    pub fn is_complete(&self) -> bool {
        !self.refs.is_empty() && !self.refs.iter().any(|r| r.is_dangling())
    }

    pub fn dangling_count(&self) -> u32 {
        self.refs.iter().filter(|r| r.is_dangling()).count() as u32
    }
}

/// 最低支持 QEMU 版本（**单源引用 F0216 宣告的 `MIN_QEMU`**）。
///
/// 刻意**不写 `super::veb16_declaration::MIN_QEMU`**：跨模块引用会把本模块
/// 的编译成败绑到 F0216 上——那一条若是他人的半成品（E0433 未解析模块），
/// 本模块会被无辜拖垮，而那种失败长得和真实缺陷一模一样、极难分辨。
/// 代价是这行数值要与 F0216 一致，故下方以判据「引用一致性」把它钉住：
/// [`crate::svstar2::veb20_checks`] 直接比对两处常量，一旦漂移即判红。
pub const MIN_QEMU_MAJOR: u32 = 6;
pub const MIN_QEMU_MINOR: u32 = 0;

/// 移交包（**引用式，不含任何实现拷贝**）。
#[derive(Clone, Debug)]
pub struct HandoverPack {
    /// 收口组单号（VE-F0220）。
    pub closing_task: u32,
    /// 移交对象组（下游 F0221 Intel 组）。
    pub target_group: u32,
    /// 最低支持 QEMU 版本（**引用 F0216 宣告，不重述数值**）。
    pub min_qemu_major: u32,
    pub min_qemu_minor: u32,
    /// 经验包条目。
    pub notes: Vec<MethodNote>,
}

impl HandoverPack {
    pub fn new(closing_task: u32, target_group: u32) -> HandoverPack {
        // **最低支持版本取本模块的 `MIN_QEMU_*`**（其值即 F0216 宣告值，
        // 由判据比对两侧常量保证不漂移）——不跨模块 `super::` 引用。
        HandoverPack {
            closing_task,
            target_group,
            min_qemu_major: MIN_QEMU_MAJOR,
            min_qemu_minor: MIN_QEMU_MINOR,
            notes: alloc::vec::Vec::new(),
        }
    }

    pub fn push(&mut self, n: MethodNote) {
        self.notes.push(n);
    }

    /// 该方法论是否已收录（按枚举查，不按位置）。
    pub fn has(&self, m: Methodology) -> bool {
        self.notes.iter().any(|n| n.methodology == m)
    }

    /// 两条方法论是否齐备（**逐条判，不看条数**）。
    ///
    /// **同名条目必须全部合格**，不是「找到第一条就算了」：
    /// 原实现用 `find`，于是「先注册一条合格的、再注册一条带悬空引用的
    /// 同名条目」会被判成完备——而移交包照抄下游，坏引用就一起走了。
    /// `dangling_total() > 0` 虽有总闸兜底，但`is_complete` 本身说谎，
    /// 任何复用它的调用方都会被骗。故改为 `filter(..).all(..)`
    /// （空集时 `all` 恒真，故前面先断「至少有一条」）。
    pub fn methodology_complete(&self) -> bool {
        Methodology::ALL.iter().all(|m| {
            let group: Vec<&MethodNote> =
                self.notes.iter().filter(|n| n.methodology == *m).collect();
            match group.first() {
                // 该方法论一条都没登记 ⇒ 不完备
                None => false,
                // 登记了但全部不合格（空引用集或含悬空）⇒ 不完备
                Some(_) => group.iter().all(|n| n.is_complete()),
            }
        })
    }

    /// 某方法论名下是否有**不合格**条目（供报告点名用，不只给个数）。
    pub fn bad_notes(&self, m: Methodology) -> Vec<&MethodNote> {
        self.notes
            .iter()
            .filter(|n| n.methodology == m && !n.is_complete())
            .collect()
    }

    /// 全部悬空引用数（**跨条目累加**）。
    pub fn dangling_total(&self) -> u32 {
        self.notes.iter().map(|n| n.dangling_count()).sum()
    }
}

// ===========================================================================
// §6 收口总闸（十件套 + 缺陷 + 基线 + 双签 + 经验包）
// ===========================================================================

/// 收口结论（**五闸全过才 `Cleared`**，任一不过即 `Blocked`）。
#[derive(Clone, Debug)]
pub struct CloseoutVerdict {
    pub phase: Phase,
    /// 十件套缺项数。
    pub missing_slots: u32,
    /// 红项未清零数。
    pub red_open: u32,
    /// 双签状态。
    pub sign: SignState,
    /// 经验包是否完备。
    pub pack_ok: bool,
    /// 基线实验档数（**登记不阻断**，但必须留在报告里）。
    pub experimental_tiers: u32,
    /// 是否可移交。
    pub cleared: bool,
    /// 移交包（**仅 `cleared == true` 时有效**）。
    pub pack: HandoverPack,
}

impl CloseoutVerdict {
    /// 缺项槽位名清单（**逐个点名**，报告不只给数字）。
    pub fn missing_names(&self, cl: &Checklist) -> Vec<&'static str> {
        cl.missing_slots().iter().map(|s| s.name()).collect()
    }
}

/// 收口总闸。
///
/// **只读，不改上游状态**：本函数把五闸的结论汇成一处，
/// 记码走 `bag`，**不改 `cl` / `defects` / `entries`**。
pub fn close_out(
    cl: &Checklist,
    defects: &[Defect],
    entries: &[BaselineEntry],
    sigs: &[Signature],
    pack: &HandoverPack,
    bag: &mut DiagBag,
) -> CloseoutVerdict {
    // ---- 闸一：十件套 ----
    let missing = cl.missing();
    if missing > 0 {
        bag.block(CloseoutDiag::CHECKLIST_INCOMPLETE);
    }

    // ---- 闸二：缺陷总账 ----
    let tally = tally_defects(defects);
    if !tally.red_cleared() {
        bag.block(CloseoutDiag::RED_DEFECT_OPEN);
    }
    if tally.closed_missing_evidence > 0 {
        // 黄项标「已关闭」却无闭环证据 ⇒ 该标注本身是错的，记码。
        bag.warn(CloseoutDiag::CLOSED_WITHOUT_EVIDENCE);
    }

    // ---- 闸三：性能基线 ----
    audit_baselines(entries, bag);
    let experimental = entries.iter().filter(|e| !e.is_certified()).count() as u32;

    // ---- 闸四：双签 ----
    let sign = judge_signs(sigs);
    match sign {
        SignState::Incomplete => bag.block(CloseoutDiag::SIGN_MISSING),
        SignState::SameSigner => bag.block(CloseoutDiag::SIGN_SAME_ROLE),
        SignState::DoubleSigned => {}
    }

    // ---- 闸五：经验包 ----
    if !pack.methodology_complete() || pack.dangling_total() > 0 {
        bag.block(CloseoutDiag::REF_DANGLING);
    }

    let cleared = missing == 0
        && tally.clean()
        && matches!(sign, SignState::DoubleSigned)
        && pack.methodology_complete()
        && pack.dangling_total() == 0;

    let phase = if cleared { Phase::HandedOver } else { Phase::Checklist };

    CloseoutVerdict {
        phase,
        missing_slots: missing,
        red_open: tally.blocking_open,
        sign,
        pack_ok: pack.methodology_complete(),
        experimental_tiers: experimental,
        cleared,
        pack: pack.clone(),
    }
}

/// 阶段跃迁守卫（**阶段必须逐级过，不许跳**）。
///
/// 锚点错误路径「收口证据缺项→阻断收口回补」意味着**回补后要重走**，
/// 因此「补完直接跳移交」不是合法路径。
pub fn can_advance(from: Phase, to: Phase) -> bool {
    if from == Phase::HandedOver {
        return false; //终态不可再进
    }
    // 允许回退到清点（回补重走），其余只许前进一级。
    if to == Phase::Checklist && from != Phase::Checklist {
        return true;
    }
    to.index() == from.index() + 1
}

/// 跃迁非法时记码。
pub fn advance(from: Phase, to: Phase, bag: &mut DiagBag) -> bool {
    if can_advance(from, to) {
        return true;
    }
    bag.warn(CloseoutDiag::PHASE_SKIPPED);
    false
}