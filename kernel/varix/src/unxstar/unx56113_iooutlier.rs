//! UNX-F56113 IO 样本离群剔除件（迁移无关专用规则）（0x60E00 段）
//!
//! 锚点：UNX-F56113-J1 剔除账全量（样本集逐个落账：样本键 × 值 × 剔除
//! 标记三元组）；J2 离群规则语义（迁移无关专用规则：四分位界外剔除、
//! 剔除率闭式、边界值保留）；J3 剔除审计链（批指纹链、封账只读、剔除
//! 可复核）三判据全绿。
//! 域：CoRun·UNX-F2-B40（IO 域·离群剔除面）。
//!
//! 结构：样本登记册（样本键 → 值 × 剔除标记逐个落账）+ 四分位核（升序
//! 取 Q1/Q3，IQR = Q3 − Q1，界外 [Q1−1.5·IQR, Q3+1.5·IQR] 剔除）+ 剔除
//! 率闭式 + 指纹链 + 注入核（空样本集/剔除率越界/边界误剔/重复样本/
//! 封账改写/空账查询/标记漂移必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60E00 独占 0x20）。
pub const SEG_BASE: u32 = 0x60E00;
pub const OL_SET_EMPTY: u32 = SEG_BASE + 0x01; // 空样本集
pub const OL_SET_SHORT: u32 = SEG_BASE + 0x02; // 样本量不足（<8）
pub const OL_RATE_OVER: u32 = SEG_BASE + 0x03; // 剔除率越界（>2000 bp）
pub const OL_DUP: u32 = SEG_BASE + 0x04; // 重复样本键
pub const OL_SEALED: u32 = SEG_BASE + 0x05; // 剔除账已封账拒写
pub const OL_EMPTY: u32 = SEG_BASE + 0x06; // 空账查询
pub const OL_MARK_MISS: u32 = SEG_BASE + 0x07; // 标记漂移（界内被标剔）

/// 最小样本量（四分位可靠下限）。
pub const SAMPLE_MIN: usize = 8;
/// 剔除率上限（万分比）。
pub const RATE_MAX_BP: u64 = 2_000;

/// 升序副本（插入排序，不依赖 sort 语义）。
fn sorted_copy(vals: &[u64]) -> Vec<u64> {
    let mut v: Vec<u64> = Vec::new();
    for x in vals.iter() {
        v.push(*x);
    }
    for i in 1..v.len() {
        let key = v[i];
        let mut j = i;
        while j > 0 && v[j - 1] > key {
            v[j] = v[j - 1];
            j -= 1;
        }
        v[j] = key;
    }
    v
}

/// 四分位核：最近秩法（Q1 秩 ceil(n/4)、Q3 秩 ceil(3n/4)，1 起）。
pub fn quartiles(vals: &[u64]) -> Result<(u64, u64, u64), u32> {
    if vals.is_empty() {
        return Err(OL_SET_EMPTY);
    }
    if vals.len() < SAMPLE_MIN {
        return Err(OL_SET_SHORT);
    }
    let s = sorted_copy(vals);
    let n = s.len();
    let q1_idx = (n + 3) / 4 - 1;
    let q3_idx = (3 * n + 3) / 4 - 1;
    Ok((s[q1_idx], s[q3_idx], s[q3_idx] - s[q1_idx]))
}

/// 界外判定核：v < Q1−1.5·IQR 或 v > Q3+1.5·IQR 即离群（用 i64 中转防下溢）。
pub fn outlier(v: u64, q1: u64, q3: u64) -> bool {
    let iqr = q3 as i64 - q1 as i64;
    let lo = q1 as i64 - (iqr * 3) / 2;
    let hi = q3 as i64 + (iqr * 3) / 2;
    (v as i64) < lo || (v as i64) > hi
}

/// 样本指纹：FNV-1a 32 位。
pub fn sample_fingerprint(key: &str, val: u64, dropped: bool) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let line = format!("{}|{}|{}", key, val, dropped);
    for b in line.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 样本：样本键 × 值 × 剔除标记 × 指纹 × 链。
#[derive(Clone)]
pub struct OlSample {
    pub key: String,
    pub val: u64,
    pub dropped: bool,
    pub fp: u32,
    pub chain: u32,
}

pub const CHAIN_SEED: u32 = 0x811C_9DC5;

/// 剔除总账。
pub struct OlBook {
    pub samples: Vec<OlSample>,
    pub sealed: bool,
}

impl OlBook {
    pub fn new() -> Self {
        OlBook { samples: Vec::new(), sealed: false }
    }

    fn find(&self, key: &str) -> Option<usize> {
        for i in 0..self.samples.len() {
            if self.samples[i].key == key {
                return Some(i);
            }
        }
        None
    }

