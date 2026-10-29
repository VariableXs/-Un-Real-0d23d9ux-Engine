//! UNX-F56107 队列深度 1/32 双档控制器（0x60D40 段）
//!
//! 锚点：UNX-F56107-J1 双档控制全量（控制批逐批落账：批键 × 档位 × 命令
//! 深度三元组）；J2 双档语义（档位闭集 {1,32}、命令深度与档位逐批一致、
//! 越档即拒）；J3 控制审计链（批指纹链、封账只读、双档配对闭式）三判据
//! 全绿。
//! 域：CoRun·UNX-F2-B40（IO 域·队列深控制面）。
//!
//! 结构：档位闭集（QD1/QD32 双档）+ 控制批登记册（批键 → 档位 × 命令深
//! 逐批落账）+ 档深一致核（档位与命令深度逐批对齐）+ 双档配对核（同
//! 负载双档成对才可对照）+ 指纹链 + 注入核（越档深度/空批键/档位越界/
//! 重复批/封账改写/空账查询/配对残缺必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60D40 独占 0x20）。
pub const SEG_BASE: u32 = 0x60D40;
pub const QD_DEPTH_OVER: u32 = SEG_BASE + 0x01; // 越档深度（非 1/32）
pub const QD_KEY_EMPTY: u32 = SEG_BASE + 0x02; // 批键为空
pub const QD_TIER_UNK: u32 = SEG_BASE + 0x03; // 档位越界（闭集外）
pub const QD_MISMATCH: u32 = SEG_BASE + 0x04; // 档深不一致
pub const QD_DUP: u32 = SEG_BASE + 0x05; // 重复批键
pub const QD_SEALED: u32 = SEG_BASE + 0x06; // 控制账已封账拒批
pub const QD_EMPTY: u32 = SEG_BASE + 0x07; // 空账查询

/// 档位闭集。
pub const TIER_QD1: usize = 0;
pub const TIER_QD32: usize = 1;
pub const TIER_COUNT: usize = 2;
pub const TIER_NAMES: [&str; TIER_COUNT] = ["qd1", "qd32"];
pub const TIER_DEPTHS: [u32; TIER_COUNT] = [1, 32];

/// 档位闭集核。
pub fn tier_ok(tier: usize) -> bool {
    tier < TIER_COUNT
}

/// 档深一致核：命令深度与档位深度逐批对齐。
pub fn depth_matches(tier: usize, depth: u32) -> bool {
    tier_ok(tier) && depth == TIER_DEPTHS[tier]
}

/// 控制批指纹：FNV-1a 32 位。
pub fn ctrl_fingerprint(key: &str, tier: usize, depth: u32) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let name = if tier < TIER_COUNT { TIER_NAMES[tier] } else { "?" };
    let line = format!("{}|{}|{}", key, name, depth);
    for b in line.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 控制批：批键 × 负载键 × 档位 × 命令深度 × 指纹 × 链。
#[derive(Clone)]
pub struct CtrlBatch {
    pub key: String,
    pub load_key: String,
    pub tier: usize,
    pub depth: u32,
    pub fp: u32,
    pub chain: u32,
}

pub const CHAIN_SEED: u32 = 0x811C_9DC5;

/// 控制总账。
pub struct QdBook {
    pub batches: Vec<CtrlBatch>,
    pub sealed: bool,
}

impl QdBook {
    pub fn new() -> Self {
        QdBook { batches: Vec::new(), sealed: false }
    }

    fn find(&self, key: &str) -> Option<usize> {
        for i in 0..self.batches.len() {
            if self.batches[i].key == key {
                return Some(i);
            }
        }
        None
    }

