//! 深化层 · F584 标题超长截断（回炉补深主册【设计要点】未实装机制）。
//!
//! 补深四条（判据唯一源：主册 F584 节）：
//! ①「层级感知保头截断引擎」——按「 - 」分段：层级前缀段保住、末段
//!   （应用名「记事本」）也保住、中段收纳成「…」（主册样例
//!   「文档 - 报告 - … - 记事本」形态——基础件纯保头截尾截掉末段）；
//! ②「中英混排宽度计量」——全角 2 半角 1 的逐段宽度累计（复用基础件
//!   display_cols 单一度量），截断结果永不超预算；
//! ③「三处一致性对账」——标题栏/任务栏/Alt+Tab 同源派生：首段与末段
//!   锚点一致（宽度不同策略同源），Alt+Tab 宽裕最少截；
//! ④「Tooltip 全文触发账」——被截才给全文（任一处被截即触发，未截
//!   不弹不挡路）。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;
use crate::istar::titletrunc::{
    display_cols, TitleTrunc, TruncSide, TITLE_COLS, TASKBAR_COLS, ALTTAB_COLS,
};

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 层级感知截断引擎
// ---------------------------------------------------------------------------

/// 层级分隔符（窗口标题「文档 - 报告 - … - 记事本」的分段口径）。
pub const SEP: &str = " - ";

/// 省略号（与基础件同字符——单一度量）。
const ELL: &str = "…";

/// 层级感知截断引擎。
pub struct SegmentTrunc;

impl SegmentTrunc {
    /// 分段（「 - 」切层——层级逐段计量）。
    pub fn split(text: &str) -> Vec<&str> {
        text.split(SEP).collect()
    }

    /// 层级保头 + 末段保尾：预算装得下首段 + 末段时中段收「…」；
    /// 装不下退化为基础件纯保头截尾（诚实降级，不硬塞）。
    pub fn truncate(text: &str, max_cols: usize) -> String {
        if display_cols(text) <= max_cols {
            return String::from(text);
        }
        let segs = Self::split(text);
        if segs.len() < 3 {
            return TitleTrunc::truncate(text, max_cols, TruncSide::KeepHead);
        }
        let last = segs[segs.len() - 1];
        // 头部预算 = 总宽 − 省略号(1) − 末段带分隔的宽度
        let tail_cols = display_cols(SEP) + display_cols(last);
        let head_budget = max_cols.saturating_sub(1 + tail_cols);
        let mut head = String::new();
        let mut used = 0usize;
        for (i, seg) in segs[..segs.len() - 1].iter().enumerate() {
            let w = display_cols(seg) + if i == 0 { 0 } else { display_cols(SEP) };
            if used + w > head_budget {
                break;
            }
            if i > 0 {
                head.push_str(SEP);
            }
            head.push_str(seg);
            used += w;
        }
        if head.is_empty() {
            // 首段都装不下 → 纯保头截尾（尾部省略——判据对照面）
            return TitleTrunc::truncate(text, max_cols, TruncSide::KeepHead);
        }
        head.push_str(ELL);
        head.push_str(SEP);
        head.push_str(last);
        head
    }

    /// 三处同源派生（同一台引擎三种预算——策略一致结构证据）。
    pub fn three_surfaces(text: &str) -> (String, String, String) {
        (
            Self::truncate(text, TITLE_COLS),
            Self::truncate(text, TASKBAR_COLS),
            Self::truncate(text, ALTTAB_COLS),
        )
    }
}

// ---------------------------------------------------------------------------
// Tooltip 全文触发账
// ---------------------------------------------------------------------------

/// Tooltip 全文触发账（被截才给全文——不截不弹）。
pub struct TooltipLedger {
    truncated_surfaces: u32,
    offered: u32,
}

impl TooltipLedger {
    pub fn new() -> TooltipLedger {
        TooltipLedger { truncated_surfaces: 0, offered: 0 }
    }

    /// 观察一处派生结果（显示串比原文窄 → 记被截）。
    pub fn observe(&mut self, original: &str, shown: &str) {
        if display_cols(shown) < display_cols(original) {
            self.truncated_surfaces += 1;
        }
    }

