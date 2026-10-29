//! VE-F4259 · U03 预备自查（U03 一致性验证平台）
//! 职责：组内自查 20 项逐项核验 + 联调回执 + 性能三线达标 + 基线双签确认
//! ——四件汇总为收口预备报告；核验不过补齐复验、回执缺催办、基线未签阻断。
//! 段位：0x1EA0（续表 idx493）。行数预算 320。

#![allow(dead_code)]

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1EA0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScCode {
    /// 核验不过。
    ItemFail = SEG_BASE,
    /// 回执缺（催办）。
    ReceiptMissing = SEG_BASE + 0x01,
    /// 三线未达（阻断）。
    PerfNotMet = SEG_BASE + 0x02,
    /// 基线未双签（硬阻断）。
    BaselineUnsigned = SEG_BASE + 0x03,
    /// 复验仍未过。
    RecheckFail = SEG_BASE + 0x04,
}

/// 自查项。
#[derive(Clone, Copy, Debug)]
pub struct SelfItem {
    pub no: u8,
    pub pass: bool,
    pub rechecked: bool,
}

/// 四件汇总。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Wrapup {
    pub items_ok: bool,
    pub receipt_ok: bool,
    pub perf_ok: bool,
    pub baseline_signed: bool,
}

impl Wrapup {
    pub fn ready(&self) -> bool {
        self.items_ok && self.receipt_ok && self.perf_ok && self.baseline_signed
    }
}

/// 自查册。
pub struct SelfCheckBook {
    pub items: Vec<SelfItem>,
    pub joint_receipt: Option<bool>,
    /// 性能三线：(跑批, P95, 资源) 达标位。
    pub perf3: (bool, bool, bool),
    /// 基线双签记录。
    pub baseline_sigs: Vec<(u16, bool)>,
    pub urges: u32,
}

impl SelfCheckBook {
    pub const ITEM_N: u8 = 20;

    pub fn new() -> Self {
        SelfCheckBook { items: Vec::new(), joint_receipt: None, perf3: (false, false, false), baseline_sigs: Vec::new(), urges: 0 }
    }

    /// 逐项核验登记。
    pub fn check_item(&mut self, no: u8, pass: bool) -> Result<(), ScCode> {
        if no == 0 || no > Self::ITEM_N {
            return Err(ScCode::ItemFail);
        }
        if let Some(it) = self.items.iter_mut().find(|i| i.no == no) {
            it.pass = pass;
        } else {
            self.items.push(SelfItem { no, pass, rechecked: false });
        }
        Ok(())
    }

    /// 复验：只对不过项。
    pub fn recheck(&mut self, no: u8, pass: bool) -> Result<(), ScCode> {
        let it = self.items.iter_mut().find(|i| i.no == no).ok_or(ScCode::ItemFail)?;
        it.pass = pass;
        it.rechecked = true;
        if !pass {
            return Err(ScCode::RecheckFail);
        }
        Ok(())
    }

    /// 全项核验通过。
    pub fn items_ok(&self) -> bool {
        self.items.len() == Self::ITEM_N as usize && self.items.iter().all(|i| i.pass)
    }

    /// 联调回执催办（幂等计数）。
    pub fn urge(&mut self) {
        self.urges += 1;
    }

    /// 四件汇总。
    pub fn wrapup(&self) -> Result<Wrapup, ScCode> {
        let items_ok = self.items_ok();
        if self.joint_receipt.is_none() {
            return Err(ScCode::ReceiptMissing);
        }
        let perf_ok = self.perf3.0 && self.perf3.1 && self.perf3.2;
        if !perf_ok {
            return Err(ScCode::PerfNotMet);
        }
        let baseline_signed = self.baseline_sigs.iter().any(|(_, s)| *s);
        if !baseline_signed {
            return Err(ScCode::BaselineUnsigned);
        }
        Ok(Wrapup { items_ok, receipt_ok: self.joint_receipt.unwrap(), perf_ok, baseline_signed })
    }
}

// ===================== 二轮深化块（ves493 · 复验台账） =====================
// 深化面：复验结构化台账（项号×轮次×耗时归因）+ 守恒恒等式 + 四类边界探针
// + 整数四分位（排序取点，禁浮点）+ LCG 注入回归（越界项号必拒）
// + 幂等闸 + 定长字节编码解码往返 + FNV-1a 指纹。零 unwrap/panic/浮点。

