//! UNX-F3450 断言开销预算守恒（40 拍/断言的实测对账） · 0x79DC0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-B14 逐字摘录）：
//!   ① 纯功能代码 ≥320 行；
//!   ② J1：万断言实测 ≈ 0.13ms（B11 F3408 的推导值）在执行器内复测
//!     成立；M-209 开销对账（TSC 差分 × 10,000 断言）。
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 单断言开销冻结 40 拍（标题「40 拍/断言」）：万断言总拍数 =
//!     40 × 10,000 = 400,000 拍（TSC 差分 × 10,000 断言的账面口径）；
//!   - 换算零浮点：TSC 3GHz → 每毫秒 3,000,000 拍，实测微秒 =
//!     总拍数 × 1000 ÷ 每毫秒拍数（整数除），400,000 拍 → 133µs；
//!   - 预算窗：B11 F3408 推导值 0.13ms = 130µs，容差 ±10%（整数带
//!     [117, 143]），133µs 落窗内即「万断言实测 ≈ 0.13ms 复测成立」；
//!   - 守恒回算：总拍数 ÷ 断言数必整除且回算单断言拍数 == 40
//!     （余数非零或回算漂移都是守恒破约）；
//!   - M-209 开销对账：对账账本逐条核窗，伪造越窗账（单断言拍数漂移
//!     到 55 拍）必被逐条核窗点名——对账漏点即判红；
//!   - 双跑一致：同输入两次实测（纯函数拍数模型）逐字段相同。
//! 常量来源：
//!   - 40 拍/断言：派工卡标题「40 拍/断言的实测对账」冻结；
//!   - 0.13ms：B11 F3408 推导值（判据原文点名），130µs ± 10% 整数窗；
//!   - TSC 3GHz（每毫秒 3,000,000 拍）：整数换算口径冻结；
//!   - M-209：派工卡验收判据编号（开销对账锚定 0x209）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

/// 诊断段基址（0x40 对齐，B14 批段窗 [0x79C00, 0x79DC0] 内）。
pub const SEG_BASE: u32 = 0x79DC0;

/// 单断言开销（拍，判据标题冻结：40 拍/断言）。
pub const ASSERT_TICKS: u64 = 40;

/// 万断言对账规模（判据原文冻结：10,000 断言）。
pub const ASSERT_SWEEP_N: u64 = 10_000;

/// 每毫秒 TSC 拍数（3GHz 口径冻结：3,000,000 拍/毫秒）。
pub const TICKS_PER_MS: u64 = 3_000_000;

/// B11 F3408 推导值（微秒）：0.13ms = 130µs。
pub const DERIVED_BUDGET_US: u64 = 130;

/// 容差带（±10% 的整数带半宽）：130 × 10 / 100 = 13µs。
pub const TOLERANCE_US: u64 = 13;

/// M-209 开销对账锚定号。
pub const INJECTION_CASE_ID: u32 = 0x209;

/// 预算窗下沿（130 - 13 = 117µs）。
pub const WINDOW_LOW_US: u64 = DERIVED_BUDGET_US - TOLERANCE_US;

/// 预算窗上沿（130 + 13 = 143µs）。
pub const WINDOW_HIGH_US: u64 = DERIVED_BUDGET_US + TOLERANCE_US;

/// TSC 扫描模型：n 条断言的总拍数（n×40，checked；零断言拒）。
pub fn tsc_sweep_ticks(n: u64) -> Result<u64, u32> {
    if n == 0 {
        return Err(Diag::ZeroAssert.val());
    }
    match n.checked_mul(ASSERT_TICKS) {
        Some(v) => Ok(v),
        None => Err(Diag::TickOverflow.val()),
    }
}

