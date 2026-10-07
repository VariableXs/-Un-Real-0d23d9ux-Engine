//! VE-F0215 域自检（判据逐条映射锚点）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0215`
//!
//! 判据设计纪律（承 F0213/F0214 教训）：
//! 1. **不得用表内元素验表内函数**——查清单/统计这类必须用表外真实形态。
//! 2. **阈值不得同时充当预期值**——规格常量另用**锚点字面量**断言一次。
//! 3. **断言两侧在测试点上不得同值**——测试点要选「正确与错误实现
//!    结果不同」的那一点（如「恰好等于配额」而非「远超配额」）。
//! 4. **同源驱动恒真**——若被测两处由同一 bool 驱动，须绕过聚合层
//!    直接断言被测字段，或手工构造矛盾态。
//! 5. **等价/不可达变体不算漏网**——变体必须真改变行为路径且成本可承受。

use crate::checks::CheckSet;
use crate::svstar2::veb15_suite::*;

use alloc::string::String;

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格锚点字面量（**不引用实现常量**，否则改常量=改预期，自证）
// ---------------------------------------------------------------------------

/// 锚点「三层用例」——层数 3。
const ANCHOR_LAYERS: usize = 3;
/// 锚点「用例清单」——用例数按册内明细，16 条。
const ANCHOR_CASES: usize = 16;
/// 锚点「恢复层（注入错误验证 F0212 处置）」——恢复层必须有独立用例。
const ANCHOR_RECOVERY_MIN: usize = 4;
/// 锚点「fast 档」——宿主侧分钟级，须含协议与功能两层。
const ANCHOR_FAST_MIN: usize = 12;
/// 锚点「恢复层只进 full 档」的对照：full 档用例数。
const ANCHOR_FULL_MIN: usize = 4;

// ---------------------------------------------------------------------------
// 一、三层用例（锚点「三层用例」）
// ---------------------------------------------------------------------------

fn c215_layers() -> Vec<(&'static str, bool)> {
    let mut v = Vec::new();
    // 层数对齐锚点（三层不多不少）
    v.push((
        "C215-协议-层数为3",
        ANCHOR_LAYERS == 3 && Layer::Protocol.index() != Layer::Functional.index(),
    ));
    // **三层都要有真实用例**（不是只建枚举）
    v.push((
        "C215-协议-三层各有用例",
        layer_case_count(Layer::Protocol) > 0
            && layer_case_count(Layer::Functional) > 0
            && layer_case_count(Layer::Recovery) > 0,
    ));
    v.push((
        "C215-协议-清单总数对齐锚点",
        CASE_COUNT == ANCHOR_CASES,
    ));
    // 恢复层用例数达锚点下限（锚点单列了恢复层）
    v.push((
        "C215-恢复-恢复层用例数达锚点",
        layer_case_count(Layer::Recovery) >= ANCHOR_RECOVERY_MIN,
    ));
    // 三层用例数之和 == 总数（无游离条目）
    v.push((
        "C215-协议-三层之和等于总数",
        layer_case_count(Layer::Protocol)
            + layer_case_count(Layer::Functional)
            + layer_case_count(Layer::Recovery)
            == CASE_COUNT,
    ));
    // 层名可判读且互异（输出用）
    v.push((
        "C215-协议-层名互异可判读",
        Layer::Protocol.name() != Layer::Functional.name()
            && Layer::Functional.name() != Layer::Recovery.name()
            && Layer::Protocol.name() != Layer::Recovery.name(),
    ));
    // 层下标落在 [0,3)（越界即打乱按层统计）
    let mut i = 0;
    let mut idx_ok = true;
    while i < CASE_COUNT {
        if CASES[i].layer.index() >= ANCHOR_LAYERS {
            idx_ok = false;
        }
        i += 1;
    }
    v.push(("C215-协议-层下标落在范围内", idx_ok));
    // 每条用例断言数非零（零断言的用例什么都没测）
    let mut assert_ok = true;
    i = 0;
    while i < CASE_COUNT {
        if CASES[i].asserts == 0 {
            assert_ok = false;
        }
        i += 1;
    }
    v.push(("C215-协议-用例断言数非零", assert_ok));
    // 用例号唯一（重号会让结果记录张冠李戴）
    let mut ids_ok = true;
    i = 0;
    while i < CASE_COUNT {
        let mut j = i + 1;
        while j < CASE_COUNT {
            if CASES[i].id == CASES[j].id {
                ids_ok = false;
            }
            j += 1;
        }
        i += 1;
    }
    v.push(("C215-协议-用例号唯一", ids_ok));
    // 用例名唯一（复现序列按名定位）
    let mut names_ok = true;
    i = 0;
    while i < CASE_COUNT {
        let mut j = i + 1;
        while j < CASE_COUNT {
            if CASES[i].name == CASES[j].name {
                names_ok = false;
            }
            j += 1;
        }
        i += 1;
    }
    v.push(("C215-协议-用例名唯一", names_ok));
    v
}

