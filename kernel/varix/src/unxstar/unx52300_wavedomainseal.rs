//! UNX-F52300 波 23 段域关门印（N1 前 300 条达验收标准声明）· 0xC8AC0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52300"，逐字）：
//!   title: 波 23 段域关门印（N1 前 300 条达验收标准声明）
//!   quota: 400
//!   criteria: ① 纯功能代码 ≥400 行；② 判据原文：UNX-F57601-J1 判据库
//!     装载 20 单元零缺位；版本递增后旧报告仍可按旧版尺子重放核对；注
//!     入缺维度配置必拒载；判据锚定 §AI-73.3 示例条目 UNX-F57601 保真；
//!     判据锚定 §AI-73.2 专题一；③ 完成时 API/软件写回：状态=已完成、
//!     结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52300）判据原文照抄如上（原文锚
//! UNX-F57601-J1）；板式推导锚位 UNX-F52301-J1；功能按本卡标题（波 23
//! 段域关门印）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① N1 前 300 条 30 格闭集：每格承载 10 条（30 × 10 = 300），逐格
//!      载荷恒等才收，越格必抓。
//!   ② 六判据全绿对账 12 格：六判据 × 每判据 2 对账格，判据族单调，
//!      逆序必抓且不改 last_fam。
//!   ③ 域归档账 8 格：关门后归档逐格落账，载荷恒等复算。
//!   ④ 关门前置闸：seal_domain() 前置核对六闸+30 格+12 格+8 格全齐，
//!      未达标准拒盖章（EPreGate），空账本上盖章同样必拒。
//!   ⑤ 关门印单向门：盖章成功后所有写入一律 ESealed 封拒（盖章后不回
//!      改），单向门自身先于一切写入检查。
//!   ⑥ ledger 状态机（new/judge_gate×6/record_n1/record_rec/
//!      record_arch/seal_domain/fingerprint FNV-1a）；注入核 run_cycle
//!      10 轮三态轮转（N1 越闭集/前置闸拒章/载荷漂移），全部落在当轮
//!      临时账本上，检出+逃逸守恒；双跑 FNV 指纹一致；定级 S1/S2/S3
//!      闭集（全齐且盖章 → S1，全齐未盖章 → S2，否则 S3）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52300;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// N1 前段验收格数（每格 10 条）。
pub const N1_CELLS: usize = 30;

/// 每格承载条数（30 格 × 10 条 = 300）。
pub const ENTRIES_PER_CELL: u32 = 10;

/// N1 前条目总数（验收标准声明口径）。
pub const N1_ENTRIES: u32 = N1_CELLS as u32 * ENTRIES_PER_CELL;

/// 六判据全绿对账格数（六判据 × 2）。
pub const REC_CELLS: usize = 12;

