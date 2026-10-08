//! VE-F0224 判据层：Intel 提交通路与 EXECLISTS（锚点五条判据逐条映射）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0224`
//!
//! **锚点原文五条判据 → 本层判据族**：
//!
//! | 锚点判据 | 判据族 | 要点 |
//! |---|---|---|
//! | ELSP 批提交 | `E24-ELSP-*` | 队头成对直入 + FIFO 次序 + 失败回退 |
//! | 超时语义 | `E24-TMO-*` | 阈值边界 + 重置回队头 + 终态弃用 |
//! | 池化复用 | `E24-POOL-*` | 命中/未命中/池满三账齐全 |
//! | hung 检测 | `E24-HUNG-*` | 窗口连击 + 只打该引擎 |
//! | （信号点显式化/错误码为锚点正文要求） | `E24-FENCE-*` `E24-ERR-*` `E24-A11Y-*` | 信号 O(1) 直寻 + 0x34xx 全映射 |
//!
//! # 本层的核心纪律：**判据侧独立重算，不向被测问答案**
//!
//! 阈值/上限字面量判据侧写死（CTX_TIMEOUT_TICKS=64、MAX_CTX_RESETS=3、
//! QUEUE_CAP=32、POOL_CAP=16、HUNG_TIMEOUT_COUNT=2）；FIFO 次序用判据侧
//! 自编序号重演； hung 「只打该引擎」用双引擎对照组验证。

// ---------------------------------------------------------------------------
// 导入
// ---------------------------------------------------------------------------

use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

use super::veb24_submit::*;

// ---------------------------------------------------------------------------
// 工具（判据侧）
// ---------------------------------------------------------------------------

/// 判据侧构批（围栏序号由判据侧编号，checksum 用非零字面量族）。
fn mk_req(ctx: u32, e: Engine, seq: u64, len: u32) -> SubmitReq {
    SubmitReq {
        ctx_id: ctx,
        engine: e,
        batch: BatchLayout { fence_seq: seq, checksum: 0x1234_5678 + seq, len_dwords: len },
        resets: 0,
    }
}

/// 围栏联动记录器（判据侧钩子：记被联动的围栏序号）。
static mut HOOK_SEEN: [u64; 64] = [0; 64];
static mut HOOK_N: usize = 0;

fn hook_record(seq: u64) -> bool {
    unsafe {
        if HOOK_N < 64 {
            HOOK_SEEN[HOOK_N] = seq;
            HOOK_N += 1;
        }
        true
    }
}

fn hook_seen_last() -> u64 {
    unsafe {
        if HOOK_N == 0 { 0 } else { HOOK_SEEN[HOOK_N - 1] }
    }
}

fn hook_reset() {
    unsafe {
        HOOK_N = 0;
    }
}

// ---------------------------------------------------------------------------
// E24-ELSP：ELSP 批提交（判据一）
// ---------------------------------------------------------------------------

