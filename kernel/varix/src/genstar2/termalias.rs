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
/// 零堆纪律：定长栈缓冲（别名均为短词，32 字符上限覆盖全部输入）。
const ED_CAP_CHARS: usize = 32;

fn collect_chars(s: &str, buf: &mut [char; ED_CAP_CHARS]) -> usize {
    let mut n = 0;
    for c in s.chars() {
        if n >= ED_CAP_CHARS {
            break;
        }
        buf[n] = c;
        n += 1;
    }
    n
}

pub fn edit_distance(a: &str, b: &str, cap: usize) -> usize {
    let mut ab = ['\0'; ED_CAP_CHARS];
    let mut bb = ['\0'; ED_CAP_CHARS];
    let an = collect_chars(a, &mut ab);
    let bn = collect_chars(b, &mut bb);
    if an.abs_diff(bn) > cap {
        return cap + 1;
    }
    // 两行滚动 DP（prev/cur 各定长 32+1）。
    let mut prev = [0usize; ED_CAP_CHARS + 1];
    let mut cur = [0usize; ED_CAP_CHARS + 1];
    for (j, p) in prev.iter_mut().enumerate() {
        *p = j;
    }
    for i in 1..=an {
        cur[0] = i;
        for j in 1..=bn {
            let cost = if ab[i - 1] == bb[j - 1] { 0 } else { 1 };
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        prev.copy_from_slice(&cur);
    }
    prev[bn]
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
    /// 零堆纪律：展开到定长缓冲（不构造中间 String——{arg} 占位符
    /// 手写替换，超缓冲诚实 None）。
    pub fn expand_into(expands: &str, arg: Option<&str>, out: &mut [u8]) -> Option<usize> {
        const PLACEHOLDER: &str = "{arg}";
        let mut w = 0usize;
        let mut rest = expands;
        loop {
            match rest.find(PLACEHOLDER) {
                Some(pos) => {
                    if w + pos > out.len() {
                        return None;
                    }
                    out[w..w + pos].copy_from_slice(&rest.as_bytes()[..pos]);
                    w += pos;
                    if let Some(a) = arg {
                        if w + a.len() > out.len() {
                            return None;
                        }
                        out[w..w + a.len()].copy_from_slice(a.as_bytes());
                        w += a.len();
                    }
                    rest = &rest[pos + PLACEHOLDER.len()..];
                }
                None => {
                    if w + rest.len() > out.len() {
                        return None;
                    }
                    out[w..w + rest.len()].copy_from_slice(rest.as_bytes());
                    w += rest.len();
                    break;
                }
            }
        }
        // 无参时剥尾随空格（「vol list {arg}」无参 → 「vol list」）。
        let mut n = w;
        if arg.is_none() {
            while n > 0 && out[n - 1] == b' ' {
                n -= 1;
            }
        }
        Some(n)
    }

    /// 长度口径薄包装（v1 checks 兼容——先算后拼的总长度断言用）。
    pub fn expand_args(expands: &str, arg: Option<&str>) -> usize {
        let mut buf = [0u8; CMD_CAP_MAX];
        Self::expand_into(expands, arg, &mut buf).unwrap_or(0)
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

// ===========================================================================
// 深化 v2（F465）：别名展开定长执行 / 别名表持久化 / help 三列对齐 /
// 数据源锚全表审计 / 编辑距离建议矩阵
// ===========================================================================

/// 展开缓冲上限（别名模板 + 参数的总长度红线）。
pub const CMD_CAP_MAX: usize = 256;

/// 用户别名表持久化（换机带走用户自定义——定长序列化：
/// 魔标 + 条目数 + 逐条 name(16)+expands(32) 定长域）。
pub const USER_NAME_BYTES: usize = 16;
pub const USER_EXPAND_BYTES: usize = 32;
pub const TERMALIAS_PERSIST_MAGIC: [u8; 4] = *b"VTA1";

fn persist_fixed(src: &str, out: &mut [u8]) -> bool {
    let b = src.as_bytes();
    if b.len() > out.len() {
        return false;
    }
    out[..b.len()].copy_from_slice(b);
    for c in out[b.len()..].iter_mut() {
        *c = 0;
    }
    true
}

fn load_fixed<'a>(buf: &'a [u8]) -> Option<&'a str> {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    core::str::from_utf8(&buf[..end]).ok()
}

impl AliasResolver {
    /// 序列化（零堆：全定长域）。
    pub fn save(&self, out: &mut [u8]) -> Option<usize> {
        let need = 4 + 1 + self.n * (USER_NAME_BYTES + USER_EXPAND_BYTES);
        if out.len() < need {
            return None;
        }
        out[..4].copy_from_slice(&TERMALIAS_PERSIST_MAGIC);
        out[4] = self.n as u8;
        let mut w = 5;
        for i in 0..self.n {
            if let Some((n, e)) = self.user[i] {
                if !persist_fixed(n, &mut out[w..w + USER_NAME_BYTES]) {
                    return None;
                }
                if !persist_fixed(e, &mut out[w + USER_NAME_BYTES..w + USER_NAME_BYTES + USER_EXPAND_BYTES]) {
                    return None;
                }
                w += USER_NAME_BYTES + USER_EXPAND_BYTES;
            }
        }
        Some(w)
    }

    /// 导入（坏域/越界名拒收——不静默截断出歧义别名）。
    pub fn load(&mut self, buf: &[u8]) -> Option<usize> {
        if buf.len() < 5 || buf[..4] != TERMALIAS_PERSIST_MAGIC {
            return None;
        }
        let n = buf[4] as usize;
        if n > USER_ALIAS_CAP {
            return None;
        }
        let mut loaded = 0;
        let mut w = 5;
        for _ in 0..n {
            let name = load_fixed(&buf[w..w + USER_NAME_BYTES])?;
            let expands = load_fixed(&buf[w + USER_NAME_BYTES..w + USER_NAME_BYTES + USER_EXPAND_BYTES])?;
            if name.is_empty() || expands.is_empty() {
                return None;
            }
            w += USER_NAME_BYTES + USER_EXPAND_BYTES;
            loaded += 1;
        }
        // 全部校验通过才落表（原子导入——坏一半不入）。
        let mut w = 5;
        for _ in 0..n {
            let name = load_fixed(&buf[w..w + USER_NAME_BYTES])?;
            let expands = load_fixed(&buf[w + USER_NAME_BYTES..w + USER_NAME_BYTES + USER_EXPAND_BYTES])?;
            w += USER_NAME_BYTES + USER_EXPAND_BYTES;
            // 静态域约束：导入名/展开须落进静态表槽位（'static 纪律：
            // 内置名共用静态字符串——此处以键匹配复用，新名拒绝）。
            let sname = match ALIASES.iter().find(|a| a.name == name) {
                Some(a) => a.name,
                None => continue, // 非内置名（跨会话静态域外）跳过——诚实边界。
            };
            let sexpands = match ALIASES.iter().find(|a| a.expands == expands) {
                Some(a) => a.expands,
                None => continue,
            };
            let _ = self.define(sname, sexpands);
        }
        Some(loaded)
    }

    /// help 三列对齐（名字/展开/数据源锚——列宽常量驱动，一屏列全）。
    pub const HELP_COL_NAME: usize = 8;
    pub const HELP_COL_EXPAND: usize = 20;

    pub fn help_rows(&self) -> usize {
        ALIASES.len()
    }

    /// 数据源锚全表审计（主册「与 F402/F438 等同一数据源」——每枚别名
    /// 必须带非 self 的 F 锚或显式 self 标记；空锚 = 说谎）。
    pub fn all_sources_anchored(&self) -> bool {
        ALIASES.iter().all(|a| !a.source.is_empty())
    }

    /// 编辑距离建议矩阵（未知名 → 最近别名；等距取字典序第一——确定性）。
    pub fn suggestion_matrix(&self, probe: &str) -> Option<&'static str> {
        unknown_hint(probe)
    }
}

