//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0375 · 页帧池并发原语预留（单写者期声明）· 诊断段 0xf160
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定）：
//!   单写者期声明入账，并发原语占位接口冻结（A2/A3 消费）。
//! 难点分析：
//!   早年内核是单写者（BSP 独占），页帧池不做真锁——但接口形状
//!   必须现在冻结，否则 A2/A3 引入真并发时接口大改。本模块做三件
//!   事：① 单写者期声明入账（一个显性的「现在无锁」令牌，任何
//!   持令者即声明写权——重复持令即双写者违规）；② 占位接口冻结
//!   （lock/unlock/try_lock 签名与语义定死，实现现在是空转）；
//!   ③ 违规检测（同 tick 双持令必抓——为将来的真锁预热测试面）。
//! 纪律约束：
//!   - 令牌显性：写权是拿在手里的东西，不是默认状态；
//!   - 接口冻结：占位签名 = 将来真锁签名，A2/A3 零改动接入；
//!   - 违规必抓：双持令检测 100%（现在抓模拟双写，将来抓真双写）；
//!   - 空转如实：占位实现文档标注「无内存屏障」——不假装有。
//! 成本记账：持/还令 O(1)；违规检测 O(1)。
//! ═══════════════════════════════════════════════════════════════════════

#![allow(dead_code)]

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xf160;

// ─────────────────────────────────────────────────────────────────────
// 一、单写者令牌（显性写权）
// ─────────────────────────────────────────────────────────────────────

/// 写权令牌状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TokenState {
    /// 无主（无人持令）。
    Unheld,
    /// 被持（持有者 id + 持令 tick）。
    Held(u32, u64),
}

/// 单写者期声明账。
pub struct SwrClaim {
    pub state: TokenState,
    /// 违规账：双持令记录（tick, 两个持有者）。
    pub violations: Vec<(u64, u32, u32)>,
    /// 持令次数 / 还令次数（对冲账）。
    pub acquire_n: u32,
    pub release_n: u32,
}

impl SwrClaim {
    pub fn new() -> Self {
        SwrClaim {
            state: TokenState::Unheld,
            violations: Vec::new(),
            acquire_n: 0,
            release_n: 0,
        }
    }

    /// 持令：无主 → 被持。已有主 = 双写者违规（记账但不转移——
    /// 单写者期里违规是 bug，不是仲裁场景）。
    pub fn acquire(&mut self, who: u32, tick: u64) -> bool {
        match self.state {
            TokenState::Held(prev, t) => {
                self.violations.push((tick, prev, who));
                let _ = t;
                false
            }
            TokenState::Unheld => {
                self.state = TokenState::Held(who, tick);
                self.acquire_n += 1;
                true
            }
        }
    }

    /// 还令：只有持有者本人能还。
    pub fn release(&mut self, who: u32) -> bool {
        match self.state {
            TokenState::Held(h, _) if h == who => {
                self.state = TokenState::Unheld;
                self.release_n += 1;
                true
            }
            _ => false,
        }
    }

    /// 对冲账：持令数 - 还令数 = 当前被持状态的一致性。
    pub fn coherent(&self) -> bool {
        let inflight = self.acquire_n - self.release_n;
        matches!(self.state, TokenState::Held(_, _)) == (inflight > 0)
    }
}

// ─────────────────────────────────────────────────────────────────────
// 二、占位并发原语（接口冻结，A2/A3 消费）
// ─────────────────────────────────────────────────────────────────────

/// 占位自旋锁：接口冻结，单写者期实现为空转（无屏障——文档如实）。
pub struct PlaceholderSpin {
    /// 0 = 无主，1 = 被持。单写者期只有 BSP 会碰它。
    flag: u8,
}

impl PlaceholderSpin {
    pub const fn new() -> Self {
        PlaceholderSpin { flag: 0 }
    }

    /// try_lock 语义（将来真锁同签名）：返回是否拿到。
    /// 现阶段：CAS 语义的裸等价（单核单写者下无竞争窗口）。
    pub fn try_lock(&mut self) -> bool {
        if self.flag == 0 {
            self.flag = 1;
            true
        } else {
            false
        }
    }

    /// unlock 语义（将来真锁同签名）。
    pub fn unlock(&mut self) {
        self.flag = 0;
    }

    /// 接口冻结验证：flag 域宽度恒 u8（将来换成原子序，签名不变）。
    pub fn is_locked(&self) -> bool {
        self.flag != 0
    }
}

// ─────────────────────────────────────────────────────────────────────
// 三、判据入口
// ─────────────────────────────────────────────────────────────────────

/// UNX-F0375 判据全集。
pub fn run_unx375_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0375 页帧池并发原语预留");

    // J1 主判据：声明入账 + 占位接口冻结
    let mut cl = SwrClaim::new();
    cs.add("J1 初始无主", cl.state == TokenState::Unheld && cl.coherent(), "对冲账平");
    cs.add("J1 持令", cl.acquire(0, 100), "BSP 拿写权");
    cs.add("J1 被持一致", cl.coherent() && matches!(cl.state, TokenState::Held(0, 100)), "令在手");
    cs.add("J1 还令", cl.release(0) && cl.coherent(), "权归公");
    cs.add("J1 对冲账", cl.acquire_n == 1 && cl.release_n == 1, "借还各一");

    // 注入 1：双持令 → 违规 100% 抓
    let mut c1 = SwrClaim::new();
    c1.acquire(0, 10);
    cs.add("注入1 双持必抓", !c1.acquire(1, 11) && c1.violations.len() == 1, "第二持令者被记");
    cs.add(
        "注入1 违规可解释",
        c1.violations[0] == (11, 0, 1),
        "tick+双方 id",
    );
    cs.add("注入1 原主不失", matches!(c1.state, TokenState::Held(0, 10)), "违规不夺权");

    // 注入 2：冒名还令 → 拒
    let mut c2 = SwrClaim::new();
    c2.acquire(0, 5);
    cs.add("注入2 冒名拒收", !c2.release(1), "非持有人不许还");
    cs.add("注入2 权仍在", matches!(c2.state, TokenState::Held(0, 5)), "令未丢");

    // 注入 3：占位锁接口语义（冻结面）
    let mut sp = PlaceholderSpin::new();
    cs.add("注入3 try_lock 得手", sp.try_lock() && sp.is_locked(), "首拿必成");
    cs.add("注入3 二拿必拒", !sp.try_lock(), "互斥语义在");
    sp.unlock();
    cs.add("注入3 还后可再拿", !sp.is_locked() && sp.try_lock(), "循环可复用");

    // 注入 4：对冲账破口检测（持而不还 → coherent 恒假面）
    let mut c4 = SwrClaim::new();
    c4.acquire(0, 1);
    // 手工破账：只记账不还（模拟泄漏）——coherent 仍应为真（状态一致），
    // 但 inflight=1 >0 且被持 → 真。真正的破口是状态与账不一致。
    cs.add("注入4 持而不还一致", c4.coherent(), "inflight 与状态对齐");
    c4.release(0);
    c4.release(0); // 多还 → release_n=2 > acquire_n=1
    cs.add("注入4 多还可探", c4.release_n > c4.acquire_n, "对冲账显破口");

    cs
}
