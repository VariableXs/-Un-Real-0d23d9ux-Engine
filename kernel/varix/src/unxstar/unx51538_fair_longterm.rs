//! UNX-F51538 B17 公平性长期账（×3 波饥饿零发生证明） · 0xC0980段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51539-J1 收口防重 grep 留痕
//!
//! 设计要点：
//!   ① 令牌桶模型：每设备权重 w，每轮补充 w×UNIT，桶容 w×BUCKET_UNIT；
//!      授予 grant = min(需求, 桶内令牌)，桶满溢出记为浪费（容量边界）。
//!   ② 长期账逐设备守恒：补充 = 授予 + 桶存 + 浪费（每笔都落到三者之一，
//!      总账分毫不差）；授予账只增不减。
//!   ③ 公平性判据（纯整数）：任意两设备 |g_i×w_j − g_j×w_i| ≤ SLACK，
//!      均匀重载下 granted 恰与权重成比例，长期账偏差必须为 0。
//!   ④ 零饿死：需求 > 0 时每轮每设备授予必 > 0（补充恒 > 0），
//!      ×3 波饿死事件与最大零授予连击都必须为 0。
//!   ⑤ 状态机 Setup → Running → Sealed 只进不退：封版后改权重显性拒
//!      （权重冻结）；波次只进不退，回退显性拒。
//!   ⑥ 注入三类必抓 ×10：令牌超发 / 权重篡改 / 账面回退；
//!      注入账守恒 注入 = 检出 + 逃逸，逃逸恒 0。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址：0xC0880 + 0x40 × (51538 − 51534)。
pub const SEG_BASE: u32 = 0xC0980;

/// 设备数。
pub const DEVICES: usize = 6;
/// 压力波数（×3 波）。
pub const WAVES: usize = 3;
/// 每波轮数。
pub const ROUNDS_PER_WAVE: usize = 8;
/// 单权重补充单元。
pub const UNIT: u32 = 8;
/// 桶容单元（桶容 = w × BUCKET_UNIT）。
pub const BUCKET_UNIT: u32 = 32;
/// 重载单轮需求（对全部设备一致，> 最大补充量）。
pub const DEMAND_HEAVY: u32 = 64;
/// 单轮需求上限（含）。
pub const DEMAND_MAX: u32 = 64;
/// 权重上限（含）。
pub const WEIGHT_MAX: u32 = 4;
/// 公平性整数判据容差。
pub const SLACK: i64 = 2048;

/// 冻结权重表（Setup 相位内可改，封版后冻结）。
pub const WEIGHTS: [u32; DEVICES] = [1, 1, 2, 2, 3, 4];

/// 诊断码闭集（公平性长期账域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FairCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 权重越界（零权重或超上限）
    WeightOutOfRange,
    /// 权重冻结（封版后改权重）
    WeightFrozen,
    /// 令牌超发
    TokenOverIssue,
    /// 账面回退
    LedgerRegress,
    /// 需求越界（负值或超上限）
    DemandOutOfRange,
    /// 波次回退或越界
    WaveRegress,
    /// 注入账守恒失败
    LedgerMismatch,
}

impl FairCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            FairCode::Ok => "成功",
            FairCode::SegBad => "段位越界：核对 SEG_BASE 对齐与取值范围",
            FairCode::WeightOutOfRange => "权重越界：权重必须落在 1..=4",
            FairCode::WeightFrozen => "权重冻结：封版后不得改权重",
            FairCode::TokenOverIssue => "令牌超发：授予不得超过桶内令牌",
            FairCode::LedgerRegress => "账面回退：长期账只增不减",
            FairCode::DemandOutOfRange => "需求越界：单轮需求必须落在 0..=64",
            FairCode::WaveRegress => "波次回退：波次状态机只进不退",
            FairCode::LedgerMismatch => "注入账不平：注入=检出+逃逸必须守恒",
        }
    }
}

/// 单设备长期账。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DevLedger {
    pub weight: u32,
    pub tokens: u64,
    pub granted: u64,
    pub refill_total: u64,
    pub wasted: u64,
    pub zero_grant_rows: u64,
    pub starve_events: u64,
    pub zero_streak: u64,
    pub max_zero_streak: u64,
}

