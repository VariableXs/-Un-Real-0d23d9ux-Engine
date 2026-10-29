//! ves437_s23knowledge — VE-F4197 · T 域知识库与经验归档
//! 归档：T 域收官知识归档（终审模式/联测案例/走查发现/变更案例四类入家族知识库+与 F4179 自查档合并去重）
//! +检索索引；重复条→去重合并；索引断→重建；类缺→补档；
//! 归档含多语种走查方法沉淀（域本色）；案例脱敏。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1360;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KbCode {
    Ok = 0,
    ClassMissing = SEG_BASE + 0x01, // 0x1361
    IndexBroken = SEG_BASE + 0x02,  // 0x1362
    NotSanitized = SEG_BASE + 0x03, // 0x1363
}

/// 四类归档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArchiveClass { FinalMode = 0, JointCase = 1, WalkFinding = 2, ChangeCase = 3 }

impl ArchiveClass {
    pub const ALL: [ArchiveClass; 4] = [
        ArchiveClass::FinalMode, ArchiveClass::JointCase, ArchiveClass::WalkFinding, ArchiveClass::ChangeCase];
}

/// 知识条目（案例脱敏标记 + 多语种方法沉淀标记——域本色）。
pub struct KbEntry {
    pub key: String,
    pub class: ArchiveClass,
    pub sanitized: bool,
    pub ml_method: bool, // 多语种走查方法沉淀（域本色）
}

/// 知识归档册。
pub struct KnowledgeBook {
    pub entries: Vec<KbEntry>,
    pub index: Vec<String>,     // 检索索引（key 列表）
    pub merged_from_f4179: Vec<String>, // 与 F4179 自查档合并去重记录
}

impl KnowledgeBook {
    pub fn new() -> Self { KnowledgeBook { entries: Vec::new(), index: Vec::new(), merged_from_f4179: Vec::new() } }
    pub fn put(&mut self, key: &str, class: ArchiveClass, sanitized: bool, ml: bool) {
        let key = String::from(key);
        if !self.entries.iter().any(|e| e.key == key) {
            self.entries.push(KbEntry { key: key.clone(), class, sanitized, ml_method: ml });
        }
    }
    /// 四类齐备核验。
    pub fn classes_full(&self) -> Result<(), KbCode> {
        for c in ArchiveClass::ALL {
            if !self.entries.iter().any(|e| e.class == c) { return Err(KbCode::ClassMissing); }
        }
        Ok(())
    }
    /// 建索引（key 排序稳定）。
    pub fn build_index(&mut self) {
        let mut keys: Vec<String> = self.entries.iter().map(|e| e.key.clone()).collect();
        keys.sort();
        self.index = keys;
    }
    /// 索引完整性核验（断检→重建码）。
    pub fn index_intact(&self) -> Result<(), KbCode> {
        if self.index.len() != self.entries.len() { return Err(KbCode::IndexBroken); }
        let mut sorted: Vec<&String> = self.entries.iter().map(|e| &e.key).collect();
        sorted.sort();
        for (i, k) in sorted.iter().enumerate() {
            if *k != &self.index[i] { return Err(KbCode::IndexBroken); }
        }
        Ok(())
    }
    /// 检索：按 key 前缀。
    pub fn search(&self, prefix: &str) -> Vec<&str> {
        self.index.iter().filter(|k| k.starts_with(prefix)).map(|k| k.as_str()).collect()
    }
    /// 与 F4179 自查档合并去重：返回实际新增数。
    pub fn merge_dedup(&mut self, f4179_keys: &[&str], class: ArchiveClass) -> usize {
        let mut added = 0;
        for k in f4179_keys {
            if !self.entries.iter().any(|e| e.key == *k) {
                self.entries.push(KbEntry { key: String::from(*k), class, sanitized: true, ml_method: false });
                self.merged_from_f4179.push(String::from(*k));
                added += 1;
            }
        }
        added
    }
    /// 案例脱敏核验：全部条目须脱敏（红线）。
    pub fn all_sanitized(&self) -> bool { self.entries.iter().all(|e| e.sanitized) }
    /// 多语种方法沉淀在册（域本色）。
    pub fn ml_method_present(&self) -> bool { self.entries.iter().any(|e| e.ml_method) }
}

// ───────────── 深化块 D437-A · 归档批账本（批次×类×检索热度） ─────────────

/// 一条归档入账：批次 + 类 + key 长度 + 检索热度（被查次数）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ArchiveRec {
    pub batch: u16,
    pub class: ArchiveClass,
    pub key_len: u16,
    pub hits: u32,
}

