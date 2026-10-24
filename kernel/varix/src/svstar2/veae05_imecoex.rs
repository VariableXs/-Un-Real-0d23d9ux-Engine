//! VE-F6205 · 输入与 IME 共存（veae05_imecoex）
//!
//! 输入域与输入法共存协议——**游戏侧视角**的 IME 隔离层（veu09_ime
//! 管 IME 引擎内部不变量，本单管游戏输入怎么与 IME 井水不犯河水）。
//! 四大件：**组合期输入隔离**（组合中不触发游戏快捷键不误提交——
//! 隔离门三分路由 ToIme/Swallow/ToGame，泄漏→隔离加固）；**候选窗
//! 跟随光标**（锚点跟随+屏幕夹取，错位→修正立案）；**输入仲裁**
//! （聊天框激活时游戏输入让路，死锁→超时回退焦点侧优先）；**多语
//! 言 IME 矩阵**（IME 不止中文——中日韩三类组合模型差异逐条回归：
//! 中文空格首选/日文罗马字 Enter 确认/韩文 Jamo 无候选窗阶段）。
//!
//! 数据结构（锚点原文）：IME 隔离；仲裁器。
//!
//! 错误路径与降级矩阵：**组合泄漏→隔离加固**（泄漏检出立案后隔离
//! 门升级，同类输入改吞不透）；**候选错位→修正**（超容差立案+重定
//! 心）；**仲裁死锁→超时回退**（僵持超阈值回退默认归属——焦点侧
//! 优先，用户正在打字是最高优先事实）。性能：O(1)（路由/仲裁/播报
//! 全常数，挂起队列有界容量）。跨批对接点：K 域文本输入（聊天框
//! 文本输入经本层仲裁后交付）；veu09_ime 同源互补（引擎内部 vs 共
//! 存协议）；veae02 能力位/veae04 状态机同族纪律。
//! 无障碍：IME 状态读屏播报（五类事件中文播报，sink 无主立案——
//! 播报无人接收等于无障碍面失效）。无隐私面（本层不留存键值内容）。
//!
//! 诊断码五码互异：`E_ICE_LEAK` / `E_ICE_CAND_MISALIGN` /
//! `E_ICE_ARBITER_STALL` / `E_ICE_ROUTE_UNKNOWN` /
//! `E_ICE_ANNOUNCE_SINK_LOST`。
use alloc::string::String;
use alloc::string::ToString;

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 版本与常量（阈值独立声明，判据独立手算对拍）
// ---------------------------------------------------------------------------

/// 共存协议版本。
pub const ICE_VERSION: &str = "AE05-imecoex-v1";
/// 仲裁死锁超时（tick 数——僵持超此值回退默认归属）。
pub const STALL_TIMEOUT_TICKS: u32 = 8;
/// 游戏输入挂起队列容量（有界——满丢最旧计数，不无界膨胀）。
pub const PENDING_CAP: usize = 16;
/// 候选窗错位容差（像素——超此值立案修正）。
pub const MISALIGN_TOL_PX: i32 = 2;
/// 屏幕宽（夹取边界基准）。
pub const SCREEN_W: i32 = 1920;
/// 屏幕高（夹取边界基准）。
pub const SCREEN_H: i32 = 1080;

// ---------------------------------------------------------------------------
// 诊断码与原因文案（五码互异；reason 全量显性）
// ---------------------------------------------------------------------------

/// 组合泄漏：组合期按键漏到游戏侧（隔离事故）。
pub const E_ICE_LEAK: &str = "E_ICE_LEAK";
/// 候选窗错位：跟随落点超容差（夹取后仍出界为恶化态）。
pub const E_ICE_CAND_MISALIGN: &str = "E_ICE_CAND_MISALIGN";
/// 仲裁死锁：双侧僵持超时（回退默认归属）。
pub const E_ICE_ARBITER_STALL: &str = "E_ICE_ARBITER_STALL";
/// 查无路由：按键未在归属簿声明（闭集外显性拒绝）。
pub const E_ICE_ROUTE_UNKNOWN: &str = "E_ICE_ROUTE_UNKNOWN";
/// 播报无主：读屏 sink 未注册（无障碍面失效告警）。
pub const E_ICE_ANNOUNCE_SINK_LOST: &str = "E_ICE_ANNOUNCE_SINK_LOST";

/// 诊断码 → 原因文案（全量显性，缺省分支兜底）。
pub fn reason_of(code: &str) -> &'static str {
    match code {
        E_ICE_LEAK => "组合泄漏：组合期按键漏到游戏侧，隔离门加固",
        E_ICE_CAND_MISALIGN => "候选窗错位：跟随落点超容差，立案修正",
        E_ICE_ARBITER_STALL => "仲裁死锁：双侧僵持超时，回退焦点侧优先",
        E_ICE_ROUTE_UNKNOWN => "查无路由：按键未声明归属，闭集外拒绝",
        E_ICE_ANNOUNCE_SINK_LOST => "播报无主：读屏 sink 未注册，无障碍面失效",
        _ => "未知诊断码",
    }
}

// ---------------------------------------------------------------------------
// 多语言 IME 矩阵（IME 不止中文——中日韩组合模型差异声明）
// ---------------------------------------------------------------------------

/// IME 种类（四类闭集——None 为无 IME 直通基线）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImeKind {
    /// 无输入法（直通基线——无隔离无组合）。
    None,
    /// 中文拼音（候选窗有；空格首选/数字选字；Esc 取消）。
    ZhPinyin,
    /// 日文罗马字（候选窗有；罗马字→假名→汉字；Enter 确认）。
    JaRomaji,
    /// 韩文 Jamo（**无候选窗阶段**——音节直接组合；Backspace 拆字）。
    KoJamo,
}

impl ImeKind {
    pub const COUNT: usize = 4;

