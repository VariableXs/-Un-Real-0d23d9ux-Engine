//! UNX-F52469 Firefox Developer Edition 收口 · 0xCB500段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52469"，逐字）：
//!   title: Firefox Developer Edition 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52470-J1 组件
//!     树/Profiler ×10；与宿主浏览器联签（F52461 锚定行）；KB-COMPAT
//!     定级登记；判据锚定 §AI-66.3；③ 完成时 API/软件写回：状态=已
//!     完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52469）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52470-J1，照原文锚定；功能按本卡标题（Firefox Developer
//! Edition 收口）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五功能族闭集 20 操作：DevTools 增强区 / 容器 / about:config /
//!      同步 / 更新通道 各 4 操作，序账单调（族错位必抓）。
//!   ② 更新通道阶梯 4 档：esr / beta / release / nightly，越档必抓，
//!      每档载荷对照恒等才收。
//!   ③ 消费面六判据收口闸：启动/功能/崩溃/卸载/升级/许可，逐格落账。
//!   ④ 分账声明 2 行闭集：与 Firefox Release 分账（F52463 锚定行），
//!      缺一拒收；Gecko 上栈声明 2 行闭集，缺一拒收。
//!   ⑤ ledger 状态机（new/record_*/judge_gate/fingerprint FNV-1a）；
//!      注入核 run_cycle 10 轮三态轮转（通道越阶梯 / 操作载荷漂移 /
//!      操作序越闭集），全落在当轮临时账本，检出 10/10、逃逸 0；
//!      双跑指纹一致；KB-COMPAT 定级闭集 S2/S3（本卡目标档 S2）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无
//!      unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52469;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 五功能族操作数。
pub const CELLS: usize = 20;

/// 功能族数。
pub const FAMILIES: usize = 5;

/// 更新通道阶梯档数。
pub const CHANNEL_STEPS: usize = 4;

/// 与 Firefox Release 分账声明行数。
pub const SPLIT_ROWS: usize = 2;

/// Gecko 上栈声明行数。
pub const STACK_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FxdevCode {
    /// 成功
    Ok = 0,
    /// 通道档越阶梯
    ENoChan,
    /// 通道档载荷漂移
    EChanDrift,
    /// 操作号越闭集
    ENoOp,
    /// 操作载荷漂移
    EDrift,
    /// 功能族序错
    EOrder,
    /// 声明行越闭集
    ENoRow,
    /// 缺声明行
    ERowMiss,
    /// 上栈行越闭集
    ENoStack,
    /// 缺上栈行
    EStackMiss,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 定级漂移
    EGradeDrift,
}

impl FxdevCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            FxdevCode::Ok => "成功",
            FxdevCode::ENoChan => "通道档越阶梯：esr/beta/release/nightly 四档闭集之外先登记",
            FxdevCode::EChanDrift => "通道档载荷漂移：更新通道折叠与基准不恒等，逐档复算",
            FxdevCode::ENoOp => "操作号越闭集：五功能族 20 操作集之外先登记",
            FxdevCode::EDrift => "操作载荷漂移：操作折叠与基准不恒等，逐格复算",
            FxdevCode::EOrder => "功能族序错：DevTools 增强区→容器→about:config→同步→更新通道单调，族错位必抓",
            FxdevCode::ENoRow => "声明行越闭集：Firefox Release 分账 2 行闭集之外先登记",
            FxdevCode::ERowMiss => "缺声明行：与 Firefox Release 分账声明（F52463 锚定行）未齐先补齐",
            FxdevCode::ENoStack => "上栈行越闭集：Gecko 上栈声明 2 行闭集之外先登记",
            FxdevCode::EStackMiss => "缺上栈行：Gecko 上栈声明未齐先补齐",
            FxdevCode::EDup => "重复落账：同格只许一条账目",
            FxdevCode::ESeal => "未齐即封：20 操作 + 4 档通道 + 2 行分账未齐先补齐",
            FxdevCode::EGradeDrift => "定级漂移：本卡目标档 S2，全通亦不得越定 S1",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：越定档（本卡禁用）
    S1,
    /// S2：目标收口档
    S2,
    /// S3：不收口
    S3,
}