/// 归档批账本：定容量 + 幂等闸（同批同 key_len 同类去重）+ 守恒 + 分位。
pub struct ArchiveLedger {
    pub recs: Vec<ArchiveRec>,
    pub cap: usize,
}

impl ArchiveLedger {
    pub fn new(cap: usize) -> Self { ArchiveLedger { recs: Vec::new(), cap } }
    /// 入账：满→显性 Err；同键重复→幂等 false。
    pub fn push(&mut self, r: ArchiveRec) -> Result<bool, KbCode> {
        if self.recs.len() >= self.cap { return Err(KbCode::IndexBroken); }
        if self.recs.iter().any(|x| x.batch == r.batch && x.key_len == r.key_len && x.class == r.class) {
            return Ok(false);
        }
        self.recs.push(r);
        Ok(true)
    }
    /// 守恒恒等式：∑检索热度 ≡ ∑(按四类分列求和)。
    pub fn conserves(&self) -> bool {
        let total: u64 = self.recs.iter().map(|r| r.hits as u64).sum();
        let mut by_class: u64 = 0;
        for c in ArchiveClass::ALL {
            by_class += self.recs.iter().filter(|r| r.class == c)
                .map(|r| r.hits as u64).sum::<u64>();
        }
        total == by_class
    }
    /// 类热度分位（整数排序取点，禁浮点）。
    pub fn class_hits_quantile(&self, c: ArchiveClass, q: u32) -> u32 {
        let mut vals: Vec<u32> = self.recs.iter().filter(|r| r.class == c)
            .map(|r| r.hits).collect();
        if vals.is_empty() { return 0; }
        vals.sort_unstable();
        let n = vals.len();
        let idx = ((q as usize).max(1) * n + 99) / 100;
        vals[idx.min(n) - 1]
    }
    /// 类计数投影。
    pub fn class_counts(&self) -> [usize; 4] {
        let mut out = [0usize; 4];
        for r in &self.recs { out[r.class as usize] += 1; }
        out
    }
    /// 批次去重计数。
    pub fn batch_count(&self) -> usize {
        let mut bs: Vec<u16> = self.recs.iter().map(|r| r.batch).collect();
        bs.sort_unstable();
        bs.dedup();
        bs.len()
    }
}

// ───────────── 深化块 D437-B · 边界探针 ─────────────

/// 探针 P1：越界分位号/空类→显性 Err。
pub fn probe_hits_quantile(lg: &ArchiveLedger, c: ArchiveClass, q: u32) -> Result<u32, KbCode> {
    if q == 0 || q > 100 { return Err(KbCode::NotSanitized); }
    if lg.recs.iter().filter(|r| r.class == c).count() == 0 { return Err(KbCode::ClassMissing); }
    Ok(lg.class_hits_quantile(c, q))
}

/// 探针 P2：未脱敏条目拒入归档（红线）→显性 Err。
pub fn probe_unsanitized(sanitized: bool) -> Result<(), KbCode> {
    if !sanitized { return Err(KbCode::NotSanitized); }
    Ok(())
}

// ───────────── 深化块 D437-C · LCG 注入回归 ─────────────

/// 8 行确定性 LCG。
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    fn pick(&mut self, n: u32) -> u32 { ((self.next() >> 33) as u32) % n }
}

/// 注入回归：N 轮随机抽走一类条目（四类缺一），classes_full 必抓
/// （ClassMissing 100%）；负对照：四类齐零误报。返回 (轮数, 抓获, 误报)。
pub fn lcg_class_gap_regression(rounds: usize) -> (usize, usize, usize) {
    let mut g = Lcg(0x37_37_37_37);
    let mut caught = 0usize;
    let mut false_pos = 0usize;
    for _ in 0..rounds {
        let drop = g.pick(4) as usize;
        let mut bk = KnowledgeBook::new();
        for (i, c) in ArchiveClass::ALL.iter().enumerate() {
            if i != drop { bk.put("k-class", *c, true, false); }
        }
        if bk.classes_full() == Err(KbCode::ClassMissing) { caught += 1; } else { false_pos += 1; }
        // 负对照：四类齐零误报。
        let mut bk_ok = KnowledgeBook::new();
        for c in ArchiveClass::ALL { bk_ok.put("k-class", c, true, false); }
        if bk_ok.classes_full().is_err() { false_pos += 1; }
    }
    (rounds, caught, false_pos)
}

// ───────────── 深化块 D437-D · 序列化投影 + FNV 指纹 ─────────────

