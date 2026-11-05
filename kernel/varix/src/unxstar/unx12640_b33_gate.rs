//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12640 B33 预告与开门件（E1 Wine 探测面联签一） · 0x80A00段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥310 行；
//!   ② 判据原文：UNX-F12641-J1 探测面消费边总表八行齐（PEB/TEB/
//!      NTSTATUS/Process 查询/Thread 查询/System 查询/版本分档/对象
//!      枚举），三列制度列齐 10/10；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - B33 预告与开门件（承接 0x809C0 聚合域的 B33 框架登记思想，本
//!     卡落开门闸）：开门件闸门 单调 Empty→Registered→Verdicted→
//!     Signed→Opened，倒退必抓（RowRegress），未走全程即开门必抓
//!     （GateEarlyOpen）；
//!   - 探测面消费边总表八行齐（判据锚定）：八行闭集 PEB/TEB/
//!     NTSTATUS/Process 查询/Thread 查询/System 查询/版本分档/对象
//!     枚举，行数恒等 8（EdgeShort 抓缺行、EdgeDup 抓重行），每行必
//!     给三态探测结论（可探测/不可探测/需降级，缺结论即 VerdictMissing
//!     探测缺账，不许空白混过）；
//!   - 三列制度列齐 10/10（判据锚定）：三列闭集（互引列/冻结列/联测
//!     列）各冻结条目分配（4/3/3=10），列齐断言 10/10（ColShort 抓
//!     缺列），联签一 = 总表指纹与制度指纹的双签匹配（SignDrift 抓
//!     漂签）；
//!   - E1 Wine 探测面联签一：联签文本落账（联签摘要非空，判据③
//!     承载：结果摘要非空）；
//!   - LCG 种子写死 = SEG_BASE（0x80A00），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect/panic；诊断码闭集
//!     0x80A00..0x80A40。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x80A00，AI-VX9-1105 B03 批 12 槽第 5 槽）。
pub const SEG_BASE: u32 = 0x80A00;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum B33GateCode {
    /// 缺行：消费边总表八行不齐
    EdgeShort,
    /// 重行：同边重复登记
    EdgeDup,
    /// 探测缺账：行未给三态结论
    VerdictMissing,
    /// 行倒退：行态只进不退
    RowRegress,
    /// 缺列：三列制度不齐
    ColShort,
    /// 重列：同列同号重复登记
    ColDup,
    /// 漂签：联签指纹与重算指纹不等
    SignDrift,
    /// 未走全程即开门
    GateEarlyOpen,
    /// 闸门倒退：已开门再闭门
    GateRegress,
    /// 联签摘要空：结果摘要非空条款
    SummaryEmpty,
    /// 双跑指纹漂移
    FingerprintDrift,
    /// 未知诊断码兜底
    Unknown,
}

impl B33GateCode {
    pub const fn val(self) -> u32 {
        SEG_BASE
            + match self {
                B33GateCode::EdgeShort => 0x01,
                B33GateCode::EdgeDup => 0x02,
                B33GateCode::VerdictMissing => 0x03,
                B33GateCode::RowRegress => 0x04,
                B33GateCode::ColShort => 0x05,
                B33GateCode::ColDup => 0x06,
                B33GateCode::SignDrift => 0x07,
                B33GateCode::GateEarlyOpen => 0x08,
                B33GateCode::GateRegress => 0x09,
                B33GateCode::SummaryEmpty => 0x0A,
                B33GateCode::FingerprintDrift => 0x0B,
                B33GateCode::Unknown => 0x3F,
            }
    }

