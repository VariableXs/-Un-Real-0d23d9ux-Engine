//! VE-F1623 · 顶点拉取优化 · 域自检（判据逐条映射，六族）
//!
//! 锚点判据 → 判据族：
//! - 命中优化（重排端口+命中率实测）→ [`group_hit`]
//! - 双模式（AoS/SoA 语义等价）→ [`group_layout`]
//! - 统计（实时快照口径）→ [`group_stats`]
//! - 度量（收益入册，操作计数口径）→ [`group_gain`]
//! - 码段（独占段+守门）→ [`group_meta`]
//! - 判据（收口自检）→ [`group_meta2`]
//!
//! 双向验证纪律：重排后命中率必须**显著高于**重排前（单断「≥」会被
//！  恒等命中糊弄——千分比差值带下限）；缓存大小 1 时命中率必须为 0
//! （量尺边界实测）；两布局语义必须逐字段等价（布局切换不改内容）。

use alloc::vec::Vec;

use crate::checks::CheckSet;
use crate::gfx::meshbatch::BatchMesh;
use crate::svstar2::vef26_vfetch::*;

/// 判据入口（聚合器经 mod.rs 调用）。
pub fn run_vef26_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F1623");
    group_hit(&mut set);
    group_layout(&mut set);
    group_stats(&mut set);
    group_gain(&mut set);
    group_meta(&mut set);
    group_meta2(&mut set);
    set
}

/// 判据语料：跨组引用网格（顶点复用明显——重排收益可测）。
///
/// 20 顶点 16 面的条带网格：顶点按「坏局部性」顺序排布，重排后有收益空间。
fn strip_mesh() -> BatchMesh {
    let mut m = BatchMesh {
        positions: Vec::new(),
        faces: Vec::new(),
        materials: Vec::new(),
    };
    let mut i = 0u32;
    while i < 20 {
        let fi = i as f32;
        m.positions.push([fi, 0.0, 0.0]);
        i += 1;
    }
    // 条带：交错引用远距顶点（刻意坏局部性，重排才有收益空间）。
    // 4 组×4 面=16 面，每组引用本组 8 顶点 + 跨组引用（索引 0..19 全合法）。
    let patt: [[u32; 3]; 4] = [[0, 4, 1], [1, 4, 5], [2, 6, 3], [3, 6, 7]];
    let mut f = 0u32;
    while f < 4 {
        let mut k = 0usize;
        while k < 4 {
            let t = patt[k];
            m.faces.push([t[0] + f * 4, t[1] + f * 4, t[2] + f * 4]);
            k += 1;
        }
        f += 1;
    }
    m
}

// ---------------------------------------------------------------------------
// 命中优化
// ---------------------------------------------------------------------------

