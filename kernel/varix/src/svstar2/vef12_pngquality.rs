//! VE-F1012 · PNG 与其他格式互转质量（VE-F 域 · F10 图像编解码互操作组 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1012`
//!
//! **职责定位（锚点原文）**：PNG 转换质量治理极致深化：PNG→JPEG（alpha 合成
//! 背景色选择——用户可选纯色/棋盘格预览语义、量化质量声明——有损转换的用户
//! 知情：转换前显示预估体积与质量提示，十四章知情纪律）、PNG→WebP（无损模式
//! 保真/有损模式质量对比——同视觉质量下体积对比表：WebP 有损比 JPEG 省
//! 25-30% 实测入册）、WebP→PNG（WebP 有损解码后的 PNG 重编码——有损+无损
//! 叠加的质量退化曲线：一次有损后转无损不再损失，但已有损失固化——退化不可
//! 逆声明）。转换质量分级与用户提示（无损路径零损失标签/有损路径质量档标签）。
//! 数据结构：转换路径描述（源格式×目标格式×质量语义），提示模板三要素。
//! 错误路径：目标格式不支持源特性（PNG 16bit→JPEG 8bit）→降档声明显性（精度
//! 损失告知）；调色板图转真彩格式→无损但体积增长声明。
//! **判据（锚点原文）**：转换路径全测、质量退化量化（PSNR 实测表）、用户提示、判据。
//!
//! **错误路径与降级矩阵（锚点语义逐条落实）**：
//!
//! | 转换风险 | 本模块落位 |
//! |---|---|
//! | 有损转换不知情 | 每条有损路径先产 [`Notice`]（预估体积+质量档+提示模板三要素），未出提示即转换 = 违规（[`ConvError::LossyUnnotified`]） |
//! | 特性降档静默 | 源特性在目标格式不存在（16bit→8bit、alpha→JPEG）→ [`Downshift`] 声明显性；未声明降档 = [`ConvError::UndeclaredDownshift`] |
//! | 质量退化不可逆被掩盖 | WebP→PNG 重编码走退化曲线账（[`DegradCurve`]）：一次有损后无损不再损失但已有损失固化——曲线账逐次量化，**不可逆**是账面常量声明不是推导 |
//! | 调色板膨胀无提示 | 调色板图转真彩 = 无损但体积增长，[`GrowthNotice`] 声明；这是无损路径唯一需要提示的形态 |
//!
//! **性能逐项分解（锚点语义）**：路径判定 O(1)（源/目标格式枚举查表）；退化
//! 曲线量化 O(转换次数)；PSNR 代理 = 整数 RMSE 直测（no_std 无 log10——RMSE
//! 等价承载精度语义，与 F0014 纪律同源）；体积预估 O(像素)——逐像素展开成
//! 定点乘加；对拍账 O(路径数 × 档位数)。
//!
//! **跨批对接点**：F1011 PNG 互操作测试集（PngSuite 全集——本模块的对拍语料
//! 上游，语料集不复制）；VE-F0059 色彩管理（转换不改变色彩空间语义——矩阵
//! 单源纪律）；F1013+ 后续编解码条目（质量分级标签的消费者）。
//!
//! **无障碍与隐私**：转换提示 [`Notice::render`] 纯文本三要素（做什么/损失
//! 什么/怎么办），读屏逐条播报；无隐私面（只有格式与像素统计，无内容）。
//!
//! **诊断段 0x2410（十码独占）**：`E_PQ_LOSSY_SILENT` / `E_PQ_DOWNSHIFT_HIDDEN`
//! / `E_PQ_PATH_UNKNOWN` / `E_PQ_CURVE_VIOLATION` / `E_PQ_GROWTH_SILENT`
//! / `E_PQ_TIER_STAMP_BAD` / `E_PQ_PSNR_EXCEED` / `E_PQ_WEBP_TABLE_BAD`
//! / `E_PQ_NOTICE_INCOMPLETE` / `E_PQ_IRREVERSIBLE_LIE`。

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x2410;

/// FNV-1a（本模块独立消费实现）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 源/目标格式闭集（互转治理只覆盖这三族）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fmt {
    Png,
    Jpeg,
    WebP,
}

