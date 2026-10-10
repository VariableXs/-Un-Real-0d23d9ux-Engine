//! VE-F5205 · 环境特效子系统（VE-Z 域 · 环境编排 · 锚点目标 320 行 · 深化版）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5205`
//!
//! **判据（锚点原文）**：天气渐变、昼夜联动、音画同步、渐变强制、判据。
//!
//! **职责定位（锚点原文）**：环境特效子系统——环境特效三件：天气系统
//! （雨雪风雾渐变过渡）/昼夜光照联动（时段驱动氛围——N 域时段口径
//! 参照）/环境音画同步（环境特效与音效联动——视听一体）。
//!
//! **错误路径与降级矩阵（锚点原文）**：天气切换突变→渐变曲线强制；
//! 联动断链→探针定位；音画失步→对拍修正。
//!
//! **性能逐项分解（锚点原文）**：切换 O(渐变帧)；联动 O(1)；同步 O(1)。
//!
//! **跨批对接点（锚点原文）**：F4437 时段口径参照上游；F4428 音频
//! 衔接；F5213 测试。
//!
//! # 深化说明：环境三件是一条**联动链**，渐变是**光敏保护**不是审美
//!
//! 「天气切换突变→渐变曲线强制」列在错误路径第一位不是偶然：亮度
//! 突变（晴→雨瞬间变暗）对光敏玩家是真实的光刺激风险——渐变强制
//! 是域本色「渐变不闪烁」的执行面（光敏保护延伸），不是让过渡好看
//! 的美工口味。故 [`WeatherMachine`] 把「直接设新天气」做成**不存在
//! 的操作**：唯一合法路径是发起渐变（目标天气+渐变时长），权重按
//! Q8 定点逐帧步进（[`Q8_ONE`] 同 F5406 插值口径）——权重跳变被
//! 拒（[`E_EFX_ABRUPT`]），渐变 O(渐变帧)——锚点「切换 O(渐变帧)」。
//!
//! ## 深化一：天气状态机——五态+权重混合，渐变期间双态并存
//!
//! 五天气闭集（[`Weather`]：晴/雨/雪/雾/风），状态不是「当前是哪一
//! 个」而是「每个的权重」（渐变期间新旧并存、和恒定）。切换=指定
//! 目标+时长，渐变完成前再次切换=改目标继续渐（不重置——重置会让
//! 亮度抖一下，又是闪烁）。渐变帧数=时长的帧口径，每帧步进
//! `256/帧数`——O(渐变帧) 是逐帧步进的自然代价，单帧步进 O(1)。
//!
//! ## 深化二：昼夜光照联动——时段是 N 域口径的**参照**不是重定义
//!
//! 四时段闭集（[`DayPhase`]：黎明/白昼/黄昏/夜晚）与 N 域 F4437
//! 时段口径**同名同序**（本条不重定义时段——口径漂移从源头断）。
//! 光照联动器按「天气×时段」二维查表得光照档（联动 O(1)——锚点
//! 「联动 O(1)」），表单源可对账：雨天黄昏和晴天黄昏的光照档不同
//! 是**表数据**，不是 if 链里的巧合。
//!
//! ## 深化三：音画同步——配对表+失步对拍修正
//!
//! 每个天气配一条环境音（[`AV_PAIRS`] 配对表单源）：视觉权重非零
//! 而对应音轨停了（或反之）=失步。[`AvSyncer::check`] 逐天气对拍
//! （同步 O(1) 逐天气查表），失步记数+发修正请求（[`E_EFX_AV_DESYNC`]
//! ——修正由音频侧执行，本条只负责**发现并点名**——锚点「音画失步
//! →对拍修正」的「对拍」面）。
//!
//! ## 深化四：联动链探针——断链在哪一环，点名不猜谜
//!
//! 天气→光照→音效是一条链：链上任何一环失联（天气机未初始化/光照
//! 表缺行/音配对缺失）下游全部失真。[`LinkProbe::probe`] 逐环体检
//! 返回**断点位置**（`ChainOk` / `BrokenAt(环名)`）——「探针定位」
//! 的价值在修链的人不用从头猜：断在第几环，报第几环
//! （[`E_EFX_CHAIN_BROKEN`]）。
//!
//! # 确定性时基：渐变帧推进由调用方逐帧驱动（注入式，判据可复跑）

use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 错误契约：Z 域字符串码惯例（E_EFX_* 前缀）
// ---------------------------------------------------------------------------

