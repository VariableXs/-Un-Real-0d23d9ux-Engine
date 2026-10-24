//! VE-F4351 · 一致性文档总纲（U08 测试与资产组）
//! 职责：U 域文档总纲——文档地图（U01-U08 各组文档族索引 + 阅读路径三线
//! ：新人线/集成线/专家线）+ 文档纪律（术语表锁定——与 U01 三入约术语一致
//! ：漂移 CI 拦截）+ 文档 CI 三断言（断链巡检修复 / 缺节拦截 / 术语漂移）
//! + 路径失效重绘地图 + 文档地图读屏可达（域本色）。
//! 段位：0x42E0（续表 idx585）。行数预算 320。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x42E0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DmCode {
    /// 断链（巡检修复）。
    LinkBroken = SEG_BASE,
    /// 缺节（CI 拦截）。
    SectionMissing = SEG_BASE + 0x01,
    /// 术语漂移（CI 拦截）。
    TermDrift = SEG_BASE + 0x02,
    /// 组号越界。
    GroupRange = SEG_BASE + 0x03,
    /// 路径失效（重绘地图）。
    PathStale = SEG_BASE + 0x04,
}

/// 八组文档族索引（U01-U08）。
pub const GROUPS: usize = 8;

/// 阅读路径三线。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Path {
    Newbie,
    Integration,
    Expert,
}

/// 一册文档。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Doc {
    pub group: u8,
    pub id: u32,
    /// 结构节在位（简化位掩码——0x03 = 全节）。
    pub sections: u8,
    /// 地图可达（读屏结构齐备——域色）。
    pub sr_ok: bool,
}

/// 文档总纲台。
#[derive(Default)]
pub struct DocMap {
    pub docs: Vec<Doc>,
    /// 互链表：(源册, 目标册)。
    pub links: Vec<(u32, u32)>,
    /// 术语锁定表（与 U01 三入约一致的正本）。
    pub terms_locked: Vec<(u32, &'static str)>,
    /// 三线路径登记。
    pub paths: Vec<(Path, u32)>,
    pub ci_blocks: u32,
    pub redraws: u32,
}

impl DocMap {
    pub fn new() -> Self {
        DocMap { ..Default::default() }
    }

    /// 册登记（组号校验 1..=8；幂等覆盖）。
    pub fn register(&mut self, d: Doc) -> Result<(), DmCode> {
        if d.group == 0 || d.group as usize > GROUPS {
            return Err(DmCode::GroupRange);
        }
        if let Some(x) = self.docs.iter_mut().find(|x| x.id == d.id) {
            *x = d;
        } else {
            self.docs.push(d);
        }
        Ok(())
    }

    /// 术语锁定（锁定表登记——幂等）。
    pub fn lock_term(&mut self, key: u32, term: &'static str) {
        if !self.terms_locked.iter().any(|t| t.0 == key) {
            self.terms_locked.push((key, term));
        }
    }

    /// 术语漂移 CI 断言：用词与锁定表不符 = 漂移（拦截）。
    pub fn term_check(&self, key: u32, used: &str) -> Result<(), DmCode> {
        match self.terms_locked.iter().find(|t| t.0 == key) {
            Some((_, canonical)) if *canonical == used => Ok(()),
            Some(_) => Err(DmCode::TermDrift),
            None => Ok(()), // 未锁定词不判
        }
    }

    /// 互链登记（目标未登记 = 断链立即拦截）。
    pub fn link(&mut self, src: u32, dst: u32) -> Result<(), DmCode> {
        if !self.docs.iter().any(|d| d.id == dst) {
            return Err(DmCode::LinkBroken);
        }
        if !self.links.iter().any(|l| l.0 == src && l.1 == dst) {
            self.links.push((src, dst));
        }
        Ok(())
    }

    /// 断链巡检：目标被撤的链清出（返回清除数）。
    pub fn inspect_links(&mut self) -> usize {
        let before = self.links.len();
        self.links.retain(|(_, dst)| self.docs.iter().any(|d| d.id == *dst));
        before - self.links.len()
    }

    /// 缺节 CI 断言。
    pub fn assert_sections(&mut self) -> Vec<u32> {
        let mut bad = Vec::new();
        for d in self.docs.iter() {
            if d.sections & 0x03 != 0x03 {
                bad.push(d.id);
                self.ci_blocks += 1;
            }
        }
        bad
    }

    /// 三线路径登记（路径失效检测：路径引用册不在图 = 失效重绘）。
    pub fn add_path(&mut self, p: Path, doc: u32) -> Result<(), DmCode> {
        if !self.docs.iter().any(|d| d.id == doc) {
            self.redraws += 1;
            return Err(DmCode::PathStale);
        }
        if !self.paths.iter().any(|x| x.0 == p && x.1 == doc) {
            self.paths.push((p, doc));
        }
        Ok(())
    }

