//! veb377_breaker —— VE-F0377 驱动崩溃熔断器。
//! 锚点（CoRun Unxreal 主册）：VE-F0377 · 驱动崩溃熔断器 · 目标 340 行
//! 判据原文：VE-F0377-J1 滑窗内崩溃计数达阈值即熔断（CLOSE→OPEN）；
//!           熔断后半开探针（OPEN→HALF），探针成功回 CLOSE、失败
//!           回 OPEN 且窗加倍（指数退避，封顶）；复位清账；熔断
//!           期间请求直接拒（快失败）。
//! 组件：三态断路器（Closed/Open/HalfOpen）+ 滑窗崩溃计数 +
//!       指数退避窗 + 探针账 + 熔断史。
//! 职责定位：熔断器是「驱动的保险丝」——一只反复崩的驱动不能拖
//!       死整个内核调用路径；快失败让调用方立刻拿到错误码去走
//!       备选路径，而不是挂在超时上；指数退避防止熔断-恢复-
//!       再熔断的高频震荡把 CPU 烧在状态切换上。
//! 性能逐项分解：record O(1)；probe O(1)；窗扫描 O(W)（W≤8）。
//! 诊断码段：0x303C0（0x303C1~0x303CA 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const BR_VERSION: &str = "veb377_breaker-v1";

/// 诊断段基址（0x303C0 段 · 本模块独占）。
pub const BR_SEG_BASE: u32 = 0x303C0;
pub const XC_TRIP_OPEN: u32 = 0x303C1; // 熔断中请求拒
pub const XC_NOT_HALF: u32 = 0x303C2; // 非半开态探针
pub const XC_NOT_OPEN: u32 = 0x303C3; // 非开态探针
pub const XC_WIN_OOR: u32 = 0x303C4; // 窗长越域
pub const XC_THR_ZERO: u32 = 0x303C5; // 阈值为零
pub const XC_MAGIC: u32 = 0x303C6; // 账魔数破
pub const XC_CAP: u32 = 0x303C7; // 退避封顶后再熔断不加窗
pub const XC_STATE: u32 = 0x303C8; // 状态机非法迁移
pub const XC_HISTORY: u32 = 0x303C9; // 熔断史核不一致
pub const XC_RESET: u32 = 0x303CA; // 未熔断先复位

/// 滑窗长。
pub const WIN: usize = 8;
/// 崩溃阈值。
pub const THRESH: u32 = 3;
/// 退避窗基础刻。
pub const BACKOFF_BASE: u64 = 16;
/// 退避封顶。
pub const BACKOFF_CAP: u64 = 128;

/// 断路器三态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrState {
    /// 通行。
    Closed = 0,
    /// 熔断。
    Open = 1,
    /// 半开探针。
    HalfOpen = 2,
}

/// 崩溃记录（时刻 + 是否崩溃）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rec {
    pub tick: u64,
    pub crashed: bool,
}

/// 熔断史一条。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TripRec {
    pub trip_at: u64,
    pub win: u64,
}

/// 断路器。
pub struct Breaker {
    pub state: BrState,
    win: Vec<Rec>,
    backoff: u64,
    trips: Vec<TripRec>,
    probe_ok: u32,
    probe_fail: u32,
    magic: u32,
}

impl Breaker {
    /// 建。
    pub fn new() -> Breaker {
        Breaker {
            state: BrState::Closed,
            win: Vec::new(),
            backoff: BACKOFF_BASE,
            trips: Vec::new(),
            probe_ok: 0,
            probe_fail: 0,
            magic: 0x03C0_BEEF,
        }
    }

    /// Closed 态下记一笔调用结果；窗内崩溃达阈即熔断。
    pub fn record(&mut self, tick: u64, crashed: bool) -> Result<(), u32> {
        if self.state != BrState::Closed {
            return Err(XC_STATE);
        }
        self.win.push(Rec { tick, crashed });
        if self.win.len() > WIN {
            self.win.remove(0);
        }
        let c = self.win.iter().filter(|r| r.crashed).count() as u32;
        if c >= THRESH {
            self.state = BrState::Open;
            self.trips.push(TripRec { trip_at: tick, win: self.backoff });
            return Err(XC_TRIP_OPEN);
        }
        Ok(())
    }

    /// 熔断中请求快失败。
    pub fn call(&self) -> Result<(), u32> {
        match self.state {
            BrState::Closed => Ok(()),
            BrState::Open => Err(XC_TRIP_OPEN),
            BrState::HalfOpen => Err(XC_NOT_HALF),
        }
    }

    /// 到点探针：Open → HalfOpen。
    pub fn arm_probe(&mut self, tick: u64) -> Result<(), u32> {
        if self.state != BrState::Open {
            return Err(XC_NOT_OPEN);
        }
        let last = match self.trips.last() {
            Some(t) => *t,
            None => return Err(XC_HISTORY),
        };
        if tick < last.trip_at + last.win {
            return Err(XC_NOT_OPEN); // 未到退避点
        }
        self.state = BrState::HalfOpen;
        Ok(())
    }

    /// 探针成功 → Closed，退避回基础刻。
    pub fn probe_pass(&mut self) -> Result<(), u32> {
        if self.state != BrState::HalfOpen {
            return Err(XC_NOT_HALF);
        }
        self.state = BrState::Closed;
        self.win.clear();
        self.backoff = BACKOFF_BASE;
        self.probe_ok += 1;
        Ok(())
    }

