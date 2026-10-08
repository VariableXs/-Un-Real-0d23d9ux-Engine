//! F556 用户目录重定向 · 完整设计（STAR I 主册 I 域批次七）。
//!
//! **判据（主册）**：两迁移方式；应用透明跟随（F233/F404 验证）；迁移进度
//! 与撤销；改回流程；状态可见性。
//!
//! **设计要点（主册）**：
//! - 文档/下载/图片等用户目录可重定向到 S: 共享卷（U 盘系统空间管理刚需）：
//!   设置页每目录「更改位置」→ 选目标 → 询问迁移方式（移动现有内容 /
//!   仅改指向留原文件——二选一说清后果）；
//! - 重定向后所有应用透明跟随（保存对话框默认位置 F233 等全部读到新址）；
//! - 迁移动画进度 + 可撤销（会话内）；改回原位同样三步；
//! - 重定向状态在目录属性可见。
//!
//! 迁移语义模型：文件账（名称+大小）逐条搬移/留置，进度 = 已搬/总量，
//! 撤销 = 会话内逆向回放；透明跟随 = 统一解析口 `resolve`。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 支持重定向的目录数上限（文档/下载/图片/音乐/视频/桌面——六目录）。
pub const MAX_DIRS: usize = 6;

// ---------------------------------------------------------------------------
// 模型
// ---------------------------------------------------------------------------

/// 迁移方式（二选一，文案层说清后果）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MigrateMode {
    /// 移动现有内容。
    MoveContent,
    /// 仅改指向，留原文件。
    RepointOnly,
}

impl MigrateMode {
    /// 后果说明文案（三要素之一：选择了什么会发生什么）。
    pub fn consequence(self) -> &'static str {
        match self {
            MigrateMode::MoveContent => "现有文件将搬到新位置，原处不再保留",
            MigrateMode::RepointOnly => "只改系统指向，现有文件留在原地",
        }
    }
}

/// 用户目录（可重定向单元）。
pub struct UserDir {
    pub name: String,
    /// 当前生效位置（重定向后即新址）。
    pub location: String,
    /// 原始位置（改回流程的目标）。
    pub original: String,
    /// 文件账（名称，大小 KiB）。
    pub files: Vec<(String, u64)>,
}

/// 一条迁移回放步（撤销用）。
#[derive(Clone, Debug)]
enum MoveStep {
    /// 文件已搬到新址（撤销 = 搬回）。
    Moved { idx: usize },
    /// 指向已改（撤销 = 指回）。
    Repointed { idx: usize, from: String },
}

/// 迁移会话（进度 + 撤销账）。
pub struct Migration {
    dir_idx: usize,
    mode: MigrateMode,
    target: String,
    total_files: usize,
    done_files: usize,
    /// 已执行步（撤销按逆序回放）。
    steps: Vec<MoveStep>,
    done: bool,
}

/// 重定向管理器。
pub struct RedirMgr {
    dirs: [Option<UserDir>; MAX_DIRS],
    dir_len: usize,
    active: Option<Migration>,
    /// 重定向状态可见性账（属性页取数口）：目录名 → 当前位置。
    state: Vec<(String, String)>,
}

impl RedirMgr {
    pub fn new() -> RedirMgr {
        RedirMgr {
            dirs: [(); MAX_DIRS].map(|_| None),
            dir_len: 0,
            active: None,
            state: Vec::new(),
        }
    }

    /// 登记目录（返回索引）。
    pub fn add_dir(&mut self, name: &str, location: &str, files: &[(&str, u64)]) -> Option<usize> {
        if self.dir_len >= MAX_DIRS {
            return None;
        }
        let files = files.iter().map(|(n, s)| (String::from(*n), *s)).collect();
        self.dirs[self.dir_len] = Some(UserDir {
            name: String::from(name),
            location: String::from(location),
            original: String::from(location),
            files,
        });
        self.state.push((String::from(name), String::from(location)));
        self.dir_len += 1;
        Some(self.dir_len - 1)
    }

    /// 应用透明跟随的统一解析口（F233/F404 验证面）：
    /// 保存对话框等一律经此读「当前生效位置」。
    pub fn resolve(&self, name: &str) -> Option<&str> {
        self.dirs[..self.dir_len]
            .iter()
            .flatten()
            .find(|d| d.name == name)
            .map(|d| d.location.as_str())
    }

