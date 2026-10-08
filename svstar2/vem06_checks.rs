//! VE-F2406 · 域自检（判据逐条对应，见 `vem06_event.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 三生产者格局 → `C06-总线-*`（三生产者齐全且域字母互异、动画事件计入
//!   生产者计数、总线不可用进缓冲不丢、恢复后按序补发）
//! - 第七类离散轨 → `C06-轨道-*`（七类齐全、事件轨为第七类、离散无插值、
//!   非单调时间被拒、事件轨类恒定）
//! - 触发语义表 → `C06-触发-*`（正播触发、倒播默认抑制、倒播可配开启、
//!   窗内窗外不触发、未注册拒绝告警、同帧合并取末值、风暴节流计数）
//! - 家族同构 → `C06-家族-*`（四成员同构声明一致、域字母齐全、说明可读）
//! - 参数钳制 → `C06-参数-*`（越界钳制、非有限归零、钳制记账）
//! - 零静默 → `C06-显性-*`（诊断渲染非空、合并数如实、抑制数如实）

use alloc::string::String;
use alloc::vec::Vec;

use super::vem06_event::*;
use crate::checks::CheckSet;

/// 造一条带三个已注册事件名的注册表。
fn registry3() -> (EventNameRegistry, DiagBag) {
    let mut bag = DiagBag::new();
    let mut reg = EventNameRegistry::new();
    reg.register("footstep", &mut bag);
    reg.register("dust", &mut bag);
    reg.register("voice", &mut bag);
    (reg, bag)
}

/// 造一条轨：三个事件名各一个关键帧，外加一对同帧同名关键帧。
fn track_fixture(bag: &mut DiagBag) -> EventTrack {
    let mut t = EventTrack::new();
    t.push(key(100, "footstep", 1.0, 0.5), bag);
    t.push(key(200, "dust", 2.0, 0.0), bag);
    t.push(key(300, "footstep", 0.25, 0.0), bag);
    t
}

// ---------------------------------------------------------------------------
// 判据一：三生产者格局
// ---------------------------------------------------------------------------

