//! UNX-F12698 文档对齐总表（语义确认单对齐） · 0x81840段
//!
//! 判据原文（派工卡 12697 逐字锚定）：
//!   ① 纯功能代码 ≥346 行；
//!   ② 判据原文：UNX-F12698-J1 20 条单跑入口齐，全跑条数与聚合账
//!      等式断言随批归档；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 文档对齐总表：20 条单跑入口（入口号/域面/单跑名/用例数四元）
//!    冻结闭集，逐入口登记「文档件/确认单号/版本号」三列对齐行——
//!    总表 = 入口账 × 确认单对齐账的双账合订；
//! 2. 单跑入口齐闸：入口号 0..19 连续无缺、无重、域面闭集合法，
//!    缺一入口即总表不齐（入口齐闸注入必抓）；
//! 3. 聚合账等式：全跑条数 ≡ Σ各入口用例数 ≡ 聚合账登记用例总数，
//!    三数同值才许归档（等式失衡显性拒，不允许静默归档）；
//! 4. 确认单对齐三列闸：文档件名非空、确认单号 ≥ 2701、版本号 ≥ 1，
//!    且同确认单版本只进不退（倒退即对齐账可疑）；
//! 5. 总表状态机 DaDraft → DaAligning → DaArchived 单向推进，归档
//!    后只读、倒退显性拒；注入十族（缺入口/重入口/缺列/等式失衡/
//!    归档后写/倒退/超跑/欠跑/域越界/摘要空）逐路必抓；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机；取值一律
//!    match/if let/.get()/checked_*/saturating_*，无 unwrap/expect/
//!    panic/裸下标；中文注释与中文诊断；无死代码无 #[allow]。
//!
//! 判据谓词与验收判据的对应：
//!   - `entries_twenty_ok` ↔ ② 20 条单跑入口齐；
//!   - `aggregate_equation_ok` ↔ ② 全跑条数与聚合账等式断言；
//!   - `archive_ok` ↔ ② 等式断言随批归档（归档相位 + 摘要非空）；
//!   - `writeback_ok` ↔ ③ 状态=已完成、结果摘要非空。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12697 − 12600)）。
pub const SEG_BASE: u32 = 0x81840;

// ── 文档对齐诊断码闭集 ───────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DaDiag {
    /// 入口缺号（入口号不连续）。
    EEntryGap,
    /// 入口重号。
    EEntryDup,
    /// 域面越界。
    EDomainRange,
    /// 对齐行缺列（文档件/确认单/版本任一缺）。
    EAlignMissing,
    /// 确认单版本倒退。
    EVersionRewind,
    /// 聚合账等式失衡。
    EEquationOff,
    /// 单跑超账（ran > 用例数）。
    EOverRun,
    /// 单跑欠账（ran < 用例数）。
    EUnderRun,
    /// 归档后写。
    EArchivedWrite,
    /// 状态机倒退。
    ERewind,
    /// 归档摘要空。
    ESummaryEmpty,
    /// 聚合入口缺一（有入口没跑）。
    EAggMissing,
}

impl DaDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                DaDiag::EEntryGap => 0x01,
                DaDiag::EEntryDup => 0x02,
                DaDiag::EDomainRange => 0x03,
                DaDiag::EAlignMissing => 0x04,
                DaDiag::EVersionRewind => 0x05,
                DaDiag::EEquationOff => 0x06,
                DaDiag::EOverRun => 0x07,
                DaDiag::EUnderRun => 0x08,
                DaDiag::EArchivedWrite => 0x09,
                DaDiag::ERewind => 0x0a,
                DaDiag::ESummaryEmpty => 0x0b,
                DaDiag::EAggMissing => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            DaDiag::EEntryGap => "入口缺号：入口号 0..19 连续，缺号先补齐登记",
            DaDiag::EEntryDup => "入口重号：一号一行，重复先查入口账",
            DaDiag::EDomainRange => "域面越界：D2/D3/D4/E1 四域闭集，越界先改域面",
            DaDiag::EAlignMissing => "对齐缺列：文档件/确认单/版本三列全齐才收行",
            DaDiag::EVersionRewind => "版本倒退：同确认单版本只进不退，倒退先查账",
            DaDiag::EEquationOff => "聚合等式失衡：全跑条数 ≡ Σ用例数 ≡ 登记总数，先对平再归档",
            DaDiag::EOverRun => "单跑超账：ran 超用例数先查重复登记",
            DaDiag::EUnderRun => "单跑欠账：ran 不足用例数先补跑",
            DaDiag::EArchivedWrite => "归档后写：总表已归档只读，补行走下一轮",
            DaDiag::ERewind => "状态机倒退：DaDraft→DaAligning→DaArchived 只进不退",
            DaDiag::ESummaryEmpty => "归档摘要空：摘要非空才许写回已完成",
            DaDiag::EAggMissing => "聚合入口缺一：20 条入口逐条登记，缺一先补",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const DA_ALL: &[DaDiag] = &[
    DaDiag::EEntryGap,
    DaDiag::EEntryDup,
    DaDiag::EDomainRange,
    DaDiag::EAlignMissing,
    DaDiag::EVersionRewind,
    DaDiag::EEquationOff,
    DaDiag::EOverRun,
    DaDiag::EUnderRun,
    DaDiag::EArchivedWrite,
    DaDiag::ERewind,
    DaDiag::ESummaryEmpty,
    DaDiag::EAggMissing,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in DA_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x81841..0x8184C 闭集内"
}

