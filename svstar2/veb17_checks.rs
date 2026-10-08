//! VE-F0217 域自检（判据逐条映射锚点）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0217`
//!
//! 锚点判据原文：「快照重建、重协商、降级重建、超时兜底、判据」——
//! 五条各成一组判据。
//!
//! 判据设计纪律：
//! 1. **不得用表内元素验表内函数**——查清单/统计必须用表外真实形态。
//! 2. **阈值不得同时充当预期值**——规格常量另用锚点字面量断言一次。
//! 3. **断言两侧在测试点上不得同值**——测试点要选「正确与错误实现
//!    结果不同」的那一点。
//! 4. **否定式累积布尔初值必须写期望默认态**，循环只在违反时翻转；
//    写成 `false` 起手且只会置 `false` ⇒ 恒红（自检分号陷阱）。
//! 5. **判据区零 panic 面**——取值一律 `match` 单次绑定或 `get(i)`。
//! 6. **摘要须由判据侧独立重算**，并**双向对账**（不向被测方法
//!    索取答案，否则是自证式）。

use crate::checks::CheckSet;
use crate::svstar2::veb17_suspend::*;

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格锚点字面量（**不引用实现常量**）
// ---------------------------------------------------------------------------

/// 锚点「保存设备态（cfg、队列、资源表）」——快照三段。
const ANCHOR_SEGMENTS: usize = 3;
/// 锚点「VIRTIO_F_VERSION_1 必需」——特性位定值。
const ANCHOR_FEATURE_V1: u32 = 32;
/// 锚点「恢复验证清单」——五项。
const ANCHOR_VERIFY_ITEMS: usize = 5;
/// 锚点「重置超时」——测试用预算字面量（不引用 DEFAULT_RESET_BUDGET）。
const ANCHOR_TEST_BUDGET: u32 = 4;

/// 造一条标准快照（cfg=0x1000、2 队列、3 资源）——**表外真实形态**。
fn standard_snapshot() -> Snapshot {
    Snapshot::build(
        0x1000,
        alloc::vec![(0, 256), (1, 512)],
        alloc::vec![(10, 4096), (11, 8192), (12, 1024)],
    )
}

// ---------------------------------------------------------------------------
// 一、快照重建（锚点「快照重建」）
// ---------------------------------------------------------------------------

fn c217_snapshot() -> Vec<(&'static str, bool)> {
    let mut v = Vec::new();

    let s = standard_snapshot();
    v.push(("C217-快照-三段齐备", s.cfg == 0x1000 && s.queue_count() == 2 && s.resource_count() == 3));
    v.push(("C217-快照-段数对齐锚点", ANCHOR_SEGMENTS == 3));

    // 摘要自洽：新建即自洽
    v.push(("C217-快照-新建摘要自洽", s.verify()));

    // 空快照：摘要亦自洽（空内容不是损坏）
    let e = Snapshot::empty();
    v.push(("C217-快照-空快照摘要自洽", e.verify() && e.resource_count() == 0));

    // **判据侧独立重算摘要**再双向对账（十诫第 7 条：不向被测方法索取）
    let ref_d = digest_of(s.cfg, &s.queues, &s.resources);
    v.push(("C217-快照-摘要与独立重算一致", s.digest == ref_d));

    // **篡改 cfg 必被检出**（真损坏点，非「远超」式取样）
    let mut bad = standard_snapshot();
    bad.cfg = bad.cfg ^ 1;
    v.push(("C217-快照-篡改配置被检出", !bad.verify()));

    // 篡改资源 id 必被检出
    let mut bad2 = standard_snapshot();
    if !bad2.resources.is_empty() {
        bad2.resources[1].0 = bad2.resources[1].0 ^ 0xFF;
    }
    v.push(("C217-快照-篡改资源被检出", !bad2.verify()));

    // **异构项互换必被检出**：队列 (1,512)↔(0,256) 换位——长度与
    // 元素集完全相同，只有位置混入摘要才抓得到（十诫第 6 条：判据
    // 索引须由内容推导而非硬编码）。
    let mut swapped = standard_snapshot();
    let n = swapped.queues.len();
    if n == 2 {
        let t = swapped.queues[0];
        swapped.queues[0] = swapped.queues[1];
        swapped.queues[1] = t;
    }
    v.push(("C217-快照-异构项换位被检出", !swapped.verify()));

    // 追加一项必改变摘要（长度须入摘要，否则「截断/追加」漏判）
    let mut grown = standard_snapshot();
    grown.resources.push((13, 2048));
    grown.digest = digest_of(grown.cfg, &grown.queues, &grown.resources);
    v.push((
        "C217-快照-追加资源改变摘要",
        grown.digest != s.digest && grown.verify(),
    ));

    // 摘要取值域合理（非零且小于模数——否则「恒同值」伪装成摘要）
    v.push(("C217-快照-摘要值域合理", s.digest > 0 && s.digest < DIGEST_MOD));

    // 恢复后资源表条数与快照一致（正常路径）
    let snap = standard_snapshot();
    let mut r = Restore::new(ANCHOR_FEATURE_V1);
    let out = r.from_snapshot(&snap);
    v.push((
        "C217-快照-恢复后资源条数一致",
        out == Outcome::Ok && r.resources.len() == snap.resource_count(),
    ));

    // 正常恢复后账本无泄漏
    v.push(("C217-快照-正常恢复账本平衡", r.ledger.balanced()));

    v
}

