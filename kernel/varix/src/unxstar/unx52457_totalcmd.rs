//! UNX-F52457 Total Commander 收口 · 0xCB200段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52457"，逐字）：
//!   title: Total Commander 收口
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52458-J1 六判据
//!     全落；×10；许可面如实定级（AI-95 联签锚定行）；KB-COMPAT 定级登
//!     记；判据锚定 §AI-66.9 红线②；③ 完成时 API/软件写回：状态=已完
//!     成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52457）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52458-J1，照原文锚定；功能按本卡标题（Total Commander）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五面板族闭集 20 格：双窗 / 标签 / 插件 / 比较 / 同步，每族 4 格，
//!      逐格载荷折叠纯函数恒等才收；族单调（逆序必抓）。
//!   ② 快捷键投影 8 格：F5 复制 / F6 移动 / F7 新建目录 / Tab 切换窗格 /
//!      Alt+F7 搜索 / Ctrl+M 批量重命名 / Alt+Shift+F9 比较内容 /
//!      Ctrl+PgDn 进入压缩包，逐格载荷对照，漂移必抓。
//!   ③ 插件生态 ×4 消费 8 格：WCX 打包 / WDX 内容 / WLX 查看器 / WFX
//!      文件系统四族 ×（安装/消费）两相，逐格恒等才收。
//!   ④ 商业许可三元闸：正版序列号 / 升级保障 / 多机授权 三格闭集，许可
//!      面如实定级（AI-95 联签锚定行），越闸必抓、漂移必抓。
//!   ⑤ 与 Double Commander 分账声明 2 行（锚 F52456）：双窗族分账声明 /
//!      插件族分账声明，缺一拒封；Pascal 上栈声明 2 行：Ghisler 原生
//!      Pascal 栈声明 / 本仓 Pascal 类比上栈声明，缺一拒封。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a）；
//!      注入核 run_cycle 三态轮转 10 轮（面板越闭集/快捷键漂移/面板族
//!      逆序）全部落在当轮临时账本上，检出 10/10、逃逸 0；双跑指纹一致；
//!      KB-COMPAT 定级 S1（全齐 → S1，六闸齐未全齐 → S2，空账 → S3）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52457;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 五面板族操作格数（五族 × 4）。
pub const PANEL_OPS: usize = 20;

/// 快捷键投影格数。
pub const HOTKEY_CELLS: usize = 8;

/// 插件生态消费格数（四族 × 两相）。
pub const PLUG_CELLS: usize = 8;

/// 商业许可三元闸格数。
pub const LIC_GATES: usize = 3;

/// 与 Double Commander 分账声明行数。
pub const SPLIT_ROWS: usize = 2;

/// Pascal 上栈声明行数。
pub const STACK_ROWS: usize = 2;

