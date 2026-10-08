//! VE-F3002 · 域自检（判据逐条对应，见 `vep02_lang.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 四原则（节奏/呼吸/因果/克制）→ `P02-原则-*`
//! - 四级时长（微反馈/组件转场/页面转场/复杂编排）→ `P02-分级-*`
//! - 语义化缓动（名→曲线+语义，禁裸曲线）→ `P02-缓动-*`
//! - 内建 reduce（分级表含 reduced 档，非外挂）→ `P02-降级-*`
//! - 单源取值（P02/P03 与令牌从本册取值）→ `P02-单源-*`
//! - 判据（总纲自证可追溯）→ `P02-判据-*`
//! - 注意力预算（同时活动元素 ≤3）→ `P02-预算-*`
//! - 时长白名单（超预算→警告+白名单流程）→ `P02-白名单-*`
//! - 原则冲突裁决记录（降级矩阵第三格）→ `P02-裁决-*`
//! - 读屏替述（无障碍替述与语言册同源）→ `P02-读屏-*`
//! - 错误路径零静默（五元组 + 拒绝计数显性）→ `P02-错误-*`
//!
//! 零墙钟、零 IO，回归可复现。**空集判「无数据」而非「通过」**——
//! 任何一处枚举塌成空集时，对应断言必须转红。

use super::vep02_lang::*;

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::ToString;
use alloc::vec::Vec;

/// 判据一：四原则各有可失败的判定口径。
///
/// 本组自检的核心是「**违反必须可检出**」——只查原则齐备是查不出执行体
/// 失效的，所以每条原则都造一个真实违反样本，断言它被检出。
fn chk_principles(set: &mut CheckSet) {
    let lang = MotionLanguage::standard();

    // 四原则齐备且码唯一。
    set.add(
        "P02-原则-四条齐备",
        PRINCIPLE_ORDER.len() == PRINCIPLE_COUNT && PRINCIPLE_ORDER.len() == Principle::ALL.len(),
        "原则数须等于 PRINCIPLE_COUNT 与Principle::ALL",
    );
    let mut codes: Vec<&str> = PRINCIPLE_ORDER.iter().map(|p| p.code()).collect();
    codes.sort_unstable();
    let mut uniq = codes.clone();
    uniq.dedup();
    set.add(
        "P02-原则-码唯一",
        uniq.len() == codes.len() && !codes.is_empty(),
        "原则码重复会让对拍台账指向两条原则",
    );
    // 三项合一为一项（逐条用 assert! 硬断言，出错即定位到具体原则）：
    // 口径非空 + 服务判据已登记 + 枚举往返。
    let mut per_principle_ok = true;
    let mut bad_principle = "";
    for p in PRINCIPLE_ORDER.iter() {
        if p.verdict_rule().trim().is_empty() {
            per_principle_ok = false;
            bad_principle = p.zh();
        }
        if p.serves().is_empty() {
            per_principle_ok = false;
            bad_principle = p.zh();
        }
        if Principle::from_code(p.code()) != Some(*p) {
            per_principle_ok = false;
            bad_principle = p.zh();
        }
    }
    set.add(
        "P02-原则-逐条口径与服务齐备",
        per_principle_ok && !PRINCIPLE_ORDER.is_empty(),
        "每条原则都须有判定口径、声明所服务的判据、且枚举往返成立",
    );
    assert!(per_principle_ok, "原则 {} 缺口径/缺服务/往返失败", bad_principle);
    set.add(
        "P02-原则-伪码拒绝",
        Principle::from_code("P02-P0").is_none(),
        "未知原则码必须返回 None而不是回落到第一项",
    );

    // 违反形态一：越级时长（60ms 不落任何一级）。
    let v_bad_ms = lang.audit_principles(Scene::ButtonPress, 60, None, false, 1);
    set.add(
        "P02-原则-越级时长可检出",
        v_bad_ms.iter().any(|v| v.principle == Principle::Rhythm),
        "60ms 不落四级任一区间，节奏原则须判违反",
    );
    assert!(
        v_bad_ms.iter().any(|v| v.principle == Principle::Rhythm),
        "越级时长未被检出：{:?}",
        v_bad_ms.iter().map(|v| v.screen_line()).collect::<Vec<_>>()
    );

    // 违反形态二：进退不同级（入 120ms/退 250ms）。
    let v_asym = lang.audit_principles(Scene::ButtonPress, 120, Some(250), false, 1);
    set.add(
        "P02-原则-进退不同级可检出",
        v_asym.iter().any(|v| {
            v.principle == Principle::Rhythm && v.symptom.contains("进退不对称")
        }),
        "进 120ms（微反馈）退 250ms（组件转场）须判进退不对称",
    );

    // 违反形态三：整级单一且无性格（BreathInput 手构，避免依赖册内状态）。
    let uniform = BreathInput {
        distinct: 1,
        sole_family: Some(EasingFamily::Standard),
    };
    let v_uniform = Principle::Breath.verdict(
        Scene::ButtonPress.zh(),
        120,
        None,
        uniform_enter(uniform),
        uniform_enter(uniform),
        uniform,
        false,
        1,
    );
    set.add(
        "P02-原则-一律-ease-可检出",
        v_uniform.iter().any(|v| v.principle == Principle::Breath),
        "整级只有一种且属标准族即「一律 ease」，须判违反",
    );
    assert!(
        v_uniform.iter().any(|v| v.principle == Principle::Breath),
        "一律 ease 未被检出"
    );

    // 反例（**必须不判违反**，否则本原则会把好设计错杀）：单一但有性格。
    let characterful = BreathInput {
        distinct: 1,
        sole_family: Some(EasingFamily::Elastic),
    };
    let v_char = Principle::Breath.verdict(
        Scene::RouteChange.zh(),
        350,
        None,
        "spring-soft",
        "spring-soft",
        characterful,
        false,
        1,
    );
    set.add(
        "P02-原则-单一有性格不误判",
        !v_char.iter().any(|v| v.principle == Principle::Breath),
        "页面转场共用一条弹簧是有性格，不是没性格，不得判违反",
    );
    assert!(
        !v_char.iter().any(|v| v.principle == Principle::Breath),
        "有性格的单一曲线被错杀：{:?}",
        v_char.iter().map(|v| v.screen_line()).collect::<Vec<_>>()
    );

    // 违反形态四：纯装饰（元素未变却动）。
    let v_deco = lang.audit_principles(Scene::ButtonPress, 120, None, true, 1);
    set.add(
        "P02-原则-纯装饰可检出",
        v_deco.iter().any(|v| v.principle == Principle::Causality),
        "元素未发生位移/尺寸/可见性变化却动起来即判违反",
    );

    // 违反形态五：同时活动元素超预算。
    let v_over = lang.audit_principles(
        Scene::ButtonPress,
        120,
        None,
        false,
        MAX_CONCURRENT_MOTIONS + 1,
    );
    set.add(
        "P02-原则-并发超预算可检出",
        v_over.iter().any(|v| v.principle == Principle::Restraint),
        "同时活动元素超 3 即判违反（注意力预算红线）",
    );

    // 边界：恰好等于预算是合规的（严一分就是误杀）。
    let v_at = lang.audit_principles(
        Scene::ButtonPress,
        120,
        None,
        false,
        MAX_CONCURRENT_MOTIONS,
    );
    set.add(
        "P02-原则-并发恰好达预算不误判",
        !v_at.iter().any(|v| v.principle == Principle::Restraint),
        "并发数恰等于上限须合规；严一分就是误杀",
    );

    // 正样本：标准场景全原则零违反。
    let v_ok = lang.audit_principles(
        Scene::ButtonPress,
        Scene::ButtonPress.recommended_ms(),
        None,
        false,
        1,
    );
    set.add(
        "P02-原则-标准场景零违反",
        v_ok.is_empty(),
        "按钮按下取推荐值且非装饰时不许有违反",
    );
    assert!(
        v_ok.is_empty(),
        "标准场景误报：{:?}",
        v_ok.iter().map(|v| v.screen_line()).collect::<Vec<_>>()
    );

    // 每条违反都要能念给用户听（异常零静默）。
    let all_v = v_bad_ms;
    set.add(
        "P02-原则-违反项可读屏",
        all_v
            .iter()
            .all(|v| !v.screen_line().trim().is_empty() && !v.advice.trim().is_empty()),
        "违反项须带现象与建议；缺建议等于让人自己猜",
    );

    // 册内派生口径与手构口径必须一致（口径不许两处各写一遍）。
    let derived = lang.easings.breath_input(DurationTier::PageTransition);
    set.add(
        "P02-原则-呼吸口径单源派生",
        derived.distinct == 1 && derived.sole_family == Some(EasingFamily::Elastic),
        "页面转场在册缓动应为 1 支弹簧（弹性族）",
    );
}

/// 取「一律 ease」判定用的曲线名（标准族进入曲线）。
fn uniform_enter(b: BreathInput) -> &'static str {
    if b.sole_family == Some(EasingFamily::Standard) {
        DurationTier::MicroFeedback.enter_easing()
    } else {
        DurationTier::PageTransition.enter_easing()
    }
}