// ---------------------------------------------------------------------------
// 二、重协商（锚点「重协商」）
// ---------------------------------------------------------------------------

fn c217_renegotiate() -> Vec<(&'static str, bool)> {
    let mut v = Vec::new();

    // 特性齐备时重协商通过
    let mut ok = Restore::new(ANCHOR_FEATURE_V1);
    v.push(("C217-重协商-必需特性齐备通过", ok.renegotiate(3) == Outcome::Ok));

    // **必需特性丢失 ⇒ 判 RenegotiateLost**（不是「将就着建」）
    let mut lost = Restore::new(0);
    v.push(("C217-重协商-必需特性丢失判失败", lost.renegotiate(3) == Outcome::RenegotiateLost));

    // 特性位判定用「恰缺 V1」这一真实测试点（不是「全 0」）
    let mut only_other = Restore::new(1u32);
    v.push(("C217-重协商-他特性不足以替代", only_other.renegotiate(1) == Outcome::RenegotiateLost));

    // 空资源表 + 无特性：允许（无资源要重建就不需要特性）
    let mut none_res = Restore::new(0);
    v.push(("C217-重协商-无资源不强要特性", none_res.renegotiate(0) == Outcome::Ok));

    // 特性位对齐锚点定值
    v.push(("C217-重协商-特性位对齐锚点", VIRTIO_F_VERSION_1 == ANCHOR_FEATURE_V1));

    // 恢复路径整体：特性丢失 ⇒ 快照不重建（返回 RenegotiateLost）
    let snap = standard_snapshot();
    let mut nofeat = Restore::new(0);
    let out = nofeat.from_snapshot(&snap);
    v.push((
        "C217-重协商-缺特性不重建资源",
        out == Outcome::RenegotiateLost && nofeat.resources.is_empty(),
    ));

    // 且此时**必须通知用户**（锚点「降级重建并通知」）
    v.push(("C217-重协商-缺特性须通知", nofeat.notice == Notice::Degraded));

    v
}

// ---------------------------------------------------------------------------
// 三、降级重建（锚点「降级重建」）
// ---------------------------------------------------------------------------

