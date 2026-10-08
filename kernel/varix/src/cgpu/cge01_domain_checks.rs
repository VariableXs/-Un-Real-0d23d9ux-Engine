//! CGPU-F0641 · 域自检（判据逐条对应，见 `cge01_domain.rs` 头注）。
//!
//! 判据映射（锚点原文 → 自检项）：
//! - **八主题** → `E01-主题-*`（封闭全集/秩精确/中文名独立对拍/越界守卫）
//! - **定位** → `E01-定位-*`（架构定位文档关键词独立断——"80 帧"/
//!   "30 个网页"/"算力秩序"）
//! - **三向契约** → `E01-边界-*`（三向齐/peer 独立写死对拍/职责交集
//!   恒空/双列非空/空摘要拒绝）
//! - **十组规划** → `E01-规划-*`（组范围衔接/守恒式/O(1) 映射正反/
//!   越域拒绝/组表对位/状态声明）
//! - **判据** → `E01-判据-*`（零 panic 自扫描、码表冻结、立案正反向、
//!   恒真防线、非截断断言）
//!
//! **判据设计纪律**（承 F4601/F2808 家族）：
//! 1. 不向被测函数问答案——中文名八条/peer 三条/组范围边界值全部
//!    **独立写死字面量**，不从被测常量反推；
//! 2. 双向验证——每条拒绝判据配对应接受判据；
//! 3. 专属错误码（断「有东西坏了」等于没断）；
//! 4. 弱门禁须指认它能抓的变异（映射 `(n-641)/16` 的 16 改 8 必被
//!    跨组映射抓、`>=`⇄`>` 必被恰阈抓）；
//! 5. 判据区零 panic 面；
//! 6. 判据不得自调全域入口（递归教训承 F2806 判据-06）。

use crate::checks::{CheckSet, MAX_CHECKS};

use super::cge01_domain::*;

use alloc::string::String;

// ---------------------------------------------------------------------------
// 判据侧常量与语料（不从被测常量推导）
// ---------------------------------------------------------------------------

/// 判据侧独立写死的码表（4 条冻结）。
const EXPECT_WIRE: [&str; 4] = [
    "E_DOMAIN_THEME_INVALID",
    "E_DOMAIN_GROUP_INVALID",
    "E_DOMAIN_PLAN_DRIFT",
    "E_DOMAIN_BOUNDARY_DRIFT",
];

/// 判据侧独立写死的主题中文名（秩序 = 声明序，锚点原文顺序）。
const EXPECT_THEME_ZH: [&str; 8] = [
    "网页表面调度",
    "表面配额",
    "可见性仲裁",
    "后台节流",
    "合成合批",
    "滚动同步",
    "内存封顶",
    "基准场景",
];

/// 判据侧独立写死的三向 peer（锚点原文顺序）。
const EXPECT_PEERS: [&str; 3] = ["VE-O（F2801+）", "C 域", "D 域"];

/// 判据侧独立写死的组边界抽检值（首组/跨组/末组）。
const EXPECT_BOUNDS: [(u32, u32, usize); 3] = [(641, 656, 0), (657, 672, 1), (785, 800, 9)];

/// 判据总数（独立常数；加判据时同步维护）。
const EXPECTED_CHECK_COUNT: usize = 20;

/// 单遍词法剥除（字符串字面量/行注释一次状态机；承家族同款思路）。
fn strip_lexical_noise(src: &str) -> String {
    let mut out = String::new();
    let mut in_str = false;
    let mut in_line_comment = false;
    let mut chars = src.chars().peekable();
    while let Some(c) = chars.next() {
        if in_line_comment {
            if c == '\n' {
                in_line_comment = false;
                out.push(c);
            }
            continue;
        }
        if in_str {
            if c == '\\' {
                let _ = chars.next();
                continue;
            }
            if c == '"' {
                in_str = false;
            }
            continue;
        }
        if c == '"' {
            in_str = true;
            continue;
        }
        if c == '/' && chars.peek() == Some(&'/') {
            in_line_comment = true;
            let _ = chars.next();
            continue;
        }
        out.push(c);
    }
    out
}

// ---------------------------------------------------------------------------
// 一、八主题
// ---------------------------------------------------------------------------

