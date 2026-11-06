//! UNX-F10042 回归周期表（日/批/波三档） · 0x91a80 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡号：UNX-F10042（B22 常态化回归线 · 周期表）。
//! 锚点：UNX-F10043-J1 触发矩阵生效（三类触发留痕 10/10 次），漏触发 10/10 次检出。
//!
//! 数据段说明：
//!   SEG_BASE = 0x91a80（卡面 seg 闭集）。诊断码占用段内 +0x01..+0x0C
//!   闭集窗口，码值 = SEG_BASE + 变体序号，段内两两唯一、段外不越界；
//!   全部码位由 `PeriodCode::ALL` 封闭枚举给出，禁止运行期造码。
//!
//! 结构清单：
//!   - 解析器 `PeriodParser`：把「tier|interval|cap」周期行逐字段拆成
//!     `PeriodRow`，三字段缺一即 ParseFailure；
//!   - 周期表 `PeriodTable`：三档周期 + 下一到期 tick，arm 前触发拒绝；
//!   - 触发留痕账 `TriggerJournal`：逐次触发 (档,tick) 入账，漏触发按
//!     期望次数−实际次数显性检出，逃逸恒 0；
//!   - 注入核 `BadPeriodCase`：负周期 / 未知档行 / 漏触发静默 / 重复周期 /
//!     未武装触发 五类坏样本逐类 10/10 次拒绝；
//!   - Linux errno 对照表 + FNV-1a 双跑指纹（无浮点、无随机）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10042 独占，0x40 对齐窗口起点）。
pub const SEG_BASE: u32 = 0x91a80;

/// 段窗上界（下一卡 0x91ac0 起点，闭开区间右端）。
pub const SEG_END: u32 = 0x91ac0;

/// 周期档数（日/批/波）。
pub const TIER_COUNT: usize = 3;

/// 周期行字段数（tier|interval|cap）。
pub const ROW_FIELDS: usize = 3;

/// 单类注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 漏触发审计地平线（tick 数；日档 1 → 期望 12 次）。
pub const AUDIT_HORIZON: u32 = 12;

/// 日档周期（每日 1 tick；来源：常态化总纲日档条款）。
pub const DAILY_INTERVAL: u32 = 1;

/// 批档周期（每批收口；来源：B22 收口小结批档条款）。
pub const BATCH_INTERVAL: u32 = 20;

/// 波档周期（每波出账；来源：B21 报表线六波节奏）。
pub const WAVE_INTERVAL: u32 = 6;

/// Linux errno 正数码（对照表口径）。
pub const EPERM: i32 = 1;
pub const ENOENT: i32 = 2;
pub const EACCES: i32 = 13;
pub const EINVAL: i32 = 22;
pub const ETIMEDOUT: i32 = 110;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PeriodCode {
    /// 负周期/零周期（interval=0）
    BadInterval,
    /// 未知档位行
    UnknownTier,
    /// 漏触发静默（期望>实际）
    MissedFire,
    /// 重复周期行
    DuplicateRow,
    /// 未武装触发（arm 前 tick）
    NotArmed,
    /// 缺档武装
    IncompleteArm,
    /// 留痕账倒退
    LedgerRegress,
    /// 解析失败
    ParseFailure,
    /// 参数越界（interval>240）
    ParamRange,
    /// 双跑不一致
    NotReproducible,
    /// 容量越界（cap=0 或 >200）
    CapRange,
    /// 段位越界
    SegmentViolation,
}

