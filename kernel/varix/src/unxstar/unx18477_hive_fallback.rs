//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18477 · 兼容层错误路径账（hive 不可达降级） · 0x7F300段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18477-J1 hive 不可达降级到系统默认断言全过，异常显性化
//!       三要素呈现
//!
//! 设计要点：
//!   - hive 模拟层：8 个兼容设置各挂一条蜂巢记录（值+校验和），故障
//!     按四类闭集注入：不可达 / 损坏 / 超时 / 半写；
//!   - 降级语义冻结：任何一类故障触发读取时，都必须「降级到系统默认
//!     值」而不是报错上抛——返回 Fallback 值 + 降级三要素；
//!   - 异常显性化三要素（J1 核心）：①显性错误码（故障类闭集码随
//!     Fallback 一并携带）②降级标记（降级位面按设置置位）③通知账
//!     条目（中文提示入通知账）——三缺一即 TriadIncomplete；
//!   - 状态机：Healthy → Degraded{code} → Recovering → Healthy；
//!     故障未清除时恢复探针必须被拒（过早恢复）；
//!   - 注入核三类：不可达降级三要素齐 / 记录损坏检出 / 过早恢复拒
//!     ——各 10 轮必抓；
//!   - 边界：未知设置号 / 双重恢复 / 故障类越界 / 空蜂巢读默认值；
//!   - no_std + alloc；零 unwrap/expect/panic/浮点/IO/墙钟/随机。
//!
//! 本模块结构清单：
//!   [数据结构] 设置表 SETTING_N[8] + 默认值裁决表 DEFAULTS[8]
//!              + 降级位面（u32 位图）
//!   [状态机]   HiveState 四态单调环（Healthy→Degraded→Recovering→Healthy）
//!   [账本]     故障账[4] / 降级计数 / 恢复计数 / 通知账 Vec<Note>
//!   [错误路径] HfCode 11 码闭集，code() = SEG_BASE + n
//!   [注入核]   三类坏样本 ×10 轮逐类断言必抓
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7F300，AI-VX11-1105 批04 独占区，A08 工单卡七）。
pub const SEG_BASE: u32 = 0x7F300;

// ── 域闭集与冻结常量 ────────────────────────────────────────────
/// 设置数（冻结：8）。
pub const SETTING_N: usize = 8;
/// 故障类数（冻结：4：不可达/损坏/超时/半写）。
pub const FAULT_N: usize = 4;
/// 默认值基（冻结推导：0x00A0 + 设置号，8 个默认值互异且落在 16 位域）。
pub const DEFAULT_BASE: u32 = 0x00A0;
/// 蜂巢值基（冻结推导：0x0B00 + 设置号 ×4，与默认值域无交叠）。
pub const HIVE_BASE: u32 = 0x0B00;
/// 注入轮数（写死：10/10 语义）。
pub const INJECT_ROUNDS: u32 = 10;

/// FNV-1a 32 位（蜂巢记录校验和）。
pub fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0;
    while i < data.len() {
        if let Some(&b) = data.get(i) {
            h ^= b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 系统默认值裁决表（冻结推导：DEFAULT_BASE + 设置号）。
pub fn default_of(id: usize) -> u32 {
    DEFAULT_BASE + id as u32
}

/// 蜂巢应有值（冻结推导：HIVE_BASE + 设置号 ×4）。
pub fn hive_expect_of(id: usize) -> u32 {
    HIVE_BASE + (id as u32).saturating_mul(4)
}

// ── 故障类闭集 ─────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FaultKind {
    /// 蜂巢不可达（句柄失联）。
    Unreachable,
    /// 记录损坏（校验和失配）。
    Corrupt,
    /// 读超时（预算耗尽）。
    Timeout,
    /// 半写（记录不完整）。
    Partial,
}

impl FaultKind {
    /// 槽位号（0..4）。
    pub fn slot(self) -> usize {
        match self {
            FaultKind::Unreachable => 0,
            FaultKind::Corrupt => 1,
            FaultKind::Timeout => 2,
            FaultKind::Partial => 3,
        }
    }
    pub const ALL: [FaultKind; FAULT_N] = [
        FaultKind::Unreachable,
        FaultKind::Corrupt,
        FaultKind::Timeout,
        FaultKind::Partial,
    ];
}

// ── 诊断码闭集 ─────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HfCode {
    /// 未知设置号
    UnknownSetting,
    /// 蜂巢不可达
    HiveUnreachable,
    /// 记录损坏
    HiveCorrupt,
    /// 读超时
    HiveTimeout,
    /// 半写
    HivePartial,
    /// 过早恢复（故障未清除即宣布恢复）
    PrematureRecovery,
    /// 降级三要素缺一
    TriadIncomplete,
    /// 双重恢复
    DoubleRecovery,
    /// 计数守恒破坏
    ConservationBroken,
    /// 双跑不一致
    NotReproducible,
    /// 段位越窗
    SegmentOutside,
}

