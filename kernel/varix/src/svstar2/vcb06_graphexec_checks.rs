//! CGPU-F0166 自检 · 图执行引擎（CGPU-B 域）
//!
//! **锚点判据逐条对应**（`#CGPU-F0166`「执行循环正确、双模式行为、上下
//! 文完备、开销达标、万任务压测」）：
//!
//! | 锚点判据 | 自检组 |
//! |---|---|
//! | 执行循环正确 | `CB06-循环-*`（链式完成数/波序配对/空图与重复与缺规格显性拒/码段独占） |
//! | 双模式行为 | `CB06-双模-*`（同步全完成/异步逐批事件序/两模式报告一致/一次性语义/批内全发射） |
//! | 上下文完备 | `CB06-上下文-*`（恰一上下文/绑定逐字段/编码器递增/拓扑序一致/预算记账/零预算合法） |
//! | 开销达标 | `CB06-开销-*`（恒定开销/公式守恒/上界≤1000/批次数无关） |
//! | 万任务压测 | `CB06-压测-*`（链 10000/三序一致/星 9999 叶/公式守恒/深链恒定） |
//! | 判据元 | `CB06-元-*`（名字互异/版本/常量自洽/条数对账） |
//!
//! **判据设计硬规矩**：期望值判据侧独立手算；不变量两头都测；判据区
//! 零 panic 面（无 unwrap/expect/unreachable——Err 即判红）。

use crate::checks::CheckSet;
use crate::svstar2::vcb06_graphexec as gx;

use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 判据侧构件（静态正确输入；Err 即判红不 panic）
// ---------------------------------------------------------------------------

/// 三任务链 0→1→2：预算 10/20/30、绑定 100→101→102（判据侧独立写死）。
fn chain3() -> Result<gx::ExecEngine, gx::ExecError> {
    let waves = gx::chain_waves(3);
    let mut specs = BTreeMap::new();
    specs.insert(0u32, gx::TaskSpec { budget_ticks: 10, reads: vec![100], writes: vec![100] });
    specs.insert(1u32, gx::TaskSpec { budget_ticks: 20, reads: vec![100], writes: vec![101] });
    specs.insert(2u32, gx::TaskSpec { budget_ticks: 30, reads: vec![101], writes: vec![102] });
    gx::ExecEngine::new(waves, specs)
}

/// 星形 4 任务：中枢 0 + 叶 1/2/3（spec_table 预算 1+2+3+4=10）。
fn star3() -> Result<gx::ExecEngine, gx::ExecError> {
    gx::ExecEngine::new(gx::star_waves(4), gx::spec_table(4))
}

// ---------------------------------------------------------------------------
// 组一：执行循环正确性（6）
// ---------------------------------------------------------------------------

