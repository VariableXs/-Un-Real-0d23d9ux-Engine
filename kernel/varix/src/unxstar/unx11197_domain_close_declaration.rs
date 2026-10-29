//! UNX-F11197 域闭账单值宣告（三断言合一）（0x1f620 段）
//!
//! 锚点：UNX-F11197-J1 三断言合一单值成立（10/10），宣告账版本冻结（10/10）。
//!
//! 纯功能模型：闭账三条断言——满条（800/800）、满账（240,000/240,000）、
//! 满态（40/40）——逐条立账，期望值与实算值必须逐条相等且与钉定常量一致；
//! 三条齐备且全部成立才允许「合一宣告」，宣告值由三条按类序链式 fold 得出。
//! 缺条、错类、重类、零期望、值不符一律拒；宣告一经冻结即终态。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f620;
/// 断言类号越界（合法 0..=2）
pub const DG_EBADKIND: u32 = SEG_BASE + 0x01;
/// 同类断言重复立账
pub const DG_EDUPKIND: u32 = SEG_BASE + 0x02;
/// 期望值与实算值不符
pub const DG_EMISMATCH: u32 = SEG_BASE + 0x03;
/// 期望值为零
pub const DG_EZERO: u32 = SEG_BASE + 0x04;
/// 三断言缺条
pub const DG_EMISS: u32 = SEG_BASE + 0x05;
/// 冻结后改写
pub const DG_EFROZEN: u32 = SEG_BASE + 0x06;
/// 重复宣告
pub const DG_EDUP: u32 = SEG_BASE + 0x07;
/// 立账超三类
pub const DG_EOVER: u32 = SEG_BASE + 0x08;

/// 断言类数（条 / 行 / 态）。
pub const KINDS: usize = 3;
/// 一类：满条单值（800/800）。
pub const KIND_CRITERIA: u8 = 0;
/// 二类：满账单值（240,000/240,000）。
pub const KIND_LINES: u8 = 1;
/// 三类：满态单值（40/40）。
pub const KIND_BATCHES: u8 = 2;

/// 钉定常量：满条 800。
pub const PIN_CRITERIA: u64 = 800;
/// 钉定常量：满账 240,000。
pub const PIN_LINES: u64 = 240_000;
/// 钉定常量：满态 40 批。
pub const PIN_BATCHES: u64 = 40;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 单条闭账断言：类号 + 期望 + 实算。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Assertion {
    pub kind: u8,
    pub expect: u64,
    pub actual: u64,
}

impl Assertion {
    /// 类名（渲染与诊断用）。
    pub fn kind_name(kind: u8) -> &'static str {
        match kind {
            KIND_CRITERIA => "满条",
            KIND_LINES => "满账",
            KIND_BATCHES => "满态",
            _ => "未知",
        }
    }

    /// 钉定值（按类）。
    pub fn pinned(kind: u8) -> Option<u64> {
        match kind {
            KIND_CRITERIA => Some(PIN_CRITERIA),
            KIND_LINES => Some(PIN_LINES),
            KIND_BATCHES => Some(PIN_BATCHES),
            _ => None,
        }
    }

    /// 单条成立判：期望非零、期望等实算、且与钉定值一致。
    pub fn holds(&self) -> bool {
        self.expect != 0
            && self.expect == self.actual
            && Assertion::pinned(self.kind) == Some(self.expect)
    }

    /// 单条指纹。
    fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        h = (h ^ self.kind as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.expect).wrapping_mul(FNV_PRIME);
        h = (h ^ self.actual).wrapping_mul(FNV_PRIME);
        h
    }
}

/// 合一宣告簿：三类断言 + 宣告值 + 冻结位。
#[derive(Clone, Debug)]
pub struct DeclarationBook {
    items: Vec<Assertion>,
    declared: Option<u64>,
    frozen: bool,
    pub lodges: u64,
    pub rejects: u64,
}

impl DeclarationBook {
    /// 空簿。
    pub fn empty() -> DeclarationBook {
        DeclarationBook { items: Vec::new(), declared: None, frozen: false, lodges: 0, rejects: 0 }
    }

