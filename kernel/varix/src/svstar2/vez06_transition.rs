//! VE-F5206 · 转场特效子系统（VE-Z 域 · 转场编排 · 锚点目标 320 行 · 深化版）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5206`
//!
//! **判据（锚点原文）**：场景转场、界面转场、语义一致、曲线强制、判据。
//!
//! **职责定位（锚点原文）**：转场特效子系统——转场特效三件：场景转场
//! （黑场/溶镜/擦除类转场库）/界面转场（面板切换过渡动效——词典动效
//! 口径消费）/转场语义（转场方向与内容流向一致——向前推进配前进向
//! 转场）。
//!
//! **错误路径与降级矩阵（锚点原文）**：转场突变→曲线强制；语义颠倒
//! →评审修正；词典违例→驳回。
//!
//! **性能逐项分解（锚点原文）**：转场 O(过渡帧)；映射 O(1)；驳回 O(1)。
//!
//! **跨批对接点（锚点原文）**：S 域词典动效口径上游；F5207 参数消费；
//! F5213 测试。
//!
//! # 深化说明：转场三道闸——曲线、词典、语义，各管一种「不对」
//!
//! 转场是玩家感知最直接的动效：做对了没人注意，做错了人人都晕。
//! 锚点把三类「不对」分给三道闸：**转场突变**（0 帧瞬切/直线阶跃）
//! →曲线强制（[`E_TRX_ABRUPT`]——晕动友好是域本色不是选项：闪与晃
//! 都是前庭刺激）；**词典违例**（界面转场的时长/缓动不在 S 域词典
//! 口径内）→驳回（[`E_TRX_DICT_VIOLATION`]——口径消费面只认词典
//! 词元，自创参数即驳回 O(1)）；**语义颠倒**（向前推进的内容配了
//! 后退向转场）→评审修正（[`E_TRX_SEMANTIC_FLIPPED`]——方向与内容
//! 流向的一致性映射 O(1) 查表，颠倒即立项）。
//!
//! ## 深化一：缓动曲线强制——smoothstep 是下限不是装饰
//!
//! 转场进度不允许线性直通：线性转场在起止两端速度最大（起手猛一
//! 跳收尾猛一顿）——这正是「晃」的来源。[`ease_in_out`] 用整数
//! smoothstep（3x²-2x³，Q8 定点无 f32）做进度重映射：两端速度为零、
//! 中段平滑。0 帧转场（瞬切）在入口就拒（[`TransitionEngine::play`]
//! frames==0 → [`E_TRX_ABRUPT`]）——曲线强制的执行面从入口开始，
//! 不是「跑了再说」。
//!
//! ## 深化二：词典动效口径——界面转场的参数只从词典来
//!
//! 面板过渡的时长与缓动词元必须先在 S 域词典登记（[`DictToken`]
//! 闭集：快 120ms/标准 200ms/强调 320ms 各配缓动档），消费时按词元
//! 查表——未登记词元/越域值驳回（映射 O(1)——锚点「映射 O(1)」）。
//! 「这个面板用 173ms」这类自创参数没有词典出处=口径漂移的起点。
//!
//! ## 深化三：语义映射表——方向与内容流向同构
//!
//! 内容流向闭集（[`Flow`]：前进/后退/横移/无向）→转场方向闭集
//! （[`Direction`]：前向/后向/中性）映射表单源：前进配前向（推入）、
//! 后退配后向（拉出）、横移配中性侧滑、无向配中性淡入淡出。方案
//! 声明的方向与表内映射不一致=语义颠倒→评审修正（对拍不拍板，
//! 裁决归评审——与 F5204 氛围复核同纪律）。
//!
//! ## 深化四：转场引擎——O(过渡帧) 逐帧步进、打断接续不重置
//!
//! [`TransitionEngine`] 持进度与曲线，逐帧步进（单帧 O(1)，全程
//! O(过渡帧)——锚点「转场 O(过渡帧)」）；过渡中被打断（新转场进）
//! →从当前进度接续到新目标（不回零重放——回零重放=亮度/位置抖动，
//! 与 F5205 渐变同纪律）。

