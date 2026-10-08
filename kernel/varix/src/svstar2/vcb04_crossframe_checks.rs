//! CGPU-F0164 自检 · 跨帧依赖与围栏传播（CGPU-B 域）
//!
//! **锚点判据逐条对应**（`#CGPU-F0164`「跨帧等待正确、链式传播、超时
//! 降级、持久化正确、跨 3 帧用例」）：
//!
//! | 锚点判据 | 自检组 |
//! |---|---|
//! | 跨帧等待正确 | `CB04-等待-*`（Blocked→Fulfilled→Clear 全程+登记闸三向） |
//! | 链式传播 | `CB04-链式-*`（3 链 Lost 下推+计数+幂等+Fulfilled 不翻脸） |
//! | 超时降级 | `CB04-超时-*`（恰边界 N-1/N 双向+age_out 链式降级+计账） |
//! | 持久化正确 | `CB04-持久-*`（图释放账本存活+多围栏共存+计数初始） |
//! | 跨 3 帧用例 | `CB04-三帧-*`（happy/lost/混合三路闭环） |
//! | 判据元 | `CB04-判据-*`（版本/错误码/三态封闭/条数对账） |
//!
//! **判据设计硬规矩**：期望值判据侧独立手算；不变量两头都测（恰 N-1
//! 帧 Pending+恰 N 帧 Lost）；判据区零 panic 面。

use crate::checks::CheckSet;
use crate::svstar2::vcb01_framegraph as fg;
use crate::svstar2::vcb04_crossframe as cf;

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 辅助
// ---------------------------------------------------------------------------

fn key(frame: u64, node: u32) -> cf::TaskKey {
    cf::TaskKey { frame, node }
}

/// 三链语料：d1(f1) ← c2(f2) ← c3(f3)，登记帧 2/3。
fn chain() -> (cf::CrossFrameLedger, cf::TaskKey, cf::TaskKey, cf::TaskKey) {
    let d1 = key(1, 7);
    let c2 = key(2, 3);
    let c3 = key(3, 9);
    let mut led = cf::CrossFrameLedger::new();
    let _ = led.attach(d1, c2, 2);
    let _ = led.attach(c2, c3, 3);
    (led, d1, c2, c3)
}

// ---------------------------------------------------------------------------
// 组一：跨帧等待正确
// ---------------------------------------------------------------------------

fn chk_wait(s: &mut CheckSet) {
    // CB04-等待-01：登记后生产者 Pending、消费者 Blocked（跨帧边生效）。
    let (led, d1, c2, _) = chain();
    let ok = led.state_of(d1) == Some(cf::FenceState::Pending)
        && led.upstream_state(c2) == cf::Upstream::Blocked;
    s.add("CB04-等待-01", ok, "跨帧边登记即阻塞本帧消费者");

    // CB04-等待-02：resolve 后消费者 Clear（放行）。
    let (mut led, d1, c2, _) = chain();
    let r = led.resolve(d1);
    let ok = matches!(r, Ok(1)) // 直接 waiter 恰 1（c2）
        && led.state_of(d1) == Some(cf::FenceState::Fulfilled)
        && led.upstream_state(c2) == cf::Upstream::Clear;
    s.add("CB04-等待-02", ok, "resolve 放行等待者（直接 waiter 恰 1）");

    // CB04-等待-03：无跨帧上游 → Clear（帧内任务不受账本影响）。
    let (led, _, _, _) = chain();
    let ok = led.upstream_state(key(2, 50)) == cf::Upstream::Clear;
    s.add("CB04-等待-03", ok, "无跨帧上游任务直接放行");

    // CB04-等待-04：重复同对边拒绝 E_CROSSFRAME_DUP（独立账本：条目不增）。
    let mut led = cf::CrossFrameLedger::new();
    let d1 = key(1, 7);
    let c2 = key(2, 3);
    let _ = led.attach(d1, c2, 2);
    let r = led.attach(d1, c2, 2);
    let ok = r == Err(cf::E_CROSSFRAME_DUP) && led.live_count() == 1;
    s.add("CB04-等待-04", ok, "重复登记拒绝且不建第二条目");

    // CB04-等待-05：帧号不向前拒绝（同帧等待/倒退等待同码，条目不增）。
    let mut led = cf::CrossFrameLedger::new();
    let d1 = key(1, 7);
    let _ = led.attach(d1, key(2, 3), 2);
    let same = led.attach(key(1, 8), key(1, 9), 2);
    let back = led.attach(key(3, 1), d1, 2); // waiter(f1) 等 producer(f3)——倒退
    let ok = same == Err(cf::E_CROSSFRAME_BACKWARD)
        && back == Err(cf::E_CROSSFRAME_BACKWARD)
        && led.live_count() == 1;
    s.add("CB04-等待-05", ok, "同帧与倒退等待均被登记闸拒绝");

    // CB04-等待-06：resolve 未知键 E_CROSSFRAME_UNKNOWN。
    let (mut led, _, _, _) = chain();
    let ok = led.resolve(key(9, 9)) == Err(cf::E_CROSSFRAME_UNKNOWN)
        && led.mark_lost(key(9, 9)) == Err(cf::E_CROSSFRAME_UNKNOWN);
    s.add("CB04-等待-06", ok, "未知键 resolve/mark_lost 均显性拒绝");

    // CB04-等待-07：resolve 幂等（终态不改账不重计）。
    let (mut led, d1, _, _) = chain();
    let _ = led.resolve(d1);
    let (f1, l1) = (led.fulfilled_events, led.lost_events);
    let r2 = led.resolve(d1);
    let ok = r2 == Ok(0)
        && led.fulfilled_events == f1
        && led.lost_events == l1
        && led.state_of(d1) == Some(cf::FenceState::Fulfilled);
    s.add("CB04-等待-07", ok, "终态重复 resolve 零副作用");
}

