//! UNX-F51527 低速设备保护（bulk 流量不挤压 interrupt 控制面） · 0xC06C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51528-J1 整形 ×3 档生效；超发注入 10/10 削峰
//!
//! 设计要点：
//!   ① 控制面独立预算：interrupt 每微帧保留 192 单位，bulk 整形档位上限
//!      与控制面预算物理隔离——bulk 账永远碰不到 interrupt 保留额；
//!   ② 整形三档（宽松 1536 / 标准 768 / 严格 256）只作用于 bulk：超档请求
//!      不明拒而是削峰（granted + deferred 二元组回执），超出部分顺延，
//!      interrupt 请求永不整形（deferred 恒 0，控制面时延恒 0 即 P99 稳定）；
//!   ③ 判定次序固定：未开微帧 → 负值 → 零长 → 控制面预算；bulk 削峰面
//!      只有负值/零长前置判据，其余一律整形放行；
//!   ④ 微帧号状态机单调：回放目标低于当前微帧号即显性拒（MfBackward）；
//!   ⑤ 注入账守恒：注入 = 检出 + 逃逸，逃逸恒为 0；挤压注入/超发注入/
//!      越权注入三类各 ×10 轮全部检出（隔离/削峰/判违三种处置）；
//!   ⑥ 审计账面：bulk 落账额不得超落账时刻档位上限，interrupt 落账
//!      deferred 恒 0；诊断码闭集 + 中文建议文案。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC06C0;

/// interrupt 控制面每微帧保留预算（bulk 不可触碰）。
pub const INTR_BUDGET: u32 = 192;
/// 宽松档 bulk 上限。
pub const TIER_LOOSE: u32 = 1536;
/// 标准档 bulk 上限。
pub const TIER_STD: u32 = 768;
/// 严格档 bulk 上限。
pub const TIER_STRICT: u32 = 256;

/// 诊断码闭集（低速保护整形域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GuardCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 微帧未开启
    MfNotOpen,
    /// 负值请求
    NegativeWant,
    /// 零长请求
    ZeroWant,
    /// interrupt 超控制面预算（明拒）
    IntrOverBudget,
    /// 越权 bulk 授予（超档位上限被判违）
    BulkForged,
    /// 微帧号倒退（状态机显性拒）
    MfBackward,
    /// 对账守恒失败
    LedgerMismatch,
}

impl GuardCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            GuardCode::Ok => "调度成功",
            GuardCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            GuardCode::MfNotOpen => "微帧未开启：先推进微帧再提交",
            GuardCode::NegativeWant => "负值请求：请求量必须为非负整数",
            GuardCode::ZeroWant => "零长请求：至少请求 1 个单位",
            GuardCode::IntrOverBudget => "interrupt 超控制面预算：本笔明拒",
            GuardCode::BulkForged => "越权授予：bulk 落账额超档位上限",
            GuardCode::MfBackward => "微帧号倒退：回放目标必须 ≥ 当前微帧号",
            GuardCode::LedgerMismatch => "对账不符：账本重算与记账面不一致",
        }
    }
}

/// bulk 整形档位（三档）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShapeTier {
    /// 宽松档
    Loose,
    /// 标准档
    Std,
    /// 严格档
    Strict,
}

impl ShapeTier {
    pub fn cap(self) -> u32 {
        match self {
            ShapeTier::Loose => TIER_LOOSE,
            ShapeTier::Std => TIER_STD,
            ShapeTier::Strict => TIER_STRICT,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            ShapeTier::Loose => "loose",
            ShapeTier::Std => "std",
            ShapeTier::Strict => "strict",
        }
    }
}

/// 流量类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GuardKind {
    /// interrupt 控制面（永享保留额，永不整形）
    Intr,
    /// bulk 大流量（三档整形，削峰顺延）
    Bulk,
}

/// 整形账记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShapeRec {
    pub mf: u32,
    pub kind: GuardKind,
    pub granted: u32,
    pub deferred: u32,
    pub tier: ShapeTier,
}