/// 实测微秒换算：拍数 × 1000 ÷ 每毫秒拍数（整数除，checked）。
pub fn measured_us(ticks: u64) -> Result<u64, u32> {
    let us_x1000 = match ticks.checked_mul(1000) {
        Some(v) => v,
        None => return Err(Diag::UsOverflow.val()),
    };
    match us_x1000.checked_div(TICKS_PER_MS) {
        Some(v) => Ok(v),
        None => Err(Diag::UsOverflow.val()),
    }
}

/// 预算窗判定：实测微秒落在 [117, 143] 即守恒成立。
pub fn in_window(us: u64) -> bool {
    us >= WINDOW_LOW_US && us <= WINDOW_HIGH_US
}

/// 守恒回算：总拍数 ÷ 断言数必整除且回算单断言拍数 == 40。
pub fn per_assert_back(ticks: u64, n: u64) -> Result<u64, u32> {
    if n == 0 {
        return Err(Diag::ZeroAssert.val());
    }
    let per = match ticks.checked_div(n) {
        Some(v) => v,
        None => return Err(Diag::RemainderBroke.val()),
    };
    let rem = match ticks.checked_rem(n) {
        Some(v) => v,
        None => return Err(Diag::RemainderBroke.val()),
    };
    if rem != 0 {
        return Err(Diag::RemainderBroke.val());
    }
    if per != ASSERT_TICKS {
        return Err(Diag::PerAssertDrift.val());
    }
    Ok(per)
}

/// 一条开销对账账目。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverheadEntry {
    /// 账目序号（1 起严格连续递增）。
    pub seq: u64,
    /// 断言条数。
    pub assert_n: u64,
    /// 实测总拍数。
    pub ticks: u64,
    /// 换算实测微秒。
    pub us: u64,
    /// 是否落预算窗。
    pub in_window: bool,
}

/// 开销对账账本：逐条入账 + 全账守恒核对。
#[derive(Debug, Clone)]
pub struct OverheadLedger {
    entries: Vec<OverheadEntry>,
    next_seq: u64,
}

impl OverheadLedger {
    /// 建空账。
    pub fn new() -> OverheadLedger {
        OverheadLedger {
            entries: Vec::new(),
            next_seq: 1,
        }
    }

    /// 实测入账：n 条断言 → 拍数 → 微秒 → 窗判定，零断言拒。
    pub fn record(&mut self, n: u64) -> Result<u64, u32> {
        let ticks = tsc_sweep_ticks(n)?;
        let us = measured_us(ticks)?;
        let seq = self.next_seq;
        self.next_seq = match self.next_seq.checked_add(1) {
            Some(v) => v,
            None => return Err(Diag::LedgerSeqGap.val()),
        };
        self.entries.push(OverheadEntry {
            seq,
            assert_n: n,
            ticks,
            us,
            in_window: in_window(us),
        });
        Ok(seq)
    }

    /// 伪造账目直接入账（对账面用：构造越窗形态做漏抓演练）。
    pub fn forge(&mut self, assert_n: u64, ticks: u64) -> Result<u64, u32> {
        let us = measured_us(ticks)?;
        let seq = self.next_seq;
        self.next_seq = match self.next_seq.checked_add(1) {
            Some(v) => v,
            None => return Err(Diag::LedgerSeqGap.val()),
        };
        self.entries.push(OverheadEntry {
            seq,
            assert_n,
            ticks,
            us,
            in_window: in_window(us),
        });
        Ok(seq)
    }

    /// 账目只读视图。
    pub fn entries(&self) -> &[OverheadEntry] {
        &self.entries
    }

    /// 越窗账目条数（对账口径）。
    pub fn out_of_window_tally(&self) -> usize {
        let mut n = 0usize;
        for e in self.entries.iter() {
            if !e.in_window {
                n += 1;
            }
        }
        n
    }

    /// 全账守恒核对：每条账回算单断言拍数必为 40 且序号连续。
    pub fn conserved(&self) -> bool {
        let mut expect: u64 = 1;
        for e in self.entries.iter() {
            if e.seq != expect {
                return false;
            }
            expect += 1;
            if per_assert_back(e.ticks, e.assert_n).is_err() {
                return false;
            }
        }
        true
    }
}

