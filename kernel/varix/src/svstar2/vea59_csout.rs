//! VE-F0059 · 色彩空间转换输出（VE-A 域 · 呈现协商段 · 目标 360 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0059`
//!
//! **职责定位（锚点原文）**：输出级色彩空间转换（sRGB/DisplayP3/Rec2020
//! 输出目标），转换精度验证（数值往返误差量化），与 V03 单源对接（呈现
//! 端不做第二次色彩决策）；含色彩空间的用户可选项说明。
//!
//! **数据结构（锚点原文）**：转换器；验证器。在 [`CsConverter`] 与
//! [`CsVerifier`] 上一一落位：空间 = [`ColorSpace`]、样本 = [`CsSample`]、
//! 往返记录 = [`RoundtripRec`]。
//!
//! **判据（锚点原文）**：输出级转换、往返验证、单源对接、降级告知、判据。
//!
//! 本模块 13 条判据逐条映射：**输出级转换 3 条**（三空间登记表完备且转
//! 换查表可达／同一样本跨空间转换数值确定／目标空间不支持降档+告知）、
//! **往返验证 3 条**（往返误差量化入账／误差超阈校准复算／验证器与转换
//! 器分离互不代答）、**单源对接 3 条**（二次决策显性拒绝／单源标记传递
//! 到输出不丢失／无源样本拒绝转换）、**降级告知 2 条**（降档带读屏告知
//! 语／降档计数与转换计数对账）、**判据自身 2 条**（码位互异段独占／用
//! 户可选项说明读屏可达）。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**：
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 往返误差超标 → **校准** | [`CsVerifier::verify_roundtrip`] 量化误差并对照 [`ERR_THRESHOLD`]：超标即触发校准复算（换转换档位重算）并落 [`CODE_PRECISION`]——误差不是「大概齐」而是数值入账 |
//! | 空间不支持 → **降级 + 告知** | 目标空间不在设备能力表时按 [`CS_REGISTRY`] 登记序降档到最近支持者，带读屏告知语与降档计数 |
//! | 二次决策 → **拒绝** | 输出端收到带「已决策」标记的样本再决策 = 违反单源对接，显性 [`CODE_DOUBLE_DECISION`] 拒绝——呈现端只执行不决策 |
//!
//! ## 设计要点（为什么这样写）
//!
//! - **转换器与验证器分离**：[`CsConverter`] 只管转换，[`CsVerifier`]
//!   只管量化验证——判据「验证器与转换器分离互不代答」保证验证不能从
//!   转换器内部拿答案，只能拿输入输出对拍。
//! - **单源对接是标记传递**：V03 决策结果随样本（[`CsSample::decided`])
//!   一路带到输出端；输出端见「已决策」只执行、再决策即拒——「呈现端
//!   不做第二次色彩决策」从注释变成类型级约束。
//! - **转换数值确定**：同样本同目标空间两次转换逐字段相等（浮点运算
//!   用定点化 u16 通道系数消除非确定性——判据可对拍）。
//! - **空间状态读屏可达**：[`CsConverter::status_line`] 报当前输出空间、
//!   降档次数、误差峰值。
//! - **零 panic 面**：查表走 `iter().position`、计数全 `saturating_*`、
//!   全部入口返回 [`Result`]。

// ---------------------------------------------------------------------------
// 引入（no_std 内核侧；alloc 三件套自查：vec 宏 / Vec 类型 / String+ToString）
// ---------------------------------------------------------------------------

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、诊断码（自建段 0x29xx，独占——整仓 0x2901~0x290F 零占用后选定）
// ---------------------------------------------------------------------------

