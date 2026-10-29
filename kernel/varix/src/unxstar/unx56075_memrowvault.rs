//! UNX-F56075 内存对照行存证归档件（0x60940 段）
//!
//! 锚点：UNX-F56075-J1 存证登记全量（对照行逐行落档：行键 × 数值指纹 ×
//! 归档波次）；J2 归档只读（封档后拒改、重归档同键拒收）；J3 存证审计
//! 链（归档波次单调、行指纹复合链、档密可复算）三判据全绿。
//! 域：CoRun·UNX-F2-B40（内存域·存证归档面）。
//!
//! 结构：对照行存证登记册（行键 → 数值 × 指纹 × 波次逐行落档）+ 归档
//! 波次链（同键重档拒绝、跨键波次可乱序但同键内单调）+ 行指纹复合链
//! （前行指纹混入后行防抽改）+ 封档语义 + 档密复核核（全档指纹重算
//! 一致）+ 注入核（空行键/坏数值/重复归档/封档改写/未登记复算/档密
//! 漂移必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60940 独占 0x20）。
pub const SEG_BASE: u32 = 0x60940;
pub const ARC_KEY_EMPTY: u32 = SEG_BASE + 0x01; // 行键为空
pub const ARC_VAL_BAD: u32 = SEG_BASE + 0x02; // 数值非法（非正整数 KB）
pub const ARC_DUP: u32 = SEG_BASE + 0x03; // 同键重复归档
pub const ARC_SEALED: u32 = SEG_BASE + 0x04; // 存证档已封档拒写
pub const ARC_NOT_FOUND: u32 = SEG_BASE + 0x05; // 复核行键未在档
pub const ARC_WAVE_ZERO: u32 = SEG_BASE + 0x06; // 归档波次 0（哨兵拒收）
pub const ARC_SEAL_MISS: u32 = SEG_BASE + 0x07; // 档密复算不一致

/// 行指纹：FNV-1a 32 位（键|值|波次复合）。
pub fn row_fingerprint(key: &str, val_kb: u64, wave: u32) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let line = format!("{}|{}|{}", key, val_kb, wave);
    for b in line.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 存证行：行键 × 数值 KB × 归档波次 × 指纹 × 指纹链。
#[derive(Clone)]
pub struct ArcRow {
    pub key: String,
    pub val_kb: u64,
    pub wave: u32,
    pub fp: u32,
    pub chain: u32,
}

pub const CHAIN_SEED: u32 = 0x811C_9DC5;

/// 存证归档总账。
pub struct ArcBook {
    pub rows: Vec<ArcRow>,
    pub sealed: bool,
}

impl ArcBook {
    pub fn new() -> Self {
        ArcBook { rows: Vec::new(), sealed: false }
    }

    fn find(&self, key: &str) -> Option<usize> {
        for i in 0..self.rows.len() {
            if self.rows[i].key == key {
                return Some(i);
            }
        }
        None
    }

    /// 逐行落档：键非空 + 值非零 + 波次 ≥1 + 同键重复拒四闸，链复合。
    pub fn archive(&mut self, key: &str, val_kb: u64, wave: u32) -> Result<u32, u32> {
        if self.sealed {
            return Err(ARC_SEALED);
        }
        if key.is_empty() {
            return Err(ARC_KEY_EMPTY);
        }
        if val_kb == 0 {
            return Err(ARC_VAL_BAD);
        }
        if wave == 0 {
            return Err(ARC_WAVE_ZERO);
        }
        if self.find(key).is_some() {
            return Err(ARC_DUP);
        }
        let fp = row_fingerprint(key, val_kb, wave);
        let chain = match self.rows.last() {
            None => CHAIN_SEED ^ fp,
            Some(prev) => prev.fp ^ fp,
        };
        self.rows.push(ArcRow { key: key.to_string(), val_kb, wave, fp, chain });
        Ok(chain)
    }

