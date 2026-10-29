//! UNX-F11105 演习面总对账（B35 三态终核）（0x1f585 段）
//!
//! 锚点：UNX-F11105-J1 演习总账终核对平（10/10），验收落点核销复断（10/10）。
//! 域：CoRun·UNX-C4-B36（演习面）。
//!
//! 结构：演习册（DrillFace：十场演习登记 → 三态落判 → 验收落点核销 →
//! 复断）+ 单场（DrillEntry：三态位 + 落点位 + 核销位）+ 冻结根 + 注入核
//! （红场核销/重复核销/零落点/落点重复四路必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1f585 独占 0x20）。
pub const SEG_BASE: u32 = 0x1f585;
pub const DF_BADID: u32 = SEG_BASE + 0x01; // 演习号为 0
pub const DF_DUP: u32 = SEG_BASE + 0x02; // 演习号重号
pub const DF_EMPTY: u32 = SEG_BASE + 0x03; // 场景名空
pub const DF_VERDICT: u32 = SEG_BASE + 0x04; // 非法三态值
pub const DF_RED: u32 = SEG_BASE + 0x05; // 红场不得核销
pub const DF_CLEARED: u32 = SEG_BASE + 0x06; // 重复核销
pub const DF_ANCHOR: u32 = SEG_BASE + 0x07; // 落点为零或重复
pub const DF_FROZEN: u32 = SEG_BASE + 0x08; // 冻结后写账

/// 演习场数（10）。
pub const NEEDED: usize = 10;
/// 演习册定容。
pub const CAP: usize = 14;
/// 三态值：通过 / 失败 / 豁免。
pub const V_PASS: u8 = 0;
pub const V_FAIL: u8 = 1;
pub const V_SKIP: u8 = 2;
/// 演习面标（fold 种子）。
pub const DF_TAG: u64 = 0x1f5_8585;
/// 指纹步进。
pub const FP_STEP: u64 = 0x100000001b3;

/// 单场演习：三态 + 验收落点 + 核销位。
#[derive(Clone, Debug, PartialEq)]
pub struct DrillEntry {
    pub id: u32,
    pub scenario: String,
    pub verdict: u8,
    pub anchor: u32,
    pub cleared: bool,
}

impl DrillEntry {
    pub fn passed(&self) -> bool {
        self.verdict == V_PASS
    }

    pub fn verdict_name(&self) -> &'static str {
        match self.verdict {
            V_PASS => "P",
            V_FAIL => "F",
            V_SKIP => "S",
            _ => "?",
        }
    }

    pub fn render(&self) -> String {
        format!("D{}|{}|a{}|{}", self.id, self.verdict_name(), self.anchor, self.cleared as u8)
    }
}

/// 演习册：十场 + 冻结根。
#[derive(Clone, Debug)]
pub struct DrillFace {
    pub entries: Vec<DrillEntry>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl DrillFace {
    pub fn new() -> DrillFace {
        DrillFace { entries: Vec::new(), frozen: false, root: None }
    }

    /// 演习登记：零号/重号/空名/落点重复或为零/超容皆拒。
    pub fn enroll(&mut self, id: u32, scenario: &str, anchor: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(DF_FROZEN);
        }
        if id == 0 {
            return Err(DF_BADID);
        }
        if scenario.is_empty() {
            return Err(DF_EMPTY);
        }
        if self.entries.iter().any(|e| e.id == id) {
            return Err(DF_DUP);
        }
        if self.entries.len() >= CAP {
            return Err(DF_ANCHOR);
        }
        if anchor == 0 || self.entries.iter().any(|e| e.anchor == anchor) {
            return Err(DF_ANCHOR);
        }
        self.entries.push(DrillEntry {
            id,
            scenario: scenario.to_string(),
            verdict: V_PASS,
            anchor,
            cleared: false,
        });
        Ok(())
    }

    fn find(&mut self, id: u32) -> Option<&mut DrillEntry> {
        let mut k = 0usize;
        while k < self.entries.len() {
            if self.entries[k].id == id {
                return Some(&mut self.entries[k]);
            }
            k += 1;
        }
        None
    }

    /// 三态落判：非法值拒。
    pub fn record(&mut self, id: u32, verdict: u8) -> Result<(), u32> {
        if self.frozen {
            return Err(DF_FROZEN);
        }
        if verdict > V_SKIP {
            return Err(DF_VERDICT);
        }
        let e = self.find(id).ok_or(DF_BADID)?;
        if e.cleared {
            return Err(DF_CLEARED);
        }
        e.verdict = verdict;
        Ok(())
    }

