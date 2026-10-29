//! VE-F4258 · U03 联调（U03 一致性验证平台）
//! 职责：20 条目贯通演练（静态→动态→视觉→交互→性能→无障碍六验证全链跑通）
//! + 故障注入三演练（基准投毒/闸门死锁/回放失真）+ 证据挂 F4248 报告平台。
//! 段位：0x1E60（续表 idx492）。行数预算 320。

#![allow(dead_code)]

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1E60;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JtCode {
    /// 链断。
    ChainBreak = SEG_BASE,
    /// 演练失败（预案未验证）。
    DrillFail = SEG_BASE + 0x01,
    /// 证据缺。
    EvidenceLost = SEG_BASE + 0x02,
}

/// 六验证全链站序。
pub const CHAIN: [&str; 6] = ["static", "dynamic", "visual", "interact", "perf", "a11y"];

/// 故障注入类别。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fault {
    BaselinePoison = 0,
    GateDeadlock = 1,
    ReplayDrift = 2,
}
pub const FAULT_N: usize = 3;

/// 联调册。
pub struct JointBook {
    /// 贯通进度（已通站数）。
    pub passed: usize,
    /// 各站回执。
    pub receipts: Vec<(&'static str, bool)>,
    /// 演练结果：(故障, 预案生效)。
    pub drills: Vec<(Fault, bool)>,
    /// 证据链：(链站, 证据号)。
    pub evidence: Vec<(&'static str, u32)>,
    pub reruns: u32,
}

impl JointBook {
    pub fn new() -> Self {
        JointBook { passed: 0, receipts: Vec::new(), drills: Vec::new(), evidence: Vec::new(), reruns: 0 }
    }

    /// 站贯通：顺序推进（静态先行，a11y 永不跳过——抽演在末站强制）。
    pub fn pass_station(&mut self, name: &'static str) -> Result<(), JtCode> {
        let idx = CHAIN.iter().position(|c| *c == name).ok_or(JtCode::ChainBreak)?;
        if idx != self.passed {
            return Err(JtCode::ChainBreak);
        }
        self.passed += 1;
        self.receipts.push((name, true));
        Ok(())
    }

    /// 无障碍抽演：末站强制核验。
    pub fn a11y_rehearsed(&self) -> bool {
        self.passed == CHAIN.len() && self.receipts.iter().any(|(n, _)| *n == "a11y")
    }

    /// 故障演练登记（预案生效才算过）。
    pub fn drill(&mut self, f: Fault, plan_ok: bool) {
        if !self.drills.iter().any(|(a, _)| *a == f) {
            self.drills.push((f, plan_ok));
        }
    }

    /// 三演练齐备且全过。
    pub fn drills_ok(&self) -> bool {
        (0..FAULT_N).all(|f| self.drills.iter().any(|(a, ok)| *a as usize == f && *ok))
    }

    /// 证据挂报告平台（幂等）。
    pub fn attach(&mut self, station: &'static str, ev: u32) {
        if !self.evidence.iter().any(|(s, e)| *s == station && *e == ev) {
            self.evidence.push((station, ev));
        }
    }

    /// 证据缺站补跑。
    pub fn missing_evidence(&self) -> Vec<&'static str> {
        let passed: Vec<&'static str> = CHAIN.iter().take(self.passed).copied().collect();
        passed.into_iter().filter(|s| !self.evidence.iter().any(|(es, _)| es == s)).collect()
    }

    /// 重跑登记。
    pub fn rerun(&mut self) {
        self.reruns += 1;
    }
}

// ===================== 二轮深化块（ves492 · 联调演练账本） =====================
// 深化面：演练结构化账本（批次×故障×轮次归因）+ 守恒恒等式 + 四类边界探针
// + 整数分位（排序取点，禁浮点）+ LCG 注入回归 + 幂等闸 + 字节序列化往返
// + FNV-1a 指纹。全 no_std/alloc，Err + 中文诊断，零 unwrap/panic/浮点。

/// 深化层错误码（独立 u8 域，不占 u16 段位空间）。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DvCode {
    /// 空账本无可统计。
    Empty = 0,
    /// 账本满容。
    Full = 1,
    /// 重复登记。
    Dup = 2,
    /// 越界（故障号/分位参数非法）。
    OutRange = 3,
}

/// 演练结构化条目：批次 × 故障 × 轮次归因。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DrillEntry {
    /// 批次（一次联调为一批，批内批号唯一）。
    pub batch: u16,
    /// 故障类别（须 < FAULT_N）。
    pub fault: u8,
    /// 本轮演练耗时（抽象拍数，供分位统计）。
    pub ticks: u32,
    /// 预案生效（true=故障被捕获）。
    pub caught: bool,
}

