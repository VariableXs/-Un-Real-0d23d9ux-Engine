//! ves439_s23evidence — VE-F4199 · T 域归档与证据三件套
//! 归档：T 域收官证据三件套——验收证据包（评分+走查+联测）/冻结证据包（三基线哈希）/纪律证据包（变更+承诺）
//! 封装+哈希+公示位登记；包缺→补封；哈希错→重算+立案；公示缺→补登；
//! 证据包含多语言无障碍终审证据专卷（域本色）；包内证据脱敏终版。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x13A0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EvCode {
    Ok = 0,
    PackMissing = SEG_BASE + 0x01,  // 0x13A1
    HashWrong = SEG_BASE + 0x02,    // 0x13A2
    PublicityMissing = SEG_BASE + 0x03, // 0x13A3
}

/// 三件套。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EvidencePack { Acceptance = 0, Freeze = 1, Discipline = 2 }

impl EvidencePack {
    pub const ALL: [EvidencePack; 3] = [EvidencePack::Acceptance, EvidencePack::Freeze, EvidencePack::Discipline];
    pub fn name(self) -> &'static str {
        match self {
            EvidencePack::Acceptance => "acceptance", EvidencePack::Freeze => "freeze",
            EvidencePack::Discipline => "discipline",
        }
    }
}

/// 证据包（内容件 + 脱敏终版 + a11y 专卷标记——域本色 + 哈希）。
pub struct Pack {
    pub kind: EvidencePack,
    pub contents: Vec<u32>,
    pub sanitized_final: bool,
    pub a11y_volume: bool, // 多语言无障碍终审证据专卷
    pub hash: u32,
    pub filed: bool,       // 哈希错立案位
}

impl Pack {
    pub fn fnv(contents: &[u32]) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for c in contents {
            h ^= *c;
            h = h.wrapping_mul(0x0100_0193);
        }
        h
    }
    pub fn seal(&mut self) {
        self.hash = Self::fnv(&self.contents);
        self.filed = false;
    }
    pub fn hash_ok(&self) -> bool { self.hash == Self::fnv(&self.contents) }
}

/// 证据三件套册 + 公示位。
pub struct EvidenceBook {
    pub packs: Vec<Pack>,
    pub publicity: Vec<EvidencePack>, // 公示位登记
    pub reseals: u32,
    pub registrations: u32,
}

impl EvidenceBook {
    pub fn new() -> Self { EvidenceBook { packs: Vec::new(), publicity: Vec::new(), reseals: 0, registrations: 0 } }
    pub fn put(&mut self, p: Pack) {
        self.packs.retain(|x| x.kind != p.kind);
        self.packs.push(p);
    }
    pub fn pack(&self, k: EvidencePack) -> Option<&Pack> { self.packs.iter().find(|p| p.kind == k) }
    /// 归档核验：三包齐+全脱敏终版+全 a11y 专卷+哈希一致+公示登记。
    pub fn archive(&self) -> Result<(), EvCode> {
        for k in EvidencePack::ALL {
            let p = match self.pack(k) { Some(p) => p, None => return Err(EvCode::PackMissing) };
            if !p.sanitized_final || !p.a11y_volume { return Err(EvCode::PackMissing); }
            if !p.hash_ok() { return Err(EvCode::HashWrong); }
            if !self.publicity.contains(&k) { return Err(EvCode::PublicityMissing); }
        }
        Ok(())
    }
    /// 哈希错→重算+立案。
    pub fn reseal_wrong(&mut self, k: EvidencePack) -> bool {
        for p in self.packs.iter_mut() {
            if p.kind == k && !p.hash_ok() {
                p.seal();
                p.filed = true;
                self.reseals += 1;
                return true;
            }
        }
        false
    }
    pub fn register_publicity(&mut self, k: EvidencePack) {
        if !self.publicity.contains(&k) {
            self.publicity.push(k);
            self.registrations += 1;
        }
    }
}

// ───────────── 深化块 D439-A · 证据封包批账本（批次×包×体积） ─────────────

/// 一条封包入账：批次 + 包类 + 内容件数 + 体积（整数 KB）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SealRec {
    pub batch: u16,
    pub kind: EvidencePack,
    pub n_items: u32,
    pub size_kb: u32,
}

/// 封包账本：定容量 + 幂等闸（同批同类去重）+ 守恒 + 分位。
pub struct SealLedger {
    pub recs: Vec<SealRec>,
    pub cap: usize,
}

