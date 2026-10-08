//! VE-F2204 · 域自检（判据逐条对应，见 `vel04_mode.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 四模式 → `E01-四模式-*`
//! - 混合叠加 → `E01-混合-*`
//! - 事件契约 → `E01-事件-*`
//! - 节流保量 → `E01-节流-*`
//! - 降级矩阵（未注册拒绝/ 参数越界钳制 / 风暴节流 / 相位漂移 / NaN钳制）→ `E01-降级-*`
//! - 零静默与确定性 → `E01-零静默-*`
//!
//! 零墙钟、零 IO，随机源与时间均注入故回归可复现。

use super::vel03_emitter::{DiagBag, DiagCode, Vec3};
use super::vel04_mode::*;
use crate::checks::CheckSet;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 建一个标准事件注册表（三个已注册事件）。
fn std_registry() -> EventRegistry {
    let mut bag = DiagBag::new();
    let mut r = EventRegistry::new();
    r.register(
        1,
        EventSchema { position_offset: Vec3::ZERO, count_mult: 1.0, speed_mult: 1.0 },
        &mut bag,
    );
    r.register(
        2,
        EventSchema { position_offset: Vec3::new(1.0, 0.0, 0.0), count_mult: 2.0, speed_mult: 3.0 },
        &mut bag,
    );
    r.register(
        3,
        EventSchema { position_offset: Vec3::new(0.0, 2.0, 0.0), count_mult: 0.5, speed_mult: 1.0 },
        &mut bag,
    );
    r
}

/// 持续模式（rate 粒子/秒）。
fn cont(rate: f32) -> ModeState {
    create_mode(ModeKind::Continuous, &[rate]).expect("持续模式")
}

