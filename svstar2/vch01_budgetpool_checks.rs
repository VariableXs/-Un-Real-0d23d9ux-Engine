//! CGPU-F1121 判据层：H 域开工与显存预算池架构（锚点判据逐条映射）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F1121`
//!
//! **锚点原文六条判据 → 本层判据族**：
//!
//! | 锚点判据 | 判据族 | 要点 |
//! |---|---|---|
//! | 七主题 | `H01-TOPIC-*` | 闭集 7 主题判据侧独立字面量对拍 + 域界常量 |
//! | 三级池 | `H01-POOL-*`/`H01-SET-*` | 超额拒绝/恰好放行双向 + 守恒双向 + 账实对账 + 水位边界 |
//! | C 域兑现 | `H01-VOW-*` | 三条款闭集 + 指纹对拍 + 每条款池侧落地行为直证 |
//! | G 域联动 | `H01-GPU-*` | 三厂商闭集 + Unknown 保守缺省（不假宣称） |
//! | 哲学 | `H01-PHIL-*` | 三条公理字面量冻结对拍 |
//! | 判据 | `H01-META-*` | 条数对账/截断/码段独占 != 防自判死/码两两互异 |
//!
//! # 本层核心纪律：判据侧独立重算，不向被测问答案
//!
//! - 「守恒」若只调 `PoolSet::new` 看 Ok/Err，被测把守恒检查删掉照样绿
//!   （因为判据也构造不出来）——本层**双向**：Σ==total 恰好边界必须放行、
//!   Σ==total+1 必须拒绝，两条都过才证明检查真实存在且边界精确。
//! - 「水位档位」用具体字节数在 79.9%/80%/95% 三个边界两侧重演——
//!   阈值改成 79 或 81 立即红。
//! - 「C 域兑现」不做只读指纹（空头支票）：每条款找**池侧行为兑现点**
//!   直证（拒绝路径/水位联动/归还即时性）。

// ---------------------------------------------------------------------------
// 导入
// ---------------------------------------------------------------------------

use crate::checks::CheckSet;

use super::vch01_budgetpool::*;

// ---------------------------------------------------------------------------
// 判据侧独立参照（不向被测要答案）
// ---------------------------------------------------------------------------

/// 判据侧独立写死的七主题期望标签（按官方顺序，不从被测导出）。
const EXPECTED_TOPICS: [&str; 7] = [
    "显存预算池",
    "显存压缩",
    "换页",
    "碎片治理",
    "泄漏防线",
    "跨进程显存",
    "显存遥测",
];

/// 判据侧独立写死的哲学三公理全文。
const EXPECTED_AXIOMS: [&str; 3] = [
    "显存有界是稳定的前提",
    "无界即事故（复用 E04 哲学）",
    "预算先行：无预算即无分配",
];

/// 判据侧独立写死的 C 域三条款期望标签。
const EXPECTED_VOW: [&str; 3] = [
    "分配前预算校验（超预算拒绝或降质）",
    "超用异常→预算告警",
    "回收→预算即时归还",
];

/// 判据侧独立实现的 FNV-1a（对拍被测 vow 指纹——口径同、代码异）。
fn alt_vow_fingerprint(version: u32) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for label in EXPECTED_VOW {
        for b in label.as_bytes() {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        h ^= 0xFF;
        h = h.wrapping_mul(0x0100_0193);
    }
    h ^ (version.wrapping_mul(0x9E37_79B9))
}

/// 判据侧独立写死的水位阈值。
const EXP_WATER_HIGH: u64 = 80;
const EXP_WATER_CRITICAL: u64 = 95;

/// 全部诊断码（判据侧点名）。
const ALL_CODES: [H01Code; 6] = [
    H01Code::OVER_QUOTA,
    H01Code::DOUBLE_FREE,
    H01Code::CONSERVATION,
    H01Code::UNKNOWN_HANDLE,
    H01Code::LEDGER_MISMATCH,
    H01Code::BAD_INPUT,
];

/// 预期判据条数。
const EXPECTED_CHECKS: usize = 26;

// ---------------------------------------------------------------------------
// 主判据
// ---------------------------------------------------------------------------

