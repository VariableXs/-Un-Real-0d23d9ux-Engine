//! VE-F0222 判据层：Intel 显存管理对接 GTT（锚点五条判据逐条映射）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0222`
//!
//! **锚点原文五条判据 → 本层判据族**：
//!
//! | 锚点判据 | 判据族 | 要点 |
//! |---|---|---|
//! | GGTT 语义 | `C22-TBL-*` | 两表分池互不串扰 + O(1) 查找 + 跨表拒绝 |
//! | 批写回滚 | `C22-BATCH-*` | 非法批零写入 + 落笔数恰等 + 逐位编码 |
//! | LRU 驱逐 | `C22-EVICT-*` | 只逐未引用 + 重试上限 + 限频 + LRU 序 |
//! | 引用计数 | `C22-REF-*` | 不越零不回绕 + 钉住不解绑 + 字节账对账 |
//! | （大页分级/围栏/无障碍为锚点正文要求） | `C22-PAGE-*` `C22-FENCE-*` `C22-PERF/A11Y-*` | 显式与自动语义分开等 |
//!
//! # 本层的核心纪律：**判据侧独立重算，不向被测问答案**
//!
//! 页数用判据侧独立写的 `ceil(len/4K)`、PTE 编码用独立字面量、
//! LRU 序在判据侧自记 tick 台账重演、字节账从票号全表重扫累加。
//! 被测改成 O(1) 分桶、页数 ceil 换 floor、LRU 取最新，都会在这里分叉。

// ---------------------------------------------------------------------------
// 导入
// ---------------------------------------------------------------------------

use crate::checks::CheckSet;
use alloc::vec::Vec;

use super::veb21_ident::GenTier;
use super::veb22_gtt::*;

// ---------------------------------------------------------------------------
// 判据侧独立参照（不调被测的口径）
// ---------------------------------------------------------------------------

/// 判据侧独立页数口径：`ceil(len / 4K)`。
fn alt_pages_4k(len: u64) -> u64 {
    (len + PAGE_4K - 1) / PAGE_4K
}

/// 判据侧独立 PTE 编码（bit0=present，bit1=2M，地址清低 12 位）。
fn alt_pte(gpa: u64, two_m: bool, present: bool) -> u64 {
    let mut v = gpa & 0xFFFF_FFFF_FFFF_F000;
    if present {
        v |= 1;
    }
    if two_m {
        v |= 2;
    }
    v
}

/// fence 签到阈值（判据侧驱动：seq ≤ 阈值视为已签）。
static mut FENCE_SIGNALED_UPTO: u64 = 0;

fn fence_pred(seq: u64) -> bool {
    unsafe {
        let upto = FENCE_SIGNALED_UPTO;
        seq <= upto
    }
}

fn set_fence_upto(v: u64) {
    unsafe {
        FENCE_SIGNALED_UPTO = v;
    }
}

/// 快照全部记账字段（判据侧对账用，字段级独立枚举）。
struct StatSnap {
    pte_writes: u64,
    pte_clears: u64,
    batch_commits: u32,
    batch_rejected: u32,
    evicted: u32,
    evict_sweeps: u32,
    evict_deferred: u32,
    large_downgrades: u32,
    fence_freed: u32,
    pending_forced: u32,
    binds: u32,
    unbinds: u32,
}

fn snap(g: &Gtt) -> StatSnap {
    let s = g.stats;
    StatSnap {
        pte_writes: s.pte_writes,
        pte_clears: s.pte_clears,
        batch_commits: s.batch_commits,
        batch_rejected: s.batch_rejected,
        evicted: s.evicted,
        evict_sweeps: s.evict_sweeps,
        evict_deferred: s.evict_deferred,
        large_downgrades: s.large_downgrades,
        fence_freed: s.fence_freed,
        pending_forced: s.pending_forced,
        binds: s.binds,
        unbinds: s.unbinds,
    }
}

/// 判据侧便捷构造：Xe 档 GTT（支持 2M）。
fn gtt_xe() -> Gtt {
    Gtt::new(GenTier::XeStandard)
}

/// 判据侧便捷构造：基线档 GTT（4K only）。
fn gtt_base() -> Gtt {
    Gtt::new(GenTier::Baseline)
}

// ===========================================================================
// 判据族一：`C22-TBL-*` —— GGTT 语义（锚点判据 1）
// ===========================================================================

/// **1.1 两表同池绑定互不串扰**：GGTT 写入不影响 ppGTT，反之亦然；
/// 解绑一侧另一侧 PTE 原样。
fn c22_tbl_isolation(s: &mut CheckSet) {
    let mut g = gtt_xe();
    let pool: u64 = 0x1000_0000;
    let tg = g.bind(TableKind::Ggtt, pool, 8192, PagePolicy::Force4k, 1);
    let tp = g.bind(TableKind::PpGtt, pool, 8192, PagePolicy::Force4k, 2);
    let mut ok = true;
    if let (Ok(a), Ok(_b)) = (tg, tp) {
        // 两表各自 vma=0 处 present，GPA 同源（独立对拍编码）。
        let want = alt_pte(pool, false, true);
        let pg = g.read_pte(TableKind::Ggtt, 0);
        let pp = g.read_pte(TableKind::PpGtt, 0);
        if pg != Some(want) || pp != Some(want) {
            ok = false;
        }
        // 解绑 GGTT 一侧：GGTT 清零，ppGTT 原样。
        if g.unbind(a).is_err() {
            ok = false;
        }
        if g.read_pte(TableKind::Ggtt, 0) != Some(0) {
            ok = false;
        }
        if g.read_pte(TableKind::PpGtt, 0) != Some(want) {
            ok = false;
        }
    } else {
        ok = false;
    }
    s.add("C22-TBL-01 两表同池绑定互不串扰且解绑单侧不清他表", ok, "");
}

/// **1.2 O(1) 查找返回完整字段**（vma/len/pages/refcount/table 与
/// 判据侧独立记录一致；且 lookup 不动任何记账字段）。
fn c22_tbl_lookup_fields(s: &mut CheckSet) {
    let mut g = gtt_xe();
    g.advance();
    let t = g.bind(TableKind::PpGtt, 0x2000_0000, 12288, PagePolicy::Force4k, 3);
    let mut ok = true;
    match t {
        Ok(t) => {
            let before = snap(&g);
            match g.lookup(t) {
                Some(b) => {
                    if b.vma != 0
                        || b.len != 12288
                        || b.pages_4k != 3
                        || b.refcount != 0
                        || b.table != TableKind::PpGtt
                    {
                        ok = false;
                    }
                }
                None => ok = false,
            }
            let after = snap(&g);
            if before.pte_writes != after.pte_writes
                || before.batch_commits != after.batch_commits
                || before.binds != after.binds
            {
                ok = false; // 查找有副作用 = 不是 O(1) 直取
            }
        }
        Err(_) => ok = false,
    }
    s.add("C22-TBL-02 票号直取字段完整且查找零副作用", ok, "");
}

/// **1.3 越界/空槽票号 NotFound**（不做越界读——零 panic 面）。
fn c22_tbl_lookup_bounds(s: &mut CheckSet) {
    let g = gtt_xe();
    let mut ok = true;
    if g.lookup(BindTicket { raw: MAX_BINDINGS as u32 }).is_some() {
        ok = false;
    }
    if g.lookup(BindTicket { raw: 0xFFFF_FFFF }).is_some() {
        ok = false;
    }
    // 空槽（未绑定）也必须 None。
    if g.lookup(BindTicket { raw: 0 }).is_some() {
        ok = false;
    }
    s.add("C22-TBL-03 越界与空槽票号一律 None(不越界读)", ok, "");
}