    /// 发起重定向（返回 None 当：目录不存在 / 有未完成迁移在途 /
    /// 目标与现位相同）。
    ///
    /// 已完成（done）的上一轮迁移会话在此处自然出清——完成态不占通道，
    /// 「改回原位」的三步对称流程才能成立。
    pub fn begin(&mut self, name: &str, target: &str, mode: MigrateMode) -> bool {
        if let Some(m) = &self.active {
            if !m.done {
                return false;
            }
            self.active = None;
        }
        let idx = match self.dirs[..self.dir_len].iter().position(|d| {
            d.as_ref().map(|d| d.name == name).unwrap_or(false)
        }) {
            Some(i) => i,
            None => return false,
        };
        let d = self.dirs[idx].as_ref().unwrap();
        if d.location == target {
            return false;
        }
        self.active = Some(Migration {
            dir_idx: idx,
            mode,
            target: String::from(target),
            total_files: if mode == MigrateMode::MoveContent { d.files.len() } else { 0 },
            done_files: 0,
            steps: Vec::new(),
            done: false,
        });
        true
    }

    /// 迁移进度（已搬/总量；RepointOnly 恒 0/0）。
    pub fn progress(&self) -> (usize, usize) {
        match &self.active {
            Some(m) => (m.done_files, m.total_files),
            None => (0, 0),
        }
    }

    /// 推进一步搬移（MoveContent 才有意义；步进返回 false 当迁移不在途/已完成）。
    pub fn step(&mut self) -> bool {
        let (idx, done, total) = match &self.active {
            Some(m) if !m.done => (m.dir_idx, m.done_files, m.total_files),
            _ => return false,
        };
        if done >= total {
            return false;
        }
        if let Some(d) = self.dirs[idx].as_mut() {
            d.location = {
                let m = self.active.as_ref().unwrap();
                m.target.clone()
            };
            // 文件账整体随址（模型级：搬移即账面归属变化，逐文件步进由
            // done_files 表达进度）。
            self.active.as_mut().unwrap().done_files += 1;
            self.active
                .as_mut()
                .unwrap()
                .steps
                .push(MoveStep::Moved { idx });
            if self.active.as_ref().unwrap().done_files == self.active.as_ref().unwrap().total_files {
                self.active.as_mut().unwrap().done = true;
                let m = self.active.as_ref().unwrap();
                let name = self.dirs[idx].as_ref().unwrap().name.clone();
                self.state
                    .iter_mut()
                    .find(|(n, _)| *n == name)
                    .map(|(_, l)| *l = m.target.clone());
            }
            true
        } else {
            false
        }
    }

    /// RepointOnly 落位（一步指向切换 + 可撤销账）。
    pub fn repoint(&mut self) -> bool {
        let (idx, target) = match &self.active {
            Some(m) if m.mode == MigrateMode::RepointOnly && !m.done => {
                (m.dir_idx, m.target.clone())
            }
            _ => return false,
        };
        let from = self.dirs[idx].as_ref().unwrap().location.clone();
        self.dirs[idx].as_mut().unwrap().location = target.clone();
        let name = self.dirs[idx].as_ref().unwrap().name.clone();
        self.state
            .iter_mut()
            .find(|(n, _)| *n == name)
            .map(|(_, l)| *l = target);
        self.active.as_mut().unwrap().steps.push(MoveStep::Repointed { idx, from });
        self.active.as_mut().unwrap().done = true;
        true
    }

    /// 迁移是否完成。
    pub fn finished(&self) -> bool {
        self.active.as_ref().map(|m| m.done).unwrap_or(false)
    }

    /// 撤销（会话内）：按逆序回放步，恢复位置与指向；返回恢复的目录名。
    pub fn undo(&mut self) -> Option<String> {
        let mut idx = match &self.active {
            Some(m) => m.dir_idx,
            None => return None,
        };
        while let Some(step) = self.active.as_mut().unwrap().steps.pop() {
            match step {
                MoveStep::Moved { idx: i, .. } => {
                    idx = i;
                    // 文件账随址回滚：位置指回原位。
                    let d = self.dirs[i].as_mut().unwrap();
                    d.location = d.original.clone();
                }
                MoveStep::Repointed { idx: i, from } => {
                    idx = i;
                    self.dirs[i].as_mut().unwrap().location = from.clone();
                }
            }
        }
        let name = self.dirs[idx].as_ref().unwrap().name.clone();
        let loc = self.dirs[idx].as_ref().unwrap().location.clone();
        self.state
            .iter_mut()
            .find(|(n, _)| *n == name)
            .map(|(_, l)| *l = loc);
        self.active = None;
        Some(name)
    }