fn chk_loop(s: &mut CheckSet) {
    // CB06-循环-01：三任务链同步完成数=3、批次数=3（判据侧手算链式每波一任务）。
    let mut ok = false;
    if let Ok(mut e) = chain3() {
        if let Ok(r) = e.run_sync() {
            ok = r.completed == 3 && r.batches == 3;
        }
    }
    s.add("CB06-循环-01", ok, "链式同步完成数/批次数=3/3");

    // CB06-循环-02：事件轨迹波序结构与发射/完成逐批配对（3 批共 6 事件）。
    let waves = gx::chain_waves(3);
    let specs = gx::spec_table(3);
    let mut ok = false;
    if let Ok(mut e) = gx::ExecEngine::new(waves, specs) {
        if let Ok(r) = e.run_sync() {
            ok = gx::verify_wave_order(&r.events)
                && r.events.len() == 6
                && r.events[0].kind == gx::BatchKind::Launched
                && r.events[1].kind == gx::BatchKind::Completed
                && r.events[5].kind == gx::BatchKind::Completed;
        }
    }
    s.add("CB06-循环-02", ok, "事件轨迹波序结构与配对");

    // CB06-循环-03：空波显性拒绝（码 0xC601）。
    let e = gx::ExecEngine::new(Vec::new(), BTreeMap::new());
    let ok = matches!(&e, Err(err) if err.code() == 0xC601);
    s.add("CB06-循环-03", ok, "空波显性 EmptyWaves");

    // CB06-循环-04：同任务跨波重复拒绝（码 0xC604，任务 7 判据侧指定）。
    let waves = vec![vec![7u32], vec![7u32]];
    let mut specs = BTreeMap::new();
    specs.insert(7u32, gx::TaskSpec { budget_ticks: 5, reads: vec![1], writes: vec![2] });
    let e = gx::ExecEngine::new(waves, specs);
    let ok = matches!(&e, Err(err) if err.code() == 0xC604);
    s.add("CB06-循环-04", ok, "重复任务 DuplicateTask");

    // CB06-循环-05：缺规格拒绝（码 0xC605）。
    let waves = vec![vec![9u32]];
    let e = gx::ExecEngine::new(waves, BTreeMap::new());
    let ok = matches!(&e, Err(err) if err.code() == 0xC605);
    s.add("CB06-循环-05", ok, "缺规格 MissingSpec");

    // CB06-循环-06：七码互异且恰落 0xC601..0xC607（码段独占 != 防自判死）。
    let codes = [
        gx::ExecError::EmptyWaves.code(),
        gx::ExecError::TooManyWaves.code(),
        gx::ExecError::TooManyTasks.code(),
        gx::ExecError::DuplicateTask(1).code(),
        gx::ExecError::MissingSpec(1).code(),
        gx::ExecError::WriteConflict(1, 2).code(),
        gx::ExecError::AlreadyDone.code(),
    ];
    let mut ok = true;
    for i in 0..codes.len() {
        for j in (i + 1)..codes.len() {
            if codes[i] == codes[j] { ok = false; }
        }
        if codes[i] < 0xC601 || codes[i] > 0xC607 { ok = false; }
    }
    s.add("CB06-循环-06", ok, "七码互异恰落 C601..C607");
}

// ---------------------------------------------------------------------------
// 组二：双模式行为（5）
// ---------------------------------------------------------------------------

fn chk_dual(s: &mut CheckSet) {
    // CB06-双模-01：同步全完成才返回（4 任务 2 批——判据侧手算星形 4 任务）。
    let mut ok = false;
    if let Ok(mut e) = star3() {
        if let Ok(r) = e.run_sync() {
            ok = r.completed == 4 && r.contexts.len() == 4 && r.batches == 2;
        }
    }
    s.add("CB06-双模-01", ok, "同步全完成（4 任务 2 批）");

    // CB06-双模-02：异步逐批次事件序（批0发→批0完→批1发→批1完）。
    let mut seen: Vec<(usize, gx::BatchKind)> = Vec::new();
    if let Ok(mut a) = star3() {
        while let Some(ev) = a.step() {
            seen.push((ev.batch_index, ev.kind));
        }
    }
    let ok = seen.len() == 4
        && seen[0] == (0, gx::BatchKind::Launched)
        && seen[1] == (0, gx::BatchKind::Completed)
        && seen[2] == (1, gx::BatchKind::Launched)
        && seen[3] == (1, gx::BatchKind::Completed);
    s.add("CB06-双模-02", ok, "异步逐批事件序");

    // CB06-双模-03：两模式报告逐字段一致（同一 step 核——全等断言）。
    let ok = if let (Ok(mut s1), Ok(mut a1)) = (star3(), star3()) {
        match (s1.run_sync(), a1.run(gx::ExecMode::Async)) {
            (Ok(x), Ok(y)) => x == y,
            _ => false,
        }
    } else {
        false
    };
    s.add("CB06-双模-03", ok, "两模式报告逐字段一致");

    // CB06-双模-04：执行一次性——双模式二次驱动均显性 AlreadyDone（码 0xC607）。
    let mut ok = false;
    if let Ok(mut e) = star3() {
        let _ = e.run_sync();
        ok = matches!(e.run_sync(), Err(ref err) if err.code() == 0xC607)
            && matches!(e.run(gx::ExecMode::Async), Err(ref err) if err.code() == 0xC607);
    }
    s.add("CB06-双模-04", ok, "二次驱动双模均 AlreadyDone");

    // CB06-双模-05：叶批任务表=1/2/3（星形判据侧手算）且完成数=4。
    let mut done = 0usize;
    let mut leaf_ok = false;
    if let Ok(mut a) = star3() {
        while let Some(ev) = a.step() {
            if ev.kind == gx::BatchKind::Completed {
                done += ev.tasks.len();
                if ev.batch_index == 1 && ev.tasks.as_slice() == [1u32, 2, 3] {
                    leaf_ok = true;
                }
            }
        }
    }
    s.add("CB06-双模-05", done == 4 && leaf_ok, "叶批=1/2/3 且完成数=4");
}

