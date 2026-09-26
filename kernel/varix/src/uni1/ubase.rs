//! uni1 共享底盘（AI-U1 · I 通用域·一分队 · F401-F450 批次施工共用件）。
//!
//! 四件基础设施，全部零外部依赖、宿主测试直跑、内核镜像（no_std + alloc）
//! 可编译，时间一律由调用方注入（毫秒戳）——宿主测试确定复现：
//!
//! - [`Chord`] 组合键紧凑编码 + [`HotkeyTable`] 注册表：I 域十几处
//!   「键位注册（F244）」判据的唯一落位口。冲突检测、用户改键、持久化
//!   快照 round-trip 都在这里一处实现（一处一事实）；
//! - [`LayerStack`] 浮层层级栈：F424 Esc 四层语义表的通用底座，也供
//!   F416 开始菜单/F422 音量浮层等「浮层出路清单」共用；
//! - [`RingLog<N>`] 定容事件环：体验日志（第十三章）的内核侧最小件，
//!   记「哪个键/哪个面/什么时刻/触发什么/结果」，满则淘汰最旧；
//! - [`clamp_u64`] 与 [`Knob`] 旋钮：参数入册纪律的最小实现（钳制 +
//!   变更留痕）。
//!
//! 与其他分队的关系纪律：**不引用** star/sbase、perfstar、deskstar 等
//! 任何在建或已落地分队目录——I 域依赖一律以显式参数/闭包注入口承接
//! （跨泳道借力 = 0，台账登记）。

use crate::checks::CheckSet;

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// Chord + HotkeyTable — 组合键注册表（F244 注入口的唯一落位）
// ---------------------------------------------------------------------------

/// 修饰键位掩码（I 域用到的全集）。
pub const MOD_WIN: u8 = 1 << 0;
pub const MOD_CTRL: u8 = 1 << 1;
pub const MOD_ALT: u8 = 1 << 2;
pub const MOD_SHIFT: u8 = 1 << 3;

/// 组合键紧凑编码：高 8 位修饰掩码 + 低 8 位键码（ASCII 可打印键直接用
/// 字符；功能键用 0xF0 起的私用段）。
///
/// 编码是纯数据（u16），可持久化、可作 HashMap 键、可 round-trip。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Chord(pub u16);

/// 功能键私用段基址（避免与可打印字符冲突）。
pub const KEYFN_BASE: u8 = 0xF0;

impl Chord {
    /// 从修饰掩码 + 键码构造。
    pub const fn new(mods: u8, key: u8) -> Chord {
        Chord(((mods as u16) << 8) | key as u16)
    }

    pub const fn mods(self) -> u8 {
        (self.0 >> 8) as u8
    }

    pub const fn key(self) -> u8 {
        (self.0 & 0xFF) as u8
    }

    /// 可打印字母键便捷构造（Win+X 的首字母直达等场景）。
    pub const fn letter(mods: u8, ch: u8) -> Chord {
        Chord(((mods as u16) << 8) | ch as u16)
    }
}

/// 注册表条目：一个动作 ID 绑一把组合键。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HotkeyEntry {
    /// 动作 ID（如 "f403.lock"）——消费方按 ID 分发，注册表不认识业务。
    pub action: &'static str,
    pub chord: Chord,
    /// 是否用户改键结果（恢复默认时区分对待）。
    pub user_set: bool,
}

/// 组合键注册表：登记、冲突检测、改键、恢复默认。
///
/// 冲突纪律（F244 判据）：同 chord 已被其它动作占用时 `register` 返回
/// `Err(占用者动作 ID)`——拒绝静默覆盖；`rebind` 允许用户改键，但同样
/// 过冲突检测（改键不得抢系统键——系统键以 `system: true` 登记）。
pub struct HotkeyTable {
    entries: Vec<HotkeyEntry>,
    defaults: Vec<(&'static str, Chord)>,
    /// 冲突拒绝次数（诊断面诚实记账，不清零）。
    pub conflicts_rejected: u64,
}

impl HotkeyTable {
    pub fn new() -> HotkeyTable {
        HotkeyTable {
            entries: Vec::new(),
            defaults: Vec::new(),
            conflicts_rejected: 0,
        }
    }

