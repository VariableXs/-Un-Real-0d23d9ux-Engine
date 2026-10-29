//! UNX-F12072 TEB 静态布置与线程创建交接（0x19b20 段）
//!
//! 卡面：线程创建时 TEB 静态布置的交接清单——八项字段必须逐项备妥
//! （Self/StackBase/StackLimit/ClientId/EnvironmentPointer/CurrentLocale/
//!   LastErrorValue/ActivationContextStackPointer）后线程方可起跑。
//! 域：CoRun·UNX-D1-B04。
//!
//! 判据原文：
//! - J1 八项闭集：交接清单恰为八个槽位，编号闭集 [0,8)。
//! - J2 单臂原则：同槽位二次备妥必拒。
//! - J3 指针类字段非零：Self/StackBase/StackLimit/EnvironmentPointer/
//!   ActivationContextStackPointer 五项备妥值不得为零。
//! - J4 起跑门禁：八项未齐时 start 必拒，齐备后放行。
//! - J5 值失配：指针项无对齐约束但值账逐项可回读。
//! - J6 印账冻结：seal 后备妥与起跑皆拒。
//! - J7 注入五况全抓。
//!
//! 结构：HandoverField 八项枚举 + TebHandover 位图状态机 + 备妥账 +
//! 折叠指纹（211 进制卷积）+ 注入核（双臂/坏槽位/零指针值/缺项起跑/印后臂）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19b20 独占 0x20）。
pub const SEG_BASE: u32 = 0x19b20;
pub const TH_DOUBLE_ARM: u32 = SEG_BASE + 0x01; // 双臂
pub const TH_UNKNOWN_IDX: u32 = SEG_BASE + 0x02; // 坏槽位
pub const TH_ZERO_PTR: u32 = SEG_BASE + 0x03; // 指针项零值
pub const TH_MISSING: u32 = SEG_BASE + 0x04; // 缺项起跑
pub const TH_FROZEN: u32 = SEG_BASE + 0x05; // 印账后臂

/// 交接清单八项。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HandoverField {
    SelfPtr = 0,
    StackBase = 1,
    StackLimit = 2,
    ClientId = 3,
    EnvironmentPointer = 4,
    CurrentLocale = 5,
    LastErrorValue = 6,
    ActCtxStackPtr = 7,
}

impl HandoverField {
    pub fn idx(&self) -> u32 {
        *self as u32
    }
    /// 指针类五项：备妥值不得为零。
    pub fn is_pointer(&self) -> bool {
        matches!(
            self,
            HandoverField::SelfPtr
                | HandoverField::StackBase
                | HandoverField::StackLimit
                | HandoverField::EnvironmentPointer
                | HandoverField::ActCtxStackPtr
        )
    }
}

/// 一次备妥的留痕。
#[derive(Clone)]
pub struct ArmRec {
    pub slot: u32,
    pub value: u64,
    pub note: String,
}

impl ArmRec {
    pub fn render(&self) -> String {
        format!("F{}|{:x}", self.slot, self.value)
    }
}

/// TEB 交接状态机：位图 + 备妥账 + 起跑门禁。
pub struct TebHandover {
    pub armed: u16, // 位图：bit i = 槽位 i 已备妥
    pub recs: Vec<ArmRec>,
    pub started: bool,
    pub sealed: bool,
}

impl TebHandover {
    pub const ALL: u16 = 0xFF;

    pub fn new() -> TebHandover {
        TebHandover { armed: 0, recs: Vec::new(), started: false, sealed: false }
    }

