//! 深化层 · F582 按钮防双击（回炉补深主册【设计要点】未实装机制）。
//!
//! 补深四条（判据唯一源：主册 F582 节）：
//! ①「处理态状态机」——空闲→处理中→完成/失败显式回流（基础件只有
//!   processing 布尔 + 超时解除，无回流态）；处理中/窗口内点击温和
//!   忽略且计账；
//! ②「豁免清单引擎」——导航类按钮按 id 注册豁免，位图 O(1) 判定；
//!   豁免直通零副作用（不进处理态、不写防抖账）；
//! ③「双提交注入对账」——N 连点全程账本：窗口内恰好 1 次受理入账；
//! ④「窗口期满恢复受理」——处理完成回流 + 300ms 窗口期满双门放行
//!   （基础件 299/300 边界判据保持不回退）。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;
use crate::istar::btndebounce::{BtnGuard, BtnKind, DEBOUNCE_MS};

// ---------------------------------------------------------------------------
// 处理态状态机
// ---------------------------------------------------------------------------

/// 处理态阶段（基础件 processing bool 之上的显式回流状态机）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Processing,
    Done,
    Failed,
}

/// 处理态状态机（每按钮一台；底层 300ms 防抖窗口仍归基础件 BtnGuard）。
pub struct PhaseMachine {
    guard: BtnGuard,
    phase: Phase,
    ignored: u32,
    settled_ok: u32,
    settled_err: u32,
}

impl PhaseMachine {
    pub fn new(kind: BtnKind) -> PhaseMachine {
        PhaseMachine {
            guard: BtnGuard::new(kind),
            phase: Phase::Idle,
            ignored: 0,
            settled_ok: 0,
            settled_err: 0,
        }
    }

    /// 点击：受理即进 Processing；被窗口拒即温和忽略并计账。
    /// 豁免类（导航/切换）基础件永不进处理态——本机相位保持 Idle。
    pub fn click(&mut self, ms: u64) -> bool {
        let accepted = self.guard.click(ms);
        if accepted {
            if self.guard.processing() {
                self.phase = Phase::Processing;
            }
        } else {
            self.ignored += 1;
        }
        accepted
    }

    /// 处理完成回流：ok → Done、err → Failed。非处理中回流无效（不重复计账）。
    pub fn settle(&mut self, ok: bool) -> bool {
        if self.phase != Phase::Processing {
            return false;
        }
        if ok {
            self.settled_ok += 1;
            self.phase = Phase::Done;
        } else {
            self.settled_err += 1;
            self.phase = Phase::Failed;
        }
        true
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn ignored(&self) -> u32 {
        self.ignored
    }

    pub fn settled_ok(&self) -> u32 {
        self.settled_ok
    }

    pub fn settled_err(&self) -> u32 {
        self.settled_err
    }

    pub fn guard_processing(&self) -> bool {
        self.guard.processing()
    }
}

// ---------------------------------------------------------------------------
// 豁免清单引擎（O(1)）
// ---------------------------------------------------------------------------

/// 豁免清单：按钮 id 位图注册，判定 O(1)（导航/切换类合法连点）。
pub struct ExemptRegistry {
    bits: u32,
}

impl ExemptRegistry {
    pub fn new() -> ExemptRegistry {
        ExemptRegistry { bits: 0 }
    }

    /// 注册豁免（id 0..32；越界安全忽略——清单诚实不扩容）。
    pub fn register(&mut self, id: u8) {
        if id < 32 {
            self.bits |= 1u32 << id;
        }
    }

    /// 豁免判定 O(1)。
    pub fn is_exempt(&self, id: u8) -> bool {
        id < 32 && (self.bits >> id) & 1 == 1
    }

    pub fn len(&self) -> u32 {
        self.bits.count_ones()
    }

    pub fn is_empty(&self) -> bool {
        self.bits == 0
    }
}

impl Default for ExemptRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// 豁免分派：清单内直通受理（零副作用——不碰防抖账）；清单外交基础件。
pub fn dispatch(reg: &ExemptRegistry, id: u8, guard: &mut BtnGuard, ms: u64) -> bool {
    if reg.is_exempt(id) {
        true
    } else {
        guard.click(ms)
    }
}

// ---------------------------------------------------------------------------
// 双提交注入对账账本
// ---------------------------------------------------------------------------

/// 注入对账账本：每次点击 (时刻, 是否受理) 入账（固定 16 笔——够 N 连点走查）。
pub struct ClickLedger {
    entries: [(u64, bool); 16],
    len: usize,
}

impl ClickLedger {
    pub fn new() -> ClickLedger {
        ClickLedger { entries: [(0, false); 16], len: 0 }
    }

    pub fn record(&mut self, ms: u64, accepted: bool) -> bool {
        if self.len < 16 {
            self.entries[self.len] = (ms, accepted);
            self.len += 1;
            true
        } else {
            false
        }
    }