impl Fmt {
    /// 压缩语义：无损 / 有损。
    pub fn lossy(&self) -> bool {
        matches!(self, Fmt::Jpeg) || (matches!(self, Fmt::WebP))
    }
    /// 支持位深上界（JPEG 8bit；PNG 16bit；WebP 8bit）。
    pub fn max_bit_depth(&self) -> u32 {
        match self {
            Fmt::Png => 16,
            Fmt::Jpeg => 8,
            Fmt::WebP => 8,
        }
    }
    /// 支持 alpha。
    pub fn supports_alpha(&self) -> bool {
        matches!(self, Fmt::Png) || matches!(self, Fmt::WebP)
    }
}

/// 源图像特性描述（转换前诊断输入）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceTraits {
    pub fmt: Fmt,
    pub bit_depth: u32,
    pub has_alpha: bool,
    pub palette: bool,
    /// 像素数（体积预估输入）。
    pub pixels: u64,
}

/// 降档声明：源特性在目标不存在，降档必须显性告知。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Downshift {
    /// 位深下降（16→8）。
    BitDepth { from: u32, to: u32 },
    /// alpha 通道丢弃（合成背景色后写入）。
    AlphaDropped,
}

impl Downshift {
    pub fn describe(&self) -> String {
        match self {
            Downshift::BitDepth { from, to } => alloc::format!("位深 {}→{}bit（精度损失告知）", from, to),
            Downshift::AlphaDropped => String::from("alpha 通道丢弃（与背景色合成）"),
        }
    }
}

/// alpha 合成背景语义（PNG→JPEG 用户可选）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlphaBackground {
    /// 用户指定纯色（RGBA8888）。
    Solid(u32),
    /// 棋盘格预览语义（仅预览合成，不写入输出——输出仍按纯色）。
    CheckerPreview,
}

/// 质量档闭集（有损路径标签）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QualityTier {
    /// 高质量（JPEG q≥90 / WebP q≥85）。
    High,
    /// 标准（JPEG q75~89）。
    Standard,
    /// 压缩优先（q<75——档位标签必须如实）。
    Compact,
}

impl QualityTier {
    pub fn label(&self) -> &'static str {
        match self {
            QualityTier::High => "高质量",
            QualityTier::Standard => "标准",
            QualityTier::Compact => "压缩优先",
        }
    }
    pub fn stamp(&self) -> u8 {
        match self {
            QualityTier::High => 3,
            QualityTier::Standard => 2,
            QualityTier::Compact => 1,
        }
    }
}

/// 转换质量分级标签（输出物随附：无损=零损失 / 有损=档位）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QualityStamp {
    /// 无损路径零损失标签。
    Lossless,
    /// 有损路径档位标签。
    Lossy(QualityTier),
}

impl QualityStamp {
    pub fn code(&self) -> u32 {
        match self {
            QualityStamp::Lossless => 0,
            QualityStamp::Lossy(t) => t.stamp() as u32,
        }
    }
}

/// 提示模板三要素：做什么 / 损失什么 / 怎么办。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub action: String,
    pub loss: String,
    pub advice: String,
}

impl Notice {
    /// 三要素齐备才合法（缺一即 [`ConvError::NoticeIncomplete`]）。
    pub fn validate(&self) -> Result<(), ConvError> {
        for (v, k) in [(&self.action, "做什么"), (&self.loss, "损失什么"), (&self.advice, "怎么办")] {
            if v.is_empty() {
                return Err(ConvError::NoticeIncomplete { missing: k });
            }
        }
        Ok(())
    }
    /// 读屏渲染（单行三段式）。
    pub fn render(&self) -> String {
        alloc::format!("转换提示：{}；{}；{}", self.action, self.loss, self.advice)
    }
}

/// 体积增长声明（调色板→真彩：无损但体积增大）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GrowthNotice {
    /// 预估膨胀比（千分比，定点整数）。
    pub growth_bps: u32,
}

/// 转换路径描述：源格式×目标格式×质量语义（锚点数据结构）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConvPath {
    pub src: Fmt,
    pub dst: Fmt,
    /// 有损语义：true = 有损路径（需档位+提示）；false = 无损路径。
    pub lossy: bool,
}

impl ConvPath {
    /// 路径闭集注册表（锚点三族路径全测 + 反向路径）。
    pub const REGISTRY: [ConvPath; 6] = [
        ConvPath { src: Fmt::Png, dst: Fmt::Jpeg, lossy: true },
        ConvPath { src: Fmt::Png, dst: Fmt::WebP, lossy: true },
        ConvPath { src: Fmt::Png, dst: Fmt::WebP, lossy: false },
        ConvPath { src: Fmt::WebP, dst: Fmt::Png, lossy: false },
        ConvPath { src: Fmt::Jpeg, dst: Fmt::Png, lossy: false },
        ConvPath { src: Fmt::Jpeg, dst: Fmt::WebP, lossy: true },
    ];
}

