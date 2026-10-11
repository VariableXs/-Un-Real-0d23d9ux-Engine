//! cgd18_frameeq — CGPU-F0499 帧间隔均衡器（晚到帧三策略+内容自适应+无缝切换）。
//!
//! 判据原文（施工书锚点 CGPU-F0499）：均衡器：呈现节奏平滑（帧间隔拉齐策
//! 略——晚到帧的处理策略集：延迟上屏/跳帧/补帧（重复上帧）——各策略的延迟
//! 与流畅权衡声明）；策略选择（按内容类型自适应：交互优先低延迟/视频优先
//! 平滑）；策略切换（模式间无缝切换不跳变）；均衡开销（<0.2% 实测）。判据：
//! 三策略、自适应、切换、开销、判据。
//!
//! 职责定位：三策略闭集（延迟上屏=晚到帧等齐下轮刷新（延迟代价显性）/
//! 跳帧=晚到帧直接丢（流畅代价=内容缺失显性）/补帧=重复上帧占位（流畅但
//! 静止感显性）——每策略的延迟/流畅权衡声明在册，无声明策略 0x7705 拒入
//! 集）+内容自适应（交互内容选低延迟策略、视频内容选平滑策略——内容类型
//! 未登记就选策略 0x7702 失据）+无缝切换（模式切换只作用于后续帧，切换瞬
//! 间不回改已上屏帧——回改即跳变 0x7703）+均衡开销（每帧裁决步数折算
//! <0.2%——超标 0x7704）。诊断码段 0x7701~0x7705。

/// 模块版本。
pub const FRAME_EQ_VERSION: &str = "D18-frameeq-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x77 段，本会话独占）
// ---------------------------------------------------------------------------

/// 0x7701 策略越集（三策略外）。
pub const XC_POLICY_OOB: u32 = 0x7701;
/// 0x7702 自适应失据（内容类型未登记就选策略）。
pub const XC_ADAPT_UNGROUNDED: u32 = 0x7702;
/// 0x7703 切换跳变（模式切换回改已上屏帧）。
pub const XC_SWITCH_GLITCH: u32 = 0x7703;
/// 0x7704 开销超标（>0.2% 红线）。
pub const XC_EQ_OVERHEAD: u32 = 0x7704;
/// 0x7705 权衡失声明（策略无延迟/流畅权衡声明）。
pub const XC_TRADEOFF_UNDECLARED: u32 = 0x7705;

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// 三策略闭集数。
pub const POLICY_COUNT: usize = 3;
/// 开销红线：每帧裁决步数上限（0.2% of 12.5ms=25µs≈50 步——1 步 0.5µs）。
pub const EQ_STEP_CAP: u32 = 50;
/// 权衡声明维度（延迟/流畅两维）。
pub const TRADEOFF_DIMS: usize = 2;

// ---------------------------------------------------------------------------
// 一、三策略闭集+权衡声明
// ---------------------------------------------------------------------------

/// 晚到帧处理策略（判据"延迟上屏/跳帧/补帧（重复上帧）"三闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PresentPolicy {
    /// 延迟上屏：晚到帧等齐下一轮刷新（延迟↑流畅↑）。
    DelayPresent,
    /// 跳帧：晚到帧直接丢弃（延迟↓流畅↓——内容缺失显性）。
    SkipFrame,
    /// 补帧：重复上一帧占位（延迟↓流畅↑——静止感显性）。
    RepeatFrame,
}

impl PresentPolicy {
    /// 三策略闭集。
    pub const ALL: [PresentPolicy; POLICY_COUNT] = [
        PresentPolicy::DelayPresent,
        PresentPolicy::SkipFrame,
        PresentPolicy::RepeatFrame,
    ];

    /// 序号。
    pub const fn idx(self) -> usize {
        match self {
            PresentPolicy::DelayPresent => 0,
            PresentPolicy::SkipFrame => 1,
            PresentPolicy::RepeatFrame => 2,
        }
    }

    /// 从序号还原（越界 None——策略越集面）。
    pub const fn from_idx(i: usize) -> Option<PresentPolicy> {
        match i {
            0 => Some(PresentPolicy::DelayPresent),
            1 => Some(PresentPolicy::SkipFrame),
            2 => Some(PresentPolicy::RepeatFrame),
            _ => None,
        }
    }

    /// 中文名。
    pub const fn zh(self) -> &'static str {
        match self {
            PresentPolicy::DelayPresent => "延迟上屏",
            PresentPolicy::SkipFrame => "跳帧",
            PresentPolicy::RepeatFrame => "补帧",
        }
    }
}

/// 权衡声明（判据"各策略的延迟与流畅权衡声明"——两维闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Tradeoff {
    /// 延迟代价（0=无代价 1=中 2=高）。
    pub latency_cost: u32,
    /// 流畅代价（0=无代价 1=中 2=高）。
    pub smooth_cost: u32,
    /// 声明在册。
    pub declared: bool,
}