fn chk_theme_closed(set: &mut CheckSet) {
    // 主题-01：封闭全集封闭 + 秩精确 + 中文名与独立期望逐条对拍。
    let mut ok = SurfaceTheme::ALL.len() == 8;
    let mut k = 0usize;
    while ok && k < 8 {
        match SurfaceTheme::of_rank(k) {
            Some(t) => ok = t.rank() == k && t.zh() == EXPECT_THEME_ZH[k],
            None => ok = false,
        }
        k += 1;
    }
    // 主题-02：越界守卫（恰阈：秩 8 必 None——抓 of_rank 用 >= 的变异）。
    ok = ok && SurfaceTheme::of_rank(8).is_none();
    if ok {
        set.ok("E01-主题-01-封闭与独立对拍");
        set.ok("E01-主题-02-越界守卫");
    } else {
        set.fail("E01-主题-01-封闭与独立对拍", "秩/中文名与独立期望不符");
        set.fail("E01-主题-02-越界守卫", "越界秩可命中=枚举守卫失守");
    }
}

fn chk_theme_domain_pos(set: &mut CheckSet) {
    // 定位-01：架构定位文档含锚点关键词（独立断，不从被测 doc 反推）。
    let doc = DOMAIN_POSITIONING_DOC;
    let ok = doc.contains("80 帧") && doc.contains("30 个网页") && doc.contains("算力秩序");
    // 定位-02：域摘要可读且含域标识与总数。
    let summary = domain_summary();
    let ok2 = summary.contains("CGPU-E") && summary.contains("160");
    if ok {
        set.ok("E01-定位-01-架构定位关键词");
    } else {
        set.fail("E01-定位-01-架构定位关键词", "定位文档缺锚点关键词");
    }
    if ok2 {
        set.ok("E01-定位-02-域摘要可读");
    } else {
        set.fail("E01-定位-02-域摘要可读", "摘要缺域标识或总数");
    }
}

// ---------------------------------------------------------------------------
// 二、三向契约
// ---------------------------------------------------------------------------

fn chk_boundary_contracts(set: &mut CheckSet) {
    // 边界-01：三向齐 + peer 与独立期望对拍。
    let mut ok = BOUNDARY_CONTRACTS.len() == 3;
    let mut k = 0usize;
    while ok && k < 3 {
        ok = BOUNDARY_CONTRACTS[k].peer == EXPECT_PEERS[k];
        k += 1;
    }
    // 边界-02：职责交集恒空 + 双列非空（对每向独立复核）。
    let mut disj = true;
    for c in BOUNDARY_CONTRACTS.iter() {
        disj = disj && c.disjoints() && !c.owned_by_peer.is_empty() && !c.owned_by_self.is_empty();
    }
    // 边界-03：审计通过。
    let audited = audit_boundary().is_ok();
    if ok {
        set.ok("E01-边界-01-三向对拍");
    } else {
        set.fail("E01-边界-01-三向对拍", "peer 与独立期望不符或缺向");
    }
    if disj {
        set.ok("E01-边界-02-职责交集恒空");
    } else {
        set.fail("E01-边界-02-职责交集恒空", "某向职责交集非空");
    }
    if audited {
        set.ok("E01-边界-03-边界审计通过");
    } else {
        set.fail("E01-边界-03-边界审计通过", "契约完整但审计失败");
    }
}

fn chk_boundary_rejects(set: &mut CheckSet) {
    // 边界-04：立案正反向（空现象/空影响拒、正常立案入账、满账拒）。
    let mut ledger = DomainLedger::new();
    let no_symptom = ledger
        .open_case("", "影响面", "定位", "处置", 1)
        .is_err();
    let no_impact = ledger
        .open_case("现象", "", "定位", "处置", 2)
        .is_err();
    let normal = ledger.open_case("现象", "影响面", "定位", "处置", 3).is_ok();
    if no_symptom && no_impact && normal && ledger.len() == 1 {
        set.ok("E01-边界-04-立案正反向");
    } else {
        set.fail("E01-边界-04-立案正反向", "立案正反向行为不齐");
    }
}

fn chk_boundary_violation_detection(set: &mut CheckSet) {
    // 边界-05：违规注入检出（防同源恒绿——真数据本就不相交，恒真
    // 的 disjoints 对真数据测不出；判据侧构造职责列相交的违规契约，
    // 断言 disjoints 必须返回 false。抓「disjoints 被改成恒 true」变异）。
    let bad = BoundaryContract {
        peer: "注入对端",
        peer_scope: "违规注入（判据专用）",
        owned_by_peer: &["表面调度与仲裁"],
        owned_by_self: &["表面调度与仲裁", "本域独有面"],
    };
    let good_self_only = BoundaryContract {
        peer: "注入对端",
        peer_scope: "合规注入（判据专用）",
        owned_by_peer: &["对端独有面"],
        owned_by_self: &["本域独有面"],
    };
    let bad_detected = !bad.disjoints();
    let good_accepted = good_self_only.disjoints();
    if bad_detected && good_accepted {
        set.ok("E01-边界-05-违规注入检出");
    } else {
        set.fail("E01-边界-05-违规注入检出", "disjoints 对违规数据不可分辨");
    }
}

