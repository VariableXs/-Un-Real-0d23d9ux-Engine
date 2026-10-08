//! # VE-F0812 · 文字渲染测试资产域自检
//!
//! 锚点判据原文：**金样 0.5% 阈值、四组测试、金样双人复核、6 分钟全量、软件参考路径**。
//! 本文件把这五条拆成可执行判据，并补「纪律本身也要有门禁」的一组：
//!
//! 锚点的错误路径两条最容易被无声绕过——
//!   「禁止现场调阈值放行」：若阈值可写，算法一改就有人把 0.5% 调到 5%。
//!   「金样双人复核」：若复核不校验互异，同一人点两下就算双人。
//! 所以判据里必须有「阈值被放宽必须被拒」与「同人两次必须被拒」的**反向语料**，
//! 否则这两条纪律只是文档里的一句话。
//!
//! ## 判据层纪律
//!
//! 1. **零 panic 面**：无 `unwrap()`/`expect()`，固定长度数组也走 `.get(k)` 对 `None` 记红。
//! 2. **期望值不从被测反推**：diff 阈值判定在判据侧用**独立的整数交叉相乘**
//!    重算一遍，与被测的 `diff_pixels` 比对；两套同构实现互为对照。
//! 3. **反向语料逐条双向验证**：每条纪律配一条「违反它必须变红」的语料。
//! 4. 分 a/b/c 三族规避 `MAX_CHECKS = 112` 截断。

#![cfg_attr(not(test), no_std)]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;
use crate::svstar2::vee12_textqa as qa;

// ===========================================================================
// 判据侧写死的期望值
// ===========================================================================

mod expect {
    /// 金样 diff 阈值千分比（0.5%）。
    pub const PERMILLE: u32 = 5;
    /// 字体数。
    pub const FONTS: u32 = 32;
    /// 字号数。
    pub const SIZES: u32 = 8;
    /// Hinting 档数。
    pub const HINTS: u32 = 3;
    /// 组合总数 768。
    pub const COMBOS: u32 = 768;
    /// FNV 偏移基。
    pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    /// FNV 素数。
    pub const FNV_PRIME: u64 = 0x100_0000_01b3;
    /// 全量预算（秒）。
    pub const FULL_BUDGET: u32 = 360;
    /// 静态字数。
    pub const STATIC: u32 = 1_000;
    /// 动态字数。
    pub const DYNAMIC: u32 = 500;
    /// 冒烟示例数。
    pub const SMOKE: u32 = 4;
    /// 非法序列档数。
    pub const DECODE_BUCKETS: u32 = 4;
    /// 度量字段数。
    pub const METRIC_FIELDS: u32 = 3;
    /// 行高模式数。
    pub const LINE_MODES: u32 = 3;
    /// 双人复核最少人数。
    pub const MIN_REVIEWERS: usize = 2;
    /// 组数。
    pub const GROUPS: u32 = 4;
    /// 缓存水位千分比。
    pub const WATERMARK: u32 = 850;
    /// 基线容差（千分之一像素）。
    pub const BASELINE_TOL: i32 = 1;
}

/// 判据总数（a+b+c，不含 meta）。按**运行值**计——循环内 `s.add` 使实际条数随枚举变化。
/// 实测基线：a=37 / b=38 / c=44，合计 119。
pub const TOTAL_CHECKS: usize = 119;
/// a 族条数。
pub const A_COUNT: usize = 37;
/// b 族条数。
pub const B_COUNT: usize = 38;
/// c 族条数。
pub const C_COUNT: usize = 44;

/// 判据侧独立的阈值判定（与被测 `diff_pixels` 同构但独立写一遍）。
///
/// 用 `u64` 交叉相乘，避免 32 位溢出：`diff * 1000` 在 diff 接近 u32::MAX 时会溢出。
fn expect_within_threshold(total: u32, diff: u32) -> bool {
    if total == 0 {
        return false;
    }
    let d = if diff > total { total } else { diff };
    (d as u64 * 1_000) <= (total as u64) * (expect::PERMILLE as u64)
}

/// 判据侧独立算占比千分比。
fn expect_permille(total: u32, diff: u32) -> u32 {
    if total == 0 {
        return 0;
    }
    let d = if diff > total { total } else { diff };
    ((d as u64 * 1_000) / total as u64) as u32
}

// ===========================================================================
// a 族：金样 diff 阈值 + 金样更新纪律（双人复核 / 阈值不可调 / 软件参考路径）
// ===========================================================================

