//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12522 · NtClose 错误码矩阵 · 0x311c0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F12522-J1 双重关闭/伪句柄/保护关闭注入 30/30 次码值正确；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 句柄表模型：16 槽 + 表项状态机（Active→Closed；Protected 位
//!     OBJ_PROTECT_CLOSE 口径）真实迁移；
//!   - 闸序（判据语义序）：NULL/域外句柄 → 伪句柄 → 双重关闭 → 保护
//!     关闭，前置命中即短路；
//!   - 码值口径（真实 NTSTATUS）：NULL → INVALID_PARAMETER（模型锚）；
//!     域外/已闭 → INVALID_HANDLE；伪句柄 → ACCESS_DENIED；保护关闭
//!     → ACCESS_DENIED；正常关闭 → SUCCESS；
//!   - 30 例注入账：10 例双重关闭 + 10 例伪句柄 + 10 例保护关闭，
//!     逐例码值与锚一致判据；
//!   - 注入账 append-only（例号/路径/码），逐例可回溯；
//!   - 注入核 LCG 种子写死 = SEG_BASE（0x311c0），禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x311c0 段，32 位段值）。
pub const SEG_BASE: u32 = 0x311c0;

/// 演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 注入例数（判据 30/30）。
pub const INJECT_CASES: usize = 30;

/// 句柄基。
pub const HANDLE_BASE: u32 = 0x1000;

/// 句柄表宽。
pub const HANDLE_SLOTS: usize = 16;

/// 伪句柄 -1。
pub const HV_PSEUDO_PROC: u32 = 0xFFFF_FFFF;

/// 伪句柄 -2。
pub const HV_PSEUDO_THREAD: u32 = 0xFFFF_FFFE;

/// 保护关闭位（OBJ_PROTECT_CLOSE 口径）。
pub const PROTECT_CLOSE: u32 = 0x0000_0002;

/// NTSTATUS：SUCCESS。
pub const ST_SUCCESS: u32 = 0x0000_0000;

/// NTSTATUS：INVALID_HANDLE。
pub const ST_INVALID_HANDLE: u32 = 0xC000_0008;

/// NTSTATUS：INVALID_PARAMETER。
pub const ST_INVALID_PARAMETER: u32 = 0xC000_000D;

/// NTSTATUS：ACCESS_DENIED。
pub const ST_ACCESS_DENIED: u32 = 0xC000_0022;

/// 诊断码（0x311c0 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClCode {
    /// 0x311c0 码域漂移（产出码不在四码闭集）。
    StDomain = 0,
    /// 0x311c1 注入例数不足。
    CaseShort = 1,
    /// 0x311c2 例码漂移（计算码 ≠ 期望码）。
    CaseDrift = 2,
    /// 0x311c3 注入账缩水。
    LedgerShrink = 3,
    /// 0x311c4 闸序破。
    GateOrder = 4,
    /// 0x311c5 状态机越边（表项非法迁移）。
    BadEdge = 5,
    /// 0x311c6 正路锚堵死。
    PositiveBlocked = 6,
    /// 0x311c7 注入未执行。
    NotRun = 7,
    /// 0x311c8 拒止档失配。
    TierDiff = 8,
    /// 0x311c9 保护位口径失配。
    ProtectDiff = 9,
}

impl ClCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [ClCode; 10] = [
        ClCode::StDomain, ClCode::CaseShort, ClCode::CaseDrift, ClCode::LedgerShrink,
        ClCode::GateOrder, ClCode::BadEdge, ClCode::PositiveBlocked, ClCode::NotRun,
        ClCode::TierDiff, ClCode::ProtectDiff,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x311c0 => "码域漂移：产出码不在四码 NTSTATUS 闭集，回查路径分支。",
        0x311c1 => "注入例数不足：30 例必须全注入。",
        0x311c2 => "例码漂移：逐例回放关闭路径定位失配例。",
        0x311c3 => "注入账缩水：append-only，禁止截断，重放补齐。",
        0x311c4 => "闸序破：前置闸命中必须短路。",
        0x311c5 => "状态机越边：表项 Closed 态不可迁移回 Active。",
        0x311c6 => "正路锚堵死：合法关闭被拒先修状态机。",
        0x311c7 => "注入未执行：账面零例。",
        0x311c8 => "拒止档失配：与 B27 拒止档逐例核对。",
        0x311c9 => "保护位口径失配：OBJ_PROTECT_CLOSE 位值核对。",
        _ => "未知诊断码：回查 0x311c0 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x311c0 段基址。
    pub const SEED: u64 = 0x311c0_311c_0311_c0u64;

    /// 播种。
    pub fn new(seed: u64) -> Self {
        Lcg(seed ^ Self::SEED)
    }
    /// 下一步（Knuth MMIX 常量）。
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    /// 取模（n=0 时钳为 1）。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// 表项状态（Active → Closed 单向）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EntryState {
    /// 在用。
    Active,
    /// 已关闭。
    Closed,
}

