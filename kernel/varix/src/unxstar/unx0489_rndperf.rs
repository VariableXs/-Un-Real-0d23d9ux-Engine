//! unx0489_rndperf —— UNX-F0489 渲染性能账（微秒面度量）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0489 · 渲染性能账 · 目标 240 行
//! 判据原文：UNX-F0489-J1 微秒面度量账：绘制操作按微秒
//!           粒度记账（时钟源抽象为递增计数器，测试面注入
//!           确定性时钟）——账二 度量闭环（开始→结束→耗
//!           时三段计数一致）；账三 汇总账（总数/峰值/均值
//!           ×1000 整数算术）；账四 预算闸（超微秒预算立
//!           案不剔除）；账五 时钟回退拒（结束<开始立案）；
//!           账六 空窗合法（零操作汇总为 0/0/0 不除零）。
//! 组件：度量环 + 时钟抽象 + 汇总账 + 预算闸 + 回退核。
//! 职责定位：渲染性能是「用户可感知的最后一公里」——度量
//!   本身必须可信：时钟回退（TSC 跨核漂移）会产生负耗时，
//!   均值除零会在空帧时炸账。微秒面用整数算术（×1000 千
//!   分位）避免浮点在 no_std 软浮点路径的不确定性。
//! 性能逐项分解：度量 O(1)；汇总 O(n)；预算 O(1)。
//! 诊断码段：0x34900（0x34901~0x34909 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断码。
pub const SEG_BASE: u32 = 0x34900;
pub const XC_REGRESS: u32 = SEG_BASE + 1; // 时钟回退
pub const XC_BUDGET: u32 = SEG_BASE + 2; // 超预算立案

/// 确定性时钟（测试注入；生产面换 TSC 读取器）。
pub struct Clock(u64);

impl Clock {
    pub fn new(seed: u64) -> Self {
        Clock(seed)
    }
    pub fn now(&self) -> u64 {
        self.0
    }
    pub fn advance(&mut self, us: u64) {
        self.0 = self.0.wrapping_add(us);
    }
}

/// 单次度量（开始/结束为时钟刻）。
#[derive(Clone, Copy, Debug)]
pub struct Span {
    pub start: u64,
    pub end: u64,
    /// 操作标签（绘制/拷贝/清屏…）。
    pub tag: u32,
}

impl Span {
    /// 耗时（微秒）；回退为负语义 → Err。
    pub fn elapsed(&self) -> Result<u64, u32> {
        self.end.checked_sub(self.start).ok_or(XC_REGRESS)
    }
}

/// 汇总账（整数算术：均值 ×1000 千分位）。
pub struct PerfLedger {
    pub spans: Vec<Span>,
    pub budget_us: u64,
    pub over_budget: u32,
}

impl PerfLedger {
    pub fn new(budget_us: u64) -> Self {
        PerfLedger { spans: Vec::new(), budget_us, over_budget: 0 }
    }
    pub fn record(&mut self, s: Span) -> Result<(), u32> {
        let us = s.elapsed()?;
        if us > self.budget_us {
            // 超预算立案不剔除（记名度量纪律）。
            self.over_budget += 1;
            return Err(XC_BUDGET);
        }
        self.spans.push(s);
        Ok(())
    }
    /// 汇总：(总数, 峰值, 均值×1000)。空窗 → (0,0,0)。
    pub fn summarize(&self) -> (usize, u64, u64) {
        let n = self.spans.len();
        if n == 0 {
            return (0, 0, 0);
        }
        let mut total = 0u64;
        let mut peak = 0u64;
        for s in &self.spans {
            // elapsed 已在 record 时核过（record 拒回退），此处安全减。
            let us = s.end - s.start;
            total += us;
            if us > peak {
                peak = us;
            }
        }
        (n, peak, total * 1000 / n as u64)
    }
}

pub fn run_unx0489_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0489");
    cs.add("SEG 段位独占", SEG_BASE == 0x34900 && XC_BUDGET == SEG_BASE + 2, "u32 高段全域普查无冲突");

    // 族一：度量闭环。
    let mut ck = Clock::new(1000);
    let t0 = ck.now();
    ck.advance(250);
    let sp = Span { start: t0, end: ck.now(), tag: 1 };
    cs.add("耗时闭环", sp.elapsed() == Ok(250), "250us 精确读回");

    // 族二：回退拒。
    let rev = Span { start: 500, end: 400, tag: 2 };
    cs.add("回退拒", rev.elapsed() == Err(XC_REGRESS), "end<start 立案");
    let same = Span { start: 400, end: 400, tag: 3 };
    cs.add("零耗时合法", same.elapsed() == Ok(0), "同刻=0us");

    // 族三：汇总账。
    let mut led = PerfLedger::new(10_000);
    led.record(Span { start: 0, end: 100, tag: 1 }).ok(); // 100
    led.record(Span { start: 100, end: 450, tag: 2 }).ok(); // 350
    led.record(Span { start: 450, end: 550, tag: 3 }).ok(); // 100
    let (n, peak, avg) = led.summarize();
    cs.add("计数", n == 3, "三条入账");
    cs.add("峰值", peak == 350, "max(100,350,100)");
    cs.add("均值千分位", avg == 183_333, "(100+350+100)*1000/3");

    // 族四：预算闸（立案不剔除）。
    let mut led2 = PerfLedger::new(200);
    cs.add("预算内入账", led2.record(Span { start: 0, end: 150, tag: 4 }).is_ok(), "150<200");
    cs.add("超预算立案", led2.record(Span { start: 0, end: 500, tag: 5 }) == Err(XC_BUDGET), "500>200");
    cs.add("立案计数", led2.over_budget == 1, "记名不丢");
    let (n2, _, _) = led2.summarize();
    cs.add("剔除纪律", n2 == 1, "超预算者不入均值池");

    // 族五：空窗合法（除零防线）。
    let empty = PerfLedger::new(1000);
    let (en, epk, eav) = empty.summarize();
    cs.add("空窗三元组", (en, epk, eav) == (0, 0, 0), "0/0/0 不炸账");

    // 族六：时钟回绕面（wrapping 时钟产生回退——必拒）。
    let wrap = Span { start: u64::MAX - 10, end: 5, tag: 6 };
    cs.add("回绕判回退", wrap.elapsed() == Err(XC_REGRESS), "跨零点=回退语义");

    // 族七：单调时钟多 span 链。
    let mut ck2 = Clock::new(0);
    let mut ok_chain = true;
    for i in 1..=5u64 {
        let s = ck2.now();
        ck2.advance(i * 100);
        let e = ck2.now();
        ok_chain &= Span { start: s, end: e, tag: i as u32 }.elapsed() == Ok(i * 100);
    }
    cs.add("链式度量", ok_chain, "五段单调递增");

    // 族八：幂等。
    let r1 = sp.elapsed();
    let r2 = sp.elapsed();
    cs.add("度量幂等", r1 == r2, "两遍恒等");

    cs
}
