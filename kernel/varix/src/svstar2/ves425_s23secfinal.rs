//! ves425_s23secfinal — VE-F4185 · T 域安全终审
//! 终审：T 域安全三面终审（机翻出站面/语言包供应链面/地区数据路由面——F4169 复审升级为终审）
//! +红线全量复核+用户内容不出站专项断言；
//! 面未清→阻断收口；红线破→最高级立案（复述红线实测）；断言缺→补立；
//! 安全终审与隐私红线并审（出站+驻留双核心断言——域本色）；无新增面。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x10A0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SfCode {
    Ok = 0,
    FaceUnclear = SEG_BASE + 0x01,   // 0x10A1
    RedlineBreach = SEG_BASE + 0x02, // 0x10A2
    AssertMissing = SEG_BASE + 0x03, // 0x10A3
}

/// 安全三面。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SecFace { MtOutbound = 0, LpSupplyChain = 1, RegionRouting = 2 }

impl SecFace {
    pub const ALL: [SecFace; 3] = [SecFace::MtOutbound, SecFace::LpSupplyChain, SecFace::RegionRouting];
    pub fn name(self) -> &'static str {
        match self {
            SecFace::MtOutbound => "mt-outbound", SecFace::LpSupplyChain => "lp-supply",
            SecFace::RegionRouting => "region-routing",
        }
    }
}

/// 红线双核心（域本色并审）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RedlineCore { Outbound = 0, Residency = 1 }

/// 安全终审册。
pub struct SecFinalBook {
    pub face_clear: Vec<SecFace>,        // 已清面
    pub redlines_ok: Vec<RedlineCore>,   // 复核通过红线
    pub asserts: Vec<&'static str>,      // 专项断言集
    pub filed_critical: bool,            // 最高级立案位
}

impl SecFinalBook {
    pub fn new() -> Self {
        SecFinalBook { face_clear: Vec::new(), redlines_ok: Vec::new(), asserts: Vec::new(), filed_critical: false }
    }
    pub fn clear_face(&mut self, f: SecFace) { if !self.face_clear.contains(&f) { self.face_clear.push(f); } }
    pub fn pass_redline(&mut self, r: RedlineCore) { if !self.redlines_ok.contains(&r) { self.redlines_ok.push(r); } }
    pub fn add_assert(&mut self, name: &'static str) { if !self.asserts.contains(&name) { self.asserts.push(name); } }
    /// 红线破→最高级立案（实测触发位）。
    pub fn breach(&mut self) { self.filed_critical = true; }
    /// 面未清→阻断收口。
    pub fn faces_block(&self) -> Option<SfCode> {
        for f in SecFace::ALL {
            if !self.face_clear.contains(&f) { return Some(SfCode::FaceUnclear); }
        }
        None
    }
    /// 断言缺→补立：专项断言集必须含「不出站」与「驻留延续」两条。
    pub fn asserts_complete(&self) -> bool {
        self.asserts.contains(&"user-content-no-outbound") && self.asserts.contains(&"residency-continuity")
    }
    /// 终审裁决：三面清+双红线过+断言齐+无立案。
    pub fn verdict(&self) -> Result<(), SfCode> {
        if let Some(c) = self.faces_block() { return Err(c); }
        if self.filed_critical { return Err(SfCode::RedlineBreach); }
        if !self.asserts_complete() { return Err(SfCode::AssertMissing); }
        if self.redlines_ok.len() < 2 { return Err(SfCode::RedlineBreach); }
        Ok(())
    }
}

// ───────────── 深化块 D425-A · 安全面清核批账本（批次×面×轮次） ─────────────

/// 一条清核入账：批次 + 安全面 + 清核轮次 + 用时（整数分钟）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SecAuditRec {
    pub batch: u16,
    pub face: SecFace,
    pub round: u32,
    pub mins: u32,
}

/// 清核账本：定容量 + 幂等闸（同批同面同轮去重）+ 守恒 + 分位。
pub struct SecAuditLedger {
    pub recs: Vec<SecAuditRec>,
    pub cap: usize,
}

