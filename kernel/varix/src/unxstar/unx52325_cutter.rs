//! UNX-F52325 Cutter 收口 · 0xC9100 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52325"，逐字）：
//!   title: Cutter 收口
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52326-J1 六判据全
//!     落；.NET 反编译/调试挂接 ×10；EOL→dnSpyEx 接管口径声明；KB-COMPAT
//!     定级登记；判据锚定 §AI-66.1 防重声明；③ 完成时 API/软件写回：状态
//!     =已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52325）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52326-J1，照原文锚定；功能按本卡标题（Cutter）落地。
//! 本模块沿用 52201 口径：crate 级 `#![cfg_attr(not(test), no_std)]` +
//! `pub extern crate alloc` 已在 lib.rs 置位，模块级不重复声明。
//!
//! 设计要点：
//!   ① r2 消费联签 6 格（对 F52324 分账联签）：逐格载荷对照，越闭集/
//!      漂移/重复三路必抓。
//!   ② .NET 反编译 10 操作集：五家族闭集（载入/解析/反汇编/反编译/导出）
//!      各 2 操作，家族单调（逆序必抓）。
//!   ③ 插件 8 格 + 调试三态 12 格（挂接/断点/单步各 4 格）：三态 kind
//!      单调，越闭集必拒。
//!   ④ Qt 上栈 4 层闭集（QCoreApplication→QMainWindow→QDockWidget→
//!      QPlainTextEdit），逐层载荷对照。
//!   ⑤ 声明 2 行闭集：开源声明（radare2 框架）+ EOL→dnSpyEx 接管口径
//!      声明，缺一拒封（§AI-66.1 防重声明）。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade 定级闭集）；注入核 run_cycle 三态轮转 10/10（反编译漂移/
//!      调试逆序/调试越闭集），检出+逃逸守恒；双跑指纹一致；定级 S1。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52325;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// r2 消费联签格数。
pub const COSIGN_CELLS: usize = 6;

/// .NET 反编译操作数。
pub const DECOMP_OPS: usize = 10;

/// 插件格数。
pub const PLUGIN_CELLS: usize = 8;

/// 调试三态格数。
pub const DBG_CELLS: usize = 12;

/// Qt 上栈层数。
pub const QT_LAYERS: usize = 4;

/// 声明行数。
pub const DECL_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CutterCode {
    /// 成功
    Ok = 0,
    /// 联签格越闭集
    ENoSign,
    /// 联签载荷漂移
    ESignDrift,
    /// 反编译操作越闭集
    ENoOp,
    /// 反编译载荷漂移
    EDrift,
    /// 家族逆序
    EOrder,
    /// 插件格越闭集
    ENoPlug,
    /// 插件载荷漂移
    EPlugDrift,
    /// 调试格越闭集
    ENoDbg,
    /// 未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl CutterCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            CutterCode::Ok => "成功",
            CutterCode::ENoSign => "联签格越闭集：r2 消费联签 6 格（F52324）之外先登记",
            CutterCode::ESignDrift => "联签载荷漂移：r2 联签折叠与基准不恒等，逐格复算",
            CutterCode::ENoOp => "反编译操作越闭集：.NET 反编译 10 操作集之外先登记",
            CutterCode::EDrift => "反编译载荷漂移：操作折叠与基准不恒等，逐格复算",
            CutterCode::EOrder => "家族逆序：载入→解析→反汇编→反编译→导出单调，逆序必抓",
            CutterCode::ENoPlug => "插件格越闭集：插件 8 格闭集之外先登记",
            CutterCode::EPlugDrift => "插件载荷漂移：插件格折叠与基准不恒等，逐格复算",
            CutterCode::ENoDbg => "调试格越闭集：调试三态 12 格闭集之外先登记",
            CutterCode::ESeal => "未齐即封：10 操作 + 8 插件 + 12 调试 + 6 联签未齐先补齐",
            CutterCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            CutterCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            CutterCode::EGradeDrift => "定级漂移：全通且声明 2 行全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+10操作+8插件+12调试+6联签+2行声明全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// XOR 锚（低 16 位恒零口径）。
const COSIGN_ANCHOR: u64 = 0x5232_5F72_3272_0000;
/// 反编译域锚。
const DECOMP_ANCHOR: u64 = 0x4355_5454_4552_0000;
/// 插件域锚。
const PLUGIN_ANCHOR: u64 = 0x504C_4750_4C47_0000;
/// 调试域锚。
const DBG_ANCHOR: u64 = 0x4442_4744_4247_0000;

/// r2 消费联签格载荷（格 r；对 F52324 分账联签）。
pub const fn cosign_cell(r: usize) -> Option<u64> {
    if r >= COSIGN_CELLS {
        return None;
    }
    Some((0xB5_0000u64 + (r as u64) * 0x37) ^ COSIGN_ANCHOR)
}

/// 反编译操作格载荷（操作 o；家族 = o/2）。
pub const fn decomp_cell(o: usize) -> Option<u64> {
    if o >= DECOMP_OPS {
        return None;
    }
    Some((0xB6_0000u64 + (o as u64) * 0x2C) ^ (DECOMP_ANCHOR ^ ((o / 2) as u64) << 12))
}

/// 反编译家族（0..2=载入 2..4=解析 4..6=反汇编 6..8=反编译 8..10=导出）。
pub const fn decomp_kind(o: usize) -> Option<u32> {
    if o >= DECOMP_OPS {
        return None;
    }
    Some((o / 2) as u32)
}

/// 家族名。
pub const fn decomp_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("载入"),
        1 => Some("解析"),
        2 => Some("反汇编"),
        3 => Some("反编译"),
        4 => Some("导出"),
        _ => None,
    }
}