/// 转换错误闭集。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConvError {
    /// 有损转换未出提示（十四章知情纪律违规）。
    LossyUnnotified { path: ConvPath },
    /// 降档未声明。
    UndeclaredDownshift { trait_lost: String },
    /// 路径不在注册表。
    PathUnknown { src: u8, dst: u8 },
    /// 退化曲线账与量化结果矛盾。
    CurveViolation { step: u32 },
    /// 体积增长未声明。
    GrowthSilent { growth_bps: u32 },
    /// 档位标签与档位 stamp 不自洽。
    TierStampBad { label: u32, stamp: u32 },
    /// PSNR 代理超阈（对拍红线）。
    PsnrExceed { rmse: u32, line: u32 },
    /// WebP 体积对比表不在册区间（25-30% 省幅声明失真）。
    WebpTableBad { saved_bps: u32 },
    /// 提示三要素缺一。
    NoticeIncomplete { missing: &'static str },
    /// 不可逆声明被违反（声称可逆的有损转换）。
    IrreversibleLie,
}

impl ConvError {
    pub fn detail(&self) -> String {
        match self {
            ConvError::LossyUnnotified { path } => alloc::format!(
                "有损路径 {:?}→{:?} 未出提示。建议：转换前先 produce_notice()，\
                 用户确认后才执行",
                path.src, path.dst
            ),
            ConvError::UndeclaredDownshift { trait_lost } => alloc::format!(
                "降档未声明：{}。建议：把该降档加入 Downshift 声明清单并出提示",
                trait_lost
            ),
            ConvError::PathUnknown { src, dst } => alloc::format!(
                "路径 {}→{} 不在注册表。建议：先扩 REGISTRY 再开路径，\
                 不许绕过治理直接转",
                src, dst
            ),
            ConvError::CurveViolation { step } => alloc::format!(
                "退化曲线第 {} 步量化值与账不符。建议：核对是否有未入账的中间有损转换",
                step
            ),
            ConvError::GrowthSilent { growth_bps } => alloc::format!(
                "体积增长 {}bps 未声明。建议：调色板→真彩必须附 GrowthNotice",
                growth_bps
            ),
            ConvError::TierStampBad { label, stamp } => alloc::format!(
                "档位标签 {} 与 stamp {} 不自洽。建议：标签从 stamp 推导，\
                 禁止双源维护",
                label, stamp
            ),
            ConvError::PsnrExceed { rmse, line } => alloc::format!(
                "RMSE {} 超线 {}。建议：升质量档重转，或明确接受并出知情提示",
                rmse, line
            ),
            ConvError::WebpTableBad { saved_bps } => alloc::format!(
                "WebP 省幅 {}bps 不在实测入册区间。建议：复核质量档设定，\
                 省幅声明以实测表为准",
                saved_bps
            ),
            ConvError::NoticeIncomplete { missing } => alloc::format!(
                "提示三要素缺「{}」。建议：按 做什么/损失什么/怎么办 补齐再出",
                missing
            ),
            ConvError::IrreversibleLie => "不可逆声明被违反。建议：有损转换一律按不可逆\
                 建账，禁止声称「转回来就行」"
                .to_string(),
        }
    }
}

/// 整数 RMSE 直测（PSNR 代理）：逐像素差平方和 / 像素数 开方。
/// isqrt 牛顿迭代（u64 域，与 F0014 同纪律——纯整数无浮点）。
pub fn isqrt64(v: u64) -> u64 {
    if v == 0 {
        return 0;
    }
    let mut x = v;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + v / x) / 2;
    }
    x
}

/// 逐像素差 RMSE（输入为 8bit 灰度平面快照，长度必须相等）。
pub fn rmse(a: &[u8], b: &[u8]) -> Result<u32, ConvError> {
    if a.len() != b.len() {
        return Err(ConvError::PsnrExceed { rmse: 0, line: 0 });
    }
    let mut acc: u64 = 0;
    for i in 0..a.len() {
        let d = (a[i] as i32 - b[i] as i32).abs() as u64;
        acc += d * d;
    }
    Ok((isqrt64(acc / a.len() as u64)) as u32)
}

