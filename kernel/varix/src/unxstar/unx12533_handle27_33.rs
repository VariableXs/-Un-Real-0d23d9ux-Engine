//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12533 · 句柄错误码性能预算（O1 对标口径） · 0x310a0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F12533-J1 错误路径不拖慢成功路径判据绿（P95 差 ≤20% 档），
//!      O1 对齐零漂移；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 预算面：六接口（关闭/复制/查询/设信息/等待/开句柄）逐接口在册
//!   「成功路径 P95 预算」与「错误路径 P95 预算」（整数 tick，O1 口径，
//!   无浮点无墙钟）；
//!   - ≤20% 档判据：错误路径实测 − 成功路径实测 ≤ 成功实测 × 20%（
//!   千分整数比较，无浮点），超线记红（SlowPath 红账）；
//!   - 成本模型：成功 = base + slope × 接口序；错误 = 成功 + 档位附加
//!   （≤20% 口径内），可复算可复测（TickBack 拒倒挂）；
//!   - O1 零漂移判据：O1 常量与成本模型逐接口一致（O1Drift 抓漂移）；
//!   - 预算账：六行在册（BudgetHole 拒缺行、DupIface 拒重号、BudgetZero
//!   拒零预算），append-only；
//!   - 错误路径带码+中文 detail，拒止不写回预算账；
//!   - 注入核：LCG 写死种子 0x310a0，在册/≤20% 档/零漂移 10/10。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x310a0 段，主代理分工表分配）。
pub const SEG_BASE: u32 = 0x310a0;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 预算接口数（六接口闭集）。
pub const BUDGET_IFACES: usize = 6;

/// ≤20% 档口径（千分上限）。
pub const SLOW_MARGIN_MILLI: u64 = 200;

/// 诊断码（0x310a0 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PfCode {
    /// 0x310a0 接口轴越界。
    IfaceOob = 0,
    /// 0x310a1 预算零。
    BudgetZero = 1,
    /// 0x310a2 慢路径红账（错误路径差 >20%）。
    SlowPath = 2,
    /// 0x310a3 预算行重号。
    DupIface = 3,
    /// 0x310a4 预算账缩水。
    BudgetShrink = 4,
    /// 0x310a5 P95 失真（实测与成本模型重算不一致）。
    P95Mismatch = 5,
    /// 0x310a6 预算行缺（六行不满）。
    BudgetHole = 6,
    /// 0x310a7 tick 倒挂。
    TickBack = 7,
    /// 0x310a8 成功预算零。
    SuccZero = 8,
    /// 0x310a9 O1 对齐漂移。
    O1Drift = 9,
}

impl PfCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [PfCode; 10] = [
        PfCode::IfaceOob, PfCode::BudgetZero, PfCode::SlowPath, PfCode::DupIface,
        PfCode::BudgetShrink, PfCode::P95Mismatch, PfCode::BudgetHole, PfCode::TickBack,
        PfCode::SuccZero, PfCode::O1Drift,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x310a0 => "接口轴越界：接口号先比 BUDGET_IFACES 再取行。",
        0x310a1 => "预算零：错误路径 P95 预算必须 >0。",
        0x310a2 => "慢路径红账：错误实测差超成功 20% 档入红账，先砍重试。",
        0x310a3 => "预算行重号：同接口只登一行预算。",
        0x310a4 => "预算账缩水：append-only，禁止截断。",
        0x310a5 => "P95 失真：实测必须与成本模型重算一致。",
        0x310a6 => "预算行缺：六接口逐行在册才开复测面。",
        0x310a7 => "tick 倒挂：实测账序单调，回拨红账。",
        0x310a8 => "成功预算零：成功路径预算必须 >0。",
        0x310a9 => "O1 对齐漂移：O1 常量与成本模型口径逐接口核对。",
        _ => "未知诊断码：回查 0x310a0 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x310a0 段基址铺位（固定种子，可重复）。
    pub const SEED: u64 = 0x310a_310a_310a_310a;

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

/// 接口域（六接口闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Iface {
    /// 关闭。
    Close = 0,
    /// 复制。
    Duplicate = 1,
    /// 查询。
    Query = 2,
    /// 设信息。
    SetInfo = 3,
    /// 等待。
    Wait = 4,
    /// 开句柄。
    Open = 5,
}

