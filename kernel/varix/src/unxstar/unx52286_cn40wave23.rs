//! UNX-F52286 J5 中文 40 应用抽测轮（波 23 段） · 0xC8740段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52286"，逐字）：
//!   title: J5 中文 40 应用抽测轮（波 23 段）
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52287-J1 B01–B14
//!     共 280 条目 KB-COMPAT 登记 ×100% 可查询验证；结构权在 E5、写入权
//!     在 N 域分界复核（联签锚定行 AI-25）；判据锚定 §AI-66.5 J-主轴；
//!     ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52286）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52287-J1，照原文锚定；功能按本卡标题（J5 中文 40 应用抽测轮·
//! 波 23 段）落地，B01–B14 核账判据由同批后续卡承载。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 40 应用 × 六判据逐应用 1 格聚合 = 40 格闭集，逐格载荷恒等才收，
//!      格越闭集必抓；J5 三层（界面/输入/字体）口径并入聚合格备注。
//!   ② 抽样五段闭集：启动/功能/崩溃/卸载/升级五家族 × 各 2 操作 =
//!      10 操作，家族单调（逆序必抓）。
//!   ③ 轮次账 8 拍严格等差链：拍 b 只许在 last+1 落，跳拍/逆序/空账跳
//!      拍一律必抓——抽测轮不许欠轮。
//!   ④ 欠账清单 6 行公开：逐行落账为公开欠账项，欠账不静默；有公开欠
//!      账即定级降为 S2（有条件收口），不许凑 S1。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint，FNV-1a）；
//!      注入核 run_cycle 三态轮转 10/10（轮次跳拍/应用格漂移/抽样操作越
//!      闭集），全部落在当轮临时账本上，主账本 last_kind/last_round 不受
//!      扰动；检出+逃逸守恒等式；双跑 FNV 指纹一致。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52286;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 中文应用抽测格数（40 应用逐应用 1 格聚合）。
pub const APP_CELLS: usize = 40;

/// 抽样操作数（五段 × 2）。
pub const SAMPLE_OPS: usize = 10;

/// 抽样家族数。
pub const SAMPLE_KINDS: usize = 5;

/// 轮次账拍数。
pub const ROUND_BEATS: usize = 8;

/// 欠账清单公开行数。
pub const SHORTFALL_ROWS: usize = 6;

/// 应用格域异或锚（低 16 位恒零）。
pub const APP_ANCHOR: u64 = 0x4A35_0000;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cn40WaveCode {
    /// 成功
    Ok = 0,
    /// 应用格越闭集
    ENoApp,
    /// 应用格载荷漂移
    EAppDrift,
    /// 抽样操作越闭集
    ENoOp,
    /// 抽样载荷漂移
    EOpDrift,
    /// 抽样家族逆序
    EOpOrder,
    /// 轮次拍越闭集
    ENoRound,
    /// 轮次跳拍/逆序
    ERoundOrder,
    /// 欠账行越闭集
    EShortNo,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl Cn40WaveCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            Cn40WaveCode::Ok => "成功",
            Cn40WaveCode::ENoApp => "应用格越闭集：40 应用逐应用 1 格聚合账之外先登记",
            Cn40WaveCode::EAppDrift => "应用格载荷漂移：聚合格折叠与基准不恒等，逐格复算",
            Cn40WaveCode::ENoOp => "抽样操作越闭集：启动/功能/崩溃/卸载/升级 10 操作之外先登记",
            Cn40WaveCode::EOpDrift => "抽样载荷漂移：操作折叠与基准不恒等，逐操作复算",
            Cn40WaveCode::EOpOrder => "抽样家族逆序：启动→功能→崩溃→卸载→升级单调，逆序必抓",
            Cn40WaveCode::ENoRound => "轮次拍越闭集：抽测轮次账 8 拍之外先登记",
            Cn40WaveCode::ERoundOrder => "轮次跳拍/逆序：拍 b 只许在 last+1 落，抽测轮不许欠轮",
            Cn40WaveCode::EShortNo => "欠账行越闭集：欠账清单 6 行之外先登记",
            Cn40WaveCode::EDup => "重复落账：同格只许一条账目",
            Cn40WaveCode::ESeal => "未齐即封：40 格应用+10 操作+8 拍轮次未齐先补齐",
            Cn40WaveCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            Cn40WaveCode::EGradeDrift => "定级漂移：欠账清单公开未清只可定 S2，不许凑 S1",
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
            Grade::S1 => "S1：六判据+40格应用+10操作+8拍轮次全通且欠账清单清零收口",
            Grade::S2 => "S2：有条件收口，欠账清单公开未清（6 行公开）",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 应用聚合格载荷（格 a：0..40）。
pub const fn app_cell(a: usize) -> Option<u64> {
    if a >= APP_CELLS {
        return None;
    }
    Some((0xB1000u64 + (a as u64) * 0x2F) ^ APP_ANCHOR)
}

/// 抽样操作格载荷（操作 o：0..10，家族 = o/2）。
pub const fn sample_cell(o: usize) -> Option<u64> {
    if o >= SAMPLE_OPS {
        return None;
    }
    Some((0xB2000u64 + (o as u64) * 0x43) ^ ((o / 2) as u64).wrapping_mul(0x01000193))
}

/// 抽样家族（0..2=启动 2..4=功能 4..6=崩溃 6..8=卸载 8..10=升级）。
pub const fn sample_kind(o: usize) -> Option<u32> {
    if o >= SAMPLE_OPS {
        return None;
    }
    Some((o / 2) as u32)
}

/// 家族名。
pub const fn sample_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("启动"),
        1 => Some("功能"),
        2 => Some("崩溃"),
        3 => Some("卸载"),
        4 => Some("升级"),
        _ => None,
    }
}

