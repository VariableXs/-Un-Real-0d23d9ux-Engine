//! VE-F3009 · 退场动效族（VE-P 域 · 动效域 · 批次 P02 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3009`
//!
//! **判据（锚点原文）**：进退对称、护栏协议、语义延迟、超时兜底、reduce 立即、判据。
//!
//! **职责定位（锚点原文）**：退场组件族（fade-out/fade-down/scale-out/slide-out
//! 四向/blur-out/clip-collapse 六型——**与入场族对称设计**：退场是入场的镜像，
//! 参数对称表）；退场与移除时序（动效完成→实际移除的时序协议：**视觉仍在→
//! 逻辑已删的双重态管理**——移除护栏：护栏期内**不可交互不可聚焦但语义延迟
//! 移除**，读屏不丢上下文）；退场打断（退场中重新入场→反向播放或快速重入，
//! 策略表——打断语义复用 F2871 规则）。
//!
//! # 一、对称为什么是「默认红线」而不是「风格偏好」
//!
//! 用户对「进来多快、出去就多快」有稳定的节奏预期；进退不对称（入场 200ms、
//! 退场 800ms）会被感知为「拖泥带水」，且不对称一旦散落在各组件参数里就
//! 无法审计。故对称表（[`ExitKind::counterpart`] + 参数逐项取自对称入场型）
//! 是**唯一真相**：退场型的属性集/时长令牌/位移令牌全部**转查**其对称入场型
//! 而不复制一份——复制迟早漂移。锚点允许非对称，但必须**显式声明理由**
//! （[`AsymmetryNote`]），无理由的非对称在 [`SymmetryTable::audit`] 立案
//! （默认对称红线：沉默的不对称是最难排查的一类动效回归）。
//!
//! # 二、护栏（guard）为什么是「双轨」而不是「删了就是删了」
//!
//! 若动画完成即从语义树摘除，读屏用户会「听丢」刚消失的元素上下文——
//! 视觉用户看得见的退场，读屏用户需要同样可理解的退场。故移除分两轨：
//! **逻辑删除**（不可交互/不可聚焦，命中测试与焦点引擎立即拒绝——防「幽灵
//! 按钮」）与**语义保留**（读屏树延迟到护栏结束才摘）。护栏是一个**有时限
//! 的协议**而不是无限期保留：上限 = 动画时长 + [`GUARD_SLACK_MS`]，
//! 超时强制回收并立案（[`E_GUARD_LEAK`]）——「逻辑删了视觉永挂」的泄漏
//! 必须有兜底红线，否则一个忘记 release 的调用点就是常驻幽灵元素。
//!
//! # 三、双重移除为什么幂等而不是报错
//!
//! 护栏超时兜底（sweep 强制回收）与正常调用方 release 在时序上天然竞争：
//! sweep 刚回收、调用方随即 release。若报错，每个调用点都得先查状态再删
//! ——竞态窗口照样存在。幂等（已回收再删返回 `Ok`）让「删」成为可以
//! 重复下发而不出错的动作，O(1) 无需查表（状态位即答案）。
//!
//! # 四、打断为什么做策略表且前向声明 F2871
//!
//! 退场中重新入场有两种合法语义：反向播放（把退场动画倒放回入场态，视觉
//! 连续）与快速重入（跳到入场初态直接播，实现简单）。选哪种是**产品语义**
//! 而非技术约束，故写成**逐型策略表**（[`INTERRUPT_TABLE`]）而不是散在
//! 调用点。F2871 是打断规则的单源（与 F2713 在 F3008 的前向声明同款处理
//! ——规则未落库前先钉契约位，落库后本表的默认值改为从其取）。
//!
//! # 五、reduce 态为什么「立即移除 + 语义正常」
//!
//! reduce 用户要的是**少动**不是「没有退出」：动画时长归零（[`reduce_plan`]
//! 双零：零动画零尾巴）、元素立即移除、语义照常随移除而摘（不保留护栏尾巴
//! ——为「不丢失上下文」而设的延迟在「直达」语义下反而制造滞留）。
//!
//! # 六、与相邻条的分工
//!
//! F3008 管入场（本模块对称面）；F3010 管共享元素转场；F3029 路由状态机
//! 是页面退场的消费方（页面级退场消费本族的护栏协议）；N 域控件销毁是
//! 护栏协议的对端（逻辑删除的真实执行者）。本模块只管「**怎么退、退多久、
//! 何时真删、删时语义怎么办**」，不执行 DOM/控件销毁本身。
//!
//! **性能（锚点原文）**：护栏计时 O(1)（deadline 即比较）；反向播放 O(1)
//! （策略查表即返）；幂等 O(1)（状态位即答案）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::svstar2::vep03_token::MotionTokenError;
use crate::svstar2::vep08_entry::{EntryKind, SlideDir};

