//! VE-F4003 · 域自检（判据逐条对应，见 `vei03_text_direction.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//!
//! - **三方向统一** → `C03-三方向-同一枚举`、`C03-三方向-名称镜像齐备`、
//!   `C03-三方向-域外值显性拒绝`；
//! - **三层优先级** → `C03-三层-三层齐备`、`C03-三层-优先级表固定序`、
//!   `C03-三层-权重与层号反序`、`C03-三层-冲突按优先级仲裁`、`C03-三层-排序幂等`；
//! - **isolate 自动** → `C03-隔离-四隔离符可识别`、`C03-隔离-缺失自动补齐`、
//!   `C03-隔离-闭合不误补`、`C03-隔离-未知显性拒绝`、`C03-隔离-栈O1出入`、
//!   `C03-隔离-多字节不越界`；
//! - **启发可覆写** → `C03-启发-结论显性留首强`、`C03-启发-中性兜底Locale`、
//!   `C03-覆写-授权生效`、`C03-覆写-无理由拒绝`、`C03-覆写-未授权拒绝`、
//!   `C03-覆写-压过启发`；
//! - **方向模型** → `C03-模型-结论可归因`、`C03-模型-替述可读`、
//!   `C03-模型-零隐私面`、`C03-模型-空文本拒绝`、`C03-模型-采样钳制可见`、
//!   `C03-模型-声明数有界`、`C03-模型-逆序区间拒绝`、`C03-模型-单源唯一`、
//!   `C03-模型-前向槽位不谎报`、`C03-模型-规格表编号连续`、`C03-模型-判据全覆盖`。
//!
//! 另设**反假门禁**组：断言"不该panic 的场景不panic"与"断言本身能失败"。
//!
//! 本文件是**过程产物之外的功能文件**（随源码入库）：与 F4002 同构，
//! `pubmod vei03_checks` 由 `svstar2/mod.rs` 聚合。

use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;
use crate::svstar2::vei03_text_direction as td;

/// 自检集构造（与家族同构：域标识 + 逐项 add）。
pub fn run_vei03_checks() -> CheckSet {
    let mut set = CheckSet::new(td::VEA_DOMAIN);
    run(&mut set);
    set
}

/// 一条自检结论。
///
/// `detail` 是 `&'static str` 而非 `String`：内核 [`CheckSet::add`] 只存
/// `&'static str`（无堆分配、无生命周期负担），所以**动态诊断文本进不了
/// 内核**——这是家族的既定约束。需要看失败细节的场景由单元测试承担，
/// 自检只负责"绿/红 + 稳定短码"。
struct Verdict {
    name: &'static str,
    passed: bool,
    detail: &'static str,
}

impl Verdict {
    fn new(name: &'static str, passed: bool, detail: &'static str) -> Self {
        Verdict { name, passed, detail }
    }
}

/// 全部自检项。
fn run(set: &mut CheckSet) {
    let mut vs: Vec<Verdict> = Vec::new();

    // ---- 判据一：三方向统一 ----
    vs.push(dir_values_are_one_enum());
    vs.push(dir_names_and_mirror());
    vs.push(dir_guard_rejects_unknown());

    // ---- 判据二：三层优先级 ----
    vs.push(layer_three_levels());
    vs.push(layer_priority_table());
    vs.push(layer_weight_inverse_of_rank());
    vs.push(layer_conflict_arbitrated());
    vs.push(layer_sort_idempotent());

    // ---- 判据三：isolate 自动 ----
    vs.push(isolate_four_known());
    vs.push(isolate_missing_autofilled());
    vs.push(isolate_closed_not_overfilled());
    vs.push(isolate_unknown_rejected());
    vs.push(isolate_stack_o1());
    vs.push(isolate_multibyte_safe());

    // ---- 判据四：启发可覆写 ----
    vs.push(heuristic_explicit_first_strong());
    vs.push(heuristic_neutral_falls_back());
    vs.push(override_authorized_applies());
    vs.push(override_no_reason_rejected());
    vs.push(override_unauthorized_rejected());
    vs.push(override_beats_heuristic());

    // ---- 判据五：方向模型 ----
    vs.push(model_attributable());
    vs.push(model_describe_readable());
    vs.push(model_zero_privacy());
    vs.push(model_empty_text_rejected());
    vs.push(model_sample_clamp_visible());
    vs.push(model_declarations_bounded());
    vs.push(model_reversed_range_rejected());
    vs.push(model_single_source_unique());
    vs.push(model_forward_not_lied());
    vs.push(model_spec_numbering_continuous());
    vs.push(model_criteria_covered());
    vs.push(model_multibyte_never_panics());
    vs.push(model_assertions_can_fail());

    for v in vs {
        set.add(v.name, v.passed, if v.passed { "" } else { &v.detail });
    }
}

