//! VE-F4310 · 场景应用文档（U06 场景应用组）
//! 职责：场景应用文档——文档族（七景各一册指南 + 场景选择总览）+ 文档
//! 结构定式四节（场景/嵌入点/判据/FAQ——CI 断言拦截缺节）+ 与工具面
//! 文档互链巡检修复 + 指南与实现漂移版本戳重写 + 读屏结构标准（域本色）。
//! 段位：0x3160（续表 idx544）。行数预算 300。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x3160;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DcCode {
    /// 四节定式缺节（CI 断言拦截）。
    SectionMissing = SEG_BASE,
    /// 互链悬空（巡检修复对象）。
    LinkDangling = SEG_BASE + 0x01,
    /// 指南与实现漂移（版本戳不符触发重写）。
    ImplDrift = SEG_BASE + 0x02,
    /// 景号越界。
    SceneRange = SEG_BASE + 0x03,
}

/// 四节定式：场景 / 嵌入点 / 判据 / FAQ。
pub const SECTIONS: [&str; 4] = ["scene", "embed", "verdict", "faq"];

/// 一册指南。
#[derive(Clone, Debug)]
pub struct Guide {
    pub scene: u8,
    pub ver: u32,
    /// 各节存在位（bit0..3 = scene/embed/verdict/faq）。
    pub sections: u8,
    /// 读屏结构标准（标题层级 + 表格替代文本齐备）。
    pub sr_ok: bool,
}

/// 场景文档台。
#[derive(Default)]
pub struct SceneDocs {
    pub guides: Vec<Guide>,
    /// 互链表：(源景, 目标册名)。
    pub links: Vec<(u8, u32)>,
    /// 工具面文档登记册（互链目标合法性单源）。
    pub tool_docs: Vec<u32>,
    pub ci_blocks: u32,
    pub rewritten: u32,
}

const SCENE_MAX: u8 = 7;

impl SceneDocs {
    /// 登记指南（景号校验）。
    pub fn add(&mut self, scene: u8, ver: u32, sections: u8, sr_ok: bool) -> Result<(), DcCode> {
        if scene == 0 || scene > SCENE_MAX {
            return Err(DcCode::SceneRange);
        }
        if let Some(g) = self.guides.iter_mut().find(|g| g.scene == scene) {
            g.ver = ver;
            g.sections = sections;
            g.sr_ok = sr_ok;
        } else {
            self.guides.push(Guide { scene, ver, sections, sr_ok });
        }
        Ok(())
    }

    /// CI 四节断言：缺节即拦截（返回缺节景号表，计数留痕）。
    pub fn ci_assert(&mut self) -> Vec<u8> {
        let mut missing = Vec::new();
        for g in self.guides.iter() {
            if g.sections & 0x0F != 0x0F {
                missing.push(g.scene);
                self.ci_blocks += 1;
            }
        }
        missing
    }

    /// 工具面文档登记（互链目标单源）。
    pub fn register_tool_doc(&mut self, doc: u32) {
        if !self.tool_docs.contains(&doc) {
            self.tool_docs.push(doc);
        }
    }

    /// 互链登记：目标未登记 = 悬空（立即拦截入表）。
    pub fn link(&mut self, scene: u8, doc: u32) -> Result<(), DcCode> {
        if scene == 0 || scene > SCENE_MAX {
            return Err(DcCode::SceneRange);
        }
        if !self.tool_docs.contains(&doc) {
            return Err(DcCode::LinkDangling);
        }
        if !self.links.iter().any(|l| l.0 == scene && l.1 == doc) {
            self.links.push((scene, doc));
        }
        Ok(())
    }

    /// 互链巡检：登记册被撤的目标 → 悬空清单（修复=撤链）。
    pub fn inspect_links(&mut self) -> Vec<(u8, u32)> {
        let mut dead = Vec::new();
        for (s, d) in self.links.iter() {
            if !self.tool_docs.contains(d) {
                dead.push((*s, *d));
            }
        }
        for key in dead.iter() {
            self.links.retain(|l| l != key);
        }
        dead
    }

