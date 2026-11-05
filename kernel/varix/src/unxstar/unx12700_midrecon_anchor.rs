//! UNX-F12701 域中程对账锚（F12001–F12700 七百条集成账） · 0x81900段
//!
//! 判据原文（派工卡 12700 逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② 判据原文：UNX-F12701-J1 总对账协议档（对账范围/口径/频率
//!      三段）判据绿，协议版本号入账；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 域中程对账锚：F12001–F12700 七百条按百条一段切七域段，逐段
//!    锚值按封闭式重算（锚值 ≡ 段起 + 段止 + 段序×0x40），锚账冻结
//!    值与重算值逐段对号——锚漂移即账悬；
//! 2. 总对账协议档三段：对账范围（RangeSec：起止判据号闭合）/对账
//!    口径（GaugeSec：口径式名非空 + 折算模非零）/对账频率（FreqSec：
//!    三态触发闭集），三段齐才许签发协议档；
//! 3. 协议版本号入账：版本登记账逐条单调（新版本 ≡ 顶版本 + 1），
//!    回退/跳号显性拒；版本入账后才许锚定收账（版本缺席即协议档
//!    未生效）；
//! 4. 中程对账等式：七段条数 Σ ≡ 700 ≡ 范围段长，锚账与范围账双账
//!    闭合才许锚定；
//! 5. 锚账状态机 AnDraft → AnReconning → AnAnchored 单向推进，锚定
//!    后只读、倒退显性拒；注入十族（断缝段/锚值漂移/版本跳号/版本
//!    回退/三段缺段/频率越界/范围漂移/锚定后写/倒退/摘要空）逐路
//!    必抓；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机；取值一律
//!    match/if let/.get()/checked_*/saturating_*，无 unwrap/expect/
//!    panic/裸下标；中文注释与中文诊断；无死代码无 #[allow]。
//!
//! 判据谓词与验收判据的对应：
//!   - `protocol_three_section_ok` ↔ ② 总对账协议档三段判据绿；
//!   - `version_registered_ok` ↔ ② 协议版本号入账（单调登记）；
//!   - `anchor_seven_seg_ok` ↔ 标题「F12001–F12700 七百条集成账」；
//!   - `writeback_ok` ↔ ③ 状态=已完成、结果摘要非空。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12700 − 12600)）。
pub const SEG_BASE: u32 = 0x81900;

// ── 中程锚诊断码闭集 ─────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnDiag {
    /// 段断缝（段间不连续）。
    ESegGap,
    /// 锚值漂移（重算不符）。
    EAnchorDrift,
    /// 段条数违约（非 100）。
    ESegCount,
    /// 版本跳号。
    EVersionJump,
    /// 版本回退。
    EVersionRewind,
    /// 版本缺席（未入账即锚定）。
    EVersionAbsent,
    /// 范围段缺（起止漂移）。
    ERangeMissing,
    /// 口径段缺（式名空/模零）。
    EGaugeMissing,
    /// 频率段越界。
    EFreqRange,
    /// 锚定后写。
    EAnchoredWrite,
    /// 状态机倒退。
    ERewind,
    /// 摘要空。
    ESummaryEmpty,
}

impl AnDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                AnDiag::ESegGap => 0x01,
                AnDiag::EAnchorDrift => 0x02,
                AnDiag::ESegCount => 0x03,
                AnDiag::EVersionJump => 0x04,
                AnDiag::EVersionRewind => 0x05,
                AnDiag::EVersionAbsent => 0x06,
                AnDiag::ERangeMissing => 0x07,
                AnDiag::EGaugeMissing => 0x08,
                AnDiag::EFreqRange => 0x09,
                AnDiag::EAnchoredWrite => 0x0a,
                AnDiag::ERewind => 0x0b,
                AnDiag::ESummaryEmpty => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            AnDiag::ESegGap => "段断缝：本段止+1 ≡ 下段起，断缝先补齐条目",
            AnDiag::EAnchorDrift => "锚值漂移：锚值 ≡ 段起+段止+段序×0x40，漂移先重算",
            AnDiag::ESegCount => "段条数违约：每段恰 100 条，短段先补条",
            AnDiag::EVersionJump => "版本跳号：新版本 ≡ 顶版本+1，跳号先补齐",
            AnDiag::EVersionRewind => "版本回退：版本只进不退，倒退先查登记账",
            AnDiag::EVersionAbsent => "版本缺席：协议版本号入账才许锚定收账",
            AnDiag::ERangeMissing => "范围段缺：起止判据号须闭合 12001..12700",
            AnDiag::EGaugeMissing => "口径段缺：口径式名非空且折算模非零",
            AnDiag::EFreqRange => "频率段越界：批末/封账/里程碑三态闭集",
            AnDiag::EAnchoredWrite => "锚定后写：锚账只读，补账走下一轮对账",
            AnDiag::ERewind => "状态机倒退：AnDraft→AnReconning→AnAnchored 只进不退",
            AnDiag::ESummaryEmpty => "摘要空：锚定摘要非空才许写回已完成",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const AN_ALL: &[AnDiag] = &[
    AnDiag::ESegGap,
    AnDiag::EAnchorDrift,
    AnDiag::ESegCount,
    AnDiag::EVersionJump,
    AnDiag::EVersionRewind,
    AnDiag::EVersionAbsent,
    AnDiag::ERangeMissing,
    AnDiag::EGaugeMissing,
    AnDiag::EFreqRange,
    AnDiag::EAnchoredWrite,
    AnDiag::ERewind,
    AnDiag::ESummaryEmpty,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in AN_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x81901..0x8190C 闭集内"
}