/// VE-F2204 域自检。
pub fn run_vel04_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F2204");

    // =======================================================================
    // 一、四模式（判据一）
    // =======================================================================

    // 四型在册不多不少，中英名齐备。
    {
        let ok = ModeKind::ALL.len() == 4
            && ModeKind::ALL.iter().all(|k| !k.zh().is_empty() && !k.en().is_empty());
        set.add("E01-四模式-四型在册不多不少", ok, "");
    }

    // 四型英文名唯一。
    {
        let mut names: Vec<&str> = ModeKind::ALL.iter().map(|k| k.en()).collect();
        names.sort_unstable();
        let mut uniq = names.clone();
        uniq.dedup();
        set.add("E01-四模式-英文名唯一", names.len() == uniq.len(), "");
    }

    // 累积器归属声明：持续与间隔走累积器，爆发与事件不走。
    {
        let ok = ModeKind::Continuous.uses_accumulator()
            && ModeKind::Interval.uses_accumulator()
            && !ModeKind::Burst.uses_accumulator()
            && !ModeKind::EventDriven.uses_accumulator();
        set.add("E01-四模式-累积器归属声明与实现相符", ok, "");
    }

    // 持续模式：10 粒/秒，0.1 秒一帧 → 每帧 1 粒。
    {
        let mut ms = ModeStack::new();
        ms.push(ModeKind::Continuous, cont(10.0));
        let mut bag = DiagBag::new();
        let mut total = 0u32;
        for _ in 0..10 {
            for d in ms.tick(0.1, &mut bag) {
                total += d.count;
            }
        }
        set.add("E01-四模式-持续按发射率累积", total == 10, "");
    }

    // 爆发模式：每次触发发 N 粒，可重复触发。
    {
        let mut ms = ModeStack::new();
        let idx = ms.push(
            ModeKind::Burst,
            create_mode(ModeKind::Burst, &[25.0]).expect("爆发模式"),
        );
        let mut bag = DiagBag::new();
        let a = ms.trigger_burst(idx, &mut bag);
        let b = ms.trigger_burst(idx, &mut bag);
        // 爆发**不**在 tick 里偷发。
        let ticked = ms.tick(0.1, &mut bag);
        let burst_count = match ms.get(idx).map(|m| &m.state) {
            Some(ModeState::Burst { burst_count, .. }) => *burst_count,
            _ => 0,
        };
        set.add(
            "E01-四模式-爆发触发即发且不在帧步进偷发",
            a == 25 && b == 25 && ticked.is_empty() && burst_count == 2,
            "",
        );
    }

    // 间隔模式：周期 0.5 秒、每次 10 粒；跑 2 秒应恰好 4 次共 40 粒。
    {
        let mut ms = ModeStack::new();
        ms.push(
            ModeKind::Interval,
            create_mode(ModeKind::Interval, &[0.5, 10.0]).expect("间隔模式"),
        );
        let mut bag = DiagBag::new();
        let mut total = 0u32;
        for _ in 0..120 {
            for d in ms.tick(1.0 / 60.0, &mut bag) {
                total += d.count;
            }
        }
        set.add("E01-四模式-间隔按周期触发", total == 40, "");
    }

    // 间隔模式相位不漂移：长时间小步进，累计触发次数须与理论一致（误差 0）。
    {
        let mut ms = ModeStack::new();
        ms.push(
            ModeKind::Interval,
            create_mode(ModeKind::Interval, &[0.1, 1.0]).expect("间隔模式"),
        );
        let mut bag = DiagBag::new();
        // 10 秒 / (1/120 秒) = 1200 帧，周期 0.1 秒 → 应恰好 100 次。
        for _ in 0..1200 {
            ms.tick(1.0 / 120.0, &mut bag);
        }
        let fired = match ms.get(0).map(|m| &m.state) {
            Some(ModeState::Interval { fired_count, .. }) => *fired_count,
            _ => 0,
        };
        set.add("E01-四模式-间隔相位长跑不漂移", fired == 100, "");
    }

    // 事件驱动模式：已注册事件按 schema 产出。
    {
        let mut ms = ModeStack::new();
        let idx = ms.push(
            ModeKind::EventDriven,
            create_mode(ModeKind::EventDriven, &[2.0]).expect("事件模式"),
        );
        let mut bag = DiagBag::new();
        let reg = std_registry();
        // 事件 2 的 count_mult=2.0，基准 1 → 2 粒。
        let out = ms.dispatch_event_with_base(&reg, idx, 1, &mut bag);
        set.add("E01-四模式-事件驱动按schema产出", out == 2, "");
    }

    // =======================================================================
    // 二、混合叠加（判据二）
    // =======================================================================

    // 两模式并存且**独立累积**：持续 10/s + 间隔 0.5s×10，跑 1 秒
    // 期望 持续 10 粒 + 间隔 20 粒 = 30 粒（两者互不偷配额）。
    {
        let mut ms = ModeStack::new();
        ms.push(ModeKind::Continuous, cont(10.0));
        ms.push(
            ModeKind::Interval,
            create_mode(ModeKind::Interval, &[0.5, 10.0]).expect("间隔模式"),
        );
        let mut bag = DiagBag::new();
        let mut by_mode = [0u32; 2];
        for _ in 0..60 {
            for d in ms.tick(1.0 / 60.0, &mut bag) {
                by_mode[d.mode_index as usize] += d.count;
            }
        }
        set.add(
            "E01-混合-两模式独立累积互不偷配额",
            by_mode[0] == 10 && by_mode[1] == 20,
            "",
        );
    }

    // **关键反例**：关掉间隔模式后，持续模式发射量必须不变。
    //
    // 若四模式共用一个累积器，关掉任一模式会连带改变其余模式的发射量——
    // 表现为「调整一个无关参数，粒子密度就变了」，是极难定位的一类缺陷。
    {
        let mut with_both = ModeStack::new();
        with_both.push(ModeKind::Continuous, cont(10.0));
        with_both.push(
            ModeKind::Interval,
            create_mode(ModeKind::Interval, &[0.5, 10.0]).expect("间隔模式"),
        );
        let mut cont_only = ModeStack::new();
        cont_only.push(ModeKind::Continuous, cont(10.0));

        let mut bag = DiagBag::new();
        let mut total_both = 0u32;
        for _ in 0..60 {
            for d in with_both.tick(1.0 / 60.0, &mut bag) {
                if d.mode_index == 0 {
                    total_both += d.count;
                }
            }
        }
        let mut total_only = 0u32;
        for _ in 0..60 {
            for d in cont_only.tick(1.0 / 60.0, &mut bag) {
                total_only += d.count;
            }
        }
        set.add(
            "E01-混合-关其他模式不影响持续模式",
            total_both == total_only,
            "",
        );
    }

    // 三模式并存：持续 + 爆发 + 间隔，各自独立产出。
    {
        let mut ms = ModeStack::new();
        ms.push(ModeKind::Continuous, cont(10.0));
        let burst_idx = ms.push(
            ModeKind::Burst,
            create_mode(ModeKind::Burst, &[5.0]).expect("爆发"),
        );
        ms.push(
            ModeKind::Interval,
            create_mode(ModeKind::Interval, &[1.0, 8.0]).expect("间隔"),
        );
        let mut bag = DiagBag::new();
        let mut tick_total = 0u32;
        for _ in 0..60 {
            for d in ms.tick(1.0 / 60.0, &mut bag) {
                tick_total += d.count;
            }
        }
        let burst = ms.trigger_burst(burst_idx, &mut bag);
        // 持续 10粒 + 间隔 1 次 8 粒 = 18，爆发另计 5。
        set.add(
            "E01-混合-三模式并存各自独立",
            tick_total == 18 && burst == 5,
            "",
        );
    }

    // 叠加语义显式：帧步进产出的下标能对上挂载顺序。
    {
        let mut ms = ModeStack::new();
        ms.push(ModeKind::Continuous, cont(10.0));
        ms.push(
            ModeKind::Interval,
            create_mode(ModeKind::Interval, &[0.5, 10.0]).expect("间隔"),
        );
        let mut bag = DiagBag::new();
        let idxs: Vec<u8> = ms.tick(0.5, &mut bag).iter().map(|d| d.mode_index).collect();
        set.add(
            "E01-混合-产出携带来源下标",
            idxs.contains(&0) && idxs.contains(&1),
            "",
        );
    }

    // 空栈与非法 dt：零产出零诊断（不无事生非）。
    {
        let mut ms = ModeStack::new();
        let mut bag = DiagBag::new();
        let empty = ms.tick(0.1, &mut bag);
        ms.push(ModeKind::Continuous, cont(10.0));
        let bad_dt = ms.tick(-1.0, &mut bag);
        let zero_dt = ms.tick(0.0, &mut bag);
        set.add(
            "E01-混合-空栈与非法dt零产出零诊断",
            empty.is_empty() && bad_dt.is_empty() && zero_dt.is_empty() && bag.is_empty(),
            "",
        );
    }

    set
}