// ---------------------------------------------------------------------------
// 二、双档运行（锚点「fast 档与 full 档」）
// ---------------------------------------------------------------------------

fn c215_tiers() -> Vec<(&'static str, bool)> {
    let mut v = Vec::new();
    // 档位名可判读且互异
    v.push((
        "C215-双档-档名互异",
        Tier::Fast.name() != Tier::Full.name(),
    ));
    // **full 是全集**（锚点「full 档可过夜批跑」），fast 是子集。
    // 初版把档位与层绑死（fast=协议+功能、full=恢复），导致 full
    // 档漏跑协议与功能 —— 那不是「两个投影」，是把清单劈成两半。
    v.push((
        "C215-双档-full档为全集",
        tier_case_count(Tier::Full) == CASE_COUNT,
    ));
    v.push((
        "C215-双档-fast档为full档真子集",
        tier_case_count(Tier::Fast) < tier_case_count(Tier::Full),
    ));
    // fast 档跳过的用例数 == 仅 full 的用例数
    let mut only_full = 0;
    let mut i0 = 0;
    while i0 < CASE_COUNT {
        if !Tier::Fast.includes(&CASES[i0]) {
            only_full += 1;
        }
        i0 += 1;
    }
    v.push((
        "C215-双档-fast跳过数等于仅full数",
        CASE_COUNT - tier_case_count(Tier::Fast) == only_full && only_full > 0,
    ));
    // fast 档达锚点下限
    v.push((
        "C215-双档-fast档用例数达锚点",
        tier_case_count(Tier::Fast) >= ANCHOR_FAST_MIN,
    ));
    // full 档达锚点下限（恢复层在这里）
    v.push((
        "C215-双档-full档用例数达锚点",
        tier_case_count(Tier::Full) >= ANCHOR_FULL_MIN,
    ));
    // fast 档覆盖协议与功能两层（宿主侧要能跑这两层）
    let mut fast = Suite::new(feature::ALL);
    let rep = fast.run(Tier::Fast, |_c| (V_PASS, String::new()));
    let pf = rep.layer_results(Layer::Protocol).len();
    let ff = rep.layer_results(Layer::Functional).len();
    v.push((
        "C215-双档-fast档覆盖协议与功能",
        pf > 0 && ff > 0 && rep.layer_results(Layer::Recovery).is_empty(),
    ));
    // full 档覆盖全部三层（fast ⊂ full，不是两份独立清单）
    let mut full = Suite::new(feature::ALL);
    let rep2 = full.run(Tier::Full, |_c| (V_PASS, String::new()));
    v.push((
        "C215-双档-full档覆盖三层",
        rep2.layer_results(Layer::Protocol).len() > 0
            && rep2.layer_results(Layer::Functional).len() > 0
            && rep2.layer_results(Layer::Recovery).len() > 0,
    ));
    // fast 是 full 的真子集（投影关系）
    v.push((
        "C215-双档-fast档是full档子集",
        rep2.results.len() > rep.results.len(),
    ));
    // 恢复层**不进 fast**（锚点说恢复层验注入错误，宿主侧模拟不出）
    v.push((
        "C215-恢复-恢复层不进fast档",
        !Layer::Recovery.in_fast()
            && Layer::Protocol.in_fast()
            && Layer::Functional.in_fast(),
    ));
    v
}

// ---------------------------------------------------------------------------
// 三、N/A 语义（锚点「设备缺特性→跳过并标 N/A 不算失败」）
// ---------------------------------------------------------------------------

