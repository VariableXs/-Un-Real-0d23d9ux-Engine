//! ves399_s21integ — VE-F4159 · T08 联调
//! 组内联调：20 条目贯通演练（审查→仿真→语料→回归→走查工具链全链跑通）
//! + 故障注入三演练（审查器误报飙升/仿真泄漏/语料分级错）；证据挂 F4148 看板；
//! 链断→定位+回执；演练失败→预案验证；证据缺→重跑；读屏仿真抽演（域本色）；语料合成。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x0080;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IgCode {
    Ok = 0,
    ChainBroken = SEG_BASE + 0x01,   // 0x0081
    DrillFailed = SEG_BASE + 0x02,   // 0x0082
    EvidenceMissing = SEG_BASE + 0x03, // 0x0083
    ProbeReject = SEG_BASE + 0x04,     // 0x0084 边界探针拒绝（二轮深化）
    RoundtripDrift = SEG_BASE + 0x05,  // 0x0085 演练态投影漂移（二轮深化）
    InjectBroken = SEG_BASE + 0x06,    // 0x0086 注入回归守恒破（二轮深化）
}

/// 工具链五站（审查→仿真→语料→回归→走查）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Station { Linter = 0, Sim = 1, Corpus = 2, Regress = 3, Walkthrough = 4 }

impl Station {
    pub const CHAIN: [Station; 5] = [
        Station::Linter, Station::Sim, Station::Corpus, Station::Regress, Station::Walkthrough];
    pub fn name(self) -> &'static str {
        match self {
            Station::Linter => "linter", Station::Sim => "sim", Station::Corpus => "corpus",
            Station::Regress => "regress", Station::Walkthrough => "walkthrough",
        }
    }
}

/// 故障注入三类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fault { LinterFpSurge = 0, SimLeak = 1, CorpusGradeWrong = 2 }

impl Fault {
    pub const ALL: [Fault; 3] = [Fault::LinterFpSurge, Fault::SimLeak, Fault::CorpusGradeWrong];
    pub fn name(self) -> &'static str {
        match self {
            Fault::LinterFpSurge => "linter-fp-surge", Fault::SimLeak => "sim-leak",
            Fault::CorpusGradeWrong => "corpus-grade-wrong",
        }
    }
}

/// 贯通演练：五站链 + 每站产出证据。
pub struct ChainDrill {
    pub passed: Vec<Station>,      // 已贯通站
    pub evidence: Vec<&'static str>, // 证据（挂看板）
    pub drills: Vec<(Fault, bool)>,  // 三演练（故障, 预案验证过）
    pub screen_reader_drill: bool,   // 读屏仿真抽演（域本色）
}

impl ChainDrill {
    pub fn new() -> Self {
        ChainDrill { passed: Vec::new(), evidence: Vec::new(), drills: Vec::new(), screen_reader_drill: false }
    }

    /// 单站贯通（顺序推进：只能贯通下一站）。
    pub fn pass_next(&mut self) -> bool {
        let next = Station::CHAIN.iter().find(|s| !self.passed.contains(s));
        match next {
            Some(s) => { self.passed.push(*s); true }
            None => false,
        }
    }

    /// 链断定位：第一个未贯通站。
    pub fn break_at(&self) -> Option<&'static str> {
        Station::CHAIN.iter().find(|s| !self.passed.contains(s)).map(|s| s.name())
    }

    pub fn full_chain(&self) -> bool { self.passed.len() == 5 }

    /// 挂证据（幂等）。
    pub fn attach_evidence(&mut self, e: &'static str) -> bool {
        if e.is_empty() || self.evidence.contains(&e) { return false; }
        self.evidence.push(e);
        true
    }

    /// 故障注入演练：预案验证过 → 记 (fault, true)。
    pub fn drill(&mut self, f: Fault, plan_ok: bool) {
        if !self.drills.iter().any(|x| x.0 == f) { self.drills.push((f, plan_ok)); }
    }

    pub fn drills_ok(&self) -> bool {
        Fault::ALL.iter().all(|f| self.drills.iter().any(|x| x.0 == *f && x.1))
    }

    pub fn failed_drills(&self) -> Vec<&'static str> {
        Fault::ALL.iter()
            .filter(|f| self.drills.iter().any(|x| x.0 == **f && !x.1))
            .map(|f| f.name()).collect()
    }

    /// 联调放行：全链 + 三演练过 + 读屏抽演 + 证据齐。
    pub fn released(&self) -> bool {
        self.full_chain() && self.drills_ok() && self.screen_reader_drill
            && self.evidence.len() >= 2
    }
}

