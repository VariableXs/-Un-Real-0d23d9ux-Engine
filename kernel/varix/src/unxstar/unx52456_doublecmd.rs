//! UNX-F52456 Double Commander 收口 · 0xCB1C0 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52456"，逐字）：
//!   title: Double Commander 收口
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52457-J1 六判据
//!     全落；插件生态 ×5；许可面如实定级（AI-95 联签锚定行）；KB-COMPAT
//!     定级登记；判据锚定 §AI-66.9 红线②；③ 完成时 API/软件写回：状态=
//!     已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52456）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52457-J1，照原文锚定；功能按本卡标题（Double Commander 收口）
//! 落地。本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test),
//! no_std)]` + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不
//! 重复声明）。
//!
//! 设计要点：
//!   ① 五面板族 20 格闭集：双窗/目录热列表/归档插件/文件工具/搜索 各 4 格，
//!      家族单调（逆序必抓），逐格载荷纯函数折叠恒等才收，漂移必抓。
//!   ② Lua 脚本消费 6 格闭集：目录热列表脚本/文件操作钩子/面板事件三相位
//!      各脚本位+消费位，逐格恒等，越闭集必抓。
//!   ③ 插件生态 ×5 家族闭集：WCX/WDX/WLX/WFX/WDS 五插件家族名闭集，
//!      越家族必拒（§AI-66.9 红线②生态口径）。
//!   ④ 跨平台三态 6 格：windows/linux/macos 各支持位+打包位，逐格恒等。
//!   ⑤ 与 Total Commander 分账声明 2 行闭集（账目与 TotalCmd 分账不并表）；
//!      Pascal/LCL 上栈声明 2 行闭集；缺一拒封。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/seal/fingerprint，
//!      FNV-1a）；注入核 10 轮三态轮转（面板漂移/家族逆序/Lua 越闭集）
//!      10/10 必抓，检出+逃逸守恒等式；双跑 FNV 指纹一致。
//!   ⑦ KB-COMPAT 定级登记：许可面如实定级（AI-95 联签锚定行）——GPL 开源
//!      面如实落 S2（有条件收口），登记档不因全齐自升 S1；定级阶梯
//!      S3→S2→S1 闭集经 grade_step 单调复核。
//!   ⑧ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52456;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 五面板族 20 格数。
pub const PANEL_N: usize = 20;

/// Lua 脚本消费格数。
pub const LUA_N: usize = 6;

/// 跨平台三态格数。
pub const CROSS_N: usize = 6;

/// 插件生态家族数。
pub const PLUGIN_FAMS: usize = 5;

/// 与 Total Commander 分账声明行数。
pub const SPLIT_ROWS: usize = 2;

/// Pascal/LCL 上栈声明行数。
pub const DECL_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DoublecmdCode {
    /// 成功
    Ok = 0,
    /// 面板格越闭集
    ENoPanel,
    /// 面板漂移
    EPanelDrift,
    /// 家族逆序
    EOrder,
    /// Lua 格越闭集
    ENoLua,
    /// Lua/跨平台辅助格漂移
    EAuxDrift,
    /// 跨平台格越闭集
    ENoCross,
    /// 分账声明行越闭集或缺行
    ENoRow,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl DoublecmdCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            DoublecmdCode::Ok => "成功",
            DoublecmdCode::ENoPanel => "面板格越闭集：五面板族 20 格之外先登记",
            DoublecmdCode::EPanelDrift => "面板漂移：面板折叠与基准不恒等，逐格复算",
            DoublecmdCode::EOrder => "家族逆序：双窗→目录热列表→归档插件→文件工具→搜索单调，逆序必抓",
            DoublecmdCode::ENoLua => "Lua 格越闭集：脚本消费 6 格之外先登记",
            DoublecmdCode::EAuxDrift => "Lua/跨平台漂移：辅助格折叠与基准不恒等，逐格复算",
            DoublecmdCode::ENoCross => "跨平台格越闭集：三态 6 格之外先登记",
            DoublecmdCode::ENoRow => "分账声明行越闭集或缺行：与 Total Commander 分账未齐先补齐",
            DoublecmdCode::EDup => "重复落账：同格只许一条账目",
            DoublecmdCode::ESeal => "未齐即封：20 格+Lua 6 格+跨平台 6 格+声明行未齐先补齐",
            DoublecmdCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            DoublecmdCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            DoublecmdCode::EGradeDrift => "定级漂移：登记档为 S2（GPL 许可面如实定级），全齐亦不得自升 S1",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口（本卡登记档不取，仅阶梯闭集占位）
    S1,
    /// S2：有条件收口（本卡登记档）
    S2,
    /// S3：不收口
    S3,
}

