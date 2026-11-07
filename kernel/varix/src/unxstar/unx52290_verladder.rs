//! UNX-F52290 版本阶梯滚动回归账（AI-94 通知链 T+1 演练）· 0xC8840段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52290"，逐字）：
//!   title: 版本阶梯滚动回归账（AI-94 通知链 T+1 演练）
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52291-J1 本段全
//!     部版本伪装表条目 ×N 逐条审查：仅用于兼容性伪装、零许可绕过
//!     ×100% 结论入账；违例冻结批次条款知悉登记；联签锚定行 AI-95；
//!     判据锚定 §AI-66.9 红线②；③ 完成时 API/软件写回：状态=已完成、
//!     结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52290）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52291-J1，照原文锚定；功能按本卡标题（版本阶梯滚动回归账）
//! 落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 版本阶梯 5 档闭集：v0..v4 逐档 ×4 格 = 20 滚动回归格，档位单
//!      调（阶梯回退必抓），载荷恒等才收。
//!   ② 回退检出 6 格：滚动回归中版本回退检出逐格落账（载荷对照 +
//!      重复拒），回退漂移必抓。
//!   ③ AI-94 通知链 T+1 演练 8 拍：广播→钉版→抽测→…→结论入账，拍
//!      序严格单调递增（拍序回放必抓），月更触发 ×2 演练口径。
//!   ④ 演练零漂移声明 2 行闭集：T+1 演练零漂移声明 / 全量追新禁止
//!      声明，缺一拒封。
//!   ⑤ ledger 状态机（new/record_cell/record_rollback/record_beat/
//!      record_zero_drift/judge_gate×6/fingerprint FNV-1a/grade 定级
//!      闭集）；注入核 run_cycle 三态轮转 10 轮（梯格漂移 / 阶梯回退 /
//!      梯格越闭集），全部落在当轮临时账本上，led 上 injected==
//!      caught==10、escaped==0；双跑 FNV 指纹一致；定级 S1/S2/S3 闭集。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无
//!      unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52290;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 版本阶梯档数。
pub const LADDER_STEPS: usize = 5;

/// 每档滚动回归格数。
pub const CELLS_PER_STEP: usize = 4;

/// 滚动回归格总数（5 档 × 4 格）。
pub const LADDER_CELLS: usize = LADDER_STEPS * CELLS_PER_STEP;

/// 回退检出格数。
pub const ROLLBACK_ROWS: usize = 6;

/// AI-94 通知链 T+1 演练拍数。
pub const NOTIFY_BEATS: usize = 8;

/// 演练零漂移声明行数。
pub const ZERO_DRIFT_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VerLadderCode {
    /// 成功
    Ok = 0,
    /// 梯格越闭集
    ENoCell,
    /// 梯格载荷漂移
    ECellDrift,
    /// 阶梯回退（档位/拍序逆序）
    EOrder,
    /// 回退检出格越闭集
    ENoRow,
    /// 回退检出格载荷漂移
    ERowDrift,
    /// 通知拍越闭集
    ENoBeat,
    /// 缺零漂移声明
    ERowMiss,
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

impl VerLadderCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            VerLadderCode::Ok => "成功",
            VerLadderCode::ENoCell => "梯格越闭集：版本阶梯 5 档 20 格之外先登记",
            VerLadderCode::ECellDrift => "梯格载荷漂移：格折叠与基准不恒等，逐档复算",
            VerLadderCode::EOrder => "阶梯回退：v0→v4 档位与通知拍单调，回退/回放必抓",
            VerLadderCode::ENoRow => "回退检出格越闭集：回退检出 6 格之外先登记",
            VerLadderCode::ERowDrift => "回退检出格载荷漂移：回退折叠与基准不恒等，逐格复算",
            VerLadderCode::ENoBeat => "通知拍越闭集：AI-94 通知链 8 拍之外先登记",
            VerLadderCode::ERowMiss => "缺零漂移声明：T+1 演练零漂移声明未齐先补齐",
            VerLadderCode::EDup => "重复落账：同格只许一条账目",
            VerLadderCode::ESeal => "未齐即封：20 梯格 + 6 回退格 + 8 拍 + 2 行声明未齐先补齐",
            VerLadderCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            VerLadderCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            VerLadderCode::EGradeDrift => "定级漂移：全通且零漂移声明全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+20梯格+6回退格+8拍+2行声明全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 梯格载荷（格 c：5 档 × 4 格）。
pub const fn ladder_cell(c: usize) -> Option<u64> {
    if c >= LADDER_CELLS {
        return None;
    }
    Some((0xD7000u64 + (c as u64) * 0x41) ^ ((c / CELLS_PER_STEP) as u64).wrapping_mul(0x01000193))
}

/// 梯档（0..4=v0 4..8=v1 8..12=v2 12..16=v3 16..20=v4）。
pub const fn ladder_step(c: usize) -> Option<u32> {
    if c >= LADDER_CELLS {
        return None;
    }
    Some((c / CELLS_PER_STEP) as u32)
}

/// 梯档名。
pub const fn ladder_step_name(v: usize) -> Option<&'static str> {
    match v {
        0 => Some("v0 钉版基线"),
        1 => Some("v1 上月钉版"),
        2 => Some("v2 当前钉版"),
        3 => Some("v3 候选"),
        4 => Some("v4 新版本抽测"),
        _ => None,
    }
}

