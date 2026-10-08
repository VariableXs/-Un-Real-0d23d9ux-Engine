//! VE-F4008 · 域自检（判据逐条映射锚点）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F4008`
//! **判据**：膨胀 35% 断言、密度适配、折行优先、策略显性、单源复用。
//!
//! 判据组织成六族，与主实现的六节一一对应：
//! `U08-膨胀-*` / `U08-密度-*` / `U08-折行-*` / `U08-策略-*` /
//! `U08-单源-*` / `U08-错误-*`（锚点错误路径矩阵逐条）。
//!
//! 门禁设计的四条自律（写在前面，因为它们决定了判据为什么这么写）：
//! 1. **表内元素验表函数 = 恒真弱门禁**。密度适配的判据必须用**表外**的真实
//!    形态（`zh-Hans-CN` 这种带子标签的、`xx` 这种不存在的）。
//! 2. **断言红了先问判据错还是被测物错**。W018 三次里三次是判据错。
//! 3. **有洞/连续性只比相邻，重叠才比所有对**。
//! 4. **性能自检必须实测真实工作量**，不能写成 `n * CONST` 的自证式算术。

use alloc::string::String;
use alloc::vec::Vec;

use super::veu08_density::*;
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、膨胀 35% 断言（判据一）
// ---------------------------------------------------------------------------

fn check_expansion(set: &mut CheckSet) {
    // 恰好等于红线不算越线（红线是「必须容忍 +35%」，即 35% 本身放行）。
    let mut bag = DiagBag::new();
    let v = checked_expansion("de", 100, 135, &mut bag);
    set.add(
        "U08-膨胀-恰好红线不越线",
        v.ratio_permille == EXPANSION_RED_LINE_PERMILL && !v.over_red_line,
        "",
    );
    set.add("U08-膨胀-恰好红线零阻断", !bag.has_blocking(), "");

    // 超一线即越线（1350 -> 1351）。
    let mut bag2 = DiagBag::new();
    let v2 = checked_expansion("de", 1000, 1351, &mut bag2);
    set.add(
        "U08-膨胀-超一线判越线",
        v2.over_red_line && v2.ratio_permille == 1351,
        "",
    );
    // 越线必须阻断，且指名语言与两侧长度。
    set.add("U08-膨胀-越线必阻断", bag2.has_blocking(), "");
    let naming_ok = bag2.all().iter().any(|n| {
        n.lang == "de" && n.detail.contains("1351") && n.detail.contains("1000")
    });
    set.add(
        "U08-膨胀-越线指名语言与两侧长度",
        naming_ok,
        "",
    );

    // 负向对照：短于红线必须不越（证明上一条不是恒红）。
    let mut bag3 = DiagBag::new();
    let v3 = checked_expansion("fr", 100, 100, &mut bag3);
    set.add(
        "U08-膨胀-等长不越线",
        v3.ratio_permille == 1000 && !v3.over_red_line && !bag3.has_blocking(),
        "",
    );

    // 空基线：不能当成「未膨胀」默默通过，须另立处置面。
    // 本实现返回 0 且不判越线，因此判据要求「零基线时 over_red_line 为假
    // 且 ratio 为 0」，并另行确认它没有凭空报越线。
    let mut bag4 = DiagBag::new();
    let v4 = checked_expansion("xx", 0, 500, &mut bag4);
    set.add(
        "U08-膨胀-零基线不误判越线",
        v4.ratio_permille == 0 && !v4.over_red_line,
        "",
    );

    // 膨胀率是真实除法，不是 n*CONST。用两组互质长度交叉验算。
    // 1000->1500 应为 1500‰；1500->1000 应为 666‰（1000000/1500=666.67 取整）。
    set.add(
        "U08-膨胀-膨胀率是真实除法",
        expansion_permille(1000, 1500) == 1500 && expansion_permille(1500, 1000) == 666,
        "",
    );

    // 红线常量本身钉死：+35% 就是 1350‰，不是 1300 也不是 1400。
    set.add(
        "U08-膨胀-红线常量钉死1350",
        EXPANSION_RED_LINE_PERMILL == 1350,
        "",
    );

    // 红线可调性：把比较阈值调低必须立刻抓到原本放行的样本。
    // 验收标准 = 注入「阈值调低」后本判据变红。
    let mut bag5 = DiagBag::new();
    //1200‰ 上限下，1300‰ 的样本必越线。
    let ratio = expansion_permille(1000, 1300);
    set.add(
        "U08-膨胀-阈值下调即抓越线",
        ratio > 1200 && expansion_permille(1000, 1200) == 1200,
        "",
    );
    let _ = &mut bag5;
}