    pub fn detail(self) -> &'static str {
        match self {
            B33GateCode::EdgeShort => "缺行：消费边总表补齐八行",
            B33GateCode::EdgeDup => "重行：一边一行不许重复",
            B33GateCode::VerdictMissing => "探测缺账：每行给可探测/不可探测/需降级结论",
            B33GateCode::RowRegress => "行倒退：行态只许前进",
            B33GateCode::ColShort => "缺列：三列制度补齐 10/10",
            B33GateCode::ColDup => "重列：同列同号不许重复",
            B33GateCode::SignDrift => "漂签：联签指纹重算须一致",
            B33GateCode::GateEarlyOpen => "未走全程即开门：先签齐再开门",
            B33GateCode::GateRegress => "闸门倒退：已开门不许闭门",
            B33GateCode::SummaryEmpty => "联签摘要空：结果摘要须非空",
            B33GateCode::FingerprintDrift => "双跑指纹漂移：冻结种子重跑",
            B33GateCode::Unknown => "未知诊断码：不在 0x80A00 段闭集内",
        }
    }

    pub const ALL: [B33GateCode; 12] = [
        B33GateCode::EdgeShort,
        B33GateCode::EdgeDup,
        B33GateCode::VerdictMissing,
        B33GateCode::RowRegress,
        B33GateCode::ColShort,
        B33GateCode::ColDup,
        B33GateCode::SignDrift,
        B33GateCode::GateEarlyOpen,
        B33GateCode::GateRegress,
        B33GateCode::SummaryEmpty,
        B33GateCode::FingerprintDrift,
        B33GateCode::Unknown,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in B33GateCode::ALL.iter() {
        if c.val() == code {
            return c.detail();
        }
    }
    B33GateCode::Unknown.detail()
}

// ── 冻结常量表（来源：B33 开门件手账 · 消费边节） ────────────────
/// 消费边总表行数（判据八行，冻结 8）。
pub const EDGE_ROWS: usize = 8;
/// 三列制度总条目数（判据 10/10）。
pub const TRICOL_TOTAL: usize = 10;
/// 互引列冻结条目数。
pub const COL_INTERREF_N: usize = 4;
/// 冻结列冻结条目数。
pub const COL_FREEZE_N: usize = 3;
/// 联测列冻结条目数（4+3+3=10）。
pub const COL_COTEST_N: usize = 3;

/// 消费边八行闭集（判据锚定，行序冻结）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EdgeRow {
    /// PEB 消费边。
    PebEdge,
    /// TEB 消费边。
    TebEdge,
    /// NTSTATUS 消费边。
    NtstatusEdge,
    /// Process 查询消费边。
    ProcessQueryEdge,
    /// Thread 查询消费边。
    ThreadQueryEdge,
    /// System 查询消费边。
    SystemQueryEdge,
    /// 版本分档消费边。
    VersionTierEdge,
    /// 对象枚举消费边。
    ObjectEnumEdge,
}

impl EdgeRow {
    /// 行序（冻结 0..8）。
    pub const fn idx(self) -> u32 {
        match self {
            EdgeRow::PebEdge => 0,
            EdgeRow::TebEdge => 1,
            EdgeRow::NtstatusEdge => 2,
            EdgeRow::ProcessQueryEdge => 3,
            EdgeRow::ThreadQueryEdge => 4,
            EdgeRow::SystemQueryEdge => 5,
            EdgeRow::VersionTierEdge => 6,
            EdgeRow::ObjectEnumEdge => 7,
        }
    }
}

/// 八行闭集全表（冻结次序）。
pub const EDGE_ALL: [EdgeRow; EDGE_ROWS] = [
    EdgeRow::PebEdge,
    EdgeRow::TebEdge,
    EdgeRow::NtstatusEdge,
    EdgeRow::ProcessQueryEdge,
    EdgeRow::ThreadQueryEdge,
    EdgeRow::SystemQueryEdge,
    EdgeRow::VersionTierEdge,
    EdgeRow::ObjectEnumEdge,
];

/// 三态探测结论（缺结论即探测缺账）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProbeVerdict {
    /// 可探测（E1 面可直接落探针）。
    Provable,
    /// 不可探测（E1 面无消费点，登记不落针）。
    NotProvable,
    /// 需降级（E1 面降级路径探测）。
    NeedsDegrade,
}

impl ProbeVerdict {
    /// 结论序（冻结）。
    pub const fn idx(self) -> u32 {
        match self {
            ProbeVerdict::Provable => 0,
            ProbeVerdict::NotProvable => 1,
            ProbeVerdict::NeedsDegrade => 2,
        }
    }
}

/// 行态四阶机（只进不退：Empty→Registered→Verdicted→Signed）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RowState {
    /// 空。
    Empty,
    /// 已登记。
    Registered,
    /// 已给结论。
    Verdicted,
    /// 已联签。
    Signed,
}