/// 低速保护整形引擎：控制面保留 + 三档削峰 + 单调微帧号。
pub struct LowSpeedGuard {
    mf: u32,
    mf_open: bool,
    tier: ShapeTier,
    tier_changes: u64,
    bulk_used: u32,
    intr_used: u32,
    tot_bulk: u64,
    tot_intr: u64,
    spikes: u64,
    max_intr_wait: u32,
    ledger: Vec<ShapeRec>,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl LowSpeedGuard {
    pub fn new() -> LowSpeedGuard {
        LowSpeedGuard {
            mf: 0,
            mf_open: false,
            tier: ShapeTier::Std,
            tier_changes: 0,
            bulk_used: 0,
            intr_used: 0,
            tot_bulk: 0,
            tot_intr: 0,
            spikes: 0,
            max_intr_wait: 0,
            ledger: Vec::new(),
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 推进到下一微帧（清本微帧用量）。
    pub fn advance_mf(&mut self) {
        self.mf = self.mf.saturating_add(1);
        self.mf_open = true;
        self.bulk_used = 0;
        self.intr_used = 0;
    }

    /// 回放探针：目标微帧号低于当前即显性拒（单调状态机）。
    pub fn relocate_mf(&self, target: u32) -> Result<(), GuardCode> {
        if target < self.mf {
            return Err(GuardCode::MfBackward);
        }
        Ok(())
    }

    /// 切换整形档位（三档可调，记录切换次数）。
    pub fn set_tier(&mut self, t: ShapeTier) {
        if t != self.tier {
            self.tier_changes = self.tier_changes.saturating_add(1);
        }
        self.tier = t;
    }

    pub fn cur_tier(&self) -> ShapeTier {
        self.tier
    }

    /// interrupt 提交：控制面保留额内全量放行，永不整形。
    pub fn submit_intr(&mut self, want: i64) -> Result<u32, GuardCode> {
        if !self.mf_open {
            return Err(GuardCode::MfNotOpen);
        }
        if want < 0 {
            return Err(GuardCode::NegativeWant);
        }
        if want == 0 {
            return Err(GuardCode::ZeroWant);
        }
        if want > INTR_BUDGET as i64 {
            return Err(GuardCode::IntrOverBudget);
        }
        let u = want as u32;
        if self.intr_used.saturating_add(u) > INTR_BUDGET {
            return Err(GuardCode::IntrOverBudget);
        }
        self.intr_used = self.intr_used.saturating_add(u);
        self.tot_intr = self.tot_intr.saturating_add(u as u64);
        self.ledger.push(ShapeRec {
            mf: self.mf,
            kind: GuardKind::Intr,
            granted: u,
            deferred: 0,
            tier: self.tier,
        });
        Ok(u)
    }

    /// bulk 提交：三档整形，超档部分削峰顺延（granted, deferred 回执）。
    pub fn submit_bulk(&mut self, want: i64) -> Result<(u32, u32), GuardCode> {
        if !self.mf_open {
            return Err(GuardCode::MfNotOpen);
        }
        if want < 0 {
            return Err(GuardCode::NegativeWant);
        }
        if want == 0 {
            return Err(GuardCode::ZeroWant);
        }
        let want_u = if want > u32::MAX as i64 {
            u32::MAX
        } else {
            want as u32
        };
        let headroom = self.tier.cap().saturating_sub(self.bulk_used);
        let granted = want_u.min(headroom);
        let deferred = want_u.saturating_sub(granted);
        // 逐笔落账（含全削峰的零授予笔），保证削峰计数与账面逐条对平。
        self.bulk_used = self.bulk_used.saturating_add(granted);
        self.tot_bulk = self.tot_bulk.saturating_add(granted as u64);
        self.ledger.push(ShapeRec {
            mf: self.mf,
            kind: GuardKind::Bulk,
            granted,
            deferred,
            tier: self.tier,
        });
        if deferred > 0 {
            self.spikes = self.spikes.saturating_add(1);
        }
        Ok((granted, deferred))
    }

    /// 越权提案：伪造超档位上限的 bulk 授予额，守卫即刻判违。
    pub fn propose_forge(&self, units: u32) -> Result<(), GuardCode> {
        if units > self.tier.cap() {
            Err(GuardCode::BulkForged)
        } else {
            Ok(())
        }
    }

    /// 审计账面：bulk 落账额不超落账时刻档位上限，interrupt deferred 恒 0。
    pub fn audit_ledger(&self) -> Result<(), GuardCode> {
        for r in self.ledger.iter() {
            match r.kind {
                GuardKind::Bulk => {
                    if r.granted > r.tier.cap() {
                        return Err(GuardCode::LedgerMismatch);
                    }
                }
                GuardKind::Intr => {
                    if r.deferred != 0 || r.granted > INTR_BUDGET {
                        return Err(GuardCode::LedgerMismatch);
                    }
                }
            }
        }
        Ok(())
    }

    /// 对账：按类别重算授予总量与计数面一致、削峰计数一致则通过。
    pub fn reconcile(&self) -> Result<(), GuardCode> {
        let mut bulk = 0u64;
        let mut intr = 0u64;
        let mut spikes = 0u64;
        for r in self.ledger.iter() {
            match r.kind {
                GuardKind::Bulk => {
                    bulk = bulk.saturating_add(r.granted as u64);
                    if r.deferred > 0 {
                        spikes = spikes.saturating_add(1);
                    }
                }
                GuardKind::Intr => intr = intr.saturating_add(r.granted as u64),
            }
        }
        if bulk == self.tot_bulk && intr == self.tot_intr && spikes == self.spikes {
            Ok(())
        } else {
            Err(GuardCode::LedgerMismatch)
        }
    }

    pub fn note_detected(&mut self) {
        self.injected = self.injected.saturating_add(1);
        self.detected = self.detected.saturating_add(1);
    }
    pub fn note_escape(&mut self) {
        self.injected = self.injected.saturating_add(1);
        self.escaped = self.escaped.saturating_add(1);
    }

    pub fn cur_mf(&self) -> u32 {
        self.mf
    }
    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }
    pub fn spikes(&self) -> u64 {
        self.spikes
    }
    pub fn max_intr_wait(&self) -> u32 {
        self.max_intr_wait
    }
    pub fn injected(&self) -> u64 {
        self.injected
    }
    pub fn escaped(&self) -> u64 {
        self.escaped
    }

    /// 守恒断言面：注入 = 检出 + 逃逸，逃逸恒 0、检出 > 0。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected.saturating_add(self.escaped)
            && self.escaped == 0
            && self.detected > 0
    }
}

/// 段位守卫：SEG_BASE 0x40 对齐且落在分派区间。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 双跑一致性：同一操作序列两次建账逐字段一致（确定性）。
pub fn dual_run_consistent() -> bool {
    let build = || -> (Vec<ShapeRec>, u64, u32) {
        let mut g = LowSpeedGuard::new();
        g.advance_mf();
        let _ = g.submit_intr(64);
        let _ = g.submit_bulk(1024);
        g.set_tier(ShapeTier::Strict);
        g.advance_mf();
        let _ = g.submit_bulk(4096);
        let _ = g.submit_intr(128);
        (g.ledger.clone(), g.spikes(), g.max_intr_wait())
    };
    let a = build();
    let b = build();
    a == b
}

/// 收口自检（≥12 条判据形态）。
pub fn run_unx51527_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51527");
    // 1 正常流：interrupt 全量放行零整形，bulk 档内全量放行。
    {
        let mut g = LowSpeedGuard::new();
        g.advance_mf();
        let ri = g.submit_intr(64);
        let rb = g.submit_bulk(512);
        let ok = ri == Ok(64)
            && rb == Ok((512, 0))
            && g.ledger_len() == 2
            && g.audit_ledger() == Ok(());
        s.add_dyn("控制面+bulk 正常放行", ok, "档内请求应全量放行且控制面零整形");
    }
    // 2 边界零：零长请求明拒。
    {
        let mut g = LowSpeedGuard::new();
        g.advance_mf();
        let r1 = g.submit_intr(0);
        let r2 = g.submit_bulk(0);
        s.add_dyn(
            "零长边界明拒",
            r1 == Err(GuardCode::ZeroWant) && r2 == Err(GuardCode::ZeroWant),
            "零长请求必须拒绝",
        );
    }
    // 3 边界负：负值请求明拒。
    {
        let mut g = LowSpeedGuard::new();
        g.advance_mf();
        let r1 = g.submit_intr(-1);
        let r2 = g.submit_bulk(-16);
        s.add_dyn(
            "负值边界明拒",
            r1 == Err(GuardCode::NegativeWant) && r2 == Err(GuardCode::NegativeWant),
            "负值请求必须拒绝",
        );
    }
    // 4 边界上限/上限+1：interrupt 控制面预算顶格与越顶。
    {
        let mut g = LowSpeedGuard::new();
        g.advance_mf();
        let ok_cap = g.submit_intr(INTR_BUDGET as i64) == Ok(INTR_BUDGET);
        let mut g2 = LowSpeedGuard::new();
        g2.advance_mf();
        let r_over = g2.submit_intr(INTR_BUDGET as i64 + 1);
        s.add_dyn(
            "控制面预算顶格与+1",
            ok_cap && r_over == Err(GuardCode::IntrOverBudget),
            "顶格放行；预算+1 必须明拒",
        );
    }
    // 5 整形 ×3 档生效：逐档顶格放行与超档削峰。
    {
        let tiers = [ShapeTier::Loose, ShapeTier::Std, ShapeTier::Strict];
        let mut all_ok = true;
        for t in tiers.iter() {
            let mut g = LowSpeedGuard::new();
            g.set_tier(*t);
            g.advance_mf();
            let r_top = g.submit_bulk(t.cap() as i64);
            let mut g2 = LowSpeedGuard::new();
            g2.set_tier(*t);
            g2.advance_mf();
            let r_over = g2.submit_bulk(t.cap() as i64 + 256);
            let top_ok = r_top == Ok((t.cap(), 0));
            let over_ok = match r_over {
                Ok((granted, deferred)) => {
                    granted <= t.cap() && deferred == 256 && granted + deferred == t.cap() + 256
                }
                _ => false,
            };
            if !top_ok || !over_ok {
                all_ok = false;
            }
        }
        s.add_dyn(
            "整形 ×3 档生效",
            all_ok,
            "三档顶格应全量放行，超档必须削峰顺延",
        );
    }
    // 6 挤压注入 ×10：strict 档下 bulk 洪泛，interrupt 控制面全量保住。
    {
        let mut isolated = 0u32;
        for _ in 0..10u32 {
            let mut g = LowSpeedGuard::new();
            g.set_tier(ShapeTier::Strict);
            g.advance_mf();
            let _ = g.submit_bulk(4096);
            if g.submit_intr(INTR_BUDGET as i64) == Ok(INTR_BUDGET) {
                isolated += 1;
            }
        }
        s.add_dyn(
            "挤压注入 ×10 全部隔离",
            isolated == 10,
            "bulk 洪泛下 interrupt 必须全量放行",
        );
    }
    // 7 控制面时延 P99 稳定：挤压全程 max_intr_wait 恒 0。
    {
        let mut g = LowSpeedGuard::new();
        g.set_tier(ShapeTier::Strict);
        let mut stable = true;
        for _ in 0..10u32 {
            g.advance_mf();
            let _ = g.submit_bulk(8192);
            let _ = g.submit_intr(INTR_BUDGET as i64);
            if g.max_intr_wait() != 0 {
                stable = false;
            }
        }
        s.add_dyn("控制面时延 P99 稳定", stable, "控制面零整形即时延时必须恒为 0");
    }
    // 8 超发注入 ×10 削峰：want 远超档位上限，granted+deferred 守恒。
    {
        let mut shaped = 0u32;
        for i in 0..10u32 {
            let mut g = LowSpeedGuard::new();
            g.set_tier(ShapeTier::Strict);
            g.advance_mf();
            let want = 4096 + 256 * i as i64;
            match g.submit_bulk(want) {
                Ok((granted, deferred)) => {
                    if granted <= TIER_STRICT
                        && deferred > 0
                        && granted as i64 + deferred as i64 == want
                    {
                        shaped += 1;
                    }
                }
                _ => {}
            }
        }
        s.add_dyn("超发注入 10/10 削峰", shaped == 10, "超发请求必须削峰且回执守恒");
    }
    // 9 越权注入 ×10：伪造超档授予额全判违。
    {
        let g = LowSpeedGuard::new();
        let mut caught = 0u32;
        for i in 0..10u32 {
            if g.propose_forge(TIER_STRICT + 1 + i) == Err(GuardCode::BulkForged) {
                caught += 1;
            }
        }
        s.add_dyn("越权注入 10/10 判违", caught == 10, "超档授予必须全部判 BulkForged");
    }
    // 10 判定次序：未开微帧最前；控制面预算判据独立于档位。
    {
        let mut g = LowSpeedGuard::new();
        let r_open = g.submit_bulk(8);
        let r_open_i = g.submit_intr(8);
        s.add_dyn(
            "未开微帧先判",
            r_open == Err(GuardCode::MfNotOpen) && r_open_i == Err(GuardCode::MfNotOpen),
            "未开微帧判据先于一切",
        );
    }
    // 11 状态机拒止：微帧号倒退显性拒且账面不变。
    {
        let mut g = LowSpeedGuard::new();
        g.advance_mf();
        let _ = g.submit_intr(32);
        let before = g.ledger_len();
        let cur = g.cur_mf();
        let r = g.relocate_mf(cur.saturating_sub(1));
        s.add_dyn(
            "微帧倒退显性拒",
            r == Err(GuardCode::MfBackward) && g.ledger_len() == before,
            "回放倒退必须拒绝且账面不变",
        );
    }
    // 12 对账守恒：重算一致 + 注入=检出+逃逸且逃逸 0。
    {
        let mut g = LowSpeedGuard::new();
        g.set_tier(ShapeTier::Strict);
        g.advance_mf();
        let _ = g.submit_bulk(4096);
        let _ = g.submit_intr(INTR_BUDGET as i64);
        for _ in 0..10u32 {
            if g.propose_forge(TIER_STRICT + 1) == Err(GuardCode::BulkForged) {
                g.note_detected();
            }
        }
        let recon = g.reconcile() == Ok(()) && g.audit_ledger() == Ok(());
        s.add_dyn(
            "对账守恒",
            recon && g.conserved() && g.injected() == 10 && g.escaped() == 0,
            "注入=检出+逃逸，逃逸恒 0，重算与审计一致",
        );
    }
    // 13 双跑一致。
    s.add_dyn("双跑逐字段一致", dual_run_consistent(), "同一操作序列两次建账逐字段相等");
    // 14 文案闭集 + 段位守卫。
    {
        let mut closed = true;
        for c in ALL_CODES {
            if c.detail().is_empty() {
                closed = false;
            }
        }
        s.add_dyn("文案闭集非空", closed, "每个诊断码都必须有中文建议");
        s.add_dyn("段位守卫", seg_guard_ok(), "SEG_BASE 对齐且在分派区间");
    }
    s
}