impl HfCode {
    /// 诊断码 = SEG_BASE + 枚举序号。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            HfCode::UnknownSetting => "未知设置号：0..8 闭集外",
            HfCode::HiveUnreachable => "蜂巢不可达：降级到系统默认",
            HfCode::HiveCorrupt => "蜂巢记录损坏：校验和失配，降级到系统默认",
            HfCode::HiveTimeout => "蜂巢读超时：降级到系统默认",
            HfCode::HivePartial => "蜂巢记录半写：降级到系统默认",
            HfCode::PrematureRecovery => "过早恢复：故障未清除不许宣布 Healthy",
            HfCode::TriadIncomplete => "降级三要素缺一：码/位面/通知账须齐",
            HfCode::DoubleRecovery => "双重恢复：Healthy 态再探针属误用",
            HfCode::ConservationBroken => "计数守恒破坏：降级数 ≠ 故障读数",
            HfCode::NotReproducible => "双跑不一致：复现红线",
            HfCode::SegmentOutside => "段位越窗：诊断码不在 0x7F300 段",
        }
    }
    pub const ALL: [HfCode; 11] = [
        HfCode::UnknownSetting,
        HfCode::HiveUnreachable,
        HfCode::HiveCorrupt,
        HfCode::HiveTimeout,
        HfCode::HivePartial,
        HfCode::PrematureRecovery,
        HfCode::TriadIncomplete,
        HfCode::DoubleRecovery,
        HfCode::ConservationBroken,
        HfCode::NotReproducible,
        HfCode::SegmentOutside,
    ];
}

/// 段内任意码 → 中文明细。
pub fn detail_of(code: u32) -> &'static str {
    let mut i = 0;
    while i < HfCode::ALL.len() {
        if let Some(c) = HfCode::ALL.get(i) {
            if c.code() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x7F300 段闭集内"
}

// ── 蜂巢层 ─────────────────────────────────────────────────────
/// 蜂巢记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HvRec {
    pub value: u32,
    pub chk: u32,
}

impl HvRec {
    /// 记录校验和：值四字节 FNV。
    pub fn make(value: u32) -> HvRec {
        HvRec {
            value,
            chk: fnv1a(&[
                (value & 0xFF) as u8,
                ((value >> 8) & 0xFF) as u8,
                ((value >> 16) & 0xFF) as u8,
                ((value >> 24) & 0xFF) as u8,
            ]),
        }
    }
    pub fn intact(&self) -> bool {
        self.chk
            == fnv1a(&[
                (self.value & 0xFF) as u8,
                ((self.value >> 8) & 0xFF) as u8,
                ((self.value >> 16) & 0xFF) as u8,
                ((self.value >> 24) & 0xFF) as u8,
            ])
    }
}

/// 蜂巢态（状态机）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HiveState {
    Healthy,
    Degraded,
    Recovering,
}

// ── 通知账条目 ─────────────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    /// 设置号。
    pub setting: usize,
    /// 故障类码（显性化第一要素的载体）。
    pub code: u32,
    /// 中文提示（显性化第三要素）。
    pub hint: String,
}