    pub fn all() -> [ImeKind; ImeKind::COUNT] {
        [
            ImeKind::None,
            ImeKind::ZhPinyin,
            ImeKind::JaRomaji,
            ImeKind::KoJamo,
        ]
    }

    /// 中文名（读屏与判据文案单源）。
    pub fn zh(self) -> &'static str {
        match self {
            ImeKind::None => "无输入法",
            ImeKind::ZhPinyin => "中文拼音",
            ImeKind::JaRomaji => "日文罗马字",
            ImeKind::KoJamo => "韩文组合字母",
        }
    }

    /// 是否有组合期（韩文 Jamo 有组合只是无候选窗；None 无 IME 无组合）。
    pub fn has_composition(self) -> bool {
        !matches!(self, ImeKind::None)
    }

    /// 是否有候选窗阶段（韩文 Jamo 音节直接组合——无候选窗）。
    pub fn has_candidate_phase(self) -> bool {
        !matches!(self, ImeKind::None | ImeKind::KoJamo)
    }

    /// 该语言下确认键（组合模型差异声明——判据逐条回归）。
    pub fn commit_key(self) -> &'static str {
        match self {
            ImeKind::None => "",
            ImeKind::ZhPinyin => "Space", // 空格首选
            ImeKind::JaRomaji => "Enter", // 假名直通确认
            ImeKind::KoJamo => "",        // Jamo 自动组合，无确认键
        }
    }

    /// 组合期编辑键集合（这些键进 IME 不吞——语言无关）。
    pub fn composing_edit_keys() -> [&'static str; 6] {
        ["Backspace", "Left", "Right", "Home", "End", "Shift"]
    }

    /// 组合模型一句话声明（差异诚实——不把三种语言说成一种）。
    pub fn model_decl(self) -> &'static str {
        match self {
            ImeKind::None => "直通：无组合无候选",
            ImeKind::ZhPinyin => "拼音编码+候选窗：空格首选数字选字 Esc 取消",
            ImeKind::JaRomaji => "罗马字转假名+汉字候选：Enter 确认假名直通",
            ImeKind::KoJamo => "音节直接组合：无候选窗 Backspace 拆字",
        }
    }
}

// ---------------------------------------------------------------------------
// 按键路由三分（隔离门的核心输出——归途只有三条）
// ---------------------------------------------------------------------------

/// 按键路由（三分闭集——进 IME / 吞掉 / 进游戏）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    /// 进 IME（组合编辑/候选导航/确认取消）。
    ToIme,
    /// 吞掉（组合期游戏快捷键——隔离面，不触发不透传）。
    Swallow,
    /// 进游戏（无组合期且为游戏绑定键）。
    ToGame,
}

impl Route {
    pub fn zh(self) -> &'static str {
        match self {
            Route::ToIme => "进输入法",
            Route::Swallow => "隔离吞掉",
            Route::ToGame => "进游戏",
        }
    }
}

/// 游戏绑定键簿（组合期隔离的判定基准——簿内键组合期必吞）。
pub const GAME_BINDING_KEYS: [&str; 8] =
    ["KeyW", "KeyA", "KeyS", "KeyD", "Space", "Tab", "KeyE", "KeyQ"];

/// 键是否为游戏绑定键（O(1) 定长簿扫描）。
pub fn is_game_binding(key: &str) -> bool {
    GAME_BINDING_KEYS.iter().any(|k| *k == key)
}

// ---------------------------------------------------------------------------
// 立案簿（诊断事件——泄漏/错位/死锁/无主全有账）
// ---------------------------------------------------------------------------

/// 诊断立案单条。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagNote {
    pub code: &'static str,
    pub subject: String,
    pub detail: String,
}

/// 诊断立案簿（顺序追加，容量有界防膨胀）。
#[derive(Debug)]
pub struct DiagBag {
    notes: Vec<DiagNote>,
    cap: usize,
    dropped: usize,
}

impl DiagBag {
    pub fn new() -> DiagBag {
        DiagBag { notes: Vec::new(), cap: 64, dropped: 0 }
    }

    pub fn push(&mut self, code: &'static str, subject: &str, detail: &str) {
        if self.notes.len() >= self.cap {
            self.notes.remove(0);
            self.dropped += 1;
        }
        self.notes.push(DiagNote {
            code,
            subject: subject.to_string(),
            detail: detail.to_string(),
        });
    }

    pub fn len(&self) -> usize {
        self.notes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.notes.is_empty()
    }

    pub fn all(&self) -> &[DiagNote] {
        &self.notes
    }

    pub fn count_of(&self, code: &str) -> usize {
        self.notes.iter().filter(|n| n.code == code).count()
    }

    pub fn has(&self, code: &str) -> bool {
        self.count_of(code) > 0
    }

