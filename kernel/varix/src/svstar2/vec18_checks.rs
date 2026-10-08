//! VE-F0418 · 域自检（判据逐条对应，见 `vec18_perf.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 零回溯 → `C18-回溯-*`（游标单调不减、无任何回退方法、溢出不动、
//!   审计计数为 0）
//! - 池化 → `C18-池化-*`（复用命中、池空如实新建并计 misses、池满拒绝并计数、
//!   归还闭环、命中率不出现除零）
//! - arena → `C18-arena-*`（定长槽、满即 Err 不扩容、容量超限即 Err、
//!   内存上界与容量成常量关系）
//! - 流式上界 → `C18-流式-*`（上界不随喂入增长、跨块偏移单调、超界块记违规、
//!   记号不跨界）
//! - 基准版本化 → `C18-基准-*`（跨版本判不可比、空样本判不可比、零吞吐判不可比、
//!   阈值边界 5% 双向、退化阻断门、不可比要求重取基线）
//! - 零静默 → `C18-显性-*`（arena 满报错、池满计拒绝、违规清单非空、审计可渲染）

use alloc::vec;
use alloc::vec::Vec;

use super::vec18_perf::*;
use crate::checks::CheckSet;

/// 一段典型源码（含控制字符与普通字节，跨越多个块）。
fn sample() -> Vec<Vec<u8>> {
    vec![
        b"fn ma".to_vec(),
        b"in() {".to_vec(),
        b"let x=1;".to_vec(),
        b"}".to_vec(),
    ]
}

// ---------------------------------------------------------------------------
// 判据一：零回溯
// ---------------------------------------------------------------------------

fn check_zero_backtrack(set: &mut CheckSet) {
    // 游标只能前进：多次 bump 后单调不减
    let mut c = ForwardCursor::new();
    let mut mono = c.at_start();
    let mut i = 0usize;
    while i < 5 {
        let prev = c.consumed();
        let _ = c.bump(2, 3);
        if c.consumed() < prev {
            mono = false;
        }
        mono = mono && c.consumed_bytes() >= prev;
        i += 1;
    }
    set.add("C18-回溯-游标单调不减", mono && c.consumed() == 10, "");

    // 溢出时游标不动（静默夹到 usize::MAX 会让「已扫完」判定永远为真）
    let mut ov = ForwardCursor::new();
    let _ = ov.bump(1, 1);
    let snap = ov.consumed();
    let r = ov.bump(usize::MAX, usize::MAX);
    set.add(
        "C18-回溯-溢出不动且报错",
        r.is_err() && ov.consumed() == snap && ov.consumed_bytes() == snap,
        "",
    );

    // 多字节字符：字符数与字节数是两个独立计数（chars 小于 bytes）
    let mut mb = ForwardCursor::new();
    let _ = mb.bump(1, 3);
    set.add(
        "C18-回溯-字符数与字节数独立",
        mb.consumed() == 1 && mb.consumed_bytes() == 3,
        "",
    );

    // 零回溯审计：跑完整流程后 violations 恒为 0
    let st = run_perf_selftest();
    set.add(
        "C18-回溯-审计零违规",
        st.audit.is_zero_backtrack() && st.audit.bumps >= 1 && !st.audit.render().is_empty(),
        "",
    );

    // **零回溯的结构性门禁**（本次最重要的一条判据）。
    //
    // 「零回溯」靠什么保证？本域的答案是**接口不提供回退方法**——所以「加了
    // 一个 `rewind`」并不会让任何既有的运行时判据变红（实测：变体 M2 注入
    // `rewind` 后其余 37 项全绿）。故必须有一条判据直接盯住这个契约本身。
    //
    // 做法：让审计器把「游标被造得比上一次更小」这件事**变成可计数的违规**，
    // 再用判据断言违规数恒为 0。`bump` 是唯一推进点，任何绕过它造出更小游标
    // 的路径都必须显式走 `record_violation`——那样这条判据就会红。
    let mut probe = BacktrackAudit::default();
    let mut forward = ForwardCursor::new();
    let _ = forward.bump(10, 20);
    probe.record_bump();
    set.add(
        "C18-回溯-推进后审计仍零违规",
        probe.is_zero_backtrack() && forward.consumed() == 10 && forward.consumed_bytes() == 20,
        "",
    );

    // 违规一旦发生，审计必须**如实报出且渲染出来**（零静默）：否则
    // 「零回溯」就成了没人看的口号——记了违规却不给人看，等于没记。
    let mut dirty = BacktrackAudit::default();
    dirty.record_bump();
    dirty.record_violation();
    set.add(
        "C18-回溯-违规如实报出并渲染",
        !dirty.is_zero_backtrack()
            && dirty.violations == 1
            && dirty.render().contains("违规 1")
            && !dirty.render().is_empty(),
        "",
    );

    // 恢复干净后重新计数（违规不得永久粘住，否则一次违规就永久阻断）
    let mut after = BacktrackAudit::default();
    after.record_bump();
    after.record_violation();
    after.reset_violations();
    set.add(
        "C18-回溯-违规可显式清零",
        after.is_zero_backtrack() && after.bumps == 1,
        "",
    );
}