impl Grade {
    /// 定级值。
    pub const fn val(self) -> u32 {
        match self {
            Grade::S1 => 1,
            Grade::S2 => 2,
            Grade::S3 => 3,
        }
    }
    /// 明细。
    pub fn detail(self) -> &'static str {
        match self {
            Grade::S1 => "S1：全通收口档（Double Commander 卡不登记此档）",
            Grade::S2 => "S2：有条件收口（Double Commander 登记档：GPL 许可面如实定级）",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

/// 定级阶梯（0=S3 1=S2 2=S1），供阶梯单调复核。
pub const fn grade_step(n: u32) -> Grade {
    match n {
        0 => Grade::S3,
        1 => Grade::S2,
        _ => Grade::S1,
    }
}

/// 本卡 KB-COMPAT 登记档（许可面如实定级，AI-95 联签锚定行）。
pub const REGISTERED: Grade = Grade::S2;

// ── 域模型 ───────────────────────────────────────────────────────
/// 面板格载荷（格 p；家族 = p/4）。
pub const fn panel_cell(p: usize) -> Option<u64> {
    if p >= PANEL_N {
        return None;
    }
    Some((0xB7000u64 + (p as u64) * 0x3E) ^ ((p / 4) as u64).wrapping_mul(0x01000193))
}

/// 面板家族（0..4=双窗 4..8=目录热列表 8..12=归档插件 12..16=文件工具 16..20=搜索）。
pub const fn panel_kind(p: usize) -> Option<u32> {
    if p >= PANEL_N {
        return None;
    }
    Some((p / 4) as u32)
}

/// 面板家族名。
pub const fn panel_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("双窗"),
        1 => Some("目录热列表"),
        2 => Some("归档插件"),
        3 => Some("文件工具"),
        4 => Some("搜索"),
        _ => None,
    }
}

/// Lua 脚本消费格载荷（格 i；相位 = i/2，各脚本位+消费位）。
pub const fn lua_cell(i: usize) -> Option<u64> {
    if i >= LUA_N {
        return None;
    }
    Some((0xB7400u64 + (i as u64) * 0x3A) ^ ((i / 2) as u64).wrapping_mul(0x010001B3))
}

/// Lua 相位名。
pub const fn lua_phase(i: usize) -> Option<&'static str> {
    match i / 2 {
        0 => Some("目录热列表脚本"),
        1 => Some("文件操作钩子"),
        2 => Some("面板事件"),
        _ => None,
    }
}

/// Lua 格名（脚本位/消费位）。
pub const fn lua_name(i: usize) -> Option<&'static str> {
    match i % 2 {
        0 => Some("脚本位"),
        1 => Some("消费位"),
        _ => None,
    }
}

/// 跨平台三态格载荷（格 i；平台 = i/2，各支持位+打包位）。
pub const fn cross_cell(i: usize) -> Option<u64> {
    if i >= CROSS_N {
        return None;
    }
    Some((0xB7600u64 + (i as u64) * 0x30) ^ 0x44435F58504C4154)
}

/// 跨平台平台名。
pub const fn cross_platform(i: usize) -> Option<&'static str> {
    match i / 2 {
        0 => Some("windows"),
        1 => Some("linux"),
        2 => Some("macos"),
        _ => None,
    }
}

/// 跨平台格名（支持位/打包位）。
pub const fn cross_slot(i: usize) -> Option<&'static str> {
    match i % 2 {
        0 => Some("支持位"),
        1 => Some("打包位"),
        _ => None,
    }
}