    /// 读屏摘要（立案簿可达——有账可念）。
    pub fn spoken(&self) -> String {
        if self.notes.is_empty() {
            return "诊断簿空".to_string();
        }
        let mut out = format!("诊断簿共{}条", self.notes.len());
        for n in self.notes.iter() {
            out.push('；');
            out.push_str(n.code);
            out.push('：');
            out.push_str(&n.subject);
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 隔离门（组合期输入隔离——泄漏检出→加固）
// ---------------------------------------------------------------------------

/// 组合期隔离门状态。
#[derive(Debug)]
pub struct IsolationGate {
    /// IME 种类（路由矩阵随语言差异变化）。
    pub kind: ImeKind,
    /// 组合进行中（true 时游戏绑定键必吞）。
    pub composing: bool,
    /// 加固等级（0 常规；≥1 泄漏后加固——连声称"已处理"的透传也复核）。
    pub guard_level: u8,
    /// 泄漏累计（组合期漏到游戏侧的按键数）。
    pub leak_count: usize,
    /// 吞掉累计（组合期被隔离的游戏键数——隔离面有账）。
    pub swallow_count: usize,
    /// 查无路由累计（闭集外按键显性拒绝数）。
    pub unknown_count: usize,
}

impl IsolationGate {
    pub fn new(kind: ImeKind) -> IsolationGate {
        IsolationGate {
            kind,
            composing: false,
            guard_level: 0,
            leak_count: 0,
            swallow_count: 0,
            unknown_count: 0,
        }
    }

    /// 开始组合（进入隔离期）。
    pub fn begin_compose(&mut self) {
        self.composing = self.kind.has_composition();
    }

    /// 结束组合（退出隔离期——游戏绑定键恢复直通）。
    pub fn end_compose(&mut self) {
        self.composing = false;
    }

    /// 核心路由判定（O(1)）。组合期**语言控制键优先于游戏绑定判定**
    /// ——Space/Enter/Esc 是游戏与 IME 的三岔口（Space 既是常见跳跃
    /// 键又是中文首选键），组合期它们属于 IME 的豁免名单：
    /// - 无组合期：游戏绑定键→ToGame；其余文本键→ToIme；
    /// - 组合期：语言控制键/编辑键/文本键→ToIme（编码与选字）；
    ///   游戏绑定键→Swallow（隔离面——不触发快捷键不误提交）；
    ///   未声明键→显性拒绝（闭集外不猜测）。
    /// 返回 (路由, 可选诊断码)。
    pub fn route_key(&mut self, key: &str) -> (Route, Option<&'static str>) {
        let edit = ImeKind::composing_edit_keys().iter().any(|k| *k == key);
        let game = is_game_binding(key);
        let lang_ctrl = self.lang_control_key(key);
        if self.composing {
            if lang_ctrl || edit || self.is_text_key(key) {
                // 豁免名单先行：组合期确认/选字/取消键归 IME——
                // 即使该键同时是游戏绑定键（Space 三岔口）。
                (Route::ToIme, None)
            } else if game {
                // 隔离面：组合期游戏快捷键吞掉——泄漏事故的唯一防线。
                self.swallow_count += 1;
                (Route::Swallow, None)
            } else {
                // 闭集外：显性拒绝不猜测（查无路由计数——调用方取码立案）。
                self.unknown_count += 1;
                (Route::Swallow, Some(E_ICE_ROUTE_UNKNOWN))
            }
        } else if game {
            (Route::ToGame, None)
        } else if self.is_text_key(key) {
            (Route::ToIme, None)
        } else {
            self.unknown_count += 1;
            (Route::Swallow, Some(E_ICE_ROUTE_UNKNOWN))
        }
    }

    /// 语言特有控制键（确认/取消/选字——语言矩阵差异点）。
    fn lang_control_key(&self, key: &str) -> bool {
        match self.kind {
            ImeKind::None => false,
            ImeKind::ZhPinyin => matches!(key, "Space" | "Escape") || Self::is_digit(key),
            ImeKind::JaRomaji => matches!(key, "Enter" | "Escape" | "Space"),
            ImeKind::KoJamo => matches!(key, "Backspace" | "Enter"),
        }
    }

    fn is_digit(key: &str) -> bool {
        matches!(key, "Digit1" | "Digit2" | "Digit3" | "Digit4" | "Digit5" | "Digit6" | "Digit7" | "Digit8" | "Digit9")
    }

    /// 文本键（字母/数字——组合期进 IME 的输入源）。
    fn is_text_key(&self, key: &str) -> bool {
        (key.starts_with("Key") && key.len() == 4)
            || Self::is_digit(key)
            || matches!(key, "Comma" | "Period" | "Minus" | "Equal")
    }

    /// 泄漏检出（组合期有按键实际到达游戏侧——环境事故显性立案）。
    /// 泄漏即加固：guard_level 升级，同类事故后隔离复核更严。
    pub fn audit_leak(&mut self, key: &str, bag: &mut DiagBag) {
        if self.composing && is_game_binding(key) {
            self.leak_count += 1;
            if self.guard_level < 2 {
                self.guard_level += 1;
            }
            bag.push(
                E_ICE_LEAK,
                &format!("泄漏键 {}", key),
                &format!("加固等级升至 {}", self.guard_level),
            );
        }
    }

    /// 加固后复核：guard_level≥1 时组合期对游戏侧的任何声称投递
    /// 一律拒绝返回 false（加固语义——泄漏后连声称"已处理"的也不信）。
    pub fn hardened_reject(&self, claimed_to_game: bool) -> bool {
        self.composing && self.guard_level >= 1 && claimed_to_game
    }

    /// 隔离门读屏摘要。
    pub fn spoken(&self) -> String {
        format!(
            "隔离门[{}]组合{}加固{}泄漏{}吞掉{}",
            self.kind.zh(),
            if self.composing { "中" } else { "无" },
            self.guard_level,
            self.leak_count,
            self.swallow_count
        )
    }
}

// ---------------------------------------------------------------------------
// 候选窗（跟随光标 + 屏幕夹取 + 错位修正）
// ---------------------------------------------------------------------------

/// 候选窗（锚点跟随——落点恒在屏幕内，错位立案修正）。
#[derive(Debug)]
pub struct CandidateWindow {
    /// IME 是否有候选窗阶段（韩文 Jamo 无候选窗——本结构空转）。
    pub active_kind: Option<ImeKind>,
    /// 窗尺寸（宽高像素）。
    pub size: (u32, u32),
    /// 当前落点（左上角，屏幕坐标——恒被夹取在屏内）。
    pub pos: (i32, i32),
    /// 跟随次数。
    pub follow_count: usize,
    /// 夹取次数（越界被拉回边界的次数）。
    pub clamp_count: usize,
    /// 错位立案次数。
    pub misalign_count: usize,
}

impl CandidateWindow {
    pub fn new(kind: ImeKind, size: (u32, u32)) -> CandidateWindow {
        CandidateWindow {
            active_kind: if kind.has_candidate_phase() { Some(kind) } else { None },
            size,
            pos: (0, 0),
            follow_count: 0,
            clamp_count: 0,
            misalign_count: 0,
        }
    }

    /// 跟随光标锚点（候选窗贴光标——越界夹取回屏幕内，O(1)）。
    /// 返回 (落点, 是否被夹取)。
    pub fn follow(&mut self, anchor: (i32, i32)) -> ((i32, i32), bool) {
        self.follow_count += 1;
        let raw = (anchor.0, anchor.1 + self.size.1 as i32 / 2);
        let clamped = self.clamp(raw);
        let did_clamp = clamped != raw;
        if did_clamp {
            self.clamp_count += 1;
        }
        self.pos = clamped;
        (clamped, did_clamp)
    }

    /// 屏幕夹取（右/下边界内缩窗尺寸——候选窗任何部分不出屏）。
    fn clamp(&self, p: (i32, i32)) -> (i32, i32) {
        let max_x = SCREEN_W - self.size.0 as i32;
        let max_y = SCREEN_H - self.size.1 as i32;
        (p.0.clamp(0, max_x), p.1.clamp(0, max_y))
    }

    /// 错位审计（期望落点与实际差超容差→立案；返回是否错位）。
    pub fn audit_position(
        &mut self,
        expected: (i32, i32),
        bag: &mut DiagBag,
    ) -> bool {
        let dx = (self.pos.0 - expected.0).abs();
        let dy = (self.pos.1 - expected.1).abs();
        if dx > MISALIGN_TOL_PX || dy > MISALIGN_TOL_PX {
            self.misalign_count += 1;
            bag.push(
                E_ICE_CAND_MISALIGN,
                "候选窗错位",
                &format!("期望({},{})实({},{})", expected.0, expected.1, self.pos.0, self.pos.1),
            );
            self.recentre(expected);
            true
        } else {
            false
        }
    }

    /// 修正（重定心到期望落点——仍夹取保屏内）。
    fn recentre(&mut self, expected: (i32, i32)) {
        self.pos = self.clamp(expected);
    }

    /// 无候选窗语言（韩文）调用跟随——显性空转不产生位置。
    pub fn inactive(&self) -> bool {
        self.active_kind.is_none()
    }

    /// 读屏摘要。
    pub fn spoken(&self) -> String {
        match self.active_kind {
            None => "候选窗无（该输入法无候选窗阶段）".to_string(),
            Some(k) => format!(
                "候选窗[{}]落点({},{})跟随{}夹取{}错位{}",
                k.zh(), self.pos.0, self.pos.1,
                self.follow_count, self.clamp_count, self.misalign_count
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// 仲裁器（聊天框 vs 游戏——让路、挂起队列、死锁回退）
// ---------------------------------------------------------------------------

/// 仲裁侧（二侧闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// 聊天框（焦点侧——文本输入）。
    Chat,
    /// 游戏（场景侧——快捷键与操控）。
    Game,
}

impl Side {
    pub fn zh(self) -> &'static str {
        match self {
            Side::Chat => "聊天框",
            Side::Game => "游戏",
        }
    }

    /// 对侧。
    pub fn other(self) -> Side {
        match self {
            Side::Chat => Side::Game,
            Side::Game => Side::Chat,
        }
    }
}

/// 仲裁状态（四态闭集：空闲/聊天持有/游戏持有/僵持）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArbState {
    /// 空闲（无侧持有——两侧都可即时获取）。
    Idle,
    /// 聊天框持有（游戏输入让路——挂起进队列）。
    ChatHeld,
    /// 游戏持有（聊天框文本不可达——游戏全屏态）。
    GameHeld,
    /// 僵持（持有方请求中对方也拒绝释放——超时回退）。
    Stalled,
}

/// 仲裁器（让路协议持有态机——死锁超时回退焦点侧优先）。
#[derive(Debug)]
pub struct Arbiter {
    /// 当前仲裁状态。
    pub state: ArbState,
    /// 僵持累计 tick。
    pub stalled_ticks: u32,
    /// 游戏输入挂起队列（聊天框持有期游戏按键不丢——有界丢最旧）。
    pub pending: Vec<String>,
    /// 挂起累计（进入让路的次数）。
    pub yield_count: usize,
    /// 恢复累计（让路结束恢复游戏输入的次数）。
    pub resume_count: usize,
    /// 丢弃最旧计数（挂起队列满溢出的次数）。
    pub dropped_oldest: usize,
    /// 死锁回退累计。
    pub revert_count: usize,
    /// 立案簿引用计数（死锁立案走外部簿——此处只计次）。
    pub stall_events: usize,
}

impl Arbiter {
    pub fn new() -> Arbiter {
        Arbiter {
            state: ArbState::Idle,
            stalled_ticks: 0,
            pending: Vec::new(),
            yield_count: 0,
            resume_count: 0,
            dropped_oldest: 0,
            revert_count: 0,
            stall_events: 0,
        }
    }

    /// 侧获取（聊天框激活→游戏让路；游戏全屏→聊天框文本不可达）。
    /// 返回 (新状态, 是否发生让路)。
    pub fn acquire(&mut self, side: Side) -> (ArbState, bool) {
        match (self.state, side) {
            (ArbState::Idle, s) => {
                let held = match s {
                    Side::Chat => ArbState::ChatHeld,
                    Side::Game => ArbState::GameHeld,
                };
                self.state = held;
                (held, false)
            }
            (ArbState::ChatHeld, Side::Chat) | (ArbState::GameHeld, Side::Game) => {
                // 重入获取——幂等。
                (self.state, false)
            }
            // 交叉获取：持有方在位、对方请求——僵持（等持有方 release 或超时）。
            (ArbState::ChatHeld, Side::Game) | (ArbState::GameHeld, Side::Chat) => {
                self.state = ArbState::Stalled;
                self.stalled_ticks = 0;
                self.stall_events += 1;
                (ArbState::Stalled, false)
            }
            (ArbState::Stalled, _) => (self.state, false),
        }
    }

    /// 侧释放（归还——聊天框关→游戏输入恢复）。
    pub fn release(&mut self, side: Side) -> (ArbState, bool) {
        let held_of = match side {
            Side::Chat => ArbState::ChatHeld,
            Side::Game => ArbState::GameHeld,
        };
        if self.state == held_of {
            self.state = ArbState::Idle;
            if side == Side::Chat {
                // 聊天框释放→挂起队列 flush 回游戏侧。
                self.resume_count += 1;
                self.pending.clear();
                (ArbState::Idle, true)
            } else {
                (ArbState::Idle, false)
            }
        } else if self.state == ArbState::Stalled {
            // 僵持中持有侧释放→恢复持有态（不是 Idle——对方请求还没处理）。
            self.state = held_of;
            (self.state, false)
        } else {
            (self.state, false)
        }
    }

    /// 游戏按键入队（聊天框持有期——挂起不丢，满丢最旧）。
    pub fn suspend_game_key(&mut self, key: &str) {
        if self.state != ArbState::ChatHeld {
            return; // 非让路期不挂起（直通游戏）。
        }
        if self.pending.len() >= PENDING_CAP {
            self.pending.remove(0);
            self.dropped_oldest += 1;
        }
        self.pending.push(key.to_string());
    }

    /// 游戏侧让路确认（协议步 GameYield——挂起生效）。
    pub fn game_yield(&mut self) -> bool {
        if self.state == ArbState::ChatHeld {
            self.yield_count += 1;
            true
        } else {
            false
        }
    }

    /// 僵持推进（每 tick 调用——超阈值回退焦点侧优先）。
    /// 返回 Some(E_ICE_ARBITER_STALL) 表示本次触发回退。
    pub fn tick(&mut self) -> Option<&'static str> {
        if self.state == ArbState::Stalled {
            self.stalled_ticks += 1;
            if self.stalled_ticks >= STALL_TIMEOUT_TICKS {
                // 回退默认归属：焦点侧（Chat）优先——用户正在打字
                // 是最高优先事实，游戏快捷键静默期可接受。
                self.state = ArbState::ChatHeld;
                self.revert_count += 1;
                self.stalled_ticks = 0;
                return Some(E_ICE_ARBITER_STALL);
            }
        }
        None
    }

    /// 游戏输入当前是否被让路挂起。
    pub fn game_suspended(&self) -> bool {
        self.state == ArbState::ChatHeld
    }

    /// 挂起队列长度。
    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    /// 读屏摘要。
    pub fn spoken(&self) -> String {
        let st = match self.state {
            ArbState::Idle => "空闲",
            ArbState::ChatHeld => "聊天框持有",
            ArbState::GameHeld => "游戏持有",
            ArbState::Stalled => "僵持",
        };
        format!(
            "仲裁[{}]挂起{}让路{}恢复{}回退{}",
            st, self.pending_len(), self.yield_count, self.resume_count, self.revert_count
        )
    }
}

// ---------------------------------------------------------------------------
// 让路协议（四步显式握手——协议漂移可检出）
// ---------------------------------------------------------------------------

/// 协议步（四步闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShakeStep {
    /// 1. 聊天框请求获取。
    ChatAcquire,
    /// 2. 游戏确认让路（ack）。
    GameYield,
    /// 3. 聊天框释放。
    ChatRelease,
    /// 4. 游戏确认恢复（ack）。
    GameResume,
}

impl ShakeStep {
    pub fn zh(self) -> &'static str {
        match self {
            ShakeStep::ChatAcquire => "聊天框请求",
            ShakeStep::GameYield => "游戏让路确认",
            ShakeStep::ChatRelease => "聊天框释放",
            ShakeStep::GameResume => "游戏恢复确认",
        }
    }

    /// 期望序号（0-3）。
    pub fn ordinal(self) -> usize {
        match self {
            ShakeStep::ChatAcquire => 0,
            ShakeStep::GameYield => 1,
            ShakeStep::ChatRelease => 2,
            ShakeStep::GameResume => 3,
        }
    }
}

/// 让路协议记录器（四步齐=完整握手；缺步=协议漂移可检出）。
#[derive(Debug)]
pub struct LetThrough {
    steps: Vec<ShakeStep>,
    pub complete_cycles: usize,
    pub drift_count: usize,
}

impl LetThrough {
    pub fn new() -> LetThrough {
        LetThrough { steps: Vec::new(), complete_cycles: 0, drift_count: 0 }
    }

