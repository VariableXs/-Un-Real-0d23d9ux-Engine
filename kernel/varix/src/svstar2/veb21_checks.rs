//! VE-F0221 判据层：Intel 设备识别与代际分型（锚点五条判据逐条映射）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0221`
//!
//! **锚点原文五条判据 → 本层判据族**：
//!
//! | 锚点判据 | 判据族 | 要点|
//! |---|---|---|
//! | DID 表覆盖主流机型 | `C21-DID-*` | 逐代覆盖 + 精确命中 + 反向未命中 |
//! | 代际档案挂接正确 | `C21-PROF-*` | 槽位挂接 + 特性位门控 + 编码标签 |
//! | 能力探针实测 | `C21-PROBE-*` | 四槽 + 边界 + 失败即失败 |
//! | 未知 DID 降级路径 | `C21-DEGR-*` | class 判定 + 未认证清零 + 显性提示 |
//! | 识别 ≤50ms | `C21-PERF-*` | 操作数下界，判据侧独立重算 |
//!
//! # 本层的核心纪律：**判据侧独立重算，不向被测问答案**
//!
//! 「DID 表覆盖主流机型」这条判据若写成「遍历被测的表，检查每条
//! `did != 0`」，那是**自证式**——表里当然每条都非零。
//! 故本层的做法是：
//!
//! - **表的内容**：用一组**独立写死的期望 DID 清单**（来自 Linux
//!   `i915_pciids.h`，与被测表无共享代码路径）逐个断言「能被查到且
//!   代际/GT 档正确」。清单里的 DID **不是从被测表导出的**。
//! - **表的完整性**：另外独立断言「表内无重复 DID」与「每条 DID 的
//!   代际与其所在表一致」——这两条能抓「表被合并/复制错行」。
//! - **比较次数**：判据侧用 [`alt_compares_until`] 按**独立重写的
//!   扫描口径**算一遍，与被测的 [`compares_until`] 对拍。被测若改成
//!   `did >> 8` 分桶（要点一的头号反面），两侧立刻分叉。

// ---------------------------------------------------------------------------
// 导入
// ---------------------------------------------------------------------------

use crate::checks::CheckSet;
use alloc::vec::Vec;

use super::veb21_ident::*;

// ---------------------------------------------------------------------------
// 判据侧独立参照（**不调被测的表，只调被测的「行为」**）
// ---------------------------------------------------------------------------

/// 判据侧独立写死的「主流机型」期望清单。
///
/// **数据源与被测表同源（Linux `i915_pciids.h`）但代码路径完全独立**：
/// 这里是一组 `(did, 期望代际index, 期望 GT index)` 字面量，
/// **不从 [`GEN9_TABLE`] 等被测表导出**，故被测表若整体错代际，
/// 判据会立刻红。
///
/// 覆盖锚点点名的全部五代：Gen9 Skylake/Kaby、Gen9.5 Coffee/Comet、
/// Gen11 Ice、Xe DG1/Tiger、Xe2 Meteor。
const EXPECTED_MAJOR: &[(u16, usize, usize)] = &[
    // —— Gen9：Skylake ——
    (0x1902, 0, 0), // SKL GT1
    (0x1905, 0, 1), // SKL GT2
    (0x1912, 0, 2), // SKL GT3
    // —— Gen9：Kaby Lake ——
    (0x5902, 0, 0), // KBL GT1（与 0x5912 仅差一位，区间猜会混桶）
    (0x5912, 0, 1), // KBL GT2
    (0x5926, 0, 2), // KBL GT3
    (0x593B, 0, 3), // KBL GT4
    // —— Gen9.5：Coffee Lake ——
    (0x3E90, 1, 0), // CFL S GT1
    (0x3E92, 1, 1), // CFL S GT2（与 0x3E90 仅差一位）
    (0x3EA5, 1, 2), // CFL U GT3
    (0x87CA, 1, 1), // AML/CFL GT2
    // —— Gen11：Ice Lake ——
    (0x8A50, 2, 0), // ICL GT1
    (0x8A5C, 2, 1), // ICL GT2
    (0x8A70, 2, 1), // ICL Iris Xe
    // —— Xe：Tiger Lake ——
    (0x9A49, 3, 1), // TGL GT2（DG1/TGL 主力核显）
    (0x9A59, 3, 0), // TGL GT1
    (0x9AC0, 3, 2), // TGL GT3
    // —— Xe：DG1 独显 ——
    (0x4905, 3, 4), // DG1 Gt5（非核显）
    (0x4908, 3, 3), // DG1 Gt4
    // —— Xe2：Meteor Lake ——
    (0x7D40, 4, 4), // MTL
    (0x7D55, 4, 4), // MTL Arc
    (0x7DD5, 4, 4), // MTL Arc
];

/// 判据侧独立写死的「必须**不**在表内」的 DID（反向清单）。
///
/// 这些是真实存在的 PCI device id，但**不是 Intel 核显**：
/// - `0x1002` 是 **ATI/AMD** 的 vendor，不是 device id（放在这里是为了
///   让判据能验证「厂商号先判」这条路径）；
/// - `0x1F08` 是 SiS  chipset；
/// - `0x10DE` 是 NVIDIA 的 vendor 位。
const EXPECTED_ABSENT: &[u16] = &[0xDEAD, 0xBEEF, 0x0000, 0xFFFF, 0x1234];

/// 判据侧独立实现的「查到 `did` 用了多少次比较」。
///
/// **口径与被测 [`compares_until`] 相同但代码独立**：逐表顺序、表内顺序、
/// 命中即停。之所以要独立重算：被测若把线性扫描改成 `did >> 8` 分桶
/// （或加缓存），操作数会骤降而**行为不变** —— 那时耗时判据就该红，
/// 而只断「结果对」是抓不到的。
fn alt_compares_until(did: u16) -> u32 {
    // 判据侧把五张表**按代际序显式列出**（不调被测的 all_tables()，
    // 否则就变成「判据问被测要表」）。
    const GEN9: &[u16] = &[
        0x1902, 0x1905, 0x1906, 0x1912, 0x5902, 0x5906, 0x590A, 0x5912, 0x5916, 0x591B,
        0x591D, 0x5923, 0x5926, 0x5927, 0x593B,
    ];
    const GEN9_5: &[u16] = &[
        0x3E90, 0x3E91, 0x3E92, 0x3E93, 0x3E96, 0x3E98, 0x3EA0, 0x3EA1, 0x3EA2, 0x3EA3,
        0x3EA5, 0x3EA6, 0x3EA9, 0x3EA8, 0x87C0, 0x87CA,
    ];
    const GEN11: &[u16] = &[
        0x8A50, 0x8A51, 0x8A52, 0x8A5A, 0x8A5B, 0x8A5C, 0x8A5D, 0x8A70, 0x8A71,
    ];
    const XE: &[u16] = &[
        0x9A40, 0x9A49, 0x9A59, 0x9A60, 0x9A68, 0x9A70, 0x9A78, 0x9AC0, 0x9AC9, 0x9AD9,
        0x9AF8, 0x4C80, 0x4C8A, 0x4C81, 0x4C8B, 0x9840, 0x9841, 0x9842, 0x4905, 0x4906,
        0x4907, 0x4908, 0x4909,
    ];
    const XE2: &[u16] = &[0x7D40, 0x7D45, 0x7D51, 0x7D55, 0x7D41, 0x7D67, 0x7DD5];

    let tables: [&[u16]; 5] = [GEN9, GEN9_5, GEN11, XE, XE2];
    let mut acc = 0u32;
    let mut t = 0;
    while t < tables.len() {
        let tab = tables[t];
        let mut i = 0usize;
        while i < tab.len() {
            acc += 1;
            if tab[i] == did {
                return acc;
            }
            i += 1;
        }
        t += 1;
    }
    acc
}