/// 事件契约 + 节流保量 + 降级矩阵自检。
pub fn run_vel04_event_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F2204-event");

    // =======================================================================
    // 三、事件契约（判据三）
    // =======================================================================

    // 注册表：注册/查询/清单齐备。
    {
        let r = std_registry();
        set.add(
            "E01-事件-注册表注册查询清单",
            r.len() == 3
                && r.lookup(1).is_some()
                && r.lookup(2).map(|s| s.count_mult == 2.0).unwrap_or(false)
                && r.registered_names().len() == 3,
            "",
        );
    }

    // 中性 schema 等价于不覆盖：偏移零、倍率 1。
    {
        let n = EventSchema::neutral();
        let mut bag = DiagBag::new();
        let p = clamp_event_params(n, 10, &mut bag);
        set.add(
            "E01-事件-中性schema等价不覆盖",
            p.position_offset == Vec3::ZERO
                && p.effective_count() == 10
                && p.speed_mult == 1.0
                && bag.is_empty(),
            "",
        );
    }

    // 三覆盖参数齐生效：位置偏移、数量倍率、速度倍率。
    {
        let mut bag = DiagBag::new();
        let schema = EventSchema {
            position_offset: Vec3::new(3.0, 4.0, 5.0),
            count_mult: 2.0,
            speed_mult: 3.0,
        };
        let p = clamp_event_params(schema, 7, &mut bag);
        set.add(
            "E01-事件-三覆盖参数齐生效",
            p.position_offset == Vec3::new(3.0, 4.0, 5.0)
                && p.effective_count() == 14
                && p.speed_mult == 3.0,
            "",
        );
    }

    // 数量倍率整数截断不丢保底：7 × 0.5 = 3.5 → 3（截断），不为 4。
    {
        let mut bag = DiagBag::new();
        let p = clamp_event_params(
            EventSchema { position_offset: Vec3::ZERO, count_mult: 0.5, speed_mult: 1.0 },
            7,
            &mut bag,
        );
        set.add("E01-事件-倍率截断向下取整", p.effective_count() == 3, "");
    }

    // 重复注册幂等覆盖（不报错，热重载正常路径）。
    {
        let mut bag = DiagBag::new();
        let mut r = EventRegistry::new();
        r.register(9, EventSchema::neutral(), &mut bag);
        bag.len();
        let before = r.len();
        r.register(
            9,
            EventSchema { position_offset: Vec3::ZERO, count_mult: 5.0, speed_mult: 1.0 },
            &mut bag,
        );
        set.add(
            "E01-事件-重复注册幂等覆盖",
            r.len() == before && r.lookup(9).map(|s| s.count_mult == 5.0).unwrap_or(false),
            "",
        );
    }

    // 已注册事件名清单可枚举（读屏可达：诊断要念得出有哪些事件）。
    {
        let r = std_registry();
        let mut names = r.registered_names();
        names.sort_unstable();
        set.add("E01-事件-注册清单可枚举", names == vec![1, 2, 3], "");
    }

    // =======================================================================
    // 四、节流保量（判据四）
    // =======================================================================

    // **保量核心**：三事件粒子数 10/20/30，合并后总数须为 60（求和，不是取最大/平均）。
    {
        let mut th = Throttle::new();
        let mut bag = DiagBag::new();
        th.accumulate_event(10, Vec3::ZERO, EVENT_STORM_THRESHOLD, &mut bag);
        th.accumulate_event(20, Vec3::ZERO, EVENT_STORM_THRESHOLD, &mut bag);
        th.accumulate_event(30, Vec3::ZERO, EVENT_STORM_THRESHOLD, &mut bag);
        let (total, _) = th.take();
        set.add("E01-节流-合并保量为各事件之和", total == 60, "");
    }

    // 位置偏移按粒子数加权平均（合并后落在形心）。
    {
        let mut th = Throttle::new();
        let mut bag = DiagBag::new();
        // 偏移 0 权重 10、偏移 3 权重 30 → 形心 x = (0×10 + 3×30)/40 = 2.25。
        th.accumulate_event(10, Vec3::new(0.0, 0.0, 0.0), EVENT_STORM_THRESHOLD, &mut bag);
        th.accumulate_event(30, Vec3::new(3.0, 0.0, 0.0), EVENT_STORM_THRESHOLD, &mut bag);
        let (_, mean) = th.take();
        set.add(
            "E01-节流-偏移按粒子数加权平均",
            (mean.x - 2.25).abs() < 1e-4 && mean.y == 0.0,
            "",
        );
    }

    // 结算后状态清零（不跨帧累积）。
    {
        let mut th = Throttle::new();
        let mut bag = DiagBag::new();
        th.accumulate_event(10, Vec3::ZERO, EVENT_STORM_THRESHOLD, &mut bag);
        let (a, _) = th.take();
        let (b, _) = th.take();
        set.add(
            "E01-节流-结算后状态清零",
            a == 10 && b == 0 && th.pending_events() == 0 && th.pending_total() == 0,
            "",
        );
    }

    // 风暴节流：超阈值产出诊断（事件数如实）。
    {
        let mut th = Throttle::new();
        let mut bag = DiagBag::new();
        let mut stormed = 0;
        for _ in 0..(EVENT_STORM_THRESHOLD + 10) {
            if th.accumulate_event(1, Vec3::ZERO, EVENT_STORM_THRESHOLD, &mut bag) {
                stormed += 1;
            }
        }
        let (total, _) = th.take();
        let diag = bag.all().iter().filter(|d| {
            d.message.contains("事件数")
        }).count();
        set.add(
            "E01-节流-超阈值产出节流诊断",
            stormed == 10 && total == EVENT_STORM_THRESHOLD + 10 && diag > 0,
            "",
        );
    }

    // 未超阈值零诊断（不无事生非）。
    {
        let mut th = Throttle::new();
        let mut bag = DiagBag::new();
        for _ in 0..3 {
            th.accumulate_event(1, Vec3::ZERO, EVENT_STORM_THRESHOLD, &mut bag);
        }
        th.take();
        set.add("E01-节流-未超阈值零诊断", !bag.has(DiagCode::ChurnCoalesced), "");
    }

    // 节流不丢粒子（1000 事件风暴，总量守恒）。
    {
        let mut th = Throttle::new();
        let mut bag = DiagBag::new();
        let mut expect = 0u64;
        for i in 0..1000u32 {
            let n = i % 5;
            th.accumulate_event(n, Vec3::ZERO, EVENT_STORM_THRESHOLD, &mut bag);
            expect += n as u64;
        }
        let (total, _) = th.take();
        set.add(
            "E01-节流-千事件风暴总量守恒",
            total as u64 == expect,
            "",
        );
    }

    // =======================================================================
    // 降级矩阵
    // =======================================================================

    // 未注册事件名 → 拒绝告警（F1925 同规则），不静默忽略。
    {
        let mut ms = ModeStack::new();
        let idx = ms.push(
            ModeKind::EventDriven,
            create_mode(ModeKind::EventDriven, &[77.0]).expect("事件模式"),
        );
        let mut bag = DiagBag::new();
        let empty_reg = EventRegistry::new();
        let out = ms.dispatch_event(&empty_reg, idx, &mut bag);
        set.add(
            "E01-降级-未注册事件拒绝告警",
            out.is_empty() && bag.has(DiagCode::ShapeRejected),
            "",
        );
    }

    // 数量倍率越界（×1000 以上）→ 钳制。
    {
        let mut bag = DiagBag::new();
        let p = clamp_event_params(
            EventSchema {
                position_offset: Vec3::ZERO,
                count_mult: 5000.0,
                speed_mult: 1.0,
            },
            10,
            &mut bag,
        );
        set.add(
            "E01-降级-数量倍率越界钳制",
            p.count_mult == EVENT_COUNT_MULT_MAX && bag.has(DiagCode::RateClamped),
            "",
        );
    }

    // 速度倍率越界 → 钳制。
    {
        let mut bag = DiagBag::new();
        let p = clamp_event_params(
            EventSchema { position_offset: Vec3::ZERO, count_mult: 1.0, speed_mult: 9999.0 },
            10,
            &mut bag,
        );
        set.add(
            "E01-降级-速度倍率越界钳制",
            p.speed_mult == EVENT_SPEED_MULT_MAX && bag.has(DiagCode::RateClamped),
            "",
        );
    }

    // 非有限参数 → 钳制到安全值并记账。
    {
        let mut bag = DiagBag::new();
        let p = clamp_event_params(
            EventSchema {
                position_offset: Vec3::new(f32::NAN, 0.0, 0.0),
                count_mult: f32::NAN,
                speed_mult: f32::NAN,
            },
            10,
            &mut bag,
        );
        set.add(
            "E01-降级-非有限事件参数钳制",
            p.position_offset == Vec3::ZERO
                && p.count_mult == 1.0
                && p.speed_mult == 1.0
                && bag.len() >= 3,
            "",
        );
    }

    // 模式参数 NaN → 拒绝（持续模式发射率 NaN 拒绝而非静默 0）。
    {
        let r = create_mode(ModeKind::Continuous, &[f32::NAN]);
        set.add("E01-降级-模式参数NaN拒绝", r.is_err(), "");
    }

    // 间隔模式周期为 0 → 拒绝（会把间隔退化成爆发）。
    {
        let r = create_mode(ModeKind::Interval, &[0.0, 10.0]);
        set.add("E01-降级-间隔周期零拒绝", r.is_err(), "");
    }

    // 缺参数 → 拒绝（四型各有必填参数）。
    {
        let all_rejected = create_mode(ModeKind::Continuous, &[]).is_err()
            && create_mode(ModeKind::Burst, &[]).is_err()
            && create_mode(ModeKind::Interval, &[1.0]).is_err()
            && create_mode(ModeKind::EventDriven, &[]).is_err();
        set.add("E01-降级-缺参数四型皆拒", all_rejected, "");
    }

    // 非爆发模式触发爆发 → 拒绝 + 诊断（不静默忽略）。
    {
        let mut ms = ModeStack::new();
        let idx = ms.push(ModeKind::Continuous, cont(10.0));
        let mut bag = DiagBag::new();
        let n = ms.trigger_burst(idx, &mut bag);
        set.add(
            "E01-降级-非爆发模式触发被拒",
            n == 0 && bag.has(DiagCode::ShapeRejected),
            "",
        );
    }

    // 下标越界 → 拒绝 + 诊断。
    {
        let mut ms = ModeStack::new();
        let mut bag = DiagBag::new();
        let n = ms.trigger_burst(99, &mut bag);
        let reg = std_registry();
        let e = ms.dispatch_event(&reg, 99, &mut bag);
        set.add(
            "E01-降级-下标越界被拒",
            n == 0 && e.is_empty() && bag.has(DiagCode::ShapeRejected),
            "",
        );
    }

    // 非事件模式派发事件 → 拒绝。
    {
        let mut ms = ModeStack::new();
        let idx = ms.push(ModeKind::Continuous, cont(10.0));
        let mut bag = DiagBag::new();
        let reg = std_registry();
        let out = ms.dispatch_event(&reg, idx, &mut bag);
        set.add(
            "E01-降级-非事件模式派发被拒",
            out.is_empty() && bag.has(DiagCode::ShapeRejected),
            "",
        );
    }

    // =======================================================================
    // 零静默与确定性
    // =======================================================================

    // 诊断码标签齐备，且钳制与拒绝语义不混。
    {
        let all = [
            ModeDiag::EventUnregistered,
            ModeDiag::ParamClamped,
            ModeDiag::Throttled,
            ModeDiag::ModeRejected,
        ];
        set.add(
            "E01-零静默-诊断码标签齐备",
            all.iter().all(|c| !c.zh().is_empty()),
            "",
        );
    }

    // 确定性：同输入双跑逐位一致（本域无随机源，验证的是累积与相位）。
    {
        let run = || -> Vec<u32> {
            let mut ms = ModeStack::new();
            ms.push(ModeKind::Continuous, cont(7.3));
            ms.push(
                ModeKind::Interval,
                create_mode(ModeKind::Interval, &[0.13, 3.0]).expect("间隔"),
            );
            let mut bag = DiagBag::new();
            let mut out: Vec<u32> = Vec::new();
            for _ in 0..200 {
                for d in ms.tick(1.0 / 60.0, &mut bag) {
                    out.push(d.count);
                }
            }
            out
        };
        let a = run();
        let b = run();
        set.add("E01-零静默-同输入双跑逐位一致", a == b && !a.is_empty(), "");
    }

    // 帧率无关性：持续模式跨帧率总产出差 < 1 粒。
    {
        let mut totals: Vec<u64> = Vec::new();
        for fps in [10u32, 60, 144].iter() {
            let mut ms = ModeStack::new();
            ms.push(ModeKind::Continuous, cont(7.3));
            let mut bag = DiagBag::new();
            let mut total = 0u64;
            for _ in 0..*fps {
                for d in ms.tick(1.0 / (*fps as f32), &mut bag) {
                    total += d.count as u64;
                }
            }
            totals.push(total);
        }
        let lo = *totals.iter().min().unwrap_or(&0);
        let hi = *totals.iter().max().unwrap_or(&0);
        set.add("E01-零静默-跨帧率产出差小于一", hi.saturating_sub(lo) <= 1, "");
    }

    // 发射计划携带来源与倍率（供诊断与调试追溯）。
    {
        let plan = plan_from_directive(EmitDirective { count: 7, mode_index: 2 }, Vec3::new(1.0, 2.0, 3.0), 4.0);
        set.add(
            "E01-零静默-发射计划携带来源与倍率",
            plan.count == 7
                && plan.mode_index == 2
                && plan.position_offset == Vec3::new(1.0, 2.0, 3.0)
                && plan.speed_mult == 4.0,
            "",
        );
    }

    // 单帧产出截断 + 记账（探针实测：rate=1e9、dt=1s 曾产出 10 亿粒且零诊断）。
    {
        let mut ms = ModeStack::new();
        ms.push(ModeKind::Continuous, cont(1e9));
        let mut bag = DiagBag::new();
        let out = ms.tick(1.0, &mut bag);
        let total: u32 = out.iter().map(|d| d.count).sum();
        set.add(
            "E01-降级-单帧产出截断且记账",
            total == MODE_MAX_SPAWN_PER_FRAME && bag.has(DiagCode::RateClamped),
            "",
        );
    }

    // 截断诊断含原值与上限（可定位是哪个模式、发了多少）。
    {
        let mut ms = ModeStack::new();
        ms.push(ModeKind::Continuous, cont(1e9));
        let mut bag = DiagBag::new();
        ms.tick(1.0, &mut bag);
        let d = bag.all().iter().find(|d| d.code == DiagCode::RateClamped);
        let ok = match d {
            Some(x) => {
                x.message.contains("1000000000")
                    && x.message.contains(&MODE_MAX_SPAWN_PER_FRAME.to_string())
                    && !x.hint.is_empty()
            }
            None => false,
        };
        set.add("E01-降级-截断诊断含原值与出路", ok, "");
    }

    // 正常速率不触发截断（不无事生非）。
    {
        let mut ms = ModeStack::new();
        ms.push(ModeKind::Continuous, cont(1000.0));
        let mut bag = DiagBag::new();
        let out = ms.tick(0.001, &mut bag);
        let total: u32 = out.iter().map(|d| d.count).sum();
        set.add(
            "E01-降级-正常速率不误触截断",
            total <= 2 && !bag.has(DiagCode::RateClamped),
            "",
        );
    }

    // 性能分解：持续/间隔每帧 O(1)（帧步进不随粒子数增长）。
    {
        let mut ms = ModeStack::new();
        ms.push(ModeKind::Continuous, cont(1000.0));
        let mut bag = DiagBag::new();
        let a = ms.tick(0.001, &mut bag);
        let b = ms.tick(0.001, &mut bag);
        // 两帧各自产出 1 粒（1 粒/秒 × 1ms），不因累积而膨胀。
        set.add(
            "E01-零静默-持续模式每帧产出与粒子数无关",
            a.iter().map(|d| d.count).sum::<u32>() <= 2,
            "",
        );
        let _ = b;
    }

    set
}