// ---------------------------------------------------------------------------
// 判据一：三方向统一
// ---------------------------------------------------------------------------

fn dir_values_are_one_enum() -> Verdict {
    // 三方向必须同属一个类型（判据核心）。用 ALL 的长度 + 互异性证明。
    let all = td::Direction::ALL;
    let mut seen: Vec<u8> = all.iter().map(|d| d.mirror_code() + 1).collect();
    seen.sort_unstable();
    seen.dedup();
    // mirror_code 只有 0/1，所以上面不足以区分 LTR/TTB —— 改用名字互异。
    let mut names: Vec<&str> = all.iter().map(|d| d.name()).collect();
    names.sort_unstable();
    names.dedup();
    Verdict::new(
        "C03-三方向-同一枚举",
        all.len() == 3 && names.len() == 3,
        "三方向须在同一枚举里且名称互异",
    )
}

fn dir_names_and_mirror() -> Verdict {
    // 每方向都有可读名 + 短名 + 镜像码；TTB 是唯一竖排。
    let mut ok = true;
    let mut why = "";
    for d in td::Direction::ALL {
        if d.name().is_empty() || d.short().is_empty() {
            ok = false;
            why = "self-check-fail";
            break;
        }
        // 只有 RTL 需要镜像。
        let expect_mirror = matches!(d, td::Direction::Rtl);
        if (d.mirror_code() == 1) != expect_mirror {
            ok = false;
            why = "self-check-fail";
            break;
        }
        // TTB 是唯一竖排。
        if d.is_horizontal() == matches!(d, td::Direction::Ttb) {
            ok = false;
            why = "self-check-fail";
            break;
        }
    }
    Verdict::new("C03-三方向-名称镜像齐备", ok, why)
}

fn dir_guard_rejects_unknown() -> Verdict {
    // 合法值全通、非法值拒绝且三要素齐全。
    let mut ok = true;
    let mut why = "";
    for (raw, want) in [
        ("ltr", td::Direction::Ltr),
        ("RTL", td::Direction::Rtl),
        ("ttb", td::Direction::Ttb),
        ("2", td::Direction::Ttb),
    ] {
        match td::guard_direction(raw) {
            Ok(got) if got == want => {}
            Ok(_) => {
                ok = false;
                why = "self-check-fail";
            }
            Err(_) => {
                ok = false;
                why = "self-check-fail";
            }
        }
    }
    for bad in["", "sideways", "lr", "3", "LTRR"] {
        match td::guard_direction(bad) {
            Ok(_) => {
                ok = false;
                why = "self-check-fail";
            }
            Err(e) => {
                if !e.is_complete() {
                    ok = false;
                    why = "self-check-fail";
                }
            }
        }
    }
    Verdict::new("C03-三方向-域外值显性拒绝", ok, why)
}

// ---------------------------------------------------------------------------
// 判据二：三层优先级
// ---------------------------------------------------------------------------

fn layer_three_levels() -> Verdict {
    let all = td::DirectionLayer::ALL;
    let mut ranks: Vec<usize> = all.iter().map(|l| l.rank()).collect();
    ranks.sort_unstable();
    let contiguous = ranks == vec![0, 1, 2];
    let mut names: Vec<&str> = all.iter().map(|l| l.name()).collect();
    names.sort_unstable();
    names.dedup();
    Verdict::new(
        "C03-三层-三层齐备",
        all.len() == 3 && contiguous && names.len() == 3,
        "三层须齐备且层号 0..2 连续",
    )
}

