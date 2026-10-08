//! VE-F3009 自检 · 退场动效族（VE-P 域）
//!
//! **锚点判据逐条对应**（`#VE-F3009`「进退对称、护栏协议、语义延迟、
//! 超时兜底、reduce 立即、判据」）：
//!
//! | 锚点判据 | 自检组 |
//! |---|---|
//! | 进退对称 | `P09-对称-*`（双向 counterpart + 参数两侧独立取值对账 + delta 镜像） |
//! | 护栏协议 | `P09-护栏-*`（双轨状态机全路径 + 幂等 + 不可交互不可聚焦） |
//! | 语义延迟 | `P09-护栏-*`（Released 前语义恒活） |
//! | 超时兜底 | `P09-兜底-*`（deadline 独立重算 + 边界夹逼 + sweep 立案幂等） |
//! | reduce 立即 | `P09-reduce-*`（双零直达 + 双轨竞争拒绝） |
//! | 判据 | `P09-判据-*`（闭集/版本/错误码/条数对账） |
//!
//! **判据设计硬规矩**（承 P 域先例）：期望值判据侧独立重算；不变量两头都测
//! （违规被拒 + 合规放行）；阈值/松弛常量钉死具体数值；判据区零 panic 面。

use crate::checks::CheckSet;
use crate::svstar2::vep08_entry::{EntryKind, SlideDir};
use crate::svstar2::vep09_exit as ex;

// ---------------------------------------------------------------------------
// 组一：进退对称
// ---------------------------------------------------------------------------

fn chk_symmetry(s: &mut CheckSet) {
    // P09-对称-01：六对 counterpart 正向逐对（wire 对账）。
    let pairs = [
        (ex::ExitKind::FadeOut, EntryKind::FadeIn),
        (ex::ExitKind::FadeDown, EntryKind::FadeUp),
        (ex::ExitKind::ScaleOut, EntryKind::ScaleIn),
        (ex::ExitKind::SlideOut, EntryKind::SlideIn),
        (ex::ExitKind::BlurOut, EntryKind::BlurIn),
        (ex::ExitKind::ClipCollapse, EntryKind::ClipReveal),
    ];
    let mut ok = true;
    for (x, e) in pairs.iter() {
        ok = ok && x.counterpart().wire() == e.wire();
    }
    s.add("P09-对称-01", ok, "六对对称面正向逐对一致");

    // P09-对称-02：from_counterpart 反向逐对。
    let mut ok = true;
    for (x, e) in pairs.iter() {
        ok = ok && ex::ExitKind::from_counterpart(*e).map_or(false, |b| b.wire() == x.wire());
    }
    s.add("P09-对称-02", ok, "对称面反向逐对可走");

    // P09-对称-03：六型 wire 互异（冻结面）。
    let all = ex::ExitKind::all();
    let mut ok = true;
    for i in 0..all.len() {
        for j in (i + 1)..all.len() {
            if all[i].wire() == all[j].wire() {
                ok = false;
            }
        }
    }
    s.add("P09-对称-03", ok, "六型短码两两互异");

    // P09-对称-04：属性集两侧相等（转查单源——两侧独立取值对账）。
    let mut ok = true;
    for x in all.iter() {
        ok = ok && x.properties() == x.counterpart().properties();
    }
    s.add("P09-对称-04", ok, "属性集进退一致（对称单源）");

    // P09-对称-05：时长令牌两侧相等（进退同速）。
    let mut ok = true;
    for x in all.iter() {
        ok = ok && x.duration_token_id() == x.counterpart().duration_token_id();
    }
    s.add("P09-对称-05", ok, "时长令牌进退一致");

    // P09-对称-06：位移令牌两侧相等。
    let mut ok = true;
    for x in all.iter() {
        ok = ok && x.distance_token_id() == x.counterpart().distance_token_id();
    }
    s.add("P09-对称-06", ok, "位移令牌进退一致");

    // P09-对称-07：整体机检 audit_symmetry() 为空（对称成立）。
    s.add("P09-对称-07", ex::audit_symmetry().is_empty(), "对称表审计零违规");

    // P09-对称-08：exit_delta 四向 = 入场 delta 取反（镜像断言，逐向）。
    let mut ok = true;
    for d in SlideDir::ALL.iter() {
        let (ex_, ey_) = ex::exit_delta(*d);
        let (ix, iy) = d.delta();
        ok = ok && ex_ == -ix && ey_ == -iy;
    }
    s.add("P09-对称-08", ok, "slide-out 位移向量 = 入场镜像（四向逐一）");

    // P09-对称-09：具体方向抽样——slide-in 自下而上入场(delta 0,1) ⇒ 退场 (0,-1) 向下退出。
    let ok = ex::exit_delta(SlideDir::Up) == (0, -1) && ex::exit_delta(SlideDir::Right) == (1, 0);
    s.add("P09-对称-09", ok, "镜像方向抽样对账（上入⇒下出/左入⇒右出）");

    // P09-对称-10：parse 正反向（六型成功 + 未知拒）。
    let mut ok = true;
    for x in all.iter() {
        ok = ok && ex::ExitKind::parse(x.wire()) == Some(*x);
    }
    ok = ok && ex::ExitKind::parse("fade-outx").is_none() && ex::ExitKind::parse("").is_none();
    s.add("P09-对称-10", ok, "parse 往返一致且未知拒（双向）");

    // P09-对称-11：from_index 越界 None、界内 Some（枚举守卫双向）。
    let ok = ex::ExitKind::from_index(5).is_some()
        && ex::ExitKind::from_index(6).is_none()
        && ex::ExitKind::from_index(0).is_some();
    s.add("P09-对称-11", ok, "index 守卫双向（越界拒+界内收）");

    // P09-对称-12：非对称声明空理由拒（默认对称红线）。
    let ok = ex::AsymmetryNote::new(ex::ExitKind::FadeOut, "duration", "   ").is_err();
    s.add("P09-对称-12", ok, "无理由的非对称声明拒绝（含纯空白）");

    // P09-对称-13：有理由的非对称声明放行。
    let ok = ex::AsymmetryNote::new(ex::ExitKind::FadeOut, "duration", "页面级退场按路由要求放慢").is_ok();
    s.add("P09-对称-13", ok, "有理由的非对称声明放行（合规侧）");
}