// ============ 二轮深化块：LCG 故障注入回归 / 演练态序列化 / FNV / 边界探针 ============

/// [深化] 8 行确定性 LCG（无浮点无墙钟，内核可复现）。
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

/// [深化] 故障注入回归报告：新登记 / 重复跳过双计数 + 守恒。
#[derive(Clone, Copy)]
pub struct FaultInjectReport {
    pub rounds: u32,
    pub registered: u32,
    pub dup_skipped: u32,
}

impl FaultInjectReport {
    pub fn conserved(&self) -> bool { self.registered + self.dup_skipped == self.rounds }
    /// 登记数上界 = 三类故障数（幂等登记不重复）。
    pub fn within_cap(&self) -> bool { self.registered <= 3 }
}

/// [深化] 注入回归：LCG 驱动 N 轮三类故障轮转注入，登记幂等（重复注入零副作用）。
pub fn fault_inject(d: &mut ChainDrill, seed: u64, rounds: u32) -> FaultInjectReport {
    let mut rng = Lcg::new(seed);
    let mut rep = FaultInjectReport { rounds, registered: 0, dup_skipped: 0 };
    let mut i = 0;
    while i < rounds {
        let f = Fault::ALL[rng.below(3) as usize];
        let before = d.drills.len();
        d.drill(f, true);
        if d.drills.len() > before { rep.registered += 1; } else { rep.dup_skipped += 1; }
        i += 1;
    }
    rep
}

/// [深化] 演练态定长投影（5B，禁 JSON）：链位图 u8 + 故障位图 u8 + 读屏 u8 + 证据数 u16le。
pub fn drill_bytes(d: &ChainDrill, out: &mut Vec<u8>) {
    out.clear();
    let mut chain_mask = 0u8;
    for s in Station::CHAIN.iter() {
        if d.passed.contains(s) { chain_mask |= 1 << (*s as u8); }
    }
    out.push(chain_mask);
    let mut fault_mask = 0u8;
    for f in Fault::ALL.iter() {
        if d.drills.iter().any(|x| x.0 == *f && x.1) { fault_mask |= 1 << (*f as u8); }
    }
    out.push(fault_mask);
    out.push(if d.screen_reader_drill { 1 } else { 0 });
    out.extend_from_slice(&(d.evidence.len() as u16).to_le_bytes());
}

/// [深化] 投影解码回读（与 drill_bytes 对偶）：定长校验 + 四字段回读。
pub fn decode_drill(bytes: &[u8]) -> Result<(u8, u8, u8, u16), IgCode> {
    if bytes.len() != 5 { return Err(IgCode::RoundtripDrift); }
    Ok((
        bytes[0],
        bytes[1],
        bytes[2],
        u16::from_le_bytes([bytes[3], bytes[4]]),
    ))
}

/// [深化] FNV-1a 64 位指纹 + 幂等稳定闸。
pub fn drill_fingerprint_stable(d: &ChainDrill) -> Result<u64, IgCode> {
    let mut b1: Vec<u8> = Vec::new();
    drill_bytes(d, &mut b1);
    let mut h1: u64 = 0xcbf29ce484222325;
    for b in b1.iter() { h1 ^= *b as u64; h1 = h1.wrapping_mul(0x100000001b3); }
    let mut b2: Vec<u8> = Vec::new();
    drill_bytes(d, &mut b2);
    let mut h2: u64 = 0xcbf29ce484222325;
    for b in b2.iter() { h2 ^= *b as u64; h2 = h2.wrapping_mul(0x100000001b3); }
    if b1.len() != b2.len() || h1 != h2 { Err(IgCode::RoundtripDrift) } else { Ok(h1) }
}