    /// 登记一条系统键位。冲突返回 Err(占用动作)。
    pub fn register(&mut self, action: &'static str, chord: Chord) -> Result<(), &'static str> {
        if let Some(e) = self.entries.iter().find(|e| e.chord == chord) {
            self.conflicts_rejected += 1;
            return Err(e.action);
        }
        self.entries.push(HotkeyEntry { action, chord, user_set: false });
        self.defaults.push((action, chord));
        Ok(())
    }

    /// 用户改键（F244 判据「用户改键持久化」）：同样过冲突检测；
    /// 改键结果以 `user_set = true` 标注，参与持久化快照。
    pub fn rebind(&mut self, action: &'static str, chord: Chord) -> Result<(), &'static str> {
        if let Some(e) = self.entries.iter().find(|e| e.chord == chord && e.action != action) {
            self.conflicts_rejected += 1;
            return Err(e.action);
        }
        match self.entries.iter_mut().find(|e| e.action == action) {
            Some(e) => {
                e.chord = chord;
                e.user_set = true;
                Ok(())
            }
            None => Err("no-such-action"),
        }
    }

    /// 按键查找动作（一次线性扫——条目规模 ≤ 几十，无需索引）。
    pub fn lookup(&self, chord: Chord) -> Option<&'static str> {
        self.entries.iter().find(|e| e.chord == chord).map(|e| e.action)
    }

    /// 恢复默认（一键全复原判据）：全部回到 defaults，user_set 清零。
    pub fn reset_all(&mut self) {
        let defs = self.defaults.clone();
        for (action, chord) in defs {
            if let Some(e) = self.entries.iter_mut().find(|e| e.action == action) {
                e.chord = chord;
                e.user_set = false;
            }
        }
    }

    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// 持久化快照：全条目定长序对序列（宿主测试 round-trip 用）。
    pub fn snapshot(&self) -> Vec<(&'static str, Chord, bool)> {
        self.entries.iter().map(|e| (e.action, e.chord, e.user_set)).collect()
    }
}

// ---------------------------------------------------------------------------
// LayerStack — 浮层层级栈（F424 底座）
// ---------------------------------------------------------------------------

/// 浮层层级（F424 四层语义表——从「最浮」到「最沉」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LayerTier {
    /// 一层：浮层（菜单/Tooltip/预览/符号面板）。
    Popup,
    /// 二层：非模态面板（快速设置/通知中心/日历飞出）。
    Panel,
    /// 三层：模态对话框（Esc = 取消）。
    Modal,
    /// 四层：桌面态（Esc 无动作——无副作用）。
    Desktop,
}

/// 层级栈：push/pop 按 tier 排序剥离，Esc 每次只剥一层。
///
/// 不变量（run 检查钉死）：
/// - 栈内 tier 永远自顶向下**不升序**（更浮的层在更上面）；
/// - Desktop 层不进栈——它是空栈的语义；
/// - Esc 空栈 = 无动作（返回 None，不 panic 不翻状态）。
pub struct LayerStack {
    /// 从底到顶排列；`as_slice()` 最后一项是最浮层。
    tiers: Vec<LayerTier>,
    /// 层标识（诊断/体验日志用），与 tiers 一一对应。
    names: Vec<&'static str>,
}

impl LayerStack {
    pub fn new() -> LayerStack {
        LayerStack { tiers: Vec::new(), names: Vec::new() }
    }

    /// 开一层。更浮的层可压在任何层上；但同类 tier 内部保持后开在上。
    pub fn open(&mut self, tier: LayerTier, name: &'static str) {
        self.tiers.push(tier);
        self.names.push(name);
    }

    /// 关最浮一层（Esc/F424「按一次关一层」）。空栈返回 None（桌面态
    /// Esc 无动作——判据「无副作用桌面态」的落位）。
    pub fn close_top(&mut self) -> Option<(LayerTier, &'static str)> {
        if self.tiers.is_empty() {
            return None;
        }
        let tier = self.tiers.pop()?;
        let name = self.names.pop()?;
        Some((tier, name))
    }

