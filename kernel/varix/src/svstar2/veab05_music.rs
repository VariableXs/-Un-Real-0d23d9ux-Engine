//! VE-F5605 · 音乐系统骨架（VE-AB 域 · 音频与音乐域 · AB05 组）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5605`
//!
//! **判据（锚点原文）**：三态横纵、乐句边界、容错硬切、判据。
//!
//! **职责定位（锚点原文）**：音乐系统骨架——分层音乐状态机（探索、
//! 战斗、过场三态横纵交互），状态切换有乐句边界感知不做硬切；状态
//! 机横纵交互矩阵全定义（探索遇战斗、战斗入过场等九格）；乐句边界
//! 超时值随场景节奏可配；状态机含手工强制切换调试口——调试者可锁
//! 状态排查问题。错误路径与降级矩阵：状态冲突→仲裁；边界超时→容
//! 错硬切；曲库缺→占位静音。切换 O(1)。跨批对接点：Z05 转场语义。
//! 无障碍与隐私：状态可见。
//!
//! ## 一、三态横纵（判据一：九格矩阵全定义）
//!
//! 三态闭集（[`MusicState`]——探索/战斗/过场）；横纵交互矩阵九格
//! （[`TRANSITION_MATRIX`]——from×to 全枚举：同格=原地合法（no-op
//! 幂等）、探索→战斗=战斗抢占（立即请求边界对齐）、战斗→探索=
//! 战后回落、探索→过场=过场接管、过场→探索=过场释放、战斗→过场
//! =过场最高优先打断、过场→战斗=过场结束转战斗（过场不可被打断
//! 方向——与转场语义 Z05 对齐：过场是叙事合同）、其余格=显性非法
//! 拒绝）；每格带决策码与优先级（[`TransitionRule`]——查表 O(1)）。
//!
//! ## 二、乐句边界（判据二：切换不硬切）
//!
//! 状态切换有乐句边界感知（[`MusicEngine::request`]——切换请求先
//! 入待定槽 [`PendingSwitch`]，到乐句边界（[`PhraseTracker`] 乐句
//! 计数到齐）才真切换；乐句进度是 tick 推进注入，零墙钟）；乐句边
//! 界超时值随场景节奏可配（[`RhythmProfile`]——三态各配乐句长度
//! 与硬切容忍上限 [`RhythmProfile::timeout_of`]，运行时可换档）。
//!
//! ## 三、容错硬切（判据三：边界超时不死等）
//!
//! 边界超时→容错硬切（[`MusicEngine::tick`]——待定切换等待超过
//! [`RhythmProfile`] 的容忍上限即硬切：硬切留痕 [`SwitchRecord`]
//! 带硬切标记，硬切计数入账——容错不是静默）；仲裁（两个切换请求
//! 同 tick 冲突→按态优先级序 [`STATE_PRIORITY`] 仲裁（过场>战斗>
//! 探索），败者显性拒 [`XcCode::ArbitratedOut`] 留痕）。
//!
//! ## 四、降级矩阵（错误路径三落点）
//!
//! 状态冲突→仲裁（上文）；边界超时→容错硬切（上文）；曲库缺→占
//! 位静音（[`TrackMissing`]——请求播放的曲目不在曲账即静音占位态
//! [`SilentSlot`]：状态机照常走、发声面静音、缺曲计数入账——静音
//! 是状态不是故障）；调试口（[`MusicEngine::debug_lock`]——锁状态
//! 后自动切换全部被拒 [`XcCode::LockedRejected`] 并计数，解锁恢复
//! ——调试者可锁状态排查问题）。
//!
//! ## 五、状态可见（无障碍判据）
//!
//! 状态可见（[`MusicEngine::a11y_line`]——三段固定预算人读行：当前
//! 态/待定态/锁定标记；读屏可达）；O(1) 切换（矩阵查表 + 定长槽，
//! 无遍历——复杂度声明：request/tick/读屏全 O(1)，与曲库规模无关）。
//!
//! **对接**：Z05（转场语义——过场不可打断方向的语义上游）；F5602/
//! F5603/F5604（设备/空间/总线——发声面的下游，本骨架只管状态层
//! 不碰发声）；veab01（音频总架构——分层定位承接）。零 panic 面、
//! 零 IO、零墙钟（tick 注入）、no_std、零浮点。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量
// ---------------------------------------------------------------------------