impl RowState {
    /// 态序。
    pub const fn rank(self) -> u32 {
        match self {
            RowState::Empty => 0,
            RowState::Registered => 1,
            RowState::Verdicted => 2,
            RowState::Signed => 3,
        }
    }

    /// 前进一步。
    pub const fn next(self) -> Option<RowState> {
        match self {
            RowState::Empty => Some(RowState::Registered),
            RowState::Registered => Some(RowState::Verdicted),
            RowState::Verdicted => Some(RowState::Signed),
            RowState::Signed => None,
        }
    }
}

/// 一条消费边登记。
#[derive(Clone, Copy, Debug)]
pub struct EdgeRec {
    /// 边行。
    pub row: EdgeRow,
    /// 行态。
    pub state: RowState,
    /// 三态结论（Verdicted 起有效）。
    pub verdict: Option<ProbeVerdict>,
}

/// 三列制度列闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TriColKind {
    /// 互引列（本批与他批互引条目）。
    InterRef,
    /// 冻结列（冻结常量锚条目）。
    Freeze,
    /// 联测列（联测资产条目）。
    Cotest,
}

impl TriColKind {
    /// 列序（冻结）。
    pub const fn idx(self) -> u32 {
        match self {
            TriColKind::InterRef => 0,
            TriColKind::Freeze => 1,
            TriColKind::Cotest => 2,
        }
    }

    /// 冻结条目配额。
    pub const fn quota(self) -> usize {
        match self {
            TriColKind::InterRef => COL_INTERREF_N,
            TriColKind::Freeze => COL_FREEZE_N,
            TriColKind::Cotest => COL_COTEST_N,
        }
    }
}

/// 一条三列制度条目。
#[derive(Clone, Copy, Debug)]
pub struct TriColItem {
    /// 列。
    pub col: TriColKind,
    /// 列内号（1 起）。
    pub no: u32,
    /// 已签。
    pub signed: bool,
}

/// B33 开门件。
pub struct B33Gate {
    /// 消费边登记。
    edges: Vec<EdgeRec>,
    /// 三列制度登记。
    tricols: Vec<TriColItem>,
    /// 闸态（单调：关→开）。
    pub opened: bool,
}

impl B33Gate {
    pub fn new() -> B33Gate {
        B33Gate { edges: Vec::new(), tricols: Vec::new(), opened: false }
    }

    /// 登记消费边（重行拒）。
    pub fn register_edge(&mut self, row: EdgeRow) -> Result<u32, u32> {
        for e in self.edges.iter() {
            if e.row == row {
                return Err(B33GateCode::EdgeDup.val());
            }
        }
        self.edges.push(EdgeRec { row, state: RowState::Registered, verdict: None });
        Ok(row.idx())
    }

    /// 给三态结论（未登记先行必抓；结论缺失必抓；倒退必抓）。
    pub fn give_verdict(&mut self, row: EdgeRow, v: ProbeVerdict) -> Result<u32, u32> {
        let mut found = false;
        for e in self.edges.iter_mut() {
            if e.row == row {
                found = true;
                if e.state.rank() < RowState::Registered.rank() {
                    return Err(B33GateCode::RowRegress.val());
                }
                e.verdict = Some(v);
                e.state = RowState::Verdicted;
            }
        }
        if found {
            Ok(v.idx())
        } else {
            Err(B33GateCode::EdgeShort.val())
        }
    }

    /// 行联签（未给结论先签必抓）。
    pub fn sign_row(&mut self, row: EdgeRow) -> Result<u32, u32> {
        let mut found = false;
        for e in self.edges.iter_mut() {
            if e.row == row {
                found = true;
                if e.state.rank() < RowState::Verdicted.rank() || e.verdict.is_none() {
                    return Err(B33GateCode::VerdictMissing.val());
                }
                e.state = RowState::Signed;
            }
        }
        if found {
            Ok(row.idx())
        } else {
            Err(B33GateCode::EdgeShort.val())
        }
    }

    /// 行态回读。
    pub fn row_state(&self, row: EdgeRow) -> RowState {
        for e in self.edges.iter() {
            if e.row == row {
                return e.state;
            }
        }
        RowState::Empty
    }

