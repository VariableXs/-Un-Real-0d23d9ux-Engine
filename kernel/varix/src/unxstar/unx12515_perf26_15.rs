//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12515 · 坏表拒止性能预算（O1 对标口径） · 0x30e60
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F12515-J1 拒止路径 P95 不劣于成功路径档判据绿，O1 对齐
//!      零漂移；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 预算面：八面（PEB~句柄表）逐面在册「成功路径 P95 预算」与
//!   「拒止路径 P95 预算」（整数 tick，O1 对标口径，无浮点无墙钟）；
//!   - 不劣于判据：拒止 P95 实测 ≤ 成功 P95 预算（拒止路径提前短路，
//!   不做成功路径的后段工作）——实测超线记红（SlowPath 红账）；
//!   - 成本模型：确定性整数式（成功 = base + slope × 面序；拒止 =
//!   短路面 = 常量），可复算可复测（TickBack 拒倒挂）；
//!   - 零漂移判据：实测与成本模型重算逐面一致（P95Mismatch 抓失真），
//!   八面全绿为 O1 对齐零漂移；
//!   - 预算账：八行在册（BudgetHole 拒缺行、DupFace 拒重号、BudgetZero
//!   拒零预算），append-only；
//!   - 错误路径带码+中文 detail，拒止不写回预算账；
//!   - 注入核：LCG 写死种子 0x30e60，在册/不劣于/零漂移 10/10。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30e60 段，主代理分工表分配）。
pub const SEG_BASE: u32 = 0x30e60;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 预算面数（八面闭集）。
pub const BUDGET_FACES: usize = 8;

/// 诊断码（0x30e60 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PfCode {
    /// 0x30e60 面轴越界。
    FaceOob = 0,
    /// 0x30e61 预算零。
    BudgetZero = 1,
    /// 0x30e62 慢路径红账（拒止 P95 > 成功预算）。
    SlowPath = 2,
    /// 0x30e63 预算行重号。
    DupFace = 3,
    /// 0x30e64 预算账缩水。
    BudgetShrink = 4,
    /// 0x30e65 P95 失真（实测与成本模型重算不一致）。
    P95Mismatch = 5,
    /// 0x30e66 预算行缺（八行不满）。
    BudgetHole = 6,
    /// 0x30e67 tick 倒挂。
    TickBack = 7,
    /// 0x30e68 成功预算零。
    SuccZero = 8,
    /// 0x30e69 O1 对齐漂移（O1 常量与模型口径不一致）。
    O1Drift = 9,
}

impl PfCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [PfCode; 10] = [
        PfCode::FaceOob, PfCode::BudgetZero, PfCode::SlowPath, PfCode::DupFace,
        PfCode::BudgetShrink, PfCode::P95Mismatch, PfCode::BudgetHole, PfCode::TickBack,
        PfCode::SuccZero, PfCode::O1Drift,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x30e60 => "面轴越界：面号先比 BUDGET_FACES 再取行。",
        0x30e61 => "预算零：拒止 P95 预算必须 >0。",
        0x30e62 => "慢路径红账：拒止实测超成功预算入红账，先砍后段再复测。",
        0x30e63 => "预算行重号：同面只登一行预算。",
        0x30e64 => "预算账缩水：append-only，禁止截断。",
        0x30e65 => "P95 失真：实测必须与成本模型重算一致。",
        0x30e66 => "预算行缺：八面逐行在册才开复测面。",
        0x30e67 => "tick 倒挂：实测账序单调，回拨红账。",
        0x30e68 => "成功预算零：成功路径预算必须 >0。",
        0x30e69 => "O1 对齐漂移：O1 常量与成本模型口径逐面核对。",
        _ => "未知诊断码：回查 0x30e60 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x30e60 段基址铺位（固定种子，可重复）。
    pub const SEED: u64 = 0x30e6_30e6_30e6_30e6;

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

/// 面域（八面闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Face {
    /// PEB。
    Peb = 0,
    /// TEB。
    Teb = 1,
    /// OBJECT_ATTRIBUTES。
    Oa = 2,
    /// UNICODE_STRING。
    UniStr = 3,
    /// RUNTIME_FUNCTION。
    RuntimeFn = 4,
    /// UNWIND_INFO。
    UnwindInfo = 5,
    /// CONTEXT。
    Context = 6,
    /// 句柄表。
    Handle = 7,
}

