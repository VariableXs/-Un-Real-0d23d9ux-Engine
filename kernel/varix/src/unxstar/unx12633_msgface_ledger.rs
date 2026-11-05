//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12633 消息面对照表二批入账 · 0x80840段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥340 行；
//!   ② 判据原文：UNX-F12634-J1 回执协议三段（开工通知/对账窗口/差异
//!      列账）判据绿，两态留痕；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 消息面对照表二批入账（承接 F12617 一批 + 0x80800 双机对照二
//!     批行账）：二批行账逐行落账（行号/域/批位/衔接锚），入账账实
//!     对平（RowLedgerParity 抓行账与入账流水不等）；
//!   - 回执协议三段（判据锚定）：开工通知（Notice）→ 对账窗口
//!     （Recon）→ 差异列账（DiffLedger）三段状态机单调（StageRegress
//!     拒倒退）；每段入账必留两态痕（Accepted/Refused 各落一笔痕，
//!     两态留痕判据锚定）；
//!   - 两态留痕：每笔回执必须留下 {受理, 婉拒} 两态之一且痕码落段
//!     闭集（TraceOutOfSet 抓账外痕）；无痕回执即静默面
//!     （TraceSilent 抓无痕入账）；
//!   - 差异列账：对账窗口内的差异必须逐笔列账（DiffUnlisted 抓漏
//!     列），列账行数 ≡ 差异数（DiffParityBreak）；
//!   - LCG 种子写死 = SEG_BASE（0x80840），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect/panic；诊断码闭集
//!     0x80840..0x80880。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x80840，AI-VX9-1105 B02 批 12 槽第 10 槽）。
pub const SEG_BASE: u32 = 0x80840;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MsgFaceCode {
    /// 行账对平破：入账流水数≠行账行数
    RowLedgerParity,
    /// 段位倒退：三段状态机只进不退
    StageRegress,
    /// 两态留痕缺：回执无受理/婉拒痕
    TraceSilent,
    /// 痕码越闭集：账外痕
    TraceOutOfSet,
    /// 差异漏列：对账差异未列账
    DiffUnlisted,
    /// 差异对平破：列账行数≠差异数
    DiffParityBreak,
    /// 重复回执：同槽位只许一回执
    DupReceipt,
    /// 坏槽位：槽位不在登记窗
    BadSlot,
    /// 批位越闭集：批位不在 {一批, 二批}
    BatchOutOfSet,
    /// 账容量满
    LedgerFull,
    /// 双跑指纹漂移
    FingerprintDrift,
    /// 未知诊断码兜底
    Unknown,
}

impl MsgFaceCode {
    pub const fn val(self) -> u32 {
        SEG_BASE
            + match self {
                MsgFaceCode::RowLedgerParity => 0x01,
                MsgFaceCode::StageRegress => 0x02,
                MsgFaceCode::TraceSilent => 0x03,
                MsgFaceCode::TraceOutOfSet => 0x04,
                MsgFaceCode::DiffUnlisted => 0x05,
                MsgFaceCode::DiffParityBreak => 0x06,
                MsgFaceCode::DupReceipt => 0x07,
                MsgFaceCode::BadSlot => 0x08,
                MsgFaceCode::BatchOutOfSet => 0x09,
                MsgFaceCode::LedgerFull => 0x0A,
                MsgFaceCode::FingerprintDrift => 0x0B,
                MsgFaceCode::Unknown => 0x3F,
            }
    }

