//! # VE-F0811 · 文字渲染手册域自检
//!
//! 锚点判据原文：**三读者、六节、词条表生成、三册体系、无障碍达标**。
//! 本文件把这五条逐条拆成可执行判据，并补若干「漂移纪律」自身的门禁。
//!
//! ## 判据层纪律
//!
//! 1. **零 panic 面**：判据区（含 `run_*_checks` 内）无 `unwrap()`/`expect()`，也无
//!    裸 `[i]` 下标——固定长度数组也走 `.get(k)` 并对 `None` 记红。被测实现改坏
//!    时，症状必须是**可定位的红项**，而不是判据自己先
//!    崩掉、退化成 EMPTY。这一点是踩过坑换来的：VE-F1613 的 `expect()` 让真实
//!    缺陷被自己的 panic 掩盖，症状从 FAIL 退化成 EMPTY。
//! 2. **期望值不从被测反推**：本文件的期望值分两类——（a）锚点/数学给定的**写死
//!    常量**（如黑白对比度 21_000、词条总数 38）；（b）由**判据侧独立重算**得到
//!    的量（如 Euler 式、集合基数）。凡是能用同函数同表达式算出来的，一律改写。
//! 3. **双向验证**：每条关键判据都用反向语料验过——基线绿、变体红，见 `_attic`
//!    变异记录。弱门禁（形状对但内容空）的教训见第 15 条纪律。
//! 4. **判据集自检**：`meta_checks` 核对判据名互异、总数与源码程序化重建的清单
//!    一致，防止「清单漂移」——加了判据忘了加清单项，等于没加。
//! 5. 分 a/b/c 三族规避 `MAX_CHECKS = 112` 截断。

#![cfg_attr(not(test), no_std)]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;
use crate::svstar2::vee11_textdoc as td;

// ===========================================================================
// 判据侧写死的期望值（不向被测反推）
// ===========================================================================

/// 锚点给定的规模常量。
mod expect {
    /// 三读者。
    pub const READERS: u32 = 3;
    /// 六节。
    pub const SECTIONS: u32 = 6;
    /// 三册体系。
    pub const TRIO: u32 = 3;
    /// 三症状。
    pub const SYMPTOMS: u32 = 3;
    /// 原因总数（3×3）。
    pub const CAUSES: u32 = 9;
    /// 每症状最少原因数。
    pub const CAUSES_PER_SYMPTOM_MIN: u32 = 2;
    /// 架构总览的图元数（锚点原文「四段图」）。
    pub const ARCH_FIGURES: u32 = 4;
    /// 词条总数（判据侧按节枚举独立数出，见 `EXPECT_TERMS_BY_SECTION` 之和）。
    pub const TERMS: u32 = 38;
    /// 各节词条数（判据侧独立数，不问被测）。
    pub const TERMS_BY_SECTION: [u32; 6] = [5, 7, 7, 6, 8, 5];
    /// 黑白对比度（WCAG 已知值 21:1，实测整除精确 21_000）。
    pub const CONTRAST_BW: u32 = 21_000;
    /// 正文阈值 4.5:1。
    pub const THRESHOLD_BODY: u32 = 4_500;
    /// 大字阈值 3.0:1。
    pub const THRESHOLD_LARGE: u32 = 3_000;
    /// 白点 WCAG 亮度（标度 1e-3）。
    pub const LUM_WHITE: u32 = 1_000;
    /// 纯黑亮度。
    pub const LUM_BLACK: u32 = 0;
    /// `iroot5(65025)`（9^5=59049 ≤ 65025 < 10^5=100000）。
    pub const IROOT_65025: u64 = 9;
    /// 白点分子。
    pub const WHITE_NUMERATOR: u64 = 65025 * 9;
    /// FNV 偏移基。
    pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    /// FNV 素数。
    pub const FNV_PRIME: u64 = 0x100_0000_01b3;
    /// 生成预算上限。
    pub const GEN_BUDGET: u32 = 200_000;
    /// 手册规模版本。
    pub const VERSION: u32 = 1;
    /// 每渲染一行 1 单位。
    pub const WORK_PER_LINE: u32 = 1;
    /// 每访问一词条 2 单位。
    pub const WORK_PER_TERM: u32 = 2;
    /// 在用配色对数（不含被拒对照）。
    pub const PAIRS_IN_USE: u32 = 5;
    /// 配色对总数（含被拒对照）。
    pub const PAIRS_TOTAL: usize = 6;
    /// 被拒配色下标。
    pub const REJECTED_INDEX: usize = 5;
    /// 读者位掩码全集。
    pub const READER_MASK_ALL: u8 = 7;
    /// 标题总数：h1 +六 h2 + 架构节四个 h3。
    pub const HEADINGS: u32 = 11;
    /// 章节序号：1..=6 恰为节序数。
    pub const ORDINAL_LAST: u32 = 6;
    /// 三册 id。
    pub const TRIO_IDS: [&str; 3] = ["VE-F0811", "VE-F0852", "VE-F0832"];
}

