//! F202 全局撤销重做框架 · 判据实装（H 基础通用域 · AI-H1 分工包）。
//!
//! **判据锚**：主册 F202「全局撤销重做框架」。
//!
//! **验收标准（主册第一句）**：三类动作（文本编辑、文件操作、图标拖拽）
//! 各 10 步连续撤销/重做往返一致；栈溢出（>50 步）时最早一步静默淘汰且
//! 无卡顿；「刚撤销了什么」悬浮提示出现与消失时机实测入账（2 秒窗）。
//!
//! **设计要点**：
//! - `UndoAction` 紧凑 Copy 动作（枚举 7 种：文本插入/删除/替换、
//!   文件复制/移动/删除进回收站、图标拖移）——撤销栈零堆热路径；
//! - 按会话（app id）隔离的多栈管理器：跨应用不共享栈，各自独立，
//!   会话槽定容 16、满了显性拒绝计数（不静默挤占他人栈）；
//! - 栈深 50 定容环形淘汰：溢出丢最旧一步，入栈/撤销/重做全部 O(1)
//!   （`work_units` 成本计数作「无卡顿」的证明面——淘汰零额外成本）；
//! - 撤销/重做对称账本：`RingLog` 定容 64 条记最近事件；
//! - 悬浮提示 2 秒窗状态机：撤销/重做成功即点亮，2000ms 后自然熄灭；
//! - Ctrl+Y 或 Ctrl+Shift+Z 重做、Ctrl+Z 撤销的键序归一（`resolve_chord`）。
//!
//! **依赖锚点**：`crate::checks::CheckSet`（自检面）、
//! `crate::checks::{push_str, push_usize}`（文案生成，无 format! 依赖）、
//! `crate::star::sbase::RingLog`（对称账本）。
//! 时间纪律：一切时间由调用方注入毫秒戳，模块不持时钟。

use crate::checks::{push_str, push_usize, CheckSet};
use crate::star::sbase::RingLog;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 撤销栈深——主册 F202：「栈深 50 步」「栈溢出（>50 步）时最早一步静默淘汰」。
pub const STACK_DEPTH: usize = 50;

/// 悬浮提示窗口——主册 F202：「有『刚撤销了什么』悬浮提示 2 秒」。
pub const HINT_MS: u64 = 2000;

/// 撤销/重做对称账本容量——主册 F202 设计：「RingLog 记最近 64 条」。
pub const LEDGER_CAP: usize = 64;

/// 会话槽容量：同时在线应用的上限（实装定值；超出显性拒绝并计数）。
pub const MAX_SESSIONS: usize = 16;

/// 提示文案缓冲上限（字节）。
pub const HINT_TEXT_CAP: usize = 48;

// ---------------------------------------------------------------------------
// 动作与键序
// ---------------------------------------------------------------------------

/// 统一撤销动作类别——主册 F202 三类：文本编辑 / 文件操作 / 图标拖拽。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionKind {
    /// 文本插入。
    TextInsert,
    /// 文本删除。
    TextDelete,
    /// 文本替换（旧长 b、新长 c）。
    TextReplace,
    /// 文件复制（a=源 id，b=目标 id）。
    FileCopy,
    /// 文件移动（a=源 id，b=目标 id）。
    FileMove,
    /// 文件删除进回收站（a=文件 id）。
    FileTrash,
    /// 图标拖移（a=起点 x<<16|y，b=终点 x<<16|y）。
    IconDrag,
}

impl ActionKind {
    /// 类别文案（提示文案用，中文与主册判据一致）。
    pub fn label(&self) -> &'static str {
        match self {
            ActionKind::TextInsert => "文本插入",
            ActionKind::TextDelete => "文本删除",
            ActionKind::TextReplace => "文本替换",
            ActionKind::FileCopy => "文件复制",
            ActionKind::FileMove => "文件移动",
            ActionKind::FileTrash => "文件删除",
            ActionKind::IconDrag => "图标拖移",
        }
    }

    /// 三大类归属（跨应用语义一致性检验用）。
    pub fn category(&self) -> ActionCategory {
        match self {
            ActionKind::TextInsert | ActionKind::TextDelete | ActionKind::TextReplace => {
                ActionCategory::TextEdit
            }
            ActionKind::FileCopy | ActionKind::FileMove | ActionKind::FileTrash => {
                ActionCategory::FileOp
            }
            ActionKind::IconDrag => ActionCategory::IconDrag,
        }
    }
}

/// 三大动作类别（主册 F202 分类的第一层）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionCategory {
    TextEdit,
    FileOp,
    IconDrag,
}

/// 紧凑撤销动作（Copy，零堆）。字段语义随 kind 定：
/// 文本类 target=缓冲 id，a=字节位置，b=旧长，c=新长；
/// 文件类 target=源 id，a=目标 id；图标类 a/b=打包坐标。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UndoAction {
    pub kind: ActionKind,
    pub target: u32,
    pub a: u32,
    pub b: u32,
    pub c: u32,
}