/// 判据二：四级时长分级表。
fn chk_tiers(set: &mut CheckSet) {
    set.add(
        "P02-分级-四级齐备",
        DurationTier::ALL.len() == TIER_COUNT && TIER_ORDER.len() == TIER_COUNT,
        "分级数须等于 TIER_COUNT",
    );

    let mut codes: Vec<&str> = TIER_ORDER.iter().map(|t| t.code()).collect();
    codes.sort_unstable();
    let mut uniq = codes.clone();
    uniq.dedup();
    set.add("P02-分级-码唯一", uniq.len() == codes.len(), "分级码重复会让反查歧义");

    // 区间不得倒挂、须单调递增（of() 的反查依赖单调性）。
    let mut ordered = true;
    let mut prev_max = 0u32;
    for t in DurationTier::ALL.iter() {
        if t.min_ms() > t.max_ms() || (prev_max != 0 && t.min_ms() < prev_max) {
            ordered = false;
        }
        prev_max = t.max_ms();
    }
    set.add(
        "P02-分级-区间不倒挂且单调递增",
        ordered,
        "分级须按时长递增；倒挂会让任何时长都被判越界",
    );

    // 区间有序（允许边界重合）：锚点字面给「组件转场 200-300 / 页面转场
    // 300-450」，两级在 300ms 处重合一个点——这是锚点如此，不是实现走样。
    // 真正要防的是**交叠一段**（那会让 of() 的归属取决于遍历序）。
    let mut ordered_nonempty = true;
    for w in DurationTier::ALL.windows(2) {
        if w[1].min_ms() < w[0].max_ms() {
            ordered_nonempty = false;
        }
    }
    set.add(
        "P02-分级-区间有序不交叠",
        ordered_nonempty,
        "后一级下界不得早于前一级上界；交叠一段会让of() 的归属取决于遍历序",
    );
    assert!(ordered_nonempty, "分级区间交叠了一段");

    // 300ms 边界归属已裁决（锚点两级在 300ms 重合）：按就近上界归组件转场。
    set.add(
        "P02-分级-300ms-归属已裁决",
        DurationTier::of_strict(300) == Some(DurationTier::ComponentTransition)
            && DurationTier::of_strict(301) == Some(DurationTier::PageTransition),
        "锚点两级在 300ms 重合；裁决为归组件转场（涨到 300 不该中途改判级别）",
    );
    assert_eq!(
        DurationTier::of_strict(300),
        Some(DurationTier::ComponentTransition),
        "300ms 边界归属与裁决不符"
    );
    assert_eq!(
        DurationTier::of_strict(301),
        Some(DurationTier::PageTransition)
    );

    // 锚点原文的四个数字必须逐项对上，不许"差不多"。
    set.add(
        "P02-分级-锚点数值逐项对齐",
        DurationTier::MicroFeedback.min_ms() == 100
            && DurationTier::MicroFeedback.max_ms() == 150
            && DurationTier::ComponentTransition.min_ms() == 200
            && DurationTier::ComponentTransition.max_ms() == 300
            && DurationTier::PageTransition.min_ms() == 300
            && DurationTier::PageTransition.max_ms() == 450
            && DurationTier::ComplexOrchestra.max_ms() == 500,
        "锚点：微反馈 100-150 / 组件转场 200-300 / 页面转场 300-450 / 复杂编排 500 上限",
    );
    assert!(
        DurationTier::MicroFeedback.min_ms() == 100
            && DurationTier::MicroFeedback.max_ms() == 150
            && DurationTier::ComponentTransition.min_ms() == 200
            && DurationTier::ComponentTransition.max_ms() == 300
            && DurationTier::PageTransition.min_ms() == 300
            && DurationTier::PageTransition.max_ms() == 450
            && DurationTier::ComplexOrchestra.max_ms() == 500,
        "分级数值与锚点不符"
    );

    set.add(
        "P02-分级-复杂编排是上限档",
        DurationTier::ComplexOrchestra.is_ceiling()
            && !DurationTier::PageTransition.is_ceiling(),
        "上限档只有一个：超出即须白名单而不是「下一级」",
    );
    assert!(DurationTier::ComplexOrchestra.is_ceiling(), "复杂编排必须是上限档");

    // 分级断言：四种拒绝形态各自可检出。
    assert!(DurationTier::MicroFeedback.assert_in_range(0).is_err(), "零时长须被拒");
    assert!(
        DurationTier::MicroFeedback.assert_in_range(80).is_err(),
        "低于下界须被拒"
    );
    assert!(
        DurationTier::MicroFeedback.assert_in_range(200).is_err(),
        "超出本级上界须被拒"
    );
    assert!(
        DurationTier::ComplexOrchestra.assert_in_range(COMPLEX_ORCHESTRA_MAX_MS + 1).is_err(),
        "上限档再往上没有级别可退，须走白名单"
    );
    set.add(
        "P02-分级-四种越界形态可检出",
        DurationTier::MicroFeedback.assert_in_range(0).is_err()
            && DurationTier::MicroFeedback.assert_in_range(80).is_err()
            && DurationTier::MicroFeedback.assert_in_range(200).is_err()
            && DurationTier::ComplexOrchestra
                .assert_in_range(COMPLEX_ORCHESTRA_MAX_MS + 1)
                .is_err(),
        "零时长/低于下界/超出上界/超上限档 四种形态须逐一可检出",
    );

    // 区间内须通过，且返回值等于入参（不做隐式改写）。
    set.add(
        "P02-分级-区间内原值返回",
        DurationTier::ComponentTransition.assert_in_range(250) == Ok(250)
            && DurationTier::PageTransition.assert_in_range(350) == Ok(350),
        "落在区间内须原值返回；隐式改写会让调用方算错时长",
    );

    // 错误五元组：拒绝必须给出路。
    let e_low = DurationTier::MicroFeedback.assert_in_range(80).unwrap_err();
    let e_high = DurationTier::MicroFeedback.assert_in_range(200).unwrap_err();
    let e_zero = DurationTier::MicroFeedback.assert_in_range(0).unwrap_err();
    let e_ceiling = DurationTier::ComplexOrchestra
        .assert_in_range(COMPLEX_ORCHESTRA_MAX_MS + 1)
        .unwrap_err();
    set.add(
        "P02-分级-拒绝五元组齐备",
        e_low.is_complete() && e_high.is_complete() && e_ceiling.is_complete(),
        "拒绝必须带码/现象/原因/下一步/责任方，缺 next 等于让人猜",
    );
    assert!(e_low.is_complete() && e_high.is_complete() && e_ceiling.is_complete(), "错误五元组不全");
    set.add(
        "P02-分级-上限档错误指向白名单",
        e_ceiling.code == E_TIER_ABOVE_RANGE && e_ceiling.next.contains("白名单"),
        "超上限档的下一步必须是走白名单，不是「下一级」",
    );
    assert!(
        e_ceiling.next.contains("白名单"),
        "上限档的下一步未指向白名单：{}",
        e_ceiling.next
    );
    set.add(
        "P02-分级-零时长独立错误码",
        e_zero.code == E_TIER_ZERO_DURATION
            && e_zero.code != e_low.code
            && e_zero.is_complete(),
        "零时长要单独成码：它会让分级断言永远有特例",
    );
    assert_eq!(
        e_zero.code, E_TIER_ZERO_DURATION,
        "零时长未使用独立错误码（实为 {}）", e_zero.code
    );

    // of() 反查：O(1) 且落缝隙取最近一级（不返回 None 让调用方自己猜）。
    set.add(
        "P02-分级-毫秒反查命中本级",
        DurationTier::of(120) == Some(DurationTier::MicroFeedback)
            && DurationTier::of(250) == Some(DurationTier::ComponentTransition)
            && DurationTier::of(350) == Some(DurationTier::PageTransition)
            && DurationTier::of(500) == Some(DurationTier::ComplexOrchestra),
        "反查须命中本级",
    );
    assert!(
        DurationTier::of(120) == Some(DurationTier::MicroFeedback)
            && DurationTier::of(350) == Some(DurationTier::PageTransition),
        "of() 反查失准"
    );
    set.add(
        "P02-分级-零值反查为空",
        DurationTier::of(0).is_none() && DurationTier::of_strict(0).is_none(),
        "0ms 不属任何一级，须返回 None",
    );
    // 严格反查：越级值不得被就近归属「洗白」——这是红线的判定依据。
    set.add(
        "P02-分级-严格反查不洗白越级",
        DurationTier::of_strict(60).is_none()
            && DurationTier::of_strict(160).is_none()
            && DurationTier::of_strict(900).is_none()
            && DurationTier::of_strict(120) == Some(DurationTier::MicroFeedback)
            && DurationTier::of_strict(350) == Some(DurationTier::PageTransition),
        "of() 会就近归属，of_strict() 不会；合规判定只能用后者",
    );
    assert!(DurationTier::of_strict(60).is_none(), "严格反查把越级值洗白了");
    assert!(DurationTier::of(60).is_some(), "就近反查应仍能给出该退到哪一级");
    set.add(
        "P02-分级-缝隙取最近一级",
        DurationTier::of(160) == Some(DurationTier::MicroFeedback)
            && DurationTier::of(180) == Some(DurationTier::ComponentTransition),
        "160ms 是「微反馈慢了点」，反查该给微反馈而不是判越级",
    );

    // 每级都要有适用场景（分级不得是空壳）。
    let mut all_covered = true;
    for t in DurationTier::ALL.iter() {
        if t.scenes().is_empty() || t.enter_easing().is_empty() || t.exit_easing().is_empty() {
            all_covered = false;
        }
    }
    set.add(
        "P02-分级-每级有适用清单与进出缓动",
        all_covered,
        "空级等于没有这一级；分级表不许有占位项",
    );

    // 分级表可念（读屏替述的原料）。
    set.add(
        "P02-分级-分级表可读屏",
        DurationTier::ALL
            .iter()
            .all(|t| t.screen_line().contains(t.zh()) && t.screen_line().contains("reduce")),
        "每级读屏行须含中文名与 reduce 档",
    );
    let mut tier_rt_ok = true;
    for (i, t) in DurationTier::ALL.iter().enumerate() {
        if DurationTier::from_code(t.code()) != Some(*t) || t.rank() != i {
            tier_rt_ok = false;
        }
    }
    set.add(
        "P02-分级-逐条枚举往返与rank",
        tier_rt_ok,
        "from_code 须能回到原枚举项，且 rank 与序一致",
    );
    assert!(tier_rt_ok, "分级枚举往返或 rank 与序不一致");
    set.add(
        "P02-分级-伪码拒绝",
        DurationTier::from_code("DT-NOPE").is_none(),
        "未知分级码须返回 None",
    );
}

/// 判据三：语义化缓动（名→曲线+语义，禁裸曲线）。
fn chk_easings(set: &mut CheckSet) {
    let lang = MotionLanguage::standard();

    set.add(
        "P02-缓动-登记册非空",
        lang.easings.len() == standard_easings().len() && lang.easings.len() > 0,
        "标准册须登满标准种子表",
    );
    assert_eq!(lang.easings.len(), standard_easings().len());

    // 三族全覆盖（空族即缺陷）。
    set.add(
        "P02-缓动-三族全覆盖",
        lang.easings.uncovered_families().is_empty(),
        "标准/弹性/强调任一族为空即等于没有这一族",
    );
    assert!(
        lang.easings.uncovered_families().is_empty(),
        "缺族：{:?}",
        lang.easings.uncovered_families()
    );
    let mut family_ok = true;
    for f in EasingFamily::ALL.iter() {
        if lang.easings.family_count(*f) == 0 || EasingFamily::from_code(f.code()) != Some(*f) {
            family_ok = false;
        }
    }
    set.add(
        "P02-缓动-逐族非空且往返",
        family_ok,
        "三族每族在册条数须大于零，且from_code 往返成立",
    );
    assert!(family_ok, "有族为空或枚举往返失败");

    // 册内每条三段齐备且非裸曲线。
    let mut complete = true;
    let mut bare = 0usize;
    for spec in lang.easings.iter() {
        if !spec.is_complete() {
            complete = false;
        }
        if spec.is_bare_curve() {
            bare += 1;
        }
    }
    set.add(
        "P02-缓动-册内三段齐备",
        complete,
        "名/曲线/语义缺一即不合格；缺语义的名字只是标签",
    );
    set.add(
        "P02-缓动-册内无裸曲线",
        bare == 0,
        "册内出现裸曲线即语义化红线失守",
    );
    assert_eq!(bare, 0, "册内混入裸曲线 {} 支", bare);

    // 裸曲线识别：三种写法都要能认出来。
    let bare_names = ["cubic-bezier(0.4, 0, 0.2, 1)", "0.4,0,0.2,1", "   "];
    let mut bare_ok = true;
    for n in bare_names.iter() {
        let probe = EasingSpec {
            name: n.to_string(),
            family: EasingFamily::Standard,
            curve: "曲线".to_string(),
            semantics: "语义".to_string(),
            enter: true,
        };
        if !probe.is_bare_curve() {
            bare_ok = false;
        }
    }
    set.add(
        "P02-缓动-裸曲线识别三种写法",
        bare_ok,
        "cubic-bezier(...) / 纯数值串 / 空白名 三种都要判为裸曲线",
    );
    assert!(bare_ok, "裸曲线识别漏判");

    // admit 的五项拒绝逐一可检出 + 计数显性。
    let mk = |name: &str, curve: &str, sem: &str| EasingSpec {
        name: name.to_string(),
        family: EasingFamily::Standard,
        curve: curve.to_string(),
        semantics: sem.to_string(),
        enter: true,
    };

    let mut reg = EasingRegistry::standard();
    let r_bare = reg.admit(mk("cubic-bezier(0.1, 0, 0.2, 1)", "c1", "语义"));
    set.add(
        "P02-缓动-拒裸曲线",
        r_bare.is_err() && r_bare.as_ref().err().map(|e| e.code) == Some(E_EASING_BARE_CURVE),
        "裸曲线入册须被拒并给出语义化替代路径",
    );
    assert_eq!(
        r_bare.as_ref().err().map(|e| e.code),
        Some(E_EASING_BARE_CURVE)
    );
    assert_eq!(reg.rejected_bare(), 1, "裸曲线被拒次数须显性计数");

    let long_name = "e".repeat(MAX_EASING_NAME_LEN + 1);
    let r_long = reg.admit(mk(&long_name, "c", "语义"));
    set.add(
        "P02-缓动-拒名字过长",
        r_long.as_ref().err().map(|e| e.code) == Some(E_EASING_NAME_TOO_LONG),
        "名字过长须被拒：名字要能在日志与台账里一眼读完",
    );
    assert_eq!(
        r_long.as_ref().err().map(|e| e.code),
        Some(E_EASING_NAME_TOO_LONG)
    );

    let r_dup = reg.admit(mk("ease-out-enter", "另一条曲线", "另一份语义"));
    set.add(
        "P02-缓动-拒重名",
        r_dup.as_ref().err().map(|e| e.code) == Some(E_EASING_DUP),
        "同名两支会让「查名取参」的结果取决于查了哪条",
    );
    assert_eq!(r_dup.as_ref().err().map(|e| e.code), Some(E_EASING_DUP));

    let r_inc = reg.admit(mk("no-semantics-here", "曲线", ""));
    set.add(
        "P02-缓动-拒字段缺失",
        r_inc.as_ref().err().map(|e| e.code) == Some(E_EASING_INCOMPLETE),
        "语义缺失须被拒",
    );
    assert_eq!(r_inc.as_ref().err().map(|e| e.code), Some(E_EASING_INCOMPLETE));

    // 册满拒绝（把册填到上限）。
    let mut full = EasingRegistry::new();
    let mut filled = 0usize;
    loop {
        let n = format!("filler-easing-{}", filled);
        if full.admit(mk(&n, "曲线", "语义")).is_err() {
            break;
        }
        filled += 1;
        if filled > MAX_EASINGS + 4 {
            break;
        }
    }
    let r_cap = full.admit(mk("one-too-many", "曲线", "语义"));
    set.add(
        "P02-缓动-拒册满",
        r_cap.as_ref().err().map(|e| e.code) == Some(E_EASING_CAP) && full.len() == MAX_EASINGS,
        "登记册须有上限；无上限的册迟早被临时条目灌满",
    );
    assert_eq!(
        r_cap.as_ref().err().map(|e| e.code),
        Some(E_EASING_CAP),
        "册满拒绝码不对"
    );
    assert_eq!(full.len(), MAX_EASINGS, "册满时长度须恰为上限");

    // 五项拒绝的五元组齐备。
    let mut tuples_ok = true;
    for e in [r_bare, r_long, r_dup, r_inc, r_cap].into_iter() {
        match e {
            Ok(_) => tuples_ok = false,
            Err(err) => {
                if !err.is_complete() {
                    tuples_ok = false;
                }
            }
        }
    }
    set.add(
        "P02-缓动-拒绝五元组齐备",
        tuples_ok,
        "登记拒绝的五项都须带下一步与责任方",
    );
    assert!(tuples_ok, "登记拒绝的五元组不全");

    // 名查参O(1)：命中、未命中、命中值与册内一致。
    let hit = lang.easings.spec("ease-out-enter");
    set.add(
        "P02-缓动-名查参命中",
        hit.is_some() && hit.map(|s| s.curve.clone()) == Some(lang.resolve_easing("ease-out-enter").unwrap_or("").to_string()),
        "查名取参须命中且与册内曲线一致",
    );
    assert!(lang.easings.spec("ease-out-enter").is_some());
    set.add(
        "P02-缓动-名查参未命中为空",
        lang.easings.spec("no-such-easing").is_none(),
        "未在册的名字须返回 None",
    );

    // 语义化缓动的红线本体：单源取值入口不许绕过登记册。
    let r = lang.resolve_easing("ease-out-enter");
    let r_missing = lang.resolve_easing("no-such-easing");
    set.add(
        "P02-缓动-单源取值拒绝未在册",
        r.is_ok() && r_missing.is_err(),
        "未在册的缓动取值须被拒",
    );
    assert!(r_missing.is_err(), "未在册缓动取值未被拒");
    if let Err(err) = r_missing {
        assert!(err.is_complete(), "取值拒绝五元组不全");
        set.add(
            "P02-缓动-取值拒绝五元组齐备",
            err.is_complete() && err.next.contains("登记"),
            "取值拒绝须指出路：改名登记或改用已登记名",
        );
    }

    // 弹性三档语义与 reduce 降级目标。
    set.add(
        "P02-缓动-弹性三档齐备",
        SpringFeel::ALL.len() == 3
            && SpringFeel::from_name(SpringFeel::Snappy.name()) == Some(SpringFeel::Snappy)
            && SpringFeel::from_name(SpringFeel::Soft.name()) == Some(SpringFeel::Soft)
            && SpringFeel::from_name(SpringFeel::Bouncy.name()) == Some(SpringFeel::Bouncy),
        "弹性档位 snappy/soft/bouncy 三档须齐备",
    );
    assert_eq!(SpringFeel::ALL.len(), 3);
    set.add(
        "P02-缓动-弹性格有视觉说明",
        SpringFeel::ALL
            .iter()
            .all(|s| !s.visual_note().trim().is_empty() && !s.code().trim().is_empty()),
        "档位须有语义说明与码，否则只是三个标签",
    );
    // reduce 态：bouncy 必须降级（抖动在 reduce 下是真问题），另两档保持。
    set.add(
        "P02-缓动-reduce-态弹性格降级",
        SpringFeel::Bouncy.reduced_target() == SpringFeel::Soft
            && SpringFeel::Snappy.reduced_target() == SpringFeel::Snappy
            && SpringFeel::Soft.reduced_target() == SpringFeel::Snappy,
        "reduce 态抖动须降为 soft；soft 再收敛一档到 snappy；snappy 已是最低档保持",
    );
    assert_eq!(
        SpringFeel::Bouncy.reduced_target(),
        SpringFeel::Soft,
        "bouncy 在 reduce 态须降为 soft"
    );
    assert_eq!(
        SpringFeel::Soft.reduced_target(),
        SpringFeel::Snappy,
        "soft 在 reduce 态须再收敛到 snappy"
    );
    set.add(
        "P02-缓动-降级标记自洽",
        SpringFeel::Bouncy.reduced_target() != SpringFeel::Bouncy
            && SpringFeel::Snappy.reduced_target() == SpringFeel::Snappy,
        "bouncy 须降级而 snappy 保持；两者方向相反，标记须互补",
    );

    // 册内摘要必须念出裸曲线被拒次数（那是被拦下的诱惑）。
    let txt = lang.easings.screen_text();
    set.add(
        "P02-缓动-册摘要含拒绝计数",
        txt.contains("裸曲线被拒") && txt.contains("族"),
        "册摘要须念出三族分布与裸曲线被拒次数",
    );
    assert!(txt.contains("裸曲线被拒"), "册摘要未含拒绝计数：{}", txt);

    // 逐条读屏行可念。
    set.add(
        "P02-缓动-登记项可读屏",
        lang.easings
            .iter()
            .all(|s| s.screen_line().contains(&s.name) && s.screen_line().contains("族")),
        "登记项读屏行须含名字与族别",
    );
}

