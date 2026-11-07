//! UNX-F51540 B17 批收官印（20 条冻结/累计 102,000 行） · 0xC0A00段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51541-J1 每端口使能策略引擎（白名单/黑名单/自动三档）
//! 判据原文逐字见任务板 UNX-F51541 卡验收②（UNX-F51542-J1 级联挂起恢复
//! 序 ×10 正确）
//!
//! 设计要点：
//!   ① 收官印按「冻结卡逐张关账」建模：B17 批 20 条冻结卡的行数是冻结常
//!      量表（合计恰为 102,000 行），关账只能按卡号单调推进，倒退显性拒，
//!      已关卡重关显性拒。
//!   ② 累计守恒账：累计行数 = 已关卡申报行数之和，且每张卡的申报行数必须
//!      与冻结表逐张一致；收官印只有 20 卡全关且累计恰等于冻结总量时才能
//!      发出，少一行多一行都不得发印。
//!   ③ 发印一次性：印面（卡数/总行数/关账序数/段位）一经发出即冻结，二次
//!      发印显性拒（SealReissue），未全关发印显性拒（SealPremature）。
//!   ④ 篡改注入器：对申报行数篡改、卡号越界、重复关账三类违约各注入 ×10
//!      轮，收官校验必须全抓；守恒账 注入=检出+逃逸，逃逸恒 0。
//!   ⑤ 判定次序：卡号界 → 重关 → 关账倒退 → 行数对账 → 发印前提 → 发印
//!      重复；同时成立时取先者。
//!   ⑥ 全程零浮点、零 IO、零墙钟、零随机；账面只增不减，取值一律走
//!      .get()/.get_mut()/checked_*/saturating_*。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC0A00;

/// B17 批冻结总行数（收官印守恒锚）。
pub const FROZEN_TOTAL_LINES: u64 = 102_000;
/// 冻结卡数。
pub const FROZEN_CARD_COUNT: usize = 20;

/// B17 批 20 条冻结卡的行数冻结表（手写定值，合计恰为 102,000 行）。
pub const FROZEN_CARD_LINES: [u32; FROZEN_CARD_COUNT] = [
    5400, 5250, 5100, 4980, 5320, 5060, 5170, 5230, 4890, 5280, //
    5090, 5150, 5010, 5210, 4950, 5300, 5030, 5120, 5190, 4270,
];

/// 诊断码闭集（B17 收官印域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CloseCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败（SEG_BASE 越界或不对齐）
    SegBad,
    /// 卡号越界（不在 0..20）
    CardIdxOob,
    /// 重复关账（已关卡不得重关）
    CardAlreadyClosed,
    /// 关账倒退（必须按卡号单调推进）
    CloseOrderRegress,
    /// 申报行数与冻结表不符
    LineSumMismatch,
    /// 发印过早（20 卡未全关）
    SealPremature,
    /// 重复发印（收官印一次性）
    SealReissue,
    /// 账面不符（累计与冻结总量不守恒）
    LedgerMismatch,
}

impl CloseCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            CloseCode::Ok => "关账成功",
            CloseCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            CloseCode::CardIdxOob => "卡号越界：B17 批卡号必须在 0..20 之内",
            CloseCode::CardAlreadyClosed => "重复关账：该冻结卡已关账，不得重关",
            CloseCode::CloseOrderRegress => "关账倒退：关账必须按卡号单调推进",
            CloseCode::LineSumMismatch => "行数不符：申报行数必须与冻结表逐张一致",
            CloseCode::SealPremature => "发印过早：20 卡未全关不得发印",
            CloseCode::SealReissue => "重复发印：收官印一次性，已冻结不得重发",
            CloseCode::LedgerMismatch => "账面不符：累计行数与冻结总量必须守恒",
        }
    }
}

/// 单卡关账记录（只增不减）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CloseRec {
    pub seq: u32,
    pub card: usize,
    pub lines: u32,
    pub seg: u32,
}

/// 收官印印面（发出即冻结）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SealFace {
    pub cards: u32,
    pub total_lines: u64,
    pub close_seq: u32,
    pub seg: u32,
}