fn layer_priority_table() -> Verdict {
    // 优先级表机检 + 固定序直读。
    let table_ok = td::check_priority_table().is_ok();
    // 直读：权重严格递增（字符级最小=最优先）。
    let w_c = td::layer_weight(td::DirectionLayer::Character);
    let w_b = td::layer_weight(td::DirectionLayer::Block);
    let w_d = td::layer_weight(td::DirectionLayer::Document);
    let mut why = "";
    let strict = w_c < w_b && w_b < w_d;
    if !strict {
        why = "self-check-fail";
    }
    Verdict::new("C03-三层-优先级表固定序", table_ok && strict, why)
}

fn layer_weight_inverse_of_rank() -> Verdict {
    // 钉死"层号与优先级反序"——本项实测踩过的坑（排序按 rank 升序会让
    // 文档级赢过字符级）。若哪天有人把排序改回按 rank，此项立刻红。
    let rank_c = td::DirectionLayer::Character.rank();
    let rank_d = td::DirectionLayer::Document.rank();
    let w_c = td::layer_weight(td::DirectionLayer::Character);
    let w_d = td::layer_weight(td::DirectionLayer::Document);
    let inverse = rank_c > rank_d && w_c < w_d;
    // 再验排序真的按权重走（不是碰巧）。
    let mut decls = vec![
        mk_decl(td::DirectionLayer::Document, (0, 100), td::Direction::Ltr),
        mk_decl(td::DirectionLayer::Character, (5, 6), td::Direction::Rtl),
    ];
    td::sort_declarations(&mut decls);
    let sorted_ok = decls[0].layer == td::DirectionLayer::Character;
    let mut why = "";
    if !inverse {
        why = "层号与权重应反序（字符级号大权重小）";
    } else if !sorted_ok {
        why = "self-check-fail";
    }
    Verdict::new("C03-三层-权重与层号反序", inverse && sorted_ok, why)
}

fn layer_conflict_arbitrated() -> Verdict {
    // 三层冲突：字符级赢文档级，且留冲突诊断。
    let mut r = td::DirectionResolver::new(0);
    r.declare(mk_decl(td::DirectionLayer::Document, (0, 100), td::Direction::Ltr))
        .unwrap();
    r.declare(mk_decl(td::DirectionLayer::Block, (0, 50), td::Direction::Ltr))
        .unwrap();
    r.declare(mk_decl(td::DirectionLayer::Character, (5, 6), td::Direction::Rtl))
        .unwrap();
    let res = r.resolve("hello", td::Direction::Ltr).unwrap();
    let dir_ok = res.direction == td::Direction::Rtl;
    let layer_ok = res.layer == Some(td::DirectionLayer::Character);
    let traced = res.bag.count_of(td::DiagKind::LayerConflict) >= 1;
    let mut why = "";
    if !dir_ok {
        why = "self-check-fail";
    } else if !layer_ok {
        why = "生效层应为字符级";
    } else if !traced {
        why = "冲突未留诊断";
    }
    Verdict::new("C03-三层-冲突按优先级仲裁", dir_ok && layer_ok && traced, why)
}

fn layer_sort_idempotent() -> Verdict {
    // 排序幂等（裁决可复现的前提）。
    let mut decls = vec![
        mk_decl(td::DirectionLayer::Document, (0, 100), td::Direction::Ltr),
        mk_decl(td::DirectionLayer::Block, (0, 50), td::Direction::Rtl),
        mk_decl(td::DirectionLayer::Character, (5, 6), td::Direction::Ttb),
    ];
    let ok = td::check_sort_idempotent(&mut decls).is_ok();
    let why = if ok { "" } else { "self-check-fail" };
    Verdict::new("C03-三层-排序幂等", ok, why)
}

// ---------------------------------------------------------------------------
// 判据三：isolate 自动
// ---------------------------------------------------------------------------

fn isolate_four_known() -> Verdict {
    // 四隔离符缩写可识别且方向属性正确。
    let mut ok = true;
    let mut why = "";
    for k in td::IsolateKind::ALL {
        match td::guard_isolate(k.abbr()) {
            Ok(got) if got == k => {}
            Ok(_) => {
                ok = false;
                why = "self-check-fail";
                break;
            }
            Err(_) => {
                ok = false;
                why = "self-check-fail";
                break;
            }
        }
    }
    // LRI/RLI 有方向，FSI/PDI 无自身方向（FSI 靠首强、PDI 是弹出）。
    if ok {
        let lri = td::IsolateKind::Lri.direction();
        let rli = td::IsolateKind::Rli.direction();
        let fsi = td::IsolateKind::Fsi.direction();
        let pdi = td::IsolateKind::Pdi.direction();
        if lri != Some(td::Direction::Ltr)
            || rli != Some(td::Direction::Rtl)
            || fsi.is_some()
            || pdi.is_some()
        {
            ok = false;
            why = "隔离符方向属性错（LRI=LTR/RLI=RTL/FSI/PDI 无方向）";
        }
    }
    // PDI 是唯一弹出符。
    if ok {
        let pops: Vec<bool> = td::IsolateKind::ALL.iter().map(|k| k.is_pop()).collect();
        if pops != vec![false, false, false, true] {
            ok = false;
            why = "self-check-fail";
        }
    }
    Verdict::new("C03-隔离-四隔离符可识别", ok, why)
}