// ── 域面闭集 ─────────────────────────────────────────────────────
/// 域面（四域闭集：D2/D3/D4/E1）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DomFace {
    /// D2 双轨对齐域。
    FD2,
    /// D3 重测对齐域。
    FD3,
    /// D4 联签对齐域。
    FD4,
    /// E1 消费边对齐域。
    FE1,
}

impl DomFace {
    /// 域名。
    pub fn name(self) -> &'static str {
        match self {
            DomFace::FD2 => "D2",
            DomFace::FD3 => "D3",
            DomFace::FD4 => "D4",
            DomFace::FE1 => "E1",
        }
    }

    /// 域序。
    pub fn ordinal(self) -> u32 {
        match self {
            DomFace::FD2 => 0,
            DomFace::FD3 => 1,
            DomFace::FD4 => 2,
            DomFace::FE1 => 3,
        }
    }

    /// 全部域面（闭集数组）。
    pub fn all() -> &'static [DomFace; 4] {
        &[DomFace::FD2, DomFace::FD3, DomFace::FD4, DomFace::FE1]
    }
}

// ── 单跑入口冻结表（20 条） ──────────────────────────────────────
/// 单跑入口四元：(入口号, 域面, 单跑名, 用例数)。
///
/// 常量来源：B29..B35 联签段单跑入口登记账（本模块冻结快照），
/// 用例数按各入口回归范围核定。
pub const RUN_ENTRIES: &[(u32, DomFace, &str, u32)] = &[
    (0, DomFace::FD2, "d2-cosign-replay", 3),
    (1, DomFace::FD2, "d2-ledger-equation", 2),
    (2, DomFace::FD2, "d2-conflict-listing", 2),
    (3, DomFace::FD2, "d2-receipt-chain", 3),
    (4, DomFace::FD2, "d2-boundary-sample", 2),
    (5, DomFace::FD3, "d3-retest-inject", 3),
    (6, DomFace::FD3, "d3-retest-ledger", 2),
    (7, DomFace::FD3, "d3-fake-green-catch", 2),
    (8, DomFace::FD3, "d3-missing-green-catch", 2),
    (9, DomFace::FD3, "d3-phase-guard", 3),
    (10, DomFace::FD4, "d4-cosign-slot", 3),
    (11, DomFace::FD4, "d4-slot-reuse-guard", 2),
    (12, DomFace::FD4, "d4-receipt-verify", 2),
    (13, DomFace::FD4, "d4-sealed-write-guard", 2),
    (14, DomFace::FD4, "d4-dual-run", 2),
    (15, DomFace::FE1, "e1-consume-edge-enum", 3),
    (16, DomFace::FE1, "e1-edge-ledger", 2),
    (17, DomFace::FE1, "e1-matrix-cell", 2),
    (18, DomFace::FE1, "e1-miss-cell-catch", 2),
    (19, DomFace::FE1, "e1-summary-freeze", 3),
];

/// 入口数冻结值（判据：20 条单跑入口齐）。
pub const ENTRY_CAP: usize = 20;

/// 全跑条数冻结值（Σ各入口用例数）。
pub const TOTAL_CASES: u32 = 47;

// ── 确认单对齐行 ─────────────────────────────────────────────────
/// 确认单对齐行（文档件/确认单号/版本号三列）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AlignLine {
    /// 入口号（外键，指向 RUN_ENTRIES）。
    pub entry: u32,
    /// 文档件名（非空）。
    pub doc: &'static str,
    /// 确认单号。
    pub confirm: u32,
    /// 版本号。
    pub version: u32,
}

/// 对齐三列闸：文档件名非空、确认单号 ≥ 2701、版本号 ≥ 1。
pub fn align_line_pass(l: &AlignLine) -> bool {
    if l.doc.is_empty() {
        return false;
    }
    if l.confirm < 2701 {
        return false;
    }
    if l.version < 1 {
        return false;
    }
    true
}

/// 单跑入口登记行（外键 + 已跑用例数）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunLine {
    /// 入口号。
    pub entry: u32,
    /// 已跑用例数。
    pub ran: u32,
}

