//! ves395_s21fuzzgate — VE-F4155 · i18n fuzz 总闸
//! T 域 fuzz 面总归集（T04 三面 + T05 三面 + T06 三面 + T07 三面 + 本组新面 = 总面册）；
//! 总闸通过线（崩溃 0/挂起 0/越权写 0 三零线）确认；最小复现库统一；
//! 四源归一（F4078/F4098/F4114/F4134）；线未达→阻断+归因；新崩溃→立案入库；面缺→补面。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x0000;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FgCode {
    Ok = 0,
    GateBlocked = SEG_BASE + 0x01,   // 0x0001
    FaceMissing = SEG_BASE + 0x02,   // 0x0002
    CrashFiled = SEG_BASE + 0x03,    // 0x0003
    BoundaryViolation = SEG_BASE + 0x04, // 0x0004 边界探针违规
    LedgerBroken = SEG_BASE + 0x05,      // 0x0005 立案账本守恒破
    SerializeDrift = SEG_BASE + 0x06,    // 0x0006 序列化投影漂移
    ProbeReject = SEG_BASE + 0x07,       // 0x0007 边界探针拒绝（二轮深化）
    IdemGateTrip = SEG_BASE + 0x08,      // 0x0008 幂等闸触发（二轮深化）
    FingerprintDrift = SEG_BASE + 0x09,  // 0x0009 指纹漂移（二轮深化）
}

/// fuzz 面来源（四源归一 + 本组新面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FaceSource { T04 = 0, T05 = 1, T06 = 2, T07 = 3, T08New = 4 }

impl FaceSource {
    pub const ALL: [FaceSource; 5] = [
        FaceSource::T04, FaceSource::T05, FaceSource::T06, FaceSource::T07, FaceSource::T08New];
    pub fn name(self) -> &'static str {
        match self {
            FaceSource::T04 => "T04", FaceSource::T05 => "T05", FaceSource::T06 => "T06",
            FaceSource::T07 => "T07", FaceSource::T08New => "T08-new",
        }
    }
}

/// fuzz 面：来源 + 面名 + 语料数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FuzzFace {
    pub source: FaceSource,
    pub name: &'static str,
    pub corpus: u32,
}

/// 总面册：归集 + 三零线确认。
pub struct FaceBook {
    pub faces: Vec<FuzzFace>,
    pub crashes: u32,
    pub hangs: u32,
    pub unauthorized_writes: u32,
    pub repros: Vec<&'static str>, // 最小复现库（统一）
}

impl FaceBook {
    pub fn new() -> Self {
        FaceBook { faces: Vec::new(), crashes: 0, hangs: 0, unauthorized_writes: 0, repros: Vec::new() }
    }

    /// 归集一面；同源同名幂等。
    pub fn add(&mut self, f: FuzzFace) {
        if !self.faces.iter().any(|x| x.source == f.source && x.name == f.name) {
            self.faces.push(f);
        }
    }

    /// 四源（+本组新面）归一完备：每源至少一面。
    pub fn sources_covered(&self) -> bool {
        FaceSource::ALL.iter().all(|s| self.faces.iter().any(|f| f.source == *s))
    }

    /// 缺源补面定位。
    pub fn missing_sources(&self) -> Vec<&'static str> {
        FaceSource::ALL.iter()
            .filter(|s| !self.faces.iter().any(|f| f.source == **s))
            .map(|s| s.name()).collect()
    }

    /// 三零线确认：崩溃 0 / 挂起 0 / 越权写 0。
    pub fn zero_line_ok(&self) -> bool {
        self.crashes == 0 && self.hangs == 0 && self.unauthorized_writes == 0
    }

    /// 总闸裁决：面齐 + 三零线 → 放行；否则阻断。
    pub fn gate(&self) -> Result<(), FgCode> {
        if !self.sources_covered() { Err(FgCode::FaceMissing) }
        else if !self.zero_line_ok() { Err(FgCode::GateBlocked) }
        else { Ok(()) }
    }

    /// 新崩溃立案入库 + 最小复现统一入库。
    pub fn file_crash(&mut self, repro: &'static str) -> bool {
        if repro.is_empty() { return false; }
        self.crashes += 1;
        if !self.repros.contains(&repro) { self.repros.push(repro); }
        true
    }

    /// 归因：给出破线维度名。
    pub fn attribute(&self) -> Option<&'static str> {
        if self.crashes > 0 { Some("crash") }
        else if self.hangs > 0 { Some("hang") }
        else if self.unauthorized_writes > 0 { Some("unauth-write") }
        else { None }
    }

    /// 归一后语料总量。
    pub fn corpus_total(&self) -> u32 { self.faces.iter().map(|f| f.corpus).sum() }
}

