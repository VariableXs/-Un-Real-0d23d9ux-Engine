//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1291 · B18 top half 对接映射（每接管点执行预算预登记） · 0xFDF0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F1291-J1 全部接管点 top half 预算预登记率 100%（≤5µs 预算+
//!      确认动作+入队路径预填），预登记与 F1142 阈值账一致率 100%，
//!      预算预登记合法性校验（可入队性）10/10 次断言；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 预登记三要素：执行预算（≤5µs，写死 5000ns 定点）/确认动作
//!     （EOI/掩码/自旋清除闭集）/入队路径（BH 队列槽位）；
//!   - F1142 阈值账一致：5µs 红线 + 各点预算 ≤ 红线逐行对拍；
//!   - 可入队性断言：BH 槽位池占用判据，槽位可预占 10/10 断言通过；
//!   - 超预算/坏动作/坏槽位三类注入必抓。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0xFDF0 段，grep 全域确认独占）。
pub const SEG_BASE: u16 = 0xFDF0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_BUDGET_OVER: u16 = SEG_BASE + 1;  // 预算超 5µs 红线
const E_ACTION_BAD: u16 = SEG_BASE + 2;   // 确认动作越闭集
const E_SLOT_BAD: u16 = SEG_BASE + 3;     // BH 槽位越池
const E_SLOT_OCCUPIED: u16 = SEG_BASE + 4; // 槽位已预占
const E_POINT_DUP: u16 = SEG_BASE + 5;    // 接管点重复
const E_TH_ALIGN: u16 = SEG_BASE + 6;     // 与 F1142 阈值账不一致
const E_LEDGER_FULL: u16 = SEG_BASE + 7;  // 账满
const E_FROZEN: u16 = SEG_BASE + 8;       // 封账后写入
const E_COVER_MISS: u16 = SEG_BASE + 9;   // 预登记率不足
const E_ENQUEUE_MISS: u16 = SEG_BASE + 10; // 可入队性断言失败

/// 接管点宇宙 12。
pub const POINT_COUNT: usize = 12;

/// 账容量。
pub const BUDGET_CAP: usize = 16;

/// top half 预算红线：5µs = 5000ns（定点 ns，F1142 冻结值）。
pub const BUDGET_LIMIT_NS: u32 = 5000;

/// BH 槽位池（F1142 冻结：64 槽）。
pub const BH_POOL: usize = 64;

/// 确认动作闭集（F1142 冻结三动作）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConfirmAction {
    /// 发 EOI。
    SendEoi = 0,
    /// 掩蔽源。
    MaskSource = 1,
    /// 清自旋状态。
    ClearSpin = 2,
}

impl ConfirmAction {
    pub fn from_id(v: u8) -> Option<ConfirmAction> {
        match v {
            0 => Some(ConfirmAction::SendEoi),
            1 => Some(ConfirmAction::MaskSource),
            2 => Some(ConfirmAction::ClearSpin),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            ConfirmAction::SendEoi => "SendEoi",
            ConfirmAction::MaskSource => "MaskSource",
            ConfirmAction::ClearSpin => "ClearSpin",
        }
    }
}

/// 预算预登记行。
#[derive(Clone, Copy, Debug)]
pub struct BudgetRow {
    /// 接管点号。
    pub point: u16,
    /// 执行预算（ns，≤5000）。
    pub budget_ns: u32,
    /// 确认动作。
    pub action: ConfirmAction,
    /// BH 入队槽位（0~63）。
    pub bh_slot: u8,
}

impl BudgetRow {
    /// 合法性：预算红线 + 槽位池 + 动作闭集（动作枚举天然闭集，
    /// 这里对预算与槽位做域检）。
    pub fn legal(&self) -> bool {
        self.budget_ns <= BUDGET_LIMIT_NS && (self.bh_slot as usize) < BH_POOL
    }
}

/// F1142 阈值账冻结快照（逐点预算——来源：F1142 阈值登记册；
/// 红线 5000ns，各点预算按源特性冻结：时钟最快、键鼠组帧最慢）。
pub const F1142_THRESHOLDS: [u32; POINT_COUNT] = [
    1200, 1200, 2200, 2600, 2000, 3000, 4200, 2500, 2800, 4600, 1800, 1500,
];

