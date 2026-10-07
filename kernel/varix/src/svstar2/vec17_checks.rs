//! VE-F0417 · 域自检（判据逐条对应，见 `vec17_recover.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 三策略 → `C17-策略-*`（三策略各自按类别选用、缺分号类选删除、不可识别字符
//!   类选替换、未闭合类与括号类选再同步、指令类选再同步、表外缺配保守删除、
//!   表顺序敏感、更长前缀先命中、类别与族对齐无分叉）
//! - 显性计数 → `C17-计数-*`（通知数等于动作数、三策略之和等于总数、通知带
//!   位置/策略/代价/依据、代价三态互不相同、无静默恢复）
//! - 级联反馈 → `C17-级联-*`（窗口截断为常量、只数阻断级、阈值裁定、爆发回退、
//!   未爆发记稳定点、回退无稳定点时转兜底）
//! - 死循环兜底 → `C17-兜底-*`（预算耗尽强制兜底、兜底游标严格前进、上界饱和
//!   不回绕、停滞如实停、同步点严格大于游标）
//! - 零静默 → `C17-显性-*`（缺配注记、无同步点注记、停滞注记、渲染非空）

// no_std 下 std prelude 不存在：`String` 与 `vec!` 都得显式引入。本条自检用
// `Vec::new()` 而非 `vec![]`，故只需 `alloc::string` 侧由被测模块自带。
use super::vec16_report::Severity;
use super::vec17_recover::*;
use crate::checks::CheckSet;

/// 一组典型同步点（升序：分号、行尾、块尾、文件尾）。
fn syncs() -> [SyncPoint; 4] {
    [
        SyncPoint::new(SyncKind::Semicolon, 4, 40),
        SyncPoint::new(SyncKind::LineEnd, 9, 90),
        SyncPoint::new(SyncKind::BlockEnd, 17, 170),
        SyncPoint::new(SyncKind::Eof, 30, 300),
    ]
}