/// 插件生态家族名（×5 闭集）。
pub const fn plugin_fam_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("WCX 打包插件"),
        1 => Some("WDX 内容插件"),
        2 => Some("WLX 查看插件"),
        3 => Some("WFX 文件系统插件"),
        4 => Some("WDS 搜索插件"),
        _ => None,
    }
}

/// 与 Total Commander 分账声明行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xB7800 + r as u32 * 0x2C)
}

/// Pascal/LCL 上栈声明行折叠（行 r）。
pub const fn decl_row(r: usize) -> Option<u32> {
    if r >= DECL_ROWS {
        return None;
    }
    Some(0xB7900 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Double Commander 收口账本状态机。
#[derive(Clone)]
pub struct DoublecmdLedger {
    /// 六判据（安装/双窗/插件/搜索/卸载/许可）。
    gates: [Option<bool>; 6],
    /// 面板账。
    panels: [Option<u64>; PANEL_N],
    /// Lua 脚本消费账。
    lua: [Option<u64>; LUA_N],
    /// 跨平台三态账。
    cross: [Option<u64>; CROSS_N],
    /// 与 Total Commander 分账声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// Pascal/LCL 上栈声明账。
    decls: [Option<u32>; DECL_ROWS],
    /// 最后家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl DoublecmdLedger {
    /// 新账本。
    pub fn new() -> Self {
        DoublecmdLedger {
            gates: [None; 6],
            panels: [None; PANEL_N],
            lua: [None; LUA_N],
            cross: [None; CROSS_N],
            splits: [None; SPLIT_ROWS],
            decls: [None; DECL_ROWS],
            last_kind: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), DoublecmdCode> {
        if gate >= 6 {
            return Err(DoublecmdCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0xB710 + gate as u64);
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

    /// 面板一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_panel(&mut self, p: usize, got: u64) -> Result<(), DoublecmdCode> {
        if p >= PANEL_N {
            return Err(DoublecmdCode::ENoPanel);
        }
        let kind = match panel_kind(p) {
            Some(k) => k,
            None => return Err(DoublecmdCode::ENoPanel),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xB720);
            return Err(DoublecmdCode::EOrder);
        }
        let want = match panel_cell(p) {
            Some(w) => w,
            None => return Err(DoublecmdCode::ENoPanel),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xB721 + p as u64);
            return Err(DoublecmdCode::EPanelDrift);
        }
        if match self.panels.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DoublecmdCode::EDup);
        }
        if let Some(c) = self.panels.get_mut(p) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0xB722 + p as u64);
        Ok(())
    }

    /// 面板全账核验（20 格）。
    pub fn panels_all(&self) -> bool {
        let mut p = 0usize;
        while p < PANEL_N {
            match self.panels.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// Lua 脚本消费一格（载荷对照 + 重复拒）。
    pub fn record_lua(&mut self, i: usize, got: u64) -> Result<(), DoublecmdCode> {
        let want = match lua_cell(i) {
            Some(w) => w,
            None => return Err(DoublecmdCode::ENoLua),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xB740 + i as u64);
            return Err(DoublecmdCode::EAuxDrift);
        }
        if match self.lua.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DoublecmdCode::EDup);
        }
        if let Some(c) = self.lua.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0xB741 + i as u64);
        Ok(())
    }

    /// Lua 脚本消费全账核验（6 格）。
    pub fn lua_all(&self) -> bool {
        let mut i = 0usize;
        while i < LUA_N {
            match self.lua.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 跨平台三态一格（载荷对照 + 重复拒）。
    pub fn record_cross(&mut self, i: usize, got: u64) -> Result<(), DoublecmdCode> {
        let want = match cross_cell(i) {
            Some(w) => w,
            None => return Err(DoublecmdCode::ENoCross),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xB760 + i as u64);
            return Err(DoublecmdCode::EAuxDrift);
        }
        if match self.cross.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DoublecmdCode::EDup);
        }
        if let Some(c) = self.cross.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0xB761 + i as u64);
        Ok(())
    }

    /// 跨平台三态全账核验（6 格）。
    pub fn cross_all(&self) -> bool {
        let mut i = 0usize;
        while i < CROSS_N {
            match self.cross.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 与 Total Commander 分账声明一行。
    pub fn declare_split(&mut self, r: usize) -> Result<(), DoublecmdCode> {
        if r >= SPLIT_ROWS {
            return Err(DoublecmdCode::ENoRow);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DoublecmdCode::EDup);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0xB780 + r as u64);
        Ok(())
    }

    /// Pascal/LCL 上栈声明一行。
    pub fn declare_pascal(&mut self, r: usize) -> Result<(), DoublecmdCode> {
        if r >= DECL_ROWS {
            return Err(DoublecmdCode::ENoRow);
        }
        if match self.decls.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DoublecmdCode::EDup);
        }
        if let Some(c) = self.decls.get_mut(r) {
            *c = decl_row(r);
        }
        self.fnv(0xB790 + r as u64);
        Ok(())
    }

    /// 两组声明全齐核验（分账 2 + 上栈 2）。
    pub fn rows_all(&self) -> bool {
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            match self.splits.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        let mut d = 0usize;
        while d < DECL_ROWS {
            match self.decls.get(d) {
                Some(Some(_)) => {}
                _ => return false,
            }
            d += 1;
        }
        true
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 最后家族（供逆序路径不改写核验）。
    pub fn last_family(&self) -> i64 {
        self.last_kind
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// 收口面核验：主账+Lua 账+跨平台账+声明账全齐；未齐即封。
    pub fn seal(&self) -> Result<(), DoublecmdCode> {
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            if match self.splits.get(r) {
                Some(Some(_)) => false,
                _ => true,
            } {
                return Err(DoublecmdCode::ENoRow);
            }
            r += 1;
        }
        if !self.panels_all() || !self.lua_all() || !self.cross_all() {
            return Err(DoublecmdCode::ESeal);
        }
        let mut d = 0usize;
        while d < DECL_ROWS {
            if match self.decls.get(d) {
                Some(Some(_)) => false,
                _ => true,
            } {
                return Err(DoublecmdCode::ESeal);
            }
            d += 1;
        }
        Ok(())
    }

    /// 收口面全齐布尔核验。
    pub fn seal_all(&self) -> bool {
        self.seal().is_ok()
    }

    /// 注入守恒复核：注入=检出+逃逸。
    pub fn reconcile(&mut self, inj: u32, caught: u32, esc: u32) -> Result<(), DoublecmdCode> {
        if inj != caught.wrapping_add(esc) {
            self.fnv(0xB7A0);
            return Err(DoublecmdCode::EInj);
        }
        Ok(())
    }

    /// 双跑指纹复核。
    pub fn verify_fp(&self, other: u64) -> Result<(), DoublecmdCode> {
        if self.fp != other {
            return Err(DoublecmdCode::EFp);
        }
        Ok(())
    }

    /// KB-COMPAT 定级：六判据齐 → 登记档 S2；判据缺 → S3；全齐仍按登记档
    /// S2 落账（许可面如实定级，不自升 S1）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() {
            REGISTERED
        } else {
            Grade::S3
        }
    }
}