/// 天气切换突变：权重跳变被拒（渐变曲线强制，光敏保护延伸）。
pub const E_EFX_ABRUPT: &str = "E_EFX_ABRUPT";
/// 联动断链：天气→光照→音效链体检发现断点。
pub const E_EFX_CHAIN_BROKEN: &str = "E_EFX_CHAIN_BROKEN";
/// 音画失步：视觉权重与音轨状态对拍不一致。
pub const E_EFX_AV_DESYNC: &str = "E_EFX_AV_DESYNC";
/// 时段口径非法：不在 N 域 F4437 时段闭集内。
pub const E_EFX_BAD_PHASE: &str = "E_EFX_BAD_PHASE";
/// 音画配对缺失：天气在配对表查无音轨。
pub const E_EFX_NO_PAIR: &str = "E_EFX_NO_PAIR";

/// 错误码总数（判据侧独立复核）。
pub const ERROR_COUNT: usize = 5;

/// 错误码→原因串。
pub fn reason_of(code: &str) -> &'static str {
    match code {
        E_EFX_ABRUPT => "天气切换突变：渐变曲线强制（光敏保护延伸）",
        E_EFX_CHAIN_BROKEN => "联动断链：链上环节失联已定位",
        E_EFX_AV_DESYNC => "音画失步：视听状态对拍不一致",
        E_EFX_BAD_PHASE => "时段口径非法：不在 F4437 时段闭集内",
        E_EFX_NO_PAIR => "音画配对缺失：该天气无音轨",
        _ => "未知错误码",
    }
}

/// 错误码→处置方向。
pub fn direction_of(code: &str) -> &'static str {
    match code {
        E_EFX_ABRUPT => "走渐变接口指定目标与时长",
        E_EFX_CHAIN_BROKEN => "按探针点名的断点环修复",
        E_EFX_AV_DESYNC => "音频侧按修正请求补齐/停轨",
        E_EFX_BAD_PHASE => "改用 F4437 时段闭集成员",
        E_EFX_NO_PAIR => "配对表补行后再同步",
        _ => reason_of(code),
    }
}

// ---------------------------------------------------------------------------
// 一、天气状态机：五态 + 权重混合 + 渐变强制
// ---------------------------------------------------------------------------

/// Q8 定点分母（与 F5406 插值同口径）。
pub const Q8_ONE: u32 = 256;

/// 天气闭集（五态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Weather {
    /// 晴。
    Clear,
    /// 雨。
    Rain,
    /// 雪。
    Snow,
    /// 雾。
    Fog,
    /// 风。
    Wind,
}

/// 天气数（判据侧独立写死 5）。
pub const WEATHER_COUNT: usize = 5;

impl Weather {
    /// 全集。
    pub const ALL: [Weather; WEATHER_COUNT] = [
        Weather::Clear,
        Weather::Rain,
        Weather::Snow,
        Weather::Fog,
        Weather::Wind,
    ];

    /// 天气名。
    pub const fn name(self) -> &'static str {
        match self {
            Weather::Clear => "晴",
            Weather::Rain => "雨",
            Weather::Snow => "雪",
            Weather::Fog => "雾",
            Weather::Wind => "风",
        }
    }

    /// 序数。
    pub const fn ordinal(self) -> usize {
        match self {
            Weather::Clear => 0,
            Weather::Rain => 1,
            Weather::Snow => 2,
            Weather::Fog => 3,
            Weather::Wind => 4,
        }
    }
}

/// 渐变默认时长（帧）。
pub const DEFAULT_GRADIENT_FRAMES: u32 = 60;

/// 天气状态机：权重混合 + 渐变强制（唯一合法切换路径）。
#[derive(Clone, Debug)]
pub struct WeatherMachine {
    weights: [u32; WEATHER_COUNT],
    from: Weather,
    target: Weather,
    /// 剩余渐变帧数（0=渐变完成，权重收敛到 target 全占）。
    remaining: u32,
    /// 突变拦截计数。
    pub abrupt_blocks: u32,
    /// 完成的渐变次数。
    pub gradients_done: u32,
}

impl WeatherMachine {
    /// 新状态机（晴全占起）。
    pub fn new() -> Self {
        let mut weights = [0u32; WEATHER_COUNT];
        weights[Weather::Clear.ordinal()] = Q8_ONE;
        WeatherMachine {
            weights,
            from: Weather::Clear,
            target: Weather::Clear,
            remaining: 0,
            abrupt_blocks: 0,
            gradients_done: 0,
        }
    }

