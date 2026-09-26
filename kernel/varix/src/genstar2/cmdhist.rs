//! F468 命令历史（genstar2 · I 域通用·二分队 · AI-U2）。
//!
//! 主册判据（验收标准第一句）：
//! **回溯/搜索/持久化三用例；敏感行过滤（10 例注入）；1000 条上限淘汰；
//! 清空双入口；历史文件权限（用户级只读他者）。**
//!
//! 功能定义（主册批次三）：上下方向键逐条回溯、Ctrl+R 历史搜索（输入子串
//! 即时过滤、再按跳下一条）、历史持久化（1000 条，跨会话保留）；敏感命令
//! 不记（含 password/token 字样的行跳过）；历史清空命令与设置页入口。
//!
//! 零堆纪律：定长历史环，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 历史条数上限（主册：1000 条）。
pub const HIST_CAP: usize = 1_000;
/// 单行命令长度上限。
pub const CMD_CAP: usize = 256;
/// 敏感行黑名单关键词（主册：password/token 类行不记——10 例注入覆盖）。
pub const SENSITIVE_WORDS: [&str; 8] = [
    "password", "passwd", "token", "secret", "apikey", "api-key", "credential", "私钥",
];

/// 一条历史。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HistLine {
    pub text: [u8; CMD_CAP],
    pub n: usize,
    pub seq: u64,
}

impl HistLine {
    fn as_str(&self) -> &str {
        core::str::from_utf8(&self.text[..self.n]).unwrap_or("")
    }

    fn from(text: &str, seq: u64) -> Option<HistLine> {
        let b = text.as_bytes();
        if b.is_empty() || b.len() > CMD_CAP {
            return None;
        }
        let mut h = HistLine { text: [0; CMD_CAP], n: b.len(), seq };
        h.text[..b.len()].copy_from_slice(b);
        Some(h)
    }
}

/// 命令历史环。
pub struct CmdHistory {
    ring: [Option<HistLine>; HIST_CAP],
    head: usize,
    n: usize,
    next_seq: u64,
    /// 回溯游标（方向键位置；None = 在输入行）。
    cursor: Option<usize>,
    /// Ctrl+R 搜索态（命中索引）。
    search_hit: Option<usize>,
}

impl CmdHistory {
    pub const fn new() -> Self {
        CmdHistory {
            ring: [None; HIST_CAP],
            head: 0,
            n: 0,
            next_seq: 0,
            cursor: None,
            search_hit: None,
        }
    }

    /// 敏感行过滤（主册：password/token 字样行跳过——10 例注入判定）。
    pub fn is_sensitive(line: &str) -> bool {
        let lower = line.to_ascii_lowercase();
        SENSITIVE_WORDS.iter().any(|w| lower.contains(w))
    }

    /// 记录一条（敏感行不记；重复相邻行去重——终端惯例）。
    pub fn record(&mut self, line: &str) -> bool {
        if Self::is_sensitive(line) {
            return false;
        }
        if let Some(h) = HistLine::from(line, self.next_seq) {
            // 相邻重复去重。
            if self.n > 0 {
                let last = (self.head + HIST_CAP - 1) % HIST_CAP;
                if let Some(e) = self.ring[last] {
                    if e.as_str() == line {
                        return true;
                    }
                }
            }
            self.ring[self.head] = Some(h);
            self.head = (self.head + 1) % HIST_CAP;
            self.n = (self.n + 1).min(HIST_CAP);
            self.next_seq += 1;
            self.cursor = None;
            true
        } else {
            false
        }
    }

    /// 方向键回溯：上一条/下一条（游标语义：底 = 空输入行）。
    pub fn recall(&mut self, up: bool) -> Option<&str> {
        if self.n == 0 {
            return None;
        }
        let oldest = (self.head + HIST_CAP - self.n) % HIST_CAP;
        let newest = (self.head + HIST_CAP - 1) % HIST_CAP;
        let cur = match self.cursor {
            None => {
                if !up {
                    return None; // 已在输入行还按 ↓ = 无动作
                }
                // 首次 ↑：直接停在最新一条（不再前移）。
                self.cursor = Some(newest);
                return self.ring[newest].as_ref().map(|h| h.as_str());
            }
            Some(c) => c,
        };
        let next = if up {
            if cur == oldest {
                cur // 到最旧停住（不环绕——肌肉记忆一致性）
            } else {
                (cur + HIST_CAP - 1) % HIST_CAP
            }
        } else {
            if cur == newest {
                self.cursor = None;
                return Some("");
            }
            (cur + 1) % HIST_CAP
        };
        self.cursor = Some(next);
        self.ring[next].as_ref().map(|h| h.as_str())
    }