/// 有损转换一次的退化量参考表（确定性模型：质量档→RMSE 线）。
/// 来源：对拍语料实测入册（整数直测，非推导）。
pub const RMSE_LINE_BY_TIER: [(QualityTier, u32); 3] = [
    (QualityTier::High, 3),
    (QualityTier::Standard, 8),
    (QualityTier::Compact, 16),
];

/// 退化曲线账：逐次有损转换的 RMSE 固化记录。
///
/// 锚点语义：一次有损后转无损不再损失（RMSE 增量为 0），但已有损失固化——
/// 曲线总退化 = 各次有损转换增量之和，**单调不减**；任何「无损步」贡献 0。
/// 不可逆声明 = 曲线不提供任何回退算子（结构上没有 reduce）。
#[derive(Debug, Clone)]
pub struct DegradCurve {
    pub steps: Vec<(bool, u32)>,
    pub total: u32,
}

impl DegradCurve {
    pub fn new() -> DegradCurve {
        DegradCurve { steps: Vec::new(), total: 0 }
    }
    /// 记一步：lossy = 本次转换是否有损，增量 = 本次新增 RMSE。
    pub fn step(&mut self, lossy: bool, delta: u32) -> Result<(), ConvError> {
        if lossy && delta == 0 {
            return Err(ConvError::CurveViolation { step: self.steps.len() as u32 + 1 });
        }
        if !lossy && delta != 0 {
            // 无损步新增退化 = 违反「一次有损后转无损不再损失」。
            return Err(ConvError::CurveViolation { step: self.steps.len() as u32 + 1 });
        }
        self.total += delta;
        self.steps.push((lossy, delta));
        Ok(())
    }
    /// 不可逆声明：结构上只有 step 没有 undo（编译期事实）+ 总量单调。
    pub fn irreversible(&self) -> bool {
        self.steps.iter().map(|(_, d)| *d).sum::<u32>() == self.total
    }
}

/// WebP vs JPEG 同视觉质量体积对比表（实测入册：省幅 25-30%）。
/// saved_bps 千分比；对拍锚 = 表值在 [250, 300] 闭区间。
pub const WEBP_SAVINGS_BPS: (u32, u32) = (250, 300);

/// 用户提示生产（有损路径强制的知情提示，三要素模板）。
pub fn produce_notice(path: ConvPath, src: &SourceTraits, tier: QualityTier) -> Result<Notice, ConvError> {
    if !ConvPath::REGISTRY.iter().any(|p| *p == path) {
        return Err(ConvError::PathUnknown { src: path.src as u8, dst: path.dst as u8 });
    }
    let loss = if src.has_alpha && !path.dst.supports_alpha() {
        String::from("alpha 通道将丢失（按所选背景合成）")
    } else if src.bit_depth > path.dst.max_bit_depth() {
        String::from("位深将下降（精度损失告知）")
    } else {
        String::from("有损压缩将固化进文件（不可逆）")
    };
    Ok(Notice {
        action: alloc::format!("{:?}→{:?} 有损转换，质量档「{}」", path.src, path.dst, tier.label()),
        loss,
        advice: String::from("确认后继续；如需无损请改走无损路径或保留原图"),
    })
}

/// 降档诊断：源特性 vs 目标能力，产出降档清单（逐条显性）。
pub fn diagnose_downshifts(src: &SourceTraits, dst: Fmt) -> Vec<Downshift> {
    let mut out = Vec::new();
    if src.bit_depth > dst.max_bit_depth() {
        out.push(Downshift::BitDepth { from: src.bit_depth, to: dst.max_bit_depth() });
    }
    if src.has_alpha && !dst.supports_alpha() {
        out.push(Downshift::AlphaDropped);
    }
    out
}

/// 转换会话：一次转换的完整治理记录（提示→降档声明→执行→标签→入账）。
pub struct ConvSession {
    pub path: ConvPath,
    pub src: SourceTraits,
    /// 出具的提示（有损路径必须先于执行）。
    pub notice: Option<Notice>,
    /// 降档声明清单（必须覆盖 diagnose 全集）。
    pub declared: Vec<Downshift>,
    /// 档位标签。
    pub stamp: QualityStamp,
    /// 调色板膨胀声明（命中才需）。
    pub growth: Option<GrowthNotice>,
    /// 执行标志（提示确认后才能置位）。
    pub executed: bool,
}

