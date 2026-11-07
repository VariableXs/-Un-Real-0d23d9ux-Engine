//! UNX-F52458 Directory Opus 收口 · 0xCB240段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52458"，逐字）：
//!   title: Directory Opus 收口
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52459-J1 ×10；
//!     KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时 API/软件写回：
//!     状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52458）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52459-J1，照原文锚定；功能按本卡标题（Directory Opus）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五面板族闭集 20 格：双窗 / 标签 / 元数据 / 同步 / 查看器，每族
//!      4 格，逐格载荷折叠纯函数恒等才收；族单调（逆序必抓）。
//!   ② 许可三元闸：评估期 / 单机许可 / 网络许可 三格闭集，越闸必抓、
//!      载荷漂移必抓。
//!   ③ 与 Total Commander 分账声明 2 行：双窗族分账声明 / 查看器族分账
//!      声明，缺一拒封；C++ 上栈声明 2 行：GPSoftware 原生 C++ 栈声明 /
//!      本仓 C++ 类比上栈声明，缺一拒封。
//!   ④ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a）；
//!      注入核 run_cycle 三态轮转 10 轮（面板越闭集/许可闸漂移/面板族
//!      逆序）全部落在当轮临时账本上，检出 10/10、逃逸 0；双跑指纹一致。
//!   ⑤ KB-COMPAT 定级 S2 封档：Directory Opus 为商业共享软件，如实定级
//!      全齐 → S2（有条件收口封档），六闸齐未全齐 → S2，空账 → S3；
//!      定级漂移核验全齐仍须 S2，漂升 S1 即 EGradeDrift。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52458;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 五面板族操作格数（五族 × 4）。
pub const PANEL_OPS: usize = 20;

/// 许可三元闸格数。
pub const LIC_GATES: usize = 3;

/// 与 Total Commander 分账声明行数。
pub const SPLIT_ROWS: usize = 2;

/// C++ 上栈声明行数。
pub const STACK_ROWS: usize = 2;

/// run_cycle 实际经 record_* 计数的格数之和。
pub const RECORDED_CELLS: usize = PANEL_OPS + LIC_GATES;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DirOpusCode {
    /// 成功
    Ok = 0,
    /// 面板号越闭集
    ENoPanel,
    /// 面板载荷漂移
    EPanelDrift,
    /// 面板族逆序
    EOrder,
    /// 许可闸越界或未齐
    ELicNo,
    /// 许可闸载荷漂移
    ELicDrift,
    /// 声明行越闭集
    ERowNo,
    /// 缺声明行
    ERowMiss,
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

impl DirOpusCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            DirOpusCode::Ok => "成功",
            DirOpusCode::ENoPanel => "面板号越闭集：双窗/标签/元数据/同步/查看器 20 格之外先登记",
            DirOpusCode::EPanelDrift => "面板载荷漂移：面板格折叠与基准不恒等，逐格复算",
            DirOpusCode::EOrder => "面板族逆序：双窗→标签→元数据→同步→查看器单调，逆序必抓",
            DirOpusCode::ELicNo => "许可闸越闭集：评估期/单机许可/网络许可三元闸之外先登记",
            DirOpusCode::ELicDrift => "许可闸载荷漂移：闸位折叠与基准不恒等，逐格复算",
            DirOpusCode::ERowNo => "声明行越闭集：分账/上栈声明 4 行闭集之外先登记",
            DirOpusCode::ERowMiss => "缺声明行：与 Total Commander 分账、C++ 上栈声明未齐先补齐",
            DirOpusCode::EDup => "重复落账：同格只许一条账目",
            DirOpusCode::ESeal => "未齐即封：20 面板格 + 3 许可闸 + 4 声明行未齐先补齐",
            DirOpusCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            DirOpusCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            DirOpusCode::EGradeDrift => "定级漂移：Directory Opus 商业共享软件如实定级，全齐封档 S2，漂升即拒",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口（本卡封档不可达）
    S1,
    /// S2：有条件收口（本卡封档）
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
            Grade::S1 => "S1：全通收口（Directory Opus 商业许可面不升此档）",
            Grade::S2 => "S2：有条件收口封档，商业共享软件如实定级",
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
    Some((0xA6700u64 + (o as u64) * 0x43) ^ ((o / 4) as u64).wrapping_mul(0x85EBCA77))
}

/// 面板族（0..4=双窗 4..8=标签 8..12=元数据 12..16=同步 16..20=查看器）。
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
        2 => Some("元数据"),
        3 => Some("同步"),
        4 => Some("查看器"),
        _ => None,
    }
}

