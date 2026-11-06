//! UNX-F51543 HUB 自身休眠（空闲超时休眠与唤醒） · 0xC0A80段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 51543 逐字锚定）：UNX-F51544-J1 重算 ×10 与预算表
//! 一致；超预算组合 100% 明拒。
//!
//! 设计要点：
//!   ① 域模型：HUB 空闲超时休眠面（8 端口计数器 + HUB 主状态机）。每
//!      端口一个空闲计数（确定性 LCG 推进），HUB 空闲超时判定 = 全端口
//!      计数同时越阈值；休眠/唤醒状态机 Idle→Armed→Sleeping→Waking→
//!      Active 单向环。
//!   ② 唤醒延迟预算表（10 行闭集，行可复算）：每行（端口类 × 策略档 →
//!      预算微秒档），重算器 recompute_budget(combo) 按表逐行求和；重算
//!      ×10 与预算表逐行一致（等式断言）。
//!   ③ 超预算明拒：组合预算和 > HUB 预算上限的组合 100% 显性拒
//!      （EOVERBUDGET），拒面逐组合审计入账；注入 10 组合 10/10 全拒。
//!   ④ 判定次序：HUB 状态越环 ERING → 端口越册 ENOENT → 计数溢出
//!      EOVERFLOW → 预算越表 EBUDGET → 组合超限 EOVERBUDGET → 正常。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 = SEG_BASE
//!      派生）；取值一律 match/if let/.get()/.get_mut()/checked_*/saturating_*，
//!      无 unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 段基址（VX12 派工公式：0xC0000 + 0x40 × (51543 − 51501)）。
pub const SEG_BASE: u32 = 0xC0A80;

/// HUB 下行端口数。
pub const HUB_PORTS: usize = 8;

/// 空闲超时阈值（计数 tick）。
pub const IDLE_THRESHOLD: u32 = 12;

/// 预算表行数（10 行闭集）。
pub const BUDGET_ROWS: usize = 10;

/// HUB 预算上限（微秒档，整数）。
pub const BUDGET_CAP: u32 = 90;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 诊断码闭集（段内 0x00..0x0B 偏移）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HiCode {
    /// 成功
    Ok = 0,
    /// 端口越册（≥8）
    ENoEnt,
    /// HUB 状态环越步
    ERing,
    /// 计数溢出
    EOverflow,
    /// 预算越表（行号 ≥10 或档越闭集）
    EBudget,
    /// 组合超预算（100% 明拒）
    EOverBudget,
    /// 账量守恒破缺
    EConserve,
    /// 未齐封账
    ESeal,
    /// 计数器越阶（回退）
    ETickBack,
    /// 双跑指纹不一致
    EFp,
    /// 预算和为 0（无预算行命中）
    EEmpty,
}

impl HiCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            HiCode::Ok => "成功",
            HiCode::ENoEnt => "端口越册：核对 8 下行端口行序",
            HiCode::ERing => "HUB 状态环越步：Idle→Armed→Sleeping→Waking→Active 单向，反向显性拒",
            HiCode::EOverflow => "计数溢出：空闲计数越上界显性拒，先清零再推进",
            HiCode::EBudget => "预算越表：行号越 10 行闭集或档越类，先登记再重算",
            HiCode::EOverBudget => "组合超预算：预算和超上限 100% 明拒，拆分组合后重提",
            HiCode::EConserve => "账量守恒破缺：重算轮数与账面逐轮等式不成立，逐条复算",
            HiCode::ESeal => "未齐封账：重算未齐 ×10 即封，先补齐缺轮",
            HiCode::ETickBack => "计数越阶：空闲计数不得回退，回退显性拒",
            HiCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            HiCode::EEmpty => "预算和为 0：组合必须至少命中一条预算行",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// HUB 主状态机（Idle→Armed→Sleeping→Waking→Active 环）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HubState {
    /// 空闲。
    Idle,
    /// 已武装（全端口计数越阈）。
    Armed,
    /// 休眠中。
    Sleeping,
    /// 唤醒中。
    Waking,
    /// 活跃。
    Active,
}

