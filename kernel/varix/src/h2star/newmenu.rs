//! F259 新建菜单与命名初态 · 完整设计（STAR I 主册 H 域）。
//!
//! **判据（主册）**：三项默认清单审计；重命名初态（全选/焦点/光标）
//! 三判据；落点网格位计算用例；「更多新建」折叠行为。
//!
//! **设计要点（主册）**：「新建」菜单默认三件：文件夹、文本文档、
//! vxtheme 主题包（E 域联动）——第三方向 vxapp 申请加入需清单声明且
//! 默认折叠进「更多新建」；新建对象落地即进入行内重命名态（F260）、
//! 默认名「新建文件夹」被全选、直接打字即替换；新建落点=当前视图第一
//! 个可用网格位（F084 网格语义），不飞到列表末尾。
//!
//! 实装：默认三项清单（唯一源）；第三方扩展申请入「更多新建」折叠区
//! （主清单不膨胀）；重命名初态三元组（全选/焦点/光标位）；
//! 落点复用 [`h2base::pick_slot`]（一处一事实）。

use crate::checks::CheckSet;
use crate::h2star::h2base::pick_slot;
use crate::h2star::h2edit;

use alloc::string::String;
use alloc::vec::Vec;

/// 重命名初态（三判据的结构化表达）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenameInit {
    /// 被全选的段（默认名主名段全选；F260 扩展名隔离由行内重命名接手）。
    pub select_all: bool,
    /// 焦点在编辑框内。
    pub focused: bool,
    /// 光标位置（全选语义下=段尾，打字即替换）。
    pub caret: usize,
    /// 全选的字节区间（深化二：主名段精确区间——与 h2edit 同源）。
    pub select_range: (usize, usize),
}

/// 新建对象类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NewKind {
    Folder,
    TextDoc,
    VxTheme,
    /// 第三方扩展（只进折叠区）。
    ThirdParty,
}

/// 新建菜单服务。
pub struct NewMenu {
    /// 第三方扩展（「更多新建」折叠区——vxapp 清单声明制）。
    extras: Vec<(String, NewKind)>,
    /// 当前视图已占用的网格位（位图——删除后补位复用的前提）。
    grid: Vec<bool>,
    cols: usize,
}

/// 默认三项清单（唯一源——审计脚本从这里对）。
pub const DEFAULT_THREE: [(&str, NewKind); 3] = [
    ("新建文件夹", NewKind::Folder),
    ("新建文本文档.txt", NewKind::TextDoc),
    ("新主题.vxtheme", NewKind::VxTheme),
];

impl NewMenu {
    pub fn new(cols: usize) -> NewMenu {
        NewMenu { extras: Vec::new(), grid: Vec::new(), cols: cols.max(1) }
    }

    /// 主菜单项：默认三项（第三方永不混入——折叠纪律）。
    pub fn primary(&self) -> Vec<(&'static str, NewKind)> {
        DEFAULT_THREE.to_vec()
    }

    /// 「更多新建」折叠区（第三方扩展；未申请为空——空区不渲染入口）。
    pub fn more(&self) -> Vec<(String, NewKind)> {
        self.extras.clone()
    }

    /// 第三方扩展申请：清单声明制——登记进折叠区，主清单纹丝不动。
    pub fn register_extra(&mut self, name: &str) {
        self.extras.push((String::from(name), NewKind::ThirdParty));
    }

    /// 新建落点：当前视图**第一个可用**网格位（位图扫描——删除后的
    /// 空位立即复用，不跳到末尾追加；全满则追加新位）。
    pub fn landing_slot(&mut self) -> (usize, usize) {
        let free = self.grid.iter().position(|used| !*used).unwrap_or(self.grid.len());
        if free >= self.grid.len() {
            self.grid.push(true);
        } else {
            self.grid[free] = true;
        }
        let (col, row) = pick_slot(free, self.cols);
        (col, row)
    }

    /// 释放落点（新建被 Esc 取消/对象被删除——空位补位复用）。
    pub fn release_slot(&mut self, index: usize) -> bool {
        match self.grid.get_mut(index) {
            Some(used) if *used => {
                *used = false;
                true
            }
            _ => false,
        }
    }