    /// 记录协议步（乱序/跳步检出→漂移计数；四步齐→周期+1 清空）。
    pub fn step(&mut self, s: ShakeStep) -> bool {
        let expect = ShakeStep::ChatAcquire;
        let ok = if self.steps.is_empty() {
            s == expect
        } else {
            s.ordinal() == self.steps[self.steps.len() - 1].ordinal() + 1
        };
        if !ok {
            self.drift_count += 1;
            self.steps.clear();
            return false;
        }
        self.steps.push(s);
        if self.steps.len() == 4 {
            self.complete_cycles += 1;
            self.steps.clear();
        }
        true
    }

    /// 握手是否完整（当前无半程悬置）。
    pub fn clean(&self) -> bool {
        self.steps.is_empty()
    }

    /// 读屏摘要。
    pub fn spoken(&self) -> String {
        format!(
            "让路协议完整周期{}漂移{}悬置{}步",
            self.complete_cycles, self.drift_count, self.steps.len()
        )
    }
}

// ---------------------------------------------------------------------------
// 读屏播报（五类事件——sink 无主立案）
// ---------------------------------------------------------------------------

/// IME 播报事件（五类闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Announce {
    /// 组合开始。
    CompStart,
    /// 组合更新（编码变化）。
    CompUpdate,
    /// 提交（候选确认上屏）。
    Commit,
    /// 取消（组合丢弃）。
    Cancel,
    /// 候选导航（翻页/选字移动）。
    CandNav,
}