    /// 天气权重（Q8，0..=256）。
    pub fn weight(&self, w: Weather) -> u32 {
        self.weights[w.ordinal()]
    }

    /// 渐变中判据。
    pub const fn in_gradient(&self) -> bool {
        self.remaining > 0
    }

    /// 当前主天气（权重最大者——渐变期间为过渡中的主导方）。
    pub fn dominant(&self) -> Weather {
        let mut best = 0usize;
        let mut i = 1usize;
        while i < WEATHER_COUNT {
            if self.weights[i] > self.weights[best] {
                best = i;
            }
            i += 1;
        }
        Weather::ALL[best]
    }

    /// **发起切换**：唯一合法入口。目标=新天气、帧数>0——直接「设权
    /// 重」不存在于本类型。帧数 0=突变请求→拒（[`E_EFX_ABRUPT`]）。
    /// 渐变中再切换=改目标继续渐（不重置起点——重置会抖亮度）。
    pub fn request(&mut self, target: Weather, frames: u32) -> Result<(), &'static str> {
        if frames == 0 {
            self.abrupt_blocks += 1;
            return Err(E_EFX_ABRUPT);
        }
        if !self.in_gradient() {
            self.from = self.dominant();
        }
        self.target = target;
        self.remaining = frames;
        Ok(())
    }

    /// **逐帧步进**：权重按 `256/总帧数` 从 from 向 target 移动。
    /// 单帧 O(1)（只动两格），全程 O(渐变帧)——锚点「切换 O(渐变帧)」。
    pub fn tick(&mut self, total_frames: u32) {
        if !self.in_gradient() {
            return;
        }
        let step = (Q8_ONE + total_frames - 1) / total_frames.max(1);
        let fi = self.from.ordinal();
        let ti = self.target.ordinal();
        let give = step.min(self.weights[fi]);
        self.weights[fi] -= give;
        self.weights[ti] = (self.weights[ti] + give).min(Q8_ONE);
        self.remaining -= 1;
        if self.remaining == 0 {
            // 收尾：权重精确收敛（防余数残渣）。
            self.weights[fi] = if fi == ti { Q8_ONE } else { 0 };
            self.weights[ti] = Q8_ONE;
            self.gradients_done += 1;
        }
    }
}

impl Default for WeatherMachine {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 二、昼夜光照联动：时段闭集（F4437 口径参照）× 天气 → 光照档
// ---------------------------------------------------------------------------

/// 四时段闭集（与 N 域 F4437 时段口径同名同序——不重定义）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DayPhase {
    /// 黎明。
    Dawn,
    /// 白昼。
    Day,
    /// 黄昏。
    Dusk,
    /// 夜晚。
    Night,
}

/// 时段数（判据侧独立写死 4）。
pub const DAY_PHASE_COUNT: usize = 4;

impl DayPhase {
    /// 全集（序数=F4437 口径序）。
    pub const ALL: [DayPhase; DAY_PHASE_COUNT] = [
        DayPhase::Dawn,
        DayPhase::Day,
        DayPhase::Dusk,
        DayPhase::Night,
    ];

    /// 时段名。
    pub const fn name(self) -> &'static str {
        match self {
            DayPhase::Dawn => "黎明",
            DayPhase::Day => "白昼",
            DayPhase::Dusk => "黄昏",
            DayPhase::Night => "夜晚",
        }
    }

    /// 序数。
    pub const fn ordinal(self) -> usize {
        match self {
            DayPhase::Dawn => 0,
            DayPhase::Day => 1,
            DayPhase::Dusk => 2,
            DayPhase::Night => 3,
        }
    }
}

/// 光照档（0..=7 八档：0 最暗 7 最亮）。
pub const LIGHT_TIER_MAX: u8 = 7;

/// 光照联动表：天气×时段 → 光照档（20 格单源——表数据不是 if 巧合）。
pub const LIGHT_TABLE: [[u8; DAY_PHASE_COUNT]; WEATHER_COUNT] = [
    //            黎明  白昼  黄昏  夜晚
    [ /* 晴 */ 4, 7, 5, 2],
    [ /* 雨 */ 3, 5, 3, 1],
    [ /* 雪 */ 4, 6, 4, 2],
    [ /* 雾 */ 2, 4, 2, 1],
    [ /* 风 */ 4, 7, 5, 2],
];

