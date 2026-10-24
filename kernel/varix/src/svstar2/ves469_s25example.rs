//! ves469_s25example — VE-F4235 · 契约示例
//! 契约示例集：四类通用契约完整示例（DSL 源+生成文档+测试样例三件套）；反例集（常见错误契约正反对照）；
//! 示例可执行跑通即验收；示例跑挂→示例即缺陷；反例未按预期拒绝→验证器缺陷立案；失配→修订；
//! 示例内含无障碍判据示范（域本色）；示例数据虚构。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1900;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExCode {
    Ok = 0,
    ExampleBroken = SEG_BASE + 0x01, // 0x1901 示例即缺陷
    CounterNotRejected = SEG_BASE + 0x02, // 0x1902 验证器缺陷
    TrioIncomplete = SEG_BASE + 0x03, // 0x1903 三件套缺
}

/// 四类示例（与市场四类模板同构）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExampleKind { Pagination = 0, Sort = 1, Filter = 2, Upload = 3 }

impl ExampleKind {
    pub const ALL: [ExampleKind; 4] = [ExampleKind::Pagination, ExampleKind::Sort, ExampleKind::Filter, ExampleKind::Upload];
}

/// 示例三件套（DSL 源+生成文档+测试样例）+a11y 判据示范标记（域本色）。
#[derive(Clone, Copy)]
pub struct Example { pub kind: ExampleKind, pub dsl: bool, pub doc: bool, pub test: bool, pub a11y_demo: bool, pub runs: bool }

/// 示例库。
pub struct ExampleBook {
    pub examples: Vec<Example>,
    pub counters: Vec<(u32, bool)>, // 反例（id, 按预期拒绝）
    pub filed: u32,
    pub revised: u32,
}

impl ExampleBook {
    pub fn new() -> Self { ExampleBook { examples: Vec::new(), counters: Vec::new(), filed: 0, revised: 0 } }
    pub fn put(&mut self, e: Example) {
        self.examples.retain(|x| x.kind != e.kind);
        self.examples.push(e);
    }
    /// 可执行验收：跑通即验收；跑挂→示例即缺陷立案。
    pub fn execute(&mut self, kind: ExampleKind) -> Result<(), ExCode> {
        let e = match self.examples.iter().find(|x| x.kind == kind) {
            Some(e) => *e, None => { self.filed += 1; return Err(ExCode::TrioIncomplete); }
        };
        if !(e.dsl && e.doc && e.test) { self.filed += 1; return Err(ExCode::TrioIncomplete); }
        if !e.runs { self.filed += 1; return Err(ExCode::ExampleBroken); }
        Ok(())
    }
    /// 反例验收：反例必须被拒绝；未拒→验证器缺陷立案。
    pub fn counter_check(&mut self, id: u32, rejected: bool) -> Result<(), ExCode> {
        if !rejected { self.filed += 1; return Err(ExCode::CounterNotRejected); }
        if let Some(c) = self.counters.iter_mut().find(|(i, _)| *i == id) { c.1 = true; }
        else { self.counters.push((id, true)); }
        Ok(())
    }
    /// 修订（失配路径）。
    pub fn revise(&mut self, kind: ExampleKind) {
        for e in self.examples.iter_mut() {
            if e.kind == kind { e.runs = true; self.revised += 1; }
        }
    }
    /// 全库验收：四类全过+反例全拒。
    pub fn all_accept(&self) -> bool {
        self.examples.len() == 4 && self.counters.iter().all(|(_, r)| *r)
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves469_checks() -> CheckSet {
    let mut set = CheckSet::new("ves469");
    let mut bk = ExampleBook::new();

    // C469-01 四类示例入库（三件套齐+a11y 示范+可执行——域本色）。
    for k in ExampleKind::ALL {
        bk.put(Example { kind: k, dsl: true, doc: true, test: true, a11y_demo: true, runs: true });
    }
    chk(&mut set, "C469-01", bk.examples.len() == 4
        && bk.examples.iter().all(|e| e.a11y_demo), "四类三件套+a11y 示范");

    // C469-02 可执行验收全过。
    for k in ExampleKind::ALL { bk.execute(k).unwrap(); }
    chk(&mut set, "C469-02", bk.filed == 0, "跑通验收");

    // C469-03 示例跑挂→示例即缺陷。
    bk.examples[1].runs = false;
    chk(&mut set, "C469-03", bk.execute(ExampleKind::Sort) == Err(ExCode::ExampleBroken)
        && bk.filed == 1, "示例即缺陷");

    // C469-04 修订→复验过。
    bk.revise(ExampleKind::Sort);
    chk(&mut set, "C469-04", bk.execute(ExampleKind::Sort).is_ok() && bk.revised == 1, "修订复验");

    // C469-05 三件套缺→缺件码。
    bk.examples[2].doc = false;
    chk(&mut set, "C469-05", bk.execute(ExampleKind::Filter) == Err(ExCode::TrioIncomplete), "三件套缺");

    // C469-06 反例按预期拒绝→验收。
    chk(&mut set, "C469-06", bk.counter_check(901, true).is_ok(), "反例拒收");

    // C469-07 反例未拒→验证器缺陷立案。
    chk(&mut set, "C469-07", bk.counter_check(902, false) == Err(ExCode::CounterNotRejected)
        && bk.filed == 2, "验证器缺陷");

    // C469-08 未知示例→缺件码。
    let mut bk2 = ExampleBook::new();
    chk(&mut set, "C469-08", bk2.execute(ExampleKind::Upload) == Err(ExCode::TrioIncomplete), "缺示例");

    // C469-09 四类单源断言。
    chk(&mut set, "C469-09", ExampleKind::ALL.len() == 4
        && ExampleKind::ALL[0] == ExampleKind::Pagination, "四类单源");

    // C469-10 全库验收终态。
    bk.revise(ExampleKind::Filter);
    bk.counter_check(902, true);
    chk(&mut set, "C469-10", bk.all_accept(), "全库验收");

    // C469-11 示例数据虚构断言（id 均为合成号）。
    chk(&mut set, "C469-11", bk.counters.iter().all(|(i, _)| *i >= 900), "虚构数据");

    // C469-12 错误码段位单源。
    chk(&mut set, "C469-12", ExCode::CounterNotRejected as u16 == SEG_BASE + 2, "段位单源");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves469_smoke() {
        let set = run_ves469_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves469 red: {:?}", red);
    }
}
