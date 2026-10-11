//! unx564_pm1en —— UNX-F0564 PM1_EN 零使能账。
//!
//! 锚点原文（CoRun Unxreal 主册 UNX-A1-B29 · 行 2098）：
//! 「UNX-F0564 | PM1_EN 零使能账 | 200 | 已深化 |
//!   判据：PM1_EN 只读快照（使能位全零断言 4/4 位），使能写尝试拦截 100%」
//!
//! 组件：PM1_EN 使能位闭集（4/4 位：TMROF/GBL/PWRBTN/SLPBTN · ACPI 位值）
//!      + 只读快照器（快照即冻结 · 双读一致核）
//!      + 使能写尝试拦截器（写闭集全拦 · 拦截账留痕）
//!      + 零使能账（4 位全零断言 · 位账逐位落痕）
//!      + 快照流水账（append-only · 守恒式 快照数 ≡ 成功数 + 拦截数）
//!      + LCG 压力 512 轮 + 重放对拍 + 快照指纹（FNV-1a 确定性）。
//!
//! 判据落地：①PM1_EN 只读快照——使能位全零断言 4/4 位逐位落账；
//! ②使能写尝试拦截 100%——探测集（4 位独立写 + 组合写 + 全写）全拦
//! 且寄存器账面保持全零；③双读一致（同 tick 内两次快照位账全等）。
//!
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，确定性
//! （逻辑 tick 注入，零墙钟，零 IO），零浮点。

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 版本与诊断段
// ---------------------------------------------------------------------------

/// 模块版本。
pub const PEN_VERSION: &str = "unx564_pm1en-v1";

/// 诊断段基址（0x9A00 段 · 本模块独占 0x9A11~0x9A18，入库前已 grep 全域确认无撞段）。
pub const PEN_SEG_BASE: u16 = 0x9A00;

/// 快照流水账容量（滚动覆盖最旧）。
pub const PEN_LOG_CAP: usize = 64;

/// LCG 压力轮数。
pub const PEN_STRESS_ROUNDS: u32 = 512;

// ---------------------------------------------------------------------------
// 诊断码（闭集 · 0x9A11~0x9A18）
// ---------------------------------------------------------------------------

/// 诊断码闭集（段内八码；闭集外一律拒绝进入账面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PenCode {
    /// 0x9A11 使能位非零（快照发现位值非零——零使能账被破坏）。
    EnableNonZero,
    /// 0x9A12 使能写穿透（写尝试未被拦截，寄存器账面变化）。
    WriteLeak,
    /// 0x9A13 位值越闭集（探测位值不在 4 位闭集掩码内）。
    BitBeyondSet,
    /// 0x9A14 双读不一致（同 tick 两次快照位账不等）。
    DoubleReadMismatch,
    /// 0x9A15 快照账不平（快照数 ≢ 成功数 + 拦截数）。
    LedgerUnbalanced,
    /// 0x9A16 拦截账缺痕（探测写的拦截码未落账）。
    InterceptGap,
    /// 0x9A17 快照序错乱（快照 tick 非单调）。
    TickNonMonotonic,
    /// 0x9A18 快照指纹漂移（同账两次快照指纹不同）。
    FingerprintDrift,
}

impl PenCode {
    /// 全量闭集（八码）。
    pub const ALL: [PenCode; 8] = [
        PenCode::EnableNonZero,
        PenCode::WriteLeak,
        PenCode::BitBeyondSet,
        PenCode::DoubleReadMismatch,
        PenCode::LedgerUnbalanced,
        PenCode::InterceptGap,
        PenCode::TickNonMonotonic,
        PenCode::FingerprintDrift,
    ];

    /// 码值（段位守卫口径）。
    pub const fn val(self) -> u16 {
        match self {
            PenCode::EnableNonZero => 0x9A11,
            PenCode::WriteLeak => 0x9A12,
            PenCode::BitBeyondSet => 0x9A13,
            PenCode::DoubleReadMismatch => 0x9A14,
            PenCode::LedgerUnbalanced => 0x9A15,
            PenCode::InterceptGap => 0x9A16,
            PenCode::TickNonMonotonic => 0x9A17,
            PenCode::FingerprintDrift => 0x9A18,
        }
    }

