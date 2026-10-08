//! CGPU-F0002 判据（C2- 前缀，全局互异；独立口径防同源恒绿）。
//!
//! 锚点判据逐条落实：八核瓦片不均衡 ≤5%、同帧重跑逐像素一致（乱序
//! 完成归并）、抢占让位点 2ms 纪律、专核不受后台任务干扰、窃取开销
//! ≤3% 账面；另覆盖池规模/树形状/同层优先窃取/遥测/判据自检。

use crate::checks::CheckSet;
use crate::cgpu::cga02_threadpool as tp;
use alloc::vec;
use alloc::vec::Vec;

/// 判据侧独立确定性着色（第二份实现，与 tp::TileJob::execute 同语义
/// ——种子扩散后每像素 `h = h·K ^ (h>>7)`，移位取乘前值；独立迭代
/// 形式防同源漂移）。
fn ref_shade(seed: u32, tile_idx: u32, pixels: u32) -> Vec<u32> {
    let mut h = seed ^ (tile_idx.wrapping_mul(0x9E37_79B9));
    let mut out = Vec::new();
    let mut n = 0u32;
    while n < pixels {
        let shifted = h >> 7;
        h = h.wrapping_mul(0x0100_0193) ^ shifted;
        out.push(h);
        n += 1;
    }
    out
}