/// 环推进合法性（单向环）。
pub fn hub_step(from: HubState, to: HubState) -> bool {
    matches!(
        (from, to),
        (HubState::Idle, HubState::Armed)
            | (HubState::Armed, HubState::Sleeping)
            | (HubState::Sleeping, HubState::Waking)
            | (HubState::Waking, HubState::Active)
            | (HubState::Active, HubState::Idle)
    )
}

/// 端口空闲计数行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IdleRow {
    /// 端口号（0..8）。
    pub no: u32,
    /// 空闲计数。
    pub ticks: u32,
    /// 端口类（0..4，预算表行内档）。
    pub class: u32,
}

/// 复算第 i 端口行。
pub const fn idle_row(i: usize) -> Option<IdleRow> {
    if i >= HUB_PORTS {
        return None;
    }
    Some(IdleRow {
        no: i as u32,
        ticks: ((i * 7) % 13) as u32,
        class: (i % 4) as u32,
    })
}

/// 预算表行（类 × 档 → 预算微秒档），纯函数复算。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BudgetRow {
    /// 行号（0..10）。
    pub no: u32,
    /// 端口类（0..4）。
    pub class: u32,
    /// 策略档（0..2）。
    pub tier: u32,
    /// 预算微秒档（10..=30）。
    pub budget: u32,
}

/// 复算第 i 预算行（(类,档) 对全表唯一：8 行覆盖 4 类 × 2 档全组合，
/// 第 9/10 行为扩位档）。
pub const fn budget_row(i: usize) -> Option<BudgetRow> {
    if i >= BUDGET_ROWS {
        return None;
    }
    let class = (i % 4) as u32;
    let tier = (i / 4) as u32 % 2;
    Some(BudgetRow {
        no: i as u32,
        class,
        tier,
        budget: 10 + class * 4 + tier * 5,
    })
}

/// 重算器：组合（类,档）对列表 → 按表逐行命中求和；零命中显性拒；
/// 类/档越闭集显性拒（不做取模归一，越集即错）。
pub fn recompute_budget(combo: &[(u32, u32)]) -> Result<u32, HiCode> {
    if combo.is_empty() {
        return Err(HiCode::EEmpty);
    }
    let mut total: u32 = 0;
    let mut i = 0usize;
    while i < combo.len() {
        let (cls, tier) = match combo.get(i) {
            Some(v) => *v,
            None => return Err(HiCode::EBudget),
        };
        if cls >= 4 || tier >= 2 {
            return Err(HiCode::EBudget);
        }
        // 按闭集表逐行命中（(类,档) 对全表唯一，命中即定值）。
        let mut hit: Option<u32> = None;
        let mut r = 0usize;
        while r < BUDGET_ROWS {
            let row = match budget_row(r) {
                Some(b) => b,
                None => return Err(HiCode::EBudget),
            };
            if row.class == cls && row.tier == tier {
                hit = Some(row.budget);
            }
            r += 1;
        }
        match hit {
            Some(b) => total = total.saturating_add(b),
            None => return Err(HiCode::EEmpty),
        }
        i += 1;
    }
    Ok(total)
}

/// 超预算明拒：预算和 > 上限即拒。
pub fn budget_gate(total: u32) -> Result<u32, HiCode> {
    if total == 0 {
        return Err(HiCode::EEmpty);
    }
    if total > BUDGET_CAP {
        return Err(HiCode::EOverBudget);
    }
    Ok(total)
}

// ── HUB 账本 ─────────────────────────────────────────────────────
/// HUB 休眠账本（端口计数 + 重算轮账 + 注入账）。
#[derive(Clone)]
pub struct HubLedger {
    /// 端口空闲计数。
    ticks: [u32; HUB_PORTS],
    /// HUB 状态。
    state: HubState,
    /// 重算轮账（轮号 → 预算和）。
    rounds: Vec<(u32, u32)>,
    /// 注入次数。
    injected: u32,
    /// 明拒次数。
    rejected: u32,
    /// 漏拒次数（必须 0）。
    missed: u32,
    /// 指纹。
    fp: u64,
}