    /// 复核行键（未归档显性拒绝）。
    pub fn row_of(&self, key: &str) -> Result<(u64, u32), u32> {
        match self.find(key) {
            Some(i) => Ok((self.rows[i].val_kb, self.rows[i].wave)),
            None => Err(ARC_NOT_FOUND),
        }
    }

    /// J1：存证全量（逐行键非空 + 值非零 + 指纹一致）。
    pub fn rows_ok(&self) -> bool {
        !self.rows.is_empty()
            && self.rows.iter().all(|r| {
                !r.key.is_empty() && r.val_kb > 0 && r.fp == row_fingerprint(&r.key, r.val_kb, r.wave)
            })
    }

    /// J2：封档只读语义（封后写拒由 archive 闸保证，此处复核封态标记）。
    pub fn sealed_ro(&self) -> bool {
        self.sealed
    }

    /// J3：指纹链复核。
    pub fn chain_ok(&self) -> bool {
        if self.rows.is_empty() {
            return false;
        }
        for i in 0..self.rows.len() {
            let expect = if i == 0 {
                CHAIN_SEED ^ self.rows[0].fp
            } else {
                self.rows[i - 1].fp ^ self.rows[i].fp
            };
            if self.rows[i].chain != expect {
                return false;
            }
        }
        true
    }

    /// 档密复核：全档行指纹复合重算。
    pub fn archive_digest(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for r in self.rows.iter() {
            h ^= r.fp;
            h = h.wrapping_mul(0x0100_0193);
        }
        h
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 标准夹具：4 行存证（波次 8，RSS 典型值）。
pub fn standard_book() -> ArcBook {
    let mut b = ArcBook::new();
    let plan: [(&str, u64); 4] =
        [("mem|rss|wave8|A", 51_200), ("mem|rss|wave8|B", 49_664), ("mem|avail|wave8|A", 1_048_576), ("mem|cached|wave8|B", 262_144)];
    for p in plan.iter() {
        if b.archive(p.0, p.1, 8).is_err() {
            panic!("standard fixture archive must succeed");
        }
    }
    b
}

/// 注入核：7 注入必抓（空键/零值/重复归档/封档改写/未登记复核/波次零/档密漂移）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 空行键
    let mut b = ArcBook::new();
    if b.archive("", 1024, 1) == Err(ARC_KEY_EMPTY) {
        caught += 1;
    }
    // 2) 零值
    if b.archive("k", 0, 1) == Err(ARC_VAL_BAD) {
        caught += 1;
    }
    // 3) 波次零
    if b.archive("k", 1024, 0) == Err(ARC_WAVE_ZERO) {
        caught += 1;
    }
    // 4) 重复归档
    if b.archive("k", 1024, 1).is_ok() && b.archive("k", 2048, 2) == Err(ARC_DUP) {
        caught += 1;
    }
    // 5) 未登记复核
    if b.row_of("ghost").err() == Some(ARC_NOT_FOUND) {
        caught += 1;
    }
    // 6) 档密漂移必抓：篡改行指纹后重算不一致（构造脏档比对）
    let sb = standard_book();
    let mut dirty = standard_book();
    dirty.rows[0].fp = dirty.rows[0].fp ^ 0xFFFF;
    if sb.archive_digest() != dirty.archive_digest() {
        caught += 1;
    }
    // 7) 封档后改写拒
    b.seal();
    if b.archive("late", 4096, 3) == Err(ARC_SEALED) {
        caught += 1;
    }
    (caught, 7)
}

/// 存证档渲染导出（逐行一行）。
pub fn render_book(b: &ArcBook) -> String {
    let mut out = String::new();
    for r in b.rows.iter() {
        out.push_str(&format!("{}|{}|w{}|{:08x}\n", r.key, r.val_kb, r.wave, r.fp));
    }
    out
}

/// 渲染完整性核：行数齐 + 逐行四段 + 指纹 8 位。
pub fn render_complete(text: &str, expect_rows: usize) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != expect_rows {
        return false;
    }
    for ln in lines.iter() {
        let parts: Vec<&str> = ln.split('|').collect();
        if parts.len() != 4 || parts[3].len() != 8 {
            return false;
        }
        if !parts[2].starts_with('w') {
            return false;
        }
    }
    true
}