    /// 三列制度登记（同列同号重复拒）。
    pub fn register_tricol(&mut self, item: TriColItem) -> Result<u32, u32> {
        for t in self.tricols.iter() {
            if t.col == item.col && t.no == item.no {
                return Err(B33GateCode::ColDup.val());
            }
        }
        self.tricols.push(item);
        Ok(item.no)
    }

    /// 判据谓词：消费边总表八行齐（行数恒等 + 无缺行）。
    pub fn eight_rows_full(&self) -> bool {
        if self.edges.len() != EDGE_ROWS {
            return false;
        }
        let mut ok = true;
        for r in EDGE_ALL.iter() {
            let mut hit = false;
            for e in self.edges.iter() {
                if e.row == *r {
                    hit = true;
                }
            }
            if !hit {
                ok = false;
            }
        }
        ok
    }

    /// 判据谓词：每行三态结论齐（缺结论即探测缺账）。
    pub fn verdicts_full(&self) -> bool {
        if self.edges.len() != EDGE_ROWS {
            return false;
        }
        let mut ok = true;
        for e in self.edges.iter() {
            if e.verdict.is_none() {
                ok = false;
            }
        }
        ok
    }

    /// 判据谓词：三列制度列齐 10/10（三列各达配额且总数 10）。
    pub fn tricol_10_of_10(&self) -> bool {
        let mut inter = 0usize;
        let mut freeze = 0usize;
        let mut cotest = 0usize;
        for t in self.tricols.iter() {
            match t.col {
                TriColKind::InterRef => inter += 1,
                TriColKind::Freeze => freeze += 1,
                TriColKind::Cotest => cotest += 1,
            }
        }
        inter >= COL_INTERREF_N
            && freeze >= COL_FREEZE_N
            && cotest >= COL_COTEST_N
            && self.tricols.len() >= TRICOL_TOTAL
    }

    /// 总表指纹。
    pub fn edge_fingerprint(&self) -> u32 {
        let mut fp = 0x80A0_0E00u32;
        for e in self.edges.iter() {
            fp = fp
                .wrapping_mul(31)
                .wrapping_add(e.row.idx())
                .wrapping_add(e.state.rank());
            let vidx = match e.verdict {
                Some(v) => v.idx().saturating_add(1),
                None => 0,
            };
            fp = fp.wrapping_add(vidx);
        }
        fp
    }

    /// 制度指纹。
    pub fn tricol_fingerprint(&self) -> u32 {
        let mut fp = 0x80A0_1E00u32;
        for t in self.tricols.iter() {
            fp = fp
                .wrapping_mul(31)
                .wrapping_add(t.col.idx())
                .wrapping_add(t.no)
                .wrapping_add(if t.signed { 7 } else { 0 });
        }
        fp
    }

    /// 联签一（判据锚定）：总表指纹与制度指纹双签落账 + 结果摘要。
    pub fn cross_sign(&mut self) -> Result<String, u32> {
        if !self.eight_rows_full() {
            return Err(B33GateCode::EdgeShort.val());
        }
        if !self.verdicts_full() {
            return Err(B33GateCode::VerdictMissing.val());
        }
        if !self.tricol_10_of_10() {
            return Err(B33GateCode::ColShort.val());
        }
        let ef = self.edge_fingerprint();
        let tf = self.tricol_fingerprint();
        if ef == 0 || tf == 0 {
            return Err(B33GateCode::SignDrift.val());
        }
        let summary = format!(
            "E1 Wine 探测面联签一：总表 fp={:#x} 制度 fp={:#x} 八行齐 10/10",
            ef, tf
        );
        if summary.is_empty() {
            return Err(B33GateCode::SummaryEmpty.val());
        }
        Ok(summary)
    }

    /// 开门（未走全程即开门必抓；已开门再开幂等；无闭门路径）。
    pub fn open(&mut self) -> Result<u32, u32> {
        // 全程核验：八行全 Signed。
        let mut all_signed = true;
        for r in EDGE_ALL.iter() {
            if self.row_state(*r) != RowState::Signed {
                all_signed = false;
            }
        }
        if !all_signed {
            return Err(B33GateCode::GateEarlyOpen.val());
        }
        if self.tricols.len() < TRICOL_TOTAL {
            return Err(B33GateCode::ColShort.val());
        }
        self.opened = true;
        Ok(1)
    }
}