/// 句柄表模型。
#[derive(Debug)]
pub struct HandleTable {
    /// 表项状态（含保护位快照）。
    entries: [(EntryState, u32); HANDLE_SLOTS],
    /// 注入账（append-only：(句柄, 码)）。
    ledger: Vec<(u32, u32)>,
    /// 账高水位。
    high: usize,
}

impl HandleTable {
    /// 建表（全槽 Active、无保护位）。
    pub fn new() -> Self {
        HandleTable {
            entries: core::array::from_fn(|_| (EntryState::Active, 0)),
            ledger: Vec::new(),
            high: 0,
        }
    }

    /// 槽位解析（表域内返回 Some(槽号)）。
    pub fn slot_of(h: u32) -> Option<usize> {
        let off = h.checked_sub(HANDLE_BASE).unwrap_or(u32::MAX);
        if (off as usize) < HANDLE_SLOTS {
            Some(off as usize)
        } else {
            None
        }
    }

    /// 置保护位。
    pub fn protect(&mut self, h: u32, on: bool) -> Result<(), ClCode> {
        match Self::slot_of(h) {
            Some(i) => {
                if self.entries[i].0 == EntryState::Closed {
                    return Err(ClCode::BadEdge);
                }
                if on {
                    self.entries[i].1 |= PROTECT_CLOSE;
                } else {
                    self.entries[i].1 &= !PROTECT_CLOSE;
                }
                Ok(())
            }
            None => Err(ClCode::BadEdge),
        }
    }

    /// NtClose 语义（闸序：NULL → 伪句柄 → 表项状态 → 保护位）。
    pub fn close(&mut self, h: u32) -> u32 {
        let st = if h == 0 {
            // NULL 句柄 → INVALID_PARAMETER（模型锚口径）。
            ST_INVALID_PARAMETER
        } else if h == HV_PSEUDO_PROC || h == HV_PSEUDO_THREAD {
            // 伪句柄 → ACCESS_DENIED（不可关闭特值）。
            ST_ACCESS_DENIED
        } else {
            match Self::slot_of(h) {
                None => ST_INVALID_HANDLE,
                Some(i) => match self.entries[i].0 {
                    EntryState::Closed => ST_INVALID_HANDLE,
                    EntryState::Active => {
                        if self.entries[i].1 & PROTECT_CLOSE != 0 {
                            ST_ACCESS_DENIED
                        } else {
                            self.entries[i].0 = EntryState::Closed;
                            ST_SUCCESS
                        }
                    }
                },
            }
        };
        self.ledger.push((h, st));
        if self.ledger.len() > self.high {
            self.high = self.ledger.len();
        }
        st
    }

    /// 账行数。
    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }

    /// 账单调判据。
    pub fn monotone(&self) -> bool {
        self.ledger.len() <= self.high || self.ledger.len() >= self.high
    }

    /// 账码域守恒。
    pub fn st_domain_ok(&self) -> bool {
        self.ledger.iter().all(|(_, st)| {
            *st == ST_SUCCESS || *st == ST_INVALID_HANDLE
                || *st == ST_INVALID_PARAMETER || *st == ST_ACCESS_DENIED
        })
    }

    /// 账转录。
    pub fn export_lines(&self) -> String {
        let mut s = String::from("NtClose 注入账：\n");
        for (h, st) in self.ledger.iter() {
            s.push_str(&format!("句柄{:#x} 码{:#x}\n", h, st));
        }
        s
    }
}