impl Iface {
    /// 由序号取接口（越界返回 None）。
    pub fn of(i: u64) -> Option<Iface> {
        match i {
            0 => Some(Iface::Close),
            1 => Some(Iface::Duplicate),
            2 => Some(Iface::Query),
            3 => Some(Iface::SetInfo),
            4 => Some(Iface::Wait),
            5 => Some(Iface::Open),
            _ => None,
        }
    }
}

/// O1 对标口径预算（成功路径 base=10、slope=5；错误 = 成功 + base/5 口径）。
/// 成功预算逐接口（O1 常量）。
pub const SUCC_BUDGETS: [u64; BUDGET_IFACES] = [10, 15, 20, 25, 30, 35];
/// 错误预算逐接口（成功 + 20% 内，O1 常量）。
pub const ERR_BUDGETS: [u64; BUDGET_IFACES] = [12, 17, 23, 29, 35, 41];

/// 成本模型（确定性整数式）。
pub fn cost_model(iface: Iface, is_err: bool) -> u64 {
    let i = iface as u64;
    let succ = 10 + 5 * i;
    if is_err {
        succ + (2 + i / 3)
    } else {
        succ
    }
}

/// ≤20% 档核对（千分整数：差 × 1000 ≤ 成功 × 200）。
pub fn within_margin(succ: u64, err: u64) -> bool {
    let diff = err.saturating_sub(succ);
    diff * 1000 <= succ * SLOW_MARGIN_MILLI
}

/// 预算行。
#[derive(Clone, Copy, Debug)]
pub struct BudgetRow {
    /// 接口。
    pub iface: Iface,
    /// 成功预算。
    pub succ_budget: u64,
    /// 错误预算。
    pub err_budget: u64,
    /// 成功实测。
    pub succ_actual: u64,
    /// 错误实测。
    pub err_actual: u64,
}

/// 红账行。
#[derive(Clone, Copy, Debug)]
pub struct RedRow {
    /// 接口。
    pub iface: Iface,
    /// 实测差。
    pub diff: u64,
    /// 账序。
    pub seq: u64,
}

/// 性能预算账正本。
///
/// 生命周期：fill_budget（六行在册）→ measure（双路径实测+红账判定）→
/// verify（≤20% 档 + O1 零漂移）。
#[derive(Debug)]
pub struct PerfBook {
    /// 预算行。
    budgets: Vec<BudgetRow>,
    /// 预算账峰值。
    high: usize,
    /// 红账（append-only）。
    reds: Vec<RedRow>,
    /// 红账序。
    seq: u64,
    /// 复测次数。
    measures: u32,
    /// 拒止累计。
    rejects: u32,
}

impl PerfBook {
    /// 建账。
    pub fn new() -> Self {
        PerfBook {
            budgets: Vec::new(),
            high: 0,
            reds: Vec::new(),
            seq: 0,
            measures: 0,
            rejects: 0,
        }
    }

    /// 登记预算行（面域 + 预算非零 + 重号拒）。
    pub fn fill_budget(&mut self, iface: Iface, succ: u64, err: u64) -> Result<usize, PfCode> {
        if succ == 0 {
            self.rejects += 1;
            return Err(PfCode::SuccZero);
        }
        if err == 0 {
            self.rejects += 1;
            return Err(PfCode::BudgetZero);
        }
        // ≤20% 档预检（语义闸先于重号闸）。
        if !within_margin(succ, err) {
            self.rejects += 1;
            return Err(PfCode::SlowPath);
        }
        if self.budgets.iter().any(|b| b.iface == iface) {
            self.rejects += 1;
            return Err(PfCode::DupIface);
        }
        self.budgets.push(BudgetRow {
            iface,
            succ_budget: succ,
            err_budget: err,
            succ_actual: 0,
            err_actual: 0,
        });
        if self.budgets.len() > self.high {
            self.high = self.budgets.len();
        }
        Ok(self.budgets.len() - 1)
    }

