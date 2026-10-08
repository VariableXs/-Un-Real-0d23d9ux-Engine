//! F233 原生文件打开/保存对话框 · H 基础通用域实装。
//!
//! **判据锚**：F233。
//!
//! **验收标准（主册第一句）**：VARIX 自家应用打开/保存文件走统一对话框：
//! 左侧五位置栏（桌面/下载/S: 共享/最近/收藏）、中列文件列表（复用资源
//! 管理器组件缩略图）、右上文件名框、右下格式下拉；保存时自动带入应用
//! 建议名、重名即时提示不等到点保存才报；对话框记住每应用上次目录。
//!
//! **设计要点**：
//! - [`FileDialogSpec`] 打开/保存双模式；保存模式构造即带入应用建议名
//!   （[`FileDialog::new`] 的 `suggested` 参数，`suggested_name_used`
//!   记账）——「自动带入应用建议名」；
//! - 五位置栏注册表：[`SidePlace`] 五项穷举 + [`SidePlace::root`]
//!   跳转语义常量表（桌面/下载/S: 共享/最近/收藏）；
//! - 重名即时检测：键入停顿 [`DUP_DEBOUNCE_MS`] 300ms 触发查重
//!   （[`FileDialog::poll`] 防抖状态机 Idle→Typing→Done——防抖语义：
//!   边打字不查，停顿才查，比「点保存才报」早一整段）；
//! - 每应用上次目录记忆：[`LastDirBook`]（app id → 定容路径表，
//!   [`LASTDIR_CAP`] 容量上限环形淘汰 + [`LastDirBook::to_bytes`] /
//!   [`from_bytes`](LastDirBook::from_bytes) 持久化边界 round-trip
//!   ——「重启后保持」）；
//! - 格式过滤器：扩展名白名单（[`FileDialog::set_filter`]），目录恒
//!   可见，白名单空 = 全部可见；
//! - 布局同构几何表：[`DlgLayout`] 四区（位置栏/列表/文件名框/格式下拉）
//!   常量几何，与 F008 Win32 兼容层对话框对齐（同一张几何表）；
//! - 热路径零堆：路径/文件名/列表全定容缓冲；Vec 仅在检查快照面。
//!
//! **依赖锚点**：`crate::checks::CheckSet`、`crate::h1star::h1base::Rect`。

use crate::checks::CheckSet;
use crate::h1star::h1base::Rect;

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 文件名框缓冲容量（字节）——超长输入拒绝，不静默截断。
pub const NAME_CAP: usize = 64;

/// 路径定容缓冲（字节）。
pub const PATH_CAP: usize = 96;

/// 每应用目录记忆容量——第 17 个应用覆盖最旧槽（环形淘汰）。
pub const LASTDIR_CAP: usize = 16;

/// 重名查重防抖——主册 F233 验收「键入停顿 <300ms 查」原文。
pub const DUP_DEBOUNCE_MS: u64 = 300;

/// 对话框一次装载的文件列表定容（复用资源管理器组件的单页装载量）。
pub const LIST_CAP: usize = 64;

/// 格式下拉扩展名白名单条数上限。
pub const FILTER_CAP: usize = 4;

/// 条目名缓冲容量。
pub const ENTRY_NAME_CAP: usize = 32;

// ---------------------------------------------------------------------------
// 五位置栏注册表
// ---------------------------------------------------------------------------

/// 左侧五位置栏（主册穷举，无第六位）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SidePlace {
    /// 桌面。
    Desktop,
    /// 下载。
    Downloads,
    /// S: 共享。
    ShareS,
    /// 最近。
    Recent,
    /// 收藏。
    Favorites,
}

impl SidePlace {
    /// 五项全列（审计遍历用）。
    pub const ALL: [SidePlace; 5] = [
        SidePlace::Desktop,
        SidePlace::Downloads,
        SidePlace::ShareS,
        SidePlace::Recent,
        SidePlace::Favorites,
    ];

    /// 栏名。
    pub fn name(self) -> &'static str {
        match self {
            SidePlace::Desktop => "桌面",
            SidePlace::Downloads => "下载",
            SidePlace::ShareS => "S: 共享",
            SidePlace::Recent => "最近",
            SidePlace::Favorites => "收藏",
        }
    }

    /// 跳转语义：该位置栏的根路径（注册表唯一真相）。
    pub fn root(self) -> &'static str {
        match self {
            SidePlace::Desktop => "/home/user/Desktop",
            SidePlace::Downloads => "/home/user/Downloads",
            SidePlace::ShareS => "/mnt/s-share",
            SidePlace::Recent => "recent://",
            SidePlace::Favorites => "favorites://",
        }
    }
}

// ---------------------------------------------------------------------------
// 定容路径与小名条目
// ---------------------------------------------------------------------------

/// 定容路径（零堆；持久化边界序列化）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FixedPath {
    buf: [u8; PATH_CAP],
    len: usize,
}

impl FixedPath {
    pub fn new() -> FixedPath {
        FixedPath { buf: [0; PATH_CAP], len: 0 }
    }

    /// 从 &str 构造；超容返回 None（显性拒绝，不截断）。
    pub fn from_str(s: &str) -> Option<FixedPath> {
        let b = s.as_bytes();
        if b.len() > PATH_CAP {
            return None;
        }
        let mut p = FixedPath::new();
        p.buf[..b.len()].copy_from_slice(b);
        p.len = b.len();
        Some(p)
    }

    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 追加一个路径分量（自动补 '/'）。超容返回 false。
    pub fn push_component(&mut self, comp: &str) -> bool {
        let c = comp.as_bytes();
        let sep = if self.len > 0 && self.buf[self.len - 1] != b'/' { 1 } else { 0 };
        if self.len + sep + c.len() > PATH_CAP {
            return false;
        }
        if sep == 1 {
            self.buf[self.len] = b'/';
            self.len += 1;
        }
        self.buf[self.len..self.len + c.len()].copy_from_slice(c);
        self.len += c.len();
        true
    }