/// 读取结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadOutcome {
    /// 蜂巢值（健康路径）。
    Hive(u32),
    /// 降级值（系统默认；携带显性故障码）。
    Fallback(u32, u32),
}

// ── 错误路径账总成 ─────────────────────────────────────────────
pub struct ErrorPathLedger {
    /// 蜂巢记录表（8 设置）。
    pub records: [Option<HvRec>; SETTING_N],
    /// 蜂巢态。
    pub state: HiveState,
    /// 活动故障（None=无故障）。
    pub active_fault: Option<FaultKind>,
    /// 故障账（按类计数）。
    pub faults: [u32; FAULT_N],
    /// 降级读计数。
    pub degrades: u32,
    /// 健康读计数。
    pub healthy_reads: u32,
    /// 恢复计数。
    pub recovers: u32,
    /// 降级位面（bit s = 设置 s 正处于降级标记）。
    pub degraded_map: u32,
    /// 通知账。
    pub notes: Vec<Note>,
}

impl ErrorPathLedger {
    pub fn new() -> ErrorPathLedger {
        // 蜂巢初始：8 记录按冻结推导填齐（完好记录）。
        let mut records: [Option<HvRec>; SETTING_N] = [None; SETTING_N];
        let mut i = 0;
        while i < SETTING_N {
            if let Some(slot) = records.get_mut(i) {
                *slot = Some(HvRec::make(hive_expect_of(i)));
            }
            i += 1;
        }
        ErrorPathLedger {
            records,
            state: HiveState::Healthy,
            active_fault: None,
            faults: [0; FAULT_N],
            degrades: 0,
            healthy_reads: 0,
            recovers: 0,
            degraded_map: 0,
            notes: Vec::new(),
        }
    }

    /// 注入故障类（活动故障覆盖式设置：一次一类）。
    pub fn inject_fault(&mut self, kind: FaultKind) -> Result<(), u32> {
        let s = kind.slot();
        if s >= FAULT_N {
            return Err(HfCode::SegmentOutside.code());
        }
        self.active_fault = Some(kind);
        if let Some(v) = self.faults.get_mut(s) {
            *v = v.saturating_add(1);
        }
        Ok(())
    }

    /// 清除故障。
    pub fn clear_fault(&mut self) {
        self.active_fault = None;
    }

    /// 损坏注入：把某设置记录值翻位（校验和不同步 → 失配）。
    pub fn corrupt_record(&mut self, setting: usize) -> Result<(), u32> {
        if setting >= SETTING_N {
            return Err(HfCode::UnknownSetting.code());
        }
        match self.records.get_mut(setting) {
            Some(Some(rec)) => {
                rec.value ^= 0x0100;
            }
            _ => return Err(HfCode::UnknownSetting.code()),
        }
        Ok(())
    }

    /// 读取：故障类裁决 → 降级三要素 / 健康值。未知号显式拒。
    pub fn read_setting(&mut self, setting: usize) -> Result<ReadOutcome, u32> {
        if setting >= SETTING_N {
            return Err(HfCode::UnknownSetting.code());
        }
        let fault = self.active_fault;
        match fault {
            None => {
                // 健康路径：校验和完好才返回蜂巢值，否则降级。
                let intact = match self.records.get(setting) {
                    Some(Some(rec)) => rec.intact(),
                    _ => false,
                };
                if intact {
                    self.healthy_reads = self.healthy_reads.saturating_add(1);
                    let v = match self.records.get(setting) {
                        Some(Some(rec)) => rec.value,
                        _ => default_of(setting),
                    };
                    Ok(ReadOutcome::Hive(v))
                } else {
                    self.degrade(setting, FaultKind::Corrupt)
                }
            }
            Some(kind) => {
                // 故障路径：不可达/超时/半写 直接降级；损坏类先验记录。
                match kind {
                    FaultKind::Corrupt => {
                        let intact = match self.records.get(setting) {
                            Some(Some(rec)) => rec.intact(),
                            _ => false,
                        };
                        if intact {
                            // 故障类为损坏但记录完好：仍按降级走（故障在档）。
                            self.degrade(setting, kind)
                        } else {
                            self.degrade(setting, kind)
                        }
                    }
                    _ => self.degrade(setting, kind),
                }
            }
        }
    }