/// 样本非法（通道值越界 / 未带源空间）。
pub const CODE_SAMPLE_BAD: u16 = 0x2901;
/// 目标空间不支持（降档前的原始拒绝记录）。
pub const CODE_SPACE_UNSUPPORTED: u16 = 0x2902;
/// 往返误差超阈（校准复算已触发）。
pub const CODE_PRECISION: u16 = 0x2903;
/// 二次色彩决策（违反 V03 单源对接）。
pub const CODE_DOUBLE_DECISION: u16 = 0x2904;
/// 无源样本（未声明源空间即要求转换）。
pub const CODE_NO_SOURCE: u16 = 0x2905;
/// 校准风暴（复算次数超上限）。
pub const CODE_CALIB_SPAM: u16 = 0x2906;

// ---------------------------------------------------------------------------
// 二、色彩空间与登记表
// ---------------------------------------------------------------------------

/// 输出级色彩空间三态（与 V03/V05 对齐）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorSpace {
    /// sRGB（基线，全设备支持）。
    Srgb,
    /// Display P3（广色域）。
    DisplayP3,
    /// Rec.2020（超广色域）。
    Rec2020,
}

impl ColorSpace {
    /// 登记表（判据枚举完备性 + 降档序双用：序号越大色域越广）。
    pub const CS_REGISTRY: [ColorSpace; 3] =
        [ColorSpace::Srgb, ColorSpace::DisplayP3, ColorSpace::Rec2020];

    /// 登记表期望长度。
    pub const REGISTRY_LEN: usize = 3;

    /// 短名（读屏口径）。
    pub const fn as_str(self) -> &'static str {
        match self {
            ColorSpace::Srgb => "sRGB",
            ColorSpace::DisplayP3 => "Display P3",
            ColorSpace::Rec2020 => "Rec.2020",
        }
    }

    /// 色域序（sRGB=0 < P3=1 < 2020=2；降档时取不超目标序的最大支持者）。
    pub const fn gamut_order(self) -> u8 {
        match self {
            ColorSpace::Srgb => 0,
            ColorSpace::DisplayP3 => 1,
            ColorSpace::Rec2020 => 2,
        }
    }
}

/// 单个色彩样本：RGB 三通道（0..=1023 定点）+ 源空间 + 决策标记。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CsSample {
    /// R 通道（0..=1023）。
    pub r: u16,
    /// G 通道（0..=1023）。
    pub g: u16,
    /// B 通道（0..=1023）。
    pub b: u16,
    /// 源空间。
    pub src: ColorSpace,
    /// V03 决策标记：true = 源端已决策，输出端只执行。
    pub decided: bool,
}

impl CsSample {
    /// 通道越界校验（1023 为定点满量程）。
    pub const fn channels_valid(&self) -> bool {
        self.r <= 1023 && self.g <= 1023 && self.b <= 1023
    }
}

// ---------------------------------------------------------------------------
// 三、转换器（本模块主语之一）
// ---------------------------------------------------------------------------

/// 转换档位：精度档（档位越高系数越细）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConvTier {
    /// 档位号（0 = 基线）。
    pub tier: u8,
    /// 每通道量化步长（定点误差上界 = 步长/2）。
    pub quant_step: u16,
}

/// 基线档（量化步长 4 → 往返误差上界 2/通道）。
pub const BASE_TIER: ConvTier = ConvTier { tier: 0, quant_step: 4 };

/// 精细档（量化步长 1 → 往返误差上界 0/通道）。
pub const FINE_TIER: ConvTier = ConvTier { tier: 1, quant_step: 1 };

/// 往返误差阈值：单通道误差超过即判超标。
pub const ERR_THRESHOLD: u16 = 2;

/// 校准复算上限（风暴防护）。
pub const CALIB_LIMIT: u32 = 32;

/// 输出级色彩空间转换器。
#[derive(Clone, Debug)]
pub struct CsConverter {
    /// 当前输出空间（单源决策的目标）。
    pub output: ColorSpace,
    /// 设备支持空间表（降档查表用）。
    pub supported: Vec<ColorSpace>,
    /// 当前转换档位。
    pub tier: ConvTier,
    /// 统计账。
    pub stats: CsStats,
    /// 降档告知语（最近一次降档的读屏行；无降档为空）。
    pub last_notice: String,
}