// ---------------------------------------------------------------------------
// 组二：链式传播
// ---------------------------------------------------------------------------

fn chk_chain(s: &mut CheckSet) {
    // CB04-链式-01：源头 Lost 沿 3 链下推——c2 条目转 Lost，c3 是叶子
    //  waiter（无条目）只计降级；传播长度 2（c2 转移+c3 叶子）。
    let (mut led, d1, c2, _c3) = chain();
    let r = led.mark_lost(d1);
    let ok = matches!(r, Ok(2))
        && led.state_of(d1) == Some(cf::FenceState::Lost)
        && led.state_of(c2) == Some(cf::FenceState::Lost)
        && led.state_of(_c3).is_none()
        && led.upstream_state(_c3) == cf::Upstream::Degraded;
    s.add("CB04-链式-01", ok, "源头标记丢失沿链全量下推（传播 2 级）");

    // CB04-链式-02：降级计账恰 2（c2、c3 两消费者；源头不计）。
    let ok = led.degraded_waiters == 2 && led.lost_events == 1;
    s.add("CB04-链式-02", ok, "降级消费者计数恰 2 丢失源头计 1");

    // CB04-链式-03：Fulfilled 不沿链传播（d1 完成只解除 c2 阻塞——c2
    //  任务自身还要跑，须由它自己的 resolve 放行 c3）；重复丢失不翻脸。
    let (mut led, d1, c2, c3) = chain();
    let _ = led.resolve(d1);
    let after_resolve = led.state_of(c2);
    let r = led.mark_lost(d1); // 已 Fulfilled——幂等路径
    let ok = after_resolve == Some(cf::FenceState::Pending)
        && r == Ok(0)
        && led.state_of(c2) == Some(cf::FenceState::Pending)
        && led.upstream_state(c3) == cf::Upstream::Blocked;
    s.add("CB04-链式-03", ok, "Fulfilled 不传播且不因丢失标记翻脸");

    // CB04-链式-04：已 Lost 幂等（二次标记零副作用）。
    let (mut led, d1, _, _) = chain();
    let _ = led.mark_lost(d1);
    let (l1, g1) = (led.lost_events, led.degraded_waiters);
    let r = led.mark_lost(d1);
    let ok = r == Ok(0)
        && led.lost_events == l1
        && led.degraded_waiters == g1;
    s.add("CB04-链式-04", ok, "重复标记丢失零副作用");

    // CB04-链式-05：部分传播——链中段丢失，上游不受影响下游全灭
    //  （c3 叶子经 upstream_state 判 Degraded）。
    let (mut led, _d1, c2, c3) = chain();
    let r = led.mark_lost(c2);
    let ok = matches!(r, Ok(1))
        && led.state_of(c2) == Some(cf::FenceState::Lost)
        && led.upstream_state(c3) == cf::Upstream::Degraded
        && led.state_of(_d1) == Some(cf::FenceState::Pending);
    s.add("CB04-链式-05", ok, "中段丢失只影响其下游（上游仍 Pending）");
}

// ---------------------------------------------------------------------------
// 组三：超时降级
// ---------------------------------------------------------------------------

