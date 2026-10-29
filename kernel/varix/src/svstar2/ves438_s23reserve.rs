//! ves438_s23reserve — VE-F4198 · T 域演进预留（域级）
//! 预留：T 域级演进预留——F4178 组级四口之上域级增三口（新书写系统协议口/新合规法域口/新辅助语言技术口）
//! +预留纪律复述（不预写）；预写越界→回退；口被占→迁移评审；登记缺→补登；
//! 三口均含无障碍向说明（域本色）；预留不引入新隐私面。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1380;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RvCode {
    Ok = 0,
    Prewrite = SEG_BASE + 0x01,    // 0x1381 预写越界
    SlotOccupied = SEG_BASE + 0x02, // 0x1382 口被占
    RegisterMissing = SEG_BASE + 0x03, // 0x1383 登记缺
}

/// 域级三口。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DomainSlot { NewScript = 0, NewLegalRegion = 1, NewAssistTech = 2 }

impl DomainSlot {
    pub const ALL: [DomainSlot; 3] = [DomainSlot::NewScript, DomainSlot::NewLegalRegion, DomainSlot::NewAssistTech];
    pub fn name(self) -> &'static str {
        match self {
            DomainSlot::NewScript => "new-script-protocol", DomainSlot::NewLegalRegion => "new-legal-region",
            DomainSlot::NewAssistTech => "new-assist-tech",
        }
    }
}

/// 预留口（接口位 + 断言位 + 无障碍向说明——域本色 + 预写检测）。
pub struct ReservedSlot {
    pub slot: DomainSlot,
    pub iface_ready: bool,   // 接口位就绪
    pub assert_ready: bool,  // 断言位就绪
    pub a11y_note: bool,     // 无障碍向说明
    pub prewritten: bool,    // 预写实现（越界检测）
    pub occupied: bool,      // 口被占
    pub registered: bool,    // 登记在册
}

/// 域级预留册。
pub struct ReserveBook { pub slots: Vec<ReservedSlot>, pub migrations: u32, pub rollbacks: u32 }

impl ReserveBook {
    pub fn new() -> Self { ReserveBook { slots: Vec::new(), migrations: 0, rollbacks: 0 } }
    pub fn put(&mut self, s: DomainSlot, iface: bool, assert: bool, a11y: bool, pre: bool) {
        self.slots.retain(|x| x.slot != s);
        self.slots.push(ReservedSlot { slot: s, iface_ready: iface, assert_ready: assert,
            a11y_note: a11y, prewritten: pre, occupied: false, registered: true });
    }
    /// 预留纪律核验：三口齐+登记+不预写+断言位齐+无障碍说明。
    pub fn audit(&self) -> Result<(), RvCode> {
        for s in DomainSlot::ALL {
            let x = match self.slots.iter().find(|x| x.slot == s) {
                Some(x) => x, None => return Err(RvCode::RegisterMissing),
            };
            if !x.registered { return Err(RvCode::RegisterMissing); }
            if x.prewritten { return Err(RvCode::Prewrite); }
            if !x.assert_ready { return Err(RvCode::RegisterMissing); }
            if !x.a11y_note { return Err(RvCode::RegisterMissing); } // 无障碍向说明缺→补登
        }
        Ok(())
    }
    /// 预写越界→回退（清预写标记）。
    pub fn rollback_prewrite(&mut self, s: DomainSlot) -> bool {
        for x in self.slots.iter_mut() {
            if x.slot == s && x.prewritten { x.prewritten = false; self.rollbacks += 1; return true; }
        }
        false
    }
    /// 口被占→迁移评审。
    pub fn occupy(&mut self, s: DomainSlot) -> Result<(), RvCode> {
        for x in self.slots.iter_mut() {
            if x.slot == s {
                if x.occupied {
                    self.migrations += 1;
                    return Err(RvCode::SlotOccupied);
                }
                x.occupied = true;
                return Ok(());
            }
        }
        Err(RvCode::RegisterMissing)
    }
    /// 三口名单含无障碍技术口（域本色位）。
    pub fn has_assist_tech_slot(&self) -> bool {
        self.slots.iter().any(|x| x.slot == DomainSlot::NewAssistTech)
    }
}

// ───────────── 深化块 D438-A · 预留评审批账本（批次×口×评审结论） ─────────────

/// 一条预留评审入账：批次 + 口 + 评审结论（1=过 0=缓）+ 评审工时。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReserveRec {
    pub batch: u16,
    pub slot: DomainSlot,
    pub pass: bool,
    pub mins: u32,
}