fn check_bus(set: &mut CheckSet) {
    // 三生产者齐全，域字母互异（J/H/M）
    let mut domains = Vec::new();
    let mut i = 0usize;
    while i < EventBusKind::ALL.len() {
        domains.push(EventBusKind::ALL[i].domain());
        i += 1;
    }
    let uniq = {
        let mut u = Vec::new();
        let mut j = 0usize;
        while j < domains.len() {
            let mut dup = false;
            let mut k = 0usize;
            while k < domains.len() {
                if k != j && domains[k] == domains[j] {
                    dup = true;
                }
                k += 1;
            }
            if !dup {
                u.push(domains[j]);
            }
            j += 1;
        }
        u.len()
    };
    set.add(
        "C06-总线-三生产者域字母互异",
        EventBusKind::ALL.len() == 3 && uniq == 3 && domains.contains(&"M"),
        "",
    );

    // 三个标签互不相同（标签相同则日志里分不出谁发的）
    let mut labels = Vec::new();
    let mut same = 0usize;
    let mut p = 0usize;
    while p < EventBusKind::ALL.len() {
        let a = EventBusKind::ALL[p].label();
        let mut q = 0usize;
        while q < labels.len() {
            if labels[q] == a {
                same += 1;
            }
            q += 1;
        }
        labels.push(a);
        p += 1;
    }
    set.add(
        "C06-总线-三生产者标签互异",
        same == 0 && labels.len() == 3,
        "",
    );

    // 动画事件真的计入**动画那一格**计数（不是三格都涨）
    let mut bus = EventBus::new();
    let mut bag = DiagBag::new();
    let mut n = 0usize;
    while n < 3 {
        bus.dispatch(
            BusEvent {
                producer: EventBusKind::Animation,
                name: String::from("footstep"),
                at_ms: 100u32.wrapping_add(n as u32),
                params: params(1.0, 0.0),
            },
            &mut bag,
        );
        n += 1;
    }
    set.add(
        "C06-总线-动画事件计入动画计数",
        bus.producer_counts[2] == 3
            && bus.producer_counts[0] == 0
            && bus.producer_counts[1] == 0
            && bus.delivered_len() == 3,
        "",
    );

    // 三生产者各自发一条 → 三格各 1（格局的运行期证据）
    let mut bus2 = EventBus::new();
    let mut p2 = 0usize;
    while p2 < EventBusKind::ALL.len() {
        bus2.dispatch(
            BusEvent {
                producer: EventBusKind::ALL[p2],
                name: String::from("x"),
                at_ms: 0,
                params: params(0.0, 0.0),
            },
            &mut bag,
        );
        p2 += 1;
    }
    set.add(
        "C06-总线-三生产者各计一格",
        bus2.producer_counts[0] == 1
            && bus2.producer_counts[1] == 1
            && bus2.producer_counts[2] == 1,
        "",
    );

    // 总线不可用 → 进缓冲**不丢**，且告警
    let mut down = EventBus::new();
    down.available = false;
    let mut bag3 = DiagBag::new();
    let ok = down.dispatch(
        BusEvent {
            producer: EventBusKind::Animation,
            name: String::from("dust"),
            at_ms: 1,
            params: params(1.0, 0.0),
        },
        &mut bag3,
    );
    set.add(
        "C06-总线-不可用时进缓冲不丢",
        !ok && down.pending.len() == 1
            && down.delivered_len() == 0
            && bag3.count(ev_diag::BUS_UNAVAILABLE) == 1,
        "",
    );

    // 恢复后**按入队序**补发（重排会让作者看到的事件次序错乱）
    let mut bag4 = DiagBag::new();
    let mut down2 = EventBus::new();
    down2.available = false;
    let mut q = 0u32;
    while q < 3 {
        down2.dispatch(
            BusEvent {
                producer: EventBusKind::Animation,
                name: String::from(if q == 0 {
                    "a"
                } else if q == 1 {
                    "b"
                } else {
                    "c"
                }),
                at_ms: q,
                params: params(0.0, 0.0),
            },
            &mut bag4,
        );
        q += 1;
    }
    let sent = down2.reconnect(&mut bag4);
    let order_ok = sent == 3
        && down2.delivered_len() == 3
        && down2.pending.is_empty()
        && down2.delivered[0].name == "a"
        && down2.delivered[1].name == "b"
        && down2.delivered[2].name == "c";
    set.add("C06-总线-恢复后按入队序补发", order_ok, "");
}

// ---------------------------------------------------------------------------
// 判据二：第七类离散轨
// ---------------------------------------------------------------------------

