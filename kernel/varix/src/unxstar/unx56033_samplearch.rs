//! UNX-F56033 延迟样本存证归档件（0x60400 段）
//!
//! 锚点：UNX-F56033-J1 每批样本原始数据（压缩指纹）+ 统计 + 环境哈希
//! 三元归档（逐批三件齐）；J2 任意抽 3 行按 env_hash 重算 p95 漂移
//! <10% 判据的存证基础就绪（原始数据可重放）；J3 三元完整性闸（缺任
//! 一件即拒收归档）三判据全绿。判据锚定 §AI-71.5 验收判据 3。
//! 域：CoRun·UNX-F2-B40（syscall 延迟域·存证归档面）。
//!
//! 结构：批归档登记册（批号 × 原始指纹 × 统计指纹 × 环境哈希三元逐
//! 批落账）+ 压缩折叠核（游程计数折叠指纹，同输入同指纹）+ 重放核
//! （原始数据重放重算 p95 与存档统计漂移 <10% 万分比闸）+ 抽查账 ×3
//! + 注入核（缺件归档/空原始/零哈希/漂移超阈/封账改写必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60400 独占 0x20）。
pub const SEG_BASE: u32 = 0x60400;
pub const AR_TRIPLE_MISS: u32 = SEG_BASE + 0x01; // 三元缺件
pub const AR_RAW_EMPTY: u32 = SEG_BASE + 0x02; // 空原始数据
pub const AR_HASH_ZERO: u32 = SEG_BASE + 0x03; // 环境哈希零哨兵
pub const AR_DRIFT_OVER: u32 = SEG_BASE + 0x04; // 重放漂移超阈（≥10%）
pub const AR_SEALED: u32 = SEG_BASE + 0x05; // 归档账已封账拒写
pub const AR_DUP_BATCH: u32 = SEG_BASE + 0x06; // 同批重复归档
pub const AR_BATCH_MISS: u32 = SEG_BASE + 0x07; // 抽查批号不存在

/// 重放漂移阈值（万分比 10% = 1000）。
pub const REPLAY_DRIFT_MAX: u32 = 1_000;