    /// 落点核销：仅通过场可核销，重复核销拒。
    pub fn clear(&mut self, id: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(DF_FROZEN);
        }
        let e = self.find(id).ok_or(DF_BADID)?;
        if !e.passed() {
            return Err(DF_RED);
        }
        if e.cleared {
            return Err(DF_CLEARED);
        }
        e.cleared = true;
        Ok(())
    }

    /// 三态合计（通过, 失败, 豁免）。
    pub fn tri(&self) -> (u32, u32, u32) {
        let mut p = 0u32;
        let mut f = 0u32;
        let mut s = 0u32;
        let mut k = 0usize;
        while k < self.entries.len() {
            match self.entries[k].verdict {
                V_PASS => p = p.wrapping_add(1),
                V_FAIL => f = f.wrapping_add(1),
                V_SKIP => s = s.wrapping_add(1),
                _ => f = f.wrapping_add(1),
            }
            k += 1;
        }
        (p, f, s)
    }

    /// 落点唯一：无零落点、无重复落点。
    pub fn anchors_unique(&self) -> bool {
        let mut k = 0usize;
        while k < self.entries.len() {
            if self.entries[k].anchor == 0 {
                return false;
            }
            let mut j = k + 1;
            while j < self.entries.len() {
                if self.entries[j].anchor == self.entries[k].anchor {
                    return false;
                }
                j += 1;
            }
            k += 1;
        }
        true
    }

    /// 未核销点名。
    pub fn uncleared(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.entries.len() {
            if !self.entries[k].cleared {
                v.push(self.entries[k].id);
            }
            k += 1;
        }
        v
    }

    /// 演习总账终核：十场齐、零失败、零豁免未决、落点唯一且全核销。
    pub fn reconciled(&self) -> bool {
        let (_, f, s) = self.tri();
        self.entries.len() == NEEDED
            && f == 0
            && s == 0
            && self.anchors_unique()
            && self.uncleared().is_empty()
    }

    /// 冻结并落根。
    pub fn seal(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }

    /// 册级指纹：演习号 + 三态 + 落点 + 核销位 + 场景名。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = DF_TAG;
        let mut k = 0usize;
        while k < self.entries.len() {
            let e = &self.entries[k];
            h = (h ^ e.id as u64).wrapping_mul(FP_STEP);
            h = (h ^ e.verdict as u64).wrapping_mul(FP_STEP);
            h = (h ^ e.anchor as u64).wrapping_mul(FP_STEP);
            h = (h ^ e.cleared as u64).wrapping_mul(FP_STEP);
            let bytes = e.scenario.as_bytes();
            let mut b = 0usize;
            while b < bytes.len() {
                h = (h ^ bytes[b] as u64).wrapping_mul(FP_STEP);
                b += 1;
            }
            k += 1;
        }
        h
    }
}

/// 场景名样本（确定性）。
pub fn scenario_name(id: u32) -> String {
    format!("B35 三态演习场 {}", id)
}

/// 验收落点样本：高位段 0x2110 起顺排。
pub fn anchor_of(id: u32) -> u32 {
    0x2110u32.wrapping_add(id)
}

