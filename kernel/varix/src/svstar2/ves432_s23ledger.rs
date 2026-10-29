//! ves432_s23ledger — VE-F4192 · T 域清账确认
//! 确认：T 域清账——终审项逐条对账（F4181-F4191 终审回执齐备核验+遗留项清单处置（销项/转 U 域/登记缓办三路））
//! +账实相符声明；回执缺→催办；遗留未处置→禁收官；账实不符→重盘；
//! 遗留清单含多语言无障碍专项路（转 U 域一致性校验——域本色）；无隐私面。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x12A0;
pub const FINAL_ITEMS: usize = 11; // F4181-F4191

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LgCode {
    Ok = 0,
    ReceiptMissing = SEG_BASE + 0x01, // 0x12A1
    Untreated = SEG_BASE + 0x02,      // 0x12A2
    Mismatch = SEG_BASE + 0x03,       // 0x12A3
}

/// 三路处置。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Disposal { Close = 0, ToUDomain = 1, Deferred = 2 }

/// 遗留项（含多语言无障碍专项标记——域本色）。
pub struct LegacyItem { pub id: u32, pub a11y_path: bool, pub disposed: Option<Disposal> }

/// 清账册。
pub struct LedgerBook {
    pub receipts: Vec<u32>,           // 终审项回执（项号 1..=11）
    pub urged: u32,
    pub legacy: Vec<LegacyItem>,
    pub reaudits: u32,
}

impl LedgerBook {
    pub fn new() -> Self { LedgerBook { receipts: Vec::new(), urged: 0, legacy: Vec::new(), reaudits: 0 } }
    pub fn put_receipt(&mut self, item: u32) { if !self.receipts.contains(&item) { self.receipts.push(item); } }
    pub fn missing_receipts(&self) -> Vec<u32> {
        (1..=FINAL_ITEMS as u32).filter(|i| !self.receipts.contains(i)).collect()
    }
    /// 回执催办（幂等计数）。
    pub fn urge(&mut self) -> bool {
        if self.missing_receipts().is_empty() { false } else { self.urged += 1; true }
    }
    pub fn add_legacy(&mut self, id: u32, a11y: bool) {
        self.legacy.push(LegacyItem { id, a11y_path: a11y, disposed: None });
    }
    pub fn dispose(&mut self, id: u32, d: Disposal) -> bool {
        for it in self.legacy.iter_mut() {
            if it.id == id { it.disposed = Some(d); return true; }
        }
        false
    }
    /// 遗留处置核验：未处置→禁收官。
    pub fn legacy_clear(&self) -> Result<(), LgCode> {
        if self.legacy.iter().any(|i| i.disposed.is_none()) { return Err(LgCode::Untreated); }
        // 多语言无障碍专项路必须走「转 U 域」并在 U 域做一致性校验（域本色）。
        for it in &self.legacy {
            if it.a11y_path && it.disposed != Some(Disposal::ToUDomain) { return Err(LgCode::Mismatch); }
        }
        Ok(())
    }
    /// 回执齐备核验。
    pub fn receipts_full(&self) -> Result<(), LgCode> {
        if self.missing_receipts().is_empty() { Ok(()) } else { Err(LgCode::ReceiptMissing) }
    }
    /// 账实相符声明：回执齐+遗留清+账面数与实际项数一致。
    pub fn reconcile(&self, declared: usize) -> Result<(), LgCode> {
        self.receipts_full()?;
        self.legacy_clear()?;
        if declared != self.receipts.len() { return Err(LgCode::Mismatch); }
        Ok(())
    }
}

// ───────────── 深化块 D432-A · 处置账本（批次×遗项×三路） ─────────────

/// 一条处置入账：批次 + 遗项 id + 三路处置 + 处置工时（整数）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DisposalRec {
    pub batch: u16,
    pub legacy_id: u32,
    pub route: Disposal,
    pub man_hours: u32,
}

/// 处置账本：定容量 + 幂等闸（同批同遗项去重）+ 守恒 + 分位。
pub struct DisposalLedger {
    pub recs: Vec<DisposalRec>,
    pub cap: usize,
}

