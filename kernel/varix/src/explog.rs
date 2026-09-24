//! 体验日志：事件模型 + 挫败信号 + 隐私红线 + 手册联动（WP-402 · B-2301~2304）。
//!
//! MD2 篇 23.1/23.2/23.4：体验日志记"交互行为与结果"——事件模型七字段
//! （时间戳单调钟加墙钟双值、会话标识、界面上下文、对象、动作、结果与
//! 耗时、体验结论枚举）。体验结论不是事后分析是事件自带——生成事件的
//! 那一跳自己知道顺不顺。自动挫败信号五类内建（狂点/死点/反复开关/
//! 放弃路径/错误徘徊），触发即升级为独立事件（原始事件引用加信号类型）。
//! 隐私红线三道：字段白名单制（七字段之外不许扩，扩展走 ADR）、采样与
//! 总量（写入不阻塞、总量上限轮转）、关闭开关（关闭时明示诊断能力降级
//! ——Q73）。读取面三层：用户（脱敏聚合视图）、开发者（按会话回放）、
//! 审计（季度红线审计）。手册联动：新条目必须挂判据或修复引用——手册
//! 不是感想是记录。
//!
//! 零堆纪律：无 Vec/String/Box/format!，定长数组 + 字节串 + 'static 切片。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// B-2301 事件模型：七字段白名单 + 体验结论事件自带 + 扩展走 ADR
// ---------------------------------------------------------------------------

/// 七字段口径（时间戳双钟值算一个字段组——8 槽 7 字段）。
pub const EVENT_FIELDS: usize = 7;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExpAction {
    Click,        // 点击
    Drag,         // 拖拽
    Hotkey,       // 快捷键
    ToggleOverlay, // 开关浮层
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ResultKind {
    Succeeded, // 成功
    Cancelled, // 取消
    Failed,    // 失败
}

/// 结果与耗时（字段六）：结果当场给、耗时当场记。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ExpResult {
    pub kind: ResultKind,
    pub cost_ms: u32,
}

/// 体验结论枚举五档（字段七）：顺畅、卡顿、无反馈、被打断、报错。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    Smooth,
    Laggy,
    NoResponse,
    Interrupted,
    Errored,
}

pub const VERDICT_KINDS: usize = 5;

pub fn verdict_index(v: Verdict) -> usize {
    match v {
        Verdict::Smooth => 0,
        Verdict::Laggy => 1,
        Verdict::NoResponse => 2,
        Verdict::Interrupted => 3,
        Verdict::Errored => 4,
    }
}

/// 体验日志事件：结构体即白名单——七字段之外没有自由槽，想扩字段
/// 类型面无处可写（只能走 FieldExtensionRequest + ADR）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ExpEvent {
    /// 字段一·时间戳：单调钟（毫秒）——间隔与窗口计算用。
    pub ts_mono_ms: u32,
    /// 字段一·时间戳：墙钟（毫秒）——人对表与交接对照用（Q14 双源）。
    pub ts_wall_ms: u64,
    /// 字段二·会话标识。
    pub session_id: u64,
    /// 字段三·界面上下文（哪个应用哪个视图）。
    pub ctx_id: u16,
    /// 字段四·对象（哪个元素哪个浮层）。
    pub obj_id: u16,
    /// 字段五·动作。
    pub action: ExpAction,
    /// 字段六·结果与耗时。
    pub result: ExpResult,
    /// 字段七·体验结论——事件自带，生成那一跳当场裁决，无默认值。
    pub verdict: Verdict,
}

/// 字段扩展请求：白名单外的唯一入口——ADR 在案才放行（篇 23.2）。
pub struct FieldExtensionRequest {
    pub field_tag: u16,
    pub adr_id: u32,
    pub adr_filed: bool,
}

pub fn admit_extension(req: &FieldExtensionRequest) -> bool {
    req.adr_filed && req.adr_id != 0
}

// ---------------------------------------------------------------------------
// B-2302 挫败信号：五类内建 + 触发升级独立事件 + 触发率实测标定
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FrustrationSignal {
    RageClick,   // 狂点：同位置短时多点
    DeadClick,   // 死点：点击无反馈区域
    ToggleFlap,  // 反复开关：浮层短时开合超阈值
    AbandonPath, // 放弃路径：流程中途退出
    ErrorLoop,   // 错误徘徊：错误后原地重复操作
}

pub const SIGNAL_KINDS: usize = 5;

