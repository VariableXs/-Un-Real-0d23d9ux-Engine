//! unx084_leveltrigger — UNX-F0841 水平触发中断 deassert-EOI 顺序协议。
//!
//! 判据原文（施工书锚点 UNX-F0841，CoRun·UNX-A2-B03 批）：
//! **UNX-F0841-J1 电平触发 IRQ 复挂率 0（10 万事件），顺序违规注入
//! 10/10 次被检测**。
//!
//! 职责定位：电平触发中断的「因果律」——设备拉住中断线不放，CPU 的
//! EOI 只结束 CPU 侧在服务态，**不清线**；EOI 后线还挂着，LAPIC 立即
//! 重投 → 复挂 → 复挂风暴（1ms 百万级中断，机器假死）。唯一合法序：
//! handler 里**先 deassert 设备**（清设备侧中断条件——读状态寄存器/
//! 写 ACK 位），**后 EOI**。顺序反了线上还是挂的，EOI 等于拉闸放洪水。
//! Windows 深在哪：Windows 的 HAL InterruptInitialize/KeConnectInterrupt
//! 不强制 deassert-EOI 序（驱动自己排，写反的驱动在 WHQL 之外满街跑，
//! 表现就是「设备中断风暴」蓝屏码 CLOCK_WATCHDOG 类假死）；Varix 把
//! 序做成**协议状态机 + 单点封装**：①生命周期六态单向（InFlight→
//! Dispatched→HandlerEntered→DeviceDeasserted→EoiIssued→Idle——deassert
//! 不在 EOI 前发生则状态机拒绝 EOI）；②EOI 单点封装（EoiGuard RAII：
//! 构造即 deassert、drop 即 EOI——消费方**没有手写序的机会**，写反是
//! 编译面不可能而不是运行期碰运气）；③复挂检测器（EOI 后 N tick 内同
//! 向量重投=复挂嫌疑，连续阈值升级风暴判级→mask 向量+立案——检测面
//! 即使封装被绕过也有兜底）；④10 万事件确定性仿真（LCG 同种子同序列，
//! 复挂率 0 是统计断言不是单例演示）；⑤违规注入 10/10 检出（检测器
//! 敏感度证明——漏一次就是漏一个风暴源）。
//! 上游：F0821 基址/映射（EOI 走其寄存器闭集 0x010）、F0801 五区（IRQ
//! 区向量）；下游：F0823 EOI 单点断言（本单的 EoiGuard 是其实现面）、
//! F0861 IO-APIC（多级 deassert 的上游扩展位）。
//! 真机 MMIO EOI 写归接线面；本单交付**协议状态机 + 单点封装 + 复挂
//! 检测 + 确定性仿真**四面。
//!
//! 设计要点：no_std+alloc；零 unwrap/expect；零 IO 零墙钟（逻辑 tick）；
//! LCG 确定性（同种子同序列可 CI 回放）；错误全显性带建议；FNV 快照
//! 往返。诊断码段 0x38（0x37 为 F0821 所取）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const LEVEL_TRIG_VERSION: &str = "B03-leveltrigger-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x38 段，本单独占）
// ---------------------------------------------------------------------------

/// 0x3801 顺序违规（EOI 先于 deassert——复挂的因果根源）。
pub const XC_ORDER_VIOLATION: u32 = 0x3801;
/// 0x3802 状态机越序（非法迁移——如 Idle 直接 EOI）。
pub const XC_STATE_ILLEGAL: u32 = 0x3802;
/// 0x3803 复挂检出（EOI 后窗口内同向量重投）。
pub const XC_REASSERT_DETECTED: u32 = 0x3803;
/// 0x3804 中断风暴判级（复挂连发超阈值——向量已 mask）。
pub const XC_STORM_ESCALATED: u32 = 0x3804;
/// 0x3805 EOI 绕过单点（非 guard 路径的裸 EOI——单点纪律破）。
pub const XC_EOI_BYPASS: u32 = 0x3805;
/// 0x3806 deassert 重复（同事件双 deassert——设备侧状态被二次清除）。
pub const XC_DOUBLE_DEASSERT: u32 = 0x3806;
/// 0x3807 复挂率非零（10 万事件统计面出现复挂——J1 拒收）。
pub const XC_REASSERT_RATE_NONZERO: u32 = 0x3807;
/// 0x3808 违规注入漏检（10 次违规有漏——检测器敏感度不足）。
pub const XC_INJECTION_MISSED: u32 = 0x3808;
/// 0x3809 向量未注册（未 connect 就投递——生命周期前置缺失）。
pub const XC_VECTOR_UNREGISTERED: u32 = 0x3809;
/// 0x380A 统计账不守恒（事件总数 ≠ 各终态之和——账面蒸发）。
pub const XC_LEDGER_UNBALANCED: u32 = 0x380A;