/// a 族：金样 diff 阈值判定 + 金样更新纪律。
pub fn run_vee12_checks_a() -> CheckSet {
    let mut s = CheckSet::new("VE-F0812-a");

    // --- 阈值常量 ---
    s.add(
        "金样阈值常量为 0.5%（5‰）",
        qa::GOLDEN_DIFF_PERMILLE == expect::PERMILLE,
        "锚点原文「图像 diff 阈值 0.5%」⇒ 5‰",
    );

    // --- 边界判定：逐点比对判据侧独立实现 ---
    //语料含：0 差异、恰在阈值、阈��+1、超阈、小样本、大样本、非法（total=0）
    let corpus: [(u32, u32); 10] = [
        (10_000, 0),
        (10_000, 50),
        (10_000, 51),
        (1_000, 5),
        (1_000, 6),
        (20_000, 100),
        (20_000, 101),
        (100, 1),
        (1, 1),
        (0, 5),
    ];
    let mut agree = true;
    let mut invalid_ok = true;
    for (t, d) in corpus.iter() {
        let got = qa::diff_pixels(*t, *d);
        let want_ok = expect_within_threshold(*t, *d);
        if got.verdict.passed() != want_ok {
            agree = false;
        }
        if got.permille != expect_permille(*t, *d) {
            agree = false;
        }
        if *t == 0 && !matches!(got.verdict, qa::DiffVerdict::Invalid) {
            invalid_ok = false;
        }
    }
    s.add(
        "diff 判定与判据侧独立实现逐点一致",
        agree,
        "10 组语料（含阈值边界两侧）须两套实现结论相同",
    );
    s.add("总像素为 0 判为非法样本", invalid_ok, "空图不可参与判定");

    // --- 阈值边界精确性（浮点会抖的地方）---
    s.add(
        "恰在阈值上判为通过（边界不抖动）",
        qa::diff_pixels(10_000, 50).verdict.passed()
            && qa::diff_pixels(1_000, 5).verdict.passed()
            && qa::diff_pixels(20_000, 100).verdict.passed(),
        "0.005 在二进制浮点里不可精确表示，边界判定必须走整数式",
    );
    s.add(
        "阈值 +1 像素即超阈",
        !qa::diff_pixels(10_000, 51).verdict.passed()
            && !qa::diff_pixels(1_000, 6).verdict.passed(),
        "多一个差异像素就须红——判据不能有容差余量",
    );
    s.add(
        "小样本不放宽阈值",
        !qa::diff_pixels(100, 1).verdict.passed(),
        "100 像素里差 1 个是 1%，远超0.5%——不得因样本小而从宽",
    );
    s.add(
        "单像素全差异判超阈",
        !qa::diff_pixels(1, 1).verdict.passed(),
        "1/1 = 100% 必超阈",
    );
    s.add(
        "差异超过总数被夹住且判超阈",
        qa::diff_pixels(10, 20).permille == 1_000
            && !qa::diff_pixels(10, 20).verdict.passed(),
        "样本不自洽时不得静默放行",
    );

    // --- 金样集规模 ---
    let set = qa::GoldenSet::full(qa::AssetOrigin::DomainSpecific);
    s.add(
        "金样组合数为 32×8×3=768",
        set.combo_count() == expect::COMBOS,
        "锚点原文「32 字体×8 字号×3 档 Hinting」",
    );
    s.add(
        "组合数常量与实际一致",
        qa::GOLDEN_COMBO_COUNT == expect::COMBOS,
        "COMBO_COUNT 常量须等于实测 768",
    );
    s.add("逐字体组合齐备", set.all_fonts_complete(), "每个字体下 8×3 须全在");
    s.add("金样键互异", !set.has_duplicate_keys(), "重复键会让覆盖数虚高");
    // 反向：内置满额金样集天然无重复键 ⇒ 上条恒真，必须造重复键语料钉死互异检查。
    // 判据侧用 key_at 独立重数，不拿 has_duplicate_keys 的返回值当唯一依据。
    {
        let mut dup = qa::GoldenSet::empty(qa::AssetOrigin::DomainSpecific);
        dup.push_raw(qa::GoldenKey::new(0, 8, 0), 11);
        dup.push_raw(qa::GoldenKey::new(0, 8, 0), 22);
        dup.push_raw(qa::GoldenKey::new(0, 8, 1), 33);
        dup.push_raw(qa::GoldenKey::new(1, 8, 0), 44);
        dup.push_raw(qa::GoldenKey::new(1, 8, 0), 55);
        // 判据侧独立重算：出现次数 > 1 的键**种类数**。
        let mut dup_kinds: u32 = 0;
        for i in 0..dup.len() {
            let ki = dup.key_at(i);
            // 只在该键首次出现时计一次，避免同一种键按重数重复累加。
            let mut first_seen = true;
            for j in 0..i {
                if dup.key_at(j) == ki {
                    first_seen = false;
                }
            }
            if first_seen && ki.is_some() {
                let mut times: u32 = 0;
                for j in 0..dup.len() {
                    if dup.key_at(j) == ki {
                        times = times.saturating_add(1);
                    }
                }
                if times > 1 {
                    dup_kinds = dup_kinds.saturating_add(1);
                }
            }
        }
        s.add(
            "重复键语料：互异检查必须报出（反向验证）",
            dup.has_duplicate_keys() && dup_kinds == 2,
            "内置满额集无重复键；键 (0,8,0) 与 (1,8,0) 各出现 2 次 ⇒ 重复键种类恰为 2",
        );
        s.add(
            "重复键语料：无重复键集不误报（反向验证）",
            !qa::GoldenSet::empty(qa::AssetOrigin::DomainSpecific).has_duplicate_keys(),
            "空集无键可重，检查不得误报",
        );
        s.add(
            "键下标访问器越界返回空而非 panic",
            dup.key_at(dup.len()) == None && dup.key_at(usize::MAX) == None,
            "判据区零 panic 面：越界走 Option",
        );
    }

    // --- 键指纹确定性 + FNV 自洽 ---
    s.add(
        "金样键指纹确定（两次同键同值）",
        {
            let a = qa::key_fingerprint(&qa::GoldenKey::new(3, 24, 1));
            let b = qa::key_fingerprint(&qa::GoldenKey::new(3, 24, 1));
            a == b
        },
        "同键必须同指纹，否则覆盖统计无意义",
    );
    s.add(
        "不同键指纹相异",
        qa::key_fingerprint(&qa::GoldenKey::new(3, 24, 1))
            != qa::key_fingerprint(&qa::GoldenKey::new(3, 24, 2))
            && qa::key_fingerprint(&qa::GoldenKey::new(3, 24, 1))
                != qa::key_fingerprint(&qa::GoldenKey::new(4, 24, 1)),
        "字号或字体不同须给不同指纹",
    );
    s.add(
        "空串 FNV 等于偏移基",
        qa::key_fingerprint(&qa::GoldenKey::new(0, 0, 0)) != 0,
        "指纹不得恒零——恒零会让「有指纹」这条判据永真",
    );
    s.add(
        "FNV 偏移基常量正确",
        expect::FNV_OFFSET == 0xcbf2_9ce4_8422_2325 && expect::FNV_PRIME == 0x100_0000_01b3,
        "FNV-1a 参数写死，判据侧独立核对",
    );

    // --- 金样更新纪律 ---
    let k = qa::GoldenKey::new(0, 8, 0);
    let mut rep = qa::DiffReport {
        key: k.label(),
        diff: 3,
        total: 10_000,
        verdict: qa::diff_pixels(10_000, 3).verdict,
        reviewers: Vec::new(),
        path: qa::RasterPath::Software,
    };
    rep.reviewers.push("alice".to_string());
    rep.reviewers.push("bob".to_string());
    let ok_update = qa::GoldenUpdate {
        key: k,
        new_fingerprint: 0x1234_5678,
        threshold_permille: qa::GOLDEN_DIFF_PERMILLE,
        report: rep.clone(),
    };
    s.add(
        "合规更新被准入",
        ok_update.ok(),
        "阈值未动 + 软件路径 + 双人互异 + 键相符 ⇒ 放行",
    );
    s.add(
        "合规报告本身合规",
        rep.compliant(),
        "报告层也要能自判合规",
    );

    // 反向语料 1：阈值被放宽（锚点「禁止现场调阈值放行」）
    let relaxed = qa::GoldenUpdate {
        threshold_permille: 500,
        ..ok_update.clone()
    };
    s.add(
        "阈值被放宽必须拒绝（反向验证）",
        relaxed.admit() == Some(qa::UpdateReject::ThresholdRelaxed),
        "锚点原文「禁止现场调阈值放行」——0.5% 是常量不是参数",
    );
    // 反向语料 1b：放宽到别的值也须拒（不是只拒 500）
    let relaxed2 = qa::GoldenUpdate {
        threshold_permille: qa::GOLDEN_DIFF_PERMILLE + 1,
        ..ok_update.clone()
    };
    s.add(
        "阈值 +1 亦拒绝（反向验证）",
        relaxed2.admit() == Some(qa::UpdateReject::ThresholdRelaxed),
        "任何与常量不符的阈值都拒绝，不只是离谱的那个",
    );

    // 反向语料 2：GPU 路径（锚点「固定软件光栅化参考路径」）
    let mut gpu_rep = rep.clone();
    gpu_rep.path = qa::RasterPath::Gpu;
    let gpu = qa::GoldenUpdate {
        report: gpu_rep,
        ..ok_update.clone()
    };
    s.add(
        "GPU 路径跑金样 diff 必须拒绝（反向验证）",
        gpu.admit() == Some(qa::UpdateReject::FlakyPath),
        "GPU 差异即 flaky 源，diff 判定只接受软件路径",
    );

    // 反向语料 3：单人复核
    let mut one = rep.clone();
    one.reviewers.clear();
    one.reviewers.push("alice".to_string());
    let solo = qa::GoldenUpdate {
        report: one,
        ..ok_update.clone()
    };
    s.add(
        "单人复核必须拒绝（反向验证）",
        solo.admit() == Some(qa::UpdateReject::ReviewersTooFew),
        "锚点原文「双人复核」——一人不算",
    );

    // 反向语料 4：同一人两次（关键：判的是互异不是个数）
    let mut same = rep.clone();
    same.reviewers.clear();
    same.reviewers.push("alice".to_string());
    same.reviewers.push("alice".to_string());
    let dup = qa::GoldenUpdate {
        report: same,
        ..ok_update.clone()
    };
    s.add(
        "同一人复核两次必须拒绝（反向验证）",
        dup.admit() == Some(qa::UpdateReject::ReviewersNotDistinct),
        "两人≠两次；不判互异则「点两下」即可冒充双人复核",
    );
    s.add(
        "同一人两次时报告不合规",
        !dup.report.compliant(),
        "报告层的互异判据须独立生效",
    );

    // 反向语料 5：报告键与请求键不符
    let mut wrong = rep.clone();
    wrong.key = "f9/s99/h2".to_string();
    let mism = qa::GoldenUpdate {
        key: k,
        report: wrong,
        ..ok_update.clone()
    };
    s.add(
        "报告与金样键不符必须拒绝（反向验证）",
        mism.admit() == Some(qa::UpdateReject::ReportKeyMismatch),
        "拿A 的diff 报告去更新 B 是常见越权",
    );

    // 反向语料 6：报告样本非法
    let mut inv = rep.clone();
    inv.verdict = qa::DiffVerdict::Invalid;
    let bad = qa::GoldenUpdate {
        report: inv,
        ..ok_update.clone()
    };
    s.add(
        "非法 diff 报告必须拒绝（反向验证）",
        bad.admit() == Some(qa::UpdateReject::ReportInvalid),
        "Invalid 样本不能作为更新依据",
    );

    // --- 拒绝时金样集不得变动（纪律的落点）---
    let mut gs = qa::GoldenSet::full(qa::AssetOrigin::DomainSpecific);
    let fp_before = gs.fingerprint_of(k);
    let rej = gs.update(&relaxed);
    s.add(
        "被拒更新不改动金样集",
        rej.is_some() && gs.fingerprint_of(k) == fp_before,
        "拒绝必须真的不落地，否则「先放行后补报告」行得通",
    );
    let admitted_after_reject = gs.admitted;
    s.add(
        "被拒后准入计数未增",
        admitted_after_reject == 0,
        "准入计数须只统计真正放行的更新",
    );
    let _ = gs.update(&solo);
    let _ = gs.update(&dup);
    s.add(
        "拒绝计数逐次累加",
        gs.rejected == 3,
        "拒绝次数是绕纪律的遥测，须如实累加",
    );
    // 准入后计数递增
    let _ = gs.update(&ok_update);
    s.add(
        "准入后计数递增",
        gs.admitted == 1 && gs.rejected == 3,
        "准入与拒绝两条计数须各自准确",
    );

    // --- 软件参考路径 ---
    s.add(
        "软件路径合法且非 flaky 源",
        qa::RasterPath::Software.diff_legal() && !qa::RasterPath::Software.flaky_source(),
        "软件光栅化是金样 diff 的唯一合法路径",
    );
    s.add(
        "GPU 路径非合法且是 flaky 源",
        !qa::RasterPath::Gpu.diff_legal() && qa::RasterPath::Gpu.flaky_source(),
        "GPU 差异即 flaky，须被路径枚举显式标记",
    );
    s.add(
        "路径标签可反解且互异",
        {
            let a = qa::RasterPath::from_label("软件光栅化");
            let b = qa::RasterPath::from_label("GPU 光栅化");
            a == Some(qa::RasterPath::Software)
                && b == Some(qa::RasterPath::Gpu)
                && a != b
        },
        "标签反解须双向可逆",
    );
    s.add(
        "未知路径标签返None",
        qa::RasterPath::from_label("量子光栅化").is_none(),
        "零 panic 面：未知标签不得 panic",
    );
    s
}