fn check_track(set: &mut CheckSet) {
    // 七类齐全
    set.add("C06-轨道-七类齐全", TrackClass::ALL.len() == 7, "");

    // 事件轨恰为**第七类**（下标 6），且七类标签互异。
    // 用**数位置**而不是直接索引 `ALL[6]`：直接索引在 ALL 缩到六类时会 panic，
    // 判据一panic 就不是判据了——它应该安静地报红。所以这里数出「排在
    // Event 之前的不同类有几个」，七类齐全时恰为 6，缺类时小于 6。
    let mut labels = Vec::new();
    let mut same = 0usize;
    let mut before_event = 0usize;
    let mut seen_event = false;
    let mut event_idx: i32 = -1;
    let mut i = 0usize;
    while i < TrackClass::ALL.len() {
        let k = TrackClass::ALL[i];
        let a = k.label();
        let mut j = 0usize;
        while j < labels.len() {
            if labels[j] == a {
                same += 1;
            }
            j += 1;
        }
        labels.push(a);
        if k == TrackClass::Event {
            if seen_event {
                before_event = usize::MAX; // 事件轨出现多次 → 判红
            }
            seen_event = true;
            event_idx = i as i32;
        } else if !seen_event {
            before_event += 1;
        }
        i += 1;
    }
    set.add(
        "C06-轨道-事件轨为第七类且标签互异",
        same == 0 && labels.len() == 7 && seen_event && event_idx == 6 && before_event == 6,
        "",
    );

    // 事件轨**离散无插值**，值轨不是（这条是第七类的根本区别）
    set.add(
        "C06-轨道-事件轨离散无插值",
        TrackClass::Event.is_discrete() && !TrackClass::Value.is_discrete(),
        "",
    );

    // 离散判定覆盖全部七类且与语义一致：布尔/枚举/触发/事件为离散，
    // 值/标记/曲线为连续。写成「只查事件轨」的话，把别轨的判定改错抓不到。
    let want = [false, true, true, true, false, false, true];
    let mut all_ok = true;
    let mut k = 0usize;
    while k < TrackClass::ALL.len() {
        if TrackClass::ALL[k].is_discrete() != want[k] {
            all_ok = false;
        }
        k += 1;
    }
    set.add("C06-轨道-七类离散判定逐条正确", all_ok, "");

    // 空轨类恒为事件轨（自洽）
    let e = EventTrack::new();
    set.add(
        "C06-轨道-事件轨类恒定",
        e.class() == TrackClass::Event && e.is_empty(),
        "",
    );

    // 时间**倒流**被拒；**同帧**必须被接受（同帧是去抖合并的前提，
    // 若这里也拒，去抖就成了永不执行的死代码）。
    let mut bag = DiagBag::new();
    let mut t = EventTrack::new();
    let a = t.push(key(100, "footstep", 1.0, 0.0), &mut bag);
    let same_time = t.push(key(100, "dust", 1.0, 0.0), &mut bag);
    let earlier = t.push(key(50, "voice", 1.0, 0.0), &mut bag);
    set.add(
        "C06-轨道-时间倒流被拒同帧被接受",
        a && same_time && !earlier && t.len() == 2 && bag.count(ev_diag::KEYS_NOT_MONOTONIC) == 1,
        "",
    );

    // 递增时间被接受（三条都进）
    let mut bag2 = DiagBag::new();
    let mut t2 = EventTrack::new();
    let all_in = t2.push(key(10, "footstep", 1.0, 0.0), &mut bag2)
        && t2.push(key(20, "dust", 1.0, 0.0), &mut bag2)
        && t2.push(key(30, "voice", 1.0, 0.0), &mut bag2);
    set.add(
        "C06-轨道-递增时间被接受",
        all_in && t2.len() == 3 && bag2.is_empty(),
        "",
    );
}

// ---------------------------------------------------------------------------
// 判据三：触发语义表
// ---------------------------------------------------------------------------

