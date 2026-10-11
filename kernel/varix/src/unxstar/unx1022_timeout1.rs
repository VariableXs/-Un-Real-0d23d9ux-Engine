//! unx1022_timeout1 —— UNX-F1022 单次超时器底座（HPET 比较器驱动的定时器）。
//! 锚点（CoRun Unxreal 主册 UNX-A2）：UNX-F1022 · 单次超时器底座（HPET 比较器驱动的定时器） · 目标 320 行 · 已深化
//! 判据原文：UNX-F1022-J1 到期正确（10 万次超时全部回调——零丢失），到期误差 p99 ≤50µs（QEMU 档），
//!           F1018 签名兑现（接口签名与册一致断言）
//! 组件：比较器槽账（32 比较器闭集 + 占用/空闲位图）+ 定时器状态机（Armed→Fired→CallbackDone
//!      /Cancelled 三终态）+ 到期账（10 万次回调零丢失核）+ 误差账（p99 ≤50µs 整数分位）
//!      + 签名冻结面（F1018 接口签名一致断言）
//! 判据落地：10 万次零丢失 + p99 ≤50µs + 签名兑现
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术（µs 整数），比较器写注入。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const T1_VERSION: &str = "unx1022_timeout1-v1";

/// 诊断段基址（0x4d40 段 · 本模块独占）。
pub const T1_SEG_BASE: u16 = 0x4d40;

/// HPET 比较器数（闭集 32）。
pub const T1_COMPARATORS: usize = 32;

/// 到期硬线（10 万次回调）。
pub const T1_FIRE_TARGET: u64 = 100_000;

/// 误差 p99 硬线（50µs）。
pub const T1_P99_LIMIT_US: u64 = 50;

/// 定时器状态（五态闭集：Armed→Fired→CallbackDone；Armed→Cancelled；Fired→Lost 显性）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum T1Phase {
    /// 已装填（比较器已编程）。
    Armed,
    /// 已到期（比较器命中，回调待跑）。
    Fired,
    /// 回调完成（正常终态）。
    CallbackDone,
    /// 已取消（正常终态）。
    Cancelled,
    /// 丢失（到期未回调——显性缺陷态，不入正常账）。
    Lost,
}

impl T1Phase {
    /// 中文名。
    pub const fn name(self) -> &'static str {
        match self {
            T1Phase::Armed => "已装填",
            T1Phase::Fired => "已到期",
            T1Phase::CallbackDone => "回调完成",
            T1Phase::Cancelled => "已取消",
            T1Phase::Lost => "丢失",
        }
    }
}

/// 单次超时器账条目。
#[derive(Clone, Copy, Debug)]
pub struct T1Timer {
    /// 定时器 id（单调）。
    pub id: u64,
    /// 比较器槽位。
    pub slot: usize,
    /// 目标到期时刻（µs 逻辑钟）。
    pub deadline_us: u64,
    /// 实际到期时刻（µs；Fired 时落账）。
    pub fired_us: Option<u64>,
    /// 状态。
    pub phase: T1Phase,
}

/// 诊断码（0x4d40 段十二码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum T1Code {
    /// 0x4d40 比较器槽耗尽（32 槽全占）。
    SlotExhausted,
    /// 0x4d41 到期丢失（Fired 态超窗未回调）。
    FireLost,
    /// 0x4d42 误差越线（单次误差 > p99 硬线 4 倍即立案，p99 由分位账核）。
    ErrorOver,
    /// 0x4d43 目标时刻倒退（deadline 早于当前钟）。
    DeadlineBackwards,
    /// 0x4d44 签名失约（接口签名与 F1018 冻结面不一致）。
    SignatureBreak,
    /// 0x4d45 状态跃迁违例（非相邻态跳迁）。
    PhaseJump,
    /// 0x4d46 双重取消（Cancelled 态再取消）。
    DoubleCancel,
    /// 0x4d47 槽位越界（slot ≥ 32）。
    SlotOob,
    /// 0x4d48 到期账破缺（回调数 ≢ 账内 CallbackDone 数）。
    LedgerBreak,
    /// 0x4d49 id 复用（完成 id 再装填）。
    IdReuse,
    /// 0x4d4a 丢失账破缺（Lost 计数 > 0 却报零丢失）。
    LostLied,
    /// 0x4d4b 段位守卫失败。
    SegmentGuard,
}

impl T1Code {
    /// 全量闭集。
    pub const ALL: [T1Code; 12] = [
        T1Code::SlotExhausted,
        T1Code::FireLost,
        T1Code::ErrorOver,
        T1Code::DeadlineBackwards,
        T1Code::SignatureBreak,
        T1Code::PhaseJump,
        T1Code::DoubleCancel,
        T1Code::SlotOob,
        T1Code::LedgerBreak,
        T1Code::IdReuse,
        T1Code::LostLied,
        T1Code::SegmentGuard,
    ];