/// 预留评审账本：定容量 + 幂等闸（同批同口去重）+ 守恒 + 分位。
pub struct ReserveLedger {
    pub recs: Vec<ReserveRec>,
    pub cap: usize,
}

impl ReserveLedger {
    pub fn new(cap: usize) -> Self { ReserveLedger { recs: Vec::new(), cap } }
    /// 入账：满→显性 Err；同批同口重复→幂等 false。
    pub fn push(&mut self, r: ReserveRec) -> Result<bool, RvCode> {
        if self.recs.len() >= self.cap { return Err(RvCode::RegisterMissing); }
        if self.recs.iter().any(|x| x.batch == r.batch && x.slot == r.slot) { return Ok(false); }
        self.recs.push(r);
        Ok(true)
    }
    /// 守恒恒等式：∑工时 ≡ ∑(按过/缓两桶分列求和)。
    pub fn conserves(&self) -> bool {
        let total: u64 = self.recs.iter().map(|r| r.mins as u64).sum();
        let pass: u64 = self.recs.iter().filter(|r| r.pass).map(|r| r.mins as u64).sum();
        let hold: u64 = self.recs.iter().filter(|r| !r.pass).map(|r| r.mins as u64).sum();
        total == pass + hold
    }
    /// 口工时分位（整数排序取点，禁浮点）。
    pub fn slot_mins_quantile(&self, s: DomainSlot, q: u32) -> u32 {
        let mut vals: Vec<u32> = self.recs.iter().filter(|r| r.slot == s)
            .map(|r| r.mins).collect();
        if vals.is_empty() { return 0; }
        vals.sort_unstable();
        let n = vals.len();
        let idx = ((q as usize).max(1) * n + 99) / 100;
        vals[idx.min(n) - 1]
    }
    /// 口覆盖投影：三口中已评审口数。
    pub fn slot_coverage(&self) -> usize {
        let mut ss: Vec<u8> = self.recs.iter().map(|r| r.slot as u8).collect();
        ss.sort_unstable();
        ss.dedup();
        ss.len()
    }
    /// 过缓比投影（整数千分比：过审占比）。
    pub fn pass_rate_pm(&self) -> u32 {
        if self.recs.is_empty() { return 0; }
        let p = self.recs.iter().filter(|r| r.pass).count();
        (p * 1000 / self.recs.len()) as u32
    }
    /// 批次去重计数。
    pub fn batch_count(&self) -> usize {
        let mut bs: Vec<u16> = self.recs.iter().map(|r| r.batch).collect();
        bs.sort_unstable();
        bs.dedup();
        bs.len()
    }
}

// ───────────── 深化块 D438-B · 边界探针 ─────────────

/// 探针 P1：越界分位号/空口→显性 Err。
pub fn probe_slot_quantile(lg: &ReserveLedger, s: DomainSlot, q: u32) -> Result<u32, RvCode> {
    if q == 0 || q > 100 { return Err(RvCode::Prewrite); }
    if lg.recs.iter().filter(|r| r.slot == s).count() == 0 { return Err(RvCode::RegisterMissing); }
    Ok(lg.slot_mins_quantile(s, q))
}

/// 探针 P2：口未登记却占用→显性 Err（补登前置）。
pub fn probe_occupy_unregistered(slot_in_book: bool) -> Result<(), RvCode> {
    if !slot_in_book { return Err(RvCode::RegisterMissing); }
    Ok(())
}

// ───────────── 深化块 D438-C · LCG 注入回归 ─────────────

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

/// 注入回归：N 轮随机预写注入，audit 必抓（Prewrite 100%）；
/// 负对照：无预写注入零误报。返回 (轮数, 抓获, 误报)。
pub fn lcg_prewrite_regression(rounds: usize) -> (usize, usize, usize) {
    let mut g = Lcg(0x38_38_38_38);
    let mut caught = 0usize;
    let mut false_pos = 0usize;
    for _ in 0..rounds {
        let s = DomainSlot::ALL[g.pick(3) as usize];
        let mut bk = ReserveBook::new();
        for x in DomainSlot::ALL {
            bk.put(x, true, true, true, x == s); // 只有 s 口预写
        }
        if bk.audit() == Err(RvCode::Prewrite) { caught += 1; } else { false_pos += 1; }
        // 负对照：全不预写零误报。
        let mut bk_ok = ReserveBook::new();
        for x in DomainSlot::ALL { bk_ok.put(x, true, true, true, false); }
        if bk_ok.audit().is_err() { false_pos += 1; }
    }
    (rounds, caught, false_pos)
}

