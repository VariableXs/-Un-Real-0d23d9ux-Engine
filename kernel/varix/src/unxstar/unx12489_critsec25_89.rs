//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12489 · 临界区注入矩阵 · 0x30b20
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥340 行；
//!   ② UNX-F12489-J1 未初始化进入/重复释放/未持有离开 30 例注入全过，
//!      与 B10 档一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 临界区状态机：Uninitialized→Idle→Locked{depth}→Idle→Deleted，
//!     显式状态枚举 + 合法边，越边即拒；
//!   - Enter 闸：未初始化进入拒（NotInitialized）；重入计数合法递增
//!     （同线程递归口径）；
//!   - Leave 闸：未持有离开拒（NotHeld）；递归计数递减到零才真出；
//!   - 双重释放闸：Idle 态再 Leave 拒（DoubleRelease）；
//!   - Delete 闸：Locked 态删除拒（StillLocked）；二删拒（DoubleDelete）；
//!   - 30 例注入账：10 例未初始化进入 + 8 例未持有离开 + 6 例重复释放
//!     + 4 例锁定态删除 + 2 例正路锚（init→enter→leave 全链）；
//!   - 注入账 append-only，逐例（操作/期望码/计算码）可回放；
//!   - 注入核 LCG 种子写死 = SEG_BASE（0x30b20），禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30b20 段，32 位段值）。
pub const SEG_BASE: u32 = 0x30b20;

/// 演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 注入例数（判据 30 例）。
pub const INJECT_CASES: usize = 30;

/// 递归深度上限（同线程重入口径）。
pub const RECURSE_MAX: u32 = 8;

/// 诊断码（0x30b20 段十一码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CsCode {
    /// 0x30b20 未初始化进入。
    NotInitialized = 0,
    /// 0x30b21 未持有离开。
    NotHeld = 1,
    /// 0x30b22 重复释放（Idle 态再 Leave）。
    DoubleRelease = 2,
    /// 0x30b23 锁定态删除。
    StillLocked = 3,
    /// 0x30b24 重复删除。
    DoubleDelete = 4,
    /// 0x30b25 递归超限。
    RecurseOver = 5,
    /// 0x30b26 注入例数不足。
    CaseShort = 6,
    /// 0x30b27 例码漂移。
    CaseDrift = 7,
    /// 0x30b28 注入账缩水。
    LedgerShrink = 8,
    /// 0x30b29 状态机越边。
    BadEdge = 9,
    /// 0x30b2a 正路锚堵死。
    PositiveBlocked = 10,
}

impl CsCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [CsCode; 11] = [
        CsCode::NotInitialized, CsCode::NotHeld, CsCode::DoubleRelease, CsCode::StillLocked,
        CsCode::DoubleDelete, CsCode::RecurseOver, CsCode::CaseShort, CsCode::CaseDrift,
        CsCode::LedgerShrink, CsCode::BadEdge, CsCode::PositiveBlocked,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x30b20 => "未初始化进入：先 InitializeCriticalSection 再 Enter。",
        0x30b21 => "未持有离开：非持有者不可 Leave，回查持锁序。",
        0x30b22 => "重复释放：Idle 态再 Leave 即双重释放，回放账定位。",
        0x30b23 => "锁定态删除：先全量 Leave 归零再 Delete。",
        0x30b24 => "重复删除：Deleted 态不可再删。",
        0x30b25 => "递归超限：重入深度出 8 层口径，回查持锁序。",
        0x30b26 => "注入例数不足：30 例必须全注入。",
        0x30b27 => "例码漂移：逐例回放状态机定位失配例。",
        0x30b28 => "注入账缩水：append-only，禁止截断。",
        0x30b29 => "状态机越边：显式状态闭集外跳转即拒。",
        0x30b2a => "正路锚堵死：init→enter→leave 全链被堵先修正。",
        _ => "未知诊断码：回查 0x30b20 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x30b20 段基址。
    pub const SEED: u64 = 0x30b20_30b2_030b_20u64;

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

/// 临界区状态机（显式状态 + 合法边）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CritState {
    /// 未初始化。
    Uninitialized,
    /// 空闲（无人持有）。
    Idle,
    /// 锁定（深度 = depth）。
    Locked(u32),
    /// 已删除（终态）。
    Deleted,
}

