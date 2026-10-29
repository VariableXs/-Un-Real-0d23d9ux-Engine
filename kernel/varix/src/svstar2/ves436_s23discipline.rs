//! ves436_s23discipline — VE-F4196 · T 域变更纪律与承诺条款
//! 纪律：T 域冻结后变更纪律（变更申请→影响面评估→双签→哈希更新→通告五步——步步留痕）
//! +承诺条款（对外双承诺状态终审+违诺处置）；
//! 越步变更→作废+回退；违诺→处置+通告；评估缺→拒批；
//! 变更评估含多语言无障碍影响面专列（域本色）；承诺不含用户数据越界条款。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1340;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DcCode {
    Ok = 0,
    StepSkipped = SEG_BASE + 0x01, // 0x1341 越步
    AssessMissing = SEG_BASE + 0x02, // 0x1342 评估缺
    Breach = SEG_BASE + 0x03,      // 0x1343 违诺
    Rejected = SEG_BASE + 0x04,    // 0x1344 拒批
}

/// 变更五步。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChangeStep { Apply = 0, Assess = 1, DualSign = 2, HashUpdate = 3, Notify = 4 }

impl ChangeStep {
    pub const ALL: [ChangeStep; 5] = [
        ChangeStep::Apply, ChangeStep::Assess, ChangeStep::DualSign, ChangeStep::HashUpdate, ChangeStep::Notify];
}

/// 影响面评估（含多语言无障碍专列——域本色）。
pub struct ImpactAssess { pub scope: u32, pub a11y_impact: bool, pub a11y_note: bool }

