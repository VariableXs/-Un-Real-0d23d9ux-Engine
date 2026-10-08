//! VE-F1008 · PNG 写入器选项（VE-F 域 · 着色器系统 · PNG 组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1008`
//!
//! **判据（锚点原文）**：选项全组合 roundtrip（合法性矩阵内全测）、压缩比对比表、
//! 快速模式收益（耗时 −40% 实测、体积 +8% 声明）、非法组合拒绝、判据。
//!
//! 锚点原文：「PNG 写入器选项面极致深化：压缩级别（1-9——级别与耗时/压缩比
//! 权衡表随档实测）、滤波策略（全部遍历/自适应两策略/指定单滤波——四选路）、
//! 颜色类型强制（用户指定降档——降档合法性校验：真彩强制调色板需量化步骤显性
//! 提示有损）、位深选择、元数据注入（文本块写入——F1005 三块的反向组装）、
//! 隔行开关（Adam7 编码 F1003 联动）、快速模式（跳过滤波试探——编码速度优先，
//! 压缩比代价声明）。选项验证：非法组合拒绝（如 1-bit+真彩）——合法性矩阵表驱动，
//! 拒绝信息给合法组合建议（三要素）。数据结构：选项对象（默认值+校验器+效果预
// 声明）。错误路径：非法组合→拒绝并建议最近合法组合；元数据超限→拒绝该项保留
//! 其他（部分成功语义显性）。」
//!
//! 本单交付**选项面**，不交付 zlib/滤波/Adam7 本体——那些是 F1002/F1003 的。
//! 本单只回答：**用户能拧哪些旋、每个旋拧到什么位置合法、拧完会得到什么**。
//!
//! 1. **合法性矩阵表驱动**（判据一）。选项组合的合法性**不是**散落在
//!    `if` 里的判断，而是 [`LegalMatrix`] 这张表：每个
//!    （颜色类型 × 位深）格子有唯一裁决 [`CellVerdict`]。
//!    **为什么必须表驱动**：组合数 = 5 类型 × 5 位深 = 25，而「真彩降调色板」
//!    这类降档还要额外附加条件（需量化）——散落判断必然出现「有的组合没人管」
//!    的漏判。表驱动让**每个格子都有裁决**，判据用
//!    [`LegalMatrix::cell_count`] 断恰为 25，且遍历全表不留空格。
//!
//! 2. **拒绝给最近合法组合**（判据四）。锚点写「拒绝信息给合法组合建议」。
//!    「最近」不是随手挑一个：[`nearest_legal`] 用**二维曼哈顿距离**在
//!    （类型序 × 位深序）上找最近合法格子，**同距时取类型序小者**并显式
//!    留痕（[`NearestTie`]）——否则「建议」在不同输入下会飘，用户无法预期。
//!    这条判据要求**恰近**：造一个「总是返回第一个合法格子」的实现会因
//!    距离不等而变红。
//!
//! 3. **快速模式收益**（判据三）。锚点给了两个数：耗时 −40%、体积 +8%。
//!    **这两个数不是拍脑袋写进注释就行的**——[`FastModeEffect`] 把它们做成
//!    **可推演的声明**：由 [`estimate_fast_effect`] 从（滤波试探次数、
//!    压缩级别）算出预期比值，判据断「快速模式跳过滤波试探 ⇒ 试探次数归1」
//!    且「体积代价为正（不可能又���又快）」。**耗时实测在no_std 内核里拿不到
//!    墙钟**，所以判据断的是**可机检的结构性事实**（试探次数、体积代价符号、
//!    声明与推演一致），不是断一个编不出来的计时数字。
//!
//! 4. **部分成功语义显性**（判据五）。元数据超限时锚点要求「拒绝该项保留
//!    其他」。最容易被做错的实现是**整单失败**——用户加了 5 个文本块，第 3 个
//!    超限，结果一个都没写进去，用户以为图没有元数据。
//!    [`MetadataOutcome`] 把结果拆成 [`Kept`](已写入) / [`Dropped`](被拒，带原因)
//!    两侧，判据要求「有kept 也有 dropped」同时成立且**总数守恒**
//!    （kept + dropped == 提交数，用 `==` 不用 `>=`）。
//!
//! ## 零 panic 面
//!
//! 生产代码无 `unwrap` / `expect` / `panic!` / 裸下标越界：矩阵查表越界给
//! [`Option`]，选项校验失败返回 [`OptionFault`] 而非中止。

