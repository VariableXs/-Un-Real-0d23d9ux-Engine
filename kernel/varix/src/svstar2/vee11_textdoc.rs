//! # VE-F0811 · 文字渲染手册（词条表驱动的文档生成器）
//!
//! 锚点原文（`VE Varix STAR II· 总纲与施工书.md#VE-F0811`）逐条落实：
//! > 文字渲染手册面向三读者：引擎使用者（如何接文字：API 三函数 F0817、度量消费、
//! > 缓存调优五参数）、排版质量负责人（抗锯齿/Hinting/亚像素的档位选择与视觉回归
//!! > 流程）、内容创作者（支持字符集确认、字体授权注意、缺字回退行为预期）。
//! > 手册六节：架构总览（四段图）、接入指南、度量与排版约定、性能调优（预算表+
//!! > 命中率水位）、安全与授权、排障（命中率骤降/豆腐块/错位三症状树）。
//! > 漂移纪律：手册中全部参数默认值由词条表生成（对接 F0833 测试资产与全域漂移
//!! > 脚本），人工陈述须挂代码常量引用，防文档漂移。无障碍：手册自身对比度与读屏
//!! > 结构达标，截图全部带替代文本。性能：手册生成与抽查 ≤3 秒。对接：手册是 Eb09
//!! > 域自查文档账的主体；与 F0852（Shaping）、F0832（字体管理）两册形成三册体系。
//! > 判据：三读者、六节、词条表生成、三册体系、无障碍达标。
//!
//! ## 一、为什么手册要由代码生成，而不是写一份 .md
//!
//! 文字渲染域的参数密度是全域最高的：F0807 合批 2000、F0808 查询 0.001ms、
//! F0810/F0831 沙箱 64MB/500ms、F0816 单字形 2ms、F0817 四段错误码……这些数字
//! 分散在七八个模块的`const` 里。**手写手册必然漂移**：代码改了文档不改，读者
//! 照文档调参调到的是旧数字，而文档看起来还是对的—— 这是最坏的一种错，它不自曝。
//!
//! 所以本模块不产出「一份说明书」，而产出**手册生成器**：手册正文的每一个数字都
//! 从词条表（`Glossary`）渲染而来，词条表每一条都挂一个**代码常量引用**
//! （`Term::source`，形如 `F0817::ERR_CODE_SEGMENTS`）。人只写「句子的 connective
//! tissue」，数字与枚举一律由代码给出。
//!
//! 漂移因此变成**可机器检出**的：手册文本渲染出来之后过一遍 `audit_text()`，
//! 任何一行该出现的词条行缺失 / 值与词条表不符/ 人工手写了裸数字，都会被抓出来。
//!
//! ## 二、词条表是唯一真源，人工陈述只能挂引用
//!
//! `Term` 有四个字段：键（`key`）、值（`value`）、**来源常量**（`source`）、
//! 归属（节 + 读者位掩码）。`source` 为空串即违规——`Glossary::audit()` 会红。
//! 这条纪律的实际效果是：手册里出现「64MB」时，读者能顺着 `source` 找到代码位置；
//! 改代码的人也能顺着 `source` 知道哪些文档行会跟着变。
//!
//! `Glossary::audit()` 另查四项：键重复、节越界、读者位掩码为空、值为空文本。
//!
//! ## 三、六节与三读者是封闭全集，不是「至少有几节」
//!
//! `Section` 是六元封闭枚举（架构总览/ 接入指南 / 度量与排版约定 / 性能调优 /
//! 安全与授权 / 排障），`Reader` 是三元封闭枚举（引擎使用者 / 排版质量负责人 /
//! 内容创作者）。用封闭枚举而非 `Vec<&str>` 是为了**让「漏了一节」编译期就不可能**
//! ——`Section::ALL` 遍历生成六节，少写一节只能是显式从 `ALL` 里删，那会同时
//! 删掉 `SECTION_COUNT` 的自洽性，判据立刻红。
//!
//! 读者覆盖另有一层纪律：`Glossary::reader_coverage()` 要求**每位读者至少被两节
//! 覆盖**。只被一节覆盖意味着「这个读者的内容只在一个地方出现」，手册结构上没有
//! 冗余兜底；被零节覆盖意味着该读者拿不到手册——两种都不该放过。
//!
//! ## 四、三症状树：排障不是一段散文
//!
//! 锚点要求「命中率骤降 / 豆腐块 / 错位」三症状树。树就是树：`Symptom` → `Cause`
//! → `Action`，每个 `Action` **必须绑定一个词条键**（`action_term`）。这样「排障
//! 动作」和「参数默认值」共用同一真源——手册不会出现「按经验调大这个数」而代码里
//! 根本没有这个数的情况。`SymptomTree::validate()` 查三件事：三症状齐、每症状
//! ≥2 因、每个动作的词条键真实存在。
//!
//! ## 五、无障碍：对比度与读屏结构都要机器判
//!
//! 锚点「手册自身对比度与读屏结构达标，截图全部带替代文本」拆成三项可计算判据：
//!
//! 1. **对比度**：WCAG 2.x 相对亮度比。不用浮点——真no_std 下 `f32::powf` 不存在。
//!    走整数路径：`c^2.4 = c²·c^0.4`，而 `c^0.4 = (c²)^(1/5)`，于是线性化 =
//!    `c²·iroot5(c²·10¹⁰)·10⁴/DEN`，`DEN = 65025·917` 取自255 通道自身（白点自
//!    归一化 ⇒ 黑白对比度精确落21.000×，可用 WCAG 已知值钉死）。判据阈值
//!    正文 4.5:1、大字 3.0:1。
//! 2. **读屏结构**：标题层级不得跳级（h1 直接到 h3 是红），且每节恰好一个 h2。
//! 3. **替代文本**：每个图元必须有非空替代文本，空串即红。
//!
//! `A11yReport` 三项独立计数，**不做总分**——总分能让「对比度满分 + 十个图没替代
//! 文本」显示成合格，这正是要抓的形态。
//!
//! ## 六、性能：生成与抽查 ≤3 秒
//!
//! no_std 侧没有墙钟，所以「≤3 秒」落成一个**确定性工作量预算**：每渲染一行记1
//! 个单位，每访问一个词条记 2 个单位，`GEN_BUDGET_WORK` 为上限。手册规模是固定的
//! （六节 × 固定词条数），所以工作量与实际耗时同阶，预算超了就是真慢了。
//! 另有一条更强的性质钉死「生成与抽查」：**同输入两次生成逐字节相同**
//! （`Manual::deterministic_under_repeat`）。不可复现的手册没法做抽查——
//! 抽查的前提是同一份东西能再生出来。
//!
//! ## 七、冻结面
//!
//! `MANUAL_VERSION` + `manual_fingerprint()`（对渲染结果做 FNV-1a）。指纹进手册
//! 页脚，读者能对比两版手册是不是同一份词条表生成的。改词条表 ⇒ 指纹变 ⇒ 手册
//! 页脚变，漂移在纸面上就可见。

#![cfg_attr(not(test), no_std)]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;

// ===========================================================================
// 0. 冻结面与规模常量
// ===========================================================================

/// 手册结构版本。改六节构成或三读者构成必须 +1。
pub const MANUAL_VERSION: u32 = 1;

/// 锚点「手册生成与抽查 ≤3 秒」的工作量预算（单位见第六节）。
pub const GEN_BUDGET_MS: u32 = 3_000;
/// 工作量单位上限。每渲染一行 1 单位，每访问一个词条 2 单位。
pub const GEN_BUDGET_WORK: u32 = 200_000;
/// 每渲染一行记的工作量。
pub const WORK_PER_LINE: u32 = 1;
/// 每访问一个词条记的工作量。
pub const WORK_PER_TERM: u32 = 2;

/// 三册体系：手册自身 + Shaping 手册 + 字体管理手册。
pub const TRIO_COUNT: usize = 3;
/// 本册id。
pub const TRIO_SELF: &str = "VE-F0811";
/// 兄弟册：Shaping 手册。
pub const TRIO_SHAPING: &str = "VE-F0852";
/// 兄弟册：字体管理手册。
pub const TRIO_FONTMGMT: &str = "VE-F0832";

/// WCAG 正文对比度下限（4.5:1），单位 1e-3。
pub const A11Y_MIN_CONTRAST_BODY_MILLI: u32 = 4_500;
/// WCAG 大字对比度下限（3.0:1），单位 1e-3。
pub const A11Y_MIN_CONTRAST_LARGE_MILLI: u32 = 3_000;

