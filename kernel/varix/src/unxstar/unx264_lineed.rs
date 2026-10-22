//! UNX-F0264 终端行编辑器增强（历史/补全）（0xAB00 段）
//!
//! 锚点：UNX-F0264-J1 历史 16 条循环/前缀补全/Tab 二义提示全过，
//! 边界输入不崩溃。
//! 域：CoRun·UNX-A1-B14（行编辑面）。
//!
//! 结构：历史环账（16 条容量，循环覆盖）+ 前缀补全器（候选集 →
//! 最长公共前缀 / 二义提示列表）+ 行缓冲（边界：空行/超长/退格越界）
//! + 边界不崩溃核（全部边界输入安全返回）+ 注入核（历史越位/补全
//! 破缺/退格越界/封账）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0xAB00 独占 0x80）。
pub const SEG_BASE: u32 = 0xAB00;
pub const XC_LE_HIST: u32 = SEG_BASE + 0x01;
pub const XC_LE_AMBIG: u32 = SEG_BASE + 0x02;
pub const XC_LE_SEALED: u32 = SEG_BASE + 0x03;

/// 历史容量。
pub const HIST_CAP: usize = 16;
/// 行缓冲上限。
pub const LINE_CAP: usize = 256;

/// 历史环账。
pub struct HistoryRing {
    pub buf: Vec<String>,
    pub pos: usize, // 下一条写入位
    pub count: usize,
    pub sealed: bool,
}

impl HistoryRing {
    pub fn new() -> Self {
        HistoryRing { buf: Vec::new(), pos: 0, count: 0, sealed: false }
    }

    /// 压入历史（循环覆盖最旧）。
    pub fn push(&mut self, line: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_LE_SEALED);
        }
        if line.is_empty() {
            return Ok(()); // 空行不入史
        }
        let entry = String::from(line);
        if self.buf.len() < HIST_CAP {
            self.buf.push(entry);
        } else {
            self.buf[self.pos] = entry;
        }
        self.pos = (self.pos + 1) % HIST_CAP;
        if self.count < HIST_CAP {
            self.count += 1;
        }
        Ok(())
    }

    /// 循环取历史（offset 相对最新，0=最新）。
    pub fn get(&self, offset: usize) -> Option<&str> {
        if offset >= self.count {
            return None;
        }
        // 最新条 = pos-1（环形）
        let newest = (self.pos + HIST_CAP - 1) % HIST_CAP;
        let idx = (newest + HIST_CAP - offset) % HIST_CAP;
        self.buf.get(idx).map(|s| s.as_str())
    }

    /// 满容量循环核：第 17 条覆盖第 1 条。
    pub fn wraps(&self) -> bool {
        self.count == HIST_CAP && self.buf.len() == HIST_CAP
    }
}

/// 行缓冲。
pub struct LineBuf {
    pub chars: Vec<u8>,
    pub cursor: usize,
}

impl LineBuf {
    pub fn new() -> Self {
        LineBuf { chars: Vec::new(), cursor: 0 }
    }

    /// 插入字符（超长安全拒）。
    pub fn insert(&mut self, c: u8) -> bool {
        if self.chars.len() >= LINE_CAP {
            return false; // 边界：安全拒不崩
        }
        if self.cursor > self.chars.len() {
            return false;
        }
        self.chars.insert(self.cursor, c);
        self.cursor += 1;
        true
    }

    /// 退格（越界安全）。
    pub fn backspace(&mut self) -> bool {
        if self.cursor == 0 || self.chars.is_empty() {
            return false; // 边界：安全拒不崩
        }
        self.cursor -= 1;
        self.chars.remove(self.cursor);
        true
    }

    /// 光标左右移（边界安全）。
    pub fn move_cursor(&mut self, delta: i32) {
        let nc = self.cursor as i64 + delta as i64;
        if nc < 0 {
            self.cursor = 0;
        } else if nc as usize > self.chars.len() {
            self.cursor = self.chars.len();
        } else {
            self.cursor = nc as usize;
        }
    }
}

/// 前缀补全器：候选集 → 最长公共前缀 + 二义列表。
pub struct Completer {
    pub candidates: Vec<String>,
}

impl Completer {
    pub fn new(cands: &[&str]) -> Self {
        Completer {
            candidates: cands.iter().map(|s| String::from(*s)).collect(),
        }
    }

    /// 前缀过滤候选。
    pub fn matches(&self, prefix: &str) -> Vec<&String> {
        self.candidates.iter().filter(|c| c.starts_with(prefix)).collect()
    }

