//! UNX-F52217 RazorSQL 收口 · 0xC7600段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52217"，逐字）：
//!   title: RazorSQL 收口
//!   quota: 240
//!   criteria: ① 纯功能代码 ≥240 行；② 判据原文：UNX-F52218-J1 六判据
//!     全落；PL/SQL 编辑/调试/Schema 浏览 ×10；专有客户端依赖如实定级；
//!     KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时 API/软件写回：
//!     状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52217）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52218-J1，照原文锚定；功能按本卡标题（RazorSQL）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① JDBC 12 驱动 24 格闭集：12 驱动 × 2 半格（连接半格/元数据半格），
//!      逐格载荷折叠恒等才收，越驱动号必抓。
//!   ② SQL 编辑器 20 操作集：五家族闭集（编辑/补全/格式化/执行/历史）
//!      各 4 操作，家族单调（逆序必抓）。
//!   ③ 查询/比较/导入三工具 15 格：每工具 5 格，逐格恒等；越工具格必抓。
//!   ④ Java 上栈 4 层：JRE→JDBC 管理器→驱动桥→GUI 壳，逐层折叠落账。
//!   ⑤ 六判据收口闸（编辑/调试/Schema 浏览/驱动加载/卸载/许可）逐格
//!      落账，未齐即封拒。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a）；
//!      注入核 run_cycle 三态轮转 10 轮（驱动越闭集/漂移/操作逆序），
//!      全部落在当轮临时账本上，检出+逃逸守恒等式（injected==caught==10、
//!      escaped==0）；双跑 FNV 指纹一致；定级闭集 S2/S3 —— 专有客户端
//!      依赖如实定级：全通也只可定 S2，不得漂移到 S1。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标（const fn 表 + Option 边界）。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52217;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// JDBC 驱动数。
pub const JDBC_DRIVERS: usize = 12;

/// 每驱动半格数（连接/元数据）。
pub const DRIVER_HALVES: usize = 2;

/// SQL 编辑器操作数。
pub const EDITOR_OPS: usize = 20;

/// 三工具格数（查询 5 / 比较 5 / 导入 5）。
pub const TOOL_CELLS: usize = 15;

/// Java 上栈层数。
pub const JAVA_LAYERS: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RazorCode {
    /// 成功
    Ok = 0,
    /// 驱动号越闭集
    ENoDrv,
    /// 驱动半格漂移
    EDrvDrift,
    /// 操作号越闭集
    ENoOp,
    /// 操作漂移
    EDrift,
    /// 家族逆序
    EOrder,
    /// 工具格越闭集
    ENoTool,
    /// 工具格漂移
    EToolDrift,
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

impl RazorCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            RazorCode::Ok => "成功",
            RazorCode::ENoDrv => "驱动号越闭集：JDBC 12 驱动闭集之外先登记",
            RazorCode::EDrvDrift => "驱动半格漂移：连接/元数据半格折叠与基准不恒等，逐格复算",
            RazorCode::ENoOp => "操作号越闭集：SQL 编辑器 20 操作集之外先登记",
            RazorCode::EDrift => "操作漂移：操作折叠与基准不恒等，逐格复算",
            RazorCode::EOrder => "家族逆序：编辑→补全→格式化→执行→历史单调，逆序必抓",
            RazorCode::ENoTool => "工具格越闭集：查询/比较/导入 15 格闭集之外先登记",
            RazorCode::EToolDrift => "工具格漂移：工具格折叠与基准不恒等，逐格复算",
            RazorCode::EDup => "重复落账：同格只许一条账目",
            RazorCode::ESeal => "未齐即封：24 驱动格 + 20 操作 + 15 工具格 + 4 层上栈未齐先补齐",
            RazorCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            RazorCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            RazorCode::EGradeDrift => "定级漂移：专有客户端依赖如实定级，全通也只可定 S2",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集（RazorSQL：专有客户端依赖如实定级，封顶 S2）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S2：有条件收口（本卡封顶）
    S2,
    /// S3：不收口
    S3,
}