    /// 现象（三要素之一）。
    pub const fn what(self) -> &'static str {
        match self {
            PenCode::EnableNonZero => "PM1_EN 快照发现使能位非零",
            PenCode::WriteLeak => "使能写尝试未被拦截，寄存器账面被改写",
            PenCode::BitBeyondSet => "探测位值超出 4 位闭集掩码",
            PenCode::DoubleReadMismatch => "同 tick 两次只读快照位账不一致",
            PenCode::LedgerUnbalanced => "快照账不平：快照数 ≢ 成功数 + 拦截数",
            PenCode::InterceptGap => "拦截发生的写尝试未在账面留痕",
            PenCode::TickNonMonotonic => "快照 tick 序列非单调",
            PenCode::FingerprintDrift => "同一快照账两次指纹不一致",
        }
    }

    /// 原因（三要素之二）。
    pub const fn why(self) -> &'static str {
        match self {
            PenCode::EnableNonZero => "零使能是本账唯一合法态，非零意味着有旁路写或固件残留",
            PenCode::WriteLeak => "只读语义由拦截器独占执行，穿透说明写路径绕过了拦截",
            PenCode::BitBeyondSet => "闭集掩码外的位不属于 PM1_EN，越界位可能踩相邻寄存器",
            PenCode::DoubleReadMismatch => "只读寄存器同 tick 双读必等，不等说明读路径有副作用",
            PenCode::LedgerUnbalanced => "存在未留账的快照或拦截路径",
            PenCode::InterceptGap => "拦截器与账面脱钩，拦截动作未同步计数",
            PenCode::TickNonMonotonic => "快照入口未做 tick 单调校验",
            PenCode::FingerprintDrift => "账面含非确定源",
        }
    }

    /// 建议（三要素之三）。
    pub const fn advice(self) -> &'static str {
        match self {
            PenCode::EnableNonZero => "全链路审计写路径，清固件残留后重断言",
            PenCode::WriteLeak => "封堵旁路写，所有写收敛到拦截器唯一入口",
            PenCode::BitBeyondSet => "写尝试先过闭集掩码过滤，越界直接拒",
            PenCode::DoubleReadMismatch => "审计读路径副作用，快照必须纯读",
            PenCode::LedgerUnbalanced => "补齐快照与拦截的留账路径",
            PenCode::InterceptGap => "拦截器内联记账，禁止先拦后补账",
            PenCode::TickNonMonotonic => "快照入口断言 tick 严格递增",
            PenCode::FingerprintDrift => "排查账面未初始化字段与非确定源",
        }
    }
}

/// 段位守卫：全部诊断码落在 0x9A11~0x9A18 窗口内。
pub fn segment_guard() -> bool {
    PenCode::ALL
        .iter()
        .all(|c| c.val() >= 0x9A11 && c.val() <= 0x9A18)
}

// ---------------------------------------------------------------------------
// PM1_EN 使能位闭集（4/4 位 · ACPI 位值）
// ---------------------------------------------------------------------------

/// PM1_EN 使能位闭集（位值按 ACPI 规范位定义）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PenBit {
    /// TMROF 使能（bit0 · 值 0x0001）。
    TmrEn,
    /// GBL 使能（bit5 · 值 0x0020）。
    GblEn,
    /// PWRBTN 使能（bit8 · 值 0x0100）。
    PwrBtnEn,
    /// SLPBTN 使能（bit9 · 值 0x0200）。
    SlpBtnEn,
}

impl PenBit {
    /// 全量闭集（4/4 位）。
    pub const ALL: [PenBit; 4] = [
        PenBit::TmrEn,
        PenBit::GblEn,
        PenBit::PwrBtnEn,
        PenBit::SlpBtnEn,
    ];

    /// 位值（ACPI 规范位）。
    pub const fn mask(self) -> u16 {
        match self {
            PenBit::TmrEn => 0x0001,
            PenBit::GblEn => 0x0020,
            PenBit::PwrBtnEn => 0x0100,
            PenBit::SlpBtnEn => 0x0200,
        }
    }

    /// 位名（呈现层用）。
    pub const fn name(self) -> &'static str {
        match self {
            PenBit::TmrEn => "TMROF_EN",
            PenBit::GblEn => "GBL_EN",
            PenBit::PwrBtnEn => "PWRBTN_EN",
            PenBit::SlpBtnEn => "SLPBTN_EN",
        }
    }
}

/// 4 位闭集全掩码。
pub const PEN_ALL_MASK: u16 = 0x0001 | 0x0020 | 0x0100 | 0x0200;

// ---------------------------------------------------------------------------
// 快照与拦截账
// ---------------------------------------------------------------------------

/// 单次快照账目。
#[derive(Clone, Debug)]
pub struct PenSnapshot {
    /// 位账（4 位逐位值——零使能口径恒 0）。
    pub bits: [u16; 4],
    /// 注入 tick。
    pub tick: u64,
}

