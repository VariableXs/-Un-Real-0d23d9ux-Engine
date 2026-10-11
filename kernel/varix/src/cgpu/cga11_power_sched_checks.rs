//! cga11_power_sched_checks — CGPU-F0011 功耗感知调度·判据集（20 条）。
//!
//! 判据覆盖锚点五要素：三模式切换确定（1~4、8）/ 预测性降载提前 ≥2s
//! （9~13）/ 归因联动闭环（15~17）/ 墙精度实测（18）/ 红线与遥测全量
//! （1、14、19）。全部走隔离探针与真仓两路验证。

use crate::checks::CheckSet;
use crate::cgpu::cga11_power_sched::*;

/// 判据集入口（cgpu/mod.rs 折叠聚合器挂载点）。
pub fn run_cga11_checks() -> CheckSet {
    let mut s = CheckSet::new("CGPU-F0011");

    // 1 红线闸：五族探索性硬件写全口拒 + 留痕 + 计数 + 非禁写放行。
    let ok1 = {
        let mut p = PowerScheduler::new();
        let mut acc = true;
        let mut i = 0;
        while i < FORBIDDEN_WRITE_OPS.len() {
            acc = acc && p.red_line_gate(FORBIDDEN_WRITE_OPS[i]) == Err(XC_PWR_RED_LINE_WRITE);
            i += 1;
        }
        acc = acc
            && p.red_line_rejects == 5
            && p.red_line_gate("freq_read").is_ok()
            && match p.audit_at(0) {
                Some(a) => a.op == AuditOp::RedLineRejected,
                None => false,
            };
        acc
    };
    s.add("red_line_gate", ok1, "5 forbidden writes rejected");

    // 2 双写对拍（全域双写纪律第 6 单）：POWER_WALL_MATRIX ↔ wall_rule 三格
    //   + BACKGROUND_SHARE_MATRIX ↔ background_share_rule 六格逐格全等。
    let ok2 = {
        let mut acc = true;
        let mut i = 0;
        while i < POWER_WALL_MATRIX.len() {
            let (m, w) = POWER_WALL_MATRIX[i];
            acc = acc && wall_rule(m) == w;
            i += 1;
        }
        let mut j = 0;
        while j < BACKGROUND_SHARE_MATRIX.len() {
            let (m, over, share) = BACKGROUND_SHARE_MATRIX[j];
            acc = acc && background_share_rule(m, over) == share;
            j += 1;
        }
        acc
    };
    s.add("doublewrite_matrix_rule", ok2, "wall 3 cells+share 6 cells");

    // 3 三模式切换确定：幂等重设零副作用 + 切换必留痕 + 墙瓦特换算
    //   （Balanced 200W / Battery 125W / Performance 语义钳制锚）。
    let ok3 = {
        let mut p = PowerScheduler::new();
        p.set_mode(PowerMode::Balanced);
        p.set_mode(PowerMode::Balanced);
        let idem = p.mode_switches == 0;
        p.set_mode(PowerMode::Performance);
        let sw1 = p.mode_switches == 1 && p.mode() == PowerMode::Performance;
        p.set_mode(PowerMode::Battery);
        let sw2 = p.mode_switches == 2 && p.mode() == PowerMode::Battery;
        idem
            && sw1
            && sw2
            && wall_watts_of(PowerMode::Balanced) == 200
            && wall_watts_of(PowerMode::Battery) == 125
            && wall_watts_of(PowerMode::Performance) == PERF_UNLIMITED_WATTS
            && match p.audit_at(1) {
                Some(a) => a.op == AuditOp::ModeChanged && a.arg == 2,
                None => false,
            }
    };
    s.add("mode_switch_determinism", ok3, "idempotent+audit+watts");

    // 4 闭集防伪：模式码/动作码 from_code 闭集外 None（遥测防伪根基）。
    let ok4 = {
        PowerMode::from_code(0).is_some()
            && PowerMode::from_code(2).is_some()
            && PowerMode::from_code(3).is_none()
            && PowerMode::from_code(255).is_none()
            && ThrottleAction::from_code(0).is_some()
            && ThrottleAction::from_code(3).is_some()
            && ThrottleAction::from_code(4).is_none()
            && ThrottleAction::from_code(255).is_none()
    };
    s.add("codeset_closed", ok4, "mode/action codes closed");

    // 5 均衡超墙响应：Balanced 200W 墙，210W 超墙 → ThrottleBackground。
    let ok5 = {
        let mut p = PowerScheduler::new();
        p.set_mode(PowerMode::Balanced);
        let smp = p.sample(fx_sample(1, 210, 70_000, 800_000));
        let d = p.decide();
        smp.is_ok()
            && match d {
                Ok(d) => d.action == ThrottleAction::ThrottleBackground && d.over_wall && p.wall_exceeds == 1,
                Err(_) => false,
            }
    };
    s.add("wall_response_balanced", ok5, "background first on over-wall");

    // 6 续航超墙响应：Battery 125W 墙，130W 超墙 → DegradeHandoff + 计数。
    let ok6 = {
        let mut p = PowerScheduler::new();
        p.set_mode(PowerMode::Battery);
        let smp = p.sample(fx_sample(1, 130, 70_000, 800_000));
        let d = p.decide();
        smp.is_ok()
            && match d {
                Ok(d) => {
                    d.action == ThrottleAction::DegradeHandoff
                        && p.handoff_count == 1
                        // 审计序：[0]=ModeChanged（set_mode）→ [1]=WallExceeded
                        // → [2]=DegradeHandoff。
                        && match (p.audit_at(1), p.audit_at(2)) {
                            (Some(a1), Some(a2)) => {
                                a1.op == AuditOp::WallExceeded && a2.op == AuditOp::DegradeHandoff
                            }
                            _ => false,
                        }
                }
                Err(_) => false,
            }
    };
    s.add("wall_response_battery", ok6, "degrade handoff on over-wall");

    // 7 性能模式不设限：超大瓦特也不超墙不降载（只记录）。
    let ok7 = {
        let mut p = PowerScheduler::new();
        p.set_mode(PowerMode::Performance);
        let smp = p.sample(fx_sample(1, 2000, 70_000, 900_000));
        let d = p.decide();
        !p.over_wall(24000)
            && smp.is_ok()
            && match d {
                Ok(d) => d.action == ThrottleAction::None && !d.over_wall,
                Err(_) => false,
            }
    };
    s.add("performance_no_throttle", ok7, "unlimited mode records only");

    // 8 后台份额阶梯：五点采样（Balanced 60/30、Battery 40/20、Perf 100）。
    let ok8 = {
        let a = background_share_rule(0, false) == 100
            && background_share_rule(0, true) == 100
            && background_share_rule(1, false) == 60
            && background_share_rule(1, true) == 30
            && background_share_rule(2, false) == 40
            && background_share_rule(2, true) == 20;
        let mut p = PowerScheduler::new();
        p.set_mode(PowerMode::Balanced);
        a && p.background_share(190) == 60 && p.background_share(210) == 30
    };
    s.add("background_share_ladder", ok8, "60/30/40/20/100 ladder");

    // 9 热趋势外推：+250 milli_c/tick 序列 → 斜率 250_000（×1000 定点）+
    //   TTI 73 tick（×100 定点 7300）。
    let ok9 = {
        let mut p = PowerScheduler::new();
        let mut t = 1;
        let mut push_ok = true;
        while t <= 8 {
            let r = p.sample(fx_sample(t as u64, 200, 75_000 + (t as u32 - 1) * 250, 900_000));
            if r.is_err() {
                push_ok = false;
            }
            t += 1;
        }
        let slope = p.window().slope_per_tick();
        let tti = p.window().tti_ticks();
        push_ok
            && match (slope, tti) {
                (Ok((sl, _, _)), Ok(Some(tti_x100))) => sl == 250_000 && tti_x100 / 100 == 73,
                _ => false,
            }
    };
    s.add("temp_extrapolation", ok9, "slope 250/tick TTI 73");

    // 10 预测性降载提前 ≥2s：TTI 73 tick 触发 PredictiveThrottle，提前量
    //     73×100ms = 7300ms ≥ 2000ms（锚点判据的实测面）+ 计数。
    let ok10 = {
        let mut p = PowerScheduler::new();
        p.set_mode(PowerMode::Performance);
        let mut t = 1;
        while t <= 8 {
            let _ = p.sample(fx_sample(t as u64, 200, 75_000 + (t as u32 - 1) * 250, 900_000));
            t += 1;
        }
        let d = p.decide();
        match d {
            Ok(d) => {
                d.action == ThrottleAction::ThrottleGlobal
                    && p.last_predict_ahead_ticks >= PREDICT_MIN_AHEAD_TICKS
                    && p.last_predict_ahead_ticks * TICK_MS >= 2000
                    && p.last_predict_ahead_ticks == 73
                    && p.predictive_count == 1
                    // 审计序：[0]=ModeChanged（set_mode 留痕）→ [1]=PredictiveThrottle。
                    && match (p.audit_at(0), p.audit_at(1)) {
                        (Some(a0), Some(a1)) => {
                            a0.op == AuditOp::ModeChanged
                                && a1.op == AuditOp::PredictiveThrottle
                                && a1.arg == 73
                        }
                        _ => false,
                    }
            }
            Err(_) => false,
        }
    };
    s.add("predictive_ahead_2s", ok10, "ahead 7300ms >= 2000ms");

    // 11 预测前置约束：窗口未满拒（不猜）+ 负斜率无趋势 None。
    let ok11 = {
        let mut p = PowerScheduler::new();
        let underfull = p.window().tti_ticks() == Err(XC_PWR_WINDOW_UNDERFULL);
        let mut q = PowerScheduler::new();
        let mut t = 1;
        while t <= 8 {
            let _ = q.sample(fx_sample(t as u64, 200, 80_000 - (t as u32 - 1) * 100, 900_000));
            t += 1;
        }
        let cooling = q.window().tti_ticks() == Ok(None);
        let d = q.decide();
        underfull
            && cooling
            && match d {
                Ok(d) => d.action == ThrottleAction::None,
                Err(_) => false,
            }
    };
    s.add("window_underfull_reject", ok11, "no guess on short window");

    // 12 应急区兜底：距热墙 ≤3°C → ThrottleGlobal（错过预测窗口的兜底）。
    let ok12 = {
        let mut p = PowerScheduler::new();
        p.set_mode(PowerMode::Performance);
        let mut t = 1;
        while t <= 8 {
            let _ = p.sample(fx_sample(t as u64, 200, 90_000 + (t as u32 - 1) * 400, 900_000));
            t += 1;
        }
        let d = p.decide();
        match d {
            Ok(d) => d.action == ThrottleAction::ThrottleGlobal,
            Err(_) => false,
        }
    };
    s.add("emergency_fallback", ok12, "near-wall global throttle");

    // 13 降载斜率限制：目标 60% 每 tick ≤10% 渐进（10→20——帧率悬崖防护）。
    let ok13 = {
        let mut p = PowerScheduler::new();
        p.set_mode(PowerMode::Performance);
        let mut t = 1;
        while t <= 8 {
            let _ = p.sample(fx_sample(t as u64, 200, 75_000 + (t as u32 - 1) * 250, 900_000));
            t += 1;
        }
        let d1 = p.decide();
        let d2 = p.decide();
        match (d1, d2) {
            (Ok(a), Ok(b)) => {
                a.throttle_pct == MAX_THROTTLE_STEP_PCT
                    && b.throttle_pct == 2 * MAX_THROTTLE_STEP_PCT
            }
            _ => false,
        }
    };
    s.add("throttle_step_limit", ok13, "ramp 10%/tick no cliff");

    // 14 遥测全量：七元记录入账 + well_formed 防伪正反例 + 字段闭集。
    let ok14 = {
        let mut p = PowerScheduler::new();
        p.set_mode(PowerMode::Balanced);
        let _ = p.sample(fx_sample(1, 210, 70_000, 800_000));
        let d = p.decide();
        let good = PowerTelemetryRecord {
            tick: 1,
            mode_code: 1,
            watts: 210,
            milli_c: 70_000,
            busy_ppm: 800_000,
            wall_watts: 200,
            action_code: 1,
        };
        let bad = PowerTelemetryRecord {
            tick: 1,
            mode_code: 9,
            watts: 210,
            milli_c: 70_000,
            busy_ppm: 800_000,
            wall_watts: 200,
            action_code: 0,
        };
        let bad2 = PowerTelemetryRecord {
            tick: 1,
            mode_code: 1,
            watts: 210,
            milli_c: 70_000,
            busy_ppm: 800_000,
            wall_watts: 200,
            action_code: 7,
        };
        match d {
            Ok(_) => {
                p.telemetry_len() == 1
                    && good.well_formed()
                    && !bad.well_formed()
                    && !bad2.well_formed()
                    && match p.telemetry_at(0) {
                        Some(r) => r.watts == 210 && r.wall_watts == 200 && r.mode_code == 1,
                        None => false,
                    }
            }
            Err(_) => false,
        }
    };
    s.add("telemetry_seven_fields", ok14, "7-field well-formed+full");

    // 15 归因闭环：遥测 → attribute_stall → 证据 tick 回指（后台降载 →
    //     PowerWallClamp，证据动作码回读）。
    let ok15 = {
        let mut p = PowerScheduler::new();
        p.set_mode(PowerMode::Balanced);
        let _ = p.sample(fx_sample(1, 210, 70_000, 800_000));
        let _ = p.decide();
        match p.attribute_stall(1) {
            Ok(a) => {
                a.cause == StallCause::PowerWallClamp
                    && a.evidence_tick == 1
                    && a.evidence_action == ThrottleAction::ThrottleBackground.code()
            }
            Err(_) => false,
        }
    };
    s.add("attribution_loop", ok15, "telemetry->attr->evidence");

    // 16 热降频归因：温度近墙 + ThrottleGlobal → ThermalThrottle（F0007
    //     "热降频"归因直接消费本域遥测——锚点闭环面）。
    let ok16 = {
        let mut p = PowerScheduler::new();
        p.set_mode(PowerMode::Performance);
        let mut t = 1;
        while t <= 8 {
            let _ = p.sample(fx_sample(t as u64, 200, 92_000 + (t as u32 - 1) * 200, 900_000));
            t += 1;
        }
        let _ = p.decide();
        match p.attribute_stall(8) {
            Ok(a) => a.cause == StallCause::ThermalThrottle,
            Err(_) => false,
        }
    };
    s.add("attribution_thermal", ok16, "F0007 thermal attribution");

    // 17 归因边界：无功耗证据 tick → Unrelated；遥测缺位 → 显性拒。
    let ok17 = {
        let mut p = PowerScheduler::new();
        p.set_mode(PowerMode::Performance);
        let _ = p.sample(fx_sample(1, 100, 50_000, 300_000)); // 无窗不满预测
        let _ = p.decide(); // 动作 None
        match (p.attribute_stall(1), p.attribute_stall(99)) {
            (Ok(a), Err(e)) => a.cause == StallCause::Unrelated && e == XC_PWR_ATTR_NO_EVIDENCE,
            _ => false,
        }
    };
    s.add("attribution_unrelated", ok17, "unrelated+no-evidence");

    // 18 墙精度实测：drift_ppm 三例（±2.5% 命中容差 / 10% 超容差）+
    //   sample 期记账（wall_drift_samples 增长）。
    let ok18 = {
        let d1 = drift_ppm(205, 200) == 25_000;
        let d2 = drift_ppm(195, 200) == 25_000;
        let d3 = drift_ppm(220, 200) > WALL_ACCURACY_TOL * 100;
        let mut p = PowerScheduler::new();
        p.set_mode(PowerMode::Balanced);
        let _ = p.sample(fx_sample(1, 205, 70_000, 800_000));
        let acc = p.wall_drift_samples == 1 && p.wall_drift_ppm_acc == 25_000;
        d1 && d2 && d3 && acc
    };
    s.add("wall_accuracy", ok18, "drift ppm measured");

    // 19 审计链：260 次红线拒绝 → 账面 256 满 + dropped 4 + seq 严格递增
    //     + 旧痕不覆盖（首条必为 RedLineRejected）。
    let ok19 = {
        let mut p = PowerScheduler::new();
        let mut k = 0;
        while k < AUDIT_CAP + 4 {
            let _ = p.red_line_gate("msr_write");
            k += 1;
        }
        let mut acc = p.audit_len() == AUDIT_CAP && p.audit_dropped == 4;
        let mut i = 1;
        while i < p.audit_len() {
            match (p.audit_at(i - 1), p.audit_at(i)) {
                (Some(a), Some(b)) => acc = acc && b.seq > a.seq,
                _ => acc = false,
            }
            i += 1;
        }
        acc = acc
            && match p.audit_at(0) {
                Some(a) => a.op == AuditOp::RedLineRejected,
                None => false,
            };
        acc
    };
    s.add("audit_cap_seq", ok19, "cap 256 drop 4 seq strict");

    // 20 聚合·模块十组自检全绿。
    let ok20 = {
        let mut p = PowerScheduler::new();
        p.selfcheck_all().is_ok()
    };
    s.add("selfcheck_all_agg", ok20, "10 selfcheck groups green");

    s
}
