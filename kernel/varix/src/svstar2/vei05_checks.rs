//! VE-F4005 · 域自检（判据逐条对应，见 `vei05_font.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//!
//! - **回退链配置** → `C05-链-每locale有序链`、`C05-链-长度钳制可见`、
//!   `C05-链-缺失走默认且诊断`、`C05-链-空链不伪装就绪`；
//! - **覆盖验证** → `C05-覆盖-缺字跳下一跳`、`C05-覆盖-跳数被计数`、
//!   `C05-覆盖-漏字必须上报`、`C05-覆盖-超界拒绝不截断`；
//! - **许可地域** → `C05-许可-地域违规阻断`、`C05-许可-未指定即阻断`、
//!   `C05-许可-无许可面不误拦`；
//! - **度量对齐** → `C05-基线-容差内对齐`、`C05-基线-超容差拒绝`、
//!   `C05-基线-混排越界拒绝`；
//! - **三单源复用** → `C05-单源-三owner正确`、`C05-单源-唯一不重复`、
//!   `C05-预留-未落地不谎报`、`C05-预留-规格编号连续`、`C05-预留-判据全覆盖`；
//!
//! 另设**反假门禁**组：`C05-反假-断言能失败`、`C05-反假-非ASCII不崩`。
//!
//! `detail` 用 `&'static str`（内核 `CheckSet::add` 只存 `&'static str`），
//! 失败细节由单元测试承担——自检只负责绿/红 + 稳定短码。

use alloc::vec::Vec;

use crate::checks::CheckSet;
use crate::svstar2::vei05_font as ft;

/// 自检集构造。
pub fn run_vei05_checks() -> CheckSet {
    let mut set = CheckSet::new(ft::VEA_DOMAIN);
    run(&mut set);
    set
}

/// 一条自检结论。
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

    // 判据一：回退链配置
    vs.push(chain_per_locale());
    vs.push(chain_length_clamped());
    vs.push(chain_missing_defaulted());
    vs.push(chain_empty_not_faked_ready());

    // 判据二：覆盖验证
    vs.push(coverage_hops_next());
    vs.push(coverage_hops_counted());
    vs.push(coverage_missing_visible());
    vs.push(coverage_oversize_rejected());

    // 判据三：许可地域
    vs.push(license_violation_blocks());
    vs.push(license_unspecified_blocks());
    vs.push(license_not_applicable_passes());

    // 判据四：度量对齐
    vs.push(baseline_aligns());
    vs.push(baseline_out_of_tolerance());
    vs.push(baseline_mix_oversize_rejected());

    // 判据五：三单源复用
    vs.push(single_source_owners());
    vs.push(single_source_unique());
    vs.push(reserved_not_lied());
    vs.push(spec_numbering_continuous());
    vs.push(criteria_covered());

    // 反假门禁
    vs.push(assertions_can_fail());
    vs.push(non_ascii_no_crash());

    for v in vs {
        set.add(v.name, v.passed, if v.passed { "" } else { v.detail });
    }
}

// ---------------------------------------------------------------------------
// 判据一：回退链配置
// ---------------------------------------------------------------------------

fn chain_per_locale() -> Verdict {
    // 每 Locale 一条**有序**链：顺序即优先级，不排序。
    let (c, _) = ft::FallbackChain::new("zh-Hans-CN", &["noto-sans-cjk", "cjk-fallback-kai"]);
    // 有序：第0 档就是首选，不能被后续档顶掉。
    if c.font_at(0) != Some("noto-sans-cjk") || c.font_at(1) != Some("cjk-fallback-kai") {
        return Verdict::new("C05-链-每locale有序链", false, "self-check-fail");
    }
    // 四族每族都有字体集（每族至少首选+回退两档）。
    let mut ok = true;
    for f in ft::FontFamily::ALL.iter() {
        let n = ft::FONT_TABLE.iter().filter(|e| e.family == *f).count();
        if n < 2 {
            ok = false;
            break;
        }
    }
    Verdict::new("C05-链-每locale有序链", ok, "self-check-fail")
}