fn c215_na() -> Vec<(&'static str, bool)> {
    let mut v = Vec::new();
    // 最小设备（只有 2D）跑 fast：带高级特性的用例全 N/A
    let mut s = Suite::new(feature::MINIMAL);
    let rep = s.run(Tier::Fast, |_c| (V_PASS, String::new()));
    // N/A 不算失败
    v.push((
        "C215-NA-缺特性不算失败",
        rep.failed() == 0,
    ));
    // N/A 单列统计，且**不计入通过数**（否则统计说谎）
    v.push((
        "C215-NA-NA单列不计通过",
        rep.na() > 0 && rep.passed() + rep.na() == rep.executed() + rep.na(),
    ));
    // 已执行数 = 通过 + 失败（不含 N/A）
    v.push((
        "C215-NA-已执行数不含NA",
        rep.executed() == rep.passed() + rep.failed(),
    ));
    // 无特性需求的用例（requires_feature == 0）**不得被标 N/A**
    let mut always_run = true;
    let mut i = 0;
    while i < CASE_COUNT {
        if CASES[i].tier == Tier::Fast && CASES[i].requires_feature == 0 {
            // 该用例在最小设备上也必须真跑（用 run_one 验证它不返 NA）
            let mut s2 = Suite::new(feature::MINIMAL);
            let vd = s2.run_one(Tier::Fast, CASES[i].id, |_c| (V_PASS, String::new()));
            if vd.is_na() {
                always_run = false;
            }
        }
        i += 1;
    }
    v.push(("C215-NA-无需求用例不标NA", always_run));
    // NA 判定本身：三值枚举语义
    v.push((
        "C215-NA-三值语义互斥",
        V_PASS.is_pass() && V_FAIL.is_fail() && V_NA.is_na()
            && !V_NA.is_fail() && !V_NA.is_pass()
            && !V_PASS.is_fail() && !V_FAIL.is_pass(),
    ));
    // 整体结论不受 NA 影响（零失败 + 清理过 = 通过）
    v.push((
        "C215-NA-NA不影响整体结论",
        rep.ok(),
    ));
    // N/A 用例**不建资源**（否则收尾对账被「建了又销了」掩盖泄漏）。
    //
    // **判别式在「绝对值」不在「净值」**：只断 `created == destroyed`
    // 会被「N/A 也建了再销了」骗过——建 1 销 1 两边同样相等，账平，
    // 而 N/A 路径本应**一个资源都不碰**。故此处按判据侧**独立重算**
    // 「本档真跑的用例数」（含 fast 档 + 特性齐备者），并断
    // `created()` **恰等于**该数：N/A 用例若偷偷建销，created 会多出
    // N/A 条数，转红。
    //
    // 预期数由判据自己数（遍历 CASES 判档位与特性），**不读**实现的
    // 任何计数器——否则两侧同源，改实现时一起变，恒真。
    let mut expect_ran: u32 = 0;
    let mut ei = 0;
    while ei < CASE_COUNT {
        if CASES[ei].tier == Tier::Fast && s.has_feature(CASES[ei].requires_feature) {
            expect_ran += 1;
        }
        ei += 1;
    }
    v.push((
        "C215-NA-NA用例不建资源",
        s.ledger().created() == expect_ran,
    ));
    // 佐证：跑完的用例建销成对（净值口径，作为**补充**而非主判据）。
    // 主判据是上面的绝对值——净值这条在 N/A 偷建销时仍会绿，
    // 故不可单独承担「不建资源」的判定。
    v.push((
        "C215-NA-建销成对",
        s.ledger().created() == s.ledger().destroyed(),
    ));
    // **反向对照**：全特性设备上「只跑一条 N/A 用例」时，
    // `run_one` 的 N/A 早退路径必须**零建销**。这条直接打早退分支
    // （`run` 的 N/A 分支由上面的绝对值判据覆盖），是 M12 类变体
    // ——「N/A 判定不短路，继续建/销一轮」——的唯一 catcher。
    {
        // 找一条需要 fast 档之外特性的用例（最小设备上必 N/A）
        let mut na_id: Option<u32> = None;
        let mut fi = 0;
        while fi < CASE_COUNT {
            if !Suite::new(feature::MINIMAL).has_feature(CASES[fi].requires_feature) {
                na_id = Some(CASES[fi].id);
                break;
            }
            fi += 1;
        }
        match na_id {
            None => v.push(("C215-NA-run_one的NA路径零建销", false)),
            Some(id) => {
                let mut sx = Suite::new(feature::MINIMAL);
                let vd = sx.run_one(Tier::Full, id, |_c| (V_PASS, String::new()));
                v.push((
                    "C215-NA-run_one的NA路径零建销",
                    vd.is_na() && sx.ledger().created() == 0 && sx.ledger().destroyed() == 0,
                ));
            }
        }
    }
    // **零需求恒可用**：设备特性为 0 时 `requires_feature == 0` 的用例
    // 仍必须能跑（否则一台无特性设备会把全部用例标 N/A，测试形同虚设）。
    let s_none = Suite::new(0);
    let mut zero_ok = true;
    let mut i2 = 0;
    while i2 < CASE_COUNT {
        if CASES[i2].requires_feature == 0 && !s_none.has_feature(CASES[i2].requires_feature) {
            zero_ok = false;
        }
        i2 += 1;
    }
    v.push(("C215-NA-零需求在无特性设备上仍可用", zero_ok && s_none.has_feature(0)));
    // **多位特性须全部具备**：只给一位不得判具备
    // （变体把 `(d & bit) == bit` 改成 `!= 0` 时，此处转红）
    let s_half = Suite::new(feature::BIT_2D);
    let multi = feature::BIT_2D | feature::BIT_VIRGL;
    v.push((
        "C215-NA-多位特性须全部具备",
        !s_half.has_feature(multi) && s_half.has_feature(feature::BIT_2D),
    ));
    // 零特性位也不得判具备
    let s_zero = Suite::new(0);
    v.push((
        "C215-NA-无特性设备不具备任何真特性",
        !s_zero.has_feature(feature::BIT_2D) && !s_zero.has_feature(feature::BIT_RESET),
    ));
    // Verdict 三态可判读（输出用）
    v.push((
        "C215-NA-三态判定函数自洽",
        V_PASS.is_pass() != V_PASS.is_na() && V_NA.is_na() != V_NA.is_pass(),
    ));
    v
}