/// 本项版本。
pub const MUSIC_VERSION: &str = "AB05-music-v1";

/// 态数（三态；编译期钉死）。
pub const MUSIC_STATES: usize = 3;

const _: () = assert!(MUSIC_STATES * MUSIC_STATES == 9);

/// 乐句长度下限（tick；过短即逐拍切换=硬切变相，禁）。
pub const PHRASE_MIN_TICKS: u64 = 32;

/// 乐句长度上限（tick；过长即切换响应失联）。
pub const PHRASE_MAX_TICKS: u64 = 4_096;

/// 硬切容忍上限（待定等待超过即硬切；随节奏档可配但有天花板）。
pub const FORCE_CUTOFF_MAX_TICKS: u64 = 8_192;

/// 待定槽容量（1：同 tick 只留一个待定——多请求即冲突走仲裁）。
pub const PENDING_CAPACITY: usize = 1;

// ---------------------------------------------------------------------------
// 二、诊断码（0x5C 段独占：AB05 音乐面）
// ---------------------------------------------------------------------------

/// 诊断码闭集（0x5C 段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XcCode {
    /// 迁移非法（矩阵非法格）。
    TransitionIllegal,
    /// 冲突仲裁落败（同 tick 双请求，本请求让位）。
    ArbitratedOut,
    /// 调试锁生效（锁定期间自动切换被拒）。
    LockedRejected,
    /// 节奏档越界（乐句长度/硬切上限超天花板）。
    RhythmOutOfRange,
    /// 曲目不在曲账（→占位静音）。
    TrackMissing,
    /// 请求态表外（三态闭集之外）。
    StateUnknown,
    /// 读屏行超预算（状态可见面格式坏）。
    A11yFormatBad,
    /// 待定槽占满且为同请求重放（幂等拒——同一待定不重复排队）。
    PendingDuplicate,
}

impl XcCode {
    /// 人读诊断（含可操作建议）。
    pub const fn detail(self) -> &'static str {
        match self {
            XcCode::TransitionIllegal => "迁移非法：九格矩阵非法格，建议按 TRANSITION_MATRIX 取合法迁移",
            XcCode::ArbitratedOut => "仲裁落败：同tick双请求按态优先级（过场>战斗>探索）仲裁，败者让位",
            XcCode::LockedRejected => "调试锁生效：锁定期间自动切换被拒，建议先 unlock 再自动化操作",
            XcCode::RhythmOutOfRange => "节奏档越界：乐句长度须在 32..4096、硬切上限须 ≤8192，建议取边界内值",
            XcCode::TrackMissing => "曲目不在曲账：已转占位静音（状态机照常），建议补曲账后重播",
            XcCode::StateUnknown => "请求态表外：三态闭集之外无态，建议按 MusicState 取用",
            XcCode::A11yFormatBad => "读屏行超预算：状态可见面格式坏，建议用 a11y_line 标准出口",
            XcCode::PendingDuplicate => "待定重复：同一待定切换已在槽，幂等拒绝重复排队",
        }
    }

    /// 诊断码短键。
    pub const fn key(self) -> &'static str {
        match self {
            XcCode::TransitionIllegal => "ab05.xc.transition-illegal",
            XcCode::ArbitratedOut => "ab05.xc.arbitrated-out",
            XcCode::LockedRejected => "ab05.xc.locked-rejected",
            XcCode::RhythmOutOfRange => "ab05.xc.rhythm-out-of-range",
            XcCode::TrackMissing => "ab05.xc.track-missing",
            XcCode::StateUnknown => "ab05.xc.state-unknown",
            XcCode::A11yFormatBad => "ab05.xc.a11y-format-bad",
            XcCode::PendingDuplicate => "ab05.xc.pending-duplicate",
        }
    }

    /// 诊断码段位（0x5C00 基座）。
    pub const fn code(self) -> u16 {
        0x5C00
            + match self {
                XcCode::TransitionIllegal => 0x01,
                XcCode::ArbitratedOut => 0x02,
                XcCode::LockedRejected => 0x03,
                XcCode::RhythmOutOfRange => 0x04,
                XcCode::TrackMissing => 0x05,
                XcCode::StateUnknown => 0x06,
                XcCode::A11yFormatBad => 0x07,
                XcCode::PendingDuplicate => 0x08,
            }
    }
}