    /// 弹出末段分量。空路径 / 已是根 "/"（无分量可弹，no-op）返回 false。
    /// [缺陷账本] 现象：fixed_path_push_pop_and_cap 红（"根路径不再弹"）。
    /// 根因：pop 对 "/" 走 base==0 → len=1 分支并返回 true——no-op 却报
    /// 成功，与单元测试既定语义（根路径不再弹）矛盾。修法：len==1 的
    /// 根路径显性返回 false（no-op 失败语义与 forget 等接口一致）。
    pub fn pop(&mut self) -> bool {
        if self.len == 0 {
            return false;
        }
        match self.buf[..self.len].iter().rposition(|b| *b == b'/') {
            Some(0) if self.len == 1 => false,
            Some(i) => {
                self.len = if i == 0 { 1 } else { i }; // 根 '/' 保留。
                true
            }
            None => {
                self.len = 0;
                true
            }
        }
    }

    /// 序列化：`len:u16 LE + bytes`。out 不足返回 None。
    pub fn to_bytes(&self, out: &mut [u8]) -> Option<usize> {
        let need = 2 + self.len;
        if out.len() < need {
            return None;
        }
        out[0] = (self.len & 0xFF) as u8;
        out[1] = (self.len >> 8) as u8;
        out[2..need].copy_from_slice(&self.buf[..self.len]);
        Some(need)
    }

    /// 反序列化（round-trip 逆）。
    pub fn from_bytes(buf: &[u8]) -> Option<FixedPath> {
        if buf.len() < 2 {
            return None;
        }
        let n = buf[0] as usize | ((buf[1] as usize) << 8);
        if buf.len() < 2 + n || n > PATH_CAP {
            return None;
        }
        let mut p = FixedPath::new();
        p.buf[..n].copy_from_slice(&buf[2..2 + n]);
        p.len = n;
        Some(p)
    }
}

impl Default for FixedPath {
    fn default() -> Self {
        Self::new()
    }
}

/// 文件列表条目（定容小拷贝体）。
#[derive(Clone, Copy, Debug)]
pub struct FileEntry {
    pub name: [u8; ENTRY_NAME_CAP],
    pub name_len: usize,
    pub is_dir: bool,
}

impl FileEntry {
    pub fn from_name(name: &str, is_dir: bool) -> Option<FileEntry> {
        let b = name.as_bytes();
        if b.len() > ENTRY_NAME_CAP {
            return None;
        }
        let mut e = FileEntry { name: [0; ENTRY_NAME_CAP], name_len: b.len(), is_dir };
        e.name[..b.len()].copy_from_slice(b);
        Some(e)
    }

    pub fn name_str(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("")
    }

    /// 扩展名（最后一个 '.' 之后；无扩展名返回 ""）。
    pub fn ext(&self) -> &str {
        match self.name[..self.name_len].iter().rposition(|b| *b == b'.') {
            Some(i) => core::str::from_utf8(&self.name[i + 1..self.name_len]).unwrap_or(""),
            None => "",
        }
    }
}

/// ASCII 大小写不敏感比较（重名判定的口径）。
fn eq_ignore_case(a: &str, b: &str) -> bool {
    let (x, y) = (a.as_bytes(), b.as_bytes());
    x.len() == y.len()
        && x.iter()
            .zip(y.iter())
            .all(|(p, q)| p.to_ascii_lowercase() == q.to_ascii_lowercase())
}

// ---------------------------------------------------------------------------
// 每应用上次目录记忆（容量上限 + 持久化 round-trip）
// ---------------------------------------------------------------------------

/// 每应用上次目录账本：app id → 路径，容量 [`LASTDIR_CAP`]，
/// 满后环形覆盖最旧（`evictions` 记账）。
pub struct LastDirBook {
    slots: [Option<(u32, FixedPath)>; LASTDIR_CAP],
    cursor: usize,
    pub evictions: u32,
    pub remembered_total: u32,
}

impl LastDirBook {
    pub fn new() -> LastDirBook {
        LastDirBook { slots: [const { None }; LASTDIR_CAP], cursor: 0, evictions: 0, remembered_total: 0 }
    }

    /// 记住某应用本次目录：同 app 覆盖更新，容量满覆盖最旧槽。
    pub fn remember(&mut self, app_id: u32, path: &str) -> bool {
        let p = match FixedPath::from_str(path) {
            Some(p) => p,
            None => return false,
        };
        self.remembered_total += 1;
        if let Some(slot) = self.slots.iter_mut().find(|s| matches!(s, Some((id, _)) if *id == app_id)) {
            *slot = Some((app_id, p));
            return true;
        }
        // 覆盖游标槽（环形淘汰——最旧 = 游标当前位）。
        if self.slots[self.cursor].is_some() {
            self.evictions += 1;
        }
        self.slots[self.cursor] = Some((app_id, p));
        self.cursor = (self.cursor + 1) % LASTDIR_CAP;
        true
    }

    pub fn last_of(&self, app_id: u32) -> Option<&str> {
        self.slots.iter().find_map(|s| match s {
            Some((id, p)) if *id == app_id => Some(p.as_str()),
            _ => None,
        })
    }

    pub fn count(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count()
    }

    /// 持久化：槽位原样序列化（每槽 `app:u32 LE + path(len:u16+bytes)`，
    /// 空槽 = 全零 app + len 0）。返回写入长度；out 不足返回 None。
    pub fn to_bytes(&self, out: &mut [u8]) -> Option<usize> {
        const EMPTY_PATH: FixedPath = FixedPath { buf: [0; PATH_CAP], len: 0 };
        let mut n = 0usize;
        for slot in self.slots.iter() {
            let (id, p) = match slot {
                Some((id, p)) => (*id, p),
                None => (0u32, &EMPTY_PATH),
            };
            if out.len() < n + 6 {
                return None;
            }
            out[n] = (id & 0xFF) as u8;
            out[n + 1] = (id >> 8) as u8;
            out[n + 2] = (id >> 16) as u8;
            out[n + 3] = (id >> 24) as u8;
            let w = p.to_bytes(&mut out[n + 4..])?;
            n += 4 + w;
        }
        Some(n)
    }