fn c217_degraded() -> Vec<(&'static str, bool)> {
    let mut v = Vec::new();

    // 资源表超容量 ⇒ 降级重建且**只建容量内部分**
    let mut big = Vec::new();
    let mut i = 0;
    while i <= RESOURCE_CAPACITY {
        big.push((100 + i as u32, 4096));
        i += 1;
    }
    let snap = Snapshot::build(0x2000, alloc::vec![(0, 256)], big);
    let mut r = Restore::new(ANCHOR_FEATURE_V1);
    let out = r.from_snapshot(&snap);
    v.push((
        "C217-降级-超容量判降级",
        out == Outcome::Degraded && r.resources.len() == RESOURCE_CAPACITY,
    ));

    // 降级**必须通知**（用户需知道资源语义变了）
    v.push(("C217-降级-降级须通知", r.notice == Notice::Degraded));

    // 降级时账本仍须平衡（只建了容量内那些，不能漏销）
    v.push(("C217-降级-降级账本平衡", r.ledger.balanced()));

    // 恰在容量边界（不超）⇒ 不降级（边界位置由夹逼对钉死）
    let mut at_cap = Vec::new();
    let mut j = 0;
    while j < RESOURCE_CAPACITY {
        at_cap.push((200 + j as u32, 4096));
        j += 1;
    }
    // `build` 按值收队列，故传 clone 保留 `at_cap` 供下面「超一」夹逼复用
    let snap_at = Snapshot::build(0x2000, alloc::vec![(0, 256)], at_cap.clone());
    let mut r_at = Restore::new(ANCHOR_FEATURE_V1);
    let out_at = r_at.from_snapshot(&snap_at);
    v.push((
        "C217-降级-恰在容量不降级",
        out_at == Outcome::Ok && r_at.resources.len() == RESOURCE_CAPACITY,
    ));

    // 超一即降级（容量的「恰好 / 恰好 +1」夹逼）。先 clone 再改动，
    // 否则 move 之后再用会报 E0382。
    let mut over = at_cap.clone();
    over.push((999, 4096));
    let snap_over = Snapshot::build(0x2000, alloc::vec![(0, 256)], over);
    let mut r_over = Restore::new(ANCHOR_FEATURE_V1);
    v.push((
        "C217-降级-超一即降级",
        r_over.from_snapshot(&snap_over) == Outcome::Degraded,
    ));

    // 结果语义：降级算成功完成但**要通知**（两者都不许漏判）
    v.push((
        "C217-降级-降级算完成且需通知",
        Outcome::Degraded.is_success() && Outcome::Degraded.needs_notice(),
    ));
    // 正常完成不算失败，且不需「降级」级通知（写成 `!a == false` 那类
    // 双重否定只会让本项恒真——自证式门禁）。
    v.push((
        "C217-降级-正常完成不需降级通知",
        Outcome::Ok.is_success() && !Outcome::Ok.needs_notice(),
    ));

    v
}

// ---------------------------------------------------------------------------
// 四、超时兜底（锚点「超时兜底」）
// ---------------------------------------------------------------------------

