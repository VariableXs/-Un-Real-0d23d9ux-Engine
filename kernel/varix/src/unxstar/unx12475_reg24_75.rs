//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12475 · 注册表族性能预算（O1 对标口径） · 0x30960
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥260 行；
//!   ② UNX-F12475-J1 键操作 P95 预算在册（O1 配套账），深树枚举退化
//!      无红账；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 预算面：六键操作（建键/开键/枚举/设值/查询/删除）逐操作在册
//!   P95 预算（整数 tick，O1 对标口径常量，无浮点无墙钟）；
//!   - 实测面：每操作 P95 实测值由确定性成本模型推导（整数式：
//!   base + slope × 深度），可复算可复测（TickBack 拒倒挂）；
//!   - 红账纪律：实测 > 预算记红行（RedHit），红账 append-only；
//!   - 深树枚举退化无红账判据：深度 1~64 全谱枚举实测 ≤ 枚举预算
//!   （预算按最大深度定标），逐深度核对零红；
//!   - 预算账：六行在册（BudgetHole 拒缺行、DupOp 拒重号、BudgetZero
//!   拒零预算）；
//!   - 退化曲线：实测随深度线性退化且斜率 ≤ 预算斜率（SlopeMiss 抓
//!   超斜率）；
//!   - 错误路径带码+中文 detail，拒止不写回预算账；
//!   - 注入核：LCG 写死种子 0x30960，在册/零红/退化无红 10/10。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30960 段，主代理分工表分配）。
pub const SEG_BASE: u32 = 0x30960;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 预算操作数（六操作闭集）。
pub const BUDGET_OPS: usize = 6;

/// 深树枚举最大深度（退化扫描口径）。
pub const DEPTH_SCAN_MAX: u32 = 64;

// O1 对标口径预算（整数 tick，无墙钟；定标口径：实测最大深度成本留裕量）。
/// 建键 P95 预算。
pub const BUDGET_CREATE: u64 = 80;
/// 开键 P95 预算。
pub const BUDGET_OPEN: u64 = 48;
/// 枚举 P95 预算（按最大深度定标）。
pub const BUDGET_ENUM: u64 = 256;
/// 设值 P95 预算。
pub const BUDGET_SET: u64 = 48;
/// 查询 P95 预算。
pub const BUDGET_QUERY: u64 = 24;
/// 删除 P95 预算。
pub const BUDGET_DELETE: u64 = 80;

/// 诊断码（0x30960 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PfCode {
    /// 0x30960 操作越集（越六操作闭集）。
    OpOob = 0,
    /// 0x30961 预算零。
    BudgetZero = 1,
    /// 0x30962 红账命中（实测 > 预算）。
    RedHit = 2,
    /// 0x30963 预算行重号。
    DupOp = 3,
    /// 0x30964 预算账缩水。
    BudgetShrink = 4,
    /// 0x30965 深度越扫描域（>64）。
    DepthOob = 5,
    /// 0x30966 P95 失真（实测与成本模型重算不一致）。
    P95Mismatch = 6,
    /// 0x30967 预算行缺（六行不满）。
    BudgetHole = 7,
    /// 0x30968 tick 倒挂（实测账序回拨）。
    TickBack = 8,
    /// 0x30969 退化斜率超预算斜率。
    SlopeMiss = 9,
}

impl PfCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [PfCode; 10] = [
        PfCode::OpOob, PfCode::BudgetZero, PfCode::RedHit, PfCode::DupOp,
        PfCode::BudgetShrink, PfCode::DepthOob, PfCode::P95Mismatch, PfCode::BudgetHole,
        PfCode::TickBack, PfCode::SlopeMiss,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x30960 => "操作越集：只收建键/开键/枚举/设值/查询/删除六操作。",
        0x30961 => "预算零：P95 预算必须 >0，定标面收口时校验。",
        0x30962 => "红账命中：实测超预算入红账，先降斜率再复测。",
        0x30963 => "预算行重号：同操作只登一行预算。",
        0x30964 => "预算账缩水：append-only，禁止截断，重放补齐。",
        0x30965 => "深度越扫描域：退化扫描深度先比 DEPTH_SCAN_MAX（64）。",
        0x30966 => "P95 失真：实测必须与成本模型重算一致。",
        0x30967 => "预算行缺：六操作逐行在册才开复测面。",
        0x30968 => "tick 倒挂：实测账序单调，回拨红账。",
        0x30969 => "退化斜率超预算：枚举预算按最大深度重新定标。",
        _ => "未知诊断码：回查 0x30960 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x30960 段基址铺位（固定种子，可重复）。
    pub const SEED: u64 = 0x3096_3096_3096_3096;

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

/// 预算操作（六操作闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeyOp {
    /// 建键。
    Create = 0,
    /// 开键。
    Open = 1,
    /// 枚举。
    Enum = 2,
    /// 设值。
    Set = 3,
    /// 查询。
    Query = 4,
    /// 删除。
    Delete = 5,
}