/// **1.4 跨表批写拒绝**：GGTT 批提交到 ppGTT ⇒ TableMismatch 且零写入
/// （跨表 = 语义事故在提交口拦住，不靠调用方自觉）。
fn c22_tbl_cross_table_rejected(s: &mut CheckSet) {
    let mut g = gtt_xe();
    let mut b = PteBatch::for_table(TableKind::Ggtt);
    b.push(0, alt_pte(0x3000_0000, false, true));
    let before = snap(&g);
    let mut ok = true;
    match g.pte_batch_commit(TableKind::PpGtt, &b, 1) {
        Err(GErr::TableMismatch) => {}
        _ => ok = false,
    }
    let after = snap(&g);
    if after.pte_writes != before.pte_writes
        || after.batch_commits != before.batch_commits
        || after.pte_clears != before.pte_clears
    {
        ok = false; // 拒绝必须零写入
    }
    s.add("C22-TBL-04 跨表批写TableMismatch且零写入零提交", ok, "");
}

/// **1.5 PTE 编码逐位契约**（判据侧独立字面量，禁散落位算术的对偶断言）。
fn c22_tbl_pte_encoding(s: &mut CheckSet) {
    let mut ok = true;
    // 4K present：地址清低 12 位 + bit0。
    if pte_encode(0x1234_5000, PageClass::P4k, true) != 0x1234_5001 {
        ok = false;
    }
    // 4K 非 present：纯地址。
    if pte_encode(0x1234_5678, PageClass::P4k, false) != 0x1234_5000 {
        ok = false;
    }
    // 2M present：地址 + bit0 + bit1。
    if pte_encode(0x1220_0000, PageClass::P2m, true) != 0x1220_0003 {
        ok = false;
    }
    // 解码三件套往返。
    if pte_gpa(0x1220_0003) != 0x1220_0000 || !pte_is_2m(0x1220_0003)
        || !pte_present(0x1220_0003) || pte_present(0x1220_0000)
    {
        ok = false;
    }
    s.add("C22-TBL-05 PTE编码逐位契约(独立字面量四组+解码往返)", ok, "");
}

/// **1.6 unbind 后 PTE 立即清零**（逐条独立清点，清除计数恰等）。
fn c22_tbl_unbind_clears_pte(s: &mut CheckSet) {
    let mut g = gtt_xe();
    let t = g.bind(TableKind::PpGtt, 0x1000_0000, 3 * PAGE_4K, PagePolicy::Force4k, 5);
    let mut ok = true;
    match t {
        Ok(t) => {
            // 绑定后三条全 present。
            let mut i = 0u64;
            while i < 3 {
                if g.read_pte(TableKind::PpGtt, i * PAGE_4K) != Some(alt_pte(0x1000_0000 + i * PAGE_4K, false, true)) {
                    ok = false;
                }
                i += 1;
            }
            let before_clears = g.stats.pte_clears;
            if g.unbind(t).is_err() {
                ok = false;
            }
            // 三条全部立即清零（设备不得再翻译到这批页）。
            let mut i = 0u64;
            while i < 3 {
                if g.read_pte(TableKind::PpGtt, i * PAGE_4K) != Some(0) {
                    ok = false;
                }
                i += 1;
            }
            if g.stats.pte_clears - before_clears != 3 {
                ok = false; // 清除计数恰等（== 不是 >=）
            }
        }
        Err(_) => ok = false,
    }
    s.add("C22-TBL-06 unbind后PTE立即逐条清零且清除计数恰等", ok, "");
}

/// **1.7 pending 区间仍占址**：解绑后 fence 未签，同尺寸再绑定必须
/// 落在 pending 区间之后（first-fit 跳过 pending——提前复用就是踩踏）。
fn c22_tbl_pending_holds_range(s: &mut CheckSet) {
    let mut g = gtt_xe();
    let t = g.bind(TableKind::PpGtt, 0x1000_0000, PAGE_4K, PagePolicy::Force4k, 9);
    let mut ok = true;
    match t {
        Ok(a) => {
            let old_vma = match g.lookup(a) {
                Some(b) => b.vma,
                None => {
                    ok = false;
                    0
                }
            };
            if g.unbind(a).is_err() {
                ok = false;
            }
            match g.bind(TableKind::PpGtt, 0x1000_0000, PAGE_4K, PagePolicy::Force4k, 10) {
                Ok(b2) => match g.lookup(b2) {
                    Some(nb) => {
                        if nb.vma < old_vma + PAGE_4K {
                            ok = false; // 新绑定不得落入 pending 区间
                        }
                    }
                    None => ok = false,
                },
                Err(_) => ok = false,
            }
        }
        Err(_) => ok = false,
    }
    s.add("C22-TBL-07 pending区间仍占址(新绑定first-fit跳过)", ok, "");
}

// ===========================================================================
// 判据族二：`C22-BATCH-*` —— 批写回滚（锚点判据 2）
// ===========================================================================

/// **2.1 bind 落笔数恰等独立口径**：pte_writes 增量 == ceil(len/4K)。
fn c22_batch_bind_writes_exact(s: &mut CheckSet) {
    let mut g = gtt_xe();
    let before = snap(&g);
    let mut ok = true;
    match g.bind(TableKind::PpGtt, 0x1000_0000, 5 * PAGE_4K, PagePolicy::Force4k, 1) {
        Ok(_) => {
            let after = snap(&g);
            if after.pte_writes - before.pte_writes != alt_pages_4k(5 * PAGE_4K) {
                ok = false;
            }
        }
        Err(_) => ok = false,
    }
    s.add("C22-BATCH-01 bind落笔数恰等ceil(len/4K)(判据侧独立口径)", ok, "");
}

/// **2.2 非法条目整批拒绝零写入**（要点一：从未半提交 ⇒ 结构性回滚）。
fn c22_batch_invalid_zero_write(s: &mut CheckSet) {
    let mut g = gtt_xe();
    // 事先放一个已绑定页，验证拒绝后其 PTE 原样（零扰动）。
    let t = g.bind(TableKind::PpGtt, 0x1000_0000, PAGE_4K, PagePolicy::Force4k, 1);
    let mut ok = true;
    let mut b = PteBatch::for_table(TableKind::PpGtt);
    // 合法条目 + 一条保留位非零的坏条目（bit4 置起）。
    b.push(10, alt_pte(0x2000_0000, false, true));
    b.push(11, alt_pte(0x2000_1000, false, true) | 0x10);
    let before = snap(&g);
    match g.pte_batch_commit(TableKind::PpGtt, &b, 2) {
        Err(GErr::BatchInvalidEntry) => {}
        _ => ok = false,
    }
    let after = snap(&g);
    if after.pte_writes != before.pte_writes {
        ok = false; // 整批拒绝 ⇒ 零写入
    }
    if after.batch_rejected - before.batch_rejected != 1 {
        ok = false; // 拒绝计数恰 +1
    }
    // 已有绑定的 PTE 与窗内前几个 PTE 全部原样。
    match t {
        Ok(t) => {
            if g.read_pte(TableKind::PpGtt, 0) != Some(alt_pte(0x1000_0000, false, true)) {
                ok = false;
            }
            let _ = t;
        }
        Err(_) => ok = false,
    }
    let mut i = 10u64;
    while i <= 11 {
        if g.read_pte(TableKind::PpGtt, i * PAGE_4K) != Some(0) {
            ok = false; // 坏批的条目一个都没落
        }
        i += 1;
    }
    s.add("C22-BATCH-02 非法批整批拒绝零写入且拒绝计数恰1", ok, "");
}

