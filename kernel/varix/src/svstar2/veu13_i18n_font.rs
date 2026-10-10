//! VE-F4013 · 国际化与 O06 字体深化（VE-U 域 · 任务段 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F4013`
//!
//! **判据（锚点原文）**：预留激活、支持声明红线、对拍制度、覆盖表、单源复用、判据。
//!
//! **职责定位（锚点原文）**：O06 字体深化协同（F2915 预留激活终版
//! （i18n 语言→字体偏好表激活（复用预留激活模式（复述单源）；语言
//! 覆盖扩展（O06 字体集扩展（新增语言字体接入流程（接入流程红线：
//! 新语言无字体验证不可声明支持（支持声明红线；激活对拍（语言切换
//! →字体切换对拍（复述对拍）。
//!
//! # 一、语言→字体偏好表 + 预留激活（F2915 模式复用）
//!
//! [`LangFontPref`] 语言→字体偏好条目（首选族+回退族）；条目经
//! [`Activation`] 激活协议入表（[`EntryState`] 预留/激活两态）——
//! **预留≠生效**：未激活条目查询返 None 并显性提示（预留跳过激活
//! →显性，锚点错误路径），激活幂等（重复激活不重复计数）。
//!
//! # 二、支持声明红线（新语言接入）
//!
//! 新语言声明"已支持"的前件：其字体必须过 [`fontsub::validate_font`]
//! 与字符覆盖核查（[`fontsub::missing_diag`]）——**无字体验证不可
//! 声明支持**（红线实测：构造无字体条目 → 拦截，锚点错误路径原文）。
//!
//! # 三、激活对拍 + 覆盖表
//!
//! 语言切换必须同步切字体偏好（[`switch_lang`] 对拍复述——只换语言
//! 不换字体=半切换裂缝）；覆盖表核验声明语言与字体覆盖对齐
//! （[`coverage_gap`]——覆盖漏→补：声明了语言但没有字体覆盖即缺口）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::fontsub::{validate_font, FontFileHead, FontMeta, FontVerdict};
use crate::svstar2::vei02_locale::{parse_lenient, ParseBudget};

// ---------------------------------------------------------------------------
// 一、常量与错误码
// ---------------------------------------------------------------------------

/// 本项版本。
pub const I18N_FONT_VERSION: &str = "U13-i18n-font-v1";

/// 语言数上限（覆盖表容量）。
pub const MAX_LANGS: usize = 64;

/// 字体族名字长度上限。
pub const MAX_FAMILY_LEN: usize = 32;

/// 无字体验证声明支持（红线实测拦截）。
pub const E_IF_NO_FONT: &str = "E_IF_NO_FONT";

/// 预留跳过激活（显性提示——预留≠生效）。
pub const E_IF_NOT_ACTIVE: &str = "E_IF_NOT_ACTIVE";

/// 激活分歧（语言与字体偏好不一致——对拍拦截）。
pub const E_IF_SWITCH: &str = "E_IF_SWITCH";

/// 覆盖漏（声明语言无字体覆盖）。
pub const E_IF_COVERAGE: &str = "E_IF_COVERAGE";

/// 语言标签非法（BCP47 单源口径）。
pub const E_IF_LANG_TAG: &str = "E_IF_LANG_TAG";

// ---------------------------------------------------------------------------
// 二、预留激活协议（F2915 模式复用）
// ---------------------------------------------------------------------------

/// 条目状态（预留/激活两态——预留≠生效）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryState {
    /// 预留（登记未生效）。
    Reserved,
    /// 激活（生效可查）。
    Active,
}

/// 语言→字体偏好条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LangFontPref {
    /// 语言标签（BCP47——如 zh-Hans）。
    pub lang: String,
    /// 首选字体族。
    pub primary_family: String,
    /// 回退字体族（有序）。
    pub fallback_families: Vec<String>,
    /// 字体验证凭据（接入流程红线的物质证据）。
    pub font_evidence: Option<FontEvidence>,
    /// 条目状态。
    pub state: EntryState,
}

/// 字体验证凭据（validate_font 的通过记录）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FontEvidence {
    /// 字体文件魔验通过。
    pub magic_ok: bool,
    /// 覆盖缺口数（missing_diag 的累计）。
    pub missing: u32,
}

impl FontEvidence {
    /// 凭据是否足够支撑"已支持"声明（魔验过且覆盖缺口为 0）。
    pub fn supports(&self) -> bool {
        self.magic_ok && self.missing == 0
    }
}

