//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0452 表缺失降级账（分级路径） · 0x752C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，WBV1104·AI-VX7-1104）：
//!   降级矩阵 5 级（无 XSDT/MADT 缺/HPET 缺/MCFG 缺/根表双缺）路径账齐，
//!   消费方兼容断言 5/5 过。
//! 设计要点：
//!   1. 存在性五元组闭集（XSDT/RSDT/MADT/HPET/MCFG）→ 触发条件闭集
//!      5 路显性映射，优先级 L1→L5 定序，不猜不折中。
//!   2. 降级矩阵 L0~L5 六态冻结：L0 全功能 / L1 无 XSDT→RSDT 32 位
//!      路径 / L2 MADT 缺→单核假设 / L3 HPET 缺→PIT 回退 / L4 MCFG
//!      缺→legacy PCI 枚举 / L5 根表双缺→ACMI 局部降级。
//!   3. 级别单调：只升不降（0→5 单向），倒退/回滚注入显性拒，
//!      史账只追加不抹账。
//!   4. 消费方 5 路闭集（表遍历/核数/定时器/PCI ECAM/ACMI 初始化），
//!      每级每消费方替代路径账齐（6×5 冻结矩阵），兼容断言 5/5 过。
//!   5. 根路径闭集守卫：L1 的替代路径是 RSDT 32 位——若 RSDT 同缺，
//!      该触发显性歧义拒，必须按根表双缺走 L5。
//!   6. 降级史账：事件（seq/触发/级别 from→to）只追加，seq 单调可复算，
//!      重建重放必须与终态一致；双跑一致收口。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x752C0，WBV1104·AI-VX7-1104 F0452 独占窗）。
pub const SEG_BASE: u32 = 0x752C0;

// ── 冻结常量表（来源：ACPI 6.5 §5.2.7/§5.2.8/§5.2.9/§5.2.12/§5.2.14）
/// 降级级别 L0（全功能）。
pub const DEGRADE_L0: u8 = 0;
/// 降级级别 L1（无 XSDT → RSDT 32 位路径）。
pub const DEGRADE_L1: u8 = 1;
/// 降级级别 L2（MADT 缺 → 单核假设）。
pub const DEGRADE_L2: u8 = 2;
/// 降级级别 L3（HPET 缺 → PIT 回退）。
pub const DEGRADE_L3: u8 = 3;
/// 降级级别 L4（MCFG 缺 → legacy PCI 枚举）。
pub const DEGRADE_L4: u8 = 4;
/// 降级级别 L5（根表双缺 → ACMI 局部降级）。
pub const DEGRADE_L5: u8 = 5;
/// 降级级别总数（L0~L5 六态闭集）。
pub const DEGRADE_LEVELS: usize = 6;
/// 消费方总数（表遍历/核数/定时器/PCI ECAM/ACMI 初始化，5 路闭集）。
pub const CONSUMER_COUNT: usize = 5;
/// 降级史账容量上限（本仓冻结 64，只追加不抹账）。
pub const MAX_HISTORY: usize = 64;
/// 触发条件路数（无 XSDT/MADT 缺/HPET 缺/MCFG 缺/根表双缺，5 路闭集）。
pub const TRIGGER_KINDS: usize = 5;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DegCode {
    /// 触发条件不在闭集（5 路之外）
    TriggerUnknown,
    /// 降级倒退（级别只升不降，回滚注入）
    LevelRegress,
    /// 抹账注入（史账不许清空/回滚）
    HistoryErase,
    /// 消费方兼容断言失败（级别越界或替代路径缺）
    CompatBroken,
    /// 替代路径缺失（文案账空）
    SubPathMissing,
    /// 根路径歧义（无 XSDT 但 RSDT 同缺，须走根表双缺）
    RootPairAmbiguous,
    /// 史账满（64 事件上限）
    HistoryFull,
    /// 史账复算失配（重放终态与账面不符）
    ReplayMismatch,
    /// 双跑不一致（复现红线）
    NotReproducible,
}