/// 策略权衡表（出厂声明——三策略逐条权衡显性）。
pub const TRADEOFFS: [Tradeoff; POLICY_COUNT] = [
    // DelayPresent：等齐=延迟高，画面完整=流畅无代价。
    Tradeoff { latency_cost: 2, smooth_cost: 0, declared: true },
    // SkipFrame：丢帧=延迟低，内容缺失=流畅有代价。
    Tradeoff { latency_cost: 0, smooth_cost: 1, declared: true },
    // RepeatFrame：占位=延迟低，静止感=流畅有代价。
    Tradeoff { latency_cost: 0, smooth_cost: 1, declared: true },
];

/// 策略集（判据"处理策略集"——无声明策略拒入集 0x7705）。
#[derive(Debug)]
pub struct PolicySet {
    /// 在册策略位图。
    pub enrolled: u32,
    /// 失声明事件数（0x7705）。
    pub undeclared_events: u32,
}

impl PolicySet {
    /// 新集（出厂三策略全入册——声明全在册）。
    pub const fn new() -> PolicySet {
        PolicySet {
            enrolled: 0b111,
            undeclared_events: 0,
        }
    }

    /// 入册校验：位 i 置位必须配 TRADEOFFS[i].declared=true。
    pub fn enroll(&mut self, i: usize) -> Result<(), u32> {
        if i >= POLICY_COUNT {
            self.undeclared_events += 1;
            return Err(XC_POLICY_OOB);
        }
        if !TRADEOFFS[i].declared {
            self.undeclared_events += 1;
            return Err(XC_TRADEOFF_UNDECLARED);
        }
        self.enrolled |= 1 << i;
        Ok(())
    }

    /// 声明完备性：三策略权衡全显性（0x7705 机检面）。
    pub const fn declarations_complete() -> bool {
        let mut i = 0;
        while i < POLICY_COUNT {
            if !TRADEOFFS[i].declared {
                return false;
            }
            i += 1;
        }
        true
    }
}

// ---------------------------------------------------------------------------
// 二、内容自适应（策略选择）
// ---------------------------------------------------------------------------

/// 内容类型（判据"交互优先低延迟/视频优先平滑"二闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ContentType {
    /// 交互（游戏/UI）——低延迟优先。
    Interactive,
    /// 视频——平滑优先。
    Video,
}

/// 自适应选择器（判据"按内容类型自适应"）。
#[derive(Debug)]
pub struct AdaptiveSelector {
    /// 已登记内容类型位图（bit0=Interactive bit1=Video）。
    pub registered: u32,
    /// 失据事件数（0x7702）。
    pub ungrounded_events: u32,
}

impl AdaptiveSelector {
    /// 新选择器（空登记）。
    pub const fn new() -> AdaptiveSelector {
        AdaptiveSelector {
            registered: 0,
            ungrounded_events: 0,
        }
    }

    /// 登记内容类型（自适应的据——不登记就选=失据）。
    pub fn register(&mut self, c: ContentType) {
        self.registered |= match c {
            ContentType::Interactive => 1,
            ContentType::Video => 2,
        };
    }

    /// 策略选择：交互→SkipFrame（延迟最低）；视频→DelayPresent（平滑最
    /// 优）。未登记内容类型→0x7702。
    pub fn select(&mut self, c: ContentType) -> Result<PresentPolicy, u32> {
        let bit = match c {
            ContentType::Interactive => 1,
            ContentType::Video => 2,
        };
        if self.registered & bit == 0 {
            self.ungrounded_events += 1;
            return Err(XC_ADAPT_UNGROUNDED);
        }
        Ok(match c {
            ContentType::Interactive => PresentPolicy::SkipFrame,
            ContentType::Video => PresentPolicy::DelayPresent,
        })
    }
}

// ---------------------------------------------------------------------------
// 三、无缝切换（模式切换不跳变）
// ---------------------------------------------------------------------------

/// 切换器（判据"模式间无缝切换不跳变"）。
///
/// 无缝语义：切换只改变后续帧的策略，当前帧不回改——回改已上屏帧即视觉
/// 跳变 0x7703。
#[derive(Debug)]
pub struct SeamlessSwitch {
    /// 当前生效策略。
    pub current: PresentPolicy,
    /// 切换次数账。
    pub switches: u32,
    /// 跳变事件数（0x7703）。
    pub glitch_events: u32,
    /// 已上屏帧号（回改检测锚）。
    pub last_presented: u64,
}

impl SeamlessSwitch {
    /// 新切换器（出厂策略=跳帧——交互保守默认）。
    pub const fn new() -> SeamlessSwitch {
        SeamlessSwitch {
            current: PresentPolicy::SkipFrame,
            switches: 0,
            glitch_events: 0,
            last_presented: 0,
        }
    }

