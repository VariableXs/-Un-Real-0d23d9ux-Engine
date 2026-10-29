//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1353 · 压测安全护栏（压测中系统保护与自动熔断） · 0x2E70
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F1353-J1 护栏 ≥4 项（资源水位/时间上限/破坏性动作拦截/自动
//!      熔断）全部生效，熔断触发准确率 100%（注入 10/10 次），熔断后
//!      系统可恢复（恢复断言）10/10 次；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 四护栏闭集：G1 资源水位（队列>90% 拒新）、G2 时间上限（步数
//!     超预算截断）、G3 破坏性动作拦截（清账/改冻结表必拒）、G4 自动
//!     熔断（丢失率超线即停）；
//!   - 熔断状态机：Armed→Fired→Recovering→Armed（恢复后可复用）；
//!   - 熔断准确率 10/10：构造超线样本必熔断 + 线内样本不熔断（反向面）；
//!   - 恢复断言 10/10：熔断后走恢复协议（清队/回配额），护栏重武装。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x2E70 段，grep 全域确认独占）。
pub const SEG_BASE: u16 = 0x2E70;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_WATER_HIGH: u16 = SEG_BASE + 1;  // 水位超限
const E_TIME_OVER: u16 = SEG_BASE + 2;   // 时间上限
const E_DESTRUCTIVE: u16 = SEG_BASE + 3; // 破坏性动作
const E_LOSS_OVER: u16 = SEG_BASE + 4;   // 丢失率超线
const E_STATE_EDGE: u16 = SEG_BASE + 5;  // 熔断态非法边
const E_RECOVER_MISS: u16 = SEG_BASE + 6; // 恢复断言失败
const E_FUSE_MISS: u16 = SEG_BASE + 7;   // 应熔未熔
const E_FALSE_FUSE: u16 = SEG_BASE + 8;  // 不应熔而熔
const E_GUARD_MISS: u16 = SEG_BASE + 9;  // 护栏未生效
const E_LEDGER_FULL: u16 = SEG_BASE + 10; // 记录账满

/// 四护栏闭集（≥4 判准；本账 4）。
pub const GUARD_COUNT: usize = 4;

/// 队列水位红线（90%）。
pub const WATER_LIMIT: u64 = 90;

/// 时间上限（步数预算）。
pub const STEP_BUDGET: usize = 128;

/// 丢失率熔断线（万分比 500 = 5%）。
pub const LOSS_FUSE_BPS: u64 = 500;

/// 队列容量。
pub const QUEUE_CAP: u64 = 4096;

/// 熔断状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FuseState {
    Armed = 0,
    Fired = 1,
    Recovering = 2,
}

/// 合法边：Armed→Fired→Recovering→Armed。
pub fn fuse_edge_legal(from: FuseState, to: FuseState) -> bool {
    matches!(
        (from, to),
        (FuseState::Armed, FuseState::Fired)
            | (FuseState::Fired, FuseState::Recovering)
            | (FuseState::Recovering, FuseState::Armed)
    )
}

/// 护栏面板：四护栏 + 熔断状态。
pub struct GuardPanel {
    /// G1..G4 生效位。
    pub guards_on: [bool; GUARD_COUNT],
    pub fuse: FuseState,
    /// 熔断触发计数。
    pub fired_count: u32,
    rejects: [u32; 11],
}

impl GuardPanel {
    pub fn new() -> GuardPanel {
        GuardPanel {
            guards_on: [true; GUARD_COUNT],
            fuse: FuseState::Armed,
            fired_count: 0,
            rejects: [0; 11],
        }
    }

    fn reject(&mut self, code: u16) -> u16 {
        let idx = (code - SEG_BASE) as usize;
        if idx < self.rejects.len() {
            self.rejects[idx] += 1;
        }
        code
    }

    /// G1 资源水位检查（队列深度 → 允许入队?）。
    pub fn check_water(&mut self, queue: u64) -> Result<(), u16> {
        if !self.guards_on[0] {
            return Err(self.reject(E_GUARD_MISS));
        }
        if queue * 100 > QUEUE_CAP * WATER_LIMIT {
            return Err(self.reject(E_WATER_HIGH));
        }
        Ok(())
    }

    /// G2 时间上限检查。
    pub fn check_time(&mut self, steps: usize) -> Result<(), u16> {
        if !self.guards_on[1] {
            return Err(self.reject(E_GUARD_MISS));
        }
        if steps > STEP_BUDGET {
            return Err(self.reject(E_TIME_OVER));
        }
        Ok(())
    }