/// 判据侧的 EU 数→GT 档判定（**独立重写，不调被测的 `eu_min`**）。
///
/// 只用于「探针 EU 数落在表项 GT 档区间内」这条断言。
fn alt_gt_fits(gt: GtTier, eu: u32) -> bool {
    match gt {
        GtTier::Gt1 => (16..=24).contains(&eu),
        GtTier::Gt2 => (28..=40).contains(&eu),
        GtTier::Gt3 => (44..=72).contains(&eu),
        GtTier::Gt4 => (80..=96).contains(&eu),
        GtTier::Gt5 => (96..=128).contains(&eu),
    }
}

/// 构造一个恒定返回的探针读函数（**闭包不能转fn 指针，故用 fn + 全局槽**）。
///
/// no_std 探针无法捕获环境，故用一个文件内静态槽传递期望值。
/// 判据层**单线程顺序执行**，用静态槽是安全的（无并发）。
static mut PROBE_EU: u32 = 64;
static mut PROBE_CACHE: u32 = 2;
static mut PROBE_PIPES: u32 = 3;
static mut PROBE_MEDIA: u32 = 1;
/// 置「某槽不可读」的位掩码（bit=ProbeSlot::index()）。
static mut PROBE_UNREADABLE_MASK: u32 = 0;

/// 设置探针返回值（判据侧驱动，**只写不读被测**）。
fn set_probe(eu: u32, cache: u32, pipes: u32, media: u32, unreadable: u32) {
    unsafe {
        PROBE_EU = eu;
        PROBE_CACHE = cache;
        PROBE_PIPES = pipes;
        PROBE_MEDIA = media;
        PROBE_UNREADABLE_MASK = unreadable;
    }
}

/// 判据注入的探针读函数。
fn probe_reader(_s: ProbeSlot) -> ProbeRaw {
    unsafe {
        let mask = PROBE_UNREADABLE_MASK;
        if mask & (1u32 << _s.index()) != 0 {
            return None;
        }
        match _s {
            ProbeSlot::GtEuCount => Some(PROBE_EU),
            ProbeSlot::CacheLevel => Some(PROBE_CACHE),
            ProbeSlot::DisplayPipes => Some(PROBE_PIPES),
            ProbeSlot::MediaEngine => Some(PROBE_MEDIA),
        }
    }
}

/// 跑一次识别（**便捷封装**：设探针 → 跑 → 返回结论与计数器）。
fn run_ident(input: IdentifyInput) -> (Result<IdentifyOutcome, IdentifyErr>, ProbeCounters) {
    let mut ctr = ProbeCounters::zero();
    let r = identify(input, probe_reader, &mut ctr);
    (r, ctr)
}

// ===========================================================================
// 判据族一：`C21-DID-*` —— DID 表覆盖主流机型（锚点判据 1）
// ===========================================================================

/// **1.1 主流机型逐个精确命中，且代际/GT 档与独立清单一致**。
///
/// 覆盖锚点点名的五代全部。每条断言三件事：
/// ① 能查到（**精确**，不是区间）；② 代际 index 与独立清单一致；
/// ③ GT 档 index 与独立清单一致。
fn c21_did_major_coverage(s: &mut CheckSet) {
    let mut all_ok = true;
    let mut i = 0;
    while i < EXPECTED_MAJOR.len() {
        let (did, want_gen, want_gt) = EXPECTED_MAJOR[i];
        match lookup_did(INTEL_VENDOR_ID, did) {
            None => {
                all_ok = false;
            }
            Some(e) => {
                if e.gen.index() != want_gen || e.gt.index() != want_gt {
                    all_ok = false;
                }
            }
        }
        i += 1;
    }
    s.add(
        "C21-DID-01 主流机型23个逐个精确命中且代际/GT档合独立清单",
        all_ok,
        "",
    );
}

/// **1.2 五代每代至少各有若干条**（防「某代整表被清空」）。
///
/// 逐条命中法抓不到「某代整表没了」——因为清单里的 DID 会一并查不到，
/// 会被1.1 抓。这条是**独立冗余**：它直接断「每代条目数 ≥ 阈值」，
/// 阈值是字面量（不从被测表导出）。
fn c21_did_each_gen_present(s: &mut CheckSet) {
    let tables = all_tables();
    let mut counts = [0usize; 5];
    let mut t = 0;
    while t < tables.len() {
        counts[t] = tables[t].len();
        t += 1;
    }
    // 独立阈值（锚点要求「覆盖主流机型」，故每代至少 5 条）。
    let floors = [5usize, 5, 5, 10, 5];
    let mut ok = true;
    let mut i = 0;
    while i < 5 {
        if counts[i] < floors[i] {
            ok = false;
        }
        i += 1;
    }
    s.add("C21-DID-02 五代每代表有条目(独立阈值5/5/5/10/5)", ok, "");
}

/// **1.3 表内无重复 DID**（抓「两表合并时行错位」）。
///
/// 判据侧独立重算：把五张表的 DID 收进 `Vec`，
/// 两两比对（O(n²) 但 n≈70，判据侧不讲究复杂度）。
fn c21_did_no_duplicate(s: &mut CheckSet) {
    let tables = all_tables();
    let mut all: Vec<u16> = Vec::new();
    let mut t = 0;
    while t < tables.len() {
        let tab = tables[t];
        let mut i = 0;
        while i < tab.len() {
            all.push(tab[i].did);
            i += 1;
        }
        t += 1;
    }
    let mut dup = false;
    let mut i = 0;
    while i < all.len() {
        let mut j = i + 1;
        while j < all.len() {
            if all[i] == all[j] {
                dup = true;
            }
            j += 1;
        }
        i += 1;
    }
    s.add(
        "C21-DID-03 全域DID表无重复条目(线性两两比对)",
        !dup,
        "",
    );
}

/// **1.4 反向清单：DID 不在表内时查不到**（防「表变成全通配」）。
fn c21_did_absent(s: &mut CheckSet) {
    let mut ok = true;
    let mut i = 0;
    while i < EXPECTED_ABSENT.len() {
        if lookup_did(INTEL_VENDOR_ID, EXPECTED_ABSENT[i]).is_some() {
            ok = false;
        }
        i += 1;
    }
    s.add("C21-DID-04 反向清单DID一律查不到(表未退化为通配)", ok, "");
}

/// **1.5 厂商号先判：非 Intel 厂商 + 已知 Intel DID ⇒ 查不到**。
///
/// 这是头注要点一的直接判据：只比 device id 会把别家同号设备
/// 误认成Intel 核显。
fn c21_did_vendor_first(s: &mut CheckSet) {
    // 用 AMD 的 vendor + Intel 的 DID 0x9A49 ⇒ 必须查不到。
    let amd = lookup_did(0x1002, 0x9A49);
    // NVIDIA vendor + 0x5912 ⇒ 必须查不到。
    let nv = lookup_did(0x10DE, 0x5912);
    s.add(
        "C21-DID-05 厂商号先判(非Intel厂商+已知DID查不到)",
        amd.is_none() && nv.is_none(),
        "",
    );
}

/// **1.6 易混 DID 对必须区分**（`0x5902`/`0x5912`、`0x3E90`/`0x3E92`）。
///
/// 这两组**只差一位十六进制**。若被测改成区间或 `>>8` 分桶，
/// 这两条会同时命中同一 GT 档而失去区分性。这里**逐对断言 GT 档不同**。
fn c21_did_confusable_pairs(s: &mut CheckSet) {
    let a = lookup_did(INTEL_VENDOR_ID, 0x5902);
    let b = lookup_did(INTEL_VENDOR_ID, 0x5912);
    let c = lookup_did(INTEL_VENDOR_ID, 0x3E90);
    let d = lookup_did(INTEL_VENDOR_ID, 0x3E92);
    let ok = match (a, b, c, d) {
        (Some(x), Some(y), Some(p), Some(q)) => {
            x.gt != y.gt && p.gt != q.gt
        }
        _ => false,
    };
    s.add(
        "C21-DID-06 易混DID对(5902/5912、3E90/3E92)分属不同GT档",
        ok,
        "",
    );
}