/// VE-F0417 域自检。
pub fn run_vec17_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vec17");

    // ---- 判据：三策略 ----

    // 缺分号类 → 单记号删除
    let (c1, s1, h1) = strategy_for("VE-F0409-MISSING-SEMICOLON");
    set.add(
        "C17-策略-缺分号类选单记号删除",
        c1 == RecoveryClass::MissingSeparator && s1 == RecoveryStrategy::TokenDelete && h1,
        "",
    );

    // 不可识别字符类 → 记号替换
    let (c2, s2, h2) = strategy_for("VE-F0403-UNKNOWN-TOKEN");
    set.add(
        "C17-策略-不可识别字符类选记号替换",
        c2 == RecoveryClass::UnrecognizedChar && s2 == RecoveryStrategy::TokenReplace && h2,
        "",
    );

    // 未闭合字面量类 → 再同步（不留前向扫描的独苗）
    let (c3, s3, _) = strategy_for("VE-F0407-UNTERMINATED");
    set.add(
        "C17-策略-未闭合字面量类选再同步",
        c3 == RecoveryClass::UnterminatedLiteral && s3 == RecoveryStrategy::SyncResync,
        "",
    );

    // 括号失配类 → 再同步
    let (c3b, s3b, _) = strategy_for("VE-F0410-MISMATCH");
    set.add(
        "C17-策略-括号失配类选再同步",
        c3b == RecoveryClass::BracketMismatch && s3b == RecoveryStrategy::SyncResync,
        "",
    );

    // 指令类 → 再同步
    let (c4, s4, _) = strategy_for("VE-F0411-UNKNOWN-DIRECTIVE");
    set.add(
        "C17-策略-指令语法类选再同步",
        c4 == RecoveryClass::DirectiveSyntax && s4 == RecoveryStrategy::SyncResync,
        "",
    );

    // 三策略都被真正用上（否则「三策略」只是枚举里躺着）
    let used: [RecoveryStrategy; 3] = [s1, s2, s3];
    let mut distinct = 0usize;
    let mut k = 0usize;
    while k < used.len() {
        let mut m = 0usize;
        while m < used.len() {
            if used[m] == used[k] {
                break;
            }
            m += 1;
        }
        if m == k {
            distinct += 1;
        }
        k += 1;
    }
    set.add("C17-策略-三策略均被类别实际选用", distinct == 3, "");

    // 表外缺配 → 保守单记号删除（表外码，绝不与表内元素混淆）
    let (c5, s5, h5) = strategy_for("VE-F0418-PERF-SOMETHING");
    set.add(
        "C17-策略-表外缺配保守单记号删除",
        c5 == RecoveryClass::Unclassified && s5 == RecoveryStrategy::TokenDelete && !h5,
        "",
    );

    // 策略表条目必须都能落地成非空依据（不得有空依据的条目）
    let mut empty_rationale = 0usize;
    let mut ri = 0usize;
    while ri < RECOVERY_TABLE.len() {
        if RECOVERY_TABLE[ri].rationale.is_empty() {
            empty_rationale += 1;
        }
        ri += 1;
    }
    set.add("C17-策略-表内依据无空项", empty_rationale == 0, "");

    // 表顺序敏感：更长前缀必须排在更短前缀之前，否则长码会被短前缀抢走
    let mut order_bad = 0usize;
    let mut oi = 0usize;
    while oi + 1 < RECOVERY_TABLE.len() {
        let cur = RECOVERY_TABLE[oi].code_prefix;
        let nxt = RECOVERY_TABLE[oi + 1].code_prefix;
        if nxt.starts_with(cur) {
            order_bad += 1;
        }
        oi += 1;
    }
    set.add("C17-策略-表顺序敏感（长前缀在前）", order_bad == 0, "");

    // 三策略的「代价」文案互不相同（否则「告知代价」等于没告知）
    let costs = [
        RecoveryStrategy::TokenDelete.cost(),
        RecoveryStrategy::TokenReplace.cost(),
        RecoveryStrategy::SyncResync.cost(),
    ];
    let mut cost_same = 0usize;
    let mut ci = 0usize;
    while ci < 3 {
        let mut cj = ci + 1;
        while cj < 3 {
            if costs[ci] == costs[cj] {
                cost_same += 1;
            }
            cj += 1;
        }
        ci += 1;
    }
    set.add("C17-策略-三策略代价文案互不相同", cost_same == 0, "");

    // 类别→族映射必须与上游 F0416 族表对齐（跨域分叉探测器）
    let gaps = table_alignment_gaps();
    set.add("C17-策略-类别与族表对齐无分叉", gaps.is_empty(), "");

    // ---- 判据：显性计数 ----

    let mut sess = RecoverySession::new(100);
    let a1 = sess.recover("VE-F0409-MISSING-SEMICOLON", Cursor::new(0, 0), 1, &syncs());
    let a2 = sess.recover("VE-F0403-UNKNOWN-TOKEN", Cursor::new(1, 10), 1, &syncs());
    let a3 = sess.recover("VE-F0407-UNTERMINATED", Cursor::new(2, 20), 3, &syncs());
    let cnt = *sess.counters();
    set.add("C17-计数-通知数等于动作数（无静默恢复）", cnt.notice_covers_all(), "");
    set.add("C17-计数-三策略之和等于总数", cnt.strategy_sum_matches(), "");
    set.add(
        "C17-计数-分策略计数正确",
        cnt.token_delete == 1 && cnt.token_replace == 1 && cnt.sync_resync == 1,
        "",
    );

    // 三策略的落脚行为各不相同：删/替各走一步，再同步跳到同步点之后
    set.add(
        "C17-策略-删与替各前进一记号",
        a1.after.token_index == 1 && a2.after.token_index == 2,
        "",
    );
    set.add(
        "C17-策略-再同步落脚在同步点之后",
        a3.sync_point.map(|s| s.token_index) == Some(4) && a3.after.token_index == 5,
        "",
    );

    // 通知必须带 位置/策略/代价/依据 四要素
    let n = &a1.notice;
    set.add(
        "C17-计数-通知四要素齐备",
        !n.cost.is_empty() && !n.rationale.is_empty() && !n.strategy.label().is_empty(),
        "",
    );

    // 缺配码的通知必须带注记（如实说「表外缺配」）
    let mut sess2 = RecoverySession::new(100);
    let an = sess2.recover("VE-F0418-PERF-SOMETHING", Cursor::new(0, 0), 1, &syncs());
    set.add(
        "C17-显性-表外缺配出注记",
        !an.notice.notes.is_empty() && strategy_reason("VE-F0418-PERF-SOMETHING").contains("表外缺配"),
        "",
    );

    // 无同步点时按单步前进兜底并出注记（不是静默跳）
    let empty: [SyncPoint; 0] = [];
    let mut sess3 = RecoverySession::new(100);
    let ae = sess3.recover("VE-F0410-MISMATCH", Cursor::new(0, 0), 1, &empty);
    set.add(
        "C17-显性-无可用同步点出注记",
        !ae.notice.notes.is_empty() && ae.after.token_index == 1,
        "",
    );

    // ---- 判据：级联反馈 ----

    // 窗口是常量（级联只问紧邻几步，不问全文）
    set.add(
        "C17-级联-窗口为固定常量",
        CASCADE_WINDOW == 4 && CASCADE_BURST_THRESHOLD == 3,
        "",
    );

    // 只数阻断级：三条 Error + 十条 Note 仍不爆发
    let mut mixed: Vec<Severity> = Vec::new();
    let mut mi = 0usize;
    while mi < 3 {
        mixed.push(Severity::Error);
        mi += 1;
    }
    let mut mn = 0usize;
    while mn < 10 {
        mixed.push(Severity::Note);
        mn += 1;
    }
    let vm = evaluate_cascade(&mixed);
    set.add(
        "C17-级联-窗口外与注记不计入",
        vm.observed == CASCADE_WINDOW && vm.blocking == 3 && vm.burst,
        "",
    );

    // 全 Warning 不爆发（注记/警告不是级联）
    let all_warn = [Severity::Warning, Severity::Warning, Severity::Warning, Severity::Warning];
    let vw = evaluate_cascade(&all_warn);
    set.add("C17-级联-全警告不判爆发", !vw.burst && vw.blocking == 0, "");

    // 阈值边界：恰好 3 条 Error 爆发，2 条不爆发（真边界，非自证）
    let two = [Severity::Error, Severity::Error, Severity::Note, Severity::Note];
    let v2 = evaluate_cascade(&two);
    let three = [Severity::Error, Severity::Error, Severity::Error, Severity::Note];
    let v3 = evaluate_cascade(&three);
    set.add(
        "C17-级联-阈值边界（2不爆发3爆发）",
        !v2.burst && v3.burst,
        "",
    );

    // 未爆发 → 记稳定点；爆发 → 回退到稳定点
    let mut sess4 = RecoverySession::new(100);
    let act4 = sess4.recover("VE-F0409-MISSING-SEMICOLON", Cursor::new(0, 0), 1, &syncs());
    let v4 = sess4.observe(act4.after, &all_warn);
    let sp = sess4.stable_point();
    set.add(
        "C17-级联-未爆发记稳定点",
        !v4.burst && sp.is_some() && sp.map(|p| p.cursor) == Some(act4.after),
        "",
    );
    let rb = sess4.rollback_to_stable();
    set.add(
        "C17-级联-回退落到稳定点",
        rb == Some(act4.after) && sess4.counters().rollback == 1,
        "",
    );

    // 无稳定点时回退返回 None（不得原地重试）
    let mut sess5 = RecoverySession::new(100);
    set.add("C17-级联-无稳定点回退返回None", sess5.rollback_to_stable().is_none(), "");

    // ---- 判据：死循环兜底 ----

    // 同步点必须严格大于游标（否则「跳过去」等于没跳）
    let nxt = next_sync_after(&syncs(), Cursor::new(4, 40));
    set.add(
        "C17-兜底-同步点严格大于游标",
        nxt.is_some() && nxt.map(|s| s.token_index > 4).unwrap_or(false),
        "",
    );

    // 预算耗尽 → 强制兜底（同一位置反复恢复）
    let mut sess6 = RecoverySession::with_budget(100, 2);
    let _r1 = sess6.recover("VE-F0409-MISSING-SEMICOLON", Cursor::new(0, 0), 1, &syncs());
    let _r2 = sess6.recover("VE-F0409-MISSING-SEMICOLON", Cursor::new(0, 0), 1, &syncs());
    let r3 = sess6.recover("VE-F0409-MISSING-SEMICOLON", Cursor::new(0, 0), 1, &syncs());
    set.add(
        "C17-兜底-预算耗尽强制兜底",
        r3.forced && sess6.counters().forced >= 1,
        "",
    );

    // 兜底游标必须严格前进（可终止性的构造性保证）。
    // 关键：**游标由会话自己延续**（用上一次的 `after` 作为下一次的输入），
    // 而不是外部手工喂不同的下标——手工喂下标时即使内部「不前进就强制 advance」
    // 的兜底被删掉，本项也会照样绿，等于没测到那条保证（实测：删掉兜底后本项
    // 仍全绿，是被变体测试抓出来的假门禁）。
    let mut sess7 = RecoverySession::with_budget(100, 0);
    let first = sess7.recover("VE-F0409-MISSING-SEMICOLON", Cursor::new(0, 0), 1, &syncs());
    let mut cur = first.after;
    let mut mono = first.progressed();
    let mut it = 0usize;
    while it < 8 {
        let a = sess7.recover("VE-F0409-MISSING-SEMICOLON", cur, 1, &syncs());
        if !a.stalled && !a.progressed() {
            mono = false;
        }
        cur = a.after;
        if a.stalled {
            break;
        }
        it += 1;
    }
    set.add("C17-兜底-兜底游标严格前进", mono, "");

    // 上界饱和：input_len=usize::MAX 不得回绕成 0（回绕会让停滞判定失效→真死循环）
    let sat = advance(Cursor::new(usize::MAX - 1, usize::MAX - 1), 8, usize::MAX);
    set.add("C17-兜底-上界饱和不回绕", sat.token_index == usize::MAX, "");

    // 钳制必须真正生效：游标**已经越过**上界时（上游给了越界游标），advance 要把
    // 它拉回 input_len 而不是任其留在界外。此项与上一项分工：上一项隔离「加法饱和」，
    // 本项隔离「钳制」——两者任一被去掉，只有对应这项会红。
    let pulled = advance(Cursor::new(50, 50), 1, 10);
    set.add(
        "C17-兜底-越界游标被钳回上界",
        pulled.token_index == 10 && pulled.byte_offset == 10,
        "",
    );

    // advance 对任意 by（含 usize::MAX）都不得使游标后退或越界
    let big_by = advance(Cursor::new(3, 3), usize::MAX, usize::MAX);
    set.add(
        "C17-兜底-超大步长不后退不越界",
        big_by.token_index >= 3 && big_by.token_index == usize::MAX,
        "",
    );

    // 游标已在上界 → 如实停滞，不自增
    let mut sess8 = RecoverySession::new(4);
    let st = sess8.recover("VE-F0409-MISSING-SEMICOLON", Cursor::new(4, 40), 1, &syncs());
    set.add(
        "C17-兜底-游标到上界如实停滞",
        st.stalled && st.after.token_index == 4 && sess8.counters().stalled == 1,
        "",
    );

    // 穷尽推进必在有限步内停滞（终止性的实测）
    let mut sess9 = RecoverySession::with_budget(8, 60000);
    let mut cur = Cursor::new(0, 0);
    let mut steps = 0usize;
    let mut term = false;
    while steps < 200 {
        let a = sess9.recover("VE-F0407-UNTERMINATED", cur, 1, &syncs());
        steps += 1;
        if a.stalled {
            term = true;
            break;
        }
        cur = a.after;
    }
    set.add("C17-兜底-反复恢复必在有限步内停滞", term, "");

    // ---- 判据：级联反馈端到端 ----

    // 全程无级联的恢复流
    let ins_ok = [
        RecoveryInput { code: "VE-F0409-MISSING-SEMICOLON", at: Cursor::new(0, 0), line: 1 },
        RecoveryInput { code: "VE-F0403-UNKNOWN-TOKEN", at: Cursor::new(1, 10), line: 1 },
    ];
    let obs_ok: [&[Severity]; 2] = [&all_warn, &all_warn];
    let st_ok = recover_stream(&ins_ok, &syncs(), &obs_ok, 100);
    set.add(
        "C17-级联-无级联恢复流判定正确",
        st_ok.cascade_free() && st_ok.actions.len() == 2 && st_ok.fallback_free(),
        "",
    );

    // 有级联时 burst 被计数（端到端）
    let obs_bad: [&[Severity]; 2] = [&three, &all_warn];
    let st_bad = recover_stream(&ins_ok, &syncs(), &obs_bad, 100);
    set.add(
        "C17-级联-级联爆发被计数",
        !st_bad.cascade_free() && st_bad.bursts >= 1,
        "",
    );

    // 恢复流渲染非空（恢复不静默）
    set.add("C17-显性-恢复流渲染非空", !st_ok.render_all().is_empty(), "");

    // 空输入 → 空流、不 panic
    let st_empty = recover_stream(&[], &syncs(), &[], 100);
    set.add(
        "C17-显性-空输入恢复流为空",
        st_empty.actions.is_empty() && st_empty.cascade_free(),
        "",
    );

    // 会话计数与流计数一致（两处记账不许打架）
    set.add(
        "C17-计数-流计数与动作数一致",
        st_ok.counters.total == st_ok.actions.len() as u32 && st_ok.counters.notice_covers_all(),
        "",
    );

    // peek 与实际恢复策略一致（预查不许与主路径分叉）
    let pk = RecoverySession::peek_strategy("VE-F0410-MISMATCH");
    set.add(
        "C17-策略-预查与主路径一致",
        pk.0 == RecoveryClass::BracketMismatch && pk.1 == RecoveryStrategy::SyncResync,
        "",
    );

    // 每条动作都带通知（流级覆盖）
    let mut no_notice = 0usize;
    let mut ai = 0usize;
    while ai < st_ok.actions.len() {
        if st_ok.actions[ai].notice.cost.is_empty() {
            no_notice += 1;
        }
        ai += 1;
    }
    set.add("C17-显性-流内每条动作均带通知", no_notice == 0, "");

    set
}

#[cfg(test)]
mod red_recover {
    use super::*;
    #[test]
    fn recover_red_items() {
        let set = run_vec17_checks();
        for i in 0..set.len() {
            if let Some(c) = set.get(i) {
                if !c.passed {
                    println!("RED: {} | {}", c.name, c.detail);
                }
            }
        }
        println!("total={} dropped={}", set.len(), set.dropped());
    }
}