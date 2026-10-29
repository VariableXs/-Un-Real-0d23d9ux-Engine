//! UNX-F10974 滚动重绘一致性联测（输出与逻辑屏对账）（0x1d940 段）
//!
//! 卡片判据（UNX-F10974-J1）：
//! - 滚动百轮输出序一致（10/10），重绘账对平（10/10）。
//!
//! 域：CoRun·UNX-C4-B29。本模块建模终端滚动重绘一致性：逻辑屏
//! 滚动一行（顶部行移出、底部新行移入）后重绘按行序输出全屏，
//! 输出序与逻辑屏逐行一致；重绘账（行数/字节）逐轮对平。

use crate::alloc::string::String;
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 错误码（SEG_BASE = 0x1d000 + (10974-10900)*0x20 = 0x1d940）
// ---------------------------------------------------------------------------
const SEG_BASE: u32 = 0x1d940;

/// SR_EINVAL：行号/内容非法
pub const SR_EINVAL: u32 = SEG_BASE + 0x01;
/// SR_EORDER：输出序与逻辑屏不一致（自检面）
pub const SR_EORDER: u32 = SEG_BASE + 0x02;
/// SR_ELOSE：重绘账不平（自检面）
pub const SR_ELOSE: u32 = SEG_BASE + 0x03;
/// SR_EEMPTY：空屏滚动
pub const SR_EEMPTY: u32 = SEG_BASE + 0x04;
/// SR_ETWICE：同轮重复重绘
pub const SR_ETWICE: u32 = SEG_BASE + 0x05;
/// SR_ESTATE：逻辑屏状态错乱
pub const SR_ESTATE: u32 = SEG_BASE + 0x06;

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------
/// 逻辑屏行数
pub const SCREEN_ROWS: usize = 8;
/// 每行宽度
pub const ROW_W: usize = 20;

// ---------------------------------------------------------------------------
// 数据结构
// ---------------------------------------------------------------------------

/// 一行内容
#[derive(Clone, Debug, PartialEq)]
pub struct ScrLine {
    pub no: u64,
    pub text: String,
}

impl ScrLine {
    /// FNV 折叠
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        h = (h ^ self.no).wrapping_mul(0x100000001b3);
        for b in self.text.as_bytes() {
            h = (h ^ (*b as u64)).wrapping_mul(0x100000001b3);
        }
        h
    }
}

/// 滚动重绘联测台
#[derive(Clone, Debug)]
pub struct ScrollRig {
    /// 逻辑屏（顶→底）
    pub screen: Vec<ScrLine>,
    /// 下一行号
    next_no: u64,
    /// 重绘输出（行号序列）
    pub out_order: Vec<u64>,
    /// 重绘轮数
    pub rounds: u64,
    /// 重绘行数账
    pub redraw_lines: u64,
    /// 当前轮是否已重绘
    drew_this_round: bool,
    /// 错误计数
    pub n_eseq: u32,
    pub n_lose: u32,
}

impl ScrollRig {
    pub fn new() -> Self {
        ScrollRig {
            screen: Vec::new(),
            next_no: 1,
            out_order: Vec::new(),
            rounds: 0,
            redraw_lines: 0,
            drew_this_round: false,
            n_eseq: 0,
            n_lose: 0,
        }
    }

    /// 底部追加一行
    pub fn append(&mut self, text: &str) -> Result<u64, u32> {
        if text.len() > ROW_W {
            return Err(SR_EINVAL);
        }
        let no = self.next_no;
        self.next_no = self.next_no.wrapping_add(1);
        self.screen.push(ScrLine { no, text: String::from(text) });
        self.drew_this_round = false;
        Ok(no)
    }

    /// 滚动一行：顶行移出，底部新行可选
    pub fn scroll(&mut self, new_text: Option<&str>) -> Result<u64, u32> {
        if self.screen.is_empty() {
            return Err(SR_EEMPTY);
        }
        let _ = self.screen.remove(0);
        self.drew_this_round = false;
        match new_text {
            Some(t) => self.append(t),
            None => Ok(0),
        }
    }

    /// 重绘：按行序输出全部行
    pub fn redraw(&mut self) -> Result<usize, u32> {
        if self.drew_this_round {
            self.n_eseq = self.n_eseq.wrapping_add(1);
            return Err(SR_ETWICE);
        }
        if self.screen.len() > SCREEN_ROWS {
            self.n_lose = self.n_lose.wrapping_add(1);
            return Err(SR_ESTATE);
        }
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.screen.len() {
            self.out_order.push(self.screen[i].no);
            self.redraw_lines = self.redraw_lines.wrapping_add(1);
            n = n.wrapping_add(1);
            i = i.wrapping_add(1);
        }
        self.rounds = self.rounds.wrapping_add(1);
        self.drew_this_round = true;
        Ok(n)
    }