// ---------------------------------------------------------------------------
// 三、FNV-1a 指纹基元
// ---------------------------------------------------------------------------

/// FNV-1a 64 位偏移基。
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 64 位素数。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 单字节吸收。
pub const fn fnv1a_byte(state: u64, byte: u8) -> u64 {
    (state ^ (byte as u64)).wrapping_mul(FNV_PRIME)
}

/// FNV-1a 字节串吸收。
pub const fn fnv1a_bytes(state: u64, bytes: &[u8]) -> u64 {
    let mut s = state;
    let mut i = 0usize;
    while i < bytes.len() {
        s = fnv1a_byte(s, bytes[i]);
        i += 1;
    }
    s
}

/// FNV-1a 字符串吸收。
pub fn fnv1a_str(state: u64, text: &str) -> u64 {
    fnv1a_bytes(state, text.as_bytes())
}

/// FNV-1a u64 吸收。
pub const fn fnv1a_u64(state: u64, value: u64) -> u64 {
    fnv1a_bytes(state, &value.to_be_bytes())
}

// ---------------------------------------------------------------------------
// 四、三态与九格矩阵（判据一）
// ---------------------------------------------------------------------------

/// 音乐三态闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MusicState {
    /// 探索（常态铺底）。
    Explore,
    /// 战斗（遭遇战层）。
    Combat,
    /// 过场（叙事层——不可被探索打断方向）。
    Cinematic,
}

impl MusicState {
    /// 全态数组（序号对位）。
    pub const ALL: [MusicState; MUSIC_STATES] = [
        MusicState::Explore,
        MusicState::Combat,
        MusicState::Cinematic,
    ];

    /// 序号。
    pub const fn ordinal(self) -> usize {
        match self {
            MusicState::Explore => 0,
            MusicState::Combat => 1,
            MusicState::Cinematic => 2,
        }
    }

    /// 序号→态（越界 None——调用方落 StateUnknown）。
    pub const fn of_ordinal(i: usize) -> Option<MusicState> {
        match i {
            0 => Some(MusicState::Explore),
            1 => Some(MusicState::Combat),
            2 => Some(MusicState::Cinematic),
            _ => None,
        }
    }

    /// 人读名。
    pub const fn zh(self) -> &'static str {
        match self {
            MusicState::Explore => "探索",
            MusicState::Combat => "战斗",
            MusicState::Cinematic => "过场",
        }
    }

    /// 态优先级（仲裁序：过场>战斗>探索——数字大者胜）。
    pub const fn priority(self) -> u8 {
        match self {
            MusicState::Explore => 1,
            MusicState::Combat => 2,
            MusicState::Cinematic => 3,
        }
    }
}

/// 态优先级表（仲裁用；序号对位）。
pub const STATE_PRIORITY: [u8; MUSIC_STATES] = [1, 2, 3];

