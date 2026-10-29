//! unx4140_b07assert — UNX-F4140 B07 批判据聚合断言集（诊断段 0x9360）。
//!
//! 判据原文：UNX-F4140-J1 本批 19 条判据全部聚合入 ktest vfs B07 面，聚合报告 19/19
//!           无缺项，通过率 100% 才算绿。
//!   J2 缺项与失败给不同诊断码（返工方向可区分）。
//!   J3 一判据一结论，重复裁决被拒。
//!   J4 报告绿判据为三元合取，且与三数自洽。
//! 设计纪律：
//!   1. 批账是「登记表 + 结论表」两表合一：每条判据先登记（占位、给权重），后被裁决
//!      （通过/失败）；未裁决者算缺项，缺项不得以「默认通过」补平。
//!   2. 绿判据是三元合取：登记数 == 19、缺项 == 0、通过率 == 100%。三者缺一即非绿，
//!      且诊断码要能区分「缺项」与「失败」——两者的返工动作完全不同。
//!   3. 本模块不与同批其它模块发生依赖（不 import 其文件、不调用其函数）：判据结论
//!      由调用方逐条 mark，聚合器只做账，从而可在任意编译单元独立跑。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x9360 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9360;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_DUP: u16 = SEG_BASE + 1; // 判据号重复登记
const E_UNKNOWN: u16 = SEG_BASE + 2; // 判据号不在批内闭集
const E_MISSING: u16 = SEG_BASE + 3; // 有缺项（未裁决）
const E_FAILED: u16 = SEG_BASE + 4; // 有失败判据
const E_COUNT: u16 = SEG_BASE + 5; // 登记数与批规模不符
const E_STATE: u16 = SEG_BASE + 6; // 判据重复裁决（一判据一结论）
const E_ACCT: u16 = SEG_BASE + 7; // 通过/失败/缺项三数与总数不平
const E_ORDER: u16 = SEG_BASE + 8; // 未登记即裁决

/// 本批判据总数（判据原文 19 条）。
pub const CRIT_TOTAL: u32 = 19;
/// 批内模块数（UNX-F4124..UNX-F4140 共 17 张，其中 2 张各贡献 2 条）。
pub const MODULE_TOTAL: u32 = 17;

/// 判据状态机：Missing（已登记未裁决）→ Passed / Failed；两端皆终态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CritState {
    Missing,
    Passed,
    Failed,
}

/// 单条判据：判据号 + 归属模块号 + 标签 + 权重 + 状态。
pub struct Criterion {
    /// 批内判据序号（0..CRIT_TOTAL）
    pub idx: u16,
    /// 归属模块号（如 4124）
    pub module: u16,
    /// 判据标签（如 "J1"）
    pub tag: &'static str,
    /// 权重（用于加权通过率，1..3）
    pub weight: u8,
    /// 状态
    pub state: CritState,
}

/// 批账：判据表 + 聚合结论。
pub struct BatchLedger {
    /// 判据表
    pub crits: Vec<Criterion>,
    /// 通过条数
    pub passed: u32,
    /// 失败条数
    pub failed: u32,
    /// 未裁决条数
    pub missing: u32,
    /// 重复裁决拒收次数
    pub dup_rulings: u32,
    /// 对账次数
    pub reconciles: u32,
}

impl BatchLedger {
    pub fn new() -> BatchLedger {
        BatchLedger {
            crits: Vec::new(),
            passed: 0,
            failed: 0,
            missing: 0,
            dup_rulings: 0,
            reconciles: 0,
        }
    }

    /// 登记一条判据：判据号唯一、模块号在批内、权重在 1..=3。
    pub fn register(&mut self, idx: u16, module: u16, tag: &'static str, weight: u8) -> Result<(), u16> {
        if idx as u32 >= CRIT_TOTAL {
            return Err(E_UNKNOWN);
        }
        if module < 4124 || module > 4140 {
            return Err(E_UNKNOWN); // 批内模块闭集（含 4141 缺口，按批号区间受理）
        }
        if weight == 0 || weight > 3 {
            return Err(E_UNKNOWN);
        }
        let mut i = 0usize;
        while i < self.crits.len() {
            if self.crits[i].idx == idx {
                return Err(E_DUP);
            }
            i += 1;
        }
        self.crits.push(Criterion {
            idx,
            module,
            tag,
            weight,
            state: CritState::Missing,
        });
        self.missing += 1;
        Ok(())
    }

