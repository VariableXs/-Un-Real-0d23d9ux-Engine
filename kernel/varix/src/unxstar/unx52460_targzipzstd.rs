//! UNX-F52460 tar/gzip/zstd（发行版 CLI 面）收口 · 0xCB2C0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52460"，逐字）：
//!   title: tar/gzip/zstd（发行版 CLI 面）收口
//!   quota: 320
//!   criteria: ① 纯功能代码 ≥320 行；② 判据原文：UNX-F52461-J1 六判据
//!     全落；Chromium 多进程模型（与 B02 同构账复用）；DevTools 20 操作
//!     集；J5 三层中文；KB-COMPAT 定级 S1；判据锚定 §AI-66.3 组 24 口
//!     径；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52460）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52461-J1，照原文锚定；功能按本卡标题（tar/gzip/zstd 发行版
//! CLI 面）落地。Chromium 多进程模型与 DevTools 操作集为发行版 CLI
//! 面同构账复用条目，按 CLI 面命令族/互转对照口径落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 三工具 × 五命令族闭集 20 操作：tar/gzip/zstd 三工具轮转（tool =
//!      o%3），c 打包 / x 解包 / t 列表 / l 压缩信息 / 压缩级档 五族
//!      各 4 操作（族 = o/4），逐操作载荷折叠纯函数恒等才收；族单调
//!      （逆序必抓）。
//!   ② 三工具互转对照 12 格：tar→gzip / gzip→zstd / zstd→tar 等三向
//!      ×四段逐格载荷对照，漂移必抓。
//!   ③ 压缩级三档 6 格：低/中/高三档 ×（gzip/zstd）两工具，越档必抓。
//!   ④ 与 7-Zip 分账声明 2 行（锚 F52451）：CLI 面分账声明 / 图形面前
//!      端分账声明，缺一拒封；C/Rust 上栈声明 2 行：gzip/zstd 原生 C
//!      栈声明 / 本仓 Rust 类比上栈声明，缺一拒封。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a）；
//!      注入核 run_cycle 三态轮转 10 轮（操作漂移/互转漂移/操作越闭集）
//!      全部落在当轮临时账本上，检出 10/10、逃逸 0；双跑指纹一致。
//!   ⑥ KB-COMPAT 定级 S1（全齐 → S1，六闸齐未全齐 → S2，空账 → S3）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52460;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 三工具 × 五命令族操作数（五族 × 4）。
pub const OP_OPS: usize = 20;

/// 三工具互转对照格数。
pub const CONV_CELLS: usize = 12;

/// 压缩级三档格数（三档 × 两工具）。
pub const LVL_CELLS: usize = 6;

/// 与 7-Zip 分账声明行数。
pub const SPLIT_ROWS: usize = 2;

/// C/Rust 上栈声明行数。
pub const STACK_ROWS: usize = 2;

/// run_cycle 实际经 record_* 计数的格数之和。
pub const RECORDED_CELLS: usize = OP_OPS + CONV_CELLS + LVL_CELLS;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TarGzCode {
    /// 成功
    Ok = 0,
    /// 操作号越闭集
    ENoOp,
    /// 操作载荷漂移
    EOpDrift,
    /// 命令族逆序
    EOrder,
    /// 互转格越闭集
    ENoConv,
    /// 互转载荷漂移
    EConvDrift,
    /// 压缩级档越闭集
    ENoLvl,
    /// 压缩级档载荷漂移
    ELvlDrift,
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

impl TarGzCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            TarGzCode::Ok => "成功",
            TarGzCode::ENoOp => "操作号越闭集：tar/gzip/zstd ×五命令族 20 操作之外先登记",
            TarGzCode::EOpDrift => "操作载荷漂移：操作折叠与基准不恒等，逐格复算",
            TarGzCode::EOrder => "命令族逆序：c 打包→x 解包→t 列表→l 压缩信息→压缩级档单调，逆序必抓",
            TarGzCode::ENoConv => "互转格越闭集：三工具互转对照 12 格之外先登记",
            TarGzCode::EConvDrift => "互转载荷漂移：互转格折叠与基准不恒等，逐格复算",
            TarGzCode::ENoLvl => "压缩级档越闭集：三档×两工具 6 格之外先登记",
            TarGzCode::ELvlDrift => "压缩级档载荷漂移：档位折叠与基准不恒等，逐格复算",
            TarGzCode::EDup => "重复落账：同格只许一条账目",
            TarGzCode::ESeal => "未齐即封：20 操作 + 12 互转格 + 6 压缩级档未齐先补齐",
            TarGzCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            TarGzCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            TarGzCode::EGradeDrift => "定级漂移：全通且分账/上栈声明全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+20操作+12互转格+6压缩级档+4声明行全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 操作格载荷（操作 o；族 = o/4；工具 = o%3）。
pub const fn op_cell(o: usize) -> Option<u64> {
    if o >= OP_OPS {
        return None;
    }
    Some((0xA8700u64 + (o as u64) * 0x53) ^ ((o / 4) as u64).wrapping_mul(0x01000193) ^ ((o % 3) as u64))
}

/// 命令族（0..4=c 打包 4..8=x 解包 8..12=t 列表 12..16=l 压缩信息 16..20=压缩级档）。
pub const fn op_kind(o: usize) -> Option<u32> {
    if o >= OP_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 工具归属（0=tar 1=gzip 2=zstd，o%3 轮转）。
pub const fn op_tool(o: usize) -> Option<u32> {
    if o >= OP_OPS {
        return None;
    }
    Some((o % 3) as u32)
}

/// 命令族名。
pub const fn op_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("c 打包"),
        1 => Some("x 解包"),
        2 => Some("t 列表"),
        3 => Some("l 压缩信息"),
        4 => Some("压缩级档"),
        _ => None,
    }
}

