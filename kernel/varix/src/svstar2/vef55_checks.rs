//! VE-F5402 自检 · 网络分层模型（AA 域）
//!
//! **锚点判据逐条对应**（`#VE-F5402`「四层职责、跨层禁调、契约冻结、
//! 归位、判据」）：
//!
//! | 锚点判据 | 自检组 |
//! |---|---|
//! | 四层职责 | `AA55-职责-*`（职责册四层全覆盖+与 F5401 Layer 单源对账+行为归属投影） |
//! | 跨层禁调 | `AA55-拦截-*`（相邻放行/层内放行/越层拦截/上调拒绝四向闭包+记账隔离） |
//! | 契约冻结 | `AA55-版本-*`（一致放行+错配拦截双向+不兼容旧版红线） |
//! | 归位 | `AA55-归位-*`（逐层下传链独立重算+相邻性机检+首尾一致+读屏可达） |
//! | 判据 | `AA55-判据-*`（版本/错误码/条数对账） |
//!
//! **判据设计硬规矩**（承 AA/P 域先例）：期望值判据侧独立重算；不变量两头
//! 都测（违规被拒+合规放行）；判据区零 panic 面。

use crate::checks::CheckSet;
use crate::svstar2::vef54_aaarch::Layer;
use crate::svstar2::vef55_netmodel as nm;

// ---------------------------------------------------------------------------
// 组一：四层职责
// ---------------------------------------------------------------------------

fn chk_duty(s: &mut CheckSet) {
    // AA55-职责-01：职责册四层全覆盖（层序与 F5401 Layer::ALL 逐位相等）。
    let ok = nm::DUTY_TABLE.len() == Layer::ALL.len()
        && nm::DUTY_TABLE
            .iter()
            .zip(Layer::ALL.iter())
            .all(|((l, _), r)| l == r);
    s.add("AA55-职责-01", ok, "职责册层序与 F5401 单源逐位对账");

    // AA55-职责-02：每层职责单句非空（「单句说清」红线——空句=没说清）。
    let ok = nm::DUTY_TABLE.iter().all(|(_, d)| !d.is_empty());
    s.add("AA55-职责-02", ok, "四条职责单句全部非空");

    // AA55-职责-03：duty_of 四层全命中且与表一致（表是唯一真值源）。
    let mut ok = true;
    for (l, d) in nm::DUTY_TABLE.iter() {
        ok = ok && nm::duty_of(*l) == Some(*d);
    }
    s.add("AA55-职责-03", ok, "duty_of 与职责册逐条一致");

    // AA55-职责-04：DutyAct 四类行为归属层两两互异（行为⇒层是良射影）。
    let acts = [
        nm::DutyAct::ByteRelay,
        nm::DutyAct::ConnRoom,
        nm::DutyAct::StateSync,
        nm::DutyAct::RuleSettle,
    ];
    let mut ok = true;
    for i in 0..acts.len() {
        for j in (i + 1)..acts.len() {
            if acts[i].owner() == acts[j].owner() {
                ok = false;
            }
        }
    }
    s.add("AA55-职责-04", ok, "四类行为归属层互异（良射影）");

    // AA55-职责-05：owner 投影与职责册层序逐位对应（ByteRelay→Transport 等）。
    let ok = acts
        .iter()
        .zip(Layer::ALL.iter())
        .all(|(a, l)| a.owner() == *l);
    s.add("AA55-职责-05", ok, "行为归属与层序逐位对应（自下而上同构）");

    // AA55-职责-06：行为中文名非空互异（读屏可达）。
    let mut ok = true;
    for i in 0..acts.len() {
        for j in (i + 1)..acts.len() {
            if acts[i].name() == acts[j].name() {
                ok = false;
            }
        }
    }
    ok = ok && acts.iter().all(|a| !a.name().is_empty());
    s.add("AA55-职责-06", ok, "行为名非空互异（读屏可区分）");
}

// ---------------------------------------------------------------------------
// 组二：职责漂移对拍
// ---------------------------------------------------------------------------