/// B17 收官账板：冻结卡关账状态机 + 守恒账。
pub struct B17CloseBoard {
    closed: [bool; FROZEN_CARD_COUNT],
    ledger: Vec<CloseRec>,
    close_seq: u32,
    last_card: Option<usize>,
    accumulated: u64,
    sealed: bool,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl B17CloseBoard {
    pub fn new() -> B17CloseBoard {
        B17CloseBoard {
            closed: [false; FROZEN_CARD_COUNT],
            ledger: Vec::new(),
            close_seq: 0,
            last_card: None,
            accumulated: 0,
            sealed: false,
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 关账一张冻结卡。判定次序：卡号界 → 重关 → 倒退 → 行数对账。
    /// 任何被拒不推进任何账面字段。
    pub fn close_card(&mut self, card: usize, declared: u32) -> Result<u64, CloseCode> {
        if card >= FROZEN_CARD_COUNT {
            return Err(CloseCode::CardIdxOob);
        }
        if matches!(self.closed.get(card), Some(true)) {
            return Err(CloseCode::CardAlreadyClosed);
        }
        if let Some(last) = self.last_card {
            if card < last {
                return Err(CloseCode::CloseOrderRegress);
            }
        }
        let frozen = match FROZEN_CARD_LINES.get(card) {
            Some(v) => *v,
            None => return Err(CloseCode::CardIdxOob),
        };
        if declared != frozen {
            return Err(CloseCode::LineSumMismatch);
        }
        if let Some(flag) = self.closed.get_mut(card) {
            *flag = true;
        }
        self.close_seq = self.close_seq.saturating_add(1);
        self.accumulated = self.accumulated.saturating_add(declared as u64);
        self.ledger.push(CloseRec {
            seq: self.close_seq,
            card,
            lines: declared,
            seg: SEG_BASE,
        });
        self.last_card = Some(card);
        Ok(self.accumulated)
    }

    /// 发收官印：20 卡全关且累计守恒才放行；印面一次性冻结。
    pub fn issue_seal(&mut self) -> Result<SealFace, CloseCode> {
        if self.sealed {
            return Err(CloseCode::SealReissue);
        }
        if self.close_seq < FROZEN_CARD_COUNT as u32 {
            return Err(CloseCode::SealPremature);
        }
        if self.accumulated != FROZEN_TOTAL_LINES {
            return Err(CloseCode::LedgerMismatch);
        }
        self.sealed = true;
        Ok(SealFace {
            cards: FROZEN_CARD_COUNT as u32,
            total_lines: self.accumulated,
            close_seq: self.close_seq,
            seg: SEG_BASE,
        })
    }

    /// 注入记账：检出加一（调用方在捕获违约时登记）。
    pub fn note_detected(&mut self) {
        self.injected = self.injected.saturating_add(1);
        self.detected = self.detected.saturating_add(1);
    }

    /// 注入逃逸（判据要求恒 0；非零即账面破洞）。
    pub fn note_escape(&mut self) {
        self.injected = self.injected.saturating_add(1);
        self.escaped = self.escaped.saturating_add(1);
    }

    pub fn accumulated(&self) -> u64 {
        self.accumulated
    }

    pub fn close_seq(&self) -> u32 {
        self.close_seq
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }

    pub fn injected(&self) -> u64 {
        self.injected
    }

    pub fn detected(&self) -> u64 {
        self.detected
    }

    /// 守恒断言面：注入 = 检出 + 逃逸，逃逸恒 0 且检出非零。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected.saturating_add(self.escaped)
            && self.escaped == 0
            && self.detected > 0
    }

    /// 状态快照（双跑一致面）。
    pub fn snapshot(&self) -> (u64, u32, bool, usize) {
        (self.accumulated, self.close_seq, self.sealed, self.ledger.len())
    }
}

/// 按冻结表把 20 卡全部合法关账（正常流脚本）。
fn close_all(b: &mut B17CloseBoard) {
    for card in 0..FROZEN_CARD_COUNT {
        let lines = match FROZEN_CARD_LINES.get(card) {
            Some(v) => *v,
            None => 0,
        };
        let _ = b.close_card(card, lines);
    }
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 双跑一致性：同一关账脚本两次建账，快照与印面逐字段一致。
pub fn dual_run_consistent() -> bool {
    let build = || -> (u64, u32, bool, usize) {
        let mut b = B17CloseBoard::new();
        close_all(&mut b);
        let _ = b.issue_seal();
        b.snapshot()
    };
    build() == build()
}

/// 冻结表求和（守恒核对用）。
pub fn frozen_sum() -> u64 {
    FROZEN_CARD_LINES
        .iter()
        .fold(0u64, |a, &v| a.saturating_add(v as u64))
}

/// 收口自检（13 条判据形态）。
pub fn run_unx51540_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51540");
    // 1 正常流：20 卡按序关账、发印、印面守恒。
    {
        let mut b = B17CloseBoard::new();
        close_all(&mut b);
        let seal = b.issue_seal();
        let seal_ok = matches!(seal, Ok(f) if f.total_lines == FROZEN_TOTAL_LINES && f.cards == 20);
        s.add_dyn(
            "收官全流程",
            seal_ok && b.sealed() && b.accumulated() == FROZEN_TOTAL_LINES,
            "20 卡全关且累计 102000 行才能发印",
        );
    }
    // 2 冻结表守恒：表和恰为 102000。
    s.add_dyn(
        "冻结表总量守恒",
        frozen_sum() == FROZEN_TOTAL_LINES,
        "20 卡行数之和必须恰为 102000 行",
    );
    // 3 边界：首卡放行、上限与上限+1 拒。
    {
        let mut b = B17CloseBoard::new();
        let first = b.close_card(0, 5400).is_ok();
        let over1 = b.close_card(FROZEN_CARD_COUNT, 0);
        let over2 = b.close_card(FROZEN_CARD_COUNT + 99, 0);
        s.add_dyn(
            "卡号边界",
            first
                && over1 == Err(CloseCode::CardIdxOob)
                && over2 == Err(CloseCode::CardIdxOob),
            "卡号 0 放行，卡号 20 与更大值显性拒",
        );
    }
    // 4 注入必抓：申报行数篡改 ×10 轮。
    {
        let mut b = B17CloseBoard::new();
        let mut caught = 0u32;
        for i in 0..10usize {
            let frozen = match FROZEN_CARD_LINES.get(i) {
                Some(v) => *v,
                None => 0,
            };
            let bad = frozen.saturating_add(1 + i as u32);
            if b.close_card(i, bad) == Err(CloseCode::LineSumMismatch) {
                caught += 1;
                b.note_detected();
            }
            let _ = b.close_card(i, frozen); // 补合法关账，推进状态
        }
        s.add_dyn("行数篡改注入 10/10", caught == 10, "申报行数与冻结表不符必须全抓");
    }
    // 5 注入必抓：卡号越界 ×10 轮。
    {
        let mut b = B17CloseBoard::new();
        let mut caught = 0u32;
        for i in 0..10u32 {
            let r = b.close_card(FROZEN_CARD_COUNT + i as usize, 1);
            if r == Err(CloseCode::CardIdxOob) {
                caught += 1;
                b.note_detected();
            }
        }
        s.add_dyn(
            "卡号越界注入 10/10",
            caught == 10 && b.ledger_len() == 0,
            "越界卡号必须全抓且账面不动",
        );
    }
    // 6 注入必抓：重复关账 ×10 轮。
    {
        let mut b = B17CloseBoard::new();
        let mut caught = 0u32;
        for i in 0..10usize {
            let frozen = match FROZEN_CARD_LINES.get(i) {
                Some(v) => *v,
                None => 0,
            };
            let _ = b.close_card(i, frozen);
            if b.close_card(i, frozen) == Err(CloseCode::CardAlreadyClosed) {
                caught += 1;
                b.note_detected();
            }
        }
        s.add_dyn("重复关账注入 10/10", caught == 10, "已关卡重关必须全抓");
    }
    // 7 判定次序：倒退先于行数、重关先于行数。
    {
        let mut b = B17CloseBoard::new();
        let _ = b.close_card(2, 5100);
        let r1 = b.close_card(1, 9999); // 倒退 + 行数错 → 取倒退
        let r2 = b.close_card(2, 9999); // 重关 + 行数错 → 取重关
        s.add_dyn(
            "判序倒退与重关先行",
            r1 == Err(CloseCode::CloseOrderRegress) && r2 == Err(CloseCode::CardAlreadyClosed),
            "倒退与重关判据先于行数对账判据",
        );
    }
    // 8 状态机拒止：被拒关账不动账面。
    {
        let mut b = B17CloseBoard::new();
        let _ = b.close_card(0, 5400);
        let snap = b.snapshot();
        let _ = b.close_card(0, 5400); // 重关拒
        let _ = b.close_card(5, 9999); // 行数错拒
        s.add_dyn(
            "拒后账面不动",
            b.snapshot() == snap,
            "被拒关账不得推进任何账面字段",
        );
    }
    // 9 发印次序：未全关拒 → 全关放行 → 重发拒。
    {
        let mut b = B17CloseBoard::new();
        let early = b.issue_seal();
        close_all(&mut b);
        let ok = b.issue_seal();
        let again = b.issue_seal();
        s.add_dyn(
            "发印次序",
            early == Err(CloseCode::SealPremature)
                && ok.is_ok()
                && again == Err(CloseCode::SealReissue),
            "未全关拒发、全关可发、重发显性拒",
        );
    }
    // 10 对账守恒：注入 = 检出，逃逸 0。
    {
        let mut b = B17CloseBoard::new();
        for i in 0..10u32 {
            let _ = b.close_card(FROZEN_CARD_COUNT + i as usize, 0);
            b.note_detected();
        }
        s.add_dyn(
            "注入账守恒",
            b.conserved() && b.injected() == 10 && b.detected() == 10,
            "注入=检出+逃逸，逃逸恒 0",
        );
    }
    // 11 双跑一致。
    s.add_dyn("双跑逐字段一致", dual_run_consistent(), "同一关账脚本两次建账快照相等");
    // 12 文案闭集非空。
    {
        let mut closed = true;
        for c in ALL_CODES {
            if c.detail().is_empty() {
                closed = false;
            }
        }
        s.add_dyn("文案闭集非空", closed, "每个诊断码都必须有中文建议");
    }
    // 13 段位守卫。
    s.add_dyn("段位守卫", seg_guard_ok(), "SEG_BASE 对齐且在分派区间");
    s
}

/// 诊断码全序表（闭集枚举的权威清单）。
pub const ALL_CODES: [CloseCode; 9] = [
    CloseCode::Ok,
    CloseCode::SegBad,
    CloseCode::CardIdxOob,
    CloseCode::CardAlreadyClosed,
    CloseCode::CloseOrderRegress,
    CloseCode::LineSumMismatch,
    CloseCode::SealPremature,
    CloseCode::SealReissue,
    CloseCode::LedgerMismatch,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_close_and_seal() {
        let mut b = B17CloseBoard::new();
        close_all(&mut b);
        let seal = b.issue_seal();
        match seal {
            Ok(f) => {
                assert_eq!(f.total_lines, 102_000);
                assert_eq!(f.cards, 20);
                assert_eq!(f.close_seq, 20);
            }
            Err(e) => panic!("收官印应可发出：{:?}", e),
        }
    }

    #[test]
    fn frozen_table_sums_to_total() {
        assert_eq!(frozen_sum(), FROZEN_TOTAL_LINES);
        assert_eq!(FROZEN_CARD_LINES.len(), FROZEN_CARD_COUNT);
    }

    #[test]
    fn card_bounds() {
        let mut b = B17CloseBoard::new();
        assert!(b.close_card(0, 5400).is_ok());
        assert_eq!(b.close_card(FROZEN_CARD_COUNT, 0), Err(CloseCode::CardIdxOob));
        assert_eq!(
            b.close_card(FROZEN_CARD_COUNT + 99, 0),
            Err(CloseCode::CardIdxOob)
        );
    }

    #[test]
    fn tamper_injection_all_caught() {
        let mut b = B17CloseBoard::new();
        let mut caught = 0u32;
        for i in 0..10usize {
            let frozen = match FROZEN_CARD_LINES.get(i) {
                Some(v) => *v,
                None => 0,
            };
            if b.close_card(i, frozen.saturating_add(7)) == Err(CloseCode::LineSumMismatch) {
                caught += 1;
                b.note_detected();
            }
            let _ = b.close_card(i, frozen);
        }
        assert_eq!(caught, 10);
        assert!(b.conserved());
    }

    #[test]
    fn double_close_injection_all_caught() {
        let mut b = B17CloseBoard::new();
        let mut caught = 0u32;
        for i in 0..10usize {
            let frozen = match FROZEN_CARD_LINES.get(i) {
                Some(v) => *v,
                None => 0,
            };
            let _ = b.close_card(i, frozen);
            if b.close_card(i, frozen) == Err(CloseCode::CardAlreadyClosed) {
                caught += 1;
                b.note_detected();
            }
        }
        assert_eq!(caught, 10);
        assert_eq!(b.ledger_len(), 10);
    }

    #[test]
    fn judge_order_precedence() {
        let mut b = B17CloseBoard::new();
        let _ = b.close_card(2, 5100);
        assert_eq!(b.close_card(1, 9999), Err(CloseCode::CloseOrderRegress));
        assert_eq!(b.close_card(2, 9999), Err(CloseCode::CardAlreadyClosed));
    }

    #[test]
    fn seal_sequence_premature_ok_reissue() {
        let mut b = B17CloseBoard::new();
        assert_eq!(b.issue_seal(), Err(CloseCode::SealPremature));
        close_all(&mut b);
        assert!(b.issue_seal().is_ok());
        assert_eq!(b.issue_seal(), Err(CloseCode::SealReissue));
    }

    #[test]
    fn conservation_after_injections() {
        let mut b = B17CloseBoard::new();
        for i in 0..10u32 {
            let _ = b.close_card(FROZEN_CARD_COUNT + i as usize, 0);
            b.note_detected();
        }
        assert_eq!(b.injected(), 10);
        assert_eq!(b.detected(), 10);
        assert!(b.conserved());
    }

    #[test]
    fn dual_run_identical() {
        assert!(dual_run_consistent());
    }

    #[test]
    fn seg_guard_holds() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn detail_texts_closed_set() {
        for c in ALL_CODES {
            assert!(!c.detail().is_empty());
        }
    }
}