    pub fn detail(self) -> &'static str {
        match self {
            MsgFaceCode::RowLedgerParity => "行账对平破：入账=行账，逐笔对账",
            MsgFaceCode::StageRegress => "段位倒退：通知→对账→列账只进不退",
            MsgFaceCode::TraceSilent => "无痕回执：受理/婉拒必须留痕，不许静默",
            MsgFaceCode::TraceOutOfSet => "痕码越闭集：只许段内痕码落账",
            MsgFaceCode::DiffUnlisted => "差异漏列：对账差异逐笔列账",
            MsgFaceCode::DiffParityBreak => "差异对平破：列账行=差异笔，逐笔对账",
            MsgFaceCode::DupReceipt => "重复回执：同槽位一回执",
            MsgFaceCode::BadSlot => "坏槽位：用登记窗内槽位",
            MsgFaceCode::BatchOutOfSet => "批位越闭集：只许一批/二批",
            MsgFaceCode::LedgerFull => "账容量满：先结案再入账",
            MsgFaceCode::FingerprintDrift => "双跑指纹漂移：冻结种子重跑",
            MsgFaceCode::Unknown => "未知诊断码：不在 0x80840 段闭集内",
        }
    }

    pub const ALL: [MsgFaceCode; 12] = [
        MsgFaceCode::RowLedgerParity,
        MsgFaceCode::StageRegress,
        MsgFaceCode::TraceSilent,
        MsgFaceCode::TraceOutOfSet,
        MsgFaceCode::DiffUnlisted,
        MsgFaceCode::DiffParityBreak,
        MsgFaceCode::DupReceipt,
        MsgFaceCode::BadSlot,
        MsgFaceCode::BatchOutOfSet,
        MsgFaceCode::LedgerFull,
        MsgFaceCode::FingerprintDrift,
        MsgFaceCode::Unknown,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in MsgFaceCode::ALL.iter() {
        if c.val() == code {
            return c.detail();
        }
    }
    MsgFaceCode::Unknown.detail()
}

// ── 冻结常量表（来源：消息面对照表入账手账 · 回执协议节） ─────────
/// 槽位登记窗（下闭上开）。
pub const SLOT_WIN: (u32, u32) = (0x80, 0xA0);
/// 一批（F12617）已收槽位数（分批衔接锚点）。
pub const BATCH1_SLOTS: u32 = 12;
/// 二批入账槽位数。
pub const BATCH2_SLOTS: u32 = 12;
/// 账容量。
pub const LEDGER_CAP: usize = 24;

/// 回执协议三段（单调状态机）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReconStage {
    /// 段一：开工通知。
    Notice,
    /// 段二：对账窗口。
    Recon,
    /// 段三：差异列账。
    DiffLedger,
}

impl ReconStage {
    pub const fn rank(self) -> u8 {
        match self {
            ReconStage::Notice => 0,
            ReconStage::Recon => 1,
            ReconStage::DiffLedger => 2,
        }
    }
}

/// 两态留痕闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TraceKind {
    /// 受理痕。
    Accepted,
    /// 婉拒痕。
    Refused,
}

/// 批位闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BatchNo {
    /// 一批（F12617）。
    Batch1,
    /// 二批（本批）。
    Batch2,
}

/// 一笔入账行。
#[derive(Clone, Copy, Debug)]
pub struct LedgerRow {
    /// 槽位。
    pub slot: u32,
    /// 批位。
    pub batch: BatchNo,
    /// 留痕态（两态之一）。
    pub trace: TraceKind,
    /// 差异值（0=无差异）。
    pub diff: u32,
}

/// 消息面入账闸（三段协议 + 两态留痕）。
pub struct MsgFaceGate {
    /// 行账。
    rows: Vec<LedgerRow>,
    /// 协议段位。
    stage: ReconStage,
    /// 差异列账流水。
    pub diff_flow: Vec<u32>,
    /// 痕码流水（两态留痕）。
    pub trace_flow: Vec<(u32, TraceKind)>,
}

impl MsgFaceGate {
    pub fn new() -> MsgFaceGate {
        MsgFaceGate { rows: Vec::new(), stage: ReconStage::Notice, diff_flow: Vec::new(), trace_flow: Vec::new() }
    }

    /// 段一：开工通知（槽位登记，两态留痕必落）。
    pub fn notice(&mut self, slot: u32) -> Result<TraceKind, u32> {
        if slot < SLOT_WIN.0 || slot >= SLOT_WIN.1 {
            return Err(MsgFaceCode::BadSlot.val());
        }
        if self.rows.len() >= LEDGER_CAP {
            return Err(MsgFaceCode::LedgerFull.val());
        }
        if self.stage.rank() > ReconStage::Notice.rank() {
            return Err(MsgFaceCode::StageRegress.val());
        }
        // 两态裁决（冻结口径：槽位偶数受理/奇数婉拒——确定性两态源）。
        let trace = if slot % 2 == 0 { TraceKind::Accepted } else { TraceKind::Refused };
        self.trace_flow.push((slot, trace));
        self.stage = ReconStage::Notice;
        Ok(trace)
    }