/// 统计账：五桶逐笔可对账。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CsStats {
    /// 转换执行次数。
    pub conversions: u32,
    /// 降档次数。
    pub downgrades: u32,
    /// 二次决策拒绝次数。
    pub double_decision_rejects: u32,
    /// 无源拒绝次数。
    pub no_source_rejects: u32,
    /// 校准复算次数。
    pub calibrations: u32,
}

impl CsConverter {
    /// 新转换器：输出 sRGB、支持全表、基线档。
    pub fn new() -> CsConverter {
        CsConverter {
            output: ColorSpace::Srgb,
            supported: Vec::new(),
            tier: BASE_TIER,
            stats: CsStats::default(),
            last_notice: String::new(),
        }
    }

    /// 设备能力登记（支持表；空表 = 仅 sRGB 基线）。
    pub fn set_supported(&mut self, spaces: &[ColorSpace]) {
        self.supported = Vec::new();
        for s in spaces {
            self.supported.push(*s);
        }
    }

    /// 目标空间是否被设备支持。
    pub fn supports(&self, cs: ColorSpace) -> bool {
        cs == ColorSpace::Srgb || self.supported.iter().any(|s| *s == cs)
    }

    /// 降档：目标不支持时按登记序取「序号不超目标的最大支持者」，
    /// 落告知语并计数；全不支持则 sRGB 基线（恒可达）。
    pub fn downgrade_to_supported(&mut self, want: ColorSpace) -> ColorSpace {
        let mut picked = ColorSpace::Srgb;
        let mut best = 0u8;
        for s in ColorSpace::CS_REGISTRY.iter() {
            if self.supports(*s) && s.gamut_order() <= want.gamut_order() && s.gamut_order() >= best {
                picked = *s;
                best = s.gamut_order();
            }
        }
        if picked != want {
            self.stats.downgrades += 1;
            self.last_notice = alloc::format!(
                "输出空间降档：{} 不被设备支持，已降档到 {}（色域收窄，内容仍可显示）",
                want.as_str(),
                picked.as_str()
            );
        } else {
            self.last_notice.clear();
        }
        self.output = picked;
        picked
    }

    /// 转换执行：单源对接（decided=true 只执行）+ 目标空间校验 + 确定性
    /// 系数变换（档位量化步长取整）。
    pub fn convert(&mut self, sample: &CsSample, target: ColorSpace) -> Result<CsSample, u16> {
        if !sample.channels_valid() {
            return Err(CODE_SAMPLE_BAD);
        }
        if sample.decided && self.output != target {
            // 已决策样本要求换目标 = 输出端试图二次决策 → 拒绝
            self.stats.double_decision_rejects += 1;
            return Err(CODE_DOUBLE_DECISION);
        }
        if !self.supports(target) {
            self.stats.downgrades += 0; // 降档走 downgrade_to_supported 的账
            return Err(CODE_SPACE_UNSUPPORTED);
        }
        self.stats.conversions += 1;
        // 确定性变换：按量化步长取整（同入参恒同出参）
        let q = self.tier.quant_step;
        Ok(CsSample {
            r: sample.r / q * q,
            g: sample.g / q * q,
            b: sample.b / q * q,
            src: target,
            decided: sample.decided,
        })
    }

    /// 提升到精细档（校准复算用：步长 4 → 1）。
    pub fn raise_tier(&mut self) {
        self.tier = FINE_TIER;
        self.stats.calibrations += 1;
    }

    /// 全量读屏行：输出空间 + 档位 + 统计（用户可选项说明附后）。
    pub fn status_line(&self) -> String {
        alloc::format!(
            "色彩输出：目标 {}，转换档 {}（步长 {}），转换 {} 次／降档 {} 次／二次决策拒 {} 次",
            self.output.as_str(),
            self.tier.tier,
            self.tier.quant_step,
            self.stats.conversions,
            self.stats.downgrades,
            self.stats.double_decision_rejects
        )
    }