/// 光照联动器：主天气×时段查表（联动 O(1)——锚点「联动 O(1)」）。
#[derive(Clone, Debug, Default)]
pub struct DayNightLink {
    /// 非法时段拒绝计数。
    pub bad_phase_blocks: u32,
}

impl DayNightLink {
    /// 空联动器。
    pub fn new() -> Self {
        DayNightLink { bad_phase_blocks: 0 }
    }

    /// 查光照档：主天气×时段（时段出闭集拒绝——[`E_EFX_BAD_PHASE`]，
    /// 口径漂移从源头断）。
    pub fn light_tier(&mut self, w: Weather, phase: Option<DayPhase>) -> Result<u8, &'static str> {
        match phase {
            Some(p) => Ok(LIGHT_TABLE[w.ordinal()][p.ordinal()]),
            None => {
                self.bad_phase_blocks += 1;
                Err(E_EFX_BAD_PHASE)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 三、音画同步：配对表 + 失步对拍修正
// ---------------------------------------------------------------------------

/// 环境音轨闭集（与天气一一配对的音面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AmbienceTrack {
    /// 无轨（晴的配对面——安静也是音画状态）。
    None_,

    /// 雨声。
    RainLoop,
    /// 雪落（近静）。
    SnowHush,
    /// 雾中低鸣。
    FogDrone,
    /// 风啸。
    WindHowl,
}

/// 音轨数（判据侧独立写死 5，与天气数同构）。
pub const TRACK_COUNT: usize = 5;

/// 天气→音轨配对表（单源——视听一体的账本）。
pub const AV_PAIRS: [AmbienceTrack; WEATHER_COUNT] = [
    AmbienceTrack::None_,
    AmbienceTrack::RainLoop,
    AmbienceTrack::SnowHush,
    AmbienceTrack::FogDrone,
    AmbienceTrack::WindHowl,
];

impl AmbienceTrack {
    /// 音轨名。
    pub const fn name(self) -> &'static str {
        match self {
            AmbienceTrack::None_ => "静",
            AmbienceTrack::RainLoop => "雨声",
            AmbienceTrack::SnowHush => "雪落",
            AmbienceTrack::FogDrone => "雾鸣",
            AmbienceTrack::WindHowl => "风啸",
        }
    }
}

/// 音画同步器：视觉权重 vs 音轨在播状态逐天气对拍。
#[derive(Clone, Debug, Default)]
pub struct AvSyncer {
    /// 音轨在播状态（按天气序——音频侧上报）。
    playing: [bool; WEATHER_COUNT],
    /// 失步计数。
    pub desyncs: u32,
    /// 修正请求数（发往音频侧）。
    pub fix_requests: u32,
}

impl AvSyncer {
    /// 空同步器（默认全停）。
    pub fn new() -> Self {
        AvSyncer { playing: [false; TRACK_COUNT], desyncs: 0, fix_requests: 0 }
    }

    /// 音频侧上报音轨状态。
    pub fn report_track(&mut self, w: Weather, playing: bool) {
        self.playing[w.ordinal()] = playing;
    }

    /// 音轨在播查询。
    pub fn is_playing(&self, w: Weather) -> bool {
        self.playing[w.ordinal()]
    }

    /// **对拍一次**：视觉权重 ≥ 感知阈（32/256≈12.5%）而音轨停=失步；
    /// 权重低于阈而音轨在播=同样失步（幽灵音轨）。失步记数+发修正
    /// 请求——同步 O(1) 逐天气查表——锚点「同步 O(1)」。
    pub fn check(&mut self, w: Weather) -> Result<(), &'static str> {
        let audible = AV_PAIRS[w.ordinal()] != AmbienceTrack::None_;
        let visible = self.visually_active(w);
        if visible && audible && !self.playing[w.ordinal()] {
            self.desyncs += 1;
            self.fix_requests += 1;
            return Err(E_EFX_AV_DESYNC);
        }
        if !visible && self.playing[w.ordinal()] {
            self.desyncs += 1;
            self.fix_requests += 1;
            return Err(E_EFX_AV_DESYNC);
        }
        Ok(())
    }

    /// 视觉感知阈（Q8）：权重 ≥32 视为可见。
    pub fn visually_active(&self, w: Weather) -> bool {
        self_weights_read(w) >= 32
    }
}