impl DegCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            DegCode::TriggerUnknown => "触发条件不在闭集：只认无 XSDT/MADT 缺/HPET 缺/MCFG 缺/根表双缺五路，建议核对存在性五元组账",
            DegCode::LevelRegress => "降级倒退：级别只升不降（0→5 单向），建议按单调矩阵重排触发序，不许回滚",
            DegCode::HistoryErase => "抹账注入被拒：降级史账只追加不抹账，建议保留全事件序并按 seq 复算",
            DegCode::CompatBroken => "消费方兼容断言失败：该级别下存在无替代路径的消费方，建议核对 6×5 替代路径矩阵",
            DegCode::SubPathMissing => "替代路径缺失：替代路径文案账为空，建议补齐对应级别的替代路径条目",
            DegCode::RootPairAmbiguous => "根路径歧义：无 XSDT 但 RSDT 同缺，L1 的 32 位替代路径不可用，必须按根表双缺走 L5",
            DegCode::HistoryFull => "降级史账满（64）：建议核对触发序是否重复或提升冻结上限",
            DegCode::ReplayMismatch => "史账复算失配：按事件序重放的终态与账面级别不符，建议核对 seq 单调性与 from/to 账",
            DegCode::NotReproducible => "双跑不一致：同一触发序两次降级结果不同，复现红线，建议排查全局可变态",
        }
    }
    pub const ALL: [DegCode; 9] = [
        DegCode::TriggerUnknown,
        DegCode::LevelRegress,
        DegCode::HistoryErase,
        DegCode::CompatBroken,
        DegCode::SubPathMissing,
        DegCode::RootPairAmbiguous,
        DegCode::HistoryFull,
        DegCode::ReplayMismatch,
        DegCode::NotReproducible,
    ];
}

/// 诊断码闭集查表（段内窗口外一律未知码）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < DegCode::ALL.len() {
        if let Some(c) = DegCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x752C0 段闭集内"
}

// ── 存在性五元组（表在账） ──────────────────────────────────────
/// 表存在性五元组：XSDT/RSDT/MADT/HPET/MCFG（true = 表在）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TablePresence {
    /// XSDT（根表，64 位指针代）
    pub xsdt: bool,
    /// RSDT（根表，32 位指针代）
    pub rsdt: bool,
    /// MADT（多核 APIC 表）
    pub madt: bool,
    /// HPET（高精度定时器表）
    pub hpet: bool,
    /// MCFG（PCIe ECAM 表）
    pub mcfg: bool,
}

impl TablePresence {
    /// 根表至少一根在（L1 替代路径的先决条件）。
    pub fn root_ok(&self) -> bool {
        self.rsdt || self.xsdt
    }
}

// ── 触发条件闭集（5 路） ────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DegradeTrigger {
    /// 无 XSDT（RSDT 在）→ RSDT 32 位路径
    XsdtMissing,
    /// MADT 缺 → 单核假设
    MadtMissing,
    /// HPET 缺 → PIT 回退
    HpetMissing,
    /// MCFG 缺 → legacy PCI 枚举
    McfgMissing,
    /// 根表双缺（RSDT 与 XSDT 均缺）→ ACMI 局部降级
    RootBothMissing,
}

impl DegradeTrigger {
    pub fn label(self) -> &'static str {
        match self {
            DegradeTrigger::XsdtMissing => "无 XSDT（RSDT 在）",
            DegradeTrigger::MadtMissing => "MADT 缺",
            DegradeTrigger::HpetMissing => "HPET 缺",
            DegradeTrigger::McfgMissing => "MCFG 缺",
            DegradeTrigger::RootBothMissing => "根表双缺",
        }
    }
    /// 触发 → 目标级别的冻结映射（L1~L5 一一对应）。
    pub fn level_of(self) -> u8 {
        match self {
            DegradeTrigger::XsdtMissing => DEGRADE_L1,
            DegradeTrigger::MadtMissing => DEGRADE_L2,
            DegradeTrigger::HpetMissing => DEGRADE_L3,
            DegradeTrigger::McfgMissing => DEGRADE_L4,
            DegradeTrigger::RootBothMissing => DEGRADE_L5,
        }
    }
    pub const ALL: [DegradeTrigger; TRIGGER_KINDS] = [
        DegradeTrigger::XsdtMissing,
        DegradeTrigger::MadtMissing,
        DegradeTrigger::HpetMissing,
        DegradeTrigger::McfgMissing,
        DegradeTrigger::RootBothMissing,
    ];
}

/// 触发求值：按 L1→L5 优先级扫描存在性五元组，命中首路；
/// 五路全不命中即全功能（None）。
pub fn evaluate(p: &TablePresence) -> Option<DegradeTrigger> {
    if !p.xsdt && p.rsdt {
        return Some(DegradeTrigger::XsdtMissing);
    }
    if !p.madt {
        return Some(DegradeTrigger::MadtMissing);
    }
    if !p.hpet {
        return Some(DegradeTrigger::HpetMissing);
    }
    if !p.mcfg {
        return Some(DegradeTrigger::McfgMissing);
    }
    if !p.rsdt && !p.xsdt {
        return Some(DegradeTrigger::RootBothMissing);
    }
    None
}

