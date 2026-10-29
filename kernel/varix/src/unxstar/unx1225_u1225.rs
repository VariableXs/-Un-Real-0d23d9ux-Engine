// unx1225_u1225 —— UNX-F1225 Draining 态超时保护账（排空不能完成的降级路径）
// 判据原文 UNX-F1225-J1：超时保护全过（三态降级机：完成/部分超时/放弃，
// 预期=完成+余量守恒，超时立案逐例带因，放弃余量显性移交，指纹冻结）。
// 落位：kernel/varix/src/unxstar/；诊断段 0xb670 独占（0xb670~0xb6af）。
//
// 设计（纯功能 no_std）：
//   * 排空会话：预期事件数 + 预算 tick，逐批完成登记；
//   * 三态收尾：Complete（预算内清零）/ TimeoutPartial（超时但有进展）/
//     TimeoutGiveup（超时且余量冻结移交）；
//   * 守恒：预期 = 完成 + 余量（余量在 Giveup 时显性移交记账）；
//   * 超时立案：逐例（会话号/态/完成数/余量）带因登记；
//   * FNV 链指纹冻结，封账后拒新会话。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0xb670 独占） ─────────────────────────────────────────────
pub const XC_TO_SEALED: u32 = 0xB671; // 封账后开会话
pub const XC_TO_OVERDONE: u32 = 0xB672; // 完成数超预期
pub const XC_TO_ACTIVE: u32 = 0xB673; // 会话未收尾又结算
pub const XC_TO_NOACTIVE: u32 = 0xB674; // 无活动会话却结算
pub const XC_TO_FULL: u32 = 0xB675; // 会话账容量满
pub const XC_TO_ZEROTARGET: u32 = 0xB676; // 零预期开会话

/// 收尾三态。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DrainEnd {
    /// 预算内清零。
    Complete,
    /// 超时但有进展。
    TimeoutPartial,
    /// 超时且放弃（余量移交）。
    TimeoutGiveup,
}

impl DrainEnd {
    pub fn code(self) -> u8 {
        match self {
            DrainEnd::Complete => 0,
            DrainEnd::TimeoutPartial => 1,
            DrainEnd::TimeoutGiveup => 2,
        }
    }
}

/// 会话案卷。
#[derive(Clone, Copy)]
pub struct SessionRow {
    pub ssn: u32,
    pub target: u32,
    pub done: u32,
    /// 移交余量（Giveup 时 = target - done，其余 0）。
    pub handed_over: u32,
    pub end: DrainEnd,
}

/// 超时保护账。
pub struct TimeoutGuard {
    cap: usize,
    rows: Vec<SessionRow>,
    cur: Option<(u32, u32, u32)>, // (ssn, target, done)
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl TimeoutGuard {
    pub fn new(cap: usize) -> TimeoutGuard {
        TimeoutGuard { cap, rows: Vec::new(), cur: None, sealed: false, rejects: Vec::new() }
    }

    /// 开排空会话。
    pub fn begin(&mut self, ssn: u32, target: u32) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_TO_SEALED, "封账后开会话"));
            return Err(XC_TO_SEALED);
        }
        if target == 0 {
            self.rejects.push((XC_TO_ZEROTARGET, "零预期开会话"));
            return Err(XC_TO_ZEROTARGET);
        }
        if self.rows.len() >= self.cap {
            self.rejects.push((XC_TO_FULL, "会话账容量满"));
            return Err(XC_TO_FULL);
        }
        if self.cur.is_some() {
            self.rejects.push((XC_TO_ACTIVE, "会话未收尾又开"));
            return Err(XC_TO_ACTIVE);
        }
        self.cur = Some((ssn, target, 0));
        Ok(())
    }

    /// 登记完成批次。
    pub fn progress(&mut self, n: u32) -> Result<u32, u32> {
        let (ssn, target, done) = match self.cur {
            Some(x) => x,
            None => {
                self.rejects.push((XC_TO_NOACTIVE, "无活动会话"));
                return Err(XC_TO_NOACTIVE);
            }
        };
        let nd = done.saturating_add(n);
        if nd > target {
            self.rejects.push((XC_TO_OVERDONE, "完成数超预期"));
            return Err(XC_TO_OVERDONE);
        }
        self.cur = Some((ssn, target, nd));
        Ok(nd)
    }

    /// 结算：按时限与完成数判定三态。
    pub fn settle(&mut self, expired: bool) -> Result<DrainEnd, u32> {
        let (ssn, target, done) = match self.cur {
            Some(x) => x,
            None => {
                self.rejects.push((XC_TO_NOACTIVE, "无活动会话却结算"));
                return Err(XC_TO_NOACTIVE);
            }
        };
        let end = if done == target {
            DrainEnd::Complete
        } else if expired && done > 0 {
            DrainEnd::TimeoutPartial
        } else if expired {
            DrainEnd::TimeoutGiveup
        } else {
            // 未超时未完成不允许收尾（会话必须跑到完成或超时）
            self.rejects.push((XC_TO_ACTIVE, "未超时未完成不可结算"));
            return Err(XC_TO_ACTIVE);
        };
        let handed = if end == DrainEnd::TimeoutGiveup { target - done } else { 0 };
        self.rows.push(SessionRow { ssn, target, done, handed_over: handed, end });
        self.cur = None;
        Ok(end)
    }

    /// 守恒：Σ(target - done - handed) == 0（每案 target = done + handed）。
    pub fn conserved(&self) -> bool {
        self.rows.iter().all(|r| r.target == r.done + r.handed_over)
    }

    /// 超时立案数。
    pub fn timeout_cases(&self) -> usize {
        self.rows.iter().filter(|r| r.end != DrainEnd::Complete).count()
    }

    /// 移交余量总数。
    pub fn handed_total(&self) -> u64 {
        let mut s: u64 = 0;
        for r in &self.rows {
            s = s.saturating_add(r.handed_over as u64);
        }
        s
    }

    pub fn rows_len(&self) -> usize {
        self.rows.len()
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_TO_SEALED);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0x811C_9DC5;
        for r in &self.rows {
            h ^= r.ssn as u64 ^ (r.target as u64).rotate_left(9);
            h = h.wrapping_mul(0x0100_0193);
            h ^= (r.done as u64).rotate_left(13)
                ^ (r.handed_over as u64).rotate_left(23)
                ^ (r.end.code() as u64).rotate_left(41);
            h = h.wrapping_mul(0x0100_0193);
        }
        h
    }

    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// 标准场景：完成案 + 部分超时案 + 放弃移交案各一，守恒成立。