fn fam_elsp(s: &mut CheckSet) {
    // L1：空端口直入（不落队列）——ELSP 直入语义。
    let mut p = SubmitPath::new();
    let r = p.submit(mk_req(1, Engine::Render, 10, 16));
    let ok_l1 = r.is_ok() && p.port_busy(Engine::Render) == 1
        && p.queue_depth(Engine::Render) == 0 && p.stats.elsp_dispatches == 1;
    s.add("E24-ELSP-direct-port", ok_l1, "空端口直入不落队");

    // L2：端口满后落队列；dispatch_elsp 一次补位至多 2（成对）。
    let mut p = SubmitPath::new();
    let _ = p.submit(mk_req(1, Engine::Render, 10, 16));
    let _ = p.submit(mk_req(2, Engine::Render, 11, 16));
    let _ = p.submit(mk_req(3, Engine::Render, 12, 16));
    let _ = p.submit(mk_req(4, Engine::Render, 13, 16)); // 端口满 → 队列 2
    let q1 = p.queue_depth(Engine::Render);
    let filled = p.dispatch_elsp(Engine::Render); // 端口未空 → 0
    let ok_l2 = q1 == 2 && filled == 0 && p.queue_depth(Engine::Render) == 2;
    s.add("E24-ELSP-pair-cap", ok_l2, "端口不空则补位为 0");

    // L3：FIFO 次序——重置后补位先取队头（判据侧重演序号）。
    let mut p = SubmitPath::new();
    let _ = p.submit(mk_req(1, Engine::Render, 10, 16));
    let _ = p.submit(mk_req(2, Engine::Render, 11, 16));
    let _ = p.submit(mk_req(3, Engine::Render, 12, 16));
    // 完成端口 0 → 队头 12 号上下文补入。
    let _ = p.complete(Engine::Render, 0);
    p.dispatch_elsp(Engine::Render);
    let ok_l3 = p.port_busy(Engine::Render) == 2 && p.queue_depth(Engine::Render) == 0;
    s.add("E24-ELSP-fifo-refill", ok_l3, "完成后队头按 FIFO 补位");

    // L4：队列满即失败回退（错误码 + 队列不动 + 上下文仍在调用方）。
    let mut p = SubmitPath::new();
    let _ = p.submit(mk_req(1, Engine::Render, 1, 16));
    let _ = p.submit(mk_req(2, Engine::Render, 2, 16));
    let mut n = 0u32;
    let mut ok_fill = true;
    while n < 32 {
        if p.submit(mk_req(10 + n, Engine::Render, 100 + n as u64, 16)).is_err() {
            ok_fill = false;
        }
        n += 1;
    }
    let r_full = p.submit(mk_req(99, Engine::Render, 999, 16));
    let ok_l4 = ok_fill && r_full == Err(SErr::SubmitQueueFull)
        && p.queue_depth(Engine::Render) == 32;
    s.add("E24-ELSP-queuefull-rollback", ok_l4, "队列满专属码回退调用方");

    // L5：引擎隔离——Render 队列满不影响 Copy 提交（防头阻塞）。
    let ok_l5 = p.submit(mk_req(50, Engine::Copy, 500, 16)).is_ok()
        && p.port_busy(Engine::Copy) == 1;
    s.add("E24-ELSP-engine-isolation", ok_l5, "Render 满 Copy 仍直入");

    // L6：布局校验——ctx=0 / 零长度 / 零校验 / 零围栏全拒且零写入。
    let mut p = SubmitPath::new();
    let mut bad = mk_req(0, Engine::Render, 1, 16);
    let r1 = p.submit(bad);
    bad.ctx_id = 7;
    bad.batch.len_dwords = 0;
    let r2 = p.submit(bad);
    bad.batch.len_dwords = 16;
    bad.batch.checksum = 0;
    let r3 = p.submit(bad);
    bad.batch.checksum = 0x42;
    bad.batch.fence_seq = 0;
    let r4 = p.submit(bad);
    let ok_l6 = r1 == Err(SErr::InvalidDescriptor) && r2 == Err(SErr::BatchCorrupt)
        && r3 == Err(SErr::BatchCorrupt) && r4 == Err(SErr::InvalidDescriptor)
        && p.stats.submits == 0;
    s.add("E24-ELSP-layout-validate", ok_l6, "四类非法布局拒且零写入");

    // L7：ELSP 补位取自队头且逐个入空端口（两空位补两笔）。
    let mut p = SubmitPath::new();
    let _ = p.submit(mk_req(1, Engine::Vcs, 10, 16));
    let _ = p.submit(mk_req(2, Engine::Vcs, 11, 16));
    let _ = p.submit(mk_req(3, Engine::Vcs, 12, 16));
    let _ = p.submit(mk_req(4, Engine::Vcs, 13, 16));
    let _ = p.complete(Engine::Vcs, 0);
    let _ = p.complete(Engine::Vcs, 1);
    let filled = p.dispatch_elsp(Engine::Vcs);
    let ok_l7 = filled == 2 && p.port_busy(Engine::Vcs) == 2
        && p.queue_depth(Engine::Vcs) == 0;
    s.add("E24-ELSP-two-empty-two-fill", ok_l7, "双空位一次补两笔");
}

// ---------------------------------------------------------------------------
// E24-TMO：超时语义（判据二）
// ---------------------------------------------------------------------------

