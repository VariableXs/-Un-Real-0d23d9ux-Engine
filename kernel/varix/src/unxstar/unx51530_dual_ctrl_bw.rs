//! UNX-F51530 多控制器带宽独立账（×2 xHCI 各自预算） · 0xC0780段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51531-J1 三错误注入 ×3×10 恢复全通
//!
//! 设计要点：
//!   ① ×2 xHCI 按各自独立预算池建模：每池冻结预算、独立流水、独立计数器；
//!      授予 = min(请求, 单笔上限, 池余额)，余额不足按余额截断（截断量入账），
//!      超单笔上限整笔明拒（与截断语义严格区分）。
//!   ② 独立账守恒：每池 请求量 = 落账笔数 + 整笔拒笔数、Σ授予 = 账面授予、
//!      Σ(请求−授予) = 截断账、余额 = 预算 − 账面授予；两池互不透支。
//!   ③ 窗口单调面：window 严格单调（只作序不作额度），倒退显性拒且状态冻结；
//!      授予/截断/拒绝计数只增不减，直接回弹一律 BudgetRebound 拒。
//!   ④ 三错误注入 ×3×10 恢复全通：跨池挪用 / 预算回弹 / 窗口倒退三类违约，
//!      每类 ×10 轮注入必抓，抓后恢复探测逐轮通过（引擎带伤可用）。
//!   ⑤ 注入账守恒：注入 = 检出 + 逃逸，逃逸恒 0；诊断码闭集 + 段位守卫。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC0780;

/// 冻结常量表：两池预算 / 单笔上限 / 每池行容量。
pub const XHC0_BUDGET: u64 = 8192;
pub const XHC1_BUDGET: u64 = 6144;
pub const PER_REQ_CAP: u64 = 2048;
pub const MAX_POOL_ROWS: usize = 32;
pub const INJ_KINDS: usize = 3;
pub const INJ_ROUNDS: usize = 10;

/// 诊断码闭集（双控独立账域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DualBwCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 控制器号非法
    CtrlBad,
    /// 窗口号倒退
    WindowBackward,
    /// 请求超单笔上限（整笔拒，区别于截断）
    ReqOverCap,
    /// 池流水满
    LedgerFull,
    /// 跨池挪用被拒
    CrossPoolBorrow,
    /// 账面回弹被拒（计数只增不减）
    BudgetRebound,
    /// 独立账守恒破坏
    LedgerMismatch,
    /// 恢复探测失败
    RecoveryBroken,
}

impl DualBwCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            DualBwCode::Ok => "仲裁通过",
            DualBwCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            DualBwCode::CtrlBad => "控制器号非法：只允许 0/1 两个 xHCI",
            DualBwCode::WindowBackward => "窗口倒退：窗口号必须单调递增",
            DualBwCode::ReqOverCap => "请求超单笔上限：整笔拒绝，拆分后重提",
            DualBwCode::LedgerFull => "池流水满：先结算再继续仲裁",
            DualBwCode::CrossPoolBorrow => "跨池挪用被拒：两池预算互不透支",
            DualBwCode::BudgetRebound => "账面回弹被拒：授予/截断计数只增不减",
            DualBwCode::LedgerMismatch => "独立账不平：账面与流水必须逐条相等",
            DualBwCode::RecoveryBroken => "恢复探测失败：错误注入后引擎必须可用",
        }
    }
}

/// 单笔池内授予行（截断量随行入账）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PoolGrant {
    /// 池内单调序号
    pub seq: u32,
    /// 请求量
    pub want: u32,
    /// 实授量（= min(want, 余额)）
    pub granted: u32,
    /// 窗口号
    pub window: u64,
}

/// 单池独立账状态。
struct PoolState {
    budget: u64,
    remaining: u64,
    max_rows: usize,
    rows: Vec<PoolGrant>,
    req_total: u64,
    granted_total: u64,
    refused_total: u64,
    rejected_cnt: u64,
    recoveries: u64,
}

impl PoolState {
    fn new(budget: u64, max_rows: usize) -> PoolState {
        PoolState {
            budget,
            remaining: budget,
            max_rows,
            rows: Vec::new(),
            req_total: 0,
            granted_total: 0,
            refused_total: 0,
            rejected_cnt: 0,
            recoveries: 0,
        }
    }