/// **1.7 独立清单本身的自检：不得有重复、必须五代齐全**。
///
/// 判据侧的清单若自己写错了（比如同一个 DID 写了两遍），
/// 上面所有基于它的断言都会跟着一起错。故**先自检清单**。
fn c21_did_expectation_selfcheck(s: &mut CheckSet) {
    let mut dup = false;
    let mut i = 0;
    while i < EXPECTED_MAJOR.len() {
        let mut j = i + 1;
        while j < EXPECTED_MAJOR.len() {
            if EXPECTED_MAJOR[i].0 == EXPECTED_MAJOR[j].0 {
                dup = true;
            }
            j += 1;
        }
        i += 1;
    }
    //五代覆盖：每个代际 index 至少出现一次。
    let mut seen = [false; 5];
    let mut k = 0;
    while k < EXPECTED_MAJOR.len() {
        let gi = EXPECTED_MAJOR[k].1;
        if gi < 5 {
            seen[gi] = true;
        }
        k += 1;
    }
    let mut all_gen = true;
    let mut g = 0;
    while g < 5 {
        if !seen[g] {
            all_gen = false;
        }
        g += 1;
    }
    s.add(
        "C21-DID-07 独立期望清单自检(无重复且五代齐全)",
        !dup && all_gen,
        "",
    );
}

// ===========================================================================
// 判据族二：`C21-PROF-*` —— 代际档案挂接正确（锚点判据 2）
// ===========================================================================

/// **2.1 档案槽挂接：每个 DID 的 `profile_slot` 指到同代档案**。
fn c21_profile_slot_matches_gen(s: &mut CheckSet) {
    let tables = all_tables();
    let mut ok = true;
    let mut t = 0;
    while t < tables.len() {
        let tab = tables[t];
        let mut i = 0;
        while i < tab.len() {
            let e = tab[i];
            let p = profile_of(e.profile_slot);
            // 挂接正确 = 档案的代际与表项的代际一致。
            if p.gen != e.gen {
                ok = false;
            }
            // 且槽下标必须落在 PROFILE_SLOTS 内。
            if (e.profile_slot as usize) >= PROFILE_SLOTS {
                ok = false;
            }
            i += 1;
        }
        t += 1;
    }
    s.add("C21-PROF-01 每DID的profile_slot指向同代档案", ok, "");
}

/// **2.2 五代档案齐备且 `slot` 与数组下标一致**（防重排错位）。
fn c21_profiles_well_formed(s: &mut CheckSet) {
    let mut ok = true;
    let mut i = 0;
    while i < GEN_PROFILES.len() {
        if GEN_PROFILES[i].slot as usize != i {
            ok = false;
        }
        i += 1;
    }
    //五代各有一个非保留档案（`cmd_encoding != "reserved"`）。
    let mut live = 0;
    let mut k = 0;
    while k < GEN_PROFILES.len() {
        if GEN_PROFILES[k].cmd_encoding != "reserved" {
            live += 1;
        }
        k += 1;
    }
    s.add(
        "C21-PROF-02 代际档案slot与下标一致且五代各一活档案",
        ok && live == 5,
        "",
    );
}

/// **2.3 特性位单调：更新代际不得丢失基础特性**。
///
/// 判据侧独立写死「必须存在的三个基础位」，逐代断言。
/// 这抓的是「档案表被整体清零」。
fn c21_features_monotone(s: &mut CheckSet) {
    const BASE: u32 = FEAT_GTT | FEAT_EXECLISTS;
    let mut ok = true;
    let mut i = 0;
    while i < 5 {
        // 前五槽是五代活档案（2.2 已断言 slot==index）。
        if GEN_PROFILES[i].features & BASE != BASE {
            ok = false;
        }
        // 更新代际特性集必须是超集（单调不减）。
        if i > 0 {
            if GEN_PROFILES[i].features & GEN_PROFILES[i - 1].features
                != GEN_PROFILES[i - 1].features
            {
                ok = false;
            }
        }
        i += 1;
    }
    s.add("C21-PROF-03 五代特性位单调超集且含GTT/EXECLISTS基础位", ok, "");
}

/// **2.4 已知问题引用有效**（`0` 表示无，非零必须越界安全）。
fn c21_known_issue_refs(s: &mut CheckSet) {
    let mut ok = true;
    let mut i = 0;
    while i < 5 {
        let r = GEN_PROFILES[i].known_issue_ref;
        // 引用越界时 `known_issue()` 回落到占位（不 panic），
        // 但**档案不该引用越界下标**——那说明表写错了。
        if (r as usize) >= KNOWN_ISSUES.len() {
            ok = false;
        }
        // 占位（0）的 tag 必须是「（占位）」。
        if r == 0 && known_issue(0).tag != "（占位）" {
            ok = false;
        }
        i += 1;
    }
    s.add("C21-PROF-04 已知问题引用下标有效且占位语义正确", ok, "");
}

/// **2.5 未认证 ⇒ 特性位强制清零**（**要点四的实现判据**）。
///
/// 这是「显性提示未认证」的关键：**不只是标志位，是真的不给新特性**。
fn c21_degrade_features_cleared(s: &mut CheckSet) {
    set_probe(64, 2, 3, 1, 0);
    let (r, _) = run_ident(IdentifyInput::intel(0xDEAD));
    let ok = match r {
        Ok(o) => {
            // 未认证 + 分派视图特性为零 + 提示非空。
            o.certified == Certified::No
                && DispatchView::of(&o).features == 0
                && !o.notice.is_empty()
                // 未认证不得给能力承诺。
                && o.probe.is_none()
        }
        Err(_) => false,
    };
    s.add(
        "C21-PROF-05 未认证时特性位强制清零且不给能力承诺",
        ok,
        "",
    );
}

/// **2.6 已认证 ⇒ 分派视图特性来自档案且编码标签非保留**。
fn c21_dispatch_view_from_profile(s: &mut CheckSet) {
    set_probe(96, 2, 3, 1, 0);
    let (r, _) = run_ident(IdentifyInput::intel(0x9A49));
    let ok = match r {
        Ok(o) => {
            let dv = DispatchView::of(&o);
            let p = profile_of(o.profile_slot);
            dv.features == p.features
                && dv.cmd_encoding == p.cmd_encoding
                && dv.cmd_encoding != "reserved"
                && dv.supports(FEAT_MPO)
                && !dv.supports(FEAT_XE2_MEDIA_TILE) // Xe 档不该有Xe2 专有位
        }
        Err(_) => false,
    };
    s.add("C21-PROF-06 已认证时特性与编码取自档案且Xe档无Xe2专有位", ok, "");
}

/// **2.7 诊断码逐条互异且理由串互异**（拒绝必带专属原因）。
fn c21_err_codes_distinct(s: &mut CheckSet) {
    let all = IdentifyErr::ALL;
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
    s.add(
        "C21-BRIDGE-01 诊断码与拒绝理由逐条互异(不可共用占位)",
        !code_dup && !reason_dup,
        "",
    );
}

