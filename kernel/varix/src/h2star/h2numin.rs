//! H2 数字输入解析 · 深化批次六（F296 round-trip 的输入侧——用户
//! 键入带千分位/空格/全角的数字串 → 数值，与 h2locale 格式化闭环）。
//!
//! **承接判据**（主册 H 域正文，一处一事实）：
//! - **F296 区域显示格式**：显示侧 `format_number` 已在
//!   [`crate::h2star::h2locale`]——输入侧容错解析在此：千分位分隔
//!   （按区域档案）、空格、全角数字全容忍（用户怎么输都能读对，
//!   「写得出也读得回」的双向闭环）；
//! - **六章「粘贴内容清洗」**：粘贴进来的数字串经本解析清洗——
//!   非法字符拒绝并指出位置（错误三要素：哪里错、为什么、怎么改）；
//! - **十年不变**：解析规则只认档案里的分隔符与十进制数字——
//!   不猜格式（与 h2settings「不猜格式」同纪律）。
//!
//! 负数、小数（按档案 decimal）均支持；溢出 u64/i64 显式拒绝。

use crate::checks::CheckSet;

use crate::h2star::h2locale::RegionProfile;

use alloc::string::String;

// ---------------------------------------------------------------------------
// 容错数字解析
// ---------------------------------------------------------------------------

/// 全角数字 → 半角（0-9 与正负号——中文输入法最常见偏移）。
fn normalize_char(c: char) -> Option<char> {
    match c {
        '０'..='９' => Some((c as u32 - '０' as u32 + '0' as u32) as u8 as char),
        '－' | '−' => Some('-'),
        '．' => Some('.'),
        '0'..='9' | '+' | '-' | '.' => Some(c),
        _ => None, // 字母与其他符号显式拒绝（指出位置）
    }
}

/// 解析整数（i64）：剥离千分位与空格 → 逐字符归一 → 解析。
/// 非法字符返回 Err(位置, 人话)。
pub fn parse_int(p: &RegionProfile, input: &str) -> Result<i64, (usize, String)> {
    let mut cleaned = String::new();
    for (i, raw) in input.char_indices() {
        if raw == p.thousands || raw == ' ' || raw == '\u{3000}' {
            continue; // 千分位/空格先剥（不算非法字符）
        }
        let c = normalize_char(raw).ok_or_else(|| {
            (i, alloc::format!("字符「{raw}」不是数字——只允许数字、{0}、小数点和正负号", p.thousands))
        })?;
        cleaned.push(c);
    }
    let cleaned = cleaned.as_str();
    if cleaned.is_empty() || cleaned == "-" {
        return Err((0, "没有可读的数字".into()));
    }
    cleaned
        .parse::<i64>()
        .map_err(|_| (0, "数值超出范围（±922 亿亿）——请检查位数".into()))
}

/// 解析小数（f64）：整数部分同上，小数点按档案 decimal。
pub fn parse_float(p: &RegionProfile, input: &str) -> Result<f64, (usize, String)> {
    let mut cleaned = String::new();
    for (i, raw) in input.char_indices() {
        if raw == p.thousands || raw == ' ' || raw == '\u{3000}' {
            continue;
        }
        if raw == p.decimal {
            cleaned.push('.'); // 小数点按档案映射（先于白名单校验）
            continue;
        }
        let c = normalize_char(raw).ok_or_else(|| {
            (i, alloc::format!("字符「{raw}」不是数字——只允许数字、{0}、小数点和正负号", p.thousands))
        })?;
        cleaned.push(c);
    }
    cleaned
        .trim()
        .parse::<f64>()
        .map_err(|_| (0, "不是合法的小数——示例：1.5".into()))
}

/// round-trip 断言：format_number 输出必能被 parse_int 读回原值
/// （显示与输入的闭环机检——F296「三处同数」的输入面）。
pub fn roundtrip(p: &RegionProfile, v: i64) -> bool {
    let text = crate::h2star::h2locale::format_number(p, v);
    parse_int(p, &text) == Ok(v)
}

// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

pub fn run_h2numin_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-h2numin");
    let p = crate::h2star::h2locale::DEFAULT;
    // 千分位与空格剥离。
    set.add(
        "h2numin separators",
        parse_int(&p, "1,234,567") == Ok(1234567)
            && parse_int(&p, "1 234 567") == Ok(1234567)
            && parse_int(&p, "1234567") == Ok(1234567),
        "strip , and space",
    );
    // 全角数字/负号归一（中文输入法偏移）。
    set.add(
        "h2numin fullwidth",
        parse_int(&p, "１２３４") == Ok(1234)
            && parse_int(&p, "－５") == Ok(-5)
            && parse_int(&p, "-42") == Ok(-42),
        "IME offsets normalized",
    );
    // 非法字符：指出位置（错误三要素——哪里错）。
    let bad = parse_int(&p, "12O4");
    set.add(
        "h2numin bad char located",
        matches!(bad, Err((2, ref m)) if m.contains("O")),
        "position + human reason",
    );
    // 空输入/纯负号拒绝。
    set.add(
        "h2numin empty refused",
        parse_int(&p, "").is_err() && parse_int(&p, "-").is_err(),
        "nothing to read",
    );
    // 小数：档案 decimal + 全角点。
    let pf = crate::h2star::h2locale::RegionProfile { thousands: '.', decimal: ',', ..p };
    set.add(
        "h2numin float decimal",
        parse_float(&p, "1.5") == Ok(1.5)
            && parse_float(&pf, "1,5") == Ok(1.5)
            && parse_float(&p, "１．５") == Ok(1.5),
        "decimal per profile",
    );
    // round-trip：千分位格式化输出必能读回（正负、多档位数）。
    set.add(
        "h2numin roundtrip",
        [0i64, 1, -1, 999, 1_000, 123_456_789, -9_876_543]
            .iter()
            .all(|v| roundtrip(&p, *v)),
        "format→parse closes",
    );
    // 空格清理位置不偏移：分隔符位置不算非法字符位置。
    let pos = parse_int(&p, "1,2,x");
    set.add(
        "h2numin position after separators",
        matches!(pos, Err((4, _))),
        "index is char index",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h2numin_all_green() {
        let set = run_h2numin_checks();
        assert!(set.all_passed(), "h2numin 自检有红项");
        assert!(!set.truncated(), "h2numin 自检溢出");
    }

    #[test]
    fn paste_garbage_never_panics() {
        // 粘贴垃圾串 fuzz：任意输入只产 Ok/Err，不炸（清洗不变式）。
        let long_nine = "9".repeat(30);
        let garbage = ["，；！abc", "１２３", "-----", "1,2,3,,", "　", long_nine.as_str(), "1.2.3.4"];
        for g in garbage {
            let _ = parse_int(&crate::h2star::h2locale::DEFAULT, g);
            let _ = parse_float(&crate::h2star::h2locale::DEFAULT, g);
        }
    }
}