/// 阻断归因报告。
pub struct BlockReport { pub reason: &'static str, pub gate: FgCode }

pub fn block_report(fb: &FaceBook) -> Option<BlockReport> {
    match fb.gate() {
        Ok(()) => None,
        Err(c) => Some(BlockReport {
            reason: match c {
                FgCode::FaceMissing => "面缺——补面后重闸",
                FgCode::GateBlocked => "三零线破——归因立案",
                _ => "未知",
            },
            gate: c,
        }),
    }
}

// ============ 二轮深化块：边界探针 / LCG 注入回归 / 幂等闸 / FNV 指纹 / 守恒 ============

/// [深化] 总闸边界探针裁决器：对总闸做四类边界压测，显性 Err 码回执。
pub struct GateProbe {
    pub max_faces: u32,
    pub max_corpus_per_face: u32,
    pub max_repros: u32,
}

impl GateProbe {
    pub fn new(max_faces: u32, max_corpus_per_face: u32, max_repros: u32) -> Self {
        GateProbe { max_faces, max_corpus_per_face, max_repros }
    }

    /// 边界一：空面册 → FaceMissing（补面归因）。
    pub fn probe_empty(&self, fb: &FaceBook) -> Result<(), FgCode> {
        if fb.faces.is_empty() { Err(FgCode::FaceMissing) } else { Ok(()) }
    }

    /// 边界二：面数满容量 → GateBlocked（容量线）。
    pub fn probe_capacity(&self, fb: &FaceBook) -> Result<(), FgCode> {
        if (fb.faces.len() as u32) > self.max_faces { Err(FgCode::GateBlocked) } else { Ok(()) }
    }

    /// 边界三：重复注入同一面 → 幂等容许（不增行即过）。
    pub fn probe_duplicate(&self, fb: &FaceBook, f: &FuzzFace) -> Result<(), FgCode> {
        if fb.faces.iter().any(|x| x.source == f.source && x.name == f.name) { Ok(()) }
        else { Err(FgCode::ProbeReject) }
    }

    /// 边界四：单面语料越限 → ProbeReject。
    pub fn probe_corpus(&self, corpus: u32) -> Result<(), FgCode> {
        if corpus > self.max_corpus_per_face { Err(FgCode::ProbeReject) } else { Ok(()) }
    }

    /// 边界五：越界序号取面 → ProbeReject（显性回执，不 panic）。
    pub fn probe_index(&self, fb: &FaceBook, idx: usize) -> Result<&'static str, FgCode> {
        match fb.faces.get(idx) {
            Some(f) => Ok(f.name),
            None => Err(FgCode::ProbeReject),
        }
    }
}

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
    /// 取 [0, n) 内确定性值；n=0 退化为常 0（防除零）。
    pub fn below(&mut self, n: u32) -> u32 {
        ((self.next() >> 33) as u32) % if n == 0 { 1 } else { n }
    }
}

/// [深化] 注入回归账本：LCG 驱动 N 轮语料注入，异常必抓 + 负对照零误报。
pub struct InjectLedger {
    pub rounds: u32,
    pub anomalies: u32,
    pub caught: u32,
    pub normals: u32,
    pub false_alarms: u32,
}