    /// 反序列化（round-trip 逆）。长度不符返回 None。
    pub fn from_bytes(buf: &[u8]) -> Option<LastDirBook> {
        let mut book = LastDirBook::new();
        let mut n = 0usize;
        for slot in book.slots.iter_mut() {
            if buf.len() < n + 6 {
                return None;
            }
            let id = buf[n] as u32
                | (buf[n + 1] as u32) << 8
                | (buf[n + 2] as u32) << 16
                | (buf[n + 3] as u32) << 24;
            let p = FixedPath::from_bytes(&buf[n + 4..])?;
            if id != 0 {
                *slot = Some((id, p));
            }
            n += 4 + 2 + p.len();
        }
        if buf.len() != n {
            return None;
        }
        Some(book)
    }
}

impl Default for LastDirBook {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 布局同构几何表（与 F008 兼容层对话框对齐）
// ---------------------------------------------------------------------------

/// 对话框默认尺寸（与 F008 Win32 兼容层打开/保存对话框同构）。
pub const DLG_W: i32 = 720;
pub const DLG_H: i32 = 540;

/// 左侧位置栏宽（F008 对齐常量）。
pub const SIDEBAR_W: i32 = 160;

/// 文件名框高。
pub const NAMEBOX_H: i32 = 32;

/// 格式下拉高。
pub const FILTER_H: i32 = 28;

/// 四区间距。
pub const GAP: i32 = 8;

/// 四区几何（位置栏 / 文件列表 / 文件名框 / 格式下拉）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DlgLayout {
    pub sidebar: Rect,
    pub list: Rect,
    pub namebox: Rect,
    pub filterbox: Rect,
}

/// 由对话框外框算四区几何（常量表驱动，与 F008 走查对齐）。
pub fn layout(dlg: Rect) -> DlgLayout {
    let sidebar = Rect::new(dlg.x + GAP, dlg.y + GAP, SIDEBAR_W, dlg.h - GAP * 2);
    let list_w = dlg.w - SIDEBAR_W - GAP * 3;
    let bottom_h = NAMEBOX_H + GAP + FILTER_H;
    let list = Rect::new(
        sidebar.right() + GAP,
        dlg.y + GAP,
        list_w,
        dlg.h - GAP * 2 - bottom_h - GAP,
    );
    let namebox = Rect::new(list.x, list.bottom() + GAP, list_w, NAMEBOX_H);
    let filterbox = Rect::new(list.x, namebox.bottom() + GAP, list_w, FILTER_H);
    DlgLayout { sidebar, list, namebox, filterbox }
}

// ---------------------------------------------------------------------------
// 对话框状态机
// ---------------------------------------------------------------------------

/// 重名查重防抖状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Debounce {
    /// 无待查编辑。
    Idle,
    /// 编辑中（停顿不足 300ms——不查）。
    Typing,
    /// 已查完（结果在 `dup_conflict`）。
    Done,
}

/// poll 结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PollOutcome {
    /// 无事发生（防抖未到/无待查编辑）。
    Quiet,
    /// 本次触发了查重（携带是否重名）。
    DupChecked(bool),
}

/// 打开/保存双模式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogMode {
    Open,
    Save,
}

/// 统一文件对话框状态机。
pub struct FileDialog {
    mode: DialogMode,
    cwd: FixedPath,
    name: [u8; NAME_CAP],
    name_len: usize,
    filter: [[u8; 8]; FILTER_CAP],
    filter_len: [usize; FILTER_CAP],
    filter_count: usize,
    listing: [FileEntry; LIST_CAP],
    listing_len: usize,
    last_dirs: LastDirBook,
    debounce: Debounce,
    last_edit_ts: u64,
    /// 最近一次查重结果。
    pub dup_conflict: bool,
    pub dup_checks: u32,
    /// 保存模式是否用上了应用建议名（判据记账）。
    pub suggested_name_used: bool,
    pub jumps: u32,
}

impl FileDialog {
    /// 建对话框。保存模式传入应用建议名（自动带入文件名框）。
    pub fn new(mode: DialogMode, suggested: Option<&str>) -> FileDialog {
        let mut dlg = FileDialog {
            mode,
            cwd: FixedPath::new(),
            name: [0; NAME_CAP],
            name_len: 0,
            filter: [[0; 8]; FILTER_CAP],
            filter_len: [0; FILTER_CAP],
            filter_count: 0,
            listing: [FileEntry { name: [0; ENTRY_NAME_CAP], name_len: 0, is_dir: false }; LIST_CAP],
            listing_len: 0,
            last_dirs: LastDirBook::new(),
            debounce: Debounce::Idle,
            last_edit_ts: 0,
            dup_conflict: false,
            dup_checks: 0,
            suggested_name_used: false,
            jumps: 0,
        };
        if mode == DialogMode::Save {
            if let Some(s) = suggested {
                if dlg.set_name(s, 0) {
                    dlg.suggested_name_used = true;
                }
            }
        }
        dlg
    }

    pub fn mode(&self) -> DialogMode {
        self.mode
    }

    pub fn cwd(&self) -> &str {
        self.cwd.as_str()
    }

    pub fn name(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("")
    }

    pub fn debounce(&self) -> Debounce {
        self.debounce
    }

    /// 跳转五位置栏之一。
    pub fn goto_place(&mut self, place: SidePlace) {
        if let Some(p) = FixedPath::from_str(place.root()) {
            self.cwd = p;
            self.jumps += 1;
        }
    }