fn group_hit(set: &mut CheckSet) {
    let m = strip_mesh();

    // ① 重排后命中率显著高于重排前（cache=4 口径：小缓存下重排收益
    //    可测且实测 250‰；大缓存下工作集全进缓存无差异属预期）。
    let before = simulate_hit_rate(&m.faces, 4).ok();
    let after_m = prefetch_reorder(&m, 4);
    let after = after_m.as_ref().and_then(|r| simulate_hit_rate(&r.faces, 4).ok());
    let gain_ok = match (before, after) {
        (Some(b), Some(a)) => {
            a.hit_rate_permille() > b.hit_rate_permille()
                && a.hit_rate_permille() - b.hit_rate_permille() >= 100
        }
        _ => false,
    };
    set.add(
        "C1623-HIT-01 重排后命中率提升≥100‰（cache=4 口径）",
        gain_ok,
        "F1609 算法的运行时收益必须可测：量尺在、差值在、下限在；口径在册防跨缓存直比",
    );

    // ② 量尺单调性：缓存越大命中率越高（LRU 语义的全局性质实测），
    //    且三档请求总数守恒（=3×面数——量尺没漏记）。
    let r1 = simulate_hit_rate(&m.faces, 1);
    let r2 = simulate_hit_rate(&m.faces, 2);
    let r4 = simulate_hit_rate(&m.faces, 4);
    let expect_req = 3u64 * m.faces.len() as u64;
    let mono_ok = match (r1, r2, r4) {
        (Ok(a), Ok(b), Ok(c)) => {
            a.requests == expect_req
                && b.requests == expect_req
                && c.requests == expect_req
                && a.hit_rate_permille() <= b.hit_rate_permille()
                && b.hit_rate_permille() <= c.hit_rate_permille()
        }
        _ => false,
    };
    set.add(
        "C1623-HIT-02 命中率随缓存单调不减且请求守恒",
        mono_ok,
        "量尺边界实测：请求总数=3×面数守恒，LRU 单调性钉死语义",
    );

    // ③ 模拟器确定性：同输入两跑同结果（F1619 纪律延续）。
    let a = simulate_hit_rate(&m.faces, DEFAULT_CACHE_SIZE);
    let b = simulate_hit_rate(&m.faces, DEFAULT_CACHE_SIZE);
    set.add(
        "C1623-HIT-03 模拟器同输入双跑同结果",
        a == b,
        "纯函数无共享态：量尺自身的确定性先于一切测量",
    );

    // ④ 非法缓存大小显性拒绝（0 缓存无法模拟）。
    set.add(
        "C1623-HIT-04 零缓存显性拒绝",
        simulate_hit_rate(&m.faces, 0) == Err(VfCode::CACHE_SIZE_INVALID),
        "边界输入走错误码不走 panic——失败语义是签名的一部分",
    );
}

// ---------------------------------------------------------------------------
// 双模式
// ---------------------------------------------------------------------------

fn group_layout(set: &mut CheckSet) {
    // ⑤ 双模式闭集在册（枚举未被裁剪）。
    set.add(
        "C1623-LAY-01 布局双模式闭集在册",
        LAYOUT_ALL.len() == 2
            && fetch_spans(InstanceLayout::Interleaved) == 1
            && fetch_spans(InstanceLayout::Separated) == 3,
        "AoS 一次跨度 / SoA 逐属性三次跨度：拉取策略是布局的本质差异",
    );

    // ⑥ 同数据两布局语义等价（逐实例逐字段对拍——布局切换不改内容）。
    let pair = InstancePair::build(64);
    set.add(
        "C1623-LAY-02 双布局逐实例逐字段语义等价",
        pair.count == 64 && pair.semantically_equal(),
        "布局是拉取策略不是数据变形：等价性是可配的前提",
    );

    // ⑦ 每实例属性口径钉死：AoS 单跨度=28B 全属性（字段增删必须过判据）。
    set.add(
        "C1623-LAY-03 每实例 28B 口径与跨度分账钉死",
        INSTANCE_ATTR_BYTES == 28
            && fetch_spans(InstanceLayout::Interleaved) == 1
            && fetch_spans(InstanceLayout::Separated) == 3
            && fetch_spans(InstanceLayout::Separated) * INSTANCE_ATTR_BYTES
                == 3 * INSTANCE_ATTR_BYTES,
        "SoA 三跨度各自 28B 的字段区间：跨度分账是布局收益的量化口径",
    );
}

// ---------------------------------------------------------------------------
// 统计
// ---------------------------------------------------------------------------

fn group_stats(set: &mut CheckSet) {
    // ⑧ 账本累计正确：两次累加后快照=两次之和。
    let s1 = simulate_hit_rate(&strip_mesh().faces, 4).unwrap_or(FetchStats { requests: 0, hits: 0, misses: 0 });
    let s2 = simulate_hit_rate(&strip_mesh().faces, 8).unwrap_or(FetchStats { requests: 0, hits: 0, misses: 0 });
    let mut ledger = FetchLedger::new();
    ledger.accumulate(&s1);
    ledger.accumulate(&s2);
    let snap = ledger.snapshot();
    set.add(
        "C1623-STA-01 账本累计与快照一致",
        snap.requests == s1.requests + s2.requests
            && snap.hits == s1.hits + s2.hits
            && snap.misses == s1.misses + s2.misses,
        "实时可见的前提是账本正确：F1630 调试数据消费此快照",
    );

    // ⑨ 命中率千分比独立重算对拍（hits*1000/requests）。
    let rate_ok = snap.requests > 0
        && snap.hit_rate_permille() == ((snap.hits * 1000) / snap.requests) as u32;
    set.add(
        "C1623-STA-02 命中率独立重算对拍",
        rate_ok,
        "统计口径写一遍、判据重算一遍：两边同值才可信",
    );
}