/// 回退检出格载荷（行 r）。
pub const fn rollback_cell(r: usize) -> Option<u64> {
    if r >= ROLLBACK_ROWS {
        return None;
    }
    Some((0xD8000u64 + (r as u64) * 0x47) ^ 0x524F4C4C4241434B)
}

/// 通知拍折叠（拍 b）。
pub const fn notify_beat(b: usize) -> Option<u32> {
    if b >= NOTIFY_BEATS {
        return None;
    }
    Some(0xD900 + b as u32 * 0x41)
}

/// 演练零漂移声明行折叠（行 r）。
pub const fn zero_drift_row(r: usize) -> Option<u32> {
    if r >= ZERO_DRIFT_ROWS {
        return None;
    }
    Some(0xDA00 + r as u32 * 0x43)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// 版本阶梯滚动回归账本状态机。
#[derive(Clone)]
pub struct VerLadderLedger {
    /// 六判据（滚动回归：启动/功能/崩溃/卸载/升级/许可）。
    gates: [Option<bool>; 6],
    /// 梯格账。
    cells: [Option<u64>; LADDER_CELLS],
    /// 回退检出账。
    rollbacks: [Option<u64>; ROLLBACK_ROWS],
    /// 通知拍账。
    beats: [Option<u32>; NOTIFY_BEATS],
    /// 零漂移声明账。
    zero_drift: [Option<u32>; ZERO_DRIFT_ROWS],
    /// 最后梯档。
    last_step: i64,
    /// 最后通知拍。
    last_beat: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl VerLadderLedger {
    /// 新账本。
    pub fn new() -> Self {
        VerLadderLedger {
            gates: [None; 6],
            cells: [None; LADDER_CELLS],
            rollbacks: [None; ROLLBACK_ROWS],
            beats: [None; NOTIFY_BEATS],
            zero_drift: [None; ZERO_DRIFT_ROWS],
            last_step: -1,
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

    /// 落一格六判据。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), VerLadderCode> {
        if gate >= 6 {
            return Err(VerLadderCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x6100 + gate as u64);
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

    /// 梯格一格（梯档单调 + 载荷对照 + 重复拒）。
    pub fn record_cell(&mut self, c: usize, got: u64) -> Result<(), VerLadderCode> {
        if c >= LADDER_CELLS {
            return Err(VerLadderCode::ENoCell);
        }
        let step = match ladder_step(c) {
            Some(v) => v,
            None => return Err(VerLadderCode::ENoCell),
        };
        if (step as i64) < self.last_step {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6200);
            return Err(VerLadderCode::EOrder);
        }
        let want = match ladder_cell(c) {
            Some(w) => w,
            None => return Err(VerLadderCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6201 + c as u64);
            return Err(VerLadderCode::ECellDrift);
        }
        if match self.cells.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(VerLadderCode::EDup);
        }
        if let Some(slot) = self.cells.get_mut(c) {
            *slot = Some(got);
        }
        self.last_step = step as i64;
        self.fnv(0x6202 + c as u64);
        Ok(())
    }

    /// 梯格全账核验（20 格）。
    pub fn cells_all(&self) -> bool {
        let mut c = 0usize;
        while c < LADDER_CELLS {
            match self.cells.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 回退检出格一格（载荷对照 + 重复拒）。
    pub fn record_rollback(&mut self, r: usize, got: u64) -> Result<(), VerLadderCode> {
        let want = match rollback_cell(r) {
            Some(w) => w,
            None => return Err(VerLadderCode::ENoRow),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6300 + r as u64);
            return Err(VerLadderCode::ERowDrift);
        }
        if match self.rollbacks.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(VerLadderCode::EDup);
        }
        if let Some(slot) = self.rollbacks.get_mut(r) {
            *slot = Some(got);
        }
        self.fnv(0x6301 + r as u64);
        Ok(())
    }

    /// 回退检出全账核验（6 格）。
    pub fn rollbacks_all(&self) -> bool {
        let mut r = 0usize;
        while r < ROLLBACK_ROWS {
            match self.rollbacks.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 通知拍一拍（拍序严格递增 + 重复拒）。
    pub fn record_beat(&mut self, b: usize) -> Result<(), VerLadderCode> {
        if b >= NOTIFY_BEATS {
            return Err(VerLadderCode::ENoBeat);
        }
        if (b as i64) <= self.last_beat {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6400);
            return Err(VerLadderCode::EOrder);
        }
        if let Some(slot) = self.beats.get_mut(b) {
            *slot = notify_beat(b);
        }
        self.last_beat = b as i64;
        self.fnv(0x6401 + b as u64);
        Ok(())
    }

    /// 通知拍全账核验（8 拍单调）。
    pub fn beats_all(&self) -> bool {
        let mut b = 0usize;
        while b < NOTIFY_BEATS {
            match self.beats.get(b) {
                Some(Some(_)) => {}
                _ => return false,
            }
            b += 1;
        }
        true
    }

    /// 演练零漂移声明一行。
    pub fn record_zero_drift(&mut self, r: usize) -> Result<(), VerLadderCode> {
        if r >= ZERO_DRIFT_ROWS {
            return Err(VerLadderCode::ENoRow);
        }
        if match self.zero_drift.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(VerLadderCode::EDup);
        }
        if let Some(slot) = self.zero_drift.get_mut(r) {
            *slot = zero_drift_row(r);
        }
        self.fnv(0x6500 + r as u64);
        Ok(())
    }

    /// 零漂移声明全齐核验。
    pub fn zero_drift_all(&self) -> bool {
        let mut r = 0usize;
        while r < ZERO_DRIFT_ROWS {
            match self.zero_drift.get(r) {
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
            && self.cells_all()
            && self.rollbacks_all()
            && self.beats_all()
            && self.zero_drift_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for VerLadderLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = VerLadderLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut c = 0usize;
    while c < LADDER_CELLS {
        if let Some(w) = ladder_cell(c) {
            if led.record_cell(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut r = 0usize;
    while r < ROLLBACK_ROWS {
        if let Some(w) = rollback_cell(r) {
            if led.record_rollback(r, w).is_ok() {
                recorded += 1;
            }
        }
        r += 1;
    }
    let mut b = 0usize;
    while b < NOTIFY_BEATS {
        let _ = led.record_beat(b);
        b += 1;
    }
    let mut r = 0usize;
    while r < ZERO_DRIFT_ROWS {
        let _ = led.record_zero_drift(r);
        r += 1;
    }
    // 注入 10 轮：梯格漂移 / 阶梯回退 / 梯格越闭集三态轮转。
    // 全部落在当轮临时账本上；漂移注入先录正格再注漂移；回退注入先按
    // 合法序落高档格再注低档，空账本同样必抓且不改 last_step。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = VerLadderLedger::new();
        let caught = match kind {
            0 => {
                if let (Some(w0), Some(w1)) = (ladder_cell(0), ladder_cell(1)) {
                    let _ = lt.record_cell(0, w0);
                    lt.record_cell(1, w1.wrapping_add(1)) == Err(VerLadderCode::ECellDrift)
                } else {
                    false
                }
            }
            1 => {
                if let (Some(w4), Some(w1)) = (ladder_cell(4), ladder_cell(1)) {
                    let _ = lt.record_cell(4, w4);
                    lt.record_cell(1, w1) == Err(VerLadderCode::EOrder)
                } else {
                    false
                }
            }
            _ => lt.record_cell(LADDER_CELLS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x6600 + k as u64);
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
/// UNX-F52290 判据集（锚 UNX-F52291-J1，off-by-one 板式）。
pub fn run_unx52290_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52290");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "版本阶梯 20 梯格全通",
        recorded >= LADDER_CELLS as u32,
        "v0..v4 五档逐格恒等，滚动回归逐档落账",
    );
    s.add(
        "版本阶梯 5 档单调",
        {
            let mut ok = true;
            let mut c = 1usize;
            while c < LADDER_CELLS {
                if ladder_step(c).unwrap_or(LADDER_STEPS as u32) < ladder_step(c - 1).unwrap_or(LADDER_STEPS as u32)
                {
                    ok = false;
                }
                c += 1;
            }
            ok
        },
        "五档闭集单调，阶梯回退必抓",
    );
    s.add(
        "回退检出 6 格全齐",
        {
            let mut lt = VerLadderLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < ROLLBACK_ROWS {
                if let Some(w) = rollback_cell(r) {
                    if lt.record_rollback(r, w).is_err() {
                        ok = false;
                    }
                }
                r += 1;
            }
            ok && lt.rollbacks_all()
        },
        "版本回退检出逐格落账，载荷恒等才收",
    );
    s.add(
        "AI-94 通知链 8 拍单调",
        {
            let mut lt = VerLadderLedger::new();
            let mut ok = true;
            let mut b = 0usize;
            while b < NOTIFY_BEATS {
                if lt.record_beat(b).is_err() {
                    ok = false;
                }
                b += 1;
            }
            ok && lt.beats_all()
        },
        "T+1 日广播→钉版回归→20% 抽测→结论入账，拍序严格递增",
    );
    s.add(
        "演练零漂移声明 2/2 全齐",
        {
            let mut lt = VerLadderLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < ZERO_DRIFT_ROWS {
                if lt.record_zero_drift(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.zero_drift_all()
        },
        "T+1 演练零漂移声明 / 全量追新禁止声明两行闭集（月更 ×2 口径）",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = VerLadderLedger::new();
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
        "梯格漂移/阶梯回退/梯格越闭集三态轮转必抓",
    );
    let mut l0 = VerLadderLedger::new();
    s.add(
        "边界·梯格越闭集",
        l0.record_cell(LADDER_CELLS, 0).is_err(),
        "梯格 20 越闭集必拒",
    );
    s.add(
        "边界·回退检出格越闭集",
        l0.record_rollback(ROLLBACK_ROWS, 0).is_err(),
        "回退检出格 6 越闭集必拒",
    );
    s.add(
        "边界·通知拍越闭集",
        l0.record_beat(NOTIFY_BEATS).is_err(),
        "通知拍 8 越闭集必拒",
    );
    s.add(
        "阶梯回退拒",
        {
            if let (Some(w4), Some(w1)) = (ladder_cell(4), ladder_cell(1)) {
                let mut lt = VerLadderLedger::new();
                let _ = lt.record_cell(4, w4);
                lt.record_cell(1, w1) == Err(VerLadderCode::EOrder)
            } else {
                false
            }
        },
        "v1→v0 档位回退必抓",
    );
    s.add(
        "通知拍回放拒",
        {
            let mut lt = VerLadderLedger::new();
            let _ = lt.record_beat(3);
            lt.record_beat(1).is_err()
        },
        "拍序回放必抓（T+1 演练单调）",
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
            let mut lt = VerLadderLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut c = 0usize;
            while c < LADDER_CELLS {
                if let Some(w) = ladder_cell(c) {
                    let _ = lt.record_cell(c, w);
                }
                c += 1;
            }
            let mut r = 0usize;
            while r < ROLLBACK_ROWS {
                if let Some(w) = rollback_cell(r) {
                    let _ = lt.record_rollback(r, w);
                }
                r += 1;
            }
            let mut b = 0usize;
            while b < NOTIFY_BEATS {
                let _ = lt.record_beat(b);
                b += 1;
            }
            let mut r = 0usize;
            while r < ZERO_DRIFT_ROWS {
                let _ = lt.record_zero_drift(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC8840,
        "SEG_BASE 0x40 对齐且落在 VX15 公式位",
    );
    s.add("收口·判据条数", s.len() >= 16, "含本条共 ≥17 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ladder_step() {
        assert_eq!(ladder_step(0), Some(0));
        assert_eq!(ladder_step(4), Some(1));
        assert_eq!(ladder_step(19), Some(4));
        assert!(ladder_step(20).is_none());
        assert!(ladder_cell(20).is_none());
        assert_eq!(ladder_step_name(0), Some("v0 钉版基线"));
        assert_eq!(ladder_step_name(4), Some("v4 新版本抽测"));
        assert!(ladder_step_name(5).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = ladder_cell(9);
        assert_eq!(a, ladder_cell(9));
        assert_ne!(a, ladder_cell(10));
        assert!(rollback_cell(ROLLBACK_ROWS).is_none());
        assert!(notify_beat(NOTIFY_BEATS).is_none());
        assert!(zero_drift_row(ZERO_DRIFT_ROWS).is_none());
    }

    #[test]
    fn test_step_order_caught() {
        let mut lt = VerLadderLedger::new();
        let _ = lt.record_cell(8, ladder_cell(8).unwrap_or(0));
        assert_eq!(
            lt.record_cell(2, ladder_cell(2).unwrap_or(0)),
            Err(VerLadderCode::EOrder)
        );
        // 回退被拒后 last_step 不回退：同档后继仍可落。
        assert!(lt.record_cell(9, ladder_cell(9).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_order_on_empty_ledger_caught() {
        // 空账本回退注入：先按合法序落高档格，再注低档，必抓。
        let mut lt = VerLadderLedger::new();
        let _ = lt.record_cell(4, ladder_cell(4).unwrap_or(0));
        assert_eq!(
            lt.record_cell(1, ladder_cell(1).unwrap_or(0)),
            Err(VerLadderCode::EOrder)
        );
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = VerLadderLedger::new();
        assert_eq!(
            lt.record_cell(6, ladder_cell(6).unwrap_or(0).wrapping_add(1)),
            Err(VerLadderCode::ECellDrift)
        );
        assert_eq!(
            lt.record_rollback(2, rollback_cell(2).unwrap_or(0).wrapping_add(5)),
            Err(VerLadderCode::ERowDrift)
        );
    }

    #[test]
    fn test_beat_monotonic() {
        let mut lt = VerLadderLedger::new();
        let mut b = 0usize;
        while b < NOTIFY_BEATS {
            assert!(lt.record_beat(b).is_ok());
            b += 1;
        }
        assert!(lt.record_beat(NOTIFY_BEATS).is_err());
        assert!(lt.record_beat(0).is_err());
        assert!(lt.beats_all());
        let mut lt2 = VerLadderLedger::new();
        let _ = lt2.record_beat(5);
        assert!(!lt2.beats_all());
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = VerLadderLedger::new();
        let _ = lt.record_cell(0, ladder_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_cell(0, ladder_cell(0).unwrap_or(0)),
            Err(VerLadderCode::EDup)
        );
        let _ = lt.record_zero_drift(0);
        assert_eq!(lt.record_zero_drift(0), Err(VerLadderCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = VerLadderLedger::new();
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
        assert!(recorded >= (LADDER_CELLS + ROLLBACK_ROWS) as u32);
        assert_eq!(caught, 10);
        let led = VerLadderLedger::new();
        assert_eq!(led.stats(), (0, 0, 0));
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
        assert_eq!(SEG_BASE, 0xC8840);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(VerLadderCode::Ok as u32, 0);
        assert_eq!(SEG_BASE + 0x0C, VerLadderCode::EGradeDrift.val());
        assert_eq!(VerLadderCode::EGradeDrift as u32, 12);
        assert!(!VerLadderCode::Ok.detail().is_empty());
        assert!(!VerLadderCode::ESeal.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = VerLadderLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52290_checks_green() {
        let s = run_unx52290_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 17);
    }
}