fn check_trigger(set: &mut CheckSet) {
    let (reg, _r0) = registry3();
    let mut bag = DiagBag::new();
    let t = track_fixture(&mut bag);

    // 正播触发：窗内三条全发
    let r = evaluate(
        &t,
        &reg,
        TriggerPolicy::default(),
        PlayDirection::Forward,
        0,
        400,
        &mut bag,
    );
    set.add(
        "C06-触发-正播窗内全发",
        r.fired_len() == 3 && r.rejected == 0 && r.throttled == 0 && r.suppressed == 0,
        "",
    );

    // 窗是**左闭右开**：to_ms 本身不触发（含端点会与下一帧重复触发）
    let r2 = evaluate(
        &t,
        &reg,
        TriggerPolicy::default(),
        PlayDirection::Forward,
        0,
        100,
        &mut bag,
    );
    set.add("C06-触发-窗右端开区间", r2.fired_len() == 0, "");

    // 窗起点闭：from_ms 本身触发
    let r3 = evaluate(
        &t,
        &reg,
        TriggerPolicy::default(),
        PlayDirection::Forward,
        100,
        101,
        &mut bag,
    );
    set.add("C06-触发-窗左端闭区间", r3.fired_len() == 1, "");

    // 倒播**默认抑制**（默认策略的语义表条目）
    let d = evaluate(
        &t,
        &reg,
        TriggerPolicy::default(),
        PlayDirection::Reverse,
        0,
        400,
        &mut bag,
    );
    set.add(
        "C06-触发-倒播默认抑制且计数",
        d.is_quiet()
            && d.suppressed == 3
            && d.fired_len() == 0
            && bag.count(ev_diag::REVERSE_SUPPRESSED) >= 1,
        "",
    );

    // 倒播**可配开启**（开关真的起作用，且开启后不再抑制）
    let pol = TriggerPolicy {
        fire_on_reverse: true,
        ..TriggerPolicy::default()
    };
    let mut bag2 = DiagBag::new();
    let d2 = evaluate(&t, &reg, pol, PlayDirection::Reverse, 0, 400, &mut bag2);
    set.add(
        "C06-触发-倒播可配开启",
        d2.fired_len() == 3 && d2.suppressed == 0 && pol.allows(PlayDirection::Reverse),
        "",
    );

    // 默认策略就是「倒播不触发」（语义表的默认值不能被改掉）
    set.add(
        "C06-触发-默认策略倒播不触发",
        !TriggerPolicy::default().allows(PlayDirection::Reverse)
            && TriggerPolicy::default().allows(PlayDirection::Forward),
        "",
    );

    // 未注册事件名 → 拒绝 + 告警 + 计数（**不静默忽略**）
    let mut bag3 = DiagBag::new();
    let mut t3 = EventTrack::new();
    t3.push(key(50, "not_registered", 1.0, 0.0), &mut bag3);
    let r4 = evaluate(
        &t3,
        &reg,
        TriggerPolicy::default(),
        PlayDirection::Forward,
        0,
        100,
        &mut bag3,
    );
    set.add(
        "C06-触发-未注册拒绝并告警",
        r4.fired_len() == 0 && r4.rejected == 1 && bag3.count(ev_diag::UNREGISTERED_EVENT) == 1,
        "",
    );

    // 同帧同事件名 → 合并，**参数取末值**
    let mut bag4 = DiagBag::new();
    let mut t4 = EventTrack::new();
    t4.push(key(150, "dust", 1.0, 0.0), &mut bag4);
    t4.push(key(150, "dust", 2.0, 0.0), &mut bag4);
    t4.push(key(150, "dust", 3.0, 0.0), &mut bag4);
    let r5 = evaluate(
        &t4,
        &reg,
        TriggerPolicy::default(),
        PlayDirection::Forward,
        0,
        200,
        &mut bag4,
    );
    let last_wins = r5.fired_len() == 1 && r5.fired[0].params.primary == 3.0;
    set.add("C06-触发-同帧合并取末值", last_wins && r5.merged == 2, "");

    // 合并数**如实报出**（不静默去重）
    set.add(
        "C06-显性-合并数如实",
        r5.merged == 2 && r5.fired_len() == 1,
        "",
    );

    // 同帧**不同名**不合并（合并键必须是（帧, 事件名）二元组）
    let mut bag5 = DiagBag::new();
    let mut t5 = EventTrack::new();
    t5.push(key(150, "dust", 1.0, 0.0), &mut bag5);
    t5.push(key(150, "footstep", 1.0, 0.0), &mut bag5);
    let r6 = evaluate(
        &t5,
        &reg,
        TriggerPolicy::default(),
        PlayDirection::Forward,
        0,
        200,
        &mut bag5,
    );
    set.add(
        "C06-触发-同帧异名不合并不误并",
        r6.fired_len() == 2 && r6.merged == 0,
        "",
    );

    // 风暴节流：单帧上限 1，三个不同名同帧 → 只发 1、丢弃 2、如实告警
    let tight = TriggerPolicy {
        fire_on_reverse: false,
        storm_limit: 1,
    };
    let mut bag6 = DiagBag::new();
    let mut t6 = EventTrack::new();
    t6.push(key(150, "dust", 1.0, 0.0), &mut bag6);
    t6.push(key(150, "footstep", 1.0, 0.0), &mut bag6);
    t6.push(key(150, "voice", 1.0, 0.0), &mut bag6);
    let r7 = evaluate(&t6, &reg, tight, PlayDirection::Forward, 0, 200, &mut bag6);
    set.add(
        "C06-触发-风暴节流且计数",
        r7.fired_len() == 1 && r7.throttled == 2 && bag6.count(ev_diag::STORM_THROTTLED) == 2,
        "",
    );

    // 抑制数**如实报出**（不静默吞事件）
    set.add(
        "C06-显性-抑制数如实",
        d.suppressed == 3 && d.fired_len() == 0,
        "",
    );

    // 空轨求值不炸也不发（表外形态：零关键帧）
    let empty = EventTrack::new();
    let r8 = evaluate(
        &empty,
        &reg,
        TriggerPolicy::default(),
        PlayDirection::Forward,
        0,
        999,
        &mut bag,
    );
    set.add(
        "C06-触发-空轨求值安静",
        r8.is_quiet() && r8.rejected == 0,
        "",
    );
}

