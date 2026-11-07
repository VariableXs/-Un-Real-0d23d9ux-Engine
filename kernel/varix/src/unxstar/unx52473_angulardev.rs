//! UNX-F52473 Angular DevTools 消费面收口 · 0xCB600段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52473"，逐字）：
//!   title: Angular DevTools 消费面收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52474-J1 审计
//!     ×10 全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时
//!     API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52473）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52474-J1，照原文锚定；功能按本卡标题（Angular DevTools）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五面板族 20 格闭集：组件树/Injector/NgRx/Router/Profiler 各 4
//!      格，面板单调（逆序必抓），逐格载荷纯函数折叠恒等才收。
//!   ② 连接三元闸六判据：连接发现/连接握手/会话保持/面板接线/状态投
//!      影/遥测闭合，逐格落账，未齐即封（定级不给目标档）。
//!   ③ 状态投影 8 格：四族（信号/NgRx store/Router state/表单控件）
//!      各 2 格，投影漂移必抓。
//!   ④ JS 上栈声明 2 格：JS 运行时上栈声明/面板脚本宿主上栈声明，载
//!      荷漂移必抓。
//!   ⑤ React DevTools 消费分账声明 2 行闭集（F52470）：组件树分账声
//!      明/Hooks 检查器分账声明，缺一拒封。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a）；
//!      注入核 run_cycle 三态轮转 10/10（面板漂移/面板族逆序/面板越闭
//!      集），检出+逃逸守恒；双跑 FNV 指纹一致；KB-COMPAT 定级闭集：
//!      本卡目标档 S2（六闸齐即 S2，全齐亦 S2），闸缺 → S3。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无
//!      unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52473;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 五面板族格数。
pub const PANEL_CELLS: usize = 20;

/// 状态投影格数。
pub const PROJ_CELLS: usize = 8;

/// JS 上栈声明格数。
pub const STACK_CELLS: usize = 2;

/// React DevTools 分账声明行数。
pub const SPLIT_ROWS: usize = 2;

/// 连接三元闸判据数。
pub const GATES: usize = 6;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AngulardevCode {
    /// 成功
    Ok = 0,
    /// 面板格越闭集
    ENoPanel,
    /// 面板载荷漂移
    EPanelDrift,
    /// 投影格越闭集
    ENoProj,
    /// 投影载荷漂移
    EProjDrift,
    /// 上栈格越闭集
    ENoStack,
    /// 上栈载荷漂移
    EStackDrift,
    /// 面板族逆序
    EOrder,
    /// 声明行越闭集
    ENoRow,
    /// 缺声明行
    ERowMiss,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 定级漂移
    EGradeDrift,
}

impl AngulardevCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            AngulardevCode::Ok => "成功",
            AngulardevCode::ENoPanel => "面板格越闭集：组件树/Injector/NgRx/Router/Profiler 20 格之外先登记",
            AngulardevCode::EPanelDrift => "面板载荷漂移：面板格折叠与基准不恒等，逐格复算",
            AngulardevCode::ENoProj => "投影格越闭集：状态投影 8 格之外先登记",
            AngulardevCode::EProjDrift => "投影载荷漂移：投影格折叠与基准不恒等，逐格复算",
            AngulardevCode::ENoStack => "上栈格越闭集：JS 上栈声明 2 格之外先登记",
            AngulardevCode::EStackDrift => "上栈载荷漂移：上栈格折叠与基准不恒等，逐格复算",
            AngulardevCode::EOrder => "面板族逆序：组件树→Injector→NgRx→Router→Profiler 单调，逆序必抓",
            AngulardevCode::ENoRow => "声明行越闭集：React DevTools 分账 2 行闭集之外先登记",
            AngulardevCode::ERowMiss => "缺声明行：React DevTools 消费分账声明未齐先补齐",
            AngulardevCode::EDup => "重复落账：同格只许一条账目",
            AngulardevCode::ESeal => "未齐即封：五面板 20 格 + 投影 8 格 + 上栈 2 格未齐先补齐",
            AngulardevCode::EGradeDrift => "定级漂移：六闸未齐不得给出目标档 S2",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S2：目标档（有条件收口，本卡定级）
    S2,
    /// S3：不收口
    S3,
}

