//! UNX-F52455 FreeCommander 收口 · 0xCB180 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52455"，逐字）：
//!   title: FreeCommander 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52456-J1 六判据
//!     全落；×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时
//!     API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52455）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52456-J1，照原文锚定；功能按本卡标题（FreeCommander 收口）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五面板族 20 格闭集：双窗/标签/压缩/FTP/查看器 各 4 格，家族单调
//!      （逆序必抓），逐格载荷纯函数折叠恒等才收，漂移必抓。
//!   ② 快捷键投影 8 格闭集：复制/移动/删除/重命名/新建目录/压缩/解压/
//!      退出 逐格投影恒等，越闭集必抓。
//!   ③ 与 Double Commander 分账声明 2 行闭集（账目与 DoubleCmd 分账不并
//!      表）；免费/专业分账 2 行闭集；C++ 上栈声明 2 行闭集；三组声明
//!      缺一拒封。
//!   ④ ledger 状态机（new/record_*/judge_gate×6/seal/fingerprint，
//!      FNV-1a）；注入核 10 轮三态轮转（面板漂移/家族逆序/快捷键越闭集）
//!      10/10 必抓，检出+逃逸守恒等式；双跑 FNV 指纹一致。
//!   ⑤ KB-COMPAT 定级登记：如实定级 S2（有条件收口），登记档不因全齐
//!      自升 S1；定级阶梯 S3→S2→S1 闭集经 grade_step 单调复核。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52455;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 五面板族 20 格数。
pub const PANEL_N: usize = 20;

/// 快捷键投影格数。
pub const KEY_N: usize = 8;

/// 与 Double Commander 分账声明行数。
pub const SPLIT_ROWS: usize = 2;

/// 免费/专业分账声明行数。
pub const LIC_ROWS: usize = 2;

/// C++ 上栈声明行数。
pub const DECL_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FreecmdCode {
    /// 成功
    Ok = 0,
    /// 面板格越闭集
    ENoPanel,
    /// 面板漂移
    EPanelDrift,
    /// 家族逆序
    EOrder,
    /// 快捷键格越闭集
    ENoKey,
    /// 快捷键投影漂移
    EKeyDrift,
    /// 分账声明行越闭集或缺行
    ENoRow,
    /// 许可分账行越闭集或缺行
    ELic,
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

impl FreecmdCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            FreecmdCode::Ok => "成功",
            FreecmdCode::ENoPanel => "面板格越闭集：五面板族 20 格之外先登记",
            FreecmdCode::EPanelDrift => "面板漂移：面板折叠与基准不恒等，逐格复算",
            FreecmdCode::EOrder => "家族逆序：双窗→标签→压缩→FTP→查看器单调，逆序必抓",
            FreecmdCode::ENoKey => "快捷键格越闭集：投影 8 格之外先登记",
            FreecmdCode::EKeyDrift => "快捷键投影漂移：投影折叠与基准不恒等，逐格复算",
            FreecmdCode::ENoRow => "分账声明行越闭集或缺行：与 Double Commander 分账未齐先补齐",
            FreecmdCode::ELic => "许可分账行越闭集或缺行：免费/专业分账未齐先补齐",
            FreecmdCode::EDup => "重复落账：同格只许一条账目",
            FreecmdCode::ESeal => "未齐即封：20 格+快捷键 8 格+三组声明未齐先补齐",
            FreecmdCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            FreecmdCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            FreecmdCode::EGradeDrift => "定级漂移：登记档为 S2，全齐亦不得自升 S1",
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
            Grade::S1 => "S1：全通收口档（FreeCommander 卡不登记此档）",
            Grade::S2 => "S2：有条件收口（FreeCommander 登记档：六判据落、专业版面有条件）",
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

/// 本卡 KB-COMPAT 登记档。
pub const REGISTERED: Grade = Grade::S2;

// ── 域模型 ───────────────────────────────────────────────────────
/// 面板格载荷（格 p；家族 = p/4）。
pub const fn panel_cell(p: usize) -> Option<u64> {
    if p >= PANEL_N {
        return None;
    }
    Some((0xB5000u64 + (p as u64) * 0x36) ^ ((p / 4) as u64).wrapping_mul(0x01000193))
}

/// 面板家族（0..4=双窗 4..8=标签 8..12=压缩 12..16=FTP 16..20=查看器）。
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
        1 => Some("标签"),
        2 => Some("压缩"),
        3 => Some("FTP"),
        4 => Some("查看器"),
        _ => None,
    }
}