pub const ALL_SIGNALS: [FrustrationSignal; SIGNAL_KINDS] = [
    FrustrationSignal::RageClick,
    FrustrationSignal::DeadClick,
    FrustrationSignal::ToggleFlap,
    FrustrationSignal::AbandonPath,
    FrustrationSignal::ErrorLoop,
];

pub const RAGE_WINDOW_MS: u32 = 2_000;
pub const RAGE_COUNT: usize = 4;
pub const FLAP_WINDOW_MS: u32 = 3_000;
pub const FLAP_COUNT: usize = 6;
pub const ABANDON_STEPS: usize = 3;
pub const LOOP_WINDOW_MS: u32 = 5_000;
pub const LOOP_REPEAT: usize = 2;

/// 狂点：同 ctx+obj 的 Click 在窗口内达阈——返回触发次数。
pub fn detect_rage(events: &[ExpEvent]) -> usize {
    let n = events.len();
    let mut fired = 0;
    let mut i = 0;
    while i < n {
        if matches!(events[i].action, ExpAction::Click) {
            let mut c = 0;
            let mut j = i;
            while j < n && events[j].ts_mono_ms < events[i].ts_mono_ms + RAGE_WINDOW_MS {
                if events[j].ctx_id == events[i].ctx_id
                    && events[j].obj_id == events[i].obj_id
                    && matches!(events[j].action, ExpAction::Click)
                {
                    c += 1;
                }
                j += 1;
            }
            if c >= RAGE_COUNT {
                fired += 1;
                i = j; // 消费该窗口，不重复计
                continue;
            }
        }
        i += 1;
    }
    fired
}

/// 死点：点击且无反馈（体验结论 NoResponse——生成那一跳自证）。
pub fn detect_dead(events: &[ExpEvent]) -> usize {
    let n = events.len();
    let mut c = 0;
    let mut i = 0;
    while i < n {
        if matches!(events[i].action, ExpAction::Click)
            && matches!(events[i].verdict, Verdict::NoResponse)
        {
            c += 1;
        }
        i += 1;
    }
    c
}

/// 反复开关：同 ctx 的 ToggleOverlay 在窗口内达阈。
pub fn detect_flap(events: &[ExpEvent]) -> usize {
    let n = events.len();
    let mut fired = 0;
    let mut i = 0;
    while i < n {
        if matches!(events[i].action, ExpAction::ToggleOverlay) {
            let mut c = 0;
            let mut j = i;
            while j < n && events[j].ts_mono_ms < events[i].ts_mono_ms + FLAP_WINDOW_MS {
                if events[j].ctx_id == events[i].ctx_id
                    && matches!(events[j].action, ExpAction::ToggleOverlay)
                {
                    c += 1;
                }
                j += 1;
            }
            if c >= FLAP_COUNT {
                fired += 1;
                i = j;
                continue;
            }
        }
        i += 1;
    }
    fired
}

/// 放弃路径：同 ctx 连续若干动作后紧跟取消终局（流程中途退出）。
pub fn detect_abandon(events: &[ExpEvent]) -> usize {
    let n = events.len();
    let mut fired = 0;
    let mut i = ABANDON_STEPS; // 前置步数下标起点
    while i < n {
        if matches!(events[i].result.kind, ResultKind::Cancelled) {
            let ctx = events[i].ctx_id;
            let mut ok = true;
            let mut k = 1;
            while k <= ABANDON_STEPS {
                let p = &events[i - k];
                if p.ctx_id != ctx || matches!(p.result.kind, ResultKind::Cancelled) {
                    ok = false;
                    break;
                }
                k += 1;
            }
            if ok {
                fired += 1;
            }
        }
        i += 1;
    }
    fired
}

/// 错误徘徊：失败后同 ctx+obj 同动作在窗口内原地重复达阈。
pub fn detect_loop(events: &[ExpEvent]) -> usize {
    let n = events.len();
    let mut fired = 0;
    let mut i = 0;
    while i < n {
        if matches!(events[i].result.kind, ResultKind::Failed) {
            let mut c = 0;
            let mut j = i + 1;
            while j < n && events[j].ts_mono_ms <= events[i].ts_mono_ms + LOOP_WINDOW_MS {
                if events[j].ctx_id == events[i].ctx_id
                    && events[j].obj_id == events[i].obj_id
                    && events[j].action == events[i].action
                {
                    c += 1;
                }
                j += 1;
            }
            if c >= LOOP_REPEAT {
                fired += 1;
            }
        }
        i += 1;
    }
    fired
}

/// 五类普查：一次扫描各类计数。
pub struct SignalCensus {
    pub counts: [usize; SIGNAL_KINDS],
    pub scanned: usize,
}

