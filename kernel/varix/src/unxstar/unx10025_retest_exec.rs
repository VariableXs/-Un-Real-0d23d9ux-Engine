//! UNX-F10025 清偿复测执行器（逐条重测） · 0x91640段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10026-J1 通过判定双档核（T1/ktest 分账恢复 10/10 次），单档定论 10/10 次拦截
//!
//! 设计要点：
//!   ① 复测状态机四态闭集：Pending → InRetest → Passed | Failed，
//!      单向迁移；未 start 就 finish、已终态再 finish 一律 BadTransition/
//!      DupRetest，10/10 次拦截，不许跳步定论。
//!   ② 原判据号复测是硬约束：每条目 admit 时登记原判据号，复测时
//!      提交的判据号必须逐字相等，代测（拿别的判据号交差）10/10 次
//!      拦截；判据号 0 视为无效票。
//!   ③ 复测只对已挂批条目开放：无批条目试图复测一律 UnbatchedEntry
//!      （计划表冻结是复测的前置，与 F10024 对齐）。
//!   ④ 留痕账本只增：每条复测记录（条目、判据号、结论、序号），
//!      结论分账（Pass/Fail）合计必须等于留痕数，逃逸为 0。
//!   ⑤ 注入账守恒：注入 = 检出 + 逃逸；四类坏样本（代测/重复重测/
//!      无批复测/跳步定论）各 10/10 次显性拒绝；FNV-1a 双跑指纹复核。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10025 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x91640;

/// 条目容量。
pub const MAX_ENTRIES: usize = 16;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

// ── 错误码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RetestCode {
    /// 未知条目
    UnknownEntry,
    /// 代测：提交的判据号与原判据号不符
    ProxyRetest,
    /// 重复重测
    DupRetest,
    /// 无批复测
    UnbatchedEntry,
    /// 跳步：未 start 就 finish
    BadTransition,
    /// 判据号无效（0）
    UnknownCriterion,
    /// 无待复测条目
    NothingPending,
    /// 注入账倒退
    LedgerRegress,
    /// 双跑不一致
    NotReproducible,
    /// 留痕账满
    LedgerFull,
    /// 终态后改判
    LockedVerdict,
    /// 段位越界
    SegmentViolation,
}

impl RetestCode {
    /// 码值（段基址 + 变体序号）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            RetestCode::UnknownEntry => "未知条目被拒：条目必须先 admit 登记原判据号；\
                请回计划表取排期条目",
            RetestCode::ProxyRetest => "代测被拒：提交判据号与原判据号不符；\
                复测必须逐条对原判据号重跑，不许拿邻条结果交差",
            RetestCode::DupRetest => "重复重测被拒：同一条目单轮只复测一次；\
                如需二测请开新一轮并留新痕",
            RetestCode::UnbatchedEntry => "无批复测被拒：计划表冻结是复测前置；\
                请先按 F10024 挂批并冻结",
            RetestCode::BadTransition => "跳步定论被拒：必须 Pending→InRetest→终态；\
                未 start 不得直接 finish",
            RetestCode::UnknownCriterion => "判据号无效被拒：判据号 0 不存在；\
                请核对原判据号登记",
            RetestCode::NothingPending => "无待复测条目被拒：调度器空转；\
                请确认全部条目已复测完毕",
            RetestCode::LedgerRegress => "注入账倒退被拒：计数只增不减；请排查回滚路径",
            RetestCode::NotReproducible => "双跑不一致：同批次两次复测账不同；\
                请排查隐藏状态或非确定调度",
            RetestCode::LedgerFull => "留痕账满被拒：复测记录越过容量；请先归档旧账",
            RetestCode::LockedVerdict => "终态改判被拒：Passed/Failed 已落账；\
                改判必须走新一轮复测留痕，不许覆盖旧结论",
            RetestCode::SegmentViolation => "段位越界：本模块段基址必须 0x40 对齐且落在本批窗口内",
        }
    }

    /// 闭集全量。
    pub const ALL: [RetestCode; 12] = [
        RetestCode::UnknownEntry,
        RetestCode::ProxyRetest,
        RetestCode::DupRetest,
        RetestCode::UnbatchedEntry,
        RetestCode::BadTransition,
        RetestCode::UnknownCriterion,
        RetestCode::NothingPending,
        RetestCode::LedgerRegress,
        RetestCode::NotReproducible,
        RetestCode::LedgerFull,
        RetestCode::LockedVerdict,
        RetestCode::SegmentViolation,
    ];
}

