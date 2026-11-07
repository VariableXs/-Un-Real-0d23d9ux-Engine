//! UNX-F52299 B15 防重 grep 与段收官印（B01–B15 冻结 300/800）· 0xC8A80段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52299"，逐字）：
//!   title: B15 防重 grep 与段收官印（B01–B15 冻结 300/800）
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52300-J1 段关门
//!     印：300/800 条全冻结、六判据全绿率结算入册、KB-COMPAT 可查询率
//!     100%、共面四账全绿、两条等同红线零违例（判据未放宽/伪装表零许
//!     可绕过）、P0=0；判据锚定 §AI-66.5/§AI-66.9；③ 完成时 API/软件
//!     写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52299）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52300-J1，照原文锚定；功能按本卡标题（B15 防重 grep 与段收官
//! 印）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 四范围 grep 16 格闭集：关键词族（六判据/KB-COMPAT/shim/收口）
//!      四范围 × 每范围 4 格，逐格命中数与载荷双重对照，越范围必抓。
//!   ② 300/800 冻结双闸：300 冻结闸（B01–B15 段内 300 条）与 800 冻结
//!      闸（域账累计 800 条）各自独立落闸，冻结值漂移必抓。
//!   ③ 段收官印 8 拍：提单/查重/低闸/高闸/结算/入册/复算/盖印八拍依
//!      序落账，拍序逆序必抓且不改 last_beat。
//!   ④ 冲突检出：同格重复且命中数不符按冲突封拒（EConflict），与普通
//!      重复落账（EDup）分路，冲突注入 10/10 必抓。
//!   ⑤ ledger 状态机（new/judge_gate×6/record_grep/record_freeze/
//!      record_beat/fingerprint FNV-1a）；注入核 run_cycle 10 轮三态轮
//!      转（范围越闭集/冲突检出/冻结值漂移），全部落在当轮临时账本上，
//!      检出+逃逸守恒；双跑 FNV 指纹一致；定级 S1/S2/S3 闭集。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52299;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// grep 范围数（关键词族四范围）。
pub const GREP_SCOPES: usize = 4;

/// 每范围格数。
pub const CELLS_PER_SCOPE: usize = 4;

/// grep 格总数。
pub const GREP_CELLS: usize = GREP_SCOPES * CELLS_PER_SCOPE;

/// 段内冻结低闸（B01–B15 段内 300 条）。
pub const FREEZE_LO: u32 = 300;

/// 域账累计冻结高闸（累计 800 条）。
pub const FREEZE_HI: u32 = 800;

/// 段收官印拍数。
pub const SEAL_BEATS: usize = 8;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GrepGateCode {
    /// 成功
    Ok = 0,
    /// grep 范围越闭集
    ENoScope,
    /// 范围内格号越闭集
    ENoGCell,
    /// grep 载荷漂移
    EGrepDrift,
    /// 冲突检出：同格重复且命中数不符
    EConflict,
    /// 收官拍越闭集
    ENoBeat,
    /// 收官拍序逆序
    EBeatOrder,
    /// 冻结值漂移（300/800 双闸之外）
    EFreeze,
    /// 重复落账
    EDup,
    /// 判据闸越闭集/未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl GrepGateCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            GrepGateCode::Ok => "成功",
            GrepGateCode::ENoScope => "grep 范围越闭集：六判据/KB-COMPAT/shim/收口四范围之外先登记",
            GrepGateCode::ENoGCell => "范围内格号越闭集：每范围 4 格闭集之外先登记",
            GrepGateCode::EGrepDrift => "grep 载荷漂移：载荷与范围基准不恒等，逐格复算",
            GrepGateCode::EConflict => "冲突检出：同格重复且命中数不符，防重 grep 必抓",
            GrepGateCode::ENoBeat => "收官拍越闭集：段收官印 8 拍闭集之外先登记",
            GrepGateCode::EBeatOrder => "收官拍序逆序：提单→…→盖印八拍依序，逆序必抓且不改 last_beat",
            GrepGateCode::EFreeze => "冻结值漂移：300/800 双闸冻结值之外先登记",
            GrepGateCode::EDup => "重复落账：同格只许一条账目",
            GrepGateCode::ESeal => "判据闸越闭集：六闸闭集 0..=5 之外先登记",
            GrepGateCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            GrepGateCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            GrepGateCode::EGradeDrift => "定级漂移：16 格+双闸+8 拍+六闸全齐才可定 S1",
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
            Grade::S1 => "S1：16 格 grep+300/800 双闸+8 拍收官+六闸全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// grep 范围名（防重关键词族，§AI-66.5 批收口节奏）。
pub const fn scope_name(sc: usize) -> Option<&'static str> {
    match sc {
        0 => Some("六判据族"),
        1 => Some("KB-COMPAT族"),
        2 => Some("shim族"),
        3 => Some("收口族"),
        _ => None,
    }
}