fn chain_length_clamped() -> Verdict {
    // 超上界钳到 MAX_CHAIN_LEN 且留痕（钳制必须可见）。
    let filler: alloc::string::String = "x".to_string();
    let mut many: Vec<&str> = Vec::new();
    let mut owned: Vec<alloc::string::String> = Vec::new();
    for _ in 0..(ft::MAX_CHAIN_LEN + 8) {
        owned.push(filler.clone());
    }
    for s in owned.iter() {
        many.push(s.as_str());
    }
    let (c, rec) = ft::FallbackChain::new("ar-EG", &many);
    if c.len() != ft::MAX_CHAIN_LEN {
        return Verdict::new("C05-链-长度钳制可见", false, "self-check-fail");
    }
    let r = match rec {
        Some(x) => x,
        None => return Verdict::new("C05-链-长度钳制可见", false, "self-check-fail"),
    };
    if r.field != "chain-len" || !r.to_upper || r.effective != ft::MAX_CHAIN_LEN {
        return Verdict::new("C05-链-长度钳制可见", false, "self-check-fail");
    }
    // 在界内不留痕（不 spurious 报警）。
    let (ok_c, ok_rec) = ft::FallbackChain::new("en-US", &["varix-latin"]);
    if ok_rec.is_some() {
        return Verdict::new("C05-链-长度钳制可见", false, "self-check-fail");
    }
    Verdict::new("C05-链-长度钳制可见", ok_c.len() == 1, "self-check-fail")
}

fn chain_missing_defaulted() -> Verdict {
    // 不给链配置 → 默认链 + 诊断，不阻断（阻断会让整页打不开）。
    let p = match ft::resolve_font_chain(
        "ar-EG",
        ft::FontFamily::Arabic,
        None,
        &[0x0627],
        ft::region::GLOBAL,
        &[],
        4,
    ) {
        Ok(x) => x,
        Err(_) => return Verdict::new("C05-链-缺失走默认且诊断", false, "self-check-fail"),
    };
    if !p.chain_defaulted || p.bag.count_of(ft::DiagKind::ChainDefaulted) < 1 {
        return Verdict::new("C05-链-缺失走默认且诊断", false, "self-check-fail");
    }
    // 默认链按族给，不是千篇一律的拉丁链。
    if ft::default_chain_for(ft::FontFamily::Arabic) == ft::default_chain_for(ft::FontFamily::Cjk) {
        return Verdict::new("C05-链-缺失走默认且诊断", false, "self-check-fail");
    }
    Verdict::new("C05-链-缺失走默认且诊断", true, "")
}

fn chain_empty_not_faked_ready() -> Verdict {
    // 空链必须保持 0 档——"钳成"1 档会产出len()==1 但取不到字体的矛盾态，
    // 让配置缺失伪装成配置就绪。
    let (c, rec) = ft::FallbackChain::new("xx", &[]);
    if c.len() != 0 || !c.is_empty() || c.font_at(0).is_some() {
        return Verdict::new("C05-链-空链不伪装就绪", false, "self-check-fail");
    }
    if rec.is_none() {
        return Verdict::new("C05-链-空链不伪装就绪", false, "self-check-fail");
    }
    // 空链交给 resolve 兜底并被显性标记。
    match ft::resolve_font_chain(
        "xx",
        ft::FontFamily::Latin,
        Some(&c),
        &[0x0041],
        ft::region::GLOBAL,
        &[],
        4,
    ) {
        Ok(p) => {
            if !p.chain_defaulted {
                return Verdict::new("C05-链-空链不伪装就绪", false, "self-check-fail");
            }
        }
        Err(_) => return Verdict::new("C05-链-空链不伪装就绪", false, "self-check-fail"),
    }
    Verdict::new("C05-链-空链不伪装就绪", true, "")
}

// ---------------------------------------------------------------------------
// 判据二：覆盖验证
// ---------------------------------------------------------------------------

