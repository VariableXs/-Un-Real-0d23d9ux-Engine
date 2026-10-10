//! VE-F4014 · 国际化与翻译流程（VE-U 域 · 任务段 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F4014`
//!
//! **判据（锚点原文）**：硬编码红线、上下文断言、回退显性、翻译流程、伪检单源、判据。
//!
//! **职责定位（锚点原文）**：翻译流程（翻译工作流对接（i18n 字符串
//! 外部化→翻译管理→回注——字符串外部化红线：硬编码字符串禁令（硬
//! 编码扫描（复用 F3811 伪检（复述单源）；翻译完整性（缺失翻译回退
//! 链（复用回退（复述）；上下文提供（翻译上下文注释（translators
//! notes（上下文红线：无上下文翻译=错译（上下文断言）。
//!
//! # 一、硬编码扫描（F3811 伪检复用）
//!
//! [`scan_hardcoded`] 按模式族扫外部化源（CJK 字面量出现在代码位/
//! 格式串拆断/同一串多处重复定义）——伪检纪律：**每条 Findings 带
//! 证据**（位置+命中模式），没有"感觉像硬编码"这种结论。F3811 的
//! 复用是**模式族与证据格式**的复述（单源声明在 [`peers`]），不是
//! 把 F3811 的代码拉过来。
//!
//! # 二、回退链与显性回退
//!
//! [`fallback_chain`] 语言回退链（zh-Hans→zh→en，与 vei02
//! [`LocaleTag`] 同口径）；翻译缺失不静默用源语言糊——
//! [`resolve`] 走链取首个有译语言并**登记回退事件**（缺翻译静默→
//! 回退显性，锚点错误路径）。
//!
//! # 三、上下文断言 + 回注校验
//!
//! 无上下文（translators notes 为空）阻断提交（红线实测）；
//! 回注译文必须保留占位符集合（回注格式错→校验——占位符丢了
//! 就是格式串炸裂的起点）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::svstar2::vei02_locale::{parse_lenient, ParseBudget};

// ---------------------------------------------------------------------------
// 一、常量与错误码
// ---------------------------------------------------------------------------

/// 本项版本。
pub const TRANSLATION_VERSION: &str = "U14-translation-v1";

/// 硬编码检出（P1——红线实测）。
pub const E_TR_HARDCODE: &str = "E_TR_HARDCODE";

/// 无上下文阻断提交（红线实测）。
pub const E_TR_NO_CONTEXT: &str = "E_TR_NO_CONTEXT";

/// 回退静默（缺翻译未记事件）。
pub const E_TR_SILENT_FALLBACK: &str = "E_TR_SILENT_FALLBACK";

/// 回注格式错（占位符集合破坏）。
pub const E_TR_INJECT: &str = "E_TR_INJECT";

/// 回退链最大深度（防自引用环把查找变死循环）。
pub const MAX_FALLBACK_DEPTH: usize = 4;

/// 占位符最大数（回注校验的容量界）。
pub const MAX_PLACEHOLDERS: usize = 16;

// ---------------------------------------------------------------------------
// 二、硬编码扫描（F3811 伪检复用）
// ---------------------------------------------------------------------------

/// 硬编码命中模式（伪检模式族——证据分类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HardcodePattern {
    /// CJK 字面量出现在代码位（应为 key 引用）。
    CjkLiteralInCode,
    /// 格式串拆断（"…{0}…" 被切成两段各含一半占位符语境）。
    SplitFormat,
    /// 同一字符串多处重复定义（应抽为单 key）。
    DuplicateLiteral,
}

impl HardcodePattern {
    /// 模式名（wire 名）。
    pub fn wire(self) -> &'static str {
        match self {
            HardcodePattern::CjkLiteralInCode => "cjk-literal-in-code",
            HardcodePattern::SplitFormat => "split-format",
            HardcodePattern::DuplicateLiteral => "duplicate-literal",
        }
    }
}

/// 扫描发现（带证据——位置+模式+原文摘录）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    /// 位置（源内序号）。
    pub at: usize,
    /// 命中模式。
    pub pattern: HardcodePattern,
    /// 证据摘录（原文截断）。
    pub excerpt: String,
}