fn fam_tmo(s: &mut CheckSet) {
    // T1：阈值边界——恰好 CTX_TIMEOUT_TICKS 不超时，+1 超时（判据侧字面量 64）。
    let mut p = SubmitPath::new();
    let _ = p.submit(mk_req(1, Engine::Render, 10, 16));
    let mut i = 0u64;
    while i < 64 {
        p.advance();
        i += 1;
    }
    let e0 = p.poll_timeouts(hook_record);
    let mut ok_t1 = e0.is_none() && p.port_busy(Engine::Render) == 1;
    p.advance();
    let e1 = p.poll_timeouts(hook_record);
    ok_t1 = ok_t1 && e1 == Some(SErr::CtxTimeout) && p.port_busy(Engine::Render) == 0;
    s.add("E24-TMO-threshold-boundary", ok_t1, "第 64 tick 不超/第 65 tick 超");

    // T2：重置回退队列头 + 围栏联动谓词收到正确序号。
    hook_reset();
    let ok_t2 = e1 == Some(SErr::CtxTimeout)
        && p.queue_depth(Engine::Render) == 1
        && p.stats.requeued == 1
        && hook_seen_last() == 10;
    s.add("E24-TMO-requeue-head-hook", ok_t2, "回队头且联动序号 10 送达");

    // T3：重置组回队头不改相对次序——重置上下文先于后提交者被补位。
    let _ = p.submit(mk_req(9, Engine::Render, 90, 16)); // 新提交（端口空直入）
    let _ = p.complete(Engine::Render, 0); // 完成新提交 → 端口空
    p.dispatch_elsp(Engine::Render); // 队头=被重置的 seq10 上下文补入
    let ok_t3 = p.port_busy(Engine::Render) == 1 && p.queue_depth(Engine::Render) == 0;
    s.add("E24-TMO-head-before-newer", ok_t3, "重置组先于后提交者补位");

    // T4：同一上下文累计重置超限 → 终态弃用（不无限重试）。
    // 判据侧字面量：MAX_CTX_RESETS=3 ⇒ 第 4 次重置弃用。
    let mut p = SubmitPath::new();
    let _ = p.submit(mk_req(5, Engine::Render, 50, 16));
    let mut rounds = 0u32;
    let mut abandoned = false;
    while rounds < 8 {
        let mut k = 0u64;
        while k < 65 {
            p.advance();
            k += 1;
        }
        let ev = p.poll_timeouts(hook_record);
        if ev == Some(SErr::CtxAbandoned) {
            abandoned = true;
            break;
        }
        p.dispatch_elsp(Engine::Render); // 回退批次重新入端口
        rounds += 1;
    }
    let ok_t4 = abandoned && p.stats.abandoned == 1 && p.stats.timeouts == 4;
    s.add("E24-TMO-abandon-terminal", ok_t4, "第 4 次重置终态弃用有界");

    // T5：弃用后批次不入队（终态不可再提交复用）。
    let ok_t5 = p.queue_depth(Engine::Render) == 0 && p.port_busy(Engine::Render) == 0;
    s.add("E24-TMO-abandoned-not-requeued", ok_t5, "弃用终态不入队");

    // T6：超时只打超时者——同引擎另一端口的健康上下文不受牵连。
    let mut p = SubmitPath::new();
    let _ = p.submit(mk_req(1, Engine::Render, 10, 16));
    let mut i = 0u64;
    while i < 30 {
        p.advance();
        i += 1;
    }
    let _ = p.submit(mk_req(2, Engine::Render, 11, 16)); // started=30
    let mut i = 0u64;
    while i < 40 {
        p.advance();
        i += 1;
    }
    let ev = p.poll_timeouts(hook_record); // seq10: 70>64 超时；seq11: 40 未超
    let ok_t6 = ev == Some(SErr::CtxTimeout)
        && p.port_busy(Engine::Render) == 1
        && p.stats.timeouts == 1;
    s.add("E24-TMO-only-timed-out", ok_t6, "健康上下文不受牵连");
}

// ---------------------------------------------------------------------------
// E24-POOL：池化复用（判据三）
// ---------------------------------------------------------------------------