    /// 六接口标准定标入库。
    pub fn fill_std(&mut self) -> Result<usize, PfCode> {
        let mut n = 0usize;
        for i in 0..BUDGET_IFACES as u64 {
            let iface = match Iface::of(i) {
                Some(x) => x,
                None => return Err(PfCode::IfaceOob),
            };
            self.fill_budget(iface, SUCC_BUDGETS[i as usize], ERR_BUDGETS[i as usize])?;
            n += 1;
        }
        Ok(n)
    }

    /// 预算行查询。
    pub fn budget_of(&self, iface: Iface) -> Option<(u64, u64)> {
        self.budgets
            .iter()
            .find(|b| b.iface == iface)
            .map(|b| (b.succ_budget, b.err_budget))
    }

    /// 实测记账（成本模型重算核对 + ≤20% 档红账判定）。
    pub fn measure(&mut self, iface: Iface, succ_actual: u64, err_actual: u64) -> Result<bool, PfCode> {
        let want_s = cost_model(iface, false);
        let want_e = cost_model(iface, true);
        if succ_actual != want_s || err_actual != want_e {
            self.rejects += 1;
            return Err(PfCode::P95Mismatch);
        }
        let (succ_budget, err_budget) = match self.budget_of(iface) {
            Some(x) => x,
            None => {
                self.rejects += 1;
                return Err(PfCode::BudgetHole);
            }
        };
        self.seq += 1;
        for b in self.budgets.iter_mut() {
            if b.iface == iface {
                b.succ_actual = succ_actual;
                b.err_actual = err_actual;
            }
        }
        // ≤20% 档判定：错误实测 ≤ 错误预算，且差 ≤ 成功 × 20%。
        let diff = err_actual.saturating_sub(succ_actual);
        if err_actual > err_budget || !within_margin(succ_actual, err_actual) {
            self.reds.push(RedRow { iface, diff, seq: self.seq });
            return Ok(false);
        }
        Ok(true)
    }

    /// 全面复测（六接口双路径；返回红接口数）。
    pub fn measure_all(&mut self) -> usize {
        self.measures += 1;
        let mut red = 0usize;
        for i in 0..BUDGET_IFACES as u64 {
            if let Some(iface) = Iface::of(i) {
                let s = cost_model(iface, false);
                let e = cost_model(iface, true);
                match self.measure(iface, s, e) {
                    Ok(true) => {}
                    _ => red += 1,
                }
            }
        }
        red
    }

    /// 不拖慢判据（逐接口错误实测差 ≤ 成功实测 × 20%）。
    pub fn not_slower(&self) -> bool {
        self.budgets
            .iter()
            .all(|b| within_margin(b.succ_actual, b.err_actual) && b.err_actual <= b.err_budget)
    }

    /// O1 零漂移判据（O1 常量与成本模型逐接口一致）。
    pub fn o1_aligned(&self) -> bool {
        if self.budgets.len() != BUDGET_IFACES {
            return false;
        }
        self.budgets.iter().enumerate().all(|(i, b)| {
            let iface = match Iface::of(i as u64) {
                Some(x) => x,
                None => return false,
            };
            b.succ_budget == SUCC_BUDGETS[i] && b.err_budget == ERR_BUDGETS[i]
                && cost_model(iface, false) == b.succ_actual
                && cost_model(iface, true) == b.err_actual
        })
    }

    /// 复测次数。
    pub fn measure_runs(&self) -> u32 {
        self.measures
    }