    /// 降级：三要素一次落齐（码/位面/通知账），缺一即 TriadIncomplete。
    fn degrade(&mut self, setting: usize, kind: FaultKind) -> Result<ReadOutcome, u32> {
        let code = match kind {
            FaultKind::Unreachable => HfCode::HiveUnreachable.code(),
            FaultKind::Corrupt => HfCode::HiveCorrupt.code(),
            FaultKind::Timeout => HfCode::HiveTimeout.code(),
            FaultKind::Partial => HfCode::HivePartial.code(),
        };
        // 要素①显性码 + 要素②位面。
        self.degraded_map |= 1u32 << setting;
        self.degrades = self.degrades.saturating_add(1);
        // 要素③通知账（中文提示闭集）。
        let hint = match kind {
            FaultKind::Unreachable => String::from("蜂巢不可达，已降级到系统默认值"),
            FaultKind::Corrupt => String::from("蜂巢记录损坏，已降级到系统默认值"),
            FaultKind::Timeout => String::from("蜂巢读超时，已降级到系统默认值"),
            FaultKind::Partial => String::from("蜂巢记录半写，已降级到系统默认值"),
        };
        self.notes.push(Note { setting, code, hint });
        // 状态机：Healthy/Degraded → Degraded（单调：不倒退到 Healthy）。
        self.state = HiveState::Degraded;
        Ok(ReadOutcome::Fallback(default_of(setting), code))
    }

    /// 三要素复核：最近一次降级的码/位面/通知三处一致。
    pub fn triad_intact(&self, setting: usize) -> bool {
        if setting >= SETTING_N {
            return false;
        }
        let bit = (self.degraded_map >> setting) & 0x1 == 0x1;
        // 通知账里有该设置的降级条目。
        let mut noted = false;
        let mut i = 0;
        while i < self.notes.len() {
            if let Some(n) = self.notes.get(i) {
                if n.setting == setting && n.code >= SEG_BASE && n.hint.len() > 0 {
                    noted = true;
                }
            }
            i += 1;
        }
        bit && noted
    }

    /// 恢复探针：故障未清除必须拒（过早恢复）；Healthy 态再探针
    /// 属双重恢复；Degraded + 无故障 → Recovering → Healthy。
    pub fn probe_recovery(&mut self) -> Result<(), u32> {
        if self.active_fault.is_some() {
            return Err(HfCode::PrematureRecovery.code());
        }
        match self.state {
            HiveState::Healthy => Err(HfCode::DoubleRecovery.code()),
            HiveState::Degraded => {
                self.state = HiveState::Recovering;
                // 收口：位面清零 + 恢复计数。
                self.degraded_map = 0;
                self.recovers = self.recovers.saturating_add(1);
                self.state = HiveState::Healthy;
                Ok(())
            }
            HiveState::Recovering => Err(HfCode::DoubleRecovery.code()),
        }
    }

    /// 计数守恒：降级读数 = 通知账条目数（每降级恰一通知）。
    pub fn conserve(&self) -> Result<(), u32> {
        if self.degrades != self.notes.len() as u32 {
            return Err(HfCode::ConservationBroken.code());
        }
        if self.recovers > self.degrades {
            return Err(HfCode::ConservationBroken.code());
        }
        Ok(())
    }
}