// ---------------------------------------------------------------------------
// 四、黄金流（锚点「命令流编码用黄金流比对（对齐 F0206 版本戳）」）
// ---------------------------------------------------------------------------

fn c215_golden() -> Vec<(&'static str, bool)> {
    let mut v = Vec::new();
    // 空库
    let empty = GoldenSet::new();
    v.push(("C215-黄金流-空库无条目", empty.is_empty() && empty.len() == 0));

    // 一致 ⇒ Match
    let mut g = GoldenSet::new();
    g.add("case-a", 7, alloc::vec![1u8, 2, 3, 4]);
    let same = EncodedFlow { version: 7, bytes: alloc::vec![1u8, 2, 3, 4] };
    v.push((
        "C215-黄金流-版本与字节一致判Match",
        g.compare("case-a", &same) == FlowVerdict::Match,
    ));

    // 字节不同 ⇒ BytesDiffer（版本相同）
    let diff = EncodedFlow { version: 7, bytes: alloc::vec![1u8, 2, 3, 5] };
    v.push((
        "C215-黄金流-字节不同判差异",
        g.compare("case-a", &diff) == FlowVerdict::BytesDiffer,
    ));

    // 版本不同 ⇒ **VersionMismatch 且阻断**（不报字节差异——布局都变了）
    let mism = EncodedFlow { version: 8, bytes: alloc::vec![9u8, 9, 9, 9] };
    v.push((
        "C215-黄金流-版本不符判阻断",
        {
            let fv = g.compare("case-a", &mism);
            fv == FlowVerdict::VersionMismatch && fv.is_blocking() && !fv.is_match()
        },
    ));
    // 未登记用例 ⇒ 差异（不得静默通过）
    let missing = EncodedFlow { version: 7, bytes: alloc::vec![1u8, 2, 3, 4] };
    v.push((
        "C215-黄金流-未登记用例不静默通过",
        g.compare("case-zzz", &missing) == FlowVerdict::BytesDiffer,
    ));
    // 查得到
    v.push((
        "C215-黄金流-按名可查",
        g.find("case-a").is_some() && g.find("nope").is_none(),
    ));
    // 多条版本戳体检（跨用例一致性）
    let mut g2 = GoldenSet::new();
    g2.add("x", 7, alloc::vec![0u8]);
    g2.add("y", 8, alloc::vec![0u8]);
    v.push((
        "C215-黄金流-版本戳不齐可检出",
        !g2.versions_uniform(),
    ));
    let mut g3 = GoldenSet::new();
    g3.add("x", 7, alloc::vec![0u8]);
    g3.add("y", 7, alloc::vec![0u8]);
    v.push((
        "C215-黄金流-版本戳齐时体检通过",
        g3.versions_uniform(),
    ));
    // 空库体检不报不齐（没有样本 ≠ 不一致）
    v.push((
        "C215-黄金流-空库体检不误报",
        empty.versions_uniform(),
    ));
    // **重建必须显式确认**（锚点：重建黄金流需人工确认）
    let mut g4 = GoldenSet::new();
    g4.add("r", 7, alloc::vec![1u8]);
    let refused = g4.rebuild("r", 8, alloc::vec![2u8], false);
    v.push((
        "C215-黄金流-未确认拒绝重建",
        !refused && g4.find("r").unwrap().version == 7,
    ));
    let ok = g4.rebuild("r", 8, alloc::vec![2u8], true);
    v.push((
        "C215-黄金流-确认后重建生效",
        ok && g4.find("r").unwrap().version == 8 && g4.len() == 1,
    ));
    // 重建不新增条目（避免每次版本变更都膨胀）
    v.push((
        "C215-黄金流-重建不增条目",
        g4.len() == 1,
    ));
    v
}