fn chk_timeout(s: &mut CheckSet) {
    // CB04-超时-01：恰 deadline-1 帧仍 Pending（边界内不丢）。
    let (led, d1, _, _) = chain(); // born=2, deadline=2+4=6
    let mut late = led.clone();
    let _ = late.age_out(5);
    let ok = led.state_of(d1) == Some(cf::FenceState::Pending)
        && late.state_of(d1) == Some(cf::FenceState::Pending);
    s.add("CB04-超时-01", ok, "恰 deadline-1 帧仍在等待（不判丢）");

    // CB04-超时-02：恰 deadline 帧 Lost（>= 语义——>⇄>= 变异即红）。
    //  d1 born=2 deadline=6 判丢；c2 born=3 未到期但被链式传播 Lost。
    let (mut led, d1, c2, _c3) = chain();
    let n = led.age_out(6);
    let ok = n == 1
        && led.state_of(d1) == Some(cf::FenceState::Lost)
        && led.state_of(c2) == Some(cf::FenceState::Lost)
        && led.upstream_state(_c3) == cf::Upstream::Degraded;
    s.add("CB04-超时-02", ok, "恰 deadline 帧判丢且链式传播");

    // CB04-超时-03：age_out 后消费者走降级（Degraded 优先）。
    let ok = led.upstream_state(c2) == cf::Upstream::Degraded
        && led.upstream_state(_c3) == cf::Upstream::Degraded
        && led.degraded_waiters == 2;
    s.add("CB04-超时-03", ok, "超时链上消费者全部转降级");

    // CB04-超时-04：lost_events 恰 1（源头一次，传播不重复计）。
    let ok = led.lost_events == 1 && led.fulfilled_events == 0;
    s.add("CB04-超时-04", ok, "丢失计数源头恰 1 完成计数 0");

    // CB04-超时-05：超时窗与常量互洽（deadline == born + TIMEOUT）。
    let (led, d1, _, _) = chain();
    let e = led.entries.iter().find(|e| e.key == d1);
    let ok = match e {
        Some(e) => e.born_frame == 2 && e.deadline == 2 + cf::CROSSFRAME_TIMEOUT_FRAMES,
        None => false,
    };
    s.add("CB04-超时-05", ok, "deadline=born+N 与超时常量互洽");
}

// ---------------------------------------------------------------------------
// 组四：持久化正确（跨帧边不随帧图释放）
// ---------------------------------------------------------------------------

fn chk_persist(s: &mut CheckSet) {
    // CB04-持久-01：帧图构造→登记跨帧边→图消费 drop→账本状态存活。
    let mut b = fg::FrameGraphBuilder::for_frame(1);
    let _ = b.add_node(fg::NodeSpec {
        kind: fg::NodeKind::Copy,
        reads: Vec::new(),
        writes: Vec::new(),
        budget_us: 10,
        priority: 0,
        deadline_us: 0,
        queue: fg::QueueKind::Transfer,
    });
    let g = match b.into_snapshot() {
        Ok(g) => g,
        Err(_) => {
            s.add("CB04-持久-01", false, "图构造失败");
            return;
        }
    };
    let d1 = key(1, 0);
    let c2 = key(2, 0);
    let mut led = cf::CrossFrameLedger::new();
    let _ = led.attach(d1, c2, 2);
    drop(g); // 帧图释放——账本不随图蒸发
    let ok = led.state_of(d1) == Some(cf::FenceState::Pending)
        && led.upstream_state(c2) == cf::Upstream::Blocked;
    s.add("CB04-持久-01", ok, "帧图 drop 后围栏账本状态存活");

    // CB04-持久-02：多围栏共存互不串扰（两生产者独立 resolve）。
    let (mut led, _d1, _c2, _c3) = chain();
    let e9 = key(1, 20);
    let _ = led.attach(e9, key(2, 21), 2);
    let _ = led.resolve(e9);
    let ok = led.state_of(e9) == Some(cf::FenceState::Fulfilled)
        && led.state_of(key(1, 7)) == Some(cf::FenceState::Pending)
        && led.upstream_state(key(2, 21)) == cf::Upstream::Clear;
    s.add("CB04-持久-02", ok, "多围栏独立状态互不串扰");

    // CB04-持久-03：三计数初始为 0（无静默路径——账本从零起账）。
    let led = cf::CrossFrameLedger::new();
    let ok = led.fulfilled_events == 0
        && led.lost_events == 0
        && led.degraded_waiters == 0
        && led.live_count() == 0;
    s.add("CB04-持久-03", ok, "空账本四账全零");

    // CB04-持久-04：账本可克隆（跨帧移交/快照对账的持久化面）。
    let (led, d1, _, _) = chain();
    let snap = led.clone();
    let ok = snap == led && snap.state_of(d1) == Some(cf::FenceState::Pending);
    s.add("CB04-持久-04", ok, "账本克隆逐字段相等（快照对账可用）");
}

// ---------------------------------------------------------------------------
// 组五：跨 3 帧用例
// ---------------------------------------------------------------------------

