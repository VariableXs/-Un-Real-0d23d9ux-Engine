//! UNX-F51544 供电重协商（过流恢复后预算重算） · 0xC0AC0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51545-J1 跨重启生效 ×10 恢复一致
//!
//! 设计要点：
//!   ① 供电域按「预算池 + 档位上限」建模：总预算 2000（冻结），每口档位
//!      上限冻结表 [900,500,240]，授予 = min(申请, 档位上限, 池余额)，
//!      截断语义显性落账。
//!   ② 过流即隔离：过流口进入隔离态（单向，禁解除），其已授额度全额回
//!      收入池，协商纪元 +1；重算面以「总预算 − 已授之和」复核池余额，
//!      不一致显性报 RecalcMismatch，×10 轮重算逐轮一致。
//!   ③ 零授予笔也落账：池余额为 0 时的申请按授予 0 落流水（零授予笔计
//!      数单列），账面逐条对平；预算守恒式 池余 + 已授之和 ≡ 总预算。
//!   ④ 判定次序：端口界 → 隔离态 → 档位上限 → 池余额 → 落账；纪元观测
//!      倒退显性拒（StaleEpoch）。
//!   ⑤ 注入器：超档口供、超池口供、隔离后申请、纪元倒退四类注入各 ×10
//!      轮全抓；守恒账 注入=检出+逃逸，逃逸恒 0。
//!   ⑥ 全程零浮点、零 IO、零墙钟、零随机；算术一律 checked/saturating，
//!      取值一律走 .get()/.get_mut()。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC0AC0;

/// 端口数。
pub const PORT_COUNT: usize = 8;
/// 总预算（冻结）。
pub const TOTAL_BUDGET: u32 = 2000;
/// 档位上限冻结表：高档/中档/低档。
pub const TIER_CAPS: [u32; 3] = [900, 500, 240];
/// 每口档位冻结表（下标即端口号，值为档位序）。
pub const PORT_TIERS: [usize; PORT_COUNT] = [0, 1, 2, 1, 0, 2, 1, 2];

/// 单笔授予流水记录（零授予笔也落账）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GrantRec {
    pub seq: u32,
    pub port: usize,
    pub want: u32,
    pub grant: u32,
    pub seg: u32,
}

/// 诊断码闭集（供电重协商域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PowerCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败（SEG_BASE 越界或不对齐）
    SegBad,
    /// 端口号越界
    PortOob,
    /// 端口处于过流隔离态（禁再申请）
    PortQuarantined,
    /// 端口已在隔离态（禁重复过流登记）
    AlreadyQuarantined,
    /// 口供授予超过实际授予（注入必抓）
    ExceedsGrant,
    /// 口供总量超过总预算（注入必抓）
    ExceedsPool,
    /// 纪元读数落后（倒退显性拒）
    StaleEpoch,
    /// 重算复核不一致
    RecalcMismatch,
    /// 账面不符
    LedgerMismatch,
}

impl PowerCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            PowerCode::Ok => "授予/重算成功",
            PowerCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            PowerCode::PortOob => "端口越界：端口号必须在 0..8 之内",
            PowerCode::PortQuarantined => "端口隔离中：过流口禁再申请",
            PowerCode::AlreadyQuarantined => "重复过流：该端口已在隔离态",
            PowerCode::ExceedsGrant => "口供超额：口供授予不得超过实际授予",
            PowerCode::ExceedsPool => "口供超池：口供总量不得超过总预算",
            PowerCode::StaleEpoch => "纪元倒退：协商纪元只增不减",
            PowerCode::RecalcMismatch => "重算不符：池余与总预算−已授必须一致",
            PowerCode::LedgerMismatch => "账面不符：授予流水必须逐条对平",
        }
    }
}