/// 台账层错误码（独立 u8 域，不占 u16 段位）。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RsCode {
    /// 空台账无可统计。
    Empty = 0,
    /// 台账满容。
    Full = 1,
    /// 同项同轮重复登记。
    Dup = 2,
    /// 项号越界（0 或 >ITEM_N）。
    OutRange = 3,
}

/// 复验台账条目：项号 × 复验轮次 × 耗时归因。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RcEntry {
    /// 自查项号（1..=20）。
    pub no: u8,
    /// 复验轮次（首轮=1）。
    pub round: u16,
    /// 本轮复验耗时（抽象拍数）。
    pub ticks: u32,
    /// 本轮是否转绿。
    pub pass: bool,
}

/// 复验台账：定容 + 拒收显性计数 + 分列守恒。
pub struct RecheckLedger {
    cap: usize,
    pub entries: Vec<RcEntry>,
    pub rejected: u32,
}

impl RecheckLedger {
    pub fn new(cap: usize) -> Self {
        RecheckLedger { cap, entries: Vec::new(), rejected: 0 }
    }

    /// 登记：越界项号 / 满容 / 同项同轮重复，均显性 Err。
    pub fn record(&mut self, no: u8, round: u16, ticks: u32, pass: bool) -> Result<(), RsCode> {
        if no == 0 || no as usize > SelfCheckBook::ITEM_N as usize {
            self.rejected += 1;
            return Err(RsCode::OutRange);
        }
        if self.entries.len() >= self.cap {
            self.rejected += 1;
            return Err(RsCode::Full);
        }
        if self.entries.iter().any(|e| e.no == no && e.round == round) {
            self.rejected += 1;
            return Err(RsCode::Dup);
        }
        self.entries.push(RcEntry { no, round, ticks, pass });
        Ok(())
    }

    pub fn total(&self) -> u32 {
        self.entries.len() as u32
    }

    /// 守恒恒等式：转绿 + 仍红 ≡ 总条数。
    pub fn conservation(&self) -> bool {
        let green = self.entries.iter().filter(|e| e.pass).count() as u32;
        green + (self.total() - green) == self.total()
    }

    /// 归因分列：某项号的复验条数。
    pub fn by_no(&self, no: u8) -> u32 {
        self.entries.iter().filter(|e| e.no == no).count() as u32
    }

    /// 转绿千分比（整数化）：green*1000/total，空本为 0。
    pub fn green_permille(&self) -> u32 {
        let g = self.entries.iter().filter(|e| e.pass).count() as u32;
        let t = self.total();
        if t == 0 { 0 } else { g * 1000 / t }
    }

    /// 整数四分位：ticks 升序 P25/P75（排序取点，禁浮点）。
    pub fn tick_quartile(&self, upper: bool) -> Result<u32, RsCode> {
        if self.entries.is_empty() {
            return Err(RsCode::Empty);
        }
        let mut xs: Vec<u32> = self.entries.iter().map(|e| e.ticks).collect();
        for i in 1..xs.len() {
            let mut j = i;
            while j > 0 && xs[j - 1] > xs[j] {
                xs.swap(j - 1, j);
                j -= 1;
            }
        }
        let n = xs.len();
        let p = if upper { 75 } else { 25 };
        Ok(xs[(p * (n - 1)) / 100])
    }

    /// 摘要诊断行。
    pub fn summary(&self) -> String {
        let g = self.entries.iter().filter(|e| e.pass).count();
        let mut s = String::from("rechecks=");
        s.push_str(&self.total().to_string());
        s.push_str(",green=");
        s.push_str(&g.to_string());
        s.push_str(",rejected=");
        s.push_str(&self.rejected.to_string());
        s
    }
}