impl PeriodCode {
    /// 码值（段基址 +0x01 起连续编号，闭集唯一）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            PeriodCode::BadInterval => "负周期被拒：interval 必须 ≥1；请修正周期后重报",
            PeriodCode::UnknownTier => "未知档位被拒：周期行档位只允许 daily/batch/wave；请核对照",
            PeriodCode::MissedFire => "漏触发检出：到期档未触发即静默；请排查触发链断点",
            PeriodCode::DuplicateRow => "重复周期行被拒：同档已有周期；请先解绑再改配",
            PeriodCode::NotArmed => "未武装触发被拒：周期表武装前不得推进 tick；请先 arm",
            PeriodCode::IncompleteArm => "缺档武装被拒：三档周期未配齐不得武装；请补齐",
            PeriodCode::LedgerRegress => "留痕账倒退被拒：计数只增不减；请排查回滚误改",
            PeriodCode::ParseFailure => "解析失败：周期行必须恰为三段 tier|interval|cap；请补齐字段",
            PeriodCode::ParamRange => "参数越界被拒：周期窗口 1..=240；出窗即拦",
            PeriodCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查隐藏状态",
            PeriodCode::CapRange => "容量越界被拒：cap 窗口 1..=200；出窗即拦",
            PeriodCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在 0x91a80..=0x91ac0",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [PeriodCode; 12] = [
        PeriodCode::BadInterval,
        PeriodCode::UnknownTier,
        PeriodCode::MissedFire,
        PeriodCode::DuplicateRow,
        PeriodCode::NotArmed,
        PeriodCode::IncompleteArm,
        PeriodCode::LedgerRegress,
        PeriodCode::ParseFailure,
        PeriodCode::ParamRange,
        PeriodCode::NotReproducible,
        PeriodCode::CapRange,
        PeriodCode::SegmentViolation,
    ];
}

// ── 档位与解析器 ──────────────────────────────────────────────────────
/// 周期档位（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TierTag {
    /// 日档
    Daily,
    /// 批档
    PerBatch,
    /// 波档
    PerWave,
}

impl TierTag {
    /// 闭集全量。
    pub const ALL: [TierTag; 3] = [TierTag::Daily, TierTag::PerBatch, TierTag::PerWave];

    /// 从文本解析（"daily"/"batch"/"wave"）。
    pub fn from_field(s: &str) -> Option<TierTag> {
        match s {
            "daily" => Some(TierTag::Daily),
            "batch" => Some(TierTag::PerBatch),
            "wave" => Some(TierTag::PerWave),
            _ => None,
        }
    }

    /// 槽位下标。
    pub fn index(self) -> usize {
        match self {
            TierTag::Daily => 0,
            TierTag::PerBatch => 1,
            TierTag::PerWave => 2,
        }
    }
}

/// 解析出的周期行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PeriodRow {
    /// 档位
    pub tier: TierTag,
    /// 周期（tick 数）
    pub interval: u32,
    /// 单轮容量上限
    pub cap: u32,
}

/// 周期文本解析器：字段数恰为 3，数值段须可解析，缺一即 ParseFailure。
#[derive(Default)]
pub struct PeriodParser {
    /// 累计成功解析条数
    pub parsed: u32,
    /// 累计拒绝条数
    pub rejected: u32,
}

impl PeriodParser {
    /// 解析一行周期行；失败原因由 PeriodCode 表达。
    pub fn parse(&mut self, text: &str) -> Result<PeriodRow, PeriodCode> {
        let parts: Vec<&str> = text.split('|').collect();
        if parts.len() != ROW_FIELDS {
            self.rejected = self.rejected.saturating_add(1);
            return Err(PeriodCode::ParseFailure);
        }
        let tier = match TierTag::from_field(parts.get(0).copied().unwrap_or("")) {
            Some(t) => t,
            None => {
                self.rejected = self.rejected.saturating_add(1);
                return Err(PeriodCode::UnknownTier);
            }
        };
        let interval = parse_u32_dec(parts.get(1).copied().unwrap_or(""));
        let cap = parse_u32_dec(parts.get(2).copied().unwrap_or(""));
        match (interval, cap) {
            (Some(iv), Some(cp)) => {
                self.parsed = self.parsed.saturating_add(1);
                Ok(PeriodRow { tier, interval: iv, cap: cp })
            }
            _ => {
                self.rejected = self.rejected.saturating_add(1);
                Err(PeriodCode::ParseFailure)
            }
        }
    }
}

fn parse_u32_dec(s: &str) -> Option<u32> {
    if s.is_empty() {
        return None;
    }
    let mut acc: u32 = 0;
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes.get(i)?;
        if !b.is_ascii_digit() {
            return None;
        }
        acc = acc.checked_mul(10)?.checked_add((*b - b'0') as u32)?;
        i += 1;
    }
    Some(acc)
}