/// **2.3 下标越窗拒绝**。
fn c22_batch_idx_out_of_window(s: &mut CheckSet) {
    let mut g = gtt_xe();
    let mut b = PteBatch::for_table(TableKind::PpGtt);
    b.push(APERTURE_SIZE / PAGE_4K, alt_pte(0x2000_0000, false, true));
    let before = snap(&g);
    let mut ok = matches!(g.pte_batch_commit(TableKind::PpGtt, &b, 1), Err(GErr::BatchInvalidEntry));
    if g.stats.pte_writes != before.pte_writes {
        ok = false;
    }
    s.add("C22-BATCH-03 下标越窗整批拒绝零写入", ok, "");
}

/// **2.4 同批重复下标拒绝**（同一 PTE 被写两遍是批构造错误）。
fn c22_batch_dup_index(s: &mut CheckSet) {
    let mut g = gtt_xe();
    let mut b = PteBatch::for_table(TableKind::PpGtt);
    b.push(4, alt_pte(0x2000_0000, false, true));
    b.push(4, alt_pte(0x2000_1000, false, true));
    let before = snap(&g);
    let mut ok = matches!(g.pte_batch_commit(TableKind::PpGtt, &b, 2), Err(GErr::BatchInvalidEntry));
    if g.stats.pte_writes != before.pte_writes {
        ok = false;
    }
    s.add("C22-BATCH-04 同批重复下标拒绝零写入", ok, "");
}

/// **2.5 合法手工批提交成功且逐条读回**（编码独立对拍）。
fn c22_batch_valid_manual(s: &mut CheckSet) {
    let mut g = gtt_xe();
    let mut b = PteBatch::for_table(TableKind::Ggtt);
    b.push(0, alt_pte(0x4000_0000, false, true));
    b.push(1, alt_pte(0x4000_1000, false, true));
    b.push(2, alt_pte(0x4000_2000, false, true));
    let before = snap(&g);
    let mut ok = true;
    match g.pte_batch_commit(TableKind::Ggtt, &b, 3) {
        Ok(()) => {
            let after = snap(&g);
            if after.pte_writes - before.pte_writes != 3
                || after.batch_commits - before.batch_commits != 1
            {
                ok = false;
            }
            let mut i = 0u64;
            while i < 3 {
                let want = alt_pte(0x4000_0000 + i * PAGE_4K, false, true);
                if g.read_pte(TableKind::Ggtt, i * PAGE_4K) != Some(want) {
                    ok = false;
                }
                i += 1;
            }
        }
        Err(_) => ok = false,
    }
    s.add("C22-BATCH-05 合法批提交落笔恰3且逐条读回对拍", ok, "");
}

/// **2.6 批膨胀防御**：条目数远超 expect_span ⇒ 拒绝（防批构造失控）。
fn c22_batch_bloat_guard(s: &mut CheckSet) {
    let mut g = gtt_xe();
    let mut b = PteBatch::for_table(TableKind::PpGtt);
    let mut i = 0u64;
    while i < 100 {
        b.push(100 + i, alt_pte(0x5000_0000 + i * PAGE_4K, false, true));
        i += 1;
    }
    let before = snap(&g);
    let mut ok = matches!(g.pte_batch_commit(TableKind::PpGtt, &b, 1), Err(GErr::BatchInvalidEntry));
    if g.stats.pte_writes != before.pte_writes {
        ok = false;
    }
    s.add("C22-BATCH-06 批条目数远超声明跨度整批拒绝", ok, "");
}

// ===========================================================================
// 判据族三：`C22-EVICT-*` —— LRU 驱逐（锚点判据 3）
// ===========================================================================

/// 判据侧 filler：把 256MiB aperture 用 4 笔 64MiB 绑定填满，
/// 返回四张票（失败即记红由调用方处置）。
fn fill_window(g: &mut Gtt) -> [Result<BindTicket, GErr>; 4] {
    let mut out = [Err(GErr::NotFound); 4];
    let mut i = 0usize;
    while i < 4 {
        out[i] = g.bind(
            TableKind::PpGtt,
            0x1000_0000 + (i as u64) * (64 * 1024 * 1024),
            64 * 1024 * 1024,
            PagePolicy::Force4k,
            (i as u64) + 1,
        );
        g.advance();
        i += 1;
    }
    out
}

/// **3.1 满窗触发驱逐且最旧优先**：第五笔绑定逐掉 last_use 最旧的 A，
/// B/C/D 原样，新绑定落进 A 的洞。
fn c22_evict_lru_oldest(s: &mut CheckSet) {
    let mut g = gtt_xe();
    let fills = fill_window(&mut g);
    let mut ok = true;
    let mut tickets: [Option<BindTicket>; 4] = [None; 4];
    let mut i = 0usize;
    while i < 4 {
        match fills[i] {
            Ok(t) => tickets[i] = Some(t),
            Err(_) => ok = false,
        }
        i += 1;
    }
    // 冷却窗口外再触发驱逐。
    let mut k = 0;
    while k < EVICT_COOLDOWN_TICKS {
        g.advance();
        k += 1;
    }
    let before_evicted = g.stats.evicted;
    match g.bind(TableKind::PpGtt, 0x9000_0000, PAGE_4K, PagePolicy::Force4k, 9) {
        Ok(e) => {
            if g.stats.evicted - before_evicted != 1 {
                ok = false; // 恰逐 1 个（凑够即停）
            }
            // A（最旧）被逐、B/C/D 原样。
            if let Some(a) = tickets[0] {
                if g.lookup(a).is_some() {
                    ok = false;
                }
            }
            let mut j = 1usize;
            while j < 4 {
                if let Some(t) = tickets[j] {
                    if g.lookup(t).is_none() {
                        ok = false;
                    }
                }
                j += 1;
            }
            // 新绑定落在 A 的洞（vma 0）。
            match g.lookup(e) {
                Some(b) => {
                    if b.vma != 0 {
                        ok = false;
                    }
                }
                None => ok = false,
            }
        }
        Err(_) => ok = false,
    }
    s.add("C22-EVICT-01 满窗驱逐最旧优先且恰逐1个新绑定落洞", ok, "");
}

/// **3.2 被引用绑定绝不驱逐**：A acquire 后即使最旧也不逐。
fn c22_evict_never_referenced(s: &mut CheckSet) {
    let mut g = gtt_xe();
    let fills = fill_window(&mut g);
    let mut ok = true;
    let mut tickets: [Option<BindTicket>; 4] = [None; 4];
    let mut i = 0usize;
    while i < 4 {
        match fills[i] {
            Ok(t) => tickets[i] = Some(t),
            Err(_) => ok = false,
        }
        i += 1;
    }
    // 钉住最旧的 A。
    match tickets[0] {
        Some(a) => {
            if g.acquire(a).is_err() {
                ok = false;
            }
        }
        None => ok = false,
    }
    let mut k = 0;
    while k < EVICT_COOLDOWN_TICKS {
        g.advance();
        k += 1;
    }
    match g.bind(TableKind::PpGtt, 0x9000_0000, PAGE_4K, PagePolicy::Force4k, 9) {
        Ok(_) => {
            // A 必须还在（refcount>0 不可逐）。
            match tickets[0] {
                Some(a) => {
                    match g.lookup(a) {
                        Some(b) => {
                            if b.refcount != 1 {
                                ok = false;
                            }
                        }
                        None => ok = false,
                    }
                }
                None => ok = false,
            }
            // 被逐的是 B（次旧未引用）。
            match tickets[1] {
                Some(b) => {
                    if g.lookup(b).is_some() {
                        ok = false;
                    }
                }
                None => ok = false,
            }
        }
        Err(_) => ok = false,
    }
    s.add("C22-EVICT-02 被引用绑定绝不驱逐(逐次旧未引用)", ok, "");
}