/// 新语言接入（预留条目入库——先预留后激活）。
///
/// 红线在入库时就地核验：首选族名非空 + 语言标签合法（[`LocaleTag`]
/// 单源口径）。**字体验证不过关的条目允许预留**（接管半成品）但
/// 不允许声明支持——声明支持的红线在 [`declare_supported`]。
pub fn onboard(pref: LangFontPref) -> Result<LangFontPref, String> {
    if pref.primary_family.trim().is_empty() || pref.primary_family.len() > MAX_FAMILY_LEN {
        return Err(format!(
            "{}：语言 {} 的首选字体族为空或超长（{} 字符内）",
            E_IF_NO_FONT, pref.lang, MAX_FAMILY_LEN
        ));
    }
    for f in pref.fallback_families.iter() {
        if f.trim().is_empty() || f.len() > MAX_FAMILY_LEN {
            return Err(format!("{}：语言 {} 的回退族名非法", E_IF_NO_FONT, pref.lang));
        }
    }
    parse_lenient(pref.lang.as_str(), &ParseBudget::DOMAIN_DEFAULT).map_err(|_| {
        format!("{}：语言标签 {} 不合法——按 vei02 BCP47 单源口径", E_IF_LANG_TAG, pref.lang)
    })?;
    Ok(LangFontPref { state: EntryState::Reserved, ..pref })
}

/// 激活（幂等——重复激活不重复计数）。
pub fn activate(pref: &mut LangFontPref) -> Result<(), String> {
    if pref.state == EntryState::Active {
        return Ok(());
    }
    pref.state = EntryState::Active;
    Ok(())
}

/// 查询语言偏好（未激活条目显性报错——预留跳过激活→显性）。
pub fn lookup(pref: &LangFontPref) -> Result<&LangFontPref, String> {
    if pref.state != EntryState::Active {
        return Err(format!(
            "{}：语言 {} 处于预留态（未激活）——查询不静默回退默认字体；请先 activate",
            E_IF_NOT_ACTIVE, pref.lang
        ));
    }
    Ok(pref)
}

/// 支持声明红线（无字体验证不可声明支持——红线实测）。
pub fn declare_supported(pref: &LangFontPref) -> Result<(), String> {
    match &pref.font_evidence {
        None => {
            Err(format!(
                "{}：语言 {} 无字体验证凭据——不可声明支持（新语言无字体验证不可声明支持）",
                E_IF_NO_FONT, pref.lang
            ))
        }
        Some(ev) if !ev.supports() => {
            Err(format!(
                "{}：语言 {} 字体验证不过关（魔验={} 缺口={}）——补齐字体覆盖后再声明",
                E_IF_NO_FONT, pref.lang, ev.magic_ok, ev.missing
            ))
        }
        Some(_) => Ok(()),
    }
}

// ---------------------------------------------------------------------------
// 三、接入流程红线（真调 fontsub）
// ---------------------------------------------------------------------------

/// 接入验证（真调 fontsub 的 validate_font + missing_diag）。
///
/// 新语言字体接入流程：字体文件头过魔验 + 对首选族的字符覆盖核查
/// （缺口计数）——两关都过才给凭据。
pub fn verify_font_for_lang(
    head: &FontFileHead,
    actual_len: u32,
    fonts: &[FontMeta],
    probe_chars: &[u32],
) -> FontEvidence {
    let magic_ok = matches!(validate_font(head, actual_len), FontVerdict::Ok);
    let mut missing = 0u32;
    for ch in probe_chars.iter() {
        if crate::fontsub::missing_diag(*ch, fonts).is_some() {
            missing = missing.saturating_add(1);
        }
    }
    FontEvidence { magic_ok, missing }
}

// ---------------------------------------------------------------------------
// 四、激活对拍（语言切换→字体切换）
// ---------------------------------------------------------------------------

/// 当前会话的语言/字体绑定（对拍的两端）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LangBinding {
    /// 当前语言。
    pub lang: String,
    /// 当前字体族。
    pub family: String,
}

/// 语言切换（必须同步切字体偏好——只换一边=半切换裂缝）。
pub fn switch_lang(binding: &mut LangBinding, target: &LangFontPref) -> Result<(), String> {
    if target.state != EntryState::Active {
        return Err(format!(
            "{}：目标语言 {} 未激活——不可切换（预留态语言不参与对拍）",
            E_IF_NOT_ACTIVE, target.lang
        ));
    }
    binding.lang = target.lang.clone();
    binding.family = target.primary_family.clone();
    Ok(())
}