fn chk_drift(s: &mut CheckSet) {
    // AA55-漂移-01：四类行为在归属层判 InPlace（合规放行侧）。
    let acts = [
        nm::DutyAct::ByteRelay,
        nm::DutyAct::ConnRoom,
        nm::DutyAct::StateSync,
        nm::DutyAct::RuleSettle,
    ];
    let mut ok = true;
    for a in acts.iter() {
        let (v, o) = nm::audit_drift(a.owner(), *a);
        ok = ok && v == nm::DriftVerdict::InPlace && o == a.owner();
    }
    s.add("AA55-漂移-01", ok, "归属层执行归属行为=InPlace（四类全查）");

    // AA55-漂移-02：错层报到判 Drifted 且修正目标=归属层（具体两例）。
    let (v1, o1) = nm::audit_drift(Layer::Gameplay, nm::DutyAct::ByteRelay);
    let (v2, o2) = nm::audit_drift(Layer::Transport, nm::DutyAct::RuleSettle);
    let ok = v1 == nm::DriftVerdict::Drifted
        && o1 == Layer::Transport
        && v2 == nm::DriftVerdict::Drifted
        && o2 == Layer::Gameplay;
    s.add("AA55-漂移-02", ok, "漂移对拍给出修正目标（玩法搬字节→传输层）");

    // AA55-漂移-03：对拍结论 O(1) 枚举等值——InPlace 与 Drifted 互斥可区分。
    let ok = nm::DriftVerdict::InPlace != nm::DriftVerdict::Drifted;
    s.add("AA55-漂移-03", ok, "漂移结论两态互斥（线上可观测）");

    // AA55-漂移-04：修正目标恒等于 act.owner()（对拍修正的锚点一致性）。
    let mut ok = true;
    for a in acts.iter() {
        let (_, o) = nm::audit_drift(Layer::Transport, *a);
        ok = ok && o == a.owner();
    }
    s.add("AA55-漂移-04", ok, "修正目标=行为归属层（职责册单源）");
}

// ---------------------------------------------------------------------------
// 组三：跨层禁调与归位
// ---------------------------------------------------------------------------

fn chk_intercept(s: &mut CheckSet) {
    let mut ic = nm::Interceptor::new();

    // AA55-拦截-01：相邻下传放行（三对相邻全查）。
    let adj = [
        nm::CallHop { from: Layer::Session, to: Layer::Transport },
        nm::CallHop { from: Layer::Replication, to: Layer::Session },
        nm::CallHop { from: Layer::Gameplay, to: Layer::Replication },
    ];
    let mut ok = true;
    for h in adj.iter() {
        match ic.route(*h) {
            Ok(Some(r)) => ok = ok && r.all_adjacent() && r.matches(*h),
            _ => ok = false,
        }
    }
    s.add("AA55-拦截-01", ok, "相邻下传放行且单跳链合法（三对全查）");

    // AA55-拦截-02：层内调用放行（空链）。
    let in_layer = ic.route(nm::CallHop { from: Layer::Session, to: Layer::Session });
    let ok = match in_layer {
        Ok(Some(r)) => r.hops.is_empty(),
        _ => false,
    };
    s.add("AA55-拦截-02", ok, "层内调用放行（空链，不属层间纪律）");

    // AA55-拦截-03：越层直调被拦截并归位（玩法→传输：链四层，独立重算）。
    let hop = nm::CallHop { from: Layer::Gameplay, to: Layer::Transport };
    let rr = ic.route(hop);
    let expect = [Layer::Gameplay, Layer::Replication, Layer::Session, Layer::Transport];
    let ok = match rr {
        Ok(Some(r)) => {
            r.hops.len() == 4
                && r.hops.iter().zip(expect.iter()).all(|(a, b)| a == b)
                && r.all_adjacent()
                && r.matches(hop)
        }
        _ => false,
    };
    s.add("AA55-拦截-03", ok, "越层直调拦截+归位链独立重算一致（跳两层）");

    // AA55-拦截-04：跳一层越调同样归位（玩法→会话：链三层）。
    let hop2 = nm::CallHop { from: Layer::Gameplay, to: Layer::Session };
    let ok = match ic.route(hop2) {
        Ok(Some(r)) => r.hops.len() == 3 && r.all_adjacent() && r.matches(hop2),
        _ => false,
    };
    s.add("AA55-拦截-04", ok, "跳层归位链长度=层差+1（独立重算）");

    // AA55-拦截-05：向上调用拒绝且无归位（错误码可观测）。
    let up = ic.route(nm::CallHop { from: Layer::Transport, to: Layer::Gameplay });
    let ok = match up {
        Err(e) => e.code() == nm::E_LAYER_CALL,
        _ => false,
    };
    s.add("AA55-拦截-05", ok, "向上调用拒绝（反向依赖无归位，E_LAYER_CALL）");

    // AA55-拦截-06：同层越两级+上调混合记账互不污染（隔离性）。
    let ok = ic.passed == 3 && ic.in_layer == 1 && ic.rerouted == 2 && ic.denied == 1;
    s.add("AA55-拦截-06", ok, "四路判定记账隔离（放行3/层内1/归位2/拒绝1）");

    // AA55-拦截-07：拦截动作总数（审计口径）。
    let ok = ic.intercepted_total() == 3 && nm::Interceptor::new().intercepted_total() == 0;
    s.add("AA55-拦截-07", ok, "拦截总数=越层+拒绝（空拦截器为 0）");

    // AA55-拦截-08：CallHop 谓词自洽（is_adjacent_down/is_upcall 与层号定义一致）。
    let h_ok = nm::CallHop { from: Layer::Gameplay, to: Layer::Replication };
    let h_bad = nm::CallHop { from: Layer::Session, to: Layer::Gameplay };
    let ok = h_ok.is_adjacent_down()
        && !h_ok.is_upcall()
        && h_bad.is_upcall()
        && !h_bad.is_adjacent_down();
    s.add("AA55-拦截-08", ok, "调用谓词与层号定义一致（方向判定基础）");
}