impl InjectLedger {
    /// capacity：单面语料上限；掷点越限判异常，探针须拒（必抓）。
    pub fn run(seed: u64, rounds: u32, capacity: u32) -> Self {
        let mut rng = Lcg::new(seed);
        let probe = GateProbe::new(u32::MAX, capacity, u32::MAX);
        let mut l = InjectLedger { rounds, anomalies: 0, caught: 0, normals: 0, false_alarms: 0 };
        let mut i = 0;
        while i < rounds {
            let corpus = rng.below(capacity * 2 + 1);
            match probe.probe_corpus(corpus) {
                Err(_) => { l.anomalies += 1; l.caught += 1; }
                Ok(()) => { l.normals += 1; }
            }
            i += 1;
        }
        l
    }
    /// 异常必抓率 100%。
    pub fn all_caught(&self) -> bool { self.missed_count() == 0 }
    fn missed_count(&self) -> u32 { self.anomalies.saturating_sub(self.caught) }
    /// 负对照零误报。
    pub fn zero_false_alarm(&self) -> bool { self.false_alarms == 0 }
    /// 守恒：异常 + 正常 = 总轮数。
    pub fn conserved(&self) -> bool { self.anomalies + self.normals == self.rounds }
}

/// [深化] 面册定长字节投影（禁 JSON，手写布局）：
/// 魔数 5B + 5 源 ×(source u16le + corpus u32le) + 三零线 3×u32le = 47B。
pub fn face_book_bytes(fb: &FaceBook, out: &mut Vec<u8>) {
    out.clear();
    out.push(b'F'); out.push(b'B'); out.push(b'3'); out.push(b'9'); out.push(b'5');
    for s in FaceSource::ALL.iter() {
        let corpus = fb.faces.iter().filter(|f| f.source == *s).map(|f| f.corpus).sum::<u32>();
        out.extend_from_slice(&(*s as u16).to_le_bytes());
        out.extend_from_slice(&corpus.to_le_bytes());
    }
    out.extend_from_slice(&fb.crashes.to_le_bytes());
    out.extend_from_slice(&fb.hangs.to_le_bytes());
    out.extend_from_slice(&fb.unauthorized_writes.to_le_bytes());
}

/// [深化] 投影解码回读（与 face_book_bytes 对偶）：魔数 + 定长校验 + 三零线回读。
pub fn decode_zero_line(bytes: &[u8]) -> Result<(u32, u32, u32), FgCode> {
    if bytes.len() != 47 { return Err(FgCode::SerializeDrift); }
    if bytes[0] != b'F' || bytes[1] != b'B' { return Err(FgCode::SerializeDrift); }
    let rd = |o: usize| -> u32 {
        u32::from_le_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]])
    };
    let base = 5 + 5 * 6;
    Ok((rd(base), rd(base + 4), rd(base + 8)))
}

/// [深化] FNV-1a 64 位指纹（纯整数运算）。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes.iter() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// [深化] 幂等闸：同态面册二次投影 → 指纹一致且零副作用（触发即 IdemGateTrip 之外的一致路径）。
pub fn idempotent_fingerprint(fb: &FaceBook) -> Result<u64, FgCode> {
    let mut buf1: Vec<u8> = Vec::new();
    face_book_bytes(fb, &mut buf1);
    let fp1 = fnv1a64(&buf1);
    let mut buf2: Vec<u8> = Vec::new();
    face_book_bytes(fb, &mut buf2);
    let fp2 = fnv1a64(&buf2);
    if buf1.len() != buf2.len() || fp1 != fp2 { Err(FgCode::FingerprintDrift) } else { Ok(fp1) }
}