/// 判据四：内建 reduce（降级矩阵第一格：超预算时长→警告+白名单流程）。
fn chk_reduce_built_in(set: &mut CheckSet) {
    // 四级全部内建 reduce 档，且 reduce 态时长归零。
    let mut all_built_in = true;
    for t in DurationTier::ALL.iter() {
        if !t.has_reduced_band() || t.reduced_ms() != REDUCED_DURATION_MS {
            all_built_in = false;
        }
    }
    set.add(
        "P02-降级-四级内建-reduce-档",
        all_built_in,
        "reduced 档须建在分级表里；让消费方自己写 if reduce 必漏一处",
    );
    assert!(all_built_in, "有级别缺内建 reduce 档");

    // 「无障碍不是外挂」的可执行证据：reduce 档时长为 0 但分级表本身不变
    // ——即 reduce 是在分级表内的一个档位，而不是另建一套表。
    set.add(
        "P02-降级-reduce-不另建表",
        DurationTier::ALL
            .iter()
            .all(|t| t.reduced_ms() <= t.min_ms() && DurationTier::from_code(t.code()) == Some(*t)),
        "reduce 是同一张分级表里的档位；另建一套表等于分叉",
    );

    // 单源对账里的 reduce 一致性（分级缺档会被对账抓出来）。
    let lang = MotionLanguage::standard();
    let audit = lang.audit_single_source();
    set.add(
        "P02-降级-单源对账无缺档",
        audit.no_reduced_band.is_empty(),
        "分级表缺 reduce 档须被单源对账检出",
    );
    assert!(audit.no_reduced_band.is_empty());

    // 每个场景都要有 reduce 态行为说明（内建到场景级，不只到分级级）。
    let mut scene_notes = true;
    for s in Scene::all_scenes() {
        let n = s.reduced_note();
        if !n.contains("reduce") {
            scene_notes = false;
        }
    }
    set.add(
        "P02-降级-每场景有reduce-说明",
        scene_notes,
        "分级级reduce 档之外，场景级还要说清 reduce 态到底怎么变",
    );
    assert!(scene_notes, "有场景缺 reduce 态说明");

    // 场景推荐值三件套必须带 reduce 时长。
    let rec = lang.scene_recommendation(Scene::ButtonPress, 120, 0);
    set.add(
        "P02-降级-推荐值含reduce-时长",
        rec.reduced_ms == REDUCED_DURATION_MS,
        "单源取值交付形态须带 reduce 态时长，消费方不必自己算",
    );
    assert_eq!(rec.reduced_ms, REDUCED_DURATION_MS);

    // 分级读屏行必须念出 reduce 档（无障碍信息不许只在文档里）。
    set.add(
        "P02-降级-读屏行念reduce-档",
        DurationTier::ALL
            .iter()
            .all(|t| t.screen_line().contains(&format!("{}ms", REDUCED_DURATION_MS))),
        "分级读屏行须念出 reduce 档时长",
    );
    for t in DurationTier::ALL.iter() {
        assert!(
            t.screen_line().contains(&format!("{}ms", REDUCED_DURATION_MS)),
            "{}读屏行未念 reduce 档：{}",
            t.zh(),
            t.screen_line()
        );
    }
}

/// 判据五：单源取值（P02/P03 与令牌从本册取值，双向对账）。
fn chk_single_source(set: &mut CheckSet) {
    let lang = MotionLanguage::standard();

    // 场景全集 = 8 常规 + 2 编排级（锚点括注歧义的裁决落点）。
    set.add(
        "P02-单源-场景全集十项",
        Scene::ALL.len() == SCENE_COUNT && Scene::EXTENDED.len() == 2 && Scene::all_scenes().len() == 10,
        "锚点括注列 8 常规场景，分级表覆盖 10 场景；本册拆为8+2 并全量遍历",
    );
    assert_eq!(Scene::all_scenes().len(), 10);
    set.add(
        "P02-单源-两集合不相交",
        !Scene::ALL
            .iter()
            .any(|s| Scene::EXTENDED.contains(s)),
        "编排级两场景不得同时出现在常规集合里（否则单源对账会双计）",
    );
    assert!(
        !Scene::ALL.iter().any(|s| Scene::EXTENDED.contains(s)),
        "ALL 与 EXTENDED 有交集"
    );

    // 双向对账之一：场景 → 族须在册。
    let audit = lang.audit_single_source();
    set.add(
        "P02-单源-场景引用全在册",
        audit.unregistered.is_empty(),
        "适用清单引用了不存在的缓动名 = 单源分叉",
    );
    assert!(
        audit.unregistered.is_empty(),
        "未在册引用：{:?}",
        audit.unregistered
    );

    // 双向对账之二：族 → 场景须有使用（无主缓动）。
    set.add(
        "P02-单源-检出无主缓动",
        !audit.ownerless.is_empty(),
        "册内留了两支弹性档（snappy/bouncy）作备用档；\
         它们当前无场景直用，单源对账须如实报出来而不是当没这回事",
    );
    assert!(
        !audit.ownerless.is_empty(),
        "无主缓动应被检出（snappy/bouncy 为预留档位）"
    );
    assert_eq!(
        audit.ownerless.len(),
        2,
        "无主缓动应为两支预留弹性档，实为 {:?}",
        audit.ownerless
    );
    set.add(
        "P02-单源-无主缓动是弹性预留档",
        audit
            .ownerless
            .iter()
            .all(|n| lang.easings.spec(n).map(|s| s.family) == Some(EasingFamily::Elastic)),
        "无主的两支须是弹性族预留档；若是别族的，说明册内漏了适用清单",
    );

    // 无主是警告不是阻断（否则标准册永远放不了行）。
    set.add(
        "P02-单源-无主缓动不阻断放行",
        lang.preflight().is_empty() && lang.warnings().contains(&E_EASING_NO_SCENE),
        "预留档位属警告级：门只挡真缺陷，账照样留痕",
    );
    assert!(
        lang.preflight().is_empty(),
        "标准语言册preflight 非空：{:?}",
        lang.preflight()
    );
    assert!(
        lang.warnings().contains(&E_EASING_NO_SCENE),
        "无主缓动须走警告通道：{:?}",
        lang.warnings()
    );

    // 对账结论可读屏。
    let at = audit.screen_text();
    set.add(
        "P02-单源-对账结论可读屏",
        at.contains("未在册引用") && at.contains("无主缓动") && at.contains("reduce"),
        "对账结论三项计数都要念出来",
    );
    assert!(at.contains("无主缓动"), "对账结论未念无主缓动：{}", at);

    // 单源取值交付形态：三件套齐备且两族都已登记。
    let mut rec_ok = true;
    for s in Scene::all_scenes() {
        let rec = lang.scene_recommendation(s, s.recommended_ms(), 0);
        if !rec.is_clean() || !rec.enter_registered || !rec.exit_registered {
            rec_ok = false;
        }
    }
    set.add(
        "P02-单源-十场景推荐值全合规",
        rec_ok,
        "每个场景按推荐时长取值都须完全合规（时长+两族已登记）",
    );
    assert!(rec_ok, "有场景取推荐值仍不合规");

    // 越界时长由白名单账裁，不由调用方猜。
    let over = lang.scene_recommendation(Scene::ButtonPress, 900, 0);
    set.add(
        "P02-单源-越界给最近合规值",
        over.verdict == DurationVerdict::OutOfRange {
            tier: DurationTier::MicroFeedback,
            nearest: 150,
        } && over.effective_ms == 150,
        "900ms 越出微反馈级须给最近合规值 150ms，而不是原样放行",
    );
    assert_eq!(over.effective_ms, 150, "越界未夹到最近合规值");

    // 推荐三件套读屏行含全部要素。
    let rl = over.screen_line();
    set.add(
        "P02-单源-推荐值可读屏",
        rl.contains("生效") && rl.contains("推荐") && rl.contains("已登记") && rl.contains("reduce"),
        "推荐值读屏行须含生效/推荐/登记态/reduce 四要素",
    );
    assert!(rl.contains("reduce"), "推荐值读屏行缺 reduce：{}", rl);

    // 场景推荐时长取分级中值而非上限（默认短是红线）。
    let rec_btn = lang.scene_recommendation(Scene::ButtonPress, 120, 0);
    set.add(
        "P02-单源-推荐时长取中值",
        rec_btn.recommended_ms == 125
            && rec_btn.recommended_ms < DurationTier::MicroFeedback.max_ms(),
        "推荐值取分级中值而非上限：给上限等于鼓励「能慢就慢」",
    );
    assert_eq!(rec_btn.recommended_ms, 125);

    // 场景码唯一且往返。
    let scodes: Vec<&str> = Scene::all_scenes().iter().map(|s| s.code()).collect();
    let mut suniq = scodes.clone();
    suniq.sort_unstable();
    suniq.dedup();
    set.add(
        "P02-单源-场景码唯一",
        suniq.len() == scodes.len() && scodes.len() == 10,
        "场景码重复会让适用清单反查歧义",
    );
    assert_eq!(suniq.len(), 10);
    let scene_rt_ok = Scene::all_scenes()
        .iter()
        .all(|s| Scene::from_code(s.code()) == Some(*s));
    set.add(
        "P02-单源-场景枚举往返",
        scene_rt_ok,
        "from_code 必须能回到原枚举项（含编排级两场景）",
    );
    assert!(scene_rt_ok, "场景枚举往返失败");
    set.add(
        "P02-单源-场景伪码拒绝",
        Scene::from_code("SC-NOPE").is_none(),
        "未知场景码须返回 None",
    );
    set.add(
        "P02-单源-场景行可读屏",
        Scene::all_scenes()
            .iter()
            .all(|s| s.screen_line().contains(s.zh()) && s.screen_line().contains("reduce")),
        "场景读屏行须含中文名与 reduce 行为",
    );
    set.add(
        "P02-单源-场景推荐缓动已登记",
        Scene::all_scenes().iter().all(|s| {
            lang.easings.spec(s.enter_easing()).is_some()
                && lang.easings.spec(s.exit_easing()).is_some()
        }),
        "全部十场景的进出缓动名都须已在册",
    );
}

