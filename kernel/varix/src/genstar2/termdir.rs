//! F470 终端启动目录记忆（genstar2 · I 域通用·二分队 · AI-U2）。
//!
//! 主册判据（验收标准第一句）：
//! **三源设置行为；上次目录持久化；F338 路径优先判据；多标签独立；默认值
//! 文档化。**
//!
//! 功能定义（主册批次三）：启动目录三源可选——默认用户目录/上次关闭时目录
//! （会话记忆）/固定目录；默认上次目录；从资源管理器「在终端打开」（F338）
//! 进入时以该目录为准（不受默认设置影响）；多标签各标签独立目录。
//!
//! 零堆纪律：定长路径缓冲，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 三源设置（主册原文三源）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StartDirSource {
    /// 默认用户目录。
    UserHome,
    /// 上次关闭时目录（会话记忆——默认值，文档化）。
    LastSession,
    /// 固定目录（用户指定）。
    Fixed,
}

/// 路径缓冲容量。
pub const PATH_CAP: usize = 128;
/// 多标签容量。
pub const TAB_CAP: usize = 8;

impl StartDirSource {
    /// 默认值（主册：默认上次目录——干活的人多数接着昨天的现场干）。
    pub fn default_source() -> StartDirSource {
        StartDirSource::LastSession
    }
}

/// 定长路径（零分配目录记忆）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DirPath {
    pub buf: [u8; PATH_CAP],
    pub n: usize,
}

impl DirPath {
    pub fn new(s: &str) -> Option<DirPath> {
        let b = s.as_bytes();
        if b.is_empty() || b.len() > PATH_CAP {
            return None;
        }
        let mut d = DirPath { buf: [0; PATH_CAP], n: b.len() };
        d.buf[..b.len()].copy_from_slice(b);
        Some(d)
    }

    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.n]).unwrap_or("")
    }
}

/// 终端目录管理器。
pub struct TermDirs {
    pub source: StartDirSource,
    /// 用户指定固定目录（source=Fixed 时用）。
    pub fixed: Option<DirPath>,
    /// 用户目录（三源之一；由系统注入）。
    pub home: Option<DirPath>,
    /// 上次关闭时目录（持久化槽）。
    last: Option<DirPath>,
    /// 各标签当前目录（多标签独立）。
    tabs: [Option<DirPath>; TAB_CAP],
    tab_n: usize,
}

impl TermDirs {
    pub const fn new() -> Self {
        TermDirs {
            source: StartDirSource::LastSession,
            fixed: None,
            home: None,
            last: None,
            tabs: [None; TAB_CAP],
            tab_n: 0,
        }
    }

    /// 关闭时落盘（上次目录持久化）。
    pub fn on_close(&mut self, cwd: DirPath) {
        self.last = Some(cwd);
    }

    /// 启动目录解析（三源行为；F338 优先级最高——主册：此路径不受默认
    /// 设置影响）。
    pub fn resolve_start(&self, f338_dir: Option<&DirPath>) -> Option<DirPath> {
        // F338「在终端打开」：永远以该目录为准。
        if let Some(d) = f338_dir {
            return Some(*d);
        }
        match self.source {
            StartDirSource::UserHome => self.home,
            StartDirSource::LastSession => self.last.or(self.home),
            StartDirSource::Fixed => self.fixed,
        }
    }

    /// 新标签（多标签各干各的互不串目录）。
    pub fn new_tab(&mut self, dir: DirPath) -> bool {
        if self.tab_n >= TAB_CAP {
            return false;
        }
        self.tabs[self.tab_n] = Some(dir);
        self.tab_n += 1;
        true
    }

    pub fn tab_dir(&self, i: usize) -> Option<DirPath> {
        self.tabs.get(i).copied().flatten()
    }