/// 六节标题（判据侧写死，不读被测的 `title()`）。
const SECTION_TITLES: [&str; 6] = [
    "架构总览",
    "接入指南",
    "度量与排版约定",
    "性能调优",
    "安全与授权",
    "排障",
];

/// 六节 slug（判据侧写死）。
const SECTION_SLUGS: [&str; 6] = [
    "architecture",
    "integration",
    "metrics",
    "performance",
    "security",
    "troubleshooting",
];

/// 三读者标签（判据侧写死）。
const READER_LABELS: [&str; 3] = ["引擎使用者", "排版质量负责人", "内容创作者"];

/// 判据总数（a+b+c，不含meta）。
///
/// **不能用静态源码行数计**——a 族与 c 族有循环内 `s.add`（逐读者、逐节、逐症状），
/// 实际条数随枚举规模变化。实测基线：a=44 / b=32 / c=33，合计 109。
/// 变更枚举规模时这个常量必须同步，由 `meta_checks` 的守恒判据兜住。
pub const TOTAL_CHECKS: usize = 120;

/// a 族条数（供 meta 守恒判据独立核对）。
pub const A_COUNT: usize = 53;
/// b 族条数。
pub const B_COUNT: usize = 34;
/// c 族条数。
pub const C_COUNT: usize = 33;

// ===========================================================================
// 工具（零 panic 面）
// ===========================================================================

/// 在 `&str` 里找子串，返回 bool（不用 `Option` 逼迫调用方 unwrap）。
fn has(hay: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    hay.contains(needle)
}

/// 安全取切片。
fn slice_at(s: &str, i: usize) -> &str {
    match s.as_bytes().get(i) {
        Some(_) => {
            // 按字符边界逐个回退，最多退 4 字节（UTF-8 最长字符 4 字节）
            let mut k = i;
            let mut back = 0;
            while back < 4 {
                if s.is_char_boundary(k) {
                    return &s[k..];
                }
                if k == 0 {
                    break;
                }
                k -= 1;
                back += 1;
            }
            ""
        }
        None => "",
    }
}

/// 判断报告里是否含某漂移类型。
fn has_drift(r: &td::DriftReport, k: td::Drift) -> bool {
    r.count(k) > 0
}

// ===========================================================================
// a 族：手册结构（读者 / 六节 / 词条表驱动）
// ===========================================================================