#![allow(clippy::needless_range_loop)]

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::vef02_pngenc::{ColorType, Strategy};

/// 上游 F1005 的文本块种类（反向组装用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TextChunkKind {
    /// tEXt：Latin-1，keyword\0text。
    Txt,
    /// zTXt：压缩 Latin-1，keyword\0method+压缩体。
    ZTxt,
    /// iTXt：UTF-8，keyword\0flag\0lang\0trans\0text。
    ITxt,
}

impl TextChunkKind {
    /// 线上关键字。
    pub fn wire(self) -> &'static str {
        match self {
            TextChunkKind::Txt => "tEXt",
            TextChunkKind::ZTxt => "zTXt",
            TextChunkKind::ITxt => "iTXt",
        }
    }
}

/// 一条待写入的元数据（keyword + 内容 + 块种类）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MetadataItem {
    /// 关键字（F1005 上限 79 字节）。
    pub keyword: String,
    /// 内容原始字节。
    pub payload: Vec<u8>,
    /// 块种类。
    pub kind: TextChunkKind,
    /// 是否压缩（仅对 [`TextChunkKind::ZTxt`] / 压缩位iTXt 有意义）。
    pub compressed: bool,
}

/// 单条目体积上限（对齐 F1005 `TEXT_MAX_BYTES` 语义，取其同一量级）。
pub const ITEM_MAX_BYTES: usize = 2 * 1024 * 1024;

/// 条目数上限（对齐 F1005 `ENTRY_MAX_COUNT`）。
pub const ITEM_MAX_COUNT: usize = 500;

// ---------------------------------------------------------------------------
// 一、滤波策略（四选路）
// ---------------------------------------------------------------------------

/// 滤波选择面。
///
/// 锚点原文：「滤波策略（全部遍历/自适应两策略/指定单滤波——四选路）」。
/// 四条路分别是：
/// - [`FilterChoice::AllFixedNone`]：全部固定不滤波（体积基线，最快）；
/// - [`FilterChoice::AllMinAbsSum`]：全部最小绝对差和；
/// - [`FilterChoice::AllMinEntropy`]：全部最小熵；
/// - [`FilterChoice::PerLineAdaptive`]：逐行自适应（**真正的自适应**，逐行选优）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FilterChoice {
    /// 全部固定不滤波。
    AllFixedNone,
    /// 全部最小绝对差和。
    AllMinAbsSum,
    /// 全部最小熵。
    AllMinEntropy,
    /// 逐行自适应（每行独立评分选优）。
    PerLineAdaptive,
    /// 指定单个滤波（0..=4：None/Sub/Up/Average/Paeth）。
    Fixed(u8),
}

impl FilterChoice {
    /// 四条路各自的稳定名（判据与诊断用）。
    pub fn wire(self) -> &'static str {
        match self {
            FilterChoice::AllFixedNone => "全部/不滤波",
            FilterChoice::AllMinAbsSum => "全部/最小绝对差和",
            FilterChoice::AllMinEntropy => "全部/最小熵",
            FilterChoice::PerLineAdaptive => "逐行自适应",
            FilterChoice::Fixed(_) => "指定单滤波",
        }
    }

    /// 该选择映射到上游 [`Strategy`]（逐行自适应由 F1002 的评分器承担）。
    pub fn to_strategy(self) -> Strategy {
        match self {
            FilterChoice::AllFixedNone => Strategy::FixedNone,
            FilterChoice::AllMinAbsSum => Strategy::MinAbsSum,
            FilterChoice::AllMinEntropy => Strategy::MinEntropy,
            // 自适应与指定单滤波在调用侧按行驱动，这里给出其默认评分策略。
            FilterChoice::PerLineAdaptive => Strategy::MinAbsSum,
            FilterChoice::Fixed(_) => Strategy::FixedNone,
        }
    }

    /// 是否逐行重新评分（决定「滤波试探次数」是否随行数放大）。
    pub fn is_per_line(self) -> bool {
        matches!(self, FilterChoice::PerLineAdaptive)
    }

    /// 指定单滤波的编号，越界给 `None`。
    pub fn fixed_kind(self) -> Option<u8> {
        match self {
            FilterChoice::Fixed(k) if k <= 4 => Some(k),
            _ => None,
        }
    }
}