// ───────────── 深化块 D438-D · 序列化投影 + FNV 指纹 ─────────────

/// ReserveRec 定长 8 字节：batch(2,LE)+slot(1)+pass(1)+mins(4,LE)。
pub fn encode_reserve(r: &ReserveRec) -> Vec<u8> {
    let mut out = Vec::with_capacity(8);
    out.extend_from_slice(&r.batch.to_le_bytes());
    out.push(r.slot as u8);
    out.push(if r.pass { 1 } else { 0 });
    out.extend_from_slice(&r.mins.to_le_bytes());
    out
}

/// 解码：错长/坏口号→显性 Err。
pub fn decode_reserve(b: &[u8]) -> Result<ReserveRec, RvCode> {
    if b.len() != 8 { return Err(RvCode::RegisterMissing); }
    let slot = match DomainSlot::ALL.iter().find(|s| **s as u8 == b[2]) {
        Some(s) => *s,
        None => return Err(RvCode::Prewrite),
    };
    let mut batch = [0u8; 2];
    batch.copy_from_slice(&b[0..2]);
    let mut mins = [0u8; 4];
    mins.copy_from_slice(&b[4..8]);
    Ok(ReserveRec { batch: u16::from_le_bytes(batch), slot, pass: b[3] == 1, mins: u32::from_le_bytes(mins) })
}

/// FNV-1a 32 位。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for x in bytes { h ^= *x as u32; h = h.wrapping_mul(0x0100_0193); }
    h
}