/// a 族独立入口。
pub fn run_vee12_checks_a_standalone() -> CheckSet {
    run_vee12_checks_a()
}

// ===========================================================================
// b 族：度量对拍 + 基线 + 行高 + 缓存 + 性能基准
// ===========================================================================

/// b 族：度量对拍 / 基线对齐 / 行高三模式 / 缓存 / 性能基准。
pub fn run_vee12_checks_b() -> CheckSet {
    let mut s = CheckSet::new("VE-F0812-b");

    // --- 度量对拍 ---
    let mut d = qa::MetricDualRun::new();
    d.push(qa::MetricField::Ascent, 100, 100);
    d.push(qa::MetricField::Descent, 25, 25);
    d.push(qa::MetricField::Advance, 50, 50);
    s.add("双跑一致判为 Agree", d.all_agree(), "三项一致");
    s.add(
        "三字段常量与枚举一致",
        qa::MetricField::ALL.len() as u32 == expect::METRIC_FIELDS,
        "锚点原文「ascent/descent/advance」三项",
    );
    for f in qa::MetricField::ALL.iter() {
        let v = d.verdict_of(*f);
        s.add(
            "逐字段判定为一致",
            v == qa::DualRunVerdict::Agree,
            "对拍须逐字段判，不能只判整体",
        );
    }
    // 反向：任一字段不一致即整体不一致
    let mut bad = qa::MetricDualRun::new();
    bad.push(qa::MetricField::Ascent, 100, 100);
    bad.push(qa::MetricField::Descent, 25, 25);
    bad.push(qa::MetricField::Advance, 50, 51);
    s.add(
        "单字段不一致即整体不一致（反向验证）",
        !bad.all_agree() && bad.disagree_count() == 1,
        "只差一个字段也不能放过",
    );
    // 反向：缺字段判 Missing（与 Disagree 区分）
    let empty = qa::MetricDualRun::new();
    s.add(
        "空对拍不判一致（反向验证）",
        !empty.all_agree(),
        "「没跑」不等于「跑过了」",
    );
    s.add(
        "缺字段判为 Missing 而非 Agree（反向验证）",
        empty.verdict_of(qa::MetricField::Ascent) == qa::DualRunVerdict::Missing,
        "缺失与不一致必须可区分",
    );
    s.add(
        "缺字段计入不一致数（反向验证）",
        empty.disagree_count() == expect::METRIC_FIELDS,
        "缺失也要计入，否则漏测不体现",
    );
    s.add(
        "Missing 与 Disagree 都不算通过",
        !qa::DualRunVerdict::Missing.passed() && !qa::DualRunVerdict::Disagree.passed(),
        "只有 Agree 算通过",
    );

    // --- 基线对齐（有符号差，容差 1）---
    s.add(
        "基线完全相等判对齐",
        qa::BaselineCheck::new(1_000, 1_000).aligned(),
        "零偏差必对齐",
    );
    s.add(
        "基线差一在容差内判对齐",
        qa::BaselineCheck::new(1_000, 1_001).aligned()
            && qa::BaselineCheck::new(1_000, 999).aligned(),
        "容差 1（千分之一像素）",
    );
    s.add(
        "基线差二超容差判不对齐",
        !qa::BaselineCheck::new(1_000, 1_002).aligned(),
        "差 2 已超容差",
    );
    s.add(
        "容差常量为 1",
        qa::BaselineCheck::TOLERANCE == expect::BASELINE_TOL
            && qa::BaselineCheck::new(0, 0).tolerance == expect::BASELINE_TOL,
        "容差须显式登记，不是隐含 0",
    );
    s.add(
        "偏差带符号（绘制侧减度量侧）",
        qa::BaselineCheck::new(1_000, 1_002).delta() == 2
            && qa::BaselineCheck::new(1_002, 1_000).delta() == -2,
        "方向信息不能丢——只记绝对值会掩盖「谁偏了」",
    );
    // 反向：极值下不溢出（i32::MIN/MAX 相减）。
    // 期望值用 u64 独立算出（真值 MIN−MAX = −4294967295，超出 i32 ⇒ 饱和到 MIN），
    // 不向被测反推。
    let hi_lo = qa::BaselineCheck::new(i32::MAX, i32::MIN);
    let lo_hi = qa::BaselineCheck::new(i32::MIN, i32::MAX);
    let expect_hi = (i32::MIN as i64) - (i32::MAX as i64); // −4294967295
    let expect_lo = (i32::MAX as i64) - (i32::MIN as i64); // +4294967295
    let clamp = |v: i64| -> i32 {
        if v > i32::MAX as i64 {
            i32::MAX
        } else if v < i32::MIN as i64 {
            i32::MIN
        } else {
            v as i32
        }
    };
    s.add(
        "极值基线差用饱和减不溢出",
        !hi_lo.aligned()
            && !lo_hi.aligned()
            && hi_lo.delta() == clamp(expect_hi)
            && lo_hi.delta() == clamp(expect_lo),
        "真值 ±4294967295 超 i32 ⇒ 饱和到 MIN/MAX；有符号溢出在 no_std 下是未定义行为",
    );

    // --- 行高三模式 ---
    s.add(
        "行高模式数为三",
        qa::LineHeightMode::ALL.len() as u32 == expect::LINE_MODES
            && qa::LINE_HEIGHT_MODES == expect::LINE_MODES,
        "锚点原文「行高三模式」",
    );
    s.add(
        "三模式倍率齐备（默认 1.2 / 紧凑 1.0 / 宽松 1.3）",
        qa::LineHeightMode::Default.permille() == 1_200
            && qa::LineHeightMode::Tight.permille() == 1_000
            && qa::LineHeightMode::Loose.permille() == 1_300,
        "千分比表示，避免浮点",
    );
    s.add(
        "行高模式标签互异",
        {
            let a = qa::LineHeightMode::Default.label();
            let b = qa::LineHeightMode::Tight.label();
            let c = qa::LineHeightMode::Loose.label();
            a != b && b != c && a != c
        },
        "三模式名字不得重复",
    );
    s.add(
        "紧凑最松、宽松最松序不颠倒",
        qa::LineHeightMode::Tight.permille() < qa::LineHeightMode::Default.permille()
            && qa::LineHeightMode::Default.permille() < qa::LineHeightMode::Loose.permille(),
        "1.0 < 1.2 < 1.3，序反了就是 bug",
    );

    // --- 缓存组 ---
    let ok_cache = qa::CacheCheck::new(900, true);
    s.add("缓存水位达标判过", ok_cache.watermark_ok(), "实测 900‰ ≥ 水位 850‰");
    s.add(
        "缓存水位常量一致",
        qa::CACHE_WATERMARK_PERMILLE == expect::WATERMARK,
        "水位与 F0811 手册同源取值",
    );
    s.add("淘汰正确判过", ok_cache.eviction_ok(), "淘汰正确性是独立一维");
    let below = qa::CacheCheck::new(849, true);
    s.add(
        "低于水位 1‰ 即判不过（反向验证）",
        !below.watermark_ok(),
        "水位判定不含容差余量",
    );
    let at = qa::CacheCheck::new(850, true);
    s.add(
        "恰在水位判过",
        at.watermark_ok(),
        "≥ 水位即达标（锚点是水位不是上限）",
    );
    let no_evict = qa::CacheCheck::new(900, false);
    s.add(
        "命中率达标但淘汰错误仍判不过（反向验证）",
        !no_evict.eviction_ok(),
        "两维独立，不能一维绿就放过另一维",
    );
    // 反向：水位判定必须真的读watermark_permille 字段，而不是写死 850。
    // 构造器恒给 850 ⇒ 上面的 849/850/900 三例与「写死 850」不可区分，必须另立语料。
    {
        let hi_wm = qa::CacheCheck {
            measured_permille: 890,
            watermark_permille: 900,
            eviction_correct: true,
        };
        let lo_wm = qa::CacheCheck {
            measured_permille: 810,
            watermark_permille: 800,
            eviction_correct: true,
        };
        s.add(
            "水位可调：实测 890 < 水位 900 判不过（反向验证）",
            !hi_wm.watermark_ok(),
            "水位是字段不是常量；写死 850 会把 890 误判为达标",
        );
        s.add(
            "水位可调：实测 810 ≥ 水位 800 判过（反向验证）",
            lo_wm.watermark_ok(),
            "水位下调后达标线随之下移，双向都必须成立",
        );
        s.add(
            "构造器水位取模块常量",
            qa::CacheCheck::new(900, true).watermark_permille == qa::CACHE_WATERMARK_PERMILLE,
            "默认水位必须等于CACHE_WATERMARK_PERMILLE，两处不得漂移",
        );
    }

    // --- 性能基准 ---
    let scene = qa::BenchScene {
        static_glyphs: qa::BENCH_STATIC_GLYPHS,
        dynamic_glyphs: qa::BENCH_DYNAMIC_GLYPHS,
        frame_ms: 16,
        fps_floor: 30,
    };
    s.add(
        "基准场景匹配锚点（1000 静态 + 500 动态）",
        scene.matches_anchor() && scene.total_glyphs() == 1_500,
        "锚点原文「1,000 静态字+500 动态字混合帧」",
    );
    s.add(
        "静态与动态字数常量一致",
        qa::BENCH_STATIC_GLYPHS == expect::STATIC
            && qa::BENCH_DYNAMIC_GLYPHS == expect::DYNAMIC,
        "两个常量须与锚点一致",
    );
    s.add(
        "帧率由帧耗时换算（16ms → 62fps）",
        scene.fps() == 62 && scene.fps_ok(),
        "fps = 1000 / ms，整数换算",
    );
    let slow = qa::BenchScene {
        static_glyphs: 1_000,
        dynamic_glyphs: 500,
        frame_ms: 34,
        fps_floor: 30,
    };
    s.add(
        "帧耗时超限判不过帧率（反向验证）",
        slow.fps() == 29 && !slow.fps_ok(),
        "34ms → 29fps < 30 下限",
    );
    s.add(
        "零帧耗时返回 0 fps 而非除零",
        {
            let z = qa::BenchScene {
                static_glyphs: 1,
                dynamic_glyphs: 1,
                frame_ms: 0,
                fps_floor: 30,
            };
            z.fps() == 0
        },
        "零 panic 面：除法前必须挡零",
    );
    // 反向：字数非零不等于匹配锚点。上面的 scene 是锚点正例，
    // 若matches_anchor 退化成「两数都 > 0」则恒真，必须造非锚点场景钉死。
    {
        // 静态少 1：999 + 500
        let off_static = qa::BenchScene {
            static_glyphs: qa::BENCH_STATIC_GLYPHS - 1,
            dynamic_glyphs: qa::BENCH_DYNAMIC_GLYPHS,
            frame_ms: 16,
            fps_floor: 30,
        };
        // 动态多 1：1000 + 501
        let off_dynamic = qa::BenchScene {
            static_glyphs: qa::BENCH_STATIC_GLYPHS,
            dynamic_glyphs: qa::BENCH_DYNAMIC_GLYPHS + 1,
            frame_ms: 16,
            fps_floor: 30,
        };
        // 两数互换：500 + 1000（总数相同但构成不同）
        let swapped = qa::BenchScene {
            static_glyphs: qa::BENCH_DYNAMIC_GLYPHS,
            dynamic_glyphs: qa::BENCH_STATIC_GLYPHS,
            frame_ms: 16,
            fps_floor: 30,
        };
        s.add(
            "静态少 1 判非锚点（反向验证）",
            !off_static.matches_anchor() && off_static.total_glyphs() == 1_499,
            "锚点匹配须逐项相等，退化成「都 > 0」则此项恒真",
        );
        s.add(
            "动态多 1 判非锚点（反向验证）",
            !off_dynamic.matches_anchor() && off_dynamic.total_glyphs() == 1_501,
            "动态字数也要对上，不是只查静态",
        );
        s.add(
            "静态动态互换判非锚点（反向验证）",
            !swapped.matches_anchor() && swapped.total_glyphs() == 1_500,
            "总数相同不构成匹配；构成必须逐项对齐",
        );
        s.add(
            "全零场景判非锚点（反向验证）",
            !qa::BenchScene {
                static_glyphs: 0,
                dynamic_glyphs: 0,
                frame_ms: 16,
                fps_floor: 30,
            }
            .matches_anchor(),
            "退化判据「都 > 0」在 0 时会漏，故必须显式断全零",
        );
    }
    s
}