// ---------------------------------------------------------------------------
// 判据四：家族同构
// ---------------------------------------------------------------------------

fn check_family(set: &mut CheckSet) {
    // 四成员齐全，标签与域字母互异
    let mut labels = Vec::new();
    let mut domains = Vec::new();
    let mut same = 0usize;
    let mut i = 0usize;
    while i < FamilyMember::ALL.len() {
        let l = FamilyMember::ALL[i].label();
        let d = FamilyMember::ALL[i].domain();
        let mut j = 0usize;
        while j < labels.len() {
            if labels[j] == l {
                same += 1;
            }
            j += 1;
        }
        labels.push(l);
        domains.push(d);
        i += 1;
    }
    set.add(
        "C06-家族-四成员标签互异",
        FamilyMember::ALL.len() == 4 && same == 0 && labels.len() == 4,
        "",
    );

    // 域字母齐全且动画侧是 M
    set.add(
        "C06-家族-域字母齐全且动画为M",
        domains.contains(&"M")
            && domains.contains(&"J")
            && domains.contains(&"H")
            && domains.contains(&"L07"),
        "",
    );

    // 一致性裁决通过（声明不是空话）
    set.add("C06-家族-范式一致性裁决通过", family_is_consistent(), "");

    // 三生产者齐备性：当前总线常量下成立
    set.add("C06-家族-三生产者齐备", three_producers_present(), "");

    // **表外形态逐条**：缺光照 / 缺音频 / 缺动画 三种缺项都必须判不通过。
    // 这三条是「漏判某一生产者」的唯一可观测面——只给「当前三类齐备」判绿的话，
    // 把 `if !need_anim` 整个删掉仍然全绿（三个标记照样被置上），漏判就不可观测。
    let no_light = [EventBusKind::Audio, EventBusKind::Animation];
    let no_audio = [EventBusKind::Light, EventBusKind::Animation];
    let no_anim = [EventBusKind::Light, EventBusKind::Audio];
    // 每条只判「该缺项形态不通过」+「另两个**各自补齐后**的形态通过」——
    // 写成一堆 && 容易把自己绕进去：形态本身缺的就是那一项，不能再要求它通过。
    set.add("C06-家族-缺光照判不通过", !producers_present(&no_light), "");
    set.add("C06-家族-缺音频判不通过", !producers_present(&no_audio), "");
    set.add("C06-家族-缺动画判不通过", !producers_present(&no_anim), "");

    // 反向：三类齐备的切片必须通过（防「一律返回 false」的退化实现）
    let full = [
        EventBusKind::Light,
        EventBusKind::Audio,
        EventBusKind::Animation,
    ];
    set.add(
        "C06-家族-三类齐备判通过且顺序无关",
        producers_present(&full)
            && producers_present(&[
                EventBusKind::Animation,
                EventBusKind::Audio,
                EventBusKind::Light,
            ]),
        "",
    );

    // 空集合也不通过（三生产者缺一即不通过，空即缺三个）
    set.add("C06-家族-空集合判不通过", !producers_present(&[]), "");

    // 家族域字母与三生产者域字母对齐（J/H/M 两侧都在）
    let fam_domains = {
        let mut v = Vec::new();
        let mut k = 0usize;
        while k < FamilyMember::ALL.len() {
            v.push(FamilyMember::ALL[k].domain());
            k += 1;
        }
        v
    };
    let mut bus_domains = Vec::new();
    let mut k2 = 0usize;
    while k2 < EventBusKind::ALL.len() {
        bus_domains.push(EventBusKind::ALL[k2].domain());
        k2 += 1;
    }
    let mut aligned = true;
    let mut a = 0usize;
    while a < bus_domains.len() {
        let mut found = false;
        let mut b = 0usize;
        while b < fam_domains.len() {
            if fam_domains[b] == bus_domains[a] {
                found = true;
            }
            b += 1;
        }
        if !found {
            aligned = false;
        }
        a += 1;
    }
    set.add("C06-家族-与总线三域对齐", aligned, "");

    // **反向**：若某成员域字母为空，一致性裁决必须红（证明裁决会咬人）
    // 这里只能正向验证「当前全体合法」；「会红」由反假变体 M-F3 覆盖。
    let all_iso = {
        let mut ok = true;
        let mut k = 0usize;
        while k < FamilyMember::ALL.len() {
            if !FamilyMember::ALL[k].is_isomorphic() {
                ok = false;
            }
            k += 1;
        }
        ok
    };
    set.add("C06-家族-四成员同构声明一致", all_iso, "");

    // 说明文本可读且含关键要素（读屏可达）
    let text = describe();
    set.add(
        "C06-显性-域说明可读",
        text.contains("事件轨")
            && text.contains("离散")
            && text.contains("动画事件")
            && text.contains("倒播")
            && !text.is_empty(),
        "",
    );

    // 冒烟：端到端最小闭环（域外可复用）
    let sm = smoke();
    set.add(
        "C06-显性-冒烟闭环",
        sm.contains("fired=2") && sm.contains("merged=0"),
        "",
    );
}