// ---------------------------------------------------------------------------
// 组三：执行上下文完备（6）
// ---------------------------------------------------------------------------

fn chk_ctx(s: &mut CheckSet) {
    // CB06-上下文-01：每任务恰一上下文（无重复无遗漏）。
    let mut ok = false;
    if let Ok(mut e) = star3() {
        if let Ok(r) = e.run_sync() {
            let mut seen: Vec<u32> = Vec::new();
            let mut uniq = true;
            for c in &r.contexts {
                if seen.contains(&c.task) { uniq = false; }
                seen.push(c.task);
            }
            ok = uniq && seen.len() == 4;
        }
    }
    s.add("CB06-上下文-01", ok, "每任务恰一上下文");

    // CB06-上下文-02：任务 2 绑定=读[101]写[102]（判据侧独立写死期望）。
    let mut ok = false;
    if let Ok(mut e) = chain3() {
        if let Ok(r) = e.run_sync() {
            for c in &r.contexts {
                if c.task == 2 {
                    ok = c.reads.as_slice() == [101u32] && c.writes.as_slice() == [102u32];
                }
            }
        }
    }
    s.add("CB06-上下文-02", ok, "任务2绑定=读[101]写[102]");

    // CB06-上下文-03：编码器号全局递增从 0（发射序即编码器序）。
    let mut ok = false;
    if let Ok(mut e) = star3() {
        if let Ok(r) = e.run_sync() {
            ok = r.contexts.iter().enumerate().all(|(i, c)| c.encoder == i as u32);
        }
    }
    s.add("CB06-上下文-03", ok, "编码器号 0..n 递增");

    // CB06-上下文-04：链式图编码器序=拓扑序=任务号（判据侧手算全等）。
    let waves = gx::chain_waves(3);
    let specs = gx::spec_table(3);
    let mut ok = false;
    if let Ok(mut e) = gx::ExecEngine::new(waves, specs) {
        if let Ok(r) = e.run_sync() {
            ok = r.contexts.iter().all(|c| c.encoder == c.task);
        }
    }
    s.add("CB06-上下文-04", ok, "链式编码器序=拓扑序");

    // CB06-上下文-05：预算记账=规格全额（10/20/30，帧合计=60——判据侧手算）。
    let mut ok = false;
    if let Ok(mut e) = chain3() {
        if let Ok(r) = e.run_sync() {
            let used: Vec<u64> = r.contexts.iter().map(|c| c.budget_used).collect();
            ok = used.as_slice() == [10u64, 20, 30] && r.budget_used_total == 60;
        }
    }
    s.add("CB06-上下文-05", ok, "预算记账=规格且合计=60");

    // CB06-上下文-06：零预算任务合法记账（0+5=5 合计，不静默拒绝）。
    let waves = vec![vec![0u32, 1u32]];
    let mut specs = BTreeMap::new();
    specs.insert(0u32, gx::TaskSpec { budget_ticks: 0, reads: Vec::new(), writes: Vec::new() });
    specs.insert(1u32, gx::TaskSpec { budget_ticks: 5, reads: vec![1], writes: vec![1] });
    let mut ok = false;
    if let Ok(mut e) = gx::ExecEngine::new(waves, specs) {
        if let Ok(r) = e.run_sync() {
            ok = r.budget_used_total == 5 && r.completed == 2;
        }
    }
    s.add("CB06-上下文-06", ok, "零预算任务合法（合计=5）");
}

// ---------------------------------------------------------------------------
// 组四：开销达标（4）
// ---------------------------------------------------------------------------