/// ArchiveRec 定长 9 字节：batch(2,LE)+class(1)+key_len(2,LE)+hits(4,LE)。
pub fn encode_archive(r: &ArchiveRec) -> Vec<u8> {
    let mut out = Vec::with_capacity(9);
    out.extend_from_slice(&r.batch.to_le_bytes());
    out.push(r.class as u8);
    out.extend_from_slice(&r.key_len.to_le_bytes());
    out.extend_from_slice(&r.hits.to_le_bytes());
    out
}

/// 解码：错长/坏类号→显性 Err。
pub fn decode_archive(b: &[u8]) -> Result<ArchiveRec, KbCode> {
    if b.len() != 9 { return Err(KbCode::IndexBroken); }
    let class = match ArchiveClass::ALL.iter().find(|c| **c as u8 == b[2]) {
        Some(c) => *c,
        None => return Err(KbCode::ClassMissing),
    };
    let mut batch = [0u8; 2];
    batch.copy_from_slice(&b[0..2]);
    let mut kl = [0u8; 2];
    kl.copy_from_slice(&b[3..5]);
    let mut hits = [0u8; 4];
    hits.copy_from_slice(&b[5..9]);
    Ok(ArchiveRec { batch: u16::from_le_bytes(batch), class,
        key_len: u16::from_le_bytes(kl), hits: u32::from_le_bytes(hits) })
}

/// FNV-1a 32 位。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for x in bytes { h ^= *x as u32; h = h.wrapping_mul(0x0100_0193); }
    h
}

/// 归档册快照指纹：条目 key/类/脱敏/沉淀位序列化后 FNV。
pub fn archive_fingerprint(bk: &KnowledgeBook) -> u32 {
    let mut bytes = Vec::new();
    for e in &bk.entries {
        bytes.extend_from_slice(e.key.as_bytes());
        bytes.push(e.class as u8);
        bytes.push(e.sanitized as u8);
        bytes.push(e.ml_method as u8);
    }
    bytes.extend_from_slice(&(bk.index.len() as u16).to_le_bytes());
    fnv1a(&bytes)
}