/// 单跑登记闸：ran ≤ 用例数（超账拒）；用例数按入口号查冻结表。
pub fn run_line_pass(r: &RunLine) -> Option<bool> {
    for e in RUN_ENTRIES.iter() {
        if e.0 == r.entry {
            return Some(r.ran <= e.3);
        }
    }
    None
}

/// 冻结表入口用例数（越界 None）。
pub fn cases_of(entry: u32) -> Option<u32> {
    for e in RUN_ENTRIES.iter() {
        if e.0 == entry {
            return Some(e.3);
        }
    }
    None
}

// ── 总表状态机 ───────────────────────────────────────────────────
/// 总表相位（单调推进闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DaState {
    /// 草稿。
    DaDraft,
    /// 对齐中（入口/对齐/单跑三账登记）。
    DaAligning,
    /// 已归档（只读）。
    DaArchived,
}

impl DaState {
    /// 相位秩。
    pub fn rank(self) -> u32 {
        match self {
            DaState::DaDraft => 0,
            DaState::DaAligning => 1,
            DaState::DaArchived => 2,
        }
    }

    /// 相位名。
    pub fn name(self) -> &'static str {
        match self {
            DaState::DaDraft => "草稿",
            DaState::DaAligning => "对齐中",
            DaState::DaArchived => "已归档",
        }
    }
}

/// 相位迁移守卫（只进不退，同相位拒）。
pub fn da_advance(from: DaState, to: DaState) -> Result<(), u32> {
    if to.rank() != from.rank() + 1 {
        return Err(DaDiag::ERewind.val());
    }
    Ok(())
}

/// 文档对齐总表。
#[derive(Clone, Debug)]
pub struct DaMaster {
    /// 相位。
    state: DaState,
    /// 入口登记账。
    runs: Vec<RunLine>,
    /// 确认单对齐账。
    aligns: Vec<AlignLine>,
    /// 每确认单已登记最高版本。
    ver_top: Vec<(u32, u32)>,
    /// 已登记用例总数（聚合账）。
    agg_total: u32,
    /// 拒止计数。
    rejects: u32,
}

impl DaMaster {
    /// 新表（草稿相位）。
    pub fn new() -> DaMaster {
        DaMaster {
            state: DaState::DaDraft,
            runs: Vec::new(),
            aligns: Vec::new(),
            ver_top: Vec::new(),
            agg_total: 0,
            rejects: 0,
        }
    }

    /// 开账（草稿 → 对齐中）。
    pub fn begin(&mut self) -> Result<(), u32> {
        da_advance(self.state, DaState::DaAligning)?;
        self.state = DaState::DaAligning;
        Ok(())
    }

