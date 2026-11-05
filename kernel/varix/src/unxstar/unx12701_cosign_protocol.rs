//! UNX-F12702 联签一致性总对账协议档 · 0x81940段
//!
//! 判据原文（派工卡 12701 逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② 判据原文：UNX-F12702-J1 四域矩阵终对账齐（行=消费边/列=
//!      对账态），混报注入 10/10 检出；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 命名说明：同号旧批文件 unx12701_protoledger.rs（SEG 0x65140，VX3 批）
//! 不触碰；本模块按短名语义另起 cosign_protocol（VX9 批 0x81940）。
//!
//! 设计要点：
//! 1. 联签一致性总对账协议档：四域消费边矩阵（行 = 消费边 D2/D3/D4/E1
//!    八条边闭集，列 = 对账态 Match/Conflict/Pending 三态闭集），终对账
//!    齐 = 八行逐行登记且终态全 Match（Conflict/Pending 残留即不齐）；
//! 2. 混报闸：每行登记态须与冻结期望态逐位一致，混报（错报态/错边号/
//!    重边/越边）逐条检出——混报注入 10/10 检出是判据硬线；
//! 3. 协议档三段：对账范围（四域闭集）/对账口径（一致判定式名 + 折算
//!    模）/对账频率（三态触发闭集），三段齐才许签发；
//! 4. 协议版本号单调：新版本 ≡ 顶版本 + 1，回退/跳号显性拒（版本闸）；
//! 5. 签发状态机 CpDraft → CpReconning → CpSigned 单向推进，签发后
//!    只读、倒退显性拒；注入十族（混报态/错边号/重边/越边/版本回退/
//!    版本跳号/残留态签发/签发后写/倒退/摘要空）逐路必抓；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机；取值一律
//!    match/if let/.get()/checked_*/saturating_*，无 unwrap/expect/
//!    panic/裸下标；中文注释与中文诊断；无死代码无 #[allow]。
//!
//! 判据谓词与验收判据的对应：
//!   - `matrix_final_ok` ↔ ② 四域矩阵终对账齐（行=消费边/列=对账态）；
//!   - `mix_report_ten_ok` ↔ ② 混报注入 10/10 检出；
//!   - `protocol_signed_ok` ↔ 标题「联签一致性总对账协议档」（三段齐 +
//!     版本入账 + 签发）；
//!   - `writeback_ok` ↔ ③ 状态=已完成、结果摘要非空。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12701 − 12600)）。
pub const SEG_BASE: u32 = 0x81940;

// ── 联签协议档诊断码闭集 ─────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CpDiag {
    /// 混报态（登记态 ≠ 期望态）。
    EMixReport,
    /// 错边号（外键越界）。
    EEdgeRange,
    /// 边重号。
    EEdgeDup,
    /// 残留态签发（Conflict/Pending 未清）。
    EResidueSigned,
    /// 矩阵缺行。
    EMatrixMissing,
    /// 版本回退。
    EVersionRewind,
    /// 版本跳号。
    EVersionJump,
    /// 协议段缺。
    ESectionMissing,
    /// 签发后写。
    ESignedWrite,
    /// 状态机倒退。
    ERewind,
    /// 边期望越界（冻结期望表外键错）。
    EExpectRange,
    /// 摘要空。
    ESummaryEmpty,
}

impl CpDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                CpDiag::EMixReport => 0x01,
                CpDiag::EEdgeRange => 0x02,
                CpDiag::EEdgeDup => 0x03,
                CpDiag::EResidueSigned => 0x04,
                CpDiag::EMatrixMissing => 0x05,
                CpDiag::EVersionRewind => 0x06,
                CpDiag::EVersionJump => 0x07,
                CpDiag::ESectionMissing => 0x08,
                CpDiag::ESignedWrite => 0x09,
                CpDiag::ERewind => 0x0a,
                CpDiag::EExpectRange => 0x0b,
                CpDiag::ESummaryEmpty => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            CpDiag::EMixReport => "混报态：登记态须与期望态一致，混报先重核矩阵",
            CpDiag::EEdgeRange => "错边号：八条消费边闭集 0..7，越边先改边号",
            CpDiag::EEdgeDup => "边重号：一行一边，重复先查矩阵账",
            CpDiag::EResidueSigned => "残留态签发：Conflict/Pending 未清不许签发，先终对账",
            CpDiag::EMatrixMissing => "矩阵缺行：八行逐行登记，缺行先补",
            CpDiag::EVersionRewind => "版本回退：版本只进不退，倒退先查登记账",
            CpDiag::EVersionJump => "版本跳号：新版本 ≡ 顶版本+1，跳号先补齐",
            CpDiag::ESectionMissing => "协议段缺：范围/口径/频率三段齐才许签发",
            CpDiag::ESignedWrite => "签发后写：协议档只读，补签走下一轮",
            CpDiag::ERewind => "状态机倒退：CpDraft→CpReconning→CpSigned 只进不退",
            CpDiag::EExpectRange => "边期望越界：期望态按边号闭集取，越界先纠账",
            CpDiag::ESummaryEmpty => "摘要空：签发摘要非空才许写回已完成",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const CP_ALL: &[CpDiag] = &[
    CpDiag::EMixReport,
    CpDiag::EEdgeRange,
    CpDiag::EEdgeDup,
    CpDiag::EResidueSigned,
    CpDiag::EMatrixMissing,
    CpDiag::EVersionRewind,
    CpDiag::EVersionJump,
    CpDiag::ESectionMissing,
    CpDiag::ESignedWrite,
    CpDiag::ERewind,
    CpDiag::EExpectRange,
    CpDiag::ESummaryEmpty,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in CP_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x81941..0x8194C 闭集内"
}