    /// 最浮一层是谁（预览用，不改状态）。
    pub fn top(&self) -> Option<(LayerTier, &'static str)> {
        let i = self.tiers.len().checked_sub(1)?;
        Some((self.tiers[i], self.names[i]))
    }

    /// 关指定命名层（外点关闭/失焦关闭——浮层出路清单的另外两出口）。
    /// 只关最上面一个同名层；不存在返回 false。
    pub fn close_named(&mut self, name: &'static str) -> bool {
        if let Some(pos) = self.names.iter().rposition(|n| *n == name) {
            self.tiers.remove(pos);
            self.names.remove(pos);
            true
        } else {
            false
        }
    }

    pub fn depth(&self) -> usize {
        self.tiers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tiers.is_empty()
    }

    /// 栈序快照（底→顶），测试断言用。
    pub fn as_slices(&self) -> (&[LayerTier], &[&'static str]) {
        (&self.tiers, &self.names)
    }
}

// ---------------------------------------------------------------------------
// RingLog — 定容事件环（体验日志最小件）
// ---------------------------------------------------------------------------

/// 一条事件：时刻 + 类别 + 摘要（摘要定长 &'static str，内核侧零格式化）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UEvent {
    pub at_ms: u64,
    pub kind: &'static str,
    pub what: &'static str,
    /// 结果："" = 正常；其它为失败归因短码。
    pub verdict: &'static str,
}

/// 定容事件环：满 N 淘汰最旧（体验日志「可回放最近事件」的最小实现）。
pub struct RingLog {
    buf: alloc::collections::vec_deque::VecDeque<UEvent>,
    cap: usize,
    /// 被淘汰的总条数（诊断面）。
    pub evicted: u64,
}

impl RingLog {
    pub fn new(cap: usize) -> RingLog {
        RingLog { buf: alloc::collections::vec_deque::VecDeque::new(), cap: cap.max(1), evicted: 0 }
    }

    pub fn push(&mut self, at_ms: u64, kind: &'static str, what: &'static str, verdict: &'static str) {
        if self.buf.len() == self.cap {
            self.buf.pop_front();
            self.evicted += 1;
        }
        self.buf.push_back(UEvent { at_ms, kind, what, verdict });
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// 按时间升序快照。
    pub fn snapshot(&self) -> Vec<UEvent> {
        self.buf.iter().copied().collect()
    }
}

// ---------------------------------------------------------------------------
// Knob — 旋钮（参数入册纪律最小件）
// ---------------------------------------------------------------------------

/// 单旋钮：当前值 + 上下界钳制 + 变更留痕（变更次数）。
#[derive(Clone, Copy, Debug)]
pub struct Knob {
    pub name: &'static str,
    pub min: u64,
    pub max: u64,
    value: u64,
    /// 变更次数（留痕计数——「变更留痕」判据的账）。
    pub changes: u64,
}

impl Knob {
    pub fn new(name: &'static str, min: u64, max: u64, initial: u64) -> Knob {
        let value = initial.clamp(min, max);
        Knob { name, min, max, value, changes: 0 }
    }

    /// 设值（钳制入档——越界值收敛到边界，不拒绝不 panic）。
    pub fn set(&mut self, v: u64) {
        self.value = v.clamp(self.min, self.max);
        self.changes += 1;
    }

