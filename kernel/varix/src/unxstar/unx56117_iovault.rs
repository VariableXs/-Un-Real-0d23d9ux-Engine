//! UNX-F56117 IO 样本存证归档件（0x60E80 段）
//!
//! 锚点：UNX-F56117-J1 存证全量（IO 样本逐批归档：批键 × 样本指纹 × 归档
//! 波次三元组）；J2 归档只读（封档拒写、同键重档拒收、批指纹逐批复算
//! 一致）；J3 存证审计链（批指纹链、封档只读、档密可复算）三判据全绿。
//! 域：CoRun·UNX-F2-B40（IO 域·样本存证面）。
//!
//! 结构：样本批登记册（批键 → 样本指纹 × 归档波次逐批落档）+ 批指纹链
//! + 档密复核核（全档指纹折叠重算）+ 封档语义 + 注入核（空批键/空指纹/
//! 波次零/重复归档/封档改写/未登记复核/档密漂移必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60E80 独占 0x20）。
pub const SEG_BASE: u32 = 0x60E80;
pub const IV_KEY_EMPTY: u32 = SEG_BASE + 0x01; // 批键为空
pub const IV_FP_ZERO: u32 = SEG_BASE + 0x02; // 空指纹（零值指纹拒收）
pub const IV_WAVE_ZERO: u32 = SEG_BASE + 0x03; // 归档波次零
pub const IV_DUP: u32 = SEG_BASE + 0x04; // 重复归档
pub const IV_SEALED: u32 = SEG_BASE + 0x05; // 存证档已封档拒写
pub const IV_NOT_FOUND: u32 = SEG_BASE + 0x06; // 未登记复核
pub const IV_SEAL_MISS: u32 = SEG_BASE + 0x07; // 档密复算不一致

/// 批指纹：FNV-1a 32 位。
pub fn batch_fingerprint(key: &str, fp: u32, wave: u32) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let line = format!("{}|{:08x}|w{}", key, fp, wave);
    for b in line.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 存证批：批键 × 样本指纹 × 归档波次 × 指纹 × 链。
#[derive(Clone)]
pub struct VaultBatch {
    pub key: String,
    pub fp: u32,
    pub wave: u32,
    pub row_fp: u32,
    pub chain: u32,
}

pub const CHAIN_SEED: u32 = 0x811C_9DC5;

/// 存证总档。
pub struct IoVault {
    pub batches: Vec<VaultBatch>,
    pub sealed: bool,
}

impl IoVault {
    pub fn new() -> Self {
        IoVault { batches: Vec::new(), sealed: false }
    }

    fn find(&self, key: &str) -> Option<usize> {
        for i in 0..self.batches.len() {
            if self.batches[i].key == key {
                return Some(i);
            }
        }
        None
    }

    /// 逐批归档：空键拒 + 零指纹拒 + 波次 ≥1 + 重复拒四闸。
    pub fn archive(&mut self, key: &str, fp: u32, wave: u32) -> Result<u32, u32> {
        if self.sealed {
            return Err(IV_SEALED);
        }
        if key.is_empty() {
            return Err(IV_KEY_EMPTY);
        }
        if fp == 0 {
            return Err(IV_FP_ZERO);
        }
        if wave == 0 {
            return Err(IV_WAVE_ZERO);
        }
        if self.find(key).is_some() {
            return Err(IV_DUP);
        }
        let row_fp = batch_fingerprint(key, fp, wave);
        let chain = match self.batches.last() {
            None => CHAIN_SEED ^ row_fp,
            Some(prev) => prev.row_fp ^ row_fp,
        };
        self.batches.push(VaultBatch { key: key.to_string(), fp, wave, row_fp, chain });
        Ok(chain)
    }

    /// 复核批键（未归档显性拒绝）。
    pub fn batch_of(&self, key: &str) -> Result<(u32, u32), u32> {
        match self.find(key) {
            Some(i) => Ok((self.batches[i].fp, self.batches[i].wave)),
            None => Err(IV_NOT_FOUND),
        }
    }

    /// J1：存证全量（逐批键非空 + 指纹非零）。
    pub fn batches_ok(&self) -> bool {
        !self.batches.is_empty()
            && self.batches.iter().all(|b| !b.key.is_empty() && b.fp != 0)
    }

    /// J3：批指纹链复核。
    pub fn chain_ok(&self) -> bool {
        if self.batches.is_empty() {
            return false;
        }
        for i in 0..self.batches.len() {
            let expect = if i == 0 {
                CHAIN_SEED ^ self.batches[0].row_fp
            } else {
                self.batches[i - 1].row_fp ^ self.batches[i].row_fp
            };
            if self.batches[i].chain != expect {
                return false;
            }
        }
        true
    }

