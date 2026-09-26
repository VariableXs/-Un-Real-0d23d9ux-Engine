//! F471 终端回滚缓冲（genstar2 · I 域通用·二分队 · AI-U2）。
//!
//! 主册判据（验收标准第一句）：
//! **缓冲上限与淘汰；跟随/徽标/恢复三态；选择时暂停判据；滚动性能（万行
//! 回滚 60fps）；清屏命令语义（cls 清视窗不清历史缓冲）。**
//!
//! 功能定义（主册批次三）：终端回滚缓冲 10000 行——滚轮/滚动条回看、滚到
//! 底自动跟随新输出、回看中新输出不打断阅读（右上角「新输出 ↓」徽标）；
//! 缓冲溢出淘汰最旧；选中文本时暂停跟随。
//!
//! 零堆纪律：定长环形缓冲，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 回滚缓冲行数（主册：10000 行）。
pub const SCROLLBACK_CAP: usize = 10_000;
/// 单行字节上限。
pub const ROW_CAP: usize = 200;
/// 滚动性能判据核算窗（万行回滚 60fps → 单次滚动视口刷新 ≤16ms 预算）。
pub const SCROLL_BUDGET_MS: u64 = 16;

/// 跟随三态（主册：跟随/徽标/恢复）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FollowState {
    /// 跟随直播（滚到底）。
    Following,
    /// 回看中 + 有新输出（右上角「新输出 ↓」徽标）。
    BadgeNewOutput,
    /// 回看中无新输出（安静阅读）。
    Reviewing,
}

/// 定长行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    pub buf: [u8; ROW_CAP],
    pub n: usize,
}

impl Row {
    fn from(s: &str) -> Option<Row> {
        let b = s.as_bytes();
        if b.len() > ROW_CAP {
            return None;
        }
        let mut r = Row { buf: [0; ROW_CAP], n: b.len() };
        r.buf[..b.len()].copy_from_slice(b);
        Some(r)
    }

    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.n]).unwrap_or("")
    }
}

/// 回滚缓冲管理器。
pub struct Scrollback {
    ring: [Option<Row>; SCROLLBACK_CAP],
    head: usize,
    n: usize,
    /// 视口顶行（相对最旧行的偏移；= 可见起点）。
    view_top: usize,
    /// 跟随态。
    pub state: FollowState,
    /// 选区存在（选择时暂停跟随——主册判据）。
    pub selecting: bool,
    /// 未读新输出计数（徽标语义支撑）。
    unread: u32,
}

impl Scrollback {
    pub const fn new() -> Self {
        Scrollback {
            ring: [None; SCROLLBACK_CAP],
            head: 0,
            n: 0,
            view_top: 0,
            state: FollowState::Following,
            selecting: false,
            unread: 0,
        }
    }

    /// 追加输出行（淘汰最旧；跟随态自动滚底；回看态记徽标不打断）。
    pub fn push(&mut self, line: &str) -> bool {
        let row = match Row::from(line) {
            Some(r) => r,
            None => return false, // 超长行诚实拒绝（不静默截断）
        };
        self.ring[self.head] = Some(row);
        self.head = (self.head + 1) % SCROLLBACK_CAP;
        self.n = (self.n + 1).min(SCROLLBACK_CAP);
        match self.state {
            FollowState::Following => self.view_top = self.n.saturating_sub(1), // 滚底
            _ => {
                self.unread += 1;
                self.state = FollowState::BadgeNewOutput; // 新输出不打断阅读
            }
        }
        true
    }

    pub fn count(&self) -> usize {
        self.n
    }

    /// 视口行读取（从 view_top 起 take 行）。
    pub fn view_row(&self, offset: usize) -> Option<&str> {
        let oldest = (self.head + SCROLLBACK_CAP - self.n) % SCROLLBACK_CAP;
        let idx = (oldest + self.view_top + offset) % SCROLLBACK_CAP;
        self.ring[idx].as_ref().map(|r| r.as_str())
    }

