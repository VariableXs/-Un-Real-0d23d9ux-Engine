//! ves403_s22t04t06 — VE-F4163 · T04-T06 组自查核验
//! 三组逐项核验：T04 翻译流水线复测（抽取/记忆/机翻/编辑/LQA 五段交接签名 + Critical 清零复核）；
//! T05 资产链复测（打包五检 + 热更新原子性 + 字形覆盖）；
//! T06 地区引擎复测（驻留路由白名单 + 合规库覆盖度 + 格式缓存命中）——证据引用制执行。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x07C0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RvCode {
    Ok = 0,
    SigMismatch = SEG_BASE + 0x01,   // 0x07C1
    CriticalBack = SEG_BASE + 0x02,  // 0x07C2
    FiveCheckFail = SEG_BASE + 0x03, // 0x07C3
    CoverageShort = SEG_BASE + 0x04, // 0x07C4
    ProbeReject = SEG_BASE + 0x05,   // 0x07C5 边界探针拒绝（二轮深化）
    RoundtripDrift = SEG_BASE + 0x06, // 0x07C6 核验态投影漂移（二轮深化）
    LedgerBroken = SEG_BASE + 0x07,  // 0x07C7 账本守恒破（二轮深化）
}

/// T04 五段流水线。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TransStage { Extract = 0, TmMemory = 1, MtEngine = 2, Edit = 3, Lqa = 4 }

impl TransStage {
    pub const ALL: [TransStage; 5] = [
        TransStage::Extract, TransStage::TmMemory, TransStage::MtEngine, TransStage::Edit, TransStage::Lqa];
    pub fn name(self) -> &'static str {
        match self {
            TransStage::Extract => "extract", TransStage::TmMemory => "tm", TransStage::MtEngine => "mt",
            TransStage::Edit => "edit", TransStage::Lqa => "lqa",
        }
    }
}

/// T05 五检。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FiveCheck { Manifest = 0, Checksum = 1, GlyphCover = 2, RefClosure = 3, A11yAsset = 4 }

impl FiveCheck {
    pub const ALL: [FiveCheck; 5] = [
        FiveCheck::Manifest, FiveCheck::Checksum, FiveCheck::GlyphCover,
        FiveCheck::RefClosure, FiveCheck::A11yAsset];
    pub fn name(self) -> &'static str {
        match self {
            FiveCheck::Manifest => "manifest", FiveCheck::Checksum => "checksum",
            FiveCheck::GlyphCover => "glyph-cover", FiveCheck::RefClosure => "ref-closure",
            FiveCheck::A11yAsset => "a11y-asset",
        }
    }
}

/// T06 核验三项。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum T06Check { ResidencyRoute = 0, ComplianceCover = 1, FormatCacheHit = 2 }

impl T06Check {
    pub const ALL: [T06Check; 3] = [
        T06Check::ResidencyRoute, T06Check::ComplianceCover, T06Check::FormatCacheHit];
    pub fn name(self) -> &'static str {
        match self {
            T06Check::ResidencyRoute => "residency-route", T06Check::ComplianceCover => "compliance-cover",
            T06Check::FormatCacheHit => "format-cache-hit",
        }
    }
}

/// 三组核验矩阵。
pub struct VerifyT04T06 {
    pub t04_sigs: Vec<(TransStage, u32)>, // 五段交接签名
    pub t04_critical: u32,                // Critical 残留（须 0）
    pub t05_checks: Vec<(FiveCheck, bool)>,
    pub t05_atomic: bool,                 // 热更新原子性
    pub t06: Vec<(T06Check, bool)>,
    pub compliance_cover_bp: u32,         // 合规覆盖度（万分比）
    pub cache_hit_bp: u32,
}

impl VerifyT04T06 {
    pub fn new() -> Self {
        VerifyT04T06 {
            t04_sigs: Vec::new(), t04_critical: 0,
            t05_checks: FiveCheck::ALL.iter().map(|c| (*c, false)).collect(),
            t05_atomic: false, t06: T06Check::ALL.iter().map(|c| (*c, false)).collect(),
            compliance_cover_bp: 0, cache_hit_bp: 0,
        }
    }

