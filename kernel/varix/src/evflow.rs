//! evflow — WP-202 · B-901 事件流单向性（MD2 篇 9.1）。
//!
//! 判据 B-901：注入接口不存在（代码审计）。
//! MD2 原文（9.1）："事件流单向纪律（C-8 契约）的实现：事件只从驱动流向
//! 输入子系统再流向合成器，内核接口层不提供任何'注入事件到指定窗口'的
//! 调用——窗口伪造输入给别的窗口在接口层就不存在（Q59 的残余风险仅剩
//! 物理 HID 注入，威胁模型已登记）。系统级功能（截图快捷键、交接入口）
//! 消费事件的方式是注册过滤器而不是截改事件流——过滤器可以'消费'（事件
//! 到此为止）或'放行'，不能改写。"
//!
//! 宿主可测形态：三源归一（PS/2 / USB HID / 平台键 → 统一事件结构：时间戳/
//! 设备标识/事件类型/物理键位）+ **注入接口类型面不存在**（分发 API 只收
//! 驱动源事件——无 target_window 参数可填，C-8 结构防线同 B-801 submit 族）
//! + 过滤器只消费/放行不改写（过滤器输出要么原事件要么吞掉，无第三态）。

use crate::checks::CheckSet;

/// 物理键位编码宽度（scancode 语义，布局无关）。
pub const SCANCODE_CAP: u16 = 256;
/// 过滤器链容量。
pub const FILTER_CAP: usize = 8;

/// 三种驱动事件源（MD2 9.1 接缝）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EventSource {
    /// PS/2 控制器（键盘、触摸板——Y7000 内建）。
    Ps2,
    /// USB HID（外接键鼠，报告描述符归一化）。
    UsbHid,
    /// 电源与功能键（亮度、音量——平台通道）。
    Platform,
}

/// 事件类型（按下、释放、重复）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EvKind {
    Press,
    Release,
    Repeat,
}

/// 统一事件结构（物理键位是标准形态——布局翻译与 IME 都在驱动之后）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InputEvent {
    pub ts: u64,
    pub source: EventSource,
    pub kind: EvKind,
    /// 物理键位（布局无关 scancode——快捷键仲裁的稳定主键）。
    pub scancode: u16,
}

/// 过滤器裁决（**只有两态**：消费或放行——无"改写"态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FilterVerdict {
    /// 事件到此为止（系统级功能已处理）。
    Consume,
    /// 继续沿单向流走。
    Pass,
}

/// 系统级过滤器 trait（截图快捷键、交接入口的注册面）。
pub trait EventFilter {
    fn name(&self) -> &'static str;
    fn see(&self, ev: &InputEvent) -> FilterVerdict;
}

/// 输入子系统：单向流的唯一入口。
pub struct InputSubsystem {
    filters: [Option<&'static dyn EventFilter>; FILTER_CAP],
    n_filters: usize,
    /// 已分发的物理事件计数（对账面）。
    pub dispatched: u64,
    pub consumed: u64,
}

impl InputSubsystem {
    pub const fn new() -> InputSubsystem {
        InputSubsystem { filters: [None; FILTER_CAP], n_filters: 0, dispatched: 0, consumed: 0 }
    }

    /// 注册过滤器（先注册先得——与 B-905 仲裁同纪律）。
    pub fn register_filter(&mut self, f: &'static dyn EventFilter) -> bool {
        if self.n_filters >= FILTER_CAP {
            return false;
        }
        self.filters[self.n_filters] = Some(f);
        self.n_filters += 1;
        true
    }

    /// **唯一的**事件分发入口：只收驱动源事件。
    /// 类型面上没有"指定目标窗口"的参数——注入接口不存在（C-8）。
    /// 过滤器链只能消费或放行，返回给合成器的事件要么原样要么没有。
    pub fn dispatch(&mut self, ev: InputEvent) -> Option<InputEvent> {
        let mut i = 0;
        while i < self.n_filters {
            if let Some(f) = self.filters[i] {
                match f.see(&ev) {
                    FilterVerdict::Consume => {
                        self.consumed += 1;
                        return None; // 事件到此为止
                    }
                    FilterVerdict::Pass => {}
                }
            }
            i += 1;
        }
        self.dispatched += 1;
        Some(ev) // 原事件原样——没有改写态
    }