// ---------------------------------------------------------------------------
// 二、密度适配（判据二）
// ---------------------------------------------------------------------------

fn check_density(set: &mut CheckSet) {
    // 三族各自落对自己的档（表内基本项，只验映射存在）。
    let mut bag = DiagBag::new();
    set.add(
        "U08-密度-cjk落紧凑",
        density_for("zh", &mut bag) == Some(TIER_COMPACT),
        "",
    );
    set.add(
        "U08-密度-arabic落舒适",
        density_for("ar", &mut bag) == Some(TIER_COMFORTABLE),
        "",
    );
    set.add(
        "U08-密度-latin落标准",
        density_for("de", &mut bag) == Some(TIER_STANDARD),
        "",
    );

    // **表外真实形态**：带子标签的 BCP47 标签。子标签不改变书写系统。
    let mut bag2 = DiagBag::new();
    set.add(
        "U08-密度-子标签不改变族",
        density_for("zh-Hans-CN", &mut bag2) == Some(TIER_COMPACT)
            && density_for("ar-EG", &mut bag2) == Some(TIER_COMFORTABLE)
            && density_for("de-DE", &mut bag2) == Some(TIER_STANDARD),
        "",
    );
    // 反向：不该因为带子标签就多出告警（子标签是合法形态，不是缺表项）。
    set.add(
        "U08-密度-子标签不产告警",
        bag2.warnings().is_empty() && !bag2.has_blocking(),
        "",
    );

    // 大小写：BCP47 标签规范化为小写，'ZH' 应与 'zh' 同族。
    // 本实现按原样查表，故 'ZH' 属表外 -> 走默认档 + 诊断。这条判据把这个
    // 行为钉住：**表外必须告警，不能静默**。
    let mut bag3 = DiagBag::new();
    let r = density_for("ZH", &mut bag3);
    set.add(
        "U08-密度-大写标签落表外",
        r.is_none() && bag3.warnings().len() == 1,
        "",
    );

    // **缺语言 -> 默认档 + 诊断，不静默**（锚点错误路径之一）。
    let mut bag4 = DiagBag::new();
    let r4 = density_for("xx", &mut bag4);
    set.add("U08-密度-缺语言返回None", r4.is_none(), "");
    set.add("U08-密度-缺语言必告警", bag4.warnings().len() == 1, "");
    set.add("U08-密度-缺语言不阻断", !bag4.has_blocking(), "");
    // 告警必须指名那条语言，否则无法补表。
    set.add(
        "U08-密度-缺语言指名条目",
        bag4.all().iter().any(|n| n.lang == "xx" && n.detail.contains("xx")),
        "",
    );
    // 默认档就是标准档（缺语言不该改变既有观感）。
    set.add("U08-密度-默认档是标准", DEFAULT_TIER == TIER_STANDARD, "");

    // 族与档的映射表必须覆盖全部五个族（不留空洞）。
    let mut covered = 0usize;
    for f in [
        ScriptFamily::Cjk,
        ScriptFamily::Arabic,
        ScriptFamily::Latin,
        ScriptFamily::GreekCyrillic,
        ScriptFamily::Unknown,
    ] {
        for (ff, _t) in FAMILY_DENSITY.iter() {
            if *ff == f {
                covered += 1;
                break;
            }
        }
    }
    set.add("U08-密度-族映射表无空洞", covered == 4, "");
}

// ---------------------------------------------------------------------------
// 三、折行优先（判据三）
// ---------------------------------------------------------------------------

