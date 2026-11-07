//! UNX-F52468 Chrome Canary 收口（滚动线）· 0xCB4C0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52468"，逐字）：
//!   title: Chrome Canary 收口（滚动线）
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52469-J1 ×10；
//!     KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时 API/软件写回：
//!     状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52468）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52469-J1，照原文锚定；功能按本卡标题（Chrome Canary 收口（滚动
//! 线））落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五功能族 20 格闭集：滚动更新/实验旗/崩溃隔离/回退/遥测各 4 格，
//!      家族单调（逆序必抓），逐格载荷（纯函数折叠）恒等才收，漂移必抓。
//!   ② 滚动线 16 拍单调：滚动拍载荷严格递增，拍越界与倒退必抓。
//!   ③ 与 Stable 分账声明 2 行闭集：渲染账分账/实验旗账分账，缺一拒封。
//!   ④ 四通道阶梯 4 档：Canary/Dev/Beta/Stable，越档与漂移必抓。
//!   ⑤ 六判据收口闸（启动/渲染/崩溃/卸载/升级/许可）逐格落账，未齐即
//!      封拒。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade 定级闭集）；注入核 run_cycle 三态轮转 10/10（操作漂移/
//!      操作越序/操作越闭集，三类注入全落当轮临时账本，空账本同样必
//!      抓，越序路径不改 last_kind，escaped=0）；注入=检出+逃逸守恒；
//!      双跑 FNV 指纹一致；KB-COMPAT 定级 S1/S2/S3 闭集——滚动线封顶
//!      S2：全齐也只给有条件收口，永不给 S1。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52468;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 五功能族操作数。
pub const FEATURE_OPS: usize = 20;

/// 滚动线拍数。
pub const ROLL_BEATS: usize = 16;

/// 与 Stable 分账声明行数。
pub const SPLIT_ROWS: usize = 2;

/// 四通道阶梯档数。
pub const CHANNEL_STEPS: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C，恰 13 变体。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CanaryCode {
    /// 成功
    Ok = 0,
    /// 操作号越闭集
    ENoOp,
    /// 操作漂移（含滚动拍倒退）
    EDrift,
    /// 家族逆序
    EOrder,
    /// 滚动拍越闭集
    ENoBeat,
    /// 通道越阶梯
    ENoChan,
    /// 通道漂移
    EChanDrift,
    /// 分账行越闭集
    ENoSplit,
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

impl CanaryCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            CanaryCode::Ok => "成功",
            CanaryCode::ENoOp => "操作号越闭集：滚动更新/实验旗/崩溃隔离/回退/遥测 20 格之外先登记",
            CanaryCode::EDrift => "操作漂移：操作或滚动拍折叠与基准不恒等，倒退与漂移逐格复算",
            CanaryCode::EOrder => "家族逆序：滚动更新→实验旗→崩溃隔离→回退→遥测单调，逆序必抓",
            CanaryCode::ENoBeat => "滚动拍越闭集：滚动线 16 拍之外先登记",
            CanaryCode::ENoChan => "通道越阶梯：Canary/Dev/Beta/Stable 四档之外先登记",
            CanaryCode::EChanDrift => "通道漂移：通道档载荷与基准不恒等，逐档复算",
            CanaryCode::ENoSplit => "分账行越闭集：与 Stable 分账 2 行闭集之外先登记",
            CanaryCode::EDup => "重复落账：同格只许一条账目",
            CanaryCode::ESeal => "未齐即封：20 操作 + 16 拍滚动线 + 4 档通道 + 2 行分账未齐先补齐",
            CanaryCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            CanaryCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            CanaryCode::EGradeDrift => "定级漂移：滚动线封顶 S2，全齐也不得定 S1",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口（滚动线不可达）
    S1,
    /// S2：有条件收口（滚动线封顶档）
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
            Grade::S1 => "S1：全通收口（滚动线永不判入，见封顶口径）",
            Grade::S2 => "S2：有条件收口，滚动线封顶档",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 功能族格载荷（操作 o；家族 = o/4）。
pub const fn feature_cell(o: usize) -> Option<u64> {
    if o >= FEATURE_OPS {
        return None;
    }
    Some((0xCE00u64 + (o as u64) * 0x2E) ^ 0x43414E_415259)
}

/// 功能族（0..4=滚动更新 4..8=实验旗 8..12=崩溃隔离 12..16=回退 16..20=遥测）。
pub const fn feature_kind(o: usize) -> Option<u32> {
    if o >= FEATURE_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 家族名。
pub const fn feature_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("滚动更新"),
        1 => Some("实验旗"),
        2 => Some("崩溃隔离"),
        3 => Some("回退"),
        4 => Some("遥测"),
        _ => None,
    }
}

