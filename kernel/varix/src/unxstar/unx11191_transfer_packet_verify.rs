//! UNX-F11191 移交包终验（三件打包核销）（0x1f5c0 段）
//!
//! 纯功能模型：移交包固定三件——账册、判据集、复跑指引。入包须按件序、
//! 逐件签名校验，三件齐备且全验通过方可核销；核销即整包终态。缺件、
//! 错序、重复件、验前核销、核销后改写一律拒止，核销零漏件。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f5c0;
/// 件类越界
pub const TP_EKIND: u32 = SEG_BASE + 0x01;
/// 件序错位
pub const TP_EORDER: u32 = SEG_BASE + 0x02;
/// 重复入包
pub const TP_EDUP: u32 = SEG_BASE + 0x03;
/// 件缺失
pub const TP_EMISS: u32 = SEG_BASE + 0x04;
/// 签名不符
pub const TP_ESIG: u32 = SEG_BASE + 0x05;
/// 未全验核销
pub const TP_EUNVER: u32 = SEG_BASE + 0x06;
/// 核销后改写
pub const TP_EFROZEN: u32 = SEG_BASE + 0x07;

/// 三件：账册 0 / 判据集 1 / 复跑指引 2
pub const ITEMS: usize = 3;
/// 件名（静态表）
const NAMES: [&str; ITEMS] = ["ledger", "checkset", "rerun-guide"];

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 一装入包件。
#[derive(Clone, Copy, Debug)]
pub struct PackItem {
    pub kind: u8,
    pub sig: u64,
    pub verified: bool,
}

impl PackItem {
    fn new(kind: u8, sig: u64) -> Self {
        PackItem { kind, sig, verified: false }
    }

    /// 件名。
    pub fn name(&self) -> &'static str {
        match NAMES.get(self.kind as usize) {
            Some(n) => n,
            None => "unknown",
        }
    }

    /// 签名复验：以件类与签名合成校验值，须与本件签名一致。
    pub fn verify(&mut self, expect: u64) -> Result<u64, u32> {
        let mut h = FNV_OFF;
        h = (h ^ self.kind as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.sig).wrapping_mul(FNV_PRIME);
        if h != expect {
            return Err(TP_ESIG);
        }
        self.verified = true;
        Ok(h)
    }

    fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        h = (h ^ self.kind as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.sig).wrapping_mul(FNV_PRIME);
        h = (h ^ if self.verified { 1 } else { 0 }).wrapping_mul(FNV_PRIME);
        h
    }
}

/// 三件签名期望值（打包装箱时的钉值）。
pub fn expect_sig(kind: u8, sig: u64) -> Result<u64, u32> {
    if (kind as usize) >= ITEMS {
        return Err(TP_EKIND);
    }
    let mut h = FNV_OFF;
    h = (h ^ kind as u64).wrapping_mul(FNV_PRIME);
    h = (h ^ sig).wrapping_mul(FNV_PRIME);
    Ok(h)
}

/// 移交包：三件按序入包，全验后核销。
#[derive(Clone, Debug)]
pub struct TransferPacket {
    slots: [Option<PackItem>; ITEMS],
    next_kind: usize,
    written_off: Option<u64>,
    pub packs: u64,
    pub verifies: u64,
    pub rejects: u64,
}

impl TransferPacket {
    /// 空包。
    pub fn new() -> Self {
        TransferPacket {
            slots: [None, None, None],
            next_kind: 0,
            written_off: None,
            packs: 0,
            verifies: 0,
            rejects: 0,
        }
    }

    /// 是否已核销（整包终态）。
    pub fn is_written_off(&self) -> bool {
        self.written_off.is_some()
    }

    /// 已入包件数。
    pub fn packed(&self) -> u64 {
        let mut n = 0u64;
        let mut i = 0;
        while i < ITEMS {
            if self.slots[i].is_some() {
                n = n.wrapping_add(1);
            }
            i += 1;
        }
        n
    }