/// 判据六：判据本身自证可追溯。
fn chk_criteria(set: &mut CheckSet) {
    set.add(
        "P02-判据-六项齐备",
        CRITERIA.len() == 6,
        "锚点判据列六项，本总纲须一项不缺",
    );
    assert_eq!(CRITERIA.len(), 6);

    let mut codes: Vec<&str> = CRITERIA.iter().map(|c| c.code()).collect();
    codes.sort_unstable();
    let mut uniq = codes.clone();
    uniq.dedup();
    set.add(
        "P02-判据-码唯一",
        uniq.len() == codes.len(),
        "判据码重复会让对拍台账指向两条判据",
    );

    for c in CRITERIA.iter() {
        // 承诺句非空 + 无软词 + 执行体已指名 + 枚举往返，四项合并为一项
        // （逐条 assert! 保证出错即定位到具体判据）。
        let soft = ["应该", "尽量", "最好", "可能会"];
        let mut bad = "";
        if c.promise().trim().is_empty() {
            bad = c.zh();
        }
        for w in soft.iter() {
            if c.promise().contains(w) {
                bad = c.zh();
            }
        }
        if c.enforced_by().trim().is_empty() {
            bad = c.zh();
        }
        if Criterion::from_code(c.code()) != Some(*c) {
            bad = c.zh();
        }
        set.add(
            "P02-判据-逐条承诺与执行体齐备",
            bad.is_empty(),
            "承诺句非空且无软词、执行体已指名、枚举往返成立",
        );
        assert!(bad.is_empty(), "判据 {} 的承诺句/执行体/往返有缺", bad);
    }
    set.add(
        "P02-判据-伪码拒绝",
        Criterion::from_code("P02-J0").is_none(),
        "未知判据码须返回 None",
    );

    // 执行体须真的存在（本册函数名可在册内查到）。
    let bodies = [
        "Principle::verdict",
        "DurationTier::assert_in_range",
        "EasingRegistry::admit",
        "DurationTier::reduced_ms",
        "MotionLanguage::audit_single_source",
        "self_audit",
    ];
    let mut bodies_ok = true;
    for b in bodies.iter() {
        if !CRITERIA.iter().any(|c| c.enforced_by().contains(b)) {
            bodies_ok = false;
        }
    }
    set.add(
        "P02-判据-执行体与判据对应",
        bodies_ok,
        "六条判据的执行体名须与承诺的实现体逐条对上",
    );
    assert!(bodies_ok, "执行体与判据未逐条对应");

    // 判据 → 原则反查链不断（四原则须声明自己服务哪条判据）。
    let mut serves_ok = true;
    for c in CRITERIA.iter() {
        if !PRINCIPLE_ORDER.iter().any(|p| p.serves().contains(c)) {
            serves_ok = false;
        }
    }
    set.add(
        "P02-判据-原则服务判据全覆盖",
        serves_ok,
        "每条判据至少被一条原则声明服务，否则判据与原则是两张皮",
    );
    assert!(serves_ok, "有判据无原则声明服务");

    // 自审：标准语言册无阻断级问题。
    let lang = MotionLanguage::standard();
    let issues = lang.self_audit();
    let blocking: Vec<&str> = issues
        .iter()
        .filter(|i| i.severity == Severity::Blocking)
        .map(|i| i.code)
        .collect();
    set.add(
        "P02-判据-标准册零阻断问题",
        blocking.is_empty(),
        "标准语言册不得有阻断级自审问题",
    );
    assert!(
        blocking.is_empty(),
        "标准册阻断项：{:?}",
        blocking
            .iter()
            .map(|c| issues.iter().find(|i| i.code == *c).map(|i| i.screen_line()))
            .collect::<Vec<_>>()
    );

    // 问题项五元组齐备。
    set.add(
        "P02-判据-问题项含建议",
        issues
            .iter()
            .all(|i| !i.advice.trim().is_empty() && !i.symptom.trim().is_empty()),
        "自审问题须带现象与建议",
    );
    assert!(
        issues
            .iter()
            .all(|i| !i.advice.trim().is_empty() && !i.symptom.trim().is_empty()),
        "有自审问题缺建议"
    );

    // 版本漂移须被自审抓出来（真缺陷注入）。
    let mut drifted = MotionLanguage::standard();
    drifted.version = "P01-lang-v0";
    let d_issues = drifted.self_audit();
    set.add(
        "P02-判据-版本漂移可检出",
        d_issues.iter().any(|i| i.code == E_LANG_VERSION_DRIFT),
        "语言册版本被改而未升版须被自审拦下",
    );
    assert!(
        d_issues.iter().any(|i| i.code == E_LANG_VERSION_DRIFT),
        "版本漂移未被检出"
    );
    set.add(
        "P02-判据-版本漂移阻断放行",
        drifted.preflight().contains(&E_LANG_VERSION_DRIFT),
        "版本漂移是阻断级：契约变了却按老版本消费，比报错更糟",
    );
    assert!(drifted.preflight().contains(&E_LANG_VERSION_DRIFT));

    // 越界版本号拒绝（未知码不得回落）。
    set.add(
        "P02-判据-版本常量唯一真源",
        LANG_VERSION == "P01-lang-v1" && MotionLanguage::standard().version == LANG_VERSION,
        "语言册版本须等于常量LANG_VERSION",
    );

    // 预检门：阻断项进 preflight，警告项进 warnings（各走各的通道）。
    // 标准册此刻只有一条警告（弹性预留档无主），故两者之和等于自审总数——
    // 这条等式本身就是「没有项被静默丢弃」的可执行证据。
    let total = issues.len();
    let gates = lang.preflight().len() + lang.warnings().len();
    set.add(
        "P02-判据-预检只收阻断级",
        lang.preflight().is_empty()
            && lang.warnings().contains(&E_EASING_NO_SCENE)
            && gates == total,
        "把警告也当阻断会让标准册永远放不了行，门就形同虚设",
    );
    assert!(
        lang.preflight().is_empty(),
        "标准册不该有阻断项：{:?}",
        lang.preflight()
    );
    assert!(
        gates == total,
        "有自审项既没进 preflight 也没进 warnings（{} vs {}）",
        gates,
        total
    );
}

/// 注意力预算（判据一「克制」的执行体）。
fn chk_attention_budget(set: &mut CheckSet) {
    set.add(
        "P02-预算-上限为三",
        MAX_CONCURRENT_MOTIONS == 3,
        "锚点原文：一个界面同时活动元素 ≤3",
    );
    assert_eq!(MAX_CONCURRENT_MOTIONS, 3);

    let mut b = AttentionBudget::new();
    set.add(
        "P02-预算-空账起始",
        b.is_empty() && b.len() == 0 && b.peak() == 0,
        "空账须真的空",
    );

    // 前三次登记成功，第四次被拒（**不静默排队**）。
    let ids: Vec<u64> = (0..MAX_CONCURRENT_MOTIONS)
        .map(|_| b.acquire(Scene::ButtonPress, 0).unwrap_or(0))
        .collect();
    set.add(
        "P02-预算-三次登记成功",
        ids.len() == 3 && b.len() == 3 && b.peak() == 3,
        "预算内登记须成功",
    );
    assert_eq!(b.len(), 3);
    assert_eq!(b.peak(), 3);

    let r4 = b.acquire(Scene::Toggle, 0);
    set.add(
        "P02-预算-第四次被拒不排队",
        r4.is_err() && b.len() == MAX_CONCURRENT_MOTIONS as usize,
        "超预算须显式拒绝；偷偷排队会让「为什么我的动效没播」查不出来",
    );
    assert!(r4.is_err(), "第四次登记未被拒");
    assert_eq!(b.len(), 3, "被拒后不得入账（不能偷偷排队）");
    set.add(
        "P02-预算-拒绝次数显性",
        b.refused() == 1,
        "被拒次数须显性计数，否则红线破没破过都查不到",
    );
    assert_eq!(b.refused(), 1);

    // 拒绝的五元组齐备且指路。
    if let Err(err) = r4 {
        assert!(err.is_complete(), "预算拒绝五元组不全");
        set.add(
            "P02-预算-拒绝五元组齐备",
            err.is_complete() && err.code == E_ATTENTION_BUDGET && !err.who.trim().is_empty(),
            "预算拒绝须带码/现象/原因/下一步/责任方",
        );
        set.add(
            "P02-预算-拒绝指路可执行",
            err.next.contains("合并") || err.next.contains("等"),
            "下一步须是「合并同源动效」或「等当前结束」这类可执行动作",
        );
        assert!(
            err.next.contains("合并") || err.next.contains("等"),
            "预算拒绝的下一步不可执行：{}",
            err.next
        );
    }

    // 释放后可再登记（一次调用同时验证「释放生效」与「名额可复用」）。
    let after_release = b.release(ids[0]);
    let re_acquired = b.acquire(Scene::Toggle, 0);
    set.add(
        "P02-预算-释放后可再登记",
        after_release && re_acquired.is_ok() && b.len() == 3,
        "释放腾出的名额须能再登记",
    );
    assert!(after_release, "释放已登记的活动失败");
    assert!(
        re_acquired.is_ok(),
        "释放后仍登记失败：{:?}",
        re_acquired.as_ref().err().map(|e| e.screen_text())
    );
    assert_eq!(b.len(), 3);
    set.add(
        "P02-预算-释放不存在者返回假",
        !b.release(9999),
        "释放不存在的登记须如实返回假，不能静默成功",
    );
    assert!(!b.release(9999));

    // 峰值如实记录（不截断到上限）。
    set.add(
        "P02-预算-峰值如实记录",
        b.peak() == MAX_CONCURRENT_MOTIONS,
        "峰值就是峰值；截断了就查不出曾经超没超",
    );
    assert_eq!(b.peak(), MAX_CONCURRENT_MOTIONS);

    // 僵死淘汰（动画被打断没走完流程的情形）。
    let mut stale = AttentionBudget::new();
    let sid = stale.acquire(Scene::RouteChange, 0).unwrap_or(0);
    set.add(
        "P02-预算-未到龄不淘汰",
        stale.advance(MOTION_STALE_FRAMES).is_empty() && stale.len() == 1,
        "恰好到龄不算僵死（严一分就是误杀正常动效）",
    );
    assert_eq!(stale.len(), 1);
    let killed = stale.advance(MOTION_STALE_FRAMES + 1);
    set.add(
        "P02-预算-僵死活动被淘汰",
        killed == vec![sid] && stale.is_empty(),
        "超龄未结束的活动须淘汰，否则预算被僵尸占满",
    );
    assert_eq!(killed, vec![sid]);
    assert!(stale.is_empty());

    // 满仓判定与读屏摘要。
    let mut full = AttentionBudget::new();
    for _ in 0..MAX_CONCURRENT_MOTIONS {
        let _ = full.acquire(Scene::MenuOpen, 0);
    }
    let txt = full.screen_text();
    set.add(
        "P02-预算-满仓判定与摘要",
        full.is_full()
            && txt.contains("历史峰值")
            && txt.contains("超预算被拒"),
        "预算摘要须念出在册/峰值/被拒三项（峰值是最易被忽略的红线证据）",
    );
    assert!(full.is_full());
    assert!(txt.contains("历史峰值"), "预算摘要缺峰值：{}", txt);

    // 活动项可读屏。
    set.add(
        "P02-预算-活动项可读屏",
        full.iter().all(|m| m.screen_line().contains(m.scene.zh())),
        "活动项读屏行须含场景中文名",
    );
    assert!(full.iter().all(|m| m.screen_line().contains(m.scene.zh())));
}