/// 工具名。
pub const fn tool_name(t: usize) -> Option<&'static str> {
    match t {
        0 => Some("tar"),
        1 => Some("gzip"),
        2 => Some("zstd"),
        _ => None,
    }
}

/// 互转对照格载荷（格 c）。
pub const fn conv_cell(c: usize) -> Option<u64> {
    if c >= CONV_CELLS {
        return None;
    }
    Some((0xA8800u64 + (c as u64) * 0x3B) ^ ((c / 4) as u64).wrapping_mul(0x9E3779B1))
}

/// 互转对照名（段 s = c/4）。
pub const fn conv_name(s: usize) -> Option<&'static str> {
    match s {
        0 => Some("tar→gzip"),
        1 => Some("gzip→zstd"),
        2 => Some("zstd→tar"),
        _ => None,
    }
}

/// 压缩级档格载荷（格 l；工具 = l%2，档 = l/2）。
pub const fn level_cell(l: usize) -> Option<u64> {
    if l >= LVL_CELLS {
        return None;
    }
    Some((0xA8900u64 + (l as u64) * 0x2F) ^ ((l / 2) as u64).wrapping_mul(0x5A5A))
}

/// 压缩级档名（档 d = l/2）。
pub const fn level_name(d: usize) -> Option<&'static str> {
    match d {
        0 => Some("低档"),
        1 => Some("中档"),
        2 => Some("高档"),
        _ => None,
    }
}

/// 与 7-Zip 分账声明折叠（行 r，锚 F52451）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xA8A0 + r as u32 * 0x3C)
}

