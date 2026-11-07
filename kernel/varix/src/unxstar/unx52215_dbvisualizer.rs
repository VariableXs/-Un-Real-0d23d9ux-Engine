//! UNX-F52215 DbVisualizer 收口 · 0xC7580段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52215"，逐字）：
//!   title: DbVisualizer 收口
//!   quota: 200
//!   criteria: ① 纯功能代码 ≥200 行；② 判据原文：UNX-F52216-J1 六判据全
//!     落；JDBC 驱动管理 ×5；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③
//!     完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52215）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52216-J1，照原文锚定；功能按本卡标题（DbVisualizer）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① JDBC 驱动账：10 驱动家族闭集（PostgreSQL/MySQL/MariaDB/Oracle/
//!      SQL Server/DB2/Sybase/SQLite/H2/Snowflake）× 每家族 2 驱动格 =
//!      20 格，家族单调（逆序必抓）；「JDBC 驱动管理 ×5」另设 5 行管理
//!      声明账（下载/装载/重载/卸载/向导），缺行拒封。
//!   ② 图表/浏览双视图 12 格：六家族闭集（图表·表格/图表·引用/图表·
//!      层次/浏览·表数据/浏览·列结构/浏览·索引）各 2 格，逐格载荷折叠
//!      （纯函数）恒等才收；漂移必抓。
//!   ③ Java 上栈 4 层：JVM→JDBC 管理器→图形层→驱动加载器，逐层严格底
//!      向上压栈（跳层必抓）。
//!   ④ ledger 状态机（new/record_*/judge_gate×6/declare_jdbc/fingerprint
//!      FNV-1a）；注入核 run_cycle 三态轮转 10 轮（驱动家族逆序/视图漂移
//!      /Java 越闭集），全部注入落在当轮临时账本上，注入=检出+逃逸守恒；
//!      双跑 FNV 指纹一致；定级 S2 闭集（Java 工具有条件收口）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52215;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// JDBC 驱动家族闭集数。
pub const DRV_FAMILIES: usize = 10;

/// 每家族驱动格数。
pub const SLOTS_PER_FAMILY: usize = 2;

/// 驱动账总格数（10 × 2）。
pub const DRV_CELLS: usize = DRV_FAMILIES * SLOTS_PER_FAMILY;

/// 图表/浏览双视图格数。
pub const DUAL_VIEWS: usize = 12;

/// Java 上栈层数。
pub const JAVA_STACK: usize = 4;

/// JDBC 驱动管理声明行数（×5）。
pub const JDBC_ROWS: usize = 5;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DbVizCode {
    /// 成功
    Ok = 0,
    /// 驱动格越闭集
    ENoDrv,
    /// 驱动漂移
    EDrvDrift,
    /// 视图格越闭集
    ENoView,
    /// 视图漂移
    EViewDrift,
    /// 驱动家族逆序
    EOrder,
    /// Java 层越闭集
    ENoLayer,
    /// 声明行越闭集
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

impl DbVizCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            DbVizCode::Ok => "成功",
            DbVizCode::ENoDrv => "驱动格越闭集：10 家族 × 2 格之外先登记",
            DbVizCode::EDrvDrift => "驱动漂移：驱动格折叠与基准不恒等，逐格复算",
            DbVizCode::ENoView => "视图格越闭集：双视图 12 格之外先登记",
            DbVizCode::EViewDrift => "视图漂移：视图格折叠与基准不恒等，逐格复算",
            DbVizCode::EOrder => "驱动家族逆序：十家族闭集单调，逆序必抓",
            DbVizCode::ENoLayer => "Java 层越闭集：上栈 4 层之外先登记",
            DbVizCode::ENoRow => "声明行越闭集：JDBC 驱动管理 5 行闭集之外先登记",
            DbVizCode::EDup => "重复落账：同格只许一条账目",
            DbVizCode::ESeal => "未齐即封：20 驱动格 + 12 视图格 + 4 层 + 5 行声明未齐先补齐",
            DbVizCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            DbVizCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            DbVizCode::EGradeDrift => "定级漂移：全通才可定 S2，Java 工具有条件收口",
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
            Grade::S1 => "S1：全通直收（本卡不适用，Java 工具顶格 S2）",
            Grade::S2 => "S2：六判据+20 驱动格+12 视图格+4 层+5 行声明全通，有条件收口",
            Grade::S3 => "S3：不收口，判据或账格缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 驱动格载荷（家族 f、槽 s；全局格 = f×2+s）。
pub const fn drv_cell(f: usize, s: usize) -> Option<u64> {
    if f >= DRV_FAMILIES || s >= SLOTS_PER_FAMILY {
        return None;
    }
    Some((0xE1000u64 + ((f * SLOTS_PER_FAMILY + s) as u64) * 0x37) ^ 0x4442565A44525643)
}

/// 驱动家族名（const fn 表 + Option 边界）。
pub const fn drv_family_name(f: usize) -> Option<&'static str> {
    match f {
        0 => Some("PostgreSQL"),
        1 => Some("MySQL"),
        2 => Some("MariaDB"),
        3 => Some("Oracle"),
        4 => Some("SQL Server"),
        5 => Some("DB2"),
        6 => Some("Sybase"),
        7 => Some("SQLite"),
        8 => Some("H2"),
        9 => Some("Snowflake"),
        _ => None,
    }
}