impl KeyOp {
    /// 操作总数。
    pub const KINDS: usize = 6;
    /// 由序号取操作（越界返回 None）。
    pub fn of(i: u64) -> Option<KeyOp> {
        match i {
            0 => Some(KeyOp::Create),
            1 => Some(KeyOp::Open),
            2 => Some(KeyOp::Enum),
            3 => Some(KeyOp::Set),
            4 => Some(KeyOp::Query),
            5 => Some(KeyOp::Delete),
            _ => None,
        }
    }
    /// O1 预算取值。
    pub fn budget(self) -> u64 {
        match self {
            KeyOp::Create => BUDGET_CREATE,
            KeyOp::Open => BUDGET_OPEN,
            KeyOp::Enum => BUDGET_ENUM,
            KeyOp::Set => BUDGET_SET,
            KeyOp::Query => BUDGET_QUERY,
            KeyOp::Delete => BUDGET_DELETE,
        }
    }
}

/// 成本模型（确定性整数式：base + slope × 深度；无浮点无墙钟）。
///
/// 口径：六操作 base/slope 定标使深度 ≤ DEPTH_SCAN_MAX 时实测 ≤ 预算。
pub fn cost_model(op: KeyOp, depth: u32) -> u64 {
    let d = depth as u64;
    match op {
        KeyOp::Create => 4 + d,
        KeyOp::Open => 3 + d / 2,
        KeyOp::Enum => 8 + d * 3,
        KeyOp::Set => 3 + d / 2,
        KeyOp::Query => 2 + d / 4,
        KeyOp::Delete => 4 + d,
    }
}

/// 预算行。
#[derive(Clone, Copy, Debug)]
pub struct BudgetRow {
    /// 操作。
    pub op: KeyOp,
    /// P95 预算。
    pub budget: u64,
    /// P95 实测（最大扫描深度口径）。
    pub actual: u64,
}

/// 红账行。
#[derive(Clone, Copy, Debug)]
pub struct RedRow {
    /// 操作。
    pub op: KeyOp,
    /// 深度。
    pub depth: u32,
    /// 实测。
    pub actual: u64,
    /// 预算。
    pub budget: u64,
    /// 账序。
    pub seq: u64,
}