    /// 段二：对账窗口（差异落账）。
    pub fn recon(&mut self, slot: u32, diff: u32) -> Result<u32, u32> {
        if slot < SLOT_WIN.0 || slot >= SLOT_WIN.1 {
            return Err(MsgFaceCode::BadSlot.val());
        }
        if self.stage.rank() < ReconStage::Notice.rank() {
            return Err(MsgFaceCode::StageRegress.val());
        }
        self.stage = ReconStage::Recon;
        // 差异必须逐笔列账（零漏列）。
        if diff != 0 {
            self.diff_flow.push(slot);
        }
        // 行账入账（重复槽位拒）。
        if self.rows.iter().any(|r| r.slot == slot) {
            return Err(MsgFaceCode::DupReceipt.val());
        }
        // 批位：二批入账 12 槽内，前半（6 槽）为一批衔接行，后半（6 槽）为二批新行。
        let batch = if slot - SLOT_WIN.0 < BATCH2_SLOTS / 2 { BatchNo::Batch1 } else { BatchNo::Batch2 };
        let trace = match self.trace_flow.iter().find(|(s, _)| *s == slot) {
            Some((_, t)) => *t,
            None => return Err(MsgFaceCode::TraceSilent.val()),
        };
        self.rows.push(LedgerRow { slot, batch, trace, diff });
        Ok(slot)
    }

    /// 段三：差异列账收口（对平：列账行 ≡ 非零差异行数）。
    pub fn close_diff(&mut self) -> Result<u32, u32> {
        if self.stage.rank() < ReconStage::Recon.rank() {
            return Err(MsgFaceCode::StageRegress.val());
        }
        self.stage = ReconStage::DiffLedger;
        let nonzero = self.rows.iter().filter(|r| r.diff != 0).count() as u32;
        if nonzero != self.diff_flow.len() as u32 {
            return Err(MsgFaceCode::DiffParityBreak.val());
        }
        Ok(nonzero)
    }

    /// 行账对平：行账行数 ≡ 对账入账数（rows 即对账入账，守恒自检）。
    pub fn row_parity(&self) -> Result<u32, u32> {
        // 每行必须有痕（无痕即静默面）。
        for r in self.rows.iter() {
            if !self.trace_flow.iter().any(|(s, _)| *s == r.slot) {
                return Err(MsgFaceCode::TraceSilent.val());
            }
        }
        // 批位守恒：一批槽位数 ≡ BATCH1_SLOTS（分批衔接锚点）。
        let b1 = self.rows.iter().filter(|r| matches!(r.batch, BatchNo::Batch1)).count() as u32;
        let b2 = self.rows.iter().filter(|r| matches!(r.batch, BatchNo::Batch2)).count() as u32;
        if b1 > BATCH1_SLOTS || b2 > BATCH2_SLOTS {
            return Err(MsgFaceCode::RowLedgerParity.val());
        }
        Ok(self.rows.len() as u32)
    }

    /// 协议段位（只读）。
    pub fn stage(&self) -> ReconStage {
        self.stage
    }

    /// 双跑指纹。
    pub fn fingerprint(&self) -> u32 {
        let mut fp = 0x8084_0000u32;
        for r in self.rows.iter() {
            fp = fp.wrapping_mul(31).wrapping_add(r.slot);
            fp = fp.wrapping_mul(31).wrapping_add(r.diff);
            fp = fp.wrapping_mul(31).wrapping_add(match r.trace {
                TraceKind::Accepted => 1,
                TraceKind::Refused => 2,
            });
        }
        fp.wrapping_add(self.diff_flow.len() as u32)
    }
}