impl SecAuditLedger {
    pub fn new(cap: usize) -> Self { SecAuditLedger { recs: Vec::new(), cap } }
    /// 入账：满→显性 Err；同键重复→幂等 false。
    pub fn push(&mut self, r: SecAuditRec) -> Result<bool, SfCode> {
        if self.recs.len() >= self.cap { return Err(SfCode::FaceUnclear); }
        if self.recs.iter().any(|x| x.batch == r.batch && x.face == r.face && x.round == r.round) {
            return Ok(false);
        }
        self.recs.push(r);
        Ok(true)
    }
    /// 守恒恒等式：∑用时 ≡ ∑(按三面分列求和)。
    pub fn conserves(&self) -> bool {
        let total: u64 = self.recs.iter().map(|r| r.mins as u64).sum();
        let mut by_face: u64 = 0;
        for f in SecFace::ALL {
            by_face += self.recs.iter().filter(|r| r.face == f)
                .map(|r| r.mins as u64).sum::<u64>();
        }
        total == by_face
    }
    /// 面内轮次分位（整数排序取点，禁浮点）。
    pub fn face_round_quantile(&self, f: SecFace, q: u32) -> u32 {
        let mut vals: Vec<u32> = self.recs.iter().filter(|r| r.face == f)
            .map(|r| r.round).collect();
        if vals.is_empty() { return 0; }
        vals.sort_unstable();
        let n = vals.len();
        let idx = ((q as usize).max(1) * n + 99) / 100;
        vals[idx.min(n) - 1]
    }
    /// 面覆盖投影：三面中已清核面数。
    pub fn face_coverage(&self) -> usize {
        let mut fs: Vec<u8> = self.recs.iter().map(|r| r.face as u8).collect();
        fs.sort_unstable();
        fs.dedup();
        fs.len()
    }
    /// 批次去重计数。
    pub fn batch_count(&self) -> usize {
        let mut bs: Vec<u16> = self.recs.iter().map(|r| r.batch).collect();
        bs.sort_unstable();
        bs.dedup();
        bs.len()
    }
}

// ───────────── 深化块 D425-B · 边界探针 ─────────────

/// 探针 P1：越界分位号/空面→显性 Err。
pub fn probe_round_quantile(lg: &SecAuditLedger, f: SecFace, q: u32) -> Result<u32, SfCode> {
    if q == 0 || q > 100 { return Err(SfCode::AssertMissing); }
    if lg.recs.iter().filter(|r| r.face == f).count() == 0 { return Err(SfCode::FaceUnclear); }
    Ok(lg.face_round_quantile(f, q))
}

/// 探针 P2：零轮次清核（无效核）→显性 Err。
pub fn probe_zero_round(r: &SecAuditRec) -> Result<(), SfCode> {
    if r.round == 0 { return Err(SfCode::FaceUnclear); }
    Ok(())
}

// ───────────── 深化块 D425-C · LCG 注入回归 ─────────────

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

/// 注入回归：N 轮随机抽走一面清核，verdict 的面闸必抓（FaceUnclear 100%）；
/// 负对照：三面全清零误报。返回 (轮数, 抓获, 误报)。
pub fn lcg_face_gap_regression(rounds: usize) -> (usize, usize, usize) {
    let mut g = Lcg(0x25_25_25_25);
    let mut caught = 0usize;
    let mut false_pos = 0usize;
    for _ in 0..rounds {
        let drop = g.pick(3) as usize;
        let mut bk = SecFinalBook::new();
        for (i, f) in SecFace::ALL.iter().enumerate() {
            if i != drop { bk.clear_face(*f); }
        }
        bk.add_assert("user-content-no-outbound");
        bk.add_assert("residency-continuity");
        bk.pass_redline(RedlineCore::Outbound);
        bk.pass_redline(RedlineCore::Residency);
        if bk.verdict() == Err(SfCode::FaceUnclear) { caught += 1; } else { false_pos += 1; }
        // 负对照：三面全清零误报。
        let mut bk_ok = SecFinalBook::new();
        for f in SecFace::ALL { bk_ok.clear_face(f); }
        bk_ok.add_assert("user-content-no-outbound");
        bk_ok.add_assert("residency-continuity");
        bk_ok.pass_redline(RedlineCore::Outbound);
        bk_ok.pass_redline(RedlineCore::Residency);
        if bk_ok.verdict().is_err() { false_pos += 1; }
    }
    (rounds, caught, false_pos)
}

// ───────────── 深化块 D425-D · 序列化投影 + FNV 指纹 ─────────────

/// SecAuditRec 定长 9 字节：batch(2,LE)+face(1)+round(4,LE)+mins(2,LE)。
pub fn encode_audit(r: &SecAuditRec) -> Vec<u8> {
    let mut out = Vec::with_capacity(9);
    out.extend_from_slice(&r.batch.to_le_bytes());
    out.push(r.face as u8);
    out.extend_from_slice(&r.round.to_le_bytes());
    out.extend_from_slice(&(r.mins as u16).to_le_bytes());
    out
}

