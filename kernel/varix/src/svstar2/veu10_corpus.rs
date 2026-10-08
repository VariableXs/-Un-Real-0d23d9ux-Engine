//! VE-F4010 · 国际化测试语料（VE-U 域 · i18n 语料组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F4010`
//!
//! 锚点原文：「i18n 测试语料（多语言语料库（全支持语言样文+边界样文（最长词/
//! 无空格语言/混合方向/emoji/组合字符——语料覆盖断言（支持语言全有语料（覆盖
//! 红线：语言无语料=该语言无验证（无料红线；边界语料（极端样文（超长/纯符号/
//! 零宽字符风暴——边界覆盖；语料管理（语料版本化（复述制度；依据红线复用（复述。
//! 数据结构：覆盖断言；边界语料集；版本化复述；依据复用。错误路径与降级矩阵：
//! 语言无料→阻断新增语言声明（红线实测）；边界漏→补（覆盖复述）；语料无依据→
//! 撤回（复用红线实测）；版本漂→锚定（复述）。性能逐项分解：覆盖 O(语言数)；
//! 边界 O(样文)；版本 O(1)；依据 O(断言)。跨批对接点：F3945 模式复用声明；
//! F4010 消费；T02/T03 语料对端。无隐私面。」
//!
//! # 一、无料红线是**支持声明的前置条件**，不是事后补课
//!
//! 「语言无语料=该语言无验证」：一个语言被声明为「支持」的那一刻起，它的每条
//! 用户可见路径都在被测试声称覆盖。若该语言连一条样文都没有，这个声称就是
//! 空头支票——缺陷会在上线后以「该语言全部文案乱码/溢出/错排」的形式集中爆雷。
//! 故 [`CorpusLib::claim_language_support`] 对无料语言**阻断**：想声明支持，
//! 先补语料（锚点「语言无料→阻断新增语言声明」）。
//!
//! # 二、边界语料不是「加分项」，是**每类必须存在的探测针**
//!
//! 排版/渲染/合成对正常文本大多正确，事故集中在极端输入：超长复合词撑爆
//! 单行（德语）、无空格分词语言（日语）、双向混排（阿拉伯语夹路径/代码）、
//! emoji ZWJ 序列（一个字素 = 多个码点）、组合字符簇（一个字素 = 基字符+多个
//! 组合标记）、零宽字符风暴（不可见字符注入与双向控制攻击）。
//! [`EdgeCase`] 八类逐一对应一条真实样文，且每条样文必须**自证特征**：
//! 零宽风暴样文必须真的含零宽码点（[`has_zero_width`]），emoji 样文必须真的
//! 含 ZWJ/扩展码点（[`has_emoji`])——否则「边界覆盖」只是表格里的一行字。
//!
//! # 三、语料的**依据**与**版本**是同一枚硬币的两面
//!
//! 每条语料必须带 `source`（依据）：无依据的语料可能是手滑编造的假文本，
//! 用它测出来的「通过」不可信——[`CorpusLib::retract_unsourced`] 撤回并计数
//! （锚点「语料无依据→撤回」）。版本锚定：语料内容变更而版本号不动（版本漂）
//! 会让「回归通过」指向一个已经不存在的语料基线——[`CorpusLib::version_anchor`]
//! 用内容指纹（FNV-1a 全量）绑定版本号，声称 v1 的库指纹必须等于登记的 v1
//! 指纹，否则 [`E_CORPUS_VERSION_DRIFT`]。
//!
//! # 四、单源复用（F3945 模式复用声明）
//!
//! 支持语言全集**不复制** [`super::veu08_density::LANGUAGE_TABLE`]——覆盖断言
//! 直接遍历该单源常量。veu08 增删语言时本单覆盖断言自动跟随，两处清单漂移
//! 在编译期就不可能发生（同一常量的两个消费者）。
//!
//! ## 零 panic 面
//!
//! 生产代码无 `unwrap` / `expect` / 索引越界 / 算术溢出：字符串扫描一律
//! `chars().any()` / `chars().take()`；指纹用 `wrapping_*` 运算；所有失败
//! 返回 `Result` 或清单，绝不 panic。

// lib.rs 只有 `extern crate alloc` 且无 `#[macro_use]`，宏逐文件显式导入。
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::veu08_density::LANGUAGE_TABLE;

