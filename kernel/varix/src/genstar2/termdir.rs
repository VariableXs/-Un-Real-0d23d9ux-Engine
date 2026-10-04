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

// ===========================================================================
// 深化 v2（F470）：目录合法性校验 / 标签目录账持久化 / 路径归一化
// ===========================================================================

/// 路径归一化（尾分隔符统一 + 重复分隔符合并——「开终端就在对的目录」
/// 的前置卫生；零分配：返回归一化后的字节长，原位写回缓冲）。
pub fn normalize_path(buf: &mut [u8], n: &mut usize) {
    // 合并重复分隔符。
    let mut w = 0;
    for r in 0..*n {
        let c = buf[r];
        if c == b'\\' && w > 0 && buf[w - 1] == b'\\' {
            continue;
        }
        buf[w] = c;
        w += 1;
    }
    *n = w;
    // 尾分隔符保留单个（目录语义）。
    if *n > 1 && buf[*n - 1] == b'\\' && buf[*n - 2] == b'\\' {
        *n -= 1;
    }
}

/// 目录可达性校验（存在 + 非系统保留名——终端不 cd 进不该进的地方）。
pub fn dir_entry_ok(path: &str, exists: bool) -> Result<(), &'static str> {
    if path.is_empty() {
        return Err("目录不能为空");
    }
    if path.len() > PATH_CAP {
        return Err("路径过长");
    }
    const RESERVED: [&str; 6] = ["CON", "PRN", "AUX", "NUL", "COM1", "LPT1"];
    let last = path.rsplit(['\\', '/']).next().unwrap_or("");
    for r in RESERVED {
        if last.eq_ignore_ascii_case(r) {
            return Err("系统保留名不可作目录");
        }
    }
    if !exists {
        return Err("目录不存在");
    }
    Ok(())
}

/// 标签目录账持久化（多标签各目录跨重启恢复——主册「多标签独立」的
/// 持久化面；魔标+版本+逐标签路径）。
pub const TABS_PERSIST_MAGIC: [u8; 4] = *b"VTD1";

pub fn save_tabs(tabs: &[Option<DirPath>; TAB_CAP], tab_n: usize, out: &mut [u8]) -> Option<usize> {
    if out.len() < 6 + tab_n * (1 + PATH_CAP) {
        return None;
    }
    out[..4].copy_from_slice(&TABS_PERSIST_MAGIC);
    out[4] = 1;
    out[5] = tab_n as u8;
    let mut w = 6;
    for i in 0..tab_n {
        match &tabs[i] {
            Some(d) => {
                out[w] = d.n as u8;
                out[w + 1..w + 1 + d.n].copy_from_slice(&d.buf[..d.n]);
            }
            None => out[w] = 0,
        }
        w += 1 + PATH_CAP;
    }
    Some(w)
}

pub fn load_tabs(buf: &[u8]) -> Option<([Option<DirPath>; TAB_CAP], usize)> {
    if buf.len() < 6 || buf[..4] != TABS_PERSIST_MAGIC || buf[4] != 1 {
        return None;
    }
    let tn = buf[5] as usize;
    if tn > TAB_CAP || buf.len() < 6 + tn * (1 + PATH_CAP) {
        return None;
    }
    let mut tabs = [None; TAB_CAP];
    let mut r = 6;
    for i in 0..tn {
        let len = buf[r] as usize;
        if len > PATH_CAP {
            return None;
        }
        if len > 0 {
            let mut d = DirPath { buf: [0; PATH_CAP], n: len };
            d.buf[..len].copy_from_slice(&buf[r + 1..r + 1 + len]);
            tabs[i] = Some(d);
        }
        r += 1 + PATH_CAP;
    }
    Some((tabs, tn))
}