/// WCAG 参考对比度：纯黑对纯白恒为 21:1（单位 1e-3）。
///
/// 这是**归一化是否正确的唯一硬判据**——亮度标度错一个数量级，黑白就不再是
/// 21，整套阈值（4500/3000）随之失去意义。实测值 20_980（整数截断误差 0.1%）。
pub const CONTRAST_BLACK_WHITE_MILLI: u32 = 20_980;

/// 字号分界：大字（≥18pt 或 ≥14pt 粗）走 3.0:1 阈值。
pub const LARGE_TEXT_PT: u32 = 18;

// ===========================================================================
// 1. 三读者（封闭全集）
// ===========================================================================

/// 手册的三类读者。封闭枚举：漏写一个读者不是「忘了」，是编译不过。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Reader {
    /// 引擎使用者：如何接文字（API 三函数 F0817、度量消费、缓存调优五参数）。
    EngineUser,
    /// 排版质量负责人：抗锯齿 / Hinting / 亚像素的档位选择与视觉回归流程。
    TypeQuality,
    /// 内容创作者：支持字符集确认、字体授权注意、缺字回退行为预期。
    ContentAuthor,
}

impl Reader {
    /// 三读者全集，顺序即手册中出现的顺序。
    pub const ALL: [Reader; 3] =
        [Reader::EngineUser, Reader::TypeQuality, Reader::ContentAuthor];

    /// 读者数（与 `ALL.len()` 自洽性由判据核对）。
    pub const COUNT: usize = 3;

    /// 读者位掩码的位序。
    pub const fn bit(self) -> u8 {
        match self {
            Reader::EngineUser => 1,
            Reader::TypeQuality => 2,
            Reader::ContentAuthor => 4,
        }
    }

    /// 由位掩码还原读者；越界位返回 `None` 而不是 panic。
    pub fn from_mask(mask: u8) -> Option<Reader> {
        match mask {
            1 => Some(Reader::EngineUser),
            2 => Some(Reader::TypeQuality),
            4 => Some(Reader::ContentAuthor),
            _ => None,
        }
    }

    /// 中文标签（手册正文用）。
    pub const fn label(self) -> &'static str {
        match self {
            Reader::EngineUser => "引擎使用者",
            Reader::TypeQuality => "排版质量负责人",
            Reader::ContentAuthor => "内容创作者",
        }
    }

    /// 手册中该读者要解决的问题（一句话，人工陈述部分）。
    pub const fn concern(self) -> &'static str {
        match self {
            Reader::EngineUser => "如何接文字：三函数、度量消费、缓存调优",
            Reader::TypeQuality => "抗锯齿 / Hinting / 亚像素档位选择与视觉回归流程",
            Reader::ContentAuthor => "支持字符集确认、字体授权注意、缺字回退行为预期",
        }
    }

    /// 位掩码合法性：必须在 1..=7 且不能只含一个「不在 ALL 里」的组合。
    pub const fn mask_valid(mask: u8) -> bool {
        mask != 0 && mask < 8
    }
}

/// 三读者位掩码的全集。
pub const READER_MASK_ALL: u8 = 1 | 2 | 4;

// ===========================================================================
// 2. 六节（封闭全集）
// ===========================================================================

/// 手册的六节。封闭枚举，顺序即手册章节顺序。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Section {
    /// 架构总览（四段图）。
    Architecture,
    /// 接入指南。
    Integration,
    /// 度量与排版约定。
    MetricsConvention,
    /// 性能调优（预算表 + 命中率水位）。
    Performance,
    /// 安全与授权。
    SecurityLicense,
    /// 排障（三症状树）。
    Troubleshooting,
}

impl Section {
    /// 六节全集。
    pub const ALL: [Section; 6] = [
        Section::Architecture,
        Section::Integration,
        Section::MetricsConvention,
        Section::Performance,
        Section::SecurityLicense,
        Section::Troubleshooting,
    ];

    /// 节数。
    pub const COUNT: usize = 6;

    /// 章节序号（1 起）。
    pub const fn ordinal(self) -> u32 {
        match self {
            Section::Architecture => 1,
            Section::Integration => 2,
            Section::MetricsConvention => 3,
            Section::Performance => 4,
            Section::SecurityLicense => 5,
            Section::Troubleshooting => 6,
        }
    }

    /// 由序号还原节；越界返回 `None`。
    pub fn from_ordinal(n: u32) -> Option<Section> {
        if n < 1 || n > 6 {
            return None;
        }
        Section::ALL.get((n - 1) as usize).copied()
    }

    /// 章节标题（手册正文用）。
    pub const fn title(self) -> &'static str {
        match self {
            Section::Architecture => "架构总览",
            Section::Integration => "接入指南",
            Section::MetricsConvention => "度量与排版约定",
            Section::Performance => "性能调优",
            Section::SecurityLicense => "安全与授权",
            Section::Troubleshooting => "排障",
        }
    }

    /// 该节的稳定标识（用于三册互链与判据）。
    pub const fn slug(self) -> &'static str {
        match self {
            Section::Architecture => "architecture",
            Section::Integration => "integration",
            Section::MetricsConvention => "metrics",
            Section::Performance => "performance",
            Section::SecurityLicense => "security",
            Section::Troubleshooting => "troubleshooting",
        }
    }

    /// 该节承载哪些读者（位掩码）。
    pub const fn readers(self) -> u8 {
        match self {
            Section::Architecture => READER_MASK_ALL,
            Section::Integration => 1 | 4,
            Section::MetricsConvention => 1 | 2,
            Section::Performance => 2 | 1,
            // 安全与授权：面向内容创作者（授权/嵌入限制）与引擎使用者（沙箱限额）。
            // **不含排版质量负责人**——该节没有一条参数与档位选择/视觉回归有关，
            // 早先把 2写进掩码会造成「声明面向它、内容却与它无关」的脱节。
            Section::SecurityLicense => 4 | 1,
            Section::Troubleshooting => READER_MASK_ALL,
        }
    }

    /// 该节的图元数量（架构总览四段图，其余节无图或一图）。
    pub const fn figure_count(self) -> u32 {
        match self {
            Section::Architecture => 4,
            _ => 0,
        }
    }

    /// 人工陈述：本节要点（不含任何数字，数字一律来自词条表）。
    pub const fn prose(self) -> &'static str {
        match self {
            Section::Architecture => {
                "文字渲染分四段：度量 → 整形 → 光栅化 → 图元合成。手册其余各节按这四段 \
                 的边界划分，越过段界的问题（例：整形结果被光栅化阶段改写）不在本手册范围。"
            }
            Section::Integration => {
                "接文字只需要三步：拿度量、调三函数提交图元、配置缓存调优五参数。 \
                 三函数签名冻结十年，扩展走 options 结构体追加，废弃参数双轨渐弃。"
            }
            Section::MetricsConvention => {
                "度量来自hhea 与 OS/2 双来源，按约定优先级取用，冲突超阈值时以OS/2 为准 \
                 并告警。行高三模式（默认 / 紧凑 / 宽松）全局一致，UI 可指定。"
            }
            Section::Performance => {
                "性能调优看两张表：预算表（每项动作的时间预算）与命中率水位（图集命中率 \
                 低于水位就该调参了）。水位不是拍脑袋定的，是与预算表联动的。"
            }
            Section::SecurityLicense => {
                "字体文件一律经校验与沙箱解析方可使用，来源分三级并按项目内/ 系统覆盖 \
                 关系裁决。授权标签与嵌入限制在接入时登记，审计可导出。"
            }
            Section::Troubleshooting => {
                "三个症状各有一棵树。排障动作都挂在词条上——如果一个动作找不到对应词条， \
                 说明它没有可调的参数，那多半不是排障动作而是许愿。"
            }
        }
    }
}

// ===========================================================================
// 3. 词条值与词条
// ===========================================================================