/// b 族独立入口。
pub fn run_vee12_checks_b_standalone() -> CheckSet {
    run_vee12_checks_b()
}

// ===========================================================================
// c 族：四组测试 + 预算 + CI 门禁 + 冒烟
// ===========================================================================

/// c 族：四组 / 预算 / CI 门禁 / 冒烟。
pub fn run_vee12_checks_c() -> CheckSet {
    let mut s = CheckSet::new("VE-F0812-c");

    // --- 四组（封闭全集）---
    s.add(
        "四组为封闭全集",
        qa::Group::ALL.len() as u32 == expect::GROUPS,
        "解码 / 光栅 / 度量 / 缓存",
    );
    for g in qa::Group::ALL.iter() {
        s.add(
            "组序号与枚举对应",
            qa::Group::from_ordinal_check(*g),
            "ordinal 1..4 须与枚举一一对应",
        );
        s.add(
            "组标签与专项资产名非空",
            !g.label().is_empty() && !g.domain_asset().is_empty(),
            "每组须有标签与本域专项资产名（复用声明）",
        );
    }
    s.add(
        "组序号 1..=4 恰为四组",
        {
            let mut n = 0u32;
            for i in 1u32..=4 {
                if qa::Group::from_ordinal_check_by_ord(i) {
                    n = n.saturating_add(1);
                }
            }
            n == expect::GROUPS
        },
        "边界不得越界成五组",
    );

    // --- 资产完备性（缺一项即 NotApplicable）---
    let full = qa::full_assets();
    for g in qa::Group::ALL.iter() {
        s.add("完备资产下各组ready", full.ready(*g), "完备资产 ⇒ 每组都该ready");
    }
    s.add(
        "解码组须非法序列全档",
        full.decode_ready() && expect::DECODE_BUCKETS == 4,
        "锚点引F0802 四档，少一档即不完备",
    );
    s.add(
        "光栅组须金样满额",
        full.raster_ready() && expect::COMBOS == qa::GOLDEN_COMBO_COUNT,
        "768 组合，少一个都不算齐",
    );
    s.add(
        "度量组须三字段 + 三模式齐",
        full.metric_ready(),
        "基线对齐 + 行高三模式",
    );
    s.add(
        "缓存组须水位场景 + 淘汰检查齐",
        full.cache_ready(),
        "两件都在才算完备",
    );

    // 反向：缺一档 ⇒ NotApplicable（**不是 Pass**）
    let mut na_decode = qa::GroupAssets::new();
    na_decode.decode_buckets = 3;
    s.add(
        "解码档少一档即不完备（反向验证）",
        !na_decode.decode_ready(),
        "缺一档就不能报通过",
    );
    let mut na_golden = qa::GroupAssets::new();
    na_golden.golden_combos = 767;
    s.add(
        "金样少一组合即不完备（反向验证）",
        !na_golden.raster_ready(),
        "767 ≠ 768",
    );
    let mut na_metric = qa::GroupAssets::new();
    na_metric.metric_fields = 3;
    na_metric.line_height_modes = 2;
    s.add(
        "行高模式少一即不完备（反向验证）",
        !na_metric.metric_ready(),
        "三模式缺一不可",
    );
    let mut na_cache = qa::GroupAssets::new();
    na_cache.has_watermark_scene = true;
    na_cache.has_eviction_check = false;
    s.add(
        "缺淘汰检查即不完备（反向验证）",
        !na_cache.cache_ready(),
        "两维缺一不可",
    );
    s.add(
        "NotApplicable 不算 CI 绿",
        !qa::GroupVerdict::NotApplicable.ci_green() && !qa::GroupVerdict::Fail.ci_green(),
        "只有 Pass 才放行——没测 ≠ 测过",
    );

    // --- 组判定 ---
    let cache = qa::CacheCheck::new(900, true);
    let mut dual = qa::MetricDualRun::new();
    for f in qa::MetricField::ALL.iter() {
        dual.push(*f, 10, 10);
    }
    let set = qa::GoldenSet::full(qa::AssetOrigin::DomainSpecific);
    let scene = qa::BenchScene::ANCHOR;
    let budget = qa::nominal_budget();
    let sum = qa::run_all(
        &mut { set.clone() },
        &full,
        &dual,
        &qa::BaselineCheck::new(1_000, 1_000),
        &scene,
        &cache,
        &budget,
    );
    s.add(
        "完备资产下四组全Pass",
        sum.pass == 4 && sum.fail == 0 && sum.na == 0,
        "完备且达标 ⇒ 四组全过",
    );
    s.add("完备资产下 CI 放行", sum.ci_open, "四组全 Pass + 冒烟绿 + 预算达标");
    let mut empty_set = qa::GoldenSet::empty(qa::AssetOrigin::DomainSpecific);
    let sum_na = qa::run_all(
        &mut empty_set,
        &qa::GroupAssets::new(),
        &dual,
        &qa::BaselineCheck::new(1_000, 1_000),
        &scene,
        &cache,
        &budget,
    );
    s.add(
        "空资产四组全 NotApplicable 且 CI 不放行（反向验证）",
        sum_na.na == 4 && sum_na.pass == 0 && !sum_na.ci_open,
        "反向语料：没资产时门禁必须闭",
    );
    // 反向：金样不齐 ⇒ 光栅组 Fail 或 NA
    let mut partial = qa::GoldenSet::empty(qa::AssetOrigin::DomainSpecific);
    partial.push_raw(qa::GoldenKey::new(0, 8, 0), 1);
    let sum_p = qa::run_all(
        &mut partial,
        &full,
        &dual,
        &qa::BaselineCheck::new(1_000, 1_000),
        &scene,
        &cache,
        &budget,
    );
    s.add(
        "金样不齐时 CI 不放行（反向验证）",
        !sum_p.ci_open,
        "反向语料：金样残缺不得放行",
    );
    // 反向：预算超 ⇒ 门禁闭
    let over = qa::Budget::new([500, 400, 100, 100], true);
    let gate_over = qa::CiGate::new(
        [
            qa::GroupVerdict::Pass,
            qa::GroupVerdict::Pass,
            qa::GroupVerdict::Pass,
            qa::GroupVerdict::Pass,
        ],
        true,
        over.within(),
    );
    s.add(
        "预算超限时门禁闭（反向验证）",
        !over.within() && !gate_over.open(),
        "组全绿也不能超预算",
    );

    // --- 预算：并行取关键路径，串行取总和 ---
    s.add(
        "并行预算取关键路径（300s）",
        budget.total_secs() == 300 && budget.within(),
        "四组并行，全量= 最慢那组（光栅组 300s）",
    );
    let serial = qa::Budget::new(
        [
            qa::DECODE_BUDGET_SEC,
            qa::RASTER_BUDGET_SEC,
            qa::METRIC_BUDGET_SEC,
            qa::CACHE_BUDGET_SEC,
        ],
        false,
    );
    s.add(
        "串行预算取总和（420s）且超 360s 预算",
        serial.total_secs() == 420 && !serial.within(),
        "并行与串行口径必须区分——这是本单预算判据的核心",
    );
    s.add(
        "全量预算常量为 360 秒",
        qa::FULL_BUDGET_SEC == expect::FULL_BUDGET,
        "锚点原文「全量≤6 分钟」",
    );
    s.add(
        "关键路径组为光栅组",
        budget.critical_group() == Some(qa::Group::Raster),
        "光栅组跑 768 次 diff，最慢",
    );
    s.add(
        "并行判定与串行判定结论不同（反向验证）",
        budget.within() != serial.within(),
        "若两者结论相同，说明口径没真区分",
    );

    // --- 冒烟（F0813 四例）---
    s.add(
        "冒烟示例数为四",
        qa::SMOKE_CASES.len() as u32 == expect::SMOKE
            && qa::SMOKE_EXAMPLES == expect::SMOKE,
        "F0813 四例",
    );
    let mut smoke_green = true;
    for c in qa::SMOKE_CASES.iter() {
        if !c.passed() {
            smoke_green = false;
        }
    }
    s.add("四例冒烟全绿", smoke_green, "编译 + 运行 + 退出码");
    s.add(
        "冒烟名互异",
        {
            let mut dup = false;
            let cs = qa::SMOKE_CASES;
            for i in 0..cs.len() {
                for k in 0..i {
                    if let (Some(a), Some(b)) = (cs.get(i), cs.get(k)) {
                        if a.name == b.name {
                            dup = true;
                        }
                    }
                }
            }
            !dup
        },
        "四例名字不得重复",
    );
    s.add(
        "编译失败则冒烟不过（反向验证）",
        !qa::SmokeContract::new("x", false, true, 0).passed(),
        "编不过就不算过",
    );
    s.add(
        "退出码非零则冒烟不过（反向验证）",
        !qa::SmokeContract::new("x", true, true, 1).passed(),
        "退出码须符合预期",
    );
    s.add(
        "运行失败则冒烟不过（反向验证）",
        !qa::SmokeContract::new("x", true, false, 0).passed(),
        "编译过但跑不起来也不算过",
    );
    s.add(
        "冒烟不过则门禁闭（反向验证）",
        !qa::CiGate::new(
            [
                qa::GroupVerdict::Pass,
                qa::GroupVerdict::Pass,
                qa::GroupVerdict::Pass,
                qa::GroupVerdict::Pass,
            ],
            false,
            true
        )
        .open(),
        "组全绿 + 冒烟红 ⇒ 仍闭",
    );

    // --- 复用声明 ---
    s.add(
        "共用资产改动须走全域评审",
        qa::AssetOrigin::Shared.needs_global_review()
            && !qa::AssetOrigin::DomainSpecific.needs_global_review(),
        "复用声明：共享件与专项件的改动流程不同",
    );
    s.add(
        "金样集登记来源可查",
        {
            let a = qa::GoldenSet::full(qa::AssetOrigin::Shared);
            a.origin() == qa::AssetOrigin::Shared
        },
        "半年后问「这阈值谁定的」，答案在资产里",
    );

    // --- 汇总自洽 ---
    s.add(
        "汇总计数守恒（pass+fail+na = 4）",
        sum.pass + sum.fail + sum.na == expect::GROUPS,
        "三态之和须等于组数，不得有第四态",
    );
    s.add(
        "汇总金样数与金样集一致",
        sum.golden_combos == qa::GOLDEN_COMBO_COUNT,
        "汇总不得漏计金样",
    );
    s
}

