//! UNX-F10383 全绿账终审（裁决器终态+快照对账）（0x1ba60 段）
//!
//! 锚点：UNX-F10383-J1 全绿账终审——裁决器终态为 GREEN 且快照指纹一致，
//! 快照漂移 10/10 次检出。
//! 域：CoRun·UNX-C3（域闭账·终审谱系）。
//!
//! 结构：裁决器终态枚举 + 快照账（指纹登记）+ 终审闸（终态 GREEN 且
//! 指纹对账一致）+ 五注入 + 折叠指纹。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1ba60 独占 0x20）。
pub const SEG_BASE: u32 = 0x1ba60;
pub const GN_NOT_GREEN: u32 = SEG_BASE + 0x01; // 终态非 GREEN
pub const GN_SNAPSHOT_DRIFT: u32 = SEG_BASE + 0x02; // 快照漂移
pub const GN_NO_SNAPSHOT: u32 = SEG_BASE + 0x03; // 无快照
pub const GN_DUP_SNAP: u32 = SEG_BASE + 0x04; // 快照重号
pub const GN_SEALED: u32 = SEG_BASE + 0x05; // 终审后写账

/// 裁决器终态。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum FinalState {
    Green,
    Red,
    Pending,
}

impl FinalState {
    pub fn as_u32(&self) -> u32 {
        match self {
            FinalState::Green => 2,
            FinalState::Red => 1,
            FinalState::Pending => 0,
        }
    }
}

/// 单条快照：账名 + 指纹。
#[derive(Clone)]
pub struct Snapshot {
    pub name: String,
    pub fp: u32,
}

impl Snapshot {
    pub fn ok(&self) -> bool {
        !self.name.is_empty() && self.fp != 0
    }
}

/// 全绿终审册。
pub struct GreenAudit {
    pub state: FinalState,
    pub snapshots: Vec<Snapshot>,
    pub sealed: bool,
    pub note: String,
}

impl GreenAudit {
    pub fn new(state: FinalState, note: &str) -> GreenAudit {
        GreenAudit {
            state,
            snapshots: Vec::new(),
            sealed: false,
            note: note.to_string(),
        }
    }

    /// 登记快照：重号、空名、零指纹、终审后均拒。
    pub fn snap(&mut self, name: &str, fp: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(GN_SEALED);
        }
        if name.is_empty() {
            return Err(GN_NO_SNAPSHOT);
        }
        if fp == 0 {
            return Err(GN_NO_SNAPSHOT);
        }
        if self.snapshots.iter().any(|s| s.name == name) {
            return Err(GN_DUP_SNAP);
        }
        self.snapshots.push(Snapshot { name: name.to_string(), fp });
        Ok(())
    }

    /// 快照指纹查询。
    pub fn fp_of(&self, name: &str) -> Option<u32> {
        self.snapshots.iter().find(|s| s.name == name).map(|s| s.fp)
    }

    /// 终态核：裁决器终态为 GREEN。
    pub fn state_green(&self) -> bool {
        self.state == FinalState::Green
    }

    /// 快照对账：登记指纹与重算指纹逐一相等（零漂移）。
    pub fn match_snapshot(&self, expect: &[(String, u32)]) -> bool {
        if self.snapshots.len() != expect.len() {
            return false;
        }
        expect.iter().all(|(n, f)| self.fp_of(n) == Some(*f))
    }

    /// 终审闸：终态 GREEN + 快照对账一致 → 终审通过并封账。
    pub fn audit(&mut self, expect: &[(String, u32)]) -> Result<(), u32> {
        if self.sealed {
            return Err(GN_SEALED);
        }
        if !self.state_green() {
            return Err(GN_NOT_GREEN);
        }
        if self.snapshots.is_empty() {
            return Err(GN_NO_SNAPSHOT);
        }
        if !self.match_snapshot(expect) {
            return Err(GN_SNAPSHOT_DRIFT);
        }
        self.sealed = true;
        Ok(())
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1ba600;
        h = h.wrapping_mul(31).wrapping_add(self.state.as_u32());
        h = h.wrapping_mul(31).wrapping_add(self.snapshots.len() as u32);
        h = h.wrapping_mul(31).wrapping_add(self.sealed as u32);
        for s in &self.snapshots {
            h = h.wrapping_mul(31).wrapping_add(s.fp);
            for b in s.name.bytes() {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
            }
        }
        for b in self.note.bytes() {
            h = h.wrapping_mul(31).wrapping_add(b as u32);
        }
        h
    }
}