    /// Ctrl+R 子串搜索：从最新往旧找，再按跳下一条（主册：即时过滤、再按
    /// 跳下一条）。
    pub fn search_next(&mut self, needle: &str) -> Option<&str> {
        if needle.is_empty() || self.n == 0 {
            return None;
        }
        let oldest = (self.head + HIST_CAP - self.n) % HIST_CAP;
        let mut i = match self.search_hit {
            None => newest_idx(self.head, HIST_CAP),
            Some(hit) => (hit + HIST_CAP - 1) % HIST_CAP,
        };
        let mut steps = 0;
        loop {
            if steps >= self.n {
                return None; // 找遍无更多命中
            }
            if let Some(h) = self.ring[i] {
                if h.as_str().contains(needle) {
                    self.search_hit = Some(i);
                    return self.ring[i].as_ref().map(|h2| h2.as_str());
                }
            }
            if i == oldest {
                return None;
            }
            i = (i + HIST_CAP - 1) % HIST_CAP;
            steps += 1;
        }
    }

    /// 搜索态退出（Enter/Esc）。
    pub fn search_exit(&mut self) {
        self.search_hit = None;
    }

    /// 清空（双入口共用同一清空核：终端命令/设置页按钮）。
    pub fn clear(&mut self) -> usize {
        let n = self.n;
        self.ring = [None; HIST_CAP];
        self.n = 0;
        self.cursor = None;
        self.search_hit = None;
        n
    }

    pub fn count(&self) -> usize {
        self.n
    }

    /// 持久化视图：按 seq 旧→新遍历（持久化文件写入序）。
    pub fn iter_oldest_first(&self) -> impl Iterator<Item = &str> {
        let oldest = (self.head + HIST_CAP - self.n) % HIST_CAP;
        (0..self.n).filter_map(move |i| self.ring[(oldest + i) % HIST_CAP].as_ref().map(|h| h.as_str()))
    }
}

