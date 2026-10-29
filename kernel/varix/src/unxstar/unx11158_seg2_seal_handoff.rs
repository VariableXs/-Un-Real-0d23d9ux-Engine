//! UNX-F11158 清账段二封存（段二账册移交注账）（0x1f3a0 段）
//!
//! 纯功能模型：段二收口后封存三件——总账册、明细册、指纹册，各带版本戳与
//! 内容哈希；封存后向 B39 验收域移交并注账接收人。三件须件件已封、版本同戳、
//! 哈希非零，移交注账须有接收人，缺一项即封存不成立。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f3a0;
/// 封存件号越界（只认 0..3）
pub const SL_ENO: u32 = SEG_BASE + 0x01;
/// 封存件重记
pub const SL_EDUP: u32 = SEG_BASE + 0x02;
/// 封存件序倒挂
pub const SL_EORDER: u32 = SEG_BASE + 0x03;
/// 版本戳为零或哈希为零（形状违规）
pub const SL_EVER: u32 = SEG_BASE + 0x04;
/// 三件未齐
pub const SL_ESHORT: u32 = SEG_BASE + 0x05;
/// 有件未封
pub const SL_EUNSEALED: u32 = SEG_BASE + 0x06;
/// 移交注账无接收人
pub const SL_ERECV: u32 = SEG_BASE + 0x07;
/// 终值已锁定
pub const SL_EFROZEN: u32 = SEG_BASE + 0x08;

/// 封存件数。
pub const ITEMS: usize = 3;
/// 件名（静态）。
pub const ITEM_NAMES: [&str; ITEMS] = ["ledger", "detail", "digest"];
/// 版本戳口径。
pub const VERSION: u16 = 1;
/// 移交目标域。
pub const HANDOFF_DOMAIN: &str = "B39";

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 一件封存物。
#[derive(Clone, Copy, Debug)]
pub struct SealItem {
    pub id: u8,
    pub version: u16,
    pub hash: u32,
    pub sealed: bool,
}

impl SealItem {
    /// 件是否封到位（已封、版本合口径、哈希非零）。
    pub fn sealed_ok(&self) -> bool {
        self.sealed && self.version == VERSION && self.hash != 0
    }
}

/// 段二封存与移交注账。
#[derive(Clone, Debug)]
pub struct SealLedger {
    items: Vec<SealItem>,
    receiver: u8,
    settled: Option<u64>,
    frozen: bool,
    pub accepts: u64,
    pub rejects: u64,
}

impl SealLedger {
    /// 空账。
    pub fn new() -> SealLedger {
        SealLedger {
            items: Vec::new(),
            receiver: 0,
            settled: None,
            frozen: false,
            accepts: 0,
            rejects: 0,
        }
    }