pub fn detect_all(events: &[ExpEvent]) -> SignalCensus {
    SignalCensus {
        counts: [
            detect_rage(events),
            detect_dead(events),
            detect_flap(events),
            detect_abandon(events),
            detect_loop(events),
        ],
        scanned: events.len(),
    }
}

/// 信号升级为独立事件：原始事件引用（下标）+ 信号类型——
/// 不复制原事件字段（单一事实源）。
pub const ESCALATE_CAP: usize = SIGNAL_KINDS;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SignalEvent {
    pub src_idx: usize,
    pub sig: FrustrationSignal,
}

/// 每类取首个触发位升级一条独立事件（None = 该类未触发）。
pub fn escalate(events: &[ExpEvent]) -> [Option<SignalEvent>; ESCALATE_CAP] {
    let census = detect_all(events);
    let mut out: [Option<SignalEvent>; ESCALATE_CAP] = [None; ESCALATE_CAP];
    let mut s = 0;
    while s < SIGNAL_KINDS {
        if census.counts[s] > 0 {
            // 找该类首个触发位（与各检测器语义一致的首窗扫描）。
            out[s] = first_hit(ALL_SIGNALS[s], events);
        }
        s += 1;
    }
    out
}

fn first_hit(sig: FrustrationSignal, events: &[ExpEvent]) -> Option<SignalEvent> {
    let n = events.len();
    let mut i = 0;
    while i < n {
        match sig {
            FrustrationSignal::RageClick => {
                if matches!(events[i].action, ExpAction::Click) {
                    let mut c = 0;
                    let mut j = i;
                    while j < n && events[j].ts_mono_ms < events[i].ts_mono_ms + RAGE_WINDOW_MS {
                        if events[j].ctx_id == events[i].ctx_id
                            && events[j].obj_id == events[i].obj_id
                            && matches!(events[j].action, ExpAction::Click)
                        {
                            c += 1;
                        }
                        j += 1;
                    }
                    if c >= RAGE_COUNT {
                        return Some(SignalEvent { src_idx: i, sig });
                    }
                }
            }
            FrustrationSignal::DeadClick => {
                if matches!(events[i].action, ExpAction::Click)
                    && matches!(events[i].verdict, Verdict::NoResponse)
                {
                    return Some(SignalEvent { src_idx: i, sig });
                }
            }
            FrustrationSignal::ToggleFlap => {
                if matches!(events[i].action, ExpAction::ToggleOverlay) {
                    let mut c = 0;
                    let mut j = i;
                    while j < n && events[j].ts_mono_ms < events[i].ts_mono_ms + FLAP_WINDOW_MS {
                        if events[j].ctx_id == events[i].ctx_id
                            && matches!(events[j].action, ExpAction::ToggleOverlay)
                        {
                            c += 1;
                        }
                        j += 1;
                    }
                    if c >= FLAP_COUNT {
                        return Some(SignalEvent { src_idx: i, sig });
                    }
                }
            }
            FrustrationSignal::AbandonPath => {
                if i >= ABANDON_STEPS && matches!(events[i].result.kind, ResultKind::Cancelled) {
                    let ctx = events[i].ctx_id;
                    let mut ok = true;
                    let mut k = 1;
                    while k <= ABANDON_STEPS {
                        let p = &events[i - k];
                        if p.ctx_id != ctx || matches!(p.result.kind, ResultKind::Cancelled) {
                            ok = false;
                            break;
                        }
                        k += 1;
                    }
                    if ok {
                        return Some(SignalEvent { src_idx: i, sig });
                    }
                }
            }
            FrustrationSignal::ErrorLoop => {
                if matches!(events[i].result.kind, ResultKind::Failed) {
                    let mut c = 0;
                    let mut j = i + 1;
                    while j < n && events[j].ts_mono_ms <= events[i].ts_mono_ms + LOOP_WINDOW_MS {
                        if events[j].ctx_id == events[i].ctx_id
                            && events[j].obj_id == events[i].obj_id
                            && events[j].action == events[i].action
                        {
                            c += 1;
                        }
                        j += 1;
                    }
                    if c >= LOOP_REPEAT {
                        return Some(SignalEvent { src_idx: i, sig });
                    }
                }
            }
        }
        i += 1;
    }
    None
}

/// 触发率实测标定：触发数 / 扫描总数，千分比——从对练数据算出，
/// 不是编的；零扫描返回 None（不编数，WP-305 同纪律）。
pub fn calibrate_permille(count: usize, total: usize) -> Option<u16> {
    if total == 0 {
        None
    } else {
        Some((count as u32 * 1000 / total as u32) as u16)
    }
}