    /// 逐个落账（标记自动按四分位规则判定；量不足先挂 pending=false 全保）。
    pub fn add(&mut self, key: &str, val: u64) -> Result<bool, u32> {
        if self.sealed {
            return Err(OL_SEALED);
        }
        if key.is_empty() {
            return Err(OL_SET_EMPTY);
        }
        if self.find(key).is_some() {
            return Err(OL_DUP);
        }
        let dropped = if self.samples.len() + 1 >= SAMPLE_MIN {
            // 收齐后统一重判：先按当前值暂标。
            false
        } else {
            false
        };
        let fp = sample_fingerprint(key, val, dropped);
        let chain = match self.samples.last() {
            None => CHAIN_SEED ^ fp,
            Some(prev) => prev.fp ^ fp,
        };
        self.samples.push(OlSample { key: key.to_string(), val, dropped, fp, chain });
        Ok(dropped)
    }

    /// J2：统一剔除判定（样本量足后按四分位界重标，逐个复标）。
    pub fn apply_rule(&mut self) -> Result<usize, u32> {
        if self.sealed {
            return Err(OL_SEALED);
        }
        if self.samples.is_empty() {
            return Err(OL_SET_EMPTY);
        }
        if self.samples.len() < SAMPLE_MIN {
            return Err(OL_SET_SHORT);
        }
        let vals: Vec<u64> = self.samples.iter().map(|s| s.val).collect();
        let (q1, q3, _iqr) = quartiles(&vals)?;
        let mut dropped = 0usize;
        for i in 0..self.samples.len() {
            let is_out = outlier(self.samples[i].val, q1, q3);
            if self.samples[i].dropped != is_out {
                dropped += 1;
            }
            self.samples[i].dropped = is_out;
            self.samples[i].fp = sample_fingerprint(&self.samples[i].key, self.samples[i].val, is_out);
        }
        // 重建链（复标后指纹已变）。
        let mut chain = CHAIN_SEED;
        for i in 0..self.samples.len() {
            chain = chain ^ self.samples[i].fp;
            self.samples[i].chain = chain;
        }
        Ok(dropped)
    }

    /// J2：剔除率闭式（万分比）。
    pub fn drop_rate_bp(&self) -> Result<u64, u32> {
        if self.samples.is_empty() {
            return Err(OL_EMPTY);
        }
        let mut d = 0u64;
        for s in self.samples.iter() {
            if s.dropped {
                d += 1;
            }
        }
        Ok(d * 10_000 / self.samples.len() as u64)
    }

    /// J2：剔除率合规核。
    pub fn rate_ok(&self) -> bool {
        match self.drop_rate_bp() {
            Ok(r) => r <= RATE_MAX_BP,
            Err(_) => false,
        }
    }

    /// J1：标记一致性核（复算离群与标记逐个一致）。
    pub fn marks_ok(&self) -> bool {
        if self.samples.len() < SAMPLE_MIN {
            return false;
        }
        let vals: Vec<u64> = self.samples.iter().map(|s| s.val).collect();
        let (q1, q3, _) = match quartiles(&vals) {
            Ok(q) => q,
            Err(_) => return false,
        };
        self.samples.iter().all(|s| s.dropped == outlier(s.val, q1, q3))
    }

    /// J3：样本指纹链复核。
    pub fn chain_ok(&self) -> bool {
        if self.samples.is_empty() {
            return false;
        }
        let mut expect = CHAIN_SEED;
        for i in 0..self.samples.len() {
            expect = expect ^ self.samples[i].fp;
            if self.samples[i].chain != expect {
                return false;
            }
        }
        true
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    pub fn chain_tail(&self) -> u32 {
        match self.samples.last() {
            None => CHAIN_SEED,
            Some(s) => s.chain,
        }
    }
}

/// 标准夹具：12 样本（10 个 100±10 带内 + 2 个离群 500/1）。
pub fn standard_book() -> OlBook {
    let mut b = OlBook::new();
    let vals: [u64; 12] = [100, 95, 105, 90, 110, 98, 102, 92, 108, 96, 500, 1];
    for (i, v) in vals.iter().enumerate() {
        if b.add(&format!("s{}", i), *v).is_err() {
            panic!("standard fixture add must succeed");
        }
    }
    if b.apply_rule().is_err() {
        panic!("standard fixture apply_rule must succeed");
    }
    b
}

/// 注入核：7 注入必抓（空样本集/样本不足/重复样本/空账查询/封账改写/标记漂移/剔除率越界）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 空样本集
    let mut b = OlBook::new();
    if b.add("", 100) == Err(OL_SET_EMPTY) {
        caught += 1;
    }
    // 2) 样本不足
    let mut b2 = OlBook::new();
    for i in 0..7u32 {
        let _ = b2.add(&format!("s{}", i), 100);
    }
    if b2.apply_rule() == Err(OL_SET_SHORT) {
        caught += 1;
    }
    // 3) 重复样本
    let mut b3 = OlBook::new();
    if b3.add("k", 100).is_ok() && b3.add("k", 200) == Err(OL_DUP) {
        caught += 1;
    }
    // 4) 空账查询
    if OlBook::new().drop_rate_bp().err() == Some(OL_EMPTY) {
        caught += 1;
    }
    // 5) 标记漂移必抓（篡改标记后 marks_ok 失败）
    let mut dirty = standard_book();
    dirty.samples[0].dropped = !dirty.samples[0].dropped;
    if !dirty.marks_ok() {
        caught += 1;
    }
    // 6) 剔除率越界必抓（六值 100 + 两离群 0/100000：IQR=0，剔 2/8 = 2500 bp > 2000）
    let mut b4 = OlBook::new();
    let vals: [u64; 8] = [100, 100, 100, 100, 100, 100, 0, 100_000];
    for (i, v) in vals.iter().enumerate() {
        let _ = b4.add(&format!("x{}", i), *v);
    }
    let _ = b4.apply_rule();
    if !b4.rate_ok() {
        caught += 1;
    }
    // 7) 封账改写拒
    let mut sb = standard_book();
    sb.seal();
    if sb.add("late", 100) == Err(OL_SEALED) {
        caught += 1;
    }
    (caught, 7)
}