    /// G3 破坏性动作拦截（动作码闭集：0=读 1=采样 2=清账 3=改冻结表）。
    pub fn check_action(&mut self, action: u8) -> Result<(), u16> {
        if !self.guards_on[2] {
            return Err(self.reject(E_GUARD_MISS));
        }
        if action >= 2 {
            return Err(self.reject(E_DESTRUCTIVE));
        }
        Ok(())
    }

    /// G4 熔断评估：丢失率（万分比）超线 → Fired。
    pub fn evaluate_loss(&mut self, loss_bps: u64) -> Result<bool, u16> {
        if !self.guards_on[3] {
            return Err(self.reject(E_GUARD_MISS));
        }
        if self.fuse != FuseState::Armed {
            return Ok(false);
        }
        if loss_bps > LOSS_FUSE_BPS {
            if !fuse_edge_legal(self.fuse, FuseState::Fired) {
                return Err(self.reject(E_STATE_EDGE));
            }
            self.fuse = FuseState::Fired;
            self.fired_count += 1;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// 恢复协议：清队+回配额+重武装。
    pub fn recover(&mut self, queue: &mut u64, quota: &mut u64, quota_full: u64) -> Result<(), u16> {
        if self.fuse != FuseState::Fired {
            return Err(self.reject(E_STATE_EDGE));
        }
        if !fuse_edge_legal(self.fuse, FuseState::Recovering) {
            return Err(self.reject(E_STATE_EDGE));
        }
        self.fuse = FuseState::Recovering;
        *queue = 0;
        *quota = quota_full;
        if !fuse_edge_legal(self.fuse, FuseState::Armed) {
            return Err(self.reject(E_STATE_EDGE));
        }
        self.fuse = FuseState::Armed;
        Ok(())
    }
}

/// 熔断准确率 10/10：超线必熔 + 线内不熔。
pub fn drill_fuse_accuracy_10() -> (u32, u32, Vec<String>) {
    let mut ok = 0u32;
    let mut detail: Vec<String> = Vec::new();
    let mut r = 0;
    while r < 10 {
        let mut panel = GuardPanel::new();
        // 超线样本：501+ bps。
        let fired = panel.evaluate_loss(LOSS_FUSE_BPS + 1 + (r as u64));
        // 线内样本：≤500 bps。
        let mut panel2 = GuardPanel::new();
        let not_fired = panel2.evaluate_loss(LOSS_FUSE_BPS - (r as u64 + 1));
        if fired == Ok(true) && panel.fuse == FuseState::Fired && not_fired == Ok(false) && panel2.fuse == FuseState::Armed {
            ok += 1;
        } else {
            detail.push(format!("轮{}熔断判定不符", r));
        }
        r += 1;
    }
    if ok == 10 {
        detail.push("熔断准确率 10/10".to_string());
    }
    (ok, 10, detail)
}

/// 恢复断言 10/10：熔断→恢复→重武装（队清/配额回）10 轮。
pub fn drill_recover_10() -> (u32, u32, Vec<String>) {
    let mut ok = 0u32;
    let mut detail: Vec<String> = Vec::new();
    let mut r = 0;
    while r < 10 {
        let mut panel = GuardPanel::new();
        let _ = panel.evaluate_loss(LOSS_FUSE_BPS + 100);
        let mut queue = QUEUE_CAP;
        let mut quota = 0u64;
        let full = QUEUE_CAP * 8;
        let rec = panel.recover(&mut queue, &mut quota, full);
        if rec.is_ok() && panel.fuse == FuseState::Armed && queue == 0 && quota == full {
            ok += 1;
        } else {
            detail.push(format!("轮{}恢复不符", r));
        }
        r += 1;
    }
    if ok == 10 {
        detail.push("恢复断言 10/10".to_string());
    }
    (ok, 10, detail)
}

/// 四护栏生效演练：G1 水位拒 / G2 时间拒 / G3 破坏性拒 / G4 熔断，
/// 逐护栏验证 + 护栏关闭时显性报 E_GUARD_MISS。
pub fn drill_four_guards() -> (u32, u32, Vec<String>) {
    let mut ok = 0u32;
    let mut detail: Vec<String> = Vec::new();
    let mut panel = GuardPanel::new();
    // G1：队列 95% → 拒。
    if matches!(panel.check_water(QUEUE_CAP * 95 / 100), Err(E_WATER_HIGH)) {
        ok += 1;
    } else {
        detail.push("G1 未拒".to_string());
    }
    // G1 线内：50% 放行。
    if panel.check_water(QUEUE_CAP / 2).is_ok() {
        ok += 1;
    } else {
        detail.push("G1 线内误拒".to_string());
    }
    // G2：超预算拒 / 线内放行。
    if matches!(panel.check_time(STEP_BUDGET + 1), Err(E_TIME_OVER))
        && panel.check_time(STEP_BUDGET).is_ok()
    {
        ok += 1;
    } else {
        detail.push("G2 不符".to_string());
    }
    // G3：清账拒 / 读放行。
    if matches!(panel.check_action(2), Err(E_DESTRUCTIVE)) && panel.check_action(0).is_ok() {
        ok += 1;
    } else {
        detail.push("G3 不符".to_string());
    }
    // G4：熔断生效。
    if panel.evaluate_loss(LOSS_FUSE_BPS + 10) == Ok(true) {
        ok += 1;
    } else {
        detail.push("G4 不符".to_string());
    }
    (ok, 5, detail)
}

/// 护栏关闭显性演练：关 G1 后水位检查报 E_GUARD_MISS（不静默放行）。
pub fn drill_guard_off_explicit() -> (bool, Vec<String>) {
    let mut detail: Vec<String> = Vec::new();
    let mut panel = GuardPanel::new();
    panel.guards_on[0] = false;
    let caught = matches!(panel.check_water(QUEUE_CAP / 2), Err(E_GUARD_MISS));
    if !caught {
        detail.push("护栏关闭未显性".to_string());
    }
    (caught, detail)
}

/// 状态机非法边演练：Armed 直 Recovering / Fired 直 Armed 必拒。
pub fn inject_fuse_edges() -> (u32, u32, Vec<String>) {
    let mut caught = 0u32;
    let mut detail: Vec<String> = Vec::new();
    // Armed 直恢复（未熔先复）。
    let mut p1 = GuardPanel::new();
    let mut q = 0u64;
    let mut quota = 0u64;
    if matches!(p1.recover(&mut q, &mut quota, 1), Err(E_STATE_EDGE)) {
        caught += 1;
    } else {
        detail.push("未熔先复未拒".to_string());
    }
    // 非法边表核验：六条非法边全拒。
    let illegal = [
        (FuseState::Armed, FuseState::Recovering),
        (FuseState::Fired, FuseState::Armed),
        (FuseState::Recovering, FuseState::Fired),
        (FuseState::Armed, FuseState::Armed),
        (FuseState::Fired, FuseState::Fired),
        (FuseState::Recovering, FuseState::Recovering),
    ];
    let mut i = 0;
    let mut all_reject = true;
    while i < illegal.len() {
        if fuse_edge_legal(illegal[i].0, illegal[i].1) {
            all_reject = false;
        }
        i += 1;
    }
    if all_reject {
        caught += 1;
    } else {
        detail.push("非法边有漏".to_string());
    }
    if caught == 2 {
        detail.push("熔断边注入 2/2 全抓".to_string());
    }
    (caught, 2, detail)
}

/// 段位守卫：0x2E70 段十码闭集连续。
pub fn segment_guard() -> bool {
    let codes = [
        E_WATER_HIGH, E_TIME_OVER, E_DESTRUCTIVE, E_LOSS_OVER, E_STATE_EDGE,
        E_RECOVER_MISS, E_FUSE_MISS, E_FALSE_FUSE, E_GUARD_MISS, E_LEDGER_FULL,
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

/// run_unx1353_checks：本单判据聚合（诊断段 0x2E70）。
pub fn run_unx1353_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1353 压测安全护栏");
    let (c1, t1, d1) = drill_fuse_accuracy_10();
    s.add_dyn("J1-熔断准确率10/10", c1 == t1, &format!("{}/{} {}", c1, t1, d1.join(" ")));
    let (c2, t2, d2) = drill_recover_10();
    s.add_dyn("J1-恢复断言10/10", c2 == t2, &format!("{}/{} {}", c2, t2, d2.join(" ")));
    let (c3, t3, d3) = drill_four_guards();
    s.add_dyn("J1-四护栏全生效", c3 == t3, &format!("{}/{} {}", c3, t3, d3.join(" ")));
    let (ok4, d4) = drill_guard_off_explicit();
    s.add_dyn("J1-护栏关闭显性", ok4, &d4.join(" "));
    let (c5, t5, d5) = inject_fuse_edges();
    s.add_dyn("J1-熔断边注入2/2", c5 == t5, &format!("{}/{} {}", c5, t5, d5.join(" ")));
    s.add_dyn("J2-段位守卫", segment_guard(), "0x2E70段十码闭集");
    s
}