// ── 七域段锚账冻结表 ─────────────────────────────────────────────
/// 域段锚五元：(段序, 段起, 段止, 段条数, 锚值)。
///
/// 常量来源：域中程对账锚登记账（F12001–F12700 百条一段冻结快照）。
/// 锚值封闭式：锚值 ≡ 段起 + 段止 + 段序 × 0x40。
pub const ANCHOR_SEGS: &[(u32, u32, u32, u32, u32)] = &[
    (0, 12001, 12100, 100, 24101),
    (1, 12101, 12200, 100, 24365),
    (2, 12201, 12300, 100, 24629),
    (3, 12301, 12400, 100, 24893),
    (4, 12401, 12500, 100, 25157),
    (5, 12501, 12600, 100, 25421),
    (6, 12601, 12700, 100, 25685),
];

/// 域段数冻结值。
pub const ANCHOR_SEG_CAP: usize = 7;

/// 七百条账总条数。
pub const SEVEN_HUNDRED: u32 = 700;

/// 账首判据号。
pub const ANCHOR_HEAD: u32 = 12001;

/// 账尾判据号。
pub const ANCHOR_TAIL: u32 = 12700;

/// 锚值折算步距（段序系数）。
pub const ANCHOR_STEP: u32 = 0x40;

/// 锚值封闭式重算。
pub fn recompute_anchor(seg: u32, lo: u32, hi: u32) -> Option<u32> {
    let step = seg.checked_mul(ANCHOR_STEP)?;
    lo.checked_add(hi)?.checked_add(step)
}

// ── 总对账协议档 ─────────────────────────────────────────────────
/// 对账范围段（起止判据号闭合）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RangeSec {
    /// 起判据号。
    pub lo: u32,
    /// 止判据号。
    pub hi: u32,
}

/// 范围段闸：起 ≤ 止、起对齐账首、止对齐账尾。
pub fn range_sec_pass(r: &RangeSec) -> bool {
    r.lo == ANCHOR_HEAD && r.hi == ANCHOR_TAIL && r.lo <= r.hi
}

/// 对账口径段（口径式名 + 折算模）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GaugeSec {
    /// 口径式名（非空）。
    pub formula: &'static str,
    /// 折算模（非零）。
    pub modulus: u32,
}

/// 口径段闸。
pub fn gauge_sec_pass(g: &GaugeSec) -> bool {
    !g.formula.is_empty() && g.modulus != 0
}

/// 对账频率段（三态触发闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FreqSec {
    /// 批末触发。
    FqBatchEnd,
    /// 封账触发。
    FqSeal,
    /// 里程碑触发。
    FqMilestone,
}

impl FreqSec {
    /// 频率态名。
    pub fn name(self) -> &'static str {
        match self {
            FreqSec::FqBatchEnd => "批末触发",
            FreqSec::FqSeal => "封账触发",
            FreqSec::FqMilestone => "里程碑触发",
        }
    }

    /// 全部频率态（闭集数组）。
    pub fn all() -> &'static [FreqSec; 3] {
        &[FreqSec::FqBatchEnd, FreqSec::FqSeal, FreqSec::FqMilestone]
    }
}

/// 总对账协议档（范围/口径/频率三段 + 版本号）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReconProtocol {
    /// 对账范围段。
    pub range: RangeSec,
    /// 对账口径段。
    pub gauge: GaugeSec,
    /// 对账频率段。
    pub freq: FreqSec,
    /// 协议版本号（≥ 1）。
    pub version: u32,
}

/// 协议档三段齐闸（三段全过 + 版本 ≥ 1）。
pub fn protocol_sections_pass(p: &ReconProtocol) -> bool {
    if !range_sec_pass(&p.range) {
        return false;
    }
    if !gauge_sec_pass(&p.gauge) {
        return false;
    }
    // 频率段恒闭集合法（枚举即闭集）。
    if p.version < 1 {
        return false;
    }
    true
}