/// 时长白名单（降级矩阵第一格：超预算时长→警告+白名单流程）。
fn chk_exemptions(set: &mut CheckSet) {
    let mut x = DurationExemptionLedger::new();
    set.add(
        "P02-白名单-空册起始",
        x.is_empty() && x.refused() == 0,
        "空册须真的空",
    );

    let ex = |ms: u32, reason: &str, scene: Scene, until: u64| DurationExemption {
        granted_ms: ms,
        reason: reason.to_string(),
        scene,
        effective_from_frame: 0,
        expires_at_frame: until,
    };

    // 四项拒绝逐一可检出 + 计数显性。
    let r_reason = x.apply(ex(400, "", Scene::OnboardingTour, 1000));
    set.add(
        "P02-白名单-拒无理由",
        r_reason.as_ref().err().map(|e| e.code) == Some(E_EXEMPTION_NO_REASON),
        "无理由的长动效无法被复核，须拒",
    );
    assert_eq!(
        r_reason.as_ref().err().map(|e| e.code),
        Some(E_EXEMPTION_NO_REASON)
    );

    let r_deadline = x.apply(ex(400, "首次引导要讲清一件事", Scene::OnboardingTour, 0));
    set.add(
        "P02-白名单-拒无期限",
        r_deadline.as_ref().err().map(|e| e.code) == Some(E_EXEMPTION_NO_DEADLINE),
        "期限为零等于永久突破上限，须拒",
    );
    assert_eq!(
        r_deadline.as_ref().err().map(|e| e.code),
        Some(E_EXEMPTION_NO_DEADLINE)
    );

    let r_ceiling = x.apply(ex(
        COMPLEX_ORCHESTRA_MAX_MS + 1,
        "确实需要更慢",
        Scene::OnboardingTour,
        1000,
    ));
    set.add(
        "P02-白名单-拒超全局上限",
        r_ceiling.as_ref().err().map(|e| e.code) == Some(E_EXEMPTION_ABOVE_CEILING),
        "白名单是暂准延长不是取消上限，须拒",
    );
    assert_eq!(
        r_ceiling.as_ref().err().map(|e| e.code),
        Some(E_EXEMPTION_ABOVE_CEILING)
    );

    let ok = x.apply(ex(400, "首次引导要讲清一件事", Scene::OnboardingTour, 1000));
    set.add(
        "P02-白名单-合规申请通过",
        ok.is_ok() && x.len() == 1,
        "有理由有期限且不超上限的白名单须放行",
    );
    assert!(ok.is_ok(), "合规白名单被误拒");
    assert_eq!(x.len(), 1);

    let r_dup = x.apply(ex(450, "换个理由再来一次", Scene::OnboardingTour, 2000));
    set.add(
        "P02-白名单-拒同场景窗口重叠",
        r_dup.as_ref().err().map(|e| e.code) == Some(E_EXEMPTION_DUP),
        "同一时刻两条生效会让实际时长取决于查了哪条",
    );
    assert_eq!(
        r_dup.as_ref().err().map(|e| e.code),
        Some(E_EXEMPTION_DUP),
        "窗口重叠未被拒"
    );

    // 反例：不重叠的窗口必须放行（否则白名单册最多只能存「场景数」条，
    // 上限常量形同虚设）。
    let later = DurationExemption {
        granted_ms: 420,
        reason: "新版引导要讲清两件事".to_string(),
        scene: Scene::OnboardingTour,
        effective_from_frame: 1000,
        expires_at_frame: 2000,
    };
    let r_ok_later = x.apply(later);
    set.add(
        "P02-白名单-不重叠窗口须放行",
        r_ok_later.is_ok() && x.len() == 2,
        "老条目到期之后再排一条不算重复；判重过严会让上限形同虚设",
    );
    assert!(
        r_ok_later.is_ok(),
        "不重叠窗口被误拒：{:?}",
        r_ok_later.as_ref().err().map(|e| e.screen_text())
    );

    set.add(
        "P02-白名单-拒绝次数显性",
        x.refused() == 4,
        "四次拒绝须如实计数",
    );
    assert_eq!(x.refused(), 4, "白名单拒绝计数不对");

    // 册满拒绝：按「场景 × 互不重叠窗口」灌满（每场景可排多条不冲突的）。
    let mut full = DurationExemptionLedger::new();
    let scenes = Scene::all_scenes();
    let mut slot = 0u64;
    while full.len() < MAX_DURATION_EXEMPTIONS {
        let sc = scenes[full.len() % scenes.len()];
        let from = slot * 1000;
        let applied = full.apply(DurationExemption {
            granted_ms: 400,
            reason: format!("第{} 轮评审通过的理由", slot),
            scene: sc,
            effective_from_frame: from,
            expires_at_frame: from + 500,
        });
        assert!(
            applied.is_ok(),
            "灌册失败于第 {} 条：{:?}",
            slot,
            applied.as_ref().err().map(|e| e.screen_text())
        );
        slot += 1;
        if slot > MAX_DURATION_EXEMPTIONS as u64 * 4 + 8 {
            break;
        }
    }
    let r_cap = full.apply(DurationExemption {
        granted_ms: 400,
        reason: "册已满还想再加一条".to_string(),
        scene: Scene::ButtonPress,
        effective_from_frame: 900000,
        expires_at_frame: 900500,
    });
    set.add(
        "P02-白名单-拒册满",
        r_cap.as_ref().err().map(|e| e.code) == Some(E_EXEMPTION_CAP)
            && full.len() == MAX_DURATION_EXEMPTIONS,
        "白名单册须有上限；无上限的册迟早被临时条目灌满",
    );
    assert_eq!(
        r_cap.as_ref().err().map(|e| e.code),
        Some(E_EXEMPTION_CAP),
        "册满拒绝码不对（实际 {:?}）",
        r_cap.as_ref().err().map(|e| e.code)
    );
    assert_eq!(full.len(), MAX_DURATION_EXEMPTIONS, "册满时长度须恰为上限");

    // 五元组齐备（含拒绝的三项）。
    let mut tuples_ok = true;
    for e in [r_reason, r_deadline, r_ceiling, r_dup].into_iter() {
        if let Err(err) = e {
            if !err.is_complete() {
                tuples_ok = false;
            }
        }
    }
    set.add(
        "P02-白名单-拒绝五元组齐备",
        tuples_ok,
        "白名单拒绝的五元组齐发",
    );
    assert!(tuples_ok, "白名单拒绝五元组不全");

    // 判定顺序：白名单优先于分级区间（例外优先但必须已登记）。
    let v_exempt = x.resolve(Scene::OnboardingTour, 400, 0);
    set.add(
        "P02-白名单-生效期内放行",
        v_exempt == DurationVerdict::Exempt(400),
        "已登记白名单在生效期内须放行",
    );
    assert_eq!(v_exempt, DurationVerdict::Exempt(400));

    // 超出获准值即不适用（白名单不是空白支票）。
    let v_over_grant = x.resolve(Scene::OnboardingTour, 450, 0);
    set.add(
        "P02-白名单-超获准值不放行",
        v_over_grant != DurationVerdict::Exempt(400),
        "白名单只准到获准时长，超出即回落分级判定",
    );
    assert_ne!(v_over_grant, DurationVerdict::Exempt(400));

    // 过期后失效（期限是硬语义）。
    let v_expired = x.resolve(Scene::OnboardingTour, 400, 1000);
    set.add(
        "P02-白名单-过期即失效",
        v_expired != DurationVerdict::Exempt(400),
        "期限到即失效；续期须重新评审",
    );
    assert_ne!(v_expired, DurationVerdict::Exempt(400));

    // 未登记场景仍走分级判定。
    let v_other = x.resolve(Scene::ButtonPress, 120, 0);
    set.add(
        "P02-白名单-未登记走分级",
        v_other == DurationVerdict::InRange(DurationTier::MicroFeedback),
        "白名单是「这条场景例外」，别的场景不受影响",
    );
    assert_eq!(v_other, DurationVerdict::InRange(DurationTier::MicroFeedback));

    // 生效期判定与窗口相交判定。
    let e0 = ex(400, "理由", Scene::ButtonPress, 1000);
    let e_late = DurationExemption {
        granted_ms: 400,
        reason: "后期条目".to_string(),
        scene: Scene::ButtonPress,
        effective_from_frame: 1000,
        expires_at_frame: 2000,
    };
    set.add(
        "P02-白名单-生效期边界",
        e0.is_active(0) && e0.is_active(999) && !e0.is_active(1000),
        "期限帧本身即失效（左闭右开）",
    );
    assert!(e0.is_active(999) && !e0.is_active(1000));
    set.add(
        "P02-白名单-窗口相交判定",
        e0.overlaps(&ex(400, "理由", Scene::ButtonPress, 500))
            && !e0.overlaps(&e_late)
            && !e_late.overlaps(&e0),
        "首尾相接（前条到期帧 == 后条生效帧）不算相交",
    );
    assert!(e0.overlaps(&ex(400, "理由", Scene::ButtonPress, 500)));
    assert!(!e0.overlaps(&e_late), "首尾相接被判成相交");

    // 册摘要与条目读屏。
    let txt = x.screen_text(0);
    set.add(
        "P02-白名单-摘要含生效计数",
        txt.contains("生效") && txt.contains("拒绝"),
        "白名单摘要须念出生效条数与累计拒绝数",
    );
    assert!(txt.contains("生效"), "白名单摘要缺生效计数：{}", txt);
    set.add(
        "P02-白名单-条目可读屏",
        x.iter().all(|e| {
            e.screen_line().contains(e.scene.zh()) && e.screen_line().contains("窗口")
        }),
        "白名单条目读屏行须含场景与生效窗口",
    );
    assert!(x
        .iter()
        .all(|e| e.screen_line().contains(e.scene.zh()) && e.screen_line().contains("窗口")));

    // 有效生效时长：越界给最近合规值。
    set.add(
        "P02-白名单-越界给最近合规值",
        DurationVerdict::OutOfRange {
            tier: DurationTier::MicroFeedback,
            nearest: 100,
        }
        .effective_ms(60)
            == 100
            && DurationVerdict::OutOfRange {
                tier: DurationTier::MicroFeedback,
                nearest: 150,
            }
            .effective_ms(900)
                == 150,
        "越界时给最近合规值而不是原样放行",
    );
    set.add(
        "P02-白名单-生效结论恒合规",
        DurationVerdict::Exempt(400).is_ok()
            && DurationVerdict::InRange(DurationTier::MicroFeedback).is_ok()
            && !DurationVerdict::OutOfRange {
                tier: DurationTier::MicroFeedback,
                nearest: 150,
            }
            .is_ok(),
        "白名单放行与区间内都算合规，越界不算",
    );
    assert!(!DurationVerdict::OutOfRange {
        tier: DurationTier::MicroFeedback,
        nearest: 150,
    }
    .is_ok());

    // 空集判「无数据」而非「通过」：空册时 resolve 仍须走分级。
    let empty = DurationExemptionLedger::new();
    set.add(
        "P02-白名单-空册仍走分级",
        empty.resolve(Scene::ButtonPress, 120, 0)
            == DurationVerdict::InRange(DurationTier::MicroFeedback),
        "空册不是「全部放行」，是「没有例外」",
    );
    assert!(empty.is_empty());
}