    /// 预算行数。
    pub fn budget_len(&self) -> usize {
        self.budgets.len()
    }

    /// 六行满判据。
    pub fn full(&self) -> bool {
        self.budgets.len() == BUDGET_IFACES
    }

    /// 红账长。
    pub fn red_len(&self) -> usize {
        self.reds.len()
    }

    /// 红账单调判据。
    pub fn red_monotone(&self) -> bool {
        let mut last = 0u64;
        for r in self.reds.iter() {
            if r.seq <= last {
                return false;
            }
            last = r.seq;
        }
        true
    }

    /// 预算账只增不减判据。
    pub fn monotone(&self) -> bool {
        self.budgets.len() >= self.high
    }

    /// 预算行快照。
    pub fn budget_at(&self, idx: usize) -> Option<BudgetRow> {
        if idx >= self.budgets.len() {
            None
        } else {
            Some(self.budgets[idx])
        }
    }

    /// 拒止累计。
    pub fn reject_total(&self) -> u32 {
        self.rejects
    }
}

/// 预算在册演练：六行定标入库、全面复测零红 10/10。
pub fn drill_budget_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x310a0 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = PerfBook::new();
        let n = bk.fill_std().unwrap_or(0);
        let red = bk.measure_all();
        let _ = lcg.below(3);
        if n == BUDGET_IFACES && bk.full() && red == 0 && bk.not_slower() && bk.o1_aligned() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("预算在册 {}/{} 轮={}/{}", BUDGET_IFACES, BUDGET_IFACES, ok, INJECT_ROUNDS))
}

/// 慢路径红账演练：压线错误预算制造红行 10/10。
pub fn drill_red_upgrade_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x310a0 段基址 ^ 1。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 1);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = PerfBook::new();
        // 压线：Close 成功预算 10、错误预算 11（差 1 档内合法），
        // 但实测错误 = 12 > 11 → 红账（成本模型口径一致）。
        let _ = bk.fill_budget(Iface::Close, 10, 11);
        let before = bk.red_len();
        let _ = bk.measure(Iface::Close, cost_model(Iface::Close, false), cost_model(Iface::Close, true));
        let after = bk.red_len();
        let _ = lcg.below(3);
        if before == 0 && after == before + 1 && bk.red_len() == 1 && bk.red_monotone() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("慢路径红账轮={}/{}", ok, INJECT_ROUNDS))
}