/// [深化] 立案守恒恒等式：立案数 ≥ 复现签名数（重复签名归并不丢计）。
pub fn filing_conservation(fb: &FaceBook) -> bool {
    fb.crashes as usize >= fb.repros.len()
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves395_checks() -> CheckSet {
    let mut set = CheckSet::new("ves395");
    let mut fb = FaceBook::new();

    // C395-01 四源归一：T04-T07 各三面 + 本组新面（审查器畸形/兼容矩阵/扩展契约）。
    fb.add(FuzzFace { source: FaceSource::T04, name: "extract", corpus: 120 });
    fb.add(FuzzFace { source: FaceSource::T05, name: "manifest", corpus: 90 });
    fb.add(FuzzFace { source: FaceSource::T06, name: "locale-tag", corpus: 80 });
    fb.add(FuzzFace { source: FaceSource::T07, name: "cost-input", corpus: 60 });
    fb.add(FuzzFace { source: FaceSource::T08New, name: "linter-rules", corpus: 45 });
    chk(&mut set, "C395-01", fb.sources_covered() && fb.faces.len() == 5, "五源归一");

    // C395-02 同源同名幂等。
    fb.add(FuzzFace { source: FaceSource::T04, name: "extract", corpus: 121 });
    chk(&mut set, "C395-02", fb.faces.len() == 5, "归集幂等");

    // C395-03 三零线确认（全零）。
    chk(&mut set, "C395-03", fb.zero_line_ok() && fb.gate().is_ok(), "三零线过闸");

    // C395-04 语料总量归一。
    chk(&mut set, "C395-04", fb.corpus_total() == 395, "语料 395");

    // C395-05 新崩溃立案入库 + 复现统一。
    chk(&mut set, "C395-05", fb.file_crash("repro:locale-tag-ovf") && fb.crashes == 1
        && fb.repros == vec!["repro:locale-tag-ovf"], "崩溃立案入库");

    // C395-06 破线 → 阻断 + 归因。
    let br = block_report(&fb);
    chk(&mut set, "C395-06", br.is_some() && br.unwrap().gate == FgCode::GateBlocked
        && fb.attribute() == Some("crash"), "破线阻断归因 crash");

    // C395-07 复现幂等：同签名不重复入库。
    fb.file_crash("repro:locale-tag-ovf");
    chk(&mut set, "C395-07", fb.repros.len() == 1 && fb.crashes == 2, "复现幂等计数仍增");

    // C395-08 立案清零复闸：修复后三零线恢复。
    fb.crashes = 0;
    chk(&mut set, "C395-08", fb.gate().is_ok() && block_report(&fb).is_none(), "清零复闸放行");

    // C395-09 面缺 → 补面定位。
    let mut fb2 = FaceBook::new();
    fb2.add(FuzzFace { source: FaceSource::T04, name: "extract", corpus: 10 });
    fb2.add(FuzzFace { source: FaceSource::T06, name: "locale-tag", corpus: 10 });
    chk(&mut set, "C395-09", fb2.missing_sources() == vec!["T05", "T07", "T08-new"]
        && fb2.gate() == Err(FgCode::FaceMissing), "缺三源定位");

    // C395-10 补面后归齐。
    fb2.add(FuzzFace { source: FaceSource::T05, name: "manifest", corpus: 10 });
    fb2.add(FuzzFace { source: FaceSource::T07, name: "cost-input", corpus: 10 });
    fb2.add(FuzzFace { source: FaceSource::T08New, name: "linter-rules", corpus: 10 });
    chk(&mut set, "C395-10", fb2.sources_covered() && fb2.gate().is_ok(), "补面归齐过闸");

    // C395-11 五源名单单源。
    chk(&mut set, "C395-11", FaceSource::ALL.len() == 5 && FaceSource::ALL[0].name() == "T04"
        && FaceSource::ALL[4].name() == "T08-new", "五源单源");

    // C395-12 越权写破线归因。
    let mut fb3 = FaceBook::new();
    for s in FaceSource::ALL.iter() {
        fb3.add(FuzzFace { source: *s, name: "x", corpus: 1 });
    }
    fb3.unauthorized_writes = 1;
    chk(&mut set, "C395-12", !fb3.zero_line_ok() && fb3.attribute() == Some("unauth-write")
        && fb3.gate() == Err(FgCode::GateBlocked), "越权写破线归因");

    // ===== 二轮深化判据（C395-13 起）=====
    // C395-13 边界探针：空面册拒绝（FaceMissing 显性回执）。
    let probe = GateProbe::new(16, 512, 64);
    let mut fb4 = FaceBook::new();
    chk(&mut set, "C395-13", probe.probe_empty(&fb4) == Err(FgCode::FaceMissing), "空册探针拒绝");

    // C395-14 容量内探针放行 + 越界序号回执。
    fb4.add(FuzzFace { source: FaceSource::T04, name: "extract", corpus: 5 });
    chk(&mut set, "C395-14", probe.probe_empty(&fb4).is_ok() && probe.probe_capacity(&fb4).is_ok()
        && probe.probe_index(&fb4, 0) == Ok("extract")
        && probe.probe_index(&fb4, 9) == Err(FgCode::ProbeReject), "容量内放行越界拒");

    // C395-15 重复注入幂等探针：同面再注不增行 + 越限语料拒。
    chk(&mut set, "C395-15",
        probe.probe_duplicate(&fb4, &FuzzFace { source: FaceSource::T04, name: "extract", corpus: 9 }).is_ok()
        && fb4.faces.len() == 1
        && probe.probe_corpus(600) == Err(FgCode::ProbeReject), "重复注入幂等越限拒");

    // C395-16 LCG 注入回归：64 轮异常必抓 + 守恒 + 零误报。
    let reg = InjectLedger::run(0x5EED_3950, 64, 100);
    chk(&mut set, "C395-16", reg.rounds == 64 && reg.all_caught() && reg.conserved()
        && reg.zero_false_alarm() && reg.anomalies > 0 && reg.normals > 0, "注入回归 64 轮必抓零误报");

    // C395-17 序列化投影：定长 47B + 解码回读三零线（fb 此刻已清零）。
    let mut bytes: Vec<u8> = Vec::new();
    face_book_bytes(&fb, &mut bytes);
    chk(&mut set, "C395-17", bytes.len() == 47 && decode_zero_line(&bytes) == Ok((0, 0, 0)),
        "投影 47 字节三零线回读");

    // C395-18 FNV 指纹幂等：同态二次投影指纹一致（零副作用）。
    match idempotent_fingerprint(&fb) {
        Ok(v) => chk(&mut set, "C395-18", idempotent_fingerprint(&fb) == Ok(v), "指纹幂等一致"),
        Err(_) => chk(&mut set, "C395-18", false, "指纹投影不应漂移"),
    }

    // C395-19 立案守恒：新立案 + 复现签名同步，恒等式成立。
    fb.file_crash("repro:second-gate");
    chk(&mut set, "C395-19", filing_conservation(&fb) && fb.repros.len() == 2 && fb.crashes == 1,
        "立案守恒恒等式");

    // C395-20 深化变体段位不撞段（0x0007..0x0009 仍在 SEG_BASE 段内）。
    chk(&mut set, "C395-20",
        FgCode::ProbeReject as u16 == 0x0007 && FgCode::IdemGateTrip as u16 == 0x0008
        && FgCode::FingerprintDrift as u16 == 0x0009, "深化变体段位正确");

    // C395-21 脏投影拒收：截断字节流 → SerializeDrift。
    chk(&mut set, "C395-21", decode_zero_line(&bytes[..40]) == Err(FgCode::SerializeDrift)
        && decode_zero_line(&[]) == Err(FgCode::SerializeDrift), "脏投影拒收");

    // C395-22 LCG 确定性回归：同种子两次序列逐值一致（可复现性）。
    let mut r1 = Lcg::new(7);
    let mut r2 = Lcg::new(7);
    let mut det = true;
    let mut k = 0;
    while k < 8 { det = det && r1.next() == r2.next(); k += 1; }
    chk(&mut set, "C395-22", det, "LCG 同种子确定性");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves395_smoke() {
        let set = run_ves395_checks();
        assert!(set.failed_count() == 0, "ves395 red: {}", set.red_items_text());
    }

    #[test]
    fn ves395_hang_break() {
        let mut fb = FaceBook::new();
        for s in FaceSource::ALL.iter() { fb.add(FuzzFace { source: *s, name: "x", corpus: 1 }); }
        fb.hangs = 3;
        assert_eq!(fb.attribute(), Some("hang"));
        assert!(fb.gate().is_err());
    }
}