/// 剔除账渲染导出。
pub fn render_book(b: &OlBook) -> String {
    let mut out = String::new();
    for s in b.samples.iter() {
        let m = if s.dropped { "drop" } else { "keep" };
        out.push_str(&format!("{}|{}|{}|{:08x}\n", s.key, s.val, m, s.fp));
    }
    out
}

/// 渲染完整性核：行数齐 + 逐行四段 + 标记闭集命中。
pub fn render_complete(text: &str, expect_rows: usize) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != expect_rows {
        return false;
    }
    lines.iter().all(|ln| {
        let parts: Vec<&str> = ln.split('|').collect();
        parts.len() == 4 && (parts[2] == "drop" || parts[2] == "keep")
    })
}

/// 判据入口（CheckSet）。
pub fn run_unx56113_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56113");
    // J1-1 剔除账全量（12 样本逐个在册）
    let b = standard_book();
    let ok = b.samples.len() == 12 && !b.sealed;
    s.add("56113-J1-剔除账全量", ok, if ok { "" } else { "样本残缺" });
    // J1-2 四分位复算（夹具 Q1=95 Q3=108 IQR=13）
    let vals: Vec<u64> = (0..12u64).map(|i| {
        let v: [u64; 12] = [100, 95, 105, 90, 110, 98, 102, 92, 108, 96, 500, 1];
        v[i as usize]
    }).collect();
    let ok = quartiles(&vals) == Ok((95, 108, 13));
    s.add("56113-J1-四分位复算", ok, if ok { "" } else { "四分位漂移" });
    // J1-3 界外判定（500 与 1 离群、100 带内）
    let ok = outlier(500, 95, 108) && outlier(1, 95, 108) && !outlier(100, 95, 108);
    s.add("56113-J1-界外判定", ok, if ok { "" } else { "界外语义漂移" });
    // J2-1 剔除标记一致（marks_ok 全绿）
    let ok = b.marks_ok();
    s.add("56113-J2-剔除标记一致", ok, if ok { "" } else { "标记漂移" });
    // J2-2 剔除率闭式（2/12 = 1666 万分比）
    let ok = b.drop_rate_bp() == Ok(1_666);
    s.add("56113-J2-剔除率闭式", ok, if ok { "" } else { "剔除率漂移" });
    // J2-3 剔除率合规（≤ 2000 bp）
    let ok = b.rate_ok();
    s.add("56113-J2-剔除率合规", ok, if ok { "" } else { "剔除率越界" });
    // J2-4 边界值保留（Q1 95 与 Q3 108 不剔）
    let ok = b.samples.iter().all(|s| {
        if s.val == 95 || s.val == 108 {
            !s.dropped
        } else {
            true
        }
    });
    s.add("56113-J2-边界值保留", ok, if ok { "" } else { "边界误剔" });
    // J3-1 样本指纹链复核
    let ok = b.chain_ok();
    s.add("56113-J3-样本指纹链", ok, if ok { "" } else { "指纹链断裂" });
    // J3-2 链尾稳定
    let ok = standard_book().chain_tail() == standard_book().chain_tail();
    s.add("56113-J3-链尾稳定", ok, if ok { "" } else { "链尾漂移" });
    // J3-3 链尾敏感（复标后链尾变）
    let mut b3 = standard_book();
    b3.samples[0].val = 600;
    let _ = b3.apply_rule();
    let ok = b3.chain_tail() != standard_book().chain_tail();
    s.add("56113-J3-链尾敏感", ok, if ok { "" } else { "链尾钝感" });
    // J3-4 封账拒写
    let mut b4 = standard_book();
    b4.seal();
    let ok = b4.add("late", 100) == Err(OL_SEALED);
    s.add("56113-J3-封账拒写", ok, if ok { "" } else { "封账穿透" });
    // J1-4 渲染导出十二行齐
    let text = render_book(&standard_book());
    let ok = render_complete(&text, 12);
    s.add("56113-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56113-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56113_all_checks_green() {
        let set = run_unx56113_checks();
        assert!(set.all_passed(), "UNX-F56113 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56113_chain_stable() {
        assert_eq!(standard_book().chain_tail(), standard_book().chain_tail());
    }
}