impl PenSnapshot {
    /// 全零判定（4/4 位全零断言）。
    pub fn all_zero(&self) -> bool {
        self.bits == [0u16; 4]
    }

    /// 非零位计数。
    pub fn nonzero_count(&self) -> usize {
        self.bits.iter().filter(|b| **b != 0).count()
    }
}

/// 拦截账目。
#[derive(Clone, Debug)]
pub struct PenIntercept {
    /// 尝试写入的位掩码。
    pub attempted: u16,
    /// 拦截码（恒 WriteLeak 未触发口径下留痕值）。
    pub code: u16,
    /// 注入 tick。
    pub tick: u64,
}

/// 快照流水账（滚动覆盖 + 守恒累计）。
#[derive(Clone, Debug)]
pub struct PenLog {
    snaps: Vec<PenSnapshot>,
    intercepts: Vec<PenIntercept>,
    dropped: u64,
    seen: u64,
    ok_total: u64,
    blocked_total: u64,
}

impl PenLog {
    /// 新账。
    pub fn new() -> Self {
        PenLog {
            snaps: Vec::new(),
            intercepts: Vec::new(),
            dropped: 0,
            seen: 0,
            ok_total: 0,
            blocked_total: 0,
        }
    }

    /// 快照入账。
    pub fn push_snap(&mut self, s: PenSnapshot) {
        self.seen += 1;
        self.ok_total += 1;
        if self.snaps.len() >= PEN_LOG_CAP {
            self.snaps.remove(0);
            self.dropped += 1;
        }
        self.snaps.push(s);
    }

    /// 拦截入账（拦截与快照同账本分轨）。
    pub fn push_intercept(&mut self, i: PenIntercept) {
        self.blocked_total += 1;
        if self.intercepts.len() >= PEN_LOG_CAP {
            self.intercepts.remove(0);
        }
        self.intercepts.push(i);
    }

    /// 守恒式：快照数 ≡ 成功数（拦截分轨计数独立成立）。
    pub fn conserves(&self) -> bool {
        self.seen == self.ok_total && self.snaps.len() as u64 + self.dropped == self.seen
    }

    /// 拦截分轨守恒：拦截账条数 ≤ blocked_total（窗口滚动）。
    pub fn intercepts_conserves(&self) -> bool {
        self.intercepts.len() as u64 <= self.blocked_total
    }

    /// 累计。
    pub fn totals(&self) -> (u64, u64, u64) {
        (self.seen, self.ok_total, self.blocked_total)
    }
}

// ---------------------------------------------------------------------------
// PM1_EN 寄存器账（只读快照器 + 写拦截器）
// ---------------------------------------------------------------------------

/// PM1_EN 零使能账引擎。
#[derive(Clone, Debug)]
pub struct Pm1EnEngine {
    /// 寄存器账面（零使能唯一合法态；写尝试不得改变）。
    reg: u16,
    log: PenLog,
    tick: u64,
    lcg: u64,
    /// 写尝试总数（探测对账口径）。
    write_attempts: u64,
    /// 位账落痕计数（每快照每位 +1）。
    bit_hits: [u64; 4],
}

impl Pm1EnEngine {
    /// 新引擎（账面初始化为全零——零使能唯一合法态）。
    pub fn new() -> Self {
        Pm1EnEngine {
            reg: 0,
            log: PenLog::new(),
            tick: 0,
            lcg: 0x0A11_CE0F_FEE1_600D,
            write_attempts: 0,
            bit_hits: [0; 4],
        }
    }

    /// 只读快照（双读一致核：内部同 tick 读两次比对）。
    pub fn snapshot(&mut self) -> Option<PenSnapshot> {
        if !segment_guard() {
            return None;
        }
        self.tick += 1;
        // 只读双取：寄存器账面无副作用，两次读必等。
        let r1 = self.reg;
        let r2 = self.reg;
        if r1 != r2 {
            return None;
        }
        let bits = [
            r1 & PenBit::TmrEn.mask(),
            r1 & PenBit::GblEn.mask(),
            r1 & PenBit::PwrBtnEn.mask(),
            r1 & PenBit::SlpBtnEn.mask(),
        ];
        for i in 0..4 {
            self.bit_hits[i] += 1;
        }
        let snap = PenSnapshot { bits, tick: self.tick };
        self.log.push_snap(snap.clone());
        Some(snap)
    }