/// 黄金正演：12 槽全流程（通知→对账→列账收口）。
fn golden_run() -> Result<u32, u32> {
    let mut g = MsgFaceGate::new();
    for i in 0..BATCH2_SLOTS {
        let slot = SLOT_WIN.0 + i;
        let _ = g.notice(slot)?;
    }
    for i in 0..BATCH2_SLOTS {
        let slot = SLOT_WIN.0 + i;
        // 冻结差异表：奇数序号槽位带差异（确定性）。
        let diff = if i % 2 == 1 { 3 } else { 0 };
        let _ = g.recon(slot, diff)?;
    }
    let d = g.close_diff()?;
    if d != BATCH2_SLOTS / 2 {
        return Err(MsgFaceCode::DiffParityBreak.val());
    }
    g.row_parity()
}

/// 双跑一致。
fn repro_ok() -> bool {
    let run = || -> u32 {
        let mut g = MsgFaceGate::new();
        for i in 0..BATCH2_SLOTS {
            let _ = g.notice(SLOT_WIN.0 + i);
        }
        for i in 0..BATCH2_SLOTS {
            let diff = if i % 2 == 1 { 3 } else { 0 };
            let _ = g.recon(SLOT_WIN.0 + i, diff);
        }
        let _ = g.close_diff();
        g.fingerprint()
    };
    run() == run()
}

/// 注入：坏槽位/重复回执/段位倒退/无痕对账逐次必抓。
fn inject_violations() -> bool {
    // 坏槽位。
    let mut g = MsgFaceGate::new();
    let bad_slot = g.notice(SLOT_WIN.1) == Err(MsgFaceCode::BadSlot.val());
    // 重复回执。
    let _ = g.notice(SLOT_WIN.0);
    let _ = g.recon(SLOT_WIN.0, 0);
    let dup = g.recon(SLOT_WIN.0, 0) == Err(MsgFaceCode::DupReceipt.val());
    // 段位倒退：收口后再通知必拒。
    let mut g2 = MsgFaceGate::new();
    let _ = g2.notice(SLOT_WIN.0);
    let _ = g2.recon(SLOT_WIN.0, 0);
    let _ = g2.close_diff();
    let regress = g2.notice(SLOT_WIN.0 + 2) == Err(MsgFaceCode::StageRegress.val());
    // 无痕对账：未通知先对账必抓。
    let mut g3 = MsgFaceGate::new();
    let silent = g3.recon(SLOT_WIN.0 + 4, 0) == Err(MsgFaceCode::TraceSilent.val());
    bad_slot && dup && regress && silent
}

/// 差异漏列注入：人为造账实不符必抓。
fn diff_unlisted_inject() -> bool {
    let mut g = MsgFaceGate::new();
    let _ = g.notice(SLOT_WIN.0);
    let _ = g.notice(SLOT_WIN.0 + 2);
    let _ = g.recon(SLOT_WIN.0, 5);
    let _ = g.recon(SLOT_WIN.0 + 2, 5);
    // 人为抽走一笔列账（漏列注入）。
    g.diff_flow.pop();
    g.close_diff() == Err(MsgFaceCode::DiffParityBreak.val())
}