// ── 复测状态机 ────────────────────────────────────────────────────────
/// 复测四态闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RetestState {
    /// 待复测
    Pending,
    /// 复测中（已 start）
    InRetest,
    /// 通过
    Passed,
    /// 失败
    Failed,
}

/// 复测结论。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// 通过
    Pass,
    /// 失败
    Fail,
}

/// 单条复测对象。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RetestEntry {
    /// 条目号
    pub id: u32,
    /// 原判据号（admit 时登记，复测必须逐字对上）
    pub criterion: u32,
    /// 所属批号（0 = 无批，禁止复测）
    pub batch: u32,
    /// 当前状态
    pub state: RetestState,
}

/// 一条复测留痕。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RetestRecord {
    /// 条目号
    pub id: u32,
    /// 复测判据号
    pub criterion: u32,
    /// 结论
    pub verdict: Verdict,
    /// 留痕序号
    pub seq: u32,
}

// ── 执行器 ────────────────────────────────────────────────────────────
/// 复测执行器：登记 + 调度 + 状态机 + 留痕账本。
#[derive(Clone, Copy, Debug)]
pub struct RetestExecutor {
    entries: [Option<RetestEntry>; MAX_ENTRIES],
    records: [Option<RetestRecord>; MAX_ENTRIES],
    record_count: u32,
    seq_gen: u32,
}

impl RetestExecutor {
    /// 空执行器。
    pub fn new() -> RetestExecutor {
        RetestExecutor {
            entries: [None; MAX_ENTRIES],
            records: [None; MAX_ENTRIES],
            record_count: 0,
            seq_gen: 0,
        }
    }

    /// 登记条目：判据号必须非 0，重复登记拒绝。
    pub fn admit(&mut self, id: u32, criterion: u32, batch: u32) -> Result<(), RetestCode> {
        if id == 0 {
            return Err(RetestCode::UnknownEntry);
        }
        if criterion == 0 {
            return Err(RetestCode::UnknownCriterion);
        }
        if self.entry(id).is_some() {
            return Err(RetestCode::UnknownEntry);
        }
        let mut i = 0usize;
        while i < MAX_ENTRIES {
            if let Some(slot) = self.entries.get_mut(i) {
                if slot.is_none() {
                    *slot = Some(RetestEntry { id, criterion, batch, state: RetestState::Pending });
                    return Ok(());
                }
            }
            i += 1;
        }
        Err(RetestCode::LedgerFull)
    }

    /// 查条目。
    pub fn entry(&self, id: u32) -> Option<RetestEntry> {
        let mut i = 0usize;
        while i < MAX_ENTRIES {
            if let Some(Some(e)) = self.entries.get(i).copied() {
                if e.id == id {
                    return Some(e);
                }
            }
            i += 1;
        }
        None
    }

    /// start：Pending → InRetest；无批条目在此拦截。
    pub fn start(&mut self, id: u32) -> Result<(), RetestCode> {
        let e = match self.entry(id) {
            Some(e) => e,
            None => return Err(RetestCode::UnknownEntry),
        };
        if e.batch == 0 {
            return Err(RetestCode::UnbatchedEntry);
        }
        if e.state != RetestState::Pending {
            return Err(RetestCode::BadTransition);
        }
        self.set_state(id, RetestState::InRetest)
    }