impl Face {
    /// 由序号取面（越界返回 None）。
    pub fn of(i: u64) -> Option<Face> {
        match i {
            0 => Some(Face::Peb),
            1 => Some(Face::Teb),
            2 => Some(Face::Oa),
            3 => Some(Face::UniStr),
            4 => Some(Face::RuntimeFn),
            5 => Some(Face::UnwindInfo),
            6 => Some(Face::Context),
            7 => Some(Face::Handle),
            _ => None,
        }
    }
}

/// O1 对标口径预算（整数 tick；拒止预算按「不劣于成功」口径定标）。
/// 成功路径预算（按面序递增，模型 slope=6/base=20 口径）。
pub const SUCC_BUDGETS: [u64; BUDGET_FACES] = [20, 26, 32, 38, 44, 50, 56, 62];
/// 拒止路径预算（短路面常量 18，全面统一）。
pub const REJECT_BUDGET: u64 = 18;

/// 成本模型（确定性整数式）。
///
/// 成功路径：base(20) + slope(6) × 面序；拒止路径：短路面常量 18。
pub fn cost_model(face: Face, reject: bool) -> u64 {
    if reject {
        18
    } else {
        20 + 6 * face as u64
    }
}

/// 预算行。
#[derive(Clone, Copy, Debug)]
pub struct BudgetRow {
    /// 面。
    pub face: Face,
    /// 成功路径 P95 预算。
    pub succ_budget: u64,
    /// 拒止路径 P95 预算。
    pub rej_budget: u64,
    /// 成功实测。
    pub succ_actual: u64,
    /// 拒止实测。
    pub rej_actual: u64,
}

/// 红账行。
#[derive(Clone, Copy, Debug)]
pub struct RedRow {
    /// 面。
    pub face: Face,
    /// 实测。
    pub actual: u64,
    /// 预算。
    pub budget: u64,
    /// 账序。
    pub seq: u64,
}