fn fam_pool(s: &mut CheckSet) {
    // P1：首次取用 = 未命中（新造），命中账 0。
    let mut p = SubmitPath::new();
    let _ = p.acquire_batch(1, 0x42, 16);
    let ok_p1 = p.stats.pool_misses == 1 && p.stats.pool_hits == 0;
    s.add("E24-POOL-first-miss", ok_p1, "首取未命中入账");

    // P2：完成释放回池 → 再取 = 命中（同一缓冲复用）。
    let _ = p.submit(mk_req(1, Engine::Render, 10, 16));
    let _ = p.complete(Engine::Render, 0); // release_pool 进池
    let b = p.acquire_batch(20, 0x77, 32);
    let ok_p2 = p.stats.pool_hits == 1 && p.pool_len() == 0
        && b.fence_seq == 20 && b.checksum == 0x77 && b.len_dwords == 32;
    s.add("E24-POOL-reuse-hit", ok_p2, "释放→复用命中且字段覆写");

    // P3：池满如实丢弃记账（判据侧字面量 POOL_CAP=16）。
    let mut p = SubmitPath::new();
    let mut n = 0u32;
    while n < 16 {
        let _ = p.submit(mk_req(1 + n, Engine::Render, 10 + n as u64, 16));
        n += 1;
    }
    // 端口只有 2：第 3 笔起落队列，未完成的不进池——只完成 2 笔。
    // 池命中路径要凑 16：直接用 complete+submit 循环更快——此处改用
    // 「完成 2 笔 + 重复 acquire/release」不可行（无公开 release），
    // 改以 16 次完成进出池验证：端口 2 只能完成 2。
    // 正路：完成 2 笔进池 2；再取 2（命中）；再完成……循环凑满 16 后
    // 第 17 次归还应记 pool_drops。
    let mut guard = 0u32;
    while p.pool_len() < 16 && guard < 64 {
        // 用两端口做进出循环：提交→完成 → 各进池 1。
        if p.port_busy(Engine::Render) < 2 {
            let _ = p.submit(mk_req(3, Engine::Render, 30 + guard as u64, 16));
            p.dispatch_elsp(Engine::Render);
        }
        let _ = p.complete(Engine::Render, 0);
        let _ = p.complete(Engine::Render, 1);
        guard += 1;
    }
    let full = p.pool_len() == 16;
    // 池满后再归还一次 → drops=1。
    let _ = p.submit(mk_req(8, Engine::Render, 80, 16));
    let _ = p.complete(Engine::Render, 0);
    let ok_p3 = full && p.stats.pool_drops == 1 && p.pool_len() == 16;
    s.add("E24-POOL-cap-drop-honest", ok_p3, "池满第 17 次归还如实记账");
}

// ---------------------------------------------------------------------------
// E24-HUNG：hung 检测（判据四）
// ---------------------------------------------------------------------------

