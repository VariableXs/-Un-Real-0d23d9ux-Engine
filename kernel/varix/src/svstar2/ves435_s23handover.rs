//! ves435_s23handover — VE-F4195 · T 域移交包（U 域 F4201）
//! 移交：T 域移交包封装（总账/词典/联测/基准/安全/无障碍/观测/fuzz/文档/清账十件+冻结哈希+遗留三路清单）；
//! U 域交接面声明（一致性域承接点：交互词典/地区规则/术语库作为一致性契约源输入）；
//! 件缺→拒收；哈希不符→重封；交接面缺→补面；
//! 交接面含无障碍口径契约化专项（域本色）；包内证据已脱敏。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1320;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HvCode {
    Ok = 0,
    ItemMissing = SEG_BASE + 0x01, // 0x1321
    HashMismatch = SEG_BASE + 0x02, // 0x1322
    FaceMissing = SEG_BASE + 0x03, // 0x1323
    Unsealed = SEG_BASE + 0x04,    // 0x1324
}

/// 十件移交内容。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HandoverItem {
    Ledger = 0, Lexicon = 1, JointTest = 2, PerfBase = 3, Security = 4,
    A11y = 5, Observe = 6, Fuzz = 7, Docs = 8, Accounting = 9,
}

impl HandoverItem {
    pub const ALL: [HandoverItem; 10] = [
        HandoverItem::Ledger, HandoverItem::Lexicon, HandoverItem::JointTest, HandoverItem::PerfBase,
        HandoverItem::Security, HandoverItem::A11y, HandoverItem::Observe, HandoverItem::Fuzz,
        HandoverItem::Docs, HandoverItem::Accounting];
    pub fn name(self) -> &'static str {
        match self {
            HandoverItem::Ledger => "ledger", HandoverItem::Lexicon => "lexicon",
            HandoverItem::JointTest => "joint-test", HandoverItem::PerfBase => "perf-base",
            HandoverItem::Security => "security", HandoverItem::A11y => "a11y",
            HandoverItem::Observe => "observe", HandoverItem::Fuzz => "fuzz",
            HandoverItem::Docs => "docs", HandoverItem::Accounting => "accounting",
        }
    }
}

/// U 域交接面（一致性契约源输入，域本色承接点）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HandoverFace { LexiconContract = 0, RegionRules = 1, TermBase = 2, A11yCaliber = 3 }

impl HandoverFace {
    pub const ALL: [HandoverFace; 4] = [
        HandoverFace::LexiconContract, HandoverFace::RegionRules, HandoverFace::TermBase, HandoverFace::A11yCaliber];
}

/// 移交包。
pub struct HandoverPack {
    pub items: Vec<HandoverItem>,
    pub sanitized: bool,             // 包内证据脱敏终版
    pub faces: Vec<HandoverFace>,    // 交接面
    pub legacy_disposed: Vec<u32>,   // 遗留三路清单（已处置 id）
    pub hash: u32,
    pub sealed: bool,
}

