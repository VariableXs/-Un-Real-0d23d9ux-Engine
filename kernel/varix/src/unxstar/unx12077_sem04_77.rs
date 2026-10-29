//! UNX-F12077 EnvironmentPointer/ArbitraryUserPointer 语义（0x19bc0 段）
//!
//! 卡面：TEB→EnvironmentPointer 与 ArbitraryUserPointer——两枚用户侧
//! 指针字段：值域/对齐/侧别三分约束，字段一经登记不可改写。
//! 域：CoRun·UNX-D1-B04。
//!
//! 判据原文：
//! - J1 用户值域：用户侧指针须落 [0x10000, 0x7FFE0000)。
//! - J2 对齐约束：指针值 8 字节对齐。
//! - J3 侧别一致：内核段值（≥0x80000000）不得登记进用户侧字段。
//! - J4 字段单写：同一字段二次登记必拒。
//! - J5 双字段独立：EnvironmentPointer 与 ArbitraryUserPointer 各自记账
//!   互不串位。
//! - J6 印账冻结：seal 后登记必拒。
//! - J7 注入五况全抓。
//!
//! 结构：PointerKind 双字段枚举 + TebPointer 登记 + 指针账本 + 折叠指纹 +
//! 注入核（值域外/未对齐/侧别冲突/双写/印后登记）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19bc0 独占 0x20）。
pub const SEG_BASE: u32 = 0x19bc0;
pub const PT_BAD_RANGE: u32 = SEG_BASE + 0x01; // 值域外
pub const PT_UNALIGNED: u32 = SEG_BASE + 0x02; // 未对齐
pub const PT_SIDE_CONFLICT: u32 = SEG_BASE + 0x03; // 侧别冲突
pub const PT_DUP_FIELD: u32 = SEG_BASE + 0x04; // 字段双写
pub const PT_FROZEN: u32 = SEG_BASE + 0x05; // 印账后登记

/// 用户值域边界。
pub const USER_LO: u32 = 0x10000;
pub const USER_HI: u32 = 0x7FFE0000;

/// 指针字段枚举。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PointerKind {
    Environment = 1,
    ArbitraryUser = 2,
}

/// 一次指针登记。
#[derive(Clone, Copy)]
pub struct TebPointer {
    pub kind: PointerKind,
    pub value: u32,
}

impl TebPointer {
    /// 用户侧合法性：值域 + 对齐。
    pub fn user_ok(&self) -> bool {
        self.value >= USER_LO && self.value < USER_HI && self.value % 8 == 0
    }
    /// 内核段值判定。
    pub fn kernel_ranged(&self) -> bool {
        self.value >= 0x8000_0000
    }
    pub fn render(&self) -> String {
        format!("P{}|{:08x}", self.kind as u16, self.value)
    }
}

/// 指针账本：双字段各一槽，单写。
pub struct PointerBook {
    pub slots: Vec<TebPointer>,
    pub sealed: bool,
}

impl PointerBook {
    pub fn new() -> PointerBook {
        PointerBook { slots: Vec::new(), sealed: false }
    }

    fn slot_of(&self, kind: PointerKind) -> Option<usize> {
        self.slots.iter().position(|p| p.kind == kind)
    }

    /// 登记：值域 → 对齐 → 侧别 → 单写。
    pub fn register(&mut self, p: TebPointer) -> Result<(), u32> {
        if self.sealed {
            return Err(PT_FROZEN);
        }
        // 侧别门先于值域门：内核段值（≥0x80000000）必然越出用户值域上界，
        // 若值域门先行则 PT_SIDE_CONFLICT 成死分支（J3 承诺独立分码）。
        if p.kernel_ranged() {
            return Err(PT_SIDE_CONFLICT);
        }
        if p.value < USER_LO || p.value >= USER_HI {
            return Err(PT_BAD_RANGE);
        }
        if p.value % 8 != 0 {
            return Err(PT_UNALIGNED);
        }
        if self.slot_of(p.kind).is_some() {
            return Err(PT_DUP_FIELD);
        }
        self.slots.push(p);
        Ok(())
    }

    /// 字段值回读。
    pub fn value_of(&self, kind: PointerKind) -> Option<u32> {
        self.slot_of(kind).and_then(|i| Some(self.slots[i].value))
    }

    /// 已登记字段数。
    pub fn count(&self) -> usize {
        self.slots.len()
    }