/// H01 域自检入口。
pub fn run_vch01_checks() -> CheckSet {
    let mut s = CheckSet::new("cgpu-membudget");

    // ================= 一、七主题（H01-TOPIC-*） =================

    // TOPIC-1：七主题闭集逐一对拍（判据侧独立字面量）。
    let mut topic_ok = TopicKind::ALL.len() == TOPIC_COUNT && TOPIC_COUNT == 7;
    for (i, t) in TopicKind::ALL.iter().enumerate() {
        if t.label() != EXPECTED_TOPICS[i] {
            topic_ok = false;
        }
    }
    s.add("H01-TOPIC-七主题闭集对拍", topic_ok, "");

    // TOPIC-2：域界常量（F1121 开工、F1280 收官、本单即起点）。
    s.add(
        "H01-TOPIC-域界宣言",
        H_DOMAIN_FIRST == 1121 && H_DOMAIN_LAST == 1280 && H_DOMAIN_FIRST < H_DOMAIN_LAST,
        "",
    );

    // ================= 二、哲学（H01-PHIL-*） =================

    // PHIL-1：三条公理全文冻结对拍（改动任何一个字都红）。
    let phil_ok = PHILOSOPHY_AXIOMS.len() == 3
        && PHILOSOPHY_AXIOMS[0] == EXPECTED_AXIOMS[0]
        && PHILOSOPHY_AXIOMS[1] == EXPECTED_AXIOMS[1]
        && PHILOSOPHY_AXIOMS[2] == EXPECTED_AXIOMS[2];
    s.add("H01-PHIL-三公理字面量冻结", phil_ok, "");

    // PHIL-2：无界即事故的可观察面——超配额必须显性拒绝（哲学落到行为）。
    let mut p = BudgetPool::new(PoolKind::Process, 100);
    let over = p.try_allocate(1, 101);
    let within = p.try_allocate(2, 100);
    s.add(
        "H01-PHIL-无界即事故落到行为",
        over == Err(H01Code::OVER_QUOTA) && within.is_ok(),
        "",
    );

    // ================= 三、三级池（H01-POOL-*） =================

    // POOL-1：三级闭集对拍（判据侧独立用途语义标签）。
    let mut kinds_ok = PoolKind::ALL.len() == 3;
    for (i, k) in PoolKind::ALL.iter().enumerate() {
        if k.index() != i || k.label().is_empty() {
            kinds_ok = false;
        }
    }
    s.add("H01-POOL-三级池闭集对拍", kinds_ok, "");

    // POOL-2：恰好配额边界放行 / +1 拒绝（双向同侧）。
    let mut p2 = BudgetPool::new(PoolKind::System, 1000);
    let edge_ok = p2.try_allocate(1, 1000).is_ok();
    let mut p3 = BudgetPool::new(PoolKind::System, 1000);
    let edge_rej = p3.try_allocate(1, 1001) == Err(H01Code::OVER_QUOTA);
    s.add("H01-POOL-配额恰边界双向", edge_ok && edge_rej, "");

    // POOL-3：零字节申请拒绝（BAD_INPUT 而非 OVER_QUOTA——语义分账）。
    let mut p4 = BudgetPool::new(PoolKind::Process, 100);
    s.add(
        "H01-POOL-零字节申请拒绝",
        p4.try_allocate(1, 0) == Err(H01Code::BAD_INPUT),
        "",
    );

    // POOL-4：重复释放拒绝且账实不变（首释放后 used 精确回退）。
    let h4 = p4.try_allocate(3, 60).unwrap();
    let first = p4.release(h4);
    let second = p4.release(h4);
    s.add(
        "H01-POOL-重复释放拒绝账实不变",
        first == Ok(60)
            && second == Err(H01Code::UNKNOWN_HANDLE)
            && p4.used() == 0,
        "",
    );

    // POOL-5：峰值记账（used 回退后 peak 保留——遥测事实面不被抹）。
    let mut p5 = BudgetPool::new(PoolKind::Process, 1000);
    let a = p5.try_allocate(1, 700).unwrap();
    let _ = p5.release(a);
    let b = p5.try_allocate(1, 500).unwrap();
    let peak_ok = p5.peak() == 700 && p5.used() == 500;
    let _ = p5.release(b);
    s.add("H01-POOL-峰值记账不随回退抹除", peak_ok, "");

    // POOL-6：水位三边界两侧重演（79.9%/80%/95% 用具体字节）。
    let mut w = BudgetPool::new(PoolKind::Process, 1000);
    let l_empty = w.water_level() == WaterLevel::Empty;
    let h1 = w.try_allocate(1, 799).unwrap();
    let l_normal = w.water_level() == WaterLevel::Normal;
    let _ = w.release(h1);
    let h2 = w.try_allocate(1, 800).unwrap();
    let l_high = w.water_level() == WaterLevel::High;
    let _ = w.release(h2);
    let h3 = w.try_allocate(1, 950).unwrap();
    let l_crit = w.water_level() == WaterLevel::Critical;
    s.add(
        "H01-POOL-水位档位边界重演",
        l_empty && l_normal && l_high && l_crit,
        "",
    );

    // POOL-7：水位阈值常量判据侧写死对拍（改阈值必红）。
    s.add(
        "H01-POOL-水位阈值独立对拍",
        WATER_HIGH_PCT == EXP_WATER_HIGH && WATER_CRITICAL_PCT == EXP_WATER_CRITICAL,
        "",
    );

    // POOL-8：在途记录容量上界——满 MAX_ALLOCS 后拒绝（防无界增长）。
    let mut p8 = BudgetPool::new(PoolKind::Reserved, u64::MAX);
    let mut last = Ok(0u64);
    for _ in 0..MAX_ALLOCS {
        last = p8.try_allocate(9, 1);
        if last.is_err() {
            break;
        }
    }
    let filled = last.is_ok() && p8.alloc_count() == MAX_ALLOCS;
    let overflow = p8.try_allocate(9, 1) == Err(H01Code::OVER_QUOTA);
    s.add("H01-POOL-在途容量上界如实拒绝", filled && overflow, "");

    // ================= 四、PoolSet 守恒（H01-SET-*） =================

    // SET-1：Σ==total 恰好边界放行（守恒不误拒合法配置）。
    let ok_exact = PoolSet::new(1000, [600, 300, 100]).is_ok();
    // SET-2：Σ==total+1 拒绝（守恒真实存在且边界精确）。
    let rej_over = match PoolSet::new(1000, [601, 300, 100]) {
        Err(c) => c == H01Code::CONSERVATION,
        Ok(_) => false,
    };
    s.add("H01-SET-守恒恰边界双向", ok_exact && rej_over, "");

    // SET-3：保留池为零拒绝（应急通道必须有底）。
    let rej_no_reserve = match PoolSet::new(1000, [900, 100, 0]) {
        Err(c) => c == H01Code::CONSERVATION,
        Ok(_) => false,
    };
    s.add("H01-SET-保留池必须有底", rej_no_reserve, "");

    // SET-4：账实对账——混合申请/释放后 audit 仍守恒。
    let mut ps = PoolSet::new(1000, [600, 300, 100]).unwrap();
    let x1 = ps.allocate(PoolKind::Process, 1, 300).unwrap();
    let _x2 = ps.allocate(PoolKind::System, 1, 150).unwrap();
    let _x3 = ps.allocate(PoolKind::Reserved, 1, 40).unwrap();
    let _ = ps.release(PoolKind::Process, x1);
    let aud_ok = ps.audit().is_ok();
    // 判据侧独立重算在用量（不调被测 total_used）。
    let exp_used = ps.pool(PoolKind::Process).used()
        + ps.pool(PoolKind::System).used()
        + ps.pool(PoolKind::Reserved).used();
    let exp_ok = exp_used == 150 + 40 && exp_used <= ps.total_vram();
    s.add("H01-SET-账实对账独立重算", aud_ok && exp_ok, "");

    // SET-5：全局在用 ≤ 总显存 + 池路由正确（跨池互不串账）。
    let mut ps2 = PoolSet::new(1000, [200, 200, 100]).unwrap();
    let _ = ps2.allocate(PoolKind::System, 5, 200);
    let cross = ps2.allocate(PoolKind::Process, 5, 201);
    let routed = ps2.pool(PoolKind::System).used() == 200
        && ps2.pool(PoolKind::Process).used() == 0
        && cross == Err(H01Code::OVER_QUOTA);
    s.add("H01-SET-池路由不串账", routed, "");

    // ================= 五、C 域兑现（H01-VOW-*） =================

    // VOW-1：三条款闭集判据侧独立字面量对拍。
    let mut vow_ok = VowClause::ALL.len() == 3;
    for (i, c) in VowClause::ALL.iter().enumerate() {
        if c.label() != EXPECTED_VOW[i] {
            vow_ok = false;
        }
    }
    s.add("H01-VOW-三条款闭集对拍", vow_ok, "");

    // VOW-2：指纹判据侧独立 FNV 对拍（版本参与混入）。
    let vow = MemDimVow::frozen();
    s.add(
        "H01-VOW-指纹独立重算对拍",
        vow.fingerprint() == alt_vow_fingerprint(vow.version) && vow.version == 1,
        "",
    );

    // VOW-3：条款一池侧落地直证——超预算拒绝是可观察行为（非口头）。
    let mut ps3 = PoolSet::new(500, [300, 150, 50]).unwrap();
    let rejected = ps3.allocate(PoolKind::Process, 1, 301) == Err(H01Code::OVER_QUOTA);
    let accepted = ps3.allocate(PoolKind::Process, 1, 300).is_ok();
    s.add(
        "H01-VOW-条款一分配前预算校验落地",
        rejected && accepted,
        "",
    );

    // VOW-4：条款二+三落地直证——高水位联动源存在 + 释放即时归还。
    let mut ps4 = PoolSet::new(1000, [500, 400, 100]).unwrap();
    let y = ps4.allocate(PoolKind::Process, 1, 480).unwrap(); // 96% 危急
    let alarmed = ps4.pool(PoolKind::Process).water_level() == WaterLevel::Critical;
    let returned = ps4.release(PoolKind::Process, y) == Ok(480)
        && ps4.pool(PoolKind::Process).used() == 0;
    s.add(
        "H01-VOW-条款二三告警源与即时归还落地",
        alarmed && returned,
        "",
    );

    // ================= 六、G 域联动（H01-GPU-*） =================

    // GPU-1：三厂商闭集（判据侧独立写死）。
    let vendors = [VendorKind::Nvidia, VendorKind::Amd, VendorKind::Intel];
    let mut vendor_ok = vendors.len() == 3;
    for v in vendors {
        let t = vendor_trait(v);
        // GPU-2：差异表未冻结前一律 Unknown 保守缺省（不假宣称）。
        if t.resizable_bar != TriState::Unknown
            || t.compress != TriState::Unknown
            || t.page_granularity.is_some()
        {
            vendor_ok = false;
        }
    }
    s.add("H01-GPU-三厂商闭集且保守Unknown", vendor_ok, "");

    // GPU-3：三态闭集自身完备（Unknown 不等于 Unsupported——语义分账）。
    s.add(
        "H01-GPU-三态语义分账",
        TriState::Unknown != TriState::Unsupported
            && TriState::Unknown != TriState::Supported
            && TriState::Supported != TriState::Unsupported,
        "",
    );

    // ================= 七、判据自检（H01-META-*） =================

    // META-2：判据容量无截断。
    s.add("H01-META-判据容量无截断", !s.truncated(), "");

    // META-3：码段独占——全部 0x50xx，且 != 0x21/0x2F/0x3D（防自判死）。
    let section_ok = ALL_CODES.iter().all(|c| (c.code() >> 8) == 0x50)
        && ALL_CODES
            .iter()
            .all(|c| c.code() >> 8 != 0x21 && c.code() >> 8 != 0x2F && c.code() >> 8 != 0x3D);
    s.add("H01-META-诊断码段独占", section_ok, "");

    // META-4：码两两互异 + 人话原因非空。
    let mut code_ok = true;
    for i in 0..ALL_CODES.len() {
        for j in 0..ALL_CODES.len() {
            if i != j && ALL_CODES[i].code() == ALL_CODES[j].code() {
                code_ok = false;
            }
        }
    }
    for c in ALL_CODES {
        if c.reason().is_empty() {
            code_ok = false;
        }
    }
    s.add("H01-META-码互异原因非空", code_ok, "");

    // META-5：判据条数对账（放末位：此时 len 应为 28，加自身恰 29）。
    s.add("H01-META-判据条数对账", s.len() + 1 == EXPECTED_CHECKS, "");

    s
}