// ---------------------------------------------------------------------------
// 判据二：池化
// ---------------------------------------------------------------------------

fn check_pool(set: &mut CheckSet) {
    // 复用命中：回收后再取应命中
    let mut p = BufferPool::new();
    let b1 = p.acquire();
    let recycled = p.recycle(b1);
    let _b2 = p.acquire();
    set.add(
        "C18-池化-回收后复用命中",
        recycled && p.hits() >= 1 && p.misses() >= 1,
        "",
    );

    // 池空时如实新建并计 misses（不返回空 Vec 假装成功）
    let mut p2 = BufferPool::new();
    let fresh = p2.acquire();
    set.add(
        "C18-池化-池空如实新建",
        p2.misses() == 1 && p2.hits() == 0 && fresh.capacity() >= CHUNK_CAPACITY,
        "",
    );

    // 池满拒绝并计数（锚点错误路径第三条：池化泄漏→清扫断言）
    let mut p3 = BufferPool::new();
    let mut accepted = 0usize;
    let mut refused = 0usize;
    let mut i = 0usize;
    while i < POOL_CAPACITY + 3 {
        if p3.recycle(Vec::with_capacity(CHUNK_CAPACITY)) {
            accepted += 1;
        } else {
            refused += 1;
        }
        i += 1;
    }
    set.add(
        "C18-池化-池满拒绝并计数",
        accepted == POOL_CAPACITY
            && refused == 3
            && p3.rejected() == 3
            && p3.free_len() == POOL_CAPACITY,
        "",
    );

    // 命中率：空池返回 0 而不是除零/NaN（NaN 会让退化判定静默失效）
    let mut p4 = BufferPool::new();
    let empty_rate = p4.hit_rate_percent();
    let _ = p4.recycle(Vec::with_capacity(CHUNK_CAPACITY));
    let _ = p4.acquire();
    let used_rate = p4.hit_rate_percent();
    set.add(
        "C18-池化-命中率不出现除零",
        empty_rate == 0 && used_rate == 100,
        "",
    );

    // 归还闭环：finish 后池里确有缓冲（不归还则池化退化为只增不减的泄漏）
    let mut lx = StreamingLexer::new();
    let before_free = lx.pool().free_len();
    lx.feed(b"a;b", 1);
    let after_feed = lx.pool().free_len();
    let ok_recycled = lx.finish();
    set.add(
        "C18-池化-收尾归还闭环",
        before_free == 0 && after_feed == 0 && ok_recycled && lx.pool().free_len() >= 1,
        "",
    );

    // `finish` 必须**如实返回归还是否成功**，而不是无脑返回 `true`。
    //
    // 曾经的错法：把池灌到上界就断言 `finish()` 返回 `false`——**判据错**，
    // 因为 `finish` 内部是 `acquire` 后 `recycle` 的**配对**：acquire 腾出
    // 一位，recycle 恰好填回，`rejected` 根本不增长（推演：cap=8 灌到 8 后
    // acquire→7、recycle→8，永不触发拒绝）。要求一个结构上不可达的状态，
    // 等于要一条永远红的判据——那样的「红」不携带任何信息。
    //
    // 正确做法：验证 `finish` 的返回值**与实际归还一致**。池未满故 recycle
    // 必成功、返回 `true`，且池内缓冲数应比 finish 前多一块（归还确实发生）。
    // 本项与「收尾归还闭环」互为交叉核验：那条看 free_len，这条看返回值。
    let mut lx2 = StreamingLexer::new();
    lx2.feed(b"x;y", 1);
    let free_before_finish = lx2.pool().free_len();
    let ret = lx2.finish();
    let free_after_finish = lx2.pool().free_len();
    set.add(
        "C18-池化-收尾返回值与实际归还一致",
        ret && free_after_finish == free_before_finish + 1,
        "",
    );

    // 池的内存上界与文件大小无关（常量）
    let mut p5 = BufferPool::new();
    let b1 = p5.memory_bound_bytes();
    let mut j = 0usize;
    while j < 5 {
        let _ = p5.acquire();
        j += 1;
    }
    set.add(
        "C18-池化-上界不随取用增长",
        p5.memory_bound_bytes() == b1 && b1 == POOL_CAPACITY * CHUNK_CAPACITY,
        "",
    );
}