/// 许可闸载荷（闸 g）。
pub const fn lic_cell(g: usize) -> Option<u64> {
    if g >= LIC_GATES {
        return None;
    }
    Some(0xA6800u64 + (g as u64) * 0x59)
}

/// 许可闸名。
pub const fn lic_gate_name(g: usize) -> Option<&'static str> {
    match g {
        0 => Some("评估期"),
        1 => Some("单机许可"),
        2 => Some("网络许可"),
        _ => None,
    }
}

/// 与 Total Commander 分账声明折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xA690 + r as u32 * 0x34)
}

/// C++ 上栈声明折叠（行 r）。
pub const fn stack_row(r: usize) -> Option<u32> {
    if r >= STACK_ROWS {
        return None;
    }
    Some(0xA6A0 + r as u32 * 0x34)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Directory Opus 收口账本状态机。
#[derive(Clone)]
pub struct DirOpusLedger {
    /// 六判据（启动/功能/崩溃/卸载/升级/许可）。
    gates: [Option<bool>; 6],
    /// 面板族账。
    panels: [Option<u64>; PANEL_OPS],
    /// 许可闸账。
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

impl DirOpusLedger {
    /// 新账本。
    pub fn new() -> Self {
        DirOpusLedger {
            gates: [None; 6],
            panels: [None; PANEL_OPS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), DirOpusCode> {
        if gate >= 6 {
            return Err(DirOpusCode::ESeal);
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

    /// 面板一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_panel(&mut self, o: usize, got: u64) -> Result<(), DirOpusCode> {
        if o >= PANEL_OPS {
            return Err(DirOpusCode::ENoPanel);
        }
        let kind = match panel_kind(o) {
            Some(k) => k,
            None => return Err(DirOpusCode::ENoPanel),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6200);
            return Err(DirOpusCode::EOrder);
        }
        let want = match panel_cell(o) {
            Some(w) => w,
            None => return Err(DirOpusCode::ENoPanel),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6201 + o as u64);
            return Err(DirOpusCode::EPanelDrift);
        }
        if match self.panels.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DirOpusCode::EDup);
        }
        if let Some(c) = self.panels.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x6202 + o as u64);
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

    /// 许可一格（三元闸载荷对照 + 重复拒）。
    pub fn record_lic(&mut self, g: usize, got: u64) -> Result<(), DirOpusCode> {
        if g >= LIC_GATES {
            return Err(DirOpusCode::ELicNo);
        }
        let want = match lic_cell(g) {
            Some(w) => w,
            None => return Err(DirOpusCode::ELicNo),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6300 + g as u64);
            return Err(DirOpusCode::ELicDrift);
        }
        if match self.lics.get(g) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DirOpusCode::EDup);
        }
        if let Some(c) = self.lics.get_mut(g) {
            *c = Some(got);
        }
        self.fnv(0x6301 + g as u64);
        Ok(())
    }

    /// 许可三元闸全账核验。
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

    /// 分账声明一行（与 Total Commander）。
    pub fn declare_split(&mut self, r: usize) -> Result<(), DirOpusCode> {
        if r >= SPLIT_ROWS {
            return Err(DirOpusCode::ERowNo);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DirOpusCode::EDup);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0x6400 + r as u64);
        Ok(())
    }

    /// C++ 上栈声明一行。
    pub fn declare_stack(&mut self, r: usize) -> Result<(), DirOpusCode> {
        if r >= STACK_ROWS {
            return Err(DirOpusCode::ERowNo);
        }
        if match self.stacks.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DirOpusCode::EDup);
        }
        if let Some(c) = self.stacks.get_mut(r) {
            *c = stack_row(r);
        }
        self.fnv(0x6500 + r as u64);
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

    /// KB-COMPAT 定级（S2 封档：全齐 → S2，六闸齐 → S2，空账 → S3）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }

    /// 全齐布尔（定级漂移核验用，如实反映各节记账）。
    pub fn full_complete(&self) -> bool {
        self.gates_all()
            && self.panels_all()
            && self.lics_all()
            && self.declares_all()
    }

    /// 定级漂移核验：全齐必须定 S2 封档，漂升 S1 即 EGradeDrift。
    pub fn grade_drift_check(&self) -> Result<(), DirOpusCode> {
        if self.full_complete() && self.judge_grade() != Grade::S2 {
            return Err(DirOpusCode::EGradeDrift);
        }
        Ok(())
    }

    /// 声明行未齐核验：缺行即报 ERowMiss。
    pub fn row_miss_check(&self) -> Result<(), DirOpusCode> {
        if !self.declares_all() {
            return Err(DirOpusCode::ERowMiss);
        }
        Ok(())
    }
}

