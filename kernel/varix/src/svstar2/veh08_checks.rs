//! VE-F1408 · 域自检（判据逐条对应，见 `veh08_eventbus.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 四要素 → `H08-四要素-*`
//! - 节流合并 → `H08-节流-*`
//! - 边界声明 → `H08-边界-*`
//! - 优先级 → `H08-优先级-*`
//! - 解耦（语义映射的发布订阅面）→ `H08-解耦-*`

use alloc::vec;
use alloc::string::ToString;
use super::veh08_eventbus::*;
use crate::checks::CheckSet;

use alloc::{format, vec};

fn ev(name: &str, source: &str, intensity: u32) -> AudioEvent {
    AudioEvent {
        name: name.to_string(),
        source: source.to_string(),
        intensity,
        context: vec![("category".to_string(), "mail".to_string())],
    }
}

/// VE-F1408 域自检。
pub fn run_veh08_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-veh08");

    // ---- 判据：四要素（校验 + 缺要素拒绝）----

    {
        let ok = ev("notification.arrived", "通知模块", 500).validate().is_ok();
        let no_name = ev("", "通知模块", 500).validate().is_err();
        let no_src = ev("e", "", 500).validate().is_err();
        let bad_intensity = ev("e", "m", 2000).validate().is_err();
        set.add("H08-四要素-校验与拒绝", ok && no_name && no_src && bad_intensity, "");
    }

    // ---- 判据：解耦（发布方只发事件，令牌登记决定响什么）----

    {
        let mut bus = EventBus::new();
        bus.subscribe(Subscription {
            event_name: "notification.arrived".to_string(),
            token_name: "notification.pop".to_string(),
            class: SubClass::System,
            sink: "通知音".to_string(),
        });
        let r = bus.publish(&ev("notification.arrived", "通知模块", 500), 0).unwrap();
        let dispatched = bus.dispatches.len() == 1
            && bus.dispatches[0].token_name == "notification.pop";
        // 换音效 = 改订阅登记，发布方零改动
        bus.subs[0].token_name = "notification.chime".to_string();
        let _ = bus.publish(&ev("notification.arrived", "通知模块", 500), 1);
        let swapped = bus.dispatches[1].token_name == "notification.chime";
        set.add(
            "H08-解耦-发布方零改动",
            r == 1 && dispatched && swapped,
            "",
        );
    }

    // ---- 判据：节流合并（窗口内 N 发 1 收 + 强度聚合）----

    {
        let mut bus = EventBus::new();
        bus.subscribe(Subscription {
            event_name: "ui.scroll".to_string(),
            token_name: "ui.scroll_tick".to_string(),
            class: SubClass::App,
            sink: "滚动音".to_string(),
        });
        bus.set_throttle(
            "ui.scroll",
            ThrottleRule { window_ticks: 100, max_per_window: 1, agg: IntensityAgg::Max },
        );
        // 窗口 0..100：五连发只触发一次，强度取最大
        let r1 = bus.publish(&ev("ui.scroll", "滚动模块", 200), 0).unwrap();
        let r2 = bus.publish(&ev("ui.scroll", "滚动模块", 900), 10).unwrap();
        let r3 = bus.publish(&ev("ui.scroll", "滚动模块", 400), 20).unwrap();
        let r4 = bus.publish(&ev("ui.scroll", "滚动模块", 700), 30).unwrap();
        // 新窗口（>=100）：重新触发
        let r5 = bus.publish(&ev("ui.scroll", "滚动模块", 300), 150).unwrap();
        let once = bus.dispatches.len() == 2;
        let agg = bus.dispatches[0].intensity == 900; // 窗口内最大
        let merged_count = bus.merged_total == 3;
        set.add(
            "H08-节流-N发一收强度取大",
            r1 == 1 && r2 == 0 && r3 == 0 && r4 == 0 && r5 == 1
                && once && agg && merged_count,
            "",
        );
    }
    // Latest 聚合策略
    {
        let mut bus = EventBus::new();
        bus.subscribe(Subscription {
            event_name: "drag".to_string(),
            token_name: "drag.tick".to_string(),
            class: SubClass::App,
            sink: "拖动音".to_string(),
        });
        bus.set_throttle(
            "drag",
            ThrottleRule { window_ticks: 50, max_per_window: 1, agg: IntensityAgg::Latest },
        );
        let _ = bus.publish(&ev("drag", "拖动模块", 100), 0);
        let _ = bus.publish(&ev("drag", "拖动模块", 800), 10);
        set.add(
            "H08-节流-Latest聚合策略",
            bus.dispatches[0].intensity == 800,
            "",
        );
    }

    // ---- 判据：边界声明（走错总线 = 架构违例）----

    {
        let mut bus = EventBus::new();
        let e = bus.publish_to_visualization(&ev("ui.click", "UI", 500));
        let rejected = e.as_ref().err().map(|x| {
            x.code == "E_WRONG_BUS" && x.is_complete() && x.why.contains("F1332")
        }).unwrap_or(false);
        let recorded = bus.boundary_violations.len() == 1;
        set.add("H08-边界-触发事件不走数据流", rejected && recorded, "");
    }

    // ---- 判据：优先级（系统 > 应用，同 sink 竞争让行记账）----

    {
        let mut bus = EventBus::new();
        // 同 sink 同事件被两级订阅：系统类与应用类
        bus.subscribe(Subscription {
            event_name: "system.alert".to_string(),
            token_name: "alert_sys".to_string(),
            class: SubClass::System,
            sink: "警示音".to_string(),
        });
        bus.subscribe(Subscription {
            event_name: "system.alert".to_string(),
            token_name: "alert_app".to_string(),
            class: SubClass::App,
            sink: "警示音".to_string(),
        });
        let _ = bus.publish(&ev("system.alert", "系统模块", 900), 0);
        let sys_won = bus.dispatches.len() == 1
            && bus.dispatches[0].class == SubClass::System
            && bus.dispatches[0].token_name == "alert_sys";
        let yielded = bus.yield_log.len() == 1; // 应用类让行有账，不静默
        set.add("H08-优先级-系统必胜让行有账", sys_won && yielded, "");
    }
    // 不同 sink 不仲裁：系统与应用各响各的
    {
        let mut bus = EventBus::new();
        bus.subscribe(Subscription {
            event_name: "e".to_string(),
            token_name: "t1".to_string(),
            class: SubClass::System,
            sink: "S1".to_string(),
        });
        bus.subscribe(Subscription {
            event_name: "e".to_string(),
            token_name: "t2".to_string(),
            class: SubClass::App,
            sink: "S2".to_string(),
        });
        let _ = bus.publish(&ev("e", "m", 100), 0);
        set.add(
            "H08-优先级-异sink各自分发",
            bus.dispatches.len() == 2 && bus.yield_log.is_empty(),
            "",
        );
    }

    // ---- 读屏与确定性 ----

    {
        let mut bus = EventBus::new();
        bus.subscribe(Subscription {
            event_name: "notification.arrived".to_string(),
            token_name: "notification.pop".to_string(),
            class: SubClass::System,
            sink: "通知音".to_string(),
        });
        let _ = bus.publish(&ev("notification.arrived", "通知模块", 500), 0);
        let d = &bus.dispatches[0];
        let summary = alloc::format!(
            "分发：令牌 {} → sink {}（类别 {:?}，强度 {}）",
            d.token_name, d.sink, d.class, d.intensity
        );
        set.add(
            "H08-读屏-分发摘要可播",
            summary.contains("notification.pop") && summary.contains("500"),
            "",
        );
    }
    {
        let run = || {
            let mut bus = EventBus::new();
            bus.subscribe(Subscription {
                event_name: "s".to_string(),
                token_name: "t".to_string(),
                class: SubClass::System,
                sink: "K".to_string(),
            });
            bus.set_throttle("s", ThrottleRule { window_ticks: 10, max_per_window: 1, agg: IntensityAgg::Max });
            let _ = bus.publish(&ev("s", "m", 100), 0);
            let _ = bus.publish(&ev("s", "m", 400), 5);
            let _ = bus.publish(&ev("s", "m", 250), 20);
            (bus.dispatches.clone(), bus.merged_total, bus.published_total)
        };
        let a = run();
        let b = run();
        set.add("H08-确定-同操作同账面", a == b, "");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::{format, vec};

    /// 域自检必须全绿——红项即施工未完成。
    #[test]
    fn veh08_checks_all_green() {
        let set = run_veh08_checks();
        let (passed, failed) = set.tally();
        if !set.all_passed() {
            let (items, n) = set.red_items();
            let mut msg = format!("VE-H08 域自检红项：{}/{} 绿", passed, passed + failed);
            for it in items.iter().take(n) {
                if let Some(c) = it {
                    if !c.passed {
                        msg.push_str(&format!("\n  [红] {} — {}", c.name, c.detail));
                    }
                }
            }
            panic!("{}", msg);
        }
    }

    /// 轰炸根治：高频滚动事件连续 50 发，只出窗口数个分发。
    #[test]
    fn scroll_bombing_tamed() {
        let mut bus = EventBus::new();
        bus.subscribe(Subscription {
            event_name: "ui.scroll".to_string(),
            token_name: "t".to_string(),
            class: SubClass::App,
            sink: "K".to_string(),
        });
        bus.set_throttle(
            "ui.scroll",
            ThrottleRule { window_ticks: 100, max_per_window: 1, agg: IntensityAgg::Max },
        );
        for i in 0..50u64 {
            let _ = bus.publish(&ev("ui.scroll", "m", (i * 20 % 1000) as u32), i * 5);
        }
        // 250 tick / 100 窗口 = 3 个窗口 → 至多 3 次触发
        assert!(bus.dispatches.len() <= 3, "50 发事件炸出 {} 次分发", bus.dispatches.len());
        assert!(bus.merged_total >= 47);
    }

    /// 无订阅的事件安全落地（不 panic、不分发）。
    #[test]
    fn unsubscribed_event_is_safe() {
        let mut bus = EventBus::new();
        let r = bus.publish(&ev("nobody.listens", "m", 1), 0).unwrap();
        assert_eq!(r, 1); // 发布成功
        assert!(bus.dispatches.is_empty()); // 但无人听就无声
    }
}