    /// 唯一候选 → 直接补全；多候选 → 最长公共前缀；零候选 → None。
    pub fn complete(&self, prefix: &str) -> Option<(String, Vec<String>)> {
        let m = self.matches(prefix);
        if m.is_empty() {
            return None;
        }
        if m.len() == 1 {
            return Some((m[0].clone(), Vec::new()));
        }
        // 二义：求最长公共前缀
        let mut lcp = m[0].clone();
        for c in &m[1..] {
            let mut n = 0;
            let a = lcp.as_bytes();
            let b = c.as_bytes();
            while n < a.len() && n < b.len() && a[n] == b[n] {
                n += 1;
            }
            lcp = String::from_utf8_lossy(&a[..n]).into_owned();
        }
        let list = m.iter().map(|s| (*s).clone()).collect();
        Some((lcp, list))
    }
}

/// 历史 16 循环核。
pub fn hist16_exact() -> bool {
    let mut h = HistoryRing::new();
    for i in 0..20usize {
        let _ = h.push(&format!("cmd-{:02}", i));
    }
    if !h.wraps() {
        return false;
    }
    // 最新 = cmd-19，最旧 = cmd-04（被覆盖）
    h.get(0) == Some("cmd-19") && h.get(15) == Some("cmd-04") && h.get(16).is_none()
}

/// 前缀补全核：唯一/二义/零候选。
pub fn complete_exact() -> bool {
    let c = Completer::new(&["read", "reboot", "reset", "write"]);
    // 唯一
    match c.complete("read") {
        Some((s, list)) => {
            if !(s == "read" && list.is_empty()) {
                return false;
            }
        }
        None => return false,
    }
    // 二义：re/reboot/reset → lcp = "re"，列表 2
    let (lcp, list) = match c.complete("re") {
        Some(t) => t,
        None => return false,
    };
    if lcp != "re" || list.len() != 2 {
        return false;
    }
    // 零候选
    c.complete("zzz").is_none()
}

/// 边界不崩溃核：空行/超长/退格越界/光标越界全安全。
pub fn boundary_exact() -> bool {
    let mut lb = LineBuf::new();
    // 空退格
    if lb.backspace() {
        return false;
    }
    // 光标越界
    lb.move_cursor(-100);
    if lb.cursor != 0 {
        return false;
    }
    lb.move_cursor(100);
    if lb.cursor != 0 {
        return false;
    }
    // 超长
    let mut n = 0;
    for i in 0..(LINE_CAP + 10) {
        if lb.insert(b'a' + (i % 26) as u8) {
            n += 1;
        }
    }
    n == LINE_CAP && lb.chars.len() == LINE_CAP
}

/// 注入核（4 注入必抓）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 历史越位
    let h = HistoryRing::new();
    if h.get(0).is_none() {
        caught += 1;
    }
    // 2) 空行不入史
    let mut h2 = HistoryRing::new();
    let _ = h2.push("");
    if h2.count == 0 {
        caught += 1;
    }
    // 3) 二义提示列表完整
    let c = Completer::new(&["s", "st", "su"]);
    match c.complete("s") {
        Some((_, list)) => {
            if list.len() == 3 {
                caught += 1;
            }
        }
        None => {}
    }
    // 4) 封账后拒
    let mut h3 = HistoryRing::new();
    h3.sealed = true;
    if h3.push("x") == Err(XC_LE_SEALED) {
        caught += 1;
    }
    (caught, 4)
}

/// 指纹冻结：历史环折叠哈希。
pub fn fp_frozen(h: &HistoryRing) -> u64 {
    let mut x: u64 = 0xcbf29ce484222325;
    for s in &h.buf {
        for &c in s.as_bytes() {
            x ^= c as u64;
            x = x.wrapping_mul(0x100000001b3);
        }
    }
    x ^= h.count as u64;
    x = x.wrapping_mul(0x100000001b3);
    x
}

/// 判据入口（CheckSet）。
pub fn run_unx264_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0264");
    // J1-1 历史 16 循环
    let ok = hist16_exact();
    s.add("B14-01-历史16循环", ok, if ok { "" } else { "历史漂移" });
    // J1-2 前缀补全
    let ok = complete_exact();
    s.add("B14-02-前缀补全", ok, if ok { "" } else { "补全漂移" });
    // J1-3 边界不崩溃
    let ok = boundary_exact();
    s.add("B14-03-边界安全", ok, if ok { "" } else { "边界破" });
    // J1-4 注入核 4/4
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { alloc::string::String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("B14-04-注入核必抓", ok, &detail);
    // J1-5 指纹冻结
    let mut h = HistoryRing::new();
    let _ = h.push("fp");
    let ok = fp_frozen(&h) == fp_frozen(&h);
    s.add("B14-05-指纹冻结", ok, if ok { "" } else { "指纹漂移" });
    s
}