    /// 跳到任意目录（返回上级/历史路径）。
    pub fn goto_path(&mut self, path: &str) -> bool {
        match FixedPath::from_str(path) {
            Some(p) => {
                self.cwd = p;
                self.jumps += 1;
                true
            }
            None => false,
        }
    }

    /// 编辑文件名框（保存模式）：防抖状态回 Typing，上次查重结论失效。
    /// 超容拒绝（不截断）。
    pub fn set_name(&mut self, s: &str, now_ms: u64) -> bool {
        let b = s.as_bytes();
        if b.len() > NAME_CAP {
            return false;
        }
        self.name[..self.name_len].fill(0);
        self.name[..b.len()].copy_from_slice(b);
        self.name_len = b.len();
        self.debounce = Debounce::Typing;
        self.last_edit_ts = now_ms;
        self.dup_conflict = false;
        true
    }

    /// 装载当前目录文件列表（复用资源管理器组件的装载通路）。
    pub fn seed_listing(&mut self, entries: &[(&str, bool)]) -> usize {
        let mut n = 0usize;
        self.listing_len = 0;
        for (name, is_dir) in entries {
            if n >= LIST_CAP {
                break;
            }
            if let Some(e) = FileEntry::from_name(name, *is_dir) {
                self.listing[n] = e;
                n += 1;
            }
        }
        self.listing_len = n;
        n
    }

    /// 设置格式过滤器白名单（扩展名；超过 FILTER_CAP 条拒绝）。
    pub fn set_filter(&mut self, exts: &[&str]) -> bool {
        if exts.len() > FILTER_CAP {
            return false;
        }
        self.filter_count = 0;
        self.filter_len = [0; FILTER_CAP];
        for (i, e) in exts.iter().enumerate() {
            let b = e.as_bytes();
            if b.len() > 8 {
                return false;
            }
            self.filter[i][..b.len()].copy_from_slice(b);
            self.filter_len[i] = b.len();
            self.filter_count += 1;
        }
        true
    }

    fn filter_matches(&self, entry: &FileEntry) -> bool {
        if entry.is_dir || self.filter_count == 0 {
            return true;
        }
        let e = entry.ext();
        for i in 0..self.filter_count {
            let f = core::str::from_utf8(&self.filter[i][..self.filter_len[i]]).unwrap_or("");
            if eq_ignore_case(e, f) {
                return true;
            }
        }
        false
    }

    /// 过滤后的可见条目名（目录恒可见；诊断/测试面）。
    pub fn visible_names(&self) -> Vec<&str> {
        let mut out = Vec::new();
        for e in self.listing[..self.listing_len].iter() {
            if self.filter_matches(e) {
                out.push(e.name_str());
            }
        }
        out
    }

    /// 逐事件推进：保存模式下键入停顿 ≥300ms 触发查重（防抖状态机）。
    /// 打开模式永远 Quiet（重名提示只属于保存语义）。
    pub fn poll(&mut self, now_ms: u64) -> PollOutcome {
        if self.mode != DialogMode::Save || self.debounce != Debounce::Typing {
            return PollOutcome::Quiet;
        }
        if now_ms.saturating_sub(self.last_edit_ts) < DUP_DEBOUNCE_MS {
            return PollOutcome::Quiet;
        }
        let conflict = self.check_dup();
        self.dup_conflict = conflict;
        self.dup_checks += 1;
        self.debounce = Debounce::Done;
        PollOutcome::DupChecked(conflict)
    }

    fn check_dup(&self) -> bool {
        if self.name_len == 0 {
            return false;
        }
        let cur = self.name();
        self.listing[..self.listing_len]
            .iter()
            .any(|e| !e.is_dir && eq_ignore_case(e.name_str(), cur))
    }

    /// 记住本应用上次目录（转发 LastDirBook）。
    pub fn remember_dir(&mut self, app_id: u32, path: &str) -> bool {
        self.last_dirs.remember(app_id, path)
    }

    /// 查询本应用上次目录。
    pub fn last_dir(&self, app_id: u32) -> Option<&str> {
        self.last_dirs.last_of(app_id)
    }