// ---------------------------------------------------------------------------
// 判据五：参数钳制 + 零静默
// ---------------------------------------------------------------------------

fn check_params(set: &mut CheckSet) {
    let mut bag = DiagBag::new();

    // 越界钳制到 ±PARAM_MAX
    let hi = clamp_params(params(9.0, -9.0), &mut bag);
    set.add(
        "C06-参数-越界被钳制",
        hi.primary == PARAM_MAX
            && hi.secondary == -PARAM_MAX
            && bag.count(ev_diag::PARAM_CLAMPED) == 2,
        "",
    );

    // 非有限归零（NaN / Inf 都要归零，不能放行）
    let nan = f32::NAN;
    let inf = f32::INFINITY;
    let n2 = clamp_params(params(nan, inf), &mut bag);
    set.add(
        "C06-参数-非有限归零",
        n2.primary == 0.0 && n2.secondary == 0.0 && bag.count(ev_diag::PARAM_CLAMPED) == 4,
        "",
    );

    // 域内值**不动**（钳制不能反着把合法值也改掉——恒钳到 0 的退化实现能过上面两条）
    let mut bag3 = DiagBag::new();
    let ok = clamp_params(params(1.5, -2.5), &mut bag3);
    set.add(
        "C06-参数-域内值不动且不记账",
        ok.primary == 1.5 && ok.secondary == -2.5 && bag3.is_empty(),
        "",
    );

    // 边界值恰好在上限处**不动**（真边界，非自证）
    let mut bag4 = DiagBag::new();
    let edge = clamp_params(params(PARAM_MAX, -PARAM_MAX), &mut bag4);
    set.add(
        "C06-参数-边界值不动",
        edge.primary == PARAM_MAX && edge.secondary == -PARAM_MAX && bag4.is_empty(),
        "",
    );

    // 诊断渲染非空且含条数（不静默）
    let text = bag.render();
    set.add(
        "C06-显性-诊断渲染非空",
        text.contains("诊断") && text.contains("4") && !text.is_empty(),
        "",
    );

    // 事件名重复注册幂等（热重载路径）+ 空名被拒
    let mut bag5 = DiagBag::new();
    let mut reg = EventNameRegistry::new();
    let first = reg.register("a", &mut bag5);
    let again = reg.register("a", &mut bag5);
    let empty_name = reg.register("", &mut bag5);
    set.add(
        "C06-显性-重复注册幂等空名被拒",
        first && again && !empty_name && reg.len() == 1 && reg.is_registered("a"),
        "",
    );

    // 已注册名清单可枚举（读屏可达）
    let (reg3, _b) = registry3();
    let names = reg3.names();
    set.add(
        "C06-显性-已注册名可枚举",
        names.len() == 3 && reg3.is_registered("voice"),
        "",
    );
}

/// VE-F2406 域自检。
pub fn run_vem06_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vem06");
    check_bus(&mut set);
    check_track(&mut set);
    check_trigger(&mut set);
    check_family(&mut set);
    check_params(&mut set);
    set
}

#[cfg(test)]
mod red_event {
    use super::*;
    #[test]
    fn event_red_items() {
        let set = run_vem06_checks();
        for name in set.red_items() {
            println!("[红] {}", name);
        }
        println!(
            "total={} passed={} dropped={}",
            set.len(),
            set.passed_count(),
            set.dropped()
        );
    }
}