    /// 漂移重写：指南版本戳与实现版本不符 → 以实现为准重写（留痕计数）。
    pub fn drift_rewrite(&mut self, scene: u8, impl_ver: u32) -> Result<bool, DcCode> {
        let g = self.guides.iter_mut().find(|g| g.scene == scene).ok_or(DcCode::SceneRange)?;
        if g.ver != impl_ver {
            g.ver = impl_ver;
            self.rewritten += 1;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// 读屏结构断言：全册读屏结构齐备（域本色）。
    pub fn sr_clean(&self) -> bool {
        self.guides.iter().all(|g| g.sr_ok)
    }

    /// 总览册：七景全齐才可发布总览。
    pub fn overview_ready(&self) -> bool {
        (1..=SCENE_MAX).all(|s| self.guides.iter().any(|g| g.scene == s))
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves544_checks() -> CheckSet {
    let mut set = CheckSet::new("ves544");
    let mut sd = SceneDocs::default();

    // C544-01 七景指南登记（四节全齐 + 读屏结构齐）。
    let mut all_ok = true;
    for s in 1..=7u8 {
        if sd.add(s, 100 + s as u32, 0x0F, true).is_err() {
            all_ok = false;
        }
    }
    chk(&mut set, "C544-01", all_ok && sd.overview_ready(), "七景指南齐备");

    // C544-02 四节定式缺节 CI 断言拦截（计数留痕）。
    let _ = sd.add(5, 105, 0x0B, true); // 缺 faq(bit3)
    let missing = sd.ci_assert();
    chk(&mut set, "C544-02", missing == [5] && sd.ci_blocks == 1, "缺节断言拦截");

    // C544-03 复扫无新拦截（修复后清零不再误报）。
    let _ = sd.add(5, 106, 0x0F, true);
    let missing2 = sd.ci_assert();
    chk(&mut set, "C544-03", missing2.is_empty(), "修复后复扫洁");

    // C544-04 景号越界拒绝。
    chk(&mut set, "C544-04", sd.add(0, 1, 0x0F, true).is_err() && sd.add(8, 1, 0x0F, true).is_err(), "景号越界拒");

    // C544-05 工具面文档登记后互链成功。
    sd.register_tool_doc(7001);
    chk(&mut set, "C544-05", sd.link(1, 7001).is_ok() && sd.links.len() == 1, "登记后互链成");

    // C544-06 未登记目标互链 = 悬空拦截。
    chk(&mut set, "C544-06", sd.link(2, 7002) == Err(DcCode::LinkDangling), "悬空目标拦截");

    // C544-07 登记册撤销 → 巡检出悬空并修复（撤链）。
    sd.register_tool_doc(7003);
    let _ = sd.link(3, 7003);
    sd.tool_docs.retain(|d| *d != 7003);
    let dead = sd.inspect_links();
    chk(&mut set, "C544-07", dead == [(3, 7003)] && sd.links.len() == 1, "巡检撤链修复");

    // C544-08 漂移重写以实现为准（版本戳不符触发）。
    let rw = sd.drift_rewrite(1, 999);
    chk(&mut set, "C544-08", rw == Ok(true) && sd.rewritten == 1 && sd.guides[0].ver == 999, "漂移重写留痕");

    // C544-09 版本一致不重写（零误重）。
    chk(&mut set, "C544-09", sd.drift_rewrite(1, 999) == Ok(false) && sd.rewritten == 1, "一致不重写");

    // C544-10 读屏结构断言：缺结构检出。
    let _ = sd.add(7, 107, 0x0F, false);
    chk(&mut set, "C544-10", !sd.sr_clean(), "读屏结构缺检出");

    // C544-11 修复后全册读屏洁（域本色）。
    let _ = sd.add(7, 108, 0x0F, true);
    chk(&mut set, "C544-11", sd.sr_clean(), "全册读屏洁");

    // C544-12 四节定式常量单源（场景/嵌入点/判据/FAQ）。
    chk(&mut set, "C544-12", SECTIONS == ["scene", "embed", "verdict", "faq"], "四节定式单源");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ci_sections_assert() {
        let mut sd = SceneDocs::default();
        assert!(sd.add(1, 1, 0x0F, true).is_ok());
        assert!(sd.add(2, 1, 0x07, true).is_ok());
        assert_eq!(sd.ci_assert(), [2]);
        assert_eq!(sd.ci_assert(), [2]); // 未修复前复扫仍拦
        let _ = sd.add(2, 2, 0x0F, true);
        assert!(sd.ci_assert().is_empty());
    }

    #[test]
    fn link_dangling_flow() {
        let mut sd = SceneDocs::default();
        assert_eq!(sd.link(1, 9), Err(DcCode::LinkDangling));
        sd.register_tool_doc(9);
        assert!(sd.link(1, 9).is_ok());
        sd.tool_docs.clear();
        assert_eq!(sd.inspect_links(), [(1, 9)]);
        assert!(sd.links.is_empty());
    }

    #[test]
    fn overview_requires_all() {
        let mut sd = SceneDocs::default();
        for s in 1..=6u8 {
            let _ = sd.add(s, 1, 0x0F, true);
        }
        assert!(!sd.overview_ready());
        let _ = sd.add(7, 1, 0x0F, true);
        assert!(sd.overview_ready());
    }
}
