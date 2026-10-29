//! ves400_s21close — VE-F4160 · T08 组收口双签
//! 四件齐备双签：20 项自查 + 走查汇总回执 + fuzz 总闸三零线 + 资产汇总齐备；
//! 自查签与复核签分离禁同人；四件缺一→拒签；同人→作废；阻断未消→硬性拒签；
//! 双签后 T 域八组全收口（T01-T08 8/8）触发域收官预备；签收含无障碍工具族齐备确认项（域本色）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x00A0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClCode {
    Ok = 0,
    ItemMissing = SEG_BASE + 0x01,   // 0x00A1
    SamePerson = SEG_BASE + 0x02,    // 0x00A2
    BlockOpen = SEG_BASE + 0x03,     // 0x00A3
    A11yToolShort = SEG_BASE + 0x04, // 0x00A4
    Invalidated = SEG_BASE + 0x05,   // 0x00A5
    LedgerBroken = SEG_BASE + 0x06,  // 0x00A6 进度账本守恒破（二轮深化）
    ProbeReject = SEG_BASE + 0x07,   // 0x00A7 边界探针拒绝（二轮深化）
    RoundtripDrift = SEG_BASE + 0x08, // 0x00A8 收口态投影漂移（二轮深化）
}

/// 收口四件。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CloseItem { SelfCheck = 0, WalkReceipt = 1, FuzzZeroLine = 2, AssetComplete = 3 }

impl CloseItem {
    pub const ALL: [CloseItem; 4] = [
        CloseItem::SelfCheck, CloseItem::WalkReceipt, CloseItem::FuzzZeroLine, CloseItem::AssetComplete];
    pub fn name(self) -> &'static str {
        match self {
            CloseItem::SelfCheck => "self-check", CloseItem::WalkReceipt => "walk-receipt",
            CloseItem::FuzzZeroLine => "fuzz-zero-line", CloseItem::AssetComplete => "asset-complete",
        }
    }
}

/// 收口清单：四件 + 走查阻断状态 + a11y 工具族确认。
pub struct CloseChecklist {
    pub items: Vec<CloseItem>,
    pub blockers_open: bool,      // 走查阻断未消
    pub a11y_tools_ready: bool,   // 无障碍工具族齐备确认（域本色）
}

impl CloseChecklist {
    pub fn new() -> Self { CloseChecklist { items: Vec::new(), blockers_open: false, a11y_tools_ready: false } }

    pub fn put(&mut self, it: CloseItem) { if !self.items.contains(&it) { self.items.push(it); } }

    pub fn missing(&self) -> Vec<&'static str> {
        CloseItem::ALL.iter().filter(|c| !self.items.contains(&**c)).map(|c| c.name()).collect()
    }
    pub fn four_complete(&self) -> bool { self.missing().is_empty() }
}

/// 双签记录。
pub struct DualSign {
    pub signer_a: Option<&'static str>,
    pub signer_b: Option<&'static str>,
    pub valid: bool,
    pub void_reason: Option<ClCode>,
}

impl DualSign {
    pub fn new() -> Self { DualSign { signer_a: None, signer_b: None, valid: false, void_reason: None } }
    pub fn sign_a(&mut self, who: &'static str) { self.signer_a = Some(who); }
    pub fn sign_b(&mut self, who: &'static str) -> Result<(), ClCode> {
        if let Some(a) = self.signer_a {
            if a == who {
                self.valid = false;
                self.void_reason = Some(ClCode::SamePerson);
                return Err(ClCode::SamePerson);
            }
        }
        self.signer_b = Some(who);
        self.valid = true;
        Ok(())
    }
    pub fn void(&mut self, why: ClCode) {
        self.valid = false;
        self.void_reason = Some(why);
        self.signer_a = None;
        self.signer_b = None;
    }
}

/// 域收官预备：T01-T08 八组收口计数（8/8 触发预备）。
pub struct DomainWrapup { pub groups_closed: u8 }

impl DomainWrapup {
    pub const TOTAL: u8 = 8;
    pub fn ready(&self) -> bool { self.groups_closed >= Self::TOTAL }
    pub fn progress(&self) -> u8 { self.groups_closed.min(Self::TOTAL) }
}

/// 收口裁决。
pub struct CloseVerdict { pub passed: bool, pub rejected_by: Option<ClCode> }

pub fn try_close(list: &CloseChecklist, sign: &DualSign) -> CloseVerdict {
    let rejected_by = if !list.four_complete() {
        Some(ClCode::ItemMissing)
    } else if list.blockers_open {
        Some(ClCode::BlockOpen)
    } else if !sign.valid {
        Some(sign.void_reason.unwrap_or(ClCode::SamePerson))
    } else if !list.a11y_tools_ready {
        Some(ClCode::A11yToolShort)
    } else {
        None
    };
    CloseVerdict { passed: rejected_by.is_none(), rejected_by }
}

// ============ 二轮深化块：收口进度账本 / 整数分位 / 序列化指纹 / 边界探针 ============

/// [深化] 签署事件结构化账本：按件记签入 + 守恒恒等式（Σ件 ≡ 总数）。
#[derive(Clone, Copy)]
pub struct SignLedger {
    pub per_item: [u32; 4],
    pub total: u32,
}

impl SignLedger {
    pub fn new() -> Self { SignLedger { per_item: [0; 4], total: 0 } }
    pub fn record(&mut self, it: CloseItem) {
        self.per_item[it as usize] += 1;
        self.total += 1;
    }
    pub fn conserved(&self) -> bool { self.per_item.iter().sum::<u32>() == self.total }
    /// 已触及件数（去重）。
    pub fn signed_items(&self) -> u32 {
        self.per_item.iter().filter(|&&c| c > 0).count() as u32
    }
}

/// [深化] 收口进度账本：八组逐组计完成自查项（每组上限 20）。
pub struct CloseProgress { pub items_done: [u16; 8] }

impl CloseProgress {
    pub const PER_GROUP: u16 = 20;