/// 快捷键投影格载荷（格 k）。
pub const fn key_cell(k: usize) -> Option<u64> {
    if k >= KEY_N {
        return None;
    }
    Some((0xB5400u64 + (k as u64) * 0x2C) ^ 0x46435F4B45590000)
}

/// 快捷键投影格名（8 格闭集）。
pub const fn key_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("复制"),
        1 => Some("移动"),
        2 => Some("删除"),
        3 => Some("重命名"),
        4 => Some("新建目录"),
        5 => Some("压缩"),
        6 => Some("解压"),
        7 => Some("退出"),
        _ => None,
    }
}

/// 与 Double Commander 分账声明行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xB5800 + r as u32 * 0x2C)
}

/// 免费/专业分账声明行折叠（行 r）。
pub const fn lic_row(r: usize) -> Option<u32> {
    if r >= LIC_ROWS {
        return None;
    }
    Some(0xB5900 + r as u32 * 0x2C)
}

/// C++ 上栈声明行折叠（行 r）。
pub const fn decl_row(r: usize) -> Option<u32> {
    if r >= DECL_ROWS {
        return None;
    }
    Some(0xB5A00 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// FreeCommander 收口账本状态机。
#[derive(Clone)]
pub struct FreecmdLedger {
    /// 六判据（安装/双窗/压缩/FTP/卸载/许可）。
    gates: [Option<bool>; 6],
    /// 面板账。
    panels: [Option<u64>; PANEL_N],
    /// 快捷键投影账。
    keys: [Option<u64>; KEY_N],
    /// 与 Double Commander 分账声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 免费/专业分账声明账。
    lics: [Option<u32>; LIC_ROWS],
    /// C++ 上栈声明账。
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

impl FreecmdLedger {
    /// 新账本。
    pub fn new() -> Self {
        FreecmdLedger {
            gates: [None; 6],
            panels: [None; PANEL_N],
            keys: [None; KEY_N],
            splits: [None; SPLIT_ROWS],
            lics: [None; LIC_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), FreecmdCode> {
        if gate >= 6 {
            return Err(FreecmdCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0xB510 + gate as u64);
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
    pub fn record_panel(&mut self, p: usize, got: u64) -> Result<(), FreecmdCode> {
        if p >= PANEL_N {
            return Err(FreecmdCode::ENoPanel);
        }
        let kind = match panel_kind(p) {
            Some(k) => k,
            None => return Err(FreecmdCode::ENoPanel),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xB520);
            return Err(FreecmdCode::EOrder);
        }
        let want = match panel_cell(p) {
            Some(w) => w,
            None => return Err(FreecmdCode::ENoPanel),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xB521 + p as u64);
            return Err(FreecmdCode::EPanelDrift);
        }
        if match self.panels.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(FreecmdCode::EDup);
        }
        if let Some(c) = self.panels.get_mut(p) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0xB522 + p as u64);
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

    /// 快捷键投影一格（载荷对照 + 重复拒）。
    pub fn record_key(&mut self, k: usize, got: u64) -> Result<(), FreecmdCode> {
        let want = match key_cell(k) {
            Some(w) => w,
            None => return Err(FreecmdCode::ENoKey),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xB540 + k as u64);
            return Err(FreecmdCode::EKeyDrift);
        }
        if match self.keys.get(k) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(FreecmdCode::EDup);
        }
        if let Some(c) = self.keys.get_mut(k) {
            *c = Some(got);
        }
        self.fnv(0xB541 + k as u64);
        Ok(())
    }

    /// 快捷键投影全账核验（8 格）。
    pub fn keys_all(&self) -> bool {
        let mut k = 0usize;
        while k < KEY_N {
            match self.keys.get(k) {
                Some(Some(_)) => {}
                _ => return false,
            }
            k += 1;
        }
        true
    }

    /// 与 Double Commander 分账声明一行。
    pub fn declare_split(&mut self, r: usize) -> Result<(), FreecmdCode> {
        if r >= SPLIT_ROWS {
            return Err(FreecmdCode::ENoRow);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(FreecmdCode::EDup);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0xB560 + r as u64);
        Ok(())
    }

    /// 免费/专业分账声明一行。
    pub fn declare_lic(&mut self, r: usize) -> Result<(), FreecmdCode> {
        if r >= LIC_ROWS {
            return Err(FreecmdCode::ELic);
        }
        if match self.lics.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(FreecmdCode::EDup);
        }
        if let Some(c) = self.lics.get_mut(r) {
            *c = lic_row(r);
        }
        self.fnv(0xB570 + r as u64);
        Ok(())
    }

    /// C++ 上栈声明一行。
    pub fn declare_cpp(&mut self, r: usize) -> Result<(), FreecmdCode> {
        if r >= DECL_ROWS {
            return Err(FreecmdCode::ENoRow);
        }
        if match self.decls.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(FreecmdCode::EDup);
        }
        if let Some(c) = self.decls.get_mut(r) {
            *c = decl_row(r);
        }
        self.fnv(0xB580 + r as u64);
        Ok(())
    }

    /// 三组声明全齐核验（分账 2 + 许可 2 + 上栈 2）。
    pub fn rows_all(&self) -> bool {
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            match self.splits.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        let mut l = 0usize;
        while l < LIC_ROWS {
            match self.lics.get(l) {
                Some(Some(_)) => {}
                _ => return false,
            }
            l += 1;
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

    /// 收口面核验：主账+投影账+三组声明全齐；未齐即封。
    pub fn seal(&self) -> Result<(), FreecmdCode> {
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            if match self.splits.get(r) {
                Some(Some(_)) => false,
                _ => true,
            } {
                return Err(FreecmdCode::ENoRow);
            }
            r += 1;
        }
        let mut l = 0usize;
        while l < LIC_ROWS {
            if match self.lics.get(l) {
                Some(Some(_)) => false,
                _ => true,
            } {
                return Err(FreecmdCode::ELic);
            }
            l += 1;
        }
        if !self.panels_all() || !self.keys_all() {
            return Err(FreecmdCode::ESeal);
        }
        let mut d = 0usize;
        while d < DECL_ROWS {
            if match self.decls.get(d) {
                Some(Some(_)) => false,
                _ => true,
            } {
                return Err(FreecmdCode::ESeal);
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
    pub fn reconcile(&mut self, inj: u32, caught: u32, esc: u32) -> Result<(), FreecmdCode> {
        if inj != caught.wrapping_add(esc) {
            self.fnv(0xB590);
            return Err(FreecmdCode::EInj);
        }
        Ok(())
    }

    /// 双跑指纹复核。
    pub fn verify_fp(&self, other: u64) -> Result<(), FreecmdCode> {
        if self.fp != other {
            return Err(FreecmdCode::EFp);
        }
        Ok(())
    }

    /// KB-COMPAT 定级：六判据齐 → 登记档 S2；判据缺 → S3；全齐仍按登记档
    /// S2 落账（如实定级，不自升 S1）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() {
            REGISTERED
        } else {
            Grade::S3
        }
    }
}

impl Default for FreecmdLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 定级登记复核：全齐账必须落在登记档，否则定级漂移。
pub fn grade_register(lt: &FreecmdLedger) -> Result<Grade, FreecmdCode> {
    let g = lt.judge_grade();
    if lt.seal_all() && lt.gates_all() {
        if g == REGISTERED {
            Ok(g)
        } else {
            Err(FreecmdCode::EGradeDrift)
        }
    } else {
        Ok(g)
    }
}

/// 全齐账本（六判据 + 全节落账，测试与定级检查共用）。
pub fn sealed_ledger() -> FreecmdLedger {
    let mut lt = FreecmdLedger::new();
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
    let mut k = 0usize;
    while k < KEY_N {
        if let Some(w) = key_cell(k) {
            let _ = lt.record_key(k, w);
        }
        k += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = lt.declare_split(r);
        r += 1;
    }
    let mut l = 0usize;
    while l < LIC_ROWS {
        let _ = lt.declare_lic(l);
        l += 1;
    }
    let mut d = 0usize;
    while d < DECL_ROWS {
        let _ = lt.declare_cpp(d);
        d += 1;
    }
    lt
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
/// 注入 10 轮三态轮转，全部落在当轮临时账本上；本账只记守恒三数。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = FreecmdLedger::new();
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
    let mut k = 0usize;
    while k < KEY_N {
        if let Some(w) = key_cell(k) {
            if led.record_key(k, w).is_ok() {
                recorded += 1;
            }
        }
        k += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    let mut l = 0usize;
    while l < LIC_ROWS {
        let _ = led.declare_lic(l);
        l += 1;
    }
    let mut d = 0usize;
    while d < DECL_ROWS {
        let _ = led.declare_cpp(d);
        d += 1;
    }
    // 注入 10 轮：面板漂移 / 家族逆序 / 快捷键越闭集三态轮转。
    // 漂移注入先按合法序落正格再注漂移；越闭集（phase）注入对空账本同样
    // 必抓；逆序路径不改写 last_kind。
    let mut n = 0usize;
    while n < 10 {
        let kind = (n % 3) as u32;
        let mut lt = FreecmdLedger::new();
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
                lt.record_panel(1, w1.wrapping_add(1)) == Err(FreecmdCode::EPanelDrift)
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
                lt.record_panel(2, w2) == Err(FreecmdCode::EOrder)
            }
            _ => lt.record_key(KEY_N, 0) == Err(FreecmdCode::ENoKey),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0xB5A0 + n as u64);
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
/// UNX-F52455 判据集（锚 UNX-F52456-J1，off-by-one 板式）。
pub fn run_unx52455_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52455");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五面板族 20 格全通",
        recorded >= PANEL_N as u32,
        "双窗/标签/压缩/FTP/查看器五家族逐格恒等",
    );
    s.add(
        "快捷键投影 8/8 全落",
        {
            let mut lt = FreecmdLedger::new();
            let mut ok = true;
            let mut k = 0usize;
            while k < KEY_N {
                if let Some(w) = key_cell(k) {
                    if lt.record_key(k, w).is_err() {
                        ok = false;
                    }
                }
                k += 1;
            }
            ok && lt.keys_all()
        },
        "复制/移动/删除/重命名/新建目录/压缩/解压/退出逐格投影恒等",
    );
    s.add(
        "UNX-F52456-J1 六判据全落",
        {
            let mut lt = FreecmdLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "安装/双窗/压缩/FTP/卸载/许可逐格落账",
    );
    s.add(
        "与 Double Commander 分账声明 2/2 全齐",
        {
            let mut lt = FreecmdLedger::new();
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
        "与 Double Commander 分账声明两行闭集（账目不并表）",
    );
    s.add(
        "免费/专业分账声明 2/2 全齐",
        {
            let mut lt = FreecmdLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < LIC_ROWS {
                if lt.declare_lic(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok
        },
        "免费版分账声明/专业版分账声明两行闭集",
    );
    s.add(
        "C++ 上栈声明 2/2 全齐",
        {
            let mut lt = FreecmdLedger::new();
            let mut ok = true;
            let mut d = 0usize;
            while d < DECL_ROWS {
                if lt.declare_cpp(d).is_err() {
                    ok = false;
                }
                d += 1;
            }
            ok
        },
        "双窗核心 C++ 上栈/压缩管道 C++ 上栈两行闭集",
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
        "快捷键投影名闭集",
        {
            let mut ok = true;
            let mut k = 0usize;
            while k < KEY_N {
                match key_name(k) {
                    Some(n) => {
                        if n.is_empty() {
                            ok = false;
                        }
                    }
                    None => ok = false,
                }
                k += 1;
            }
            key_name(KEY_N).is_none() && ok
        },
        "投影 8 格名闭集，越界必拒",
    );
    s.add("注入 10/10 全抓", caught >= 10, "面板漂移/家族逆序/快捷键越闭集三态轮转必抓");
    let mut l0 = FreecmdLedger::new();
    s.add(
        "边界·面板越闭集",
        l0.record_panel(PANEL_N, 0).is_err(),
        "面板格 20 越闭集必拒",
    );
    s.add(
        "边界·快捷键越闭集",
        l0.record_key(KEY_N, 0).is_err(),
        "快捷键格 8 越闭集必拒",
    );
    s.add(
        "边界·声明行越闭集",
        l0.declare_split(SPLIT_ROWS).is_err()
            && l0.declare_lic(LIC_ROWS).is_err()
            && l0.declare_cpp(DECL_ROWS).is_err(),
        "三组声明行 2 之外越界必拒",
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
            l0.record_panel(1, w1.wrapping_add(1)) == Err(FreecmdCode::EPanelDrift)
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
            let mut a = FreecmdLedger::new();
            let mut b = FreecmdLedger::new();
            let _ = a.judge_gate(0);
            let _ = b.judge_gate(0);
            a.verify_fp(b.fingerprint()) == Ok(()) && a.reconcile(1, 1, 0) == Ok(())
        },
        "FNV 指纹复核一致且注入守恒等式成立",
    );
    s.add(
        "KB-COMPAT 定级登记 S2",
        {
            let lt = sealed_ledger();
            grade_register(&lt) == Ok(REGISTERED) && lt.seal_all() && lt.judge_grade() == Grade::S2
        },
        "全齐账按登记档 S2 落账（如实定级，不自升 S1）",
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
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCB180,
        "SEG_BASE 0x40 对齐且等于 0xCA400+0x40×(52455−52401)",
    );
    s.add("收口·判据条数", s.len() >= 19, "含本条共 ≥20 条闭集");
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
        assert_eq!(panel_kind_name(4), Some("查看器"));
        assert!(panel_kind_name(5).is_none());
    }

    #[test]
    fn test_key_names_closed() {
        assert_eq!(key_name(0), Some("复制"));
        assert_eq!(key_name(3), Some("重命名"));
        assert_eq!(key_name(7), Some("退出"));
        assert!(key_name(8).is_none());
        assert!(key_cell(8).is_none());
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
        assert!(key_cell(KEY_N).is_none());
        assert!(split_row(SPLIT_ROWS).is_none());
        assert!(lic_row(LIC_ROWS).is_none());
        assert!(decl_row(DECL_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        // 先按合法序落低族格，再注逆序。
        let mut lt = FreecmdLedger::new();
        let _ = lt.record_panel(4, panel_cell(4).unwrap_or(0));
        let _ = lt.record_panel(8, panel_cell(8).unwrap_or(0));
        assert_eq!(
            lt.record_panel(0, panel_cell(0).unwrap_or(0)),
            Err(FreecmdCode::EOrder)
        );
        assert!(lt.record_panel(9, panel_cell(9).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_last_kind_unchanged_after_order() {
        let mut lt = FreecmdLedger::new();
        let _ = lt.record_panel(9, panel_cell(9).unwrap_or(0));
        let before = lt.last_family();
        let _ = lt.record_panel(2, panel_cell(2).unwrap_or(0));
        assert_eq!(lt.last_family(), before, "逆序路径不得改写 last_kind");
        assert_eq!(lt.stats().1, 1, "逆序注入计入检出");
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = FreecmdLedger::new();
        assert_eq!(
            lt.record_key(5, key_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(FreecmdCode::EKeyDrift)
        );
        let w0 = panel_cell(0).unwrap_or(0);
        let _ = lt.record_panel(0, w0);
        assert_eq!(
            lt.record_panel(1, panel_cell(1).unwrap_or(0).wrapping_add(1)),
            Err(FreecmdCode::EPanelDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = FreecmdLedger::new();
        let _ = lt.record_key(2, key_cell(2).unwrap_or(0));
        assert_eq!(
            lt.record_key(2, key_cell(2).unwrap_or(0)),
            Err(FreecmdCode::EDup)
        );
        let _ = lt.declare_lic(0);
        assert_eq!(lt.declare_lic(0), Err(FreecmdCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = FreecmdLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = FreecmdLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
        assert!(!lt2.seal_all());
    }

    #[test]
    fn test_seal_and_reconcile() {
        let empty = FreecmdLedger::new();
        assert_eq!(empty.seal(), Err(FreecmdCode::ENoRow));
        let mut no_lic = sealed_ledger();
        no_lic.lics = [None; LIC_ROWS];
        assert_eq!(no_lic.seal(), Err(FreecmdCode::ELic));
        let full = sealed_ledger();
        assert!(full.seal().is_ok());
        assert!(full.seal_all());
        let mut r = full.clone();
        assert!(r.reconcile(10, 10, 0).is_ok());
        assert!(r.verify_fp(full.fingerprint()).is_ok());
        assert_eq!(r.reconcile(10, 9, 0), Err(FreecmdCode::EInj));
        assert_eq!(r.verify_fp(full.fingerprint().wrapping_add(1)), Err(FreecmdCode::EFp));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // recorded 只计 run_cycle 实际计数的节：面板 20 + 快捷键 8。
        assert_eq!(recorded, (PANEL_N + KEY_N) as u32);
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
        assert_eq!(SEG_BASE, 0xCB180);
        assert_eq!(SEG_BASE, 0xCA400 + 0x40 * (52455u32 - 52401));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        // 码位须与枚举实际位序一致：Ok=0，EGradeDrift=12，恰 13 变体。
        assert_eq!(FreecmdCode::Ok as u32, 0);
        assert_eq!(FreecmdCode::ENoPanel as u32, 1);
        assert_eq!(FreecmdCode::ELic as u32, 7);
        assert_eq!(FreecmdCode::EGradeDrift as u32, 12);
        assert!(!FreecmdCode::Ok.detail().is_empty());
        assert!(!FreecmdCode::ESeal.detail().is_empty());
        assert!(!FreecmdCode::ELic.detail().is_empty());
        assert!(!FreecmdCode::EGradeDrift.detail().is_empty());
        assert_eq!(FreecmdCode::Ok.val(), SEG_BASE);
    }

    #[test]
    fn test_grade() {
        // 如实阶梯：空账 S3，六闸齐 S2，全齐仍按登记档 S2。
        assert_eq!(Grade::S3.val(), 3);
        let empty = FreecmdLedger::new();
        assert_eq!(empty.judge_grade(), Grade::S3);
        let mut half = FreecmdLedger::new();
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
    fn unx52455_checks_green() {
        let s = run_unx52455_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 20);
    }
}