/// 变更纪律册。
pub struct DisciplineBook {
    pub steps_done: Vec<ChangeStep>,
    pub assess: Option<ImpactAssess>,
    pub signers: [&'static str; 2],
    pub hash_updated: bool,
    pub notified: bool,
    pub voided: u32,
    pub reverted: u32,
    pub filed: u32,
}

impl DisciplineBook {
    pub fn new() -> Self {
        DisciplineBook { steps_done: Vec::new(), assess: None, signers: ["", ""], hash_updated: false,
            notified: false, voided: 0, reverted: 0, filed: 0 }
    }
    pub fn set_assess(&mut self, scope: u32, a11y_impact: bool, a11y_note: bool) {
        self.assess = Some(ImpactAssess { scope, a11y_impact, a11y_note });
    }
    pub fn do_step(&mut self, s: ChangeStep) -> Result<(), DcCode> {
        // 步序强制：只允许做紧邻下一步（越步→作废+回退）。
        let idx = s as usize;
        if idx != self.steps_done.len() {
            self.voided += 1;
            self.reverted += 1;
            self.steps_done.clear();
            return Err(DcCode::StepSkipped);
        }
        // 评估步前置：评估未做→拒批（Apply 后必须先评估再走后续步）。
        if idx >= 2 && self.assess.is_none() {
            self.filed += 1;
            return Err(DcCode::AssessMissing);
        }
        // 评估缺无障碍专列说明→拒批（域本色）。
        if idx >= 2 {
            if let Some(a) = &self.assess {
                if a.a11y_impact && !a.a11y_note { self.filed += 1; return Err(DcCode::AssessMissing); }
            }
        }
        if s == ChangeStep::DualSign && (self.signers[0].is_empty() || self.signers[1].is_empty()) {
            return Err(DcCode::Rejected);
        }
        self.steps_done.push(s);
        if s == ChangeStep::HashUpdate { self.hash_updated = true; }
        if s == ChangeStep::Notify { self.notified = true; }
        Ok(())
    }
    pub fn complete(&self) -> bool { self.steps_done.len() == 5 && self.hash_updated && self.notified }
}

/// 双承诺台账（对外双承诺状态终审+违诺处置）。
pub struct PromiseBook {
    pub promises: Vec<(&'static str, bool)>, // (承诺名, 守诺)
    pub breaches: u32,
    pub notices: u32,
}

impl PromiseBook {
    pub fn new() -> Self { PromiseBook { promises: Vec::new(), breaches: 0, notices: 0 } }
    pub fn put(&mut self, name: &'static str, kept: bool) {
        for p in self.promises.iter_mut() { if p.0 == name { p.1 = kept; return; } }
        self.promises.push((name, kept));
    }
    /// 双承诺终审：两条承诺全在册且全守诺。
    pub fn dual_ok(&self) -> bool {
        self.promises.len() == 2 && self.promises.iter().all(|(_, k)| *k)
    }
    /// 违诺处置：违诺→处置+通告（无用户数据越界条款复述）。
    pub fn audit(&mut self) -> Result<(), DcCode> {
        let broken: Vec<&'static str> = self.promises.iter().filter(|(_, k)| !k).map(|(n, _)| *n).collect();
        if broken.is_empty() { return Ok(()); }
        self.breaches += broken.len() as u32;
        self.notices += broken.len() as u32;
        Err(DcCode::Breach)
    }
}

// ───────────── 深化块 D436-A · 变更批账本（批次×步×工时） ─────────────

/// 一条变更步入账：批次 + 步 + 执行人序号 + 工时（整数分钟）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ChangeRec {
    pub batch: u16,
    pub step: ChangeStep,
    pub actor: u8,
    pub mins: u32,
}

/// 变更批账本：定容量 + 幂等闸（同批同步去重）+ 守恒 + 分位。
pub struct ChangeLedger {
    pub recs: Vec<ChangeRec>,
    pub cap: usize,
}

impl ChangeLedger {
    pub fn new(cap: usize) -> Self { ChangeLedger { recs: Vec::new(), cap } }
    /// 入账：满→显性 Err；同批同步重复→幂等 false。
    pub fn push(&mut self, r: ChangeRec) -> Result<bool, DcCode> {
        if self.recs.len() >= self.cap { return Err(DcCode::Rejected); }
        if self.recs.iter().any(|x| x.batch == r.batch && x.step == r.step) { return Ok(false); }
        self.recs.push(r);
        Ok(true)
    }
    /// 守恒恒等式：∑工时 ≡ ∑(按五步分列求和)。
    pub fn conserves(&self) -> bool {
        let total: u64 = self.recs.iter().map(|r| r.mins as u64).sum();
        let mut by_step: u64 = 0;
        for s in ChangeStep::ALL {
            by_step += self.recs.iter().filter(|r| r.step == s)
                .map(|r| r.mins as u64).sum::<u64>();
        }
        total == by_step
    }
    /// 步工时分位（整数排序取点，禁浮点）。
    pub fn step_mins_quantile(&self, s: ChangeStep, q: u32) -> u32 {
        let mut vals: Vec<u32> = self.recs.iter().filter(|r| r.step == s)
            .map(|r| r.mins).collect();
        if vals.is_empty() { return 0; }
        vals.sort_unstable();
        let n = vals.len();
        let idx = ((q as usize).max(1) * n + 99) / 100;
        vals[idx.min(n) - 1]
    }
    /// 步覆盖投影：五步中已入账步数。
    pub fn step_coverage(&self) -> usize {
        let mut ss: Vec<u8> = self.recs.iter().map(|r| r.step as u8).collect();
        ss.sort_unstable();
        ss.dedup();
        ss.len()
    }
    /// 执行人分列计数（actor 0..=3 桶）。
    pub fn actor_counts(&self) -> [usize; 4] {
        let mut out = [0usize; 4];
        for r in &self.recs { if (r.actor as usize) < 4 { out[r.actor as usize] += 1; } }
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

// ───────────── 深化块 D436-B · 边界探针 ─────────────

/// 探针 P1：越界分位号/空步→显性 Err。
pub fn probe_mins_quantile(lg: &ChangeLedger, s: ChangeStep, q: u32) -> Result<u32, DcCode> {
    if q == 0 || q > 100 { return Err(DcCode::StepSkipped); }
    if lg.recs.iter().filter(|r| r.step == s).count() == 0 { return Err(DcCode::AssessMissing); }
    Ok(lg.step_mins_quantile(s, q))
}

/// 探针 P2：零工时变更步（无留痕）→显性 Err（步步留痕纪律）。
pub fn probe_zero_mins(r: &ChangeRec) -> Result<(), DcCode> {
    if r.mins == 0 { return Err(DcCode::StepSkipped); }
    Ok(())
}

// ───────────── 深化块 D436-C · LCG 注入回归 ─────────────

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

/// 注入回归：N 轮随机越步注入（先做非 Apply 步），do_step 必抓
/// （StepSkipped 100%）+作废回退计数必增；负对照：按序步零误报。
pub fn lcg_skip_regression(rounds: usize) -> (usize, usize, usize) {
    let mut g = Lcg(0x36_36_36_36);
    let mut caught = 0usize;
    let mut false_pos = 0usize;
    for _ in 0..rounds {
        let s = ChangeStep::ALL[(g.pick(4) + 1) as usize]; // 必非 Apply
        let mut bk = DisciplineBook::new();
        if bk.do_step(s) == Err(DcCode::StepSkipped) && bk.voided == 1 && bk.reverted == 1 {
            caught += 1;
        } else { false_pos += 1; }
        // 负对照：首步按序零误报。
        let mut bk_ok = DisciplineBook::new();
        if bk_ok.do_step(ChangeStep::Apply).is_err() { false_pos += 1; }
    }
    (rounds, caught, false_pos)
}

// ───────────── 深化块 D436-D · 序列化投影 + FNV 指纹 ─────────────

/// ChangeRec 定长 8 字节：batch(2,LE)+step(1)+actor(1)+mins(4,LE)。
pub fn encode_change(r: &ChangeRec) -> Vec<u8> {
    let mut out = Vec::with_capacity(8);
    out.extend_from_slice(&r.batch.to_le_bytes());
    out.push(r.step as u8);
    out.push(r.actor);
    out.extend_from_slice(&r.mins.to_le_bytes());
    out
}

/// 解码：错长/坏步号→显性 Err。
pub fn decode_change(b: &[u8]) -> Result<ChangeRec, DcCode> {
    if b.len() != 8 { return Err(DcCode::Rejected); }
    let step = match ChangeStep::ALL.iter().find(|s| **s as u8 == b[2]) {
        Some(s) => *s,
        None => return Err(DcCode::StepSkipped),
    };
    let mut batch = [0u8; 2];
    batch.copy_from_slice(&b[0..2]);
    let mut mins = [0u8; 4];
    mins.copy_from_slice(&b[4..8]);
    Ok(ChangeRec { batch: u16::from_le_bytes(batch), step, actor: b[3], mins: u32::from_le_bytes(mins) })
}

/// FNV-1a 32 位。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for x in bytes { h ^= *x as u32; h = h.wrapping_mul(0x0100_0193); }
    h
}

/// 纪律册快照指纹：五步位图+双签+评估摘要+留痕计数序列化后 FNV。
pub fn discipline_fingerprint(bk: &DisciplineBook) -> u32 {
    let mut bytes = Vec::new();
    for s in ChangeStep::ALL { bytes.push(if bk.steps_done.contains(&s) { 1 } else { 0 }); }
    bytes.push(if bk.signers[0].is_empty() { 0 } else { 1 });
    bytes.push(if bk.signers[1].is_empty() { 0 } else { 1 });
    bytes.push(bk.assess.is_some() as u8);
    bytes.extend_from_slice(&(bk.voided as u16).to_le_bytes());
    bytes.extend_from_slice(&(bk.reverted as u16).to_le_bytes());
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

pub fn run_ves436_checks() -> CheckSet {
    let mut set = CheckSet::new("ves436");
    let mut bk = DisciplineBook::new();

    // C436-01 越步（直接做第三步）→作废+回退。
    chk(&mut set, "C436-01", bk.do_step(ChangeStep::DualSign) == Err(DcCode::StepSkipped)
        && bk.voided == 1 && bk.reverted == 1 && bk.steps_done.is_empty(), "越步作废回退");

    // C436-02 步序归零后按序走第一步。
    chk(&mut set, "C436-02", bk.do_step(ChangeStep::Apply).is_ok(), "申请步");

    // C436-03 评估缺→拒批（跳过评估直接双签）。
    chk(&mut set, "C436-03", bk.do_step(ChangeStep::DualSign) == Err(DcCode::AssessMissing)
        && bk.filed == 1, "评估缺拒批");

    // C436-04 评估有 a11y 影响但缺专列说明→拒（域本色）。
    bk.set_assess(3, true, false);
    chk(&mut set, "C436-04", bk.do_step(ChangeStep::Assess).is_ok()
        && bk.do_step(ChangeStep::DualSign) == Err(DcCode::AssessMissing), "a11y 专列缺拒");

    // C436-05 补专列说明+双签齐→五步走通。
    bk.set_assess(3, true, true);
    bk.signers = ["wb-a", "wb-b"];
    for s in [ChangeStep::DualSign, ChangeStep::HashUpdate, ChangeStep::Notify] {
        bk.do_step(s).unwrap();
    }
    chk(&mut set, "C436-05", bk.complete(), "五步走通");

    // C436-06 哈希更新与通告留痕断言。
    chk(&mut set, "C436-06", bk.hash_updated && bk.notified, "哈希通告留痕");

    // C436-07 双签缺一→拒。
    let mut bk2 = DisciplineBook::new();
    bk2.set_assess(1, false, false);
    bk2.do_step(ChangeStep::Apply).unwrap();
    bk2.do_step(ChangeStep::Assess).unwrap();
    bk2.signers = ["wb-a", ""];
    chk(&mut set, "C436-07", bk2.do_step(ChangeStep::DualSign) == Err(DcCode::Rejected), "双签缺一拒");

    // C436-08 承诺台账：双承诺全守。
    let mut pb = PromiseBook::new();
    pb.put("frozen-stability", true);
    pb.put("a11y-floor", true);
    chk(&mut set, "C436-08", pb.dual_ok() && pb.audit().is_ok(), "双守诺");

    // C436-09 违诺→处置+通告。
    pb.put("a11y-floor", false);
    chk(&mut set, "C436-09", pb.audit() == Err(DcCode::Breach)
        && pb.breaches == 1 && pb.notices == 1, "违诺处置通告");

    // C436-10 违诺整改复诺复绿。
    pb.put("a11y-floor", true);
    chk(&mut set, "C436-10", pb.audit().is_ok(), "复诺复绿");

    // C436-11 五步单源断言。
    chk(&mut set, "C436-11", ChangeStep::ALL.len() == 5
        && ChangeStep::ALL[0] == ChangeStep::Apply
        && ChangeStep::ALL[4] == ChangeStep::Notify, "五步单源");

    // C436-12 无用户数据越界条款：台账不含该条（复述断言）。
    chk(&mut set, "C436-12", !pb.promises.iter().any(|(n, _)| *n == "user-data-outbound"), "无越界条款");

    // ── 深化块 D436 判据 ──
    // D436-01 变更批账本：两批五步入账 + 守恒恒等式。
    let mut lg = ChangeLedger::new(32);
    let _ = lg.push(ChangeRec { batch: 1, step: ChangeStep::Apply, actor: 0, mins: 10 });
    let _ = lg.push(ChangeRec { batch: 1, step: ChangeStep::Assess, actor: 1, mins: 35 });
    let _ = lg.push(ChangeRec { batch: 1, step: ChangeStep::DualSign, actor: 2, mins: 15 });
    let _ = lg.push(ChangeRec { batch: 2, step: ChangeStep::HashUpdate, actor: 3, mins: 8 });
    let _ = lg.push(ChangeRec { batch: 2, step: ChangeStep::Notify, actor: 0, mins: 5 });
    chk(&mut set, "D436-01", lg.recs.len() == 5 && lg.conserves(), "变更账本守恒");

    // D436-02 幂等闸：同批同步二次提交零副作用。
    chk(&mut set, "D436-02", lg.push(ChangeRec { batch: 1, step: ChangeStep::Apply, actor: 9, mins: 1 }) == Ok(false)
        && lg.recs.len() == 5, "变更幂等闸");

    // D436-03 步工时分位 + 步覆盖 + 执行人分列。
    chk(&mut set, "D436-03", lg.step_mins_quantile(ChangeStep::Assess, 50) == 35
        && lg.step_coverage() == 5 && lg.actor_counts() == [2, 1, 1, 1], "步分位覆盖");

    // D436-04 边界探针：越界分位号/空步→显性 Err。
    chk(&mut set, "D436-04", probe_mins_quantile(&lg, ChangeStep::Apply, 0) == Err(DcCode::StepSkipped)
        && probe_mins_quantile(&lg, ChangeStep::Apply, 101) == Err(DcCode::StepSkipped), "变更分位探针");

    // D436-05 边界探针：零工时步无留痕→显性 Err。
    chk(&mut set, "D436-05", probe_zero_mins(&ChangeRec { batch: 0, step: ChangeStep::Notify, actor: 1, mins: 0 }) == Err(DcCode::StepSkipped)
        && probe_zero_mins(&ChangeRec { batch: 0, step: ChangeStep::Notify, actor: 1, mins: 1 }).is_ok(), "零工时探针");

    // D436-06 批次去重投影。
    chk(&mut set, "D436-06", lg.batch_count() == 2, "变更批次去重");

    // D436-07 LCG 越步注入回归：14 轮 100% 抓 StepSkipped + 作废回退必增 + 零误报。
    let (n, c, fp) = lcg_skip_regression(14);
    chk(&mut set, "D436-07", n == 14 && c == 14 && fp == 0, "越步注入全抓");

    // D436-08 序列化往返：变更记录编码→解码恒等。
    let c0 = ChangeRec { batch: 6, step: ChangeStep::HashUpdate, actor: 3, mins: 8 };
    chk(&mut set, "D436-08", decode_change(&encode_change(&c0)) == Ok(c0), "变更序列化往返");

    // D436-09 解码边界：错长/坏步号→显性 Err。
    chk(&mut set, "D436-09", decode_change(&[0u8; 7]) == Err(DcCode::Rejected)
        && decode_change(&{ let mut v = encode_change(&c0); v[2] = 0x0A; v }) == Err(DcCode::StepSkipped), "变更加码边界");

    // D436-10 纪律册快照指纹：越步前后指纹变 + 确定性 + hex 投影。
    let mut bk3 = DisciplineBook::new();
    let f0 = discipline_fingerprint(&bk3);
    let f0b = discipline_fingerprint(&bk3);
    let _ = bk3.do_step(ChangeStep::DualSign); // 越步：作废+回退
    let f1 = discipline_fingerprint(&bk3);
    let _ = bk3.do_step(ChangeStep::Apply);
    let f2 = discipline_fingerprint(&bk3);
    chk(&mut set, "D436-10", f0 == f0b && f0 != f1 && f1 != f2
        && fingerprint_hex(f2).len() == 8, "纪律册指纹");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves436_smoke() {
        let set = run_ves436_checks();
        assert!(set.failed_count() == 0, "ves436 red: {}", set.red_items_text());
    }
}