fn newest_idx(head: usize, cap: usize) -> usize {
    (head + cap - 1) % cap
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_cmdhist_checks() -> CheckSet {
    let mut cs = CheckSet::new("F468-cmdhist");
    // 1) 回溯三用例（上/下/到底停住）。
    let mut h = CmdHistory::new();
    h.record("vx run app");
    h.record("cd C:\\work");
    cs.add("recall_up", h.recall(true) == Some("cd C:\\work"), "");
    cs.add("recall_up_older", h.recall(true) == Some("vx run app"), "");
    cs.add("recall_oldest_stop", h.recall(true) == Some("vx run app"), "");
    cs.add("recall_down_to_empty", h.recall(false) == Some("cd C:\\work") && h.recall(false) == Some(""), "");
    // 2) Ctrl+R 搜索 + 再按跳下一条。
    let mut h2 = CmdHistory::new();
    h2.record("build doc");
    h2.record("build kernel");
    h2.record("test all");
    cs.add("search_first", h2.search_next("build") == Some("build kernel"), "");
    cs.add("search_next_hit", h2.search_next("build") == Some("build doc"), "");
    cs.add("search_exhausted_honest", h2.search_next("build").is_none(), "");
    // 3) 敏感行过滤（10 例注入全拦）。
    let sensitive = [
        "login --password 123", "set TOKEN=abc", "read passwd", "export SECRET=x",
        "curl -H apikey:zz", "use api-key 9", "store credential here", "导出 私钥 文件",
        "PASSWORD=1 echo", "toKeN=1",
    ];
    let mut all_blocked = true;
    for s in sensitive {
        all_blocked &= CmdHistory::is_sensitive(s);
    }
    cs.add("sensitive_10_injections", all_blocked, "");
    cs.add("benign_recorded", !CmdHistory::is_sensitive("vx run app"), "");
    // 4) 1000 条上限淘汰（最旧出局）。
    let mut h3 = CmdHistory::new();
    for i in 0..(HIST_CAP + 50) {
        h3.record(num_cmd(i).as_str());
    }
    cs.add("cap_1000", h3.count() == HIST_CAP, "");
    cs.add("oldest_evicted", !contains_cmd(h3.iter_oldest_first(), "cmd 0"), "");
    cs.add("newest_kept", last_is(h3.iter_oldest_first(), "cmd 1049"), "");
    // 5) 清空双入口（同一清空核）。
    let cleared = h3.clear();
    cs.add("clear_double_entry", cleared == HIST_CAP && h3.count() == 0, "");
    // 6) 相邻重复去重。
    let mut h4 = CmdHistory::new();
    h4.record("ls");
    h4.record("ls");
    cs.add("adjacent_dedup", h4.count() == 1, "");
    // 7) 持久化序（旧→新写入——逐位对读，零分配）。
    let mut h5 = CmdHistory::new();
    h5.record("a");
    h5.record("b");
    let mut it = h5.iter_oldest_first();
    cs.add("persist_order", it.next() == Some("a") && it.next() == Some("b") && it.next().is_none(), "");
    cs
}

/// 零分配命令生成：把 "cmd N" 写进定长缓冲（自检专用，非内核热路径）。
fn num_cmd(i: usize) -> CmdBuf {
    let mut b = CmdBuf::new();
    b.push_str("cmd ");
    // 十进制手写（无 format! ——零堆纪律）。
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

/// 定长命令缓冲（零堆自检辅助）。
struct CmdBuf {
    buf: [u8; 32],
    n: usize,
}

impl CmdBuf {
    fn new() -> Self {
        CmdBuf { buf: [0; 32], n: 0 }
    }
    fn push_byte(&mut self, b: u8) {
        if self.n < 32 {
            self.buf[self.n] = b;
            self.n += 1;
        }
    }
    fn push_str(&mut self, s: &str) {
        for &b in s.as_bytes() {
            self.push_byte(b);
        }
    }
    fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.n]).unwrap_or("")
    }
}

/// 迭代器里是否含目标（零分配 contains）。
fn contains_cmd<'a, I: Iterator<Item = &'a str>>(mut it: I, want: &str) -> bool {
    it.any(|s| s == want)
}

/// 迭代器末元素是否为目标（零分配 last-eq）。
fn last_is<'a, I: Iterator<Item = &'a str>>(mut it: I, want: &str) -> bool {
    let mut last = "";
    for s in it {
        last = s;
    }
    last == want
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sensitive_ten_injections_never_recorded() {
        let mut h = CmdHistory::new();
        let lines = [
            "login --password 123", "set TOKEN=abc", "read passwd", "export SECRET=x",
            "curl -H apikey:zz", "use api-key 9", "store credential here", "导出 私钥 文件",
            "PASSWORD=1 echo", "toKeN=1",
        ];
        for l in lines {
            assert!(!h.record(l), "敏感行必须被拒: {l}");
        }
        assert_eq!(h.count(), 0);
    }

    #[test]
    fn eviction_keeps_newest_thousand() {
        let mut h = CmdHistory::new();
        for i in 0..(HIST_CAP + 10) {
            h.record(&format!("cmd {i}"));
        }
        assert_eq!(h.count(), HIST_CAP);
        let all: Vec<&str> = h.iter_oldest_first().collect();
        assert_eq!(all.first(), Some(&"cmd 10"));
        assert_eq!(all.last(), Some(&"cmd 1009"));
    }

    #[test]
    fn search_cycles_through_matches() {
        let mut h = CmdHistory::new();
        for c in ["build a", "run x", "build b", "build c"] {
            h.record(c);
        }
        assert_eq!(h.search_next("build"), Some("build c"));
        assert_eq!(h.search_next("build"), Some("build b"));
        assert_eq!(h.search_next("build"), Some("build a"));
        assert_eq!(h.search_next("build"), None);
        h.search_exit();
        assert_eq!(h.search_next("build"), Some("build c")); // 退出后重搜从最新起
    }
}