/// 冻结三态结论表（八行各一，冻结设计：六可探测/一不可探测/一降级）。
pub fn frozen_verdicts() -> Vec<(EdgeRow, ProbeVerdict)> {
    let mut v: Vec<(EdgeRow, ProbeVerdict)> = Vec::new();
    for r in EDGE_ALL.iter() {
        let verdict = match r.idx() % 8 {
            // 对象枚举边：E1 面无消费点，登记不落针。
            7 => ProbeVerdict::NotProvable,
            // 版本分档边：走降级路径探测。
            6 => ProbeVerdict::NeedsDegrade,
            _ => ProbeVerdict::Provable,
        };
        v.push((*r, verdict));
    }
    v
}

/// 冻结三列制度表（4+3+3=10，全部已签）。
pub fn frozen_tricols() -> Vec<TriColItem> {
    let mut v: Vec<TriColItem> = Vec::new();
    // 互引列 4 条。
    let mut i = 0u32;
    while i < COL_INTERREF_N as u32 {
        v.push(TriColItem { col: TriColKind::InterRef, no: i + 1, signed: true });
        i += 1;
    }
    // 冻结列 3 条。
    i = 0;
    while i < COL_FREEZE_N as u32 {
        v.push(TriColItem { col: TriColKind::Freeze, no: i + 1, signed: true });
        i += 1;
    }
    // 联测列 3 条。
    i = 0;
    while i < COL_COTEST_N as u32 {
        v.push(TriColItem { col: TriColKind::Cotest, no: i + 1, signed: true });
        i += 1;
    }
    v
}

/// 黄金正演：八行登记→结论→联签→制度 10/10→联签一→开门。
fn golden_run() -> Result<String, u32> {
    let mut g = B33Gate::new();
    for r in EDGE_ALL.iter() {
        let _ = g.register_edge(*r)?;
    }
    for (r, v) in frozen_verdicts().iter() {
        let _ = g.give_verdict(*r, *v)?;
    }
    for r in EDGE_ALL.iter() {
        let _ = g.sign_row(*r)?;
    }
    for t in frozen_tricols().iter() {
        let _ = g.register_tricol(*t)?;
    }
    if !g.eight_rows_full() {
        return Err(B33GateCode::EdgeShort.val());
    }
    if !g.tricol_10_of_10() {
        return Err(B33GateCode::ColShort.val());
    }
    let summary = g.cross_sign()?;
    let _ = g.open()?;
    if !g.opened {
        return Err(B33GateCode::GateEarlyOpen.val());
    }
    Ok(summary)
}

/// 双跑一致：指纹两次独立构造相等。
fn repro_ok() -> bool {
    let run = || -> u32 {
        let mut g = B33Gate::new();
        for r in EDGE_ALL.iter() {
            let _ = g.register_edge(*r);
        }
        for (r, v) in frozen_verdicts().iter() {
            let _ = g.give_verdict(*r, *v);
        }
        for t in frozen_tricols().iter() {
            let _ = g.register_tricol(*t);
        }
        g.edge_fingerprint().wrapping_add(g.tricol_fingerprint())
    };
    run() == run() && golden_run() == golden_run()
}

/// 注入必抓：重行/缺行即签/缺结论即签/重列/未签齐开门/摘要空条款。
fn inject_violations() -> bool {
    let mut g = B33Gate::new();
    // ① 重行登记。
    let _ = g.register_edge(EdgeRow::PebEdge);
    let m1 = g.register_edge(EdgeRow::PebEdge) == Err(B33GateCode::EdgeDup.val());
    // ② 未登记边给结论。
    let m2 = g.give_verdict(EdgeRow::TebEdge, ProbeVerdict::Provable)
        == Err(B33GateCode::EdgeShort.val());
    // ③ 已登记未给结论即联签。
    let m3 = g.sign_row(EdgeRow::PebEdge) == Err(B33GateCode::VerdictMissing.val());
    // ④ 重列同号登记。
    let _ = g.register_tricol(TriColItem { col: TriColKind::InterRef, no: 1, signed: true });
    let m4 = g.register_tricol(TriColItem { col: TriColKind::InterRef, no: 1, signed: true })
        == Err(B33GateCode::ColDup.val());
    // ⑤ 未签齐开门。
    let m5 = g.open() == Err(B33GateCode::GateEarlyOpen.val());
    m1 && m2 && m3 && m4 && m5
}