// ── 降级级别闭集（L0~L5 六态） ──────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DegradeLevel {
    /// L0 全功能
    L0Full,
    /// L1 无 XSDT → RSDT 32 位路径
    L1NoXsdt,
    /// L2 MADT 缺 → 单核假设
    L2NoMadt,
    /// L3 HPET 缺 → PIT 回退
    L3NoHpet,
    /// L4 MCFG 缺 → legacy PCI 枚举
    L4NoMcfg,
    /// L5 根表双缺 → ACMI 局部降级
    L5AcmiLocal,
}

impl DegradeLevel {
    /// 级别序数（0~5，单调口径）。
    pub fn rank(self) -> u8 {
        self as u8
    }
    pub fn label(self) -> &'static str {
        match self {
            DegradeLevel::L0Full => "L0 全功能",
            DegradeLevel::L1NoXsdt => "L1 无 XSDT→RSDT 32 位路径",
            DegradeLevel::L2NoMadt => "L2 MADT 缺→单核假设",
            DegradeLevel::L3NoHpet => "L3 HPET 缺→PIT 回退",
            DegradeLevel::L4NoMcfg => "L4 MCFG 缺→legacy PCI 枚举",
            DegradeLevel::L5AcmiLocal => "L5 根表双缺→ACMI 局部降级",
        }
    }
    /// 序数 → 级别（6 以上越界 None）。
    pub fn from_rank(r: u8) -> Option<DegradeLevel> {
        match r {
            DEGRADE_L0 => Some(DegradeLevel::L0Full),
            DEGRADE_L1 => Some(DegradeLevel::L1NoXsdt),
            DEGRADE_L2 => Some(DegradeLevel::L2NoMadt),
            DEGRADE_L3 => Some(DegradeLevel::L3NoHpet),
            DEGRADE_L4 => Some(DegradeLevel::L4NoMcfg),
            DEGRADE_L5 => Some(DegradeLevel::L5AcmiLocal),
            _ => None,
        }
    }
    pub const ALL: [DegradeLevel; DEGRADE_LEVELS] = [
        DegradeLevel::L0Full,
        DegradeLevel::L1NoXsdt,
        DegradeLevel::L2NoMadt,
        DegradeLevel::L3NoHpet,
        DegradeLevel::L4NoMcfg,
        DegradeLevel::L5AcmiLocal,
    ];
}

// ── 消费方闭集（5 路）与替代路径矩阵（6×5 冻结） ────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Consumer {
    /// 表遍历（根表清单消费方）
    TableWalk,
    /// 核数（MADT 消费方）
    CoreCount,
    /// 定时器（HPET 消费方）
    Timer,
    /// PCI ECAM（MCFG 消费方）
    PciEcam,
    /// ACMI 初始化（根表总消费方）
    AcmiInit,
}

impl Consumer {
    pub fn label(self) -> &'static str {
        match self {
            Consumer::TableWalk => "表遍历",
            Consumer::CoreCount => "核数",
            Consumer::Timer => "定时器",
            Consumer::PciEcam => "PCI ECAM",
            Consumer::AcmiInit => "ACMI 初始化",
        }
    }
    pub const ALL: [Consumer; CONSUMER_COUNT] = [
        Consumer::TableWalk,
        Consumer::CoreCount,
        Consumer::Timer,
        Consumer::PciEcam,
        Consumer::AcmiInit,
    ];
}