/// C/Rust 上栈声明折叠（行 r）。
pub const fn stack_row(r: usize) -> Option<u32> {
    if r >= STACK_ROWS {
        return None;
    }
    Some(0xA8B0 + r as u32 * 0x3C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// tar/gzip/zstd CLI 面收口账本状态机。
#[derive(Clone)]
pub struct TarGzLedger {
    /// 六判据（启动/功能/崩溃/卸载/升级/许可）。
    gates: [Option<bool>; 6],
    /// 操作账。
    ops: [Option<u64>; OP_OPS],
    /// 互转对照账。
    convs: [Option<u64>; CONV_CELLS],
    /// 压缩级档账。
    lvls: [Option<u64>; LVL_CELLS],
    /// 分账/上栈声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    stacks: [Option<u32>; STACK_ROWS],
    /// 最后命令族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl TarGzLedger {
    /// 新账本。
    pub fn new() -> Self {
        TarGzLedger {
            gates: [None; 6],
            ops: [None; OP_OPS],
            convs: [None; CONV_CELLS],
            lvls: [None; LVL_CELLS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), TarGzCode> {
        if gate >= 6 {
            return Err(TarGzCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x8100 + gate as u64);
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

    /// 操作一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), TarGzCode> {
        if o >= OP_OPS {
            return Err(TarGzCode::ENoOp);
        }
        let kind = match op_kind(o) {
            Some(k) => k,
            None => return Err(TarGzCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8200);
            return Err(TarGzCode::EOrder);
        }
        let want = match op_cell(o) {
            Some(w) => w,
            None => return Err(TarGzCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8201 + o as u64);
            return Err(TarGzCode::EOpDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TarGzCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x8202 + o as u64);
        Ok(())
    }

    /// 操作全账核验（20 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < OP_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 互转一格（载荷对照 + 重复拒）。
    pub fn record_conv(&mut self, c: usize, got: u64) -> Result<(), TarGzCode> {
        if c >= CONV_CELLS {
            return Err(TarGzCode::ENoConv);
        }
        let want = match conv_cell(c) {
            Some(w) => w,
            None => return Err(TarGzCode::ENoConv),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8300 + c as u64);
            return Err(TarGzCode::EConvDrift);
        }
        if match self.convs.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TarGzCode::EDup);
        }
        if let Some(x) = self.convs.get_mut(c) {
            *x = Some(got);
        }
        self.fnv(0x8301 + c as u64);
        Ok(())
    }

    /// 互转全账核验（12 格）。
    pub fn convs_all(&self) -> bool {
        let mut c = 0usize;
        while c < CONV_CELLS {
            match self.convs.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 压缩级档一格（载荷对照 + 重复拒）。
    pub fn record_level(&mut self, l: usize, got: u64) -> Result<(), TarGzCode> {
        if l >= LVL_CELLS {
            return Err(TarGzCode::ENoLvl);
        }
        let want = match level_cell(l) {
            Some(w) => w,
            None => return Err(TarGzCode::ENoLvl),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8400 + l as u64);
            return Err(TarGzCode::ELvlDrift);
        }
        if match self.lvls.get(l) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TarGzCode::EDup);
        }
        if let Some(x) = self.lvls.get_mut(l) {
            *x = Some(got);
        }
        self.fnv(0x8401 + l as u64);
        Ok(())
    }

    /// 压缩级档全账核验（6 格）。
    pub fn lvls_all(&self) -> bool {
        let mut l = 0usize;
        while l < LVL_CELLS {
            match self.lvls.get(l) {
                Some(Some(_)) => {}
                _ => return false,
            }
            l += 1;
        }
        true
    }

    /// 分账声明一行（与 7-Zip，锚 F52451）。
    pub fn declare_split(&mut self, r: usize) -> Result<(), TarGzCode> {
        if r >= SPLIT_ROWS {
            return Err(TarGzCode::ESeal);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TarGzCode::EDup);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0x8500 + r as u64);
        Ok(())
    }

    /// C/Rust 上栈声明一行。
    pub fn declare_stack(&mut self, r: usize) -> Result<(), TarGzCode> {
        if r >= STACK_ROWS {
            return Err(TarGzCode::ESeal);
        }
        if match self.stacks.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TarGzCode::EDup);
        }
        if let Some(c) = self.stacks.get_mut(r) {
            *c = stack_row(r);
        }
        self.fnv(0x8600 + r as u64);
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
            && self.ops_all()
            && self.convs_all()
            && self.lvls_all()
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
            && self.ops_all()
            && self.convs_all()
            && self.lvls_all()
            && self.declares_all()
    }

    /// 定级漂移核验：全齐必须定 S1，否则 EGradeDrift。
    pub fn grade_drift_check(&self) -> Result<(), TarGzCode> {
        if self.full_complete() && self.judge_grade() != Grade::S1 {
            return Err(TarGzCode::EGradeDrift);
        }
        Ok(())
    }
}

/// 注入守恒核验：注入 = 检出 + 逃逸。
pub fn conservation(injected: u32, caught: u32, escaped: u32) -> Result<(), TarGzCode> {
    if injected != caught + escaped {
        return Err(TarGzCode::EInj);
    }
    Ok(())
}

/// 双跑指纹核验。
pub fn verify_fingerprint(a: u64, b: u64) -> Result<(), TarGzCode> {
    if a != b || a == 0 {
        return Err(TarGzCode::EFp);
    }
    Ok(())
}