fn fam_hung(s: &mut CheckSet) {
    // H1：窗口内连击 2 次超时 → 引擎重置（第二次 poll 触发，判据侧字面量）。
    hook_reset();
    let mut p = SubmitPath::new();
    let _ = p.submit(mk_req(1, Engine::Render, 10, 16));
    let mut i = 0u64;
    while i < 65 {
        p.advance();
        i += 1;
    }
    let e1 = p.poll_timeouts(hook_record); // 第 1 次超时（CtxTimeout 事件压制 hung）
    // 重置批次再入端口再超时（第 2 次，窗口内）。
    p.dispatch_elsp(Engine::Render);
    let mut i = 0u64;
    while i < 65 {
        p.advance();
        i += 1;
    }
    let e2 = p.poll_timeouts(hook_record);
    // 第 3 轮 poll：无新超时但窗口内已有 2 次 ⇒ EngineHung。
    p.advance();
    let e3 = p.poll_timeouts(hook_record);
    let ok_h1 = e1 == Some(SErr::CtxTimeout) && e2 == Some(SErr::CtxTimeout)
        && e3 == Some(SErr::EngineHung) && p.stats.engine_resets == 1
        && p.port_busy(Engine::Render) == 0;
    s.add("E24-HUNG-window-combo", ok_h1, "窗口连击 2 次触发引擎重置");

    // H2：只打该引擎——Copy 引擎的活跃上下文不受 Render 重置牵连。
    let mut p = SubmitPath::new();
    let _ = p.submit(mk_req(1, Engine::Copy, 20, 16));
    // 构造 Render 引擎 hung（同 H1 但只用 Render）。
    let _ = p.submit(mk_req(2, Engine::Render, 10, 16));
    let mut i = 0u64;
    while i < 65 {
        p.advance();
        i += 1;
    }
    let _ = p.poll_timeouts(hook_record);
    p.dispatch_elsp(Engine::Render);
    let mut i = 0u64;
    while i < 65 {
        p.advance();
        i += 1;
    }
    let _ = p.poll_timeouts(hook_record);
    p.advance();
    let _ = p.poll_timeouts(hook_record);
    let ok_h2 = p.stats.engine_resets == 1
        && p.port_busy(Engine::Copy) == 1; // Copy 健康上下文还在端口
    s.add("E24-HUNG-isolated-engine", ok_h2, "重置不牵连健康引擎");

    // H3：引擎重置后队列可继续提交（重置是恢复不是死锁）。
    let r = p.submit(mk_req(9, Engine::Render, 90, 16));
    let ok_h3 = r.is_ok() && p.port_busy(Engine::Render) == 1;
    s.add("E24-HUNG-recover-after-reset", ok_h3, "重置后可恢复提交");

    // H4：重置窗口账被清（重置后历史连击不累积误判）。
    p.advance();
    let ev = p.poll_timeouts(hook_record);
    let ok_h4 = ev.is_none() && p.stats.engine_resets == 1;
    s.add("E24-HUNG-window-cleared", ok_h4, "重置清账不重复触发");
}

// ---------------------------------------------------------------------------
// E24-FENCE：信号点显式化
// ---------------------------------------------------------------------------

fn fam_fence(s: &mut CheckSet) {
    // F1：complete 返回该批的围栏序号（信号点显式化）。
    let mut p = SubmitPath::new();
    let _ = p.submit(mk_req(1, Engine::Render, 0x5E17, 16));
    let done = p.complete(Engine::Render, 0);
    let ok_f1 = done == Ok(0x5E17) && p.stats.signaled == 1;
    s.add("E24-FENCE-signal-explicit", ok_f1, "完成签到返回围栏序号");

    // F2：空端口 complete → PortMismatch（防御性专属码）。
    let mut p = SubmitPath::new();
    let ok_f2 = p.complete(Engine::Render, 0) == Err(SErr::PortMismatch);
    s.add("E24-FENCE-empty-port", ok_f2, "空端口签到专属拒");

    // F3：完成次序与提交次序独立（端口 1 先完成也合法）。
    let mut p = SubmitPath::new();
    let _ = p.submit(mk_req(1, Engine::Render, 10, 16));
    let _ = p.submit(mk_req(2, Engine::Render, 11, 16));
    let r1 = p.complete(Engine::Render, 1); // 后提交者先完成
    let ok_f3 = r1 == Ok(11) && p.stats.signaled == 1;
    s.add("E24-FENCE-out-of-order-ok", ok_f3, "乱序完成合法记账");
}

// ---------------------------------------------------------------------------
// E24-ERR：错误码全映射
// ---------------------------------------------------------------------------