/// 原则冲突裁决记录（降级矩阵第三格）。
fn chk_rulings(set: &mut CheckSet) {
    let mut r = RulingLedger::new();
    set.add(
        "P02-裁决-空账起始",
        r.is_empty() && r.incomplete_count() == 0,
        "空账须真的空",
    );

    // 拒绝一：同原则自冲突（那是笔误不是冲突）。
    let r_same = r.open_ruling(
        Principle::Rhythm,
        Principle::Rhythm,
        "节奏与节奏打架",
        "都听节奏的",
        "没道理",
        "全局",
    );
    set.add(
        "P02-裁决-拒同原则自冲突",
        r_same.as_ref().err().map(|e| e.code) == Some(E_RULING_SAME_PRINCIPLE),
        "同一条原则内部的取值不合适走分级断言，不是裁决",
    );
    assert_eq!(
        r_same.as_ref().err().map(|e| e.code),
        Some(E_RULING_SAME_PRINCIPLE)
    );

    // 拒绝二：四要素缺失（最常漏「适用边界」）。
    let r_inc = r.open_ruling(
        Principle::Rhythm,
        Principle::Restraint,
        "引导要长动效但同时只能动三个",
        "引导期间放宽预算",
        "引导是唯一需要讲清一件事的时刻",
        "",
    );
    set.add(
        "P02-裁决-拒四要素缺失",
        r_inc.as_ref().err().map(|e| e.code) == Some(E_RULING_INCOMPLETE),
        "没边界的裁决会被当成普适规则",
    );
    assert_eq!(
        r_inc.as_ref().err().map(|e| e.code),
        Some(E_RULING_INCOMPLETE)
    );

    // 正常裁决四要素齐发。
    let id = r.open_ruling(
        Principle::Rhythm,
        Principle::Restraint,
        "引导要长动效但同时只能动三个元素",
        "引导期间把预算临时抬到 5，且只对引导场景生效",
        "引导是唯一需要讲清一件事的时刻；此时并发上限让位于叙事完整",
        "仅新手引导场景；其余场景仍守 3",
    );
    set.add(
        "P02-裁决-合规裁决入账",
        id.is_ok() && r.len() == 1 && r.incomplete_count() == 0,
        "四要素齐发的裁决须入账且判为完整",
    );
    assert!(id.is_ok(), "合规裁决被误拒：{:?}", id.as_ref().err().map(|e| e.screen_text()));
    assert_eq!(r.len(), 1);
    assert_eq!(r.incomplete_count(), 0);

    // 双向可查（冲突复现时要能查到上次怎么裁的）。
    set.add(
        "P02-裁决-双向可查",
        r.find_between(Principle::Rhythm, Principle::Restraint).is_some()
            && r.find_between(Principle::Restraint, Principle::Rhythm).is_some(),
        "反序查也须命中：冲突复现时谁先谁后都可能",
    );
    assert!(r.find_between(Principle::Restraint, Principle::Rhythm).is_some());
    set.add(
        "P02-裁决-无关原则查不到",
        r.find_between(Principle::Breath, Principle::Causality).is_none(),
        "不存在的冲突须查不到；空结果不等于什么都命中",
    );
    assert!(r.find_between(Principle::Breath, Principle::Causality).is_none());

    // 账满拒绝。
    let mut full = RulingLedger::new();
    let pairs = [(Principle::Rhythm, Principle::Restraint), (Principle::Breath, Principle::Causality)];
    let mut n = 0usize;
    while full.len() < MAX_RULINGS {
        let (l, rr) = pairs[n % 2];
        let _ = full.open_ruling(l, rr, "现象", "裁决", "理由", "边界");
        n += 1;
        if n > MAX_RULINGS + 8 {
            break;
        }
    }
    let r_cap = full.open_ruling(
        Principle::Rhythm,
        Principle::Restraint,
        "现象",
        "裁决",
        "理由",
        "边界",
    );
    set.add(
        "P02-裁决-拒账满",
        r_cap.as_ref().err().map(|e| e.code) == Some(E_RULING_CAP) && full.len() == MAX_RULINGS,
        "裁决账须有上限",
    );
    assert_eq!(r_cap.as_ref().err().map(|e| e.code), Some(E_RULING_CAP));
    assert_eq!(full.len(), MAX_RULINGS);

    // 五元组齐备（含两项拒绝）。
    let mut tuples_ok = true;
    for e in [r_same, r_inc, r_cap].into_iter() {
        if let Err(err) = e {
            if !err.is_complete() {
                tuples_ok = false;
            }
        }
    }
    set.add(
        "P02-裁决-拒绝五元组齐备",
        tuples_ok,
        "裁决拒绝的五元组齐发",
    );
    assert!(tuples_ok, "裁决拒绝五元组不全");

    // 条目四要素可读屏。
    let rec = r.iter().next().expect("裁决条目非空");
    set.add(
        "P02-裁决-条目四要素可读屏",
        rec.screen_line().contains("理由") && rec.screen_line().contains("边界"),
        "裁决读屏行须念出理由与边界",
    );
    assert!(rec.screen_line().contains("边界"));
    assert!(rec.is_complete());

    // 账摘要。
    let txt = r.screen_text();
    set.add(
        "P02-裁决-摘要含不完整计数",
        txt.contains("不完整"),
        "裁决摘要须念出不完整条数",
    );
    assert!(txt.contains("不完整"), "裁决摘要缺不完整计数：{}", txt);

    // 空集判「无数据」：空账的 incomplete_count 是 0 但 len 也是 0。
    let empty = RulingLedger::new();
    set.add(
        "P02-裁决-空账不是零不完整",
        empty.len() == 0 && empty.incomplete_count() == 0 && empty.screen_text().contains('0'),
        "空账与「零条不完整」在计数上同值，须靠 len 区分",
    );
    assert_eq!(empty.len(), 0);
}

/// 参数域钳制（降级矩阵第四格：边界越界→钳制+告警）。
fn chk_clamp(set: &mut CheckSet) {
    let mut log = ClampLog::new();
    set.add(
        "P02-错误-空告警账起始",
        log.is_empty() && log.dropped() == 0,
        "空账须真的空",
    );

    // 区间内不落告警（不告警噪声）。
    let inside = clamp_ms(&mut log, "微反馈.in", 120, 100, 150, 7);
    set.add(
        "P02-错误-区间内原值且不告警",
        inside == 120 && log.is_empty(),
        "落在区间内不落告警；否则告警会被噪声淹没",
    );
    assert_eq!(inside, 120);
    assert!(log.is_empty());

    // 低于下界：夹取 + 落告警（成对出现，缺一不可）。
    let below = clamp_ms(&mut log, "微反馈.in", 60, 100, 150, 8);
    set.add(
        "P02-错误-低越界夹取并告警",
        below == 100 && log.len() == 1 && log.count_for("微反馈.in") == 1,
        "静默夹取等于撒谎：返回值与告警必须成对出现",
    );
    assert_eq!(below, 100);
    assert_eq!(log.len(), 1);

    // 高于上界。
    let above = clamp_ms(&mut log, "微反馈.in", 900, 100, 150, 9);
    set.add(
        "P02-错误-高越界夹取并告警",
        above == 150 && log.len() == 2,
        "高于上界须夹到上界并告警",
    );
    assert_eq!(above, 150);
    assert_eq!(log.len(), 2);

    // 告警内容完整（原值/夹后/上下界/帧号）。
    let n = log.iter().next().expect("告警条目非空");
    set.add(
        "P02-错误-告警含原值与帧号",
        n.original == 60 && n.clamped == 100 && n.low == 100 && n.high == 150 && n.frame == 8,
        "告警须能回答「这个值为什么不是我写的那个」",
    );
    assert_eq!(n.original, 60);
    assert_eq!(n.frame, 8);

    // 读屏行含全部要素。
    let ln = n.screen_line();
    set.add(
        "P02-错误-告警可读屏",
        ln.contains("60") && ln.contains("100") && ln.contains("帧"),
        "告警读屏行须含原值/夹后值/帧号",
    );
    assert!(ln.contains("帧"), "告警读屏行缺帧号：{}", ln);

    // 账满拒绝 + 丢弃计数显性。
    let mut full = ClampLog::new();
    let mut n2 = 0usize;
    while full.len() < CLAMP_LOG_CAP && n2 < CLAMP_LOG_CAP + 8 {
        let _ = full.push(ClampNotice {
            field: format!("f{}", n2),
            original: 1,
            clamped: 2,
            low: 2,
            high: 3,
            frame: n2 as u64,
        });
        n2 += 1;
    }
    let r_full = full.push(ClampNotice {
        field: "overflow".to_string(),
        original: 1,
        clamped: 2,
        low: 2,
        high: 3,
        frame: 9999,
    });
    set.add(
        "P02-错误-告警账满拒绝并计数",
        r_full.as_ref().err().map(|e| e.code) == Some(E_CLAMP_LOG_FULL)
            && full.dropped() == 1
            && full.len() == CLAMP_LOG_CAP,
        "丢弃须显性计数；把丢弃的那几条当成「没发生过」是撒谎",
    );
    assert_eq!(r_full.as_ref().err().map(|e| e.code), Some(E_CLAMP_LOG_FULL));
    assert_eq!(full.dropped(), 1);
    assert_eq!(full.len(), CLAMP_LOG_CAP);
    if let Err(err) = r_full {
        assert!(err.is_complete(), "告警账满拒绝五元组不全");
        set.add(
            "P02-错误-账满拒绝五元组齐备",
            err.is_complete() && err.next.contains("归档"),
            "账满拒绝的下一步须是归档轮转或按 ADR 提上限",
        );
        assert!(
            err.next.contains("归档"),
            "账满拒绝的下一步不可执行：{}",
            err.next
        );
    }

    // 账摘要含丢弃计数。
    let txt = full.screen_text();
    set.add(
        "P02-错误-告警摘要含丢弃数",
        txt.contains("丢弃"),
        "告警摘要须念出丢弃数",
    );
    assert!(txt.contains("丢弃"), "告警摘要缺丢弃数：{}", txt);
}

/// 读屏替述（无障碍替述与语言册同源，不另写一份）。
fn chk_narration(set: &mut CheckSet) {
    let lang = MotionLanguage::standard();
    let text = lang.language_narration(0);
    set.add(
        "P02-读屏-替述非空且含版本",
        !text.trim().is_empty() && text.contains(LANG_VERSION),
        "替述须念出版本号",
    );
    assert!(text.contains(LANG_VERSION));

    // 四原则逐项念出（含判定口径）。
    let mut principle_ok = true;
    for p in PRINCIPLE_ORDER.iter() {
        if !text.contains(p.zh()) || !text.contains(p.code()) {
            principle_ok = false;
        }
    }
    set.add(
        "P02-读屏-四原则逐项念出",
        principle_ok,
        "替述缺原则项等于用户听不到这条约束",
    );
    assert!(principle_ok, "替述缺原则项");

    // 四级分级逐项念出。
    let mut tier_ok = true;
    for t in DurationTier::ALL.iter() {
        if !text.contains(t.zh()) || !text.contains(&t.min_ms().to_string()) {
            tier_ok = false;
        }
    }
    set.add(
        "P02-读屏-四级分级逐项念出",
        tier_ok,
        "替述缺分级项等于用户不知道自己的动效该多长",
    );
    assert!(tier_ok, "替述缺分级项");

    // 缓动登记明细逐项念出（名字是语义化替述的核心）。
    let mut easing_ok = true;
    for spec in lang.easings.iter() {
        if !text.contains(&spec.name) {
            easing_ok = false;
        }
    }
    set.add(
        "P02-读屏-缓动登记逐项念出",
        easing_ok,
        "每支缓动的名字都要念出来（名字就是给读屏用户的语义）",
    );
    assert!(easing_ok, "替述缺缓动项");

    // 适用清单十场景逐项念出。
    let mut scene_ok = true;
    for s in Scene::all_scenes() {
        if !text.contains(s.zh()) {
            scene_ok = false;
        }
    }
    set.add(
        "P02-读屏-适用清单逐项念出",
        scene_ok,
        "替述须覆盖全部十场景（含编排级两场景）",
    );
    assert!(scene_ok, "替述缺场景项");

    // 六判据逐项念出。
    let mut crit_ok = true;
    for c in CRITERIA.iter() {
        if !text.contains(c.zh()) || !text.contains(c.code()) {
            crit_ok = false;
        }
    }
    set.add(
        "P02-读屏-六判据逐项念出",
        crit_ok,
        "替述须念出判据清单与码",
    );
    assert!(crit_ok, "替述缺判据项");

    // 关键账目（预算峰值/白名单/裁决/告警/单源对账）都要在替述里。
    set.add(
        "P02-读屏-关键账目在替述中",
        text.contains("注意力预算")
            && text.contains("白名单")
            && text.contains("裁决")
            && text.contains("钳制")
            && text.contains("单源对账"),
        "替述漏账目等于这些红线在无障碍通道里消失",
    );
    assert!(
        text.contains("注意力预算") && text.contains("单源对账"),
        "替述漏关键账目"
    );

    // reduce 信息必须在替述里（内建无障碍的最后一环）。
    set.add(
        "P02-读屏-reduce-在替述中",
        text.contains("reduce") && text.contains("直达终态"),
        "reduce 态行为须能被读出来，否则内建无障碍只是代码里的事实",
    );
    assert!(text.contains("reduce"));

    // 替述随语言册状态变化（不是写死的一段话）。
    let mut used = MotionLanguage::standard();
    used.budget
        .acquire(Scene::ButtonPress, 0)
        .expect("预算内登记");
    let text2 = used.language_narration(0);
    set.add(
        "P02-读屏-替述随册态变化",
        text.contains("历史峰值 0") && text2.contains("历史峰值 1"),
        "替述与册同源：册态变了替述必须跟着变",
    );
    assert!(text2.contains("历史峰值 1"), "替述未随预算变化");
}