/// 替代路径矩阵（级别 × 消费方，6×5 冻结账）：每格为该级别下该消费方
/// 的替代路径中文账注；越界级别一律空串（兼容断言抓）。
pub fn substitute_path(level: u8, consumer: Consumer) -> &'static str {
    match (level, consumer) {
        (DEGRADE_L0, Consumer::TableWalk) => "XSDT 直读根表清单",
        (DEGRADE_L0, Consumer::CoreCount) => "MADT LAPIC 条目计数",
        (DEGRADE_L0, Consumer::Timer) => "HPET 主定时器",
        (DEGRADE_L0, Consumer::PciEcam) => "MCFG ECAM 配置空间",
        (DEGRADE_L0, Consumer::AcmiInit) => "根表全量初始化",
        (DEGRADE_L1, Consumer::TableWalk) => "RSDT 32 位指针代根表清单",
        (DEGRADE_L1, Consumer::CoreCount) => "MADT LAPIC 条目计数",
        (DEGRADE_L1, Consumer::Timer) => "HPET 主定时器",
        (DEGRADE_L1, Consumer::PciEcam) => "MCFG ECAM 配置空间",
        (DEGRADE_L1, Consumer::AcmiInit) => "RSDT 路径局部初始化",
        (DEGRADE_L2, Consumer::TableWalk) => "XSDT 直读根表清单",
        (DEGRADE_L2, Consumer::CoreCount) => "单核假设（bootstrap 1 核）",
        (DEGRADE_L2, Consumer::Timer) => "HPET 主定时器",
        (DEGRADE_L2, Consumer::PciEcam) => "MCFG ECAM 配置空间",
        (DEGRADE_L2, Consumer::AcmiInit) => "无 MADT 局部初始化",
        (DEGRADE_L3, Consumer::TableWalk) => "XSDT 直读根表清单",
        (DEGRADE_L3, Consumer::CoreCount) => "MADT LAPIC 条目计数",
        (DEGRADE_L3, Consumer::Timer) => "PIT 8253/8254 回退",
        (DEGRADE_L3, Consumer::PciEcam) => "MCFG ECAM 配置空间",
        (DEGRADE_L3, Consumer::AcmiInit) => "无 HPET 局部初始化",
        (DEGRADE_L4, Consumer::TableWalk) => "XSDT 直读根表清单",
        (DEGRADE_L4, Consumer::CoreCount) => "MADT LAPIC 条目计数",
        (DEGRADE_L4, Consumer::Timer) => "HPET 主定时器",
        (DEGRADE_L4, Consumer::PciEcam) => "legacy PCI 枚举（CF8/CFC 端口）",
        (DEGRADE_L4, Consumer::AcmiInit) => "无 MCFG 局部初始化",
        (DEGRADE_L5, Consumer::TableWalk) => "ACMI 内嵌最小根表",
        (DEGRADE_L5, Consumer::CoreCount) => "单核假设（bootstrap 1 核）",
        (DEGRADE_L5, Consumer::Timer) => "PIT 8253/8254 回退",
        (DEGRADE_L5, Consumer::PciEcam) => "legacy PCI 枚举（CF8/CFC 端口）",
        (DEGRADE_L5, Consumer::AcmiInit) => "ACMI 局部降级初始化",
        _ => "",
    }
}

/// 兼容断言：该级别下该消费方有非空替代路径即为兼容。
pub fn compat_ok(level: u8, consumer: Consumer) -> bool {
    !substitute_path(level, consumer).is_empty()
}

/// 兼容断言 5/5：该级别下全部 5 路消费方均有替代路径。
pub fn compat_count(level: u8) -> usize {
    let mut n = 0;
    let mut i = 0;
    while i < Consumer::ALL.len() {
        if let Some(c) = Consumer::ALL.get(i) {
            if compat_ok(level, *c) {
                n += 1;
            }
        }
        i += 1;
    }
    n
}

// ── 降级史账（事件只追加不抹账） ────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DegradeEvent {
    /// 事件序号（0 起单调）
    pub seq: u32,
    /// 触发条件（闭集 5 路）
    pub trigger: DegradeTrigger,
    /// 触发前级别
    pub from: u8,
    /// 触发后级别
    pub to: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DegradeEngine {
    /// 当前级别（0~5，只升不降）
    pub level: u8,
    /// 降级史账（只追加）
    pub history: Vec<DegradeEvent>,
}

impl DegradeEngine {
    pub fn new() -> DegradeEngine {
        DegradeEngine { level: DEGRADE_L0, history: Vec::new() }
    }

    /// 观察存在性五元组：求值触发并应用；全在即保持 L0。
    pub fn observe(&mut self, p: &TablePresence) -> Result<u8, u32> {
        match evaluate(p) {
            Some(t) => self.apply_trigger(t, p),
            None => Ok(self.level),
        }
    }