use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 错误契约：Z 域字符串码惯例（E_TRX_* 前缀）
// ---------------------------------------------------------------------------

/// 转场突变：0 帧瞬切或无曲线直通（晕动友好红线）。
pub const E_TRX_ABRUPT: &str = "E_TRX_ABRUPT";
/// 词典违例：界面转场参数不在 S 域词典口径内。
pub const E_TRX_DICT_VIOLATION: &str = "E_TRX_DICT_VIOLATION";
/// 语义颠倒：转场方向与内容流向映射不一致。
pub const E_TRX_SEMANTIC_FLIPPED: &str = "E_TRX_SEMANTIC_FLIPPED";
/// 未登记流向：语义映射表查无此流向。
pub const E_TRX_NO_FLOW: &str = "E_TRX_NO_FLOW";
/// 转场库缺项：请求的转场类不在库内。
pub const E_TRX_NOT_IN_LIB: &str = "E_TRX_NOT_IN_LIB";

/// 错误码总数（判据侧独立复核）。
pub const ERROR_COUNT: usize = 5;

/// 错误码→原因串。
pub fn reason_of(code: &str) -> &'static str {
    match code {
        E_TRX_ABRUPT => "转场突变：0 帧瞬切被拒（晕动友好）",
        E_TRX_DICT_VIOLATION => "词典违例：动效参数不在 S 域口径内",
        E_TRX_SEMANTIC_FLIPPED => "语义颠倒：方向与内容流向不一致",
        E_TRX_NO_FLOW => "未登记流向：先入语义映射表",
        E_TRX_NOT_IN_LIB => "转场库缺项：请求类未登记",
        _ => "未知错误码",
    }
}

/// 错误码→处置方向。
pub fn direction_of(code: &str) -> &'static str {
    match code {
        E_TRX_ABRUPT => "指定过渡帧数走缓动曲线",
        E_TRX_DICT_VIOLATION => "改用词典词元或先扩词典",
        E_TRX_SEMANTIC_FLIPPED => "交评审：改方向或改映射表",
        E_TRX_NO_FLOW => "先在语义映射表登记流向",
        E_TRX_NOT_IN_LIB => "先扩转场库再请求",
        _ => reason_of(code),
    }
}

// ---------------------------------------------------------------------------
// 一、缓动曲线：整数 smoothstep（Q8 定点，无 f32）
// ---------------------------------------------------------------------------

/// Q8 定点分母（与 F5405/F5406 同口径）。
pub const Q8_ONE: u32 = 256;

/// 整数 smoothstep：`3x²-2x³`，x=t/256 → 返回 [0,256]。
/// 两端速度为零（缓入缓出）、中点对称（t=128→128）。
/// 判据侧独立手算锚：t=0→0、t=64→40（<线性 64，缓入）、
/// t=128→128、t=192→216（>线性 192，缓出）、t=256→256。
pub const fn ease_in_out(t: u32) -> u32 {
    let t = if t > Q8_ONE { Q8_ONE } else { t };
    let t2 = (t * t) as u64;
    let numer = 3 * t2 * (Q8_ONE as u64) - 2 * t2 * (t as u64);
    (numer / ((Q8_ONE as u64) * (Q8_ONE as u64))) as u32
}

// ---------------------------------------------------------------------------
// 二、转场库：场景转场闭集 + 方向闭集
// ---------------------------------------------------------------------------

/// 场景转场类闭集（转场库的行）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TransitionKind {
    /// 黑场（全暗渐入渐出）。
    FadeBlack,
    /// 溶镜（交叉溶解）。
    Dissolve,
    /// 擦除（线性扫过）。
    Wipe,
    /// 推入（新画面推走旧画面——前向语义）。
    Push,
    /// 拉出（旧画面拉走露出新画面——后向语义）。
    Pull,
}