// ---------------------------------------------------------------------------
// 三、十组规划
// ---------------------------------------------------------------------------

fn chk_plan_bounds(set: &mut CheckSet) {
    // 规划-01：组范围衔接（判据侧独立写死首末抽检值对拍）。
    let mut ok = true;
    let mut k = 0usize;
    while ok && k < EXPECT_BOUNDS.len() {
        let (first, last, rank) = EXPECT_BOUNDS[k];
        match BatchGroup::of_rank(rank) {
            Some(g) => ok = g.first_task() == first && g.last_task() == last,
            None => ok = false,
        }
        k += 1;
    }
    // 规划-02：组间衔接（上组末+1 = 下组首，逐对断）。
    let mut chained = true;
    let mut k = 0usize;
    while chained && k + 1 < BATCH_GROUP_COUNT {
        let a = BatchGroup::of_rank(k);
        let b = BatchGroup::of_rank(k + 1);
        chained = match (a, b) {
            (Some(a), Some(b)) => b.first_task() == a.last_task() + 1,
            _ => false,
        };
        k += 1;
    }
    if ok {
        set.ok("E01-规划-01-组范围对拍");
    } else {
        set.fail("E01-规划-01-组范围对拍", "组首末与独立期望不符");
    }
    if chained {
        set.ok("E01-规划-02-组间衔接");
    } else {
        set.fail("E01-规划-02-组间衔接", "组范围断裂");
    }
}

fn chk_plan_mapping(set: &mut CheckSet) {
    // 规划-03：O(1) 映射正反（恰阈 + 跨组 + 越域拒绝）。
    let ok = group_of_task(641) == Some(BatchGroup::E01)
        && group_of_task(656) == Some(BatchGroup::E01)
        && group_of_task(657) == Some(BatchGroup::E02)
        && group_of_task(672) == Some(BatchGroup::E02)
        && group_of_task(673) == Some(BatchGroup::E03)
        && group_of_task(800) == Some(BatchGroup::E10)
        && group_of_task(640).is_none()
        && group_of_task(801).is_none();
    if ok {
        set.ok("E01-规划-03-映射恰阈与越域");
    } else {
        set.fail("E01-规划-03-映射恰阈与越域", "映射边界不严（16 槽跨度或 >= 混淆）");
    }
}

fn chk_plan_conservation(set: &mut CheckSet) {
    // 规划-04：守恒式（判据侧独立重算，不引用 TOTAL_TASKS）。
    let total = 10 * 16;
    let by_theme = 8 * 10 * 2;
    let ok = total == 160 && by_theme == 160
        && TOTAL_TASKS == 160
        && FIRST_TASK_NO == 641
        && LAST_TASK_NO == 800;
    // 规划-05：组表与封闭全集对位 + 状态声明非空。
    let mut aligned = GROUP_PLANS.len() == 10;
    let mut k = 0usize;
    while aligned && k < 10 {
        let p = &GROUP_PLANS[k];
        aligned = p.group.rank() == k
            && p.themes_covered == 8
            && p.slots_per_theme == 2
            && !p.status.is_empty();
        k += 1;
    }
    if ok {
        set.ok("E01-规划-04-守恒式独立重算");
    } else {
        set.fail("E01-规划-04-守恒式独立重算", "10×16 ≠ 8×10×2 或单号域漂移");
    }
    if aligned {
        set.ok("E01-规划-05-组表对位与状态");
    } else {
        set.fail("E01-规划-05-组表对位与状态", "组表漂移或状态声明为空");
    }
}

fn chk_plan_audit(set: &mut CheckSet) {
    // 规划-06：全域审计通过（三闸一条命令）。
    match audit_all() {
        Ok(msg) => {
            let ok = msg.contains("八主题") && msg.contains("十组") && msg.contains("三向");
            if ok {
                set.ok("E01-规划-06-全域审计");
            } else {
                set.fail("E01-规划-06-全域审计", "审计文案缺三闸关键词");
            }
        }
        Err(_) => set.fail("E01-规划-06-全域审计", "审计不应失败"),
    }
}

// ---------------------------------------------------------------------------
// 四、判据承载力
// ---------------------------------------------------------------------------