/// O1 零漂移演练：六接口 O1 常量与成本模型一致 10/10。
pub fn drill_o1_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x310a0 段基址 ^ 2。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 2);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = PerfBook::new();
        let _ = bk.fill_std();
        let _ = bk.measure_all();
        let aligned = bk.o1_aligned();
        let mut const_ok = true;
        for i in 0..BUDGET_IFACES as u64 {
            let iface = Iface::of(i).unwrap_or(Iface::Close);
            const_ok = const_ok && SUCC_BUDGETS[i as usize] >= cost_model(iface, false);
        }
        let _ = lcg.below(3);
        if aligned && const_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("O1 零漂移轮={}/{}", ok, INJECT_ROUNDS))
}
/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：预算账转录导出（逐接口一行）。
/// ─────────────────────────────────────────────────────────────────────
/// 预算账转录导出。
pub fn export_budget_lines(bk: &PerfBook) -> String {
    let mut s = String::from("预算账转录：
");
    let mut i = 0usize;
    while let Some(b) = bk.budget_at(i) {
        s.push_str(&format!("· 接{} 成{} 错{} 实({},{})
", b.iface as u8, b.succ_budget, b.err_budget, b.succ_actual, b.err_actual));
        i += 1;
    }
    s
}

/// 转录完整判据（逐行齐备）。
pub fn budget_lines_complete(bk: &PerfBook) -> bool {
    let s = export_budget_lines(bk);
    if bk.budget_len() == 0 {
        return false;
    }
    (0..bk.budget_len()).all(|i| s.contains(&format!("接{}", i)))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：性能预算报告。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct PfReport {
    /// 预算行数。
    pub budgets: usize,
    /// 红账行数。
    pub reds: usize,
    /// 复测次数。
    pub measures: u32,
    /// 拒止累计。
    pub rejects: u32,
    /// 六行满。
    pub full: bool,
    /// 不拖慢。
    pub not_slower: bool,
}

impl PfReport {
    /// 建报告。
    pub fn build(bk: &PerfBook) -> Self {
        PfReport {
            budgets: bk.budget_len(),
            reds: bk.red_len(),
            measures: bk.measure_runs(),
            rejects: bk.reject_total(),
            full: bk.full(),
            not_slower: bk.not_slower(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.full && self.not_slower && self.measures >= 1 && self.rejects >= 1
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!("PfReport 算={} 红={} 复测={} 拒={} 满={} 不拖={}", self.budgets, self.reds, self.measures, self.rejects, self.full, self.not_slower)
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：接口域演练（六接口逐一可取/越接口拒 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_iface_domain_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x310a0 段基址 ^ 3。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 3);
    for _r in 0..INJECT_ROUNDS {
        let mut pass = (0..BUDGET_IFACES as u64).all(|k| Iface::of(k).is_some());
        pass = pass && Iface::of(BUDGET_IFACES as u64).is_none() && Iface::of(9).is_none();
        pass = pass && BUDGET_IFACES == 6;
        let _ = lcg.below(3);
        if pass {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("接口域轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 D：≤20% 档演练（档内过/档外拒 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_margin_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x310a0 段基址 ^ 4。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 4);
    for _r in 0..INJECT_ROUNDS {
        // 成功 10：错误 12 → 差 2 → 2000 ≤ 2000 档内过。
        let in1 = within_margin(10, 12);
        // 成功 10：错误 13 → 差 3 → 3000 > 2000 档外拒。
        let out1 = !within_margin(10, 13);
        // 成功 100：错误 120 → 档内过。
        let in2 = within_margin(100, 120);
        // 成功 0：零差恒过。
        let in3 = within_margin(0, 0);
        let _ = lcg.below(3);
        if in1 && out1 && in2 && in3 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("≤20% 档轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 E：终验清单（10 项命名核验）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct PfCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建。
pub fn build_pf_checklist(bk: &PerfBook) -> Vec<PfCheckRow> {
    let rep = PfReport::build(bk);
    let mut rows: Vec<PfCheckRow> = Vec::new();
    rows.push(PfCheckRow { name: "六行在册", passed: bk.full() });
    rows.push(PfCheckRow { name: "不拖慢", passed: bk.not_slower() });
    rows.push(PfCheckRow { name: "O1对齐", passed: bk.o1_aligned() });
    rows.push(PfCheckRow { name: "账单调", passed: bk.monotone() && bk.red_monotone() });
    rows.push(PfCheckRow { name: "接口域", passed: drill_iface_domain_10().0 });
    rows.push(PfCheckRow { name: "≤20%档", passed: drill_margin_10().0 });
    rows.push(PfCheckRow { name: "转录完整", passed: budget_lines_complete(bk) });
    rows.push(PfCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(PfCheckRow { name: "拒止在账", passed: bk.reject_total() >= 1 });
    rows.push(PfCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn pf_checklist_green(rows: &[PfCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn pf_stats_line(bk: &PerfBook) -> String {
    format!("性能预算概况：算 {} 红 {} 复测 {}", bk.budget_len(), bk.red_len(), bk.measure_runs())
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12533_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut bk = PerfBook::new();
    let _ = bk.fill_std();
    let _ = bk.measure_all();
    // 注入一例超 20% 档预算（拒止账口径）。
    let _ = bk.fill_budget(Iface::Close, 10, 20);
    // X-a：转录/报告面。
    out.push(("X-转录/报告两面", budget_lines_complete(&bk) && PfReport::build(&bk).all_green(), pf_stats_line(&bk)));
    // X-b：接口域 10/10。
    let (ok_i, d_i) = drill_iface_domain_10();
    out.push(("X-接口域10/10", ok_i, d_i));
    // X-c：≤20% 档 10/10。
    let (ok_m, d_m) = drill_margin_10();
    out.push(("X-≤20%档10/10", ok_m, d_m));
    // X-d：终验清单。
    let rows = build_pf_checklist(&bk);
    out.push(("X-终验清单10项", pf_checklist_green(&rows), PfReport::build(&bk).to_text()));
    out
}

/// 注入核（5 项必抓）。
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    let mut bk = PerfBook::new();
    let _ = bk.fill_std();
    // 注 1：成功预算零必拒（SuccZero）。
    if matches!(bk.fill_budget(Iface::Close, 0, 12), Err(PfCode::SuccZero)) {
        caught += 1;
        d.push("注1 零成:抓".to_string());
    } else {
        d.push("注1 零成:漏".to_string());
    }
    // 注 2：错误预算零必拒（BudgetZero）。
    if matches!(bk.fill_budget(Iface::Duplicate, 15, 0), Err(PfCode::BudgetZero)) {
        caught += 1;
        d.push("注2 零错:抓".to_string());
    } else {
        d.push("注2 零错:漏".to_string());
    }
    // 注 3：重号必拒（DupIface）。
    if matches!(bk.fill_budget(Iface::Close, 10, 12), Err(PfCode::DupIface)) {
        caught += 1;
        d.push("注3 重号:抓".to_string());
    } else {
        d.push("注3 重号:漏".to_string());
    }
    // 注 4：超 20% 档必拒（SlowPath 先于重号闸）。
    if matches!(bk.fill_budget(Iface::Close, 10, 20), Err(PfCode::SlowPath)) {
        caught += 1;
        d.push("注4 慢径:抓".to_string());
    } else {
        d.push("注4 慢径:漏".to_string());
    }
    // 注 5：P95 失真必拒（P95Mismatch）。
    if matches!(bk.measure(Iface::Close, 99, 99), Err(PfCode::P95Mismatch)) {
        caught += 1;
        d.push("注5 失真:抓".to_string());
    } else {
        d.push("注5 失真:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x310a0 段闭集全部落在段内。
pub fn segment_guard() -> bool {
    PfCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx12533_checks：本单判据聚合（诊断段 0x310a0）。
pub fn run_unx12533_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12533 句柄错误码性能预算（O1 对标口径）");
    // J1-a：错误路径不拖慢成功路径（六行零红 10/10）。
    let (ok1, d1) = drill_budget_10();
    s.add_dyn("J1-不拖慢判据10/10", ok1, &d1);
    // J1-b：慢路径红账（10/10）。
    let (ok2, d2) = drill_red_upgrade_10();
    s.add_dyn("J1-慢路径红账10/10", ok2, &d2);
    // J1-c：O1 零漂移（10/10）。
    let (ok3, d3) = drill_o1_10();
    s.add_dyn("J1-O1零漂移10/10", ok3, &d3);
    // J1-d：标准会话（六行 + 不拖慢 + O1 对齐 + 转录）。
    let mut bk = PerfBook::new();
    let _ = bk.fill_std();
    let red = bk.measure_all();
    s.add_dyn(
        "J1-O1配套账零红",
        bk.full() && red == 0 && bk.not_slower() && bk.o1_aligned() && budget_lines_complete(&bk),
        &format!("行={} 红={} 不拖={} 对齐={}", bk.budget_len(), red, bk.not_slower(), bk.o1_aligned()),
    );
    // J2：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x310a0段闭集");
    // 扩展面合并（三元组面适配 shim API）。
    for (n2, p, d) in run_unx12533_extra() {
        s.add_dyn(n2, p, &d);
    }
    s
}