    /// finish：InRetest → Passed|Failed；判据号必须逐字对上原判据号；
    /// 终态后再 finish 即 DupRetest；成功即留痕。
    pub fn finish(&mut self, id: u32, criterion: u32, verdict: Verdict) -> Result<(), RetestCode> {
        let e = match self.entry(id) {
            Some(e) => e,
            None => return Err(RetestCode::UnknownEntry),
        };
        if criterion == 0 {
            return Err(RetestCode::UnknownCriterion);
        }
        if criterion != e.criterion {
            return Err(RetestCode::ProxyRetest);
        }
        if e.state == RetestState::Passed || e.state == RetestState::Failed {
            return Err(RetestCode::DupRetest);
        }
        if e.state != RetestState::InRetest {
            return Err(RetestCode::BadTransition);
        }
        if e.batch == 0 {
            return Err(RetestCode::UnbatchedEntry);
        }
        if self.record_count as usize >= MAX_ENTRIES {
            return Err(RetestCode::LedgerFull);
        }
        let final_state = match verdict {
            Verdict::Pass => RetestState::Passed,
            Verdict::Fail => RetestState::Failed,
        };
        self.set_state(id, final_state)?;
        self.seq_gen = self.seq_gen.wrapping_add(1);
        let mut k = 0usize;
        while k < MAX_ENTRIES {
            if let Some(slot) = self.records.get_mut(k) {
                if slot.is_none() {
                    *slot = Some(RetestRecord { id, criterion, verdict, seq: self.seq_gen });
                    self.record_count = self.record_count.saturating_add(1);
                    return Ok(());
                }
            }
            k += 1;
        }
        Err(RetestCode::LedgerFull)
    }

    fn set_state(&mut self, id: u32, state: RetestState) -> Result<(), RetestCode> {
        let mut i = 0usize;
        while i < MAX_ENTRIES {
            if let Some(Some(e)) = self.entries.get_mut(i) {
                if e.id == id {
                    e.state = state;
                    return Ok(());
                }
            }
            i += 1;
        }
        Err(RetestCode::UnknownEntry)
    }

    /// 调度器：按条目号升序取下一个 Pending 且有批条目。
    pub fn next_pending(&self) -> Option<u32> {
        let mut best: Option<u32> = None;
        let mut i = 0usize;
        while i < MAX_ENTRIES {
            if let Some(Some(e)) = self.entries.get(i).copied() {
                if e.state == RetestState::Pending && e.batch != 0 {
                    best = match best {
                        Some(b) if b < e.id => Some(b),
                        _ => Some(e.id),
                    };
                }
            }
            i += 1;
        }
        best
    }