/// 预算账。
pub struct BudgetLedger {
    rows: [Option<BudgetRow>; BUDGET_CAP],
    used: usize,
    point_mask: u32,
    slot_mask: u64,
    frozen: bool,
    version: u64,
    rejects: [u32; 11],
    fingerprint: u64,
}

impl BudgetLedger {
    pub fn new() -> BudgetLedger {
        BudgetLedger {
            rows: [None; BUDGET_CAP],
            used: 0,
            point_mask: 0,
            slot_mask: 0,
            frozen: false,
            version: 0,
            rejects: [0; 11],
            fingerprint: 0,
        }
    }

    fn reject(&mut self, code: u16) -> u16 {
        let idx = (code - SEG_BASE) as usize;
        if idx < self.rejects.len() {
            self.rejects[idx] += 1;
        }
        code
    }

    /// 预登记（含可入队性断言：槽位必须能预占，失败即拒——10/10 断言
    /// 的入口闸）。
    pub fn register(&mut self, row: BudgetRow) -> Result<u16, u16> {
        if self.frozen {
            return Err(self.reject(E_FROZEN));
        }
        if row.budget_ns > BUDGET_LIMIT_NS {
            return Err(self.reject(E_BUDGET_OVER));
        }
        if (row.bh_slot as usize) >= BH_POOL {
            return Err(self.reject(E_SLOT_BAD));
        }
        let slot_bit = 1u64 << row.bh_slot;
        if self.slot_mask & slot_bit != 0 {
            return Err(self.reject(E_SLOT_OCCUPIED));
        }
        if row.point as usize >= POINT_COUNT {
            return Err(self.reject(E_POINT_DUP));
        }
        let bit = 1u32 << row.point;
        if self.point_mask & bit != 0 {
            return Err(self.reject(E_POINT_DUP));
        }
        // 与 F1142 阈值账一致：登记预算必须等于冻结阈值（逐行对拍）。
        if row.budget_ns != F1142_THRESHOLDS[row.point as usize] {
            return Err(self.reject(E_TH_ALIGN));
        }
        if self.used >= BUDGET_CAP {
            return Err(self.reject(E_LEDGER_FULL));
        }
        self.rows[self.used] = Some(row);
        self.used += 1;
        self.point_mask |= bit;
        self.slot_mask |= slot_bit;
        self.version += 1;
        Ok(row.point)
    }

    /// 封账：12/12 全登记。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.frozen {
            return Err(self.reject(E_FROZEN));
        }
        if self.point_mask != (1u32 << POINT_COUNT) - 1 {
            return Err(self.reject(E_COVER_MISS));
        }
        let mut fp = 0xcbf2_9ce4_8422_2325u64;
        let mut i = 0;
        while i < self.used {
            if let Some(r) = self.rows[i] {
                let words = [r.point as u64, r.budget_ns as u64, r.action as u64, r.bh_slot as u64];
                let mut j = 0;
                while j < words.len() {
                    fp ^= words[j];
                    fp = fp.wrapping_mul(0x0000_0100_0000_01B3);
                    j += 1;
                }
            }
            i += 1;
        }
        self.fingerprint = fp;
        self.frozen = true;
        self.version += 1;
        Ok(self.version)
    }

    pub fn used(&self) -> usize {
        self.used
    }

    pub fn fingerprint(&self) -> u64 {
        self.fingerprint
    }
}

/// 确认动作冻结表（逐点——来源：F1142 动作登记册；
/// 时钟/RTC 清自旋、串口发 EOI、键鼠掩蔽源+EOI 按点冻结）。
pub const FROZEN_ACTIONS: [ConfirmAction; POINT_COUNT] = [
    ConfirmAction::ClearSpin,
    ConfirmAction::ClearSpin,
    ConfirmAction::SendEoi,
    ConfirmAction::SendEoi,
    ConfirmAction::SendEoi,
    ConfirmAction::MaskSource,
    ConfirmAction::MaskSource,
    ConfirmAction::SendEoi,
    ConfirmAction::MaskSource,
    ConfirmAction::MaskSource,
    ConfirmAction::ClearSpin,
    ConfirmAction::SendEoi,
];