/// 错误路径与五元组纪律（全局收口）。
fn chk_error_discipline(set: &mut CheckSet) {
    // 五元组：next 为空即不合格。
    let ok = LangError::new("E-X", "现象", "原因", "下一步", "责任方");
    let bad = LangError::new("E-X", "现象", "原因", "  ", "责任方");
    set.add(
        "P02-错误-五元组next-必填",
        ok.is_complete() && !bad.is_complete(),
        "拒绝必须给出路；next 为空的错误等于让人自己猜",
    );
    assert!(ok.is_complete());
    assert!(!bad.is_complete());

    // 五个字段逐项为空都要判不合格。
    let mut all_fields = true;
    for e in [
        LangError::new("", "现象", "原因", "下一步", "责任方"),
        LangError::new("E-X", " ", "原因", "下一步", "责任方"),
        LangError::new("E-X", "现象", " ", "下一步", "责任方"),
        LangError::new("E-X", "现象", "原因", "下一步", " "),
    ] {
        if e.is_complete() {
            all_fields = false;
        }
    }
    set.add(
        "P02-错误-五元组逐字段校验",
        all_fields,
        "码/现象/原因/下一步/责任方任一为空即不合格",
    );
    assert!(all_fields, "有字段为空却判合格");

    // 读屏行含三要素。
    let txt = ok.screen_text();
    set.add(
        "P02-错误-错误可读屏",
        txt.contains("原因") && txt.contains("下一步") && txt.contains("责任方"),
        "错误读屏行须含原因/下一步/责任方",
    );
    assert!(txt.contains("下一步"), "错误读屏行缺下一步：{}", txt);

    // 错误码常量非空且唯一（本册定义的码不得重号）。
    let codes = [
        E_TIER_ZERO_DURATION,
        E_TIER_BELOW_RANGE,
        E_TIER_ABOVE_RANGE,
        E_ATTENTION_BUDGET,
        E_EASING_BARE_CURVE,
        E_EASING_DUP,
        E_EASING_INCOMPLETE,
        E_EASING_NAME_TOO_LONG,
        E_EASING_CAP,
        E_EXEMPTION_NO_REASON,
        E_EXEMPTION_NO_DEADLINE,
        E_EXEMPTION_ABOVE_CEILING,
        E_EXEMPTION_DUP,
        E_EXEMPTION_CAP,
        E_RULING_SAME_PRINCIPLE,
        E_RULING_INCOMPLETE,
        E_RULING_CAP,
        E_CLAMP_LOG_FULL,
        E_SCENE_EASING_UNREGISTERED,
        E_EASING_NO_SCENE,
        E_TIER_NO_REDUCED_BAND,
        E_PRINCIPLE_UNKNOWN,
        E_LANG_VERSION_DRIFT,
    ];
    let mut uniq = codes.to_vec();
    uniq.sort_unstable();
    uniq.dedup();
    set.add(
        "P02-错误-错误码唯一",
        uniq.len() == codes.len() && !codes.is_empty(),
        "错误码重号会让台账把两处缺陷并成一条",
    );
    assert_eq!(uniq.len(), codes.len(), "错误码重号");
    set.add(
        "P02-错误-错误码非空",
        codes.iter().all(|c| !c.trim().is_empty()),
        "空错误码等于没编码",
    );
    assert!(codes.iter().all(|c| !c.trim().is_empty()));

    // 处置方向相反的状态不得共用码：零时长 vs 超上界必须分码。
    set.add(
        "P02-错误-反向处置分码",
        E_TIER_ZERO_DURATION != E_TIER_BELOW_RANGE
            && E_TIER_BELOW_RANGE != E_TIER_ABOVE_RANGE,
        "零时长/低于下界/超出上界处置方向不同，必须分开",
    );
    assert!(
        E_TIER_ZERO_DURATION != E_TIER_BELOW_RANGE
            && E_TIER_BELOW_RANGE != E_TIER_ABOVE_RANGE,
        "分级错误码有重合"
    );

    // 上限常量与降级矩阵一致性。
    set.add(
        "P02-错误-上限常量贯穿三处",
        COMPLEX_ORCHESTRA_MAX_MS == DurationTier::ComplexOrchestra.max_ms()
            && DurationTier::ComplexOrchestra.max_ms() == 500,
        "500ms 上限须是单一常量，不得三处各写一个 500",
    );
    assert_eq!(COMPLEX_ORCHESTRA_MAX_MS, 500);

    // 容量上限常量非零。
    set.add(
        "P02-错误-容量常量非零",
        MAX_EASINGS > 0
            && MAX_DURATION_EXEMPTIONS > 0
            && MAX_RULINGS > 0
            && CLAMP_LOG_CAP > 0
            && MAX_EASING_NAME_LEN > 0,
        "容量上限为零等于功能直接不可用",
    );
    assert!(MAX_EASINGS > 0 && CLAMP_LOG_CAP > 0);

    // 僵死帧数非零（零会让所有活动立即僵死）。
    set.add(
        "P02-错误-僵死帧数非零",
        MOTION_STALE_FRAMES > 0,
        "僵死帧数为零会让正常动效被立即淘汰",
    );
    assert!(MOTION_STALE_FRAMES > 0);

    // 下游归属表非空且码唯一（防抢活与漏活）。
    let mut dcodes: Vec<&str> = DOWNSTREAM_OWNERSHIP.iter().map(|(c, _)| *c).collect();
    dcodes.sort_unstable();
    let mut duniq = dcodes.clone();
    duniq.dedup();
    set.add(
        "P02-判据-下游归属码唯一",
        duniq.len() == dcodes.len() && !dcodes.is_empty(),
        "下游归属码唯一",
    );
    assert_eq!(duniq.len(), dcodes.len());
    set.add(
        "P02-判据-下游归属非空",
        DOWNSTREAM_OWNERSHIP
            .iter()
            .all(|(c, d)| !c.trim().is_empty() && !d.trim().is_empty()),
        "归属表每行须有单号与职责说明（防抢活也防漏活）",
    );
    assert!(DOWNSTREAM_OWNERSHIP
        .iter()
        .all(|(c, d)| !c.trim().is_empty() && !d.trim().is_empty()));
    set.add(
        "P02-判据-本项不代做下游",
        !DOWNSTREAM_OWNERSHIP
            .iter()
            .any(|(c, _)| c.starts_with("VE-F3002")),
        "本项不得把自己列进下游归属表",
    );
    assert!(!DOWNSTREAM_OWNERSHIP
        .iter()
        .any(|(c, _)| c.starts_with("VE-F3002")));
}

/// VE-F3002 域自检·第一批（**语言层判据**）。
///
/// 分两批不是图省事：[`CheckSet`] 的容量 [`MAX_CHECKS`] 是**全仓共享**的
/// 固定上限（本域 168 项已超），而抬高那个常量会连带放大全部域的聚合数组
/// ——那是动别人家的账。本域按判据族自然切成两批，各批独立成集，
/// 聚合器登记两条（`VE-F3002-a` / `VE-F3002-b`），谁都不会被截断丢红。
///
/// 第一批收「语言是什么」：四原则 / 四级时长 / 语义化缓动 / 内建 reduce /
/// 单源取值 / 判据自证。
pub fn run_vep02_checks_a() -> CheckSet {
    let mut set = CheckSet::new("vep02-lang-a");
    chk_principles(&mut set);
    chk_tiers(&mut set);
    chk_easings(&mut set);
    chk_reduce_built_in(&mut set);
    chk_single_source(&mut set);
    chk_criteria(&mut set);
    set
}

/// VE-F3002 域自检·第二批（**约束怎么执行**）。
///
/// 第二批收「谁来守」：注意力预算 / 时长白名单 / 裁决记录 / 参数钳制 /
/// 读屏替述 / 错误纪律。
pub fn run_vep02_checks_b() -> CheckSet {
    let mut set = CheckSet::new("vep02-lang-b");
    chk_attention_budget(&mut set);
    chk_exemptions(&mut set);
    chk_rulings(&mut set);
    chk_clamp(&mut set);
    chk_narration(&mut set);
    chk_error_discipline(&mut set);
    set
}

/// VE-F3002 域自检总入口（第一批）。
///
/// 保留这个单参入口是为了让[`vep02_lang::run_vep02_checks`] 与既有
/// 「一个功能一个自检入口」的惯例一致；第二批请直接调
/// [`run_vep02_checks_b`]。
pub fn run_vep02_checks() -> CheckSet {
    run_vep02_checks_a()
}