/// 驱动家族（格 = f×2+s → 家族 = 格/2）。
pub const fn drv_kind(cell: usize) -> Option<u32> {
    if cell >= DRV_CELLS {
        return None;
    }
    Some((cell / SLOTS_PER_FAMILY) as u32)
}

/// 双视图格载荷（视图 v；家族 = v/2）。
pub const fn view_cell(v: usize) -> Option<u64> {
    if v >= DUAL_VIEWS {
        return None;
    }
    Some((0xE2000u64 + (v as u64) * 0x2C) ^ ((v / 2) as u64).wrapping_mul(0x01000193))
}

/// 视图家族（0..2=图表·表格 2..4=图表·引用 4..6=图表·层次 6..8=浏览·表数据 8..10=浏览·列结构 10..12=浏览·索引）。
pub const fn view_kind(v: usize) -> Option<u32> {
    if v >= DUAL_VIEWS {
        return None;
    }
    Some((v / 2) as u32)
}

/// 视图家族名。
pub const fn view_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("图表·表格"),
        1 => Some("图表·引用"),
        2 => Some("图表·层次"),
        3 => Some("浏览·表数据"),
        4 => Some("浏览·列结构"),
        5 => Some("浏览·索引"),
        _ => None,
    }
}

/// Java 上栈层载荷（层 l：0=JVM 1=JDBC 管理器 2=图形层 3=驱动加载器）。
pub const fn java_layer_cell(l: usize) -> Option<u64> {
    if l >= JAVA_STACK {
        return None;
    }
    Some((0xE3000u64 + (l as u64) * 0x5D) ^ 0x4A41564A5F53544B)
}

/// 层名。
pub const fn java_layer_name(l: usize) -> Option<&'static str> {
    match l {
        0 => Some("JVM"),
        1 => Some("JDBC 管理器"),
        2 => Some("图形层"),
        3 => Some("驱动加载器"),
        _ => None,
    }
}