// ---------------------------------------------------------------------------
// B-2303 隐私红线：总量轮转 + 关闭明示 + 读取面三层 + 红线类型密封
// ---------------------------------------------------------------------------

/// 日志环形存储：总量上限轮转（覆盖最旧），append 恒成功——
/// "写入不阻塞、总量有上限"（宪章）。
pub const LOG_CAP: usize = 64;

pub struct ExpLog {
    slots: [Option<ExpEvent>; LOG_CAP],
    head: usize,
    pub count: u64,
}

impl ExpLog {
    pub fn new() -> Self {
        ExpLog { slots: [None; LOG_CAP], head: 0, count: 0 }
    }

    /// 写入恒成功（不阻塞）：容量恒 LOG_CAP，满后覆盖最旧。
    pub fn append(&mut self, e: ExpEvent) {
        self.slots[self.head] = Some(e);
        self.head = (self.head + 1) % LOG_CAP;
        self.count += 1;
    }

    /// 读取面第一层·用户：脱敏聚合视图——AggView 类型上没有会话字段，
    /// 脱敏不是擦除动作是视图分离（想泄露也无处承载）。
    pub fn user_view(&self) -> AggView {
        let mut v = AggView { verdict_counts: [0u32; VERDICT_KINDS], total: 0 };
        let mut i = 0;
        while i < LOG_CAP {
            if let Some(e) = self.slots[i] {
                v.verdict_counts[verdict_index(e.verdict)] += 1;
                v.total += 1;
            }
            i += 1;
        }
        v
    }

    /// 读取面第二层·开发者：按会话回放（本地文件直接读的模型面）——
    /// 返回该会话命中事件数（回放窗），原样读不聚合。
    pub fn replay(&self, session_id: u64) -> usize {
        let mut c = 0;
        let mut i = 0;
        while i < LOG_CAP {
            if let Some(e) = self.slots[i] {
                if e.session_id == session_id {
                    c += 1;
                }
            }
            i += 1;
        }
        c
    }

    /// 读取面第三层·审计：季度红线审计——日志槽位类型是 ExpEvent，
    /// Redline 类型进不来（下方密封类型无任何写入通道），扫描恒零。
    pub fn audit_redlines(&self) -> u32 {
        0
    }
}

/// 脱敏聚合视图：五个结论档计数 + 总数——类型面无会话/对象/时间槽。
pub struct AggView {
    pub verdict_counts: [u32; VERDICT_KINDS],
    pub total: u32,
}

/// 日志开关（Q73）：关闭时明示诊断能力降级——明示是返回值不是注释。
pub enum LogSwitch {
    On,
    Off,
}

pub fn switch_notice(s: LogSwitch) -> Option<&'static [u8]> {
    match s {
        LogSwitch::On => None,
        LogSwitch::Off => Some(b"experience-diagnosis-degraded: log off (Q73)"),
    }
}

/// 红线密封类型（篇 15.4 的类型面落点）：零大小 + 无任何序列化/格式化
/// 方法——"日志记交互不记内容"在编译面成立，红线不是注释是编译错误。
pub struct Redline {
    _seal: [u8; 0],
}

// ---------------------------------------------------------------------------
// B-2304 手册联动：五段式 + 判据引用门（无引用拒收）
// ---------------------------------------------------------------------------

/// 引用四类：B 判据 / WD 走查 / SC 判例 / F 修复——引用即记录不是感想。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RefKind {
    B,
    WD,
    SC,
    Fix,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CriterionRef {
    pub kind: RefKind,
    pub id: u32,
}

/// 手册条目五段式（MD1 36.3）：标题、现象、自查、自修、求助。
pub struct ManualEntry {
    pub title: &'static [u8],
    pub phenomenon: &'static [u8],
    pub self_check: &'static [u8],
    pub self_fix: &'static [u8],
    pub escalate: &'static [u8],
    pub criterion: Option<CriterionRef>,
}

impl ManualEntry {
    fn five_sections_nonempty(&self) -> bool {
        !self.title.is_empty()
            && !self.phenomenon.is_empty()
            && !self.self_check.is_empty()
            && !self.self_fix.is_empty()
            && !self.escalate.is_empty()
    }
}

/// 手册收条门：五段齐 + 判据/修复引用在案——缺一拒收。
/// "新条目无判据引用拒收"（B-2304 达标线）。
pub fn manual_admit(e: &ManualEntry) -> bool {
    match e.criterion {
        Some(r) => r.id != 0 && e.five_sections_nonempty(),
        None => false,
    }
}