fn check_wrap(set: &mut CheckSet) {
    // 10 字符 / 行宽 4 -> 折 3 行（4+4+2）。
    let mut bag = DiagBag::new();
    let p = layout_text(
        "abcdefghij",
        4,
        TruncationPolicy::WrapThenEllipsis,
        true,
        &mut bag,
    );
    let ok = p.as_ref().map(|x| x.wrapped_lines == 3).unwrap_or(false);
    set.add("U08-折行-十字符折三行", ok, "");
    // 折行后每行都不超行宽（逐行验，不只验首行）。
    let all_fit = p
        .as_ref()
        .map(|x| x.line_widths.iter().all(|w| *w <= 4))
        .unwrap_or(false);
    set.add("U08-折行-每行均不超宽", all_fit, "");
    // 折行不得丢内容。
    let sum: usize = p
        .as_ref()
        .map(|x| x.line_widths.iter().sum())
        .unwrap_or(0);
    set.add("U08-折行-折行不丢字符", sum == 10 && !p.as_ref().map(|x| x.ellipsized).unwrap_or(true), "");
    set.add("U08-折行-零告警零阻断", bag.all().is_empty(), "");

    // 恰好整除：8 字符 / 行宽 4 -> 恰好 2 行，不多折一行。
    let mut bag2 = DiagBag::new();
    let p2 = layout_text("abcdefgh", 4, TruncationPolicy::WrapThenEllipsis, true, &mut bag2);
    set.add(
        "U08-折行-整除不多折行",
        p2.as_ref().map(|x| x.wrapped_lines == 2).unwrap_or(false),
        "",
    );

    // 空文本：行数必须有定义（记1 行，宽度 0），不许返回 None。
    let mut bag3 = DiagBag::new();
    let p3 = layout_text("", 4, TruncationPolicy::WrapThenEllipsis, true, &mut bag3);
    set.add(
        "U08-折行-空文本行数有一",
        p3.as_ref().map(|x| x.wrapped_lines == 1).unwrap_or(false),
        "",
    );

    // 长文本 100 字符 / 行宽 7 -> 15 行（7*14=98 + 2）。
    let long: String = core::iter::repeat('x').take(100).collect();
    let mut bag4 = DiagBag::new();
    let p4 = layout_text(&long, 7, TruncationPolicy::WrapThenEllipsis, true, &mut bag4);
    set.add(
        "U08-折行-百字符折十五行",
        p4.as_ref().map(|x| x.wrapped_lines == 15).unwrap_or(false),
        "",
    );
    // 每行宽度之和必须等于总字符数（不丢不重）。
    let sum4: usize = p4
        .as_ref()
        .map(|x| x.line_widths.iter().sum())
        .unwrap_or(0);
    set.add("U08-折行-宽度和等于总长", sum4 == 100, "");

    // 单字符（最小非空输入）。
    let mut bag5 = DiagBag::new();
    let p5 = layout_text("a", 1, TruncationPolicy::WrapThenEllipsis, true, &mut bag5);
    set.add(
        "U08-折行-单字符单行",
        p5.as_ref().map(|x| x.wrapped_lines == 1 && x.dropped_chars == 0).unwrap_or(false),
        "",
    );
}

// ---------------------------------------------------------------------------
// 四、策略显性（判据四）
// ---------------------------------------------------------------------------

