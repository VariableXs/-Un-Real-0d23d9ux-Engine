//! CGPU-F0357 · 视频播放场景配方（判据段 0x08B0）
//! 视频配方：视频播放保障（解码/渲染/音频三账户联动保障——播放
//! 不掉帧不掉音）；多路播放配额（画中画+主画面的分配比例表）；
//! 与 VE-G 域契约（G 域预算标签→本域配方映射——跨卷对接）；低功
//! 耗播放模式（音频为主、画面暂停的省电配方）。判据：三账户联动、
//! 配额表、跨卷映射、省电、判据。
//!
//! 锚点原文摘录（施工书 1591 行）：
//! 「视频播放保障（解码/渲染/音频三账户联动保障——播放不掉帧不
//! 掉音）；多路播放配额（画中画+主画面的分配比例表）；与 VE-G 域
//! 契约（G 域预算标签→本域配方映射）；低功耗播放模式（音频为主、
//! 画面暂停）。判据：三账户联动、配额表、跨卷映射、省电、判据。」
//!
//! 诊断码段 0x08B0（本模块独占：0x08B0~0x08BF）。
//! no_std + alloc 零 unwrap（测试除外），全部 Result 化。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x08B0;

/// 三账户权重基准（千分比）：解码 400 / 渲染 400 / 音频 200。
/// 来源：判据「播放不掉帧不掉音」——音频账户最薄但不可为零，
/// 掉音比掉帧对播放体验破坏更大，故解码渲染对半、音频保底。
pub const DECODE_WEIGHT_PERMILLE: u32 = 400;
pub const RENDER_WEIGHT_PERMILLE: u32 = 400;
pub const AUDIO_WEIGHT_PERMILLE: u32 = 200;

/// 音频账户最低保底（千分比）：任何裁剪不得把音频裁到 150 以下。
pub const AUDIO_FLOOR_PERMILLE: u32 = 150;

/// 主画面 vs 画中画默认比例（判据「配额表」）：主 750 / 画中画 250。
pub const MAIN_VS_PIP_RATIO_PERMILLE: (u32, u32) = (750, 250);

/// 画中画最大路数（判据「配额表」边界）：超过即拒。
pub const PIP_MAX_LANES: usize = 4;

/// 视频账户三元组（判据「三账户联动」的联动对象）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VideoAccount {
    /// 解码账户。
    Decode,
    /// 渲染账户。
    Render,
    /// 音频账户。
    Audio,
}

impl VideoAccount {
    /// 闭集遍历序（判定与表构建依赖稳定顺序）。
    pub fn all() -> [VideoAccount; 3] {
        [VideoAccount::Decode, VideoAccount::Render, VideoAccount::Audio]
    }
    /// 基准权重（千分比）。
    pub fn base_weight(&self) -> u32 {
        match self {
            VideoAccount::Decode => DECODE_WEIGHT_PERMILLE,
            VideoAccount::Render => RENDER_WEIGHT_PERMILLE,
            VideoAccount::Audio => AUDIO_WEIGHT_PERMILLE,
        }
    }
    /// 名称（诊断与遥测面）。
    pub fn name(&self) -> &'static str {
        match self {
            VideoAccount::Decode => "decode",
            VideoAccount::Render => "render",
            VideoAccount::Audio => "audio",
        }
    }
}

/// 配方输入：帧预算（千分比，恒 1000 口径）与播放路数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RecipeInput {
    /// 帧预算千分比（=1000）。
    pub budget_permille: u32,
    /// 主画面路数（0 或 1）。
    pub main_lanes: u32,
    /// 画中画路数（0..=PIP_MAX_LANES）。
    pub pip_lanes: u32,
}

impl RecipeInput {
    /// 合法性。
    pub fn validate(&self) -> Result<(), u16> {
        if self.budget_permille != 1000 {
            return Err(SEG_BASE);
        }
        if self.main_lanes > 1 {
            return Err(SEG_BASE + 1);
        }
        if self.pip_lanes as usize > PIP_MAX_LANES {
            return Err(SEG_BASE + 2);
        }
        Ok(())
    }
    /// 总播放路数。
    pub fn total_lanes(&self) -> u32 {
        self.main_lanes + self.pip_lanes
    }
}

/// 三账户分配结果（判据「三账户联动」产出）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AccountSplit {
    /// 解码份额（千分比）。
    pub decode: u32,
    /// 渲染份额（千分比）。
    pub render: u32,
    /// 音频份额（千分比）。
    pub audio: u32,
}

impl AccountSplit {
    /// 守恒核：三分和恰等于预算。
    pub fn sum(&self) -> u32 {
        self.decode + self.render + self.audio
    }
    /// 音频保底核（判据「不掉音」硬约束）。
    pub fn audio_floor_ok(&self) -> bool {
        self.audio >= AUDIO_FLOOR_PERMILLE
    }
}

