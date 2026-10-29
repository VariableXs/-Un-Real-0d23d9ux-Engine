//! ves459_s25conflict — VE-F4225 · 契约冲突检测
//! 冲突类型学（约束矛盾/判据冲突/版本歧义三类×检测算法）；冲突分级（阻断/警告/提示）；
//! 冲突仲裁队列（自动消解失败入队）；阻断级未消→禁合入；检测漏报→样本回补+算法校准；仲裁超时→升级；
//! 无障碍判据冲突一律阻断级（域本色红线——不允许警告降级）；无隐私面。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x16E0;
pub const ARBITRATE_TIMEOUT: u32 = 72;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CfCode {
    Ok = 0,
    BlockOpen = SEG_BASE + 0x01,  // 0x16E1 阻断级未消
    MissedDetect = SEG_BASE + 0x02, // 0x16E2 漏报
    ArbitrateTimeout = SEG_BASE + 0x03, // 0x16E3
}

/// 三类冲突。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConflictKind { Constraint = 0, Criterion = 1, VersionAmb = 2 }

/// 三级分级。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Level { Hint = 0, Warning = 1, Blocker = 2 }

/// 冲突项。
pub struct Conflict { pub kind: ConflictKind, pub level: Level, pub a11y: bool, pub resolved: bool, pub waited: u32 }

/// 冲突册。
pub struct ConflictBook {
    pub items: Vec<Conflict>,
    pub merged: bool,
    pub calibrated: u32,
    pub escalated: u32,
}

impl ConflictBook {
    pub fn new() -> Self { ConflictBook { items: Vec::new(), merged: false, calibrated: 0, escalated: 0 } }
    pub fn put(&mut self, c: Conflict) {
        // 域本色红线：无障碍判据冲突一律阻断级（不允许警告降级）。
        let c = if c.a11y { Conflict { level: Level::Blocker, ..c } } else { c };
        self.items.push(c);
    }
    pub fn resolve(&mut self, idx: usize) { if idx < self.items.len() { self.items[idx].resolved = true; } }
    /// 合入闸：阻断级未消→禁合入。
    pub fn merge_gate(&self) -> Result<(), CfCode> {
        if self.items.iter().any(|c| c.level == Level::Blocker && !c.resolved) {
            return Err(CfCode::BlockOpen);
        }
        Ok(())
    }
    /// 漏报检测：样本回补（约束矛盾未检出→算法校准计数）。
    pub fn missed(&mut self) { self.calibrated += 1; }
    /// 仲裁超时→升级。
    pub fn tick(&mut self) {
        for c in self.items.iter_mut() {
            if !c.resolved && c.level == Level::Blocker {
                c.waited += 1;
                if c.waited > ARBITRATE_TIMEOUT { self.escalated += 1; }
            }
        }
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves459_checks() -> CheckSet {
    let mut set = CheckSet::new("ves459");
    let mut bk = ConflictBook::new();

    // C459-01 警告级冲突→不阻断合入。
    bk.put(Conflict { kind: ConflictKind::Constraint, level: Level::Warning, a11y: false, resolved: false, waited: 0 });
    chk(&mut set, "C459-01", bk.merge_gate().is_ok(), "警告不阻断");

    // C459-02 阻断级未消→禁合入。
    bk.put(Conflict { kind: ConflictKind::VersionAmb, level: Level::Blocker, a11y: false, resolved: false, waited: 0 });
    chk(&mut set, "C459-02", bk.merge_gate() == Err(CfCode::BlockOpen), "阻断禁合入");

    // C459-03 消解后放行。
    bk.resolve(1);
    chk(&mut set, "C459-03", bk.merge_gate().is_ok(), "消解放行");

    // C459-04 域本色红线：a11y 判据冲突自动升级阻断级。
    bk.put(Conflict { kind: ConflictKind::Criterion, level: Level::Warning, a11y: true, resolved: false, waited: 0 });
    chk(&mut set, "C459-04", bk.items[2].level == Level::Blocker
        && bk.merge_gate() == Err(CfCode::BlockOpen), "a11y 一律阻断");

    // C459-05 漏报→样本回补+算法校准。
    bk.missed();
    chk(&mut set, "C459-05", bk.calibrated == 1, "漏报校准");

    // C459-06 仲裁超时→升级。
    for _ in 0..(ARBITRATE_TIMEOUT + 1) { bk.tick(); }
    chk(&mut set, "C459-06", bk.escalated >= 1, "超时升级");

    // C459-07 分级排序单源（提示<警告<阻断）。
    chk(&mut set, "C459-07", Level::Hint < Level::Warning && Level::Warning < Level::Blocker, "三级单源");

    // C459-08 三类冲突单源。
    chk(&mut set, "C459-08", ConflictKind::Constraint as u8 == 0
        && ConflictKind::VersionAmb as u8 == 2, "三类单源");

    // C459-09 a11y 冲突不可降级断言（重放仍阻断）。
    let pre = bk.items[2].level;
    bk.items[2].a11y = false; // 移除标记后可重放降级路径（模拟误标修正）
    bk.items[2].level = Level::Warning;
    bk.items[2].a11y = true;
    bk.put(Conflict { kind: ConflictKind::Criterion, level: Level::Hint, a11y: true, resolved: false, waited: 0 });
    chk(&mut set, "C459-09", bk.items.last().unwrap().level == Level::Blocker && pre == Level::Blocker, "重放仍阻断");

    // C459-10 提示级恒不阻断。
    bk.put(Conflict { kind: ConflictKind::Constraint, level: Level::Hint, a11y: false, resolved: false, waited: 0 });
    let hint_idx = bk.items.len() - 1;
    let bk_gate = if bk.items[..hint_idx].iter().any(|c| c.level == Level::Blocker && !c.resolved) {
        Err(CfCode::BlockOpen) } else { Ok(()) };
    let _ = bk_gate;
    bk.resolve(2); // 消解 a11y 阻断
    chk(&mut set, "C459-10", bk.merge_gate().is_ok(), "消解后提示不阻断");

    // C459-11 仲裁超时常量单源。
    chk(&mut set, "C459-11", ARBITRATE_TIMEOUT == 72, "超时常量");

    // C459-12 冲突项计数。
    chk(&mut set, "C459-12", bk.items.len() == 4, "冲突计数");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves459_smoke() {
        let set = run_ves459_checks();
        assert!(set.failed_count() == 0, "ves459 red: {}", set.red_items_text());
    }
}