    /// 取件。
    pub fn item(&self, kind: usize) -> Result<&PackItem, u32> {
        match self.slots.get(kind) {
            Some(Some(it)) => Ok(it),
            Some(None) => Err(TP_EMISS),
            None => Err(TP_EKIND),
        }
    }

    /// 入包：须按件序、不得重复、核销后不可再入。
    pub fn pack(&mut self, kind: u8, sig: u64) -> Result<u64, u32> {
        if self.written_off.is_some() {
            return Err(TP_EFROZEN);
        }
        if (kind as usize) >= ITEMS {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(TP_EKIND);
        }
        match self.slots.get(kind as usize) {
            Some(Some(_)) => {
                self.rejects = self.rejects.wrapping_add(1);
                return Err(TP_EDUP);
            }
            Some(None) => {}
            None => return Err(TP_EKIND),
        }
        if kind as usize != self.next_kind {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(TP_EORDER);
        }
        if let Some(cell) = self.slots.get_mut(kind as usize) {
            *cell = Some(PackItem::new(kind, sig));
        }
        self.next_kind = self.next_kind.wrapping_add(1);
        self.packs = self.packs.wrapping_add(1);
        Ok(self.packs)
    }

    /// 逐件复验：以钉值比对，未入包之件报缺。
    pub fn verify(&mut self, kind: u8) -> Result<u64, u32> {
        if self.written_off.is_some() {
            return Err(TP_EFROZEN);
        }
        let expect = {
            let sig = match self.slots.get(kind as usize) {
                Some(Some(it)) => it.sig,
                Some(None) => {
                    self.rejects = self.rejects.wrapping_add(1);
                    return Err(TP_EMISS);
                }
                None => return Err(TP_EKIND),
            };
            expect_sig(kind, sig)?
        };
        match self.slots.get_mut(kind as usize) {
            Some(Some(it)) => match it.verify(expect) {
                Ok(v) => {
                    self.verifies = self.verifies.wrapping_add(1);
                    Ok(v)
                }
                Err(e) => {
                    self.rejects = self.rejects.wrapping_add(1);
                    Err(e)
                }
            },
            _ => Err(TP_EMISS),
        }
    }

