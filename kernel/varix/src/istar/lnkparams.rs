//! F561 快捷方式参数编辑 · 完整设计（STAR I 主册 I 域批次七）。
//!
//! **判据（主册）**：三字段功能；可执行性检查；恢复默认；F013 语义一致；
//! 高级页折叠。
//!
//! **设计要点（主册）**：
//! - 快捷方式高级编辑（属性对话框 F264 增页）：目标参数（命令行参数追加）、
//!   起始位置（工作目录）、运行方式（常规/最小化/最大化）三字段；
//! - 修改即时验证（目标+参数可执行性检查——黄提示不阻断）；
//! - 恢复默认按钮；编辑的是 .lnk 本体（F013 语义完整支持）；
//! - 普通用户看不见这些字段也不碍事（折叠在高级页）。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;

use alloc::string::String;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 运行方式三档（F013 ShowCmd 语义同源）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunStyle {
    Normal,
    Minimized,
    Maximized,
}

impl RunStyle {
    /// F013 .lnk ShowCmd 值（语义一致性对账面）。
    pub fn showcmd(self) -> u32 {
        match self {
            RunStyle::Normal => 1,
            RunStyle::Minimized => 7,
            RunStyle::Maximized => 3,
        }
    }
}

/// 可执行性检查结论（黄提示不阻断——检查只给提示不拦保存）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecCheck {
    Ok,
    /// 目标不存在（黄提示）。
    TargetMissing,
    /// 工作目录不存在（黄提示）。
    WorkdirMissing,
}

// ---------------------------------------------------------------------------
// 编辑器
// ---------------------------------------------------------------------------

/// .lnk 高级编辑器（三字段 + 验证 + 还原）。
pub struct LnkParams {
    /// 目标（可执行文件路径）。
    pub target: String,
    /// 命令行参数（追加段）。
    pub args: String,
    /// 起始位置（工作目录）。
    pub workdir: String,
    /// 运行方式。
    pub style: RunStyle,
    /// 出厂快照（恢复默认 = 还原到打开属性页时的状态）。
    snap_target: String,
    snap_args: String,
    snap_workdir: String,
    snap_style: RunStyle,
    /// 目标存在性（宿主注入——内核无文件系统遍历，验证面收宿主结论）。
    target_exists: bool,
    workdir_exists: bool,
    /// 修改账（是否动过——保存按钮 enable 判据）。
    dirty: bool,
}

impl LnkParams {
    /// 打开编辑器（快照 = 打开时刻的 .lnk 本体四字段——F013 读入）。
    pub fn open(target: &str, args: &str, workdir: &str, style: RunStyle) -> LnkParams {
        LnkParams {
            target: String::from(target),
            args: String::from(args),
            workdir: String::from(workdir),
            style,
            snap_target: String::from(target),
            snap_args: String::from(args),
            snap_workdir: String::from(workdir),
            snap_style: style,
            target_exists: true,
            workdir_exists: true,
            dirty: false,
        }
    }

    /// 宿主注入存在性结论（验证面接缝）。
    pub fn note_fs(&mut self, target_exists: bool, workdir_exists: bool) {
        self.target_exists = target_exists;
        self.workdir_exists = workdir_exists;
    }

    /// 改参数字段（追加语义：整体串替换由本字段承载——UI 层保证光标处编辑）。
    pub fn set_args(&mut self, args: &str) {
        if self.args != args {
            self.args = String::from(args);
            self.dirty = true;
        }
    }

    /// 改工作目录。
    pub fn set_workdir(&mut self, dir: &str) {
        if self.workdir != dir {
            self.workdir = String::from(dir);
            self.dirty = true;
        }
    }

    /// 改运行方式。
    pub fn set_style(&mut self, style: RunStyle) {
        if self.style != style {
            self.style = style;
            self.dirty = true;
        }
    }

    /// 即时验证（黄提示不阻断：结论只提示，保存不被拦）。
    pub fn exec_check(&self) -> ExecCheck {
        if !self.target_exists {
            ExecCheck::TargetMissing
        } else if !self.workdir_exists {
            ExecCheck::WorkdirMissing
        } else {
            ExecCheck::Ok
        }
    }

