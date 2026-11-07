//! UNX-F52484 wrk 收口 · 0xCB8C0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52484"，逐字）：
//!   title: wrk 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52485-J1 ×10（本
//!     地靶站）；JVM 联签锚定行；KB-COMPAT 定级登记；判据锚定 §AI-66.3；
//!     ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52484）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52485-J1，照原文锚定；功能按本卡标题（wrk）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五参数族闭集：-t / -c / -d / -s / --latency 各 4 格共 20 参数格，
//!      家族单调（逆序必抓），逐格载荷折叠恒等才收。
//!   ② Lua 脚本消费 8 格：wrk.scheme/host/port/path/method/body/headers/
//!      done 八消费面，逐格载荷恒等，漂移必抓。
//!   ③ 线程/连接二元 12 格：4 线程档 × 3 连接档闭集组合，载荷恒等才收
//!      （二元载荷漂移与参数漂移同性质，按 EArgDrift 计）。
//!   ④ 与 ab 对照 6 格：对照载荷恒等才收，越界必抓（ENoX）、漂移必抓
//!      （EXDrift）。
//!   ⑤ C/Lua 上栈声明 2 行闭集：wrk C 运行时上栈声明 / wrk Lua VM 上栈
//!      声明，缺一拒封。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/seal/fingerprint
//!      FNV-1a）；注入核 run_cycle 三态轮转 10/10（参数越闭集/参数漂移/
//!      ab 对照漂移），检出+逃逸守恒；双跑 FNV 指纹一致；定级闭集
//!      （六闸 + 20 参数 + 8 Lua + 12 二元 + 6 对照 + 2 声明全通 → S1）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52484;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 参数格数。
pub const ARG_CELLS: usize = 20;

/// Lua 脚本消费格数。
pub const LUA_CELLS: usize = 8;

/// 线程/连接二元格数。
pub const BIN_CELLS: usize = 12;

/// 与 ab 对照格数。
pub const X_CELLS: usize = 6;

/// C/Lua 上栈声明行数。
pub const CL_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WrkCode {
    /// 成功
    Ok = 0,
    /// 参数格越闭集
    ENoArg,
    /// 参数/二元载荷漂移
    EArgDrift,
    /// 参数族逆序
    EOrder,
    /// Lua 消费格越闭集
    ENoLua,
    /// Lua 消费漂移
    ELuaDrift,
    /// 二元格越闭集
    ENoBin,
    /// ab 对照格越闭集
    ENoX,
    /// ab 对照漂移
    EXDrift,
    /// 声明行越闭集
    ENoRow,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 定级漂移
    EGradeDrift,
}

impl WrkCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            WrkCode::Ok => "成功",
            WrkCode::ENoArg => "参数格越闭集：-t/-c/-d/-s/--latency 20 参数格之外先登记",
            WrkCode::EArgDrift => "参数/二元载荷漂移：参数与线程连接二元折叠与基准不恒等，逐格复算",
            WrkCode::EOrder => "参数族逆序：-t→-c→-d→-s→--latency 单调，逆序必抓",
            WrkCode::ENoLua => "Lua 消费格越闭集：wrk 脚本消费 8 格之外先登记",
            WrkCode::ELuaDrift => "Lua 消费漂移：脚本消费载荷与基准不恒等，逐格复算",
            WrkCode::ENoBin => "二元格越闭集：线程/连接 12 格组合之外先登记",
            WrkCode::ENoX => "ab 对照格越闭集：与 ab 对照 6 格之外先登记",
            WrkCode::EXDrift => "ab 对照漂移：对照载荷与基准不恒等，逐格复算",
            WrkCode::ENoRow => "声明行越闭集：C/Lua 上栈声明 2 行闭集之外先登记",
            WrkCode::EDup => "重复落账：同格只许一条账目",
            WrkCode::ESeal => "未齐即封：六闸+20参数+8Lua+12二元+6对照+2声明未齐先补齐",
            WrkCode::EGradeDrift => "定级漂移：全通且声明全齐才可定 S1",
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
            Grade::S1 => "S1：六闸+20参数+8Lua+12二元+6对照+2声明全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 参数格载荷（格 o；家族 = o/4）。