// ---------------------------------------------------------------------------
// 五、版本阻断联动（锚点「版本不匹配→阻断」）
// ---------------------------------------------------------------------------

fn c215_version() -> Vec<(&'static str, bool)> {
    let mut v = Vec::new();
    let mut s = Suite::new(feature::ALL);
    v.push(("C215-版本-初始未阻断", !s.blocked()));
    // 阻断后用例判失败（而非继续比噪声）
    s.set_blocked(true);
    v.push(("C215-版本-阻断位可置", s.blocked()));
    let vd = s.run_one(Tier::Fast, 1, |_c| (V_PASS, String::new()));
    v.push((
        "C215-版本-阻断后不判通过",
        vd.is_fail(),
    ));
    // 解除阻断后恢复
    s.set_blocked(false);
    let vd2 = s.run_one(Tier::Fast, 1, |_c| (V_PASS, String::new()));
    v.push((
        "C215-版本-解除阻断后恢复",
        vd2.is_pass(),
    ));
    // 阻断态下整档全判失败
    let mut s2 = Suite::new(feature::ALL);
    s2.set_blocked(true);
    let rep = s2.run(Tier::Fast, |_c| (V_PASS, String::new()));
    v.push((
        "C215-版本-阻断态整档失败",
        rep.failed() as usize == tier_case_count(Tier::Fast),
    ));
    // 阻断态的复现序列指向阻断源（可诊断）
    let mut s3 = Suite::new(feature::ALL);
    s3.set_blocked(true);
    let rep3 = s3.run(Tier::Fast, |_c| (V_PASS, String::new()));
    let tagged = rep3
        .results
        .iter()
        .all(|r| r.verdict.is_fail() && r.repro == "blocked-by-version-mismatch");
    v.push(("C215-版本-阻断失败带复现标记", tagged));
    v
}

// ---------------------------------------------------------------------------
// 六、清理断言（锚点「测试资源泄漏→退出前清理断言」）
// ---------------------------------------------------------------------------