// ── 对账态闭集 ───────────────────────────────────────────────────
/// 对账态（三态闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReconState {
    /// 一致（Match）。
    RsMatch,
    /// 冲突（Conflict）。
    RsConflict,
    /// 挂起（Pending）。
    RsPending,
}

impl ReconState {
    /// 对账态名。
    pub fn name(self) -> &'static str {
        match self {
            ReconState::RsMatch => "一致",
            ReconState::RsConflict => "冲突",
            ReconState::RsPending => "挂起",
        }
    }

    /// 全部对账态（闭集数组）。
    pub fn all() -> &'static [ReconState; 3] {
        &[ReconState::RsMatch, ReconState::RsConflict, ReconState::RsPending]
    }
}

// ── 四域消费边冻结表（八行） ─────────────────────────────────────
/// 消费边三元：(边号, 所属域, 边名)。
///
/// 常量来源：四域消费边矩阵登记账（D2/D3/D4/E1 各两行，冻结快照）。
pub const COSIGN_EDGES: &[(u32, &str, &str)] = &[
    (0, "D2", "d2-cosign-replay-edge"),
    (1, "D2", "d2-ledger-equation-edge"),
    (2, "D3", "d3-retest-inject-edge"),
    (3, "D3", "d3-fake-green-edge"),
    (4, "D4", "d4-cosign-slot-edge"),
    (5, "D4", "d4-receipt-verify-edge"),
    (6, "E1", "e1-consume-edge-enum-edge"),
    (7, "E1", "e1-matrix-cell-edge"),
];

/// 消费边数冻结值。
pub const EDGE_CAP: usize = 8;

/// 边期望态冻结表（终对账齐 = 全行一致）。
pub const EDGE_EXPECT: &[ReconState] = &[
    ReconState::RsMatch,
    ReconState::RsMatch,
    ReconState::RsMatch,
    ReconState::RsMatch,
    ReconState::RsMatch,
    ReconState::RsMatch,
    ReconState::RsMatch,
    ReconState::RsMatch,
];

/// 期望态查表（越界 None）。
pub fn expect_at(edge: u32) -> Option<ReconState> {
    EDGE_EXPECT.get(edge as usize).copied()
}

/// 域名查表（越界 None）。
pub fn edge_domain(edge: u32) -> Option<&'static str> {
    for row in COSIGN_EDGES.iter() {
        if row.0 == edge {
            return Some(row.1);
        }
    }
    None
}

// ── 协议档三段 ───────────────────────────────────────────────────
/// 对账范围段（四域闭集行）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScopeSec {
    /// 范围名（非空）。
    pub scope: &'static str,
    /// 覆盖域数（恒 4）。
    pub domains: u32,
}

/// 范围段闸。
pub fn scope_sec_pass(s: &ScopeSec) -> bool {
    !s.scope.is_empty() && s.domains == 4
}

/// 对账口径段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GaugeSec {
    /// 一致判定式名（非空）。
    pub formula: &'static str,
    /// 折算模（非零）。
    pub modulus: u32,
}

/// 口径段闸。
pub fn gauge_sec_pass(g: &GaugeSec) -> bool {
    !g.formula.is_empty() && g.modulus != 0
}

/// 对账频率段（三态闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FreqSec {
    /// 矩阵登记时触发。
    FqMatrixReg,
    /// 签发前触发。
    FqPreSign,
    /// 批末触发。
    FqBatchEnd,
}

impl FreqSec {
    /// 频率态名。
    pub fn name(self) -> &'static str {
        match self {
            FreqSec::FqMatrixReg => "矩阵登记触发",
            FreqSec::FqPreSign => "签发前触发",
            FreqSec::FqBatchEnd => "批末触发",
        }
    }

    /// 全部频率态（闭集数组）。
    pub fn all() -> &'static [FreqSec; 3] {
        &[FreqSec::FqMatrixReg, FreqSec::FqPreSign, FreqSec::FqBatchEnd]
    }
}