// ── 周期表与触发留痕账 ────────────────────────────────────────────────
/// 触发留痕账：逐次触发 (档下标, tick) 入账。
#[derive(Clone, Debug, Default)]
pub struct TriggerJournal {
    /// 留痕条目
    pub entries: Vec<(u8, u32)>,
    /// 触发总次数
    pub fired: u32,
    /// 漏触发检出数
    pub missed: u32,
}

impl TriggerJournal {
    /// 记一次触发留痕。
    pub fn trace(&mut self, tier_idx: u8, tick: u32) {
        self.entries.push((tier_idx, tick));
        self.fired = self.fired.saturating_add(1);
    }

    /// 漏触发审计：每档期望触发次数 = horizon / interval，实际少一次记一次漏。
    pub fn audit(&mut self, intervals: &[u32; TIER_COUNT], horizon: u32) {
        self.missed = 0;
        let mut t = 0usize;
        while t < TIER_COUNT {
            let iv = *intervals.get(t).unwrap_or(&0);
            if iv > 0 {
                let expected = horizon / iv;
                let mut actual = 0u32;
                for e in self.entries.iter() {
                    if e.0 as usize == t {
                        actual = actual.saturating_add(1);
                    }
                }
                if expected > actual {
                    self.missed = self.missed.saturating_add(expected - actual);
                }
            }
            t += 1;
        }
    }
}

/// 周期表：三档周期 + 下一到期 tick + 武装状态。
#[derive(Clone, Copy, Debug)]
pub struct PeriodTable {
    /// 三档周期（0=未配置）
    pub intervals: [u32; TIER_COUNT],
    /// 三档下一到期 tick
    pub next_due: [u32; TIER_COUNT],
    /// 是否已武装
    pub armed: bool,
}

impl PeriodTable {
    /// 构造空周期表。
    pub const fn new() -> PeriodTable {
        PeriodTable { intervals: [0; TIER_COUNT], next_due: [1; TIER_COUNT], armed: false }
    }

    /// 配置一档周期：interval/cap 越窗与重复配置显性拒绝。
    pub fn set(&mut self, tier: TierTag, interval: u32, cap: u32) -> Result<(), PeriodCode> {
        if interval == 0 {
            return Err(PeriodCode::BadInterval);
        }
        if interval > 240 {
            return Err(PeriodCode::ParamRange);
        }
        if cap == 0 || cap > 200 {
            return Err(PeriodCode::CapRange);
        }
        let idx = tier.index();
        if *self.intervals.get(idx).unwrap_or(&0) != 0 {
            return Err(PeriodCode::DuplicateRow);
        }
        if let Some(slot) = self.intervals.get_mut(idx) {
            *slot = interval;
        }
        if let Some(due) = self.next_due.get_mut(idx) {
            *due = interval;
        }
        Ok(())
    }

    /// 武装：三档配齐才可武装；武装后 tick 才可推进。
    pub fn arm(&mut self) -> Result<(), PeriodCode> {
        if self.armed {
            return Err(PeriodCode::DuplicateRow);
        }
        let mut i = 0usize;
        while i < TIER_COUNT {
            if *self.intervals.get(i).unwrap_or(&0) == 0 {
                return Err(PeriodCode::IncompleteArm);
            }
            i += 1;
        }
        self.armed = true;
        Ok(())
    }

    /// 推进一个 tick：到期档触发并留痕；返回本 tick 触发档数。
    pub fn tick(&mut self, t: u32, journal: &mut TriggerJournal) -> Result<usize, PeriodCode> {
        if !self.armed {
            return Err(PeriodCode::NotArmed);
        }
        let mut fired = 0usize;
        let mut i = 0usize;
        while i < TIER_COUNT {
            let due = *self.next_due.get(i).unwrap_or(&u32::MAX);
            if t >= due {
                if let Some(due_slot) = self.next_due.get_mut(i) {
                    let iv = *self.intervals.get(i).unwrap_or(&1);
                    *due_slot = t.saturating_add(iv);
                }
                journal.trace(i as u8, t);
                fired = fired.saturating_add(1);
            }
            i += 1;
        }
        Ok(fired)
    }
}