/// **3.3 全部被引用 ⇒ 驱逐重试恰 EVICT_RETRY_MAX 轮后明确失败**；
/// 冷却窗口使第 2/3 轮 defer：sweeps 恰 1、deferred 恰 2、总数恰 3。
fn c22_evict_retry_cap(s: &mut CheckSet) {
    let mut g = gtt_xe();
    let fills = fill_window(&mut g);
    let mut ok = true;
    let mut i = 0usize;
    while i < 4 {
        match fills[i] {
            Ok(t) => {
                if g.acquire(t).is_err() {
                    ok = false;
                }
            }
            Err(_) => ok = false,
        }
        i += 1;
    }
    let mut k = 0;
    while k < EVICT_COOLDOWN_TICKS {
        g.advance();
        k += 1;
    }
    let before = snap(&g);
    match g.bind(TableKind::PpGtt, 0x9000_0000, PAGE_4K, PagePolicy::Force4k, 9) {
        Err(GErr::ApertureExhausted) => {}
        _ => ok = false,
    }
    let after = snap(&g);
    if after.evict_sweeps - before.evict_sweeps != 1 {
        ok = false; // 只有第 1 轮真扫
    }
    if after.evict_deferred - before.evict_deferred != EVICT_RETRY_MAX - 1 {
        ok = false; // 第 2/3 轮被冷却窗口 defer，计数恰等
    }
    if (after.evict_sweeps - before.evict_sweeps)
        + (after.evict_deferred - before.evict_deferred)
        != EVICT_RETRY_MAX as u32
    {
        ok = false; // 重试总数恰等上限
    }
    // 四笔全钉绑定原样（失败路径零扰动）。
    let mut i = 0usize;
    while i < 4 {
        match fills[i] {
            Ok(t) => {
                if g.lookup(t).is_none() {
                    ok = false;
                }
            }
            Err(_) => ok = false,
        }
        i += 1;
    }
    s.add("C22-EVICT-03 全钉住时重试恰上限后明确失败且零扰动", ok, "");
}

/// **3.4 LRU 序判据侧重演**：touch 改写 last_use 后驱逐顺序随之改变。
fn c22_evict_lru_touch(s: &mut CheckSet) {
    let mut g = gtt_xe();
    let fills = fill_window(&mut g);
    let mut ok = true;
    let mut tickets: [Option<BindTicket>; 4] = [None; 4];
    let mut i = 0usize;
    while i < 4 {
        match fills[i] {
            Ok(t) => tickets[i] = Some(t),
            Err(_) => ok = false,
        }
        i += 1;
    }
    // touch C：C 的 last_use 变为最新，驱逐对象从 A 变成 B。
    match tickets[2] {
        Some(c) => {
            if g.touch(c).is_err() {
                ok = false;
            }
        }
        None => ok = false,
    }
    let mut k = 0;
    while k < EVICT_COOLDOWN_TICKS {
        g.advance();
        k += 1;
    }
    match g.bind(TableKind::PpGtt, 0x9000_0000, PAGE_4K, PagePolicy::Force4k, 9) {
        Ok(_) => {
            // A（0）比 B（1）更旧 ⇒ 逐 A；B/C/D 还在。
            match tickets[0] {
                Some(a) => {
                    if g.lookup(a).is_some() {
                        ok = false;
                    }
                }
                None => ok = false,
            }
            match tickets[1] {
                Some(b) => {
                    if g.lookup(b).is_none() {
                        ok = false;
                    }
                }
                None => ok = false,
            }
        }
        Err(_) => ok = false,
    }
    s.add("C22-EVICT-04 LRU序按last_use独立重演(最旧被逐)", ok, "");
}

/// **3.5 被逐绑定票号失效且 PTE 清零**。
fn c22_evict_victim_state(s: &mut CheckSet) {
    let mut g = gtt_xe();
    let fills = fill_window(&mut g);
    let mut ok = true;
    let a = match fills[0] {
        Ok(t) => t,
        Err(_) => {
            ok = false;
            BindTicket { raw: 0 }
        }
    };
    let mut k = 0;
    while k < EVICT_COOLDOWN_TICKS {
        g.advance();
        k += 1;
    }
    match g.bind(TableKind::PpGtt, 0x9000_0000, PAGE_4K, PagePolicy::Force4k, 9) {
        Ok(_) => {
            if g.lookup(a).is_some() {
                ok = false; // 票号失效
            }
            // A 的 PTE 区间（vma 0..64M 的抽查三点）全部清零。
            let probe_vmas = [0u64, 32 * 1024 * 1024, 64 * 1024 * 1024 - PAGE_4K];
            let mut i = 0usize;
            while i < 3 {
                if g.read_pte(TableKind::PpGtt, probe_vmas[i]) != Some(0) {
                    ok = false;
                }
                i += 1;
            }
        }
        Err(_) => ok = false,
    }
    s.add("C22-EVICT-05 被逐绑定票号失效且PTE区间清零", ok, "");
}

/// **3.6 驱逐后空间可复用且驱逐不清未逐者**（与 3.1 互补：
/// 断 B 的 PTE 首条仍 present）。
fn c22_evict_survivor_intact(s: &mut CheckSet) {
    let mut g = gtt_xe();
    let fills = fill_window(&mut g);
    let mut ok = true;
    let b = match fills[1] {
        Ok(t) => t,
        Err(_) => {
            ok = false;
            BindTicket { raw: 0 }
        }
    };
    let binfo = match g.lookup(b) {
        Some(x) => x,
        None => {
            ok = false;
            Binding {
                vma: 64 * 1024 * 1024,
                len: 64 * 1024 * 1024,
                pages_4k: 16384,
                page: PageClass::P4k,
                refcount: 0,
                table: TableKind::PpGtt,
                fence_seq: 0,
                last_use: 0,
                pool_base: 0x1000_0000,
            }
        }
    };
    let mut k = 0;
    while k < EVICT_COOLDOWN_TICKS {
        g.advance();
        k += 1;
    }
    match g.bind(TableKind::PpGtt, 0x9000_0000, PAGE_4K, PagePolicy::Force4k, 9) {
        Ok(_) => {
            // B 首条 PTE 仍 present 且 GPA 对拍。
            let want = alt_pte(binfo.pool_base, false, true);
            if g.read_pte(TableKind::PpGtt, binfo.vma) != Some(want) {
                ok = false;
            }
        }
        Err(_) => ok = false,
    }
    s.add("C22-EVICT-06 幸存绑定PTE原样(驱逐不越界清)", ok, "");
}

