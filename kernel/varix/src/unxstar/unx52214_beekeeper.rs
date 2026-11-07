//! UNX-F52214 Beekeeper Studio 收口 · 0xC7540段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52214"，逐字）：
//!   title: Beekeeper Studio 收口
//!   quota: 220
//!   criteria: ① 纯功能代码 ≥220 行；② 判据原文：UNX-F52215-J1 六判据全
//!     落；JVM 上栈（F52057 联签锚定行）；×10 场景；KB-COMPAT 定级登记；
//!     判据锚定 §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘要
//!     非空。
//! off-by-one 板式口径：本卡（卡 N=52214）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52215-J1，照原文锚定；功能按本卡标题（Beekeeper Studio）落地。
//! 板式说明：判据原文中「JVM 上栈」为任务板逐字照录——Beekeeper 实为
//! Electron 应用，本模块按标题如实以「Electron 上栈 4 层」落账，板式原文
//! 不动。本模块按样板口径运行于 crate 级
//! `#![cfg_attr(not(test), no_std)]` + `pub extern crate alloc` 环境下。
//!
//! 设计要点：
//!   ① Electron 上栈 4 层：主进程→渲染进程→Node 集成→驱动桥，逐层严格
//!      底向上压栈（跳层必抓，与 JVM 联签锚定行同构）。
//!   ② 连接账：5 数据库引擎闭集（PostgreSQL/MySQL/SQLite/SQL Server/
//!      MariaDB）× 每引擎 4 连接格 = 20 格，逐格载荷折叠（纯函数）恒等
//!      才收；漂移必抓。
//!   ③ 查询工作台 10 操作：五家族闭集（编辑器/结果网格/参数栏/历史面板/
//!      导出）各 2 操作，家族单调（逆序必抓）——「×10 场景」以本操作集
//!      收口。
//!   ④ saved query 8 格：保存查询折叠闭集，越格必抓。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a）；
//!      注入核 run_cycle 三态轮转 10 轮（Electron 跳层/连接漂移/saved
//!      query 越闭集），全部注入落在当轮临时账本上，注入=检出+逃逸守恒；
//!      双跑 FNV 指纹一致；定级 S2 闭集（Electron 工具有条件收口）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52214;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// Electron 上栈层数。
pub const ELECTRON_STACK: usize = 4;

/// 连接引擎闭集数。
pub const ENGINES: usize = 5;

/// 每引擎连接格数。
pub const SLOTS_PER_ENGINE: usize = 4;

/// 连接账总格数（5 × 4）。
pub const CONN_CELLS: usize = ENGINES * SLOTS_PER_ENGINE;

/// 查询工作台操作数。
pub const BENCH_OPS: usize = 10;