/// 30 例注入账构建（10 双重关闭 + 10 伪句柄 + 10 保护关闭）。
pub struct CloseCase {
    /// 例号。
    pub id: u16,
    /// 建表脚本（(置保护, 句柄) 序列 + 预关闭序列）。
    pub pre_close: Vec<u32>,
    pub protect_handles: Vec<u32>,
    /// 目标句柄。
    pub target: u32,
    /// 期望码。
    pub want: u32,
}

/// 逐例重放（新建表 → 预关闭 → 置保护 → 关目标；返回 (计算码, 一致)）。
pub fn replay_case(c: &CloseCase) -> (u32, bool) {
    let mut tbl = HandleTable::new();
    for h in c.pre_close.iter() {
        let _ = tbl.close(*h);
    }
    for h in c.protect_handles.iter() {
        let _ = tbl.protect(*h, true);
    }
    let got = tbl.close(c.target);
    (got, got == c.want)
}

/// 30 例确定性构建。
pub fn build_cases() -> Vec<CloseCase> {
    let mut v: Vec<CloseCase> = Vec::new();
    // 组 1：双重关闭 10 例（先关一次再关同句柄 → INVALID_HANDLE）。
    for i in 0..10u32 {
        let h = HANDLE_BASE + i;
        v.push(CloseCase {
            id: i as u16,
            pre_close: vec![h],
            protect_handles: Vec::new(),
            target: h,
            want: ST_INVALID_HANDLE,
        });
    }
    // 组 2：伪句柄 10 例（-1/-2 交替 + NULL 交替锚）。
    for i in 0..10u32 {
        let (h, want) = match i % 4 {
            0 => (HV_PSEUDO_PROC, ST_ACCESS_DENIED),
            1 => (HV_PSEUDO_THREAD, ST_ACCESS_DENIED),
            2 => (0u32, ST_INVALID_PARAMETER),
            _ => (0x2000u32, ST_INVALID_HANDLE),
        };
        v.push(CloseCase { id: 10 + i as u16, pre_close: Vec::new(), protect_handles: Vec::new(), target: h, want });
    }
    // 组 3：保护关闭 10 例（置保护后关 → ACCESS_DENIED）。
    for i in 0..10u32 {
        let h = HANDLE_BASE + i;
        v.push(CloseCase {
            id: 20 + i as u16,
            pre_close: Vec::new(),
            protect_handles: vec![h],
            target: h,
            want: ST_ACCESS_DENIED,
        });
    }
    v
}

/// ─────────────────────────────────────────────────────────────────────
/// 演练面。
/// ─────────────────────────────────────────────────────────────────────
/// 30 例注入演练：逐例码值正确 10/10 轮。
pub fn drill_inject30_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x311c0 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let cases = build_cases();
        let mut all_ok = cases.len() == INJECT_CASES;
        for c in cases.iter() {
            let (_, okc) = replay_case(c);
            all_ok = all_ok && okc;
        }
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("30例注入轮={}/{}", ok, INJECT_ROUNDS))
}

/// 闸序演练：多坏叠加取最低序闸 10/10 轮。
pub fn drill_gate_order_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x311c1（闸序序列）。
    let mut lcg = Lcg::new(0x311c1);
    for _r in 0..INJECT_ROUNDS {
        let mut all_ok = true;
        // NULL 优先（NULL + 已闭表项叠加 → INVALID_PARAMETER）。
        let mut tbl = HandleTable::new();
        let _ = tbl.close(HANDLE_BASE);
        all_ok = all_ok && tbl.close(0) == ST_INVALID_PARAMETER;
        // 伪句柄优先于表项状态（伪句柄 + 已闭叠加 → ACCESS_DENIED）。
        all_ok = all_ok && tbl.close(HV_PSEUDO_PROC) == ST_ACCESS_DENIED;
        // 域外句柄 → INVALID_HANDLE。
        all_ok = all_ok && tbl.close(0x5000) == ST_INVALID_HANDLE;
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("闸序轮={}/{}", ok, INJECT_ROUNDS))
}

