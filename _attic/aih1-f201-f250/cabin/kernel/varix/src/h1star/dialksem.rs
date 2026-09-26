//! F207 对话框 Esc/Enter 语义统一 · 判据实装（H 基础通用域 · AI-H1 分工包）。
//!
//! **判据锚**：主册 F207「对话框 Esc/Enter 语义统一」。
//!
//! **验收标准（主册第一句）**：全系统对话框清单扫描，默认按钮/焦点位置
//! 逐个标注入册；删除类对话框 Esc 行为 10 例全为取消；Enter 双触发
//! （文本提交+关框竞态）专项测试 0 竞态。
//!
//! **设计要点**：
//! - [`DialogKind`] 四分类：普通 / 危险 / 含文本输入 / 无按钮浮层——
//!   一切按键语义都由 kind 决定（规则表唯一出口 [`route`]）；
//! - Esc 语义：一切对话框 Esc 恒为取消（危险框 Esc **绝不等于确认**——
//!   删除确认框按 Esc 永远是不删）；无按钮浮层 Esc 仅关闭、无副作用；
//! - Enter 语义：无按钮浮层不动作；含文本输入且草稿脏 → **先提交文本
//!   再确认关框**（同一次按键按序两个动作）；否则确认焦点按钮
//!   （未落焦回退默认按钮）；
//! - 危险对话框登记制：`default_btn` 与 `focus_btn` 登记时**强制落在
//!   取消**（偏离即纠正并入账 `coerced_focus`）——Enter 默认路径天然
//!   安全；
//! - Enter 双触发竞态消除：单耗队列（[`KeyRouter`]）——一次按压
//!   `press_id` 只消费一次，重复派发直接拒绝并入账；
//! - [`DialogRuntime`] 会话态：打开/焦点循环/草稿/按键应用/账本
//!   （`RingLog` 64 条决策入账，`render_route` 出可读文案）。
//!
//! **依赖锚点**：`crate::checks::CheckSet`（自检面）、
//! `crate::checks::{push_str, push_usize}`（文案渲染）、
//! `crate::star::sbase::RingLog`（决策账本）。
//! 时间纪律：一切时间由调用方注入毫秒戳，模块不持时钟。

use crate::checks::{push_str, CheckSet};
use crate::star::sbase::RingLog;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 登记册容量（全系统对话框清单上限，实装定值；超出显性拒绝）。
pub const REG_CAP: usize = 128;

/// 单耗队列容量（最近 32 次 press_id；Enter 双触发竞态消除的防线）。
pub const PRESS_QUEUE_CAP: usize = 32;

/// 决策账本容量。
pub const LEDGER_CAP: usize = 64;

/// 高危险级阈值（danger_level ≥ 1 计入高危清单）。
pub const DANGER_LEVEL_GUARD: u8 = 1;

// ---------------------------------------------------------------------------
// 分类与语义枚举
// ---------------------------------------------------------------------------

/// 对话框四分类（主册 F207；一切语义由 kind 决定）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogKind {
    /// 普通对话框（确认/取消）。
    Normal,
    /// 危险操作对话框（删除等——Esc 恒为不执行）。
    Dangerous,
    /// 含文本输入（Enter 先提交文本再关框）。
    TextEntry,
    /// 无按钮浮层（Esc 仅关闭、Enter 无动作）。
    NoButton,
}

/// 按钮。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Btn {
    Ok,
    Cancel,
    /// 自定义按钮（编号）。
    Custom(u8),
    /// 无（未落焦/浮层）。
    None,
}

/// 按键。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Enter,
    Esc,
}

/// 动作语义（渲染层把语义映射到具体按钮）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    /// 无动作。
    None,
    /// 确认（默认/焦点按钮的确认语义）。
    Ok,
    /// 取消。
    Cancel,
    /// 提交文本（TextEntry 草稿落账）。
    CommitText,
    /// 仅关闭（无副作用）。
    Close,
}

/// 一次派发的路由结果：按序动作表（n ≤ 2）+ 按压 id。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Route {
    pub acts: [Act; 2],
    pub n: u8,
    pub press: u32,
}

impl Route {
    /// 主动作（n == 0 → None）。
    pub fn primary(&self) -> Act {
        if self.n >= 1 {
            self.acts[0]
        } else {
            Act::None
        }
    }

    /// 次动作（先提交文本再关框的第二步）。
    pub fn secondary(&self) -> Act {
        if self.n >= 2 {
            self.acts[1]
        } else {
            Act::None
        }
    }

    pub fn empty(press: u32) -> Route {
        Route { acts: [Act::None; 2], n: 0, press }
    }
}

/// 对话框登记规格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DialogSpec {
    pub id: u16,
    pub kind: DialogKind,
    pub default_btn: Btn,
    pub focus_btn: Btn,
    /// 危险级（≥ DANGER_LEVEL_GUARD 计入高危清单）。
    pub danger_level: u8,
}

/// 按钮集（规则表的一部分：Tab 循环面）。
pub fn buttons(kind: DialogKind) -> &'static [Btn] {
    match kind {
        DialogKind::Normal | DialogKind::Dangerous | DialogKind::TextEntry => {
            &[Btn::Ok, Btn::Cancel]
        }
        DialogKind::NoButton => &[],
    }
}

/// 焦点循环：按钮间 Tab 一跳（未落焦从首个开始）。
pub fn next_button(kind: DialogKind, cur: Btn) -> Btn {
    let list = buttons(kind);
    if list.is_empty() {
        return Btn::None;
    }
    let idx = list.iter().position(|&b| b == cur).unwrap_or(0);
    list[(idx + 1) % list.len()]
}