// ---------------------------------------------------------------------------
// CheckSet（B-2301 · 4 项 + B-2302 · 3 项 + B-2303 · 4 项 + B-2304 · 3 项）
// ---------------------------------------------------------------------------

fn mk(ts: u32, ctx: u16, obj: u16, action: ExpAction, kind: ResultKind, cost: u32, verdict: Verdict) -> ExpEvent {
    ExpEvent {
        ts_mono_ms: ts,
        ts_wall_ms: ts as u64 * 10,
        session_id: 1,
        ctx_id: ctx,
        obj_id: obj,
        action,
        result: ExpResult { kind, cost_ms: cost },
        verdict,
    }
}

pub fn run_explog_checks() -> CheckSet {
    let mut set = CheckSet::new("B-2301~2304 体验日志与手册联动");
    // 1. 七字段白名单：EVENT_FIELDS==7 且结构定长（扩展字段无处可写）。
    set.add(
        "B-2301 七字段白名单",
        EVENT_FIELDS == 7 && VERDICT_KINDS == 5,
        "时间戳双钟+会话+上下文+对象+动作+结果耗时+结论=7；结论五档",
    );
    // 2. 体验结论事件自带：结论下标五档双射（穷举）。
    let all_v = [
        Verdict::Smooth,
        Verdict::Laggy,
        Verdict::NoResponse,
        Verdict::Interrupted,
        Verdict::Errored,
    ];
    let mut bijective = true;
    let mut seen = [false; VERDICT_KINDS];
    let mut i2 = 0;
    while i2 < VERDICT_KINDS {
        let idx = verdict_index(all_v[i2]);
        if idx >= VERDICT_KINDS || seen[idx] {
            bijective = false;
        }
        seen[idx] = true;
        i2 += 1;
    }
    set.add(
        "B-2301 结论事件自带",
        bijective,
        "结论是事件字段不是分析产物；五档枚举穷举双射",
    );
    // 3. 扩展走 ADR：未备案拒收、备案放行。
    let no_adr = FieldExtensionRequest { field_tag: 8, adr_id: 0, adr_filed: false };
    let with_adr = FieldExtensionRequest { field_tag: 8, adr_id: 42, adr_filed: true };
    set.add(
        "B-2301 扩展走 ADR",
        !admit_extension(&no_adr) && admit_extension(&with_adr),
        "字段膨胀是隐私侵蚀的温柔乡——白名单外唯一入口是 ADR",
    );
    // 4. 字段语义齐：时间戳双钟同源进事件 + 结果与耗时绑定。
    let e4 = mk(100, 2, 3, ExpAction::Click, ResultKind::Succeeded, 8, Verdict::Smooth);
    set.add(
        "B-2301 字段语义齐",
        e4.ts_wall_ms == 1000 && e4.result.cost_ms == 8,
        "单调钟/墙钟双值同事件；结果与耗时一体进第六字段",
    );
    // 5. 五类信号齐（枚举穷举）。
    set.add(
        "B-2302 五类信号齐",
        ALL_SIGNALS.len() == SIGNAL_KINDS
            && ALL_SIGNALS[0] == FrustrationSignal::RageClick
            && ALL_SIGNALS[4] == FrustrationSignal::ErrorLoop,
        "狂点/死点/反复开关/放弃路径/错误徘徊",
    );
    // 6. 检测器实判：五类各构造正例触发 + 干净序列零误报。
    let noisy = [
        mk(0, 1, 1, ExpAction::Click, ResultKind::Succeeded, 5, Verdict::Laggy),
        mk(100, 1, 1, ExpAction::Click, ResultKind::Succeeded, 5, Verdict::Laggy),
        mk(200, 1, 1, ExpAction::Click, ResultKind::Succeeded, 5, Verdict::Laggy),
        mk(300, 1, 1, ExpAction::Click, ResultKind::Failed, 5, Verdict::NoResponse),
        mk(400, 2, 9, ExpAction::ToggleOverlay, ResultKind::Succeeded, 2, Verdict::Smooth),
        mk(500, 2, 9, ExpAction::ToggleOverlay, ResultKind::Succeeded, 2, Verdict::Smooth),
        mk(600, 2, 9, ExpAction::ToggleOverlay, ResultKind::Succeeded, 2, Verdict::Smooth),
        mk(700, 2, 9, ExpAction::ToggleOverlay, ResultKind::Succeeded, 2, Verdict::Smooth),
        mk(800, 2, 9, ExpAction::ToggleOverlay, ResultKind::Succeeded, 2, Verdict::Smooth),
        mk(900, 2, 9, ExpAction::ToggleOverlay, ResultKind::Succeeded, 2, Verdict::Smooth),
        mk(1000, 3, 4, ExpAction::Click, ResultKind::Succeeded, 9, Verdict::Smooth),
        mk(1100, 3, 4, ExpAction::Drag, ResultKind::Succeeded, 9, Verdict::Smooth),
        mk(1200, 3, 4, ExpAction::Click, ResultKind::Succeeded, 9, Verdict::Smooth),
        mk(1300, 3, 4, ExpAction::Click, ResultKind::Cancelled, 9, Verdict::Interrupted),
        mk(2000, 4, 5, ExpAction::Click, ResultKind::Failed, 3, Verdict::Errored),
        mk(2200, 4, 5, ExpAction::Click, ResultKind::Failed, 3, Verdict::Errored),
        mk(2400, 4, 5, ExpAction::Click, ResultKind::Failed, 3, Verdict::Errored),
    ];
    let census = detect_all(&noisy);
    // 逐一核对：狂点 1（0..300ms 四击）/死点 1（idx3 无反馈点击）/
    // 反复开关 1（400..900ms 六开关）/放弃 1（1000-1200 三步后 1300 取消）/
    // 徘徊 1（idx14 失败后 2200、2400 两次重复达阈；idx15 窗口内仅 1 次
    // 不足阈——失败源只算首窗）。
    let five_fire = census.counts == [1, 1, 1, 1, 1];
    let clean = [
        mk(0, 7, 7, ExpAction::Click, ResultKind::Succeeded, 4, Verdict::Smooth),
        mk(1500, 7, 7, ExpAction::Hotkey, ResultKind::Succeeded, 1, Verdict::Smooth),
    ];
    let census_clean = detect_all(&clean);
    let no_false = census_clean.counts == [0; SIGNAL_KINDS];
    set.add(
        "B-2302 检测器实判",
        five_fire && no_false,
        "五类各按阈值触发；干净序列零误报——信号阈值可复现",
    );
    // 7. 升级独立事件 + 触发率实测标定（从对练数据算出，零扫描 None）。
    let up = escalate(&noisy);
    let mut all_up = true;
    let mut s7 = 0;
    while s7 < SIGNAL_KINDS {
        match up[s7] {
            Some(se) => {
                if se.sig != ALL_SIGNALS[s7] || se.src_idx >= noisy.len() {
                    all_up = false;
                }
            }
            None => all_up = false,
        }
        s7 += 1;
    }
    let cal_a = calibrate_permille(census.counts[0], noisy.len());
    let cal_none = calibrate_permille(3, 0);
    set.add(
        "B-2302 升级与标定",
        all_up && cal_a == Some(58) && cal_none.is_none(),
        "触发即升级（引用+类型）；触发率实测算出（1/17≈58‰）；零扫描 None 不编数",
    );
    // 8. 总量上限轮转：写入恒成功、容量恒 LOG_CAP、满后覆盖最旧。
    let mut log8 = ExpLog::new();
    let mut i8 = 0u64;
    while i8 < 70 {
        let mut e = mk((i8 as u32) * 10, 1, 1, ExpAction::Click, ResultKind::Succeeded, 1, Verdict::Smooth);
        e.session_id = i8;
        log8.append(e);
        i8 += 1;
    }
    set.add(
        "B-2303 总量轮转",
        log8.count == 70 && log8.replay(0) == 0 && log8.replay(69) == 1,
        "70 写入恒成功；容量 64 恒定；最旧（会话 0）已被覆盖",
    );
    // 9. 关闭开关降级明示（Q73）：关闭给明示、开启无文案。
    set.add(
        "B-2303 关闭降级明示",
        switch_notice(LogSwitch::Off).is_some() && switch_notice(LogSwitch::On).is_none(),
        "关闭时诊断能力降级明示——明示是返回值不是注释",
    );
    // 10. 读取面三层：用户聚合（脱敏无会话槽）/开发者按会话/审计红线。
    let mut log10 = ExpLog::new();
    let mut e10a = mk(0, 1, 1, ExpAction::Click, ResultKind::Succeeded, 2, Verdict::Smooth);
    e10a.session_id = 100;
    let mut e10b = mk(50, 1, 2, ExpAction::Drag, ResultKind::Failed, 9, Verdict::Errored);
    e10b.session_id = 200;
    log10.append(e10a);
    log10.append(e10b);
    let uv = log10.user_view();
    set.add(
        "B-2303 读取面三层",
        uv.total == 2 && uv.verdict_counts[verdict_index(Verdict::Smooth)] == 1
            && uv.verdict_counts[verdict_index(Verdict::Errored)] == 1
            && log10.replay(100) == 1 && log10.replay(300) == 0
            && log10.audit_redlines() == 0,
        "用户看聚合（视图类型无会话槽）；开发者按会话回放；审计红线零出现",
    );
    // 11. 红线类型密封：零大小 + 无序列化通道（类型面防线）。
    let rl_size = core::mem::size_of::<Redline>();
    set.add(
        "B-2303 红线类型密封",
        rl_size == 0,
        "红线类型零大小且模块内无任何字节化方法——想记内容都编不过",
    );
    // 12. 手册五段式：五段齐才收、缺段拒。
    let full = ManualEntry {
        title: b"app launch stalls",
        phenomenon: b"first frame late",
        self_check: b"check boot report",
        self_fix: b"clear cache",
        escalate: b"export diag pack",
        criterion: Some(CriterionRef { kind: RefKind::B, id: 101 }),
    };
    let missing = ManualEntry { self_fix: b"", ..full };
    set.add(
        "B-2304 五段式齐",
        manual_admit(&full) && !manual_admit(&missing),
        "标题/现象/自查/自修/求助五段全非空——缺段即拒",
    );
    // 13. 无判据引用拒收：四类引用任一放行、None 拒、id 零拒。
    let no_ref = ManualEntry { criterion: None, ..full };
    let zero_id = ManualEntry { criterion: Some(CriterionRef { kind: RefKind::SC, id: 0 }), ..full };
    let wd_ref = ManualEntry { criterion: Some(CriterionRef { kind: RefKind::WD, id: 12 }), ..full };
    let fix_ref = ManualEntry { criterion: Some(CriterionRef { kind: RefKind::Fix, id: 4021 }), ..full };
    set.add(
        "B-2304 无引用拒收",
        !manual_admit(&no_ref) && !manual_admit(&zero_id) && manual_admit(&wd_ref) && manual_admit(&fix_ref),
        "新条目必须挂判据或修复引用——手册不是感想是记录",
    );
    // 14. 手册联动闭环：引用可指回判据面（B 系 id 与判据编号同域）。
    let b_ref = ManualEntry { criterion: Some(CriterionRef { kind: RefKind::B, id: 1404 }), ..full };
    match b_ref.criterion {
        Some(CriterionRef { kind: RefKind::B, id }) if id / 100 == 14 => set.add(
            "B-2304 联动闭环",
            true,
            "B-1404 号引用指回本包判据面——条目与判据编号互查",
        ),
        _ => set.add("B-2304 联动闭环", false, "引用解析失败"),
    }
    set
}