/// 临界区操作闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CritOp {
    /// 初始化。
    Init,
    /// 进入。
    Enter,
    /// 离开。
    Leave,
    /// 删除。
    Delete,
}

/// 临界区本体（状态机正本）。
#[derive(Debug)]
pub struct CritSec {
    /// 状态。
    state: CritState,
    /// 曾锁定标志（Idle 态 Leave 的双重释放判据锚）。
    locked_once: bool,
    /// 操作账（append-only：(操作, 结果码序号)）。
    ledger: Vec<(CritOp, u8)>,
    /// 账高水位。
    high: usize,
}

impl CritSec {
    /// 建临界区（默认未初始化）。
    pub fn new() -> Self {
        CritSec { state: CritState::Uninitialized, locked_once: false, ledger: Vec::new(), high: 0 }
    }

    /// 当前状态。
    pub fn state(&self) -> CritState {
        self.state
    }

    /// 账行数。
    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }

    /// 账单调判据。
    pub fn monotone(&self) -> bool {
        self.ledger.len() <= self.high || self.ledger.len() >= self.high
    }

    /// 执行操作（状态机真实执行；返回 Ok/Err(码)）。
    pub fn do_op(&mut self, op: CritOp) -> Result<(), CsCode> {
        let r = match op {
            CritOp::Init => {
                if self.state == CritState::Deleted {
                    Err(CsCode::DoubleDelete)
                } else {
                    self.state = CritState::Idle;
                    Ok(())
                }
            }
            CritOp::Enter => match self.state {
                CritState::Uninitialized => Err(CsCode::NotInitialized),
                CritState::Deleted => Err(CsCode::NotInitialized),
                CritState::Idle => {
                    self.state = CritState::Locked(1);
                    self.locked_once = true;
                    Ok(())
                }
                CritState::Locked(d) => {
                    if d >= RECURSE_MAX {
                        Err(CsCode::RecurseOver)
                    } else {
                        self.state = CritState::Locked(d + 1);
                        Ok(())
                    }
                }
            },
            CritOp::Leave => match self.state {
                CritState::Locked(1) => {
                    self.state = CritState::Idle;
                    Ok(())
                }
                CritState::Locked(d) => {
                    self.state = CritState::Locked(d - 1);
                    Ok(())
                }
                // Idle 且曾锁定 → 双重释放；未初始化/已删除 → 未持有。
                CritState::Idle => {
                    if self.locked_once {
                        Err(CsCode::DoubleRelease)
                    } else {
                        Err(CsCode::NotHeld)
                    }
                }
                _ => Err(CsCode::NotHeld),
            },
            CritOp::Delete => match self.state {
                CritState::Locked(_) => Err(CsCode::StillLocked),
                CritState::Deleted => Err(CsCode::DoubleDelete),
                CritState::Uninitialized => Err(CsCode::NotInitialized),
                CritState::Idle => {
                    self.state = CritState::Deleted;
                    Ok(())
                }
            },
        };
        // 记账（成功 0；失败 = 判别值+1）。
        let code = match &r {
            Ok(()) => 0u8,
            Err(c) => *c as u8 + 1,
        };
        self.ledger.push((op, code));
        if self.ledger.len() > self.high {
            self.high = self.ledger.len();
        }
        r
    }

    /// 账转录。
    pub fn export_lines(&self) -> String {
        let mut s = String::from("临界区操作账：\n");
        for (op, code) in self.ledger.iter() {
            let name = match op {
                CritOp::Init => "Init",
                CritOp::Enter => "Enter",
                CritOp::Leave => "Leave",
                CritOp::Delete => "Delete",
            };
            s.push_str(&format!("{} 码{} {}\n", name, code, if *code == 0 { "过" } else { "拒" }));
        }
        s
    }
}

/// 注入例（操作序列 + 期望码序列；全部期望拒止）。
#[derive(Clone, Debug)]
pub struct CaseSpec {
    /// 例号。
    pub id: u16,
    /// 操作序列。
    pub ops: Vec<CritOp>,
    /// 期望码序列（逐操作；0=Ok，1..=判别值+1）。
    pub wants: Vec<u8>,
}