/// 标准三档配置并武装。
pub fn arm_standard(table: &mut PeriodTable) -> Result<(), PeriodCode> {
    table.set(TierTag::Daily, DAILY_INTERVAL, 200)?;
    table.set(TierTag::PerBatch, BATCH_INTERVAL, 200)?;
    table.set(TierTag::PerWave, WAVE_INTERVAL, 60)?;
    table.arm()
}

/// 推进 1..=horizon 全部 tick（到期必触发），返回留痕账。
pub fn run_ticks(table: &mut PeriodTable, horizon: u32) -> TriggerJournal {
    let mut j = TriggerJournal::default();
    let mut t = 1u32;
    while t <= horizon {
        let _ = table.tick(t, &mut j);
        t += 1;
    }
    j
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 五类坏周期样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadPeriodCase {
    /// 零周期
    BadInterval,
    /// 未知档位行
    UnknownTier,
    /// 漏触发静默（日档中途哑火）
    MissedFire,
    /// 重复周期行
    DuplicateRow,
    /// 未武装触发
    NotArmed,
}

impl BadPeriodCase {
    /// 闭集全量。
    pub const ALL: [BadPeriodCase; 5] = [
        BadPeriodCase::BadInterval,
        BadPeriodCase::UnknownTier,
        BadPeriodCase::MissedFire,
        BadPeriodCase::DuplicateRow,
        BadPeriodCase::NotArmed,
    ];

    /// 注入一轮：构造坏样本并触发周期表/审计，返回是否被显式拒绝。
    pub fn inject(self) -> bool {
        match self {
            BadPeriodCase::BadInterval => {
                let mut tb = PeriodTable::new();
                tb.set(TierTag::Daily, 0, 200).is_err()
            }
            BadPeriodCase::UnknownTier => {
                let mut p = PeriodParser::default();
                p.parse("nightly|1|200").is_err()
            }
            BadPeriodCase::MissedFire => {
                // 正常地平线内日档应触发 horizon 次；哑火版缺后 5 次 → 审计检出。
                let mut good = PeriodTable::new();
                if arm_standard(&mut good).is_err() {
                    return false;
                }
                let mut full = run_ticks(&mut good, AUDIT_HORIZON);
                full.audit(&good.intervals, AUDIT_HORIZON);
                let mut muted = PeriodTable::new();
                if arm_standard(&mut muted).is_err() {
                    return false;
                }
                let mut j = run_ticks(&mut muted, AUDIT_HORIZON / 2);
                j.audit(&muted.intervals, AUDIT_HORIZON);
                full.missed == 0 && j.missed > 0
            }
            BadPeriodCase::DuplicateRow => {
                let mut tb = PeriodTable::new();
                if tb.set(TierTag::Daily, DAILY_INTERVAL, 200).is_err() {
                    return false;
                }
                tb.set(TierTag::Daily, DAILY_INTERVAL, 200).is_err()
            }
            BadPeriodCase::NotArmed => {
                let mut tb = PeriodTable::new();
                if tb.set(TierTag::Daily, DAILY_INTERVAL, 200).is_err() {
                    return false;
                }
                if tb.set(TierTag::PerBatch, BATCH_INTERVAL, 200).is_err() {
                    return false;
                }
                let mut j = TriggerJournal::default();
                tb.tick(1, &mut j).is_err()
            }
        }
    }
}

/// 注入账：注入 = 检出 + 逃逸，逃逸恒 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FireLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl FireLedger {
    const fn zero() -> FireLedger {
        FireLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果；账失衡回 LedgerRegress。
    pub fn record(&mut self, caught: bool) -> Result<(), PeriodCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(PeriodCode::LedgerRegress);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行五类坏样本注入：每类 INJECT_ROUNDS 次，返回账本与逐类检出数。
pub fn run_period_injection() -> (FireLedger, [u32; 5]) {
    let mut ledger = FireLedger::zero();
    let mut per_class = [0u32; 5];
    let mut ci = 0usize;
    while ci < BadPeriodCase::ALL.len() {
        if let Some(case) = BadPeriodCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = case.inject();
                let _ = ledger.record(caught);
                if caught {
                    if let Some(slot) = per_class.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                }
                round += 1;
            }
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；未武装触发族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；未知档位族"),
    ("EACCES", -13, 5, "Windows ERROR_ACCESS_DENIED；重复配置占用族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；负周期/越窗族"),
    ("ETIMEDOUT", -110, 146, "Windows ERROR_TIMEOUT；漏触发超时族"),
];

/// 查表：按 errno 正数取负值；表外返回 0（显性可查）。
pub fn linux_neg(code: i32) -> i32 {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == -code {
                return row.1;
            }
        }
        i += 1;
    }
    0
}