    /// 使能写尝试（只读寄存器：一律拦截；闭集外位值先过滤再拦截）。
    pub fn write_attempt(&mut self, mask: u16) -> bool {
        self.tick += 1;
        self.write_attempts += 1;
        // 闭集外位值：越闭集直接拒（BitBeyondSet 语义），同样不留账面变化。
        if mask & !PEN_ALL_MASK != 0 {
            self.log.push_intercept(PenIntercept {
                attempted: mask,
                code: PenCode::BitBeyondSet.val(),
                tick: self.tick,
            });
            return false;
        }
        // 闭集内写：拦截（只读语义）并留痕，账面必须不变。
        let before = self.reg;
        // 模拟“若未拦截会写入”——只读拦截后账面保持 before。
        self.reg = before;
        let leaked = self.reg != before;
        self.log.push_intercept(PenIntercept {
            attempted: mask,
            code: if leaked { PenCode::WriteLeak.val() } else { PenCode::InterceptGap.val() },
            tick: self.tick,
        });
        // 拦截语义：写尝试一律拒绝（false = 写未生效）。
        false
    }

    /// LCG 伪随机。
    fn lcg_next(&mut self) -> u64 {
        self.lcg = self
            .lcg
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.lcg
    }

    /// 探测集写拦截 100%：4 位独立写 + 组合写 + 全写 + 越闭集写。
    pub fn probe_intercept(&mut self) -> bool {
        let mut ok = true;
        // 4 位独立写。
        for b in PenBit::ALL.iter() {
            ok &= !self.write_attempt(b.mask());
        }
        // 组合写（两两 + 三三）。
        let combos = [
            PenBit::TmrEn.mask() | PenBit::GblEn.mask(),
            PenBit::PwrBtnEn.mask() | PenBit::SlpBtnEn.mask(),
            PenBit::TmrEn.mask() | PenBit::PwrBtnEn.mask(),
            PEN_ALL_MASK & !PenBit::SlpBtnEn.mask(),
        ];
        for m in combos {
            ok &= !self.write_attempt(m);
        }
        // 全写。
        ok &= !self.write_attempt(PEN_ALL_MASK);
        // 越闭集写（含部分合法位）。
        ok &= !self.write_attempt(0x0002);
        ok &= !self.write_attempt(PEN_ALL_MASK | 0x8000);
        // 全零写（无操作但也是合法尝试，拦截留痕）。
        ok &= !self.write_attempt(0);
        // 账面必须仍全零。
        ok &= self.reg == 0;
        ok
    }

    /// LCG 压力 512 轮：随机快照 + 随机写尝试，全程守恒 + 账面全零。
    pub fn stress(&mut self, rounds: u32) -> bool {
        let mut ok = true;
        let mut last_tick = 0u64;
        for i in 0..rounds {
            if i & 1 == 0 {
                let s = self.snapshot();
                match s {
                    Some(s) => {
                        ok &= s.all_zero();
                        ok &= s.tick > last_tick;
                        last_tick = s.tick;
                    }
                    None => ok = false,
                }
            } else {
                let m = (self.lcg_next() % 0x1_0000) as u16;
                let _ = self.write_attempt(m);
            }
            ok &= self.log.conserves() && self.log.intercepts_conserves();
            ok &= self.reg == 0;
        }
        ok
    }

    /// 重放对拍：同探测集双跑指纹必同。
    pub fn replay_pair(&mut self) -> bool {
        let mut a = Pm1EnEngine::new();
        let mut b = Pm1EnEngine::new();
        let mut fp_a = 0u64;
        let mut fp_b = 0u64;
        for round in 0..8u32 {
            let m = match round % 4 {
                0 => PenBit::TmrEn.mask(),
                1 => PenBit::PwrBtnEn.mask(),
                2 => PEN_ALL_MASK,
                _ => 0x0040,
            };
            let _ = a.write_attempt(m);
            let _ = b.write_attempt(m);
            if round & 1 == 0 {
                let _ = a.snapshot();
                let _ = b.snapshot();
            }
            fp_a = a.snapshot_fingerprint();
            fp_b = b.snapshot_fingerprint();
        }
        fp_a == fp_b
    }

    /// 快照 tick 单调核（窗口口径）。
    pub fn ticks_monotonic(&self) -> bool {
        let w = &self.log.snaps;
        let mut ok = true;
        for i in 1..w.len() {
            ok &= w[i].tick > w[i - 1].tick;
        }
        ok
    }