    /// 应用触发（根路径闭集守卫 + 单调闸 + 史账只追加）。
    pub fn apply_trigger(&mut self, t: DegradeTrigger, p: &TablePresence) -> Result<u8, u32> {
        // 根路径闭集守卫：L1 的替代路径是 RSDT，RSDT 同缺即歧义拒。
        if t == DegradeTrigger::XsdtMissing && !p.rsdt {
            return Err(DegCode::RootPairAmbiguous.val());
        }
        let target = t.level_of();
        // 单调闸：倒退显性拒。
        if target < self.level {
            return Err(DegCode::LevelRegress.val());
        }
        // 史账容量闸。
        if self.history.len() >= MAX_HISTORY {
            return Err(DegCode::HistoryFull.val());
        }
        let ev = DegradeEvent {
            seq: self.history.len() as u32,
            trigger: t,
            from: self.level,
            to: target,
        };
        self.history.push(ev);
        self.level = target;
        Ok(self.level)
    }

    /// 回滚/抹账注入口：任何形式的降级回滚都显性拒（级别不动、史账不动）。
    pub fn rollback_attempt(&mut self) -> Result<(), u32> {
        Err(DegCode::LevelRegress.val())
    }

    /// 史账复算：按事件序重放，seq 单调、from 与重放级别一致、
    /// 终态与账面级别一致。
    pub fn history_replay_ok(&self) -> bool {
        let mut cur = DEGRADE_L0;
        let mut i = 0;
        while i < self.history.len() {
            let ev = match self.history.get(i) {
                Some(e) => *e,
                None => return false,
            };
            if ev.seq != i as u32 {
                return false;
            }
            if ev.from != cur {
                return false;
            }
            if ev.to < ev.from {
                return false;
            }
            cur = ev.to;
            i += 1;
        }
        cur == self.level
    }
}

// ── mock 存在性语料（六态机） ───────────────────────────────────
/// 全功能机：五表全在。
pub fn presence_full() -> TablePresence {
    TablePresence { xsdt: true, rsdt: true, madt: true, hpet: true, mcfg: true }
}

/// L1 机：XSDT 缺（RSDT 在）。
pub fn presence_l1() -> TablePresence {
    TablePresence { xsdt: false, rsdt: true, madt: true, hpet: true, mcfg: true }
}

/// L2 机：MADT 缺（根表全在）。
pub fn presence_l2() -> TablePresence {
    TablePresence { xsdt: true, rsdt: true, madt: false, hpet: true, mcfg: true }
}

/// L3 机：HPET 缺（根表全在）。
pub fn presence_l3() -> TablePresence {
    TablePresence { xsdt: true, rsdt: true, madt: true, hpet: false, mcfg: true }
}

/// L4 机：MCFG 缺（根表全在）。
pub fn presence_l4() -> TablePresence {
    TablePresence { xsdt: true, rsdt: true, madt: true, hpet: true, mcfg: false }
}

/// L5 机：根表双缺（RSDT 与 XSDT 均缺）。
pub fn presence_l5() -> TablePresence {
    TablePresence { xsdt: false, rsdt: false, madt: true, hpet: true, mcfg: true }
}

/// 黄金全链：L0 → 五路触发逐级升到 L5，史账 5 事件可复算。
pub fn golden_run() -> Result<u8, u32> {
    let mut e = DegradeEngine::new();
    let l0 = e.observe(&presence_full())?;
    if l0 != DEGRADE_L0 {
        return Err(DegCode::NotReproducible.val());
    }
    let t1 = e.apply_trigger(DegradeTrigger::XsdtMissing, &presence_l1())?;
    let t2 = e.apply_trigger(DegradeTrigger::MadtMissing, &presence_full())?;
    let t3 = e.apply_trigger(DegradeTrigger::HpetMissing, &presence_full())?;
    let t4 = e.apply_trigger(DegradeTrigger::McfgMissing, &presence_full())?;
    let t5 = e.apply_trigger(DegradeTrigger::RootBothMissing, &presence_full())?;
    if t1 != DEGRADE_L1 || t2 != DEGRADE_L2 || t3 != DEGRADE_L3 || t4 != DEGRADE_L4 || t5 != DEGRADE_L5 {
        return Err(DegCode::NotReproducible.val());
    }
    if e.history.len() != TRIGGER_KINDS {
        return Err(DegCode::ReplayMismatch.val());
    }
    if !e.history_replay_ok() {
        return Err(DegCode::ReplayMismatch.val());
    }
    Ok(e.level)
}

/// 六态机逐级单发：每级独立引擎 observe 存在性语料，级别一一对应。
pub fn six_state_ok() -> bool {
    let mut i = 0;
    let presences = [
        presence_full(),
        presence_l1(),
        presence_l2(),
        presence_l3(),
        presence_l4(),
        presence_l5(),
    ];
    while i < presences.len() {
        let p = match presences.get(i) {
            Some(p) => *p,
            None => return false,
        };
        let mut e = DegradeEngine::new();
        let lvl = match e.observe(&p) {
            Ok(l) => l,
            Err(_) => return false,
        };
        if lvl != i as u8 {
            return false;
        }
        if !e.history_replay_ok() {
            return false;
        }
        i += 1;
    }
    true
}