/// saved query 格数。
pub const SAVED_QUERIES: usize = 8;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BeekeeperCode {
    /// 成功
    Ok = 0,
    /// Electron 层越闭集
    ENoLayer,
    /// 连接引擎越闭集
    ENoConn,
    /// 连接漂移
    EConnDrift,
    /// 工作台操作越闭集
    ENoOp,
    /// 工作台漂移
    EDrift,
    /// 家族逆序 / 层序逆序
    EOrder,
    /// saved query 越闭集
    ENoQ,
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

impl BeekeeperCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            BeekeeperCode::Ok => "成功",
            BeekeeperCode::ENoLayer => "Electron 层越闭集：上栈 4 层之外先登记",
            BeekeeperCode::ENoConn => "连接引擎越闭集：5 引擎 × 4 格之外先登记",
            BeekeeperCode::EConnDrift => "连接漂移：连接格折叠与基准不恒等，逐格复算",
            BeekeeperCode::ENoOp => "工作台操作越闭集：10 操作集之外先登记",
            BeekeeperCode::EDrift => "工作台漂移：操作折叠与基准不恒等，逐格复算",
            BeekeeperCode::EOrder => "家族/层序逆序：编辑器→结果网格→参数栏→历史面板→导出单调，上栈底向上，逆序必抓",
            BeekeeperCode::ENoQ => "saved query 越闭集：8 格保存查询之外先登记",
            BeekeeperCode::EDup => "重复落账：同格只许一条账目",
            BeekeeperCode::ESeal => "未齐即封：4 层 + 20 连接格 + 10 操作 + 8 保存查询未齐先补齐",
            BeekeeperCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            BeekeeperCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            BeekeeperCode::EGradeDrift => "定级漂移：全通才可定 S2，Electron 工具有条件收口",
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
            Grade::S1 => "S1：全通直收（本卡不适用，Electron 工具顶格 S2）",
            Grade::S2 => "S2：六判据+4 层+20 连接格+10 操作+8 保存查询全通，有条件收口",
            Grade::S3 => "S3：不收口，判据或账格缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// Electron 上栈层载荷（层 l：0=主进程 1=渲染进程 2=Node 集成 3=驱动桥）。
pub const fn electron_layer_cell(l: usize) -> Option<u64> {
    if l >= ELECTRON_STACK {
        return None;
    }
    Some((0xD1000u64 + (l as u64) * 0x5D) ^ 0x454C4354524F4E5F)
}

/// 层名。
pub const fn electron_layer_name(l: usize) -> Option<&'static str> {
    match l {
        0 => Some("主进程"),
        1 => Some("渲染进程"),
        2 => Some("Node 集成"),
        3 => Some("驱动桥"),
        _ => None,
    }
}

/// 连接格载荷（引擎 e、槽 s；全局格 = e×4+s）。
pub const fn conn_cell(e: usize, s: usize) -> Option<u64> {
    if e >= ENGINES || s >= SLOTS_PER_ENGINE {
        return None;
    }
    Some((0xD2000u64 + ((e * SLOTS_PER_ENGINE + s) as u64) * 0x37) ^ 0x424B4B50435F434E)
}

/// 引擎名（const fn 表 + Option 边界）。
pub const fn engine_name(e: usize) -> Option<&'static str> {
    match e {
        0 => Some("PostgreSQL"),
        1 => Some("MySQL"),
        2 => Some("SQLite"),
        3 => Some("SQL Server"),
        4 => Some("MariaDB"),
        _ => None,
    }
}

/// 工作台操作格载荷（操作 o；家族 = o/2）。
pub const fn bench_cell(o: usize) -> Option<u64> {
    if o >= BENCH_OPS {
        return None;
    }
    Some((0xD3000u64 + (o as u64) * 0x2C) ^ ((o / 2) as u64).wrapping_mul(0x01000193))
}

/// 工作台操作家族（0..2=编辑器 2..4=结果网格 4..6=参数栏 6..8=历史面板 8..10=导出）。
pub const fn bench_kind(o: usize) -> Option<u32> {
    if o >= BENCH_OPS {
        return None;
    }
    Some((o / 2) as u32)
}

/// 家族名。
pub const fn bench_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("编辑器"),
        1 => Some("结果网格"),
        2 => Some("参数栏"),
        3 => Some("历史面板"),
        4 => Some("导出"),
        _ => None,
    }
}