fn coverage_hops_next() -> Verdict {
    // 首选缺字 → 必须落到后续档，而不是停在首选或直接漏掉。
    let (chain, _) = ft::FallbackChain::new("zh", &["varix-latin", "noto-sans-cjk"]);
    let mut bag = ft::DiagBag::new();
    // 0x0041 在 latin 内；0x4E00 不在 latin 内但在 cjk 内。
    let r = match ft::scan_coverage("zh", &chain, &[0x0041, 0x4E00], &mut bag) {
        Ok(x) => x,
        Err(_) => return Verdict::new("C05-覆盖-缺字跳下一跳", false, "self-check-fail"),
    };
    if r.hits.len() != 2 {
        return Verdict::new("C05-覆盖-缺字跳下一跳", false, "self-check-fail");
    }
    if r.hits[0].hop != 0 || r.hits[1].hop != 1 {
        return Verdict::new("C05-覆盖-缺字跳下一跳", false, "self-check-fail");
    }
    Verdict::new("C05-覆盖-缺字跳下一跳", true, "")
}

fn coverage_hops_counted() -> Verdict {
    // 锚点要求「下一跳+**计数**」——跳数必须被数出来，不许只跳不计。
    let (chain, _) = ft::FallbackChain::new("zh", &["varix-latin", "noto-sans-cjk"]);
    let mut bag = ft::DiagBag::new();
    let r = match ft::scan_coverage("zh", &chain, &[0x4E00, 0x4E01, 0x4E02], &mut bag) {
        Ok(x) => x,
        Err(_) => return Verdict::new("C05-覆盖-跳数被计数", false, "self-check-fail"),
    };
    // 三个 CJK 字各回退一次 → 计数必须是 3，不是 0 也不是 1。
    if r.fallback_hops != 3 {
        return Verdict::new("C05-覆盖-跳数被计数", false, "self-check-fail");
    }
    if bag.count_of(ft::DiagKind::GlyphMissingFallback) != 3 {
        return Verdict::new("C05-覆盖-跳数被计数", false, "self-check-fail");
    }
    Verdict::new("C05-覆盖-跳数被计数", true, "")
}

fn coverage_missing_visible() -> Verdict {
    // 全链走完仍缺 → 必须进 missing 且产诊断，不许静默漏字。
    let (chain, _) = ft::FallbackChain::new("x", &["varix-latin"]);
    let mut bag = ft::DiagBag::new();
    let r = match ft::scan_coverage("x", &chain, &[0x4E00], &mut bag) {
        Ok(x) => x,
        Err(_) => return Verdict::new("C05-覆盖-漏字必须上报", false, "self-check-fail"),
    };
    if r.is_full() || r.missing.len() != 1 || r.coverage_percent() != 0 {
        return Verdict::new("C05-覆盖-漏字必须上报", false, "self-check-fail");
    }
    if bag.count_of(ft::DiagKind::GlyphMissingFallback) < 1 {
        return Verdict::new("C05-覆盖-漏字必须上报", false, "self-check-fail");
    }
    Verdict::new("C05-覆盖-漏字必须上报", true, "")
}

fn coverage_oversize_rejected() -> Verdict {
    // 字符集超上界显性拒绝，不静默截断（截断会漏字且看不出来）。
    let (chain, _) = ft::FallbackChain::new("x", &["varix-latin"]);
    let mut bag = ft::DiagBag::new();
    let big: Vec<u32> = (0..(ft::MAX_CHARSET_LEN + 1)).map(|i| i as u32).collect();
    match ft::scan_coverage("x", &chain, &big, &mut bag) {
        Ok(_) => return Verdict::new("C05-覆盖-超界拒绝不截断", false, "self-check-fail"),
        Err(e) => {
            if e.code != ft::E_CHARSET_TOO_LONG || !e.is_complete() {
                return Verdict::new("C05-覆盖-超界拒绝不截断", false, "self-check-fail");
            }
        }
    }
    // 恰好在界内必须放行（不能越界拒绝，也不能误拒）。
    let ok: Vec<u32> = (0..ft::MAX_CHARSET_LEN).map(|i| (i % 0x24F) as u32).collect();
    match ft::scan_coverage("x", &chain, &ok, &mut ft::DiagBag::new()) {
        Ok(_) => {}
        Err(_) => return Verdict::new("C05-覆盖-超界拒绝不截断", false, "self-check-fail"),
    }
    Verdict::new("C05-覆盖-超界拒绝不截断", true, "")
}