/// 解码：错长/坏面号→显性 Err。
pub fn decode_audit(b: &[u8]) -> Result<SecAuditRec, SfCode> {
    if b.len() != 9 { return Err(SfCode::FaceUnclear); }
    let face = match SecFace::ALL.iter().find(|f| **f as u8 == b[2]) {
        Some(f) => *f,
        None => return Err(SfCode::FaceUnclear),
    };
    let mut batch = [0u8; 2];
    batch.copy_from_slice(&b[0..2]);
    let mut rd = [0u8; 4];
    rd.copy_from_slice(&b[3..7]);
    let mut mn = [0u8; 2];
    mn.copy_from_slice(&b[7..9]);
    Ok(SecAuditRec { batch: u16::from_le_bytes(batch), face,
        round: u32::from_le_bytes(rd), mins: u16::from_le_bytes(mn) as u32 })
}

/// FNV-1a 32 位。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for x in bytes { h ^= *x as u32; h = h.wrapping_mul(0x0100_0193); }
    h
}

/// 终审册快照指纹：面位图+红线位图+断言数+立案位序列化后 FNV。
pub fn secfinal_fingerprint(bk: &SecFinalBook) -> u32 {
    let mut bytes = Vec::new();
    for f in SecFace::ALL { bytes.push(if bk.face_clear.contains(&f) { 1 } else { 0 }); }
    bytes.push(if bk.redlines_ok.contains(&RedlineCore::Outbound) { 1 } else { 0 });
    bytes.push(if bk.redlines_ok.contains(&RedlineCore::Residency) { 1 } else { 0 });
    bytes.push(bk.asserts.len() as u8);
    bytes.push(bk.filed_critical as u8);
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

pub fn run_ves425_checks() -> CheckSet {
    let mut set = CheckSet::new("ves425");
    let mut bk = SecFinalBook::new();

    // C425-01 三面未清→阻断收口。
    chk(&mut set, "C425-01", bk.verdict() == Err(SfCode::FaceUnclear), "面未清阻断");

    // C425-02 三面清后仍因断言缺被拒（补立路径）。
    for f in SecFace::ALL { bk.clear_face(f); }
    chk(&mut set, "C425-02", bk.verdict() == Err(SfCode::AssertMissing), "断言缺拒");

    // C425-03 断言缺定位。
    chk(&mut set, "C425-03", !bk.asserts_complete(), "缺两条专项断言");

    // C425-04 补立断言后因红线未过被拒。
    bk.add_assert("user-content-no-outbound");
    chk(&mut set, "C425-04", bk.verdict() == Err(SfCode::AssertMissing), "仍缺驻留断言");

    // C425-05 断言齐+双红线过→终审通过。
    bk.add_assert("residency-continuity");
    bk.pass_redline(RedlineCore::Outbound);
    bk.pass_redline(RedlineCore::Residency);
    chk(&mut set, "C425-05", bk.verdict().is_ok(), "终审通过");

    // C425-06 红线破→最高级立案+终审翻转。
    bk.breach();
    chk(&mut set, "C425-06", bk.verdict() == Err(SfCode::RedlineBreach) && bk.filed_critical, "红线破立案");

    // C425-07 三面名单单源。
    chk(&mut set, "C425-07", SecFace::ALL.len() == 3
        && SecFace::ALL[0].name() == "mt-outbound"
        && SecFace::ALL[1].name() == "lp-supply"
        && SecFace::ALL[2].name() == "region-routing", "三面单源");

    // C425-08 面清幂等（重复清不重复入册）。
    let mut bk2 = SecFinalBook::new();
    bk2.clear_face(SecFace::MtOutbound);
    bk2.clear_face(SecFace::MtOutbound);
    chk(&mut set, "C425-08", bk2.face_clear.len() == 1, "面清幂等");

    // C425-09 双红线单源断言。
    chk(&mut set, "C425-09", RedlineCore::Outbound as u8 == 0 && RedlineCore::Residency as u8 == 1, "双核心单源");

    // C425-10 缺单红线终审拒。
    let mut bk3 = SecFinalBook::new();
    for f in SecFace::ALL { bk3.clear_face(f); }
    bk3.add_assert("user-content-no-outbound");
    bk3.add_assert("residency-continuity");
    bk3.pass_redline(RedlineCore::Outbound);
    chk(&mut set, "C425-10", bk3.verdict() == Err(SfCode::RedlineBreach), "缺红线拒");

    // C425-11 断言补立幂等。
    bk.add_assert("user-content-no-outbound");
    chk(&mut set, "C425-11", bk.asserts.len() == 2, "断言幂等");

    // C425-12 无新增面：终审不引入第四面。
    chk(&mut set, "C425-12", SecFace::ALL.iter().all(|f| bk.face_clear.contains(f))
        && bk.face_clear.len() == 3, "无新增面");

    // ── 深化块 D425 判据 ──
    // D425-01 清核账本：两批三面入账 + 守恒恒等式。
    let mut lg = SecAuditLedger::new(32);
    let _ = lg.push(SecAuditRec { batch: 1, face: SecFace::MtOutbound, round: 1, mins: 40 });
    let _ = lg.push(SecAuditRec { batch: 1, face: SecFace::LpSupplyChain, round: 2, mins: 55 });
    let _ = lg.push(SecAuditRec { batch: 2, face: SecFace::RegionRouting, round: 1, mins: 30 });
    let _ = lg.push(SecAuditRec { batch: 2, face: SecFace::MtOutbound, round: 2, mins: 25 });
    chk(&mut set, "D425-01", lg.recs.len() == 4 && lg.conserves(), "清核账本守恒");

    // D425-02 幂等闸：同批同面同轮二次提交零副作用。
    chk(&mut set, "D425-02", lg.push(SecAuditRec { batch: 1, face: SecFace::MtOutbound, round: 1, mins: 999 }) == Ok(false)
        && lg.recs.len() == 4, "清核幂等闸");

    // D425-03 面轮次分位 + 面覆盖投影。
    chk(&mut set, "D425-03", lg.face_round_quantile(SecFace::MtOutbound, 50) == 1
        && lg.face_round_quantile(SecFace::MtOutbound, 100) == 2
        && lg.face_coverage() == 3, "轮次分位覆盖");

    // D425-04 边界探针：越界分位号/空面→显性 Err。
    chk(&mut set, "D425-04", probe_round_quantile(&lg, SecFace::LpSupplyChain, 0) == Err(SfCode::AssertMissing)
        && probe_round_quantile(&lg, SecFace::LpSupplyChain, 101) == Err(SfCode::AssertMissing), "清核分位探针");

    // D425-05 边界探针：零轮次无效核。
    chk(&mut set, "D425-05", probe_zero_round(&SecAuditRec { batch: 0, face: SecFace::RegionRouting, round: 0, mins: 10 }) == Err(SfCode::FaceUnclear)
        && probe_zero_round(&SecAuditRec { batch: 0, face: SecFace::RegionRouting, round: 1, mins: 10 }).is_ok(), "零轮次探针");

    // D425-06 批次去重投影。
    chk(&mut set, "D425-06", lg.batch_count() == 2, "清核批次去重");

    // D425-07 LCG 面缺注入回归：15 轮 100% 抓 FaceUnclear + 负对照零误报。
    let (n, c, fp) = lcg_face_gap_regression(15);
    chk(&mut set, "D425-07", n == 15 && c == 15 && fp == 0, "面缺注入全抓");

    // D425-08 序列化往返：清核记录编码→解码恒等。
    let a0 = SecAuditRec { batch: 8, face: SecFace::RegionRouting, round: 3, mins: 45 };
    chk(&mut set, "D425-08", decode_audit(&encode_audit(&a0)) == Ok(a0), "清核序列化往返");

    // D425-09 解码边界：错长/坏面号→显性 Err。
    chk(&mut set, "D425-09", decode_audit(&[0u8; 8]) == Err(SfCode::FaceUnclear)
        && decode_audit(&{ let mut v = encode_audit(&a0); v[2] = 0x77; v }) == Err(SfCode::FaceUnclear), "清核解码边界");

    // D425-10 终审册快照指纹：立案位翻转变 + 确定性 + hex 投影。
    let mut bk2 = SecFinalBook::new();
    let f0 = secfinal_fingerprint(&bk2);
    let f0b = secfinal_fingerprint(&bk2);
    bk2.clear_face(SecFace::MtOutbound);
    let f1 = secfinal_fingerprint(&bk2);
    bk2.breach();
    let f2 = secfinal_fingerprint(&bk2);
    chk(&mut set, "D425-10", f0 == f0b && f0 != f1 && f1 != f2
        && fingerprint_hex(f2).len() == 8, "终审册指纹");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves425_smoke() {
        let set = run_ves425_checks();
        assert!(set.failed_count() == 0, "ves425 red: {}", set.red_items_text());
    }
}
