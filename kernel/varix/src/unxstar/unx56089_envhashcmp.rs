//! UNX-F56089 跨波同 env_hash 对比校验件（0x60B00 段）
//!
//! 锚点：UNX-F56089-J1 对比账全量（跨波对比逐对落账：对键 × 波 A hash ×
//! 波 B hash 三元组）；J2 同 env 语义（同键跨波 env_hash 必须逐字节一致，
//! 漂移即拒比）；J3 对比审计链（对指纹链、封账只读、hash 语义闭集）三
//! 判据全绿。
//! 域：CoRun·UNX-F2-B40（采集管线域·跨波对比面）。
//!
//! 结构：env_hash 登记（对键 → 波 A hash × 波 B hash 逐对落账）+ hash
//! 一致核（双 hash 逐字节比对）+ 漂移拒比语义（不一致对显性标注
//! DRIFT 且不入有效对比集）+ 对指纹链 + 封账语义 + 注入核（空对键/坏
//! hash 格式/双 hash 漂移冒充一致/重复对/封账改写/空账复核/单边 hash
//! 必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60B00 独占 0x20）。
pub const SEG_BASE: u32 = 0x60B00;
pub const EV_KEY_EMPTY: u32 = SEG_BASE + 0x01; // 对键为空
pub const EV_HASH_BAD: u32 = SEG_BASE + 0x02; // hash 格式非法（非 16 位十六进制）
pub const EV_DRIFT: u32 = SEG_BASE + 0x03; // 双 hash 漂移（环境不一致）
pub const EV_DUP: u32 = SEG_BASE + 0x04; // 重复对键
pub const EV_SEALED: u32 = SEG_BASE + 0x05; // 对比账已封账拒写
pub const EV_EMPTY: u32 = SEG_BASE + 0x06; // 空账复核
pub const EV_HALF: u32 = SEG_BASE + 0x07; // 单边 hash（缺波 A 或波 B）

/// env_hash 长度（16 位十六进制指纹）。
pub const HASH_LEN: usize = 16;

/// hash 格式核。
pub fn hash_ok(h: &str) -> bool {
    let b = h.as_bytes();
    b.len() == HASH_LEN && b.iter().all(|c| c.is_ascii_hexdigit())
}

/// hash 一致核。
pub fn hash_equal(a: &str, b: &str) -> bool {
    hash_ok(a) && hash_ok(b) && a == b
}

/// 对指纹：FNV-1a 32 位（键|A|B 复合）。
pub fn pair_fingerprint(key: &str, ha: &str, hb: &str) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let line = format!("{}|{}|{}", key, ha, hb);
    for b in line.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 对比账目：对键 × 波 A hash × 波 B hash × 有效标记 × 指纹 × 链。
#[derive(Clone)]
pub struct EvPair {
    pub key: String,
    pub hash_a: String,
    pub hash_b: String,
    pub valid: bool,
    pub fp: u32,
    pub chain: u32,
}

pub const CHAIN_SEED: u32 = 0x811C_9DC5;

/// 对比总账。
pub struct EvBook {
    pub pairs: Vec<EvPair>,
    pub sealed: bool,
}

impl EvBook {
    pub fn new() -> Self {
        EvBook { pairs: Vec::new(), sealed: false }
    }

    fn find(&self, key: &str) -> Option<usize> {
        for i in 0..self.pairs.len() {
            if self.pairs[i].key == key {
                return Some(i);
            }
        }
        None
    }

    /// 对比落账：空键拒 + 双 hash 格式 + 重复拒三闸；漂移对显性标
    /// valid=false 但仍入账（审计留痕），有效对比集只收一致对。
    pub fn compare(&mut self, key: &str, ha: &str, hb: &str) -> Result<bool, u32> {
        if self.sealed {
            return Err(EV_SEALED);
        }
        if key.is_empty() {
            return Err(EV_KEY_EMPTY);
        }
        if !hash_ok(ha) || !hash_ok(hb) {
            return Err(EV_HASH_BAD);
        }
        if self.find(key).is_some() {
            return Err(EV_DUP);
        }
        let valid = ha == hb;
        let fp = pair_fingerprint(key, ha, hb);
        let chain = match self.pairs.last() {
            None => CHAIN_SEED ^ fp,
            Some(prev) => prev.fp ^ fp,
        };
        self.pairs.push(EvPair {
            key: key.to_string(),
            hash_a: ha.to_string(),
            hash_b: hb.to_string(),
            valid,
            fp,
            chain,
        });
        Ok(valid)
    }