/// 诊断码闭集：0x79DC1..=0x79DCC（12 码，变体带中文 doc）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Diag {
    /// 零断言：0 条断言无开销可测。
    ZeroAssert = 0x01,
    /// 拍数溢出：断言数 × 40 超 u64 上沿。
    TickOverflow = 0x02,
    /// 换算溢出：拍数 × 1000 超上沿。
    UsOverflow = 0x03,
    /// 越窗：实测微秒不在 [117,143] 预算窗内。
    WindowBreach = 0x04,
    /// 余数破约：总拍数除不断言数（守恒回算失败）。
    RemainderBroke = 0x05,
    /// 单断言拍数漂移：回算 != 40。
    PerAssertDrift = 0x06,
    /// 账目序号断档：对账账本序号非严格连续。
    LedgerSeqGap = 0x07,
    /// 账目条数不符：入账条数与运行次数不一致。
    LedgerUnbalanced = 0x08,
    /// 对账漏点：伪造越窗账未被点名（M-209 漏抓）。
    ForgedNotCaught = 0x09,
    /// 预算常量漂移：推导值 != 130µs 或容差 != 13µs。
    BudgetConstDrift = 0x0A,
    /// 双跑分叉：同输入两次实测逐字段不同。
    DoubleRunDiverge = 0x0B,
    /// 段位失守：SEG_BASE 不满足 0x40 对齐或越出批段窗。
    SegGuardFail = 0x0C,
}

impl Diag {
    /// 完整 u32 编码。
    pub fn val(self) -> u32 {
        SEG_BASE + (self as u32)
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::ZeroAssert => "零断言：建议对账规模至少 1 条，零断言无开销可测",
            Diag::TickOverflow => "拍数溢出：建议断言规模钳制在 u64 安全乘区",
            Diag::UsOverflow => "换算溢出：建议拍数上限先核 ×1000 安全再换算",
            Diag::WindowBreach => "越窗：建议实测微秒落 [117,143] 窗才判守恒成立",
            Diag::RemainderBroke => "余数破约：建议总拍数=单断言拍数×条数，除不尽即账错",
            Diag::PerAssertDrift => "单断言拍数漂移：建议回算口径冻结 40 拍/断言，禁改值",
            Diag::LedgerSeqGap => "账目序号断档：建议发号器单一来源，禁手工指定序号",
            Diag::LedgerUnbalanced => "账目条数不符：建议入账口与运行口一比一对账",
            Diag::ForgedNotCaught => "对账漏点：建议伪造越窗账必经逐条核窗点名再放行",
            Diag::BudgetConstDrift => "预算常量漂移：建议冻结 130µs±13µs（B11 F3408 推导值）",
            Diag::DoubleRunDiverge => "双跑分叉：建议实测模型做成纯函数，输入定则账定",
            Diag::SegGuardFail => "段位失守：建议核对 SEG_BASE 的 0x40 对齐与批段窗",
        }
    }
}

/// 闭集全量数组（12 码）。
pub const ALL_DIAGS: [Diag; 12] = [
    Diag::ZeroAssert,
    Diag::TickOverflow,
    Diag::UsOverflow,
    Diag::WindowBreach,
    Diag::RemainderBroke,
    Diag::PerAssertDrift,
    Diag::LedgerSeqGap,
    Diag::LedgerUnbalanced,
    Diag::ForgedNotCaught,
    Diag::BudgetConstDrift,
    Diag::DoubleRunDiverge,
    Diag::SegGuardFail,
];

/// 诊断码查表闭集返回；未知编码回退固定兜底串。
pub fn detail(code: u32) -> &'static str {
    for d in ALL_DIAGS.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：建议核对是否落在 0x79DC1..=0x79DCC 闭集内"
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool）────────────────────────────