    /// 受理笔数（对账：N 连点窗口内 → 恰好 1）。
    pub fn accepted(&self) -> u32 {
        self.entries[..self.len].iter().filter(|e| e.1).count() as u32
    }

    pub fn len(&self) -> usize {
        self.len
    }
}

impl Default for ClickLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

pub fn run_f582_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new(ISTAR_DOMAIN);

    // 1) 状态机：空闲 → 处理中（点击即进，基础件处理态同步亮）。
    let mut pm = PhaseMachine::new(BtnKind::Submit);
    let a = pm.click(0);
    cs.add(
        "idle to processing on click",
        a && pm.phase() == Phase::Processing && pm.guard_processing(),
        "",
    );

    // 2) 双提交注入对账：8 连点 0..70ms 全程入账——恰好 1 次受理。
    let mut pm2 = PhaseMachine::new(BtnKind::Submit);
    let mut ledger = ClickLedger::new();
    for i in 0..8u64 {
        let ok = pm2.click(i * 10);
        let _ = ledger.record(i * 10, ok);
    }
    cs.add(
        "n rapid clicks exactly one submit",
        ledger.accepted() == 1
            && ledger.len() == 8
            && pm2.ignored() == 7
            && pm2.settled_ok() == 0,
        "",
    );

    // 3) 完成回流 + 窗口期满双门放行：Done 回流后窗口期满再点再受理。
    let s1 = pm2.settle(true);
    let after_settle = pm2.phase() == Phase::Done && pm2.settled_ok() == 1;
    let re = pm2.click(320); // 窗口 300ms 已期满 → 恢复受理
    cs.add(
        "done flows back and rearm after window",
        s1 && after_settle && re && pm2.phase() == Phase::Processing,
        "",
    );

    // 4) 失败回流：Failed 后重试（期满）受理——失败不锁死。
    let mut pm3 = PhaseMachine::new(BtnKind::Submit);
    let _ = pm3.click(0);
    let s2 = pm3.settle(false);
    let failed = pm3.phase() == Phase::Failed && pm3.settled_err() == 1;
    let retry = pm3.click(400);
    cs.add(
        "fail flows back retry accepted",
        s2 && failed && retry && pm3.phase() == Phase::Processing,
        "",
    );

    // 5) 豁免清单 O(1)：注册即判、未注册不豁免、计数诚实。
    let mut reg = ExemptRegistry::new();
    reg.register(3);
    reg.register(7);
    cs.add(
        "exempt registry o1",
        reg.is_exempt(3) && reg.is_exempt(7) && !reg.is_exempt(4) && reg.len() == 2,
        "",
    );

    // 6) 豁免直通零副作用 vs 清单外走防抖：豁免连点不写账、不进处理态。
    let mut g6 = BtnGuard::new(BtnKind::Submit);
    let mut reg6 = ExemptRegistry::new();
    reg6.register(9);
    let ex1 = dispatch(&reg6, 9, &mut g6, 0);
    let ex2 = dispatch(&reg6, 9, &mut g6, 10);
    let first = dispatch(&reg6, 1, &mut g6, 0);
    let plain = dispatch(&reg6, 1, &mut g6, 20);
    cs.add(
        "exempt pass through no side effect",
        ex1 && ex2 && first && !plain && g6.accepted() == 1 && g6.rejected() == 1,
        "",
    );

    // 7) 基础判据不回退：299ms 拒 / 300ms 放（窗口边界钉死）。
    let mut g = BtnGuard::new(BtnKind::Submit);
    let b1 = g.click(0);
    let b2 = g.click(299);
    let b3 = g.click(300);
    cs.add(
        "base window boundary kept",
        b1 && !b2 && b3 && DEBOUNCE_MS == 300,
        "",
    );

    // 8) 账本容量诚实：16 笔封顶、溢出如实报告（不静默吞）。
    let mut l8 = ClickLedger::new();
    let mut all = true;
    for i in 0..20u64 {
        all &= l8.record(i, true);
    }
    cs.add("ledger cap honest", !all && l8.len() == 16, "");

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settle_outside_processing_noop() {
        let mut pm = PhaseMachine::new(BtnKind::Submit);
        assert!(!pm.settle(true)); // Idle 态回流无效
        assert_eq!(pm.settled_ok(), 0);
    }

    #[test]
    fn navigate_never_enters_processing() {
        let mut pm = PhaseMachine::new(BtnKind::Navigate);
        assert!(pm.click(0));
        assert_eq!(pm.phase(), Phase::Idle); // 豁免类不进处理态
    }

    #[test]
    fn duplicate_register_counts_once() {
        let mut r = ExemptRegistry::new();
        r.register(5);
        r.register(5);
        assert_eq!(r.len(), 1);
    }
}