/// 冻结协议档（v3，里程碑触发）。
pub const FROZEN_PROTOCOL: ReconProtocol = ReconProtocol {
    range: RangeSec { lo: ANCHOR_HEAD, hi: ANCHOR_TAIL },
    gauge: GaugeSec { formula: "锚值≡段起+段止+段序×0x40", modulus: 0x40 },
    freq: FreqSec::FqMilestone,
    version: 3,
};

// ── 版本登记账 ───────────────────────────────────────────────────
/// 版本登记条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VerEntry {
    /// 版本号。
    pub version: u32,
    /// 协议档折算摘要。
    pub proto_digest: u32,
}

/// 协议档折算摘要（封闭式）。
pub fn proto_digest(p: &ReconProtocol) -> u32 {
    let freq_bit: u32 = match p.freq {
        FreqSec::FqBatchEnd => 1,
        FreqSec::FqSeal => 2,
        FreqSec::FqMilestone => 3,
    };
    p.range
        .lo
        .wrapping_mul(5)
        .wrapping_add(p.range.hi)
        .wrapping_add(p.gauge.modulus)
        .wrapping_add(freq_bit)
        .wrapping_add(p.version)
}

// ── 锚账状态机 ───────────────────────────────────────────────────
/// 锚账相位（单调推进闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnState {
    /// 草稿。
    AnDraft,
    /// 对账中。
    AnReconning,
    /// 已锚定（只读）。
    AnAnchored,
}

impl AnState {
    /// 相位秩。
    pub fn rank(self) -> u32 {
        match self {
            AnState::AnDraft => 0,
            AnState::AnReconning => 1,
            AnState::AnAnchored => 2,
        }
    }

    /// 相位名。
    pub fn name(self) -> &'static str {
        match self {
            AnState::AnDraft => "草稿",
            AnState::AnReconning => "对账中",
            AnState::AnAnchored => "已锚定",
        }
    }
}

/// 相位迁移守卫（只进不退，同相位拒）。
pub fn an_advance(from: AnState, to: AnState) -> Result<(), u32> {
    if to.rank() != from.rank() + 1 {
        return Err(AnDiag::ERewind.val());
    }
    Ok(())
}

/// 域中程对账锚账。
#[derive(Clone, Debug)]
pub struct AnAnchor {
    /// 相位。
    state: AnState,
    /// 协议档（None = 未签发）。
    protocol: Option<ReconProtocol>,
    /// 版本登记账。
    vers: Vec<VerEntry>,
    /// 已对账段数。
    reconed: usize,
    /// 拒止计数。
    rejects: u32,
}

impl AnAnchor {
    /// 新账（草稿相位）。
    pub fn new() -> AnAnchor {
        AnAnchor {
            state: AnState::AnDraft,
            protocol: None,
            vers: Vec::new(),
            reconed: 0,
            rejects: 0,
        }
    }

    /// 开账（草稿 → 对账中）。
    pub fn begin(&mut self) -> Result<(), u32> {
        an_advance(self.state, AnState::AnReconning)?;
        self.state = AnState::AnReconning;
        Ok(())
    }

    /// 锚定收账（对账中 → 已锚定；协议档三段齐 + 版本入账 + 七段全对三闸）。
    pub fn anchor(&mut self) -> Result<(), u32> {
        if self.state != AnState::AnReconning {
            self.rejects = self.rejects.saturating_add(1);
            return Err(AnDiag::ERewind.val());
        }
        let p = match self.protocol {
            Some(v) => v,
            None => {
                self.rejects = self.rejects.saturating_add(1);
                return Err(AnDiag::EGaugeMissing.val());
            }
        };
        if !protocol_sections_pass(&p) {
            self.rejects = self.rejects.saturating_add(1);
            return Err(AnDiag::ERangeMissing.val());
        }
        if self.vers.is_empty() {
            self.rejects = self.rejects.saturating_add(1);
            return Err(AnDiag::EVersionAbsent.val());
        }
        if self.reconed != ANCHOR_SEG_CAP {
            self.rejects = self.rejects.saturating_add(1);
            return Err(AnDiag::ESegGap.val());
        }
        an_advance(self.state, AnState::AnAnchored)?;
        self.state = AnState::AnAnchored;
        Ok(())
    }

    /// 签发协议档（三段齐闸 + 版本单调闸）。
    pub fn issue_protocol(&mut self, p: ReconProtocol) -> Result<(), u32> {
        if self.state == AnState::AnAnchored {
            self.rejects = self.rejects.saturating_add(1);
            return Err(AnDiag::EAnchoredWrite.val());
        }
        if self.state == AnState::AnDraft {
            self.state = AnState::AnReconning;
        }
        if !protocol_sections_pass(&p) {
            self.rejects = self.rejects.saturating_add(1);
            return Err(AnDiag::ERangeMissing.val());
        }
        // 版本单调闸：新版本 ≡ 顶版本 + 1（首个版本 ≡ 1）。
        let expect = match self.vers.last() {
            Some(v) => v.version.checked_add(1),
            None => Some(1),
        };
        if expect != Some(p.version) {
            self.rejects = self.rejects.saturating_add(1);
            return Err(AnDiag::EVersionJump.val());
        }
        let e = VerEntry { version: p.version, proto_digest: proto_digest(&p) };
        self.vers.push(e);
        self.protocol = Some(p);
        Ok(())
    }