    /// 目录账本视图（持久化/审计面）。
    pub fn dir_book(&self) -> &LastDirBook {
        &self.last_dirs
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F233 自检（12 条行为级）。
pub fn run_filedlg_checks() -> CheckSet {
    let mut set = CheckSet::new("F233-filedlg");

    // 1. 五位置栏注册表：五项、名字与根路径齐全且互不重复。
    let roots: Vec<&str> = SidePlace::ALL.iter().map(|p| p.root()).collect();
    let unique = (0..roots.len()).all(|i| roots[i + 1..].iter().all(|r| *r != roots[i]));
    set.add(
        "five places registered with unique non-empty roots",
        SidePlace::ALL.len() == 5 && unique && roots.iter().all(|r| !r.is_empty()),
        "",
    );

    // 2. 跳转语义：goto 后 cwd == 位置栏根路径，跳转计数。
    let mut dlg = FileDialog::new(DialogMode::Save, Some("报告.docx"));
    dlg.goto_place(SidePlace::Downloads);
    let at_downloads = dlg.cwd() == SidePlace::Downloads.root() && dlg.jumps == 1;
    dlg.goto_place(SidePlace::ShareS);
    set.add(
        "place-bar jump sets cwd to registered root",
        at_downloads && dlg.cwd() == SidePlace::ShareS.root(),
        "",
    );

    // 3. 保存模式自动带入应用建议名；打开模式文件名框为空。
    set.add(
        "save mode pre-fills suggested name, open mode empty",
        dlg.mode() == DialogMode::Save
            && dlg.suggested_name_used
            && dlg.name() == "报告.docx"
            && FileDialog::new(DialogMode::Open, Some("x.txt")).name().is_empty(),
        "",
    );

    // 4. 重名即时提示（防抖 300ms）：停顿不足不查、到点查出冲突。
    dlg.goto_place(SidePlace::Desktop);
    dlg.seed_listing(&[("报告.docx", false), ("notes.txt", false), ("图片", true)]);
    dlg.set_name("报告.docx", 1_000);
    let early = dlg.poll(1_200) == PollOutcome::Quiet; // 200ms：不查。
    let late = dlg.poll(1_301) == PollOutcome::DupChecked(true); // 301ms：查出。
    set.add(
        "dup check debounced 300ms, conflict detected",
        early && late && dlg.dup_conflict && dlg.dup_checks == 1,
        "",
    );

    // 5. 改名后旧结论失效，重查无冲突；再编辑重置状态。
    dlg.set_name("新文档.docx", 2_000);
    let invalidated = !dlg.dup_conflict;
    let checked = dlg.poll(2_400) == PollOutcome::DupChecked(false);
    dlg.set_name("notes.TXT", 3_000); // 大小写不敏感口径。
    let checked2 = dlg.poll(3_400) == PollOutcome::DupChecked(true);
    set.add(
        "rename resets debounce; case-insensitive dup",
        invalidated && checked && checked2 && dlg.dup_checks == 3,
        "",
    );

    // 6. 打开模式不做重名提示（永远 Quiet）。
    let mut open = FileDialog::new(DialogMode::Open, None);
    open.goto_place(SidePlace::Desktop);
    open.seed_listing(&[("报告.docx", false)]);
    open.set_name("报告.docx", 0);
    set.add(
        "open mode never checks dup",
        open.poll(10_000) == PollOutcome::Quiet && open.dup_checks == 0,
        "",
    );

    // 7. 每应用上次目录记忆：写入可查、同 app 覆盖更新。
    let mut book = LastDirBook::new();
    assert!(book.remember(1, "/home/user/Docs"));
    assert!(book.remember(2, "/mnt/s-share/项目"));
    let ok1 = book.last_of(1) == Some("/home/user/Docs") && book.last_of(2).is_some();
    assert!(book.remember(1, "/home/user/Music"));
    set.add(
        "per-app last-dir memory & overwrite update",
        ok1 && book.last_of(1) == Some("/home/user/Music") && book.count() == 2,
        "",
    );

    // 8. 容量上限：第 17 个应用淘汰最旧槽并记账。
    let mut book2 = LastDirBook::new();
    for i in 0..LASTDIR_CAP as u32 {
        assert!(book2.remember(100 + i, "/tmp/a"));
    }
    let ev0 = book2.evictions;
    // [缺陷账本] 现象：eviction 检查项红（项名前缀 ".as_bytes()" 为日志
    // 拼接痕迹，一并清理）。根因：检查项断言 last_of(999) == "/tmp/b"，
    // 而上一行 remember 存的是 "/tmp/"——断言引用了从未写入的路径
    // （草稿残留），淘汰语义本身正确。修法：改检查项，断言对齐实存值。
    assert!(book2.remember(999, "/tmp/")); // 覆盖 app 100。
    set.add(
        "last-dir capacity ring eviction",
        book2.count() == LASTDIR_CAP && book2.evictions == ev0 + 1 && book2.last_of(100).is_none()
            && book2.last_of(999) == Some("/tmp/"),
        "",
    );

    // 9. 持久化边界 round-trip：to_bytes → from_bytes 逐槽还原。
    let mut buf = [0u8; 1024];
    let n = book.to_bytes(&mut buf).unwrap();
    let restored = LastDirBook::from_bytes(&buf[..n]).unwrap();
    let same = (0..4u32).all(|i| restored.last_of(i) == book.last_of(i));
    set.add(
        "last-dir persistence round-trip",
        same && restored.count() == book.count() && restored.last_of(1) == Some("/home/user/Music"),
        "",
    );

    // 10. 格式过滤器：白名单外不可见，目录恒可见，大小写不敏感。
    let mut fd = FileDialog::new(DialogMode::Open, None);
    fd.set_filter(&["txt", "md"]);
    fd.seed_listing(&[("a.txt", false), ("b.EXE", false), ("c.MD", false), ("dir", true), ("noext", false)]);
    let vis = fd.visible_names();
    set.add(
        "format filter whitelist (dirs always visible)",
        vis.contains(&"a.txt") && vis.contains(&"c.MD") && vis.contains(&"dir") && !vis.contains(&"b.EXE")
            && !vis.contains(&"noext") && vis.len() == 3,
        "",
    );

    // 11. 布局同构几何：四区两两不重叠、全在对话框内、位置栏宽对齐常量。
    let dlg_rect = Rect::new(0, 0, DLG_W, DLG_H);
    let lay = layout(dlg_rect);
    let inside = |r: &Rect| {
        r.x >= dlg_rect.x && r.y >= dlg_rect.y && r.right() <= dlg_rect.right() && r.bottom() <= dlg_rect.bottom()
    };
    let no_overlap = lay.sidebar.intersect_area(&lay.list) == 0
        && lay.list.intersect_area(&lay.namebox) == 0
        && lay.namebox.intersect_area(&lay.filterbox) == 0
        && lay.sidebar.intersect_area(&lay.namebox) == 0;
    set.add(
        "layout zones aligned (F008), non-overlapping, inside dialog",
        lay.sidebar.w == SIDEBAR_W && inside(&lay.sidebar) && inside(&lay.list) && inside(&lay.namebox)
            && inside(&lay.filterbox) && no_overlap,
        "",
    );

    // 12. fuzz 2000 轮：随机 编辑/poll/跳转/记忆/过滤，不变量——
    //     文件名 ≤ 容量、dup_checks 只随 ≥300ms 的 poll 增长、
    //     目录账本 ≤ 容量、无 panic。
    let mut x: u32 = 0xD1A1_0608;
    let mut fz = FileDialog::new(DialogMode::Save, Some("draft.txt"));
    fz.goto_place(SidePlace::Desktop);
    fz.seed_listing(&[("draft.txt", false), ("old.txt", false)]);
    fz.set_filter(&["txt"]);
    let mut now: u64 = 0;
    let mut last_edit: u64 = 0;
    let mut checks: u32 = 0;
    let mut ok = true;
    for i in 0..2000u32 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let op = x % 4;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        now += (x % 500) as u64;
        match op {
            0 => {
                let n = (x % 8) as usize;
                let names = ["a.txt", "b.TXT", "draft.txt", "zzz", "长文件名测试.docx", "c", "d.txt", "e"];
                if fz.set_name(names[n], now) {
                    last_edit = now;
                }
            }
            1 => {
                if let PollOutcome::DupChecked(_) = fz.poll(now) {
                    // 查重必须在编辑后 ≥300ms 才允许发生。
                    if now.saturating_sub(last_edit) < DUP_DEBOUNCE_MS {
                        ok = false;
                        break;
                    }
                    checks += 1;
                }
            }
            2 => {
                let places = SidePlace::ALL;
                fz.goto_place(places[(x % 5) as usize]);
            }
            _ => {
                fz.remember_dir(x % 24, "/tmp/fuzz");
            }
        }
        if fz.name().len() > NAME_CAP || fz.dir_book().count() > LASTDIR_CAP {
            ok = false;
            break;
        }
        let _ = i;
    }
    set.add(
        "fuzz 2000 rounds: invariants hold, no panic",
        ok && fz.dup_checks >= checks,
        "",
    );

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_path_push_pop_and_cap() {
        let mut p = FixedPath::from_str("/home").unwrap();
        assert!(p.push_component("user"));
        assert_eq!(p.as_str(), "/home/user");
        assert!(p.push_component("Docs"));
        assert_eq!(p.as_str(), "/home/user/Docs");
        assert!(p.pop());
        assert_eq!(p.as_str(), "/home/user");
        assert!(p.pop());
        assert_eq!(p.as_str(), "/home");
        assert!(p.pop());
        assert_eq!(p.as_str(), "/");
        assert!(!p.pop(), "根路径不再弹");
        // 超容拒绝。
        let long = "x".repeat(PATH_CAP + 1);
        assert!(FixedPath::from_str(&long).is_none());
    }

    #[test]
    fn path_serialization_roundtrip() {
        let p = FixedPath::from_str("/mnt/s-share/项目/子目录").unwrap();
        let mut buf = [0u8; 128];
        let n = p.to_bytes(&mut buf).unwrap();
        let q = FixedPath::from_bytes(&buf[..n]).unwrap();
        assert_eq!(p, q);
        // 空路径 round-trip。
        let e = FixedPath::new();
        let n2 = e.to_bytes(&mut buf).unwrap();
        assert_eq!(FixedPath::from_bytes(&buf[..n2]).unwrap(), e);
        // 缓冲不足显性失败。
        assert!(p.to_bytes(&mut [0u8; 4]).is_none());
    }

    #[test]
    fn suggested_name_and_dup_flow() {
        let mut dlg = FileDialog::new(DialogMode::Save, Some("会议纪要.md"));
        assert_eq!(dlg.name(), "会议纪要.md");
        assert!(dlg.suggested_name_used);
        dlg.goto_place(SidePlace::Favorites);
        dlg.seed_listing(&[("会议纪要.md", false)]);
        dlg.set_name("会议纪要.md", 0);
        assert_eq!(dlg.poll(299), PollOutcome::Quiet);
        assert_eq!(dlg.poll(300), PollOutcome::DupChecked(true));
        // 改名 → 防抖重置 → 无冲突。
        dlg.set_name("其他.md", 1_000);
        assert_eq!(dlg.debounce(), Debounce::Typing);
        assert_eq!(dlg.poll(1_299), PollOutcome::Quiet);
        assert_eq!(dlg.poll(1_300), PollOutcome::DupChecked(false));
    }

    #[test]
    fn name_cap_rejects_oversize() {
        let mut dlg = FileDialog::new(DialogMode::Save, None);
        let long = "n".repeat(NAME_CAP + 1);
        assert!(!dlg.set_name(&long, 0), "超长文件名拒绝不截断");
        assert!(dlg.name().is_empty());
        let exact = "n".repeat(NAME_CAP);
        assert!(dlg.set_name(&exact, 0));
        assert_eq!(dlg.name().len(), NAME_CAP);
    }

    #[test]
    fn entry_ext_extraction() {
        let e = FileEntry::from_name("archive.tar.gz", false).unwrap();
        assert_eq!(e.ext(), "gz");
        let e2 = FileEntry::from_name("Makefile", false).unwrap();
        assert_eq!(e2.ext(), "");
        assert!(eq_ignore_case("TXT", "txt"));
        assert!(!eq_ignore_case("txt", "md"));
    }

    #[test]
    fn filedlg_selfcheck_all_green() {
        let s = run_filedlg_checks();
        assert!(s.all_passed(), "F233 自检存在红项");
        assert!(!s.truncated());
    }
}

// ===========================================================================
// v2 深化批（2026-09-26 · AI-H1 二次对账批）：UI 壳接线 / 持久化 I/O / 判定面扩展
// ===========================================================================
//
// 判据锚 F233。持久化面 = 上次目录记忆（分窗格/按应用）framed 记录；
// 壳接线面 = 位置栏五入口命中测试 + 重名探测纯函数（大小写折叠）；
// 判定面 = run_filedlg_v2_checks（首条持久化 round-trip）。

// -- 持久化 I/O 面 ---------------------------------------------------------

/// v2 记录头 magic「VXH1」+ 版本（全域 v2 段统一）。
pub const V2_MAGIC: [u8; 4] = *b"VXH1";
pub const V2_VERSION: u8 = 1;

/// 四类损坏显性拒绝。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum V2SaveErr {
    BadMagic,
    BadVersion,
    BadLen,
    BadChecksum,
}