    /// 批量登记本批 19 条（模块号与标签按批清单硬编码，来源=施工单）。
    pub fn register_batch(&mut self) -> Result<u32, u16> {
        // (模块号, 标签, 权重)：17 张各 1 条 + 2 张补 1 条 = 19
        let rows: [(u16, &'static str, u8); CRIT_TOTAL as usize] = [
            (4124, "J1", 3),
            (4125, "J1", 2),
            (4126, "J1", 2),
            (4127, "J1", 3),
            (4128, "J1", 2),
            (4129, "J1", 3),
            (4130, "J1", 2),
            (4131, "J1", 2),
            (4132, "J1", 3),
            (4133, "J1", 3),
            (4134, "J1", 3),
            (4135, "J1", 2),
            (4136, "J1", 3),
            (4137, "J1", 3),
            (4138, "J1", 2),
            (4139, "J1", 2),
            (4140, "J1", 3),
            (4124, "J2", 1), // 约束矩阵全格
            (4126, "J2", 1), // 排空先于释放
        ];
        let mut i = 0usize;
        while i < rows.len() {
            let (m, t, w) = rows[i];
            self.register(i as u16, m, t, w)?;
            i += 1;
        }
        if self.crits.len() as u32 != CRIT_TOTAL {
            return Err(E_COUNT);
        }
        Ok(self.crits.len() as u32)
    }

    fn find(&self, idx: u16) -> Option<usize> {
        let mut i = 0usize;
        while i < self.crits.len() {
            if self.crits[i].idx == idx {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 裁决：Missing → Passed/Failed，一条判据只许一个结论。
    pub fn mark(&mut self, idx: u16, pass: bool) -> Result<(), u16> {
        let i = match self.find(idx) {
            Some(x) => x,
            None => return Err(E_ORDER),
        };
        if self.crits[i].state != CritState::Missing {
            self.dup_rulings += 1;
            return Err(E_STATE);
        }
        if pass {
            self.crits[i].state = CritState::Passed;
            self.passed += 1;
        } else {
            self.crits[i].state = CritState::Failed;
            self.failed += 1;
        }
        if self.missing == 0 {
            return Err(E_ACCT); // 缺项账穿底
        }
        self.missing -= 1;
        Ok(())
    }

    /// 三账守恒：passed + failed + missing == 登记数。
    pub fn conserved(&self) -> bool {
        self.passed + self.failed + self.missing == self.crits.len() as u32
    }

    /// 通过率（千分比，整数）：passed * 1000 / 总数。
    pub fn pass_permille(&self) -> u32 {
        let total = self.crits.len() as u32;
        if total == 0 {
            return 0;
        }
        (self.passed as u64 * 1000 / total as u64) as u32
    }

    /// 加权通过率（千分比）：Σ(权重·通过) / Σ权重。
    pub fn weighted_permille(&self) -> u32 {
        let mut num = 0u64;
        let mut den = 0u64;
        let mut i = 0usize;
        while i < self.crits.len() {
            let w = self.crits[i].weight as u64;
            den += w;
            if self.crits[i].state == CritState::Passed {
                num += w;
            }
            i += 1;
        }
        if den == 0 {
            return 0;
        }
        (num * 1000 / den) as u32
    }

    /// 对账：三元合取（19 条 / 零缺项 / 百分百）。失败给最具体的诊断码。
    pub fn reconcile(&mut self) -> Result<u32, u16> {
        self.reconciles += 1;
        if self.crits.len() as u32 != CRIT_TOTAL {
            return Err(E_COUNT);
        }
        if !self.conserved() {
            return Err(E_ACCT);
        }
        if self.failed != 0 {
            return Err(E_FAILED);
        }
        if self.missing != 0 {
            return Err(E_MISSING);
        }
        if self.passed != CRIT_TOTAL {
            return Err(E_ACCT);
        }
        if self.pass_permille() != 1000 || self.weighted_permille() != 1000 {
            return Err(E_ACCT);
        }
        Ok(self.passed)
    }

    /// 缺项清单导出（返工定位用）：(判据号, 模块号)。
    pub fn missing_list(&self) -> Vec<(u16, u16)> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < self.crits.len() {
            if self.crits[i].state == CritState::Missing {
                out.push((self.crits[i].idx, self.crits[i].module));
            }
            i += 1;
        }
        out
    }

    /// 模块覆盖：批内 17 张模块号是否都被至少一条判据覆盖。
    pub fn module_coverage(&self) -> u32 {
        let mut seen: Vec<u16> = Vec::new();
        let mut i = 0usize;
        while i < self.crits.len() {
            let m = self.crits[i].module;
            let mut hit = false;
            let mut k = 0usize;
            while k < seen.len() {
                if seen[k] == m {
                    hit = true;
                }
                k += 1;
            }
            if !hit {
                seen.push(m);
            }
            i += 1;
        }
        seen.len() as u32
    }
}

/// 聚合报告：给上游（ktest vfs B07 面）的一页纸结论。
pub struct BatchReport {
    /// 登记数
    pub total: u32,
    /// 通过数
    pub passed: u32,
    /// 失败数
    pub failed: u32,
    /// 缺项数
    pub missing: u32,
    /// 是否绿
    pub green: bool,
    /// 结论码（0 表绿，其余为诊断码）
    pub code: u16,
}

impl BatchReport {
    pub fn build(led: &mut BatchLedger) -> BatchReport {
        let code = match led.reconcile() {
            Ok(_) => 0u16,
            Err(e) => e,
        };
        BatchReport {
            total: led.crits.len() as u32,
            passed: led.passed,
            failed: led.failed,
            missing: led.missing,
            green: code == 0,
            code,
        }
    }

    /// 报告自洽：绿 ⇔ (total==19 且 failed==0 且 missing==0)。
    pub fn consistent(&self) -> bool {
        let want = self.total == CRIT_TOTAL && self.failed == 0 && self.missing == 0;
        want == self.green && (self.green == (self.code == 0))
    }
}

/// 诊断码解释。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_DUP => "判据号重复登记",
        E_UNKNOWN => "判据号/模块号/权重越界",
        E_MISSING => "有未裁决的缺项",
        E_FAILED => "有失败判据",
        E_COUNT => "登记数与批规模 19 不符",
        E_STATE => "同一判据重复裁决",
        E_ACCT => "通过/失败/缺项三数不平",
        E_ORDER => "未登记即裁决",
        _ => "未知诊断码",
    }
}

/// 确定性演练：全登记 → 19/19 全绿 → 缺项/失败注入 → 重复裁决 → 对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：批量登记 19 条，覆盖 17 张模块，缺项 19。
    let mut led = BatchLedger::new();
    if led.register_batch()? != CRIT_TOTAL {
        return Err(E_COUNT);
    }
    if led.missing != CRIT_TOTAL || led.passed != 0 || led.failed != 0 {
        return Err(E_ACCT);
    }
    if led.module_coverage() != MODULE_TOTAL {
        return Err(E_COUNT);
    }
    // 未裁决即对账 → E_MISSING（缺项优先于其它判断）
    match led.reconcile() {
        Err(E_MISSING) => {}
        _ => return Err(E_MISSING),
    }
    hits += 1;