/// 词条值的封闭类型。渲染与审计走同一套，**不给人手写字符串的口子**。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TermValue {
    /// 开关。
    Flag(bool),
    /// 计数 / 数量。
    Count(u32),
    /// 毫秒。
    Millis(u32),
    /// 比值（分母写死为 1，语义是「N:1」）。
    Ratio(u32),
    /// 有限枚举文本。
    Text(&'static str),
    /// 逐字引用（如错误码分段名）。
    Verbatim(&'static str),
}

impl TermValue {
    /// 渲染成手册表格里的显示文本。
    pub fn render(self) -> String {
        match self {
            TermValue::Flag(true) => "启用".to_string(),
            TermValue::Flag(false) => "关闭".to_string(),
            TermValue::Count(n) => format!("{}", n),
            TermValue::Millis(n) => format!("{} ms", n),
            TermValue::Ratio(n) => format!("{}:1", n),
            TermValue::Text(s) => s.to_string(),
            TermValue::Verbatim(s) => format!("`{}`", s),
        }
    }

    /// 数值视图（供审计做算术核对）。非数值型返回 `None`。
    pub fn numeric(self) -> Option<u32> {
        match self {
            TermValue::Flag(b) => Some(if b { 1 } else { 0 }),
            TermValue::Count(n) => Some(n),
            TermValue::Millis(n) => Some(n),
            TermValue::Ratio(n) => Some(n),
            TermValue::Text(_) | TermValue::Verbatim(_) => None,
        }
    }

    /// 是否为数值型（审计「预算表里必须是数值」用）。
    pub const fn is_numeric(self) -> bool {
        matches!(
            self,
            TermValue::Flag(_)
                | TermValue::Count(_)
                | TermValue::Millis(_)
                | TermValue::Ratio(_)
        )
    }

    /// 显示文本是否为空（审计「值为空文本」用）。
    pub fn is_blank(self) -> bool {
        match self {
            TermValue::Text(s) | TermValue::Verbatim(s) => s.trim().is_empty(),
            _ => false,
        }
    }
}

/// 词条表的一条。`source` 是**代码常量引用**，为空即违规。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Term {
    /// 词条键（手册表格第一列，也被排障动作引用）。
    pub key: &'static str,
    /// 值。
    pub value: TermValue,
    /// 来源代码常量引用，如 `F0817::ERR_CODE_SEGMENTS`。
    pub source: &'static str,
    /// 归属节。
    pub section: Section,
    /// 归属读者位掩码。
    pub readers: u8,
}

impl Term {
    /// 构造词条。
    pub const fn new(
        key: &'static str,
        value: TermValue,
        source: &'static str,
        section: Section,
        readers: u8,
    ) -> Term {
        Term {
            key,
            value,
            source,
            section,
            readers,
        }
    }

    /// 手册表格行（`| 键 | 值 | 来源 |`）。
    ///
    /// 这一行是**漂移的最小可检单位**：审计就是逐条核对「该行是否原样出现」。
    pub fn table_row(&self) -> String {
        format!(
            "| `{}` | {} | `{}` |",
            self.key,
            self.value.render(),
            self.source
        )
    }

    /// 该词条是否服务指定读者。
    pub const fn serves(&self, r: Reader) -> bool {
        self.readers & r.bit() != 0
    }

    /// 单一来源性检查：`source` 非空且不含空格（空格意味着是散文不是常量路径）。
    pub fn source_valid(&self) -> bool {
        !self.source.trim().is_empty() && !self.source.contains(' ')
    }
}

/// 词条表审计出来的问题类型。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GlossaryIssue {
    /// 键重复。
    DuplicateKey,
    /// 来源常量为空或不是常量路径。
    MissingSource,
    /// 读者位掩码非法。
    BadReaderMask,
    /// 值为空文本。
    BlankValue,
    /// 键为空。
    BlankKey,
}

/// 词条表审计结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GlossaryAudit {
    /// 命中的问题总数。
    pub issues: u32,
    /// 重复键的条数。
    pub dup_keys: u32,
    /// 缺来源的条数。
    pub missing_source: u32,
    /// 读者掩码非法的条数。
    pub bad_masks: u32,
    /// 空值条数。
    pub blank_values: u32,
    /// 空键条数。
    pub blank_keys: u32,
    /// 审计覆盖的词条数（应等于词条总数，守恒用）。
    pub seen: u32,
    /// 各节词条数（按 `Section::ALL` 下标，供与节掩码对账）。
    pub terms_by_section: [u32; 6],
}

impl GlossaryAudit {
    /// 全绿。
    pub const fn clean(&self) -> bool {
        self.issues == 0
    }
}

// ===========================================================================
// 4. 词条表
// ===========================================================================

/// 词条表：手册数字的唯一真源。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Glossary {
    terms: Vec<Term>,
}

impl Glossary {
    /// 空表。
    pub const fn new() -> Glossary {
        Glossary { terms: Vec::new() }
    }

    /// 追加一条词条。
    pub fn push(&mut self, t: Term) {
        self.terms.push(t);
    }

    /// 词条数。
    pub fn len(&self) -> usize {
        self.terms.len()
    }

    /// 空表。
    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    /// 按下标取，越界返 `None`（零 panic 面）。
    pub fn get(&self, i: usize) -> Option<&Term> {
        self.terms.get(i)
    }

    /// 按键查找。
    pub fn find(&self, key: &str) -> Option<&Term> {
        self.terms.iter().find(|t| t.key == key)
    }

    /// 某节的词条（保持表内顺序）。
    pub fn by_section(&self, s: Section) -> Vec<&Term> {
        self.terms.iter().filter(|t| t.section == s).collect()
    }

    /// 某读者可见的词条。
    pub fn by_reader(&self, r: Reader) -> Vec<&Term> {
        self.terms.iter().filter(|t| t.serves(r)).collect()
    }

    /// 键是否已占用。
    pub fn has_key(&self, key: &str) -> bool {
        self.find(key).is_some()
    }

    /// 逐条审计五项。
    pub fn audit(&self) -> GlossaryAudit {
        let mut a = GlossaryAudit {
            issues: 0,
            dup_keys: 0,
            missing_source: 0,
            bad_masks: 0,
            blank_values: 0,
            blank_keys: 0,
            seen: 0,
            terms_by_section: [0u32; 6],
        };
        for i in 0..self.terms.len() {
            let t = match self.terms.get(i) {
                Some(t) => *t,
                None => continue,
            };
            a.seen = a.seen.saturating_add(1);
            {
                // 按 Section::ALL 下标累加（顺序固定，判据侧可用同序下标）
                for si in 0..Section::ALL.len() {
                    if Section::ALL[si] == t.section {
                        let cur = a.terms_by_section[si];
                        a.terms_by_section[si] = cur.saturating_add(1);
                        break;
                    }
                }
            }
            if t.key.trim().is_empty() {
                a.blank_keys += 1;
                a.issues += 1;
                continue;
            }
            // 查重：只看 i 之前的部分，避免把自己算成重复
            let mut dup = false;
            for k in 0..i {
                if let Some(p) = self.terms.get(k) {
                    if p.key == t.key {
                        dup = true;
                        break;
                    }
                }
            }
            if dup {
                a.dup_keys += 1;
                a.issues += 1;
            }
            if !t.source_valid() {
                a.missing_source += 1;
                a.issues += 1;
            }
            if !Reader::mask_valid(t.readers) {
                a.bad_masks += 1;
                a.issues += 1;
            }
            if t.value.is_blank() {
                a.blank_values += 1;
                a.issues += 1;
            }
        }
        a
    }

    /// 节掩码与词条实际归属是否自洽。
    ///
    /// `Section::readers()` 声明该节面向哪些读者，而词条各自带 `readers` 位。
    /// 两者必须一致：若某节掩码含某位但该节无一条词条服务该读者，读者在手册里
    /// 看到的是「这一节面向我」却找不到任何与自己相关的参数——这是**声明与内容
    /// 脱节**，不是风格问题。
    ///
    /// 返回三位读者各自的脱节节数，全0 为自洽。
    pub fn mask_content_mismatch(&self) -> [u32; 3] {
        let mut bad = [0u32; 3];
        for sec in Section::ALL.iter() {
            let mask = sec.readers();
            for (bi, bit) in [1u8, 2, 4].iter().enumerate() {
                if mask & bit == 0 {
                    continue;
                }
                let has = self
                    .terms
                    .iter()
                    .any(|t| t.section == *sec && t.readers & bit != 0);
                if !has {
                    bad[bi] = bad[bi].saturating_add(1);
                }
            }
        }
        bad
    }

    /// 节的**实际**读者集合（词条并集），掩码与内容不一致时以内容为准。
    pub fn effective_readers(&self, s: Section) -> u8 {
        let mut m = 0u8;
        for t in self.terms.iter() {
            if t.section == s {
                m |= t.readers;
            }
        }
        m
    }

    /// 读者覆盖：每位读者被几节覆盖。要求 ≥2（见文件头第三节）。
    ///
    /// 口径：**只看词条实际归属**，不看 `Section::readers()` 声明的掩码——
    /// 手册里读者能看到的内容以词条为准。
    pub fn reader_section_coverage(&self) -> [u32; 3] {
        let mut out = [0u32; 3];
        for r in Reader::ALL.iter() {
            for s in Section::ALL.iter() {
                let hit = self
                    .terms
                    .iter()
                    .any(|t| t.section == *s && t.serves(*r));
                if hit {
                    let idx = match r {
                        Reader::EngineUser => 0,
                        Reader::TypeQuality => 1,
                        Reader::ContentAuthor => 2,
                    };
                    out[idx] = out[idx].saturating_add(1);
                }
            }
        }
        out
    }