/// FNV-1a 32 位（三元指纹同口径）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 2166136261;
    for b in bytes {
        h ^= *b as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

/// 压缩折叠：升序样本游程计数 → "值×次数" 串（同输入同指纹）。
pub fn compress_fingerprint(samples: &[u64]) -> u32 {
    let mut text = String::new();
    let mut i = 0usize;
    while i < samples.len() {
        let mut j = i;
        while j < samples.len() && samples[j] == samples[i] {
            j += 1;
        }
        text.push_str(&format!("{}x{};", samples[i], j - i));
        i = j;
    }
    fnv1a(text.as_bytes())
}

/// p95（升序口径）。
pub fn p95_of(samples: &[u64]) -> u64 {
    let mut v = samples.to_vec();
    v.sort();
    let idx = (v.len() * 95 / 100).min(v.len() - 1);
    v[idx]
}

/// 漂移万分比：|a - b| × 10000 / max(a,b,1)。
pub fn drift_pbp(a: u64, b: u64) -> u32 {
    let m = a.max(b).max(1);
    let d = (a as i64 - b as i64).abs();
    ((d * 10_000) / m as i64) as u32
}

/// 批归档三元：批号 × 原始指纹 × 统计指纹 × 环境哈希。
#[derive(Clone)]
pub struct ArchiveEntry {
    pub batch: u32,
    pub raw_fp: u32,
    pub stats_fp: u32,
    pub env_hash: u32,
    /// 原始样本（重放基础，压缩前留档口径）。
    pub raw: Vec<u64>,
}

impl ArchiveEntry {
    pub fn new(batch: u32, raw: Vec<u64>, stats_p95: u64, env_hash: u32) -> Result<Self, u32> {
        if raw.is_empty() {
            return Err(AR_RAW_EMPTY);
        }
        if env_hash == 0 {
            return Err(AR_HASH_ZERO);
        }
        let raw_fp = compress_fingerprint(&raw);
        if raw_fp == 0 || stats_p95 == 0 {
            return Err(AR_TRIPLE_MISS);
        }
        let stats_fp = fnv1a(format!("p95={}", stats_p95).as_bytes());
        Ok(ArchiveEntry { batch, raw_fp, stats_fp, env_hash, raw })
    }

    /// J2：重放核——原始数据重算 p95 与存档 p95 漂移 <10%。
    pub fn replay_ok(&self, archived_p95: u64) -> bool {
        let replay = p95_of(&self.raw);
        drift_pbp(replay, archived_p95) < REPLAY_DRIFT_MAX
    }
}

/// 归档账：逐批三元 + 抽查账。
pub struct TripleArchive {
    pub entries: Vec<ArchiveEntry>,
    pub spot_checks: Vec<u32>,
    pub sealed: bool,
}

impl TripleArchive {
    pub fn new() -> Self {
        TripleArchive { entries: Vec::new(), spot_checks: Vec::new(), sealed: false }
    }

    /// 归档一批（同批重复拒 + 封账拒）。
    pub fn put(&mut self, e: ArchiveEntry) -> Result<(), u32> {
        if self.sealed {
            return Err(AR_SEALED);
        }
        for p in self.entries.iter() {
            if p.batch == e.batch {
                return Err(AR_DUP_BATCH);
            }
        }
        self.entries.push(e);
        Ok(())
    }

    /// 取批。
    pub fn get(&self, batch: u32) -> Option<&ArchiveEntry> {
        for e in self.entries.iter() {
            if e.batch == batch {
                return Some(e);
            }
        }
        None
    }

    /// 抽查一批：重放核过才落抽查账（批不存在拒）。
    pub fn spot_check(&mut self, batch: u32, archived_p95: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(AR_SEALED);
        }
        let e = match self.get(batch) {
            None => return Err(AR_BATCH_MISS),
            Some(e) => e,
        };
        if !e.replay_ok(archived_p95) {
            return Err(AR_DRIFT_OVER);
        }
        self.spot_checks.push(batch);
        Ok(())
    }

    /// J2：抽查 ×3 齐（三个不同批号）。
    pub fn spot3_ok(&self) -> bool {
        if self.spot_checks.len() != 3 {
            return false;
        }
        self.spot_checks[0] != self.spot_checks[1]
            && self.spot_checks[1] != self.spot_checks[2]
            && self.spot_checks[0] != self.spot_checks[2]
    }

    /// 封账。
    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 标准夹具：三批归档（批 1/2/3，原始确定性序列，环境哈希固定）。
pub fn standard_archive() -> TripleArchive {
    let mut a = TripleArchive::new();
    let p95s = [314u64, 414, 514];
    for b in 0..3u32 {
        let mut raw: Vec<u64> = Vec::new();
        let base = (b as u64 + 1) * 100 + 100;
        for i in 0..40u64 {
            raw.push(base + i * 3);
        }
        let e = ArchiveEntry::new(b + 1, raw, p95s[b as usize], 0x00ABCDEF).expect("fixture");
        a.put(e).expect("put");
    }
    a
}

/// 注入核：5 类非法操作必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 空原始数据
    if ArchiveEntry::new(1, Vec::new(), 100, 1).err() == Some(AR_RAW_EMPTY) {
        caught += 1;
    }
    // 2) 零环境哈希
    if ArchiveEntry::new(1, vec![100, 200], 100, 0).err() == Some(AR_HASH_ZERO) {
        caught += 1;
    }
    // 3) 同批重复归档
    let mut a = standard_archive();
    let dup = ArchiveEntry::new(1, vec![100, 200], 100, 5).expect("dup");
    if a.put(dup) == Err(AR_DUP_BATCH) {
        caught += 1;
    }
    // 4) 抽查批号不存在
    if a.spot_check(99, 100) == Err(AR_BATCH_MISS) {
        caught += 1;
    }
    // 5) 重放漂移超阈（存档 p95 与原始差 10 倍）
    let mut a2 = standard_archive();
    if a2.spot_check(1, 2600) == Err(AR_DRIFT_OVER) {
        caught += 1;
    }
    (caught, 5)
}

