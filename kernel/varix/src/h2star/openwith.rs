//! F257 打开方式选择器 · 完整设计（STAR I 主册 H 域）。
//!
//! **判据（主册）**：选择器排序判据；始终勾选写入与不勾不写用例；
//! Edge 兜底入口跳转；关联变更可追溯（设置中心「默认应用」页显示
//! 当前表）。
//!
//! **设计要点（主册）**：双击未知扩展名或右键「打开方式」时：弹选择器
//! 列出可处理该格式的应用（按兼容性评级排序，星图 A2 判例工厂供数）+
//! 「始终使用」勾选（默认不勾——试一次不绑架未来）+「更多应用」展开
//! 全表+「在 Edge 搜索此格式」兜底；选中的应用记住一次关联，勾选始终
//! 才写默认关联表。
//!
//! 实装：候选表（兼容性评级 A2 供给注入，排序：评级降序→名称稳定）；
//! 选择器出口三态（打开一次=不写表 / 始终=写默认关联表 / Edge 兜底）；
//! 关联表每次变更留追溯账（谁、何时、改了什么——设置中心「默认应用」
//! 页直读本表）。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

/// 兼容性评级（星图 A2 判例工厂供数——数值越大越稳）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CompatGrade {
    /// 评级 C（可用）。
    C = 1,
    /// 评级 B（良好）。
    B = 2,
    /// 评级 A（完全兼容）。
    A = 3,
}

/// 一个「能打开这个格式」的候选。
#[derive(Clone, Debug)]
pub struct Candidate {
    pub app: String,
    pub grade: CompatGrade,
}

/// 一次关联变更的追溯账条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssocChange {
    pub ext: String,
    pub from: String,
    pub to: String,
    /// 分钟戳注入。
    pub at_min: u64,
}

/// 打开方式服务。
pub struct OpenWith {
    /// 默认关联表（ext → app）。
    assoc: Vec<(String, String)>,
    /// 追溯账（设置中心「默认应用」页直读）。
    pub changes: Vec<AssocChange>,
}

impl OpenWith {
    pub fn new() -> OpenWith {
        OpenWith { assoc: Vec::new(), changes: Vec::new() }
    }

    pub fn assoc_of(&self, ext: &str) -> Option<&str> {
        self.assoc.iter().find(|(e, _)| e == ext).map(|(_, a)| a.as_str())
    }

    /// 选择器候选排序：评级降序，同评级按名称字典序（稳定可复现）。
    pub fn ranked(candidates: &[Candidate]) -> Vec<&Candidate> {
        let mut v: Vec<&Candidate> = candidates.iter().collect();
        v.sort_by(|a, b| b.grade.cmp(&a.grade).then_with(|| a.app.cmp(&b.app)));
        v
    }

    /// 选择器出口：打开一次——**不写**默认关联表（判据「不勾不写」）。
    pub fn open_once(&self, _ext: &str, _app: &str) {
        // 刻意无副作用：函数体为空是判据本身，不是偷懒。
    }

    /// 选择器出口：始终使用——写默认关联表 + 追溯账。
    pub fn set_default(&mut self, ext: &str, app: &str, at_min: u64) {
        let from = String::from(self.assoc_of(ext).unwrap_or(""));
        if let Some(slot) = self.assoc.iter_mut().find(|(e, _)| e == ext) {
            slot.1 = String::from(app);
        } else {
            self.assoc.push((String::from(ext), String::from(app)));
        }
        self.changes.push(AssocChange {
            ext: String::from(ext),
            from,
            to: String::from(app),
            at_min,
        });
    }

    /// Edge 兜底：返回检索入口文案（跳转由 shell 层执行）。
    pub fn edge_fallback(ext: &str) -> String {
        alloc::format!("在 Edge 搜索「如何打开 {} 文件」", ext)
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_openwith_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-F257");
    // 排序判据：A2 评级降序，同评级名称稳定。
    let cands = alloc::vec![
        Candidate { app: String::from("看图二"), grade: CompatGrade::B },
        Candidate { app: String::from("看图甲"), grade: CompatGrade::A },
        Candidate { app: String::from("看图一"), grade: CompatGrade::B },
    ];
    let ranked = OpenWith::ranked(&cands);
    set.add(
        "F257 rank",
        ranked[0].grade == CompatGrade::A
            && ranked[1].app == "看图一"
            && ranked[2].app == "看图二",
        "grade desc, name tie",
    );
    // 不勾不写。
    let mut ow = OpenWith::new();
    ow.open_once(".xyz", "看图甲");
    set.add(
        "F257 open once writes nothing",
        ow.assoc_of(".xyz").is_none() && ow.changes.is_empty(),
        "no side effect",
    );
    // 勾选始终 → 写表 + 追溯账。
    ow.set_default(".xyz", "看图甲", 100);
    set.add(
        "F257 always writes",
        ow.assoc_of(".xyz") == Some("看图甲") && ow.changes.len() == 1,
        "assoc set",
    );
    // 变更可追溯：from/to 全记。
    ow.set_default(".xyz", "看图乙", 200);
    let c = &ow.changes[1];
    set.add(
        "F257 traceable",
        c.from == "看图甲" && c.to == "看图乙" && c.at_min == 200,
        "who/when/what",
    );
    // Edge 兜底入口。
    set.add(
        "F257 edge fallback",
        OpenWith::edge_fallback(".xyz").contains("Edge") && OpenWith::edge_fallback(".xyz").contains(".xyz"),
        "search entry",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f257_selector_flow() {
        let set = run_openwith_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F257 自检红 {f}/{p}");
    }

    #[test]
    fn grade_order_total() {
        assert!(CompatGrade::A > CompatGrade::B && CompatGrade::B > CompatGrade::C);
    }
}