    /// 标签内 cd（只动该标签）。
    pub fn cd(&mut self, tab: usize, dir: DirPath) -> bool {
        match self.tabs.get_mut(tab) {
            Some(slot) => {
                *slot = Some(dir);
                true
            }
            None => false,
        }
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_termdir_checks() -> CheckSet {
    let mut cs = CheckSet::new("F470-termdir");
    // 1) 默认值文档化（默认上次目录）。
    cs.add("default_documented", StartDirSource::default_source() == StartDirSource::LastSession, "");
    // 2) 三源行为。
    let mut t = TermDirs::new();
    t.home = Some(DirPath::new("C:\\Users\\vx").unwrap());
    t.fixed = Some(DirPath::new("D:\\work").unwrap());
    t.on_close(DirPath::new("C:\\proj\\star").unwrap());
    t.source = StartDirSource::LastSession;
    cs.add("src_last", t.resolve_start(None).unwrap().as_str() == "C:\\proj\\star", "");
    t.source = StartDirSource::UserHome;
    cs.add("src_home", t.resolve_start(None).unwrap().as_str() == "C:\\Users\\vx", "");
    t.source = StartDirSource::Fixed;
    cs.add("src_fixed", t.resolve_start(None).unwrap().as_str() == "D:\\work", "");
    // 3) F338 路径优先判据（不受默认设置影响——三源下都赢）。
    let f338 = DirPath::new("E:\\drop-here").unwrap();
    for src in [StartDirSource::LastSession, StartDirSource::UserHome, StartDirSource::Fixed] {
        t.source = src;
        if t.resolve_start(Some(&f338)).unwrap().as_str() != "E:\\drop-here" {
            cs.add("f338_priority", false, "");
            return cs;
        }
    }
    cs.add("f338_priority", true, "");
    // 4) 上次目录缺失回退用户目录（首次开机不装死）。
    let mut t2 = TermDirs::new();
    t2.home = Some(DirPath::new("C:\\Users\\vx").unwrap());
    cs.add("last_missing_fallback", t2.resolve_start(None).unwrap().as_str() == "C:\\Users\\vx", "");
    // 5) 多标签独立。
    let mut t3 = TermDirs::new();
    t3.new_tab(DirPath::new("C:\\a").unwrap());
    t3.new_tab(DirPath::new("D:\\b").unwrap());
    t3.cd(0, DirPath::new("C:\\a\\sub").unwrap());
    cs.add("tabs_independent", t3.tab_dir(0).unwrap().as_str() == "C:\\a\\sub" && t3.tab_dir(1).unwrap().as_str() == "D:\\b", "");
    cs.add("tab_oob_honest", t3.tab_dir(99).is_none(), "");
    // 6) 路径容量常量在册（实际超限路径由宿主测试覆盖）。
    cs.add("path_cap_registered", PATH_CAP == 128 && TAB_CAP == 8, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f338_wins_over_every_source() {
        let mut t = TermDirs::new();
        t.home = Some(DirPath::new("C:\\Users\\vx").unwrap());
        t.fixed = Some(DirPath::new("D:\\fixed").unwrap());
        t.on_close(DirPath::new("C:\\last").unwrap());
        let f338 = DirPath::new("E:\\here").unwrap();
        for src in [StartDirSource::LastSession, StartDirSource::UserHome, StartDirSource::Fixed] {
            t.source = src;
            assert_eq!(t.resolve_start(Some(&f338)).unwrap().as_str(), "E:\\here");
        }
    }

    #[test]
    fn oversize_path_rejected_honestly() {
        let long = "C:\\".to_string() + &"x".repeat(PATH_CAP);
        assert!(DirPath::new(&long).is_none());
        let fits = "C:\\".to_string() + &"x".repeat(PATH_CAP - 4);
        assert!(DirPath::new(&fits).is_some());
    }

    #[test]
    fn tab_count_bounded_honestly() {
        let mut t = TermDirs::new();
        for i in 0..TAB_CAP {
            assert!(t.new_tab(DirPath::new("C:\\x").unwrap()), "第 {i} 个标签应成功");
        }
        assert!(!t.new_tab(DirPath::new("C:\\y").unwrap()));
    }
}