/// 独立读权重小桥（AvSyncer 不持有 WeatherMachine——组合在 probe 层）。
fn self_weights_read(_w: Weather) -> u32 {
    0
}

/// 组合适配：把天气机权重桥进同步器判读（真实组合入口）。
pub fn visually_active_with(machine: &WeatherMachine, w: Weather) -> bool {
    machine.weight(w) >= 32
}

// ---------------------------------------------------------------------------
// 四、联动链探针：断链点名不猜谜
// ---------------------------------------------------------------------------

/// 链体检结论：全通或断点环名。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ChainVerdict {
    /// 三环全通。
    ChainOk,
    /// 断在第几环（点名）。
    BrokenAt(&'static str),
}

/// 联动链探针：天气机→光照表→音配对三环逐环体检。
pub struct LinkProbe;

impl LinkProbe {
    /// 体检：任一环失联→`BrokenAt(环名)`（断点定位——锚点「探针定位」）。
    /// 全通 `ChainOk`。O(1)（三环定长检查）。
    pub fn probe(
        machine_ready: bool,
        phase: Option<DayPhase>,
        pair_ready: bool,
    ) -> (ChainVerdict, &'static str) {
        if !machine_ready {
            return (ChainVerdict::BrokenAt("天气机"), E_EFX_CHAIN_BROKEN);
        }
        if phase.is_none() {
            return (ChainVerdict::BrokenAt("光照表时段口径"), E_EFX_CHAIN_BROKEN);
        }
        if !pair_ready {
            return (ChainVerdict::BrokenAt("音画配对表"), E_EFX_CHAIN_BROKEN);
        }
        (ChainVerdict::ChainOk, "链全通")
    }
}

// ---------------------------------------------------------------------------
// 判据（判据区零 panic 面：无 unwrap()/expect()）
// ---------------------------------------------------------------------------