    /// 三线齐备断言。
    pub fn paths_complete(&self) -> bool {
        [Path::Newbie, Path::Integration, Path::Expert]
            .iter()
            .all(|p| self.paths.iter().any(|x| x.0 == *p))
    }

    /// 地图读屏可达断言（域本色）。
    pub fn sr_clean(&self) -> bool {
        self.docs.iter().all(|d| d.sr_ok)
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves585_checks() -> CheckSet {
    let mut set = CheckSet::new("ves585");
    let mut dm = DocMap::new();

    // C585-01 八组文档族登记。
    let mut ok = true;
    for g in 1..=8u8 {
        if dm.register(Doc { group: g, id: g as u32, sections: 0x03, sr_ok: true }).is_err() {
            ok = false;
        }
    }
    chk(&mut set, "C585-01", ok && dm.docs.len() == 8, "八组登记");

    // C585-02 组号越界拒。
    chk(&mut set, "C585-02", dm.register(Doc { group: 0, id: 99, sections: 3, sr_ok: true }).is_err()
        && dm.register(Doc { group: 9, id: 99, sections: 3, sr_ok: true }).is_err(), "组号越界拒");

    // C585-03 术语锁定 + 漂移 CI 拦截。
    dm.lock_term(1, "令牌");
    dm.lock_term(2, "判定权");
    chk(&mut set, "C585-03", dm.term_check(1, "令牌").is_ok() && dm.term_check(1, "Token") == Err(DmCode::TermDrift), "术语锁定拦截");

    // C585-04 未锁定词不判。
    chk(&mut set, "C585-04", dm.term_check(9, "任意词").is_ok(), "未锁定不判");

    // C585-05 互链登记 + 断链拦截。
    chk(&mut set, "C585-05", dm.link(1, 2).is_ok() && dm.link(1, 99) == Err(DmCode::LinkBroken), "互链+断链拦");

    // C585-06 断链巡检清除（目标撤册）。
    dm.docs.retain(|d| d.id != 2);
    chk(&mut set, "C585-06", dm.inspect_links() == 1 && dm.links.is_empty(), "巡检清除断链");

    // C585-07 缺节 CI 断言。
    let _ = dm.register(Doc { group: 2, id: 2, sections: 0x02, sr_ok: true });
    let bad = dm.assert_sections();
    chk(&mut set, "C585-07", bad == [2] && dm.ci_blocks == 1, "缺节断言拦截");

    // C585-08 修复后断言清零。
    let _ = dm.register(Doc { group: 2, id: 2, sections: 0x03, sr_ok: true });
    chk(&mut set, "C585-08", dm.assert_sections().is_empty(), "修复清零");

    // C585-09 三线路径登记（引用失效册 = 重绘）。
    chk(&mut set, "C585-09", dm.add_path(Path::Newbie, 1).is_ok() && dm.add_path(Path::Newbie, 99).is_err()
        && dm.redraws == 1, "路径失效重绘");

    // C585-10 三线齐备断言。
    let _ = dm.add_path(Path::Integration, 3);
    let _ = dm.add_path(Path::Expert, 8);
    chk(&mut set, "C585-10", dm.paths_complete(), "三线齐备");

    // C585-11 地图读屏可达断言（域本色）。
    let _ = dm.register(Doc { group: 3, id: 3, sections: 3, sr_ok: false });
    chk(&mut set, "C585-11", !dm.sr_clean(), "读屏缺检出");
    let _ = dm.register(Doc { group: 3, id: 3, sections: 3, sr_ok: true });
    chk(&mut set, "C585-11b", dm.sr_clean(), "修复全洁");

    // C585-12 登记幂等（同 id 覆盖不膨胀）。
    let n0 = dm.docs.len();
    let _ = dm.register(Doc { group: 1, id: 1, sections: 3, sr_ok: true });
    chk(&mut set, "C585-12", dm.docs.len() == n0, "登记幂等");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn term_drift_gate() {
        let mut dm = DocMap::new();
        dm.lock_term(1, "词典");
        assert!(dm.term_check(1, "词典").is_ok());
        assert_eq!(dm.term_check(1, "字典"), Err(DmCode::TermDrift));
        assert!(dm.term_check(2, "自由").is_ok());
    }

    #[test]
    fn link_inspect() {
        let mut dm = DocMap::new();
        let _ = dm.register(Doc { group: 1, id: 1, sections: 3, sr_ok: true });
        let _ = dm.register(Doc { group: 2, id: 2, sections: 3, sr_ok: true });
        assert!(dm.link(1, 2).is_ok());
        assert_eq!(dm.link(2, 9), Err(DmCode::LinkBroken));
        dm.docs.retain(|d| d.id != 2);
        assert_eq!(dm.inspect_links(), 1);
    }
}