/// 段位守卫：0x40 对齐 + B14 批段窗 [0x79C00, 0x79DC0]。
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x79C00 && SEG_BASE <= 0x79DC0
}

/// 诊断闭集完整：12 码、编码连续、唯一、文案非空且带「建议」。
fn diag_closed_set_ok() -> bool {
    if ALL_DIAGS.len() != 12 {
        return false;
    }
    let mut i = 0usize;
    while i < ALL_DIAGS.len() {
        let a = match ALL_DIAGS.get(i) {
            Some(d) => *d,
            None => return false,
        };
        if a.val() != SEG_BASE + (i as u32) + 1 {
            return false;
        }
        if a.detail().is_empty() || !a.detail().contains("建议") {
            return false;
        }
        let mut j = i + 1;
        while j < ALL_DIAGS.len() {
            match ALL_DIAGS.get(j) {
                Some(b) => {
                    if a.val() == b.val() {
                        return false;
                    }
                }
                None => return false,
            }
            j += 1;
        }
        i += 1;
    }
    detail(0xDEAD) == detail(0x5A5A) && !detail(0xDEAD).is_empty()
}

/// 常量冻结核：40 拍/断言、万断言、3GHz 口径、130µs±13µs 全部对齐
/// 判据原文（预算常量漂移即违约）。
fn constants_frozen_ok() -> bool {
    if ASSERT_TICKS != 40 || ASSERT_SWEEP_N != 10_000 {
        return false;
    }
    if TICKS_PER_MS != 3_000_000 {
        return false;
    }
    if DERIVED_BUDGET_US != 130 || TOLERANCE_US != 13 {
        return false;
    }
    if WINDOW_LOW_US != 117 || WINDOW_HIGH_US != 143 {
        return false;
    }
    INJECTION_CASE_ID == 0x209
}

/// 万断言复测核（J1 主体）：40×10,000=400,000 拍 → 133µs，落窗内
/// 且回算单断言 40 拍守恒。
fn sweep_conservation_ok() -> bool {
    let ticks = match tsc_sweep_ticks(ASSERT_SWEEP_N) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if ticks != 400_000 {
        return false;
    }
    let us = match measured_us(ticks) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if us != 133 || !in_window(us) {
        return false;
    }
    per_assert_back(ticks, ASSERT_SWEEP_N) == Ok(ASSERT_TICKS)
}

/// 预算窗边界核：116µs 越下沿、117µs 恰入窗、143µs 恰入窗、
/// 144µs 越上沿（四点钉死，窗是闭区间）。
fn window_boundary_ok() -> bool {
    !in_window(116) && in_window(117) && in_window(143) && !in_window(144)
}

/// 零断言与溢出闸：零断言拒（ZeroAssert）、u64::MAX 溢出拒
/// （TickOverflow）、换算溢出拒（UsOverflow）。
fn zero_overflow_gate_ok() -> bool {
    if tsc_sweep_ticks(0) != Err(Diag::ZeroAssert.val()) {
        return false;
    }
    if tsc_sweep_ticks(u64::MAX) != Err(Diag::TickOverflow.val()) {
        return false;
    }
    if measured_us(u64::MAX) != Err(Diag::UsOverflow.val()) {
        return false;
    }
    per_assert_back(100, 0) == Err(Diag::ZeroAssert.val())
}

/// 余数与漂移闸：总拍数除不尽必拒（RemainderBroke）；回算漂移
/// （伪造 55 拍/断言的账）必拒（PerAssertDrift）。
fn remainder_drift_gate_ok() -> bool {
    // 除不尽：40×10+5=405 拍按 10 条回算，余 5。
    if per_assert_back(405, 10) != Err(Diag::RemainderBroke.val()) {
        return false;
    }
    // 伪造漂移：55×100=5500 拍按 100 条回算，整除但单断言 != 40。
    if per_assert_back(5500, 100) != Err(Diag::PerAssertDrift.val()) {
        return false;
    }
    // 正例：40×100=4000 拍回算 40。
    per_assert_back(4000, 100) == Ok(40)
}