    /// 输出序与逻辑屏一致：末轮输出的最后 n 项 == 当前屏行号列
    pub fn order_ok(&self, n: usize) -> bool {
        let len = self.out_order.len();
        if len < n || self.screen.len() != n {
            return false;
        }
        let mut i = 0usize;
        while i < n {
            if self.out_order[len - n + i] != self.screen[i].no {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 行号严格递增（FIFO 滚动序）
    pub fn fifo_ok(&self) -> bool {
        for w in self.screen.windows(2) {
            if w[1].no <= w[0].no {
                return false;
            }
        }
        true
    }

    /// 对账：重绘行数 = 轮数×屏行数
    pub fn reconcile(&self, expect_rounds: u64) -> Result<(), u32> {
        if self.rounds != expect_rounds {
            return Err(SR_ELOSE);
        }
        if self.screen.len() == SCREEN_ROWS
            && self.redraw_lines != self.rounds * SCREEN_ROWS as u64
        {
            return Err(SR_ELOSE);
        }
        Ok(())
    }

    /// 账目对平
    pub fn ledger_ok(&self, expect_rounds: u64) -> bool {
        self.rounds == expect_rounds && self.reconcile(expect_rounds).is_ok()
    }

    /// 总折叠
    pub fn fold_all(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for l in &self.screen {
            h = (h ^ l.fold()).wrapping_mul(0x100000001b3);
        }
        h = (h ^ self.rounds).wrapping_mul(0x100000001b3);
        h = (h ^ self.redraw_lines).wrapping_mul(0x100000001b3);
        h
    }

    /// 渲染
    pub fn render(&self) -> String {
        crate::alloc::format!(
            "scroll:rows={} rounds={} lines={} out={}",
            self.screen.len(),
            self.rounds,
            self.redraw_lines,
            self.out_order.len()
        )
    }
}

impl Default for ScrollRig {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 注入四路
// ---------------------------------------------------------------------------
pub fn inject_four() -> [bool; 4] {
    let mut r = ScrollRig::new();
    // 路1：空屏滚动 → EEMPTY
    let r1 = matches!(r.scroll(None), Err(x) if x == SR_EEMPTY);
    // 路2：超宽行 → EINVAL
    let r2 = matches!(r.append("xxxxxxxxxxxxxxxxxxxxx"), Err(x) if x == SR_EINVAL);
    // 路3：重复重绘 → ETWICE
    let _ = r.append("a");
    let _ = r.redraw();
    let r3 = matches!(r.redraw(), Err(x) if x == SR_ETWICE);
    // 路4：输出序面：伪造逆序行号必被 FIFO 检出
    let mut r2b = ScrollRig::new();
    for i in 0..3u64 {
        let _ = r2b.append("l");
        r2b.screen[i as usize].no = 10 - i; // 逆序行号
    }
    let _ = r2b.redraw();
    let r4 = !r2b.fifo_ok();
    [r1, r2, r3, r4]
}

// ---------------------------------------------------------------------------
// 确定性千轮样本
// ---------------------------------------------------------------------------
/// 千轮：每轮滚动一次并重绘一次；输出序每轮一致记 1 轮。
pub fn sample_thousand() -> u32 {
    let mut r = ScrollRig::new();
    // 预填满屏
    for i in 0..SCREEN_ROWS {
        let _ = r.append("init");
        let _ = i;
    }
    let mut good: u32 = 0;
    for t in 0..1000u32 {
        let txt = crate::alloc::format!("t{}", t % 10);
        if r.scroll(Some(&txt)).is_err() {
            return good;
        }
        match r.redraw() {
            Ok(n) => {
                if n == SCREEN_ROWS && r.order_ok(SCREEN_ROWS) && r.fifo_ok() {
                    good = good.wrapping_add(1);
                }
            }
            Err(_) => {}
        }
    }
    good
}

// ---------------------------------------------------------------------------
// 判据检查
// ---------------------------------------------------------------------------
pub fn run_unx10974_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10974");

    let inj = inject_four();
    s.add("J1-注入四路全拦截", inj.iter().all(|&b| b), "EEMPTY/EINVAL/ETWICE/乱序面坏样本全部命中");

    // J1 滚动百轮输出序一致 10/10
    let mut ok10 = 0u32;
    for seed in 0..10u32 {
        let mut r = ScrollRig::new();
        for i in 0..SCREEN_ROWS {
            let _ = r.append("pre");
        }
        let mut all_ok = true;
        for t in 0..100u32 {
            let txt = crate::alloc::format!("s{}-{}", seed, t % 10);
            let _ = r.scroll(Some(&txt));
            if r.redraw() != Ok(SCREEN_ROWS) || !r.order_ok(SCREEN_ROWS) {
                all_ok = false;
            }
        }
        if all_ok && r.ledger_ok(100) && r.fifo_ok() {
            ok10 = ok10.wrapping_add(1);
        }
    }
    s.add(
        "J1-百轮输出序一致10/10",
        ok10 == 10,
        "十组百轮滚动重绘输出序全一致",
    );

    // 重绘账对平 10/10
    let mut led10 = 0u32;
    for _ in 0..10 {
        let mut r = ScrollRig::new();
        for i in 0..SCREEN_ROWS {
            let _ = r.append("x");
        }
        for _ in 0..50 {
            let _ = r.scroll(Some("n"));
            let _ = r.redraw();
        }
        if r.ledger_ok(50) && r.redraw_lines == 50 * SCREEN_ROWS as u64 {
            led10 = led10.wrapping_add(1);
        }
    }
    s.add("J2-重绘账对平10/10", led10 == 10, "行数账=轮数×屏行数");

    // 千轮样本
    let good = sample_thousand();
    s.add("J3-千轮样本", good == 1000, "千轮滚动重绘全对");

    // FIFO 滚动序
    let mut r2 = ScrollRig::new();
    for i in 0..5 {
        let _ = r2.append("row");
    }
    let first_no = r2.screen[0].no;
    let _ = r2.scroll(Some("new"));
    let fifo_ok = r2.screen.len() == 5 && r2.screen[0].no == first_no + 1;
    s.add("J4-FIFO滚动", fifo_ok, "滚动后顶行号前移一位");

    // 错误码分段
    let codes = [SR_EINVAL, SR_EORDER, SR_ELOSE, SR_EEMPTY, SR_ETWICE, SR_ESTATE];
    let seg_ok = codes.iter().all(|&c| c > SEG_BASE && c < SEG_BASE + 0x20);
    s.add("J5-错误码分段", seg_ok, "六码全部落在 0x1d940 段内");

    // 错误码互异
    let mut distinct = true;
    for i in 0..codes.len() {
        for j in (i + 1)..codes.len() {
            if codes[i] == codes[j] {
                distinct = false;
            }
        }
    }
    s.add("J6-错误码互异", distinct, "六码两两互异");

    // 账目对平
    s.add("J7-账目对平", r2.ledger_ok(0), "未重绘轮账面为零");

    // fold 稳定
    let h1 = r2.fold_all();
    let h2 = r2.fold_all();
    s.add("J8-fold稳定", h1 == h2, "同一状态两次折叠一致");

    // 渲染
    let r_txt = r2.render();
    s.add(
        "J9-渲染非空",
        r_txt.starts_with("scroll:") && r_txt.contains("rows=5"),
        "摘要含屏行与重绘账",
    );

    // 屏行上界面
    let mut r3 = ScrollRig::new();
    for _ in 0..SCREEN_ROWS {
        let _ = r3.append("f");
    }
    // 模拟超屏：手工推入超限行并重绘必报
    r3.screen.push(ScrLine { no: 999, text: String::from("over") });
    let over = matches!(r3.redraw(), Err(x) if x == SR_ESTATE);
    s.add("J10-超屏保护", over, "逻辑屏超限重绘显性报错");

    // 动态判据
    s.add_dyn(
        "J11-重绘账",
        r2.rounds == 0 && r2.redraw_lines == 0,
        &crate::alloc::format!("rounds={} lines={}", r2.rounds, r2.redraw_lines),
    );
    s.add_dyn(
        "J12-千轮通过率",
        good == 1000,
        &crate::alloc::format!("{}//1000", good),
    );
    s.add_dyn(
        "J13-输出账",
        r2.out_order.is_empty(),
        &crate::alloc::format!("out={}", r2.out_order.len()),
    );

    s
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10974_scroll_redraw_flow() {
        let mut r = ScrollRig::new();
        for _ in 0..SCREEN_ROWS {
            let _ = r.append("line");
        }
        assert!(r.redraw().is_ok());
        assert!(r.order_ok(SCREEN_ROWS));
        let _ = r.scroll(Some("next"));
        assert!(r.redraw().is_ok());
        assert!(r.fifo_ok());
        assert!(matches!(r.redraw(), Err(x) if x == SR_ETWICE));
        let inj = inject_four();
        assert!(inj.iter().all(|&b| b));
        assert_eq!(sample_thousand(), 1000);
    }

    #[test]
    fn unx10974_checks_green() {
        let s = run_unx10974_checks();
        assert!(s.all_passed(), "UNX-F10974 判据未全绿");
    }
}