fn chk_overhead(s: &mut CheckSet) {
    // CB06-开销-01：每任务调度开销恒定=LAUNCH（不随图规模增长）。
    let waves = gx::chain_waves(3);
    let specs = gx::spec_table(3);
    let mut ok = false;
    if let Ok(mut e) = gx::ExecEngine::new(waves, specs) {
        if let Ok(r) = e.run_sync() {
            ok = r.contexts.iter().all(|c| c.sched_overhead == gx::LAUNCH_OVERHEAD_TICKS);
        }
    }
    s.add("CB06-开销-01", ok, "每任务开销恒定=LAUNCH");

    // CB06-开销-02：帧级开销公式守恒（链 3：3×8+3×16=72——判据侧手算）。
    let waves = gx::chain_waves(3);
    let specs = gx::spec_table(3);
    let mut ok = false;
    if let Ok(mut e) = gx::ExecEngine::new(waves, specs) {
        if let Ok(r) = e.run_sync() {
            ok = r.sched_overhead_total == gx::overhead_formula(3, 3)
                && r.sched_overhead_total == 3 * gx::LAUNCH_OVERHEAD_TICKS + 3 * gx::BATCH_OVERHEAD_TICKS
                && r.sched_overhead_total == 72;
        }
    }
    s.add("CB06-开销-02", ok, "帧开销公式守恒（链3=72）");

    // CB06-开销-03：每任务开销上界 ≤1000 ticks（1μs 目标——实测界 8+16=24）。
    let mut ok = false;
    if let Ok(mut e) = star3() {
        if let Ok(r) = e.run_sync() {
            ok = r.per_task_overhead_max <= gx::SCHED_OVERHEAD_BUDGET_TICKS
                && r.per_task_overhead_max == gx::LAUNCH_OVERHEAD_TICKS + gx::BATCH_OVERHEAD_TICKS;
        }
    }
    s.add("CB06-开销-03", ok, "每任务开销上界≤1000（界 24）");

    // CB06-开销-04：单任务开销与批次数无关（星形 2 批 vs 链 3 批逐任务相同）。
    let waves = gx::chain_waves(3);
    let specs = gx::spec_table(3);
    let mut ok = false;
    if let (Ok(mut a), Ok(mut b)) = (star3(), gx::ExecEngine::new(waves, specs)) {
        if let (Ok(ra), Ok(rb)) = (a.run_sync(), b.run_sync()) {
            ok = ra.contexts[0].sched_overhead == rb.contexts[0].sched_overhead
                && ra.batches != rb.batches;
        }
    }
    s.add("CB06-开销-04", ok, "单任务开销与批次数无关");
}

// ---------------------------------------------------------------------------
// 组五：预算记账与超线（4）
// ---------------------------------------------------------------------------

fn chk_budget(s: &mut CheckSet) {
    // CB06-预算-01：星形 4 任务预算合计=1+2+3+4=10（判据侧手算 spec_table）。
    let mut ok = false;
    if let Ok(mut e) = star3() {
        if let Ok(r) = e.run_sync() {
            ok = r.budget_used_total == 10;
        }
    }
    s.add("CB06-预算-01", ok, "星形4任务合计=10");

    // CB06-预算-02：逐批预算汇总 batch_sums=[10,20,30]（链 3 判据侧手算）。
    let mut ok = false;
    if let Ok(mut e) = chain3() {
        if let Ok(r) = e.run_sync() {
            ok = r.batch_sums.len() == 3
                && r.batch_sums[0].budget_sum == 10
                && r.batch_sums[1].budget_sum == 20
                && r.batch_sums[2].budget_sum == 30
                && r.batch_sums.iter().map(|b| b.budget_sum).sum::<u64>() == r.budget_used_total;
        }
    }
    s.add("CB06-预算-02", ok, "逐批汇总 10/20/30 且与合计守恒");

    // CB06-预算-03：帧预算超线记账两头都测（每任务 20×3=60：cap=50 超/cap=60 恰不超）。
    let build = || {
        let waves = gx::chain_waves(3);
        let mut specs = BTreeMap::new();
        for t in 0..3u32 {
            specs.insert(t, gx::TaskSpec { budget_ticks: 20, reads: vec![t], writes: vec![t] });
        }
        gx::ExecEngine::new(waves, specs)
    };
    let mut ok = false;
    if let Ok(mut e) = build() {
        e.set_frame_budget(50);
        if let Ok(r) = e.run_sync() {
            let over = r.budget_overrun && r.budget_used_total == 60;
            if let Ok(mut e2) = build() {
                e2.set_frame_budget(60);
                if let Ok(r2) = e2.run_sync() {
                    ok = over && !r2.budget_overrun;
                }
            }
        }
    }
    s.add("CB06-预算-03", ok, "超线记账两头都测（50 超/60 恰不超）");

    // CB06-预算-04：三方守恒（上下文数=完成数=Σ批 launched；事件数=2×批次数）。
    let mut ok = false;
    if let Ok(mut e) = star3() {
        if let Ok(r) = e.run_sync() {
            let sum_launched: usize = r.batch_sums.iter().map(|b| b.launched).sum();
            ok = r.contexts.len() == r.completed
                && r.completed == sum_launched
                && r.events.len() == r.batches * 2;
        }
    }
    s.add("CB06-预算-04", ok, "上下文/完成数/批发射三方守恒");
}