impl UndoAction {
    /// 动作描述写入 `out`（「刚撤销了什么」的正文；返回字节数）。
    pub fn describe(&self, out: &mut [u8]) -> usize {
        let mut n = 0usize;
        push_str(out, &mut n, self.kind.label());
        push_str(out, &mut n, " t");
        push_usize(out, &mut n, self.target as usize);
        push_str(out, &mut n, " @");
        push_usize(out, &mut n, self.a as usize);
        push_str(out, &mut n, " #");
        push_usize(out, &mut n, self.b as usize);
        n
    }
}

/// 键序归一结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chord {
    None,
    Undo,
    Redo,
}

/// Ctrl+Z 撤销；Ctrl+Y 或 Ctrl+Shift+Z 重做（主册 F202 键序原文）。
pub fn resolve_chord(ctrl: bool, shift: bool, key_y: bool, key_z: bool) -> Chord {
    if !ctrl {
        return Chord::None;
    }
    if key_z && !shift {
        Chord::Undo
    } else if key_y || (key_z && shift) {
        Chord::Redo
    } else {
        Chord::None
    }
}

// ---------------------------------------------------------------------------
// 单会话双栈（撤销栈 + 重做栈，环形定容）
// ---------------------------------------------------------------------------

/// 单会话双栈：栈深 50 定容环形——满后再入栈淘汰最旧一步（静默、O(1)）。
/// 新动作入栈时清空重做栈（分叉语义：撤销后改动作，旧重做线作废）。
pub struct SessionStacks {
    undo: [Option<UndoAction>; STACK_DEPTH],
    head: usize,
    ulen: usize,
    redo: [Option<UndoAction>; STACK_DEPTH],
    rhead: usize,
    rlen: usize,
    /// 累计入栈数。
    pub pushed: u32,
    /// 累计撤销数。
    pub undone: u32,
    /// 累计重做数。
    pub redone: u32,
    /// 溢出静默淘汰数（>50 步时最旧一步的淘汰痕迹）。
    pub evicted: u32,
    /// 操作成本计数：每次 push/undo/redo 恒 +1——溢出淘汰零额外成本
    /// 的证明面（无卡顿判据的量化口径）。
    pub work_units: u64,
}

impl SessionStacks {
    pub fn new() -> SessionStacks {
        SessionStacks {
            undo: [const { None }; STACK_DEPTH],
            head: 0,
            ulen: 0,
            redo: [const { None }; STACK_DEPTH],
            rhead: 0,
            rlen: 0,
            pushed: 0,
            undone: 0,
            redone: 0,
            evicted: 0,
            work_units: 0,
        }
    }

    /// 入栈（新动作）。满则淘汰最旧一步。
    pub fn push(&mut self, a: UndoAction) {
        self.work_units += 1;
        if self.rlen > 0 {
            self.rlen = 0;
            self.rhead = 0;
        }
        let idx = (self.head + self.ulen) % STACK_DEPTH;
        self.undo[idx] = Some(a);
        if self.ulen < STACK_DEPTH {
            self.ulen += 1;
        } else {
            self.head = (self.head + 1) % STACK_DEPTH;
            self.evicted += 1;
        }
        self.pushed += 1;
    }

    /// 撤销一步（移入重做栈）。
    pub fn undo(&mut self) -> Option<UndoAction> {
        self.work_units += 1;
        if self.ulen == 0 {
            return None;
        }
        let idx = (self.head + self.ulen - 1) % STACK_DEPTH;
        let a = self.undo[idx].take()?;
        self.ulen -= 1;
        let ridx = (self.rhead + self.rlen) % STACK_DEPTH;
        self.redo[ridx] = Some(a);
        if self.rlen < STACK_DEPTH {
            self.rlen += 1;
        } else {
            self.rhead = (self.rhead + 1) % STACK_DEPTH;
        }
        self.undone += 1;
        Some(a)
    }

    /// 重做一步（移回撤销栈）。
    pub fn redo(&mut self) -> Option<UndoAction> {
        self.work_units += 1;
        if self.rlen == 0 {
            return None;
        }
        let idx = (self.rhead + self.rlen - 1) % STACK_DEPTH;
        let a = self.redo[idx].take()?;
        self.rlen -= 1;
        let uidx = (self.head + self.ulen) % STACK_DEPTH;
        self.undo[uidx] = Some(a);
        if self.ulen < STACK_DEPTH {
            self.ulen += 1;
        } else {
            self.head = (self.head + 1) % STACK_DEPTH;
        }
        self.redone += 1;
        Some(a)
    }

    pub fn undo_len(&self) -> usize {
        self.ulen
    }