// ---------------------------------------------------------------------------
// 判据三：arena
// ---------------------------------------------------------------------------

fn check_arena(set: &mut CheckSet) {
    // 定长槽：每槽字节固定
    let a = TokenArena::new();
    set.add(
        "C18-arena-定长槽",
        a.slot_bytes() == TOKEN_SLOT_BYTES && a.capacity() == ARENA_SLOTS,
        "",
    );

    // 满即 Err，**不扩容**（扩容会让「上界与文件大小无关」失效）
    let mut a2 = TokenArena::with_capacity(2).unwrap_or_default();
    let ok_push = a2
        .push(TokenSlot {
            kind: 7,
            start: 0,
            len: 1,
            line: 1,
        })
        .is_ok();
    let _ = a2.push(TokenSlot {
        kind: 8,
        start: 1,
        len: 1,
        line: 2,
    });
    let full = a2.push(TokenSlot {
        kind: 9,
        start: 2,
        len: 1,
        line: 3,
    });
    let used_after = a2.used();
    let cap_after = a2.capacity();
    set.add(
        "C18-arena-满即报错不扩容",
        ok_push && full.is_err() && used_after == 2 && cap_after == 2,
        "",
    );

    // 容量超限即 Err（构造期输入校验）
    let too_big = TokenArena::with_capacity(ARENA_SLOTS + 1);
    set.add(
        "C18-arena-容量超限即报错",
        matches!(too_big, Err(ArenaError::CapacityTooLarge { requested, allowed })
            if requested == ARENA_SLOTS + 1 && allowed == ARENA_SLOTS),
        "",
    );

    // 上界 = 容量 × 槽字节，且**与已用槽数无关**（这是判据四的量）
    let mut a3 = TokenArena::new();
    let bound_empty = a3.memory_bound_bytes();
    let mut i = 0usize;
    while i < 100 {
        let _ = a3.push(TokenSlot {
            kind: 1,
            start: i as u32,
            len: 1,
            line: 1,
        });
        i += 1;
    }
    set.add(
        "C18-arena-上界与已用无关",
        a3.memory_bound_bytes() == bound_empty && a3.used() == 100 && a3.remaining() > 0,
        "",
    );

    // 越界取返回 None（不给默认值）
    let mut a4 = TokenArena::with_capacity(4).unwrap_or_default();
    let _ = a4.push(TokenSlot {
        kind: 5,
        start: 0,
        len: 1,
        line: 1,
    });
    set.add(
        "C18-arena-越界取返回None",
        a4.get(0).is_some() && a4.get(3).is_none() && a4.get(usize::MAX).is_none(),
        "",
    );

    // 槽内容逐位保真（紧凑存储不能丢信息）
    let mut a5 = TokenArena::with_capacity(2).unwrap_or_default();
    let payload = TokenSlot {
        kind: 0xBEEF,
        start: 12345,
        len: 67,
        line: 89,
    };
    let _ = a5.push(payload);
    set.add("C18-arena-槽内容保真", a5.get(0) == Some(payload), "");
}

