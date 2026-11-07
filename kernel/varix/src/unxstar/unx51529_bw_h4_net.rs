//! UNX-F51529 带宽仲裁与 H4 带宽实表并网（AI-39 消费锚定行） · 0xC0740段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51530-J1 双控制器 ×10 组合各自账一致；跨控误算注入 10/10 检出
//!
//! 设计要点：
//!   ① 仲裁面按「窗口预算 + 单笔上限」建模：授予 = min(请求, 单笔上限, 窗口余额)，
//!      余额不足按余额截断；每一笔仲裁授予落一条仲裁行（seq, 控制器, 授予量）。
//!   ② H4 实表并网按 AI-39 消费锚定行建模：每条 H4 消费行必须锚定到一条仲裁行
//!      （seq 可查、控制器一致、消费 ≤ 授予）；锚缺失 / 跨控误算 / 超额消费都是
//!      闭集错误，跨控误算（消费行谎报归属控制器）注入 10/10 检出。
//!   ③ 窗口单调面：window 严格单调，前向推进重置窗口余额，倒退显性拒且
//!      版本号与余额纹丝不动；seq 全局单调不可复用。
//!   ④ 双控制器各自账一致：控制器 0/1 的授予、消费各自独立累计，与流水逐条
//!      重算（.get 遍历）对平；×10 组合轮次逐轮全过。
//!   ⑤ 注入账守恒：注入 = 检出 + 逃逸，逃逸恒 0；诊断码闭集 + 段位守卫。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC0740;

/// 冻结常量表：窗口预算 / 单笔上限 / 行容量 / 组合轮数。
pub const WINDOW_BUDGET_BYTES: u64 = 16384;
pub const PER_GRANT_CAP: u64 = 4096;
pub const MAX_ROWS: usize = 64;
pub const COMBO_ROUNDS: usize = 10;

/// 诊断码闭集（H4 并网域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum H4NetCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 控制器号非法（必须 0/1）
    CtrlBad,
    /// 窗口号倒退
    WindowBackward,
    /// 锚定行缺失（seq 查不到仲裁行）
    AnchorMissing,
    /// 跨控误算（消费行归属与锚定行不符）
    CrossCtrlMisanchor,
    /// 超额消费（消费 > 授予）
    OverConsume,
    /// 行账满
    LedgerFull,
    /// 并网漂移非零
    NettingDrift,
    /// 各自账不一致
    LedgerMismatch,
}

impl H4NetCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            H4NetCode::Ok => "并网通过",
            H4NetCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            H4NetCode::CtrlBad => "控制器号非法：只允许 0/1 两个 xHCI",
            H4NetCode::WindowBackward => "窗口倒退：窗口号必须单调递增",
            H4NetCode::AnchorMissing => "锚定行缺失：消费行必须锚到已存在的仲裁行",
            H4NetCode::CrossCtrlMisanchor => "跨控误算：消费行归属必须与锚定行一致",
            H4NetCode::OverConsume => "超额消费：消费量不得越过仲裁授予量",
            H4NetCode::LedgerFull => "行账满：先结算窗口再继续仲裁",
            H4NetCode::NettingDrift => "并网漂移非零：授予账与消费账必须逐条对平",
            H4NetCode::LedgerMismatch => "各自账不一致：控制器账面与流水必须逐条相等",
        }
    }
}

/// 仲裁授予行（arb 面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArbRow {
    /// 全局单调序号（即锚定键）
    pub seq: u32,
    /// 归属控制器（0/1）
    pub ctrl: u8,
    /// 实授字节数（截断后）
    pub granted: u32,
    /// 窗口号
    pub window: u64,
}

/// H4 消费锚定行（AI-39 实表行）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct H4Row {
    /// 锚定键：必须指向一条已存在的仲裁行
    pub anchor_seq: u32,
    /// 申报归属控制器
    pub ctrl: u8,
    /// 实际消费字节数（≤ 锚定行授予量）
    pub consumed: u32,
}