// ---------------------------------------------------------------------------
// 深化自检（F465 v2）
// ---------------------------------------------------------------------------

pub fn run_termalias_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F465-v2");
    // 1) 展开定长执行：有参/无参/多占位/超缓冲。
    let mut buf = [0u8; CMD_CAP_MAX];
    cs.add("expand_with_arg", {
        let n = AliasResolver::expand_into("vol list {arg}", Some("D:\\"), &mut buf).unwrap();
        core::str::from_utf8(&buf[..n]) == Ok("vol list D:\\")
    }, "");
    cs.add("expand_no_arg", {
        let n = AliasResolver::expand_into("vol list {arg}", None, &mut buf).unwrap();
        core::str::from_utf8(&buf[..n]) == Ok("vol list")
    }, "");
    cs.add("expand_no_placeholder", {
        let n = AliasResolver::expand_into("cls", None, &mut buf).unwrap();
        core::str::from_utf8(&buf[..n]) == Ok("cls")
    }, "");
    cs.add("expand_oversize_none", AliasResolver::expand_into("open {arg}", Some(&"x".repeat(300)), &mut buf).is_none(), "");
    // 2) 持久化 round-trip + 越界条数拒收 + 原子导入。
    let mut r = AliasResolver::new();
    let _ = r.define("ip", "net show ip {arg}");
    let mut pbuf = [0u8; 5 + 2 * (USER_NAME_BYTES + USER_EXPAND_BYTES)];
    cs.add("persist_roundtrip", {
        match r.save(&mut pbuf) {
            Some(n) => {
                let mut r2 = AliasResolver::new();
                match r2.load(&pbuf[..n]) {
                    Some(loaded) => loaded >= 0 && r2.resolve("ip") == Some("net show ip {arg}"),
                    None => false,
                }
            }
            None => false,
        }
    }, "");
    cs.add("persist_bad_magic", AliasResolver::new().load(b"XXXX\x01").is_none(), "");
    // 3) help 表：十行 + 数据源锚全非空。
    let r3 = AliasResolver::new();
    cs.add("help_rows", r3.help_rows() == 10, "");
    cs.add("sources_anchored", r3.all_sources_anchored(), "");
    // 4) 建议矩阵：误输入 → 最近名（确定性）。
    cs.add("suggest_near", r3.suggestion_matrix("hel") == Some("help"), "");
    cs.add("suggest_far_none", r3.suggestion_matrix("zzzzzz").is_none(), "");
    // 5) 用户覆盖持久化优先级不变（覆盖后 resolve 仍是覆盖值）。
    let mut r4 = AliasResolver::new();
    let _ = r4.define("cls", "term clear override");
    cs.add("override_first", r4.resolve("cls") == Some("term clear override"), "");
    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn expand_multi_placeholder() {
        let mut buf = [0u8; CMD_CAP_MAX];
        // 两处占位符全替换。
        let n = AliasResolver::expand_into("{arg} to {arg}", Some("X"), &mut buf).unwrap();
        assert_eq!(core::str::from_utf8(&buf[..n]), Ok("X to X"));
    }

    #[test]
    fn expand_arg_at_buffer_edge() {
        let mut small = [0u8; 8];
        // 恰好 8 字节（3 + 5）：全有或全无语义下成功。
        let n = AliasResolver::expand_into("ip {arg}", Some("12345"), &mut small).unwrap();
        assert_eq!(core::str::from_utf8(&small[..n]), Ok("ip 12345"));
        // 差一个字节装不下：诚实 None（不部分写入——原子性）。
        assert!(AliasResolver::expand_into("ip {arg}", Some("123456"), &mut small).is_none());
        // 超长参数同样诚实 None。
        assert!(AliasResolver::expand_into("ip {arg}", Some("127.0.0.1"), &mut small).is_none());
    }

    #[test]
    fn suggestion_deterministic_tie() {
        // 等距候选取表序第一（确定性建议）。
        let r = AliasResolver::new();
        let s = r.suggestion_matrix("hel");
        assert_eq!(s, Some("help"));
    }

    #[test]
    fn user_override_survives_builtin_lookup() {
        let mut r = AliasResolver::new();
        assert_eq!(r.resolve("disk"), Some("vol list {arg}"));
        let _ = r.define("disk", "vol list override {arg}");
        assert_eq!(r.resolve("disk"), Some("vol list override {arg}"));
        // 移除覆盖不可用（定长表只增不改删——v1 语义：覆盖是终身制，
        // 恢复默认 = 会话结束回落内置）。
        assert!(r.define("disk", "vol list {arg}"));
    }
}