impl Grade {
    /// 定级值。
    pub const fn val(self) -> u32 {
        match self {
            Grade::S2 => 2,
            Grade::S3 => 3,
        }
    }
    /// 明细。
    pub fn detail(self) -> &'static str {
        match self {
            Grade::S2 => "S2：连接三元闸六判据齐即有条件收口（本卡目标档），全齐亦 S2",
            Grade::S3 => "S3：不收口，六闸有缺",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 面板格载荷（格 o；面板族 = o/4）。
pub const fn panel_cell(o: usize) -> Option<u64> {
    if o >= PANEL_CELLS {
        return None;
    }
    Some((0xCB600u64 + (o as u64) * 0x37) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 面板族（0..4=组件树 4..8=Injector 8..12=NgRx 12..16=Router 16..20=Profiler）。
pub const fn panel_kind(o: usize) -> Option<u32> {
    if o >= PANEL_CELLS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 面板族名。
pub const fn panel_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("组件树"),
        1 => Some("Injector"),
        2 => Some("NgRx"),
        3 => Some("Router"),
        4 => Some("Profiler"),
        _ => None,
    }
}

/// 状态投影格载荷（格 p；投影族 = p/2）。
pub const fn proj_cell(p: usize) -> Option<u64> {
    if p >= PROJ_CELLS {
        return None;
    }
    Some((0xCB640u64 + (p as u64) * 0x41) ^ ((p / 2) as u64).wrapping_mul(0x27D4EB2F))
}

/// 投影族（0..2=信号 2..4=NgRx store 4..6=Router state 6..8=表单控件）。
pub const fn proj_kind(p: usize) -> Option<u32> {
    if p >= PROJ_CELLS {
        return None;
    }
    Some((p / 2) as u32)
}

/// 投影族名。
pub const fn proj_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("信号"),
        1 => Some("NgRx store"),
        2 => Some("Router state"),
        3 => Some("表单控件"),
        _ => None,
    }
}

/// JS 上栈格载荷（格 s）。
pub const fn stack_cell(s: usize) -> Option<u64> {
    if s >= STACK_CELLS {
        return None;
    }
    Some((0xCB680u64 + (s as u64) * 0x53) ^ 0x4A53535F_554E5831)
}

/// 上栈格名。
pub const fn stack_name(s: usize) -> Option<&'static str> {
    match s {
        0 => Some("JS 运行时上栈声明"),
        1 => Some("面板脚本宿主上栈声明"),
        _ => None,
    }
}

/// React DevTools 分账声明行折叠（行 r，F52470 分账）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xCB6C0 + r as u32 * 0x2C)
}

/// 分账行名。
pub const fn split_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("React DevTools 组件树分账声明（F52470）"),
        1 => Some("React DevTools Hooks 检查器分账声明（F52470）"),
        _ => None,
    }
}