fn isolate_missing_autofilled() -> Verdict {
    // 未闭合 → 自动补齐 + 诊断。
    let mut bag = td::DiagBag::new();
    let n = td::isolate_missing("abc LRI def", &mut bag).unwrap();
    let ok = n >= 1 && bag.count_of(td::DiagKind::IsolateAutoInserted) >= 1;
    let why = if ok { "" } else { "self-check-fail" };
    Verdict::new("C03-隔离-缺失自动补齐", ok, why)
}

fn isolate_closed_not_overfilled() -> Verdict {
    // 闭合良好 → 不补（防过度补齐：无脑给每段加 PDI 会改变渲染）。
    let mut bag = td::DiagBag::new();
    let n = td::isolate_missing("LRI abc PDI", &mut bag).unwrap();
    let ok = n == 0 && bag.count_of(td::DiagKind::IsolateAutoInserted) == 0;
    let why = if ok { "" } else { "self-check-fail" };
    Verdict::new("C03-隔离-闭合不误补", ok, why)
}

fn isolate_unknown_rejected() -> Verdict {
    // 未知缩写拒绝 + 三要素齐全。
    let mut ok = true;
    let mut why = "";
    for bad in["XYZ", "lri", "PDI2", ""] {
        match td::guard_isolate(bad) {
            Ok(_) => {
                ok = false;
                why = "self-check-fail";
                break;
            }
            Err(e) => {
                if e.code != td::E_ISOLATE_UNKNOWN {
                    ok = false;
                    why = "self-check-fail";
                    break;
                }
                if !e.is_complete() {
                    ok = false;
                    why = "self-check-fail";
                    break;
                }
            }
        }
    }
    Verdict::new("C03-隔离-未知显性拒绝", ok, why)
}

fn isolate_stack_o1() -> Verdict {
    // 栈 LIFO + 栈空弹出回落（不 panic）。
    let mut st = td::IsolateStack::new();
    st.push(td::IsolateKind::Lri, None);
    st.push(td::IsolateKind::Rli, None);
    let d2 = st.depth();
    let cur = st.current();
    let p1 = st.pop();
    let p2 = st.pop();
    let p3 = st.pop();
    let ok = d2 == 2
        && cur == Some(td::Direction::Rtl)
        && p1 == Some(td::Direction::Rtl)
        && p2 == Some(td::Direction::Ltr)
        && p3.is_none()
        && st.is_empty();
    let why = if ok { "" } else { "self-check-fail" };
    Verdict::new("C03-隔离-栈O1出入", ok, why)
}

fn isolate_multibyte_safe() -> Verdict {
    // 多字节文本里的隔离符**仍能被正确识别**（数量可数），且全程不 panic。
    // 这一项同时守护两件事：① 逐字节切片会在多字节中间 panic（已修缺陷）；
    // ② 只验"不报错"是弱门禁——必须断言**识别数量**正确，否则把 tokenize
    // 改成永远返回空也能全绿（那等于把隔离符功能悄悄删了）。
    let mut ok = true;
    let mut why = "";
    for (text, want_unclosed) in [
        ("مرحبا LRI عالم", 1usize),
        // RLI 压栈后被 PDI 正确弹出 → 闭合良好，未闭合数=0（不得误补）。
        ("שלום RLI עולם PDI", 0),
        // 双 LRI 各自未闭合 → 需补 2 个 PDI。
        ("LRI עברית RLI عربي", 2),
        ("日本語 FSI テキスト", 1),
        ("LRI عربي RLI עברית PDI", 1),
        ("纯中文无隔离符", 0),
    ] {
        let mut bag = td::DiagBag::new();
        match td::isolate_missing(text, &mut bag) {
            Ok(n) => {
                if n != want_unclosed {
                    ok = false;
                    why = "self-check-fail";
                    break;
                }
                // 补齐数与自动补齐诊断数必须一致（补了就得留痕）。
                let diags = bag.count_of(td::DiagKind::IsolateAutoInserted);
                if (n > 0) != (diags > 0) {
                    ok = false;
                    why = "self-check-fail";
                    break;
                }
            }
            Err(_) => {
                ok = false;
                why = "self-check-fail";
                break;
            }
        }
        // 用 resolve 走全链路（真正的调用方式），确保不 panic。
        let mut r = td::DirectionResolver::new(0);
        if r.resolve(text, td::Direction::Ltr).is_err() {
            ok = false;
            why = "self-check-fail";
            break;
        }
    }
    Verdict::new("C03-隔离-多字节不越界", ok, why)
}