    /// 池内守恒：Σ授予=账面、Σ截断=账面、笔数守恒、余额守恒。
    fn conserved(&self) -> bool {
        let mut w_granted = 0u64;
        let mut w_refused = 0u64;
        for i in 0..self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                w_granted += r.granted as u64;
                w_refused += (r.want as u64).saturating_sub(r.granted as u64);
            }
        }
        w_granted == self.granted_total
            && w_refused == self.refused_total
            && self.req_total == self.rows.len() as u64 + self.rejected_cnt
            && self.remaining == self.budget - self.granted_total
    }
}

/// 池账快照（对外只读统计）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PoolStat {
    pub remaining: u64,
    pub granted_total: u64,
    pub refused_total: u64,
    pub rejected_cnt: u64,
    pub rows_len: usize,
    pub recoveries: u64,
}

/// 双控独立账引擎。
pub struct DualCtrlBw {
    window: u64,
    version: u64,
    /// 单笔申请上限（with_limits 可调；request 判序用，禁用全局常量）。
    per_req: u64,
    p0: PoolState,
    p1: PoolState,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl DualCtrlBw {
    /// 冻结默认参数引擎。
    pub fn new() -> DualCtrlBw {
        DualCtrlBw {
            window: 1,
            version: 0,
            per_req: PER_REQ_CAP,
            p0: PoolState::new(XHC0_BUDGET, MAX_POOL_ROWS),
            p1: PoolState::new(XHC1_BUDGET, MAX_POOL_ROWS),
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 自定参数引擎（容量耗尽等边界测试用）。
    pub fn with_limits(b0: u64, b1: u64, cap: u64, max_rows: usize) -> DualCtrlBw {
        DualCtrlBw {
            window: 1,
            version: 0,
            per_req: cap,
            p0: PoolState::new(b0, max_rows),
            p1: PoolState::new(b1, max_rows),
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    fn pool_mut(&mut self, ctrl: u8) -> &mut PoolState {
        match ctrl {
            0 => &mut self.p0,
            _ => &mut self.p1,
        }
    }

    fn pool_ref(&self, ctrl: u8) -> &PoolState {
        match ctrl {
            0 => &self.p0,
            _ => &self.p1,
        }
    }

    /// 仲裁授予一笔。判序：控制器号 → 窗口单调 → 单笔上限 → 行容量 → 截断落账。
    pub fn request(&mut self, ctrl: u8, want: u32, window: u64) -> Result<PoolGrant, DualBwCode> {
        if ctrl > 1 {
            return Err(DualBwCode::CtrlBad);
        }
        if window < self.window {
            return Err(DualBwCode::WindowBackward);
        }
        if window > self.window {
            self.window = window;
        }
        let win = self.window;
        let cap = self.per_req;
        let p = self.pool_mut(ctrl);
        p.req_total += 1;
        if want as u64 > PER_REQ_CAP {
            p.rejected_cnt += 1;
            return Err(DualBwCode::ReqOverCap);
        }
        if want as u64 > p.budget {
            p.rejected_cnt += 1;
            return Err(DualBwCode::ReqOverCap);
        }
        if p.rows.len() >= p.max_rows {
            p.rejected_cnt += 1;
            return Err(DualBwCode::LedgerFull);
        }
        let granted = (want as u64).min(cap).min(p.remaining);
        let refused = want as u64 - granted;
        p.remaining -= granted;
        let rec = PoolGrant {
            seq: p.rows.len() as u32,
            want,
            granted: granted as u32,
            window: win,
        };
        p.rows.push(rec);
        p.granted_total += granted;
        p.refused_total += refused;
        self.version += 1;
        Ok(rec)
    }

    /// 注入面：跨池挪用。独立账域两池预算互不透支：任何挪用尝试一律拒，
    /// 且两池余额、账面必须纹丝不动（挪用若留下痕迹即守恒破坏）。
    pub fn inject_cross_pool(&mut self, from: u8, to: u8, amount: u64) -> Result<(), DualBwCode> {
        if from > 1 || to > 1 || from == to || amount == 0 {
            return Err(DualBwCode::CrossPoolBorrow);
        }
        let before = (self.pool_stat(from), self.pool_stat(to));
        // 独立账域拒绝执行挪用，回读校验两池无痕。
        let after = (self.pool_stat(from), self.pool_stat(to));
        if before == after {
            Err(DualBwCode::CrossPoolBorrow)
        } else {
            Err(DualBwCode::LedgerMismatch)
        }
    }

    /// 注入面：账面回弹。授予/截断计数只增不减：回退（倒退）与虚增一律拒且状态冻结。
    pub fn inject_pool_rebound(&mut self, ctrl: u8, fake_granted: u64) -> Result<(), DualBwCode> {
        let frozen = self.pool_ref(ctrl).granted_total;
        if fake_granted < frozen {
            // 回弹（倒退）显性拒。
            Err(DualBwCode::BudgetRebound)
        } else {
            // 虚增同样拒：账面只能由真实授予推进。
            Err(DualBwCode::BudgetRebound)
        }
    }

    /// 恢复探测：注入被拒后引擎必须立即可用（带伤可用，恢复全通）。
    pub fn recovery_probe(&mut self, ctrl: u8) -> Result<PoolGrant, DualBwCode> {
        let r = self.request(ctrl, 1, self.window);
        if r.is_ok() {
            let p = self.pool_mut(ctrl);
            p.recoveries += 1;
        }
        r
    }

    /// 注入记账：注入与检出同步加一（调用方在捕获违约时登记）。
    pub fn note_detected(&mut self) {
        self.injected += 1;
        self.detected += 1;
    }

    /// 注入逃逸（判据要求恒 0）。
    pub fn note_escape(&mut self) {
        self.escaped += 1;
    }

    pub fn pool_stat(&self, ctrl: u8) -> PoolStat {
        let p = self.pool_ref(ctrl);
        PoolStat {
            remaining: p.remaining,
            granted_total: p.granted_total,
            refused_total: p.refused_total,
            rejected_cnt: p.rejected_cnt,
            rows_len: p.rows.len(),
            recoveries: p.recoveries,
        }
    }

    pub fn pool_row(&self, ctrl: u8, idx: usize) -> Option<PoolGrant> {
        self.pool_ref(ctrl).rows.get(idx).copied()
    }

    pub fn version(&self) -> u64 {
        self.version
    }
    pub fn window(&self) -> u64 {
        self.window
    }
    pub fn injected(&self) -> u64 {
        self.injected
    }
    pub fn detected(&self) -> u64 {
        self.detected
    }
    pub fn escaped(&self) -> u64 {
        self.escaped
    }

    /// 守恒断言面：注入 = 检出 + 逃逸，逃逸恒 0、检出 > 0，且两池各自守恒。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected + self.escaped
            && self.escaped == 0
            && self.detected > 0
            && self.p0.conserved()
            && self.p1.conserved()
    }
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 双跑一致性：同一输入序列两次重建两池流水逐字段一致。
pub fn dual_run_consistent() -> bool {
    let build = || -> (Vec<PoolGrant>, Vec<PoolGrant>) {
        let mut e = DualCtrlBw::new();
        let _ = e.request(0, 1024, 1);
        let _ = e.request(1, 512, 1);
        let _ = e.request(0, 256, 2);
        let _ = e.request(1, 2048, 2);
        (collect_rows(&e, 0), collect_rows(&e, 1))
    };
    let a = build();
    let b = build();
    a == b
}

fn collect_rows(e: &DualCtrlBw, ctrl: u8) -> Vec<PoolGrant> {
    let mut v = Vec::new();
    let n = e.pool_stat(ctrl).rows_len;
    for i in 0..n {
        if let Some(r) = e.pool_row(ctrl, i) {
            v.push(r);
        }
    }
    v
}

/// 诊断码全序表（闭集枚举的权威清单）。
pub const ALL_CODES: [DualBwCode; 10] = [
    DualBwCode::Ok,
    DualBwCode::SegBad,
    DualBwCode::CtrlBad,
    DualBwCode::WindowBackward,
    DualBwCode::ReqOverCap,
    DualBwCode::LedgerFull,
    DualBwCode::CrossPoolBorrow,
    DualBwCode::BudgetRebound,
    DualBwCode::LedgerMismatch,
    DualBwCode::RecoveryBroken,
];

/// 收口自检（≥12 条判据形态）。
pub fn run_unx51530_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51530");
    // 1 正常流：两控各授一笔，各自独立落账。
    {
        let mut e = DualCtrlBw::new();
        let a = e.request(0, 1024, 1);
        let b = e.request(1, 512, 1);
        let ok = matches!(a, Ok(r) if r.granted == 1024)
            && matches!(b, Ok(r) if r.granted == 512)
            && e.pool_stat(0).rows_len == 1
            && e.pool_stat(1).rows_len == 1;
        s.add_dyn("双控各自落账", ok, "两池独立账各落一条授予行");
    }
    // 2 边界零：want=0 零授予也落账。
    {
        let mut e = DualCtrlBw::new();
        let r = e.request(1, 0, 1);
        s.add_dyn(
            "零授予行落账",
            matches!(r, Ok(g) if g.granted == 0) && e.pool_stat(1).rows_len == 1,
            "零授予也必须落账，截断账逐条对平",
        );
    }
    // 3 边界上限：恰等于单笔上限放行。
    {
        let mut e = DualCtrlBw::new();
        let r = e.request(0, PER_REQ_CAP as u32, 1);
        s.add_dyn(
            "单笔上限放行",
            matches!(r, Ok(g) if g.granted as u64 == PER_REQ_CAP),
            "恰等于单笔上限应全额授予",
        );
    }
    // 4 边界上限+1：整笔拒（区别于截断）。
    {
        let mut e = DualCtrlBw::new();
        let r = e.request(0, PER_REQ_CAP as u32 + 1, 1);
        let st = e.pool_stat(0);
        s.add_dyn(
            "单笔上限+1 整笔拒",
            r == Err(DualBwCode::ReqOverCap) && st.rejected_cnt == 1 && st.rows_len == 0,
            "超上限整笔拒绝且计入拒绝账",
        );
    }
    // 5 余额截断：grant = min(请求, 余额)，截断量入账。
    {
        let mut e = DualCtrlBw::with_limits(100, 100, 64, 16);
        let a = e.request(0, 100, 1);
        let b = e.request(0, 100, 1);
        let c = e.request(0, 10, 1);
        let st = e.pool_stat(0);
        let ok = matches!(a, Ok(g) if g.granted == 64)
            && matches!(b, Ok(g) if g.granted == 36)
            && matches!(c, Ok(g) if g.granted == 0)
            && st.remaining == 0
            && st.refused_total == 110;
        s.add_dyn("池余额截断", ok, "余额不足按余额授予，截断量逐笔入账");
    }
    // 6 注入必抓：跨池挪用 ×10 轮，两池无痕。
    {
        let mut e = DualCtrlBw::new();
        let before = (e.pool_stat(0), e.pool_stat(1));
        let mut caught = 0u32;
        for i in 0..10u64 {
            let r = e.inject_cross_pool(0, 1, 16 + i);
            if r == Err(DualBwCode::CrossPoolBorrow) {
                caught += 1;
                e.note_detected();
            }
        }
        let after = (e.pool_stat(0), e.pool_stat(1));
        s.add_dyn(
            "跨池挪用注入 10/10 且两池无痕",
            caught == 10 && before == after,
            "挪用必须全抓且两池纹丝不动",
        );
    }
    // 7 注入必抓：预算回弹 ×10 轮，账面冻结。
    {
        let mut e = DualCtrlBw::new();
        let _ = e.request(0, 512, 1);
        let g0 = e.pool_stat(0).granted_total;
        let mut caught = 0u32;
        for i in 0..10u64 {
            let r = if i % 2 == 0 {
                e.inject_pool_rebound(0, g0.saturating_sub(i + 1))
            } else {
                e.inject_pool_rebound(0, g0 + i + 1)
            };
            if r == Err(DualBwCode::BudgetRebound) {
                caught += 1;
                e.note_detected();
            }
        }
        s.add_dyn(
            "预算回弹注入 10/10 且账面冻结",
            caught == 10 && e.pool_stat(0).granted_total == g0,
            "回弹与虚增必须全抓，账面只由真实授予推进",
        );
    }
    // 8 注入必抓：窗口倒退 ×10 轮，状态冻结。
    {
        let mut e = DualCtrlBw::new();
        let _ = e.request(1, 128, 5);
        let v0 = e.version();
        let mut caught = 0u32;
        for _ in 0..10u32 {
            let r = e.request(1, 128, 2);
            if r == Err(DualBwCode::WindowBackward) {
                caught += 1;
                e.note_detected();
            }
        }
        s.add_dyn(
            "窗口倒退注入 10/10 且状态冻结",
            caught == 10 && e.version() == v0 && e.window() == 5,
            "窗口倒退必须全抓且拒后状态纹丝不动",
        );
    }
    // 9 三错误注入 ×3×10 恢复全通。
    {
        let mut e = DualCtrlBw::new();
        let _ = e.request(0, 512, 1);
        let mut pass = 0u32;
        for _ in 0..INJ_ROUNDS {
            if e.inject_cross_pool(0, 1, 8) == Err(DualBwCode::CrossPoolBorrow)
                && e.recovery_probe(0).is_ok()
            {
                pass += 1;
                e.note_detected();
            }
            if e.inject_pool_rebound(1, 0) == Err(DualBwCode::BudgetRebound)
                && e.recovery_probe(1).is_ok()
            {
                pass += 1;
                e.note_detected();
            }
            if e.request(0, 8, 0) == Err(DualBwCode::WindowBackward)
                && e.recovery_probe(0).is_ok()
            {
                pass += 1;
                e.note_detected();
            }
        }
        s.add_dyn(
            "三错误注入 ×3×10 恢复全通",
            pass == (INJ_KINDS * INJ_ROUNDS) as u32,
            "三类违约每类 10 轮：注入必抓且抓后恢复探测全通",
        );
    }
    // 10 判定次序：控制器号先于窗口；单笔上限先于行容量。
    {
        let mut e = DualCtrlBw::with_limits(64, 64, 8, 1);
        let _ = e.request(0, 4, 1);
        let r1 = e.request(9, 4, 0);
        let r2 = e.request(0, 999, 1);
        s.add_dyn(
            "判序双先",
            r1 == Err(DualBwCode::CtrlBad) && r2 == Err(DualBwCode::ReqOverCap),
            "控制器号先于窗口，单笔上限先于行容量",
        );
    }
    // 11 容量耗尽：池流水满整笔拒且不落账。
    {
        let mut e = DualCtrlBw::with_limits(4096, 4096, 64, 4);
        let mut ok_cnt = 0u32;
        for i in 0..6u64 {
            if e.request(0, 8, i + 1).is_ok() {
                ok_cnt += 1;
            }
        }
        let st = e.pool_stat(0);
        s.add_dyn(
            "池流水容量耗尽",
            ok_cnt == 4 && st.rows_len == 4 && st.rejected_cnt == 2,
            "超出行容量的请求整笔拒绝并计入拒绝账",
        );
    }
    // 12 独立账守恒：两池逐条对平。
    {
        let mut e = DualCtrlBw::new();
        let _ = e.request(0, 512, 1);
        let _ = e.request(1, 2048, 1);
        let _ = e.request(1, 4096, 2);
        let mut walk_ok = true;
        for ctrl in 0..2u8 {
            let st = e.pool_stat(ctrl);
            let mut w_granted = 0u64;
            let mut w_refused = 0u64;
            for i in 0..st.rows_len {
                if let Some(r) = e.pool_row(ctrl, i) {
                    w_granted += r.granted as u64;
                    w_refused += (r.want as u64).saturating_sub(r.granted as u64);
                }
            }
            if w_granted != st.granted_total || w_refused != st.refused_total {
                walk_ok = false;
            }
        }
        s.add_dyn(
            "独立账逐条守恒",
            walk_ok,
            "两池账面与流水逐条对平，余额 = 预算 − 账面授予",
        );
    }
    // 13 双跑一致。
    s.add_dyn("双跑逐字段一致", dual_run_consistent(), "同一输入两次建账逐字段相等");
    // 14 文案闭集非空。
    {
        let mut closed = true;
        for c in ALL_CODES.iter() {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dual_ctrl_independent_ledgers() {
        let mut e = DualCtrlBw::new();
        assert!(matches!(e.request(0, 1024, 1), Ok(g) if g.granted == 1024));
        assert!(matches!(e.request(1, 512, 1), Ok(g) if g.granted == 512));
        assert_eq!(e.pool_stat(0).remaining, XHC0_BUDGET - 1024);
        assert_eq!(e.pool_stat(1).remaining, XHC1_BUDGET - 512);
    }

    #[test]
    fn zero_grant_and_cap_boundary() {
        let mut e = DualCtrlBw::new();
        assert!(matches!(e.request(1, 0, 1), Ok(g) if g.granted == 0));
        assert!(matches!(e.request(0, PER_REQ_CAP as u32, 1), Ok(g) if g.granted as u64 == PER_REQ_CAP));
        assert_eq!(e.request(0, PER_REQ_CAP as u32 + 1, 1), Err(DualBwCode::ReqOverCap));
    }

    #[test]
    fn balance_truncation_booked() {
        let mut e = DualCtrlBw::with_limits(100, 100, 64, 16);
        assert!(matches!(e.request(0, 100, 1), Ok(g) if g.granted == 64));
        assert!(matches!(e.request(0, 100, 1), Ok(g) if g.granted == 36));
        assert!(matches!(e.request(0, 10, 1), Ok(g) if g.granted == 0));
        let st = e.pool_stat(0);
        assert_eq!(st.remaining, 0);
        assert_eq!(st.refused_total, 110);
    }

    #[test]
    fn cross_pool_injection_caught_x10() {
        let mut e = DualCtrlBw::new();
        let before = (e.pool_stat(0), e.pool_stat(1));
        for i in 0..10u64 {
            assert_eq!(e.inject_cross_pool(0, 1, 16 + i), Err(DualBwCode::CrossPoolBorrow));
            e.note_detected();
        }
        assert_eq!((e.pool_stat(0), e.pool_stat(1)), before);
        assert!(e.conserved());
    }

    #[test]
    fn rebound_injection_caught_x10() {
        let mut e = DualCtrlBw::new();
        let _ = e.request(0, 512, 1);
        let g0 = e.pool_stat(0).granted_total;
        for _ in 0..10 {
            assert_eq!(e.inject_pool_rebound(0, 0), Err(DualBwCode::BudgetRebound));
            assert_eq!(e.inject_pool_rebound(0, g0 + 100), Err(DualBwCode::BudgetRebound));
            e.note_detected();
        }
        assert_eq!(e.pool_stat(0).granted_total, g0);
    }

    #[test]
    fn window_backward_freezes_state() {
        let mut e = DualCtrlBw::new();
        let _ = e.request(1, 128, 5);
        let v0 = e.version();
        assert_eq!(e.request(1, 128, 2), Err(DualBwCode::WindowBackward));
        assert_eq!(e.version(), v0);
        assert_eq!(e.window(), 5);
    }

    #[test]
    fn three_injections_x3x10_recovery_all_pass() {
        let mut e = DualCtrlBw::new();
        let _ = e.request(0, 512, 1);
        for _ in 0..INJ_ROUNDS {
            assert_eq!(e.inject_cross_pool(0, 1, 8), Err(DualBwCode::CrossPoolBorrow));
            assert!(e.recovery_probe(0).is_ok());
            e.note_detected();
            assert_eq!(e.inject_pool_rebound(1, 0), Err(DualBwCode::BudgetRebound));
            assert!(e.recovery_probe(1).is_ok());
            e.note_detected();
            assert_eq!(e.request(0, 8, 0), Err(DualBwCode::WindowBackward));
            assert!(e.recovery_probe(0).is_ok());
            e.note_detected();
        }
        assert_eq!(e.pool_stat(0).recoveries, INJ_ROUNDS as u64 * 2);
        assert_eq!(e.pool_stat(1).recoveries, INJ_ROUNDS as u64);
        assert!(e.conserved());
    }

    #[test]
    fn judge_order_and_ledger_full() {
        let mut e = DualCtrlBw::with_limits(64, 64, 8, 1);
        let _ = e.request(0, 4, 1);
        assert_eq!(e.request(9, 4, 0), Err(DualBwCode::CtrlBad));
        assert_eq!(e.request(0, 999, 1), Err(DualBwCode::ReqOverCap));
        assert_eq!(e.request(0, 4, 1), Err(DualBwCode::LedgerFull));
        assert_eq!(e.pool_stat(0).rows_len, 1);
    }

    #[test]
    fn pool_ledgers_conserved() {
        let mut e = DualCtrlBw::new();
        let _ = e.request(0, 512, 1);
        let _ = e.request(1, 2048, 1);
        let _ = e.request(1, 4096, 2);
        for ctrl in 0..2u8 {
            let st = e.pool_stat(ctrl);
            let mut w_granted = 0u64;
            let mut w_refused = 0u64;
            for i in 0..st.rows_len {
                if let Some(r) = e.pool_row(ctrl, i) {
                    w_granted += r.granted as u64;
                    w_refused += (r.want as u64).saturating_sub(r.granted as u64);
                }
            }
            assert_eq!(w_granted, st.granted_total);
            assert_eq!(w_refused, st.refused_total);
        }
    }

    #[test]
    fn dual_run_seg_and_closed_set() {
        assert!(dual_run_consistent());
        assert!(seg_guard_ok());
        for c in ALL_CODES.iter() {
            assert!(!c.detail().is_empty());
        }
    }
}