/// 演练账本（深化版）：定容 + 拒收计数 + 分列守恒。
pub struct DrillLedger {
    cap: usize,
    pub entries: Vec<DrillEntry>,
    /// 被显性拒绝的登记数（不静默）。
    pub rejected: u32,
}

impl DrillLedger {
    pub fn new(cap: usize) -> Self {
        DrillLedger { cap, entries: Vec::new(), rejected: 0 }
    }

    /// 登记：越界故障号 / 满容 / 同批同故障重复，均显性 Err。
    pub fn record(&mut self, batch: u16, fault: u8, ticks: u32, caught: bool) -> Result<(), DvCode> {
        if fault as usize >= FAULT_N {
            self.rejected += 1;
            return Err(DvCode::OutRange);
        }
        if self.entries.len() >= self.cap {
            self.rejected += 1;
            return Err(DvCode::Full);
        }
        if self.entries.iter().any(|e| e.batch == batch && e.fault == fault) {
            self.rejected += 1;
            return Err(DvCode::Dup);
        }
        self.entries.push(DrillEntry { batch, fault, ticks, caught });
        Ok(())
    }

    /// 总条目数。
    pub fn total(&self) -> u32 {
        self.entries.len() as u32
    }

    /// 守恒恒等式：按故障分列计数之和 ≡ 总条目数。
    pub fn conservation(&self) -> bool {
        let mut per = [0u32; FAULT_N];
        for e in &self.entries {
            per[e.fault as usize] += 1;
        }
        per.iter().fold(0u32, |a, b| a + b) == self.total()
    }

    /// 归因分列：某故障类别的演练条数。
    pub fn by_fault(&self, fault: u8) -> u32 {
        self.entries.iter().filter(|e| e.fault == fault).count() as u32
    }

    /// 捕获千分比（整数化，禁浮点）：captured*1000/total，空本返回 0。
    pub fn catch_permille(&self) -> u32 {
        let c = self.entries.iter().filter(|e| e.caught).count() as u32;
        let t = self.total();
        if t == 0 { 0 } else { c * 1000 / t }
    }

    /// 整数分位：ticks 升序第 p 百分位（p∈[0,100]，排序取点）。
    pub fn tick_percentile(&self, p: u32) -> Result<u32, DvCode> {
        if self.entries.is_empty() {
            return Err(DvCode::Empty);
        }
        if p > 100 {
            return Err(DvCode::OutRange);
        }
        let mut xs: Vec<u32> = self.entries.iter().map(|e| e.ticks).collect();
        // 插入排序（小样本 no_std 友好）。
        for i in 1..xs.len() {
            let mut j = i;
            while j > 0 && xs[j - 1] > xs[j] {
                xs.swap(j - 1, j);
                j -= 1;
            }
        }
        let n = xs.len();
        Ok(xs[(p as usize * (n - 1)) / 100])
    }

    /// 摘要诊断行（ToString 组装）。
    pub fn summary(&self) -> String {
        let c = self.entries.iter().filter(|e| e.caught).count();
        let mut s = String::from("drills=");
        s.push_str(&self.total().to_string());
        s.push_str(",caught=");
        s.push_str(&c.to_string());
        s.push_str(",rejected=");
        s.push_str(&self.rejected.to_string());
        s
    }
}

/// FNV-1a 32 位指纹（序列化校验与幂等闸对拍用）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in bytes {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 条目定长编码：batch(u16LE)+fault(u8)+ticks(u32LE)+caught(u8) = 8 字节。
pub fn encode_drill(e: &DrillEntry) -> [u8; 8] {
    let mut out = [0u8; 8];
    out[0] = (e.batch & 0xFF) as u8;
    out[1] = (e.batch >> 8) as u8;
    out[2] = e.fault;
    out[3] = (e.ticks & 0xFF) as u8;
    out[4] = ((e.ticks >> 8) & 0xFF) as u8;
    out[5] = ((e.ticks >> 16) & 0xFF) as u8;
    out[6] = ((e.ticks >> 24) & 0xFF) as u8;
    out[7] = if e.caught { 1 } else { 0 };
    out
}