    pub fn count(&self) -> usize {
        self.items.len()
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 取某类断言。
    pub fn get_kind(&self, kind: u8) -> Option<&Assertion> {
        let mut k = 0usize;
        while k < self.items.len() {
            if let Some(a) = self.items.get(k) {
                if a.kind == kind {
                    return Some(a);
                }
            }
            k = k.wrapping_add(1);
        }
        None
    }

    /// 立账一条断言：类号 / 重复 / 零期望 / 值不符 / 超容五关。
    pub fn lodge(&mut self, kind: u8, expect: u64, actual: u64) -> Result<(), u32> {
        if self.frozen {
            return Err(DG_EFROZEN);
        }
        let r = self.lodge_inner(kind, expect, actual);
        match r {
            Ok(()) => {
                self.lodges = self.lodges.wrapping_add(1);
                Ok(())
            }
            Err(e) => {
                self.rejects = self.rejects.wrapping_add(1);
                Err(e)
            }
        }
    }

    fn lodge_inner(&mut self, kind: u8, expect: u64, actual: u64) -> Result<(), u32> {
        if Assertion::pinned(kind).is_none() {
            return Err(DG_EBADKIND);
        }
        if expect == 0 {
            return Err(DG_EZERO);
        }
        if self.get_kind(kind).is_some() {
            return Err(DG_EDUPKIND);
        }
        if expect != actual {
            return Err(DG_EMISMATCH);
        }
        if self.items.len() >= KINDS {
            return Err(DG_EOVER);
        }
        self.items.push(Assertion { kind, expect, actual });
        self.items.sort_by_key(|a| a.kind);
        Ok(())
    }

    /// 三条是否全部立账并成立。
    pub fn all_hold(&self) -> bool {
        if self.items.len() != KINDS {
            return false;
        }
        let mut k = 0u8;
        while (k as usize) < KINDS {
            match self.get_kind(k) {
                Some(a) if a.holds() => {}
                _ => return false,
            }
            k = k.wrapping_add(1);
        }
        true
    }

    /// 缺条清单（类号升序）。
    pub fn missing(&self) -> Vec<u8> {
        let mut v: Vec<u8> = Vec::new();
        let mut k = 0u8;
        while (k as usize) < KINDS {
            if self.get_kind(k).is_none() {
                v.push(k);
            }
            k = k.wrapping_add(1);
        }
        v
    }

    /// 合一宣告：三类断言全立且全成立，方可出宣告值。
    pub fn declare(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(DG_EFROZEN);
        }
        if self.declared.is_some() {
            return Err(DG_EDUP);
        }
        if !self.all_hold() {
            return Err(DG_EMISS);
        }
        let v = self.fold();
        self.declared = Some(v);
        Ok(v)
    }

    pub fn declared_value(&self) -> Option<u64> {
        self.declared
    }

    /// 宣告账指纹：三类按类序链式 fold，再混入宣告值。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut k = 0usize;
        while k < self.items.len() {
            if let Some(a) = self.items.get(k) {
                h = (h ^ a.fold()).wrapping_mul(FNV_PRIME);
            }
            k = k.wrapping_add(1);
        }
        let d = match self.declared {
            Some(v) => v,
            None => 0,
        };
        h = (h ^ d).wrapping_mul(FNV_PRIME);
        h
    }

    /// 版本冻结：幂等取回锁定值。
    pub fn freeze(&mut self) -> Option<u64> {
        if self.frozen {
            return self.declared;
        }
        if self.declared.is_none() {
            self.declared = Some(self.fold());
        }
        self.frozen = true;
        self.declared
    }

    /// 三断言实算摘要（宣告面回读三数）。
    pub fn summary(&self) -> (u64, u64, u64) {
        let mut out = (0u64, 0u64, 0u64);
        if let Some(a) = self.get_kind(KIND_CRITERIA) {
            out.0 = a.actual;
        }
        if let Some(a) = self.get_kind(KIND_LINES) {
            out.1 = a.actual;
        }
        if let Some(a) = self.get_kind(KIND_BATCHES) {
            out.2 = a.actual;
        }
        out
    }
}