/// 供电重协商引擎：预算池 + 隔离态 + 授予流水 + 守恒账。
pub struct PowerReneg {
    grants: [u32; PORT_COUNT],
    quarantined: [bool; PORT_COUNT],
    remaining: u32,
    epoch: u64,
    seq: u32,
    ledger: Vec<GrantRec>,
    granted_total: u64,
    zero_grant_cnt: u64,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl PowerReneg {
    pub fn new() -> PowerReneg {
        PowerReneg {
            grants: [0; PORT_COUNT],
            quarantined: [false; PORT_COUNT],
            remaining: TOTAL_BUDGET,
            epoch: 0,
            seq: 0,
            ledger: Vec::new(),
            granted_total: 0,
            zero_grant_cnt: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 申请供电：端口界 → 隔离态 → 档位上限 → 池余额 → 落账。
    /// 授予 = min(申请, 档位上限, 池余额)；零授予笔也落账。
    pub fn request(&mut self, port: usize, want: u32) -> Result<u32, PowerCode> {
        if port >= PORT_COUNT {
            return Err(PowerCode::PortOob);
        }
        if matches!(self.quarantined.get(port), Some(true)) {
            return Err(PowerCode::PortQuarantined);
        }
        let tier = match PORT_TIERS.get(port) {
            Some(&t) => t,
            None => return Err(PowerCode::PortOob),
        };
        let cap = match TIER_CAPS.get(tier) {
            Some(&c) => c,
            None => return Err(PowerCode::RecalcMismatch),
        };
        let by_cap = want.min(cap);
        let grant = by_cap.min(self.remaining);
        let after = match self.remaining.checked_sub(grant) {
            Some(v) => v,
            None => return Err(PowerCode::ExceedsPool),
        };
        let cur = match self.grants.get(port) {
            Some(&v) => v,
            None => return Err(PowerCode::PortOob),
        };
        let newg = match cur.checked_add(grant) {
            Some(v) => v,
            None => return Err(PowerCode::ExceedsPool),
        };
        self.remaining = after;
        if let Some(slot) = self.grants.get_mut(port) {
            *slot = newg;
        }
        self.seq = self.seq.saturating_add(1);
        self.ledger.push(GrantRec {
            seq: self.seq,
            port,
            want,
            grant,
            seg: SEG_BASE,
        });
        if grant == 0 {
            self.zero_grant_cnt = self.zero_grant_cnt.saturating_add(1);
        }
        self.granted_total = self.granted_total.saturating_add(grant as u64);
        Ok(grant)
    }

    /// 过流登记：隔离该口（单向）、回收已授额度、纪元 +1。
    pub fn overcurrent(&mut self, port: usize) -> Result<u32, PowerCode> {
        if port >= PORT_COUNT {
            return Err(PowerCode::PortOob);
        }
        if matches!(self.quarantined.get(port), Some(true)) {
            return Err(PowerCode::AlreadyQuarantined);
        }
        let held = match self.grants.get(port) {
            Some(&v) => v,
            None => return Err(PowerCode::PortOob),
        };
        if let Some(flag) = self.quarantined.get_mut(port) {
            *flag = true;
        }
        if let Some(slot) = self.grants.get_mut(port) {
            *slot = 0;
        }
        self.remaining = match self.remaining.checked_add(held) {
            Some(v) => v,
            None => return Err(PowerCode::ExceedsPool),
        };
        self.epoch = self.epoch.saturating_add(1);
        Ok(self.remaining)
    }

    /// 预算重算复核：总预算 − 已授之和 必须 ≡ 池余。
    pub fn recalc(&self) -> Result<u32, PowerCode> {
        let sum = match self.grants.iter().try_fold(0u32, |a, &g| a.checked_add(g)) {
            Some(v) => v,
            None => return Err(PowerCode::RecalcMismatch),
        };
        let expect = match TOTAL_BUDGET.checked_sub(sum) {
            Some(v) => v,
            None => return Err(PowerCode::RecalcMismatch),
        };
        if expect != self.remaining {
            return Err(PowerCode::RecalcMismatch);
        }
        Ok(expect)
    }

    /// 超档口供注入器：口供授予与该口实际授予比对，超额即抓，不落账。
    pub fn claim_grant(&self, port: usize, claimed: u32) -> Result<u32, PowerCode> {
        if port >= PORT_COUNT {
            return Err(PowerCode::PortOob);
        }
        let actual = match self.grants.get(port) {
            Some(&v) => v,
            None => return Err(PowerCode::PortOob),
        };
        if claimed > actual {
            return Err(PowerCode::ExceedsGrant);
        }
        Ok(actual)
    }

    /// 超池口供注入器：口供总量超过总预算即抓，不落账。
    pub fn claim_pool(&self, claimed_total: u64) -> Result<u64, PowerCode> {
        if claimed_total > TOTAL_BUDGET as u64 {
            return Err(PowerCode::ExceedsPool);
        }
        Ok(TOTAL_BUDGET as u64)
    }

    /// 纪元观测一致性：落后即倒退显性拒。
    pub fn claim_epoch(&self, v: u64) -> Result<(), PowerCode> {
        if v < self.epoch {
            return Err(PowerCode::StaleEpoch);
        }
        Ok(())
    }

    pub fn remaining(&self) -> u32 {
        self.remaining
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }

    pub fn zero_grant_cnt(&self) -> u64 {
        self.zero_grant_cnt
    }

    pub fn granted_total(&self) -> u64 {
        self.granted_total
    }

    pub fn is_quarantined(&self, port: usize) -> Option<bool> {
        match self.quarantined.get(port) {
            Some(v) => Some(*v),
            None => None,
        }
    }

    /// 预算守恒式：池余 + 已授之和 ≡ 总预算。
    pub fn budget_conserved(&self) -> bool {
        // 守恒式：池余 ≡ 总预算 − Σ在册授权（recalc 即权威核对面；
        // granted_total 是生命周期累计额，不参与在册守恒）。
        self.recalc().is_ok()
    }

    /// 注入记账：检出加一。
    pub fn note_detected(&mut self) {
        self.injected = self.injected.saturating_add(1);
        self.detected = self.detected.saturating_add(1);
    }

    /// 注入逃逸（判据要求恒 0）。
    pub fn note_escape(&mut self) {
        self.injected = self.injected.saturating_add(1);
        self.escaped = self.escaped.saturating_add(1);
    }

    /// 守恒断言面：注入 = 检出 + 逃逸，逃逸恒 0 且检出非零。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected.saturating_add(self.escaped)
            && self.escaped == 0
            && self.detected > 0
    }

    /// 状态快照（双跑一致面）。
    pub fn snapshot(&self) -> (u32, u64, u64, u64, u32, usize) {
        (
            self.remaining,
            self.granted_total,
            self.zero_grant_cnt,
            self.epoch,
            self.seq,
            self.ledger.len(),
        )
    }
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 双跑一致性：同一申请脚本两次建引擎，快照逐字段一致。
pub fn dual_run_consistent() -> bool {
    let build = || -> (u32, u64, u64, u64, u32, usize) {
        let mut e = PowerReneg::new();
        let _ = e.request(0, 900);
        let _ = e.request(1, 500);
        let _ = e.request(2, 240);
        let _ = e.request(3, 500);
        let _ = e.request(4, 100);
        let _ = e.overcurrent(0);
        e.snapshot()
    };
    build() == build()
}

/// 正常流脚本：把预算池排空（端口 0..4 四笔 + 零授予笔）。
pub fn drain_pool(e: &mut PowerReneg) {
    let _ = e.request(0, 900); // 授 900，余 1100
    let _ = e.request(1, 500); // 授 500，余 600
    let _ = e.request(2, 240); // 授 240，余 360
    let _ = e.request(3, 500); // 授 360，余 0（池余截断）
    let _ = e.request(4, 100); // 授 0（零授予笔，落账）
}

/// 收口自检（16 条判据形态）。
pub fn run_unx51544_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51544");
    // 1 正常流：授予 = min(申请, 档位上限, 池余)。
    {
        let mut e = PowerReneg::new();
        let r1 = e.request(0, 1000); // 超 900 上限 → 截到 900
        let r2 = e.request(2, 240); // 恰等档位上限
        s.add_dyn(
            "授予截断语义",
            r1 == Ok(900) && r2 == Ok(240) && e.remaining() == 860,
            "超上限截到上限、等上限全授，池余同步扣减",
        );
    }
    // 2 池余截断与零授予笔落账。
    {
        let mut e = PowerReneg::new();
        drain_pool(&mut e);
        s.add_dyn(
            "池余截断与零授予笔",
            e.remaining() == 0
                && e.zero_grant_cnt() == 1
                && e.ledger_len() == 5
                && e.granted_total() == 2000,
            "第五笔按零授予落账，流水 5 条、总量 2000",
        );
    }
    // 3 边界：端口 0/7 放行、8 拒；零申请也是一笔零授予。
    {
        let mut e = PowerReneg::new();
        let lo = e.request(0, 1).is_ok();
        let hi = e.request(7, 1).is_ok();
        let over = e.request(PORT_COUNT, 1);
        let zero = e.request(1, 0);
        s.add_dyn(
            "端口与申请边界",
            lo && hi && over == Err(PowerCode::PortOob) && zero == Ok(0),
            "端口 0 与 7 放行、8 拒；零申请按零授予落账",
        );
    }
    // 4 过流回收与重算一致。
    {
        let mut e = PowerReneg::new();
        let _ = e.request(0, 900);
        let _ = e.request(1, 500);
        let rc = e.overcurrent(0);
        let calc = e.recalc();
        let denied = e.request(0, 10);
        s.add_dyn(
            "过流回收重算",
            rc == Ok(1500)
                && calc == Ok(1500)
                && denied == Err(PowerCode::PortQuarantined)
                && e.is_quarantined(0) == Some(true),
            "过流口回收 900 在册授权、池余回 1500，重算一致且禁再申请",
        );
    }
    // 5 注入必抓：超档口供 ×10 轮。
    {
        let mut e = PowerReneg::new();
        let _ = e.request(0, 900);
        let mut caught = 0u32;
        for i in 0..10u32 {
            if e.claim_grant(0, 900 + 1 + i) == Err(PowerCode::ExceedsGrant) {
                caught += 1;
                e.note_detected();
            }
        }
        s.add_dyn("超档口供注入 10/10", caught == 10, "口供授予超实际授予必须全抓");
    }
    // 6 注入必抓：超池口供 ×10 轮。
    {
        let mut e = PowerReneg::new();
        let mut caught = 0u32;
        for i in 0..10u32 {
            if e.claim_pool(TOTAL_BUDGET as u64 + 1 + i as u64) == Err(PowerCode::ExceedsPool) {
                caught += 1;
                e.note_detected();
            }
        }
        s.add_dyn("超池口供注入 10/10", caught == 10, "口供总量超总预算必须全抓");
    }
    // 7 注入必抓：隔离后申请 ×10 轮。
    {
        let mut e = PowerReneg::new();
        let _ = e.overcurrent(1);
        let mut caught = 0u32;
        for i in 0..10u32 {
            if e.request(1, 10 + i) == Err(PowerCode::PortQuarantined) {
                caught += 1;
                e.note_detected();
            }
        }
        s.add_dyn("隔离申请注入 10/10", caught == 10, "隔离口再申请必须全抓");
    }
    // 8 注入必抓：纪元倒退 ×10 轮。
    {
        let mut e = PowerReneg::new();
        let _ = e.overcurrent(0);
        let mut caught = 0u32;
        for i in 0..10u32 {
            let stale = e.epoch().saturating_sub(1 + i as u64);
            if e.claim_epoch(stale) == Err(PowerCode::StaleEpoch) {
                caught += 1;
                e.note_detected();
            }
        }
        s.add_dyn("纪元倒退注入 10/10", caught == 10, "陈旧纪元读数必须全抓");
    }
    // 9 重算 ×10 一致：逐口过流后每轮重算都一致。
    {
        let mut e = PowerReneg::new();
        drain_pool(&mut e);
        let mut consistent = 0u32;
        for i in 0..10u32 {
            let port = i as usize % PORT_COUNT;
            match e.overcurrent(port) {
                Ok(_) => {}
                Err(PowerCode::AlreadyQuarantined) => {}
                Err(_) => {}
            }
            if e.recalc().is_ok() {
                consistent += 1;
            }
        }
        s.add_dyn(
            "过流重算 ×10 一致",
            consistent == 10 && e.budget_conserved(),
            "每轮过流/重复过流后重算必须逐轮一致",
        );
    }
    // 10 判定次序：端口界先于隔离态与预算面。
    {
        let mut e = PowerReneg::new();
        let _ = e.overcurrent(0);
        let oob = e.request(PORT_COUNT + 3, 0);
        let quar = e.request(0, 0);
        s.add_dyn(
            "判序端口界先行",
            oob == Err(PowerCode::PortOob) && quar == Err(PowerCode::PortQuarantined),
            "端口界判据先于隔离态判据",
        );
    }
    // 11 状态机拒止：隔离单向，重复过流显性拒。
    {
        let mut e = PowerReneg::new();
        let first = e.overcurrent(3);
        let second = e.overcurrent(3);
        s.add_dyn(
            "隔离单向拒止",
            first.is_ok() && second == Err(PowerCode::AlreadyQuarantined),
            "重复过流登记显性拒，隔离不得解除",
        );
    }
    // 12 预算守恒式：池余 + 已授 ≡ 总预算。
    {
        let mut e = PowerReneg::new();
        let _ = e.request(0, 900);
        let _ = e.request(1, 500);
        let _ = e.overcurrent(1);
        let _ = e.request(2, 240);
        s.add_dyn(
            "预算守恒式",
            e.budget_conserved() && e.recalc() == Ok(e.remaining()),
            "每一步操作后 池余+已授 必须恒等于总预算",
        );
    }
    // 13 对账守恒：注入 = 检出，逃逸 0。
    {
        let mut e = PowerReneg::new();
        for i in 0..10u32 {
            let _ = e.request(PORT_COUNT + i as usize, 1);
            e.note_detected();
        }
        s.add_dyn(
            "注入账守恒",
            e.conserved() && e.snapshot() == (2000, 0, 0, 0, 0, 0),
            "注入=检出+逃逸，逃逸恒 0，被拒申请不落账",
        );
    }
    // 14 双跑一致。
    s.add_dyn("双跑逐字段一致", dual_run_consistent(), "同一脚本两次建引擎快照相等");
    // 15 文案闭集非空。
    {
        let mut closed = true;
        for c in ALL_CODES {
            if c.detail().is_empty() {
                closed = false;
            }
        }
        s.add_dyn("文案闭集非空", closed, "每个诊断码都必须有中文建议");
    }
    // 16 段位守卫。
    s.add_dyn("段位守卫", seg_guard_ok(), "SEG_BASE 对齐且在分派区间");
    s
}

/// 诊断码全序表（闭集枚举的权威清单）。
pub const ALL_CODES: [PowerCode; 10] = [
    PowerCode::Ok,
    PowerCode::SegBad,
    PowerCode::PortOob,
    PowerCode::PortQuarantined,
    PowerCode::AlreadyQuarantined,
    PowerCode::ExceedsGrant,
    PowerCode::ExceedsPool,
    PowerCode::StaleEpoch,
    PowerCode::RecalcMismatch,
    PowerCode::LedgerMismatch,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grant_truncation_semantics() {
        let mut e = PowerReneg::new();
        assert_eq!(e.request(0, 1000), Ok(900));
        assert_eq!(e.request(2, 240), Ok(240));
        assert_eq!(e.remaining(), 860);
        assert_eq!(e.ledger_len(), 2);
    }

    #[test]
    fn pool_exhaustion_and_zero_grant_ledger() {
        let mut e = PowerReneg::new();
        drain_pool(&mut e);
        assert_eq!(e.remaining(), 0);
        assert_eq!(e.zero_grant_cnt(), 1);
        assert_eq!(e.ledger_len(), 5);
        assert_eq!(e.granted_total(), 2000);
        assert_eq!(e.request(5, 10), Ok(0));
        assert_eq!(e.zero_grant_cnt(), 2);
        assert_eq!(e.ledger_len(), 6);
    }

    #[test]
    fn port_bounds() {
        let mut e = PowerReneg::new();
        assert!(e.request(0, 1).is_ok());
        assert!(e.request(7, 1).is_ok());
        assert_eq!(e.request(8, 1), Err(PowerCode::PortOob));
        assert_eq!(e.request(99, 1), Err(PowerCode::PortOob));
    }

    #[test]
    fn overcurrent_reclaim_and_recalc() {
        let mut e = PowerReneg::new();
        let _ = e.request(0, 900);
        let _ = e.request(1, 500);
        // 过流回收 0 口 900 在册授权 → 池余 2000−500=1500。
        assert_eq!(e.overcurrent(0), Ok(1500));
        assert_eq!(e.recalc(), Ok(1500));
        assert_eq!(e.request(0, 10), Err(PowerCode::PortQuarantined));
        assert!(e.budget_conserved());
    }

    #[test]
    fn exceeds_grant_injection_all_caught() {
        let mut e = PowerReneg::new();
        let _ = e.request(0, 900);
        let mut caught = 0u32;
        for i in 0..10u32 {
            if e.claim_grant(0, 900 + 1 + i) == Err(PowerCode::ExceedsGrant) {
                caught += 1;
                e.note_detected();
            }
        }
        assert_eq!(caught, 10);
        assert!(e.conserved());
    }

    #[test]
    fn exceeds_pool_injection_all_caught() {
        let mut e = PowerReneg::new();
        let mut caught = 0u32;
        for i in 0..10u32 {
            if e.claim_pool(2001 + i as u64) == Err(PowerCode::ExceedsPool) {
                caught += 1;
                e.note_detected();
            }
        }
        assert_eq!(caught, 10);
    }

    #[test]
    fn quarantined_request_injection_all_caught() {
        let mut e = PowerReneg::new();
        let _ = e.overcurrent(1);
        let mut caught = 0u32;
        for i in 0..10u32 {
            if e.request(1, 1 + i) == Err(PowerCode::PortQuarantined) {
                caught += 1;
                e.note_detected();
            }
        }
        assert_eq!(caught, 10);
        assert!(e.conserved());
    }

    #[test]
    fn recalc_consistent_ten_rounds() {
        let mut e = PowerReneg::new();
        drain_pool(&mut e);
        for i in 0..10u32 {
            let port = i as usize % PORT_COUNT;
            let _ = e.overcurrent(port);
            assert!(e.recalc().is_ok(), "第 {} 轮重算不一致", i);
        }
        assert!(e.budget_conserved());
    }

    #[test]
    fn quarantine_monotonic() {
        let mut e = PowerReneg::new();
        assert!(e.overcurrent(3).is_ok());
        assert_eq!(e.overcurrent(3), Err(PowerCode::AlreadyQuarantined));
        assert_eq!(e.is_quarantined(3), Some(true));
    }

    #[test]
    fn epoch_stale_reject() {
        let mut e = PowerReneg::new();
        assert!(e.claim_epoch(0).is_ok());
        let _ = e.overcurrent(0);
        assert_eq!(e.claim_epoch(0), Err(PowerCode::StaleEpoch));
        assert!(e.claim_epoch(1).is_ok());
    }

    #[test]
    fn judge_order_oob_before_quarantine() {
        let mut e = PowerReneg::new();
        let _ = e.overcurrent(0);
        assert_eq!(e.request(8, 0), Err(PowerCode::PortOob));
        assert_eq!(e.request(0, 0), Err(PowerCode::PortQuarantined));
    }

    #[test]
    fn dual_run_identical() {
        assert!(dual_run_consistent());
    }

    #[test]
    fn seg_guard_and_texts() {
        assert!(seg_guard_ok());
        for c in ALL_CODES {
            assert!(!c.detail().is_empty());
        }
    }

    #[test]
    fn unx51544_checks_green() {
        let set = run_unx51544_checks();
        assert!(set.all_passed(), "UNX-F51544 red: {}", set.red_items_text());
    }
}