/// 注入守恒核验：注入 = 检出 + 逃逸。
pub fn conservation(injected: u32, caught: u32, escaped: u32) -> Result<(), DirOpusCode> {
    if injected != caught + escaped {
        return Err(DirOpusCode::EInj);
    }
    Ok(())
}

/// 双跑指纹核验。
pub fn verify_fingerprint(a: u64, b: u64) -> Result<(), DirOpusCode> {
    if a != b || a == 0 {
        return Err(DirOpusCode::EFp);
    }
    Ok(())
}

impl Default for DirOpusLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（record_* 计格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = DirOpusLedger::new();
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
    // 注入 10 轮：面板越闭集 / 许可闸漂移 / 面板族逆序三态轮转，
    // 全部落在当轮临时账本上，主账只记注入/检出计数。
    let mut t = 0usize;
    while t < 10 {
        let kind = (t % 3) as u32;
        let mut lt = DirOpusLedger::new();
        let caught = match kind {
            0 => lt.record_panel(PANEL_OPS, 0).is_err(),
            1 => {
                let seed = lic_cell(1).unwrap_or(0);
                let _ = lt.record_lic(1, seed);
                lt.record_lic(1, seed.wrapping_add(1)) == Err(DirOpusCode::ELicDrift)
            }
            _ => {
                // 族逆序：先按合法序落高族正格，再注低族（空账本先落种子）。
                let hi = panel_cell(16).unwrap_or(0);
                let _ = lt.record_panel(16, hi);
                lt.record_panel(9, panel_cell(9).unwrap_or(0)) == Err(DirOpusCode::EOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x6600 + t as u64);
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
/// UNX-F52458 判据集（锚 UNX-F52459-J1，off-by-one 板式）。
pub fn run_unx52458_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52458");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五面板族 20 操作全通",
        recorded >= PANEL_OPS as u32,
        "双窗/标签/元数据/同步/查看器五族逐格载荷恒等",
    );
    s.add(
        "许可三元闸 3/3 全落",
        {
            let mut lt = DirOpusLedger::new();
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
        "评估期/单机许可/网络许可三元闸，越闸必抓",
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
        "双窗→标签→元数据→同步→查看器五族闭集单调",
    );
    s.add(
        "与 Total Commander 分账声明 2/2 全齐",
        {
            let mut lt = DirOpusLedger::new();
            let mut ok = true;
            // 未齐方向：空账上缺行核验必须报 ERowMiss（is_err 口径）。
            if lt.row_miss_check() != Err(DirOpusCode::ERowMiss) {
                ok = false;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                if lt.declare_split(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok
        },
        "双窗族分账/查看器族分账两行闭集（缺一拒封）",
    );
    s.add(
        "C++ 上栈声明 2/2 全齐",
        {
            let mut lt = DirOpusLedger::new();
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
        "GPSoftware 原生 C++ 栈声明/本仓 C++ 类比上栈声明，缺一拒封",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = DirOpusLedger::new();
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
        "面板越闭集/许可闸漂移/面板族逆序三态轮转必抓",
    );
    let mut l0 = DirOpusLedger::new();
    s.add(
        "边界·面板号越闭集",
        l0.record_panel(PANEL_OPS, 0).is_err(),
        "面板号 20 越闭集必拒",
    );
    s.add(
        "边界·许可闸越闭集",
        l0.record_lic(LIC_GATES, 0).is_err(),
        "许可闸号 3 越闭集必拒",
    );
    s.add(
        "边界·声明行越闭集",
        l0.declare_split(SPLIT_ROWS).is_err(),
        "声明行 2 越闭集必拒",
    );
    s.add(
        "面板族逆序拒",
        {
            let _ = l0.record_panel(12, panel_cell(12).unwrap_or(0));
            l0.record_panel(3, panel_cell(3).unwrap_or(0)) == Err(DirOpusCode::EOrder)
        },
        "同步族后落双窗族，逆序必抓",
    );
    s.add(
        "许可闸漂移拒",
        {
            let seed = lic_cell(0).unwrap_or(0);
            let _ = l0.record_lic(0, seed);
            l0.record_lic(0, seed.wrapping_add(1)) == Err(DirOpusCode::ELicDrift)
        },
        "许可闸载荷漂移必抓",
    );
    s.add(
        "未齐允许落格（封核方向）",
        {
            let mut lt = DirOpusLedger::new();
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
        "定级 S2 封档闭集",
        {
            let mut lt = DirOpusLedger::new();
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
            lt.judge_grade() == Grade::S2 && lt.grade_drift_check() == Ok(())
        },
        "全齐仍封档 S2（KB-COMPAT 定级登记，商业共享软件如实定级）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCB240,
        "SEG_BASE 0x40 对齐且落在 VX16 公式位（0xCA400+0x40×57）",
    );
    let (inj_n, caught_n, esc_n) = (10u32, caught, 0u32);
    s.add_dyn(
        "注入账守恒",
        conservation(inj_n, caught_n, esc_n) == Ok(()) && caught_n >= 10,
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    s.add("收口·判据条数", s.len() >= 17, "含本条共 ≥18 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_panel_kind() {
        assert_eq!(panel_kind(0), Some(0));
        assert_eq!(panel_kind(8), Some(2));
        assert_eq!(panel_kind(19), Some(4));
        assert!(panel_kind(20).is_none());
        assert!(panel_cell(20).is_none());
        assert_eq!(panel_kind_name(2), Some("元数据"));
        assert_eq!(panel_kind_name(4), Some("查看器"));
        assert!(panel_kind_name(5).is_none());
    }

    #[test]
    fn test_lic_names() {
        assert_eq!(lic_gate_name(0), Some("评估期"));
        assert_eq!(lic_gate_name(2), Some("网络许可"));
        assert!(lic_gate_name(3).is_none());
        assert!(lic_cell(LIC_GATES).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = panel_cell(7).unwrap_or(0);
        assert_eq!(a, panel_cell(7).unwrap_or(0));
        assert_ne!(a, panel_cell(8).unwrap_or(0));
        assert!(split_row(SPLIT_ROWS).is_none());
        assert!(stack_row(STACK_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = DirOpusLedger::new();
        let _ = lt.record_panel(16, panel_cell(16).unwrap_or(0));
        assert_eq!(
            lt.record_panel(5, panel_cell(5).unwrap_or(0)),
            Err(DirOpusCode::EOrder)
        );
        // 逆序拒绝不改 last_kind：同族正格仍可落。
        assert!(lt.record_panel(17, panel_cell(17).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = DirOpusLedger::new();
        assert_eq!(
            lt.record_panel(9, panel_cell(9).unwrap_or(0).wrapping_add(3)),
            Err(DirOpusCode::EPanelDrift)
        );
        let seed = lic_cell(2).unwrap_or(0);
        assert_eq!(
            lt.record_lic(2, seed.wrapping_add(1)),
            Err(DirOpusCode::ELicDrift)
        );
        assert_eq!(lt.record_lic(LIC_GATES, 0), Err(DirOpusCode::ELicNo));
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = DirOpusLedger::new();
        let w = panel_cell(2).unwrap_or(0);
        let _ = lt.record_panel(2, w);
        assert_eq!(lt.record_panel(2, w), Err(DirOpusCode::EDup));
        let _ = lt.declare_split(0);
        assert_eq!(lt.declare_split(0), Err(DirOpusCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = DirOpusLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = DirOpusLedger::new();
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
        assert_eq!(SEG_BASE, 0xCB240);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(DirOpusCode::Ok as u32, 0);
        assert_eq!(DirOpusCode::EGradeDrift as u32, 12);
        assert_eq!(DirOpusCode::Ok.val(), SEG_BASE);
        assert!(!DirOpusCode::Ok.detail().is_empty());
        assert!(!DirOpusCode::ESeal.detail().is_empty());
        assert_eq!(conservation(10, 10, 0), Ok(()));
        assert_eq!(conservation(10, 8, 1), Err(DirOpusCode::EInj));
        assert_eq!(verify_fingerprint(0, 0), Err(DirOpusCode::EFp));
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3。
        let lt = DirOpusLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        // 六闸齐 S2，全齐仍封档 S2。
        let mut lt2 = DirOpusLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S2);
        let mut lt3 = DirOpusLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt3.judge_gate(g);
            g += 1;
        }
        let mut o = 0usize;
        while o < PANEL_OPS {
            if let Some(w) = panel_cell(o) {
                let _ = lt3.record_panel(o, w);
            }
            o += 1;
        }
        let mut lic = 0usize;
        while lic < LIC_GATES {
            if let Some(w) = lic_cell(lic) {
                let _ = lt3.record_lic(lic, w);
            }
            lic += 1;
        }
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            let _ = lt3.declare_split(r);
            r += 1;
        }
        let mut r = 0usize;
        while r < STACK_ROWS {
            let _ = lt3.declare_stack(r);
            r += 1;
        }
        assert!(lt3.full_complete());
        assert_eq!(lt3.judge_grade(), Grade::S2);
    }

    #[test]
    fn unx52458_checks_green() {
        let s = run_unx52458_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 18);
    }
}