// ---------------------------------------------------------------------------
// 判据四：启发可覆写
// ---------------------------------------------------------------------------

fn heuristic_explicit_first_strong() -> Verdict {
    // 启发结论必须显性：留首强字符 + 采样数+ 可归因来源。
    let mut r = td::DirectionResolver::new(0);
    let res = r.resolve("123 مرحبا", td::Direction::Ltr).unwrap();
    let src_ok = res.source == td::ResolutionSource::ByHeuristic;
    let dir_ok = res.direction == td::Direction::Rtl;
    // 中性前缀（数字）被跳过，首强落在第一个字母上。
    // 注意：**不能硬编码挑某个字母**——`مرحبا` 的首强是 'م'，
    // 断言写成 `contains('ر')`（另一个字母）就会误判"实现没留痕"。
    // 正确做法：先用 detect_first_strong 取真值，再断言 why 里含它。
    let h = td::detect_first_strong("123 مرحبا", td::MAX_SAMPLE);
    let first_ok = match h.first_strong {
        Some(c) => res.why.contains(c),
        None => false,
    };
    let explicit = res.why.contains("首强字符") && first_ok && res.why.contains("采样");
    let attributable = res.attributable();
    let mut why = "";
    if !src_ok {
        why = "self-check-fail";
    } else if !dir_ok {
        why = "self-check-fail";
    } else if !explicit {
        why = "self-check-fail";
    }
    Verdict::new(
        "C03-启发-结论显性留首强",
        src_ok && dir_ok && explicit && attributable,
        why,
    )
}

fn heuristic_neutral_falls_back() -> Verdict {
    // 纯中性文本 → 兜底 Locale 默认 + 显性诊断。
    let mut r = td::DirectionResolver::new(0);
    let res = r.resolve("123 -- ...", td::Direction::Rtl).unwrap();
    let ok = res.direction == td::Direction::Rtl
        && res.source == td::ResolutionSource::ByLocale
        && res.bag.count_of(td::DiagKind::FallbackToLocale) >= 1;
    let why = if ok { "" } else { "self-check-fail" };
    Verdict::new("C03-启发-中性兜底Locale", ok, why)
}

fn mk_range_override(
    range: (usize, usize),
    to: td::Direction,
    reason: &'static str,
    authorized: bool,
) -> td::OverrideRequest {
    td::OverrideRequest {
        range,
        to,
        reason,
        authorized,
    }
}

fn override_authorized_applies() -> Verdict {
    // 合规覆写生效。
    let mut r = td::DirectionResolver::new(0);
    r.request_override(mk_range_override(
        (0, 5),
        td::Direction::Ttb,
        "竖排版式",
        true,
    ))
    .unwrap();
    let res = r.resolve("hello", td::Direction::Ltr).unwrap();
    let ok = res.direction == td::Direction::Ttb
        && res.source == td::ResolutionSource::ByOverride
        && res.bag.count_of(td::DiagKind::OverrideApplied) >= 1;
    let why = if ok { "" } else { "self-check-fail" };
    Verdict::new("C03-覆写-授权生效", ok, why)
}