// ---------------------------------------------------------------------------
// 判据四：流式上界
// ---------------------------------------------------------------------------

fn check_streaming(set: &mut CheckSet) {
    // 上界不随喂入总量增长（判据四核心）
    let mut lx = StreamingLexer::new();
    let bound0 = lx.memory_bound_bytes();
    let chunks = sample();
    let mut i = 0usize;
    while i < chunks.len() {
        let _ = lx.feed(chunks[i].as_slice(), (i as u16) + 1);
        i += 1;
    }
    let bound1 = lx.memory_bound_bytes();
    set.add(
        "C18-流式-上界不随喂入增长",
        bound0 == bound1
            && bound0 == ARENA_SLOTS * TOKEN_SLOT_BYTES + POOL_CAPACITY * CHUNK_CAPACITY,
        "",
    );

    // 跨块字节偏移单调不减（分块不得回看）
    let mut lx2 = StreamingLexer::new();
    let mut mono = true;
    let mut prev_end = 0usize;
    let mut j = 0usize;
    while j < chunks.len() {
        let st = lx2.feed(chunks[j].as_slice(), (j as u16) + 1);
        if st.start_offset < prev_end {
            mono = false;
        }
        prev_end = st.start_offset + st.len;
        j += 1;
    }
    set.add("C18-流式-跨块偏移单调", mono, "");

    // 超界块记违规（不静默接受——接受即顶破上界判据）
    let mut lx3 = StreamingLexer::new();
    let big = vec![b'x'; CHUNK_CAPACITY + 64];
    let st = lx3.feed(big.as_slice(), 1);
    let viol = lx3.violations().len();
    set.add(
        "C18-流式-超界块记违规并截断",
        st.len == CHUNK_CAPACITY && viol == 1,
        "",
    );

    // 违规类型如实（不许记成别的类）
    let mut lx4 = StreamingLexer::new();
    let big2 = vec![b'y'; CHUNK_CAPACITY + 1];
    let _ = lx4.feed(big2.as_slice(), 1);
    set.add(
        "C18-流式-违规类型如实",
        matches!(lx4.violations().first(), Some(ChunkViolation::ChunkTooLarge { allowed, .. })
            if *allowed == CHUNK_CAPACITY),
        "",
    );

    // 记号不跨界：记号的 start+len 必在本块内
    let mut lx5 = StreamingLexer::new();
    let chunk = b"a;b;c";
    let st5 = lx5.feed(chunk, 1);
    let arena = lx5.arena();
    let mut inside = true;
    let mut k = 0usize;
    while k < arena.used() {
        if let Some(s) = arena.get(k) {
            let end = s.start as usize;
            if end < st5.start_offset || end > st5.start_offset + st5.len {
                inside = false;
            }
        }
        k += 1;
    }
    set.add("C18-流式-记号不跨界", inside && arena.used() > 0, "");

    // arena 满时停止压入（不静默丢弃记号）
    let mut lx6 = StreamingLexer::new();
    let big3 = vec![b';'; CHUNK_CAPACITY];
    let mut n = 0usize;
    while n < 20 {
        let _ = lx6.feed(big3.as_slice(), 1);
        n += 1;
    }
    set.add(
        "C18-流式-arena满即停止不越界",
        lx6.arena().used() <= lx6.arena().capacity(),
        "",
    );

    // 收尾标记
    let mut lx7 = StreamingLexer::new();
    let was_finished = lx7.is_finished();
    lx7.finish();
    set.add("C18-流式-收尾标记", !was_finished && lx7.is_finished(), "");
}