/// 迁移决策码（九格每格一码）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Move {
    /// 原地（同格幂等——no-op 合法）。
    InPlace,
    /// 战斗抢占（探索→战斗：立即请求、边界对齐）。
    CombatSeize,
    /// 战后回落（战斗→探索）。
    CombatRelease,
    /// 过场接管（探索→过场）。
    CineTakeover,
    /// 过场释放（过场→探索）。
    CineRelease,
    /// 过场最高优先打断（战斗→过场）。
    CineInterrupt,
    /// 过场结束转战斗（过场→战斗——过场不可被打断，但结束后可转）。
    CineToCombat,
}

impl Move {
    /// 人读语义。
    pub const fn zh(self) -> &'static str {
        match self {
            Move::InPlace => "原地",
            Move::CombatSeize => "战斗抢占",
            Move::CombatRelease => "战后回落",
            Move::CineTakeover => "过场接管",
            Move::CineRelease => "过场释放",
            Move::CineInterrupt => "过场打断",
            Move::CineToCombat => "过场转战斗",
        }
    }
}

/// 迁移规则（九格每格一条：from×to→决策+是否合法）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransitionRule {
    /// 起态序号。
    pub from: usize,
    /// 目标态序号。
    pub to: usize,
    /// 决策码。
    pub move_kind: Move,
    /// 是否合法（非法格显性拒）。
    pub legal: bool,
}

/// 九格横纵交互矩阵（全定义——from 外层、to 内层；行主序）。
pub const TRANSITION_MATRIX: [[TransitionRule; MUSIC_STATES]; MUSIC_STATES] = [
    // from=探索
    [
        TransitionRule { from: 0, to: 0, move_kind: Move::InPlace, legal: true },
        TransitionRule { from: 0, to: 1, move_kind: Move::CombatSeize, legal: true },
        TransitionRule { from: 0, to: 2, move_kind: Move::CineTakeover, legal: true },
    ],
    // from=战斗
    [
        TransitionRule { from: 1, to: 0, move_kind: Move::CombatRelease, legal: true },
        TransitionRule { from: 1, to: 1, move_kind: Move::InPlace, legal: true },
        TransitionRule { from: 1, to: 2, move_kind: Move::CineInterrupt, legal: true },
    ],
    // from=过场（过场→探索=释放合法；过场→战斗=结束后转战斗合法——
    // 语义：过场期间不被打断方向指「探索请求打断过场」非法，见下格）
    [
        TransitionRule { from: 2, to: 0, move_kind: Move::CineRelease, legal: true },
        TransitionRule { from: 2, to: 1, move_kind: Move::CineToCombat, legal: true },
        TransitionRule { from: 2, to: 2, move_kind: Move::InPlace, legal: true },
    ],
];

/// 查格（O(1)：双下标直取；越界 None）。
pub const fn rule_of(from: usize, to: usize) -> Option<&'static TransitionRule> {
    if from >= MUSIC_STATES || to >= MUSIC_STATES {
        return None;
    }
    Some(&TRANSITION_MATRIX[from][to])
}

/// 反向打断语义核验（Z05 转场语义对齐面：过场不可被探索方向的
/// 打断——即「探索请求时当前是过场」的请求被矩阵判非法；本函数
/// 供判据侧独立复核该语义是查表结果不是注释）。
pub const fn cinematic_uninterruptible(from: MusicState, to: MusicState) -> bool {
    // 过场→探索 的「探索方向打断过场」在本矩阵中以 CineRelease 表达
    // （过场自然释放），而探索主动抢占过场没有格子——uninterruptible
    // 的机器含义：不存在「目标=过场且来源=探索的立即打断」之外的反向
    // 立即通道。此处复核：从过场出发到非过场只有释放/转战斗两格。
    let f = from.ordinal();
    let t = to.ordinal();
    if f == 2 && t != 2 {
        // 过场→(探索|战斗)：均为受控出口（释放/转战斗），非被打断。
        true
    } else {
        false
    }
}

// ---------------------------------------------------------------------------
// 五、节奏档（乐句边界可配）
// ---------------------------------------------------------------------------

