//! vef68_pdomain_checks — VE-F5416 网络与 P 域联动·判据集（16 条）。
//!
//! 判据覆盖锚点五要素：同源管道（2~5）/ 一张表（6~8）/ 门接入（9~10）/
//! 断言（1、3、11~13）/ 判据面（14~16）。

use crate::checks::CheckSet;
use crate::svstar2::vef68_pdomain::*;

/// 判据集入口（svstar2/mod.rs 折叠聚合器挂载点）。
pub fn run_vef68_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F5416");

    // 1 双写对拍（全域双写纪律第 8 单）：DOMAIN_BUDGET_MATRIX 5 格 ↔
    //   budget_rule + 五域守恒 Σ=1000‰ + STALL_MATRIX ↔ stall_rule。
    let ok1 = {
        let mut h = PDomainLink::new();
        h.selfcheck_doublewrite().is_ok()
            && budget_rule(9).is_none()
            && stall_rule(7) == 0
    };
    s.add("doublewrite_matrix_rule", ok1, "budget 5+sum 1000+stall 2");

    // 2 同源单管道：publish 唯一入口供样 → 面板快照产出 + 快照可读。
    let ok2 = {
        let mut h = PDomainLink::new();
        let r = h.publish(MetricSample {
            metric: NetMetric::RttMs,
            value: 42,
            tick: 1,
            schema_ver: SCHEMA_VER,
        });
        r.is_ok()
            && h.panel_len() == 1
            && match h.panel_snapshot(NetMetric::RttMs) {
                Ok((snap, st)) => snap.value == 42 && st == PanelState::Live.code(),
                Err(_) => false,
            }
    };
    s.add("single_pipeline_publish", ok2, "publish sole entry");

    // 3 口径版本戳断言：schema_ver 失配即口径漂移显性拒 + 计数 + 审计
    //   留痕（漂移样本不入面板）。
    let ok3 = {
        let mut h = PDomainLink::new();
        let r1 = h.publish(MetricSample {
            metric: NetMetric::RttMs,
            value: 42,
            tick: 1,
            schema_ver: 2,
        });
        let r2 = h.publish(MetricSample {
            metric: NetMetric::RttMs,
            value: 42,
            tick: 2,
            schema_ver: SCHEMA_VER,
        });
        r1 == Err(XC_PD_SCHEMA_DRIFT)
            && h.drift_rejects == 1
            && r2.is_ok()
            && match h.audit_at(0) {
                Some(a) => a.op == AuditOp::SchemaDrifted && a.arg == 2,
                None => false,
            }
    };
    s.add("schema_version_assert", ok3, "drift rejected by ver stamp");

    // 4 管道断流置灰：连续 STALL_TICKS 无样本 → 面板置灰（诚实降级——
    //   快照仍可读但状态码 Grey 不装活）。
    let ok4 = {
        let mut h = PDomainLink::new();
        let r = h.publish(MetricSample {
            metric: NetMetric::JitterMs,
            value: 7,
            tick: 1,
            schema_ver: SCHEMA_VER,
        });
        let mut k = 0;
        let mut greyed = false;
        let mut t: u64 = 2;
        while k < stall_rule(0) {
            match h.tick_advance(t) {
                Ok(g) => greyed = greyed || g,
                Err(_) => break,
            }
            t += 1;
            k += 1;
        }
        r.is_ok()
            && greyed
            && h.panel_state() == PanelState::Grey
            && h.grey_count == 1
            && match h.panel_snapshot(NetMetric::JitterMs) {
                Ok((_, st)) => st == PanelState::Grey.code(),
                Err(_) => false,
            }
    };
    s.add("panel_grey_on_stall", ok4, "stall 5 ticks -> grey");

    // 5 面板复活：置灰后恢复供样自动复活 + 计数 + 审计（PanelGreyed→
    //   PanelRevived 序）。
    let ok5 = {
        let mut h = PDomainLink::new();
        let _ = h.publish(MetricSample {
            metric: NetMetric::LossPpm,
            value: 5,
            tick: 1,
            schema_ver: SCHEMA_VER,
        });
        let mut t: u64 = 2;
        let mut k = 0;
        while k < stall_rule(0) {
            let _ = h.tick_advance(t);
            t += 1;
            k += 1;
        }
        let r = h.publish(MetricSample {
            metric: NetMetric::LossPpm,
            value: 6,
            tick: t,
            schema_ver: SCHEMA_VER,
        });
        r.is_ok()
            && h.panel_state() == PanelState::Live
            && h.revive_count == 1
            && h.grey_count == 1
            && h.audit_len() >= 4
    };
    s.add("panel_revive", ok5, "supply resumes -> live");

    // 6 一张表·注册：网络预算行注册（F4921 千分比口径）返回 150‰ +
    //   重复注册同值。
    let ok6 = {
        let mut h = PDomainLink::new();
        let b1 = h.register_budget(1);
        let b2 = h.register_budget(1);
        b1 == Ok(150) && b2 == Ok(150)
    };
    s.add("budget_register", ok6, "network row 150 ppm");

    // 7 一张表·超支仲裁：用量超预算 → 以总表为准（超支 true + 剩余 0）+
    //   仲裁计数 + 审计留痕；额度内正常。
    let ok7 = {
        let mut h = PDomainLink::new();
        let reg = h.register_budget(1);
        let a1 = h.account_usage(1, 100);
        let a2 = h.account_usage(1, 160);
        reg.is_ok()
            && match (a1, a2) {
                (Ok((o1, r1)), Ok((o2, r2))) => {
                    !o1 && r1 == 50 && o2 && r2 == 0 && h.overruled_count == 1
                }
                _ => false,
            }
    };
    s.add("budget_over_arbitration", ok7, "total table rules");

    // 8 一张表·行防伪：未注册行记账拒 + 闭集外域码注册拒。
    let ok8 = {
        let mut h = PDomainLink::new();
        h.account_usage(1, 100) == Err(XC_PD_ROW_UNKNOWN)
            && h.register_budget(9) == Err(XC_PD_ROW_UNKNOWN)
            && h.account_usage(9, 10) == Err(XC_PD_ROW_UNKNOWN)
    };
    s.add("budget_row_unknown", ok8, "unregistered rows denied");

    // 9 门联动·冲突仲裁：网络过总表不过 / 网络不过总表过 → 生效均以
    //   总表为准 + 冲突计数 + 审计。
    let ok9 = {
        let mut h = PDomainLink::new();
        let v1 = h.consume_gate(true, false);
        let v2 = h.consume_gate(false, true);
        !v1.effective_pass
            && v1.conflicted
            && v2.effective_pass
            && v2.conflicted
            && h.gate_conflicts == 2
    };
    s.add("gate_conflict_table_wins", ok9, "conflict -> table rules");

    // 10 门联动·一致直通：网络与总表判定一致 → 无冲突生效随判定。
    let ok10 = {
        let mut h = PDomainLink::new();
        let v1 = h.consume_gate(true, true);
        let v2 = h.consume_gate(false, false);
        v1.effective_pass
            && !v1.conflicted
            && !v2.effective_pass
            && !v2.conflicted
            && h.gate_conflicts == 0
    };
    s.add("gate_consistent", ok10, "aligned verdicts pass through");

    // 11 指标闭集：五指标 from_code 双向恒等 + 闭集外 None。
    let ok11 = {
        let mut acc = true;
        let mut i = 0;
        while i < NetMetric::ALL.len() {
            let m = NetMetric::ALL[i];
            acc = acc && NetMetric::from_code(m.code()) == Some(m);
            i += 1;
        }
        acc && NetMetric::from_code(5).is_none() && NetMetric::from_code(255).is_none()
    };
    s.add("metric_closed", ok11, "5 metrics closed set");

    // 12 时序纪律：publish 与 tick_advance 双面 tick 倒流拒。
    let ok12 = {
        let mut h = PDomainLink::new();
        let p1 = h.publish(MetricSample {
            metric: NetMetric::ReplQueue,
            value: 3,
            tick: 10,
            schema_ver: SCHEMA_VER,
        });
        let p2 = h.publish(MetricSample {
            metric: NetMetric::ReplQueue,
            value: 3,
            tick: 9,
            schema_ver: SCHEMA_VER,
        });
        let t1 = h.tick_advance(9);
        p1.is_ok() && p2 == Err(XC_PD_TICK_REWIND) && t1 == Err(XC_PD_TICK_REWIND)
    };
    s.add("temporal_rewind", ok12, "tick monotonic both faces");

    // 13 对拍 O(指标数)：五指标全入板 → reconcile 单遍对拍全绿。
    let ok13 = {
        let mut h = PDomainLink::new();
        let mut all_ok = true;
        let mut i = 0;
        while i < NetMetric::ALL.len() {
            let r = h.publish(MetricSample {
                metric: NetMetric::ALL[i],
                value: (i as u32) * 10,
                tick: (i as u64) + 1,
                schema_ver: SCHEMA_VER,
            });
            if r.is_err() {
                all_ok = false;
                break;
            }
            i += 1;
        }
        all_ok && h.panel_len() == 5 && h.reconcile()
    };
    s.add("reconcile_o_metrics", ok13, "single-pass 5 metrics");

    // 14 审计满容拒记：260 次 publish → 256 满 + dropped 4 + seq 严格
    //   递增 + 遥测同步满容（遥测 dropped 亦 4）。
    let ok14 = {
        let mut h = PDomainLink::new();
        let mut k: u64 = 1;
        while k <= AUDIT_CAP as u64 + 4 {
            let _ = h.publish(MetricSample {
                metric: NetMetric::BwUtilPpm,
                value: (k % 1000) as u32,
                tick: k,
                schema_ver: SCHEMA_VER,
            });
            k += 1;
        }
        let mut acc =
            h.audit_len() == AUDIT_CAP && h.audit_dropped == 4 && h.telemetry_dropped == 4;
        let mut i = 1;
        while i < h.audit_len() {
            match (h.audit_at(i - 1), h.audit_at(i)) {
                (Some(a), Some(b)) => acc = acc && b.seq > a.seq,
                _ => acc = false,
            }
            i += 1;
        }
        acc
    };
    s.add("audit_cap_seq", ok14, "cap 256 drop 4 both ledgers");

    // 15 遥测防伪：well_formed 闭集外拒 + 版本戳一致性 + 快照账可读。
    let ok15 = {
        let bad = LinkTelemetry {
            tick: 1,
            metric_code: 9,
            value: 1,
            panel_state: 0,
            schema_ver: SCHEMA_VER,
        };
        let mut h = PDomainLink::new();
        let _ = h.publish(MetricSample {
            metric: NetMetric::RttMs,
            value: 33,
            tick: 1,
            schema_ver: SCHEMA_VER,
        });
        !bad.well_formed()
            && match h.telemetry_at(0) {
                Some(t) => t.well_formed() && t.value == 33 && t.schema_ver == SCHEMA_VER,
                None => false,
            }
    };
    s.add("telemetry_wellformed", ok15, "5-field closed+ver locked");

    // 16 聚合·模块七组自检全绿。
    let ok16 = {
        let mut h = PDomainLink::new();
        h.selfcheck_all().is_ok()
    };
    s.add("selfcheck_all_agg", ok16, "7 selfcheck groups green");

    s
}