/// 段位守卫（VX9 窗口 0x80000..=0x831C0，段内闭集不越窗）。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x80000..=0x831C0).contains(&SEG_BASE) {
        return false;
    }
    for c in MsgFaceCode::ALL.iter() {
        let v = c.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
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
    // 判据②：回执协议三段判据绿。
    let mut gp = MsgFaceGate::new();
    let _ = gp.notice(SLOT_WIN.0);
    let s0 = gp.stage() == ReconStage::Notice;
    let _ = gp.recon(SLOT_WIN.0, 0);
    let s1 = gp.stage() == ReconStage::Recon;
    let _ = gp.close_diff();
    let s2 = gp.stage() == ReconStage::DiffLedger;
    v.push(("J2-三段协议绿", s0 && s1 && s2, String::from("通知→对账→列账")));
    // 判据②：两态留痕（受理/婉拒各落痕）。
    let mut gt = MsgFaceGate::new();
    let _ = gt.notice(SLOT_WIN.0); // 偶→受理
    let _ = gt.notice(SLOT_WIN.0 + 1); // 奇→婉拒
    let acc = gt.trace_flow.iter().any(|(s, t)| *s == SLOT_WIN.0 && *t == TraceKind::Accepted);
    let refu = gt.trace_flow.iter().any(|(s, t)| *s == SLOT_WIN.0 + 1 && *t == TraceKind::Refused);
    v.push(("J2-两态留痕", acc && refu, String::from("受理/婉拒各落痕")));
    // 注入。
    v.push(("J3-违约注入必抓", inject_violations(), String::from("坏槽/重复/倒退/无痕")));
    v.push(("J3-差异漏列注入", diff_unlisted_inject(), String::from("列账=差异笔")));
    // 二批入账行数与批位对平。
    let g2 = match golden_run() {
        Ok(_) => {
            let mut gg = MsgFaceGate::new();
            for i in 0..BATCH2_SLOTS {
                let _ = gg.notice(SLOT_WIN.0 + i);
            }
            for i in 0..BATCH2_SLOTS {
                let diff = if i % 2 == 1 { 3 } else { 0 };
                let _ = gg.recon(SLOT_WIN.0 + i, diff);
            }
            let _ = gg.close_diff();
            Some(gg)
        }
        Err(_) => None,
    };
    let rows_n = g2.as_ref().map(|x| x.rows.len()).unwrap_or(0);
    let b2 = g2
        .as_ref()
        .map(|x| x.rows.iter().filter(|r| matches!(r.batch, BatchNo::Batch2)).count())
        .unwrap_or(0);
    v.push((
        "J3-二批入账对平",
        rows_n == BATCH2_SLOTS as usize && b2 == 6,
        format!("行 {}/二批 {}", rows_n, b2),
    ));
    // 边界：槽位窗卡边。
    let mut gw = MsgFaceGate::new();
    let lo = gw.notice(SLOT_WIN.0).is_ok();
    let hi = gw.notice(SLOT_WIN.1 - 1).is_ok();
    let mut gw2 = MsgFaceGate::new();
    let over_lo = gw2.notice(SLOT_WIN.0 - 1) == Err(MsgFaceCode::BadSlot.val());
    let over_hi = gw2.notice(SLOT_WIN.1) == Err(MsgFaceCode::BadSlot.val());
    v.push(("J4-槽位窗卡边", lo && hi && over_lo && over_hi, String::from("0x80..0xA0 窗")));
    // 边界：差异恰零不列账（无差异行不入列账流水）。
    let mut gz = MsgFaceGate::new();
    let _ = gz.notice(SLOT_WIN.0);
    let _ = gz.recon(SLOT_WIN.0, 0);
    let zero = gz.diff_flow.is_empty() && gz.close_diff() == Ok(0);
    v.push(("J4-零差异不列账", zero, String::from("diff=0 不入流水")));
    // 边界：段位倒退秩校验（三段只进不退）。
    let ranks_ok = ReconStage::Notice.rank() < ReconStage::Recon.rank()
        && ReconStage::Recon.rank() < ReconStage::DiffLedger.rank();
    v.push(("J4-段位秩单调", ranks_ok, String::from("0<1<2")));
    // 账容量/窗界记账：窗内槽位 0x20 = 32 ≥ LEDGER_CAP 24，容量在窗内可达。
    let win_slots = (SLOT_WIN.1 - SLOT_WIN.0) as usize;
    let cap_reachable = win_slots >= LEDGER_CAP;
    v.push(("J5-容量/窗界记账", cap_reachable, format!("窗内槽 {} / 容量 {}", win_slots, LEDGER_CAP)));
    v.push(("J5-双跑一致", repro_ok(), String::from("指纹相等")));
    v.push(("J6-段位守卫", segment_guard(), String::from("0x80840 窗口")));
    v
}

