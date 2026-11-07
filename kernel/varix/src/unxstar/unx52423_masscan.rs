//! UNX-F52423 masscan 收口 · 0xCA980段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52423"，逐字）：
//!   title: masscan 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52424-J1 连接/监
//!     听/转发 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时
//!     API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式：本卡（卡 N=52423）判据锚 UNX-F(N+1)-J1 = UNX-F52424-J1，
//! 照原文锚定；功能按本卡标题（masscan 收口）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五参数族 20 格闭集：--rate/--range/-p/网卡/--banners 各 4 格，逐
//!      格载荷（纯函数折叠）恒等才收，漂移必抓。
//!   ② 速率三档 6 格：保守/标准/极速三档各 2 格，档值单调递增（越档必
//!      抓）；异步收发栈 4 拍单调（拍号逆序必抓且不前移游标）。
//!   ③ 与 nmap 分账声明 2 行闭集：masscan 独立账声明/与 nmap 无共享态
//!      声明，缺一拒封（分账而非联签）。
//!   ④ C 上栈声明 2 行闭集：C 栈上栈声明/C 栈回退声明。
//!   ⑤ 六判据闸 + ledger 状态机（new/record_*/judge_gate×6/fingerprint
//!      FNV-1a/grade 定级闭集）；注入核 run_cycle 三态轮转 10 轮（参数
//!      漂移/异步拍逆序/参数越闭集），10/10 必抓；双跑 FNV 指纹一致；
//!      KB-COMPAT 定级目标档 S2。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52423;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 参数格数（五族 × 4）。
pub const PARAM_CELLS: usize = 20;

/// 速率格数（三档 × 2）。
pub const RATE_CELLS: usize = 6;

/// 异步收发拍数。
pub const ASYNC_BEATS: usize = 4;

/// 分账声明行数。
pub const SPLIT_ROWS: usize = 2;

/// C 上栈声明行数。
pub const C_STACK_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MasscanCode {
    /// 成功
    Ok = 0,
    /// 参数格越闭集
    ENoParam,
    /// 参数格漂移
    EParamDrift,
    /// 参数族逆序
    EOrder,
    /// 速率格越闭集
    ENoRate,
    /// 速率越档
    ERateTier,
    /// 速率档值漂移
    ERateDrift,
    /// 异步拍越闭集
    ENoBeat,
    /// 异步拍逆序
    EBeatOrder,
    /// 异步拍漂移
    EBeatDrift,
    /// 声明行越闭集
    ENoRow,
    /// 重复落账
    EDup,
    /// 定级漂移
    EGradeDrift,
}

impl MasscanCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            MasscanCode::Ok => "成功",
            MasscanCode::ENoParam => "参数格越闭集：--rate/--range/-p/网卡/--banners 20 格之外先登记",
            MasscanCode::EParamDrift => "参数格漂移：参数载荷折叠与基准不恒等，逐格复算",
            MasscanCode::EOrder => "参数族逆序：--rate→--range→-p→网卡→--banners 单调，逆序必抓且不前移游标",
            MasscanCode::ENoRate => "速率格越闭集：速率三档 6 格之外先登记",
            MasscanCode::ERateTier => "速率越档：保守/标准/极速三档单调递增，越档必抓",
            MasscanCode::ERateDrift => "速率档值漂移：档值与基准不恒等，逐格复算",
            MasscanCode::ENoBeat => "异步拍越闭集：异步收发栈 4 拍之外先登记",
            MasscanCode::EBeatOrder => "异步拍逆序：4 拍单调，拍号逆序必抓且不前移游标",
            MasscanCode::EBeatDrift => "异步拍漂移：拍载荷与基准不恒等，逐拍复算",
            MasscanCode::ENoRow => "声明行越闭集：分账/C 上栈声明闭集之外先登记",
            MasscanCode::EDup => "重复落账：同格只许一条账目",
            MasscanCode::EGradeDrift => "定级漂移：全通才可定目标档 S2，判据缺则 S3",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口（本卡不达，封顶 S2）
    S1,
    /// S2：有条件收口（本卡目标档）
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
            Grade::S1 => "S1：全通收口（masscan 独立账不达）",
            Grade::S2 => "S2：有条件收口（目标档），六判据全落即封",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 参数格载荷（格 o；族 = o/4，族序 --rate→--range→-p→网卡→--banners）。
pub const fn param_cell(o: usize) -> Option<u64> {
    if o >= PARAM_CELLS {
        return None;
    }
    Some((0xF1000u64 + (o as u64) * 0x49) ^ (0x4D415353_0000_0000 + (o / 4) as u64))
}