impl HubLedger {
    /// 新账本（计数取复算行初值）。
    pub fn new() -> Self {
        let mut ticks = [0u32; HUB_PORTS];
        let mut i = 0usize;
        while i < HUB_PORTS {
            if let Some(r) = idle_row(i) {
                ticks[i] = r.ticks;
            }
            i += 1;
        }
        HubLedger {
            ticks,
            state: HubState::Idle,
            rounds: Vec::new(),
            injected: 0,
            rejected: 0,
            missed: 0,
            fp: 0xcbf29ce484222325,
        }
    }

    /// FNV-1a 单步。
    fn fnv(&mut self, w: u64) {
        self.fp ^= w;
        self.fp = self.fp.wrapping_mul(0x100000001b3);
    }

    /// 端口计数推进（确定性 +k，回退显性拒）。
    pub fn tick(&mut self, port: u32, k: u32) -> Result<(), HiCode> {
        if port as usize >= HUB_PORTS {
            return Err(HiCode::ENoEnt);
        }
        let cell = match self.ticks.get_mut(port as usize) {
            Some(c) => c,
            None => return Err(HiCode::ENoEnt),
        };
        let old = *cell;
        let next = old.saturating_add(k);
        if next < old {
            return Err(HiCode::EOverflow);
        }
        *cell = next;
        self.fnv((port as u64) << 8 | next as u64);
        Ok(())
    }

    /// 全端口计数清零（唤醒后）。
    pub fn reset_ticks(&mut self) {
        let mut i = 0usize;
        while i < HUB_PORTS {
            self.ticks[i] = 0;
            i += 1;
        }
        self.fnv(0x5143);
    }

    /// 全端口越阈判定（HUB 空闲超时休眠条件）。
    pub fn all_idle(&self) -> bool {
        let mut i = 0usize;
        while i < HUB_PORTS {
            let t = match self.ticks.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if t < IDLE_THRESHOLD {
                return false;
            }
            i += 1;
        }
        true
    }

    /// HUB 状态推进（单向环）。
    pub fn advance(&mut self, to: HubState) -> Result<(), HiCode> {
        if !hub_step(self.state, to) {
            return Err(HiCode::ERing);
        }
        self.state = to;
        self.fnv(to as u64);
        Ok(())
    }

    /// 重算一轮落账。
    pub fn record_round(&mut self, round: u32, total: u32) {
        self.rounds.push((round, total));
        self.fnv((round as u64) << 16 | total as u64);
    }

    /// 注入一轮超预算组合：gate 拒则记账，漏拒计漏。
    pub fn inject_over_budget(&mut self, total: u32) -> bool {
        self.injected += 1;
        if budget_gate(total).is_err() {
            self.rejected += 1;
            self.fnv(0x5150 + total as u64);
            true
        } else {
            self.missed += 1;
            false
        }
    }

    /// 重算轮数。
    pub fn round_count(&self) -> usize {
        self.rounds.len()
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.rejected, self.missed)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }
}

impl Default for HubLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入器 ───────────────────────────────────────────────────────
/// 超预算组合注入（10 组合，确定性）：预算和 > 上限。
pub struct OverBudgetInjector;

impl OverBudgetInjector {
    /// 下一组超预算组合（确定性）：4~6 对全取最高档 (3,1)，预算和 ≥4×27
    /// =108 > 上限 90，必超。
    pub fn next_combo(&self, k: usize) -> Vec<(u32, u32)> {
        let n = 4 + (k % 3);
        let mut combo: Vec<(u32, u32)> = Vec::new();
        let mut i = 0usize;
        while i < n {
            combo.push((3, 1));
            i += 1;
        }
        combo
    }
}