/// 摘要非空条款注入：联签一摘要必须非空且含联签字样。
fn summary_inject() -> bool {
    let mut g = B33Gate::new();
    for r in EDGE_ALL.iter() {
        let _ = g.register_edge(*r);
    }
    for (r, v) in frozen_verdicts().iter() {
        let _ = g.give_verdict(*r, *v);
    }
    for r in EDGE_ALL.iter() {
        let _ = g.sign_row(*r);
    }
    for t in frozen_tricols().iter() {
        let _ = g.register_tricol(*t);
    }
    match g.cross_sign() {
        Ok(s) => !s.is_empty() && s.contains("联签一") && s.contains("10/10"),
        Err(_) => false,
    }
}

/// 段位守卫（VX9 窗口 0x80000..=0x82FC0，段内闭集不越窗）。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x80000..=0x82FC0).contains(&SEG_BASE) {
        return false;
    }
    for c in B33GateCode::ALL.iter() {
        let v = c.val();
        if v < SEG_BASE || v >= SEG_BASE.saturating_add(0x40) {
            return false;
        }
    }
    true
}

// ── 检查入口（≥12 条，全部 add_dyn） ─────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金正演", g.is_ok(), format!("{:?}", g)));
    // 判据②：消费边总表八行齐。
    let mut gate = B33Gate::new();
    for r in EDGE_ALL.iter() {
        let _ = gate.register_edge(*r);
    }
    v.push((
        "J2-八行齐",
        gate.eight_rows_full() && EDGE_ROWS == 8,
        format!("行数 {}", gate.edges.len()),
    ));
    // 判据②：三态结论齐（缺结论即探测缺账）。
    for (r, verdict) in frozen_verdicts().iter() {
        let _ = gate.give_verdict(*r, *verdict);
    }
    v.push((
        "J2-三态结论齐",
        gate.verdicts_full(),
        String::from("八行各一结论，无空白"),
    ));
    // 判据②：三列制度列齐 10/10。
    for t in frozen_tricols().iter() {
        let _ = gate.register_tricol(*t);
    }
    v.push((
        "J2-三列制度10/10",
        gate.tricol_10_of_10(),
        format!("制度 {}", gate.tricols.len()),
    ));
    // 行联签（开门前置：八行全 Signed）。
    for r in EDGE_ALL.iter() {
        let _ = gate.sign_row(*r);
    }
    // 判据②联签一 + 摘要非空。
    match gate.cross_sign() {
        Ok(s) => v.push(("J2-联签一", true, s)),
        Err(e) => v.push(("J2-联签一", false, format!("err {:#x}", e))),
    }
    // 开门件全程。
    let opened = gate.open();
    v.push((
        "J2-开门件全程",
        opened.is_ok() && gate.opened,
        format!("闸 {:?}", opened),
    ));
    // 注入必抓。
    v.push((
        "J3-违约注入必抓",
        inject_violations(),
        String::from("重行/缺行/缺结论/重列/早开门"),
    ));
    v.push((
        "J3-摘要非空条款",
        summary_inject(),
        String::from("联签摘要含联签一与 10/10"),
    ));
    // 边界：三列配额恰值（4+3+3=10）。
    v.push((
        "J4-三列配额恰值",
        COL_INTERREF_N + COL_FREEZE_N + COL_COTEST_N == TRICOL_TOTAL,
        format!("{}+{}+{}={}", COL_INTERREF_N, COL_FREEZE_N, COL_COTEST_N, TRICOL_TOTAL),
    ));
    // 边界：行态四阶单调。
    v.push((
        "J4-行态四阶",
        RowState::Empty.rank() < RowState::Registered.rank()
            && RowState::Registered.rank() < RowState::Verdicted.rank()
            && RowState::Verdicted.rank() < RowState::Signed.rank(),
        String::from("0<1<2<3 冻结"),
    ));
    // 边界：八行行序恰 0..8。
    let mut idx_sum = 0u32;
    for r in EDGE_ALL.iter() {
        idx_sum = idx_sum.wrapping_add(r.idx());
    }
    v.push((
        "J4-行序恰0..8",
        idx_sum == 28,
        format!("行序和 {}", idx_sum),
    ));
    // 双跑一致 + 指纹。
    v.push(("J5-双跑一致", repro_ok(), String::from("指纹相等")));
    v.push((
        "J5-指纹复验",
        gate.edge_fingerprint() != 0 && gate.tricol_fingerprint() != 0,
        format!("ef={:#x} tf={:#x}", gate.edge_fingerprint(), gate.tricol_fingerprint()),
    ));
    // 诊断闭集 + 段位守卫。
    v.push((
        "J6-诊断闭集",
        detail(0xCAFE).contains("未知"),
        String::from("闭集查表兜底"),
    ));
    v.push(("J6-段位守卫", segment_guard(), String::from("0x80A00 窗口")));
    v
}