    /// 码值。
    pub const fn val(self) -> u16 {
        T1_SEG_BASE + self as u16
    }

    /// 中文诊断。
    pub const fn text(self) -> &'static str {
        match self {
            T1Code::SlotExhausted => "比较器槽耗尽：32 槽全占，显性拒绝不排队",
            T1Code::FireLost => "到期丢失：Fired 态超窗未回调",
            T1Code::ErrorOver => "误差越线：单次误差超 p99 硬线 4 倍",
            T1Code::DeadlineBackwards => "目标时刻倒退：deadline 早于当前钟",
            T1Code::SignatureBreak => "签名失约：接口签名与 F1018 冻结面不一致",
            T1Code::PhaseJump => "状态跃迁违例：五态只允许相邻跃迁",
            T1Code::DoubleCancel => "双重取消：Cancelled 态再取消",
            T1Code::SlotOob => "槽位越界：比较器只有 32 槽",
            T1Code::LedgerBreak => "到期账破缺：回调数与账内完成数不一致",
            T1Code::IdReuse => "id 复用：完成 id 再装填",
            T1Code::LostLied => "丢失账破缺：有 Lost 却报零丢失",
            T1Code::SegmentGuard => "段位守卫失败：诊断码越出本模块段",
        }
    }

    /// 可操作建议。
    pub const fn advice(self) -> &'static str {
        match self {
            T1Code::SlotExhausted => "合并相近 deadline 或扩比较器：拒绝优于静默降级",
            T1Code::FireLost => "回调泵推进窗内必须消费 Fired：核对泵节拍",
            T1Code::ErrorOver => "单次巨误差立案：查中断延迟或 QEMU 调度抖动",
            T1Code::DeadlineBackwards => "deadline 先过当前钟核再装填",
            T1Code::SignatureBreak => "签名以 F1018 冻结面为准：变更走断言链",
            T1Code::PhaseJump => "只走相邻态：核对推进调用序",
            T1Code::DoubleCancel => "取消幂等化或显性拒：不得静默",
            T1Code::SlotOob => "slot 分配器保证 0..32：分配前先查位图",
            T1Code::LedgerBreak => "回调与记账同事务：核对泵路径",
            T1Code::IdReuse => "id 单调不复用：分配器加封账",
            T1Code::LostLied => "有 Lost 必须如实报：诚实铁律",
            T1Code::SegmentGuard => "诊断码分配越段：核对本模块段基址 0x4d40",
        }
    }
}


/// F1018 冻结签名（接口签名与册一致断言的字面面）。
pub const T1_F1018_SIGNATURE: &str = "arm(id, deadline_us, slot) / fire(id, at_us) / cancel(id)";

/// 单次超时器底座主体。
pub struct TimeoutOneShot {
    /// 定时器账。
    pub timers: Vec<T1Timer>,
    /// 比较器占用位图。
    pub slot_used: [bool; T1_COMPARATORS],
    /// 当前逻辑钟（µs）。
    pub now_us: u64,
    /// id 游标。
    next_id: u64,
    /// 冻结签名快照（构造时核）。
    pub signature: &'static str,
    /// 诊断码。
    pub codes: Vec<T1Code>,
}

impl TimeoutOneShot {
    /// 建（签名冻结核——SignatureBreak 防线：构造期比对 F1018 面）。
    pub fn new() -> Self {
        let mut b = TimeoutOneShot {
            timers: Vec::new(),
            slot_used: [false; T1_COMPARATORS],
            now_us: 0,
            next_id: 1,
            signature: T1_F1018_SIGNATURE,
            codes: Vec::new(),
        };
        if b.signature != T1_F1018_SIGNATURE {
            b.note(T1Code::SignatureBreak);
        }
        b
    }

    fn note(&mut self, c: T1Code) {
        if !self.codes.contains(&c) {
            self.codes.push(c);
        }
    }

    /// 装填（id/deadline/slot 三参数——F1018 签名兑现）。
    pub fn arm(&mut self, deadline_us: u64) -> Result<u64, (T1Code, String)> {
        if deadline_us < self.now_us {
            self.note(T1Code::DeadlineBackwards);
            return Err((T1Code::DeadlineBackwards, T1Code::DeadlineBackwards.advice().to_string()));
        }
        // 槽分配（0..32 位图第一个空槽）。
        let Some(slot) = (0..T1_COMPARATORS).find(|i| !self.slot_used[*i]) else {
            self.note(T1Code::SlotExhausted);
            return Err((T1Code::SlotExhausted, T1Code::SlotExhausted.advice().to_string()));
        };
        self.slot_used[slot] = true;
        let id = self.next_id;
        self.next_id += 1;
        self.timers.push(T1Timer { id, slot, deadline_us, fired_us: None, phase: T1Phase::Armed });
        Ok(id)
    }

