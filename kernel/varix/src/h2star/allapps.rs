//! F274 「所有应用」列表 · 完整设计（STAR I 主册 H 域）。
//!
//! **判据（主册）**：索引跳转精度（首应用可见）；混排排序规则用例；
//! 装/卸即时性（<2s）；系统组件可见判据。
//!
//! **设计要点（主册）**：开始菜单「所有应用」页：A-Z 字母索引条（右侧
//! 竖条，点击字母跳段、按住滑动连续跳）、应用按显示名排序（中英混排
//! 按拼音/字母二序列）、系统组件同样列出（用户看得到全貌，不藏）；
//! 列表数据来自 vxapp 注册表+系统组件清单，装/卸即时反映。
//!
//! 实装：混排排序器（每应用带拼音首字母键——中英二序列单一规则）；
//! 字母索引（段锚计算——跳转后首应用可见）；装/卸事件即时反映（注册
//! 表增删直接进序列，无延迟队列）；系统组件来源标记（可见不藏）。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

/// 一个应用条目。
#[derive(Clone, Debug)]
pub struct AppEntry {
    pub name: String,
    /// 拼音首字母键（中文应用供给，如「记事本」→ "jsb"；英文=自身首字母）。
    pub pinyin_initials: String,
    /// 系统组件标记（可见不藏——判据）。
    pub system: bool,
}

/// 所有应用页模型。
pub struct AllApps {
    entries: Vec<AppEntry>,
}

impl AllApps {
    pub fn new() -> AllApps {
        AllApps { entries: Vec::new() }
    }

    /// 排序键：拼音首字母（英文应用传首字母大写）——中英混排单一二序列。
    fn sort_key(e: &AppEntry) -> char {
        e.pinyin_initials.chars().next().unwrap_or('~').to_ascii_uppercase()
    }

    /// 重建序列：按排序键稳定排序（装/卸即时反映——每次变更后重排）。
    pub fn rebuild(&mut self) {
        self.entries.sort_by_key(|e| (Self::sort_key(e), e.name.clone()));
    }

    /// 安装即时入列 + 重排（无延迟队列——<2s 判据的机制保证）。
    pub fn install(&mut self, name: &str, pinyin: &str) {
        self.entries.push(AppEntry {
            name: String::from(name),
            pinyin_initials: String::from(pinyin),
            system: false,
        });
        self.rebuild();
    }

    /// 卸载即时出列。
    pub fn uninstall(&mut self, name: &str) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| e.name != name);
        self.entries.len() != before
    }

    /// 系统组件登记（用户看得到全貌——不藏）。
    pub fn register_system(&mut self, name: &str, pinyin: &str) {
        self.entries.push(AppEntry {
            name: String::from(name),
            pinyin_initials: String::from(pinyin),
            system: true,
        });
        self.rebuild();
    }

    pub fn list(&self) -> &[AppEntry] {
        &self.entries
    }

    /// 字母索引：返回该字母段的起始下标（段不存在→下一个更靠后的段——
    /// 「首应用可见」判据：跳转落点必须可见一个应用，绝不空段死跳）。
    pub fn section_start(&self, letter: char) -> Option<usize> {
        let want = letter.to_ascii_uppercase();
        // 找第一个键 >= want 的条目。
        self.entries.iter().position(|e| Self::sort_key(e) >= want)
    }

    /// 索引条（右侧竖条）：现有序列里实际出现的字母段。
    pub fn index_bar(&self) -> Vec<char> {
        let mut out: Vec<char> = Vec::new();
        for e in &self.entries {
            let k = Self::sort_key(e);
            if out.last() != Some(&k) {
                out.push(k);
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_allapps_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-F274");
    let mut page = AllApps::new();
    // 混排数据：中文+英文+系统组件。
    page.register_system("设置中心", "sz");
    page.install("记事本", "jsb");
    page.install("计算器", "jsq");
    page.install("Edge", "E");
    page.install("画图", "ht");
    // 混排排序：E < 画图(H) < 计算器(J) < 记事本(J，名称次序) < 设置(S)。
    let names: Vec<String> = page.list().iter().map(|e| e.name.clone()).collect();
    set.add(
        "F274 mixed sort",
        names == alloc::vec![
            String::from("Edge"),
            String::from("画图"),
            String::from("计算器"),
            String::from("记事本"),
            String::from("设置中心"),
        ],
        "pinyin/letter two-lane",
    );
    // 索引跳转精度：跳 J 段首应用可见。
    let j = page.section_start('J');
    set.add(
        "F274 jump lands visible",
        j == Some(2) && page.list()[2].name == "计算器",
        "first app visible",
    );
    // 不存在的段落到下一实际段（绝不空跳）：K 段不存在 → 落 S 段。
    let k = page.section_start('K');
    set.add(
        "F274 missing letter",
        k == Some(4) && page.list()[4].name == "设置中心",
        "next real section",
    );
    // 索引条。
    let bar = page.index_bar();
    set.add(
        "F274 index bar",
        bar == alloc::vec!['E', 'H', 'J', 'S'],
        "letters only",
    );
    // 装/卸即时性。
    page.install("浏览器", "llq");
    let pos = page.list().iter().position(|e| e.name == "浏览器");
    let removed = page.uninstall("浏览器");
    set.add(
        "F274 instant install/uninstall",
        pos.is_some() && removed && page.list().iter().all(|e| e.name != "浏览器"),
        "<2s reaction",
    );
    // 系统组件可见。
    set.add(
        "F274 system visible",
        page.list().iter().any(|e| e.system && e.name == "设置中心"),
        "not hidden",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f274_list_semantics() {
        let set = run_allapps_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F274 自检红 {f}/{p}");
    }

    #[test]
    fn empty_page_never_panics() {
        let page = AllApps::new();
        assert_eq!(page.section_start('A'), None);
        assert!(page.index_bar().is_empty());
    }
}