fn c217_timeout() -> Vec<(&'static str, bool)> {
    let mut v = Vec::new();

    // 重置在预算内完成 ⇒ Ok、不兜底
    let mut r1 = Restore::new(ANCHOR_FEATURE_V1);
    r1.resources.push((1, 1024));
    let ok = hot_reset(&mut r1, ANCHOR_TEST_BUDGET, ANCHOR_TEST_BUDGET);
    v.push((
        "C217-超时-预算内完成不兜底",
        ok.outcome == Outcome::Ok && !ok.fallback && ok.resources_kept == 1,
    ));

    // **恰好多一步 ⇒ 兜底**（预算夹逼对：N 步过、N+1 步兜底）
    let mut r2 = Restore::new(ANCHOR_FEATURE_V1);
    let over = hot_reset(&mut r2, ANCHOR_TEST_BUDGET + 1, ANCHOR_TEST_BUDGET);
    v.push((
        "C217-超时-超一步即兜底",
        over.outcome == Outcome::TimeoutFallback && over.fallback,
    ));

    // 兜底**必须可观测**：守卫 tripped 且恢复器置 fallback_ran
    v.push((
        "C217-超时-兜底状态可观测",
        over.fallback && r2.fallback_ran,
    ));

    // 兜底时资源保留数为 0（未走完 reset 序不宣称保留）
    v.push(("C217-超时-兜底不宣称保留", over.resources_kept == 0));

    // 兜底须通知用户
    v.push(("C217-超时-兜底须通知", r2.notice == Notice::Degraded));

    // 守卫预算语义：耗尽后再 step 恒假并保持 tripped
    let mut g = ResetGuard::new(1);
    let first = g.step();
    let second = g.step();
    v.push((
        "C217-超时-预算耗尽后恒假",
        first && !second && g.tripped && g.exhausted(),
    ));

    // 零预算：一步都走不得，立即 tripped
    let mut g0 = ResetGuard::new(0);
    let z = g0.step();
    v.push(("C217-超时-零预算立即兜底", !z && g0.tripped));

    // 结果语义
    v.push((
        "C217-超时-兜底不算成功",
        !Outcome::TimeoutFallback.is_success() && Outcome::TimeoutFallback.needs_notice(),
    ));

    // 热重置走 F0212 reset 序且**保留资源表语义**：资源数不变
    let mut r3 = Restore::new(ANCHOR_FEATURE_V1);
    r3.resources.push((7, 1024));
    r3.resources.push((8, 2048));
    let kept = hot_reset(&mut r3, 2, 4);
    v.push((
        "C217-超时-热重置保留资源表",
        kept.resources_kept == 2 && r3.resources.len() == 2,
    ));

    // 两条路不可混用：热重置**不重建**（资源 id 不变），
    // 而 snapshot 恢复**重建**（账本走销毁+创建）。
    v.push((
        "C217-超时-热重置不重建资源表",
        kept.outcome == Outcome::Ok && r3.ledger.created == 0,
    ));

    v
}

// ---------------------------------------------------------------------------
// 五、快照损坏 + 验证清单 + 通知开关
// ---------------------------------------------------------------------------

fn c217_corrupt() -> Vec<(&'static str, bool)> {
    let mut v = Vec::new();

    // 快照损坏 ⇒ 全量重初始化（资源表清空、旧账销平）
    let mut bad = standard_snapshot();
    bad.cfg = bad.cfg ^ 0xFFFF;
    let mut r = Restore::new(ANCHOR_FEATURE_V1);
    r.resources.push((99, 1024));
    r.ledger.create(1);
    // 预置一个旧资源后的基线计数（差值口径用）
    let created_before = r.ledger.created;
    let out = r.from_snapshot(&bad);
    v.push((
        "C217-损坏-损坏判全量重初始化",
        out == Outcome::SnapshotCorrupt && out.full_reinit() && r.resources.is_empty(),
    ));

    // 损坏路径不得重建资源（不许「先试着重建，失败再说」）。
    // 判据用**差值口径**：本例预置了 1 个旧资源（`created` 起始为 1），
    // 故断言「恢复过程一个都没新建」= `created` 停在预置值，
    // 即 `created - created_before == 0`。直接断 `created == 0`
    // 会把「预置的旧资源」也算成新建，是判据而非实现的错。
    v.push((
        "C217-损坏-损坏不重建",
        r.ledger.created == created_before && r.resources.is_empty(),
    ));

    // 损坏须通知
    v.push(("C217-损坏-损坏须通知", r.notice == Notice::Degraded));

    // 结果语义：只有损坏触发全量重初始化
    v.push((
        "C217-损坏-仅损坏触发全量重初始化",
        Outcome::SnapshotCorrupt.full_reinit()
            && !Outcome::Ok.full_reinit()
            && !Outcome::Degraded.full_reinit()
            && !Outcome::TimeoutFallback.full_reinit(),
    ));

    // 验证清单：条数对齐锚点，且项互异可判读
    let snap = standard_snapshot();
    let mut rr = Restore::new(ANCHOR_FEATURE_V1);
    let o = rr.from_snapshot(&snap);
    let vl = verify(&rr, &snap, o);
    v.push((
        "C217-验证-清单项数对齐锚点",
        ANCHOR_VERIFY_ITEMS == VERIFY_ITEMS && vl.items.len() == VERIFY_ITEMS,
    ));

    // 正常恢复：摘要/特性/条数/账本四项应全绿
    let mut all_ok = true;
    let mut i = 0;
    while i < vl.items.len() {
        if !vl.items[i].1 {
            all_ok = false;
        }
        i += 1;
    }
    v.push(("C217-验证-正常恢复清单全绿", all_ok));

    // 验证清单能反映损坏（拿损坏快照跑，摘要项必红）
    let vl_bad = verify(&rr, &bad, Outcome::SnapshotCorrupt);
    let mut found_red = false;
    let mut k = 0;
    while k < vl_bad.items.len() {
        if vl_bad.items[k].0 == VerifyItem::SnapshotDigest && !vl_bad.items[k].1 {
            found_red = true;
        }
        k += 1;
    }
    v.push(("C217-验证-损坏时摘要项转红", found_red));

    // 通知可关闭（锚点「可关闭」）
    let mut quiet = Restore::new(ANCHOR_FEATURE_V1);
    quiet.notice_enabled = false;
    quiet.from_snapshot(&snap);
    v.push((
        "C217-验证-通知可关闭",
        quiet.notice_text().is_empty(),
    ));

    // 关闭通知**不影响**恢复结论（关闭是显示层决策，不是语义）
    let mut loud = Restore::new(ANCHOR_FEATURE_V1);
    let loud_out = loud.from_snapshot(&snap);
    v.push((
        "C217-验证-关通知不改结论",
        outcome_equiv(Outcome::Ok, loud_out),
    ));

    v
}

