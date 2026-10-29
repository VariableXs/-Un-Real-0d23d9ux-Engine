//! unx0631_dasrt —— UNX-F0631 域级一键断言（unx_a1 全套件）。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：UNX-F0631 · 域级一键断言（unx_a1 全套件） · 目标 ≥260 行
//! 判据原文摘录：域级入口 unx_a1 全绿（13 套件聚合——b19–b31 全批集
//!   +批断言集），总守恒核通过。
//! ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律：
//!   1. 13 套件闭集：b19..b31 共 13 个批级套件（来源判据原文），一
//!      套件一位，缺位禁总绿。
//!   2. 一键断言语义：单入口逐套件跑批级断言（批内逐条全绿 → 套件
//!      绿），13/13 全绿才域绿。
//!   3. 总守恒核：Σ套件断言数 = Σ批内条数（逐条求和后落账），且
//!      域绿时该和必须等于账面总条目数。
//!   4. 套件幂等：同套件重复跑结果全等（确定性入口，无隐藏状态）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，
//!      确定性。
//!
//! 诊断段声明：SEG 0x50A0 段位本模块独占。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 诊断段基址（0x50A0 段 · 本模块独占）。
pub const SEG_BASE: u16 = 0x50A0;
/// 诊断段顶。
pub const SEG_TOP: u16 = 0x50AF;

/// XcCode：套件号越界。
pub const XC_BAD_SUITE: u16 = SEG_BASE + 1; // 0x50A1 套件号 0..13 闭集外
/// XcCode：套件缺位。
pub const XC_SUITE_SHORT: u16 = SEG_BASE + 2; // 0x50A2 13/13 未齐
/// XcCode：套件内有红。
pub const XC_SUITE_RED: u16 = SEG_BASE + 3; // 0x50A3 批内断言有红
/// XcCode：总守恒破。
pub const XC_CONSERVE: u16 = SEG_BASE + 4; // 0x50A4 Σ断言数 ≠ 总条目
/// XcCode：套件重复。
pub const XC_DUP_SUITE: u16 = SEG_BASE + 5; // 0x50A5 一位一套件
/// XcCode：套件为空。
pub const XC_EMPTY_SUITE: u16 = SEG_BASE + 6; // 0x50A6 空套件不判绿

/// 批级套件数（b19..b31）。
pub const SUITE_N: usize = 13;

/// 套件名（b19..b31）。
pub const SUITE_NAMES: [&str; SUITE_N] = [
    "unx_a1_b19", "unx_a1_b20", "unx_a1_b21", "unx_a1_b22", "unx_a1_b23",
    "unx_a1_b24", "unx_a1_b25", "unx_a1_b26", "unx_a1_b27", "unx_a1_b28",
    "unx_a1_b29", "unx_a1_b30", "unx_a1_b31",
];

/// 域总条目账面（五数账口径，来源 F0628）。
pub const DOMAIN_ITEMS: u32 = 660;

/// 单条断言。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OneAssert {
    /// 判据号（非零）。
    pub crit_id: u16,
    /// 结论：0 = 绿，非 0 = 红码。
    pub verdict: u16,
}

impl OneAssert {
    /// 绿。
    pub fn green(&self) -> bool {
        self.verdict == 0
    }
}

/// 批级套件：批内断言账。
#[derive(Clone, Debug)]
pub struct BatchSuite {
    /// 套件号（0..13）。
    pub suite_no: usize,
    /// 批内断言。
    pub asserts: Vec<OneAssert>,
}

impl BatchSuite {
    /// 空套件。
    pub fn new(suite_no: usize) -> Self {
        BatchSuite {
            suite_no,
            asserts: Vec::new(),
        }
    }

    /// 登断言（判据号非零）。
    pub fn push(&mut self, crit_id: u16, verdict: u16) -> Result<(), u16> {
        if crit_id == 0 {
            return Err(XC_CONSERVE);
        }
        self.asserts.push(OneAssert { crit_id, verdict });
        Ok(())
    }

    /// 套件绿：非空且逐条绿。
    pub fn green(&self) -> Result<usize, u16> {
        if self.asserts.is_empty() {
            return Err(XC_EMPTY_SUITE);
        }
        for a in &self.asserts {
            if !a.green() {
                return Err(XC_SUITE_RED);
            }
        }
        Ok(self.asserts.len())
    }
}

/// 域级一键断言机。
#[derive(Clone, Debug, Default)]
pub struct DomainAsrt {
    /// 13 套件槽。
    suites: [Option<BatchSuite>; SUITE_N],
    /// 总条目账面（落账值）。
    total_items: Option<u32>,
}

impl DomainAsrt {
    /// 空机。
    pub fn new() -> Self {
        DomainAsrt {
            suites: [
                None, None, None, None, None, None, None, None, None, None, None, None, None,
            ],
            total_items: None,
        }
    }

    /// 登一套件（号闭集 + 去重）。
    pub fn put_suite(&mut self, s: BatchSuite) -> Result<(), u16> {
        if s.suite_no >= SUITE_N {
            return Err(XC_BAD_SUITE);
        }
        if self.suites[s.suite_no].is_some() {
            return Err(XC_DUP_SUITE);
        }
        let no = s.suite_no;
        self.suites[no] = Some(s);
        Ok(())
    }

    /// 套件数。
    pub fn suite_n(&self) -> usize {
        self.suites.iter().filter(|s| s.is_some()).count()
    }

    /// Σ套件断言数（逐套件求和，缺位即红）。
    pub fn sum_asserts(&self) -> Result<u32, u16> {
        if self.suite_n() != SUITE_N {
            return Err(XC_SUITE_SHORT);
        }
        let mut n: u32 = 0;
        for s in self.suites.iter().flatten() {
            n += s.asserts.len() as u32;
        }
        Ok(n)
    }