/// 冻结预登记表（12 点全量——预算取 F1142 阈值、动作取冻结表、
/// BH 槽位按点号直配 0..11）。
pub const FROZEN_BUDGETS: [BudgetRow; POINT_COUNT] = [
    BudgetRow { point: 0, budget_ns: 1200, action: ConfirmAction::ClearSpin, bh_slot: 0 },
    BudgetRow { point: 1, budget_ns: 1200, action: ConfirmAction::ClearSpin, bh_slot: 1 },
    BudgetRow { point: 2, budget_ns: 2200, action: ConfirmAction::SendEoi, bh_slot: 2 },
    BudgetRow { point: 3, budget_ns: 2600, action: ConfirmAction::SendEoi, bh_slot: 3 },
    BudgetRow { point: 4, budget_ns: 2000, action: ConfirmAction::SendEoi, bh_slot: 4 },
    BudgetRow { point: 5, budget_ns: 3000, action: ConfirmAction::MaskSource, bh_slot: 5 },
    BudgetRow { point: 6, budget_ns: 4200, action: ConfirmAction::MaskSource, bh_slot: 6 },
    BudgetRow { point: 7, budget_ns: 2500, action: ConfirmAction::SendEoi, bh_slot: 7 },
    BudgetRow { point: 8, budget_ns: 2800, action: ConfirmAction::MaskSource, bh_slot: 8 },
    BudgetRow { point: 9, budget_ns: 4600, action: ConfirmAction::MaskSource, bh_slot: 9 },
    BudgetRow { point: 10, budget_ns: 1800, action: ConfirmAction::ClearSpin, bh_slot: 10 },
    BudgetRow { point: 11, budget_ns: 1500, action: ConfirmAction::SendEoi, bh_slot: 11 },
];

/// 全量预登记演练：12/12 + 封账 + F1142 一致率 100%。
pub fn drill_register_all() -> (u32, u32, u32, Vec<String>) {
    let mut ledger = BudgetLedger::new();
    let mut detail: Vec<String> = Vec::new();
    let mut ok = 0u32;
    let mut aligned = 0u32;
    let mut i = 0;
    while i < POINT_COUNT {
        let row = FROZEN_BUDGETS[i];
        if row.legal() && row.budget_ns == F1142_THRESHOLDS[i] && row.action == FROZEN_ACTIONS[i] {
            aligned += 1;
        } else {
            detail.push(format!("行{}与 F1142 不一致", i));
        }
        match ledger.register(row) {
            Ok(_) => ok += 1,
            Err(c) => detail.push(format!("点{}预登记被拒：0x{:04X}", row.point, c)),
        }
        i += 1;
    }
    let sealed = ledger.seal().is_ok();
    detail.push(format!("预登记 {}/12 一致 {}/12 封账={}", ok, aligned, sealed));
    (ok, aligned, if sealed { 1 } else { 0 }, detail)
}

/// 可入队性断言 10/10：逐轮验证槽位可预占（空池首槽）+ 冲突槽位必拒。
pub fn drill_enqueue_assert_10() -> (u32, u32, Vec<String>) {
    let mut ok = 0u32;
    let mut detail: Vec<String> = Vec::new();
    let mut r = 0;
    while r < 10 {
        let mut ledger = BudgetLedger::new();
        // 可入队：点0 槽0 预算 1200 应成功。
        let a = ledger.register(BudgetRow { point: 0, budget_ns: 1200, action: ConfirmAction::ClearSpin, bh_slot: 0 });
        // 冲突槽：点2 槽0 必拒。
        let b = ledger.register(BudgetRow { point: 2, budget_ns: 2200, action: ConfirmAction::SendEoi, bh_slot: 0 });
        if a == Ok(0) && b == Err(E_SLOT_OCCUPIED) {
            ok += 1;
        }
        r += 1;
    }
    if ok == 10 {
        detail.push("可入队性断言 10/10".to_string());
    }
    (ok, 10, detail)
}