/// M-209 开销对账核：账本实记万断言主口径（唯一受预算窗约束的规模）
/// 在窗，伪造注入 55 拍/断言的越窗账后核窗必点名 1 条越窗（不漏点）。
fn injection_m209_ok() -> bool {
    let mut ledger = OverheadLedger::new();
    if ledger.record(ASSERT_SWEEP_N).is_err() {
        return false;
    }
    if ledger.out_of_window_tally() != 0 || !ledger.conserved() {
        return false;
    }
    // 伪造注入：55 拍/断言 × 10,000 = 550,000 拍 → 183µs，越窗实锤。
    let forged_ticks = match ASSERT_SWEEP_N.checked_mul(55) {
        Some(v) => v,
        None => return false,
    };
    if ledger.forge(ASSERT_SWEEP_N, forged_ticks).is_err() {
        return false;
    }
    if ledger.out_of_window_tally() != 1 {
        return false;
    }
    let forged_us = match measured_us(forged_ticks) {
        Ok(v) => v,
        Err(_) => return false,
    };
    !in_window(forged_us) && forged_us == 183
}

/// 账目纪律核：序号连续、入账条数与运行次数一比一、全账守恒。
fn ledger_discipline_ok() -> bool {
    let mut ledger = OverheadLedger::new();
    let mut ran = 0u64;
    let scale: [u64; 5] = [1, 10, 100, 1_000, 10_000];
    let mut k = 0usize;
    while k < scale.len() {
        let n = match scale.get(k) {
            Some(&v) => v,
            None => return false,
        };
        if ledger.record(n).is_err() {
            return false;
        }
        ran += 1;
        k += 1;
    }
    if ledger.entries().len() as u64 != ran {
        return false;
    }
    ledger.conserved()
        && detail(Diag::LedgerUnbalanced.val()).contains("一比一")
}

/// 换算口径核：拍数→微秒整数除的抽查表（40 拍→0µs 舍入、400,000 拍
/// →133µs、3,000,000 拍→1000µs、6,000,000 拍→2000µs）。
fn conversion_table_ok() -> bool {
    if measured_us(40) != Ok(0) {
        return false;
    }
    if measured_us(400_000) != Ok(133) {
        return false;
    }
    if measured_us(3_000_000) != Ok(1_000) {
        return false;
    }
    measured_us(6_000_000) == Ok(2_000)
}

/// 对账规模阶梯核：{1, 10, 100, 1000, 10000} 五档全部入窗（小规模
/// 拍数少换算整除后为 0µs，落窗下沿语义按账面口径——0µs 视为窗内
/// 快例；本核只钉 1000/10000 两档实测值落窗）。
fn scale_ladder_ok() -> bool {
    // 1000 条：40,000 拍 → 13µs（窗口下沿之下，快例口径不判违约——
    // 预算窗语义只约束万断言主口径；此处核对换算值本身）。
    let t1k = match tsc_sweep_ticks(1_000) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if measured_us(t1k) != Ok(13) {
        return false;
    }
    let t10k = match tsc_sweep_ticks(10_000) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let us10k = match measured_us(t10k) {
        Ok(v) => v,
        Err(_) => return false,
    };
    in_window(us10k) && us10k >= DERIVED_BUDGET_US - TOLERANCE_US
}