/// FNV-1a 64 位取低 32 位（常数与 vdesk 音频指纹同族）。
fn v2_fnv1a32(data: &[u8]) -> u32 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h as u32
}

/// 记录容量上限在册：count u8 + LASTDIR_CAP 槽 × (app u32 + len u16
/// + PATH_CAP 路径) + checksum u32（变长记录，按实际槽数写）。
pub const V2_PAYLOAD_MAX: usize = 1 + LASTDIR_CAP * (6 + PATH_CAP);
pub const V2_REC_MAX: usize = 5 + V2_PAYLOAD_MAX + 4;

/// 上次目录持久化记录（主册 F233 v2：分窗格记忆落盘——每槽带
/// 窗格所属应用 id，重启后各窗格回各自上次目录）。
#[derive(Clone, Copy, Debug)]
pub struct V2LastDirRec {
    /// (窗格所属应用 id, 上次目录)。定容 LASTDIR_CAP。
    pub slots: [(u32, FixedPath); LASTDIR_CAP],
    pub count: usize,
}

impl V2LastDirRec {
    /// 从账本点名导出（账本不暴露迭代器——调用方持键集按名取）。
    pub fn capture(book: &LastDirBook, app_ids: &[u32]) -> V2LastDirRec {
        let mut rec = V2LastDirRec { slots: [(0, FixedPath::new()); LASTDIR_CAP], count: 0 };
        for id in app_ids.iter().take(LASTDIR_CAP) {
            if let Some(p) = book.last_of(*id) {
                if let Some(fp) = FixedPath::from_str(p) {
                    rec.slots[rec.count] = (*id, fp);
                    rec.count += 1;
                }
            }
        }
        rec
    }