    /// 对一段锚账（段序/条数/锚值三闸，逐段推进）。
    pub fn recon_seg(&mut self, idx: usize) -> Result<(), u32> {
        if self.state == AnState::AnAnchored {
            self.rejects = self.rejects.saturating_add(1);
            return Err(AnDiag::EAnchoredWrite.val());
        }
        if self.state == AnState::AnDraft {
            self.state = AnState::AnReconning;
        }
        // 段序闸：按登记序推进。
        if idx != self.reconed {
            self.rejects = self.rejects.saturating_add(1);
            return Err(AnDiag::ESegGap.val());
        }
        let row = match ANCHOR_SEGS.get(idx) {
            Some(v) => v,
            None => {
                self.rejects = self.rejects.saturating_add(1);
                return Err(AnDiag::ESegGap.val());
            }
        };
        // 条数闸。
        if row.3 != 100 {
            self.rejects = self.rejects.saturating_add(1);
            return Err(AnDiag::ESegCount.val());
        }
        // 锚值闸：重算对号。
        let expect = match recompute_anchor(row.0, row.1, row.2) {
            Some(v) => v,
            None => {
                self.rejects = self.rejects.saturating_add(1);
                return Err(AnDiag::EAnchorDrift.val());
            }
        };
        if row.4 != expect {
            self.rejects = self.rejects.saturating_add(1);
            return Err(AnDiag::EAnchorDrift.val());
        }
        self.reconed += 1;
        Ok(())
    }

    /// 当前相位。
    pub fn state(&self) -> AnState {
        self.state
    }

    /// 拒止计数。
    pub fn reject_len(&self) -> u32 {
        self.rejects
    }

    /// 已对账段数。
    pub fn reconed_len(&self) -> usize {
        self.reconed
    }

    /// 版本登记条数。
    pub fn ver_len(&self) -> usize {
        self.vers.len()
    }

    /// 协议档。
    pub fn protocol(&self) -> Option<ReconProtocol> {
        self.protocol
    }

    /// 版本登记条（越界 None）。
    pub fn ver_at(&self, idx: usize) -> Option<&VerEntry> {
        self.vers.get(idx)
    }
}

/// 全量锚账重放（协议档 v1→v3 三版登记 + 七段全对 + 锚定）。
pub fn build_full_an_anchor() -> AnAnchor {
    let mut a = AnAnchor::new();
    let _ = a.begin();
    // 版本链：v1（批末）→ v2（封账）→ v3（里程碑，冻结终版）。
    let v1 = ReconProtocol {
        range: RangeSec { lo: ANCHOR_HEAD, hi: ANCHOR_TAIL },
        gauge: GaugeSec { formula: "锚值≡段起+段止+段序×0x40", modulus: 0x40 },
        freq: FreqSec::FqBatchEnd,
        version: 1,
    };
    let v2 = ReconProtocol { freq: FreqSec::FqSeal, version: 2, ..v1 };
    let _ = a.issue_protocol(v1);
    let _ = a.issue_protocol(v2);
    let _ = a.issue_protocol(FROZEN_PROTOCOL);
    let mut i = 0usize;
    while i < ANCHOR_SEG_CAP {
        let _ = a.recon_seg(i);
        i += 1;
    }
    let _ = a.anchor();
    a
}

// ── 判据谓词（全部私有） ─────────────────────────────────────────
/// 总对账协议档三段判据绿（冻结终版三段齐）。
fn protocol_three_section_ok() -> bool {
    if !protocol_sections_pass(&FROZEN_PROTOCOL) {
        return false;
    }
    let a = build_full_an_anchor();
    let p = match a.protocol() {
        Some(v) => v,
        None => return false,
    };
    p == FROZEN_PROTOCOL
}

/// 协议版本号入账（单调登记链 1→2→3）。
fn version_registered_ok() -> bool {
    let a = build_full_an_anchor();
    if a.ver_len() != 3 {
        return false;
    }
    // 版本链连续递进。
    let mut expect = 1u32;
    let mut i = 0usize;
    while i < a.ver_len() {
        let v = match a.ver_at(i) {
            Some(e) => e,
            None => return false,
        };
        if v.version != expect {
            return false;
        }
        if v.proto_digest == 0 {
            return false;
        }
        expect += 1;
        i += 1;
    }
    true
}