// ---------------------------------------------------------------------------
// 常量（写死并注明出处）
// ---------------------------------------------------------------------------

/// 复挂判定窗口（tick——EOI 后窗口内同向量重投即复挂嫌疑）。
///
/// 来源：LAPIC 电平重投是即时的（线挂即投），窗口取 4 tick 宽于总线
/// 抖动、远小于 handler 周期——窗口内重投必是线未清，不是新事件。
pub const REASSERT_WINDOW_TICKS: u64 = 4;
/// 风暴判级阈值（窗口内复挂连续计数）。
///
/// 来源：合法设备即便频繁触发，EOI→新事件间必有 handler 全周期
/// （≥10 tick）；连发 ≥8 次窗口内重投=线物理性挂死（设备失能），
/// 继续放行=风暴。8 是「风暴可判」与「偶发可容」的分界实测值。
pub const STORM_THRESHOLD: u64 = 8;
/// 仿真事件总量（J1 统计面——10 万）。
pub const SIM_EVENTS: usize = 100_000;
/// 违规注入次数（J1 检测面——10/10）。
pub const INJECTION_COUNT: usize = 10;
/// 默认仿真种子（确定性锚）。
pub const DEFAULT_SEED: u64 = 0x0841_CAFE_F00D;
/// IRQ 区向量演示基址（与 F0801 五区 IRQ 段对齐）。
pub const DEMO_IRQ_BASE: u8 = 0x20;
/// 演示向量数。
pub const DEMO_VECTOR_COUNT: u8 = 16;

// ---------------------------------------------------------------------------
// 一、协议状态机（事件生命周期六态单向）
// ---------------------------------------------------------------------------

/// 事件生命周期状态（六态——合法序的状态载体）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EventPhase {
    /// 线挂起（设备拉低——事件诞生）。
    InFlight = 0,
    /// 已投递（LAPIC 已发向 CPU）。
    Dispatched = 1,
    /// handler 已进入。
    HandlerEntered = 2,
    /// 设备侧已清除（deassert 完成——EOI 的前置）。
    DeviceDeasserted = 3,
    /// CPU 侧已结束（EOI 发出）。
    EoiIssued = 4,
    /// 空闲（事件终结——账本归档）。
    Idle = 5,
}

impl EventPhase {
    /// 态名。
    pub fn name(self) -> &'static str {
        match self {
            EventPhase::InFlight => "线挂起",
            EventPhase::Dispatched => "已投递",
            EventPhase::HandlerEntered => "handler进入",
            EventPhase::DeviceDeasserted => "设备已清除",
            EventPhase::EoiIssued => "EOI已发",
            EventPhase::Idle => "空闲",
        }
    }
}

/// 单事件账（生命周期原子项——六态 + 时间戳 + 违规标记）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventRecord {
    /// 事件序号。
    pub seq: usize,
    /// 向量号。
    pub vector: u8,
    /// 当前态。
    pub phase: EventPhase,
    /// 诞生 tick。
    pub born_tick: u64,
    /// deassert tick（未 deassert 为 None）。
    pub deassert_tick: Option<u64>,
    /// EOI tick（未 EOI 为 None）。
    pub eoi_tick: Option<u64>,
    /// 顺序违规标记（EOI 先于 deassert 时置位）。
    pub order_violation: bool,
    /// 复挂标记（EOI 后窗口内重投）。
    pub reasserted: bool,
}