/// 与 Linux 逐码一致：只允许表内码且负值精确匹配。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 规范语料双跑指纹：解析→配置→武装→推 tick 全链逐字节入哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut p = PeriodParser::default();
    let mut tb = PeriodTable::new();
    let mut bytes: Vec<u8> = Vec::new();
    let texts = ["daily|1|200", "batch|20|200", "wave|6|60", "nightly|1|1"];
    let mut i = 0usize;
    while i < texts.len() {
        if let Some(t) = texts.get(i) {
            let r = p.parse(t);
            bytes.push(if r.is_ok() { 1 } else { 0 });
            if let Ok(row) = r {
                bytes.push(match tb.set(row.tier, row.interval, row.cap) {
                    Ok(()) => 2,
                    Err(_) => 3,
                });
            }
        }
        i += 1;
    }
    bytes.push(match tb.arm() {
        Ok(()) => 4,
        Err(_) => 5,
    });
    let j = run_ticks(&mut tb, AUDIT_HORIZON);
    bytes.extend_from_slice(&j.fired.to_le_bytes());
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10042 判据集（17 条静态 + 2 条动态明细）。
pub fn run_unx10042_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10042");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (SEG_BASE..SEG_END).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x91a80..=0x91ac0", seg_ok,
          "段基址唯一且落在本卡窗口；越界即拒绝注册");

    // 2. 解析器：合法三段行可解析
    let mut p = PeriodParser::default();
    let ok_parse = p.parse("daily|1|200").is_ok();
    s.add("解析器：合法三段周期行 1/1 解析成功", ok_parse,
          "tier|interval|cap 三段齐备即入账");

    // 3. 解析器：缺段/未知档/坏数字全拒
    let bad = p.parse("daily|1").is_err()
        && p.parse("nightly|1|1").is_err()
        && p.parse("daily|x|1").is_err()
        && p.parse("|1|1").is_err();
    s.add("解析器：缺段/未知档/坏数字 4/4 拒绝", bad,
          "任何字段缺失或不可解析都不得静默吞掉");

    // 4. 配置窗：负周期/超窗/坏容量全拦
    let mut tb = PeriodTable::new();
    let range_ok = tb.set(TierTag::Daily, 0, 200).is_err()
        && tb.set(TierTag::Daily, 999, 200).is_err()
        && tb.set(TierTag::Daily, 1, 0).is_err()
        && tb.set(TierTag::Daily, 1, 999).is_err();
    s.add("参数窗：周期 1..=240、容量 1..=200 越界拒绝", range_ok,
          "周期与容量闭集窗口，出窗即拦");

    // 5. 重复配置拦截
    let mut dup = PeriodTable::new();
    let _ = dup.set(TierTag::PerWave, WAVE_INTERVAL, 60);
    let dup_ok = dup.set(TierTag::PerWave, WAVE_INTERVAL, 60) == Err(PeriodCode::DuplicateRow);
    s.add("重复配置：同档二次配置被拒", dup_ok,
          "一档一周期，重复配置显性拒绝");

    // 6. 缺档武装拦截 + 齐档武装成功
    let mut part = PeriodTable::new();
    let _ = part.set(TierTag::Daily, DAILY_INTERVAL, 200);
    let part_rej = part.arm() == Err(PeriodCode::IncompleteArm);
    let mut full = PeriodTable::new();
    let full_ok = arm_standard(&mut full).is_ok() && full.armed;
    s.add("武装闸：缺档拒绝、三档齐武装通过", part_rej && full_ok,
          "武装是单向闸，未武装不得推 tick");

    // 7. 未武装触发拦截
    let mut nf = PeriodTable::new();
    let _ = nf.set(TierTag::Daily, DAILY_INTERVAL, 200);
    let mut jd = TriggerJournal::default();
    let nf_rej = nf.tick(1, &mut jd) == Err(PeriodCode::NotArmed);
    s.add("未武装触发：arm 前 tick 被拒", nf_rej,
          "武装前任何触发请求都显性拒绝");

    // 8. 到期必触发：地平线 12 内日档触发 12 次、波档 2 次
    let mut arm_tb = PeriodTable::new();
    let _ = arm_standard(&mut arm_tb);
    let j = run_ticks(&mut arm_tb, AUDIT_HORIZON);
    let mut daily_n = 0u32;
    let mut wave_n = 0u32;
    for e in j.entries.iter() {
        if e.0 == 0 {
            daily_n = daily_n.saturating_add(1);
        }
        if e.0 == 2 {
            wave_n = wave_n.saturating_add(1);
        }
    }
    s.add("到期必触发：日档 12/12、波档 2/12 地平线内入账",
          daily_n == AUDIT_HORIZON && wave_n == AUDIT_HORIZON / WAVE_INTERVAL,
          "周期 1 → 12 tick 12 次；周期 6 → 12 tick 2 次");

    // 9. 漏触发审计：全量零漏、哑火版检出
    let mut good_tb = PeriodTable::new();
    let _ = arm_standard(&mut good_tb);
    let mut good_j = run_ticks(&mut good_tb, AUDIT_HORIZON);
    good_j.audit(&good_tb.intervals, AUDIT_HORIZON);
    let mut mute_tb = PeriodTable::new();
    let _ = arm_standard(&mut mute_tb);
    let mut mute_j = run_ticks(&mut mute_tb, AUDIT_HORIZON / 2);
    mute_j.audit(&mute_tb.intervals, AUDIT_HORIZON);
    s.add("漏触发审计：全量零漏、半程哑火显性检出",
          good_j.missed == 0 && mute_j.missed > 0,
          "期望=horizon/interval，实际少一次记一次漏");

    // 10. 注入核：五类坏样本全检出
    let (ledger, per_class) = run_period_injection();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：五类坏样本各 10/10 检出", all_caught,
          "负周期/未知档/漏触发/重复/未武装全被显式拒绝");

    // 11. 注入账守恒：逃逸=0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == 5 * INJECT_ROUNDS,
          "50 次注入必须 50 次检出、0 逃逸");

    // 12. 文案闭集：非空
    let text_ok = PeriodCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 13. 诊断码唯一 + 落段
    let mut uniq = true;
    let mut i = 0usize;
    while i < PeriodCode::ALL.len() {
        let mut jj = i + 1;
        while jj < PeriodCode::ALL.len() {
            if let (Some(x), Some(y)) = (PeriodCode::ALL.get(i), PeriodCode::ALL.get(jj)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            jj += 1;
        }
        i += 1;
    }
    let in_seg = PeriodCode::ALL.iter().all(|c| c.val() > SEG_BASE && c.val() <= SEG_BASE + 0x0C);
    s.add("诊断码：12 个码值两两唯一且落段窗", uniq && in_seg,
          "码值闭集唯一且不得越过 +0x0C 窗口");

    // 14. Linux 逐码一致
    let codes_ok = agrees_with_linux(EPERM)
        && agrees_with_linux(EACCES)
        && agrees_with_linux(ENOENT)
        && agrees_with_linux(EINVAL)
        && agrees_with_linux(ETIMEDOUT)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 15. 双跑一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", f1 == f2,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    // 16. 解析账闭合
    let mut p2 = PeriodParser::default();
    let _ = p2.parse("daily|1|200");
    let _ = p2.parse("bad");
    let _ = p2.parse("wave|6|60");
    s.add("解析账闭合：解析+拒绝=提交总数",
          p2.parsed.saturating_add(p2.rejected) == 3,
          "每行提交必须入两账之一，无第三去向");

    // 17. 触发留痕 10/10：连续十轮到期必触发（日档逐 tick 留痕）
    let mut ten_ok = true;
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        let mut tb10 = PeriodTable::new();
        if arm_standard(&mut tb10).is_err() {
            ten_ok = false;
            break;
        }
        let j10 = run_ticks(&mut tb10, AUDIT_HORIZON);
        if j10.fired != AUDIT_HORIZON + AUDIT_HORIZON / WAVE_INTERVAL {
            // 批档周期 20 > 12，不触发；日 12 + 波 2 = 14。
            ten_ok = false;
            break;
        }
        r += 1;
    }
    s.add("触发留痕：十轮到期必触发逐次入账", ten_ok,
          "每轮 14 次留痕（日 12 + 波 2），漏一次即红");

    // 动态明细（≤8）：留痕账一行 + 指纹一行
    let fired_text = format!("地平线 {} tick 触发 {} 次 / 漏检 {}", AUDIT_HORIZON, j.fired, good_j.missed);
    s.add_dyn("触发快照", daily_n == AUDIT_HORIZON && good_j.missed == 0, &fired_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_accepts_wellformed() {
        let mut p = PeriodParser::default();
        let r = p.parse("batch|20|200");
        assert!(r.is_ok());
        assert_eq!(r.unwrap_or(PeriodRow { tier: TierTag::Daily, interval: 0, cap: 0 }).interval, 20);
    }

    #[test]
    fn parser_rejects_malformed() {
        let mut p = PeriodParser::default();
        assert!(p.parse("daily|1").is_err());
        assert!(p.parse("nightly|1|1").is_err());
        assert!(p.parse("daily|1|abc").is_err());
    }

    #[test]
    fn bad_interval_rejected() {
        let mut tb = PeriodTable::new();
        assert_eq!(tb.set(TierTag::Daily, 0, 200), Err(PeriodCode::BadInterval));
    }

    #[test]
    fn duplicate_row_rejected() {
        let mut tb = PeriodTable::new();
        let _ = tb.set(TierTag::Daily, 1, 200);
        assert_eq!(tb.set(TierTag::Daily, 1, 200), Err(PeriodCode::DuplicateRow));
    }

    #[test]
    fn incomplete_arm_rejected() {
        let mut tb = PeriodTable::new();
        let _ = tb.set(TierTag::Daily, 1, 200);
        assert_eq!(tb.arm(), Err(PeriodCode::IncompleteArm));
    }

    #[test]
    fn tick_before_arm_rejected() {
        let mut tb = PeriodTable::new();
        let _ = tb.set(TierTag::Daily, 1, 200);
        let mut j = TriggerJournal::default();
        assert_eq!(tb.tick(1, &mut j), Err(PeriodCode::NotArmed));
    }

    #[test]
    fn due_fires_every_interval() {
        let mut tb = PeriodTable::new();
        let _ = arm_standard(&mut tb);
        let j = run_ticks(&mut tb, AUDIT_HORIZON);
        assert_eq!(j.fired, AUDIT_HORIZON + AUDIT_HORIZON / WAVE_INTERVAL);
    }

    #[test]
    fn audit_detects_muted_tier() {
        let mut tb = PeriodTable::new();
        let _ = arm_standard(&mut tb);
        let mut j = run_ticks(&mut tb, AUDIT_HORIZON / 2);
        j.audit(&tb.intervals, AUDIT_HORIZON);
        assert!(j.missed > 0);
    }

    #[test]
    fn injection_matrix_zero_escape() {
        let (l, per) = run_period_injection();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, 5 * INJECT_ROUNDS);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(ETIMEDOUT));
        assert!(agrees_with_linux(EINVAL));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(ENOENT), -ENOENT);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10042_checks_green() {
        let s = run_unx10042_checks();
        assert!(s.all_passed(), "UNX-F10042 red: {}", s.red_items_text());
    }
}
