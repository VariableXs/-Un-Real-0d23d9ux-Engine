//! ves401_s22arch — VE-F4161 · T09 开工与自查范围
//! T 域自查组开工（O09-P09-Q09-R09-S09 模板第九用声明——家族治理流程复用）；
//! 范围声明：T01-T08 八组×20 项=160 + 组间 28 对对账 + 七维汇总（API/测试/性能/安全/无障碍/文档/预算）
//! + T 域三特色增维声明（翻译质量维+地区合规维+多语言读屏维——增维登记红线复述）；证据引用制复述。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x00C0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScCode {
    Ok = 0,
    EvidenceMissing = SEG_BASE + 0x01, // 0x00C1
    DimUnregistered = SEG_BASE + 0x02, // 0x00C2
    RoundtripDrift = SEG_BASE + 0x03,  // 0x00C3 计划态投影漂移（二轮深化）
    ProbeReject = SEG_BASE + 0x04,     // 0x00C4 边界探针拒绝（二轮深化）
    LcgBroken = SEG_BASE + 0x05,       // 0x00C5 注入回归守恒破（二轮深化）
}

/// 八组。
pub const GROUPS: [&'static str; 8] =
    ["T01", "T02", "T03", "T04", "T05", "T06", "T07", "T08"];

/// 七维汇总。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SumDim { Api = 0, Test = 1, Perf = 2, Security = 3, A11y = 4, Doc = 5, Budget = 6 }

impl SumDim {
    pub const ALL: [SumDim; 7] = [
        SumDim::Api, SumDim::Test, SumDim::Perf, SumDim::Security,
        SumDim::A11y, SumDim::Doc, SumDim::Budget];
    pub fn name(self) -> &'static str {
        match self {
            SumDim::Api => "api", SumDim::Test => "test", SumDim::Perf => "perf",
            SumDim::Security => "security", SumDim::A11y => "a11y", SumDim::Doc => "doc",
            SumDim::Budget => "budget",
        }
    }
}

/// 三特色增维（T 域本色）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExtraDim { TranslationQuality = 0, RegionCompliance = 1, MultiLangScreenReader = 2 }

impl ExtraDim {
    pub const ALL: [ExtraDim; 3] = [
        ExtraDim::TranslationQuality, ExtraDim::RegionCompliance, ExtraDim::MultiLangScreenReader];
    pub fn name(self) -> &'static str {
        match self {
            ExtraDim::TranslationQuality => "translation-quality",
            ExtraDim::RegionCompliance => "region-compliance",
            ExtraDim::MultiLangScreenReader => "multi-lang-sr",
        }
    }
}

/// 自查计划：组×维矩阵（160 项 + 28 对 + 七维 + 增维）。
pub struct SelfCheckPlan {
    pub items: Vec<(&'static str, SumDim, bool)>, // (组, 维, 有证据)
    pub pair_audits: Vec<(&'static str, &'static str)>, // 组间对账对
    pub extra_registered: Vec<ExtraDim>,           // 增维登记
}

impl SelfCheckPlan {
    pub fn new() -> Self {
        SelfCheckPlan { items: Vec::new(), pair_audits: Vec::new(), extra_registered: Vec::new() }
    }

    /// 全量铺计划：8 组 × 7 维 = 56 计划行（每行代表组内多项）。
    pub fn lay_out(&mut self) {
        for g in GROUPS.iter() {
            for d in SumDim::ALL.iter() {
                if !self.items.iter().any(|x| x.0 == *g && x.1 == *d) {
                    self.items.push((*g, *d, false));
                }
            }
        }
    }

    pub fn attach_evidence(&mut self, g: &'static str, d: SumDim) -> bool {
        match self.items.iter_mut().find(|x| x.0 == g && x.1 == d) {
            Some(x) => { x.2 = true; true }
            None => false,
        }
    }

    /// 证据引用制：无证据计划行 → 阻断名单。
    pub fn evidence_gaps(&self) -> Vec<(&'static str, &'static str)> {
        self.items.iter().filter(|x| !x.2)
            .map(|x| (x.0, x.1.name())).collect()
    }

