//! ves435_s23handover — VE-F4195 · T 域移交包（U 域 F4201）
//! 移交：T 域移交包封装（总账/词典/联测/基准/安全/无障碍/观测/fuzz/文档/清账十件+冻结哈希+遗留三路清单）；
//! U 域交接面声明（一致性域承接点：交互词典/地区规则/术语库作为一致性契约源输入）；
//! 件缺→拒收；哈希不符→重封；交接面缺→补面；
//! 交接面含无障碍口径契约化专项（域本色）；包内证据已脱敏。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1320;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HvCode {
    Ok = 0,
    ItemMissing = SEG_BASE + 0x01, // 0x1321
    HashMismatch = SEG_BASE + 0x02, // 0x1322
    FaceMissing = SEG_BASE + 0x03, // 0x1323
    Unsealed = SEG_BASE + 0x04,    // 0x1324
}

/// 十件移交内容。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HandoverItem {
    Ledger = 0, Lexicon = 1, JointTest = 2, PerfBase = 3, Security = 4,
    A11y = 5, Observe = 6, Fuzz = 7, Docs = 8, Accounting = 9,
}

impl HandoverItem {
    pub const ALL: [HandoverItem; 10] = [
        HandoverItem::Ledger, HandoverItem::Lexicon, HandoverItem::JointTest, HandoverItem::PerfBase,
        HandoverItem::Security, HandoverItem::A11y, HandoverItem::Observe, HandoverItem::Fuzz,
        HandoverItem::Docs, HandoverItem::Accounting];
    pub fn name(self) -> &'static str {
        match self {
            HandoverItem::Ledger => "ledger", HandoverItem::Lexicon => "lexicon",
            HandoverItem::JointTest => "joint-test", HandoverItem::PerfBase => "perf-base",
            HandoverItem::Security => "security", HandoverItem::A11y => "a11y",
            HandoverItem::Observe => "observe", HandoverItem::Fuzz => "fuzz",
            HandoverItem::Docs => "docs", HandoverItem::Accounting => "accounting",
        }
    }
}

/// U 域交接面（一致性契约源输入，域本色承接点）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HandoverFace { LexiconContract = 0, RegionRules = 1, TermBase = 2, A11yCaliber = 3 }

impl HandoverFace {
    pub const ALL: [HandoverFace; 4] = [
        HandoverFace::LexiconContract, HandoverFace::RegionRules, HandoverFace::TermBase, HandoverFace::A11yCaliber];
}

/// 移交包。
pub struct HandoverPack {
    pub items: Vec<HandoverItem>,
    pub sanitized: bool,             // 包内证据脱敏终版
    pub faces: Vec<HandoverFace>,    // 交接面
    pub legacy_disposed: Vec<u32>,   // 遗留三路清单（已处置 id）
    pub hash: u32,
    pub sealed: bool,
}

impl HandoverPack {
    pub fn new() -> Self {
        HandoverPack { items: Vec::new(), sanitized: false, faces: Vec::new(), legacy_disposed: Vec::new(), hash: 0, sealed: false }
    }
    pub fn put(&mut self, it: HandoverItem) { if !self.items.contains(&it) { self.items.push(it); } }
    pub fn missing(&self) -> Vec<HandoverItem> {
        HandoverItem::ALL.iter().filter(|i| !self.items.contains(i)).map(|i| *i).collect()
    }
    /// 封包：十件齐+脱敏+四交接面齐+遗留全登记→哈希+封。
    pub fn seal(&mut self) -> Result<(), HvCode> {
        if self.missing().len() > 0 { return Err(HvCode::ItemMissing); }
        if !self.sanitized { return Err(HvCode::Unsealed); }
        for f in HandoverFace::ALL {
            if !self.faces.contains(&f) { return Err(HvCode::FaceMissing); }
        }
        if self.legacy_disposed.is_empty() { return Err(HvCode::Unsealed); }
        let mut h: u32 = 0x811C_9DC5;
        for it in &self.items {
            h ^= (*it) as u32; h = h.wrapping_mul(0x0100_0193);
        }
        self.hash = h;
        self.sealed = true;
        Ok(())
    }
    /// 收方核验：重算哈希与封印一致。
    pub fn verify(&self) -> Result<(), HvCode> {
        if !self.sealed { return Err(HvCode::Unsealed); }
        let mut h: u32 = 0x811C_9DC5;
        for it in &self.items {
            h ^= (*it) as u32; h = h.wrapping_mul(0x0100_0193);
        }
        if h != self.hash { return Err(HvCode::HashMismatch); }
        Ok(())
    }
    /// 重封（哈希不符路径）。
    pub fn reseal(&mut self) { self.sealed = false; self.seal().unwrap(); }
}