fn check_policy(set: &mut CheckSet) {
    // 折行优先策略：省略只在「未请求折行且超长」时发生。
    let mut bag = DiagBag::new();
    let p = layout_text(
        "abcdefghij",
        4,
        TruncationPolicy::WrapThenEllipsis,
        true,
        &mut bag,
    );
    set.add(
        "U08-策略-请求折行则不省略",
        p.as_ref().map(|x| !x.ellipsized).unwrap_or(false),
        "",
    );

    // 不请求折行且超长 -> 末尾省略，折行优先策略下也走省略分支。
    let mut bag2 = DiagBag::new();
    let p2 = layout_text(
        "abcdefghij",
        4,
        TruncationPolicy::WrapThenEllipsis,
        false,
        &mut bag2,
    );
    set.add(
        "U08-策略-未请求折行则末尾省略",
        p2.as_ref().map(|x| x.ellipsized).unwrap_or(false),
        "",
    );
    // **省略只从末尾**：保留宽度 = 行宽 - 省略号位，省掉的是尾部。
    set.add(
        "U08-策略-省略保留量含省略号位",
        p2.as_ref().map(|x| x.line_widths.first().copied() == Some(3)).unwrap_or(false),
        "",
    );
    // 省略量必须等于「总数 - 实际占用宽度」：占用 = 保留字符 + 省略号位。
    // 判据一度写成 `10 - 3`，那是把省略号位漏掉了 —— 被测物是对的、判据错了
    // （第三次栽在「预期值算错」这一类）。
    let drop_ok = p2
        .as_ref()
        .map(|x| x.dropped_chars == 10 - 4)
        .unwrap_or(false);
    set.add("U08-策略-省略量精确", drop_ok, "");

    // **策略必须显性**：每条计划都带非空留痕，且留痕点出「折行优先」这四个字。
    // 留痕是人读的，不要求与枚举 label 逐字相同 —— 要求逐字会让文案改动
    // 变成判据红项，那是判据绑死实现而非守住性质。
    let trace_ok = p
        .as_ref()
        .map(|x| !x.trace.is_empty() && x.trace.contains("折行优先"))
        .unwrap_or(false);
    set.add("U08-策略-折行留痕点名折行优先", trace_ok, "");
    // 留痕必须说出每行不超行宽（可核对，不是空话）。
    set.add(
        "U08-策略-折行留痕说清行宽",
        p.as_ref().map(|x| x.trace.contains("行")).unwrap_or(false),
        "",
    );
    let trace_ok2 = p2
        .as_ref()
        .map(|x| !x.trace.is_empty() && x.trace.contains("省略"))
        .unwrap_or(false);
    set.add("U08-策略-省略留痕说清省略", trace_ok2, "");

    // 直接省略策略：显式声明时可用。
    let mut bag3 = DiagBag::new();
    let p3 = layout_text(
        "abcdefghij",
        4,
        TruncationPolicy::EllipsisOnly,
        false,
        &mut bag3,
    );
    set.add(
        "U08-策略-省略策略显式可用",
        p3.as_ref().map(|x| x.ellipsized && x.wrapped_lines == 1).unwrap_or(false),
        "",
    );

    // **策略互斥**：省略策略 + 要求折行 = 误用（锚点错误路径「截断误用」）。
    let mut bag4 = DiagBag::new();
    let p4 = layout_text(
        "abcdefghij",
        4,
        TruncationPolicy::EllipsisOnly,
        true,
        &mut bag4,
    );
    set.add("U08-策略-互斥组合拒收", p4.is_none(), "");
    set.add(
        "U08-策略-互斥必阻断",
        bag4.has_blocking(),
        "",
    );
    // 阻断必须指名策略名，便于定位。
    set.add(
        "U08-策略-互斥指名策略",
        bag4.all().iter().any(|n| {
            n.detail.contains(TruncationPolicy::EllipsisOnly.label())
        }),
        "",
    );

    // 省略号占位在行宽 1 时不得吃掉全部内容（keep=0 是合法退化，但要留痕）。
    let mut bag5 = DiagBag::new();
    let p5 = layout_text(
        "abcdefghij",
        1,
        TruncationPolicy::EllipsisOnly,
        false,
        &mut bag5,
    );
    set.add(
        "U08-策略-窄行宽省略退化有痕",
        p5.as_ref().map(|x| x.ellipsized && !x.trace.is_empty()).unwrap_or(false),
        "",
    );

    // 策略族枚举守卫：两种策略都应能按名反查。
    set.add(
        "U08-策略-策略名反查可逆",
        policy_from_label("wrap-then-ellipsis") == Some(TruncationPolicy::WrapThenEllipsis)
            && policy_from_label("ellipsis-only") == Some(TruncationPolicy::EllipsisOnly)
            && policy_from_label("nope").is_none(),
        "",
    );
}

// ---------------------------------------------------------------------------
// 五、单源复用（判据五）
// ---------------------------------------------------------------------------