/// 复算：同一触发序重建两次，终态与史账全等。
pub fn replay_ok() -> Result<(), u32> {
    let mut a = DegradeEngine::new();
    let mut b = DegradeEngine::new();
    let seq = [
        (DegradeTrigger::XsdtMissing, presence_l1()),
        (DegradeTrigger::MadtMissing, presence_full()),
        (DegradeTrigger::HpetMissing, presence_full()),
        (DegradeTrigger::McfgMissing, presence_full()),
        (DegradeTrigger::RootBothMissing, presence_full()),
    ];
    let mut i = 0;
    while i < seq.len() {
        let (t, p) = match seq.get(i) {
            Some(tp) => *tp,
            None => return Err(DegCode::NotReproducible.val()),
        };
        a.apply_trigger(t, &p)?;
        b.apply_trigger(t, &p)?;
        i += 1;
    }
    if a != b || a.level != DEGRADE_L5 {
        return Err(DegCode::ReplayMismatch.val());
    }
    Ok(())
}

/// 双跑一致（复现红线）。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(DegCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫：SEG_BASE 64 对齐、落在 0x75000~0x781C0、闭集码全在窗内。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    let mut i = 0;
    while i < DegCode::ALL.len() {
        if let Some(c) = DegCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 注入四路 ────────────────────────────────────────────────────
/// 注入 1：倒退注入——L3 状态下再发 L1 级触发，必拒且级别不动。
pub fn inject_level_regress() -> Result<(), u32> {
    let mut e = DegradeEngine::new();
    let l3 = e.apply_trigger(DegradeTrigger::HpetMissing, &presence_full())?;
    if l3 != DEGRADE_L3 {
        return Err(DegCode::NotReproducible.val());
    }
    match e.apply_trigger(DegradeTrigger::XsdtMissing, &presence_l1()) {
        Err(c) if c == DegCode::LevelRegress.val() => {
            if e.level != DEGRADE_L3 || e.history.len() != 1 {
                return Err(DegCode::NotReproducible.val());
            }
            Ok(())
        }
        _ => Err(DegCode::NotReproducible.val()),
    }
}

/// 注入 2：抹账注入——回滚口显性拒，史账长度不变。
pub fn inject_history_erase() -> Result<(), u32> {
    let mut e = DegradeEngine::new();
    let _ = e.apply_trigger(DegradeTrigger::MadtMissing, &presence_full())?;
    let n_before = e.history.len();
    match e.rollback_attempt() {
        Err(c) if c == DegCode::LevelRegress.val() => {
            if e.history.len() != n_before || e.level != DEGRADE_L2 {
                return Err(DegCode::NotReproducible.val());
            }
            Ok(())
        }
        _ => Err(DegCode::NotReproducible.val()),
    }
}

/// 注入 3：根路径歧义——发无 XSDT 触发但 RSDT 同缺，必拒。
pub fn inject_root_ambiguous() -> Result<(), u32> {
    let mut e = DegradeEngine::new();
    let both_missing = TablePresence { xsdt: false, rsdt: false, madt: true, hpet: true, mcfg: true };
    match e.apply_trigger(DegradeTrigger::XsdtMissing, &both_missing) {
        Err(c) if c == DegCode::RootPairAmbiguous.val() => {
            if e.level != DEGRADE_L0 || !e.history.is_empty() {
                return Err(DegCode::NotReproducible.val());
            }
            Ok(())
        }
        _ => Err(DegCode::NotReproducible.val()),
    }
}

/// 注入 4：非法级别兼容断言——level 6（越界）替代路径空，必抓。
pub fn inject_invalid_level() -> Result<(), u32> {
    if compat_ok(DEGRADE_L5 + 1, Consumer::TableWalk) {
        return Err(DegCode::NotReproducible.val());
    }
    if !substitute_path(DEGRADE_L5 + 1, Consumer::TableWalk).is_empty() {
        return Err(DegCode::NotReproducible.val());
    }
    Ok(())
}

/// 史账只追加口径：事件数=触发数，seq 单调，触发序与事件序一致。
fn history_append_only_ok() -> bool {
    let mut e = DegradeEngine::new();
    let seq = [
        (DegradeTrigger::XsdtMissing, presence_l1()),
        (DegradeTrigger::MadtMissing, presence_full()),
        (DegradeTrigger::HpetMissing, presence_full()),
        (DegradeTrigger::McfgMissing, presence_full()),
        (DegradeTrigger::RootBothMissing, presence_full()),
    ];
    let mut i = 0;
    while i < seq.len() {
        let (t, p) = match seq.get(i) {
            Some(tp) => *tp,
            None => return false,
        };
        if e.apply_trigger(t, &p).is_err() {
            return false;
        }
        i += 1;
    }
    if e.history.len() != TRIGGER_KINDS {
        return false;
    }
    let mut j = 0;
    while j < e.history.len() {
        let ev = match e.history.get(j) {
            Some(e) => *e,
            None => return false,
        };
        let (t, _) = match seq.get(j) {
            Some(tp) => *tp,
            None => return false,
        };
        if ev.seq != j as u32 || ev.trigger != t || ev.to != ev.from + 1 {
            return false;
        }
        j += 1;
    }
    e.history_replay_ok()
}

fn compat_matrix_ok() -> bool {
    // L0~L5 每级 5/5，且替代路径文案账 30 格非空。
    let mut lvl = DEGRADE_L0;
    while lvl <= DEGRADE_L5 {
        if compat_count(lvl) != CONSUMER_COUNT {
            return false;
        }
        let mut k = 0;
        while k < Consumer::ALL.len() {
            if let Some(c) = Consumer::ALL.get(k) {
                if substitute_path(lvl, *c).is_empty() {
                    return false;
                }
            }
            k += 1;
        }
        lvl += 1;
    }
    true
}

fn trigger_map_ok() -> bool {
    // 触发→级别冻结映射 + 存在性求值五路全中。
    let mut i = 0;
    while i < DegradeTrigger::ALL.len() {
        if let Some(t) = DegradeTrigger::ALL.get(i) {
            if t.level_of() != (i + 1) as u8 {
                return false;
            }
        }
        i += 1;
    }
    evaluate(&presence_l1()) == Some(DegradeTrigger::XsdtMissing)
        && evaluate(&presence_l2()) == Some(DegradeTrigger::MadtMissing)
        && evaluate(&presence_l3()) == Some(DegradeTrigger::HpetMissing)
        && evaluate(&presence_l4()) == Some(DegradeTrigger::McfgMissing)
        && evaluate(&presence_l5()) == Some(DegradeTrigger::RootBothMissing)
        && evaluate(&presence_full()).is_none()
}

// ── 检查节 ──────────────────────────────────────────────────────
pub fn run_unx0452_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0452 表缺失降级账（分级路径）");
    let chain_txt = match golden_run() {
        Ok(l) => match DegradeLevel::from_rank(l) {
            Some(d) => format!("终态={} 史账 5 事件复算过", d.label()),
            None => String::from("级别越界"),
        },
        Err(_) => String::from("golden_run 红"),
    };
    s.add_dyn("F0452-黄金全链 L0→L5", golden_run().is_ok(), &chain_txt);
    s.add_dyn(
        "F0452-六态机逐级单发",
        six_state_ok(),
        "L0~L5 存在性语料 → 级别一一对应",
    );
    s.add_dyn(
        "F0452-触发闭集5路映射",
        trigger_map_ok(),
        "无 XSDT/MADT 缺/HPET 缺/MCFG 缺/根表双缺 → L1~L5 冻结映射",
    );
    s.add_dyn(
        "F0452-兼容断言5/5",
        compat_matrix_ok(),
        "L0~L5 每级 5 路消费方替代路径齐（6×5 矩阵 30 格非空）",
    );
    s.add_dyn(
        "F0452-注入：倒退必抓",
        inject_level_regress().is_ok(),
        "L3 上发 L1 级触发显性拒，级别与史账不动",
    );
    s.add_dyn(
        "F0452-注入：抹账必抓",
        inject_history_erase().is_ok(),
        "回滚口显性拒，史账只追加不抹账",
    );
    s.add_dyn(
        "F0452-注入：根路径歧义必抓",
        inject_root_ambiguous().is_ok(),
        "无 XSDT 且 RSDT 同缺 → RootPairAmbiguous，须走 L5",
    );
    s.add_dyn(
        "F0452-注入：非法级别必抓",
        inject_invalid_level().is_ok(),
        "level 6 越界替代路径空，兼容断言抓 CompatBroken 口径",
    );
    s.add_dyn(
        "F0452-史账只追加",
        history_append_only_ok(),
        "事件数=触发数，seq 单调，from→to 逐级 +1，重放一致",
    );
    s.add_dyn("F0452-复算一致", replay_ok().is_ok(), "同触发序重建两次终态与史账全等");
    s.add_dyn(
        "F0452-双跑一致",
        repro_check().is_ok(),
        "golden_run 两跑同值（复现红线）",
    );
    s.add_dyn(
        "F0452-段位守卫",
        segment_guard(),
        "0x752C0 窗口：64 对齐 + 闭集码全在窗内",
    );
    s.add_dyn(
        "F0452-诊断码闭集查表",
        detail(DegCode::RootPairAmbiguous.val()) == DegCode::RootPairAmbiguous.detail()
            && detail(SEG_BASE + 0x3F).starts_with("未知诊断码"),
        "段内命中 + 窗外未知码",
    );
    s.add_dyn(
        "F0452-冻结常量口径",
        DEGRADE_LEVELS == 6
            && CONSUMER_COUNT == 5
            && TRIGGER_KINDS == 5
            && MAX_HISTORY == 64
            && DegradeLevel::ALL.len() == DEGRADE_LEVELS,
        "六态闭集 + 5 路消费方 + 5 路触发 + 史账上限 64",
    );
    s
}

