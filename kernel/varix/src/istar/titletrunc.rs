//! F584 标题超长截断 · 完整设计（STAR I 主册 I 域批次八）。
//!
//! **判据（主册）**：保头规则；三处一致；Tooltip 全文；与 F247 对照表；
//! 中英混排截断。
//!
//! **设计要点（主册）**：
//! - 窗口标题超长的显示策略：保头截尾（「文档 - 报告 - 2026年… - 记事本」
//!   ——层级前缀保住、尾部省略）+ Tooltip 全文（F205）——与文件路径保尾
//!   （F247）相反因为标题的识别信息在前；
//! - 任务栏按钮标题同步策略；Alt+Tab 列表（F082）显示宽裕可少截。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;

use alloc::string::String;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 标题栏可用宽度（字符位——中英混排按显示位计：CJK=2、ASCII=1）。
pub const TITLE_COLS: usize = 40;

/// 任务栏按钮宽（更窄——截断更狠，策略一致）。
pub const TASKBAR_COLS: usize = 16;

/// Alt+Tab 卡片宽（宽裕——可少截）。
pub const ALTTAB_COLS: usize = 60;

// ---------------------------------------------------------------------------
// 模型
// ---------------------------------------------------------------------------

/// 显示位宽（CJK 记 2、其余记 1——中英混排截断的统一度量）。
pub fn display_cols(s: &str) -> usize {
    s.chars().map(|c| if (c as u32) > 0x2E7F { 2 } else { 1 }).sum()
}

/// 截断方向。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TruncSide {
    /// 保头截尾（标题——识别信息在前）。
    KeepHead,
    /// 保尾截头（路径 F247——对照面）。
    KeepTail,
}

/// 标题截断器（三处显示共用一台策略机——三处一致的结构证据）。
pub struct TitleTrunc;

impl TitleTrunc {
    /// 截断（保头模式：超出宽度 → 砍尾 + 省略号「…」；不超 → 原文）。
    ///
    /// 与 F247 对照：KeepHead 用于标题、KeepTail 用于路径——同机不同向，
    /// 对照表即本枚举（一处一事实）。
    pub fn truncate(text: &str, max_cols: usize, side: TruncSide) -> String {
        if display_cols(text) <= max_cols {
            return String::from(text);
        }
        let budget = max_cols.saturating_sub(1); // 省略号「…」占 1 位
        let mut out = String::new();
        let mut used = 0usize;
        match side {
            TruncSide::KeepHead => {
                for c in text.chars() {
                    let w = if (c as u32) > 0x2E7F { 2 } else { 1 };
                    if used + w > budget {
                        break;
                    }
                    out.push(c);
                    used += w;
                }
                out.push('…');
            }
            TruncSide::KeepTail => {
                let chars: alloc::vec::Vec<char> = text.chars().collect();
                let mut taken = alloc::vec::Vec::new();
                let mut tail_used = 0usize;
                for c in chars.iter().rev() {
                    let w = if (*c as u32) > 0x2E7F { 2 } else { 1 };
                    if tail_used + w > budget {
                        break;
                    }
                    tail_used += w;
                    taken.push(*c);
                }
                taken.reverse();
                out.push('…');
                for c in taken {
                    out.push(c);
                }
            }
        }
        out
    }

    /// 三处一致：标题栏/任务栏/Alt+Tab 同走本机（宽度不同策略相同）。
    pub fn three_surfaces(text: &str) -> (String, String, String) {
        (
            Self::truncate(text, TITLE_COLS, TruncSide::KeepHead),
            Self::truncate(text, TASKBAR_COLS, TruncSide::KeepHead),
            Self::truncate(text, ALTTAB_COLS, TruncSide::KeepHead),
        )
    }