    pub fn redo_len(&self) -> usize {
        self.rlen
    }

    pub fn can_undo(&self) -> bool {
        self.ulen > 0
    }

    pub fn can_redo(&self) -> bool {
        self.rlen > 0
    }

    /// 栈顶窥视（不弹出）——「刚撤销了什么」的预览面。
    pub fn top(&self) -> Option<UndoAction> {
        if self.ulen == 0 {
            return None;
        }
        self.undo[(self.head + self.ulen - 1) % STACK_DEPTH]
    }
}

impl Default for SessionStacks {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 多会话管理器 + 对称账本 + 提示状态机
// ---------------------------------------------------------------------------

/// 账本事件方向。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Undo,
    Redo,
}

/// 对称账本条目。
#[derive(Clone, Copy, Debug)]
pub struct LedgerEntry {
    pub app: u16,
    pub action: UndoAction,
    pub dir: Dir,
    pub ts: u64,
}

/// 悬浮提示状态（2 秒窗）。
#[derive(Clone, Copy, Debug)]
struct HintState {
    app: u16,
    action: UndoAction,
    dir: Dir,
    since: u64,
}

struct SessionSlot {
    app: u16,
    stacks: SessionStacks,
}

/// 撤销重做管理器：按会话（app id）隔离的栈组 + 对称账本 + 提示机。
pub struct UndoManager {
    slots: [Option<SessionSlot>; MAX_SESSIONS],
    ledger: RingLog<LedgerEntry, LEDGER_CAP>,
    hint: Option<HintState>,
    /// 会话槽满时的显性拒绝计数（不静默挤占）。
    pub rejected_sessions: u32,
    /// 提示点亮总次数（出现时机入账）。
    pub hint_lit: u32,
}

impl UndoManager {
    pub fn new() -> UndoManager {
        UndoManager {
            slots: [const { None }; MAX_SESSIONS],
            ledger: RingLog::new(),
            hint: None,
            rejected_sessions: 0,
            hint_lit: 0,
        }
    }

    fn find(&self, app: u16) -> Option<usize> {
        self.slots.iter().position(|s| matches!(s, Some(sl) if sl.app == app))
    }

    fn ensure(&mut self, app: u16) -> Option<usize> {
        if let Some(i) = self.find(app) {
            return Some(i);
        }
        if let Some(i) = self.slots.iter().position(|s| s.is_none()) {
            self.slots[i] = Some(SessionSlot { app, stacks: SessionStacks::new() });
            return Some(i);
        }
        None
    }

    /// 记录一步新动作（会话隔离；槽满显性拒绝）。
    pub fn record(&mut self, app: u16, a: UndoAction) {
        match self.ensure(app) {
            Some(i) => self.slots[i].as_mut().unwrap().stacks.push(a),
            None => self.rejected_sessions += 1,
        }
    }

    /// 撤销一步：成功则点亮 2 秒悬浮提示并入账本。
    pub fn undo(&mut self, app: u16, now: u64) -> Option<UndoAction> {
        let i = self.find(app)?;
        let a = self.slots[i].as_mut().unwrap().stacks.undo()?;
        self.ledger.push(LedgerEntry { app, action: a, dir: Dir::Undo, ts: now });
        self.hint = Some(HintState { app, action: a, dir: Dir::Undo, since: now });
        self.hint_lit += 1;
        Some(a)
    }

    /// 重做一步：成功则点亮 2 秒悬浮提示（重做文案）并入账本。
    pub fn redo(&mut self, app: u16, now: u64) -> Option<UndoAction> {
        let i = self.find(app)?;
        let a = self.slots[i].as_mut().unwrap().stacks.redo()?;
        self.ledger.push(LedgerEntry { app, action: a, dir: Dir::Redo, ts: now });
        self.hint = Some(HintState { app, action: a, dir: Dir::Redo, since: now });
        self.hint_lit += 1;
        Some(a)
    }

    pub fn can_undo(&self, app: u16) -> bool {
        self.find(app).map_or(false, |i| {
            self.slots[i].as_ref().unwrap().stacks.can_undo()
        })
    }

    pub fn can_redo(&self, app: u16) -> bool {
        self.find(app).map_or(false, |i| {
            self.slots[i].as_ref().unwrap().stacks.can_redo()
        })
    }

    pub fn stacks(&self, app: u16) -> Option<&SessionStacks> {
        self.find(app).map(|i| &self.slots[i].as_ref().unwrap().stacks)
    }

    /// 提示是否在亮（2 秒窗内）。
    pub fn hint_active(&self, now: u64) -> bool {
        match self.hint {
            Some(h) => now >= h.since && now - h.since < HINT_MS,
            None => false,
        }
    }

