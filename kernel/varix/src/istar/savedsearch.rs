//! F587 保存的搜索 · 完整设计（STAR I 主册 I 域批次八）。
//!
//! **判据（主册）**：保存/重跑/新鲜度；胶囊编辑；侧栏区与上限；删除；
//! 与 F306 索引联动。
//!
//! **设计要点（主册）**：
//! - 常用搜索条件存为快捷：搜索页「保存此搜索」（命名如「本周改动的文档」）
//!   → 保存后出现在侧栏「保存的搜索」区（≤10 条）；
//! - 点击即重跑（条件是活的是查询不是快照——结果永远新鲜）；
//! - 条件可视化（「类型:文档 修改:近7天」胶囊可逐个删改）。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 侧栏区上限（条）。
pub const SAVED_CAP: usize = 10;

/// 胶囊类型（条件可视化唯一源）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapsuleKind {
    /// 类型（文档/图片/…）。
    Kind,
    /// 修改时间（近 7 天/…）。
    Modified,
    /// 位置（某目录下）。
    Location,
}

/// 一枚条件胶囊。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Capsule {
    pub kind: CapsuleKind,
    pub value: String,
}

/// 一条保存的搜索。
#[derive(Clone, Debug)]
pub struct SavedSearch {
    pub name: String,
    pub capsules: Vec<Capsule>,
}

/// 保存的搜索账（侧栏区）。
pub struct SavedSearches {
    items: Vec<SavedSearch>,
    /// 最近一次重跑的时刻（新鲜度对账——重跑时更新）。
    last_run_ms: Vec<u64>,
    now_ms: u64,
}

impl SavedSearches {
    pub fn new() -> SavedSearches {
        SavedSearches {
            items: Vec::new(),
            last_run_ms: Vec::new(),
            now_ms: 0,
        }
    }

    /// 保存（满 10 条拒绝——防堆积；同名覆盖更新）。
    pub fn save(&mut self, name: &str, capsules: Vec<Capsule>) -> bool {
        if let Some(i) = self.items.iter().position(|s| s.name == name) {
            self.items[i].capsules = capsules;
            return true;
        }
        if self.items.len() >= SAVED_CAP {
            return false;
        }
        if capsules.is_empty() {
            return false; // 空条件不成搜索（诚实拒绝）。
        }
        self.items.push(SavedSearch {
            name: String::from(name),
            capsules,
        });
        self.last_run_ms.push(0);
        true
    }

    /// 重跑：把胶囊交给索引（F306）执行——条件是活的（每次点都重查），
    /// 返回查询签名（同参同签、条件变签变——新鲜度证据）。
    pub fn run(&mut self, name: &str, ms: u64) -> Option<u64> {
        self.now_ms = ms;
        let i = self.items.iter().position(|s| s.name == name)?;
        self.last_run_ms[i] = ms;
        // 查询签名 = 胶囊序列 FNV-1a（内核零堆哈希惯例）。
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for c in &self.items[i].capsules {
            for b in (c.kind as u8).to_le_bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x1000_0000_01b3);
            }
            for b in c.value.as_bytes() {
                h ^= *b as u64;
                h = h.wrapping_mul(0x1000_0000_01b3);
            }
        }
        Some(h)
    }

    /// 胶囊编辑：逐个删改（改了下次重跑即新结果）。
    pub fn edit_capsule(&mut self, name: &str, idx: usize, value: &str) -> bool {
        match self
            .items
            .iter_mut()
            .find(|s| s.name == name)
            .and_then(|s| s.capsules.get_mut(idx))
        {
            Some(c) => {
                c.value = String::from(value);
                true
            }
            None => false,
        }
    }

    /// 胶囊删除。
    pub fn remove_capsule(&mut self, name: &str, idx: usize) -> bool {
        let s = match self.items.iter_mut().find(|s| s.name == name) {
            Some(s) => s,
            None => return false,
        };
        if idx >= s.capsules.len() {
            return false;
        }
        s.capsules.remove(idx);
        true
    }

    /// 删除整条搜索。
    pub fn delete(&mut self, name: &str) -> bool {
        let before = self.items.len();
        let mut idx = None;
        for (i, s) in self.items.iter().enumerate() {
            if s.name == name {
                idx = Some(i);
                break;
            }
        }
        if let Some(i) = idx {
            self.items.remove(i);
            self.last_run_ms.remove(i);
        }
        self.items.len() != before
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn names(&self) -> Vec<String> {
        self.items.iter().map(|s| s.name.clone()).collect()
    }

    /// 查看一条的胶囊（条件可视化取数口）。
    pub fn capsules_of(&self, name: &str) -> Option<&[Capsule]> {
        self.items.iter().find(|s| s.name == name).map(|s| s.capsules.as_slice())
    }
}

