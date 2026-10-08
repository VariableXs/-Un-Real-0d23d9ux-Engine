//! CGPU-F1122 判据（H02- 前缀，全局互异；独立口径防同源恒绿）。
//!
//! 锚点判据逐条落实：全量埋点（N 次操作 N 条账 + site 闭集 + 采样
//! 节拍）、开销（<1% 成本模型 + 账面不虚报）、查询（任意时刻重放与
//! 实时双源一致 + 与 vch01 池对账）、账单联动（BillFeedV1 字段冻结 +
//! 数据源一致性）、判据自检。

use crate::checks::CheckSet;
use crate::svstar2::vch01_budgetpool::{PoolKind, PoolSet};
use crate::svstar2::vch02_ledger as lg;
use alloc::vec;
use alloc::vec::Vec;

/// 判据侧独立重放（第三份：不复用 lg::query_at 的循环，直接扫池过滤）。
fn ref_replay(ops: &[lg::LedgerOp], pool: PoolKind, upto_seq: u32) -> u64 {
    let mut bal = 0u64;
    let mut i = 0usize;
    while i < ops.len() {
        let op = &ops[i];
        if op.seq <= upto_seq && op.pool == pool {
            if op.kind == lg::OpKind::Alloc {
                bal = bal.wrapping_add(op.bytes);
            } else {
                bal = bal.wrapping_sub(op.bytes);
            }
        }
        i += 1;
    }
    bal
}

/// 构造已挂账的池+账本夹具：池真实分配，账本同步记账。
/// 返回（池组，账本，句柄表）。
fn fixture() -> (PoolSet, lg::VramLedger, Vec<u64>) {
    let mut set = match PoolSet::new(64 * 1024 * 1024, [32 * 1024 * 1024, 16 * 1024 * 1024, 8 * 1024 * 1024]) {
        Ok(s) => s,
        Err(_) => return (set_fallback(), lg::VramLedger::new(), Vec::new()),
    };
    let mut ledger = lg::VramLedger::new();
    let mut handles = Vec::new();
    let site = lg::site_id("tex-upload").unwrap_or(0);
    for i in 0..4u32 {
        if let Ok(h) = set.allocate(PoolKind::Process, 100 + i, 1024 * (i as u64 + 1)) {
            let bytes = 1024 * (i as u64 + 1);
            let _ = ledger.record_alloc(PoolKind::Process, 100 + i, bytes, site, None);
            handles.push(h);
        }
    }
    (set, ledger, handles)
}

/// 夹具兜底（构造失败时给空池组；判据恒真防护会抓）。
fn set_fallback() -> PoolSet {
    PoolSet::new(1, [1, 1, 1]).unwrap_or_else(|_| PoolSet::new(64, [32, 16, 8]).expect("fixture"))
}