// ===========================================================================
// 一、诊断码（独占 U02 语料段，码间互异由判据承载）
// ===========================================================================

/// 语料版本标识（家族格式：U<域内序号>-<slug>-v<N>）。
pub const CORPUS_VERSION: &str = "U02-corpus-v1";

/// 无料红线：语言被声明支持但语料库无其样文。
pub const E_CORPUS_MISSING: &str = "E_CORPUS_MISSING";
/// 依据红线：语料条目无出处依据。
pub const E_CORPUS_UNSOERCED: &str = "E_CORPUS_UNSOERCED";
/// 边界覆盖缺口：某类边界语料缺失或特征自检不过。
pub const E_CORPUS_EDGE_MISSING: &str = "E_CORPUS_EDGE_MISSING";
/// 版本锚定失败：声称的版本与内容指纹不符（版本漂移）。
pub const E_CORPUS_VERSION_DRIFT: &str = "E_CORPUS_VERSION_DRIFT";
/// 版本未登记：版本号不在锚点登记表内。
pub const E_CORPUS_VERSION_UNKNOWN: &str = "E_CORPUS_VERSION_UNKNOWN";
/// 语料语言标签非法（空串/无主标签）。
pub const E_CORPUS_BAD_LANG: &str = "E_CORPUS_BAD_LANG";

// ===========================================================================
// 二、数据结构：语料条目 / 边界枚举 / 覆盖断言
// ===========================================================================

/// 边界语料的八类（锚点「最长词/无空格语言/混合方向/emoji/组合字符」+「超长/
/// 纯符号/零宽字符风暴」）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EdgeCase {
    /// 最长词：超长复合词（德语教科书案例）。
    LongestWord,
    /// 无空格分词语言：书写传统不含词间空格。
    NoSpace,
    /// 混合方向：RTL 语言文本内嵌 LTR 段（或反向）。
    MixedDirection,
    /// emoji：ZWJ 序列 / 肤色修饰 / 国旗对——一个字素多个码点。
    Emoji,
    /// 组合字符：基字符 + 组合标记的字素簇。
    CombiningMarks,
    /// 超长：单条样文字节数超阈值。
    Oversize,
    /// 纯符号：无任何字母/表意字符。
    PureSymbols,
    /// 零宽字符风暴：不可见字符与双向控制码点密集注入。
    ZeroWidthStorm,
}

impl EdgeCase {
    /// 全部八类（边界覆盖断言的枚举域）。
    pub const ALL: [EdgeCase; 8] = [
        EdgeCase::LongestWord,
        EdgeCase::NoSpace,
        EdgeCase::MixedDirection,
        EdgeCase::Emoji,
        EdgeCase::CombiningMarks,
        EdgeCase::Oversize,
        EdgeCase::PureSymbols,
        EdgeCase::ZeroWidthStorm,
    ];

    /// 稳定标识（判据与诊断输出共用，防枚举顺序漂移；const 期指纹亦调用）。
    pub const fn slug(self) -> &'static str {
        match self {
            EdgeCase::LongestWord => "longest-word",
            EdgeCase::NoSpace => "no-space",
            EdgeCase::MixedDirection => "mixed-direction",
            EdgeCase::Emoji => "emoji",
            EdgeCase::CombiningMarks => "combining-marks",
            EdgeCase::Oversize => "oversize",
            EdgeCase::PureSymbols => "pure-symbols",
            EdgeCase::ZeroWidthStorm => "zero-width-storm",
        }
    }
}

/// 语料条目种类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EntryKind {
    /// 常规样文（覆盖断言用）。
    Sample,
    /// 边界样文（边界覆盖用，携带边界类别）。
    Edge(EdgeCase),
}

/// 一条语料：语言 + 样文 + 种类 + 依据 + 加入版本。
///
/// `source` 是依据（复用红线）：空依据 = 不可信语料，撤回。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CorpusEntry {
    /// BCP47 主标签（zh/ja/ko/ar/fa/ur/de/fr/es/pt/it/ru/el…）。
    pub lang: &'static str,
    /// 样文本体（真实文本，非占位符）。
    pub text: &'static str,
    /// 条目种类。
    pub kind: EntryKind,
    /// 依据（出处/制度复述）：空 = 无依据，撤回红线实测对象。
    pub source: &'static str,
    /// 加入时的语料库版本号（版本化复述）。
    pub since: &'static str,
}