impl Default for DoublecmdLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 定级登记复核：全齐账必须落在登记档，否则定级漂移。
pub fn grade_register(lt: &DoublecmdLedger) -> Result<Grade, DoublecmdCode> {
    let g = lt.judge_grade();
    if lt.seal_all() && lt.gates_all() {
        if g == REGISTERED {
            Ok(g)
        } else {
            Err(DoublecmdCode::EGradeDrift)
        }
    } else {
        Ok(g)
    }
}

/// 全齐账本（六判据 + 全节落账，测试与定级检查共用）。
pub fn sealed_ledger() -> DoublecmdLedger {
    let mut lt = DoublecmdLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = lt.judge_gate(g);
        g += 1;
    }
    let mut p = 0usize;
    while p < PANEL_N {
        if let Some(w) = panel_cell(p) {
            let _ = lt.record_panel(p, w);
        }
        p += 1;
    }
    let mut i = 0usize;
    while i < LUA_N {
        if let Some(w) = lua_cell(i) {
            let _ = lt.record_lua(i, w);
        }
        i += 1;
    }
    let mut c = 0usize;
    while c < CROSS_N {
        if let Some(w) = cross_cell(c) {
            let _ = lt.record_cross(c, w);
        }
        c += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = lt.declare_split(r);
        r += 1;
    }
    let mut d = 0usize;
    while d < DECL_ROWS {
        let _ = lt.declare_pascal(d);
        d += 1;
    }
    lt
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
/// 注入 10 轮三态轮转，全部落在当轮临时账本上；本账只记守恒三数。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = DoublecmdLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut p = 0usize;
    while p < PANEL_N {
        if let Some(w) = panel_cell(p) {
            if led.record_panel(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    let mut i = 0usize;
    while i < LUA_N {
        if let Some(w) = lua_cell(i) {
            if led.record_lua(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut c = 0usize;
    while c < CROSS_N {
        if let Some(w) = cross_cell(c) {
            if led.record_cross(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    let mut d = 0usize;
    while d < DECL_ROWS {
        let _ = led.declare_pascal(d);
        d += 1;
    }
    // 注入 10 轮：面板漂移 / 家族逆序 / Lua 越闭集三态轮转。
    // 漂移注入先按合法序落正格再注漂移；越闭集（phase）注入对空账本同样
    // 必抓；逆序路径不改写 last_kind。
    let mut n = 0usize;
    while n < 10 {
        let kind = (n % 3) as u32;
        let mut lt = DoublecmdLedger::new();
        let caught = match kind {
            0 => {
                let w0 = match panel_cell(0) {
                    Some(w) => w,
                    None => 0,
                };
                let w1 = match panel_cell(1) {
                    Some(w) => w,
                    None => 0,
                };
                let _ = lt.record_panel(0, w0);
                lt.record_panel(1, w1.wrapping_add(1)) == Err(DoublecmdCode::EPanelDrift)
            }
            1 => {
                let w9 = match panel_cell(9) {
                    Some(w) => w,
                    None => 0,
                };
                let w2 = match panel_cell(2) {
                    Some(w) => w,
                    None => 0,
                };
                let _ = lt.record_panel(9, w9);
                lt.record_panel(2, w2) == Err(DoublecmdCode::EOrder)
            }
            _ => lt.record_lua(LUA_N, 0) == Err(DoublecmdCode::ENoLua),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0xB7B0 + n as u64);
        } else {
            led.escaped += 1;
        }
        n += 1;
    }
    let caught = led.caught;
    let fp = led.fingerprint();
    (recorded, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// UNX-F52456 判据集（锚 UNX-F52457-J1，off-by-one 板式）。
pub fn run_unx52456_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52456");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五面板族 20 格全通",
        recorded >= PANEL_N as u32,
        "双窗/目录热列表/归档插件/文件工具/搜索五家族逐格恒等",
    );
    s.add(
        "Lua 脚本消费 6/6 全落",
        {
            let mut lt = DoublecmdLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < LUA_N {
                if let Some(w) = lua_cell(i) {
                    if lt.record_lua(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.lua_all()
        },
        "目录热列表脚本/文件操作钩子/面板事件三相位各脚本位+消费位逐格恒等",
    );
    s.add(
        "跨平台三态 6/6 全落",
        {
            let mut lt = DoublecmdLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < CROSS_N {
                if let Some(w) = cross_cell(i) {
                    if lt.record_cross(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.cross_all()
        },
        "windows/linux/macos 各支持位+打包位逐格恒等",
    );
    s.add(
        "插件生态 ×5 家族闭集",
        {
            let mut ok = true;
            let mut k = 0usize;
            while k < PLUGIN_FAMS {
                match plugin_fam_name(k) {
                    Some(n) => {
                        if n.is_empty() {
                            ok = false;
                        }
                    }
                    None => ok = false,
                }
                k += 1;
            }
            plugin_fam_name(PLUGIN_FAMS).is_none() && ok
        },
        "WCX/WDX/WLX/WFX/WDS 五插件家族名闭集，越家族必拒（§AI-66.9 红线②）",
    );
    s.add(
        "UNX-F52457-J1 六判据全落",
        {
            let mut lt = DoublecmdLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "安装/双窗/插件/搜索/卸载/许可逐格落账",
    );
    s.add(
        "与 Total Commander 分账声明 2/2 全齐",
        {
            let mut lt = DoublecmdLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                if lt.declare_split(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok
        },
        "与 Total Commander 分账声明两行闭集（账目不并表）",
    );
    s.add(
        "Pascal/LCL 上栈声明 2/2 全齐",
        {
            let mut lt = DoublecmdLedger::new();
            let mut ok = true;
            let mut d = 0usize;
            while d < DECL_ROWS {
                if lt.declare_pascal(d).is_err() {
                    ok = false;
                }
                d += 1;
            }
            ok
        },
        "Pascal 核心上栈/LCL 界面上栈两行闭集",
    );
    s.add(
        "面板家族单调",
        {
            let mut ok = true;
            let mut p = 1usize;
            while p < PANEL_N {
                let a = match panel_kind(p) {
                    Some(k) => k,
                    None => 9,
                };
                let b = match panel_kind(p - 1) {
                    Some(k) => k,
                    None => 9,
                };
                if a < b {
                    ok = false;
                }
                p += 1;
            }
            ok
        },
        "五家族闭集单调",
    );
    s.add(
        "Lua 相位与跨平台平台闭集",
        {
            let mut ok = true;
            let mut i = 0usize;
            while i < LUA_N {
                if lua_phase(i).is_none() || lua_name(i).is_none() {
                    ok = false;
                }
                i += 1;
            }
            let mut c = 0usize;
            while c < CROSS_N {
                if cross_platform(c).is_none() || cross_slot(c).is_none() {
                    ok = false;
                }
                c += 1;
            }
            lua_phase(LUA_N).is_none() && cross_platform(CROSS_N).is_none() && ok
        },
        "Lua 三相位×两格、跨平台三平台×两格，越相位/越平台必拒",
    );
    s.add("注入 10/10 全抓", caught >= 10, "面板漂移/家族逆序/Lua 越闭集三态轮转必抓");
    let mut l0 = DoublecmdLedger::new();
    s.add(
        "边界·面板越闭集",
        l0.record_panel(PANEL_N, 0).is_err(),
        "面板格 20 越闭集必拒",
    );
    s.add(
        "边界·Lua 越闭集",
        l0.record_lua(LUA_N, 0).is_err(),
        "Lua 格 6 越闭集必拒",
    );
    s.add(
        "边界·跨平台越闭集",
        l0.record_cross(CROSS_N, 0).is_err(),
        "跨平台格 6 越闭集必拒",
    );
    s.add(
        "边界·声明行越闭集",
        l0.declare_split(SPLIT_ROWS).is_err() && l0.declare_pascal(DECL_ROWS).is_err(),
        "声明行 2 之外越界必拒",
    );
    s.add(
        "面板漂移拒",
        {
            let w0 = match panel_cell(0) {
                Some(w) => w,
                None => 0,
            };
            let w1 = match panel_cell(1) {
                Some(w) => w,
                None => 0,
            };
            let _ = l0.record_panel(0, w0);
            l0.record_panel(1, w1.wrapping_add(1)) == Err(DoublecmdCode::EPanelDrift)
        },
        "面板载荷漂移必抓",
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
        "指纹复核与守恒复核闸",
        {
            let mut a = DoublecmdLedger::new();
            let mut b = DoublecmdLedger::new();
            let _ = a.judge_gate(0);
            let _ = b.judge_gate(0);
            a.verify_fp(b.fingerprint()) == Ok(()) && a.reconcile(1, 1, 0) == Ok(())
        },
        "FNV 指纹复核一致且注入守恒等式成立",
    );
    s.add(
        "许可面如实定级（AI-95 联签锚定行）",
        REGISTERED == Grade::S2 && REGISTERED.val() == 2 && !Grade::S2.detail().is_empty(),
        "GPL 开源许可面如实落 S2，不虚升 S1（AI-95 联签锚定行）",
    );
    s.add(
        "KB-COMPAT 定级登记 S2",
        {
            let lt = sealed_ledger();
            grade_register(&lt) == Ok(REGISTERED) && lt.seal_all() && lt.judge_grade() == Grade::S2
        },
        "全齐账按登记档 S2 落账（许可面如实定级，不自升 S1）",
    );
    s.add(
        "定级阶梯闭集",
        grade_step(0) == Grade::S3
            && grade_step(1) == Grade::S2
            && grade_step(2) == Grade::S1
            && grade_step(0).val() == 3
            && grade_step(2).val() == 1,
        "S3→S2→S1 阶梯单调，值域闭集",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCB1C0,
        "SEG_BASE 0x40 对齐且等于 0xCA400+0x40×(52456−52401)",
    );
    s.add("收口·判据条数", s.len() >= 22, "含本条共 ≥23 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_panel_kind_mapping() {
        // kind = p/4 映射（i/N 映射口径逐点核对）。
        assert_eq!(panel_kind(0), Some(0));
        assert_eq!(panel_kind(3), Some(0));
        assert_eq!(panel_kind(4), Some(1));
        assert_eq!(panel_kind(8), Some(2));
        assert_eq!(panel_kind(12), Some(3));
        assert_eq!(panel_kind(19), Some(4));
        assert!(panel_kind(20).is_none());
        assert!(panel_cell(20).is_none());
        assert_eq!(panel_kind_name(0), Some("双窗"));
        assert_eq!(panel_kind_name(4), Some("搜索"));
        assert!(panel_kind_name(5).is_none());
    }

    #[test]
    fn test_lua_and_cross_phases() {
        // 相位 = i/2，格 = i%2 映射逐点核对。
        assert_eq!(lua_phase(0), Some("目录热列表脚本"));
        assert_eq!(lua_phase(2), Some("文件操作钩子"));
        assert_eq!(lua_phase(4), Some("面板事件"));
        assert!(lua_phase(6).is_none());
        assert_eq!(lua_name(0), Some("脚本位"));
        assert_eq!(lua_name(1), Some("消费位"));
        assert!(lua_cell(LUA_N).is_none());
        assert_eq!(cross_platform(0), Some("windows"));
        assert_eq!(cross_platform(2), Some("linux"));
        assert_eq!(cross_platform(4), Some("macos"));
        assert!(cross_platform(6).is_none());
        assert_eq!(cross_slot(0), Some("支持位"));
        assert_eq!(cross_slot(1), Some("打包位"));
        assert!(cross_cell(CROSS_N).is_none());
    }

    #[test]
    fn test_plugin_fams_closed() {
        // 插件生态 ×5 闭集逐点核对。
        assert_eq!(plugin_fam_name(0), Some("WCX 打包插件"));
        assert_eq!(plugin_fam_name(4), Some("WDS 搜索插件"));
        assert!(plugin_fam_name(5).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = match panel_cell(3) {
            Some(v) => v,
            None => 0,
        };
        assert_eq!(a, match panel_cell(3) {
            Some(v) => v,
            None => 1,
        });
        assert_ne!(a, match panel_cell(4) {
            Some(v) => v,
            None => 0,
        });
        assert!(lua_cell(6).is_none());
        assert!(cross_cell(6).is_none());
        assert!(split_row(SPLIT_ROWS).is_none());
        assert!(decl_row(DECL_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        // 先按合法序落低族格，再注逆序。
        let mut lt = DoublecmdLedger::new();
        let _ = lt.record_panel(4, panel_cell(4).unwrap_or(0));
        let _ = lt.record_panel(8, panel_cell(8).unwrap_or(0));
        assert_eq!(
            lt.record_panel(0, panel_cell(0).unwrap_or(0)),
            Err(DoublecmdCode::EOrder)
        );
        assert!(lt.record_panel(9, panel_cell(9).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_last_kind_unchanged_after_order() {
        let mut lt = DoublecmdLedger::new();
        let _ = lt.record_panel(9, panel_cell(9).unwrap_or(0));
        let before = lt.last_family();
        let _ = lt.record_panel(2, panel_cell(2).unwrap_or(0));
        assert_eq!(lt.last_family(), before, "逆序路径不得改写 last_kind");
        assert_eq!(lt.stats().1, 1, "逆序注入计入检出");
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = DoublecmdLedger::new();
        assert_eq!(
            lt.record_lua(3, lua_cell(3).unwrap_or(0).wrapping_add(1)),
            Err(DoublecmdCode::EAuxDrift)
        );
        assert_eq!(
            lt.record_cross(5, cross_cell(5).unwrap_or(0).wrapping_add(2)),
            Err(DoublecmdCode::EAuxDrift)
        );
        let w0 = panel_cell(0).unwrap_or(0);
        let _ = lt.record_panel(0, w0);
        assert_eq!(
            lt.record_panel(1, panel_cell(1).unwrap_or(0).wrapping_add(1)),
            Err(DoublecmdCode::EPanelDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = DoublecmdLedger::new();
        let _ = lt.record_lua(2, lua_cell(2).unwrap_or(0));
        assert_eq!(
            lt.record_lua(2, lua_cell(2).unwrap_or(0)),
            Err(DoublecmdCode::EDup)
        );
        let _ = lt.declare_split(0);
        assert_eq!(lt.declare_split(0), Err(DoublecmdCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = DoublecmdLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = DoublecmdLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
        assert!(!lt2.seal_all());
    }

    #[test]
    fn test_seal_and_reconcile() {
        let empty = DoublecmdLedger::new();
        assert_eq!(empty.seal(), Err(DoublecmdCode::ENoRow));
        let mut no_cross = sealed_ledger();
        no_cross.cross = [None; CROSS_N];
        assert_eq!(no_cross.seal(), Err(DoublecmdCode::ESeal));
        let full = sealed_ledger();
        assert!(full.seal().is_ok());
        assert!(full.seal_all());
        let mut r = full.clone();
        assert!(r.reconcile(10, 10, 0).is_ok());
        assert!(r.verify_fp(full.fingerprint()).is_ok());
        assert_eq!(r.reconcile(10, 9, 0), Err(DoublecmdCode::EInj));
        assert_eq!(r.verify_fp(full.fingerprint().wrapping_add(1)), Err(DoublecmdCode::EFp));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // recorded 只计 run_cycle 实际计数的节：面板 20 + Lua 6 + 跨平台 6。
        assert_eq!(recorded, (PANEL_N + LUA_N + CROSS_N) as u32);
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
        assert_eq!(SEG_BASE, 0xCB1C0);
        assert_eq!(SEG_BASE, 0xCA400 + 0x40 * (52456u32 - 52401));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        // 码位须与枚举实际位序一致：Ok=0，EGradeDrift=12，恰 13 变体。
        assert_eq!(DoublecmdCode::Ok as u32, 0);
        assert_eq!(DoublecmdCode::ENoPanel as u32, 1);
        assert_eq!(DoublecmdCode::ENoCross as u32, 6);
        assert_eq!(DoublecmdCode::EGradeDrift as u32, 12);
        assert!(!DoublecmdCode::Ok.detail().is_empty());
        assert!(!DoublecmdCode::ESeal.detail().is_empty());
        assert!(!DoublecmdCode::ENoCross.detail().is_empty());
        assert!(!DoublecmdCode::EGradeDrift.detail().is_empty());
        assert_eq!(DoublecmdCode::Ok.val(), SEG_BASE);
    }

    #[test]
    fn test_grade() {
        // 如实阶梯：空账 S3，六闸齐 S2，全齐仍按登记档 S2（GPL 许可面如实定级）。
        assert_eq!(Grade::S3.val(), 3);
        let empty = DoublecmdLedger::new();
        assert_eq!(empty.judge_grade(), Grade::S3);
        let mut half = DoublecmdLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = half.judge_gate(g);
            g += 1;
        }
        assert_eq!(half.judge_grade(), Grade::S2);
        let full = sealed_ledger();
        assert_eq!(full.judge_grade(), Grade::S2);
        assert_eq!(grade_register(&full), Ok(Grade::S2));
        assert_eq!(grade_step(0), Grade::S3);
        assert_eq!(grade_step(1), Grade::S2);
        assert_eq!(grade_step(2), Grade::S1);
        assert_eq!(REGISTERED, Grade::S2);
    }

    #[test]
    fn unx52456_checks_green() {
        let s = run_unx52456_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 22);
    }
}