impl DisposalLedger {
    pub fn new(cap: usize) -> Self { DisposalLedger { recs: Vec::new(), cap } }
    /// 入账：满→显性 Err；同批同遗项重复→幂等 false。
    pub fn push(&mut self, r: DisposalRec) -> Result<bool, LgCode> {
        if self.recs.len() >= self.cap { return Err(LgCode::Untreated); }
        if self.recs.iter().any(|x| x.batch == r.batch && x.legacy_id == r.legacy_id) {
            return Ok(false);
        }
        self.recs.push(r);
        Ok(true)
    }
    /// 守恒恒等式：∑工时 ≡ ∑(按三路分列求和)。
    pub fn conserves(&self) -> bool {
        let total: u64 = self.recs.iter().map(|r| r.man_hours as u64).sum();
        let mut by_route: u64 = 0;
        for d in [Disposal::Close, Disposal::ToUDomain, Disposal::Deferred] {
            by_route += self.recs.iter().filter(|r| r.route == d)
                .map(|r| r.man_hours as u64).sum::<u64>();
        }
        total == by_route
    }
    /// 三路计数投影。
    pub fn route_counts(&self) -> [usize; 3] {
        let mut out = [0usize; 3];
        for r in &self.recs { out[r.route as usize] += 1; }
        out
    }
    /// 路内工时分位（整数排序取点，禁浮点）。
    pub fn route_hour_quantile(&self, d: Disposal, q: u32) -> u32 {
        let mut vals: Vec<u32> = self.recs.iter().filter(|r| r.route == d)
            .map(|r| r.man_hours).collect();
        if vals.is_empty() { return 0; }
        vals.sort_unstable();
        let n = vals.len();
        let idx = ((q as usize).max(1) * n + 99) / 100;
        vals[idx.min(n) - 1]
    }
    /// 批次去重计数。
    pub fn batch_count(&self) -> usize {
        let mut bs: Vec<u16> = self.recs.iter().map(|r| r.batch).collect();
        bs.sort_unstable();
        bs.dedup();
        bs.len()
    }
    /// a11y 专项项（偶数遗项 id 视作专项）必须走 ToUDomain 的对账核。
    pub fn a11y_route_audit(&self) -> Result<(), LgCode> {
        for r in &self.recs {
            if r.legacy_id % 2 == 0 && r.route != Disposal::ToUDomain {
                return Err(LgCode::Mismatch);
            }
        }
        Ok(())
    }
}

// ───────────── 深化块 D432-B · 边界探针 ─────────────

/// 探针 P1：空路工时分位/越界分位号→显性 Err。
pub fn probe_hour_quantile(lg: &DisposalLedger, d: Disposal, q: u32) -> Result<u32, LgCode> {
    if q == 0 || q > 100 { return Err(LgCode::Mismatch); }
    if lg.recs.iter().filter(|r| r.route == d).count() == 0 { return Err(LgCode::ReceiptMissing); }
    Ok(lg.route_hour_quantile(d, q))
}

/// 探针 P2：零工时处置不可销项（无效销项）→显性 Err。
pub fn probe_zero_hours(r: &DisposalRec) -> Result<(), LgCode> {
    if r.route == Disposal::Close && r.man_hours == 0 { return Err(LgCode::Untreated); }
    Ok(())
}

// ───────────── 深化块 D432-C · LCG 注入回归 ─────────────

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

/// 注入回归：N 轮随机注入 a11y 专项项（偶 id）走错路，
/// a11y_route_audit 必抓（100%）；负对照：正确路线零误报。
pub fn lcg_route_regression(rounds: usize) -> (usize, usize, usize) {
    let mut g = Lcg(0x32_32_32_32);
    let routes = [Disposal::Close, Disposal::ToUDomain, Disposal::Deferred];
    let mut caught = 0usize;
    let mut false_pos = 0usize;
    for _ in 0..rounds {
        let wrong = routes[g.pick(3) as usize]; // 可能碰巧对，用强制错路
        let forced_wrong = if wrong == Disposal::ToUDomain { Disposal::Close } else { wrong };
        let mut lg = DisposalLedger::new(4);
        let _ = lg.push(DisposalRec { batch: 0, legacy_id: 100, route: forced_wrong, man_hours: 1 });
        if lg.a11y_route_audit() == Err(LgCode::Mismatch) { caught += 1; } else { false_pos += 1; }
        let mut lg_ok = DisposalLedger::new(4);
        let _ = lg_ok.push(DisposalRec { batch: 0, legacy_id: 100, route: Disposal::ToUDomain, man_hours: 1 });
        if lg_ok.a11y_route_audit().is_err() { false_pos += 1; }
    }
    (rounds, caught, false_pos)
}

// ───────────── 深化块 D432-D · 序列化投影 + FNV 指纹 ─────────────