/// 转场类数（判据侧独立写死 5）。
pub const TRANSITION_KIND_COUNT: usize = 5;

impl TransitionKind {
    /// 全集。
    pub const ALL: [TransitionKind; TRANSITION_KIND_COUNT] = [
        TransitionKind::FadeBlack,
        TransitionKind::Dissolve,
        TransitionKind::Wipe,
        TransitionKind::Push,
        TransitionKind::Pull,
    ];

    /// 转场名。
    pub const fn name(self) -> &'static str {
        match self {
            TransitionKind::FadeBlack => "黑场",
            TransitionKind::Dissolve => "溶镜",
            TransitionKind::Wipe => "擦除",
            TransitionKind::Push => "推入",
            TransitionKind::Pull => "拉出",
        }
    }

    /// 序数。
    pub const fn ordinal(self) -> usize {
        match self {
            TransitionKind::FadeBlack => 0,
            TransitionKind::Dissolve => 1,
            TransitionKind::Wipe => 2,
            TransitionKind::Push => 3,
            TransitionKind::Pull => 4,
        }
    }
}

/// 转场方向闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direction {
    /// 前向（推入类）。
    Forward,
    /// 后向（拉出类）。
    Backward,
    /// 中性（黑场/溶镜/擦除——无方向语义）。
    Neutral,
}

/// 方向数（判据侧独立写死 3）。
pub const DIRECTION_COUNT: usize = 3;

impl Direction {
    /// 全集。
    pub const ALL: [Direction; DIRECTION_COUNT] = [
        Direction::Forward,
        Direction::Backward,
        Direction::Neutral,
    ];

    /// 方向名。
    pub const fn name(self) -> &'static str {
        match self {
            Direction::Forward => "前向",
            Direction::Backward => "后向",
            Direction::Neutral => "中性",
        }
    }
}

/// 转场库：类→方向归属表（单源——语义映射的地面真值）。
pub const KIND_DIRECTION: [Direction; TRANSITION_KIND_COUNT] = [
    Direction::Neutral, // 黑场
    Direction::Neutral, // 溶镜
    Direction::Neutral, // 擦除
    Direction::Forward, // 推入
    Direction::Backward, // 拉出
];

/// 转场库登记簿：请求的类必须已登记（缺项拒）。
#[derive(Clone, Debug)]
pub struct TransitionLib {
    registered: [bool; TRANSITION_KIND_COUNT],
    /// 缺项拒绝计数。
    pub not_in_lib_blocks: u32,
}

impl TransitionLib {
    /// 空库。
    pub fn new() -> Self {
        TransitionLib { registered: [false; TRANSITION_KIND_COUNT], not_in_lib_blocks: 0 }
    }

    /// 登记一个转场类。
    pub fn enroll(&mut self, k: TransitionKind) {
        self.registered[k.ordinal()] = true;
    }

    /// 类在库判据。
    pub fn has(&self, k: TransitionKind) -> bool {
        self.registered[k.ordinal()]
    }

    /// 已登记类数。
    pub fn enrolled(&self) -> usize {
        self.registered.iter().filter(|v| **v).count()
    }

    /// 请求校验：不在库→`E_TRX_NOT_IN_LIB`（驳回 O(1)）。
    pub fn request_check(&mut self, k: TransitionKind) -> Result<(), &'static str> {
        if self.has(k) {
            Ok(())
        } else {
            self.not_in_lib_blocks += 1;
            Err(E_TRX_NOT_IN_LIB)
        }
    }
}

// ---------------------------------------------------------------------------
// 三、词典动效口径：界面转场参数只从词典来
// ---------------------------------------------------------------------------

/// 词典动效词元闭集（S 域口径——界面转场的合法参数面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DictToken {
    /// 快速过渡：120ms。
    Fast,
    /// 标准过渡：200ms。
    Standard,
    /// 强调过渡：320ms。
    Emphasis,
}