// ===========================================================================
// 三、样文特征检测（边界语料的「自证特征」——断言真流经逻辑）
// ===========================================================================

/// 零宽与双向控制码点：U+200B..=U+200F（ZWJ/ZWNJ/ZWSP/LRM/RLM）与 U+FEFF（BOM/零宽不换行空格）。
fn is_zero_width_cp(cp: char) -> bool {
    matches!(cp as u32, 0x200B..=0x200F | 0xFEFF)
}

/// emoji/扩展码点：主要象形文字区（含肤色修饰区间）、ZWJ、区域指示对首字符。
fn is_emoji_cp(cp: char) -> bool {
    let c = cp as u32;
    matches!(c, 0x1F300..=0x1FAFF | 0x200D | 0x1F1E6..=0x1F1FF)
}

/// 组合标记：U+0300..=U+036F（组合发音符）、U+0900..=U+097F 内的上/下标元音与
/// 声随符（梵文/印地文）、U+3099..=U+309A（浊点）。
fn is_combining_cp(cp: char) -> bool {
    let c = cp as u32;
    matches!(c, 0x0300..=0x036F | 0x093C | 0x094D | 0x3099 | 0x309A)
}

/// 样文是否含零宽/双向控制码点。
pub fn has_zero_width(text: &str) -> bool {
    text.chars().any(is_zero_width_cp)
}

/// 样文是否含 emoji 特征码点（ZWJ 序列/象形区/区域指示）。
pub fn has_emoji(text: &str) -> bool {
    text.chars().any(is_emoji_cp)
}

/// 样文是否含组合标记（真组合序列，非预组合字符）。
pub fn has_combining(text: &str) -> bool {
    text.chars().any(is_combining_cp)
}

/// 样文是否为纯符号（无字母/数字/表意文字码点）。
pub fn is_pure_symbols(text: &str) -> bool {
    !text.chars().any(|c| c.is_alphanumeric())
}

/// 样文是否超长（字节口径；阈值 [`OVERSIZE_BYTES`]）。
///
/// 超长阈值 256 字节：UI 单行样文的合理上界——超过它排版折行/截断/溢出路径
/// 才会被真正踩到（正常 UI 文案几乎不会到 256 字节）。
pub const OVERSIZE_BYTES: usize = 256;

/// 样文是否超长。
pub fn is_oversize(text: &str) -> bool {
    text.len() > OVERSIZE_BYTES
}

/// 样文是否为混合方向（同时含 RTL 主文与 LTR 嵌入段）。
///
/// 判定：含 Arabic 区码点（RTL 证据）**且**含拉丁字母（LTR 证据）。
/// 只含单一方向的样文（哪怕整段 RTL）不构成混合方向。
pub fn is_mixed_direction(text: &str) -> bool {
    let has_rtl = text.chars().any(|c| matches!(c as u32, 0x0600..=0x06FF | 0x0750..=0x077F));
    let has_ltr = text.chars().any(|c| c.is_ascii_alphabetic());
    has_rtl && has_ltr
}

// ===========================================================================
// 四、内置语料（全支持语言样文 + 八类边界样文）
// ===========================================================================

/// 超长样文（三段字面量拼接，~1KB，故意超 [`OVERSIZE_BYTES`]）。
const TEXT_OVERSIZE: &str = concat!(
    "这是一条用于触发排版超长路径的样文：The quick brown fox jumps over the lazy dog. ",
    "کیا آپ جاری رکھنا چاہتے ہیں؟ ファイルを保存しました。 파일이 저장되었습니다. ",
    "Donaudampfschifffahrtsgesellschaftskapitänskabinenschlüsselband überall wiederholen. ",
    "το αρχείο αποθηκεύτηκε και η επεξεργασία μπορεί να συνεχιστεί κανονικά ",
    "0123456789 0123456789 0123456789 0123456789 0123456789 0123456789 ",
);

/// 零宽字符风暴样文（U+200B/200C/200D/200E/200F/FEFF 全谱注入）。
const TEXT_ZERO_WIDTH_STORM: &str = "a\u{200B}b\u{200C}c\u{200D}d\u{200E}e\u{200F}f\u{FEFF}g";