    /// 已封件数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 取某件。
    pub fn item(&self, id: usize) -> Option<SealItem> {
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].id as usize == id {
                return Some(self.items[i]);
            }
            i = i.wrapping_add(1);
        }
        None
    }

    /// 封一件：冻结 → 越界 → 重记 → 倒序 → 版本与哈希形状。
    pub fn seal(&mut self, id: usize, version: u16, hash: u32, sealed: bool) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return self.fail(SL_EFROZEN);
        }
        if id >= ITEMS {
            return self.fail(SL_ENO);
        }
        if self.item(id).is_some() {
            return self.fail(SL_EDUP);
        }
        if let Some(last) = self.items.last() {
            if id <= last.id as usize {
                return self.fail(SL_EORDER);
            }
        }
        if version == 0 || hash == 0 {
            return self.fail(SL_EVER);
        }
        self.items.push(SealItem {
            id: id as u8,
            version,
            hash,
            sealed,
        });
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.fold())
    }

    /// 注账接收人（只允许一次，非零）。
    pub fn note(&mut self, receiver: u8) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return self.fail(SL_EFROZEN);
        }
        if receiver == 0 {
            return self.fail(SL_ERECV);
        }
        if self.receiver != 0 {
            return self.fail(SL_EDUP);
        }
        self.receiver = receiver;
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.fold())
    }

    /// 记账一次拒止。
    fn fail(&mut self, code: u32) -> Result<u64, u32> {
        self.rejects = self.rejects.wrapping_add(1);
        Err(code)
    }

    /// 接收位。
    pub fn receiver(&self) -> u8 {
        self.receiver
    }

    /// 已封件数（sealed 为真）。
    pub fn sealed_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].sealed_ok() {
                n = n.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 版本戳是否同戳。
    pub fn versions_uniform(&self) -> bool {
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].version != VERSION {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 指纹是否非零且两两不同（三件各自身份）。
    pub fn hashes_distinct(&self) -> bool {
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].hash == 0 {
                return false;
            }
            let mut j = i.wrapping_add(1);
            while j < self.items.len() {
                if self.items[i].hash == self.items[j].hash {
                    return false;
                }
                j = j.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 缺件表。
    pub fn gaps(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut id = 0usize;
        while id < ITEMS {
            if self.item(id).is_none() {
                out.push(id as u32);
            }
            id = id.wrapping_add(1);
        }
        out
    }

    /// 件序严格。
    pub fn order_strict(&self) -> bool {
        let mut i = 1usize;
        while i < self.items.len() {
            if self.items[i].id <= self.items[i - 1].id {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 封存指纹。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.items.len() {
            let it = self.items[i];
            h = (h ^ it.id as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ it.version as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ it.hash as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ it.sealed as u64).wrapping_mul(FNV_PRIME);
            i = i.wrapping_add(1);
        }
        h = (h ^ self.items.len() as u64).wrapping_mul(FNV_PRIME);
        (h ^ self.receiver as u64).wrapping_mul(FNV_PRIME)
    }

    /// 封存成立：三件齐 → 件件封到位 → 版本同戳 → 哈希互异 → 接收人非零。
    pub fn settle(&mut self) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return self.fail(SL_EFROZEN);
        }
        if self.items.len() != ITEMS || !self.gaps().is_empty() {
            return self.fail(SL_ESHORT);
        }
        if self.sealed_count() != ITEMS as u32 {
            return self.fail(SL_EUNSEALED);
        }
        if !self.versions_uniform() || !self.hashes_distinct() {
            return self.fail(SL_EVER);
        }
        if self.receiver == 0 {
            return self.fail(SL_ERECV);
        }
        let v = self.fold();
        self.settled = Some(v);
        Ok(v)
    }

    /// 冻结：锁当前值，幂等。
    pub fn freeze(&mut self) -> Option<u64> {
        if self.frozen {
            return self.settled;
        }
        if self.settled.is_none() {
            self.settled = Some(self.fold());
        }
        self.frozen = true;
        self.settled
    }

    /// 终值。
    pub fn settled_value(&self) -> Option<u64> {
        self.settled
    }

    /// 是否冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}

/// 建账：三件封存（同版本、互异哈希），再注账接收人并冻结移交。
pub fn build_seal() -> (SealLedger, bool) {
    let mut l = SealLedger::new();
    let hashes = [0xA1B2_C3D4u32, 0x1122_3344, 0x5566_7788];
    let mut id = 0usize;
    while id < ITEMS {
        let hash = hashes.get(id).copied().unwrap_or(0);
        if l.seal(id, VERSION, hash, true).is_err() {
            return (l, false);
        }
        id = id.wrapping_add(1);
    }
    if l.note(7).is_err() {
        return (l, false);
    }
    let ok = l.settle().is_ok();
    (l, ok)
}

/// 四类注入：件号越界 / 重记与倒序 / 版本或哈希为零 / 终账后再封。
pub fn inject_four() -> [bool; 4] {
    // ① 件号越界
    let mut a0 = SealLedger::new();
    let r0 = a0.seal(ITEMS, 1, 0x10, true).err() == Some(SL_ENO)
        && a0.seal(ITEMS.wrapping_add(3), 1, 0x20, true).err() == Some(SL_ENO)
        && a0.is_empty();

    // ② 重记与倒序
    let mut a1 = SealLedger::new();
    let s0 = a1.seal(0, 1, 0x10, true).is_ok();
    let s1 = a1.seal(1, 1, 0x20, true).is_ok();
    let dup = s0 && s1 && a1.seal(0, 1, 0x10, true).err() == Some(SL_EDUP);
    let mut a2 = SealLedger::new();
    let _ = a2.seal(2, 1, 0x30, true);
    let order = a2.seal(1, 1, 0x40, true).err() == Some(SL_EORDER);
    let r1 = dup && order;

    // ③ 零版本 / 零哈希 / 无接收人
    let mut a3 = SealLedger::new();
    let shape = a3.seal(0, 0, 0x10, true).err() == Some(SL_EVER)
        && a3.seal(0, 1, 0, true).err() == Some(SL_EVER);
    let mut a4 = SealLedger::new();
    let _ = a4.seal(0, 1, 0x10, true);
    let _ = a4.seal(1, 1, 0x20, true);
    let _ = a4.seal(2, 1, 0x30, true);
    let recv = a4.settle().err() == Some(SL_ERECV);
    let r2 = shape && recv;

    // ④ 终账后再封 / 再清 / 再注账
    let (mut a5, ok5) = build_seal();
    let locked = a5.freeze().is_some();
    let r3 = ok5
        && locked
        && a5.seal(0, 1, 0x10, true).err() == Some(SL_EFROZEN)
        && a5.note(9).err() == Some(SL_EFROZEN)
        && a5.settle().err() == Some(SL_EFROZEN);

    [r0, r1, r2, r3]
}

/// 判据：UNX-F11158 清账段二封存与移交注账。
pub fn run_unx11158_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11158");
    let (mut l, ok) = build_seal();

    // X1 三件齐备
    s.add_dyn(
        "X1-三件齐备",
        ok && l.len() == ITEMS && l.gaps().is_empty(),
        &format!("items={}/{} gaps={}", l.len(), ITEMS, l.gaps().len()),
    );

    // X2 件件封到位
    s.add_dyn(
        "X2-件件封到位",
        l.sealed_count() == ITEMS as u32,
        &format!("sealed={}/{}", l.sealed_count(), ITEMS),
    );

    // X3 版本同戳
    s.add(
        "X3-版本同戳",
        l.versions_uniform() && l.item(0).map(|x| x.version) == Some(VERSION),
        "三件版本戳一致，零漂移",
    );

    // X4 哈希互异非零
    s.add("X4-哈希互异", l.hashes_distinct(), "三件内容哈希非零且两两不同");

    // X5 接收人非零
    s.add_dyn(
        "X5-接收人非零",
        l.receiver() != 0,
        &format!("receiver={} target={}", l.receiver(), HANDOFF_DOMAIN),
    );

    // X6 件序严格
    s.add("X6-件序严格", l.order_strict(), "ledger/detail/digest 按 0..2 递增");

    // X7 终值即指纹
    let root = l.settled_value();
    s.add("X7-终值即指纹", root == Some(l.fold()), "封存单值等于三件指纹");

    // X8 冻结幂等
    let f1 = l.freeze();
    let f2 = l.freeze();
    s.add("X8-冻结幂等", f1.is_some() && f1 == f2, "二次冻结返回同一单值");

    // X9 冻结后拒封拒注
    s.add(
        "X9-冻结后拒封拒注",
        l.is_frozen()
            && l.seal(0, 1, 0x10, true).err() == Some(SL_EFROZEN)
            && l.note(9).err() == Some(SL_EFROZEN)
            && l.settle().err() == Some(SL_EFROZEN),
        "冻结后 seal/note/settle 都落 SL_EFROZEN",
    );

    // X10 越界 / 重记 / 倒序
    let mut g = SealLedger::new();
    let g0 = g.seal(0, 1, 0x10, true).is_ok();
    let mut o = SealLedger::new();
    let _ = o.seal(2, 1, 0x30, true);
    s.add(
        "X10-越界重记倒序",
        g.seal(ITEMS, 1, 0x40, true).err() == Some(SL_ENO)
            && g0
            && g.seal(0, 1, 0x10, true).err() == Some(SL_EDUP)
            && o.seal(1, 1, 0x20, true).err() == Some(SL_EORDER),
        "越界 ENO、重记 EDUP、倒序 EORDER 各按其码",
    );

    // X11 零版本 / 零哈希 / 无接收人
    let mut sh = SealLedger::new();
    s.add(
        "X11-形状与接收人拒止",
        sh.seal(0, 0, 0x10, true).err() == Some(SL_EVER)
            && sh.seal(0, 1, 0, true).err() == Some(SL_EVER)
            && sh.note(0).err() == Some(SL_ERECV)
            && sh.is_empty(),
        "零版本/零哈希落 EVER，零接收人落 ERECV",
    );

    // X12 未齐 / 有件未封 / 无接收人
    let mut q1 = SealLedger::new();
    let _ = q1.seal(0, 1, 0x10, true);
    let _ = q1.seal(1, 1, 0x20, true);
    let _ = q1.note(7);
    let mut q2 = SealLedger::new();
    let _ = q2.seal(0, 1, 0x10, true);
    let _ = q2.seal(1, 1, 0x20, false);
    let _ = q2.seal(2, 1, 0x30, true);
    let _ = q2.note(7);
    let mut q3 = SealLedger::new();
    let _ = q3.seal(0, 1, 0x10, true);
    let _ = q3.seal(1, 1, 0x20, true);
    let _ = q3.seal(2, 1, 0x30, true);
    let hits = inject_four();
    let all = hits[0] && hits[1] && hits[2] && hits[3];
    let (mut fresh, ok2) = build_seal();
    let root2 = fresh.freeze().unwrap_or(0);
    s.add_dyn(
        "X12-三拒与注入",
        q1.settle().err() == Some(SL_ESHORT)
            && q2.settle().err() == Some(SL_EUNSEALED)
            && q3.settle().err() == Some(SL_ERECV)
            && all
            && ok2
            && fresh.accepts == 4
            && fresh.rejects == 0
            && root2 != 0,
        &format!(
            "inject={}{}{}{} acc={} rej={} root={:#x}",
            hits[0] as u8, hits[1] as u8, hits[2] as u8, hits[3] as u8,
            fresh.accepts, fresh.rejects, root2
        ),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f11158_flow() {
        let (mut l, ok) = build_seal();
        assert!(ok);
        assert_eq!(l.len(), ITEMS);
        assert_eq!(l.sealed_count(), ITEMS as u32);
        assert!(l.versions_uniform());
        assert!(l.hashes_distinct());
        assert_eq!(l.receiver(), 7);
        assert_eq!(l.accepts, 4);
        let root = l.settled_value();
        assert_eq!(root, Some(l.fold()));
        assert_eq!(l.freeze(), root);
        assert_eq!(l.freeze(), root);
        assert!(l.is_frozen());
        assert_eq!(l.seal(0, 1, 0x10, true), Err(SL_EFROZEN));
        assert_eq!(l.note(9), Err(SL_EFROZEN));
        assert_eq!(l.settle(), Err(SL_EFROZEN));
    }

    #[test]
    fn f11158_gates() {
        let mut l = SealLedger::new();
        assert!(l.seal(0, 1, 0x10, true).is_ok());
        assert!(l.seal(1, 1, 0x20, true).is_ok());
        assert_eq!((l.accepts, l.rejects), (2, 0));
        assert_eq!(l.seal(ITEMS, 1, 0x40, true), Err(SL_ENO));
        assert_eq!(l.seal(0, 1, 0x10, true), Err(SL_EDUP));
        assert_eq!((l.accepts, l.rejects), (2, 2));

        let mut v = SealLedger::new();
        assert_eq!(v.seal(0, 0, 0x10, true), Err(SL_EVER));
        assert_eq!(v.seal(0, 1, 0, true), Err(SL_EVER));
        assert_eq!((v.accepts, v.rejects), (0, 2));

        let mut o = SealLedger::new();
        let _ = o.seal(2, 1, 0x30, true);
        assert_eq!(o.seal(1, 1, 0x20, true), Err(SL_EORDER));
        assert_eq!((o.accepts, o.rejects), (1, 1));

        let mut sh = SealLedger::new();
        let _ = sh.seal(0, 1, 0x10, true);
        let _ = sh.seal(1, 1, 0x20, true);
        let _ = sh.note(7);
        assert_eq!(sh.settle(), Err(SL_ESHORT));
        assert_eq!((sh.accepts, sh.rejects), (3, 1));

        let mut u = SealLedger::new();
        let _ = u.seal(0, 1, 0x10, true);
        let _ = u.seal(1, 1, 0x20, false);
        let _ = u.seal(2, 1, 0x30, true);
        let _ = u.note(7);
        assert_eq!(u.settle(), Err(SL_EUNSEALED));
        assert_eq!((u.accepts, u.rejects), (4, 1));

        let mut r = SealLedger::new();
        let _ = r.seal(0, 1, 0x10, true);
        let _ = r.seal(1, 1, 0x20, true);
        let _ = r.seal(2, 1, 0x30, true);
        assert_eq!(r.settle(), Err(SL_ERECV));
        assert_eq!((r.accepts, r.rejects), (3, 1));

        let mut rc = SealLedger::new();
        assert_eq!(rc.note(0), Err(SL_ERECV));
        assert_eq!((rc.accepts, rc.rejects), (0, 1));

        let (mut d, ok) = build_seal();
        assert!(ok);
        assert!(d.freeze().is_some());
        assert_eq!(d.seal(0, 1, 0x10, true), Err(SL_EFROZEN));
        assert_eq!(d.note(9), Err(SL_EFROZEN));
        assert_eq!(d.settle(), Err(SL_EFROZEN));
        assert_eq!((d.accepts, d.rejects), (4, 3));
    }

    #[test]
    fn f11158_checks_green() {
        assert!(run_unx11158_checks().all_passed());
    }
}