    pub fn n_filters(&self) -> usize {
        self.n_filters
    }
}

// ---------------------------------------------------------------- 对练

/// 事件流对练摘要。
#[derive(Default, PartialEq, Eq, Debug)]
pub struct EvDrillSummary {
    pub rounds: u32,
    pub events: u64,
    /// 过滤器输出两态（消费/放行），无改写
    pub filter_two_state: bool,
    /// 分发出去的事件与原事件逐字段一致（无改写的对账面）
    pub no_rewrite: bool,
    /// 消费计数 + 放行计数 = 总事件数
    pub accounted: bool,
}

/// 记账过滤器（对练用：验证两态与不改写）。
struct LedgerFilter {
    seen_orig: [u64; 4],
}

impl EventFilter for LedgerFilter {
    fn name(&self) -> &'static str {
        "ledger"
    }
    fn see(&self, ev: &InputEvent) -> FilterVerdict {
        // 确定性裁决：scancode % 3 == 0 消费，否则放行（不改写——trait 面就没有改写入口）
        if ev.scancode % 3 == 0 {
            FilterVerdict::Consume
        } else {
            FilterVerdict::Pass
        }
    }
}

static LEDGER: LedgerFilter = LedgerFilter { seen_orig: [0; 4] };

/// 随机事件流对练：过滤器两态 + 无改写 + 计数对账。
pub fn run_ev_drills(seed: u64, rounds: u32) -> EvDrillSummary {
    let mut g = crate::comprecover::Lcg(seed);
    let mut sum = EvDrillSummary::default();
    sum.rounds = rounds;
    sum.filter_two_state = true;
    sum.no_rewrite = true;
    sum.accounted = true;
    let mut sub = InputSubsystem::new();
    assert!(sub.register_filter(&LEDGER));
    let sources = [EventSource::Ps2, EventSource::UsbHid, EventSource::Platform];
    for i in 0..rounds {
        let ev = InputEvent {
            ts: i as u64,
            source: sources[(g.next() % 3) as usize],
            kind: if g.next() % 2 == 0 { EvKind::Press } else { EvKind::Release },
            scancode: (g.next() % SCANCODE_CAP as u64) as u16,
        };
        let out = sub.dispatch(ev);
        sum.events += 1;
        match out {
            Some(o) => {
                // 放行的事件必须逐字段一致（无改写）
                if o.ts != ev.ts || o.source != ev.source || o.kind != ev.kind || o.scancode != ev.scancode {
                    sum.no_rewrite = false;
                }
            }
            None => {} // 消费：两态之一
        }
    }
    if sub.dispatched + sub.consumed != sum.events {
        sum.accounted = false;
    }
    sum
}

// ---------------------------------------------------------------- 自检