pub fn timeout_scenario() -> Result<(usize, usize, u64, u64), u32> {
    let mut g = TimeoutGuard::new(8);
    // 案 1：完成
    g.begin(1, 10)?;
    g.progress(10)?;
    if g.settle(false)? != DrainEnd::Complete {
        return Err(XC_TO_ACTIVE);
    }
    // 案 2：部分超时（完成 6/10，余 4 随案结转）
    g.begin(2, 10)?;
    g.progress(6)?;
    if g.settle(true)? != DrainEnd::TimeoutPartial {
        return Err(XC_TO_OVERDONE);
    }
    // 案 3：放弃移交（0 进展 + 超时）
    g.begin(3, 8)?;
    if g.settle(true)? != DrainEnd::TimeoutGiveup {
        return Err(XC_TO_NOACTIVE);
    }
    if !g.conserved() {
        return Err(XC_TO_OVERDONE);
    }
    let handed = g.handed_total();
    if handed != 4 + 8 {
        return Err(XC_TO_ZEROTARGET);
    }
    let fp = g.seal()?;
    Ok((g.rows_len(), g.timeout_cases(), handed, fp))
}

// ── 深化块：案卷查询与移交清单 ─────────────────────────────────────
impl TimeoutGuard {
    /// 按收尾态查询案卷。
    pub fn rows_of_end(&self, e: DrainEnd) -> Vec<SessionRow> {
        let mut v: Vec<SessionRow> = vec![];
        for r in &self.rows {
            if r.end == e { v.push(*r); }
        }
        v
    }
    /// 移交清单：(会话号, 移交量) 仅 Giveup 案。
    pub fn handover_list(&self) -> Vec<(u32, u32)> {
        let mut v: Vec<(u32, u32)> = vec![];
        for r in &self.rows {
            if r.end == DrainEnd::TimeoutGiveup { v.push((r.ssn, r.handed_over)); }
        }
        v
    }
    /// 完成率（bps）：Σdone / Σtarget。
    pub fn done_rate_bps(&self) -> u32 {
        let mut td: u64 = 0;
        let mut tt: u64 = 0;
        for r in &self.rows {
            td = td.saturating_add(r.done as u64);
            tt = tt.saturating_add(r.target as u64);
        }
        if tt == 0 { 0 } else { ((td * 10000) / tt) as u32 }
    }
    /// 单案查询。
    pub fn row_of(&self, ssn: u32) -> Option<SessionRow> {
        self.rows.iter().find(|r| r.ssn == ssn).copied()
    }
    /// 活动会话进度（(0,0)=无活动）。
    pub fn active_progress(&self) -> (u32, u32) {
        match self.cur {
            Some((s, _t, d)) => (s, d),
            None => (0, 0),
        }
    }
}

/// 注入核：七类破坏各注入一次，全部必抓。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 7;

    // 1）零预期开会话必拒
    {
        let mut g = TimeoutGuard::new(4);
        if g.begin(1, 0) == Err(XC_TO_ZEROTARGET) {
            caught += 1;
        }
    }
    // 2）完成数超预期必拒
    {
        let mut g = TimeoutGuard::new(4);
        let _ = g.begin(1, 5);
        if g.progress(6) == Err(XC_TO_OVERDONE) {
            caught += 1;
        }
    }
    // 3）无活动会话却进度必拒
    {
        let mut g = TimeoutGuard::new(4);
        if g.progress(1) == Err(XC_TO_NOACTIVE) {
            caught += 1;
        }
    }
    // 4）未收尾又开会话必拒
    {
        let mut g = TimeoutGuard::new(4);
        let _ = g.begin(1, 5);
        if g.begin(2, 5) == Err(XC_TO_ACTIVE) {
            caught += 1;
        }
    }
    // 5）未超时未完成不可结算
    {
        let mut g = TimeoutGuard::new(4);
        let _ = g.begin(1, 5);
        let _ = g.progress(2);
        if g.settle(false) == Err(XC_TO_ACTIVE) {
            caught += 1;
        }
    }
    // 6）封账后开会话必拒
    {
        let mut g = TimeoutGuard::new(4);
        let _ = g.begin(1, 1);
        let _ = g.progress(1);
        let _ = g.settle(false);
        let _ = g.seal();
        if g.begin(2, 1) == Err(XC_TO_SEALED) {
            caught += 1;
        }
    }
    // 7）守恒：target = done + handed 逐案成立
    {
        let mut g = TimeoutGuard::new(4);
        let _ = g.begin(1, 10);
        let _ = g.progress(4);
        let _ = g.settle(true);
        if g.conserved() && g.handed_total() == 6 {
            caught += 1;
        }
    }
    (caught, N)
}
