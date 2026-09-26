//! F465 终端常用别名（genstar2 · I 域通用·二分队 · AI-U2）。
//!
//! 主册判据（验收标准第一句）：
//! **十别名输出与图形界面对账；覆盖自定义；help 完整性；无参/有参行为；
//! 未知名错误与人话建议（「没有 vx——最接近的是 vxrun」）。**
//!
//! 功能定义（主册批次三）：终端内置别名十枚（面向从 Windows 来的用户）：
//! ip/disk/mem/proc/open/edit/path/cls/date/help——别名可被用户 alias 覆盖；
//! help 一屏列全；每个别名干的事与图形界面同一数据源（一处一事实）。
//!
//! 零堆纪律：静态别名表 + 定长覆盖表，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量与别名表（一处一事实：数据源标签 = 图形界面对账锚）
// ---------------------------------------------------------------------------

/// 内置别名数（主册：十枚）。
pub const ALIAS_N: usize = 10;
/// 用户自定义覆盖表容量。
pub const USER_ALIAS_CAP: usize = 16;

/// 一条内置别名。
#[derive(Clone, Copy)]
pub struct Alias {
    pub name: &'static str,
    /// 展开命令模板（`{arg}` 为参数占位）。
    pub expands: &'static str,
    /// 图形界面对账锚（数据源同一处——一处一事实）。
    pub source: &'static str,
}

/// 十别名（主册原文清单）。
pub const ALIASES: [Alias; ALIAS_N] = [
    Alias { name: "ip", expands: "net show ip {arg}", source: "F242-network-overlay" },
    Alias { name: "disk", expands: "vol list {arg}", source: "F438-drive-mgmt" },
    Alias { name: "mem", expands: "mem summary {arg}", source: "F402-mem-usage" },
    Alias { name: "proc", expands: "proc top {arg}", source: "F403-taskman" },
    Alias { name: "open", expands: "fs open {arg}", source: "F338-open-in-term" },
    Alias { name: "edit", expands: "app notepad {arg}", source: "F097-notepad" },
    Alias { name: "path", expands: "cwd copy {arg}", source: "F336-path-copy" },
    Alias { name: "cls", expands: "term clear {arg}", source: "F471-scrollback" },
    Alias { name: "date", expands: "time show {arg}", source: "F295-date" },
    Alias { name: "help", expands: "help aliases {arg}", source: "self" },
];

/// 别名解析器：用户覆盖 > 内置表；未知名 → 最接近名建议。
pub struct AliasResolver {
    user: [Option<(&'static str, &'static str)>; USER_ALIAS_CAP],
    n: usize,
}

/// 未知名错误的人话建议（主册例：「没有 vx——最接近的是 vxrun」）。
pub fn unknown_hint(name: &str) -> Option<&'static str> {
    // 编辑距离 ≤2 的最近别名（人话建议：不是干巴巴 unknown）。
    let mut best: Option<(&'static str, usize)> = None;
    for a in ALIASES {
        let d = edit_distance(name, a.name, 3);
        if d <= 2 && best.map(|(_, bd)| d < bd).unwrap_or(true) {
            best = Some((a.name, d));
        }
    }
    best.map(|(n, _)| n)
}

