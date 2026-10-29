//! ves440_s23declare — VE-F4200 · T 域收官宣告
//! 宣告：域级收官宣告——总对账（200 项实测 200/200——硬门）；八组收口 8/8；三基线冻结哈希确认；
//! 硬门六重宣告（完备/冻结/零🔴/GO/长稳/心流——T 域六重）；20 维三 90 确认；条件核销；
//! 最终宣告（T 域深化与规格收官——第十七个域宣告——家族第十七位登记）；
//! 任一硬门未过→阻断；失实→撤回+立案；逾期→复议。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x13E0;
pub const DOMAIN_ITEMS: usize = 200;
pub const DOMAIN_SEQ: usize = 17; // 家族第十七位

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DcCode {
    Ok = 0,
    GateBlocked = SEG_BASE + 0x01,  // 0x13E1
    FalseClaim = SEG_BASE + 0x02,   // 0x13E2 失实撤回
    OverdueReview = SEG_BASE + 0x03, // 0x13E3 逾期复议
}

/// 六重硬门。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HardGate { Completeness = 0, Frozen = 1, ZeroRed = 2, GoSign = 3, LongStable = 4, Flow = 5 }

impl HardGate {
    pub const ALL: [HardGate; 6] = [
        HardGate::Completeness, HardGate::Frozen, HardGate::ZeroRed,
        HardGate::GoSign, HardGate::LongStable, HardGate::Flow];
    pub fn name(self) -> &'static str {
        match self {
            HardGate::Completeness => "complete", HardGate::Frozen => "frozen", HardGate::ZeroRed => "zero-red",
            HardGate::GoSign => "go", HardGate::LongStable => "long-stable", HardGate::Flow => "flow",
        }
    }
}

/// 收官宣告册。
pub struct DeclareBook {
    pub items_done: usize,       // 总对账实测完成项
    pub groups_closed: usize,    // 八组收口数（0..8）
    pub freeze_hash_ok: bool,    // 三基线冻结哈希确认
    pub gates: Vec<HardGate>,    // 六重宣告已过门
    pub three90_ok: bool,        // 20 维三 90 确认
    pub conditions_cleared: Vec<&'static str>, // 条件核销
    pub declared: bool,
    pub withdrawn: bool,         // 失实撤回
    pub filed: u32,
}

impl DeclareBook {
    pub fn new() -> Self {
        DeclareBook { items_done: 0, groups_closed: 0, freeze_hash_ok: false, gates: Vec::new(),
            three90_ok: false, conditions_cleared: Vec::new(), declared: false, withdrawn: false, filed: 0 }
    }
    pub fn pass_gate(&mut self, g: HardGate) { if !self.gates.contains(&g) { self.gates.push(g); } }
    pub fn clear_condition(&mut self, c: &'static str) {
        if !self.conditions_cleared.contains(&c) { self.conditions_cleared.push(c); }
    }
    /// 总对账硬门：200/200。
    pub fn ledger_gate(&self) -> bool { self.items_done == DOMAIN_ITEMS }
    /// 八组收口 8/8。
    pub fn groups_gate(&self) -> bool { self.groups_closed == 8 }
    /// 六重宣告核验：六门全过。
    pub fn gates_full(&self) -> bool {
        HardGate::ALL.iter().all(|g| self.gates.contains(g))
    }
    /// 宣告裁决：全硬门→宣告；任一未过→阻断。
    pub fn declare(&mut self) -> Result<(), DcCode> {
        if self.withdrawn { return Err(DcCode::FalseClaim); }
        if !self.ledger_gate() || !self.groups_gate() || !self.freeze_hash_ok
            || !self.gates_full() || !self.three90_ok {
            self.filed += 1;
            return Err(DcCode::GateBlocked);
        }
        self.declared = true;
        Ok(())
    }
    /// 失实→撤回+立案。
    pub fn withdraw(&mut self) {
        self.declared = false;
        self.withdrawn = true;
        self.filed += 1;
    }
    /// 撤回后整改复告：清撤回位。
    pub fn clear_withdrawal(&mut self) { self.withdrawn = false; }
    /// 逾期→复议。
    pub fn overdue_review(&mut self) { self.filed += 1; }
}

// ───────────── 深化块 D440-A · 宣告批账本（批次×门×核销工时） ─────────────

/// 一条门核销入账：批次 + 门 + 核销工时（整数分钟）+ 复核人序号。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GateRec {
    pub batch: u16,
    pub gate: HardGate,
    pub mins: u32,
    pub reviewer: u8,
}