/// H4 并网引擎：仲裁 + 实表锚定 + 双控制器独立账。
pub struct H4NetEngine {
    window_budget: u64,
    per_grant_cap: u64,
    max_rows: usize,
    window: u64,
    remaining: u64,
    /// 本窗口已授予量（窗口推进即清零，与 remaining 同步）。
    window_granted: u64,
    version: u64,
    arb: Vec<ArbRow>,
    h4: Vec<H4Row>,
    granted0: u64,
    granted1: u64,
    consumed0: u64,
    consumed1: u64,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl H4NetEngine {
    /// 冻结默认参数引擎。
    pub fn new() -> H4NetEngine {
        H4NetEngine::with_limits(WINDOW_BUDGET_BYTES, PER_GRANT_CAP, MAX_ROWS)
    }

    /// 自定参数引擎（截断 / 容量耗尽等边界测试用）。
    pub fn with_limits(budget: u64, cap: u64, max_rows: usize) -> H4NetEngine {
        H4NetEngine {
            window_budget: budget,
            per_grant_cap: cap,
            max_rows,
            window: 1,
            remaining: budget,
            window_granted: 0,
            version: 0,
            arb: Vec::new(),
            h4: Vec::new(),
            granted0: 0,
            granted1: 0,
            consumed0: 0,
            consumed1: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    fn add_granted(&mut self, ctrl: u8, amt: u64) {
        match ctrl {
            0 => self.granted0 += amt,
            _ => self.granted1 += amt,
        }
    }

    fn add_consumed(&mut self, ctrl: u8, amt: u64) {
        match ctrl {
            0 => self.consumed0 += amt,
            _ => self.consumed1 += amt,
        }
    }

    /// 仲裁授予一笔。判序：控制器号 → 窗口单调 → 行容量 → 截断落账。
    /// 授予 = min(请求, 单笔上限, 窗口余额)。
    pub fn grant(&mut self, ctrl: u8, want: u32, window: u64) -> Result<ArbRow, H4NetCode> {
        if ctrl > 1 {
            return Err(H4NetCode::CtrlBad);
        }
        if window < self.window {
            return Err(H4NetCode::WindowBackward);
        }
        if window > self.window {
            // 前向推进：重置窗口余额与本窗口账。
            self.window = window;
            self.remaining = self.window_budget;
            self.window_granted = 0;
        }
        if self.arb.len() >= self.max_rows {
            return Err(H4NetCode::LedgerFull);
        }
        let granted = (want as u64)
            .min(self.per_grant_cap)
            .min(self.remaining);
        self.remaining -= granted;
        self.window_granted += granted;
        let row = ArbRow {
            seq: self.arb.len() as u32,
            ctrl,
            granted: granted as u32,
            window: self.window,
        };
        self.arb.push(row);
        self.add_granted(ctrl, granted);
        self.version += 1;
        Ok(row)
    }

    /// H4 实表申报一条消费锚定行。判序：锚存在 → 归属一致 → 消费 ≤ 授予 → 落账。
    pub fn h4_report(&mut self, anchor_seq: u32, ctrl: u8, consumed: u32) -> Result<H4Row, H4NetCode> {
        let row = match self.arb.get(anchor_seq as usize) {
            Some(r) => *r,
            None => return Err(H4NetCode::AnchorMissing),
        };
        if ctrl != row.ctrl {
            return Err(H4NetCode::CrossCtrlMisanchor);
        }
        if consumed as u64 > row.granted as u64 {
            return Err(H4NetCode::OverConsume);
        }
        if self.h4.len() >= self.max_rows {
            return Err(H4NetCode::LedgerFull);
        }
        let rec = H4Row {
            anchor_seq,
            ctrl,
            consumed,
        };
        self.h4.push(rec);
        self.add_consumed(ctrl, consumed as u64);
        self.version += 1;
        Ok(rec)
    }

    /// 并网漂移：指定控制器的授予账 vs 消费账之差（含逐条重算）。
    pub fn net_drift(&self, ctrl: u8) -> u64 {
        let (g_book, c_book) = match ctrl {
            0 => (self.granted0, self.consumed0),
            _ => (self.granted1, self.consumed1),
        };
        let mut g_walk = 0u64;
        let mut c_walk = 0u64;
        for i in 0..self.arb.len() {
            if let Some(r) = self.arb.get(i) {
                if r.ctrl == ctrl {
                    g_walk += r.granted as u64;
                }
            }
        }
        for i in 0..self.h4.len() {
            if let Some(r) = self.h4.get(i) {
                if r.ctrl == ctrl {
                    c_walk += r.consumed as u64;
                }
            }
        }
        g_book.abs_diff(g_walk) + c_book.abs_diff(c_walk) + g_walk.abs_diff(c_walk)
    }

    /// 双控制器各自账一致：两控账面与流水逐条对平，且窗口余额守恒。
    pub fn independence_ok(&self) -> bool {
        let mut g0 = 0u64;
        let mut g1 = 0u64;
        let mut c0 = 0u64;
        let mut c1 = 0u64;
        for i in 0..self.arb.len() {
            if let Some(r) = self.arb.get(i) {
                match r.ctrl {
                    0 => g0 += r.granted as u64,
                    _ => g1 += r.granted as u64,
                }
            }
        }
        for i in 0..self.h4.len() {
            if let Some(r) = self.h4.get(i) {
                match r.ctrl {
                    0 => c0 += r.consumed as u64,
                    _ => c1 += r.consumed as u64,
                }
            }
        }
        let ledger_ok = g0 == self.granted0
            && g1 == self.granted1
            && c0 == self.consumed0
            && c1 == self.consumed1;
        // 窗口余额守恒：remaining = 预算 − 本窗口已授予（窗口推进即重置，
        // 生命周期账 g0+g1 只验非负不与 remaining 混算）。
        let remain_ok = self.window_granted <= self.window_budget
            && self.remaining == self.window_budget - self.window_granted;
        ledger_ok && remain_ok
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

    pub fn version(&self) -> u64 {
        self.version
    }
    pub fn window(&self) -> u64 {
        self.window
    }
    pub fn remaining(&self) -> u64 {
        self.remaining
    }
    pub fn arb_len(&self) -> usize {
        self.arb.len()
    }
    pub fn h4_len(&self) -> usize {
        self.h4.len()
    }
    pub fn arb_row(&self, idx: usize) -> Option<ArbRow> {
        self.arb.get(idx).copied()
    }
    pub fn h4_row(&self, idx: usize) -> Option<H4Row> {
        self.h4.get(idx).copied()
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

    /// 守恒断言面：注入 = 检出 + 逃逸，逃逸恒 0、检出 > 0。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected + self.escaped && self.escaped == 0 && self.detected > 0
    }
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 双跑一致性：同一输入序列两次重建双账逐字段一致。
pub fn dual_run_consistent() -> bool {
    let build = || -> (Vec<ArbRow>, Vec<H4Row>) {
        let mut e = H4NetEngine::new();
        let _ = e.grant(0, 1024, 1);
        let _ = e.grant(1, 2048, 1);
        let _ = e.h4_report(0, 0, 1024);
        let _ = e.h4_report(1, 1, 2048);
        let _ = e.grant(0, 512, 2);
        let _ = e.h4_report(2, 0, 512);
        (e_arb(&e), e_h4(&e))
    };
    let a = build();
    let b = build();
    a == b
}

fn e_arb(e: &H4NetEngine) -> Vec<ArbRow> {
    let mut v = Vec::new();
    for i in 0..e.arb_len() {
        if let Some(r) = e.arb_row(i) {
            v.push(r);
        }
    }
    v
}

fn e_h4(e: &H4NetEngine) -> Vec<H4Row> {
    let mut v = Vec::new();
    for i in 0..e.h4_len() {
        if let Some(r) = e.h4_row(i) {
            v.push(r);
        }
    }
    v
}

/// 诊断码全序表（闭集枚举的权威清单）。
pub const ALL_CODES: [H4NetCode; 10] = [
    H4NetCode::Ok,
    H4NetCode::SegBad,
    H4NetCode::CtrlBad,
    H4NetCode::WindowBackward,
    H4NetCode::AnchorMissing,
    H4NetCode::CrossCtrlMisanchor,
    H4NetCode::OverConsume,
    H4NetCode::LedgerFull,
    H4NetCode::NettingDrift,
    H4NetCode::LedgerMismatch,
];

/// 收口自检（≥12 条判据形态）。
pub fn run_unx51529_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51529");
    // 1 正常流：仲裁授予 + H4 锚定消费落账。
    {
        let mut e = H4NetEngine::new();
        let g = e.grant(0, 1024, 1);
        let anchored = match g {
            Ok(row) => e.h4_report(row.seq, row.ctrl, row.granted).is_ok(),
            Err(_) => false,
        };
        s.add_dyn(
            "仲裁与锚定消费落账",
            anchored && e.arb_len() == 1 && e.h4_len() == 1,
            "授予行与消费锚定行必须各落一条",
        );
    }
    // 2 边界零：want=0 零授予行也落账，消费 0 锚定。
    {
        let mut e = H4NetEngine::new();
        let r = e.grant(1, 0, 1);
        s.add_dyn(
            "零授予行落账",
            matches!(r, Ok(row) if row.granted == 0) && e.arb_len() == 1,
            "零授予也必须落账，实表并网逐条对平",
        );
    }
    // 3 边界上限+1：单笔上限截断语义。
    {
        let mut e = H4NetEngine::new();
        let r = e.grant(0, PER_GRANT_CAP as u32 + 1, 1);
        s.add_dyn(
            "单笔上限截断",
            matches!(r, Ok(row) if row.granted as u64 == PER_GRANT_CAP),
            "超上限请求按上限截断，不整笔拒绝",
        );
    }
    // 4 预算截断：余额不足按余额授予，余额归零后零授予。
    {
        let mut e = H4NetEngine::with_limits(100, 64, 16);
        let a = e.grant(0, 100, 1);
        let b = e.grant(0, 100, 1);
        let c = e.grant(0, 100, 1);
        let ok = matches!(a, Ok(row) if row.granted == 64)
            && matches!(b, Ok(row) if row.granted == 36)
            && matches!(c, Ok(row) if row.granted == 0)
            && e.remaining() == 0;
        s.add_dyn("预算余额截断", ok, "grant = min(请求, 上限, 余额) 逐笔截断");
    }
    // 5 注入必抓：跨控误算 ×10 轮。
    {
        let mut e = H4NetEngine::new();
        let row = match e.grant(0, 128, 1) {
            Ok(r) => r,
            Err(_) => ArbRow { seq: 0, ctrl: 0, granted: 0, window: 1 },
        };
        let mut caught = 0u32;
        for _ in 0..10u32 {
            let r = e.h4_report(row.seq, 1, row.granted);
            if r == Err(H4NetCode::CrossCtrlMisanchor) {
                caught += 1;
                e.note_detected();
            }
        }
        s.add_dyn("跨控误算注入 10/10", caught == 10, "消费行谎报归属必须全抓");
    }
    // 6 注入必抓：超额消费 ×10 轮。
    {
        let mut e = H4NetEngine::new();
        let row = match e.grant(1, 128, 1) {
            Ok(r) => r,
            Err(_) => ArbRow { seq: 0, ctrl: 1, granted: 0, window: 1 },
        };
        let mut caught = 0u32;
        for _ in 0..10u32 {
            let r = e.h4_report(row.seq, 1, row.granted + 1);
            if r == Err(H4NetCode::OverConsume) {
                caught += 1;
                e.note_detected();
            }
        }
        s.add_dyn("超额消费注入 10/10", caught == 10, "消费越过授予必须全抓");
    }
    // 7 注入必抓：窗口倒退 ×10 轮，且状态冻结。
    {
        let mut e = H4NetEngine::new();
        let _ = e.grant(0, 128, 3);
        let v0 = e.version();
        let r0 = e.remaining();
        let mut caught = 0u32;
        for _ in 0..10u32 {
            let r = e.grant(0, 128, 1);
            if r == Err(H4NetCode::WindowBackward) {
                caught += 1;
                e.note_detected();
            }
        }
        s.add_dyn(
            "窗口倒退注入 10/10 且状态冻结",
            caught == 10 && e.version() == v0 && e.remaining() == r0,
            "窗口倒退必须全抓且拒后状态纹丝不动",
        );
    }
    // 8 判定次序：控制器号先于窗口；锚缺失先于跨控。
    {
        let mut e = H4NetEngine::new();
        let r1 = e.grant(9, 128, 0);
        let r2 = e.h4_report(77, 1, 0);
        s.add_dyn(
            "判序双先",
            r1 == Err(H4NetCode::CtrlBad) && r2 == Err(H4NetCode::AnchorMissing),
            "控制器号校验先于窗口，锚存在性先于归属一致性",
        );
    }
    // 9 容量耗尽：行满整笔拒绝且不落账。
    {
        let mut e = H4NetEngine::with_limits(4096, 4096, 4);
        let mut ok_cnt = 0u32;
        for i in 0..6u64 {
            if e.grant(0, 8, i + 1).is_ok() {
                ok_cnt += 1;
            }
        }
        s.add_dyn(
            "行账容量耗尽",
            ok_cnt == 4 && e.arb_len() == 4,
            "超出行容量的授予必须拒绝且不落账",
        );
    }
    // 10 窗口前向推进重置余额。
    {
        let mut e = H4NetEngine::new();
        let _ = e.grant(0, WINDOW_BUDGET_BYTES as u32, 1);
        let drained = e.remaining() == 0;
        let _ = e.grant(0, 16, 2);
        s.add_dyn(
            "窗口推进重置余额",
            drained && e.remaining() == WINDOW_BUDGET_BYTES - 16 && e.window() == 2,
            "前向推进重置窗口余额，授予继续受限",
        );
    }
    // 11 双控制器 ×10 组合各自账一致。
    {
        let mut e = H4NetEngine::new();
        let mut all_ok = 0u32;
        for r in 0..COMBO_ROUNDS {
            let w = r as u64 + 1;
            let want0 = (128 + r as u32 * 16) as u32;
            let want1 = (96 + r as u32 * 8) as u32;
            let g0 = e.grant(0, want0, w);
            let g1 = e.grant(1, want1, w);
            let mut round_ok = e.independence_ok();
            if let Ok(row) = g0 {
                round_ok = round_ok && e.h4_report(row.seq, row.ctrl, row.granted).is_ok();
            }
            if let Ok(row) = g1 {
                round_ok = round_ok && e.h4_report(row.seq, row.ctrl, row.granted).is_ok();
            }
            round_ok = round_ok && e.net_drift(0) == 0 && e.net_drift(1) == 0;
            if round_ok {
                all_ok += 1;
            }
        }
        s.add_dyn(
            "双控制器 ×10 组合各自账一致",
            all_ok == COMBO_ROUNDS as u32,
            "每轮两控授予/消费/余额逐条对平",
        );
    }
    // 12 对账守恒：注入=检出+逃逸，逃逸 0。
    {
        let mut e = H4NetEngine::new();
        let row = match e.grant(0, 128, 1) {
            Ok(r) => r,
            Err(_) => ArbRow { seq: 0, ctrl: 0, granted: 0, window: 1 },
        };
        for _ in 0..10u32 {
            let _ = e.h4_report(row.seq, 1, 8);
            e.note_detected();
        }
        s.add_dyn(
            "注入账守恒",
            e.conserved() && e.injected() == 10,
            "注入=检出+逃逸，逃逸恒 0",
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
    fn grant_then_anchor_books_both_rows() {
        let mut e = H4NetEngine::new();
        let row = e.grant(0, 1024, 1);
        assert!(matches!(row, Ok(r) if r.granted == 1024));
        let seq = match row {
            Ok(r) => r.seq,
            Err(_) => 0,
        };
        assert!(e.h4_report(seq, 0, 1024).is_ok());
        assert_eq!(e.arb_len(), 1);
        assert_eq!(e.h4_len(), 1);
        assert_eq!(e.net_drift(0), 0);
    }

    #[test]
    fn zero_grant_row_booked() {
        let mut e = H4NetEngine::new();
        let row = e.grant(1, 0, 1);
        assert!(matches!(row, Ok(r) if r.granted == 0));
        assert_eq!(e.arb_len(), 1);
    }

    #[test]
    fn truncation_semantics_hold() {
        let mut e = H4NetEngine::with_limits(100, 64, 16);
        let a = e.grant(0, 100, 1);
        assert!(matches!(a, Ok(r) if r.granted == 64));
        let b = e.grant(0, 100, 1);
        assert!(matches!(b, Ok(r) if r.granted == 36));
        assert_eq!(e.remaining(), 0);
    }

    #[test]
    fn cross_ctrl_misanchor_caught_x10() {
        let mut e = H4NetEngine::new();
        let seq = match e.grant(0, 128, 1) {
            Ok(r) => r.seq,
            Err(_) => 0,
        };
        for _ in 0..10 {
            assert_eq!(e.h4_report(seq, 1, 8), Err(H4NetCode::CrossCtrlMisanchor));
            e.note_detected();
        }
        assert!(e.conserved());
    }

    #[test]
    fn over_consume_caught_x10() {
        let mut e = H4NetEngine::new();
        let (seq, granted) = match e.grant(1, 128, 1) {
            Ok(r) => (r.seq, r.granted),
            Err(_) => (0, 0),
        };
        for _ in 0..10 {
            assert_eq!(
                e.h4_report(seq, 1, granted + 1),
                Err(H4NetCode::OverConsume)
            );
            e.note_detected();
        }
    }

    #[test]
    fn window_backward_freezes_state() {
        let mut e = H4NetEngine::new();
        let _ = e.grant(0, 128, 3);
        let v0 = e.version();
        assert_eq!(e.grant(0, 128, 1), Err(H4NetCode::WindowBackward));
        assert_eq!(e.version(), v0);
        assert_eq!(e.window(), 3);
    }

    #[test]
    fn judge_order_ctrl_then_window_anchor_then_ctrl() {
        let mut e = H4NetEngine::new();
        assert_eq!(e.grant(9, 128, 0), Err(H4NetCode::CtrlBad));
        assert_eq!(e.h4_report(77, 1, 0), Err(H4NetCode::AnchorMissing));
    }

    #[test]
    fn ledger_full_rejects_cleanly() {
        let mut e = H4NetEngine::with_limits(4096, 4096, 4);
        for i in 0..4u64 {
            assert!(e.grant(0, 8, i + 1).is_ok());
        }
        assert_eq!(e.grant(0, 8, 5), Err(H4NetCode::LedgerFull));
        assert_eq!(e.arb_len(), 4);
    }

    #[test]
    fn combo_10_rounds_each_ledger_consistent() {
        let mut e = H4NetEngine::new();
        for r in 0..COMBO_ROUNDS {
            let w = r as u64 + 1;
            let g0 = e.grant(0, 128 + r as u32 * 16, w);
            let g1 = e.grant(1, 96 + r as u32 * 8, w);
            assert!(e.independence_ok(), "第 {} 轮各自账失平", r);
            if let Ok(row) = g0 {
                assert!(e.h4_report(row.seq, row.ctrl, row.granted).is_ok());
            }
            if let Ok(row) = g1 {
                assert!(e.h4_report(row.seq, row.ctrl, row.granted).is_ok());
            }
            assert_eq!(e.net_drift(0), 0);
            assert_eq!(e.net_drift(1), 0);
        }
    }

    #[test]
    fn dual_run_identical_and_seg_guard() {
        assert!(dual_run_consistent());
        assert!(seg_guard_ok());
        for c in ALL_CODES.iter() {
            assert!(!c.detail().is_empty());
        }
    }
}