impl SealLedger {
    pub fn new(cap: usize) -> Self { SealLedger { recs: Vec::new(), cap } }
    /// 入账：满→显性 Err；同批同类重复→幂等 false。
    pub fn push(&mut self, r: SealRec) -> Result<bool, EvCode> {
        if self.recs.len() >= self.cap { return Err(EvCode::PackMissing); }
        if self.recs.iter().any(|x| x.batch == r.batch && x.kind == r.kind) { return Ok(false); }
        self.recs.push(r);
        Ok(true)
    }
    /// 守恒恒等式：∑体积 ≡ ∑(按三包类分列求和)。
    pub fn conserves(&self) -> bool {
        let total: u64 = self.recs.iter().map(|r| r.size_kb as u64).sum();
        let mut by_kind: u64 = 0;
        for k in EvidencePack::ALL {
            by_kind += self.recs.iter().filter(|r| r.kind == k)
                .map(|r| r.size_kb as u64).sum::<u64>();
        }
        total == by_kind
    }
    /// 类体积分位（整数排序取点，禁浮点）。
    pub fn kind_kb_quantile(&self, k: EvidencePack, q: u32) -> u32 {
        let mut vals: Vec<u32> = self.recs.iter().filter(|r| r.kind == k)
            .map(|r| r.size_kb).collect();
        if vals.is_empty() { return 0; }
        vals.sort_unstable();
        let n = vals.len();
        let idx = ((q as usize).max(1) * n + 99) / 100;
        vals[idx.min(n) - 1]
    }
    /// 类计数投影。
    pub fn kind_counts(&self) -> [usize; 3] {
        let mut out = [0usize; 3];
        for r in &self.recs { out[r.kind as usize] += 1; }
        out
    }
    /// 件均体积投影（整数 KB/件，禁浮点）。
    pub fn per_item_kb(&self) -> u32 {
        let items: u64 = self.recs.iter().map(|r| r.n_items as u64).sum();
        let kbs: u64 = self.recs.iter().map(|r| r.size_kb as u64).sum();
        if items == 0 { return 0; }
        (kbs / items) as u32
    }
    /// 批次去重计数。
    pub fn batch_count(&self) -> usize {
        let mut bs: Vec<u16> = self.recs.iter().map(|r| r.batch).collect();
        bs.sort_unstable();
        bs.dedup();
        bs.len()
    }
}

// ───────────── 深化块 D439-B · 边界探针 ─────────────

/// 探针 P1：越界分位号/空类→显性 Err。
pub fn probe_kb_quantile(lg: &SealLedger, k: EvidencePack, q: u32) -> Result<u32, EvCode> {
    if q == 0 || q > 100 { return Err(EvCode::HashWrong); }
    if lg.recs.iter().filter(|r| r.kind == k).count() == 0 { return Err(EvCode::PackMissing); }
    Ok(lg.kind_kb_quantile(k, q))
}

/// 探针 P2：零件包/未脱敏包不可封→显性 Err。
pub fn probe_sealable(n_items: u32, sanitized: bool) -> Result<(), EvCode> {
    if n_items == 0 || !sanitized { return Err(EvCode::PackMissing); }
    Ok(())
}

// ───────────── 深化块 D439-C · LCG 注入回归 ─────────────

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

/// 注入回归：N 轮随机篡改注入（改内容不重封），archive 必抓
/// （HashWrong 100%）；负对照：封后不改零误报。返回 (轮数, 抓获, 误报)。
pub fn lcg_tamper_regression(rounds: usize) -> (usize, usize, usize) {
    let mut g = Lcg(0x39_39_39_39);
    let mut caught = 0usize;
    let mut false_pos = 0usize;
    for _ in 0..rounds {
        let mut p = Pack { kind: EvidencePack::Acceptance, contents: vec![100, 200, 300],
            sanitized_final: true, a11y_volume: true, hash: 0, filed: false };
        p.seal();
        let idx = g.pick(3) as usize;
        p.contents[idx] += 1; // 篡改
        if !p.hash_ok() { caught += 1; } else { false_pos += 1; }
        // 负对照：重封后零误报。
        p.seal();
        if !p.hash_ok() { false_pos += 1; }
    }
    (rounds, caught, false_pos)
}

// ───────────── 深化块 D439-D · 序列化投影 + FNV 指纹 ─────────────

/// SealRec 定长 11 字节：batch(2,LE)+kind(1)+n_items(4,LE)+size(4,LE)。
pub fn encode_seal(r: &SealRec) -> Vec<u8> {
    let mut out = Vec::with_capacity(11);
    out.extend_from_slice(&r.batch.to_le_bytes());
    out.push(r.kind as u8);
    out.extend_from_slice(&r.n_items.to_le_bytes());
    out.extend_from_slice(&r.size_kb.to_le_bytes());
    out
}