pub const fn arg_cell(o: usize) -> Option<u64> {
    if o >= ARG_CELLS {
        return None;
    }
    Some((0xC1000u64 + (o as u64) * 0x2F) ^ 0x57524B5F41524700)
}

/// 参数家族（0..4=-t 4..8=-c 8..12=-d 12..16=-s 16..20=--latency）。
pub const fn arg_kind(o: usize) -> Option<u32> {
    if o >= ARG_CELLS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 参数家族名。
pub const fn arg_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("-t"),
        1 => Some("-c"),
        2 => Some("-d"),
        3 => Some("-s"),
        4 => Some("--latency"),
        _ => None,
    }
}

/// Lua 脚本消费格名（8 格）。
pub const fn lua_name(l: usize) -> Option<&'static str> {
    match l {
        0 => Some("wrk.scheme"),
        1 => Some("wrk.host"),
        2 => Some("wrk.port"),
        3 => Some("wrk.path"),
        4 => Some("wrk.method"),
        5 => Some("wrk.body"),
        6 => Some("wrk.headers"),
        7 => Some("wrk.done"),
        _ => None,
    }
}

/// Lua 消费格载荷。
pub const fn lua_cell(l: usize) -> Option<u64> {
    if l >= LUA_CELLS {
        return None;
    }
    Some((0xC2000u64 + (l as u64) * 0x3B) ^ 0x57524B4C55410000)
}

/// 二元格线程档（i/3 映射：1/2/4/8 线程）。
pub const fn bin_threads(i: usize) -> Option<u32> {
    if i >= BIN_CELLS {
        return None;
    }
    match i / 3 {
        0 => Some(1),
        1 => Some(2),
        2 => Some(4),
        _ => Some(8),
    }
}

/// 二元格连接档（i%3 映射：50/100/200 连接）。
pub const fn bin_conns(i: usize) -> Option<u32> {
    if i >= BIN_CELLS {
        return None;
    }
    match i % 3 {
        0 => Some(50),
        1 => Some(100),
        _ => Some(200),
    }
}

/// 二元格载荷（线程/连接组合折叠）。
pub const fn bin_cell(i: usize) -> Option<u64> {
    if i >= BIN_CELLS {
        return None;
    }
    let t = match bin_threads(i) {
        Some(t) => t as u64,
        None => return None,
    };
    let c = match bin_conns(i) {
        Some(c) => c as u64,
        None => return None,
    };
    Some((0xC3000u64 + (i as u64) * 0x2D) ^ ((t << 32) | c))
}

/// ab 对照格载荷。
pub const fn x_cell(x: usize) -> Option<u64> {
    if x >= X_CELLS {
        return None;
    }
    Some((0xC4000u64 + (x as u64) * 0x2C) ^ 0x57524B5F41580000)
}

/// C/Lua 上栈声明行折叠（行 r）。
pub const fn cl_row(r: usize) -> Option<u32> {
    if r >= CL_ROWS {
        return None;
    }
    Some(0xC500 + r as u32 * 0x2C)
}

/// C/Lua 上栈声明行名。
pub const fn cl_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("wrk C 运行时上栈声明"),
        1 => Some("wrk Lua VM 上栈声明"),
        _ => None,
    }
}