/// 三账户联动分配（判据「三账户联动」主判据）：按基准权重切分
/// 帧预算；预算缩水时按「音频保底优先、解码渲染等比」的次序裁剪。
pub fn split_accounts(input: &RecipeInput) -> Result<AccountSplit, u16> {
    input.validate()?;
    let budget = input.budget_permille;
    // 第一步：音频先取保底与基准权重的较小者（全程 u32——千分比域内无溢出）。
    let audio = AUDIO_FLOOR_PERMILLE.min(AUDIO_WEIGHT_PERMILLE);
    let rest = budget - audio;
    // 第二步：解码/渲染按 1:1 分余量（权重对半）。
    let decode = rest * DECODE_WEIGHT_PERMILLE
        / (DECODE_WEIGHT_PERMILLE + RENDER_WEIGHT_PERMILLE);
    let render = rest - decode;
    let out = AccountSplit {
        decode,
        render,
        audio: audio as u32,
    };
    if out.sum() != input.budget_permille {
        return Err(SEG_BASE + 3);
    }
    if !out.audio_floor_ok() {
        return Err(SEG_BASE + 4);
    }
    Ok(out)
}

/// 多路配额行（判据「配额表」一等公民）：路号 + 份额千分比。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LaneQuota {
    /// 路号（0 = 主画面，1..=PIP_MAX_LANES = 画中画）。
    pub lane: u32,
    /// 份额（千分比）。
    pub share_permille: u32,
    /// 是否主画面。
    pub is_main: bool,
}

impl LaneQuota {
    /// 合法性。
    pub fn validate(&self) -> Result<(), u16> {
        if self.share_permille == 0 || self.share_permille > 1000 {
            return Err(SEG_BASE + 5);
        }
        if self.is_main != (self.lane == 0) {
            return Err(SEG_BASE + 6);
        }
        Ok(())
    }
}

/// 多路播放配额表（判据「配额表」主判据）：主画面取
/// MAIN_VS_PIP_RATIO_PERMILLE，画中画等分余量；无画中画时主画面
/// 独占。守恒：Σ行份额 ≡ 1000。
pub fn lane_quota_table(input: &RecipeInput) -> Result<Vec<LaneQuota>, u16> {
    input.validate()?;
    let mut out = Vec::new();
    if input.total_lanes() == 0 {
        return Err(SEG_BASE + 7);
    }
    if input.pip_lanes == 0 {
        out.push(LaneQuota { lane: 0, share_permille: 1000, is_main: true });
        return Ok(out);
    }
    let (main, _) = MAIN_VS_PIP_RATIO_PERMILLE;
    // 主画面按比例表取整，画中画等分余量——余量全部派完保证守恒。
    let per_pip = (1000 - main) / input.pip_lanes;
    let assigned_pip = per_pip * input.pip_lanes;
    out.push(LaneQuota { lane: 0, share_permille: main + (1000 - main - assigned_pip), is_main: true });
    for i in 1..=input.pip_lanes {
        out.push(LaneQuota { lane: i, share_permille: per_pip, is_main: false });
    }
    let sum: u32 = out.iter().map(|q| q.share_permille).sum();
    if sum != 1000 {
        return Err(SEG_BASE + 8);
    }
    for q in &out {
        q.validate()?;
    }
    Ok(out)
}

/// VE-G 域预算标签（判据「跨卷映射」的 G 侧输入——G 域只给标签，
/// 本域负责映射为配方参数）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GBudgetLabel {
    /// G 域充裕档。
    Plentiful,
    /// G 域紧凑档。
    Tight,
    /// G 域枯竭档。
    Starved,
}

impl GBudgetLabel {
    /// 档位号（映射表序）。
    pub fn ordinal(&self) -> u8 {
        match self {
            GBudgetLabel::Plentiful => 0,
            GBudgetLabel::Tight => 1,
            GBudgetLabel::Starved => 2,
        }
    }
}

/// 跨卷映射结果（判据「跨卷映射」产出）：G 标签 → 本域配方档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MappedProfile {
    /// 渲染分辨率档（1000 = 全分辨率）。
    pub res_scale_permille: u32,
    /// 解码最大并发路数。
    pub max_decode_lanes: u32,
    /// 是否允许画中画。
    pub pip_allowed: bool,
}

impl MappedProfile {
    /// 合法性：分辨率档 ∈ (0,1000]。
    pub fn validate(&self) -> Result<(), u16> {
        if self.res_scale_permille == 0 || self.res_scale_permille > 1000 {
            return Err(SEG_BASE + 9);
        }
        Ok(())
    }
}