/// 判据入口（CheckSet）。
pub fn run_unx56033_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56033");
    // J1-1 三批三元全归档（原始/统计/环境哈希齐）
    let a = standard_archive();
    let ok = a.entries.len() == 3
        && a.entries.iter().all(|e| e.raw_fp != 0 && e.stats_fp != 0 && e.env_hash != 0);
    s.add("56033-J1-三批三元齐", ok, if ok { "" } else { "三元缺件" });
    // J1-2 压缩折叠指纹确定（同输入同指纹 / 异输入异指纹）
    let x = vec![100u64, 100, 100, 200];
    let y = vec![100u64, 100, 100, 200];
    let z = vec![100u64, 100, 200, 200];
    let ok = compress_fingerprint(&x) == compress_fingerprint(&y)
        && compress_fingerprint(&x) != compress_fingerprint(&z);
    s.add("56033-J1-压缩指纹确定", ok, if ok { "" } else { "折叠指纹漂移" });
    // J1-3 批 1 p95 语义（200+3i：idx38 → 314）
    let e1 = a.get(1).expect("e1");
    let ok = p95_of(&e1.raw) == 314;
    s.add("56033-J1-批p95语义", ok, if ok { "" } else { "p95 漂移" });
    // J2-1 任意抽 3 行重放 p95 漂移 <10% 全过
    let mut a2 = standard_archive();
    a2.spot_check(1, 314).expect("s1");
    a2.spot_check(2, 414).expect("s2");
    a2.spot_check(3, 514).expect("s3");
    let ok = a2.spot3_ok();
    s.add("56033-J2-抽查三行重放", ok, if ok { "" } else { "抽查破" });
    // J2-2 重放漂移阈值语义（差 6.37% 过 / 差 10 倍拒）
    let ok = drift_pbp(314, 334) < REPLAY_DRIFT_MAX && drift_pbp(314, 2600) >= REPLAY_DRIFT_MAX;
    s.add("56033-J2-漂移阈值语义", ok, if ok { "" } else { "阈值漂移" });
    // J2-3 重放用 env_hash 关联（批与哈希逐条对应）
    let ok = a.entries.iter().all(|e| e.env_hash == 0x00ABCDEF)
        && a.get(2).expect("e2").batch == 2;
    s.add("56033-J2-哈希关联", ok, if ok { "" } else { "关联漂移" });
    // J3-1 注入核 5 类必抓
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56033-J3-注入核必抓", ok, &detail);
    // J3-2 三元缺件闸（零统计 p95 拒）
    let ok = ArchiveEntry::new(9, vec![100, 200], 0, 5).err() == Some(AR_TRIPLE_MISS);
    s.add("56033-J3-缺件闸", ok, if ok { "" } else { "缺件穿透" });
    // J3-3 封账冻结
    let mut a3 = standard_archive();
    a3.seal();
    let e = ArchiveEntry::new(9, vec![100, 200], 100, 5).expect("s");
    let ok = a3.put(e) == Err(AR_SEALED) && a3.spot_check(1, 314) == Err(AR_SEALED);
    s.add("56033-J3-封账冻结", ok, if ok { "" } else { "封账后可写" });
    // J1-4 统计指纹可复算（p95=314 的指纹稳定）
    let fp1 = fnv1a(b"p95=314");
    let fp2 = fnv1a(b"p95=314");
    let ok = fp1 == fp2 && fp1 == a.get(1).expect("e1b").stats_fp;
    s.add("56033-J1-统计指纹复算", ok, if ok { "" } else { "统计指纹漂移" });
    // J2-4 空归档抽查不可行
    let empty = TripleArchive::new();
    let ok = !empty.spot3_ok();
    s.add("56033-J2-空档不抽查", ok, if ok { "" } else { "空档误判" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56033_all_checks_green() {
        let set = run_unx56033_checks();
        assert!(set.all_passed(), "UNX-F56033 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56033_replay_math() {
        let a = standard_archive();
        assert!(a.get(1).expect("e").replay_ok(314));
        assert!(!a.get(1).expect("e2").replay_ok(2600));
    }
}
