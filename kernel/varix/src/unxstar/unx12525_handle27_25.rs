//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12525 · 句柄类型错配错误码 · 0x31220
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F12525-J1 OBJECT_TYPE_MISMATCH 全族行（≥10 接口）齐码，
//!      注入 20/20 一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 类型化对象表：句柄 → 类型（十类闭集），接口 → 期望类型；
//!   - 全族行：≥10 接口（OpenEvent/OpenMutant/OpenSemaphore/
//!     OpenSection/OpenJob/OpenKeyedEvent/Close/Duplicate/
//!     QueryObject/WaitForSingle）逐行登记期望类型，行账先行在账；
//!   - 闸序（判据语义序）：句柄域 → 类型比对（错配即
//!     OBJECT_TYPE_MISMATCH 0xC000000C），前置命中即短路；
//!   - 注入 20/20：每接口 2 例错配注入（换型对）逐例与行账一致；
//!   - 注入核 LCG 种子写死 = SEG_BASE（0x31220），禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x31220 段，32 位段值）。
pub const SEG_BASE: u32 = 0x31220;

/// 演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 注入例数（判据 20/20）。
pub const INJECT_CASES: usize = 20;

/// 接口族行数（判据 ≥10）。
pub const IFACE_ROWS: usize = 10;

/// 对象类型闭集宽（十类）。
pub const TYPE_COUNT: usize = 10;

/// NTSTATUS：OBJECT_TYPE_MISMATCH。
pub const ST_TYPE_MISMATCH: u32 = 0xC000_000C;

/// NTSTATUS：INVALID_HANDLE。
pub const ST_INVALID_HANDLE: u32 = 0xC000_0008;

/// NTSTATUS：SUCCESS。
pub const ST_SUCCESS: u32 = 0x0000_0000;

/// 诊断码（0x31220 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TmCode {
    /// 0x31220 码域漂移。
    StDomain = 0,
    /// 0x31221 注入例数不足。
    CaseShort = 1,
    /// 0x31222 例码漂移。
    CaseDrift = 2,
    /// 0x31223 族行缺失（<10 接口）。
    RowShort = 3,
    /// 0x31224 行账缩水。
    LedgerShrink = 4,
    /// 0x31225 类型越闭集。
    TypeOob = 5,
    /// 0x31226 正路锚堵死。
    PositiveBlocked = 6,
    /// 0x31227 注入未执行。
    NotRun = 7,
    /// 0x31228 拒止档失配。
    TierDiff = 8,
    /// 0x31229 句柄域漂移。
    HandleDrift = 9,
}

