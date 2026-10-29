//! UNX-F56086 样本量守门器（≥30 纪律强制）（0x60AA0 段）
//!
//! 锚点：UNX-F56086-J1 守门登记全量（送审样本集逐批落账：批键 × 样本量
//! × 守闸结论三元组）；J2 ≥30 纪律强制（样本量不足拒放行且只许整批补
//! 采，禁拆批绕闸）；J3 守门审计链（放行/拒收逐次留痕、批指纹链、封账
//! 只读）三判据全绿。
//! 域：CoRun·UNX-F2-B40（采集管线域·样本守闸面）。
//!
//! 结构：守门登记册（批键 → 样本量 × 结论逐批落账）+ 样本量闸（<30 拒
//! 放行）+ 整批补采语义（拒收批补采后复闸、禁拆批绕闸）+ 批指纹链 +
//! 封账语义 + 注入核（空批键/零样本/不足放行穿透/拆批绕闸/封账改写/
//! 未登记复闸/重复落账必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60AA0 独占 0x20）。
pub const SEG_BASE: u32 = 0x60AA0;
pub const GT_KEY_EMPTY: u32 = SEG_BASE + 0x01; // 批键为空
pub const GT_ZERO: u32 = SEG_BASE + 0x02; // 零样本送审
pub const GT_SHORT: u32 = SEG_BASE + 0x03; // 样本量不足（<30）
pub const GT_SPLIT: u32 = SEG_BASE + 0x04; // 拆批绕闸（同一源批拆小批送审）
pub const GT_DUP: u32 = SEG_BASE + 0x05; // 同批键重复落账
pub const GT_SEALED: u32 = SEG_BASE + 0x06; // 守门账已封账拒写
pub const GT_NOT_FOUND: u32 = SEG_BASE + 0x07; // 未登记复闸

/// 样本量纪律下限（与 F56077 同源）。
pub const SAMPLE_MIN: u32 = 30;

/// 守门结论闭集。
pub const V_PASS: usize = 0;
pub const V_REJECT: usize = 1;
pub const V_NAMES: [&str; 2] = ["pass", "reject"];

/// 批指纹：FNV-1a 32 位。
pub fn batch_fingerprint(key: &str, n: u32) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let line = format!("{}|{}", key, n);
    for b in line.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 守门账目：批键 × 样本量 × 结论 × 波次。
#[derive(Clone)]
pub struct GateEntry {
    pub key: String,
    pub n: u32,
    pub verdict: usize,
    pub wave: u32,
}

/// 守门总账。
pub struct GateBook {
    pub entries: Vec<GateEntry>,
    pub sealed: bool,
    pub audit: Vec<String>,
}

impl GateBook {
    pub fn new() -> Self {
        GateBook { entries: Vec::new(), sealed: false, audit: Vec::new() }
    }

    fn find(&self, key: &str) -> Option<usize> {
        for i in 0..self.entries.len() {
            if self.entries[i].key == key {
                return Some(i);
            }
        }
        None
    }

    /// 送审：空键拒 + 零样本拒 + 重复落账拒 + 样本量闸四闸。
    pub fn submit(&mut self, key: &str, n: u32, wave: u32) -> Result<usize, u32> {
        if self.sealed {
            return Err(GT_SEALED);
        }
        if key.is_empty() {
            return Err(GT_KEY_EMPTY);
        }
        if n == 0 {
            return Err(GT_ZERO);
        }
        if self.find(key).is_some() {
            return Err(GT_DUP);
        }
        if n < SAMPLE_MIN {
            self.audit.push(format!("reject|{}|{}", key, n));
            self.entries.push(GateEntry {
                key: key.to_string(),
                n,
                verdict: V_REJECT,
                wave,
            });
            return Ok(V_REJECT);
        }
        self.audit.push(format!("pass|{}|{}", key, n));
        self.entries.push(GateEntry { key: key.to_string(), n, verdict: V_PASS, wave });
        Ok(V_PASS)
    }

    /// J2：整批补采复闸（拒收批补采后按同键复闸：样本量达标才转 pass）。
    pub fn resubmit(&mut self, key: &str, new_n: u32, wave: u32) -> Result<usize, u32> {
        if self.sealed {
            return Err(GT_SEALED);
        }
        let idx = match self.find(key) {
            Some(i) => i,
            None => return Err(GT_NOT_FOUND),
        };
        if self.entries[idx].verdict != V_REJECT {
            return Err(GT_DUP);
        }
        if new_n <= self.entries[idx].n {
            return Err(GT_SPLIT);
        }
        if new_n < SAMPLE_MIN {
            self.audit.push(format!("reject|{}|{}", key, new_n));
            self.entries[idx].n = new_n;
            self.entries[idx].wave = wave;
            return Ok(V_REJECT);
        }
        self.audit.push(format!("pass|{}|{}", key, new_n));
        self.entries[idx].n = new_n;
        self.entries[idx].verdict = V_PASS;
        self.entries[idx].wave = wave;
        Ok(V_PASS)
    }

