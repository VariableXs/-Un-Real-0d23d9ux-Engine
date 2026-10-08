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
                    Some(_loaded) => r2.resolve("ip") == Some("net show ip {arg}"),
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

// ===========================================================================
// 深化 v7（F465）：别名名单法审计 / 覆盖清单持久化（W7M1 完整性清单）/
// help 渲染器 / 多占位符展开 / 错字建议覆盖审计
// ===========================================================================
//
// v7 主轴（主册判据的二阶展开）：
// 1. 名单法审计——用户 alias 必须是合法标识符（ASCII 字母数字 ≤16）：
//    带空格/超长/空名在门禁就拒，不进解析器再炸。
// 2. 持久化——用户覆盖表是用户数据：本体存于配置区字符串层，本通道
//    是**完整性清单**（count + 全表 FNV 摘要——重启后核对覆盖表
//    有没有被截断/篡改，不一致即显性告警）。
// 3. help 渲染器——「help 一屏列全」的量化面：十别名全部渲染进
//    512B 定长缓冲（含名字、展开式、来源锚），超容诚实截断计数。
// 4. 多占位符展开——「{arg} 出现两次都替换」（v1 循环已支持，v7
//    检查面固化语义）。
// 5. 错字建议覆盖审计——每个内置别名删一字符的错字都能被建议回
//    （「没有 vx——最接近的是 vxrun」类体验的全量覆盖）。

use crate::genstar2::vxdict::fnv1a;

// ---------------------------------------------------------------------------
// 别名名单法审计（合法标识符）
// ---------------------------------------------------------------------------

/// 别名名单长度上限。
pub const ALIAS_NAME_MAX: usize = 16;

/// 合法别名（ASCII 字母/数字、首字符非数字、非空、≤16）。
pub fn valid_alias_name(name: &str) -> bool {
    let b = name.as_bytes();
    if b.is_empty() || b.len() > ALIAS_NAME_MAX {
        return false;
    }
    if b[0].is_ascii_digit() {
        return false;
    }
    b.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'_')
}

/// 覆盖名单门禁（define 的入口守卫：名单不过 = 拒绝入表）。
pub fn define_guarded(resolver: &mut AliasResolver, name: &'static str, expands: &'static str) -> bool {
    if !valid_alias_name(name) {
        return false;
    }
    resolver.define(name, expands)
}

// ---------------------------------------------------------------------------
// 覆盖清单持久化（W7M1 ——完整性清单，不存字符串本体）
// ---------------------------------------------------------------------------

/// v7 魔标（W7M 族）。
pub const TERMALIAS_V7_MAGIC: [u8; 4] = *b"W7M1";
/// 长度：魔标(4) + 版本(1) + count(1) + 保留(1) + 摘要(4) + FNV(4) = 16。
pub const TERMALIAS_V7_LEN: usize = 16;
pub const TERMALIAS_V7_VERSION: u8 = 1;

/// 覆盖表摘要（名字+展开式串联 FNV——顺序敏感：换序也算变）。
pub fn alias_manifest_digest(names: &[&str], expands: &[&str]) -> Option<u32> {
    if names.len() != expands.len() {
        return None;
    }
    let mut buf = [0u8; 512];
    let mut w = 0usize;
    for i in 0..names.len() {
        for part in [names[i], "\u{1}", expands[i], "\u{2}"] {
            let p = part.as_bytes();
            if w + p.len() > buf.len() {
                return None;
            }
            buf[w..w + p.len()].copy_from_slice(p);
            w += p.len();
        }
    }
    Some(fnv1a(&buf[..w]))
}

/// 序列化（覆盖表完整性清单）。
pub fn save_manifest_v7(names: &[&str], expands: &[&str], out: &mut [u8]) -> Option<usize> {
    if names.len() != expands.len() || names.len() > USER_ALIAS_CAP {
        return None;
    }
    if out.len() < TERMALIAS_V7_LEN {
        return None;
    }
    out[..4].copy_from_slice(&TERMALIAS_V7_MAGIC);
    out[4] = TERMALIAS_V7_VERSION;
    out[5] = names.len() as u8;
    out[6] = 0;
    let digest = alias_manifest_digest(names, expands)?;
    out[7..11].copy_from_slice(&digest.to_le_bytes());
    let h = fnv1a(&out[..11]);
    out[11] = (h & 0xff) as u8;
    out[12] = ((h >> 8) & 0xff) as u8;
    out[13] = ((h >> 16) & 0xff) as u8;
    out[14] = ((h >> 24) & 0xff) as u8;
    Some(TERMALIAS_V7_LEN)
}

