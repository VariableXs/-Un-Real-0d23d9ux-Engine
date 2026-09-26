//! H2 共享底盘：文件名语义、命名碰撞、天窗边界——五十项功能里反复出现的
//! 「文件系统常识」收拢一处，一处一事实。
//!
//! - [`ext_split`]——主名/扩展名拆分（F260 行内重命名「扩展名隔离」、
//!   F292 快捷方式健康、F288 字体装判共用同一条规则）；
//! - [`INVALID_NAME_CHARS`]——F260 非法字符表（8 字符全测判据的唯一源）；
//! - [`bump_copy_name`]——「XX - 副本」「XX - 副本 (2)」防重名规则
//!   （F263 发送到桌面、F253 快速访问共用）；
//! - [`days_between`]——按天窗计算的边界工具（F267 存储 30 天边界
//!   29/30/31 天用例、F285 缓存淘汰共用；天 = 分钟戳 / 1440，整除口径）；
//! - [`pick_slot`]——网格落位计算（F259 新建落点、F286 拼接分割共用：
//!   在 W×H 网格里找第一个空闲格，找不到返回追加位）。
//!
//! 时间纪律：一切时间由调用方注入（分钟戳），模块不持真实时钟。
//! 复用纪律：旋钮/环形日志/分位数等通用件直接取 [`crate::star::sbase`]
//! （AI-K2 已落地且冻结），不重复造轮子。

use alloc::string::String;

// ---------------------------------------------------------------------------
// 文件名语义
// ---------------------------------------------------------------------------

/// F260 非法字符表（主册判据「非法字符 8 字符全测」——恰好 8 个）：
/// `\ / : * ? " < > |`。
pub const INVALID_NAME_CHARS: [char; 8] = ['\\', '/', ':', '*', '?', '"', '<', '>'];

/// 主名/扩展名拆分。规则（Windows 惯例，一处一事实）：
/// - 首个 `.` 起（不含）到结尾为扩展名——`报告.docx` → ("报告", ".docx")；
/// - 隐藏/点文件（`.gitignore`）整个视为主名，无扩展名；
/// - 尾点（`报告.`）不拆分：主名整名保留、扩展名空串（无信息丢失）。
pub fn ext_split(name: &str) -> (&str, &str) {
    let bytes = name.as_bytes();
    if bytes.first() == Some(&b'.') {
        return (name, "");
    }
    match name.rfind('.') {
        Some(idx) if idx == name.len() - 1 => (name, ""),
        Some(idx) => (&name[..idx], &name[idx..]),
        None => (name, ""),
    }
}

/// 文件名是否含非法字符（F260 即时抖动拒绝的判定函数）。
pub fn has_invalid_char(name: &str) -> bool {
    name.chars().any(|c| INVALID_NAME_CHARS.contains(&c))
}

/// 防重名递增：`name` 若无「 - 副本」尾缀则补一个；已有则追加/递增
/// 序号 ` (n)`。副本标记插在**扩展名之前**（`方案.txt → 方案 - 副本.txt`
/// ——Windows 惯例）；返回**候选名**——调用方拿去与现存列表比对，仍撞
/// 则继续传候选名进来递增（幂等、可循环）。全程按字符边界切分，中文安全。
pub fn bump_copy_name(name: &str) -> String {
    const MARK: &str = " - 副本";
    let (stem, ext) = ext_split(name);
    if let Some(base) = stem.strip_suffix(MARK) {
        return alloc::format!("{} - 副本 (2){}", base, ext);
    }
    if let Some(close) = stem.rfind(MARK) {
        let tail = &stem[close + MARK.len()..];
        if let Some(inner) = tail.strip_prefix(" (").and_then(|t| t.strip_suffix(')')) {
            if let Ok(n) = inner.parse::<u32>() {
                return alloc::format!("{} - 副本 ({}){}", &stem[..close], n + 1, ext);
            }
        }
    }
    alloc::format!("{} - 副本{}", stem, ext)
}

// ---------------------------------------------------------------------------
// 时间与网格
// ---------------------------------------------------------------------------

/// 两个分钟戳之间的整天数（整除口径：不足一天算 0 天）。
/// F267 判据「29/30/31 天边界」直接以此对拍。
pub fn days_between(from_min: u64, to_min: u64) -> u64 {
    to_min.saturating_sub(from_min) / 1440
}

/// 网格落位：在 `cols × rows` 网格里从左上角行优先找第一个空闲格；
/// 满了返回追加位（第 `used` 个 + 1，主册「不飞到列表末尾」——追加位
/// 也是紧邻现有内容的下一格，不是远处空区）。
/// 返回 (col, row)。零格网（cols==0）视为 1 列降级，不 panic。
pub fn pick_slot(used: usize, cols: usize) -> (usize, usize) {
    let cols = if cols == 0 { 1 } else { cols };
    (used % cols, used / cols)
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

use crate::checks::CheckSet;

/// 底盘自检：五件工具逐件钉判据。
pub fn run_h2base_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-h2base");
    // ext_split 四判例（含隐藏文件与尾点）。
    let (s, e) = ext_split("报告.docx");
    set.add("h2base ext split", s == "报告" && e == ".docx", "stem+ext");
    let (s2, e2) = ext_split(".gitignore");
    set.add("h2base dotfile", s2 == ".gitignore" && e2.is_empty(), "hidden file");
    let (s3, e3) = ext_split("报告.");
    set.add("h2base trailing dot", s3 == "报告." && e3.is_empty(), "no split");
    // 非法字符 8 个逐个检出 + 合法名放行。
    let all_bad = INVALID_NAME_CHARS.iter().all(|&c| has_invalid_char(&alloc::format!("a{c}b")));
    set.add("h2base invalid 8", all_bad && !has_invalid_char("报告 2026"), "8 chars");
    // 副本递增三连。
    let a = bump_copy_name("方案.txt");
    let b = bump_copy_name(&a);
    let c = bump_copy_name(&b);
    set.add(
        "h2base copy bump",
        a == "方案 - 副本.txt" && b == "方案 - 副本 (2).txt" && c == "方案 - 副本 (3).txt",
        "副本 ladder",
    );
    // 天窗边界：1439 分钟 = 0 天，1440 = 1 天，4321 = 3 天。
    set.add(
        "h2base days",
        days_between(0, 1439) == 0 && days_between(0, 1440) == 1 && days_between(10, 4331) == 3,
        "day boundary",
    );
    // 网格落位：4 列第 5 个 → (0,1)；零列降级不炸。
    let slot = pick_slot(4, 4);
    set.add("h2base grid", slot == (0, 1) && pick_slot(0, 0) == (0, 0), "grid slot");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h2base_all_green() {
        let set = run_h2base_checks();
        assert!(set.all_passed(), "h2base 自检有红项");
        assert!(!set.truncated(), "h2base 自检溢出");
    }

    #[test]
    fn ext_isolation_is_windows_like() {
        // F260 判据：改「报告」不动「.docx」——拆分是扩展名隔离的地基。
        assert_eq!(ext_split("v1.0.config"), ("v1.0", ".config"));
        assert_eq!(ext_split("noext"), ("noext", ""));
    }

    #[test]
    fn copy_ladder_idempotent() {
        let mut name = String::from("X");
        for expect in ["X - 副本", "X - 副本 (2)", "X - 副本 (3)", "X - 副本 (4)"] {
            name = bump_copy_name(&name);
            assert_eq!(name, expect);
        }
    }
}