// ---------------------------------------------------------------------------
// 判据三：许可地域
// ---------------------------------------------------------------------------

fn license_violation_blocks() -> Verdict {
    // 链里出现表外字体 → 无法核验许可 → 阻断（不按"合法"放行）。
    let (chain, _) = ft::FallbackChain::new("zh", &["not-a-real-font"]);
    match ft::resolve_font_chain(
        "zh",
        ft::FontFamily::Cjk,
        Some(&chain),
        &[0x0041],
        ft::region::GLOBAL,
        &[],
        4,
    ) {
        Ok(_) => return Verdict::new("C05-许可-地域违规阻断", false, "self-check-fail"),
        Err(e) => {
            if e.code != ft::E_LICENSE_BLOCKED || !e.is_complete() {
                return Verdict::new("C05-许可-地域违规阻断", false, "self-check-fail");
            }
        }
    }
    // 只授权 EU 的字体在北美 → 违规阻断。
    let eu_only = ft::FontEntry {
        id: "eu-only",
        family: ft::FontFamily::Latin,
        source: ft::GlyphSource::ExternalFile,
        covers_upto: 0x024F,
        license_regions: 1u32 << ft::region::EU,
    };
    if !ft::check_license(&eu_only, ft::region::NA).blocks() {
        return Verdict::new("C05-许可-地域违规阻断", false, "self-check-fail");
    }
    if ft::check_license(&eu_only, ft::region::EU).blocks() {
        return Verdict::new("C05-许可-地域违规阻断", false, "self-check-fail");
    }
    Verdict::new("C05-许可-地域违规阻断", true, "")
}

fn license_unspecified_blocks() -> Verdict {
    // **未指定地域必须按违规阻断**——这是最容易漏的一条。
    // 若放行，等于把"不知道能不能用"当成"能用"，那正是许可事故的起点。
    let cjk = match ft::find_font("noto-sans-cjk") {
        Some(e) => e,
        None => return Verdict::new("C05-许可-未指定即阻断", false, "self-check-fail"),
    };
    if !ft::check_license(cjk, ft::REGION_UNSPECIFIED).blocks() {
        return Verdict::new("C05-许可-未指定即阻断", false, "self-check-fail");
    }
    if ft::is_supported_region(ft::REGION_UNSPECIFIED) {
        return Verdict::new("C05-许可-未指定即阻断", false, "self-check-fail");
    }
    // 全球许可字体也不能因"未指定"而放行——未指定 ≠ 全球。
    if !ft::check_license(cjk, ft::REGION_UNSPECIFIED).blocks() {
        return Verdict::new("C05-许可-未指定即阻断", false, "self-check-fail");
    }
    Verdict::new("C05-许可-未指定即阻断", true, "")
}

fn license_not_applicable_passes() -> Verdict {
    // 内建/系统字体不带许可面→ NotApplicable，不阻断（否则把所有内建字体
    // 都拦了，等于功能不可用）。
    for id in ["varix-latin", "dejavu-sans"] {
        let e = match ft::find_font(id) {
            Some(x) => x,
            None => return Verdict::new("C05-许可-无许可面不误拦", false, "self-check-fail"),
        };
        if ft::check_license(e, ft::region::GLOBAL) != ft::LicenseVerdict::NotApplicable {
            return Verdict::new("C05-许可-无许可面不误拦", false, "self-check-fail");
        }
    }
    // 且必须真的没有许可位（否则说明表填错了）。
    let latin = ft::find_font("varix-latin").unwrap();
    if latin.source.needs_license() {
        return Verdict::new("C05-许可-无许可面不误拦", false, "self-check-fail");
    }
    Verdict::new("C05-许可-无许可面不误拦", true, "")
}

// ---------------------------------------------------------------------------
// 判据四：度量对齐
// ---------------------------------------------------------------------------