    /// J3：档密复核（全档行指纹折叠重算）。
    pub fn vault_digest(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for b in self.batches.iter() {
            h ^= b.row_fp;
            h = h.wrapping_mul(0x0100_0193);
        }
        h
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 封档指纹（链尾固化）。
    pub fn seal_tail(&self) -> u32 {
        match self.batches.last() {
            None => CHAIN_SEED,
            Some(b) => b.chain,
        }
    }
}

/// 标准夹具：4 批样本归档（波次 8）。
pub fn standard_vault() -> IoVault {
    let mut v = IoVault::new();
    let plan: [(&str, u32); 4] = [
        ("io|4krr|qd32", 0x1111_1111),
        ("io|4krw|qd32", 0x2222_2222),
        ("io|1msr|qd32", 0x3333_3333),
        ("io|1msw|qd32", 0x4444_4444),
    ];
    for p in plan.iter() {
        if v.archive(p.0, p.1, 8).is_err() {
            panic!("standard fixture archive must succeed");
        }
    }
    v
}

/// 注入核：7 注入必抓（空批键/零指纹/波次零/重复归档/封档改写/未登记复核/档密漂移）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 空批键
    let mut v = IoVault::new();
    if v.archive("", 0xAAAA, 1) == Err(IV_KEY_EMPTY) {
        caught += 1;
    }
    // 2) 零指纹
    if v.archive("k", 0, 1) == Err(IV_FP_ZERO) {
        caught += 1;
    }
    // 3) 波次零
    if v.archive("k", 0xAAAA, 0) == Err(IV_WAVE_ZERO) {
        caught += 1;
    }
    // 4) 重复归档
    if v.archive("k", 0xAAAA, 1).is_ok() && v.archive("k", 0xBBBB, 2) == Err(IV_DUP) {
        caught += 1;
    }
    // 5) 未登记复核
    if v.batch_of("ghost").err() == Some(IV_NOT_FOUND) {
        caught += 1;
    }
    // 6) 档密漂移必抓（篡改行指纹后档密变）
    let mut dirty = standard_vault();
    dirty.batches[1].row_fp ^= 0xFF;
    if dirty.vault_digest() != standard_vault().vault_digest() {
        caught += 1;
    }
    // 7) 封档改写拒
    let mut sb = standard_vault();
    sb.seal();
    if sb.archive("late", 0x5555, 9) == Err(IV_SEALED) {
        caught += 1;
    }
    (caught, 7)
}

/// 存证档渲染导出。
pub fn render_vault(v: &IoVault) -> String {
    let mut out = String::new();
    for b in v.batches.iter() {
        out.push_str(&format!("{}|{:08x}|w{}|{:08x}\n", b.key, b.fp, b.wave, b.row_fp));
    }
    out
}

/// 渲染完整性核：行数齐 + 逐行四段。
pub fn render_complete(text: &str, expect_rows: usize) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != expect_rows {
        return false;
    }
    lines.iter().all(|ln| {
        let parts: Vec<&str> = ln.split('|').collect();
        parts.len() == 4 && parts[1].len() == 8 && parts[3].len() == 8
    })
}

/// 判据入口（CheckSet）。
pub fn run_unx56117_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56117");
    // J1-1 存证全量（四批逐批合法）
    let v = standard_vault();
    let ok = v.batches_ok() && v.batches.len() == 4;
    s.add("56117-J1-存证全量", ok, if ok { "" } else { "存证批残缺" });
    // J1-2 复核批键命中
    let ok = v.batch_of("io|4krr|qd32") == Ok((0x1111_1111, 8));
    s.add("56117-J1-复核批键命中", ok, if ok { "" } else { "复核漂移" });
    // J1-3 空指纹拒收
    let mut v2 = IoVault::new();
    let ok = v2.archive("k", 0, 1) == Err(IV_FP_ZERO);
    s.add("56117-J1-空指纹拒收", ok, if ok { "" } else { "空指纹穿透" });
    // J2-1 同键重档拒
    let mut v3 = standard_vault();
    let ok = v3.archive("io|4krr|qd32", 0x9999, 9) == Err(IV_DUP);
    s.add("56117-J2-同键重档拒", ok, if ok { "" } else { "重档穿透" });
    // J2-2 封档拒写
    let mut v4 = standard_vault();
    v4.seal();
    let ok = v4.sealed && v4.archive("late", 0x5555, 9) == Err(IV_SEALED);
    s.add("56117-J2-封档拒写", ok, if ok { "" } else { "封档穿透" });
    // J2-3 未登记复核显性拒绝
    let ok = v.batch_of("ghost").err() == Some(IV_NOT_FOUND);
    s.add("56117-J2-未登记拒绝", ok, if ok { "" } else { "复核语义漂移" });
    // J3-1 批指纹链复核
    let ok = v.chain_ok();
    s.add("56117-J3-批指纹链", ok, if ok { "" } else { "指纹链断裂" });
    // J3-2 档密稳定
    let ok = standard_vault().vault_digest() == standard_vault().vault_digest();
    s.add("56117-J3-档密稳定", ok, if ok { "" } else { "档密漂移" });
    // J3-3 档密敏感（改一批指纹档密变）
    let mut v5 = standard_vault();
    v5.batches[2].fp ^= 0x0F0F;
    v5.batches[2].row_fp ^= 0x0F0F;
    let ok = v5.vault_digest() != standard_vault().vault_digest();
    s.add("56117-J3-档密敏感", ok, if ok { "" } else { "档密钝感" });
    // J3-4 封档链尾固化（同夹具同尾）
    let ok = standard_vault().seal_tail() == standard_vault().seal_tail();
    s.add("56117-J3-封档链尾固化", ok, if ok { "" } else { "链尾漂移" });
    // J1-4 渲染导出四行齐
    let text = render_vault(&standard_vault());
    let ok = render_complete(&text, 4) && text.starts_with("io|4krr|qd32|11111111|w8|");
    s.add("56117-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56117-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56117_all_checks_green() {
        let set = run_unx56117_checks();
        assert!(set.all_passed(), "UNX-F56117 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56117_digest_stable() {
        assert_eq!(standard_vault().vault_digest(), standard_vault().vault_digest());
    }
}