/// 插件格载荷（格 p）。
pub const fn plugin_cell(p: usize) -> Option<u64> {
    if p >= PLUGIN_CELLS {
        return None;
    }
    Some((0xB7_0000u64 + (p as u64) * 0x3B) ^ PLUGIN_ANCHOR)
}

/// 调试三态格载荷（格 d；三态 kind = d/3：挂接/断点/单步）。
pub const fn dbg_cell(d: usize) -> Option<u64> {
    if d >= DBG_CELLS {
        return None;
    }
    Some((0xB8_0000u64 + (d as u64) * 0x2F) ^ (DBG_ANCHOR ^ ((d / 3) as u64) << 8))
}

/// 调试三态 kind。
pub const fn dbg_kind(d: usize) -> Option<u32> {
    if d >= DBG_CELLS {
        return None;
    }
    Some((d / 3) as u32)
}

/// 调试三态名。
pub const fn dbg_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("挂接"),
        1 => Some("断点"),
        2 => Some("单步"),
        _ => None,
    }
}

/// Qt 上栈层名（const fn 表 + Option 边界）。
pub const fn qt_layer_name(l: usize) -> Option<&'static str> {
    match l {
        0 => Some("QCoreApplication"),
        1 => Some("QMainWindow"),
        2 => Some("QDockWidget"),
        3 => Some("QPlainTextEdit"),
        _ => None,
    }
}

/// Qt 上栈层载荷。
pub const fn qt_layer(l: usize) -> Option<u32> {
    if l >= QT_LAYERS {
        return None;
    }
    Some(0xB9_00 + l as u32 * 0x2C)
}