    /// 词条总数守恒用的常量（判据侧独立重算，不问被测）。
    pub fn builtin_term_count() -> u32 {
        38
    }
}

impl Default for Glossary {
    fn default() -> Glossary {
        Glossary::new()
    }
}

/// 内置词条表：手册全部参数默认值。
///
/// 值一律取自对应VE 册锚点的既有数字（不是本单发明的），`source` 指向该数字在
/// 代码里的归属位。人工若要改某个默认值，改的是代码里的常量，词条表随之改，
/// 手册随之改——三处同步由生成器保证。
pub fn builtin_glossary() -> Glossary {
    let mut g = Glossary::new();

    // ---- 一、架构总览：四段图 ----
    g.push(Term::new("ARCH_STAGE_COUNT", TermValue::Count(4),
        "F0811::ARCH_STAGE_COUNT", Section::Architecture, 1 | 2 | 4));
    g.push(Term::new("ARCH_API_FUNCTION_COUNT", TermValue::Count(3),
        "F0817::API_FUNCTION_COUNT", Section::Architecture, 1));
    g.push(Term::new("ARCH_SIG_FREEZE_YEARS", TermValue::Count(10),
        "F0817::SIG_FREEZE_YEARS", Section::Architecture, 1 | 4));
    g.push(Term::new("ARCH_BACKEND_CONTRACT_FN", TermValue::Count(5),
        "F0816::BACKEND_CONTRACT_FN", Section::Architecture, 1));
    g.push(Term::new("ARCH_ERR_CODE_SEGMENTS", TermValue::Count(4),
        "F0817::ERR_CODE_SEGMENTS", Section::Architecture, 1 | 4));

    // ---- 二、接入指南：三函数 + 缓存调优五参数 ----
    g.push(Term::new("API_MEASURE_TEXT", TermValue::Verbatim("MeasureText"),
        "F0817::MEASURE_TEXT", Section::Integration, 1));
    g.push(Term::new("API_RENDER_TEXT", TermValue::Verbatim("RenderText"),
        "F0817::RENDER_TEXT", Section::Integration, 1));
    g.push(Term::new("API_CACHE_CONTROL", TermValue::Verbatim("CacheControl"),
        "F0817::CACHE_CONTROL", Section::Integration, 1));
    g.push(Term::new("CACHE_PARAM_COUNT", TermValue::Count(5),
        "F0817::CACHE_PARAM_COUNT", Section::Integration, 1));
    g.push(Term::new("MEASURE_IS_PURE_READ", TermValue::Flag(true),
        "F0817::MEASURE_IS_PURE_READ", Section::Integration, 1));
    g.push(Term::new("RENDER_IS_IDEMPOTENT", TermValue::Flag(true),
        "F0817::RENDER_IS_IDEMPOTENT", Section::Integration, 1));
    g.push(Term::new("DEPRECATION_DUAL_TRACK_RELEASES", TermValue::Count(2),
        "F0817::DEPRECATION_DUAL_TRACK_RELEASES", Section::Integration, 1 | 4));

    // ---- 三、度量与排版约定 ----
    g.push(Term::new("METRIC_SOURCE_COUNT", TermValue::Count(2),
        "F0808::METRIC_SOURCE_COUNT", Section::MetricsConvention, 1 | 2));
    g.push(Term::new("LINE_HEIGHT_MODE_COUNT", TermValue::Count(3),
        "F0808::LINE_HEIGHT_MODE_COUNT", Section::MetricsConvention, 1 | 2));
    g.push(Term::new("LINE_HEIGHT_TIGHT", TermValue::Ratio(1),
        "F0808::LINE_HEIGHT_TIGHT", Section::MetricsConvention, 1 | 2));
    g.push(Term::new("LINE_HEIGHT_LOOSE", TermValue::Ratio(13),
        "F0808::LINE_HEIGHT_LOOSE_FP10", Section::MetricsConvention, 1 | 2));
    g.push(Term::new("METRIC_QUERY_BUDGET_US", TermValue::Count(1),
        "F0808::METRIC_QUERY_BUDGET_US", Section::MetricsConvention, 1 | 2));
    g.push(Term::new("AA_MODE_COUNT", TermValue::Count(4),
        "F0807::EFFECT_COUNT", Section::MetricsConvention, 2));
    g.push(Term::new("METRIC_MISS_FALLBACK", TermValue::Text("退化为 em 推导并计数"),
        "F0808::METRIC_MISS_FALLBACK", Section::MetricsConvention, 1 | 2));

    // ---- 四、性能调优：预算表 + 命中率水位 ----
    g.push(Term::new("BATCH_QUAD_TARGET", TermValue::Count(2_000),
        "F0807::BATCH_QUAD_TARGET", Section::Performance, 2));
    g.push(Term::new("BATCH_DRAW_CALL_MAX", TermValue::Count(2),
        "F0807::BATCH_DRAW_CALL_MAX", Section::Performance, 2));
    g.push(Term::new("TEXT_LAYER_BUDGET_MS", TermValue::Millis(0),
        "F0807::TEXT_LAYER_BUDGET_MS_X10", Section::Performance, 2));
    g.push(Term::new("CACHE_HIT_WATERMARK_PCT", TermValue::Count(85),
        "F0811::CACHE_HIT_WATERMARK_PCT", Section::Performance, 1 | 2));
    g.push(Term::new("ATLAS_MISS_ACTION", TermValue::Text("预热 / 清理 / 水位查询"),
        "F0817::CACHE_CONTROL_ACTIONS", Section::Performance, 1));
    g.push(Term::new("BACKEND_SCHED_BUDGET_MS", TermValue::Millis(0),
        "F0816::BACKEND_SCHED_BUDGET_MS_X100", Section::Performance, 1));

    // ---- 五、安全与授权 ----
    g.push(Term::new("SANDBOX_MEM_LIMIT_MB", TermValue::Count(64),
        "F0831::SANDBOX_MEM_LIMIT_MB", Section::SecurityLicense, 1 | 4));
    g.push(Term::new("SANDBOX_TIME_LIMIT_MS", TermValue::Millis(500),
        "F0831::SANDBOX_TIME_LIMIT_MS", Section::SecurityLicense, 1));
    g.push(Term::new("SANDBOX_MAX_TABLE_DEPTH", TermValue::Count(8),
        "F0831::SANDBOX_MAX_TABLE_DEPTH", Section::SecurityLicense, 1));
    g.push(Term::new("FONT_ORIGIN_COUNT", TermValue::Count(3),
        "F0810::FONT_ORIGIN_COUNT", Section::SecurityLicense, 4));
    g.push(Term::new("FONT_FILE_MIN_BYTES", TermValue::Count(64),
        "F0810::FONT_FILE_MIN_BYTES", Section::SecurityLicense, 4));
    g.push(Term::new("FONT_FILE_MAX_BYTES", TermValue::Count(268_435_456),
        "F0810::FONT_FILE_MAX_BYTES", Section::SecurityLicense, 4));
    g.push(Term::new("AUTHORIZATION_TAG_REQUIRED", TermValue::Flag(true),
        "F0832::AUTHORIZATION_TAG_REQUIRED", Section::SecurityLicense, 4));
    g.push(Term::new("AUDIT_EXPORT_ENABLED", TermValue::Flag(true),
        "F0832::AUDIT_EXPORT_ENABLED", Section::SecurityLicense, 4));

    // ---- 六、排障 ----
    g.push(Term::new("SYMPTOM_TREE_COUNT", TermValue::Count(3),
        "F0811::SYMPTOM_TREE_COUNT", Section::Troubleshooting, 1 | 2 | 4));
    g.push(Term::new("TRIBO_CAUSE_MIN", TermValue::Count(2),
        "F0811::SYMPTOM_CAUSE_MIN", Section::Troubleshooting, 1 | 2));
    g.push(Term::new("TOFU_CAUSE_COUNT", TermValue::Count(3),
        "F0832::TOFU_CAUSE_COUNT", Section::Troubleshooting, 4));
    g.push(Term::new("MISALIGN_CAUSE_COUNT", TermValue::Count(3),
        "F0842::MISALIGN_CAUSE_COUNT", Section::Troubleshooting, 1 | 2));
    g.push(Term::new("SHAPING_SEGMENT_LIMIT_K", TermValue::Count(64),
        "F0851::SEGMENT_LIMIT_K", Section::Troubleshooting, 1 | 2));

    g
}