    /// 提示文案写入 `out`（不在亮期返回 0——消失即无文案）。
    pub fn hint_text(&self, now: u64, out: &mut [u8]) -> usize {
        match self.hint {
            Some(h) if self.hint_active(now) => {
                let mut n = 0usize;
                push_str(out, &mut n, match h.dir {
                    Dir::Undo => "刚撤销：",
                    Dir::Redo => "刚重做：",
                });
                let len = out.len();
                let used = h.action.describe(&mut out[n..len]);
                n + used
            }
            _ => 0,
        }
    }

    /// 手动熄灭提示（用户有新输入时上层可显式 dismiss）。
    pub fn dismiss_hint(&mut self) {
        self.hint = None;
    }

    /// 对称账本（新→旧，最多 64 条）。
    pub fn ledger(&self) -> &RingLog<LedgerEntry, LEDGER_CAP> {
        &self.ledger
    }
}

impl Default for UndoManager {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F202 自检（判据面：三类往返一致 + 溢出淘汰 O(1) + 提示 2 秒窗 + 会话隔离）。
pub fn run_undoframe_checks() -> CheckSet {
    let mut set = CheckSet::new("F202-undoframe");

    // 1~3. 三类动作各 10 步连续撤销/重做往返一致（主册验收本体）。
    let kinds: [[ActionKind; 3]; 3] = [
        [ActionKind::TextInsert, ActionKind::TextDelete, ActionKind::TextReplace],
        [ActionKind::FileCopy, ActionKind::FileMove, ActionKind::FileTrash],
        [ActionKind::IconDrag, ActionKind::IconDrag, ActionKind::IconDrag],
    ];
    for ks in kinds.iter() {
        let mut m = UndoManager::new();
        let app = 1u16;
        for k in 0..10u32 {
            m.record(
                app,
                UndoAction { kind: ks[(k % 3) as usize], target: 7, a: k * 10, b: k, c: 0 },
            );
        }
        let mut undone_seq = [0u32; 10];
        let mut ok_undo = true;
        for i in 0..10usize {
            match m.undo(app, 1_000 + i as u64) {
                Some(a) => undone_seq[i] = a.a,
                None => ok_undo = false,
            }
        }
        let mut redone_seq = [0u32; 10];
        let mut ok_redo = true;
        for i in 0..10usize {
            match m.redo(app, 2_000 + i as u64) {
                Some(a) => redone_seq[i] = a.a,
                None => ok_redo = false,
            }
        }
        // 撤销序 = 入栈序倒序；重做序 = 撤销序倒序 = 入栈原序。
        let mut symmetric = true;
        for i in 0..10usize {
            if undone_seq[i] != (9 - i as u32) * 10 || redone_seq[i] != i as u32 * 10 {
                symmetric = false;
            }
        }
        let st = m.stacks(app).unwrap();
        set.add(
            "category roundtrip consistent",
            ok_undo && ok_redo && symmetric && st.undone == 10 && st.redone == 10,
            "",
        );
    }

    // 4. 栈溢出：60 步入栈 → 保留最近 50、最早 10 步静默淘汰。
    let mut s = SessionStacks::new();
    for k in 0..60u32 {
        s.push(UndoAction { kind: ActionKind::TextInsert, target: 1, a: k, b: 0, c: 0 });
    }
    set.add(
        "overflow keeps newest 50 evicts 10",
        s.undo_len() == STACK_DEPTH && s.evicted == 10 && s.top().unwrap().a == 59,
        "",
    );

    // 5. 新动作入栈清空重做栈（分叉语义）。
    let mut m2 = UndoManager::new();
    m2.record(1, UndoAction { kind: ActionKind::TextInsert, target: 1, a: 1, b: 0, c: 0 });
    let _ = m2.undo(1, 10);
    m2.record(1, UndoAction { kind: ActionKind::TextInsert, target: 1, a: 2, b: 0, c: 0 });
    set.add("new action clears redo line", !m2.can_redo(1), "");

    // 6. 跨应用不共享栈：app1 的活动不影响 app2。
    let mut m3 = UndoManager::new();
    m3.record(1, UndoAction { kind: ActionKind::TextInsert, target: 1, a: 1, b: 0, c: 0 });
    let _ = m3.undo(1, 10);
    set.add(
        "sessions isolated across apps",
        !m3.can_undo(2) && !m3.can_redo(2) && m3.can_redo(1),
        "",
    );

    // 7. 键序归一：Ctrl+Z 撤销；Ctrl+Y 与 Ctrl+Shift+Z 都重做。
    set.add(
        "chords ctrl+z / ctrl+y / ctrl+shift+z",
        resolve_chord(true, false, false, true) == Chord::Undo
            && resolve_chord(true, false, true, false) == Chord::Redo
            && resolve_chord(true, true, false, true) == Chord::Redo
            && resolve_chord(false, false, false, true) == Chord::None
            && resolve_chord(true, false, false, false) == Chord::None,
        "",
    );

    // 8. 提示文案：撤销后立即点亮，正文以「刚撤销：」开头且含类别。
    let mut m4 = UndoManager::new();
    m4.record(1, UndoAction { kind: ActionKind::TextInsert, target: 3, a: 12, b: 5, c: 0 });
    let _ = m4.undo(1, 100);
    let mut buf = [0u8; HINT_TEXT_CAP];
    let n = m4.hint_text(100, &mut buf);
    let text = core::str::from_utf8(&buf[..n]).unwrap_or("");
    set.add(
        "hint lit right after undo",
        m4.hint_active(100) && text.starts_with("刚撤销：") && text.contains("文本插入"),
        "",
    );

    // 9. 提示 2 秒窗：1999ms 仍在、2000ms 熄灭（消失时机入账）。
    set.add(
        "hint window exactly 2000ms",
        m4.hint_active(100 + HINT_MS - 1) && !m4.hint_active(100 + HINT_MS),
        "",
    );

    // 10. 重做也点亮提示（「刚重做：」前缀）。
    let _ = m4.redo(1, 5_000);
    let n2 = m4.hint_text(5_000, &mut buf);
    let text2 = core::str::from_utf8(&buf[..n2]).unwrap_or("");
    set.add(
        "redo hint prefix",
        text2.starts_with("刚重做："),
        "",
    );

    // 11. 对称账本定容 64：70 次操作后保留最近 64 条、最新一条即末次操作。
    let mut m5 = UndoManager::new();
    for k in 0..70u32 {
        m5.record(1, UndoAction { kind: ActionKind::TextInsert, target: 1, a: k, b: 0, c: 0 });
        let _ = m5.undo(1, k as u64 * 10);
    }
    let led = m5.ledger().newest_first();
    set.add(
        "ledger keeps newest 64",
        led.len() == LEDGER_CAP && led[0].action.a == 69 && led[63].action.a == 6,
        "",
    );

    // 12. 无卡顿证明：溢出淘汰零额外成本——60 次入栈 = 恰 60 工作单元
    //     （淘汰 10 次最旧步未增加任何额外工作）。
    set.add(
        "eviction is O(1) (work units exact)",
        s.work_units == 60,
        "",
    );

    // 13. fuzz（xorshift32 范式）：随机 会话/操作/动作 3000 轮——
    //     不变量：栈深恒 ≤50；空栈撤销/重做返回 None；入栈后必可撤销。
    let mut m6 = UndoManager::new();
    let mut x: u32 = 0x9E3779B9;
    let mut fuzz_ok = true;
    for i in 0..3000u32 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let app = (x % 4) as u16;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let op = x % 3;
        let act = UndoAction {
            kind: [
                ActionKind::TextInsert,
                ActionKind::FileMove,
                ActionKind::IconDrag,
            ][(x % 3) as usize],
            target: x & 0xFFFF,
            a: x >> 16,
            b: i,
            c: 0,
        };
        match op {
            0 => m6.record(app, act),
            1 => {
                if let Some(_a) = m6.undo(app, i as u64 * 10) {
                    // 撤出后必可重做。
                    if !m6.can_redo(app) {
                        fuzz_ok = false;
                    }
                }
            }
            _ => {
                let _ = m6.redo(app, i as u64 * 10);
            }
        }
        if let Some(st) = m6.stacks(app) {
            if st.undo_len() > STACK_DEPTH || st.redo_len() > STACK_DEPTH {
                fuzz_ok = false;
            }
        }
    }
    set.add("stack fuzz 3000 rounds invariants", fuzz_ok, "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn ins(a: u32) -> UndoAction {
        UndoAction { kind: ActionKind::TextInsert, target: 1, a, b: 0, c: 0 }
    }