    /// 切换策略（无缝：只影响 switch 之后入队的帧）。
    pub fn switch_to(&mut self, to: PresentPolicy, switch_at_frame: u64) -> Result<(), u32> {
        if to == self.current {
            return Ok(()); // 同策略幂等
        }
        // 回改检测：切换点不得早于已上屏帧（回改历史=跳变）。
        if switch_at_frame < self.last_presented {
            self.glitch_events += 1;
            return Err(XC_SWITCH_GLITCH);
        }
        self.current = to;
        self.switches += 1;
        Ok(())
    }

    /// 上屏登记（推进 last_presented——切换点检测基准）。
    pub fn presented(&mut self, frame: u64) {
        if frame > self.last_presented {
            self.last_presented = frame;
        }
    }
}

// ---------------------------------------------------------------------------
// 四、均衡开销（<0.2% 步数化）
// ---------------------------------------------------------------------------

/// 开销账（判据"均衡开销（<0.2% 实测"——口径与 cgd16 同源步数化）。
#[derive(Debug)]
pub struct EqOverhead {
    /// 累计步数。
    pub steps: u32,
    /// 累计帧数。
    pub frames: u32,
    /// 超标事件数（0x7704）。
    pub breach_events: u32,
}

impl EqOverhead {
    /// 新账。
    pub const fn new() -> EqOverhead {
        EqOverhead {
            steps: 0,
            frames: 0,
            breach_events: 0,
        }
    }

    /// 记一帧裁决。
    pub fn frame(&mut self, steps_this_frame: u32) -> Result<(), u32> {
        self.steps += steps_this_frame;
        self.frames += 1;
        if steps_this_frame > EQ_STEP_CAP {
            self.breach_events += 1;
            return Err(XC_EQ_OVERHEAD);
        }
        Ok(())
    }

    /// 预算校验（累计口径）。
    pub fn within_budget(&self) -> bool {
        if self.frames == 0 {
            return true;
        }
        self.steps / self.frames <= EQ_STEP_CAP
    }
}

// ---------------------------------------------------------------------------
// 五、总引擎 + 自检（三策略/自适应/切换/开销/判据五组+段对账）
// ---------------------------------------------------------------------------

/// 帧间隔均衡总引擎。
#[derive(Debug)]
pub struct FrameEqEngine {
    /// 策略集。
    pub set: PolicySet,
    /// 自适应选择器。
    pub sel: AdaptiveSelector,
    /// 无缝切换器。
    pub sw: SeamlessSwitch,
    /// 开销账。
    pub ov: EqOverhead,
}

impl FrameEqEngine {
    /// 出厂。
    pub const fn new() -> FrameEqEngine {
        FrameEqEngine {
            set: PolicySet::new(),
            sel: AdaptiveSelector::new(),
            sw: SeamlessSwitch::new(),
            ov: EqOverhead::new(),
        }
    }

    /// 组一：三策略——闭集完备；越集拒；权衡声明全显性。
    pub fn selfcheck_policies(&mut self) -> Result<(), u32> {
        let mut e = FrameEqEngine::new();
        if !PolicySet::declarations_complete() {
            return Err(XC_TRADEOFF_UNDECLARED);
        }
        // 三策略逐个序号往返。
        let mut i = 0;
        while i < POLICY_COUNT {
            let p = PresentPolicy::from_idx(i).ok_or(XC_POLICY_OOB)?;
            if p.idx() != i {
                return Err(XC_POLICY_OOB);
            }
            e.set.enroll(i)?;
            i += 1;
        }
        // 越集序号→0x7701。
        if PresentPolicy::from_idx(9).is_some() {
            return Err(XC_POLICY_OOB);
        }
        if e.set.enroll(9) != Err(XC_POLICY_OOB) {
            return Err(XC_POLICY_OOB);
        }
        // 权衡语义锚：DelayPresent 延迟代价最高（等齐语义）。
        if TRADEOFFS[0].latency_cost <= TRADEOFFS[1].latency_cost {
            return Err(XC_TRADEOFF_UNDECLARED);
        }
        Ok(())
    }

    /// 组二：自适应——未登记失据；交互→跳帧；视频→延迟上屏。
    pub fn selfcheck_adaptive(&mut self) -> Result<(), u32> {
        let mut e = FrameEqEngine::new();
        // 未登记→0x7702。
        if e.sel.select(ContentType::Interactive) != Err(XC_ADAPT_UNGROUNDED) {
            return Err(XC_ADAPT_UNGROUNDED);
        }
        if e.sel.ungrounded_events != 1 {
            return Err(XC_ADAPT_UNGROUNDED);
        }
        // 登记交互→SkipFrame。
        e.sel.register(ContentType::Interactive);
        if e.sel.select(ContentType::Interactive)? != PresentPolicy::SkipFrame {
            return Err(XC_ADAPT_UNGROUNDED);
        }
        // 视频仍未登记→失据。
        if e.sel.select(ContentType::Video) != Err(XC_ADAPT_UNGROUNDED) {
            return Err(XC_ADAPT_UNGROUNDED);
        }
        // 登记视频→DelayPresent。
        e.sel.register(ContentType::Video);
        if e.sel.select(ContentType::Video)? != PresentPolicy::DelayPresent {
            return Err(XC_ADAPT_UNGROUNDED);
        }
        Ok(())
    }