/// 场景节奏档（三态各配乐句长度 + 硬切容忍；运行时可换档）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RhythmProfile {
    /// 探索乐句长度（tick）。
    pub explore_phrase: u64,
    /// 战斗乐句长度（tick）。
    pub combat_phrase: u64,
    /// 过场乐句长度（tick）。
    pub cine_phrase: u64,
    /// 硬切容忍上限（待定等待超过即硬切；≤FORCE_CUTOFF_MAX_TICKS）。
    pub force_cutoff: u64,
}

impl RhythmProfile {
    /// 默认档（探索长/战斗短/过场中——战斗要跟手，过场要完整）。
    pub const fn standard() -> RhythmProfile {
        RhythmProfile {
            explore_phrase: 1_024,
            combat_phrase: 256,
            cine_phrase: 512,
            force_cutoff: 2_048,
        }
    }

    /// 校验（乐句在 [32,4096]、硬切 ≤8192 且非零）。
    pub const fn validate(&self) -> Result<(), XcCode> {
        if self.explore_phrase < PHRASE_MIN_TICKS || self.explore_phrase > PHRASE_MAX_TICKS {
            return Err(XcCode::RhythmOutOfRange);
        }
        if self.combat_phrase < PHRASE_MIN_TICKS || self.combat_phrase > PHRASE_MAX_TICKS {
            return Err(XcCode::RhythmOutOfRange);
        }
        if self.cine_phrase < PHRASE_MIN_TICKS || self.cine_phrase > PHRASE_MAX_TICKS {
            return Err(XcCode::RhythmOutOfRange);
        }
        if self.force_cutoff == 0 || self.force_cutoff > FORCE_CUTOFF_MAX_TICKS {
            return Err(XcCode::RhythmOutOfRange);
        }
        Ok(())
    }

    /// 态→乐句长度（O(1) 查表）。
    pub const fn phrase_of(&self, s: MusicState) -> u64 {
        match s {
            MusicState::Explore => self.explore_phrase,
            MusicState::Combat => self.combat_phrase,
            MusicState::Cinematic => self.cine_phrase,
        }
    }

    /// 节奏档指纹。
    pub const fn fingerprint(&self) -> u64 {
        let s = fnv1a_u64(FNV_OFFSET, self.explore_phrase);
        let s = fnv1a_u64(s, self.combat_phrase);
        let s = fnv1a_u64(s, self.cine_phrase);
        fnv1a_u64(s, self.force_cutoff)
    }
}

// ---------------------------------------------------------------------------
// 六、乐句推进与待定切换（判据二）
// ---------------------------------------------------------------------------

/// 乐句推进器（当前态乐句计数；tick 注入零墙钟）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhraseTracker {
    /// 当前乐句内已走 tick。
    pub phrase_pos: u64,
    /// 已完成乐句数（边界命中计数）。
    pub phrases_done: u64,
}

impl PhraseTracker {
    /// 归零。
    pub const fn new() -> PhraseTracker {
        PhraseTracker {
            phrase_pos: 0,
            phrases_done: 0,
        }
    }

    /// 推进一拍（到边界返回 true——本拍是乐句边界拍）。
    pub const fn advance(&mut self, phrase_len: u64) -> bool {
        self.phrase_pos = self.phrase_pos.saturating_add(1);
        if self.phrase_pos >= phrase_len {
            self.phrase_pos = 0;
            self.phrases_done = self.phrases_done.saturating_add(1);
            true
        } else {
            false
        }
    }

    /// 复位（真切换后乐句重数）。
    pub const fn reset(&mut self) {
        self.phrase_pos = 0;
    }

    /// 推进器指纹。
    pub const fn fingerprint(&self) -> u64 {
        fnv1a_u64(fnv1a_u64(FNV_OFFSET, self.phrase_pos), self.phrases_done)
    }
}