// ───────────── 深化块 D435-A · 交接批账本（批次×件×交接面） ─────────────

/// 一条交接入账：批次 + 件 + 交接面 + 包大小（整数 KB）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HandoverRec {
    pub batch: u16,
    pub item: HandoverItem,
    pub face: HandoverFace,
    pub size_kb: u32,
}

/// 交接批账本：定容量 + 幂等闸（同批同件去重）+ 守恒 + 分位。
pub struct HandoverLedger {
    pub recs: Vec<HandoverRec>,
    pub cap: usize,
}

impl HandoverLedger {
    pub fn new(cap: usize) -> Self { HandoverLedger { recs: Vec::new(), cap } }
    /// 入账：满→显性 Err；同批同件重复→幂等 false。
    pub fn push(&mut self, r: HandoverRec) -> Result<bool, HvCode> {
        if self.recs.len() >= self.cap { return Err(HvCode::ItemMissing); }
        if self.recs.iter().any(|x| x.batch == r.batch && x.item == r.item) { return Ok(false); }
        self.recs.push(r);
        Ok(true)
    }
    /// 守恒恒等式：∑包大小 ≡ ∑(按四交接面分列求和)。
    pub fn conserves(&self) -> bool {
        let total: u64 = self.recs.iter().map(|r| r.size_kb as u64).sum();
        let mut by_face: u64 = 0;
        for f in HandoverFace::ALL {
            by_face += self.recs.iter().filter(|r| r.face == f)
                .map(|r| r.size_kb as u64).sum::<u64>();
        }
        total == by_face
    }
    /// 面包大小分位（整数排序取点，禁浮点）。
    pub fn face_kb_quantile(&self, f: HandoverFace, q: u32) -> u32 {
        let mut vals: Vec<u32> = self.recs.iter().filter(|r| r.face == f)
            .map(|r| r.size_kb).collect();
        if vals.is_empty() { return 0; }
        vals.sort_unstable();
        let n = vals.len();
        let idx = ((q as usize).max(1) * n + 99) / 100;
        vals[idx.min(n) - 1]
    }
    /// 面计数投影。
    pub fn face_counts(&self) -> [usize; 4] {
        let mut out = [0usize; 4];
        for r in &self.recs { out[r.face as usize] += 1; }
        out
    }
    /// 件覆盖投影：已入账件数（十件为满覆盖）。
    pub fn item_coverage(&self) -> usize {
        let mut items: Vec<HandoverItem> = self.recs.iter().map(|r| r.item).collect();
        items.sort_by_key(|i| *i as u8);
        items.dedup();
        items.len()
    }
    /// 批次去重计数。
    pub fn batch_count(&self) -> usize {
        let mut bs: Vec<u16> = self.recs.iter().map(|r| r.batch).collect();
        bs.sort_unstable();
        bs.dedup();
        bs.len()
    }
}

// ───────────── 深化块 D435-B · 边界探针 ─────────────

/// 探针 P1：越界分位号/空面→显性 Err。
pub fn probe_kb_quantile(lg: &HandoverLedger, f: HandoverFace, q: u32) -> Result<u32, HvCode> {
    if q == 0 || q > 100 { return Err(HvCode::HashMismatch); }
    if lg.recs.iter().filter(|r| r.face == f).count() == 0 { return Err(HvCode::FaceMissing); }
    Ok(lg.face_kb_quantile(f, q))
}