    /// Tooltip 全文（F205——截了什么悬停可见）。
    pub fn tooltip(text: &str, max_cols: usize) -> Option<&str> {
        if display_cols(text) > max_cols {
            Some(text)
        } else {
            None // 不超长不弹 tooltip（不该出现时不挡路）。
        }
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_titletrunc_checks() -> CheckSet {
    let mut set = CheckSet::new(ISTAR_DOMAIN);

    let long = "文档 - 报告 - 2026年第三季度 - 终版 - 记事本";

    // 1. 保头规则：头（「文档」前缀）保住、尾（「记事本」）被截。
    let t = TitleTrunc::truncate(long, TITLE_COLS, TruncSide::KeepHead);
    set.add(
        "keep head rule",
        t.starts_with("文档") && t.ends_with('…') && display_cols(&t) <= TITLE_COLS,
        "",
    );

    // 2. 不超长不截（原样返回——截断是设计不是习惯）。
    let short = "报告 - 记事本";
    set.add(
        "within width untouched",
        TitleTrunc::truncate(short, TITLE_COLS, TruncSide::KeepHead) == short,
        "",
    );

    // 3. 三处一致：同策略不同宽（Alt+Tab 宽裕可少截——三串保头一致）。
    let (a, b, c) = TitleTrunc::three_surfaces(long);
    set.add(
        "three surfaces same policy",
        a.starts_with("文档") && b.starts_with("文档") && c.starts_with("文档")
            && display_cols(&c) <= ALTTAB_COLS,
        "",
    );

    // 4. Alt+Tab 少截：60 宽位的卡片比 40 宽位的标题栏留得多（或全留）。
    set.add(
        "alt tab truncates less",
        display_cols(&c) >= display_cols(&a),
        "",
    );

    // 5. Tooltip 全文：超长给全文、不超长不给（不挡路）。
    let tip = TitleTrunc::tooltip(long, TITLE_COLS);
    let no_tip = TitleTrunc::tooltip(short, TITLE_COLS);
    set.add(
        "tooltip full text conditional",
        tip == Some(long) && no_tip.is_none(),
        "",
    );

    // 6. 与 F247 对照表：标题保头、路径保尾——同机两向（对照即枚举）。
    let path = "D:\\工作\\项目\\2026\\第三季度\\报告\\终版\\季度报告.docx";
    let path_trunc = TitleTrunc::truncate(path, TITLE_COLS, TruncSide::KeepTail);
    set.add(
        "f247 comparison keep tail for paths",
        path_trunc.starts_with('…') && path_trunc.ends_with(".docx"),
        "",
    );

    // 7. 中英混排截断：CJK 2 位 / ASCII 1 位，混排不切出半个字符。
    let mixed = "Varix 报告 report 文档 2026";
    let m = TitleTrunc::truncate(mixed, 12, TruncSide::KeepHead);
    set.add(
        "mixed cjk ascii safe cut",
        display_cols(&m) <= 12 && m.ends_with('…') && m.starts_with("Varix"),
        "",
    );

    // 8. 截断结果永不超宽（上限红线全域成立）。
    let fits = [long, path, mixed, short]
        .iter()
        .all(|s| display_cols(&TitleTrunc::truncate(s, TITLE_COLS, TruncSide::KeepHead)) <= TITLE_COLS);
    set.add("never exceeds width", fits, "");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_cols_mixed() {
        assert_eq!(display_cols("ab"), 2);
        assert_eq!(display_cols("中"), 2);
        assert_eq!(display_cols("a中"), 3);
    }

    #[test]
    fn exact_fit_no_ellipsis() {
        let s = "abcdefghij"; // 10 位
        assert_eq!(TitleTrunc::truncate(s, 10, TruncSide::KeepHead), s);
    }

    #[test]
    fn one_over_truncates_one() {
        let s = "abcdefghijk"; // 11 位 → 限宽 10：保 9 位 + 省略号
        let t = TitleTrunc::truncate(s, 10, TruncSide::KeepHead);
        assert_eq!(t, "abcdefghi…");
    }
}