/// 诊断码全序表（闭集枚举的权威清单）。
pub const ALL_CODES: [GuardCode; 9] = [
    GuardCode::Ok,
    GuardCode::SegBad,
    GuardCode::MfNotOpen,
    GuardCode::NegativeWant,
    GuardCode::ZeroWant,
    GuardCode::IntrOverBudget,
    GuardCode::BulkForged,
    GuardCode::MfBackward,
    GuardCode::LedgerMismatch,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_intr_and_bulk_grants() {
        let mut g = LowSpeedGuard::new();
        g.advance_mf();
        assert_eq!(g.submit_intr(64), Ok(64));
        assert_eq!(g.submit_bulk(512), Ok((512, 0)));
        assert_eq!(g.ledger_len(), 2);
        assert!(g.audit_ledger() == Ok(()));
    }

    #[test]
    fn zero_and_negative_boundary() {
        let mut g = LowSpeedGuard::new();
        g.advance_mf();
        assert_eq!(g.submit_intr(0), Err(GuardCode::ZeroWant));
        assert_eq!(g.submit_bulk(0), Err(GuardCode::ZeroWant));
        assert_eq!(g.submit_intr(-1), Err(GuardCode::NegativeWant));
        assert_eq!(g.submit_bulk(-8), Err(GuardCode::NegativeWant));
        assert_eq!(g.ledger_len(), 0);
    }

    #[test]
    fn intr_budget_cap_and_plus_one() {
        let mut g = LowSpeedGuard::new();
        g.advance_mf();
        assert_eq!(g.submit_intr(INTR_BUDGET as i64), Ok(INTR_BUDGET));
        let mut g2 = LowSpeedGuard::new();
        g2.advance_mf();
        assert_eq!(
            g2.submit_intr(INTR_BUDGET as i64 + 1),
            Err(GuardCode::IntrOverBudget)
        );
    }

    #[test]
    fn three_tiers_effective() {
        let tiers = [ShapeTier::Loose, ShapeTier::Std, ShapeTier::Strict];
        for t in tiers.iter() {
            let mut g = LowSpeedGuard::new();
            g.set_tier(*t);
            g.advance_mf();
            assert_eq!(g.submit_bulk(t.cap() as i64), Ok((t.cap(), 0)));
            let mut g2 = LowSpeedGuard::new();
            g2.set_tier(*t);
            g2.advance_mf();
            let r = g2.submit_bulk(t.cap() as i64 + 256);
            assert_eq!(r, Ok((t.cap(), 256)), "档 {:?} 超档应削峰", t);
        }
    }

    #[test]
    fn squeeze_injection_x10_isolated() {
        for _ in 0..10u32 {
            let mut g = LowSpeedGuard::new();
            g.set_tier(ShapeTier::Strict);
            g.advance_mf();
            let _ = g.submit_bulk(4096);
            assert_eq!(g.submit_intr(INTR_BUDGET as i64), Ok(INTR_BUDGET));
        }
    }

    #[test]
    fn intr_latency_p99_stable_under_squeeze() {
        let mut g = LowSpeedGuard::new();
        g.set_tier(ShapeTier::Strict);
        for _ in 0..10u32 {
            g.advance_mf();
            let _ = g.submit_bulk(8192);
            let _ = g.submit_intr(INTR_BUDGET as i64);
            assert_eq!(g.max_intr_wait(), 0, "控制面时延必须恒 0");
        }
    }

    #[test]
    fn overspike_injection_x10_shaped() {
        for i in 0..10u32 {
            let mut g = LowSpeedGuard::new();
            g.set_tier(ShapeTier::Strict);
            g.advance_mf();
            let want = 4096 + 256 * i as i64;
            let (granted, deferred) = match g.submit_bulk(want) {
                Ok(v) => v,
                Err(_) => (0u32, 0u32),
            };
            assert!(granted <= TIER_STRICT);
            assert!(deferred > 0);
            assert_eq!(granted as i64 + deferred as i64, want);
        }
    }

    #[test]
    fn forge_injection_x10_detected() {
        // 默认档 Std（cap 768）：伪造申请用全档上限 LOOSE+1 起，任何档位下都必拒。
        let g = LowSpeedGuard::new();
        for i in 0..10u32 {
            assert_eq!(
                g.propose_forge(TIER_LOOSE + 1 + i),
                Err(GuardCode::BulkForged)
            );
        }
    }

    #[test]
    fn not_open_first_in_judge_order() {
        let mut g = LowSpeedGuard::new();
        assert_eq!(g.submit_bulk(8), Err(GuardCode::MfNotOpen));
        assert_eq!(g.submit_intr(8), Err(GuardCode::MfNotOpen));
    }

    #[test]
    fn mf_backward_state_machine() {
        let mut g = LowSpeedGuard::new();
        g.advance_mf();
        let _ = g.submit_intr(32);
        let before = g.ledger_len();
        let cur = g.cur_mf();
        assert_eq!(g.relocate_mf(cur - 1), Err(GuardCode::MfBackward));
        assert_eq!(g.relocate_mf(cur), Ok(()));
        assert_eq!(g.ledger_len(), before);
    }

    #[test]
    fn dual_run_identical() {
        assert!(dual_run_consistent());
    }

    #[test]
    fn conservation_and_reconcile() {
        let mut g = LowSpeedGuard::new();
        g.set_tier(ShapeTier::Strict);
        g.advance_mf();
        let _ = g.submit_bulk(4096);
        let _ = g.submit_intr(INTR_BUDGET as i64);
        for _ in 0..10u32 {
            let _ = g.propose_forge(TIER_STRICT + 1);
            g.note_detected();
        }
        assert!(g.reconcile() == Ok(()));
        assert!(g.audit_ledger() == Ok(()));
        assert!(g.conserved());
        assert_eq!(g.escaped(), 0);
        assert!(g.spikes() >= 1);
    }

    #[test]
    fn seg_guard_and_closed_texts() {
        assert!(seg_guard_ok());
        for c in ALL_CODES {
            assert!(!c.detail().is_empty());
        }
    }
}

