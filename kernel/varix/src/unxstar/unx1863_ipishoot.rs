//! UNX-F1863 · 击落发起原语（本地 flush+远程广播判定）
//!
//! 诊断段 0x9C71~0x9C78（IPI 族发起段）。
//! 判据（施工书 J1）：发起原语冻结 v1（先本地后远程/先登记后发送），广播判定断言全过，次序颠倒注入 10/10 检出。

use alloc::vec::Vec;

/// 击落发起诊断码闭集（0x9C71~0x9C78 八码）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShootCode {
    /// 0x9C71 发起登记完成（先登记）。
    Filed = 0x9C71,
    /// 0x9C72 本地 flush 完成（先本地）。
    LocalFlushed = 0x9C72,
    /// 0x9C73 远程广播已发（后远程）。
    RemoteSent = 0x9C73,
    /// 0x9C74 广播判定：单核目标 → 直发不广播。
    UnicastDecided = 0x9C74,
    /// 0x9C75 广播判定：多核目标 → 广播。
    BroadcastDecided = 0x9C75,
    /// 0x9C76 次序颠倒检出（先远程后本地/先发送后登记）。
    OrderViolated = 0x9C76,
    /// 0x9C77 发起闭环完成（登记+本地+远程三账齐）。
    Closed = 0x9C77,
    /// 0x9C78 发起账守恒破坏。
    LedgerBroken = 0x9C78,
}

impl ShootCode {
    /// 三要素：现象。
    pub const fn what(self) -> &'static str {
        match self {
            ShootCode::Filed => "击落发起已登记",
            ShootCode::LocalFlushed => "本地 TLB/流水 flush 完成",
            ShootCode::RemoteSent => "远程 IPI 已发送",
            ShootCode::UnicastDecided => "广播判定为单核直发",
            ShootCode::BroadcastDecided => "广播判定为多核广播",
            ShootCode::OrderViolated => "发起次序颠倒检出",
            ShootCode::Closed => "发起三账闭环",
            ShootCode::LedgerBroken => "发起账守恒破坏",
        }
    }
    /// 三要素：原因。
    pub const fn why(self) -> &'static str {
        match self {
            ShootCode::Filed => "击落请求先落发起账",
            ShootCode::LocalFlushed => "发起核先自清",
            ShootCode::RemoteSent => "本地 flush 后广播",
            ShootCode::UnicastDecided => "目标核掩码单bit",
            ShootCode::BroadcastDecided => "目标核掩码多bit",
            ShootCode::OrderViolated => "远程先于本地或发送先于登记",
            ShootCode::Closed => "三步账目齐备",
            ShootCode::LedgerBroken => "登记数≠闭环数+在途数",
        }
    }
    /// 三要素：建议。
    pub const fn advice(self) -> &'static str {
        match self {
            ShootCode::Filed => "继续本地 flush",
            ShootCode::LocalFlushed => "按广播判定发送",
            ShootCode::RemoteSent => "等待闭环核账",
            ShootCode::UnicastDecided => "无需处置",
            ShootCode::BroadcastDecided => "无需处置",
            ShootCode::OrderViolated => "回退发起并按序重走",
            ShootCode::Closed => "无需处置",
            ShootCode::LedgerBroken => "冻结发起账并上报 0x9C78",
        }
    }
}

/// 发起三步闭集（严格序：File → LocalFlush → RemoteSend）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShootStep {
    /// 1 登记。
    File,
    /// 2 本地 flush。
    LocalFlush,
    /// 3 远程发送。
    RemoteSend,
}

impl ShootStep {
    /// 全序。
    pub const ALL: [ShootStep; 3] = [ShootStep::File, ShootStep::LocalFlush, ShootStep::RemoteSend];

    /// 序号（1 起）。
    pub const fn ordinal(self) -> u8 {
        match self {
            ShootStep::File => 1,
            ShootStep::LocalFlush => 2,
            ShootStep::RemoteSend => 3,
        }
    }
}