/// 词元数（判据侧独立写死 3）。
pub const DICT_TOKEN_COUNT: usize = 3;

impl DictToken {
    /// 全集。
    pub const ALL: [DictToken; DICT_TOKEN_COUNT] = [
        DictToken::Fast,
        DictToken::Standard,
        DictToken::Emphasis,
    ];

    /// 词元名。
    pub const fn name(self) -> &'static str {
        match self {
            DictToken::Fast => "快速",
            DictToken::Standard => "标准",
            DictToken::Emphasis => "强调",
        }
    }

    /// 词元→时长 ms（词典口径单源——判据侧独立写死对拍）。
    pub const fn duration_ms(self) -> u32 {
        match self {
            DictToken::Fast => 120,
            DictToken::Standard => 200,
            DictToken::Emphasis => 320,
        }
    }

    /// 序数。
    pub const fn ordinal(self) -> usize {
        match self {
            DictToken::Fast => 0,
            DictToken::Standard => 1,
            DictToken::Emphasis => 2,
        }
    }
}

/// 词典消费闸：界面转场请求的词元必须已登记（未登记=自创参数→驳回）。
#[derive(Clone, Debug, Default)]
pub struct DictGate {
    registered: [bool; DICT_TOKEN_COUNT],
    /// 驳回计数（O(1)——锚点「驳回 O(1)」）。
    pub violations: u32,
}

impl DictGate {
    /// 空闸。
    pub fn new() -> Self {
        DictGate { registered: [false; DICT_TOKEN_COUNT], violations: 0 }
    }

    /// 登记词元（词典扩容走 S 域流程，此处只消费）。
    pub fn enroll(&mut self, t: DictToken) {
        self.registered[t.ordinal()] = true;
    }

    /// **消费校验**：未登记词元→`E_TRX_DICT_VIOLATION`（驳回 O(1)）；
    /// 已登记→Ok 返回词元（时长查表得）。
    pub fn consume(&mut self, t: DictToken) -> Result<u32, &'static str> {
        if self.registered[t.ordinal()] {
            Ok(t.duration_ms())
        } else {
            self.violations += 1;
            Err(E_TRX_DICT_VIOLATION)
        }
    }
}

// ---------------------------------------------------------------------------
// 四、语义映射表：内容流向 → 转场方向（一致性 O(1)）
// ---------------------------------------------------------------------------

/// 内容流向闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Flow {
    /// 前进（深入内容）。
    Advance,
    /// 后退（回溯内容）。
    Retreat,
    /// 横移（并列内容）。
    Lateral,
    /// 无向（并列切换无空间隐喻）。
    Directionless,
}

/// 流向数（判据侧独立写死 4）。
pub const FLOW_COUNT: usize = 4;

impl Flow {
    /// 全集。
    pub const ALL: [Flow; FLOW_COUNT] = [
        Flow::Advance,
        Flow::Retreat,
        Flow::Lateral,
        Flow::Directionless,
    ];

    /// 流向名。
    pub const fn name(self) -> &'static str {
        match self {
            Flow::Advance => "前进",
            Flow::Retreat => "后退",
            Flow::Lateral => "横移",
            Flow::Directionless => "无向",
        }
    }

    /// 序数。
    pub const fn ordinal(self) -> usize {
        match self {
            Flow::Advance => 0,
            Flow::Retreat => 1,
            Flow::Lateral => 2,
            Flow::Directionless => 3,
        }
    }
}

/// 语义映射表：流向→应有方向（单源真值）。
pub const FLOW_DIRECTION: [Direction; FLOW_COUNT] = [
    Direction::Forward,  // 前进配前向（推入）——锚点「向前推进配前进向转场」
    Direction::Backward, // 后退配后向（拉出）
    Direction::Neutral,  // 横移配中性侧滑
    Direction::Neutral,  // 无向配中性淡入淡出
];