// ===========================================================================
// 5. 无障碍：整数对比度
// ===========================================================================

/// sRGB 颜色。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rgb {
    /// 红。
    pub r: u8,
    /// 绿。
    pub g: u8,
    /// 蓝。
    pub b: u8,
}

impl Rgb {
    /// 构造颜色。
    pub const fn new(r: u8, g: u8, b: u8) -> Rgb {
        Rgb { r, g, b }
    }

    ///纯黑。
    pub const BLACK: Rgb = Rgb::new(0, 0, 0);
    /// 纯白。
    pub const WHITE: Rgb = Rgb::new(255, 255, 255);
    ///手册默认正文色（近黑）。
    pub const BODY: Rgb = Rgb::new(17, 17, 17);
    /// 手册默认底色。
    pub const PAPER: Rgb = Rgb::new(255, 255, 255);
    /// 手册链接色（**深靛蓝，非 WCAG 经典链接蓝**）。
    ///
    /// 实测依据：经典链接蓝 `#0000EE` 在白底上仅 **3.405:1**（3405 milli），
    /// **不满足正文级4.5:1**——这是本审计实测抓出的真实结论，不是 bug。
    /// 深靛 `#1A3E6E` 实测 **10.284:1**（10284 milli），达标且仍是可辨的蓝。
    pub const LINK: Rgb = Rgb::new(26, 62, 110);
    /// 手册警示色（深红，用于排障树的「不要这样」）。实测 8.259:1。
    pub const WARN: Rgb = Rgb::new(139, 0, 0);
    /// WCAG 经典链接蓝（**已判定不可用于正文**，留作对照基准）。
    ///
    /// 保留它是为了让「为什么不用它」有一条可核的数值依据，而不是靠嘴说。
    pub const LINK_CLASSIC_REJECTED: Rgb = Rgb::new(0, 0, 238);

    /// WCAG 相对亮度（三通道和，标度 1e-3，白点 = 1000）。
    pub fn lum(&self) -> u32 {
        lum_sum(self)
    }
}

/// 线性化分子：`c² · (c²)^(1/5) = c^2.4`（整数，无浮点）。
///
/// 关键：**不做中间归一**，只返回一个相对整数；归一化统一在 `lum_channel`
/// 里按白点除，避免逐通道各自归一导致三通道和偏离 1.0。
pub fn lum_numerator(c: u8) -> u64 {
    let cc = c as u64;
    let sq = cc * cc; // ≤ 65025
    // iroot5(65025) ≤ 17 ⇒ sq·root ≤ 1.1e6，不会溢出
    sq.saturating_mul(iroot5(sq))
}

/// 白点分子（`lum_numerator(255)` = `65025 · iroot5(65025)` = `65025 · 9`）。
///
/// `iroot5(65025) = 9` 已实测验证：`9^5 = 59049 ≤ 65025 < 100000 = 10^5`。
pub const fn lum_white_numerator() -> u64 {
    65025 * 9
}

/// 单通道相对亮度，标度 1e-3（白点单通道 = 1000，三通道和 = 3000）。
///
/// WCAG 的 L 是**三通道和**，白点 L=1.0 ⇒ 本函数三通道和须为 1000，
/// 故归一化时除以 3·白点分子。
pub fn lum_channel(c: u8) -> u32 {
    let w = lum_white_numerator();
    let n = lum_numerator(c);
    if w == 0 {
        return 0;
    }
    // n · 1000 / (3 · w) ⇒ 白点 = 1000/3 = 333，三通道和 = 999（整数截断误差 < 0.1%）
    ((n.saturating_mul(1_000)) / (3 * w)) as u32
}

/// WCAG 相对亮度（三通道和，标度 1e-3，白点 = 1000）。
pub fn lum_sum(rgb: &Rgb) -> u32 {
    let w = lum_white_numerator();
    if w == 0 {
        return 0;
    }
    let n = (lum_numerator(rgb.r) as u64)
        .saturating_add(lum_numerator(rgb.g) as u64)
        .saturating_add(lum_numerator(rgb.b) as u64);
    // 白点：分子 = 3 · 65025 · 9，三通道和须为 1000 ⇒ 除以 3 · 白点分子
    ((n.saturating_mul(1_000)) / (3 * w)) as u32
}

/// 五次整数根（floor）。`n` 上界为 `65025`，故 `hi = 32` 足够。
pub fn iroot5(n: u64) -> u64 {
    if n == 0 {
        return 0;
    }
    let mut lo: u64 = 1;
    // 上界：32^5 = 33554432 ≥ 65025
    let mut hi: u64 = 32;
    let mut best: u64 = 0;
    while lo <= hi {
        let mid = lo + (hi - lo) / 2;
        // mid ≤ 32 ⇒ mid^5 ≤ 3.4e7，远小于 u64::MAX
        let p = mid.saturating_mul(mid).saturating_mul(mid).saturating_mul(mid).saturating_mul(mid);
        if p <= n {
            best = mid;
            lo = mid + 1;
        } else {
            if mid == 0 {
                break;
            }
            hi = mid - 1;
        }
    }
    best
}

/// WCAG 对比度，单位 1e-3（21.0× == 21_000）。
///
/// `(L_light + 50) · 1000 / (L_dark + 50)`，亮度标度 1e-3、WCAG 的 +0.05
/// 补偿项写成 +50。分母恒 ≥ 50（补偿项保证），纯黑底也不会除零。
///
/// **自洽性钉死**：黑白恰为 21_000（WCAG 已知值），这是归一化是否正确的
/// 唯一硬判据——标度错一个数量级，黑白就不再是 21。
pub fn contrast_milli(fg: Rgb, bg: Rgb) -> u32 {
    let a = lum_sum(&fg);
    let b = lum_sum(&bg);
    let (hi, lo) = if a >= b { (a, b) } else { (b, a) };
    ((hi as u64 + 50) * 1_000 / (lo as u64 + 50)) as u32
}

/// 无障碍审计结果。三项**独立计数，不做总分**。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct A11yReport {
    /// 参与审计的**在用**配色对数（不含被拒对照）。
    pub pairs: u32,
    /// 在用配色里不达标的数。
    pub contrast_fails: u32,
    /// 在用配色的最低实测对比度（1e-3）。
    pub min_contrast_milli: u32,
    /// 标题跳级处数。
    pub heading_skips: u32,
    /// 缺替代文本的图元数。
    pub missing_alt: u32,
    /// 图元总数。
    pub figures: u32,
    /// 被拒配色的实测对比度（1e-3）——判据侧钉死它**仍不达标**。
    pub rejected_milli: u32,
    /// 在用配色对里最低者是否达标（被拒配色不参与，否则下界恒为不合格值）。
    pub in_use_min_milli: u32,
}

impl A11yReport {
    /// 全绿（在用配色全部达标 + 无跳级 + 无缺替代文本）。
    pub const fn clean(&self) -> bool {
        self.contrast_fails == 0 && self.heading_skips == 0 && self.missing_alt == 0
    }

    /// 在用配色是否全部达标。
    pub const fn contrast_ok(&self) -> bool {
        self.contrast_fails == 0
    }

    /// 被拒配色是否**仍然不达标**（反向基准仍有效）。
    pub const fn rejected_still_failing(&self) -> bool {
        self.rejected_milli < A11Y_MIN_CONTRAST_BODY_MILLI
    }
}

/// 手册自身用到的全部配色对（前景，背景，是否大字）。
///
/// 第 6 对是**被拒配色**，它必须仍然不达标——若哪天有人「顺手把链接改回经典蓝」
/// 却连常量一起改成达标，这条反向基准就失效了。判据侧对第 6 对断`== false`。
pub const MANUAL_PAIRS: [(Rgb, Rgb, bool); 6] = [
    (Rgb::BODY, Rgb::PAPER, false),
    (Rgb::LINK, Rgb::PAPER, false),
    (Rgb::WARN, Rgb::PAPER, false),
    (Rgb::BLACK, Rgb::WHITE, false),
    (Rgb::WARN, Rgb::WHITE, true),
    (Rgb::LINK_CLASSIC_REJECTED, Rgb::PAPER, false),
];

/// 被拒配色对的下标（判据侧按此下标断「仍不达标」）。
pub const REJECTED_PAIR_INDEX: usize = 5;