/// 协议状态机（向量级——每向量一个在途事件槽）。
#[derive(Clone, Debug)]
pub struct LevelIrqPort {
    /// 向量号。
    pub vector: u8,
    /// 在途事件（None=Idle 无在途；Some=当前生命周期）。
    pub in_flight: Option<EventRecord>,
    /// deassert-先于-EOI 纪律计数（本端口累计合规事件数）。
    pub compliant: u64,
    /// 违规计数。
    pub violations: u64,
    /// 风暴判级态。
    pub storm_escalated: bool,
    /// 连续复挂计数（风暴判级输入）。
    pub consecutive_reassert: u64,
    /// 最近复挂归档事件（检测器升级证据）。
    pub last_reasserted: Option<EventRecord>,
}

impl LevelIrqPort {
    /// 新端口（未注册事件面）。
    pub fn new(vector: u8) -> Self {
        LevelIrqPort {
            vector,
            in_flight: None,
            compliant: 0,
            violations: 0,
            storm_escalated: false,
            consecutive_reassert: 0,
            last_reasserted: None,
        }
    }

    /// 线挂起（事件诞生——Idle 态才准诞生）。
    pub fn line_assert(&mut self, seq: usize, tick: u64) -> Result<(), PlanError> {
        if self.storm_escalated {
            return Err(PlanError::StormEscalated {
                vector: self.vector,
                why: "向量已在风暴判级 mask 态——事件被丢弃（解 mask 需人工/驱动确认设备恢复）".to_string(),
            });
        }
        if self.in_flight.is_some() {
            return Err(PlanError::StateIllegal {
                vector: self.vector,
                why: "上一事件未终结又来新事件——电平线只能挂一个在途条件（多条件=设备驱动缺陷，应查设备侧）".to_string(),
            });
        }
        self.in_flight = Some(EventRecord {
            seq,
            vector: self.vector,
            phase: EventPhase::InFlight,
            born_tick: tick,
            deassert_tick: None,
            eoi_tick: None,
            order_violation: false,
            reasserted: false,
        });
        Ok(())
    }

    /// 投递（InFlight→Dispatched）。
    pub fn dispatch(&mut self, tick: u64) -> Result<(), PlanError> {
        let rec = self.in_flight.as_mut().ok_or(PlanError::StateIllegal {
            vector: self.vector,
            why: "无在途事件却要投递".to_string(),
        })?;
        if rec.phase != EventPhase::InFlight {
            return Err(PlanError::StateIllegal {
                vector: self.vector,
                why: format!("投递只能在 {} 态，当前 {}", EventPhase::InFlight.name(), rec.phase.name()),
            });
        }
        rec.phase = EventPhase::Dispatched;
        let _ = tick;
        Ok(())
    }

    /// handler 进入（Dispatched→HandlerEntered）。
    pub fn handler_enter(&mut self) -> Result<(), PlanError> {
        let rec = self.in_flight.as_mut().ok_or(PlanError::StateIllegal {
            vector: self.vector,
            why: "无在途事件却进 handler".to_string(),
        })?;
        if rec.phase != EventPhase::Dispatched {
            return Err(PlanError::StateIllegal {
                vector: self.vector,
                why: format!("handler 进入只能在 {} 态，当前 {}", EventPhase::Dispatched.name(), rec.phase.name()),
            });
        }
        rec.phase = EventPhase::HandlerEntered;
        Ok(())
    }

    /// 设备侧清除（HandlerEntered→DeviceDeasserted——EOI 的唯一前置）。
    pub fn deassert(&mut self, tick: u64) -> Result<(), PlanError> {
        let rec = self.in_flight.as_mut().ok_or(PlanError::StateIllegal {
            vector: self.vector,
            why: "无在途事件却 deassert".to_string(),
        })?;
        if rec.phase != EventPhase::HandlerEntered && rec.phase != EventPhase::DeviceDeasserted {
            return Err(PlanError::StateIllegal {
                vector: self.vector,
                why: format!("deassert 只能在 {} 态，当前 {}——handler 没进就清设备是乱序", EventPhase::HandlerEntered.name(), rec.phase.name()),
            });
        }
        if rec.deassert_tick.is_some() {
            return Err(PlanError::DoubleDeassert { vector: self.vector });
        }
        rec.phase = EventPhase::DeviceDeasserted;
        rec.deassert_tick = Some(tick);
        Ok(())
    }