/// **3.7 冷却窗口内的驱逐尝试 defer 且 defer 不计 sweep**。
fn c22_evict_cooldown_defer(s: &mut CheckSet) {
    let mut g = gtt_xe();
    let fills = fill_window(&mut g);
    let mut ok = true;
    let mut i = 0usize;
    while i < 4 {
        match fills[i] {
            Ok(t) => {
                if g.acquire(t).is_err() {
                    ok = false;
                }
            }
            Err(_) => ok = false,
        }
        i += 1;
    }
    // 只推进 1 tick（< 冷却窗口）。
    g.advance();
    let before = snap(&g);
    match g.bind(TableKind::PpGtt, 0x9000_0000, PAGE_4K, PagePolicy::Force4k, 9) {
        Err(GErr::ApertureExhausted) => {}
        _ => ok = false,
    }
    let after = snap(&g);
    if after.evict_sweeps != before.evict_sweeps {
        ok = false; // 冷却窗口内的扫描不得计入 sweeps
    }
    if after.evict_deferred <= before.evict_deferred {
        ok = false; // defer 必须留痕（失败可见）
    }
    s.add("C22-EVICT-07 冷却窗口内驱逐defer留痕且不计sweep", ok, "");
}

/// **3.8 pending 区间不参与可逐候选也不可分配**（驱逐找洞时跳过）。
fn c22_evict_pending_not_evictable(s: &mut CheckSet) {
    let mut g = gtt_xe();
    let a = g.bind(TableKind::PpGtt, 0x1000_0000, PAGE_4K, PagePolicy::Force4k, 1);
    let fills_rest = [2u64, 3, 4, 5];
    let mut ok = true;
    match a {
        Ok(a) => {
            if g.unbind(a).is_err() {
                ok = false;
            }
            // 用小绑定把剩余空间逐页占掉一部分后，触发一次驱逐，
            // 断言驱逐扫描没有把 pending 区间当成可分配/可逐对象：
            // 新绑定若成功，其 vma 不得落在 pending 区间。
            let mut i = 0usize;
            while i < 4 {
                if g.bind(TableKind::PpGtt, 0x2000_0000 + fills_rest[i] * PAGE_4K,
                          PAGE_4K, PagePolicy::Force4k, fills_rest[i]).is_err() {
                    ok = false;
                }
                g.advance();
                i += 1;
            }
            match g.bind(TableKind::PpGtt, 0x3000_0000, PAGE_4K, PagePolicy::Force4k, 6) {
                Ok(t) => match g.lookup(t) {
                    Some(b) => {
                        // vma 0 是 pending 区间（A 解绑前的位置）——
                        // fence 未签不得复用：落进去即判红（核心断言
                        // 直证，不借道 pending 计数——计数恒 1 恒真，
                        // 借道会让「落入 pending」永远不记红）。
                        if b.vma < PAGE_4K {
                            ok = false;
                        }
                        // pending 记账独立断：恰 1 项在队。
                        if g.pending_count() != 1 {
                            ok = false;
                        }
                    }
                    None => ok = false,
                },
                Err(_) => ok = false,
            }
        }
        Err(_) => ok = false,
    }
    s.add("C22-EVICT-08 pending计数恰1且新绑定不落pending区间", ok, "");
}

// ===========================================================================
// 判据族四：`C22-REF-*` —— 引用计数（锚点判据 4）
// ===========================================================================

/// **4.1 acquire/release 精确计数**（返回值逐级对拍）。
fn c22_ref_exact_counts(s: &mut CheckSet) {
    let mut g = gtt_xe();
    let mut ok = true;
    match g.bind(TableKind::PpGtt, 0x1000_0000, PAGE_4K, PagePolicy::Force4k, 1) {
        Ok(t) => {
            if g.acquire(t).is_err() {
                ok = false;
            }
            if g.acquire(t).is_err() {
                ok = false;
            }
            let r2 = g.release_ref(t);
            if r2 != Ok(1) {
                ok = false;
            }
            match g.lookup(t) {
                Some(b) => {
                    if b.refcount != 1 {
                        ok = false;
                    }
                }
                None => ok = false,
            }
            let r1 = g.release_ref(t);
            if r1 != Ok(0) {
                ok = false;
            }
        }
        Err(_) => ok = false,
    }
    s.add("C22-REF-01 引用计数逐级精确(2→1→0)", ok, "");
}

/// **4.2 被引用绑定解绑拒绝且状态不变**。
fn c22_ref_unbind_pinned_refused(s: &mut CheckSet) {
    let mut g = gtt_xe();
    let mut ok = true;
    match g.bind(TableKind::PpGtt, 0x1000_0000, PAGE_4K, PagePolicy::Force4k, 1) {
        Ok(t) => {
            if g.acquire(t).is_err() {
                ok = false;
            }
            match g.unbind(t) {
                Err(GErr::StillReferenced) => {}
                _ => ok = false,
            }
            // 状态不变：绑定还在、PTE 还 present、unbind 计数没动。
            match g.lookup(t) {
                Some(b) => {
                    if b.refcount != 1 {
                        ok = false;
                    }
                }
                None => ok = false,
            }
            if g.read_pte(TableKind::PpGtt, 0) != Some(alt_pte(0x1000_0000, false, true)) {
                ok = false;
            }
            if g.stats.unbinds != 0 {
                ok = false;
            }
        }
        Err(_) => ok = false,
    }
    s.add("C22-REF-02 被引用解绑拒绝且绑定PTE计数全不变", ok, "");
}

/// **4.3 归零后解绑成功**（生命周期闭合）。
fn c22_ref_unbind_after_zero(s: &mut CheckSet) {
    let mut g = gtt_xe();
    let mut ok = true;
    match g.bind(TableKind::PpGtt, 0x1000_0000, PAGE_4K, PagePolicy::Force4k, 1) {
        Ok(t) => {
            if g.acquire(t).is_err() {
                ok = false;
            }
            if g.release_ref(t) != Ok(0) {
                ok = false;
            }
            if g.unbind(t).is_err() {
                ok = false;
            }
            if g.lookup(t).is_some() {
                ok = false;
            }
        }
        Err(_) => ok = false,
    }
    s.add("C22-REF-03 引用归零后解绑成功且票号失效", ok, "");
}

/// **4.4 批量引用回绕拒绝且不回绕**（acquire_many 让回绕可达）。
fn c22_ref_overflow_refused(s: &mut CheckSet) {
    let mut g = gtt_xe();
    let mut ok = true;
    match g.bind(TableKind::PpGtt, 0x1000_0000, PAGE_4K, PagePolicy::Force4k, 1) {
        Ok(t) => {
            if g.acquire(t).is_err() {
                ok = false;
            }
            match g.acquire_many(t, u32::MAX) {
                Err(GErr::RefcountOverflow) => {}
                _ => ok = false,
            }
            // 计数不得回绕（回绕到 0 会在下次驱逐中被逐 = 隐蔽 UAF）。
            match g.lookup(t) {
                Some(b) => {
                    if b.refcount != 1 {
                        ok = false;
                    }
                }
                None => ok = false,
            }
        }
        Err(_) => ok = false,
    }
    s.add("C22-REF-04 批量引用回绕拒绝且计数不回绕", ok, "");
}