    /// T04 五段签名链一致：全段同值。
    pub fn check_t04(&mut self, sigs: [u32; 5]) -> bool {
        self.t04_sigs = TransStage::ALL.iter().zip(sigs.iter()).map(|(s, v)| (*s, *v)).collect();
        sigs.iter().all(|v| *v == sigs[0])
    }
    pub fn t04_ok(&self) -> bool { self.t04_sigs.len() == 5 && self.t04_critical == 0 }

    /// T05 五检 + 原子性。
    pub fn mark_t05(&mut self, c: FiveCheck, pass: bool) {
        if let Some(x) = self.t05_checks.iter_mut().find(|x| x.0 == c) { x.1 = pass; }
    }
    pub fn t05_ok(&self) -> bool {
        self.t05_checks.iter().all(|x| x.1) && self.t05_atomic
    }
    pub fn five_missing(&self) -> Vec<&'static str> {
        self.t05_checks.iter().filter(|x| !x.1).map(|x| x.0.name()).collect()
    }

    /// T06 三项。
    pub fn mark_t06(&mut self, c: T06Check, pass: bool) {
        if let Some(x) = self.t06.iter_mut().find(|x| x.0 == c) { x.1 = pass; }
    }
    /// T06 通过：三标记过 + 覆盖度 ≥ 9500bp + 缓存命中 ≥ 9000bp。
    pub fn t06_ok(&self) -> bool {
        self.t06.iter().all(|x| x.1) && self.compliance_cover_bp >= 9500 && self.cache_hit_bp >= 9000
    }

    pub fn all_ok(&self) -> bool { self.t04_ok() && self.t05_ok() && self.t06_ok() }
}

// ============ 二轮深化块：分位账本 / 阈值注入回归 / 序列化指纹 / 边界探针 ============

/// [深化] 覆盖度/命中历史账本：双列等长记录 + 整数分位（禁浮点）。
pub struct BpLedger {
    pub cover_hist: Vec<u32>,
    pub hit_hist: Vec<u32>,
}

impl BpLedger {
    pub fn new() -> Self { BpLedger { cover_hist: Vec::new(), hit_hist: Vec::new() } }

    pub fn record(&mut self, cover_bp: u32, hit_bp: u32) {
        self.cover_hist.push(cover_bp);
        self.hit_hist.push(hit_bp);
    }

    /// 双列等长守恒。
    pub fn conserved(&self) -> bool { self.cover_hist.len() == self.hit_hist.len() }

    /// 整数分位（插入排序取点）：q ∈ [0,100]，空列/越参 → ProbeReject。
    pub fn quantile(vals: &[u32], q: u32) -> Result<u32, RvCode> {
        if vals.is_empty() || q > 100 { return Err(RvCode::ProbeReject); }
        let mut v: Vec<u32> = Vec::new();
        for x in vals.iter() { v.push(*x); }
        for i in 1..v.len() {
            let mut j = i;
            while j > 0 && v[j - 1] > v[j] { v.swap(j - 1, j); j -= 1; }
        }
        Ok(v[(q as usize * (v.len() - 1) + 50) / 100])
    }
}

/// [深化] 五检通过结构化账本 + 守恒恒等式。
#[derive(Clone, Copy)]
pub struct FiveCheckLedger {
    pub pass: u32,
    pub fail: u32,
    pub total_marks: u32,
}

impl FiveCheckLedger {
    pub fn new() -> Self { FiveCheckLedger { pass: 0, fail: 0, total_marks: 0 } }
    pub fn record(&mut self, pass: bool) {
        self.total_marks += 1;
        if pass { self.pass += 1; } else { self.fail += 1; }
    }
    pub fn conserved(&self) -> bool { self.pass + self.fail == self.total_marks }
}