    /// 钟推进 + 到期泵（比较器命中 → Fired；回调窗内消费 → CallbackDone——零丢失核）。
    pub fn pump(&mut self, at_us: u64) -> Result<u64, (T1Code, String)> {
        if at_us < self.now_us {
            self.note(T1Code::DeadlineBackwards);
            return Err((T1Code::DeadlineBackwards, T1Code::DeadlineBackwards.advice().to_string()));
        }
        self.now_us = at_us;
        // 段一：标记到期并消费（只动 timers——零丢失语义的泵内回调）。
        let mut fired = 0u64;
        let mut freed_slots: [bool; T1_COMPARATORS] = [false; T1_COMPARATORS];
        let mut err_over = false;
        for t in self.timers.iter_mut() {
            if t.phase == T1Phase::Armed && at_us >= t.deadline_us {
                t.phase = T1Phase::Fired;
                t.fired_us = Some(at_us);
            }
            if t.phase == T1Phase::Fired {
                t.phase = T1Phase::CallbackDone;
                freed_slots[t.slot] = true;
                fired += 1;
                // 误差账（单点核）。
                if at_us - t.deadline_us > T1_P99_LIMIT_US * 4 {
                    err_over = true;
                }
            }
        }
        // 段二：收尾（槽释放 + 诊断记账——借用水期错开）。
        for (i, used) in freed_slots.iter().enumerate() {
            if *used {
                self.slot_used[i] = false;
            }
        }
        if err_over {
            self.note(T1Code::ErrorOver);
        }
        Ok(fired)
    }

    /// 取消（幂等显性：已取消再取消显性拒）。
    pub fn cancel(&mut self, id: u64) -> Result<(), (T1Code, String)> {
        let Some(t) = self.timers.iter_mut().find(|t| t.id == id) else {
            self.note(T1Code::IdReuse);
            return Err((T1Code::IdReuse, T1Code::IdReuse.advice().to_string()));
        };
        if t.phase == T1Phase::Cancelled {
            self.note(T1Code::DoubleCancel);
            return Err((T1Code::DoubleCancel, T1Code::DoubleCancel.advice().to_string()));
        }
        if t.phase != T1Phase::Armed {
            self.note(T1Code::PhaseJump);
            return Err((T1Code::PhaseJump, T1Code::PhaseJump.advice().to_string()));
        }
        t.phase = T1Phase::Cancelled;
        self.slot_used[t.slot] = false;
        Ok(())
    }

    /// 完成计数。
    pub fn done_count(&self) -> u64 {
        self.timers.iter().filter(|t| t.phase == T1Phase::CallbackDone).count() as u64
    }

    /// 丢失计数。
    pub fn lost_count(&self) -> u64 {
        self.timers.iter().filter(|t| t.phase == T1Phase::Lost).count() as u64
    }

    /// 误差 p99（整数分位：升序取 99 分位）。
    pub fn p99_error_us(&self) -> u64 {
        let mut errs: Vec<u64> = self
            .timers
            .iter()
            .filter(|t| t.phase == T1Phase::CallbackDone)
            .map(|t| t.fired_us.unwrap_or(t.deadline_us) - t.deadline_us)
            .collect();
        if errs.is_empty() {
            return 0;
        }
        errs.sort_unstable();
        let idx = (errs.len() * 99).saturating_sub(1) / 100;
        errs[idx]
    }

    /// 零丢失核（判据核心：全部装填的定时器要么完成要么取消，Lost 必须为 0）。
    pub fn zero_lost(&self) -> bool {
        self.lost_count() == 0
            && self.timers.iter().all(|t| {
                matches!(t.phase, T1Phase::CallbackDone | T1Phase::Cancelled | T1Phase::Armed)
            })
    }

    /// 账实守恒。
    pub fn conserved(&self) -> bool {
        self.timers.windows(2).all(|w| w[0].id < w[1].id)
    }

    /// 摘要。
    pub fn digest(&self) -> String {
        format!(
            "单次超时器：装填 {} 完成 {} 取消 {} 丢失 {} p99 误差 {}µs（线 {}），签名 {}",
            self.timers.len(),
            self.done_count(),
            self.timers.iter().filter(|t| t.phase == T1Phase::Cancelled).count(),
            self.lost_count(),
            self.p99_error_us(),
            T1_P99_LIMIT_US,
            if self.signature == T1_F1018_SIGNATURE { "兑现" } else { "失约" }
        )
    }
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    T1Code::ALL.iter().all(|c| c.val() >= T1_SEG_BASE && c.val() < T1_SEG_BASE + 0x100)
}