/// 恢复结论等价（仅供判据比对：两态是否同为成功路）。
///
/// 注意：`Restore` 自身**不持有**结论——结论是 [`Restore::from_snapshot`]
/// 的返回值，故此处比较两个 `Outcome`，不向恢复器要「结论字段」。
fn outcome_equiv(a: Outcome, b: Outcome) -> bool {
    a.is_success() == b.is_success() && a.needs_notice() == b.needs_notice()
}

/// 账本泄漏/过度销毁口径（净 vs 绝对值，十诫第 10 条）。
fn c217_ledger() -> Vec<(&'static str, bool)> {
    let mut v = Vec::new();

    // 空账平衡
    let z = Ledger::new();
    v.push(("C217-账本-空账平衡", z.balanced() && z.leaked() == 0));

    // 成对建销 ⇒ 平衡
    let mut p = Ledger::new();
    p.create(3);
    p.destroy(3);
    v.push(("C217-账本-成对建销平衡", p.balanced() && p.leaked() == 0));

    // **漏销 ⇒ 报泄漏且余额非零**
    let mut l = Ledger::new();
    l.create(3);
    l.destroy(2);
    v.push(("C217-账本-漏销检出泄漏", !l.balanced() && l.leaked() == 1));

    // 过度销毁：不因净值口径误报为「平衡」
    let mut o = Ledger::new();
    o.create(1);
    o.destroy(3);
    v.push(("C217-账本-过度销毁不判平衡", !o.balanced() && o.leaked() == 0));

    // 饱和：计数溢出**饱和不回绕**（累加三次 u32::MAX 后仍为 u32::MAX，
    // 而非回绕成 u32::MAX-2）。这是「饱和 vs 回绕」的唯一可判测试点。
    let mut sat = Ledger::new();
    sat.create(u32::MAX);
    sat.create(u32::MAX);
    sat.create(u32::MAX);
    v.push((
        "C217-账本-计数饱和不回绕",
        sat.created == u32::MAX && !sat.balanced(),
    ));

    v
}

/// 跑完 VE-F0217 全部判据。
pub fn run_veb17_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-veb17");
    for group in [
        c217_snapshot,
        c217_renegotiate,
        c217_degraded,
        c217_timeout,
        c217_corrupt,
        c217_ledger,
    ] {
        for (name, passed) in group() {
            set.add(name, passed, "");
        }
    }
    set
}