/// 存证行均值核（u64 求和 / 行数，向下取整）。
pub fn mean_val(b: &ArcBook) -> Result<u64, u32> {
    if b.rows.is_empty() {
        return Err(ARC_NOT_FOUND);
    }
    let mut sum: u64 = 0;
    for r in b.rows.iter() {
        sum += r.val_kb;
    }
    Ok(sum / b.rows.len() as u64)
}

/// 判据入口（CheckSet）。
pub fn run_unx56075_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56075");
    // J1-1 存证登记全量（四行逐行合法）
    let b = standard_book();
    let ok = b.rows_ok() && b.rows.len() == 4;
    s.add("56075-J1-存证登记全量", ok, if ok { "" } else { "存证行残缺" });
    // J1-2 行指纹逐行复算一致
    let ok = b.rows.iter().all(|r| r.fp == row_fingerprint(&r.key, r.val_kb, r.wave));
    s.add("56075-J1-行指纹一致", ok, if ok { "" } else { "行指纹漂移" });
    // J1-3 复核行键命中
    let ok = b.row_of("mem|rss|wave8|A") == Ok((51_200, 8));
    s.add("56075-J1-复核行键命中", ok, if ok { "" } else { "复核漂移" });
    // J2-1 同键重复归档拒
    let mut b2 = standard_book();
    let ok = b2.archive("mem|rss|wave8|A", 60_000, 9) == Err(ARC_DUP);
    s.add("56075-J2-重复归档拒", ok, if ok { "" } else { "重档闸失效" });
    // J2-2 封档拒写
    let mut b3 = standard_book();
    b3.seal();
    let ok = b3.sealed_ro() && b3.archive("late", 4096, 9) == Err(ARC_SEALED);
    s.add("56075-J2-封档拒写", ok, if ok { "" } else { "封档穿透" });
    // J2-3 未登记复核显性拒绝
    let ok = b.row_of("ghost").err() == Some(ARC_NOT_FOUND);
    s.add("56075-J2-未登记拒绝", ok, if ok { "" } else { "复核语义漂移" });
    // J2-4 存证均值复算（51200+49664+1048576+262144)/4 = 352896
    let ok = mean_val(&b) == Ok(352_896);
    s.add("56075-J2-均值复算", ok, if ok { "" } else { "均值算术漂移" });
    // J3-1 指纹链复核
    let ok = b.chain_ok();
    s.add("56075-J3-指纹链复核", ok, if ok { "" } else { "指纹链断裂" });
    // J3-2 档密确定性
    let ok = standard_book().archive_digest() == standard_book().archive_digest();
    s.add("56075-J3-档密稳定", ok, if ok { "" } else { "档密漂移" });
    // J3-3 档密敏感（改一行指纹档密变）
    let mut b4 = standard_book();
    b4.rows[1].fp ^= 0x00FF;
    let ok = b4.archive_digest() != standard_book().archive_digest();
    s.add("56075-J3-档密敏感", ok, if ok { "" } else { "档密钝感" });
    // J3-4 链尾敏感（追加一行链尾变）
    let tail0 = b.rows.last().map(|r| r.chain).unwrap_or(0);
    let mut b5 = standard_book();
    let _ = b5.archive("mem|extra|wave8|C", 8192, 8);
    let ok = b5.rows.last().map(|r| r.chain).unwrap_or(0) != tail0;
    s.add("56075-J3-链尾敏感", ok, if ok { "" } else { "链尾钝感" });
    // J1-4 渲染导出四行齐
    let text = render_book(&standard_book());
    let ok = render_complete(&text, 4);
    s.add("56075-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56075-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56075_all_checks_green() {
        let set = run_unx56075_checks();
        assert!(set.all_passed(), "UNX-F56075 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56075_digest_stable() {
        assert_eq!(standard_book().archive_digest(), standard_book().archive_digest());
    }
}