fn c215_cleanup() -> Vec<(&'static str, bool)> {
    let mut v = Vec::new();
    // 无泄漏时清理通过
    let mut s = Suite::new(feature::ALL);
    let rep = s.run(Tier::Fast, |_c| (V_PASS, String::new()));
    v.push((
        "C215-清理-无泄漏清理通过",
        s.finish() && rep.cleanup_ok == Some(true),
    ));
    // 收尾步**独立于用例**（整体结论里含清理项）
    v.push((
        "C215-清理-整体结论含清理",
        rep.ok(),
    ));
    // **未跑收尾步**（`cleanup_ok == None`）时整体**不得**判通过——
    // 否则「还没收尾」会被当成「收尾通过」，变体把 `Some(true)` 换成
    // `!= Some(false)` 就全绿。
    let rep_no = RunReport::new(Tier::Fast);
    v.push((
        "C215-清理-未跑收尾步不判通过",
        rep_no.cleanup_ok.is_none() && !rep_no.ok(),
    ));
    // 收尾失败态整体不通过
    let mut rep_bad = RunReport::new(Tier::Fast);
    rep_bad.cleanup_ok = Some(false);
    v.push((
        "C215-清理-收尾失败态不判通过",
        !rep_bad.ok(),
    ));
    // 制造泄漏 ⇒ 清理断言转红，且整体不通过
    let mut s2 = Suite::new(feature::ALL);
    s2.leak(3);
    v.push((
        "C215-清理-泄漏可检出",
        !s2.finish() && s2.ledger().leaked() == 3,
    ));
    // 泄漏时整档报告判不通过
    let mut s3 = Suite::new(feature::ALL);
    s3.leak(2);
    let rep3 = s3.run(Tier::Fast, |_c| (V_PASS, String::new()));
    v.push((
        "C215-清理-泄漏使整体不通过",
        !rep3.ok(),
    ));
    // 账破坏（销毁多于创建）⇒ 失败而非「清理干净」
    let mut led = ResourceLedger::new();
    led.create();
    led.destroy();
    led.destroy();
    v.push((
        "C215-清理-过度销毁判失败",
        // 自由函数与 Suite 方法必须**同结论**——但要用**同一本账**比。
        // （先前误写成拿 `Suite::new(0)` 的空账与 `led` 比，两者本就不同
        //  结论，判据必红；这属判据写错，不是被测物缺陷。）
        led.over_destroyed()
            && led.leaked() == 0
            && !finish_ledger(&led)
            && {
                // 同账代入：把 `led` 的内容搬进一个 Suite 再验方法面。
                let mut s_led = Suite::new(0);
                let mut i = 0;
                while i < led.created() {
                    s_led.ledger_mut().create();
                    i += 1;
                }
                let mut j = 0;
                while j < led.destroyed() {
                    s_led.ledger_mut().destroy();
                    j += 1;
                }
                s_led.finish() == finish_ledger(&led)
            },
    ));
    // 空账清理通过
    let s4 = Suite::new(feature::ALL);
    v.push(("C215-清理-空账清理通过", s4.finish()));
    // 账读写一致
    let mut led2 = ResourceLedger::new();
    led2.create();
    led2.create();
    led2.destroy();
    v.push((
        "C215-清理-账计数对账",
        led2.created() == 2 && led2.destroyed() == 1 && led2.leaked() == 1,
    ));
    v
}

// ---------------------------------------------------------------------------
// 七、复现序列（锚点「失败输出含复现最小命令序列」）
// ---------------------------------------------------------------------------

fn c215_repro() -> Vec<(&'static str, bool)> {
    let mut v = Vec::new();
    // 失败用例带复现序列（按用例声明的断言数给步数）
    let mut s = Suite::new(feature::ALL);
    let rep = s.run(Tier::Full, |c| {
        if c.id == 13 {
            (V_FAIL, repro_sequence(c.id, c.asserts))
        } else {
            (V_PASS, String::new())
        }
    });
    let fails = rep.failures_with_repro();
    v.push((
        "C215-复现-失败项带复现序列",
        fails.len() == 1 && !fails[0].repro.is_empty(),
    ));
    // 复现序列含用例号与步数
    let r0 = fails.first().map(|f| f.repro.clone()).unwrap_or_default();
    v.push((
        "C215-复现-序列含用例号",
        r0.contains("case:13"),
    ));
    // 复现序列步数来自用例断言数（可照着敲一遍）
    let case13 = find_case(13);
    let expect_steps = case13.map(|c| c.asserts).unwrap_or(0);
    v.push((
        "C215-复现-步数来自用例断言数",
        r0 == repro_sequence(13, expect_steps),
    ));
    // **注入器给通过项也返回非空串**时，实现不得把它写进记录。
    // 变体「通过项也带复现噪声」在此转红——原先的判据用注入器返回
    // 空串做对照，与「无条件赋值」同值 ⇒ 恒真弱门禁。
    let mut s_noise = Suite::new(feature::ALL);
    let rep_noise = s_noise.run(Tier::Fast, |c| {
        if c.id == 2 {
            (V_FAIL, repro_sequence(c.id, c.asserts))
        } else {
            // 通过项**故意**返回非空串
            (V_PASS, repro_sequence(c.id, c.asserts))
        }
    });
    let mut pass_clean = true;
    let mut i3 = 0;
    while i3 < rep_noise.results.len() {
        if rep_noise.results[i3].verdict.is_pass() && !rep_noise.results[i3].repro.is_empty() {
            pass_clean = false;
        }
        i3 += 1;
    }
    v.push(("C215-复现-通过项即使注入器返串也不记", pass_clean));
    // 通过用例**不带**复现序列（不产噪声）
    let mut noise = true;
    let mut i = 0;
    while i < rep.results.len() {
        if rep.results[i].verdict.is_pass() && !rep.results[i].repro.is_empty() {
            noise = false;
        }
        i += 1;
    }
    v.push(("C215-复现-通过项不带序列", noise));
    // 复现序列文本规范统一（都含 "case:" 前缀）
    let mut uniform = true;
    i = 0;
    while i < rep.results.len() {
        if rep.results[i].verdict.is_fail() && !rep.results[i].repro.contains("case:") {
            uniform = false;
        }
        i += 1;
    }
    v.push(("C215-复现-输出文本规范统一", uniform));
    v
}