/// 外部化源单元（i18n 源文件的一条——key 或明文）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceUnit {
    /// 源内位置。
    pub at: usize,
    /// 原文（去掉引号的字面内容）。
    pub literal: String,
    /// 是否已外部化（key 引用=已外部化）。
    pub externalized: bool,
}

/// 伪检单源声明的对端行（F3811 模式族复用的对账凭据）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PeerDecl {
    /// 对端任务号。
    pub peer: &'static str,
    /// 复述内容（复用了什么）。
    pub restated: &'static str,
}

/// 对端声明表（F3811 伪检模式族/vei02 标签口径）。
pub fn peers() -> [PeerDecl; 2] {
    [
        PeerDecl { peer: "VE-F3811", restated: "硬编码扫描复用伪检模式族（证据式发现，无感觉式结论）" },
        PeerDecl { peer: "VE-F2904", restated: "语言标签口径复用 vei02 BCP47（parse_lenient+DOMAIN_DEFAULT）" },
    ]
}

/// CJK 粗判（CJK 统一表意文字+日文假名两区命中——不引 full unicode
/// 表，够扫硬编码用；误判方向保守：漏收比误收好，扫描还有人工复核）。
fn has_cjk(s: &str) -> bool {
    s.chars().any(|c| {
        let u = c as u32;
        (0x4E00..=0x9FFF).contains(&u) || (0x3040..=0x30FF).contains(&u)
    })
}

/// 硬编码扫描（伪检复用——每条发现带证据）。
pub fn scan_hardcoded(units: &[SourceUnit]) -> Vec<Finding> {
    let mut out: Vec<Finding> = Vec::new();
    // 1) 未外部化且含 CJK → 代码位字面量。
    for u in units.iter() {
        if !u.externalized && has_cjk(&u.literal) {
            out.push(Finding {
                at: u.at,
                pattern: HardcodePattern::CjkLiteralInCode,
                excerpt: clip(&u.literal, 24),
            });
        }
    }
    // 2) 格式串拆断（含 { 或 } 但不配对）。
    for u in units.iter() {
        let opens = u.literal.matches('{').count();
        let closes = u.literal.matches('}').count();
        if opens != closes && (opens > 0 || closes > 0) {
            out.push(Finding {
                at: u.at,
                pattern: HardcodePattern::SplitFormat,
                excerpt: clip(&u.literal, 24),
            });
        }
    }
    // 3) 重复字面量（同文两处以上——应抽 key）。
    let mut seen: Vec<(String, usize)> = Vec::new();
    for u in units.iter() {
        let hit = seen.iter().find(|(s, _)| *s == u.literal);
        match hit {
            Some((_, first)) => {
                out.push(Finding {
                    at: u.at,
                    pattern: HardcodePattern::DuplicateLiteral,
                    excerpt: clip(&u.literal, 24),
                });
                let _ = first;
            }
            None => seen.push((u.literal.clone(), u.at)),
        }
    }
    out
}

/// 摘录截断（证据可读不刷屏）。
fn clip(s: &str, n: usize) -> String {
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i >= n {
            out.push('…');
            break;
        }
        out.push(c);
    }
    out
}

// ---------------------------------------------------------------------------
// 三、回退链与显性回退
// ---------------------------------------------------------------------------

/// 回退链（语言→回退序列；zh-Hans→zh→root）。
pub fn fallback_chain(lang: &str) -> Vec<String> {
    let mut chain: Vec<String> = Vec::new();
    let mut cur = locale_canonical(lang);
    let mut depth = 0usize;
    loop {
        if depth >= MAX_FALLBACK_DEPTH || chain.iter().any(|c| *c == cur) {
            break;
        }
        chain.push(cur.clone());
        // 拆地区：zh-Hans → zh；第二个 '-' 后的 script 也拆。
        let next = match cur.rfind('-') {
            Some(i) => cur[..i].to_string(),
            None => {
                if cur != "en" {
                    "en".to_string()
                } else {
                    break;
                }
            }
        };
        if next == cur {
            break;
        }
        cur = next;
        depth += 1;
    }
    if !chain.iter().any(|c| c == "en") && chain.len() < MAX_FALLBACK_DEPTH {
        chain.push("en".to_string());
    }
    chain
}