    /// 双字段齐备。
    pub fn both_set(&self) -> bool {
        self.count() == 2
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for p in &self.slots {
            h = h.wrapping_mul(211).wrapping_add(p.kind as u32);
            h = h.wrapping_mul(211).wrapping_add(p.value);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("ptrs={}", self.slots.len())
    }

    /// 诊断面：渲染行。
    pub fn slot_line(&self, kind: PointerKind) -> String {
        match self.slot_of(kind) {
            Some(i) => self.slots[i].render(),
            None => "missing".to_string(),
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = PointerBook::new();
    // 1) 值域下界外必拒。
    got[0] = b.register(TebPointer { kind: PointerKind::Environment, value: 0x8000 })
        .err() == Some(PT_BAD_RANGE);
    // 2) 值域上界外必拒。
    got[1] = b.register(TebPointer { kind: PointerKind::Environment, value: 0x7FFE0000 })
        .err() == Some(PT_BAD_RANGE);
    // 3) 未对齐必拒。
    got[2] = b.register(TebPointer { kind: PointerKind::Environment, value: 0x10004 })
        .err() == Some(PT_UNALIGNED);
    // 4) 侧别冲突：内核段值入用户字段必拒。
    got[3] = b.register(TebPointer { kind: PointerKind::Environment, value: 0x8100_0008 })
        .err() == Some(PT_SIDE_CONFLICT);
    // 5) 双写必拒（先正登记一次，再二次登记）。
    let _ = b.register(TebPointer { kind: PointerKind::Environment, value: 0x0040_0000 });
    got[4] = b.register(TebPointer { kind: PointerKind::Environment, value: 0x0050_0000 })
        .err() == Some(PT_DUP_FIELD);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12077_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12077");

    // J1 用户值域：上下界双门。
    let j1 = {
        let mut b = PointerBook::new();
        b.register(TebPointer { kind: PointerKind::Environment, value: USER_LO - 8 })
            .err() == Some(PT_BAD_RANGE)
            && b.register(TebPointer { kind: PointerKind::Environment, value: USER_HI })
                .err() == Some(PT_BAD_RANGE)
    };
    s.add("J1-值域双门", j1, "上下界外皆拒");

    // J2 对齐约束。
    let j2 = {
        let mut b = PointerBook::new();
        b.register(TebPointer { kind: PointerKind::ArbitraryUser, value: 0x2000_0001 })
            .err() == Some(PT_UNALIGNED)
    };
    s.add("J2-对齐约束", j2, "8 字节未对齐必拒");

    // J3 侧别冲突。
    let j3 = {
        let mut b = PointerBook::new();
        b.register(TebPointer { kind: PointerKind::Environment, value: 0xFFFF_FFF8 })
            .err() == Some(PT_SIDE_CONFLICT)
    };
    s.add("J3-侧别冲突", j3, "内核段值入用户字段必拒");

    // J4 字段单写。
    let j4 = {
        let mut b = PointerBook::new();
        let _ = b.register(TebPointer { kind: PointerKind::ArbitraryUser, value: 0x3000_0000 });
        b.register(TebPointer { kind: PointerKind::ArbitraryUser, value: 0x3000_0008 })
            .err() == Some(PT_DUP_FIELD)
    };
    s.add("J4-字段单写", j4, "二次登记必拒");

    // J5 双字段独立记账。
    let j5 = {
        let mut b = PointerBook::new();
        let _ = b.register(TebPointer { kind: PointerKind::Environment, value: 0x1000_0000 });
        let _ = b.register(TebPointer { kind: PointerKind::ArbitraryUser, value: 0x2000_0000 });
        b.both_set()
            && b.value_of(PointerKind::Environment) == Some(0x1000_0000)
            && b.value_of(PointerKind::ArbitraryUser) == Some(0x2000_0000)
    };
    s.add("J5-双字段独立", j5, "各自回读互不串位");

    // J6 正路径用户指针合法。
    let j6 = {
        let p = TebPointer { kind: PointerKind::Environment, value: 0x1000_0000 };
        p.user_ok() && !p.kernel_ranged() && p.render().contains("10000000")
    };
    s.add("J6-用户指针合法", j6, "值域对齐双过");

    // J7 印账冻结。
    let j7 = {
        let mut b = PointerBook::new();
        b.seal();
        b.register(TebPointer { kind: PointerKind::Environment, value: 0x1000_0000 })
            .err() == Some(PT_FROZEN)
    };
    s.add("J7-印账冻结", j7, "seal 后必拒");

    // J8 指纹幂等。
    let j8 = {
        let mut b = PointerBook::new();
        let _ = b.register(TebPointer { kind: PointerKind::Environment, value: 0x1000_0000 });
        b.fold() == b.fold()
    };
    s.add("J8-指纹幂等", j8, "同态同指纹");

    // J9 注入核 5/5。
    let got = inject_five();
    let j9 = got.iter().all(|x| *x);
    s.add_dyn("J9-注入全抓", j9, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut b = PointerBook::new();
    let _ = b.register(TebPointer { kind: PointerKind::Environment, value: 0x1000_0000 });

    s.add("摘要回读", b.summary().contains("ptrs=1"), "单字段在账");
    s.add("渲染行", b.slot_line(PointerKind::Environment).contains("P1"), "字段号 1 渲染");
    s.add("缺席行", b.slot_line(PointerKind::ArbitraryUser) == "missing", "未登记显性");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12077_register_flow() {
        let mut b = PointerBook::new();
        assert_eq!(
            b.register(TebPointer { kind: PointerKind::Environment, value: 0x1000 }),
            Err(PT_BAD_RANGE)
        );
        assert!(b.register(TebPointer { kind: PointerKind::Environment, value: 0x1000_0000 }).is_ok());
        assert_eq!(b.count(), 1);
    }

    #[test]
    fn unx12077_checks_green() {
        let s = run_unx12077_checks();
        assert!(s.all_passed(), "UNX-F12077 判据未全绿");
    }

    #[test]
    fn pointer_helpers() {
        let k = TebPointer { kind: PointerKind::ArbitraryUser, value: 0x9000_0000 };
        assert!(k.kernel_ranged());
        assert!(!k.user_ok());
    }
}