/// 内置语料：单源语言全集各一条真实样文 + 八类边界条目。
///
/// 依据（source）逐条注明出处；since = CORPUS_VERSION（v1 基线）。
pub const CORPUS_ENTRIES: &[CorpusEntry] = &[
    // ---- 常规样文：13 个单源语言（veu08::LANGUAGE_TABLE 顺序对齐）------
    CorpusEntry { lang: "zh", kind: EntryKind::Sample, since: CORPUS_VERSION,
        text: "文件已保存，是否继续编辑当前图层？（混排区域 Version 123）",
        source: "F4008 LANGUAGE_TABLE 单源样文：CJK 简体中文界面文案" },
    CorpusEntry { lang: "ja", kind: EntryKind::Sample, since: CORPUS_VERSION,
        text: "ファイルを保存しました。編集を続けますか？",
        source: "F4008 LANGUAGE_TABLE 单源样文：日文汉字假名混排" },
    CorpusEntry { lang: "ko", kind: EntryKind::Sample, since: CORPUS_VERSION,
        text: "파일이 저장되었습니다. 편집을 계속하시겠습니까?",
        source: "F4008 LANGUAGE_TABLE 单源样文：韩文谚文音节块" },
    CorpusEntry { lang: "ar", kind: EntryKind::Sample, since: CORPUS_VERSION,
        text: "تم حفظ الملف بنجاح. هل تريد متابعة التحرير؟",
        source: "F4008 LANGUAGE_TABLE 单源样文：阿拉伯文 RTL 连写" },
    CorpusEntry { lang: "fa", kind: EntryKind::Sample, since: CORPUS_VERSION,
        text: "پرونده ذخیره شد. نیم‌فاصله و کاجله",
        source: "F4008 LANGUAGE_TABLE 单源样文：波斯文（含 ZWNJ نیم‌فاصله 特征）" },
    CorpusEntry { lang: "ur", kind: EntryKind::Sample, since: CORPUS_VERSION,
        text: "فائل محفوظ ہو گئی ہے۔ کیا آپ جاری رکھنا چاہتے ہیں؟",
        source: "F4008 LANGUAGE_TABLE 单源样文：乌尔都文（Nastaliq 风格 RTL）" },
    CorpusEntry { lang: "de", kind: EntryKind::Sample, since: CORPUS_VERSION,
        text: "Die Datei wurde gespeichert. Möchten Sie die Bearbeitung fortsetzen?",
        source: "F4008 LANGUAGE_TABLE 单源样文：德文（复合词与变音符）" },
    CorpusEntry { lang: "fr", kind: EntryKind::Sample, since: CORPUS_VERSION,
        text: "Le fichier a été enregistré. Voulez-vous continuer la modification ?",
        source: "F4008 LANGUAGE_TABLE 单源样文：法文（尖音与连字）" },
    CorpusEntry { lang: "es", kind: EntryKind::Sample, since: CORPUS_VERSION,
        text: "El archivo se ha guardado. ¿Desea continuar con la edición?",
        source: "F4008 LANGUAGE_TABLE 单源样文：西班牙文（反问号与重音）" },
    CorpusEntry { lang: "pt", kind: EntryKind::Sample, since: CORPUS_VERSION,
        text: "O arquivo foi salvo. Deseja continuar a edição do documento?",
        source: "F4008 LANGUAGE_TABLE 单源样文：葡萄牙文（鼻音与波浪线）" },
    CorpusEntry { lang: "it", kind: EntryKind::Sample, since: CORPUS_VERSION,
        text: "Il file è stato salvato. Vuoi continuare la modifica del progetto?",
        source: "F4008 LANGUAGE_TABLE 单源样文：意大利文（重音与软化符）" },
    CorpusEntry { lang: "ru", kind: EntryKind::Sample, since: CORPUS_VERSION,
        text: "Файл успешно сохранён. Продолжить редактирование документа?",
        source: "F4008 LANGUAGE_TABLE 单源样文：俄文（西里尔字母与 Ё）" },
    CorpusEntry { lang: "el", kind: EntryKind::Sample, since: CORPUS_VERSION,
        text: "Το αρχείο αποθηκεύτηκε. Να συνεχιστεί η επεξεργασία;",
        source: "F4008 LANGUAGE_TABLE 单源样文：希腊文（终局符 ς 与 σ）" },
    // ---- 边界样文：八类逐一（锚点「最长词/无空格/混合方向/emoji/组合字符」
    //      +「超长/纯符号/零宽字符风暴」），每条自证特征由判据承载 ------------
    CorpusEntry { lang: "de", kind: EntryKind::Edge(EdgeCase::LongestWord), since: CORPUS_VERSION,
        text: "Donaudampfschifffahrtsgesellschaftskapitänskabinenschlüsselband",
        source: "德语真实复合词教科书案例（多瑙河蒸汽航运公司船长室钥匙带）" },
    CorpusEntry { lang: "ja", kind: EntryKind::Edge(EdgeCase::NoSpace), since: CORPUS_VERSION,
        text: "日本語の文章は通常単語の間にスペースを入れずに連続して書かれる",
        source: "日文书写传统无词间空格（分词靠辞书与统计）" },
    CorpusEntry { lang: "ar", kind: EntryKind::Edge(EdgeCase::MixedDirection), since: CORPUS_VERSION,
        text: "افتح المجلد C:\\Users\\Public ثم اضغط موافق",
        source: "阿拉伯文界面内嵌 Windows 路径（真实 RTL×LTR 混排案例）" },
    CorpusEntry { lang: "zh", kind: EntryKind::Edge(EdgeCase::Emoji), since: CORPUS_VERSION,
        text: "家人: 👨‍👩‍👧‍👦 肤色: 👍🏽 旗帜: 🇨🇳",
        source: "Unicode 15 §23.1：ZWJ 家族序列/肤色修饰符/区域指示对（一字素多码点）" },
    CorpusEntry { lang: "hi", kind: EntryKind::Edge(EdgeCase::CombiningMarks), since: CORPUS_VERSION,
        text: "क्ष त्र ज्ञ हैलो",
        source: "天城文辅音簇（क+virāma+ष 组合簇——组合字符真序列）" },
    CorpusEntry { lang: "zh", kind: EntryKind::Edge(EdgeCase::Oversize), since: CORPUS_VERSION,
        text: TEXT_OVERSIZE,
        source: "多语言拼接超长样文（>256 字节，触发折行/截断/溢出路径）" },
    CorpusEntry { lang: "zh", kind:EntryKind::Edge(EdgeCase::PureSymbols), since: CORPUS_VERSION,
        text: "≠≈∑∂√∞∫≡§¶†‡‰←→↔⇐⇒⇔",
        source: "数学与排版符号全集（无字母数字表意码点）" },
    CorpusEntry { lang: "zh", kind: EntryKind::Edge(EdgeCase::ZeroWidthStorm), since: CORPUS_VERSION,
        text: TEXT_ZERO_WIDTH_STORM,
        source: "零宽与双向控制全谱注入（U+200B..F 与 U+FEFF——不可见字符审计样文）" },
];