/// 语义审查器：方案声明方向 vs 映射表对拍。
#[derive(Clone, Debug, Default)]
pub struct SemanticMapper {
    enrolled: [bool; FLOW_COUNT],
    /// 颠倒立项数（评审修正的可观测面）。
    pub flips: u32,
    /// 未登记流向拒绝数。
    pub no_flow_blocks: u32,
}

impl SemanticMapper {
    /// 空表。
    pub fn new() -> Self {
        SemanticMapper { enrolled: [false; FLOW_COUNT], flips: 0, no_flow_blocks: 0 }
    }

    /// 登记流向（表行可审）。
    pub fn enroll(&mut self, f: Flow) {
        self.enrolled[f.ordinal()] = true;
    }

    /// **一致性审查**：未登记→`E_TRX_NO_FLOW`；声明方向≠表内应有方向
    /// →语义颠倒（`E_TRX_SEMANTIC_FLIPPED`，计数留痕交评审——对拍
    /// 不拍板）；一致→Ok。映射 O(1)——锚点「映射 O(1)」。
    pub fn review(&mut self, f: Flow, claimed: Direction) -> Result<(), &'static str> {
        if !self.enrolled[f.ordinal()] {
            self.no_flow_blocks += 1;
            return Err(E_TRX_NO_FLOW);
        }
        if claimed != FLOW_DIRECTION[f.ordinal()] {
            self.flips += 1;
            return Err(E_TRX_SEMANTIC_FLIPPED);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 五、转场引擎：曲线强制 + 逐帧步进 + 打断接续
// ---------------------------------------------------------------------------

/// 转场引擎状态。
#[derive(Clone, Debug)]
pub struct TransitionEngine {
    kind: Option<TransitionKind>,
    /// 总过渡帧数（>0——0 帧不存在于合法路径）。
    total_frames: u32,
    /// 已走帧数。
    elapsed: u32,
    /// 曲线强制拦截计数（0 帧请求）。
    pub abrupt_blocks: u32,
    /// 完成转场数。
    pub completed: u32,
}

impl TransitionEngine {
    /// 空引擎。
    pub fn new() -> Self {
        TransitionEngine { kind: None, total_frames: 0, elapsed: 0, abrupt_blocks: 0, completed: 0 }
    }

    /// **播放转场**：0 帧→`E_TRX_ABRUPT`（曲线强制从入口开始——晕动
    /// 红线）；过渡中再进→从当前进度接续到新目标（不回零重放）。
    pub fn play(&mut self, k: TransitionKind, frames: u32) -> Result<(), &'static str> {
        if frames == 0 {
            self.abrupt_blocks += 1;
            return Err(E_TRX_ABRUPT);
        }
        if self.kind.is_none() {
            self.elapsed = 0;
        }
        self.kind = Some(k);
        self.total_frames = frames;
        Ok(())
    }

    /// 转场进行中判据。
    pub fn playing(&self) -> bool {
        self.kind.is_some() && self.elapsed < self.total_frames
    }

    /// **当前曲线进度**（Q8）：线性帧比经 smoothstep 重映射——
    /// 两端速度为零。O(1) 单点计算。
    pub fn eased_progress(&self) -> u32 {
        if self.total_frames == 0 {
            return 0;
        }
        let linear = ((self.elapsed.min(self.total_frames) as u64) * (Q8_ONE as u64)
            / (self.total_frames as u64)) as u32;
        ease_in_out(linear)
    }

    /// **逐帧步进**：单帧 O(1)，全程 O(过渡帧)——锚点「转场 O(过渡帧)」。
    pub fn tick(&mut self) {
        if !self.playing() {
            return;
        }
        self.elapsed += 1;
        if self.elapsed >= self.total_frames {
            self.elapsed = self.total_frames;
            self.kind = None;
            self.completed += 1;
        }
    }
}

impl Default for TransitionEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 判据（判据区零 panic 面：无 unwrap()/expect()）
// ---------------------------------------------------------------------------

/// F5206 域自检（判据分六组：曲线/转场库/词典/语义/引擎/判据元）。
pub fn run_vez06_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F5206");