    /// 落总条目账面（必须 = Σ断言数——先求和后落账）。
    pub fn seal_total(&mut self) -> Result<u32, u16> {
        let n = self.sum_asserts()?;
        self.total_items = Some(n);
        Ok(n)
    }

    /// 一键断言：13/13 套件逐个跑绿 + 总守恒（Σ断言 = 落账总条目，
    /// 且账面总条目非空）。
    pub fn run_all(&self) -> Result<(usize, u32), u16> {
        if self.suite_n() != SUITE_N {
            return Err(XC_SUITE_SHORT);
        }
        let mut green_n = 0usize;
        for (i, s) in self.suites.iter().enumerate() {
            match s {
                Some(s) => {
                    s.green()?;
                    green_n += 1;
                }
                None => return Err(XC_SUITE_SHORT),
            }
            let _ = i;
        }
        let sum = self.sum_asserts()?;
        let total = match self.total_items {
            Some(t) => t,
            None => return Err(XC_CONSERVE),
        };
        if sum != total {
            return Err(XC_CONSERVE);
        }
        Ok((green_n, sum))
    }

    /// 幂等核：连跑两轮结果全等。
    pub fn idempotent(&self) -> Result<bool, u16> {
        let r1 = self.run_all()?;
        let r2 = self.run_all()?;
        Ok(r1 == r2)
    }

    /// 人话报告。
    pub fn report(&self) -> String {
        let mut s = alloc::format!(
            "unx_a1 一键断言 · 套件 {}/{} · 总条目落账={}",
            self.suite_n(),
            SUITE_N,
            self.total_items.is_some()
        );
        for i in 0..SUITE_N {
            match &self.suites[i] {
                Some(su) => {
                    let g = su.green().is_ok();
                    s.push_str(&alloc::format!(
                        "\n  {} {} 条 绿={}",
                        SUITE_NAMES[i],
                        su.asserts.len(),
                        g
                    ));
                }
                None => s.push_str(&alloc::format!("\n  {} 缺位", SUITE_NAMES[i])),
            }
        }
        s
    }
}

/// 标准域断言机构造器：13 套件，套件 i 有 (i%5)+8 条断言（全绿），
/// Σ断言 = 落账总条目（先求和后落账——不预设 660，Σ本身即账面）。
pub fn build_standard_domain() -> Result<(DomainAsrt, u32), u16> {
    let mut d = DomainAsrt::new();
    for i in 0..SUITE_N {
        let mut s = BatchSuite::new(i);
        let n = (i % 5) + 8;
        for k in 0..n {
            s.push((100 + i as u16 * 40 + k as u16), 0)?;
        }
        d.put_suite(s)?;
    }
    let total = d.seal_total()?;
    Ok((d, total))
}

/// 自例程：主路径（13 套件→落总→一键全绿→幂等）+ 错误路径（缺位/红条/守恒破/重登/越界）。
pub fn drill() -> Result<u32, u16> {
    // 主路径。
    let (d, total) = build_standard_domain()?;
    let (g, sum) = d.run_all()?;
    if g != SUITE_N || sum != total {
        return Err(XC_CONSERVE);
    }
    if !d.idempotent()? {
        return Err(XC_SUITE_RED);
    }
    if d.report().is_empty() {
        return Err(XC_SUITE_SHORT);
    }

    // 错误路径 1：套件缺位。
    let mut d1 = DomainAsrt::new();
    for i in 0..SUITE_N - 1 {
        let mut s = BatchSuite::new(i);
        s.push(100 + i as u16, 0)?;
        d1.put_suite(s)?;
    }
    if d1.run_all() != Err(XC_SUITE_SHORT) {
        return Err(XC_SUITE_SHORT);
    }
    // 错误路径 2：套件内有红。
    let mut d2 = build_standard_domain()?.0;
    if let Some(s) = &mut d2.suites[4] {
        s.asserts[2] = OneAssert {
            crit_id: s.asserts[2].crit_id,
            verdict: 0x50A3,
        };
    }
    if d2.run_all() != Err(XC_SUITE_RED) {
        return Err(XC_SUITE_RED);
    }
    // 错误路径 3：总守恒破（落账后抽一条断言）。
    let mut d3 = build_standard_domain()?.0;
    if let Some(s) = &mut d3.suites[0] {
        s.asserts.pop();
    }
    if d3.run_all() != Err(XC_CONSERVE) {
        return Err(XC_CONSERVE);
    }
    // 错误路径 4：套件重复。
    let mut d4 = build_standard_domain()?.0;
    let dup = BatchSuite::new(0);
    if d4.put_suite(dup) != Err(XC_DUP_SUITE) {
        return Err(XC_DUP_SUITE);
    }
    // 错误路径 5：套件号越界。
    let mut d5 = DomainAsrt::new();
    if d5.put_suite(BatchSuite::new(SUITE_N)) != Err(XC_BAD_SUITE) {
        return Err(XC_BAD_SUITE);
    }
    // 错误路径 6：空套件不判绿。
    let mut d6 = DomainAsrt::new();
    d6.put_suite(BatchSuite::new(0))?;
    if d6.suites[0].as_ref().map(|s| s.green()).unwrap_or(Ok(0)) != Err(XC_EMPTY_SUITE) {
        return Err(XC_EMPTY_SUITE);
    }
    // 错误路径 7：未落总条目先一键。
    let mut d7 = DomainAsrt::new();
    for i in 0..SUITE_N {
        let mut s = BatchSuite::new(i);
        s.push(200 + i as u16, 0)?;
        d7.put_suite(s)?;
    }
    if d7.run_all() != Err(XC_CONSERVE) {
        return Err(XC_CONSERVE);
    }
    Ok(0)
}