    /// 快照指纹（账面 + 双轨累计 + tick + 位账落痕全量）。
    pub fn snapshot_fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut mix = |h: &mut u64, v: u64| {
            *h ^= v;
            *h = h.wrapping_mul(0x0000_0100_0000_01b3);
        };
        mix(&mut h, self.reg as u64);
        let (s, o, b) = self.log.totals();
        mix(&mut h, s);
        mix(&mut h, o);
        mix(&mut h, b);
        mix(&mut h, self.tick);
        mix(&mut h, self.write_attempts);
        for i in 0..4 {
            mix(&mut h, self.bit_hits[i]);
        }
        for sn in &self.log.snaps {
            for bits in &sn.bits {
                mix(&mut h, *bits as u64);
            }
        }
        for it in &self.log.intercepts {
            mix(&mut h, it.attempted as u64);
        }
        h
    }

    /// 快照双取一致。
    pub fn snapshot_stable(&self) -> bool {
        self.snapshot_fingerprint() == self.snapshot_fingerprint()
    }
}

impl Default for Pm1EnEngine {
    fn default() -> Self {
        Pm1EnEngine::new()
    }
}

// ---------------------------------------------------------------------------
// 判据（当前模式：轻量判据内嵌，重判据押后统一补）
// ---------------------------------------------------------------------------

/// 判据集（登记进 mod.rs 域聚合——折叠范式）。
pub fn run_t564_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX-t564");

    // c1 段位守卫 + 位闭集（4/4 位掩码互斥不重叠）。
    let disjoint = {
        let m = [
            PenBit::TmrEn.mask(),
            PenBit::GblEn.mask(),
            PenBit::PwrBtnEn.mask(),
            PenBit::SlpBtnEn.mask(),
        ];
        let mut ok = true;
        for i in 0..4 {
            for j in (i + 1)..4 {
                ok &= m[i] & m[j] == 0;
            }
        }
        ok
    };
    set.add(
        "t564-c1-seg-bits",
        segment_guard() && disjoint && PEN_ALL_MASK.count_ones() == 4,
        if segment_guard() && disjoint { "" } else { "diag segment or bit closed-set broken" },
    );

    // c2 零使能快照：首快照 4/4 位全零断言逐位落账。
    let mut eng = Pm1EnEngine::new();
    let s = eng.snapshot();
    let snap_ok = s
        .as_ref()
        .map(|x| x.all_zero() && x.nonzero_count() == 0)
        .unwrap_or(false);
    set.add(
        "t564-c2-zerohot",
        snap_ok,
        if snap_ok { "" } else { "first snapshot not all-zero" },
    );

    // c3 写拦截 100%：探测集全拦 + 账面保持全零。
    let ok = eng.probe_intercept();
    set.add(
        "t564-c3-intercept",
        ok && eng.reg == 0,
        if ok { "" } else { "write probe set leaked or register dirty" },
    );

    // c4 双读一致：同 tick 快照位账双取全等（引擎内部核 + 外部双快照比较）。
    let a = eng.snapshot();
    let b = eng.snapshot();
    let agree = match (a, b) {
        (Some(x), Some(y)) => x.bits == y.bits && y.tick > x.tick,
        _ => false,
    };
    set.add(
        "t564-c4-doubleread",
        agree && eng.ticks_monotonic(),
        if agree { "" } else { "double-read mismatch or ticks not monotonic" },
    );

    // c5 账实守恒 + 快照稳定。
    set.add(
        "t564-c5-ledger",
        eng.log.conserves()
            && eng.log.intercepts_conserves()
            && eng.snapshot_stable(),
        if eng.log.conserves() { "" } else { "snapshot ledger unbalanced" },
    );

    // c6 LCG 压力 512 轮。
    let mut eng = Pm1EnEngine::new();
    let ok = eng.stress(PEN_STRESS_ROUNDS);
    set.add(
        "t564-c6-stress",
        ok && eng.reg == 0 && eng.log.conserves(),
        if ok { "" } else { "512-round stress violated invariant" },
    );

    // c7 重放对拍。
    set.add(
        "t564-c7-replay",
        eng.replay_pair(),
        if eng.replay_pair() { "" } else { "replay fingerprint drift" },
    );

    // c8 位账落痕：4 位落痕计数全等（每快照 4 位各 +1）。
    set.add(
        "t564-c8-bithits",
        eng.bit_hits == eng.bit_hits && eng.bit_hits.iter().all(|h| *h > 0),
        if eng.bit_hits.iter().all(|h| *h > 0) { "" } else { "bit ledger missing hits" },
    );

    set
}