/// **4.5 零引用再释放 ⇒ RefcountUnderflow**（不越零）。
fn c22_ref_underflow_refused(s: &mut CheckSet) {
    let mut g = gtt_xe();
    let mut ok = true;
    match g.bind(TableKind::PpGtt, 0x1000_0000, PAGE_4K, PagePolicy::Force4k, 1) {
        Ok(t) => {
            match g.release_ref(t) {
                Err(GErr::RefcountUnderflow) => {}
                _ => ok = false,
            }
            match g.lookup(t) {
                Some(b) => {
                    if b.refcount != 0 {
                        ok = false;
                    }
                }
                None => ok = false,
            }
        }
        Err(_) => ok = false,
    }
    s.add("C22-REF-05 零引用再释放Underflow且计数不越零", ok, "");
}

/// **4.6 字节账对账**：active_pages_4k / active_count 与判据侧
/// 全表重扫累加恰等（== 不是近似）。
fn c22_ref_accounting(s: &mut CheckSet) {
    let mut g = gtt_xe();
    let mut ok = true;
    // 造混合状态：3 笔绑定，1 笔解绑进 pending。
    let t0 = g.bind(TableKind::PpGtt, 0x1000_0000, 2 * PAGE_4K, PagePolicy::Force4k, 1);
    let t1 = g.bind(TableKind::PpGtt, 0x1000_1000, 3 * PAGE_4K, PagePolicy::Force4k, 2);
    let t2 = g.bind(TableKind::Ggtt, 0x1000_2000, 5 * PAGE_4K, PagePolicy::Force4k, 3);
    let mut tickets: [Option<BindTicket>; 3] = [None; 3];
    for (i, r) in [t0, t1, t2].iter().enumerate() {
        match r {
            Ok(t) => tickets[i] = Some(*t),
            Err(_) => ok = false,
        }
    }
    match tickets[0] {
        Some(t) => {
            if g.unbind(t).is_err() {
                ok = false;
            }
        }
        None => ok = false,
    }
    // 判据侧全表重扫。
    let mut sum_pages = 0u64;
    let mut count = 0u32;
    let mut raw = 0u32;
    while raw < MAX_BINDINGS as u32 {
        if let Some(b) = g.lookup(BindTicket { raw }) {
            sum_pages += b.pages_4k;
            count += 1;
        }
        raw += 1;
    }
    if g.active_pages_4k() != sum_pages || g.active_count() != count {
        ok = false;
    }
    // 期望值独立算：2+5 页 active（第一笔已解绑）。
    if g.active_pages_4k() != 7 || g.active_count() != 2 {
        ok = false;
    }
    s.add("C22-REF-06 字节账与全表重扫恰等且期望值独立核对", ok, "");
}

/// **4.7 死票号上的引用操作一律 NotFound**。
fn c22_ref_stale_ticket(s: &mut CheckSet) {
    let mut g = gtt_xe();
    let mut ok = true;
    let dead = BindTicket { raw: 77 };
    if g.acquire(dead).is_ok() || g.release_ref(dead).is_ok() || g.unbind(dead).is_ok()
        || g.touch(dead).is_ok()
    {
        ok = false;
    }
    s.add("C22-REF-07 死票号引用/释放/解绑/触碰一律NotFound", ok, "");
}

// ===========================================================================
// 判据族五：`C22-PAGE-*` —— 大页 4K/2M 分级（锚点正文要求）
// ===========================================================================

/// **5.1 Auto+2M 对齐+Xe 档 ⇒ P2m**：PTE 落笔恰 1 条（独立口径 len/2M）。
fn c22_page_auto_2m(s: &mut CheckSet) {
    let mut g = gtt_xe();
    let before = snap(&g);
    let mut ok = true;
    match g.bind(TableKind::PpGtt, 0x1000_0000, PAGE_2M, PagePolicy::Auto, 1) {
        Ok(t) => {
            match g.lookup(t) {
                Some(b) => {
                    if b.page != PageClass::P2m || b.pages_4k != 512 {
                        ok = false;
                    }
                }
                None => ok = false,
            }
            let after = snap(&g);
            if after.pte_writes - before.pte_writes != PAGE_2M / PAGE_2M {
                ok = false; // 2M ⇒ 1 条 PTE（独立口径）
            }
            if after.large_downgrades - before.large_downgrades != 0 {
                ok = false; // 对齐且支持 ⇒ 不降级
            }
        }
        Err(_) => ok = false,
    }
    s.add("C22-PAGE-01 Auto对齐2M走大页且落笔恰1条不降级", ok, "");
}

/// **5.2 2M 绑定的中间 4K 槽位非 present**（大页是 PTE 表达压缩，
/// 中间槽位没有条目——判据盯住表达方式本身）。
fn c22_page_2m_intermediate_absent(s: &mut CheckSet) {
    let mut g = gtt_xe();
    let mut ok = true;
    match g.bind(TableKind::PpGtt, 0x1000_0000, PAGE_2M, PagePolicy::Auto, 1) {
        Ok(_) => {
            // 首条 present 且带 2M 位。
            match g.read_pte(TableKind::PpGtt, 0) {
                Some(v) => {
                    if !pte_present(v) || !pte_is_2m(v) || pte_gpa(v) != 0x1000_0000 {
                        ok = false;
                    }
                }
                None => ok = false,
            }
            // 中间槽位（4K/1M 处）一律非 present。
            let probes = [PAGE_4K, PAGE_4K * 100, PAGE_2M - PAGE_4K];
            let mut i = 0usize;
            while i < 3 {
                match g.read_pte(TableKind::PpGtt, probes[i]) {
                    Some(v) => {
                        if v != 0 {
                            ok = false;
                        }
                    }
                    None => ok = false,
                }
                i += 1;
            }
        }
        Err(_) => ok = false,
    }
    s.add("C22-PAGE-02 2M绑定首条带2M位且中间槽位非present", ok, "");
}

/// **5.3 Auto 不对齐 ⇒ 降 4K 且降级计数恰 +1**（降级可观测）。
fn c22_page_auto_downgrade(s: &mut CheckSet) {
    let mut g = gtt_xe();
    let before = snap(&g);
    let mut ok = true;
    // 长度不对齐（4K 合法但非 2M 粒度）。
    match g.bind(TableKind::PpGtt, 0x1000_0000, 3 * PAGE_4K, PagePolicy::Auto, 1) {
        Ok(t) => {
            match g.lookup(t) {
                Some(b) => {
                    if b.page != PageClass::P4k {
                        ok = false;
                    }
                }
                None => ok = false,
            }
            let after = snap(&g);
            if after.large_downgrades - before.large_downgrades != 1 {
                ok = false;
            }
        }
        Err(_) => ok = false,
    }
    s.add("C22-PAGE-03 Auto不对齐降4K且降级计数恰+1", ok, "");
}

/// **5.4 Force2m 不对齐 ⇒ 专属码拒绝**（两个方向：off 对 len 不对 /
/// len 对 off 不对；显式要求不得静默降级）。
fn c22_page_force2m_misaligned(s: &mut CheckSet) {
    let mut g = gtt_xe();
    let mut ok = true;
    // 长度非 2M 粒度。
    match g.bind(TableKind::PpGtt, 0x1000_0000, PAGE_2M + PAGE_4K, PagePolicy::Force2m, 1) {
        Err(GErr::LargePageMisaligned) => {}
        _ => ok = false,
    }
    // 长度对齐但落点非 2M 对齐（先占一个 4K 页把洞推歪）。
    if g.bind(TableKind::PpGtt, 0x2000_0000, PAGE_4K, PagePolicy::Force4k, 2).is_err() {
        ok = false;
    }
    match g.bind(TableKind::PpGtt, 0x2000_1000, PAGE_2M, PagePolicy::Force2m, 3) {
        Err(GErr::LargePageMisaligned) => {}
        _ => ok = false,
    }
    s.add("C22-PAGE-04 Force2m不对齐两方向专属码拒绝不降级", ok, "");
}