// ---------------------------------------------------------------------------
// 组二：护栏协议（双轨 + 语义延迟 + 幂等）
// ---------------------------------------------------------------------------

fn chk_guard(s: &mut CheckSet) {
    // P09-护栏-01：begin 合法 → Visual 期 + deadline 独立重算。
    let g = ex::RemovalGuard::begin(7, ex::ExitKind::FadeOut, 200, 1000);
    let ok = match g {
        Ok(g) => {
            g.phase == ex::GuardPhase::Visual
                && g.deadline_ms == 1000 + 200 + ex::GUARD_SLACK_MS as u64
        }
        Err(_) => false,
    };
    s.add("P09-护栏-01", ok, "begin 状态与 deadline=start+dur+100 独立对账");

    // P09-护栏-02：duration=0 拒（零时长归 reduce 通道）。
    let ok = ex::RemovalGuard::begin(1, ex::ExitKind::FadeOut, 0, 0).is_err();
    s.add("P09-护栏-02", ok, "零时长护栏拒绝（合规归 reduce）");

    // P09-护栏-03：Visual 期不可交互 + 不可聚焦。
    let g = ex::RemovalGuard::begin(7, ex::ExitKind::FadeOut, 200, 0).unwrap_or_else(|_| {
        ex::RemovalGuard {
            element: 7,
            kind: ex::ExitKind::FadeOut,
            duration_ms: 200,
            start_ms: 0,
            deadline_ms: ex::GUARD_SLACK_MS as u64,
            phase: ex::GuardPhase::Visual,
        }
    });
    s.add(
        "P09-护栏-03",
        !g.hit_test_allowed() && !g.focusable(),
        "护栏期不可交互不可聚焦（幽灵按钮防线）",
    );

    // P09-护栏-04：Visual 期语义存活。
    s.add("P09-护栏-04", g.semantics_alive(), "动画期语义存活（读屏可读）");

    // P09-护栏-05：animation_done → SemanticsTail。
    let mut g2 = g;
    g2.animation_done();
    s.add("P09-护栏-05", g2.phase == ex::GuardPhase::SemanticsTail, "动画完成推进到语义尾巴期");

    // P09-护栏-06：animation_done 重复幂等。
    let mut g3 = ex::RemovalGuard::begin(8, ex::ExitKind::ScaleOut, 150, 0)
        .unwrap_or_else(|_| {
            ex::RemovalGuard {
                element: 8,
                kind: ex::ExitKind::ScaleOut,
                duration_ms: 150,
                start_ms: 0,
                deadline_ms: ex::GUARD_SLACK_MS as u64,
                phase: ex::GuardPhase::Visual,
            }
        });
    g3.animation_done();
    g3.animation_done();
    s.add("P09-护栏-06", g3.phase == ex::GuardPhase::SemanticsTail, "完成回执重复幂等");

    // P09-护栏-07：SemanticsTail 期语义仍活（语义延迟移除核心）。
    s.add("P09-护栏-07", g2.semantics_alive(), "动画完语义仍在（读屏上下文不丢）");

    // P09-护栏-08：release → Released 且语义死。
    let mut g4 = g3;
    let ok = g4.release().is_ok() && !g4.semantics_alive() && g4.phase == ex::GuardPhase::Released;
    s.add("P09-护栏-08", ok, "release 后语义摘除（全删）");

    // P09-护栏-09：双重 release 幂等（O(1) 状态位）。
    let ok = g4.release().is_ok() && g4.phase == ex::GuardPhase::Released;
    s.add("P09-护栏-09", ok, "双重移除幂等 Ok");

    // P09-护栏-10：松弛常量钉死 100ms（锚点红线）。
    s.add("P09-护栏-10", ex::GUARD_SLACK_MS == 100, "护栏松弛=100ms 钉死");

    // P09-护栏-11：无护栏记录的 release 拒（不假装成功）。
    let mut led = ex::GuardLedger::new();
    let ok = led.release(99).is_err();
    s.add("P09-护栏-11", ok, "无护栏记录的 release 拒绝（显性）");

    // P09-护栏-12：ledger begin → get → release 全链幂等。
    let ok = led.begin(7, ex::ExitKind::FadeDown, 120, 0).is_ok()
        && led.len() == 1
        && led.release(7).is_ok()
        && led.release(7).is_ok();
    s.add("P09-护栏-12", ok, "ledger 全链：begin/release/双删幂等");

    // P09-护栏-13：期限边界——now == deadline 不算泄漏（严格大于）。
    let g5 = ex::RemovalGuard::begin(9, ex::ExitKind::BlurOut, 100, 0)
        .unwrap_or_else(|_| {
            ex::RemovalGuard {
                element: 9,
                kind: ex::ExitKind::BlurOut,
                duration_ms: 100,
                start_ms: 0,
                deadline_ms: ex::GUARD_SLACK_MS as u64,
                phase: ex::GuardPhase::Visual,
            }
        });
    s.add("P09-护栏-13", !g5.leaked(g5.deadline_ms), "恰在护栏截止点不算泄漏（含端点）");

    // P09-护栏-14：deadline+1 即泄漏。
    let d = g5.deadline_ms + 1;
    s.add("P09-护栏-14", g5.leaked(d), "超期 1ms 即泄漏（兜底红线可达）");
}