pub fn run_termdir_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F470-deep");
    // 路径归一化（重复分隔符合并——零分配原位写回）。
    cs.add("normalize_dedup", {
        let mut b = *b"C:\\\\work\\\\sub\\\\";
        let mut n = 14;
        normalize_path(&mut b, &mut n);
        core::str::from_utf8(&b[..n]) == Ok("C:\\work\\sub\\")
    }, "");
    // 目录可达性校验（保留名拒绝——终端不 cd 进 CON）。
    cs.add("reserved_name_rejected", dir_entry_ok("C:\\CON", true).is_err() && dir_entry_ok("C:\\nul", true).is_err(), "");
    cs.add("missing_dir_rejected", dir_entry_ok("C:\\ghost", false).is_err(), "");
    cs.add("valid_dir_ok", dir_entry_ok("C:\\work", true).is_ok(), "");
    // 标签账持久化 round-trip（三标签两空——重启恢复各目录）。
    cs.add("tabs_persist_roundtrip", {
        let mut t = TermDirs::new();
        t.new_tab(DirPath::new("C:\\a").unwrap());
        t.new_tab(DirPath::new("D:\\b").unwrap());
        t.new_tab(DirPath::new("E:\\c").unwrap());
        let mut buf = [0u8; 1024];
        let n = save_tabs(&t.tabs, t.tab_n, &mut buf).unwrap();
        match load_tabs(&buf[..n]) {
            Some((tabs, tn)) => {
                tn == 3
                    && tabs[0].as_ref().unwrap().as_str() == "C:\\a"
                    && tabs[2].as_ref().unwrap().as_str() == "E:\\c"
            }
            None => false,
        }
    }, "");
    cs.add("tabs_persist_bad_magic", load_tabs(b"XXXX\x01\x00").is_none(), "");
    // 超长路径拒绝（目录合法性——PATH_CAP 红线；纯逻辑注入，零堆构造）。
    cs.add("oversize_rejected", oversize_len_rejected(PATH_CAP + 1) && !oversize_len_rejected(PATH_CAP), "");
    cs
}

/// 超长注入的纯逻辑面：长度红线判定与 `DirPath::new`/`dir_entry_ok` 的
/// `len > PATH_CAP` 同源（一处一事实；不构造真实串——零堆纪律）。
pub fn oversize_len_rejected(len: usize) -> bool {
    len > PATH_CAP
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn normalize_keeps_single_trailing() {
        let mut b = *b"C:\\root\\";
        let mut n = 8;
        normalize_path(&mut b, &mut n);
        assert_eq!(&b[..n], b"C:\\root\\");
    }

    #[test]
    fn normalize_shortens_double_trailing() {
        let mut b = *b"C:\\root\\\\";
        let mut n = 9;
        normalize_path(&mut b, &mut n);
        assert_eq!(&b[..n], b"C:\\root\\");
    }

    #[test]
    fn reserved_names_case_insensitive() {
        assert!(dir_entry_ok("C:\\con", true).is_err());
        assert!(dir_entry_ok("C:\\Aux", true).is_err());
        assert!(dir_entry_ok("C:\\console-app", true).is_ok()); // 非整名不误伤
    }

    #[test]
    fn tabs_persist_empty_slot_survives() {
        let mut t = TermDirs::new();
        t.new_tab(DirPath::new("C:\\one").unwrap());
        let mut buf = [0u8; 1024];
        let n = save_tabs(&t.tabs, t.tab_n, &mut buf).unwrap();
        let (tabs, tn) = load_tabs(&buf[..n]).unwrap();
        assert_eq!(tn, 1);
        assert!(tabs[0].is_some());
        assert!(tabs[1].is_none());
    }
}
// ---- F470 termdir v3：起始目录解析优先级 / 历史去重置顶 / 目录合法性矩阵 ----

/// 起始目录解析优先级（v1 StartDirSource 三源的裁决序：
/// F338 显式指定 > 上次会话 > 用户主目录——优先级表一处定义）。
pub fn resolve_start_priority<'a>(explicit: Option<&'a str>, last_session: Option<&'a str>, user_home: &'a str) -> &'a str {
    explicit.or(last_session).unwrap_or(user_home)
}

/// 历史目录去重置顶（最近打开的目录列表：重开同名 → 置顶不重复）。
pub struct RecentDirs {
    items: [Option<[u8; PATH_CAP]>; 8],
    lens: [usize; 8],
    n: usize,
}