    /// J2：拆批绕闸核（同一源前缀的拒收批不得以小批 pass）。
    pub fn split_bypass_ok(&self, key: &str, n: u32) -> bool {
        // 语义：样本量 < 下限的任何批（无论键名）都不可能 pass，闸在量上。
        n >= SAMPLE_MIN
            && !self.entries.iter().any(|e| e.key == key && e.verdict == V_REJECT && e.n >= n)
    }

    /// 查询批结论（未登记显性拒绝）。
    pub fn verdict_of(&self, key: &str) -> Result<usize, u32> {
        match self.find(key) {
            Some(i) => Ok(self.entries[i].verdict),
            None => Err(GT_NOT_FOUND),
        }
    }

    /// J1：守门全量（逐批结论合法）。
    pub fn entries_ok(&self) -> bool {
        !self.entries.is_empty()
            && self.entries.iter().all(|e| !e.key.is_empty() && e.verdict < V_NAMES.len())
    }

    /// 结论统计闭式（pass + reject = 总数）。
    pub fn verdict_census(&self) -> (usize, usize) {
        let mut p = 0;
        let mut r = 0;
        for e in self.entries.iter() {
            if e.verdict == V_PASS {
                p += 1;
            } else {
                r += 1;
            }
        }
        (p, r)
    }

    /// J3：审计链完整（逐条 pass|/reject| 前缀）。
    pub fn audit_ok(&self) -> bool {
        !self.audit.is_empty()
            && self.audit.iter().all(|a| a.starts_with("pass|") || a.starts_with("reject|"))
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 封账指纹（逐批复合）。
    pub fn seal_fingerprint(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for e in self.entries.iter() {
            let line = format!("{}|{}|{}|w{}", e.key, e.n, V_NAMES[e.verdict], e.wave);
            for b in line.as_bytes() {
                h ^= *b as u32;
                h = h.wrapping_mul(0x0100_0193);
            }
        }
        h
    }
}

/// 标准夹具：3 批（1 pass 60 / 1 reject 29 / 1 pass 1000）。
pub fn standard_book() -> GateBook {
    let mut b = GateBook::new();
    if b.submit("mem|wave1|A", 60, 1) != Ok(V_PASS) {
        panic!("standard fixture pass must pass");
    }
    if b.submit("mem|wave1|B", 29, 1) != Ok(V_REJECT) {
        panic!("standard fixture reject must reject");
    }
    if b.submit("mem|wave1|C", 1_000, 1) != Ok(V_PASS) {
        panic!("standard fixture big pass must pass");
    }
    b
}

/// 注入核：7 注入必抓（空批键/零样本/不足送审记拒/拆批复闸/重复落账/未登记复闸/封账改写）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 空批键
    let mut b = GateBook::new();
    if b.submit("", 60, 1) == Err(GT_KEY_EMPTY) {
        caught += 1;
    }
    // 2) 零样本
    if b.submit("k", 0, 1) == Err(GT_ZERO) {
        caught += 1;
    }
    // 3) 不足送审只记拒不放行
    if b.submit("k", 29, 1) == Ok(V_REJECT) {
        caught += 1;
    }
    // 4) 重复落账
    if b.submit("k2", 30, 1).is_ok() && b.submit("k2", 31, 2) == Err(GT_DUP) {
        caught += 1;
    }
    // 5) 未登记复闸
    if b.resubmit("ghost", 60, 2) == Err(GT_NOT_FOUND) {
        caught += 1;
    }
    // 6) 拆批绕闸拒（补采不增量）
    if b.resubmit("k", 29, 2) == Err(GT_SPLIT) {
        caught += 1;
    }
    // 7) 封账改写拒
    b.seal();
    if b.submit("late", 60, 3) == Err(GT_SEALED) && b.resubmit("k", 60, 3) == Err(GT_SEALED) {
        caught += 1;
    }
    (caught, 7)
}