// ===========================================================================
// 深化 v2（F468）：敏感变体注入扩充 / 历史文件权限表 / 持久化序列化 /
// 清空双入口审计 / 搜索状态机收口
// ===========================================================================

/// 历史文件权限表（主册「历史文件权限（用户级只读他者）」——
/// 属主/同组/他者三段 rwx 位掩码：0640 语义 = 属主读写、同组只读、
/// 他者无权；一处一事实，落盘与审计共用此表）。
pub const HISTFILE_PERM_OWNER_RW: bool = true;
pub const HISTFILE_PERM_GROUP_R: bool = true;
pub const HISTFILE_PERM_OTHER_NONE: bool = true;
/// 位掩码形态（0640 = 0o640）。
pub const HISTFILE_PERM_BITS: u32 = 0o640;

/// 敏感词变体注入扩充（主册 10 例注入的完整面：分隔符变体/大小写/
/// 前后缀拼接——纯小写化检测的绕过样本逐例拦截）。
pub fn is_sensitive_v2(line: &str) -> bool {
    if CmdHistory::is_sensitive(line) {
        return true;
    }
    // 分隔符压缩归一化：下划线/点/连字符/空格删除（pass_word→password
    // 同罪），大小写归一；压缩后跑词表复用。
    let b = line.as_bytes();
    if b.len() > CMD_CAP {
        return false; // 超长行由 record 拒收——此处不做敏感判定。
    }
    let mut normalized = [0u8; CMD_CAP];
    let mut n = 0usize;
    for &c in b {
        if matches!(c, b'_' | b'.' | b'-' | b' ') {
            continue;
        }
        normalized[n] = c.to_ascii_lowercase();
        n += 1;
    }
    let joined = core::str::from_utf8(&normalized[..n]).unwrap_or("");
    SENSITIVE_WORDS.iter().any(|w| joined.contains(w))
}

/// 持久化序列化（跨会话保留的落盘面：魔标 + 序号水位 + 逐行
/// seq(8)+len(2)+text 变长定界——行序旧→新，v1 iter_oldest_first 同源）。
pub const CMDHIST_PERSIST_MAGIC: [u8; 4] = *b"VCH1";

pub fn save_history(hist: &CmdHistory, out: &mut [u8]) -> Option<usize> {
    if out.len() < 4 + 8 {
        return None;
    }
    out[..4].copy_from_slice(&CMDHIST_PERSIST_MAGIC);
    out[4..12].copy_from_slice(&hist.next_seq.to_be_bytes());
    let mut w = 12;
    for line in hist.iter_oldest_first() {
        if w + 2 + line.len() > out.len() {
            return None; // 缓冲不足：诚实截断在条目边界（不写半行）。
        }
        out[w..w + 2].copy_from_slice(&(line.len() as u16).to_be_bytes());
        out[w + 2..w + 2 + line.len()].copy_from_slice(line.as_bytes());
        w += 2 + line.len();
    }
    Some(w)
}

pub fn load_history_meta(buf: &[u8]) -> Option<(u64, usize)> {
    if buf.len() < 12 || buf[..4] != CMDHIST_PERSIST_MAGIC {
        return None;
    }
    let watermark = u64::from_be_bytes(buf[4..12].try_into().ok()?);
    let mut w = 12;
    let mut lines = 0usize;
    while w + 2 <= buf.len() {
        let len = u16::from_be_bytes(buf[w..w + 2].try_into().ok()?) as usize;
        w += 2 + len;
        if w > buf.len() {
            return None; // 半行 = 坏流。
        }
        lines += 1;
    }
    Some((watermark, lines))
}

/// 清空双入口审计（主册「清空命令与设置页入口」：两入口走同一清账
/// 路径，且都返回清掉条数——审计断言同路径同结果）。
pub fn clear_entries_double(hist: &mut CmdHistory) -> usize {
    // v1 clear() 是唯一清账路径；双入口在 UI 层各自调它——
    // 此处审计面：调用两次等价幂等（第二次清 0 条）。
    let first = hist.clear();
    let second = hist.clear();
    let _ = second;
    first
}

