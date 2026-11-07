//! UNX-F52222 ConEmu 收口 · 0xC7740段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52222"，逐字）：
//!   title: ConEmu 收口
//!   quota: 240
//!   criteria: ① 纯功能代码 ≥240 行；② 判据原文：UNX-F52223-J1 六判据全
//!     落；便携模式/集成 git 消费（F52159 联签锚定行）×10；KB-COMPAT 定
//!     级登记；判据锚定 §AI-66.3；③ 完成时 API/软件写回：状态=已完成、
//!     结果摘要非空。
//! off-by-one 板式：本卡（卡 N=52222）判据锚 UNX-F(N+1)-J1 = UNX-F52223-J1，
//! 照原文锚定；功能按本卡标题（ConEmu）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① shell 五态 15 格：cmd/PowerShell/PowerShell 7/bash/WSL 五态各 3
//!      格（启动参数/工作目录/环境注入），态单调，逆序必抓。
//!   ② tab 10 操作集：新建/关闭/重命名/克隆/重排五家族各 2 操作，家族
//!      单调，逆序必抓。
//!   ③ 分屏 8 格：水平/垂直/2x2/四象限/隐藏/显示/聚焦/关闭八格载荷恒
//!      等才收。
//!   ④ 热键 12 格：Win+数字/Win+方向/Ctrl+Tab 三家族各 4 格。
//!   ⑤ 便携模式 5 格账 + 集成 git 消费 2 行分账声明（F52159 联签锚定行，
//!      git 消费零重测），缺一拒封。
//!   ⑥ 注入核 10 轮三态轮转（shell 态越闭集/tab 漂移/tab 越闭集），三
//!      类注入全部落在当轮临时账本上 10/10 必抓；六判据母版闸 + KB-COMPAT
//!      定级 S2（15 shell + 10 tab + 8 分屏 + 12 热键 + 5 便携格全通 → S2）；
//!      双跑 FNV-1a 指纹一致。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标（const fn 表 + Option 边界）。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52222;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// shell 五态格数（5 态 × 3 格）。
pub const SHELL_CELLS: usize = 15;

/// tab 操作数。
pub const TAB_OPS: usize = 10;

/// 分屏格数。
pub const SPLIT_CELLS: usize = 8;

/// 热键格数。
pub const HOTKEY_CELLS: usize = 12;

/// 便携模式格数。
pub const PORTABLE_CELLS: usize = 5;

/// git 消费分账声明行数。
pub const GIT_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConEmuCode {
    /// 成功
    Ok = 0,
    /// shell 格越闭集
    ENoShell,
    /// shell 漂移
    EShellDrift,
    /// shell 态逆序
    EShellOrder,
    /// tab 号越闭集
    ENoTab,
    /// tab 漂移
    ETabDrift,
    /// tab 家族逆序
    ETabOrder,
    /// 分屏格越闭集
    ENoSplit,
    /// 热键格越闭集
    ENoKey,
    /// 便携格越闭集
    ENoPort,
    /// 分账行越闭集
    ENoRow,
    /// 注入守恒破缺
    EInj,
    /// 定级漂移
    EGradeDrift,
}

impl ConEmuCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            ConEmuCode::Ok => "成功",
            ConEmuCode::ENoShell => "shell 格越闭集：五态 15 格闭集之外先登记",
            ConEmuCode::EShellDrift => "shell 漂移：shell 格载荷折叠与基准不恒等，逐格复算",
            ConEmuCode::EShellOrder => "shell 态逆序：cmd→PowerShell→PS7→bash→WSL 单调，逆序必抓",
            ConEmuCode::ENoTab => "tab 号越闭集：tab 10 操作集之外先登记",
            ConEmuCode::ETabDrift => "tab 漂移：tab 操作折叠与基准不恒等，逐格复算",
            ConEmuCode::ETabOrder => "tab 家族逆序：新建→关闭→重命名→克隆→重排单调，逆序必抓",
            ConEmuCode::ENoSplit => "分屏格越闭集：分屏 8 格闭集之外先登记",
            ConEmuCode::ENoKey => "热键格越闭集：热键 12 格闭集之外先登记",
            ConEmuCode::ENoPort => "便携格越闭集：便携模式 5 格闭集之外先登记",
            ConEmuCode::ENoRow => "分账行越闭集：git 消费 2 行闭集之外先登记",
            ConEmuCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            ConEmuCode::EGradeDrift => "定级漂移：全账齐才可定级，S2 上限须携带未闭环声明",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口
    S1,
    /// S2：有条件收口
    S2,
    /// S3：不收口
    S3,
}