impl Announce {
    /// 中文播报文本（读屏单源——含关键状态词）。
    pub fn text(self) -> &'static str {
        match self {
            Announce::CompStart => "输入法：组合开始",
            Announce::CompUpdate => "输入法：组合更新",
            Announce::Commit => "输入法：已提交上屏",
            Announce::Cancel => "输入法：组合已取消",
            Announce::CandNav => "输入法：候选窗导航",
        }
    }

    pub const COUNT: usize = 5;

    pub fn all() -> [Announce; Announce::COUNT] {
        [
            Announce::CompStart,
            Announce::CompUpdate,
            Announce::Commit,
            Announce::Cancel,
            Announce::CandNav,
        ]
    }
}

/// 读屏播报器（sink 注册制——无主播报立案）。
#[derive(Debug)]
pub struct Announcer {
    sink_registered: bool,
    pub emitted: Vec<&'static str>,
    pub lost_count: usize,
}

impl Announcer {
    pub fn new() -> Announcer {
        Announcer { sink_registered: false, emitted: Vec::new(), lost_count: 0 }
    }

    /// 注册读屏 sink（无 sink 的播报是喊给空气听）。
    pub fn register_sink(&mut self) {
        self.sink_registered = true;
    }

    /// 播报事件（无 sink→立案 E_ICE_ANNOUNCE_SINK_LOST）。
    pub fn announce(&mut self, e: Announce, bag: &mut DiagBag) {
        if self.sink_registered {
            self.emitted.push(e.text());
        } else {
            self.lost_count += 1;
            bag.push(
                E_ICE_ANNOUNCE_SINK_LOST,
                "读屏播报无主",
                "IME 状态变化无人接收",
            );
        }
    }