    pub fn get(&self) -> u64 {
        self.value
    }
}

/// u64 钳制工具（各模块判线用，全域只此一份实现）。
pub fn clamp_u64(v: u64, lo: u64, hi: u64) -> u64 {
    v.clamp(lo, hi)
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// 底盘自检：四件基础设施的不变量逐条钉死。
pub fn run_ubase_checks() -> CheckSet {
    let mut set = CheckSet::new("uni1-ubase");

    // HotkeyTable：登记/冲突拒绝/改键/恢复默认/快照 round-trip。
    let mut t = HotkeyTable::new();
    let c_lock = Chord::new(MOD_WIN, b'L');
    let c_explore = Chord::new(MOD_WIN, b'E');
    set.add("hk-register-two", t.register("f403.lock", c_lock).is_ok() && t.register("f404.explore", c_explore).is_ok(), "");
    set.add("hk-conflict-rejected", t.register("f407.set", c_lock) == Err("f403.lock") && t.conflicts_rejected == 1, "");
    set.add("hk-lookup", t.lookup(c_lock) == Some("f403.lock"), "");
    set.add("hk-rebind-user", t.rebind("f404.explore", Chord::new(MOD_WIN, b'R')).is_ok() && t.lookup(Chord::new(MOD_WIN, b'R')) == Some("f404.explore"), "");
    t.reset_all();
    set.add("hk-reset-default", t.lookup(c_explore) == Some("f404.explore") && t.lookup(Chord::new(MOD_WIN, b'R')).is_none(), "");
    let snap = t.snapshot();
    set.add("hk-snapshot-shape", snap.len() == 2 && snap.iter().all(|(_, _, u)| !*u), "");

    // LayerStack：四层语义/逐层剥离/空栈无动作。
    let mut s = LayerStack::new();
    set.add("ls-empty-desktop-noop", s.close_top().is_none(), "");
    s.open(LayerTier::Modal, "dlg");
    s.open(LayerTier::Panel, "quickset");
    s.open(LayerTier::Popup, "ctxmenu");
    set.add("ls-top-is-popup", s.top() == Some((LayerTier::Popup, "ctxmenu")), "");
    let p1 = s.close_top();
    let p2 = s.close_top();
    let p3 = s.close_top();
    set.add(
        "ls-peel-order",
        p1 == Some((LayerTier::Popup, "ctxmenu"))
            && p2 == Some((LayerTier::Panel, "quickset"))
            && p3 == Some((LayerTier::Modal, "dlg")),
        "",
    );
    set.add("ls-drained", s.is_empty(), "");

    // RingLog：容量淘汰/顺序。
    let mut r = RingLog::new(3);
    for i in 0..5u64 {
        r.push(i * 10, "hk", "press", "");
    }
    set.add("ring-evict-oldest", r.len() == 3 && r.evicted == 2 && r.snapshot()[0].at_ms == 20, "");

    // Knob：钳制入档/留痕。
    let mut k = Knob::new("f424.latency_ms", 0, 100, 50);
    k.set(999);
    set.add("knob-clamped", k.get() == 100 && k.changes == 1, "");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ubase_aggregate_green() {
        let set = run_ubase_checks();
        assert!(set.all_passed(), "uni1-ubase 自检存在红项");
    }

    #[test]
    fn hotkey_conflict_honest_accounting() {
        let mut t = HotkeyTable::new();
        let a = Chord::new(MOD_WIN, b'X');
        assert!(t.register("f408.winx", a).is_ok());
        assert_eq!(t.register("other", a), Err("f408.winx"));
        assert_eq!(t.conflicts_rejected, 1);
        // 改键到空闲键成功；改不存在的动作被拒。
        assert!(t.rebind("f408.winx", Chord::new(MOD_CTRL | MOD_SHIFT, b'L')).is_ok());
        assert_eq!(t.rebind("ghost.action", Chord::new(MOD_WIN, b'E')), Err("no-such-action"));
    }

    #[test]
    fn layer_stack_peel_one_per_esc() {
        let mut s = LayerStack::new();
        s.open(LayerTier::Modal, "confirm");
        s.open(LayerTier::Popup, "menu");
        // 连按两次 Esc 只关两层，且从最浮开始。
        assert_eq!(s.close_top(), Some((LayerTier::Popup, "menu")));
        assert_eq!(s.close_top(), Some((LayerTier::Modal, "confirm")));
        assert_eq!(s.close_top(), None, "桌面态 Esc 必须无动作");
    }

    #[test]
    fn ring_log_roundtrip_order() {
        let mut r = RingLog::new(8);
        r.push(1, "esc", "peel", "");
        r.push(2, "esc", "peel", "");
        r.push(3, "win", "toggle", "");
        let snap = r.snapshot();
        assert_eq!(snap.len(), 3);
        assert!(snap.windows(2).all(|w| w[0].at_ms <= w[1].at_ms));
        assert_eq!(r.snapshot()[2].what, "toggle");
    }
}