/// 发起记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ShootRec {
    /// 目标核掩码。
    pub target_mask: u64,
    /// 广播判定。
    pub broadcast: bool,
    /// 完成步数。
    pub steps: u8,
    /// 闭环。
    pub closed: bool,
}

/// 击落发起引擎。
pub struct ShootEngine {
    /// 发起账。
    recs: Vec<ShootRec>,
    /// 当前发起（None=空闲）。
    active: Option<(u8, u64)>, // (steps, target_mask)
    /// 颠倒检出数。
    violations: u64,
    /// 闭环数。
    closed: u64,
}

impl ShootEngine {
    /// 新引擎。
    pub fn new() -> Self {
        ShootEngine { recs: Vec::new(), active: None, violations: 0, closed: 0 }
    }

    /// 广播判定（独立口径）：掩码单 bit → 单播；多 bit → 广播。
    pub fn broadcast_decide(mask: u64) -> Option<bool> {
        if mask == 0 {
            return None;
        }
        Some(mask & (mask.wrapping_sub(1)) != 0)
    }

    /// 推进一步（严格序）。
    pub fn step(&mut self, s: ShootStep, mask: u64) -> ShootCode {
        match s {
            ShootStep::File => {
                if self.active.is_some() {
                    return ShootCode::LedgerBroken;
                }
                if mask == 0 {
                    return ShootCode::LedgerBroken;
                }
                self.active = Some((1, mask));
                ShootCode::Filed
            }
            ShootStep::LocalFlush => match self.active {
                Some((st, m)) if st == 1 => {
                    self.active = Some((2, m));
                    ShootCode::LocalFlushed
                }
                Some((st, m)) if st >= 2 => {
                    // 先远程后本地：颠倒检出。
                    self.violations += 1;
                    self.recs.push(ShootRec { target_mask: m, broadcast: Self::broadcast_decide(m).unwrap_or(false), steps: st, closed: false });
                    self.active = None;
                    ShootCode::OrderViolated
                }
                _ => ShootCode::LedgerBroken,
            },
            ShootStep::RemoteSend => match self.active {
                Some((st, m)) if st == 2 => {
                    let bc = Self::broadcast_decide(m).unwrap_or(false);
                    self.recs.push(ShootRec { target_mask: m, broadcast: bc, steps: 3, closed: true });
                    self.active = None;
                    self.closed += 1;
                    if bc {
                        ShootCode::BroadcastDecided
                    } else {
                        ShootCode::UnicastDecided
                    }
                }
                Some((st, m)) => {
                    // 未本地 flush 直发：颠倒检出。
                    self.violations += 1;
                    self.recs.push(ShootRec { target_mask: m, broadcast: Self::broadcast_decide(m).unwrap_or(false), steps: st, closed: false });
                    self.active = None;
                    ShootCode::OrderViolated
                }
                None => ShootCode::LedgerBroken,
            },
        }
    }

    /// 次序颠倒注入 10/10：File → RemoteSend（跳过本地 flush）。
    pub fn inject_order_violation(&mut self, mask: u64) -> ShootCode {
        let r1 = self.step(ShootStep::File, mask);
        if r1 != ShootCode::Filed {
            return r1;
        }
        self.step(ShootStep::RemoteSend, mask)
    }

    /// 统计。
    pub const fn stats(&self) -> (u64, u64, usize) {
        (self.closed, self.violations, self.recs.len())
    }

    /// 广播判定账核对：闭环记录的 broadcast 位与独立口径一致。
    pub fn broadcast_ledger_ok(&self) -> bool {
        self.recs.iter().all(|r| r.broadcast == Self::broadcast_decide(r.target_mask).unwrap_or(false))
    }

    /// 账守恒：闭环数 + 颠倒数 == 记录总数，且无在途。
    pub fn ledger_consistent(&self) -> bool {
        self.active.is_none() && self.closed + self.violations == self.recs.len() as u64
    }
}