    // --- 曲线强制（判据一）---
    // 曲线-01：smoothstep 判据侧独立手算锚（五点：端点/缓入/中点/缓出/端点）。
    let e0 = ease_in_out(0);
    let e64 = ease_in_out(64);
    let e128 = ease_in_out(128);
    let e192 = ease_in_out(192);
    let e256 = ease_in_out(256);
    // 手算：64→(64²·(768-128))/65536=4096·640/65536=40；
    // 192→(192²·(768-384))/65536=36864·384/65536=216。
    let curve_ok = e0 == 0 && e64 == 40 && e128 == 128 && e192 == 216 && e256 == 256;
    // 曲线-02：缓入段进度落后线性、缓出段超前线性（两端速度为零的可观测面）。
    let slow_in = e64 < 64;
    let fast_out = e192 > 192;
    // 曲线-03：0 帧请求拒绝（曲线强制从入口开始）。
    let mut eng = TransitionEngine::new();
    let abrupt = eng.play(TransitionKind::Dissolve, 0) == Err(E_TRX_ABRUPT)
        && eng.abrupt_blocks == 1
        && !eng.playing();
    if curve_ok && slow_in && fast_out && abrupt {
        s.ok("EZ6-曲线-01-smoothstep手算与入口强制");
    } else {
        s.fail("EZ6-曲线-01-smoothstep手算与入口强制", "曲线数值或入口闸失效");
    }

    // --- 转场库（判据二）---
    // 库-01：类→方向归属判据侧独立写死对拍（推入=前向/拉出=后向/其余中性）。
    let expect_dir: [Direction; TRANSITION_KIND_COUNT] = [
        Direction::Neutral,
        Direction::Neutral,
        Direction::Neutral,
        Direction::Forward,
        Direction::Backward,
    ];
    let dir_ok = KIND_DIRECTION
        .iter()
        .zip(expect_dir.iter())
        .all(|(a, b)| a == b);
    // 库-02：缺项拒绝与登记放行（驳回 O(1)）。
    let mut lib = TransitionLib::new();
    let miss = lib.request_check(TransitionKind::Push) == Err(E_TRX_NOT_IN_LIB)
        && lib.not_in_lib_blocks == 1;
    lib.enroll(TransitionKind::Push);
    let hit = lib.request_check(TransitionKind::Push).is_ok() && lib.enrolled() == 1;
    if dir_ok && miss && hit {
        s.ok("EZ6-库-01-归属对拍与缺项驳回");
    } else {
        s.fail("EZ6-库-01-归属对拍与缺项驳回", "归属表或缺项闸失效");
    }

    // --- 词典口径（判据三）---
    // 词典-01：词元时长判据侧独立写死对拍（120/200/320）。
    let durations: [u32; DICT_TOKEN_COUNT] = [120, 200, 320];
    let dur_ok = DictToken::ALL
        .iter()
        .enumerate()
        .all(|(i, t)| t.duration_ms() == durations[i]);
    // 词典-02：未登记词元驳回、登记后放行（口径消费面只认词典）。
    let mut gate = DictGate::new();
    let violate = gate.consume(DictToken::Standard) == Err(E_TRX_DICT_VIOLATION)
        && gate.violations == 1;
    gate.enroll(DictToken::Standard);
    let consume_ok = gate.consume(DictToken::Standard) == Ok(200);
    if dur_ok && violate && consume_ok {
        s.ok("EZ6-词典-01-词元时长对拍与驳回");
    } else {
        s.fail("EZ6-词典-01-词元时长对拍与驳回", "词典闸失效");
    }