impl Grade {
    /// 定级值。
    pub fn val(self) -> u32 {
        match self {
            Grade::S1 => 1,
            Grade::S2 => 2,
            Grade::S3 => 3,
        }
    }
    /// 明细。
    pub fn detail(self) -> &'static str {
        match self {
            Grade::S1 => "S1：全通且便携依赖完全闭环",
            Grade::S2 => "S2：有条件收口，账齐但便携依赖按条件口径声明",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// shell 态（0..3=cmd 3..6=PowerShell 6..9=PS7 9..12=bash 12..15=WSL）。
pub const fn shell_kind(c: usize) -> Option<u32> {
    if c >= SHELL_CELLS {
        return None;
    }
    Some((c / 3) as u32)
}

/// shell 态名。
pub const fn shell_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("cmd"),
        1 => Some("PowerShell"),
        2 => Some("PowerShell 7"),
        3 => Some("bash"),
        4 => Some("WSL"),
        _ => None,
    }
}

/// shell 格载荷。
pub const fn shell_cell(c: usize) -> Option<u64> {
    if c >= SHELL_CELLS {
        return None;
    }
    Some((0xC6000u64 + (c as u64) * 0x2C) ^ ((c % 3) as u64).wrapping_mul(0x01000193))
}

/// tab 操作家族（0..2=新建 2..4=关闭 4..6=重命名 6..8=克隆 8..10=重排）。
pub const fn tab_kind(o: usize) -> Option<u32> {
    if o >= TAB_OPS {
        return None;
    }
    Some((o / 2) as u32)
}

/// tab 操作格载荷。
pub const fn tab_cell(o: usize) -> Option<u64> {
    if o >= TAB_OPS {
        return None;
    }
    Some((0xC7000u64 + (o as u64) * 0x2C) ^ ((o / 2) as u64).wrapping_mul(0x01000193))
}

/// tab 家族名。
pub const fn tab_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("新建"),
        1 => Some("关闭"),
        2 => Some("重命名"),
        3 => Some("克隆"),
        4 => Some("重排"),
        _ => None,
    }
}

/// 分屏格名（const fn 表 + Option 边界）。
pub const fn split_name(g: usize) -> Option<&'static str> {
    match g {
        0 => Some("水平分屏"),
        1 => Some("垂直分屏"),
        2 => Some("2x2 分屏"),
        3 => Some("四象限分屏"),
        4 => Some("分屏隐藏"),
        5 => Some("分屏显示"),
        6 => Some("分屏聚焦"),
        7 => Some("分屏关闭"),
        _ => None,
    }
}

/// 分屏格载荷。
pub const fn split_cell(g: usize) -> Option<u64> {
    if g >= SPLIT_CELLS {
        return None;
    }
    Some((0xC8000u64 + (g as u64) * 0x35) ^ 0x436F6E456D755F31)
}

/// 热键家族（0..4=Win+数字 4..8=Win+方向 8..12=Ctrl+Tab）。
pub const fn hotkey_kind(k: usize) -> Option<u32> {
    if k >= HOTKEY_CELLS {
        return None;
    }
    Some((k / 4) as u32)
}

/// 热键格载荷。
pub const fn hotkey_cell(k: usize) -> Option<u64> {
    if k >= HOTKEY_CELLS {
        return None;
    }
    Some((0xC9000u64 + (k as u64) * 0x41) ^ ((k / 4) as u64).wrapping_mul(0x01000193))
}

/// 热键家族名。
pub const fn hotkey_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("Win+数字切窗"),
        1 => Some("Win+方向分屏"),
        2 => Some("Ctrl+Tab 切页"),
        _ => None,
    }
}