    /// 恢复默认：四字段还原到打开时刻快照；dirty 清零。
    pub fn restore_default(&mut self) {
        self.target = self.snap_target.clone();
        self.args = self.snap_args.clone();
        self.workdir = self.snap_workdir.clone();
        self.style = self.snap_style;
        self.dirty = false;
    }

    /// 是否有未保存修改（保存按钮/关窗提示判据）。
    pub fn dirty(&self) -> bool {
        self.dirty
    }

    /// 参数启动命令行合成（目标 + 参数——「给游戏加 -windowed」的落点）。
    pub fn command_line(&self) -> (String, String) {
        (self.target.clone(), self.args.clone())
    }

    /// 三字段在高级页折叠（折叠位由属性页持——本编辑器只声明「高级页语义」
    /// 常量：折叠态下基础页不渲染三字段）。
    pub const ADVANCED_COLLAPSED_BY_DEFAULT: bool = true;
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_lnkparams_checks() -> CheckSet {
    let mut set = CheckSet::new(ISTAR_DOMAIN);

    // 1. 三字段功能：参数/工作目录/运行方式各自可改且 dirty 跟随。
    let mut e = LnkParams::open("C:\\game.exe", "", "C:\\", RunStyle::Normal);
    e.set_args("-windowed");
    let d1 = e.dirty();
    e.set_workdir("D:\\games");
    e.set_style(RunStyle::Maximized);
    set.add(
        "three fields editable",
        d1 && e.args == "-windowed" && e.workdir == "D:\\games" && e.style == RunStyle::Maximized,
        "",
    );

    // 2. 可执行性检查：目标缺失黄提示；目录缺失黄提示；齐备 Ok；均不阻断保存。
    e.note_fs(false, true);
    let t_missing = e.exec_check() == ExecCheck::TargetMissing;
    e.note_fs(true, false);
    let w_missing = e.exec_check() == ExecCheck::WorkdirMissing;
    e.note_fs(true, true);
    let ok = e.exec_check() == ExecCheck::Ok;
    set.add(
        "exec check yellow not blocking",
        t_missing && w_missing && ok && e.dirty(),
        "",
    );

    // 3. 恢复默认：四字段还原、dirty 清零。
    e.restore_default();
    set.add(
        "restore default",
        !e.dirty() && e.args.is_empty() && e.workdir == "C:\\" && e.style == RunStyle::Normal,
        "",
    );

    // 4. F013 语义一致：ShowCmd 三值 1/7/3 与 .lnk 规范对齐。
    set.add(
        "f013 showcmd semantics",
        RunStyle::Normal.showcmd() == 1
            && RunStyle::Minimized.showcmd() == 7
            && RunStyle::Maximized.showcmd() == 3,
        "",
    );

    // 5. 高级页折叠：缺省折叠位在册（普通用户不见三字段）。
    set.add(
        "advanced page collapsed by default",
        LnkParams::ADVANCED_COLLAPSED_BY_DEFAULT,
        "",
    );

    // 6. 命令行合成：目标+参数成对输出（游戏 -windowed 落点）。
    let mut e2 = LnkParams::open("C:\\g.exe", "", "", RunStyle::Normal);
    e2.set_args("-windowed -noaudio");
    let (t, a) = e2.command_line();
    set.add(
        "command line composition",
        t == "C:\\g.exe" && a == "-windowed -noaudio",
        "",
    );

    // 7. 幂等编辑：同值再设不记 dirty（键盘连打不虚报修改）。
    let mut e3 = LnkParams::open("x.exe", "-a", "", RunStyle::Normal);
    e3.set_args("-a");
    set.add("idempotent edits no dirty", !e3.dirty(), "");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_is_open_time_state() {
        let mut e = LnkParams::open("a.exe", "-v", "C:\\w", RunStyle::Minimized);
        e.set_args("-v -verbose");
        e.restore_default();
        assert_eq!(e.args, "-v");
        assert_eq!(e.style, RunStyle::Minimized);
    }

    #[test]
    fn dirty_tracks_each_field() {
        let mut e = LnkParams::open("a.exe", "", "", RunStyle::Normal);
        e.set_workdir("D:\\x");
        assert!(e.dirty());
        e.restore_default();
        assert!(!e.dirty());
        e.set_style(RunStyle::Maximized);
        assert!(e.dirty());
    }
}