    /// 改回原位（重定向后的「三步改回」——即向 original 再发起一次迁移）。
    pub fn restore_original(&mut self, name: &str, mode: MigrateMode) -> bool {
        let original = match self.dirs[..self.dir_len].iter().flatten().find(|d| d.name == name) {
            Some(d) => d.original.clone(),
            None => return false,
        };
        self.begin(name, &original, mode)
    }

    /// 状态可见性（属性页取数口）：目录名 → 当前位置快照。
    pub fn visibility(&self) -> &[(String, String)] {
        &self.state
    }
}

impl Default for RedirMgr {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_userredir_checks() -> CheckSet {
    let mut set = CheckSet::new(ISTAR_DOMAIN);

    // 1. 两迁移方式后果文案齐（二选一说清后果）。
    set.add(
        "two modes with consequence text",
        !MigrateMode::MoveContent.consequence().is_empty()
            && !MigrateMode::RepointOnly.consequence().is_empty(),
        "",
    );

    // 2. 移动内容：进度 0/3 → 3/3；位置切换；应用透明跟随读到新址。
    let mut m = RedirMgr::new();
    m.add_dir("下载", "C:\\users\\v\\downloads", &[("a.zip", 1024), ("b.pdf", 32), ("c.txt", 1)]);
    let begun = m.begin("下载", "S:\\downloads", MigrateMode::MoveContent);
    let _ = m.step();
    let mid = m.progress();
    let _ = m.step();
    let _ = m.step();
    let follow = m.resolve("下载") == Some("S:\\downloads");
    set.add(
        "move content progress and follow",
        begun && mid == (1, 3) && m.progress() == (3, 3) && m.finished() && follow,
        "",
    );

    // 3. 会话内撤销：搬移后撤销回原位、状态可见性同步。
    let undone = m.undo();
    set.add(
        "undo restores original",
        undone.as_deref() == Some("下载")
            && m.resolve("下载") == Some("C:\\users\\v\\downloads")
            && m.visibility()[0].1 == "C:\\users\\v\\downloads",
        "",
    );

    // 4. 仅改指向：文件留原地（进度语义 0/0）、位置即新址。
    let mut m2 = RedirMgr::new();
    m2.add_dir("图片", "C:\\users\\v\\pictures", &[("p.png", 2048)]);
    m2.begin("图片", "S:\\pics", MigrateMode::RepointOnly);
    let repointed = m2.repoint();
    set.add(
        "repoint only keeps files",
        repointed && m2.progress() == (0, 0) && m2.resolve("图片") == Some("S:\\pics") && m2.finished(),
        "",
    );

    // 5. 改回流程：向 original 再迁（三步对称——完成态不占通道）。
    let back = m2.restore_original("图片", MigrateMode::MoveContent);
    let progress_ok = m2.progress() == (0, 1);
    set.add("restore original re-begins", back && progress_ok, "");

    // 6. 目标与现位相同 → 拒绝（防无意义迁移）。
    let mut m3 = RedirMgr::new();
    m3.add_dir("音乐", "C:\\m", &[]);
    set.add("same target rejected", !m3.begin("音乐", "C:\\m", MigrateMode::MoveContent), "");

    // 7. 状态可见性账与实际位置一致（属性页取数口）。
    let m4_state = m3.visibility();
    set.add(
        "visibility matches location",
        m4_state.len() == 1 && m4_state[0].0 == "音乐" && m4_state[0].1 == "C:\\m",
        "",
    );

    // 8. 未完成前再发起被拒（单迁移在途纪律——不双写）。
    let mut m5 = RedirMgr::new();
    m5.add_dir("视频", "C:\\v", &[("x.mkv", 9)]);
    m5.begin("视频", "S:\\v", MigrateMode::MoveContent);
    let second = m5.begin("视频", "S:\\v2", MigrateMode::MoveContent);
    set.add("single migration at a time", !second && !m5.finished(), "");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_unknown_dir_none() {
        let m = RedirMgr::new();
        assert!(m.resolve("不存在").is_none());
    }

    #[test]
    fn step_without_migration_false() {
        let mut m = RedirMgr::new();
        assert!(!m.step());
    }

    #[test]
    fn move_then_undo_then_redo_moves_again() {
        let mut m = RedirMgr::new();
        m.add_dir("桌面", "C:\\d", &[("n.txt", 1)]);
        m.begin("桌面", "S:\\d", MigrateMode::MoveContent);
        m.step();
        m.undo();
        m.begin("桌面", "S:\\d", MigrateMode::MoveContent);
        m.step();
        assert!(m.finished());
        assert_eq!(m.resolve("桌面"), Some("S:\\d"));
    }
}