    // R2：逐条裁决全通过 → 19/19 无缺项，通过率 1000‰（判据主断言）。
    let total = led.crits.len() as u16;
    let mut i = 0u16;
    while i < total {
        led.mark(i, true)?;
        i += 1;
    }
    if led.passed != CRIT_TOTAL || led.missing != 0 || led.failed != 0 {
        return Err(E_ACCT);
    }
    if led.pass_permille() != 1000 || led.weighted_permille() != 1000 {
        return Err(E_ACCT);
    }
    if led.reconcile()? != CRIT_TOTAL {
        return Err(E_COUNT);
    }
    let rep = BatchReport::build(&mut led);
    if !rep.green || rep.code != 0 || !rep.consistent() {
        return Err(E_ACCT);
    }
    hits += 1;

    // R3：缺项注入——新账只裁 18 条，reconcile 必须报缺项而非绿。
    let mut l2 = BatchLedger::new();
    l2.register_batch()?;
    let mut k = 0u16;
    while k < 18 {
        l2.mark(k, true)?;
        k += 1;
    }
    if l2.reconcile() != Err(E_MISSING) {
        return Err(E_MISSING);
    }
    if l2.missing_list().len() != 1 {
        return Err(E_ACCT);
    }
    let rep2 = BatchReport::build(&mut l2);
    if rep2.green || rep2.code != E_MISSING || !rep2.consistent() {
        return Err(E_MISSING);
    }
    hits += 1;