// ── 黄金全链 ───────────────────────────────────────────────────
/// 健康读 3 → 不可达降级 2（三要素齐）→ 清故障 → 过早恢复先拒 →
/// 恢复过 → 健康读 1。返回（降级数, 恢复数）。
pub fn golden_run() -> Result<(u32, u32), u32> {
    let mut led = ErrorPathLedger::new();
    // 健康段：读 0/1/2 拿蜂巢值。
    let mut i = 0;
    while i < 3 {
        match led.read_setting(i)? {
            ReadOutcome::Hive(v) => {
                if v != hive_expect_of(i) {
                    return Err(HfCode::ConservationBroken.code());
                }
            }
            ReadOutcome::Fallback(_, _) => return Err(HfCode::TriadIncomplete.code()),
        }
        i += 1;
    }
    // 故障段：不可达 → 读 3/4 全降级且三要素齐。
    let _ = led.inject_fault(FaultKind::Unreachable);
    let mut j = 3;
    while j < 5 {
        match led.read_setting(j)? {
            ReadOutcome::Fallback(v, code) => {
                if v != default_of(j) {
                    return Err(HfCode::TriadIncomplete.code());
                }
                if code != HfCode::HiveUnreachable.code() {
                    return Err(HfCode::TriadIncomplete.code());
                }
                if !led.triad_intact(j) {
                    return Err(HfCode::TriadIncomplete.code());
                }
            }
            ReadOutcome::Hive(_) => return Err(HfCode::TriadIncomplete.code()),
        }
        j += 1;
    }
    if led.degrades != 2 {
        return Err(HfCode::ConservationBroken.code());
    }
    // 过早恢复：故障在档 → 必拒。
    let early = led.probe_recovery().err() == Some(HfCode::PrematureRecovery.code());
    if !early {
        return Err(HfCode::ConservationBroken.code());
    }
    // 清故障 → 恢复过。
    led.clear_fault();
    led.probe_recovery()?;
    if led.recovers != 1 {
        return Err(HfCode::ConservationBroken.code());
    }
    // 健康读回：设置 3 恢复蜂巢值。
    match led.read_setting(3)? {
        ReadOutcome::Hive(v) => {
            if v != hive_expect_of(3) {
                return Err(HfCode::ConservationBroken.code());
            }
        }
        ReadOutcome::Fallback(_, _) => return Err(HfCode::TriadIncomplete.code()),
    }
    led.conserve()?;
    Ok((led.degrades, led.recovers))
}

// ── 注入核：三类坏样本 ×10 轮 ──────────────────────────────────
/// 注入一：不可达降级——降级值=系统默认 + 三要素齐（缺一即漏）。
/// 注入二：记录损坏——校验和失配必须被健康路径检出并降级。
/// 注入三：过早恢复——故障在档宣布恢复必拒。
pub fn inject_rounds(rounds: u32) -> Result<(u32, u32), u32> {
    let mut tries = 0u32;
    let mut caught = 0u32;
    let mut r = 0u32;
    while r < rounds {
        // —— 类一：不可达降级三要素 ——
        tries = tries.saturating_add(1);
        {
            let mut led = ErrorPathLedger::new();
            let _ = led.inject_fault(FaultKind::Unreachable);
            let hit = match led.read_setting(2) {
                Ok(ReadOutcome::Fallback(v, code)) => {
                    v == default_of(2)
                        && code == HfCode::HiveUnreachable.code()
                        && led.triad_intact(2)
                }
                _ => false,
            };
            if hit {
                caught = caught.saturating_add(1);
            }
        }
        // —— 类二：记录损坏检出 ——
        tries = tries.saturating_add(1);
        {
            let mut led = ErrorPathLedger::new();
            let _ = led.corrupt_record(5);
            let hit = match led.read_setting(5) {
                Ok(ReadOutcome::Fallback(v, code)) => {
                    v == default_of(5) && code == HfCode::HiveCorrupt.code()
                }
                _ => false,
            };
            if hit {
                caught = caught.saturating_add(1);
            }
        }
        // —— 类三：过早恢复 ——
        // 故障在档先读一次（状态机进 Degraded），再探恢复必拒：
        // 不先读则状态仍是 Healthy，「state==Degraded」判据不成立。
        tries = tries.saturating_add(1);
        {
            let mut led = ErrorPathLedger::new();
            let _ = led.inject_fault(FaultKind::Timeout);
            let _ = led.read_setting(0);
            let hit = led.probe_recovery().err() == Some(HfCode::PrematureRecovery.code())
                && led.state == HiveState::Degraded;
            if hit {
                caught = caught.saturating_add(1);
            }
        }
        r = r.saturating_add(1);
    }
    if caught != tries {
        return Err(HfCode::ConservationBroken.code());
    }
    Ok((tries, caught))
}