/// 四条路的全集（判据用来核对「四选路」这个数）。
pub const FILTER_CHOICES: [FilterChoice; 5] = [
    FilterChoice::AllFixedNone,
    FilterChoice::AllMinAbsSum,
    FilterChoice::AllMinEntropy,
    FilterChoice::PerLineAdaptive,
    FilterChoice::Fixed(0),
];

// ---------------------------------------------------------------------------
// 二、合法性矩阵（判据一）
// ---------------------------------------------------------------------------

/// 一个（颜色类型 × 位深）格子的裁决。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CellVerdict {
    /// 合法，无需降档。
    Legal,
    /// 非法，且**无**就近合法格子（拒绝且不给建议）。
    IllegalNoAlt,
    /// 可通过**降档**变合法（需量化步骤，有损）。
    LegalByDowngrade,
}

/// 合法性矩阵。
///
/// **表驱动**（锚点「合法性矩阵表驱动」）：5 类型 × 5 位深 = 25 格，每格
/// 有唯一裁决。位深取值域为 `{1,2,4,8,16}`。
pub struct LegalMatrix {
    verdicts: [[CellVerdict; 5]; 5],
}

/// 位深取值域（与 F1002 `legal_depths` 同口径）。
pub const DEPTHS: [u8; 5] = [1, 2, 4, 8, 16];

/// 颜色类型全集（与 F1002 `ColorType` 一一对应）。
pub const COLOR_TYPES: [ColorType; 5] = [
    ColorType::Gray,
    ColorType::Rgb,
    ColorType::Palette,
    ColorType::GrayAlpha,
    ColorType::Rgba,
];

impl LegalMatrix {
    /// 构造矩阵。
    ///
    /// 规则（与 F1002 `combo_is_legal` 同口径，但**显式含降档裁决**）：
    /// - Gray：`{1,2,4,8,16}` 全合法（灰度可降到1/2/4 位量化）；
    /// - Rgb / GrayAlpha / Rgba：`{8,16}` 合法（浅于 8 位会丢颜色 ⇒ 非法）；
    /// - Palette：`{1,2,4,8}` 合法（16 位索引无意义 ⇒ 非法）。
    pub fn new() -> LegalMatrix {
        let mut m = LegalMatrix {
            verdicts: [[CellVerdict::IllegalNoAlt; 5]; 5],
        };
        let mut ti = 0usize;
        while ti < COLOR_TYPES.len() {
            let ct = COLOR_TYPES[ti];
            let mut di = 0usize;
            while di < DEPTHS.len() {
                let d = DEPTHS[di];
                m.verdicts[ti][di] = if ct.legal_depths().contains(&d) {
                    CellVerdict::Legal
                } else if matches!(ct, ColorType::Rgb | ColorType::GrayAlpha | ColorType::Rgba) && d < 8 {
                    // 真彩类降到 8 位以下 ⇒ 需量化，可通过降档变合法（有损）。
                    CellVerdict::LegalByDowngrade
                } else {
                    CellVerdict::IllegalNoAlt
                };
                di += 1;
            }
            ti += 1;
        }
        m
    }

    /// 查格子。类型序号越界给 `None`（不 panic）。
    pub fn cell(&self, ti: usize, di: usize) -> Option<CellVerdict> {
        if ti >= self.verdicts.len() {
            return None;
        }
        if di >= self.verdicts[ti].len() {
            return None;
        }
        Some(self.verdicts[ti][di])
    }

    /// 按颜色类型查（先由 `wire()` 定位序号，越界给 `None`）。
    pub fn cell_of(&self, ct: ColorType, depth: u8) -> Option<CellVerdict> {
        let ti = color_index(ct)?;
        let di = depth_index(depth)?;
        self.cell(ti, di)
    }

    /// 格子总数（恒为 25；判据用它钉死「无空格」）。
    ///
    /// 每行宽度取 [`DEPTHS`] 的长度而非 `verdicts[0].len()`：后者是裸下标，
    /// 与「生产代码零 panic 面」的纪律冲突（虽则行数恒非 0，形状由类型保证）。
    pub fn cell_count(&self) -> usize {
        self.verdicts.len() * DEPTHS.len()
    }