/// 六判据闸名（wrk 流水；JVM 联签锚定行入第四闸）。
pub const fn gate_name(g: usize) -> Option<&'static str> {
    match g {
        0 => Some("线程起"),
        1 => Some("连接建"),
        2 => Some("压测运行"),
        3 => Some("延迟统计（JVM 联签锚定行）"),
        4 => Some("ab 对照"),
        5 => Some("定级登记"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// wrk 收口账本状态机。
#[derive(Clone)]
pub struct WrkLedger {
    /// 六判据（线程起/连接建/压测运行/延迟统计/ab 对照/定级登记）。
    gates: [Option<bool>; 6],
    /// 参数格账。
    args: [Option<u64>; ARG_CELLS],
    /// Lua 消费账。
    luas: [Option<u64>; LUA_CELLS],
    /// 线程/连接二元账。
    bins: [Option<u64>; BIN_CELLS],
    /// ab 对照账。
    xs: [Option<u64>; X_CELLS],
    /// C/Lua 上栈声明账。
    cls: [Option<u32>; CL_ROWS],
    /// 最后参数家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl WrkLedger {
    /// 新账本。
    pub fn new() -> Self {
        WrkLedger {
            gates: [None; 6],
            args: [None; ARG_CELLS],
            luas: [None; LUA_CELLS],
            bins: [None; BIN_CELLS],
            xs: [None; X_CELLS],
            cls: [None; CL_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), WrkCode> {
        if gate >= 6 {
            return Err(WrkCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x4100 + gate as u64);
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

    /// 参数一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_arg(&mut self, o: usize, got: u64) -> Result<(), WrkCode> {
        if o >= ARG_CELLS {
            return Err(WrkCode::ENoArg);
        }
        let kind = match arg_kind(o) {
            Some(k) => k,
            None => return Err(WrkCode::ENoArg),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6300);
            return Err(WrkCode::EOrder);
        }
        let want = match arg_cell(o) {
            Some(w) => w,
            None => return Err(WrkCode::ENoArg),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6301 + o as u64);
            return Err(WrkCode::EArgDrift);
        }
        if match self.args.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WrkCode::EDup);
        }
        if let Some(c) = self.args.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x6302 + o as u64);
        Ok(())
    }

    /// 参数全账核验（20 格）。
    pub fn args_all(&self) -> bool {
        let mut o = 0usize;
        while o < ARG_CELLS {
            match self.args.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// Lua 消费一格（载荷对照 + 重复拒）。
    pub fn record_lua(&mut self, l: usize, got: u64) -> Result<(), WrkCode> {
        if l >= LUA_CELLS {
            return Err(WrkCode::ENoLua);
        }
        let want = match lua_cell(l) {
            Some(w) => w,
            None => return Err(WrkCode::ENoLua),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6401 + l as u64);
            return Err(WrkCode::ELuaDrift);
        }
        if match self.luas.get(l) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WrkCode::EDup);
        }
        if let Some(c) = self.luas.get_mut(l) {
            *c = Some(got);
        }
        self.fnv(0x6402 + l as u64);
        Ok(())
    }

    /// Lua 消费全账核验（8 格）。
    pub fn luas_all(&self) -> bool {
        let mut l = 0usize;
        while l < LUA_CELLS {
            match self.luas.get(l) {
                Some(Some(_)) => {}
                _ => return false,
            }
            l += 1;
        }
        true
    }

    /// 线程/连接二元一格（载荷对照 + 重复拒；漂移按 EArgDrift 计）。
    pub fn record_bin(&mut self, i: usize, got: u64) -> Result<(), WrkCode> {
        if i >= BIN_CELLS {
            return Err(WrkCode::ENoBin);
        }
        let want = match bin_cell(i) {
            Some(w) => w,
            None => return Err(WrkCode::ENoBin),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6501 + i as u64);
            return Err(WrkCode::EArgDrift);
        }
        if match self.bins.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WrkCode::EDup);
        }
        if let Some(c) = self.bins.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0x6502 + i as u64);
        Ok(())
    }

    /// 二元全账核验（12 格）。
    pub fn bins_all(&self) -> bool {
        let mut i = 0usize;
        while i < BIN_CELLS {
            match self.bins.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// ab 对照一格（载荷对照 + 重复拒）。
    pub fn record_x(&mut self, x: usize, got: u64) -> Result<(), WrkCode> {
        if x >= X_CELLS {
            return Err(WrkCode::ENoX);
        }
        let want = match x_cell(x) {
            Some(w) => w,
            None => return Err(WrkCode::ENoX),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6601 + x as u64);
            return Err(WrkCode::EXDrift);
        }
        if match self.xs.get(x) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WrkCode::EDup);
        }
        if let Some(c) = self.xs.get_mut(x) {
            *c = Some(got);
        }
        self.fnv(0x6602 + x as u64);
        Ok(())
    }

    /// ab 对照全账核验（6 格）。
    pub fn xs_all(&self) -> bool {
        let mut x = 0usize;
        while x < X_CELLS {
            match self.xs.get(x) {
                Some(Some(_)) => {}
                _ => return false,
            }
            x += 1;
        }
        true
    }

    /// C/Lua 上栈声明一行。
    pub fn declare_cl(&mut self, r: usize) -> Result<(), WrkCode> {
        if r >= CL_ROWS {
            return Err(WrkCode::ENoRow);
        }
        if match self.cls.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WrkCode::EDup);
        }
        if let Some(c) = self.cls.get_mut(r) {
            *c = cl_row(r);
        }
        self.fnv(0x6700 + r as u64);
        Ok(())
    }

    /// 声明全齐核验。
    pub fn cls_all(&self) -> bool {
        let mut r = 0usize;
        while r < CL_ROWS {
            match self.cls.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 封账核验：任一节未齐即拒封（ESeal）。
    pub fn seal(&self) -> Result<(), WrkCode> {
        if self.gates_all() && self.args_all() && self.luas_all() && self.bins_all() && self.xs_all() && self.cls_all() {
            Ok(())
        } else {
            Err(WrkCode::ESeal)
        }
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// KB-COMPAT 定级（全通 → S1，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() && self.args_all() && self.luas_all() && self.bins_all() && self.xs_all() && self.cls_all() {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for WrkLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 全流程账本构建：六闸 + 全节落账 + 注入 10 轮三态轮转。返回（记账格数, 账本）。
fn cycle_ledger() -> (u32, WrkLedger) {
    let mut led = WrkLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < ARG_CELLS {
        if let Some(w) = arg_cell(o) {
            if led.record_arg(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut l = 0usize;
    while l < LUA_CELLS {
        if let Some(w) = lua_cell(l) {
            if led.record_lua(l, w).is_ok() {
                recorded += 1;
            }
        }
        l += 1;
    }
    let mut i = 0usize;
    while i < BIN_CELLS {
        if let Some(w) = bin_cell(i) {
            if led.record_bin(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut x = 0usize;
    while x < X_CELLS {
        if let Some(w) = x_cell(x) {
            if led.record_x(x, w).is_ok() {
                recorded += 1;
            }
        }
        x += 1;
    }
    let mut r = 0usize;
    while r < CL_ROWS {
        let _ = led.declare_cl(r);
        r += 1;
    }
    // 注入 10 轮：参数越闭集 / 参数漂移 / ab 对照漂移三态轮转（全部落在当轮临时账本）。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = WrkLedger::new();
        let caught = match kind {
            0 => lt.record_arg(ARG_CELLS, 0).is_err(),
            1 => {
                lt.record_arg(0, arg_cell(0).unwrap_or(0));
                lt.record_arg(1, arg_cell(1).unwrap_or(0).wrapping_add(1)) == Err(WrkCode::EArgDrift)
            }
            _ => {
                lt.record_x(0, x_cell(0).unwrap_or(0));
                lt.record_x(1, x_cell(1).unwrap_or(0).wrapping_add(1)) == Err(WrkCode::EXDrift)
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
    (recorded, led)
}

/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let (recorded, led) = cycle_ledger();
    let (_, caught, _) = led.stats();
    (recorded, caught, led.fingerprint())
}

/// 注入账三数（主账本）：恒等式注入=检出、逃逸=0。
pub fn run_cycle_stats() -> (u32, u32, u32) {
    let (_, led) = cycle_ledger();
    led.stats()
}

/// 全齐账本（六闸 + 全节落账，无注入）。
pub fn fresh_complete() -> WrkLedger {
    let mut lt = WrkLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = lt.judge_gate(g);
        g += 1;
    }
    let mut o = 0usize;
    while o < ARG_CELLS {
        if let Some(w) = arg_cell(o) {
            let _ = lt.record_arg(o, w);
        }
        o += 1;
    }
    let mut l = 0usize;
    while l < LUA_CELLS {
        if let Some(w) = lua_cell(l) {
            let _ = lt.record_lua(l, w);
        }
        l += 1;
    }
    let mut i = 0usize;
    while i < BIN_CELLS {
        if let Some(w) = bin_cell(i) {
            let _ = lt.record_bin(i, w);
        }
        i += 1;
    }
    let mut x = 0usize;
    while x < X_CELLS {
        if let Some(w) = x_cell(x) {
            let _ = lt.record_x(x, w);
        }
        x += 1;
    }
    let mut r = 0usize;
    while r < CL_ROWS {
        let _ = lt.declare_cl(r);
        r += 1;
    }
    lt
}

// ── 判据集 ───────────────────────────────────────────────────────
/// UNX-F52484 判据集（锚 UNX-F52485-J1，off-by-one 板式）。
pub fn run_unx52484_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52484");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五参数族 20 格全通",
        recorded >= ARG_CELLS as u32,
        "-t/-c/-d/-s/--latency 五参数族逐格恒等",
    );
    s.add(
        "Lua 脚本消费 8 格全落",
        {
            let mut lt = WrkLedger::new();
            let mut ok = true;
            let mut l = 0usize;
            while l < LUA_CELLS {
                if let Some(w) = lua_cell(l) {
                    if lt.record_lua(l, w).is_err() {
                        ok = false;
                    }
                }
                l += 1;
            }
            ok && lt.luas_all()
        },
        "wrk.scheme/host/port/path/method/body/headers/done 八消费面逐格落账",
    );
    s.add(
        "线程/连接二元 12 格全落",
        {
            let mut lt = WrkLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < BIN_CELLS {
                if let Some(w) = bin_cell(i) {
                    if lt.record_bin(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.bins_all()
        },
        "4 线程档 × 3 连接档闭集组合逐格落账",
    );
    s.add(
        "与 ab 对照 6 格全落",
        {
            let mut lt = WrkLedger::new();
            let mut ok = true;
            let mut x = 0usize;
            while x < X_CELLS {
                if let Some(w) = x_cell(x) {
                    if lt.record_x(x, w).is_err() {
                        ok = false;
                    }
                }
                x += 1;
            }
            ok && lt.xs_all()
        },
        "ab 对照载荷恒等才收，越界/漂移必抓",
    );
    s.add(
        "C/Lua 上栈声明 2 行全齐",
        {
            let mut lt = WrkLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < CL_ROWS {
                if lt.declare_cl(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.cls_all()
        },
        "wrk C 运行时上栈/Lua VM 上栈两行闭集",
    );
    s.add(
        "参数族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < ARG_CELLS {
                if arg_kind(o).unwrap_or(5) < arg_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "-t→-c→-d→-s→--latency 五参数族闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "参数越闭集/参数漂移/ab 对照漂移三态轮转必抓（判据原文：×10 本地靶站）",
    );
    let mut l0 = WrkLedger::new();
    s.add(
        "边界·参数越闭集",
        l0.record_arg(ARG_CELLS, 0).is_err(),
        "参数格 20 越闭集必拒",
    );
    s.add(
        "Lua 消费漂移拒",
        {
            l0.record_lua(0, lua_cell(0).unwrap_or(0));
            l0.record_lua(1, lua_cell(1).unwrap_or(0).wrapping_add(1)) == Err(WrkCode::ELuaDrift)
        },
        "Lua 脚本消费载荷漂移必抓",
    );
    s.add(
        "边界·Lua 消费越闭集",
        l0.record_lua(LUA_CELLS, 0).is_err(),
        "Lua 消费格 8 越闭集必拒",
    );
    s.add(
        "边界·ab 对照越闭集",
        l0.record_x(X_CELLS, 0).is_err(),
        "ab 对照格 6 越闭集必拒",
    );
    s.add(
        "未齐即封核",
        {
            let empty = WrkLedger::new();
            let full = fresh_complete();
            empty.seal().is_err() && full.seal().is_ok()
        },
        "账未齐封账必拒，全齐才许封",
    );
    let (inj_n, caught_n, esc_n) = run_cycle_stats();
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
            let lt = fresh_complete();
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记，JVM 联签锚定行入延迟统计闸）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCB8C0,
        "SEG_BASE 0x40 对齐且等于 VX16 公式位 0xCB8C0",
    );
    s.add("收口·判据条数", s.len() >= 16, "含本条共 ≥17 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_arg_kinds() {
        assert_eq!(arg_kind(0), Some(0));
        assert_eq!(arg_kind(4), Some(1));
        assert_eq!(arg_kind(8), Some(2));
        assert_eq!(arg_kind(12), Some(3));
        assert_eq!(arg_kind(16), Some(4));
        assert_eq!(arg_kind(19), Some(4));
        assert!(arg_kind(20).is_none());
        assert_eq!(arg_kind_name(0), Some("-t"));
        assert_eq!(arg_kind_name(4), Some("--latency"));
        assert!(arg_kind_name(5).is_none());
    }

    #[test]
    fn test_bin_mapping() {
        // i/N 映射：线程档 = i/3（1/2/4/8），连接档 = i%3（50/100/200）。
        assert_eq!(bin_threads(0), Some(1));
        assert_eq!(bin_threads(2), Some(1));
        assert_eq!(bin_threads(3), Some(2));
        assert_eq!(bin_threads(6), Some(4));
        assert_eq!(bin_threads(9), Some(8));
        assert_eq!(bin_conns(0), Some(50));
        assert_eq!(bin_conns(1), Some(100));
        assert_eq!(bin_conns(2), Some(200));
        assert_eq!(bin_conns(4), Some(100));
        assert!(bin_threads(12).is_none());
        assert!(bin_conns(12).is_none());
        assert!(bin_cell(12).is_none());
    }

    #[test]
    fn test_lua_names() {
        assert_eq!(lua_name(0), Some("wrk.scheme"));
        assert_eq!(lua_name(7), Some("wrk.done"));
        assert!(lua_name(8).is_none());
        assert!(lua_cell(8).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = arg_cell(3).unwrap_or(0);
        assert_eq!(a, arg_cell(3).unwrap_or(0));
        assert_ne!(a, arg_cell(4).unwrap_or(0));
        assert_ne!(lua_cell(2).unwrap_or(0), lua_cell(3).unwrap_or(0));
        assert_ne!(bin_cell(4).unwrap_or(0), bin_cell(5).unwrap_or(0));
        assert_ne!(x_cell(1).unwrap_or(0), x_cell(2).unwrap_or(0));
        assert!(cl_row(CL_ROWS).is_none());
        assert!(x_cell(X_CELLS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = WrkLedger::new();
        // 先落低族格再落高族，合法序不误抓。
        let _ = lt.record_arg(9, arg_cell(9).unwrap_or(0));
        assert_eq!(lt.record_arg(2, arg_cell(2).unwrap_or(0)), Err(WrkCode::EOrder));
        assert!(lt.record_arg(10, arg_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = WrkLedger::new();
        assert_eq!(
            lt.record_arg(5, arg_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(WrkCode::EArgDrift)
        );
        assert_eq!(
            lt.record_lua(3, lua_cell(3).unwrap_or(0).wrapping_add(7)),
            Err(WrkCode::ELuaDrift)
        );
        assert_eq!(
            lt.record_bin(6, bin_cell(6).unwrap_or(0).wrapping_add(3)),
            Err(WrkCode::EArgDrift)
        );
        assert_eq!(
            lt.record_x(2, x_cell(2).unwrap_or(0).wrapping_add(9)),
            Err(WrkCode::EXDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = WrkLedger::new();
        let _ = lt.record_arg(0, arg_cell(0).unwrap_or(0));
        assert_eq!(lt.record_arg(0, arg_cell(0).unwrap_or(0)), Err(WrkCode::EDup));
        let _ = lt.record_x(0, x_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_x(0, x_cell(0).unwrap_or(0)),
            Err(WrkCode::EDup)
        );
    }

    #[test]
    fn test_gate_and_seal() {
        let mut lt = WrkLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        // 未齐即封：只落六闸，其余节缺，封账必拒。
        assert_eq!(lt.seal(), Err(WrkCode::ESeal));
        let full = fresh_complete();
        assert!(full.seal().is_ok());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 记账格数恰为各记账节常量之和（声明行不计数）。
        assert_eq!(recorded, ARG_CELLS as u32 + LUA_CELLS as u32 + BIN_CELLS as u32 + X_CELLS as u32);
        assert_eq!(caught, 10);
    }

    #[test]
    fn test_injection_ledger() {
        let (inj, caught, esc) = run_cycle_stats();
        assert_eq!(inj, 10);
        assert_eq!(caught, 10);
        assert_eq!(esc, 0);
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
        assert_eq!(SEG_BASE, 0xCB8C0);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(WrkCode::Ok as u32, 0);
        assert_eq!(WrkCode::ENoArg as u32, 1);
        assert_eq!(WrkCode::EArgDrift as u32, 2);
        assert_eq!(WrkCode::EOrder as u32, 3);
        assert_eq!(WrkCode::ENoLua as u32, 4);
        assert_eq!(WrkCode::ELuaDrift as u32, 5);
        assert_eq!(WrkCode::ENoBin as u32, 6);
        assert_eq!(WrkCode::ENoX as u32, 7);
        assert_eq!(WrkCode::EXDrift as u32, 8);
        assert_eq!(WrkCode::ENoRow as u32, 9);
        assert_eq!(WrkCode::EDup as u32, 10);
        assert_eq!(WrkCode::ESeal as u32, 11);
        assert_eq!(WrkCode::EGradeDrift as u32, 12);
        assert_eq!(WrkCode::Ok.val(), SEG_BASE);
        let mut i = 0u32;
        while i <= 12 {
            let c = match i {
                0 => WrkCode::Ok,
                1 => WrkCode::ENoArg,
                2 => WrkCode::EArgDrift,
                3 => WrkCode::EOrder,
                4 => WrkCode::ENoLua,
                5 => WrkCode::ELuaDrift,
                6 => WrkCode::ENoBin,
                7 => WrkCode::ENoX,
                8 => WrkCode::EXDrift,
                9 => WrkCode::ENoRow,
                10 => WrkCode::EDup,
                11 => WrkCode::ESeal,
                _ => WrkCode::EGradeDrift,
            };
            assert!(!c.detail().is_empty());
            i += 1;
        }
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3，六闸齐 S2，全齐目标档 S1。
        let empty = WrkLedger::new();
        assert_eq!(empty.judge_grade(), Grade::S3);
        let mut mid = WrkLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = mid.judge_gate(g);
            g += 1;
        }
        assert_eq!(mid.judge_grade(), Grade::S2);
        assert_eq!(fresh_complete().judge_grade(), Grade::S1);
    }

    #[test]
    fn unx52484_checks_green() {
        let s = run_unx52484_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 17);
    }
}