/// 判据测试（段 0x9C71~0x9C78）。
pub fn run_t863_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX-t863");
    // c1 先登记→本地→远程 闭环。
    let mut e = ShootEngine::new();
    let mut c1 = true;
    c1 &= e.step(ShootStep::File, 0b11) == ShootCode::Filed;
    c1 &= e.step(ShootStep::LocalFlush, 0b11) == ShootCode::LocalFlushed;
    c1 &= e.step(ShootStep::RemoteSend, 0b11) == ShootCode::BroadcastDecided;
    set.add("t863-c1-order-close", c1 && e.ledger_consistent(), "");
    // c2 广播判定：单 bit 单播/多 bit 广播/零掩码拒。
    let d1 = ShootEngine::broadcast_decide(0b100);
    let d2 = ShootEngine::broadcast_decide(0b101);
    let d3 = ShootEngine::broadcast_decide(0);
    set.add("t863-c2-broadcast-decide", d1 == Some(false) && d2 == Some(true) && d3.is_none(), "");
    // c3 次序颠倒注入 10/10 检出。
    let mut e3 = ShootEngine::new();
    let mut v = 0;
    for _ in 0..10u32 {
        if e3.inject_order_violation(0b10) == ShootCode::OrderViolated {
            v += 1;
        }
    }
    set.add("t863-c3-violation-10of10", v == 10 && e3.stats().1 == 10, "");
    // c4 未登记直发检出。
    let mut e4 = ShootEngine::new();
    set.add("t863-c4-unfiled-send", e4.step(ShootStep::RemoteSend, 0b1) == ShootCode::LedgerBroken, "");
    // c5 广播判定账核对。
    set.add("t863-c5-broadcast-ledger", e.broadcast_ledger_ok(), "");
    // c6 零掩码登记拒。
    let mut e6 = ShootEngine::new();
    set.add("t863-c6-zero-mask", e6.step(ShootStep::File, 0) == ShootCode::LedgerBroken, "");
    // c7 单播闭环。
    let mut e7 = ShootEngine::new();
    let _ = e7.step(ShootStep::File, 0b100);
    let _ = e7.step(ShootStep::LocalFlush, 0b100);
    let r7 = e7.step(ShootStep::RemoteSend, 0b100);
    set.add("t863-c7-unicast", r7 == ShootCode::UnicastDecided, "");
    // c8 LCG 512 轮注入：随机掩码与步序，账守恒恒成立。
    let mut seed: u64 = 0x8630_9C71_0000_0001;
    let mut lc = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        seed
    };
    let mut inj_ok = true;
    for _ in 0..512u32 {
        let mut eng = ShootEngine::new();
        for _ in 0..10 {
            let mask = lc() % 256;
            match lc() % 3 {
                0 => {
                    let _ = eng.inject_order_violation(mask);
                }
                _ => {
                    for s in ShootStep::ALL {
                        let r = eng.step(s, mask);
                        if r == ShootCode::LedgerBroken || r == ShootCode::OrderViolated {
                            break;
                        }
                    }
                }
            }
        }
        inj_ok &= eng.ledger_consistent() && eng.broadcast_ledger_ok();
    }
    set.add("t863-c8-lcg-inject", inj_ok, "");
    // 三要素自核。
    let mut tri_ok = true;
    for c in [
        ShootCode::Filed,
        ShootCode::LocalFlushed,
        ShootCode::RemoteSent,
        ShootCode::UnicastDecided,
        ShootCode::BroadcastDecided,
        ShootCode::OrderViolated,
        ShootCode::Closed,
        ShootCode::LedgerBroken,
    ] {
        tri_ok &= !c.what().is_empty() && !c.why().is_empty() && !c.advice().is_empty();
    }
    set.add("t863-c9-triad", tri_ok, "");
    set
}