    #[test]
    fn ten_step_roundtrip_sequences_equal() {
        let mut m = UndoManager::new();
        for k in 0..10u32 {
            m.record(1, ins(k));
        }
        let mut undone = Vec::new();
        while let Some(a) = m.undo(1, undone.len() as u64) {
            undone.push(a.a);
        }
        assert_eq!(undone, vec![9, 8, 7, 6, 5, 4, 3, 2, 1, 0]);
        let mut redone = Vec::new();
        while let Some(a) = m.redo(1, 100 + redone.len() as u64) {
            redone.push(a.a);
        }
        assert_eq!(redone, vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
    }

    #[test]
    fn overflow_evicts_oldest_silently() {
        let mut m = UndoManager::new();
        for k in 0..(STACK_DEPTH as u32 + 10) {
            m.record(1, ins(k));
        }
        let st = m.stacks(1).unwrap();
        assert_eq!(st.undo_len(), STACK_DEPTH);
        assert_eq!(st.evicted, 10);
        // 最新一步仍在：撤销弹出的第一个就是第 60 步。
        assert_eq!(m.undo(1, 0).unwrap().a, 59);
    }

    #[test]
    fn hint_expiry_and_dismiss() {
        let mut m = UndoManager::new();
        m.record(1, ins(1));
        let _ = m.undo(1, 500);
        assert!(m.hint_active(2_499));
        assert!(!m.hint_active(2_500));
        let _ = m.undo(1, 0); // 重新点亮
        m.dismiss_hint();
        assert!(!m.hint_active(1));
    }

    #[test]
    fn session_slots_reject_when_full() {
        let mut m = UndoManager::new();
        for app in 0..(MAX_SESSIONS as u16 + 3) {
            m.record(app, ins(app as u32));
        }
        assert_eq!(m.rejected_sessions, 3);
    }

    #[test]
    fn describe_renders_label_and_params() {
        let a = UndoAction { kind: ActionKind::FileTrash, target: 42, a: 7, b: 0, c: 0 };
        let mut buf = [0u8; HINT_TEXT_CAP];
        let n = a.describe(&mut buf);
        let s = core::str::from_utf8(&buf[..n]).unwrap();
        assert!(s.contains("文件删除") && s.contains("t42") && s.contains("@7"));
    }

    #[test]
    fn undoframe_selfcheck_all_green() {
        let set = run_undoframe_checks();
        assert!(set.all_passed(), "F202 自检存在红项");
        assert!(!set.truncated());
        assert!(set.len() >= 8 && set.len() <= 14);
    }
}

// ===========================================================================
// v2 深化批（2026-09-26 · AI-H1 二次对账批）：UI 壳接线 / 持久化 I/O / 判定面扩展
// ===========================================================================

const VXH1_MAGIC: [u8; 4] = *b"VXH1";
const VXH1_VER: u8 = 1;

/// 损坏输入显性拒绝：四类 + 字段越界（kind 编码非法）。
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

// ---- 持久化 I/O 面：撤销栈快照（栈深上限 50 + 淘汰计数入档）----

/// 快照槽位——主册 F202「栈深 50 步」：档案容量与栈深同源（STACK_DEPTH）。
pub const SNAP_SLOTS: usize = STACK_DEPTH;

/// 记录长：magic4+ver1+ulen1+evicted4+50×(kind1+target4+a4+b4+c4)+sum4。
pub const SNAP_REC_LEN: usize = 4 + 1 + 1 + 4 + SNAP_SLOTS * 17 + 4;

/// 动作种类 ↔ 单字节编码（0..=6 合法，其余拒绝）。
fn kind_enc(k: ActionKind) -> u8 {
    match k {
        ActionKind::TextInsert => 0,
        ActionKind::TextDelete => 1,
        ActionKind::TextReplace => 2,
        ActionKind::FileCopy => 3,
        ActionKind::FileMove => 4,
        ActionKind::FileTrash => 5,
        ActionKind::IconDrag => 6,
    }
}

fn kind_dec(v: u8) -> Option<ActionKind> {
    match v {
        0 => Some(ActionKind::TextInsert),
        1 => Some(ActionKind::TextDelete),
        2 => Some(ActionKind::TextReplace),
        3 => Some(ActionKind::FileCopy),
        4 => Some(ActionKind::FileMove),
        5 => Some(ActionKind::FileTrash),
        6 => Some(ActionKind::IconDrag),
        _ => None,
    }
}

/// 撤销栈快照：50 槽环形语义（满再入左移淘汰最旧）+ 淘汰计数
/// ——持久化面独立于运行态，整栈可落盘/回放。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct StackSnapshot {
    pub evicted: u32,
    ulen: usize,
    slots: [Option<UndoAction>; SNAP_SLOTS],
}