/// 预留册快照指纹：三口七位状态序列化后 FNV。
pub fn reserve_fingerprint(bk: &ReserveBook) -> u32 {
    let mut bytes = Vec::new();
    for s in DomainSlot::ALL {
        match bk.slots.iter().find(|x| x.slot == s) {
            Some(x) => {
                bytes.push(x.iface_ready as u8);
                bytes.push(x.assert_ready as u8);
                bytes.push(x.a11y_note as u8);
                bytes.push(x.prewritten as u8);
                bytes.push(x.occupied as u8);
                bytes.push(x.registered as u8);
            }
            None => bytes.extend_from_slice(&[0xCCu8; 6]),
        }
    }
    bytes.extend_from_slice(&(bk.migrations as u16).to_le_bytes());
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

pub fn run_ves438_checks() -> CheckSet {
    let mut set = CheckSet::new("ves438");
    let mut bk = ReserveBook::new();

    // C438-01 三口未登记→补登码。
    chk(&mut set, "C438-01", bk.audit() == Err(RvCode::RegisterMissing), "未登记补登");

    // C438-02 登记但预写→越界码。
    for s in DomainSlot::ALL { bk.put(s, true, true, true, true); }
    chk(&mut set, "C438-02", bk.audit() == Err(RvCode::Prewrite), "预写越界");

    // C438-03 预写回退→纪律复绿。
    for s in DomainSlot::ALL { bk.rollback_prewrite(s); }
    chk(&mut set, "C438-03", bk.audit().is_ok() && bk.rollbacks == 3, "回退复绿");

    // C438-04 无障碍向说明缺→补登拒（域本色）。
    bk.put(DomainSlot::NewScript, true, true, false, false);
    chk(&mut set, "C438-04", bk.audit() == Err(RvCode::RegisterMissing), "a11y 说明缺拒");

    // C438-05 补说明后复绿。
    bk.put(DomainSlot::NewScript, true, true, true, false);
    chk(&mut set, "C438-05", bk.audit().is_ok(), "补说明复绿");

    // C438-06 首次占用放行。
    chk(&mut set, "C438-06", bk.occupy(DomainSlot::NewScript).is_ok(), "首次占用");

    // C438-07 口被占→迁移评审。
    chk(&mut set, "C438-07", bk.occupy(DomainSlot::NewScript) == Err(RvCode::SlotOccupied)
        && bk.migrations == 1, "口被占迁移");

    // C438-08 断言位缺→拒。
    bk.put(DomainSlot::NewLegalRegion, true, false, true, false);
    chk(&mut set, "C438-08", bk.audit() == Err(RvCode::RegisterMissing), "断言位缺拒");

    // C438-09 三口名单单源（书写系统/法域/辅助技术）。
    chk(&mut set, "C438-09", DomainSlot::ALL.len() == 3
        && DomainSlot::ALL[0].name() == "new-script-protocol"
        && DomainSlot::ALL[1].name() == "new-legal-region"
        && DomainSlot::ALL[2].name() == "new-assist-tech", "三口单源");

    // C438-10 无障碍技术口在册（域本色位）。
    chk(&mut set, "C438-10", bk.has_assist_tech_slot(), "辅助技术口在册");

    // C438-11 无新隐私面断言（预留册只含预留字段）。
    chk(&mut set, "C438-11", bk.slots.len() == 3
        && bk.slots.iter().all(|x| x.registered), "登记全在册");

    // C438-12 终态：全绿（回退后全部合规）。
    bk.put(DomainSlot::NewLegalRegion, true, true, true, false);
    chk(&mut set, "C438-12", bk.audit().is_ok(), "终态全绿");

    // ── 深化块 D438 判据 ──
    // D438-01 预留评审账本：两批三口入账 + 守恒恒等式（过/缓两桶）。
    let mut lg = ReserveLedger::new(16);
    let _ = lg.push(ReserveRec { batch: 1, slot: DomainSlot::NewScript, pass: true, mins: 45 });
    let _ = lg.push(ReserveRec { batch: 1, slot: DomainSlot::NewLegalRegion, pass: false, mins: 20 });
    let _ = lg.push(ReserveRec { batch: 2, slot: DomainSlot::NewAssistTech, pass: true, mins: 35 });
    chk(&mut set, "D438-01", lg.recs.len() == 3 && lg.conserves(), "预留账本守恒");

    // D438-02 幂等闸：同批同口二次提交零副作用。
    chk(&mut set, "D438-02", lg.push(ReserveRec { batch: 1, slot: DomainSlot::NewScript, pass: false, mins: 1 }) == Ok(false)
        && lg.recs.len() == 3, "预留幂等闸");

    // D438-03 口工时分位 + 口覆盖 + 过缓比。
    chk(&mut set, "D438-03", lg.slot_mins_quantile(DomainSlot::NewScript, 50) == 45
        && lg.slot_coverage() == 3 && lg.pass_rate_pm() == 666, "口分位覆盖投影");

    // D438-04 边界探针：越界分位号/空口→显性 Err。
    chk(&mut set, "D438-04", probe_slot_quantile(&lg, DomainSlot::NewLegalRegion, 0) == Err(RvCode::Prewrite)
        && probe_slot_quantile(&lg, DomainSlot::NewLegalRegion, 101) == Err(RvCode::Prewrite), "预留分位探针");

    // D438-05 边界探针：未登记口占用→补登前置拒绝。
    chk(&mut set, "D438-05", probe_occupy_unregistered(false) == Err(RvCode::RegisterMissing)
        && probe_occupy_unregistered(true).is_ok(), "未登记占用探针");

    // D438-06 批次去重投影。
    chk(&mut set, "D438-06", lg.batch_count() == 2, "预留批次去重");

    // D438-07 LCG 预写注入回归：18 轮 100% 抓 Prewrite + 零误报。
    let (n, c, fp) = lcg_prewrite_regression(18);
    chk(&mut set, "D438-07", n == 18 && c == 18 && fp == 0, "预写注入全抓");

    // D438-08 序列化往返：评审记录编码→解码恒等。
    let r0 = ReserveRec { batch: 3, slot: DomainSlot::NewAssistTech, pass: true, mins: 35 };
    chk(&mut set, "D438-08", decode_reserve(&encode_reserve(&r0)) == Ok(r0), "预留序列化往返");

    // D438-09 解码边界：错长/坏口号→显性 Err。
    chk(&mut set, "D438-09", decode_reserve(&[0u8; 7]) == Err(RvCode::RegisterMissing)
        && decode_reserve(&{ let mut v = encode_reserve(&r0); v[2] = 0x0B; v }) == Err(RvCode::Prewrite), "预留解码边界");

    // D438-10 预留册快照指纹：占用前后指纹变 + 确定性 + hex 投影。
    let mut bk2 = ReserveBook::new();
    for x in DomainSlot::ALL { bk2.put(x, true, true, true, false); }
    let f0 = reserve_fingerprint(&bk2);
    let f0b = reserve_fingerprint(&bk2);
    let _ = bk2.occupy(DomainSlot::NewScript);
    let f1 = reserve_fingerprint(&bk2);
    chk(&mut set, "D438-10", f0 == f0b && f0 != f1
        && fingerprint_hex(f1).len() == 8, "预留册指纹");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves438_smoke() {
        let set = run_ves438_checks();
        assert!(set.failed_count() == 0, "ves438 red: {}", set.red_items_text());
    }
}