    /// 备妥一槽：单臂、槽位闭集、指针类零值拒。
    pub fn arm(&mut self, slot: u32, value: u64, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(TH_FROZEN);
        }
        if slot >= 8 {
            return Err(TH_UNKNOWN_IDX);
        }
        if self.started {
            return Err(TH_DOUBLE_ARM); // 起跑后不得再臂
        }
        if HandoverField::SelfPtr.idx() <= slot
            && slot <= HandoverField::ActCtxStackPtr.idx()
        {
            let f = HandoverField::SelfPtr;
            let _ = f; // 编号线性，is_pointer 以显式表判定
            if slot_is_pointer(slot) && value == 0 {
                return Err(TH_ZERO_PTR);
            }
        }
        let bit = 1u16 << slot;
        if self.armed & bit != 0 {
            return Err(TH_DOUBLE_ARM);
        }
        self.armed |= bit;
        self.recs.push(ArmRec { slot, value, note: note.to_string() });
        Ok(())
    }

    /// 起跑门禁：八项齐备方放行。
    pub fn start(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(TH_FROZEN);
        }
        if self.armed != TebHandover::ALL {
            return Err(TH_MISSING);
        }
        self.started = true;
        Ok(())
    }

    /// 缺项清单：未备妥槽位号收集。
    pub fn missing_slots(&self) -> Vec<u32> {
        let mut v = Vec::new();
        for i in 0..8u32 {
            if self.armed & (1u16 << i) == 0 {
                v.push(i);
            }
        }
        v
    }

    /// 值账回读：某槽位备妥值。
    pub fn value_of(&self, slot: u32) -> Option<u64> {
        self.recs.iter().find(|r| r.slot == slot).map(|r| r.value)
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        h = h.wrapping_mul(211).wrapping_add(self.armed as u32);
        for r in &self.recs {
            h = h.wrapping_mul(211).wrapping_add(r.slot);
            h = h.wrapping_mul(211).wrapping_add((r.value & 0xFFFF_FFFF) as u32);
            h = h.wrapping_mul(211).wrapping_add((r.value >> 32) as u32);
            for by in r.note.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：备妥项数。
    pub fn armed_count(&self) -> u32 {
        self.armed.count_ones()
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("armed={:02x} started={}", self.armed, self.started)
    }

    /// 诊断面：留痕渲染行。
    pub fn rec_line(&self, i: usize) -> String {
        match self.recs.get(i) {
            Some(r) => r.render(),
            None => "missing".to_string(),
        }
    }
}

/// 指针类槽位显式表（与 HandoverField::is_pointer 对应）。
fn slot_is_pointer(slot: u32) -> bool {
    matches!(slot, 0 | 1 | 2 | 4 | 7)
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut h = TebHandover::new();
    // 1) 坏槽位必拒。
    got[0] = h.arm(99, 1, "bad").err() == Some(TH_UNKNOWN_IDX);
    // 2) 指针项零值必拒。
    got[1] = h.arm(0, 0, "zero").err() == Some(TH_ZERO_PTR);
    // 3) 双臂必拒。
    let _ = h.arm(0, 0x7efdd000, "a");
    got[2] = h.arm(0, 0x7efdd000, "b").err() == Some(TH_DOUBLE_ARM);
    // 4) 缺项起跑必拒。
    got[3] = h.start().err() == Some(TH_MISSING);
    // 5) 印账后备妥必抓。
    h.seal();
    got[4] = h.arm(1, 0x7efdb000, "post").err() == Some(TH_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12072_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12072");

    // J1 八项闭集：全部臂齐后位图满格。
    let j1 = {
        let mut h = TebHandover::new();
        for i in 0..8u32 {
            let _ = h.arm(i, 0x1000 + i as u64, "fill");
        }
        h.armed == TebHandover::ALL && h.armed_count() == 8
    };
    s.add("J1-八项闭集", j1, "位图满格");

    // J2 单臂原则。
    let j2 = {
        let mut h = TebHandover::new();
        let _ = h.arm(3, 0x0409, "a");
        h.arm(3, 0x0409, "b").err() == Some(TH_DOUBLE_ARM)
    };
    s.add("J2-单臂", j2, "同槽二次必拒");

    // J3 指针类零值拒。
    let j3 = {
        let mut h = TebHandover::new();
        h.arm(1, 0, "zero").err() == Some(TH_ZERO_PTR)
            && h.arm(6, 0, "ok").is_ok() // 非指针项可零
    };
    s.add("J3-指针非零", j3, "指针类零值必拒");

    // J4 起跑门禁：齐备放行、缺项拒并回读缺项清单。
    let j4 = {
        let mut h = TebHandover::new();
        h.start().err() == Some(TH_MISSING) && h.missing_slots().len() == 8
    };
    s.add("J4-起跑门禁", j4, "全缺时拒且清单八项");

    // J5 全备后起跑放行且值可回读。
    let j5 = {
        let mut h = TebHandover::new();
        for i in 0..8u32 {
            let _ = h.arm(i, 0x2000 + i as u64, "f");
        }
        h.start().is_ok() && h.started && h.value_of(2) == Some(0x2002)
    };
    s.add("J5-起跑放行", j5, "齐备起跑且值可回读");

    // J6 起跑后再臂必拒。
    let j6 = {
        let mut h = TebHandover::new();
        for i in 0..8u32 {
            let _ = h.arm(i, 0x100, "f");
        }
        let _ = h.start();
        h.arm(0, 0x100, "late").err() == Some(TH_DOUBLE_ARM)
    };
    s.add("J6-起跑后禁臂", j6, "起跑后必拒");

    // J7 印账冻结。
    let j7 = {
        let mut h = TebHandover::new();
        let _ = h.arm(0, 0x1000, "a");
        h.seal();
        h.arm(1, 0x1000, "post").err() == Some(TH_FROZEN)
            && h.start().err() == Some(TH_FROZEN)
    };
    s.add("J7-印账冻结", j7, "seal 后皆拒");

    // J8 指纹幂等。
    let j8 = {
        let mut h = TebHandover::new();
        let _ = h.arm(0, 0x1000, "a");
        h.fold() == h.fold()
    };
    s.add("J8-指纹幂等", j8, "同态同指纹");

    // J9 注入核 5/5。
    let got = inject_five();
    let j9 = got.iter().all(|x| *x);
    s.add_dyn("J9-注入全抓", j9, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut h = TebHandover::new();
    let _ = h.arm(0, 0x7efdd000, "self");
    let _ = h.arm(5, 0x0409_0409, "locale");

    s.add("摘要回读", h.summary().contains("armed=21"), "两位已臂");
    s.add("留痕渲染", h.rec_line(0).contains("7efdd000"), "Self 行含基址");
    s.add("值回读", h.value_of(5) == Some(0x0409_0409), "locale 值可回读");
    s.add("指针表", slot_is_pointer(7) && !slot_is_pointer(5), "指针类显性表");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12072_handover_flow() {
        let mut h = TebHandover::new();
        assert_eq!(h.arm(8, 1, "x"), Err(TH_UNKNOWN_IDX));
        assert!(h.arm(0, 0x7efdd000, "self").is_ok());
        assert_eq!(h.arm(0, 0x7efdd000, "again"), Err(TH_DOUBLE_ARM));
        assert_eq!(h.start(), Err(TH_MISSING));
    }

    #[test]
    fn unx12072_checks_green() {
        let s = run_unx12072_checks();
        assert!(s.all_passed(), "UNX-F12072 判据未全绿");
    }

    #[test]
    fn field_pointer_classification() {
        assert!(HandoverField::StackBase.is_pointer());
        assert!(!HandoverField::CurrentLocale.is_pointer());
        assert_eq!(HandoverField::LastErrorValue.idx(), 6);
    }
}