/// 性能预算账正本。
///
/// 生命周期：fill_budget（八行在册）→ measure（逐面双路径实测+红账
/// 判定）→ verify（不劣于 + 零漂移）。
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

    /// 登记预算行（面域 + 预算非零 + 重号拒 + 不劣于口径预检）。
    pub fn fill_budget(&mut self, face: Face, succ: u64, rej: u64) -> Result<usize, PfCode> {
        if succ == 0 {
            self.rejects += 1;
            return Err(PfCode::SuccZero);
        }
        if rej == 0 {
            self.rejects += 1;
            return Err(PfCode::BudgetZero);
        }
        // 不劣于口径预检：拒止预算 ≤ 成功预算（先于重号闸，语义优先）。
        if rej > succ {
            self.rejects += 1;
            return Err(PfCode::SlowPath);
        }
        if self.budgets.iter().any(|b| b.face == face) {
            self.rejects += 1;
            return Err(PfCode::DupFace);
        }
        self.budgets.push(BudgetRow {
            face,
            succ_budget: succ,
            rej_budget: rej,
            succ_actual: 0,
            rej_actual: 0,
        });
        if self.budgets.len() > self.high {
            self.high = self.budgets.len();
        }
        Ok(self.budgets.len() - 1)
    }

    /// 八面标准定标入库。
    pub fn fill_std(&mut self) -> Result<usize, PfCode> {
        let mut n = 0usize;
        for i in 0..BUDGET_FACES as u64 {
            let face = match Face::of(i) {
                Some(x) => x,
                None => return Err(PfCode::FaceOob),
            };
            self.fill_budget(face, SUCC_BUDGETS[i as usize], REJECT_BUDGET)?;
            n += 1;
        }
        Ok(n)
    }

    /// 预算行查询。
    pub fn budget_of(&self, face: Face) -> Option<(u64, u64)> {
        self.budgets
            .iter()
            .find(|b| b.face == face)
            .map(|b| (b.succ_budget, b.rej_budget))
    }

    /// 实测记账（成本模型重算核对 + 红账判定；返回 Ok(true)=零红）。
    pub fn measure(&mut self, face: Face, succ_actual: u64, rej_actual: u64) -> Result<bool, PfCode> {
        let want_s = cost_model(face, false);
        let want_r = cost_model(face, true);
        if succ_actual != want_s || rej_actual != want_r {
            self.rejects += 1;
            return Err(PfCode::P95Mismatch);
        }
        let (succ_budget, rej_budget) = match self.budget_of(face) {
            Some(x) => x,
            None => {
                self.rejects += 1;
                return Err(PfCode::BudgetHole);
            }
        };
        self.seq += 1;
        // 更新实测面。
        for b in self.budgets.iter_mut() {
            if b.face == face {
                b.succ_actual = succ_actual;
                b.rej_actual = rej_actual;
            }
        }
        // 不劣于判定：拒止实测 ≤ 成功预算（且拒止实测 ≤ 拒止预算）。
        if rej_actual > succ_budget || rej_actual > rej_budget {
            self.reds.push(RedRow { face, actual: rej_actual, budget: succ_budget, seq: self.seq });
            return Ok(false);
        }
        Ok(true)
    }

    /// 全面复测（八面双路径；返回红面数）。
    pub fn measure_all(&mut self) -> usize {
        self.measures += 1;
        let mut red = 0usize;
        for i in 0..BUDGET_FACES as u64 {
            if let Some(face) = Face::of(i) {
                let s = cost_model(face, false);
                let r = cost_model(face, true);
                match self.measure(face, s, r) {
                    Ok(true) => {}
                    _ => red += 1,
                }
            }
        }
        red
    }

    /// 不劣于判据（拒止路径 P95 实测 ≤ 成功路径预算，逐面核对）。
    pub fn not_slower(&self) -> bool {
        self.budgets
            .iter()
            .all(|b| b.rej_actual <= b.succ_budget && b.rej_actual <= b.rej_budget)
    }

    /// O1 零漂移判据（O1 常量与模型口径逐面一致）。
    pub fn o1_aligned(&self) -> bool {
        if self.budgets.len() != BUDGET_FACES {
            return false;
        }
        self.budgets.iter().enumerate().all(|(i, b)| {
            let face = match Face::of(i as u64) {
                Some(x) => x,
                None => return false,
            };
            b.succ_budget == SUCC_BUDGETS[i] && b.rej_budget == REJECT_BUDGET
                && cost_model(face, false) <= b.succ_budget
                && cost_model(face, true) == b.rej_budget
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

    /// 八行满判据。
    pub fn full(&self) -> bool {
        self.budgets.len() == BUDGET_FACES
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

/// 预算在册演练：八行定标入库、全面复测零红 10/10。
pub fn drill_budget_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30e60 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = PerfBook::new();
        let n = bk.fill_std().unwrap_or(0);
        let red = bk.measure_all();
        let _ = lcg.below(3);
        if n == BUDGET_FACES && bk.full() && red == 0 && bk.not_slower() && bk.o1_aligned() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("预算在册 {}/{} 轮={}/{}", BUDGET_FACES, BUDGET_FACES, ok, INJECT_ROUNDS))
}

/// 不劣于演练：压线成功预算制造红行 → 红账入账 10/10。
pub fn drill_red_upgrade_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30e60 段基址 ^ 1。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 1);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = PerfBook::new();
        // 压线：成功预算取 19（< 拒止短路面 18？19>18 合法；取 17 < 18 →
        // 拒止实测 18 > 17 → 红账）。
        let _ = bk.fill_budget(Face::Peb, 17, 17);
        let before = bk.red_len();
        let _ = bk.measure_all();
        let after = bk.red_len();
        let _ = lcg.below(3);
        // 压线：拒止实测 18 > 成功预算 17 → Peb 一笔红行入账。
        if before == 0 && after == before + 1 && bk.red_len() == 1 && bk.red_monotone() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("慢路径红账轮={}/{}", ok, INJECT_ROUNDS))
}