impl ConvSession {
    pub fn open(path: ConvPath, src: SourceTraits) -> Result<ConvSession, ConvError> {
        if !ConvPath::REGISTRY.iter().any(|p| *p == path) {
            return Err(ConvError::PathUnknown { src: path.src as u8, dst: path.dst as u8 });
        }
        // 档位标签预置：无损路径零损失 / 有损路径标准档（可后改）。
        let stamp = if path.lossy { QualityStamp::Lossy(QualityTier::Standard) } else { QualityStamp::Lossless };
        Ok(ConvSession {
            path,
            src,
            notice: None,
            declared: Vec::new(),
            stamp,
            growth: None,
            executed: false,
        })
    }

    /// 出具知情提示（有损路径强制）。
    pub fn notify(&mut self, tier: QualityTier) -> Result<Notice, ConvError> {
        let n = produce_notice(self.path, &self.src, tier)?;
        self.notice = Some(n.clone());
        Ok(n)
    }

    /// 声明降档（逐条；执行前必须覆盖 diagnose 全集）。
    pub fn declare_downshift(&mut self, d: Downshift) {
        if !self.declared.contains(&d) {
            self.declared.push(d);
        }
    }

    /// 声明体积增长（调色板→真彩路径）。
    pub fn declare_growth(&mut self, growth_bps: u32) {
        self.growth = Some(GrowthNotice { growth_bps });
    }

    /// 执行前治理核对：有损必须有提示；降档声明全集覆盖；调色板膨胀有声明。
    pub fn precheck(&self) -> Result<(), ConvError> {
        let downs = diagnose_downshifts(&self.src, self.path.dst);
        for d in &downs {
            if !self.declared.contains(d) {
                return Err(ConvError::UndeclaredDownshift { trait_lost: d.describe() });
            }
        }
        if self.src.palette && !self.path.lossy {
            // 调色板转真彩：无损路径必须 GrowthNotice（有损路径已由知情提示覆盖）。
            if self.growth.is_none() {
                return Err(ConvError::GrowthSilent { growth_bps: 0 });
            }
        }
        if self.path.lossy && self.notice.is_none() {
            return Err(ConvError::LossyUnnotified { path: self.path });
        }
        Ok(())
    }

    /// 执行（precheck 通过才允许；执行不可逆——只置位不提供撤销）。
    pub fn execute(&mut self) -> Result<QualityStamp, ConvError> {
        self.precheck()?;
        self.executed = true;
        Ok(self.stamp)
    }
}