fn baseline_aligns() -> Verdict {
    // 容差内失配必须被对齐（锚点：基线失配→对齐修正）。
    let runs = [
        ft::RunMetrics { hop: 0, ascent: 20, descent: 5 },
        ft::RunMetrics { hop: 1, ascent: 16, descent: 4 },
    ];
    let mut bag = ft::DiagBag::new();
    let r = match ft::align_baseline(&runs, 8, &mut bag) {
        Ok(x) => x,
        Err(_) => return Verdict::new("C05-基线-容差内对齐", false, "self-check-fail"),
    };
    if r.common_ascent != 20 || r.offsets != vec![0, 4] || r.misaligned != 1 {
        return Verdict::new("C05-基线-容差内对齐", false, "self-check-fail");
    }
    if bag.count_of(ft::DiagKind::BaselineAligned) < 1 {
        return Verdict::new("C05-基线-容差内对齐", false, "self-check-fail");
    }
    // 全部一致 → 无失配、无偏移。
    let same = [
        ft::RunMetrics { hop: 0, ascent: 20, descent: 5 },
        ft::RunMetrics { hop: 1, ascent: 20, descent: 5 },
    ];
    match ft::align_baseline(&same, 8, &mut ft::DiagBag::new()) {
        Ok(x) => {
            if x.misaligned != 0 {
                return Verdict::new("C05-基线-容差内对齐", false, "self-check-fail");
            }
        }
        Err(_) => return Verdict::new("C05-基线-容差内对齐", false, "self-check-fail"),
    }
    Verdict::new("C05-基线-容差内对齐", true, "")
}

fn baseline_out_of_tolerance() -> Verdict {
    // 超容差必须显性拒绝，不许硬拉齐（硬拉齐会让"字体选错"永远看不见）。
    let runs = [
        ft::RunMetrics { hop: 0, ascent: 30, descent: 5 },
        ft::RunMetrics { hop: 1, ascent: 10, descent: 4 },
    ];
    match ft::align_baseline(&runs, 4, &mut ft::DiagBag::new()) {
        Ok(_) => return Verdict::new("C05-基线-超容差拒绝", false, "self-check-fail"),
        Err(e) => {
            if e.code != ft::E_BASELINE_OUT_OF_TOLERANCE || !e.is_complete() {
                return Verdict::new("C05-基线-超容差拒绝", false, "self-check-fail");
            }
        }
    }
    // 容差超上界被钳到上界，且不因此误拒。
    let one = [ft::RunMetrics { hop: 0, ascent: 30, descent: 5 }];
    match ft::align_baseline(&one, 99999, &mut ft::DiagBag::new()) {
        Ok(_) => {}
        Err(_) => return Verdict::new("C05-基线-超容差拒绝", false, "self-check-fail"),
    }
    Verdict::new("C05-基线-超容差拒绝", true, "")
}

fn baseline_mix_oversize_rejected() -> Verdict {
    // 混排条目超上界显性拒绝（O(混排数) 会拖垮单帧）。
    let filler = ft::RunMetrics { hop: 0, ascent: 10, descent: 2 };
    let big: Vec<ft::RunMetrics> = (0..(ft::MAX_MIX_RUNS + 1)).map(|_| filler).collect();
    match ft::align_baseline(&big, 8, &mut ft::DiagBag::new()) {
        Ok(_) => return Verdict::new("C05-基线-混排越界拒绝", false, "self-check-fail"),
        Err(e) => {
            if e.code != ft::E_MIX_TOO_LONG || !e.is_complete() {
                return Verdict::new("C05-基线-混排越界拒绝", false, "self-check-fail");
            }
        }
    }
    // 空混排是合法的（不是错误），且不产基线诊断。
    let mut bag = ft::DiagBag::new();
    match ft::align_baseline(&[], 8, &mut bag) {
        Ok(x) => {
            if x.misaligned != 0 || x.common_ascent != 0 {
                return Verdict::new("C05-基线-混排越界拒绝", false, "self-check-fail");
            }
        }
        Err(_) => return Verdict::new("C05-基线-混排越界拒绝", false, "self-check-fail"),
    }
    Verdict::new("C05-基线-混排越界拒绝", true, "")
}

// ---------------------------------------------------------------------------
// 判据五：三单源复用
// ---------------------------------------------------------------------------