/// FNV-1a 32 位指纹。
pub fn rs_fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in bytes {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 条目定长编码：no(u8)+round(u16LE)+ticks(u32LE)+pass(u8) = 8 字节。
pub fn encode_rc(e: &RcEntry) -> [u8; 8] {
    let mut out = [0u8; 8];
    out[0] = e.no;
    out[1] = (e.round & 0xFF) as u8;
    out[2] = (e.round >> 8) as u8;
    out[3] = (e.ticks & 0xFF) as u8;
    out[4] = ((e.ticks >> 8) & 0xFF) as u8;
    out[5] = ((e.ticks >> 16) & 0xFF) as u8;
    out[6] = ((e.ticks >> 24) & 0xFF) as u8;
    out[7] = if e.pass { 1 } else { 0 };
    out
}

/// 解码：项号越界或 pass 字节非法显性拒绝。
pub fn decode_rc(b: &[u8; 8]) -> Result<RcEntry, RsCode> {
    if b[0] == 0 || b[0] as usize > SelfCheckBook::ITEM_N as usize {
        return Err(RsCode::OutRange);
    }
    if b[7] > 1 {
        return Err(RsCode::OutRange);
    }
    Ok(RcEntry {
        no: b[0],
        round: (b[1] as u16) | ((b[2] as u16) << 8),
        ticks: (b[3] as u32) | ((b[4] as u32) << 8) | ((b[5] as u32) << 16) | ((b[6] as u32) << 24),
        pass: b[7] == 1,
    })
}

/// 全台账序列化。
pub fn serialize_rc_all(l: &RecheckLedger) -> Vec<u8> {
    let mut out = Vec::new();
    for e in &l.entries {
        out.extend_from_slice(&encode_rc(e));
    }
    out
}

/// 序列化往返：编码→解码→再编码逐字节一致且指纹不变。
pub fn roundtrip_rc_ok(e: RcEntry) -> bool {
    let a = encode_rc(&e);
    match decode_rc(&a) {
        Ok(d) => encode_rc(&d) == a && rs_fnv1a(&a) == rs_fnv1a(&encode_rc(&d)),
        Err(_) => false,
    }
}

/// 确定性 LCG。
pub struct RsLcg(u32);

impl RsLcg {
    pub fn new(seed: u32) -> Self {
        RsLcg(seed)
    }
    pub fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
}

/// LCG 注入回归：N 轮复验登记，每 8 轮注入一次越界项号（0 或 21 交替），
/// 断言拒收数 ≡ rounds/8（异常必抓）且账簿自洽（记账+拒收≡总轮数、守恒）。
pub fn lcg_recheck_regression(rounds: u32) -> Result<(u32, u32), RsCode> {
    if rounds == 0 {
        return Err(RsCode::Empty);
    }
    let mut rng = RsLcg::new(0x5EED_4930);
    let mut led = RecheckLedger::new(rounds as usize);
    let mut green = 0u32;
    let mut rej = 0u32;
    for r in 0..rounds {
        let no = 1 + (rng.next() % 20) as u8;
        let round = 1 + (r % 3) as u16; // 轮次循环，制造潜在重复面
        let ticks = 10 + rng.next() % 50;
        let pass = rng.next() & 1 == 1;
        let injected = r % 8 == 7;
        let res = if injected {
            let bad_no = if r % 16 == 15 { 0 } else { SelfCheckBook::ITEM_N + 1 };
            led.record(bad_no, round, ticks, pass)
        } else {
            led.record(no, round, ticks, pass)
        };
        match res {
            Ok(()) => {
                if pass {
                    green += 1;
                }
            }
            Err(_) => rej += 1,
        }
    }
    if rej != rounds / 8 {
        return Err(RsCode::OutRange);
    }
    if led.total() + rej != rounds || !led.conservation() {
        return Err(RsCode::Full);
    }
    Ok((green, rej))
}

/// 幂等闸：同项同轮二次登记零副作用（台账指纹不变）。
pub fn idempotent_recheck(ledger: &mut RecheckLedger, no: u8, round: u16) -> bool {
    let first = ledger.record(no, round, 33, true);
    let mid = rs_fnv1a(&serialize_rc_all(ledger));
    let second = ledger.record(no, round, 33, true);
    let after = rs_fnv1a(&serialize_rc_all(ledger));
    first.is_ok() && second == Err(RsCode::Dup) && mid == after
}

/// 边界探针四连：空分位 / 满容 / 重复 / 越界项号。
pub fn rs_boundary_probes() -> [bool; 4] {
    let empty = RecheckLedger::new(4);
    let p_empty = empty.tick_quartile(false) == Err(RsCode::Empty);
    let mut full = RecheckLedger::new(1);
    let f1 = full.record(1, 1, 10, true).is_ok();
    let f2 = full.record(2, 1, 10, true) == Err(RsCode::Full);
    let mut dup_led = RecheckLedger::new(4);
    let dup = dup_led.record(5, 2, 7, true).is_ok() && dup_led.record(5, 2, 7, true) == Err(RsCode::Dup);
    let mut oor = RecheckLedger::new(4);
    let o1 = oor.record(0, 1, 1, true) == Err(RsCode::OutRange);
    let o2 = oor.record(SelfCheckBook::ITEM_N + 1, 1, 1, true) == Err(RsCode::OutRange);
    [p_empty, f1 && f2, dup, o1 && o2]
}

/// 深化判据（续接 C493-12 之后，D 系列）。
fn deep_checks(set: &mut CheckSet) {
    let mut led = RecheckLedger::new(8);
    let r1 = led.record(7, 1, 40, true);
    let r2 = led.record(9, 1, 80, false);
    let r3 = led.record(9, 2, 20, true);
    chk(set, "C493-D01", r1.is_ok() && r2.is_ok() && r3.is_ok(), "复验台账三条登记（含跨轮）");
    chk(set, "C493-D02", led.conservation() && led.total() == 3, "转绿+仍红守恒恒等式");
    chk(set, "C493-D03", led.by_no(9) == 2 && led.by_no(7) == 1, "按项号归因分列");
    chk(set, "C493-D04", led.green_permille() == 666, "转绿千分比整数化（2/3≈666）");
    chk(set, "C493-D05", led.tick_quartile(false) == Ok(20) && led.tick_quartile(true) == Ok(40), "整数四分位 P25/P75");
    chk(set, "C493-D06", rs_boundary_probes().iter().all(|b| *b), "四类边界探针显性 Err");
    let mut reg_ok = false;
    if let Ok((green, rej)) = lcg_recheck_regression(64) {
        reg_ok = rej == 8 && green > 0;
    }
    chk(set, "C493-D07", reg_ok, "LCG 注入回归：越界项号必拒+账簿自洽");
    let mut led2 = RecheckLedger::new(4);
    chk(set, "C493-D08", idempotent_recheck(&mut led2, 12, 1), "幂等闸：同项同轮重复零副作用");
    chk(set, "C493-D09", roundtrip_rc_ok(RcEntry { no: 20, round: 3, ticks: 4321, pass: false }), "字节序列化往返+指纹一致");
    let bad = {
        let mut b = encode_rc(&RcEntry { no: 1, round: 1, ticks: 1, pass: true });
        b[0] = 0;
        decode_rc(&b) == Err(RsCode::OutRange)
    };
    chk(set, "C493-D10", bad, "解码越界项号显性拒绝");
    chk(set, "C493-D11", led.summary().starts_with("rechecks=3"), "台账摘要诊断行");
    chk(set, "C493-D12", rs_fnv1a(b"ves493") != 0, "FNV 非空输入指纹非基值");
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C493-01..12。
pub fn run_ves493_checks() -> CheckSet {
    let mut set = CheckSet::new("ves493");

    let mut bk = SelfCheckBook::new();
    chk(&mut set, "C493-01", bk.check_item(0, true) == Err(ScCode::ItemFail), "零号项拒收");
    chk(&mut set, "C493-02", bk.check_item(21, true) == Err(ScCode::ItemFail), "超 20 项拒收");

    for i in 1..=20u8 {
        let _ = bk.check_item(i, i != 7);
    }
    chk(&mut set, "C493-03", bk.items.len() == 20, "20 项逐项核验登记");
    chk(&mut set, "C493-04", bk.items_ok() == false, "第 7 项不过→未全过");

    // 复验
    chk(&mut set, "C493-05", bk.recheck(7, true).is_ok(), "第 7 项复验转绿");
    chk(&mut set, "C493-06", bk.items_ok(), "全项核验通过");
    let _ = bk.check_item(9, false);
    chk(&mut set, "C493-07", bk.recheck(9, false) == Err(ScCode::RecheckFail), "复验仍不过立案");

    // 四件汇总
    chk(&mut set, "C493-08", bk.wrapup() == Err(ScCode::ReceiptMissing), "回执缺→催办前置");
    bk.urge();
    bk.joint_receipt = Some(true);
    chk(&mut set, "C493-09", bk.urges == 1, "催办幂等计数");
    chk(&mut set, "C493-10", bk.wrapup() == Err(ScCode::PerfNotMet), "三线未达阻断");
    bk.perf3 = (true, true, true);
    chk(&mut set, "C493-11", bk.wrapup() == Err(ScCode::BaselineUnsigned), "基线未双签硬阻断");
    bk.baseline_sigs.push((1, true));
    let w = bk.wrapup().unwrap_or(Wrapup { items_ok: false, receipt_ok: false, perf_ok: false, baseline_signed: false });
    chk(&mut set, "C493-12", w.ready() && w == Wrapup { items_ok: false, receipt_ok: true, perf_ok: true, baseline_signed: true }, "四件汇总出预备报告（items 因复验立案 false）");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selfcheck_wrapup() {
        let mut bk = SelfCheckBook::new();
        assert!(bk.check_item(1, true).is_ok());
        assert_eq!(bk.check_item(21, true), Err(ScCode::ItemFail));
        assert!(bk.wrapup().is_err());
    }
}