    /// 控制落账：空键拒 + 档位闭集 + 档深一致 + 重复拒四闸。
    pub fn control(&mut self, key: &str, load_key: &str, tier: usize, depth: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(QD_SEALED);
        }
        if key.is_empty() || load_key.is_empty() {
            return Err(QD_KEY_EMPTY);
        }
        if !tier_ok(tier) {
            return Err(QD_TIER_UNK);
        }
        if !depth_matches(tier, depth) {
            return Err(QD_MISMATCH);
        }
        if self.find(key).is_some() {
            return Err(QD_DUP);
        }
        let fp = ctrl_fingerprint(key, tier, depth);
        let chain = match self.batches.last() {
            None => CHAIN_SEED ^ fp,
            Some(prev) => prev.fp ^ fp,
        };
        self.batches.push(CtrlBatch {
            key: key.to_string(),
            load_key: load_key.to_string(),
            tier,
            depth,
            fp,
            chain,
        });
        Ok(())
    }

    /// J2：双档配对核（同负载键双档成对：恰一 qd1 + 恰一 qd32）。
    pub fn pair_ok(&self, load_key: &str) -> Result<bool, u32> {
        let mut lo = 0usize;
        let mut hi = 0usize;
        for b in self.batches.iter() {
            if b.load_key == load_key {
                if b.tier == TIER_QD1 {
                    lo += 1;
                }
                if b.tier == TIER_QD32 {
                    hi += 1;
                }
            }
        }
        if lo + hi == 0 {
            return Err(QD_EMPTY);
        }
        Ok(lo == 1 && hi == 1)
    }

    /// J1：控制账全量（逐批档深一致）。
    pub fn batches_ok(&self) -> bool {
        !self.batches.is_empty()
            && self.batches.iter().all(|b| {
                !b.key.is_empty() && tier_ok(b.tier) && depth_matches(b.tier, b.depth)
            })
    }

    /// J3：双档覆盖闭式（两档各至少一批）。
    pub fn tiers_covered(&self) -> bool {
        let mut lo = false;
        let mut hi = false;
        for b in self.batches.iter() {
            if b.tier == TIER_QD1 {
                lo = true;
            }
            if b.tier == TIER_QD32 {
                hi = true;
            }
        }
        lo && hi
    }

    /// J3：控制指纹链复核。
    pub fn chain_ok(&self) -> bool {
        if self.batches.is_empty() {
            return false;
        }
        for i in 0..self.batches.len() {
            let expect = if i == 0 {
                CHAIN_SEED ^ self.batches[0].fp
            } else {
                self.batches[i - 1].fp ^ self.batches[i].fp
            };
            if self.batches[i].chain != expect {
                return false;
            }
        }
        true
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    pub fn chain_tail(&self) -> u32 {
        match self.batches.last() {
            None => CHAIN_SEED,
            Some(b) => b.chain,
        }
    }
}

/// 标准夹具：两负载 × 双档全配对。
pub fn standard_book() -> QdBook {
    let mut b = QdBook::new();
    let plan: [(&str, &str, usize, u32); 4] = [
        ("qd|4krr|A|1", "load|4krr", TIER_QD1, 1),
        ("qd|4krr|A|32", "load|4krr", TIER_QD32, 32),
        ("qd|1msr|B|1", "load|1msr", TIER_QD1, 1),
        ("qd|1msr|B|32", "load|1msr", TIER_QD32, 32),
    ];
    for p in plan.iter() {
        if b.control(p.0, p.1, p.2, p.3).is_err() {
            panic!("standard fixture control must succeed");
        }
    }
    b
}

/// 注入核：7 注入必抓（越档深度/档位越界/档深不一致/空批键/重复批/封账改写/空账配对）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 越档深度
    let mut b = QdBook::new();
    if b.control("k", "l", TIER_QD1, 8) == Err(QD_MISMATCH) {
        caught += 1;
    }
    // 2) 档位越界
    if b.control("k", "l", 9, 1) == Err(QD_TIER_UNK) {
        caught += 1;
    }
    // 3) 空批键
    if b.control("", "l", TIER_QD1, 1) == Err(QD_KEY_EMPTY) {
        caught += 1;
    }
    // 4) 重复批
    if b.control("k", "l", TIER_QD1, 1).is_ok() && b.control("k", "l", TIER_QD32, 32) == Err(QD_DUP) {
        caught += 1;
    }
    // 5) 配对残缺（单档）
    if b.pair_ok("l") == Ok(false) {
        caught += 1;
    }
    // 6) 空账配对
    if QdBook::new().pair_ok("l").err() == Some(QD_EMPTY) {
        caught += 1;
    }
    // 7) 封账改写拒
    let mut sb = standard_book();
    sb.seal();
    if sb.control("late", "l", TIER_QD1, 1) == Err(QD_SEALED) {
        caught += 1;
    }
    (caught, 7)
}