/// [深化] 8 行确定性 LCG（无浮点无墙钟）。
pub struct Lcg { state: u64 }

impl Lcg {
    pub fn new(seed: u64) -> Self { Lcg { state: seed } }
    pub fn next(&mut self) -> u64 {
        self.state = self.state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }
    pub fn below(&mut self, n: u32) -> u32 {
        ((self.next() >> 33) as u32) % if n == 0 { 1 } else { n }
    }
}

/// [深化] 阈值注入回归报告：线下必抓 + 线上零误报。
#[derive(Clone, Copy)]
pub struct ThresholdReport {
    pub rounds: u32,
    pub below_line: u32,
    pub caught: u32,
    pub false_alarms: u32,
}

impl ThresholdReport {
    pub fn all_caught(&self) -> bool { self.caught == self.below_line && self.false_alarms == 0 }
}

/// [深化] 注入回归：LCG 生成 N 轮覆盖度/命中对，门槛线（9500bp）复刻裁决必抓。
pub fn inject_threshold(seed: u64, rounds: u32) -> ThresholdReport {
    let mut rng = Lcg::new(seed);
    let mut rep = ThresholdReport { rounds, below_line: 0, caught: 0, false_alarms: 0 };
    let mut i = 0;
    while i < rounds {
        let mut m = VerifyT04T06::new();
        for c in T06Check::ALL.iter() { m.mark_t06(*c, true); }
        let cover = 8000 + rng.below(3000);
        let hit = 9000 + rng.below(1500); // 命中恒过线，单变量隔离
        m.compliance_cover_bp = cover;
        m.cache_hit_bp = hit;
        let ok = m.t06_ok();
        if cover < 9500 {
            rep.below_line += 1;
            if !ok { rep.caught += 1; }
        } else if !ok {
            rep.false_alarms += 1;
        }
        i += 1;
    }
    rep
}

/// [深化] 核验态定长投影（11B，禁 JSON）：五检位图 u8 + T06 位图 u8 + 原子 u8 + 覆盖 u32le + 命中 u32le。
pub fn verify_bytes(m: &VerifyT04T06, out: &mut Vec<u8>) {
    out.clear();
    let mut m5 = 0u8;
    for x in m.t05_checks.iter() { if x.1 { m5 |= 1 << (x.0 as u8); } }
    out.push(m5);
    let mut m6 = 0u8;
    for x in m.t06.iter() { if x.1 { m6 |= 1 << (x.0 as u8); } }
    out.push(m6);
    out.push(if m.t05_atomic { 1 } else { 0 });
    out.extend_from_slice(&m.compliance_cover_bp.to_le_bytes());
    out.extend_from_slice(&m.cache_hit_bp.to_le_bytes());
}

/// [深化] 投影解码回读（与 verify_bytes 对偶）：定长校验 + 五字段回读。
pub fn decode_verify(bytes: &[u8]) -> Result<(u8, u8, u8, u32, u32), RvCode> {
    if bytes.len() != 11 { return Err(RvCode::RoundtripDrift); }
    Ok((
        bytes[0],
        bytes[1],
        bytes[2],
        u32::from_le_bytes([bytes[3], bytes[4], bytes[5], bytes[6]]),
        u32::from_le_bytes([bytes[7], bytes[8], bytes[9], bytes[10]]),
    ))
}

/// [深化] FNV-1a 64 位指纹 + 幂等稳定闸。
pub fn verify_fingerprint_stable(m: &VerifyT04T06) -> Result<u64, RvCode> {
    let mut b1: Vec<u8> = Vec::new();
    verify_bytes(m, &mut b1);
    let mut h1: u64 = 0xcbf29ce484222325;
    for b in b1.iter() { h1 ^= *b as u64; h1 = h1.wrapping_mul(0x100000001b3); }
    let mut b2: Vec<u8> = Vec::new();
    verify_bytes(m, &mut b2);
    let mut h2: u64 = 0xcbf29ce484222325;
    for b in b2.iter() { h2 ^= *b as u64; h2 = h2.wrapping_mul(0x100000001b3); }
    if b1.len() != b2.len() || h1 != h2 { Err(RvCode::RoundtripDrift) } else { Ok(h1) }
}