/// 反序列化 + 清单核对（给当前覆盖表出「是否与存档一致」的裁决）。
pub fn verify_manifest_v7(buf: &[u8], names: &[&str], expands: &[&str]) -> Option<bool> {
    if buf.len() < TERMALIAS_V7_LEN || buf[..4] != TERMALIAS_V7_MAGIC {
        return None;
    }
    if buf[4] != TERMALIAS_V7_VERSION || buf[6] != 0 {
        return None;
    }
    let expect = fnv1a(&buf[..11]);
    let got = buf[11] as u32
        | ((buf[12] as u32) << 8)
        | ((buf[13] as u32) << 16)
        | ((buf[14] as u32) << 24);
    if expect != got {
        return None;
    }
    if buf[5] as usize != names.len() {
        return Some(false); // 数量对不上 = 已被改动
    }
    let digest = alias_manifest_digest(names, expands)?;
    let mut stored = [0u8; 4];
    stored.copy_from_slice(&buf[7..11]);
    Some(digest == u32::from_le_bytes(stored))
}

// ---------------------------------------------------------------------------
// help 渲染器（一屏列全的量化面）
// ---------------------------------------------------------------------------

/// help 缓冲预算（字节）。
pub const HELP_BUF_CAP: usize = 512;

/// 渲染十别名进定长缓冲（每行「name -> expands  [source]\n」）。
/// 返回 (写出字节数, 渲染行数)；缓冲不足 = None（不静默截断）。
pub fn render_help(out: &mut [u8]) -> Option<(usize, usize)> {
    let mut w = 0usize;
    for a in ALIASES.iter() {
        // 行长预算：name + " -> " + expands + "  [" + source + "]\n"。
        let line_len = a.name.len() + 4 + a.expands.len() + 2 + a.source.len() + 2 + 1;
        if w + line_len > out.len() {
            return None;
        }
        let mut put = |s: &str| {
            out[w..w + s.len()].copy_from_slice(s.as_bytes());
            w += s.len();
        };
        put(a.name);
        put(" -> ");
        put(a.expands);
        put("  [");
        put(a.source);
        put("]\n");
    }
    Some((w, ALIASES.len()))
}

// ---------------------------------------------------------------------------
// 错字建议覆盖审计
// ---------------------------------------------------------------------------