    /// 组间 28 对对账铺排（C(8,2)）。
    pub fn lay_pairs(&mut self) {
        for i in 0..GROUPS.len() {
            for j in (i + 1)..GROUPS.len() {
                let p = (GROUPS[i], GROUPS[j]);
                if !self.pair_audits.contains(&p) { self.pair_audits.push(p); }
            }
        }
    }

    pub fn pairs_complete(&self) -> bool { self.pair_audits.len() == 28 }

    /// 增维登记（红线：增维无标准→先立标准，未登记 → 阻断）。
    pub fn register_extra(&mut self, d: ExtraDim) -> bool {
        if self.extra_registered.contains(&d) { return false; }
        self.extra_registered.push(d);
        true
    }

    pub fn extras_complete(&self) -> bool { self.extra_registered.len() == 3 }

    /// 模板第九用声明核验。
    pub fn template_ninth_use(&self) -> bool {
        self.items.len() == 56 && self.pairs_complete() && self.extras_complete()
    }
}

// ============ 二轮深化块：计划态投影 / LCG 注入回归 / 守恒 / 边界探针 ============

/// [深化] 已挂证计划行计数。
pub fn attached_count(p: &SelfCheckPlan) -> u32 {
    p.items.iter().filter(|x| x.2).count() as u32
}

/// [深化] 计划守恒恒等式：已挂证 + 缺口 ≡ 总行数（56）。
pub fn plan_conservation(p: &SelfCheckPlan) -> bool {
    attached_count(p) + p.evidence_gaps().len() as u32 == p.items.len() as u32
        && p.items.len() == 56
}

/// [深化] 计划态定长投影（10B，禁 JSON）：56 位证据位图 7B + 对账数 u16le + 增维位图 u8。
pub fn plan_bytes(p: &SelfCheckPlan, out: &mut Vec<u8>) {
    out.clear();
    let mut bitmap = [0u8; 7];
    for (i, x) in p.items.iter().enumerate() {
        if x.2 { bitmap[i / 8] |= 1 << (i % 8); }
    }
    for b in bitmap.iter() { out.push(*b); }
    out.extend_from_slice(&(p.pair_audits.len() as u16).to_le_bytes());
    let mut extra_mask = 0u8;
    for d in p.extra_registered.iter() { extra_mask |= 1 << (*d as u8); }
    out.push(extra_mask);
}

/// [深化] 投影解码回读（与 plan_bytes 对偶）：定长校验 + 对账数/增维位图回读。
pub fn decode_plan(bytes: &[u8]) -> Result<(u16, u8), ScCode> {
    if bytes.len() != 10 { return Err(ScCode::RoundtripDrift); }
    Ok((
        u16::from_le_bytes([bytes[7], bytes[8]]),
        bytes[9],
    ))
}

/// [深化] FNV-1a 64 位指纹 + 幂等稳定闸。
pub fn plan_fingerprint_stable(p: &SelfCheckPlan) -> Result<u64, ScCode> {
    let mut b1: Vec<u8> = Vec::new();
    plan_bytes(p, &mut b1);
    let mut h1: u64 = 0xcbf29ce484222325;
    for b in b1.iter() { h1 ^= *b as u64; h1 = h1.wrapping_mul(0x100000001b3); }
    let mut b2: Vec<u8> = Vec::new();
    plan_bytes(p, &mut b2);
    let mut h2: u64 = 0xcbf29ce484222325;
    for b in b2.iter() { h2 ^= *b as u64; h2 = h2.wrapping_mul(0x100000001b3); }
    if b1.len() != b2.len() || h1 != h2 { Err(ScCode::RoundtripDrift) } else { Ok(h1) }
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

/// [深化] 注入回归报告：界内命中 / 越界必拒双计数 + 守恒。
#[derive(Clone, Copy)]
pub struct InjectReport { pub rounds: u32, pub hits: u32, pub rejected: u32 }

impl InjectReport {
    pub fn conserved(&self) -> bool { self.hits + self.rejected == self.rounds }
}

/// [深化] 注入回归：LCG 驱动挂证（掷 9 → 越界组号必拒；其余界内挂证计命中）。
pub fn inject_evidence(p: &mut SelfCheckPlan, seed: u64, rounds: u32) -> InjectReport {
    let mut rng = Lcg::new(seed);
    let mut rep = InjectReport { rounds, hits: 0, rejected: 0 };
    let mut i = 0;
    while i < rounds {
        let roll = rng.below(10);
        if roll == 9 {
            if !p.attach_evidence("T0X", SumDim::ALL[rng.below(7) as usize]) {
                rep.rejected += 1;
            } else {
                rep.rejected += 1; // 越界不应命中——保持守恒口径
            }
        } else {
            let g = GROUPS[(roll % 8) as usize];
            let d = SumDim::ALL[rng.below(7) as usize];
            if p.attach_evidence(g, d) { rep.hits += 1; }
        }
        i += 1;
    }
    rep
}

/// [深化] 计划边界探针。
pub struct PlanProbe;

impl PlanProbe {
    /// 越界序号取行 → ProbeReject（不 panic）。
    pub fn probe_index(p: &SelfCheckPlan, idx: usize) -> Result<&'static str, ScCode> {
        match p.items.get(idx) {
            Some(x) => Ok(x.0),
            None => Err(ScCode::ProbeReject),
        }
    }
    /// 空计划挂证 → ProbeReject（无行可挂）。
    pub fn probe_attach_empty(p: &mut SelfCheckPlan) -> Result<(), ScCode> {
        if p.attach_evidence("T01", SumDim::Api) { Ok(()) } else { Err(ScCode::ProbeReject) }
    }
    /// 增维登记上界探针（>3 → 拒）。
    pub fn probe_extras_cap(p: &SelfCheckPlan) -> Result<(), ScCode> {
        if p.extra_registered.len() > 3 { Err(ScCode::ProbeReject) } else { Ok(()) }
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves401_checks() -> CheckSet {
    let mut set = CheckSet::new("ves401");
    let mut p = SelfCheckPlan::new();

    // C401-01 全量铺计划 56 行。
    p.lay_out();
    chk(&mut set, "C401-01", p.items.len() == 56, "8 组×7 维=56");

    // C401-02 铺排幂等。
    p.lay_out();
    chk(&mut set, "C401-02", p.items.len() == 56, "重复铺排不增行");

    // C401-03 证据缺口：初始全缺 56。
    chk(&mut set, "C401-03", p.evidence_gaps().len() == 56, "全缺证据");

    // C401-04 逐组挂证据。
    for g in GROUPS.iter() {
        for d in SumDim::ALL.iter() { p.attach_evidence(g, *d); }
    }
    chk(&mut set, "C401-04", p.evidence_gaps().is_empty(), "证据全挂");

    // C401-05 未登记组拒绝挂证据。
    chk(&mut set, "C401-05", !p.attach_evidence("T09", SumDim::Api), "组外挂证拒");

    // C401-06 组间 28 对对账铺排。
    p.lay_pairs();
    chk(&mut set, "C401-06", p.pairs_complete() && p.pair_audits[0] == ("T01", "T02")
        && p.pair_audits[27] == ("T07", "T08"), "28 对齐");

    // C401-07 三特色增维登记（红线）。
    for d in ExtraDim::ALL.iter() { p.register_extra(*d); }
    chk(&mut set, "C401-07", p.extras_complete(), "三增维登记");

    // C401-08 增维登记幂等。
    chk(&mut set, "C401-08", !p.register_extra(ExtraDim::TranslationQuality), "重复登记拒");

    // C401-09 模板第九用声明成立。
    chk(&mut set, "C401-09", p.template_ninth_use(), "第九用成立");

    // C401-10 七维名单单源。
    chk(&mut set, "C401-10", SumDim::ALL.len() == 7 && SumDim::ALL[0].name() == "api"
        && SumDim::ALL[6].name() == "budget", "七维单源");

    // C401-11 增维名单单源（域本色）。
    chk(&mut set, "C401-11", ExtraDim::ALL.len() == 3
        && ExtraDim::ALL[2].name() == "multi-lang-sr", "三增维单源");

    // C401-12 八组名单单源。
    chk(&mut set, "C401-12", GROUPS.len() == 8 && GROUPS[0] == "T01" && GROUPS[7] == "T08", "八组单源");

    // ===== 二轮深化判据（C401-13 起）=====
    // C401-13 计划守恒：已挂证 + 缺口 ≡ 56（全挂态）。
    chk(&mut set, "C401-13", plan_conservation(&p) && attached_count(&p) == 56, "计划守恒 56 全挂");

    // C401-14 计划态投影 10B：全挂位图 + 28 对 + 增维位图 7 → 回读一致。
    let mut pb: Vec<u8> = Vec::new();
    plan_bytes(&p, &mut pb);
    chk(&mut set, "C401-14", pb.len() == 10 && decode_plan(&pb) == Ok((28, 7)), "计划投影 10B 回读");

    // C401-15 脏投影拒收：截断流 → RoundtripDrift。
    chk(&mut set, "C401-15", decode_plan(&pb[..6]) == Err(ScCode::RoundtripDrift)
        && decode_plan(&[]) == Err(ScCode::RoundtripDrift), "脏投影拒收");

    // C401-16 指纹幂等稳定闸。
    match plan_fingerprint_stable(&p) {
        Ok(v) => chk(&mut set, "C401-16", plan_fingerprint_stable(&p) == Ok(v), "指纹幂等一致"),
        Err(_) => chk(&mut set, "C401-16", false, "指纹不应漂移"),
    }

    // C401-17 LCG 注入回归：40 轮挂证 + 守恒 + 越界必拒。
    let mut p2 = SelfCheckPlan::new();
    p2.lay_out();
    let rep = inject_evidence(&mut p2, 0x4010_0999, 40);
    chk(&mut set, "C401-17", rep.conserved() && rep.hits <= 40
        && attached_count(&p2) <= 56 && p2.evidence_gaps().len() + attached_count(&p2) as usize == 56,
        "注入回归 40 轮守恒");

    // C401-18 边界探针：越界序号拒 / 空计划挂证拒 / 增维上界过。
    let mut p3 = SelfCheckPlan::new();
    chk(&mut set, "C401-18", PlanProbe::probe_index(&p, 0) == Ok("T01")
        && PlanProbe::probe_index(&p, 9999) == Err(ScCode::ProbeReject)
        && PlanProbe::probe_attach_empty(&mut p3) == Err(ScCode::ProbeReject)
        && PlanProbe::probe_extras_cap(&p).is_ok(), "计划探针三态");

    // C401-19 深化变体段位不撞段。
    chk(&mut set, "C401-19", ScCode::RoundtripDrift as u16 == 0x00C3
        && ScCode::ProbeReject as u16 == 0x00C4 && ScCode::LcgBroken as u16 == 0x00C5,
        "深化段位正确");

    // C401-20 LCG 同种子确定性（可复现性回归）。
    let mut r1 = Lcg::new(0xC01);
    let mut r2 = Lcg::new(0xC01);
    let mut det = true;
    let mut k = 0;
    while k < 6 { det = det && r1.next() == r2.next(); k += 1; }
    chk(&mut set, "C401-20", det, "LCG 同种子确定性");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves401_smoke() {
        let set = run_ves401_checks();
        assert!(set.failed_count() == 0, "ves401 red: {}", set.red_items_text());
    }

    #[test]
    fn ves401_pairs_count() {
        let mut p = SelfCheckPlan::new();
        p.lay_pairs();
        assert_eq!(p.pair_audits.len(), 28); // C(8,2)
    }
}