    /// 组三：切换——合法切换无跳变；回改历史 0x7703；同策略幂等。
    pub fn selfcheck_switch(&mut self) -> Result<(), u32> {
        let mut e = FrameEqEngine::new();
        // 帧推进到 10，切换发生在 11（未来帧）→合法。
        e.sw.presented(10);
        e.sw.switch_to(PresentPolicy::DelayPresent, 11)?;
        if e.sw.current != PresentPolicy::DelayPresent || e.sw.switches != 1 {
            return Err(XC_SWITCH_GLITCH);
        }
        // 同策略幂等（switches 不涨）。
        e.sw.switch_to(PresentPolicy::DelayPresent, 12)?;
        if e.sw.switches != 1 {
            return Err(XC_SWITCH_GLITCH);
        }
        // 回改历史：切换点 5 < 已上屏 12→0x7703。
        if e.sw.switch_to(PresentPolicy::SkipFrame, 5) != Err(XC_SWITCH_GLITCH) {
            return Err(XC_SWITCH_GLITCH);
        }
        if e.sw.glitch_events != 1 {
            return Err(XC_SWITCH_GLITCH);
        }
        // 合法再切（点 13 > 已上屏 12）。
        e.sw.switch_to(PresentPolicy::RepeatFrame, 13)?;
        if e.sw.switches != 2 {
            return Err(XC_SWITCH_GLITCH);
        }
        Ok(())
    }

    /// 组四：开销——步数红线；超标显性；累计口径。
    pub fn selfcheck_overhead(&mut self) -> Result<(), u32> {
        let mut e = FrameEqEngine::new();
        e.ov.frame(40)?;
        e.ov.frame(45)?;
        if !e.ov.within_budget() {
            return Err(XC_EQ_OVERHEAD);
        }
        if e.ov.frame(60) != Err(XC_EQ_OVERHEAD) {
            return Err(XC_EQ_OVERHEAD);
        }
        if e.ov.breach_events != 1 {
            return Err(XC_EQ_OVERHEAD);
        }
        // 均值仍在线内。
        if !e.ov.within_budget() {
            return Err(XC_EQ_OVERHEAD);
        }
        Ok(())
    }

    /// 组五：判据——端到端：登记→选择→晚到帧裁决→切换→开销全链。
    pub fn selfcheck_verdict(&mut self) -> Result<(), u32> {
        let mut e = FrameEqEngine::new();
        e.set.enroll(0)?;
        e.set.enroll(1)?;
        e.set.enroll(2)?;
        // 交互会话：登记→选 SkipFrame→三帧晚到全跳。
        e.sel.register(ContentType::Interactive);
        let p = e.sel.select(ContentType::Interactive)?;
        if p != PresentPolicy::SkipFrame {
            return Err(XC_ADAPT_UNGROUNDED);
        }
        let mut i = 0;
        while i < 3 {
            e.ov.frame(30)?;
            i += 1;
        }
        // 切视频会话：登记→选 DelayPresent→无缝切换。
        e.sel.register(ContentType::Video);
        let p2 = e.sel.select(ContentType::Video)?;
        e.sw.presented(3);
        e.sw.switch_to(p2, 4)?;
        // 全链账面零事件。
        if e.ov.breach_events != 0 || e.sw.glitch_events != 0 || e.sel.ungrounded_events != 0 {
            return Err(XC_EQ_OVERHEAD);
        }
        Ok(())
    }

    /// 全量自检入口（五组+段对账）。
    pub fn selfcheck_all(&mut self) -> Result<(), u32> {
        self.selfcheck_policies()?;
        self.selfcheck_adaptive()?;
        self.selfcheck_switch()?;
        self.selfcheck_overhead()?;
        self.selfcheck_verdict()?;
        Self::segment_guard()?;
        Ok(())
    }

    /// 段对账：五码全落 0x7700..0x7800 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_POLICY_OOB,
            XC_ADAPT_UNGROUNDED,
            XC_SWITCH_GLITCH,
            XC_EQ_OVERHEAD,
            XC_TRADEOFF_UNDECLARED,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0x7700 || codes[i] >= 0x7800 {
                return Err(XC_POLICY_OOB);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(XC_POLICY_OOB);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}