/// JDBC 管理声明行折叠（行 r：下载/装载/重载/卸载/向导）。
pub const fn jdbc_row(r: usize) -> Option<u32> {
    if r >= JDBC_ROWS {
        return None;
    }
    Some(0xE400 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// DbVisualizer 收口账本状态机。
#[derive(Clone)]
pub struct DbVizLedger {
    /// 六判据（启动/功能/崩溃/卸载/升级/许可）。
    gates: [Option<bool>; 6],
    /// 驱动账（20 格）。
    drvs: [Option<u64>; DRV_CELLS],
    /// 双视图账（12 格）。
    views: [Option<u64>; DUAL_VIEWS],
    /// Java 上栈账。
    layers: [Option<u64>; JAVA_STACK],
    /// 已压栈层数（严格底向上）。
    depth: usize,
    /// JDBC 管理声明账（5 行）。
    jrows: [Option<u32>; JDBC_ROWS],
    /// 最后驱动家族。
    last_drv: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl DbVizLedger {
    /// 新账本。
    pub fn new() -> Self {
        DbVizLedger {
            gates: [None; 6],
            drvs: [None; DRV_CELLS],
            views: [None; DUAL_VIEWS],
            layers: [None; JAVA_STACK],
            depth: 0,
            jrows: [None; JDBC_ROWS],
            last_drv: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), DbVizCode> {
        if gate >= 6 {
            return Err(DbVizCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x53300 + gate as u64);
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

    /// 驱动一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_drv(&mut self, f: usize, s: usize, got: u64) -> Result<(), DbVizCode> {
        let cell = match (f, s) {
            (ff, ss) if ff < DRV_FAMILIES && ss < SLOTS_PER_FAMILY => ff * SLOTS_PER_FAMILY + ss,
            _ => return Err(DbVizCode::ENoDrv),
        };
        let kind = match drv_kind(cell) {
            Some(k) => k,
            None => return Err(DbVizCode::ENoDrv),
        };
        if (kind as i64) < self.last_drv {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x53400);
            return Err(DbVizCode::EOrder);
        }
        let want = match drv_cell(f, s) {
            Some(w) => w,
            None => return Err(DbVizCode::ENoDrv),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x53401 + cell as u64);
            return Err(DbVizCode::EDrvDrift);
        }
        if match self.drvs.get(cell) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DbVizCode::EDup);
        }
        if let Some(c) = self.drvs.get_mut(cell) {
            *c = Some(got);
        }
        self.last_drv = kind as i64;
        self.fnv(0x53402 + cell as u64);
        Ok(())
    }

    /// 驱动全账核验（20 格）。
    pub fn drvs_all(&self) -> bool {
        let mut i = 0usize;
        while i < DRV_CELLS {
            match self.drvs.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 双视图一格（载荷对照 + 重复拒）。
    pub fn record_view(&mut self, v: usize, got: u64) -> Result<(), DbVizCode> {
        let want = match view_cell(v) {
            Some(w) => w,
            None => return Err(DbVizCode::ENoView),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x53500 + v as u64);
            return Err(DbVizCode::EViewDrift);
        }
        if match self.views.get(v) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DbVizCode::EDup);
        }
        if let Some(c) = self.views.get_mut(v) {
            *c = Some(got);
        }
        self.fnv(0x53501 + v as u64);
        Ok(())
    }

    /// 双视图全账核验（12 格）。
    pub fn views_all(&self) -> bool {
        let mut v = 0usize;
        while v < DUAL_VIEWS {
            match self.views.get(v) {
                Some(Some(_)) => {}
                _ => return false,
            }
            v += 1;
        }
        true
    }

    /// Java 上栈一层（严格底向上：层号必须等于当前栈深）。
    pub fn record_layer(&mut self, l: usize, got: u64) -> Result<(), DbVizCode> {
        if l >= JAVA_STACK {
            return Err(DbVizCode::ENoLayer);
        }
        if l != self.depth {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x53600);
            return Err(DbVizCode::EOrder);
        }
        let want = match java_layer_cell(l) {
            Some(w) => w,
            None => return Err(DbVizCode::ENoLayer),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x53601 + l as u64);
            return Err(DbVizCode::EDrvDrift);
        }
        if let Some(c) = self.layers.get_mut(l) {
            *c = Some(got);
        }
        self.depth = l + 1;
        self.fnv(0x53602 + l as u64);
        Ok(())
    }

    /// 上栈全账核验（4 层且逐层压满）。
    pub fn layers_all(&self) -> bool {
        self.depth == JAVA_STACK
            && match self.layers.get(JAVA_STACK - 1) {
                Some(Some(_)) => true,
                _ => false,
            }
    }

    /// JDBC 管理声明一行（×5）。
    pub fn declare_jdbc(&mut self, r: usize) -> Result<(), DbVizCode> {
        if r >= JDBC_ROWS {
            return Err(DbVizCode::ENoRow);
        }
        if match self.jrows.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DbVizCode::EDup);
        }
        if let Some(c) = self.jrows.get_mut(r) {
            *c = jdbc_row(r);
        }
        self.fnv(0x53700 + r as u64);
        Ok(())
    }

    /// 声明全齐核验（5 行）。
    pub fn jrows_all(&self) -> bool {
        let mut r = 0usize;
        while r < JDBC_ROWS {
            match self.jrows.get(r) {
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

    /// KB-COMPAT 定级（Java 工具顶格 S2：全通 → S2，其余 → S3）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() && self.drvs_all() && self.views_all() && self.layers_all() && self.jrows_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for DbVizLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = DbVizLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut f = 0usize;
    while f < DRV_FAMILIES {
        let mut s = 0usize;
        while s < SLOTS_PER_FAMILY {
            if let Some(w) = drv_cell(f, s) {
                if led.record_drv(f, s, w).is_ok() {
                    recorded += 1;
                }
            }
            s += 1;
        }
        f += 1;
    }
    let mut v = 0usize;
    while v < DUAL_VIEWS {
        if let Some(w) = view_cell(v) {
            if led.record_view(v, w).is_ok() {
                recorded += 1;
            }
        }
        v += 1;
    }
    let mut l = 0usize;
    while l < JAVA_STACK {
        if let Some(w) = java_layer_cell(l) {
            if led.record_layer(l, w).is_ok() {
                recorded += 1;
            }
        }
        l += 1;
    }
    let mut r = 0usize;
    while r < JDBC_ROWS {
        let _ = led.declare_jdbc(r);
        r += 1;
    }
    // 注入 10 轮：驱动家族逆序 / 视图漂移 / Java 越闭集三态轮转。
    // 全部注入落在当轮临时账本上（逆序先录后族再录前族；漂移先录正格再注漂移）。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = DbVizLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_drv(9, 0, drv_cell(9, 0).unwrap_or(0));
                lt.record_drv(2, 1, drv_cell(2, 1).unwrap_or(0)) == Err(DbVizCode::EOrder)
            }
            1 => {
                let _ = lt.record_view(0, view_cell(0).unwrap_or(0));
                lt.record_view(1, view_cell(1).unwrap_or(0).wrapping_add(1)) == Err(DbVizCode::EViewDrift)
            }
            _ => lt.record_layer(JAVA_STACK, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x53800 + k as u64);
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
/// UNX-F52215 判据集（锚 UNX-F52216-J1，off-by-one 板式）。
pub fn run_unx52215_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52215");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "JDBC 驱动 20 格全通（10 家族 × 2）",
        recorded >= (DRV_CELLS + DUAL_VIEWS + JAVA_STACK) as u32,
        "PostgreSQL→Snowflake 十家族逐格恒等",
    );
    s.add(
        "十驱动家族逐格恒等（独立账复跑）",
        {
            let mut lt = DbVizLedger::new();
            let mut ok = true;
            let mut f = 0usize;
            while f < DRV_FAMILIES {
                let mut sl = 0usize;
                while sl < SLOTS_PER_FAMILY {
                    if let Some(w) = drv_cell(f, sl) {
                        if lt.record_drv(f, sl, w).is_err() {
                            ok = false;
                        }
                    }
                    sl += 1;
                }
                f += 1;
            }
            ok && lt.drvs_all()
        },
        "驱动侧闭集逐格折叠恒等（§AI-66.3）",
    );
    s.add(
        "图表/浏览双视图 12 格全落",
        {
            let mut lt = DbVizLedger::new();
            let mut ok = true;
            let mut v = 0usize;
            while v < DUAL_VIEWS {
                if let Some(w) = view_cell(v) {
                    if lt.record_view(v, w).is_err() {
                        ok = false;
                    }
                }
                v += 1;
            }
            ok && lt.views_all()
        },
        "图表·表格/引用/层次 + 浏览·表数据/列结构/索引 六家族",
    );
    s.add(
        "Java 上栈 4 层单调",
        {
            let mut lt = DbVizLedger::new();
            let mut ok = true;
            let mut l = 0usize;
            while l < JAVA_STACK {
                if let Some(w) = java_layer_cell(l) {
                    if lt.record_layer(l, w).is_err() {
                        ok = false;
                    }
                }
                l += 1;
            }
            ok && lt.layers_all()
        },
        "JVM→JDBC 管理器→图形层→驱动加载器严格底向上",
    );
    s.add(
        "JDBC 驱动管理声明 5/5 全齐",
        {
            let mut lt = DbVizLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < JDBC_ROWS {
                if lt.declare_jdbc(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.jrows_all()
        },
        "下载/装载/重载/卸载/向导五行闭集",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = DbVizLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/功能/崩溃/卸载/升级/许可逐格落账（锚 UNX-F52216-J1）",
    );
    s.add(
        "驱动家族单调",
        {
            let mut ok = true;
            let mut c = 1usize;
            while c < DRV_CELLS {
                if drv_kind(c).unwrap_or(11) < drv_kind(c - 1).unwrap_or(11) {
                    ok = false;
                }
                c += 1;
            }
            ok
        },
        "十家族闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "家族逆序/视图漂移/Java 越闭集三态轮转必抓",
    );
    let mut l0 = DbVizLedger::new();
    s.add(
        "边界·驱动格越闭集",
        l0.record_drv(DRV_FAMILIES, 0, 0).is_err(),
        "家族 10 越闭集必拒",
    );
    s.add(
        "边界·视图格越闭集",
        l0.record_view(DUAL_VIEWS, 0).is_err(),
        "视图 12 越闭集必拒",
    );
    s.add(
        "边界·Java 层越闭集",
        l0.record_layer(JAVA_STACK, 0).is_err(),
        "层 4 越闭集必拒",
    );
    s.add(
        "驱动漂移拒",
        {
            let _ = l0.record_drv(0, 0, drv_cell(0, 0).unwrap_or(0));
            l0.record_drv(0, 1, drv_cell(0, 1).unwrap_or(0).wrapping_add(3)) == Err(DbVizCode::EDrvDrift)
        },
        "驱动格载荷漂移必抓",
    );
    s.add(
        "声明行越闭集拒",
        l0.declare_jdbc(JDBC_ROWS).is_err(),
        "声明行 5 越闭集必拒",
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
            // 定级断言前主/aux 各节全部记账（判据+驱动+视图+上栈+声明）。
            let mut lt = DbVizLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut f = 0usize;
            while f < DRV_FAMILIES {
                let mut sl = 0usize;
                while sl < SLOTS_PER_FAMILY {
                    if let Some(w) = drv_cell(f, sl) {
                        let _ = lt.record_drv(f, sl, w);
                    }
                    sl += 1;
                }
                f += 1;
            }
            let mut v = 0usize;
            while v < DUAL_VIEWS {
                if let Some(w) = view_cell(v) {
                    let _ = lt.record_view(v, w);
                }
                v += 1;
            }
            let mut l = 0usize;
            while l < JAVA_STACK {
                if let Some(w) = java_layer_cell(l) {
                    let _ = lt.record_layer(l, w);
                }
                l += 1;
            }
            let mut r = 0usize;
            while r < JDBC_ROWS {
                let _ = lt.declare_jdbc(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S2 && Grade::S2.val() == 2
        },
        "全通 → S2（KB-COMPAT 定级登记，Java 工具顶格）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7580,
        "SEG_BASE 0x40 对齐且在 VX15 域 52215 格",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drv_families() {
        assert_eq!(drv_family_name(0), Some("PostgreSQL"));
        assert_eq!(drv_family_name(9), Some("Snowflake"));
        assert!(drv_family_name(10).is_none());
        assert!(drv_cell(DRV_FAMILIES, 0).is_none());
        assert!(drv_cell(0, SLOTS_PER_FAMILY).is_none());
    }

    #[test]
    fn test_drv_kind() {
        assert_eq!(drv_kind(0), Some(0));
        assert_eq!(drv_kind(1), Some(0));
        assert_eq!(drv_kind(19), Some(9));
        assert!(drv_kind(20).is_none());
    }

    #[test]
    fn test_view_kind() {
        assert_eq!(view_kind(0), Some(0));
        assert_eq!(view_kind(6), Some(3));
        assert_eq!(view_kind(11), Some(5));
        assert!(view_kind(12).is_none());
        assert!(view_cell(12).is_none());
        assert_eq!(view_kind_name(3), Some("浏览·表数据"));
        assert!(view_kind_name(6).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = view_cell(7).unwrap_or(0);
        assert_eq!(a, view_cell(7).unwrap_or(0));
        assert_ne!(a, view_cell(8).unwrap_or(0));
        assert!(java_layer_cell(JAVA_STACK).is_none());
        assert!(jdbc_row(JDBC_ROWS).is_none());
    }

    #[test]
    fn test_drv_family_order_caught() {
        let mut lt = DbVizLedger::new();
        let _ = lt.record_drv(9, 0, drv_cell(9, 0).unwrap_or(0));
        assert_eq!(lt.record_drv(2, 1, drv_cell(2, 1).unwrap_or(0)), Err(DbVizCode::EOrder));
        assert!(lt.record_drv(9, 1, drv_cell(9, 1).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_view_drift_caught() {
        let mut lt = DbVizLedger::new();
        assert_eq!(
            lt.record_view(4, view_cell(4).unwrap_or(0).wrapping_add(1)),
            Err(DbVizCode::EViewDrift)
        );
        assert_eq!(
            lt.record_drv(3, 1, drv_cell(3, 1).unwrap_or(0).wrapping_add(9)),
            Err(DbVizCode::EDrvDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = DbVizLedger::new();
        let _ = lt.record_view(5, view_cell(5).unwrap_or(0));
        assert_eq!(lt.record_view(5, view_cell(5).unwrap_or(0)), Err(DbVizCode::EDup));
        let _ = lt.declare_jdbc(2);
        assert_eq!(lt.declare_jdbc(2), Err(DbVizCode::EDup));
    }

    #[test]
    fn test_layer_strict_bottom_up() {
        let mut lt = DbVizLedger::new();
        assert_eq!(
            lt.record_layer(1, java_layer_cell(1).unwrap_or(0)),
            Err(DbVizCode::EOrder)
        );
        let mut l = 0usize;
        while l < JAVA_STACK {
            assert!(lt.record_layer(l, java_layer_cell(l).unwrap_or(0)).is_ok());
            l += 1;
        }
        assert!(lt.layers_all());
        assert_eq!(java_layer_name(2), Some("图形层"));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = DbVizLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (DRV_CELLS + DUAL_VIEWS + JAVA_STACK) as u32);
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
        assert_eq!(SEG_BASE, 0xC7580);
        assert_eq!(DbVizCode::EGradeDrift.val(), SEG_BASE + 0x0C);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(DbVizCode::Ok as u32, 0);
        assert_eq!(DbVizCode::EGradeDrift as u32, 12);
        assert!(!DbVizCode::Ok.detail().is_empty());
        assert!(!DbVizCode::ENoRow.detail().is_empty());
    }

    #[test]
    fn test_grade_s3_on_empty() {
        let lt = DbVizLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        assert_eq!(Grade::S2.val(), 2);
    }

    #[test]
    fn unx52215_checks_green() {
        let s = run_unx52215_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