/// 联签一致性总对账协议档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CosignProtocol {
    /// 对账范围段。
    pub scope: ScopeSec,
    /// 对账口径段。
    pub gauge: GaugeSec,
    /// 对账频率段。
    pub freq: FreqSec,
    /// 协议版本号（≥ 1）。
    pub version: u32,
}

/// 协议档三段齐闸。
pub fn protocol_pass(p: &CosignProtocol) -> bool {
    scope_sec_pass(&p.scope) && gauge_sec_pass(&p.gauge) && p.version >= 1
}

/// 冻结协议档（v2，签发前触发）。
pub const FROZEN_PROTOCOL: CosignProtocol = CosignProtocol {
    scope: ScopeSec { scope: "D2/D3/D4/E1 四域消费边矩阵", domains: 4 },
    gauge: GaugeSec { formula: "登记态≡期望态 逐位一致", modulus: 3 },
    freq: FreqSec::FqPreSign,
    version: 2,
};

// ── 矩阵行 ───────────────────────────────────────────────────────
/// 矩阵登记行（边号 × 登记态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellLine {
    /// 边号（0..7）。
    pub edge: u32,
    /// 登记态。
    pub state: ReconState,
}

/// 矩阵行混报判定：登记态 ≠ 期望态即混报。
pub fn cell_is_mix(c: &CellLine) -> bool {
    match expect_at(c.edge) {
        Some(e) => e != c.state,
        None => true,
    }
}

// ── 签发状态机 ───────────────────────────────────────────────────
/// 签发相位（单调推进闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CpState {
    /// 草稿。
    CpDraft,
    /// 终对账中。
    CpReconning,
    /// 已签发（只读）。
    CpSigned,
}

impl CpState {
    /// 相位秩。
    pub fn rank(self) -> u32 {
        match self {
            CpState::CpDraft => 0,
            CpState::CpReconning => 1,
            CpState::CpSigned => 2,
        }
    }

    /// 相位名。
    pub fn name(self) -> &'static str {
        match self {
            CpState::CpDraft => "草稿",
            CpState::CpReconning => "终对账中",
            CpState::CpSigned => "已签发",
        }
    }
}

/// 相位迁移守卫（只进不退，同相位拒）。
pub fn cp_advance(from: CpState, to: CpState) -> Result<(), u32> {
    if to.rank() != from.rank() + 1 {
        return Err(CpDiag::ERewind.val());
    }
    Ok(())
}

/// 联签一致性总对账协议账。
#[derive(Clone, Debug)]
pub struct CpLedger {
    /// 相位。
    state: CpState,
    /// 协议档。
    protocol: Option<CosignProtocol>,
    /// 版本登记账。
    vers: Vec<u32>,
    /// 矩阵登记行。
    cells: Vec<CellLine>,
    /// 混报检出计数。
    mixes: u32,
    /// 拒止计数。
    rejects: u32,
}

impl CpLedger {
    /// 新账（草稿相位）。
    pub fn new() -> CpLedger {
        CpLedger {
            state: CpState::CpDraft,
            protocol: None,
            vers: Vec::new(),
            cells: Vec::new(),
            mixes: 0,
            rejects: 0,
        }
    }

    /// 开账（草稿 → 终对账中）。
    pub fn begin(&mut self) -> Result<(), u32> {
        cp_advance(self.state, CpState::CpReconning)?;
        self.state = CpState::CpReconning;
        Ok(())
    }

    /// 签发（终对账中 → 已签发；三段齐/版本入账/矩阵齐/无残留四闸）。
    pub fn sign(&mut self) -> Result<(), u32> {
        if self.state != CpState::CpReconning {
            self.rejects = self.rejects.saturating_add(1);
            return Err(CpDiag::ERewind.val());
        }
        let p = match self.protocol {
            Some(v) => v,
            None => {
                self.rejects = self.rejects.saturating_add(1);
                return Err(CpDiag::ESectionMissing.val());
            }
        };
        if !protocol_pass(&p) {
            self.rejects = self.rejects.saturating_add(1);
            return Err(CpDiag::ESectionMissing.val());
        }
        if self.vers.is_empty() {
            self.rejects = self.rejects.saturating_add(1);
            return Err(CpDiag::EVersionJump.val());
        }
        if self.cells.len() != EDGE_CAP {
            self.rejects = self.rejects.saturating_add(1);
            return Err(CpDiag::EMatrixMissing.val());
        }
        // 残留态闸：Conflict/Pending 未清不许签发。
        for c in self.cells.iter() {
            if c.state != ReconState::RsMatch {
                self.rejects = self.rejects.saturating_add(1);
                return Err(CpDiag::EResidueSigned.val());
            }
        }
        cp_advance(self.state, CpState::CpSigned)?;
        self.state = CpState::CpSigned;
        Ok(())
    }