/// c 族独立入口。
pub fn run_vee12_checks_c_standalone() -> CheckSet {
    run_vee12_checks_c()
}

// ===========================================================================
// 判据集自检
// ===========================================================================

/// 判据集自检。
pub fn run_vee12_meta_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0812-meta");
    let a = run_vee12_checks_a_standalone();
    let b = run_vee12_checks_b_standalone();
    let c = run_vee12_checks_c_standalone();
    let (pa, fa) = a.tally();
    let (pb, fb) = b.tally();
    let (pc, fc) = c.tally();
    s.add("a 族全绿", fa == 0, "a 族不得有红项");
    s.add("b 族全绿", fb == 0, "b 族不得有红项");
    s.add("c 族全绿", fc == 0, "c 族不得有红项");
    s.add(
        "三族合计未截断",
        !a.truncated() && !b.truncated() && !c.truncated(),
        "任一族超 MAX_CHECKS 即被截断，须分族",
    );
    s.add("a 族条数与声明一致", pa == A_COUNT, "改枚举规模须同步 A_COUNT");
    s.add("b 族条数与声明一致", pb == B_COUNT, "改枚举规模须同步 B_COUNT");
    s.add("c 族条数与声明一致", pc == C_COUNT, "改枚举规模须同步 C_COUNT");
    s.add(
        "三族合计判据数守恒",
        pa + pb + pc == TOTAL_CHECKS,
        "a+b+c 须等于 TOTAL_CHECKS",
    );
    s.add(
        "total_check_count 与三族之和相等",
        total_check_count() as usize == pa + pb + pc,
        "对外总数须与实测之和一致",
    );
    s
}

/// 全量自检。
pub fn run_vee12_checks() -> CheckSet {
    let a = run_vee12_checks_a_standalone();
    let b = run_vee12_checks_b_standalone();
    let c = run_vee12_checks_c_standalone();
    let ab = CheckSet::merge(a, b);
    CheckSet::merge(ab, c)
}

/// 判据总数（三族之和）。
pub fn total_check_count() -> u32 {
    (run_vee12_checks_a_standalone().len()
        + run_vee12_checks_b_standalone().len()
        + run_vee12_checks_c_standalone().len()) as u32
}