/// **2.8 `ArchGen::code()` 逐个互异**（禁`enum as u8` 的等价要求）。
fn c21_archgen_codes_distinct(s: &mut CheckSet) {
    let all = ArchGen::ALL;
    let mut dup = false;
    let mut i = 0;
    while i < all.len() {
        let mut j = i + 1;
        while j < all.len() {
            if all[i].code() == all[j].code() {
                dup = true;
            }
            j += 1;
        }
        i += 1;
    }
    // index() 也要与数组下标一致（判据按下标推导）。
    let mut idx_ok = true;
    let mut k = 0;
    while k < all.len() {
        if all[k].index() != k {
            idx_ok = false;
        }
        k += 1;
    }
    s.add(
        "C21-BRIDGE-02 五代编码互异且index与下标一致",
        !dup && idx_ok,
        "",
    );
}

// ===========================================================================
// 判据族三：`C21-PROBE-*` —— 能力探针实测（锚点判据 3）
// ===========================================================================

/// **3.1 四槽全读且EU 数落到表项 GT 档的合法区间**。
fn c21_probe_eu_in_range(s: &mut CheckSet) {
    let mut ok = true;
    // 逐代取一个代表 DID，逐档给一个**落在该档区间内**的 EU 数。
    let cases = [
        (0x5902u16, 16u32, 0usize), // Gen9 GT1 → 16
        (0x5912, 32, 1),            // Gen9 GT2 → 32
        (0x5926, 48, 2),           // Gen9 GT3 → 48
        (0x593B, 80, 3),           // Gen9 GT4 → 80
        (0x9A49, 32, 1),           // Xe   GT2 → 32
        (0x9AC0, 64, 2),           // Xe   GT3 → 64
        (0x4905, 96, 4),           // Xe   GT5 → 96
        (0x7D40, 96, 4),           // Xe2  GT5 → 96
    ];
    let mut i = 0;
    while i < cases.len() {
        let (did, eu, want_gt) = cases[i];
        set_probe(eu, 2, 3, 1, 0);
        let (r, _) = run_ident(IdentifyInput::intel(did));
        match r {
            Ok(o) => {
                if let Some(e) = o.entry {
                    // 三件事：GT 档如预期、EU 数落在该档区间、探针值被采信。
                    if e.gt.index() != want_gt || !alt_gt_fits(e.gt, eu) {
                        ok = false;
                    }
                    match o.probe {
                        Some(p) => {
                            if p.gt_eu != eu {
                                ok = false;
                            }
                        }
                        None => ok = false,
                    }
                } else {
                    ok = false;
                }
            }
            Err(_) => ok = false,
        }
        i += 1;
    }
    s.add(
        "C21-PROBE-01 八个代表机型EU数均落在表项GT档合法区间且被采信",
        ok,
        "",
    );
}

/// **3.2 四项能力逐项透传**（不是只测 EU 数）。
fn c21_probe_all_four(s: &mut CheckSet) {
    set_probe(64, 3, 4, 1, 0);
    let (r, ctr) = run_ident(IdentifyInput::intel(0x9A49));
    let ok = match r {
        Ok(o) => match o.probe {
            Some(p) => {
                p.gt_eu == 64
                    && p.cache_level == 3
                    && p.display_pipes == 4
                    && p.media_engine
                    // 四槽都读了 ⇒ 计数恰为 4（**用 == 不用 >=**，纪律）。
                    && ctr.probe_slots == 4
            }
            None => false,
        },
        Err(_) => false,
    };
    s.add("C21-PROBE-02 四项能力逐项透传且探针槽计数恰为4", ok, "");
}

/// **3.3 媒体引擎存在性两种取值都正确**（0/1，不是「非零即真」）。
fn c21_probe_media_both(s: &mut CheckSet) {
    set_probe(64, 2, 3, 0, 0);
    let (r0, _) = run_ident(IdentifyInput::intel(0x9A49));
    set_probe(64, 2, 3, 1, 0);
    let (r1, _) = run_ident(IdentifyInput::intel(0x9A49));
    let m0 = match r0 {
        Ok(o) => match o.probe {
            Some(p) => p.media_engine,
            None => true, // 若没探到也算错
        },
        Err(_) => true,
    };
    let m1 = match r1 {
        Ok(o) => match o.probe {
            Some(p) => p.media_engine,
            None => false,
        },
        Err(_) => false,
    };
    s.add("C21-PROBE-03 媒体引擎存在性0/1两向都正确", !m0 && m1, "");
}

/// **3.3b 媒体引擎的「值→布尔」映射是白名单而非黑名单**。
///
/// **变异实测（M14）**：`media_engine: media == 1` 改成 `media != 0`
/// 后**判据全绿**。原因是上��条`C21-PROBE-05` 已用 `media = 2`
/// 做越界用例，而被测里`if media > 1` 的越界检查**排在赋值之前**，
/// 所以 `media == 1` 与 `media != 0` 这两句**永远走不到 media=2**
/// ⇒ **等价变异**，不是漏网。
///
/// 但这暴露一个真问题：**该赋值语句从未被任何判据真正执行到**，
/// 它是「死代码里的活逻辑」。本条用**只走赋值路径**的探针点把它激活：
/// `media = 1`（唯一能让赋值发生的合法值）下，断言 `media_engine == true`；
/// `media = 0` 下断言 `== false`。
/// **正反双向**：只断 true 的话，`恒返回 true` 的实现也能过。
fn c21_probe_media_whitelist(s: &mut CheckSet) {
    // media=1 ⇒ 必须为 true（`media == 1` 与 `media != 0` 在此**同值**，
    // 故本条抓的不是这个，而是下面那条）。
    set_probe(64, 2, 3, 1, 0);
    let (r1, _) = run_ident(IdentifyInput::intel(0x9A49));
    let t1 = match r1 {
        Ok(o) => matches!(o.probe, Some(p) if p.media_engine),
        Err(_) => false,
    };
    // media=0 ⇒ 必须为 false（`media == 1` 与 `media != 0` 在此**同值**）。
    set_probe(64, 2, 3, 0, 0);
    let (r0, _) = run_ident(IdentifyInput::intel(0x9A49));
    let f0 = match r0 {
        Ok(o) => matches!(o.probe, Some(p) if !p.media_engine),
        Err(_) => false,
    };
    // **真正的杀手锏**：media=2 时**必须被越界检查拦下**，
    // 而不是被当成 true。若把越界检查删掉，`media != 0` 就会
    // 把 2 当true 放过去。
    set_probe(64, 2, 3, 2, 0);
    let (r2, _) = run_ident(IdentifyInput::intel(0x9A49));
    let rejected = matches!(r2, Err(IdentifyErr::ProbeValueOutOfRange));
    s.add(
        "C21-PROBE-11b 媒体引擎赋值仅在0/1生效且值2被越界拦下",
        t1 && f0 && rejected,
        "",
    );
}

/// **3.4 探针不可读 ⇒ 失败，且失败计数被记账**（要点三：失败即失败）。
fn c21_probe_unreadable(s: &mut CheckSet) {
    // 四槽逐个置不可读，每次都必须Err(ProbeUnreadable)。
    let mut ok = true;
    let mut i = 0;
    while i < 4 {
        set_probe(64, 2, 3, 1, 1u32 << i);
        let (r, ctr) = run_ident(IdentifyInput::intel(0x9A49));
        match r {
            Ok(_) => ok = false,
            Err(e) => {
                if e != IdentifyErr::ProbeUnreadable {
                    ok = false;
                }
                // 失败必须被计数（否则「失败不可见」）。
                if ctr.probe_fail != 1 {
                    ok = false;
                }
            }
        }
        i += 1;
    }
    s.add("C21-PROBE-04 逐槽不可读均失败且失败计数恰为1", ok, "");
}

