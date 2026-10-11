//! veac03_perception_checks — VE-F5803 感知系统·判据集（18 条）。
//!
//! 判据覆盖锚点四要素：三类通道（2、5~6）/ 感知模型非真值（3）/
//! 拒绝全知（4）/ 记忆遗忘与判据（1、7~18）。

use crate::checks::CheckSet;
use crate::svstar2::veac03_perception::*;

/// 判据集入口（svstar2/mod.rs 折叠聚合器挂载点）。
pub fn run_veac03_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F5803");

    // 1 双写对拍（全域双写纪律第 7 单）：FORGET_CURVE_MATRIX 15 格 ↔
    //   retention_rule 全等 + 一半遗忘点 t=S 恒 500‰ 三档 + 闭集外 None。
    let ok1 = {
        let mut h = PerceptionHub::new();
        h.selfcheck_curve_doublewrite().is_ok()
            && retention_rule(0, 10) == Some(500)
            && retention_rule(1, 30) == Some(500)
            && retention_rule(2, 90) == Some(500)
            && retention_rule(3, 10).is_none()
    };
    s.add("curve_doublewrite", ok1, "15 cells+half-life 500");

    // 2 三类通道闭集：三通道注册 + 重复注册拒 + from_code 闭集外 None。
    let ok2 = {
        let mut h = PerceptionHub::new();
        let r1 = h.register_channel(ChannelKind::Vision);
        let r2 = h.register_channel(ChannelKind::Auditory);
        let r3 = h.register_channel(ChannelKind::Event);
        let dup = h.register_channel(ChannelKind::Vision);
        r1.is_ok()
            && r2.is_ok()
            && r3.is_ok()
            && h.channel_count() == 3
            && dup == Err(XC_PERC_CHANNEL_UNKNOWN)
            && ChannelKind::from_code(3).is_none()
            && ChannelKind::from_code(255).is_none()
            && ChannelKind::from_code(0) == Some(ChannelKind::Vision)
            && ChannelKind::from_code(1) == Some(ChannelKind::Auditory)
            && ChannelKind::from_code(2) == Some(ChannelKind::Event)
    };
    s.add("channels_closed", ok2, "3-channel closed set");

    // 3 感知模型非真值（结构级公平红线）：model_id=0 真值直通请求显性拒
    //   + 计数 + 审计留痕；非零模型标注放行。
    let ok3 = {
        let mut h = PerceptionHub::new();
        let reg = h.register_channel(ChannelKind::Vision);
        let r0 = h.perceive(1, ChannelKind::Vision, 42, 0, 900_000, 1);
        let r1 = h.perceive(2, ChannelKind::Vision, 42, 77, 900_000, 1);
        reg.is_ok()
            && r0 == Err(XC_PERC_NO_MODEL)
            && h.no_model_rejects == 1
            && r1.is_ok()
            && match h.audit_at(0) {
                Some(a) => a.op == AuditOp::NoModelRejected && a.arg == XC_PERC_NO_MODEL,
                None => false,
            }
    };
    s.add("fairness_no_truth", ok3, "model-tag mandatory");

    // 4 拒绝全知（公平红线 API 面）：omniscient_query 恒拒 + 计数累计 +
    //   审计逐次留痕。
    let ok4 = {
        let mut h = PerceptionHub::new();
        let mut k = 0;
        let mut all_deny = true;
        while k < 3 {
            if h.omniscient_query() != Err(XC_PERC_OMNISCIENT_DENY) {
                all_deny = false;
                break;
            }
            k += 1;
        }
        all_deny
            && h.omniscient_rejects == 3
            && match h.audit_at(2) {
                Some(a) => a.op == AuditOp::OmniscientRejected,
                None => false,
            }
    };
    s.add("fairness_omniscient", ok4, "omniscient always denied");

    // 5 通道降级感知：Degraded 通道置信度 80% 折减（500_000 → 400_000）。
    let ok5 = {
        let mut h = PerceptionHub::new();
        let reg = h.register_channel(ChannelKind::Auditory);
        let deg = h.set_channel_state(ChannelKind::Auditory, ChannelState::Degraded);
        let pr = h.perceive(1, ChannelKind::Auditory, 7, 101, 500_000, 1);
        match (reg, deg, pr) {
            (Ok(()), Ok(()), Ok(seq)) => match h.query_percept(seq) {
                Ok((rec, st)) => {
                    rec.confidence_ppm == 400_000
                        && rec.model_id == 101
                        && st == PerceptStatus::Fresh.code()
                }
                Err(_) => false,
            },
            _ => false,
        }
    };
    s.add("degrade_confidence", ok5, "degraded ×80% fold");

    // 6 通道失效降级：Failed 感知显性拒 + 其余通道接棒（降级感知语义）+
    //   非法迁移拒（Failed 不可复活——诚实降级不装活）。
    let ok6 = {
        let mut h = PerceptionHub::new();
        let r1 = h.register_channel(ChannelKind::Auditory);
        let r2 = h.register_channel(ChannelKind::Event);
        let fail = h.set_channel_state(ChannelKind::Auditory, ChannelState::Failed);
        let dead = h.perceive(1, ChannelKind::Auditory, 8, 101, 500_000, 1) == Err(XC_PERC_CHANNEL_DEAD);
        let alive = h.perceive(2, ChannelKind::Event, 9, 102, 800_000, 0).is_ok();
        let no_revive =
            h.set_channel_state(ChannelKind::Auditory, ChannelState::Active) == Err(XC_PERC_CHANNEL_UNKNOWN);
        r1.is_ok() && r2.is_ok() && fail.is_ok() && dead && alive && no_revive
    };
    s.add("degrade_failed_reject", ok6, "dead channel+handoff");

    // 7 记忆生命周期：S=10 档感知 → t=11 Faded（476‰）→ t=51 sweep 遗忘 →
    //   查询显性不可用 → 重复 sweep 零重复计数。
    let ok7 = {
        let mut h = PerceptionHub::new();
        let reg = h.register_channel(ChannelKind::Vision);
        let pr = h.perceive(1, ChannelKind::Vision, 42, 7, 900_000, 0);
        match (reg, pr) {
            (Ok(()), Ok(seq)) => {
                // t=12 → age=11 → 1000*10/21=476‰ → Faded（t=11 时 age=10
                // 恰一半遗忘点 500=Fresh 边界，刻意避开）。
                let mid = h.query_percept_at(seq, 12);
                let n1 = h.sweep_forget(51);
                let q51 = h.query_percept(seq);
                let n2 = h.sweep_forget(52);
                match mid {
                    Ok((_, st)) => {
                        st == PerceptStatus::Faded.code()
                            && n1 == 1
                            && h.forgotten_count == 1
                            && q51 == Err(XC_PERC_FORGOTTEN)
                            && n2 == 0
                    }
                    Err(_) => false,
                }
            }
            _ => false,
        }
    };
    s.add("forget_lifecycle", ok7, "fresh->faded->forgotten once");

    // 8 遗忘阈值边界：of_retention 四点（500 Fresh/499 Faded/200 Faded/
    //   199 Forgotten）。
    let ok8 = {
        match (
            PerceptStatus::of_retention(500),
            PerceptStatus::of_retention(499),
            PerceptStatus::of_retention(200),
            PerceptStatus::of_retention(199),
        ) {
            (
                PerceptStatus::Fresh,
                PerceptStatus::Faded,
                PerceptStatus::Faded,
                PerceptStatus::Forgotten,
            ) => true,
            _ => false,
        }
    };
    s.add("forget_threshold_boundary", ok8, "500/499/200/199 four pts");

    // 9 漂移标定：偏差累计 15_000+15_000=30_000 超阈 → 第二次显性标定
    //   返回 + 计数 + 审计留痕；单次不触发。
    let ok9 = {
        let mut h = PerceptionHub::new();
        let reg = h.register_channel(ChannelKind::Event);
        let r1 = h.report_drift(ChannelKind::Event, 15_000);
        let r2 = h.report_drift(ChannelKind::Event, 15_000);
        reg.is_ok()
            && r1.is_ok()
            && r2 == Err(XC_PERC_DRIFT_HIGH)
            && h.calibration_count == 1
            && match h.audit_at(0) {
                Some(a) => a.op == AuditOp::CalibrationRequested,
                None => false,
            }
    };
    s.add("drift_calibration", ok9, "drift->calibration request");

    // 10 时序纪律：tick 倒流拒 + 记忆强度档闭集外拒。
    let ok10 = {
        let mut h = PerceptionHub::new();
        let reg = h.register_channel(ChannelKind::Vision);
        let p1 = h.perceive(10, ChannelKind::Vision, 1, 5, 900_000, 1);
        let p2 = h.perceive(9, ChannelKind::Vision, 2, 5, 900_000, 1);
        let p3 = h.perceive(11, ChannelKind::Vision, 2, 5, 900_000, 3);
        reg.is_ok()
            && p1.is_ok()
            && p2 == Err(XC_PERC_TICK_REWIND)
            && p3 == Err(XC_PERC_STRENGTH_UNKNOWN)
    };
    s.add("temporal_rewind", ok10, "tick monotonic+strength closed");

    // 11 记忆账满容：257 条感知 → 256 入账 + dropped 1（满容拒记不覆盖）。
    let ok11 = {
        let mut h = PerceptionHub::new();
        let reg = h.register_channel(ChannelKind::Vision);
        let mut k: u64 = 1;
        let mut all_ok = true;
        while k <= (MEMORY_CAP as u64) + 1 {
            if h
                .perceive(k, ChannelKind::Vision, (k % 97) as u32, 9, 900_000, 2)
                .is_err()
            {
                all_ok = false;
                break;
            }
            k += 1;
        }
        reg.is_ok() && all_ok && h.memory_len() == MEMORY_CAP && h.memory_dropped == 1
    };
    s.add("memory_cap", ok11, "cap 256 drop 1");

    // 12 遥测防伪：well_formed 闭集外通道码 false + 正例 true +
    //   telemetry_last 快照一致。
    let ok12 = {
        let bad = PerceptionTelemetry {
            tick: 1,
            channel_code: 9,
            state_code: 0,
            content_code: 1,
            model_id: 5,
            confidence_ppm: 900_000,
            strength_idx: 1,
        };
        let good = PerceptionTelemetry {
            tick: 1,
            channel_code: 0,
            state_code: 0,
            content_code: 1,
            model_id: 5,
            confidence_ppm: 900_000,
            strength_idx: 1,
        };
        let sub = {
            let mut h = PerceptionHub::new();
            let reg = h.register_channel(ChannelKind::Vision);
            let pr = h.perceive(1, ChannelKind::Vision, 33, 5, 900_000, 2);
            match (reg, pr) {
                (Ok(_), Ok(_)) => match h.telemetry_last() {
                    Some(t) => t.well_formed() && t.content_code == 33 && t.strength_idx == 2,
                    None => false,
                },
                _ => false,
            }
        };
        !bad.well_formed() && good.well_formed() && sub
    };
    s.add("telemetry_wellformed", ok12, "7-field closed+snapshot");

    // 13 审计链 seq 严格递增（感知+拒绝混流）。
    let ok13 = {
        let mut h = PerceptionHub::new();
        let reg = h.register_channel(ChannelKind::Vision);
        let p1 = h.perceive(1, ChannelKind::Vision, 1, 5, 900_000, 1);
        let p2 = h.perceive(2, ChannelKind::Vision, 1, 0, 900_000, 1); // NoModel 拒
        let p3 = h.perceive(3, ChannelKind::Vision, 2, 6, 900_000, 1);
        let mut acc = reg.is_ok() && p1.is_ok() && p2.is_err() && p3.is_ok();
        let mut i = 1;
        while i < h.audit_len() {
            match (h.audit_at(i - 1), h.audit_at(i)) {
                (Some(a), Some(b)) => acc = acc && b.seq > a.seq,
                _ => acc = false,
            }
            i += 1;
        }
        acc && h.audit_len() >= 3
    };
    s.add("audit_seq", ok13, "seq strict mixed stream");

    // 14 审计满容拒记：260 次全知拒绝 → 256 满 + dropped 4 + 旧痕不覆盖。
    let ok14 = {
        let mut h = PerceptionHub::new();
        let mut k = 0;
        while k < AUDIT_CAP + 4 {
            let _ = h.omniscient_query();
            k += 1;
        }
        h.audit_len() == AUDIT_CAP && h.audit_dropped == 4
    };
    s.add("audit_cap", ok14, "cap 256 drop 4");

    // 15 过滤 O(事件)：filtered_count 随感知条数线性增长（单遍无嵌套）。
    let ok15 = {
        let mut h = PerceptionHub::new();
        let reg = h.register_channel(ChannelKind::Event);
        let mut k: u64 = 1;
        let mut all_ok = true;
        while k <= 50 {
            if h
                .perceive(k, ChannelKind::Event, (k % 13) as u32, 3, 850_000, 1)
                .is_err()
            {
                all_ok = false;
                break;
            }
            k += 1;
        }
        match h.find_channel(ChannelKind::Event) {
            Some(c) => reg.is_ok() && all_ok && c.filtered_count == 50,
            None => false,
        }
    };
    s.add("filter_o1", ok15, "filtered_count linear in events");

    // 16 确定性红线：同输入双跑（感知序列+降级+遗忘扫描）逐位一致。
    let ok16 = {
        fn build_run() -> Result<(u64, u32), u32> {
            let mut h = PerceptionHub::new();
            h.register_channel(ChannelKind::Vision)?;
            h.register_channel(ChannelKind::Auditory)?;
            h.perceive(1, ChannelKind::Vision, 11, 7, 900_000, 0)?;
            h.perceive(2, ChannelKind::Auditory, 22, 8, 700_000, 1)?;
            h.set_channel_state(ChannelKind::Auditory, ChannelState::Degraded)?;
            h.perceive(3, ChannelKind::Auditory, 33, 8, 600_000, 1)?;
            let _ = h.sweep_forget(60);
            let mut digest: u64 = 0xcbf29ce484222325;
            let mut i = 0;
            while i < h.memory_len() {
                if let Some(p) = h.memory_at(i) {
                    digest ^= p.confidence_ppm as u64 + p.seq * 31;
                    digest = digest.wrapping_mul(0x100000001b3);
                }
                i += 1;
            }
            Ok((digest, h.forgotten_count))
        }
        match (build_run(), build_run()) {
            (Ok(a), Ok(b)) => a == b,
            _ => false,
        }
    };
    s.add("determinism", ok16, "bit-identical double run");

    // 17 遗忘曲线可配：三档强度同 tick 保留率互异且递增（可配性查表证据：
    //   t=30 时 S=10→250、S=30→500、S=90→750）。
    let ok17 = match (
        retention_rule(0, 30),
        retention_rule(1, 30),
        retention_rule(2, 30),
    ) {
        (Some(a), Some(b), Some(c)) => a == 250 && b == 500 && c == 750 && a < b && b < c,
        _ => false,
    };
    s.add("forget_curve_configurable", ok17, "3 strengths differ @t=30");

    // 18 聚合·模块九组自检全绿。
    let ok18 = {
        let mut h = PerceptionHub::new();
        h.selfcheck_all().is_ok()
    };
    s.add("selfcheck_all_agg", ok18, "9 selfcheck groups green");

    s
}