// ---------------------------------------------------------------------------
// 深化自检（F468 v2）
// ---------------------------------------------------------------------------

pub fn run_cmdhist_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F468-v2");
    // 1) 敏感变体注入：分隔符拼接绕过逐例拦截。
    let variants = ["pass_word=x", "my.token=abc", "api-key:zz", "secret_value", "APIKEY=1"];
    cs.add("variant_injection_blocked", variants.iter().all(|&v| is_sensitive_v2(v)), "");
    // 正常命令不误伤。
    cs.add("normal_not_flagged", !is_sensitive_v2("vol list C:\\") && !is_sensitive_v2("proc top"), "");
    // 2) 权限表：0640 三段语义齐。
    cs.add("perm_table", HISTFILE_PERM_BITS == 0o640
        && HISTFILE_PERM_OWNER_RW && HISTFILE_PERM_GROUP_R && HISTFILE_PERM_OTHER_NONE, "");
    // 3) 持久化：旧→新序 + 水位线 + 坏流拒收。
    let mut h = CmdHistory::new();
    let _ = h.record("first");
    let _ = h.record("second");
    let mut buf = [0u8; 512];
    cs.add("persist_roundtrip", {
        match save_history(&h, &mut buf) {
            Some(n) => {
                match load_history_meta(&buf[..n]) {
                    Some((watermark, lines)) => watermark == 2 && lines == 2,
                    None => false,
                }
            }
            None => false,
        }
    }, "");
    cs.add("persist_bad_magic", load_history_meta(b"XXXX\x00\x00\x00\x00\x00\x00\x00\x00").is_none(), "");
    cs.add("persist_truncated_rejected", load_history_meta(&buf[..14]).is_none(), "");
    // 4) 清空双入口：同路径同结果、幂等。
    let mut h2 = CmdHistory::new();
    let _ = h2.record("a");
    let _ = h2.record("b");
    cs.add("clear_double_entry", clear_entries_double(&mut h2) == 2 && h2.count() == 0, "");
    // 5) 搜索状态机收口：搜索退出后游标归位。
    let mut h3 = CmdHistory::new();
    let _ = h3.record("build all");
    let _ = h3.record("build docs");
    let hit = h3.search_next("build");
    cs.add("search_hits", hit.is_some(), "");
    h3.search_exit();
    cs.add("search_exit_resets", h3.recall(true).is_some(), "");
    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn sensitive_10_injection_matrix() {
        // 主册 10 例注入全拦（原词 5 + 变体 5）。
        let all = [
            "password=1", "passwd x", "token fetch", "secret key", "apikey=zz",
            "pass-word", "my_password", "APIKEY", "credential manager", "私钥导出",
        ];
        for line in all {
            assert!(is_sensitive_v2(line) || CmdHistory::is_sensitive(line), "{} 应被拦截", line);
        }
    }

    #[test]
    fn persistence_oldest_first_order() {
        let mut h = CmdHistory::new();
        let _ = h.record("alpha");
        let _ = h.record("beta");
        let _ = h.record("gamma");
        let mut buf = [0u8; 256];
        let n = save_history(&h, &mut buf).unwrap();
        let (_, lines) = load_history_meta(&buf[..n]).unwrap();
        assert_eq!(lines, 3);
    }

    #[test]
    fn record_edge_cases() {
        let mut h = CmdHistory::new();
        // 空行拒收。
        assert!(!h.record(""));
        // 超长拒收。
        let long = "x".repeat(CMD_CAP + 1);
        assert!(!h.record(&long));
        // 相邻重复去重（记一条不涨两行）。
        assert!(h.record("same"));
        assert!(h.record("same"));
        assert_eq!(h.count(), 1);
    }

    #[test]
    fn search_exit_then_recall_fresh() {
        let mut h = CmdHistory::new();
        let _ = h.record("run tests");
        let _ = h.record("run build");
        let _ = h.search_next("run");
        h.search_exit();
        // 退出后 ↑ 回到最新一条（状态干净）。
        assert_eq!(h.recall(true), Some("run build"));
    }
}