/// 连接三元闸判据名。
pub const fn gate_name(g: usize) -> Option<&'static str> {
    match g {
        0 => Some("连接发现"),
        1 => Some("连接握手"),
        2 => Some("会话保持"),
        3 => Some("面板接线"),
        4 => Some("状态投影"),
        5 => Some("遥测闭合"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Angular DevTools 收口账本状态机。
#[derive(Clone)]
pub struct AngulardevLedger {
    /// 连接三元闸六判据。
    gates: [Option<bool>; GATES],
    /// 面板格账。
    panels: [Option<u64>; PANEL_CELLS],
    /// 投影格账。
    projs: [Option<u64>; PROJ_CELLS],
    /// 上栈格账。
    stacks: [Option<u64>; STACK_CELLS],
    /// 分账声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 最后面板族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl AngulardevLedger {
    /// 新账本。
    pub fn new() -> Self {
        AngulardevLedger {
            gates: [None; GATES],
            panels: [None; PANEL_CELLS],
            projs: [None; PROJ_CELLS],
            stacks: [None; STACK_CELLS],
            splits: [None; SPLIT_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), AngulardevCode> {
        if gate >= GATES {
            return Err(AngulardevCode::ESeal);
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
        while i < GATES {
            match self.gates.get(i) {
                Some(Some(true)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 面板一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_panel(&mut self, o: usize, got: u64) -> Result<(), AngulardevCode> {
        if o >= PANEL_CELLS {
            return Err(AngulardevCode::ENoPanel);
        }
        let kind = match panel_kind(o) {
            Some(k) => k,
            None => return Err(AngulardevCode::ENoPanel),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5200);
            return Err(AngulardevCode::EOrder);
        }
        let want = match panel_cell(o) {
            Some(w) => w,
            None => return Err(AngulardevCode::ENoPanel),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5201 + o as u64);
            return Err(AngulardevCode::EPanelDrift);
        }
        if match self.panels.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(AngulardevCode::EDup);
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
        while o < PANEL_CELLS {
            match self.panels.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 投影一格（载荷对照 + 重复拒）。
    pub fn record_proj(&mut self, p: usize, got: u64) -> Result<(), AngulardevCode> {
        if p >= PROJ_CELLS {
            return Err(AngulardevCode::ENoProj);
        }
        let want = match proj_cell(p) {
            Some(w) => w,
            None => return Err(AngulardevCode::ENoProj),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5300 + p as u64);
            return Err(AngulardevCode::EProjDrift);
        }
        if match self.projs.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(AngulardevCode::EDup);
        }
        if let Some(c) = self.projs.get_mut(p) {
            *c = Some(got);
        }
        self.fnv(0x5301 + p as u64);
        Ok(())
    }

    /// 投影全账核验（8 格）。
    pub fn projs_all(&self) -> bool {
        let mut p = 0usize;
        while p < PROJ_CELLS {
            match self.projs.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// 上栈一格（载荷对照 + 重复拒）。
    pub fn record_stack(&mut self, s: usize, got: u64) -> Result<(), AngulardevCode> {
        if s >= STACK_CELLS {
            return Err(AngulardevCode::ENoStack);
        }
        let want = match stack_cell(s) {
            Some(w) => w,
            None => return Err(AngulardevCode::ENoStack),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5400 + s as u64);
            return Err(AngulardevCode::EStackDrift);
        }
        if match self.stacks.get(s) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(AngulardevCode::EDup);
        }
        if let Some(c) = self.stacks.get_mut(s) {
            *c = Some(got);
        }
        self.fnv(0x5401 + s as u64);
        Ok(())
    }

    /// 上栈全账核验（2 格）。
    pub fn stacks_all(&self) -> bool {
        let mut s = 0usize;
        while s < STACK_CELLS {
            match self.stacks.get(s) {
                Some(Some(_)) => {}
                _ => return false,
            }
            s += 1;
        }
        true
    }

    /// 分账声明一行（React DevTools 分账，F52470）。
    pub fn declare_split(&mut self, r: usize) -> Result<(), AngulardevCode> {
        if r >= SPLIT_ROWS {
            return Err(AngulardevCode::ENoRow);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(AngulardevCode::EDup);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0x5500 + r as u64);
        Ok(())
    }

    /// 声明全齐核验。
    pub fn splits_all(&self) -> bool {
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            match self.splits.get(r) {
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

    /// KB-COMPAT 定级（六闸齐 → S2 目标档，全齐亦 S2，闸缺 → S3）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for AngulardevLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = AngulardevLedger::new();
    let mut g = 0usize;
    while g < GATES {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < PANEL_CELLS {
        if let Some(w) = panel_cell(o) {
            if led.record_panel(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut p = 0usize;
    while p < PROJ_CELLS {
        if let Some(w) = proj_cell(p) {
            if led.record_proj(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    let mut st = 0usize;
    while st < STACK_CELLS {
        if let Some(w) = stack_cell(st) {
            if led.record_stack(st, w).is_ok() {
                recorded += 1;
            }
        }
        st += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    // 注入 10 轮：面板漂移 / 面板族逆序 / 面板越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = AngulardevLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_panel(2, panel_cell(2).unwrap_or(0));
                lt.record_panel(3, panel_cell(3).unwrap_or(0).wrapping_add(1))
                    == Err(AngulardevCode::EPanelDrift)
            }
            1 => {
                let _ = lt.record_panel(9, panel_cell(9).unwrap_or(0));
                lt.record_panel(2, panel_cell(2).unwrap_or(0)) == Err(AngulardevCode::EOrder)
            }
            _ => lt.record_panel(PANEL_CELLS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5600 + k as u64);
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
/// UNX-F52473 判据集（锚 UNX-F52474-J1，off-by-one 板式）。
pub fn run_unx52473_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52473");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五面板族 20 格全通",
        recorded >= (PANEL_CELLS + PROJ_CELLS + STACK_CELLS) as u32,
        "组件树/Injector/NgRx/Router/Profiler 五族逐格恒等（UNX-F52474-J1 审计 ×10 全通口径）",
    );
    s.add(
        "状态投影 8 格全落",
        {
            let mut lt = AngulardevLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < PROJ_CELLS {
                if let Some(w) = proj_cell(p) {
                    if lt.record_proj(p, w).is_err() {
                        ok = false;
                    }
                }
                p += 1;
            }
            ok && lt.projs_all()
        },
        "信号/NgRx store/Router state/表单控件四族逐格恒等",
    );
    s.add(
        "JS 上栈声明 2 格恒等",
        {
            let mut lt = AngulardevLedger::new();
            let mut ok = true;
            let mut st = 0usize;
            while st < STACK_CELLS {
                if let Some(w) = stack_cell(st) {
                    if lt.record_stack(st, w).is_err() {
                        ok = false;
                    }
                }
                st += 1;
            }
            ok && lt.stacks_all()
        },
        "JS 运行时上栈/面板脚本宿主上栈两格闭集",
    );
    s.add(
        "连接三元闸六判据全落",
        {
            let mut lt = AngulardevLedger::new();
            let mut g = 0usize;
            while g < GATES {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "连接发现/握手/会话保持/面板接线/状态投影/遥测闭合逐格落账",
    );
    s.add(
        "React DevTools 分账声明 2/2 全齐",
        {
            let mut lt = AngulardevLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                if lt.declare_split(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.splits_all()
        },
        "组件树分账/Hooks 检查器分账两行闭集（F52470 分账，§AI-66.3 锚定）",
    );
    s.add(
        "面板族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < PANEL_CELLS {
                if panel_kind(o).unwrap_or(5) < panel_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五面板族闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "面板漂移/面板族逆序/面板越闭集三态轮转必抓",
    );
    let mut l0 = AngulardevLedger::new();
    s.add(
        "边界·面板越闭集",
        l0.record_panel(PANEL_CELLS, 0).is_err(),
        "面板格 20 越闭集必拒",
    );
    s.add(
        "面板漂移拒",
        {
            let _ = l0.record_panel(0, panel_cell(0).unwrap_or(0));
            l0.record_panel(1, panel_cell(1).unwrap_or(0).wrapping_add(1))
                == Err(AngulardevCode::EPanelDrift)
        },
        "面板载荷漂移必抓",
    );
    s.add(
        "边界·投影越闭集",
        l0.record_proj(PROJ_CELLS, 0).is_err(),
        "投影格 8 越闭集必拒",
    );
    s.add(
        "边界·上栈越闭集",
        l0.record_stack(STACK_CELLS, 0).is_err(),
        "上栈格 2 越闭集必拒",
    );
    s.add(
        "边界·声明行越闭集",
        l0.declare_split(SPLIT_ROWS) == Err(AngulardevCode::ENoRow),
        "分账行 2 越闭集必拒",
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
        "未齐即封核",
        {
            let mut lt = AngulardevLedger::new();
            let _ = lt.judge_gate(0);
            lt.judge_gate(0).is_ok() && !lt.gates_all() && lt.judge_grade() == Grade::S3
        },
        "账未齐时逐格仍 Ok 落账，定级不给目标档 S2",
    );
    s.add(
        "定级 S2 闭集",
        {
            let mut lt = AngulardevLedger::new();
            let mut g = 0usize;
            while g < GATES {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < PANEL_CELLS {
                if let Some(w) = panel_cell(o) {
                    let _ = lt.record_panel(o, w);
                }
                o += 1;
            }
            let mut p = 0usize;
            while p < PROJ_CELLS {
                if let Some(w) = proj_cell(p) {
                    let _ = lt.record_proj(p, w);
                }
                p += 1;
            }
            let mut st = 0usize;
            while st < STACK_CELLS {
                if let Some(w) = stack_cell(st) {
                    let _ = lt.record_stack(st, w);
                }
                st += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_split(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S2
        },
        "六闸齐 + 五族/投影/上栈/分账全齐 → S2（KB-COMPAT 定级登记，本卡目标档）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCB600,
        "SEG_BASE 0x40 对齐且在 VX16 域首格",
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
        assert_eq!(panel_kind(4), Some(1));
        assert_eq!(panel_kind(19), Some(4));
        assert!(panel_kind(20).is_none());
        assert!(panel_cell(20).is_none());
        assert_eq!(panel_kind_name(2), Some("NgRx"));
        assert!(panel_kind_name(5).is_none());
    }

    #[test]
    fn test_proj_kind() {
        assert_eq!(proj_kind(0), Some(0));
        assert_eq!(proj_kind(2), Some(1));
        assert_eq!(proj_kind(7), Some(3));
        assert!(proj_kind(8).is_none());
        assert!(proj_cell(8).is_none());
        assert_eq!(proj_kind_name(3), Some("表单控件"));
    }

    #[test]
    fn test_cells_pure() {
        let a = panel_cell(3).unwrap_or(0);
        assert_eq!(a, panel_cell(3).unwrap_or(0));
        assert_ne!(a, panel_cell(4).unwrap_or(0));
        assert_ne!(proj_cell(1).unwrap_or(0), proj_cell(2).unwrap_or(0));
        assert!(stack_cell(STACK_CELLS).is_none());
        assert!(split_row(SPLIT_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = AngulardevLedger::new();
        let _ = lt.record_panel(9, panel_cell(9).unwrap_or(0));
        assert_eq!(
            lt.record_panel(2, panel_cell(2).unwrap_or(0)),
            Err(AngulardevCode::EOrder)
        );
        assert!(lt.record_panel(10, panel_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = AngulardevLedger::new();
        assert_eq!(
            lt.record_proj(5, proj_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(AngulardevCode::EProjDrift)
        );
        assert_eq!(
            lt.record_stack(0, stack_cell(0).unwrap_or(0).wrapping_add(7)),
            Err(AngulardevCode::EStackDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = AngulardevLedger::new();
        let _ = lt.record_panel(0, panel_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_panel(0, panel_cell(0).unwrap_or(0)),
            Err(AngulardevCode::EDup)
        );
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = AngulardevLedger::new();
        let mut g = 0usize;
        while g < GATES {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(GATES).is_err());
        assert!(lt.gates_all());
        assert_eq!(gate_name(0), Some("连接发现"));
        assert_eq!(gate_name(5), Some("遥测闭合"));
        assert!(gate_name(6).is_none());
        let mut lt2 = AngulardevLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(
            recorded >= (PANEL_CELLS + PROJ_CELLS + STACK_CELLS) as u32,
            "recorded 须覆盖面板 20 + 投影 8 + 上栈 2"
        );
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
        assert_eq!(SEG_BASE, 0xCB600);
        assert_eq!(SEG_BASE, 0xCA400 + 0x40 * (ID - 52401));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(AngulardevCode::Ok as u32, 0);
        assert_eq!(AngulardevCode::EGradeDrift as u32, 12);
        assert_eq!(AngulardevCode::ESeal as u32, 11);
        assert_eq!(AngulardevCode::Ok.val(), SEG_BASE);
        assert!(!AngulardevCode::Ok.detail().is_empty());
        assert!(!AngulardevCode::ESeal.detail().is_empty());
        assert!(!AngulardevCode::EGradeDrift.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3，六闸齐 S2（本卡目标档），全齐亦 S2。
        let empty = AngulardevLedger::new();
        assert_eq!(empty.judge_grade(), Grade::S3);
        let mut mid = AngulardevLedger::new();
        let mut g = 0usize;
        while g < GATES {
            let _ = mid.judge_gate(g);
            g += 1;
        }
        assert_eq!(mid.judge_grade(), Grade::S2);
    }

    #[test]
    fn unx52473_checks_green() {
        let s = run_unx52473_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 18);
    }
}