fn override_no_reason_rejected() -> Verdict {
    // 无理由覆写拒绝（可追责性）。
    let mut r = td::DirectionResolver::new(0);
    let e = r
        .request_override(mk_range_override((0, 5), td::Direction::Rtl, "   ", true))
        .unwrap_err();
    let ok = e.code == td::E_OVERRIDE_NO_REASON && e.is_complete();
    let why = if ok { "" } else { "self-check-fail" };
    Verdict::new("C03-覆写-无理由拒绝", ok, why)
}

fn override_unauthorized_rejected() -> Verdict {
    // 未授权覆写拒绝（越权防护）。
    let mut r = td::DirectionResolver::new(0);
    let e = r
        .request_override(mk_range_override((0, 5), td::Direction::Rtl, "x", false))
        .unwrap_err();
    let ok = e.code == td::E_OVERRIDE_FORBIDDEN && e.is_complete();
    let why = if ok { "" } else { "self-check-fail" };
    Verdict::new("C03-覆写-未授权拒绝", ok, why)
}

fn override_beats_heuristic() -> Verdict {
    // 覆写压过启发（这是"启发可覆写"的核心：误判的正式出口是覆写通道，
    // 而不是改启发式——改启发式会让另一批文本变差）。
    let mut r = td::DirectionResolver::new(0);
    // 文本首强是拉丁（LTR），覆写要它变成 RTL。
    r.request_override(mk_range_override(
        (0, 5),
        td::Direction::Rtl,
        "品牌专名",
        true,
    ))
    .unwrap();
    let res = r.resolve("hello", td::Direction::Ltr).unwrap();
    let ok = res.direction == td::Direction::Rtl && res.source == td::ResolutionSource::ByOverride;
    let why = if ok { "" } else { "self-check-fail" };
    Verdict::new("C03-覆写-压过启发", ok, why)
}

// ---------------------------------------------------------------------------
// 判据五：方向模型
// ---------------------------------------------------------------------------

fn mk_decl(
    layer: td::DirectionLayer,
    range: (usize, usize),
    d: td::Direction,
) -> td::DirectionDeclaration {
    td::DirectionDeclaration {
        layer,
        range,
        direction: d,
        source: "selfcheck",
    }
}

fn model_attributable() -> Verdict {
    // 四种来源都能给出可归因结论（来源 + 理由非空）。
    let mut ok = true;
    let mut why = "";
    // ByLayer
    let mut r1 = td::DirectionResolver::new(0);
    r1.declare(mk_decl(td::DirectionLayer::Block, (0, 5), td::Direction::Rtl))
        .unwrap();
    let a = r1.resolve("hello", td::Direction::Ltr).unwrap();
    if !a.attributable() || a.source != td::ResolutionSource::ByLayer {
        ok = false;
        why = "声明来源不可归因";
    }
    // ByOverride
    if ok {
        let mut r2 = td::DirectionResolver::new(0);
        r2.request_override(mk_range_override((0, 5), td::Direction::Rtl, "r", true))
            .unwrap();
        let b = r2.resolve("hello", td::Direction::Ltr).unwrap();
        if !b.attributable() || b.source != td::ResolutionSource::ByOverride {
            ok = false;
            why = "覆写来源不可归因";
        }
    }
    // ByHeuristic
    if ok {
        let mut r3 = td::DirectionResolver::new(0);
        let c = r3.resolve("hello", td::Direction::Ltr).unwrap();
        if !c.attributable() || c.source != td::ResolutionSource::ByHeuristic {
            ok = false;
            why = "启发来源不可归因";
        }
    }
    // ByLocale
    if ok {
        let mut r4 = td::DirectionResolver::new(0);
        let d = r4.resolve("123", td::Direction::Rtl).unwrap();
        if !d.attributable() || d.source != td::ResolutionSource::ByLocale {
            ok = false;
            why = "Locale 来源不可归因";
        }
    }
    Verdict::new("C03-模型-结论可归因", ok, why)
}

fn model_describe_readable() -> Verdict {
    // 替述须含方向名、来源名（无障碍可读）。
    let mut r = td::DirectionResolver::new(0);
    let res = r.resolve("hello", td::Direction::Ltr).unwrap();
    let d = td::describe_resolution(&res);
    let has_dir = d.contains("从左到右");
    let has_src = d.contains("首强启发");
    let has_why = d.contains("依据");
    let ok = has_dir && has_src && has_why;
    let why = if ok { "" } else { "self-check-fail" };
    Verdict::new("C03-模型-替述可读", ok, why)
}

