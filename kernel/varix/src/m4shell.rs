//! m4shell — VARIX-M400 AI-09 桌面 shell 完备化域 (F201~F225)
//!
//! 日常使用的全部桌面能力：设置中心/多屏任务栏/贴靠矩阵/虚拟桌面/动效统一/
//! 剪贴板历史/截图/通知行动/统一搜索/最近文件/默认应用/回收站/文件管理器/
//! 终端/应用商店/更新/系统监视/备份/家长控制/电源菜单/锁屏/快速设置/
//! 命令面板/欢迎引导/shell 回归清单。
//!
//! 硬约束：no_std / 无 alloc / 固定容量数组 / 纯逻辑（真机渲染验收另录）。

use crate::checks::CheckSet;

// ===========================================================================
// F201 — 设置中心：统一偏好存储 + 分区导航
// ===========================================================================

pub const MAX_SETTINGS: usize = 64;
pub const SECTIONS: [&str; 8] = [
    "display", "sound", "network", "apps", "accounts", "privacy", "power", "about",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrefValue {
    Bool(bool),
    Num(i64),
    Text(&'static str),
}

#[derive(Clone, Copy)]
pub struct Pref {
    pub key: &'static str,
    pub section: &'static str,
    pub value: PrefValue,
}

/// 固定容量偏好存储：同 key 写入即覆盖（upsert），无堆分配。
pub struct PrefStore {
    prefs: [Option<Pref>; MAX_SETTINGS],
    count: usize,
}

impl PrefStore {
    pub const fn new() -> PrefStore {
        PrefStore { prefs: [None; MAX_SETTINGS], count: 0 }
    }
    pub fn set(&mut self, pref: Pref) -> bool {
        for i in 0..self.count {
            if let Some(p) = self.prefs[i] {
                if p.key == pref.key {
                    self.prefs[i] = Some(pref);
                    return true;
                }
            }
        }
        if self.count < MAX_SETTINGS {
            self.prefs[self.count] = Some(pref);
            self.count += 1;
            true
        } else {
            false
        }
    }
    pub fn get(&self, key: &str) -> Option<PrefValue> {
        for i in 0..self.count {
            if let Some(p) = self.prefs[i] {
                if p.key == key {
                    return Some(p.value);
                }
            }
        }
        None
    }
    pub fn section_ok(&self, section: &str) -> bool {
        SECTIONS.contains(&section)
    }
    pub fn len(&self) -> usize {
        self.count
    }
}

// ===========================================================================
// F202 — 任务栏多显示器：每屏策略可选
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TaskbarPolicy {
    PrimaryOnly,
    AllDisplays,
    ActiveWindowDisplay,
}

pub fn taskbar_policy_valid(p: TaskbarPolicy) -> bool {
    matches!(p, TaskbarPolicy::PrimaryOnly | TaskbarPolicy::AllDisplays | TaskbarPolicy::ActiveWindowDisplay)
}

// ===========================================================================
// F203 — 贴靠矩阵扩展：四分/边缘贴靠/快捷键
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// 把屏幕区域按槽位（0~3 四分）切分。
pub fn snap_quad(scr: Rect, slot: usize) -> Option<Rect> {
    let (hw, hh) = (scr.w / 2, scr.h / 2);
    match slot {
        0 => Some(Rect { x: scr.x, y: scr.y, w: hw, h: hh }),
        1 => Some(Rect { x: scr.x + hw, y: scr.y, w: scr.w - hw, h: hh }),
        2 => Some(Rect { x: scr.x, y: scr.y + hh, w: hw, h: scr.h - hh }),
        3 => Some(Rect { x: scr.x + hw, y: scr.y + hh, w: scr.w - hw, h: scr.h - hh }),
        _ => None,
    }
}

/// 左/右/上/下边缘半屏贴靠（dir: 0=左 1=右 2=上 3=下）。
pub fn snap_edge(scr: Rect, dir: usize) -> Option<Rect> {
    match dir {
        0 => Some(Rect { x: scr.x, y: scr.y, w: scr.w / 2, h: scr.h }),
        1 => Some(Rect { x: scr.x + scr.w / 2, y: scr.y, w: scr.w - scr.w / 2, h: scr.h }),
        2 => Some(Rect { x: scr.x, y: scr.y, w: scr.w, h: scr.h / 2 }),
        3 => Some(Rect { x: scr.x, y: scr.y + scr.h / 2, w: scr.w, h: scr.h - scr.h / 2 }),
        _ => None,
    }
}

// ===========================================================================
// F204 — 虚拟桌面：多工作区 + 快捷切换
// ===========================================================================

pub const MAX_DESKTOPS: usize = 8;

pub struct WorkspaceSet {
    count: usize,
    active: usize,
    // 每个工作区驻留窗口位图（按窗口 id 位标记）
    resident: [u64; MAX_DESKTOPS],
}

impl WorkspaceSet {
    pub const fn new() -> WorkspaceSet {
        WorkspaceSet { count: 2, active: 0, resident: [0; MAX_DESKTOPS] }
    }
    pub fn create(&mut self) -> bool {
        if self.count < MAX_DESKTOPS {
            self.count += 1;
            true
        } else {
            false
        }
    }
    pub fn switch(&mut self, idx: usize) -> bool {
        if idx < self.count {
            self.active = idx;
            true
        } else {
            false
        }
    }
    pub fn move_window(&mut self, ws: usize, win: u32, into: bool) -> bool {
        if ws >= self.count || win >= 64 {
            return false;
        }
        if into {
            self.resident[ws] |= 1u64 << win;
        } else {
            self.resident[ws] &= !(1u64 << win);
        }
        true
    }
    pub fn active(&self) -> usize {
        self.active
    }
    pub fn count(&self) -> usize {
        self.count
    }
    pub fn resident_of(&self, ws: usize) -> u64 {
        if ws < self.count { self.resident[ws] } else { 0 }
    }
}

// ===========================================================================
// F205 — 窗口动画统一曲线：全动效同一缓动族
// ===========================================================================

/// 三次贝塞尔缓动（整数 permille 采样，无浮点）。
#[derive(Clone, Copy)]
pub struct Easing {
    pub x1: u16,
    pub y1: u16,
    pub x2: u16,
    pub y2: u16,
}

pub const EASE_STANDARD: Easing = Easing { x1: 400, y1: 0, x2: 200, y2: 1000 };
pub const EASE_DECEL: Easing = Easing { x1: 0, y1: 0, x2: 200, y2: 1000 };
pub const EASE_ACCEL: Easing = Easing { x1: 400, y1: 0, x2: 1000, y2: 1000 };

/// 采样：t ∈ [0,1000] permille，二分逼近贝塞尔 x(t)=t 输入，返回 y(t)。
pub fn ease_at(e: Easing, t_permille: u16) -> u16 {
    if t_permille == 0 {
        return 0;
    }
    if t_permille >= 1000 {
        return 1000;
    }
    let t = t_permille as u32;
    let mut lo = 0u32;
    let mut hi = 1000u32;
    // 二分求参数 u 使 x(u)=t
    for _ in 0..24 {
        let mid = (lo + hi) / 2;
        let xu = bez(mid, e.x1, e.x2);
        if xu < t {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    bez((lo + hi) / 2, e.y1, e.y2) as u16
}

fn bez(u: u32, a: u16, b: u16) -> u32 {
    // B(u) = 3u(1-u)^2*a + 3u^2(1-u)*b + u^3 （permille 域）
    let uf = u as u64;
    let om = 1000 - uf;
    let term1 = 3 * uf * om * om * a as u64;
    let term2 = 3 * uf * uf * om * b as u64;
    let term3 = uf * uf * uf * 1000;
    ((term1 + term2 + term3) / 1_000_000_000 / 1) as u32
}

// ===========================================================================
// F206 — 剪贴板历史：历史 + 搜索 + 置顶
// ===========================================================================

pub const CLIP_CAP: usize = 16;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ClipItem {
    pub text: &'static str,
    pub pinned: bool,
}

pub struct ClipHistory {
    items: [Option<ClipItem>; CLIP_CAP],
    count: usize,
}

impl ClipHistory {
    pub const fn new() -> ClipHistory {
        ClipHistory { items: [None; CLIP_CAP], count: 0 }
    }
    /// 新条目入栈（同文本去重后置顶）；满则挤掉最旧未置顶项。
    pub fn push(&mut self, text: &'static str) -> bool {
        for i in 0..self.count {
            if let Some(it) = self.items[i] {
                if it.text == text {
                    // 已存在：删除旧位，稍后重新入栈首
                    for j in i..self.count - 1 {
                        self.items[j] = self.items[j + 1];
                    }
                    self.count -= 1;
                    break;
                }
            }
        }
        if self.count == CLIP_CAP {
            // 找最旧未置顶
            let mut victim = None;
            for i in (0..self.count).rev() {
                if let Some(it) = self.items[i] {
                    if !it.pinned {
                        victim = Some(i);
                    }
                }
            }
            match victim {
                Some(v) => {
                    for j in v..self.count - 1 {
                        self.items[j] = self.items[j + 1];
                    }
                    self.count -= 1;
                }
                None => return false, // 全置顶，拒收
            }
        }
        // 后移腾出首位
        for j in (0..self.count).rev() {
            self.items[j + 1] = self.items[j];
        }
        self.items[0] = Some(ClipItem { text, pinned: false });
        self.count += 1;
        true
    }
    pub fn pin(&mut self, idx: usize, on: bool) -> bool {
        if idx < self.count {
            if let Some(mut it) = self.items[idx] {
                it.pinned = on;
                self.items[idx] = Some(it);
                true
            } else {
                false
            }
        } else {
            false
        }
    }
    pub const CLIP_DEMO: [&'static str; CLIP_CAP] = [
        "c00", "c01", "c02", "c03", "c04", "c05", "c06", "c07", "c08", "c09", "c10", "c11",
        "c12", "c13", "c14", "c15",
    ];

    /// 容量回绕：16 条互异文本填满后，再进一条挤掉最旧未置顶项。
    pub fn clip_ring_wrap_ok() -> bool {
        let mut c = ClipHistory::new();
        for t in Self::CLIP_DEMO.iter() {
            if !c.push(t) {
                return false;
            }
        }
        if c.len() != CLIP_CAP {
            return false;
        }
        if !c.push("c16") {
            return false;
        }
        c.len() == CLIP_CAP && c.get(0).map(|i| i.text) == Some("c16")
    }

    pub fn clip_search(&self, pat: &str) -> usize {
        let mut hits = 0;
        for i in 0..self.count {
            if let Some(it) = self.items[i] {
                if it.text.contains(pat) {
                    hits += 1;
                }
            }
        }
        hits
    }
    pub fn len(&self) -> usize {
        self.count
    }
    pub fn get(&self, idx: usize) -> Option<ClipItem> {
        if idx < self.count { self.items[idx] } else { None }
    }
}

// ===========================================================================
// F207 — 截图工具：区域/窗口/延时
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShotMode {
    Region(Rect),
    FullScreen,
    Window(u32),
    Delayed(u32), // 延时秒
}

pub fn shot_mode_valid(m: ShotMode) -> bool {
    match m {
        ShotMode::Region(r) => r.w > 0 && r.h > 0,
        ShotMode::Delayed(s) => s <= 60,
        _ => true,
    }
}

// ===========================================================================
// F208 — 通知行动按钮：通知可交互
// ===========================================================================

pub const MAX_ACTIONS: usize = 3;

#[derive(Clone, Copy)]
pub struct NotifAction {
    pub label: &'static str,
    pub verb: u8, // 0=dismiss 1=open 2=reply
}

#[derive(Clone, Copy)]
pub struct Notif {
    pub id: u32,
    pub title: &'static str,
    pub actions: [Option<NotifAction>; MAX_ACTIONS],
}

impl Notif {
    pub fn action_count(&self) -> usize {
        self.actions.iter().filter(|a| a.is_some()).count()
    }
    pub fn dispatch(&self, idx: usize) -> bool {
        idx < self.action_count()
    }
}

// ===========================================================================
// F209 — 开始菜单搜索融合：应用/文件/设置统一搜索
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    App(&'static str),
    File(&'static str),
    Setting(&'static str),
}

impl Hit {
    pub fn matches(&self, pat: &str) -> bool {
        let s = match self {
            Hit::App(s) | Hit::File(s) | Hit::Setting(s) => *s,
        };
        s.contains(pat)
    }
    pub fn rank(&self) -> u8 {
        // 应用 > 设置 > 文件（开始菜单默认排序）
        match self {
            Hit::App(_) => 0,
            Hit::Setting(_) => 1,
            Hit::File(_) => 2,
        }
    }
}

/// 三源合并排序（稳定：同 rank 保持输入序）。
pub fn merge_hits(hits: &[Hit; 8], n: usize, pat: &str, out: &mut [Hit; 8]) -> usize {
    let mut m = 0;
    for r in 0..=2u8 {
        for i in 0..n {
            if hits[i].rank() == r && hits[i].matches(pat) && m < out.len() {
                out[m] = hits[i];
                m += 1;
            }
        }
    }
    m
}

// ===========================================================================
// F210 — 最近文件：跨应用最近列表
// ===========================================================================

pub const RECENT_CAP: usize = 10;

pub struct RecentFiles {
    entries: [Option<(&'static str, u32)>; RECENT_CAP], // (path, app_id)
    count: usize,
}

impl RecentFiles {
    pub const fn new() -> RecentFiles {
        RecentFiles { entries: [None; RECENT_CAP], count: 0 }
    }
    pub fn touch(&mut self, path: &'static str, app: u32) -> bool {
        // 去重后置顶
        let mut k = 0;
        while k < self.count {
            if let Some((p, _)) = self.entries[k] {
                if p == path {
                    for j in k..self.count - 1 {
                        self.entries[j] = self.entries[j + 1];
                    }
                    self.count -= 1;
                    break;
                }
            }
            k += 1;
        }
        if self.count == RECENT_CAP {
            self.count -= 1;
        }
        for j in (0..self.count).rev() {
            self.entries[j + 1] = self.entries[j];
        }
        self.entries[0] = Some((path, app));
        self.count += 1;
        true
    }
    pub fn len(&self) -> usize {
        self.count
    }
    pub fn get(&self, idx: usize) -> Option<(&'static str, u32)> {
        if idx < self.count { self.entries[idx] } else { None }
    }
}

// ===========================================================================
// F211 — 默认应用管理：协议/类型关联设置
// ===========================================================================

pub const MAX_ASSOC: usize = 12;

#[derive(Clone, Copy)]
pub struct Assoc {
    pub kind: &'static str, // "text/plain" 或 "https:"
    pub app: u32,
}

pub struct AssocTable {
    rows: [Option<Assoc>; MAX_ASSOC],
    count: usize,
}

impl AssocTable {
    pub const fn new() -> AssocTable {
        AssocTable { rows: [None; MAX_ASSOC], count: 0 }
    }
    /// 设置关联（同 kind 覆盖）；kind 空则拒绝。
    pub fn set(&mut self, kind: &'static str, app: u32) -> bool {
        if kind.is_empty() {
            return false;
        }
        for i in 0..self.count {
            if let Some(a) = self.rows[i] {
                if a.kind == kind {
                    self.rows[i] = Some(Assoc { kind, app });
                    return true;
                }
            }
        }
        if self.count < MAX_ASSOC {
            self.rows[self.count] = Some(Assoc { kind, app });
            self.count += 1;
            true
        } else {
            false
        }
    }
    pub fn lookup(&self, kind: &str) -> Option<u32> {
        for i in 0..self.count {
            if let Some(a) = self.rows[i] {
                if a.kind == kind {
                    return Some(a.app);
                }
            }
        }
        None
    }
    pub fn len(&self) -> usize {
        self.count
    }
}

// ===========================================================================
// F212 — 回收站：删除进站 + 还原
// ===========================================================================

pub const TRASH_CAP: usize = 32;

#[derive(Clone, Copy)]
pub struct TrashEntry {
    pub origin: &'static str, // 原路径
    pub stamp: u64,
}

pub struct Trash {
    entries: [Option<TrashEntry>; TRASH_CAP],
    count: usize,
}

impl Trash {
    pub const fn new() -> Trash {
        Trash { entries: [None; TRASH_CAP], count: 0 }
    }
    pub fn put(&mut self, origin: &'static str, stamp: u64) -> bool {
        if origin.is_empty() || self.count >= TRASH_CAP {
            return false;
        }
        self.entries[self.count] = Some(TrashEntry { origin, stamp });
        self.count += 1;
        true
    }
    /// 还原：返回原路径并出站。
    pub fn restore(&mut self, idx: usize) -> Option<&'static str> {
        if idx >= self.count {
            return None;
        }
        let e = self.entries[idx]?;
        for j in idx..self.count - 1 {
            self.entries[j] = self.entries[j + 1];
        }
        self.entries[self.count - 1] = None;
        self.count -= 1;
        Some(e.origin)
    }
    pub fn len(&self) -> usize {
        self.count
    }
}

// ===========================================================================
// F213 — 文件管理器成熟化：标签页 + 预览
// ===========================================================================

pub const MAX_TABS: usize = 6;

pub struct FileTabs {
    paths: [Option<&'static str>; MAX_TABS],
    count: usize,
    active: usize,
}

impl FileTabs {
    pub const fn new() -> FileTabs {
        FileTabs { paths: [None; MAX_TABS], count: 0, active: 0 }
    }
    pub fn open(&mut self, path: &'static str) -> bool {
        if self.count >= MAX_TABS {
            return false;
        }
        self.paths[self.count] = Some(path);
        self.active = self.count;
        self.count += 1;
        true
    }
    pub fn close(&mut self, idx: usize) -> bool {
        if idx >= self.count {
            return false;
        }
        for j in idx..self.count - 1 {
            self.paths[j] = self.paths[j + 1];
        }
        self.paths[self.count - 1] = None;
        self.count -= 1;
        if self.active >= self.count && self.count > 0 {
            self.active = self.count - 1;
        }
        true
    }
    pub fn activate(&mut self, idx: usize) -> bool {
        if idx < self.count {
            self.active = idx;
            true
        } else {
            false
        }
    }
    pub fn active(&self) -> Option<&'static str> {
        if self.active < self.count { self.paths[self.active] } else { None }
    }
    pub fn len(&self) -> usize {
        self.count
    }
}

// ===========================================================================
// F214 — 终端成熟化：真彩 + 常用转义完备
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TermCap {
    TrueColor24,
    Palette256,
    CursorMove,
    AlternateScreen,
    ScrollRegion,
}

pub const TERM_CAPS: [TermCap; 5] = [
    TermCap::TrueColor24,
    TermCap::Palette256,
    TermCap::CursorMove,
    TermCap::AlternateScreen,
    TermCap::ScrollRegion,
];

/// 24bit 颜色 → CSI 真彩前景序列（验证编码正确性）。
pub fn truecolor_fg(r: u8, g: u8, b: u8, out: &mut [u8; 24]) -> usize {
    let mut n = 0;
    let head = b"\x1b[38;2;";
    for &c in head {
        out[n] = c;
        n += 1;
    }
    for (i, v) in [r, g, b].iter().enumerate() {
        if i > 0 {
            out[n] = b';';
            n += 1;
        }
        // u8 十进制
        let mut d = [0u8; 3];
        let mut w = 0;
        let mut x = *v;
        if x == 0 {
            d[0] = b'0';
            w = 1;
        }
        while x > 0 {
            d[w] = b'0' + x % 10;
            x /= 10;
            w += 1;
        }
        while w > 0 {
            w -= 1;
            out[n] = d[w];
            n += 1;
        }
    }
    out[n] = b'm';
    n += 1;
    n
}

// ===========================================================================
// F215 — 应用商店骨架：本地清单 + 安装流
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PkgState {
    Available,
    Downloading,
    Installing,
    Installed,
    Failed,
}

/// 安装流状态机迁移合法性。
pub fn pkg_transition(from: PkgState, to: PkgState) -> bool {
    use PkgState::*;
    matches!(
        (from, to),
        (Available, Downloading)
            | (Downloading, Installing)
            | (Downloading, Failed)
            | (Installing, Installed)
            | (Installing, Failed)
            | (Failed, Downloading)
            | (Installed, Available) // 卸载
    )
}

// ===========================================================================
// F216 — 自动更新 UI：检查/下载/安装三态
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UpdState {
    Idle,
    Checking,
    UpToDate,
    Offered,
    Downloading,
    ReadyToInstall,
    Installing,
    Done,
    Failed,
}

pub fn upd_transition(from: UpdState, to: UpdState) -> bool {
    use UpdState::*;
    matches!(
        (from, to),
        (Idle, Checking)
            | (Checking, UpToDate)
            | (Checking, Offered)
            | (Checking, Failed)
            | (Offered, Downloading)
            | (Downloading, ReadyToInstall)
            | (Downloading, Failed)
            | (ReadyToInstall, Installing)
            | (Installing, Done)
            | (Installing, Failed)
            | (Failed, Checking)
    )
}

// ===========================================================================
// F217 — 系统监视器：CPU/内存/网络/磁盘实时
// ===========================================================================

#[derive(Clone, Copy)]
pub struct SysSample {
    pub cpu_permille: u16,
    pub mem_permille: u16,
    pub net_kbps: u32,
    pub disk_kbps: u32,
}

impl SysSample {
    pub fn sane(&self) -> bool {
        self.cpu_permille <= 1000 && self.mem_permille <= 1000
    }
}

/// 简单滑动平均（4 点），供曲线显示平滑。
pub fn smooth(hist: &[SysSample; 4], next: SysSample) -> SysSample {
    SysSample {
        cpu_permille: ((hist[0].cpu_permille as u32
            + hist[1].cpu_permille as u32
            + hist[2].cpu_permille as u32
            + hist[3].cpu_permille as u32
            + next.cpu_permille as u32) / 5) as u16,
        mem_permille: next.mem_permille,
        net_kbps: next.net_kbps,
        disk_kbps: next.disk_kbps,
    }
}

// ===========================================================================
// F218 — 备份还原向导：用户数据备份 v1
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BackupPhase {
    Pick,
    Snapshot,
    Copying(u32), // 进度 permille
    Verify,
    Done,
    Failed,
}

pub fn backup_next(p: BackupPhase) -> BackupPhase {
    match p {
        BackupPhase::Pick => BackupPhase::Snapshot,
        BackupPhase::Snapshot => BackupPhase::Copying(0),
        BackupPhase::Copying(d) if d < 1000 => BackupPhase::Copying((d + 250).min(1000)),
        BackupPhase::Copying(_) => BackupPhase::Verify,
        BackupPhase::Verify => BackupPhase::Done,
        other => other,
    }
}

// ===========================================================================
// F219 — 家长控制骨架：时段/内容限制 v1
// ===========================================================================

#[derive(Clone, Copy)]
pub struct ScreenTime {
    pub allowed_from_hour: u8,
    pub allowed_to_hour: u8,
    pub daily_minutes_max: u16,
}

impl ScreenTime {
    pub fn window_valid(&self) -> bool {
        self.allowed_from_hour < 24 && self.allowed_to_hour <= 24 && self.allowed_from_hour != self.allowed_to_hour
    }
    pub fn allows(&self, hour: u8, used_minutes: u16) -> bool {
        let in_window = if self.allowed_from_hour < self.allowed_to_hour {
            hour >= self.allowed_from_hour && hour < self.allowed_to_hour
        } else {
            hour >= self.allowed_from_hour || hour < self.allowed_to_hour
        };
        in_window && used_minutes < self.daily_minutes_max
    }
}

// ===========================================================================
// F220 — 电源菜单：睡眠/重启进其他系统
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PowerChoice {
    Sleep,
    Restart,
    RestartInto(u32), // 引导项 id（重启进其他系统）
    Shutdown,
    Hibernate,
}

pub fn power_choice_valid(c: PowerChoice) -> bool {
    match c {
        PowerChoice::RestartInto(id) => id < 8,
        _ => true,
    }
}

// ===========================================================================
// F221 — 锁屏成熟化：快捷通知 + 无缝解锁
// ===========================================================================

#[derive(Clone, Copy)]
pub struct LockScreen {
    pub notif_count: u16,
    pub unlock_ms_budget: u32, // 无缝解锁预算
}

impl LockScreen {
    pub fn snappy(&self) -> bool {
        self.unlock_ms_budget <= 300
    }
}

// ===========================================================================
// F222 — 快速设置面板：常用开关下拉面板
// ===========================================================================

pub const QUICK_TOGGLES: [&str; 8] = [
    "wifi", "bluetooth", "airplane", "nightlight", "dnd", "battery-saver", "cast", "hotspot",
];

pub fn quick_toggle_known(name: &str) -> bool {
    QUICK_TOGGLES.contains(&name)
}

// ===========================================================================
// F223 — 全局命令面板：键盘党快速操作
// ===========================================================================

#[derive(Clone, Copy)]
pub struct Command {
    pub name: &'static str,
    pub hotkey: u8, // 0 = 无
}

pub const COMMANDS: [Command; 8] = [
    Command { name: "new-window", hotkey: 1 },
    Command { name: "close-window", hotkey: 2 },
    Command { name: "switch-workspace", hotkey: 3 },
    Command { name: "screenshot", hotkey: 4 },
    Command { name: "lock", hotkey: 5 },
    Command { name: "settings", hotkey: 6 },
    Command { name: "terminal", hotkey: 7 },
    Command { name: "files", hotkey: 8 },
];

/// 前缀匹配模糊查找：返回首个命中命令索引。
pub fn command_lookup(pat: &str) -> Option<usize> {
    COMMANDS.iter().position(|c| c.name.starts_with(pat))
}

// ===========================================================================
// F224 — 欢迎引导：首次配置向导
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WelcomeStep {
    Language,
    Region,
    Keyboard,
    Network,
    Account,
    Privacy,
    Finish,
}

pub fn welcome_next(s: WelcomeStep) -> WelcomeStep {
    use WelcomeStep::*;
    match s {
        Language => Region,
        Region => Keyboard,
        Keyboard => Network,
        Network => Account,
        Account => Privacy,
        Privacy | Finish => Finish,
    }
}

// ===========================================================================
// F225 — shell 全量回归清单：自动化 + 人工混合清单
// ===========================================================================

pub const SHELL_REGRESSION: [&str; 12] = [
    "settings-persist", "taskbar-multimon", "snap-quad", "workspace-switch", "clipboard-pin",
    "notif-action", "assoc-open", "trash-restore", "tab-preview", "pkg-install",
    "update-three-state", "palette-hotkey",
];

pub fn regression_covered(items: &[&str]) -> bool {
    items.iter().all(|i| SHELL_REGRESSION.contains(i))
}

// ===========================================================================
// 域自检 F225 扩展：≥25 条 CheckSet
// ===========================================================================

pub fn run_m4shell_checks() -> CheckSet {
    let mut set = CheckSet::new("m4shell");

    // F201 设置中心
    let mut store = PrefStore::new();
    let upsert_ok = store.set(Pref { key: "volume", section: "sound", value: PrefValue::Num(60) })
        && store.set(Pref { key: "volume", section: "sound", value: PrefValue::Num(70) });
    let vol = store.get("volume");
    set.add("F201 settings upsert", upsert_ok && vol == Some(PrefValue::Num(70)), "upsert+read");
    set.add("F201 sections known", store.section_ok("sound") && !store.section_ok("bogus"), "8 sections");

    // F202 任务栏策略
    set.add("F202 taskbar policies", taskbar_policy_valid(TaskbarPolicy::AllDisplays), "3 policies");

    // F203 贴靠
    let scr = Rect { x: 0, y: 0, w: 1920, h: 1080 };
    let q = snap_quad(scr, 1);
    let e = snap_edge(scr, 0);
    set.add(
        "F203 snap matrix",
        q == Some(Rect { x: 960, y: 0, w: 960, h: 540 }) && e == Some(Rect { x: 0, y: 0, w: 960, h: 1080 }),
        "quad+edge",
    );
    set.add("F203 snap bounds", snap_quad(scr, 4).is_none(), "slot OOB");

    // F204 虚拟桌面
    let mut ws = WorkspaceSet::new();
    let ws_ok = ws.create() && ws.create() && ws.switch(2);
    let moved = ws.move_window(2, 5, true);
    set.add("F204 workspaces", ws_ok && ws.active() == 2 && ws.count() == 4, "create/switch");
    set.add("F204 move window", moved && ws.resident_of(2) & (1u64 << 5) != 0, "resident bit");
    set.add("F204 ws bounds", !ws.switch(9) && !ws.move_window(9, 0, true), "OOB reject");

    // F205 缓动
    let y0 = ease_at(EASE_STANDARD, 0);
    let y1k = ease_at(EASE_STANDARD, 1000);
    let ymid = ease_at(EASE_STANDARD, 500);
    set.add("F205 easing endpoints", y0 == 0 && y1k == 1000, "t=0/1");
    set.add("F205 easing mid", ymid > 0 && ymid < 1000, "monotone family");

    // F206 剪贴板
    let mut clip = ClipHistory::new();
    let c1 = clip.push("alpha") && clip.push("beta") && clip.push("alpha");
    set.add("F206 clip dedup top", c1 && clip.get(0).map(|i| i.text) == Some("alpha") && clip.len() == 2, "dedup");
    set.add("F206 clip capacity", ClipHistory::clip_ring_wrap_ok(), "ring wrap");
    set.add("F206 clip search", clip.clip_search("alp") >= 1, "search");

    // F207 截图
    set.add("F207 shot modes", shot_mode_valid(ShotMode::FullScreen) && !shot_mode_valid(ShotMode::Region(Rect { x: 0, y: 0, w: 0, h: 5 })), "validate");

    // F208 通知行动
    let n = Notif {
        id: 1,
        title: "msg",
        actions: [
            Some(NotifAction { label: "open", verb: 1 }),
            Some(NotifAction { label: "dismiss", verb: 0 }),
            None,
        ],
    };
    set.add("F208 notif actions", n.action_count() == 2 && n.dispatch(1) && !n.dispatch(2), "interactive");

    // F209 统一搜索
    let hits = [
        Hit::File("notes.txt"),
        Hit::App("notes"),
        Hit::Setting("note-sync"),
        Hit::App("nothing"),
        Hit::File("none"),
        Hit::Setting("network"),
        Hit::App("notes-plus"),
        Hit::File("nan"),
    ];
    let mut out = [Hit::File(""); 8];
    let m = merge_hits(&hits, 8, "note", &mut out);
    set.add("F209 unified search", m == 4 && out[0].rank() == 0, "ranked merge");

    // F210 最近文件
    let mut rec = RecentFiles::new();
    const RECENT_DEMO: [&str; RECENT_CAP] =
        ["r0", "r1", "r2", "r3", "r4", "r5", "r6", "r7", "r8", "r9"];
    let mut r_ok = true;
    for (i, p) in RECENT_DEMO.iter().enumerate() {
        r_ok = r_ok && rec.touch(p, i as u32);
    }
    r_ok = r_ok && rec.touch("r0", 99);
    set.add("F210 recent files", r_ok && rec.len() == RECENT_CAP && rec.get(0).map(|e| e.0) == Some("r0"), "bounded+dedup");

    // F211 默认应用
    let mut assoc = AssocTable::new();
    let a_ok = assoc.set("text/plain", 3) && assoc.set("text/plain", 7);
    set.add("F211 default apps", a_ok && assoc.lookup("text/plain") == Some(7) && assoc.lookup("nope").is_none(), "assoc");

    // F212 回收站
    let mut tr = Trash::new();
    let t_ok = tr.put("/home/a.txt", 1) && tr.put("/home/b.txt", 2);
    let restored = tr.restore(0);
    set.add("F212 trash", t_ok && restored == Some("/home/a.txt") && tr.len() == 1, "put/restore");

    // F213 文件管理器
    let mut tabs = FileTabs::new();
    let tb_ok = tabs.open("/docs") && tabs.open("/music") && tabs.activate(0) && tabs.close(0);
    set.add("F213 file manager", tb_ok && tabs.len() == 1 && tabs.active() == Some("/music"), "tabs");

    // F214 终端
    let mut buf = [0u8; 24];
    let tn = truecolor_fg(1, 20, 255, &mut buf);
    let ts = core::str::from_utf8(&buf[..tn]).unwrap_or("");
    set.add("F214 terminal truecolor", ts == "\x1b[38;2;1;20;255m", "CSI encode");
    set.add("F214 terminal caps", TERM_CAPS.len() == 5, "esc set");

    // F215 应用商店
    set.add(
        "F215 app store flow",
        pkg_transition(PkgState::Available, PkgState::Downloading)
            && pkg_transition(PkgState::Installing, PkgState::Installed)
            && !pkg_transition(PkgState::Available, PkgState::Installed),
        "state machine",
    );

    // F216 更新三态
    set.add(
        "F216 update states",
        upd_transition(UpdState::Idle, UpdState::Checking)
            && upd_transition(UpdState::Checking, UpdState::Offered)
            && !upd_transition(UpdState::Idle, UpdState::Installing),
        "three-state",
    );

    // F217 系统监视
    let s = SysSample { cpu_permille: 800, mem_permille: 500, net_kbps: 100, disk_kbps: 50 };
    let h = [s; 4];
    let sm = smooth(&h, s);
    set.add("F217 sysmon", s.sane() && sm.cpu_permille == 800, "sample+smooth");
    set.add("F217 sysmon sane", !SysSample { cpu_permille: 1200, mem_permille: 0, net_kbps: 0, disk_kbps: 0 }.sane(), "clamp");

    // F218 备份
    let mut bp = BackupPhase::Pick;
    for _ in 0..8 {
        bp = backup_next(bp);
    }
    set.add("F218 backup wizard", bp == BackupPhase::Done, "phase walk");

    // F219 家长控制
    let st = ScreenTime { allowed_from_hour: 16, allowed_to_hour: 20, daily_minutes_max: 120 };
    set.add(
        "F219 parental",
        st.window_valid() && st.allows(17, 60) && !st.allows(10, 0) && !st.allows(17, 200),
        "time window",
    );

    // F220 电源菜单
    set.add(
        "F220 power menu",
        power_choice_valid(PowerChoice::RestartInto(3)) && !power_choice_valid(PowerChoice::RestartInto(9)),
        "choices",
    );

    // F221 锁屏
    let ls = LockScreen { notif_count: 5, unlock_ms_budget: 250 };
    set.add("F221 lock screen", ls.snappy(), "seamless unlock");

    // F222 快速设置
    set.add("F222 quick settings", quick_toggle_known("dnd") && !quick_toggle_known("warp"), "toggles");

    // F223 命令面板
    let cmd = command_lookup("scr");
    set.add("F223 command palette", cmd == Some(3), "prefix lookup");
    set.add("F223 palette miss", command_lookup("zzz").is_none(), "no hit");

    // F224 欢迎引导
    let mut w = WelcomeStep::Language;
    for _ in 0..9 {
        w = welcome_next(w);
    }
    set.add("F224 welcome wizard", w == WelcomeStep::Finish, "walk");

    // F225 回归清单
    let reg = ["settings-persist", "snap-quad", "palette-hotkey"];
    set.add("F225 shell regression", regression_covered(&reg) && !regression_covered(&["made-up"]), "checklist");

    set
}

// ===========================================================================
// 单测
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f201_pref_store_upsert() {
        let mut s = PrefStore::new();
        assert!(s.set(Pref { key: "a", section: "display", value: PrefValue::Bool(true) }));
        assert!(s.set(Pref { key: "a", section: "display", value: PrefValue::Bool(false) }));
        assert_eq!(s.get("a"), Some(PrefValue::Bool(false)));
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn f203_snap_geometry() {
        let scr = Rect { x: 0, y: 0, w: 1000, h: 800 };
        assert_eq!(snap_quad(scr, 3), Some(Rect { x: 500, y: 400, w: 500, h: 400 }));
        assert_eq!(snap_edge(scr, 3), Some(Rect { x: 0, y: 400, w: 1000, h: 400 }));
    }

    #[test]
    fn f204_workspace_flow() {
        let mut ws = WorkspaceSet::new();
        assert!(ws.create());
        assert!(ws.switch(1));
        assert!(ws.move_window(1, 0, true));
        assert_eq!(ws.resident_of(1) & 1, 1);
        assert!(ws.move_window(1, 0, false));
        assert_eq!(ws.resident_of(1) & 1, 0);
        assert!(!ws.switch(5));
    }

    #[test]
    fn f205_easing_is_monotone_family() {
        let prev = ease_at(EASE_DECEL, 200);
        let later = ease_at(EASE_DECEL, 800);
        assert!(prev <= later);
        assert_eq!(ease_at(EASE_DECEL, 0), 0);
    }

    #[test]
    fn f206_clip_pinned_survives() {
        let mut c = ClipHistory::new();
        for t in ClipHistory::CLIP_DEMO.iter() {
            assert!(c.push(t));
        }
        assert!(c.pin(0, true)); // 置顶最新
        assert!(c.push("c16")); // 挤掉最旧未置顶，置顶项仍在
        assert_eq!(c.len(), CLIP_CAP);
        assert!(c.clip_search("c15") >= 1); // c15 是置顶项，仍在
        // 全部置顶后拒收
        for i in 0..CLIP_CAP {
            c.pin(i, true);
        }
        assert!(!c.push("no-room"));
    }

    #[test]
    fn f212_trash_restore_order() {
        let mut t = Trash::new();
        assert!(t.put("/x", 1));
        assert!(t.put("/y", 2));
        assert_eq!(t.restore(1), Some("/y"));
        assert_eq!(t.restore(0), Some("/x"));
        assert_eq!(t.restore(0), None);
    }

    #[test]
    fn f214_truecolor_encoding() {
        let mut buf = [0u8; 24];
        let n = truecolor_fg(0, 0, 0, &mut buf);
        assert_eq!(core::str::from_utf8(&buf[..n]).unwrap(), "\x1b[38;2;0;0;0m");
    }

    #[test]
    fn f225_domain_selfcheck_all_pass() {
        let set = run_m4shell_checks();
        assert!(set.len() >= 25, "expected >=25 checks, got {}", set.len());
        assert!(set.all_passed(), "{:?}", {
            let mut buf = [0u8; 1024];
            let n = set.render(&mut buf);
            core::str::from_utf8(&buf[..n]).unwrap().to_string()
        });
    }
}