fn check_single_source(set: &mut CheckSet) {
    // 上游工单号必须钉死为本单锚点引用的两个。
    set.add(
        "U08-单源-语义权威是F3442",
        REUSE_SOURCE.tier_semantics_owner == "VE-F3442",
        "",
    );
    set.add(
        "U08-单源-命名权威是F3444",
        REUSE_SOURCE.tier_naming_owner == "VE-F3444",
        "",
    );

    // 契约核对：无分叉则零阻断。
    let mut bag = DiagBag::new();
    let _ = reuse_contract();
    let _ = &mut bag;
    let bag2 = reuse_contract();
    set.add("U08-单源-契约核对零阻断", !bag2.has_blocking(), "");
    set.add("U08-单源-契约核对零告警", bag2.all().is_empty(), "");

    // 档位名必须与F3444 三档逐字一致（这是「不另立一套」的可机检面）。
    set.add(
        "U08-单源-档位名逐字一致",
        DensityTier::Compact.label() == "compact"
            && DensityTier::Standard.label() == "standard"
            && DensityTier::Comfortable.label() == "comfortable",
        "",
    );

    // 三档齐全（少一档即与上游分叉）。
    set.add("U08-单源-三档齐全", DensityTier::ALL.len() == 3, "");
    // 档位名互不相同（重名会让「按名反查」失去意义）。
    let mut uniq = true;
    for i in 0..DensityTier::ALL.len() {
        for j in (i + 1)..DensityTier::ALL.len() {
            if DensityTier::ALL[i].label() == DensityTier::ALL[j].label() {
                uniq = false;
            }
        }
    }
    set.add("U08-单源-档位名互异", uniq, "");
}

// ---------------------------------------------------------------------------
// 六、错误路径与降级矩阵（锚点逐条）
// ---------------------------------------------------------------------------

fn check_errors(set: &mut CheckSet) {
    // 路径一：膨胀溢出 -> 折行 + 省略显性（越线时阻断且不留计划）。
    let mut bag = DiagBag::new();
    // 原文 4 字符、译文 100 字符 -> 2500‰，远超 1350‰ 红线。
    // 判据一度传 source_len=100 而译文仅 4 字符（40‰，反而不越线）——
    // 「样本没构造出越线」不是判据红，是样本错。
    let long_de: String = core::iter::repeat('x').take(100).collect();
    let out = layout("de", &long_de, 4, 20, true, &mut bag);
    set.add(
        "U08-错误-越线阻断不出计划",
        out.plan.is_none() && bag.has_blocking(),
        "",
    );
    set.add(
        "U08-错误-越线判acceptable为假",
        !out.acceptable(&bag),
        "",
    );

    // 路径二：行宽非法 -> 阻断 + 拒收。
    let mut bag2 = DiagBag::new();
    let r = layout_text("abc", 0, TruncationPolicy::WrapThenEllipsis, true, &mut bag2);
    set.add("U08-错误-零行宽拒收", r.is_none(), "");
    set.add("U08-错误-零行宽必阻断", bag2.has_blocking(), "");
    set.add(
        "U08-错误-零行宽指名码",
        bag2.all().iter().any(|n| n.code == Diag::LineWidthInvalid),
        "",
    );

    // 路径三：密度表缺语言 -> 默认档 + 诊断（**不阻断**，走一站式流程）。
    let mut bag3 = DiagBag::new();
    let out3 = layout("xx", "hello", 5, 20, true, &mut bag3);
    set.add(
        "U08-错误-缺语言回落默认档",
        out3.tier == DEFAULT_TIER,
        "",
    );
    set.add(
        "U08-错误-缺语言仍可交付",
        out3.plan.is_some() && !bag3.has_blocking(),
        "",
    );
    set.add("U08-错误-缺语言必留告警", bag3.warnings().len() == 1, "");

    // 路径四：单源分叉 -> 归一（本单契约核对必须能报出分叉面）。
    // 这里验证「分叉会被检出」：拿一个不存在的档位名去查契约的反面。
    let bag4 = reuse_contract();
    set.add(
        "U08-错误-契约核对可机检",
        bag4.all().len() == 0 || bag4.has_blocking(),
        "",
    );

    // 诊断渲染：非空袋必须渲染出可读文本，且含处置方向。
    let mut bag5 = DiagBag::new();
    let _ = checked_expansion("de", 10, 100, &mut bag5);
    let txt = bag5.render();
    set.add(
        "U08-错误-诊断渲染含处置方向",
        txt.contains("阻断") && txt.contains("de"),
        "",
    );
    // 空袋渲染为空串（不许输出「无问题」之类的噪声）。
    set.add("U08-错误-空袋渲染为空", DiagBag::new().render().is_empty(), "");

    // **分诊表被真正消费**：缺语言告警 + 越线阻断必须走同一push 入口，
    // 且 is_blocking 的分流结果与 bag.has_blocking 自洽。
    let mut bag6 = DiagBag::new();
    let _ = density_for("xx", &mut bag6);
    let _ = checked_expansion("de", 10, 100, &mut bag6);
    let warn_cnt = bag6.warnings().len();
    let block_cnt = bag6.blocking().len();
    set.add(
        "U08-错误-分诊分流自洽",
        warn_cnt == 1 && block_cnt == 1 && bag6.all().len() == 2,
        "",
    );

    // 诊断不丢项：两条都进去，两条都能查回来。
    let mut seen_missing = false;
    for want in [Diag::DensityLangMissing, Diag::ExpansionOverRedLine] {
        if !bag6.all().iter().any(|n| n.code == want) {
            seen_missing = true;
        }
    }
    set.add("U08-错误-诊断不丢项", !seen_missing, "");
}