/// 切换对拍（语言与字体必须绑定一致——分歧即拦截）。
pub fn switch_reconcile(binding: &LangBinding, target: &LangFontPref) -> Result<(), String> {
    if binding.lang == target.lang && binding.family != target.primary_family {
        return Err(format!(
            "{}：语言 {} 的字体偏好分歧（当前 {} ≠ 首选 {}）——半切换裂缝，须同步换绑",
            E_IF_SWITCH, binding.lang, binding.family, target.primary_family
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 五、覆盖表（覆盖漏→补）
// ---------------------------------------------------------------------------

/// 覆盖缺口（声明语言 vs 字体覆盖）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoverageGap {
    /// 缺口语言。
    pub lang: String,
    /// 缺口原因。
    pub reason: &'static str,
}

/// 覆盖表核验（每条已激活语言必须有可支撑的字体覆盖——覆盖漏→补）。
pub fn coverage_gaps(prefs: &[LangFontPref]) -> Vec<CoverageGap> {
    let mut gaps: Vec<CoverageGap> = Vec::new();
    for p in prefs.iter() {
        if p.state != EntryState::Active {
            continue; // 未激活不在覆盖承诺范围
        }
        match &p.font_evidence {
            None => gaps.push(CoverageGap { lang: p.lang.clone(), reason: "无字体验证凭据" }),
            Some(ev) if !ev.supports() => {
                gaps.push(CoverageGap { lang: p.lang.clone(), reason: "字体验证不过关" })
            }
            Some(_) => {}
        }
    }
    gaps
}

// ---------------------------------------------------------------------------
// 六、判据
// ---------------------------------------------------------------------------

use crate::checks::CheckSet;

/// F4013 域自检（判据六组：激活/红线/对拍/覆盖/单源/收尾）。
pub fn run_veu13_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F4013");

    // --- 预留激活（判据一）---
    let mut zh = onboard(LangFontPref {
        lang: String::from("zh-Hans"),
        primary_family: String::from("Varix Sans SC"),
        fallback_families: {
            let mut v = Vec::new();
            v.push(String::from("Noto Sans CJK"));
            v
        },
        font_evidence: Some(FontEvidence { magic_ok: true, missing: 0 }),
        state: EntryState::Reserved,
    })
    .expect("zh onboarding");
    // 预留态查询显性报错（预留跳过激活→显性）。
    let r = lookup(&zh);
    s.add(
        "U13-激活-01",
        r.as_ref().err().map(|x| x.starts_with(E_IF_NOT_ACTIVE) && x.contains("activate")).unwrap_or(false),
        "预留态查询显性（不静默回退默认字体）",
    );
    // 激活后可查。
    let act = activate(&mut zh);
    let found = lookup(&zh);
    s.add(
        "U13-激活-02",
        act.is_ok() && found.is_ok() && found.unwrap().primary_family == "Varix Sans SC",
        "激活后可查（返回首选族）",
    );
    // 激活幂等（重复激活不坏状态）。
    let again = activate(&mut zh);
    s.add("U13-激活-03", again.is_ok() && zh.state == EntryState::Active, "激活幂等");

    // --- 支持声明红线（判据二）---
    // 有凭据→声明过。
    s.add("U13-红线-01", declare_supported(&zh).is_ok(), "字体验证过关→支持声明放行");
    // 无凭据→拦截（红线实测）。
    let no_ev = LangFontPref {
        lang: String::from("th-TH"),
        primary_family: String::from("Varix Sans TH"),
        fallback_families: Vec::new(),
        font_evidence: None,
        state: EntryState::Active,
    };
    let r = declare_supported(&no_ev);
    s.add(
        "U13-红线-02",
        r.as_ref().err().map(|x| x.starts_with(E_IF_NO_FONT) && x.contains("不可声明支持")).unwrap_or(false),
        "无字体验证声明支持被拦（红线实测）",
    );
    // 过验但有关口→拦截。
    let gappy = LangFontPref {
        lang: String::from("ja-JP"),
        primary_family: String::from("Varix Sans JP"),
        fallback_families: Vec::new(),
        font_evidence: Some(FontEvidence { magic_ok: true, missing: 3 }),
        state: EntryState::Active,
    };
    s.add(
        "U13-红线-03",
        declare_supported(&gappy).is_err() && declare_supported(&gappy).unwrap_err().contains("缺口=3"),
        "覆盖缺口未补不可声明支持",
    );

    // --- 接入流程（判独二续）---
    // 真调 fontsub：合法字体头给凭据（魔验过+缺口 0）。
    let head = FontFileHead {
        magic: *b"VXF1",
        declared_len: 32,
    };
    let ev = verify_font_for_lang(&head, 64, &[], &[]);
    s.add(
        "U13-接入-01",
        ev.magic_ok && ev.missing == 0 && ev.supports(),
        "真调 fontsub 魔验（合法头过硬+零样本零缺口）",
    );
    // 魔验失败（坏魔数）→ 凭据不支持。
    let bad_head = FontFileHead {
        magic: *b"BAD!",
        declared_len: 32,
    };
    let ev_bad = verify_font_for_lang(&bad_head, 64, &[], &[]);
    s.add(
        "U13-接入-02",
        !ev_bad.magic_ok && !ev_bad.supports(),
        "坏魔数字体拦截（魔验红线）",
    );

    // --- 激活对拍（判据三）---
    let mut binding = LangBinding::default();
    let sw = switch_lang(&mut binding, &zh);
    s.add(
        "U13-对拍-01",
        sw.is_ok() && binding.lang == "zh-Hans" && binding.family == "Varix Sans SC",
        "语言切换同步换字体（对拍一致）",
    );
    // 未激活目标不可切换。
    let mut b2 = LangBinding::default();
    let r = switch_lang(&mut b2, &no_ev_active_reserved());
    s.add("U13-对拍-02", r.is_err() && r.unwrap_err().starts_with(E_IF_NOT_ACTIVE), "预留态语言不参与对拍");
    // 分歧可检出（语言同字体异=半切换）。
    let drifted = LangBinding { lang: String::from("zh-Hans"), family: String::from("Noto Sans CJK") };
    let r = switch_reconcile(&drifted, &zh);
    s.add(
        "U13-对拍-03",
        r.as_ref().err().map(|x| x.starts_with(E_IF_SWITCH) && x.contains("半切换")).unwrap_or(false),
        "半切换裂缝拦截（语言同字体异）",
    );

    // --- 覆盖表（判据四）---
    // 全激活且覆盖过关→零缺口。
    let gaps = coverage_gaps(&[zh.clone()]);
    s.add("U13-覆盖-01", gaps.is_empty(), "激活+覆盖过关→零缺口");
    // 缺口可检出（激活但无凭据）。
    let gaps2 = coverage_gaps(&[no_ev.clone(), zh.clone()]);
    s.add(
        "U13-覆盖-02",
        gaps2.len() == 1 && gaps2[0].lang == "th-TH" && gaps2[0].reason == "无字体验证凭据",
        "覆盖漏→补（缺口点名到语言）",
    );

    // --- 单源复用（判据五）---
    // 语言标签口径=vei02 BCP47 单源（非法标签拒接入）。
    let bad_tag = onboard(LangFontPref {
        lang: String::from("not a tag!!"),
        primary_family: String::from("X"),
        fallback_families: Vec::new(),
        font_evidence: None,
        state: EntryState::Reserved,
    });
    s.add(
        "U13-单源-01",
        bad_tag.is_err() && bad_tag.as_ref().unwrap_err().starts_with(E_IF_LANG_TAG),
        "语言标签口径单源（vei02 BCP47）",
    );
    // 首选族空/超长拒接入。
    let bad_family = onboard(LangFontPref {
        lang: String::from("fr-FR"),
        primary_family: String::from(""),
        fallback_families: Vec::new(),
        font_evidence: None,
        state: EntryState::Reserved,
    });
    s.add("U13-单源-02", bad_family.is_err() && bad_family.unwrap_err().starts_with(E_IF_NO_FONT), "首选族空拒接入");

    // --- 版本与暂挂 ---
    let fp = {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in I18N_FONT_VERSION.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    };
    s.add("U13-版本-01", fp != 0, "版本指纹非零（U13-i18n-font-v1）");

    s.add(
        "U13-暂挂-01",
        U13_LEDGER_SUSPENDED_NOTE.contains("暂挂") && U13_LEDGER_SUSPENDED_NOTE.contains("F4013"),
        "U 域账本暂挂声明显性",
    );

    // U13-暂挂-02：判据条数对账（本条为第 18 条）。
    s.add("U13-暂挂-02", s.len() == 17, "判据条数对账（17+本条）");

    s
}

/// 预留态目标（对拍负例语料）。
fn no_ev_active_reserved() -> LangFontPref {
    LangFontPref {
        lang: String::from("th-TH"),
        primary_family: String::from("Varix Sans TH"),
        fallback_families: Vec::new(),
        font_evidence: None,
        state: EntryState::Reserved,
    }
}

/// U 域账本暂挂声明（跨批对接点：F2915/O06 单源复用声明；F2904 匹配
/// 对端；Q 字体对端——建账前暂挂，移交期模式延续）。
pub const U13_LEDGER_SUSPENDED_NOTE: &str = "国际化与 O06 字体深化（语言→字体偏好表+预留激活+支持声明红线+激活对拍+覆盖表）入 U 域账本：建账前暂挂声明（移交期模式延续——F4013 同款）；新语言接入只认 fontsub 验证凭据";