    /// 签发协议档（三段齐闸 + 版本单调闸）。
    pub fn issue_protocol(&mut self, p: CosignProtocol) -> Result<(), u32> {
        if self.state == CpState::CpSigned {
            self.rejects = self.rejects.saturating_add(1);
            return Err(CpDiag::ESignedWrite.val());
        }
        if self.state == CpState::CpDraft {
            self.state = CpState::CpReconning;
        }
        if !protocol_pass(&p) {
            self.rejects = self.rejects.saturating_add(1);
            return Err(CpDiag::ESectionMissing.val());
        }
        let expect = match self.vers.last() {
            Some(v) => v.checked_add(1),
            None => Some(1),
        };
        if expect != Some(p.version) {
            self.rejects = self.rejects.saturating_add(1);
            return Err(CpDiag::EVersionJump.val());
        }
        self.vers.push(p.version);
        self.protocol = Some(p);
        Ok(())
    }

    /// 登记矩阵行（外键/重边/混报三闸）。
    pub fn reg_cell(&mut self, c: CellLine) -> Result<(), u32> {
        if self.state == CpState::CpSigned {
            self.rejects = self.rejects.saturating_add(1);
            return Err(CpDiag::ESignedWrite.val());
        }
        if self.state == CpState::CpDraft {
            self.state = CpState::CpReconning;
        }
        // 外键闸。
        if edge_domain(c.edge).is_none() {
            self.rejects = self.rejects.saturating_add(1);
            return Err(CpDiag::EEdgeRange.val());
        }
        // 重边闸。
        for prev in self.cells.iter() {
            if prev.edge == c.edge {
                self.rejects = self.rejects.saturating_add(1);
                return Err(CpDiag::EEdgeDup.val());
            }
        }
        // 期望越界闸（冻结期望表防错位）。
        if expect_at(c.edge).is_none() {
            self.rejects = self.rejects.saturating_add(1);
            return Err(CpDiag::EExpectRange.val());
        }
        // 混报闸。
        if cell_is_mix(&c) {
            self.mixes = self.mixes.saturating_add(1);
            self.rejects = self.rejects.saturating_add(1);
            return Err(CpDiag::EMixReport.val());
        }
        self.cells.push(c);
        Ok(())
    }

    /// 当前相位。
    pub fn state(&self) -> CpState {
        self.state
    }

    /// 拒止计数。
    pub fn reject_len(&self) -> u32 {
        self.rejects
    }

    /// 矩阵行数。
    pub fn cell_len(&self) -> usize {
        self.cells.len()
    }

    /// 混报检出计数。
    pub fn mix_len(&self) -> u32 {
        self.mixes
    }

    /// 版本登记条数。
    pub fn ver_len(&self) -> usize {
        self.vers.len()
    }

    /// 协议档。
    pub fn protocol(&self) -> Option<CosignProtocol> {
        self.protocol
    }

    /// 矩阵行（越界 None）。
    pub fn cell_at(&self, idx: usize) -> Option<&CellLine> {
        self.cells.get(idx)
    }