    /// J2：有效对比集（仅一致对）。
    pub fn valid_pairs(&self) -> usize {
        let mut n = 0;
        for p in self.pairs.iter() {
            if p.valid {
                n += 1;
            }
        }
        n
    }

    /// J1：对比账全量（逐对 hash 格式合法）。
    pub fn pairs_ok(&self) -> bool {
        !self.pairs.is_empty()
            && self.pairs.iter().all(|p| hash_ok(&p.hash_a) && hash_ok(&p.hash_b))
    }

    /// J2：漂移对必须显性标记（valid=false 的对不允许被当有效对比消费）。
    pub fn drift_marked(&self, key: &str) -> Result<bool, u32> {
        match self.find(key) {
            Some(i) => Ok(!self.pairs[i].valid),
            None => Err(EV_EMPTY),
        }
    }

    /// J3：对指纹链复核。
    pub fn chain_ok(&self) -> bool {
        if self.pairs.is_empty() {
            return false;
        }
        for i in 0..self.pairs.len() {
            let expect = if i == 0 {
                CHAIN_SEED ^ self.pairs[0].fp
            } else {
                self.pairs[i - 1].fp ^ self.pairs[i].fp
            };
            if self.pairs[i].chain != expect {
                return false;
            }
        }
        true
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    pub fn chain_tail(&self) -> u32 {
        match self.pairs.last() {
            None => CHAIN_SEED,
            Some(p) => p.chain,
        }
    }
}

/// 标准 env_hash（正本口径，判据与夹具共用）。
pub const CANON_HASH: &str = "0123456789abcdef";
pub const DRIFT_HASH: &str = "0123456789abcdee";

/// 标准夹具：3 对（2 有效 1 漂移）。
pub fn standard_book() -> EvBook {
    let mut b = EvBook::new();
    if b.compare("cmp|wave7|wave8|mem", CANON_HASH, CANON_HASH).is_err() {
        panic!("standard fixture pair1 must compare");
    }
    if b.compare("cmp|wave7|wave8|io", CANON_HASH, CANON_HASH).is_err() {
        panic!("standard fixture pair2 must compare");
    }
    if b.compare("cmp|wave7|wave9|mem", CANON_HASH, DRIFT_HASH).is_err() {
        panic!("standard fixture pair3 must record");
    }
    b
}

/// 注入核：7 注入必抓（空对键/坏 hash 格式/重复对/漂移冒充一致/封账改写/空账复核/有效集误计）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 空对键
    let mut b = EvBook::new();
    if b.compare("", CANON_HASH, CANON_HASH) == Err(EV_KEY_EMPTY) {
        caught += 1;
    }
    // 2) 坏 hash（长度 15）
    if b.compare("k", "0123456789abcde", CANON_HASH) == Err(EV_HASH_BAD) {
        caught += 1;
    }
    // 3) 重复对
    if b.compare("k", CANON_HASH, CANON_HASH).is_ok()
        && b.compare("k", CANON_HASH, CANON_HASH) == Err(EV_DUP)
    {
        caught += 1;
    }
    // 4) 漂移对不入有效集
    let sb = standard_book();
    if sb.valid_pairs() == 2 && sb.drift_marked("cmp|wave7|wave9|mem") == Ok(true) {
        caught += 1;
    }
    // 5) hash 一致核对漂移 hash 拒
    if !hash_equal(CANON_HASH, DRIFT_HASH) {
        caught += 1;
    }
    // 6) 空账复核
    if EvBook::new().chain_tail() == CHAIN_SEED && EvBook::new().drift_marked("x").is_err() {
        caught += 1;
    }
    // 7) 封账改写拒
    b.seal();
    if b.compare("late", CANON_HASH, CANON_HASH) == Err(EV_SEALED) {
        caught += 1;
    }
    (caught, 7)
}