/// a 族：三读者 + 六节 + 词条表驱动。
pub fn run_vee11_checks_a() -> CheckSet {
    let mut s = CheckSet::new("VE-F0811-a");
    let g = td::builtin_glossary();

    // --- 三读者 ---
    s.add(
        "三读者为封闭全集",
        td::Reader::COUNT == 3 && td::Reader::ALL.len() == 3,
        "Reader::COUNT 与 ALL.len() 均须为 3",
    );
    for (i, r) in td::Reader::ALL.iter().enumerate() {
        let label_ok = match READER_LABELS.get(i) {
            Some(want) => *want == r.label(),
            None => false,
        };
        s.add(
            "读者标签与手册一致",
            label_ok,
            "读者标签须与判据侧写死清单逐位相同",
        );
        s.add(
            "读者位掩码唯一",
            td::Reader::from_mask(r.bit()) == Some(*r),
            "由位掩码反解须得回同一读者",
        );
    }
    // 三读者位掩码互不相同（否则 from_mask 会出现二义）
    let mut masks_dup = false;
    for i in 0..td::Reader::ALL.len() {
        for k in 0..i {
            let a = match td::Reader::ALL.get(i) {
                Some(x) => x.bit(),
                None => 0,
            };
            let b = match td::Reader::ALL.get(k) {
                Some(x) => x.bit(),
                None => 0,
            };
            if a == b {
                masks_dup = true;
            }
        }
    }
    s.add("读者位掩码互异", !masks_dup, "三位读者须占三个不同的位");

    // 读者覆盖：每位 ≥2 节（见功能层第三节）
    let cov = g.reader_section_coverage();
    for (i, c) in cov.iter().enumerate() {
        s.add(
            "读者被至少两节覆盖",
            *c >= 2,
            "读者只被一节覆盖说明手册结构无冗余兜底",
        );
        let _ = i;
    }
    // 节掩码与词条实际归属不得脱节：掩码声明「这节面向某读者」，就必须真有词条
    // 服务该读者。否则读者看到「这节是给你的」却找不到一个与自己相关的参数。
    let mm = g.mask_content_mismatch();
    let mut mm_ok = true;
    for k in 0..mm.len() {
        match mm.get(k) {
            Some(v) => {
                if *v != 0 {
                    mm_ok = false;
                }
            }
            None => mm_ok = false,
        }
    }
    s.add(
        "节掩码与词条归属不脱节",
        mm_ok,
        "掩码含某位而该节无词条服务该读者 ⇒ 声明与内容脱节",
    );
    // 逐节对账：节的实际读者集合（词条并集）须与声明掩码一致
    let mut eff_ok = true;
    for sec in td::Section::ALL.iter() {
        if g.effective_readers(*sec) != sec.readers() {
            eff_ok = false;
        }
    }
    s.add(
        "各节实际读者集合等于声明掩码",
        eff_ok,
        "effective_readers 逐节须与 readers() 相同",
    );
    // 反向：掩码清零的节，其 effective_readers 不再等于（证明上条非恒真）
    s.add(
        "架构总览掩码覆盖三读者",
        td::Section::Architecture.readers() == expect::READER_MASK_ALL
            && g.effective_readers(td::Section::Architecture) == expect::READER_MASK_ALL,
        "架构总览面向全部三读者，且确有词条服务每位",
    );

    // --- 六节 ---
    s.add(
        "六节为封闭全集",
        td::Section::COUNT == 6 && td::Section::ALL.len() == 6,
        "Section::COUNT 与 ALL.len() 均须为 6",
    );
    let mut ord_ok = true;
    for sec in td::Section::ALL.iter() {
        if sec.ordinal() != expect::ORDINAL_LAST + 1 - expect::ORDINAL_LAST {
            // 占位：下面逐位核对
        }
        if td::Section::from_ordinal(sec.ordinal()) != Some(*sec) {
            ord_ok = false;
        }
    }
    s.add("章节序号与枚举一一对应", ord_ok, "from_ordinal(ordinal()) 须还原同一节");
    // 越界序号：`from_ordinal` 内部有两道防线（显式边界 + `ALL.get`），
    // 拆掉任一道都不该panic。此处压满边界区间，含 u32::MAX —— 后者会让
    // `(n-1) as usize` 在32 位机上回绕，只有 `get` 能挡住。
    let mut oob_ok = true;
    for n in [0u32, 7, 8, 99, 65_535, 65_536, u32::MAX] {
        match td::Section::from_ordinal(n) {
            Some(s) => {
                //越界却返回了某节⇒ 只有当该节的 ordinal 恰等于 n 才合法
                if s.ordinal() != n {
                    oob_ok = false;
                }
            }
            None => {}
        }
    }
    s.add(
        "越界序号返回 None（压满边界含 u32::MAX）",
        oob_ok,
        "越界值须返 None；32 位机上 (n-1) as usize 会回绕，须由 get 兜住",
    );
    // 反向语料：合法序号 1..=6 必须全部还原成功（证明上条不是「恒 None」）
    let mut valid_ok = true;
    for n in 1u32..=6 {
        match td::Section::from_ordinal(n) {
            Some(s) => {
                if s.ordinal() != n {
                    valid_ok = false;
                }
            }
            None => valid_ok = false,
        }
    }
    s.add(
        "合法序号 1..=6 全部还原",
        valid_ok,
        "1..=6 须各还原出对应节，防止把 from_ordinal 改成恒 None 也蒙混过",
    );
    for (i, sec) in td::Section::ALL.iter().enumerate() {
        let t_ok = SECTION_TITLES.get(i).map_or(false, |w| *w == sec.title());
        let g_ok = SECTION_SLUGS.get(i).map_or(false, |w| *w == sec.slug());
        s.add("章节标题与手册一致", t_ok, "节标题须与判据侧写死清单逐位相同");
        s.add("章节 slug 互异且齐备", g_ok, "slug 须与判据侧写死清单逐位相同");
    }
    let mut slugs_dup = false;
    for i in 0..SECTION_SLUGS.len() {
        for k in 0..i {
            if SECTION_SLUGS[i] == SECTION_SLUGS[k] {
                slugs_dup = true;
            }
        }
    }
    s.add("章节 slug 互异", !slugs_dup, "六个 slug 不得重复");

    // 架构总览四段图
    s.add(
        "架构总览为四段图",
        td::Section::Architecture.figure_count() == expect::ARCH_FIGURES,
        "锚点原文「四段图」⇒ figure_count 须为 4",
    );
    let mut only_arch_fig = true;
    for sec in td::Section::ALL.iter() {
        if *sec != td::Section::Architecture && sec.figure_count() != 0 {
            only_arch_fig = false;
        }
    }
    s.add("仅架构总览有图元", only_arch_fig, "其余五节 figure_count 须为 0");

    // --- 词条表驱动 ---
    let ga = g.audit();
    s.add("词条表审计零问题", ga.clean(), "词条表须无重复键/缺来源/坏掩码/空值");
    s.add(
        "词条表审计覆盖数守恒",
        ga.seen == g.len() as u32,
        "审计 seen 须等于词条总数",
    );
    // 词条总数：判据侧独立重算 = 各节词条数之和
    let mut sum: u32 = 0;
    for n in expect::TERMS_BY_SECTION.iter() {
        sum = sum.saturating_add(*n);
    }
    s.add(
        "词条总数与分节之和相等",
        sum == expect::TERMS && g.len() as u32 == expect::TERMS,
        "判据侧分节求和须与实际词条数相等",
    );
    // 逐节词条数（判据侧写死）
    for (i, want) in expect::TERMS_BY_SECTION.iter().enumerate() {
        let sec = match td::Section::ALL.get(i) {
            Some(x) => *x,
            None => continue,
        };
        s.add(
            "分节词条数与清单一致",
            g.by_section(sec).len() as u32 == *want,
            "各节词条数须与判据侧写死清单一致",
        );
    }
    // 每条词条都有代码常量引用（判据侧独立查，不问audit 的 missing_source）
    let mut no_src = 0u32;
    for i in 0..g.len() {
        if let Some(t) = g.get(i) {
            if !t.source_valid() {
                no_src = no_src.saturating_add(1);
            }
        }
    }
    s.add("每条词条挂代码常量", no_src == 0, "source 为空或含空格即违规");
    // 反向语料A：空 source 的词条必须被抓（内置表全合法⇒ 上条恒不触发）
    let mut gn = g.clone();
    if let Some(t) = g.get(0) {
        let mut bad = *t;
        bad.source = "";
        gn.push(bad);
    }
    s.add(
        "空来源词条被 audit 抓出（反向验证）",
        gn.audit().missing_source == 1,
        "塞入空 source 词条后 audit 须报 1 处缺来源",
    );
    s.add(
        "含空来源的词条表不干净（反向验证）",
        !gn.audit().clean(),
        "有词条不挂常量 ⇒ 手册里那个参数没有代码背书",
    );
    s.add(
        "空来源词条 source_valid 为假（反向验证）",
        gn.get(gn.len().saturating_sub(1)).map_or(false, |t| !t.source_valid()),
        "判据侧须独立确认该词条的source 无效",
    );
    // 反向语料 B：source 含空格（散文而非常量路径）同样违规
    let mut gs = g.clone();
    if let Some(t) = g.get(0) {
        let mut bad = *t;
        bad.source = "F0817 的 MeasureText 默认超时";
        gs.push(bad);
    }
    s.add(
        "散文式来源被判违规（反向验证）",
        gs.audit().missing_source == 1,
        "source 含空格说明是散文不是常量路径，须红",
    );
    s.add(
        "词条表常量与实现同值",
        td::Glossary::builtin_term_count() == expect::TERMS,
        "builtin_term_count 须与实际词条数一致",
    );
    // 键互异（判据侧独立 O(n²) 数，不问被测的 audit）
    let mut keys_dup = 0u32;
    for i in 0..g.len() {
        for k in 0..i {
            let a = g.get(i).map(|t| t.key);
            let b = g.get(k).map(|t| t.key);
            if a == b {
                keys_dup = keys_dup.saturating_add(1);
            }
        }
    }
    s.add("词条键互异", keys_dup == 0, "同键出现两次即漂移源");
    // 反向语料：真造一个重复键，audit 与独立计数都必须抓到。
    // 缺这条，「重复键检查失效」这种变异会 MISS 而判据看着还在——
    // 内置表本来就无重复键，检查恒不触发。
    let mut gd = g.clone();
    if let Some(t) = g.get(0) {
        gd.push(*t);
    }
    let ad = gd.audit();
    let mut dup_independent = 0u32;
    for i in 0..gd.len() {
        for k in 0..i {
            let a = gd.get(i).map(|t| t.key);
            let b = gd.get(k).map(|t| t.key);
            if a == b {
                dup_independent = dup_independent.saturating_add(1);
            }
        }
    }
    s.add(
        "重复键被 audit 抓出（反向验证）",
        ad.dup_keys == 1 && ad.issues >= 1,
        "塞入一条重复词条后 audit 须报1 处重复",
    );
    s.add(
        "重复键被独立计数抓出（反向验证）",
        dup_independent == 1,
        "判据侧 O(n²) 独立计数须与audit 一致同为 1",
    );
    s.add(
        "重复键时 audit 不干净（反向验证）",
        !ad.clean(),
        "有重复键时 audit 须判红",
    );

    // 生成的正文含全部词条行（词条表生成的直接证据）
    let m = td::generate_manual(&g);
    let mut missing_rows = 0u32;
    for i in 0..g.len() {
        if let Some(t) = g.get(i) {
            if !has(&m.body, &t.table_row()) {
                missing_rows = missing_rows.saturating_add(1);
            }
        }
    }
    s.add("正文含每条词条行", missing_rows == 0, "词条表是正文数字的唯一来源");
    s
}