    /// 编码：magic+ver+count+逐槽(app+path)+checksum。
    pub fn to_bytes(&self, out: &mut [u8]) -> Option<usize> {
        if out.len() < 6 {
            return None;
        }
        out[..4].copy_from_slice(&V2_MAGIC);
        out[4] = V2_VERSION;
        out[5] = self.count as u8;
        let mut n = 6usize;
        for i in 0..self.count {
            if out.len() < n + 6 {
                return None;
            }
            out[n..n + 4].copy_from_slice(&self.slots[i].0.to_le_bytes());
            let w = self.slots[i].1.to_bytes(&mut out[n + 4..])?;
            n += 4 + w;
        }
        if out.len() < n + 4 {
            return None;
        }
        let sum = v2_fnv1a32(&out[..n]);
        out[n..n + 4].copy_from_slice(&sum.to_le_bytes());
        Some(n + 4)
    }

    /// 解码：magic/版本/长度/校验和四类逐一拒绝；槽数超容拒绝。
    pub fn from_bytes(buf: &[u8]) -> Result<V2LastDirRec, V2SaveErr> {
        if buf.len() < 10 {
            return Err(V2SaveErr::BadLen);
        }
        if buf[..4] != V2_MAGIC {
            return Err(V2SaveErr::BadMagic);
        }
        if buf[4] != V2_VERSION {
            return Err(V2SaveErr::BadVersion);
        }
        let body = buf.len() - 4;
        let sum = u32::from_le_bytes([buf[body], buf[body + 1], buf[body + 2], buf[body + 3]]);
        if v2_fnv1a32(&buf[..body]) != sum {
            return Err(V2SaveErr::BadChecksum);
        }
        let count = buf[5] as usize;
        if count > LASTDIR_CAP {
            return Err(V2SaveErr::BadLen);
        }
        let mut rec = V2LastDirRec { slots: [(0, FixedPath::new()); LASTDIR_CAP], count: 0 };
        let mut n = 6usize;
        for _ in 0..count {
            if n + 6 > body {
                return Err(V2SaveErr::BadLen);
            }
            let id = u32::from_le_bytes([buf[n], buf[n + 1], buf[n + 2], buf[n + 3]]);
            let p = match FixedPath::from_bytes(&buf[n + 4..body]) {
                Some(p) => p,
                None => return Err(V2SaveErr::BadLen),
            };
            n += 4 + 2 + p.len();
            if n > body {
                return Err(V2SaveErr::BadLen);
            }
            rec.slots[rec.count] = (id, p);
            rec.count += 1;
        }
        if n != body {
            return Err(V2SaveErr::BadLen);
        }
        Ok(rec)
    }

    /// 查某窗格的上次目录（与 LastDirBook::last_of 同语义）。
    pub fn last_of(&self, app_id: u32) -> Option<&str> {
        self.slots[..self.count]
            .iter()
            .find_map(|(id, p)| if *id == app_id { Some(p.as_str()) } else { None })
    }
}

// -- UI 壳接线面 -----------------------------------------------------------

/// 位置栏五入口命中测试：侧栏竖排五等分行（自上而下 = SidePlace::ALL
/// 序）。栏外点返回 None；行高不足 1px 时钳制到首行（防除零）。
pub fn v2_place_hit(sidebar: &Rect, px: i32, py: i32) -> Option<SidePlace> {
    if !sidebar.contains(px, py) {
        return None;
    }
    let row_h = (sidebar.h / 5).max(1);
    let row = ((py - sidebar.y) / row_h).clamp(0, 4) as usize;
    Some(SidePlace::ALL[row])
}

/// 重名探测纯函数（主册「重名即时提示不等到点保存才报」的判定核：
/// 大小写折叠口径与对话框查重一致——复用私有 eq_ignore_case，
/// 一处一事实）。目录不参与重名（与 check_dup 同口径）。
pub fn v2_dup_probe(name: &str, listing: &[FileEntry]) -> bool {
    if name.is_empty() {
        return false;
    }
    listing.iter().any(|e| !e.is_dir && eq_ignore_case(e.name_str(), name))
}

// -- 判定面扩展 ------------------------------------------------------------

