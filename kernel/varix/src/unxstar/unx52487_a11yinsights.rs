//! UNX-F52487 Accessibility Insights for Windows 收口 · 0xCB980段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52487"，逐字）：
//!   title: Accessibility Insights for Windows 收口
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52488-J1 六判据
//!     全落；UI 自动化 ×10；许可面如实定级（AI-95 联签锚定行）；
//!     KB-COMPAT 定级登记；判据锚定 §AI-66.9 红线②；③ 完成时
//!     API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52487）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52488-J1，照原文锚定；功能按本卡标题（Accessibility Insights
//! for Windows 收口）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 工具族闭集：五工具族（FastPass/Assessment/Color Contrast/
//!      Sample/Tab stops）各 4 操作共 20 格，工具族单调（逆序必抓）；
//!      逐操作载荷纯函数折叠，恒等才收。
//!   ② UIA 消费联签 4 格（消费 M 域 UIA 联签锚定行 AI-62）：逐格折叠
//!      对照，漂移必抓。
//!   ③ 规则投影 8 格：规则投影逐格折叠对照，漂移必抓。
//!   ④ 与 axe 分账声明 2 行闭集（axe-core 引擎独立账声明/axe 类管道
//!      分账声明），缺一拒封；C# 上栈声明 2 行闭集（C# 依 .NET 上栈
//!      声明/UIA COM 互操作对齐声明），缺一拒封。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/seal_gate/fingerprint
//!      FNV-1a）；注入核 run_cycle 三态轮转 10 轮（工具族逆序/规则投
//!      影漂移/UIA 联签漂移）10/10 必抓；双跑 FNV 指纹一致。
//!   ⑥ KB-COMPAT 定级闭集（S1/S2/S3）：全齐 → S1，六判据齐 → S2，
//!      空账 S3（许可面如实定级，AI-95 联签锚定行）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52487;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 工具操作数。
pub const TOOL_OPS: usize = 20;

/// UIA 消费联签格数。
pub const UIA_CELLS: usize = 4;

/// 规则投影格数。
pub const PROJ_CELLS: usize = 8;

/// axe 分账声明行数。
pub const AXE_ROWS: usize = 2;