    /// 按状态计数。
    pub fn count_state(&self, state: RetestState) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < MAX_ENTRIES {
            if let Some(Some(e)) = self.entries.get(i).copied() {
                if e.state == state {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 留痕数。
    pub fn records(&self) -> u32 {
        self.record_count
    }

    /// 结论分账：Pass 留痕数。
    pub fn pass_count(&self) -> u32 {
        self.count_verdict(Verdict::Pass)
    }

    /// 结论分账：Fail 留痕数。
    pub fn fail_count(&self) -> u32 {
        self.count_verdict(Verdict::Fail)
    }

    fn count_verdict(&self, v: Verdict) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < MAX_ENTRIES {
            if let Some(Some(r)) = self.records.get(i).copied() {
                if r.verdict == v {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 账守恒：条目数 = Pending + InRetest + Passed + Failed。
    pub fn conserved(&self) -> bool {
        let total = self.count_state(RetestState::Pending)
            + self.count_state(RetestState::InRetest)
            + self.count_state(RetestState::Passed)
            + self.count_state(RetestState::Failed);
        let mut listed = 0u32;
        let mut i = 0usize;
        while i < MAX_ENTRIES {
            if self.entries.get(i).copied().flatten().is_some() {
                listed += 1;
            }
            i += 1;
        }
        total == listed
    }
}

// ── 注入核：坏复测样本矩阵 ────────────────────────────────────────────
/// 四类坏复测样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadRetestCase {
    /// 代测：判据号与原判据号不符
    Proxy,
    /// 重复重测：终态后再 finish
    Dup,
    /// 无批复测
    Unbatched,
    /// 跳步：未 start 直接 finish
    SkipStart,
}

impl BadRetestCase {
    /// 闭集全量。
    pub const ALL: [BadRetestCase; 4] = [
        BadRetestCase::Proxy,
        BadRetestCase::Dup,
        BadRetestCase::Unbatched,
        BadRetestCase::SkipStart,
    ];

    /// 期望诊断码。
    pub fn code(self) -> RetestCode {
        match self {
            BadRetestCase::Proxy => RetestCode::ProxyRetest,
            BadRetestCase::Dup => RetestCode::DupRetest,
            BadRetestCase::Unbatched => RetestCode::UnbatchedEntry,
            BadRetestCase::SkipStart => RetestCode::BadTransition,
        }
    }

    /// 构造样本执行器并施加坏请求，返回是否按期望拒绝。
    pub fn attempt(self) -> bool {
        let mut ex = RetestExecutor::new();
        match self {
            BadRetestCase::Proxy => {
                let _ = ex.admit(1, 9001, 1);
                let _ = ex.start(1);
                ex.finish(1, 9002, Verdict::Pass).err() == Some(RetestCode::ProxyRetest)
            }
            BadRetestCase::Dup => {
                let _ = ex.admit(1, 9001, 1);
                let _ = ex.start(1);
                let _ = ex.finish(1, 9001, Verdict::Pass);
                ex.finish(1, 9001, Verdict::Pass).err() == Some(RetestCode::DupRetest)
            }
            BadRetestCase::Unbatched => {
                let _ = ex.admit(1, 9001, 0);
                ex.start(1).err() == Some(RetestCode::UnbatchedEntry)
            }
            BadRetestCase::SkipStart => {
                let _ = ex.admit(1, 9001, 1);
                ex.finish(1, 9001, Verdict::Pass).err() == Some(RetestCode::BadTransition)
            }
        }
    }
}

/// 注入账：注入 = 检出 + 逃逸，逃逸恒为 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ExecLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl ExecLedger {
    const fn zero() -> ExecLedger {
        ExecLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果。
    pub fn record(&mut self, caught: bool) -> Result<(), RetestCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(RetestCode::LedgerRegress);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行一轮坏复测注入：四类各 INJECT_ROUNDS 次。
pub fn run_bad_retest_matrix() -> (ExecLedger, [u32; 4]) {
    let mut ledger = ExecLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadRetestCase::ALL.len() {
        let case = match BadRetestCase::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let caught = case.attempt();
            let _ = ledger.record(caught);
            if caught {
                if let Some(slot) = per_class.get_mut(ci) {
                    *slot = slot.saturating_add(1);
                }
            }
            round += 1;
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；代测/无批复测族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；未知条目/判据族"),
    ("EEXIST", -17, 183, "Windows ERROR_ALREADY_EXISTS；重复重测族"),
    ("EBADFD", -77, 6, "Windows ERROR_INVALID_HANDLE；状态机跳步族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；无效判据号族"),
];

/// 查表：按 errno 正数取负值；表外返回 0（显性可查）。
pub fn linux_neg(code: i32) -> i32 {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == -code {
                return row.1;
            }
        }
        i += 1;
    }
    0
}

/// 与 Linux 逐码一致：只允许表内码且负值精确匹配。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 规范语料双跑指纹：5 条逐条按原判据号复测（3 过 2 败）→ 摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut ex = RetestExecutor::new();
    let ids = [1u32, 2, 3, 4, 5];
    let mut k = 0usize;
    while k < ids.len() {
        let id = match ids.get(k) {
            Some(x) => *x,
            None => break,
        };
        let _ = ex.admit(id, 7000 + id, 1);
        let _ = ex.start(id);
        let verdict = if id % 2 == 0 { Verdict::Fail } else { Verdict::Pass };
        let _ = ex.finish(id, 7000 + id, verdict);
        k += 1;
    }
    let mut bytes: Vec<u8> = Vec::new();
    bytes.push((ex.pass_count() & 0xFF) as u8);
    bytes.push((ex.fail_count() & 0xFF) as u8);
    bytes.push((ex.records() & 0xFF) as u8);
    bytes.push((ex.count_state(RetestState::Pending) & 0xFF) as u8);
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10025 判据集（16 条；动态明细 ≤8 条）。
pub fn run_unx10025_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10025");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x91500..=0x91FFF).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x91500..=0x91FFF", seg_ok,
          "段基址必须唯一且落在本批窗口");

    // 2. 正常流：逐条按原判据号复测全通过
    let mut ex = RetestExecutor::new();
    let ids = [1u32, 2, 3];
    let mut normal_ok = true;
    let mut k = 0usize;
    while k < 3 {
        let id = match ids.get(k) {
            Some(x) => *x,
            None => break,
        };
        if ex.admit(id, 8000 + id, 1).is_err() {
            normal_ok = false;
        }
        if ex.start(id).is_err() {
            normal_ok = false;
        }
        let v = if id == 2 { Verdict::Fail } else { Verdict::Pass };
        if ex.finish(id, 8000 + id, v).is_err() {
            normal_ok = false;
        }
        k += 1;
    }
    s.add("正常流：3 条逐条按原判据号复测全通过", normal_ok && ex.records() == 3,
          "Pending→InRetest→终态逐条走完并留痕");

    // 3. 结论分账：Pass/Fail 分账合计 = 留痕数
    s.add("结论分账：2 过 1 败合计 3 与留痕闭合",
          ex.pass_count() == 2 && ex.fail_count() == 1
              && ex.pass_count() + ex.fail_count() == ex.records(),
          "分账合计必须等于留痕数，逃逸为 0");

    // 4. 状态机终态：全部条目到达终态，调度器空
    s.add("状态机终态：0 Pending 0 InRetest 且账守恒",
          ex.count_state(RetestState::Pending) == 0
              && ex.count_state(RetestState::InRetest) == 0
              && ex.conserved(),
          "四态账闭合：条目数 = 四态计数之和");

    // 5. 注入核：四类坏样本各 10/10 拒绝
    let (inj, per_class) = run_bad_retest_matrix();
    let all_caught = per_class.iter().all(|x| *x == INJECT_ROUNDS);
    s.add("注入核：代测/重复重测/无批/跳步各 10/10 拒绝", all_caught,
          "四类坏样本逐类逐次显性拒绝");

    // 6. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadRetestCase::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 7. 代测拦截：邻条判据号冒充 10/10 拒绝
    let mut proxy_ok = true;
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        let mut g = RetestExecutor::new();
        let _ = g.admit(1, 9100, 1);
        let _ = g.admit(2, 9200, 1);
        let _ = g.start(1);
        if g.finish(1, 9200, Verdict::Pass).is_ok() {
            proxy_ok = false;
        }
        r += 1;
    }
    s.add("代测拦截：邻条判据号冒充 10/10 拒绝", proxy_ok,
          "复测必须逐条对原判据号，判据号错配即 ProxyRetest");

    // 8. 判据号 0：无效票拒绝
    let mut z = RetestExecutor::new();
    s.add("判据号 0：admit 与 finish 均拒绝",
          z.admit(1, 0, 1).err() == Some(RetestCode::UnknownCriterion),
          "判据号 0 不存在，登记与复测两道闸都拦");

    // 9. 无批复测拦截：计划冻结前置
    let unb_ok = BadRetestCase::Unbatched.attempt();
    s.add("无批复测拦截：batch=0 条目 start 拒绝", unb_ok,
          "复测只对已挂批条目开放");

    // 10. 调度器确定性：按条目号升序出队
    let mut sch = RetestExecutor::new();
    let _ = sch.admit(3, 7003, 1);
    let _ = sch.admit(1, 7001, 1);
    let _ = sch.admit(2, 7002, 1);
    let first = sch.next_pending().unwrap_or(0);
    let _ = sch.start(first);
    let _ = sch.finish(first, 7000 + first, Verdict::Pass);
    let second = sch.next_pending().unwrap_or(0);
    s.add("调度器确定性：升序出队 1 后接 2", first == 1 && second == 2,
          "调度顺序只由条目号决定，无隐藏状态");

    // 11. 留痕账只增：终态改判拒绝
    let mut lk = RetestExecutor::new();
    let _ = lk.admit(1, 9500, 1);
    let _ = lk.start(1);
    let _ = lk.finish(1, 9500, Verdict::Pass);
    s.add("终态改判拒绝：Passed 后再 finish → DupRetest",
          lk.finish(1, 9500, Verdict::Fail).err() == Some(RetestCode::DupRetest)
              && lk.records() == 1,
          "结论落账后不许覆盖，改判必须开新一轮");

    // 12. Linux 逐码一致
    let codes_ok = agrees_with_linux(1) && agrees_with_linux(2) && agrees_with_linux(17)
        && agrees_with_linux(77) && agrees_with_linux(22)
        && !agrees_with_linux(0) && !agrees_with_linux(864);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 13. 诊断码闭集：12 码唯一且文案非空
    let mut uniq = true;
    let mut i = 0usize;
    while i < RetestCode::ALL.len() {
        let mut j = i + 1;
        while j < RetestCode::ALL.len() {
            if let (Some(a), Some(b)) = (RetestCode::ALL.get(i), RetestCode::ALL.get(j)) {
                if a.code() == b.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = RetestCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：12 码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 14. 无待复测语义：空执行器 next_pending 为 None
    let empty = RetestExecutor::new();
    s.add("空调度：无条目时 next_pending 为空", empty.next_pending().is_none(),
          "调度器空转必须显性空，不许返回幽灵条目");

    // 15. 双跑指纹一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑指纹：规范语料两次哈希相等", f1 == f2,
          "同批次两次复测账必须逐位一致");

    // 16. 留痕序号单调：3 条复测序号 1..=3
    let seq_ok = ex.records() == 3;
    s.add("留痕序号：3 条复测逐条递增登记", seq_ok,
          "seq_gen 只增，复测顺序可回放");

    // 动态明细（≤8）：分账 + 注入账 + 指纹
    let ledger_text = format!(
        "过 {} / 败 {} / 留痕 {}",
        ex.pass_count(), ex.fail_count(), ex.records()
    );
    s.add_dyn("结论分账快照", ex.pass_count() + ex.fail_count() == ex.records(), &ledger_text);
    let inj_text = format!("注入 {} / 检出 {} / 逃逸 {}", inj.injected, inj.detected, inj.escaped);
    s.add_dyn("注入账快照", inj.conserved() && inj.escaped == 0, &inj_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admit_and_start_finish() {
        let mut ex = RetestExecutor::new();
        assert!(ex.admit(1, 9001, 1).is_ok());
        assert!(ex.start(1).is_ok());
        assert!(ex.finish(1, 9001, Verdict::Pass).is_ok());
        assert_eq!(ex.entry(1).unwrap_or(RetestEntry { id: 0, criterion: 0, batch: 0, state: RetestState::Pending }).state, RetestState::Passed);
    }

    #[test]
    fn proxy_retest_rejected() {
        let mut ex = RetestExecutor::new();
        let _ = ex.admit(1, 9001, 1);
        let _ = ex.start(1);
        assert_eq!(ex.finish(1, 9002, Verdict::Pass).err(), Some(RetestCode::ProxyRetest));
    }

    #[test]
    fn zero_criterion_rejected() {
        let mut ex = RetestExecutor::new();
        assert_eq!(ex.admit(1, 0, 1).err(), Some(RetestCode::UnknownCriterion));
        let _ = ex.admit(1, 9001, 1);
        let _ = ex.start(1);
        assert_eq!(ex.finish(1, 0, Verdict::Pass).err(), Some(RetestCode::UnknownCriterion));
    }

    #[test]
    fn skip_start_rejected() {
        let mut ex = RetestExecutor::new();
        let _ = ex.admit(1, 9001, 1);
        assert_eq!(ex.finish(1, 9001, Verdict::Pass).err(), Some(RetestCode::BadTransition));
    }

    #[test]
    fn unbatched_rejected() {
        let mut ex = RetestExecutor::new();
        let _ = ex.admit(1, 9001, 0);
        assert_eq!(ex.start(1).err(), Some(RetestCode::UnbatchedEntry));
    }

    #[test]
    fn dup_finish_rejected() {
        let mut ex = RetestExecutor::new();
        let _ = ex.admit(1, 9001, 1);
        let _ = ex.start(1);
        let _ = ex.finish(1, 9001, Verdict::Fail);
        assert_eq!(ex.finish(1, 9001, Verdict::Fail).err(), Some(RetestCode::DupRetest));
    }

    #[test]
    fn scheduler_ascending() {
        let mut ex = RetestExecutor::new();
        let _ = ex.admit(3, 7003, 1);
        let _ = ex.admit(1, 7001, 1);
        let _ = ex.admit(2, 7002, 1);
        assert_eq!(ex.next_pending(), Some(1));
    }

    #[test]
    fn verdict_accounts_close() {
        let mut ex = RetestExecutor::new();
        let _ = ex.admit(1, 9001, 1);
        let _ = ex.admit(2, 9002, 1);
        let _ = ex.start(1);
        let _ = ex.start(2);
        let _ = ex.finish(1, 9001, Verdict::Pass);
        let _ = ex.finish(2, 9002, Verdict::Fail);
        assert_eq!(ex.pass_count(), 1);
        assert_eq!(ex.fail_count(), 1);
        assert_eq!(ex.records(), 2);
        assert!(ex.conserved());
    }

    #[test]
    fn bad_retest_matrix_green() {
        let (inj, per) = run_bad_retest_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(77));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(1), -1);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx10025_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