/// 双跑确定性。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(HfCode::NotReproducible.code());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < HfCode::ALL.len() {
        if let Some(c) = HfCode::ALL.get(i) {
            let v = c.code();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x7F000 && SEG_BASE <= 0x7F400
}

/// 四类故障全谱：每类降级一次、通知账逐条码正确（全谱对表）。
pub fn fault_spectrum() -> Result<u32, u32> {
    let mut led = ErrorPathLedger::new();
    let mut k = 0;
    while k < FAULT_N {
        let kind = match FaultKind::ALL.get(k) {
            Some(f) => *f,
            None => return Err(HfCode::SegmentOutside.code()),
        };
        let _ = led.inject_fault(kind);
        match led.read_setting(k)? {
            ReadOutcome::Fallback(v, code) => {
                let expect_code = match kind {
                    FaultKind::Unreachable => HfCode::HiveUnreachable.code(),
                    FaultKind::Corrupt => HfCode::HiveCorrupt.code(),
                    FaultKind::Timeout => HfCode::HiveTimeout.code(),
                    FaultKind::Partial => HfCode::HivePartial.code(),
                };
                if v != default_of(k) || code != expect_code || !led.triad_intact(k) {
                    return Err(HfCode::TriadIncomplete.code());
                }
            }
            ReadOutcome::Hive(_) => return Err(HfCode::TriadIncomplete.code()),
        }
        led.clear_fault();
        k += 1;
    }
    if led.degrades != FAULT_N as u32 {
        return Err(HfCode::ConservationBroken.code());
    }
    led.conserve()?;
    Ok(led.degrades)
}

/// 空蜂巢边界：记录缺失（None）视同损坏降级，不 panic 不误报健康。
pub fn empty_hive_edge() -> Result<(), u32> {
    let mut led = ErrorPathLedger::new();
    if let Some(slot) = led.records.get_mut(7) {
        *slot = None;
    }
    match led.read_setting(7)? {
        ReadOutcome::Fallback(v, code) => {
            if v != default_of(7) || code != HfCode::HiveCorrupt.code() {
                return Err(HfCode::TriadIncomplete.code());
            }
        }
        ReadOutcome::Hive(_) => return Err(HfCode::TriadIncomplete.code()),
    }
    Ok(())
}

/// 未知设置号边界：读/损坏注入双拒。
pub fn unknown_setting_edge() -> Result<(), u32> {
    let mut led = ErrorPathLedger::new();
    let r1 = led.read_setting(SETTING_N).err();
    let r2 = led.corrupt_record(SETTING_N).err();
    if r1 == Some(HfCode::UnknownSetting.code())
        && r2 == Some(HfCode::UnknownSetting.code())
    {
        Ok(())
    } else {
        Err(HfCode::ConservationBroken.code())
    }
}

/// 双重恢复边界：Healthy 态再探针必拒。
pub fn double_recover_edge() -> Result<(), u32> {
    let mut led = ErrorPathLedger::new();
    let _ = led.inject_fault(FaultKind::Partial);
    let _ = led.read_setting(0)?;
    led.clear_fault();
    led.probe_recovery()?;
    let again = led.probe_recovery().err() == Some(HfCode::DoubleRecovery.code());
    if again {
        Ok(())
    } else {
        Err(HfCode::ConservationBroken.code())
    }
}