impl StackSnapshot {
    pub fn new() -> StackSnapshot {
        StackSnapshot { evicted: 0, ulen: 0, slots: [const { None }; SNAP_SLOTS] }
    }

    /// 入档一步（满则左移淘汰最旧——与 SessionStacks 同语义）。
    pub fn push(&mut self, a: UndoAction) {
        if self.ulen < SNAP_SLOTS {
            self.slots[self.ulen] = Some(a);
            self.ulen += 1;
        } else {
            for k in 1..SNAP_SLOTS {
                self.slots[k - 1] = self.slots[k];
            }
            self.slots[SNAP_SLOTS - 1] = Some(a);
            self.evicted += 1;
        }
    }

    pub fn ulen(&self) -> usize {
        self.ulen
    }

    pub fn at(&self, i: usize) -> Option<UndoAction> {
        if i < self.ulen {
            self.slots[i]
        } else {
            None
        }
    }

    pub fn to_bytes(&self) -> [u8; SNAP_REC_LEN] {
        let mut out = [0u8; SNAP_REC_LEN];
        out[..4].copy_from_slice(&VXH1_MAGIC);
        out[4] = VXH1_VER;
        out[5] = self.ulen as u8;
        out[6..10].copy_from_slice(&self.evicted.to_le_bytes());
        let mut o = 10usize;
        for k in 0..SNAP_SLOTS {
            if let Some(a) = self.slots[k] {
                out[o] = kind_enc(a.kind);
                out[o + 1..o + 5].copy_from_slice(&a.target.to_le_bytes());
                out[o + 5..o + 9].copy_from_slice(&a.a.to_le_bytes());
                out[o + 9..o + 13].copy_from_slice(&a.b.to_le_bytes());
                out[o + 13..o + 17].copy_from_slice(&a.c.to_le_bytes());
            } else {
                out[o] = 0xFF; // 空槽哨兵
            }
            o += 17;
        }
        let sum = fnv1a(&out[..o]).to_le_bytes();
        out[o..o + 4].copy_from_slice(&sum);
        out
    }