/// 解码：fault 越界或 caught 字节非法均显性拒绝。
pub fn decode_drill(b: &[u8; 8]) -> Result<DrillEntry, DvCode> {
    if b[2] as usize >= FAULT_N {
        return Err(DvCode::OutRange);
    }
    if b[7] > 1 {
        return Err(DvCode::OutRange);
    }
    Ok(DrillEntry {
        batch: (b[0] as u16) | ((b[1] as u16) << 8),
        fault: b[2],
        ticks: (b[3] as u32) | ((b[4] as u32) << 8) | ((b[5] as u32) << 16) | ((b[6] as u32) << 24),
        caught: b[7] == 1,
    })
}

/// 全账本序列化（逐条编码拼接）。
pub fn serialize_all(l: &DrillLedger) -> Vec<u8> {
    let mut out = Vec::new();
    for e in &l.entries {
        out.extend_from_slice(&encode_drill(e));
    }
    out
}

/// 序列化往返：编码→解码→再编码逐字节一致，指纹不变。
pub fn roundtrip_ok(e: DrillEntry) -> bool {
    let a = encode_drill(&e);
    match decode_drill(&a) {
        Ok(d) => encode_drill(&d) == a && fnv1a(&a) == fnv1a(&encode_drill(&d)),
        Err(_) => false,
    }
}

/// 确定性 LCG（注入回归驱动，8 行核心）。
pub struct DvLcg(u32);

impl DvLcg {
    pub fn new(seed: u32) -> Self {
        DvLcg(seed)
    }
    pub fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
}

/// LCG 注入回归：N 轮确定性注入演练入账本，每 8 轮混入一次越界故障号
/// （异常必抓：拒收数≡rounds/8），断言账簿自洽（记账+拒收≡总轮数、
/// 分列守恒、未捕获条数与捕获数对拍吻合）。
pub fn lcg_drill_regression(rounds: u32) -> Result<(u32, u32), DvCode> {
    if rounds == 0 {
        return Err(DvCode::Empty);
    }
    let mut rng = DvLcg::new(0x5EED_4920);
    let mut led = DrillLedger::new(rounds as usize);
    let mut caught = 0u32;
    let mut rej = 0u32;
    for r in 0..rounds {
        let plan_ok = rng.next() & 1 == 1;
        let ticks = 20 + rng.next() % 80;
        let mut fault = (rng.next() % FAULT_N as u32) as u8;
        if r % 8 == 7 {
            fault = FAULT_N as u8 + 9; // 注入越界故障号
        }
        match led.record(r as u16, fault, ticks, plan_ok) {
            Ok(()) => {
                if plan_ok {
                    caught += 1;
                }
            }
            Err(_) => rej += 1,
        }
    }
    if rej != rounds / 8 {
        return Err(DvCode::OutRange);
    }
    let uncaught = led.entries.iter().filter(|e| !e.caught).count() as u32;
    if led.total() + rej != rounds || !led.conservation() || uncaught != led.total() - caught {
        return Err(DvCode::Full);
    }
    Ok((caught, rej))
}

/// 幂等闸：同一 (batch, fault) 二次登记零副作用（账面字节指纹不变）。
pub fn idempotent_reregister(ledger: &mut DrillLedger, batch: u16, fault: u8) -> bool {
    let first = ledger.record(batch, fault, 42, true);
    let mid = fnv1a(&serialize_all(ledger));
    let second = ledger.record(batch, fault, 42, true);
    let after = fnv1a(&serialize_all(ledger));
    first.is_ok() && second == Err(DvCode::Dup) && mid == after
}

/// 边界探针四连：空分位 / 满容 / 重复 / 越界，各自显性 Err。
pub fn boundary_probes() -> [bool; 4] {
    let empty = DrillLedger::new(4);
    let p_empty = empty.tick_percentile(50) == Err(DvCode::Empty);
    let mut full = DrillLedger::new(1);
    let f1 = full.record(1, 0, 10, true).is_ok();
    let f2 = full.record(2, 1, 10, true) == Err(DvCode::Full);
    full.entries.clear();
    let dup = full.record(1, 0, 5, true).is_ok() && full.record(1, 0, 5, true) == Err(DvCode::Dup);
    let mut oor_led = DrillLedger::new(4);
    let oor = oor_led.record(9, FAULT_N as u8, 1, true) == Err(DvCode::OutRange);
    [p_empty, f1 && f2, dup, oor]
}