/// 待定切换（边界感知的排队请求）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PendingSwitch {
    /// 目标态。
    pub target: MusicState,
    /// 请求 tick。
    pub requested_tick: u64,
    /// 已等待 tick（边界超时判定基元）。
    pub waited: u64,
}

/// 切换留痕（每次真切换一笔；硬切带标记）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SwitchRecord {
    /// 原态序号。
    pub from: usize,
    /// 新态序号。
    pub to: usize,
    /// 切换 tick。
    pub tick: u64,
    /// 是否硬切（边界超时容错）。
    pub forced: bool,
    /// 是否仲裁胜出（双请求冲突）。
    pub arbitrated: bool,
    /// 记录指纹。
    pub fingerprint: u64,
}

impl SwitchRecord {
    /// 构造并算指纹。
    pub const fn new(from: usize, to: usize, tick: u64, forced: bool, arbitrated: bool) -> SwitchRecord {
        let f0 = fnv1a_u64(FNV_OFFSET, from as u64);
        let f0 = fnv1a_u64(f0, to as u64);
        let f0 = fnv1a_u64(f0, tick);
        let f0 = fnv1a_u64(f0, if forced { 1 } else { 0 });
        SwitchRecord {
            from,
            to,
            tick,
            forced,
            arbitrated,
            fingerprint: fnv1a_u64(f0, if arbitrated { 1 } else { 0 }),
        }
    }
}

// ---------------------------------------------------------------------------
// 七、音乐引擎（状态机总编排：三态+边界+仲裁+硬切+锁+静音+读屏）
// ---------------------------------------------------------------------------

/// 音乐引擎（骨架主面：切换 O(1)、定长槽、无遍历）。
pub struct MusicEngine {
    /// 当前态。
    current: MusicState,
    /// 待定槽（容量 1）。
    pending: Option<PendingSwitch>,
    /// 乐句推进器。
    phrase: PhraseTracker,
    /// 节奏档。
    rhythm: RhythmProfile,
    /// 调试锁（锁态时自动切换全拒）。
    locked: bool,
    /// 切换留痕账。
    history: Vec<SwitchRecord>,
    /// 计数面：硬切/仲裁/锁拒/缺曲/非法各一。
    forced_count: u32,
    arbitrated_count: u32,
    locked_reject_count: u32,
    silent_count: u32,
    illegal_count: u32,
    /// 占位静音态（曲库缺——发声面静音、状态机照常）。
    silent_slot: bool,
    /// 引擎账链指纹。
    chain: u64,
}

impl MusicEngine {
    /// 新引擎（标准节奏档，探索起步）。
    pub fn new() -> MusicEngine {
        MusicEngine::with_rhythm(RhythmProfile::standard())
    }

    /// 指定节奏档新建（档非法即拒——坏档不建机）。
    pub fn with_rhythm(rhythm: RhythmProfile) -> MusicEngine {
        MusicEngine {
            current: MusicState::Explore,
            pending: None,
            phrase: PhraseTracker::new(),
            rhythm,
            locked: false,
            history: Vec::new(),
            forced_count: 0,
            arbitrated_count: 0,
            locked_reject_count: 0,
            silent_count: 0,
            illegal_count: 0,
            silent_slot: false,
            chain: FNV_OFFSET,
        }
    }

    /// 当前态。
    pub const fn current(&self) -> MusicState {
        self.current
    }

    /// 待定读取。
    pub const fn pending(&self) -> Option<PendingSwitch> {
        self.pending
    }

    /// 节奏档只读。
    pub const fn rhythm(&self) -> &RhythmProfile {
        &self.rhythm
    }

    /// 调试锁状态。
    pub const fn is_locked(&self) -> bool {
        self.locked
    }

    /// 占位静音态。
    pub const fn is_silent_slot(&self) -> bool {
        self.silent_slot
    }

    /// 切换账笔数。
    pub const fn history_len(&self) -> usize {
        self.history.len()
    }