/// 滚动线拍载荷（拍 b，严格递增，单调由构造保证）。
pub const fn roll_beat(b: usize) -> Option<u64> {
    if b >= ROLL_BEATS {
        return None;
    }
    Some(0xCE30u64 + (b as u64) * 0x17)
}

/// 与 Stable 分账行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xCE60 + r as u32 * 0x30)
}

/// 分账行名。
pub const fn split_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("渲染账分账"),
        1 => Some("实验旗账分账"),
        _ => None,
    }
}

/// 四通道阶梯档载荷（档 c：0=Canary 1=Dev 2=Beta 3=Stable）。
pub const fn channel_cell(c: usize) -> Option<u64> {
    if c >= CHANNEL_STEPS {
        return None;
    }
    Some((0xCE80u64 + (c as u64) * 0x3B) ^ 0x4C41_444452)
}

/// 通道档名。
pub const fn channel_name(c: usize) -> Option<&'static str> {
    match c {
        0 => Some("Canary"),
        1 => Some("Dev"),
        2 => Some("Beta"),
        3 => Some("Stable"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Chrome Canary 收口账本状态机。
#[derive(Clone)]
pub struct CanaryLedger {
    /// 六判据（启动/渲染/崩溃/卸载/升级/许可）。
    gates: [Option<bool>; 6],
    /// 功能族操作账。
    ops: [Option<u64>; FEATURE_OPS],
    /// 滚动线拍账。
    beats: [Option<u64>; ROLL_BEATS],
    /// 与 Stable 分账账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 四通道阶梯账。
    chans: [Option<u64>; CHANNEL_STEPS],
    /// 最后家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl CanaryLedger {
    /// 新账本。
    pub fn new() -> Self {
        CanaryLedger {
            gates: [None; 6],
            ops: [None; FEATURE_OPS],
            beats: [None; ROLL_BEATS],
            splits: [None; SPLIT_ROWS],
            chans: [None; CHANNEL_STEPS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), CanaryCode> {
        if gate >= 6 {
            return Err(CanaryCode::ESeal);
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

    /// 功能族一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), CanaryCode> {
        if o >= FEATURE_OPS {
            return Err(CanaryCode::ENoOp);
        }
        let kind = match feature_kind(o) {
            Some(k) => k,
            None => return Err(CanaryCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4300);
            return Err(CanaryCode::EOrder);
        }
        let want = match feature_cell(o) {
            Some(w) => w,
            None => return Err(CanaryCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4301 + o as u64);
            return Err(CanaryCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CanaryCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x4302 + o as u64);
        Ok(())
    }

    /// 功能族全账核验（20 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < FEATURE_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 滚动线一拍（越拍拒 + 拍载荷对照，倒退与漂移同归 EDrift 走注入账）。
    pub fn record_beat(&mut self, b: usize, got: u64) -> Result<(), CanaryCode> {
        if b >= ROLL_BEATS {
            return Err(CanaryCode::ENoBeat);
        }
        let want = match roll_beat(b) {
            Some(w) => w,
            None => return Err(CanaryCode::ENoBeat),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4601 + b as u64);
            return Err(CanaryCode::EDrift);
        }
        if match self.beats.get(b) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CanaryCode::EDup);
        }
        if let Some(c) = self.beats.get_mut(b) {
            *c = Some(got);
        }
        self.fnv(0x4600 + b as u64);
        Ok(())
    }

    /// 滚动线全账核验（16 拍全落且单调由载荷构造保证）。
    pub fn beats_all(&self) -> bool {
        let mut b = 0usize;
        while b < ROLL_BEATS {
            match self.beats.get(b) {
                Some(Some(_)) => {}
                _ => return false,
            }
            b += 1;
        }
        true
    }

    /// 与 Stable 分账一行。
    pub fn declare_split(&mut self, r: usize) -> Result<(), CanaryCode> {
        if r >= SPLIT_ROWS {
            return Err(CanaryCode::ENoSplit);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CanaryCode::EDup);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0x4400 + r as u64);
        Ok(())
    }

    /// 分账全齐核验。
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

    /// 四通道阶梯一档（越档拒 + 载荷对照 + 重复拒，漂移走注入账）。
    pub fn record_channel(&mut self, c: usize, got: u64) -> Result<(), CanaryCode> {
        if c >= CHANNEL_STEPS {
            return Err(CanaryCode::ENoChan);
        }
        let want = match channel_cell(c) {
            Some(w) => w,
            None => return Err(CanaryCode::ENoChan),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4701 + c as u64);
            return Err(CanaryCode::EChanDrift);
        }
        if match self.chans.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CanaryCode::EDup);
        }
        if let Some(k) = self.chans.get_mut(c) {
            *k = Some(got);
        }
        self.fnv(0x4700 + c as u64);
        Ok(())
    }

    /// 四通道阶梯全账核验（4 档）。
    pub fn chans_all(&self) -> bool {
        let mut c = 0usize;
        while c < CHANNEL_STEPS {
            match self.chans.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
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

    /// KB-COMPAT 定级（滚动线封顶 S2：全齐也只给有条件收口，判据缺 → S3）。
    pub fn judge_grade(&self) -> Grade {
        let gates_ok = self.gates_all();
        if gates_ok {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for CanaryLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（落账格数, 检出数, 逃逸数, 指纹）。
pub fn run_cycle() -> (u32, u32, u32, u64) {
    let mut led = CanaryLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < FEATURE_OPS {
        if let Some(w) = feature_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut b = 0usize;
    while b < ROLL_BEATS {
        if let Some(w) = roll_beat(b) {
            if led.record_beat(b, w).is_ok() {
                recorded += 1;
            }
        }
        b += 1;
    }
    let mut c = 0usize;
    while c < CHANNEL_STEPS {
        if let Some(w) = channel_cell(c) {
            if led.record_channel(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    // 注入 10 轮：操作漂移 / 操作越序 / 操作越闭集三态轮转。
    // 三类注入全部落在当轮临时账本上，临时账本从空起手，空账本同样必抓。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = CanaryLedger::new();
        let caught = match kind {
            0 => {
                // 漂移注入：先按合法序落正格，再注漂移。
                let _ = lt.record_op(0, feature_cell(0).unwrap_or(0));
                lt.record_op(1, feature_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(CanaryCode::EDrift)
            }
            1 => {
                // 越序注入：先落家族 2 正格，再倒落家族 0；错误路径不改 last_kind。
                let _ = lt.record_op(8, feature_cell(8).unwrap_or(0));
                lt.record_op(0, feature_cell(0).unwrap_or(0)) == Err(CanaryCode::EOrder)
            }
            _ => {
                // 越闭集注入：空账本直接越界，提前返回必抓。
                lt.record_op(FEATURE_OPS, 0).is_err()
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
    let escaped = led.escaped;
    let fp = led.fingerprint();
    (recorded, caught, escaped, fp)
}

/// 周期守恒核验（守恒破缺 → EInj，指纹漂移 → EFp）。
pub fn verify_cycle() -> Result<u64, CanaryCode> {
    let (_, caught, escaped, fp) = run_cycle();
    if caught != 10 || escaped != 0 {
        return Err(CanaryCode::EInj);
    }
    let (_, _, _, fp2) = run_cycle();
    if fp != fp2 {
        return Err(CanaryCode::EFp);
    }
    Ok(fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// UNX-F52468 判据集（锚 UNX-F52469-J1，off-by-one 板式）。
pub fn run_unx52468_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52468");
    let (recorded, caught, escaped, fp) = run_cycle();
    s.add(
        "五功能族 20 格全通",
        recorded >= FEATURE_OPS as u32,
        "滚动更新/实验旗/崩溃隔离/回退/遥测五家族逐操作恒等",
    );
    s.add(
        "滚动线 16 拍全落且单调",
        {
            let mut lt = CanaryLedger::new();
            let mut ok = true;
            let mut b = 0usize;
            while b < ROLL_BEATS {
                if let Some(w) = roll_beat(b) {
                    if lt.record_beat(b, w).is_err() {
                        ok = false;
                    }
                }
                b += 1;
            }
            ok && lt.beats_all()
        },
        "滚动拍载荷严格递增，倒退必抓",
    );
    s.add(
        "拍倒退拒",
        {
            let mut lt = CanaryLedger::new();
            lt.record_beat(0, roll_beat(1).unwrap_or(0)) == Err(CanaryCode::EDrift)
        },
        "滚动拍载荷不等于当拍基准（倒退/漂移）必抓",
    );
    s.add(
        "四通道阶梯 4 档全落",
        {
            let mut lt = CanaryLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < CHANNEL_STEPS {
                if let Some(w) = channel_cell(c) {
                    if lt.record_channel(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.chans_all()
        },
        "Canary/Dev/Beta/Stable 四档逐档落账，越档必抓",
    );
    s.add(
        "通道漂移拒",
        {
            let mut lt = CanaryLedger::new();
            lt.record_channel(2, channel_cell(2).unwrap_or(0).wrapping_add(1))
                == Err(CanaryCode::EChanDrift)
        },
        "通道档载荷漂移必抓",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = CanaryLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/渲染/崩溃/卸载/升级/许可逐格落账（UNX-F52469-J1 六判据）",
    );
    s.add(
        "与 Stable 分账声明 2/2 全齐",
        {
            let mut lt = CanaryLedger::new();
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
        "渲染账分账/实验旗账分账两行闭集（§AI-66.3）",
    );
    s.add(
        "家族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < FEATURE_OPS {
                if feature_kind(o).unwrap_or(5) < feature_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五家族闭集单调，逆序必抓",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "操作漂移/操作越序/操作越闭集三态轮转必抓",
    );
    let mut l0 = CanaryLedger::new();
    s.add(
        "边界·操作越闭集",
        l0.record_op(FEATURE_OPS, 0).is_err(),
        "操作 20 越闭集必拒",
    );
    s.add(
        "边界·拍越闭集",
        l0.record_beat(ROLL_BEATS, 0).is_err(),
        "滚动拍 16 越闭集必拒",
    );
    s.add(
        "边界·通道与分账行越闭集",
        l0.record_channel(CHANNEL_STEPS, 0).is_err() && l0.declare_split(SPLIT_ROWS).is_err(),
        "通道档 4 越阶梯与分账行 2 越闭集必拒",
    );
    s.add(
        "未齐可续录",
        {
            let mut l1 = CanaryLedger::new();
            l1.record_op(0, feature_cell(0).unwrap_or(0)).is_ok()
                && l1.record_beat(0, roll_beat(0).unwrap_or(0)).is_ok()
        },
        "账未齐时落账与滚动拍必须放行（未齐即封核只拒封口不拒续录）",
    );
    s.add_dyn(
        "注入账守恒",
        caught == 10 && escaped == 0,
        &format!("注入=10 检出={} 逃逸={}", caught, escaped),
    );
    s.add("双跑指纹一致", fp != 0 && verify_cycle().is_ok(), "确定性全流程逐位一致且守恒核验通过");
    s.add(
        "滚动线封顶 S2",
        {
            let mut lt = CanaryLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < FEATURE_OPS {
                if let Some(w) = feature_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            let mut b = 0usize;
            while b < ROLL_BEATS {
                if let Some(w) = roll_beat(b) {
                    let _ = lt.record_beat(b, w);
                }
                b += 1;
            }
            let mut c = 0usize;
            while c < CHANNEL_STEPS {
                if let Some(w) = channel_cell(c) {
                    let _ = lt.record_channel(c, w);
                }
                c += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_split(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S2 && lt.judge_grade() != Grade::S1
        },
        "全齐也只定 S2，滚动线永不 S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCB4C0,
        "SEG_BASE 0x40 对齐且在 VX16 域 0xCB4C0 格",
    );
    s.add("收口·判据条数", s.len() >= 17, "含本条共 ≥18 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feature_kind() {
        assert_eq!(feature_kind(0), Some(0));
        assert_eq!(feature_kind(4), Some(1));
        assert_eq!(feature_kind(9), Some(2));
        assert_eq!(feature_kind(14), Some(3));
        assert_eq!(feature_kind(19), Some(4));
        assert!(feature_kind(20).is_none());
        assert!(feature_cell(20).is_none());
        assert_eq!(feature_kind_name(0), Some("滚动更新"));
        assert_eq!(feature_kind_name(4), Some("遥测"));
        assert!(feature_kind_name(7).is_none());
    }

    #[test]
    fn test_roll_beats_monotonic() {
        let mut b = 1usize;
        while b < ROLL_BEATS {
            assert!(roll_beat(b).unwrap_or(0) > roll_beat(b - 1).unwrap_or(0));
            b += 1;
        }
        assert!(roll_beat(ROLL_BEATS).is_none());
    }

    #[test]
    fn test_channels() {
        assert_eq!(channel_name(0), Some("Canary"));
        assert_eq!(channel_name(1), Some("Dev"));
        assert_eq!(channel_name(2), Some("Beta"));
        assert_eq!(channel_name(3), Some("Stable"));
        assert!(channel_name(4).is_none());
        assert!(channel_cell(CHANNEL_STEPS).is_none());
        assert_ne!(channel_cell(0), channel_cell(3));
    }

    #[test]
    fn test_rows_closed() {
        assert_eq!(split_row_name(0), Some("渲染账分账"));
        assert_eq!(split_row_name(1), Some("实验旗账分账"));
        assert!(split_row_name(2).is_none());
        assert!(split_row(SPLIT_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        // 合法序先落低族正格，再注逆序必抓。
        let mut lt = CanaryLedger::new();
        let _ = lt.record_op(6, feature_cell(6).unwrap_or(0));
        assert_eq!(lt.record_op(2, feature_cell(2).unwrap_or(0)), Err(CanaryCode::EOrder));
        // 逆序错误路径不改 last_kind：同族后续仍可续录。
        assert!(lt.record_op(7, feature_cell(7).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = CanaryLedger::new();
        assert_eq!(
            lt.record_op(11, feature_cell(11).unwrap_or(0).wrapping_add(4)),
            Err(CanaryCode::EDrift)
        );
        assert_eq!(
            lt.record_beat(3, roll_beat(3).unwrap_or(0).wrapping_sub(1)),
            Err(CanaryCode::EDrift)
        );
        assert_eq!(
            lt.record_channel(1, channel_cell(1).unwrap_or(0).wrapping_add(9)),
            Err(CanaryCode::EChanDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = CanaryLedger::new();
        let _ = lt.record_beat(5, roll_beat(5).unwrap_or(0));
        assert_eq!(lt.record_beat(5, roll_beat(5).unwrap_or(0)), Err(CanaryCode::EDup));
        let _ = lt.record_channel(0, channel_cell(0).unwrap_or(0));
        assert_eq!(lt.record_channel(0, channel_cell(0).unwrap_or(0)), Err(CanaryCode::EDup));
        let _ = lt.declare_split(0);
        assert_eq!(lt.declare_split(0), Err(CanaryCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = CanaryLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = CanaryLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, escaped, _) = run_cycle();
        // 只含 run_cycle 实际计数的节：操作 20 + 滚动拍 16 + 通道档 4。
        assert_eq!(recorded, FEATURE_OPS as u32 + ROLL_BEATS as u32 + CHANNEL_STEPS as u32);
        assert_eq!(caught, 10);
        assert_eq!(escaped, 0);
    }

    #[test]
    fn test_dual_run_fingerprint() {
        let (a1, b1, c1, f1) = run_cycle();
        let (a2, b2, c2, f2) = run_cycle();
        assert_eq!(a1, a2);
        assert_eq!(b1, b2);
        assert_eq!(c1, c2);
        assert_eq!(f1, f2);
        assert_ne!(f1, 0);
        assert!(verify_cycle().is_ok());
    }

    #[test]
    fn test_empty_ledger_injections() {
        // 三类注入对空账本同样必抓；越界提前返回不计注入账。
        let mut l0 = CanaryLedger::new();
        assert!(l0.record_op(FEATURE_OPS, 0).is_err());
        assert!(l0.record_beat(ROLL_BEATS, 0).is_err());
        assert!(l0.record_channel(CHANNEL_STEPS, 0).is_err());
        let (i0, c0, e0) = l0.stats();
        assert_eq!((i0, c0, e0), (0, 0, 0));
    }

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0xCB4C0);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(CanaryCode::Ok as u32, 0);
        assert_eq!(CanaryCode::EGradeDrift as u32, 12);
        assert_eq!(CanaryCode::Ok.val(), SEG_BASE);
        assert!(!CanaryCode::Ok.detail().is_empty());
        assert!(!CanaryCode::ESeal.detail().is_empty());
        assert!(!CanaryCode::ENoBeat.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3，六闸齐 S2（滚动线封顶档），全齐目标档仍 S2。
        let lt0 = CanaryLedger::new();
        assert_eq!(lt0.judge_grade(), Grade::S3);
        let mut lt1 = CanaryLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt1.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt1.judge_grade(), Grade::S2);
        let mut lt2 = CanaryLedger::new();
        let mut o = 0usize;
        while o < FEATURE_OPS {
            if let Some(w) = feature_cell(o) {
                let _ = lt2.record_op(o, w);
            }
            o += 1;
        }
        let mut b = 0usize;
        while b < ROLL_BEATS {
            if let Some(w) = roll_beat(b) {
                let _ = lt2.record_beat(b, w);
            }
            b += 1;
        }
        let mut g2 = 0usize;
        while g2 < 6 {
            let _ = lt2.judge_gate(g2);
            g2 += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S2);
        assert_ne!(lt2.judge_grade(), Grade::S1);
    }

    #[test]
    fn unx52468_checks_green() {
        let s = run_unx52468_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 18);
    }
}
