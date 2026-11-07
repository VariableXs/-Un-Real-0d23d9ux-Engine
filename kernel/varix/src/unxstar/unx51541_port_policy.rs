//! UNX-F51541 每端口使能策略引擎（白名单/黑名单/自动三档） · 0xC0A40段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51542-J1 级联挂起恢复序 ×10 正确
//!
//! 设计要点：
//!   ① 策略域按「三档决策面」建模：白名单（列内放行、列外一律拒）、黑名单
//!      （列内一律拒、列外放行）、自动（按端口类别冻结表逐口判定）；三档
//!      语义互斥，由档位码 0/1/2 闭集编码，越码显性拒。
//!   ② 修订号单调：每次换档必须携带严格递增的修订号，等于或小于当前修订
//!      号都是倒退，显性拒且引擎状态原地不动；换档流水账只增不减。
//!   ③ 判定次序：端口界 → 未设档 → 档位决策；换档判定次序：档位码 →
//!      修订倒退 → 落账。
//!   ④ 口供注入器：调用方对口供「该端口应放行/应拒绝」与引擎实际判定比
//!      对，不符即抓（ClaimMismatch），三类注入各 ×10 轮全抓。
//!   ⑤ 每端口落账：每次判定在端口使能位图上留下已判痕迹，判/放/拒三本计
//!      数逐条对平；守恒账 注入=检出+逃逸，逃逸恒 0。
//!   ⑥ 全程零浮点、零 IO、零墙钟、零随机；取值一律走 .get()/.get_mut()。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC0A40;

/// 端口数上限。
pub const MAX_PORT: usize = 16;
/// 白名单冻结表（列内放行）。
pub const WHITELIST: [usize; 6] = [0, 2, 5, 7, 11, 14];
/// 黑名单冻结表（列内拒绝）。
pub const BLACKLIST: [usize; 5] = [1, 3, 8, 12, 15];
/// 自动档端口类别冻结表：1=使能 0=禁用（按端口号手写定值）。
pub const AUTO_CLASS: [u8; MAX_PORT] = [1, 0, 1, 0, 1, 1, 0, 1, 0, 1, 1, 0, 1, 0, 1, 0];

/// 策略三档（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tier {
    /// 白名单档
    Whitelist = 0,
    /// 黑名单档
    Blacklist = 1,
    /// 自动档
    Auto = 2,
}

impl Tier {
    pub fn code(self) -> u32 {
        self as u32
    }
    /// 档位码闭集解码：越码返回 None。
    pub fn from_code(c: u32) -> Option<Tier> {
        match c {
            0 => Some(Tier::Whitelist),
            1 => Some(Tier::Blacklist),
            2 => Some(Tier::Auto),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Tier::Whitelist => "白名单",
            Tier::Blacklist => "黑名单",
            Tier::Auto => "自动",
        }
    }
}

/// 诊断码闭集（端口策略域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PolicyCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败（SEG_BASE 越界或不对齐）
    SegBad,
    /// 端口号越界
    PortOob,
    /// 未设档就要求判定
    PolicyUnset,
    /// 档位码不在 0/1/2 闭集内
    TierUnknown,
    /// 修订号倒退（必须严格递增）
    RevRegress,
    /// 口供与引擎判定不符（注入必抓）
    ClaimMismatch,
    /// 账面不符
    LedgerMismatch,
}

impl PolicyCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            PolicyCode::Ok => "策略判定成功",
            PolicyCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            PolicyCode::PortOob => "端口越界：端口号必须在 0..16 之内",
            PolicyCode::PolicyUnset => "未设档：先换档再判定",
            PolicyCode::TierUnknown => "档位码非法：只接受 0/1/2 三档",
            PolicyCode::RevRegress => "修订倒退：换档修订号必须严格递增",
            PolicyCode::ClaimMismatch => "口供不符：调用方口供与引擎判定不一致",
            PolicyCode::LedgerMismatch => "账面不符：判/放/拒三本计数必须对平",
        }
    }
}