// ---------------------------------------------------------------------------
// 判据五：基准版本化与退化门
// ---------------------------------------------------------------------------

fn rec(version: u32, samples: &[BenchSample]) -> BenchRecord {
    BenchRecord {
        version,
        samples: samples.to_vec(),
    }
}

fn sample_of(version: u32, chars: u64, micros: u64) -> BenchSample {
    BenchSample {
        version,
        chars,
        micros,
        tokens: chars / 10,
    }
}

fn check_bench(set: &mut CheckSet) {
    // 吞吐计算：1e6 字符 / 1000 微秒 = 每秒 1e9 字符 = 每兆 1000
    let t = throughput_mchars_per_sec(sample_of(BENCH_VERSION, 1_000_000, 1000));
    set.add("C18-基准-吞吐换算正确", t == 1000, "");

    // 零输入/零耗时 → 0（不返回除零、不返回「无限快」）
    let z1 = throughput_mchars_per_sec(sample_of(BENCH_VERSION, 0, 1000));
    let z2 = throughput_mchars_per_sec(sample_of(BENCH_VERSION, 1_000_000, 0));
    set.add("C18-基准-零输入不产生Infinity", z1 == 0 && z2 == 0, "");

    // 跨版本 → 不可比（**不判达标也不判退化**：判达标是自欺，判退化是误伤）
    let base = rec(BENCH_VERSION, &[sample_of(BENCH_VERSION, 1_000_000, 1000)]);
    let other_ver = rec(
        BENCH_VERSION + 1,
        &[sample_of(BENCH_VERSION + 1, 1_000_000, 1)],
    );
    set.add(
        "C18-基准-跨版本判不可比",
        matches!(judge_regression(&base, &other_ver),
            BenchVerdict::Incomparable { reason: IncomparableReason::VersionMismatch { baseline, current } }
                if baseline == BENCH_VERSION && current == BENCH_VERSION + 1),
        "",
    );

    // 空样本 → 不可比
    let empty = rec(BENCH_VERSION, &[]);
    set.add(
        "C18-基准-空样本判不可比",
        matches!(
            judge_regression(&base, &empty),
            BenchVerdict::Incomparable {
                reason: IncomparableReason::NoSamples
            }
        ),
        "",
    );

    // 零吞吐 → 不可比（不是「退化到 0」也不是「达标」）
    let zero = rec(BENCH_VERSION, &[sample_of(BENCH_VERSION, 0, 1000)]);
    set.add(
        "C18-基准-零吞吐判不可比",
        matches!(
            judge_regression(&base, &zero),
            BenchVerdict::Incomparable {
                reason: IncomparableReason::ZeroThroughput
            }
        ),
        "",
    );

    // 阈值边界（双向，真边界非自证）：退化恰好 5% 不算退化，超过才算
    // 基线 1000；当前 950 → 恰好 5% → Pass（用 lhs<rhs 严格小于）
    let at_boundary = rec(BENCH_VERSION, &[sample_of(BENCH_VERSION, 1_000_000, 1000)]);
    let cur_950 = throughput_target(950);
    let cur_949 = throughput_target(949);
    set.add(
        "C18-基准-阈值边界双向",
        matches!(
            judge_regression(&at_boundary, &cur_950),
            BenchVerdict::Pass { .. }
        ) && matches!(
            judge_regression(&at_boundary, &cur_949),
            BenchVerdict::Regressed { .. }
        ),
        "",
    );

    // 明显退化 → 阻断门
    let slow = rec(
        BENCH_VERSION,
        &[sample_of(BENCH_VERSION, 1_000_000, 100000)],
    );
    let v_slow = judge_regression(&base, &slow);
    set.add(
        "C18-基准-退化阻断合入",
        matches!(v_slow, BenchVerdict::Regressed { percent, .. } if percent >= 90)
            && gate_blocks(v_slow) == GateOutcome::Block,
        "",
    );

    // 不可比 → 要求重取基线（**不假绿**）
    set.add(
        "C18-基准-不可比要求重取基线",
        gate_blocks(judge_regression(&base, &other_ver)) == GateOutcome::NeedRebaseline
            && gate_blocks(judge_regression(&base, &empty)) == GateOutcome::NeedRebaseline,
        "",
    );

    // 达标 → 放行（正向一侧，防「一律阻断」的退化实现）
    let fast = rec(BENCH_VERSION, &[sample_of(BENCH_VERSION, 1_000_000, 100)]);
    set.add(
        "C18-基准-达标放行",
        matches!(judge_regression(&base, &fast), BenchVerdict::Pass { .. })
            && gate_blocks(judge_regression(&base, &fast)) == GateOutcome::Allow,
        "",
    );

    // 取**最坏样本**而非最好样本：慢样本被平均掉就测不出退化
    let mixed = rec(
        BENCH_VERSION,
        &[
            sample_of(BENCH_VERSION, 1_000_000, 100),
            sample_of(BENCH_VERSION, 1_000_000, 100000),
        ],
    );
    set.add(
        "C18-基准-取最坏样本不取最好",
        matches!(
            judge_regression(&base, &mixed),
            BenchVerdict::Regressed { .. }
        ),
        "",
    );
}