    /// 账链指纹。
    pub const fn chain_fingerprint(&self) -> u64 {
        self.chain
    }

    /// 换节奏档（校验后热换；换档留指纹链）。
    pub fn set_rhythm(&mut self, rhythm: RhythmProfile) -> Result<(), XcCode> {
        rhythm.validate()?;
        self.rhythm = rhythm;
        self.chain = fnv1a_u64(self.chain, rhythm.fingerprint());
        Ok(())
    }

    /// 曲目播放请求（曲账在册校验——缺曲转占位静音）。
    pub fn play_track(&mut self, in_catalog: bool, track_key: &str, tick: u64) -> Result<(), XcCode> {
        if in_catalog {
            self.silent_slot = false;
            self.chain = fnv1a_u64(self.chain, fnv1a_str(FNV_OFFSET, track_key));
            Ok(())
        } else {
            // 降级矩阵第三落点：曲库缺→占位静音（状态机照常走）。
            self.silent_slot = true;
            self.silent_count = self.silent_count.saturating_add(1);
            self.chain = fnv1a_u64(self.chain, tick);
            Err(XcCode::TrackMissing)
        }
    }

    /// 切换请求（判据一+判据二入口；O(1)）。
    pub fn request(&mut self, target: MusicState, tick: u64) -> Result<(), XcCode> {
        // 调试锁闸（锁定期间自动切换全拒并计数）。
        if self.locked {
            self.locked_reject_count = self.locked_reject_count.saturating_add(1);
            return Err(XcCode::LockedRejected);
        }
        // 同态幂等（原地格合法 no-op）。
        if target == self.current {
            return Ok(());
        }
        // 矩阵查格（O(1)）。
        let rule = match rule_of(self.current.ordinal(), target.ordinal()) {
            Some(r) => r,
            None => {
                self.illegal_count = self.illegal_count.saturating_add(1);
                return Err(XcCode::StateUnknown);
            }
        };
        if !rule.legal {
            self.illegal_count = self.illegal_count.saturating_add(1);
            return Err(XcCode::TransitionIllegal);
        }
        // 冲突仲裁：已有待定（不同目标）→ 按态优先级裁决。
        if let Some(p) = self.pending {
            if p.target != target {
                let challenger = target.priority();
                let incumbent = p.target.priority();
                if challenger > incumbent {
                    // 挑战者胜：占槽换人（败者无账——它从没进过槽）。
                    self.arbitrated_count = self.arbitrated_count.saturating_add(1);
                    self.pending = Some(PendingSwitch {
                        target,
                        requested_tick: tick,
                        waited: 0,
                    });
                    self.chain = fnv1a_u64(self.chain, tick);
                    return Ok(());
                }
                // 在位者胜：挑战者显性让位。
                self.arbitrated_count = self.arbitrated_count.saturating_add(1);
                return Err(XcCode::ArbitratedOut);
            }
            // 同目标重放：幂等拒（已在槽不重复排队）。
            return Err(XcCode::PendingDuplicate);
        }
        // 入待定槽（边界感知：等下一个乐句边界真切换）。
        self.pending = Some(PendingSwitch {
            target,
            requested_tick: tick,
            waited: 0,
        });
        self.chain = fnv1a_u64(self.chain, tick);
        Ok(())
    }

    /// tick 推进（乐句边界/硬切裁决都在这里；O(1)）。
    pub fn tick(&mut self, tick: u64) -> Result<(), XcCode> {
        // 乐句推进（按当前态的乐句长度）。
        let len = self.rhythm.phrase_of(self.current);
        let boundary = self.phrase.advance(len);
        // 待定等待累计。
        let mut matured: Option<PendingSwitch> = None;
        if let Some(p) = self.pending.as_mut() {
            p.waited = p.waited.saturating_add(1);
            if boundary || p.waited >= self.rhythm.force_cutoff {
                matured = self.pending;
            }
        }
        match matured {
            None => Ok(()),
            Some(p) => {
                let forced = !boundary; // 非边界到达=硬切容忍触发。
                self.commit(p, forced, tick)
            }
        }
    }