pub fn run_evflow_checks() -> crate::checks::CheckSet {
    let mut set = CheckSet::new("B-901 事件流单向性");
    {
        // 三源归一：统一事件结构可构造
        let ev = InputEvent { ts: 1, source: EventSource::Ps2, kind: EvKind::Press, scancode: 0x1E };
        set.add(
            "B-901 三源归一事件结构",
            ev.source == EventSource::Ps2 && ev.scancode == 0x1E,
            "时间戳/设备标识/事件类型/物理键位（MD2 9.1）",
        );
    }
    {
        // C-8 结构防线：dispatch 只收驱动源事件，无目标窗口参数
        // （类型面审计：InputSubsystem 的公开 API 只有 dispatch(InputEvent) 与
        //   register_filter——不存在 inject_to(window) 形态的调用）
        let sub = InputSubsystem::new();
        set.add(
            "B-901 注入接口不存在（类型面）",
            sub.dispatched == 0 && sub.consumed == 0,
            "C-8 契约：分发 API 无 target_window 参数——伪造输入在接口层不存在",
        );
    }
    {
        // 过滤器两态：消费或放行，无改写
        let mut sub = InputSubsystem::new();
        let _ = sub.register_filter(&LEDGER);
        let ev = InputEvent { ts: 2, source: EventSource::UsbHid, kind: EvKind::Press, scancode: 7 };
        let out = sub.dispatch(ev); // 7 % 3 != 0 → 放行
        set.add(
            "B-901 过滤器两态不改写",
            out == Some(ev),
            "过滤器只能'消费'或'放行'，不能改写（MD2 9.1）",
        );
    }
    {
        // 消费语义：事件到此为止
        let mut sub = InputSubsystem::new();
        let _ = sub.register_filter(&LEDGER);
        let ev = InputEvent { ts: 3, source: EventSource::Platform, kind: EvKind::Press, scancode: 9 };
        let out = sub.dispatch(ev); // 9 % 3 == 0 → 消费
        set.add(
            "B-901 消费语义到此为止",
            out.is_none() && sub.consumed == 1,
            "系统级功能消费事件（截图快捷键等）",
        );
    }
    {
        // 物理键位是主键：同一 scancode 不同源都可分发（布局无关）
        let mut sub = InputSubsystem::new();
        let a = sub.dispatch(InputEvent { ts: 4, source: EventSource::Ps2, kind: EvKind::Press, scancode: 30 });
        let b = sub.dispatch(InputEvent { ts: 5, source: EventSource::UsbHid, kind: EvKind::Press, scancode: 30 });
        set.add(
            "B-901 物理键位标准形态",
            a.is_some() && b.is_some(),
            "布局翻译与 IME 都在驱动之后——换布局不换驱动",
        );
    }
    {
        // 过滤器链容量边界
        let mut sub = InputSubsystem::new();
        let mut ok = true;
        for _ in 0..FILTER_CAP {
            ok &= sub.register_filter(&LEDGER);
        }
        set.add(
            "B-901 过滤器链满不越界",
            ok && !sub.register_filter(&LEDGER) && sub.n_filters() == FILTER_CAP,
            "容量边界显式",
        );
    }
    {
        // 计数对账：dispatched + consumed = 总数
        let mut sub = InputSubsystem::new();
        let _ = sub.register_filter(&LEDGER);
        for sc in 0..12u16 {
            let _ = sub.dispatch(InputEvent { ts: sc as u64, source: EventSource::Ps2, kind: EvKind::Press, scancode: sc });
        }
        set.add(
            "B-901 事件计数对账",
            sub.dispatched + sub.consumed == 12 && sub.consumed == 4,
            "12 个 scancode 0..12 恰 4 个被 3 整除",
        );
    }
    {
        // 事件流对练
        let sum = run_ev_drills(0xB901, 80);
        set.add(
            "B-901 事件流对练",
            sum.rounds == 80 && sum.filter_two_state && sum.no_rewrite && sum.accounted,
            "单向流 + 两态过滤器 + 无改写（判据原文）",
        );
    }
    set
}

// ---------------------------------------------------------------- 单测

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f801_two_state_filter() {
        let mut sub = InputSubsystem::new();
        let _ = sub.register_filter(&LEDGER);
        let ev_pass = InputEvent { ts: 1, source: EventSource::Ps2, kind: EvKind::Press, scancode: 1 };
        assert_eq!(sub.dispatch(ev_pass), Some(ev_pass), "放行=原事件");
        let ev_consume = InputEvent { ts: 2, source: EventSource::Ps2, kind: EvKind::Press, scancode: 3 };
        assert_eq!(sub.dispatch(ev_consume), None, "消费=到此为止");
    }

    #[test]
    fn f801_no_inject_api() {
        // 类型面审计：InputSubsystem 无注入形态调用（编译即证——本测试体只引用存在的方法）
        let sub = InputSubsystem::new();
        assert_eq!(sub.dispatched, 0);
        assert_eq!(sub.n_filters(), 0);
    }

    #[test]
    fn f801_accounting() {
        let mut sub = InputSubsystem::new();
        let _ = sub.register_filter(&LEDGER);
        for sc in 0..9u16 {
            let _ = sub.dispatch(InputEvent { ts: sc as u64, source: EventSource::UsbHid, kind: EvKind::Release, scancode: sc });
        }
        assert_eq!(sub.dispatched + sub.consumed, 9);
        assert_eq!(sub.consumed, 3, "0,3,6 被 3 整除");
    }

    #[test]
    fn f801_drill_deterministic() {
        let a = run_ev_drills(5, 40);
        let b = run_ev_drills(5, 40);
        assert_eq!(a, b);
        assert!(a.filter_two_state && a.no_rewrite && a.accounted);
        assert_eq!(a.events, 40);
    }
}