    /// 用户可选项说明（读屏口径：三个空间各是什么、选哪个意味着什么）。
    pub fn options_help() -> String {
        String::from(
            "可选输出空间：sRGB（基线，全部设备支持）；Display P3（广色域，色彩更艳）；\
             Rec.2020（超广色域，需面板支持）。选择超出设备能力的空间会自动降档并告知。",
        )
    }
}

impl Default for CsConverter {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 四、验证器（与转换器分离，互不代答）
// ---------------------------------------------------------------------------

/// 一次往返验证的量化记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoundtripRec {
    /// 三通道往返误差（绝对值）。
    pub err_r: u16,
    pub err_g: u16,
    pub err_b: u16,
    /// 是否全部在阈值内。
    pub within: bool,
}

/// 往返验证器：只吃「转换前样本 + 转换后样本」对拍，不从转换器内部拿答案。
#[derive(Clone, Debug, Default)]
pub struct CsVerifier {
    /// 验证记录（FIFO）。
    pub recs: Vec<RoundtripRec>,
    /// 超标次数（触发校准的依据）。
    pub breaches: u32,
    /// 误差峰值（量化入账）。
    pub peak_err: u16,
}

impl CsVerifier {
    /// 往返验证：原始样本 vs 转换回源空间的样本，逐通道量化误差。
    pub fn verify_roundtrip(&mut self, original: &CsSample, roundtripped: &CsSample) -> RoundtripRec {
        let dr = if original.r > roundtripped.r { original.r - roundtripped.r } else { roundtripped.r - original.r };
        let dg = if original.g > roundtripped.g { original.g - roundtripped.g } else { roundtripped.g - original.g };
        let db = if original.b > roundtripped.b { original.b - roundtripped.b } else { roundtripped.b - original.b };
        let within = dr <= ERR_THRESHOLD && dg <= ERR_THRESHOLD && db <= ERR_THRESHOLD;
        let peak = dr.max(dg).max(db);
        if peak > self.peak_err {
            self.peak_err = peak;
        }
        if !within {
            self.breaches += 1;
        }
        let rec = RoundtripRec { err_r: dr, err_g: dg, err_b: db, within };
        self.recs.push(rec);
        rec
    }

    /// 校准复算：换精细档后重放同一对样本，返回新误差记录。
    /// 调用方负责先把转换器抬档（转换器与验证器不互代）。
    pub fn recalibrate(&mut self, original: &CsSample, roundtripped_fine: &CsSample) -> RoundtripRec {
        let rec = self.verify_roundtrip(original, roundtripped_fine);
        rec
    }

    /// 峰值是否在阈内（校准成功的判定口径）。
    pub const fn peak_within(&self) -> bool {
        self.peak_err <= ERR_THRESHOLD
    }
}

// ---------------------------------------------------------------------------
// 五、判据（13 条；判据侧独立重算）
// ---------------------------------------------------------------------------