fn chk_three_frames(s: &mut CheckSet) {
    // CB04-三帧-01：happy path——逐级 resolve，尾帧 Clear，完成计 2。
    let mut led = cf::three_frame_happy_path();
    let c3 = key(3, 9);
    let ok = led.upstream_state(c3) == cf::Upstream::Clear
        && led.fulfilled_events == 2
        && led.lost_events == 0;
    s.add("CB04-三帧-01", ok, "跨 3 帧完成链尾帧放行");

    // CB04-三帧-02：lost path——源头超时，尾帧 Degraded，降级计 2。
    let led = cf::three_frame_lost_path();
    let ok = led.upstream_state(key(3, 9)) == cf::Upstream::Degraded
        && led.lost_events == 1
        && led.degraded_waiters == 2;
    s.add("CB04-三帧-02", ok, "跨 3 帧丢失链尾帧走降级");

    // CB04-三帧-03：混合路径——一支完成一支丢失互不干扰。
    let d1 = key(1, 1); // 好支生产者
    let e1 = key(1, 2); // 坏支生产者
    let c2a = key(2, 1);
    let c2b = key(2, 2);
    let mut led = cf::CrossFrameLedger::new();
    let _ = led.attach(d1, c2a, 2);
    let _ = led.attach(e1, c2b, 2);
    let _ = led.resolve(d1);
    let _ = led.mark_lost(e1);
    let ok = led.upstream_state(c2a) == cf::Upstream::Clear
        && led.upstream_state(c2b) == cf::Upstream::Degraded
        && led.fulfilled_events == 1
        && led.lost_events == 1;
    s.add("CB04-三帧-03", ok, "同帧两支完成/丢失各走各路");

    // CB04-三帧-04：读屏行携带四计数。
    let mut led = cf::three_frame_happy_path();
    let line = led.screen_line();
    let ok = line.contains("2 围栏") && line.contains("完成 2");
    s.add("CB04-三帧-04", ok, "账本读屏行携带围栏数与完成计数");
}

// ---------------------------------------------------------------------------
// 组六：判据元
// ---------------------------------------------------------------------------

fn chk_meta(s: &mut CheckSet) {
    // CB04-判据-01：版本字面量钉死。
    let ok = cf::CROSSFRAME_VERSION == "CB04-crossframe-v1";
    s.add("CB04-判据-01", ok, "CROSSFRAME_VERSION 字面量钉死");

    // CB04-判据-02：三错误码非空互异。
    let ok = !cf::E_CROSSFRAME_UNKNOWN.is_empty()
        && !cf::E_CROSSFRAME_DUP.is_empty()
        && !cf::E_CROSSFRAME_BACKWARD.is_empty()
        && cf::E_CROSSFRAME_UNKNOWN != cf::E_CROSSFRAME_DUP
        && cf::E_CROSSFRAME_DUP != cf::E_CROSSFRAME_BACKWARD
        && cf::E_CROSSFRAME_UNKNOWN != cf::E_CROSSFRAME_BACKWARD;
    s.add("CB04-判据-02", ok, "三错误码非空互异");

    // CB04-判据-03：围栏三态与上游三态封闭（tag 互异+中文互异）。
    let fence = [
        cf::FenceState::Pending.tag(),
        cf::FenceState::Fulfilled.tag(),
        cf::FenceState::Lost.tag(),
    ];
    let fence_zh = [
        cf::FenceState::Pending.zh(),
        cf::FenceState::Fulfilled.zh(),
        cf::FenceState::Lost.zh(),
    ];
    let up = [
        cf::Upstream::Clear.tag(),
        cf::Upstream::Blocked.tag(),
        cf::Upstream::Degraded.tag(),
    ];
    let ok = fence[0] != fence[1]
        && fence[1] != fence[2]
        && fence[0] != fence[2]
        && fence_zh[0] != fence_zh[1]
        && fence_zh[1] != fence_zh[2]
        && fence_zh[0] != fence_zh[2]
        && up[0] != up[1]
        && up[1] != up[2]
        && up[0] != up[2];
    s.add("CB04-判据-03", ok, "围栏三态与上游三态封闭可逆");
}

// ---------------------------------------------------------------------------
// 聚合
// ---------------------------------------------------------------------------

/// CGPU-F0164 域自检入口（聚合防自调：只调组函数+自身 tally）。
pub fn run_vcb04_checks() -> CheckSet {
    let mut s = CheckSet::new("CGPU-F0164");
    chk_wait(&mut s);
    chk_chain(&mut s);
    chk_timeout(&mut s);
    chk_persist(&mut s);
    chk_three_frames(&mut s);
    chk_meta(&mut s);
    // 条数对账：6 组 28 条（等待 7+链式 5+超时 5+持久 4+三帧 4+判据 3）。
    let (pass, fail) = s.tally();
    let ok = pass + fail == 28;
    s.add("CB04-判据-04", ok, "条数对账：实挂 28 条（6 组）");
    s
}