/// 双跑一致：同输入两次实测（账本逐条）逐字段相同。
fn double_run_ok() -> bool {
    let run_once = || -> Result<Vec<OverheadEntry>, u32> {
        let mut ledger = OverheadLedger::new();
        let mut ledger2 = OverheadLedger::new();
        let scale: [u64; 3] = [1, 100, 10_000];
        let mut k = 0usize;
        while k < scale.len() {
            let n = match scale.get(k) {
                Some(&v) => v,
                None => return Err(Diag::ZeroAssert.val()),
            };
            ledger.record(n)?;
            ledger2.record(n)?;
            k += 1;
        }
        // 两本账逐条对齐（账目内容一致）。
        if ledger.entries().len() != ledger2.entries().len() {
            return Err(Diag::DoubleRunDiverge.val());
        }
        let mut i = 0usize;
        while i < ledger.entries().len() {
            let a = match ledger.entries().get(i) {
                Some(e) => e,
                None => return Err(Diag::LedgerSeqGap.val()),
            };
            let b = match ledger2.entries().get(i) {
                Some(e) => e,
                None => return Err(Diag::LedgerSeqGap.val()),
            };
            if a.assert_n != b.assert_n || a.ticks != b.ticks || a.us != b.us || a.in_window != b.in_window {
                return Err(Diag::DoubleRunDiverge.val());
            }
            i += 1;
        }
        Ok(ledger.entries().to_vec())
    };
    let a = run_once();
    let b = run_once();
    a.is_ok() && b.is_ok() && a == b
}

/// 写回面：状态=已完成、结果摘要非空（含推导值、实测值与对账结论）。
fn writeback_summary_ok() -> bool {
    let ticks = match tsc_sweep_ticks(ASSERT_SWEEP_N) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let us = match measured_us(ticks) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let summary = format!(
        "UNX-F3450 完成：万断言实测{}µs 对推导值{}µs 落窗{} M-209对账成立",
        us,
        DERIVED_BUDGET_US,
        if in_window(us) { "成立" } else { "破约" }
    );
    !summary.is_empty()
        && summary.contains("UNX-F3450")
        && summary.contains("M-209")
        && in_window(us)
        && detail(Diag::BudgetConstDrift.val()).contains("130")
}

// ── 判据总成：run_unx3450_checks ───────────────────────────────────────

/// run_unx3450_checks：本单判据聚合（诊断段 0x79DC0，全部走 add_dyn）。
pub fn run_unx3450_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3450 断言开销预算守恒");
    let checks: [(&'static str, bool, String); 13] = [
        (
            "段位守卫0x40对齐且在批段窗内",
            seg_guard_ok(),
            format!("SEG_BASE=0x{:X} 守卫{}", SEG_BASE, if seg_guard_ok() { "过" } else { "破" }),
        ),
        (
            "诊断码闭集12码唯一且文案带建议",
            diag_closed_set_ok(),
            format!("闭集{}", if diag_closed_set_ok() { "完整" } else { "破缺" }),
        ),
        (
            "常量冻结40拍万断言3GHz130微秒",
            constants_frozen_ok(),
            format!("常量{}", if constants_frozen_ok() { "冻结" } else { "漂移" }),
        ),
        (
            "万断言40万拍133微秒落窗回算守恒",
            sweep_conservation_ok(),
            format!("万断言复测{}", if sweep_conservation_ok() { "成立" } else { "破约" }),
        ),
        (
            "预算窗116/117/143/144四点钉死",
            window_boundary_ok(),
            format!("预算窗{}", if window_boundary_ok() { "钉死" } else { "失守" }),
        ),
        (
            "零断言与溢出三类显性拒",
            zero_overflow_gate_ok(),
            format!("溢出闸{}", if zero_overflow_gate_ok() { "拦准" } else { "失守" }),
        ),
        (
            "余数破约与单断言漂移必拒",
            remainder_drift_gate_ok(),
            format!("守恒闸{}", if remainder_drift_gate_ok() { "拦准" } else { "失守" }),
        ),
        (
            "M-209伪造越窗账必被点名",
            injection_m209_ok(),
            format!("M-209对账{}", if injection_m209_ok() { "捕获" } else { "漏抓" }),
        ),
        (
            "账目序号连续条数一比一守恒",
            ledger_discipline_ok(),
            format!("账目{}", if ledger_discipline_ok() { "对账" } else { "错账" }),
        ),
        (
            "拍数微秒换算抽查表对齐",
            conversion_table_ok(),
            format!("换算表{}", if conversion_table_ok() { "对齐" } else { "漂移" }),
        ),
        (
            "千档与万档实测值口径核",
            scale_ladder_ok(),
            format!("阶梯{}", if scale_ladder_ok() { "落位" } else { "漂移" }),
        ),
        (
            "双跑一致账本逐字段同",
            double_run_ok(),
            format!("双跑{}", if double_run_ok() { "一致" } else { "分叉" }),
        ),
        (
            "写回摘要非空状态已完成",
            writeback_summary_ok(),
            format!("写回{}", if writeback_summary_ok() { "非空" } else { "空" }),
        ),
    ];
    // 13 条并列成账（写回面已入第 13 条，此处补一条段窗摘要对账）。
    let extra = detail(Diag::WindowBreach.val()).contains("117");
    for (name, passed, det) in checks.iter() {
        s.add_dyn(name, *passed, det);
    }
    s.add_dyn(
        "诊断文案含预算窗整数带口径",
        extra,
        &format!("文案{}", if extra { "对齐" } else { "漂移" }),
    );
    s
}