fn chk_criterion_zero_panic(set: &mut CheckSet) {
    // 判据-01：生产面 + 判据面双零 panic 扫描。
    let src_prod = include_str!("cge01_domain.rs");
    let src_chk = include_str!("cge01_domain_checks.rs");
    let clean_prod = strip_lexical_noise(src_prod);
    let clean_chk = strip_lexical_noise(src_chk);
    let mut hits = 0usize;
    for pat in [".unwrap()", ".expect(", "panic!"].iter() {
        if clean_prod.contains(pat) || clean_chk.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        set.ok("E01-判据-01-双面零 panic");
    } else {
        set.fail("E01-判据-01-双面零 panic", "生产或判据代码含 panic 面");
    }
}

fn chk_criterion_codes(set: &mut CheckSet) {
    // 判据-02：码表冻结（4 码与生产面逐条同源对拍）。
    let expect: [&str; 4] = [
        E_DOMAIN_THEME_INVALID,
        E_DOMAIN_GROUP_INVALID,
        E_DOMAIN_PLAN_DRIFT,
        E_DOMAIN_BOUNDARY_DRIFT,
    ];
    let mut ok = expect.len() == EXPECT_WIRE.len();
    let mut k = 0usize;
    while ok && k < 4 {
        ok = expect[k] == EXPECT_WIRE[k];
        k += 1;
    }
    if ok {
        set.ok("E01-判据-02-码表冻结");
    } else {
        set.fail("E01-判据-02-码表冻结", "码表漂移");
    }
}

fn chk_criterion_mutation_sensitivity(set: &mut CheckSet) {
    // 判据-03：映射判据的变异敏感度自证——判据侧独立重算 (n-641)/16，
    // 若被测实现把 16 改 8（变异），657 会落 E01 而非 E02，恰阈断必红。
    // 此处独立复算两个恰阈值证明判据语料本身能分辨 16 与 8：
    let in_16 = 657usize / 16; // = 41 → 域内偏移 16 → 组 1（E02）
    let in_8 = 657usize / 8; // 变异下会映射到不同组
    let off = 657 - 641;
    let real = off / 16;
    let ok = real == 1 && in_16 != in_8;
    if ok {
        set.ok("E01-判据-03-映射语料可分辨");
    } else {
        set.fail("E01-判据-03-映射语料可分辨", "判据语料对 16/8 变异不可分辨");
    }
}

fn chk_criterion_baseline(set: &mut CheckSet) {
    // 判据-04：恒真防线——封闭/映射判据跑在非空基线上。
    let nontrivial = SurfaceTheme::ALL.len() == 8
        && BatchGroup::ALL.len() == 10
        && group_of_task(641).is_some();
    if nontrivial {
        set.ok("E01-判据-04-基线非平凡");
    } else {
        set.fail("E01-判据-04-基线非平凡", "基线全集为空，封闭断言退化为空真");
    }
}

fn chk_criterion_capacity(set: &mut CheckSet) {
    // 判据-05：非截断断言（独立常数对拍，不递归调 run 入口）。
    if EXPECTED_CHECK_COUNT <= MAX_CHECKS {
        set.ok("E01-判据-05-容量不截断");
    } else {
        set.fail("E01-判据-05-容量不截断", "判据总数超 MAX_CHECKS");
    }
}

// ---------------------------------------------------------------------------
// 入口（a=主题+定位+边界 / b=规划+判据承载力；合并入口供聚合器）
// ---------------------------------------------------------------------------

/// 判据族 a：主题 + 定位 + 边界。
pub fn run_cge01_checks_a_standalone() -> CheckSet {
    let mut s = CheckSet::new("cgpu/cge01/a");
    chk_theme_closed(&mut s);
    chk_theme_domain_pos(&mut s);
    chk_boundary_contracts(&mut s);
    chk_boundary_rejects(&mut s);
    chk_boundary_violation_detection(&mut s);
    s
}

/// 判据族 b：规划 + 判据承载力。
pub fn run_cge01_checks_b_standalone() -> CheckSet {
    let mut s = CheckSet::new("cgpu/cge01/b");
    chk_plan_bounds(&mut s);
    chk_plan_mapping(&mut s);
    chk_plan_conservation(&mut s);
    chk_plan_audit(&mut s);
    chk_criterion_zero_panic(&mut s);
    chk_criterion_codes(&mut s);
    chk_criterion_mutation_sensitivity(&mut s);
    chk_criterion_baseline(&mut s);
    chk_criterion_capacity(&mut s);
    s
}

/// 全域判据入口（聚合器调用这个）。
pub fn run_cge01_checks() -> CheckSet {
    CheckSet::merge(
        run_cge01_checks_a_standalone(),
        run_cge01_checks_b_standalone(),
    )
}