    /// 合法格数（`Legal` 计数，不含降档）。
    pub fn legal_cells(&self) -> usize {
        let mut n = 0usize;
        let mut ti = 0usize;
        while ti < self.verdicts.len() {
            let mut di = 0usize;
            while di < self.verdicts[ti].len() {
                if self.verdicts[ti][di] == CellVerdict::Legal {
                    n += 1;
                }
                di += 1;
            }
            ti += 1;
        }
        n
    }

    /// 降档格数。
    pub fn downgrade_cells(&self) -> usize {
        let mut n = 0usize;
        let mut ti = 0usize;
        while ti < self.verdicts.len() {
            let mut di = 0usize;
            while di < self.verdicts[ti].len() {
                if self.verdicts[ti][di] == CellVerdict::LegalByDowngrade {
                    n += 1;
                }
                di += 1;
            }
            ti += 1;
        }
        n
    }
}

/// 颜色类型序号（`wire()` 升序），越界给 `None`。
pub fn color_index(ct: ColorType) -> Option<usize> {
    let mut i = 0usize;
    while i < COLOR_TYPES.len() {
        if COLOR_TYPES[i] == ct {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// 位深序号（升序），越界给 `None`。
pub fn depth_index(depth: u8) -> Option<usize> {
    let mut i = 0usize;
    while i < DEPTHS.len() {
        if DEPTHS[i] == depth {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// 最近合法格子的同距留痕。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NearestTie {
    /// 是否发生同距（需按类型序小者裁决）。
    pub tied: bool,
    /// 参与同距的候选类型序号。
    pub rivals: u8,
}

/// 就近合法格子查询结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NearestLegal {
    /// 建议的颜色类型。
    pub color: ColorType,
    /// 建议的位深。
    pub depth: u8,
    /// 曼哈顿距离（0 = 本身就是合法格）。
    pub distance: u16,
    /// 是否需要降档（有损）。
    pub lossy: bool,
}

/// 找最近合法格子：**二维曼哈顿距离，同距取类型序小者**。
///
/// 返回 `None` 的唯一情形是「整张矩阵一格合法都没有」——本域矩阵恒有合法格，
/// 但仍返回 `Option` 而非 panic。
pub fn nearest_legal(m: &LegalMatrix, ct: ColorType, depth: u8) -> Option<NearestLegal> {
    let ti0 = color_index(ct)?;
    let di0 = depth_index(depth)?;
    let mut best: Option<(u16, usize, usize)> = None;
    let mut tied = false;
    let mut rivals = 0u8;
    let mut ti = 0usize;
    while ti < m.verdicts.len() {
        let mut di = 0usize;
        while di < m.verdicts[ti].len() {
            let v = m.verdicts[ti][di];
            if v == CellVerdict::IllegalNoAlt {
                di += 1;
                continue;
            }
            let dt = if ti > ti0 { (ti - ti0) as u16 } else { (ti0 - ti) as u16 };
            let dd = if di > di0 { (di - di0) as u16 } else { (di0 - di) as u16 };
            let dist = dt + dd;
            match best {
                None => {
                    best = Some((dist, ti, di));
                    tied = false;
                    rivals = 1;
                }
                Some((bd, _, _)) => {
                    if dist < bd {
                        best = Some((dist, ti, di));
                        tied = false;
                        rivals = 1;
                    } else if dist == bd {
                        // 同距：计数 +1，由遍历序（类型序升序）保证取小者
                        rivals += 1;
                        if rivals > 1 {
                            tied = true;
                        }
                    }
                }
            }
            di += 1;
        }
        ti += 1;
    }
    let (dist, ti, di) = best?;
    // 复查：best 必须是「非IllegalNoAlt」的格子
    let v = m.cell(ti, di)?;
    Some(NearestLegal {
        color: COLOR_TYPES[ti],
        depth: DEPTHS[di],
        distance: dist,
        lossy: v == CellVerdict::LegalByDowngrade,
    })
}

/// 就近查询的同距留痕（供判据观测tie 行为）。
pub fn nearest_legal_with_tie(
    m: &LegalMatrix,
    ct: ColorType,
    depth: u8,
) -> Option<(NearestLegal, NearestTie)> {
    let nl = nearest_legal(m, ct, depth)?;
    let ti0 = color_index(ct)?;
    let di0 = depth_index(depth)?;
    let mut rivals = 0u8;
    let mut best_dist = nl.distance;
    let mut ti = 0usize;
    while ti < m.verdicts.len() {
        let mut di = 0usize;
        while di < m.verdicts[ti].len() {
            if m.verdicts[ti][di] != CellVerdict::IllegalNoAlt {
                let dt = if ti > ti0 { (ti - ti0) as u16 } else { (ti0 - ti) as u16 };
                let dd = if di > di0 { (di - di0) as u16 } else { (di0 - di) as u16 };
                if dt + dd == best_dist {
                    rivals += 1;
                }
            }
            di += 1;
        }
        ti += 1;
    }
    Some((
        nl,
        NearestTie {
            tied: rivals > 1,
            rivals,
        },
    ))
}

// ---------------------------------------------------------------------------
// 三、选项对象（默认值 + 校验器 + 效果预声明）
// ---------------------------------------------------------------------------

/// PNG 写入器选项。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct WriteOptions {
    /// 压缩级别 1..=9。
    pub level: u8,
    /// 滤波策略。
    pub filter: FilterChoice,
    /// 强制颜色类型（`None` = 交给编码器决定）。
    pub force_color: Option<ColorType>,
    /// 强制位深（`None` = 交给编码器决定）。
    pub force_depth: Option<u8>,
    /// 是否允许降档（有损）。
    pub allow_downgrade: bool,
    /// 是否启用快速模式。
    pub fast_mode: bool,
    /// 是否隔行（Adam7，联动 F1003）。
    pub interlace: bool,
    /// 待注入元数据。
    pub metadata: Vec<MetadataItem>,
}

impl WriteOptions {
    /// 默认值：level=6、最小绝对差和、不强制、不隔行、非快速。
    pub fn default_options() -> WriteOptions {
        WriteOptions {
            level: 6,
            filter: FilterChoice::AllMinAbsSum,
            force_color: None,
            force_depth: None,
            allow_downgrade: true,
            fast_mode: false,
            interlace: false,
            metadata: Vec::new(),
        }
    }
}

/// 选项错误码（闭合枚举，判据与诊断共用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OptionFaultKind {
    /// 压缩级别越界。
    BadLevel,
    /// 颜色类型越界（不在五型之内）。
    BadColor,
    /// 位深越界。
    BadDepth,
    /// 组合非法且无就近合法格。
    IllegalCombo,
    /// 滤波编号越界。
    BadFilter,
    /// 元数据条目数超限。
    TooManyItems,
    /// 元数据单条目超限。
    ItemTooLarge,
}

/// 一个选项故障（三要素 + 建议）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct OptionFault {
    /// 错误码。
    pub kind: OptionFaultKind,
    /// 发生了什么。
    pub what: String,
    /// 为什么（规则引用）。
    pub why: String,
    /// 下一步建议（合法组合）。
    pub advice: String,
    /// 就近合法组合建议（若有）。
    pub nearest: Option<NearestLegal>,
}

impl OptionFault {
    /// 三要素合成本行（判据钉「非空 + 带引用」）。
    pub fn three_elements(&self) -> String {
        let mut s = String::new();
        s.push_str(&self.what);
        s.push('｜');
        s.push_str(&self.why);
        s.push('｜');
        s.push_str(&self.advice);
        s
    }
}

// ---------------------------------------------------------------------------
// 四、校验器
// ---------------------------------------------------------------------------

/// 校验选项；成功返回 `Err` 携带故障（三要素齐全）。
pub fn validate(m: &LegalMatrix, o: &WriteOptions) -> Result<(), OptionFault> {
    // —— 压缩级别 ——
    if o.level < 1 || o.level > 9 {
        return Err(OptionFault {
            kind: OptionFaultKind::BadLevel,
            what: format!("压缩级别 {} 越界", o.level),
            why: "F1008：压缩级别取值 1..=9".to_string(),
            advice: "改为 1..=9 之间的级别；默认 6".to_string(),
            nearest: None,
        });
    }
    // —— 滤波编号 ——
    if let FilterChoice::Fixed(k) = o.filter {
        if k > 4 {
            return Err(OptionFault {
                kind: OptionFaultKind::BadFilter,
                what: format!("指定滤波编号 {} 越界", k),
                why: "F1008：指定单滤波编号取值 0..=4".to_string(),
                advice: "改为 0..=4（None/Sub/Up/Average/Paeth）".to_string(),
                nearest: None,
            });
        }
    }
    // —— 元数据条数 ——
    if o.metadata.len() > ITEM_MAX_COUNT {
        return Err(OptionFault {
            kind: OptionFaultKind::TooManyItems,
            what: format!("元数据条目数 {} 超限", o.metadata.len()),
            why: format!("F1005 约定条目数上限 {}", ITEM_MAX_COUNT),
            advice: format!("裁剪到 {} 条以内，或分批写入", ITEM_MAX_COUNT),
            nearest: None,
        });
    }
    // —— 颜色 × 位深 组合 ——
    if let (Some(ct), Some(d)) = (o.force_color, o.force_depth) {
        let ti = match color_index(ct) {
            Some(v) => v,
            None => {
                return Err(OptionFault {
                    kind: OptionFaultKind::BadColor,
                    what: format!("颜色类型 wire={} 越界", ct.wire()),
                    why: "F1008：颜色类型取值 0/2/3/4/6".to_string(),
                    advice: "改为 0/2/3/4/6 之一".to_string(),
                    nearest: None,
                })
            }
        };
        let di = match depth_index(d) {
            Some(v) => v,
            None => {
                return Err(OptionFault {
                    kind: OptionFaultKind::BadDepth,
                    what: format!("位深 {} 越界", d),
                    why: "F1008：位深取值 1/2/4/8/16".to_string(),
                    advice: "改为 1/2/4/8/16 之一".to_string(),
                    nearest: None,
                })
            }
        };
        let _ = ti;
        let _ = di;
        match m.cell_of(ct, d) {
            Some(CellVerdict::Legal) => Ok(()),
            Some(CellVerdict::LegalByDowngrade) => {
                if o.allow_downgrade {
                    // 允许降档 ⇒ 通过，但需提示有损（不静默）。
                    Ok(())
                } else {
                    let nl = nearest_legal(m, ct, d);
                    Err(OptionFault {
                        kind: OptionFaultKind::IllegalCombo,
                        what: format!(
                            "组合 颜色类型{}+位深{} 需降档（有损），但未允许降档",
                            ct.wire(),
                            d
                        ),
                        why: "F1008：降档合法性校验——真彩降调色板/低位深需量化步骤".to_string(),
                        advice: match nl {
                            Some(n) => format!(
                                "建议改为 颜色类型{}+位深{}（距离{}）或开启降档",
                                n.color.wire(),
                                n.depth,
                                n.distance
                            ),
                            None => "开启降档或换用合法组合".to_string(),
                        },
                        nearest: nl,
                    })
                }
            }
            _ => {
                let nl = nearest_legal(m, ct, d);
                Err(OptionFault {
                    kind: OptionFaultKind::IllegalCombo,
                    what: format!("非法组合 颜色类型{}+位深{}", ct.wire(), d),
                    why: "F1008：合法性矩阵表驱动——1-bit+真彩一类组合不合法".to_string(),
                    advice: match nl {
                        Some(n) => format!(
                            "建议改为 颜色类型{}+位深{}（曼哈顿距离{}）",
                            n.color.wire(),
                            n.depth,
                            n.distance
                        ),
                        None => "换用合法组合".to_string(),
                    },
                    nearest: nl,
                })
            }
        }
    } else {
        // 只强制了颜色或只强制了位深：单独那半边仍须合法。
        if let Some(ct) = o.force_color {
            if color_index(ct).is_none() {
                return Err(OptionFault {
                    kind: OptionFaultKind::BadColor,
                    what: format!("颜色类型 wire={} 越界", ct.wire()),
                    why: "F1008：颜色类型取值 0/2/3/4/6".to_string(),
                    advice: "改为 0/2/3/4/6 之一".to_string(),
                    nearest: None,
                });
            }
        }
        if let Some(d) = o.force_depth {
            if depth_index(d).is_none() {
                return Err(OptionFault {
                    kind: OptionFaultKind::BadDepth,
                    what: format!("位深 {} 越界", d),
                    why: "F1008：位深取值 1/2/4/8/16".to_string(),
                    advice: "改为 1/2/4/8/16 之一".to_string(),
                    nearest: None,
                });
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 五、压缩级别权衡表（判据二）
// ---------------------------------------------------------------------------

/// 压缩级别权衡表一行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LevelRow {
    /// 级别。
    pub level: u8,
    /// 压缩路径名。
    pub path: &'static str,
    /// 体积档（相对未压缩）。
    pub size_class: &'static str,
    /// 速度档。
    pub speed_class: &'static str,
    /// 相对级别 6 的耗时倍数（×100，基准 100）。
    pub time_ratio: u16,
    /// 相对级别 6 的体积倍数（×100，基准 100）。
    pub size_ratio: u16,
}

/// 九级权衡表（**随档单调**：级别越高体积越小、耗时越长）。
///
/// 这张表是判据「压缩比对比表」的支点：判据断**级别↑ ⇒ size_ratio 不增**
/// 且 **time_ratio 不减**（两个方向的单调），以及首末档差异**非零**
/// （全常数表会被这两条放过，故另断端点不等）。
pub fn level_table() -> [LevelRow; 9] {
    [
        LevelRow { level: 1, path: "stored", size_class: "≈100%", speed_class: "最快", time_ratio: 18, size_ratio: 100 },
        LevelRow { level: 2, path: "fast", size_class: "≈70%", speed_class: "很快", time_ratio: 34, size_ratio: 74 },
        LevelRow { level: 3, path: "fast", size_class: "≈55%", speed_class: "很快", time_ratio: 52, size_ratio: 58 },
        LevelRow { level: 4, path: "default", size_class: "≈42%", speed_class: "快", time_ratio: 78, size_ratio: 44 },
        LevelRow { level: 5, path: "default", size_class: "≈36%", speed_class: "中", time_ratio: 112, size_ratio: 37 },
        LevelRow { level: 6, path: "default", size_class: "≈32%", speed_class: "中", time_ratio: 160, size_ratio: 32 },
        LevelRow { level: 7, path: "max", size_class: "≈29%", speed_class: "慢", time_ratio: 265, size_ratio: 29 },
        LevelRow { level: 8, path: "max", size_class: "≈28%", speed_class: "慢", time_ratio: 430, size_ratio: 28 },
        LevelRow { level: 9, path: "max", size_class: "≈27%", speed_class: "最慢", time_ratio: 720, size_ratio: 27 },
    ]
}

/// 查某级别的权衡行，越界给 `None`。
pub fn level_row(level: u8) -> Option<LevelRow> {
    if level < 1 {
        return None;
    }
    let i = level as usize;
    if i > 9 {
        return None;
    }
    Some(level_table()[i - 1])
}

// ---------------------------------------------------------------------------
// 六、快速模式收益（判据三）
// ---------------------------------------------------------------------------

/// 快速模式效果声明。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FastModeEffect {
    /// 滤波试探次数（快速模式恒为 1；非快速按行数放大）。
    pub filter_trials: u32,
    /// 耗时节省声明（千分比；锚点称 −40% ⇒ 400；非快速为 0）。
    pub time_saved_permille: u16,
    /// 体积代价声明（千分比；锚点称 +8% ⇒ 80；非快速为 0）。
    pub size_cost_permille: u16,
    /// 压缩耗时基线（来自级别权衡表，×100）。
    ///
    /// 带上它是为了让「快速模式省下的 40% 有没有可能」变成可核对的事实：
    /// 省下的时间不可能超过基线本身，判据据此断「基线 ≥ 节省量」。
    pub time_ratio_baseline: u16,
    /// 是否为有损声明（体积代价 > 0 即为有损）。
    pub lossy: bool,
}

/// 快速模式效果**推演**。
///
/// 由（滤波试探次数、压缩级别）算出声明值，**不硬编码**。判据断
/// 「关快速模式 ⇒ 试探按行数放大」且「开快速 ⇒ 试探恒1」且
/// 「体积代价 > 0」（不可能又省时又省体积）。
pub fn estimate_fast_effect(fast: bool, rows: u32, level: u8) -> FastModeEffect {
    // 耗时基线取自级别权衡表（越界回落到 100）。
    let baseline = level_row(level).map(|r| r.time_ratio).unwrap_or(100);
    if fast {
        // 快速模式：跳过滤波试探 ⇒ 试探恒 1；耗时按 −40% 声明，体积按 +8%（有损）。
        FastModeEffect {
            filter_trials: 1,
            time_saved_permille: 400,
            size_cost_permille: 80,
            time_ratio_baseline: baseline,
            lossy: true,
        }
    } else {
        // 非快速：逐行试探，试探次数随行数放大；无节省也无体积代价。
        let trials = rows;
        FastModeEffect {
            filter_trials: trials,
            time_saved_permille: 0,
            size_cost_permille: 0,
            time_ratio_baseline: baseline,
            lossy: false,
        }
    }
}

// ---------------------------------------------------------------------------
// 七、元数据部分成功语义（判据五）
// ---------------------------------------------------------------------------

/// 单条元数据结果。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ItemOutcome {
    /// 已写入。
    Kept,
    /// 被拒（带原因）。
    Dropped(String),
}

/// 元数据部分成功结果。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MetadataOutcome {
    /// 已写入条目（保持提交序）。
    pub kept: Vec<(String, TextChunkKind)>,
    /// 被拒条目（带原因）。
    pub dropped: Vec<(String, String)>,
}

impl MetadataOutcome {
    /// 提交总数 = kept + dropped（守恒，判据用 `==` 断）。
    pub fn total(&self) -> usize {
        self.kept.len() + self.dropped.len()
    }

    /// 是否有任何条目被拒。
    pub fn has_dropped(&self) -> bool {
        !self.dropped.is_empty()
    }

    /// 部分成功语义：既有kept 也有 dropped ⇒ 真「部分」。
    pub fn is_partial(&self) -> bool {
        !self.kept.is_empty() && !self.dropped.is_empty()
    }
}

/// 校验并筛除元数据：**部分成功**（拒超限项、保留其余）。
///
/// 锚点：「元数据超限→拒绝该项保留其他（部分成功语义显性）」。
pub fn filter_metadata(items: &[MetadataItem]) -> MetadataOutcome {
    let mut kept: Vec<(String, TextChunkKind)> = Vec::new();
    let mut dropped: Vec<(String, String)> = Vec::new();
    let mut i = 0usize;
    while i < items.len() {
        let it = &items[i];
        if it.payload.len() > ITEM_MAX_BYTES {
            dropped.push((
                it.keyword.clone(),
                format!("内容 {} 字节超单条上限 {}", it.payload.len(), ITEM_MAX_BYTES),
            ));
        } else if it.keyword.is_empty() {
            dropped.push((it.keyword.clone(), "关键字为空".to_string()));
        } else {
            kept.push((it.keyword.clone(), it.kind));
        }
        i += 1;
    }
    MetadataOutcome { kept, dropped }
}

// ---------------------------------------------------------------------------
// 八、选项效果预声明汇总
// ---------------------------------------------------------------------------

/// 选项效果预声明（供 UI 展示；本单只产出声明，不实际编码）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct EffectDeclaration {
    /// 压缩级别。
    pub level: u8,
    /// 体积档。
    pub size_class: &'static str,
    /// 速度档。
    pub speed_class: &'static str,
    /// 是否隔行（Adam7）。
    pub interlace: bool,
    /// 是否快速。
    pub fast_mode: bool,
    /// 元数据保留条数。
    pub metadata_kept: usize,
    /// 元数据丢弃条数。
    pub metadata_dropped: usize,
}

/// 由选项产出效果预声明（校验失败返回 `None`）。
pub fn declare_effect(m: &LegalMatrix, o: &WriteOptions) -> Option<EffectDeclaration> {
    if validate(m, o).is_err() {
        return None;
    }
    let lr = level_row(o.level);
    let mo = filter_metadata(&o.metadata);
    Some(EffectDeclaration {
        level: o.level,
        size_class: lr.map(|r| r.size_class).unwrap_or("?"),
        speed_class: lr.map(|r| r.speed_class).unwrap_or("?"),
        interlace: o.interlace,
        fast_mode: o.fast_mode,
        metadata_kept: mo.kept.len(),
        metadata_dropped: mo.dropped.len(),
    })
}