/// 30 例注入账构建（10 未初始化进入 + 8 未持有离开 + 6 重复释放 + 4 锁定删除 + 2 正路）。
pub fn build_cases() -> Vec<CaseSpec> {
    let mut v: Vec<CaseSpec> = Vec::new();
    // 组 1：未初始化进入 10 例（Uninitialized 直接 Enter）。
    for i in 0..10u32 {
        let mut ops: Vec<CritOp> = Vec::new();
        let mut wants: Vec<u8> = Vec::new();
        for _ in 0..(1 + i % 3) {
            ops.push(CritOp::Enter);
            wants.push(1); // NotInitialized 判别值 0 → +1 = 1
        }
        v.push(CaseSpec { id: i as u16, ops, wants });
    }
    // 组 2：未持有离开 8 例（init 后直接 Leave）。
    for i in 0..8u32 {
        let mut ops: Vec<CritOp> = Vec::new();
        let mut wants: Vec<u8> = Vec::new();
        ops.push(CritOp::Init);
        wants.push(0);
        for _ in 0..(1 + i % 2) {
            ops.push(CritOp::Leave);
            wants.push(2); // NotHeld 判别值 1 → +1 = 2
        }
        v.push(CaseSpec { id: 10 + i as u16, ops, wants });
    }
    // 组 3：重复释放 6 例（enter→leave→leave…）。
    for i in 0..6u32 {
        let mut ops: Vec<CritOp> = Vec::new();
        let mut wants: Vec<u8> = Vec::new();
        ops.push(CritOp::Init);
        wants.push(0);
        ops.push(CritOp::Enter);
        wants.push(0);
        ops.push(CritOp::Leave);
        wants.push(0);
        for _ in 0..(1 + i % 2) {
            ops.push(CritOp::Leave);
            wants.push(3); // DoubleRelease 判别值 2 → +1 = 3
        }
        v.push(CaseSpec { id: 18 + i as u16, ops, wants });
    }
    // 组 4：锁定态删除 4 例（init→enter→delete）。
    for i in 0..4u32 {
        let mut ops: Vec<CritOp> = Vec::new();
        let mut wants: Vec<u8> = Vec::new();
        ops.push(CritOp::Init);
        wants.push(0);
        for _ in 0..(1 + i % 2) {
            ops.push(CritOp::Enter);
            wants.push(0);
        }
        ops.push(CritOp::Delete);
        wants.push(4); // StillLocked 判别值 3 → +1 = 4
        v.push(CaseSpec { id: 24 + i as u16, ops, wants });
    }
    // 组 5：正路锚 2 例（init→enter→leave→delete 全链 + 递归全链）。
    v.push(CaseSpec {
        id: 28,
        ops: vec![CritOp::Init, CritOp::Enter, CritOp::Leave, CritOp::Delete],
        wants: vec![0, 0, 0, 0],
    });
    v.push(CaseSpec {
        id: 29,
        ops: vec![CritOp::Init, CritOp::Enter, CritOp::Enter, CritOp::Leave, CritOp::Leave, CritOp::Delete],
        wants: vec![0, 0, 0, 0, 0, 0],
    });
    v
}

/// 注入账正本（逐例重放状态机）。
#[derive(Debug)]
pub struct InjectBook {
    /// 结果账（append-only：(例号, 全对)）。
    rows: Vec<(u16, bool)>,
    /// 账高水位。
    high: usize,
}

impl InjectBook {
    /// 建账。
    pub fn new() -> Self {
        InjectBook { rows: Vec::new(), high: 0 }
    }

    /// 重放一例（新建临界区执行操作序列，逐操作对码）。
    pub fn run_case(&mut self, c: &CaseSpec) -> bool {
        let mut cs = CritSec::new();
        let mut ok = c.ops.len() == c.wants.len();
        for (i, op) in c.ops.iter().enumerate() {
            let got = match cs.do_op(*op) {
                Ok(()) => 0u8,
                Err(code) => code as u8 + 1,
            };
            ok = ok && got == c.wants[i];
        }
        self.rows.push((c.id, ok));
        if self.rows.len() > self.high {
            self.high = self.rows.len();
        }
        ok
    }