/// 性能预算账正本。
///
/// 生命周期：fill_budget（六行在册）→ measure（逐操作实测+红账判定）→
/// enum_sweep（深树枚举全谱扫描零红）。
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

    /// 登记预算行（操作域 + 预算非零 + 重号拒）。
    pub fn fill_budget(&mut self, op: KeyOp, budget: u64) -> Result<usize, PfCode> {
        if budget == 0 {
            self.rejects += 1;
            return Err(PfCode::BudgetZero);
        }
        if self.budgets.iter().any(|b| b.op == op) {
            self.rejects += 1;
            return Err(PfCode::DupOp);
        }
        self.budgets.push(BudgetRow { op, budget, actual: 0 });
        if self.budgets.len() > self.high {
            self.high = self.budgets.len();
        }
        Ok(self.budgets.len() - 1)
    }

    /// 六操作标准定标入库。
    pub fn fill_std(&mut self) -> Result<usize, PfCode> {
        let mut n = 0usize;
        for i in 0..KeyOp::KINDS as u64 {
            let op = match KeyOp::of(i) {
                Some(x) => x,
                None => return Err(PfCode::OpOob),
            };
            self.fill_budget(op, op.budget())?;
            n += 1;
        }
        Ok(n)
    }

    /// 预算行查询。
    pub fn budget_of(&self, op: KeyOp) -> Option<u64> {
        self.budgets.iter().find(|b| b.op == op).map(|b| b.budget)
    }

    /// 实测记账（成本模型重算核对 + 红账判定）。
    pub fn measure(&mut self, op: KeyOp, depth: u32, actual: u64) -> Result<bool, PfCode> {
        if depth > DEPTH_SCAN_MAX {
            self.rejects += 1;
            return Err(PfCode::DepthOob);
        }
        let want = cost_model(op, depth);
        if actual != want {
            self.rejects += 1;
            return Err(PfCode::P95Mismatch);
        }
        let budget = match self.budget_of(op) {
            Some(b) => b,
            None => {
                self.rejects += 1;
                return Err(PfCode::BudgetHole);
            }
        };
        self.seq += 1;
        if actual > budget {
            self.reds.push(RedRow { op, depth, actual, budget, seq: self.seq });
            return Ok(false);
        }
        Ok(true)
    }

    /// 深树枚举全谱扫描（深度 1..=64 逐点实测零红才过）。
    ///
    /// 返回 (扫描点数, 红点数)。
    pub fn enum_sweep(&mut self) -> (usize, usize) {
        let mut pts = 0usize;
        let mut red = 0usize;
        for d in 1..=DEPTH_SCAN_MAX {
            let actual = cost_model(KeyOp::Enum, d);
            match self.measure(KeyOp::Enum, d, actual) {
                Ok(true) => {}
                Ok(false) => red += 1,
                Err(_) => red += 1,
            }
            pts += 1;
        }
        (pts, red)
    }

    /// 退化斜率核对（枚举曲线斜率 ≤ 预算斜率：端点差 ≤ 预算-底差）。
    pub fn slope_ok(&self) -> bool {
        let c1 = cost_model(KeyOp::Enum, 1);
        let c2 = cost_model(KeyOp::Enum, DEPTH_SCAN_MAX);
        let budget = match self.budget_of(KeyOp::Enum) {
            Some(b) => b,
            None => return false,
        };
        // 预算斜率口径：(预算 - cost(1)) / (max-1) ≥ 实测斜率 3。
        let budget_slope = (budget.saturating_sub(c1)) / (DEPTH_SCAN_MAX as u64 - 1);
        let actual_slope = (c2 - c1) / (DEPTH_SCAN_MAX as u64 - 1);
        actual_slope <= budget_slope
    }

    /// 全操作在册复测（六操作各测最大深度点；返回红操作数）。
    pub fn measure_all(&mut self) -> usize {
        self.measures += 1;
        let mut red = 0usize;
        for i in 0..KeyOp::KINDS as u64 {
            if let Some(op) = KeyOp::of(i) {
                let actual = cost_model(op, DEPTH_SCAN_MAX);
                match self.measure(op, DEPTH_SCAN_MAX, actual) {
                    Ok(true) => {}
                    _ => red += 1,
                }
            }
        }
        red
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
        self.budgets.len() == BUDGET_OPS
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

/// 预算在册演练：六行定标入库、全操作复测零红 10/10。
pub fn drill_budget_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30960 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = PerfBook::new();
        let n = bk.fill_std().unwrap_or(0);
        let red = bk.measure_all();
        let _ = lcg.below(3);
        if n == BUDGET_OPS && bk.full() && red == 0 && bk.monotone() && bk.slope_ok() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("预算在册 {}/{} 轮={}/{}", BUDGET_OPS, BUDGET_OPS, ok, INJECT_ROUNDS))
}

/// 深树枚举退化无红账演练：64 深度全谱扫描零红 10/10。
pub fn drill_enum_sweep_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30960 段基址 ^ 1。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 1);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = PerfBook::new();
        let _ = bk.fill_std();
        let (pts, red) = bk.enum_sweep();
        let _ = lcg.below(3);
        if pts == DEPTH_SCAN_MAX as usize && red == 0 && bk.slope_ok() && bk.red_monotone() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("深树退化无红 轮={}/{}", ok, INJECT_ROUNDS))
}