fn single_source_owners() -> Verdict {
    // 三条复用声明的 owner 必须分别是 F2908/F2909/F2919（本项只复用不代做）。
    let want = [
        ("glyph-cmap-scan", "VE-F2908"),
        ("font-metrics", "VE-F2909"),
        ("license-region-audit", "VE-F2919"),
    ];
    for (key, owner) in want.iter() {
        let c = match ft::FONT_SINGLE_SOURCE.iter().find(|c| c.key == *key) {
            Some(x) => x,
            None => return Verdict::new("C05-单源-三owner正确", false, "self-check-fail"),
        };
        if c.owner != *owner {
            return Verdict::new("C05-单源-三owner正确", false, "self-check-fail");
        }
        // 每条复用声明至少有一个 consumer（本项自己就该是其中之一）。
        if c.consumers.is_empty() {
            return Verdict::new("C05-单源-三owner正确", false, "self-check-fail");
        }
    }
    // 自有能力的 owner 必须是本项。
    match ft::FONT_SINGLE_SOURCE.iter().find(|c| c.key == "font-fallback-chain") {
        Some(x) => {
            if x.owner != "VE-F4005" {
                return Verdict::new("C05-单源-三owner正确", false, "self-check-fail");
            }
        }
        None => return Verdict::new("C05-单源-三owner正确", false, "self-check-fail"),
    }
    Verdict::new("C05-单源-三owner正确", true, "")
}

fn single_source_unique() -> Verdict {
    // 单源唯一性机检必须绿（同能力两个 owner 必然分叉）。
    match ft::check_single_source() {
        Ok(()) => {}
        Err(e) => {
            if e.code != ft::E_SINGLE_SOURCE_DUP || !e.is_complete() {
                return Verdict::new("C05-单源-唯一不重复", false, "self-check-fail");
            }
            return Verdict::new("C05-单源-唯一不重复", false, "self-check-fail");
        }
    }
    // 反向验证：声明里 key 不得重复（否则机检形同虚设）。
    let mut keys: Vec<&str> = ft::FONT_SINGLE_SOURCE.iter().map(|c| c.key).collect();
    let before = keys.len();
    keys.sort_unstable();
    keys.dedup();
    if keys.len() != before {
        return Verdict::new("C05-单源-唯一不重复", false, "self-check-fail");
    }
    Verdict::new("C05-单源-唯一不重复", true, "")
}

fn reserved_not_lied() -> Verdict {
    // 预留槽位机检必须绿；且四条槽位当前全部 landed=false。
    match ft::check_reserved() {
        Ok(()) => {}
        Err(_) => return Verdict::new("C05-预留-未落地不谎报", false, "self-check-fail"),
    }
    if ft::RESERVED_SLOTS.iter().any(|s| s.landed) {
        return Verdict::new("C05-预留-未落地不谎报", false, "self-check-fail");
    }
    // 三条 F29xx 复用槽位必须齐全（锚点钦定三单源）。
    for up in ["VE-F2908", "VE-F2909", "VE-F2919"] {
        if !ft::RESERVED_SLOTS.iter().any(|s| s.upstream == up) {
            return Verdict::new("C05-预留-未落地不谎报", false, "self-check-fail");
        }
    }
    Verdict::new("C05-预留-未落地不谎报", true, "")
}

fn spec_numbering_continuous() -> Verdict {
    // 规格表编号必须从1 起连续（对拍按号定位）。
    for (i, item) in ft::SPEC_SHEET.iter().enumerate() {
        if item.no != (i + 1) as u16 {
            return Verdict::new("C05-预留-规格编号连续", false, "self-check-fail");
        }
        // 每条规格都要有中文标签与强制它的自检项名。
        if item.label.is_empty() || item.enforced_by.is_empty() {
            return Verdict::new("C05-预留-规格编号连续", false, "self-check-fail");
        }
    }
    Verdict::new("C05-预留-规格编号连续", true, "")
}