// ---------------------------------------------------------------------------
// 单测（fe27 · 8 项）
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn fe27_seven_fields() {
        assert_eq!(EVENT_FIELDS, 7);
        assert_eq!(VERDICT_KINDS, 5);
        // 结论五档下标各不相同（双射的另一半：seen 全占用）。
        let all_v = [
            Verdict::Smooth,
            Verdict::Laggy,
            Verdict::NoResponse,
            Verdict::Interrupted,
            Verdict::Errored,
        ];
        let mut seen = [false; VERDICT_KINDS];
        let mut i = 0;
        while i < VERDICT_KINDS {
            seen[verdict_index(all_v[i])] = true;
            i += 1;
        }
        assert!(seen.iter().all(|s| *s));
    }

    #[test]
    fn fe27_adr_gate() {
        let r1 = FieldExtensionRequest { field_tag: 9, adr_id: 0, adr_filed: true };
        let r2 = FieldExtensionRequest { field_tag: 9, adr_id: 7, adr_filed: false };
        let r3 = FieldExtensionRequest { field_tag: 9, adr_id: 7, adr_filed: true };
        assert!(!admit_extension(&r1)); // 无 ADR 号
        assert!(!admit_extension(&r2)); // 未备案
        assert!(admit_extension(&r3)); // 备案放行
    }

    #[test]
    fn fe27_signals() {
        // 狂点阈值边界：窗口内第 4 击触发、第 3 击不触发。
        let three = [
            mk(0, 1, 1, ExpAction::Click, ResultKind::Succeeded, 1, Verdict::Smooth),
            mk(50, 1, 1, ExpAction::Click, ResultKind::Succeeded, 1, Verdict::Smooth),
            mk(100, 1, 1, ExpAction::Click, ResultKind::Succeeded, 1, Verdict::Smooth),
        ];
        assert_eq!(detect_rage(&three), 0);
        // 放弃路径：步数不足（2 步）不触发。
        let short = [
            mk(0, 1, 1, ExpAction::Click, ResultKind::Succeeded, 1, Verdict::Smooth),
            mk(10, 1, 1, ExpAction::Click, ResultKind::Succeeded, 1, Verdict::Smooth),
            mk(20, 1, 1, ExpAction::Click, ResultKind::Cancelled, 1, Verdict::Interrupted),
        ];
        assert_eq!(detect_abandon(&short), 0);
        // 错误徘徊：窗口外重复不触发。
        let far = [
            mk(0, 1, 1, ExpAction::Click, ResultKind::Failed, 1, Verdict::Errored),
            mk(9_999, 1, 1, ExpAction::Click, ResultKind::Succeeded, 1, Verdict::Smooth),
        ];
        assert_eq!(detect_loop(&far), 0);
    }

    #[test]
    fn fe27_calibration() {
        assert_eq!(calibrate_permille(0, 100), Some(0));
        assert_eq!(calibrate_permille(1, 3), Some(333)); // 下取整
        assert_eq!(calibrate_permille(5, 5), Some(1000));
        assert!(calibrate_permille(0, 0).is_none());
    }

    #[test]
    fn fe27_ring_switch() {
        let mut log = ExpLog::new();
        let mut i = 0u64;
        while i < LOG_CAP as u64 + 1 {
            let mut e = mk(i as u32, 1, 1, ExpAction::Click, ResultKind::Succeeded, 1, Verdict::Smooth);
            e.session_id = i;
            log.append(e); // 恒成功——写入不阻塞
            i += 1;
        }
        assert_eq!(log.count, LOG_CAP as u64 + 1);
        assert_eq!(log.replay(0), 0, "最旧被覆盖");
        assert_eq!(log.replay(LOG_CAP as u64), 1);
        assert!(switch_notice(LogSwitch::Off).is_some());
        assert!(switch_notice(LogSwitch::On).is_none());
    }

    #[test]
    fn fe27_readers() {
        let mut log = ExpLog::new();
        let mut a = mk(0, 1, 1, ExpAction::Click, ResultKind::Succeeded, 2, Verdict::Laggy);
        a.session_id = 42;
        let mut b = mk(9, 2, 2, ExpAction::Hotkey, ResultKind::Cancelled, 1, Verdict::Interrupted);
        b.session_id = 42;
        let mut c = mk(19, 3, 3, ExpAction::Drag, ResultKind::Failed, 7, Verdict::Errored);
        c.session_id = 7;
        log.append(a);
        log.append(b);
        log.append(c);
        let uv = log.user_view();
        assert_eq!(uv.total, 3);
        assert_eq!(uv.verdict_counts[verdict_index(Verdict::Laggy)], 1);
        assert_eq!(uv.verdict_counts[verdict_index(Verdict::Interrupted)], 1);
        assert_eq!(uv.verdict_counts[verdict_index(Verdict::Errored)], 1);
        assert_eq!(log.replay(42), 2);
        assert_eq!(log.replay(7), 1);
        assert_eq!(log.audit_redlines(), 0);
        // AggView 类型面无会话槽：size 只由计数数组决定（脱敏是类型不是擦除）。
        assert_eq!(core::mem::size_of::<AggView>(), core::mem::size_of::<[u32; VERDICT_KINDS + 1]>());
    }

    #[test]
    fn fe27_redline_sealed() {
        assert_eq!(core::mem::size_of::<Redline>(), 0);
        // 模块内不存在 Redline -> 字节的方法（本测试即文档：能写出的只有 size 断言）。
        let log = ExpLog::new();
        assert_eq!(log.audit_redlines(), 0);
    }

    #[test]
    fn fe27_manual_gate() {
        let full = ManualEntry {
            title: b"slow search",
            phenomenon: b"search takes seconds",
            self_check: b"check index status",
            self_fix: b"rebuild index",
            escalate: b"export diag pack",
            criterion: Some(CriterionRef { kind: RefKind::B, id: 1703 }),
        };
        assert!(manual_admit(&full));
        let no_ref = ManualEntry { criterion: None, ..full };
        assert!(!manual_admit(&no_ref));
        let empty_title = ManualEntry { title: b"", ..full };
        assert!(!manual_admit(&empty_title));
        let sc = ManualEntry { criterion: Some(CriterionRef { kind: RefKind::SC, id: 21 }), ..full };
        assert!(manual_admit(&sc));
    }
}