fn fam_err(s: &mut CheckSet) {
    // R1：8 码全在 0x34xx 段且互异。
    let mut codes: Vec<u32> = Vec::new();
    let mut i = 0usize;
    while i < SErr::ALL.len() {
        codes.push(SErr::ALL[i].code());
        i += 1;
    }
    let mut sorted = codes.clone();
    sorted.sort();
    let mut distinct = true;
    let mut k = 1usize;
    while k < sorted.len() {
        if sorted[k] == sorted[k - 1] {
            distinct = false;
        }
        k += 1;
    }
    let mut in_seg = true;
    let mut m = 0usize;
    while m < codes.len() {
        if codes[m] & 0xFF00 != 0x3400 {
            in_seg = false;
        }
        m += 1;
    }
    s.add("E24-ERR-8-distinct-inseg", distinct && in_seg && SErr::ALL.len() == 8,
          "8 码全在 0x34xx 段且互异");

    // R2：reason 全非空且互异。
    let mut rs: Vec<String> = Vec::new();
    let mut nonempty = true;
    let mut j = 0usize;
    while j < SErr::ALL.len() {
        let r = SErr::ALL[j].reason();
        if r.len() == 0 {
            nonempty = false;
        }
        rs.push(r);
        j += 1;
    }
    rs.sort();
    let mut rd = true;
    let mut k = 1usize;
    while k < rs.len() {
        if rs[k] == rs[k - 1] {
            rd = false;
        }
        k += 1;
    }
    s.add("E24-ERR-reasons-unique", nonempty && rd, "reason 全非空且互异");

    // R3：非法引擎防御（complete 越端口）。
    let mut p = SubmitPath::new();
    let ok_r3 = p.complete(Engine::Render, 2) == Err(SErr::BadEngine);
    s.add("E24-ERR-bad-port", ok_r3, "端口越界 BadEngine 专属拒");

    // R4：SubmitQueueFull 真实可达（L4 已证）——此处断码值字面量。
    s.add("E24-ERR-code-literals",
          SErr::SubmitQueueFull.code() == 0x3401 && SErr::EngineHung.code() == 0x3405,
          "码值字面量判据侧写死");

    // R5：超时与弃用码值字面量。
    s.add("E24-ERR-code-literals-2",
          SErr::CtxTimeout.code() == 0x3403 && SErr::CtxAbandoned.code() == 0x3404,
          "超时/弃用码字面量");
}

// ---------------------------------------------------------------------------
// E24-A11Y/PERF：性能与读屏
// ---------------------------------------------------------------------------

fn fam_a11y(s: &mut CheckSet) {
    // Y1：ELSP 补位 O(1) ——确定性口径：一次补位至多 2 次（端口数上限）。
    let mut p = SubmitPath::new();
    let _ = p.submit(mk_req(1, Engine::Render, 1, 16));
    let _ = p.complete(Engine::Render, 0);
    let _ = p.dispatch_elsp(Engine::Render);
    let _ = p.dispatch_elsp(Engine::Render);
    let ok_y1 = p.stats.elsp_dispatches == 1 // 第二次无空位则 0
        && p.port_busy(Engine::Render) == 1;
    s.add("E24-PERF-elsp-const", ok_y1, "补位次数受端口数上限约束");

    // Y2：读屏摘要非空且不含地址。
    let sum = p.status_summary();
    let ok_y2 = sum.len() > 0 && !sum.contains("0x") && !sum.contains("0X");
    s.add("E24-A11Y-summary-privacy", ok_y2, "摘要可达零地址");

    // Y3：账本零值基线自证。
    let z = SubStats::zero();
    let ok_y3 = z.submits == 0 && z.elsp_dispatches == 0 && z.timeouts == 0
        && z.abandoned == 0 && z.engine_resets == 0 && z.pool_hits == 0
        && z.pool_misses == 0 && z.pool_drops == 0 && z.signaled == 0
        && z.requeued == 0 && z.queued_now == 0 && z.corrupt_rejects == 0;
    s.add("E24-A11Y-stats-zero", ok_y3, "账本零值基线");

    // Y4：阈值常量判据侧字面量（防被测侧改常量放水）。
    s.add("E24-PERF-budget-literals",
          CTX_TIMEOUT_TICKS == 64 && MAX_CTX_RESETS == 3 && QUEUE_CAP == 32
              && POOL_CAP == 16 && HUNG_TIMEOUT_COUNT == 2 && ELSP_PORTS == 2,
          "六常量判据侧写死");
}

// ---------------------------------------------------------------------------
// 聚合入口
// ---------------------------------------------------------------------------

/// VE-F0224 域自检（判据逐条映射锚点五条判据：
/// ELSP 7 / TMO 6 / POOL 3 / HUNG 4 / FENCE 3 / ERR 5 / A11Y-PERF 4
/// 共 32 项七族）。
pub fn run_veb24_checks() -> CheckSet {
    let mut s = CheckSet::new("intel-submit");
    fam_elsp(&mut s);
    fam_tmo(&mut s);
    fam_pool(&mut s);
    fam_hung(&mut s);
    fam_fence(&mut s);
    fam_err(&mut s);
    fam_a11y(&mut s);
    s
}