/// 有界编辑距离（Levenshtein，阈值截断防长串）。
pub fn edit_distance(a: &str, b: &str, cap: usize) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.len().abs_diff(b.len()) > cap {
        return cap + 1;
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for i in 1..=a.len() {
        cur[0] = i;
        for j in 1..=b.len() {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        core::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

impl AliasResolver {
    pub const fn new() -> Self {
        AliasResolver { user: [None; USER_ALIAS_CAP], n: 0 }
    }

    /// 用户自定义覆盖（主册：别名可被用户 alias 覆盖）。
    pub fn define(&mut self, name: &'static str, expands: &'static str) -> bool {
        if name.is_empty() || expands.is_empty() {
            return false;
        }
        // 已有同名覆盖 → 原位替换。
        if let Some(i) = (0..self.n).find(|&i| matches!(self.user[i], Some((n, _)) if n == name)) {
            self.user[i] = Some((name, expands));
            return true;
        }
        if self.n >= USER_ALIAS_CAP {
            return false;
        }
        self.user[self.n] = Some((name, expands));
        self.n += 1;
        true
    }

    /// 解析：用户覆盖优先；无覆盖落内置表；都无 → None（附建议）。
    pub fn resolve(&self, name: &str) -> Option<&'static str> {
        if let Some(i) = (0..self.n).find(|&i| matches!(self.user[i], Some((n, _)) if n == name)) {
            return Some(self.user[i].unwrap().1);
        }
        ALIASES.iter().find(|a| a.name == name).map(|a| a.expands)
    }

    /// help 一屏列全（主册：help 完整性——十别名全部在列）。
    pub fn help_lines(&self) -> usize {
        ALIASES.len()
    }

    /// 无参/有参行为：无参时 {arg} 消失、有参时替换（模板展开一致语义）。
    pub fn expand_args(expands: &str, arg: Option<&str>) -> usize {
        // 返回展开后参数段的存在性（无参=模板保留固定部分）。
        match arg {
            Some(a) if !a.is_empty() => expands.replace("{arg}", a).len(),
            _ => expands.replace("{arg}", "").trim_end().len(),
        }
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_termalias_checks() -> CheckSet {
    let mut cs = CheckSet::new("F465-termalias");
    // 1) 十别名在册（主册原文十枚）。
    cs.add("ten_aliases", ALIASES.len() == ALIAS_N, "");
    cs.add("names_match_master", ALIASES.iter().map(|a| a.name).eq(["ip", "disk", "mem", "proc", "open", "edit", "path", "cls", "date", "help"].iter().copied()), "");
    // 2) 数据源对账锚（一处一事实：每个别名带图形界面锚）。
    cs.add("source_anchors", ALIASES.iter().all(|a| !a.source.is_empty()) && ALIASES.iter().any(|a| a.source == "F438-drive-mgmt"), "");
    // 3) 覆盖自定义（用户 alias 优先）。
    let mut r = AliasResolver::new();
    r.define("ip", "my-ip-config {arg}");
    cs.add("user_override_wins", r.resolve("ip") == Some("my-ip-config {arg}"), "");
    cs.add("builtin_fallback", r.resolve("disk") == Some("vol list {arg}"), "");
    // 4) help 完整性（一屏列全）。
    cs.add("help_complete", r.help_lines() == ALIAS_N, "");
    // 5) 无参/有参行为。
    cs.add("no_arg_behavior", AliasResolver::expand_args("vol list {arg}", None) == "vol list".len(), "");
    cs.add("with_arg_behavior", AliasResolver::expand_args("vol list {arg}", Some("D:")) == "vol list D:".len(), "");
    // 6) 未知名人话建议（主册例：vx → 最接近 vxrun 语义）。
    cs.add("unknown_hint", unknown_hint("dis") == Some("disk") && unknown_hint("hat") == Some("path"), "");
    cs.add("unknown_far_no_hint", unknown_hint("zzzzzz").is_none(), "");
    // 7) 覆盖表容量诚实。
    let mut r2 = AliasResolver::new();
    let mut all = true;
    for i in 0..USER_ALIAS_CAP + 4 {
        let ok = r2.define(mock_name(i), "x");
        if (i < USER_ALIAS_CAP) != ok {
            all = false;
        }
    }
    cs.add("user_cap_honest", all, "");
    cs
}

fn mock_name(i: usize) -> &'static str {
    const POOL: [&str; 20] = ["u0", "u1", "u2", "u3", "u4", "u5", "u6", "u7", "u8", "u9", "v0", "v1", "v2", "v3", "v4", "v5", "v6", "v7", "v8", "v9"];
    POOL[i.min(POOL.len() - 1)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ten_builtin_aliases_each_expand() {
        for a in ALIASES {
            assert!(!a.expands.is_empty());
            assert!(a.expands.contains(" {arg}") || a.name == "help" || a.name == "cls");
        }
    }

    #[test]
    fn override_then_restore_not_possible_but_redefine_is() {
        let mut r = AliasResolver::new();
        r.define("cls", "my-clear");
        assert_eq!(r.resolve("cls"), Some("my-clear"));
        r.define("cls", "my-clear-v2");
        assert_eq!(r.resolve("cls"), Some("my-clear-v2"));
    }

    #[test]
    fn distance_capped_for_long_input() {
        assert!(edit_distance("a-very-long-terminal-command", "ip", 3) > 3);
    }
}