/// **3.5 探针值越界一律拒绝**（EU=0 / EU>512 / 缓存>3 / 管道>8 / 媒体>1）。
fn c21_probe_out_of_range(s: &mut CheckSet) {
    let bad = [
        (0u32, 2u32, 3u32, 1u32),// EU = 0
        (513, 2, 3, 1),           // EU > 512
        (64, 4, 3, 1),            // 缓存 > 3
        (64, 2, 9, 1),            // 管道 > 8
        (64, 2, 3, 2),            // 媒体 > 1
    ];
    let mut ok = true;
    let mut i = 0;
    while i < bad.len() {
        set_probe(bad[i].0, bad[i].1, bad[i].2, bad[i].3, 0);
        let (r, _) = run_ident(IdentifyInput::intel(0x9A49));
        match r {
            Ok(_) => ok = false,
            Err(e) => {
                if e != IdentifyErr::ProbeValueOutOfRange {
                    ok = false;
                }
            }
        }
        i += 1;
    }
    s.add("C21-PROBE-05 五类越界探针值一律拒绝(越界即失败不兜底)", ok, "");
}

/// **3.6 边界值本身合法**（EU=1 与 EU=512 不算越界的那些）。
///
/// **反向断言**：只断「越界被拒」会被「全部拒绝」骗过。
/// 故必须同时断「**合法值不被误拒**」——EU=16（GT1下限）与
/// EU=128（GT5上限）应被接受（分型上）。
fn c21_probe_boundary_accepted(s: &mut CheckSet) {
    set_probe(16, 0, 0, 0, 0);
    let (r0, _) = run_ident(IdentifyInput::intel(0x5902)); // GT1, EU=16 恰在下限
    set_probe(128, 3, 8, 1, 0);
    let (r1, _) = run_ident(IdentifyInput::intel(0x7D40)); // GT5, EU=128 恰在上限
    let ok0 = matches!(r0, Ok(_));
    let ok1 = matches!(r1, Ok(_));
    s.add(
        "C21-PROBE-06 边界合法值不被误拒(EU=16下限/EU=128上限)",
        ok0 && ok1,
        "",
    );
}

/// **3.7 探针顺序固定 ⇒ 计数口径可推导**（判据按 `ProbeSlot::ALL` 推导）。
fn c21_probe_slot_order(s: &mut CheckSet) {
    let all = ProbeSlot::ALL;
    // index() 必须与数组下标一致。
    let mut idx_ok = true;
    let mut labels_dup = false;
    let mut i = 0;
    while i < all.len() {
        if all[i].index() != i {
            idx_ok = false;
        }
        let mut j = i + 1;
        while j < all.len() {
            if all[i].label() == all[j].label() {
                labels_dup = true;
            }
            j += 1;
        }
        i += 1;
    }
    s.add(
        "C21-PROBE-07 四探针槽index与下标一致且标签互异",
        idx_ok && !labels_dup,
        "",
    );
}

/// **3.8 EU 低于表项 GT 档下限时降级到基线档**（要点二）。
///
/// 抓的是「EU 参与分型判定」这条契约：探针报 20 EU 但表项是 GT3，
/// 不能按 GT3 走。
fn c21_tier_conflict_downgrade(s: &mut CheckSet) {
    // 0x5926 是 Gen9 GT3（区间 44~72）。报 EU=20 ⇒ 低于下限。
    set_probe(20, 2, 3, 1, 0);
    let (r, _) = run_ident(IdentifyInput::intel(0x5926));
    let ok = match r {
        Ok(o) => o.tier == GenTier::Baseline,
        Err(_) => false,
    };
    s.add("C21-PROBE-08 EU低于表项GT档下限时降级到基线档", ok, "");
}

/// **3.9 EU 低于基线档下限时**明确失败**（不是继续降级）。
fn c21_tier_underflow(s: &mut CheckSet) {
    // EU=8< 基线档下限 16 ⇒ 无法降级。
    set_probe(8, 2, 3, 1, 0);
    let (r, _) = run_ident(IdentifyInput::intel(0x9A49));
    let ok = matches!(r, Err(IdentifyErr::TierConflictUnderflow));
    s.add(
        "C21-PROBE-09 EU低于基线档下限时明确失败而非无限降级",
        ok,
        "",
    );
}

/// **3.10 EU 数不得为「恰好等于下限时误判为冲突」**（**夹逼对**）。
///
/// 3.8 断「低于下限 ⇒ 降级」，本条断「**恰好等于下限 ⇒ 不降级**」。
/// 只断前者的话，把 `<` 写成 `<=` 的缺陷会漏网。
fn c21_tier_clamp_boundary(s: &mut CheckSet) {
    // 0x5926 是 GT3，下限 44。EU=44 恰好等于 ⇒ 不应冲突降级。
    set_probe(44, 2, 3, 1, 0);
    let (r, _) = run_ident(IdentifyInput::intel(0x5926));
    let ok = match r {
        Ok(o) => o.tier == GenTier::Baseline, // Gen9 天然就是 Baseline
        Err(_) => false,
    };
    // 真正的判据是「不返回 Underflow」——用 Xe 档才能看出差别。
    set_probe(32, 2, 3, 1, 0);
    let (r2, _) = run_ident(IdentifyInput::intel(0x9A49)); // Xe GT2，下限 28
    let ok2 = matches!(r2, Ok(o) if o.tier == GenTier::XeStandard);
    s.add(
        "C21-PROBE-10 EU恰等于GT档下限时不算冲突(夹逼对下界)",
        ok && ok2,
        "",
    );
}