/// 探针 P2：零包大件不可入账（空件）→显性 Err。
pub fn probe_zero_kb(r: &HandoverRec) -> Result<(), HvCode> {
    if r.size_kb == 0 { return Err(HvCode::Unsealed); }
    Ok(())
}

// ───────────── 深化块 D435-C · LCG 注入回归 ─────────────

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

/// 注入回归：N 轮随机缺件注入（十件中抽走一件），seal 必拒（100% 抓）；
/// 负对照：十件齐+全前置满足零误报。返回 (轮数, 抓获, 误报)。
pub fn lcg_missing_item_regression(rounds: usize) -> (usize, usize, usize) {
    let mut g = Lcg(0x35_35_35_35);
    let mut caught = 0usize;
    let mut false_pos = 0usize;
    for _ in 0..rounds {
        let drop = g.pick(10) as usize;
        let mut pk = HandoverPack::new();
        for (i, it) in HandoverItem::ALL.iter().enumerate() {
            if i != drop { pk.put(*it); }
        }
        pk.sanitized = true;
        for f in HandoverFace::ALL { pk.faces.push(f); }
        pk.legacy_disposed.push(1);
        if pk.seal() == Err(HvCode::ItemMissing) { caught += 1; } else { false_pos += 1; }
        // 负对照：全件齐零误报。
        let mut pk_ok = HandoverPack::new();
        for it in HandoverItem::ALL { pk_ok.put(it); }
        pk_ok.sanitized = true;
        for f in HandoverFace::ALL { pk_ok.faces.push(f); }
        pk_ok.legacy_disposed.push(1);
        if pk_ok.seal().is_err() { false_pos += 1; }
    }
    (rounds, caught, false_pos)
}

// ───────────── 深化块 D435-D · 序列化投影 + FNV 指纹 ─────────────

/// HandoverRec 定长 9 字节：batch(2,LE)+item(1)+face(1)+size(4,LE)+保留(1)。
pub fn encode_handover(r: &HandoverRec) -> Vec<u8> {
    let mut out = Vec::with_capacity(9);
    out.extend_from_slice(&r.batch.to_le_bytes());
    out.push(r.item as u8);
    out.push(r.face as u8);
    out.extend_from_slice(&r.size_kb.to_le_bytes());
    out.push(0);
    out
}

/// 解码：错长/坏件号/坏面号→显性 Err。
pub fn decode_handover(b: &[u8]) -> Result<HandoverRec, HvCode> {
    if b.len() != 9 { return Err(HvCode::ItemMissing); }
    let item = match HandoverItem::ALL.iter().find(|i| **i as u8 == b[2]) {
        Some(i) => *i,
        None => return Err(HvCode::ItemMissing),
    };
    let face = match HandoverFace::ALL.iter().find(|f| **f as u8 == b[3]) {
        Some(f) => *f,
        None => return Err(HvCode::FaceMissing),
    };
    let mut batch = [0u8; 2];
    batch.copy_from_slice(&b[0..2]);
    let mut sz = [0u8; 4];
    sz.copy_from_slice(&b[4..8]);
    Ok(HandoverRec { batch: u16::from_le_bytes(batch), item, face, size_kb: u32::from_le_bytes(sz) })
}

/// FNV-1a 32 位。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for x in bytes { h ^= *x as u32; h = h.wrapping_mul(0x0100_0193); }
    h
}