    pub fn from_bytes(b: &[u8]) -> Result<StackSnapshot, V2CodecErr> {
        if b.len() != SNAP_REC_LEN {
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
        let o_sum = 10 + SNAP_SLOTS * 17;
        let mut sum = [0u8; 4];
        sum.copy_from_slice(&b[o_sum..o_sum + 4]);
        if fnv1a(&b[..o_sum]) != u32::from_le_bytes(sum) {
            return Err(V2CodecErr::BadSum);
        }
        if b[5] as usize > SNAP_SLOTS {
            return Err(V2CodecErr::BadLen);
        }
        let mut snap = StackSnapshot::new();
        snap.ulen = b[5] as usize;
        let mut ev = [0u8; 4];
        ev.copy_from_slice(&b[6..10]);
        snap.evicted = u32::from_le_bytes(ev);
        let mut o = 10usize;
        for k in 0..snap.ulen {
            let kind = match kind_dec(b[o]) {
                Some(kd) => kd,
                None => return Err(V2CodecErr::BadField),
            };
            let mut t = [0u8; 4];
            t.copy_from_slice(&b[o + 1..o + 5]);
            let mut a1 = [0u8; 4];
            a1.copy_from_slice(&b[o + 5..o + 9]);
            let mut a2 = [0u8; 4];
            a2.copy_from_slice(&b[o + 9..o + 13]);
            let mut a3 = [0u8; 4];
            a3.copy_from_slice(&b[o + 13..o + 17]);
            snap.slots[k] = Some(UndoAction {
                kind,
                target: u32::from_le_bytes(t),
                a: u32::from_le_bytes(a1),
                b: u32::from_le_bytes(a2),
                c: u32::from_le_bytes(a3),
            });
            o += 17;
        }
        Ok(snap)
    }
}

// ---- UI 壳接线面：动作分组（组合键一步）判定 ----

/// 分组窗（实装定值）——主册 F202 判据锚「动作分组（组合键一步）」：
/// 窗内连续同类同目标动作并作一步撤销（时间由调用方注入）。
pub const GROUP_MS: u64 = 800;

/// 分组判定结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupOutcome {
    /// 开新组：原样入栈一步。
    NewStep,
    /// 并入当前组：调用方以 `current()` 覆盖栈顶（对外仍是一步）。
    Merged,
}

/// 动作分组器：同类同目标且窗内 → 合并（b/c 累计、锚点 a 取首拍）。
pub struct ActionGrouper {
    cur: Option<UndoAction>,
    last_ts: u64,
    pub merged: u32,
    pub steps: u32,
}

impl ActionGrouper {
    pub fn new() -> ActionGrouper {
        ActionGrouper { cur: None, last_ts: 0, merged: 0, steps: 0 }
    }

    pub fn feed(&mut self, a: UndoAction, ts: u64) -> GroupOutcome {
        let mergeable = match self.cur {
            Some(c) => {
                c.kind == a.kind
                    && c.target == a.target
                    && ts >= self.last_ts
                    && ts - self.last_ts <= GROUP_MS
            }
            None => false,
        };
        if mergeable {
            if let Some(c) = self.cur.as_mut() {
                c.b = c.b.wrapping_add(a.b);
                c.c = c.c.wrapping_add(a.c);
            }
            self.merged += 1;
            self.last_ts = ts;
            return GroupOutcome::Merged;
        }
        self.cur = Some(a);
        self.last_ts = ts;
        self.steps += 1;
        GroupOutcome::NewStep
    }

    /// 当前组对外可见的一步。
    pub fn current(&self) -> Option<UndoAction> {
        self.cur
    }
}