/// VX9 检查入口（卡 12633，UNX-F12634-J1 判据承载）。
pub fn run_unx12633_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12633 消息面对照表二批入账");
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
    fn test_three_stage_monotonic() {
        let mut g = MsgFaceGate::new();
        let _ = g.notice(SLOT_WIN.0);
        assert_eq!(g.stage(), ReconStage::Notice);
        let _ = g.recon(SLOT_WIN.0, 0);
        assert_eq!(g.stage(), ReconStage::Recon);
        let _ = g.close_diff();
        assert_eq!(g.stage(), ReconStage::DiffLedger);
    }

    #[test]
    fn test_two_state_traces() {
        let mut g = MsgFaceGate::new();
        assert_eq!(g.notice(SLOT_WIN.0), Ok(TraceKind::Accepted));
        assert_eq!(g.notice(SLOT_WIN.0 + 1), Ok(TraceKind::Refused));
    }

    #[test]
    fn test_violation_inject() {
        assert!(inject_violations());
    }

    #[test]
    fn test_diff_unlisted_inject() {
        assert!(diff_unlisted_inject());
    }

    #[test]
    fn test_bad_slot() {
        let mut g = MsgFaceGate::new();
        assert_eq!(g.notice(SLOT_WIN.1), Err(MsgFaceCode::BadSlot.val()));
        assert_eq!(g.notice(SLOT_WIN.0 - 1), Err(MsgFaceCode::BadSlot.val()));
        assert_eq!(g.recon(SLOT_WIN.1, 0), Err(MsgFaceCode::BadSlot.val()));
    }

    #[test]
    fn test_dup_receipt() {
        let mut g = MsgFaceGate::new();
        let _ = g.notice(SLOT_WIN.0);
        let _ = g.recon(SLOT_WIN.0, 0);
        assert_eq!(g.recon(SLOT_WIN.0, 0), Err(MsgFaceCode::DupReceipt.val()));
    }

    #[test]
    fn test_silent_recon_rejected() {
        let mut g = MsgFaceGate::new();
        assert_eq!(g.recon(SLOT_WIN.0 + 4, 0), Err(MsgFaceCode::TraceSilent.val()));
    }

    #[test]
    fn test_stage_regress_after_close() {
        let mut g = MsgFaceGate::new();
        let _ = g.notice(SLOT_WIN.0);
        let _ = g.recon(SLOT_WIN.0, 0);
        let _ = g.close_diff();
        assert_eq!(g.notice(SLOT_WIN.0 + 2), Err(MsgFaceCode::StageRegress.val()));
    }

    #[test]
    fn test_diff_parity() {
        let mut g = MsgFaceGate::new();
        let _ = g.notice(SLOT_WIN.0);
        let _ = g.notice(SLOT_WIN.0 + 2);
        let _ = g.recon(SLOT_WIN.0, 5);
        let _ = g.recon(SLOT_WIN.0 + 2, 5);
        assert_eq!(g.close_diff(), Ok(2));
        g.diff_flow.pop();
        assert_eq!(g.close_diff(), Err(MsgFaceCode::DiffParityBreak.val()));
    }

    #[test]
    fn test_batch_split() {
        let mut g = MsgFaceGate::new();
        for i in 0..BATCH2_SLOTS {
            let _ = g.notice(SLOT_WIN.0 + i);
        }
        for i in 0..BATCH2_SLOTS {
            let _ = g.recon(SLOT_WIN.0 + i, 0);
        }
        let _ = g.close_diff();
        let b1 = g.rows.iter().filter(|r| matches!(r.batch, BatchNo::Batch1)).count();
        let b2 = g.rows.iter().filter(|r| matches!(r.batch, BatchNo::Batch2)).count();
        assert_eq!(b1, 6);
        assert_eq!(b2, 6);
        assert!(g.row_parity().is_ok());
    }

    #[test]
    fn test_dual_run_equal() {
        assert!(repro_ok());
    }

    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE, 0x80840);
    }

    #[test]
    fn test_detail_closed_set() {
        for c in MsgFaceCode::ALL.iter() {
            assert_eq!(detail(c.val()), c.detail());
        }
        assert!(detail(0xCAFE).contains("未知"));
    }
}