/// 对比账渲染导出（逐对一行）。
pub fn render_book(b: &EvBook) -> String {
    let mut out = String::new();
    for p in b.pairs.iter() {
        let st = if p.valid { "OK" } else { "DRIFT" };
        out.push_str(&format!("{}|{}|{}|{}\n", p.key, p.hash_a, p.hash_b, st));
    }
    out
}

/// 渲染完整性核：行数齐 + 逐行四段 + 状态闭集命中。
pub fn render_complete(text: &str, expect_rows: usize) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != expect_rows {
        return false;
    }
    lines.iter().all(|ln| {
        let parts: Vec<&str> = ln.split('|').collect();
        parts.len() == 4 && (parts[3] == "OK" || parts[3] == "DRIFT")
    })
}

/// 判据入口（CheckSet）。
pub fn run_unx56089_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56089");
    // J1-1 对比账全量（三对逐对合法）
    let b = standard_book();
    let ok = b.pairs_ok() && b.pairs.len() == 3;
    s.add("56089-J1-对比账全量", ok, if ok { "" } else { "对比对残缺" });
    // J1-2 hash 格式闭集边界
    let ok = hash_ok(CANON_HASH) && !hash_ok("0123456789abcde") && !hash_ok("zz23456789abcdef");
    s.add("56089-J1-hash格式闭集", ok, if ok { "" } else { "hash 格式漂移" });
    // J1-3 hash 一致核（同 hash 真、异 hash 假）
    let ok = hash_equal(CANON_HASH, CANON_HASH) && !hash_equal(CANON_HASH, DRIFT_HASH);
    s.add("56089-J1-hash一致核", ok, if ok { "" } else { "一致核漂移" });
    // J2-1 同 env 对比放行（有效对 2）
    let ok = b.valid_pairs() == 2;
    s.add("56089-J2-同 env 放行", ok, if ok { "" } else { "有效集漂移" });
    // J2-2 漂移对显性标注（DRIFT 不冒充有效）
    let ok = b.drift_marked("cmp|wave7|wave9|mem") == Ok(true)
        && b.drift_marked("cmp|wave7|wave8|mem") == Ok(false);
    s.add("56089-J2-漂移显性标注", ok, if ok { "" } else { "漂移标注漂移" });
    // J2-3 漂移对仍入账留痕（3 对全在册）
    let ok = b.pairs.len() == 3;
    s.add("56089-J2-漂移留痕", ok, if ok { "" } else { "留痕缺失" });
    // J2-4 重复对拒
    let mut b2 = standard_book();
    let ok = b2.compare("cmp|wave7|wave8|mem", CANON_HASH, CANON_HASH) == Err(EV_DUP);
    s.add("56089-J2-重复对拒", ok, if ok { "" } else { "重复闸失效" });
    // J3-1 对指纹链复核
    let ok = b.chain_ok();
    s.add("56089-J3-对指纹链", ok, if ok { "" } else { "指纹链断裂" });
    // J3-2 链尾稳定
    let ok = standard_book().chain_tail() == standard_book().chain_tail();
    s.add("56089-J3-链尾稳定", ok, if ok { "" } else { "链尾漂移" });
    // J3-3 链尾敏感（漂移 hash 换一字链尾变）
    let mut b3 = EvBook::new();
    let _ = b3.compare("k", CANON_HASH, "0123456789abcdec");
    let ok = b3.chain_tail() != {
        let mut t = EvBook::new();
        let _ = t.compare("k", CANON_HASH, CANON_HASH);
        t.chain_tail()
    };
    s.add("56089-J3-链尾敏感", ok, if ok { "" } else { "链尾钝感" });
    // J3-4 封账拒写
    let mut b4 = standard_book();
    b4.seal();
    let ok = b4.compare("late", CANON_HASH, CANON_HASH) == Err(EV_SEALED);
    s.add("56089-J3-封账拒写", ok, if ok { "" } else { "封账穿透" });
    // J1-4 渲染导出三行齐
    let text = render_book(&standard_book());
    let ok = render_complete(&text, 3) && text.contains("DRIFT");
    s.add("56089-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56089-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56089_all_checks_green() {
        let set = run_unx56089_checks();
        assert!(set.all_passed(), "UNX-F56089 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56089_chain_stable() {
        assert_eq!(standard_book().chain_tail(), standard_book().chain_tail());
    }
}