    // R4：失败注入——18 过 1 败，reconcile 必须报失败（且优先于缺项判定）。
    let mut l3 = BatchLedger::new();
    l3.register_batch()?;
    let mut m = 0u16;
    while m < 18 {
        l3.mark(m, true)?;
        m += 1;
    }
    l3.mark(18, false)?;
    if l3.reconcile() != Err(E_FAILED) {
        return Err(E_FAILED);
    }
    // 补一次全过也不可能变绿：失败是终态，不可翻转
    if l3.mark(18, true) != Err(E_STATE) {
        return Err(E_STATE);
    }
    if l3.failed != 1 {
        return Err(E_ACCT);
    }
    hits += 1;

    // R5：通过率对照——19 中 18 过 = 947‰（整数截断），加权值另有口径。
    let mut l4 = BatchLedger::new();
    l4.register_batch()?;
    let mut n = 0u16;
    while n < 18 {
        l4.mark(n, true)?;
        n += 1;
    }
    l4.mark(18, false)?;
    if l4.pass_permille() != 947 {
        return Err(E_ACCT); // 18000/19 = 947.36… → 947
    }
    if l4.weighted_permille() >= 1000 {
        return Err(E_ACCT);
    }
    hits += 1;

    // I1：重复裁决 10/10 次全部 E_STATE，且通过数不变。
    let mut d = 0u32;
    let mut q = 0;
    while q < 10 {
        if led.mark(0, true) != Err(E_STATE) {
            return Err(E_STATE);
        }
        d += 1;
        q += 1;
    }
    if d != 10 || led.dup_rulings != 10 {
        return Err(E_ACCT);
    }
    if led.passed != CRIT_TOTAL {
        return Err(E_ACCT);
    }
    hits += 1;

    // I2：越界判据号登记 → E_UNKNOWN；重复登记 → E_DUP；模块号越界 → E_UNKNOWN。
    let mut l5 = BatchLedger::new();
    if l5.register(CRIT_TOTAL as u16, 4124, "J1", 1) != Err(E_UNKNOWN) {
        return Err(E_UNKNOWN);
    }
    l5.register(0, 4124, "J1", 1)?;
    if l5.register(0, 4125, "J1", 1) != Err(E_DUP) {
        return Err(E_DUP);
    }
    if l5.register(1, 4141, "J1", 1) != Err(E_UNKNOWN) {
        return Err(E_UNKNOWN);
    }
    if l5.register(1, 4124, "J1", 0) != Err(E_UNKNOWN) {
        return Err(E_UNKNOWN);
    }
    hits += 1;

    // I3：未登记即裁决 → E_ORDER。
    if l5.mark(5, true) != Err(E_ORDER) {
        return Err(E_ORDER);
    }
    hits += 1;

    // C1：三账终审——led 全绿且三数守恒（R1 一次 + R2 一次 + 报告构建一次 = 3 次对账）。
    if !led.conserved() || led.reconciles != 3 {
        return Err(E_ACCT);
    }
    if !l4.conserved() || l4.missing != 0 || l4.failed != 1 {
        return Err(E_ACCT);
    }
    if led.module_coverage() != MODULE_TOTAL {
        return Err(E_COUNT);
    }
    hits += 1;

    Ok(hits)
}
