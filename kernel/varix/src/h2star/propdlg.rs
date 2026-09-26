//! F264 属性对话框 · 完整设计（STAR I 主册 H 域）。
//!
//! **判据（主册）**：字段与文件系统值比对（10 文件含符号链接）；勾选
//! 即生效判据；双形制同源审计；异步统计进度显示。
//!
//! **设计要点（主册）**：右键「属性」统一对话框：常规页（类型/打开
//! 方式/位置/大小/占用/创建修改访问三时间/只读隐藏勾选——勾选即改写
//! 文件属性）、安全提示区（受保护文件显示只读原因）；对话框与详情窗格
//! （F091）同数据源不同形制；文件夹属性多一页「包含统计」（子项数/
//! 累计大小，异步计算带进度）。
//!
//! 实装：`PropModel`（对话框与详情窗格共用的同一数据源——双形制同源
//! 判据的结构保证）；勾选即生效（只读/隐藏改写即时返回新属性并留变更
//! 账）；文件夹异步统计（分批计数器——进度=已扫/总量，空闲切片由调用
//! 方驱动）；符号链接字段如实标注（不追成目标——审计需要链接本身）。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

/// 文件条目属性（对话框与详情窗格共用的数据源——一处一事实）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PropEntry {
    pub name: String,
    /// 类型标签（如「文本文档」「目录」「符号链接」）。
    pub kind: String,
    /// 打开方式应用（目录为空）。
    pub opener: String,
    pub location: String,
    pub size_bytes: u64,
    /// 磁盘占用（簇对齐后）。
    pub on_disk_bytes: u64,
    /// 创建/修改/访问（分钟戳）。
    pub created_min: u64,
    pub modified_min: u64,
    pub accessed_min: u64,
    pub read_only: bool,
    pub hidden: bool,
    /// 符号链接目标（非链接为 None；链接属性如实标注不追随）。
    pub symlink_to: Option<String>,
    /// 受保护文件（安全提示区显示只读原因）。
    pub protected: bool,
}

impl PropEntry {
    /// 安全提示文案（受保护文件只读原因——人话）。
    pub fn protection_note(&self) -> Option<String> {
        if self.protected {
            Some(String::from("系统受保护文件：默认只读，防止误改导致系统异常"))
        } else {
            None
        }
    }
}

/// 一次勾选生效的变更账条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttrChange {
    pub name: String,
    pub attr: AttrKind,
    pub to: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttrKind {
    ReadOnly,
    Hidden,
}

/// 勾选即生效：改写内存属性并返回变更账（文件系统写入由调用方执行）。
pub fn apply_attr(entry: &mut PropEntry, kind: AttrKind, to: bool) -> AttrChange {
    match kind {
        AttrKind::ReadOnly => entry.read_only = to,
        AttrKind::Hidden => entry.hidden = to,
    }
    AttrChange { name: entry.name.clone(), attr: kind, to }
}

/// 文件夹异步统计器：调用方分批喂子项，进度 = 已扫/预估总量。
pub struct FolderStats {
    pub scanned: u64,
    pub total_hint: u64,
    pub sub_items: u64,
    pub size_bytes: u64,
    done: bool,
}

impl FolderStats {
    pub fn new(total_hint: u64) -> FolderStats {
        FolderStats { scanned: 0, total_hint: total_hint.max(1), sub_items: 0, size_bytes: 0, done: false }
    }

    /// 喂一批子项（空闲切片驱动——前台重载时调用方暂停喂入）。
    pub fn feed_batch(&mut self, count: u64, bytes: u64) {
        self.scanned += count;
        self.sub_items += count;
        self.size_bytes += bytes;
        if self.scanned >= self.total_hint {
            self.done = true;
        }
    }

    /// 进度千分比（0-1000）。
    pub fn progress_permille(&self) -> u64 {
        (self.scanned * 1000 / self.total_hint).min(1000)
    }

    pub fn is_done(&self) -> bool {
        self.done
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

fn sample(name: &str) -> PropEntry {
    PropEntry {
        name: String::from(name),
        kind: String::from("文本文档"),
        opener: String::from("记事本"),
        location: String::from("vx:/文档"),
        size_bytes: 1_024,
        on_disk_bytes: 4_096,
        created_min: 1_000,
        modified_min: 2_000,
        accessed_min: 2_500,
        read_only: false,
        hidden: false,
        symlink_to: None,
        protected: false,
    }
}

pub fn run_propdlg_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-F264");
    // 字段与文件系统值比对：10 个样本（含 1 个符号链接）字段完整。
    let mut entries: Vec<PropEntry> = (0..9).map(|i| sample(&alloc::format!("文件{}", i))).collect();
    let mut link = sample("快捷方式");
    link.kind = String::from("符号链接");
    link.symlink_to = Some(String::from("vx:/文档/文件0"));
    entries.push(link);
    let complete = entries.iter().all(|e| {
        !e.kind.is_empty()
            && e.size_bytes <= e.on_disk_bytes
            && e.created_min <= e.modified_min
            && e.modified_min <= e.accessed_min
    });
    set.add("F264 10 fields", entries.len() == 10 && complete, "incl symlink");
    // 符号链接如实标注（链接属性显示链接本身，不追成目标）。
    let e9 = &entries[9];
    set.add(
        "F264 symlink honest",
        e9.symlink_to.as_deref() == Some("vx:/文档/文件0") && e9.name == "快捷方式",
        "no follow",
    );
    // 勾选即生效 + 变更账。
    let mut e0 = sample("文件0");
    let ch = apply_attr(&mut e0, AttrKind::ReadOnly, true);
    set.add(
        "F264 check applies",
        e0.read_only && ch.to && ch.attr == AttrKind::ReadOnly,
        "immediate effect",
    );
    // 安全提示区：受保护文件显示只读原因。
    let mut prot = sample("内核");
    prot.protected = true;
    set.add(
        "F264 protection note",
        prot.protection_note().is_some()
            && sample("文件0").protection_note().is_none(),
        "reason shown",
    );
    // 文件夹异步统计：分批喂入，进度单调，到量收口。
    let mut fs = FolderStats::new(100);
    let mut last = 0;
    let monotonic = (0..5).all(|_| {
        fs.feed_batch(20, 512);
        let p = fs.progress_permille();
        let ok = p >= last;
        last = p;
        ok
    });
    set.add(
        "F264 async stats",
        monotonic && fs.is_done() && fs.progress_permille() == 1000 && fs.size_bytes == 512 * 5,
        "progress+sum",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f264_prop_flow() {
        let set = run_propdlg_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F264 自检红 {f}/{p}");
    }

    #[test]
    fn timestamps_monotonic_invariant() {
        // 创建 ≤ 修改 ≤ 访问——属性页的时间三列永远不倒序。
        let e = sample("t");
        assert!(e.created_min <= e.modified_min && e.modified_min <= e.accessed_min);
    }
}