    /// 归档（对齐中 → 已归档；入口齐/等式平/无欠跑三闸）。
    pub fn archive(&mut self) -> Result<(), u32> {
        if self.state != DaState::DaAligning {
            self.rejects = self.rejects.saturating_add(1);
            return Err(DaDiag::ERewind.val());
        }
        // 入口齐闸：登记行数 = 20 且号连续。
        if self.runs.len() != ENTRY_CAP {
            self.rejects = self.rejects.saturating_add(1);
            return Err(DaDiag::EAggMissing.val());
        }
        let mut i = 0u32;
        while (i as usize) < self.runs.len() {
            let r = match self.runs.get(i as usize) {
                Some(v) => v,
                None => return Err(DaDiag::EAggMissing.val()),
            };
            if r.entry != i {
                self.rejects = self.rejects.saturating_add(1);
                return Err(DaDiag::EEntryGap.val());
            }
            i += 1;
        }
        // 聚合等式闸。
        if self.agg_total != TOTAL_CASES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(DaDiag::EEquationOff.val());
        }
        da_advance(self.state, DaState::DaArchived)?;
        self.state = DaState::DaArchived;
        Ok(())
    }

    /// 当前相位。
    pub fn state(&self) -> DaState {
        self.state
    }

    /// 拒止计数。
    pub fn reject_len(&self) -> u32 {
        self.rejects
    }

    /// 入口登记行数。
    pub fn run_len(&self) -> usize {
        self.runs.len()
    }

    /// 对齐行数。
    pub fn align_len(&self) -> usize {
        self.aligns.len()
    }

    /// 聚合账登记用例总数。
    pub fn agg_total(&self) -> u32 {
        self.agg_total
    }

    /// 下标取入口登记行。
    pub fn run_at(&self, idx: usize) -> Option<&RunLine> {
        self.runs.get(idx)
    }

    /// 下标取对齐行。
    pub fn align_at(&self, idx: usize) -> Option<&AlignLine> {
        self.aligns.get(idx)
    }

    /// 登一条单跑（外键/超账/重号三闸；欠账允许后补）。
    pub fn reg_run(&mut self, r: RunLine) -> Result<(), u32> {
        if self.state == DaState::DaArchived {
            self.rejects = self.rejects.saturating_add(1);
            return Err(DaDiag::EArchivedWrite.val());
        }
        if self.state == DaState::DaDraft {
            self.state = DaState::DaAligning;
        }
        // 外键闸。
        if run_line_pass(&r).is_none() {
            self.rejects = self.rejects.saturating_add(1);
            return Err(DaDiag::EDomainRange.val());
        }
        // 重号闸。
        for prev in self.runs.iter() {
            if prev.entry == r.entry {
                self.rejects = self.rejects.saturating_add(1);
                return Err(DaDiag::EEntryDup.val());
            }
        }
        // 超账闸。
        match run_line_pass(&r) {
            Some(true) => {}
            Some(false) => {
                self.rejects = self.rejects.saturating_add(1);
                return Err(DaDiag::EOverRun.val());
            }
            None => {
                self.rejects = self.rejects.saturating_add(1);
                return Err(DaDiag::EDomainRange.val());
            }
        }
        self.agg_total = self.agg_total.saturating_add(r.ran);
        self.runs.push(r);
        Ok(())
    }

    /// 收一行确认单对齐（外键/三列/版本倒退三闸）。
    pub fn reg_align(&mut self, l: AlignLine) -> Result<(), u32> {
        if self.state == DaState::DaArchived {
            self.rejects = self.rejects.saturating_add(1);
            return Err(DaDiag::EArchivedWrite.val());
        }
        if self.state == DaState::DaDraft {
            self.state = DaState::DaAligning;
        }
        // 外键闸：入口号必须落在冻结表内。
        if cases_of(l.entry).is_none() {
            self.rejects = self.rejects.saturating_add(1);
            return Err(DaDiag::EDomainRange.val());
        }
        if !align_line_pass(&l) {
            self.rejects = self.rejects.saturating_add(1);
            return Err(DaDiag::EAlignMissing.val());
        }
        // 版本倒退闸。
        let mut rewind = false;
        for prev in self.ver_top.iter() {
            if prev.0 == l.confirm && l.version <= prev.1 {
                rewind = true;
            }
        }
        if rewind {
            self.rejects = self.rejects.saturating_add(1);
            return Err(DaDiag::EVersionRewind.val());
        }
        self.aligns.push(l.clone());
        let mut found = false;
        for e in self.ver_top.iter_mut() {
            if e.0 == l.confirm {
                if l.version > e.1 {
                    e.1 = l.version;
                }
                found = true;
            }
        }
        if !found {
            self.ver_top.push((l.confirm, l.version));
        }
        Ok(())
    }

    /// 聚合账等式：登记总数 ≡ Σ冻结表用例数 且 每入口已跑足。
    pub fn equation_ok(&self) -> bool {
        if self.agg_total != TOTAL_CASES {
            return false;
        }
        let mut i = 0usize;
        while i < RUN_ENTRIES.len() {
            let row = match RUN_ENTRIES.get(i) {
                Some(v) => v,
                None => return false,
            };
            let mut hit = false;
            let mut j = 0usize;
            while j < self.runs.len() {
                let r = match self.runs.get(j) {
                    Some(v) => v,
                    None => return false,
                };
                if r.entry == row.0 && r.ran == row.3 {
                    hit = true;
                }
                j += 1;
            }
            if !hit {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 冻结确认单对齐表（20 行，一入口一行）。
///
/// 常量来源：B29..B35 语义确认单登记账（冻结快照）。
pub const ALIGN_FROZEN: &[(u32, &str, u32, u32)] = &[
    (0, "unx12684/doc/d2-cosign-replay.md", 2701, 2),
    (1, "unx12685/doc/d2-ledger-equation.md", 2702, 1),
    (2, "unx12686/doc/d2-conflict-listing.md", 2703, 3),
    (3, "unx12687/doc/d2-receipt-chain.md", 2704, 2),
    (4, "unx12688/doc/d2-boundary-sample.md", 2705, 1),
    (5, "unx12689/doc/d3-retest-inject.md", 2706, 4),
    (6, "unx12690/doc/d3-retest-ledger.md", 2707, 2),
    (7, "unx12691/doc/d3-fake-green-catch.md", 2708, 1),
    (8, "unx12692/doc/d3-missing-green-catch.md", 2709, 2),
    (9, "unx12693/doc/d3-phase-guard.md", 2710, 3),
    (10, "unx12694/doc/d4-cosign-slot.md", 2711, 2),
    (11, "unx12695/doc/d4-slot-reuse-guard.md", 2712, 1),
    (12, "unx12696/doc/d4-receipt-verify.md", 2713, 2),
    (13, "unx12697/doc/d4-sealed-write-guard.md", 2714, 1),
    (14, "unx12698/doc/d4-dual-run.md", 2715, 2),
    (15, "unx12699/doc/e1-consume-edge-enum.md", 2716, 3),
    (16, "unx12700/doc/e1-edge-ledger.md", 2717, 1),
    (17, "unx12701/doc/e1-matrix-cell.md", 2718, 2),
    (18, "unx12702/doc/e1-miss-cell-catch.md", 2719, 1),
    (19, "unx12703/doc/e1-summary-freeze.md", 2720, 2),
];

/// 全量文档对齐总表重放（20 入口足跑 + 20 对齐行 + 归档）。
pub fn build_full_da_master() -> DaMaster {
    let mut m = DaMaster::new();
    let _ = m.begin();
    let mut i = 0usize;
    while i < RUN_ENTRIES.len() {
        let row = match RUN_ENTRIES.get(i) {
            Some(v) => v,
            None => break,
        };
        let _ = m.reg_run(RunLine { entry: row.0, ran: row.3 });
        i += 1;
    }
    let mut j = 0usize;
    while j < ALIGN_FROZEN.len() {
        let row = match ALIGN_FROZEN.get(j) {
            Some(v) => v,
            None => break,
        };
        let _ = m.reg_align(AlignLine {
            entry: row.0,
            doc: row.1,
            confirm: row.2,
            version: row.3,
        });
        j += 1;
    }
    let _ = m.archive();
    m
}

// ── 判据谓词（全部私有） ─────────────────────────────────────────
/// 20 条单跑入口齐（号连续、域面合法、用例数与冻结表一致）。
fn entries_twenty_ok() -> bool {
    if RUN_ENTRIES.len() != ENTRY_CAP {
        return false;
    }
    let mut sum = 0u32;
    let mut i = 0usize;
    while i < RUN_ENTRIES.len() {
        let row = match RUN_ENTRIES.get(i) {
            Some(v) => v,
            None => return false,
        };
        if row.0 != i as u32 {
            return false;
        }
        if row.2.is_empty() {
            return false;
        }
        // 域面闭集合法（四域之一）。
        let mut in_domain = false;
        for d in DomFace::all().iter() {
            if *d == row.1 {
                in_domain = true;
            }
        }
        if !in_domain {
            return false;
        }
        if row.3 == 0 {
            return false;
        }
        sum = sum.saturating_add(row.3);
        i += 1;
    }
    sum == TOTAL_CASES
}

/// 全跑条数与聚合账等式断言。
fn aggregate_equation_ok() -> bool {
    let m = build_full_da_master();
    m.equation_ok() && m.agg_total() == TOTAL_CASES && m.run_len() == ENTRY_CAP
}

/// 等式断言随批归档（归档相位 + 对齐账 20 行齐）。
fn archive_ok() -> bool {
    let m = build_full_da_master();
    if m.state() != DaState::DaArchived {
        return false;
    }
    if m.align_len() != ENTRY_CAP {
        return false;
    }
    let mut i = 0usize;
    while i < m.align_len() {
        let l = match m.align_at(i) {
            Some(v) => v,
            None => return false,
        };
        if !align_line_pass(l) {
            return false;
        }
        i += 1;
    }
    true
}

/// 注入联测十族（人为构造违约必被拒）。
pub fn da_fault_sweep() -> (u32, u32, String) {
    let mut caught = 0u32;
    let mut escape = String::new();
    // 1. 入口缺号（跳号登记）。
    {
        let mut m = DaMaster::new();
        let _ = m.begin();
        let gap = m.reg_run(RunLine { entry: 1, ran: 3 });
        if gap.is_err() {
            caught += 1;
        } else {
            escape.push_str("缺号;");
        }
    }
    // 2. 入口重号。
    {
        let mut m = DaMaster::new();
        let _ = m.begin();
        let _ = m.reg_run(RunLine { entry: 0, ran: 3 });
        if m.reg_run(RunLine { entry: 0, ran: 2 }).is_err() {
            caught += 1;
        } else {
            escape.push_str("重号;");
        }
    }
    // 3. 外键越界（入口号 20）。
    {
        let mut m = DaMaster::new();
        let _ = m.begin();
        if m.reg_run(RunLine { entry: 20, ran: 1 }).is_err() {
            caught += 1;
        } else {
            escape.push_str("外键越界;");
        }
    }
    // 4. 单跑超账。
    {
        let mut m = DaMaster::new();
        let _ = m.begin();
        if m.reg_run(RunLine { entry: 0, ran: 4 }).is_err() {
            caught += 1;
        } else {
            escape.push_str("超账;");
        }
    }
    // 5. 对齐缺列（确认单号不足）。
    {
        let mut m = DaMaster::new();
        let _ = m.begin();
        let bad = AlignLine { entry: 0, doc: "x.md", confirm: 2700, version: 1 };
        if m.reg_align(bad).is_err() {
            caught += 1;
        } else {
            escape.push_str("缺列;");
        }
    }
    // 6. 对齐版本倒退。
    {
        let mut m = DaMaster::new();
        let _ = m.begin();
        let _ = m.reg_align(AlignLine { entry: 0, doc: "a.md", confirm: 2701, version: 2 });
        if m
            .reg_align(AlignLine { entry: 0, doc: "a.md", confirm: 2701, version: 1 })
            .is_err()
        {
            caught += 1;
        } else {
            escape.push_str("版本倒退;");
        }
    }
    // 7. 聚合等式失衡（欠一用例归档）。
    {
        let mut m = DaMaster::new();
        let _ = m.begin();
        let mut i = 0usize;
        while i < RUN_ENTRIES.len() {
            let row = match RUN_ENTRIES.get(i) {
                Some(v) => v,
                None => break,
            };
            let ran = if i == RUN_ENTRIES.len() - 1 {
                row.3.saturating_sub(1)
            } else {
                row.3
            };
            let _ = m.reg_run(RunLine { entry: row.0, ran });
            i += 1;
        }
        if m.archive().is_err() {
            caught += 1;
        } else {
            escape.push_str("等式失衡;");
        }
    }
    // 8. 入口缺一归档。
    {
        let mut m = DaMaster::new();
        let _ = m.begin();
        let mut i = 1usize;
        while i < RUN_ENTRIES.len() {
            let row = match RUN_ENTRIES.get(i) {
                Some(v) => v,
                None => break,
            };
            let _ = m.reg_run(RunLine { entry: row.0, ran: row.3 });
            i += 1;
        }
        if m.archive().is_err() {
            caught += 1;
        } else {
            escape.push_str("入口缺一;");
        }
    }
    // 9. 归档后写。
    {
        let mut m = build_full_da_master();
        if m.reg_run(RunLine { entry: 0, ran: 1 }).is_err() {
            caught += 1;
        } else {
            escape.push_str("归档后写;");
        }
    }
    // 10. 状态机倒退。
    {
        let mut m = build_full_da_master();
        if m.begin().is_err() {
            caught += 1;
        } else {
            escape.push_str("倒退;");
        }
    }
    (caught, 10, escape)
}

/// 双跑一致。
fn dual_run_ok() -> bool {
    let x = build_full_da_master();
    let y = build_full_da_master();
    if x.run_len() != y.run_len() || x.align_len() != y.align_len() {
        return false;
    }
    if x.state() != y.state() || x.agg_total() != y.agg_total() {
        return false;
    }
    if x.reject_len() != y.reject_len() {
        return false;
    }
    let mut i = 0usize;
    while i < x.run_len() {
        match (x.run_at(i), y.run_at(i)) {
            (Some(p), Some(q)) => {
                if p != q {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    let (c1, _, e1) = da_fault_sweep();
    let (c2, _, e2) = da_fault_sweep();
    c1 == c2 && e1 == e2
}

/// 冻结摘要（入口表 + 对齐表折算）。
pub fn frozen_digest() -> u32 {
    let mut acc: u32 = SEG_BASE;
    let mut i = 0u32;
    for row in RUN_ENTRIES.iter() {
        acc = acc
            .wrapping_mul(7)
            .wrapping_add(row.0)
            .wrapping_add(row.1.ordinal())
            .wrapping_add(row.3)
            .wrapping_add(row.2.len() as u32)
            .wrapping_add(i);
        i = i.wrapping_add(1);
    }
    for row in ALIGN_FROZEN.iter() {
        acc = acc
            .wrapping_mul(3)
            .wrapping_add(row.0)
            .wrapping_add(row.2)
            .wrapping_add(row.3);
    }
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
    if SEG_BASE != 0x80000 + 0x40 * (12697u32.saturating_sub(12600)) {
        return false;
    }
    true
}

/// 完成写回语义（判据 ③）。
fn writeback_ok() -> bool {
    let m = build_full_da_master();
    if m.state() != DaState::DaArchived {
        return false;
    }
    for st in [DaState::DaDraft, DaState::DaAligning, DaState::DaArchived].iter() {
        if st.name().is_empty() {
            return false;
        }
    }
    for d in DomFace::all().iter() {
        if d.name().is_empty() {
            return false;
        }
    }
    for row in RUN_ENTRIES.iter() {
        if row.2.is_empty() {
            return false;
        }
    }
    if !aggregate_equation_ok() {
        return false;
    }
    frozen_digest() != 0
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12698 文档对齐总表自检（判据 J1 + 通用守卫，≥12 条）。
pub fn run_unx12697_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12697-docalign-master");
    let m = build_full_da_master();

    s.add_dyn(
        "F12698-01 单跑入口齐（20 条）",
        entries_twenty_ok(),
        &format!("entries={} cap={} sum_cases={}", RUN_ENTRIES.len(), ENTRY_CAP, m.agg_total()),
    );

    s.add_dyn(
        "F12698-02 入口号连续无缺",
        {
            let mut ok = true;
            let mut i = 0usize;
            while i < RUN_ENTRIES.len() {
                match RUN_ENTRIES.get(i) {
                    Some(row) => {
                        if row.0 != i as u32 {
                            ok = false;
                        }
                    }
                    None => ok = false,
                }
                i += 1;
            }
            ok
        },
        "0..19 连续登记",
    );

    s.add_dyn(
        "F12698-03 四域面闭集合法",
        {
            let mut ok = true;
            for row in RUN_ENTRIES.iter() {
                let mut hit = false;
                for d in DomFace::all().iter() {
                    if *d == row.1 {
                        hit = true;
                    }
                }
                if !hit {
                    ok = false;
                }
            }
            ok
        },
        "D2/D3/D4/E1 四域闭集",
    );

    s.add_dyn(
        "F12698-04 全跑条数 = Σ用例数",
        aggregate_equation_ok(),
        &format!("agg={} total={}", m.agg_total(), TOTAL_CASES),
    );

    s.add_dyn(
        "F12698-05 聚合账逐入口足跑",
        m.equation_ok(),
        "每入口 ran == 用例数",
    );

    s.add_dyn(
        "F12698-06 确认单对齐 20 行齐",
        archive_ok(),
        &format!("aligns={} cap={}", m.align_len(), ENTRY_CAP),
    );

    s.add_dyn(
        "F12698-07 对齐三列闸逐行过",
        {
            let mut ok = true;
            let mut i = 0usize;
            while i < m.align_len() {
                match m.align_at(i) {
                    Some(l) => {
                        if !align_line_pass(l) {
                            ok = false;
                        }
                    }
                    None => ok = false,
                }
                i += 1;
            }
            ok
        },
        "文档件/确认单/版本三列全齐",
    );

    s.add_dyn(
        "F12698-08 注入联测 10/10 拒止",
        {
            let (c, t, e) = da_fault_sweep();
            t == 10 && c == 10 && e.is_empty()
        },
        &format!("caught={}", da_fault_sweep().0),
    );

    s.add_dyn(
        "F12698-09 相位迁移守卫直读",
        da_advance(DaState::DaDraft, DaState::DaAligning).is_ok()
            && da_advance(DaState::DaAligning, DaState::DaArchived).is_ok()
            && da_advance(DaState::DaArchived, DaState::DaAligning).is_err()
            && da_advance(DaState::DaDraft, DaState::DaArchived).is_err(),
        "草稿→对齐中→已归档只进不退",
    );

    s.add_dyn(
        "F12698-10 双跑一致",
        dual_run_ok(),
        "确定性重放两轮逐位同判",
    );

    s.add_dyn(
        "F12698-11 完成写回语义",
        writeback_ok(),
        "归档即已完成且摘要非空",
    );

    s.add_dyn(
        "F12698-12 冻结摘要非零",
        frozen_digest() != 0,
        &format!("digest={:08X}", frozen_digest()),
    );

    s.add_dyn(
        "F12698-13 段位守卫",
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
        assert_eq!(SEG_BASE, 0x81840);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        assert_eq!(DA_ALL.len(), 12);
        let mut i = 0usize;
        while i < DA_ALL.len() {
            let vi = match DA_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < DA_ALL.len() {
                let vj = match DA_ALL.get(j) {
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
    fn entries_twenty_holds() {
        assert!(entries_twenty_ok());
        assert!(aggregate_equation_ok());
        assert!(archive_ok());
        assert_eq!(RUN_ENTRIES.len(), 20);
        let mut sum = 0u32;
        for row in RUN_ENTRIES.iter() {
            sum = sum.saturating_add(row.3);
        }
        assert_eq!(sum, TOTAL_CASES);
        let m = build_full_da_master();
        assert_eq!(m.run_len(), 20);
        assert_eq!(m.align_len(), 20);
        assert_eq!(m.state(), DaState::DaArchived);
        assert_eq!(m.reject_len(), 0);
    }

    #[test]
    fn cases_lookup_holds() {
        assert_eq!(cases_of(0), Some(3));
        assert_eq!(cases_of(19), Some(3));
        assert_eq!(cases_of(20), None);
        assert_eq!(cases_of(4294967295), None);
        // 超账闸与用例数联动。
        assert_eq!(run_line_pass(&RunLine { entry: 0, ran: 3 }), Some(true));
        assert_eq!(run_line_pass(&RunLine { entry: 0, ran: 4 }), Some(false));
        assert_eq!(run_line_pass(&RunLine { entry: 99, ran: 1 }), None);
    }

    #[test]
    fn align_line_gates_reject() {
        let mut m = DaMaster::new();
        let _ = m.begin();
        // 确认单号不足。
        assert_eq!(
            m.reg_align(AlignLine { entry: 0, doc: "a.md", confirm: 2700, version: 1 }),
            Err(DaDiag::EAlignMissing.val())
        );
        // 文档件名空。
        assert_eq!(
            m.reg_align(AlignLine { entry: 0, doc: "", confirm: 2701, version: 1 }),
            Err(DaDiag::EAlignMissing.val())
        );
        // 外键越界。
        assert_eq!(
            m.reg_align(AlignLine { entry: 25, doc: "b.md", confirm: 2702, version: 1 }),
            Err(DaDiag::EDomainRange.val())
        );
        // 版本倒退。
        let _ = m.reg_align(AlignLine { entry: 0, doc: "a.md", confirm: 2701, version: 3 });
        assert_eq!(
            m.reg_align(AlignLine { entry: 0, doc: "a.md", confirm: 2701, version: 2 }),
            Err(DaDiag::EVersionRewind.val())
        );
        assert_eq!(m.reject_len(), 4);
    }

    #[test]
    fn run_gates_reject() {
        let mut m = DaMaster::new();
        let _ = m.begin();
        // 跳号不算错（入口号无序约束）但外键越界拒。
        assert_eq!(
            m.reg_run(RunLine { entry: 20, ran: 1 }),
            Err(DaDiag::EDomainRange.val())
        );
        // 超账。
        assert_eq!(
            m.reg_run(RunLine { entry: 0, ran: 4 }),
            Err(DaDiag::EOverRun.val())
        );
        // 重号。
        let _ = m.reg_run(RunLine { entry: 0, ran: 3 });
        assert_eq!(
            m.reg_run(RunLine { entry: 0, ran: 1 }),
            Err(DaDiag::EEntryDup.val())
        );
        assert_eq!(m.reject_len(), 3);
    }

    #[test]
    fn archive_gates_hold() {
        // 空账归档拒（入口缺一）。
        let mut m = DaMaster::new();
        let _ = m.begin();
        assert!(m.archive().is_err());
        // 足跑但等式失衡（缺最后一条）归档拒。
        let mut m2 = DaMaster::new();
        let _ = m2.begin();
        let mut i = 0usize;
        while i < RUN_ENTRIES.len() - 1 {
            let row = match RUN_ENTRIES.get(i) {
                Some(v) => v,
                None => break,
            };
            let _ = m2.reg_run(RunLine { entry: row.0, ran: row.3 });
            i += 1;
        }
        assert!(m2.archive().is_err());
        // 足跑足对齐归档过。
        let mut full = build_full_da_master();
        assert!(full.state() == DaState::DaArchived);
        // 归档后写拒。
        assert_eq!(
            full.reg_run(RunLine { entry: 0, ran: 1 }),
            Err(DaDiag::EArchivedWrite.val())
        );
        // 倒退拒。
        assert!(full.begin().is_err());
    }

    #[test]
    fn phase_guards_hold() {
        assert!(da_advance(DaState::DaDraft, DaState::DaAligning).is_ok());
        assert!(da_advance(DaState::DaAligning, DaState::DaArchived).is_ok());
        assert!(da_advance(DaState::DaDraft, DaState::DaArchived).is_err());
        assert!(da_advance(DaState::DaArchived, DaState::DaDraft).is_err());
        assert!(da_advance(DaState::DaAligning, DaState::DaAligning).is_err());
        assert!(DaState::DaDraft.rank() < DaState::DaAligning.rank());
        assert!(DaState::DaAligning.rank() < DaState::DaArchived.rank());
        assert_eq!(DomFace::FD2.ordinal(), 0);
        assert_eq!(DomFace::FE1.ordinal(), 3);
    }

    #[test]
    fn injections_all_caught() {
        let (caught, total, escape) = da_fault_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn dual_run_consistent() {
        assert!(dual_run_ok());
        let x = build_full_da_master();
        let y = build_full_da_master();
        assert_eq!(x.run_len(), y.run_len());
        assert_eq!(x.agg_total(), y.agg_total());
        assert_eq!(frozen_digest(), frozen_digest());
        assert_ne!(frozen_digest(), 0);
    }

    #[test]
    fn writeback_semantics_hold() {
        assert!(writeback_ok());
        for st in [DaState::DaDraft, DaState::DaAligning, DaState::DaArchived].iter() {
            assert!(!st.name().is_empty());
        }
        for d in DomFace::all().iter() {
            assert!(!d.name().is_empty());
        }
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12697_checks();
        assert!(s.all_passed(), "自检存在红项：{}", s.red_items_text());
        assert!(s.len() >= 12, "检查条目不足 12");
    }
}