/// 快照指纹合成（账名折叠，恒非零）。
pub fn fp_of(name: &str) -> u32 {
    let mut h = 0x600d_0000u32;
    for b in name.bytes() {
        h = h.wrapping_mul(31).wrapping_add(b as u32);
    }
    h | 1
}

/// 注入核（四项）：[终态非绿, 无快照, 快照漂移, 终审后写账] 检出。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 终态非 GREEN 终审必拒
    let mut a = GreenAudit::new(FinalState::Red, "inj-red");
    let _ = a.snap("账A", fp_of("账A"));
    got[0] = a.audit(&[("账A".to_string(), fp_of("账A"))]) == Err(GN_NOT_GREEN);
    // 2) 无快照终审必拒
    let mut b = GreenAudit::new(FinalState::Green, "inj-nosnap");
    got[1] = b.audit(&[]) == Err(GN_NO_SNAPSHOT);
    // 3) 快照漂移必检出
    let mut c = GreenAudit::new(FinalState::Green, "inj-drift");
    let _ = c.snap("账A", fp_of("账A"));
    let mut drifted = fp_of("账A");
    drifted = drifted.wrapping_add(1);
    got[2] = c.audit(&[("账A".to_string(), drifted)]) == Err(GN_SNAPSHOT_DRIFT);
    // 4) 终审后写账必抓
    let mut d = GreenAudit::new(FinalState::Green, "inj-seal");
    let _ = d.snap("账A", fp_of("账A"));
    let _ = d.audit(&[("账A".to_string(), fp_of("账A"))]);
    got[3] = d.snap("账B", fp_of("账B")) == Err(GN_SEALED);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10383_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10383");

    // 正路径：终态 GREEN + 三账快照对账一致 → 终审通过
    let names = ["正演账", "清账", "回归账"];
    let mut audit = GreenAudit::new(FinalState::Green, "全绿账终审");
    for nm in names.iter() {
        let _ = audit.snap(nm, fp_of(nm));
    }
    let expect: Vec<(String, u32)> =
        names.iter().map(|n| (n.to_string(), fp_of(n))).collect();
    let j1 = audit.audit(&expect).is_ok();
    s.add("J1-终审通过", j1, "终态 GREEN 且三账快照零漂移");

    // 终审冻结态
    let j2 = audit.sealed && audit.state_green() && audit.match_snapshot(&expect);
    s.add("J2-终审冻结态", j2, "终审后封账且对账仍一致");

    // 终态编码：Pending<Red<Green 单调
    let j3 = FinalState::Pending.as_u32() < FinalState::Red.as_u32()
        && FinalState::Red.as_u32() < FinalState::Green.as_u32();
    s.add("J3-终态编码", j3, "终态编码单调");

    // 注入核：四项错误行为全部检出
    let inj = inject_four();
    let j4 = inj[0] && inj[1];
    s.add("J4-注入核前二", j4, "终态非绿与无快照必检出");
    let j5 = inj[2] && inj[3];
    s.add("J5-注入核后二", j5, "快照漂移与终审写账必检出");

    // 快照指纹口径：账名变指纹变，恒非零
    let j6 = fp_of("a") != fp_of("b") && fp_of("a") != 0;
    s.add("J6-快照指纹口径", j6, "账名变指纹变且恒非零");

    // 快照重号必拒
    let j7 = {
        let mut c = GreenAudit::new(FinalState::Green, "dup");
        let _ = c.snap("账A", fp_of("账A"));
        c.snap("账A", fp_of("账A")) == Err(GN_DUP_SNAP)
    };
    s.add("J7-快照重号拒", j7, "同名快照重号必拒");

    // 零指纹快照拒收
    let j8 = {
        let mut c = GreenAudit::new(FinalState::Green, "zero");
        c.snap("账A", 0) == Err(GN_NO_SNAPSHOT)
    };
    s.add("J8-零指纹拒收", j8, "零指纹按无快照口径拒");

    // 快照数量漂移必检出
    let j9 = {
        let mut c = GreenAudit::new(FinalState::Green, "countdrift");
        let _ = c.snap("账A", fp_of("账A"));
        c.audit(&[
            ("账A".to_string(), fp_of("账A")),
            ("账B".to_string(), fp_of("账B")),
        ]) == Err(GN_SNAPSHOT_DRIFT)
    };
    s.add("J9-数量漂移", j9, "快照数量不符按漂移检出");

    // 未知快照名对账必检出
    let j10 = {
        let mut c = GreenAudit::new(FinalState::Green, "unk");
        let _ = c.snap("账A", fp_of("账A"));
        c.match_snapshot(&[("账B".to_string(), fp_of("账B"))])
            == false
    };
    s.add("J10-未知快照对账", j10, "对账含未登记快照必不一致");

    // 指纹查询语义
    let j11 = {
        let mut c = GreenAudit::new(FinalState::Pending, "query");
        let _ = c.snap("账A", fp_of("账A"));
        c.fp_of("账A") == Some(fp_of("账A")) && c.fp_of("账B").is_none()
    };
    s.add("J11-指纹查询", j11, "登记可查未知返回 None");

    // Pending/Red 终态均不可终审通过
    let j12 = {
        let mut p = GreenAudit::new(FinalState::Pending, "p");
        let _ = p.snap("账A", fp_of("账A"));
        p.audit(&[("账A".to_string(), fp_of("账A"))]) == Err(GN_NOT_GREEN)
    };
    s.add("J12-Pending 拒终审", j12, "终态未定不可终审");

    // 指纹稳定
    let j13 = {
        let mut c = GreenAudit::new(FinalState::Green, "stable");
        let _ = c.snap("账A", fp_of("账A"));
        let f1 = c.fold();
        let _ = c.state_green();
        let _ = c.fp_of("账A");
        c.fold() == f1
    };
    s.add("J13-只读不改账", j13, "查询操作不改变折叠指纹");

    // 终审后写账（正册复现）
    let j14 = audit.snap("late", fp_of("late")) == Err(GN_SEALED);
    s.add("J14-终审拒写", j14, "终审后快照登记必拒");

    s.add_dyn(
        "J15-终审指纹落账",
        audit.fold() != 0,
        &format!("fold={:#x}", audit.fold()),
    );
    let empty = GreenAudit::new(FinalState::Pending, "empty").fold();
    s.add_dyn(
        "J16-空册指纹",
        empty == 0x1ba600u32.wrapping_mul(31).wrapping_mul(31).wrapping_mul(31),
        &format!("fold={:#x}", empty),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10383_green_flow() {
        let mut a = GreenAudit::new(FinalState::Green, "t");
        assert!(a.snap("账A", fp_of("账A")).is_ok());
        assert!(a.snap("账A", fp_of("账A")) == Err(GN_DUP_SNAP));
        assert!(a.audit(&[]) == Err(GN_SNAPSHOT_DRIFT));
        assert!(a.audit(&[("账A".to_string(), fp_of("账A"))]).is_ok());
        assert!(a.snap("账B", fp_of("账B")) == Err(GN_SEALED));
        assert_eq!(a.state, FinalState::Green);
    }

    #[test]
    fn unx10383_checks_green() {
        let s = run_unx10383_checks();
        assert!(s.all_passed(), "UNX-F10383 判据未全绿");
    }
}