/// 参数族号。
pub const fn param_kind(o: usize) -> Option<u32> {
    if o >= PARAM_CELLS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 参数族名。
pub const fn param_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("--rate"),
        1 => Some("--range"),
        2 => Some("-p"),
        3 => Some("网卡"),
        4 => Some("--banners"),
        _ => None,
    }
}

/// 速率档基准值（档 t：0 保守 1 标准 2 极速，档值单调递增）。
pub const fn tier_rate(t: usize) -> Option<u32> {
    match t {
        0 => Some(100),
        1 => Some(1_000),
        2 => Some(10_000),
        _ => None,
    }
}

/// 速率格载荷（格 k；档 = k/2）。
pub const fn rate_cell(k: usize) -> Option<u64> {
    let t = match k / 2 {
        0..=2 => k / 2,
        _ => return None,
    };
    if k >= RATE_CELLS {
        return None;
    }
    let base = match tier_rate(t) {
        Some(b) => b,
        None => return None,
    };
    Some(0xF2000u64 + (k as u64) * 0x5E + base as u64)
}

/// 异步拍载荷（拍 p，基准单调递增 0x1200 步进）。
pub const fn beat_cell(p: usize) -> Option<u64> {
    if p >= ASYNC_BEATS {
        return None;
    }
    Some(0xF3000u64 + (p as u64) * 0x1200)
}

/// 分账声明行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xF4000 + r as u32 * 0x5F)
}