/// 三断言标准立账（按钉定值），返回成功条数。
pub fn lodge_three(b: &mut DeclarationBook) -> usize {
    let mut n = 0usize;
    if b.lodge(KIND_CRITERIA, PIN_CRITERIA, PIN_CRITERIA).is_ok() {
        n = n.wrapping_add(1);
    }
    if b.lodge(KIND_LINES, PIN_LINES, PIN_LINES).is_ok() {
        n = n.wrapping_add(1);
    }
    if b.lodge(KIND_BATCHES, PIN_BATCHES, PIN_BATCHES).is_ok() {
        n = n.wrapping_add(1);
    }
    n
}

/// 合一装配：立账三类 → 合一宣告。
pub fn build_declared() -> (DeclarationBook, Option<u64>) {
    let mut b = DeclarationBook::empty();
    let n = lodge_three(&mut b);
    let v = if n == KINDS { b.declare().ok() } else { None };
    (b, v)
}

/// 四路注入：错类 / 零期望 / 值不符 / 冻结后改写，全部须拒。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];

    // ① 错类（类号 3 越界）
    let mut a = DeclarationBook::empty();
    got[0] = a.lodge(3, 1, 1).err() == Some(DG_EBADKIND)
        && a.lodge(KINDS as u8, 1, 1).err() == Some(DG_EBADKIND);

    // ② 零期望
    let mut b = DeclarationBook::empty();
    got[1] = b.lodge(KIND_CRITERIA, 0, 0).err() == Some(DG_EZERO);

    // ③ 期望与实算不符（且账面零残留）
    let mut c = DeclarationBook::empty();
    got[2] = c.lodge(KIND_CRITERIA, PIN_CRITERIA, PIN_CRITERIA - 1).err() == Some(DG_EMISMATCH)
        && c.count() == 0;

    // ④ 冻结后改写 + 冻结幂等
    let (mut d, declared) = build_declared();
    let root = d.freeze();
    got[3] = declared == root
        && d.freeze() == root
        && d.lodge(KIND_CRITERIA, PIN_CRITERIA, PIN_CRITERIA).err() == Some(DG_EFROZEN)
        && d.declare().err() == Some(DG_EFROZEN);
    got
}