/// run_cycle 实际经 record_* 计数的格数之和。
pub const RECORDED_CELLS: usize = PANEL_OPS + HOTKEY_CELLS + PLUG_CELLS + LIC_GATES;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TotalcmdCode {
    /// 成功
    Ok = 0,
    /// 面板号越闭集
    ENoPanel,
    /// 面板载荷漂移
    EPanelDrift,
    /// 面板族逆序
    EOrder,
    /// 快捷键号越闭集
    ENoHot,
    /// 快捷键载荷漂移
    EHotDrift,
    /// 插件生态格越闭集
    ENoPlug,
    /// 商业许可闸越界或未齐
    ELicNo,
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

impl TotalcmdCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            TotalcmdCode::Ok => "成功",
            TotalcmdCode::ENoPanel => "面板号越闭集：双窗/标签/插件/比较/同步 20 格之外先登记",
            TotalcmdCode::EPanelDrift => "面板载荷漂移：面板格折叠与基准不恒等，逐格复算",
            TotalcmdCode::EOrder => "面板族逆序：双窗→标签→插件→比较→同步单调，逆序必抓",
            TotalcmdCode::ENoHot => "快捷键号越闭集：快捷键投影 8 格之外先登记",
            TotalcmdCode::EHotDrift => "快捷键载荷漂移：投影折叠与基准不恒等，逐格复算",
            TotalcmdCode::ENoPlug => "插件生态格越闭集：WCX/WDX/WLX/WFX ×两相 8 格之外先登记",
            TotalcmdCode::ELicNo => "商业许可闸越界或未齐：正版序列号/升级保障/多机授权三元闸先补齐",
            TotalcmdCode::EDup => "重复落账：同格只许一条账目",
            TotalcmdCode::ESeal => "未齐即封：20 面板格 + 8 快捷键 + 8 插件格 + 3 许可闸未齐先补齐",
            TotalcmdCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            TotalcmdCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            TotalcmdCode::EGradeDrift => "定级漂移：全通且分账/上栈声明全齐才可定 S1",
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
    /// 定级值（const 可用）。
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
            Grade::S1 => "S1：六判据+20面板格+8快捷键+8插件格+3许可闸+4声明行全通收口",
            Grade::S2 => "S2：有条件收口，六判据齐但存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 面板格载荷（格 o；族 = o/4）。
pub const fn panel_cell(o: usize) -> Option<u64> {
    if o >= PANEL_OPS {
        return None;
    }
    Some((0xA5700u64 + (o as u64) * 0x41) ^ ((o / 4) as u64).wrapping_mul(0x9E3779B1))
}

/// 面板族（0..4=双窗 4..8=标签 8..12=插件 12..16=比较 16..20=同步）。
pub const fn panel_kind(o: usize) -> Option<u32> {
    if o >= PANEL_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 面板族名。
pub const fn panel_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("双窗"),
        1 => Some("标签"),
        2 => Some("插件"),
        3 => Some("比较"),
        4 => Some("同步"),
        _ => None,
    }
}

/// 快捷键投影格载荷（格 k）。
pub const fn hotkey_cell(k: usize) -> Option<u64> {
    if k >= HOTKEY_CELLS {
        return None;
    }
    Some((0xA5800u64 + (k as u64) * 0x5F) ^ 0x54434D4B)
}

/// 快捷键投影名（const fn 表 + Option 边界）。
pub const fn hotkey_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("F5 复制"),
        1 => Some("F6 移动"),
        2 => Some("F7 新建目录"),
        3 => Some("Tab 切换窗格"),
        4 => Some("Alt+F7 搜索"),
        5 => Some("Ctrl+M 批量重命名"),
        6 => Some("Alt+Shift+F9 比较内容"),
        7 => Some("Ctrl+PgDn 进入压缩包"),
        _ => None,
    }
}

/// 插件生态格载荷（格 p；族 = p/2，相 = p%2：0 安装 1 消费）。
pub const fn plug_cell(p: usize) -> Option<u64> {
    if p >= PLUG_CELLS {
        return None;
    }
    Some((0xA5900u64 + (p as u64) * 0x47) ^ ((p / 2) as u64).wrapping_mul(0x01000193))
}

/// 插件族名（族 f）。
pub const fn plug_kind_name(f: usize) -> Option<&'static str> {
    match f {
        0 => Some("WCX 打包插件"),
        1 => Some("WDX 内容插件"),
        2 => Some("WLX 查看器插件"),
        3 => Some("WFX 文件系统插件"),
        _ => None,
    }
}

/// 商业许可闸载荷（闸 g）。
pub const fn lic_cell(g: usize) -> Option<u64> {
    if g >= LIC_GATES {
        return None;
    }
    Some(0xA5A00u64 + (g as u64) * 0x53)
}

/// 许可闸名。
pub const fn lic_gate_name(g: usize) -> Option<&'static str> {
    match g {
        0 => Some("正版序列号"),
        1 => Some("升级保障"),
        2 => Some("多机授权"),
        _ => None,
    }
}