// ---------------------------------------------------------------------------
// 组三：超时兜底 sweep
// ---------------------------------------------------------------------------

fn chk_sweep(s: &mut CheckSet) {
    let mut led = ex::GuardLedger::new();
    // 元素 1：短时长（早超时）；元素 2：长时长（未超时）。
    let b1 = led.begin(1, ex::ExitKind::FadeOut, 50, 0).is_ok();
    let b2 = led.begin(2, ex::ExitKind::ClipCollapse, 10_000, 0).is_ok();

    // P09-兜底-01：sweep 回收超时元素并入清单。
    let got = led.sweep(1000);
    s.add(
        "P09-兜底-01",
        b1 && b2 && got.len() == 1 && got[0] == 1 && led.leaks == 1,
        "sweep 强制回收超时元素并立案计数",
    );

    // P09-兜底-02：回收后语义死（强制回收=全删）。
    let dead = led.get_mut(1).map_or(false, |g| {
        g.phase == ex::GuardPhase::Released && !g.semantics_alive()
    });
    s.add("P09-兜底-02", dead, "强制回收后护栏 Released（泄漏窗口关闭）");

    // P09-兜底-03：二次 sweep 不重复计（幂等）。
    let got2 = led.sweep(2000);
    s.add(
        "P09-兜底-03",
        got2.is_empty() && led.leaks == 1,
        "sweep 幂等：已回收不再立案",
    );

    // P09-兜底-04：未超时元素不受 sweep 影响（仍 SemanticsTail/Visual 活护栏）。
    let alive = led.get_mut(2).map_or(false, |g| g.phase != ex::GuardPhase::Released);
    s.add("P09-兜底-04", alive, "未超时护栏不被误回收");

    // P09-兜底-05：无超时全空 sweep。
    // 元素 3 用长时长（deadline=100+5000+100=5200）：后续兜底-06 在 1000ms
    // sweep 时它必须仍在护栏内，隔离断言才成立。
    let mut led2 = ex::GuardLedger::new();
    let _ = led2.begin(3, ex::ExitKind::FadeDown, 5000, 100);
    s.add("P09-兜底-05", led2.sweep(300).is_empty(), "窗内 sweep 零回收零立案");

    // P09-兜底-06：多元素混合 sweep 只动超时者（隔离性）。
    let _ = led2.begin(4, ex::ExitKind::SlideOut, 10, 100);
    let got = led2.sweep(1000);
    let iso = got.len() == 1
        && got[0] == 4
        && led2.get_mut(3).map_or(false, |g| g.phase != ex::GuardPhase::Released);
    s.add("P09-兜底-06", iso, "混合场景 sweep 隔离（只回收超时者）");
}