/// 移交包快照指纹：十件位图+交接面位图+遗留数序列化后 FNV。
pub fn pack_fingerprint(pk: &HandoverPack) -> u32 {
    let mut bytes = Vec::new();
    for it in HandoverItem::ALL {
        bytes.push(if pk.items.contains(&it) { 1 } else { 0 });
    }
    for f in HandoverFace::ALL {
        bytes.push(if pk.faces.contains(&f) { 1 } else { 0 });
    }
    bytes.push(pk.sanitized as u8);
    bytes.extend_from_slice(&(pk.legacy_disposed.len() as u16).to_le_bytes());
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

pub fn run_ves435_checks() -> CheckSet {
    let mut set = CheckSet::new("ves435");
    let mut pk = HandoverPack::new();

    // C435-01 件缺→拒收（缺件定位）。
    for it in HandoverItem::ALL[..8].iter() { pk.put(*it); }
    let miss = pk.missing();
    chk(&mut set, "C435-01", pk.seal() == Err(HvCode::ItemMissing)
        && miss == vec![HandoverItem::Docs, HandoverItem::Accounting], "缺两件拒收");

    // C435-02 十件齐后因未脱敏拒。
    for it in [HandoverItem::Docs, HandoverItem::Accounting] { pk.put(it); }
    chk(&mut set, "C435-02", pk.seal() == Err(HvCode::Unsealed), "未脱敏拒");

    // C435-03 脱敏后因交接面缺拒（补面路径）。
    pk.sanitized = true;
    chk(&mut set, "C435-03", pk.seal() == Err(HvCode::FaceMissing), "交接面缺拒");

    // C435-04 四交接面齐（含无障碍口径契约化——域本色）。
    for f in HandoverFace::ALL { pk.faces.push(f); }
    chk(&mut set, "C435-04", pk.seal() == Err(HvCode::Unsealed), "遗留清单缺拒");

    // C435-05 遗留三路清单登记后封包通过。
    for id in [501u32, 502] { pk.legacy_disposed.push(id); }
    chk(&mut set, "C435-05", pk.seal().is_ok() && pk.sealed && pk.hash != 0, "封包通过");

    // C435-06 收方核验哈希一致。
    chk(&mut set, "C435-06", pk.verify().is_ok(), "收方核验过");

    // C435-07 篡改检出（加件后哈希失配）。
    pk.put(HandoverItem::Ledger);
    chk(&mut set, "C435-07", pk.verify() == Err(HvCode::HashMismatch), "篡改检出");

    // C435-08 重封后核验复通（幂等件不扰动哈希）。
    pk.reseal();
    chk(&mut set, "C435-08", pk.verify().is_ok(), "重封复通");

    // C435-09 十件名单单源。
    chk(&mut set, "C435-09", HandoverItem::ALL.len() == 10
        && HandoverItem::ALL[0].name() == "ledger"
        && HandoverItem::ALL[9].name() == "accounting", "十件单源");

    // C435-10 交接面四项单源（a11y 契约化在册）。
    chk(&mut set, "C435-10", HandoverFace::ALL.len() == 4
        && HandoverFace::ALL[3] == HandoverFace::A11yCaliber, "交接面单源");

    // C435-11 U 域承接点断言：词典/地区/术语三契约源在交接面。
    chk(&mut set, "C435-11", pk.faces.contains(&HandoverFace::LexiconContract)
        && pk.faces.contains(&HandoverFace::RegionRules)
        && pk.faces.contains(&HandoverFace::TermBase), "承接点在册");

    // C435-12 件幂等（重复 put 不扰动）。
    let n = pk.items.len();
    pk.put(HandoverItem::Fuzz);
    chk(&mut set, "C435-12", pk.items.len() == n && pk.missing().is_empty(), "件幂等");

    // ── 深化块 D435 判据 ──
    // D435-01 交接批账本：两批四件入账 + 守恒恒等式。
    let mut lg = HandoverLedger::new(32);
    let _ = lg.push(HandoverRec { batch: 1, item: HandoverItem::Lexicon, face: HandoverFace::LexiconContract, size_kb: 120 });
    let _ = lg.push(HandoverRec { batch: 1, item: HandoverItem::A11y, face: HandoverFace::A11yCaliber, size_kb: 60 });
    let _ = lg.push(HandoverRec { batch: 2, item: HandoverItem::Observe, face: HandoverFace::RegionRules, size_kb: 80 });
    let _ = lg.push(HandoverRec { batch: 2, item: HandoverItem::Docs, face: HandoverFace::TermBase, size_kb: 40 });
    chk(&mut set, "D435-01", lg.recs.len() == 4 && lg.conserves(), "交接账本守恒");

    // D435-02 幂等闸：同批同件二次提交零副作用。
    chk(&mut set, "D435-02", lg.push(HandoverRec { batch: 1, item: HandoverItem::Lexicon, face: HandoverFace::TermBase, size_kb: 1 }) == Ok(false)
        && lg.recs.len() == 4, "交接幂等闸");

    // D435-03 面分位 + 面计数 + 件覆盖投影。
    chk(&mut set, "D435-03", lg.face_kb_quantile(HandoverFace::A11yCaliber, 50) == 60
        && lg.face_counts() == [1, 1, 1, 1] && lg.item_coverage() == 4, "面分位覆盖投影");

    // D435-04 边界探针：越界分位号/空面→显性 Err。
    chk(&mut set, "D435-04", probe_kb_quantile(&lg, HandoverFace::TermBase, 0) == Err(HvCode::HashMismatch)
        && probe_kb_quantile(&lg, HandoverFace::TermBase, 101) == Err(HvCode::HashMismatch), "交接分位探针");

    // D435-05 边界探针：零包大件拒入。
    chk(&mut set, "D435-05", probe_zero_kb(&HandoverRec { batch: 0, item: HandoverItem::Fuzz, face: HandoverFace::TermBase, size_kb: 0 }) == Err(HvCode::Unsealed)
        && probe_zero_kb(&HandoverRec { batch: 0, item: HandoverItem::Fuzz, face: HandoverFace::TermBase, size_kb: 1 }).is_ok(), "空件探针");

    // D435-06 批次去重投影。
    chk(&mut set, "D435-06", lg.batch_count() == 2, "交接批次去重");

    // D435-07 LCG 缺件注入回归：10 轮 100% 拒 + 负对照零误报。
    let (n, c, fp) = lcg_missing_item_regression(10);
    chk(&mut set, "D435-07", n == 10 && c == 10 && fp == 0, "缺件注入全拒");

    // D435-08 序列化往返：交接记录编码→解码恒等。
    let h0 = HandoverRec { batch: 5, item: HandoverItem::A11y, face: HandoverFace::A11yCaliber, size_kb: 256 };
    chk(&mut set, "D435-08", decode_handover(&encode_handover(&h0)) == Ok(h0), "交接序列化往返");

    // D435-09 解码边界：错长/坏件号/坏面号→显性 Err。
    chk(&mut set, "D435-09", decode_handover(&[0u8; 8]) == Err(HvCode::ItemMissing)
        && decode_handover(&{ let mut v = encode_handover(&h0); v[2] = 0x40; v }) == Err(HvCode::ItemMissing)
        && decode_handover(&{ let mut v = encode_handover(&h0); v[3] = 0x09; v }) == Err(HvCode::FaceMissing), "交接解码边界");

    // D435-10 包快照指纹：补件前后指纹变 + 确定性 + hex 投影。
    let mut pk2 = HandoverPack::new();
    pk2.put(HandoverItem::Ledger);
    let f0 = pack_fingerprint(&pk2);
    let f0b = pack_fingerprint(&pk2);
    pk2.put(HandoverItem::A11y);
    let f1 = pack_fingerprint(&pk2);
    pk2.sanitized = true;
    let f2 = pack_fingerprint(&pk2);
    chk(&mut set, "D435-10", f0 == f0b && f0 != f1 && f1 != f2
        && fingerprint_hex(f2).len() == 8, "包快照指纹");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves435_smoke() {
        let set = run_ves435_checks();
        assert!(set.failed_count() == 0, "ves435 red: {}", set.red_items_text());
    }
}