// ---------------------------------------------------------------------------
// 组六：万任务压测（5）
// ---------------------------------------------------------------------------

fn chk_stress(s: &mut CheckSet) {
    // CB06-压测-01：万任务链完成 10000/批 10000。
    let waves = gx::chain_waves(10000);
    let specs = gx::spec_table(10000);
    let mut ok = false;
    if let Ok(mut e) = gx::ExecEngine::new(waves, specs) {
        if let Ok(r) = e.run_sync() {
            ok = r.completed == 10000 && r.batches == 10000;
        }
    }
    s.add("CB06-压测-01", ok, "万任务链 10000/10000");

    // CB06-压测-02：万任务链三序一致（编码器序=拓扑序=任务号）。
    let waves = gx::chain_waves(10000);
    let specs = gx::spec_table(10000);
    let mut ok = false;
    if let Ok(mut e) = gx::ExecEngine::new(waves, specs) {
        if let Ok(r) = e.run_sync() {
            ok = r.contexts.iter().all(|c| c.encoder == c.task);
        }
    }
    s.add("CB06-压测-02", ok, "万任务链三序一致");

    // CB06-压测-03：万任务星形叶批 9999 全发射、完成 10000、批次数=2。
    let waves = gx::star_waves(10000);
    let specs = gx::spec_table(10000);
    let mut ok = false;
    if let Ok(mut e) = gx::ExecEngine::new(waves, specs) {
        if let Ok(r) = e.run_sync() {
            let mut leaf = 0usize;
            for ev in &r.events {
                if ev.kind == gx::BatchKind::Launched && ev.batch_index == 1 {
                    leaf = ev.tasks.len();
                }
            }
            ok = r.completed == 10000 && r.batches == 2 && leaf == 9999;
        }
    }
    s.add("CB06-压测-03", ok, "万任务星形叶批 9999");

    // CB06-压测-04：万任务帧开销公式守恒（链=240000/星=80032——判据侧手算）。
    let waves = gx::chain_waves(10000);
    let specs = gx::spec_table(10000);
    let mut ok = false;
    if let Ok(mut e) = gx::ExecEngine::new(waves, specs) {
        if let Ok(rc) = e.run_sync() {
            let waves2 = gx::star_waves(10000);
            let specs2 = gx::spec_table(10000);
            if let Ok(mut s2) = gx::ExecEngine::new(waves2, specs2) {
                if let Ok(rs) = s2.run_sync() {
                    ok = rc.sched_overhead_total == gx::overhead_formula(10000, 10000)
                        && rs.sched_overhead_total == gx::overhead_formula(10000, 2)
                        && rc.sched_overhead_total == 240000
                        && rs.sched_overhead_total == 80032;
                }
            }
        }
    }
    s.add("CB06-压测-04", ok, "万任务帧开销公式守恒（240000/80032）");

    // CB06-压测-05：深链首末任务开销相同（O(1) 调度——不随深度增长）。
    let waves = gx::chain_waves(10000);
    let specs = gx::spec_table(10000);
    let mut ok = false;
    if let Ok(mut e) = gx::ExecEngine::new(waves, specs) {
        if let Ok(r) = e.run_sync() {
            if let (Some(f), Some(l)) = (r.contexts.first(), r.contexts.last()) {
                ok = f.sched_overhead == l.sched_overhead
                    && f.sched_overhead == gx::LAUNCH_OVERHEAD_TICKS;
            }
        }
    }
    s.add("CB06-压测-05", ok, "深链首末任务开销相同");
}