/// 门核销账本：定容量 + 幂等闸（同批同门去重）+ 守恒 + 分位。
pub struct GateLedger {
    pub recs: Vec<GateRec>,
    pub cap: usize,
}

impl GateLedger {
    pub fn new(cap: usize) -> Self { GateLedger { recs: Vec::new(), cap } }
    /// 入账：满→显性 Err；同批同门重复→幂等 false。
    pub fn push(&mut self, r: GateRec) -> Result<bool, DcCode> {
        if self.recs.len() >= self.cap { return Err(DcCode::GateBlocked); }
        if self.recs.iter().any(|x| x.batch == r.batch && x.gate == r.gate) { return Ok(false); }
        self.recs.push(r);
        Ok(true)
    }
    /// 守恒恒等式：∑工时 ≡ ∑(按六门分列求和)。
    pub fn conserves(&self) -> bool {
        let total: u64 = self.recs.iter().map(|r| r.mins as u64).sum();
        let mut by_gate: u64 = 0;
        for g in HardGate::ALL {
            by_gate += self.recs.iter().filter(|r| r.gate == g)
                .map(|r| r.mins as u64).sum::<u64>();
        }
        total == by_gate
    }
    /// 门工时分位（整数排序取点，禁浮点）。
    pub fn gate_mins_quantile(&self, g: HardGate, q: u32) -> u32 {
        let mut vals: Vec<u32> = self.recs.iter().filter(|r| r.gate == g)
            .map(|r| r.mins).collect();
        if vals.is_empty() { return 0; }
        vals.sort_unstable();
        let n = vals.len();
        let idx = ((q as usize).max(1) * n + 99) / 100;
        vals[idx.min(n) - 1]
    }
    /// 门覆盖投影：六门中已核销门数。
    pub fn gate_coverage(&self) -> usize {
        let mut gs: Vec<u8> = self.recs.iter().map(|r| r.gate as u8).collect();
        gs.sort_unstable();
        gs.dedup();
        gs.len()
    }
    /// 复核人分列计数（reviewer 0..=3 桶）。
    pub fn reviewer_counts(&self) -> [usize; 4] {
        let mut out = [0usize; 4];
        for r in &self.recs { if (r.reviewer as usize) < 4 { out[r.reviewer as usize] += 1; } }
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

// ───────────── 深化块 D440-B · 边界探针 ─────────────

/// 探针 P1：越界分位号/空门→显性 Err。
pub fn probe_gate_quantile(lg: &GateLedger, g: HardGate, q: u32) -> Result<u32, DcCode> {
    if q == 0 || q > 100 { return Err(DcCode::FalseClaim); }
    if lg.recs.iter().filter(|r| r.gate == g).count() == 0 { return Err(DcCode::GateBlocked); }
    Ok(lg.gate_mins_quantile(g, q))
}

/// 探针 P2：总对账越界（>200 项属失实）→显性 Err。
pub fn probe_items_range(done: usize) -> Result<(), DcCode> {
    if done > DOMAIN_ITEMS { return Err(DcCode::FalseClaim); }
    Ok(())
}

// ───────────── 深化块 D440-C · LCG 注入回归 ─────────────

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

/// 注入回归：N 轮随机抽走一门（六重缺一），declare 必阻断
/// （GateBlocked 100%）；负对照：六门全过零误报。返回 (轮数, 抓获, 误报)。
pub fn lcg_gate_gap_regression(rounds: usize) -> (usize, usize, usize) {
    let mut g = Lcg(0x40_40_40_40);
    let mut caught = 0usize;
    let mut false_pos = 0usize;
    for _ in 0..rounds {
        let drop = g.pick(6) as usize;
        let mut bk = DeclareBook::new();
        bk.items_done = DOMAIN_ITEMS;
        bk.groups_closed = 8;
        bk.freeze_hash_ok = true;
        bk.three90_ok = true;
        for (i, gt) in HardGate::ALL.iter().enumerate() {
            if i != drop { bk.pass_gate(*gt); }
        }
        if bk.declare() == Err(DcCode::GateBlocked) { caught += 1; } else { false_pos += 1; }
        // 负对照：六门全过零误报。
        let mut bk_ok = DeclareBook::new();
        bk_ok.items_done = DOMAIN_ITEMS;
        bk_ok.groups_closed = 8;
        bk_ok.freeze_hash_ok = true;
        bk_ok.three90_ok = true;
        for gt in HardGate::ALL { bk_ok.pass_gate(gt); }
        if bk_ok.declare().is_err() { false_pos += 1; }
    }
    (rounds, caught, false_pos)
}

// ───────────── 深化块 D440-D · 序列化投影 + FNV 指纹 ─────────────

/// GateRec 定长 8 字节：batch(2,LE)+gate(1)+reviewer(1)+mins(4,LE)。
pub fn encode_gate(r: &GateRec) -> Vec<u8> {
    let mut out = Vec::with_capacity(8);
    out.extend_from_slice(&r.batch.to_le_bytes());
    out.push(r.gate as u8);
    out.push(r.reviewer);
    out.extend_from_slice(&r.mins.to_le_bytes());
    out
}

/// 解码：错长/坏门号→显性 Err。
pub fn decode_gate(b: &[u8]) -> Result<GateRec, DcCode> {
    if b.len() != 8 { return Err(DcCode::GateBlocked); }
    let gate = match HardGate::ALL.iter().find(|g| **g as u8 == b[2]) {
        Some(g) => *g,
        None => return Err(DcCode::FalseClaim),
    };
    let mut batch = [0u8; 2];
    batch.copy_from_slice(&b[0..2]);
    let mut mins = [0u8; 4];
    mins.copy_from_slice(&b[4..8]);
    Ok(GateRec { batch: u16::from_le_bytes(batch), gate, reviewer: b[3], mins: u32::from_le_bytes(mins) })
}

/// FNV-1a 32 位。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for x in bytes { h ^= *x as u32; h = h.wrapping_mul(0x0100_0193); }
    h
}

/// 宣告册快照指纹：对账数+组数+六门位图+宣告/撤回/立案位序列化后 FNV。
pub fn declare_fingerprint(bk: &DeclareBook) -> u32 {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&(bk.items_done as u16).to_le_bytes());
    bytes.push(bk.groups_closed as u8);
    bytes.push(bk.freeze_hash_ok as u8);
    bytes.push(bk.three90_ok as u8);
    for g in HardGate::ALL { bytes.push(if bk.gates.contains(&g) { 1 } else { 0 }); }
    bytes.push(bk.declared as u8);
    bytes.push(bk.withdrawn as u8);
    bytes.extend_from_slice(&(bk.filed as u16).to_le_bytes());
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

pub fn run_ves440_checks() -> CheckSet {
    let mut set = CheckSet::new("ves440");
    let mut bk = DeclareBook::new();

    // C440-01 空册宣告→阻断（总对账未过）。
    chk(&mut set, "C440-01", bk.declare() == Err(DcCode::GateBlocked) && bk.filed == 1, "空册阻断");

    // C440-02 总对账 200/200 过但八组未收口→仍阻断。
    bk.items_done = DOMAIN_ITEMS;
    chk(&mut set, "C440-02", bk.declare() == Err(DcCode::GateBlocked), "八组缺阻断");

    // C440-03 八组收口 8/8 + 冻结哈希确认。
    bk.groups_closed = 8;
    bk.freeze_hash_ok = true;
    chk(&mut set, "C440-03", bk.declare() == Err(DcCode::GateBlocked), "六重缺阻断");

    // C440-04 六重门逐重宣告（逐门推进核验）。
    for g in HardGate::ALL { bk.pass_gate(g); }
    chk(&mut set, "C440-04", bk.declare() == Err(DcCode::GateBlocked), "三 90 缺阻断");

    // C440-05 三 90 确认+条件核销→宣告通过。
    bk.three90_ok = true;
    bk.clear_condition("a11y-terminal");
    chk(&mut set, "C440-05", bk.declare().is_ok() && bk.declared, "宣告通过");

    // C440-06 六重门名单单源（六重全过断言）。
    chk(&mut set, "C440-06", bk.gates_full() && bk.gates.len() == 6, "六重全过");

    // C440-07 失实→撤回+立案。
    bk.withdraw();
    chk(&mut set, "C440-07", !bk.declared && bk.withdrawn && bk.filed == 1, "失实撤回立案");

    // C440-08 撤回态下再宣告→拒（FalseClaim）。
    chk(&mut set, "C440-08", bk.declare() == Err(DcCode::FalseClaim), "撤回态拒宣");

    // C440-09 整改复告复通。
    bk.clear_withdrawal();
    chk(&mut set, "C440-09", bk.declare().is_ok() && bk.declared, "复告复通");

    // C440-10 逾期→复议留痕。
    bk.overdue_review();
    chk(&mut set, "C440-10", bk.filed == 2, "逾期复议");

    // C440-11 六重门名单单源断言。
    chk(&mut set, "C440-11", HardGate::ALL.len() == 6
        && HardGate::ALL[0].name() == "complete"
        && HardGate::ALL[5].name() == "flow", "六重单源");

    // C440-12 家族第十七位登记断言（200 项 + 8 组 + 17 域序）。
    chk(&mut set, "C440-12", DOMAIN_ITEMS == 200 && DOMAIN_SEQ == 17
        && bk.ledger_gate() && bk.groups_gate(), "第十七域宣告");

    // ── 深化块 D440 判据 ──
    // D440-01 门核销账本：两批六门入账 + 守恒恒等式。
    let mut lg = GateLedger::new(16);
    for (i, g) in HardGate::ALL.iter().enumerate() {
        let _ = lg.push(GateRec { batch: 1, gate: *g, mins: 20 + i as u32 * 10, reviewer: (i % 4) as u8 });
    }
    let _ = lg.push(GateRec { batch: 2, gate: HardGate::Flow, mins: 90, reviewer: 1 });
    chk(&mut set, "D440-01", lg.recs.len() == 7 && lg.conserves(), "门核销账本守恒");

    // D440-02 幂等闸：同批同门二次提交零副作用。
    chk(&mut set, "D440-02", lg.push(GateRec { batch: 1, gate: HardGate::Flow, mins: 1, reviewer: 0 }) == Ok(false)
        && lg.recs.len() == 7, "门核销幂等闸");

    // D440-03 门工时分位 + 门覆盖 + 复核人分列。
    chk(&mut set, "D440-03", lg.gate_mins_quantile(HardGate::Flow, 50) == 70
        && lg.gate_mins_quantile(HardGate::Flow, 100) == 90
        && lg.gate_coverage() == 6 && lg.reviewer_counts() == [2, 2, 1, 2], "门分位覆盖投影");

    // D440-04 边界探针：越界分位号/空门→显性 Err。
    chk(&mut set, "D440-04", probe_gate_quantile(&lg, HardGate::Completeness, 0) == Err(DcCode::FalseClaim)
        && probe_gate_quantile(&lg, HardGate::Completeness, 101) == Err(DcCode::FalseClaim), "门分位探针");

    // D440-05 边界探针：总对账越界（>200）属失实。
    chk(&mut set, "D440-05", probe_items_range(DOMAIN_ITEMS + 1) == Err(DcCode::FalseClaim)
        && probe_items_range(DOMAIN_ITEMS).is_ok(), "对账越界探针");

    // D440-06 批次去重投影。
    chk(&mut set, "D440-06", lg.batch_count() == 2, "核销批次去重");

    // D440-07 LCG 门缺注入回归：24 轮 100% 阻断 + 负对照零误报。
    let (n, c, fp) = lcg_gate_gap_regression(24);
    chk(&mut set, "D440-07", n == 24 && c == 24 && fp == 0, "门缺注入全阻");

    // D440-08 序列化往返：门核销记录编码→解码恒等。
    let g0 = GateRec { batch: 9, gate: HardGate::LongStable, mins: 60, reviewer: 3 };
    chk(&mut set, "D440-08", decode_gate(&encode_gate(&g0)) == Ok(g0), "门核销序列化往返");

    // D440-09 解码边界：错长/坏门号→显性 Err。
    chk(&mut set, "D440-09", decode_gate(&[0u8; 7]) == Err(DcCode::GateBlocked)
        && decode_gate(&{ let mut v = encode_gate(&g0); v[2] = 0x0F; v }) == Err(DcCode::FalseClaim), "门核销解码边界");

    // D440-10 宣告册快照指纹：宣告/撤回翻转指纹变 + 确定性 + hex 投影。
    let mut bk2 = DeclareBook::new();
    let f0 = declare_fingerprint(&bk2);
    let f0b = declare_fingerprint(&bk2);
    bk2.pass_gate(HardGate::Flow);
    let f1 = declare_fingerprint(&bk2);
    bk2.withdraw();
    let f2 = declare_fingerprint(&bk2);
    chk(&mut set, "D440-10", f0 == f0b && f0 != f1 && f1 != f2
        && fingerprint_hex(f2).len() == 8, "宣告册指纹");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves440_smoke() {
        let set = run_ves440_checks();
        assert!(set.failed_count() == 0, "ves440 red: {}", set.red_items_text());
    }
}