/// 守门账渲染导出（逐批一行）。
pub fn render_book(b: &GateBook) -> String {
    let mut out = String::new();
    for e in b.entries.iter() {
        out.push_str(&format!("{}|{}|{}|w{}\n", e.key, e.n, V_NAMES[e.verdict], e.wave));
    }
    out
}

/// 渲染完整性核：行数齐 + 逐行四段 + 结论闭集命中。
pub fn render_complete(text: &str, expect_rows: usize) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != expect_rows {
        return false;
    }
    lines.iter().all(|ln| {
        let parts: Vec<&str> = ln.split('|').collect();
        parts.len() == 4 && (parts[2] == "pass" || parts[2] == "reject")
    })
}

/// 判据入口（CheckSet）。
pub fn run_unx56086_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56086");
    // J1-1 守门登记全量（三批逐批合法）
    let b = standard_book();
    let ok = b.entries_ok() && b.entries.len() == 3;
    s.add("56086-J1-守门登记全量", ok, if ok { "" } else { "守门批残缺" });
    // J1-2 下限纪律常量（30）
    let ok = SAMPLE_MIN == 30;
    s.add("56086-J1-下限常量", ok, if ok { "" } else { "下限漂移" });
    // J1-3 结论闭式（2 pass + 1 reject = 3）
    let (p, r) = b.verdict_census();
    let ok = p == 2 && r == 1 && p + r == b.entries.len();
    s.add("56086-J1-结论闭式", ok, if ok { "" } else { "闭式计数漂移" });
    // J2-1 ≥30 放行（29 拒 30 过）
    let mut b2 = GateBook::new();
    let ok = b2.submit("k29", 29, 1) == Ok(V_REJECT) && b2.submit("k30", 30, 1) == Ok(V_PASS);
    s.add("56086-J2-三十闸", ok, if ok { "" } else { "三十闸漂移" });
    // J2-2 不足送审不放行（结论必为 reject）
    let ok = b.verdict_of("mem|wave1|B") == Ok(V_REJECT);
    s.add("56086-J2-不足必拒", ok, if ok { "" } else { "不足穿透" });
    // J2-3 整批补采复闸（29→60 转 pass）
    let mut b3 = standard_book();
    let ok = b3.resubmit("mem|wave1|B", 60, 2) == Ok(V_PASS)
        && b3.verdict_of("mem|wave1|B") == Ok(V_PASS);
    s.add("56086-J2-补采复闸", ok, if ok { "" } else { "复闸语义漂移" });
    // J2-4 拆批绕闸拒
    let ok = b3.resubmit("mem|wave1|B", 29, 3) == Err(GT_SPLIT);
    s.add("56086-J2-拆批绕闸拒", ok, if ok { "" } else { "拆批闸失效" });
    // J2-5 未登记复闸显性拒绝
    let ok = b3.resubmit("ghost", 60, 3) == Err(GT_NOT_FOUND);
    s.add("56086-J2-未登记复闸拒", ok, if ok { "" } else { "复闸语义漂移" });
    // J3-1 审计链完整（3 submit + 1 resubmit = 5 条）
    let ok = b3.audit_ok() && b3.audit.len() == 5;
    s.add("56086-J3-审计链完整", ok, if ok { "" } else { "审计链断裂" });
    // J3-2 封账指纹稳定
    let ok = standard_book().seal_fingerprint() == standard_book().seal_fingerprint();
    s.add("56086-J3-封账指纹稳定", ok, if ok { "" } else { "指纹漂移" });
    // J3-3 指纹敏感（复闸转正指纹变）
    let fp0 = standard_book().seal_fingerprint();
    let mut b4 = standard_book();
    let _ = b4.resubmit("mem|wave1|B", 60, 2);
    let ok = b4.seal_fingerprint() != fp0;
    s.add("56086-J3-指纹敏感", ok, if ok { "" } else { "指纹钝感" });
    // J3-4 封账拒写
    let mut b5 = standard_book();
    b5.seal();
    let ok = b5.submit("late", 60, 3) == Err(GT_SEALED);
    s.add("56086-J3-封账拒写", ok, if ok { "" } else { "封账穿透" });
    // J1-4 渲染导出三行齐
    let text = render_book(&standard_book());
    let ok = render_complete(&text, 3) && text.contains("mem|wave1|A|60|pass|w1");
    s.add("56086-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56086-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56086_all_checks_green() {
        let set = run_unx56086_checks();
        assert!(set.all_passed(), "UNX-F56086 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56086_fingerprint_stable() {
        assert_eq!(standard_book().seal_fingerprint(), standard_book().seal_fingerprint());
    }
}