// ── 测试区 ──────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_golden_chain_l0_to_l5() {
        assert_eq!(golden_run(), Ok(DEGRADE_L5));
    }

    #[test]
    fn test_six_state_matrix() {
        assert!(six_state_ok());
        // 每级存在性语料求值精确。
        assert_eq!(evaluate(&presence_full()), None);
        assert_eq!(evaluate(&presence_l1()), Some(DegradeTrigger::XsdtMissing));
        assert_eq!(evaluate(&presence_l5()), Some(DegradeTrigger::RootBothMissing));
    }

    #[test]
    fn test_trigger_level_map() {
        assert!(trigger_map_ok());
        assert_eq!(DegradeTrigger::XsdtMissing.level_of(), DEGRADE_L1);
        assert_eq!(DegradeTrigger::RootBothMissing.level_of(), DEGRADE_L5);
        assert_eq!(DegradeTrigger::ALL.len(), 5);
    }

    #[test]
    fn test_compat_five_of_five() {
        assert!(compat_matrix_ok());
        let mut lvl = DEGRADE_L0;
        while lvl <= DEGRADE_L5 {
            assert_eq!(compat_count(lvl), 5);
            lvl += 1;
        }
        assert_eq!(compat_count(DEGRADE_L5 + 1), 0);
    }

    #[test]
    fn test_inject_regress() {
        assert!(inject_level_regress().is_ok());
    }

    #[test]
    fn test_inject_erase_and_ambiguous() {
        assert!(inject_history_erase().is_ok());
        assert!(inject_root_ambiguous().is_ok());
        assert!(inject_invalid_level().is_ok());
    }

    #[test]
    fn test_history_append_only() {
        assert!(history_append_only_ok());
        assert!(replay_ok().is_ok());
    }

    #[test]
    fn test_repro() {
        assert!(repro_check().is_ok());
    }

    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn test_diag_codes_closed_set() {
        let mut i = 0;
        while i < DegCode::ALL.len() {
            if let Some(c) = DegCode::ALL.get(i) {
                assert!(c.val() >= SEG_BASE && c.val() < SEG_BASE + 0x40);
                assert!(!c.detail().is_empty());
            }
            i += 1;
        }
        assert_eq!(DegCode::ALL.len(), 9);
        assert!(detail(0).starts_with("未知诊断码"));
    }

    #[test]
    fn test_level_enum_roundtrip() {
        let mut i = 0;
        while i <= DEGRADE_L5 {
            assert!(DegradeLevel::from_rank(i).is_some());
            if let Some(d) = DegradeLevel::from_rank(i) {
                assert_eq!(d.rank(), i);
                assert!(!d.label().is_empty());
            }
            i += 1;
        }
        assert!(DegradeLevel::from_rank(6).is_none());
    }

    #[test]
    fn test_double_run_checks() {
        let a = run_unx0452_checks();
        let b = run_unx0452_checks();
        assert!(a.all_passed());
        assert!(b.all_passed());
        assert_eq!(a.len(), b.len());
        assert_eq!(a.len(), 14);
    }
}