/// 语言规范化（vei02 BCP47 口径——非法输入按原文小写）。
fn locale_canonical(lang: &str) -> String {
    match parse_lenient(lang, &ParseBudget::DOMAIN_DEFAULT) {
        Ok(p) => p.canonical,
        Err(_) => lang.to_lowercase(),
    }
}

/// 一条可译条目（key+源文+上下文+各语言译文）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransUnit {
    /// 键。
    pub key: String,
    /// 源文（en）。
    pub source_en: String,
    /// 译者上下文注释（空=无上下文——阻断提交）。
    pub context: String,
    /// 译文表（lang → text）。
    pub translations: Vec<(String, String)>,
}

impl TransUnit {
    /// 某语言是否有译。
    pub fn has(&self, lang: &str) -> bool {
        self.translations.iter().any(|(l, _)| l == lang)
    }

    /// 取某语言译文。
    pub fn get(&self, lang: &str) -> Option<&String> {
        self.translations.iter().find(|(l, _)| l == lang).map(|(_, t)| t)
    }
}

/// 解析结果（译文+回退登记——显性回退）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolved {
    /// 最终采用的语言。
    pub lang: String,
    /// 译文。
    pub text: String,
    /// 是否发生回退（发生了就必须显性登记）。
    pub fell_back: bool,
    /// 回退事件（fell_back 时非空）。
    pub fallback_event: Option<String>,
}

/// 解析（走回退链；缺翻译不静默——回退显性登记）。
pub fn resolve(unit: &TransUnit, want: &str) -> Result<Resolved, String> {
    let chain = fallback_chain(want);
    for (i, lang) in chain.iter().enumerate() {
        if let Some(t) = unit.get(lang) {
            if i == 0 {
                return Ok(Resolved {
                    lang: lang.clone(),
                    text: t.clone(),
                    fell_back: false,
                    fallback_event: None,
                });
            }
            return Ok(Resolved {
                lang: lang.clone(),
                text: t.clone(),
                fell_back: true,
                fallback_event: Some(format!(
                    "{}：{} 缺 {} 译文，显性回退至 {}（链：{}）",
                    E_TR_SILENT_FALLBACK,
                    unit.key,
                    want,
                    lang,
                    chain.join("→")
                )),
            });
        }
    }
    Err(format!(
        "{}：{} 在回退链 {} 上全缺译——按缺译处理（不得静默显示源文占位）",
        E_TR_SILENT_FALLBACK,
        unit.key,
        chain.join("→")
    ))
}

// ---------------------------------------------------------------------------
// 四、上下文断言 + 回注校验
// ---------------------------------------------------------------------------

/// 上下文断言（无上下文=错译——阻断提交）。
pub fn context_assert(unit: &TransUnit) -> Result<(), String> {
    if unit.context.trim().is_empty() {
        return Err(format!(
            "{}：条目 {} 无译者上下文（translators notes）——无上下文翻译=错译，阻断提交",
            E_TR_NO_CONTEXT, unit.key
        ));
    }
    Ok(())
}

/// 占位符集合（{0}/{1}/%s 类——格式串的锚）。
pub fn placeholders(s: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            // 吃到 } 或 8 字符上限。
            let mut j = i + 1;
            let mut tok = String::from("{");
            while j < bytes.len() && bytes[j] != b'}' && j - i < 8 {
                tok.push(bytes[j] as char);
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b'}' {
                tok.push('}');
                out.push(tok);
                i = j + 1;
                continue;
            }
        }
        if bytes[i] == b'%' && i + 1 < bytes.len() {
            // %s/%d 两字符占位。
            out.push(format!("%{}", bytes[i + 1] as char));
            i += 2;
            continue;
        }
        i += 1;
    }
    out
}