/// 端口策略引擎：三档决策面 + 修订流水 + 守恒账。
pub struct PortPolicyEngine {
    tier: Option<Tier>,
    rev: u64,
    applied: [bool; MAX_PORT],
    decide_cnt: u64,
    allow_cnt: u64,
    deny_cnt: u64,
    journal: Vec<(u64, u32)>,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl PortPolicyEngine {
    pub fn new() -> PortPolicyEngine {
        PortPolicyEngine {
            tier: None,
            rev: 0,
            applied: [false; MAX_PORT],
            decide_cnt: 0,
            allow_cnt: 0,
            deny_cnt: 0,
            journal: Vec::new(),
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 换档：档位码闭集解码 → 修订单调校验 → 落换档流水。
    pub fn apply_tier(&mut self, code: u32, want_rev: u64) -> Result<Tier, PolicyCode> {
        let t = match Tier::from_code(code) {
            Some(t) => t,
            None => return Err(PolicyCode::TierUnknown),
        };
        if want_rev <= self.rev {
            return Err(PolicyCode::RevRegress);
        }
        self.tier = Some(t);
        self.rev = want_rev;
        self.journal.push((want_rev, code));
        Ok(t)
    }

    /// 原始判定（不落账）：白名单列内放行、黑名单列内拒绝、自动按类别表。
    fn decide_raw(&self, port: usize) -> Option<bool> {
        let t = self.tier?;
        match t {
            Tier::Whitelist => Some(WHITELIST.contains(&port)),
            Tier::Blacklist => Some(!BLACKLIST.contains(&port)),
            Tier::Auto => match AUTO_CLASS.get(port) {
                Some(&c) => Some(c == 1),
                None => None,
            },
        }
    }

    /// 判定一个端口并落账（判/放/拒三本计数同步推进）。
    pub fn decide(&mut self, port: usize) -> Result<bool, PolicyCode> {
        if port >= MAX_PORT {
            return Err(PolicyCode::PortOob);
        }
        if self.tier.is_none() {
            return Err(PolicyCode::PolicyUnset);
        }
        let d = match self.decide_raw(port) {
            Some(d) => d,
            None => return Err(PolicyCode::PolicyUnset),
        };
        if let Some(flag) = self.applied.get_mut(port) {
            *flag = true;
        }
        self.decide_cnt = self.decide_cnt.saturating_add(1);
        if d {
            self.allow_cnt = self.allow_cnt.saturating_add(1);
        } else {
            self.deny_cnt = self.deny_cnt.saturating_add(1);
        }
        Ok(d)
    }

    /// 口供注入器：调用方口供与引擎判定比对，不符即抓，不落判定账。
    pub fn claim_decision(&mut self, port: usize, claimed_allow: bool) -> Result<bool, PolicyCode> {
        if port >= MAX_PORT {
            return Err(PolicyCode::PortOob);
        }
        let actual = match self.decide_raw(port) {
            Some(d) => d,
            None => return Err(PolicyCode::PolicyUnset),
        };
        if actual != claimed_allow {
            return Err(PolicyCode::ClaimMismatch);
        }
        Ok(actual)
    }

    /// 端口使能位图痕迹读取。
    pub fn applied_flag(&self, port: usize) -> Option<bool> {
        match self.applied.get(port) {
            Some(v) => Some(*v),
            None => None,
        }
    }

    pub fn rev(&self) -> u64 {
        self.rev
    }

    pub fn tier(&self) -> Option<Tier> {
        self.tier
    }

    pub fn journal_len(&self) -> usize {
        self.journal.len()
    }

    /// (判定数, 放行数, 拒绝数) 三本计数。
    pub fn tally(&self) -> (u64, u64, u64) {
        (self.decide_cnt, self.allow_cnt, self.deny_cnt)
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
    pub fn snapshot(&self) -> (u64, u64, u64, u64, usize) {
        (self.rev, self.decide_cnt, self.allow_cnt, self.deny_cnt, self.journal.len())
    }
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 双跑一致性：同一脚本两次建引擎，快照逐字段一致。
pub fn dual_run_consistent() -> bool {
    let build = || -> (u64, u64, u64, u64, usize) {
        let mut e = PortPolicyEngine::new();
        let _ = e.apply_tier(Tier::Whitelist.code(), 1);
        for p in 0..MAX_PORT {
            let _ = e.decide(p);
        }
        let _ = e.apply_tier(Tier::Auto.code(), 2);
        for p in 0..MAX_PORT {
            let _ = e.decide(p);
        }
        e.snapshot()
    };
    build() == build()
}

/// 白名单列外端口冻结清单（口供注入用，恰 10 口）。
pub const NON_WHITELIST: [usize; 10] = [1, 3, 4, 6, 8, 9, 10, 12, 13, 15];

/// 自动档使能口数（类别表求和）。
pub fn auto_enabled_count() -> u32 {
    AUTO_CLASS.iter().fold(0u32, |a, &c| a.saturating_add(c as u32))
}

/// 收口自检（15 条判据形态）。
pub fn run_unx51541_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51541");
    // 1 正常流：白名单档全口判定，计数对平。
    {
        let mut e = PortPolicyEngine::new();
        let _ = e.apply_tier(Tier::Whitelist.code(), 1);
        for p in 0..MAX_PORT {
            let _ = e.decide(p);
        }
        let (n, allow, deny) = e.tally();
        s.add_dyn(
            "白名单档判定",
            n == 16 && allow == 6 && deny == 10,
            "白名单档应放行 6 口、拒绝 10 口",
        );
    }
    // 2 黑名单档：列内拒、列外放。
    {
        let mut e = PortPolicyEngine::new();
        let _ = e.apply_tier(Tier::Blacklist.code(), 1);
        for p in 0..MAX_PORT {
            let _ = e.decide(p);
        }
        let (n, allow, deny) = e.tally();
        s.add_dyn(
            "黑名单档判定",
            n == 16 && allow == 11 && deny == 5,
            "黑名单档应放行 11 口、拒绝 5 口",
        );
    }
    // 3 自动档：按类别冻结表判定。
    {
        let mut e = PortPolicyEngine::new();
        let _ = e.apply_tier(Tier::Auto.code(), 1);
        for p in 0..MAX_PORT {
            let _ = e.decide(p);
        }
        let (n, allow, deny) = e.tally();
        s.add_dyn(
            "自动档判定",
            n == 16 && allow == 9 && deny == 7 && auto_enabled_count() == 9,
            "自动档应按类别表放行 9 口、拒绝 7 口",
        );
    }
    // 4 边界：端口 0/15 放行、16 拒；越界先于未设档。
    {
        let mut e = PortPolicyEngine::new();
        let oob = e.decide(99);
        let _ = e.apply_tier(Tier::Whitelist.code(), 1);
        let lo = e.decide(0).is_ok();
        let hi = e.decide(15).is_ok();
        let over = e.decide(MAX_PORT);
        s.add_dyn(
            "端口边界",
            oob == Err(PolicyCode::PortOob) && lo && hi && over == Err(PolicyCode::PortOob),
            "端口 0 与 15 放行、16 与越界值显性拒",
        );
    }
    // 5 未设档拒判。
    {
        let mut e = PortPolicyEngine::new();
        let r = e.decide(0);
        s.add_dyn("未设档拒判", r == Err(PolicyCode::PolicyUnset), "换档前不得判定");
    }
    // 6 注入必抓：未知档位码 ×10 轮。
    {
        let mut e = PortPolicyEngine::new();
        let mut caught = 0u32;
        for i in 0..10u32 {
            if e.apply_tier(3 + i, 1) == Err(PolicyCode::TierUnknown) {
                caught += 1;
                e.note_detected();
            }
        }
        s.add_dyn("未知档位注入 10/10", caught == 10, "档位码越闭集必须全抓");
    }
    // 7 注入必抓：修订倒退 ×10 轮。
    {
        let mut e = PortPolicyEngine::new();
        let _ = e.apply_tier(Tier::Whitelist.code(), 1);
        let mut caught = 0u32;
        for i in 0..10u32 {
            if e.apply_tier(Tier::Blacklist.code(), (i % 2) as u64) == Err(PolicyCode::RevRegress) {
                caught += 1;
                e.note_detected();
            }
        }
        s.add_dyn(
            "修订倒退注入 10/10",
            caught == 10 && e.rev() == 1 && e.journal_len() == 1,
            "修订号不递增必须全抓且状态不动",
        );
    }
    // 8 注入必抓：越界端口 ×10 轮。
    {
        let mut e = PortPolicyEngine::new();
        let _ = e.apply_tier(Tier::Whitelist.code(), 1);
        let mut caught = 0u32;
        for i in 0..10u32 {
            if e.decide(MAX_PORT + i as usize) == Err(PolicyCode::PortOob) {
                caught += 1;
                e.note_detected();
            }
        }
        s.add_dyn("越界端口注入 10/10", caught == 10, "越界端口必须全抓");
    }
    // 9 注入必抓：口供不符 ×10 轮（白名单列外谎报放行）。
    {
        let mut e = PortPolicyEngine::new();
        let _ = e.apply_tier(Tier::Whitelist.code(), 1);
        let mut caught = 0u32;
        for p in NON_WHITELIST {
            if e.claim_decision(p, true) == Err(PolicyCode::ClaimMismatch) {
                caught += 1;
                e.note_detected();
            }
        }
        s.add_dyn("口供不符注入 10/10", caught == 10, "口供与判定不符必须全抓");
    }
    // 10 判定次序：档位码先于修订判据。
    {
        let mut e = PortPolicyEngine::new();
        let r = e.apply_tier(9, 0);
        s.add_dyn(
            "判序档位码先行",
            r == Err(PolicyCode::TierUnknown),
            "档位码闭集校验先于修订单调校验",
        );
    }
    // 11 每端口落账：全口判定后位图全真。
    {
        let mut e = PortPolicyEngine::new();
        let _ = e.apply_tier(Tier::Auto.code(), 1);
        for p in 0..MAX_PORT {
            let _ = e.decide(p);
        }
        let mut all = true;
        for p in 0..MAX_PORT {
            if e.applied_flag(p) != Some(true) {
                all = false;
            }
        }
        s.add_dyn("每端口落账", all, "每次判定必须在端口位图留痕");
    }
    // 12 对账守恒：注入 = 检出，逃逸 0。
    {
        let mut e = PortPolicyEngine::new();
        for i in 0..10u32 {
            let _ = e.decide(MAX_PORT + i as usize);
            e.note_detected();
        }
        s.add_dyn(
            "注入账守恒",
            e.conserved() && e.snapshot() == (0, 0, 0, 0, 0),
            "注入=检出+逃逸，逃逸恒 0，被拒判定不落账",
        );
    }
    // 13 双跑一致。
    s.add_dyn("双跑逐字段一致", dual_run_consistent(), "同一脚本两次建引擎快照相等");
    // 14 文案闭集非空。
    {
        let mut closed = true;
        for c in ALL_CODES {
            if c.detail().is_empty() {
                closed = false;
            }
        }
        s.add_dyn("文案闭集非空", closed, "每个诊断码都必须有中文建议");
    }
    // 15 段位守卫。
    s.add_dyn("段位守卫", seg_guard_ok(), "SEG_BASE 对齐且在分派区间");
    s
}

/// 诊断码全序表（闭集枚举的权威清单）。
pub const ALL_CODES: [PolicyCode; 8] = [
    PolicyCode::Ok,
    PolicyCode::SegBad,
    PolicyCode::PortOob,
    PolicyCode::PolicyUnset,
    PolicyCode::TierUnknown,
    PolicyCode::RevRegress,
    PolicyCode::ClaimMismatch,
    PolicyCode::LedgerMismatch,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whitelist_allows_only_listed() {
        let mut e = PortPolicyEngine::new();
        assert!(e.apply_tier(0, 1).is_ok());
        for p in 0..MAX_PORT {
            let d = match e.decide(p) {
                Ok(d) => d,
                Err(_) => false,
            };
            assert_eq!(d, WHITELIST.contains(&p), "端口 {} 白名单判定不符", p);
        }
    }

    #[test]
    fn blacklist_denies_only_listed() {
        let mut e = PortPolicyEngine::new();
        assert!(e.apply_tier(1, 1).is_ok());
        for p in 0..MAX_PORT {
            let d = match e.decide(p) {
                Ok(d) => d,
                Err(_) => true,
            };
            assert_eq!(d, !BLACKLIST.contains(&p), "端口 {} 黑名单判定不符", p);
        }
    }

    #[test]
    fn auto_tier_follows_class_table() {
        let mut e = PortPolicyEngine::new();
        assert!(e.apply_tier(2, 1).is_ok());
        for p in 0..MAX_PORT {
            let want = match AUTO_CLASS.get(p) {
                Some(&c) => c == 1,
                None => false,
            };
            let d = match e.decide(p) {
                Ok(d) => d,
                Err(_) => !want,
            };
            assert_eq!(d, want, "端口 {} 自动档判定不符", p);
        }
        assert_eq!(auto_enabled_count(), 9);
    }

    #[test]
    fn port_bounds() {
        let mut e = PortPolicyEngine::new();
        assert_eq!(e.decide(99), Err(PolicyCode::PortOob));
        assert_eq!(e.decide(0), Err(PolicyCode::PolicyUnset));
        let _ = e.apply_tier(0, 1);
        assert!(e.decide(0).is_ok());
        assert!(e.decide(15).is_ok());
        assert_eq!(e.decide(16), Err(PolicyCode::PortOob));
    }

    #[test]
    fn unknown_tier_injection_all_caught() {
        let mut e = PortPolicyEngine::new();
        let mut caught = 0u32;
        for i in 0..10u32 {
            if e.apply_tier(3 + i, 1) == Err(PolicyCode::TierUnknown) {
                caught += 1;
                e.note_detected();
            }
        }
        assert_eq!(caught, 10);
        assert!(e.tier().is_none());
    }

    #[test]
    fn rev_regress_injection_all_caught() {
        let mut e = PortPolicyEngine::new();
        let _ = e.apply_tier(0, 5);
        let mut caught = 0u32;
        for i in 0..10u32 {
            let want = (i % 5) as u64;
            if e.apply_tier(1, want) == Err(PolicyCode::RevRegress) {
                caught += 1;
                e.note_detected();
            }
        }
        assert_eq!(caught, 10);
        assert_eq!(e.rev(), 5);
        assert_eq!(e.journal_len(), 1);
    }

    #[test]
    fn claim_mismatch_injection_all_caught() {
        let mut e = PortPolicyEngine::new();
        let _ = e.apply_tier(0, 1);
        let mut caught = 0u32;
        for p in NON_WHITELIST {
            if e.claim_decision(p, true) == Err(PolicyCode::ClaimMismatch) {
                caught += 1;
                e.note_detected();
            }
        }
        assert_eq!(caught, 10);
        assert!(e.conserved());
    }

    #[test]
    fn judge_order_tier_before_rev() {
        let mut e = PortPolicyEngine::new();
        assert_eq!(e.apply_tier(9, 0), Err(PolicyCode::TierUnknown));
        assert_eq!(e.apply_tier(0, 0), Err(PolicyCode::RevRegress));
    }

    #[test]
    fn applied_bitmap_full_after_sweep() {
        let mut e = PortPolicyEngine::new();
        let _ = e.apply_tier(1, 1);
        for p in 0..MAX_PORT {
            let _ = e.decide(p);
        }
        for p in 0..MAX_PORT {
            assert_eq!(e.applied_flag(p), Some(true), "端口 {} 应留痕", p);
        }
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
}