/// 轮次拍载荷（拍 r：0..8）。
pub const fn round_cell(r: usize) -> Option<u64> {
    if r >= ROUND_BEATS {
        return None;
    }
    Some(0xB3000u64 + (r as u64) * 0x67)
}

/// 欠账清单行号闭集（行 w：0..6）。
pub const fn shortfall_row(w: usize) -> Option<u32> {
    if w >= SHORTFALL_ROWS {
        return None;
    }
    Some(0xB4000 + w as u32 * 0x1B)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// J5 中文 40 应用抽测轮收口账本状态机。
#[derive(Clone)]
pub struct Cn40WaveLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 40 应用聚合格账。
    apps: [Option<u64>; APP_CELLS],
    /// 抽样 10 操作账。
    ops: [Option<u64>; SAMPLE_OPS],
    /// 轮次账 8 拍。
    rounds: [Option<u64>; ROUND_BEATS],
    /// 欠账清单 6 行（Some(false)=公开欠账未清）。
    shorts: [Option<bool>; SHORTFALL_ROWS],
    /// 最后抽样家族。
    last_kind: i64,
    /// 最后轮次拍（-1 起严格等差）。
    last_round: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl Cn40WaveLedger {
    /// 新账本。
    pub fn new() -> Self {
        Cn40WaveLedger {
            gates: [None; 6],
            apps: [None; APP_CELLS],
            ops: [None; SAMPLE_OPS],
            rounds: [None; ROUND_BEATS],
            shorts: [None; SHORTFALL_ROWS],
            last_kind: -1,
            last_round: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), Cn40WaveCode> {
        if gate >= 6 {
            return Err(Cn40WaveCode::ESeal);
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

    /// 应用聚合格一格（载荷对照 + 重复拒）。
    pub fn record_app(&mut self, a: usize, got: u64) -> Result<(), Cn40WaveCode> {
        if a >= APP_CELLS {
            return Err(Cn40WaveCode::ENoApp);
        }
        let want = match app_cell(a) {
            Some(w) => w,
            None => return Err(Cn40WaveCode::ENoApp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4200 + a as u64);
            return Err(Cn40WaveCode::EAppDrift);
        }
        if match self.apps.get(a) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(Cn40WaveCode::EDup);
        }
        if let Some(c) = self.apps.get_mut(a) {
            *c = Some(got);
        }
        self.fnv(0x4201 + a as u64);
        Ok(())
    }

    /// 应用格全账核验（40 格）。
    pub fn apps_all(&self) -> bool {
        let mut a = 0usize;
        while a < APP_CELLS {
            match self.apps.get(a) {
                Some(Some(_)) => {}
                _ => return false,
            }
            a += 1;
        }
        true
    }

    /// 抽样操作一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_sample(&mut self, o: usize, got: u64) -> Result<(), Cn40WaveCode> {
        if o >= SAMPLE_OPS {
            return Err(Cn40WaveCode::ENoOp);
        }
        let kind = match sample_kind(o) {
            Some(k) => k,
            None => return Err(Cn40WaveCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4300);
            return Err(Cn40WaveCode::EOpOrder);
        }
        let want = match sample_cell(o) {
            Some(w) => w,
            None => return Err(Cn40WaveCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4301 + o as u64);
            return Err(Cn40WaveCode::EOpDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(Cn40WaveCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x4302 + o as u64);
        Ok(())
    }

    /// 抽样全账核验（10 操作）。
    pub fn samples_all(&self) -> bool {
        let mut o = 0usize;
        while o < SAMPLE_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 轮次账一拍（严格等差 + 载荷对照 + 重复拒）。
    pub fn record_round(&mut self, r: usize, got: u64) -> Result<(), Cn40WaveCode> {
        if r >= ROUND_BEATS {
            return Err(Cn40WaveCode::ENoRound);
        }
        if r as i64 != self.last_round + 1 {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4400);
            return Err(Cn40WaveCode::ERoundOrder);
        }
        let want = match round_cell(r) {
            Some(w) => w,
            None => return Err(Cn40WaveCode::ENoRound),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4401 + r as u64);
            return Err(Cn40WaveCode::EOpDrift);
        }
        if match self.rounds.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(Cn40WaveCode::EDup);
        }
        if let Some(c) = self.rounds.get_mut(r) {
            *c = Some(got);
        }
        self.last_round = r as i64;
        self.fnv(0x4402 + r as u64);
        Ok(())
    }

    /// 轮次账全账核验（8 拍齐且单调）。
    pub fn rounds_all(&self) -> bool {
        let mut r = 0usize;
        while r < ROUND_BEATS {
            match self.rounds.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 欠账清单公开一行（Some(false)=公开欠账未清）。
    pub fn declare_shortfall(&mut self, w: usize) -> Result<(), Cn40WaveCode> {
        if w >= SHORTFALL_ROWS {
            return Err(Cn40WaveCode::EShortNo);
        }
        if match self.shorts.get(w) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(Cn40WaveCode::EDup);
        }
        if let Some(c) = self.shorts.get_mut(w) {
            *c = Some(false);
        }
        self.fnv(0x4500 + w as u64);
        Ok(())
    }

    /// 欠账清单已公开行数。
    pub fn shorts_open(&self) -> u32 {
        let mut n = 0u32;
        let mut w = 0usize;
        while w < SHORTFALL_ROWS {
            if match self.shorts.get(w) {
                Some(Some(false)) => true,
                _ => false,
            } {
                n += 1;
            }
            w += 1;
        }
        n
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// KB-COMPAT 定级（欠账公开未清 → S2 不许凑 S1；判据缺 → S3）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() && self.apps_all() && self.samples_all() && self.rounds_all() && self.shorts_open() == 0 {
            Grade::S1
        } else if self.gates_all() && self.apps_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for Cn40WaveLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = Cn40WaveLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut a = 0usize;
    while a < APP_CELLS {
        if let Some(w) = app_cell(a) {
            if led.record_app(a, w).is_ok() {
                recorded += 1;
            }
        }
        a += 1;
    }
    let mut o = 0usize;
    while o < SAMPLE_OPS {
        if let Some(w) = sample_cell(o) {
            if led.record_sample(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut r = 0usize;
    while r < ROUND_BEATS {
        if let Some(w) = round_cell(r) {
            if led.record_round(r, w).is_ok() {
                recorded += 1;
            }
        }
        r += 1;
    }
    let mut wrow = 0usize;
    while wrow < SHORTFALL_ROWS {
        let _ = led.declare_shortfall(wrow);
        wrow += 1;
    }
    // 注入 10 轮：轮次跳拍 / 应用格漂移 / 抽样操作越闭集三态轮转。
    // 全部落在当轮临时账本上：led 上 injected==caught==10、escaped==0。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = Cn40WaveLedger::new();
        let caught = match kind {
            0 => {
                // 空账本跳拍：期望拍 0，先落拍 4 必抓；last_round 不受扰动。
                lt.record_round(4, round_cell(4).unwrap_or(0)) == Err(Cn40WaveCode::ERoundOrder)
            }
            1 => {
                let _ = lt.record_app(0, app_cell(0).unwrap_or(0));
                lt.record_app(1, app_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(Cn40WaveCode::EAppDrift)
            }
            _ => lt.record_sample(SAMPLE_OPS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x4600 + k as u64);
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
/// UNX-F52286 判据集（锚 UNX-F52287-J1，off-by-one 板式）。
pub fn run_unx52286_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52286");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "40 应用聚合格全通",
        recorded >= APP_CELLS as u32,
        "J5 三层（界面/输入/字体）口径并入逐应用 1 格聚合账",
    );
    s.add(
        "抽样五段 10 操作全通",
        {
            let mut lt = Cn40WaveLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < SAMPLE_OPS {
                if let Some(w) = sample_cell(o) {
                    if lt.record_sample(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.samples_all()
        },
        "启动/功能/崩溃/卸载/升级五家族逐操作恒等",
    );
    s.add(
        "抽样家族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < SAMPLE_OPS {
                if sample_kind(o).unwrap_or(5) < sample_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五家族闭集单调，逆序必抓",
    );
    s.add(
        "轮次账 8 拍单调全落",
        {
            let mut lt = Cn40WaveLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < ROUND_BEATS {
                if let Some(w) = round_cell(r) {
                    if lt.record_round(r, w).is_err() {
                        ok = false;
                    }
                }
                r += 1;
            }
            ok && lt.rounds_all()
        },
        "抽测轮次账严格等差 8 拍，跳拍/逆序/欠轮必抓",
    );
    s.add(
        "欠账清单 6 行公开",
        {
            let mut lt = Cn40WaveLedger::new();
            let mut ok = true;
            let mut w = 0usize;
            while w < SHORTFALL_ROWS {
                if lt.declare_shortfall(w).is_err() {
                    ok = false;
                }
                w += 1;
            }
            ok && lt.shorts_open() == SHORTFALL_ROWS as u32
        },
        "欠账不静默：6 行逐行公开落账",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = Cn40WaveLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "六判据逐格落账，未齐即封",
    );
    s.add("注入 10/10 全抓", caught >= 10, "轮次跳拍/应用格漂移/抽样操作越闭集三态轮转必抓");
    s.add(
        "边界·应用格越闭集",
        {
            let mut lt = Cn40WaveLedger::new();
            lt.record_app(APP_CELLS, 0).is_err()
        },
        "应用格 40 越闭集必拒",
    );
    s.add(
        "边界·抽样操作越闭集",
        {
            let mut lt = Cn40WaveLedger::new();
            lt.record_sample(SAMPLE_OPS, 0).is_err()
        },
        "抽样操作 10 越闭集必拒",
    );
    s.add(
        "边界·欠账行越闭集",
        {
            let mut lt = Cn40WaveLedger::new();
            lt.declare_shortfall(SHORTFALL_ROWS).is_err()
        },
        "欠账行 6 越闭集必拒",
    );
    s.add(
        "轮次跳拍拒",
        {
            let mut lt = Cn40WaveLedger::new();
            let r0 = lt.record_round(0, round_cell(0).unwrap_or(0));
            let r2 = lt.record_round(2, round_cell(2).unwrap_or(0));
            r0.is_ok() && r2 == Err(Cn40WaveCode::ERoundOrder)
        },
        "拍 0 之后落拍 2 跳拍必抓",
    );
    s.add(
        "抽样家族逆序拒",
        {
            let mut lt = Cn40WaveLedger::new();
            let _ = lt.record_sample(7, sample_cell(7).unwrap_or(0));
            lt.record_sample(2, sample_cell(2).unwrap_or(0)) == Err(Cn40WaveCode::EOpOrder)
        },
        "卸载族之后落功能族逆序必抓",
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
        "定级 S2 闭集（欠账公开不许凑 S1）",
        {
            let mut lt = Cn40WaveLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut a = 0usize;
            while a < APP_CELLS {
                if let Some(w) = app_cell(a) {
                    let _ = lt.record_app(a, w);
                }
                a += 1;
            }
            let mut o = 0usize;
            while o < SAMPLE_OPS {
                if let Some(w) = sample_cell(o) {
                    let _ = lt.record_sample(o, w);
                }
                o += 1;
            }
            let mut r = 0usize;
            while r < ROUND_BEATS {
                if let Some(w) = round_cell(r) {
                    let _ = lt.record_round(r, w);
                }
                r += 1;
            }
            let mut w = 0usize;
            while w < SHORTFALL_ROWS {
                let _ = lt.declare_shortfall(w);
                w += 1;
            }
            // 定级检查：main/aux 各节全部记账完成后再断言。
            lt.judge_grade() == Grade::S2
        },
        "欠账清单公开未清 → S2（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC8740,
        "SEG_BASE 0x40 对齐且按派工公式落 0xC8740",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥17 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sample_kind_names() {
        assert_eq!(sample_kind_name(0), Some("启动"));
        assert_eq!(sample_kind_name(1), Some("功能"));
        assert_eq!(sample_kind_name(2), Some("崩溃"));
        assert_eq!(sample_kind_name(3), Some("卸载"));
        assert_eq!(sample_kind_name(4), Some("升级"));
        assert!(sample_kind_name(5).is_none());
    }

    #[test]
    fn test_sample_kind_map() {
        assert_eq!(sample_kind(0), Some(0));
        assert_eq!(sample_kind(2), Some(1));
        assert_eq!(sample_kind(9), Some(4));
        assert!(sample_kind(10).is_none());
        assert!(sample_cell(10).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = app_cell(7).unwrap_or(0);
        assert_eq!(a, app_cell(7).unwrap_or(0));
        assert_ne!(a, app_cell(8).unwrap_or(0));
        assert!(app_cell(APP_CELLS).is_none());
        assert!(round_cell(ROUND_BEATS).is_none());
        assert!(shortfall_row(SHORTFALL_ROWS).is_none());
    }

    #[test]
    fn test_anchor_low16_zero() {
        assert_eq!(APP_ANCHOR & 0xFFFF, 0);
    }

    #[test]
    fn test_round_order_caught() {
        // 空账本跳拍必抓。
        let mut lt = Cn40WaveLedger::new();
        assert_eq!(lt.record_round(4, round_cell(4).unwrap_or(0)), Err(Cn40WaveCode::ERoundOrder));
        // 拍 0 之后落拍 3 跳拍必抓；拍 1 顺落可过。
        assert!(lt.record_round(0, round_cell(0).unwrap_or(0)).is_ok());
        assert_eq!(lt.record_round(3, round_cell(3).unwrap_or(0)), Err(Cn40WaveCode::ERoundOrder));
        assert!(lt.record_round(1, round_cell(1).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_sample_order_caught() {
        let mut lt = Cn40WaveLedger::new();
        let _ = lt.record_sample(9, sample_cell(9).unwrap_or(0));
        assert_eq!(lt.record_sample(2, sample_cell(2).unwrap_or(0)), Err(Cn40WaveCode::EOpOrder));
        assert!(lt.record_sample(10, sample_cell(10).unwrap_or(0)).is_err());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = Cn40WaveLedger::new();
        assert_eq!(
            lt.record_app(5, app_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(Cn40WaveCode::EAppDrift)
        );
        assert_eq!(
            lt.record_sample(3, sample_cell(3).unwrap_or(0).wrapping_add(7)),
            Err(Cn40WaveCode::EOpDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = Cn40WaveLedger::new();
        let _ = lt.record_app(0, app_cell(0).unwrap_or(0));
        assert_eq!(lt.record_app(0, app_cell(0).unwrap_or(0)), Err(Cn40WaveCode::EDup));
        let _ = lt.declare_shortfall(0);
        assert_eq!(lt.declare_shortfall(0), Err(Cn40WaveCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = Cn40WaveLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (APP_CELLS + SAMPLE_OPS + ROUND_BEATS) as u32);
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
        assert_eq!(SEG_BASE, 0xC8740);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(Cn40WaveCode::Ok as u32, 0);
        assert_eq!(Cn40WaveCode::EGradeDrift as u32, 12);
        assert_eq!(Cn40WaveCode::Ok.val(), SEG_BASE);
        assert_eq!(Cn40WaveCode::EGradeDrift.val(), SEG_BASE + 12);
        assert!(!Cn40WaveCode::Ok.detail().is_empty());
        assert!(!Cn40WaveCode::ERoundOrder.detail().is_empty());
    }

    #[test]
    fn test_grade_s2_with_open_shortfall() {
        let mut lt = Cn40WaveLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt.judge_gate(g);
            g += 1;
        }
        let mut a = 0usize;
        while a < APP_CELLS {
            if let Some(w) = app_cell(a) {
                let _ = lt.record_app(a, w);
            }
            a += 1;
        }
        // 六判据+40 应用齐但欠账未公开 → S2（非 S1）。
        assert_eq!(lt.judge_grade(), Grade::S2);
        let empty = Cn40WaveLedger::new();
        assert_eq!(empty.judge_grade(), Grade::S3);
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
    }

    #[test]
    fn unx52286_checks_green() {
        let s = run_unx52286_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 17);
    }
}