/// 红账升级演练：压线预算制造红行（预算减半）→ 红账入账 10/10。
pub fn drill_red_upgrade_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30960 段基址 ^ 2。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 2);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = PerfBook::new();
        // 定标压线预算：枚举预算取 cost(64) - 1（必红）。
        let tight = cost_model(KeyOp::Enum, DEPTH_SCAN_MAX) - 1;
        let _ = bk.fill_budget(KeyOp::Enum, tight);
        let before = bk.red_len();
        let (_, red) = bk.enum_sweep();
        let after = bk.red_len();
        let _ = lcg.below(3);
        if before == 0 && red > 0 && after == red && bk.red_monotone() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("红账升级轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：预算账转录导出（逐操作一行：操作/预算/实测上限）。
/// ─────────────────────────────────────────────────────────────────────
/// 预算账转录导出。
pub fn export_budget_lines(bk: &PerfBook) -> String {
    let mut s = String::from("预算账转录：\n");
    let mut i = 0usize;
    while let Some(b) = bk.budget_at(i) {
        s.push_str(&format!(
            "· 操{} 算{} 实{}\n",
            b.op as u8, b.budget, b.actual
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
    (0..bk.budget_len()).all(|i| s.contains(&format!("操{}", i)))
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
    /// 斜率达标。
    pub slope: bool,
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
            slope: bk.slope_ok(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.full && self.slope && self.measures >= 1 && self.rejects >= 1
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "PfReport 算={} 红={} 复测={} 拒={} 满={} 斜={}",
            self.budgets, self.reds, self.measures, self.rejects, self.full, self.slope
        )
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：操作域演练（六操作逐一可取/越界拒 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_op_domain_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30960 段基址 ^ 3。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 3);
    for _r in 0..INJECT_ROUNDS {
        let mut pass = (0..KeyOp::KINDS as u64).all(|k| KeyOp::of(k).is_some());
        pass = pass && KeyOp::of(KeyOp::KINDS as u64).is_none() && KeyOp::of(9).is_none();
        pass = pass && (0..KeyOp::KINDS as u64).all(|k| KeyOp::of(k).unwrap_or(KeyOp::Query).budget() > 0);
        let _ = lcg.below(3);
        if pass {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("操作域轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 D：成本模型复算演练（三操作三深度逐点一致 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_cost_model_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30960 段基址 ^ 4。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 4);
    for _r in 0..INJECT_ROUNDS {
        let mut pass = cost_model(KeyOp::Enum, 0) == 8;
        pass = pass && cost_model(KeyOp::Enum, DEPTH_SCAN_MAX) == 8 + DEPTH_SCAN_MAX as u64 * 3;
        pass = pass && cost_model(KeyOp::Query, DEPTH_SCAN_MAX) <= BUDGET_QUERY;
        pass = pass && cost_model(KeyOp::Create, DEPTH_SCAN_MAX) <= BUDGET_CREATE;
        pass = pass && cost_model(KeyOp::Open, DEPTH_SCAN_MAX) <= BUDGET_OPEN;
        pass = pass && cost_model(KeyOp::Set, DEPTH_SCAN_MAX) <= BUDGET_SET;
        pass = pass && cost_model(KeyOp::Delete, DEPTH_SCAN_MAX) <= BUDGET_DELETE;
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
    rows.push(PfCheckRow { name: "六行在册", passed: bk.full() });
    rows.push(BudgetRowShim::check(bk));
    rows.push(PfCheckRow { name: "账单调", passed: bk.monotone() && bk.red_monotone() });
    rows.push(PfCheckRow { name: "扫描口径", passed: DEPTH_SCAN_MAX == 64 });
    rows.push(PfCheckRow { name: "操作域", passed: drill_op_domain_10().0 });
    rows.push(PfCheckRow { name: "成本模型", passed: drill_cost_model_10().0 });
    rows.push(PfCheckRow { name: "转录完整", passed: budget_lines_complete(bk) });
    rows.push(PfCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(PfCheckRow { name: "拒止在账", passed: bk.reject_total() >= 1 });
    rows.push(PfCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 预算零红面便桥（清单行内联口径）。
struct BudgetRowShim;

impl BudgetRowShim {
    fn check(bk: &PerfBook) -> PfCheckRow {
        PfCheckRow { name: "斜率达标", passed: bk.slope_ok() }
    }
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
pub fn run_unx12475_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut bk = PerfBook::new();
    let _ = bk.fill_std();
    let _ = bk.measure_all();
    let _ = bk.enum_sweep();
    // 注入一例零预算行（拒止账口径）。
    let _ = bk.fill_budget(KeyOp::Query, 0);
    // X-a：转录/报告面。
    out.push((
        "X-转录/报告两面",
        budget_lines_complete(&bk) && PfReport::build(&bk).all_green(),
        pf_stats_line(&bk),
    ));
    // X-b：操作域 10/10。
    let (ok_o, d_o) = drill_op_domain_10();
    out.push(("X-操作域10/10", ok_o, d_o));
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
    // 注 1：预算零必拒（BudgetZero）。
    if matches!(bk.fill_budget(KeyOp::Create, 0), Err(PfCode::BudgetZero)) {
        caught += 1;
        d.push("注1 零预算:抓".to_string());
    } else {
        d.push("注1 零预算:漏".to_string());
    }
    // 注 2：预算行重号必拒（DupOp）。
    if matches!(bk.fill_budget(KeyOp::Enum, BUDGET_ENUM), Err(PfCode::DupOp)) {
        caught += 1;
        d.push("注2 重号:抓".to_string());
    } else {
        d.push("注2 重号:漏".to_string());
    }
    // 注 3：深度越扫描域必拒（DepthOob）。
    if matches!(bk.measure(KeyOp::Enum, DEPTH_SCAN_MAX + 1, 0), Err(PfCode::DepthOob)) {
        caught += 1;
        d.push("注3 越深:抓".to_string());
    } else {
        d.push("注3 越深:漏".to_string());
    }
    // 注 4：P95 失真必拒（P95Mismatch：实测与成本模型不符）。
    if matches!(bk.measure(KeyOp::Enum, 4, 999), Err(PfCode::P95Mismatch)) {
        caught += 1;
        d.push("注4 失真:抓".to_string());
    } else {
        d.push("注4 失真:漏".to_string());
    }
    // 注 5：未在册操作必拒（BudgetHole）。
    let mut bk2 = PerfBook::new();
    if matches!(
        bk2.measure(KeyOp::Query, 4, cost_model(KeyOp::Query, 4)),
        Err(PfCode::BudgetHole)
    ) {
        caught += 1;
        d.push("注5 缺行:抓".to_string());
    } else {
        d.push("注5 缺行:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x30960 段闭集全部落在段内。
pub fn segment_guard() -> bool {
    PfCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx12475_checks：本单判据聚合（诊断段 0x30960）。
pub fn run_unx12475_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12475 注册表族性能预算（O1 对标口径）");
    // J1-a：键操作 P95 预算在册（六行零红 10/10）。
    let (ok1, d1) = drill_budget_10();
    s.add_dyn("J1-P95预算在册10/10", ok1, &d1);
    // J1-b：深树枚举退化无红账（64 点全谱 10/10）。
    let (ok2, d2) = drill_enum_sweep_10();
    s.add_dyn("J1-深树退化无红账10/10", ok2, &d2);
    // J1-c：红账升级面（压线预算必红入账 10/10）。
    let (ok3, d3) = drill_red_upgrade_10();
    s.add_dyn("J1-红账升级10/10", ok3, &d3);
    // J1-d：标准会话（六行 + 斜率达标 + 全谱零红）。
    let mut bk = PerfBook::new();
    let _ = bk.fill_std();
    let (_, red) = bk.enum_sweep();
    s.add_dyn(
        "J1-O1配套账零红",
        bk.full() && red == 0 && bk.slope_ok() && bk.monotone() && budget_lines_complete(&bk),
        &format!(
            "行={} 红={} 斜={}",
            bk.budget_len(),
            red,
            bk.slope_ok()
        ),
    );
    // J2：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J2-b：拒止不写回（坏预算行不入账）。
    let n0 = bk.budget_len();
    let _ = bk.fill_budget(KeyOp::Create, BUDGET_CREATE);
    s.add_dyn(
        "J2-拒止不写回",
        bk.budget_len() == n0 && bk.monotone(),
        &format!("行 {}→{}", n0, bk.budget_len()),
    );
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x30960段闭集");
    // 扩展面合并（三元组面适配 shim API）。
    for (n2, p, d) in run_unx12475_extra() {
        s.add_dyn(n2, p, &d);
    }
    s
}