/// G 标签 → 配方映射表（判据「跨卷映射」主判据）：映射必须是
/// 全函数——三个标签各有一行，且档位随标签恶化单调收紧。
pub fn map_from_g_label(label: GBudgetLabel) -> MappedProfile {
    match label {
        GBudgetLabel::Plentiful => MappedProfile { res_scale_permille: 1000, max_decode_lanes: 8, pip_allowed: true },
        GBudgetLabel::Tight => MappedProfile { res_scale_permille: 640, max_decode_lanes: 4, pip_allowed: true },
        GBudgetLabel::Starved => MappedProfile { res_scale_permille: 360, max_decode_lanes: 1, pip_allowed: false },
    }
}

/// 映射单调性核（判据「跨卷映射」守卫）：标签序号增大 → 分辨率档
/// 与解码路数单调不增。
pub fn map_monotonic_ok() -> Result<(), u16> {
    let a = map_from_g_label(GBudgetLabel::Plentiful);
    let b = map_from_g_label(GBudgetLabel::Tight);
    let c = map_from_g_label(GBudgetLabel::Starved);
    for m in [&a, &b, &c] {
        m.validate()?;
    }
    if !(a.res_scale_permille >= b.res_scale_permille
        && b.res_scale_permille >= c.res_scale_permille)
    {
        return Err(SEG_BASE + 10);
    }
    if !(a.max_decode_lanes >= b.max_decode_lanes
        && b.max_decode_lanes >= c.max_decode_lanes)
    {
        return Err(SEG_BASE + 11);
    }
    Ok(())
}

/// 低功耗播放档位（判据「省电」的档位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PowerPlayMode {
    /// 正常播放。
    Normal,
    /// 省电：音频为主，画面降帧。
    AudioFirst,
    /// 后台播放：画面暂停，纯音频。
    Background,
}

impl PowerPlayMode {
    /// 档位号。
    pub fn ordinal(&self) -> u8 {
        match self {
            PowerPlayMode::Normal => 0,
            PowerPlayMode::AudioFirst => 1,
            PowerPlayMode::Background => 2,
        }
    }
    /// 画面供给开关。
    pub fn render_enabled(&self) -> bool {
        !matches!(self, PowerPlayMode::Background)
    }
}

/// 省电配额变换（判据「省电」主判据）：AudioFirst 档画面减半、
/// 音频保底提升到基准权重；Background 档画面归零、音频独占。
/// 音频保底线在任何档位下不得击穿。
pub fn power_mode_split(mode: PowerPlayMode, base: &AccountSplit) -> Result<AccountSplit, u16> {
    if base.sum() != 1000 {
        return Err(SEG_BASE + 12);
    }
    let out = match mode {
        PowerPlayMode::Normal => *base,
        PowerPlayMode::AudioFirst => AccountSplit {
            decode: base.decode / 2,
            render: base.render / 2,
            audio: 1000 - base.decode / 2 - base.render / 2,
        },
        PowerPlayMode::Background => AccountSplit { decode: 0, render: 0, audio: 1000 },
    };
    if out.sum() != 1000 {
        return Err(SEG_BASE + 13);
    }
    if !mode.render_enabled() && out.render != 0 {
        return Err(SEG_BASE + 14);
    }
    if !out.audio_floor_ok() {
        return Err(SEG_BASE + 15);
    }
    Ok(out)
}

/// 省电收益（判据「省电」验收面）：档位升高 → 渲染供给单调不增。
pub fn power_mode_monotonic_ok() -> Result<(), u16> {
    if PowerPlayMode::Normal.ordinal() >= PowerPlayMode::AudioFirst.ordinal()
        || PowerPlayMode::AudioFirst.ordinal() >= PowerPlayMode::Background.ordinal()
    {
        return Err(SEG_BASE + 16);
    }
    if !PowerPlayMode::Normal.render_enabled() || !PowerPlayMode::AudioFirst.render_enabled()
        || PowerPlayMode::Background.render_enabled()
    {
        return Err(SEG_BASE + 17);
    }
    Ok(())
}

/// 配方指纹（可审计面）：输入+分配+模式的 FNV 链。
pub fn recipe_fp(input: &RecipeInput, split: &AccountSplit, mode: PowerPlayMode) -> u64 {
    let mut acc: u64 = 0xcbf29ce484222325;
    for v in [
        input.budget_permille as u64,
        input.main_lanes as u64,
        input.pip_lanes as u64,
        split.decode as u64,
        split.render as u64,
        split.audio as u64,
        mode.ordinal() as u64,
    ] {
        acc = acc.wrapping_mul(0x100000001b3) ^ v;
    }
    acc
}