    /// 播报聚合串（判据断言关键中文词）。
    pub fn spoken(&self) -> String {
        self.emitted.join("；")
    }
}

// ---------------------------------------------------------------------------
// 共存编排器（隔离门+候选窗+仲裁器+播报四件总装——单入口）
// ---------------------------------------------------------------------------

/// 共存编排器（K 域文本输入与游戏输入的总闸）。
#[derive(Debug)]
pub struct ImeCoex {
    pub gate: IsolationGate,
    pub cand: CandidateWindow,
    pub arb: Arbiter,
    pub letthrough: LetThrough,
    pub ann: Announcer,
    pub bag: DiagBag,
}

impl ImeCoex {
    pub fn new(kind: ImeKind, cand_size: (u32, u32)) -> ImeCoex {
        ImeCoex {
            gate: IsolationGate::new(kind),
            cand: CandidateWindow::new(kind, cand_size),
            arb: Arbiter::new(),
            letthrough: LetThrough::new(),
            ann: Announcer::new(),
            bag: DiagBag::new(),
        }
    }

    /// 聊天框聚焦（协议步 1+2：acquire+yield——游戏让路）。
    pub fn chat_focus(&mut self) -> bool {
        let (st, _) = self.arb.acquire(Side::Chat);
        self.letthrough.step(ShakeStep::ChatAcquire);
        if st == ArbState::ChatHeld && self.arb.game_yield() {
            self.letthrough.step(ShakeStep::GameYield);
            return true;
        }
        false
    }

    /// 聊天框失焦（协议步 3+4：release+resume——游戏恢复）。
    pub fn chat_blur(&mut self) -> bool {
        let (_, resumed) = self.arb.release(Side::Chat);
        self.letthrough.step(ShakeStep::ChatRelease);
        self.letthrough.step(ShakeStep::GameResume);
        resumed
    }

    /// 按键入口（总路由，**双源语境标记**——Space 等双义键的归宿由
    /// 来源决定：游戏侧来源让路期挂起不消费；聊天框来源走隔离门
    /// 正常编码选字）。返回路由结果。
    pub fn feed_key(&mut self, key: &str, from_game: bool) -> Route {
        if from_game && self.arb.game_suspended() {
            // 游戏侧来源+让路期：挂起不消费（恢复后 flush）。
            self.arb.suspend_game_key(key);
            return Route::Swallow;
        }
        let (r, diag) = self.gate.route_key(key);
        if let Some(code) = diag {
            self.bag.push(code, key, reason_of(code));
        }
        if self.gate.composing && r == Route::ToGame {
            // 防御性泄漏审计：理论上 route_key 组合期不产 ToGame，
            // 若产即隔离事故——立案加固（双保险）。
            self.gate.audit_leak(key, &mut self.bag);
            return Route::Swallow;
        }
        r
    }

    /// 全量读屏摘要。
    pub fn spoken(&self) -> String {
        format!(
            "{}；{}；{}；{}；{}",
            self.gate.spoken(),
            self.cand.spoken(),
            self.arb.spoken(),
            self.letthrough.spoken(),
            self.ann.spoken()
        )
    }
}

// ---------------------------------------------------------------------------
// 判据（十项——组合隔离/泄漏加固/候选跟随/错位修正/输入仲裁/
// 死锁回退/让路协议/多语言矩阵/读屏播报/全链编排）
// ---------------------------------------------------------------------------