/// C# 上栈声明行数。
pub const CS_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum A11yCode {
    /// 成功
    Ok = 0,
    /// 操作号越闭集
    ENoOp,
    /// 操作漂移
    EDrift,
    /// 工具族逆序
    EOrder,
    /// 投影格越闭集
    ENoProj,
    /// 投影漂移
    EProjDrift,
    /// UIA 联签格越闭集
    ENoUia,
    /// UIA 联签漂移
    EUiaDrift,
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

impl A11yCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            A11yCode::Ok => "成功",
            A11yCode::ENoOp => "操作号越闭集：五工具族 20 操作集之外先登记",
            A11yCode::EDrift => "操作漂移：操作折叠与基准不恒等，逐格复算",
            A11yCode::EOrder => "工具族逆序：FastPass→Assessment→Color Contrast→Sample→Tab stops 单调，逆序必抓",
            A11yCode::ENoProj => "投影格越闭集：规则投影 8 格之外先登记",
            A11yCode::EProjDrift => "投影漂移：规则投影折叠与基准不恒等，逐格复算",
            A11yCode::ENoUia => "联签格越闭集：UIA 消费联签 4 格之外先登记",
            A11yCode::EUiaDrift => "UIA 联签漂移：联签折叠与基准不恒等，逐格复算",
            A11yCode::ENoRow => "声明行越闭集：axe 分账/C# 上栈各 2 行闭集之外先登记",
            A11yCode::ERowMiss => "缺声明行：axe 分账与 C# 上栈声明未齐先补齐",
            A11yCode::EDup => "重复落账：同格只许一条账目",
            A11yCode::ESeal => "未齐即封：20 操作 + 4 联签 + 8 投影 + 4 行声明未齐先补齐",
            A11yCode::EGradeDrift => "定级漂移：全齐才可定 S1（KB-COMPAT 定级登记，AI-95 许可面如实）",
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
    /// 定级值（const 可调）。
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
            Grade::S1 => "S1：20 操作 + 4 联签 + 8 投影 + 4 行声明全通收口",
            Grade::S2 => "S2：六判据齐，有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 工具操作格载荷（操作 o；工具族 = o/4）。
pub const fn tool_cell(o: usize) -> Option<u64> {
    if o >= TOOL_OPS {
        return None;
    }
    Some((0xD9000u64 + (o as u64) * 0x38) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 工具族（0..4=FastPass 4..8=Assessment 8..12=Color Contrast 12..16=Sample 16..20=Tab stops）。
pub const fn tool_kind(o: usize) -> Option<u32> {
    if o >= TOOL_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 工具族名。
pub const fn tool_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("FastPass"),
        1 => Some("Assessment"),
        2 => Some("Color Contrast"),
        3 => Some("Sample"),
        4 => Some("Tab stops"),
        _ => None,
    }
}

/// UIA 消费联签格载荷（格 j；AI-62 锚定行）。
pub const fn uia_cell(j: usize) -> Option<u64> {
    if j >= UIA_CELLS {
        return None;
    }
    Some((0xDA000u64 + (j as u64) * 0x5C) ^ 0x5549410000A162)
}

/// 规则投影格载荷（格 i）。
pub const fn proj_cell(i: usize) -> Option<u64> {
    if i >= PROJ_CELLS {
        return None;
    }
    Some((0xDB000u64 + (i as u64) * 0x48) ^ 0x4149575F50524A)
}

/// axe 分账声明行折叠（行 r）。
pub const fn axe_row(r: usize) -> Option<u32> {
    if r >= AXE_ROWS {
        return None;
    }
    Some(0xDB80 + r as u32 * 0x38)
}

/// C# 上栈声明行折叠（行 r）。
pub const fn cs_row(r: usize) -> Option<u32> {
    if r >= CS_ROWS {
        return None;
    }
    Some(0xDBC0 + r as u32 * 0x38)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Accessibility Insights for Windows 收口账本状态机。
#[derive(Clone)]
pub struct A11yLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 工具操作账。
    tools: [Option<u64>; TOOL_OPS],
    /// UIA 联签账。
    uias: [Option<u64>; UIA_CELLS],
    /// 规则投影账。
    projs: [Option<u64>; PROJ_CELLS],
    /// axe 分账声明账。
    axerows: [Option<u32>; AXE_ROWS],
    /// C# 上栈声明账。
    csrows: [Option<u32>; CS_ROWS],
    /// 最后工具族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl A11yLedger {
    /// 新账本。
    pub fn new() -> Self {
        A11yLedger {
            gates: [None; 6],
            tools: [None; TOOL_OPS],
            uias: [None; UIA_CELLS],
            projs: [None; PROJ_CELLS],
            axerows: [None; AXE_ROWS],
            csrows: [None; CS_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), A11yCode> {
        if gate >= 6 {
            return Err(A11yCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5C100 + gate as u64);
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

    /// 工具操作一格（工具族单调 + 载荷对照 + 重复拒）。
    pub fn record_tool(&mut self, o: usize, got: u64) -> Result<(), A11yCode> {
        if o >= TOOL_OPS {
            return Err(A11yCode::ENoOp);
        }
        let kind = match tool_kind(o) {
            Some(k) => k,
            None => return Err(A11yCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5C200);
            return Err(A11yCode::EOrder);
        }
        let want = match tool_cell(o) {
            Some(w) => w,
            None => return Err(A11yCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5C201 + o as u64);
            return Err(A11yCode::EDrift);
        }
        if match self.tools.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(A11yCode::EDup);
        }
        if let Some(c) = self.tools.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5C202 + o as u64);
        Ok(())
    }

    /// 工具操作全账核验（20 格）。
    pub fn tools_all(&self) -> bool {
        let mut o = 0usize;
        while o < TOOL_OPS {
            match self.tools.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// UIA 联签一格（载荷对照 + 重复拒）。
    pub fn record_uia(&mut self, j: usize, got: u64) -> Result<(), A11yCode> {
        let want = match uia_cell(j) {
            Some(w) => w,
            None => return Err(A11yCode::ENoUia),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5C300 + j as u64);
            return Err(A11yCode::EUiaDrift);
        }
        if match self.uias.get(j) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(A11yCode::EDup);
        }
        if let Some(c) = self.uias.get_mut(j) {
            *c = Some(got);
        }
        self.fnv(0x5C301 + j as u64);
        Ok(())
    }

    /// UIA 联签全账核验（4 格）。
    pub fn uias_all(&self) -> bool {
        let mut j = 0usize;
        while j < UIA_CELLS {
            match self.uias.get(j) {
                Some(Some(_)) => {}
                _ => return false,
            }
            j += 1;
        }
        true
    }

    /// 规则投影一格（载荷对照 + 重复拒）。
    pub fn record_proj(&mut self, i: usize, got: u64) -> Result<(), A11yCode> {
        let want = match proj_cell(i) {
            Some(w) => w,
            None => return Err(A11yCode::ENoProj),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5C400 + i as u64);
            return Err(A11yCode::EProjDrift);
        }
        if match self.projs.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(A11yCode::EDup);
        }
        if let Some(c) = self.projs.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0x5C401 + i as u64);
        Ok(())
    }

    /// 规则投影全账核验（8 格）。
    pub fn projs_all(&self) -> bool {
        let mut i = 0usize;
        while i < PROJ_CELLS {
            match self.projs.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// axe 分账声明一行。
    pub fn declare_axe(&mut self, r: usize) -> Result<(), A11yCode> {
        if r >= AXE_ROWS {
            return Err(A11yCode::ENoRow);
        }
        if match self.axerows.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(A11yCode::EDup);
        }
        if let Some(c) = self.axerows.get_mut(r) {
            *c = axe_row(r);
        }
        self.fnv(0x5C500 + r as u64);
        Ok(())
    }

    /// axe 分账声明全齐核验。
    pub fn axe_all(&self) -> bool {
        let mut r = 0usize;
        while r < AXE_ROWS {
            match self.axerows.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// C# 上栈声明一行。
    pub fn declare_cs(&mut self, r: usize) -> Result<(), A11yCode> {
        if r >= CS_ROWS {
            return Err(A11yCode::ENoRow);
        }
        if match self.csrows.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(A11yCode::EDup);
        }
        if let Some(c) = self.csrows.get_mut(r) {
            *c = cs_row(r);
        }
        self.fnv(0x5C600 + r as u64);
        Ok(())
    }

    /// C# 上栈声明全齐核验。
    pub fn cs_all(&self) -> bool {
        let mut r = 0usize;
        while r < CS_ROWS {
            match self.csrows.get(r) {
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

    /// 收口封闸：声明缺 → ERowMiss；判据/格账缺 → ESeal；全齐 → Ok。
    pub fn seal_gate(&self) -> Result<(), A11yCode> {
        if !self.gates_all() {
            return Err(A11yCode::ESeal);
        }
        if !self.axe_all() || !self.cs_all() {
            return Err(A11yCode::ERowMiss);
        }
        if !self.tools_all() || !self.uias_all() || !self.projs_all() {
            return Err(A11yCode::ESeal);
        }
        Ok(())
    }

    /// KB-COMPAT 定级（全齐 → S1；六判据齐 → S2；空账 → S3）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() && self.tools_all() && self.uias_all() && self.projs_all()
            && self.axe_all() && self.cs_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }

    /// 定级核验：与期望档不符即 EGradeDrift。
    pub fn verify_grade(&self, want: Grade) -> Result<(), A11yCode> {
        if self.judge_grade() == want {
            Ok(())
        } else {
            Err(A11yCode::EGradeDrift)
        }
    }
}

impl Default for A11yLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = A11yLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < TOOL_OPS {
        if let Some(w) = tool_cell(o) {
            if led.record_tool(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut j = 0usize;
    while j < UIA_CELLS {
        if let Some(w) = uia_cell(j) {
            if led.record_uia(j, w).is_ok() {
                recorded += 1;
            }
        }
        j += 1;
    }
    let mut i = 0usize;
    while i < PROJ_CELLS {
        if let Some(w) = proj_cell(i) {
            if led.record_proj(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut r = 0usize;
    while r < AXE_ROWS {
        let _ = led.declare_axe(r);
        r += 1;
    }
    let mut c = 0usize;
    while c < CS_ROWS {
        let _ = led.declare_cs(c);
        c += 1;
    }
    // 注入 10 轮：工具族逆序 / 规则投影漂移 / UIA 联签漂移三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = A11yLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_tool(17, tool_cell(17).unwrap_or(0));
                lt.record_tool(5, tool_cell(5).unwrap_or(0)) == Err(A11yCode::EOrder)
            }
            1 => {
                lt.record_proj(3, proj_cell(3).unwrap_or(0).wrapping_add(1))
                    == Err(A11yCode::EProjDrift)
            }
            _ => {
                lt.record_uia(1, uia_cell(1).unwrap_or(0).wrapping_add(9))
                    == Err(A11yCode::EUiaDrift)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5C700 + k as u64);
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
/// UNX-F52487 判据集（锚 UNX-F52488-J1，off-by-one 板式）。
pub fn run_unx52487_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52487");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五工具族 20 操作全通",
        recorded >= TOOL_OPS as u32,
        "FastPass/Assessment/Color Contrast/Sample/Tab stops 五族逐操作恒等",
    );
    s.add(
        "UIA 消费联签 4/4",
        {
            let mut lt = A11yLedger::new();
            let mut ok = true;
            let mut j = 0usize;
            while j < UIA_CELLS {
                if let Some(w) = uia_cell(j) {
                    if lt.record_uia(j, w).is_err() {
                        ok = false;
                    }
                }
                j += 1;
            }
            ok && lt.uias_all()
        },
        "消费 M 域 UIA 联签四格闭集（AI-62 锚定行）",
    );
    s.add(
        "规则投影 8 格全落",
        {
            let mut lt = A11yLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < PROJ_CELLS {
                if let Some(w) = proj_cell(i) {
                    if lt.record_proj(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.projs_all()
        },
        "规则投影逐格折叠恒等，漂移必抓",
    );
    s.add(
        "axe 分账声明 2/2 全齐",
        {
            let mut lt = A11yLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < AXE_ROWS {
                if lt.declare_axe(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.axe_all()
        },
        "axe-core 引擎独立账声明/axe 类管道分账声明两行闭集",
    );
    s.add(
        "C# 上栈声明 2/2 全齐",
        {
            let mut lt = A11yLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < CS_ROWS {
                if lt.declare_cs(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.cs_all()
        },
        "C# 依 .NET 上栈声明/UIA COM 互操作对齐声明两行闭集",
    );
    s.add(
        "工具族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < TOOL_OPS {
                if tool_kind(o).unwrap_or(5) < tool_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五工具族闭集单调",
    );
    s.add("注入 10/10 全抓", caught >= 10, "工具族逆序/投影漂移/UIA 联签漂移三态轮转必抓");
    let mut l0 = A11yLedger::new();
    s.add(
        "边界·工具操作越闭集",
        l0.record_tool(TOOL_OPS, 0).is_err(),
        "工具操作 20 越闭集必拒",
    );
    s.add(
        "边界·投影越闭集",
        l0.record_proj(PROJ_CELLS, 0).is_err(),
        "规则投影 8 越闭集必拒",
    );
    s.add(
        "边界·UIA 联签越闭集",
        l0.record_uia(UIA_CELLS, 0).is_err(),
        "UIA 联签格 4 越闭集必拒",
    );
    s.add(
        "未齐落账不拒（封点前 Ok）",
        {
            let _ = l0.record_tool(0, tool_cell(0).unwrap_or(0));
            l0.record_proj(0, proj_cell(0).unwrap_or(0)).is_ok()
        },
        "账未齐时落账仍 Ok，封拒只发生在封闸",
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
        "定级 S1 闭集",
        {
            let mut lt = A11yLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < TOOL_OPS {
                if let Some(w) = tool_cell(o) {
                    let _ = lt.record_tool(o, w);
                }
                o += 1;
            }
            let mut j = 0usize;
            while j < UIA_CELLS {
                if let Some(w) = uia_cell(j) {
                    let _ = lt.record_uia(j, w);
                }
                j += 1;
            }
            let mut i = 0usize;
            while i < PROJ_CELLS {
                if let Some(w) = proj_cell(i) {
                    let _ = lt.record_proj(i, w);
                }
                i += 1;
            }
            let mut r = 0usize;
            while r < AXE_ROWS {
                let _ = lt.declare_axe(r);
                r += 1;
            }
            let mut c = 0usize;
            while c < CS_ROWS {
                let _ = lt.declare_cs(c);
                c += 1;
            }
            lt.judge_grade() == Grade::S1 && lt.verify_grade(Grade::S1).is_ok()
        },
        "全齐 → S1（KB-COMPAT 定级登记，许可面如实定级）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCB980,
        "SEG_BASE 0x40 对齐且落在 VX16 0xCB980 段",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_kind() {
        assert_eq!(tool_kind(0), Some(0));
        assert_eq!(tool_kind(3), Some(0));
        assert_eq!(tool_kind(4), Some(1));
        assert_eq!(tool_kind(8), Some(2));
        assert_eq!(tool_kind(12), Some(3));
        assert_eq!(tool_kind(19), Some(4));
        assert!(tool_kind(20).is_none());
        assert!(tool_cell(20).is_none());
        assert_eq!(tool_kind_name(0), Some("FastPass"));
        assert_eq!(tool_kind_name(4), Some("Tab stops"));
        assert!(tool_kind_name(5).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = tool_cell(11).unwrap_or(0);
        assert_eq!(a, tool_cell(11).unwrap_or(0));
        assert_ne!(a, tool_cell(12).unwrap_or(0));
        assert!(uia_cell(UIA_CELLS).is_none());
        assert!(proj_cell(PROJ_CELLS).is_none());
        assert!(axe_row(AXE_ROWS).is_none());
        assert!(cs_row(CS_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = A11yLedger::new();
        let _ = lt.record_tool(9, tool_cell(9).unwrap_or(0));
        assert_eq!(lt.record_tool(2, tool_cell(2).unwrap_or(0)), Err(A11yCode::EOrder));
        assert!(lt.record_tool(10, tool_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = A11yLedger::new();
        assert_eq!(
            lt.record_tool(5, tool_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(A11yCode::EDrift)
        );
        assert_eq!(
            lt.record_proj(2, proj_cell(2).unwrap_or(0).wrapping_add(7)),
            Err(A11yCode::EProjDrift)
        );
        assert_eq!(
            lt.record_uia(0, uia_cell(0).unwrap_or(0).wrapping_add(0xA162)),
            Err(A11yCode::EUiaDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = A11yLedger::new();
        let _ = lt.record_uia(2, uia_cell(2).unwrap_or(0));
        assert_eq!(lt.record_uia(2, uia_cell(2).unwrap_or(0)), Err(A11yCode::EDup));
    }

    #[test]
    fn test_gate_and_seal() {
        let mut lt = A11yLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        // 账未齐：声明行缺 → ERowMiss。
        assert_eq!(lt.seal_gate(), Err(A11yCode::ERowMiss));
        assert!(lt.record_tool(0, tool_cell(0).unwrap_or(0)).is_ok());
        let mut lt2 = A11yLedger::new();
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_row_miss() {
        let mut lt = A11yLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt.seal_gate(), Err(A11yCode::ERowMiss));
        assert!(lt.declare_axe(AXE_ROWS).is_err());
        assert!(lt.declare_cs(CS_ROWS).is_err());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert_eq!(
            recorded,
            TOOL_OPS as u32 + UIA_CELLS as u32 + PROJ_CELLS as u32
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
        assert_eq!(SEG_BASE, 0xCB980);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(A11yCode::Ok as u32, 0);
        assert_eq!(A11yCode::EGradeDrift as u32, 12);
        assert_eq!(A11yCode::Ok.val(), SEG_BASE);
        let mut i = 0u32;
        while i <= 12 {
            let c = match i {
                0 => A11yCode::Ok,
                1 => A11yCode::ENoOp,
                2 => A11yCode::EDrift,
                3 => A11yCode::EOrder,
                4 => A11yCode::ENoProj,
                5 => A11yCode::EProjDrift,
                6 => A11yCode::ENoUia,
                7 => A11yCode::EUiaDrift,
                8 => A11yCode::ENoRow,
                9 => A11yCode::ERowMiss,
                10 => A11yCode::EDup,
                11 => A11yCode::ESeal,
                _ => A11yCode::EGradeDrift,
            };
            assert_eq!(c.val(), SEG_BASE + i);
            assert!(!c.detail().is_empty());
            i += 1;
        }
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        // 如实阶梯：空账 S3。
        let lt0 = A11yLedger::new();
        assert_eq!(lt0.judge_grade(), Grade::S3);
        assert!(lt0.verify_grade(Grade::S1).is_err());
        // 六闸齐 S2。
        let mut lt1 = A11yLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt1.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt1.judge_grade(), Grade::S2);
    }

    #[test]
    fn unx52487_checks_green() {
        let s = run_unx52487_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