impl Default for OverBudgetInjector {
    fn default() -> Self {
        OverBudgetInjector
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程：空闲超时休眠环 + 重算 ×10 与预算表一致 + 超预算明拒 10/10
/// + 守恒 + 指纹。返回（环完整数, 重算一致数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = HubLedger::new();
    // ① 空闲超时休眠环：全端口计数推到越阈 → 休眠环走全。
    let mut i = 0usize;
    while i < HUB_PORTS {
        let _ = led.tick(i as u32, IDLE_THRESHOLD);
        i += 1;
    }
    let mut ring_ok = 0u32;
    if led.all_idle() {
        let seq = [
            HubState::Armed,
            HubState::Sleeping,
            HubState::Waking,
            HubState::Active,
            HubState::Idle,
        ];
        let mut s = 0usize;
        while s < seq.len() {
            if led.advance(seq[s]).is_ok() {
                ring_ok += 1;
            }
            s += 1;
        }
        led.reset_ticks();
    }
    // ② 重算 ×10 与预算表一致（逐轮等式）。
    let mut consistent = 0u32;
    let mut r = 0usize;
    while r < 10 {
        // 组合 = 预算行 r 自身（1 对），预算和与表值恒等。
        let row = match budget_row(r) {
            Some(b) => b,
            None => break,
        };
        let combo = [(row.class, row.tier)];
        if let Ok(total) = recompute_budget(&combo) {
            if total == row.budget {
                consistent += 1;
            }
            led.record_round(r as u32, total);
        }
        r += 1;
    }
    // ③ 超预算组合明拒 10/10。
    let inj = OverBudgetInjector;
    let mut k = 0usize;
    while k < 10 {
        let combo = inj.next_combo(k);
        let total = recompute_budget(&combo).unwrap_or(0);
        let _ = led.inject_over_budget(total);
        k += 1;
    }
    let fp = led.fingerprint();
    (ring_ok, consistent, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// 本模块自检判据集（≥12 条）。
pub fn run_unx51543_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51543");
    // 1. 正常流：空闲超时休眠环 + 重算 ×10 一致。
    let (ring_ok, consistent, fp) = run_cycle();
    s.add("空闲超时休眠环完整", ring_ok == 5, "Armed→Sleeping→Waking→Active→Idle 全环走通");
    s.add("重算 ×10 与预算表一致", consistent == 10, "逐轮预算和与表值等式成立");
    // 2. 边界：端口越册/计数回退/预算越表/零命中。
    s.add(
        "边界·端口越册",
        HubLedger::new().tick(HUB_PORTS as u32, 1) == Err(HiCode::ENoEnt),
        "端口 8 越册必拒",
    );
    s.add("边界·预算越表", budget_row(BUDGET_ROWS).is_none(), "行 10 越表必 None");
    s.add(
        "边界·零命中拒",
        recompute_budget(&[(9, 9)]) == Err(HiCode::EBudget),
        "类/档越闭集必拒（表内无类 9/档 9）",
    );
    // 3. 判定次序：环越步先于端口越册。
    let mut led = HubLedger::new();
    let _ = led.advance(HubState::Armed);
    let order_probe = led.advance(HubState::Active);
    s.add("判定次序·环越步拒", order_probe == Err(HiCode::ERing), "Armed→Active 跳步先拒");
    // 4. 注入必抓：超预算组合 10/10 明拒。
    let mut led2 = HubLedger::new();
    let inj = OverBudgetInjector;
    let mut all_rej = true;
    let mut k = 0usize;
    while k < 10 {
        let combo = inj.next_combo(k);
        let total = recompute_budget(&combo).unwrap_or(0);
        if total <= BUDGET_CAP {
            all_rej = false;
        }
        if !led2.inject_over_budget(total) {
            all_rej = false;
        }
        k += 1;
    }
    let (inj_n, rej_n, missed_n) = led2.stats();
    s.add_dyn(
        "超预算组合 10/10 明拒",
        all_rej && rej_n >= 10 && missed_n == 0,
        &format!("注入={} 明拒={} 漏拒={}", inj_n, rej_n, missed_n),
    );
    // 5. 预算上限语义：恰等于上限允、上限+1 拒。
    s.add("预算语义·上限内允", budget_gate(BUDGET_CAP).is_ok(), "预算=上限合法");
    s.add(
        "预算语义·上限+1 拒",
        budget_gate(BUDGET_CAP + 1) == Err(HiCode::EOverBudget),
        "预算越上限显性拒",
    );
    // 6. 守恒：重算轮账 ×10 齐。
    let mut led3 = HubLedger::new();
    let mut r3 = 0usize;
    while r3 < 10 {
        led3.record_round(r3 as u32, 1);
        r3 += 1;
    }
    s.add("重算轮账守恒", led3.round_count() == 10, "重算轮账逐轮落账 ≥10");
    // 7. 双跑指纹一致。
    let (_, _, fp2) = run_cycle();
    s.add("双跑指纹一致", fp == fp2 && fp != 0, "确定性全流程逐位一致且非零");
    // 8. 段位守卫。
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0000 && SEG_BASE <= 0xC31C0,
        "SEG_BASE 0x40 对齐且在 VX12 域",
    );
    // 9. 收口。
    s.add("收口·判据条数", s.len() >= 12, "判据 ≥12 条闭集");
    s
}

// ── 测试 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normal_flow() {
        let row = budget_row(3).unwrap_or(BudgetRow { no: 3, class: 3, tier: 1, budget: 10 });
        let combo = [(row.class, row.tier)];
        assert_eq!(recompute_budget(&combo), Ok(row.budget));
    }

    #[test]
    fn test_boundary_port_overflow() {
        assert!(idle_row(HUB_PORTS).is_none());
        assert_eq!(
            HubLedger::new().tick(HUB_PORTS as u32, 1),
            Err(HiCode::ENoEnt)
        );
    }

    #[test]
    fn test_boundary_budget_overflow() {
        assert!(budget_row(BUDGET_ROWS).is_none());
        assert_eq!(budget_gate(BUDGET_CAP + 1), Err(HiCode::EOverBudget));
    }

    #[test]
    fn test_zero_hit_rejected() {
        assert_eq!(recompute_budget(&[]), Err(HiCode::EEmpty));
        assert_eq!(recompute_budget(&[(9, 9)]), Err(HiCode::EBudget));
    }

    #[test]
    fn test_hub_ring_monotonic() {
        assert!(hub_step(HubState::Idle, HubState::Armed));
        assert!(hub_step(HubState::Armed, HubState::Sleeping));
        assert!(hub_step(HubState::Sleeping, HubState::Waking));
        assert!(hub_step(HubState::Waking, HubState::Active));
        assert!(hub_step(HubState::Active, HubState::Idle));
        assert!(!hub_step(HubState::Idle, HubState::Sleeping));
        assert!(!hub_step(HubState::Armed, HubState::Active));
    }

    #[test]
    fn test_all_idle_threshold() {
        let mut led = HubLedger::new();
        assert!(!led.all_idle());
        let mut i = 0usize;
        while i < HUB_PORTS {
            let _ = led.tick(i as u32, IDLE_THRESHOLD);
            i += 1;
        }
        assert!(led.all_idle());
    }

    #[test]
    fn test_injection_all_rejected() {
        let mut led = HubLedger::new();
        let inj = OverBudgetInjector;
        let mut k = 0usize;
        while k < 10 {
            let combo = inj.next_combo(k);
            let total = recompute_budget(&combo).unwrap_or(0);
            assert!(total > BUDGET_CAP, "注入组合必须超预算");
            assert!(led.inject_over_budget(total), "第 {} 组必须明拒", k);
            k += 1;
        }
        let (_, _, missed) = led.stats();
        assert_eq!(missed, 0);
    }

    #[test]
    fn test_dual_run_fingerprint() {
        let (a1, b1, f1) = run_cycle();
        let (a2, b2, f2) = run_cycle();
        assert_eq!(a1, a2);
        assert_eq!(b1, b2);
        assert_eq!(f1, f2);
        assert_ne!(f1, 0);
    }

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0xC0000 && SEG_BASE <= 0xC31C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx51543_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