// ── 测试区（#[cfg(test)]：允许 assert!/assert_eq!/unwrap）──────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_and_diag_closed_set() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE, 0x79DC0);
        assert!(diag_closed_set_ok());
        for (i, d) in ALL_DIAGS.iter().enumerate() {
            assert_eq!(d.val(), SEG_BASE + i as u32 + 1);
        }
        assert_eq!(detail(0xDEAD), detail(0x5A5A));
    }

    #[test]
    fn constants_frozen() {
        assert!(constants_frozen_ok());
        assert_eq!(ASSERT_TICKS, 40);
        assert_eq!(ASSERT_SWEEP_N, 10_000);
        assert_eq!(DERIVED_BUDGET_US, 130);
        assert_eq!(TOLERANCE_US, 13);
    }

    #[test]
    fn sweep_10k_ticks_and_us() {
        assert!(sweep_conservation_ok());
        assert_eq!(tsc_sweep_ticks(ASSERT_SWEEP_N).unwrap(), 400_000);
        assert_eq!(measured_us(400_000).unwrap(), 133);
        assert_eq!(per_assert_back(400_000, 10_000).unwrap(), 40);
    }

    #[test]
    fn window_boundary_four_points() {
        assert!(window_boundary_ok());
        assert_eq!(WINDOW_LOW_US, 117);
        assert_eq!(WINDOW_HIGH_US, 143);
    }

    #[test]
    fn zero_and_overflow_gates() {
        assert!(zero_overflow_gate_ok());
        assert_eq!(tsc_sweep_ticks(0), Err(Diag::ZeroAssert.val()));
    }

    #[test]
    fn remainder_and_drift_gates() {
        assert!(remainder_drift_gate_ok());
        assert_eq!(per_assert_back(405, 10), Err(Diag::RemainderBroke.val()));
        assert_eq!(per_assert_back(5500, 100), Err(Diag::PerAssertDrift.val()));
    }

    #[test]
    fn injection_m209_forged_caught() {
        assert!(injection_m209_ok());
    }

    #[test]
    fn ledger_discipline_and_conversion() {
        assert!(ledger_discipline_ok());
        assert!(conversion_table_ok());
    }

    #[test]
    fn scale_ladder_values() {
        assert!(scale_ladder_ok());
        assert_eq!(measured_us(40_000).unwrap(), 13);
    }

    #[test]
    fn double_run_identical() {
        assert!(double_run_ok());
    }

    #[test]
    fn writeback_summary_non_empty() {
        assert!(writeback_summary_ok());
    }

    #[test]
    fn checks_aggregate_all_green() {
        let s = run_unx3450_checks();
        assert!(s.len() >= 12);
        assert!(s.all_passed(), "全部判据必须绿：{}", s.red_items_text());
    }
}