// ---------------------------------------------------------------------------
// 单元测试（宿主侧cargo test 直跑；回归可复现——零墙钟零 IO）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vep02_language_selfcheck_clean() {
        let l = MotionLanguage::standard();
        assert_eq!(l.version, LANG_VERSION);
        assert!(
            l.preflight().is_empty(),
            "标准语言册不应有阻断级缺口：{:?}",
            l.preflight()
        );
        let blocking: Vec<&str> = l
            .self_audit()
            .iter()
            .filter(|i| i.severity == Severity::Blocking)
            .map(|i| i.code)
            .collect();
        assert!(blocking.is_empty(), "标准册阻断项：{:?}", blocking);
    }

    #[test]
    fn vep02_domain_checks_all_green() {
        // 两批都验：只跑第一批等于放过后一半判据。
        for set in [run_vep02_checks_a(), run_vep02_checks_b()] {
            let (p, f) = set.tally();
            assert_eq!(f, 0, "{} 存在红项：{:?}", set.domain, failing(&set));
            assert!(
                !set.truncated(),
                "{} 自检项被截断（超过 MAX_CHECKS）",
                set.domain
            );
            assert!(p > 60, "{} 自检项偏少（{}），判据覆盖可能有漏", set.domain, p);
        }
        // 单参入口必须等价于第一批（惯例一致性）。
        assert_eq!(run_vep02_checks().tally(), run_vep02_checks_a().tally());
    }

    fn failing(set: &CheckSet) -> Vec<&'static str> {
        set.iter().filter(|c| !c.passed).map(|c| c.name).collect()
    }

    #[test]
    fn vep02_principle_order_is_single_source() {
        assert_eq!(PRINCIPLE_ORDER.len(), Principle::ALL.len());
        for (i, p) in PRINCIPLE_ORDER.iter().enumerate() {
            assert_eq!(p.rank(), i);
            assert_eq!(*p, Principle::ALL[i]);
            assert_eq!(Principle::from_code(p.code()), Some(*p));
        }
        assert_eq!(Principle::from_code("P02-P9"), None);
    }

    #[test]
    fn vep02_tier_ranges_match_anchor() {
        // 锚点原文：微反馈 100-150 / 组件转场 200-300 / 页面转场 300-450 /
        // 复杂编排 500ms 上限。
        assert_eq!(DurationTier::MicroFeedback.min_ms(), 100);
        assert_eq!(DurationTier::MicroFeedback.max_ms(), 150);
        assert_eq!(DurationTier::ComponentTransition.min_ms(), 200);
        assert_eq!(DurationTier::ComponentTransition.max_ms(), 300);
        assert_eq!(DurationTier::PageTransition.min_ms(), 300);
        assert_eq!(DurationTier::PageTransition.max_ms(), 450);
        assert_eq!(DurationTier::ComplexOrchestra.max_ms(), 500);
        // 单调递增。
        for w in DurationTier::ALL.windows(2) {
            assert!(
                w[1].min_ms() >= w[0].min_ms(),
                "分级未按时长递增：{} → {}",
                w[0].zh(),
                w[1].zh()
            );
        }
    }

    #[test]
    fn vep02_reduced_band_is_built_in() {
        for t in DurationTier::ALL.iter() {
            assert!(t.has_reduced_band(), "{} 缺内建 reduce 档", t.zh());
            assert_eq!(t.reduced_ms(), REDUCED_DURATION_MS);
        }
    }

    #[test]
    fn vep02_bare_curve_is_rejected() {
        let mut r = EasingRegistry::new();
        let spec = EasingSpec {
            name: "cubic-bezier(0.4, 0, 0.2, 1)".to_string(),
            family: EasingFamily::Standard,
            curve: "某曲线".to_string(),
            semantics: "某语义".to_string(),
            enter: true,
        };
        let e = r.admit(spec).unwrap_err();
        assert_eq!(e.code, E_EASING_BARE_CURVE);
        assert!(e.is_complete());
        assert_eq!(r.rejected_bare(), 1);
        assert!(r.is_empty(), "被拒的裸曲线不得入册");
    }

    #[test]
    fn vep02_attention_budget_is_hard_gate() {
        let mut b = AttentionBudget::new();
        for _ in 0..MAX_CONCURRENT_MOTIONS {
            b.acquire(Scene::ButtonPress, 0).expect("预算内");
        }
        let e = b.acquire(Scene::Toggle, 0).unwrap_err();
        assert_eq!(e.code, E_ATTENTION_BUDGET);
        assert!(e.is_complete());
        assert_eq!(b.len(), MAX_CONCURRENT_MOTIONS as usize, "被拒不排队");
        assert_eq!(b.refused(), 1);
        assert!(b.is_full());
    }

    #[test]
    fn vep02_stale_motion_is_evicted() {
        let mut b = AttentionBudget::new();
        let id = b.acquire(Scene::RouteChange, 0).expect("登记");
        assert!(b.advance(MOTION_STALE_FRAMES).is_empty());
        assert_eq!(b.advance(MOTION_STALE_FRAMES + 1), vec![id]);
        assert!(b.is_empty());
    }

    #[test]
    fn vep02_exemption_requires_reason_and_deadline() {
        let mut x = DurationExemptionLedger::new();
        let mk = |reason: &str, until: u64| DurationExemption {
            granted_ms: 400,
            reason: reason.to_string(),
            scene: Scene::OnboardingTour,
            effective_from_frame: 0,
            expires_at_frame: until,
        };
        assert_eq!(
            x.apply(mk("", 1000)).unwrap_err().code,
            E_EXEMPTION_NO_REASON
        );
        assert_eq!(
            x.apply(mk("理由", 0)).unwrap_err().code,
            E_EXEMPTION_NO_DEADLINE
        );
        assert_eq!(x.refused(), 2);
        assert!(x.apply(mk("首次引导要讲清一件事", 1000)).is_ok());
        assert_eq!(x.len(), 1);
    }

    #[test]
    fn vep02_ruling_requires_four_elements() {
        let mut r = RulingLedger::new();
        assert_eq!(
            r.open_ruling(Principle::Rhythm, Principle::Rhythm, "a", "b", "c", "d")
                .unwrap_err()
                .code,
            E_RULING_SAME_PRINCIPLE
        );
        assert_eq!(
            r.open_ruling(
                Principle::Rhythm,
                Principle::Restraint,
                "现象",
                "裁决",
                "理由",
                ""
            )
            .unwrap_err()
            .code,
            E_RULING_INCOMPLETE
        );
        let id = r
            .open_ruling(
                Principle::Rhythm,
                Principle::Restraint,
                "现象",
                "裁决",
                "理由",
                "边界",
            )
            .expect("合规裁决");
        assert!(id > 0);
        assert_eq!(r.incomplete_count(), 0);
    }

    #[test]
    fn vep02_clamp_always_leaves_a_notice() {
        let mut log = ClampLog::new();
        assert_eq!(clamp_ms(&mut log, "f", 120, 100, 150, 1), 120);
        assert!(log.is_empty(), "区间内不告警");
        assert_eq!(clamp_ms(&mut log, "f", 60, 100, 150, 2), 100);
        assert_eq!(log.len(), 1, "越界必落告警");
        assert_eq!(clamp_ms(&mut log, "f", 900, 100, 150, 3), 150);
        assert_eq!(log.len(), 2);
    }

    #[test]
    fn vep02_single_source_is_bidirectional() {
        let l = MotionLanguage::standard();
        let a = l.audit_single_source();
        assert!(a.unregistered.is_empty(), "场景引用了未在册缓动");
        assert!(a.no_reduced_band.is_empty());
        // 无主缓动如实报出（弹性预留档）。
        assert_eq!(a.ownerless.len(), 2, "无主缓动：{:?}", a.ownerless);
        for n in a.ownerless.iter() {
            assert_eq!(
                l.easings.spec(n).map(|s| s.family),
                Some(EasingFamily::Elastic)
            );
        }
    }

    #[test]
    fn vep02_page_transition_single_spring_is_not_a_violation() {
        // 页面转场进出共用一条弹簧——那是有性格，不是「一律 ease」。
        let l = MotionLanguage::standard();
        let v = l.audit_principles(
            Scene::RouteChange,
            Scene::RouteChange.recommended_ms(),
            None,
            false,
            1,
        );
        assert!(
            !v.iter().any(|x| x.principle == Principle::Breath),
            "页面转场被误判为「一律 ease」：{:?}",
            v.iter().map(|x| x.screen_line()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn vep02_uniform_ease_is_a_violation() {
        let b = BreathInput {
            distinct: 1,
            sole_family: Some(EasingFamily::Standard),
        };
        let v = Principle::Breath.verdict(
            "按钮按下",
            120,
            None,
            "ease-out-enter",
            "ease-out-enter",
            b,
            false,
            1,
        );
        assert!(
            v.iter().any(|x| x.principle == Principle::Breath),
            "一律 ease 未被检出"
        );
    }

    #[test]
    fn vep02_scenes_split_eight_plus_two() {
        // 锚点括注列 8 常规场景，分级表覆盖 10 场景；本册拆为 8+2。
        assert_eq!(Scene::ALL.len(), 8);
        assert_eq!(Scene::EXTENDED.len(), 2);
        assert_eq!(Scene::all_scenes().len(), 10);
        for s in Scene::EXTENDED.iter() {
            assert!(!Scene::ALL.contains(s), "{}重复登记", s.zh());
            assert_eq!(s.tier(), DurationTier::ComplexOrchestra);
        }
        for s in Scene::all_scenes() {
            assert!(Scene::from_code(s.code()) == Some(s));
        }
    }

    #[test]
    fn vep02_out_of_range_gives_nearest_compliant() {
        let l = MotionLanguage::standard();
        let over = l.scene_recommendation(Scene::ButtonPress, 900, 0);
        assert_eq!(over.effective_ms, 150);
        assert!(!over.verdict.is_ok());
        let under = l.scene_recommendation(Scene::ButtonPress, 60, 0);
        assert_eq!(under.effective_ms, 100);
        assert!(!under.verdict.is_ok());
    }

    #[test]
    fn vep02_recommended_ms_is_midpoint_not_ceiling() {
        assert_eq!(Scene::ButtonPress.recommended_ms(), 125);
        assert!(Scene::ButtonPress.recommended_ms() < 150);
        assert_eq!(Scene::RouteChange.recommended_ms(), 375);
    }

    #[test]
    fn vep02_narration_covers_everything() {
        let l = MotionLanguage::standard();
        let t = l.language_narration(0);
        for p in PRINCIPLE_ORDER.iter() {
            assert!(t.contains(p.zh()), "替述缺原则 {}", p.zh());
        }
        for tr in DurationTier::ALL.iter() {
            assert!(t.contains(tr.zh()), "替述缺分级 {}", tr.zh());
        }
        for s in Scene::all_scenes() {
            assert!(t.contains(s.zh()), "替述缺场景 {}", s.zh());
        }
        for c in CRITERIA.iter() {
            assert!(t.contains(c.zh()), "替述缺判据 {}", c.zh());
        }
        for spec in l.easings.iter() {
            assert!(t.contains(&spec.name), "替述缺缓动 {}", spec.name);
        }
        assert!(t.contains("注意力预算"));
        assert!(t.contains("单源对账"));
        assert!(t.contains("reduce"));
    }

    #[test]
    fn vep02_preflight_gates_blocking_only() {
        let l = MotionLanguage::standard();
        // 标准册：无阻断项（可放行），但有警告项（可见）。
        assert!(l.preflight().is_empty());
        assert!(!l.warnings().is_empty());
        // 注入阻断缺陷：版本漂移须拦住放行。
        let mut bad = MotionLanguage::standard();
        bad.version = "P01-lang-v0";
        assert!(bad.preflight().contains(&E_LANG_VERSION_DRIFT));
    }

    #[test]
    fn vep02_error_codes_are_unique() {
        let all = [
            E_TIER_ZERO_DURATION,
            E_TIER_BELOW_RANGE,
            E_TIER_ABOVE_RANGE,
            E_ATTENTION_BUDGET,
            E_EASING_BARE_CURVE,
            E_EASING_DUP,
            E_EASING_INCOMPLETE,
            E_EASING_NAME_TOO_LONG,
            E_EASING_CAP,
            E_EXEMPTION_NO_REASON,
            E_EXEMPTION_NO_DEADLINE,
            E_EXEMPTION_ABOVE_CEILING,
            E_EXEMPTION_DUP,
            E_EXEMPTION_CAP,
            E_RULING_SAME_PRINCIPLE,
            E_RULING_INCOMPLETE,
            E_RULING_CAP,
            E_CLAMP_LOG_FULL,
            E_SCENE_EASING_UNREGISTERED,
            E_EASING_NO_SCENE,
            E_TIER_NO_REDUCED_BAND,
            E_PRINCIPLE_UNKNOWN,
            E_LANG_VERSION_DRIFT,
        ];
        let mut uniq = all.to_vec();
        uniq.sort_unstable();
        uniq.dedup();
        assert_eq!(uniq.len(), all.len(), "错误码重号");
    }

    #[test]
    fn vep02_downstream_ownership_is_single_source() {
        let mut codes: Vec<&str> = DOWNSTREAM_OWNERSHIP.iter().map(|(c, _)| *c).collect();
        codes.sort_unstable();
        let before = codes.len();
        codes.dedup();
        assert_eq!(codes.len(), before, "下游归属码重复");
        for (c, d) in DOWNSTREAM_OWNERSHIP.iter() {
            assert!(!c.trim().is_empty() && !d.trim().is_empty());
            assert!(!c.starts_with("VE-F3002"), "本项把自己列进了下游表");
        }
    }

    #[test]
    fn vep02_criteria_execution_bodies_exist() {
        assert_eq!(CRITERIA.len(), 6);
        let bodies: Vec<&str> = CRITERIA.iter().map(|c| c.enforced_by()).collect();
        for want in [
            "Principle::verdict",
            "DurationTier::assert_in_range",
            "EasingRegistry::admit",
            "DurationTier::reduced_ms",
            "MotionLanguage::audit_single_source",
        ] {
            assert!(
                bodies.iter().any(|b| b.contains(want)),
                "无判据指向执行体 {}",
                want
            );
        }
        // 每条判据至少被一条原则声明服务。
        for c in CRITERIA.iter() {
            assert!(
                PRINCIPLE_ORDER.iter().any(|p| p.serves().contains(c)),
                "判据 {} 无原则服务",
                c.zh()
            );
        }
    }

    #[test]
    fn vep02_resolve_easing_is_the_only_way_in() {
        let l = MotionLanguage::standard();
        assert!(l.resolve_easing("ease-out-enter").is_ok());
        let e = l.resolve_easing("no-such-easing").unwrap_err();
        assert!(e.is_complete());
        assert!(e.next.contains("登记"), "未给登记指路：{}", e.next);
    }

    #[test]
    fn vep02_tier_of_returns_nearest_for_gaps() {
        // 落在档间缝隙时取最近一级而不是返回 None——160ms 是「微反馈慢了点」。
        assert_eq!(DurationTier::of(160), Some(DurationTier::MicroFeedback));
        assert_eq!(DurationTier::of(180), Some(DurationTier::ComponentTransition));
        assert_eq!(DurationTier::of(0), None);
    }

    #[test]
    fn vep02_empty_ledger_is_not_all_permit() {
        // 空集判「无数据」而非「通过」：空白名单册不等于全部放行。
        let x = DurationExemptionLedger::new();
        assert!(x.is_empty());
        assert_eq!(
            x.resolve(Scene::ButtonPress, 120, 0),
            DurationVerdict::InRange(DurationTier::MicroFeedback)
        );
        assert_eq!(
            x.resolve(Scene::ButtonPress, 900, 0),
            DurationVerdict::OutOfRange {
                tier: DurationTier::MicroFeedback,
                nearest: 150
            }
        );
    }
}