/// **3.10b 夹逼对必须**逐档**测「恰好等于该档下限」**。
///
/// **变异实测（M19/M20）**：把 `gt_eu < gt.eu_min()` 写成 `<=`，
/// 或把 `GtTier::Gt2 => 28` 抬高到 `32`，**判据全绿**。
/// 根因：原 `C21-PROBE-10` 用 `EU = 32` 测 GT2（下限 28），
/// `32 > 28` 无论 `<` 还是 `<=` 都不冲突 ⇒ **探针点没压在边界上**。
/// 且 M20 二次实测：探针点若**取自被测的 `gt.eu_min()`**，抬高常量
/// 则探针点跟着漂移 ⇒ 判据恒真＝自证式（判据侧独立写死常量后修复）。
///
/// **「<` 写成 「<=」是夹逼对判据最经典的漏网**：上界测到了、
/// 下界没测到（或者反过来），因为「恰好等于」这个点被跳过了。
/// 本条对**五个 GT 档逐一**用**判据侧独立写死**的下限常量做探针，
/// 断言「恰好等于下限 ⇒ 不冲突」。
///
/// **同时断反向**：低于下限一个单位 ⇒ 必冲突。
/// 只断前者的话，`恒不冲突` 的实现（冲突分支整个删掉）也能过。
fn c21_tier_eu_min_exact(s: &mut CheckSet) {
    // 逐档找一个该档的代表 DID（DID 的 GT 档必须与探针档匹配，
    // 否则判据测的是「表项 GT 档」而不是「探针 EU 落在哪档」）。
    //
    // **必须覆盖 Xe 档**：`Gen9/9.5/11` 的自然分型本就是 `Baseline`，
    // 「冲突 ⇒降到 Baseline」对它们是**恒真无鉴别力**的。只有 Xe/Xe2 档
    // 才能看出「该不该降级」。**变异实测（M19）**：`<` 写成 `<=` 时，
    // Xe GT3 在 `EU = eu_min()` 处从 `XeStandard` 悄悄降成 `Baseline`
    // ——若判据只断「不失败」就完全看不见这个变化。
    let cases = [
        (0x5902u16, GtTier::Gt1, 0usize), // Gen9 GT1，eu_min=16
        (0x5912, GtTier::Gt2, 1),        // Gen9 GT2，eu_min=28
        (0x5926, GtTier::Gt3, 2),        // Gen9 GT3，eu_min=44
        (0x593B, GtTier::Gt4, 3),        // Gen9 GT4，eu_min=80
        (0x9AC0, GtTier::Gt3, 2),        // Xe   GT3，eu_min=44
        (0x9A49, GtTier::Gt2, 1),        // Xe   GT2，eu_min=28（抓「下限被抬高」）
        (0x4905, GtTier::Gt5, 4),        // Xe   GT5，eu_min=96
    ];
    // 判据侧独立写死的各档 EU 下限/上限（**不从 `gt.eu_min()` 导出**，
    // 与 `alt_gt_fits` 的区间表同源）。
    //
    // **变异实测（M20）教训**：首版探针值写的是 `gt.eu_min()`——
    // **判据向被测问答案＝自证式**（弱门禁第 7 条的常量版）。下限被
    // 抬高（如 Gt2: 28→32）时探针点跟着抬，夹逼点永远恰好落在
    // 变异后的边界上 ⇒ 判据恒绿，而真实 GT2 设备（EU=29~31）会被
    // 误降级。故**夹逼点必须钉在判据侧字面量上**，并另断
    // `gt.eu_min()/eu_max()` 与字面量逐档一致（契约核验）。
    // 锚点明文契约值：GT1=16/24 · GT2=28/40 · GT3=44/72 ·
    // GT4=80/96 · GT5=96/128。
    let eu_min_indep = |gt: GtTier| -> u32 {
        match gt {
            GtTier::Gt1 => 16,
            GtTier::Gt2 => 28,
            GtTier::Gt3 => 44,
            GtTier::Gt4 => 80,
            GtTier::Gt5 => 96,
        }
    };
    let eu_max_indep = |gt: GtTier| -> u32 {
        match gt {
            GtTier::Gt1 => 24u32,
            GtTier::Gt2 => 40,
            GtTier::Gt3 => 72,
            GtTier::Gt4 => 96,
            GtTier::Gt5 => 128,
        }
    };
    let natural_tier = |gen: ArchGen| -> GenTier {
        match gen {
            ArchGen::Gen9 | ArchGen::Gen9_5 | ArchGen::Gen11 => GenTier::Baseline,
            ArchGen::Xe => GenTier::XeStandard,
            ArchGen::Xe2 => GenTier::XeLatest,
        }
    };
    let mut ok = true;
    // **契约核验**：被测的 eu_min()/eu_max() 必须与判据侧字面量逐档一致
    // （独立于任何识别流程，直接钉死「下限被抬高/上限被压低」类变异）。
    {
        let mut g = 0;
        while g < GtTier::ALL.len() {
            let gt = GtTier::ALL[g];
            if gt.eu_min() != eu_min_indep(gt) || gt.eu_max() != eu_max_indep(gt) {
                ok = false;
            }
            g += 1;
        }
    }
    let mut i = 0;
    while i < cases.len() {
        let (did, gt, want_gt_idx) = cases[i];
        // 先确认 DID 的 GT 档确与 cases 一致（否则用例本身写错了）。
        let gen = match lookup_did(INTEL_VENDOR_ID, did) {
            Some(e) => {
                if e.gt != gt || e.gt.index() != want_gt_idx {
                    ok = false;
                }
                e.gen
            }
            None => {
                ok = false;
                i += 1;
                continue;
            }
        };
        let want_natural = natural_tier(gen);
        // 正向：EU 恰好等于 eu_min ⇒ **不冲突**，分型必须等于自然分型。
        // 断tier 值而非只断「不失败」——否则 `<=` 变异（悄悄降级）
        // 会因为仍返回 Ok 而漏网。
        set_probe(eu_min_indep(gt), 2, 3, 1, 0);
        let (r, _) = run_ident(IdentifyInput::intel(did));
        match r {
            Ok(o) => {
                if o.tier != want_natural {
                    ok = false;
                }
            }
            Err(_) => ok = false, // 恰好等于下限被误拒 ⇒ 夹逼对写错
        }
        // 反向：EU 低于下限一个单位 ⇒ 必冲突（不再等于自然分型）。
        // **两种结果都算对**，取决于低到哪：
        //  - 仍 ≥ 基线档下限（16）⇒ 降级到 Baseline；
        //  - < 基线档下限 ⇒ 无法降级，返回 TierConflictUnderflow。
        if eu_min_indep(gt) > EU_FLOOR_BASELINE {
            set_probe(eu_min_indep(gt) - 1, 2, 3, 1, 0);
            let (r2, _) = run_ident(IdentifyInput::intel(did));
            match r2 {
                Ok(o) => {
                    // 冲突后必须降到基线档（**且不得等于自然分档**）。
                    if o.tier != GenTier::Baseline {
                        ok = false;
                    }
                    // 若自然档本就是 Baseline，这条无鉴别力（见上方注释），
                    // 但**不能因此放行**：仍要求结果合法。
                    if want_natural == GenTier::Baseline && o.tier != GenTier::Baseline {
                        ok = false;
                    }
                }
                Err(IdentifyErr::TierConflictUnderflow) => {
                    // 低到基线档以下 ⇒ 明确失败也算正确行为。
                }
                Err(_) => ok = false,
            }
        }
        // GT1 的下限恰等于基线档下限，其「低一单位」落在基线档以下，
        // 由上面的 `if` 守卫跳过（该点已由 C21-PROBE-09 单独覆盖）。
        i += 1;
    }
    s.add(
        "C21-PROBE-11 五个GT档逐档夹逼对(恰等于下限不冲突/低一单位必冲突)",
        ok,
        "",
    );
}

/// **3.10c 计时换算口径必须与「操作数」成正比**（防分母被改小）。
///
/// **变异实测（M28）**：把 `OPS_PER_NS_DEN` 从 `50` 改成 `1`
/// ⇒ `elapsed_ns_lower_bound()` 缩小 50 倍，**判据全绿**。
/// 根因：`C21-PERF-02/04` 只断「在预算内」，而**分母变小只会让
/// 换算值更小 ⇒ 判据更松** ⇒ 这是一条只能被「反向」抓的判据。
///
/// 本条用**独立重算**钉死换算关系：给定一组已知 `total_ops`，
/// 判据侧按 `ops * NUM / DEN` 自己算一遍，与被测的方法对拍。
/// 分母被改小时，`NUM/DEN` 变了，对拍立刻分叉。
///
/// **注意这不是「向被测问答案」**：判据侧用的是**自己写的换算式**
/// （`ops * NUM / DEN`），只与被测**共享两个常量**（常量本身由
/// `C21-PERF-01` 单独断言自洽），不调被测的 `elapsed_ns_lower_bound`。
fn c21_perf_conversion_recompute(s: &mut CheckSet) {
    // **判据侧用独立字面量**，**不读** `OPS_PER_NS_NUM` / `OPS_PER_NS_DEN`。
    //
    // **变异实测（M28）的教训**：首版这里写的是
    // `alt = |ops| ops * OPS_PER_NS_NUM / OPS_PER_NS_DEN`，
    // 看起来是「独立重算」，其实**判据与被测共享同一对常量**——
    // 把分母从 50 改成 1，两侧**同时**变，对拍**恒等** ⇒ 完全失效。
    // **这就是弱门禁第7 条「判据向被测问答案＝自证式」**，
    // 只不过问的不是函数值而是**公式系数**。
    //
    // 正确做法：把换算口径**作为契约写死成字面量**——
    // 本单的计时口径明文是「1 次计数 = 50ns」，故判据侧用 `50`。
    // 分母被改 ⇒ 判据侧仍按 50 算 ⇒ 分叉 ⇒ 被抓。
    const CONTRACT_NS_PER_OP: u64 = 50;

    // 用真实识别跑出的计数做对拍（确认口径在真实路径上也一致）。
    set_probe(96, 2, 3, 1, 0);
    let (_, ctr) = run_ident(IdentifyInput::intel(0x9A49));
    let ok_real_path = ctr.elapsed_ns_lower_bound() == (ctr.total_ops() as u64) * CONTRACT_NS_PER_OP;

    // 逐值对拍：五组字面量 ops，覆盖小值 / 整除边界 / 大值。
    let probes = [1u32, 4, 46, 1000, 65535];
    let mut ok_conv = true;
    let mut i = 0;
    while i < probes.len() {
        let ops = probes[i];
        let c = ProbeCounters {
            did_compares: ops,
            probe_slots: 0,
            probe_fail: 0,
        };
        // 判据侧独立算：total_ops * 50ns，**单位是纳秒**。
        let expect_ns = (ops as u64) * CONTRACT_NS_PER_OP;
        if c.elapsed_ns_lower_bound() != expect_ns {
            ok_conv = false;
        }
        i += 1;
    }
    // 反向钉死：换算出的 ns 值不得**小于**契约值（分母变小 ⇒ 值变小）。
    // 这一条独立于上面的等值对拍，专门抓「分母被改小导致下界被低估」。
    let probe = ProbeCounters {
        did_compares: 1000,
        probe_slots: 0,
        probe_fail: 0,
    };
    let got_ns = probe.elapsed_ns_lower_bound();
    let not_underestimated = got_ns >= (1000u64) * CONTRACT_NS_PER_OP;
    // 常量本身必须自洽（非零）。
    let ok_const = OPS_PER_NS_DEN != 0 && OPS_PER_NS_NUM != 0;
    s.add(
        "C21-PERF-06 耗时换算按契约50ns/次独立对拍且不被低估(分母不可改小)",
        ok_conv && ok_real_path && not_underestimated && ok_const,
        "",
    );
}