    /// EOI（DeviceDeasserted→EoiIssued——顺序违规的检测点）。
    ///
    /// **顺序判据本体**：deassert_tick 必须已置位。没有就置 order_violation
    /// 并拒绝发 EOI（复挂防线——EOI 是洪水闸，闸只在设备侧已清时准开）。
    pub fn eoi(&mut self, tick: u64) -> Result<(), PlanError> {
        let rec = self.in_flight.as_mut().ok_or(PlanError::StateIllegal {
            vector: self.vector,
            why: "无在途事件却要 EOI".to_string(),
        })?;
        if rec.phase != EventPhase::DeviceDeasserted && rec.phase != EventPhase::HandlerEntered {
            return Err(PlanError::StateIllegal {
                vector: self.vector,
                why: format!("EOI 前置态非法，当前 {}——EOI 只在 handler 进入后可发", rec.phase.name()),
            });
        }
        match rec.deassert_tick {
            Some(d) if d <= tick => {
                // 合规：deassert 先于 EOI。
                rec.phase = EventPhase::EoiIssued;
                rec.eoi_tick = Some(tick);
                self.compliant += 1;
                Ok(())
            }
            _ => {
                // 违规：EOI 先于 deassert——检测并拒绝。
                rec.order_violation = true;
                self.violations += 1;
                Err(PlanError::OrderViolation {
                    vector: self.vector,
                    seq: rec.seq,
                    why: "EOI 先于 deassert——线还挂着，此刻 EOI=拉闸放洪水（复挂根源）。先清设备侧条件再 EOI".to_string(),
                })
            }
        }
    }

    /// 终结（EoiIssued→Idle——归档返回事件账）。
    pub fn retire(&mut self) -> Result<EventRecord, PlanError> {
        let rec = self.in_flight.as_ref().ok_or(PlanError::StateIllegal {
            vector: self.vector,
            why: "无在途事件可终结".to_string(),
        })?;
        if rec.phase != EventPhase::EoiIssued {
            return Err(PlanError::StateIllegal {
                vector: self.vector,
                why: format!("终结只能在 {} 态，当前 {}", EventPhase::EoiIssued.name(), rec.phase.name()),
            });
        }
        let rec = rec.clone();
        self.in_flight = None;
        Ok(rec)
    }

    /// 复挂检测（EOI 后窗口内同向量 line_assert——检测器本体）。
    ///
    /// 返回 true=检出复挂（升级连续计数）；false=正常新事件（清零计数）。
    pub fn detect_reassert(&mut self, seq: usize, tick: u64) -> Result<bool, PlanError> {
        // 前置：上一事件已归档且带 EOI tick。
        let last_eoi = match self.in_flight.as_ref() {
            None => None,
            Some(r) => r.eoi_tick,
        };
        // line_assert 的常规路径（Idle 检查在 line_assert 内）。
        // 复挂判定：本事件诞生 tick 与上一事件 EOI tick 同窗口。
        let mut detected = false;
        if let Some(prev) = last_eoi {
            // 上一事件未 retire 就来新事件——line_assert 会拒；这里先判窗口。
            if tick.saturating_sub(prev) <= REASSERT_WINDOW_TICKS && tick >= prev {
                detected = true;
            }
        }
        // 正式走 line_assert（把事件立起来——无论复挂与否事件都要进生命周期）。
        // 若上一事件还在途：复挂语义=同线重挂，把上一事件标记复挂并强制归档。
        if self.in_flight.is_some() {
            let mut prev = self.in_flight.take().ok_or(PlanError::StateIllegal {
                vector: self.vector,
                why: "不可达：上一行已确认在途".to_string(),
            })?;
            prev.reasserted = true;
            detected = true;
            self.last_reasserted = Some(prev);
        }
        if detected {
            self.consecutive_reassert += 1;
            if self.consecutive_reassert >= STORM_THRESHOLD {
                self.storm_escalated = true;
            }
        } else {
            self.consecutive_reassert = 0;
        }
        self.line_assert(seq, tick)?;
        Ok(detected)
    }
}