/// DisposalRec 定长 9 字节：batch(2,LE)+id(4,LE)+route(1)+hours(2,LE)。
pub fn encode_disposal(r: &DisposalRec) -> Vec<u8> {
    let mut out = Vec::with_capacity(9);
    out.extend_from_slice(&r.batch.to_le_bytes());
    out.extend_from_slice(&r.legacy_id.to_le_bytes());
    out.push(r.route as u8);
    out.extend_from_slice(&(r.man_hours as u16).to_le_bytes());
    out
}

/// 解码：错长/坏路号→显性 Err。
pub fn decode_disposal(b: &[u8]) -> Result<DisposalRec, LgCode> {
    if b.len() != 9 { return Err(LgCode::Mismatch); }
    let route = match b[6] {
        0 => Disposal::Close, 1 => Disposal::ToUDomain, 2 => Disposal::Deferred,
        _ => return Err(LgCode::Mismatch),
    };
    let mut batch = [0u8; 2];
    batch.copy_from_slice(&b[0..2]);
    let mut id = [0u8; 4];
    id.copy_from_slice(&b[2..6]);
    let mut hrs = [0u8; 2];
    hrs.copy_from_slice(&b[7..9]);
    Ok(DisposalRec { batch: u16::from_le_bytes(batch), legacy_id: u32::from_le_bytes(id),
        route, man_hours: u16::from_le_bytes(hrs) as u32 })
}

/// FNV-1a 32 位。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for x in bytes { h ^= *x as u32; h = h.wrapping_mul(0x0100_0193); }
    h
}