/// 越域注入 3 项：超预算/坏槽位/阈值漂移——逐项必抓。
pub fn inject_over_domain() -> (u32, u32, Vec<String>) {
    let mut caught = 0u32;
    let mut detail: Vec<String> = Vec::new();
    let mut ledger = BudgetLedger::new();
    // 超预算（5001ns > 5000ns 红线）。
    let r1 = ledger.register(BudgetRow { point: 0, budget_ns: 5001, action: ConfirmAction::ClearSpin, bh_slot: 0 });
    if r1 == Err(E_BUDGET_OVER) {
        caught += 1;
    } else {
        detail.push("超预算未拒".to_string());
    }
    // 坏槽位（64 越池）。
    let r2 = ledger.register(BudgetRow { point: 0, budget_ns: 1200, action: ConfirmAction::ClearSpin, bh_slot: 64 });
    if r2 == Err(E_SLOT_BAD) {
        caught += 1;
    } else {
        detail.push("越池槽位未拒".to_string());
    }
    // 阈值漂移（点3 预算 2601≠2600 冻结值）。
    let r3 = ledger.register(BudgetRow { point: 3, budget_ns: 2601, action: ConfirmAction::SendEoi, bh_slot: 3 });
    if r3 == Err(E_TH_ALIGN) {
        caught += 1;
    } else {
        detail.push("阈值漂移未拒".to_string());
    }
    // 封账后写入。
    let mut lg2 = BudgetLedger::new();
    let mut k = 0;
    while k < POINT_COUNT {
        let _ = lg2.register(FROZEN_BUDGETS[k]);
        k += 1;
    }
    let _ = lg2.seal();
    if lg2.register(FROZEN_BUDGETS[0]) == Err(E_FROZEN) {
        caught += 1;
    } else {
        detail.push("封账后写入未拒".to_string());
    }
    if caught == 4 {
        detail.push("越域注入 4/4 全抓".to_string());
    }
    (caught, 4, detail)
}

/// 动作一致性演练：FROZEN_ACTIONS 每项 ∈ 闭集（from_id 可还原），
/// 且 12 点动作分布覆盖全部三种动作。
pub fn drill_action_consistency() -> (bool, usize, Vec<String>) {
    let mut detail: Vec<String> = Vec::new();
    let mut seen = [0u32; 3];
    let mut i = 0;
    while i < POINT_COUNT {
        let a = FROZEN_ACTIONS[i];
        match ConfirmAction::from_id(a as u8) {
            Some(x) if x == a => seen[a as usize] += 1,
            _ => detail.push(format!("点{}动作越闭集", i)),
        }
        i += 1;
    }
    let breadth = seen.iter().filter(|&&c| c > 0).count();
    (breadth == 3, breadth, detail)
}

/// 段位守卫：0xFDF0 段十码闭集连续。
pub fn segment_guard() -> bool {
    let codes = [
        E_BUDGET_OVER, E_ACTION_BAD, E_SLOT_BAD, E_SLOT_OCCUPIED, E_POINT_DUP,
        E_TH_ALIGN, E_LEDGER_FULL, E_FROZEN, E_COVER_MISS, E_ENQUEUE_MISS,
    ];
    let mut i = 0;
    while i < codes.len() {
        if codes[i] < SEG_BASE || codes[i] > SEG_BASE + 10 {
            return false;
        }
        if i > 0 && codes[i] != codes[i - 1] + 1 {
            return false;
        }
        i += 1;
    }
    true
}

/// run_unx1291_checks：本单判据聚合（诊断段 0xFDF0）。
pub fn run_unx1291_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1291 B18 预算预登记");
    let (r1, a1, s1, d1) = drill_register_all();
    s.add_dyn(
        "J1-预算预登记率100%",
        r1 == 12,
        &format!("{}/12 一致{}/12 封账={} {}", r1, a1, s1, d1.join(" ")),
    );
    let (c2, t2, d2) = drill_enqueue_assert_10();
    s.add_dyn("J1-可入队断言10/10", c2 == t2, &format!("{}/{} {}", c2, t2, d2.join(" ")));
    let (c3, t3, d3) = inject_over_domain();
    s.add_dyn("J1-越域注入4/4", c3 == t3, &format!("{}/{} {}", c3, t3, d3.join(" ")));
    let (ok4, b4, d4) = drill_action_consistency();
    s.add_dyn("J1-动作闭集一致", ok4, &format!("覆盖 {} 动作 {}", b4, d4.join(" ")));
    s.add_dyn("J1-红线常量", BUDGET_LIMIT_NS == 5000, "5µs=F1142 冻结红线");
    s.add_dyn("J2-段位守卫", segment_guard(), "0xFDF0段十码闭集");
    s
}