/// 深化判据（续接 C492-12 之后，D 系列）。
fn deep_checks(set: &mut CheckSet) {
    let mut led = DrillLedger::new(8);
    let r1 = led.record(1, 0, 30, true);
    let r2 = led.record(1, 1, 60, true);
    let r3 = led.record(1, 2, 90, false);
    chk(set, "C492-D01", r1.is_ok() && r2.is_ok() && r3.is_ok(), "演练账本三条登记");
    chk(set, "C492-D02", led.conservation(), "分列之和守恒恒等式");
    chk(set, "C492-D03", led.by_fault(0) == 1 && led.total() == 3, "按故障归因分列");
    chk(set, "C492-D04", led.catch_permille() == 666, "捕获千分比整数化（2/3≈666）");
    chk(set, "C492-D05", led.tick_percentile(0) == Ok(30) && led.tick_percentile(50) == Ok(60) && led.tick_percentile(100) == Ok(90), "整数分位三点位（P0/P50/P100）");
    chk(set, "C492-D06", boundary_probes().iter().all(|b| *b), "四类边界探针显性 Err");
    let mut reg_ok = false;
    if let Ok((caught, rej)) = lcg_drill_regression(64) {
        reg_ok = rej == 8 && caught > 0;
    }
    chk(set, "C492-D07", reg_ok, "LCG 注入回归：越界必拒+账簿自洽");
    let mut led2 = DrillLedger::new(4);
    chk(set, "C492-D08", idempotent_reregister(&mut led2, 7, 2), "幂等闸：重复登记零副作用");
    let e = DrillEntry { batch: 3, fault: 1, ticks: 1234, caught: true };
    chk(set, "C492-D09", roundtrip_ok(e), "字节序列化往返+指纹一致");
    let bad = {
        let mut b = encode_drill(&e);
        b[2] = 0xFF;
        decode_drill(&b) == Err(DvCode::OutRange)
    };
    chk(set, "C492-D10", bad, "解码越界故障号显性拒绝");
    chk(set, "C492-D11", led.summary().starts_with("drills=3"), "账本摘要诊断行");
    chk(set, "C492-D12", fnv1a(&[]) == 0x811C_9DC5, "FNV 空输入偏移基值指纹");
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C492-01..12。
pub fn run_ves492_checks() -> CheckSet {
    let mut set = CheckSet::new("ves492");

    let mut jb = JointBook::new();
    chk(&mut set, "C492-01", jb.pass_station("dynamic") == Err(JtCode::ChainBreak), "越站推进拦截（静态先行）");
    chk(&mut set, "C492-02", jb.pass_station("rogue") == Err(JtCode::ChainBreak), "未知站拒收");

    for s in CHAIN.iter() {
        let _ = jb.pass_station(s);
    }
    chk(&mut set, "C492-03", jb.passed == 6, "六验证全链贯通");
    chk(&mut set, "C492-04", jb.a11y_rehearsed(), "无障碍抽演在册（域本色）");

    // 三演练
    jb.drill(Fault::BaselinePoison, true);
    jb.drill(Fault::GateDeadlock, false);
    chk(&mut set, "C492-05", jb.drills_ok() == false, "演练未全过=联调未收");
    jb.drill(Fault::GateDeadlock, true);
    jb.drill(Fault::ReplayDrift, true);
    chk(&mut set, "C492-06", jb.drills.len() == 3 && jb.drills_ok(), "三演练齐备全过（预案验证）");
    jb.drill(Fault::GateDeadlock, false);
    chk(&mut set, "C492-07", jb.drills.iter().filter(|(f, _)| *f == Fault::GateDeadlock).count() == 1, "演练幂等登记");

    // 证据链
    jb.attach("static", 1);
    jb.attach("a11y", 6);
    jb.attach("static", 1);
    chk(&mut set, "C492-08", jb.evidence.len() == 2, "证据挂证幂等");
    let miss = jb.missing_evidence();
    chk(&mut set, "C492-09", miss.len() == 4 && miss.contains(&"visual"), "缺证四站显性列出");
    jb.rerun();
    chk(&mut set, "C492-10", jb.reruns == 1, "缺证重跑登记");

    // 补齐证据
    for s in ["dynamic", "visual", "interact", "perf"] {
        jb.attach(s, 99);
    }
    chk(&mut set, "C492-11", jb.missing_evidence().is_empty(), "补齐后零缺证");
    chk(&mut set, "C492-12", jb.receipts.len() == 6 && jb.evidence.len() == 6, "回执与证据一一对应");

    deep_checks(&mut set);

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joint_chain() {
        let mut jb = JointBook::new();
        assert_eq!(jb.pass_station("a11y"), Err(JtCode::ChainBreak));
        for s in CHAIN {
            assert!(jb.pass_station(s).is_ok());
        }
        assert!(jb.a11y_rehearsed());
        assert!(jb.missing_evidence().len() == 6);
    }
}