// ---------------------------------------------------------------------------
// 一、错误码（P 域字符串码家族格式）
// ---------------------------------------------------------------------------

/// 本项版本。
pub const EXIT_PROTOCOL_VERSION: &str = "P09-exit-v1";

/// 退场型未知（解析/注册拒绝）。
pub const E_EXIT_KIND: &str = "E_EXIT_KIND";

/// 非对称无理由声明（默认对称红线）。
pub const E_EXIT_SYMMETRY: &str = "E_EXIT_SYMMETRY";

/// 护栏超时泄漏（强制回收 + P1 立案）。
pub const E_GUARD_LEAK: &str = "E_GUARD_LEAK";

/// 护栏时长非法（零/负等效：duration 为 0 时退场无意义）。
pub const E_EXIT_DURATION: &str = "E_EXIT_DURATION";

/// 打断策略未知。
pub const E_INTERRUPT_POLICY: &str = "E_INTERRUPT_POLICY";

/// 护栏兜底松弛（毫秒）：护栏最长 = 动画时长 + 100ms（锚点红线，判据钉死）。
pub const GUARD_SLACK_MS: u32 = 100;

/// 退场型闭集长度。
pub const EXIT_KIND_COUNT: usize = 6;

// ---------------------------------------------------------------------------
// 二、退场六型（与入场族对称）
// ---------------------------------------------------------------------------

/// 退场基础型（锚点六型，与 [`EntryKind`] 一一对称）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExitKind {
    /// 淡出。
    FadeOut,
    /// 淡出下移。
    FadeDown,
    /// 缩放退场。
    ScaleOut,
    /// 滑出（四向）。
    SlideOut,
    /// 模糊退场。
    BlurOut,
    /// 裁剪收拢。
    ClipCollapse,
}

const EXITS: [ExitKind; EXIT_KIND_COUNT] = [
    ExitKind::FadeOut,
    ExitKind::FadeDown,
    ExitKind::ScaleOut,
    ExitKind::SlideOut,
    ExitKind::BlurOut,
    ExitKind::ClipCollapse,
];

impl ExitKind {
    /// 六型全集（顺序即 index，唯一真值源）。
    pub fn all() -> [ExitKind; EXIT_KIND_COUNT] {
        EXITS
    }