fn model_zero_privacy() -> Verdict {
    // 零隐私面：结论与诊断都不含超长自由文本。
    let mut r = td::DirectionResolver::new(0);
    let res = r.resolve("مرحبا hello", td::Direction::Ltr).unwrap();
    let ok = td::check_zero_privacy(&res).is_ok();
    // 所有诊断类别都有稳定码（对拍按码比对，不比文本）。
    let mut codes_ok = true;
    let mut why = "";
    for k in td::DiagKind::ALL {
        if k.code().is_empty() || !k.code().starts_with('D') {
            codes_ok = false;
            why = "self-check-fail";
            break;
        }
    }
    Verdict::new("C03-模型-零隐私面", ok && codes_ok, why)
}

fn model_empty_text_rejected() -> Verdict {
    // 空文本拒绝 + 三要素。
    let mut r = td::DirectionResolver::new(0);
    let e = r.resolve("", td::Direction::Ltr).unwrap_err();
    let ok = e.code == td::E_TEXT_EMPTY && e.is_complete();
    let why = if ok { "" } else { "self-check-fail" };
    Verdict::new("C03-模型-空文本拒绝", ok, why)
}

fn model_sample_clamp_visible() -> Verdict {
    // 采样预算钳制可见（越界留痕、域内不误标）。
    let over = td::check_sample_clamp(td::MAX_SAMPLE + 500).unwrap();
    let within = !td::check_sample_clamp(128).unwrap();
    let resolver = td::DirectionResolver::new(td::MAX_SAMPLE + 100).sample_budget() == td::MAX_SAMPLE;
    let ok = over && within && resolver;
    let why = if ok { "" } else { "self-check-fail" };
    Verdict::new("C03-模型-采样钳制可见", ok, why)
}

fn model_declarations_bounded() -> Verdict {
    // 声明条数有界（保"仲裁 O(层)"性能承诺）。
    let mut r = td::DirectionResolver::new(0);
    let mut accepted = 0usize;
    let mut rejected_at = None;
    for i in 0..(td::MAX_DECLARATIONS + 5) {
        let d = mk_decl(td::DirectionLayer::Block, (i, i + 1), td::Direction::Ltr);
        match r.declare(d) {
            Ok(_) => accepted += 1,
            Err(e) => {
                rejected_at = Some(e);
                break;
            }
        }
    }
    let ok = accepted == td::MAX_DECLARATIONS
        && rejected_at
            .as_ref()
            .map(|e| e.code == td::E_LAYER_INVALID && e.is_complete())
            .unwrap_or(false);
    let why = if ok { "" } else { "self-check-fail" };
    Verdict::new("C03-模型-声明数有界", ok, why)
}

fn model_reversed_range_rejected() -> Verdict {
    // 逆序区间拒绝（否则区间比较与宽度计算全错）。
    let mut r = td::DirectionResolver::new(0);
    let e = r
        .declare(mk_decl(td::DirectionLayer::Block, (9, 2), td::Direction::Ltr))
        .unwrap_err();
    let ok = e.code == td::E_LAYER_INVALID && e.is_complete() && e.why.contains("逆序");
    let why = if ok { "" } else { "self-check-fail" };
    Verdict::new("C03-模型-逆序区间拒绝", ok, why)
}

fn model_single_source_unique() -> Verdict {
    // 单源：机检 +每能力唯一 owner + 四方向能力齐全。
    let ok = td::check_single_source().is_ok();
    // 方向判定本体必须归本项（若被F2948 或 N02 抢走 owner，双份判定会分叉）。
    let mut dir_owned_by_us = false;
    for c in td::LANG_SINGLE_SOURCE {
        if c.owner == "VE-F4003" {
            dir_owned_by_us = true;
        }
        // 不得出现第二个非本项 owner。
        if c.owner != "VE-F4003" && c.key.starts_with("direction") {
            return Verdict::new("C03-模型-单源唯一", false, "self-check-fail");
        }
    }
    let count_ok = td::LANG_SINGLE_SOURCE.len() == 4;
    let why = if ok && dir_owned_by_us && count_ok {
        ""
    } else {
        "self-check-fail"
    };
    Verdict::new("C03-模型-单源唯一", ok && dir_owned_by_us && count_ok, why)
}