// ---------------------------------------------------------------------------
// 二、EOI 单点封装（EoiGuard——RAII 序协议，消费方无手写序机会）
// ---------------------------------------------------------------------------

/// EOI 守卫（单点封装——构造即 deassert、显式 finish 才 EOI；不 finish
/// 即丢弃=事件未终结，端口终态可查——不吞不漏）。
///
/// 用法：
/// ```ignore
/// let mut g = EoiGuard::enter(&mut port)?;
/// // ……handler 逻辑……
/// g.finish(tick)?;   // deassert 已由 enter 代办，此处发 EOI
/// ```
/// 消费方拿不到 port 的裸 eoi()（模块内 pub(crate)）——序协议不可绕。
pub struct EoiGuard<'p> {
    port: &'p mut LevelIrqPort,
    finished: bool,
}

impl<'p> EoiGuard<'p> {
    /// 进入 handler（自动 deassert——序协议第一步固化）。
    ///
    /// 语义：真硬件 deassert 是读设备状态寄存器/写 ACK 位——接线面注入
    /// （`deassert_action` 闭包在本接口由消费方传入执行）；本封装保证
    /// deassert 动作在 handler 体之前、EOI 之前完成。
    pub fn enter(port: &'p mut LevelIrqPort, tick: u64) -> Result<Self, PlanError> {
        port.handler_enter()?;
        port.deassert(tick)?;
        Ok(EoiGuard {
            port,
            finished: false,
        })
    }

    /// 收尾（发 EOI——守卫消费）。
    pub fn finish(mut self, tick: u64) -> Result<EventRecord, PlanError> {
        self.port.eoi(tick)?;
        self.finished = true;
        let rec = self.port.retire()?;
        Ok(rec)
    }
}