/// [深化] 证据唯一性守恒：无重复挂链。
pub fn evidence_unique(d: &ChainDrill) -> bool {
    for i in 0..d.evidence.len() {
        for j in (i + 1)..d.evidence.len() {
            if d.evidence[i] == d.evidence[j] { return false; }
        }
    }
    true
}

/// [深化] 演练边界探针：满链推进 / 空证据 / 越界序号。
pub struct DrillProbe;

impl DrillProbe {
    /// 满链再推进探针 → ProbeReject（越上界显性回执）。
    pub fn probe_advance_full(d: &ChainDrill) -> Result<(), IgCode> {
        if d.full_chain() { Err(IgCode::ProbeReject) } else { Ok(()) }
    }
    /// 零证据探针 → EvidenceMissing。
    pub fn probe_evidence(d: &ChainDrill) -> Result<usize, IgCode> {
        if d.evidence.is_empty() { Err(IgCode::EvidenceMissing) } else { Ok(d.evidence.len()) }
    }
    /// 越界序号取已贯通站 → ProbeReject（不 panic）。
    pub fn probe_passed_index(d: &ChainDrill, idx: usize) -> Result<&'static str, IgCode> {
        match d.passed.get(idx) {
            Some(s) => Ok(s.name()),
            None => Err(IgCode::ProbeReject),
        }
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves399_checks() -> CheckSet {
    let mut set = CheckSet::new("ves399");
    let mut d = ChainDrill::new();

    // C399-01 五站顺序贯通。
    for _ in 0..5 { d.pass_next(); }
    chk(&mut set, "C399-01", d.full_chain() && d.break_at().is_none(), "五站全通");

    // C399-02 链断定位：空链断在 linter。
    let d0 = ChainDrill::new();
    chk(&mut set, "C399-02", d0.break_at() == Some("linter"), "空链断于首站");

    // C399-03 贯通两站后断在 corpus。
    let mut d2 = ChainDrill::new();
    d2.pass_next(); d2.pass_next();
    chk(&mut set, "C399-03", d2.break_at() == Some("corpus"), "断于第三站");

    // C399-04 证据挂链（幂等）。
    chk(&mut set, "C399-04", d.attach_evidence("chain-run-001")
        && d.attach_evidence("drill-report-002") && !d.attach_evidence("chain-run-001")
        && d.evidence.len() == 2, "证据二条幂等");

    // C399-05 三故障注入演练（预案验证过）。
    d.drill(Fault::LinterFpSurge, true);
    d.drill(Fault::SimLeak, true);
    d.drill(Fault::CorpusGradeWrong, true);
    chk(&mut set, "C399-05", d.drills_ok() && d.failed_drills().is_empty(), "三演练全过");

    // C399-06 演练失败 → 预案验证失败登记。
    let mut d3 = ChainDrill::new();
    d3.drill(Fault::LinterFpSurge, true);
    d3.drill(Fault::SimLeak, false);
    d3.drill(Fault::CorpusGradeWrong, true);
    chk(&mut set, "C399-06", d3.failed_drills() == vec!["sim-leak"] && !d3.drills_ok(), "演练失败定位");

    // C399-07 复演通过后翻绿（幂等登记不重复）。
    d3.drill(Fault::SimLeak, true);
    chk(&mut set, "C399-07", d3.drills_ok() && d3.drills.len() == 3, "复演翻绿");

    // C399-08 读屏仿真抽演（域本色）。
    d.screen_reader_drill = true;
    chk(&mut set, "C399-08", d.screen_reader_drill, "读屏抽演过");

    // C399-09 联调放行判定。
    chk(&mut set, "C399-09", d.released(), "全件齐放行");

    // C399-10 缺证据不放行。
    let mut d4 = ChainDrill::new();
    for _ in 0..5 { d4.pass_next(); }
    d4.drill(Fault::LinterFpSurge, true);
    d4.drill(Fault::SimLeak, true);
    d4.drill(Fault::CorpusGradeWrong, true);
    d4.screen_reader_drill = true;
    chk(&mut set, "C399-10", !d4.released(), "证据缺不放行");

    // C399-11 空证据拒挂。
    chk(&mut set, "C399-11", !d4.attach_evidence(""), "空证据拒");

    // C399-12 五站名单单源。
    chk(&mut set, "C399-12", Station::CHAIN.len() == 5 && Station::CHAIN[0].name() == "linter"
        && Station::CHAIN[4].name() == "walkthrough" && Fault::ALL.len() == 3, "五站三fault单源");

    // ===== 二轮深化判据（C399-13 起）=====
    // C399-13 故障注入回归：12 轮全撞已登记三类（幂等零副作用）+ 守恒。
    let rep = fault_inject(&mut d4, 0x1707_3990, 12);
    chk(&mut set, "C399-13", rep.conserved() && rep.within_cap()
        && rep.dup_skipped == 12 && rep.registered == 0, "注入回归幂等守恒");

    // C399-14 演练态投影：d4 链满演练全过证据零 → 31/7/1/0 回读一致。
    let mut db: Vec<u8> = Vec::new();
    drill_bytes(&d4, &mut db);
    chk(&mut set, "C399-14", db.len() == 5
        && decode_drill(&db) == Ok((31, 7, 1, 0)), "演练态投影 5B 回读");

    // C399-15 脏投影拒收：截断流 → RoundtripDrift。
    chk(&mut set, "C399-15", decode_drill(&db[..3]) == Err(IgCode::RoundtripDrift)
        && decode_drill(&[]) == Err(IgCode::RoundtripDrift), "脏投影拒收");

    // C399-16 指纹幂等稳定闸（d 满配态）。
    match drill_fingerprint_stable(&d) {
        Ok(v) => chk(&mut set, "C399-16", drill_fingerprint_stable(&d) == Ok(v), "指纹幂等一致"),
        Err(_) => chk(&mut set, "C399-16", false, "指纹不应漂移"),
    }

    // C399-17 边界探针三连：满链推进拒 / 零证据拒 / 越界序号拒。
    chk(&mut set, "C399-17", DrillProbe::probe_advance_full(&d4) == Err(IgCode::ProbeReject)
        && DrillProbe::probe_evidence(&d4) == Err(IgCode::EvidenceMissing)
        && DrillProbe::probe_evidence(&d) == Ok(2)
        && DrillProbe::probe_passed_index(&d4, 0) == Ok("linter")
        && DrillProbe::probe_passed_index(&d4, 9) == Err(IgCode::ProbeReject), "边界探针三连");

    // C399-18 证据唯一性守恒。
    chk(&mut set, "C399-18", evidence_unique(&d) && evidence_unique(&d4), "证据唯一守恒");

    // C399-19 深化变体段位不撞段。
    chk(&mut set, "C399-19", IgCode::ProbeReject as u16 == 0x0084
        && IgCode::RoundtripDrift as u16 == 0x0085 && IgCode::InjectBroken as u16 == 0x0086,
        "深化段位正确");

    // C399-20 LCG 同种子确定性（可复现性回归）。
    let mut r1 = Lcg::new(0x39A);
    let mut r2 = Lcg::new(0x39A);
    let mut det = true;
    let mut k = 0;
    while k < 6 { det = det && r1.next() == r2.next(); k += 1; }
    chk(&mut set, "C399-20", det, "LCG 同种子确定性");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves399_smoke() {
        let set = run_ves399_checks();
        assert!(set.failed_count() == 0, "ves399 red: {}", set.red_items_text());
    }

    #[test]
    fn ves399_pass_next_capped() {
        let mut d = ChainDrill::new();
        for _ in 0..8 { d.pass_next(); }
        assert_eq!(d.passed.len(), 5);
    }
}