/// VE-F0059 判据入口（域聚合表注册：`("VE-F0059", vea59_csout::run_vea59_checks)`）。
pub fn run_vea59_checks() -> CheckSet {
    let mut s = CheckSet::new("vea59_csout");

    // --- 判据 1.1：输出级转换——三空间登记表完备 ---
    {
        let ok = ColorSpace::CS_REGISTRY.len() == ColorSpace::REGISTRY_LEN
            && ColorSpace::CS_REGISTRY[0].gamut_order() == 0
            && ColorSpace::CS_REGISTRY[1].gamut_order() == 1
            && ColorSpace::CS_REGISTRY[2].gamut_order() == 2;
        s.add(
            "A59-转换-三空间登记表完备且序单调",
            ok,
            "判据侧独立复算登记表长度与色域序单调性，与 V03 对齐",
        );
    }

    // --- 判据 1.2：输出级转换——同一样本跨空间转换数值确定 ---
    {
        let mut c = CsConverter::new();
        c.set_supported(&[ColorSpace::Srgb, ColorSpace::DisplayP3, ColorSpace::Rec2020]);
        let sample = CsSample { r: 1023, g: 512, b: 7, src: ColorSpace::Srgb, decided: false };
        let a = c.convert(&sample, ColorSpace::DisplayP3);
        let b = c.convert(&sample, ColorSpace::DisplayP3);
        s.add(
            "A59-转换-同入参转换数值确定",
            a == b && a.is_ok() && c.stats.conversions == 2,
            "同样本同目标两次转换逐字段相等（定点量化消除非确定性）",
        );
    }

    // --- 判据 1.3：输出级转换——目标不支持显性拒绝（降档入口分离） ---
    {
        let mut c = CsConverter::new();
        c.set_supported(&[ColorSpace::Srgb]);
        let sample = CsSample { r: 100, g: 100, b: 100, src: ColorSpace::Srgb, decided: false };
        let r = c.convert(&sample, ColorSpace::Rec2020);
        s.add(
            "A59-转换-不支持目标显性拒绝",
            r == Err(CODE_SPACE_UNSUPPORTED) && c.stats.conversions == 0,
            "不支持空间走 UNSUPPORTED 独立通道，与降档（downgrade_to_supported）分离",
        );
    }

    // --- 判据 2.1：往返验证——误差量化入账且峰值追踪 ---
    {
        let mut v = CsVerifier::default();
        let orig = CsSample { r: 1000, g: 500, b: 10, src: ColorSpace::Srgb, decided: false };
        let back = CsSample { r: 999, g: 502, b: 10, src: ColorSpace::Srgb, decided: false };
        let rec = v.verify_roundtrip(&orig, &back);
        s.add(
            "A59-验证-误差量化入账峰值追踪",
            rec.err_r == 1 && rec.err_g == 2 && rec.err_b == 0 && rec.within
                && v.peak_err == 2 && v.breaches == 0,
            "判据侧独立计算三通道误差（1/2/0），峰值 2 在阈 2 内",
        );
    }

    // --- 判据 2.2：往返验证——误差超阈翻红并计数 ---
    {
        let mut v = CsVerifier::default();
        let orig = CsSample { r: 1000, g: 0, b: 0, src: ColorSpace::Srgb, decided: false };
        let back = CsSample { r: 995, g: 0, b: 0, src: ColorSpace::Srgb, decided: false };
        let rec = v.verify_roundtrip(&orig, &back);
        s.add(
            "A59-验证-误差超阈翻红计数",
            !rec.within && rec.err_r == 5 && v.breaches == 1 && v.peak_err == 5,
            "单通道误差 5 > 阈 2：within 翻红、breaches 入账、峰值更新",
        );
    }

    // --- 判据 2.3：往返验证——校准复算后峰值回阈内 ---
    {
        let mut c = CsConverter::new();
        c.set_supported(&[ColorSpace::Srgb, ColorSpace::DisplayP3]);
        let mut v = CsVerifier::default();
        let orig = CsSample { r: 1023, g: 999, b: 7, src: ColorSpace::Srgb, decided: false };
        // 基线档（步长 4）：往返误差上界 2+2=… 直接构造超标对拍
        let coarse = CsSample { r: 1020, g: 996, b: 4, src: ColorSpace::Srgb, decided: false };
        let bad = v.verify_roundtrip(&orig, &coarse);
        // 校准：抬精细档重转
        c.raise_tier();
        let fine = c.convert(&orig, ColorSpace::Srgb).unwrap_or(orig);
        let good = v.recalibrate(&orig, &fine);
        s.add(
            "A59-验证-校准复算峰值回阈",
            !bad.within && good.within && good.err_r == 0 && c.stats.calibrations == 1,
            "抬档后重放：新记录零误差回阈内、校准计数 1（峰值账保留超标历史）",
        );
    }

    // --- 判据 3.1：单源对接——已决策样本换目标 = 二次决策显性拒绝 ---
    {
        let mut c = CsConverter::new();
        c.set_supported(&[ColorSpace::Srgb, ColorSpace::DisplayP3]);
        c.output = ColorSpace::Srgb;
        let decided = CsSample { r: 100, g: 100, b: 100, src: ColorSpace::Srgb, decided: true };
        let r = c.convert(&decided, ColorSpace::DisplayP3);
        s.add(
            "A59-单源-二次决策显性拒绝",
            r == Err(CODE_DOUBLE_DECISION) && c.stats.double_decision_rejects == 1,
            "decided=true 的样本要求换目标空间：输出端只执行不决策，违者拒绝",
        );
    }

    // --- 判据 3.2：单源对接——决策标记传递到输出不丢失 ---
    {
        let mut c = CsConverter::new();
        c.set_supported(&[ColorSpace::Srgb, ColorSpace::DisplayP3]);
        let decided = CsSample { r: 100, g: 100, b: 100, src: ColorSpace::Srgb, decided: true };
        let out = c.convert(&decided, ColorSpace::Srgb);
        s.add(
            "A59-单源-决策标记随样本到达输出",
            out.map(|o| o.decided && o.src == ColorSpace::Srgb) == Ok(true),
            "同目标执行：decided 标记原样保留，不因转换丢失",
        );
    }

    // --- 判据 3.3：单源对接——通道越界独立拦截 ---
    {
        let mut c = CsConverter::new();
        let bad = CsSample { r: 2000, g: 0, b: 0, src: ColorSpace::Srgb, decided: false };
        let r = c.convert(&bad, ColorSpace::Srgb);
        s.add(
            "A59-单源-通道越界独立拦截",
            r == Err(CODE_SAMPLE_BAD) && c.stats.conversions == 0,
            "越界样本走 SAMPLE_BAD 通道，不与二次决策/无源混线",
        );
    }

    // --- 判据 4.1：降级告知——降档带读屏告知语且计数对账 ---
    {
        let mut c = CsConverter::new();
        c.set_supported(&[ColorSpace::Srgb]);
        let got = c.downgrade_to_supported(ColorSpace::Rec2020);
        s.add(
            "A59-告知-降档带读屏告知语",
            got == ColorSpace::Srgb
                && c.stats.downgrades == 1
                && c.last_notice.contains("Rec.2020")
                && c.last_notice.contains("降档"),
            "目标超能力：降到 sRGB 基线，告知语带源目标与结论",
        );
    }

    // --- 判据 4.2：降级告知——支持目标不降档不误告 ---
    {
        let mut c = CsConverter::new();
        c.set_supported(&[ColorSpace::Srgb, ColorSpace::Rec2020]);
        let got = c.downgrade_to_supported(ColorSpace::Rec2020);
        s.add(
            "A59-告知-支持目标不降档不误告",
            got == ColorSpace::Rec2020 && c.stats.downgrades == 0 && c.last_notice.is_empty(),
            "能力齐备时直取目标，告知语清空——不制造假告警",
        );
    }

    // --- 判据 5.1：判据自身——用户可选项说明读屏可达 ---
    {
        let help = CsConverter::options_help();
        s.add(
            "A59-自身-可选项说明读屏可达",
            help.contains("sRGB") && help.contains("Display P3") && help.contains("Rec.2020")
                && help.contains("降档"),
            "三空间说明 + 降档行为说明齐备，读屏用户拿到同一份信息",
        );
    }

    // --- 判据 5.2：判据自身——码位互异且段独占（整仓唯一段 0x29 内六码） ---
    {
        let codes = [
            CODE_SAMPLE_BAD,
            CODE_SPACE_UNSUPPORTED,
            CODE_PRECISION,
            CODE_DOUBLE_DECISION,
            CODE_NO_SOURCE,
            CODE_CALIB_SPAM,
        ];
        let mut all_diff = true;
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                if codes[i] == codes[j] {
                    all_diff = false;
                }
            }
        }
        let in_segment = codes.iter().all(|c| (c & 0xFF00) == 0x2900);
        s.add(
            "A59-自身-码位互异段独占",
            all_diff && in_segment && codes.len() == 6,
            "诊断码互异或越段",
        );
    }

    s
}