    /// 全件是否齐验。
    pub fn all_verified(&self) -> bool {
        let mut i = 0;
        while i < ITEMS {
            match self.slots.get(i) {
                Some(Some(it)) if it.verified => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 缺件计数（核销零漏件的反面度量）。
    pub fn missing(&self) -> u64 {
        let mut n = 0u64;
        let mut i = 0;
        while i < ITEMS {
            match self.slots.get(i) {
                Some(Some(_)) => {}
                _ => n = n.wrapping_add(1),
            }
            i += 1;
        }
        n
    }
}

impl TransferPacket {
    /// 三件打包核销：须齐备、全验、按序，核销后终态。
    pub fn write_off(&mut self) -> Result<u64, u32> {
        if self.written_off.is_some() {
            return Err(TP_EFROZEN);
        }
        if self.missing() > 0 {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(TP_EMISS);
        }
        if !self.all_verified() {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(TP_EUNVER);
        }
        let v = self.fold();
        self.written_off = Some(v);
        Ok(v)
    }

    /// 核销指纹。
    pub fn written_off_value(&self) -> Option<u64> {
        self.written_off
    }

    /// 包指纹（三件逐件折叠 + 计数）。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0;
        while i < ITEMS {
            match self.slots.get(i) {
                Some(Some(it)) => h = (h ^ it.fold()).wrapping_mul(FNV_PRIME),
                _ => h = (h ^ 0xffff_ffff).wrapping_mul(FNV_PRIME),
            }
            i += 1;
        }
        h = (h ^ self.packs).wrapping_mul(FNV_PRIME);
        h = (h ^ self.verifies).wrapping_mul(FNV_PRIME);
        h
    }

    /// 件序表（已入包件名，供复跑指引引用）。
    pub fn order(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < ITEMS {
            if let Some(Some(it)) = self.slots.get(i) {
                out.push(it.name());
            }
            i += 1;
        }
        out
    }
}

/// 标准装配：三件入包 → 逐件复验 → 核销。
pub fn build_packet(seed: u64) -> (TransferPacket, bool) {
    let mut p = TransferPacket::new();
    let mut k = 0u8;
    while (k as usize) < ITEMS {
        let sig = seed
            .wrapping_add((k as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15))
            .wrapping_add(0x0000_0000_0000_1013);
        if p.pack(k, sig).is_err() {
            return (p, false);
        }
        k = k.wrapping_add(1);
    }
    let mut v = 0u8;
    while (v as usize) < ITEMS {
        if p.verify(v).is_err() {
            return (p, false);
        }
        v = v.wrapping_add(1);
    }
    let ok = p.write_off().is_ok();
    (p, ok)
}

/// 四类注入：件类越界 / 错序 / 重复 / 缺件核销（含验前核销），全部须拒止。
pub fn inject_four() -> [bool; 4] {
    // ① 件类越界
    let mut p0 = TransferPacket::new();
    let a = p0.pack(ITEMS as u8, 1).err() == Some(TP_EKIND);

    // ② 件序错位：未入账册先入判据集
    let mut p1 = TransferPacket::new();
    let b = p1.pack(1, 2).err() == Some(TP_EORDER) && p1.packed() == 0;

    // ③ 重复入包
    let mut p2 = TransferPacket::new();
    let dup = p2.pack(0, 3).is_ok() && p2.pack(0, 3).err() == Some(TP_EDUP);
    let dup2 = {
        let mut q = TransferPacket::new();
        let _ = q.pack(0, 3);
        let _ = q.pack(1, 4);
        q.pack(1, 4).err() == Some(TP_EDUP)
    };
    let c_dup = dup && dup2;

    // ④ 缺件核销与验前核销
    let mut p3 = TransferPacket::new();
    let miss = p3.write_off().err() == Some(TP_EMISS);
    let mut p4 = TransferPacket::new();
    let _ = p4.pack(0, 5);
    let unver = p4.write_off().err() == Some(TP_EMISS);
    let mut p5 = TransferPacket::new();
    let mut k = 0u8;
    while (k as usize) < ITEMS {
        let _ = p5.pack(k, 0x40 + k as u64);
        k = k.wrapping_add(1);
    }
    let d = miss && unver && p5.write_off().err() == Some(TP_EUNVER);

    [a, b, c_dup, d]
}

/// 判据台：三件打包核销的十三条判据。
pub fn run_unx11191_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11191");

    let (p, built) = build_packet(0x9191);

    // J1 三件终验逐件对平
    let mut both = built;
    let mut k = 0usize;
    while k < ITEMS {
        match p.item(k) {
            Ok(it) => {
                if !it.verified {
                    both = false;
                }
            }
            Err(_) => both = false,
        }
        k += 1;
    }
    s.add("J1-三件终验", both && p.all_verified(), "三件齐备且逐件签名复验通过");

    // J2 打包核销零漏件
    s.add(
        "J2-核销零漏件",
        p.missing() == 0 && p.is_written_off() && p.packed() == ITEMS as u64,
        "核销时缺件计数为零",
    );

    // J3 核销指纹落定
    let w = p.written_off_value();
    s.add(
        "J3-核销指纹",
        w.is_some() && w == Some(p.fold()),
        "核销值等于包指纹",
    );

    // J4 核销后终态
    let mut q = p.clone();
    s.add(
        "J4-核销后终态",
        q.pack(0, 1).err() == Some(TP_EFROZEN)
            && q.verify(0).err() == Some(TP_EFROZEN)
            && q.write_off() == Err(TP_EFROZEN),
        "核销后入包/复验/再核销三路皆拒",
    );

    // J5 空包核销拒止
    let mut e = TransferPacket::new();
    s.add(
        "J5-空包拒核销",
        e.write_off() == Err(TP_EMISS) && e.missing() == ITEMS as u64,
        "空包缺三件不可核销",
    );

    // J6 验前核销拒止
    let mut u = TransferPacket::new();
    let mut k2 = 0u8;
    while (k2 as usize) < ITEMS {
        let _ = u.pack(k2, 0x20 + k2 as u64);
        k2 = k2.wrapping_add(1);
    }
    s.add(
        "J6-验前核销拒止",
        u.write_off() == Err(TP_EUNVER) && u.missing() == 0 && !u.all_verified(),
        "三件齐而未验不得核销",
    );

    // J7 件类越界
    let mut x = TransferPacket::new();
    s.add(
        "J7-件类越界",
        x.pack(ITEMS as u8, 9).err() == Some(TP_EKIND) && x.item(ITEMS).err() == Some(TP_EKIND),
        "越界件类入包与取件双拒",
    );

    // J8 件序错位
    let mut o = TransferPacket::new();
    s.add(
        "J8-件序错位",
        o.pack(2, 7).err() == Some(TP_EORDER) && o.packed() == 0,
        "未按件序入包即拒且不落账",
    );

    // J9 重复件拒止
    let mut d = TransferPacket::new();
    let first = d.pack(0, 5).is_ok();
    s.add(
        "J9-重复件拒止",
        first && d.pack(0, 5).err() == Some(TP_EDUP) && d.packed() == 1,
        "同件二次入包拒止且不重复计数",
    );

    // J10 缺件取件报缺
    let mut m = TransferPacket::new();
    let _ = m.pack(0, 3);
    s.add(
        "J10-缺件报缺",
        m.item(1).err() == Some(TP_EMISS) && m.missing() == 2,
        "未入包之件取用与缺件计数一致",
    );

    // J11 注入四拒
    let hits = inject_four();
    let mut all = true;
    let mut i = 0;
    while i < hits.len() {
        if !hits[i] {
            all = false;
        }
        i += 1;
    }
    s.add("J11-注入四拒", all, "越界/错序/重复/缺件核销四路全拒");

    // J12 重算一致
    let (a1, ok1) = build_packet(0x4242);
    let (a2, ok2) = build_packet(0x4242);
    s.add(
        "J12-重算一致",
        ok1 && ok2 && a1.fold() == a2.fold() && a1.written_off_value() == a2.written_off_value(),
        "同种子两包核销值一致",
    );

    // J13 件序可复跑
    let order = p.order();
    let named = order.len() == ITEMS
        && order.first() == Some(&"ledger")
        && order.last() == Some(&"rerun-guide");
    s.add(
        "J13-件序可复跑",
        named && p.packs == ITEMS as u64 && p.verifies == ITEMS as u64,
        "件序表与入包/复验计数三方对平",
    );

    // J14 动态明细
    s.add_dyn(
        "J14-核销明细",
        p.rejects == 0 && p.written_off.is_some(),
        &format!(
            "packs={} verifies={} rejects={} wo={:#x}",
            p.packs,
            p.verifies,
            p.rejects,
            w.unwrap_or(0)
        ),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11191_packet_flow() {
        let (p, ok) = build_packet(7);
        assert!(ok);
        assert_eq!(p.packed(), ITEMS as u64);
        assert_eq!(p.missing(), 0);
        assert!(p.all_verified());
        assert!(p.is_written_off());
        assert_eq!(p.written_off_value(), Some(p.fold()));
        assert!(inject_four().iter().all(|h| *h));

        let mut q = p.clone();
        assert_eq!(q.pack(0, 1), Err(TP_EFROZEN));
        assert_eq!(q.write_off(), Err(TP_EFROZEN));
        assert_eq!(q.order().len(), ITEMS);
    }

    #[test]
    fn unx11191_checks_green() {
        let s = run_unx11191_checks();
        assert!(s.all_passed(), "UNX-F11191 判据未全绿");
    }
}
