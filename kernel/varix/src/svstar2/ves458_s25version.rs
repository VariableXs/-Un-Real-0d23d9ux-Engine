//! ves458_s25version — VE-F4224 · 契约版本管理
//! 语义版本（主/次/补丁）×兼容性承诺映射（主版本=破坏/次版本=向后兼容新增/补丁=修复）；
//! 版本谱系图（派生关系可视）+弃用流程（标记→宽限期→移除三步）；
//! 破坏未升主→阻断发布；弃用未走三步→立案；谱系环→拒绝；
//! 版本承诺含无障碍判据不回退条款（域色红线入约）；无隐私面。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x16A0;
pub const GRACE_DAYS: u32 = 30;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VmCode {
    Ok = 0,
    MajorRequired = SEG_BASE + 0x01, // 0x16A1 破坏未升主
    DeprecationGap = SEG_BASE + 0x02, // 0x16A2 弃用未走三步
    LineageCycle = SEG_BASE + 0x03,  // 0x16A3
    A11yRegress = SEG_BASE + 0x04,   // 0x16A4 不回退条款（域色红线）
}

/// 变更类型。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChangeKind { Breaking = 0, Additive = 1, Patch = 2 }

/// 语义版本（主.次.补丁）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct SemVer { pub major: u32, pub minor: u32, pub patch: u32 }

/// 版本册。
pub struct VersionBook {
    pub current: SemVer,
    pub lineage: Vec<(SemVer, SemVer)>, // 派生边（子, 父）
    pub deprecations: Vec<(&'static str, u32)>, // (契约名, 弃用步骤 0..3)
    pub a11y_floor: u32, // 无障碍判据基线（不得回退——域色红线）
}

impl VersionBook {
    pub fn new(v: SemVer, a11y_floor: u32) -> Self {
        VersionBook { current: v, lineage: Vec::new(), deprecations: Vec::new(), a11y_floor }
    }
    /// 发布闸：破坏必须升主；新增升次；修复升补丁；a11y 基线不得回退（红线）。
    pub fn publish(&mut self, kind: ChangeKind, next: SemVer, new_a11y_floor: u32) -> Result<(), VmCode> {
        if new_a11y_floor < self.a11y_floor { return Err(VmCode::A11yRegress); }
        match kind {
            ChangeKind::Breaking => {
                if next.major <= self.current.major { return Err(VmCode::MajorRequired); }
            }
            ChangeKind::Additive => {
                if next.major != self.current.major || next.minor <= self.current.minor {
                    return Err(VmCode::MajorRequired);
                }
            }
            ChangeKind::Patch => {
                if next.major != self.current.major || next.minor != self.current.minor
                    || next.patch <= self.current.patch {
                    return Err(VmCode::MajorRequired);
                }
            }
        }
        self.lineage.push((next, self.current));
        self.current = next;
        self.a11y_floor = new_a11y_floor;
        Ok(())
    }
    /// 弃用三步：标记(0)→宽限(1)→移除(2)；跳步→立案码。
    pub fn deprecate(&mut self, name: &'static str, step: u32) -> Result<(), VmCode> {
        let cur = self.deprecations.iter().find(|(n, _)| *n == name).map(|(_, s)| *s);
        let expect = match cur { None => 0, Some(s) => s + 1 };
        if step != expect { return Err(VmCode::DeprecationGap); }
        if step == 1 && GRACE_DAYS == 0 { return Err(VmCode::DeprecationGap); }
        self.deprecations.retain(|(n, _)| *n != name);
        self.deprecations.push((name, step));
        Ok(())
    }
    /// 谱系环检出：派生边成环→拒绝。
    pub fn lineage_cycle(&self) -> bool {
        for (child, mut cur) in &self.lineage {
            let mut hops = 0;
            while hops <= self.lineage.len() {
                match self.lineage.iter().find(|(c, _)| c == &cur) {
                    Some((_, parent)) => {
                        if parent == child { return true; }
                        cur = *parent;
                        hops += 1;
                    }
                    None => break,
                }
            }
        }
        false
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves458_checks() -> CheckSet {
    let mut set = CheckSet::new("ves458");
    let v1 = SemVer { major: 1, minor: 0, patch: 0 };
    let mut bk = VersionBook::new(v1, 450);

    // C458-01 破坏未升主→阻断。
    let v11 = SemVer { major: 1, minor: 1, patch: 0 };
    chk(&mut set, "C458-01", bk.publish(ChangeKind::Breaking, v11, 450) == Err(VmCode::MajorRequired), "破坏拒");

    // C458-02 破坏升主→过（谱系登记）。
    let v2 = SemVer { major: 2, minor: 0, patch: 0 };
    chk(&mut set, "C458-02", bk.publish(ChangeKind::Breaking, v2, 450).is_ok()
        && bk.lineage.len() == 1, "升主过");

    // C458-03 向后兼容新增→升次。
    let v21 = SemVer { major: 2, minor: 1, patch: 0 };
    chk(&mut set, "C458-03", bk.publish(ChangeKind::Additive, v21, 450).is_ok(), "升次过");

    // C458-04 修复→升补丁。
    let v211 = SemVer { major: 2, minor: 1, patch: 1 };
    chk(&mut set, "C458-04", bk.publish(ChangeKind::Patch, v211, 450).is_ok(), "升补丁过");

    // C458-05 域色红线：a11y 基线回退→拒（不回退条款入约）。
    let v22 = SemVer { major: 2, minor: 2, patch: 0 };
    chk(&mut set, "C458-05", bk.publish(ChangeKind::Additive, v22, 400) == Err(VmCode::A11yRegress), "基线回退拒");

    // C458-06 a11y 基线提升→过。
    chk(&mut set, "C458-06", bk.publish(ChangeKind::Additive, v22, 460).is_ok() && bk.a11y_floor == 460, "基线提升");

    // C458-07 弃用三步顺序走。
    chk(&mut set, "C458-07", bk.deprecate("old-field", 0).is_ok()
        && bk.deprecate("old-field", 1).is_ok()
        && bk.deprecate("old-field", 2).is_ok(), "三步弃用");

    // C458-08 跳步→立案码。
    chk(&mut set, "C458-08", bk.deprecate("other", 1) == Err(VmCode::DeprecationGap), "跳步拒");

    // C458-09 谱系环检出：构造 2.2→2.1→2.2 环。
    bk.lineage.push((SemVer { major: 2, minor: 1, patch: 0 }, SemVer { major: 2, minor: 2, patch: 0 }));
    chk(&mut set, "C458-09", bk.lineage_cycle(), "谱系环拒");

    // C458-10 拆环后无环。
    bk.lineage.pop();
    chk(&mut set, "C458-10", !bk.lineage_cycle(), "拆环通过");

    // C458-11 语义版本排序单源（Ord 派生可比较）。
    let a = SemVer { major: 1, minor: 2, patch: 3 };
    let b = SemVer { major: 1, minor: 3, patch: 0 };
    chk(&mut set, "C458-11", a < b, "版本可序");

    // C458-12 宽限期常量单源。
    chk(&mut set, "C458-12", GRACE_DAYS == 30 && VmCode::A11yRegress as u16 == SEG_BASE + 4, "常量单源");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves458_smoke() {
        let set = run_ves458_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves458 red: {:?}", red);
    }
}
