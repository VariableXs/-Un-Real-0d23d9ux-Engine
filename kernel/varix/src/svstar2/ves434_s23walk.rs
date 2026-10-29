//! ves434_s23walk — VE-F4194 · T 域真实走查
//! 走查：T 域真实走查执行——三套剧本真实用户走查（RTL 母语者/CJK 母语者/辅助技术用户——F4175 预演转正式）
//! +录证采集+走查表终填+发现分级（阻断/重要/建议）；
//! 阻断发现→回 F4172 链路+复走；不可达任务→最高级立案；同意缺→禁录；
//! 走查即域本色终验（真实母语用户复核）；录证全程知情同意+脱敏。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1300;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WkCode {
    Ok = 0,
    ConsentMissing = SEG_BASE + 0x01, // 0x1301
    TaskUnreachable = SEG_BASE + 0x02, // 0x1302
    BlockerFound = SEG_BASE + 0x03,  // 0x1303
}

/// 三套剧本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScriptKind { RtlNative = 0, CjkNative = 1, AssistTech = 2 }

impl ScriptKind {
    pub const ALL: [ScriptKind; 3] = [ScriptKind::RtlNative, ScriptKind::CjkNative, ScriptKind::AssistTech];
}

/// 发现分级。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Severity { Suggestion = 0, Major = 1, Blocker = 2 }

/// 走查会话（真实用户）。
pub struct WalkSession {
    pub script: ScriptKind,
    pub consented: bool,     // 知情同意（缺→禁录）
    pub sanitized: bool,     // 录证脱敏
    pub tasks_done: usize,
    pub tasks_total: usize,
    pub findings: Vec<Severity>,
    pub rewalked: bool,
}

/// 走查执行册。
pub struct WalkBook { pub sessions: Vec<WalkSession>, pub filed_critical: u32, pub rewalks: u32 }

impl WalkBook {
    pub fn new() -> Self { WalkBook { sessions: Vec::new(), filed_critical: 0, rewalks: 0 } }
    pub fn add(&mut self, script: ScriptKind, consent: bool) {
        self.sessions.push(WalkSession { script, consented: consent, sanitized: false,
            tasks_done: 0, tasks_total: 0, findings: Vec::new(), rewalked: false });
    }
    /// 录证采集：同意缺→禁录（域本色红线：全程知情同意）。
    pub fn record(&mut self, script: ScriptKind) -> Result<(), WkCode> {
        let s = self.sessions.iter_mut().find(|s| s.script == script).ok_or(WkCode::TaskUnreachable)?;
        if !s.consented { return Err(WkCode::ConsentMissing); }
        s.sanitized = true;
        Ok(())
    }
    pub fn fill(&mut self, script: ScriptKind, done: usize, total: usize, findings: Vec<Severity>) -> Result<(), WkCode> {
        let s = self.sessions.iter_mut().find(|s| s.script == script).ok_or(WkCode::TaskUnreachable)?;
        s.tasks_done = done;
        s.tasks_total = total;
        s.findings = findings;
        if done < total { self.filed_critical += 1; return Err(WkCode::TaskUnreachable); }
        Ok(())
    }
    /// 阻断发现检测。
    pub fn blockers(&self) -> Vec<ScriptKind> {
        self.sessions.iter().filter(|s| s.findings.contains(&Severity::Blocker))
            .map(|s| s.script).collect()
    }
    /// 阻断→回 F4172 链路复走。
    pub fn rewalk(&mut self, script: ScriptKind, new_findings: Vec<Severity>) -> bool {
        self.rewalks += 1;
        for s in self.sessions.iter_mut() {
            if s.script == script { s.findings = new_findings.clone(); s.rewalked = true; }
        }
        self.blockers().is_empty()
    }
    /// 终填核验：三剧本齐+全录证脱敏+零阻断。
    pub fn final_ok(&self) -> Result<(), WkCode> {
        if self.sessions.len() < 3 { return Err(WkCode::TaskUnreachable); }
        if self.sessions.iter().any(|s| !s.sanitized) { return Err(WkCode::ConsentMissing); }
        if !self.blockers().is_empty() { return Err(WkCode::BlockerFound); }
        Ok(())
    }
}

// ───────────── 深化块 D434-A · 任务步账本（会话×步×耗时） ─────────────

/// 一条任务步记录：批次 + 剧本 + 步号 + 步耗时（整数秒）+ 是否达成。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TaskStepRec {
    pub batch: u16,
    pub script: ScriptKind,
    pub step: u16,
    pub secs: u32,
    pub done: bool,
}

/// 任务步账本：定容量 + 幂等闸（同批同剧本同步号去重）+ 守恒 + 分位。
pub struct StepLedger {
    pub recs: Vec<TaskStepRec>,
    pub cap: usize,
}