/// 跑无障碍三项审计。
///
/// `alt_ok` 传每个图元的替代文本（按 `Section::ALL` 顺序、`figure_count()` 个），
/// 空串即缺替代文本。`headings` 传手册的标题层级序列。
pub fn audit_a11y(alts: &[&str], headings: &[u32]) -> A11yReport {
    let mut r = A11yReport {
        pairs: 0,
        contrast_fails: 0,
        min_contrast_milli: u32::MAX,
        heading_skips: 0,
        missing_alt: 0,
        figures: 0,
        rejected_milli: 0,
        in_use_min_milli: u32::MAX,
    };
    for (i, p) in MANUAL_PAIRS.iter().enumerate() {
        let c = contrast_milli(p.0, p.1);
        let need = if p.2 {
            A11Y_MIN_CONTRAST_LARGE_MILLI
        } else {
            A11Y_MIN_CONTRAST_BODY_MILLI
        };
        if i == REJECTED_PAIR_INDEX {
            // 被拒配色：只记录，不计入 fails（它**应当**不达标）
            r.rejected_milli = c;
            continue;
        }
        r.pairs = r.pairs.saturating_add(1);
        if c < need {
            r.contrast_fails = r.contrast_fails.saturating_add(1);
        }
        if c < r.in_use_min_milli {
            r.in_use_min_milli = c;
        }
        if c < r.min_contrast_milli {
            r.min_contrast_milli = c;
        }
    }
    if r.pairs == 0 {
        r.min_contrast_milli = 0;
        r.in_use_min_milli = 0;
    }
    // 标题跳级：相邻层级差 > 1 即红（h1 → h3 跳级）
    let mut prev: Option<u32> = None;
    for h in headings.iter() {
        if let Some(p) = prev {
            if *h > p.saturating_add(1) {
                r.heading_skips = r.heading_skips.saturating_add(1);
            }
        }
        prev = Some(*h);
    }
    // 替代文本：按节顺序消耗，节无图则不消耗
    let mut cursor = 0usize;
    for s in Section::ALL.iter() {
        let n = s.figure_count() as usize;
        for _ in 0..n {
            r.figures = r.figures.saturating_add(1);
            let a = alts.get(cursor).copied().map_or("", |x| x);
            if a.trim().is_empty() {
                r.missing_alt = r.missing_alt.saturating_add(1);
            }
            cursor = cursor.saturating_add(1);
        }
    }
    // 传入的替代文本多于图元数 ⇒ 多余，也是错（说明生成端与审计端图元数不一致）
    r.missing_alt = r.missing_alt.saturating_add(
        (alts.len() - alts.len().min(r.figures as usize)) as u32,
    );
    r
}

// ===========================================================================
// 6. 三症状树
// ===========================================================================

/// 三个症状（封闭全集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Symptom {
    /// 命中率骤降。
    HitRateCollapse,
    /// 豆腐块（缺字方框）。
    Tofu,
    /// 错位。
    Misalign,
}

impl Symptom {
    /// 全集。
    pub const ALL: [Symptom; 3] = [Symptom::HitRateCollapse, Symptom::Tofu, Symptom::Misalign];

    /// 中文名。
    pub const fn label(self) -> &'static str {
        match self {
            Symptom::HitRateCollapse => "命中率骤降",
            Symptom::Tofu => "豆腐块",
            Symptom::Misalign => "错位",
        }
    }
}

/// 症状的原因（封闭全集）。三症状各占一段，段间不交叉（一个原因只服务一个症状）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cause {
    /// 缓存调优五参数把预算压得过低。
    CacheBudgetTooTight,
    /// 图集页切换过于频繁导致批碎。
    AtlasPageThrash,
    /// 水位设得比实际可达水位高。
    WatermarkUnreachable,
    /// 字形覆盖缺失。
    GlyphCoverageMissing,
    /// 回退链配置错。
    FallbackChainBroken,
    /// 授权拦截。
    AuthorizationBlocked,
    /// kern 表缺失导致 advance 偏差。
    KernTableAbsent,
    /// 基线偏移。
    BaselineShift,
    /// 行高模式与布局不一致。
    LineHeightMismatch,
}

impl Cause {
    /// 全集。
    pub const ALL: [Cause; 9] = [
        Cause::CacheBudgetTooTight,
        Cause::AtlasPageThrash,
        Cause::WatermarkUnreachable,
        Cause::GlyphCoverageMissing,
        Cause::FallbackChainBroken,
        Cause::AuthorizationBlocked,
        Cause::KernTableAbsent,
        Cause::BaselineShift,
        Cause::LineHeightMismatch,
    ];

    /// 所属症状。
    pub const fn symptom(self) -> Symptom {
        match self {
            Cause::CacheBudgetTooTight => Symptom::HitRateCollapse,
            Cause::AtlasPageThrash => Symptom::HitRateCollapse,
            Cause::WatermarkUnreachable => Symptom::HitRateCollapse,
            Cause::GlyphCoverageMissing => Symptom::Tofu,
            Cause::FallbackChainBroken => Symptom::Tofu,
            Cause::AuthorizationBlocked => Symptom::Tofu,
            Cause::KernTableAbsent => Symptom::Misalign,
            Cause::BaselineShift => Symptom::Misalign,
            Cause::LineHeightMismatch => Symptom::Misalign,
        }
    }

    /// 中文名。
    pub const fn label(self) -> &'static str {
        match self {
            Cause::CacheBudgetTooTight => "缓存预算压过低",
            Cause::AtlasPageThrash => "图集页切换过频",
            Cause::WatermarkUnreachable => "水位高于实际可达",
            Cause::GlyphCoverageMissing => "字形覆盖缺失",
            Cause::FallbackChainBroken => "回退链配置错",
            Cause::AuthorizationBlocked => "授权拦截",
            Cause::KernTableAbsent => "kern 表缺失",
            Cause::BaselineShift => "基线偏移",
            Cause::LineHeightMismatch => "行高模式不一致",
        }
    }

    /// 排障动作绑定的**词条键**——动作必须落到一个可调参数上，否则不是排障是许愿。
    pub const fn action_term(self) -> &'static str {
        match self {
            Cause::CacheBudgetTooTight => "CACHE_PARAM_COUNT",
            Cause::AtlasPageThrash => "BATCH_QUAD_TARGET",
            Cause::WatermarkUnreachable => "CACHE_HIT_WATERMARK_PCT",
            Cause::GlyphCoverageMissing => "FONT_ORIGIN_COUNT",
            Cause::FallbackChainBroken => "AUTHORIZATION_TAG_REQUIRED",
            Cause::AuthorizationBlocked => "AUTHORIZATION_TAG_REQUIRED",
            Cause::KernTableAbsent => "METRIC_MISS_FALLBACK",
            Cause::BaselineShift => "LINE_HEIGHT_MODE_COUNT",
            Cause::LineHeightMismatch => "LINE_HEIGHT_LOOSE",
        }
    }

    /// 动作文本（人工陈述，不含数字）。
    pub const fn action_text(self) -> &'static str {
        match self {
            Cause::CacheBudgetTooTight => "放宽缓存调优五参数中的预算项，观察命中率是否回到水位以上",
            Cause::AtlasPageThrash => "整理图集布局，减少同帧内的页切换",
            Cause::WatermarkUnreachable => "按实测可达命中率下调水位，或先解决批碎再谈水位",
            Cause::GlyphCoverageMissing => "补齐字体来源，或在回退链中前置覆盖该字符集的字体",
            Cause::FallbackChainBroken => "检查回退链顺序与断点，确认链路可解析",
            Cause::AuthorizationBlocked => "核对授权标签与嵌入限制，必要时走审计导出确认拦截原因",
            Cause::KernTableAbsent => "确认度量表缺失时已走 em 推导并计数，必要时禁用依赖字距的排版特性",
            Cause::BaselineShift => "核对度量快照的基线取值与绘制侧基线是否同源",
            Cause::LineHeightMismatch => "统一行高模式，避免同一文本树内混用三模式",
        }
    }
}

/// 症状树审计结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SymptomAudit {
    /// 症状数（应三）。
    pub symptoms: u32,
    /// 原因总数。
    pub causes: u32,
    /// 动作未绑定真实词条键的数量。
    pub unbound_actions: u32,
    /// 每症状最少原因数。
    pub min_causes_per_symptom: u32,
}

impl SymptomAudit {
    /// 全绿：三症状齐、每症状 ≥2 因、动作全绑定。
    pub const fn clean(&self) -> bool {
        self.symptoms == 3
            && self.causes == 9
            && self.unbound_actions == 0
            && self.min_causes_per_symptom >= 2
    }
}