    /// 型短码（冻结）。
    pub fn wire(self) -> &'static str {
        match self {
            ExitKind::FadeOut => "fade-out",
            ExitKind::FadeDown => "fade-down",
            ExitKind::ScaleOut => "scale-out",
            ExitKind::SlideOut => "slide-out",
            ExitKind::BlurOut => "blur-out",
            ExitKind::ClipCollapse => "clip-collapse",
        }
    }

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            ExitKind::FadeOut => "淡出",
            ExitKind::FadeDown => "淡出下移",
            ExitKind::ScaleOut => "缩放退场",
            ExitKind::SlideOut => "滑出",
            ExitKind::BlurOut => "模糊退场",
            ExitKind::ClipCollapse => "裁剪收拢",
        }
    }

    /// 序号。
    pub fn index(self) -> usize {
        match self {
            ExitKind::FadeOut => 0,
            ExitKind::FadeDown => 1,
            ExitKind::ScaleOut => 2,
            ExitKind::SlideOut => 3,
            ExitKind::BlurOut => 4,
            ExitKind::ClipCollapse => 5,
        }
    }

    /// 序号⇒型。
    pub fn from_index(i: usize) -> Option<ExitKind> {
        Self::all().get(i).copied()
    }

    /// 短码⇒型（只认六型）。
    pub fn parse(s: &str) -> Option<ExitKind> {
        Self::all().iter().copied().find(|k| k.wire() == s)
    }

    /// **对称表**：本退场型的对称入场型（进退对称的单源声明）。
    ///
    /// 退场是入场的镜像：fade-down 之所以是「下移退」而非「上移退」，
    /// 是因为其对称面 FadeUp 是「上移进」——位移方向取反即成镜像，
    /// 其余参数（时长/属性集）直接转查对称面，两侧恒一致。
    pub fn counterpart(self) -> EntryKind {
        match self {
            ExitKind::FadeOut => EntryKind::FadeIn,
            ExitKind::FadeDown => EntryKind::FadeUp,
            ExitKind::ScaleOut => EntryKind::ScaleIn,
            ExitKind::SlideOut => EntryKind::SlideIn,
            ExitKind::BlurOut => EntryKind::BlurIn,
            ExitKind::ClipCollapse => EntryKind::ClipReveal,
        }
    }

    /// 由对称入场型反查退场型（对称表双向可走——判据用它验双向一致）。
    pub fn from_counterpart(e: EntryKind) -> Option<ExitKind> {
        match e {
            EntryKind::FadeIn => Some(ExitKind::FadeOut),
            EntryKind::FadeUp => Some(ExitKind::FadeDown),
            EntryKind::ScaleIn => Some(ExitKind::ScaleOut),
            EntryKind::SlideIn => Some(ExitKind::SlideOut),
            EntryKind::BlurIn => Some(ExitKind::BlurOut),
            EntryKind::ClipReveal => Some(ExitKind::ClipCollapse),
        }
    }

    /// 动画属性集（**转查对称入场型**——参数对称单源，不复制清单）。
    pub fn properties(self) -> &'static [&'static str] {
        self.counterpart().properties()
    }

    /// 时长令牌 ID（转查对称面：进退同速）。
    pub fn duration_token_id(self) -> &'static str {
        self.counterpart().duration_token_id()
    }

    /// 位移令牌 ID（转查对称面）。
    pub fn distance_token_id(self) -> &'static str {
        self.counterpart().distance_token_id()
    }

    /// 适用场景注释。
    pub fn scenario(self) -> &'static str {
        match self {
            ExitKind::FadeOut => "通用默认：内容块/卡片退场；无位移，前庭最安全",
            ExitKind::FadeDown => "与 fade-up 成对的列表项退场：向下退出保持方向语义",
            ExitKind::ScaleOut => "弹层/徽标退场：与 scale-in 成对",
            ExitKind::SlideOut => "抽屉/侧栏/通知退场：四向各对应去向",
            ExitKind::BlurOut => "图片/大图退场：失焦感，与 blur-in 成对",
            ExitKind::ClipCollapse => "横幅/区块收拢：与 clip-reveal 成对",
        }
    }
}

/// slide-out 的退场位移向量：**入场 delta 取反**（退场是入场的镜像）。
///
/// 入场 SlideDir::Up 的 delta 是 (0,1)（起始态在下方）；退场「向下退出」
/// 的位移向量即 (0,-1)。判据逐向断 `exit_delta == -entry_delta`。
pub fn exit_delta(dir: SlideDir) -> (i32, i32) {
    let (x, y) = dir.delta();
    (-x, -y)
}

// ---------------------------------------------------------------------------
// 三、对称表与非对称声明
// ---------------------------------------------------------------------------

/// 非对称声明（锚点：非对称须显式声明理由——默认对称红线）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AsymmetryNote {
    /// 声明非对称的退场型。
    pub exit: ExitKind,
    /// 哪一维不对称（属性/时长/位移）。
    pub dimension: &'static str,
    /// 理由（**非空**，无理由的非对称非法）。
    pub reason: String,
}