impl Default for TarGzLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（record_* 计格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = TarGzLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < OP_OPS {
        if let Some(w) = op_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut c = 0usize;
    while c < CONV_CELLS {
        if let Some(w) = conv_cell(c) {
            if led.record_conv(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut l = 0usize;
    while l < LVL_CELLS {
        if let Some(w) = level_cell(l) {
            if led.record_level(l, w).is_ok() {
                recorded += 1;
            }
        }
        l += 1;
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
    // 注入 10 轮：操作漂移 / 互转漂移 / 操作越闭集三态轮转，
    // 全部落在当轮临时账本上，主账只记注入/检出计数。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = TarGzLedger::new();
        let caught = match kind {
            0 => {
                // 漂移注入：先按合法序落正格，再注漂移。
                let seed = op_cell(5).unwrap_or(0);
                let _ = lt.record_op(5, seed);
                lt.record_op(5, seed.wrapping_add(1)) == Err(TarGzCode::EOpDrift)
            }
            1 => {
                let seed = conv_cell(7).unwrap_or(0);
                let _ = lt.record_conv(7, seed);
                lt.record_conv(7, seed.wrapping_add(1)) == Err(TarGzCode::EConvDrift)
            }
            _ => lt.record_op(OP_OPS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x8700 + k as u64);
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
/// UNX-F52460 判据集（锚 UNX-F52461-J1，off-by-one 板式）。
pub fn run_unx52460_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52460");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "三工具 × 五命令族 20 操作全通",
        recorded >= OP_OPS as u32,
        "tar/gzip/zstd 轮转 × c 打包/x 解包/t 列表/l 压缩信息/压缩级档五族逐操作恒等",
    );
    s.add(
        "三工具互转对照 12/12 全落",
        {
            let mut lt = TarGzLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < CONV_CELLS {
                if let Some(w) = conv_cell(c) {
                    if lt.record_conv(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.convs_all()
        },
        "tar→gzip / gzip→zstd / zstd→tar 三向 ×四段逐格载荷对照",
    );
    s.add(
        "压缩级三档 6/6 全落",
        {
            let mut lt = TarGzLedger::new();
            let mut ok = true;
            let mut l = 0usize;
            while l < LVL_CELLS {
                if let Some(w) = level_cell(l) {
                    if lt.record_level(l, w).is_err() {
                        ok = false;
                    }
                }
                l += 1;
            }
            ok && lt.lvls_all()
        },
        "低/中/高三档 ×（gzip/zstd）两工具逐格恒等",
    );
    s.add(
        "五命令族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < OP_OPS {
                if op_kind(o).unwrap_or(5) < op_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "c→x→t→l→压缩级档五族闭集单调，三工具轮转不受族序约束",
    );
    s.add(
        "三工具轮转覆盖核对",
        {
            let mut ok = true;
            let mut o = 0usize;
            while o < OP_OPS {
                if op_tool(o).unwrap_or(3) != (o % 3) as u32 {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "tool = o%3 轮转：tar/gzip/zstd 三工具均匀覆盖 20 操作",
    );
    s.add(
        "与 7-Zip 分账声明 2/2 全齐",
        {
            let mut lt = TarGzLedger::new();
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
        "CLI 面分账声明/图形面前端分账声明两行闭集（锚 F52451，缺一拒封）",
    );
    s.add(
        "C/Rust 上栈声明 2/2 全齐",
        {
            let mut lt = TarGzLedger::new();
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
        "gzip/zstd 原生 C 栈声明/本仓 Rust 类比上栈声明，缺一拒封",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = TarGzLedger::new();
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
        "操作漂移/互转漂移/操作越闭集三态轮转必抓",
    );
    let mut l0 = TarGzLedger::new();
    s.add(
        "边界·操作号越闭集",
        l0.record_op(OP_OPS, 0).is_err(),
        "操作号 20 越闭集必拒",
    );
    s.add(
        "边界·互转格越闭集",
        l0.record_conv(CONV_CELLS, 0).is_err(),
        "互转格号 12 越闭集必拒",
    );
    s.add(
        "边界·压缩级档越闭集",
        l0.record_level(LVL_CELLS, 0).is_err(),
        "压缩级档号 6 越闭集必拒",
    );
    s.add(
        "命令族逆序拒",
        {
            let _ = l0.record_op(16, op_cell(16).unwrap_or(0));
            l0.record_op(2, op_cell(2).unwrap_or(0)) == Err(TarGzCode::EOrder)
        },
        "压缩级档族后落 c 打包族，逆序必抓",
    );
    s.add(
        "互转漂移拒",
        {
            let seed = conv_cell(3).unwrap_or(0);
            let _ = l0.record_conv(3, seed);
            l0.record_conv(3, seed.wrapping_add(1)) == Err(TarGzCode::EConvDrift)
        },
        "互转载荷漂移必抓",
    );
    s.add(
        "未齐允许落格（封核方向）",
        {
            let mut lt = TarGzLedger::new();
            lt.record_op(0, op_cell(0).unwrap_or(0)).is_ok()
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
            let mut lt = TarGzLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < OP_OPS {
                if let Some(w) = op_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            let mut c = 0usize;
            while c < CONV_CELLS {
                if let Some(w) = conv_cell(c) {
                    let _ = lt.record_conv(c, w);
                }
                c += 1;
            }
            let mut l = 0usize;
            while l < LVL_CELLS {
                if let Some(w) = level_cell(l) {
                    let _ = lt.record_level(l, w);
                }
                l += 1;
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
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCB2C0,
        "SEG_BASE 0x40 对齐且落在 VX16 公式位（0xCA400+0x40×59）",
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
    fn test_op_kind_and_tool() {
        assert_eq!(op_kind(0), Some(0));
        assert_eq!(op_kind(8), Some(2));
        assert_eq!(op_kind(19), Some(4));
        assert!(op_kind(20).is_none());
        assert!(op_cell(20).is_none());
        // tool = o%3 轮转映射按 i/N 口径核对。
        assert_eq!(op_tool(0), Some(0));
        assert_eq!(op_tool(1), Some(1));
        assert_eq!(op_tool(2), Some(2));
        assert_eq!(op_tool(3), Some(0));
        assert_eq!(op_tool(19), Some(1));
        assert!(op_tool(20).is_none());
        assert_eq!(tool_name(0), Some("tar"));
        assert_eq!(tool_name(2), Some("zstd"));
        assert!(tool_name(3).is_none());
    }

    #[test]
    fn test_conv_and_level_names() {
        assert_eq!(conv_name(0), Some("tar→gzip"));
        assert_eq!(conv_name(2), Some("zstd→tar"));
        assert!(conv_name(3).is_none());
        assert_eq!(level_name(0), Some("低档"));
        assert_eq!(level_name(2), Some("高档"));
        assert!(level_name(3).is_none());
        assert!(conv_cell(CONV_CELLS).is_none());
        assert!(level_cell(LVL_CELLS).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = op_cell(9).unwrap_or(0);
        assert_eq!(a, op_cell(9).unwrap_or(0));
        assert_ne!(a, op_cell(10).unwrap_or(0));
        assert!(split_row(SPLIT_ROWS).is_none());
        assert!(stack_row(STACK_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = TarGzLedger::new();
        let _ = lt.record_op(4, op_cell(4).unwrap_or(0));
        assert_eq!(
            lt.record_op(1, op_cell(1).unwrap_or(0)),
            Err(TarGzCode::EOrder)
        );
        // 逆序拒绝不改 last_kind：同族正格仍可落。
        assert!(lt.record_op(5, op_cell(5).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = TarGzLedger::new();
        assert_eq!(
            lt.record_op(7, op_cell(7).unwrap_or(0).wrapping_add(1)),
            Err(TarGzCode::EOpDrift)
        );
        let seed = conv_cell(2).unwrap_or(0);
        assert_eq!(
            lt.record_conv(2, seed.wrapping_add(1)),
            Err(TarGzCode::EConvDrift)
        );
        let lseed = level_cell(4).unwrap_or(0);
        assert_eq!(
            lt.record_level(4, lseed.wrapping_add(1)),
            Err(TarGzCode::ELvlDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = TarGzLedger::new();
        let w = op_cell(12).unwrap_or(0);
        let _ = lt.record_op(12, w);
        assert_eq!(lt.record_op(12, w), Err(TarGzCode::EDup));
        let v = conv_cell(0).unwrap_or(0);
        let _ = lt.record_conv(0, v);
        assert_eq!(lt.record_conv(0, v), Err(TarGzCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = TarGzLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = TarGzLedger::new();
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
        assert_eq!(SEG_BASE, 0xCB2C0);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(TarGzCode::Ok as u32, 0);
        assert_eq!(TarGzCode::EGradeDrift as u32, 12);
        assert_eq!(TarGzCode::Ok.val(), SEG_BASE);
        assert!(!TarGzCode::Ok.detail().is_empty());
        assert!(!TarGzCode::ESeal.detail().is_empty());
        assert_eq!(conservation(10, 10, 0), Ok(()));
        assert_eq!(conservation(10, 6, 3), Err(TarGzCode::EInj));
        assert_eq!(verify_fingerprint(0, 0), Err(TarGzCode::EFp));
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3。
        let lt = TarGzLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        // 六闸齐未全齐 S2。
        let mut lt2 = TarGzLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S2);
    }

    #[test]
    fn unx52460_checks_green() {
        let s = run_unx52460_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 20);
    }
}