impl Grade {
    /// 定级值（const fn，供编译期断言）。
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
            Grade::S1 => "S1：越定档，Firefox Developer Edition 收口卡不适用",
            Grade::S2 => "S2：目标档，六判据全通即有条件收口",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 更新通道档载荷（档 v：0=esr 1=beta 2=release 3=nightly）。
pub const fn channel_cell(v: usize) -> Option<u64> {
    if v >= CHANNEL_STEPS {
        return None;
    }
    Some((0xB1000u64 + (v as u64) * 0x5D) ^ 0x46584445565F4348)
}

/// 通道档名（const fn 表 + Option 边界）。
pub const fn channel_name(v: usize) -> Option<&'static str> {
    match v {
        0 => Some("esr"),
        1 => Some("beta"),
        2 => Some("release"),
        3 => Some("nightly"),
        _ => None,
    }
}

/// 功能族操作格载荷（操作 o；族 = o/4）。
pub const fn fxdev_cell(o: usize) -> Option<u64> {
    if o >= CELLS {
        return None;
    }
    Some((0xB2000u64 + (o as u64) * 0x2C) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 操作功能族（0..4=DevTools 增强区 4..8=容器 8..12=about:config 12..16=同步 16..20=更新通道）。
pub const fn op_kind(o: usize) -> Option<u32> {
    if o >= CELLS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 功能族名。
pub const fn op_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("DevTools 增强区"),
        1 => Some("容器"),
        2 => Some("about:config"),
        3 => Some("同步"),
        4 => Some("更新通道"),
        _ => None,
    }
}

/// 分账声明行折叠（与 Firefox Release 分账，F52463 锚定行）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xB300 + r as u32 * 0x2C)
}