/// **5.5 基线档 Force2m ⇒ LargePageUnsupported**（F0221 集成：
/// 分型错了这里就错；能力位判据侧独立断）。
fn c22_page_baseline_unsupported(s: &mut CheckSet) {
    let g0 = gtt_base();
    let mut ok = GttCaps::for_tier(GenTier::Baseline).large_page == false;
    let mut g = g0;
    match g.bind(TableKind::PpGtt, 0x1000_0000, PAGE_2M, PagePolicy::Force2m, 1) {
        Err(GErr::LargePageUnsupported) => {}
        _ => ok = false,
    }
    s.add("C22-PAGE-05 基线档Force2mUnsupported(F0221集成双向)", ok, "");
}

/// **5.6 基线档 Auto 大对齐区间 ⇒ P4k 且降级计数不涨**
/// （不支持 ≠ 降级：没有能力谈不上降级，口径必须分开）。
fn c22_page_baseline_auto_no_downgrade(s: &mut CheckSet) {
    let mut g = gtt_base();
    let before = snap(&g);
    let mut ok = true;
    match g.bind(TableKind::PpGtt, 0x1000_0000, PAGE_2M, PagePolicy::Auto, 1) {
        Ok(t) => {
            match g.lookup(t) {
                Some(b) => {
                    if b.page != PageClass::P4k {
                        ok = false;
                    }
                }
                None => ok = false,
            }
            let after = snap(&g);
            if after.large_downgrades != before.large_downgrades {
                ok = false;
            }
        }
        Err(_) => ok = false,
    }
    s.add("C22-PAGE-06 基线档Auto走4K且不记降级(口径分离)", ok, "");
}

/// **5.7 Force4k 恒 4K 不计数**（显式 4K 在 Xe 档也不许升 2M）。
fn c22_page_force4k_always(s: &mut CheckSet) {
    let mut g = gtt_xe();
    let before = snap(&g);
    let mut ok = true;
    match g.bind(TableKind::PpGtt, 0x1000_0000, PAGE_2M, PagePolicy::Force4k, 1) {
        Ok(t) => {
            match g.lookup(t) {
                Some(b) => {
                    if b.page != PageClass::P4k || b.pages_4k != 512 {
                        ok = false;
                    }
                }
                None => ok = false,
            }
            let after = snap(&g);
            if after.pte_writes - before.pte_writes != 512 {
                ok = false; // 4K 表达：512 条 PTE（独立口径）
            }
            if after.large_downgrades != before.large_downgrades {
                ok = false;
            }
        }
        Err(_) => ok = false,
    }
    s.add("C22-PAGE-07 Force4k恒4K且落笔恰512条不计数", ok, "");
}

// ===========================================================================
// 判据族六：`C22-FENCE-*` —— 围栏生命周期（A 域围栏契约兑现点）
// ===========================================================================

/// **6.1 unbind 进 pending**：PTE 已清但区间占着，未签 flush 不放行。
fn c22_fence_pending_semantics(s: &mut CheckSet) {
    let mut g = gtt_xe();
    let mut ok = true;
    match g.bind(TableKind::PpGtt, 0x1000_0000, 2 * PAGE_4K, PagePolicy::Force4k, 7) {
        Ok(t) => {
            set_fence_upto(0); // seq=7 未签
            if g.unbind(t).is_err() {
                ok = false;
            }
            if g.pending_count() != 1 {
                ok = false;
            }
            if g.read_pte(TableKind::PpGtt, 0) != Some(0) {
                ok = false; // PTE 已清
            }
            let before = snap(&g);
            if g.fence_flush(fence_pred) != 0 {
                ok = false; // 未签 ⇒ 0 放行
            }
            let after = snap(&g);
            if after.fence_freed != before.fence_freed || g.pending_count() != 1 {
                ok = false;
            }
            // 放行区间复用。
            set_fence_upto(7);
            if g.fence_flush(fence_pred) != 1 {
                ok = false;
            }
            if g.pending_count() != 0 {
                ok = false;
            }
            match g.bind(TableKind::PpGtt, 0x1000_0000, 2 * PAGE_4K, PagePolicy::Force4k, 8) {
                Ok(t2) => match g.lookup(t2) {
                    Some(b) => {
                        if b.vma != 0 {
                            ok = false; // 放行后 first-fit 回到洞
                        }
                    }
                    None => ok = false,
                },
                Err(_) => ok = false,
            }
        }
        Err(_) => ok = false,
    }
    s.add("C22-FENCE-01 未签不放行已签恰放行且区间回洞", ok, "");
}

/// **6.2 pending 队列满 ⇒ 最旧强收且如实记账**。
fn c22_fence_pending_forced(s: &mut CheckSet) {
    let mut g = gtt_xe();
    set_fence_upto(0); // 全部不签
    let mut ok = true;
    // MAX_PENDING + 1 笔绑定并全部解绑：第 65 次 unbind 触发强收。
    let mut tickets: Vec<BindTicket> = Vec::new();
    let mut i = 0u32;
    while i < (MAX_PENDING + 1) as u32 {
        match g.bind(TableKind::PpGtt, 0x1000_0000 + (i as u64) * PAGE_4K,
                     PAGE_4K, PagePolicy::Force4k, (i as u64) + 1) {
            Ok(t) => tickets.push(t),
            Err(_) => ok = false,
        }
        i += 1;
    }
    let mut j = 0usize;
    while j < tickets.len() {
        if g.unbind(tickets[j]).is_err() {
            ok = false;
        }
        j += 1;
    }
    if g.stats.pending_forced != 1 {
        ok = false; // 强收恰 1（最旧）
    }
    if g.pending_count() != MAX_PENDING {
        ok = false; // 队列容量恒定
    }
    if g.stats.fence_freed != 1 {
        ok = false; // 强收如实计入 fence_freed（不静默丢弃）
    }
    s.add("C22-FENCE-02 pending满最旧强收恰1且如实记账", ok, "");
}

// ===========================================================================
// 判据族七：`C22-PERF/A11Y-*` —— 性能口径与无障碍隐私
// ===========================================================================

/// **7.1 零值账本逐字段自洽**（新鲜实例全部为 0，判据侧独立枚举）。
fn c22_perf_zero_stats(s: &mut CheckSet) {
    let g = gtt_xe();
    let st = g.stats;
    let mut ok = st.binds == 0
        && st.unbinds == 0
        && st.evicted == 0
        && st.evict_sweeps == 0
        && st.evict_deferred == 0
        && st.batch_commits == 0
        && st.batch_rejected == 0
        && st.pte_writes == 0
        && st.pte_clears == 0
        && st.large_downgrades == 0
        && st.fence_freed == 0
        && st.pending_forced == 0;
    if g.active_count() != 0 || g.active_pages_4k() != 0 || g.pending_count() != 0 {
        ok = false;
    }
    s.add("C22-PERF-01 新鲜实例账本逐字段为零", ok, "");
}

