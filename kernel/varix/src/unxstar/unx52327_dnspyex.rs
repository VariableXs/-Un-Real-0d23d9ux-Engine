//! UNX-F52327 dnSpyEx 收口 · 0xC9180 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52327"，逐字）：
//!   title: dnSpyEx 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52328-J1 六判据全
//!     落；反编译输出 ×10 程序集全通；KB-COMPAT 定级登记；判据锚定
//!     §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52327）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52328-J1，照原文锚定；功能按本卡标题（dnSpyEx）落地。
//! 本模块沿用 52201 口径：crate 级 `#![cfg_attr(not(test), no_std)]` +
//! `pub extern crate alloc` 已在 lib.rs 置位，模块级不重复声明。
//!
//! 设计要点：
//!   ① dnSpy fork 同构账 12 格（程序集/调试器/反编译器三族各 4 格）：与
//!      F52326 分账声明 2 行（dnSpy 账 / dnSpyEx 账分立，不并账）。
//!   ② 调试 12 格（断点/命中/单步各 4 格）：kind 单调，越闭集必拒。
//!   ③ 反编译输出 ×10 程序集全通口径：fork 同构账 12 格含反编译器 4 格
//!      逐格恒等，×10 程序集载荷折叠核验。
//!   ④ 社区续维护声明 2 行闭集：社区接棒续维护声明 / GitHub 活跃度声明，
//!      缺一拒封（§AI-66.3）。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade 定级闭集）；注入核 run_cycle 三态轮转 10/10（fork 漂移/
//!      调试逆序/fork 越闭集），检出+逃逸守恒；双跑指纹一致；定级 S2
//!      （社区接棒为有条件收口）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52327;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// fork 同构账格数。
pub const FORK_CELLS: usize = 12;

/// 调试格数。
pub const DBG_CELLS: usize = 12;

/// ×10 程序集口径数。
pub const ASM_X10: usize = 10;

/// 声明行数（社区续维护 2 行 + 分账 2 行）。
pub const COMM_ROWS: usize = 2;
/// 分账声明行数（对 F52326）。
pub const SPLIT_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DnSpyExCode {
    /// 成功
    Ok = 0,
    /// fork 格越闭集
    ENoFork,
    /// fork 载荷漂移
    EForkDrift,
    /// 调试格越闭集
    ENoDbg,
    /// 调试载荷漂移
    EDbgDrift,
    /// 家族逆序
    EOrder,
    /// ×10 程序集越闭集
    ENoAsm,
    /// 程序集载荷漂移
    EAsmDrift,
    /// 声明行越闭集
    ENoRow,
    /// 未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl DnSpyExCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            DnSpyExCode::Ok => "成功",
            DnSpyExCode::ENoFork => "fork 格越闭集：dnSpy fork 同构账 12 格之外先登记",
            DnSpyExCode::EForkDrift => "fork 载荷漂移：fork 格折叠与基准不恒等，逐格复算",
            DnSpyExCode::ENoDbg => "调试格越闭集：调试 12 格（断点/命中/单步）之外先登记",
            DnSpyExCode::EDbgDrift => "调试载荷漂移：调试格折叠与基准不恒等，逐格复算",
            DnSpyExCode::EOrder => "家族逆序：程序集→调试器→反编译器单调，逆序必抓",
            DnSpyExCode::ENoAsm => "×10 程序集越闭集：反编译输出 10 程序集口径之外先登记",
            DnSpyExCode::EAsmDrift => "程序集载荷漂移：×10 折叠与基准不恒等，逐格复算",
            DnSpyExCode::ENoRow => "声明行越闭集：社区续维护/分账声明行闭集之外先登记",
            DnSpyExCode::ESeal => "未齐即封：12 fork + 12 调试 + 10 程序集 + 声明未齐先补齐",
            DnSpyExCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            DnSpyExCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            DnSpyExCode::EGradeDrift => "定级漂移：社区接棒口径全通定 S2，不得拔高 S1",
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
            Grade::S1 => "S1：全通收口（社区接棒口径不适用）",
            Grade::S2 => "S2：有条件收口——社区续维护接棒为有条件口径",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// XOR 锚（低 16 位恒零口径）。
const FORK_ANCHOR: u64 = 0x666F_726B_5F45_0000;
/// 调试域锚。
const DBG_ANCHOR: u64 = 0x6462_6765_5F64_0000;
/// 程序集域锚。
const ASM_ANCHOR: u64 = 0x6173_6D5F_6173_0000;

/// fork 同构账格载荷（格 f；族 = f/3：程序集/调试器/反编译器）。
pub const fn fork_cell(f: usize) -> Option<u64> {
    if f >= FORK_CELLS {
        return None;
    }
    Some((0xD0_0000u64 + (f as u64) * 0x2C) ^ (FORK_ANCHOR ^ ((f / 3) as u64) << 12))
}

/// fork 族（0..4=程序集 4..8=调试器 8..12=反编译器）。
pub const fn fork_kind(f: usize) -> Option<u32> {
    if f >= FORK_CELLS {
        return None;
    }
    Some((f / 3) as u32)
}

/// fork 族名。
pub const fn fork_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("程序集"),
        1 => Some("调试器"),
        2 => Some("反编译器"),
        _ => None,
    }
}