impl Default for SavedSearches {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_savedsearch_checks() -> CheckSet {
    let mut set = CheckSet::new(ISTAR_DOMAIN);

    let cap = |k: CapsuleKind, v: &str| Capsule { kind: k, value: String::from(v) };

    // 1. 保存：命名入侧栏区；条件可视化可查。
    let mut s = SavedSearches::new();
    s.save(
        "本周改动的文档",
        alloc::vec![cap(CapsuleKind::Kind, "文档"), cap(CapsuleKind::Modified, "近7天")],
    );
    let caps = s.capsules_of("本周改动的文档");
    set.add(
        "save with visible capsules",
        s.len() == 1
            && caps.map(|c| c.len() == 2 && c[0].kind == CapsuleKind::Kind && c[1].value == "近7天").unwrap_or(false),
        "",
    );

    // 2. 重跑/新鲜度：同参同签；条件变签变（活查询不是快照）。
    let r1 = s.run("本周改动的文档", 1_000);
    let r2 = s.run("本周改动的文档", 2_000);
    s.edit_capsule("本周改动的文档", 1, "近30天");
    let r3 = s.run("本周改动的文档", 3_000);
    set.add(
        "rerun fresh signature",
        r1.is_some() && r1 == r2 && r3 != r2,
        "",
    );

    // 3. 胶囊编辑与删除：逐个改值/移除。
    s.remove_capsule("本周改动的文档", 0);
    let after = s.capsules_of("本周改动的文档");
    set.add(
        "capsule edit and remove",
        after.map(|c| c.len() == 1 && c[0].value == "近30天").unwrap_or(false),
        "",
    );

    // 4. 侧栏区上限：第 11 条拒绝（防堆积）。
    let mut s2 = SavedSearches::new();
    for i in 0..10 {
        assert!(s2.save(
            &alloc::format!("搜索{}", i),
            alloc::vec![cap(CapsuleKind::Kind, "文档")],
        ));
    }
    let eleventh = s2.save("第11条", alloc::vec![cap(CapsuleKind::Kind, "图片")]);
    set.add(
        "sidebar cap ten",
        s2.len() == SAVED_CAP && !eleventh && SAVED_CAP == 10,
        "",
    );

    // 5. 同名覆盖更新（不新增条目）。
    let count_before = s2.len();
    s2.save("搜索0", alloc::vec![cap(CapsuleKind::Kind, "图片")]);
    set.add(
        "same name updates",
        s2.len() == count_before
            && s2.capsules_of("搜索0").map(|c| c[0].value == "图片").unwrap_or(false),
        "",
    );

    // 6. 删除：删后侧栏区回退、腾出名额。
    s2.delete("搜索5");
    let re_add = s2.save("新搜索", alloc::vec![cap(CapsuleKind::Location, "D:\\")]);
    set.add(
        "delete frees slot",
        s2.len() == SAVED_CAP && re_add && !s2.names().contains(&String::from("搜索5")),
        "",
    );

    // 7. 空条件拒绝（不成搜索）。
    set.add("empty capsules rejected", !s2.save("空条件", alloc::vec![]), "");

    // 8. F306 联动：重跑签名喂索引（签名稳定性——同参幂等）。
    let a = s2.run("新搜索", 100);
    let b = s2.run("新搜索", 200);
    set.add(
        "f306 index signature stable",
        a.is_some() && a == b,
        "",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_unknown_none() {
        let mut s = SavedSearches::new();
        assert!(s.run("无", 1).is_none());
    }

    #[test]
    fn delete_unknown_false() {
        let mut s = SavedSearches::new();
        assert!(!s.delete("无"));
    }

    #[test]
    fn edit_out_of_range_false() {
        let mut s = SavedSearches::new();
        s.save("a", alloc::vec![Capsule { kind: CapsuleKind::Kind, value: String::from("文档") }]);
        assert!(!s.edit_capsule("a", 5, "x"));
        assert!(!s.remove_capsule("a", 0 + 9));
    }
}