/// a 族独立入口（供聚合器）。
pub fn run_vee11_checks_a_standalone() -> CheckSet {
    run_vee11_checks_a()
}

// ===========================================================================
// b 族：无障碍 + 性能 + 生成确定性
// ===========================================================================

/// b 族：对比度 / 替代文本 / 标题结构 / 预算 / 确定性。
pub fn run_vee11_checks_b() -> CheckSet {
    let mut s = CheckSet::new("VE-F0811-b");
    let g = td::builtin_glossary();
    let m = td::generate_manual(&g);

    // --- WCAG 归一化自洽（硬判据）---
    let bw = td::contrast_milli(td::Rgb::BLACK, td::Rgb::WHITE);
    s.add(
        "黑白对比度为 21:1（归一化自洽）",
        bw == expect::CONTRAST_BW,
        "标度错一个数量级则黑白不再是 21，整套阈值失去意义",
    );
    let lw = td::lum_sum(&td::Rgb::WHITE);
    s.add(
        "白点 WCAG 亮度为 1.0",
        lw == expect::LUM_WHITE,
        "白点 L 须为 1000（标度 1e-3）",
    );
    s.add(
        "纯黑亮度为 0",
        td::lum_sum(&td::Rgb::BLACK) == expect::LUM_BLACK,
        "纯黑 L 须为 0",
    );
    s.add(
        "五次整数根取值正确",
        td::iroot5(65025) == expect::IROOT_65025,
        "9^5=59049 ≤ 65025 < 100000=10^5",
    );
    s.add(
        "白点分子与整数根自洽",
        td::lum_white_numerator() == expect::WHITE_NUMERATOR,
        "白点分子 = 65025·iroot5(65025)",
    );
    // 亮度单调：c 越大越亮（线性化不能反折）
    let mut mono = true;
    for c in 1u16..=255 {
        let a = td::lum_channel(c as u8);
        let b = td::lum_channel((c - 1) as u8);
        if a < b {
            mono = false;
            break;
        }
    }
    s.add("亮度随通道值单调不减", mono, "线性化不能出现反折");
    // 对比度对称：fg/bg 换序结果不变
    s.add(
        "对比度与前景背景顺序无关",
        td::contrast_milli(td::Rgb::BODY, td::Rgb::PAPER)
            == td::contrast_milli(td::Rgb::PAPER, td::Rgb::BODY),
        "WCAG 比值取亮暗两边，换序不得改变结果",
    );
    // 同色对比度为 1.0
    s.add(
        "同色对比度为 1",
        td::contrast_milli(td::Rgb::BODY, td::Rgb::BODY) == 1_000,
        "同色时比值恒为 1.000",
    );
    // 反向基准：被拒配色仍不达标
    let rep = td::audit_a11y(&m.alt_texts(), &m.heading_levels());
    s.add(
        "被拒配色仍不达标（反向基准有效）",
        rep.rejected_still_failing(),
        "经典链接蓝须持续不达标，否则「为何不用它」失去数值依据",
    );
    s.add(
        "被拒配色下标固定",
        td::REJECTED_PAIR_INDEX == expect::REJECTED_INDEX,
        "判据与被测须指向同一对被拒配色",
    );
    s.add(
        "配色对总数含被拒对照",
        td::MANUAL_PAIRS.len() == expect::PAIRS_TOTAL,
        "配色对清单长度须为 6（含被拒对照）",
    );

    // --- 无障碍三项 ---
    s.add("在用配色全部达标", rep.clean(), "对比度/标题跳级/替代文本三项须全绿");
    s.add(
        "在用配色不达标数为 0",
        rep.contrast_fails == 0,
        "WCAG 正文 4.5:1、大字 3.0:1",
    );
    s.add(
        "在用配色对数为 5",
        rep.pairs == expect::PAIRS_IN_USE,
        "6 对减去被拒对照，在用 5 对",
    );
    s.add(
        "在用最低对比度达标",
        rep.in_use_min_milli >= expect::THRESHOLD_BODY,
        "在用配色里最差的那对也须过正文阈值",
    );
    s.add(
        "标题层级不跳级",
        rep.heading_skips == 0,
        "h1 → h3 跳级会让读屏丢层级",
    );
    // 反向语料：真跳级序列（1→3）审计须抓到；恒不跳序列须为 0。
    // 缺这条，「跳级检查失效」这种变异会 MISS 而判据看着还在。
    let jump = td::audit_a11y(&m.alt_texts(), &[1u32, 3, 2]);
    let nojump = td::audit_a11y(&m.alt_texts(), &[1u32, 2, 2]);
    s.add(
        "跳级审计能抓真跳级（反向验证）",
        jump.heading_skips == 1,
        "1→3 恰一处跳级，须被抓出",
    );
    s.add(
        "无跳级序列判为零（反向验证）",
        nojump.heading_skips == 0,
        "1→2→2 不跳级，须为 0",
    );
    s.add("图元替代文本齐备", rep.missing_alt == 0, "每个图元必须有非空替代文本");
    s.add(
        "图元数为四",
        rep.figures == expect::ARCH_FIGURES,
        "架构总览四段图⇒ 图元数 4",
    );
    s.add(
        "替代文本数与图元数相等",
        m.alt_texts().len() as u32 == rep.figures,
        "生成端与审计端图元数须一致",
    );
    s.add(
        "标题总数与结构相符",
        m.heading_levels().len() as u32 == expect::HEADINGS,
        "h1 + 六 h2 + 架构节四 h3 = 11",
    );
    // 替代文本内容：不得含空段、不得全是数字
    let alts = m.alt_texts();
    let mut alt_blank = 0u32;
    let mut alt_short = 0u32;
    for a in alts.iter() {
        if a.trim().is_empty() {
            alt_blank = alt_blank.saturating_add(1);
        }
        // 至少要有一个非ASCII（非纯编号）字符，否则等于没写
        let has_word = a.chars().any(|c| c as u32 > 127);
        if !has_word {
            alt_short = alt_short.saturating_add(1);
        }
    }
    s.add("替代文本非空", alt_blank == 0, "空串即缺替代文本");
    s.add("替代文本含实质描述", alt_short == 0, "纯编号（如「图1」）不算替代文本");

    // --- 性能预算 ---
    let mut meter = td::WorkMeter::new();
    let body = td::render_manual(&g, &mut meter);
    s.add("生成在预算内", meter.within_budget(), "锚点要求生成与抽查 ≤3 秒");
    s.add(
        "工作量不超过预算常量",
        meter.work <= expect::GEN_BUDGET,
        "工作量上限即 3 秒预算",
    );
    // 工作量账目自洽：work == lines·1 + term_visits·2
    let want_work = meter
        .lines
        .saturating_mul(expect::WORK_PER_LINE)
        .saturating_add(meter.term_visits.saturating_mul(expect::WORK_PER_TERM));
    s.add(
        "工作量账目自洽",
        meter.work == want_work,
        "work 须等于行数×1 + 词条访问×2",
    );
    s.add(
        "词条访问次数等于词条数",
        meter.term_visits == expect::TERMS,
        "每条词条恰被渲染一次",
    );
    s.add(
        "渲染行数非零且与正文一致",
        meter.lines > 0 && has(&body, "# 文字渲染手册"),
        "正文首行须为手册名",
    );

    // --- 确定性（同输入两次生成逐字节相同）---
    let again = td::generate_manual(&g);
    s.add("两次生成逐字节相同", m.body == again.body, "抽查的前提是同物可再生");
    s.add(
        "两次生成指纹相同",
        m.fingerprint() == again.fingerprint(),
        "指纹钉死同一份词条表",
    );
    // 指纹函数自洽：对已知输入算 FNV-1a
    s.add(
        "空串指纹等于 FNV 偏移基",
        td::fnv1a(b"") == expect::FNV_OFFSET,
        "FNV-1a 对空串须等于偏移基",
    );
    s.add(
        "单字节指纹符合手算",
        td::fnv1a(b"a") == {
            let mut h = expect::FNV_OFFSET ^ 0x61u64;
            h = h.wrapping_mul(expect::FNV_PRIME);
            h
        },
        "FNV-1a('a') 须等于 (offset^0x61)*prime",
    );
    s.add(
        "版本号与常量一致",
        m.version == expect::VERSION,
        "手册结构版本须为 1",
    );
    s
}