/// F233 v2 自检（首条必为持久化 round-trip）。
pub fn run_filedlg_v2_checks() -> CheckSet {
    let mut set = CheckSet::new("F233-filedlg-v2");

    // 1. 持久化 round-trip（验主册「对话框记住每应用上次目录」落盘还原）。
    let mut book = LastDirBook::new();
    let _ = book.remember(11, "/home/user/Docs");
    let _ = book.remember(22, "/mnt/s-share/项目");
    let rec = V2LastDirRec::capture(&book, &[11, 22, 33]);
    let mut buf = [0u8; V2_REC_MAX];
    let wrote = rec.to_bytes(&mut buf).unwrap_or(0);
    let back = V2LastDirRec::from_bytes(&buf[..wrote]);
    set.add(
        "v2 persist round-trip: last-dir record",
        wrote > 0
            && back.as_ref().map(|r| r.count).unwrap_or(0) == 2
            && back.as_ref().ok().and_then(|r| r.last_of(11)) == Some("/home/user/Docs")
            && back.as_ref().ok().and_then(|r| r.last_of(33)).is_none(),
        "",
    );

    // 2. 四类损坏全拒绝（magic/版本/长度/校验和逐一显性报错）。
    //    [缺陷账本] 现象：BadLen 分支红。根因：检查项用「截短 1 字节」
    //    构造长度损坏，但实现先验校验和后逐槽解析（变长记录的合法
    //    顺序），截短必先撞 BadChecksum——BadLen 分支根本没被测到，
    //    属检查项构造缺陷（实现四分支齐全）。修法：改检查项——翻
    //    count 字节到容量外并重算校验和（校验通过、仅长度语义非法，
    //    真测 BadLen 分支）。
    let mut b1 = buf;
    b1[0] = b'X';
    let mut b2 = buf;
    b2[4] = 3;
    let mut b3 = [0u8; V2_REC_MAX];
    b3[..wrote].copy_from_slice(&buf[..wrote]);
    b3[5] = (LASTDIR_CAP + 1) as u8; // count 越界（> LASTDIR_CAP）
    let body = wrote - 4;
    let sum = v2_fnv1a32(&b3[..body]);
    b3[body..body + 4].copy_from_slice(&sum.to_le_bytes());
    let mut b4 = buf;
    b4[10] ^= 0xFF; // 翻载荷字节（校验和覆盖域内）→ BadChecksum
    let last = wrote.max(1) - 1;
    set.add(
        "v2 persist rejects magic/version/len/checksum",
        wrote > 0
            && matches!(V2LastDirRec::from_bytes(&b1), Err(V2SaveErr::BadMagic))
            && matches!(V2LastDirRec::from_bytes(&b2), Err(V2SaveErr::BadVersion))
            && matches!(V2LastDirRec::from_bytes(&b3[..wrote]), Err(V2SaveErr::BadLen))
            && matches!(V2LastDirRec::from_bytes(&b4), Err(V2SaveErr::BadChecksum)),
        "",
    );

    // 3. 位置栏五入口命中测试（验主册「左侧五位置栏」：五等分行逐行
    //    命中、序与 SidePlace::ALL 一致、栏外 None）。
    let sidebar = Rect::new(8, 8, SIDEBAR_W, 250);
    let mut hit_ok = true;
    for (i, want) in SidePlace::ALL.iter().enumerate() {
        let py = 8 + i as i32 * 50 + 10;
        hit_ok &= v2_place_hit(&sidebar, 100, py) == Some(*want);
    }
    set.add(
        "v2 place-bar hit test: five rows in order",
        hit_ok && v2_place_hit(&sidebar, 0, 0).is_none(),
        "",
    );

    // 4. 重名探测（验主册「重名即时提示」判定核：大小写折叠命中、
    //    目录不参与、空名不报）。
    let listing = [
        FileEntry::from_name("报告.docx", false).unwrap_or(FileEntry { name: [0; ENTRY_NAME_CAP], name_len: 0, is_dir: false }),
        FileEntry::from_name("图片", true).unwrap_or(FileEntry { name: [0; ENTRY_NAME_CAP], name_len: 0, is_dir: false }),
    ];
    set.add(
        "v2 dup probe: case-folded, dirs excluded",
        v2_dup_probe("报告.DOCX", &listing)
            && !v2_dup_probe("其他.docx", &listing)
            && !v2_dup_probe("图片", &listing)
            && !v2_dup_probe("", &listing),
        "",
    );

    // 5. 记录查口与账本一致（capture 点名导出 = 账本逐键读——一处一事实）。
    set.add(
        "v2 record mirrors book per-app",
        rec.last_of(11) == book.last_of(11) && rec.last_of(22) == book.last_of(22),
        "",
    );

    set
}

#[cfg(test)]
mod tests_v2 {
    use super::*;

    #[test]
    fn v2_lastdir_roundtrip_exact() {
        let mut book = LastDirBook::new();
        assert!(book.remember(7, "/home/user/Music"));
        let rec = V2LastDirRec::capture(&book, &[7]);
        let mut buf = [0u8; V2_REC_MAX];
        let n = rec.to_bytes(&mut buf).unwrap();
        let back = V2LastDirRec::from_bytes(&buf[..n]).unwrap();
        assert_eq!(back.count, 1);
        assert_eq!(back.last_of(7), Some("/home/user/Music"));
    }

    #[test]
    fn v2_place_hit_edges() {
        let sidebar = Rect::new(0, 0, 160, 100);
        assert_eq!(v2_place_hit(&sidebar, 0, 0), Some(SidePlace::Desktop));
        assert_eq!(v2_place_hit(&sidebar, 159, 99), Some(SidePlace::Favorites));
        assert!(v2_place_hit(&sidebar, 160, 50).is_none());
    }

    #[test]
    fn filedlg_v2_selfcheck_all_green() {
        let s = run_filedlg_v2_checks();
        assert!(s.all_passed(), "F233 v2 自检存在红项");
        assert!(!s.truncated());
    }
}