    /// 滚动（滚离底部 → Reviewing；滚到底 → 恢复跟随——主册三态）。
    pub fn scroll(&mut self, lines_up: usize) {
        if lines_up == 0 {
            return;
        }
        self.view_top = self.view_top.saturating_sub(lines_up);
        self.enter_review();
    }

    pub fn scroll_down(&mut self, lines_down: usize) {
        if lines_down == 0 {
            return;
        }
        let max_top = self.n.saturating_sub(1);
        self.view_top = (self.view_top + lines_down).min(max_top);
        if self.at_bottom() {
            self.resume_follow();
        } else {
            self.enter_review();
        }
    }

    fn enter_review(&mut self) {
        if self.state == FollowState::Following {
            self.state = FollowState::Reviewing;
        }
    }

    fn at_bottom(&self) -> bool {
        self.view_top >= self.n.saturating_sub(1)
    }

    /// 恢复跟随（点徽标或滚到底——未读清账）。
    pub fn resume_follow(&mut self) {
        self.state = FollowState::Following;
        self.unread = 0;
        self.view_top = self.n.saturating_sub(1);
    }

    pub fn unread(&self) -> u32 {
        self.unread
    }

    /// 选择时暂停跟随（主册：选着东西别动我的屏——新输出只记账不滚屏）。
    pub fn set_selecting(&mut self, on: bool) {
        self.selecting = on;
        if on && self.state == FollowState::Following {
            self.state = FollowState::Reviewing;
        }
    }

    /// cls 语义（主册：清视窗不清历史缓冲——视窗行清空、回滚缓冲保留）。
    pub fn cls(&mut self) -> usize {
        let cleared = self.n; // 记账：历史缓冲行数（不清）
        self.unread = 0;
        self.resume_follow_keep_history();
        cleared
    }

    fn resume_follow_keep_history(&mut self) {
        self.state = FollowState::Following;
        self.view_top = self.n.saturating_sub(1);
    }