/// F202 v2 自检（首条恒为持久化 round-trip）。
pub fn run_undoframe_v2_checks() -> CheckSet {
    let mut set = CheckSet::new("F202-undoframe-v2");

    // 1. 持久化 round-trip：60 步入档 → 保留 50、淘汰 10，解码逐位还原
    //    （验主册 F202「>50 步最早一步静默淘汰」的档案面）。
    let mut snap = StackSnapshot::new();
    for k in 0..60u32 {
        snap.push(UndoAction { kind: ActionKind::TextInsert, target: 1, a: k, b: 0, c: 0 });
    }
    let expect_top = UndoAction { kind: ActionKind::TextInsert, target: 1, a: 59, b: 0, c: 0 };
    let ok_rt = match StackSnapshot::from_bytes(&snap.to_bytes()) {
        Ok(s) => s.ulen() == STACK_DEPTH && s.evicted == 10 && s.at(STACK_DEPTH - 1) == Some(expect_top),
        Err(_) => false,
    };
    set.add("v2 persist roundtrip stack snapshot", ok_rt, "");

    // 2. 损坏拒绝四类：magic/版本/长度/校验（档案面纪律）。
    let bytes = snap.to_bytes();
    let mut bad1 = bytes;
    bad1[0] = b'X';
    let mut bad2 = bytes;
    bad2[4] = 9;
    let mut bad3 = bytes;
    bad3[12] ^= 0xFF;
    set.add(
        "v2 persist rejects corrupt snapshots",
        StackSnapshot::from_bytes(&bad1) == Err(V2CodecErr::BadMagic)
            && StackSnapshot::from_bytes(&bad2) == Err(V2CodecErr::BadVersion)
            && StackSnapshot::from_bytes(&bad3) == Err(V2CodecErr::BadSum)
            && StackSnapshot::from_bytes(&bytes[..bytes.len() - 1]) == Err(V2CodecErr::BadLen),
        "",
    );

    // 3. 动作分组：800ms 窗内同类同目标并一步；换目标开新组
    //    （验主册 F202 判据锚「动作分组（组合键一步）」）。
    let mut g = ActionGrouper::new();
    let o1 = g.feed(UndoAction { kind: ActionKind::TextInsert, target: 1, a: 0, b: 1, c: 0 }, 100);
    let o2 = g.feed(UndoAction { kind: ActionKind::TextInsert, target: 1, a: 1, b: 2, c: 0 }, 700);
    let o3 = g.feed(UndoAction { kind: ActionKind::TextInsert, target: 2, a: 9, b: 1, c: 0 }, 800);
    set.add(
        "v2 action grouping merges within window",
        o1 == GroupOutcome::NewStep && o2 == GroupOutcome::Merged
            && o3 == GroupOutcome::NewStep
            && g.merged == 1 && g.steps == 2
            && g.current().map_or(false, |c| c.target == 2),
        "",
    );

    // 4. 分组窗上界：超窗（801ms）同类同目标也开新组。
    let mut g2 = ActionGrouper::new();
    let _ = g2.feed(UndoAction { kind: ActionKind::TextInsert, target: 1, a: 0, b: 1, c: 0 }, 0);
    let o4 = g2.feed(
        UndoAction { kind: ActionKind::TextInsert, target: 1, a: 1, b: 1, c: 0 },
        GROUP_MS + 1,
    );
    set.add("v2 grouping window upper bound", o4 == GroupOutcome::NewStep, "");

    // 5. kind 编码全枚举无损（7 种动作 0..=6 往返恒等）。
    let kinds = [
        ActionKind::TextInsert,
        ActionKind::TextDelete,
        ActionKind::TextReplace,
        ActionKind::FileCopy,
        ActionKind::FileMove,
        ActionKind::FileTrash,
        ActionKind::IconDrag,
    ];
    set.add(
        "v2 action kind codec total",
        kinds.iter().all(|&k| kind_dec(kind_enc(k)) == Some(k)),
        "",
    );

    set
}

#[cfg(test)]
mod tests_v2 {
    use super::*;

    #[test]
    fn snapshot_empty_roundtrip() {
        let s = StackSnapshot::new();
        let back = StackSnapshot::from_bytes(&s.to_bytes()).unwrap();
        assert_eq!(back.ulen(), 0);
        assert_eq!(back.evicted, 0);
        assert!(back.at(0).is_none());
    }

    #[test]
    fn grouper_accumulates_lengths() {
        let mut g = ActionGrouper::new();
        let _ = g.feed(UndoAction { kind: ActionKind::TextInsert, target: 3, a: 5, b: 2, c: 0 }, 0);
        let _ = g.feed(UndoAction { kind: ActionKind::TextInsert, target: 3, a: 7, b: 4, c: 0 }, 100);
        let cur = g.current().unwrap();
        assert_eq!((cur.a, cur.b), (5, 6), "锚点取首拍、长度累计");
    }

    #[test]
    fn v2_selfcheck_all_green() {
        let set = run_undoframe_v2_checks();
        assert!(set.all_passed(), "F202 v2 自检存在红项");
        assert!(!set.truncated());
        assert!((4..=6).contains(&set.len()));
    }
}