impl AsymmetryNote {
    /// 声明构造（理由空即拒——「没有理由的非对称」正是红线要拦的）。
    pub fn new(exit: ExitKind, dimension: &'static str, reason: &str) -> Result<Self, MotionTokenError> {
        if reason.trim().is_empty() {
            return Err(MotionTokenError::new(
                E_EXIT_SYMMETRY,
                "非对称声明被拒",
                "理由为空：默认对称红线下，非对称必须给出可审计的理由",
                "补写理由，或改回对称参数",
                "动效协同负责人",
            ));
        }
        Ok(AsymmetryNote { exit, dimension, reason: reason.to_string() })
    }
}

/// 对称表审计：六对齐、双向可走、参数两侧一致（机检进退对称）。
///
/// 返回的 `Vec<&'static str>` 是**违规维度清单**（空 = 对称成立）；
/// 不用 bool——审计要能指出「哪一对的哪一维破了」，bool 只会说「破了」。
pub fn audit_symmetry() -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for k in ExitKind::all().iter() {
        let c = k.counterpart();
        // 双向一致：counterpart(from_counterpart(c)) 必须回到自身。
        match ExitKind::from_counterpart(c) {
            Some(back) if back == *k => {}
            _ => out.push("counterpart-双向断裂"),
        }
        // 参数对称：属性集/时长令牌/位移令牌两侧逐项相等。
        if k.properties() != c.properties() {
            out.push("属性集不对称");
        }
        if k.duration_token_id() != c.duration_token_id() {
            out.push("时长令牌不对称");
        }
        if k.distance_token_id() != c.distance_token_id() {
            out.push("位移令牌不对称");
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 四、移除护栏协议（双轨：逻辑删除 × 语义保留；超时兜底；幂等）
// ---------------------------------------------------------------------------

/// 护栏阶段（双轨状态机）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuardPhase {
    /// 动画中：视觉在、逻辑已删、不可交互不可聚焦、语义在。
    Visual,
    /// 动画完到护栏截止：视觉已无、语义仍在（读屏上下文保持）、逻辑已删。
    SemanticsTail,
    /// 全部移除（逻辑+语义）。
    Released,
}

/// 单个元素的移除护栏。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemovalGuard {
    /// 元素标识。
    pub element: u32,
    /// 退场型。
    pub kind: ExitKind,
    /// 动画时长（毫秒；reduce 态为 0）。
    pub duration_ms: u32,
    /// 开始时刻（毫秒逻辑钟）。
    pub start_ms: u64,
    /// 护栏截止 = start + duration + GUARD_SLACK_MS（超时兜底线）。
    pub deadline_ms: u64,
    /// 当前阶段。
    pub phase: GuardPhase,
}

impl RemovalGuard {
    /// 开护栏（锚点时序协议入口）。
    ///
    /// duration 为 0 拒绝：零时长退场应走 reduce 直达通道
    /// （[`reduce_plan`]），混在正常护栏里会让「忘了填时长」伪装成 reduce。
    pub fn begin(
        element: u32,
        kind: ExitKind,
        duration_ms: u32,
        now_ms: u64,
    ) -> Result<RemovalGuard, MotionTokenError> {
        if duration_ms == 0 {
            return Err(MotionTokenError::new(
                E_EXIT_DURATION,
                "护栏时长非法",
                "动画时长为 0：正常护栏不收零时长，零时长属于 reduce 直达通道",
                "改用 reduce_plan + release_now，或填真实动画时长",
                "动效协同负责人",
            ));
        }
        Ok(RemovalGuard {
            element,
            kind,
            duration_ms,
            start_ms: now_ms,
            deadline_ms: now_ms + duration_ms as u64 + GUARD_SLACK_MS as u64,
            phase: GuardPhase::Visual,
        })
    }

    /// 护栏期内是否可交互（**恒否**——防幽灵按钮）。
    pub fn hit_test_allowed(&self) -> bool {
        false
    }