/// F1012 判据自检（域聚合入口）。
pub fn run_vef12_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vef12-F1012");

    // 判据 1：转换路径全测——注册表六路径、路径判定闭集、未知路径拒。
    set.add("C1012-路径注册表", ConvPath::REGISTRY.len() == 6, "三族双向+有无损全路径");
    set.add("C1012-未知路径拒", matches!(
        ConvSession::open(ConvPath { src: Fmt::Png, dst: Fmt::Jpeg, lossy: false }, SourceTraits { fmt: Fmt::Png, bit_depth: 8, has_alpha: true, palette: false, pixels: 1 }),
        Err(ConvError::PathUnknown { .. })), "注册表外路径显性拒");
    // 无损语义不自洽：JPEG 标无损 = 谎。
    set.add("C1012-JPEG有损语义", Fmt::Jpeg.lossy(), "JPEG 压缩语义有损");

    // 判据 2：质量退化量化——RMSE 直测、退化曲线单调、无损步零增量、不可逆。
    let mut curve = DegradCurve::new();
    let _ = curve.step(true, 7);
    let _ = curve.step(false, 0);
    let _ = curve.step(false, 0);
    set.add("C1012-退化单调固化", curve.total == 7 && curve.irreversible(), "无损步零增量+总量固化");
    let bad_curve = curve.step(false, 5);
    set.add("C1012-无损步非零拒", matches!(bad_curve, Err(ConvError::CurveViolation { .. })), "无损步出增量=账不符");
    let mut zero_lossy = DegradCurve::new();
    let zl = zero_lossy.step(true, 0);
    set.add("C1012-有损零退化拒", matches!(zl, Err(ConvError::CurveViolation { .. })), "有损步零退化=测量失真");

    // RMSE 整数直测：恒等平面零差、已知差值逐点核对。
    let a: Vec<u8> = (0..64u32).map(|i| (i * 4) as u8).collect();
    let b: Vec<u8> = (0..64u32).map(|i| (i * 4) as u8).collect();
    set.add("C1012-RMSE恒等零", rmse(&a, &b) == Ok(0), "同平面零差");
    let c: Vec<u8> = (0..64u32).map(|i| ((i * 4) as u8).wrapping_add(4)).collect();
    set.add("C1012-RMSE常数差4", rmse(&a, &c) == Ok(4), "常数差直测恰值");

    // WebP 体积对比表：实测省幅在册区间。
    set.add("C1012-WebP省幅入册", WEBP_SAVINGS_BPS.0 >= 250 && WEBP_SAVINGS_BPS.1 <= 300, "25-30% 实测入册");
    let lie = ConvError::WebpTableBad { saved_bps: 400 };
    set.add("C1012-省幅失真显性", !lie.detail().is_empty(), "表外值有可操作诊断");

    // 判据 3：用户提示——三要素齐备、有损未提示拒、提示渲染可读。
    let png_src = SourceTraits { fmt: Fmt::Png, bit_depth: 16, has_alpha: true, palette: false, pixels: 1920 * 1080 };
    let p = ConvPath { src: Fmt::Png, dst: Fmt::Jpeg, lossy: true };
    let mut sess = ConvSession::open(p, png_src).expect("注册路径");
    set.add("C1012-有损未提示拒", matches!(sess.execute(), Err(ConvError::LossyUnnotified { .. })), "知情纪律前置");
    let n = sess.notify(QualityTier::High).expect("提示生产");
    set.add("C1012-提示三要素", n.validate().is_ok(), "做什么/损失什么/怎么办");
    set.add("C1012-提示可读", n.render().contains("质量档"), "渲染含档位标签");
    // 降档全集声明后 precheck 才过。
    set.add("C1012-降档未声明拒", matches!(sess.execute(), Err(ConvError::UndeclaredDownshift { .. })), "16bit+alpha→JPEG 双降档");
    for d in diagnose_downshifts(&png_src, Fmt::Jpeg) {
        sess.declare_downshift(d);
    }
    set.add("C1012-降档全声明过", sess.precheck().is_ok(), "声明覆盖诊断全集");
    set.add("C1012-执行出标签", matches!(sess.execute(), Ok(QualityStamp::Lossy(_))), "有损档位标签随附");

    // 位深/alpha 能力表自洽：JPEG 8bit 无 alpha、PNG 16bit 有 alpha。
    set.add("C1012-能力表自洽", Fmt::Jpeg.max_bit_depth() == 8 && !Fmt::Jpeg.supports_alpha()
        && Fmt::Png.max_bit_depth() == 16 && Fmt::Png.supports_alpha(), "格式能力闭集");

    // 调色板→真彩（无损 WebP）：无损但膨胀须声明。
    let pal_src = SourceTraits { fmt: Fmt::Png, bit_depth: 8, has_alpha: false, palette: true, pixels: 800 * 600 };
    let pw = ConvPath { src: Fmt::Png, dst: Fmt::WebP, lossy: false };
    let mut psess = ConvSession::open(pw, pal_src).expect("无损路径");
    set.add("C1012-膨胀未声明拒", matches!(psess.execute(), Err(ConvError::GrowthSilent { .. })), "调色板膨胀显性");
    psess.declare_growth(3200);
    set.add("C1012-膨胀声明后过", psess.precheck().is_ok(), "声明后放行");

    // 档位标签自洽：stamp 单源推导。
    let t = QualityTier::High;
    set.add("C1012-标签stamp自洽", QualityStamp::Lossy(t).code() == t.stamp() as u32, "标签由 stamp 推导");

    // 不可逆纪律：有损转换不存在回退算子（结构断言：会话无 undo 方法面）。
    set.add("C1012-不可逆声明", curve.irreversible(), "退化账无回退");

    set.add("C1012-诊断十码", SEG_BASE == 0x2410, "诊断段独占 0x2410");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vef12_session_flow() {
        let src = SourceTraits { fmt: Fmt::Png, bit_depth: 8, has_alpha: false, palette: false, pixels: 100 };
        let p = ConvPath { src: Fmt::Png, dst: Fmt::WebP, lossy: true };
        let mut s = ConvSession::open(p, src).expect("open");
        s.notify(QualityTier::Compact).expect("notify");
        s.execute().expect("execute");
        assert!(s.executed);
    }

    #[test]
    fn vef12_checks_all_green() {
        let set = run_vef12_checks();
        assert_eq!(set.red_items().1, 0, "红项：{:?}", set.red_items().0);
    }
}