/// 回注校验（译文占位符集合必须与源文一致——回注格式错→校验）。
pub fn inject_validate(unit: &TransUnit, lang: &str) -> Result<(), String> {
    let text = match unit.get(lang) {
        Some(t) => t,
        None => {
            return Err(format!(
                "{}：{} 无 {} 译文可回注",
                E_TR_INJECT, unit.key, lang
            ))
        }
    };
    let src = placeholders(&unit.source_en);
    let dst = placeholders(text);
    if src.len() != dst.len() {
        return Err(format!(
            "{}：{} 回注占位符数 {} ≠ 源文 {}（{:?} vs {:?}）——格式串会炸",
            E_TR_INJECT, unit.key, dst.len(), src.len(), src, dst
        ));
    }
    for (i, p) in src.iter().enumerate() {
        if dst.get(i) != Some(p) {
            return Err(format!(
                "{}：{} 回注占位符 #{} {:?} ≠ 源文 {:?}",
                E_TR_INJECT, unit.key, i, dst.get(i), p
            ));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 五、判据
// ---------------------------------------------------------------------------

use crate::checks::CheckSet;

/// F4014 域自检（判据六组：扫描/回退/上下文/回注/单源/收尾）。
pub fn run_veu14_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F4014");

    // --- 硬编码红线（判据一）---
    let units = [
        SourceUnit { at: 0, literal: String::from("OK"), externalized: true },
        SourceUnit { at: 1, literal: String::from("确定"), externalized: false },
        SourceUnit { at: 2, literal: String::from("取消"), externalized: false },
        SourceUnit { at: 3, literal: String::from("确定"), externalized: true },
        SourceUnit { at: 4, literal: String::from("共 {0} 项"), externalized: false },
        SourceUnit { at: 5, literal: String::from("共 {0 项"), externalized: false },
        SourceUnit { at: 6, literal: String::from("Progress: {0}%"), externalized: false },
    ];
    let findings = scan_hardcoded(&units);
    let cjk = findings.iter().filter(|f| f.pattern == HardcodePattern::CjkLiteralInCode).count();
    let split = findings.iter().filter(|f| f.pattern == HardcodePattern::SplitFormat).count();
    let dup = findings.iter().filter(|f| f.pattern == HardcodePattern::DuplicateLiteral).count();
    s.add(
        "U14-硬编码-01",
        cjk == 4 && split == 1 && dup == 1,
        "硬编码扫描三模式族各命中（4 条未外部化 CJK+1 拆断+1 重复）",
    );
    // 每条发现带证据（摘录非空+位置在册）。
    s.add(
        "U14-硬编码-02",
        findings.iter().all(|f| !f.excerpt.is_empty() && f.at < units.len()),
        "每条发现带证据（摘录+位置）",
    );
    // 干净源零发现（不误报）。
    let clean = [
        SourceUnit { at: 0, literal: String::from("OK"), externalized: true },
        SourceUnit { at: 1, literal: String::from("共 {0} 项"), externalized: true },
    ];
    s.add("U14-硬编码-03", scan_hardcoded(&clean).is_empty(), "干净源零发现（伪检不凑数）");

    // --- 回退显性（判据二）---
    // 链构造（zh-Hans→zh→en）。
    let chain = fallback_chain("zh-Hans");
    s.add(
        "U14-回退-01",
        chain.len() >= 2 && chain[0] == "zh-Hans" && chain.iter().any(|c| c == "zh") && chain.iter().any(|c| c == "en"),
        "回退链构造（zh-Hans→zh→en，vei02 规范口径）",
    );
    // 缺失翻译→显性回退（有事件登记）。
    let mut u = TransUnit {
        key: String::from("btn.ok"),
        source_en: String::from("OK"),
        context: String::from("确认按钮"),
        translations: alloc::vec![(String::from("en"), String::from("OK"))],
    };
    let r = resolve(&u, "zh-Hans");
    s.add(
        "U14-回退-02",
        r.as_ref().map(|x| x.fell_back && x.fallback_event.is_some()).unwrap_or(false),
        "缺译显性回退（事件登记）",
    );
    // 有译不回退（零事件）。
    u.translations.push((String::from("zh-Hans"), String::from("确定")));
    let r2 = resolve(&u, "zh-Hans");
    s.add(
        "U14-回退-03",
        r2.as_ref().map(|x| !x.fell_back && x.fallback_event.is_none() && x.text == "确定").unwrap_or(false),
        "有译零回退（不制造噪声事件）",
    );
    // 全缺译→报错（不静默显示源文）。
    let bare = TransUnit {
        key: String::from("k"),
        source_en: String::from("X"),
        context: String::from("c"),
        translations: Vec::new(),
    };
    let r3 = resolve(&bare, "fr-FR");
    s.add(
        "U14-回退-04",
        r3.is_err() && r3.as_ref().unwrap_err().starts_with(E_TR_SILENT_FALLBACK),
        "全缺译报错（不得静默源文占位）",
    );

    // --- 上下文断言（判据三）---
    let no_ctx = TransUnit {
        key: String::from("k2"),
        source_en: String::from("Y"),
        context: String::from("   "),
        translations: Vec::new(),
    };
    let r = context_assert(&no_ctx);
    s.add(
        "U14-上下文-01",
        r.is_err() && r.as_ref().unwrap_err().starts_with(E_TR_NO_CONTEXT),
        "无上下文阻断提交（红线实测）",
    );
    s.add("U14-上下文-02", context_assert(&u).is_ok(), "有上下文放行");

    // --- 回注校验（判据四）---
    let mut inj = TransUnit {
        key: String::from("k3"),
        source_en: String::from("共 {0} 项，完成 {1}%"),
        context: String::from("进度提示"),
        translations: Vec::new(),
    };
    inj.translations.push((String::from("zh-Hans"), String::from("共 {0} 项，完成 {1}%")));
    s.add("U14-回注-01", inject_validate(&inj, "zh-Hans").is_ok(), "占位符一致回注放行");
    // 丢占位符→拒。
    let mut broken = inj.clone();
    broken.translations.clear();
    broken.translations.push((String::from("zh-Hans"), String::from("共 {0} 项")));
    let r = inject_validate(&broken, "zh-Hans");
    s.add(
        "U14-回注-02",
        r.is_err() && r.as_ref().unwrap_err().starts_with(E_TR_INJECT),
        "丢占位符回注拒绝（格式串防护）",
    );
    // 顺序错→拒。
    let mut swapped = inj.clone();
    swapped.translations.clear();
    swapped.translations.push((String::from("zh-Hans"), String::from("共 {1} 项，完成 {0}%")));
    s.add("U14-回注-03", inject_validate(&swapped, "zh-Hans").is_err(), "占位符顺序错回注拒绝");

    // --- 伪检单源（判据五）---
    let ps = peers();
    s.add(
        "U14-单源-01",
        ps.len() == 2 && ps[0].peer == "VE-F3811" && ps[1].peer == "VE-F2904"
            && ps.iter().all(|p| !p.restated.is_empty()),
        "对端声明两行（F3811 伪检族+vei02 标签口径）",
    );

    // --- 版本与暂挂 ---
    let fp = {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in TRANSLATION_VERSION.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    };
    s.add("U14-版本-01", fp != 0, "版本指纹非零（U14-translation-v1）");

    s.add(
        "U14-暂挂-01",
        U14_LEDGER_SUSPENDED_NOTE.contains("暂挂") && U14_LEDGER_SUSPENDED_NOTE.contains("F4014"),
        "U 域账本暂挂声明显性",
    );

    // U14-暂挂-02：判据条数对账（本条为第 16 条）。
    s.add("U14-暂挂-02", s.len() == 15, "判据条数对账（15+本条）");

    s
}

/// U 域账本暂挂声明（跨批对接点：F3811/F4007 复用单源声明；翻译管理/
/// R01 工作流对端——建账前暂挂，移交期模式延续）。
pub const U14_LEDGER_SUSPENDED_NOTE: &str = "国际化与翻译流程（硬编码扫描+回退链+上下文断言+回注校验）入 U 域账本：建账前暂挂声明（移交期模式延续——F4014 同款）；翻译管理与 R01 工作流对端落地后按 key 对账";