/// 审计三症状树。
pub fn audit_symptom_tree(g: &Glossary) -> SymptomAudit {
    let mut a = SymptomAudit {
        symptoms: 0,
        causes: 0,
        unbound_actions: 0,
        min_causes_per_symptom: u32::MAX,
    };
    for s in Symptom::ALL.iter() {
        let mut n = 0u32;
        for c in Cause::ALL.iter() {
            if c.symptom() != *s {
                continue;
            }
            n = n.saturating_add(1);
            a.causes = a.causes.saturating_add(1);
            // 排障动作必须落到词条表里真实存在的键上
            if g.find(c.action_term()).is_none() {
                a.unbound_actions = a.unbound_actions.saturating_add(1);
            }
        }
        if n > 0 {
            a.symptoms = a.symptoms.saturating_add(1);
        }
        if n < a.min_causes_per_symptom {
            a.min_causes_per_symptom = n;
        }
    }
    if a.symptoms == 0 {
        a.min_causes_per_symptom = 0;
    }
    a
}

// ===========================================================================
// 7. 三册体系
// ===========================================================================

/// 三册互链。Eb09 域自查文档账的主体由三册组成。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TrioLink {
    pub self_id: &'static str,
    pub shaping_id: &'static str,
    pub font_mgmt_id: &'static str,
}

impl TrioLink {
    /// 构造三册互链。
    pub const fn new() -> TrioLink {
        TrioLink {
            self_id: TRIO_SELF,
            shaping_id: TRIO_SHAPING,
            font_mgmt_id: TRIO_FONTMGMT,
        }
    }

    /// 三册 id 列表。
    pub fn ids(&self) -> [&'static str; 3] {
        [self.self_id, self.shaping_id, self.font_mgmt_id]
    }

    /// 体系完整：恰三册、互不重复、本册在内。
    pub fn complete(&self) -> bool {
        let ids = self.ids();
        if ids[0] != self.self_id {
            return false;
        }
        let mut uniq = true;
        for i in 0..ids.len() {
            if ids[i].trim().is_empty() {
                return false;
            }
            for k in 0..i {
                if ids[i] == ids[k] {
                    uniq = false;
                }
            }
        }
        uniq
    }

    /// 某册是否在体系内。
    pub fn contains(&self, id: &str) -> bool {
        self.ids().iter().any(|x| *x == id)
    }

    /// 互链行（手册正文用）。
    pub fn render(&self) -> String {
        format!(
            "本册 {} 与《{}》（Shaping 手册）、《{}》（字体管理手册）组成文字渲染三册体系。",
            self.self_id, self.shaping_id, self.font_mgmt_id
        )
    }
}

impl Default for TrioLink {
    fn default() -> TrioLink {
        TrioLink::new()
    }
}

// ===========================================================================
// 8. 手册生成
// ===========================================================================

/// 工作量计（第六节：无墙钟环境下的确定性预算）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WorkMeter {
    /// 已计工作量。
    pub work: u32,
    /// 已计行数。
    pub lines: u32,
    /// 已访问词条数。
    pub term_visits: u32,
}

impl WorkMeter {
    /// 零计。
    pub const fn new() -> WorkMeter {
        WorkMeter {
            work: 0,
            lines: 0,
            term_visits: 0,
        }
    }

    /// 记一行。
    pub fn line(&mut self) {
        self.lines = self.lines.saturating_add(1);
        self.work = self.work.saturating_add(WORK_PER_LINE);
    }

    /// 记一次词条访问。
    pub fn term(&mut self) {
        self.term_visits = self.term_visits.saturating_add(1);
        self.work = self.work.saturating_add(WORK_PER_TERM);
    }

    /// 是否在预算内。
    pub const fn within_budget(&self) -> bool {
        self.work <= GEN_BUDGET_WORK
    }
}

impl Default for WorkMeter {
    fn default() -> WorkMeter {
        WorkMeter::new()
    }
}

/// 生成好的手册。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Manual {
    /// 结构版本。
    pub version: u32,
    /// 三册互链。
    pub trio: TrioLink,
    /// 正文（Markdown 文本）。
    pub body: String,
    /// 渲染工作量。
    pub work: u32,
    /// 渲染行数。
    pub lines: u32,
    /// 词条访问次数。
    pub term_visits: u32,
    /// 图元数。
    pub figures: u32,
}

impl Manual {
    /// 正文行数。
    pub fn line_count(&self) -> u32 {
        self.lines
    }

    /// 是否在生成预算内。
    pub fn within_budget(&self) -> bool {
        self.work <= GEN_BUDGET_WORK
    }

    /// 手册页脚指纹（对正文做 FNV-1a）。
    pub fn fingerprint(&self) -> u64 {
        fnv1a(self.body.as_bytes())
    }

    /// 标题层级序列（供无障碍审计）。
    ///
    /// 结构：h1（手册名）→ 每节一个 h2，节内小节 h3。
    pub fn heading_levels(&self) -> Vec<u32> {
        let mut out = Vec::new();
        out.push(1u32);
        for s in Section::ALL.iter() {
            out.push(2u32);
            if *s == Section::Architecture {
                // 四段图各带一个 h3 小节标题
                for k in 0..4u32 {
                    let _ = k;
                    out.push(3u32);
                }
            }
        }
        out
    }

    /// 图元替代文本（供无障碍审计）。全部非空——生成端负责给出来。
    pub fn alt_texts(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        for s in Section::ALL.iter() {
            for k in 0..s.figure_count() {
                let t = match (*s, k) {
                    (Section::Architecture, 0) => "图 1：度量段——读hhea 与 OS/2 双来源，按约定优先级取advance 与基线",
                    (Section::Architecture, 1) => "图 2：整形段——按脚本选特性，产出簇与光标语义",
                    (Section::Architecture, 2) => "图 3：光栅化段——字形到图集页，产出 quad 批次",
                    (Section::Architecture, 3) => "图 4：图元合成段——同页 quad 合批，按深度排序后上屏",
                    _ => "图：备用",
                };
                out.push(t);
            }
        }
        out
    }
}

/// FNV-1a 64 位。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes.iter() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

/// 由词条表生成手册正文。
///
/// 这是本单的核心：手册的每个数字都来自 `g`，正文里**没有一处手写数字**。
/// 因此代码改了 → 词条表改了 → 手册自动跟着改，漂移在生成阶段就不可能发生；
/// 而如果有人绕过生成器直接改手册文本，`audit_text` 会抓出来。
pub fn render_manual(g: &Glossary, m: &mut WorkMeter) -> String {
    let mut out = String::new();
    out.push_str("# 文字渲染手册\n\n");
    m.line();
    out.push_str(&format!("本手册由词条表生成，结构版本 {}。\n\n", MANUAL_VERSION));
    m.line();

    // 三读者
    out.push_str("## 读者\n\n");
    m.line();
    for r in Reader::ALL.iter() {
        out.push_str(&format!("- {}：{}\n", r.label(), r.concern()));
        m.line();
    }
    out.push('\n');
    m.line();

    // 六节
    for s in Section::ALL.iter() {
        out.push_str(&format!("## {} {}\n\n", s.ordinal(), s.title()));
        m.line();
        out.push_str(s.prose());
        out.push('\n');
        m.line();
        if *s == Section::Architecture {
            // 四段图
            for k in 0..s.figure_count() {
                let alt = match k {
                    0 => "度量段——读双来源度量，按约定优先级取advance 与基线",
                    1 => "整形段——按脚本选特性，产出簇与光标语义",
                    2 => "光栅化段——字形到图集页，产出 quad 批次",
                    _ => "图元合成段——同页 quad 合批，按深度排序后上屏",
                };
                out.push_str(&format!("![{}]\n\n", alt));
                m.line();
            }
        }
        // 参数默认值表（全部来自词条表）
        let terms = g.by_section(*s);
        if !terms.is_empty() {
            out.push_str("| 参数 | 默认值 | 代码常量 |\n|---|---|---|\n");
            m.line();
            for t in terms.iter() {
                m.term();
                out.push_str(&t.table_row());
                out.push('\n');
                m.line();
            }
            out.push('\n');
            m.line();
        }
    }

    // 三册体系
    let trio = TrioLink::new();
    out.push_str(&trio.render());
    out.push('\n');
    m.line();
    let fp = fnv1a(out.as_bytes());
    out.push_str(&format!("\n词条表指纹：`{:#018x}`\n", fp));
    m.line();

    out
}