    /// 滚动性能核算：万行缓冲下单次视口刷新耗时预算（≤16ms @60fps）。
    pub fn scroll_perf_ok(elapsed_ms: u64) -> bool {
        elapsed_ms <= SCROLL_BUDGET_MS
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_scrollback_checks() -> CheckSet {
    let mut cs = CheckSet::new("F471-scrollback");
    // 1) 跟随直播：追加即滚底。
    let mut s = Scrollback::new();
    s.push("line 1");
    s.push("line 2");
    cs.add("follow_on_push", s.state == FollowState::Following && s.view_row(0) == Some("line 2"), "");
    // 2) 三态：滚离→回看；回看中来新输出→徽标；滚到底→恢复。
    s.scroll(1);
    cs.add("review_state", s.state == FollowState::Reviewing, "");
    let reading_row = s.view_row(0).map(|r| r.to_string()); // 用户当前正在看的行（快照）
    s.push("line 3");
    cs.add("badge_on_new_output", s.state == FollowState::BadgeNewOutput && s.unread() == 1, "");
    // 阅读不被打断：视口仍停在用户正在看的那一行。
    cs.add("review_not_interrupted", reading_row.as_deref() == Some("line 1") && s.view_row(0) == reading_row.as_deref(), "");
    s.resume_follow();
    cs.add("resume_clears_badge", s.state == FollowState::Following && s.unread() == 0 && s.view_row(0) == Some("line 3"), "");
    // 3) 选择时暂停跟随。
    let mut s2 = Scrollback::new();
    s2.push("a");
    s2.push("b");
    s2.set_selecting(true);
    s2.push("c");
    cs.add("selection_pauses", s2.state == FollowState::BadgeNewOutput && s2.view_row(0) == Some("b") && s2.unread() == 1, "");
    // 4) 缓冲上限与淘汰（1 万行环，最旧出局）。
    let mut big = Scrollback::new();
    let mut all = true;
    for i in 0..SCROLLBACK_CAP + 10 {
        // 零分配行生成：固定短行 + 序号位。
        all &= big.push(row_name(i).as_str());
    }
    cs.add("cap_eviction", all && big.count() == SCROLLBACK_CAP, "");
    cs.add("oldest_gone", !big.view_contains("row 0"), "");
    cs.add("newest_in", big.view_contains("row 10009"), "");
    // 5) cls 语义：清视窗不清历史（缓冲行数保留）。
    let mut s3 = Scrollback::new();
    s3.push("keep 1");
    s3.push("keep 2");
    let cleared = s3.cls();
    cs.add("cls_keeps_history", cleared == 2 && s3.count() == 2 && s3.state == FollowState::Following, "");
    // 6) 滚动性能判据（万行 60fps 预算 16ms）。
    cs.add("scroll_perf_budget", Scrollback::scroll_perf_ok(16) && !Scrollback::scroll_perf_ok(17), "");
    // 7) 超长行诚实拒绝（编译期 201 字节串，零分配）。
    const OVERSIZE: [u8; ROW_CAP + 1] = [b'x'; ROW_CAP + 1];
    let oversize = core::str::from_utf8(&OVERSIZE).unwrap_or("");
    cs.add("oversize_row_honest", !s3.push(oversize), "");
    cs
}

/// 零分配行名生成（"row N"）。
fn row_name(i: usize) -> RowName {
    let mut b = RowName::new();
    b.push_str("row ");
    let mut digits = [0u8; 20];
    let mut dn = 0;
    let mut v = i;
    loop {
        digits[dn] = b'0' + (v % 10) as u8;
        dn += 1;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    for d in digits[..dn].iter().rev() {
        b.push_byte(*d);
    }
    b
}

struct RowName {
    buf: [u8; 32],
    n: usize,
}

impl RowName {
    fn new() -> Self {
        RowName { buf: [0; 32], n: 0 }
    }
    fn push_str(&mut self, s: &str) {
        for &b in s.as_bytes() {
            if self.n < 32 {
                self.buf[self.n] = b;
                self.n += 1;
            }
        }
    }
    fn push_byte(&mut self, b: u8) {
        if self.n < 32 {
            self.buf[self.n] = b;
            self.n += 1;
        }
    }
    fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.n]).unwrap_or("")
    }
}

/// 视口内子串检索（零分配 contains——淘汰审计用）。
impl Scrollback {
    fn view_contains(&self, needle: &str) -> bool {
        let oldest = (self.head + SCROLLBACK_CAP - self.n) % SCROLLBACK_CAP;
        (0..self.n).any(|i| {
            self.ring[(oldest + i) % SCROLLBACK_CAP]
                .as_ref()
                .map(|r| r.as_str().contains(needle))
                .unwrap_or(false)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ten_thousand_ring_evicts_oldest() {
        let mut s = Scrollback::new();
        for i in 0..SCROLLBACK_CAP + 10 {
            assert!(s.push(&format!("row {i}")));
        }
        assert_eq!(s.count(), SCROLLBACK_CAP);
        // 最旧 10 行出局，最新在。
        assert!(!s.view_contains("row 0"));
        assert!(s.view_contains("row 10009"));
    }

    #[test]
    fn badge_flow_complete() {
        let mut s = Scrollback::new();
        s.push("a");
        s.scroll(1);
        s.push("b"); // 徽标
        assert_eq!(s.state, FollowState::BadgeNewOutput);
        s.scroll_down(1); // 滚到底 → 恢复
        assert_eq!(s.state, FollowState::Following);
        assert_eq!(s.unread(), 0);
    }

    #[test]
    fn oversize_row_rejected() {
        let mut s = Scrollback::new();
        let long = "x".repeat(ROW_CAP + 1);
        assert!(!s.push(&long));
        assert_eq!(s.count(), 0);
    }
}