    pub fn new() -> Self { CloseProgress { items_done: [0; 8] } }

    /// 单组推进一项；越界组号 / 满容量 → ProbeReject。
    pub fn bump(&mut self, g: usize) -> Result<(), ClCode> {
        if g >= 8 { return Err(ClCode::ProbeReject); }
        if self.items_done[g] < Self::PER_GROUP {
            self.items_done[g] += 1;
            Ok(())
        } else {
            Err(ClCode::ProbeReject)
        }
    }

    pub fn total(&self) -> u32 {
        let mut t = 0u32;
        for x in self.items_done.iter() { t += *x as u32; }
        t
    }

    /// 守恒：总数 ≤ 160（8 组 × 20）。
    pub fn conserved(&self) -> bool { self.total() <= 160 }

    /// 组完成度。
    pub fn group_done(&self, g: usize) -> bool {
        g < 8 && self.items_done[g] == Self::PER_GROUP
    }

    /// 整数分位（排序取点，禁浮点）：q ∈ [0,100]。
    pub fn quantile(&self, q: u32) -> Result<u16, ClCode> {
        if q > 100 { return Err(ClCode::ProbeReject); }
        let mut vals: Vec<u16> = Vec::new();
        for x in self.items_done.iter() { vals.push(*x); }
        for i in 1..vals.len() {
            let mut j = i;
            while j > 0 && vals[j - 1] > vals[j] { vals.swap(j - 1, j); j -= 1; }
        }
        Ok(vals[(q as usize * 7 + 50) / 100])
    }
}

/// [深化] 收口态定长投影（6B，禁 JSON）：四件位图 u8 + 阻断 u8 + 工具族 u8 + 签位图 u8 + 保留 u16le。
pub fn close_bytes(list: &CloseChecklist, sign: &DualSign, out: &mut Vec<u8>) {
    out.clear();
    let mut item_mask = 0u8;
    for it in CloseItem::ALL.iter() {
        if list.items.contains(it) { item_mask |= 1 << (*it as u8); }
    }
    out.push(item_mask);
    out.push(if list.blockers_open { 1 } else { 0 });
    out.push(if list.a11y_tools_ready { 1 } else { 0 });
    out.push(match (sign.signer_a.is_some(), sign.signer_b.is_some(), sign.valid) {
        (true, true, true) => 3,
        (true, false, false) => 1,
        (false, false, false) => 0,
        _ => 2,
    });
    out.extend_from_slice(&0u16.to_le_bytes()); // 保留槽
}

/// [深化] FNV-1a 64 位指纹。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes.iter() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// [深化] 指纹幂等闸：同态二次投影一致。
pub fn close_fingerprint_stable(list: &CloseChecklist, sign: &DualSign) -> Result<u64, ClCode> {
    let mut b1: Vec<u8> = Vec::new();
    close_bytes(list, sign, &mut b1);
    let fp1 = fnv1a64(&b1);
    let mut b2: Vec<u8> = Vec::new();
    close_bytes(list, sign, &mut b2);
    let fp2 = fnv1a64(&b2);
    if b1.len() != b2.len() || fp1 != fp2 { Err(ClCode::RoundtripDrift) } else { Ok(fp1) }
}

/// [深化] 收口边界探针。
pub struct CloseProbe;

impl CloseProbe {
    /// 空清单 → ItemMissing。
    pub fn probe_empty(list: &CloseChecklist) -> Result<(), ClCode> {
        if list.items.is_empty() { Err(ClCode::ItemMissing) } else { Ok(()) }
    }
    /// 越界序号取件 → ProbeReject（不 panic）。
    pub fn probe_index(list: &CloseChecklist, idx: usize) -> Result<&'static str, ClCode> {
        match list.items.get(idx) {
            Some(it) => Ok(it.name()),
            None => Err(ClCode::ProbeReject),
        }
    }
    /// 无 A 签探针 → ItemMissing。
    pub fn probe_sign_a(sign: &DualSign) -> Result<(), ClCode> {
        if sign.signer_a.is_none() { Err(ClCode::ItemMissing) } else { Ok(()) }
    }
    /// 作废幂等闸：二次作废零副作用。
    pub fn void_idempotent(sign: &mut DualSign) -> bool {
        sign.void(ClCode::Invalidated);
        let snap = (sign.valid, sign.void_reason, sign.signer_a.is_none());
        sign.void(ClCode::Invalidated);
        snap == (sign.valid, sign.void_reason, sign.signer_a.is_none())
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves400_checks() -> CheckSet {
    let mut set = CheckSet::new("ves400");
    let mut list = CloseChecklist::new();

    // C400-01 四件缺一拒签。
    list.put(CloseItem::SelfCheck);
    list.put(CloseItem::WalkReceipt);
    let mut sign = DualSign::new();
    sign.sign_a("wb-a");
    sign.sign_b("wb-b").unwrap();
    let v = try_close(&list, &sign);
    chk(&mut set, "C400-01", !v.passed && v.rejected_by == Some(ClCode::ItemMissing), "缺两件即拒");

    // C400-02 缺件定位。
    chk(&mut set, "C400-02", list.missing() == vec!["fuzz-zero-line", "asset-complete"], "缺两件定位");

    // C400-03 阻断未消硬拒（四件齐但走查有阻断）。
    list.put(CloseItem::FuzzZeroLine);
    list.put(CloseItem::AssetComplete);
    list.blockers_open = true;
    let v = try_close(&list, &sign);
    chk(&mut set, "C400-03", !v.passed && v.rejected_by == Some(ClCode::BlockOpen), "阻断硬拒");

    // C400-04 阻断消解后 a11y 工具族缺仍拒（域本色）。
    list.blockers_open = false;
    let v = try_close(&list, &sign);
    chk(&mut set, "C400-04", !v.passed && v.rejected_by == Some(ClCode::A11yToolShort), "工具族缺拒");

    // C400-05 a11y 确认后收口通过。
    list.a11y_tools_ready = true;
    let v = try_close(&list, &sign);
    chk(&mut set, "C400-05", v.passed, "四件+双签+无阻断+工具族齐 → 过");

    // C400-06 同人双签作废。
    let mut bad = DualSign::new();
    bad.sign_a("wb-a");
    chk(&mut set, "C400-06", bad.sign_b("wb-a") == Err(ClCode::SamePerson) && !bad.valid, "同人作废");

    // C400-07 作废后换人重签复通（留痕）。
    let mut re = DualSign::new();
    re.sign_a("wb-a");
    re.sign_b("wb-a").unwrap_err();
    re.void(ClCode::SamePerson);
    re.sign_a("wb-a");
    re.sign_b("wb-c").unwrap();
    chk(&mut set, "C400-07", re.valid && re.void_reason == Some(ClCode::SamePerson), "换签复通留痕");

    // C400-08 域收官预备：T 域八组计数。
    let mut w = DomainWrapup { groups_closed: 7 };
    chk(&mut set, "C400-08", !w.ready() && w.progress() == 7, "7/8 未触发");

    // C400-09 8/8 触发域收官预备。
    w.groups_closed = 8;
    chk(&mut set, "C400-09", w.ready() && DomainWrapup::TOTAL == 8, "8/8 触发预备");

    // C400-10 超计数钳制。
    w.groups_closed = 12;
    chk(&mut set, "C400-10", w.progress() == 8 && w.ready(), "计数钳制 8");

    // C400-11 四件名单单源。
    chk(&mut set, "C400-11", CloseItem::ALL.len() == 4
        && CloseItem::ALL[0].name() == "self-check"
        && CloseItem::ALL[3].name() == "asset-complete", "四件单源");

    // C400-12 缺复核签不通过。
    let half = DualSign::new();
    let mut half = half;
    half.sign_a("wb-a");
    let v = try_close(&list, &half);
    chk(&mut set, "C400-12", !v.passed && v.rejected_by.is_some(), "缺 B 签不通过");

    // ===== 二轮深化判据（C400-13 起）=====
    // C400-13 签署事件账本：四件各记二笔 + 守恒 + 去重触及。
    let mut sl = SignLedger::new();
    for it in CloseItem::ALL.iter() { sl.record(*it); sl.record(*it); }
    chk(&mut set, "C400-13", sl.conserved() && sl.total == 8 && sl.signed_items() == 4, "签署账本守恒");

    // C400-14 进度账本推进 + 整数分位（排序取点禁浮点）。
    let mut cp = CloseProgress::new();
    let _ = cp.bump(0); let _ = cp.bump(0); let _ = cp.bump(0);
    let q50 = cp.quantile(50);
    let q100 = cp.quantile(100);
    chk(&mut set, "C400-14", cp.conserved() && q50 == Ok(0) && q100 == Ok(3), "进度分位 0/3");

    // C400-15 边界探针：越界组号拒 + 满容量拒 + 非法分位拒。
    let mut cp2 = CloseProgress::new();
    let mut full_ok = true;
    for _ in 0..20 { full_ok = full_ok && cp2.bump(3).is_ok(); }
    chk(&mut set, "C400-15", cp2.bump(8) == Err(ClCode::ProbeReject)
        && cp2.bump(3) == Err(ClCode::ProbeReject) && full_ok
        && cp2.quantile(101) == Err(ClCode::ProbeReject)
        && cp2.group_done(3) && !cp2.group_done(0), "越界满容分位三拒");

    // C400-16 收口态投影 6B + 指纹幂等稳定。
    let mut cb: Vec<u8> = Vec::new();
    close_bytes(&list, &sign, &mut cb);
    match close_fingerprint_stable(&list, &sign) {
        Ok(v) => chk(&mut set, "C400-16", cb.len() == 6
            && close_fingerprint_stable(&list, &sign) == Ok(v), "收口态投影 6B 指纹幂等"),
        Err(_) => chk(&mut set, "C400-16", false, "指纹不应漂移"),
    }

    // C400-17 边界探针：空清单拒 / 越界序号拒 / 无 A 签拒。
    let empty_list = CloseChecklist::new();
    let no_sign = DualSign::new();
    chk(&mut set, "C400-17", CloseProbe::probe_empty(&empty_list) == Err(ClCode::ItemMissing)
        && CloseProbe::probe_empty(&list).is_ok()
        && CloseProbe::probe_index(&list, 99) == Err(ClCode::ProbeReject)
        && CloseProbe::probe_index(&list, 0) == Ok("self-check")
        && CloseProbe::probe_sign_a(&no_sign) == Err(ClCode::ItemMissing), "收口探针三拒");

    // C400-18 作废幂等闸：二次作废零副作用。
    let mut vs = DualSign::new();
    vs.sign_a("wb-a");
    vs.sign_b("wb-b").unwrap();
    chk(&mut set, "C400-18", CloseProbe::void_idempotent(&mut vs) && !vs.valid, "作废幂等零副作用");

    // C400-19 深化变体段位不撞段。
    chk(&mut set, "C400-19", ClCode::LedgerBroken as u16 == 0x00A6
        && ClCode::ProbeReject as u16 == 0x00A7 && ClCode::RoundtripDrift as u16 == 0x00A8,
        "深化段位正确");

    // C400-20 进度账本空态退化安全：全零分位与守恒。
    let cp0 = CloseProgress::new();
    chk(&mut set, "C400-20", cp0.conserved() && cp0.total() == 0
        && cp0.quantile(0) == Ok(0) && cp0.quantile(100) == Ok(0), "空进度退化安全");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves400_smoke() {
        let set = run_ves400_checks();
        assert!(set.failed_count() == 0, "ves400 red: {}", set.red_items_text());
    }

    #[test]
    fn ves400_void_clears() {
        let mut d = DualSign::new();
        d.sign_a("x");
        d.sign_b("y").unwrap();
        d.void(ClCode::Invalidated);
        assert!(!d.valid);
        assert!(d.signer_a.is_none() && d.signer_b.is_none());
    }
}