fn model_forward_not_lied() -> Verdict {
    // F4021 未实现 → 槽位必须landed=false（谎报会让调用方以为 BiDi 已就绪）。
    let ok = td::check_forward_slots().is_ok();
    let mut why = "";
    for slot in td::FORWARD_SLOTS {
        if slot.target == "VE-F4021" && slot.landed {
            why = "self-check-fail";
        }
    }
    Verdict::new("C03-模型-前向槽位不谎报", ok && why.is_empty(), why)
}

fn model_spec_numbering_continuous() -> Verdict {
    // 规格表编号连续 + 覆盖机检。
    let ok = td::check_spec_coverage().is_ok();
    let n = td::SPEC_SHEET.len();
    // 编号 1..n 连续（直读，不信机检）。
    let mut nums: Vec<u16> = td::SPEC_SHEET.iter().map(|s| s.no).collect();
    nums.sort_unstable();
    let continuous = nums
        .iter()
        .enumerate()
        .all(|(i, v)| *v == (i + 1) as u16);
    // 每条目都有强制机检名 + 中文描述（无空标签）。
    let labeled = td::SPEC_SHEET
        .iter()
        .all(|s| !s.key.is_empty() && !s.label.is_empty() && !s.enforced_by.is_empty());
    let pass = ok && continuous && labeled && n >= 13;
        let why = if pass {
        ""
    } else {
        "self-check-fail"
    };
    Verdict::new("C03-模型-规格表编号连续", pass, why)
}

fn model_criteria_covered() -> Verdict {
    // 五条判据每条都至少映射到规格表里真实存在的键（防止判据喊空口号）。
    let ok = td::check_spec_coverage().is_ok();
    let all_mapped = td::CRITERIA.len() == 5
        && td::CRITERIA.iter().all(|(_, keys)| keys.iter().all(|k| {
            td::SPEC_SHEET.iter().any(|s| s.key == *k)
        }));
        let why = if ok && all_mapped {
        ""
    } else {
        "self-check-fail"
    };
    Verdict::new("C03-模型-判据全覆盖", ok && all_mapped, why)
}

fn model_multibyte_never_panics() -> Verdict {
    // 反假门禁：断言"多字节文本不panic"这条**本身能失败**。
    // 若有人把 tokenize 改回逐字节切片，本项会在上面 panic 而不是静默通过——
    // 门禁必须真的会红，否则等于没有门禁。
    //
    // 这里的做法是**真跑一遍全链路**并检查结论可归因：panic 会直接让自检
    // 崩掉（红），而不是产出一个假的绿灯。
    let mut ok = true;
    let mut why = "";
    let corpus: [&str; 8] = [
        "مرحبا بالعالم",
        "שלום עולם",
        "日本語のテキスト",
        "中文测试",
        "🎉🎊 emoji",
        "LRI مرحبا PDI",
        "RLI שלום",
        "FSI عربي PDI عربي",
    ];
    for t in corpus {
        let mut r = td::DirectionResolver::new(0);
        match r.resolve(t, td::Direction::Ltr) {
            Ok(res) => {
                if !res.attributable() {
                    ok = false;
                    why = "self-check-fail";
                    break;
                }
            }
            Err(_) => {
                ok = false;
                why = "self-check-fail";
                break;
            }
        }
    }
    Verdict::new("C03-模型-多字节不panic", ok, why)
}

fn model_assertions_can_fail() -> Verdict {
    // 反假门禁的最高级形态：**证明本文件的判据不是恒真的**。
    // 做法是拿一个**故意错误**的输入去撞真实API，断言它**必须被拒**——
    // 若这条也通过，说明守卫形同虚设（恒真），那么上面所有判据都不可信。
    //
    // 具体撞三道守卫：层号越界、逆序区间、未知隔离符。三者都必须拒。
    let layer_rejects = td::guard_layer(td::MAX_LAYERS + 5).is_err();
    let mut r = td::DirectionResolver::new(0);
    let range_rejects = r
        .declare(mk_decl(td::DirectionLayer::Block, (7, 1), td::Direction::Ltr))
        .is_err();
    let isolate_rejects = td::guard_isolate("QQQ").is_err();
    let ok = layer_rejects && range_rejects && isolate_rejects;
    let why = if ok { "" } else { "self-check-fail" };
    Verdict::new("C03-模型-断言能失败", ok, why)
}