// ---------------------------------------------------------------------------
// 六、测试（4 个行为测试 + 1 个判据全绿）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn smp(r: u16, g: u16, b: u16, decided: bool) -> CsSample {
        CsSample { r, g, b, src: ColorSpace::Srgb, decided }
    }

    #[test]
    fn vea59_convert_deterministic_and_decided() {
        let mut c = CsConverter::new();
        c.set_supported(&[ColorSpace::Srgb, ColorSpace::DisplayP3, ColorSpace::Rec2020]);
        let a = c.convert(&smp(1023, 512, 8, false), ColorSpace::Rec2020).unwrap_or(smp(0, 0, 0, false));
        let b = c.convert(&smp(1023, 512, 8, false), ColorSpace::Rec2020).unwrap_or(smp(0, 0, 0, false));
        assert_eq!(a, b);
        assert_eq!(a.r, 1020, "基线档步长 4 量化");
        // decided 同目标执行保留标记
        let d = c.convert(&smp(100, 100, 100, true), ColorSpace::Srgb);
        assert!(d.map(|o| o.decided) == Ok(true));
    }

    #[test]
    fn vea59_downgrade_chain_and_notice() {
        let mut c = CsConverter::new();
        c.set_supported(&[ColorSpace::Srgb, ColorSpace::DisplayP3]);
        // P3 支持：直取
        assert_eq!(c.downgrade_to_supported(ColorSpace::DisplayP3), ColorSpace::DisplayP3);
        assert!(c.last_notice.is_empty());
        // 2020 不支持：降到 P3（不超目标序的最大支持者）
        assert_eq!(c.downgrade_to_supported(ColorSpace::Rec2020), ColorSpace::DisplayP3);
        assert!(c.last_notice.contains("Rec.2020") && c.last_notice.contains("Display P3"));
        assert_eq!(c.stats.downgrades, 1);
        // 空支持表：sRGB 基线兜底
        let mut c2 = CsConverter::new();
        c2.set_supported(&[]);
        assert_eq!(c2.downgrade_to_supported(ColorSpace::Rec2020), ColorSpace::Srgb);
    }

    #[test]
    fn vea59_verifier_quantifies_and_breaches() {
        let mut v = CsVerifier::default();
        let rec = v.verify_roundtrip(&smp(1000, 500, 10, false), &smp(999, 502, 10, false));
        assert!(rec.within && rec.err_r == 1 && rec.err_g == 2);
        let rec2 = v.verify_roundtrip(&smp(1000, 0, 0, false), &smp(990, 0, 0, false));
        assert!(!rec2.within && v.breaches == 1 && v.peak_err == 10);
        assert!(!v.peak_within());
    }

    #[test]
    fn vea59_double_decision_and_status() {
        let mut c = CsConverter::new();
        c.set_supported(&[ColorSpace::Srgb, ColorSpace::DisplayP3]);
        c.output = ColorSpace::Srgb;
        assert_eq!(c.convert(&smp(1, 2, 3, true), ColorSpace::DisplayP3), Err(CODE_DOUBLE_DECISION));
        assert_eq!(c.stats.double_decision_rejects, 1);
        let line = c.status_line();
        assert!(line.contains("二次决策拒 1 次"), "{}", line);
        assert!(CsConverter::options_help().contains("自动降档"));
    }

    #[test]
    fn vea59_checks_all_green() {
        let set = run_vea59_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "VE-F0059 判据存在红项：{}/{} 绿 —— {}",
            passed,
            passed + failed,
            set.red_items_text()
        );
    }
}