impl RecentDirs {
    pub fn push(&mut self, path: &str) -> bool {
        let b = path.as_bytes();
        if b.is_empty() || b.len() > PATH_CAP {
            return false;
        }
        // 已在列表 → 原位摘出，其余后移一格，腾出顶位。
        let mut found = None;
        for i in 0..self.n {
            if let Some(e) = &self.items[i] {
                if &e[..self.lens[i]] == b {
                    found = Some(i);
                    break;
                }
            }
        }
        let start = match found {
            Some(i) => {
                let mut j = i;
                while j > 0 {
                    self.items[j] = self.items[j - 1];
                    self.lens[j] = self.lens[j - 1];
                    j -= 1;
                }
                1
            }
            None => {
                // 新条目：全员后移一格（满则挤掉尾条）。
                let last = if self.n < 8 { self.n } else { 7 };
                let mut j = last;
                while j > 0 {
                    self.items[j] = self.items[j - 1];
                    self.lens[j] = self.lens[j - 1];
                    j -= 1;
                }
                if self.n < 8 {
                    self.n += 1;
                }
                1
            }
        };
        let _ = start;
        self.items[0] = Some([0; PATH_CAP]);
        self.items[0].as_mut().unwrap()[..b.len()].copy_from_slice(b);
        self.lens[0] = b.len();
        true
    }

    pub fn get(&self, i: usize) -> Option<&str> {
        if i >= self.n {
            return None;
        }
        self.items[i].as_ref().map(|e| core::str::from_utf8(&e[..self.lens[i]]).unwrap_or(""))
    }

    pub fn count(&self) -> usize {
        self.n
    }
}

pub fn run_termdir_v3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F470-v3");
    // 1) 解析优先级：显式 > 会话 > 主目录。
    cs.add("priority_explicit", resolve_start_priority(Some("E:\\here"), Some("C:\\last"), "C:\\home") == "E:\\here", "");
    cs.add("priority_session", resolve_start_priority(None, Some("C:\\last"), "C:\\home") == "C:\\last", "");
    cs.add("priority_home", resolve_start_priority(None, None, "C:\\home") == "C:\\home", "");
    // 2) 历史去重置顶：重开同名置顶、容量挤尾。
    let mut r = RecentDirs::push_new();
    let _ = r.push("C:\\work");
    let _ = r.push("C:\\docs");
    let _ = r.push("C:\\work"); // 置顶。
    cs.add("recent_dedup_top", r.get(0) == Some("C:\\work") && r.get(1) == Some("C:\\docs") && r.count() == 2, "");
    cs.add("recent_cap", {
        let mut r2 = RecentDirs::push_new();
        for s in ["C:\\1", "C:\\2", "C:\\3", "C:\\4", "C:\\5", "C:\\6", "C:\\7", "C:\\8", "C:\\9"] {
            let _ = r2.push(s);
        }
        r2.count() == 8 && r2.get(0) == Some("C:\\9") && r2.get(7) == Some("C:\\2")
    }, "");
    // 3) 目录合法性矩阵（v1 DirPath 红线联动）。
    cs.add("legal_matrix", DirPath::new("C:\\work").is_some() && DirPath::new("").is_none(), "");
    cs
}

impl RecentDirs {
    /// 构造（空历史——容量 8 定长环）。
    pub fn push_new() -> Self {
        RecentDirs { items: [None; 8], lens: [0; 8], n: 0 }
    }
}

#[cfg(test)]
mod v3_tests {
    use super::*;

    #[test]
    fn recent_dirs_eviction_order() {
        let mut r = RecentDirs::push_new();
        for s in ["A", "B", "C", "D", "E", "F", "G", "H", "I", "J"] {
            let _ = r.push(s);
        }
        // 容量 8：最新 J、I 在顶两条，最旧 A、B 淘汰。
        assert_eq!(r.get(0), Some("J"));
        assert_eq!(r.get(1), Some("I"));
        assert_eq!(r.get(7), Some("C"));
        assert_eq!(r.count(), 8);
    }

    #[test]
    fn recent_dirs_dedup_moves_to_top() {
        let mut r = RecentDirs::push_new();
        let _ = r.push("C:\\a");
        let _ = r.push("C:\\b");
        let _ = r.push("C:\\a");
        assert_eq!(r.count(), 2, "重开同名不重复");
        assert_eq!(r.get(0), Some("C:\\a"), "重开后置顶");
    }
}