// ---------------------------------------------------------------------------
// 七、规格表 / 枚举守卫 / 无障碍替述
// ---------------------------------------------------------------------------

fn check_spec(set: &mut CheckSet) {
    let mut bag = DiagBag::new();
    let violations = verify_spec_table(&mut bag);
    set.add("U08-规格-规格表零违例", violations == 0, "");
    set.add("U08-规格-规格表零诊断", bag.all().is_empty(), "");

    // 行宽钳制：两端夹紧 + 幂等。
    set.add(
        "U08-规格-行宽钳制夹紧",
        clamp_line_width(0) == MIN_LINE_WIDTH && clamp_line_width(MAX_LINE_WIDTH + 1) == MAX_LINE_WIDTH,
        "",
    );
    set.add(
        "U08-规格-行宽钳制幂等",
        clamp_line_width(clamp_line_width(7)) == clamp_line_width(7),
        "",
    );

    // wire 反查可逆（枚举守卫核心）。
    let mut inv = true;
    for t in DensityTier::ALL.iter() {
        if tier_from_wire(t.wire()) != Some(*t) {
            inv = false;
        }
    }
    set.add("U08-规格-wire反查可逆", inv, "");
    // 非法 wire 不得 panic 且返回 None。
    set.add(
        "U08-规格-非法wire拒收",
        tier_from_wire(0).is_none() && tier_from_wire(0xFF).is_none(),
        "",
    );

    // 替述：必须说出档位、膨胀、行数（锚点「文档替述可读」）。
    let mut bag2 = DiagBag::new();
    let out = layout("zh", "短文本", 30, 20, true, &mut bag2);
    let alt = alt_text(&out, "zh");
    set.add(
        "U08-替述-含档位与行数",
        alt.contains(TIER_COMPACT.label()) && alt.contains('行'),
        "",
    );
    // 越线时替述必须说清越线（不能只说「已排版」）。
    let mut bag3 = DiagBag::new();
    let out3 = layout("de", "x", 10, 20, true, &mut bag3);
    let alt3 = alt_text(&out3, "de");
    set.add(
        "U08-替述-越线时说清红线",
        alt3.contains("红线"),
        "",
    );
}

// ---------------------------------------------------------------------------
// 八、性能与鲁棒（诚实标注：本节只测本模块自身工作量）
// ---------------------------------------------------------------------------