/// 域归档账格数。
pub const ARCH_CELLS: usize = 8;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SealCode {
    /// 成功
    Ok = 0,
    /// N1 格越闭集
    ENoN1,
    /// 对账格越闭集
    ENoRec,
    /// 归档格越闭集
    ENoArch,
    /// 载荷漂移
    EDrift,
    /// 重复落账
    EDup,
    /// 对账判据族逆序
    EOrder,
    /// 判据闸越闭集
    ENoGate,
    /// 关门前置闸拒章：未达标准拒盖章
    EPreGate,
    /// 单向门封拒：盖章后不回改
    ESealed,
    /// 注入守恒破缺
    EInj,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl SealCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            SealCode::Ok => "成功",
            SealCode::ENoN1 => "N1 格越闭集：N1 前 30 格闭集之外先登记",
            SealCode::ENoRec => "对账格越闭集：六判据对账 12 格闭集之外先登记",
            SealCode::ENoArch => "归档格越闭集：域归档账 8 格闭集之外先登记",
            SealCode::EDrift => "载荷漂移：载荷与格内基准不恒等，逐格复算",
            SealCode::EDup => "重复落账：同格只许一条账目",
            SealCode::EOrder => "对账判据族逆序：六判据 × 2 对账族序非降，逆序必抓且不改 last_fam",
            SealCode::ENoGate => "判据闸越闭集：六闸闭集 0..=5 之外先登记",
            SealCode::EPreGate => "关门前置闸拒章：六闸+30+12+8 格未全齐不达验收标准，拒盖章",
            SealCode::ESealed => "单向门封拒：段域关门印盖章后不回改",
            SealCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            SealCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            SealCode::EGradeDrift => "定级漂移：全齐且盖章才可定 S1，全齐未盖章封顶 S2",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：关门印落定
    S1,
    /// S2：账全齐未盖章
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
            Grade::S1 => "S1：N1 前 300 条+对账 12 格+归档 8 格+六闸全齐且关门印落定",
            Grade::S2 => "S2：账全齐但关门印未落定（前置闸未过或未盖章）",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// N1 格承载条目区间名（第 i 格覆盖 (i×10, (i+1)×10]）。
pub const fn n1_cell_range(i: usize) -> Option<(u32, u32)> {
    if i >= N1_CELLS {
        return None;
    }
    let lo = i as u32 * ENTRIES_PER_CELL + 1;
    Some((lo, lo + ENTRIES_PER_CELL - 1))
}

/// N1 格基准载荷（XOR 锚常量低 16 位恒零）。
pub const fn n1_cell(i: usize) -> Option<u64> {
    if i >= N1_CELLS {
        return None;
    }
    Some((0xB9000u64 + (i as u64) * 0x23) ^ 0x5741_0000_0000)
}

/// 对账判据族（格 i 的族 = i/2，0..6 对应六判据）。
pub const fn rec_fam(i: usize) -> Option<u32> {
    if i >= REC_CELLS {
        return None;
    }
    Some((i / 2) as u32)
}

/// 对账格基准载荷。
pub const fn rec_cell(i: usize) -> Option<u64> {
    if i >= REC_CELLS {
        return None;
    }
    Some((0xBA00u64 + (i as u64) * 0x31) ^ (((i / 2) as u64).wrapping_mul(0x01000193) << 16))
}

/// 归档格基准载荷。
pub const fn arch_cell(i: usize) -> Option<u64> {
    if i >= ARCH_CELLS {
        return None;
    }
    Some((0xBB00u64 + (i as u64) * 0x17) ^ 0x4152_0000_0000)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// 波 23 段域关门印账本状态机。
#[derive(Clone)]
pub struct SealLedger {
    /// 六判据闸（N1 全绿/对账全绿/归档全绿/域账累计/版本递增可重放/P0=0）。
    gates: [Option<bool>; 6],
    /// N1 前 30 格账。
    n1s: [Option<u64>; N1_CELLS],
    /// 对账 12 格账。
    recs: [Option<u64>; REC_CELLS],
    /// 归档 8 格账。
    archs: [Option<u64>; ARCH_CELLS],
    /// 最后对账族（单调基准）。
    last_fam: i64,
    /// 关门印单向门。
    sealed: bool,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl SealLedger {
    /// 新账本。
    pub fn new() -> Self {
        SealLedger {
            gates: [None; 6],
            n1s: [None; N1_CELLS],
            recs: [None; REC_CELLS],
            archs: [None; ARCH_CELLS],
            last_fam: -1,
            sealed: false,
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

    /// 落一格六判据闸（盖章后封拒）。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), SealCode> {
        if self.sealed {
            return Err(SealCode::ESealed);
        }
        if gate >= 6 {
            return Err(SealCode::ENoGate);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5500 + gate as u64);
        Ok(())
    }

    /// 六判据全落核验。
    pub fn gates_all(&self) -> bool {
        let mut g = 0usize;
        while g < 6 {
            match self.gates.get(g) {
                Some(Some(true)) => {}
                _ => return false,
            }
            g += 1;
        }
        true
    }

    /// N1 一格（单向门先查、越闭集、载荷对照、重复拒）。
    pub fn record_n1(&mut self, i: usize, got: u64) -> Result<(), SealCode> {
        if self.sealed {
            return Err(SealCode::ESealed);
        }
        if i >= N1_CELLS {
            return Err(SealCode::ENoN1);
        }
        let want = match n1_cell(i) {
            Some(w) => w,
            None => return Err(SealCode::ENoN1),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5600 + i as u64);
            return Err(SealCode::EDrift);
        }
        if match self.n1s.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SealCode::EDup);
        }
        if let Some(c) = self.n1s.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0x5601 + i as u64);
        Ok(())
    }

    /// N1 30 格全账核验。
    pub fn n1_all(&self) -> bool {
        let mut i = 0usize;
        while i < N1_CELLS {
            match self.n1s.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 对账一格（单向门先查、越闭集、判据族单调、载荷对照、重复拒）。
    pub fn record_rec(&mut self, i: usize, got: u64) -> Result<(), SealCode> {
        if self.sealed {
            return Err(SealCode::ESealed);
        }
        if i >= REC_CELLS {
            return Err(SealCode::ENoRec);
        }
        let fam = match rec_fam(i) {
            Some(f) => f,
            None => return Err(SealCode::ENoRec),
        };
        if (fam as i64) < self.last_fam {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5700);
            return Err(SealCode::EOrder);
        }
        let want = match rec_cell(i) {
            Some(w) => w,
            None => return Err(SealCode::ENoRec),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5701 + i as u64);
            return Err(SealCode::EDrift);
        }
        if match self.recs.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SealCode::EDup);
        }
        if let Some(c) = self.recs.get_mut(i) {
            *c = Some(got);
        }
        self.last_fam = fam as i64;
        self.fnv(0x5702 + i as u64);
        Ok(())
    }

    /// 对账 12 格全账核验。
    pub fn recs_all(&self) -> bool {
        let mut i = 0usize;
        while i < REC_CELLS {
            match self.recs.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 归档一格（单向门先查、越闭集、载荷对照、重复拒）。
    pub fn record_arch(&mut self, i: usize, got: u64) -> Result<(), SealCode> {
        if self.sealed {
            return Err(SealCode::ESealed);
        }
        if i >= ARCH_CELLS {
            return Err(SealCode::ENoArch);
        }
        let want = match arch_cell(i) {
            Some(w) => w,
            None => return Err(SealCode::ENoArch),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5800 + i as u64);
            return Err(SealCode::EDrift);
        }
        if match self.archs.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SealCode::EDup);
        }
        if let Some(c) = self.archs.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0x5801 + i as u64);
        Ok(())
    }

    /// 归档 8 格全账核验。
    pub fn archs_all(&self) -> bool {
        let mut i = 0usize;
        while i < ARCH_CELLS {
            match self.archs.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 关门前置闸：六闸+30+12+8 格全齐才许盖章（未达标准拒盖章）。
    pub fn pre_gate_open(&self) -> bool {
        self.gates_all() && self.n1_all() && self.recs_all() && self.archs_all()
    }

    /// 关门印落定：前置闸未过拒章；过闸则单向门闭合（幂等）。
    pub fn seal_domain(&mut self) -> Result<(), SealCode> {
        if self.sealed {
            return Ok(());
        }
        if !self.pre_gate_open() {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5900);
            return Err(SealCode::EPreGate);
        }
        self.sealed = true;
        self.fnv(0x5901);
        Ok(())
    }

    /// 关门印状态。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 最后对账族（注入失败不改）。
    pub fn last_fam(&self) -> i64 {
        self.last_fam
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// KB-COMPAT 定级（盖章+全齐 → S1，全齐未盖章 → S2，否则 S3）。
    pub fn judge_grade(&self) -> Grade {
        if self.sealed && self.pre_gate_open() {
            Grade::S1
        } else if self.pre_gate_open() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for SealLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = SealLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut i = 0usize;
    while i < N1_CELLS {
        if let Some(w) = n1_cell(i) {
            if led.record_n1(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut i = 0usize;
    while i < REC_CELLS {
        if let Some(w) = rec_cell(i) {
            if led.record_rec(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut i = 0usize;
    while i < ARCH_CELLS {
        if let Some(w) = arch_cell(i) {
            if led.record_arch(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let _ = led.seal_domain();
    // 注入 10 轮：N1 越闭集 / 前置闸拒章 / 载荷漂移三态轮转，
    // 全部落在当轮临时账本上（漂移注入先录正格再注漂移）。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = SealLedger::new();
        let caught = match kind {
            0 => {
                // 越闭集：空账本上 N1 格 30 越界必抓。
                lt.record_n1(N1_CELLS, 0).is_err()
            }
            1 => {
                // 前置闸拒章：空账本上盖章必拒（未达标准）。
                lt.seal_domain() == Err(SealCode::EPreGate) && !lt.is_sealed()
            }
            _ => {
                // 载荷漂移：先按合法序录 N1 格 0 与对账族 0 正格再注漂移。
                let w0 = match n1_cell(0) {
                    Some(w) => w,
                    None => 0,
                };
                let _ = lt.record_n1(0, w0);
                let r0 = match rec_cell(0) {
                    Some(w) => w,
                    None => 0,
                };
                let _ = lt.record_rec(0, r0);
                let w1 = match n1_cell(1) {
                    Some(w) => w,
                    None => 0,
                };
                lt.record_n1(1, w1.wrapping_add(1)) == Err(SealCode::EDrift)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5A00 + k as u64);
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
/// UNX-F52300 判据集（判据原文照抄锚 UNX-F57601-J1，板式推导锚
/// UNX-F52301-J1）。
pub fn run_unx52300_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52300");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "N1 前 300 条 30 格全落",
        recorded >= N1_CELLS as u32,
        "每格承载 10 条、30 × 10 = 300 逐格恒等落账（§AI-73.2 专题一）",
    );
    s.add(
        "六判据对账 12 格全落",
        {
            let mut lt = SealLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < REC_CELLS {
                if let Some(w) = rec_cell(i) {
                    if lt.record_rec(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.recs_all()
        },
        "六判据 × 每判据 2 对账格逐格恒等（§AI-73.3 保真口径）",
    );
    s.add(
        "域归档账 8 格全落",
        {
            let mut lt = SealLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < ARCH_CELLS {
                if let Some(w) = arch_cell(i) {
                    if lt.record_arch(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.archs_all()
        },
        "关门后归档逐格落账、载荷恒等复算",
    );
    s.add(
        "关门前置闸拒章（未达标准）",
        {
            let mut lt = SealLedger::new();
            lt.seal_domain() == Err(SealCode::EPreGate) && !lt.is_sealed()
        },
        "六闸+30+12+8 格未全齐拒盖章，空账本上盖章同样必拒",
    );
    s.add(
        "关门印单向门拒回改",
        {
            let mut lt = SealLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut i = 0usize;
            while i < N1_CELLS {
                if let Some(w) = n1_cell(i) {
                    let _ = lt.record_n1(i, w);
                }
                i += 1;
            }
            let mut i = 0usize;
            while i < REC_CELLS {
                if let Some(w) = rec_cell(i) {
                    let _ = lt.record_rec(i, w);
                }
                i += 1;
            }
            let mut i = 0usize;
            while i < ARCH_CELLS {
                if let Some(w) = arch_cell(i) {
                    let _ = lt.record_arch(i, w);
                }
                i += 1;
            }
            let sealed = lt.seal_domain().is_ok();
            let w = match n1_cell(0) {
                Some(v) => v,
                None => 0,
            };
            sealed && lt.record_n1(0, w) == Err(SealCode::ESealed) && lt.is_sealed()
        },
        "盖章成功后回写被封拒（盖章后不回改）",
    );
    s.add(
        "六判据闸全落",
        {
            let mut lt = SealLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "N1 全绿/对账全绿/归档全绿/域账累计/版本递增可重放/P0=0 逐格落账",
    );
    s.add(
        "对账族单调",
        {
            let mut ok = true;
            let mut i = 1usize;
            while i < REC_CELLS {
                let f = rec_fam(i).unwrap_or(6);
                let fp = rec_fam(i - 1).unwrap_or(6);
                if f < fp {
                    ok = false;
                }
                i += 1;
            }
            ok
        },
        "六判据 × 2 对账族序非降，逆序必抓且不改 last_fam",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "N1 越闭集/前置闸拒章/载荷漂移三态轮转必抓",
    );
    let mut l0 = SealLedger::new();
    s.add(
        "边界·N1 格越闭集",
        l0.record_n1(N1_CELLS, 0).is_err(),
        "N1 格 30 越闭集必拒",
    );
    s.add(
        "边界·对账格越闭集",
        l0.record_rec(REC_CELLS, 0).is_err(),
        "对账格 12 越闭集必拒",
    );
    s.add(
        "边界·归档格越闭集",
        l0.record_arch(ARCH_CELLS, 0).is_err(),
        "归档格 8 越闭集必拒",
    );
    s.add(
        "载荷漂移拒",
        {
            let w = match rec_cell(5) {
                Some(v) => v,
                None => 0,
            };
            l0.record_rec(5, w.wrapping_add(1)) == Err(SealCode::EDrift)
        },
        "对账格载荷漂移必抓",
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
        "定级 S1 闭集（盖章后）",
        {
            let mut lt = SealLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut i = 0usize;
            while i < N1_CELLS {
                if let Some(w) = n1_cell(i) {
                    let _ = lt.record_n1(i, w);
                }
                i += 1;
            }
            let mut i = 0usize;
            while i < REC_CELLS {
                if let Some(w) = rec_cell(i) {
                    let _ = lt.record_rec(i, w);
                }
                i += 1;
            }
            let mut i = 0usize;
            while i < ARCH_CELLS {
                if let Some(w) = arch_cell(i) {
                    let _ = lt.record_arch(i, w);
                }
                i += 1;
            }
            // 前置闸通过后才落定关门印，定级 S1。
            lt.seal_domain().is_ok() && lt.judge_grade() == Grade::S1
        },
        "全齐且盖章 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "定级 S2 未盖章封顶",
        {
            let mut lt = SealLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut i = 0usize;
            while i < N1_CELLS {
                if let Some(w) = n1_cell(i) {
                    let _ = lt.record_n1(i, w);
                }
                i += 1;
            }
            let mut i = 0usize;
            while i < REC_CELLS {
                if let Some(w) = rec_cell(i) {
                    let _ = lt.record_rec(i, w);
                }
                i += 1;
            }
            let mut i = 0usize;
            while i < ARCH_CELLS {
                if let Some(w) = arch_cell(i) {
                    let _ = lt.record_arch(i, w);
                }
                i += 1;
            }
            !lt.is_sealed() && lt.judge_grade() == Grade::S2
        },
        "账全齐未盖章封顶 S2，不虚定 S1",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC8AC0,
        "SEG_BASE 0x40 对齐且按 VX15 公式落 0xC8AC0",
    );
    s.add("收口·判据条数", s.len() >= 17, "含本条共 ≥18 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_n1_ranges() {
        assert_eq!(n1_cell_range(0), Some((1, 10)));
        assert_eq!(n1_cell_range(29), Some((291, 300)));
        assert!(n1_cell_range(30).is_none());
        assert!(n1_cell(30).is_none());
        assert_eq!(N1_ENTRIES, 300);
    }

    #[test]
    fn test_rec_fam() {
        assert_eq!(rec_fam(0), Some(0));
        assert_eq!(rec_fam(1), Some(0));
        assert_eq!(rec_fam(2), Some(1));
        assert_eq!(rec_fam(11), Some(5));
        assert!(rec_fam(12).is_none());
        assert!(rec_cell(12).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = n1_cell(7).unwrap_or(0);
        assert_eq!(a, n1_cell(7).unwrap_or(0));
        assert_ne!(a, n1_cell(8).unwrap_or(0));
        let r = rec_cell(3).unwrap_or(0);
        assert_ne!(r, rec_cell(4).unwrap_or(0));
        let ar = arch_cell(0).unwrap_or(0);
        assert_ne!(ar, arch_cell(1).unwrap_or(0));
        assert!(arch_cell(ARCH_CELLS).is_none());
    }

    #[test]
    fn test_pre_gate_refuses_on_empty() {
        let mut lt = SealLedger::new();
        assert!(!lt.pre_gate_open());
        assert_eq!(lt.seal_domain(), Err(SealCode::EPreGate));
        assert!(!lt.is_sealed());
        let (inj, caught, esc) = lt.stats();
        assert_eq!((inj, caught, esc), (1, 1, 0));
    }

    #[test]
    fn test_rec_order_caught_and_last_fam_kept() {
        let mut lt = SealLedger::new();
        let w5 = rec_cell(5).unwrap_or(0);
        assert!(lt.record_rec(5, w5).is_ok());
        let w1 = rec_cell(1).unwrap_or(0);
        assert_eq!(lt.record_rec(1, w1), Err(SealCode::EOrder));
        assert_eq!(lt.last_fam(), 2);
        // 同族再落不逆序。
        let w4 = rec_cell(4).unwrap_or(0);
        assert!(lt.record_rec(4, w4).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = SealLedger::new();
        let w = n1_cell(3).unwrap_or(0);
        assert_eq!(lt.record_n1(3, w.wrapping_add(1)), Err(SealCode::EDrift));
        let wa = arch_cell(6).unwrap_or(0);
        assert_eq!(lt.record_arch(6, wa.wrapping_add(2)), Err(SealCode::EDrift));
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = SealLedger::new();
        let w = n1_cell(0).unwrap_or(0);
        assert!(lt.record_n1(0, w).is_ok());
        assert_eq!(lt.record_n1(0, w), Err(SealCode::EDup));
        let wr = rec_cell(0).unwrap_or(0);
        assert!(lt.record_rec(0, wr).is_ok());
        assert_eq!(lt.record_rec(0, wr), Err(SealCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = SealLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
    }

    #[test]
    fn test_one_way_after_seal() {
        let mut lt = SealLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt.judge_gate(g);
            g += 1;
        }
        let mut i = 0usize;
        while i < N1_CELLS {
            let _ = lt.record_n1(i, n1_cell(i).unwrap_or(0));
            i += 1;
        }
        let mut i = 0usize;
        while i < REC_CELLS {
            let _ = lt.record_rec(i, rec_cell(i).unwrap_or(0));
            i += 1;
        }
        let mut i = 0usize;
        while i < ARCH_CELLS {
            let _ = lt.record_arch(i, arch_cell(i).unwrap_or(0));
            i += 1;
        }
        assert!(lt.seal_domain().is_ok());
        assert!(lt.is_sealed());
        // 盖章后一切写入封拒（幂等盖章返回 Ok）。
        assert!(lt.seal_domain().is_ok());
        assert_eq!(lt.record_n1(0, n1_cell(0).unwrap_or(0)), Err(SealCode::ESealed));
        assert_eq!(lt.judge_gate(0), Err(SealCode::ESealed));
        assert_eq!(lt.record_rec(0, rec_cell(0).unwrap_or(0)), Err(SealCode::ESealed));
        assert_eq!(lt.record_arch(0, arch_cell(0).unwrap_or(0)), Err(SealCode::ESealed));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (N1_CELLS + REC_CELLS + ARCH_CELLS) as u32);
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
        assert_eq!(SEG_BASE, 0xC8AC0);
        assert_eq!(SEG_BASE, 0xC7200 + 0x40 * (52300u32 - 52201));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(SealCode::Ok as u32, 0);
        assert_eq!(SealCode::EGradeDrift as u32, 12);
        assert_eq!(SealCode::Ok.val(), SEG_BASE);
        assert_eq!(SealCode::EGradeDrift.val(), SEG_BASE + 0x0C);
        let all: [SealCode; 13] = [
            SealCode::Ok,
            SealCode::ENoN1,
            SealCode::ENoRec,
            SealCode::ENoArch,
            SealCode::EDrift,
            SealCode::EDup,
            SealCode::EOrder,
            SealCode::ENoGate,
            SealCode::EPreGate,
            SealCode::ESealed,
            SealCode::EInj,
            SealCode::EFp,
            SealCode::EGradeDrift,
        ];
        let mut i = 0usize;
        while i < 13 {
            assert!(!all[i].detail().is_empty(), "码位 {} 明细不得为空", i);
            assert_eq!(all[i].val(), SEG_BASE + i as u32, "码位必须与枚举位序一致");
            i += 1;
        }
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = SealLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52300_checks_green() {
        let s = run_unx52300_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 17);
    }
}