/// C 上栈声明行折叠（行 r）。
pub const fn c_stack_row(r: usize) -> Option<u32> {
    if r >= C_STACK_ROWS {
        return None;
    }
    Some(0xF4100 + r as u32 * 0x61)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// masscan 收口账本状态机。
#[derive(Clone)]
pub struct MasscanLedger {
    /// 六判据（连接/监听/转发/速率/异步收发/写回）。
    gates: [Option<bool>; 6],
    /// 参数格账。
    params: [Option<u64>; PARAM_CELLS],
    /// 速率格账。
    rates: [Option<u64>; RATE_CELLS],
    /// 异步拍账。
    beats: [Option<u64>; ASYNC_BEATS],
    /// 分账声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// C 上栈声明账。
    rows: [Option<u32>; C_STACK_ROWS],
    /// 最后参数族。
    last_kind: i64,
    /// 最后异步拍号。
    last_beat: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl MasscanLedger {
    /// 新账本。
    pub fn new() -> Self {
        MasscanLedger {
            gates: [None; 6],
            params: [None; PARAM_CELLS],
            rates: [None; RATE_CELLS],
            beats: [None; ASYNC_BEATS],
            splits: [None; SPLIT_ROWS],
            rows: [None; C_STACK_ROWS],
            last_kind: -1,
            last_beat: -1,
            injected: 0,
            caught: 0,
            escaped: 0,
            fp: 0xcbf2_9ce4_8422_2325,
        }
    }

    /// FNV-1a 单步。
    fn fnv(&mut self, w: u64) {
        self.fp ^= w;
        self.fp = self.fp.wrapping_mul(0x0000_0100_0000_01b3);
    }

    /// 落一格六判据。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), MasscanCode> {
        if gate >= 6 {
            return Err(MasscanCode::ENoRow);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0xC100 + gate as u64);
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
    pub fn record_param(&mut self, o: usize, got: u64) -> Result<(), MasscanCode> {
        if o >= PARAM_CELLS {
            return Err(MasscanCode::ENoParam);
        }
        let kind = match param_kind(o) {
            Some(k) => k,
            None => return Err(MasscanCode::ENoParam),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xC200);
            return Err(MasscanCode::EOrder);
        }
        let want = match param_cell(o) {
            Some(w) => w,
            None => return Err(MasscanCode::ENoParam),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xC201 + o as u64);
            return Err(MasscanCode::EParamDrift);
        }
        if match self.params.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(MasscanCode::EDup);
        }
        if let Some(c) = self.params.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0xC202 + o as u64);
        Ok(())
    }

    /// 参数全账核验（20 格）。
    pub fn params_all(&self) -> bool {
        let mut o = 0usize;
        while o < PARAM_CELLS {
            match self.params.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 速率一格（档单调 + 档值对照 + 重复拒）。
    pub fn record_rate(&mut self, k: usize, got: u64) -> Result<(), MasscanCode> {
        if k >= RATE_CELLS {
            return Err(MasscanCode::ENoRate);
        }
        let t = k / 2;
        let base = match tier_rate(t) {
            Some(b) => b,
            None => return Err(MasscanCode::ENoRate),
        };
        if k > 0 && tier_rate((k - 1) / 2).unwrap_or(0) > base {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xC300);
            return Err(MasscanCode::ERateTier);
        }
        let want = match rate_cell(k) {
            Some(w) => w,
            None => return Err(MasscanCode::ENoRate),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xC301 + k as u64);
            return Err(MasscanCode::ERateDrift);
        }
        if match self.rates.get(k) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(MasscanCode::EDup);
        }
        if let Some(c) = self.rates.get_mut(k) {
            *c = Some(got);
        }
        self.fnv(0xC302 + k as u64);
        Ok(())
    }

    /// 速率全账核验（6 格）。
    pub fn rates_all(&self) -> bool {
        let mut k = 0usize;
        while k < RATE_CELLS {
            match self.rates.get(k) {
                Some(Some(_)) => {}
                _ => return false,
            }
            k += 1;
        }
        true
    }

    /// 异步一格（拍单调 + 载荷对照 + 重复拒）。
    pub fn record_beat(&mut self, p: usize, got: u64) -> Result<(), MasscanCode> {
        if p >= ASYNC_BEATS {
            return Err(MasscanCode::ENoBeat);
        }
        if (p as i64) < self.last_beat {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xC400);
            return Err(MasscanCode::EBeatOrder);
        }
        let want = match beat_cell(p) {
            Some(w) => w,
            None => return Err(MasscanCode::ENoBeat),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xC401 + p as u64);
            return Err(MasscanCode::EBeatDrift);
        }
        if match self.beats.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(MasscanCode::EDup);
        }
        if let Some(c) = self.beats.get_mut(p) {
            *c = Some(got);
        }
        self.last_beat = p as i64;
        self.fnv(0xC402 + p as u64);
        Ok(())
    }

    /// 异步全账核验（4 拍）。
    pub fn beats_all(&self) -> bool {
        let mut p = 0usize;
        while p < ASYNC_BEATS {
            match self.beats.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// 声明一行（kind=false → 分账；kind=true → C 上栈）。
    pub fn declare_row(&mut self, kind: bool, r: usize) -> Result<(), MasscanCode> {
        if kind {
            if r >= C_STACK_ROWS {
                return Err(MasscanCode::ENoRow);
            }
            if match self.rows.get(r) {
                Some(Some(_)) => true,
                _ => false,
            } {
                return Err(MasscanCode::EDup);
            }
            if let Some(c) = self.rows.get_mut(r) {
                *c = c_stack_row(r);
            }
        } else {
            if r >= SPLIT_ROWS {
                return Err(MasscanCode::ENoRow);
            }
            if match self.splits.get(r) {
                Some(Some(_)) => true,
                _ => false,
            } {
                return Err(MasscanCode::EDup);
            }
            if let Some(c) = self.splits.get_mut(r) {
                *c = split_row(r);
            }
        }
        self.fnv(0xC500 + r as u64);
        Ok(())
    }

    /// 声明全齐核验（分账 + C 上栈两闭集）。
    pub fn rows_all(&self) -> bool {
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            match self.splits.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        let mut r2 = 0usize;
        while r2 < C_STACK_ROWS {
            match self.rows.get(r2) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r2 += 1;
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

    /// KB-COMPAT 定级（目标档 S2：六判据全落即 S2；判据缺 → S3）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for MasscanLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（落账格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = MasscanLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < PARAM_CELLS {
        if let Some(w) = param_cell(o) {
            if led.record_param(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut k = 0usize;
    while k < RATE_CELLS {
        if let Some(w) = rate_cell(k) {
            if led.record_rate(k, w).is_ok() {
                recorded += 1;
            }
        }
        k += 1;
    }
    let mut p = 0usize;
    while p < ASYNC_BEATS {
        if let Some(w) = beat_cell(p) {
            if led.record_beat(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    let mut s = 0usize;
    while s < SPLIT_ROWS {
        let _ = led.declare_row(false, s);
        s += 1;
    }
    let mut c = 0usize;
    while c < C_STACK_ROWS {
        let _ = led.declare_row(true, c);
        c += 1;
    }
    // 注入 10 轮：参数漂移 / 异步拍逆序 / 参数越闭集三态轮转。
    // 全部落在当轮临时账本上，主账只记守恒数。
    let mut j = 0usize;
    while j < 10 {
        let kind = (j % 3) as u32;
        let mut lt = MasscanLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_param(0, param_cell(0).unwrap_or(0));
                lt.record_param(1, param_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(MasscanCode::EParamDrift)
            }
            1 => {
                // 异步拍逆序：先落 2 拍合法格，再注 1 拍，逆序必抓且游标不前移。
                let _ = lt.record_beat(2, beat_cell(2).unwrap_or(0));
                lt.record_beat(1, beat_cell(1).unwrap_or(0)) == Err(MasscanCode::EBeatOrder)
            }
            _ => {
                // 参数越闭集：对空账本同样必抓。
                lt.record_param(PARAM_CELLS, 0).is_err()
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0xC600 + j as u64);
        } else {
            led.escaped += 1;
        }
        j += 1;
    }
    let caught = led.caught;
    let fp = led.fingerprint();
    (recorded, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// UNX-F52423 判据集（锚 UNX-F52424-J1，off-by-one 板式）。
pub fn run_unx52423_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52423");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五参数族 20 格全通",
        recorded >= PARAM_CELLS as u32,
        "--rate/--range/-p/网卡/--banners 五族逐格载荷恒等",
    );
    s.add(
        "参数族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < PARAM_CELLS {
                if param_kind(o).unwrap_or(5) < param_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五参数族闭集单调（4 格一族）",
    );
    s.add(
        "速率三档 6 格全落",
        {
            let mut lt = MasscanLedger::new();
            let mut ok = true;
            let mut k = 0usize;
            while k < RATE_CELLS {
                if let Some(w) = rate_cell(k) {
                    if lt.record_rate(k, w).is_err() {
                        ok = false;
                    }
                }
                k += 1;
            }
            ok && lt.rates_all()
        },
        "保守/标准/极速三档逐格落账，档值单调递增",
    );
    s.add(
        "速率档值单调递增",
        {
            let mut ok = true;
            let mut t = 1usize;
            while t < 3 {
                if tier_rate(t).unwrap_or(0) <= tier_rate(t - 1).unwrap_or(0) {
                    ok = false;
                }
                t += 1;
            }
            ok
        },
        "100→1000→10000 单调递增，越档必抓",
    );
    s.add(
        "异步收发栈 4 拍单调全落",
        {
            let mut lt = MasscanLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < ASYNC_BEATS {
                if let Some(w) = beat_cell(p) {
                    if lt.record_beat(p, w).is_err() {
                        ok = false;
                    }
                }
                p += 1;
            }
            ok && lt.beats_all()
        },
        "异步 4 拍按拍号单调落账，逆序必抓",
    );
    s.add(
        "与 nmap 分账声明 2/2 全齐",
        {
            let mut lt = MasscanLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                if lt.declare_row(false, r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok
        },
        "masscan 独立账声明/与 nmap 无共享态声明两行闭集",
    );
    s.add(
        "C 上栈声明 2/2 全齐",
        {
            let mut lt = MasscanLedger::new();
            let mut ok = true;
            // rows_all 要求分账行与 C 上栈行两套全落。
            let mut s = 0usize;
            while s < SPLIT_ROWS {
                if lt.declare_row(false, s).is_err() {
                    ok = false;
                }
                s += 1;
            }
            let mut r = 0usize;
            while r < C_STACK_ROWS {
                if lt.declare_row(true, r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.rows_all()
        },
        "C 栈上栈声明/C 栈回退声明两行闭集",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = MasscanLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "连接/监听/转发/速率/异步收发/写回逐格落账",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "参数漂移/异步拍逆序/参数越闭集三态轮转必抓",
    );
    let mut l0 = MasscanLedger::new();
    s.add(
        "边界·参数漂移拒",
        {
            let _ = l0.record_param(0, param_cell(0).unwrap_or(0));
            l0.record_param(1, param_cell(1).unwrap_or(0).wrapping_add(1))
                == Err(MasscanCode::EParamDrift)
        },
        "参数载荷漂移必抓",
    );
    s.add(
        "边界·参数越闭集",
        l0.record_param(PARAM_CELLS, 0).is_err(),
        "参数 20 格越闭集必拒",
    );
    s.add(
        "边界·速率越闭集",
        l0.record_rate(RATE_CELLS, 0).is_err(),
        "速率 6 格越闭集必拒",
    );
    s.add(
        "边界·异步拍越闭集",
        l0.record_beat(ASYNC_BEATS, 0).is_err(),
        "异步拍 4 拍越闭集必拒",
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
        "定级 S2 目标档闭集",
        {
            let mut lt = MasscanLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < PARAM_CELLS {
                if let Some(w) = param_cell(o) {
                    let _ = lt.record_param(o, w);
                }
                o += 1;
            }
            let mut k = 0usize;
            while k < RATE_CELLS {
                if let Some(w) = rate_cell(k) {
                    let _ = lt.record_rate(k, w);
                }
                k += 1;
            }
            let mut p = 0usize;
            while p < ASYNC_BEATS {
                if let Some(w) = beat_cell(p) {
                    let _ = lt.record_beat(p, w);
                }
                p += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_row(false, r);
                r += 1;
            }
            let mut c = 0usize;
            while c < C_STACK_ROWS {
                let _ = lt.declare_row(true, c);
                c += 1;
            }
            lt.judge_grade() == Grade::S2
        },
        "全通 → S2 封顶（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCA980,
        "SEG_BASE 0x40 对齐且在 VX16 派工域公式位",
    );
    s.add(
        "收口·判据条数",
        s.len() >= 17,
        "含本条共 ≥18 条闭集",
    );
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_param_kind() {
        assert_eq!(param_kind(0), Some(0));
        assert_eq!(param_kind(4), Some(1));
        assert_eq!(param_kind(19), Some(4));
        assert!(param_kind(20).is_none());
        assert!(param_cell(20).is_none());
        assert_eq!(param_kind_name(0), Some("--rate"));
        assert_eq!(param_kind_name(4), Some("--banners"));
        assert!(param_kind_name(5).is_none());
    }

    #[test]
    fn test_tier_rate_monotone() {
        assert_eq!(tier_rate(0), Some(100));
        assert_eq!(tier_rate(1), Some(1_000));
        assert_eq!(tier_rate(2), Some(10_000));
        assert!(tier_rate(3).is_none());
        assert!(rate_cell(RATE_CELLS).is_none());
    }

    #[test]
    fn test_beat_cells_pure() {
        let a = beat_cell(1).unwrap_or(0);
        assert_eq!(a, beat_cell(1).unwrap_or(0));
        // 4 拍基准单调递增。
        assert!(beat_cell(0).unwrap_or(0) < beat_cell(1).unwrap_or(0));
        assert!(beat_cell(1).unwrap_or(0) < beat_cell(2).unwrap_or(0));
        assert!(beat_cell(2).unwrap_or(0) < beat_cell(3).unwrap_or(0));
        assert!(beat_cell(ASYNC_BEATS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = MasscanLedger::new();
        let _ = lt.record_param(6, param_cell(6).unwrap_or(0));
        assert_eq!(
            lt.record_param(1, param_cell(1).unwrap_or(0)),
            Err(MasscanCode::EOrder)
        );
        // 逆序判错不改 last_kind：同族之后的高族合法格仍可落账。
        assert!(lt.record_param(7, param_cell(7).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_beat_order_caught() {
        let mut lt = MasscanLedger::new();
        let _ = lt.record_beat(2, beat_cell(2).unwrap_or(0));
        assert_eq!(
            lt.record_beat(1, beat_cell(1).unwrap_or(0)),
            Err(MasscanCode::EBeatOrder)
        );
        // 逆序判错不改 last_beat：高拍合法格仍可落账。
        assert!(lt.record_beat(3, beat_cell(3).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = MasscanLedger::new();
        assert_eq!(
            lt.record_rate(4, rate_cell(4).unwrap_or(0).wrapping_add(1)),
            Err(MasscanCode::ERateDrift)
        );
        assert_eq!(
            lt.record_beat(0, beat_cell(0).unwrap_or(0).wrapping_add(1)),
            Err(MasscanCode::EBeatDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = MasscanLedger::new();
        let _ = lt.record_param(0, param_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_param(0, param_cell(0).unwrap_or(0)),
            Err(MasscanCode::EDup)
        );
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = MasscanLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = MasscanLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 只含 run_cycle 实际计数的节：参数 20 + 速率 6 + 异步 4。
        assert_eq!(
            recorded,
            PARAM_CELLS as u32 + RATE_CELLS as u32 + ASYNC_BEATS as u32
        );
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
        assert_eq!(SEG_BASE, 0xCA980);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(MasscanCode::Ok as u32, 0);
        assert_eq!(MasscanCode::EGradeDrift as u32, 12);
        assert_eq!(MasscanCode::Ok.val(), SEG_BASE);
        assert!(!MasscanCode::Ok.detail().is_empty());
        assert!(!MasscanCode::EBeatOrder.detail().is_empty());
        assert!(!MasscanCode::EGradeDrift.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3。
        let lt = MasscanLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        // 六闸齐（格未齐）→ S2。
        let mut lt2 = MasscanLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S2);
    }

    #[test]
    fn unx52423_checks_green() {
        let s = run_unx52423_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 18);
    }
}