// ===========================================================================
// 五、覆盖断言 / 无料红线 / 边界审计 / 撤回 / 版本锚定
// ===========================================================================

/// 单源语言全集（复用 [`LANGUAGE_TABLE`]，不复制清单——F3945 模式）。
pub fn supported_langs() -> Vec<String> {
    LANGUAGE_TABLE.iter().map(|(l, _)| (*l).to_string()).collect()
}

/// 主标签抽取：`zh-Hans-CN` → `zh`；无 `-` 即整串；空串非法。
fn primary_tag(lang: &str) -> Result<&str, &'static str> {
    match lang.split_once('-') {
        Some((p, _)) if !p.is_empty() => Ok(p),
        Some(_) => Err(E_CORPUS_BAD_LANG),
        None if lang.is_empty() => Err(E_CORPUS_BAD_LANG),
        None => Ok(lang),
    }
}

/// 语料库（可变视图：撤回/新增/审计的操作面）。
pub struct CorpusLib {
    version: String,
    entries: Vec<CorpusEntry>,
}

impl CorpusLib {
    /// 内置基线库（v1 语料全量克隆）。
    pub fn builtin() -> Self {
        CorpusLib { version: CORPUS_VERSION.to_string(), entries: CORPUS_ENTRIES.to_vec() }
    }

    /// 当前版本号。
    pub fn version(&self) -> &str {
        &self.version
    }

    /// 当前条目只读视图。
    pub fn entries(&self) -> &[CorpusEntry] {
        &self.entries
    }

    /// 条目可变视图（判据反向用例的注入面：构造缺料/无依据/越界子集）。
    pub fn entries_mut(&mut self) -> &mut Vec<CorpusEntry> {
        &mut self.entries
    }