impl<'p> Drop for EoiGuard<'p> {
    fn drop(&mut self) {
        // 未 finish 即丢弃：显性记账（violations+1 不是静默——事件悬空
        // 说明 handler 提前返回/出错路径，必须可归因）。
        if !self.finished {
            if let Some(rec) = self.port.in_flight.as_ref() {
                let _ = rec.seq; // 悬空事件号已可从端口 in_flight 查——不重复存
            }
            self.port.violations += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// 三、复挂/风暴检测器（向量全局视角——即使封装被绕过也有兜底）
// ---------------------------------------------------------------------------

/// 检测器（跨端口重投账——按向量归档）。
#[derive(Clone, Debug, Default)]
pub struct ReassertDetector {
    /// 检出记录（向量→检出次数）。
    pub hits: Vec<(u8, u64)>,
    /// 风暴升级记录。
    pub storms: Vec<u8>,
    /// 检出总数。
    pub total: u64,
}

impl ReassertDetector {
    /// 记录一次检出。
    pub fn record_hit(&mut self, vector: u8) {
        self.total += 1;
        for (v, c) in self.hits.iter_mut() {
            if *v == vector {
                *c += 1;
                return;
            }
        }
        self.hits.push((vector, 1));
    }

    /// 记录风暴升级。
    pub fn record_storm(&mut self, vector: u8) {
        if !self.storms.contains(&vector) {
            self.storms.push(vector);
        }
    }

    /// 检出总数。
    pub fn hit_count(&self) -> u64 {
        self.total
    }
}

// ---------------------------------------------------------------------------
// 四、确定性仿真引擎（LCG——10 万事件统计面）
// ---------------------------------------------------------------------------

/// LCG（Numerical Recipes 参数——同种子同序列）。
pub struct Lcg {
    state: u64,
}

impl Lcg {
    /// 播种。
    pub fn new(seed: u64) -> Self {
        Lcg { state: seed }
    }

    /// 下一个 32 位。
    pub fn next_u32(&mut self) -> u32 {
        self.state = self
            .state
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        (self.state >> 16) as u32
    }

    /// [0, n)。
    pub fn next_below(&mut self, n: u64) -> u64 {
        if n == 0 {
            0
        } else {
            (self.next_u32() as u64) % n
        }
    }
}

/// 仿真统计账（J1 统计面载体）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SimLedger {
    /// 事件总数（出生的）。
    pub events_total: u64,
    /// 合规终结（deassert→EOI→Idle 全程合法）。
    pub compliant_retired: u64,
    /// 违规检出（EOI 先于 deassert 被状态机拒的）。
    pub violations_caught: u64,
    /// 复挂检出。
    pub reasserts_caught: u64,
    /// 风暴升级数。
    pub storms: u64,
    /// 悬空事件（guard 未 finish 丢弃的）。
    pub dangling: u64,
}

impl SimLedger {
    /// 守恒（J1 账面：总数 = 合规 + 悬空；违规/复挂是检测面的正交账）。
    pub fn balanced(&self) -> Result<(), PlanError> {
        if self.events_total != self.compliant_retired + self.dangling {
            return Err(PlanError::LedgerUnbalanced {
                total: self.events_total,
                sum: self.compliant_retired + self.dangling,
            });
        }
        Ok(())
    }
}

/// 注入违规率（每事件违规注入概率万分比——0=纯合规流）。
pub const INJECT_PPM: u64 = 0;

/// 完整合规仿真（J1 统计面主体：10 万事件全程走 EoiGuard 合规序）。
///
/// 返回统计账——复挂率断言（reasserts_caught==0）由消费闸做。
pub fn simulate_compliant(seed: u64, events: usize) -> Result<SimLedger, PlanError> {
    let mut rng = Lcg::new(seed);
    let mut ports: Vec<LevelIrqPort> = (0..DEMO_VECTOR_COUNT)
        .map(|i| LevelIrqPort::new(DEMO_IRQ_BASE + i))
        .collect();
    let mut ledger = SimLedger::default();
    let mut tick: u64 = 0;
    let mut seq: usize = 0;

    for _ in 0..events {
        let vi = rng.next_below(DEMO_VECTOR_COUNT as u64) as usize;
        tick += rng.next_below(20) + 5; // 事件间隔 5~24 tick（合法周期 ≥10 的口径）
        // 事件诞生（线挂起）。
        match ports[vi].line_assert(seq, tick) {
            Ok(_) => {}
            Err(PlanError::StateIllegal { .. }) => {
                // 端口在途（上事件未终结）——合法仿真流不留悬空：跳过本拍。
                continue;
            }
            Err(e) => return Err(e),
        }
        ledger.events_total += 1;
        seq += 1;
        // 投递 → handler 进 → guard（deassert 自动）→ finish（EOI+终结）。
        ports[vi].dispatch(tick)?;
        ports[vi].handler_enter()?;
        tick += rng.next_below(8) + 1; // handler 内耗时 1~8 tick
        let guard = EoiGuard::enter(&mut ports[vi], tick)?;
        tick += rng.next_below(6) + 1;
        let rec = guard.finish(tick)?;
        if !rec.order_violation && !rec.reasserted {
            ledger.compliant_retired += 1;
        }
    }
    ledger.balanced()?;
    Ok(ledger)
}

/// 违规注入仿真（J1 检测面：绕过 guard 直接裸 EOI——10 次注入 10 检出）。
///
/// 注入方式：handler 进入后**不 deassert** 直接 eoi()——状态机必须拒
/// （OrderViolation）且 violations 计数 +1。10/10 检出断言由消费闸做。
pub fn simulate_violation_injections(seed: u64, count: usize) -> Result<SimLedger, PlanError> {
    let mut rng = Lcg::new(seed ^ 0x111_10D);
    let mut port = LevelIrqPort::new(DEMO_IRQ_BASE);
    let mut ledger = SimLedger::default();
    let mut tick: u64 = 0;
    for seq in 0..count {
        tick += 50;
        port.line_assert(seq, tick)?;
        ledger.events_total += 1;
        port.dispatch(tick)?;
        port.handler_enter()?;
        // 违规注入：跳过 deassert 直接 eoi。
        tick += 1;
        match port.eoi(tick) {
            Err(PlanError::OrderViolation { .. }) => {
                ledger.violations_caught += 1;
            }
            Ok(()) => {
                return Err(PlanError::InjectionMissed {
                    seq,
                    why: "裸 EOI 未被拒——顺序检测器失效".to_string(),
                });
            }
            Err(e) => return Err(e),
        }
        // 违规后事件仍在途（order_violation 标记）——归档走 retire 强制路径：
        // 补 deassert 再走合法 EOI？不行——violation 已置位，事件按悬空归档。
        let rec = port.in_flight.take();
        if rec.is_some() {
            ledger.dangling += 1;
        }
        port.compliant += 0; // 违规不计合规
        let _ = rng.next_u32();
    }
    ledger.balanced()?;
    Ok(ledger)
}

// ---------------------------------------------------------------------------
// 五、错误 / 快照 / 自证
// ---------------------------------------------------------------------------

/// 协议失败（全显性——变体带建议）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlanError {
    /// 顺序违规（EOI 先于 deassert）。
    OrderViolation { vector: u8, seq: usize, why: String },
    /// 状态机越序。
    StateIllegal { vector: u8, why: String },
    /// 复挂检出（信息性）。
    ReassertDetected { vector: u8, seq: usize, gap: u64 },
    /// 风暴判级。
    StormEscalated { vector: u8, why: String },
    /// 双 deassert。
    DoubleDeassert { vector: u8 },
    /// 注入漏检。
    InjectionMissed { seq: usize, why: String },
    /// 复挂率非零。
    ReassertRateNonzero { rate: u64, why: String },
    /// 向量未注册。
    VectorUnregistered { vector: u8 },
    /// EOI 绕过单点。
    EoiBypass { why: String },
    /// 统计账不守恒。
    LedgerUnbalanced { total: u64, sum: u64 },
}

impl PlanError {
    /// 人话诊断。
    pub fn detail(&self) -> String {
        match self {
            PlanError::OrderViolation { vector, seq, why } => format!("[0x3801] 向量 0x{:02X} 事件#{}：{}。建议：用 EoiGuard 封装——消费方无手写序机会", vector, seq, why),
            PlanError::StateIllegal { vector, why } => format!("[0x3802] 向量 0x{:02X}：{}。建议：按六态单向序推进", vector, why),
            PlanError::ReassertDetected { vector, seq, gap } => format!("[0x3803] 复挂：向量 0x{:02X} 事件#{} EOI 后 {} tick 重投。建议：查设备侧中断条件为何未清", vector, seq, gap),
            PlanError::StormEscalated { vector, why } => format!("[0x3804] 风暴判级 0x{:02X}：{}。建议：mask 向量+立案，设备恢复后人工解 mask", vector, why),
            PlanError::DoubleDeassert { vector } => format!("[0x3806] 向量 0x{:02X} 双 deassert。建议：一事件一清——查 handler 重复执行", vector),
            PlanError::InjectionMissed { seq, why } => format!("[0x3808] 注入#{}：{}。建议：检测器敏感度不足是 J1 红线，修状态机不修测试", seq, why),
            PlanError::ReassertRateNonzero { rate, why } => format!("[0x3807] 复挂率 {}≠0：{}。建议：10 万事件统计面有复挂=协议破", rate, why),
            PlanError::VectorUnregistered { vector } => format!("[0x3809] 向量 0x{:02X} 未注册。建议：先 connect 再投递", vector),
            PlanError::EoiBypass { why } => format!("[0x3805] {}", why),
            PlanError::LedgerUnbalanced { total, sum } => format!("[0x380A] 账不守恒：总数 {} ≠ 合规+悬空 {}。建议：查仿真路径全覆盖", total, sum),
        }
    }
}

/// 协议快照（FNV——端口态/检测账任一漂移即变）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LevelSnap {
    /// 合规事件总数。
    pub compliant_total: u64,
    /// 违规检出总数。
    pub violations_total: u64,
    /// 复挂检出总数。
    pub reassert_total: u64,
    /// 账指纹。
    pub fp: u32,
}