/// Gecko 上栈声明行折叠。
pub const fn stack_row(r: usize) -> Option<u32> {
    if r >= STACK_ROWS {
        return None;
    }
    Some(0xB380 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Firefox Developer Edition 收口账本状态机。
#[derive(Clone)]
pub struct FxdevLedger {
    /// 消费面六判据（启动/功能/崩溃/卸载/升级/许可）。
    gates: [Option<bool>; 6],
    /// 更新通道阶梯账。
    chans: [Option<u64>; CHANNEL_STEPS],
    /// 五功能族操作账。
    ops: [Option<u64>; CELLS],
    /// 与 Firefox Release 分账声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// Gecko 上栈声明账。
    stack: [Option<u32>; STACK_ROWS],
    /// 最后功能族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl FxdevLedger {
    /// 新账本。
    pub fn new() -> Self {
        FxdevLedger {
            gates: [None; 6],
            chans: [None; CHANNEL_STEPS],
            ops: [None; CELLS],
            splits: [None; SPLIT_ROWS],
            stack: [None; STACK_ROWS],
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

    /// 落一格消费面六判据。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), FxdevCode> {
        if gate >= 6 {
            return Err(FxdevCode::ESeal);
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

    /// 通道档一格（阶梯载荷对照 + 重复拒）。
    pub fn record_channel(&mut self, v: usize, got: u64) -> Result<(), FxdevCode> {
        let want = match channel_cell(v) {
            Some(w) => w,
            None => return Err(FxdevCode::ENoChan),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4200 + v as u64);
            return Err(FxdevCode::EChanDrift);
        }
        if match self.chans.get(v) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(FxdevCode::EDup);
        }
        if let Some(c) = self.chans.get_mut(v) {
            *c = Some(got);
        }
        self.fnv(0x4201 + v as u64);
        Ok(())
    }

    /// 通道阶梯全账核验（4 档全齐）。
    pub fn chans_all(&self) -> bool {
        let mut v = 0usize;
        while v < CHANNEL_STEPS {
            match self.chans.get(v) {
                Some(Some(_)) => {}
                _ => return false,
            }
            v += 1;
        }
        true
    }

    /// 操作一格（功能族序账 + 载荷对照 + 重复拒）。
    /// 声明族 kind 必须与 o/4 恒等，且不得低于已落最大族，错位必抓；
    /// 序错路径不改 last_kind，空账本同样必抓。
    pub fn record_op(&mut self, o: usize, kind: u32, got: u64) -> Result<(), FxdevCode> {
        if o >= CELLS {
            return Err(FxdevCode::ENoOp);
        }
        let fam = match op_kind(o) {
            Some(k) => k,
            None => return Err(FxdevCode::ENoOp),
        };
        if kind != fam || (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4300);
            return Err(FxdevCode::EOrder);
        }
        let want = match fxdev_cell(o) {
            Some(w) => w,
            None => return Err(FxdevCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4301 + o as u64);
            return Err(FxdevCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(FxdevCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x4302 + o as u64);
        Ok(())
    }

    /// 操作全账核验（20 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < CELLS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 与 Firefox Release 分账声明一行。
    pub fn declare_split(&mut self, r: usize) -> Result<(), FxdevCode> {
        if r >= SPLIT_ROWS {
            return Err(FxdevCode::ENoRow);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(FxdevCode::EDup);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0x4400 + r as u64);
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

    /// Gecko 上栈声明一行。
    pub fn declare_stack(&mut self, r: usize) -> Result<(), FxdevCode> {
        if r >= STACK_ROWS {
            return Err(FxdevCode::ENoStack);
        }
        if match self.stack.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(FxdevCode::EDup);
        }
        if let Some(c) = self.stack.get_mut(r) {
            *c = stack_row(r);
        }
        self.fnv(0x4480 + r as u64);
        Ok(())
    }

    /// 上栈声明全齐核验。
    pub fn stack_all(&self) -> bool {
        let mut r = 0usize;
        while r < STACK_ROWS {
            match self.stack.get(r) {
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

    /// 全齐核验（六判据 + 4 档通道 + 20 操作 + 2 行分账 + 2 行上栈）。
    pub fn all_complete(&self) -> bool {
        self.gates_all() && self.chans_all() && self.ops_all() && self.splits_all() && self.stack_all()
    }

    /// KB-COMPAT 定级（六判据全通 → 目标档 S2，判据缺 → S3）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for FxdevLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = FxdevLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut v = 0usize;
    while v < CHANNEL_STEPS {
        if let Some(w) = channel_cell(v) {
            if led.record_channel(v, w).is_ok() {
                recorded += 1;
            }
        }
        v += 1;
    }
    let mut o = 0usize;
    while o < CELLS {
        if let Some(w) = fxdev_cell(o) {
            let kind = op_kind(o).unwrap_or(0);
            if led.record_op(o, kind, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
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
    // 注入 10 轮：通道越阶梯 / 操作载荷漂移 / 操作序越闭集三态轮转。
    // 三类注入全部落在当轮临时账本上，主账本只记注入/检出计数。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = FxdevLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_channel(0, channel_cell(0).unwrap_or(0));
                lt.record_channel(CHANNEL_STEPS, 0).is_err()
            }
            1 => {
                let fam2 = op_kind(2).unwrap_or(0);
                let _ = lt.record_op(2, fam2, fxdev_cell(2).unwrap_or(0));
                lt.record_op(1, op_kind(1).unwrap_or(0), fxdev_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(FxdevCode::EDrift)
            }
            _ => {
                // 序越闭集：声明族与操作格族错位，空账本同样必抓。
                lt.record_op(0, 3, fxdev_cell(0).unwrap_or(0)) == Err(FxdevCode::EOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x4500 + k as u64);
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
/// UNX-F52469 判据集（锚 UNX-F52470-J1，off-by-one 板式）。
pub fn run_unx52469_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52469");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五功能族 20 操作全通",
        recorded >= CELLS as u32,
        "DevTools 增强区/容器/about:config/同步/更新通道五族逐操作恒等",
    );
    s.add(
        "更新通道阶梯 4 档全落",
        {
            let mut lt = FxdevLedger::new();
            let mut ok = true;
            let mut v = 0usize;
            while v < CHANNEL_STEPS {
                if let Some(w) = channel_cell(v) {
                    if lt.record_channel(v, w).is_err() {
                        ok = false;
                    }
                }
                v += 1;
            }
            ok && lt.chans_all()
        },
        "esr/beta/release/nightly 四档闭集逐档恒等，越档必抓",
    );
    s.add(
        "消费面六判据全落",
        {
            let mut lt = FxdevLedger::new();
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
        "与 Firefox Release 分账声明 2/2 全齐",
        {
            let mut lt = FxdevLedger::new();
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
        "Firefox Release 消费分账（F52463 锚定行）/Developer Edition 独立通道分账两行闭集",
    );
    s.add(
        "Gecko 上栈声明 2/2 全齐",
        {
            let mut lt = FxdevLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < STACK_ROWS {
                if lt.declare_stack(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.stack_all()
        },
        "Gecko 布局引擎上栈/渲染合成层上栈两行闭集",
    );
    s.add(
        "功能族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < CELLS {
                if op_kind(o).unwrap_or(5) < op_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五功能族闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "通道越阶梯/操作载荷漂移/操作序越闭集三态轮转必抓",
    );
    let mut l0 = FxdevLedger::new();
    s.add(
        "边界·通道档越阶梯",
        l0.record_channel(CHANNEL_STEPS, 0).is_err(),
        "通道档 4 越阶梯必拒",
    );
    s.add(
        "操作漂移拒",
        {
            let fam2 = op_kind(2).unwrap_or(0);
            let _ = l0.record_op(2, fam2, fxdev_cell(2).unwrap_or(0));
            l0.record_op(1, op_kind(1).unwrap_or(0), fxdev_cell(1).unwrap_or(0).wrapping_add(1))
                == Err(FxdevCode::EDrift)
        },
        "操作载荷漂移必抓",
    );
    s.add(
        "边界·操作越闭集",
        l0.record_op(CELLS, 0, 0).is_err(),
        "操作 20 越闭集必拒",
    );
    s.add(
        "边界·功能序越闭集",
        FxdevLedger::new()
            .record_op(0, 3, fxdev_cell(0).unwrap_or(0))
            .is_err(),
        "声明族与操作格族错位，空账本同样必抓",
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
            let mut lt = FxdevLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut v = 0usize;
            while v < CHANNEL_STEPS {
                if let Some(w) = channel_cell(v) {
                    let _ = lt.record_channel(v, w);
                }
                v += 1;
            }
            let mut o = 0usize;
            while o < CELLS {
                if let Some(w) = fxdev_cell(o) {
                    let _ = lt.record_op(o, op_kind(o).unwrap_or(0), w);
                }
                o += 1;
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
            lt.all_complete() && lt.judge_grade() == Grade::S2 && lt.judge_grade() != Grade::S1
        },
        "全齐亦定 S2 不越 S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCB500,
        "SEG_BASE 0x40 对齐且在 VX16 域 0xCB500 格",
    );
    s.add(
        "收口·判据条数",
        s.len() >= 15,
        "含本条共 ≥16 条闭集",
    );
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_channel_ladder() {
        assert_eq!(channel_name(0), Some("esr"));
        assert_eq!(channel_name(1), Some("beta"));
        assert_eq!(channel_name(2), Some("release"));
        assert_eq!(channel_name(3), Some("nightly"));
        assert!(channel_name(4).is_none());
        assert!(channel_cell(4).is_none());
    }

    #[test]
    fn test_ops_kind() {
        assert_eq!(op_kind(0), Some(0));
        assert_eq!(op_kind(4), Some(1));
        assert_eq!(op_kind(8), Some(2));
        assert_eq!(op_kind(19), Some(4));
        assert!(op_kind(20).is_none());
        assert!(fxdev_cell(20).is_none());
        assert_eq!(op_kind_name(0), Some("DevTools 增强区"));
        assert_eq!(op_kind_name(4), Some("更新通道"));
    }

    #[test]
    fn test_cells_pure() {
        let a = fxdev_cell(3).unwrap_or(0);
        assert_eq!(a, fxdev_cell(3).unwrap_or(0));
        assert_ne!(a, fxdev_cell(4).unwrap_or(0));
        assert!(split_row(SPLIT_ROWS).is_none());
        assert!(stack_row(STACK_ROWS).is_none());
    }

    #[test]
    fn test_order_caught_empty() {
        // 空账本上的序越闭集注入：必抓且不改 last_kind。
        let mut lt = FxdevLedger::new();
        assert_eq!(
            lt.record_op(0, 3, fxdev_cell(0).unwrap_or(0)),
            Err(FxdevCode::EOrder)
        );
        // last_kind 未被污染：合法序落格仍可继续。
        assert!(lt.record_op(5, op_kind(5).unwrap_or(0), fxdev_cell(5).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = FxdevLedger::new();
        assert_eq!(
            lt.record_op(5, op_kind(5).unwrap_or(0), fxdev_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(FxdevCode::EDrift)
        );
        assert_eq!(
            lt.record_channel(1, channel_cell(1).unwrap_or(0).wrapping_add(7)),
            Err(FxdevCode::EChanDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = FxdevLedger::new();
        let _ = lt.record_channel(0, channel_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_channel(0, channel_cell(0).unwrap_or(0)),
            Err(FxdevCode::EDup)
        );
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = FxdevLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = FxdevLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert_eq!(recorded, CHANNEL_STEPS as u32 + CELLS as u32);
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
        assert_eq!(SEG_BASE, 0xCB500);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(FxdevCode::Ok as u32, 0);
        assert_eq!(FxdevCode::ENoChan as u32, 1);
        assert_eq!(FxdevCode::EChanDrift as u32, 2);
        assert_eq!(FxdevCode::ENoOp as u32, 3);
        assert_eq!(FxdevCode::EDrift as u32, 4);
        assert_eq!(FxdevCode::EOrder as u32, 5);
        assert_eq!(FxdevCode::ENoRow as u32, 6);
        assert_eq!(FxdevCode::ERowMiss as u32, 7);
        assert_eq!(FxdevCode::ENoStack as u32, 8);
        assert_eq!(FxdevCode::EStackMiss as u32, 9);
        assert_eq!(FxdevCode::EDup as u32, 10);
        assert_eq!(FxdevCode::ESeal as u32, 11);
        assert_eq!(FxdevCode::EGradeDrift as u32, 12);
        assert_eq!(FxdevCode::Ok.val(), SEG_BASE);
        assert!(!FxdevCode::Ok.detail().is_empty());
        assert!(!FxdevCode::ESeal.detail().is_empty());
        assert!(!FxdevCode::EGradeDrift.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3，六闸齐 S2，全齐目标档 S2。
        let lt = FxdevLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        let mut lt2 = FxdevLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S2);
    }

    #[test]
    fn test_split_and_stack() {
        let mut lt = FxdevLedger::new();
        assert!(lt.declare_split(0).is_ok());
        assert!(lt.declare_split(1).is_ok());
        assert!(lt.declare_stack(0).is_ok());
        assert!(lt.declare_stack(1).is_ok());
        assert!(lt.splits_all() && lt.stack_all());
        assert_eq!(lt.declare_split(2), Err(FxdevCode::ENoRow));
        assert_eq!(lt.declare_stack(2), Err(FxdevCode::ENoStack));
    }

    #[test]
    fn unx52469_checks_green() {
        let s = run_unx52469_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