// ---------------------------------------------------------------------------
// 八、结果记录（锚点「结果记录」）
// ---------------------------------------------------------------------------

fn c215_report() -> Vec<(&'static str, bool)> {
    let mut v = Vec::new();
    let mut s = Suite::new(feature::ALL);
    let rep = s.run(Tier::Fast, |_c| (V_PASS, String::new()));
    // 记录条数 == 本档用例数
    v.push((
        "C215-记录-条数等于本档用例数",
        rep.results.len() == tier_case_count(Tier::Fast),
    ));
    // 断言总数 = 各用例断言数之和
    let mut sum = 0u32;
    let mut i = 0;
    while i < rep.results.len() {
        sum += rep.results[i].asserts as u32;
        i += 1;
    }
    v.push((
        "C215-记录-断言总数对账",
        rep.asserts_total() == sum && sum > 0,
    ));
    // 未知用例号在**判据层**判失败（原先只有单测覆盖，判据层缺）
    let mut s_unk = Suite::new(feature::ALL);
    v.push((
        "C215-记录-未知用例号判失败",
        s_unk.run_one(Tier::Fast, 9999, |_c| (V_PASS, String::new())).is_fail(),
    ));
    // 三态之和 == 条数（无第四态、无重复计数）
    v.push((
        "C215-记录-三态之和等于条数",
        rep.passed() + rep.failed() + rep.na() == rep.results.len() as u32,
    ));
    // 档位被记录（输出用）
    v.push((
        "C215-记录-档位被记录",
        rep.tier == Some(Tier::Fast),
    ));
    // 按层取结果可用
    v.push((
        "C215-记录-可按层取结果",
        rep.layer_results(Layer::Protocol).len() == layer_case_count(Layer::Protocol),
    ));
    // **用例名被真实消费**：执行名清单与档位投影一致（死字段反证：
    // 若 executed_names 恒空，说明 `Case::name` 没有读取面）。
    let names = rep.executed_names();
    let expect = RunReport::case_names_in_tier(Tier::Fast);
    v.push((
        "C215-记录-用例名清单与投影一致",
        names.len() == expect.len() && !names.is_empty(),
    ));
    // 清单里每条都能按号取回结果（记录不张冠李戴）
    let mut by_id_ok = true;
    let mut i1 = 0;
    while i1 < rep.results.len() {
        let cid = rep.results[i1].case_id;
        match rep.result_of(cid) {
            Some(r) if r.case_id == cid => {}
            _ => by_id_ok = false,
        }
        i1 += 1;
    }
    v.push(("C215-记录-可按用例号取回", by_id_ok));
    // 取不存在的用例号返None（不panic）
    v.push(("C215-记录-取不存在用例返None", rep.result_of(9999).is_none()));
    // 档位条数按记录自带字段核（与清单投影互相印证）
    v.push((
        "C215-记录-档位条数与投影一致",
        rep.count_in_tier(Tier::Fast) as usize == tier_case_count(Tier::Fast),
    ));
    // 混合三态的统计：1 失败 2 NA
    let mut s2 = Suite::new(feature::MINIMAL);
    let rep2 = s2.run(Tier::Fast, |c| {
        if c.id == 2 {
            (V_FAIL, String::from("x"))
        } else if c.requires_feature == 0 && c.id != 2 {
            (V_PASS, String::new())
        } else {
            (V_PASS, String::new())
        }
    });
    v.push((
        "C215-记录-失败被计入",
        rep2.failed() >= 1 && !rep2.ok(),
    ));
    v
}

/// 跑完 VE-F0215 全部判据。
pub fn run_veb15_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-veb15");
    for group in [
        c215_layers,
        c215_tiers,
        c215_na,
        c215_golden,
        c215_version,
        c215_cleanup,
        c215_repro,
        c215_report,
    ] {
        for (name, passed) in group() {
            set.add(name, passed, "");
        }
    }
    set
}