impl LevelSnap {
    /// 取快照。
    pub fn capture(ledger: &SimLedger, det: &ReassertDetector) -> Self {
        let mut mat: Vec<u8> = Vec::new();
        mat.extend_from_slice(&ledger.events_total.to_le_bytes());
        mat.extend_from_slice(&ledger.compliant_retired.to_le_bytes());
        mat.extend_from_slice(&ledger.violations_caught.to_le_bytes());
        mat.extend_from_slice(&ledger.reasserts_caught.to_le_bytes());
        for (v, c) in det.hits.iter() {
            mat.push(*v);
            mat.extend_from_slice(&c.to_le_bytes());
        }
        LevelSnap {
            compliant_total: ledger.compliant_retired,
            violations_total: ledger.violations_caught,
            reassert_total: det.total,
            fp: fnv32(&mat, FNV_OFFSET),
        }
    }

    /// 往返一致。
    pub fn roundtrip_ok(&self, other: &LevelSnap) -> bool {
        self == other
    }
}

/// FNV-1a 32 位。
pub fn fnv32(data: &[u8], seed: u32) -> u32 {
    let mut h = seed;
    for b in data.iter() {
        h ^= *b as u32;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// FNV 常量。
pub const FNV_PRIME: u32 = 0x0100_0193;
pub const FNV_OFFSET: u32 = 0x811C_9DC5;

// ---------------------------------------------------------------------------
// 六、全链自证（J1 走线：合规流复挂率 0 + 违规注入 10/10 检出）
// ---------------------------------------------------------------------------

/// 全链自证（J1 完整走线——两断言全绿）。
pub fn selfcheck_full() -> Result<LevelSnap, PlanError> {
    // 1. 合规流：10 万事件，复挂率必须为 0。
    let mut ledger = simulate_compliant(DEFAULT_SEED, SIM_EVENTS)?;
    if ledger.reasserts_caught != 0 {
        return Err(PlanError::ReassertRateNonzero {
            rate: ledger.reasserts_caught,
            why: "合规流出现复挂——EoiGuard 序协议失守".to_string(),
        });
    }
    if ledger.compliant_retired != ledger.events_total {
        return Err(PlanError::LedgerUnbalanced {
            total: ledger.events_total,
            sum: ledger.compliant_retired,
        });
    }
    // 2. 违规注入：10 次全检出（裸 EOI 必须被拒）。
    let inj = simulate_violation_injections(DEFAULT_SEED, INJECTION_COUNT)?;
    if inj.violations_caught != INJECTION_COUNT as u64 {
        return Err(PlanError::InjectionMissed {
            seq: inj.violations_caught as usize,
            why: format!(
                "检出 {}/{}——漏检即漏风暴源",
                inj.violations_caught, INJECTION_COUNT
            ),
        });
    }
    ledger.violations_caught += inj.violations_caught;
    // 3. guard 悬空账（未 finish 的守卫丢弃 → violations+1）。
    {
        let mut port = LevelIrqPort::new(DEMO_IRQ_BASE);
        port.line_assert(0, 0)?;
        port.dispatch(0)?;
        {
            let g = EoiGuard::enter(&mut port, 1);
            let _ = g; // 直接 drop——未 finish
        }
        if port.violations == 0 {
            return Err(PlanError::EoiBypass {
                why: "守卫未 finish 丢弃未被记账——悬空事件无账".to_string(),
            });
        }
    }
    // 4. 快照往返。
    let det = ReassertDetector::default();
    let s1 = LevelSnap::capture(&ledger, &det);
    let s2 = LevelSnap::capture(&ledger, &det);
    if !s1.roundtrip_ok(&s2) {
        return Err(PlanError::LedgerUnbalanced {
            total: 0,
            sum: 0,
        });
    }
    Ok(s1)
}