/// 指纹十六进制投影。
pub fn fingerprint_hex(h: u32) -> String {
    let mut out = String::new();
    for i in 0..8 {
        let nib = ((h >> (28 - i * 4)) & 0xF) as u8;
        out.push(if nib < 10 { (b'0' + nib) as char } else { (b'a' + nib - 10) as char });
    }
    out
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves437_checks() -> CheckSet {
    let mut set = CheckSet::new("ves437");
    let mut bk = KnowledgeBook::new();

    // C437-01 类缺→补档码。
    bk.put("mode-final-a11y", ArchiveClass::FinalMode, true, true);
    chk(&mut set, "C437-01", bk.classes_full() == Err(KbCode::ClassMissing), "类缺补档");

    // C437-02 四类齐→核验过。
    bk.put("joint-case-12", ArchiveClass::JointCase, true, false);
    bk.put("walk-finding-rtl", ArchiveClass::WalkFinding, true, true);
    bk.put("change-case-freeze", ArchiveClass::ChangeCase, true, false);
    chk(&mut set, "C437-02", bk.classes_full().is_ok() && bk.entries.len() == 4, "四类齐");

    // C437-03 建索引+完整核验。
    bk.build_index();
    chk(&mut set, "C437-03", bk.index_intact().is_ok() && bk.index.len() == 4, "索引完整");

    // C437-04 索引断检→重建码。
    bk.index.pop();
    chk(&mut set, "C437-04", bk.index_intact() == Err(KbCode::IndexBroken), "断检定位");

    // C437-05 重建索引复验过。
    bk.build_index();
    chk(&mut set, "C437-05", bk.index_intact().is_ok(), "重建复验");

    // C437-06 前缀检索。
    let hits = bk.search("walk-");
    chk(&mut set, "C437-06", hits == vec!["walk-finding-rtl"], "前缀检索");

    // C437-07 与 F4179 合并去重：新 2 旧 1 → 仅新增 2。
    let added = bk.merge_dedup(&["mode-final-a11y", "f4179-self-1", "f4179-self-2"], ArchiveClass::FinalMode);
    chk(&mut set, "C437-07", added == 2 && bk.entries.len() == 6, "合并去重");

    // C437-08 合并记录留痕。
    chk(&mut set, "C437-08", bk.merged_from_f4179 == vec!["f4179-self-1", "f4179-self-2"], "合并留痕");

    // C437-09 条目幂等（重 key 不重入）。
    bk.put("mode-final-a11y", ArchiveClass::FinalMode, true, true);
    chk(&mut set, "C437-09", bk.entries.len() == 6, "条目幂等");

    // C437-10 案例脱敏全真核验（红线）。
    chk(&mut set, "C437-10", bk.all_sanitized(), "脱敏全真");

    // C437-11 多语种方法沉淀在册（域本色）。
    chk(&mut set, "C437-11", bk.ml_method_present(), "方法沉淀在册");

    // C437-12 四类单源断言。
    chk(&mut set, "C437-12", ArchiveClass::ALL.len() == 4
        && ArchiveClass::ALL[3] == ArchiveClass::ChangeCase, "四类单源");

    // ── 深化块 D437 判据 ──
    // D437-01 归档批账本：两批四类入账 + 守恒恒等式。
    let mut lg = ArchiveLedger::new(32);
    let _ = lg.push(ArchiveRec { batch: 1, class: ArchiveClass::FinalMode, key_len: 15, hits: 30 });
    let _ = lg.push(ArchiveRec { batch: 1, class: ArchiveClass::JointCase, key_len: 13, hits: 20 });
    let _ = lg.push(ArchiveRec { batch: 2, class: ArchiveClass::WalkFinding, key_len: 16, hits: 45 });
    let _ = lg.push(ArchiveRec { batch: 2, class: ArchiveClass::ChangeCase, key_len: 18, hits: 5 });
    chk(&mut set, "D437-01", lg.recs.len() == 4 && lg.conserves(), "归档账本守恒");

    // D437-02 幂等闸：同键二次提交零副作用。
    chk(&mut set, "D437-02", lg.push(ArchiveRec { batch: 1, class: ArchiveClass::FinalMode, key_len: 15, hits: 999 }) == Ok(false)
        && lg.recs.len() == 4, "归档幂等闸");

    // D437-03 类热度分位 + 类计数 + 批次去重。
    chk(&mut set, "D437-03", lg.class_hits_quantile(ArchiveClass::WalkFinding, 50) == 45
        && lg.class_counts() == [1, 1, 1, 1] && lg.batch_count() == 2, "热度分位投影");

    // D437-04 边界探针：越界分位号/空类→显性 Err。
    chk(&mut set, "D437-04", probe_hits_quantile(&lg, ArchiveClass::FinalMode, 0) == Err(KbCode::NotSanitized)
        && probe_hits_quantile(&lg, ArchiveClass::FinalMode, 101) == Err(KbCode::NotSanitized), "归档分位探针");

    // D437-05 边界探针：未脱敏条目拒入（红线）。
    chk(&mut set, "D437-05", probe_unsanitized(false) == Err(KbCode::NotSanitized)
        && probe_unsanitized(true).is_ok(), "未脱敏拒入探针");

    // D437-06 LCG 类缺注入回归：12 轮 100% 抓 ClassMissing + 零误报。
    let (n, c, fp) = lcg_class_gap_regression(12);
    chk(&mut set, "D437-06", n == 12 && c == 12 && fp == 0, "类缺注入全抓");

    // D437-07 序列化往返：归档记录编码→解码恒等。
    let a0 = ArchiveRec { batch: 4, class: ArchiveClass::WalkFinding, key_len: 16, hits: 45 };
    chk(&mut set, "D437-07", decode_archive(&encode_archive(&a0)) == Ok(a0), "归档序列化往返");

    // D437-08 解码边界：错长/坏类号→显性 Err。
    chk(&mut set, "D437-08", decode_archive(&[0u8; 8]) == Err(KbCode::IndexBroken)
        && decode_archive(&{ let mut v = encode_archive(&a0); v[2] = 0x0C; v }) == Err(KbCode::ClassMissing), "归档解码边界");

    // D437-09 归档册快照指纹：建索引前后指纹变 + 确定性 + hex 投影。
    let mut bk2 = KnowledgeBook::new();
    bk2.put("a-entry", ArchiveClass::FinalMode, true, true);
    bk2.put("b-entry", ArchiveClass::JointCase, true, false);
    let f0 = archive_fingerprint(&bk2);
    let f0b = archive_fingerprint(&bk2);
    bk2.build_index();
    let f1 = archive_fingerprint(&bk2);
    chk(&mut set, "D437-09", f0 == f0b && f0 != f1
        && fingerprint_hex(f1).len() == 8, "归档册指纹");

    // D437-10 前缀检索投影加深：多前缀命中计数一致性。
    bk2.put("a-walk-ml", ArchiveClass::WalkFinding, true, true);
    bk2.build_index();
    let hits_a = bk2.search("a-");
    chk(&mut set, "D437-10", hits_a.len() == 2 && bk2.search("zzz").is_empty()
        && bk2.index_intact().is_ok(), "多前缀检索一致");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves437_smoke() {
        let set = run_ves437_checks();
        assert!(set.failed_count() == 0, "ves437 red: {}", set.red_items_text());
    }
}