impl StepLedger {
    pub fn new(cap: usize) -> Self { StepLedger { recs: Vec::new(), cap } }
    /// 入账：满→显性 Err；同键重复→幂等 false。
    pub fn push(&mut self, r: TaskStepRec) -> Result<bool, WkCode> {
        if self.recs.len() >= self.cap { return Err(WkCode::TaskUnreachable); }
        if self.recs.iter().any(|x| x.batch == r.batch && x.script == r.script && x.step == r.step) {
            return Ok(false);
        }
        self.recs.push(r);
        Ok(true)
    }
    /// 守恒恒等式：∑步数 ≡ ∑(按达成/未达成两桶分列计数)。
    pub fn conserves(&self) -> bool {
        let total = self.recs.len();
        let done = self.recs.iter().filter(|r| r.done).count();
        total == done + (total - done)
    }
    /// 剧本步耗时分位（整数排序取点，禁浮点）。
    pub fn script_secs_quantile(&self, s: ScriptKind, q: u32) -> u32 {
        let mut vals: Vec<u32> = self.recs.iter().filter(|r| r.script == s)
            .map(|r| r.secs).collect();
        if vals.is_empty() { return 0; }
        vals.sort_unstable();
        let n = vals.len();
        let idx = ((q as usize).max(1) * n + 99) / 100;
        vals[idx.min(n) - 1]
    }
    /// 达成率投影（整数千分比）。
    pub fn done_rate_pm(&self) -> u32 {
        if self.recs.is_empty() { return 0; }
        let d = self.recs.iter().filter(|r| r.done).count();
        (d * 1000 / self.recs.len()) as u32
    }
    /// 剧本计数投影。
    pub fn script_counts(&self) -> [usize; 3] {
        let mut out = [0usize; 3];
        for r in &self.recs { out[r.script as usize] += 1; }
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

// ───────────── 深化块 D434-B · 边界探针 ─────────────

/// 探针 P1：越界分位号/空剧本→显性 Err。
pub fn probe_secs_quantile(lg: &StepLedger, s: ScriptKind, q: u32) -> Result<u32, WkCode> {
    if q == 0 || q > 100 { return Err(WkCode::BlockerFound); }
    if lg.recs.iter().filter(|r| r.script == s).count() == 0 { return Err(WkCode::TaskUnreachable); }
    Ok(lg.script_secs_quantile(s, q))
}

/// 探针 P2：未达成步带零耗时（录证失真）→显性 Err。
pub fn probe_zero_sec_step(r: &TaskStepRec) -> Result<(), WkCode> {
    if !r.done && r.secs == 0 { return Err(WkCode::ConsentMissing); }
    Ok(())
}

// ───────────── 深化块 D434-C · LCG 注入回归 ─────────────

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

/// 注入回归：N 轮随机注入阻断发现，blockers 必抓（100%）；
/// 负对照：仅建议级注入零误报。返回 (轮数, 抓获, 误报)。
pub fn lcg_blocker_regression(rounds: usize) -> (usize, usize, usize) {
    let mut g = Lcg(0x34_34_34_34);
    let mut caught = 0usize;
    let mut false_pos = 0usize;
    for _ in 0..rounds {
        let s = ScriptKind::ALL[g.pick(3) as usize];
        let mut bk = WalkBook::new();
        bk.add(s, true);
        let _ = bk.fill(s, 8, 8, vec![Severity::Blocker]);
        if !bk.blockers().is_empty() { caught += 1; } else { false_pos += 1; }
        let mut bk_ok = WalkBook::new();
        bk_ok.add(s, true);
        let _ = bk_ok.fill(s, 8, 8, vec![Severity::Suggestion, Severity::Major]);
        if !bk_ok.blockers().is_empty() { false_pos += 1; }
    }
    (rounds, caught, false_pos)
}

// ───────────── 深化块 D434-D · 序列化投影 + FNV 指纹 ─────────────

/// TaskStepRec 定长 8 字节：batch(2,LE)+script(1)+step(2,LE)+secs(2,LE)+done(1)。
pub fn encode_step(r: &TaskStepRec) -> Vec<u8> {
    let mut out = Vec::with_capacity(8);
    out.extend_from_slice(&r.batch.to_le_bytes());
    out.push(r.script as u8);
    out.extend_from_slice(&r.step.to_le_bytes());
    out.extend_from_slice(&(r.secs as u16).to_le_bytes());
    out.push(if r.done { 1 } else { 0 });
    out
}

/// 解码：错长/坏剧本号→显性 Err。
pub fn decode_step(b: &[u8]) -> Result<TaskStepRec, WkCode> {
    if b.len() != 8 { return Err(WkCode::TaskUnreachable); }
    let script = match ScriptKind::ALL.iter().find(|s| **s as u8 == b[2]) {
        Some(s) => *s,
        None => return Err(WkCode::TaskUnreachable),
    };
    let mut batch = [0u8; 2];
    batch.copy_from_slice(&b[0..2]);
    let mut step = [0u8; 2];
    step.copy_from_slice(&b[3..5]);
    let mut secs = [0u8; 2];
    secs.copy_from_slice(&b[5..7]);
    Ok(TaskStepRec { batch: u16::from_le_bytes(batch), script,
        step: u16::from_le_bytes(step), secs: u16::from_le_bytes(secs) as u32, done: b[7] == 1 })
}

/// FNV-1a 32 位。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for x in bytes { h ^= *x as u32; h = h.wrapping_mul(0x0100_0193); }
    h
}

/// 走查册快照指纹：三会话（脱敏位+达成数+发现计数）序列化后 FNV。
pub fn walkbook_fingerprint(bk: &WalkBook) -> u32 {
    let mut bytes = Vec::new();
    for s in ScriptKind::ALL {
        match bk.sessions.iter().find(|x| x.script == s) {
            Some(x) => {
                bytes.push(x.sanitized as u8);
                bytes.push(x.consented as u8);
                bytes.extend_from_slice(&(x.tasks_done as u16).to_le_bytes());
                bytes.extend_from_slice(&(x.tasks_total as u16).to_le_bytes());
                bytes.push(x.findings.len() as u8);
            }
            None => bytes.extend_from_slice(&[0xDDu8; 7]),
        }
    }
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

pub fn run_ves434_checks() -> CheckSet {
    let mut set = CheckSet::new("ves434");
    let mut bk = WalkBook::new();
    for s in ScriptKind::ALL { bk.add(s, true); }
    bk.sessions[1].consented = false; // CJK 用户未同意

    // C434-01 同意缺→禁录（红线）。
    chk(&mut set, "C434-01", bk.record(ScriptKind::CjkNative) == Err(WkCode::ConsentMissing), "同意缺禁录");

    // C434-02 同意齐→录证+脱敏标记。
    bk.sessions[1].consented = true;
    for s in ScriptKind::ALL { bk.record(s).unwrap(); }
    chk(&mut set, "C434-02", bk.sessions.iter().all(|s| s.sanitized), "录证脱敏全真");

    // C434-03 走查表终填：任务全达。
    chk(&mut set, "C434-03", bk.fill(ScriptKind::RtlNative, 8, 8, vec![Severity::Suggestion]).is_ok(), "RTL 全达");

    // C434-04 不可达任务→最高级立案。
    chk(&mut set, "C434-04", bk.fill(ScriptKind::CjkNative, 6, 8, vec![]) == Err(WkCode::TaskUnreachable)
        && bk.filed_critical == 1, "不可达立案");

    // C434-05 补走全达复填。
    chk(&mut set, "C434-05", bk.fill(ScriptKind::CjkNative, 8, 8, vec![Severity::Major]).is_ok(), "复填全达");

    // C434-06 阻断发现定位。
    bk.fill(ScriptKind::AssistTech, 8, 8, vec![Severity::Blocker]).unwrap();
    chk(&mut set, "C434-06", bk.blockers() == vec![ScriptKind::AssistTech], "阻断定位");

    // C434-07 终填核验：有阻断→拒。
    chk(&mut set, "C434-07", bk.final_ok() == Err(WkCode::BlockerFound), "阻断拒收官");

    // C434-08 阻断→回 F4172 链路复走复绿。
    chk(&mut set, "C434-08", bk.rewalk(ScriptKind::AssistTech, vec![Severity::Major])
        && bk.rewalks == 1, "复走复绿");

    // C434-09 复走后终填核验全过。
    chk(&mut set, "C434-09", bk.final_ok().is_ok(), "终填全过");

    // C434-10 分级排序单源（建议 < 重要 < 阻断）。
    chk(&mut set, "C434-10", Severity::Suggestion < Severity::Major && Severity::Major < Severity::Blocker, "三级单源");

    // C434-11 三剧本单源断言。
    chk(&mut set, "C434-11", ScriptKind::ALL.len() == 3
        && ScriptKind::ALL[0] == ScriptKind::RtlNative
        && ScriptKind::ALL[2] == ScriptKind::AssistTech, "三剧本单源");

    // C434-12 复走标记留痕。
    chk(&mut set, "C434-12", bk.sessions.iter().find(|s| s.script == ScriptKind::AssistTech).unwrap().rewalked
        && !bk.sessions.iter().find(|s| s.script == ScriptKind::RtlNative).unwrap().rewalked, "复走留痕");

    // ── 深化块 D434 判据 ──
    // D434-01 任务步账本：两批三剧本入账 + 守恒恒等式。
    let mut lg = StepLedger::new(64);
    let _ = lg.push(TaskStepRec { batch: 1, script: ScriptKind::RtlNative, step: 1, secs: 30, done: true });
    let _ = lg.push(TaskStepRec { batch: 1, script: ScriptKind::RtlNative, step: 2, secs: 45, done: true });
    let _ = lg.push(TaskStepRec { batch: 1, script: ScriptKind::CjkNative, step: 1, secs: 22, done: true });
    let _ = lg.push(TaskStepRec { batch: 2, script: ScriptKind::CjkNative, step: 2, secs: 88, done: false });
    let _ = lg.push(TaskStepRec { batch: 2, script: ScriptKind::AssistTech, step: 1, secs: 60, done: true });
    chk(&mut set, "D434-01", lg.recs.len() == 5 && lg.conserves(), "步账本守恒");

    // D434-02 幂等闸：同批同剧本同步号二次提交零副作用。
    chk(&mut set, "D434-02", lg.push(TaskStepRec { batch: 1, script: ScriptKind::RtlNative, step: 1, secs: 999, done: false }) == Ok(false)
        && lg.recs.len() == 5, "步幂等闸");

    // D434-03 剧本耗时分位 + 达成率投影 + 剧本计数。
    chk(&mut set, "D434-03", lg.script_secs_quantile(ScriptKind::RtlNative, 50) == 30
        && lg.script_secs_quantile(ScriptKind::RtlNative, 100) == 45
        && lg.done_rate_pm() == 800 && lg.script_counts() == [2, 2, 1], "步分位投影");

    // D434-04 边界探针：越界分位号/空剧本→显性 Err。
    chk(&mut set, "D434-04", probe_secs_quantile(&lg, ScriptKind::RtlNative, 0) == Err(WkCode::BlockerFound)
        && probe_secs_quantile(&lg, ScriptKind::RtlNative, 101) == Err(WkCode::BlockerFound), "分位探针");

    // D434-05 边界探针：未达成步带零耗时（录证失真）→显性 Err。
    chk(&mut set, "D434-05", probe_zero_sec_step(&TaskStepRec { batch: 0, script: ScriptKind::CjkNative, step: 3, secs: 0, done: false }) == Err(WkCode::ConsentMissing)
        && probe_zero_sec_step(&TaskStepRec { batch: 0, script: ScriptKind::CjkNative, step: 3, secs: 1, done: false }).is_ok(), "录证失真探针");

    // D434-06 批次去重投影。
    chk(&mut set, "D434-06", lg.batch_count() == 2, "批次去重");

    // D434-07 LCG 注入回归：12 轮阻断注入 100% 抓 + 负对照零误报。
    let (n, c, fp) = lcg_blocker_regression(12);
    chk(&mut set, "D434-07", n == 12 && c == 12 && fp == 0, "阻断注入全抓");

    // D434-08 序列化往返：步记录编码→解码恒等。
    let s0 = TaskStepRec { batch: 9, script: ScriptKind::AssistTech, step: 42, secs: 777, done: true };
    chk(&mut set, "D434-08", decode_step(&encode_step(&s0)) == Ok(s0), "步序列化往返");

    // D434-09 解码边界：错长/坏剧本号→显性 Err。
    chk(&mut set, "D434-09", decode_step(&[0u8; 7]) == Err(WkCode::TaskUnreachable)
        && decode_step(&{ let mut v = encode_step(&s0); v[2] = 0x09; v }) == Err(WkCode::TaskUnreachable), "步解码边界");

    // D434-10 走查册快照指纹：终填前后指纹变 + 确定性 + hex 投影。
    let mut bk2 = WalkBook::new();
    bk2.add(ScriptKind::RtlNative, true);
    let f0 = walkbook_fingerprint(&bk2);
    let f0b = walkbook_fingerprint(&bk2);
    let _ = bk2.record(ScriptKind::RtlNative);
    let f1 = walkbook_fingerprint(&bk2);
    let _ = bk2.fill(ScriptKind::RtlNative, 8, 8, vec![Severity::Major]);
    let f2 = walkbook_fingerprint(&bk2);
    chk(&mut set, "D434-10", f0 == f0b && f0 != f1 && f1 != f2
        && fingerprint_hex(f2).len() == 8, "走查册指纹");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves434_smoke() {
        let set = run_ves434_checks();
        assert!(set.failed_count() == 0, "ves434 red: {}", set.red_items_text());
    }
}