/// 解码：错长/坏包号→显性 Err。
pub fn decode_seal(b: &[u8]) -> Result<SealRec, EvCode> {
    if b.len() != 11 { return Err(EvCode::PackMissing); }
    let kind = match EvidencePack::ALL.iter().find(|k| **k as u8 == b[2]) {
        Some(k) => *k,
        None => return Err(EvCode::HashWrong),
    };
    let mut batch = [0u8; 2];
    batch.copy_from_slice(&b[0..2]);
    let mut ni = [0u8; 4];
    ni.copy_from_slice(&b[3..7]);
    let mut sz = [0u8; 4];
    sz.copy_from_slice(&b[7..11]);
    Ok(SealRec { batch: u16::from_le_bytes(batch), kind,
        n_items: u32::from_le_bytes(ni), size_kb: u32::from_le_bytes(sz) })
}

/// FNV-1a 32 位（字节版，补 Pack::fnv 之外的快照指纹用）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for x in bytes { h ^= *x as u32; h = h.wrapping_mul(0x0100_0193); }
    h
}

/// 证据册快照指纹：三包哈希+公示位图+立案计数序列化后 FNV。
pub fn evidencebook_fingerprint(bk: &EvidenceBook) -> u32 {
    let mut bytes = Vec::new();
    for k in EvidencePack::ALL {
        match bk.pack(k) {
            Some(p) => {
                bytes.extend_from_slice(&p.hash.to_le_bytes());
                bytes.push(p.sanitized_final as u8);
                bytes.push(p.a11y_volume as u8);
                bytes.push(p.filed as u8);
            }
            None => bytes.extend_from_slice(&[0xBBu8; 7]),
        }
    }
    for k in EvidencePack::ALL {
        bytes.push(if bk.publicity.contains(&k) { 1 } else { 0 });
    }
    bytes.extend_from_slice(&(bk.reseals as u16).to_le_bytes());
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

pub fn run_ves439_checks() -> CheckSet {
    let mut set = CheckSet::new("ves439");
    let mut bk = EvidenceBook::new();

    // C439-01 三包缺→补封码。
    chk(&mut set, "C439-01", bk.archive() == Err(EvCode::PackMissing), "包缺补封");

    // C439-02 封三包（验收含评分/走查/联测证据；冻结含三基线哈希证据；纪律含变更+承诺证据）。
    let mut p1 = Pack { kind: EvidencePack::Acceptance, contents: vec![9301, 9302, 9303], sanitized_final: true, a11y_volume: true, hash: 0, filed: false };
    p1.seal();
    let mut p2 = Pack { kind: EvidencePack::Freeze, contents: vec![1200, 5000, 4001], sanitized_final: true, a11y_volume: true, hash: 0, filed: false };
    p2.seal();
    let mut p3 = Pack { kind: EvidencePack::Discipline, contents: vec![5001, 5002], sanitized_final: true, a11y_volume: true, hash: 0, filed: false };
    p3.seal();
    bk.put(p1); bk.put(p2); bk.put(p3);
    chk(&mut set, "C439-02", bk.archive() == Err(EvCode::PublicityMissing), "公示缺补登");

    // C439-03 公示位登记后归档全过。
    for k in EvidencePack::ALL { bk.register_publicity(k); }
    chk(&mut set, "C439-03", bk.archive().is_ok() && bk.registrations == 3, "公示登记归档过");

    // C439-04 篡改检出（改内容→哈希错）。
    bk.packs[0].contents[0] = 9999;
    chk(&mut set, "C439-04", bk.archive() == Err(EvCode::HashWrong), "篡改检出");

    // C439-05 哈希错→重算+立案留痕。
    chk(&mut set, "C439-05", bk.reseal_wrong(EvidencePack::Acceptance)
        && bk.pack(EvidencePack::Acceptance).unwrap().filed
        && bk.reseals == 1, "重算立案");

    // C439-06 重算后归档复通。
    chk(&mut set, "C439-06", bk.archive().is_ok(), "重算复通");

    // C439-07 a11y 专卷缺→补封码（域本色）。
    bk.packs[1].a11y_volume = false;
    chk(&mut set, "C439-07", bk.archive() == Err(EvCode::PackMissing), "专卷缺拒");

    // C439-08 补专卷复通。
    bk.packs[1].a11y_volume = true;
    chk(&mut set, "C439-08", bk.archive().is_ok(), "专卷补齐");

    // C439-09 公示登记幂等。
    bk.register_publicity(EvidencePack::Freeze);
    chk(&mut set, "C439-09", bk.registrations == 3 && bk.publicity.len() == 3, "公示幂等");

    // C439-10 三包名单单源。
    chk(&mut set, "C439-10", EvidencePack::ALL.len() == 3
        && EvidencePack::ALL[0].name() == "acceptance"
        && EvidencePack::ALL[2].name() == "discipline", "三包单源");

    // C439-11 FNV 哈希可复现。
    chk(&mut set, "C439-11", Pack::fnv(&[1, 2, 3]) == Pack::fnv(&[1, 2, 3])
        && Pack::fnv(&[1, 2]) != Pack::fnv(&[1, 2, 3]), "哈希可复现");

    // C439-12 脱敏终版全真断言。
    chk(&mut set, "C439-12", bk.packs.iter().all(|p| p.sanitized_final), "脱敏终版");

    // ── 深化块 D439 判据 ──
    // D439-01 封包账本：两批三包入账 + 守恒恒等式。
    let mut lg = SealLedger::new(16);
    let _ = lg.push(SealRec { batch: 1, kind: EvidencePack::Acceptance, n_items: 3, size_kb: 300 });
    let _ = lg.push(SealRec { batch: 1, kind: EvidencePack::Freeze, n_items: 3, size_kb: 150 });
    let _ = lg.push(SealRec { batch: 2, kind: EvidencePack::Discipline, n_items: 2, size_kb: 100 });
    let _ = lg.push(SealRec { batch: 2, kind: EvidencePack::Acceptance, n_items: 4, size_kb: 250 });
    chk(&mut set, "D439-01", lg.recs.len() == 4 && lg.conserves(), "封包账本守恒");

    // D439-02 幂等闸：同批同类二次提交零副作用。
    chk(&mut set, "D439-02", lg.push(SealRec { batch: 1, kind: EvidencePack::Freeze, n_items: 9, size_kb: 1 }) == Ok(false)
        && lg.recs.len() == 4, "封包幂等闸");

    // D439-03 类体积分位 + 类计数 + 件均体积。
    chk(&mut set, "D439-03", lg.kind_kb_quantile(EvidencePack::Acceptance, 50) == 250
        && lg.kind_counts() == [2, 1, 1] && lg.per_item_kb() == 125, "体积分位投影");

    // D439-04 边界探针：越界分位号/空类→显性 Err。
    chk(&mut set, "D439-04", probe_kb_quantile(&lg, EvidencePack::Freeze, 0) == Err(EvCode::HashWrong)
        && probe_kb_quantile(&lg, EvidencePack::Freeze, 101) == Err(EvCode::HashWrong), "封包分位探针");

    // D439-05 边界探针：零件包/未脱敏包拒封。
    chk(&mut set, "D439-05", probe_sealable(0, true) == Err(EvCode::PackMissing)
        && probe_sealable(3, false) == Err(EvCode::PackMissing)
        && probe_sealable(3, true).is_ok(), "封包前置探针");

    // D439-06 批次去重投影。
    chk(&mut set, "D439-06", lg.batch_count() == 2, "封包批次去重");

    // D439-07 LCG 篡改注入回归：20 轮 100% 抓 HashWrong + 负对照零误报。
    let (n, c, fp) = lcg_tamper_regression(20);
    chk(&mut set, "D439-07", n == 20 && c == 20 && fp == 0, "篡改注入全抓");

    // D439-08 序列化往返：封包记录编码→解码恒等。
    let s0 = SealRec { batch: 7, kind: EvidencePack::Discipline, n_items: 2, size_kb: 100 };
    chk(&mut set, "D439-08", decode_seal(&encode_seal(&s0)) == Ok(s0), "封包序列化往返");

    // D439-09 解码边界：错长/坏包号→显性 Err。
    chk(&mut set, "D439-09", decode_seal(&[0u8; 10]) == Err(EvCode::PackMissing)
        && decode_seal(&{ let mut v = encode_seal(&s0); v[2] = 0x0D; v }) == Err(EvCode::HashWrong), "封包解码边界");

    // D439-10 证据册快照指纹：篡改前后指纹变 + 确定性 + hex 投影。
    let mut bk2 = EvidenceBook::new();
    let mut p = Pack { kind: EvidencePack::Freeze, contents: vec![7, 8], sanitized_final: true, a11y_volume: true, hash: 0, filed: false };
    p.seal();
    bk2.put(p);
    let f0 = evidencebook_fingerprint(&bk2);
    let f0b = evidencebook_fingerprint(&bk2);
    bk2.packs[0].contents[0] += 1; // 篡改
    let f1 = evidencebook_fingerprint(&bk2);
    bk2.reseal_wrong(EvidencePack::Freeze);
    let f2 = evidencebook_fingerprint(&bk2);
    chk(&mut set, "D439-10", f0 == f0b && f0 != f1 && f1 != f2
        && fingerprint_hex(f2).len() == 8, "证据册指纹");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves439_smoke() {
        let set = run_ves439_checks();
        assert!(set.failed_count() == 0, "ves439 red: {}", set.red_items_text());
    }
}