/// 标准演习面：十场登记 → 全判通过 → 全落点核销。
pub fn build_face() -> (DrillFace, bool) {
    let mut f = DrillFace::new();
    let mut ok = true;
    let mut id = 1u32;
    while id <= NEEDED as u32 {
        if f.enroll(id, &scenario_name(id), anchor_of(id)).is_err() {
            ok = false;
        }
        if f.record(id, V_PASS).is_err() {
            ok = false;
        }
        if f.clear(id).is_err() {
            ok = false;
        }
        id += 1;
    }
    (f, ok)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 红场不得核销
    let mut a = DrillFace::new();
    let _ = a.enroll(1, "红场", anchor_of(1));
    let _ = a.record(1, V_FAIL);
    got[0] = a.clear(1).err() == Some(DF_RED) && a.uncleared().len() == 1;
    // 2) 重复核销必拒
    let mut b = DrillFace::new();
    let _ = b.enroll(2, "重销", anchor_of(2));
    let first = b.clear(2).is_ok();
    got[1] = first && b.clear(2).err() == Some(DF_CLEARED);
    // 3) 零落点必拒
    let mut c = DrillFace::new();
    got[2] = c.enroll(3, "零落点", 0).err() == Some(DF_ANCHOR);
    // 4) 落点重复必拒且唯一断言为假
    let mut d = DrillFace::new();
    let _ = d.enroll(4, "甲", anchor_of(4));
    let dup_reject = d.enroll(5, "乙", anchor_of(4)).err() == Some(DF_ANCHOR);
    let _ = d.enroll(6, "丙", anchor_of(6));
    got[3] = dup_reject && d.anchors_unique();
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11105_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11105");

    let (mut face, built) = build_face();
    // J1 演习总账终核对平
    s.add(
        "J1-演习总账对平",
        built && face.entries.len() == NEEDED && face.reconciled(),
        "十场演习登记、落判、核销三面对平",
    );

    // J2 验收落点核销复断
    s.add(
        "J2-落点核销复断",
        face.anchors_unique() && face.uncleared().is_empty(),
        "十个落点唯一且逐点核销",
    );

    // J3 三态和与通过数对平
    let (p, f, sk) = face.tri();
    s.add(
        "J3-三态和对平",
        p == 10 && f == 0 && sk == 0 && p.wrapping_add(f).wrapping_add(sk) == NEEDED as u32,
        "十场全通过且三态和恰为十",
    );

    // J4 红场核销必拒
    let j4 = {
        let mut c = DrillFace::new();
        let _ = c.enroll(1, "红", anchor_of(1));
        let _ = c.record(1, V_FAIL);
        c.clear(1).err() == Some(DF_RED)
    };
    s.add("J4-红场拒核销", j4, "失败场核销必拒");

    // J5 豁免场核销必拒
    let j5 = {
        let mut c = DrillFace::new();
        let _ = c.enroll(1, "豁", anchor_of(1));
        let _ = c.record(1, V_SKIP);
        c.clear(1).err() == Some(DF_RED)
    };
    s.add("J5-豁免拒核销", j5, "豁免场不得以通过论核销");

    // J6 重复核销拒
    let j6 = {
        let mut c = DrillFace::new();
        let _ = c.enroll(1, "重", anchor_of(1));
        let _ = c.clear(1);
        c.clear(1).err() == Some(DF_CLEARED)
    };
    s.add("J6-重复核销拒", j6, "同场二次核销必拒");

    // J7 非法三态值拒
    let j7 = {
        let mut c = DrillFace::new();
        let _ = c.enroll(1, "非", anchor_of(1));
        c.record(1, 7).err() == Some(DF_VERDICT)
    };
    s.add("J7-非法三态拒", j7, "三态值大于 2 必拒");

    // J8 重号与空名拒
    let j8 = {
        let mut c = DrillFace::new();
        let _ = c.enroll(1, "甲", anchor_of(1));
        let d = c.enroll(1, "乙", anchor_of(2)).err() == Some(DF_DUP);
        let e = c.enroll(2, "", anchor_of(2)).err() == Some(DF_EMPTY);
        d && e
    };
    s.add("J8-重号空名拒", j8, "演习重号与空名两路必拒");

    // J9 注入检出：红场核销 + 重复核销
    let inj = inject_four();
    s.add("J9-红场重销检出", inj[0] && inj[1], "红场核销与重复核销两路必抓");

    // J10 注入检出：零落点 + 落点重复
    s.add("J10-落点异常检出", inj[2] && inj[3], "零落点与落点重复两路必抓");

    // J11 未清不判绿
    let j11 = {
        let mut c = DrillFace::new();
        let mut id = 1u32;
        while id <= NEEDED as u32 {
            let _ = c.enroll(id, &scenario_name(id), anchor_of(id));
            let _ = c.record(id, V_PASS);
            if id != 5 {
                let _ = c.clear(id);
            }
            id += 1;
        }
        !c.reconciled() && c.uncleared().len() == 1 && c.uncleared().contains(&5)
    };
    s.add("J11-未清不判绿", j11, "落点核销后清单为空");

    // J12 冻结拒写
    let root = face.seal();
    let j12 = face.record(1, V_PASS).err() == Some(DF_FROZEN)
        && face.clear(1).err() == Some(DF_FROZEN)
        && face.enroll(11, "追场", anchor_of(11)).err() == Some(DF_FROZEN);
    s.add("J12-冻结拒写", j12, "封存后落判/核销/登记三路必拒");

    // J13 冻结根稳定
    s.add(
        "J13-冻结根稳定",
        face.root == Some(root) && root == face.fold() && root != 0,
        "演习册冻结根可重算零漂移",
    );

    // J14 渲染面
    let j14 = {
        let (c, _) = build_face();
        c.entries[0].render().contains("|P|") && c.entries[0].render().contains("a8465")
    };
    s.add("J14-演习渲染", j14, "通过场渲染带 P 旗与落点号");

    // J15 动态明细
    s.add_dyn(
        "J15-演习明细",
        face.reconciled() && face.fold() == root,
        &format!("p={} f={} s={} anchors={} root={:#x}", p, f, sk, face.entries.len(), root),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11105_drill_flow() {
        let (mut f, built) = build_face();
        assert!(built);
        assert!(f.reconciled());
        assert!(f.tri() == (10, 0, 0));
        assert!(f.seal() != 0);
        assert!(f.clear(1).err() == Some(DF_FROZEN));
        assert!(f.record(1, V_PASS).err() == Some(DF_FROZEN));
    }

    #[test]
    fn unx11105_checks_green() {
        let s = run_unx11105_checks();
        assert!(s.all_passed(), "UNX-F11105 判据未全绿");
    }
}