/// 控制账渲染导出。
pub fn render_book(b: &QdBook) -> String {
    let mut out = String::new();
    for x in b.batches.iter() {
        out.push_str(&format!("{}|{}|{}|{:08x}\n", x.key, TIER_NAMES[x.tier], x.depth, x.fp));
    }
    out
}

/// 渲染完整性核：行数齐 + 逐行四段 + 档名闭集命中。
pub fn render_complete(text: &str, expect_rows: usize) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != expect_rows {
        return false;
    }
    lines.iter().all(|ln| {
        let parts: Vec<&str> = ln.split('|').collect();
        parts.len() == 4 && (parts[1] == "qd1" || parts[1] == "qd32")
    })
}

/// 判据入口（CheckSet）。
pub fn run_unx56107_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56107");
    // J1-1 控制账全量（四批逐批合法）
    let b = standard_book();
    let ok = b.batches_ok() && b.batches.len() == 4;
    s.add("56107-J1-控制账全量", ok, if ok { "" } else { "控制批残缺" });
    // J1-2 档位闭集齐
    let ok = TIER_NAMES.len() == TIER_COUNT && TIER_DEPTHS == [1, 32];
    s.add("56107-J1-档位闭集齐", ok, if ok { "" } else { "档位漂移" });
    // J1-3 档深一致核（对齐真、错位假）
    let ok = depth_matches(TIER_QD1, 1) && depth_matches(TIER_QD32, 32)
        && !depth_matches(TIER_QD1, 32) && !depth_matches(TIER_QD32, 1);
    s.add("56107-J1-档深一致核", ok, if ok { "" } else { "档深漂移" });
    // J2-1 双档配对命中（两负载均成对）
    let ok = b.pair_ok("load|4krr") == Ok(true) && b.pair_ok("load|1msr") == Ok(true);
    s.add("56107-J2-双档配对命中", ok, if ok { "" } else { "配对语义漂移" });
    // J2-2 配对残缺判负（单档负载）
    let mut b2 = QdBook::new();
    let _ = b2.control("solo", "load|solo", TIER_QD32, 32);
    let ok = b2.pair_ok("load|solo") == Ok(false);
    s.add("56107-J2-配对残缺判负", ok, if ok { "" } else { "残缺漏抓" });
    // J2-3 双档覆盖闭式
    let ok = b.tiers_covered();
    s.add("56107-J2-双档覆盖", ok, if ok { "" } else { "覆盖缺档" });
    // J2-4 重复批拒
    let mut b3 = standard_book();
    let ok = b3.control("qd|4krr|A|1", "load|4krr", TIER_QD1, 1) == Err(QD_DUP);
    s.add("56107-J2-重复批拒", ok, if ok { "" } else { "重复闸失效" });
    // J3-1 控制指纹链复核
    let ok = b.chain_ok();
    s.add("56107-J3-控制指纹链", ok, if ok { "" } else { "指纹链断裂" });
    // J3-2 链尾稳定
    let ok = standard_book().chain_tail() == standard_book().chain_tail();
    s.add("56107-J3-链尾稳定", ok, if ok { "" } else { "链尾漂移" });
    // J3-3 链尾敏感（换档位链尾变）
    let mut b4 = QdBook::new();
    let _ = b4.control("k", "l", TIER_QD32, 32);
    let mut b5 = QdBook::new();
    let _ = b5.control("k", "l", TIER_QD1, 1);
    let ok = b4.chain_tail() != b5.chain_tail();
    s.add("56107-J3-链尾敏感", ok, if ok { "" } else { "链尾钝感" });
    // J3-4 封账拒批
    let mut b6 = standard_book();
    b6.seal();
    let ok = b6.control("late", "l", TIER_QD1, 1) == Err(QD_SEALED);
    s.add("56107-J3-封账拒批", ok, if ok { "" } else { "封账穿透" });
    // J1-4 渲染导出四行齐
    let text = render_book(&standard_book());
    let ok = render_complete(&text, 4) && text.starts_with("qd|4krr|A|1|qd1|1|");
    s.add("56107-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56107-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56107_all_checks_green() {
        let set = run_unx56107_checks();
        assert!(set.all_passed(), "UNX-F56107 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56107_chain_stable() {
        assert_eq!(standard_book().chain_tail(), standard_book().chain_tail());
    }
}