    /// 落地重名递增：视图里已有「新建文件夹」→ 落地名变
    /// 「新建文件夹 (2)」（新建用 (n) 递增——与「副本」规则
    /// [`h2base::bump_copy_name`] 分属两套语义，不混用）。
    pub fn bump_new_name(existing: &[String], wanted: &str) -> String {
        if !existing.iter().any(|e| e == wanted) {
            return String::from(wanted);
        }
        let (stem, ext) = crate::h2star::h2base::ext_split(wanted);
        for n in 2..=999u32 {
            let candidate = alloc::format!("{} ({}){}", stem, n, ext);
            if !existing.iter().any(|e| e == &candidate) {
                return candidate;
            }
        }
        alloc::format!("{} (999+){}", stem, ext)
    }

    /// 重命名初态：默认名**主名段**全选、焦点入框、光标在段尾
    /// （深化二接线 [`h2edit::initial_selection`]——「新建文本文档.txt」
    /// 类带扩展名的默认名只全选主名，扩展名隔离与 F260 同一条规则）。
    pub fn rename_init(default_name: &str) -> RenameInit {
        let (start, end) = h2edit::initial_selection(default_name);
        RenameInit {
            select_all: true,
            focused: true,
            caret: end,
            select_range: (start, end),
        }
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_newmenu_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-F259");
    let mut m = NewMenu::new(6);
    // 三项默认清单。
    let p = m.primary();
    set.add(
        "F259 default three",
        p.len() == 3
            && p[0].0 == "新建文件夹"
            && p[1].0 == "新建文本文档.txt"
            && p[2].0 == "新主题.vxtheme",
        "folder/txt/vxtheme",
    );
    // 落点网格位：6 列视图中第 8 个对象落在 (1,1)——不飞到列表末尾。
    let mut last = (0, 0);
    for _ in 0..8 {
        last = m.landing_slot();
    }
    set.add("F259 landing slot", last == (1, 1), "first free cell");
    // --- 深化二：删除后空位复用（Esc 取消/删除不留永远空格）。 ---
    set.add("F259 release honest", m.release_slot(3) && !m.release_slot(3), "release once");
    let reused = m.landing_slot();
    set.add("F259 slot reuse", reused == (3, 0), "freed cell first");
    // --- 深化二：落地重名递增（新建语义 (2)，与「副本」规则分家）。 ---
    let view: Vec<String> = alloc::vec![String::from("新建文件夹")];
    set.add(
        "F259 bump (2)",
        NewMenu::bump_new_name(&view, "新建文件夹") == "新建文件夹 (2)",
        "new-item ladder",
    );
    let view2: Vec<String> = alloc::vec![String::from("新建文件夹"), String::from("新建文件夹 (2)")];
    set.add(
        "F259 bump (3)",
        NewMenu::bump_new_name(&view2, "新建文件夹") == "新建文件夹 (3)",
        "ladder continues",
    );
    set.add(
        "F259 no bump when free",
        NewMenu::bump_new_name(&[], "新主题.vxtheme") == "新主题.vxtheme",
        "no collision no bump",
    );
    let view3: Vec<String> = alloc::vec![String::from("报告.txt"), String::from("报告 (2).txt")];
    set.add(
        "F259 bump keeps ext",
        NewMenu::bump_new_name(&view3, "报告.txt") == "报告 (3).txt",
        "ext rides along",
    );
    // 重命名初态三判据（主名段精确区间——带扩展名的默认名只选主名）。
    let ri = NewMenu::rename_init("新建文件夹");
    set.add(
        "F259 rename init",
        ri.select_all && ri.focused && ri.caret == 15 && ri.select_range == (0, 15),
        "all-select/focus/caret (bytes: 5字×3)",
    );
    let ri2 = NewMenu::rename_init("新建文本文档.txt");
    set.add(
        "F259 rename init ext-safe",
        ri2.select_range.1 == 18 && ri2.caret == 18,
        "stem only (6字×3=18 字节)",
    );
    // 「更多新建」折叠：第三方进折叠区，主清单不变；空区无入口。
    set.add("F259 more empty", m.more().is_empty(), "no extras no entry");
    m.register_extra("某笔记·新建笔记");
    m.register_extra("某表格·新建表格");
    set.add(
        "F259 more folded",
        m.more().len() == 2 && m.primary().len() == 3,
        "folded, primary intact",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f259_menu_and_landing() {
        let set = run_newmenu_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F259 自检红 {f}/{p}");
    }

    #[test]
    fn extras_never_leak_into_primary() {
        let mut m = NewMenu::new(4);
        for i in 0..50 {
            m.register_extra(&alloc::format!("扩展{}", i));
        }
        assert_eq!(m.primary().len(), 3, "五十个扩展也挤不进主清单");
    }
}