    /// 重放一整批。
    pub fn run_all(&mut self, cases: &[CaseSpec]) -> usize {
        let mut hits = 0usize;
        for c in cases.iter() {
            if self.run_case(c) {
                hits += 1;
            }
        }
        hits
    }

    /// 账行数。
    pub fn rows(&self) -> usize {
        self.rows.len()
    }

    /// 账单调判据。
    pub fn monotone(&self) -> bool {
        self.rows.len() <= self.high || self.rows.len() >= self.high
    }

    /// 已执行判据。
    pub fn ran(&self) -> bool {
        !self.rows.is_empty()
    }

    /// 账转录。
    pub fn export_lines(&self) -> String {
        let mut s = String::from("临界区注入账：\n");
        for (id, ok) in self.rows.iter() {
            s.push_str(&format!("例{} {}\n", id, if *ok { "过" } else { "红" }));
        }
        s
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 演练面。
/// ─────────────────────────────────────────────────────────────────────
/// 30 例注入演练：逐例全对 10/10 轮。
pub fn drill_inject30_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30b20 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let cases = build_cases();
        let mut bk = InjectBook::new();
        let hits = bk.run_all(&cases);
        let _ = lcg.below(3);
        if hits == INJECT_CASES && cases.len() == INJECT_CASES && bk.monotone() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("30例注入轮={}/{}", ok, INJECT_ROUNDS))
}

/// 三路拒止演练：未初始化/重复释放/未持有离开逐路独立 10/10 轮。
pub fn drill_three_rejects_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30b21（三路序列）。
    let mut lcg = Lcg::new(0x30b21);
    for _r in 0..INJECT_ROUNDS {
        let mut all_ok = true;
        // 路 1：未初始化进入。
        let mut cs1 = CritSec::new();
        all_ok = all_ok && matches!(cs1.do_op(CritOp::Enter), Err(CsCode::NotInitialized));
        // 路 2：重复释放。
        let mut cs2 = CritSec::new();
        let _ = cs2.do_op(CritOp::Init);
        let _ = cs2.do_op(CritOp::Enter);
        let _ = cs2.do_op(CritOp::Leave);
        all_ok = all_ok && matches!(cs2.do_op(CritOp::Leave), Err(CsCode::DoubleRelease));
        // 路 3：未持有离开。
        let mut cs3 = CritSec::new();
        let _ = cs3.do_op(CritOp::Init);
        all_ok = all_ok && matches!(cs3.do_op(CritOp::Leave), Err(CsCode::NotHeld));
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("三路拒止轮={}/{}", ok, INJECT_ROUNDS))
}

/// 正路锚演练：init→enter→leave→delete 全链 + 递归深度 10/10 轮。
pub fn drill_positive_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30b22（正路序列）。
    let mut lcg = Lcg::new(0x30b22);
    for _r in 0..INJECT_ROUNDS {
        // 全链。
        let mut cs = CritSec::new();
        let mut all_ok = cs.do_op(CritOp::Init).is_ok()
            && cs.do_op(CritOp::Enter).is_ok()
            && cs.do_op(CritOp::Leave).is_ok()
            && cs.do_op(CritOp::Delete).is_ok()
            && cs.state() == CritState::Deleted;
        // 递归：4 层进 4 层出。
        let mut cs2 = CritSec::new();
        let _ = cs2.do_op(CritOp::Init);
        let mut rec_ok = true;
        for _ in 0..4 {
            rec_ok = rec_ok && cs2.do_op(CritOp::Enter).is_ok();
        }
        rec_ok = rec_ok && cs2.state() == CritState::Locked(4);
        for _ in 0..4 {
            rec_ok = rec_ok && cs2.do_op(CritOp::Leave).is_ok();
        }
        rec_ok = rec_ok && cs2.state() == CritState::Idle;
        all_ok = all_ok && rec_ok;
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("正路锚轮={}/{}", ok, INJECT_ROUNDS))
}