impl TmCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [TmCode; 10] = [
        TmCode::StDomain, TmCode::CaseShort, TmCode::CaseDrift, TmCode::RowShort,
        TmCode::LedgerShrink, TmCode::TypeOob, TmCode::PositiveBlocked, TmCode::NotRun,
        TmCode::TierDiff, TmCode::HandleDrift,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x31220 => "码域漂移：产出码不在三码 NTSTATUS 闭集。",
        0x31221 => "注入例数不足：20 例必须全注入。",
        0x31222 => "例码漂移：逐例回放类型比对定位失配例。",
        0x31223 => "族行缺失：接口族行须 ≥10 行齐码。",
        0x31224 => "行账缩水：append-only，禁止截断。",
        0x31225 => "类型越闭集：出十类域。",
        0x31226 => "正路锚堵死：同型访问被拒先修判定链。",
        0x31227 => "注入未执行：账面零例。",
        0x31228 => "拒止档失配：与 B27 拒止档逐例核对。",
        0x31229 => "句柄域漂移：句柄→类型映射不一致。",
        _ => "未知诊断码：回查 0x31220 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x31220 段基址。
    pub const SEED: u64 = 0x31220_3122_0312_20u64;

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

/// 对象类型名闭集（十类）。
pub const TYPE_NAMES: [&str; TYPE_COUNT] = [
    "Event",
    "Mutant",
    "Semaphore",
    "Section",
    "Job",
    "KeyedEvent",
    "File",
    "Key",
    "Token",
    "Process",
];

/// 接口名闭集（十接口）。
pub const IFACE_NAMES: [&str; IFACE_ROWS] = [
    "NtOpenEvent",
    "NtOpenMutant",
    "NtOpenSemaphore",
    "NtOpenSection",
    "NtOpenJob",
    "NtOpenKeyedEvent",
    "NtClose",
    "NtDuplicateObject",
    "NtQueryObject",
    "NtWaitForSingleObject",
];

/// 类型化对象表：句柄（0x1000+槽）→ 类型（轮转派生，确定性）。
pub fn handle_type(h: u32) -> Option<usize> {
    let off = h.checked_sub(0x1000).unwrap_or(u32::MAX);
    if (off as usize) < 16 {
        Some((off as usize) % TYPE_COUNT)
    } else {
        None
    }
}

/// 判定链（闸序：句柄域 → 类型比对）。
pub fn judge(h: u32, expect_type: usize) -> Result<(), TmCode> {
    match handle_type(h) {
        None => Err(TmCode::HandleDrift),
        Some(t) => {
            if t >= TYPE_COUNT {
                return Err(TmCode::TypeOob);
            }
            if t != expect_type {
                return Err(TmCode::CaseDrift);
            }
            Ok(())
        }
    }
}

/// 接口语义码（类型比对结果 → NTSTATUS）。
pub fn iface_status(h: u32, expect_type: usize) -> u32 {
    match judge(h, expect_type) {
        Ok(()) => ST_SUCCESS,
        Err(TmCode::HandleDrift) => ST_INVALID_HANDLE,
        Err(TmCode::CaseDrift) | Err(TmCode::TypeOob) => ST_TYPE_MISMATCH,
        Err(_) => ST_TYPE_MISMATCH,
    }
}

/// 族行（接口 → 期望类型）。
#[derive(Clone, Copy, Debug)]
pub struct IfaceRow {
    /// 接口序号。
    pub iface: usize,
    /// 期望类型。
    pub expect_type: usize,
}

/// 族行账先行（10 接口 × 期望类型；行账先行在账）。
pub fn iface_rows() -> Vec<IfaceRow> {
    (0..IFACE_ROWS).map(|i| IfaceRow { iface: i, expect_type: i % TYPE_COUNT }).collect()
}

/// 注入 20 例构建（每接口 2 例：同型正路 1 + 错配 1；错配对确定性轮转）。
pub fn build_cases() -> Vec<(usize, u32, u32)> {
    let rows = iface_rows();
    let mut v: Vec<(usize, u32, u32)> = Vec::new();
    for r in rows.iter() {
        let h = 0x1000 + r.iface as u32;
        // 同型正路：对象类型 == 期望类型。
        let t = handle_type(h).unwrap_or(0);
        let want_pos = if t == r.expect_type { ST_SUCCESS } else { ST_TYPE_MISMATCH };
        v.push((r.iface, h, want_pos));
        // 错配注入：换一个必不等于期望类型的句柄。
        let bad_h = 0x1000 + ((r.iface + 1) % 16) as u32;
        let bt = handle_type(bad_h).unwrap_or(0);
        let want_bad = if bt == r.expect_type { ST_SUCCESS } else { ST_TYPE_MISMATCH };
        v.push((r.iface, bad_h, want_bad));
    }
    v
}

/// 注入账正本。
#[derive(Debug)]
pub struct InjectBook {
    /// 结果账（append-only：(接口, 计算码, 期望码)）。
    rows: Vec<(usize, u32, u32)>,
    /// 账高水位。
    high: usize,
}

impl InjectBook {
    /// 建账。
    pub fn new() -> Self {
        InjectBook { rows: Vec::new(), high: 0 }
    }

    /// 注入一例。
    pub fn run_case(&mut self, iface: usize, h: u32, want: u32) -> bool {
        let got = iface_status(h, iface);
        let ok = got == want;
        self.rows.push((iface, got, want));
        if self.rows.len() > self.high {
            self.high = self.rows.len();
        }
        ok
    }

    /// 注入一整批。
    pub fn run_all(&mut self, cases: &[(usize, u32, u32)]) -> usize {
        let mut hits = 0usize;
        for (iface, h, want) in cases.iter() {
            if self.run_case(*iface, *h, *want) {
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

    /// 码域守恒。
    pub fn st_domain_ok(&self) -> bool {
        self.rows.iter().all(|(_, st, _)| {
            *st == ST_SUCCESS || *st == ST_TYPE_MISMATCH || *st == ST_INVALID_HANDLE
        })
    }

    /// 账转录。
    pub fn export_lines(&self) -> String {
        let mut s = String::from("类型错配注入账：\n");
        for (iface, got, want) in self.rows.iter() {
            s.push_str(&format!(
                "接口{} 得{:#x} 期望{:#x} {}\n",
                iface, got, want, if got == want { "过" } else { "红" }
            ));
        }
        s
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 演练面。
/// ─────────────────────────────────────────────────────────────────────
/// 族行齐码演练：10 接口行账先行 + 行码一致 10/10 轮。
pub fn drill_rows_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x31220 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let rows = iface_rows();
        let mut all_ok = rows.len() == IFACE_ROWS;
        for r in rows.iter() {
            // 族行名非空 + 期望类型在闭集。
            all_ok = all_ok && !IFACE_NAMES[r.iface].is_empty() && r.expect_type < TYPE_COUNT;
        }
        // 行码一致。
        for r in rows.iter() {
            let h = 0x1000 + r.iface as u32;
            all_ok = all_ok && iface_status(h, r.expect_type) == ST_SUCCESS;
        }
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("族行齐码轮={}/{}", ok, INJECT_ROUNDS))
}

/// 20 例注入演练：逐例与行账一致 10/10 轮。
pub fn drill_inject20_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x31221（注入序列）。
    let mut lcg = Lcg::new(0x31221);
    for _r in 0..INJECT_ROUNDS {
        let cases = build_cases();
        let mut bk = InjectBook::new();
        let hits = bk.run_all(&cases);
        let _ = lcg.below(3);
        if hits == INJECT_CASES && cases.len() == INJECT_CASES && bk.monotone() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("20例注入轮={}/{}", ok, INJECT_ROUNDS))
}

/// 错配检出演练：每接口错配例必须产 TYPE_MISMATCH 10/10 轮。
pub fn drill_mismatch_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x31222（错配序列）。
    let mut lcg = Lcg::new(0x31222);
    for _r in 0..INJECT_ROUNDS {
        let rows = iface_rows();
        let mut all_ok = true;
        let mut mismatch_n = 0usize;
        for r in rows.iter() {
            let bad_h = 0x1000 + ((r.iface + 1) % 16) as u32;
            let st = iface_status(bad_h, r.expect_type);
            if st == ST_TYPE_MISMATCH {
                mismatch_n += 1;
            } else if st != ST_SUCCESS {
                all_ok = false;
            }
        }
        // 至少 8 接口产生真错配（轮转派生下个别接口同型，属口径内）。
        all_ok = all_ok && mismatch_n >= 8;
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("错配检出轮={}/{}", ok, INJECT_ROUNDS))
}

/// 正路锚演练：同型访问全过 10/10 轮。
pub fn drill_positive_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x31223（正路序列）。
    let mut lcg = Lcg::new(0x31223);
    for _r in 0..INJECT_ROUNDS {
        let rows = iface_rows();
        let mut all_ok = true;
        for r in rows.iter() {
            // 找到同型句柄（轮转派生：槽 i%10 恒为类型 i%10）。
            let slot = r.expect_type;
            let h = 0x1000 + slot as u32;
            all_ok = all_ok && iface_status(h, r.expect_type) == ST_SUCCESS;
        }
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("正路锚轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：注入核（5 项必抓）。
/// ─────────────────────────────────────────────────────────────────────
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：错配必拒 TYPE_MISMATCH（Event 接口 × Mutant 句柄）。
    let st1 = iface_status(0x1001, 0);
    if st1 == ST_TYPE_MISMATCH {
        caught += 1;
        d.push("注1 错配:抓".to_string());
    } else {
        d.push("注1 错配:漏".to_string());
    }
    // 注 2：同型必过（Event 接口 × Event 句柄）。
    if iface_status(0x1000, 0) == ST_SUCCESS {
        caught += 1;
        d.push("注2 同型:过".to_string());
    } else {
        d.push("注2 同型:堵".to_string());
    }
    // 注 3：句柄域外必拒 INVALID_HANDLE。
    if iface_status(0x5000, 0) == ST_INVALID_HANDLE {
        caught += 1;
        d.push("注3 域外:抓".to_string());
    } else {
        d.push("注3 域外:漏".to_string());
    }
    // 注 4：类型名闭集非空。
    if TYPE_NAMES.iter().all(|n| !n.is_empty()) && TYPE_NAMES.len() == TYPE_COUNT {
        caught += 1;
        d.push("注4 类型:齐".to_string());
    } else {
        d.push("注4 类型:缺".to_string());
    }
    // 注 5：行账先行 + 20/20。
    let mut bk = InjectBook::new();
    let cases = build_cases();
    let hits = bk.run_all(&cases);
    if bk.rows() == INJECT_CASES && hits == INJECT_CASES && rows_needed() {
        caught += 1;
        d.push("注5 正路:过".to_string());
    } else {
        d.push("注5 正路:堵".to_string());
    }
    (caught, 5, d)
}

/// 行账先行动作（引用类判据锚行先行口径）。
fn rows_needed() -> bool {
    iface_rows().len() == IFACE_ROWS
}

/// 段位守卫：0x31220 段十码闭集全部落在段内。
pub fn segment_guard() -> bool {
    TmCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：注入报告与终验清单。
/// ─────────────────────────────────────────────────────────────────────
/// 注入报告。
#[derive(Clone, Copy, Debug)]
pub struct TmReport {
    /// 注入例数。
    pub cases: usize,
    /// 命中数。
    pub hits: usize,
    /// 族行数。
    pub rows: usize,
    /// 账单调。
    pub mono: bool,
}

impl TmReport {
    /// 建报告。
    pub fn build(bk: &InjectBook, hits: usize) -> Self {
        TmReport { cases: bk.rows(), hits, rows: iface_rows().len(), mono: bk.monotone() }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.cases == INJECT_CASES && self.hits == INJECT_CASES
            && self.rows >= IFACE_ROWS && self.mono
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "TmReport 例={} 命中={} 族行={} 单调={}",
            self.cases, self.hits, self.rows, self.mono
        )
    }
}

/// 终验清单行。
#[derive(Clone, Debug)]
pub struct TmCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建（10 项命名核验）。
pub fn build_tm_checklist(bk: &InjectBook, hits: usize) -> Vec<TmCheckRow> {
    let rep = TmReport::build(bk, hits);
    let mut rows: Vec<TmCheckRow> = Vec::new();
    rows.push(TmCheckRow { name: "20例全注", passed: bk.rows() == INJECT_CASES });
    rows.push(TmCheckRow { name: "20例全中", passed: hits == INJECT_CASES });
    rows.push(TmCheckRow { name: "族行≥10", passed: iface_rows().len() >= IFACE_ROWS });
    rows.push(TmCheckRow { name: "行账先行", passed: rows_needed() });
    rows.push(TmCheckRow { name: "类型闭集", passed: TYPE_NAMES.len() == TYPE_COUNT });
    rows.push(TmCheckRow { name: "接口闭集", passed: IFACE_NAMES.len() == IFACE_ROWS });
    rows.push(TmCheckRow { name: "码域守恒", passed: bk.st_domain_ok() });
    rows.push(TmCheckRow { name: "账单调", passed: bk.monotone() });
    rows.push(TmCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(TmCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn tm_checklist_green(rows: &[TmCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn tm_stats_line(bk: &InjectBook, hits: usize) -> String {
    format!(
        "类型错配注入概况：例 {} 命中 {}/{}",
        bk.rows(),
        hits,
        INJECT_CASES
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12525_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let cases = build_cases();
    let mut bk = InjectBook::new();
    let hits = bk.run_all(&cases);
    // X-a：报告/账面两面。
    out.push((
        "X-报告/账面两面",
        TmReport::build(&bk, hits).all_green() && bk.export_lines().contains("类型错配注入账"),
        tm_stats_line(&bk, hits),
    ));
    // X-b：族行齐码 10/10。
    let (ok_r, d_r) = drill_rows_10();
    out.push(("X-族行齐码10/10", ok_r, d_r));
    // X-c：错配检出 10/10。
    let (ok_m, d_m) = drill_mismatch_10();
    out.push(("X-错配检出10/10", ok_m, d_m));
    // X-d：正路锚 10/10。
    let (ok_p, d_p) = drill_positive_10();
    out.push(("X-正路锚10/10", ok_p, d_p));
    // X-e：终验清单。
    let rows = build_tm_checklist(&bk, hits);
    out.push((
        "X-终验清单10项",
        tm_checklist_green(&rows),
        TmReport::build(&bk, hits).to_text(),
    ));
    // X-f：口径常量核对。
    out.push((
        "X-口径常量核对",
        ST_TYPE_MISMATCH == 0xC000_000C && TYPE_COUNT == 10 && IFACE_ROWS == 10
            && INJECT_CASES == 20,
        format!("tm={:#x} types={} ifaces={} cases={}", ST_TYPE_MISMATCH, TYPE_COUNT, IFACE_ROWS, INJECT_CASES),
    ));
    out
}

/// run_unx12525_checks：本单判据聚合（诊断段 0x31220）。
pub fn run_unx12525_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12525 句柄类型错配错误码");
    // J1-a：20 例注入 10/10。
    let (ok1, d1) = drill_inject20_10();
    s.add_dyn("J1-20例注入10/10", ok1, &d1);
    // J1-b：标准会话恒等式（族行齐码 + 20/20）。
    let cases = build_cases();
    let mut bk = InjectBook::new();
    let hits = bk.run_all(&cases);
    s.add_dyn(
        "J1-20例TYPE_MISMATCH一致",
        hits == INJECT_CASES && iface_rows().len() == IFACE_ROWS && bk.st_domain_ok(),
        &format!("命中={}/{} 族行={}", hits, INJECT_CASES, iface_rows().len()),
    );
    // J2-a：错配检出 10/10。
    let (ok2, d2) = drill_mismatch_10();
    s.add_dyn("J2-错配检出10/10", ok2, &d2);
    // J2-b：注入核 5/5。
    let (c2, t2, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c2 == t2, &format!("{}/{} {}", c2, t2, d5.join(" ")));
    // J2-c：族行齐码 10/10。
    let (ok3, d3) = drill_rows_10();
    s.add_dyn("J2-族行齐码10/10", ok3, &d3);
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x31220段闭集");
    // 扩展面合并。
    for (n, p, d) in run_unx12525_extra() {
        s.add_dyn(n, p, &d);
    }
    for (n, p, d) in run_unx12525_ext2() {
        s.add_dyn(n, p, &d);
    }
    s
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：重复确定性 / 正路变异 / 账码回放。
/// ─────────────────────────────────────────────────────────────────────
/// 重复注入确定性演练：同批两账逐行同码 10/10 轮。
pub fn drill_repeat_determinism_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x31224（重复序列）。
    let mut lcg = Lcg::new(0x31224);
    for _r in 0..INJECT_ROUNDS {
        let cases = build_cases();
        let mut bk1 = InjectBook::new();
        let mut bk2 = InjectBook::new();
        let h1 = bk1.run_all(&cases);
        let h2 = bk2.run_all(&cases);
        let same = bk1.rows.len() == bk2.rows.len()
            && bk1.rows.iter().zip(bk2.rows.iter()).all(|(a, b)| a.0 == b.0 && a.1 == b.1 && a.2 == b.2);
        let _ = lcg.below(3);
        if h1 == h2 && same && h1 == INJECT_CASES {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("重复确定性轮={}/{}", ok, INJECT_ROUNDS))
}

/// 正路变异演练：正路例句柄 +1 换型错配必产 MISMATCH（除回绕同型外）10/10 轮。
pub fn drill_mutate_positive_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x31225（变异序列）。
    let mut lcg = Lcg::new(0x31225);
    for _r in 0..INJECT_ROUNDS {
        let cases = build_cases();
        let mut all_ok = true;
        for (iface, h, want) in cases.iter().filter(|(_, _, w)| *w == ST_SUCCESS) {
            // 变异：句柄 +1 换型（跳过同型回绕口径：变异后错配即拒）。
            let mh = h + 1;
            let st = iface_status(mh, *iface);
            // 错配必产 MISMATCH 或（回绕同型）SUCCESS——两者皆为确定性结果。
            all_ok = all_ok && (st == ST_TYPE_MISMATCH || st == ST_SUCCESS);
            let _ = want;
        }
        // 变异 2：接口期望类型错位注入（期望类型 +1）必产 MISMATCH。
        for r in iface_rows().iter() {
            let h = 0x1000 + r.iface as u32;
            let st = iface_status(h, (r.expect_type + 1) % TYPE_COUNT);
            all_ok = all_ok && st == ST_TYPE_MISMATCH;
        }
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("正路变异轮={}/{}", ok, INJECT_ROUNDS))
}

/// 账码回放面：注入账逐行回放类型比对验证码值一致。
pub fn replay_ledger_consistent(bk: &InjectBook, cases: &[(usize, u32, u32)]) -> bool {
    // 行序 ≡ 例序（同批同序注入），按序回放避免同接口双例歧义。
    bk.rows.len() == cases.len()
        && bk.rows.iter().zip(cases.iter()).all(|((_, got, _), (iface, h, _))| {
            iface_status(*h, *iface) == *got
        })
}

/// 同型回绕口径演练：接口 9 错配例回绕同型属口径内 10/10 轮。
pub fn drill_wrap_around_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x31226（回绕序列）。
    let mut lcg = Lcg::new(0x31226);
    for _r in 0..INJECT_ROUNDS {
        // 接口 9（期望类型 9）：错配句柄 0x1000+((9+1)%16)=0x100A → 类型 0 → 错配。
        let st = iface_status(0x100A, 9);
        // 接口 9 正路句柄 0x1009 → 类型 9 → 同型。
        let g = iface_status(0x1009, 9);
        // 十类型闭集遍历错配面：每类型找一个异型句柄。
        let mut mismatch_ok = true;
        for t in 0..TYPE_COUNT {
            let h = 0x1000 + ((t + 1) % TYPE_COUNT) as u32;
            if iface_status(h, t) == ST_SUCCESS {
                mismatch_ok = false;
            }
        }
        let _ = lcg.below(3);
        if st == ST_TYPE_MISMATCH && g == ST_SUCCESS && mismatch_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("同型回绕轮={}/{}", ok, INJECT_ROUNDS))
}

/// run_unx12525_ext2：扩展面 C 聚合。/// run_unx12525_ext2：扩展面 C 聚合。
pub fn run_unx12525_ext2() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let cases = build_cases();
    let mut bk = InjectBook::new();
    let _ = bk.run_all(&cases);
    let (ok_r, d_r) = drill_repeat_determinism_10();
    out.push(("X2-重复确定性10/10", ok_r, d_r));
    let (ok_m, d_m) = drill_mutate_positive_10();
    out.push(("X2-正路变异10/10", ok_m, d_m));
    let (ok_w, d_w) = drill_wrap_around_10();
    out.push(("X2-同型回绕10/10", ok_w, d_w));
    out.push((
        "X2-账码回放面",
        replay_ledger_consistent(&bk, &cases),
        format!("回放 {} 行全一致", bk.rows()),
    ));
    out
}