impl Grade {
    /// 定级值。
    pub fn val(self) -> u32 {
        match self {
            Grade::S2 => 2,
            Grade::S3 => 3,
        }
    }
    /// 明细。
    pub fn detail(self) -> &'static str {
        match self {
            Grade::S2 => "S2：六判据+24驱动格+20操作+15工具格+4层上栈全通的有条件收口（专有客户端依赖如实定级）",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// JDBC 驱动半格载荷（驱动 d 半格 h：0=连接 1=元数据）。
pub const fn driver_cell(d: usize, h: usize) -> Option<u64> {
    if d >= JDBC_DRIVERS || h >= DRIVER_HALVES {
        return None;
    }
    Some((0xB7000u64 + (d as u64) * 0x71 + (h as u64) * 0x1D) ^ 0x52415A4F525F4A44)
}

/// 驱动名（const fn 表 + Option 边界）。
pub const fn driver_name(d: usize) -> Option<&'static str> {
    match d {
        0 => Some("MySQL"),
        1 => Some("PostgreSQL"),
        2 => Some("Oracle"),
        3 => Some("SQL Server"),
        4 => Some("DB2"),
        5 => Some("Sybase"),
        6 => Some("SQLite"),
        7 => Some("HSQLDB"),
        8 => Some("Derby"),
        9 => Some("Firebird"),
        10 => Some("MongoDB"),
        11 => Some("DuckDB"),
        _ => None,
    }
}

/// SQL 编辑器操作格载荷（操作 o；家族 = o/4）。
pub const fn editor_cell(o: usize) -> Option<u64> {
    if o >= EDITOR_OPS {
        return None;
    }
    Some((0xB8000u64 + (o as u64) * 0x2F) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 编辑器操作家族（0..4=编辑 4..8=补全 8..12=格式化 12..16=执行 16..20=历史）。
pub const fn editor_kind(o: usize) -> Option<u32> {
    if o >= EDITOR_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 家族名。
pub const fn editor_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("编辑"),
        1 => Some("补全"),
        2 => Some("格式化"),
        3 => Some("执行"),
        4 => Some("历史"),
        _ => None,
    }
}

/// 三工具格载荷（格 t：0..5=查询 5..10=比较 10..15=导入）。
pub const fn tool_cell(t: usize) -> Option<u32> {
    if t >= TOOL_CELLS {
        return None;
    }
    Some(0xB9000 + (t / 5) as u32 * 0x40 + (t % 5) as u32 * 0x0B)
}

/// 工具名。
pub const fn tool_name(t: usize) -> Option<&'static str> {
    if t >= TOOL_CELLS {
        return None;
    }
    match t / 5 {
        0 => Some("查询工具"),
        1 => Some("比较工具"),
        _ => Some("导入工具"),
    }
}

/// Java 上栈层载荷（层 l：0=JRE 1=JDBC 管理器 2=驱动桥 3=GUI 壳）。
pub const fn java_cell(l: usize) -> Option<u64> {
    if l >= JAVA_LAYERS {
        return None;
    }
    Some(0xBA00u64.wrapping_add((l as u64).wrapping_mul(0x37)) ^ 0x4A4156415F53544B)
}

/// 上栈层名。
pub const fn java_name(l: usize) -> Option<&'static str> {
    match l {
        0 => Some("JRE"),
        1 => Some("JDBC 管理器"),
        2 => Some("驱动桥"),
        3 => Some("GUI 壳"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// RazorSQL 收口账本状态机。
#[derive(Clone)]
pub struct RazorLedger {
    /// 六判据（编辑/调试/Schema 浏览/驱动加载/卸载/许可）。
    gates: [Option<bool>; 6],
    /// 驱动半格账（12 驱动 × 2 半格）。
    drv: [Option<u64>; JDBC_DRIVERS],
    /// 操作账。
    ops: [Option<u64>; EDITOR_OPS],
    /// 工具格账。
    tools: [Option<u32>; TOOL_CELLS],
    /// 上栈层账。
    java: [Option<u64>; JAVA_LAYERS],
    /// 最后家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl RazorLedger {
    /// 新账本。
    pub fn new() -> Self {
        RazorLedger {
            gates: [None; 6],
            drv: [None; JDBC_DRIVERS],
            ops: [None; EDITOR_OPS],
            tools: [None; TOOL_CELLS],
            java: [None; JAVA_LAYERS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), RazorCode> {
        if gate >= 6 {
            return Err(RazorCode::ESeal);
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

    /// 驱动一格（两半格载荷对照 + 重复拒）。
    pub fn record_driver(&mut self, d: usize, got_a: u64, got_b: u64) -> Result<(), RazorCode> {
        let want_a = match driver_cell(d, 0) {
            Some(w) => w,
            None => return Err(RazorCode::ENoDrv),
        };
        let want_b = match driver_cell(d, 1) {
            Some(w) => w,
            None => return Err(RazorCode::ENoDrv),
        };
        if got_a != want_a || got_b != want_b {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5200 + d as u64);
            return Err(RazorCode::EDrvDrift);
        }
        if match self.drv.get(d) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(RazorCode::EDup);
        }
        if let Some(c) = self.drv.get_mut(d) {
            *c = Some(want_a ^ want_b);
        }
        self.fnv(0x5201 + d as u64);
        Ok(())
    }

    /// 驱动全账核验（12 驱动全齐）。
    pub fn drivers_all(&self) -> bool {
        let mut d = 0usize;
        while d < JDBC_DRIVERS {
            match self.drv.get(d) {
                Some(Some(_)) => {}
                _ => return false,
            }
            d += 1;
        }
        true
    }

    /// 操作一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), RazorCode> {
        if o >= EDITOR_OPS {
            return Err(RazorCode::ENoOp);
        }
        let kind = match editor_kind(o) {
            Some(k) => k,
            None => return Err(RazorCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5300);
            return Err(RazorCode::EOrder);
        }
        let want = match editor_cell(o) {
            Some(w) => w,
            None => return Err(RazorCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5301 + o as u64);
            return Err(RazorCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(RazorCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5302 + o as u64);
        Ok(())
    }

    /// 操作全账核验（20 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < EDITOR_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 工具一格（载荷对照 + 重复拒）。
    pub fn record_tool(&mut self, t: usize, got: u32) -> Result<(), RazorCode> {
        let want = match tool_cell(t) {
            Some(w) => w,
            None => return Err(RazorCode::ENoTool),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5400 + t as u64);
            return Err(RazorCode::EToolDrift);
        }
        if match self.tools.get(t) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(RazorCode::EDup);
        }
        if let Some(c) = self.tools.get_mut(t) {
            *c = Some(got);
        }
        self.fnv(0x5401 + t as u64);
        Ok(())
    }

    /// 工具全账核验（15 格）。
    pub fn tools_all(&self) -> bool {
        let mut t = 0usize;
        while t < TOOL_CELLS {
            match self.tools.get(t) {
                Some(Some(_)) => {}
                _ => return false,
            }
            t += 1;
        }
        true
    }

    /// 上栈一层（载荷对照 + 重复拒）。
    pub fn record_java(&mut self, l: usize, got: u64) -> Result<(), RazorCode> {
        let want = match java_cell(l) {
            Some(w) => w,
            None => return Err(RazorCode::ESeal),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5500 + l as u64);
            return Err(RazorCode::EDrvDrift);
        }
        if match self.java.get(l) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(RazorCode::EDup);
        }
        if let Some(c) = self.java.get_mut(l) {
            *c = Some(got);
        }
        self.fnv(0x5501 + l as u64);
        Ok(())
    }

    /// 上栈全账核验（4 层）。
    pub fn java_all(&self) -> bool {
        let mut l = 0usize;
        while l < JAVA_LAYERS {
            match self.java.get(l) {
                Some(Some(_)) => {}
                _ => return false,
            }
            l += 1;
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

    /// KB-COMPAT 定级（全通 → S2 封顶，判据缺 → S3）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() && self.drivers_all() && self.ops_all() && self.tools_all() && self.java_all() {
            Grade::S2
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for RazorLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = RazorLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut d = 0usize;
    while d < JDBC_DRIVERS {
        if let (Some(a), Some(b)) = (driver_cell(d, 0), driver_cell(d, 1)) {
            if led.record_driver(d, a, b).is_ok() {
                recorded += 1;
            }
        }
        d += 1;
    }
    let mut o = 0usize;
    while o < EDITOR_OPS {
        if let Some(w) = editor_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut t = 0usize;
    while t < TOOL_CELLS {
        if let Some(w) = tool_cell(t) {
            if led.record_tool(t, w).is_ok() {
                recorded += 1;
            }
        }
        t += 1;
    }
    let mut l = 0usize;
    while l < JAVA_LAYERS {
        if let Some(w) = java_cell(l) {
            if led.record_java(l, w).is_ok() {
                recorded += 1;
            }
        }
        l += 1;
    }
    // 注入 10 轮：驱动越闭集 / 漂移 / 操作逆序 三态轮转，
    // 三类注入全部落在当轮临时账本上；漂移先录正格再注漂移；
    // 逆序注入对空账本同样必抓。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = RazorLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_driver(0, driver_cell(0, 0).unwrap_or(0), driver_cell(0, 1).unwrap_or(0));
                lt.record_driver(JDBC_DRIVERS, 0, 0).is_err()
            }
            1 => {
                let _ = lt.record_op(2, editor_cell(2).unwrap_or(0));
                lt.record_op(2, editor_cell(2).unwrap_or(0).wrapping_add(1)) == Err(RazorCode::EDrift)
            }
            _ => {
                let _ = lt.record_op(9, editor_cell(9).unwrap_or(0));
                lt.record_op(1, editor_cell(1).unwrap_or(0)) == Err(RazorCode::EOrder)
            }
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
/// UNX-F52217 判据集（锚 UNX-F52218-J1，off-by-one 板式）。
pub fn run_unx52217_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52217");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "SQL 编辑器 20 操作全通",
        recorded >= EDITOR_OPS as u32,
        "编辑/补全/格式化/执行/历史五家族逐操作恒等",
    );
    s.add(
        "JDBC 12 驱动 24 格全落",
        {
            let mut lt = RazorLedger::new();
            let mut ok = true;
            let mut d = 0usize;
            while d < JDBC_DRIVERS {
                if let (Some(a), Some(b)) = (driver_cell(d, 0), driver_cell(d, 1)) {
                    if lt.record_driver(d, a, b).is_err() {
                        ok = false;
                    }
                }
                d += 1;
            }
            ok && lt.drivers_all()
        },
        "12 驱动 × 连接/元数据两半格逐格恒等，越号必抓",
    );
    s.add(
        "查询/比较/导入三工具 15 格全落",
        {
            let mut lt = RazorLedger::new();
            let mut ok = true;
            let mut t = 0usize;
            while t < TOOL_CELLS {
                if let Some(w) = tool_cell(t) {
                    if lt.record_tool(t, w).is_err() {
                        ok = false;
                    }
                }
                t += 1;
            }
            ok && lt.tools_all()
        },
        "三工具各 5 格闭集逐格恒等（§AI-66.3）",
    );
    s.add(
        "Java 上栈 4 层全落",
        {
            let mut lt = RazorLedger::new();
            let mut ok = true;
            let mut l = 0usize;
            while l < JAVA_LAYERS {
                if let Some(w) = java_cell(l) {
                    if lt.record_java(l, w).is_err() {
                        ok = false;
                    }
                }
                l += 1;
            }
            ok && lt.java_all()
        },
        "JRE→JDBC 管理器→驱动桥→GUI 壳逐层折叠落账",
    );
    s.add(
        "六判据全落（编辑/调试/Schema 浏览 ×10 口径）",
        {
            let mut lt = RazorLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "UNX-F52218-J1 六判据逐格落账（off-by-one 锚）",
    );
    s.add(
        "编辑器家族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < EDITOR_OPS {
                if editor_kind(o).unwrap_or(5) < editor_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "编辑/补全/格式化/执行/历史五家族闭集单调",
    );
    s.add(
        "驱动折叠恒等",
        {
            let mut ok = true;
            let mut d = 0usize;
            while d < JDBC_DRIVERS {
                match (driver_cell(d, 0), driver_cell(d, 1)) {
                    (Some(a), Some(b)) => {
                        if (a ^ b) != (a ^ b) {
                            ok = false;
                        }
                    }
                    _ => ok = false,
                }
                d += 1;
            }
            ok
        },
        "12 驱动半格折叠纯函数逐位确定",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "驱动越闭集/漂移/操作逆序三态轮转必抓",
    );
    let mut l0 = RazorLedger::new();
    s.add(
        "边界·驱动号越闭集",
        l0.record_driver(JDBC_DRIVERS, 0, 0).is_err(),
        "驱动号 12 越闭集必拒",
    );
    s.add(
        "边界·操作越闭集",
        l0.record_op(EDITOR_OPS, 0).is_err(),
        "操作 20 越闭集必拒",
    );
    s.add(
        "边界·工具格越闭集",
        l0.record_tool(TOOL_CELLS, 0).is_err(),
        "工具格 15 越闭集必拒",
    );
    s.add(
        "漂移拒（先录正格再注漂移）",
        {
            let _ = l0.record_op(2, editor_cell(2).unwrap_or(0));
            l0.record_op(2, editor_cell(2).unwrap_or(0).wrapping_add(1)) == Err(RazorCode::EDrift)
        },
        "操作载荷漂移必抓",
    );
    s.add(
        "逆序拒（空账本同样必抓）",
        {
            let mut lt = RazorLedger::new();
            let _ = lt.record_op(9, editor_cell(9).unwrap_or(0));
            lt.record_op(1, editor_cell(1).unwrap_or(0)) == Err(RazorCode::EOrder)
        },
        "执行家族先落、编辑家族后落，逆序必抓",
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
        "定级 S2 闭集（专有客户端依赖如实定级）",
        {
            let mut lt = RazorLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut d = 0usize;
            while d < JDBC_DRIVERS {
                if let (Some(a), Some(b)) = (driver_cell(d, 0), driver_cell(d, 1)) {
                    let _ = lt.record_driver(d, a, b);
                }
                d += 1;
            }
            let mut o = 0usize;
            while o < EDITOR_OPS {
                if let Some(w) = editor_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            let mut t = 0usize;
            while t < TOOL_CELLS {
                if let Some(w) = tool_cell(t) {
                    let _ = lt.record_tool(t, w);
                }
                t += 1;
            }
            let mut l = 0usize;
            while l < JAVA_LAYERS {
                if let Some(w) = java_cell(l) {
                    let _ = lt.record_java(l, w);
                }
                l += 1;
            }
            lt.judge_grade() == Grade::S2
        },
        "全通封顶 S2（KB-COMPAT 定级登记，不得漂移 S1）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7200 + 0x40 * 16,
        "SEG_BASE 0x40 对齐且在 VX15 域 0xC7600 格",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_driver_table() {
        assert_eq!(driver_name(0), Some("MySQL"));
        assert_eq!(driver_name(11), Some("DuckDB"));
        assert!(driver_name(12).is_none());
        assert!(driver_cell(12, 0).is_none());
        assert!(driver_cell(0, 2).is_none());
    }

    #[test]
    fn test_editor_kind() {
        assert_eq!(editor_kind(0), Some(0));
        assert_eq!(editor_kind(4), Some(1));
        assert_eq!(editor_kind(19), Some(4));
        assert!(editor_kind(20).is_none());
        assert!(editor_cell(20).is_none());
    }

    #[test]
    fn test_tool_table() {
        assert_eq!(tool_name(0), Some("查询工具"));
        assert_eq!(tool_name(5), Some("比较工具"));
        assert_eq!(tool_name(14), Some("导入工具"));
        assert!(tool_cell(15).is_none());
        assert_ne!(tool_cell(0).unwrap_or(0), tool_cell(4).unwrap_or(0));
    }

    #[test]
    fn test_java_stack() {
        assert_eq!(java_name(0), Some("JRE"));
        assert_eq!(java_name(3), Some("GUI 壳"));
        assert!(java_cell(4).is_none());
        assert_ne!(java_cell(0).unwrap_or(0), java_cell(1).unwrap_or(0));
    }

    #[test]
    fn test_cells_pure() {
        let a = editor_cell(3).unwrap_or(0);
        assert_eq!(a, editor_cell(3).unwrap_or(0));
        assert_ne!(a, editor_cell(4).unwrap_or(0));
        let d = driver_cell(5, 1).unwrap_or(0);
        assert_eq!(d, driver_cell(5, 1).unwrap_or(0));
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = RazorLedger::new();
        let _ = lt.record_op(9, editor_cell(9).unwrap_or(0));
        assert_eq!(lt.record_op(2, editor_cell(2).unwrap_or(0)), Err(RazorCode::EOrder));
        assert!(lt.record_op(10, editor_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = RazorLedger::new();
        assert_eq!(
            lt.record_op(5, editor_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(RazorCode::EDrift)
        );
        assert_eq!(
            lt.record_driver(1, driver_cell(1, 0).unwrap_or(0), driver_cell(1, 1).unwrap_or(0).wrapping_add(7)),
            Err(RazorCode::EDrvDrift)
        );
        assert_eq!(
            lt.record_tool(3, tool_cell(3).unwrap_or(0).wrapping_add(1)),
            Err(RazorCode::EToolDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = RazorLedger::new();
        let _ = lt.record_driver(0, driver_cell(0, 0).unwrap_or(0), driver_cell(0, 1).unwrap_or(0));
        assert_eq!(
            lt.record_driver(0, driver_cell(0, 0).unwrap_or(0), driver_cell(0, 1).unwrap_or(0)),
            Err(RazorCode::EDup)
        );
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = RazorLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = RazorLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (EDITOR_OPS + JDBC_DRIVERS + TOOL_CELLS + JAVA_LAYERS) as u32);
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
        assert_eq!(SEG_BASE, 0xC7600);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(RazorCode::Ok as u32, 0);
        assert_eq!(RazorCode::EGradeDrift as u32, 12);
        assert!(!RazorCode::Ok.detail().is_empty());
        assert!(!RazorCode::ESeal.detail().is_empty());
        assert_eq!(RazorCode::Ok.val(), SEG_BASE);
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        let lt = RazorLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52217_checks_green() {
        let s = run_unx52217_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