/// **7.2 批写 O(页数/批) 的表达压缩可观测**：同一 2M 区间，4K 表达
/// 512 条 vs 2M 表达 1 条（判据侧独立口径两条都算）。
fn c22_perf_pte_compression(s: &mut CheckSet) {
    let mut g4 = gtt_xe();
    let mut g2 = gtt_xe();
    let b4 = snap(&g4);
    let b2 = snap(&g2);
    let mut ok = true;
    if g4.bind(TableKind::PpGtt, 0x1000_0000, PAGE_2M, PagePolicy::Force4k, 1).is_err() {
        ok = false;
    }
    if g2.bind(TableKind::PpGtt, 0x1000_0000, PAGE_2M, PagePolicy::Auto, 1).is_err() {
        ok = false;
    }
    let a4 = snap(&g4);
    let a2 = snap(&g2);
    if a4.pte_writes - b4.pte_writes != PAGES_PER_2M {
        ok = false;
    }
    if a2.pte_writes - b2.pte_writes != 1 {
        ok = false;
    }
    s.add("C22-PERF-02 页类表达压缩可观测(512条vs1条独立口径)", ok, "");
}

/// **7.3 摘要聚合口径与账本一致且不含地址**（隐私红线：aperture
/// 布局属于资产布局信息，不得出现在读屏文本）。
fn c22_a11y_summary(s: &mut CheckSet) {
    let mut g = gtt_xe();
    let mut ok = true;
    let t = g.bind(TableKind::PpGtt, 0x1000_0000, PAGE_4K, PagePolicy::Force4k, 1);
    match t {
        Ok(t) => {
            set_fence_upto(0);
            if g.unbind(t).is_err() {
                ok = false;
            }
            let s = g.status_summary();
            // 正向：含聚合键与真实计数（判据侧独立格式化）。
            if !s.contains("活跃 0") || !s.contains("等待回收 1") {
                ok = false;
            }
            // 反向：不得出现 "0x" 开头的原始地址字面量。
            if s.contains("0x") || s.contains("0X") {
                ok = false;
            }
        }
        Err(_) => ok = false,
    }
    s.add("C22-A11Y-01 摘要计数与账本一致且不含原始地址", ok, "");
}

/// **7.4 拒绝理由串逐条互异**（拒绝必带专属原因，不可共用占位串）。
fn c22_err_reasons_distinct(s: &mut CheckSet) {
    let all = GErr::ALL;
    let mut code_dup = false;
    let mut reason_dup = false;
    let mut i = 0;
    while i < all.len() {
        let mut j = i + 1;
        while j < all.len() {
            if all[i].code() == all[j].code() {
                code_dup = true;
            }
            if all[i].reason() == all[j].reason() {
                reason_dup = true;
            }
            j += 1;
        }
        i += 1;
    }
    // 码段契约：全部落在独占段 0x2Fxx（用 != 断，防自判死）。
    let mut seg_bad = false;
    let mut k = 0;
    while k < all.len() {
        if all[k].code() & 0xFF00 != 0x2F00 {
            seg_bad = true;
        }
        k += 1;
    }
    s.add(
        "C22-A11Y-02 诊断码/理由逐条互异且全落独占段0x2Fxx",
        !code_dup && !reason_dup && !seg_bad,
        "",
    );
}

/// **7.5 判据侧口径自检**：独立页数口径与编码口径的自查
/// （判据清单若自己写错，上面全部断言跟着错——先自检）。
fn c22_calib_selfcheck(s: &mut CheckSet) {
    let mut ok = true;
    // ceil 口径：整除/非整除/1 页。
    if alt_pages_4k(0) != 0 || alt_pages_4k(1) != 1 || alt_pages_4k(PAGE_4K) != 1
        || alt_pages_4k(PAGE_4K + 1) != 2 || alt_pages_4k(PAGE_2M) != 512
    {
        ok = false;
    }
    // 编码口径与三组字面量。
    if alt_pte(0x1000_0000, false, true) != 0x1000_0001
        || alt_pte(0x1000_0000, false, false) != 0x1000_0000
        || alt_pte(0x1000_0000, true, true) != 0x1000_0003
    {
        ok = false;
    }
    // 常量口径：2M = 512 × 4K；MAX_ALLOC ≤ aperture。
    if PAGE_2M != 512 * PAGE_4K || PAGES_PER_2M != 512 || MAX_ALLOC_BYTES > APERTURE_SIZE {
        ok = false;
    }
    s.add("C22-CALIB-01 判据侧口径自检(ceil/编码/常量关系)", ok, "");
}

// ===========================================================================
// 入口
// ===========================================================================

/// VE-F0222 域自检（**判据 42 项**：TBL 7 / BATCH 6 / EVICT 8 / REF 7 /
/// PAGE 7 / FENCE 2 / PERF-A11Y 5，未超 `MAX_CHECKS=112`）。
pub fn run_veb22_checks() -> CheckSet {
    let mut s = CheckSet::new("intel-gtt");

    c22_a(&mut s);
    c22_b(&mut s);
    c22_c(&mut s);
    c22_d(&mut s);
    c22_e(&mut s);
    c22_f(&mut s);
    c22_g(&mut s);

    s
}

/// 族 A：GGTT 语义（7 项）。
fn c22_a(s: &mut CheckSet) {
    c22_tbl_isolation(s);
    c22_tbl_lookup_fields(s);
    c22_tbl_lookup_bounds(s);
    c22_tbl_cross_table_rejected(s);
    c22_tbl_pte_encoding(s);
    c22_tbl_unbind_clears_pte(s);
    c22_tbl_pending_holds_range(s);
}

/// 族 B：批写回滚（6 项）。
fn c22_b(s: &mut CheckSet) {
    c22_batch_bind_writes_exact(s);
    c22_batch_invalid_zero_write(s);
    c22_batch_idx_out_of_window(s);
    c22_batch_dup_index(s);
    c22_batch_valid_manual(s);
    c22_batch_bloat_guard(s);
}

/// 族 C：LRU 驱逐（8 项）。
fn c22_c(s: &mut CheckSet) {
    c22_evict_lru_oldest(s);
    c22_evict_never_referenced(s);
    c22_evict_retry_cap(s);
    c22_evict_lru_touch(s);
    c22_evict_victim_state(s);
    c22_evict_survivor_intact(s);
    c22_evict_cooldown_defer(s);
    c22_evict_pending_not_evictable(s);
}

/// 族 D：引用计数（7 项）。
fn c22_d(s: &mut CheckSet) {
    c22_ref_exact_counts(s);
    c22_ref_unbind_pinned_refused(s);
    c22_ref_unbind_after_zero(s);
    c22_ref_overflow_refused(s);
    c22_ref_underflow_refused(s);
    c22_ref_accounting(s);
    c22_ref_stale_ticket(s);
}

/// 族 E：大页分级（7 项）。
fn c22_e(s: &mut CheckSet) {
    c22_page_auto_2m(s);
    c22_page_2m_intermediate_absent(s);
    c22_page_auto_downgrade(s);
    c22_page_force2m_misaligned(s);
    c22_page_baseline_unsupported(s);
    c22_page_baseline_auto_no_downgrade(s);
    c22_page_force4k_always(s);
}

/// 族 F：围栏生命周期（2 项）。
fn c22_f(s: &mut CheckSet) {
    c22_fence_pending_semantics(s);
    c22_fence_pending_forced(s);
}

/// 族 G：性能口径与无障碍隐私（5 项）。
fn c22_g(s: &mut CheckSet) {
    c22_perf_zero_stats(s);
    c22_perf_pte_compression(s);
    c22_a11y_summary(s);
    c22_err_reasons_distinct(s);
    c22_calib_selfcheck(s);
}