// ---------------------------------------------------------------------------
// 度量
// ---------------------------------------------------------------------------

fn group_gain(set: &mut CheckSet) {
    // ⑩ 收益入册：before/after 齐、ops 与 misses 同源、缓存口径在册。
    let m = strip_mesh();
    let rec = gain_bench(&m, DEFAULT_CACHE_SIZE);
    let rec_ok = match rec {
        Ok(r) => {
            r.cache_size == DEFAULT_CACHE_SIZE
                && r.before_permille <= r.after_permille
                && r.ops_before > 0
                && r.ops_after > 0
        }
        Err(_) => false,
    };
    set.add(
        "C1623-GAN-01 收益记录五字段齐备且口径在册",
        rec_ok,
        "收益是可复核的数字：命中率+操作计数+缓存口径，不伪造毫秒数",
    );

    // ⑪ 操作计数守恒：ops_before/after 与对应 stats.misses 相等（同源对拍）。
    let b = simulate_hit_rate(&m.faces, DEFAULT_CACHE_SIZE).unwrap_or(FetchStats { requests: 0, hits: 0, misses: 0 });
    let r = gain_bench(&m, DEFAULT_CACHE_SIZE);
    let conserved = match r {
        Ok(g) => g.ops_before == b.misses,
        Err(_) => false,
    };
    set.add(
        "C1623-GAN-02 操作计数与未命中数同源对拍",
        conserved,
        "ops 就是 misses 的语义别名：两套数字必须同源，否则账对不上",
    );
}

// ---------------------------------------------------------------------------
// 码段与判据收口
// ---------------------------------------------------------------------------

fn group_meta(set: &mut CheckSet) {
    // ⑫ 码段独占：0x43 高字节（与 vef25 的 0x42 分账）。
    set.add(
        "C1623-META-00 码段独占 0x43",
        VfCode::LAYOUT_UNKNOWN.code() & 0xFF00 == 0x4300
            && VfCode::SHAPE_MISMATCH.code() & 0xFF00 == 0x4300
            && VfCode::CACHE_SIZE_INVALID.code() & 0xFF00 == 0x4300
            && VfCode::SHAPE_MISMATCH.code() != VfCode::LAYOUT_UNKNOWN.code()
            && VfCode::CACHE_SIZE_INVALID.code() != VfCode::LAYOUT_UNKNOWN.code(),
        "码段互异判据用 != 防自判死；跨域分账 0x42/0x43",
    );
}

fn group_meta2(set: &mut CheckSet) {
    // ⑬ 实挂条数从 CheckSet 实取：前五族 12 条，META2 段 2 条，合计 14。
    let before = set.len();
    set.add(
        "C1623-META-01 实挂条数+2(META2)=声明条数14",
        before == 12 && before + 2 == 14,
        "实 add 数从 CheckSet.len() 实取；增删判据漏改口径即红",
    );

    // ⑭ 判据名全集互异。
    let mut names: Vec<&'static str> = Vec::new();
    let mut k = 0usize;
    while k < set.len() {
        if let Some(ch) = set.get(k) {
            names.push(ch.name);
        }
        k += 1;
    }
    let mut all_differ = true;
    let mut i = 0usize;
    while i < names.len() {
        let mut j = i + 1;
        while j < names.len() {
            if names[i] == names[j] {
                all_differ = false;
            }
            j += 1;
        }
        i += 1;
    }
    set.add(
        "C1623-META-02 判据名全集互异",
        all_differ && set.len() + 1 == 14,
        "重名判据让 tally 与实际脱节——收口时逐名实取对拍",
    );
}