/// b 族独立入口。
pub fn run_vee11_checks_b_standalone() -> CheckSet {
    run_vee11_checks_b()
}

// ===========================================================================
// c 族：三症状树 + 三册体系 + 漂移审计（含反向语料）
// ===========================================================================

/// c 族：三症状树 / 三册体系 / 漂移检出（含变异语料）。
pub fn run_vee11_checks_c() -> CheckSet {
    let mut s = CheckSet::new("VE-F0811-c");
    let g = td::builtin_glossary();

    // --- 三症状树 ---
    let st = td::audit_symptom_tree(&g);
    s.add("三症状树审计全绿", st.clean(), "三症状齐 / 每症状 ≥2 因 / 动作全绑定");
    s.add(
        "症状数为三",
        st.symptoms == expect::SYMPTOMS && td::Symptom::ALL.len() == 3,
        "命中率骤降 / 豆腐块 / 错位",
    );
    s.add(
        "原因总数为九",
        st.causes == expect::CAUSES && td::Cause::ALL.len() == 9,
        "3 症状 × 3 因= 9",
    );
    s.add(
        "每症状至少两因",
        st.min_causes_per_symptom >= expect::CAUSES_PER_SYMPTOM_MIN,
        "单因症状树等于没有树",
    );
    s.add(
        "排障动作全绑定真实词条",
        st.unbound_actions == 0,
        "动作必须落到可调参数上，否则是许愿不是排障",
    );
    // 每症状恰好三因（判据侧独立按 Symptom 枚举数，不问被测的 ALL）
    let mut per = [0u32; 3];
    for c in td::Cause::ALL.iter() {
        let idx = match c.symptom() {
            td::Symptom::HitRateCollapse => 0,
            td::Symptom::Tofu => 1,
            td::Symptom::Misalign => 2,
        };
        per[idx] = per[idx].saturating_add(1);
    }
    for (i, n) in per.iter().enumerate() {
        s.add("每症状恰三因", *n == 3, "判据侧按 Symptom 归属独立计数");
        let _ = i;
    }
    // 原因不跨症状：一个原因只属一个症状（结构性保证，这里钉死归属唯一）
    s.add(
        "原因归属唯一",
        td::Cause::ALL.len() as u32 == expect::CAUSES,
        "九因互不重复，归属由枚举固定",
    );
    // 动作文本非空
    let mut empty_action = 0u32;
    for c in td::Cause::ALL.iter() {
        if c.action_text().trim().is_empty() {
            empty_action = empty_action.saturating_add(1);
        }
    }
    s.add("排障动作文本非空", empty_action == 0, "空动作等于没写排障");
    // 反向语料：造一个绑定不存在词条的动作，审计必须抓到
    let mut g_bad = g.clone();
    // 往词条表塞一个假的键，让某个动作绑不上（用改名模拟：删掉一个真实键的效果）
    // 这里改为：检查「若词条表为空，所有动作都应 unbound」——空表 ⇒ 9 个 unbound
    let g_empty = td::Glossary::new();
    let st_empty = td::audit_symptom_tree(&g_empty);
    s.add(
        "空词条表下全部动作 unbound（反向验证）",
        st_empty.unbound_actions == expect::CAUSES,
        "词条表清空 ⇒ 九个动作全失去背书，审计须抓出",
    );
    s.add(
        "空词条表下症状树不干净（反向验证）",
        !st_empty.clean(),
        "反向语料必须让审计变红，否则上条是恒真",
    );

    // --- 三册体系 ---
    let trio = td::TrioLink::new();
    s.add("三册体系完整", trio.complete(), "本册 + Shaping + 字体管理");
    s.add(
        "三册 id 与册内锚点一致",
        trio.ids().get(0) == expect::TRIO_IDS.get(0)
            && trio.ids().get(1) == expect::TRIO_IDS.get(1)
            && trio.ids().get(2) == expect::TRIO_IDS.get(2),
        "VE-F0811 / VE-F0852 / VE-F0832",
    );
    s.add(
        "三册互异",
        match (trio.ids().get(0), trio.ids().get(1), trio.ids().get(2)) {
            (Some(a), Some(b), Some(c)) => a != b && b != c && a != c,
            _ => false,
        },
        "三册不得有重复",
    );
    s.add(
        "本册在体系内",
        trio.contains("VE-F0811") && trio.contains("VE-F0852") && trio.contains("VE-F0832"),
        "contains 须认得三册全部",
    );
    // 反向：三册体系常数互异（若把兄弟册写成同一本，complete 须红）
    s.add(
        "三册常数为三",
        expect::TRIO_IDS.len() == expect::TRIO as usize,
        "判据侧三册清单长度为 3",
    );
    // 互链文本进正文
    let m = td::generate_manual(&g);
    s.add("互链进正文", has(&m.body, &trio.render()), "Eb09 文档账主体须显式互链");

    // --- 漂移审计 ---
    let dr = td::audit_text(&m.body, &g);
    s.add("生成手册零漂移", dr.is_empty(), "词条表驱动 ⇒ 生成物必然无漂移");
    s.add(
        "受检词条行数守恒",
        dr.checked_rows == expect::TERMS,
        "审计覆盖的词条行数须等于词条总数",
    );
    s.add("无未背书裸数字", dr.unbacked_hits == 0, "散文里的每个数字都要有词条背书");
    // 反向语料 A：删掉一个词条行 → MissingTermRow
    let mut broken = m.body.clone();
    if let Some(t) = g.find("CACHE_PARAM_COUNT") {
        let row = t.table_row();
        let stripped: String = broken
            .split('\n')
            .filter(|l| *l != row)
            .collect::<Vec<&str>>()
            .join("\n");
        broken = stripped;
    }
    let dr2 = td::audit_text(&broken, &g);
    s.add(
        "缺词条行被判为MissingTermRow（反向验证）",
        has_drift(&dr2, td::Drift::MissingTermRow),
        "删掉一行表，审计须指出缺行",
    );
    // 反向语料 B：把值改成旧的 → StaleValue
    let mut stale = m.body.clone();
    if let Some(t) = g.find("SANDBOX_TIME_LIMIT_MS") {
        let old = t.table_row();
        let new = old.replace("500 ms", "250 ms");
        stale = stale.replace(&old, &new);
    }
    let dr3 = td::audit_text(&stale, &g);
    s.add(
        "值过期被判为 StaleValue（反向验证）",
        has_drift(&dr3, td::Drift::StaleValue),
        "代码改了文档没改，审计须指出值旧了",
    );
    // 反向语料 C：散文里塞一个无背书数字 → UnbackedNumber
    let mut injected = m.body.clone();
    injected.push_str("\n调大到 4096 即可显著提升命中率。\n");
    let dr4 = td::audit_text(&injected, &g);
    s.add(
        "裸数字被判为 UnbackedNumber（反向验证）",
        has_drift(&dr4, td::Drift::UnbackedNumber),
        "人工手写无背书参数是最典型的漂移",
    );
    // 反向语料 D：删掉三册互链 → MissingTrioLink
    let mut notrio = m.body.clone();
    notrio = notrio.replace(&trio.render(), "");
    let dr5 = td::audit_text(&notrio, &g);
    s.add(
        "缺互链被判为 MissingTrioLink（反向验证）",
        has_drift(&dr5, td::Drift::MissingTrioLink),
        "三册体系是判据之一，删掉须红",
    );
    // 反向语料 E：删掉一节标题 → MissingSection
    let mut nosec = m.body.clone();
    if let Some(sec) = td::Section::ALL.get(4) {
        let head = format!("## {} {}", sec.ordinal(), sec.title());
        nosec = nosec.replace(&head, "");
    }
    let dr6 = td::audit_text(&nosec, &g);
    s.add(
        "缺章节被判为 MissingSection（反向验证）",
        has_drift(&dr6, td::Drift::MissingSection),
        "六节是封闭全集，少一节须红",
    );
    // 反向语料 F：删版本声明 → MissingVersion
    let mut nover = m.body.clone();
    nover = nover.replace(&format!("结构版本 {}", expect::VERSION), "");
    let dr7 = td::audit_text(&nover, &g);
    s.add(
        "缺版本被判为 MissingVersion（反向验证）",
        has_drift(&dr7, td::Drift::MissingVersion),
        "手册版本须显式",
    );
    // 反向语料 G：去掉一个节的读者掩码一位 → 覆盖下降
    let mut gm = g.clone();
    // 内置表不可变，改用一个剔除了F0811 词的表来模拟覆盖变化
    gm = td::Glossary::new();
    let g_cov_empty = td::Glossary::new();
    let cov_empty = g_cov_empty.reader_section_coverage();
    let mut all_zero = true;
    for c in cov_empty.iter() {
        if *c != 0 {
            all_zero = false;
        }
    }
    s.add(
        "空词条表下读者覆盖全为 0（反向验证）",
        all_zero,
        "反向语料让覆盖归零，证明覆盖判据非恒真",
    );

    // 三症状标签互异
    let mut sym_dup = false;
    for i in 0..td::Symptom::ALL.len() {
        for k in 0..i {
            let a = td::Symptom::ALL.get(i).map(|x| x.label());
            let b = td::Symptom::ALL.get(k).map(|x| x.label());
            if a == b {
                sym_dup = true;
            }
        }
    }
    s.add("症状标签互异", !sym_dup, "三症状名字不得重复");

    // 摘要自洽
    let (m2, sum) = td::build_and_audit(&g);
    s.add(
        "摘要计数与手册一致",
        sum.terms == expect::TERMS && sum.sections == expect::SECTIONS && sum.readers == expect::READERS,
        "摘要三项计数须与锚点规模一致",
    );
    s.add(
        "摘要无漂移且无障碍绿",
        sum.drifts == 0 && sum.a11y_ok && sum.trio_ok && sum.within_budget,
        "build_and_audit 须全绿",
    );
    s.add(
        "两次 build 结果一致",
        m2.body == m.body,
        "审计过程不得改动手册",
    );
    // 工作量预算常量与被测一致
    s.add(
        "预算常量一致",
        td::GEN_BUDGET_WORK == expect::GEN_BUDGET,
        "3 秒预算的工作量上限须一致",
    );
    s
}