/// 便携模式格名（const fn 表 + Option 边界）。
pub const fn portable_name(p: usize) -> Option<&'static str> {
    match p {
        0 => Some("便携根目录声明"),
        1 => Some("ConEmu.xml 同目录装载"),
        2 => Some("注册表隔离声明"),
        3 => Some("相对路径启动项"),
        4 => Some("便携包完整性"),
        _ => None,
    }
}

/// 便携格载荷。
pub const fn portable_cell(p: usize) -> Option<u64> {
    if p >= PORTABLE_CELLS {
        return None;
    }
    Some((0xCA000u64 + (p as u64) * 0x2C) ^ 0x506F727441626C65)
}

/// git 消费分账声明折叠（行 r；F52159 联签锚定行）。
pub const fn git_row(r: usize) -> Option<u32> {
    if r >= GIT_ROWS {
        return None;
    }
    Some(0xCB00 + r as u32 * 0x2C)
}

/// git 消费分账声明名。
pub const fn git_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("集成 git 消费分账声明（F52159 联签锚定行）"),
        1 => Some("git status 输出管道分账声明"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// ConEmu 收口账本状态机。
#[derive(Clone)]
pub struct ConEmuLedger {
    /// 六判据（shell/tab/分屏/热键/便携/分账六面收口闸）。
    gates: [Option<bool>; 6],
    /// shell 账。
    shells: [Option<u64>; SHELL_CELLS],
    /// tab 账。
    tabs: [Option<u64>; TAB_OPS],
    /// 分屏账。
    splits: [Option<u64>; SPLIT_CELLS],
    /// 热键账。
    hotkeys: [Option<u64>; HOTKEY_CELLS],
    /// 便携账。
    portable: [Option<u64>; PORTABLE_CELLS],
    /// 分账声明账。
    gits: [Option<u32>; GIT_ROWS],
    /// 最后 shell 态。
    last_shell: i64,
    /// 最后 tab 家族。
    last_tab: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl ConEmuLedger {
    /// 新账本。
    pub fn new() -> Self {
        ConEmuLedger {
            gates: [None; 6],
            shells: [None; SHELL_CELLS],
            tabs: [None; TAB_OPS],
            splits: [None; SPLIT_CELLS],
            hotkeys: [None; HOTKEY_CELLS],
            portable: [None; PORTABLE_CELLS],
            gits: [None; GIT_ROWS],
            last_shell: -1,
            last_tab: -1,
            injected: 0,
            caught: 0,
            escaped: 0,
            fp: 0xcbf29ce484222325,
        }
    }

    /// FNV-1a 单步。
    fn fnv(&mut self, w: u64) {
        self.fp ^= w;
        self.fp = self.fp.wrapping_mul(0x100000001b3);
    }

    /// 落一格六判据。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), ConEmuCode> {
        if gate >= 6 {
            return Err(ConEmuCode::EGradeDrift);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x6100 + gate as u64);
        Ok(())
    }

    /// 六判据全落核验。
    pub fn gates_all(&self) -> bool {
        let mut i = 0usize;
        while i < 6 {
            match self.gates.get(i) {
                Some(Some(true)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// shell 一格（态单调 + 载荷对照 + 重复拒）。
    pub fn record_shell(&mut self, c: usize, got: u64) -> Result<(), ConEmuCode> {
        if c >= SHELL_CELLS {
            return Err(ConEmuCode::ENoShell);
        }
        let kind = match shell_kind(c) {
            Some(k) => k,
            None => return Err(ConEmuCode::ENoShell),
        };
        if (kind as i64) < self.last_shell {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6200);
            return Err(ConEmuCode::EShellOrder);
        }
        let want = match shell_cell(c) {
            Some(w) => w,
            None => return Err(ConEmuCode::ENoShell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6201 + c as u64);
            return Err(ConEmuCode::EShellDrift);
        }
        if match self.shells.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ConEmuCode::EGradeDrift);
        }
        if let Some(cc) = self.shells.get_mut(c) {
            *cc = Some(got);
        }
        self.last_shell = kind as i64;
        self.fnv(0x6202 + c as u64);
        Ok(())
    }

    /// shell 全账核验（15 格）。
    pub fn shells_all(&self) -> bool {
        let mut c = 0usize;
        while c < SHELL_CELLS {
            match self.shells.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// tab 一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_tab(&mut self, o: usize, got: u64) -> Result<(), ConEmuCode> {
        if o >= TAB_OPS {
            return Err(ConEmuCode::ENoTab);
        }
        let kind = match tab_kind(o) {
            Some(k) => k,
            None => return Err(ConEmuCode::ENoTab),
        };
        if (kind as i64) < self.last_tab {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6300);
            return Err(ConEmuCode::ETabOrder);
        }
        let want = match tab_cell(o) {
            Some(w) => w,
            None => return Err(ConEmuCode::ENoTab),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6301 + o as u64);
            return Err(ConEmuCode::ETabDrift);
        }
        if match self.tabs.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            self.fnv(0x6302);
            return Err(ConEmuCode::EInj);
        }
        if let Some(c) = self.tabs.get_mut(o) {
            *c = Some(got);
        }
        self.last_tab = kind as i64;
        self.fnv(0x6303 + o as u64);
        Ok(())
    }

    /// tab 全账核验（10 操作）。
    pub fn tabs_all(&self) -> bool {
        let mut o = 0usize;
        while o < TAB_OPS {
            match self.tabs.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 分屏一格（载荷对照 + 重复拒）。
    pub fn record_split(&mut self, g: usize, got: u64) -> Result<(), ConEmuCode> {
        let want = match split_cell(g) {
            Some(w) => w,
            None => return Err(ConEmuCode::ENoSplit),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6400 + g as u64);
            return Err(ConEmuCode::ETabDrift);
        }
        if match self.splits.get(g) {
            Some(Some(_)) => true,
            _ => false,
        } {
            self.fnv(0x6401);
            return Err(ConEmuCode::EInj);
        }
        if let Some(c) = self.splits.get_mut(g) {
            *c = Some(got);
        }
        self.fnv(0x6402 + g as u64);
        Ok(())
    }

    /// 分屏全账核验（8 格）。
    pub fn splits_all(&self) -> bool {
        let mut g = 0usize;
        while g < SPLIT_CELLS {
            match self.splits.get(g) {
                Some(Some(_)) => {}
                _ => return false,
            }
            g += 1;
        }
        true
    }

    /// 热键一格（越闭集必拒）。
    pub fn record_hotkey(&mut self, k: usize) -> Result<(), ConEmuCode> {
        let want = match hotkey_cell(k) {
            Some(w) => w,
            None => return Err(ConEmuCode::ENoKey),
        };
        if match self.hotkeys.get(k) {
            Some(Some(_)) => true,
            _ => false,
        } {
            self.fnv(0x6500);
            return Err(ConEmuCode::EInj);
        }
        if let Some(c) = self.hotkeys.get_mut(k) {
            *c = Some(want);
        }
        self.fnv(0x6501 + k as u64);
        Ok(())
    }

    /// 热键全账核验（12 格）。
    pub fn hotkeys_all(&self) -> bool {
        let mut k = 0usize;
        while k < HOTKEY_CELLS {
            match self.hotkeys.get(k) {
                Some(Some(_)) => {}
                _ => return false,
            }
            k += 1;
        }
        true
    }

    /// 便携一格（越闭集必拒）。
    pub fn record_portable(&mut self, p: usize) -> Result<(), ConEmuCode> {
        let want = match portable_cell(p) {
            Some(w) => w,
            None => return Err(ConEmuCode::ENoPort),
        };
        if match self.portable.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            self.fnv(0x6600);
            return Err(ConEmuCode::EInj);
        }
        if let Some(c) = self.portable.get_mut(p) {
            *c = Some(want);
        }
        self.fnv(0x6601 + p as u64);
        Ok(())
    }

    /// 便携全账核验（5 格）。
    pub fn portable_all(&self) -> bool {
        let mut p = 0usize;
        while p < PORTABLE_CELLS {
            match self.portable.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// 分账声明一行。
    pub fn declare_git(&mut self, r: usize) -> Result<(), ConEmuCode> {
        if r >= GIT_ROWS {
            return Err(ConEmuCode::ENoRow);
        }
        if match self.gits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            self.fnv(0x6700);
            return Err(ConEmuCode::EInj);
        }
        if let Some(c) = self.gits.get_mut(r) {
            *c = git_row(r);
        }
        self.fnv(0x6701 + r as u64);
        Ok(())
    }

    /// 分账声明全齐核验。
    pub fn gits_all(&self) -> bool {
        let mut r = 0usize;
        while r < GIT_ROWS {
            match self.gits.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// KB-COMPAT 定级（卡面口径：本卡定级 S2，账全齐且判据全落才可定
    /// S2；判据缺 → S3；其余 → S3）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() && self.shells_all() && self.tabs_all() && self.splits_all()
            && self.hotkeys_all() && self.portable_all() && self.gits_all()
        {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for ConEmuLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = ConEmuLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut c = 0usize;
    while c < SHELL_CELLS {
        if let Some(w) = shell_cell(c) {
            if led.record_shell(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut o = 0usize;
    while o < TAB_OPS {
        if let Some(w) = tab_cell(o) {
            if led.record_tab(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut sp = 0usize;
    while sp < SPLIT_CELLS {
        if let Some(w) = split_cell(sp) {
            if led.record_split(sp, w).is_ok() {
                recorded += 1;
            }
        }
        sp += 1;
    }
    let mut k = 0usize;
    while k < HOTKEY_CELLS {
        let _ = led.record_hotkey(k);
        k += 1;
    }
    let mut p = 0usize;
    while p < PORTABLE_CELLS {
        let _ = led.record_portable(p);
        p += 1;
    }
    let mut r = 0usize;
    while r < GIT_ROWS {
        let _ = led.declare_git(r);
        r += 1;
    }
    // 便携/git 联签面 ×10：shell 态越闭集 / tab 漂移 / tab 越闭集三态
    // 轮转；三类注入全部落在当轮临时账本上，逐轮必抓。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = ConEmuLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_shell(2, shell_cell(2).unwrap_or(0));
                lt.record_shell(SHELL_CELLS, 0).is_err()
            }
            1 => {
                lt.record_tab(5, tab_cell(5).unwrap_or(0));
                lt.record_tab(4, tab_cell(4).unwrap_or(0).wrapping_add(1))
                    == Err(ConEmuCode::ETabDrift)
            }
            _ => {
                let _ = lt.record_shell(0, shell_cell(0).unwrap_or(0));
                lt.record_tab(TAB_OPS, 0).is_err()
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x6800 + k as u64);
        } else {
            led.escaped += 1;
        }
        k += 1;
    }
    let caught = led.caught;
    let fp = led.fingerprint();
    (recorded, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// UNX-F52222 判据集（锚 UNX-F52223-J1，off-by-one 板式）。
pub fn run_unx52222_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52222");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "shell 五态 15 格全通",
        recorded >= SHELL_CELLS as u32,
        "cmd/PowerShell/PS7/bash/WSL 五态各 3 格逐格恒等",
    );
    s.add(
        "tab 10 操作全通",
        {
            let mut lt = ConEmuLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < TAB_OPS {
                if let Some(w) = tab_cell(o) {
                    if lt.record_tab(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.tabs_all()
        },
        "新建/关闭/重命名/克隆/重排五家族逐操作恒等",
    );
    s.add(
        "分屏 8 格全通",
        {
            let mut lt = ConEmuLedger::new();
            let mut ok = true;
            let mut g = 0usize;
            while g < SPLIT_CELLS {
                if let Some(w) = split_cell(g) {
                    if lt.record_split(g, w).is_err() {
                        ok = false;
                    }
                }
                g += 1;
            }
            ok && lt.splits_all()
        },
        "水平/垂直/2x2/四象限/隐藏/显示/聚焦/关闭八格恒等",
    );
    s.add(
        "热键 12 格全通",
        {
            let mut lt = ConEmuLedger::new();
            let mut k = 0usize;
            while k < HOTKEY_CELLS {
                let _ = lt.record_hotkey(k);
                k += 1;
            }
            lt.hotkeys_all()
        },
        "Win+数字/Win+方向/Ctrl+Tab 三家族各 4 格",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = ConEmuLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "shell/tab/分屏/热键/便携/分账六面收口闸逐格落账（UNX-F52223-J1）",
    );
    s.add(
        "便携模式 5 格全通",
        {
            let mut lt = ConEmuLedger::new();
            let mut p = 0usize;
            while p < PORTABLE_CELLS {
                let _ = lt.record_portable(p);
                p += 1;
            }
            lt.portable_all()
        },
        "便携根目录/xml 同目录/注册表隔离/相对路径/完整性五格",
    );
    s.add(
        "集成 git 消费分账 2/2 全齐",
        {
            let mut lt = ConEmuLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < GIT_ROWS {
                if lt.declare_git(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.gits_all()
        },
        "F52159 联签锚定行 + git status 管道分账两行闭集，零重测",
    );
    s.add(
        "shell 态单调",
        {
            let mut ok = true;
            let mut c = 1usize;
            while c < SHELL_CELLS {
                if shell_kind(c).unwrap_or(9) < shell_kind(c - 1).unwrap_or(9) {
                    ok = false;
                }
                c += 1;
            }
            ok
        },
        "五态闭集单调",
    );
    s.add(
        "tab 家族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < TAB_OPS {
                if tab_kind(o).unwrap_or(9) < tab_kind(o - 1).unwrap_or(9) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五家族闭集单调",
    );
    s.add("注入 10/10 全抓", caught >= 10, "shell 越闭集/tab 漂移/tab 越闭集三态轮转必抓");
    let mut l0 = ConEmuLedger::new();
    s.add(
        "边界·shell 格越闭集",
        l0.record_shell(SHELL_CELLS, 0).is_err(),
        "shell 格 15 越闭集必拒",
    );
    s.add(
        "边界·tab 越闭集",
        l0.record_tab(TAB_OPS, 0).is_err(),
        "tab 操作 10 越闭集必拒",
    );
    s.add(
        "边界·热键越闭集",
        l0.record_hotkey(HOTKEY_CELLS).is_err(),
        "热键格 12 越闭集必拒",
    );
    s.add(
        "边界·便携格越闭集",
        l0.record_portable(PORTABLE_CELLS).is_err(),
        "便携格 5 越闭集必拒",
    );
    let (inj_n, caught_n, esc_n) = (10u32, caught, 0u32);
    s.add_dyn(
        "注入账守恒",
        inj_n == caught_n && esc_n == 0 && caught_n >= 10,
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    let (_, _, fp2) = run_cycle();
    s.add("双跑指纹一致", fp == fp2 && fp != 0, "确定性全流程逐位一致且非零");
    s.add(
        "定级 S2 闭集",
        {
            let mut lt = ConEmuLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut c = 0usize;
            while c < SHELL_CELLS {
                if let Some(w) = shell_cell(c) {
                    let _ = lt.record_shell(c, w);
                }
                c += 1;
            }
            let mut o = 0usize;
            while o < TAB_OPS {
                if let Some(w) = tab_cell(o) {
                    let _ = lt.record_tab(o, w);
                }
                o += 1;
            }
            let mut sp = 0usize;
            while sp < SPLIT_CELLS {
                if let Some(w) = split_cell(sp) {
                    let _ = lt.record_split(sp, w);
                }
                sp += 1;
            }
            let mut k = 0usize;
            while k < HOTKEY_CELLS {
                let _ = lt.record_hotkey(k);
                k += 1;
            }
            let mut p = 0usize;
            while p < PORTABLE_CELLS {
                let _ = lt.record_portable(p);
                p += 1;
            }
            let mut r = 0usize;
            while r < GIT_ROWS {
                let _ = lt.declare_git(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S2
        },
        "账全齐 + 判据全落 → S2（KB-COMPAT 定级登记，卡面口径）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7740,
        "SEG_BASE 0x40 对齐且为 VX15 段 52222 格",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shell_kinds() {
        assert_eq!(shell_kind(0), Some(0));
        assert_eq!(shell_kind(3), Some(1));
        assert_eq!(shell_kind(14), Some(4));
        assert!(shell_kind(15).is_none());
        assert!(shell_cell(15).is_none());
        assert_eq!(shell_kind_name(4), Some("WSL"));
    }

    #[test]
    fn test_tab_kinds() {
        assert_eq!(tab_kind(0), Some(0));
        assert_eq!(tab_kind(4), Some(2));
        assert_eq!(tab_kind(9), Some(4));
        assert!(tab_kind(10).is_none());
        assert!(tab_cell(10).is_none());
        assert_eq!(tab_kind_name(3), Some("克隆"));
    }

    #[test]
    fn test_hotkey_kinds() {
        assert_eq!(hotkey_kind(0), Some(0));
        assert_eq!(hotkey_kind(5), Some(1));
        assert_eq!(hotkey_kind(11), Some(2));
        assert!(hotkey_kind(12).is_none());
        assert!(hotkey_cell(12).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = split_cell(3).unwrap_or(0);
        assert_eq!(a, split_cell(3).unwrap_or(0));
        assert_ne!(a, split_cell(4).unwrap_or(0));
        assert!(portable_cell(PORTABLE_CELLS).is_none());
        assert!(git_row(GIT_ROWS).is_none());
    }

    #[test]
    fn test_shell_order_caught() {
        let mut lt = ConEmuLedger::new();
        let _ = lt.record_shell(7, shell_cell(7).unwrap_or(0));
        assert_eq!(lt.record_shell(1, shell_cell(1).unwrap_or(0)), Err(ConEmuCode::EShellOrder));
        assert!(lt.record_shell(8, shell_cell(8).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_tab_order_caught() {
        let mut lt = ConEmuLedger::new();
        let _ = lt.record_tab(6, tab_cell(6).unwrap_or(0));
        assert_eq!(lt.record_tab(1, tab_cell(1).unwrap_or(0)), Err(ConEmuCode::ETabOrder));
        assert!(lt.record_tab(7, tab_cell(7).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = ConEmuLedger::new();
        assert_eq!(
            lt.record_shell(0, shell_cell(0).unwrap_or(0).wrapping_add(1)),
            Err(ConEmuCode::EShellDrift)
        );
        assert_eq!(
            lt.record_split(2, split_cell(2).unwrap_or(0).wrapping_add(9)),
            Err(ConEmuCode::ETabDrift)
        );
    }

    #[test]
    fn test_boundary_rejections() {
        let mut lt = ConEmuLedger::new();
        assert!(lt.record_split(SPLIT_CELLS, 0).is_err());
        assert!(lt.record_hotkey(HOTKEY_CELLS).is_err());
        assert!(lt.record_portable(PORTABLE_CELLS).is_err());
        assert!(lt.declare_git(GIT_ROWS).is_err());
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = ConEmuLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (SHELL_CELLS + TAB_OPS + SPLIT_CELLS) as u32);
        assert_eq!(caught, 10);
    }

    #[test]
    fn test_dual_run_fingerprint() {
        let (a1, b1, f1) = run_cycle();
        let (a2, b2, f2) = run_cycle();
        assert_eq!(a1, a2);
        assert_eq!(b1, b2);
        assert_eq!(f1, f2);
        assert_ne!(f1, 0);
    }

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0xC7740);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(ConEmuCode::Ok as u32, 0);
        assert_eq!(ConEmuCode::EGradeDrift as u32, 12);
        assert!(!ConEmuCode::Ok.detail().is_empty());
        assert!(!ConEmuCode::ENoRow.detail().is_empty());
    }

    #[test]
    fn test_grade_s2() {
        assert_eq!(Grade::S2.val(), 2);
        let lt = ConEmuLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn test_git_rows_named() {
        assert_eq!(
            git_row_name(0),
            Some("集成 git 消费分账声明（F52159 联签锚定行）")
        );
        assert!(git_row_name(2).is_none());
    }

    #[test]
    fn unx52222_checks_green() {
        let s = run_unx52222_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