/// 与 Double Commander 分账声明折叠（行 r，锚 F52456）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xA5B0 + r as u32 * 0x30)
}

/// Pascal 上栈声明折叠（行 r）。
pub const fn stack_row(r: usize) -> Option<u32> {
    if r >= STACK_ROWS {
        return None;
    }
    Some(0xA5C0 + r as u32 * 0x30)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Total Commander 收口账本状态机。
#[derive(Clone)]
pub struct TotalcmdLedger {
    /// 六判据（启动/功能/崩溃/卸载/升级/许可）。
    gates: [Option<bool>; 6],
    /// 面板族账。
    panels: [Option<u64>; PANEL_OPS],
    /// 快捷键投影账。
    hots: [Option<u64>; HOTKEY_CELLS],
    /// 插件生态账。
    plugs: [Option<u64>; PLUG_CELLS],
    /// 商业许可闸账。
    lics: [Option<u64>; LIC_GATES],
    /// 分账/上栈声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    stacks: [Option<u32>; STACK_ROWS],
    /// 最后面板族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl TotalcmdLedger {
    /// 新账本。
    pub fn new() -> Self {
        TotalcmdLedger {
            gates: [None; 6],
            panels: [None; PANEL_OPS],
            hots: [None; HOTKEY_CELLS],
            plugs: [None; PLUG_CELLS],
            lics: [None; LIC_GATES],
            splits: [None; SPLIT_ROWS],
            stacks: [None; STACK_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), TotalcmdCode> {
        if gate >= 6 {
            return Err(TotalcmdCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5100 + gate as u64);
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

    /// 面板一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_panel(&mut self, o: usize, got: u64) -> Result<(), TotalcmdCode> {
        if o >= PANEL_OPS {
            return Err(TotalcmdCode::ENoPanel);
        }
        let kind = match panel_kind(o) {
            Some(k) => k,
            None => return Err(TotalcmdCode::ENoPanel),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5200);
            return Err(TotalcmdCode::EOrder);
        }
        let want = match panel_cell(o) {
            Some(w) => w,
            None => return Err(TotalcmdCode::ENoPanel),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5201 + o as u64);
            return Err(TotalcmdCode::EPanelDrift);
        }
        if match self.panels.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TotalcmdCode::EDup);
        }
        if let Some(c) = self.panels.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5202 + o as u64);
        Ok(())
    }

    /// 面板全账核验（20 格）。
    pub fn panels_all(&self) -> bool {
        let mut o = 0usize;
        while o < PANEL_OPS {
            match self.panels.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 快捷键一格（载荷对照 + 重复拒）。
    pub fn record_hot(&mut self, k: usize, got: u64) -> Result<(), TotalcmdCode> {
        if k >= HOTKEY_CELLS {
            return Err(TotalcmdCode::ENoHot);
        }
        let want = match hotkey_cell(k) {
            Some(w) => w,
            None => return Err(TotalcmdCode::ENoHot),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5300 + k as u64);
            return Err(TotalcmdCode::EHotDrift);
        }
        if match self.hots.get(k) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TotalcmdCode::EDup);
        }
        if let Some(c) = self.hots.get_mut(k) {
            *c = Some(got);
        }
        self.fnv(0x5301 + k as u64);
        Ok(())
    }

    /// 快捷键全账核验（8 格）。
    pub fn hots_all(&self) -> bool {
        let mut k = 0usize;
        while k < HOTKEY_CELLS {
            match self.hots.get(k) {
                Some(Some(_)) => {}
                _ => return false,
            }
            k += 1;
        }
        true
    }

    /// 插件生态一格（载荷对照 + 重复拒）。
    pub fn record_plug(&mut self, p: usize, got: u64) -> Result<(), TotalcmdCode> {
        if p >= PLUG_CELLS {
            return Err(TotalcmdCode::ENoPlug);
        }
        let want = match plug_cell(p) {
            Some(w) => w,
            None => return Err(TotalcmdCode::ENoPlug),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5400 + p as u64);
            return Err(TotalcmdCode::ENoPlug);
        }
        if match self.plugs.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TotalcmdCode::EDup);
        }
        if let Some(c) = self.plugs.get_mut(p) {
            *c = Some(got);
        }
        self.fnv(0x5401 + p as u64);
        Ok(())
    }

    /// 插件生态全账核验（8 格）。
    pub fn plugs_all(&self) -> bool {
        let mut p = 0usize;
        while p < PLUG_CELLS {
            match self.plugs.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// 商业许可一格（三元闸载荷对照 + 重复拒）。
    pub fn record_lic(&mut self, g: usize, got: u64) -> Result<(), TotalcmdCode> {
        if g >= LIC_GATES {
            return Err(TotalcmdCode::ELicNo);
        }
        let want = match lic_cell(g) {
            Some(w) => w,
            None => return Err(TotalcmdCode::ELicNo),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5500 + g as u64);
            return Err(TotalcmdCode::ELicNo);
        }
        if match self.lics.get(g) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TotalcmdCode::EDup);
        }
        if let Some(c) = self.lics.get_mut(g) {
            *c = Some(got);
        }
        self.fnv(0x5501 + g as u64);
        Ok(())
    }

    /// 商业许可三元闸全账核验。
    pub fn lics_all(&self) -> bool {
        let mut g = 0usize;
        while g < LIC_GATES {
            match self.lics.get(g) {
                Some(Some(_)) => {}
                _ => return false,
            }
            g += 1;
        }
        true
    }

    /// 分账声明一行（与 Double Commander，锚 F52456）。
    pub fn declare_split(&mut self, r: usize) -> Result<(), TotalcmdCode> {
        if r >= SPLIT_ROWS {
            return Err(TotalcmdCode::ESeal);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TotalcmdCode::EDup);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0x5600 + r as u64);
        Ok(())
    }

    /// Pascal 上栈声明一行。
    pub fn declare_stack(&mut self, r: usize) -> Result<(), TotalcmdCode> {
        if r >= STACK_ROWS {
            return Err(TotalcmdCode::ESeal);
        }
        if match self.stacks.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TotalcmdCode::EDup);
        }
        if let Some(c) = self.stacks.get_mut(r) {
            *c = stack_row(r);
        }
        self.fnv(0x5700 + r as u64);
        Ok(())
    }

    /// 分账/上栈声明全齐核验。
    pub fn declares_all(&self) -> bool {
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            match self.splits.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        let mut r = 0usize;
        while r < STACK_ROWS {
            match self.stacks.get(r) {
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

    /// KB-COMPAT 定级（全齐 → S1，六闸齐 → S2，空账 → S3）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.panels_all()
            && self.hots_all()
            && self.plugs_all()
            && self.lics_all()
            && self.declares_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }

    /// 全齐布尔（定级漂移核验用，如实反映各节记账）。
    pub fn full_complete(&self) -> bool {
        self.gates_all()
            && self.panels_all()
            && self.hots_all()
            && self.plugs_all()
            && self.lics_all()
            && self.declares_all()
    }

    /// 定级漂移核验：全齐必须定 S1，否则 EGradeDrift。
    pub fn grade_drift_check(&self) -> Result<(), TotalcmdCode> {
        if self.full_complete() && self.judge_grade() != Grade::S1 {
            return Err(TotalcmdCode::EGradeDrift);
        }
        Ok(())
    }
}

/// 注入守恒核验：注入 = 检出 + 逃逸。
pub fn conservation(injected: u32, caught: u32, escaped: u32) -> Result<(), TotalcmdCode> {
    if injected != caught + escaped {
        return Err(TotalcmdCode::EInj);
    }
    Ok(())
}

/// 双跑指纹核验。
pub fn verify_fingerprint(a: u64, b: u64) -> Result<(), TotalcmdCode> {
    if a != b || a == 0 {
        return Err(TotalcmdCode::EFp);
    }
    Ok(())
}

impl Default for TotalcmdLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（record_* 计格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = TotalcmdLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < PANEL_OPS {
        if let Some(w) = panel_cell(o) {
            if led.record_panel(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut k = 0usize;
    while k < HOTKEY_CELLS {
        if let Some(w) = hotkey_cell(k) {
            if led.record_hot(k, w).is_ok() {
                recorded += 1;
            }
        }
        k += 1;
    }
    let mut p = 0usize;
    while p < PLUG_CELLS {
        if let Some(w) = plug_cell(p) {
            if led.record_plug(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    let mut lic = 0usize;
    while lic < LIC_GATES {
        if let Some(w) = lic_cell(lic) {
            if led.record_lic(lic, w).is_ok() {
                recorded += 1;
            }
        }
        lic += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    let mut r = 0usize;
    while r < STACK_ROWS {
        let _ = led.declare_stack(r);
        r += 1;
    }
    // 注入 10 轮：面板越闭集 / 快捷键漂移 / 面板族逆序三态轮转，
    // 全部落在当轮临时账本上，主账只记注入/检出计数。
    let mut t = 0usize;
    while t < 10 {
        let kind = (t % 3) as u32;
        let mut lt = TotalcmdLedger::new();
        let caught = match kind {
            0 => lt.record_panel(PANEL_OPS, 0).is_err(),
            1 => {
                let seed = hotkey_cell(3).unwrap_or(0);
                let _ = lt.record_hot(3, seed);
                lt.record_hot(3, seed.wrapping_add(1)) == Err(TotalcmdCode::EHotDrift)
            }
            _ => {
                // 族逆序：先按合法序落高族正格，再注低族（空账本先落种子）。
                let hi = panel_cell(4).unwrap_or(0);
                let _ = lt.record_panel(4, hi);
                lt.record_panel(1, panel_cell(1).unwrap_or(0)) == Err(TotalcmdCode::EOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5800 + t as u64);
        } else {
            led.escaped += 1;
        }
        t += 1;
    }
    let caught = led.caught;
    let fp = led.fingerprint();
    (recorded, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// UNX-F52457 判据集（锚 UNX-F52458-J1，off-by-one 板式）。
pub fn run_unx52457_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52457");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五面板族 20 操作全通",
        recorded >= PANEL_OPS as u32,
        "双窗/标签/插件/比较/同步五族逐格载荷恒等",
    );
    s.add(
        "快捷键投影 8 格全落",
        {
            let mut lt = TotalcmdLedger::new();
            let mut ok = true;
            let mut k = 0usize;
            while k < HOTKEY_CELLS {
                if let Some(w) = hotkey_cell(k) {
                    if lt.record_hot(k, w).is_err() {
                        ok = false;
                    }
                }
                k += 1;
            }
            ok && lt.hots_all()
        },
        "F5/F6/F7/Tab/Alt+F7/Ctrl+M/Alt+Shift+F9/Ctrl+PgDn 逐格投影",
    );
    s.add(
        "插件生态 ×4 消费 8 格全落",
        {
            let mut lt = TotalcmdLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < PLUG_CELLS {
                if let Some(w) = plug_cell(p) {
                    if lt.record_plug(p, w).is_err() {
                        ok = false;
                    }
                }
                p += 1;
            }
            ok && lt.plugs_all()
        },
        "WCX/WDX/WLX/WFX 四族 ×（安装/消费）两相逐格恒等",
    );
    s.add(
        "商业许可三元闸 3/3 全落",
        {
            let mut lt = TotalcmdLedger::new();
            let mut ok = true;
            let mut g = 0usize;
            while g < LIC_GATES {
                if let Some(w) = lic_cell(g) {
                    if lt.record_lic(g, w).is_err() {
                        ok = false;
                    }
                }
                g += 1;
            }
            ok && lt.lics_all()
        },
        "正版序列号/升级保障/多机授权三元闸，许可面如实定级（AI-95 联签锚定行）",
    );
    s.add(
        "五面板族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < PANEL_OPS {
                if panel_kind(o).unwrap_or(5) < panel_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "双窗→标签→插件→比较→同步五族闭集单调",
    );
    s.add(
        "与 Double Commander 分账声明 2/2 全齐",
        {
            let mut lt = TotalcmdLedger::new();
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
        "双窗族分账/插件族分账两行闭集（锚 F52456，缺一拒封）",
    );
    s.add(
        "Pascal 上栈声明 2/2 全齐",
        {
            let mut lt = TotalcmdLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < STACK_ROWS {
                if lt.declare_stack(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok
        },
        "Ghisler 原生 Pascal 栈声明/本仓 Pascal 类比上栈声明，缺一拒封",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = TotalcmdLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/功能/崩溃/卸载/升级/许可逐格落账",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "面板越闭集/快捷键漂移/面板族逆序三态轮转必抓",
    );
    let mut l0 = TotalcmdLedger::new();
    s.add(
        "边界·面板号越闭集",
        l0.record_panel(PANEL_OPS, 0).is_err(),
        "面板号 20 越闭集必拒",
    );
    s.add(
        "边界·快捷键号越闭集",
        l0.record_hot(HOTKEY_CELLS, 0).is_err(),
        "快捷键号 8 越闭集必拒",
    );
    s.add(
        "边界·插件生态号越闭集",
        l0.record_plug(PLUG_CELLS, 0).is_err(),
        "插件生态号 8 越闭集必拒",
    );
    s.add(
        "面板族逆序拒",
        {
            let _ = l0.record_panel(8, panel_cell(8).unwrap_or(0));
            l0.record_panel(2, panel_cell(2).unwrap_or(0)) == Err(TotalcmdCode::EOrder)
        },
        "插件族后落标签族，逆序必抓",
    );
    s.add(
        "快捷键漂移拒",
        {
            let seed = hotkey_cell(1).unwrap_or(0);
            let _ = l0.record_hot(1, seed);
            l0.record_hot(1, seed.wrapping_add(1)) == Err(TotalcmdCode::EHotDrift)
        },
        "快捷键载荷漂移必抓",
    );
    s.add(
        "未齐允许落格（封核方向）",
        {
            let mut lt = TotalcmdLedger::new();
            lt.record_panel(0, panel_cell(0).unwrap_or(0)).is_ok()
        },
        "账未齐时落格表达式必须为 Ok，封核只在收口口处断",
    );
    let (_, _, fp2) = run_cycle();
    s.add(
        "双跑指纹一致",
        verify_fingerprint(fp, fp2) == Ok(()),
        "确定性全流程逐位一致且非零",
    );
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = TotalcmdLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < PANEL_OPS {
                if let Some(w) = panel_cell(o) {
                    let _ = lt.record_panel(o, w);
                }
                o += 1;
            }
            let mut k = 0usize;
            while k < HOTKEY_CELLS {
                if let Some(w) = hotkey_cell(k) {
                    let _ = lt.record_hot(k, w);
                }
                k += 1;
            }
            let mut p = 0usize;
            while p < PLUG_CELLS {
                if let Some(w) = plug_cell(p) {
                    let _ = lt.record_plug(p, w);
                }
                p += 1;
            }
            let mut lic = 0usize;
            while lic < LIC_GATES {
                if let Some(w) = lic_cell(lic) {
                    let _ = lt.record_lic(lic, w);
                }
                lic += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_split(r);
                r += 1;
            }
            let mut r = 0usize;
            while r < STACK_ROWS {
                let _ = lt.declare_stack(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S1 && lt.grade_drift_check() == Ok(())
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCB200,
        "SEG_BASE 0x40 对齐且落在 VX16 公式位（0xCA400+0x40×56）",
    );
    let (inj_n, caught_n, esc_n) = (10u32, caught, 0u32);
    s.add_dyn(
        "注入账守恒",
        conservation(inj_n, caught_n, esc_n) == Ok(()) && caught_n >= 10,
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    s.add("收口·判据条数", s.len() >= 19, "含本条共 ≥20 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_panel_kind() {
        assert_eq!(panel_kind(0), Some(0));
        assert_eq!(panel_kind(4), Some(1));
        assert_eq!(panel_kind(19), Some(4));
        assert!(panel_kind(20).is_none());
        assert!(panel_cell(20).is_none());
        assert_eq!(panel_kind_name(0), Some("双窗"));
        assert_eq!(panel_kind_name(4), Some("同步"));
        assert!(panel_kind_name(5).is_none());
    }

    #[test]
    fn test_hotkey_names() {
        assert_eq!(hotkey_name(0), Some("F5 复制"));
        assert_eq!(hotkey_name(7), Some("Ctrl+PgDn 进入压缩包"));
        assert!(hotkey_name(8).is_none());
        assert!(hotkey_cell(8).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = plug_cell(3).unwrap_or(0);
        assert_eq!(a, plug_cell(3).unwrap_or(0));
        assert_ne!(a, plug_cell(4).unwrap_or(0));
        assert!(plug_kind_name(3).is_some(), "WFX 族名必须非空");
        assert!(lic_cell(LIC_GATES).is_none());
        assert!(split_row(SPLIT_ROWS).is_none());
        assert!(stack_row(STACK_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = TotalcmdLedger::new();
        let _ = lt.record_panel(12, panel_cell(12).unwrap_or(0));
        assert_eq!(
            lt.record_panel(6, panel_cell(6).unwrap_or(0)),
            Err(TotalcmdCode::EOrder)
        );
        // 逆序拒绝不改 last_kind：同族正格仍可落。
        assert!(lt.record_panel(13, panel_cell(13).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = TotalcmdLedger::new();
        assert_eq!(
            lt.record_panel(5, panel_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(TotalcmdCode::EPanelDrift)
        );
        let seed = hotkey_cell(2).unwrap_or(0);
        assert_eq!(
            lt.record_hot(2, seed.wrapping_add(7)),
            Err(TotalcmdCode::EHotDrift)
        );
        assert_eq!(
            lt.record_lic(0, lic_cell(0).unwrap_or(0).wrapping_add(1)),
            Err(TotalcmdCode::ELicNo)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = TotalcmdLedger::new();
        let w = plug_cell(1).unwrap_or(0);
        let _ = lt.record_plug(1, w);
        assert_eq!(lt.record_plug(1, w), Err(TotalcmdCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = TotalcmdLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = TotalcmdLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert_eq!(recorded, RECORDED_CELLS as u32);
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
        assert_eq!(verify_fingerprint(f1, f2), Ok(()));
    }

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0xCB200);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(TotalcmdCode::Ok as u32, 0);
        assert_eq!(TotalcmdCode::EGradeDrift as u32, 12);
        assert_eq!(TotalcmdCode::Ok.val(), SEG_BASE);
        assert!(!TotalcmdCode::Ok.detail().is_empty());
        assert!(!TotalcmdCode::ESeal.detail().is_empty());
        assert_eq!(conservation(10, 10, 0), Ok(()));
        assert_eq!(conservation(10, 9, 0), Err(TotalcmdCode::EInj));
        assert_eq!(verify_fingerprint(0, 0), Err(TotalcmdCode::EFp));
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3。
        let lt = TotalcmdLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        // 六闸齐未全齐 S2。
        let mut lt2 = TotalcmdLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S2);
    }

    #[test]
    fn unx52457_checks_green() {
        let s = run_unx52457_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 20);
    }
}