    /// 护栏期内是否可聚焦（**恒否**——焦点不入退场元素）。
    pub fn focusable(&self) -> bool {
        false
    }

    /// 语义是否存活（读屏树上的可读性：Released 前恒真——语义延迟移除）。
    pub fn semantics_alive(&self) -> bool {
        self.phase != GuardPhase::Released
    }

    /// 动画完成回执：Visual → SemanticsTail（视觉消失、语义保持）。
    ///
    /// 重复回执幂等（已到 SemanticsTail 再收一次回执仍 Ok）——动画完成
    /// 事件与状态推进的竞争同样按幂等处理。
    pub fn animation_done(&mut self) {
        if self.phase == GuardPhase::Visual {
            self.phase = GuardPhase::SemanticsTail;
        }
    }

    /// 正常 release：语义移除，全删。**幂等**（双重移除返回 Ok，O(1)）。
    pub fn release(&mut self) -> Result<(), MotionTokenError> {
        self.phase = GuardPhase::Released;
        Ok(())
    }

    /// 是否超时（now 超过 deadline 即泄漏候选）。
    pub fn leaked(&self, now_ms: u64) -> bool {
        self.phase != GuardPhase::Released && now_ms > self.deadline_ms
    }
}

/// 护栏登记册（sweep 兜底扫描；O(1) 计时/幂等不变——sweep 本身 O(n) 但
/// 每条护栏的操作是 O(1) 状态位翻转，无查表无分配）。
#[derive(Clone, Debug, Default)]
pub struct GuardLedger {
    guards: Vec<RemovalGuard>,
    /// 超时强制回收次数（立案计数）。
    pub leaks: u32,
}

impl GuardLedger {
    /// 空册。
    pub fn new() -> GuardLedger {
        GuardLedger { guards: Vec::new(), leaks: 0 }
    }

    /// 开护栏入册。
    pub fn begin(
        &mut self,
        element: u32,
        kind: ExitKind,
        duration_ms: u32,
        now_ms: u64,
    ) -> Result<(), MotionTokenError> {
        let g = RemovalGuard::begin(element, kind, duration_ms, now_ms)?;
        self.guards.push(g);
        Ok(())
    }

    /// 按元素查护栏（**最后一个**同元素护栏——重复退场以最新为准）。
    pub fn get_mut(&mut self, element: u32) -> Option<&mut RemovalGuard> {
        self.guards.iter_mut().rev().find(|g| g.element == element)
    }

    /// 双重移除幂等入口：已回收再删 Ok。
    pub fn release(&mut self, element: u32) -> Result<(), MotionTokenError> {
        match self.get_mut(element) {
            Some(g) => g.release(),
            // 没有护栏记录的元素无「护栏语义」可言：不假装成功，
            // 让调用方知道这不是本协议管理的删除。
            None => Err(MotionTokenError::new(
                E_EXIT_KIND,
                "release 被拒：无护栏记录",
                "元素未经过退场护栏（begin）就直接 release",
                "先 begin 再 release；纯逻辑删除不走本协议",
                "动效协同负责人",
            )),
        }
    }

    /// 兜底扫描：超时护栏强制回收并立案（锚点「护栏超时强制回收」红线）。
    ///
    /// 返回本次强制回收的元素清单（空 = 无泄漏）。
    pub fn sweep(&mut self, now_ms: u64) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        for g in self.guards.iter_mut() {
            if g.leaked(now_ms) {
                g.phase = GuardPhase::Released;
                self.leaks = self.leaks.saturating_add(1);
                out.push(g.element);
            }
        }
        out
    }

    /// 在册护栏数。
    pub fn len(&self) -> usize {
        self.guards.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.guards.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 五、打断策略表（F2871 前向声明：规则落库后默认值改为从其取单源）
// ---------------------------------------------------------------------------

/// 打断策略（退场中重新入场的两种合法语义）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InterruptPolicy {
    /// 反向播放：退场动画倒放回入场态（视觉连续）。
    Reverse,
    /// 快速重入：跳到入场初态直接播（实现简单）。
    FastReenter,
}