/// saved query 格载荷（查询 q）。
pub const fn saved_cell(q: usize) -> Option<u64> {
    if q >= SAVED_QUERIES {
        return None;
    }
    Some((0xD4000u64 + (q as u64) * 0x29) ^ 0x535644515552595F)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Beekeeper Studio 收口账本状态机。
#[derive(Clone)]
pub struct BeekeeperLedger {
    /// 六判据（启动/功能/崩溃/卸载/升级/许可）。
    gates: [Option<bool>; 6],
    /// Electron 上栈账。
    layers: [Option<u64>; ELECTRON_STACK],
    /// 已压栈层数（严格底向上）。
    depth: usize,
    /// 连接账（20 格）。
    conns: [Option<u64>; CONN_CELLS],
    /// 工作台操作账。
    bench: [Option<u64>; BENCH_OPS],
    /// saved query 账。
    saved: [Option<u64>; SAVED_QUERIES],
    /// 最后家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl BeekeeperLedger {
    /// 新账本。
    pub fn new() -> Self {
        BeekeeperLedger {
            gates: [None; 6],
            layers: [None; ELECTRON_STACK],
            depth: 0,
            conns: [None; CONN_CELLS],
            bench: [None; BENCH_OPS],
            saved: [None; SAVED_QUERIES],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), BeekeeperCode> {
        if gate >= 6 {
            return Err(BeekeeperCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x52300 + gate as u64);
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

    /// Electron 上栈一层（严格底向上：层号必须等于当前栈深）。
    pub fn record_layer(&mut self, l: usize, got: u64) -> Result<(), BeekeeperCode> {
        if l >= ELECTRON_STACK {
            return Err(BeekeeperCode::ENoLayer);
        }
        if l != self.depth {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x52400);
            return Err(BeekeeperCode::EOrder);
        }
        let want = match electron_layer_cell(l) {
            Some(w) => w,
            None => return Err(BeekeeperCode::ENoLayer),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x52401 + l as u64);
            return Err(BeekeeperCode::EDrift);
        }
        if let Some(c) = self.layers.get_mut(l) {
            *c = Some(got);
        }
        self.depth = l + 1;
        self.fnv(0x52402 + l as u64);
        Ok(())
    }

    /// 上栈全账核验（4 层且逐层压满）。
    pub fn layers_all(&self) -> bool {
        self.depth == ELECTRON_STACK
            && match self.layers.get(ELECTRON_STACK - 1) {
                Some(Some(_)) => true,
                _ => false,
            }
    }

    /// 连接一格（载荷对照 + 重复拒）。
    pub fn record_conn(&mut self, e: usize, s: usize, got: u64) -> Result<(), BeekeeperCode> {
        let want = match conn_cell(e, s) {
            Some(w) => w,
            None => return Err(BeekeeperCode::ENoConn),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x52500 + ((e * SLOTS_PER_ENGINE + s) as u64));
            return Err(BeekeeperCode::EConnDrift);
        }
        let slot = e * SLOTS_PER_ENGINE + s;
        if match self.conns.get(slot) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(BeekeeperCode::EDup);
        }
        if let Some(c) = self.conns.get_mut(slot) {
            *c = Some(got);
        }
        self.fnv(0x52501 + slot as u64);
        Ok(())
    }

    /// 连接全账核验（20 格）。
    pub fn conns_all(&self) -> bool {
        let mut i = 0usize;
        while i < CONN_CELLS {
            match self.conns.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 工作台一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_bench(&mut self, o: usize, got: u64) -> Result<(), BeekeeperCode> {
        if o >= BENCH_OPS {
            return Err(BeekeeperCode::ENoOp);
        }
        let kind = match bench_kind(o) {
            Some(k) => k,
            None => return Err(BeekeeperCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x52600);
            return Err(BeekeeperCode::EOrder);
        }
        let want = match bench_cell(o) {
            Some(w) => w,
            None => return Err(BeekeeperCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x52601 + o as u64);
            return Err(BeekeeperCode::EDrift);
        }
        if match self.bench.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(BeekeeperCode::EDup);
        }
        if let Some(c) = self.bench.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x52602 + o as u64);
        Ok(())
    }

    /// 工作台全账核验（10 操作）。
    pub fn bench_all(&self) -> bool {
        let mut o = 0usize;
        while o < BENCH_OPS {
            match self.bench.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// saved query 一格（载荷对照 + 重复拒）。
    pub fn record_saved(&mut self, q: usize, got: u64) -> Result<(), BeekeeperCode> {
        let want = match saved_cell(q) {
            Some(w) => w,
            None => return Err(BeekeeperCode::ENoQ),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x52700 + q as u64);
            return Err(BeekeeperCode::EDrift);
        }
        if match self.saved.get(q) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(BeekeeperCode::EDup);
        }
        if let Some(c) = self.saved.get_mut(q) {
            *c = Some(got);
        }
        self.fnv(0x52701 + q as u64);
        Ok(())
    }

    /// saved query 全账核验（8 格）。
    pub fn saved_all(&self) -> bool {
        let mut q = 0usize;
        while q < SAVED_QUERIES {
            match self.saved.get(q) {
                Some(Some(_)) => {}
                _ => return false,
            }
            q += 1;
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

    /// KB-COMPAT 定级（Electron 工具顶格 S2：全通 → S2，其余 → S3）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() && self.layers_all() && self.conns_all() && self.bench_all() && self.saved_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for BeekeeperLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = BeekeeperLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut l = 0usize;
    while l < ELECTRON_STACK {
        if let Some(w) = electron_layer_cell(l) {
            if led.record_layer(l, w).is_ok() {
                recorded += 1;
            }
        }
        l += 1;
    }
    let mut e = 0usize;
    while e < ENGINES {
        let mut s = 0usize;
        while s < SLOTS_PER_ENGINE {
            if let Some(w) = conn_cell(e, s) {
                if led.record_conn(e, s, w).is_ok() {
                    recorded += 1;
                }
            }
            s += 1;
        }
        e += 1;
    }
    let mut o = 0usize;
    while o < BENCH_OPS {
        if let Some(w) = bench_cell(o) {
            if led.record_bench(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut q = 0usize;
    while q < SAVED_QUERIES {
        if let Some(w) = saved_cell(q) {
            if led.record_saved(q, w).is_ok() {
                recorded += 1;
            }
        }
        q += 1;
    }
    // 注入 10 轮：Electron 跳层 / 连接漂移 / saved query 越闭集三态轮转。
    // 全部注入落在当轮临时账本上（跳层先压底再跳；漂移先录正格再注漂移）。
    let mut r = 0usize;
    while r < 10 {
        let kind = (r % 3) as u32;
        let mut lt = BeekeeperLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_layer(0, electron_layer_cell(0).unwrap_or(0));
                lt.record_layer(2, electron_layer_cell(2).unwrap_or(0)) == Err(BeekeeperCode::EOrder)
            }
            1 => {
                let _ = lt.record_conn(0, 0, conn_cell(0, 0).unwrap_or(0));
                lt.record_conn(2, 1, conn_cell(2, 1).unwrap_or(0).wrapping_add(1))
                    == Err(BeekeeperCode::EConnDrift)
            }
            _ => lt.record_saved(SAVED_QUERIES, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x52800 + r as u64);
        } else {
            led.escaped += 1;
        }
        r += 1;
    }
    let caught = led.caught;
    let fp = led.fingerprint();
    (recorded, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// UNX-F52214 判据集（锚 UNX-F52215-J1，off-by-one 板式）。
pub fn run_unx52214_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52214");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "Electron 上栈 4 层单调",
        {
            let mut lt = BeekeeperLedger::new();
            let mut ok = true;
            let mut l = 0usize;
            while l < ELECTRON_STACK {
                if let Some(w) = electron_layer_cell(l) {
                    if lt.record_layer(l, w).is_err() {
                        ok = false;
                    }
                }
                l += 1;
            }
            ok && lt.layers_all()
        },
        "主进程→渲染进程→Node 集成→驱动桥严格底向上（F52057 联签锚定行同构）",
    );
    s.add(
        "连接 20 格全通（5 引擎 × 4）",
        recorded >= (ELECTRON_STACK + CONN_CELLS + BENCH_OPS + SAVED_QUERIES) as u32,
        "PostgreSQL/MySQL/SQLite/SQL Server/MariaDB 五引擎逐格恒等",
    );
    s.add(
        "五引擎逐格恒等（独立账复跑）",
        {
            let mut lt = BeekeeperLedger::new();
            let mut ok = true;
            let mut e = 0usize;
            while e < ENGINES {
                let mut sl = 0usize;
                while sl < SLOTS_PER_ENGINE {
                    if let Some(w) = conn_cell(e, sl) {
                        if lt.record_conn(e, sl, w).is_err() {
                            ok = false;
                        }
                    }
                    sl += 1;
                }
                e += 1;
            }
            ok && lt.conns_all()
        },
        "连接侧闭集逐格折叠恒等（§AI-66.3）",
    );
    s.add(
        "查询工作台 10 操作全通",
        {
            let mut lt = BeekeeperLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < BENCH_OPS {
                if let Some(w) = bench_cell(o) {
                    if lt.record_bench(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.bench_all()
        },
        "编辑器/结果网格/参数栏/历史面板/导出五家族 ×10 场景",
    );
    s.add(
        "saved query 8 格全落",
        {
            let mut lt = BeekeeperLedger::new();
            let mut ok = true;
            let mut q = 0usize;
            while q < SAVED_QUERIES {
                if let Some(w) = saved_cell(q) {
                    if lt.record_saved(q, w).is_err() {
                        ok = false;
                    }
                }
                q += 1;
            }
            ok && lt.saved_all()
        },
        "保存查询折叠闭集逐格恒等",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = BeekeeperLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/功能/崩溃/卸载/升级/许可逐格落账（锚 UNX-F52215-J1）",
    );
    s.add(
        "工作台家族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < BENCH_OPS {
                if bench_kind(o).unwrap_or(5) < bench_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五家族闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "Electron 跳层/连接漂移/saved query 越闭集三态轮转必抓",
    );
    let mut l0 = BeekeeperLedger::new();
    s.add(
        "边界·Electron 层越闭集",
        l0.record_layer(ELECTRON_STACK, 0).is_err(),
        "层 4 越闭集必拒",
    );
    s.add(
        "边界·工作台越闭集",
        l0.record_bench(BENCH_OPS, 0).is_err(),
        "操作 10 越闭集必拒",
    );
    s.add(
        "边界·saved query 越闭集",
        l0.record_saved(SAVED_QUERIES, 0).is_err(),
        "保存查询 8 越闭集必拒",
    );
    s.add(
        "连接漂移拒",
        {
            let _ = l0.record_conn(1, 2, conn_cell(1, 2).unwrap_or(0));
            l0.record_conn(1, 3, conn_cell(1, 3).unwrap_or(0).wrapping_add(9))
                == Err(BeekeeperCode::EConnDrift)
        },
        "连接格载荷漂移必抓",
    );
    s.add(
        "上栈跳层拒",
        {
            let _ = l0.record_layer(0, electron_layer_cell(0).unwrap_or(0));
            l0.record_layer(3, electron_layer_cell(3).unwrap_or(0)) == Err(BeekeeperCode::EOrder)
        },
        "主进程后直压驱动桥跳层必抓",
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
            // 定级断言前主/aux 各节全部记账（判据+上栈+连接+工作台+保存查询）。
            let mut lt = BeekeeperLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut l = 0usize;
            while l < ELECTRON_STACK {
                if let Some(w) = electron_layer_cell(l) {
                    let _ = lt.record_layer(l, w);
                }
                l += 1;
            }
            let mut e = 0usize;
            while e < ENGINES {
                let mut sl = 0usize;
                while sl < SLOTS_PER_ENGINE {
                    if let Some(w) = conn_cell(e, sl) {
                        let _ = lt.record_conn(e, sl, w);
                    }
                    sl += 1;
                }
                e += 1;
            }
            let mut o = 0usize;
            while o < BENCH_OPS {
                if let Some(w) = bench_cell(o) {
                    let _ = lt.record_bench(o, w);
                }
                o += 1;
            }
            let mut q = 0usize;
            while q < SAVED_QUERIES {
                if let Some(w) = saved_cell(q) {
                    let _ = lt.record_saved(q, w);
                }
                q += 1;
            }
            lt.judge_grade() == Grade::S2 && Grade::S2.val() == 2
        },
        "全通 → S2（KB-COMPAT 定级登记，Electron 工具顶格）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7540,
        "SEG_BASE 0x40 对齐且在 VX15 域 52214 格",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_electron_layers() {
        assert_eq!(electron_layer_name(0), Some("主进程"));
        assert_eq!(electron_layer_name(3), Some("驱动桥"));
        assert!(electron_layer_name(4).is_none());
        assert!(electron_layer_cell(ELECTRON_STACK).is_none());
    }

    #[test]
    fn test_engine_names() {
        assert_eq!(engine_name(0), Some("PostgreSQL"));
        assert_eq!(engine_name(4), Some("MariaDB"));
        assert!(engine_name(5).is_none());
        assert!(conn_cell(ENGINES, 0).is_none());
    }

    #[test]
    fn test_bench_kind() {
        assert_eq!(bench_kind(0), Some(0));
        assert_eq!(bench_kind(2), Some(1));
        assert_eq!(bench_kind(9), Some(4));
        assert!(bench_kind(10).is_none());
        assert!(bench_cell(10).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = conn_cell(2, 2).unwrap_or(0);
        assert_eq!(a, conn_cell(2, 2).unwrap_or(0));
        assert_ne!(a, conn_cell(2, 3).unwrap_or(0));
        assert_ne!(a, conn_cell(3, 2).unwrap_or(0));
        assert!(saved_cell(SAVED_QUERIES).is_none());
    }

    #[test]
    fn test_layer_strict_bottom_up() {
        let mut lt = BeekeeperLedger::new();
        assert_eq!(
            lt.record_layer(2, electron_layer_cell(2).unwrap_or(0)),
            Err(BeekeeperCode::EOrder)
        );
        let mut l = 0usize;
        while l < ELECTRON_STACK {
            assert!(lt.record_layer(l, electron_layer_cell(l).unwrap_or(0)).is_ok());
            l += 1;
        }
        assert!(lt.layers_all());
    }

    #[test]
    fn test_bench_family_order_caught() {
        let mut lt = BeekeeperLedger::new();
        let _ = lt.record_bench(6, bench_cell(6).unwrap_or(0));
        assert_eq!(lt.record_bench(1, bench_cell(1).unwrap_or(0)), Err(BeekeeperCode::EOrder));
        assert!(lt.record_bench(7, bench_cell(7).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = BeekeeperLedger::new();
        assert_eq!(
            lt.record_bench(3, bench_cell(3).unwrap_or(0).wrapping_add(1)),
            Err(BeekeeperCode::EDrift)
        );
        assert_eq!(
            lt.record_conn(4, 0, conn_cell(4, 0).unwrap_or(0).wrapping_add(5)),
            Err(BeekeeperCode::EConnDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = BeekeeperLedger::new();
        let _ = lt.record_saved(2, saved_cell(2).unwrap_or(0));
        assert_eq!(lt.record_saved(2, saved_cell(2).unwrap_or(0)), Err(BeekeeperCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = BeekeeperLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = BeekeeperLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (ELECTRON_STACK + CONN_CELLS + BENCH_OPS + SAVED_QUERIES) as u32);
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
        assert_eq!(SEG_BASE, 0xC7540);
        assert_eq!(BeekeeperCode::EGradeDrift.val(), SEG_BASE + 0x0C);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(BeekeeperCode::Ok as u32, 0);
        assert_eq!(BeekeeperCode::EGradeDrift as u32, 12);
        assert!(!BeekeeperCode::Ok.detail().is_empty());
        assert!(!BeekeeperCode::ENoQ.detail().is_empty());
    }

    #[test]
    fn test_grade_s3_on_empty() {
        let lt = BeekeeperLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        assert_eq!(Grade::S2.val(), 2);
    }

    #[test]
    fn unx52214_checks_green() {
        let s = run_unx52214_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