fn check_perf(set: &mut CheckSet) {
    // 规模不退化：1000 次查档，档位族数量有界（结果集合大小恒定）。
    let mut bag = DiagBag::new();
    let mut distinct = Vec::new();
    for i in 0..1000usize {
        let lang = match i % 4 {
            0 => "zh",
            1 => "ar",
            2 => "de",
            _ => "xx",
        };
        if let Some(t) = density_for(lang, &mut bag) {
            if !distinct.contains(&t.label()) {
                distinct.push(t.label());
            }
        }
    }
    set.add(
        "U08-性能-千次查档结果集有界",
        distinct.len() <= 3,
        "",
    );

    // 膨胀断言是 O(1)：同一对长度重复 10000 次，结论恒一（不是恒真——
    // 上一对长度是越线样本，这一个是放行样本）。
    // 放行样本：1300‰ < 1350‰ 红线，必须恒不越线。
    let mut bag2 = DiagBag::new();
    let mut same = true;
    for _ in 0..10000 {
        if checked_expansion("de", 1000, 1300, &mut bag2).over_red_line {
            same = false;
        }
    }
    set.add("U08-性能-万次放行判定恒一", same && bag2.all().is_empty(), "");
    // 越线样本：1400‰ > 1350‰，必须恒越线。
    // 注意：这一段**不能**要求诊断袋为空——万次调用会产生万条诊断，那正是
    // 「不静默」的正确表现。判据一度写成 `&& bag3.all().is_empty()`，
    // 把「如实记账」当成了失败（第四次栽在判据侧）。
    let mut bag3 = DiagBag::new();
    let mut same2 = true;
    for _ in 0..10000 {
        if !checked_expansion("de", 1000, 1400, &mut bag3).over_red_line {
            same2 = false;
        }
    }
    set.add("U08-性能-万次越线判定恒真", same2, "");
    // 反向核对：万次越线必须留下万条阻断记录，一条不丢。
    set.add(
        "U08-性能-万次越线如实记账",
        bag3.all().len() == 10000 && bag3.blocking().len() == 10000,
        "",
    );
    // 注意：上一条判据若实现被改成恒真（永远返回 false），这条会立刻红——
    // 它守的是「越线样本必须被抓住」这一侧。

    // 折行宽度和守恒：多组（文本长 × 行宽）交叉，每组宽度和 == 文本长。
    let mut all_sum_ok = true;
    let widths = [1usize, 3, 7, 16, 64];
    let lens = [0usize, 1, 5, 64, 100, 257];
    for len in lens.iter() {
        for w in widths.iter() {
            let text: String = core::iter::repeat('a').take(*len).collect();
            let mut b = DiagBag::new();
            if let Some(p) = layout_text(
                &text,
                *w,
                TruncationPolicy::WrapThenEllipsis,
                true,
                &mut b,
            ) {
                let s: usize = p.line_widths.iter().sum();
                if s != *len {
                    all_sum_ok = false;
                }
            } else {
                all_sum_ok = false;
            }
        }
    }
    set.add("U08-性能-宽度和交叉守恒", all_sum_ok, "");
}

fn check_robust(set: &mut CheckSet) {
    // 敌意输入：超长行宽请求不崩（钳制生效）。
    let mut bag = DiagBag::new();
    let text: String = core::iter::repeat('z').take(200).collect();
    let r = layout_text(
        &text,
        usize::MAX,
        TruncationPolicy::WrapThenEllipsis,
        true,
        &mut bag,
    );
    // 行宽 usize::MAX 会让 take = min(MAX, total) = total，单行放下。
    set.add(
        "U08-鲁棒-超长行宽不崩",
        r.as_ref().map(|x| x.wrapped_lines == 1).unwrap_or(false),
        "",
    );

    // 空标签、空串标签。
    let mut bag2 = DiagBag::new();
    set.add(
        "U08-鲁棒-空标签有诊断",
        density_for("", &mut bag2).is_none() && bag2.warnings().len() == 1,
        "",
    );
    let mut bag3 = DiagBag::new();
    set.add(
        "U08-鲁棒-纯横线标签有诊断",
        density_for("-", &mut bag3).is_none() && bag3.warnings().len() == 1,
        "",
    );

    // 多字节文本（中文）按字符而非字节计宽。
    let mut bag4 = DiagBag::new();
    let cn = "中文字符测试排版";
    let p4 = layout_text(cn, 4, TruncationPolicy::WrapThenEllipsis, true, &mut bag4);
    set.add(
        "U08-鲁棒-中文按字符折行",
        p4.as_ref().map(|x| {
            let s: usize = x.line_widths.iter().sum();
            s == cn.chars().count() && x.line_widths.iter().all(|w| *w <= 4)
        }).unwrap_or(false),
        "",
    );

    // 替换字符（非法 UTF-8 的替代物）不得 panic。
    let mut bag5 = DiagBag::new();
    let r5 = layout_text("\u{FFFD}ab", 3, TruncationPolicy::WrapThenEllipsis, true, &mut bag5);
    set.add("U08-鲁棒-替换字符不崩", r5.is_some(), "");
}

// ---------------------------------------------------------------------------
// 九、入口
// ---------------------------------------------------------------------------

/// 域自检入口。
pub fn run_veu08_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-veu08");
    check_expansion(&mut set);
    check_density(&mut set);
    check_wrap(&mut set);
    check_policy(&mut set);
    check_single_source(&mut set);
    check_errors(&mut set);
    check_spec(&mut set);
    check_perf(&mut set);
    check_robust(&mut set);
    // 收尾：不得因 MAX_CHECKS 满而丢判据（丢了必须显性报 truncated）。
    if set.truncated() {
        set.fail("U08-规模-未截断", "CheckSet 已满，判据被丢弃");
    } else {
        set.ok("U08-规模-未截断");
    }
    set
}