    /// 触发全文：任一处被截 → 给一次全文；未截 → None（不挡路）。
    pub fn offer<'a>(&mut self, full: &'a str) -> Option<&'a str> {
        if self.truncated_surfaces > 0 {
            self.offered += 1;
            Some(full)
        } else {
            None
        }
    }

    pub fn truncated_surfaces(&self) -> u32 {
        self.truncated_surfaces
    }

    pub fn offered(&self) -> u32 {
        self.offered
    }
}

impl Default for TooltipLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

pub fn run_f584_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new(ISTAR_DOMAIN);

    let long = "文档 - 报告 - 2026年第三季度 - 终版审阅批注稿 - 记事本";

    // 1) 层级保头 + 末段保尾：首段「文档」、末段「记事本」、中段「…」。
    let t = SegmentTrunc::truncate(long, TITLE_COLS);
    cs.add(
        "head prefix and app tail kept",
        t.starts_with("文档") && t.ends_with(" - 记事本") && t.contains('…'),
        "",
    );

    // 2) 宽度红线：三处预算下结果永不超宽（混排度量单一源）。
    let (a, b, c) = SegmentTrunc::three_surfaces(long);
    cs.add(
        "never exceeds three budgets",
        display_cols(&a) <= TITLE_COLS
            && display_cols(&b) <= TASKBAR_COLS
            && display_cols(&c) <= ALTTAB_COLS,
        "",
    );

    // 3) 三处一致性对账：同源派生——首段与末段锚点一致。
    cs.add(
        "three surfaces same anchors",
        a.starts_with("文档") && b.starts_with("文档") && c.starts_with("文档")
            && a.ends_with(" - 记事本")
            && b.ends_with(" - 记事本")
            && c.ends_with(" - 记事本"),
        "",
    );

    // 4) Alt+Tab 最少截：宽预算派生串不窄于窄预算（可少截判据）。
    cs.add(
        "alt tab truncates least",
        display_cols(&c) >= display_cols(&a) && display_cols(&a) >= display_cols(&b),
        "",
    );

    // 5) 退化路径：预算连首段都装不下 → 纯保头截尾（不硬塞末段）。
    let tiny = SegmentTrunc::truncate(long, 8);
    cs.add(
        "tight budget honest fallback",
        display_cols(&tiny) <= 8 && tiny.ends_with('…'),
        "",
    );

    // 6) 不超长原样（截断是设计不是习惯）。
    let short = "报告 - 记事本";
    cs.add(
        "within budget untouched",
        SegmentTrunc::truncate(short, TITLE_COLS) == short,
        "",
    );

    // 7) Tooltip 触发账：三处观察有被截 → 全文恰给一次；短标题全未截
    //    → 不弹（被截才给全文）。
    let mut tip = TooltipLedger::new();
    tip.observe(long, &a);
    tip.observe(long, &b);
    tip.observe(long, &c);
    let offered = tip.offer(long);
    let (sa, sb, sc) = SegmentTrunc::three_surfaces(short);
    let mut tip2 = TooltipLedger::new();
    tip2.observe(short, &sa);
    tip2.observe(short, &sb);
    tip2.observe(short, &sc);
    cs.add(
        "tooltip only when truncated",
        offered == Some(long)
            && tip.offered() == 1
            && tip.truncated_surfaces() >= 1
            && tip2.offer(short).is_none(),
        "",
    );

    // 8) F247 对照面：路径保尾不回退（同机两向对照仍成立）。
    let path = "D:\\工作\\项目库\\2026\\第三季度\\季度报告终版.docx";
    let pt = TitleTrunc::truncate(path, TITLE_COLS, TruncSide::KeepTail);
    cs.add("f247 keep tail contrast kept", pt.starts_with('…') && pt.ends_with(".docx"), "");

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_segments_falls_back_to_keep_head() {
        let t = SegmentTrunc::truncate("文档 - 记事本超长标题啊啊啊啊啊啊啊", 12);
        assert!(display_cols(&t) <= 12);
        assert!(t.ends_with('…'));
    }

    #[test]
    fn split_counts_segments() {
        assert_eq!(SegmentTrunc::split("a - b - c - d").len(), 4);
        assert_eq!(SegmentTrunc::split("无分隔符标题").len(), 1);
    }

    #[test]
    fn exact_fit_untouched() {
        let s = "a - b - c"; // 显示位宽 9
        assert_eq!(SegmentTrunc::truncate(s, 9), s);
    }
}