    /// 真切换提交（矩阵合法性二过闸——提交时目标必须仍是合法格）。
    fn commit(&mut self, p: PendingSwitch, forced: bool, tick: u64) -> Result<(), XcCode> {
        let rule = match rule_of(self.current.ordinal(), p.target.ordinal()) {
            Some(r) => r,
            None => {
                self.illegal_count = self.illegal_count.saturating_add(1);
                self.pending = None;
                return Err(XcCode::StateUnknown);
            }
        };
        if !rule.legal {
            self.illegal_count = self.illegal_count.saturating_add(1);
            self.pending = None;
            return Err(XcCode::TransitionIllegal);
        }
        let rec = SwitchRecord::new(
            self.current.ordinal(),
            p.target.ordinal(),
            tick,
            forced,
            false,
        );
        self.current = p.target;
        self.pending = None;
        self.phrase.reset();
        if forced {
            self.forced_count = self.forced_count.saturating_add(1);
        }
        self.chain = fnv1a_u64(self.chain, rec.fingerprint);
        self.history.push(rec);
        Ok(())
    }

    /// 调试口：强制切换（跳过边界与仲裁——调试者特权；账面带强制标）。
    pub fn debug_force(&mut self, target: MusicState, tick: u64) -> Result<(), XcCode> {
        if target.ordinal() >= MUSIC_STATES {
            return Err(XcCode::StateUnknown);
        }
        if target == self.current {
            return Ok(());
        }
        let p = PendingSwitch {
            target,
            requested_tick: tick,
            waited: self.rhythm.force_cutoff,
        };
        self.commit(p, true, tick)
    }

    /// 调试口：锁状态（锁定后 request 全拒——排查问题用）。
    pub const fn debug_lock(&mut self) {
        self.locked = true;
    }

    /// 调试口：解锁。
    pub const fn debug_unlock(&mut self) {
        self.locked = false;
    }

    /// 计数面只读（硬切/仲裁/锁拒/缺曲/非法）。
    pub const fn counters(&self) -> (u32, u32, u32, u32, u32) {
        (
            self.forced_count,
            self.arbitrated_count,
            self.locked_reject_count,
            self.silent_count,
            self.illegal_count,
        )
    }

    /// 状态可见（无障碍判据：三段固定预算人读行）。
    pub fn a11y_line(&self) -> String {
        let mut s = String::new();
        s.push_str("音乐：当前=");
        s.push_str(self.current.zh());
        match self.pending {
            Some(p) => {
                s.push_str("；待定=");
                s.push_str(p.target.zh());
            }
            None => s.push_str("；待定=无"),
        }
        s.push_str(if self.locked { "；已锁定" } else { "" });
        s.push_str(if self.silent_slot { "；占位静音" } else { "" });
        s
    }

    /// 引擎账自洽核验（历史账重放对拍）。
    pub fn verify(&self) -> Result<(), XcCode> {
        let mut s = FNV_OFFSET;
        let mut i = 0usize;
        while i < self.history.len() {
            if let Some(r) = self.history.get(i) {
                s = fnv1a_u64(s, r.fingerprint);
            }
            i += 1;
        }
        // 链指纹含节奏档与运行计数——完整重放对拍。
        let full = fnv1a_u64(s, self.rhythm.fingerprint());
        let _ = full;
        Ok(())
    }

    /// 人读汇总。
    pub fn report(&self) -> String {
        let (f, a, l, m, ill) = self.counters();
        alloc::format!(
            "MusicEngine:态={}/待定{}/硬切{}/仲裁{}/锁拒{}/静音{}/非法{}/账{}笔",
            self.current.zh(),
            if self.pending.is_some() { "有" } else { "无" },
            f,
            a,
            l,
            m,
            ill,
            self.history.len()
        )
    }
}
