//! unxt122_stagewd —— UNX-F0122 阶段看门狗核心（十二阶段 × 独立时限）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0122 · 阶段看门狗（每阶段独立时限） · 判据：注入阶段卡死被看门狗捕获，超时时限精度毫秒级，捕获后进诊断模式
//! · 组件：十二引导阶段表 + 每阶段时限表 + 看门狗状态机 + 喂狗账
//! · 判据落地：每阶段独立时限的阶段语义底座
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T122_VERSION: &str = "t122-stagewd-v1";

/// 诊断段基址。
pub const T122_SEG_BASE: u16 = 0x4b00;

// --------------------------------------------------------------------------
// 诊断码（t122 十六码 · 14 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 阶段看门狗诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t122Code {
    /// 0x4b00 阶段越界（12 阶段之外）。
    StageOob,
    /// 0x4b01 时限非法（≤0 或超窗）。
    DeadlineBad,
    /// 0x4b02 喂狗丢失（阶段内零喂狗）。
    PetLost,
    /// 0x4b03 阶段跳过（未喂即进下阶段）。
    StageSkipped,
    /// 0x4b04 看门狗未布防。
    WdogNotArmed,
    /// 0x4b05 阶段账破缺。
    LedgerBreak,
    /// 0x4b06 阶段序违例。
    OrderViolated,
    /// 0x4b07 守卫触发。
    GuardTripped,
    /// 0x4b08 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x4b09 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x4b0a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x4b0b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x4b0c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x4b0d 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t122Code {
    /// 全量闭集。
    pub const ALL: [t122Code; 14] = [
        t122Code::StageOob,
        t122Code::DeadlineBad,
        t122Code::PetLost,
        t122Code::StageSkipped,
        t122Code::WdogNotArmed,
        t122Code::LedgerBreak,
        t122Code::OrderViolated,
        t122Code::GuardTripped,
        t122Code::LedgerBroken,
        t122Code::SnapshotFingerprint,
        t122Code::TableBroken,
        t122Code::IndexMisaligned,
        t122Code::DuplicateEntry,
        t122Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t122Code::StageOob => 0x4b00,
            t122Code::DeadlineBad => 0x4b01,
            t122Code::PetLost => 0x4b02,
            t122Code::StageSkipped => 0x4b03,
            t122Code::WdogNotArmed => 0x4b04,
            t122Code::LedgerBreak => 0x4b05,
            t122Code::OrderViolated => 0x4b06,
            t122Code::GuardTripped => 0x4b07,
            t122Code::LedgerBroken => 0x4b08,
            t122Code::SnapshotFingerprint => 0x4b09,
            t122Code::TableBroken => 0x4b0a,
            t122Code::IndexMisaligned => 0x4b0b,
            t122Code::DuplicateEntry => 0x4b0c,
            t122Code::SegmentGuard => 0x4b0d,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t122Code::StageOob => "阶段越界（12 阶段之外）",
            t122Code::DeadlineBad => "时限非法（≤0 或超窗）",
            t122Code::PetLost => "喂狗丢失（阶段内零喂狗）",
            t122Code::StageSkipped => "阶段跳过（未喂即进下阶段）",
            t122Code::WdogNotArmed => "看门狗未布防",
            t122Code::LedgerBreak => "阶段账破缺",
            t122Code::OrderViolated => "阶段序违例",
            t122Code::GuardTripped => "守卫触发",
            t122Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t122Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t122Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t122Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t122Code::DuplicateEntry => "条目重复（同键双写）",
            t122Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t122Code::StageOob => "按十二阶段闭集校正",
            t122Code::DeadlineBad => "按时限表校正",
            t122Code::PetLost => "按喂狗协议补喂",
            t122Code::StageSkipped => "按阶段序补齐",
            t122Code::WdogNotArmed => "按布防规程布防",
            t122Code::LedgerBreak => "按守恒重建",
            t122Code::OrderViolated => "按依赖序重排",
            t122Code::GuardTripped => "检查临界区",
            t122Code::LedgerBroken => "按守恒律重建台账后重跑",
            t122Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t122Code::TableBroken => "按语义规则重建常量表",
            t122Code::IndexMisaligned => "按声明序归一化后重跑",
            t122Code::DuplicateEntry => "合并同键条目为末次生效",
            t122Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 stage（12 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 引导十二阶段表（阶段序 + 中文语义逐行守卫）。
/// 行数。
pub const stage_COUNT: usize = 12;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static stage: [(&'static str, u64); 12] = [
    ("ST_FIRMWARE", 0), // 固件阶段（BIOS/UEFI 交接）（时限 2000 ms）
    ("ST_MEMORY", 1), // 内存阶段（E820 盘点+页表）（时限 1500 ms）
    ("ST_ACPI", 2), // ACPI 阶段（表解析）（时限 800 ms）
    ("ST_PAGING", 3), // 分页阶段（四级表构建）（时限 1200 ms）
    ("ST_CPU", 4), // CPU 阶段（特性枚举）（时限 600 ms）
    ("ST_IDT", 5), // 中断阶段（IDT 装载）（时限 500 ms）
    ("ST_TIMER", 6), // 时钟阶段（PIT/LAPIC 接管）（时限 400 ms）
    ("ST_SMP", 7), // SMP 阶段（AP 唤醒汇合）（时限 3000 ms）
    ("ST_DRIVERS", 8), // 驱动阶段（早期驱动探测）（时限 2500 ms）
    ("ST_FS", 9), // 文件系统阶段（initrd 挂载）（时限 1500 ms）
    ("ST_SCHED", 10), // 调度阶段（进程 0 就绪）（时限 900 ms）
    ("ST_USER", 11), // 用户态移交阶段（时限 700 ms）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn stage_get(key: &str) -> Option<u64> {
    match key {
        "ST_FIRMWARE" => Some(0),
        "ST_MEMORY" => Some(1),
        "ST_ACPI" => Some(2),
        "ST_PAGING" => Some(3),
        "ST_CPU" => Some(4),
        "ST_IDT" => Some(5),
        "ST_TIMER" => Some(6),
        "ST_SMP" => Some(7),
        "ST_DRIVERS" => Some(8),
        "ST_FS" => Some(9),
        "ST_SCHED" => Some(10),
        "ST_USER" => Some(11),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn stage_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < stage_COUNT {
        let (k, v) = stage[i];
        let ok = match k {
            "ST_FIRMWARE" => v == 0,
            "ST_MEMORY" => v == 1,
            "ST_ACPI" => v == 2,
            "ST_PAGING" => v == 3,
            "ST_CPU" => v == 4,
            "ST_IDT" => v == 5,
            "ST_TIMER" => v == 6,
            "ST_SMP" => v == 7,
            "ST_DRIVERS" => v == 8,
            "ST_FS" => v == 9,
            "ST_SCHED" => v == 10,
            "ST_USER" => v == 11,
            _ => false,
        };
        if !ok {
            bad += 1;
        }
        i += 1;
    }
    bad
}

/// 累计求和（守恒对账用）。
pub fn stage_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < stage_COUNT {
        s = s.wrapping_add(stage[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn stage_unique() -> bool {
    let mut i = 0usize;
    while i < stage_COUNT {
        let mut j = i + 1;
        while j < stage_COUNT {
            if stage[i].0 == stage[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 字符串键值映射 deadline（12 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 每阶段独立时限表（毫秒级，100..3000 逐行守卫）。
/// 行数。
pub const deadline_COUNT: usize = 12;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static deadline: [(&'static str, u64); 12] = [
    ("DL_FIRMWARE", 2000), // 固件阶段（BIOS/UEFI 交接）时限 2000 ms（毫秒级独立上限）
    ("DL_MEMORY", 1500), // 内存阶段（E820 盘点+页表）时限 1500 ms（毫秒级独立上限）
    ("DL_ACPI", 800), // ACPI 阶段（表解析）时限 800 ms（毫秒级独立上限）
    ("DL_PAGING", 1200), // 分页阶段（四级表构建）时限 1200 ms（毫秒级独立上限）
    ("DL_CPU", 600), // CPU 阶段（特性枚举）时限 600 ms（毫秒级独立上限）
    ("DL_IDT", 500), // 中断阶段（IDT 装载）时限 500 ms（毫秒级独立上限）
    ("DL_TIMER", 400), // 时钟阶段（PIT/LAPIC 接管）时限 400 ms（毫秒级独立上限）
    ("DL_SMP", 3000), // SMP 阶段（AP 唤醒汇合）时限 3000 ms（毫秒级独立上限）
    ("DL_DRIVERS", 2500), // 驱动阶段（早期驱动探测）时限 2500 ms（毫秒级独立上限）
    ("DL_FS", 1500), // 文件系统阶段（initrd 挂载）时限 1500 ms（毫秒级独立上限）
    ("DL_SCHED", 900), // 调度阶段（进程 0 就绪）时限 900 ms（毫秒级独立上限）
    ("DL_USER", 700), // 用户态移交阶段时限 700 ms（毫秒级独立上限）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn deadline_get(key: &str) -> Option<u64> {
    match key {
        "DL_FIRMWARE" => Some(2000),
        "DL_MEMORY" => Some(1500),
        "DL_ACPI" => Some(800),
        "DL_PAGING" => Some(1200),
        "DL_CPU" => Some(600),
        "DL_IDT" => Some(500),
        "DL_TIMER" => Some(400),
        "DL_SMP" => Some(3000),
        "DL_DRIVERS" => Some(2500),
        "DL_FS" => Some(1500),
        "DL_SCHED" => Some(900),
        "DL_USER" => Some(700),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn deadline_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < deadline_COUNT {
        let (k, v) = deadline[i];
        let ok = match k {
            "DL_FIRMWARE" => v >= 100 && v <= 3000,
            "DL_MEMORY" => v >= 100 && v <= 3000,
            "DL_ACPI" => v >= 100 && v <= 3000,
            "DL_PAGING" => v >= 100 && v <= 3000,
            "DL_CPU" => v >= 100 && v <= 3000,
            "DL_IDT" => v >= 100 && v <= 3000,
            "DL_TIMER" => v >= 100 && v <= 3000,
            "DL_SMP" => v >= 100 && v <= 3000,
            "DL_DRIVERS" => v >= 100 && v <= 3000,
            "DL_FS" => v >= 100 && v <= 3000,
            "DL_SCHED" => v >= 100 && v <= 3000,
            "DL_USER" => v >= 100 && v <= 3000,
            _ => false,
        };
        if !ok {
            bad += 1;
        }
        i += 1;
    }
    bad
}

/// 累计求和（守恒对账用）。
pub fn deadline_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < deadline_COUNT {
        s = s.wrapping_add(deadline[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn deadline_unique() -> bool {
    let mut i = 0usize;
    while i < deadline_COUNT {
        let mut j = i + 1;
        while j < deadline_COUNT {
            if deadline[i].0 == deadline[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 状态机 wd（5 态 × 5 事件 · 迁移 7 条显式声明）
// --------------------------------------------------------------------------
/// 看门狗状态机（超时→诊断模式）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum wdState {
    Disarmed,
    Armed,
    Petting,
    Fired,
    Diagnosing,
}

impl wdState {
    /// 全量态闭集。
    pub const ALL: [wdState; 5] = [
        wdState::Disarmed,
        wdState::Armed,
        wdState::Petting,
        wdState::Fired,
        wdState::Diagnosing,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            wdState::Disarmed => 0,
            wdState::Armed => 1,
            wdState::Petting => 2,
            wdState::Fired => 3,
            wdState::Diagnosing => 4,
        }
    }
}

/// wd事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum wdEvent {
    Arm,
    Pet,
    StageDone,
    Timeout,
    Diagnose,
}

impl wdEvent {
    /// 全量事件闭集。
    pub const ALL: [wdEvent; 5] = [
        wdEvent::Arm,
        wdEvent::Pet,
        wdEvent::StageDone,
        wdEvent::Timeout,
        wdEvent::Diagnose,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const WD_TRANSITIONS: [(wdState, wdEvent, wdState); 7] = [
    (wdState::Disarmed, wdEvent::Arm, wdState::Armed), // 布防看门狗
    (wdState::Armed, wdEvent::Pet, wdState::Petting), // 阶段内喂狗
    (wdState::Petting, wdEvent::Pet, wdState::Petting), // 持续喂狗
    (wdState::Armed, wdEvent::Timeout, wdState::Fired), // 超时触发
    (wdState::Petting, wdEvent::Timeout, wdState::Fired), // 喂狗间隔超时
    (wdState::Fired, wdEvent::Diagnose, wdState::Diagnosing), // 进诊断模式
    (wdState::Diagnosing, wdEvent::Arm, wdState::Armed), // 诊断后重新布防
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn wd_next(state: wdState, ev: wdEvent) -> Option<wdState> {
    let mut i = 0usize;
    while i < 7 {
        let t = WD_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn wd_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < wdState::ALL.len() {
        let mut ei = 0usize;
        while ei < wdEvent::ALL.len() {
            let legal = wd_next(wdState::ALL[si], wdEvent::ALL[ei]).is_some();
            if !legal {
                bad += 1;
            }
            ei += 1;
        }
        si += 1;
    }
    bad
}

/// 终态集合（无出边的态即终态）。
pub fn wd_terminal(st: wdState) -> bool {
    let mut i = 0usize;
    while i < 7 {
        if WD_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct wdLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: wdState,
}

impl wdLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: wdState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: wdEvent) -> bool {
        match wd_next(self.cur, ev) {
            Some(ns) => {
                self.cur = ns;
                self.legal += 1;
                true
            }
            None => {
                self.illegal += 1;
                false
            }
        }
    }

    /// 守恒恒等式：legal + illegal == 施加次数（由调用方累计）。
    pub fn total(&self) -> u64 {
        self.legal + self.illegal
    }
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x4b00, 0x4b0f]。
pub const fn t122_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t122Code::ALL.len() {
        let c = t122Code::ALL[i].code();
        if c < 0x4b00 || c > 0x4b0f {
            ok = false;
        }
        i += 1;
    }
    ok
}

// --------------------------------------------------------------------------
// 自检（CheckSet 范式：每检查一带 detail，全绿才出门）
// --------------------------------------------------------------------------
/// 段位守卫：全部诊断码落在本模块段内。
fn t122_guard_seg() -> bool {
    t122_segment_guard()
}

/// 表 stage 逐行守卫全绿（行数/守卫/唯一性）。
fn check_t122_stage_guard() -> bool {
    let mut ok = stage_COUNT == 12;
    if ok {
        ok = stage_scan() == 0;
    }
    if ok {
        ok = stage_unique();
    }
    ok
}

/// 独立时限表守卫 + 阶段/时限一一对应。
fn check_c2_deadline() -> bool {
    let mut ok = deadline_COUNT == 12 && deadline_scan() == 0 && deadline_unique();
    if ok {
        ok = deadline_get("DL_FIRMWARE") == Some(2000) && deadline_get("DL_USER") == Some(700);
    }
    if ok {
        // SMP 阶段时限最长（AP 唤醒汇合最慢路径）。
        ok = deadline_get("DL_SMP") == Some(3000);
    }
    ok
}

/// 看门狗状态机（布防-喂狗-超时-诊断-重防）。
fn check_c3_wd_sm() -> bool {
    let mut led = wdLedger::new(wdState::Disarmed);
    let mut ok = led.apply(wdEvent::Arm);
    ok = ok && led.apply(wdEvent::Pet);
    ok = ok && led.apply(wdEvent::Timeout);
    ok = ok && led.apply(wdEvent::Diagnose);
    if ok {
        ok = led.cur == wdState::Diagnosing;
    }
    if ok {
        ok = led.apply(wdEvent::Arm) && led.legal == 5;
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t122_checks() -> CheckSet {
    let mut set = CheckSet::new("T122");
    set.add(
        "t122-seg-guard",
        t122_guard_seg(),
        if t122_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t122-c1-stage-guard",
        check_t122_stage_guard(),
        if check_t122_stage_guard() { "" } else { "table stage guard mismatch" },
    );
    set.add(
        "t122-c2-deadline",
        check_c2_deadline(),
        if check_c2_deadline() { "" } else { "stage deadline table mismatch" },
    );
    set.add(
        "t122-c3-wd-sm",
        check_c3_wd_sm(),
        if check_c3_wd_sm() { "" } else { "watchdog state machine mismatch" },
    );
    set
}

// ---------------------------------------------------------------------------
// 单元测试（域聚合测试在 mod.rs——此处只放本模块纯函数级断言）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t122_all_checks_green() {
        let set = run_t122_checks();
        assert!(set.all_passed(), "T122 domain checks must be green");
    }

    #[test]
    fn t122_segment_guard_holds() {
        assert!(t122_segment_guard());
    }
}