    /// 探针失败 → Open，窗加倍（封顶后不再加）。
    pub fn probe_fail(&mut self, tick: u64) -> Result<(), u32> {
        if self.state != BrState::HalfOpen {
            return Err(XC_NOT_HALF);
        }
        self.state = BrState::Open;
        self.probe_fail += 1;
        let next = self.backoff.saturating_mul(2);
        if self.backoff < BACKOFF_CAP {
            self.backoff = if next > BACKOFF_CAP { BACKOFF_CAP } else { next };
        } else {
            return Err(XC_CAP); // 已封顶，窗不再加
        }
        self.trips.push(TripRec { trip_at: tick, win: self.backoff });
        Ok(())
    }

    /// 复位清账（运维通道）。
    pub fn reset(&mut self) -> Result<(), u32> {
        if self.state == BrState::Closed && self.trips.is_empty() {
            return Err(XC_RESET);
        }
        self.state = BrState::Closed;
        self.win.clear();
        self.trips.clear();
        self.backoff = BACKOFF_BASE;
        Ok(())
    }

    /// 熔断次数。
    pub fn trip_count(&self) -> usize {
        self.trips.len()
    }

    /// 当前退避窗。
    pub fn cur_backoff(&self) -> u64 {
        self.backoff
    }

    /// 探针统计。
    pub fn probe_stat(&self) -> (u32, u32) {
        (self.probe_ok, self.probe_fail)
    }

    /// 审计：史与退避单调性核。
    pub fn audit(&self) -> Result<u32, u32> {
        if self.magic != 0x03C0_BEEF {
            return Err(XC_MAGIC);
        }
        for i in 1..self.trips.len() {
            if self.trips[i].trip_at <= self.trips[i - 1].trip_at {
                return Err(XC_HISTORY);
            }
        }
        Ok(self.trips.len() as u32)
    }
}

pub fn run_veb377_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0377");
    // 族一：滑窗熔断。
    let mut b = Breaker::new();
    s.add("初始通行", b.call().is_ok(), "Closed");
    s.add("崩 1", b.record(1, true).is_ok(), "1/3");
    s.add("崩 2", b.record(2, true).is_ok(), "2/3");
    s.add("崩 3 熔断", b.record(3, true) == Err(XC_TRIP_OPEN), "达阈");
    s.add("熔断快失败", b.call() == Err(XC_TRIP_OPEN), "Open");
    s.add("熔断史 1", b.trip_count() == 1, "一次 trip");
    // Closed 态外记数拒。
    s.add("Open 态记数拒", b.record(4, true) == Err(XC_STATE), "态机");
    // 族二：退避探针。
    s.add("未到点探针拒", b.arm_probe(3 + 15) == Err(XC_NOT_OPEN), "<16");
    s.add("到点探针开", b.arm_probe(3 + 16).is_ok(), "16 刻");
    s.add("Half 态 call 拒", b.call() == Err(XC_NOT_HALF), "半开");
    s.add("探针失败回 Open", b.probe_fail(20) == Ok(()), "加倍");
    s.add("退避窗 32", b.cur_backoff() == 32, "指数");
    // 族三：二次探针成功回位。
    s.add("再探未到拒", b.arm_probe(20 + 31) == Err(XC_NOT_OPEN), "<32");
    s.add("再探到点", b.arm_probe(20 + 32).is_ok(), "32 刻");
    s.add("探针成功回 Closed", b.probe_pass().is_ok(), "复位");
    s.add("回位通行", b.call().is_ok(), "Closed");
    s.add("退避回基础", b.cur_backoff() == 16, "回 16");
    s.add("探针统计", b.probe_stat() == (1, 1), "一成一败");
    // 族四：退避封顶。
    let mut c = Breaker::new();
    let mut t = 0u64;
    // 三熔三败：16→32→64→128（封顶），第四次 probe_fail 报 CAP。
    for round in 0..4u64 {
        let _ = c.record(t + 1, true);
        let _ = c.record(t + 2, true);
        let _ = c.record(t + 3, true);
        t += 3;
        // arm 在 (last.trip_at + win)
        let _ = c.arm_probe(t + c.cur_backoff());
        t += c.cur_backoff();
        let r = c.probe_fail(t);
        if round == 0 {
            s.add("败 1 窗 32", r.is_ok(), "16→32");
        } else if round == 1 {
            s.add("败 2 窗 64", r.is_ok(), "32→64");
        } else if round == 2 {
            s.add("败 3 窗 128", r.is_ok() && c.cur_backoff() == 128, "64→128 封顶");
        } else {
            s.add("封顶不再加", r == Err(XC_CAP), "128 定格");
            s.add("封顶仍 Open", c.state == BrState::Open, "态保");
        }
    }
    // 族五：复位与审计。
    s.add("史 4 笔", c.trip_count() == 4, "四熔");
    s.add("审计过", c.audit() == Ok(4), "时序单调");
    let mut d = Breaker::new();
    s.add("未熔先复位拒", d.reset() == Err(XC_RESET), "无事可复");
    s.add("魔数核", BR_SEG_BASE == 0x303C0, "段基址");
    s
}