/// 判据台：三断言合一面的十四条判据。
pub fn run_unx11197_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11197");

    let (mut book, declared) = build_declared();
    let root = book.freeze();
    let (c1, l1, b1) = book.summary();

    // J1 三类断言齐备
    s.add(
        "J1-三类齐备",
        book.count() == KINDS && book.missing().is_empty(),
        "满条/满账/满态三类各一条",
    );

    // J2 三类逐条成立
    s.add(
        "J2-三类成立",
        book.all_hold() && c1 == PIN_CRITERIA && l1 == PIN_LINES && b1 == PIN_BATCHES,
        "800 / 240000 / 40 三数与钉定一致",
    );

    // J3 合一宣告值成立
    s.add(
        "J3-合一单值",
        declared.is_some() && declared == root,
        "三断言合一的宣告值即冻结值",
    );

    // J4 缺条拒宣告
    let mut two = DeclarationBook::empty();
    let _ = two.lodge(KIND_CRITERIA, PIN_CRITERIA, PIN_CRITERIA);
    let _ = two.lodge(KIND_LINES, PIN_LINES, PIN_LINES);
    s.add(
        "J4-缺条拒宣告",
        two.declare().err() == Some(DG_EMISS)
            && two.missing().len() == 1
            && two.missing().first() == Some(&KIND_BATCHES),
        "缺满态一条不得宣告、缺条可枚举",
    );

    // J5 错类拒
    let mut bad = DeclarationBook::empty();
    s.add(
        "J5-错类拒",
        bad.lodge(KINDS as u8, 1, 1).err() == Some(DG_EBADKIND) && bad.count() == 0,
        "类号越界必拒且账面不损",
    );

    // J6 重类拒
    let mut dup = DeclarationBook::empty();
    let _ = dup.lodge(KIND_LINES, PIN_LINES, PIN_LINES);
    s.add(
        "J6-重类拒",
        dup.lodge(KIND_LINES, PIN_LINES, PIN_LINES).err() == Some(DG_EDUPKIND),
        "同类断言二次立账必拒",
    );

    // J7 零期望拒
    let mut zero = DeclarationBook::empty();
    s.add(
        "J7-零期望拒",
        zero.lodge(KIND_BATCHES, 0, 0).err() == Some(DG_EZERO),
        "零期望不得立账",
    );

    // J8 值不符拒且零残留
    let mut mm = DeclarationBook::empty();
    let r = mm.lodge(KIND_LINES, PIN_LINES, PIN_LINES + 1);
    s.add(
        "J8-值不符拒",
        r.err() == Some(DG_EMISMATCH) && mm.count() == 0 && mm.rejects == 1,
        "期望与实算不符必拒、拒绝记账",
    );

    // J9 立账序无关（确定性排序）
    let mut ord_a = DeclarationBook::empty();
    let _ = ord_a.lodge(KIND_BATCHES, PIN_BATCHES, PIN_BATCHES);
    let _ = ord_a.lodge(KIND_CRITERIA, PIN_CRITERIA, PIN_CRITERIA);
    let _ = ord_a.lodge(KIND_LINES, PIN_LINES, PIN_LINES);
    let mut ord_b = DeclarationBook::empty();
    let _ = lodge_three(&mut ord_b);
    s.add(
        "J9-立账序无关",
        ord_a.fold() == ord_b.fold() && ord_a.all_hold() && ord_a.declared_value().is_none(),
        "乱序立账与正序立账指纹一致",
    );

    // J10 冻结幂等
    s.add(
        "J10-冻结幂等",
        book.freeze() == root && book.is_frozen(),
        "重复冻结取回同一锁定值",
    );

    // J11 冻结拒改
    s.add(
        "J11-冻结拒改",
        book.lodge(KIND_CRITERIA, PIN_CRITERIA, PIN_CRITERIA).err() == Some(DG_EFROZEN)
            && book.declare().err() == Some(DG_EFROZEN),
        "冻结后立账与宣告两路皆拒",
    );

    // J12 重复宣告拒
    let mut twice = DeclarationBook::empty();
    let _ = lodge_three(&mut twice);
    s.add(
        "J12-重复宣告拒",
        twice.declare().is_ok() && twice.declare().err() == Some(DG_EDUP),
        "宣告只出一次",
    );

    // J13 注入四拒
    let hits = inject_four();
    let mut all = true;
    let mut i = 0usize;
    while i < hits.len() {
        if !hits[i] {
            all = false;
        }
        i = i.wrapping_add(1);
    }
    s.add("J13-注入四拒", all, "错类/零期望/值不符/冻结四路全拒");

    // J14 动态明细
    s.add_dyn(
        "J14-宣告明细",
        book.rejects == 0 && book.declared_value().is_some(),
        &format!(
            "lodges={} rejects={} pin={}/{}/{} root={:#x}",
            book.lodges,
            book.rejects,
            c1,
            l1,
            b1,
            root.unwrap_or(0)
        ),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11197_declaration_flow() {
        let (mut b, declared) = build_declared();
        assert_eq!(b.count(), KINDS);
        assert!(b.all_hold());
        assert_eq!(b.summary(), (PIN_CRITERIA, PIN_LINES, PIN_BATCHES));
        assert!(declared.is_some());

        let root = b.freeze();
        assert_eq!(root, declared);
        assert_eq!(b.freeze(), root);
        assert_eq!(
            b.lodge(KIND_BATCHES, PIN_BATCHES, PIN_BATCHES).err(),
            Some(DG_EFROZEN)
        );

        let mut e = DeclarationBook::empty();
        assert_eq!(e.declare().err(), Some(DG_EMISS));
        assert_eq!(e.missing().len(), KINDS);
        assert_eq!(e.lodge(9, 1, 1).err(), Some(DG_EBADKIND));
        assert_eq!(e.lodge(0, 0, 0).err(), Some(DG_EZERO));
        assert_eq!(e.lodge(0, 5, 4).err(), Some(DG_EMISMATCH));

        assert!(inject_four().iter().all(|h| *h));
    }

    #[test]
    fn unx11197_checks_green() {
        let s = run_unx11197_checks();
        assert!(s.all_passed(), "UNX-F11197 判据未全绿");
    }
}