// ── 判据出口 ───────────────────────────────────────────────────
/// UNX-F18477 自检判据集（≤20 条；≤8 条动态明细）。
pub fn run_unx18477_checks() -> CheckSet {
    let mut s = CheckSet::new("unx18477");
    // 1. 闭集冻结：设置 8 / 故障类 4 / 默认值基。
    s.add(
        "J1-闭集冻结",
        SETTING_N == 8 && FAULT_N == 4 && DEFAULT_BASE == 0x00A0,
        "设置8/故障4/默认基0x00A0",
    );
    // 2. 黄金全链：健康→不可达降级→过早恢复拒→恢复→健康。
    let g = golden_run();
    s.add_dyn("J1-黄金全链", g.is_ok(), &format!("降级/恢复={:?}", g));
    // 3. 降级值=系统默认（J1 核心）。
    {
        let mut led = ErrorPathLedger::new();
        let _ = led.inject_fault(FaultKind::Unreachable);
        let ok = match led.read_setting(6) {
            Ok(ReadOutcome::Fallback(v, _)) => v == default_of(6),
            _ => false,
        };
        s.add("J1-降级到系统默认", ok, "0x00A0+6 = 0x00A6");
    }
    // 4. 三要素呈现：码/位面/通知账逐项。
    {
        let mut led = ErrorPathLedger::new();
        let _ = led.inject_fault(FaultKind::Partial);
        let _ = led.read_setting(1);
        let bit = (led.degraded_map >> 1) & 0x1 == 0x1;
        let noted = match led.notes.get(0) {
            Some(n) => n.setting == 1 && n.hint.len() > 0 && n.code >= SEG_BASE,
            None => false,
        };
        s.add("J1-三要素呈现", bit && noted && led.triad_intact(1), "码+位面+通知账");
    }
    // 5. 注入三类 ×10 轮全抓。
    let inj = inject_rounds(INJECT_ROUNDS);
    let inj_ok = match inj {
        Ok((t, c)) => t > 0 && c == t,
        Err(_) => false,
    };
    s.add_dyn("J1-注入三类10/10", inj_ok, &format!("tries/caught={:?}", inj));
    // 6. 四类故障全谱对表。
    let spec = fault_spectrum();
    s.add(
        "J1-四类故障全谱",
        spec.is_ok(),
        "不可达/损坏/超时/半写各降级一次",
    );
    // 7. 记录损坏检出（单发必红）。
    {
        let mut led = ErrorPathLedger::new();
        let _ = led.corrupt_record(5);
        let caught = match led.read_setting(5) {
            Ok(ReadOutcome::Fallback(_, code)) => code == HfCode::HiveCorrupt.code(),
            _ => false,
        };
        s.add("J1-损坏检出单发", caught, "校验和失配即降级");
    }
    // 8. 过早恢复单发必拒。
    {
        let mut led = ErrorPathLedger::new();
        let _ = led.inject_fault(FaultKind::Unreachable);
        s.add(
            "J1-过早恢复必拒",
            led.probe_recovery().err() == Some(HfCode::PrematureRecovery.code()),
            "故障在档不许宣布恢复",
        );
    }
    // 9. 边界三探：空蜂巢/未知号/双重恢复。
    s.add(
        "J1-边界三探",
        empty_hive_edge().is_ok()
            && unknown_setting_edge().is_ok()
            && double_recover_edge().is_ok(),
        "None记录/越号/再探针三边界",
    );
    // 10. 通知账中文提示非空且逐条码落段窗。
    {
        let mut led = ErrorPathLedger::new();
        let _ = led.inject_fault(FaultKind::Timeout);
        let _ = led.read_setting(0);
        let _ = led.read_setting(1);
        let mut ok = led.notes.len() == 2;
        let mut i = 0;
        while i < led.notes.len() {
            if let Some(n) = led.notes.get(i) {
                if n.hint.len() == 0
                    || n.code < SEG_BASE
                    || n.code >= SEG_BASE + 0x40
                {
                    ok = false;
                }
            }
            i += 1;
        }
        s.add("J1-通知账闭集", ok, "每条降级恰一中文通知");
    }
    // 11. 双跑一致。
    s.add("J2-双跑一致", repro_check().is_ok(), "golden_run 两跑同值");
    // 12. 段位守卫。
    s.add("J3-段位守卫", segment_guard(), "0x7F300 窗口对齐 0x40");
    // 13. 文案闭集。
    {
        let mut ok = true;
        let mut i = 0;
        while i < HfCode::ALL.len() {
            if let Some(c) = HfCode::ALL.get(i) {
                if c.detail().len() == 0 {
                    ok = false;
                }
            }
            i += 1;
        }
        s.add("J4-文案闭集", ok, "11 码中文明细全非空");
    }
    // 14. 计数守恒：降级数=通知数、恢复≤降级。
    {
        let mut led = ErrorPathLedger::new();
        let _ = led.inject_fault(FaultKind::Unreachable);
        let _ = led.read_setting(2);
        let _ = led.read_setting(3);
        led.clear_fault();
        let _ = led.probe_recovery();
        s.add(
            "J1-计数守恒",
            led.conserve().is_ok() && led.degrades == 2 && led.recovers == 1,
            "降级2通知2恢复1账平",
        );
    }
    s
}