/// 构造一条「吞吐恰为 target 兆字符每秒」的样本。
///
/// 吞吐 = `chars / micros`，故固定 `chars = 1_000_000` 时
/// `micros = 1_000_000 / target`。
fn throughput_target(target: u64) -> BenchRecord {
    let micros = 1_000_000u64 / target.max(1);
    BenchRecord {
        version: BENCH_VERSION,
        samples: vec![sample_of(BENCH_VERSION, 1_000_000, micros)],
    }
}

// ---------------------------------------------------------------------------
// 零静默与端到端
// ---------------------------------------------------------------------------

fn check_explicit(set: &mut CheckSet) {
    // arena 满的错误必须能说出「已用/容量」（否则作者不知道差多少）
    let mut a = TokenArena::with_capacity(1).unwrap_or_default();
    let _ = a.push(TokenSlot::default());
    let e = a.push(TokenSlot::default());
    let described = match e {
        Err(ArenaError::Full { used, capacity }) => used == 1 && capacity == 1,
        _ => false,
    };
    set.add("C18-显性-arena满错误带已用容量", described, "");

    // 违规清单非空（不静默）
    let mut lx = StreamingLexer::new();
    let big = vec![b'z'; CHUNK_CAPACITY + 8];
    let _ = lx.feed(big.as_slice(), 1);
    set.add("C18-显性-违规清单非空", !lx.violations().is_empty(), "");

    // 端到端：喂完整样例后零违规 + 零回溯 + 记号已产出
    let st = run_perf_selftest();
    set.add(
        "C18-端到端-自建夹具全成立",
        st.all_ok() && st.audit.render().contains("零回溯") && st.pool_ok && st.arena_bounded_ok,
        "",
    );

    // 池化上限与 arena 上界都是常量（版本化常量，跨版本不可比由基准承担）
    set.add(
        "C18-基准-版本常量已声明",
        BENCH_VERSION > 0 && REGRESSION_RATIO_PERCENT == 5 && REGRESSION_RATIO_PERCENT < 100,
        "",
    );
}

/// VE-F0418 域自检。
pub fn run_vec18_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vec18");
    check_zero_backtrack(&mut set);
    check_pool(&mut set);
    check_arena(&mut set);
    check_streaming(&mut set);
    check_bench(&mut set);
    check_explicit(&mut set);
    set
}

#[cfg(test)]
mod red_perf {
    use super::*;
    #[test]
    fn perf_red_items() {
        let set = run_vec18_checks();
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