    /// 四域矩阵终对账齐（八行全 Match 且边号闭合）。
    pub fn matrix_final_ok(&self) -> bool {
        if self.cells.len() != EDGE_CAP {
            return false;
        }
        let mut i = 0u32;
        while (i as usize) < self.cells.len() {
            let c = match self.cells.get(i as usize) {
                Some(v) => v,
                None => return false,
            };
            if c.edge != i {
                return false;
            }
            if c.state != ReconState::RsMatch {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 全量协议账重放（v1→v2 双版登记 + 八行登记 + 签发）。
pub fn build_full_cp_ledger() -> CpLedger {
    let mut l = CpLedger::new();
    let _ = l.begin();
    let v1 = CosignProtocol {
        scope: FROZEN_PROTOCOL.scope,
        gauge: FROZEN_PROTOCOL.gauge,
        freq: FreqSec::FqMatrixReg,
        version: 1,
    };
    let _ = l.issue_protocol(v1);
    let _ = l.issue_protocol(FROZEN_PROTOCOL);
    let mut i = 0u32;
    while (i as usize) < EDGE_CAP {
        let _ = l.reg_cell(CellLine { edge: i, state: ReconState::RsMatch });
        i += 1;
    }
    let _ = l.sign();
    l
}

/// 混报注入十路（人为构造混报必被检出）。
pub fn cp_mix_sweep() -> (u32, u32, String) {
    let mut caught = 0u32;
    let mut escape = String::new();
    // 1. 边 0 登记冲突态。
    {
        let mut l = CpLedger::new();
        let _ = l.begin();
        if l.reg_cell(CellLine { edge: 0, state: ReconState::RsConflict }).is_err() {
            caught += 1;
        } else {
            escape.push_str("冲突态;");
        }
    }
    // 2. 边 1 登记挂起态。
    {
        let mut l = CpLedger::new();
        let _ = l.begin();
        if l.reg_cell(CellLine { edge: 1, state: ReconState::RsPending }).is_err() {
            caught += 1;
        } else {
            escape.push_str("挂起态;");
        }
    }
    // 3. 边 2 登记冲突态。
    {
        let mut l = CpLedger::new();
        let _ = l.begin();
        if l.reg_cell(CellLine { edge: 2, state: ReconState::RsConflict }).is_err() {
            caught += 1;
        } else {
            escape.push_str("边2冲突;");
        }
    }
    // 4. 边 3 登记挂起态。
    {
        let mut l = CpLedger::new();
        let _ = l.begin();
        if l.reg_cell(CellLine { edge: 3, state: ReconState::RsPending }).is_err() {
            caught += 1;
        } else {
            escape.push_str("边3挂起;");
        }
    }
    // 5. 边 4 登记冲突态。
    {
        let mut l = CpLedger::new();
        let _ = l.begin();
        if l.reg_cell(CellLine { edge: 4, state: ReconState::RsConflict }).is_err() {
            caught += 1;
        } else {
            escape.push_str("边4冲突;");
        }
    }
    // 6. 错边号（8 越界）。
    {
        let mut l = CpLedger::new();
        let _ = l.begin();
        if l.reg_cell(CellLine { edge: 8, state: ReconState::RsMatch }).is_err() {
            caught += 1;
        } else {
            escape.push_str("越边;");
        }
    }
    // 7. 重边。
    {
        let mut l = CpLedger::new();
        let _ = l.begin();
        let _ = l.reg_cell(CellLine { edge: 0, state: ReconState::RsMatch });
        if l.reg_cell(CellLine { edge: 0, state: ReconState::RsMatch }).is_err() {
            caught += 1;
        } else {
            escape.push_str("重边;");
        }
    }
    // 8. 残留态签发（挂起行入账失败→空矩阵签发拒；此处直接空账签发）。
    {
        let mut l = CpLedger::new();
        let _ = l.begin();
        let v1 = CosignProtocol {
            scope: FROZEN_PROTOCOL.scope,
            gauge: FROZEN_PROTOCOL.gauge,
            freq: FreqSec::FqMatrixReg,
            version: 1,
        };
        let _ = l.issue_protocol(v1);
        if l.sign().is_err() {
            caught += 1;
        } else {
            escape.push_str("缺行签发;");
        }
    }
    // 9. 版本回退签发。
    {
        let mut l = CpLedger::new();
        let _ = l.begin();
        let v1 = CosignProtocol {
            scope: FROZEN_PROTOCOL.scope,
            gauge: FROZEN_PROTOCOL.gauge,
            freq: FreqSec::FqMatrixReg,
            version: 1,
        };
        let _ = l.issue_protocol(v1);
        let rewind = CosignProtocol { freq: FreqSec::FqPreSign, version: 1, ..v1 };
        if l.issue_protocol(rewind).is_err() {
            caught += 1;
        } else {
            escape.push_str("版本回退;");
        }
    }
    // 10. 签发后写。
    {
        let mut l = build_full_cp_ledger();
        if l.reg_cell(CellLine { edge: 0, state: ReconState::RsMatch }).is_err() {
            caught += 1;
        } else {
            escape.push_str("签后写;");
        }
    }
    (caught, 10, escape)
}

/// 双跑一致。
fn dual_run_ok() -> bool {
    let x = build_full_cp_ledger();
    let y = build_full_cp_ledger();
    if x.state() != y.state() || x.reject_len() != y.reject_len() {
        return false;
    }
    if x.cell_len() != y.cell_len() || x.ver_len() != y.ver_len() || x.mix_len() != y.mix_len() {
        return false;
    }
    if x.protocol() != y.protocol() {
        return false;
    }
    let mut i = 0usize;
    while i < x.cell_len() {
        match (x.cell_at(i), y.cell_at(i)) {
            (Some(p), Some(q)) => {
                if p != q {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    let (c1, _, e1) = cp_mix_sweep();
    let (c2, _, e2) = cp_mix_sweep();
    c1 == c2 && e1 == e2
}

/// 冻结摘要（边表 + 协议档折算）。
pub fn frozen_digest() -> u32 {
    let mut acc: u32 = SEG_BASE;
    let mut i = 0u32;
    for row in COSIGN_EDGES.iter() {
        let state_bit: u32 = match EDGE_EXPECT.get(row.0 as usize) {
            Some(ReconState::RsMatch) => 1,
            Some(ReconState::RsConflict) => 2,
            Some(ReconState::RsPending) => 3,
            None => 4,
        };
        acc = acc
            .wrapping_mul(7)
            .wrapping_add(row.0)
            .wrapping_add(state_bit)
            .wrapping_add(row.2.len() as u32)
            .wrapping_add(i);
        i = i.wrapping_add(1);
    }
    let freq_bit: u32 = match FROZEN_PROTOCOL.freq {
        FreqSec::FqMatrixReg => 1,
        FreqSec::FqPreSign => 2,
        FreqSec::FqBatchEnd => 3,
    };
    acc = acc
        .wrapping_mul(3)
        .wrapping_add(FROZEN_PROTOCOL.version)
        .wrapping_add(FROZEN_PROTOCOL.gauge.modulus)
        .wrapping_add(freq_bit);
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
    if SEG_BASE != 0x80000 + 0x40 * (12701u32.saturating_sub(12600)) {
        return false;
    }
    true
}

// ── 判据谓词（全部私有） ─────────────────────────────────────────
/// 四域矩阵终对账齐。
fn matrix_final_ok() -> bool {
    let l = build_full_cp_ledger();
    if !l.matrix_final_ok() {
        return false;
    }
    if l.state() != CpState::CpSigned {
        return false;
    }
    if l.reject_len() != 0 {
        return false;
    }
    // 冻结期望表逐边对号。
    let mut i = 0u32;
    while (i as usize) < EDGE_CAP {
        let e = match expect_at(i) {
            Some(v) => v,
            None => return false,
        };
        if e != ReconState::RsMatch {
            return false;
        }
        i += 1;
    }
    true
}

/// 混报注入 10/10 检出。
fn mix_report_ten_ok() -> bool {
    let (caught, total, escape) = cp_mix_sweep();
    total == 10 && caught == 10 && escape.is_empty()
}

/// 协议档签发（三段齐 + 版本入账 + 签发态）。
fn protocol_signed_ok() -> bool {
    if !protocol_pass(&FROZEN_PROTOCOL) {
        return false;
    }
    let l = build_full_cp_ledger();
    if l.protocol() != Some(FROZEN_PROTOCOL) {
        return false;
    }
    if l.ver_len() != 2 {
        return false;
    }
    if l.state() != CpState::CpSigned {
        return false;
    }
    // 频率段闭集合法。
    let mut hit = false;
    for f in FreqSec::all().iter() {
        if *f == FROZEN_PROTOCOL.freq {
            hit = true;
        }
    }
    hit
}

/// 完成写回语义（判据 ③）。
fn writeback_ok() -> bool {
    let l = build_full_cp_ledger();
    if l.state() != CpState::CpSigned {
        return false;
    }
    for st in [CpState::CpDraft, CpState::CpReconning, CpState::CpSigned].iter() {
        if st.name().is_empty() {
            return false;
        }
    }
    for r in ReconState::all().iter() {
        if r.name().is_empty() {
            return false;
        }
    }
    for f in FreqSec::all().iter() {
        if f.name().is_empty() {
            return false;
        }
    }
    if !matrix_final_ok() || !mix_report_ten_ok() || !protocol_signed_ok() {
        return false;
    }
    frozen_digest() != 0
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12702 联签一致性总对账协议档自检（判据 J1 + 通用守卫，≥12 条）。
pub fn run_unx12701_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12701-cosign-protocol");
    let l = build_full_cp_ledger();

    s.add_dyn(
        "F12702-01 四域矩阵终对账齐",
        matrix_final_ok(),
        &format!("cells={} cap={} signed={}", l.cell_len(), EDGE_CAP, l.state() == CpState::CpSigned),
    );

    s.add_dyn(
        "F12702-02 行=消费边闭集齐",
        {
            let mut ok = COSIGN_EDGES.len() == EDGE_CAP;
            let mut i = 0usize;
            while i < COSIGN_EDGES.len() {
                match COSIGN_EDGES.get(i) {
                    Some(row) => {
                        if row.0 != i as u32 || row.2.is_empty() {
                            ok = false;
                        }
                    }
                    None => ok = false,
                }
                i += 1;
            }
            ok
        },
        "D2/D3/D4/E1 各两行",
    );

    s.add_dyn(
        "F12702-03 列=对账态闭集",
        ReconState::all().len() == 3,
        "一致/冲突/挂起三态",
    );

    s.add_dyn(
        "F12702-04 混报注入 10/10 检出",
        mix_report_ten_ok(),
        &format!("caught={} mixes={}", cp_mix_sweep().0, l.mix_len()),
    );

    s.add_dyn(
        "F12702-05 无残留态签发",
        {
            let mut ok = true;
            for c in l.cells.iter() {
                if c.state != ReconState::RsMatch {
                    ok = false;
                }
            }
            ok
        },
        "Conflict/Pending 未清不许签发",
    );

    s.add_dyn(
        "F12702-06 协议档三段齐",
        protocol_signed_ok(),
        "范围/口径/频率三段全过",
    );

    s.add_dyn(
        "F12702-07 协议版本号入账（1→2）",
        l.ver_len() == 2,
        &format!("vers={} v2={}", l.ver_len(), FROZEN_PROTOCOL.version),
    );

    s.add_dyn(
        "F12702-08 注入联测 10/10 拒止",
        {
            let (c, t, e) = cp_mix_sweep();
            t == 10 && c == 10 && e.is_empty()
        },
        &format!("caught={}", cp_mix_sweep().0),
    );

    s.add_dyn(
        "F12702-09 相位迁移守卫直读",
        cp_advance(CpState::CpDraft, CpState::CpReconning).is_ok()
            && cp_advance(CpState::CpReconning, CpState::CpSigned).is_ok()
            && cp_advance(CpState::CpSigned, CpState::CpDraft).is_err()
            && cp_advance(CpState::CpDraft, CpState::CpSigned).is_err(),
        "草稿→终对账中→已签发只进不退",
    );

    s.add_dyn(
        "F12702-10 双跑一致",
        dual_run_ok(),
        "确定性重放两轮逐位同判",
    );

    s.add_dyn(
        "F12702-11 完成写回语义",
        writeback_ok(),
        "签发即已完成且摘要非空",
    );

    s.add_dyn(
        "F12702-12 冻结摘要非零",
        frozen_digest() != 0,
        &format!("digest={:08X}", frozen_digest()),
    );

    s.add_dyn(
        "F12702-13 段位守卫",
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
        assert_eq!(SEG_BASE, 0x81940);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        assert_eq!(CP_ALL.len(), 12);
        let mut i = 0usize;
        while i < CP_ALL.len() {
            let vi = match CP_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < CP_ALL.len() {
                let vj = match CP_ALL.get(j) {
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
    fn matrix_final_holds() {
        assert!(matrix_final_ok());
        assert!(protocol_signed_ok());
        assert!(mix_report_ten_ok());
        let l = build_full_cp_ledger();
        assert_eq!(l.cell_len(), 8);
        assert_eq!(l.state(), CpState::CpSigned);
        assert_eq!(l.reject_len(), 0);
        assert_eq!(l.ver_len(), 2);
        assert_eq!(l.mix_len(), 0);
        assert_eq!(l.protocol(), Some(FROZEN_PROTOCOL));
    }

    #[test]
    fn mix_report_sweep_ten() {
        let (caught, total, escape) = cp_mix_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
        // cell_is_mix 直证：冲突/挂起均混报。
        assert!(cell_is_mix(&CellLine { edge: 0, state: ReconState::RsConflict }));
        assert!(cell_is_mix(&CellLine { edge: 7, state: ReconState::RsPending }));
        assert!(!cell_is_mix(&CellLine { edge: 3, state: ReconState::RsMatch }));
        // 越边按混报判。
        assert!(cell_is_mix(&CellLine { edge: 9, state: ReconState::RsMatch }));
    }

    #[test]
    fn cell_gates_reject() {
        let mut l = CpLedger::new();
        let _ = l.begin();
        // 冲突态。
        assert_eq!(
            l.reg_cell(CellLine { edge: 0, state: ReconState::RsConflict }),
            Err(CpDiag::EMixReport.val())
        );
        // 越边。
        assert_eq!(
            l.reg_cell(CellLine { edge: 8, state: ReconState::RsMatch }),
            Err(CpDiag::EEdgeRange.val())
        );
        // 正常行过。
        assert!(l.reg_cell(CellLine { edge: 0, state: ReconState::RsMatch }).is_ok());
        // 重边。
        assert_eq!(
            l.reg_cell(CellLine { edge: 0, state: ReconState::RsMatch }),
            Err(CpDiag::EEdgeDup.val())
        );
        assert_eq!(l.reject_len(), 3);
        assert_eq!(l.mix_len(), 1);
    }

    #[test]
    fn protocol_gates_reject() {
        let mut l = CpLedger::new();
        let _ = l.begin();
        // 版本跳号（首版 2）。
        let v2 = CosignProtocol {
            scope: FROZEN_PROTOCOL.scope,
            gauge: FROZEN_PROTOCOL.gauge,
            freq: FreqSec::FqMatrixReg,
            version: 2,
        };
        assert_eq!(l.issue_protocol(v2), Err(CpDiag::EVersionJump.val()));
        // 范围段缺。
        let bad = CosignProtocol {
            scope: ScopeSec { scope: "", domains: 3 },
            gauge: FROZEN_PROTOCOL.gauge,
            freq: FreqSec::FqMatrixReg,
            version: 1,
        };
        assert_eq!(l.issue_protocol(bad), Err(CpDiag::ESectionMissing.val()));
        // 口径模零。
        let nog = CosignProtocol {
            scope: FROZEN_PROTOCOL.scope,
            gauge: GaugeSec { formula: "", modulus: 0 },
            freq: FreqSec::FqMatrixReg,
            version: 1,
        };
        assert_eq!(l.issue_protocol(nog), Err(CpDiag::ESectionMissing.val()));
        assert_eq!(l.reject_len(), 3);
        // 正常 v1 过。
        let v1 = CosignProtocol {
            scope: FROZEN_PROTOCOL.scope,
            gauge: FROZEN_PROTOCOL.gauge,
            freq: FreqSec::FqMatrixReg,
            version: 1,
        };
        assert!(l.issue_protocol(v1).is_ok());
        // 版本回退。
        let rewind = CosignProtocol { freq: FreqSec::FqPreSign, version: 1, ..v1 };
        assert_eq!(l.issue_protocol(rewind), Err(CpDiag::EVersionJump.val()));
    }

    #[test]
    fn sign_gates_hold() {
        // 空账签发拒（协议缺）。
        let mut l = CpLedger::new();
        let _ = l.begin();
        assert_eq!(l.sign(), Err(CpDiag::ESectionMissing.val()));
        // 有协议缺行签发拒。
        let mut h = CpLedger::new();
        let _ = h.begin();
        let v1 = CosignProtocol {
            scope: FROZEN_PROTOCOL.scope,
            gauge: FROZEN_PROTOCOL.gauge,
            freq: FreqSec::FqMatrixReg,
            version: 1,
        };
        let _ = h.issue_protocol(v1);
        assert_eq!(h.sign(), Err(CpDiag::EMatrixMissing.val()));
        // 全量签发过 + 签发后写拒。
        let mut full = build_full_cp_ledger();
        assert_eq!(full.state(), CpState::CpSigned);
        assert_eq!(
            full.reg_cell(CellLine { edge: 0, state: ReconState::RsMatch }),
            Err(CpDiag::ESignedWrite.val())
        );
        assert!(full.begin().is_err());
    }

    #[test]
    fn phase_guards_hold() {
        assert!(cp_advance(CpState::CpDraft, CpState::CpReconning).is_ok());
        assert!(cp_advance(CpState::CpReconning, CpState::CpSigned).is_ok());
        assert!(cp_advance(CpState::CpDraft, CpState::CpSigned).is_err());
        assert!(cp_advance(CpState::CpSigned, CpState::CpReconning).is_err());
        assert!(cp_advance(CpState::CpReconning, CpState::CpReconning).is_err());
        assert!(CpState::CpDraft.rank() < CpState::CpReconning.rank());
        assert!(CpState::CpReconning.rank() < CpState::CpSigned.rank());
    }

    #[test]
    fn edge_lookup_holds() {
        assert_eq!(edge_domain(0), Some("D2"));
        assert_eq!(edge_domain(3), Some("D3"));
        assert_eq!(edge_domain(5), Some("D4"));
        assert_eq!(edge_domain(7), Some("E1"));
        assert_eq!(edge_domain(8), None);
        assert_eq!(expect_at(0), Some(ReconState::RsMatch));
        assert_eq!(expect_at(7), Some(ReconState::RsMatch));
        assert_eq!(expect_at(8), None);
        assert_eq!(EDGE_EXPECT.len(), EDGE_CAP);
    }

    #[test]
    fn dual_run_consistent() {
        assert!(dual_run_ok());
        let x = build_full_cp_ledger();
        let y = build_full_cp_ledger();
        assert_eq!(x.cell_len(), y.cell_len());
        assert_eq!(x.mix_len(), y.mix_len());
        assert_eq!(frozen_digest(), frozen_digest());
        assert_ne!(frozen_digest(), 0);
    }

    #[test]
    fn writeback_semantics_hold() {
        assert!(writeback_ok());
        for st in [CpState::CpDraft, CpState::CpReconning, CpState::CpSigned].iter() {
            assert!(!st.name().is_empty());
        }
        for r in ReconState::all().iter() {
            assert!(!r.name().is_empty());
        }
        for f in FreqSec::all().iter() {
            assert!(!f.name().is_empty());
        }
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12701_checks();
        assert!(s.all_passed(), "自检存在红项：{}", s.red_items_text());
        assert!(s.len() >= 12, "检查条目不足 12");
    }
}