/// [深化] 核验边界探针。
pub struct VerifyProbe;

impl VerifyProbe {
    /// 空 T04 签名段探针 → ProbeReject。
    pub fn probe_sigs_empty(m: &VerifyT04T06) -> Result<(), RvCode> {
        if m.t04_sigs.is_empty() { Err(RvCode::ProbeReject) } else { Ok(()) }
    }
    /// 越界序号取五检行 → ProbeReject（不 panic）。
    pub fn probe_t05_index(m: &VerifyT04T06, idx: usize) -> Result<&'static str, RvCode> {
        match m.t05_checks.get(idx) {
            Some(x) => Ok(x.0.name()),
            None => Err(RvCode::ProbeReject),
        }
    }
    /// 缺检投影计数（幂等查询）。
    pub fn missing_projection(m: &VerifyT04T06) -> u32 { m.five_missing().len() as u32 }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves403_checks() -> CheckSet {
    let mut set = CheckSet::new("ves403");
    let mut m = VerifyT04T06::new();

    // C403-01 T04 五段交接签名一致。
    chk(&mut set, "C403-01", m.check_t04([0x77AA; 5]) && m.t04_ok(), "五段签名一致");

    // C403-02 Critical 清零复核：残留即红。
    m.t04_critical = 1;
    chk(&mut set, "C403-02", !m.t04_ok(), "Critical 复发红");
    m.t04_critical = 0;

    // C403-03 五段签名失配检出。
    let mut m2 = VerifyT04T06::new();
    chk(&mut set, "C403-03", !m2.check_t04([1, 1, 2, 1, 1]), "MT 段签名漂移");

    // C403-04 T05 五检全过 + 原子性。
    for c in FiveCheck::ALL.iter() { m.mark_t05(*c, true); }
    m.t05_atomic = true;
    chk(&mut set, "C403-04", m.t05_ok(), "五检+原子性绿");

    // C403-05 缺检定位。
    let mut m3 = VerifyT04T06::new();
    m3.mark_t05(FiveCheck::Manifest, true);
    m3.mark_t05(FiveCheck::Checksum, true);
    chk(&mut set, "C403-05", m3.five_missing() == vec!["glyph-cover", "ref-closure", "a11y-asset"], "缺三检定位");

    // C403-06 原子性缺 → T05 红。
    m3.t05_checks.iter_mut().for_each(|x| x.1 = true);
    chk(&mut set, "C403-06", !m3.t05_ok(), "原子性缺红");

    // C403-07 T06 三项 + 覆盖度门槛。
    for c in T06Check::ALL.iter() { m.mark_t06(*c, true); }
    m.compliance_cover_bp = 9600;
    m.cache_hit_bp = 9100;
    chk(&mut set, "C403-07", m.t06_ok(), "T06 三项+门槛绿");

    // C403-08 覆盖度不足红。
    let mut m4 = VerifyT04T06::new();
    for c in T06Check::ALL.iter() { m4.mark_t06(*c, true); }
    m4.compliance_cover_bp = 9400;
    m4.cache_hit_bp = 9500;
    chk(&mut set, "C403-08", !m4.t06_ok(), "覆盖 9400bp 红线");

    // C403-09 缓存命中不足红。
    m4.compliance_cover_bp = 9800;
    m4.cache_hit_bp = 8900;
    chk(&mut set, "C403-09", !m4.t06_ok(), "命中 8900bp 红线");

    // C403-10 三组全过判定。
    chk(&mut set, "C403-10", m.all_ok(), "T04-T06 全绿");

    // C403-11 名单单源（5+5+3）。
    chk(&mut set, "C403-11", TransStage::ALL.len() == 5 && FiveCheck::ALL.len() == 5
        && T06Check::ALL.len() == 3, "名单长度单源");

    // C403-12 名单名核验。
    chk(&mut set, "C403-12", TransStage::Lqa.name() == "lqa"
        && FiveCheck::A11yAsset.name() == "a11y-asset"
        && T06Check::ResidencyRoute.name() == "residency-route", "名单名正确");

    // ===== 二轮深化判据（C403-13 起）=====
    // C403-13 分位账本：三轮 (覆盖,命中) 记录 + 守恒 + 覆盖 p50 = 9600。
    let mut bl = BpLedger::new();
    bl.record(9600, 9100);
    bl.record(9400, 9500);
    bl.record(9800, 8900);
    let q50 = BpLedger::quantile(&bl.cover_hist, 50);
    chk(&mut set, "C403-13", bl.conserved() && q50 == Ok(9600), "覆盖分位 p50=9600");

    // C403-14 分位非法参显性拒：空列 / q>100。
    chk(&mut set, "C403-14", BpLedger::quantile(&[], 50) == Err(RvCode::ProbeReject)
        && BpLedger::quantile(&bl.cover_hist, 101) == Err(RvCode::ProbeReject)
        && BpLedger::quantile(&bl.hit_hist, 100) == Ok(9500), "分位越参拒");

    // C403-15 阈值注入回归：16 轮线下必抓 + 线上零误报。
    let rep = inject_threshold(0x07C0_5EED, 16);
    chk(&mut set, "C403-15", rep.rounds == 16 && rep.all_caught() && rep.below_line > 0,
        "阈值注入 16 轮必抓零误报");

    // C403-16 核验态投影 11B + 解码回读五字段。
    let mut vb: Vec<u8> = Vec::new();
    verify_bytes(&m, &mut vb);
    chk(&mut set, "C403-16", vb.len() == 11
        && decode_verify(&vb) == Ok((31, 7, 1, 9600, 9100)), "核验投影 11B 回读");

    // C403-17 脏投影拒收：截断流 → RoundtripDrift。
    chk(&mut set, "C403-17", decode_verify(&vb[..7]) == Err(RvCode::RoundtripDrift)
        && decode_verify(&[]) == Err(RvCode::RoundtripDrift), "脏投影拒收");

    // C403-18 指纹幂等稳定闸。
    match verify_fingerprint_stable(&m) {
        Ok(v) => chk(&mut set, "C403-18", verify_fingerprint_stable(&m) == Ok(v), "指纹幂等一致"),
        Err(_) => chk(&mut set, "C403-18", false, "指纹不应漂移"),
    }

    // C403-19 边界探针：空签名段拒 / 越界序号拒 / 界内取行。
    let fresh = VerifyT04T06::new();
    chk(&mut set, "C403-19", VerifyProbe::probe_sigs_empty(&fresh) == Err(RvCode::ProbeReject)
        && VerifyProbe::probe_sigs_empty(&m).is_ok()
        && VerifyProbe::probe_t05_index(&m, 0) == Ok("manifest")
        && VerifyProbe::probe_t05_index(&m, 9) == Err(RvCode::ProbeReject)
        && VerifyProbe::missing_projection(&fresh) == 5, "核验探针三态");

    // C403-20 五检账本守恒 + 深化变体段位不撞段。
    let mut fl = FiveCheckLedger::new();
    for c in FiveCheck::ALL.iter() { fl.record(true); }
    fl.record(false);
    chk(&mut set, "C403-20", fl.conserved() && fl.pass == 5 && fl.fail == 1
        && RvCode::ProbeReject as u16 == 0x07C5 && RvCode::RoundtripDrift as u16 == 0x07C6
        && RvCode::LedgerBroken as u16 == 0x07C7, "五检账本守恒段位正确");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves403_smoke() {
        let set = run_ves403_checks();
        assert!(set.failed_count() == 0, "ves403 red: {}", set.red_items_text());
    }
}