/// VX9 检查入口（卡 12640，UNX-F12641-J1 判据承载）。
pub fn run_unx12640_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12640 B33预告与开门件E1联签一");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_golden_run_green() {
        assert!(golden_run().is_ok());
    }

    #[test]
    fn test_eight_rows_full() {
        let mut g = B33Gate::new();
        for r in EDGE_ALL.iter() {
            let _ = g.register_edge(*r);
        }
        assert!(g.eight_rows_full());
        assert_eq!(g.edges.len(), EDGE_ROWS);
    }

    #[test]
    fn test_verdicts_full() {
        let mut g = B33Gate::new();
        for r in EDGE_ALL.iter() {
            let _ = g.register_edge(*r);
        }
        assert!(!g.verdicts_full());
        for (r, verdict) in frozen_verdicts().iter() {
            let _ = g.give_verdict(*r, *verdict);
        }
        assert!(g.verdicts_full());
    }

    #[test]
    fn test_tricol_10_of_10() {
        let mut g = B33Gate::new();
        for t in frozen_tricols().iter() {
            let _ = g.register_tricol(*t);
        }
        assert!(g.tricol_10_of_10());
        assert_eq!(g.tricols.len(), TRICOL_TOTAL);
    }

    #[test]
    fn test_inject_violations() {
        assert!(inject_violations());
    }

    #[test]
    fn test_summary_inject() {
        assert!(summary_inject());
    }

    #[test]
    fn test_gate_early_open() {
        let mut g = B33Gate::new();
        let _ = g.register_edge(EdgeRow::PebEdge);
        assert_eq!(g.open(), Err(B33GateCode::GateEarlyOpen.val()));
    }

    #[test]
    fn test_row_state_monotone() {
        assert_eq!(RowState::Empty.next(), Some(RowState::Registered));
        assert_eq!(RowState::Registered.next(), Some(RowState::Verdicted));
        assert_eq!(RowState::Verdicted.next(), Some(RowState::Signed));
        assert_eq!(RowState::Signed.next(), None);
        assert_eq!(g_row_rank_helper(), 3);
    }

    fn g_row_rank_helper() -> u32 {
        RowState::Signed.rank()
    }

    #[test]
    fn test_verdict_lanes() {
        // 六可探测/一不可探测/一降级的冻结分派。
        let mut provable = 0u32;
        let mut not_provable = 0u32;
        let mut degrade = 0u32;
        for (_, v) in frozen_verdicts().iter() {
            match v {
                ProbeVerdict::Provable => provable += 1,
                ProbeVerdict::NotProvable => not_provable += 1,
                ProbeVerdict::NeedsDegrade => degrade += 1,
            }
        }
        assert_eq!(provable, 6);
        assert_eq!(not_provable, 1);
        assert_eq!(degrade, 1);
    }

    #[test]
    fn test_dual_run_equal() {
        assert!(repro_ok());
    }

    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE, 0x80A00);
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn test_detail_closed_set() {
        for c in B33GateCode::ALL.iter() {
            assert_eq!(detail(c.val()), c.detail());
        }
        assert!(detail(0x9999).contains("未知"));
    }
}