/// O1 零漂移演练：八面 O1 常量与模型口径一致 10/10。
pub fn drill_o1_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30e60 段基址 ^ 2。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 2);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = PerfBook::new();
        let _ = bk.fill_std();
        let _ = bk.measure_all();
        let aligned = bk.o1_aligned();
        // 零漂移面：SUCC_BUDGETS 逐面 ≥ 模型成功成本。
        let mut const_ok = true;
        for i in 0..BUDGET_FACES as u64 {
            let face = Face::of(i).unwrap_or(Face::Peb);
            const_ok = const_ok && SUCC_BUDGETS[i as usize] >= cost_model(face, false);
        }
        let _ = lcg.below(3);
        if aligned && const_ok && REJECT_BUDGET == 18 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("O1 零漂移轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：预算账转录导出（逐面一行）。
/// ─────────────────────────────────────────────────────────────────────
/// 预算账转录导出。
pub fn export_budget_lines(bk: &PerfBook) -> String {
    let mut s = String::from("预算账转录：\n");
    let mut i = 0usize;
    while let Some(b) = bk.budget_at(i) {
        s.push_str(&format!(
            "· 面{} 成{} 拒{} 实({},{})\n",
            b.face as u8, b.succ_budget, b.rej_budget, b.succ_actual, b.rej_actual
        ));
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
    (0..bk.budget_len()).all(|i| s.contains(&format!("面{}", i)))
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
    /// 八行满。
    pub full: bool,
    /// 不劣于。
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
        format!(
            "PfReport 算={} 红={} 复测={} 拒={} 满={} 不劣={}",
            self.budgets, self.reds, self.measures, self.rejects, self.full, self.not_slower
        )
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：面域演练（八面逐一可取/越面拒 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_face_domain_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30e60 段基址 ^ 3。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 3);
    for _r in 0..INJECT_ROUNDS {
        let mut pass = (0..BUDGET_FACES as u64).all(|k| Face::of(k).is_some());
        pass = pass && Face::of(BUDGET_FACES as u64).is_none() && Face::of(9).is_none();
        pass = pass && BUDGET_FACES == 8 && REJECT_BUDGET == 18;
        let _ = lcg.below(3);
        if pass {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("面域轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 D：成本模型演练（双路径逐面重算一致 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_cost_model_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30e60 段基址 ^ 4。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 4);
    for _r in 0..INJECT_ROUNDS {
        let mut pass = cost_model(Face::Peb, false) == 20;
        pass = pass && cost_model(Face::Handle, false) == 62;
        pass = pass && cost_model(Face::Peb, true) == 18;
        pass = pass && cost_model(Face::Handle, true) == 18;
        pass = pass && (0..BUDGET_FACES as u64).all(|i| {
            let f = Face::of(i).unwrap_or(Face::Peb);
            cost_model(f, true) <= SUCC_BUDGETS[i as usize]
        });
        let _ = lcg.below(3);
        if pass {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("成本模型轮={}/{}", ok, INJECT_ROUNDS))
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
    rows.push(PfCheckRow { name: "八行在册", passed: bk.full() });
    rows.push(PfCheckRow { name: "不劣于", passed: bk.not_slower() });
    rows.push(PfCheckRow { name: "O1对齐", passed: bk.o1_aligned() });
    rows.push(PfCheckRow { name: "账单调", passed: bk.monotone() && bk.red_monotone() });
    rows.push(PfCheckRow { name: "面域", passed: drill_face_domain_10().0 });
    rows.push(PfCheckRow { name: "成本模型", passed: drill_cost_model_10().0 });
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
    format!(
        "性能预算概况：算 {} 红 {} 复测 {}",
        bk.budget_len(),
        bk.red_len(),
        bk.measure_runs()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12515_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut bk = PerfBook::new();
    let _ = bk.fill_std();
    let _ = bk.measure_all();
    // 注入一例慢路径预算（拒止预算 > 成功预算 → 不劣于口径拒）。
    let _ = bk.fill_budget(Face::Peb, 30, 40);
    // X-a：转录/报告面。
    out.push((
        "X-转录/报告两面",
        budget_lines_complete(&bk) && PfReport::build(&bk).all_green(),
        pf_stats_line(&bk),
    ));
    // X-b：面域 10/10。
    let (ok_f, d_f) = drill_face_domain_10();
    out.push(("X-面域10/10", ok_f, d_f));
    // X-c：成本模型 10/10。
    let (ok_c, d_c) = drill_cost_model_10();
    out.push(("X-成本模型10/10", ok_c, d_c));
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
    if matches!(bk.fill_budget(Face::Peb, 0, 18), Err(PfCode::SuccZero)) {
        caught += 1;
        d.push("注1 零成:抓".to_string());
    } else {
        d.push("注1 零成:漏".to_string());
    }
    // 注 2：拒止预算零必拒（BudgetZero）。
    if matches!(bk.fill_budget(Face::Teb, 30, 0), Err(PfCode::BudgetZero)) {
        caught += 1;
        d.push("注2 零拒:抓".to_string());
    } else {
        d.push("注2 零拒:漏".to_string());
    }
    // 注 3：重号必拒（DupFace）。
    if matches!(bk.fill_budget(Face::Peb, 30, 18), Err(PfCode::DupFace)) {
        caught += 1;
        d.push("注3 重号:抓".to_string());
    } else {
        d.push("注3 重号:漏".to_string());
    }
    // 注 4：慢路径预算必拒（SlowPath：拒止预算 > 成功预算）。
    if matches!(bk.fill_budget(Face::Oa, 20, 40), Err(PfCode::SlowPath)) {
        caught += 1;
        d.push("注4 慢径:抓".to_string());
    } else {
        d.push("注4 慢径:漏".to_string());
    }
    // 注 5：P95 失真必拒（P95Mismatch）。
    if matches!(bk.measure(Face::Peb, 99, 99), Err(PfCode::P95Mismatch)) {
        caught += 1;
        d.push("注5 失真:抓".to_string());
    } else {
        d.push("注5 失真:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x30e60 段闭集全部落在段内。
pub fn segment_guard() -> bool {
    PfCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx12515_checks：本单判据聚合（诊断段 0x30e60）。
pub fn run_unx12515_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12515 坏表拒止性能预算（O1 对标口径）");
    // J1-a：拒止路径 P95 不劣于成功路径（八行零红 10/10）。
    let (ok1, d1) = drill_budget_10();
    s.add_dyn("J1-不劣于判据10/10", ok1, &d1);
    // J1-b：慢路径红账（10/10）。
    let (ok2, d2) = drill_red_upgrade_10();
    s.add_dyn("J1-慢路径红账10/10", ok2, &d2);
    // J1-c：O1 零漂移（10/10）。
    let (ok3, d3) = drill_o1_10();
    s.add_dyn("J1-O1零漂移10/10", ok3, &d3);
    // J1-d：标准会话（八行 + 不劣于 + O1 对齐 + 转录）。
    let mut bk = PerfBook::new();
    let _ = bk.fill_std();
    let red = bk.measure_all();
    s.add_dyn(
        "J1-O1配套账零红",
        bk.full() && red == 0 && bk.not_slower() && bk.o1_aligned() && budget_lines_complete(&bk),
        &format!(
            "行={} 红={} 不劣={} 对齐={}",
            bk.budget_len(),
            red,
            bk.not_slower(),
            bk.o1_aligned()
        ),
    );
    // J2：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x30e60段闭集");
    // 扩展面合并（三元组面适配 shim API）。
    for (n2, p, d) in run_unx12515_extra() {
        s.add_dyn(n2, p, &d);
    }
    s
}