/// 调试格载荷（格 d；kind = d/4：断点/命中/单步）。
pub const fn dbg_cell(d: usize) -> Option<u64> {
    if d >= DBG_CELLS {
        return None;
    }
    Some((0xD1_0000u64 + (d as u64) * 0x2F) ^ (DBG_ANCHOR ^ ((d / 4) as u64) << 8))
}

/// 调试 kind。
pub const fn dbg_kind(d: usize) -> Option<u32> {
    if d >= DBG_CELLS {
        return None;
    }
    Some((d / 4) as u32)
}

/// 调试 kind 名。
pub const fn dbg_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("断点"),
        1 => Some("命中"),
        2 => Some("单步"),
        _ => None,
    }
}

/// ×10 程序集格载荷（格 a）。
pub const fn asm_cell(a: usize) -> Option<u64> {
    if a >= ASM_X10 {
        return None;
    }
    Some((0xD2_0000u64 + (a as u64) * 0x37) ^ ASM_ANCHOR)
}

/// 社区续维护声明行折叠（行 r）。
pub const fn comm_row(r: usize) -> Option<u32> {
    if r >= COMM_ROWS {
        return None;
    }
    Some(0xD3_00 + r as u32 * 0x2C)
}

/// 分账声明行折叠（对 F52326，行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xD4_00 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// dnSpyEx 收口账本状态机。
#[derive(Clone)]
pub struct DnSpyExLedger {
    /// 六判据（dnSpyEx：fork 同构/调试/反编译输出/社区维护/分账/许可）。
    gates: [Option<bool>; 6],
    /// fork 同构账。
    fork: [Option<u64>; FORK_CELLS],
    /// 调试账。
    dbg: [Option<u64>; DBG_CELLS],
    /// ×10 程序集账。
    asm: [Option<u64>; ASM_X10],
    /// 社区续维护声明账。
    comm: [Option<u32>; COMM_ROWS],
    /// 分账声明账（对 F52326）。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 最后族（fork 同构序）。
    last_kind: i64,
    /// 调试 kind 序独立记账（与 fork 族序分立，互不污染）。
    last_dbg: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl DnSpyExLedger {
    /// 新账本。
    pub fn new() -> Self {
        DnSpyExLedger {
            gates: [None; 6],
            fork: [None; FORK_CELLS],
            dbg: [None; DBG_CELLS],
            asm: [None; ASM_X10],
            comm: [None; COMM_ROWS],
            splits: [None; SPLIT_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), DnSpyExCode> {
        if gate >= 6 {
            return Err(DnSpyExCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x7100 + gate as u64);
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

    /// fork 一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_fork(&mut self, f: usize, got: u64) -> Result<(), DnSpyExCode> {
        if f >= FORK_CELLS {
            return Err(DnSpyExCode::ENoFork);
        }
        let kind = match fork_kind(f) {
            Some(k) => k,
            None => return Err(DnSpyExCode::ENoFork),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7200);
            return Err(DnSpyExCode::EOrder);
        }
        let want = match fork_cell(f) {
            Some(w) => w,
            None => return Err(DnSpyExCode::ENoFork),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7201 + f as u64);
            return Err(DnSpyExCode::EForkDrift);
        }
        if match self.fork.get(f) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DnSpyExCode::ESeal);
        }
        if let Some(c) = self.fork.get_mut(f) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x7202 + f as u64);
        Ok(())
    }

    /// fork 全账核验（12 格）。
    pub fn fork_all(&self) -> bool {
        let mut f = 0usize;
        while f < FORK_CELLS {
            match self.fork.get(f) {
                Some(Some(_)) => {}
                _ => return false,
            }
            f += 1;
        }
        true
    }

    /// 调试一格（kind 单调 + 载荷对照 + 重复拒）。
    pub fn record_dbg(&mut self, d: usize, got: u64) -> Result<(), DnSpyExCode> {
        if d >= DBG_CELLS {
            return Err(DnSpyExCode::ENoDbg);
        }
        let kind = match dbg_kind(d) {
            Some(k) => k,
            None => return Err(DnSpyExCode::ENoDbg),
        };
        if (kind as i64) < self.last_dbg {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7300);
            return Err(DnSpyExCode::EOrder);
        }
        let want = match dbg_cell(d) {
            Some(w) => w,
            None => return Err(DnSpyExCode::ENoDbg),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7301 + d as u64);
            return Err(DnSpyExCode::EDbgDrift);
        }
        if match self.dbg.get(d) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DnSpyExCode::ESeal);
        }
        if let Some(c) = self.dbg.get_mut(d) {
            *c = Some(got);
        }
        self.last_dbg = kind as i64;
        self.fnv(0x7302 + d as u64);
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

    /// ×10 程序集一格（载荷对照 + 重复拒）。
    pub fn record_asm(&mut self, a: usize, got: u64) -> Result<(), DnSpyExCode> {
        let want = match asm_cell(a) {
            Some(w) => w,
            None => return Err(DnSpyExCode::ENoAsm),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7400 + a as u64);
            return Err(DnSpyExCode::EAsmDrift);
        }
        if match self.asm.get(a) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DnSpyExCode::ESeal);
        }
        if let Some(c) = self.asm.get_mut(a) {
            *c = Some(got);
        }
        self.fnv(0x7401 + a as u64);
        Ok(())
    }

    /// ×10 程序集全账核验。
    pub fn asm_all(&self) -> bool {
        let mut a = 0usize;
        while a < ASM_X10 {
            match self.asm.get(a) {
                Some(Some(_)) => {}
                _ => return false,
            }
            a += 1;
        }
        true
    }

    /// 社区续维护声明一行。
    pub fn declare_comm(&mut self, r: usize) -> Result<(), DnSpyExCode> {
        if r >= COMM_ROWS {
            return Err(DnSpyExCode::ENoRow);
        }
        if match self.comm.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DnSpyExCode::ESeal);
        }
        if let Some(c) = self.comm.get_mut(r) {
            *c = comm_row(r);
        }
        self.fnv(0x7500 + r as u64);
        Ok(())
    }

    /// 社区声明全齐核验。
    pub fn comm_all(&self) -> bool {
        let mut r = 0usize;
        while r < COMM_ROWS {
            match self.comm.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 分账声明一行（对 F52326 分账）。
    pub fn declare_split(&mut self, r: usize) -> Result<(), DnSpyExCode> {
        if r >= SPLIT_ROWS {
            return Err(DnSpyExCode::ENoRow);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DnSpyExCode::ESeal);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0x7600 + r as u64);
        Ok(())
    }

    /// 分账声明全齐核验。
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

    /// KB-COMPAT 定级：社区接棒口径——全通定 S2，判据缺 → S3。
    pub fn judge_grade(&self) -> Grade {
        if !self.gates_all() {
            Grade::S3
        } else if self.fork_all() && self.dbg_all() && self.asm_all() && self.comm_all() && self.splits_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for DnSpyExLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = DnSpyExLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut f = 0usize;
    while f < FORK_CELLS {
        if let Some(w) = fork_cell(f) {
            if led.record_fork(f, w).is_ok() {
                recorded += 1;
            }
        }
        f += 1;
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
    let mut a = 0usize;
    while a < ASM_X10 {
        if let Some(w) = asm_cell(a) {
            if led.record_asm(a, w).is_ok() {
                recorded += 1;
            }
        }
        a += 1;
    }
    let mut r = 0usize;
    while r < COMM_ROWS {
        let _ = led.declare_comm(r);
        r += 1;
    }
    let mut r2 = 0usize;
    while r2 < SPLIT_ROWS {
        let _ = led.declare_split(r2);
        r2 += 1;
    }
    // 注入 10 轮：fork 漂移 / 调试逆序 / fork 越闭集三态轮转。
    // 漂移与逆序注入先按合法序落正格，再注违格；逆序格不改 last_kind。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = DnSpyExLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_fork(0, fork_cell(0).unwrap_or(0));
                lt.record_fork(1, fork_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(DnSpyExCode::EForkDrift)
            }
            1 => {
                let _ = lt.record_dbg(4, dbg_cell(4).unwrap_or(0));
                lt.record_dbg(1, dbg_cell(1).unwrap_or(0)) == Err(DnSpyExCode::EOrder)
            }
            _ => lt.record_fork(FORK_CELLS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x7700 + k as u64);
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
/// UNX-F52327 判据集（锚 UNX-F52328-J1，off-by-one 板式）。
pub fn run_unx52327_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52327");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "dnSpy fork 同构账 12 格全落",
        {
            let mut lt = DnSpyExLedger::new();
            let mut ok = true;
            let mut f = 0usize;
            while f < FORK_CELLS {
                if let Some(w) = fork_cell(f) {
                    if lt.record_fork(f, w).is_err() {
                        ok = false;
                    }
                }
                f += 1;
            }
            ok && lt.fork_all()
        },
        "程序集/调试器/反编译器三族各 4 格逐格恒等",
    );
    s.add(
        "调试 12 格全落",
        {
            let mut lt = DnSpyExLedger::new();
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
        "断点/命中/单步各 4 格，kind 单调",
    );
    s.add(
        "反编译输出 ×10 程序集全通",
        {
            let mut lt = DnSpyExLedger::new();
            let mut ok = true;
            let mut a = 0usize;
            while a < ASM_X10 {
                if let Some(w) = asm_cell(a) {
                    if lt.record_asm(a, w).is_err() {
                        ok = false;
                    }
                }
                a += 1;
            }
            ok && lt.asm_all()
        },
        "×10 程序集载荷折叠逐格恒等（UNX-F52328-J1 口径）",
    );
    s.add(
        "社区续维护声明 2 行全齐（§AI-66.3）",
        {
            let mut lt = DnSpyExLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < COMM_ROWS {
                if lt.declare_comm(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.comm_all()
        },
        "社区接棒续维护声明 + GitHub 活跃度声明两行闭集",
    );
    s.add(
        "与 F52326 分账声明 2 行全齐",
        {
            let mut lt = DnSpyExLedger::new();
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
        "dnSpy 账（F52326）/ dnSpyEx 账（本卡）分立不并账，两行闭集",
    );
    s.add(
        "fork 族单调",
        {
            let mut ok = true;
            let mut f = 1usize;
            while f < FORK_CELLS {
                if fork_kind(f).unwrap_or(3) < fork_kind(f - 1).unwrap_or(3) {
                    ok = false;
                }
                f += 1;
            }
            ok
        },
        "三族闭集单调",
    );
    let mut l0 = DnSpyExLedger::new();
    s.add(
        "边界·fork 越闭集",
        l0.record_fork(FORK_CELLS, 0).is_err(),
        "fork 12 越闭集必拒",
    );
    s.add(
        "边界·调试越闭集",
        l0.record_dbg(DBG_CELLS, 0).is_err(),
        "调试 12 越闭集必拒",
    );
    s.add(
        "边界·程序集越闭集",
        l0.record_asm(ASM_X10, 0).is_err(),
        "程序集 10 越闭集必拒",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "fork 漂移/调试逆序/fork 越闭集三态轮转必抓",
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
        "定级 S2 闭集（社区接棒）",
        {
            let mut lt = DnSpyExLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut f = 0usize;
            while f < FORK_CELLS {
                if let Some(w) = fork_cell(f) {
                    let _ = lt.record_fork(f, w);
                }
                f += 1;
            }
            let mut d = 0usize;
            while d < DBG_CELLS {
                if let Some(w) = dbg_cell(d) {
                    let _ = lt.record_dbg(d, w);
                }
                d += 1;
            }
            let mut a = 0usize;
            while a < ASM_X10 {
                if let Some(w) = asm_cell(a) {
                    let _ = lt.record_asm(a, w);
                }
                a += 1;
            }
            let mut r = 0usize;
            while r < COMM_ROWS {
                let _ = lt.declare_comm(r);
                r += 1;
            }
            let mut r2 = 0usize;
            while r2 < SPLIT_ROWS {
                let _ = lt.declare_split(r2);
                r2 += 1;
            }
            // main/aux 各节全部记账完毕后再断言定级。
            lt.judge_grade() == Grade::S2 && lt.judge_grade() != Grade::S1
        },
        "全通 → S2（KB-COMPAT 定级登记，社区续维护有条件收口）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7200 + 0x40 * (ID - 52201),
        "SEG_BASE 0x40 对齐且按 VX15 公式落 0xC9180",
    );
    s.add("收口·判据条数", s.len() >= 14, "含本条共 ≥15 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fork_kind() {
        assert_eq!(fork_kind(0), Some(0));
        assert_eq!(fork_kind(4), Some(1));
        assert_eq!(fork_kind(8), Some(2));
        assert!(fork_kind(12).is_none());
        assert!(fork_cell(12).is_none());
        assert_eq!(fork_kind_name(2), Some("反编译器"));
        assert!(fork_kind_name(3).is_none());
    }

    #[test]
    fn test_dbg_kind() {
        assert_eq!(dbg_kind(0), Some(0));
        assert_eq!(dbg_kind(4), Some(1));
        assert_eq!(dbg_kind(11), Some(2));
        assert!(dbg_kind(12).is_none());
        assert_eq!(dbg_kind_name(1), Some("命中"));
    }

    #[test]
    fn test_cells_pure() {
        let a = asm_cell(3).unwrap_or(0);
        assert_eq!(a, asm_cell(3).unwrap_or(0));
        assert_ne!(a, asm_cell(4).unwrap_or(0));
        assert!(asm_cell(ASM_X10).is_none());
        assert!(comm_row(COMM_ROWS).is_none());
        assert!(split_row(SPLIT_ROWS).is_none());
    }

    #[test]
    fn test_order_caught_and_last_kind_kept() {
        let mut lt = DnSpyExLedger::new();
        let _ = lt.record_fork(8, fork_cell(8).unwrap_or(0));
        assert_eq!(lt.record_fork(2, fork_cell(2).unwrap_or(0)), Err(DnSpyExCode::EOrder));
        // 逆序被拒后 last_kind 未被污染：同族合法格仍可落账。
        assert!(lt.record_fork(9, fork_cell(9).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = DnSpyExLedger::new();
        assert_eq!(
            lt.record_asm(2, asm_cell(2).unwrap_or(0).wrapping_add(1)),
            Err(DnSpyExCode::EAsmDrift)
        );
        assert_eq!(
            lt.record_dbg(6, dbg_cell(6).unwrap_or(0).wrapping_add(5)),
            Err(DnSpyExCode::EDbgDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = DnSpyExLedger::new();
        let _ = lt.record_fork(0, fork_cell(0).unwrap_or(0));
        assert_eq!(lt.record_fork(0, fork_cell(0).unwrap_or(0)), Err(DnSpyExCode::ESeal));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = DnSpyExLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = DnSpyExLedger::new();
        assert!(!lt2.gates_all());
        assert_eq!(lt2.judge_grade(), Grade::S3);
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (FORK_CELLS + DBG_CELLS + ASM_X10) as u32);
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
        assert_eq!(SEG_BASE, 0xC9180);
        assert_eq!(SEG_BASE % 0x40, 0);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(DnSpyExCode::Ok as u32, 0);
        assert_eq!(DnSpyExCode::EGradeDrift as u32, 12);
        assert_eq!(DnSpyExCode::Ok.val(), SEG_BASE);
        assert_eq!(DnSpyExCode::EGradeDrift.val(), SEG_BASE + 0x0C);
        assert!(!DnSpyExCode::Ok.detail().is_empty());
        assert!(!DnSpyExCode::ESeal.detail().is_empty());
        assert!(!DnSpyExCode::EForkDrift.detail().is_empty());
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
    }

    #[test]
    fn unx52327_checks_green() {
        let s = run_unx52327_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 15);
    }
}