fn criteria_covered() -> Verdict {
    // 判据与规格表双向对齐（判据不得引用不存在的规格键）。
    match ft::check_spec_coverage() {
        Ok(()) => {}
        Err(_) => return Verdict::new("C05-预留-判据全覆盖", false, "self-check-fail"),
    }
    // 锚点五条判据必须逐条在 CRITERIA 里。
    for crit in ["回退链配置", "覆盖验证", "许可地域", "度量对齐", "三单源复用"] {
        if !ft::CRITERIA.iter().any(|(c, _)| *c == crit) {
            return Verdict::new("C05-预留-判据全覆盖", false, "self-check-fail");
        }
    }
    // 零隐私面机检必须绿。
    if ft::check_zero_privacy().is_err() {
        return Verdict::new("C05-预留-判据全覆盖", false, "self-check-fail");
    }
    Verdict::new("C05-预留-判据全覆盖", true, "")
}

// ---------------------------------------------------------------------------
// 反假门禁
// ---------------------------------------------------------------------------

fn assertions_can_fail() -> Verdict {
    // **反假门禁**：用故意错误的输入撞守卫，证明守卫非恒真。
    // 若守卫恒真，把 SPEC_SHEET 清空也能全绿——那就等于没有门禁。
    // 1) 族守卫必须能拒域外值。
    if ft::guard_family("Klingon").is_ok() {
        return Verdict::new("C05-反假-断言能失败", false, "self-check-fail");
    }
    // 2) 规格表机检必须能抓编号断裂（临时造一个断裂表验证判据逻辑）。
    // 这里不能改const，故改为验证 check_* 的错误码确实存在且可达——
    // 用一个真实的越界输入触发拒绝路径。
    let (chain, _) = ft::FallbackChain::new("x", &["totally-unknown-font"]);
    match ft::resolve_font_chain(
        "x",
        ft::FontFamily::Latin,
        Some(&chain),
        &[0x0041],
        ft::region::GLOBAL,
        &[],
        4,
    ) {
        Ok(_) => return Verdict::new("C05-反假-断言能失败", false, "self-check-fail"),
        Err(e) => {
            if e.code != ft::E_LICENSE_BLOCKED {
                return Verdict::new("C05-反假-断言能失败", false, "self-check-fail");
            }
        }
    }
    // 3) 超容差基线必须能拒（证明基线门禁不是恒过的橡皮章）。
    let bad = [
        ft::RunMetrics { hop: 0, ascent: 200, descent: 5 },
        ft::RunMetrics { hop: 1, ascent: 1, descent: 1 },
    ];
    if ft::align_baseline(&bad, 1, &mut ft::DiagBag::new()).is_ok() {
        return Verdict::new("C05-反假-断言能失败", false, "self-check-fail");
    }
    Verdict::new("C05-反假-断言能失败", true, "")
}

fn non_ascii_no_crash() -> Verdict {
    // 非 ASCII 与畸形输入不得 panic——内核铁律：异常零静默不靠 panic 实现。
    for tag in ["汉", "*", "-CN", "x-private", ""] {
        let _ = ft::guard_family(tag);
        let (c, _) = ft::FallbackChain::new(tag, &["varix-latin"]);
        let mut bag = ft::DiagBag::new();
        // 含超出 BMP 的码位（代理对区与私用区）。
        let r = ft::scan_coverage(tag, &c, &[0x10FFFF, 0xE000, 0xD800, 0x0041], &mut bag);
        if r.is_err() {
            // 只允许"超上界"这一种错；这里的字符集很小，不该被拒。
            return Verdict::new("C05-反假-非ASCII不崩", false, "self-check-fail");
        }
    }
    // 高位码位必然缺字（所有字体 covers_upto 都远小于 0x10FFFF），
    // 但必须**报告缺字**而不是崩。
    let (c, _) = ft::FallbackChain::new("x", &["varix-latin"]);
    let mut bag = ft::DiagBag::new();
    match ft::scan_coverage("x", &c, &[0x10FFFF], &mut bag) {
        Ok(r) => {
            if r.missing.len() != 1 {
                return Verdict::new("C05-反假-非ASCII不崩", false, "self-check-fail");
            }
        }
        Err(_) => return Verdict::new("C05-反假-非ASCII不崩", false, "self-check-fail"),
    }
    Verdict::new("C05-反假-非ASCII不崩", true, "")
}