/// 登记缺陷判定（register 与 audit 共用的唯一规则）。
pub fn spec_defect(spec: &DialogSpec) -> Option<&'static str> {
    match spec.kind {
        DialogKind::NoButton if spec.default_btn != Btn::None || spec.focus_btn != Btn::None => {
            Some("no-button overlay must declare no buttons")
        }
        DialogKind::Normal | DialogKind::TextEntry if spec.default_btn == Btn::None => {
            Some("normal/text dialog missing default button")
        }
        DialogKind::Dangerous if spec.focus_btn != Btn::Cancel => {
            Some("dangerous dialog focus must be cancel")
        }
        DialogKind::Dangerous if spec.default_btn != Btn::Cancel => {
            Some("dangerous dialog default must be cancel")
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// 规则表（Esc/Enter 分派唯一出口）
// ---------------------------------------------------------------------------

/// Esc 语义：一切对话框恒为取消；无按钮浮层仅关闭（无副作用）。
/// 危险框 Esc **绝不等于确认**——本函数是唯一出口，无第二路径。
pub fn esc_action(kind: DialogKind) -> Act {
    match kind {
        DialogKind::NoButton => Act::Close,
        _ => Act::Cancel,
    }
}

/// Enter 确认语义落点：焦点按钮优先，未落焦回退默认按钮。
fn confirm_act(spec: &DialogSpec, focus: Btn) -> Act {
    let btn = match focus {
        Btn::None => spec.default_btn,
        b => b,
    };
    match btn {
        Btn::Cancel => Act::Cancel,
        Btn::None => Act::None,
        _ => Act::Ok,
    }
}

/// 纯路由（无状态）：按键 × 分类 × 焦点 × 草稿脏 → 按序动作表。
pub fn route(spec: &DialogSpec, focus: Btn, key: Key, text_dirty: bool) -> Route {
    let mut acts = [Act::None; 2];
    let n: u8 = match key {
        Key::Esc => {
            acts[0] = esc_action(spec.kind);
            1
        }
        Key::Enter => match spec.kind {
            DialogKind::NoButton => 0,
            DialogKind::TextEntry if text_dirty => {
                // 先提交文本，再确认关框（同一次按键、按序两步）。
                acts[0] = Act::CommitText;
                acts[1] = confirm_act(spec, focus);
                2
            }
            _ => {
                acts[0] = confirm_act(spec, focus);
                1
            }
        },
    };
    Route { acts, n, press: 0 }
}

// ---------------------------------------------------------------------------
// 单耗队列（Enter 双触发竞态消除）
// ---------------------------------------------------------------------------

/// 键序路由器：单耗队列——同一 press_id 只消费一次。
pub struct KeyRouter {
    fired: RingLog<u32, PRESS_QUEUE_CAP>,
    /// 被拦下的重复派发数（0 竞态判据的反面计数，恒守恒）。
    pub double_fire_blocked: u32,
    pub routed: u32,
}

impl KeyRouter {
    pub fn new() -> KeyRouter {
        KeyRouter { fired: RingLog::new(), double_fire_blocked: 0, routed: 0 }
    }

    /// 派发一次按压。重复 press_id → 空 Route（竞态被消除）。
    pub fn dispatch(&mut self, spec: &DialogSpec, focus: Btn, key: Key, text_dirty: bool, press_id: u32) -> Route {
        self.routed += 1;
        if self.fired.newest_first().contains(&press_id) {
            self.double_fire_blocked += 1;
            return Route::empty(press_id);
        }
        let mut r = route(spec, focus, key, text_dirty);
        r.press = press_id;
        if r.n > 0 {
            self.fired.push(press_id);
        }
        r
    }

    /// 专项测试口径：全历史双触发拦截数（恒 = 重复派发次数）。
    pub fn blocked(&self) -> u32 {
        self.double_fire_blocked
    }
}

impl Default for KeyRouter {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 登记册（清单扫描接口）
// ---------------------------------------------------------------------------

/// 对话框登记册：每框登记 kind/默认按钮/焦点位/危险级。
pub struct DialogRegistry {
    specs: alloc::vec::Vec<DialogSpec>,
    /// 危险框焦点被强制到取消的次数（登记制纠正入账）。
    pub coerced_focus: u32,
    /// 超容量拒绝数。
    pub rejected: u32,
    /// 仍存违规格数（register 时不纠正的部分，如 NoButton 声明了按钮）。
    pub violations: u32,
}

impl DialogRegistry {
    pub fn new() -> DialogRegistry {
        DialogRegistry { specs: alloc::vec::Vec::new(), coerced_focus: 0, rejected: 0, violations: 0 }
    }

    /// 登记（同 id 覆盖）。危险框 default/focus 强制取消并计数。
    pub fn register(&mut self, spec: DialogSpec) {
        if self.specs.len() >= REG_CAP {
            self.rejected += 1;
            return;
        }
        let mut s = spec;
        if s.kind == DialogKind::Dangerous {
            if s.focus_btn != Btn::Cancel {
                s.focus_btn = Btn::Cancel;
                self.coerced_focus += 1;
            }
            if s.default_btn != Btn::Cancel {
                s.default_btn = Btn::Cancel;
                self.coerced_focus += 1;
            }
        }
        if spec_defect(&s).is_some() {
            self.violations += 1;
        }
        match self.specs.iter_mut().find(|x| x.id == s.id) {
            Some(slot) => *slot = s,
            None => self.specs.push(s),
        }
    }

    pub fn spec(&self, id: u16) -> Option<DialogSpec> {
        self.specs.iter().copied().find(|s| s.id == id)
    }

    /// 清单扫描：全部登记框 id 写入 `out`（验收「清单扫描逐个标注入册」）。
    pub fn scan_ids(&self, out: &mut [u16]) -> usize {
        let n = self.specs.len().min(out.len());
        for (i, s) in self.specs.iter().take(n).enumerate() {
            out[i] = s.id;
        }
        n
    }

    /// 审计现存登记的违规格数（登记被纠正后应为 0）。
    pub fn audit(&self) -> usize {
        self.specs.iter().filter(|s| spec_defect(s).is_some()).count()
    }

    pub fn len(&self) -> usize {
        self.specs.len()
    }

    /// 高危对话框数（danger_level ≥ 1）。
    pub fn dangerous_count(&self) -> usize {
        self.specs.iter().filter(|s| s.danger_level >= DANGER_LEVEL_GUARD).count()
    }
}

impl Default for DialogRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 文本草稿（TextEntry 提交态）
// ---------------------------------------------------------------------------

/// 文本草稿：脏标记 + 提交计数（CommitText 的落账面）。
#[derive(Clone, Copy, Debug, Default)]
pub struct Draft {
    dirty: bool,
    commits: u32,
}

impl Draft {
    pub fn new() -> Draft {
        Draft { dirty: false, commits: 0 }
    }

    /// 用户敲入文本 → 草稿变脏。
    pub fn type_text(&mut self) {
        self.dirty = true;
    }

    /// 提交（CommitText 应用点）：清脏并计数。
    pub fn commit(&mut self) {
        if self.dirty {
            self.dirty = false;
            self.commits += 1;
        }
    }

    pub fn dirty(&self) -> bool {
        self.dirty
    }

    pub fn commits(&self) -> u32 {
        self.commits
    }
}

// ---------------------------------------------------------------------------
// 会话运行时
// ---------------------------------------------------------------------------

/// 决策账本条目。
#[derive(Clone, Copy, Debug)]
pub struct RouteRec {
    pub press: u32,
    pub spec: u16,
    pub key: Key,
    pub primary: Act,
    pub ts: u64,
}

/// 单个打开的对话框会话。
pub struct DialogSession {
    pub spec: DialogSpec,
    pub focus: Btn,
    pub draft: Draft,
    pub opened_ms: u64,
    pub tab_count: u32,
}

/// 对话框运行时：登记册 + 路由器 + 当前会话 + 决策账本。
pub struct DialogRuntime {
    reg: DialogRegistry,
    router: KeyRouter,
    session: Option<DialogSession>,
    ledger: RingLog<RouteRec, LEDGER_CAP>,
    pub opened: u32,
    pub confirmed: u32,
    pub cancelled: u32,
    pub closed: u32,
    /// 累计文本提交数（运行时级账面——缺一笔则「Enter 先提交文本再关框」
    /// 在会话关闭后不可审计；与 confirmed/cancelled/closed 同级口径）。
    pub committed: u32,
}

impl DialogRuntime {
    pub fn new() -> DialogRuntime {
        DialogRuntime {
            reg: DialogRegistry::new(),
            router: KeyRouter::new(),
            session: None,
            ledger: RingLog::new(),
            opened: 0,
            confirmed: 0,
            cancelled: 0,
            closed: 0,
            committed: 0,
        }
    }

    pub fn registry(&self) -> &DialogRegistry {
        &self.reg
    }

    pub fn registry_mut(&mut self) -> &mut DialogRegistry {
        &mut self.reg
    }

    /// 打开对话框：焦点落登记的 focus_btn（危险框 = 取消）。
    pub fn open(&mut self, spec_id: u16, now: u64) -> bool {
        if self.session.is_some() {
            return false; // 单模态：一次只开一个对话框
        }
        let spec = match self.reg.spec(spec_id) {
            Some(s) => s,
            None => return false,
        };
        self.session = Some(DialogSession {
            spec,
            focus: spec.focus_btn,
            draft: Draft::new(),
            opened_ms: now,
            tab_count: 0,
        });
        self.opened += 1;
        true
    }

    pub fn is_open(&self) -> bool {
        self.session.is_some()
    }

    pub fn session_focus(&self) -> Option<Btn> {
        self.session.as_ref().map(|s| s.focus)
    }

    pub fn session_spec(&self) -> Option<u16> {
        self.session.as_ref().map(|s| s.spec.id)
    }

    /// Tab：焦点在按钮集内循环。
    pub fn tab(&mut self) -> Option<Btn> {
        let s = self.session.as_mut()?;
        s.focus = next_button(s.spec.kind, s.focus);
        s.tab_count += 1;
        Some(s.focus)
    }

    /// 敲入文本（TextEntry 草稿变脏）。
    pub fn type_text(&mut self) -> bool {
        match self.session.as_mut() {
            Some(s) => {
                s.draft.type_text();
                true
            }
            None => false,
        }
    }

    /// 累计文本提交数（运行时级账面；会话关闭后仍可读——审计口径与
    /// confirmed/cancelled/closed 同级，一处一事实）。
    pub fn draft_commits(&self) -> u32 {
        self.committed
    }

    /// 按键派发：路由 + 会话侧效果应用 + 决策入账。
    pub fn press(&mut self, key: Key, press_id: u32, now: u64) -> Route {
        let dirty = self.session.as_ref().map_or(false, |s| s.draft.dirty());
        let (spec, focus) = match self.session.as_ref() {
            Some(s) => (s.spec, s.focus),
            None => return Route::empty(press_id),
        };
        let r = self.router.dispatch(&spec, focus, key, dirty, press_id);
        self.ledger.push(RouteRec { press: press_id, spec: spec.id, key, primary: r.primary(), ts: now });
        // 按序应用全部动作——「先提交文本再关框」在同一次按键内完成
        // （单耗队列保证同一 press_id 不会二次应用，0 竞态）。
        for k in 0..r.n as usize {
            match r.acts[k] {
                Act::Ok => {
                    self.confirmed += 1;
                    self.session = None;
                }
                Act::Cancel => {
                    self.cancelled += 1;
                    self.session = None;
                }
                Act::Close => {
                    self.closed += 1;
                    self.session = None;
                }
                Act::CommitText => {
                    // 缺陷账本：现象=「单次 Enter 提交+关框」后 draft_commits()
                    // 恒 0、检查项与单元测试双红；根因=提交计数只挂在随会话
                    // 销毁的 Draft 上（会话一关即丢账），而 confirmed 等
                    // 同类计数都是运行时级——实现丢账违反自身审计口径；
                    // 修法=CommitText 落账时同步累加运行时级 committed，
                    // draft_commits() 改读运行时账面（关框后仍可审计）。
                    if let Some(s) = self.session.as_mut() {
                        s.draft.commit();
                        self.committed += 1;
                    }
                }
                Act::None => {}
            }
            if self.session.is_none() {
                break;
            }
        }
        r
    }

    pub fn route_ledger(&self) -> &RingLog<RouteRec, LEDGER_CAP> {
        &self.ledger
    }

    pub fn blocked_count(&self) -> u32 {
        self.router.blocked()
    }
}

impl Default for DialogRuntime {
    fn default() -> Self {
        Self::new()
    }
}

/// 路由主动作渲染为可读文案（决策账本出账用）。
pub fn render_route(r: &Route, out: &mut [u8]) -> usize {
    let mut n = 0usize;
    match r.primary() {
        Act::Ok => push_str(out, &mut n, "确认"),
        Act::Cancel => push_str(out, &mut n, "取消"),
        Act::CommitText => push_str(out, &mut n, "提交文本"),
        Act::Close => push_str(out, &mut n, "关闭"),
        Act::None => push_str(out, &mut n, "无动作"),
    }
    n
}

// ---------------------------------------------------------------------------
// 审计报告 / 按钮语义辅助 / 焦点逆序 / 双步文案
// ---------------------------------------------------------------------------

/// 登记册审计报告快照（清单扫描出账面；验收「清单扫描逐个标注入册」）。
#[derive(Clone, Copy, Debug)]
pub struct RegistryAuditReport {
    pub total: usize,
    pub dangerous: usize,
    pub high_danger: usize,
    pub violations: usize,
    pub coerced_focus: u32,
    pub rejected: u32,
}

impl DialogRegistry {
    pub fn audit_report(&self) -> RegistryAuditReport {
        RegistryAuditReport {
            total: self.len(),
            dangerous: self.specs.iter().filter(|s| s.kind == DialogKind::Dangerous).count(),
            high_danger: self.dangerous_count(),
            violations: self.audit(),
            coerced_focus: self.coerced_focus,
            rejected: self.rejected,
        }
    }
}

impl Btn {
    /// 确认语义按钮（Ok/自定义）——Enter 落点合法性判定用。
    pub fn is_confirm(&self) -> bool {
        matches!(self, Btn::Ok | Btn::Custom(_))
    }

    pub fn is_cancel(&self) -> bool {
        *self == Btn::Cancel
    }
}

impl DialogRuntime {
    /// Shift+Tab：焦点在按钮集内逆序一跳。
    pub fn tab_back(&mut self) -> Option<Btn> {
        let s = self.session.as_mut()?;
        let list = buttons(s.spec.kind);
        if list.is_empty() {
            return Some(s.focus);
        }
        let idx = list.iter().position(|&b| b == s.focus).unwrap_or(0);
        s.focus = list[(idx + list.len() - 1) % list.len()];
        s.tab_count += 1;
        Some(s.focus)
    }
}

/// 路由完整文案（双步渲染：「提交文本→确认」）。
pub fn render_route_full(r: &Route, out: &mut [u8]) -> usize {
    let mut n = render_route(r, out);
    if r.secondary() != Act::None {
        let sec = Route { acts: [r.secondary(), Act::None], n: 1, press: 0 };
        let mut tmp = [0u8; 16];
        let m = render_route(&sec, &mut tmp);
        if n + 3 + m <= out.len() {
            // '→' U+2192 的 UTF-8 编码。
            out[n] = 0xE2;
            out[n + 1] = 0x86;
            out[n + 2] = 0x92;
            n += 3;
            out[n..n + m].copy_from_slice(&tmp[..m]);
            n += m;
        }
    }
    n
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F207 自检（判据面：清单入册 + 危险框 Esc 10 例全取消 + Enter 零竞态）。
pub fn run_dialksem_checks() -> CheckSet {
    let mut set = CheckSet::new("F207-dialksem");

    // 1. 登记册：四类对话框逐个入册、清单扫描可枚举。
    let mut reg = DialogRegistry::new();
    reg.register(DialogSpec { id: 1, kind: DialogKind::Normal, default_btn: Btn::Ok, focus_btn: Btn::Ok, danger_level: 0 });
    reg.register(DialogSpec { id: 2, kind: DialogKind::Dangerous, default_btn: Btn::Ok, focus_btn: Btn::Ok, danger_level: 2 });
    reg.register(DialogSpec { id: 3, kind: DialogKind::TextEntry, default_btn: Btn::Ok, focus_btn: Btn::Ok, danger_level: 0 });
    reg.register(DialogSpec { id: 4, kind: DialogKind::NoButton, default_btn: Btn::None, focus_btn: Btn::None, danger_level: 0 });
    let mut ids = [0u16; REG_CAP];
    let n = reg.scan_ids(&mut ids);
    set.add(
        "registry scans all registered ids",
        n == 4 && ids[0] == 1 && ids[3] == 4 && reg.dangerous_count() == 1,
        "",
    );

    // 2. 危险框登记制：default/focus 偏离取消 → 登记时强制纠正并入账。
    let s2 = reg.spec(2).unwrap();
    set.add(
        "dangerous dialog coerced to cancel focus/default",
        s2.focus_btn == Btn::Cancel && s2.default_btn == Btn::Cancel && reg.coerced_focus == 2,
        "",
    );

    // 3. 危险框 Esc 10 例全为取消（删除类清单，焦点任意、确认键任意）。
    let mut del_reg = DialogRegistry::new();
    for k in 0..10u16 {
        del_reg.register(DialogSpec {
            id: 100 + k,
            kind: DialogKind::Dangerous,
            default_btn: if k % 2 == 0 { Btn::Ok } else { Btn::Cancel },
            focus_btn: if k % 3 == 0 { Btn::Ok } else { Btn::Cancel },
            danger_level: 2,
        });
    }
    let mut esc_all_cancel = true;
    for k in 0..10u16 {
        let s = del_reg.spec(100 + k).unwrap();
        let r = route(&s, s.focus_btn, Key::Esc, false);
        if r.primary() != Act::Cancel {
            esc_all_cancel = false;
        }
    }
    set.add("10 delete dialogs: esc always cancel", esc_all_cancel, "");

    // 4. 普通对话框 Enter 触发确认；焦点在取消上时 Enter = 取消。
    let s1 = reg.spec(1).unwrap();
    set.add(
        "enter confirms normal dialog",
        route(&s1, Btn::Ok, Key::Enter, false).primary() == Act::Ok
            && route(&s1, Btn::None, Key::Enter, false).primary() == Act::Ok
            && route(&s1, Btn::Cancel, Key::Enter, false).primary() == Act::Cancel,
        "",
    );

    // 5. 含文本输入：草稿脏 → Enter 先提交文本再确认（按序两步）。
    let s3 = reg.spec(3).unwrap();
    let r5 = route(&s3, Btn::Ok, Key::Enter, true);
    set.add(
        "text entry: commit text then confirm",
        r5.primary() == Act::CommitText && r5.secondary() == Act::Ok && r5.n == 2,
        "",
    );

    // 6. 含文本输入：草稿干净 → Enter 只确认不重复提交。
    let r6 = route(&s3, Btn::Ok, Key::Enter, false);
    set.add(
        "text entry clean: single confirm",
        r6.primary() == Act::Ok && r6.n == 1,
        "",
    );

    // 7. 无按钮浮层：Esc 仅关闭（无副作用）、Enter 无动作。
    let s4 = reg.spec(4).unwrap();
    let r7 = route(&s4, Btn::None, Key::Esc, false);
    let r8 = route(&s4, Btn::None, Key::Enter, false);
    set.add(
        "no-button overlay: esc closes only, enter inert",
        r7.primary() == Act::Close && r7.n == 1 && r8.n == 0,
        "",
    );

    // 8. 危险框 Enter 默认安全：焦点被强制在取消 → Enter = 取消（不删）。
    let s2b = reg.spec(2).unwrap();
    set.add(
        "dangerous enter with forced focus = cancel",
        route(&s2b, s2b.focus_btn, Key::Enter, false).primary() == Act::Cancel,
        "",
    );

    // 9. Enter 双触发竞态消除：一次按键按序完成「提交文本→确认关框」，
    //    会话关闭；同 press_id 重放（重开框后）→ 空 Route（0 竞态）。
    let mut rt = DialogRuntime::new();
    rt.registry_mut().register(s3);
    let _ = rt.open(3, 0);
    rt.type_text();
    let r9 = rt.press(Key::Enter, 777, 10);
    let one_press_closes = !rt.is_open() && rt.draft_commits() == 1 && rt.confirmed == 1;
    let _ = rt.open(3, 20);
    rt.type_text();
    let r9b = rt.press(Key::Enter, 777, 30); // 777 已消费 → 单耗队列拦下
    set.add(
        "single enter commits text and closes (zero race)",
        r9.primary() == Act::CommitText
            && r9.secondary() == Act::Ok
            && one_press_closes
            && r9b.n == 0
            && rt.blocked_count() == 1
            && rt.is_open(),
        "",
    );

    // 10. 草稿干净的 Enter：单步确认关框（无提交动作、不二次提交）。
    let mut rt2 = DialogRuntime::new();
    rt2.registry_mut().register(s3);
    let _ = rt2.open(3, 0);
    let r10 = rt2.press(Key::Enter, 778, 0);
    set.add(
        "clean text-entry enter confirms once",
        r10.primary() == Act::Ok && r10.n == 1 && !rt2.is_open() && rt2.confirmed == 1,
        "",
    );

    // 11. 决策账本：动作文案可渲染、账本定容留痕（最新一条 = 被拦的 777）。
    let mut buf = [0u8; 32];
    let n11 = render_route(&r9, &mut buf);
    let led = rt.route_ledger().newest_first();
    set.add(
        "route ledger records decisions",
        &buf[..n11] == "提交文本".as_bytes() && led.len() >= 2 && led[0].press == 777,
        "",
    );

    // 12. 登记缺陷审计：NoButton 声明了按钮 → 违规入账；纠正后 audit=0。
    let mut reg2 = DialogRegistry::new();
    reg2.register(DialogSpec { id: 9, kind: DialogKind::NoButton, default_btn: Btn::Ok, focus_btn: Btn::None, danger_level: 0 });
    reg2.register(DialogSpec { id: 10, kind: DialogKind::Normal, default_btn: Btn::None, focus_btn: Btn::Ok, danger_level: 0 });
    set.add(
        "registry audit counts violations",
        reg2.violations == 2 && reg2.audit() == 2,
        "",
    );

    // 13. fuzz（xorshift32 范式）：随机 分类/焦点/键/草稿/按压 3000 轮
    //     （每个 press_id 恰派发两次）——不变量：Esc 永不产生确认/提交；
    //     无按钮浮层 Enter 恒无动作；第二次派发恒空（0 竞态）。
    let kinds = [
        DialogKind::Normal,
        DialogKind::Dangerous,
        DialogKind::TextEntry,
        DialogKind::NoButton,
    ];
    let mut router = KeyRouter::new();
    let mut x: u32 = 0x9E3779B9;
    let mut fuzz_ok = true;
    for i in 0..3000u32 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let kind = kinds[(x % 4) as usize];
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let focus = match x % 3 {
            0 => Btn::Ok,
            1 => Btn::Cancel,
            _ => Btn::None,
        };
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let key = if x % 2 == 0 { Key::Enter } else { Key::Esc };
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let dirty = x % 2 == 0;
        // 危险框按登记制纠正焦点/默认。
        let mut spec = DialogSpec { id: 1, kind, default_btn: Btn::Ok, focus_btn: focus, danger_level: 0 };
        if kind == DialogKind::Dangerous {
            spec.focus_btn = Btn::Cancel;
            spec.default_btn = Btn::Cancel;
        }
        let press_id = i / 2;
        let r = router.dispatch(&spec, focus, key, dirty, press_id);
        if key == Key::Esc {
            if r.primary() == Act::Ok || r.primary() == Act::CommitText {
                fuzz_ok = false;
            }
        }
        if kind == DialogKind::NoButton && key == Key::Enter && r.n != 0 {
            fuzz_ok = false;
        }
        if r.n > 2 || (r.n < 2 && r.secondary() != Act::None) {
            fuzz_ok = false;
        }
        // 每个 press_id 的第二次派发必空（要么被单耗队列拦下，要么首轮
        // 本就 n==0 未入队、复跑仍 n==0）——0 竞态判据。
        if i % 2 == 1 {
            let r2 = router.dispatch(&spec, focus, key, dirty, press_id);
            if r2.n > 0 {
                fuzz_ok = false;
            }
        }
    }
    set.add(
        "key routing fuzz 3000 rounds zero races",
        fuzz_ok && router.blocked() > 0,
        "",
    );

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(id: u16, kind: DialogKind) -> DialogSpec {
        DialogSpec { id, kind, default_btn: Btn::Ok, focus_btn: Btn::Ok, danger_level: 0 }
    }

    #[test]
    fn esc_never_confirms_any_kind() {
        for kind in [DialogKind::Normal, DialogKind::Dangerous, DialogKind::TextEntry] {
            let s = spec(1, kind);
            let r = route(&s, Btn::Ok, Key::Esc, true);
            assert_eq!(r.primary(), Act::Cancel);
        }
        let s = spec(2, DialogKind::NoButton);
        assert_eq!(route(&s, Btn::None, Key::Esc, false).primary(), Act::Close);
    }

    #[test]
    fn focus_cycles_through_buttons() {
        assert_eq!(next_button(DialogKind::Normal, Btn::Ok), Btn::Cancel);
        assert_eq!(next_button(DialogKind::Normal, Btn::Cancel), Btn::Ok);
        assert_eq!(next_button(DialogKind::Normal, Btn::None), Btn::Cancel, "未落焦从首按钮起");
        assert_eq!(next_button(DialogKind::NoButton, Btn::None), Btn::None);
    }

    #[test]
    fn runtime_open_focus_and_cancel() {
        let mut rt = DialogRuntime::new();
        rt.registry_mut().register(DialogSpec {
            id: 5,
            kind: DialogKind::Dangerous,
            default_btn: Btn::Ok,
            focus_btn: Btn::Ok,
            danger_level: 3,
        });
        assert!(rt.open(5, 0));
        assert_eq!(rt.session_focus(), Some(Btn::Cancel), "危险框开框即落取消");
        let r = rt.press(Key::Esc, 1, 10);
        assert_eq!(r.primary(), Act::Cancel);
        assert!(!rt.is_open() && rt.cancelled == 1 && rt.opened == 1);
    }

    #[test]
    fn text_entry_commit_then_confirm_flow() {
        let mut rt = DialogRuntime::new();
        rt.registry_mut().register(spec(3, DialogKind::TextEntry));
        let _ = rt.open(3, 0);
        assert!(rt.type_text());
        let r = rt.press(Key::Enter, 9, 10);
        assert_eq!(r.primary(), Act::CommitText);
        assert_eq!(r.secondary(), Act::Ok);
        assert!(!rt.is_open(), "一次 Enter 按序完成提交文本+确认关框");
        assert_eq!(rt.draft_commits(), 1);
        assert_eq!(rt.confirmed, 1);
    }

    #[test]
    fn registry_overwrites_same_id() {
        let mut reg = DialogRegistry::new();
        reg.register(spec(1, DialogKind::Normal));
        reg.register(DialogSpec { id: 1, kind: DialogKind::TextEntry, default_btn: Btn::Ok, focus_btn: Btn::Cancel, danger_level: 0 });
        assert_eq!(reg.len(), 1);
        assert_eq!(reg.spec(1).unwrap().kind, DialogKind::TextEntry);
    }

    #[test]
    fn tab_back_reverses_button_cycle() {
        let mut rt = DialogRuntime::new();
        rt.registry_mut().register(spec(3, DialogKind::TextEntry));
        let _ = rt.open(3, 0);
        assert_eq!(rt.session_focus(), Some(Btn::Ok));
        assert_eq!(rt.tab(), Some(Btn::Cancel));
        assert_eq!(rt.tab_back(), Some(Btn::Ok));
        assert_eq!(rt.tab_back(), Some(Btn::Cancel), "从首位逆序绕到末位");
    }

    #[test]
    fn audit_report_and_button_semantics() {
        let mut reg = DialogRegistry::new();
        reg.register(spec(1, DialogKind::Normal));
        reg.register(DialogSpec { id: 2, kind: DialogKind::Dangerous, default_btn: Btn::Ok, focus_btn: Btn::Ok, danger_level: 2 });
        let rep = reg.audit_report();
        assert_eq!(rep.total, 2);
        assert_eq!(rep.dangerous, 1);
        assert_eq!(rep.high_danger, 1);
        assert_eq!(rep.violations, 0, "危险框登记时已被纠正");
        assert_eq!(rep.coerced_focus, 2);
        assert!(Btn::Ok.is_confirm() && !Btn::Ok.is_cancel());
        assert!(Btn::Cancel.is_cancel() && !Btn::Cancel.is_confirm());
        assert!(Btn::Custom(3).is_confirm());
        assert!(!Btn::None.is_confirm());
    }

    #[test]
    fn render_route_full_two_step() {
        let s = spec(3, DialogKind::TextEntry);
        let r = route(&s, Btn::Ok, Key::Enter, true);
        let mut buf = [0u8; 32];
        let n = render_route_full(&r, &mut buf);
        assert_eq!(&buf[..n], "提交文本→确认".as_bytes());
        let r2 = route(&s, Btn::Ok, Key::Esc, true);
        let n2 = render_route_full(&r2, &mut buf);
        assert_eq!(&buf[..n2], "取消".as_bytes(), "单步路由无箭头尾");
    }

    #[test]
    fn dialksem_selfcheck_all_green() {
        let set = run_dialksem_checks();
        assert!(set.all_passed(), "F207 自检存在红项");
        assert!(!set.truncated());
        assert!(set.len() >= 8 && set.len() <= 14);
    }
}

// ===========================================================================
// v2 深化批（2026-09-26 · AI-H1 二次对账批）：UI 壳接线 / 持久化 I/O / 判定面扩展
// ===========================================================================

const VXH1_MAGIC: [u8; 4] = *b"VXH1";
const VXH1_VER: u8 = 1;

/// 损坏输入显性拒绝：四类 + 字段越界（kind/按钮标签编码非法）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum V2CodecErr {
    BadMagic,
    BadVersion,
    BadLen,
    BadSum,
    BadField,
}

/// FNV-1a 32 位（校验和唯一实现点）。
fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in data {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

// ---- 持久化 I/O 面：对话框语义登记表（逐条 magic 记录）----

/// 记录长：magic4 + ver1 + id2 + kind1 + 按钮 2×(tag1+val1) + danger1 + sum4。
pub const SPEC_REC_LEN: usize = 4 + 1 + 2 + 1 + 4 + 1 + 4;

/// 分类 ↔ 字节（0..=3，其余拒绝）。
fn kind_enc(k: DialogKind) -> u8 {
    match k {
        DialogKind::Normal => 0,
        DialogKind::Dangerous => 1,
        DialogKind::TextEntry => 2,
        DialogKind::NoButton => 3,
    }
}

fn kind_dec(v: u8) -> Option<DialogKind> {
    match v {
        0 => Some(DialogKind::Normal),
        1 => Some(DialogKind::Dangerous),
        2 => Some(DialogKind::TextEntry),
        3 => Some(DialogKind::NoButton),
        _ => None,
    }
}

/// 按钮 ↔ (标签, 值) 双字节：0=Ok、1=Cancel、2=Custom(val)、255=None；
/// 其余标签拒绝（Custom 载荷原样保真）。
fn btn_enc(b: Btn) -> (u8, u8) {
    match b {
        Btn::Ok => (0, 0),
        Btn::Cancel => (1, 0),
        Btn::Custom(n) => (2, n),
        Btn::None => (255, 0),
    }
}

fn btn_dec(tag: u8, val: u8) -> Option<Btn> {
    match tag {
        0 => Some(Btn::Ok),
        1 => Some(Btn::Cancel),
        2 => Some(Btn::Custom(val)),
        255 => Some(Btn::None),
        _ => None,
    }
}

/// 一条对话框登记的字节级记录（登记表逐条落盘的最小单元）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpecRec {
    pub id: u16,
    pub kind: DialogKind,
    pub default_btn: Btn,
    pub focus_btn: Btn,
    pub danger: u8,
}

impl SpecRec {
    pub fn of(s: &DialogSpec) -> SpecRec {
        SpecRec {
            id: s.id,
            kind: s.kind,
            default_btn: s.default_btn,
            focus_btn: s.focus_btn,
            danger: s.danger_level,
        }
    }

    pub fn to_spec(self) -> DialogSpec {
        DialogSpec {
            id: self.id,
            kind: self.kind,
            default_btn: self.default_btn,
            focus_btn: self.focus_btn,
            danger_level: self.danger,
        }
    }

    pub fn to_bytes(&self) -> [u8; SPEC_REC_LEN] {
        let mut out = [0u8; SPEC_REC_LEN];
        out[..4].copy_from_slice(&VXH1_MAGIC);
        out[4] = VXH1_VER;
        out[5..7].copy_from_slice(&self.id.to_le_bytes());
        out[7] = kind_enc(self.kind);
        let (dt, dv) = btn_enc(self.default_btn);
        out[8] = dt;
        out[9] = dv;
        let (ft, fv) = btn_enc(self.focus_btn);
        out[10] = ft;
        out[11] = fv;
        out[12] = self.danger;
        let sum = fnv1a(&out[..13]).to_le_bytes();
        out[13..17].copy_from_slice(&sum);
        out
    }

    pub fn from_bytes(b: &[u8]) -> Result<SpecRec, V2CodecErr> {
        if b.len() != SPEC_REC_LEN {
            return Err(V2CodecErr::BadLen);
        }
        let mut mg = [0u8; 4];
        mg.copy_from_slice(&b[..4]);
        if mg != VXH1_MAGIC {
            return Err(V2CodecErr::BadMagic);
        }
        if b[4] != VXH1_VER {
            return Err(V2CodecErr::BadVersion);
        }
        let mut sum = [0u8; 4];
        sum.copy_from_slice(&b[13..17]);
        if fnv1a(&b[..13]) != u32::from_le_bytes(sum) {
            return Err(V2CodecErr::BadSum);
        }
        let kind = match kind_dec(b[7]) {
            Some(k) => k,
            None => return Err(V2CodecErr::BadField),
        };
        let default_btn = match btn_dec(b[8], b[9]) {
            Some(x) => x,
            None => return Err(V2CodecErr::BadField),
        };
        let focus_btn = match btn_dec(b[10], b[11]) {
            Some(x) => x,
            None => return Err(V2CodecErr::BadField),
        };
        let mut id = [0u8; 2];
        id.copy_from_slice(&b[5..7]);
        Ok(SpecRec { id: u16::from_le_bytes(id), kind, default_btn, focus_btn, danger: b[12] })
    }
}

// ---- UI 壳接线面：同毫秒竞态时序判定 ----

/// 同毫秒事件排序规则：同毫秒内两枚按键事件按 press_id 升序定序
/// （确定性：与到达顺序无关）——「Enter 双触发 0 竞态」的时序判定面：
/// 小 id 先消费，大 id 落单耗队列被拦（KeyRouter::dispatch）。
pub fn same_ms_order(pa: u32, pb: u32) -> [u32; 2] {
    if pa <= pb {
        [pa, pb]
    } else {
        [pb, pa]
    }
}

/// 档案还原后的语义一致性：从 SpecRec 还原登记 → Esc 路由与原登记
/// 同判（语义随档案还原，不因落盘失真——危险框 Esc 仍恒为取消）。
pub fn esc_after_restore(rec: SpecRec) -> Act {
    esc_action(rec.to_spec().kind)
}

/// F207 v2 自检（首条恒为持久化 round-trip）。
pub fn run_dialksem_v2_checks() -> CheckSet {
    let mut set = CheckSet::new("F207-dialksem-v2");

    // 1. 持久化 round-trip：危险框登记（强制取消后）编码→解码逐字段还原。
    let spec = DialogSpec {
        id: 42,
        kind: DialogKind::Dangerous,
        default_btn: Btn::Cancel,
        focus_btn: Btn::Cancel,
        danger_level: 2,
    };
    let rec = SpecRec::of(&spec);
    let bytes = rec.to_bytes();
    set.add(
        "v2 persist roundtrip dialog spec",
        SpecRec::from_bytes(&bytes) == Ok(rec),
        "",
    );

    // 2. 损坏拒绝四类 + 字段越界（kind=9；重算 sum 使只坏字段）。
    let mut bad1 = bytes;
    bad1[0] = b'X';
    let mut bad2 = bytes;
    bad2[4] = 9;
    let mut bad3 = bytes;
    bad3[8] ^= 0xFF;
    let mut bad4 = bytes;
    bad4[7] = 9;
    let s4 = fnv1a(&bad4[..13]);
    bad4[13..17].copy_from_slice(&s4.to_le_bytes());
    set.add(
        "v2 persist rejects corrupt specs",
        SpecRec::from_bytes(&bad1) == Err(V2CodecErr::BadMagic)
            && SpecRec::from_bytes(&bad2) == Err(V2CodecErr::BadVersion)
            && SpecRec::from_bytes(&bad3) == Err(V2CodecErr::BadSum)
            && SpecRec::from_bytes(&bytes[..bytes.len() - 1]) == Err(V2CodecErr::BadLen)
            && SpecRec::from_bytes(&bad4) == Err(V2CodecErr::BadField),
        "",
    );

    // 3. 语义随档案还原：危险框解码后 Esc 仍恒为取消、焦点仍在取消
    //    （验主册 F207「删除类 Esc 恒为取消」的档案面）。
    set.add(
        "v2 restored dangerous spec keeps esc=cancel",
        esc_after_restore(rec) == Act::Cancel
            && rec.to_spec().focus_btn == Btn::Cancel,
        "",
    );

    // 4. 同毫秒时序：定序与到达顺序无关（两个方向输入同结果）
    //    （验主册 F207「Enter 双触发 0 竞态」时序规则）。
    set.add(
        "v2 same-ms ordering deterministic",
        same_ms_order(9, 3) == [3, 9] && same_ms_order(3, 9) == [3, 9] && same_ms_order(5, 5) == [5, 5],
        "",
    );

    // 5. 按钮编码全枚举往返：Ok/Cancel/Custom(7)/None 无损。
    let btns = [Btn::Ok, Btn::Cancel, Btn::Custom(7), Btn::None];
    set.add(
        "v2 btn codec total",
        btns.iter().all(|&b| {
            let (t, v) = btn_enc(b);
            btn_dec(t, v) == Some(b)
        }),
        "",
    );

    set
}

#[cfg(test)]
mod tests_v2 {
    use super::*;

    #[test]
    fn spec_rec_custom_button_roundtrip() {
        let rec = SpecRec {
            id: 9,
            kind: DialogKind::Normal,
            default_btn: Btn::Custom(200),
            focus_btn: Btn::None,
            danger: 0,
        };
        assert_eq!(SpecRec::from_bytes(&rec.to_bytes()).unwrap(), rec);
    }

    #[test]
    fn no_button_overlay_restores_inert_enter() {
        let rec = SpecRec {
            id: 4,
            kind: DialogKind::NoButton,
            default_btn: Btn::None,
            focus_btn: Btn::None,
            danger: 0,
        };
        let spec = rec.to_spec();
        assert_eq!(route(&spec, Btn::None, Key::Enter, false).n, 0, "还原后 Enter 仍无动作");
    }

    #[test]
    fn v2_selfcheck_all_green() {
        let set = run_dialksem_v2_checks();
        assert!(set.all_passed(), "F207 v2 自检存在红项");
        assert!(!set.truncated());
        assert!((4..=6).contains(&set.len()));
    }
}