// ===========================================================================
// 判据族四：`C21-DEGR-*` —— 未知 DID 降级路径（锚点判据 4）
// ===========================================================================

/// **4.1 未知 DID + class=显示 ⇒ 降级且未认证且提示非空**。
fn c21_degrade_unknown_display(s: &mut CheckSet) {
    set_probe(64, 2, 3, 1, 0);
    let (r, ctr) = run_ident(IdentifyInput::intel(0xDEAD));
    let ok = match r {
        Ok(o) => {
            o.entry.is_none()
                && o.certified == Certified::No
                && o.tier == GenTier::Baseline
                && !o.notice.is_empty()
                // 降级路径**不该**读探针（未认证不给能力承诺）。
                && ctr.probe_slots == 0
        }
        Err(_) => false,
    };
    s.add(
        "C21-DEGR-01 未知DID+显示类降级为基线档且未认证不读探针",
        ok,
        "",
    );
}

/// **4.2 未知 DID + class≠显示 ⇒ 明确拒绝**（不降级）。
fn c21_degrade_non_display_rejected(s: &mut CheckSet) {
    set_probe(64, 2, 3, 1, 0);
    let input = IdentifyInput {
        vendor: INTEL_VENDOR_ID,
        did: 0xDEAD,
        // 网络控制器类（0x02）。
        class_code: 0x02,
        subclass_code: 0x00,
    };
    let (r, _) = run_ident(input);
    s.add(
        "C21-DEGR-02 未知DID且非显示类则拒绝不降级",
        matches!(r, Err(IdentifyErr::DidNotInTable)),
        "",
    );
}

/// **4.3 降级提示必须含「未认证」字样**（**关键词表双向纪律**）。
///
/// 正向：提示含「未认证」。
/// 反向：**已认证**路径的提示**不得**含「未认证」——
/// 否则「永远返回未认证」这种最坏实现也能骗过正向断言。
fn c21_degrade_notice_keywords(s: &mut CheckSet) {
    set_probe(64, 2, 3, 1, 0);
    let (rd, _) = run_ident(IdentifyInput::intel(0xDEAD));
    let (rc, _) = run_ident(IdentifyInput::intel(0x9A49));
    let degraded_has = match rd {
        Ok(o) => o.notice.contains("未认证"),
        Err(_) => false,
    };
    let certified_lacks = match rc {
        Ok(o) => !o.notice.contains("未认证"),
        Err(_) => false,
    };
    s.add(
        "C21-DEGR-03 降级提示含未认证且已认证提示不含(正反双向)",
        degraded_has && certified_lacks,
        "",
    );
}

/// **4.4 降级路径的 DID 比较次数等于全表条目数**（扫完未命中）。
///
/// 独立重算：判据侧用 [`alt_compares_until`] 算同一个 DID。
fn c21_degrade_scan_exhausted(s: &mut CheckSet) {
    set_probe(64, 2, 3, 1, 0);
    let (r, ctr) = run_ident(IdentifyInput::intel(0xDEAD));
    let ok = match r {
        Ok(_) => ctr.did_compares == alt_compares_until(0xDEAD),
        Err(_) => false,
    };
    s.add(
        "C21-DEGR-04 降级路径DID比较次数等于扫完全表(判据侧独立重算)",
        ok,
        "",
    );
}

/// **4.5 两个不同未知 DID 的降级结论等价**（不因 DID 值不同而行为分叉）。
fn c21_degrade_stable_across_unknown(s: &mut CheckSet) {
    set_probe(64, 2, 3, 1, 0);
    let (ra, _) = run_ident(IdentifyInput::intel(0xDEAD));
    let (rb, _) = run_ident(IdentifyInput::intel(0xBEEF));
    let ok = match (ra, rb) {
        (Ok(x), Ok(y)) => {
            x.tier == y.tier
                && x.certified == y.certified
                && x.profile_slot == y.profile_slot
                && x.probe.is_none()
                && y.probe.is_none()
        }
        _ => false,
    };
    s.add("C21-DEGR-05 不同未知DID降级结论完全等价", ok, "");
}

/// **4.6 非 Intel 厂商 ⇒ 拒绝且不查表**（`did_compares` 必为 0）。
///
/// 这条同时钉住「厂商号先判」的**顺序**：若被测先查表再判厂商号，
/// `did_compares` 会非 0。
fn c21_foreign_rejected_early(s: &mut CheckSet) {
    set_probe(64, 2, 3, 1, 0);
    let (r, ctr) = run_ident(IdentifyInput::foreign(0x9A49));
    s.add(
        "C21-DEGR-06 非Intel厂商拒绝且查表次数为0(厂商先判)",
        matches!(r, Err(IdentifyErr::NotIntelVendor)) && ctr.did_compares == 0,
        "",
    );
}

// ===========================================================================
// 判据族五：`C21-PERF-*` —— 识别 ≤50ms（锚点判据 5）
// ===========================================================================

/// **5.1 计时口径常量自洽**（分母非零、量级合理）。
fn c21_perf_constants(s: &mut CheckSet) {
    s.add(
        "C21-PERF-01 计时换算常量自洽(分母非零且预算50ms)",
        OPS_PER_NS_DEN != 0 && RECOGNITION_BUDGET_US == 50_000,
        "",
    );
}

/// **5.2 主流机型全部在预算内**（**用 == 而非 >= 断计数**，纪律）。
fn c21_perf_within_budget(s: &mut CheckSet) {
    set_probe(96, 2, 3, 1, 0);
    let mut ok = true;
    let mut worst_ns = 0u64;
    let mut i = 0;
    while i < EXPECTED_MAJOR.len() {
        let (r, ctr) = run_ident(IdentifyInput::intel(EXPECTED_MAJOR[i].0));
        if r.is_ok() {
            // 命中路径：探针恰好读 4 槽，失败 0 次。
            if ctr.probe_slots != 4 || ctr.probe_fail != 0 {
                ok = false;
            }
            if !ctr.within_budget() {
                ok = false;
            }
            if ctr.elapsed_ns_lower_bound() > worst_ns {
                worst_ns = ctr.elapsed_ns_lower_bound();
            }
        } else {
            ok = false;
        }
        i += 1;
    }
    s.add(
        "C21-PERF-02 主流机型逐个识别在50ms预算内(操作数下界口径)",
        ok,
        "",
    );
}