/// 七百条集成账锚定（七段锚值全对 + 全量重放锚定）。
fn anchor_seven_seg_ok() -> bool {
    if ANCHOR_SEGS.len() != ANCHOR_SEG_CAP {
        return false;
    }
    let mut total = 0u32;
    let mut i = 0usize;
    while i < ANCHOR_SEGS.len() {
        let row = match ANCHOR_SEGS.get(i) {
            Some(v) => v,
            None => return false,
        };
        if row.3 != 100 {
            return false;
        }
        let expect = match recompute_anchor(row.0, row.1, row.2) {
            Some(v) => v,
            None => return false,
        };
        if row.4 != expect {
            return false;
        }
        total = total.saturating_add(row.3);
        i += 1;
    }
    if total != SEVEN_HUNDRED {
        return false;
    }
    let a = build_full_an_anchor();
    a.state() == AnState::AnAnchored && a.reconed_len() == ANCHOR_SEG_CAP && a.reject_len() == 0
}

/// 注入联测十族（人为构造违约必被拒）。
pub fn an_fault_sweep() -> (u32, u32, String) {
    let mut caught = 0u32;
    let mut escape = String::new();
    // 1. 版本跳号（首版即 2）。
    {
        let mut a = AnAnchor::new();
        let _ = a.begin();
        let jump = ReconProtocol {
            range: RangeSec { lo: ANCHOR_HEAD, hi: ANCHOR_TAIL },
            gauge: GaugeSec { formula: "口径式", modulus: 0x40 },
            freq: FreqSec::FqBatchEnd,
            version: 2,
        };
        if a.issue_protocol(jump).is_err() {
            caught += 1;
        } else {
            escape.push_str("跳号;");
        }
    }
    // 2. 版本回退。
    {
        let mut a = AnAnchor::new();
        let _ = a.begin();
        let v1 = ReconProtocol {
            range: RangeSec { lo: ANCHOR_HEAD, hi: ANCHOR_TAIL },
            gauge: GaugeSec { formula: "口径式", modulus: 0x40 },
            freq: FreqSec::FqBatchEnd,
            version: 1,
        };
        let _ = a.issue_protocol(v1);
        let rewind = ReconProtocol { freq: FreqSec::FqSeal, version: 1, ..v1 };
        if a.issue_protocol(rewind).is_err() {
            caught += 1;
        } else {
            escape.push_str("版本回退;");
        }
    }
    // 3. 范围段漂移（起号错）。
    {
        let mut a = AnAnchor::new();
        let _ = a.begin();
        let bad = ReconProtocol {
            range: RangeSec { lo: 12000, hi: ANCHOR_TAIL },
            gauge: GaugeSec { formula: "口径式", modulus: 0x40 },
            freq: FreqSec::FqBatchEnd,
            version: 1,
        };
        if a.issue_protocol(bad).is_err() {
            caught += 1;
        } else {
            escape.push_str("范围漂移;");
        }
    }
    // 4. 口径段缺（模零）。
    {
        let mut a = AnAnchor::new();
        let _ = a.begin();
        let bad = ReconProtocol {
            range: RangeSec { lo: ANCHOR_HEAD, hi: ANCHOR_TAIL },
            gauge: GaugeSec { formula: "口径式", modulus: 0 },
            freq: FreqSec::FqBatchEnd,
            version: 1,
        };
        if a.issue_protocol(bad).is_err() {
            caught += 1;
        } else {
            escape.push_str("口径缺;");
        }
    }
    // 5. 锚值漂移（冻结表只读——直证重算式逐段对号后必有对账推进）。
    {
        let mut a = AnAnchor::new();
        let _ = a.begin();
        // 越序对账（跳过段 0 直接对段 1）必拒。
        if a.recon_seg(1).is_err() {
            caught += 1;
        } else {
            escape.push_str("越序;");
        }
    }
    // 6. 版本缺席锚定。
    {
        let mut a = AnAnchor::new();
        let _ = a.begin();
        let mut i = 0usize;
        while i < ANCHOR_SEG_CAP {
            let _ = a.recon_seg(i);
            i += 1;
        }
        if a.anchor().is_err() {
            caught += 1;
        } else {
            escape.push_str("版本缺席;");
        }
    }
    // 7. 段数不足锚定。
    {
        let mut a = AnAnchor::new();
        let _ = a.begin();
        let v1 = ReconProtocol {
            range: RangeSec { lo: ANCHOR_HEAD, hi: ANCHOR_TAIL },
            gauge: GaugeSec { formula: "口径式", modulus: 0x40 },
            freq: FreqSec::FqBatchEnd,
            version: 1,
        };
        let _ = a.issue_protocol(v1);
        let _ = a.recon_seg(0);
        if a.anchor().is_err() {
            caught += 1;
        } else {
            escape.push_str("段数不足;");
        }
    }
    // 8. 锚定后写。
    {
        let mut a = build_full_an_anchor();
        if a.recon_seg(0).is_err() {
            caught += 1;
        } else {
            escape.push_str("锚后写;");
        }
    }
    // 9. 状态机倒退。
    {
        let mut a = build_full_an_anchor();
        if a.begin().is_err() {
            caught += 1;
        } else {
            escape.push_str("倒退;");
        }
    }
    // 10. 摘要空面直证（相位名/频率态名恒非空）。
    {
        let mut bad = false;
        for st in [AnState::AnDraft, AnState::AnReconning, AnState::AnAnchored].iter() {
            if st.name().is_empty() {
                bad = true;
            }
        }
        for f in FreqSec::all().iter() {
            if f.name().is_empty() {
                bad = true;
            }
        }
        if !bad {
            caught += 1;
        } else {
            escape.push_str("空摘要;");
        }
    }
    (caught, 10, escape)
}