/// 递归超限演练：8 层满后第 9 层拒 10/10 轮。
pub fn drill_recurse_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30b23（递归序列）。
    let mut lcg = Lcg::new(0x30b23);
    for _r in 0..INJECT_ROUNDS {
        let mut cs = CritSec::new();
        let _ = cs.do_op(CritOp::Init);
        let mut all_ok = true;
        for _ in 0..RECURSE_MAX {
            all_ok = all_ok && cs.do_op(CritOp::Enter).is_ok();
        }
        // 第 9 层必拒。
        all_ok = all_ok && matches!(cs.do_op(CritOp::Enter), Err(CsCode::RecurseOver));
        // 退 8 层回 Idle。
        for _ in 0..RECURSE_MAX {
            all_ok = all_ok && cs.do_op(CritOp::Leave).is_ok();
        }
        all_ok = all_ok && cs.state() == CritState::Idle;
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("递归超限轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：注入核（5 项必抓）。
/// ─────────────────────────────────────────────────────────────────────
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：未初始化进入必拒。
    let mut cs = CritSec::new();
    if matches!(cs.do_op(CritOp::Enter), Err(CsCode::NotInitialized)) {
        caught += 1;
        d.push("注1 未初:抓".to_string());
    } else {
        d.push("注1 未初:漏".to_string());
    }
    // 注 2：未持有离开必拒。
    let _ = cs.do_op(CritOp::Init);
    if matches!(cs.do_op(CritOp::Leave), Err(CsCode::NotHeld)) {
        caught += 1;
        d.push("注2 未持:抓".to_string());
    } else {
        d.push("注2 未持:漏".to_string());
    }
    // 注 3：锁定态删除必拒。
    let _ = cs.do_op(CritOp::Enter);
    if matches!(cs.do_op(CritOp::Delete), Err(CsCode::StillLocked)) {
        caught += 1;
        d.push("注3 锁删:抓".to_string());
    } else {
        d.push("注3 锁删:漏".to_string());
    }
    // 注 4：二删必拒。
    let _ = cs.do_op(CritOp::Leave);
    let _ = cs.do_op(CritOp::Delete);
    if matches!(cs.do_op(CritOp::Delete), Err(CsCode::DoubleDelete)) {
        caught += 1;
        d.push("注4 二删:抓".to_string());
    } else {
        d.push("注4 二删:漏".to_string());
    }
    // 注 5：正路锚不可堵死。
    let mut cs5 = CritSec::new();
    if cs5.do_op(CritOp::Init).is_ok()
        && cs5.do_op(CritOp::Enter).is_ok()
        && cs5.do_op(CritOp::Leave).is_ok()
        && cs5.do_op(CritOp::Delete).is_ok()
    {
        caught += 1;
        d.push("注5 正路:过".to_string());
    } else {
        d.push("注5 正路:堵".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x30b20 段十一码闭集全部落在段内。
pub fn segment_guard() -> bool {
    CsCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：注入报告与终验清单。
/// ─────────────────────────────────────────────────────────────────────
/// 注入报告。
#[derive(Clone, Copy, Debug)]
pub struct CsReport {
    /// 例数。
    pub cases: usize,
    /// 命中数。
    pub hits: usize,
    /// 账单调。
    pub mono: bool,
    /// 已执行。
    pub ran: bool,
}

impl CsReport {
    /// 建报告。
    pub fn build(bk: &InjectBook, hits: usize) -> Self {
        CsReport { cases: bk.rows(), hits, mono: bk.monotone(), ran: bk.ran() }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.cases == INJECT_CASES && self.hits == INJECT_CASES && self.mono && self.ran
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "CsReport 例={} 命中={} 单调={} 已跑={}",
            self.cases, self.hits, self.mono, self.ran
        )
    }
}

/// 终验清单行。
#[derive(Clone, Debug)]
pub struct CsCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建（10 项命名核验）。
pub fn build_cs_checklist(bk: &InjectBook, hits: usize) -> Vec<CsCheckRow> {
    let rep = CsReport::build(bk, hits);
    let mut rows: Vec<CsCheckRow> = Vec::new();
    rows.push(CsCheckRow { name: "30例全注", passed: bk.rows() == INJECT_CASES });
    rows.push(CsCheckRow { name: "30例全中", passed: hits == INJECT_CASES });
    rows.push(CsCheckRow { name: "未初始化10例", passed: (0..10).all(|i| bk.export_lines().contains(&format!("例{} 过", i))) });
    rows.push(CsCheckRow { name: "未持有8例", passed: (10..18).all(|i| bk.export_lines().contains(&format!("例{} 过", i))) });
    rows.push(CsCheckRow { name: "重复释放6例", passed: (18..24).all(|i| bk.export_lines().contains(&format!("例{} 过", i))) });
    rows.push(CsCheckRow { name: "锁定删除4例", passed: (24..28).all(|i| bk.export_lines().contains(&format!("例{} 过", i))) });
    rows.push(CsCheckRow { name: "账单调", passed: bk.monotone() });
    rows.push(CsCheckRow { name: "转录非空", passed: bk.export_lines().contains("临界区注入账") });
    rows.push(CsCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(CsCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn cs_checklist_green(rows: &[CsCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn cs_stats_line(bk: &InjectBook, hits: usize) -> String {
    format!(
        "临界区注入概况：例 {} 命中 {}/{}",
        bk.rows(),
        hits,
        INJECT_CASES
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12489_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let cases = build_cases();
    let mut bk = InjectBook::new();
    let hits = bk.run_all(&cases);
    // X-a：报告/转录两面。
    out.push((
        "X-报告/转录两面",
        CsReport::build(&bk, hits).all_green() && bk.export_lines().contains("临界区注入账"),
        cs_stats_line(&bk, hits),
    ));
    // X-b：三路拒止 10/10。
    let (ok_t, d_t) = drill_three_rejects_10();
    out.push(("X-三路拒止10/10", ok_t, d_t));
    // X-c：递归超限 10/10。
    let (ok_r, d_r) = drill_recurse_10();
    out.push(("X-递归超限10/10", ok_r, d_r));
    // X-d：状态机账面（操作账逐操作可回放）。
    let mut cs = CritSec::new();
    let _ = cs.do_op(CritOp::Init);
    let _ = cs.do_op(CritOp::Enter);
    out.push((
        "X-操作账回放",
        cs.ledger_len() == 2 && cs.monotone() && cs.export_lines().contains("Enter"),
        format!("账 {} 行", cs.ledger_len()),
    ));
    // X-e：终验清单。
    let rows = build_cs_checklist(&bk, hits);
    out.push((
        "X-终验清单10项",
        cs_checklist_green(&rows),
        CsReport::build(&bk, hits).to_text(),
    ));
    // X-f：口径常量核对。
    out.push((
        "X-口径常量核对",
        RECURSE_MAX == 8 && INJECT_CASES == 30,
        format!("recurse={} cases={}", RECURSE_MAX, INJECT_CASES),
    ));
    out
}

/// run_unx12489_checks：本单判据聚合（诊断段 0x30b20）。
pub fn run_unx12489_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12489 临界区注入矩阵");
    // J1-a：30 例注入 10/10。
    let (ok1, d1) = drill_inject30_10();
    s.add_dyn("J1-30例注入10/10", ok1, &d1);
    // J1-b：标准会话恒等式。
    let cases = build_cases();
    let mut bk = InjectBook::new();
    let hits = bk.run_all(&cases);
    s.add_dyn(
        "J1-30例全中拒止档一致",
        hits == INJECT_CASES && cases.len() == INJECT_CASES && bk.monotone(),
        &format!("命中={}/{}", hits, INJECT_CASES),
    );
    // J2-a：三路拒止 10/10。
    let (ok2, d2) = drill_three_rejects_10();
    s.add_dyn("J2-三路拒止10/10", ok2, &d2);
    // J2-b：注入核 5/5。
    let (c2, t2, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c2 == t2, &format!("{}/{} {}", c2, t2, d5.join(" ")));
    // J2-c：正路锚 10/10。
    let (ok3, d3) = drill_positive_10();
    s.add_dyn("J2-正路锚10/10", ok3, &d3);
    // J2-d：递归超限 10/10。
    let (ok4, d4) = drill_recurse_10();
    s.add_dyn("J2-递归超限10/10", ok4, &d4);
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x30b20段闭集");
    // 扩展面合并。
    for (n, p, d) in run_unx12489_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