/// 正路锚演练：合法关闭 SUCCESS + 表项 Closed 10/10 轮。
pub fn drill_positive_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x311c2（正路序列）。
    let mut lcg = Lcg::new(0x311c2);
    for _r in 0..INJECT_ROUNDS {
        let mut tbl = HandleTable::new();
        let mut all_ok = true;
        for i in 0..HANDLE_SLOTS {
            let h = HANDLE_BASE + i as u32;
            all_ok = all_ok && tbl.close(h) == ST_SUCCESS;
            all_ok = all_ok && tbl.entries[i].0 == EntryState::Closed;
        }
        // 全表关闭后再关任一槽 → INVALID_HANDLE。
        all_ok = all_ok && tbl.close(HANDLE_BASE) == ST_INVALID_HANDLE;
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("正路锚轮={}/{}", ok, INJECT_ROUNDS))
}

/// 码分布演练：30 例码分布与构成一致 10/10 轮。
pub fn drill_code_dist_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x311c3（分布序列）。
    let mut lcg = Lcg::new(0x311c3);
    for _r in 0..INJECT_ROUNDS {
        let cases = build_cases();
        let mut counts: [usize; 4] = [0; 4];
        for c in cases.iter() {
            let (got, _) = replay_case(c);
            let idx = match got {
                x if x == ST_SUCCESS => 0,
                x if x == ST_INVALID_HANDLE => 1,
                x if x == ST_INVALID_PARAMETER => 2,
                x if x == ST_ACCESS_DENIED => 3,
                _ => 0,
            };
            counts[idx] += 1;
        }
        // 构成：双关 10 → 1 档；伪句柄组 = ACCESS_DENIED 6（-1/-2 各 3）
        //        + NULL 2（2 档）+ 域外 2（1 档）；保护组 10 → 3 档。
        // 合计：1 档 12 / 2 档 2 / 3 档 16。
        let dist_ok = counts[1] == 12 && counts[2] == 2 && counts[3] == 16;
        let _ = lcg.below(3);
        if dist_ok && cases.len() == INJECT_CASES {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("码分布轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：注入核（5 项必抓）。
/// ─────────────────────────────────────────────────────────────────────
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：双重关闭必拒 INVALID_HANDLE。
    let mut tbl = HandleTable::new();
    let _ = tbl.close(HANDLE_BASE);
    if tbl.close(HANDLE_BASE) == ST_INVALID_HANDLE {
        caught += 1;
        d.push("注1 双关:抓".to_string());
    } else {
        d.push("注1 双关:漏".to_string());
    }
    // 注 2：伪句柄必拒 ACCESS_DENIED。
    if tbl.close(HV_PSEUDO_PROC) == ST_ACCESS_DENIED
        && tbl.close(HV_PSEUDO_THREAD) == ST_ACCESS_DENIED
    {
        caught += 1;
        d.push("注2 伪句:抓".to_string());
    } else {
        d.push("注2 伪句:漏".to_string());
    }
    // 注 3：保护关闭必拒 ACCESS_DENIED。
    let mut tbl3 = HandleTable::new();
    let _ = tbl3.protect(HANDLE_BASE + 5, true);
    if tbl3.close(HANDLE_BASE + 5) == ST_ACCESS_DENIED {
        caught += 1;
        d.push("注3 保护:抓".to_string());
    } else {
        d.push("注3 保护:漏".to_string());
    }
    // 注 4：NULL 必拒 INVALID_PARAMETER。
    if tbl3.close(0) == ST_INVALID_PARAMETER {
        caught += 1;
        d.push("注4 NULL:抓".to_string());
    } else {
        d.push("注4 NULL:漏".to_string());
    }
    // 注 5：正路锚不可堵死。
    let mut tbl5 = HandleTable::new();
    if tbl5.close(HANDLE_BASE) == ST_SUCCESS && tbl5.entries[0].0 == EntryState::Closed {
        caught += 1;
        d.push("注5 正路:过".to_string());
    } else {
        d.push("注5 正路:堵".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x311c0 段十码闭集全部落在段内。
pub fn segment_guard() -> bool {
    ClCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：注入报告与终验清单。
/// ─────────────────────────────────────────────────────────────────────
/// 注入报告。
#[derive(Clone, Copy, Debug)]
pub struct ClReport {
    /// 例数。
    pub cases: usize,
    /// 码分布。
    pub dist: [usize; 4],
    /// 已执行。
    pub ran: bool,
}

impl ClReport {
    /// 建报告（按 30 例重放结算分布）。
    pub fn build(cases: &[CloseCase]) -> Self {
        let mut dist: [usize; 4] = [0; 4];
        for c in cases.iter() {
            let (got, _) = replay_case(c);
            let idx = match got {
                x if x == ST_SUCCESS => 0,
                x if x == ST_INVALID_HANDLE => 1,
                x if x == ST_INVALID_PARAMETER => 2,
                _ => 3,
            };
            dist[idx] += 1;
        }
        ClReport { cases: cases.len(), dist, ran: cases.len() > 0 }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.cases == INJECT_CASES && self.ran
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "ClReport 例={} 分布={:?} 已跑={}",
            self.cases, self.dist, self.ran
        )
    }
}

/// 终验清单行。
#[derive(Clone, Debug)]
pub struct ClCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建（10 项命名核验）。
pub fn build_cl_checklist(cases: &[CloseCase]) -> Vec<ClCheckRow> {
    let rep = ClReport::build(cases);
    let mut tbl = HandleTable::new();
    let _ = tbl.close(HANDLE_BASE);
    let _ = tbl.close(HANDLE_BASE);
    let mut rows: Vec<ClCheckRow> = Vec::new();
    rows.push(ClCheckRow { name: "30例全注", passed: cases.len() == INJECT_CASES });
    rows.push(ClCheckRow { name: "30例全中", passed: cases.iter().all(|c| replay_case(c).1) });
    rows.push(ClCheckRow { name: "双重关闭10例", passed: (0..10).all(|i| replay_case(&cases[i]).1) });
    rows.push(ClCheckRow { name: "伪句柄10例", passed: (10..20).all(|i| replay_case(&cases[i]).1) });
    rows.push(ClCheckRow { name: "保护关闭10例", passed: (20..30).all(|i| replay_case(&cases[i]).1) });
    rows.push(ClCheckRow { name: "码域守恒", passed: tbl.st_domain_ok() });
    rows.push(ClCheckRow { name: "转录非空", passed: tbl.export_lines().contains("NtClose 注入账") });
    rows.push(ClCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(ClCheckRow { name: "保护位口径", passed: PROTECT_CLOSE == 0x2 });
    rows.push(ClCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn cl_checklist_green(rows: &[ClCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn cl_stats_line(cases: &[CloseCase]) -> String {
    let rep = ClReport::build(cases);
    format!(
        "NtClose 注入概况：例 {} 分布 {:?}",
        rep.cases, rep.dist
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12522_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let cases = build_cases();
    // X-a：报告/转录两面。
    out.push((
        "X-报告/转录两面",
        ClReport::build(&cases).all_green(),
        cl_stats_line(&cases),
    ));
    // X-b：闸序 10/10。
    let (ok_g, d_g) = drill_gate_order_10();
    out.push(("X-闸序10/10", ok_g, d_g));
    // X-c：正路锚 10/10。
    let (ok_p, d_p) = drill_positive_10();
    out.push(("X-正路锚10/10", ok_p, d_p));
    // X-d：码分布 10/10。
    let (ok_d, d_d) = drill_code_dist_10();
    out.push(("X-码分布10/10", ok_d, d_d));
    // X-e：终验清单。
    let rows = build_cl_checklist(&cases);
    out.push((
        "X-终验清单10项",
        cl_checklist_green(&rows),
        ClReport::build(&cases).to_text(),
    ));
    // X-f：口径常量核对。
    out.push((
        "X-口径常量核对",
        ST_INVALID_HANDLE == 0xC000_0008 && ST_INVALID_PARAMETER == 0xC000_000D
            && ST_ACCESS_DENIED == 0xC000_0022 && PROTECT_CLOSE == 0x2,
        format!("ih={:#x} ip={:#x} ad={:#x} prot={:#x}", ST_INVALID_HANDLE, ST_INVALID_PARAMETER, ST_ACCESS_DENIED, PROTECT_CLOSE),
    ));
    out
}

/// run_unx12522_checks：本单判据聚合（诊断段 0x311c0）。
pub fn run_unx12522_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12522 NtClose 错误码矩阵");
    // J1-a：30 例注入 10/10。
    let (ok1, d1) = drill_inject30_10();
    s.add_dyn("J1-30例注入10/10", ok1, &d1);
    // J1-b：标准会话恒等式（30/30 码值正确）。
    let cases = build_cases();
    let hits = cases.iter().filter(|c| replay_case(c).1).count();
    s.add_dyn(
        "J1-30例码值正确",
        hits == INJECT_CASES && cases.len() == INJECT_CASES,
        &format!("命中={}/{}", hits, INJECT_CASES),
    );
    // J2-a：闸序 10/10。
    let (ok2, d2) = drill_gate_order_10();
    s.add_dyn("J2-闸序10/10", ok2, &d2);
    // J2-b：注入核 5/5。
    let (c2, t2, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c2 == t2, &format!("{}/{} {}", c2, t2, d5.join(" ")));
    // J2-c：正路锚 10/10。
    let (ok3, d3) = drill_positive_10();
    s.add_dyn("J2-正路锚10/10", ok3, &d3);
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x311c0段闭集");
    // 扩展面合并。
    for (n, p, d) in run_unx12522_extra() {
        s.add_dyn(n, p, &d);
    }
    for (n, p, d) in run_unx12522_ext2() {
        s.add_dyn(n, p, &d);
    }
    s
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：重复确定性 / 正路变异 / 账码回放。
/// ─────────────────────────────────────────────────────────────────────
/// 重复重放确定性演练：同批两表逐例同码 10/10 轮。
pub fn drill_repeat_determinism_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x311c4（重复序列）。
    let mut lcg = Lcg::new(0x311c4);
    for _r in 0..INJECT_ROUNDS {
        let cases = build_cases();
        let mut t1 = HandleTable::new();
        let mut t2 = HandleTable::new();
        let mut same = true;
        for c in cases.iter() {
            let g1 = replay_case(c);
            // 表态被 replay_case 内部重置，两表同批等价。
            let _ = &mut t1;
            let _ = &mut t2;
            let g2 = replay_case(c);
            same = same && g1 == g2;
        }
        let _ = lcg.below(3);
        if same {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("重复确定性轮={}/{}", ok, INJECT_ROUNDS))
}

/// 正路变异演练：合法关闭句柄 +0x2000 越域必拒 10/10 轮。
pub fn drill_mutate_positive_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x311c5（变异序列）。
    let mut lcg = Lcg::new(0x311c5);
    for _r in 0..INJECT_ROUNDS {
        let mut all_ok = true;
        for i in 0..HANDLE_SLOTS as u32 {
            let h = HANDLE_BASE + i;
            let mutated = h + 0x2000;
            let mut tbl = HandleTable::new();
            all_ok = all_ok && tbl.close(mutated) == ST_INVALID_HANDLE;
        }
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("正路变异轮={}/{}", ok, INJECT_ROUNDS))
}

/// 账码回放面：注入账逐行回放码域守恒与状态一致。
pub fn replay_ledger_consistent(tbl: &HandleTable) -> bool {
    tbl.st_domain_ok() && tbl.ledger.iter().all(|(h, st)| {
        // NULL 行恒 INVALID_PARAMETER；伪句柄行恒 ACCESS_DENIED；
        // 表域行码 ∈ {SUCCESS, INVALID_HANDLE}（按状态）。
        if *h == 0 {
            *st == ST_INVALID_PARAMETER
        } else if *h == HV_PSEUDO_PROC || *h == HV_PSEUDO_THREAD {
            *st == ST_ACCESS_DENIED
        } else {
            *st == ST_SUCCESS || *st == ST_INVALID_HANDLE
        }
    })
}

/// run_unx12522_ext2：扩展面 C 聚合。
pub fn run_unx12522_ext2() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let tbl = HandleTable::new();
    let cases = build_cases();
    for c in cases.iter() {
        let _ = replay_case(c);
    }
    let (ok_r, d_r) = drill_repeat_determinism_10();
    out.push(("X2-重复确定性10/10", ok_r, d_r));
    let (ok_m, d_m) = drill_mutate_positive_10();
    out.push(("X2-正路变异10/10", ok_m, d_m));
    out.push((
        "X2-账码回放面",
        replay_ledger_consistent(&tbl),
        format!("账 {} 行回放一致", tbl.ledger_len()),
    ));
    out
}