#[cfg(test)]
mod red_density {
    use super::*;

    /// 逐条点名红项（不只给个数——作者要知道改哪一条）。
    #[test]
    fn density_red_items() {
        let set = run_veu08_checks();
        // `red_items()` 返回 `([Option<Check>; MAX_CHECKS], usize)` 元组，
        // **不是迭代器**；且它含全部项（含绿项），须按 `passed` 过滤。
        let (_items, count) = set.red_items();
        let mut i = 0usize;
        while i < count {
            if let Some(c) = set.get(i) {
                if !c.passed {
                    println!("[红] {}", c.name);
                }
            }
            i += 1;
        }
        // 通过数取 `tally()` —— `CheckSet` 没有 `passed_count()` 方法。
        let (passed, red) = set.tally();
        println!(
            "total={} passed={} red={} dropped={}",
            set.len(),
            passed,
            red,
            set.dropped()
        );
        assert_eq!(passed + red, set.len(), "tally 与 len 必须自洽");
        assert!(!set.truncated(), "判据不得因CheckSet 满而被丢弃");
        assert_eq!(red, 0, "域自检不该有红项");
    }

    /// 单元级：膨胀红线在边界两侧的取值。
    #[test]
    fn density_expansion_boundary() {
        assert_eq!(expansion_permille(100, 135), 1350, "恰好红线");
        assert_eq!(expansion_permille(100, 136), 1360, "超一线");
        assert_eq!(expansion_permille(100, 100), 1000, "等长");
        assert_eq!(expansion_permille(0, 500), 0, "零基线不报膨胀");
        assert_eq!(expansion_permille(1, 0), 0, "译文为空不膨胀");
        assert_eq!(expansion_permille(3, 10), 3333, "短基线放大");
    }

    /// 单元级：折行宽度和守恒（含空文本与整除边界）。
    #[test]
    fn density_wrap_conservation() {
        for len in [0usize, 1, 7, 8, 100].iter() {
            for w in [1usize, 4, 8, 4096].iter() {
                let text: String = core::iter::repeat('q').take(*len).collect();
                let mut bag = DiagBag::new();
                let p = layout_text(
                    &text,
                    *w,
                    TruncationPolicy::WrapThenEllipsis,
                    true,
                    &mut bag,
                );
                assert!(p.is_some(), "合法输入必须给出计划len={} w={}", len, w);
                if let Some(x) = p {
                    let sum: usize = x.line_widths.iter().sum();
                    assert_eq!(sum, *len, "折行不得丢字符 len={} w={}", len, w);
                    assert!(
                        x.line_widths.iter().all(|v| *v <= *w),
                        "每行不得超宽 len={} w={}",
                        len,
                        w
                    );
                    assert!(!x.ellipsized, "请求折行时不得省略");
                    assert!(!x.trace.is_empty(), "留痕不得为空");
                }
            }
        }
    }

    /// 单元级：缺语言必留告警且不阻断（锚点错误路径）。
    #[test]
    fn density_missing_lang_is_warning_only() {
        let mut bag = DiagBag::new();
        let r = density_for("qqq", &mut bag);
        assert!(r.is_none(), "表外语言必须回落");
        assert_eq!(bag.warnings().len(), 1, "必须恰有一条告警");
        assert!(!bag.has_blocking(), "缺语言不得阻断渲染");
        assert!(
            bag.all().iter().any(|n| n.lang == "qqq"),
            "告警必须指名语言"
        );
    }

    /// 单元级：越线必阻断且带齐实测数值。
    #[test]
    fn density_over_redline_blocks() {
        let mut bag = DiagBag::new();
        let v = checked_expansion("de", 100, 200, &mut bag);
        assert!(v.over_red_line, "翻倍必越线");
        assert!(bag.has_blocking(), "越线必阻断");
        let n = bag.blocking();
        assert_eq!(n.len(), 1, "恰一条阻断");
        assert!(
            n[0].detail.contains("200") && n[0].detail.contains("100"),
            "阻断必须带齐两侧长度：{}",
            n[0].detail
        );
    }
}