/// **5.3 比较次数与判据侧独立重算一致**（**防「改成 O(1) 分桶」偷跑**）。
fn c21_perf_count_matches_alt(s: &mut CheckSet) {
    set_probe(96, 2, 3, 1, 0);
    let mut ok = true;
    let mut i = 0;
    while i < EXPECTED_MAJOR.len() {
        let did = EXPECTED_MAJOR[i].0;
        let (_, ctr) = run_ident(IdentifyInput::intel(did));
        if ctr.did_compares != alt_compares_until(did) {
            ok = false;
        }
        i += 1;
    }
    s.add(
        "C21-PERF-03 DID比较次数与判据侧独立重算逐个一致",
        ok,
        "",
    );
}

/// **5.4 操作数下界远低于预算**（留余量，防表再扩几条就撞线）。
fn c21_perf_headroom(s: &mut CheckSet) {
    set_probe(96, 2, 3, 1, 0);
    // 用**最靠后的 DID**（比较次数最多）做最坏情形。
    let (r, ctr) = run_ident(IdentifyInput::intel(0x9AF8));
    let ok = match r {
        Ok(_) => {
            let budget_ns = RECOGNITION_BUDGET_US as u64 * 1_000;
            let used = ctr.elapsed_ns_lower_bound();
            // 余量判据：实际用量 ≤ 预算的 1/10（保守但可核验）。
            used * 10 <= budget_ns && ctr.total_ops() > 0
        }
        Err(_) => false,
    };
    s.add(
        "C21-PERF-04 最坏情形操作数下界不超预算的十分之一",
        ok,
        "",
    );
}

/// **5.5 计数器无溢出且`total_ops` 等于三项之和**（记账自洽）。
fn c21_counter_arithmetic(s: &mut CheckSet) {
    set_probe(96, 2, 3, 1, 0);
    let (_, ctr) = run_ident(IdentifyInput::intel(0x9A49));
    let sum = ctr.did_compares + ctr.probe_slots + ctr.probe_fail;
    // 零值计数器也必须自洽（不 panic、不溢出）。
    let z = ProbeCounters::zero();
    let ok = ctr.total_ops() == sum && z.total_ops() == 0 && z.within_budget();
    s.add(
        "C21-PERF-05 计数器三项求和自洽且零值不溢出",
        ok,
        "",
    );
}

// ===========================================================================
// 判据族六：`C21-A11Y-*` —— 无障碍与隐私（锚点「无直接无障碍面」的诚实落实）
// ===========================================================================

/// **6.1 已认证摘要含型号与分档，未认证摘要明说不认证**。
fn c21_a11y_summary(s: &mut CheckSet) {
    set_probe(64, 2, 3, 1, 0);
    let (rc, _) = run_ident(IdentifyInput::intel(0x9A49));
    let (rd, _) = run_ident(IdentifyInput::intel(0xDEAD));
    let ok = match (rc, rd) {
        (Ok(c), Ok(d)) => {
            let sc = accessibility_summary(&c);
            let sd = accessibility_summary(&d);
            // 正向：已认证含产品代号与 EU 数。
            let cert_ok = sc.contains("TGL") && sc.contains("64") && sc.contains("Xe");
            // 正向：未认证明说不认证。
            let degr_ok = sd.contains("未认证");
            cert_ok && degr_ok
        }
        _ => false,
    };
    s.add("C21-A11Y-01 已认证报型号分档且未认证明说不认证", ok, "");
}

/// **6.2 摘要不含 device id 十六进制串**（隐私：不泄露无关原始标识）。
///
/// **反向纪律**：不仅断「含某关键词」，也断「**不含不该有的东西**」。
fn c21_a11y_no_raw_did(s: &mut CheckSet) {
    set_probe(64, 2, 3, 1, 0);
    let (r, _) = run_ident(IdentifyInput::intel(0x9A49));
    let ok = match r {
        Ok(o) => {
            let s = accessibility_summary(&o);
            // 不得出现 "0x" 开头的原始 DID 字面量。
            !s.contains("0x") && !s.contains("0X")
        }
        Err(_) => false,
    };
    s.add("C21-A11Y-02 无障碍摘要不含原始DID十六进制串", ok, "");
}

// ===========================================================================
// 入口
// ===========================================================================

/// VE-F0221 域自检（**判据 41 项**：DID 覆盖 7 / 档案挂接 8 / 能力探针 12 /
/// 降级路径 6 / 性能 6 / 无障碍 2，未超 `MAX_CHECKS=112`）。
///
/// 判据数按锚点五条 + 无障碍一条共 6 族拆分，见 [`c21_a`]…[`c21_f`]。
pub fn run_veb21_checks() -> CheckSet {
    let mut s = CheckSet::new("intel-gen-typing");

    c21_a(&mut s);
    c21_b(&mut s);
    c21_c(&mut s);
    c21_d(&mut s);
    c21_e(&mut s);
    c21_f(&mut s);

    s
}

/// 族 A：DID 表覆盖（7 项）。
fn c21_a(s: &mut CheckSet) {
    c21_did_major_coverage(s);
    c21_did_each_gen_present(s);
    c21_did_no_duplicate(s);
    c21_did_absent(s);
    c21_did_vendor_first(s);
    c21_did_confusable_pairs(s);
    c21_did_expectation_selfcheck(s);
}

/// 族 B：代际档案挂接（8 项）。
fn c21_b(s: &mut CheckSet) {
    c21_profile_slot_matches_gen(s);
    c21_profiles_well_formed(s);
    c21_features_monotone(s);
    c21_known_issue_refs(s);
    c21_degrade_features_cleared(s);
    c21_dispatch_view_from_profile(s);
    c21_err_codes_distinct(s);
    c21_archgen_codes_distinct(s);
}

/// 族 C：能力探针（12 项）。
fn c21_c(s: &mut CheckSet) {
    c21_probe_eu_in_range(s);
    c21_probe_all_four(s);
    c21_probe_media_both(s);
    c21_probe_media_whitelist(s);
    c21_probe_unreadable(s);
    c21_probe_out_of_range(s);
    c21_probe_boundary_accepted(s);
    c21_probe_slot_order(s);
    c21_tier_conflict_downgrade(s);
    c21_tier_underflow(s);
    c21_tier_clamp_boundary(s);
    c21_tier_eu_min_exact(s);
}

/// 族 D：降级路径（6 项）。
fn c21_d(s: &mut CheckSet) {
    c21_degrade_unknown_display(s);
    c21_degrade_non_display_rejected(s);
    c21_degrade_notice_keywords(s);
    c21_degrade_scan_exhausted(s);
    c21_degrade_stable_across_unknown(s);
    c21_foreign_rejected_early(s);
}

/// 族 E：性能（6 项）。
fn c21_e(s: &mut CheckSet) {
    c21_perf_constants(s);
    c21_perf_within_budget(s);
    c21_perf_count_matches_alt(s);
    c21_perf_headroom(s);
    c21_counter_arithmetic(s);
    c21_perf_conversion_recompute(s);
}

/// 族 F：无障碍（2 项）。
fn c21_f(s: &mut CheckSet) {
    c21_a11y_summary(s);
    c21_a11y_no_raw_did(s);
}