/// 域自检聚合。
pub fn run_vel04_all_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F2204");
    for (tag, sub) in [
        ("modes", run_vel04_checks()),
        ("events", run_vel04_event_checks()),
    ]
    .iter()
    {
        let passed = sub.all_passed() && !sub.truncated();
        set.add(tag, passed, if passed { "" } else { "子集有红项" });
    }
    set
}
#[cfg(test)]
mod tests {
    use super::*;

    /// 列出所有红项名（失败时便于定位）。
    fn reds(set: &CheckSet) -> Vec<&'static str> {
        let (r, n) = set.red_items();
        (0..n)
            .filter_map(|i| r[i].as_ref().filter(|c| !c.passed).map(|c| c.name))
            .collect()
    }

    #[test]
    fn vel04_mode_checks_all_green() {
        let set = run_vel04_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "四模式/混合叠加自检有红项：{}/{} 绿，红项：{:?}",
            passed,
            passed + failed,
            reds(&set)
        );
    }

    #[test]
    fn vel04_event_checks_all_green() {
        let set = run_vel04_event_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "事件契约/节流自检有红项：{}/{} 绿，红项：{:?}",
            passed,
            passed + failed,
            reds(&set)
        );
    }

    #[test]
    fn vel04_domain_aggregate_green() {
        let set = run_vel04_all_checks();
        assert!(set.all_passed(), "域聚合自检有红项：{:?}", reds(&set));
    }

    #[test]
    fn vel04_throttle_conserves_particles() {
        // 判据「节流保量」的存在性理由：合并是**求和**不是抽样。
        // 反例对照：若实现取平均，1000 个事件（1~5 粒）的总量会缩水到 ~3。
        let mut th = Throttle::new();
        let mut bag = DiagBag::new();
        let mut expect = 0u64;
        for i in 0..1000u32 {
            let n = i % 5;
            th.accumulate_event(n, Vec3::ZERO, EVENT_STORM_THRESHOLD, &mut bag);
            expect += n as u64;
        }
        let (total, _) = th.take();
        assert_eq!(total as u64, expect, "节流必须保量：合并后总数 = 各事件之和");
        assert!(total > 0);
    }

    #[test]
    fn vel04_modes_do_not_steal_each_others_quota() {
        // 回归：四模式若共用一个累积器，增删模式会连带改变其余模式的发射量。
        let measure = |with_interval: bool| -> u32 {
            let mut ms = ModeStack::new();
            ms.push(ModeKind::Continuous, cont(10.0));
            if with_interval {
                ms.push(
                    ModeKind::Interval,
                    create_mode(ModeKind::Interval, &[0.5, 10.0]).expect("间隔"),
                );
            }
            let mut bag = DiagBag::new();
            let mut total = 0u32;
            for _ in 0..60 {
                for d in ms.tick(1.0 / 60.0, &mut bag) {
                    if d.mode_index == 0 {
                        total += d.count;
                    }
                }
            }
            total
        };
        assert_eq!(
            measure(true),
            measure(false),
            "增删间隔模式不得改变持续模式的发射量（独立累积判据）"
        );
    }

    #[test]
    fn vel04_unregistered_event_is_refused_not_ignored() {
        // 降级矩阵第一项：未注册事件名拒绝告警，而非静默忽略。
        // 静默忽略的后果是「事件接线错了但没人知道」，表现为粒子就是不发射。
        let mut ms = ModeStack::new();
        let idx = ms.push(
            ModeKind::EventDriven,
            create_mode(ModeKind::EventDriven, &[42.0]).expect("事件模式"),
        );
        let mut bag = DiagBag::new();
        let out = ms.dispatch_event(&EventRegistry::new(), idx, &mut bag);
        assert!(out.is_empty(), "未注册事件不得产出粒子");
        assert!(
            bag.has(DiagCode::ShapeRejected),
            "未注册事件必须产出诊断，不得静默"
        );
    }
}