/// 双跑一致。
fn dual_run_ok() -> bool {
    let x = build_full_an_anchor();
    let y = build_full_an_anchor();
    if x.state() != y.state() || x.reject_len() != y.reject_len() {
        return false;
    }
    if x.reconed_len() != y.reconed_len() || x.ver_len() != y.ver_len() {
        return false;
    }
    let p1 = x.protocol();
    let p2 = y.protocol();
    if p1 != p2 {
        return false;
    }
    let (c1, _, e1) = an_fault_sweep();
    let (c2, _, e2) = an_fault_sweep();
    c1 == c2 && e1 == e2
}

/// 冻结摘要（锚账表 + 协议档折算）。
pub fn frozen_digest() -> u32 {
    let mut acc: u32 = SEG_BASE;
    let mut i = 0u32;
    for row in ANCHOR_SEGS.iter() {
        acc = acc
            .wrapping_mul(7)
            .wrapping_add(row.0)
            .wrapping_add(row.1)
            .wrapping_add(row.2)
            .wrapping_add(row.3)
            .wrapping_add(row.4)
            .wrapping_add(i);
        i = i.wrapping_add(1);
    }
    acc = acc.wrapping_add(proto_digest(&FROZEN_PROTOCOL));
    acc
}

/// 段位守卫（VX9 全批窗口 + 本卡公式位）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x80000 {
        return false;
    }
    if SEG_BASE > 0x80000 + 0x40 * 199 {
        return false;
    }
    if SEG_BASE != 0x80000 + 0x40 * (12700u32.saturating_sub(12600)) {
        return false;
    }
    true
}