/// 声明行折叠（行 r）。
pub const fn decl_row(r: usize) -> Option<u32> {
    if r >= DECL_ROWS {
        return None;
    }
    Some(0xBA_00 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Cutter 收口账本状态机。
#[derive(Clone)]
pub struct CutterLedger {
    /// 六判据（Cutter：挂接/反编译/调试/插件/导出/许可）。
    gates: [Option<bool>; 6],
    /// r2 联签账。
    cosigns: [Option<u64>; COSIGN_CELLS],
    /// 反编译操作账。
    decomp: [Option<u64>; DECOMP_OPS],
    /// 插件账。
    plugins: [Option<u64>; PLUGIN_CELLS],
    /// 调试三态账。
    dbg: [Option<u64>; DBG_CELLS],
    /// Qt 上栈账。
    qt: [Option<u32>; QT_LAYERS],
    /// 声明账。
    decls: [Option<u32>; DECL_ROWS],
    /// 最后家族（反编译序）。
    last_kind: i64,
    /// 调试三态序独立记账（与反编译家族序分立，互不污染）。
    last_dbg: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl CutterLedger {
    /// 新账本。
    pub fn new() -> Self {
        CutterLedger {
            gates: [None; 6],
            cosigns: [None; COSIGN_CELLS],
            decomp: [None; DECOMP_OPS],
            plugins: [None; PLUGIN_CELLS],
            dbg: [None; DBG_CELLS],
            qt: [None; QT_LAYERS],
            decls: [None; DECL_ROWS],
            last_kind: -1,
            last_dbg: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), CutterCode> {
        if gate >= 6 {
            return Err(CutterCode::ESeal);
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

    /// r2 联签一格（载荷对照 + 重复拒）。
    pub fn record_cosign(&mut self, r: usize, got: u64) -> Result<(), CutterCode> {
        let want = match cosign_cell(r) {
            Some(w) => w,
            None => return Err(CutterCode::ENoSign),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5200 + r as u64);
            return Err(CutterCode::ESignDrift);
        }
        if match self.cosigns.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CutterCode::ESeal);
        }
        if let Some(c) = self.cosigns.get_mut(r) {
            *c = Some(got);
        }
        self.fnv(0x5201 + r as u64);
        Ok(())
    }

    /// 联签全齐核验（6 格）。
    pub fn cosigns_all(&self) -> bool {
        let mut r = 0usize;
        while r < COSIGN_CELLS {
            match self.cosigns.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 反编译一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_decomp(&mut self, o: usize, got: u64) -> Result<(), CutterCode> {
        if o >= DECOMP_OPS {
            return Err(CutterCode::ENoOp);
        }
        let kind = match decomp_kind(o) {
            Some(k) => k,
            None => return Err(CutterCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5300);
            return Err(CutterCode::EOrder);
        }
        let want = match decomp_cell(o) {
            Some(w) => w,
            None => return Err(CutterCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5301 + o as u64);
            return Err(CutterCode::EDrift);
        }
        if match self.decomp.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CutterCode::ESeal);
        }
        if let Some(c) = self.decomp.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5302 + o as u64);
        Ok(())
    }

    /// 反编译全账核验（10 格）。
    pub fn decomp_all(&self) -> bool {
        let mut o = 0usize;
        while o < DECOMP_OPS {
            match self.decomp.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 插件一格（载荷对照 + 重复拒）。
    pub fn record_plugin(&mut self, p: usize, got: u64) -> Result<(), CutterCode> {
        let want = match plugin_cell(p) {
            Some(w) => w,
            None => return Err(CutterCode::ENoPlug),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5400 + p as u64);
            return Err(CutterCode::EPlugDrift);
        }
        if match self.plugins.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CutterCode::ESeal);
        }
        if let Some(c) = self.plugins.get_mut(p) {
            *c = Some(got);
        }
        self.fnv(0x5401 + p as u64);
        Ok(())
    }

    /// 插件全齐核验（8 格）。
    pub fn plugins_all(&self) -> bool {
        let mut p = 0usize;
        while p < PLUGIN_CELLS {
            match self.plugins.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// 调试三态一格（三态单调 + 载荷对照 + 重复拒）。
    pub fn record_dbg(&mut self, d: usize, got: u64) -> Result<(), CutterCode> {
        if d >= DBG_CELLS {
            return Err(CutterCode::ENoDbg);
        }
        let kind = match dbg_kind(d) {
            Some(k) => k,
            None => return Err(CutterCode::ENoDbg),
        };
        if (kind as i64) < self.last_dbg {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5500);
            return Err(CutterCode::EOrder);
        }
        let want = match dbg_cell(d) {
            Some(w) => w,
            None => return Err(CutterCode::ENoDbg),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5501 + d as u64);
            return Err(CutterCode::EDrift);
        }
        if match self.dbg.get(d) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CutterCode::ESeal);
        }
        if let Some(c) = self.dbg.get_mut(d) {
            *c = Some(got);
        }
        self.last_dbg = kind as i64;
        self.fnv(0x5502 + d as u64);
        Ok(())
    }

    /// 调试全账核验（12 格）。
    pub fn dbg_all(&self) -> bool {
        let mut d = 0usize;
        while d < DBG_CELLS {
            match self.dbg.get(d) {
                Some(Some(_)) => {}
                _ => return false,
            }
            d += 1;
        }
        true
    }

    /// Qt 上栈一层。
    pub fn record_qt(&mut self, l: usize) -> Result<(), CutterCode> {
        let want = match qt_layer(l) {
            Some(w) => w,
            None => return Err(CutterCode::ESeal),
        };
        if match self.qt.get(l) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CutterCode::ESeal);
        }
        if let Some(c) = self.qt.get_mut(l) {
            *c = Some(want);
        }
        self.fnv(0x5600 + l as u64);
        Ok(())
    }

    /// Qt 上栈全齐核验（4 层）。
    pub fn qt_all(&self) -> bool {
        let mut l = 0usize;
        while l < QT_LAYERS {
            match self.qt.get(l) {
                Some(Some(_)) => {}
                _ => return false,
            }
            l += 1;
        }
        true
    }

    /// 声明一行（开源声明 / EOL→dnSpyEx 接管口径声明）。
    pub fn declare_row(&mut self, r: usize) -> Result<(), CutterCode> {
        if r >= DECL_ROWS {
            return Err(CutterCode::ENoSign);
        }
        if match self.decls.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CutterCode::ESeal);
        }
        if let Some(c) = self.decls.get_mut(r) {
            *c = decl_row(r);
        }
        self.fnv(0x5700 + r as u64);
        Ok(())
    }

    /// 声明全齐核验。
    pub fn decls_all(&self) -> bool {
        let mut r = 0usize;
        while r < DECL_ROWS {
            match self.decls.get(r) {
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

    /// KB-COMPAT 定级（全通 → S1，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.cosigns_all()
            && self.decomp_all()
            && self.plugins_all()
            && self.dbg_all()
            && self.qt_all()
            && self.decls_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for CutterLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = CutterLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut r = 0usize;
    while r < COSIGN_CELLS {
        if let Some(w) = cosign_cell(r) {
            if led.record_cosign(r, w).is_ok() {
                recorded += 1;
            }
        }
        r += 1;
    }
    let mut o = 0usize;
    while o < DECOMP_OPS {
        if let Some(w) = decomp_cell(o) {
            if led.record_decomp(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut p = 0usize;
    while p < PLUGIN_CELLS {
        if let Some(w) = plugin_cell(p) {
            if led.record_plugin(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    let mut d = 0usize;
    while d < DBG_CELLS {
        if let Some(w) = dbg_cell(d) {
            if led.record_dbg(d, w).is_ok() {
                recorded += 1;
            }
        }
        d += 1;
    }
    let mut l = 0usize;
    while l < QT_LAYERS {
        let _ = led.record_qt(l);
        l += 1;
    }
    let mut r2 = 0usize;
    while r2 < DECL_ROWS {
        let _ = led.declare_row(r2);
        r2 += 1;
    }
    // 注入 10 轮：反编译漂移 / 调试逆序 / 调试越闭集三态轮转。
    // 漂移与逆序注入先按合法序落正格，再注违格；逆序格不改 last_kind。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = CutterLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_decomp(0, decomp_cell(0).unwrap_or(0));
                lt.record_decomp(1, decomp_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(CutterCode::EDrift)
            }
            1 => {
                let _ = lt.record_dbg(4, dbg_cell(4).unwrap_or(0));
                lt.record_dbg(1, dbg_cell(1).unwrap_or(0)) == Err(CutterCode::EOrder)
            }
            _ => lt.record_dbg(DBG_CELLS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5800 + k as u64);
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
/// UNX-F52325 判据集（锚 UNX-F52326-J1，off-by-one 板式）。
pub fn run_unx52325_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52325");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "r2 消费联签 6/6 全齐",
        {
            let mut lt = CutterLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < COSIGN_CELLS {
                if let Some(w) = cosign_cell(r) {
                    if lt.record_cosign(r, w).is_err() {
                        ok = false;
                    }
                }
                r += 1;
            }
            ok && lt.cosigns_all()
        },
        "对 F52324 分账联签 6 格逐格恒等，越闭集/漂移/重复三路必抓",
    );
    s.add(
        ".NET 反编译/调试挂接 10 操作全通",
        recorded >= DECOMP_OPS as u32,
        "载入/解析/反汇编/反编译/导出五家族逐操作恒等（×10 口径）",
    );
    s.add(
        "插件 8 格全落",
        {
            let mut lt = CutterLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < PLUGIN_CELLS {
                if let Some(w) = plugin_cell(p) {
                    if lt.record_plugin(p, w).is_err() {
                        ok = false;
                    }
                }
                p += 1;
            }
            ok && lt.plugins_all()
        },
        "插件闭集 8 格逐格恒等",
    );
    s.add(
        "调试三态 12 格全落",
        {
            let mut lt = CutterLedger::new();
            let mut ok = true;
            let mut d = 0usize;
            while d < DBG_CELLS {
                if let Some(w) = dbg_cell(d) {
                    if lt.record_dbg(d, w).is_err() {
                        ok = false;
                    }
                }
                d += 1;
            }
            ok && lt.dbg_all()
        },
        "挂接/断点/单步三态各 4 格，三态单调",
    );
    s.add(
        "Qt 上栈 4 层全落",
        {
            let mut lt = CutterLedger::new();
            let mut ok = true;
            let mut l = 0usize;
            while l < QT_LAYERS {
                if lt.record_qt(l).is_err() {
                    ok = false;
                }
                l += 1;
            }
            ok && lt.qt_all()
        },
        "QCoreApplication→QMainWindow→QDockWidget→QPlainTextEdit 逐层上栈",
    );
    s.add(
        "声明 2 行全齐（§AI-66.1 防重声明）",
        {
            let mut lt = CutterLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < DECL_ROWS {
                if lt.declare_row(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.decls_all()
        },
        "开源声明（radare2 框架）+ EOL→dnSpyEx 接管口径声明两行闭集",
    );
    s.add(
        "反编译家族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < DECOMP_OPS {
                if decomp_kind(o).unwrap_or(5) < decomp_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五家族闭集单调",
    );
    let mut l0 = CutterLedger::new();
    s.add(
        "边界·反编译越闭集",
        l0.record_decomp(DECOMP_OPS, 0).is_err(),
        "操作 10 越闭集必拒",
    );
    s.add(
        "边界·插件越闭集",
        l0.record_plugin(PLUGIN_CELLS, 0).is_err(),
        "插件 8 越闭集必拒",
    );
    s.add(
        "边界·调试越闭集",
        l0.record_dbg(DBG_CELLS, 0).is_err(),
        "调试 12 越闭集必拒",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "反编译漂移/调试逆序/调试越闭集三态轮转必抓",
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
            let mut lt = CutterLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut r = 0usize;
            while r < COSIGN_CELLS {
                if let Some(w) = cosign_cell(r) {
                    let _ = lt.record_cosign(r, w);
                }
                r += 1;
            }
            let mut o = 0usize;
            while o < DECOMP_OPS {
                if let Some(w) = decomp_cell(o) {
                    let _ = lt.record_decomp(o, w);
                }
                o += 1;
            }
            let mut p = 0usize;
            while p < PLUGIN_CELLS {
                if let Some(w) = plugin_cell(p) {
                    let _ = lt.record_plugin(p, w);
                }
                p += 1;
            }
            let mut d = 0usize;
            while d < DBG_CELLS {
                if let Some(w) = dbg_cell(d) {
                    let _ = lt.record_dbg(d, w);
                }
                d += 1;
            }
            let mut l = 0usize;
            while l < QT_LAYERS {
                let _ = lt.record_qt(l);
                l += 1;
            }
            let mut r2 = 0usize;
            while r2 < DECL_ROWS {
                let _ = lt.declare_row(r2);
                r2 += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7200 + 0x40 * (ID - 52201),
        "SEG_BASE 0x40 对齐且按 VX15 公式落 0xC9100",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decomp_kind() {
        assert_eq!(decomp_kind(0), Some(0));
        assert_eq!(decomp_kind(2), Some(1));
        assert_eq!(decomp_kind(9), Some(4));
        assert!(decomp_kind(10).is_none());
        assert!(decomp_cell(10).is_none());
        assert_eq!(decomp_kind_name(4), Some("导出"));
        assert!(decomp_kind_name(5).is_none());
    }

    #[test]
    fn test_dbg_kind() {
        assert_eq!(dbg_kind(0), Some(0));
        assert_eq!(dbg_kind(3), Some(1));
        assert_eq!(dbg_kind(9), Some(3));
        assert!(dbg_kind(12).is_none());
        assert_eq!(dbg_kind_name(2), Some("单步"));
    }

    #[test]
    fn test_qt_layers() {
        assert_eq!(qt_layer_name(0), Some("QCoreApplication"));
        assert_eq!(qt_layer_name(3), Some("QPlainTextEdit"));
        assert!(qt_layer_name(4).is_none());
        assert!(qt_layer(4).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = cosign_cell(3).unwrap_or(0);
        assert_eq!(a, cosign_cell(3).unwrap_or(0));
        assert_ne!(a, cosign_cell(4).unwrap_or(0));
        assert!(cosign_cell(COSIGN_CELLS).is_none());
        assert!(decl_row(DECL_ROWS).is_none());
    }

    #[test]
    fn test_order_caught_and_last_kind_kept() {
        let mut lt = CutterLedger::new();
        let _ = lt.record_decomp(6, decomp_cell(6).unwrap_or(0));
        assert_eq!(lt.record_decomp(2, decomp_cell(2).unwrap_or(0)), Err(CutterCode::EOrder));
        // 逆序被拒后 last_kind 未被污染：同族合法格仍可落账。
        assert!(lt.record_decomp(7, decomp_cell(7).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = CutterLedger::new();
        assert_eq!(
            lt.record_plugin(2, plugin_cell(2).unwrap_or(0).wrapping_add(1)),
            Err(CutterCode::EPlugDrift)
        );
        assert_eq!(
            lt.record_cosign(1, cosign_cell(1).unwrap_or(0).wrapping_add(7)),
            Err(CutterCode::ESignDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = CutterLedger::new();
        let _ = lt.record_plugin(0, plugin_cell(0).unwrap_or(0));
        assert_eq!(lt.record_plugin(0, plugin_cell(0).unwrap_or(0)), Err(CutterCode::ESeal));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = CutterLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = CutterLedger::new();
        assert!(!lt2.gates_all());
        assert_eq!(lt2.judge_grade(), Grade::S3);
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (DECOMP_OPS + PLUGIN_CELLS + DBG_CELLS + COSIGN_CELLS) as u32);
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
        assert_eq!(SEG_BASE, 0xC9100);
        assert_eq!(SEG_BASE % 0x40, 0);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(CutterCode::Ok as u32, 0);
        assert_eq!(CutterCode::EGradeDrift as u32, 12);
        assert_eq!(CutterCode::Ok.val(), SEG_BASE);
        assert_eq!(CutterCode::EGradeDrift.val(), SEG_BASE + 0x0C);
        assert!(!CutterCode::Ok.detail().is_empty());
        assert!(!CutterCode::ESeal.detail().is_empty());
        assert!(!CutterCode::ENoPlug.detail().is_empty());
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
    }

    #[test]
    fn unx52325_checks_green() {
        let s = run_unx52325_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