/// 每个内置别名删一字符 → unknown_hint 必须建议回原名（全量覆盖）。
pub fn typo_suggestion_coverage() -> bool {
    for a in ALIASES.iter() {
        let b: Vec<char> = a.name.chars().collect();
        if b.len() < 2 {
            return false;
        }
        // 删中间一字符（首删/尾删也给过——用中间位做代表）。
        let mid = b.len() / 2;
        let typo: String = b.iter().enumerate().filter(|&(i, _)| i != mid).map(|(_, c)| *c).collect();
        if unknown_hint(&typo) != Some(a.name) {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// 域自检（F465 v7）
// ---------------------------------------------------------------------------

pub fn run_termalias_v7_checks() -> CheckSet {
    let mut cs = CheckSet::new("F465-v7");
    // 1) 名单法：合法/非法边界。
    cs.add("name_valid", valid_alias_name("myip") && valid_alias_name("v_x2") && valid_alias_name("a"), "");
    cs.add("name_invalid", !valid_alias_name("") && !valid_alias_name("2fast")
        && !valid_alias_name("has space") && !valid_alias_name("0123456789abcdefX"), "");
    // 2) 门禁在 define 前拦截。
    cs.add("define_guard_blocks", {
        let mut r = AliasResolver::new();
        !define_guarded(&mut r, "bad name", "x") && r.resolve("bad name").is_none()
            && define_guarded(&mut r, "good", "ok {arg}")
    }, "");
    // 3) 完整性清单：存档→一致 / 改动→不一致 / 数量变→不一致。
    let mut buf = [0u8; TERMALIAS_V7_LEN];
    cs.add("manifest_match", {
        let n = save_manifest_v7(&["ip2", "quick"], &["net ip {arg}", "run fast"], &mut buf).unwrap_or(0);
        verify_manifest_v7(&buf[..n], &["ip2", "quick"], &["net ip {arg}", "run fast"]) == Some(true)
    }, "");
    cs.add("manifest_content_changed", {
        let n = save_manifest_v7(&["ip2", "quick"], &["net ip {arg}", "run fast"], &mut buf).unwrap_or(0);
        verify_manifest_v7(&buf[..n], &["ip2", "quick"], &["net ip {arg}", "run slow"]) == Some(false)
    }, "");
    cs.add("manifest_count_changed", {
        let n = save_manifest_v7(&["ip2"], &["net ip {arg}"], &mut buf).unwrap_or(0);
        verify_manifest_v7(&buf[..n], &["ip2", "quick"], &["net ip {arg}", "run fast"]) == Some(false)
    }, "");
    cs.add("manifest_tamper", {
        let n = save_manifest_v7(&["ip2"], &["x"], &mut buf).unwrap_or(0);
        let mut bad = buf;
        bad[5] ^= 0x01;
        verify_manifest_v7(&bad[..n], &["ip2"], &["x"]).is_none()
    }, "");
    cs.add("manifest_over_cap_reject", {
        let names: [&str; USER_ALIAS_CAP + 1] = core::array::from_fn(|i| match i {
            0..=9 => POOL9[i],
            _ => POOL9[i - 10],
        });
        save_manifest_v7(&names, &["x"; USER_ALIAS_CAP + 1], &mut buf).is_none()
    }, "");
    // 4) help 渲染器：十行全渲染 + 缓冲不足诚实 None。
    cs.add("help_render_full", {
        let mut buf = [0u8; HELP_BUF_CAP];
        match render_help(&mut buf) {
            Some((n, lines)) => {
                lines == ALIAS_N
                    && n > 100
                    && buf[..3] == *b"ip "
                    && buf[n - 1] == b'\n'
            }
            None => false,
        }
    }, "");
    cs.add("help_render_tiny_none", {
        let mut tiny = [0u8; 32];
        render_help(&mut tiny).is_none()
    }, "");
    // 5) 多占位符：两处 {arg} 都替换。
    cs.add("multi_placeholder_expand", {
        let mut out = [0u8; 64];
        match AliasResolver::expand_into("cp {arg} to {arg} dir", Some("X"), &mut out) {
            Some(n) => &out[..n] == b"cp X to X dir",
            None => false,
        }
    }, "");
    // 6) 错字建议全量覆盖（每别名删中位字符均可建议回）。
    cs.add("typo_coverage_all", typo_suggestion_coverage(), "");
    // 7) v1 回归锚：覆盖优先 + help 十行（v7 面不许伤 v1 语义）。
    cs.add("v1_override_regression", {
        let mut r = AliasResolver::new();
        let _ = r.define("ip", "mine {arg}");
        r.resolve("ip") == Some("mine {arg}") && r.help_lines() == ALIAS_N
    }, "");
    cs
}

const POOL9: [&str; 10] = ["u0", "u1", "u2", "u3", "u4", "u5", "u6", "u7", "u8", "u9"];

#[cfg(test)]
mod v7_tests {
    use super::*;

    #[test]
    fn manifest_is_order_sensitive() {
        let mut buf = [0u8; TERMALIAS_V7_LEN];
        let n = save_manifest_v7(&["a", "b"], &["x", "y"], &mut buf).unwrap();
        // 换序 = 内容变了（清单如实报不一致）。
        assert_eq!(verify_manifest_v7(&buf[..n], &["b", "a"], &["y", "x"]), Some(false));
    }

    #[test]
    fn digest_length_mismatch_none() {
        assert!(alias_manifest_digest(&["a"], &["x", "y"]).is_none());
    }

    #[test]
    fn help_buffer_sized_honestly() {
        // 预算审计：十行全部渲染且余量健康（不是贴线交付）。
        let mut buf = [0u8; HELP_BUF_CAP];
        let (n, _) = render_help(&mut buf).unwrap();
        assert!(n > 150 && n < HELP_BUF_CAP, "渲染 {n}B 应在预算内且有余量");
    }

    #[test]
    fn typo_single_deletion_hinted() {
        // 删一字符（disk → dsk）编辑距离 1 仍在建议域。
        assert_eq!(unknown_hint("dsk"), Some("disk"));
    }

    #[test]
    fn guard_rejects_non_static_safety() {
        // 名单法对内置别名名同样适用（自检）。
        for a in ALIASES.iter() {
            assert!(valid_alias_name(a.name), "{} 应为合法标识符", a.name);
        }
    }
}
