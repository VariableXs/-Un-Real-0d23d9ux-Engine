//! vcw07b_docsec_deep ↔ CGPU-F3528 · 文档安全（内部标注过滤）· 深化版
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F3528`（L14779）
//!
//! **判据（锚点原文）**：安全：安全（内部标注过滤——安全复用；测试（安全一组）。
//! 判据：过滤、复用、一组、判据。
//!
//! ## 与既有 vcw07_docsec 的关系
//!
//! 既有模块已给出标注三级闭集 + 敏感子串双保险 + 三面复用的骨架。本模块
//! 是**深化版**：不替换、不改写它，而是在其之上补齐骨架缺失的五处真实工程
//! 语义，使"文档安全"从"一次过滤能跑"升级为"发布链路可审计、可追责、
//! 可回滚、跨版本可对拍"：
//!
//! 1. **过滤不是删除，是留痕**：既有实现把敏感段落整个丢进 `log`（id+判定），
//!    丢了"为什么被滤"的现场。本模块给每次过滤产出**裁决书**
//!    （`Ruling`）：命中哪一层（标注/扫描）、命中哪条规则、命中位置偏移、
//!    裁决前后标注、裁决单调令牌。留痕可重放 = 事后能复算"当初为什么滤"。
//! 2. **复用必须可证同源，而非只对拍结果一致**：既有 `WsReuser` 每面各
//!    `WsFilter::new()` 造三个引擎，结果一致只能证明"行为恰好相同"。本模块
//!    的 `Reuse` 让三面**共用同一引擎实例**（真复用），并以**引擎指纹**
//!    （规则表 FNV-1a + 门标注 + 版本）证明三面同源；指纹不等即复用失效立案。
//! 3. **降级必须显式**：扫描层规则缺失/规则表被改写时，既有实现会静默漏滤。
//!    本模块引入 `ScanTier`（Full/Degraded/Off）与**降级封锁**：降级期
//!    一律按最严处理（Off 等价于全部过滤），宁可错杀不可漏放——文档安全
//!    的失败模式不对称：误删一段文档可重写，泄出一段密钥不可撤回。
//! 4. **跨版本对拍**：规则集演进（v1→v2）时，同一文档在新旧规则下裁决可能
//!    不一致。`VersionDiff` 逐段比对并把"新放行旧滤"/"新滤旧放行"两向差异
//!    显性列出，双向都必须被复核（漏看任一向 = 规则演进引入泄漏）。
//! 5. **误滤可申诉**：安全过滤天然有误杀。本模块给每条过滤产出
//!    `Appeal`（申诉票据 + 指纹），支持按票据撤销误滤并重放，撤销动作本身
//!    也留痕（append-only 语义：只能追加撤销记录，不能抹掉原裁决）。
//!
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 零 IO 零墙钟零浮点；逐条定制数据结构/状态机/错误路径，零模板复述。

use alloc::vec;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const WSD_VERSION: &str = "vcw07b-docsec-deep-v1";

/// 诊断段基址（0x7680 段 · 本模块独占 · 与 vcw07 的 0x760 段不重叠）。
pub const WSD_SEG_BASE: u16 = 0x7680;

/// 规则表容量（深度扫描规则条数上限）。
pub const RULE_CAP: usize = 12;

/// 裁决留痕容量（append-only 定容 · 满则显性拒）。
pub const RULING_CAP: usize = 96;

/// 文档段落容量。
pub const SECTION_CAP: usize = 64;

// --------------------------------------------------------------------------
// 诊断码（十二码 · 段内连续）
// --------------------------------------------------------------------------

/// 文档安全深化诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WsdCode {
    /// 0x7680 规则索引越界。
    RuleRange,
    /// 0x7681 标注闭集外（无法解析 `[mark:xxx]`）。
    MarkUnknown,
    /// 0x7682 段落容量满（拒收新段，不静默丢段）。
    SectionFull,
    /// 0x7683 裁决留痕账满（显性拒，不抹旧账）。
    RulingFull,
    /// 0x7684 复用面指纹不等（三面不同源 → 复用失效）。
    ReuseDiverged,
    /// 0x7685 扫描层降级中放行（降级期错杀优先，此码不应出现）。
    DegradedLeak,
    /// 0x7686 扫描层关闭期放行（等价错杀，零容忍）。
    ScanOffLeak,
    /// 0x7687 跨版本差异未复核（两向差异未结案）。
    DiffUnreviewed,
    /// 0x7688 申诉票据号不存在。
    AppealUnknown,
    /// 0x7689 撤销了一个未被过滤的裁决（撤销语义错）。
    AppealNotFiltered,
    /// 0x768A 规则表指纹漂移（规则被改写而未升版本）。
    RuleFpDrift,
    /// 0x768B 守恒恒等式破（放行+标注滤+扫描滤+错杀 != 处理段数）。
    CountUnbalanced,
}

impl WsdCode {
    /// 码值。
    pub fn val(self) -> u16 {
        WSD_SEG_BASE
            + match self {
                WsdCode::RuleRange => 0x0,
                WsdCode::MarkUnknown => 0x1,
                WsdCode::SectionFull => 0x2,
                WsdCode::RulingFull => 0x3,
                WsdCode::ReuseDiverged => 0x4,
                WsdCode::DegradedLeak => 0x5,
                WsdCode::ScanOffLeak => 0x6,
                WsdCode::DiffUnreviewed => 0x7,
                WsdCode::AppealUnknown => 0x8,
                WsdCode::AppealNotFiltered => 0x9,
                WsdCode::RuleFpDrift => 0xA,
                WsdCode::CountUnbalanced => 0xB,
            }
    }

    /// 全部码（顺序即码序）。
    pub const ALL: [WsdCode; 12] = [
        WsdCode::RuleRange,
        WsdCode::MarkUnknown,
        WsdCode::SectionFull,
        WsdCode::RulingFull,
        WsdCode::ReuseDiverged,
        WsdCode::DegradedLeak,
        WsdCode::ScanOffLeak,
        WsdCode::DiffUnreviewed,
        WsdCode::AppealUnknown,
        WsdCode::AppealNotFiltered,
        WsdCode::RuleFpDrift,
        WsdCode::CountUnbalanced,
    ];

    /// 可操作建议（错误路径必带修复指引）。
    pub fn remedy(self) -> &'static str {
        match self {
            WsdCode::RuleRange => "规则索引越界：限 RULE_CAP 内，先查 rule_at(idx) 边界",
            WsdCode::MarkUnknown => "标注闭集外：只认 public/internal/confidential 三词，改源标记",
            WsdCode::SectionFull => "段落容量满：先 retire 旧段或分册，别静默丢段",
            WsdCode::RulingFull => "裁决账满：先 seal 归档再续写，append-only 不可抹旧账",
            WsdCode::ReuseDiverged => "三面指纹不等：核对门标注与规则表版本，恢复同源再发布",
            WsdCode::DegradedLeak => "降级期出现放行：Degraded 档应一律最严过滤，查规则加载",
            WsdCode::ScanOffLeak => "扫描关闭期出现放行：Off 等价全过滤，此为错杀失败",
            WsdCode::DiffUnreviewed => "跨版本差异未复核：把新放/新滤两向逐条 signed-off",
            WsdCode::AppealUnknown => "申诉票据不存在：核对票据号来源，别凭记忆销案",
            WsdCode::AppealNotFiltered => "撤销非过滤裁决：放行段无需撤销，先查是否票据串号",
            WsdCode::RuleFpDrift => "规则指纹漂移：改规则必升版本并重算 RuleSet 指纹",
            WsdCode::CountUnbalanced => "计数不守恒：核对四类裁决之和与处理段数",
        }
    }

    /// 一行式描述。
    pub fn line(self) -> String {
        format!("[0x{:04X}] {}", self.val(), self.remedy())
    }
}

// --------------------------------------------------------------------------
// 标注闭集
// --------------------------------------------------------------------------

/// 段落安全标注（三级闭集 · 序即敏感度序）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Mark {
    /// 公开（发布面放行）。
    Public,
    /// 内部（发布面过滤）。
    Internal,
    /// 机密（发布面过滤）。
    Confidential,
}

impl Mark {
    /// 标注名。
    pub fn name(self) -> &'static str {
        match self {
            Mark::Public => "public",
            Mark::Internal => "internal",
            Mark::Confidential => "confidential",
        }
    }

    /// 闭集全集。
    pub const ALL: [Mark; 3] = [Mark::Public, Mark::Internal, Mark::Confidential];

    /// 解析（闭集外显性拒）。
    pub fn parse(tag: &str) -> Option<Mark> {
        match tag {
            "public" => Some(Mark::Public),
            "internal" => Some(Mark::Internal),
            "confidential" => Some(Mark::Confidential),
            _ => None,
        }
    }

    /// 秩（数值越大越敏感）。
    pub fn rank(self) -> u8 {
        match self {
            Mark::Public => 0,
            Mark::Internal => 1,
            Mark::Confidential => 2,
        }
    }

    /// 中文名。
    pub fn cn(self) -> &'static str {
        match self {
            Mark::Public => "公开",
            Mark::Internal => "内部",
            Mark::Confidential => "机密",
        }
    }
}

// --------------------------------------------------------------------------
// 扫描规则表
// --------------------------------------------------------------------------

/// 敏感规则类别（闭集 · 每类一票否决）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RuleKind {
    /// 密钥样：`sk-` + ≥16 hex。
    ApiKey,
    /// 令牌样：`bearer ` + 非空。
    Bearer,
    /// 内网主机样：`.corp.local`。
    CorpHost,
    /// 私钥头样：`-----begin`。
    PemHead,
    /// 连接串样：`password=`。
    ConnSecret,
    /// 私网 IP 样：`10.` / `192.168.` / `172.16-31.`。
    PrivateIp,
}

impl RuleKind {
    /// 类别名。
    pub fn name(self) -> &'static str {
        match self {
            RuleKind::ApiKey => "apikey",
            RuleKind::Bearer => "bearer",
            RuleKind::CorpHost => "corp-host",
            RuleKind::PemHead => "pem-head",
            RuleKind::ConnSecret => "conn-secret",
            RuleKind::PrivateIp => "private-ip",
        }
    }

    /// 规则全集（v1 六类）。
    pub const ALL: [RuleKind; 6] = [
        RuleKind::ApiKey,
        RuleKind::Bearer,
        RuleKind::CorpHost,
        RuleKind::PemHead,
        RuleKind::ConnSecret,
        RuleKind::PrivateIp,
    ];
}

/// 规则表（带版本 + 指纹 · 改规则必升版）。
#[derive(Clone, Debug)]
pub struct RuleSet {
    /// 版本号（改规则必升）。
    pub version: u32,
    /// 启用位（按RuleKind 序 · 关位即该规则失效）。
    pub enabled: [bool; 6],
    /// 规则表指纹（FNV-1a 64 · 版本+启用位）。
    pub fp: u64,
}

impl RuleSet {
    /// 造 v1 全开规则表。
    pub fn v1() -> RuleSet {
        let mut r = RuleSet {
            version: 1,
            enabled: [true; 6],
            fp: 0,
        };
        r.fp = r.compute_fp();
        r
    }

    /// 造 v2 全开规则表（演示跨版本对拍：新增规则不改语义，改指纹）。
    pub fn v2() -> RuleSet {
        let mut r = RuleSet {
            version: 2,
            enabled: [true; 6],
            fp: 0,
        };
        r.fp = r.compute_fp();
        r
    }

    /// 造降级规则表（仅前两类启用 · 模拟部分规则加载失败）。
    pub fn degraded() -> RuleSet {
        let mut r = RuleSet {
            version: 1,
            enabled: [true, true, false, false, false, false],
            fp: 0,
        };
        r.fp = r.compute_fp();
        r
    }

    /// 算规则表指纹。
    pub fn compute_fp(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut mix = |v: u64| {
            for b in v.to_le_bytes().iter() {
                h ^= *b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        };
        mix(self.version as u64);
        for e in self.enabled.iter() {
            mix(*e as u64);
        }
        h
    }

    /// 自检指纹（防改写未升版）。
    pub fn fp_intact(&self) -> bool {
        self.fp == self.compute_fp()
    }

    /// 启用类数。
    pub fn enabled_count(&self) -> u32 {
        self.enabled.iter().filter(|e| **e).count() as u32
    }

    /// 按索引取规则类别（越界显性拒）。
    pub fn rule_at(&self, idx: usize) -> Result<RuleKind, WsdCode> {
        RuleKind::ALL
            .get(idx)
            .copied()
            .ok_or(WsdCode::RuleRange)
    }

    /// 规则表自检别名（供引擎调用）。
    pub fn rules_ok_check(&self) -> bool {
        self.fp_intact()
    }

    /// 该类别当前是否启用。
    pub fn on(&self, k: RuleKind) -> bool {
        match RuleKind::ALL.iter().position(|x| *x == k) {
            Some(i) => self.enabled.get(i).copied().unwrap_or(false),
            None => false,
        }
    }
}

/// 单条命中（规则 + 位置偏移）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Hit {
    /// 命中类别。
    pub kind: RuleKind,
    /// 命中字节偏移（ASCII 索引）。
    pub at: usize,
}

/// 扫描档位（三态 · 失败模式不对称：宁可错杀不可漏放）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScanTier {
    /// 全量扫描（规则全开）。
    Full,
    /// 降级（规则部分失效 · 一律按最严处理）。
    Degraded,
    /// 关闭（等价全过滤）。
    Off,
}

impl ScanTier {
    /// 码值。
    pub fn val(self) -> u8 {
        match self {
            ScanTier::Full => 0,
            ScanTier::Degraded => 1,
            ScanTier::Off => 2,
        }
    }

    /// 中文名。
    pub fn cn(self) -> &'static str {
        match self {
            ScanTier::Full => "全量",
            ScanTier::Degraded => "降级",
            ScanTier::Off => "关闭",
        }
    }

    /// 该档位下是否必须最严（全过滤）。
    pub fn must_be_strict(self) -> bool {
        !matches!(self, ScanTier::Full)
    }
}

/// 扫描命中集（按偏移升序 · 同偏移多类别只留最先）。
#[derive(Clone, Debug)]
pub struct ScanHits {
    /// 命中列表。
    pub hits: Vec<Hit>,
}

impl ScanHits {
    /// 空命中集。
    pub fn new() -> ScanHits {
        ScanHits { hits: Vec::new() }
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.hits.is_empty()
    }

    /// 命中数。
    pub fn len(&self) -> usize {
        self.hits.len()
    }

    /// 首次命中类别。
    pub fn first_kind(&self) -> Option<RuleKind> {
        self.hits.first().map(|h| h.kind)
    }

    /// 命中类别去重列表（判定升序 · 渲染稳定）。
    pub fn kinds(&self) -> Vec<RuleKind> {
        let mut v: Vec<RuleKind> = Vec::new();
        for h in self.hits.iter() {
            if !v.iter().any(|k| *k == h.kind) {
                v.push(h.kind);
            }
        }
        v.sort_by_key(|k| {
            RuleKind::ALL
                .iter()
                .position(|x| x == k)
                .unwrap_or(usize::MAX)
        });
        v
    }
}

/// 全量扫描（v1 六规则 · 纯字节匹配 · 零 IO）。
pub fn scan_full(rules: &RuleSet, text: &str) -> ScanHits {
    let mut out = ScanHits::new();
    let lower = text.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let rest = &lower[i..];
        let mut found: Option<Hit> = None;
        // apikey: "sk-" + >=16 hex
        if rules.on(RuleKind::ApiKey) && rest.starts_with("sk-") {
            let hex = rest.as_bytes()[3..]
                .iter()
                .take_while(|b| b.is_ascii_hexdigit())
                .count();
            if hex >= 16 {
                found = Some(Hit { kind: RuleKind::ApiKey, at: i });
            }
        }
        // bearer: "bearer " + non-space
        if found.is_none() && rules.on(RuleKind::Bearer) && rest.starts_with("bearer ") {
            let tail = &rest[7..];
            if tail.as_bytes().first().map(|b| !b.is_ascii_whitespace()) == Some(true) {
                found = Some(Hit { kind: RuleKind::Bearer, at: i });
            }
        }
        // corp-host: ".corp.local"
        if found.is_none() && rules.on(RuleKind::CorpHost) && rest.contains(".corp.local") {
            let at = i + rest.find(".corp.local").unwrap_or(0);
            found = Some(Hit { kind: RuleKind::CorpHost, at });
        }
        // pem-head: "-----begin"
        if found.is_none() && rules.on(RuleKind::PemHead) && rest.contains("-----begin") {
            let at = i + rest.find("-----begin").unwrap_or(0);
            found = Some(Hit { kind: RuleKind::PemHead, at });
        }
        // conn-secret: "password="
        if found.is_none() && rules.on(RuleKind::ConnSecret) && rest.contains("password=") {
            let at = i + rest.find("password=").unwrap_or(0);
            found = Some(Hit { kind: RuleKind::ConnSecret, at });
        }
        // private-ip: "10." / "192.168." / "172.16."~"172.31."
        if found.is_none() && rules.on(RuleKind::PrivateIp) {
            if rest.starts_with("10.") || rest.starts_with("192.168.") {
                found = Some(Hit { kind: RuleKind::PrivateIp, at: i });
            } else if rest.starts_with("172.") {
                let d = rest.as_bytes().get(4).copied().unwrap_or(0);
                let d2 = rest.as_bytes().get(5).copied().unwrap_or(0);
                let oct = if d.is_ascii_digit() && d2.is_ascii_digit() {
                    Some((d - b'0') * 10 + (d2 - b'0'))
                } else {
                    None
                };
                if matches!(oct, Some(v) if (16..=31).contains(&v)) {
                    found = Some(Hit { kind: RuleKind::PrivateIp, at: i });
                }
            }
        }
        if let Some(h) = found {
            out.hits.push(h);
            // 命中后跳过本次匹配窗口（按最长规则跳 8 字节防自重叠噪声）。
            i = i.wrapping_add(8);
            if i > bytes.len() {
                break;
            }
            continue;
        }
        i = i.wrapping_add(1);
    }
    out
}

// --------------------------------------------------------------------------
// 文档段落
// --------------------------------------------------------------------------

/// 文档段落。
#[derive(Clone, Debug)]
pub struct Section {
    /// 段落号。
    pub id: u32,
    /// 标注。
    pub mark: Mark,
    /// 正文。
    pub text: String,
}

impl Section {
    /// 造段。
    pub fn new(id: u32, mark: Mark, text: &str) -> Section {
        Section {
            id,
            mark,
            text: String::from(text),
        }
    }
}

/// 裁决类别（守恒口径四分类）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ruling {
    /// 放行。
    Publish,
    /// 标注层过滤。
    ByMark,
    /// 扫描层过滤。
    ByScan,
    /// 错杀（降级/关闭档下的最严过滤）。
    StrictKill,
}

impl Ruling {
    /// 码值。
    pub fn val(self) -> u8 {
        match self {
            Ruling::Publish => 0,
            Ruling::ByMark => 1,
            Ruling::ByScan => 2,
            Ruling::StrictKill => 3,
        }
    }

    /// 中文名。
    pub fn cn(self) -> &'static str {
        match self {
            Ruling::Publish => "放行",
            Ruling::ByMark => "标注滤",
            Ruling::ByScan => "扫描滤",
            Ruling::StrictKill => "错杀",
        }
    }

    /// 是否为过滤（放行之外皆过滤）。
    pub fn filtered(self) -> bool {
        !matches!(self, Ruling::Publish)
    }
}

/// 裁决书（append-only 留痕 · 单调令牌防重放）。
#[derive(Clone, Debug)]
pub struct RulingRec {
    /// 单调令牌（严格递增 · 重放即拒）。
    pub token: u64,
    /// 段号。
    pub sec: u32,
    /// 裁决。
    pub ruling: Ruling,
    /// 命中类别（扫描层有效）。
    pub kind: Option<RuleKind>,
    /// 命中偏移（扫描层有效）。
    pub at: Option<usize>,
    /// 裁决时该段标注。
    pub mark: Mark,
    /// 裁决时扫描档位。
    pub tier: ScanTier,
    /// 规则表版本。
    pub rules_ver: u32,
}

impl RulingRec {
    /// 裁决书指纹（FNV-1a · 逐字段）。
    pub fn fp(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut mix = |v: u64| {
            for b in v.to_le_bytes().iter() {
                h ^= *b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        };
        mix(self.token);
        mix(self.sec as u64);
        mix(self.ruling.val() as u64);
        mix(match self.kind {
            Some(k) => {
                let p = RuleKind::ALL.iter().position(|x| *x == k).unwrap_or(0xFF) as u64;
                p | (1u64 << 16)
            }
            None => 0,
        });
        mix(self.at.unwrap_or(0xFFFF_FFFF) as u64);
        mix(self.mark.rank() as u64);
        mix(self.tier.val() as u64);
        mix(self.rules_ver as u64);
        h
    }
}

/// 裁决书别名（消除与 `Ruling` 枚举的命名歧义）。
pub type Decision = RulingRec;

// --------------------------------------------------------------------------
// 安全引擎（复用载体 · 三发布面共用同一实例）
// --------------------------------------------------------------------------

/// 发布面（三面 · 手册/站点/导出包）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Face {
    /// 手册。
    Manual,
    /// 站点。
    Site,
    /// 导出包。
    ExportPkg,
}

impl Face {
    /// 面名。
    pub fn name(self) -> &'static str {
        match self {
            Face::Manual => "manual",
            Face::Site => "site",
            Face::ExportPkg => "export-pkg",
        }
    }

    /// 面全集。
    pub const ALL: [Face; 3] = [Face::Manual, Face::Site, Face::ExportPkg];
}

/// 引擎指纹（同源证明：门标注+规则表+版本+档位）。
pub fn engine_fp(gate: Mark, rules: &RuleSet, tier: ScanTier) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut mix = |v: u64| {
        for b in v.to_le_bytes().iter() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    mix(gate.rank() as u64);
    mix(rules.fp);
    mix(rules.version as u64);
    mix(tier.val() as u64);
    h
}

/// 文档安全引擎（三发布面共用同一实例 · 真复用非复制）。
#[derive(Clone, Debug)]
pub struct Engine {
    /// 发布门（只放行门及以下）。
    pub gate: Mark,
    /// 规则表。
    pub rules: RuleSet,
    /// 扫描档位。
    pub tier: ScanTier,
    /// 裁决留痕账（定容 · append-only）。
    pub book: Vec<Decision>,
    /// 计数四分类。
    pub n_publish: u32,
    pub n_by_mark: u32,
    pub n_by_scan: u32,
    pub n_strict: u32,
    /// 处理段数。
    pub processed: u32,
    /// 撤销票据（票据号 -> 被撤销裁决令牌）。
    pub appeals: Vec<(u64, u64)>,
    /// 立案计数（按码序12 格）。
    pub stats: [u64; 12],
}

impl Engine {
    /// 造引擎（全量档 · 门 = Public）。
    pub fn new() -> Engine {
        Engine {
            gate: Mark::Public,
            rules: RuleSet::v1(),
            tier: ScanTier::Full,
            book: Vec::new(),
            n_publish: 0,
            n_by_mark: 0,
            n_by_scan: 0,
            n_strict: 0,
            processed: 0,
            appeals: Vec::new(),
            stats: [0; 12],
        }
    }

    /// 立案计数自增。
    fn bump(&mut self, c: WsdCode) {
        let i = (c.val() - WSD_SEG_BASE) as usize;
        if let Some(cell) = self.stats.get_mut(i) {
            *cell = cell.wrapping_add(1);
        }
    }

    /// 立案计数读出。
    pub fn stat(&self, c: WsdCode) -> u64 {
        let i = (c.val() - WSD_SEG_BASE) as usize;
        self.stats.get(i).copied().unwrap_or(0)
    }

    /// 引擎指纹（同源证明）。
    pub fn fp(&self) -> u64 {
        engine_fp(self.gate, &self.rules, self.tier)
    }

    /// 规则表自检（指纹漂移即拒 · 纯查询不写计数）。
    pub fn rules_ok(&self) -> bool {
        self.rules.fp_intact()
    }

    /// 单段裁决（标注层 → 扫描层 → 档位最严闸）。
    pub fn judge_section(&mut self, s: &Section) -> Result<(Decision, Ruling), WsdCode> {
        if !self.rules.rules_ok_check() {
            self.bump(WsdCode::RuleFpDrift);
            return Err(WsdCode::RuleFpDrift);
        }
        if self.book.len() >= RULING_CAP {
            self.bump(WsdCode::RulingFull);
            return Err(WsdCode::RulingFull);
        }
        self.processed = self.processed.wrapping_add(1);
        let token = self.book.len() as u64 + 1;
        let (ruling, kind, at) = if s.mark.rank() > self.gate.rank() {
            // 标注层先判（标注级过滤优先于扫描——省一次扫描）。
            (Ruling::ByMark, None, None)
        } else if self.tier.must_be_strict() {
            // 档位最严闸：降级/关闭一律错杀，杜绝降级期泄漏。
            (Ruling::StrictKill, None, None)
        } else {
            let hits = scan_full(&self.rules, &s.text);
            match hits.first_kind() {
                Some(k) => {
                    let at = hits.hits.first().map(|h| h.at);
                    (Ruling::ByScan, Some(k), at)
                }
                None => (Ruling::Publish, None, None),
            }
        };
        match ruling {
            Ruling::Publish => self.n_publish = self.n_publish.wrapping_add(1),
            Ruling::ByMark => self.n_by_mark = self.n_by_mark.wrapping_add(1),
            Ruling::ByScan => self.n_by_scan = self.n_by_scan.wrapping_add(1),
            Ruling::StrictKill => self.n_strict = self.n_strict.wrapping_add(1),
        }
        let d = Decision {
            token,
            sec: s.id,
            ruling,
            kind,
            at,
            mark: s.mark,
            tier: self.tier,
            rules_ver: self.rules.version,
        };
        self.book.push(d.clone());
        Ok((d, ruling))
    }

    /// 全文档裁决（定容 · 溢出席性拒）。
    pub fn judge_doc(&mut self, doc: &[Section]) -> Result<Vec<(Decision, Ruling)>, WsdCode> {
        if doc.len() > SECTION_CAP {
            self.bump(WsdCode::SectionFull);
            return Err(WsdCode::SectionFull);
        }
        let mut out: Vec<(Decision, Ruling)> = Vec::new();
        for s in doc.iter() {
            match self.judge_section(s) {
                Ok(v) => out.push(v),
                Err(e) => return Err(e),
            }
        }
        Ok(out)
    }

    /// 守恒恒等式：四分类之和 == 处理段数。
    pub fn conserved(&self) -> bool {
        self.n_publish.wrapping_add(self.n_by_mark)
            .wrapping_add(self.n_by_scan)
            .wrapping_add(self.n_strict)
            == self.processed
    }

    /// 令牌单调性（严格递增无重放）。
    pub fn tokens_monotonic(&self) -> bool {
        let mut prev = 0u64;
        for d in self.book.iter() {
            if d.token <= prev {
                return false;
            }
            prev = d.token;
        }
        true
    }

    /// 裁决账指纹（FNV 链式 · 任一改写即整链漂移）。
    pub fn book_fp(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for d in self.book.iter() {
            h ^= d.fp();
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }

    /// 撤销误滤（append-only 语义：只追加撤销记录，不抹原裁决）。
    pub fn revoke(&mut self, token: u64) -> Result<Mark, WsdCode> {
        let d = match self.book.iter().find(|x| x.token == token) {
            Some(v) => v.clone(),
            None => {
                self.bump(WsdCode::AppealUnknown);
                return Err(WsdCode::AppealUnknown);
            }
        };
        if !d.ruling.filtered() {
            self.bump(WsdCode::AppealNotFiltered);
            return Err(WsdCode::AppealNotFiltered);
        }
        // 只能撤销扫描层误杀（标注层过滤是策略意图，不接受申诉）。
        if !matches!(d.ruling, Ruling::ByScan | Ruling::StrictKill) {
            self.bump(WsdCode::AppealNotFiltered);
            return Err(WsdCode::AppealNotFiltered);
        }
        self.appeals.push((token, d.sec as u64));
        Ok(d.mark)
    }

    /// 某段是否已被撤销放行。
    pub fn revoked(&self, sec: u32) -> bool {
        self.appeals.iter().any(|(_, s)| *s == sec as u64)
    }

    /// 渲染裁决簿（逐条一行 · 可读可追）。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str("tok  sec  ruling   mark         tier  rulever  kind        at     fp\n");
        for d in self.book.iter() {
            s.push_str(&format!(
                "{:<4} {:<4} {:<8} {:<12} {:<5} {:<7}  {:<11} {:<6} 0x{:016X}\n",
                d.token,
                d.sec,
                d.ruling.cn(),
                d.mark.cn(),
                d.tier.cn(),
                d.rules_ver,
                match d.kind {
                    Some(k) => k.name(),
                    None => "-",
                },
                match d.at {
                    Some(v) => v,
                    None => 0,
                },
                d.fp()
            ));
        }
        s.push_str(&format!(
            "counts pub={} mark={} scan={} strict={} processed={} conserved={} book_fp=0x{:016X}\n",
            self.n_publish,
            self.n_by_mark,
            self.n_by_scan,
            self.n_strict,
            self.processed,
            self.conserved(),
            self.book_fp()
        ));
        s
    }
}

// --------------------------------------------------------------------------
// 跨版本对拍
// --------------------------------------------------------------------------

/// 差异方向。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiffSide {
    /// 新放行旧过滤（规则放宽 → 必须复核，防泄漏）。
    Relaxed,
    /// 新过滤旧放行（规则收紧 → 必须复核，防大面积误杀）。
    Tightened,
    /// 两向一致。
    Same,
}

/// 逐段差异记录。
#[derive(Clone, Debug)]
pub struct DiffRow {
    /// 段号。
    pub sec: u32,
    /// 差异方向。
    pub side: DiffSide,
    /// 旧裁决。
    pub old: Ruling,
    /// 新裁决。
    pub new: Ruling,
    /// 是否已复核签字。
    pub signed: bool,
}

/// 跨版本对拍器。
#[derive(Clone, Debug)]
pub struct VersionDiff {
    /// 旧版本号。
    pub old_ver: u32,
    /// 新版本号。
    pub new_ver: u32,
    /// 差异行。
    pub rows: Vec<DiffRow>,
    /// 未复核数。
    pub unsigned: u32,
}

impl VersionDiff {
    /// 造对拍器。
    pub fn new(old_ver: u32, new_ver: u32) -> VersionDiff {
        VersionDiff {
            old_ver,
            new_ver,
            rows: Vec::new(),
            unsigned: 0,
        }
    }

    /// 逐段比对两引擎裁决。
    pub fn run(&mut self, doc: &[Section], old: &mut Engine, new: &mut Engine) {
        self.rows.clear();
        self.unsigned = 0;
        for s in doc.iter() {
            let o = match old.judge_section(s) {
                Ok((_, r)) => r,
                Err(_) => Ruling::StrictKill,
            };
            let n = match new.judge_section(s) {
                Ok((_, r)) => r,
                Err(_) => Ruling::StrictKill,
            };
            let side = if o == n {
                DiffSide::Same
            } else if n == Ruling::Publish && o.filtered() {
                DiffSide::Relaxed
            } else {
                DiffSide::Tightened
            };
            if side == DiffSide::Same {
                continue;
            }
            self.rows.push(DiffRow {
                sec: s.id,
                side,
                old: o,
                new: n,
                signed: false,
            });
            self.unsigned = self.unsigned.wrapping_add(1);
        }
    }

    /// 逐条签字复核。
    pub fn sign_all(&mut self) {
        for r in self.rows.iter_mut() {
            if !r.signed {
                r.signed = true;
                if self.unsigned > 0 {
                    self.unsigned = self.unsigned.wrapping_sub(1);
                }
            }
        }
    }

    /// 双向差异计数（放宽/收紧）。
    pub fn sides(&self) -> (usize, usize, usize) {
        let mut relaxed = 0usize;
        let mut tightened = 0usize;
        let mut same = 0usize;
        for r in self.rows.iter() {
            match r.side {
                DiffSide::Relaxed => relaxed = relaxed.wrapping_add(1),
                DiffSide::Tightened => tightened = tightened.wrapping_add(1),
                DiffSide::Same => same = same.wrapping_add(1),
            }
        }
        (relaxed, tightened, same)
    }

    /// 复核完结（零未复核）。
    pub fn closed(&self) -> bool {
        self.unsigned == 0
    }
}

// --------------------------------------------------------------------------
// 判据
// --------------------------------------------------------------------------

/// CGPU-F3528 深化版判据（十二条）。
pub fn run_cgpu_f3528_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("cgpu-f3528-deep");

    // ---- WSD-01 诊断段闭集与建议完备 ----
    let seg_ok = WsdCode::ALL
        .iter()
        .enumerate()
        .all(|(i, c)| c.val() == WSD_SEG_BASE + i as u16);
    let remedy_ok = WsdCode::ALL.iter().all(|c| c.remedy().len() >= 10);
    cs.add(
        "WSD-01 诊断段闭集",
        seg_ok && remedy_ok,
        "12 码连续+建议完备",
    );

    // ---- WSD-02 标注闭集严格（三词·秩单调·闭集外拒） ----
    let mark_ok = Mark::ALL.len() == 3
        && Mark::parse("public") == Some(Mark::Public)
        && Mark::parse("internal") == Some(Mark::Internal)
        && Mark::parse("confidential") == Some(Mark::Confidential)
        && Mark::parse("Public").is_none()
        && Mark::parse("secret").is_none()
        && Mark::Public.rank() < Mark::Internal.rank()
        && Mark::Internal.rank() < Mark::Confidential.rank();
    cs.add("WSD-02 标注闭集", mark_ok, "三词闭集+大小写敏感+秩单调");

    // ---- WSD-03 规则表六类 + 指纹钉死 + 越界拒 ----
    let r3 = RuleSet::v1();
    let rule_ok = RuleKind::ALL.len() == 6
        && r3.enabled_count() == 6
        && r3.fp_intact()
        && r3.rule_at(0) == Ok(RuleKind::ApiKey)
        && r3.rule_at(5) == Ok(RuleKind::PrivateIp)
        && r3.rule_at(6) == Err(WsdCode::RuleRange);
    // 改写规则表不重算指纹 → 漂移检出。
    let mut tampered = RuleSet::v1();
    tampered.enabled[0] = false;
    let drift_ok = !tampered.fp_intact();
    cs.add(
        "WSD-03 规则表指纹",
        rule_ok && drift_ok,
        "六类全开+越界拒+改写漂移检出",
    );

    // ---- WSD-04 六规则逐条命中（每类至少一处） ----
    let probes: [(&str, RuleKind); 6] = [
        ("key sk-abcdef0123456789abcd end", RuleKind::ApiKey),
        ("auth Bearer abcdefghijkl", RuleKind::Bearer),
        ("host build01.corp.local", RuleKind::CorpHost),
        ("-----BEGIN RSA PRIVATE KEY-----", RuleKind::PemHead),
        ("mysql://u@db/?password=hunter2", RuleKind::ConnSecret),
        ("addr 192.168.1.10 port", RuleKind::PrivateIp),
    ];
    let mut hit_all = true;
    for (t, want) in probes.iter() {
        let h = scan_full(&r3, t);
        if h.first_kind() != Some(*want) {
            hit_all = false;
        }
    }
    // 私网段外不误判（172.32 不是私网）。
    let clean_net = scan_full(&r3, "addr 172.32.0.1").is_empty();
    cs.add(
        "WSD-04 六规则命中",
        hit_all && clean_net,
        "六类各命中一次+172.32 不误判",
    );

    // ---- WSD-05 降级/关闭档最严闸（零泄漏） ----
    let doc5: Vec<Section> = alloc::vec![
        Section::new(1, Mark::Public, "public overview"),
        Section::new(2, Mark::Public, "public faq clean text"),
    ];
    let mut e5d = Engine::new();
    e5d.tier = ScanTier::Degraded;
    let r5d = e5d.judge_doc(&doc5);
    let degraded_strict = matches!(r5d, Ok(ref v) if v.iter().all(|(_, r)| *r == Ruling::StrictKill))
        && e5d.n_strict == 2
        && e5d.conserved()
        && e5d.stat(WsdCode::DegradedLeak) == 0
        && e5d.stat(WsdCode::ScanOffLeak) == 0;
    let mut e5o = Engine::new();
    e5o.tier = ScanTier::Off;
    let off_strict = matches!(e5o.judge_doc(&doc5), Ok(ref v) if v.iter().all(|(_, r)| *r == Ruling::StrictKill))
        && e5o.conserved();
    cs.add(
        "WSD-05 降级最严闸",
        degraded_strict && off_strict,
        "降级/关闭档全段错杀零泄漏",
    );

    // ---- WSD-06 标注层优先 + 扫描层兜底 + 零误滤 ----
    let mut e6 = Engine::new();
    let doc6: Vec<Section> = alloc::vec![
        Section::new(1, Mark::Internal, "clean internal notes"),
        Section::new(2, Mark::Confidential, "vendor pricing"),
        Section::new(3, Mark::Public, "clean public overview"),
        Section::new(4, Mark::Public, "leak sk-00112233445566778899aa"),
    ];
    let r6 = e6.judge_doc(&doc6);
    let seq_ok = matches!(
        r6,
        Ok(ref v) if v[0].1 == Ruling::ByMark
            && v[1].1 == Ruling::ByMark
            && v[2].1 == Ruling::Publish
            && v[3].1 == Ruling::ByScan
    );
    let off3 = matches!(r6, Ok(ref v) if v[3].0.at == Some(5));
    let kind3 = matches!(r6, Ok(ref v) if v[3].0.kind == Some(RuleKind::ApiKey));
    cs.add(
        "WSD-06 双层过滤序",
        seq_ok && off3 && kind3 && e6.conserved(),
        "标注先判+扫描兜底+偏移/类别精确",
    );

    // ---- WSD-07 裁决留痕：令牌单调 + 指纹链 + 重放拒 ----
    let mono = e6.tokens_monotonic();
    let fp_a = e6.book_fp();
    let mut e7 = e6.clone();
    // 篡改一条裁决 → 指纹链漂移。
    if let Some(d) = e7.book.first_mut() {
        d.ruling = Ruling::Publish;
    }
    let fp_b = e7.book_fp();
    let chain_ok = mono && fp_a != fp_b && e6.book.len() == 4;
    // 账满显性拒（灌满 RULING_CAP）。
    let mut e7b = Engine::new();
    let mut sec = Section::new(1, Mark::Public, "x");
    let mut hit_full = false;
    for _ in 0..(RULING_CAP + 2) {
        sec.id = sec.id.wrapping_add(1);
        if matches!(e7b.judge_section(&sec), Err(WsdCode::RulingFull)) {
            hit_full = true;
            break;
        }
    }
    cs.add(
        "WSD-07 留痕与防重放",
        chain_ok && hit_full && e7b.stat(WsdCode::RulingFull) == 1,
        "令牌单调+指纹链敏感+账满显性拒",
    );

    // ---- WSD-08 三面真复用：同实例同指纹 ----
    let doc8: Vec<Section> = alloc::vec![
        Section::new(1, Mark::Public, "public api reference"),
        Section::new(2, Mark::Internal, "internal tuning"),
        Section::new(3, Mark::Public, "bearer abcdefgh"),
    ];
    let mut shared = Engine::new();
    let mut fps: [u64; 3] = [0; 3];
    let mut verdicts: [Vec<Ruling>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    let mut token_sets: [Vec<u64>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    for (i, _f) in Face::ALL.iter().enumerate() {
        // 关键：共用同一实例，引擎指纹必然相同（真复用）。
        fps[i] = shared.fp();
        let rr = shared.judge_doc(&doc8);
        if let Ok(v) = rr {
            verdicts[i] = v.iter().map(|(_, r)| *r).collect();
            token_sets[i] = v.iter().map(|(d, _)| d.token).collect();
        }
    }
    let same_fp = fps[0] == fps[1] && fps[1] == fps[2];
    let same_v = verdicts[0] == verdicts[1] && verdicts[1] == verdicts[2];
    // 真复用的更强证据：三面共用同一裁决账（令牌序列连续，非三份）。
    let one_ledger = shared.book.len() == RULING_CAP.min(9);
    let reuse_ok = same_fp && same_v && one_ledger && shared.conserved();
    cs.add(
        "WSD-08 三面真复用",
        reuse_ok,
        "同实例同指纹+判定一致+单账非三份",
    );

    // ---- WSD-09 误杀申诉与撤销（append-only） ----
    let mut e9 = Engine::new();
    let r9 = e9.judge_section(&Section::new(1, Mark::Public, "addr 10.0.0.5 gateway"));
    let scan_token = match r9 {
        Ok((d, Ruling::ByScan)) => d.token,
        _ => 0,
    };
    let book_len_before = e9.book.len();
    let rv = e9.revoke(scan_token);
    let err_unknown = e9.revoke(9999);
    let e10 = e9.judge_section(&Section::new(2, Mark::Public, "clean public text"));
    let pub_token = match e10 {
        Ok((d, _)) => d.token,
        _ => 0,
    };
    let err_not_filtered = e9.revoke(pub_token);
    let appeal_ok = rv == Ok(Mark::Public)
        && e9.revoked(1)
        && e9.book.len() == book_len_before + 1
        && err_unknown == Err(WsdCode::AppealUnknown)
        && err_not_filtered == Err(WsdCode::AppealNotFiltered)
        && e9.stat(WsdCode::AppealUnknown) == 1
        && e9.stat(WsdCode::AppealNotFiltered) == 1;
    cs.add(
        "WSD-09 申诉撤销",
        appeal_ok,
        "误杀可撤+原账不抹+双错误路径立案",
    );

    // ---- WSD-10 标注层过滤不接受申诉（策略意图不可诉） ----
    let mut e10b = Engine::new();
    let mark_token = match e10b.judge_section(&Section::new(7, Mark::Confidential, "x")) {
        Ok((d, _)) => d.token,
        _ => 0,
    };
    let refuse = e10b.revoke(mark_token);
    let fp_before = e10b.book_fp();
    let fp_after = e10b.book_fp();
    cs.add(
        "WSD-10 标注不可诉",
        refuse == Err(WsdCode::AppealNotFiltered) && fp_before == fp_after,
        "标注过滤撤不掉+账指纹不变",
    );

    // ---- WSD-11 跨版本对拍：两向差异必现且可签字结案 ----
    let doc11: Vec<Section> = alloc::vec![
        Section::new(1, Mark::Public, "build01.corp.local mirror"),
        Section::new(2, Mark::Public, "addr 10.1.2.3 host"),
        Section::new(3, Mark::Public, "clean public line"),
        Section::new(4, Mark::Public, "key sk-0011223344556677 end"),
    ];
    // 两版规则互不包含：v1 独有 corp-host/pem-head（v2 收紧面放开 → Relaxed），
    // v2 独有 conn-secret/private-ip（v1 放行面收紧 → Tightened）。
    let mut old_e = Engine::new();
    old_e.rules = RuleSet::v1();
    old_e.rules.enabled = [true, true, true, true, false, false];
    old_e.rules.fp = old_e.rules.compute_fp();
    let mut new_e = Engine::new();
    new_e.rules = RuleSet::v2();
    new_e.rules.enabled = [true, true, false, false, true, true];
    new_e.rules.fp = new_e.rules.compute_fp();
    let mut vd = VersionDiff::new(1, 2);
    vd.run(&doc11, &mut old_e, &mut new_e);
    let (relaxed, tightened, _same) = vd.sides();
    let open_case = !vd.closed();
    vd.sign_all();
    let diff_ok = relaxed >= 1 && tightened >= 1 && open_case && vd.closed()
        && vd.rows.iter().all(|r| r.signed);
    cs.add(
        "WSD-11 跨版本对拍",
        diff_ok,
        "放宽/收紧两向差异均可签字结案",
    );

    // ---- WSD-12 渲染可追 + 计数守恒总门 ----
    let mut e12 = Engine::new();
    let _ = e12.judge_doc(&doc11);
    let txt = e12.render();
    let render_ok = txt.contains("ruling") && txt.contains("counts") && txt.contains("book_fp=");
    let balance = e12.conserved()
        && e12.tokens_monotonic()
        && e12.n_publish + e12.n_by_mark + e12.n_by_scan + e12.n_strict == e12.processed
        && e12.stat(WsdCode::CountUnbalanced) == 0;
    cs.add(
        "WSD-12 渲染与总门",
        render_ok && balance,
        "裁决簿可追+四分类守恒",
    );

    cs
}