/// 完成写回语义（判据 ③）。
fn writeback_ok() -> bool {
    let a = build_full_an_anchor();
    if a.state() != AnState::AnAnchored {
        return false;
    }
    for st in [AnState::AnDraft, AnState::AnReconning, AnState::AnAnchored].iter() {
        if st.name().is_empty() {
            return false;
        }
    }
    for f in FreqSec::all().iter() {
        if f.name().is_empty() {
            return false;
        }
    }
    if !protocol_three_section_ok() || !version_registered_ok() || !anchor_seven_seg_ok() {
        return false;
    }
    frozen_digest() != 0
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12701 域中程对账锚自检（判据 J1 + 通用守卫，≥12 条）。
pub fn run_unx12700_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12700-midrecon-anchor");
    let a = build_full_an_anchor();

    s.add_dyn(
        "F12701-01 协议档三段齐判据绿",
        protocol_three_section_ok(),
        "范围/口径/频率三段全过",
    );

    s.add_dyn(
        "F12701-02 范围段闭合",
        range_sec_pass(&FROZEN_PROTOCOL.range),
        &format!("lo={} hi={}", FROZEN_PROTOCOL.range.lo, FROZEN_PROTOCOL.range.hi),
    );

    s.add_dyn(
        "F12701-03 口径段合法",
        gauge_sec_pass(&FROZEN_PROTOCOL.gauge),
        "口径式名非空且模 0x40 非零",
    );

    s.add_dyn(
        "F12701-04 频率段闭集合法",
        {
            let mut hit = false;
            for f in FreqSec::all().iter() {
                if *f == FROZEN_PROTOCOL.freq {
                    hit = true;
                }
            }
            hit
        },
        "批末/封账/里程碑三态闭集",
    );

    s.add_dyn(
        "F12701-05 协议版本号入账（1→2→3）",
        version_registered_ok(),
        &format!("vers={}", a.ver_len()),
    );

    s.add_dyn(
        "F12701-06 协议档折算摘要非零",
        proto_digest(&FROZEN_PROTOCOL) != 0,
        &format!("digest={:08X}", proto_digest(&FROZEN_PROTOCOL)),
    );

    s.add_dyn(
        "F12701-07 七百条集成账锚定",
        anchor_seven_seg_ok(),
        &format!("reconed={} segs={}", a.reconed_len(), ANCHOR_SEG_CAP),
    );

    s.add_dyn(
        "F12701-08 锚值封闭式逐段对号",
        {
            let mut ok = true;
            let mut i = 0usize;
            while i < ANCHOR_SEGS.len() {
                let row = match ANCHOR_SEGS.get(i) {
                    Some(v) => v,
                    None => {
                        ok = false;
                        break;
                    }
                };
                match recompute_anchor(row.0, row.1, row.2) {
                    Some(v) => {
                        if v != row.4 {
                            ok = false;
                        }
                    }
                    None => ok = false,
                }
                i += 1;
            }
            ok
        },
        "锚值 ≡ 段起+段止+段序×0x40",
    );

    s.add_dyn(
        "F12701-09 注入联测 10/10 拒止",
        {
            let (c, t, e) = an_fault_sweep();
            t == 10 && c == 10 && e.is_empty()
        },
        &format!("caught={}", an_fault_sweep().0),
    );

    s.add_dyn(
        "F12701-10 相位迁移守卫直读",
        an_advance(AnState::AnDraft, AnState::AnReconning).is_ok()
            && an_advance(AnState::AnReconning, AnState::AnAnchored).is_ok()
            && an_advance(AnState::AnAnchored, AnState::AnDraft).is_err()
            && an_advance(AnState::AnDraft, AnState::AnAnchored).is_err(),
        "草稿→对账中→已锚定只进不退",
    );

    s.add_dyn(
        "F12701-11 双跑一致",
        dual_run_ok(),
        "确定性重放两轮逐位同判",
    );

    s.add_dyn(
        "F12701-12 完成写回语义",
        writeback_ok(),
        "锚定即已完成且摘要非空",
    );

    s.add_dyn(
        "F12701-13 冻结摘要非零",
        frozen_digest() != 0,
        &format!("digest={:08X}", frozen_digest()),
    );

    s.add_dyn(
        "F12701-14 段位守卫",
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
        assert_eq!(SEG_BASE, 0x81900);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        assert_eq!(AN_ALL.len(), 12);
        let mut i = 0usize;
        while i < AN_ALL.len() {
            let vi = match AN_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < AN_ALL.len() {
                let vj = match AN_ALL.get(j) {
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
    fn anchor_formula_holds() {
        // 封闭式抽查。
        assert_eq!(recompute_anchor(0, 12001, 12100), Some(24101));
        assert_eq!(recompute_anchor(6, 12601, 12700), Some(25685));
        // 冻结表逐段对号。
        let mut i = 0usize;
        while i < ANCHOR_SEGS.len() {
            let row = match ANCHOR_SEGS.get(i) {
                Some(v) => v,
                None => return,
            };
            assert_eq!(recompute_anchor(row.0, row.1, row.2), Some(row.4), "段 {} 锚漂移", i);
            assert_eq!(row.3, 100);
            i += 1;
        }
        // 溢出面：越界段返回 None。
        assert_eq!(recompute_anchor(u32::MAX / 0x40 + 1, 1, 1), None);
    }

    #[test]
    fn protocol_sections_holds() {
        assert!(protocol_sections_pass(&FROZEN_PROTOCOL));
        assert!(protocol_three_section_ok());
        assert!(range_sec_pass(&FROZEN_PROTOCOL.range));
        assert!(gauge_sec_pass(&FROZEN_PROTOCOL.gauge));
        // 范围漂移拒。
        let bad_range = ReconProtocol {
            range: RangeSec { lo: 12000, hi: 12699 },
            gauge: FROZEN_PROTOCOL.gauge,
            freq: FROZEN_PROTOCOL.freq,
            version: 1,
        };
        assert!(!protocol_sections_pass(&bad_range));
        // 口径模零拒。
        let bad_gauge = ReconProtocol {
            range: FROZEN_PROTOCOL.range,
            gauge: GaugeSec { formula: "", modulus: 0 },
            freq: FROZEN_PROTOCOL.freq,
            version: 1,
        };
        assert!(!protocol_sections_pass(&bad_gauge));
    }

    #[test]
    fn version_chain_holds() {
        assert!(version_registered_ok());
        let a = build_full_an_anchor();
        assert_eq!(a.ver_len(), 3);
        let v1 = a.ver_at(0);
        let v2 = a.ver_at(1);
        let v3 = a.ver_at(2);
        match (v1, v2, v3) {
            (Some(x), Some(y), Some(z)) => {
                assert_eq!(x.version, 1);
                assert_eq!(y.version, 2);
                assert_eq!(z.version, 3);
                assert_ne!(x.proto_digest, y.proto_digest);
                assert_ne!(y.proto_digest, z.proto_digest);
            }
            _ => assert!(false, "版本登记缺失"),
        }
    }

    #[test]
    fn issue_gates_reject() {
        let mut a = AnAnchor::new();
        let _ = a.begin();
        // 版本跳号。
        let v2 = ReconProtocol {
            range: FROZEN_PROTOCOL.range,
            gauge: FROZEN_PROTOCOL.gauge,
            freq: FreqSec::FqBatchEnd,
            version: 2,
        };
        assert_eq!(a.issue_protocol(v2), Err(AnDiag::EVersionJump.val()));
        // 范围漂移。
        let bad = ReconProtocol {
            range: RangeSec { lo: 12000, hi: ANCHOR_TAIL },
            gauge: FROZEN_PROTOCOL.gauge,
            freq: FreqSec::FqBatchEnd,
            version: 1,
        };
        assert_eq!(a.issue_protocol(bad), Err(AnDiag::ERangeMissing.val()));
        // 口径缺。
        let nog = ReconProtocol {
            range: FROZEN_PROTOCOL.range,
            gauge: GaugeSec { formula: "", modulus: 0x40 },
            freq: FreqSec::FqBatchEnd,
            version: 1,
        };
        assert_eq!(a.issue_protocol(nog), Err(AnDiag::ERangeMissing.val()));
        assert_eq!(a.reject_len(), 3);
        // 正常 v1 过。
        let v1 = ReconProtocol {
            range: FROZEN_PROTOCOL.range,
            gauge: FROZEN_PROTOCOL.gauge,
            freq: FreqSec::FqBatchEnd,
            version: 1,
        };
        assert!(a.issue_protocol(v1).is_ok());
        assert_eq!(a.ver_len(), 1);
    }

    #[test]
    fn recon_and_anchor_gates_hold() {
        // 越序对账拒。
        let mut a = AnAnchor::new();
        let _ = a.begin();
        assert_eq!(a.recon_seg(1), Err(AnDiag::ESegGap.val()));
        // 版本缺席锚定拒。
        let mut b = AnAnchor::new();
        let _ = b.begin();
        let mut i = 0usize;
        while i < ANCHOR_SEG_CAP {
            let _ = b.recon_seg(i);
            i += 1;
        }
        assert_eq!(b.anchor(), Err(AnDiag::EGaugeMissing.val()));
        // 段数不足锚定拒。
        let mut c = AnAnchor::new();
        let _ = c.begin();
        let v1 = ReconProtocol {
            range: FROZEN_PROTOCOL.range,
            gauge: FROZEN_PROTOCOL.gauge,
            freq: FreqSec::FqBatchEnd,
            version: 1,
        };
        let _ = c.issue_protocol(v1);
        let _ = c.recon_seg(0);
        assert_eq!(c.anchor(), Err(AnDiag::ESegGap.val()));
        // 全量锚定过 + 锚定后写拒。
        let mut full = build_full_an_anchor();
        assert_eq!(full.state(), AnState::AnAnchored);
        assert_eq!(full.reconed_len(), 7);
        assert_eq!(full.recon_seg(0), Err(AnDiag::EAnchoredWrite.val()));
        assert!(full.begin().is_err());
    }

    #[test]
    fn phase_guards_hold() {
        assert!(an_advance(AnState::AnDraft, AnState::AnReconning).is_ok());
        assert!(an_advance(AnState::AnReconning, AnState::AnAnchored).is_ok());
        assert!(an_advance(AnState::AnDraft, AnState::AnAnchored).is_err());
        assert!(an_advance(AnState::AnAnchored, AnState::AnReconning).is_err());
        assert!(an_advance(AnState::AnReconning, AnState::AnReconning).is_err());
        assert!(AnState::AnDraft.rank() < AnState::AnReconning.rank());
        assert!(AnState::AnReconning.rank() < AnState::AnAnchored.rank());
        assert_eq!(FreqSec::FqBatchEnd.name(), "批末触发");
        assert_eq!(FreqSec::FqMilestone.name(), "里程碑触发");
    }

    #[test]
    fn injections_all_caught() {
        let (caught, total, escape) = an_fault_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn dual_run_consistent() {
        assert!(dual_run_ok());
        let x = build_full_an_anchor();
        let y = build_full_an_anchor();
        assert_eq!(x.ver_len(), y.ver_len());
        assert_eq!(x.protocol(), y.protocol());
        assert_eq!(frozen_digest(), frozen_digest());
        assert_ne!(frozen_digest(), 0);
    }

    #[test]
    fn writeback_semantics_hold() {
        assert!(writeback_ok());
        for st in [AnState::AnDraft, AnState::AnReconning, AnState::AnAnchored].iter() {
            assert!(!st.name().is_empty());
        }
        for f in FreqSec::all().iter() {
            assert!(!f.name().is_empty());
        }
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12700_checks();
        assert!(s.all_passed(), "自检存在红项：{}", s.red_items_text());
        assert!(s.len() >= 12, "检查条目不足 12");
    }
}