// ── 测试 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t01_normal_flow() {
        let mut led = ErrorPathLedger::new();
        let out = match led.read_setting(0) {
            Ok(o) => o,
            Err(_) => ReadOutcome::Fallback(0, 0),
        };
        assert_eq!(out, ReadOutcome::Hive(hive_expect_of(0)));
    }

    #[test]
    fn t02_boundary_unknown_setting() {
        let mut led = ErrorPathLedger::new();
        assert_eq!(
            led.read_setting(SETTING_N).err(),
            Some(HfCode::UnknownSetting.code())
        );
    }

    #[test]
    fn t03_fallback_to_default() {
        let mut led = ErrorPathLedger::new();
        let _ = led.inject_fault(FaultKind::Unreachable);
        let out = match led.read_setting(4) {
            Ok(o) => o,
            Err(_) => ReadOutcome::Hive(0),
        };
        assert_eq!(
            out,
            ReadOutcome::Fallback(default_of(4), HfCode::HiveUnreachable.code())
        );
        assert!(led.triad_intact(4));
    }

    #[test]
    fn t04_inject_corrupt_record() {
        let mut led = ErrorPathLedger::new();
        let _ = led.corrupt_record(5);
        let out = match led.read_setting(5) {
            Ok(o) => o,
            Err(_) => ReadOutcome::Hive(0),
        };
        assert_eq!(
            out,
            ReadOutcome::Fallback(default_of(5), HfCode::HiveCorrupt.code())
        );
    }

    #[test]
    fn t05_inject_premature_recovery() {
        let mut led = ErrorPathLedger::new();
        let _ = led.inject_fault(FaultKind::Timeout);
        assert_eq!(
            led.probe_recovery().err(),
            Some(HfCode::PrematureRecovery.code())
        );
    }

    #[test]
    fn t06_fault_spectrum() {
        assert!(fault_spectrum().is_ok());
    }

    #[test]
    fn t07_edges() {
        assert!(empty_hive_edge().is_ok());
        assert!(unknown_setting_edge().is_ok());
        assert!(double_recover_edge().is_ok());
    }

    #[test]
    fn t08_state_machine_monotone() {
        let mut led = ErrorPathLedger::new();
        assert_eq!(led.state, HiveState::Healthy);
        let _ = led.inject_fault(FaultKind::Partial);
        let _ = led.read_setting(0);
        assert_eq!(led.state, HiveState::Degraded);
        led.clear_fault();
        let _ = led.probe_recovery();
        assert_eq!(led.state, HiveState::Healthy);
    }

    #[test]
    fn t09_double_run_determinism() {
        assert!(repro_check().is_ok());
    }

    #[test]
    fn t10_segment_guard() {
        assert!(segment_guard());
    }

    #[test]
    fn t11_checks_all_green() {
        let set = run_unx18477_checks();
        assert!(set.all_passed(), "UNX-F18477 自检须全绿：{}", set.red_items_text());
    }
}