/// 单轮报告。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoundReport {
    pub granted: u64,
    pub zero_grant_rows: u64,
}

/// 单波报告。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WaveReport {
    pub wave_idx: usize,
    pub granted: u64,
    pub starve_events: u64,
}

/// 令牌桶公平账引擎。
pub struct FairLedger {
    devs: Vec<DevLedger>,
    phase: FairPhase,
    wave: usize,
    granted_total: u64,
    injected: u64,
    detected: u64,
    escaped: u64,
}

/// 相位（单调状态机）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FairPhase {
    /// 配置期：可改权重
    Setup = 0,
    /// 运行期
    Running = 1,
    /// 封版
    Sealed = 2,
}

impl FairLedger {
    /// 新账：按冻结权重表建桶，桶空起步。
    pub fn new() -> FairLedger {
        let mut devs: Vec<DevLedger> = Vec::new();
        for i in 0..DEVICES {
            let w = match WEIGHTS.get(i) {
                Some(w) => *w,
                None => 1,
            };
            devs.push(DevLedger {
                weight: w,
                tokens: 0,
                granted: 0,
                refill_total: 0,
                wasted: 0,
                zero_grant_rows: 0,
                starve_events: 0,
                zero_streak: 0,
                max_zero_streak: 0,
            });
        }
        FairLedger {
            devs,
            phase: FairPhase::Setup,
            wave: 0,
            granted_total: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 相位单调推进（只进不退，逐级 +1）。
    pub fn advance_phase(&mut self, target: FairPhase) -> Result<FairPhase, FairCode> {
        let cur = self.phase as u32;
        let tgt = target as u32;
        if tgt <= cur || tgt > cur + 1 {
            return Err(FairCode::WaveRegress);
        }
        self.phase = target;
        Ok(self.phase)
    }

    pub fn phase(&self) -> FairPhase {
        self.phase
    }

    /// 改权重（仅 Setup 相位；判序：相位先于权重界）。
    pub fn set_weight(&mut self, dev: usize, weight: u32) -> Result<(), FairCode> {
        if self.phase != FairPhase::Setup {
            return Err(FairCode::WeightFrozen);
        }
        if weight == 0 || weight > WEIGHT_MAX {
            return Err(FairCode::WeightOutOfRange);
        }
        match self.devs.get_mut(dev) {
            Some(d) => {
                d.weight = weight;
                Ok(())
            }
            None => Err(FairCode::WeightOutOfRange),
        }
    }

    /// 令牌桶一轮：统一需求 → 补充 → 截断授予 → 长期账入账。
    pub fn bucket_round(&mut self, demand: i64) -> Result<RoundReport, FairCode> {
        if demand < 0 {
            return Err(FairCode::DemandOutOfRange);
        }
        if demand as u64 > DEMAND_MAX as u64 {
            return Err(FairCode::DemandOutOfRange);
        }
        if self.phase == FairPhase::Setup {
            let _ = self.advance_phase(FairPhase::Running)?;
        }
        let demand = demand as u64;
        let mut granted_sum = 0u64;
        let mut zero_rows = 0u64;
        for i in 0..DEVICES {
            let (weight, cap) = match self.devs.get(i) {
                Some(d) => (d.weight as u64, d.weight as u64 * BUCKET_UNIT as u64),
                None => return Err(FairCode::WeightOutOfRange),
            };
            let refill = weight * UNIT as u64;
            let tokens = match self.devs.get(i) {
                Some(d) => d.tokens,
                None => 0,
            };
            // 补充 + 桶容截断：溢出记浪费。
            let filled = tokens + refill;
            let (new_tokens, wasted) = if filled > cap {
                (cap, filled - cap)
            } else {
                (filled, 0)
            };
            // 授予 = min(需求, 桶内令牌)。
            let grant = if demand < new_tokens { demand } else { new_tokens };
            let after = new_tokens - grant;
            let d = match self.devs.get_mut(i) {
                Some(d) => d,
                None => return Err(FairCode::WeightOutOfRange),
            };
            d.tokens = after;
            d.granted += grant;
            d.refill_total += refill;
            d.wasted += wasted;
            if grant == 0 {
                d.zero_grant_rows += 1;
                d.zero_streak += 1;
                if demand > 0 {
                    d.starve_events += 1;
                }
                if d.zero_streak > d.max_zero_streak {
                    d.max_zero_streak = d.zero_streak;
                }
                zero_rows += 1;
            } else {
                d.zero_streak = 0;
            }
            granted_sum += grant;
            self.granted_total += grant;
        }
        Ok(RoundReport {
            granted: granted_sum,
            zero_grant_rows: zero_rows,
        })
    }

    /// 一波压力：ROUNDS_PER_WAVE 轮统一需求；波次只进不退。
    pub fn run_wave(&mut self, demand: i64) -> Result<WaveReport, FairCode> {
        if self.wave >= WAVES {
            return Err(FairCode::WaveRegress);
        }
        let mut granted_wave = 0u64;
        for _ in 0..ROUNDS_PER_WAVE {
            let rep = self.bucket_round(demand)?;
            granted_wave += rep.granted;
        }
        // 波末饿死对账：需求 > 0 时本波不允许有饿死事件。
        let mut starve = 0u64;
        for i in 0..DEVICES {
            let s = match self.devs.get(i) {
                Some(d) => d.starve_events,
                None => 0,
            };
            starve += s;
        }
        let report = WaveReport {
            wave_idx: self.wave,
            granted: granted_wave,
            starve_events: starve,
        };
        self.wave += 1;
        Ok(report)
    }

    /// 波次回退注入面。
    pub fn try_set_wave(&mut self, target: usize) -> Result<usize, FairCode> {
        if target >= self.wave || target >= WAVES {
            return Err(FairCode::WaveRegress);
        }
        Err(FairCode::WaveRegress)
    }

    /// 注入 ①：伪造授予（绕过桶，超桶内令牌必抓）。
    pub fn forge_grant(&mut self, dev: usize, amount: u64) -> Result<(), FairCode> {
        if dev >= DEVICES {
            return Err(FairCode::LedgerRegress);
        }
        let tokens = match self.devs.get(dev) {
            Some(d) => d.tokens,
            None => return Err(FairCode::LedgerRegress),
        };
        if amount > tokens {
            return Err(FairCode::TokenOverIssue);
        }
        match self.devs.get_mut(dev) {
            Some(d) => {
                d.tokens -= amount;
                d.granted += amount;
            }
            None => return Err(FairCode::LedgerRegress),
        }
        self.granted_total += amount;
        Ok(())
    }

    /// 注入 ③：账面回退。
    pub fn try_rewind_granted(&mut self, target: u64) -> Result<(), FairCode> {
        if target < self.granted_total {
            return Err(FairCode::LedgerRegress);
        }
        Ok(())
    }

    /// 长期账守恒：逐设备 补充 = 授予 + 桶存 + 浪费。
    pub fn ledger_conserved(&self) -> bool {
        for i in 0..DEVICES {
            let d = match self.devs.get(i) {
                Some(d) => *d,
                None => return false,
            };
            if d.refill_total != d.granted + d.tokens + d.wasted {
                return false;
            }
        }
        true
    }

    /// 公平性长期账（纯整数）：|g_i×w_j − g_j×w_i| ≤ SLACK。
    /// 返回最大偏差（均匀重载下应为 0）。
    pub fn fairness_max_dev(&self) -> i64 {
        let mut max_dev: i64 = 0;
        for i in 0..DEVICES {
            for j in 0..DEVICES {
                let (gi, wi) = match self.devs.get(i) {
                    Some(d) => (d.granted as i64, d.weight as i64),
                    None => (0, 1),
                };
                let (gj, wj) = match self.devs.get(j) {
                    Some(d) => (d.granted as i64, d.weight as i64),
                    None => (0, 1),
                };
                let a = match gi.checked_mul(wj) {
                    Some(v) => v,
                    None => i64::MAX,
                };
                let b = match gj.checked_mul(wi) {
                    Some(v) => v,
                    None => i64::MAX,
                };
                let dev = if a > b { a - b } else { b - a };
                if dev > max_dev {
                    max_dev = dev;
                }
            }
        }
        max_dev
    }

    pub fn fair_within_slack(&self) -> bool {
        self.fairness_max_dev() <= SLACK
    }

    /// 全体饿死事件总数与最大零授予连击。
    pub fn starvation_summary(&self) -> (u64, u64) {
        let mut events = 0u64;
        let mut streak = 0u64;
        for i in 0..DEVICES {
            let d = match self.devs.get(i) {
                Some(d) => *d,
                None => continue,
            };
            events += d.starve_events;
            if d.max_zero_streak > streak {
                streak = d.max_zero_streak;
            }
        }
        (events, streak)
    }

    pub fn granted_total(&self) -> u64 {
        self.granted_total
    }

    pub fn wave(&self) -> usize {
        self.wave
    }

    pub fn dev_ledger(&self, dev: usize) -> Option<DevLedger> {
        self.devs.get(dev).copied()
    }

    pub fn note_injected(&mut self) {
        self.injected += 1;
    }

    pub fn note_detected(&mut self) {
        self.detected += 1;
    }

    pub fn note_escaped(&mut self) {
        self.escaped += 1;
    }

    pub fn injected(&self) -> u64 {
        self.injected
    }

    pub fn detected(&self) -> u64 {
        self.detected
    }

    pub fn escaped(&self) -> u64 {
        self.escaped
    }

    /// 注入账守恒：注入 = 检出 + 逃逸，逃逸恒 0、检出非零。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected + self.escaped && self.escaped == 0 && self.detected > 0
    }
}

/// 便捷构造：×3 波统一重载需求压满。
pub fn run_three_heavy_waves() -> FairLedger {
    let mut fl = FairLedger::new();
    let _ = fl.advance_phase(FairPhase::Running);
    for _ in 0..WAVES {
        let _ = fl.run_wave(DEMAND_HEAVY as i64);
    }
    fl
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 双跑一致性：同一波序两次建账，长期账逐字段一致。
pub fn dual_run_consistent() -> bool {
    let build = || -> (u64, Vec<DevLedger>, i64) {
        let fl = run_three_heavy_waves();
        let mut devs: Vec<DevLedger> = Vec::new();
        for i in 0..DEVICES {
            match fl.dev_ledger(i) {
                Some(d) => devs.push(d),
                None => (),
            }
        }
        (fl.granted_total(), devs, fl.fairness_max_dev())
    };
    let a = build();
    let b = build();
    a == b
}

/// 收口自检（≥12 条判据形态）。
pub fn run_unx51538_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51538");
    // 1 正常流：×3 波重载压满，授予与权重严格成比例。
    {
        let fl = run_three_heavy_waves();
        let rounds = (WAVES * ROUNDS_PER_WAVE) as u64;
        let mut exact = true;
        for i in 0..DEVICES {
            let d = match fl.dev_ledger(i) {
                Some(d) => d,
                None => {
                    exact = false;
                    continue;
                }
            };
            if d.granted != d.weight as u64 * UNIT as u64 * rounds {
                exact = false;
            }
        }
        s.add_dyn(
            "×3 波重载压满",
            fl.wave() == WAVES && exact && fl.granted_total() > 0,
            "重载下授予恰好等于权重×单元×轮数",
        );
    }
    // 2 边界-零：零需求轮全设备零授予但逐笔落账，账面守恒。
    {
        let mut fl = FairLedger::new();
        let rep = fl.bucket_round(0);
        let zero_ok = match rep {
            Ok(r) => r.granted == 0 && r.zero_grant_rows == DEVICES as u64,
            Err(_) => false,
        };
        s.add_dyn(
            "边界零授予落账",
            zero_ok && fl.ledger_conserved(),
            "零授予笔逐设备落账且长期账守恒",
        );
    }
    // 3 边界-负：负需求显性拒。
    {
        let mut fl = FairLedger::new();
        let r = fl.bucket_round(-1);
        s.add_dyn("边界负值拒", r == Err(FairCode::DemandOutOfRange), "负需求必须显性拒");
    }
    // 4 边界-上限：需求取 DEMAND_MAX 放行。
    {
        let mut fl = FairLedger::new();
        let r = fl.bucket_round(DEMAND_MAX as i64);
        s.add_dyn("边界上限放行", r.is_ok(), "上限需求合法入桶");
    }
    // 5 边界-上限+1：需求越界拒。
    {
        let mut fl = FairLedger::new();
        let r = fl.bucket_round(DEMAND_MAX as i64 + 1);
        s.add_dyn("边界上限+1 拒", r == Err(FairCode::DemandOutOfRange), "越界需求必须拒绝");
    }
    // 6 注入必抓 ①：令牌超发 ×10 全抓。
    {
        let mut fl = FairLedger::new();
        let _ = fl.bucket_round(0);
        let tokens0 = match fl.dev_ledger(0) {
            Some(d) => d.tokens,
            None => 0,
        };
        let mut caught = 0u32;
        for k in 0..10u32 {
            fl.note_injected();
            match fl.forge_grant(0, tokens0 + 1 + k as u64) {
                Err(FairCode::TokenOverIssue) => {
                    caught += 1;
                    fl.note_detected();
                }
                _ => fl.note_escaped(),
            }
        }
        s.add_dyn("令牌超发 10/10 抓", caught == 10, "超桶授予必须全抓");
    }
    // 7 注入必抓 ②：封版后权重篡改 ×10 全抓。
    {
        let mut fl = run_three_heavy_waves();
        let _ = fl.advance_phase(FairPhase::Sealed);
        let mut caught = 0u32;
        for k in 0..10u32 {
            fl.note_injected();
            match fl.set_weight(k as usize % DEVICES, 1 + k % WEIGHT_MAX) {
                Err(FairCode::WeightFrozen) => {
                    caught += 1;
                    fl.note_detected();
                }
                _ => fl.note_escaped(),
            }
        }
        s.add_dyn("权重篡改 10/10 抓", caught == 10, "封版权重必须冻结");
    }
    // 8 注入必抓 ③：账面回退 ×10 全抓。
    {
        let mut fl = run_three_heavy_waves();
        let total = fl.granted_total();
        let mut caught = 0u32;
        for k in 0..10u32 {
            fl.note_injected();
            match fl.try_rewind_granted(total.saturating_sub(1 + k as u64)) {
                Err(FairCode::LedgerRegress) => {
                    caught += 1;
                    fl.note_detected();
                }
                _ => fl.note_escaped(),
            }
        }
        s.add_dyn("账面回退 10/10 抓", caught == 10, "长期账只增不减");
    }
    // 9 判定次序：相位判据先于权重界（封版+零权重同传报冻结）。
    {
        let mut fl = FairLedger::new();
        let _ = fl.advance_phase(FairPhase::Running);
        let _ = fl.advance_phase(FairPhase::Sealed);
        let r1 = fl.set_weight(0, 0);
        let mut fl2 = FairLedger::new();
        let r2 = fl2.set_weight(0, 0);
        s.add_dyn(
            "判序相位先于权重",
            r1 == Err(FairCode::WeightFrozen) && r2 == Err(FairCode::WeightOutOfRange),
            "封版先判、零权重在 Setup 内才显形",
        );
    }
    // 10 状态机拒止：相位回退/跳段、波次回退/越位全部显性拒。
    {
        let mut fl = FairLedger::new();
        let _ = fl.advance_phase(FairPhase::Running);
        let r1 = fl.advance_phase(FairPhase::Setup);
        let mut fl2 = FairLedger::new();
        let r2 = fl2.advance_phase(FairPhase::Sealed);
        let mut fl3 = run_three_heavy_waves();
        let r3 = fl3.try_set_wave(0);
        let r4 = fl3.run_wave(DEMAND_HEAVY as i64);
        s.add_dyn(
            "状态机拒止",
            r1.is_err() && r2.is_err() && r3 == Err(FairCode::WaveRegress) && r4 == Err(FairCode::WaveRegress),
            "回退/跳段/越波全部拒绝",
        );
    }
    // 11 容量耗尽：零需求轮注满桶后出现浪费，账面仍守恒。
    {
        let mut fl = FairLedger::new();
        for _ in 0..(BUCKET_UNIT + 2) {
            let _ = fl.bucket_round(0);
        }
        let wasted4 = match fl.dev_ledger(DEVICES - 1) {
            Some(d) => d.wasted,
            None => 0,
        };
        s.add_dyn(
            "桶容截断落浪费账",
            wasted4 > 0 && fl.ledger_conserved(),
            "桶满溢出必须记浪费且守恒不破",
        );
    }
    // 12 公平性长期账：均匀重载下最大偏差必须为 0。
    {
        let fl = run_three_heavy_waves();
        s.add_dyn(
            "公平性长期账偏差为零",
            fl.fairness_max_dev() == 0 && fl.fair_within_slack(),
            "g_i×w_j == g_j×w_i 全对成立",
        );
    }
    // 13 ×3 波零饿死：饿死事件与最大零授予连击都为 0。
    {
        let fl = run_three_heavy_waves();
        let (events, streak) = fl.starvation_summary();
        s.add_dyn(
            "×3 波零饿死",
            events == 0 && streak == 0,
            "重载下每轮每设备都有授予",
        );
    }
    // 14 长期账守恒：补充 = 授予 + 桶存 + 浪费。
    {
        let fl = run_three_heavy_waves();
        s.add_dyn("长期账守恒", fl.ledger_conserved(), "每笔补充都落到授予/桶存/浪费之一");
    }
    // 15 对账守恒：注入 = 检出 + 逃逸，逃逸 0。
    {
        let mut fl = run_three_heavy_waves();
        let total = fl.granted_total();
        for k in 0..10u32 {
            fl.note_injected();
            match fl.try_rewind_granted(total.saturating_sub(1 + k as u64)) {
                Err(FairCode::LedgerRegress) => fl.note_detected(),
                _ => fl.note_escaped(),
            }
        }
        s.add_dyn(
            "注入账守恒",
            fl.conserved() && fl.injected() == 10,
            "注入=检出+逃逸，逃逸恒 0",
        );
    }
    // 16 双跑一致。
    s.add_dyn("双跑账面一致", dual_run_consistent(), "同波序两次建账逐字段相等");
    // 17 文案闭集非空 + 段位守卫。
    {
        let mut closed = true;
        for c in ALL_CODES.iter() {
            if c.detail().is_empty() {
                closed = false;
            }
        }
        s.add_dyn("文案闭集非空", closed, "每个诊断码都必须有中文建议");
    }
    s.add_dyn("段位守卫", seg_guard_ok(), "SEG_BASE 对齐且在分派区间");
    s
}

/// 诊断码全序表（闭集枚举的权威清单）。
pub const ALL_CODES: [FairCode; 9] = [
    FairCode::Ok,
    FairCode::SegBad,
    FairCode::WeightOutOfRange,
    FairCode::WeightFrozen,
    FairCode::TokenOverIssue,
    FairCode::LedgerRegress,
    FairCode::DemandOutOfRange,
    FairCode::WaveRegress,
    FairCode::LedgerMismatch,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_heavy_waves_grant_proportional() {
        let fl = run_three_heavy_waves();
        let rounds = (WAVES * ROUNDS_PER_WAVE) as u64;
        for i in 0..DEVICES {
            let d = match fl.dev_ledger(i) {
                Some(d) => d,
                None => {
                    assert!(false, "设备账缺失");
                    return;
                }
            };
            assert_eq!(d.granted, d.weight as u64 * UNIT as u64 * rounds);
            assert_eq!(d.wasted, 0);
        }
    }

    #[test]
    fn zero_demand_round_ledgered_and_conserved() {
        let mut fl = FairLedger::new();
        let rep = match fl.bucket_round(0) {
            Ok(r) => r,
            Err(_) => {
                assert!(false, "零需求轮不应失败");
                return;
            }
        };
        assert_eq!(rep.zero_grant_rows, DEVICES as u64);
        assert!(fl.ledger_conserved());
    }

    #[test]
    fn demand_bounds() {
        let mut fl = FairLedger::new();
        assert_eq!(fl.bucket_round(-1), Err(FairCode::DemandOutOfRange));
        assert!(fl.bucket_round(DEMAND_MAX as i64).is_ok());
        assert_eq!(
            fl.bucket_round(DEMAND_MAX as i64 + 1),
            Err(FairCode::DemandOutOfRange)
        );
    }

    #[test]
    fn overissue_injection_caught_ten() {
        let mut fl = FairLedger::new();
        let _ = fl.bucket_round(0);
        let tokens0 = match fl.dev_ledger(0) {
            Some(d) => d.tokens,
            None => 0,
        };
        assert!(tokens0 > 0);
        let mut caught = 0u32;
        for k in 0..10u32 {
            fl.note_injected();
            if fl.forge_grant(0, tokens0 + 1 + k as u64) == Err(FairCode::TokenOverIssue) {
                caught += 1;
                fl.note_detected();
            } else {
                fl.note_escaped();
            }
        }
        assert_eq!(caught, 10);
        assert!(fl.conserved());
    }

    #[test]
    fn weight_frozen_after_seal_caught_ten() {
        let mut fl = run_three_heavy_waves();
        let _ = fl.advance_phase(FairPhase::Sealed);
        let mut caught = 0u32;
        for k in 0..10u32 {
            fl.note_injected();
            if fl.set_weight(k as usize % DEVICES, 1 + k % WEIGHT_MAX) == Err(FairCode::WeightFrozen) {
                caught += 1;
                fl.note_detected();
            } else {
                fl.note_escaped();
            }
        }
        assert_eq!(caught, 10);
        assert!(fl.conserved());
    }

    #[test]
    fn judge_order_phase_before_weight_bounds() {
        let mut fl = FairLedger::new();
        let _ = fl.advance_phase(FairPhase::Running);
        let _ = fl.advance_phase(FairPhase::Sealed);
        assert_eq!(fl.set_weight(0, 0), Err(FairCode::WeightFrozen));
        let mut fl2 = FairLedger::new();
        assert_eq!(fl2.set_weight(0, 0), Err(FairCode::WeightOutOfRange));
        assert!(fl2.set_weight(0, WEIGHT_MAX).is_ok());
    }

    #[test]
    fn phase_and_wave_machine_rejects() {
        let mut fl = FairLedger::new();
        let _ = fl.advance_phase(FairPhase::Running);
        assert!(fl.advance_phase(FairPhase::Setup).is_err());
        let mut fl2 = FairLedger::new();
        assert!(fl2.advance_phase(FairPhase::Sealed).is_err());
        let mut fl3 = run_three_heavy_waves();
        assert_eq!(fl3.try_set_wave(0), Err(FairCode::WaveRegress));
        assert_eq!(fl3.run_wave(DEMAND_HEAVY as i64), Err(FairCode::WaveRegress));
    }

    #[test]
    fn bucket_cap_waste_and_conservation() {
        let mut fl = FairLedger::new();
        for _ in 0..(BUCKET_UNIT + 2) {
            let _ = fl.bucket_round(0);
        }
        let d = match fl.dev_ledger(DEVICES - 1) {
            Some(d) => d,
            None => {
                assert!(false, "设备账缺失");
                return;
            }
        };
        assert!(d.wasted > 0);
        assert!(d.tokens == d.weight as u64 * BUCKET_UNIT as u64);
        assert!(fl.ledger_conserved());
    }

    #[test]
    fn fairness_exact_under_heavy_load() {
        let fl = run_three_heavy_waves();
        assert_eq!(fl.fairness_max_dev(), 0);
        let (events, streak) = fl.starvation_summary();
        assert_eq!(events, 0);
        assert_eq!(streak, 0);
    }

    #[test]
    fn ledger_rewind_rejected() {
        let mut fl = run_three_heavy_waves();
        let total = fl.granted_total();
        assert!(total > 0);
        assert_eq!(fl.try_rewind_granted(total - 1), Err(FairCode::LedgerRegress));
        assert_eq!(fl.try_rewind_granted(total), Ok(()));
    }

    #[test]
    fn dual_run_identical() {
        assert!(dual_run_consistent());
    }

    #[test]
    fn seg_guard_and_closed_texts() {
        assert!(seg_guard_ok());
        for c in ALL_CODES.iter() {
            assert!(!c.detail().is_empty());
        }
    }
}