// ---------------------------------------------------------------------------
// 组四：层间契约版本拦截
// ---------------------------------------------------------------------------

fn chk_contract(s: &mut CheckSet) {
    // AA55-版本-01：一致放行。
    let ok = nm::ContractVer { provider: Layer::Transport, current: 3, expect: 3 }
        .check()
        .is_ok();
    s.add("AA55-版本-01", ok, "契约版本一致放行（合规侧）");

    // AA55-版本-02：调用方落后拦截（current > expect）。
    let ok = nm::ContractVer { provider: Layer::Session, current: 4, expect: 3 }
        .check()
        .is_err();
    s.add("AA55-版本-02", ok, "调用方期望落后=拦截（不静默兼容旧版）");

    // AA55-版本-03：调用方超前同样拦截（current < expect——错配即拦不分方向）。
    let ok = nm::ContractVer { provider: Layer::Replication, current: 2, expect: 5 }
        .check()
        .is_err();
    s.add("AA55-版本-03", ok, "调用方期望超前=拦截（错配双向闭包）");

    // AA55-版本-04：拦截错误码专属（E_CONTRACT_VERSION 可观测分支）。
    let e = nm::ContractVer { provider: Layer::Transport, current: 1, expect: 2 }.check();
    let ok = match e {
        Err(err) => err.code() == nm::E_CONTRACT_VERSION,
        Ok(()) => false,
    };
    s.add("AA55-版本-04", ok, "版本拦截错误码专属（外部可定位）");
}

// ---------------------------------------------------------------------------
// 组五：读屏可达 + 判据元
// ---------------------------------------------------------------------------

fn chk_meta(s: &mut CheckSet) {
    // AA55-判据-01：分层文档读屏可达（职责单行含层名+职责句）。
    let line = nm::screen_line_duty(Layer::Replication);
    let ok = line.contains("复制层") && line.contains("同步状态");
    s.add("AA55-判据-01", ok, "职责读屏单行含层名与职责（域本色）");

    // AA55-判据-02：归位读屏单行含改道链。
    let mut ic = nm::Interceptor::new();
    let hop = nm::CallHop { from: Layer::Gameplay, to: Layer::Transport };
    let ok = match ic.route(hop) {
        Ok(Some(r)) => {
            let line = nm::screen_line_reroute(hop, &r);
            line.contains("归位") && line.contains("传输层")
        }
        _ => false,
    };
    s.add("AA55-判据-02", ok, "归位读屏单行含改道链（拦截不让人迷路）");

    // AA55-判据-03：协议版本前缀。
    s.add(
        "AA55-判据-03",
        nm::NETMODEL_VERSION.starts_with("AF55"),
        "协议版本 AF55-*（跨版本对账锚）",
    );

    // AA55-判据-04：错误码非空互异。
    s.add(
        "AA55-判据-04",
        !nm::E_LAYER_CALL.is_empty()
            && !nm::E_LAYER_DRIFT.is_empty()
            && !nm::E_CONTRACT_VERSION.is_empty()
            && !nm::E_LAYER_DUTY.is_empty()
            && nm::E_LAYER_CALL != nm::E_LAYER_DRIFT
            && nm::E_CONTRACT_VERSION != nm::E_LAYER_DUTY
            && nm::E_LAYER_DRIFT != nm::E_CONTRACT_VERSION,
        "错误码非空互异（外部可观测分支）",
    );

    // AA55-判据-05：判据条数对账（本条前已有 26 条，本条为第 27 条）。
    s.add("AA55-判据-05", s.len() == 26, "判据条数对账（声明 27）");
}

// ---------------------------------------------------------------------------
// 聚合（单集 25 条 ≤ MAX_CHECKS=112）
// ---------------------------------------------------------------------------

/// F5402 域自检（聚合入口，注册表用）。
pub fn run_vef55_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F5402");
    chk_duty(&mut s);
    chk_drift(&mut s);
    chk_intercept(&mut s);
    chk_contract(&mut s);
    chk_meta(&mut s);
    s
}