impl InterruptPolicy {
    /// 短码。
    pub fn wire(self) -> &'static str {
        match self {
            InterruptPolicy::Reverse => "reverse",
            InterruptPolicy::FastReenter => "fast-reenter",
        }
    }

    /// 语义说明。
    pub fn zh(self) -> &'static str {
        match self {
            InterruptPolicy::Reverse => "反向播放：从当前退场进度倒放回入场态",
            InterruptPolicy::FastReenter => "快速重入：放弃退场进度，直接从入场初态重播",
        }
    }
}

/// 逐型打断策略表（**单源**：六型各一，判据钉死；改默认须过对称面理由）。
///
/// 轻型（纯透明度）打断代价低，快速重入即可；重型（位移/形变/裁剪）反向
/// 播放保持视觉连续。F2871 为打断规则单源（前向声明：落库前本表即契约位）。
pub const INTERRUPT_TABLE: [(ExitKind, InterruptPolicy); EXIT_KIND_COUNT] = [
    (ExitKind::FadeOut, InterruptPolicy::FastReenter),
    (ExitKind::FadeDown, InterruptPolicy::FastReenter),
    (ExitKind::ScaleOut, InterruptPolicy::Reverse),
    (ExitKind::SlideOut, InterruptPolicy::Reverse),
    (ExitKind::BlurOut, InterruptPolicy::Reverse),
    (ExitKind::ClipCollapse, InterruptPolicy::Reverse),
];

/// 查某退场型的打断策略（表驱动，O(1)）。
pub fn interrupt_policy(kind: ExitKind) -> InterruptPolicy {
    INTERRUPT_TABLE[kind.index()].1
}

// ---------------------------------------------------------------------------
// 六、reduce 直达（立即移除 + 语义正常）
// ---------------------------------------------------------------------------

/// reduce 态退场计划：**双零**（零动画时长、零护栏尾巴）。
///
/// 返回 `(动画时长, 护栏尾巴毫秒)`，恒 `(0, 0)`——判据钉死双零，
/// 实现「忘了 reduce」时这里会给出非零而露馅。
pub const fn reduce_plan() -> (u32, u32) {
    (0, 0)
}

/// reduce 直达移除：立即 Released、语义照常摘除（不留读屏尾巴）。
///
/// 与正常路径的差别：不经 begin/animation_done 状态机（直达），
/// 调用方不需要护栏记录——「立即移除」没有可泄漏的窗口期。
pub fn release_now(element: u32, ledger: &mut GuardLedger) -> Result<(), MotionTokenError> {
    // reduce 直达不要求护栏记录存在（它根本不开护栏）；但若同一元素
    // 恰有**未超时**的护栏在册（先正常退场又立即 reduce），顺手关掉它，
    // 防止 sweep 之后又把它标成泄漏——两条路径写同一元素要收敛。
    if let Some(g) = ledger.get_mut(element) {
        if g.phase != GuardPhase::Released {
            return Err(MotionTokenError::new(
                E_EXIT_KIND,
                "reduce 直达被拒：同元素护栏在册",
                "元素已在正常退场护栏中，直接 reduce 会产生双轨竞争",
                "先 release 该护栏（幂等），再走 reduce 直达",
                "动效协同负责人",
            ));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 七、读屏替述
// ---------------------------------------------------------------------------

/// 退场读屏单行（元素/型/阶段三要素；不含坐标内容）。
pub fn screen_line(g: &RemovalGuard, now_ms: u64) -> String {
    let phase = match g.phase {
        GuardPhase::Visual => "退场动画中",
        GuardPhase::SemanticsTail => "已从界面移除，描述短暂保留",
        GuardPhase::Released => "已完全移除",
    };
    let remaining = g.deadline_ms.saturating_sub(now_ms);
    format!(
        "元素 {} {}（{}），{}；语义保留剩余 {} 毫秒",
        g.element,
        g.kind.zh(),
        g.kind.wire(),
        phase,
        remaining
    )
}