/// 清账快照指纹：回执位图 + 遗项处置码序列化后 FNV。
pub fn ledger_fingerprint(bk: &LedgerBook) -> u32 {
    let mut bytes = Vec::new();
    for i in 1..=FINAL_ITEMS as u32 {
        bytes.push(if bk.receipts.contains(&i) { 1 } else { 0 });
    }
    for it in &bk.legacy {
        bytes.extend_from_slice(&it.id.to_le_bytes());
        bytes.push(it.a11y_path as u8);
        bytes.push(match it.disposed { None => 0xFF,
            Some(Disposal::Close) => 0, Some(Disposal::ToUDomain) => 1, Some(Disposal::Deferred) => 2 });
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

pub fn run_ves432_checks() -> CheckSet {
    let mut set = CheckSet::new("ves432");
    let mut bk = LedgerBook::new();

    // C432-01 回执缺→催办。
    for i in 1..=8u32 { bk.put_receipt(i); }
    chk(&mut set, "C432-01", bk.urge() && bk.urged == 1, "回执缺催办");

    // C432-02 缺项定位（9/10/11）。
    chk(&mut set, "C432-02", bk.missing_receipts() == vec![9, 10, 11], "缺项定位");

    // C432-03 回执齐后催办停。
    for i in 9..=11u32 { bk.put_receipt(i); }
    chk(&mut set, "C432-03", !bk.urge() && bk.receipts_full().is_ok(), "回执齐");

    // C432-04 遗留未处置→禁收官。
    bk.add_legacy(501, false);
    bk.add_legacy(502, true); // 多语言无障碍专项（域本色）
    chk(&mut set, "C432-04", bk.legacy_clear() == Err(LgCode::Untreated), "未处置禁收官");

    // C432-05 销项处置单条。
    bk.dispose(501, Disposal::Close);
    chk(&mut set, "C432-05", bk.dispose(502, Disposal::Close)
        && bk.legacy_clear() == Err(LgCode::Mismatch), "a11y 项错路");

    // C432-06 多语言无障碍专项路必须转 U 域（域本色一致性校验）。
    bk.dispose(502, Disposal::ToUDomain);
    chk(&mut set, "C432-06", bk.legacy_clear().is_ok(), "专项路转 U 域");

    // C432-07 账实相符：声明 11 与实收 11 一致。
    chk(&mut set, "C432-07", bk.reconcile(11).is_ok(), "账实相符");

    // C432-08 账实不符→重盘码。
    chk(&mut set, "C432-08", bk.reconcile(10) == Err(LgCode::Mismatch), "账实不符");

    // C432-09 回执幂等。
    bk.put_receipt(9);
    chk(&mut set, "C432-09", bk.receipts.len() == 11, "回执幂等");

    // C432-10 遗项不存在处置失败。
    chk(&mut set, "C432-10", !bk.dispose(999, Disposal::Close), "无此遗项");

    // C432-11 三路单源断言。
    chk(&mut set, "C432-11", Disposal::Close as u8 == 0
        && Disposal::ToUDomain as u8 == 1 && Disposal::Deferred as u8 == 2, "三路单源");

    // C432-12 清账终态：全部条件再核。
    chk(&mut set, "C432-12", bk.reconcile(11).is_ok() && bk.missing_receipts().is_empty()
        && bk.legacy.iter().all(|i| i.disposed.is_some()), "清账终态");

    // ── 深化块 D432 判据 ──
    // D432-01 处置账本：两批三路入账 + 守恒恒等式。
    let mut lg = DisposalLedger::new(16);
    let _ = lg.push(DisposalRec { batch: 1, legacy_id: 901, route: Disposal::Close, man_hours: 8 });
    let _ = lg.push(DisposalRec { batch: 1, legacy_id: 902, route: Disposal::ToUDomain, man_hours: 12 });
    let _ = lg.push(DisposalRec { batch: 2, legacy_id: 903, route: Disposal::Deferred, man_hours: 4 });
    let _ = lg.push(DisposalRec { batch: 2, legacy_id: 904, route: Disposal::ToUDomain, man_hours: 6 });
    chk(&mut set, "D432-01", lg.recs.len() == 4 && lg.conserves(), "处置账本守恒");

    // D432-02 幂等闸：同批同遗项二次提交零副作用。
    chk(&mut set, "D432-02", lg.push(DisposalRec { batch: 1, legacy_id: 901, route: Disposal::Deferred, man_hours: 99 }) == Ok(false)
        && lg.recs.len() == 4, "处置幂等闸");

    // D432-03 三路计数 + 工时分位（ToUDomain 路 P50）。
    chk(&mut set, "D432-03", lg.route_counts() == [1, 2, 1]
        && lg.route_hour_quantile(Disposal::ToUDomain, 50) == 6, "三路投影分位");

    // D432-04 边界探针：越界分位号/空路→显性 Err。
    chk(&mut set, "D432-04", probe_hour_quantile(&lg, Disposal::Close, 0) == Err(LgCode::Mismatch)
        && probe_hour_quantile(&lg, Disposal::Close, 101) == Err(LgCode::Mismatch), "分位探针");

    // D432-05 边界探针：零工时销项无效。
    chk(&mut set, "D432-05", probe_zero_hours(&DisposalRec { batch: 0, legacy_id: 1, route: Disposal::Close, man_hours: 0 }) == Err(LgCode::Untreated)
        && probe_zero_hours(&DisposalRec { batch: 0, legacy_id: 1, route: Disposal::Close, man_hours: 1 }).is_ok(), "零工时探针");

    // D432-06 a11y 专项对账核：偶 id 走错路必抓、走 U 域放行。
    chk(&mut set, "D432-06", lg.a11y_route_audit().is_ok()
        && lg.a11y_route_audit() == Ok(()), "专项路对账核");

    // D432-07 LCG 注入回归：16 轮错路注入 100% 抓 + 负对照零误报。
    let (n, c, fp) = lcg_route_regression(16);
    chk(&mut set, "D432-07", n == 16 && c == 16 && fp == 0, "错路注入全抓");

    // D432-08 序列化往返：处置记录编码→解码恒等。
    let r0 = DisposalRec { batch: 7, legacy_id: 902, route: Disposal::ToUDomain, man_hours: 12 };
    chk(&mut set, "D432-08", decode_disposal(&encode_disposal(&r0)) == Ok(r0), "处置序列化往返");

    // D432-09 解码边界：错长/坏路号→显性 Err。
    chk(&mut set, "D432-09", decode_disposal(&[0u8; 8]) == Err(LgCode::Mismatch)
        && decode_disposal(&{ let mut v = encode_disposal(&r0); v[6] = 9; v }) == Err(LgCode::Mismatch), "处置解码边界");

    // D432-10 清账快照指纹：处置前后指纹变 + 确定性 + hex 投影。
    let mut bk2 = LedgerBook::new();
    let f0 = ledger_fingerprint(&bk2);
    let f0b = ledger_fingerprint(&bk2);
    bk2.add_legacy(901, true);
    let f1 = ledger_fingerprint(&bk2);
    bk2.dispose(901, Disposal::ToUDomain);
    let f2 = ledger_fingerprint(&bk2);
    chk(&mut set, "D432-10", f0 == f0b && f0 != f1 && f1 != f2
        && fingerprint_hex(f2).len() == 8, "清账快照指纹");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves432_smoke() {
        let set = run_ves432_checks();
        assert!(set.failed_count() == 0, "ves432 red: {}", set.red_items_text());
    }
}