    /// 覆盖断言（锚点「覆盖 O(语言数)」）：单源语言全集 - 已有样文的语言 = 缺料清单。
    ///
    /// 空清单 = 覆盖红线达成；非空 = 每个缺口语言都是「无验证声明」。
    pub fn coverage_gaps(&self) -> Vec<String> {
        supported_langs()
            .into_iter()
            .filter(|l| {
                !self.entries.iter().any(|e| {
                    e.lang == l.as_str() && matches!(e.kind, EntryKind::Sample)
                })
            })
            .collect()
    }

    /// 无料红线（锚点「语言无料→阻断新增语言声明」）。
    ///
    /// 声明支持某语言前调用：该语言（按主标签匹配）至少需一条**有依据**的样文，
    /// 否则阻断。空标签/无主标签一律拒绝（[`E_CORPUS_BAD_LANG`]）。
    pub fn claim_language_support(&self, lang: &str) -> Result<(), &'static str> {
        let primary = primary_tag(lang)?;
        let covered = self.entries.iter().any(|e| {
            e.lang == primary
                && matches!(e.kind, EntryKind::Sample)
                && !e.source.is_empty()
        });
        if covered {
            Ok(())
        } else {
            Err(E_CORPUS_MISSING)
        }
    }

    /// 边界审计（锚点「边界 O(样文)」）：八类边界逐一核对「存在 + 自证特征」。
    ///
    /// 返回缺口类别清单；空 = 边界覆盖达成。特征自检让「表格里有」与「样文真的
    /// 具备该特征」分开——只有前者没有后者按缺口处理。
    pub fn edge_audit(&self) -> Vec<EdgeCase> {
        EdgeCase::ALL
            .iter()
            .filter(|c| !self.edge_case_ok(**c))
            .copied()
            .collect()
    }

    /// 单类边界的「存在 + 特征自检」。
    fn edge_case_ok(&self, case: EdgeCase) -> bool {
        self.entries.iter().any(|e| matches!(e.kind, EntryKind::Edge(k) if k == case))
            && match case {
                EdgeCase::ZeroWidthStorm => self
                    .entries
                    .iter()
                    .any(|e| matches!(e.kind, EntryKind::Edge(EdgeCase::ZeroWidthStorm)) && has_zero_width(e.text)),
                EdgeCase::Emoji => self
                    .entries
                    .iter()
                    .any(|e| matches!(e.kind, EntryKind::Edge(EdgeCase::Emoji)) && has_emoji(e.text)),
                EdgeCase::CombiningMarks => self
                    .entries
                    .iter()
                    .any(|e| matches!(e.kind, EntryKind::Edge(EdgeCase::CombiningMarks)) && has_combining(e.text)),
                EdgeCase::PureSymbols => self
                    .entries
                    .iter()
                    .any(|e| matches!(e.kind, EntryKind::Edge(EdgeCase::PureSymbols)) && is_pure_symbols(e.text)),
                EdgeCase::Oversize => self
                    .entries
                    .iter()
                    .any(|e| matches!(e.kind, EntryKind::Edge(EdgeCase::Oversize)) && is_oversize(e.text)),
                EdgeCase::MixedDirection => self
                    .entries
                    .iter()
                    .any(|e| matches!(e.kind, EntryKind::Edge(EdgeCase::MixedDirection)) && is_mixed_direction(e.text)),
                EdgeCase::LongestWord => self.entries.iter().any(|e| {
                    matches!(e.kind, EntryKind::Edge(EdgeCase::LongestWord))
                        && e.text.chars().all(|c| !c.is_whitespace())
                        && e.text.chars().count() >= 32
                }),
                EdgeCase::NoSpace => self.entries.iter().any(|e| {
                    matches!(e.kind, EntryKind::Edge(EdgeCase::NoSpace))
                        && !e.text.contains(' ')
                }),
            }
    }

    /// 依据红线实测（锚点「语料无依据→撤回」）：撤回全部无依据条目并返回数量。
    ///
    /// 撤回不是删除证据——调用方应将返回值计数上行；被撤回语言的覆盖缺口会
    /// 随之出现（断言联动由判据承载）。
    pub fn retract_unsourced(&mut self) -> usize {
        let before = self.entries.len();
        self.entries.retain(|e| !e.source.is_empty());
        before - self.entries.len()
    }

    /// 新增语料条目（入口防护：无依据/语言标签非法一律拒绝）。
    ///
    /// 这是「新增语言声明」的唯一入口：无料红线在此前置阻断。
    pub fn add_entry(&mut self, entry: CorpusEntry) -> Result<(), &'static str> {
        if entry.lang.is_empty() {
            return Err(E_CORPUS_BAD_LANG);
        }
        if entry.source.is_empty() {
            return Err(E_CORPUS_UNSOERCED);
        }
        self.entries.push(entry);
        Ok(())
    }

    /// 版本推进（版本化复述的工作流入口）。
    ///
    /// 语料内容变更后必须显式 bump 版本；未登记的新版本在锚定时报
    /// [`E_CORPUS_VERSION_UNKNOWN`]——变更必须走锚点登记，不能只改内容。
    pub fn bump_version(&mut self, new_version: &str) {
        self.version = new_version.to_string();
    }

    /// 内容指纹（FNV-1a 32bit，全条目 lang+kind+text+source+since 顺序注入）。
    ///
    /// `wrapping_*` 保证无溢出 panic；kind 以 slug 稳定字符串注入防枚举重排漂移。
    pub fn fingerprint(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        let feed = |h: &mut u32, bytes: &[u8]| {
            for b in bytes {
                *h = h.wrapping_mul(0x0100_0193).wrapping_add(*b as u32);
            }
        };
        for e in &self.entries {
            feed(&mut h, e.lang.as_bytes());
            feed(&mut h, e.kind_slug().as_bytes());
            feed(&mut h, e.text.as_bytes());
            feed(&mut h, e.source.as_bytes());
            feed(&mut h, e.since.as_bytes());
        }
        h
    }

    /// 版本锚定（锚点「版本漂→锚定」）。
    ///
    /// 声称 [`CORPUS_VERSION`]（v1）的库，内容指纹必须等于 v1 登记指纹
    /// （[`V1_FINGERPRINT`]）；不符即版本漂移（[`E_CORPUS_VERSION_DRIFT`]）。
    /// 其他版本号尚未登记，一律拒绝（[`E_CORPUS_VERSION_UNKNOWN`]）。
    pub fn version_anchor(&self) -> Result<(), &'static str> {
        let fp = self.fingerprint();
        if self.version == CORPUS_VERSION {
            if fp == V1_FINGERPRINT {
                Ok(())
            } else {
                Err(E_CORPUS_VERSION_DRIFT)
            }
        } else {
            Err(E_CORPUS_VERSION_UNKNOWN)
        }
    }
}