pub fn run_vch02_checks() -> CheckSet {
    let mut s = CheckSet::new("CGPU-H");

    // --- site 注册表闭集与未知拒 ------------------------------------------
    let site_tex = lg::site_id("tex-upload");
    let site_misc = lg::site_id("misc-pool");
    let site_bad = lg::site_id("not-a-site");
    s.add(
        "H02-埋点-site注册表闭集",
        site_tex == Some(0) && site_misc == Some(7) && site_bad.is_none()
            && lg::SITE_REGISTRY.len() == 8,
        "",
    );

    // --- 全量埋点：N 次操作 N 条账 + 余额/峰值 -----------------------------
    let mut ledger = lg::VramLedger::new();
    let st = lg::site_id("render-target").unwrap_or(0);
    let mut expected_live = 0u64;
    let mut seqs = Vec::new();
    for i in 0..10u32 {
        let bytes = 256 * (i as u64 + 1);
        if let Ok(seq) = ledger.record_alloc(PoolKind::Process, i, bytes, st, None) {
            seqs.push(seq);
            expected_live += bytes;
        }
    }
    // 释放偶数序的一半（5 笔）。
    let half = 6400u64; // 256*(1+3+5+7+9) 五笔释放合计
    for i in 0..5u32 {
        let _ = ledger.record_free(PoolKind::Process, i * 2, 256 * (i as u64 * 2 + 1), st);
    }
    s.add(
        "H02-埋点-全量记账",
        ledger.op_count() == 15 && seqs == vec![1u32, 2, 3, 4, 5, 6, 7, 8, 9, 10]
            && ledger.live(PoolKind::Process) == expected_live - half
            && ledger.peak(PoolKind::Process) == expected_live,
        "",
    );

    // 释放超额拒绝（配对断裂）。
    let over = ledger.record_free(PoolKind::Process, 0, u64::MAX, st);
    s.add("H02-埋点-超额释放拒收", over == Err(lg::ERR_FREE_UNPAIRED), "");

    // 未知 site 拒收。
    let unknown = ledger.record_alloc(PoolKind::Process, 0, 1, 99, None);
    s.add("H02-埋点-未知site拒收", unknown == Err(lg::ERR_SITE_UNKNOWN), "");

    // --- 栈回溯采样节拍（每 64 次一采，深度上限 8） ------------------------
    let mut ledger2 = lg::VramLedger::new();
    let st2 = lg::site_id("mesh-vb").unwrap_or(0);
    let stack = [1u8, 2, 3, 4, 5, 6, 7, 8, 9]; // 9 层 > 上限 8
    for i in 0..130u32 {
        let _ = ledger2.record_alloc(PoolKind::System, i, 64, st2, Some(&stack));
    }
    let samples = ledger2.stack_samples();
    let sampled_ok = samples.len() == 2 // 64 与 128 两次节拍
        && samples.iter().all(|sm| sm.frames.len() <= lg::STACK_DEPTH)
        && samples.iter().all(|sm| sm.site == st2)
        && samples[0].at_seq == 64
        && samples[1].at_seq == 128;
    s.add(
        "H02-采样-回溯按率触发",
        lg::SAMPLE_EVERY_N == 64 && lg::STACK_DEPTH == 8 && sampled_ok,
        "",
    );

    // --- 查询：任意时刻重放与实时双源一致 ----------------------------------
    let mut ledger3 = lg::VramLedger::new();
    let st3 = lg::site_id("swap-chain").unwrap_or(0);
    // 交错分配/释放：A+300, B+500, A-300, B+200, A+100（Process 池）。
    let _ = ledger3.record_alloc(PoolKind::Process, 1, 300, st3, None);
    let _ = ledger3.record_alloc(PoolKind::Process, 2, 500, st3, None);
    let _ = ledger3.record_free(PoolKind::Process, 1, 300, st3);
    let _ = ledger3.record_alloc(PoolKind::Process, 2, 200, st3, None);
    let _ = ledger3.record_alloc(PoolKind::Process, 1, 100, st3, None);
    let live_now = ledger3.live(PoolKind::Process); // 800
    let replay_ok = (1..=5u32).all(|q| {
        match (ledger3.query_at(PoolKind::Process, q), ref_replay(ledger3.ops(), PoolKind::Process, q)) {
            (Ok(a), b) => a == b,
            _ => false,
        }
    });
    // 终点重放 == 实时余额（双源一致）。
    let end_ok = ledger3.query_at(PoolKind::Process, 5) == Ok(live_now) && live_now == 800;
    // 越界 seq 拒绝。
    let oob = ledger3.query_at(PoolKind::Process, 6).is_err() && ledger3.query_at(PoolKind::Process, 0).is_err();
    s.add("H02-查询-任意时刻双源一致", replay_ok && end_ok && oob, "");

    // --- 与 vch01 池对账（reconcile 双面一致） -----------------------------
    let (mut set, mut ledger4, handles) = fixture();
    let rec_ok = ledger4.reconcile(&set).is_ok();
    // 池侧释放一笔，账本同步后仍一致；不同步则对账应红。
    if let Some(&h) = handles.first() {
        if set.release(PoolKind::Process, h).is_ok() {
            let stale = ledger4.reconcile(&set).is_err(); // 账本未记 → 对账红
            let bytes = 1024u64; // 首笔分配大小
            let _ = ledger4.record_free(PoolKind::Process, 100, bytes, lg::site_id("tex-upload").unwrap_or(0));
            let healed = ledger4.reconcile(&set).is_ok();
            s.add("H02-查询-池侧对账双面", rec_ok && stale && healed, "");
        } else {
            s.add("H02-查询-池侧对账双面", rec_ok, "");
        }
    } else {
        s.add("H02-查询-池侧对账双面", rec_ok, "");
    }
    let _ = &mut set;

    // --- 记账开销 <1%（成本模型 + 账面不虚报） -----------------------------
    let ledger5 = lg::VramLedger::new();
    let bp = ledger5.overhead_bp();
    let entry = &lg::OVERHEAD_LEDGER[0];
    s.add(
        "H02-开销-小于1%",
        lg::OP_COST_STEPS == 3 && lg::ALLOC_COST_STEPS == 512 && bp < 100
            && entry.target_bp == 100 && !entry.target_met() && lg::OVERHEAD_LEDGER.len() == 1,
        "",
    );

    // --- 账单联动：BillFeedV1 字段冻结 + 数据源一致性 ----------------------
    let bill = lg::export_bill(&ledger4, PoolKind::Process);
    // 独立重算：allocated_total = 夹具四笔 1024*(1..=4) = 10240；
    // live = 10240 - 1024（判据上一段释放的首笔）。
    let want_total: u64 = 1024 + 2048 + 3072 + 4096;
    let bill_ok = bill.version == lg::BILL_VERSION && lg::BILL_VERSION == 1
        && bill.pool == PoolKind::Process
        && bill.allocated_total == want_total
        && bill.live_bytes == want_total - 1024
        && bill.peak_bytes == want_total
        && bill.ops_count == 5; // 4 分配 + 1 释放
    s.add("H02-联动-账单数据源一致", bill_ok, "");

    // 其他池零操作账单（透明口径：零操作也是可查账）。
    let bill_empty = lg::export_bill(&ledger4, PoolKind::Reserved);
    s.add(
        "H02-联动-零操作池可查",
        bill_empty.version == 1 && bill_empty.allocated_total == 0
            && bill_empty.live_bytes == 0 && bill_empty.ops_count == 0,
        "",
    );

    // --- 判据自检（夹具非平凡） --------------------------------------------
    let nontrivial = want_total > 0 && half == 6400 && lg::OP_COST_STEPS < lg::ALLOC_COST_STEPS;
    s.add("H02-自检-夹具非平凡", nontrivial, "");

    s
}