impl HandoverPack {
    pub fn new() -> Self {
        HandoverPack { items: Vec::new(), sanitized: false, faces: Vec::new(), legacy_disposed: Vec::new(), hash: 0, sealed: false }
    }
    pub fn put(&mut self, it: HandoverItem) { if !self.items.contains(&it) { self.items.push(it); } }
    pub fn missing(&self) -> Vec<HandoverItem> {
        HandoverItem::ALL.iter().filter(|i| !self.items.contains(i)).map(|i| *i).collect()
    }
    /// 封包：十件齐+脱敏+四交接面齐+遗留全登记→哈希+封。
    pub fn seal(&mut self) -> Result<(), HvCode> {
        if self.missing().len() > 0 { return Err(HvCode::ItemMissing); }
        if !self.sanitized { return Err(HvCode::Unsealed); }
        for f in HandoverFace::ALL {
            if !self.faces.contains(&f) { return Err(HvCode::FaceMissing); }
        }
        if self.legacy_disposed.is_empty() { return Err(HvCode::Unsealed); }
        let mut h: u32 = 0x811C_9DC5;
        for it in &self.items {
            h ^= (*it) as u32; h = h.wrapping_mul(0x0100_0193);
        }
        self.hash = h;
        self.sealed = true;
        Ok(())
    }
    /// 收方核验：重算哈希与封印一致。
    pub fn verify(&self) -> Result<(), HvCode> {
        if !self.sealed { return Err(HvCode::Unsealed); }
        let mut h: u32 = 0x811C_9DC5;
        for it in &self.items {
            h ^= (*it) as u32; h = h.wrapping_mul(0x0100_0193);
        }
        if h != self.hash { return Err(HvCode::HashMismatch); }
        Ok(())
    }
    /// 重封（哈希不符路径）。
    pub fn reseal(&mut self) { self.sealed = false; self.seal().unwrap(); }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves435_checks() -> CheckSet {
    let mut set = CheckSet::new("ves435");
    let mut pk = HandoverPack::new();

    // C435-01 件缺→拒收（缺件定位）。
    for it in HandoverItem::ALL[..8].iter() { pk.put(*it); }
    let miss = pk.missing();
    chk(&mut set, "C435-01", pk.seal() == Err(HvCode::ItemMissing)
        && miss == vec![HandoverItem::Docs, HandoverItem::Accounting], "缺两件拒收");

    // C435-02 十件齐后因未脱敏拒。
    for it in [HandoverItem::Docs, HandoverItem::Accounting] { pk.put(it); }
    chk(&mut set, "C435-02", pk.seal() == Err(HvCode::Unsealed), "未脱敏拒");

    // C435-03 脱敏后因交接面缺拒（补面路径）。
    pk.sanitized = true;
    chk(&mut set, "C435-03", pk.seal() == Err(HvCode::FaceMissing), "交接面缺拒");

    // C435-04 四交接面齐（含无障碍口径契约化——域本色）。
    for f in HandoverFace::ALL { pk.faces.push(f); }
    chk(&mut set, "C435-04", pk.seal() == Err(HvCode::Unsealed), "遗留清单缺拒");

    // C435-05 遗留三路清单登记后封包通过。
    for id in [501u32, 502] { pk.legacy_disposed.push(id); }
    chk(&mut set, "C435-05", pk.seal().is_ok() && pk.sealed && pk.hash != 0, "封包通过");

    // C435-06 收方核验哈希一致。
    chk(&mut set, "C435-06", pk.verify().is_ok(), "收方核验过");

    // C435-07 篡改检出（加件后哈希失配）。
    pk.put(HandoverItem::Ledger);
    chk(&mut set, "C435-07", pk.verify() == Err(HvCode::HashMismatch), "篡改检出");

    // C435-08 重封后核验复通（幂等件不扰动哈希）。
    pk.reseal();
    chk(&mut set, "C435-08", pk.verify().is_ok(), "重封复通");

    // C435-09 十件名单单源。
    chk(&mut set, "C435-09", HandoverItem::ALL.len() == 10
        && HandoverItem::ALL[0].name() == "ledger"
        && HandoverItem::ALL[9].name() == "accounting", "十件单源");

    // C435-10 交接面四项单源（a11y 契约化在册）。
    chk(&mut set, "C435-10", HandoverFace::ALL.len() == 4
        && HandoverFace::ALL[3] == HandoverFace::A11yCaliber, "交接面单源");

    // C435-11 U 域承接点断言：词典/地区/术语三契约源在交接面。
    chk(&mut set, "C435-11", pk.faces.contains(&HandoverFace::LexiconContract)
        && pk.faces.contains(&HandoverFace::RegionRules)
        && pk.faces.contains(&HandoverFace::TermBase), "承接点在册");

    // C435-12 件幂等（重复 put 不扰动）。
    let n = pk.items.len();
    pk.put(HandoverItem::Fuzz);
    chk(&mut set, "C435-12", pk.items.len() == n && pk.missing().is_empty(), "件幂等");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves435_smoke() {
        let set = run_ves435_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves435 red: {:?}", red);
    }
}