/// 配方诊断明细（错误码 → 中文可操作建议，判据「判据」可读面）。
pub fn diagnose(code: u16) -> &'static str {
    let d = code.wrapping_sub(SEG_BASE);
    match d {
        0 => "帧预算千分比必须恒等于 1000（守恒口径）",
        1 => "主画面路数只能 0 或 1",
        2 => "画中画路数超上限（PIP_MAX_LANES=4）",
        3 => "三账户分配和≠预算——分配器守恒被破坏",
        4 => "音频份额击穿保底线——掉音风险",
        5 => "路份额必须 ∈ (0,1000]",
        6 => "主画面标志与路号矛盾（路 0 才是主画面）",
        7 => "总播放路数为 0——无路可配",
        8 => "配额表 Σ≠1000——派额不守恒",
        9 => "分辨率档必须 ∈ (0,1000]",
        10 => "映射分辨率档未随标签恶化单调收紧",
        11 => "映射解码路数未随标签恶化单调收紧",
        12 => "基准分配 Σ≠1000——输入非法",
        13 => "省电变换后 Σ≠1000——变换器守恒被破坏",
        14 => "画面暂停档渲染份额必须为 0",
        15 => "省电档音频击穿保底线",
        16 => "省电档位序号必须严格递增",
        17 => "画面供给开关与档位语义矛盾",
        _ => "未知诊断码——查本模块 SEG_BASE 0x08B0",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_account_linkage() {
        let input = RecipeInput { budget_permille: 1000, main_lanes: 1, pip_lanes: 0 };
        let s = split_accounts(&input).unwrap();
        assert_eq!(s.sum(), 1000);
        assert!(s.audio_floor_ok());
        // 音频保底 150，解码渲染等分 850。
        assert_eq!(s.audio, 150);
        assert_eq!(s.decode, 425);
        assert_eq!(s.render, 425);
    }

    #[test]
    fn lane_quota_conservation() {
        // 无画中画：主画面独占。
        let a = RecipeInput { budget_permille: 1000, main_lanes: 1, pip_lanes: 0 };
        let ta = lane_quota_table(&a).unwrap();
        assert_eq!(ta.len(), 1);
        assert_eq!(ta[0].share_permille, 1000);
        // 一路画中画：750/250。
        let b = RecipeInput { budget_permille: 1000, main_lanes: 1, pip_lanes: 1 };
        let tb = lane_quota_table(&b).unwrap();
        let sum: u32 = tb.iter().map(|q| q.share_permille).sum();
        assert_eq!(sum, 1000);
        assert_eq!(tb[0].share_permille, MAIN_VS_PIP_RATIO_PERMILLE.0 + (1000 - 750) - (1000 - 750) / 1);
        // 四路画中画守恒且逐行合法。
        let c = RecipeInput { budget_permille: 1000, main_lanes: 1, pip_lanes: 4 };
        let tc = lane_quota_table(&c).unwrap();
        assert_eq!(tc.len(), 5);
        let sum: u32 = tc.iter().map(|q| q.share_permille).sum();
        assert_eq!(sum, 1000);
        for q in &tc {
            assert!(q.validate().is_ok());
        }
    }

    #[test]
    fn g_label_mapping_monotonic() {
        assert!(map_monotonic_ok().is_ok());
        let p = map_from_g_label(GBudgetLabel::Plentiful);
        let s = map_from_g_label(GBudgetLabel::Starved);
        assert_eq!(p.res_scale_permille, 1000);
        assert_eq!(s.res_scale_permille, 360);
        assert!(!s.pip_allowed);
    }

    #[test]
    fn power_mode_semantics() {
        let input = RecipeInput { budget_permille: 1000, main_lanes: 1, pip_lanes: 0 };
        let base = split_accounts(&input).unwrap();
        let bg = power_mode_split(PowerPlayMode::Background, &base).unwrap();
        assert_eq!(bg.render, 0);
        assert_eq!(bg.audio, 1000);
        let af = power_mode_split(PowerPlayMode::AudioFirst, &base).unwrap();
        assert_eq!(af.sum(), 1000);
        assert!(af.audio > base.audio);
        assert!(power_mode_monotonic_ok().is_ok());
        let fp = recipe_fp(&input, &bg, PowerPlayMode::Background);
        assert!(fp != 0);
    }

    #[test]
    fn rejects_and_diagnostics() {
        let bad = RecipeInput { budget_permille: 999, main_lanes: 1, pip_lanes: 0 };
        let e = split_accounts(&bad).unwrap_err();
        assert_eq!(e, SEG_BASE);
        assert!(diagnose(e).contains("1000"));
        let over_pip = RecipeInput { budget_permille: 1000, main_lanes: 1, pip_lanes: 5 };
        assert_eq!(split_accounts(&over_pip).unwrap_err(), SEG_BASE + 2);
        let no_lane = RecipeInput { budget_permille: 1000, main_lanes: 0, pip_lanes: 0 };
        assert_eq!(lane_quota_table(&no_lane).unwrap_err(), SEG_BASE + 7);
        assert!(diagnose(0x1234).contains("未知"));
    }
}