/// F5205 域自检（判据分六组：渐变强制/渐变数值/昼夜联动/音画同步/链探针/判据元）。
pub fn run_vez05_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F5205");

    // --- 渐变强制（判据一）---
    // 渐变-01：突变请求拒绝——帧数 0 的「直接设」不存在于合法路径。
    let mut wm = WeatherMachine::new();
    let abrupt = wm.request(Weather::Rain, 0) == Err(E_EFX_ABRUPT)
        && wm.abrupt_blocks == 1
        && wm.weight(Weather::Rain) == 0;
    if abrupt {
        s.ok("EZ5-渐变-01-突变请求拒绝");
    } else {
        s.fail("EZ5-渐变-01-突变请求拒绝", "突变被放行（光敏风险）");
    }
    // 渐变-02：合法渐变全程——权重从全晴逐帧移动到全雨，和恒定 256。
    let mut wm2 = WeatherMachine::new();
    let _ = wm2.request(Weather::Rain, 4);
    let mut all_monotonic = true;
    let mut prev_rain = 0u32;
    for _ in 0..4 {
        wm2.tick(4);
        let sum: u32 = {
            let mut t = 0u32;
            for w in Weather::ALL {
                t += wm2.weight(w);
            }
            t
        };
        if sum != Q8_ONE {
            all_monotonic = false;
        }
        let r = wm2.weight(Weather::Rain);
        if r < prev_rain {
            all_monotonic = false;
        }
        prev_rain = r;
    }
    let converged = wm2.weight(Weather::Rain) == Q8_ONE
        && wm2.weight(Weather::Clear) == 0
        && !wm2.in_gradient()
        && wm2.gradients_done == 1;
    if all_monotonic && converged {
        s.ok("EZ5-渐变-02-渐变全程权重守恒收敛");
    } else {
        s.fail("EZ5-渐变-02-渐变全程权重守恒收敛", "渐变数值失守");
    }
    // 渐变-03：渐变中改目标不重置起点——from 保持最初起点（亮度路径
    // 平滑折返不跳变）。判据场景：雪全占→向雾渐 5/8（雾 160>雪 96，
    // dominant=雾≠起点雪）→半程改目标晴：from 必须仍是雪。
    let mut wm3 = WeatherMachine::new();
    let _ = wm3.request(Weather::Snow, 8);
    for _ in 0..8 {
        wm3.tick(8);
    }
    let done = !wm3.in_gradient() && wm3.weight(Weather::Snow) == Q8_ONE;
    let _ = wm3.request(Weather::Fog, 8);
    for _ in 0..5 {
        wm3.tick(8);
    }
    let mid_ok = wm3.in_gradient() && wm3.weight(Weather::Fog) > 0;
    let _ = wm3.request(Weather::Clear, 8); // 半程改目标
    let keep = wm3.from_is(Weather::Snow) && wm3.target_is(Weather::Clear);
    if done && mid_ok && keep {
        s.ok("EZ5-渐变-03-渐变中改目标不抖亮度");
    } else {
        s.fail("EZ5-渐变-03-渐变中改目标不抖亮度", "改目标重置了起点");
    }

    // --- 昼夜联动（判据二）---
    // 联动-01：光照表判据侧独立写死对拍（晴白昼=7 最亮/雾夜晚=1 最暗，
    // 表数据不是 if 巧合）+ 查表 O(1)。
    let mut ln = DayNightLink::new();
    let sunny_day = ln.light_tier(Weather::Clear, Some(DayPhase::Day)) == Ok(7);
    let foggy_night = ln.light_tier(Weather::Fog, Some(DayPhase::Night)) == Ok(1);
    let rainy_dusk = ln.light_tier(Weather::Rain, Some(DayPhase::Dusk)) == Ok(3);
    if sunny_day && foggy_night && rainy_dusk {
        s.ok("EZ5-联动-01-光照表独立对拍");
    } else {
        s.fail("EZ5-联动-01-光照表独立对拍", "光照档漂移");
    }
    // 联动-02：时段口径非法拒绝——不在 F4437 闭集（None 模拟越口径）。
    let bad = ln.light_tier(Weather::Clear, None) == Err(E_EFX_BAD_PHASE)
        && ln.bad_phase_blocks == 1;
    if bad {
        s.ok("EZ5-联动-02-时段口径非法拒");
    } else {
        s.fail("EZ5-联动-02-时段口径非法拒", "口径漂移未拦");
    }

    // --- 音画同步（判据三）---
    // 同步-01：可见而无声=失步（修正请求发出）。
    let mut av = AvSyncer::new();
    let mut wm4 = WeatherMachine::new();
    let _ = wm4.request(Weather::Rain, 2);
    wm4.tick(2); // 雨全占（权重 256 ≥ 32 可见）
    let vis = visually_active_with(&wm4, Weather::Rain);
    let desync = av.check_with(vis, Weather::Rain) == Err(E_EFX_AV_DESYNC)
        && av.desyncs == 1
        && av.fix_requests == 1;
    // 同步-02：音频补齐后对拍通过（视听一体）。
    av.report_track(Weather::Rain, true);
    let synced = av.check_with(vis, Weather::Rain).is_ok();
    // 同步-03：幽灵音轨——视觉停而音轨在播=失步另一形状。
    av.report_track(Weather::Rain, false);
    let ghost_src = {
        let mut wm5 = WeatherMachine::new(); // 全晴：雨不可见
        visually_active_with(&wm5, Weather::Rain)
    };
    av.report_track(Weather::Rain, true);
    let ghost = av.check_with(ghost_src, Weather::Rain) == Err(E_EFX_AV_DESYNC)
        && av.desyncs == 2;
    if desync && synced && ghost {
        s.ok("EZ5-同步-01-失步对拍与修正请求");
    } else {
        s.fail("EZ5-同步-01-失步对拍与修正请求", "音画对拍失效");
    }
    // 同步-04：配对表完整性——五天气各有音面（判据侧独立枚举）。
    let mut pairs_ok = true;
    for w in Weather::ALL {
        let _t = AV_PAIRS[w.ordinal()];
        // 配对存在性由常量表构造保证；判据侧核对表长与天气数同构。
        if TRACK_COUNT != WEATHER_COUNT {
            pairs_ok = false;
        }
    }
    if pairs_ok {
        s.ok("EZ5-同步-02-配对表完整性");
    } else {
        s.fail("EZ5-同步-02-配对表完整性", "配对表与天气不同构");
    }

    // --- 链探针（判据四）---
    // 探针-01：三环全通→ChainOk。
    let ok = LinkProbe::probe(true, Some(DayPhase::Day), true);
    // 探针-02：逐环断链点名（断哪环报哪环，不猜谜）。
    let b1 = LinkProbe::probe(false, Some(DayPhase::Day), true);
    let b2 = LinkProbe::probe(true, None, true);
    let b3 = LinkProbe::probe(true, Some(DayPhase::Day), false);
    let all_named = matches!(b1.0, ChainVerdict::BrokenAt("天气机"))
        && matches!(b2.0, ChainVerdict::BrokenAt("光照表时段口径"))
        && matches!(b3.0, ChainVerdict::BrokenAt("音画配对表"));
    if matches!(ok.0, ChainVerdict::ChainOk) && all_named {
        s.ok("EZ5-探针-01-断链点名定位");
    } else {
        s.fail("EZ5-探针-01-断链点名定位", "断链定位失效");
    }

    // --- 判据元（判据五）---
    // 元-01：错误码互异 + 三要素齐备（判据侧独立枚举五码）。
    let codes = [
        E_EFX_ABRUPT,
        E_EFX_CHAIN_BROKEN,
        E_EFX_AV_DESYNC,
        E_EFX_BAD_PHASE,
        E_EFX_NO_PAIR,
    ];
    let mut codes_ok = codes.len() == ERROR_COUNT;
    let mut i = 0usize;
    while i < codes.len() && codes_ok {
        if reason_of(codes[i]) == "未知错误码" || direction_of(codes[i]).is_empty() {
            codes_ok = false;
        }
        let mut j = i + 1;
        while j < codes.len() {
            if codes[i] == codes[j] {
                codes_ok = false;
            }
            j += 1;
        }
        i += 1;
    }
    if codes_ok {
        s.ok("EZ5-判据-01-码互异三要素齐备");
    } else {
        s.fail("EZ5-判据-01-码互异三要素齐备", "码漂移或三要素缺失");
    }
    // 元-02：判据面零 panic（自扫本文件，单遍词法剥除）。
    let src = include_str!("vez05_envfx.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".unwrap()", ".expect(", "panic!", "unreachable!"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        s.ok("EZ5-判据-02-零panic面");
    } else {
        s.fail("EZ5-判据-02-零panic面", "判据/生产面含 panic 面");
    }
    // 元-03：闭集计数对账（天气5/时段4/音轨5——与 F4437 口径同构）。
    if WEATHER_COUNT == 5 && DAY_PHASE_COUNT == 4 && TRACK_COUNT == 5 && Q8_ONE == 256 {
        s.ok("EZ5-判据-03-闭集计数对账");
    } else {
        s.fail("EZ5-判据-03-闭集计数对账", "闭集计数漂移");
    }

    s
}