    // --- 语义一致（判据四）---
    // 语义-01：映射表判据侧独立写死对拍（前进前向/后退后向/横移中性/无向中性）。
    let expect_flow: [Direction; FLOW_COUNT] = [
        Direction::Forward,
        Direction::Backward,
        Direction::Neutral,
        Direction::Neutral,
    ];
    let flow_ok = FLOW_DIRECTION
        .iter()
        .zip(expect_flow.iter())
        .all(|(a, b)| a == b);
    // 语义-02：一致放行 + 颠倒立项（向前推进配后向=颠倒）+ 未登记拒。
    let mut sm = SemanticMapper::new();
    sm.enroll(Flow::Advance);
    sm.enroll(Flow::Retreat);
    let ok1 = sm.review(Flow::Advance, Direction::Forward).is_ok();
    let flip = sm.review(Flow::Advance, Direction::Backward) == Err(E_TRX_SEMANTIC_FLIPPED)
        && sm.flips == 1;
    let noflow = sm.review(Flow::Lateral, Direction::Neutral) == Err(E_TRX_NO_FLOW)
        && sm.no_flow_blocks == 1;
    if flow_ok && ok1 && flip && noflow {
        s.ok("EZ6-语义-01-映射对拍与颠倒立项");
    } else {
        s.fail("EZ6-语义-01-映射对拍与颠倒立项", "语义映射失效");
    }

    // --- 引擎（判据五）---
    // 引擎-01：逐帧步进全程 O(过渡帧)、完成计数、进度曲线单调不减。
    let mut eng2 = TransitionEngine::new();
    let _ = eng2.play(TransitionKind::Push, 8);
    let mut prev = 0u32;
    let mut mono = true;
    for _ in 0..8 {
        eng2.tick();
        let p = eng2.eased_progress();
        if p < prev {
            mono = false;
        }
        prev = p;
    }
    let done = !eng2.playing() && eng2.completed == 1 && eng2.eased_progress() == 256;
    if mono && done {
        s.ok("EZ6-引擎-01-逐帧步进曲线单调");
    } else {
        s.fail("EZ6-引擎-01-逐帧步进曲线单调", "步进或曲线失守");
    }
    // 引擎-02：过渡中打断接续不回零（新目标从当前进度继续——不抖动）。
    let mut eng3 = TransitionEngine::new();
    let _ = eng3.play(TransitionKind::Dissolve, 8);
    for _ in 0..4 {
        eng3.tick();
    }
    let mid = eng3.eased_progress();
    let _ = eng3.play(TransitionKind::FadeBlack, 8); // 打断
    let carried = eng3.playing() && eng3.eased_progress() > 0 && mid > 0;
    if carried {
        s.ok("EZ6-引擎-02-打断接续不回零");
    } else {
        s.fail("EZ6-引擎-02-打断接续不回零", "打断回零重放（抖动）");
    }

    // --- 判据元（判据六）---
    // 元-01：错误码互异 + 三要素齐备（判据侧独立枚举五码）。
    let codes = [
        E_TRX_ABRUPT,
        E_TRX_DICT_VIOLATION,
        E_TRX_SEMANTIC_FLIPPED,
        E_TRX_NO_FLOW,
        E_TRX_NOT_IN_LIB,
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
        s.ok("EZ6-判据-01-码互异三要素齐备");
    } else {
        s.fail("EZ6-判据-01-码互异三要素齐备", "码漂移或三要素缺失");
    }
    // 元-02：判据面零 panic（自扫本文件，单遍词法剥除）。
    let src = include_str!("vez06_transition.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".unwrap()", ".expect(", "panic!", "unreachable!"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        s.ok("EZ6-判据-02-零panic面");
    } else {
        s.fail("EZ6-判据-02-零panic面", "判据/生产面含 panic 面");
    }
    // 元-03：闭集计数对账（转场类5/方向3/词元3/流向4/Q8=256）。
    if TRANSITION_KIND_COUNT == 5
        && DIRECTION_COUNT == 3
        && DICT_TOKEN_COUNT == 3
        && FLOW_COUNT == 4
        && Q8_ONE == 256
    {
        s.ok("EZ6-判据-03-闭集计数对账");
    } else {
        s.fail("EZ6-判据-03-闭集计数对账", "闭集计数漂移");
    }

    s
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