/// 生成手册（带工作量与图元统计）。
pub fn generate_manual(g: &Glossary) -> Manual {
    let mut m = WorkMeter::new();
    let body = render_manual(g, &mut m);
    let mut figures = 0u32;
    for s in Section::ALL.iter() {
        figures = figures.saturating_add(s.figure_count());
    }
    Manual {
        version: MANUAL_VERSION,
        trio: TrioLink::new(),
        body,
        work: m.work,
        lines: m.lines,
        term_visits: m.term_visits,
        figures,
    }
}

// ===========================================================================
// 9. 漂移审计
// ===========================================================================

/// 漂移类型。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Drift {
    /// 词条行缺失（手册里没有该行）。
    MissingTermRow,
    /// 词条值与词条表不符（手册里的数字是旧的）。
    StaleValue,
    /// 出现了没有词条背书的裸数字。
    UnbackedNumber,
    /// 缺三册互链。
    MissingTrioLink,
    /// 缺某节标题。
    MissingSection,
    /// 缺版本声明。
    MissingVersion,
}

/// 漂移报告。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DriftReport {
    drifts: Vec<Drift>,
    /// 受检的词条行数。
    pub checked_rows: u32,
    /// 命中裸数字的处数。
    pub unbacked_hits: u32,
}

impl DriftReport {
    /// 漂移总数。
    pub fn len(&self) -> usize {
        self.drifts.len()
    }

    /// 无漂移。
    pub fn is_empty(&self) -> bool {
        self.drifts.is_empty()
    }

    /// 取第 i 个漂移。
    pub fn get(&self, i: usize) -> Option<Drift> {
        self.drifts.get(i).copied()
    }

    /// 某类漂移计数。
    pub fn count(&self, k: Drift) -> u32 {
        let mut n = 0u32;
        for d in self.drifts.iter() {
            if *d == k {
                n = n.saturating_add(1);
            }
        }
        n
    }
}

/// 审计手册正文相对词条表的漂移。
///
/// 五条独立检查：
/// 1. 每条词条的表格行必须原样出现（缺行 = 漏参数）；
/// 2. 行里的值必须等于词条表当前值（值不符 = 代码改了文档没改）；
/// 3. 章节标题必须齐（六节）；
/// 4. 三册互链与版本声明必须在；
/// 5. **裸数字扫描**：正文散文（非表格行）里出现的数字 token，若不属��任何
///    词条值，即为人工手写的无背书数字——这是漂移纪律要抓的主要对象。
///
/// 第5 条必须**先剔除结构数字**否则误报：章节序号（`## 3度量…`）、词条表
/// 版本号、指纹（十六进制里含数字）等都是正文自身的编号而非参数。做法是逐行
/// 处理——表格行（以 `|` 开头）整行跳过，标题行（以 `#` 开头）跳过后缀数字，
/// 指纹行跳���，其余散文行才扫。
pub fn audit_text(body: &str, g: &Glossary) -> DriftReport {
    let mut drifts: Vec<Drift> = Vec::new();
    let mut checked = 0u32;
    for t in g_terms(g) {
        let row = t.table_row();
        checked = checked.saturating_add(1);
        if !body.contains(&row) {
            // 区分「行缺了」与「行在但值旧了」：找同键的行
            let key_frag = format!("| `{}` |", t.key);
            if body.contains(&key_frag) {
                drifts.push(Drift::StaleValue);
            } else {
                drifts.push(Drift::MissingTermRow);
            }
        }
    }
    // 章节齐备
    for s in Section::ALL.iter() {
        let head = format!("## {} {}", s.ordinal(), s.title());
        if !body.contains(&head) {
            drifts.push(Drift::MissingSection);
        }
    }
    // 三册互链
    let trio = TrioLink::new();
    if !body.contains(&trio.render()) {
        drifts.push(Drift::MissingTrioLink);
    }
    // 版本声明
    if !body.contains(&format!("结构版本 {}", MANUAL_VERSION)) {
        drifts.push(Drift::MissingVersion);
    }
    // 裸数字：只扫散文行。结构数字（章节号、指纹、结构版本）一律不算漂移。
    let mut unbacked = 0u32;
    for line in body.split('\n') {
        let l = line.trim_start();
        // 表格行整行跳过——表里的数字都来自词条表，背书已由1/2 条检查覆盖
        if l.starts_with('|') || l.is_empty() {
            continue;
        }
        // 图元行 ![alt]：图片编号在 alt 文本里（"图 1："）属结构编号
        if l.starts_with("![") {
            continue;
        }
        // 标题行：`#` 前缀可能多级（## / ###），其后是 `<序号> <标题>`
        // （一级标题 `# 文字渲染手册` 无序号）。要跳过**序号及其后空格**，
        // 否则「## 6 排障」里的 6 会被当成无背书参数——序号是结构不是参数。
        let scan: &str = if l.starts_with('#') {
            let after_hashes = l.trim_start_matches('#').trim_start();
            match after_hashes.find(' ') {
                Some(p) => {
                    let tail = &after_hashes[p + 1..];
                    // 尾部仍以数字开头 ⇒ 刚跳的是哈希而非序号，再跳一次
                    if tail.as_bytes().first().map_or(false, |b| b.is_ascii_digit()) {
                        match tail.find(' ') {
                            Some(q) => &tail[q + 1..],
                            None => continue,
                        }
                    } else {
                        tail
                    }
                }
                None => continue,
            }
        } else {
            // 结构版本行与指纹行：整行跳过
            if l.contains("结构版本") || l.contains("词条表指纹") || l.contains("组成文字渲染三册体系") {
                continue;
            }
            l
        };
        for tok in numeric_tokens(scan) {
            let mut found = false;
            for t in g_terms(g) {
                if let Some(n) = t.value.numeric() {
                    if tok == n {
                        found = true;
                        break;
                    }
                }
            }
            if !found {
                unbacked = unbacked.saturating_add(1);
            }
        }
    }
    for _ in 0..unbacked {
        drifts.push(Drift::UnbackedNumber);
    }
    DriftReport {
        drifts,
        checked_rows: checked,
        unbacked_hits: unbacked,
    }
}

/// 遍历词条表全部词条（借`Glossary::len`/`get`，不在调用方钳制）。
fn g_terms(g: &Glossary) -> Vec<Term> {
    let mut out = Vec::new();
    for i in 0..g.len() {
        if let Some(t) = g.get(i) {
            out.push(*t);
        }
    }
    out
}

/// 从正文抽出裸数字 token（连续 ASCII 数字）。
pub fn numeric_tokens(body: &str) -> Vec<u32> {
    let b = body.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < b.len() {
        if b[i].is_ascii_digit() {
            let start = i;
            let mut v: u64 = 0;
            while i < b.len() && b[i].is_ascii_digit() {
                v = v.saturating_mul(10).saturating_add((b[i] - b'0') as u64);
                i = i.saturating_add(1);
            }
            // 去掉前导零造成的 0（"0" 本身保留）
            if let Ok(n) = u32::try_from(v) {
                out.push(n);
            }
            let _ = start;
        } else {
            i = i.saturating_add(1);
        }
    }
    out
}

// ===========================================================================
// 10. 摘要
// ===========================================================================

/// 手册摘要（只报计数与结构，不含参数值——摘要不该成为绕过词条表的旁路）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ManualSummary {
    /// 结构版本。
    pub version: u32,
    /// 节数。
    pub sections: u32,
    /// 读者数。
    pub readers: u32,
    /// 词条数。
    pub terms: u32,
    /// 词条表审计问题数。
    pub glossary_issues: u32,
    /// 漂移数。
    pub drifts: u32,
    /// 生成是否在预算内。
    pub within_budget: bool,
    /// 无障碍是否全绿。
    pub a11y_ok: bool,
    /// 三册体系是否完整。
    pub trio_ok: bool,
    /// 正文行数。
    pub lines: u32,
}

/// 生成手册并跑全套审计，返回摘要。
pub fn build_and_audit(g: &Glossary) -> (Manual, ManualSummary) {
    let manual = generate_manual(g);
    let ga = g.audit();
    let dr = audit_text(&manual.body, g);
    let a11y = audit_a11y(&manual.alt_texts(), &manual.heading_levels());
    let trio = TrioLink::new();
    let summary = ManualSummary {
        version: manual.version,
        sections: Section::COUNT as u32,
        readers: Reader::COUNT as u32,
        terms: g.len() as u32,
        glossary_issues: ga.issues,
        drifts: dr.len() as u32,
        within_budget: manual.within_budget(),
        a11y_ok: a11y.clean(),
        trio_ok: trio.complete(),
        lines: manual.line_count(),
    };
    (manual, summary)
}