impl CorpusEntry {
    /// 条目种类的稳定标识（指纹注入与诊断输出共用；const 期指纹亦调用）。
    pub const fn kind_slug(&self) -> &'static str {
        match self.kind {
            EntryKind::Sample => "sample",
            EntryKind::Edge(c) => c.slug(),
        }
    }
}

/// v1 基线指纹（const 期对内置语料算出，与运行期 [`CorpusLib::fingerprint`]
/// 同一算法——判据断两者相等防「登记值手抄漂移」）。
pub const V1_FINGERPRINT: u32 = corpus_fingerprint_const(CORPUS_ENTRIES);

/// const 期指纹（与 [`CorpusLib::fingerprint`] 算法逐字节一致）。
const fn corpus_fingerprint_const(entries: &[CorpusEntry]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < entries.len() {
        let e = &entries[i];
        h = feed_const(h, e.lang.as_bytes());
        h = feed_const(h, e.kind_slug().as_bytes());
        h = feed_const(h, e.text.as_bytes());
        h = feed_const(h, e.source.as_bytes());
        h = feed_const(h, e.since.as_bytes());
        i += 1;
    }
    h
}

/// const 期 FNV 字节注入。
const fn feed_const(mut h: u32, bytes: &[u8]) -> u32 {
    let mut j = 0usize;
    while j < bytes.len() {
        h = h.wrapping_mul(0x0100_0193).wrapping_add(bytes[j] as u32);
        j += 1;
    }
    h
}

/// 摘要行（面板/日志共用；不含样文正文——语料正文不进遥测）。
pub fn screen_line() -> String {
    let lib = CorpusLib::builtin();
    format!(
        "corpus {} entries={} gaps={} edges_missing={} fp={:08x}",
        CORPUS_VERSION,
        lib.entries().len(),
        lib.coverage_gaps().len(),
        lib.edge_audit().len(),
        lib.fingerprint(),
    )
}