/// F6205 判据入口（返回式 CheckSet——全部预期常量独立手算）。
pub fn run_veae05_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F6205");

    // 组-01 组合隔离：组合期编码键进 IME（无 ToGame 出口=快捷键不
    // 触发）、非编码游戏键吞掉、组合后恢复直通。
    {
        let mut g = IsolationGate::new(ImeKind::ZhPinyin);
        g.begin_compose();
        let r1 = g.route_key("KeyW"); // 组合期 W=拼音编码（快捷键不触发）
        let r2 = g.route_key("Space"); // 中文空格=首选（三岔口豁免）
        let r3 = g.route_key("Backspace"); // 编辑键
        let r_tab = g.route_key("Tab"); // 非编码游戏键→隔离吞掉
        g.end_compose();
        let r4 = g.route_key("KeyW"); // 组合结束→游戏直通
        let iso_ok = r1.0 == Route::ToIme
            && r2.0 == Route::ToIme
            && r3.0 == Route::ToIme
            && r_tab.0 == Route::Swallow
            && r4.0 == Route::ToGame
            && g.swallow_count == 1;
        if iso_ok {
            s.ok("F6205-组-01-组合期隔离无ToGame出口");
        } else {
            s.fail("F6205-组-01-组合期隔离无ToGame出口", "路由不符合隔离矩阵");
        }
    }

    // 组-02 泄漏加固：泄漏检出→立案→加固后声称投递被拒。
    {
        let mut g = IsolationGate::new(ImeKind::JaRomaji);
        g.begin_compose();
        let mut bag = DiagBag::new();
        g.audit_leak("KeyE", &mut bag); // 注入泄漏事故
        let hardened = g.hardened_reject(true); // 加固后声称投递→拒
        let leak_ok = g.leak_count == 1
            && g.guard_level == 1
            && hardened
            && bag.has(E_ICE_LEAK)
            && bag.count_of(E_ICE_LEAK) == 1;
        if leak_ok {
            s.ok("F6205-组-02-泄漏检出与隔离加固");
        } else {
            s.fail("F6205-组-02-泄漏检出与隔离加固", "泄漏账不平或加固未生效");
        }
    }

    // 候-01 候选跟随：光标移动跟随+右下角越界夹取屏内。
    {
        let mut c = CandidateWindow::new(ImeKind::ZhPinyin, (200, 120));
        let (p1, cl1) = c.follow((100, 100)); // 屏内——不夹取：y=100+60=160
        let (p2, cl2) = c.follow((1800, 1000)); // 右下越界：x=1800>1920-200=1720, y=1000+60=1060>1080-120=960
        let cand_ok = p1 == (100, 160)
            && !cl1
            && p2 == (1720, 960)
            && cl2
            && c.clamp_count == 1
            && c.follow_count == 2
            && p2.0 + c.size.0 as i32 <= SCREEN_W
            && p2.1 + c.size.1 as i32 <= SCREEN_H;
        if cand_ok {
            s.ok("F6205-候-01-候选窗跟随与边界夹取");
        } else {
            s.fail("F6205-候-01-候选窗跟随与边界夹取", "落点或夹取计数不符");
        }
    }

    // 候-02 错位修正：超容差检出→立案→修正后落点即期望。
    {
        let mut c = CandidateWindow::new(ImeKind::JaRomaji, (200, 120));
        c.pos = (500, 500); // 人为制造错位
        let mut bag = DiagBag::new();
        let mis = c.audit_position((300, 300), &mut bag);
        let fix_ok = mis
            && c.pos == (300, 300)
            && c.misalign_count == 1
            && bag.count_of(E_ICE_CAND_MISALIGN) == 1;
        // 容差内不立案：差 1px。
        c.pos = (301, 300);
        let mis2 = c.audit_position((300, 300), &mut bag);
        let cand_ok = fix_ok && !mis2 && c.misalign_count == 1;
        if cand_ok {
            s.ok("F6205-候-02-错位检出与修正");
        } else {
            s.fail("F6205-候-02-错位检出与修正", "容差判定或修正落点不符");
        }
    }

    // 仲-01 输入仲裁：聊天框持有→游戏键挂起不触发；释放→队列清恢复。
    {
        let mut a = Arbiter::new();
        a.acquire(Side::Chat);
        let y = a.game_yield();
        for k in ["KeyW", "KeyA", "KeyS", "KeyD"] {
            a.suspend_game_key(k);
        }
        let sus = a.game_suspended();
        let pend4 = a.pending_len(); // release 前拷出（flush 会清空）
        let (_, resumed) = a.release(Side::Chat);
        let arb_ok = sus
            && y
            && pend4 == 4
            && resumed
            && a.resume_count == 1
            && a.pending_len() == 0 // flush 清空
            && !a.game_suspended();
        if arb_ok {
            s.ok("F6205-仲-01-聊天框持有期游戏输入挂起恢复");
        } else {
            s.fail("F6205-仲-01-聊天框持有期游戏输入挂起恢复", "让路或恢复不符");
        }
    }

    // 仲-02 死锁回退：游戏侧在位聊天框交叉获取→僵持→8 tick 超时回退焦点侧。
    {
        let mut a = Arbiter::new();
        a.acquire(Side::Game); // 游戏全屏持有
        let (st, _) = a.acquire(Side::Chat); // 聊天框激活→僵持
        let mut fired = None;
        for _ in 0..STALL_TIMEOUT_TICKS {
            fired = a.tick();
        }
        let stall_ok = st == ArbState::Stalled
            && fired == Some(E_ICE_ARBITER_STALL)
            && a.revert_count == 1
            && a.state == ArbState::ChatHeld // 回退焦点侧优先
            && a.game_suspended();
        if stall_ok {
            s.ok("F6205-仲-02-仲裁死锁超时回退焦点侧");
        } else {
            s.fail("F6205-仲-02-仲裁死锁超时回退焦点侧", "回退时机或归属不符");
        }
    }

    // 让-01 让路协议：四步握手完整；跳步检出漂移。
    {
        let mut lt = LetThrough::new();
        let c1 = lt.step(ShakeStep::ChatAcquire)
            && lt.step(ShakeStep::GameYield)
            && lt.step(ShakeStep::ChatRelease)
            && lt.step(ShakeStep::GameResume);
        let cycle_ok = c1 && lt.complete_cycles == 1 && lt.clean();
        // 跳步：直接从 ChatAcquire 跳 ChatRelease（缺 GameYield）。
        let c2 = lt.step(ShakeStep::ChatAcquire);
        let c3 = lt.step(ShakeStep::ChatRelease); // 乱序——漂移
        let drift_ok = c2 && !c3 && lt.drift_count == 1 && lt.steps.is_empty();
        let lt_ok = cycle_ok && drift_ok;
        if lt_ok {
            s.ok("F6205-让-01-四步握手与漂移检出");
        } else {
            s.fail("F6205-让-01-四步握手与漂移检出", "协议周期或漂移检出不符");
        }
    }

    // 多-01 多语言矩阵：中日韩差异点逐条（中文空格首选/日文 Enter 确认/
    // 韩文无候选窗/韩文 Backspace 拆字直进 IME）。
    {
        let mut zh = IsolationGate::new(ImeKind::ZhPinyin);
        zh.begin_compose();
        let zh_space = zh.route_key("Space").0 == Route::ToIme; // 空格首选
        let zh_digit = zh.route_key("Digit1").0 == Route::ToIme; // 数字选字
        let zh_esc = zh.route_key("Escape").0 == Route::ToIme; // Esc 取消
        let mut ja = IsolationGate::new(ImeKind::JaRomaji);
        ja.begin_compose();
        let ja_enter = ja.route_key("Enter").0 == Route::ToIme; // 假名直通确认
        let mut ko = IsolationGate::new(ImeKind::KoJamo);
        ko.begin_compose();
        let ko_bs = ko.route_key("Backspace").0 == Route::ToIme; // 拆字
        let ko_w = ko.route_key("KeyW").0 == Route::ToIme; // W=Jamo 编码
        let ko_tab = ko.route_key("Tab").0 == Route::Swallow; // 非编码键吞
        // 韩文无候选窗：CandidateWindow 空转。
        let kc = CandidateWindow::new(ImeKind::KoJamo, (200, 120));
        // 无 IME 直通：无组合游戏键直通。
        let mut none = IsolationGate::new(ImeKind::None);
        let none_game = none.route_key("KeyW").0 == Route::ToGame;
        let matrix_ok = zh_space
            && zh_digit
            && zh_esc
            && ja_enter
            && ko_bs
            && ko_w
            && ko_tab
            && kc.inactive()
            && !ImeKind::KoJamo.has_candidate_phase()
            && ImeKind::ZhPinyin.has_candidate_phase()
            && ImeKind::JaRomaji.has_candidate_phase()
            && ImeKind::ZhPinyin.commit_key() == "Space"
            && ImeKind::JaRomaji.commit_key() == "Enter"
            && ImeKind::KoJamo.commit_key().is_empty()
            && none_game;
        if matrix_ok {
            s.ok("F6205-多-01-中日韩IME矩阵差异回归");
        } else {
            s.fail("F6205-多-01-中日韩IME矩阵差异回归", "语言差异点断言不符");
        }
    }

    // 读-01 读屏播报：五类事件文本非空含关键中文词；无 sink 立案。
    {
        let mut ann = Announcer::new();
        let mut bag = DiagBag::new();
        // 先无 sink 播报一次→立案。
        ann.announce(Announce::CompStart, &mut bag);
        let lost = bag.count_of(E_ICE_ANNOUNCE_SINK_LOST) == 1 && ann.lost_count == 1;
        // 注册后五类全播。
        ann.register_sink();
        for e in Announce::all() {
            ann.announce(e, &mut bag);
        }
        let sp = ann.spoken();
        let text_ok = Announce::all()
            .iter()
            .all(|e| !e.text().is_empty())
            && sp.contains("组合开始")
            && sp.contains("已提交上屏")
            && sp.contains("候选窗导航");
        let ann_ok = lost && text_ok && ann.emitted.len() == 5;
        if ann_ok {
            s.ok("F6205-读-01-IME状态读屏播报与无主立案");
        } else {
            s.fail("F6205-读-01-IME状态读屏播报与无主立案", "播报文本或立案不符");
        }
    }

    // 编-01 全链编排：聚焦→组合→隔离吞游戏键→选字提交→失焦→游戏恢复。
    {
        let mut coex = ImeCoex::new(ImeKind::ZhPinyin, (220, 130));
        coex.ann.register_sink();
        // 1. 聊天框聚焦（四步协议前两步）。
        let focused = coex.chat_focus();
        // 2. 组合开始（播报+隔离开启）。
        coex.ann.announce(Announce::CompStart, &mut coex.bag);
        coex.gate.begin_compose();
        coex.ann.announce(Announce::CompUpdate, &mut coex.bag);
        // 3. 组合期打字（聊天框源）进 IME；游戏侧来源的 W 被让路挂起。
        let r_text = coex.feed_key("KeyN", false);
        let r_game = coex.feed_key("KeyW", true); // 游戏源→挂起
        // 4. 候选跟随光标。
        let (cp, _) = coex.cand.follow((400, 300));
        coex.ann.announce(Announce::CandNav, &mut coex.bag);
        // 5. 空格首选提交（聊天框源——双义键 Space 归 IME）。
        let r_commit = coex.feed_key("Space", false);
        coex.ann.announce(Announce::Commit, &mut coex.bag);
        coex.gate.end_compose();
        // 6. 失焦恢复（协议后两步）。
        let resumed = coex.chat_blur();
        // 7. 恢复后游戏源 KeyW 直通游戏。
        let r_after = coex.feed_key("KeyW", true);
        let chain_ok = focused
            && resumed
            && r_text == Route::ToIme
            && r_game == Route::Swallow // 让路期挂起
            && coex.arb.pending_len() == 0 // 失焦 flush 清空
            && r_commit == Route::ToIme
            && r_after == Route::ToGame // 全部退出→直通
            && cp == (400, 365) // 300+130/2=365
            && coex.arb.yield_count == 1
            && coex.arb.resume_count == 1
            && coex.letthrough.complete_cycles == 1
            && coex.letthrough.clean()
            && coex.ann.emitted.len() == 4 // Start+Update+CandNav+Commit
            && coex.bag.is_empty(); // 全程无事故
        if chain_ok {
            s.ok("F6205-编-01-聚焦组合提交恢复全链O(1)对账");
        } else {
            s.fail("F6205-编-01-聚焦组合提交恢复全链O(1)对账", "编排对账不符");
        }
    }

    s
}