// ---------------------------------------------------------------------------
// 组四：打断策略 / reduce / 判据
// ---------------------------------------------------------------------------

fn chk_policy_reduce_meta(s: &mut CheckSet) {
    // P09-打断-01：表六型齐全（index 全覆盖）。
    let all = ex::ExitKind::all();
    let mut ok = true;
    for x in all.iter() {
        ok = ok && ex::INTERRUPT_TABLE[x.index()].0 == *x;
    }
    s.add("P09-打断-01", ok, "打断策略表六型齐全无空洞");

    // P09-打断-02：轻型（纯透明度）→ 快速重入。
    s.add(
        "P09-打断-02",
        ex::interrupt_policy(ex::ExitKind::FadeOut) == ex::InterruptPolicy::FastReenter
            && ex::interrupt_policy(ex::ExitKind::FadeDown) == ex::InterruptPolicy::FastReenter,
        "轻型退场打断=快速重入（钉死）",
    );

    // P09-打断-03：重型（位移/形变/裁剪）→ 反向播放。
    s.add(
        "P09-打断-03",
        ex::interrupt_policy(ex::ExitKind::ScaleOut) == ex::InterruptPolicy::Reverse
            && ex::interrupt_policy(ex::ExitKind::SlideOut) == ex::InterruptPolicy::Reverse
            && ex::interrupt_policy(ex::ExitKind::BlurOut) == ex::InterruptPolicy::Reverse
            && ex::interrupt_policy(ex::ExitKind::ClipCollapse) == ex::InterruptPolicy::Reverse,
        "重型退场打断=反向播放（钉死）",
    );

    // P09-打断-04：策略短码互异。
    s.add(
        "P09-打断-04",
        ex::InterruptPolicy::Reverse.wire() != ex::InterruptPolicy::FastReenter.wire(),
        "策略短码互异（线上可区分）",
    );

    // P09-reduce-01：双零计划（零动画零尾巴）。
    s.add("P09-reduce-01", ex::reduce_plan() == (0, 0), "reduce 计划双零（立即直达）");

    // P09-reduce-02：无护栏元素 reduce 直达 Ok。
    let mut led = ex::GuardLedger::new();
    s.add("P09-reduce-02", ex::release_now(5, &mut led).is_ok(), "reduce 直达不需要护栏记录");

    // P09-reduce-03：活护栏在册时 reduce 直达拒（双轨竞争显性）。
    let _ = led.begin(6, ex::ExitKind::FadeOut, 100, 0);
    s.add("P09-reduce-03", ex::release_now(6, &mut led).is_err(), "同元素双轨竞争拒绝");

    // P09-reduce-04：已 Released 护栏后 reduce 直达 Ok（收敛）。
    let _ = led.release(6);
    s.add("P09-reduce-04", ex::release_now(6, &mut led).is_ok(), "护栏关闭后 reduce 收敛放行");

    // P09-判据-01：闭集长度 6。
    s.add(
        "P09-判据-01",
        ex::EXIT_KIND_COUNT == 6 && ex::ExitKind::all().len() == 6,
        "退场六型闭集（加型必红）",
    );

    // P09-判据-02：协议版本前缀。
    s.add(
        "P09-判据-02",
        ex::EXIT_PROTOCOL_VERSION.starts_with("P09"),
        "协议版本 P09-*（跨版本对账锚）",
    );

    // P09-判据-03：错误码非空互异。
    s.add(
        "P09-判据-03",
        !ex::E_EXIT_KIND.is_empty()
            && !ex::E_EXIT_SYMMETRY.is_empty()
            && !ex::E_GUARD_LEAK.is_empty()
            && !ex::E_EXIT_DURATION.is_empty()
            && ex::E_GUARD_LEAK != ex::E_EXIT_SYMMETRY
            && ex::E_EXIT_KIND != ex::E_EXIT_DURATION,
        "错误码非空互异（外部可观测分支）",
    );

    // P09-判据-04：判据条数对账（声明 45 = 实际；本条前已有 44 条）。
    s.add(
        "P09-判据-04",
        s.len() == 44,
        "判据条数对账（本条为第 45 条）",
    );
}

// ---------------------------------------------------------------------------
// 聚合（单集 45 条 ≤ MAX_CHECKS=112）
// ---------------------------------------------------------------------------

/// F3009 域自检（聚合入口，注册表用）。
pub fn run_vep09_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F3009");
    chk_symmetry(&mut s);
    chk_guard(&mut s);
    chk_sweep(&mut s);
    chk_policy_reduce_meta(&mut s);
    s
}