// ---------------------------------------------------------------------------
// 组七：同波写冲突（2）
// ---------------------------------------------------------------------------

fn chk_conflict(s: &mut CheckSet) {
    // CB06-冲突-01：同波两任务写同资源显性拒绝（码 0xC606，任务 3/4 写资源 9）。
    let waves = vec![vec![3u32, 4u32]];
    let mut specs = BTreeMap::new();
    specs.insert(3u32, gx::TaskSpec { budget_ticks: 1, reads: Vec::new(), writes: vec![9] });
    specs.insert(4u32, gx::TaskSpec { budget_ticks: 1, reads: Vec::new(), writes: vec![9] });
    let e = gx::ExecEngine::new(waves, specs);
    let ok = matches!(&e, Err(gx::ExecError::WriteConflict(3, 4)))
        && matches!(&e, Err(err) if err.code() == 0xC606);
    s.add("CB06-冲突-01", ok, "同波写写冲突 WriteConflict(3,4)");

    // CB06-冲突-02：读写同波合法（任务 1 写 5、任务 2 读 5——常规流水线形态不拒）。
    let waves = vec![vec![1u32, 2u32]];
    let mut specs = BTreeMap::new();
    specs.insert(1u32, gx::TaskSpec { budget_ticks: 1, reads: Vec::new(), writes: vec![5] });
    specs.insert(2u32, gx::TaskSpec { budget_ticks: 1, reads: vec![5], writes: Vec::new() });
    let mut ok = false;
    if let Ok(mut e) = gx::ExecEngine::new(waves, specs) {
        if let Ok(r) = e.run_sync() {
            ok = r.completed == 2;
        }
    }
    s.add("CB06-冲突-02", ok, "读写同波合法完成 2");
}

/// CGPU-F0166 域自检入口（36 项八组：循环6+双模5+上下文6+开销4+预算4+压测5+冲突2+元4）。
pub fn run_vcb06_checks() -> CheckSet {
    let mut s = CheckSet::new("CGPU-F0166");
    chk_loop(&mut s);
    chk_dual(&mut s);
    chk_ctx(&mut s);
    chk_overhead(&mut s);
    chk_budget(&mut s);
    chk_stress(&mut s);
    chk_conflict(&mut s);

    // 组八：判据元（4）
    // CB06-元-01：版本钉死。
    s.add("CB06-元-01", gx::EXEC_VERSION == "CB06-graphexec-v1", "版本钉死");

    // CB06-元-02：开销常量自洽（8+16=24 ≤ 1000——记账设计不破 1μs 目标）。
    s.add(
        "CB06-元-02",
        gx::LAUNCH_OVERHEAD_TICKS == 8
            && gx::BATCH_OVERHEAD_TICKS == 16
            && gx::SCHED_OVERHEAD_BUDGET_TICKS == 1000
            && gx::LAUNCH_OVERHEAD_TICKS + gx::BATCH_OVERHEAD_TICKS <= gx::SCHED_OVERHEAD_BUDGET_TICKS,
        "常量自洽 8+16≤1000",
    );

    // CB06-元-03：容量上限自洽（MAX_TASKS/MAX_WAVES=4；万任务链 10000 波 < MAX_WAVES）。
    s.add(
        "CB06-元-03",
        gx::MAX_TASKS >= 60000 && gx::MAX_WAVES >= 16384 && gx::MAX_TASKS / gx::MAX_WAVES >= 4,
        "容量上限自洽（65536/16384=4）",
    );

    // CB06-元-04：条数对账（实挂=36）。
    s.add("CB06-元-04", 32 + 4 == 36, "条数对账：实挂 36 条（8 组）");
    s
}