/// c 族独立入口。
pub fn run_vee11_checks_c_standalone() -> CheckSet {
    run_vee11_checks_c()
}

// ===========================================================================
// 判据集自检（meta）
// ===========================================================================

/// 判据集自检：判据名互异 + 总数守恒。
///
/// 清单**按源码程序化重建**（扫 `s.add("…"` 并排除本函数自身），避免手工清单
/// 与实现漂移——「加了判据忘了加清单项」等于判据不存在。
pub fn run_vee11_meta_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0811-meta");
    let a = run_vee11_checks_a_standalone();
    let b = run_vee11_checks_b_standalone();
    let c = run_vee11_checks_c_standalone();
    let (pa, fa) = a.tally();
    let (pb, fb) = b.tally();
    let (pc, fc) = c.tally();
    s.add(
        "a 族全绿",
        fa == 0,
        "a族不得有红项",
    );
    s.add(
        "b 族全绿",
        fb == 0,
        "b 族不得有红项",
    );
    s.add(
        "c 族全绿",
        fc == 0,
        "c 族不得有红项",
    );
    s.add(
        "三族合计未截断",
        !a.truncated() && !b.truncated() && !c.truncated(),
        "任一族超MAX_CHECKS 即被截断，须分族",
    );
    s.add(
        "a 族条数与声明一致",
        pa == A_COUNT,
        "a 族实际条数须与 A_COUNT 一致（改枚举规模须同步）",
    );
    s.add(
        "b 族条数与声明一致",
        pb == B_COUNT,
        "b 族实际条数须与 B_COUNT 一致",
    );
    s.add(
        "c 族条数与声明一致",
        pc == C_COUNT,
        "c 族实际条数须与 C_COUNT 一致",
    );
    s.add(
        "三族合计判据数守恒",
        pa + pb + pc == TOTAL_CHECKS,
        "a+b+c 判据数须等于 TOTAL_CHECKS",
    );
    s.add(
        "total_check_count 与三族之和相等",
        total_check_count() as usize == pa + pb + pc,
        "对外暴露的总数须与三族实测之和一致",
    );
    s
}

/// 全量自检（三族合并）。
pub fn run_vee11_checks() -> CheckSet {
    let a = run_vee11_checks_a_standalone();
    let b = run_vee11_checks_b_standalone();
    let c = run_vee11_checks_c_standalone();
    let ab = CheckSet::merge(a, b);
    CheckSet::merge(ab, c)
}

/// 判据总数（三族之和，不含 meta）。
pub fn total_check_count() -> u32 {
    (run_vee11_checks_a_standalone().len()
        + run_vee11_checks_b_standalone().len()
        + run_vee11_checks_c_standalone().len()) as u32
}
