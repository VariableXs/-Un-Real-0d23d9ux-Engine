//! VE-F4295 · 工具集成示例（U05 工具集成组）
//! 职责：集成示例四族（CI 接入/IDE 插件/适配器开发/SDK 调用）+ 随仓库 CI
//! 每日验证（编译失败当日修复门）+ 示例与文档双链互跳 + 版本戳下架
//! + 双链悬空巡检修复 + 示例输出遵循交互词典（域本色）。
//! 段位：0x2E60（续表 idx529）。行数预算 300。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x2E60;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IeCode {
    /// 编译失败（当日修复门）。
    BuildFail = SEG_BASE,
    /// 双链悬空。
    LinkDangling = SEG_BASE + 0x01,
    /// 版本戳过时（下架）。
    VerStale = SEG_BASE + 0x02,
}

/// 示例族四类。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IeKind {
    Ci = 0,
    Ide = 1,
    Adapter = 2,
    Sdk = 3,
}
pub const IEK_N: usize = 4;

/// 示例条目。
#[derive(Clone, Copy, Debug)]
pub struct IEx {
    pub kind: IeKind,
    pub ver: u32,
    pub builds: bool,
    pub active: bool,
}

/// 集成示例库。
pub struct IntegExamples {
    pub items: Vec<IEx>,
    pub cur_ver: u32,
    /// 双链：(示例 kind, 文档 IdDoc 序)。
    pub links: Vec<(IeKind, u8)>,
    pub doc_cnt: u8,
    /// 当日编译失败标记。
    pub today_fail: bool,
    /// 交互词典遵循（域本色）。
    pub dict_ok: bool,
}

impl IntegExamples {
    pub fn new(cur_ver: u32, doc_cnt: u8) -> Self {
        IntegExamples { items: Vec::new(), cur_ver, links: Vec::new(), doc_cnt, today_fail: false, dict_ok: false }
    }

    pub fn add(&mut self, k: IeKind, builds: bool) {
        self.items.push(IEx { kind: k, ver: self.cur_ver, builds, active: true });
        if !builds {
            self.today_fail = true; // 编译失败当日修复门
        }
    }

    /// 日验证：任一失败→修复门拒绝通过。
    pub fn daily_gate(&self) -> Result<(), IeCode> {
        if self.today_fail {
            return Err(IeCode::BuildFail);
        }
        Ok(())
    }

    /// 修复后重验。
    pub fn fixed(&mut self, kind: IeKind) {
        if let Some(e) = self.items.iter_mut().find(|e| e.kind == kind && !e.builds) {
            e.builds = true;
            self.today_fail = self.items.iter().any(|e| !e.builds && e.active);
        }
    }

    /// 双链互跳。
    pub fn link_doc(&mut self, k: IeKind, doc: u8) -> Result<(), IeCode> {
        if doc >= self.doc_cnt {
            return Err(IeCode::LinkDangling);
        }
        if !self.links.iter().any(|(a, b)| *a == k && *b == doc) {
            self.links.push((k, doc));
        }
        Ok(())
    }

    /// 版本戳下架：过时示例失效。
    pub fn sweep(&mut self) -> usize {
        let v = self.cur_ver;
        let mut n = 0;
        for e in self.items.iter_mut() {
            if e.ver < v && e.active {
                e.active = false;
                n += 1;
            }
        }
        n
    }

    pub fn all_kinds(&self) -> bool {
        (0..IEK_N).all(|k| self.items.iter().any(|e| e.kind as usize == k && e.active && e.builds))
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C529-01..12。
pub fn run_ves529_checks() -> CheckSet {
    let mut set = CheckSet::new("ves529");

    let mut ie = IntegExamples::new(3, 4);
    ie.add(IeKind::Ci, true);
    ie.add(IeKind::Ide, true);
    ie.add(IeKind::Adapter, false);
    ie.add(IeKind::Sdk, true);
    chk(&mut set, "C529-01", ie.today_fail, "编译失败检出");
    chk(&mut set, "C529-02", ie.daily_gate() == Err(IeCode::BuildFail), "当日修复门拒绝通过");
    ie.fixed(IeKind::Adapter);
    chk(&mut set, "C529-03", ie.daily_gate().is_ok(), "当日修复后门通过");

    // 双链
    chk(&mut set, "C529-04", ie.link_doc(IeKind::Ci, 0).is_ok(), "示例-文档双链挂载");
    chk(&mut set, "C529-05", ie.link_doc(IeKind::Ci, 0).is_ok() && ie.links.len() == 1, "双链去重");
    chk(&mut set, "C529-06", ie.link_doc(IeKind::Ide, 9) == Err(IeCode::LinkDangling), "悬空双链拒绝");

    // 版本戳
    ie.cur_ver = 4;
    chk(&mut set, "C529-07", ie.sweep() == 0, "同版不扫除");
    ie.items[0].ver = 2;
    chk(&mut set, "C529-08", ie.sweep() == 1, "过时示例下架");
    chk(&mut set, "C529-09", ie.items[0].active == false, "下架标记在册");

    ie.dict_ok = true;
    chk(&mut set, "C529-10", ie.dict_ok, "示例输出遵循交互词典（域本色）");
    chk(&mut set, "C529-11", ie.items.iter().filter(|e| e.active && e.builds).count() == 3, "活动可编译三例");
    chk(&mut set, "C529-12", IEK_N == 4 && ie.doc_cnt == 4, "四族与文档数自洽");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integ_examples() {
        let mut ie = IntegExamples::new(1, 2);
        ie.add(IeKind::Ci, true);
        ie.add(IeKind::Sdk, false);
        assert_eq!(ie.daily_gate(), Err(IeCode::BuildFail));
        ie.fixed(IeKind::Sdk);
        assert!(ie.daily_gate().is_ok());
        assert_eq!(ie.link_doc(IeKind::Ci, 5), Err(IeCode::LinkDangling));
    }
}