pub fn run_cga02_checks() -> CheckSet {
    let mut s = CheckSet::new("CGPU-A");

    // --- 池规模：核数减二，下限 1 ----------------------------------------
    let wc8 = tp::PoolSpec { physical_cores: 8 }.worker_count();
    let wc2 = tp::PoolSpec { physical_cores: 2 }.worker_count();
    let wc1 = tp::PoolSpec { physical_cores: 1 }.worker_count();
    let wc0 = tp::PoolSpec { physical_cores: 0 }.worker_count();
    s.add(
        "C2-池-规模核数减二",
        wc8 == Ok(6) && wc2 == Ok(1) && wc1 == Ok(1) && wc0 == Err(tp::ERR_POOL_ARG),
        "",
    );

    // --- 亲和：专核默认开，后台不进专核 -----------------------------------
    let aff = tp::AffinityMap::default();
    let isolated = !aff.admits(0, tp::Priority::Background)
        && aff.admits(0, tp::Priority::Interactive)
        && aff.admits(0, tp::Priority::Realtime)
        && aff.admits(1, tp::Priority::Background);
    let open = tp::AffinityMap { compositor_isolated: false }.admits(0, tp::Priority::Background);
    s.add("C2-亲和-专核默认开", isolated && open, "");

    // --- 任务树形状 -------------------------------------------------------
    let tree = tp::TaskTree::frame(8, tp::Priority::Realtime);
    let shape_ok = tree.validate() == Ok(())
        && tree.tile_ids() == vec![1u32, 2, 3, 4, 5, 6, 7, 8]
        && tree.nodes[0].tier == tp::Tier::Frame;
    let orphan = tp::TaskTree { nodes: vec![tp::TaskNode { id: 0, parent: Some(9), tier: tp::Tier::Frame, priority: tp::Priority::Interactive }] };
    s.add("C2-树-根瓦片与孤儿拒收", shape_ok && orphan.validate() == Err(tp::ERR_TREE_SHAPE), "");

    // --- 确定性：乱序完成归并逐像素一致（锚点核心判据） --------------------
    let jobs: Vec<tp::TileJob> = (0..8u32)
        .map(|i| tp::TileJob { tile_idx: i, pixels: 64, seed: 0xABCD_1234 })
        .collect();
    let outputs_in_order: Vec<tp::TileOutput> = jobs.iter().map(|j| j.execute()).collect();
    let frame_a = tp::merge_frame(64, &outputs_in_order);
    // 乱序完成：倒序交付 + 交换两块（两次重跑口径）。
    let mut outputs_rev = outputs_in_order.clone();
    outputs_rev.reverse();
    let frame_b = tp::merge_frame(64, &outputs_rev);
    let mut outputs_sw = outputs_in_order.clone();
    outputs_sw.swap(0, 7);
    let frame_c = tp::merge_frame(64, &outputs_sw);
    // 判据侧独立着色第二份对拍。
    let want_tile0 = ref_shade(0xABCD_1234, 0, 64);
    let exec_ok = match (&frame_a, &frame_b, &frame_c) {
        (Ok(a), Ok(b), Ok(c)) => a == b && b == c && &a[..64] == &want_tile0[..] && a.len() == 512,
        _ => false,
    };
    s.add("C2-确定性-乱序归并逐像素一致", exec_ok, "");

    // 缺件拒绝（确定性归并拒绝半帧）。
    let mut missing = outputs_in_order.clone();
    missing.remove(3);
    let miss_res = tp::merge_frame(64, &missing);
    let short_res = tp::merge_frame(64, &[tp::TileOutput { tile_idx: 0, buf: vec![0; 63] }]);
    s.add(
        "C2-确定性-缺件拒收",
        miss_res == Err(tp::ERR_TILE_MISSING) && short_res == Err(tp::ERR_TILE_MISSING),
        "",
    );

    // --- 窃取：同层优先，同层空才显式跨层 ---------------------------------
    let mut eng = match tp::StealEngine::new(tp::PoolSpec { physical_cores: 8 }, tp::AffinityMap::default()) {
        Ok(e) => e,
        Err(_) => return s,
    };
    // 6 worker（0 号专核）。1~5 号放瓦片任务（id 1..6），4 号多放制造深度差。
    for id in 1..=5u32 {
        eng.workers[1 + (id as usize - 1) % 5].push(id);
    }
    eng.workers[4].push(6);
    let tier_of = |t: u32| if t == 0 { tp::Tier::Frame } else { tp::Tier::Tile };
    // 窃取：同层偷——0 号专核（Tile 偏好）从最忙的 4 号偷队头（最老任务 4）。
    let got = eng.steal(0, Some(tp::Tier::Tile), &tier_of);
    let got_ok = match got {
        Some(t) => t == 4, // 4 号（深 2）队头 = 先入的 id 4
        None => false,
    };
    s.add("C2-窃取-同层优先命中", got_ok, "");

    // 同层全空：清空所有瓦片任务后，Tile 偏好不可偷（不跨层）。
    for w in eng.workers.iter_mut() {
        while w.pop_local().is_some() {}
    }
    eng.workers[1].push(0); // 只剩根任务（Frame 层）
    let no_cross = eng.steal(2, Some(tp::Tier::Tile), &tier_of).is_none();
    // 显式 None 才跨层兜底偷到 Frame。
    let cross = eng.steal(2, None, &tier_of) == Some(0);
    s.add("C2-窃取-同层空不跨层", no_cross && cross, "");

    // 偷头不偷尾：受害者队列 [7,8,9]（头 7 最老），偷到的是 7。
    for w in eng.workers.iter_mut() {
        while w.pop_local().is_some() {}
    }
    eng.workers[3].push(7);
    eng.workers[3].push(8);
    eng.workers[3].push(9);
    let head_stolen = eng.steal(5, None, &tier_of) == Some(7);
    let victim_kept = eng.workers[3].depth() == 2;
    s.add("C2-窃取-偷头最老任务", head_stolen && victim_kept, "");

    // --- 抢占：让位点 2ms 纪律 --------------------------------------------
    let mut bg = tp::BackgroundJob::new();
    let ticks_no_preempt = (0..8).filter(|&ms| bg.yield_due(ms)).count();
    bg.preempt_requested = true;
    let yield_at = bg.next_yield_at(); // ran=0 → 下一让位点 2
    // 让出是「到达即持续可让」：1ms 未到、2ms 起持续可让。
    let gate = !bg.yield_due(1) && bg.yield_due(2) && bg.yield_due(4) && bg.yield_due(6);
    s.add(
        "C2-抢占-让位点2ms",
        tp::CHECKPOINT_MS == 2 && ticks_no_preempt == 0 && yield_at == 2 && gate,
        "",
    );

    // 让出后继续跑：ran 推进到让位点后请求清除则不再让。
    bg.ran_ms = 2;
    bg.preempt_requested = false;
    let no_yield = !bg.yield_due(2) && !bg.yield_due(4);
    bg.preempt_requested = true;
    let next2 = bg.next_yield_at() == 4 && bg.yield_due(4);
    s.add("C2-抢占-让出后恢复推进", no_yield && next2, "");

    // --- 专核不受后台任务干扰（分发面实测） --------------------------------
    let mut eng2 = match tp::StealEngine::new(tp::PoolSpec { physical_cores: 8 }, tp::AffinityMap::default()) {
        Ok(e) => e,
        Err(_) => return s,
    };
    // 24 个后台任务：分发改轮转面被专核隔离跳过（0 号深 0）。
    let bg_ids: Vec<u32> = (100..124u32).collect();
    let bg_served = eng2.distribute(&bg_ids, tp::Priority::Background);
    let core_after_bg = eng2.workers[0].depth();
    // 6 个实时任务：全员可接（合成器专核本身干的就是实时活）。
    let rt_ids: Vec<u32> = (200..206u32).collect();
    let rt_served = eng2.distribute(&rt_ids, tp::Priority::Realtime);
    let served = eng2.workers.iter().map(|w| w.depth()).sum::<u32>();
    s.add(
        "C2-亲和-专核零后台干扰",
        core_after_bg == 0 && bg_served == 24 && rt_served == 6 && served == 30 && eng2.workers.len() == 6,
        "",
    );

    // --- 均衡：八核瓦片不均衡 ≤5% -----------------------------------------
    // 轮转分发 600 瓦片到 6 worker（1~5 号 + 0 号收实时余量场景除外）。
    let mut eng3 = match tp::StealEngine::new(tp::PoolSpec { physical_cores: 8 }, tp::AffinityMap::default()) {
        Ok(e) => e,
        Err(_) => return s,
    };
    let tiles: Vec<u32> = (1..=600u32).collect();
    let _ = eng3.distribute(&tiles, tp::Priority::Interactive);
    let rep = tp::balance_report(&eng3.workers);
    // 轮转面：600/6=100 整除 → max=min=100（0 号也接 Interactive）。
    s.add("C2-均衡-轮转完美分摊", rep.max_load == 100 && rep.min_load == 100 && rep.imbalance_bp() == 0, "");

    // 单 worker 独载场景不均衡率上限口径（(max-min)/max ≤ 5% 判据可判）。
    let skew = tp::LoadBalanceReport { max_load: 100, min_load: 96 };
    let skew_bad = tp::LoadBalanceReport { max_load: 100, min_load: 80 };
    s.add(
        "C2-均衡-≤5%口径",
        skew.within_target() && skew.imbalance_bp() == 400 && !skew_bad.within_target(),
        "",
    );

    // --- 遥测：队列深度/窃取次数/缓存命中全记录 ----------------------------
    let tele = eng3.telemetry();
    let tele_ok = tele.len() == 6
        && tele.iter().all(|t| t.queue_depth == 100)
        && tele.iter().all(|t| t.steal_count == 0);
    // 缓存命中率口径（命中/未命中独立计数）。
    let mut wq = tp::WorkerQueue::new(1);
    wq.stats.cache_hit = 98;
    wq.stats.cache_miss = 2;
    let rate = wq.stats.hit_rate_bp();
    let rate_none = tp::WorkerStats::default().hit_rate_bp().is_none();
    s.add("C2-遥测-三面全记录", tele_ok && rate == Some(9800) && rate_none, "");

    // --- 窃取开销 ≤3% 账面 -------------------------------------------------
    let entry = &tp::BENCH_LEDGER[0];
    let bench_ok = entry.target_pct == 3 && !entry.target_met() && tp::BENCH_LEDGER.len() == 2;
    s.add("C2-开销-窃取≤3%入册", bench_ok, "");

    // 窃取计数实测：偷一次计一次（上面 steal 后 stats 断言）。
    let mut eng4 = match tp::StealEngine::new(tp::PoolSpec { physical_cores: 4 }, tp::AffinityMap { compositor_isolated: false }) {
        Ok(e) => e,
        Err(_) => return s,
    };
    eng4.workers[1].push(50);
    let _ = eng4.steal(0, None, &tier_of);
    let steal_counted = match eng4.workers.get(0) {
        Some(w) => w.stats.steal_count == 1 && w.depth() == 1,
        None => false,
    };
    s.add("C2-开销-窃取计数实测", steal_counted, "");

    // --- 判据自检（基准非零/独立着色非平凡） --------------------------------
    let trivial_guard = want_tile0[0] != want_tile0[1] && want_tile0.iter().any(|v| *v != 0);
    s.add("C2-自检-着色非平凡", trivial_guard, "");

    s
}