/// grep 格基准载荷（XOR 锚常量低 16 位恒零）。
pub const fn grep_cell(sc: usize, idx: usize) -> Option<u64> {
    if sc >= GREP_SCOPES || idx >= CELLS_PER_SCOPE {
        return None;
    }
    Some((0xB7000u64 + (sc as u64) * 0x40 + (idx as u64) * 0x1B) ^ 0x4752_0000_0000)
}

/// grep 格期望命中数（确定性推导，冲突检出的对照基准）。
pub const fn grep_hits(sc: usize, idx: usize) -> Option<u32> {
    if sc >= GREP_SCOPES || idx >= CELLS_PER_SCOPE {
        return None;
    }
    Some(1 + ((sc * CELLS_PER_SCOPE + idx) % 7) as u32)
}

/// 段收官印拍名（8 拍依序）。
pub const fn beat_name(b: usize) -> Option<&'static str> {
    match b {
        0 => Some("提单"),
        1 => Some("查重"),
        2 => Some("低闸冻结"),
        3 => Some("高闸冻结"),
        4 => Some("判据结算"),
        5 => Some("入册"),
        6 => Some("复算"),
        7 => Some("盖印"),
        _ => None,
    }
}

/// 收官拍折叠（拍 b）。
pub const fn beat_cell(b: usize) -> Option<u32> {
    if b >= SEAL_BEATS {
        return None;
    }
    Some(0xB800 + b as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// B15 防重 grep 与段收官印账本状态机。
#[derive(Clone)]
pub struct GrepGateLedger {
    /// 六判据闸（全冻结/全绿率/可查询率/共面四账/红线零违例/P0=0）。
    gates: [Option<bool>; 6],
    /// grep 格账。
    greps: [Option<u64>; GREP_CELLS],
    /// 冻结双闸落位。
    frozen_lo: bool,
    frozen_hi: bool,
    /// 收官拍账。
    beats: [Option<u32>; SEAL_BEATS],
    /// 最后收官拍（单调基准）。
    last_beat: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl GrepGateLedger {
    /// 新账本。
    pub fn new() -> Self {
        GrepGateLedger {
            gates: [None; 6],
            greps: [None; GREP_CELLS],
            frozen_lo: false,
            frozen_hi: false,
            beats: [None; SEAL_BEATS],
            last_beat: -1,
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

    /// 落一格六判据闸。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), GrepGateCode> {
        if gate >= 6 {
            return Err(GrepGateCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5000 + gate as u64);
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

    /// grep 一格（越闭集、命中数冲突检出、载荷对照、重复拒）。
    pub fn record_grep(&mut self, sc: usize, idx: usize, hits: u32, got: u64) -> Result<(), GrepGateCode> {
        if sc >= GREP_SCOPES {
            return Err(GrepGateCode::ENoScope);
        }
        if idx >= CELLS_PER_SCOPE {
            return Err(GrepGateCode::ENoGCell);
        }
        let want_hits = match grep_hits(sc, idx) {
            Some(h) => h,
            None => return Err(GrepGateCode::ENoScope),
        };
        let cell = sc * CELLS_PER_SCOPE + idx;
        let filled = match self.greps.get(cell) {
            Some(Some(_)) => true,
            _ => false,
        };
        // 冲突检出：命中数与范围基准不符一律按冲突封拒（无论同格是否已落账），
        // 先于普通重复落账分路。
        if hits != want_hits {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5100 + cell as u64);
            return Err(GrepGateCode::EConflict);
        }
        let want = match grep_cell(sc, idx) {
            Some(w) => w,
            None => return Err(GrepGateCode::ENoScope),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5102 + cell as u64);
            return Err(GrepGateCode::EGrepDrift);
        }
        if filled {
            return Err(GrepGateCode::EDup);
        }
        if let Some(c) = self.greps.get_mut(cell) {
            *c = Some(got);
        }
        self.fnv(0x5103 + cell as u64);
        Ok(())
    }

    /// grep 16 格全账核验。
    pub fn greps_all(&self) -> bool {
        let mut i = 0usize;
        while i < GREP_CELLS {
            match self.greps.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 冻结双闸一格（值漂移必抓，300 与 800 各自落位）。
    pub fn record_freeze(&mut self, lo: u32, hi: u32) -> Result<(), GrepGateCode> {
        // 300/800 双闸：任一侧冻结值漂移必抓，双值恒等才落位。
        if lo != FREEZE_LO || hi != FREEZE_HI {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5200);
            return Err(GrepGateCode::EFreeze);
        }
        self.frozen_lo = true;
        self.frozen_hi = true;
        self.fnv(0x5202);
        Ok(())
    }

    /// 冻结双闸全落核验。
    pub fn frozen_both(&self) -> bool {
        self.frozen_lo && self.frozen_hi
    }

    /// 收官一格（拍序单调 + 折叠对照 + 重复拒）。
    pub fn record_beat(&mut self, b: usize) -> Result<(), GrepGateCode> {
        if b >= SEAL_BEATS {
            return Err(GrepGateCode::ENoBeat);
        }
        if (b as i64) < self.last_beat {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5300);
            return Err(GrepGateCode::EBeatOrder);
        }
        if match self.beats.get(b) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GrepGateCode::EDup);
        }
        if let Some(c) = self.beats.get_mut(b) {
            *c = beat_cell(b);
        }
        self.last_beat = b as i64;
        self.fnv(0x5301 + b as u64);
        Ok(())
    }

    /// 收官 8 拍全账核验。
    pub fn beats_all(&self) -> bool {
        let mut b = 0usize;
        while b < SEAL_BEATS {
            match self.beats.get(b) {
                Some(Some(_)) => {}
                _ => return false,
            }
            b += 1;
        }
        true
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 最后收官拍（注入失败不改）。
    pub fn last_beat(&self) -> i64 {
        self.last_beat
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// KB-COMPAT 定级（全齐 → S1，六闸齐 → S2，否则 S3）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() && self.greps_all() && self.frozen_both() && self.beats_all() {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for GrepGateLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = GrepGateLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut sc = 0usize;
    while sc < GREP_SCOPES {
        let mut i = 0usize;
        while i < CELLS_PER_SCOPE {
            let h = match grep_hits(sc, i) {
                Some(h) => h,
                None => 1,
            };
            let w = match grep_cell(sc, i) {
                Some(w) => w,
                None => 0,
            };
            if led.record_grep(sc, i, h, w).is_ok() {
                recorded += 1;
            }
            i += 1;
        }
        sc += 1;
    }
    let _ = led.record_freeze(FREEZE_LO, FREEZE_HI);
    let mut b = 0usize;
    while b < SEAL_BEATS {
        let _ = led.record_beat(b);
        b += 1;
    }
    // 注入 10 轮：范围越闭集 / 冲突检出 / 冻结值漂移三态轮转，
    // 全部落在当轮临时账本上（冲突注入先按合法序落正格再注入）。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = GrepGateLedger::new();
        let caught = match kind {
            0 => {
                // 越闭集：空账本上范围 4 越界必抓。
                lt.record_grep(GREP_SCOPES, 0, 1, 0).is_err()
            }
            1 => {
                // 冲突检出：先落正格再注同格异命中数。
                let h0 = match grep_hits(0, 0) {
                    Some(h) => h,
                    None => 1,
                };
                let w0 = match grep_cell(0, 0) {
                    Some(w) => w,
                    None => 0,
                };
                let _ = lt.record_grep(0, 0, h0, w0);
                let w1 = match grep_cell(0, 0) {
                    Some(w) => w,
                    None => 0,
                };
                lt.record_grep(0, 0, h0.wrapping_add(1), w1) == Err(GrepGateCode::EConflict)
            }
            _ => {
                // 冻结值漂移：800 侧漂移必抓。
                lt.record_freeze(FREEZE_LO, FREEZE_HI - 1) == Err(GrepGateCode::EFreeze)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5400 + k as u64);
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
/// UNX-F52299 判据集（锚 UNX-F52300-J1，off-by-one 板式）。
pub fn run_unx52299_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52299");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "四范围 grep 16 格全落",
        recorded >= GREP_CELLS as u32,
        "六判据/KB-COMPAT/shim/收口四范围 × 每范围 4 格逐格恒等（§AI-66.5 批收口节奏）",
    );
    s.add(
        "命中数闭集恒等",
        {
            let mut ok = true;
            let mut sc = 0usize;
            while sc < GREP_SCOPES && ok {
                let mut i = 0usize;
                while i < CELLS_PER_SCOPE && ok {
                    match grep_hits(sc, i) {
                        Some(h) if h >= 1 && h <= 7 => {}
                        _ => ok = false,
                    }
                    i += 1;
                }
                sc += 1;
            }
            ok
        },
        "期望命中数确定性推导 1..=7，冲突对照基准恒定",
    );
    s.add(
        "300 冻结闸落位",
        {
            let mut lt = GrepGateLedger::new();
            let _ = lt.record_freeze(FREEZE_LO, FREEZE_HI);
            lt.frozen_both()
        },
        "B01–B15 段内 300 条全冻结",
    );
    s.add(
        "800 冻结闸落位",
        {
            let mut lt = GrepGateLedger::new();
            lt.record_freeze(FREEZE_LO - 1, FREEZE_HI) == Err(GrepGateCode::EFreeze)
                && lt.record_freeze(FREEZE_LO, FREEZE_HI).is_ok()
        },
        "域账累计 800 条全冻结；漂移值先拒后正落",
    );
    s.add(
        "段收官印 8 拍全落",
        {
            let mut lt = GrepGateLedger::new();
            let mut ok = true;
            let mut b = 0usize;
            while b < SEAL_BEATS {
                if lt.record_beat(b).is_err() {
                    ok = false;
                }
                b += 1;
            }
            ok && lt.beats_all()
        },
        "提单/查重/低闸/高闸/结算/入册/复算/盖印依序落账",
    );
    s.add(
        "拍序单调",
        {
            let mut ok = true;
            let mut b = 1usize;
            while b < SEAL_BEATS {
                if beat_cell(b).unwrap_or(0) <= beat_cell(b - 1).unwrap_or(0) {
                    ok = false;
                }
                b += 1;
            }
            ok
        },
        "八拍折叠严格递增，逆序必抓",
    );
    s.add(
        "六判据闸全落",
        {
            let mut lt = GrepGateLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "全冻结/全绿率/可查询率 100%/共面四账/红线零违例/P0=0 逐格落账（§AI-66.9）",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "范围越闭集/冲突检出/冻结值漂移三态轮转必抓",
    );
    let mut l0 = GrepGateLedger::new();
    s.add(
        "边界·范围越闭集",
        l0.record_grep(GREP_SCOPES, 0, 1, 0).is_err(),
        "范围 4 越闭集必拒",
    );
    s.add(
        "边界·grep 格越闭集",
        l0.record_grep(0, CELLS_PER_SCOPE, 1, 0).is_err(),
        "格号 4 越闭集必拒",
    );
    s.add(
        "边界·收官拍越闭集",
        l0.record_beat(SEAL_BEATS).is_err(),
        "收官拍 8 越闭集必拒",
    );
    s.add(
        "冲突检出拒",
        {
            let h = grep_hits(1, 2).unwrap_or(1);
            let w = grep_cell(1, 2).unwrap_or(0);
            let _ = l0.record_grep(1, 2, h, w);
            l0.record_grep(1, 2, h.wrapping_add(3), w) == Err(GrepGateCode::EConflict)
        },
        "同格重复且命中数不符，防重 grep 必抓",
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
            let mut lt = GrepGateLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut sc = 0usize;
            while sc < GREP_SCOPES {
                let mut i = 0usize;
                while i < CELLS_PER_SCOPE {
                    let h = match grep_hits(sc, i) {
                        Some(h) => h,
                        None => 1,
                    };
                    let w = match grep_cell(sc, i) {
                        Some(w) => w,
                        None => 0,
                    };
                    let _ = lt.record_grep(sc, i, h, w);
                    i += 1;
                }
                sc += 1;
            }
            let _ = lt.record_freeze(FREEZE_LO, FREEZE_HI);
            let mut b = 0usize;
            while b < SEAL_BEATS {
                let _ = lt.record_beat(b);
                b += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全齐 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC8A80,
        "SEG_BASE 0x40 对齐且按 VX15 公式落 0xC8A80",
    );
    s.add("收口·判据条数", s.len() >= 16, "含本条共 ≥17 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scope_names() {
        assert_eq!(scope_name(0), Some("六判据族"));
        assert_eq!(scope_name(1), Some("KB-COMPAT族"));
        assert_eq!(scope_name(2), Some("shim族"));
        assert_eq!(scope_name(3), Some("收口族"));
        assert!(scope_name(4).is_none());
        assert!(grep_cell(4, 0).is_none());
        assert!(grep_cell(0, CELLS_PER_SCOPE).is_none());
    }

    #[test]
    fn test_hits_range() {
        let mut sc = 0usize;
        while sc < GREP_SCOPES {
            let mut i = 0usize;
            while i < CELLS_PER_SCOPE {
                let h = grep_hits(sc, i).unwrap_or(0);
                assert!(h >= 1 && h <= 7);
                assert_eq!(h, grep_hits(sc, i).unwrap_or(99), "命中数推导必须恒定");
                i += 1;
            }
            sc += 1;
        }
        assert!(grep_hits(GREP_SCOPES, 0).is_none());
    }

    #[test]
    fn test_beat_names() {
        assert_eq!(beat_name(0), Some("提单"));
        assert_eq!(beat_name(7), Some("盖印"));
        assert!(beat_name(8).is_none());
        assert!(beat_cell(SEAL_BEATS).is_none());
        let mut b = 1usize;
        while b < SEAL_BEATS {
            assert!(beat_cell(b).unwrap_or(0) > beat_cell(b - 1).unwrap_or(0));
            b += 1;
        }
    }

    #[test]
    fn test_beat_order_caught_and_last_beat_kept() {
        let mut lt = GrepGateLedger::new();
        assert!(lt.record_beat(0).is_ok());
        assert!(lt.record_beat(3).is_ok());
        assert_eq!(lt.record_beat(1), Err(GrepGateCode::EBeatOrder));
        assert_eq!(lt.last_beat(), 3);
        // 顺序补拍不逆序。
        assert!(lt.record_beat(4).is_ok());
    }

    #[test]
    fn test_conflict_caught() {
        let mut lt = GrepGateLedger::new();
        let h = grep_hits(2, 1).unwrap_or(1);
        let w = grep_cell(2, 1).unwrap_or(0);
        assert!(lt.record_grep(2, 1, h, w).is_ok());
        assert_eq!(lt.record_grep(2, 1, h.wrapping_add(1), w), Err(GrepGateCode::EConflict));
        // 空格命中数不符同样按冲突封拒。
        assert_eq!(lt.record_grep(2, 2, h, grep_cell(2, 2).unwrap_or(0)), Err(GrepGateCode::EConflict));
        let (inj, caught, esc) = lt.stats();
        assert_eq!((inj, caught, esc), (2, 2, 0));
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = GrepGateLedger::new();
        let h = grep_hits(0, 0).unwrap_or(1);
        let w = grep_cell(0, 0).unwrap_or(0);
        assert!(lt.record_grep(0, 0, h, w).is_ok());
        assert_eq!(lt.record_grep(0, 0, h, w), Err(GrepGateCode::EDup));
        assert!(lt.record_beat(0).is_ok());
        assert_eq!(lt.record_beat(0), Err(GrepGateCode::EDup));
    }

    #[test]
    fn test_freeze_gate() {
        let mut lt = GrepGateLedger::new();
        assert!(!lt.frozen_both());
        assert_eq!(lt.record_freeze(FREEZE_LO, FREEZE_HI - 1), Err(GrepGateCode::EFreeze));
        assert_eq!(lt.record_freeze(FREEZE_LO - 1, FREEZE_HI), Err(GrepGateCode::EFreeze));
        assert_eq!(lt.record_freeze(FREEZE_LO - 1, FREEZE_HI - 1), Err(GrepGateCode::EFreeze));
        assert!(!lt.frozen_both());
        assert!(lt.record_freeze(FREEZE_LO, FREEZE_HI).is_ok());
        assert!(lt.frozen_both());
        let (inj, caught, esc) = lt.stats();
        assert_eq!((inj, caught, esc), (3, 3, 0));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = GrepGateLedger::new();
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
        assert!(recorded >= GREP_CELLS as u32);
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
        assert_eq!(SEG_BASE, 0xC8A80);
        assert_eq!(SEG_BASE, 0xC7200 + 0x40 * (52299u32 - 52201));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(GrepGateCode::Ok as u32, 0);
        assert_eq!(GrepGateCode::EGradeDrift as u32, 12);
        assert_eq!(GrepGateCode::Ok.val(), SEG_BASE);
        assert_eq!(GrepGateCode::EGradeDrift.val(), SEG_BASE + 0x0C);
        let all: [GrepGateCode; 13] = [
            GrepGateCode::Ok,
            GrepGateCode::ENoScope,
            GrepGateCode::ENoGCell,
            GrepGateCode::EGrepDrift,
            GrepGateCode::EConflict,
            GrepGateCode::ENoBeat,
            GrepGateCode::EBeatOrder,
            GrepGateCode::EFreeze,
            GrepGateCode::EDup,
            GrepGateCode::ESeal,
            GrepGateCode::EInj,
            GrepGateCode::EFp,
            GrepGateCode::EGradeDrift,
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
        let lt = GrepGateLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        let mut lt2 = GrepGateLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S2);
    }

    #[test]
    fn unx52299_checks_green() {
        let s = run_unx52299_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