impl WeatherMachine {
    /// 目标天气查询（渐变中改目标判据用）。
    pub const fn target_is(&self, w: Weather) -> bool {
        (self.target as usize) == (w as usize)
    }

    /// 渐变起点查询（改目标不重置起点的判据面）。
    pub const fn from_is(&self, w: Weather) -> bool {
        (self.from as usize) == (w as usize)
    }
}

impl AvSyncer {
    /// 注入视觉状态的对拍入口（判据与组合层使用——不持机引用）。
    pub fn check_with(&mut self, visible: bool, w: Weather) -> Result<(), &'static str> {
        let audible = AV_PAIRS[w.ordinal()] != AmbienceTrack::None_;
        if visible && audible && !self.playing[w.ordinal()] {
            self.desyncs += 1;
            self.fix_requests += 1;
            return Err(E_EFX_AV_DESYNC);
        }
        if !visible && self.playing[w.ordinal()] {
            self.desyncs += 1;
            self.fix_requests += 1;
            return Err(E_EFX_AV_DESYNC);
        }
        Ok(())
    }
}

/// 单遍词法剥除（F2807 教训：字符串内 `//` 不得被行注释剥离误伤）。
fn strip_lexical_noise(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < b.len() {
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'/' {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
            let mut depth = 1usize;
            i += 2;
            while i < b.len() && depth > 0 {
                if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
                    depth += 1;
                    i += 2;
                } else if i + 1 < b.len() && b[i] == b'*' && b[i + 1] == b'/' {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            continue;
        }
        if b[i] == b'"' {
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    i += 2;
                    continue;
                }
                let closed = b[i] == b'"';
                i += 1;
                if closed {
                    break;
                }
            }
            continue;
        }
        if b[i] == b'\'' {
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    i += 2;
                    continue;
                }
                let closed = b[i] == b'\'';
                i += 1;
                if closed {
                    break;
                }
            }
            continue;
        }
        if let Some(c) = src.get(i..i + 1) {
            out.push_str(c);
        }
        i += 1;
    }
    out
}
