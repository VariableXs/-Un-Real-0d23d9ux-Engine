//! F049 空转清零工程 · 深化件（AI-K1 深化批次三 · G-B-09）。
//!
//! | # | 主册原文 | 本件机制 |
//! | --- | --- | --- |
//! | 1 | 【设计细节】「**事件源统一 fence**（输入/VSync 定时器仅在动画注册时 armed/IPC）」 | [`FenceSet`] 事件源栅栏（未注册动画时定时器不得 armed） |
//! | 2 | 【设计细节】「**光标闪烁类周期事件只在相关窗口可见时 armed**」 | [`CaretArming`] 光标闪烁 armed 判定（窗口不可见 = 不 armed） |
//! | 3 | 【设计细节】「**VSync 空闲时关闭定时器源**」 | [`FenceSet::vsync_armed`] 与空闲自动解除 |
//! | 4 | 【设计细节】「深睡用 **MONITOR/MWAIT 指令（实测 Y7000 支持性记档）**」 | [`DeepSleep`] 支持性记档 + 深睡进入/退出账（不支持则走软路径并标注） |
//! | 5 | 【状态与异常】「**应用挂常驻动画（进度条）→ 合成器按需唤醒（不算空转违规）**」 | [`AnimationRegistry`] 常驻动画登记（登记的动画唤醒豁免空转判据） |
//! | 6 | 【验收判据】「纯桌面静置 **60 秒**：合成器 CPU **<0.5%**、内核唤醒次数 **<10 次**，两数字同录在案」 | [`IdleAudit`] 60 秒静置窗（双数字同录，缺一不可） |
//!
//! 零堆纪律：定长数组。

use crate::checks::CheckSet;
use crate::perfstar::perfkit::{DiagSev, DiagSink};

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// 静置验收窗 60 秒（主册【验收判据】）。
pub const IDLE_WINDOW_MS: u64 = 60_000;
/// 合成器 CPU 红线 0.5%（千分 5）。
pub const CPU_REDLINE_PERMILLE: u32 = 5;
/// 内核唤醒次数红线 10 次（60 秒窗内）。
pub const WAKE_REDLINE: u32 = 10;
/// 深睡支持性记档：Y7000 实测支持 MONITOR/MWAIT。
pub const Y7000_MONITOR_MWAIT: bool = true;
/// 常驻动画登记上限。
pub const ANIM_SLOTS: usize = 16;

/// 事件源（主册「输入/VSync 定时器/IPC」三源）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// 输入事件源（常 armed——输入随时可能来）。
    Input = 0,
    /// VSync 定时器（仅动画注册时 armed）。
    Vsync = 1,
    /// IPC（常 armed——进程间随时可能来消息）。
    Ipc = 2,
    /// 光标闪烁周期定时器（仅相关窗口可见时 armed）。
    Caret = 3,
}

impl Source {
    pub const fn name(self) -> &'static str {
        match self {
            Source::Input => "输入",
            Source::Vsync => "VSync 定时器",
            Source::Ipc => "IPC",
            Source::Caret => "光标闪烁",
        }
    }
    /// 是否属于「常 armed」源（输入与 IPC 不能关——关了就会丢事件）。
    pub const fn always_armed(self) -> bool {
        matches!(self, Source::Input | Source::Ipc)
    }
}

// ---------------------------------------------------------------------------
// 1. 事件源栅栏（统一 fence）
// ---------------------------------------------------------------------------

/// 事件源栅栏：把「什么时候允许唤醒」收敛到一处判定。
///
/// 主册「事件源统一 fence」——分散在各处的 armed 判断是空转 bug 的温床：
/// 任何一个漏关的定时器都会让「静置零唤醒」判据永远不达标。
#[derive(Clone, Copy, Debug)]
pub struct FenceSet {
    armed: [bool; 4],
    /// 各源被关闭的次数（审计：系统真的关过，不是声明关过）。
    disarms: [u32; 4],
    /// 各源 armed 期间触发的唤醒次数。
    wakes: [u32; 4],
}

impl FenceSet {
    /// 初始态：常 armed 源开、可关源关（保守起步，按需开）。
    pub const fn new() -> Self {
        FenceSet { armed: [true, false, true, false], disarms: [0; 4], wakes: [0; 4] }
    }
    /// 是否 armed。
    pub fn is_armed(&self, s: Source) -> bool {
        self.armed[s as usize]
    }
    /// 开/关一个源。常 armed 源不可关（关了丢事件）——请求关闭记为拒绝。
    pub fn set(&mut self, s: Source, on: bool) -> bool {
        let i = s as usize;
        if !on && s.always_armed() {
            return false; // 拒绝关闭常 armed 源
        }
        if self.armed[i] && !on {
            self.disarms[i] += 1;
        }
        self.armed[i] = on;
        true
    }
    /// 记录一次唤醒（只有 armed 的源才可能产生唤醒——未 armed 却来唤醒
    /// = 栅栏漏了，这是真缺陷信号）。
    pub fn note_wake(&mut self, s: Source) -> bool {
        let i = s as usize;
        if !self.armed[i] {
            return false;
        }
        self.wakes[i] += 1;
        true
    }
    /// 未 armed 却上报唤醒的次数（栅栏漏检计数：应为 0）。
    pub fn stray_wakes(&self, out: &mut [u32; 4]) {
        // 由调用侧对比 note_wake 返回值累计；此处提供各源唤醒快照。
        for i in 0..4 {
            out[i] = self.wakes[i];
        }
    }
    /// 是否有任何可关源仍处于 armed（静置判据的前置：可关源全关才可能零唤醒）。
    pub fn any_optional_armed(&self) -> bool {
        self.armed[Source::Vsync as usize] || self.armed[Source::Caret as usize]
    }
    /// 关闭次数（审计面）。
    pub fn disarms(&self, s: Source) -> u32 {
        self.disarms[s as usize]
    }
    /// 唤醒次数。
    pub fn wakes(&self, s: Source) -> u32 {
        self.wakes[s as usize]
    }
}

// ---------------------------------------------------------------------------
// 2. 光标闪烁 armed 判定
// ---------------------------------------------------------------------------

/// 光标闪烁 armed 判定（主册「只在相关窗口可见时 armed」）。
///
/// 三个条件同时成立才 armed：光标可见 + 所属窗口可见 + 该窗口有焦点。
/// 任一不成立就关——这是「光标闪烁也能耗掉空转判据」的真实来源。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaretState {
    pub caret_visible: bool,
    pub window_visible: bool,
    pub window_focused: bool,
}

impl CaretState {
    pub const fn new() -> Self {
        CaretState { caret_visible: false, window_visible: false, window_focused: false }
    }
    /// 是否应 armed（三条件全真）。
    pub fn should_arm(&self) -> bool {
        self.caret_visible && self.window_visible && self.window_focused
    }
}

/// 光标闪烁 armed 管理器（含被拒原因——不 armed 也要能说清为什么）。
pub struct CaretArming {
    pub state: CaretState,
    /// armed 切换次数。
    pub toggles: u32,
    /// 因条件不足而未 armed 的采样次数。
    pub skipped: u32,
}

impl CaretArming {
    pub const fn new() -> Self {
        CaretArming { state: CaretState::new(), toggles: 0, skipped: 0 }
    }
    /// 更新状态并同步 fence。
    pub fn update(&mut self, s: CaretState, fence: &mut FenceSet) -> bool {
        let want = s.should_arm();
        let before = fence.is_armed(Source::Caret);
        fence.set(Source::Caret, want);
        self.state = s;
        if !want {
            self.skipped += 1;
        }
        if before != want {
            self.toggles += 1;
        }
        want
    }
    /// 未 armed 的原因（人话，可诊断）。
    pub fn why_not_armed(&self) -> Option<&'static str> {
        if self.state.should_arm() {
            return None;
        }
        if !self.state.caret_visible {
            return Some("光标不可见（无文本焦点或未显示插入符）");
        }
        if !self.state.window_visible {
            return Some("所属窗口不可见（最小化或被完全遮挡）");
        }
        Some("所属窗口无焦点")
    }
}

// ---------------------------------------------------------------------------
// 3. 深睡（MONITOR/MWAIT 支持性记档）
// ---------------------------------------------------------------------------

/// 深睡路径。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SleepPath {
    /// 硬件深睡（MONITOR/MWAIT）。
    MonitorMwait,
    /// 软路径兜底（等待事件而非轮询——仍然不是轮询，只是省电程度低）。
    HaltWait,
}

/// 深睡账：支持性记档 + 进入/退出计数。
///
/// 主册「深睡用 MONITOR/MWAIT 指令（**实测 Y7000 支持性记档**）」——记档是
/// 判据的一部分：不支持的机型必须走软路径并说明，不能假装深睡了。
#[derive(Clone, Copy, Debug)]
pub struct DeepSleep {
    /// 硬件是否支持（启动时探测一次；Y7000 实测支持）。
    pub supported: bool,
    pub path: SleepPath,
    pub entries: u64,
    pub exits: u64,
    /// 深睡被唤醒且唤醒源不可解释的次数（应为 0——与主域唤醒分类对账）。
    pub unexplained: u64,
}

impl DeepSleep {
    pub const fn new(supported: bool) -> Self {
        DeepSleep {
            supported,
            path: if supported { SleepPath::MonitorMwait } else { SleepPath::HaltWait },
            entries: 0,
            exits: 0,
            unexplained: 0,
        }
    }
    /// Y7000 记档构造（主册实测结论落成常量，一处一事实）。
    pub const fn y7000() -> Self {
        Self::new(Y7000_MONITOR_MWAIT)
    }
    /// 进入深睡。
    pub fn enter(&mut self) {
        self.entries += 1;
    }
    /// 退出深睡（唤醒）。
    pub fn exit(&mut self, explained: bool) {
        self.exits += 1;
        if !explained {
            self.unexplained += 1;
        }
    }
    /// 路径说明（不支持也不含糊）。
    pub fn text(&self) -> &'static str {
        match self.path {
            SleepPath::MonitorMwait => "硬件深睡（MONITOR/MWAIT，Y7000 实测支持）",
            SleepPath::HaltWait => "软路径等待（固件/CPU 不支持 MONITOR/MWAIT，已降级）",
        }
    }
}

// ---------------------------------------------------------------------------
// 4. 常驻动画登记（进度条唤醒不算空转违规）
// ---------------------------------------------------------------------------

/// 一条常驻动画登记。
#[derive(Clone, Copy, Debug)]
pub struct AnimEntry {
    pub owner_id: u32,
    /// 动画周期毫秒（进度条这类常驻动画的唤醒节奏）。
    pub period_ms: u32,
    /// 登记时刻。
    pub since_ms: u64,
}

/// 常驻动画登记簿：登记过的动画唤醒**豁免**空转判据。
///
/// 主册「应用挂常驻动画（进度条）→ 合成器按需唤醒（**不算空转违规**）」——
/// 不登记就无法区分「合成器自己在空转」和「应用真的在动」，判据会误杀正常应用。
pub struct AnimationRegistry {
    entries: [Option<AnimEntry>; ANIM_SLOTS],
    n: usize,
    /// 因容量满被拒的登记次数（零静默）。
    pub rejected: u32,
    /// 豁免计数（被豁免的唤醒次数）。
    pub exempted_wakes: u64,
}

impl AnimationRegistry {
    pub const fn new() -> Self {
        AnimationRegistry { entries: [None; ANIM_SLOTS], n: 0, rejected: 0, exempted_wakes: 0 }
    }
    /// 登记一个常驻动画。
    pub fn register(&mut self, e: AnimEntry) -> bool {
        if self.n >= ANIM_SLOTS {
            self.rejected += 1;
            return false;
        }
        // 同 owner 重复登记 = 更新周期（不占第二个槽）
        for i in 0..self.n {
            if let Some(x) = self.entries[i] {
                if x.owner_id == e.owner_id {
                    self.entries[i] = Some(e);
                    return true;
                }
            }
        }
        self.entries[self.n] = Some(e);
        self.n += 1;
        true
    }
    /// 注销。
    pub fn unregister(&mut self, owner_id: u32) -> bool {
        for i in 0..self.n {
            if let Some(x) = self.entries[i] {
                if x.owner_id == owner_id {
                    for j in i..self.n - 1 {
                        self.entries[j] = self.entries[j + 1];
                    }
                    self.entries[self.n - 1] = None;
                    self.n -= 1;
                    return true;
                }
            }
        }
        false
    }
    /// 某 owner 是否登记了常驻动画。
    pub fn has(&self, owner_id: u32) -> bool {
        self.entries.iter().any(|e| e.map(|x| x.owner_id == owner_id).unwrap_or(false))
    }
    /// 判定一次唤醒是否豁免（登记过的 owner 的唤醒不算空转违规）。
    pub fn exempt(&mut self, owner_id: u32) -> bool {
        if self.has(owner_id) {
            self.exempted_wakes += 1;
            true
        } else {
            false
        }
    }
    pub fn len(&self) -> usize {
        self.n
    }
}

// ---------------------------------------------------------------------------
// 5. 60 秒静置验收窗（两个数字同录在案）
// ---------------------------------------------------------------------------

/// 静置验收结论（主册「两数字同录在案」——缺一不可）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdleVerdict {
    /// 窗未满（不能下结论）。
    Incomplete,
    /// 两项都达标。
    Pass,
    /// CPU 超标。
    CpuOver,
    /// 唤醒次数超标。
    WakeOver,
    /// 两项都超标。
    BothOver,
}

impl IdleVerdict {
    pub fn text(&self) -> &'static str {
        match self {
            IdleVerdict::Incomplete => "静置观察窗未满 60 秒，暂不结论",
            IdleVerdict::Pass => "静置达标：合成器 CPU <0.5% 且内核唤醒 <10 次",
            IdleVerdict::CpuOver => "合成器 CPU 超过 0.5%（合成器自己在动）",
            IdleVerdict::WakeOver => "内核唤醒次数超过 10 次（有源在空转）",
            IdleVerdict::BothOver => "CPU 与唤醒次数双双超标",
        }
    }
}

/// 60 秒静置验收窗（双指标同窗同判）。
#[derive(Clone, Copy, Debug)]
pub struct IdleAudit {
    pub window_start_ms: Option<u64>,
    /// 窗内合成器 CPU 千分累计（按采样次数平均）。
    pub cpu_permille_sum: u64,
    pub cpu_samples: u32,
    /// 窗内唤醒次数（已扣除豁免）。
    pub wakes: u32,
    /// 窗内被豁免的唤醒（常驻动画）。
    pub exempted: u32,
    /// 完成的窗数。
    pub rounds: u32,
    pub last: IdleVerdict,
    /// **上一窗的两个数字**（主册「两数字同录在案」）——收窗后窗内计数会复位，
    /// 但结论必须带着它的证据留在案上，供诊断快照与监视器回读。
    pub last_cpu_permille: u32,
    pub last_wakes: u32,
    pub last_exempted: u32,
}

impl IdleAudit {
    pub const fn new() -> Self {
        IdleAudit {
            window_start_ms: None,
            cpu_permille_sum: 0,
            cpu_samples: 0,
            wakes: 0,
            exempted: 0,
            rounds: 0,
            last: IdleVerdict::Incomplete,
            last_cpu_permille: 0,
            last_wakes: 0,
            last_exempted: 0,
        }
    }
    /// 开/续窗。
    pub fn open(&mut self, now_ms: u64) {
        if self.window_start_ms.is_none() {
            self.window_start_ms = Some(now_ms);
        }
    }
    /// 喂一次 CPU 采样（千分）。
    pub fn note_cpu(&mut self, permille: u32) {
        self.cpu_permille_sum += permille as u64;
        self.cpu_samples += 1;
    }
    /// 喂一次唤醒（`exempt` = 是否属常驻动画豁免）。
    pub fn note_wake(&mut self, exempt: bool) {
        if exempt {
            self.exempted += 1;
            return;
        }
        self.wakes += 1;
    }
    /// 平均 CPU 千分（零样本返回 0——与「达标」区分靠 `cpu_samples`）。
    pub fn cpu_permille(&self) -> u32 {
        if self.cpu_samples == 0 {
            return 0;
        }
        (self.cpu_permille_sum / self.cpu_samples as u64) as u32
    }
    /// 窗是否满 60 秒。
    pub fn window_full(&self, now_ms: u64) -> bool {
        match self.window_start_ms {
            Some(s) => now_ms.saturating_sub(s) >= IDLE_WINDOW_MS,
            None => false,
        }
    }
    /// 收窗下结论（两数字同判，缺一不可）。
    pub fn close(&mut self, now_ms: u64) -> IdleVerdict {
        if !self.window_full(now_ms) {
            self.last = IdleVerdict::Incomplete;
            return self.last;
        }
        let cpu_over = self.cpu_samples == 0 || self.cpu_permille() >= CPU_REDLINE_PERMILLE;
        let wake_over = self.wakes >= WAKE_REDLINE;
        self.last = match (cpu_over, wake_over) {
            (true, true) => IdleVerdict::BothOver,
            (true, false) => IdleVerdict::CpuOver,
            (false, true) => IdleVerdict::WakeOver,
            (false, false) => IdleVerdict::Pass,
        };
        self.rounds += 1;
        // 结论带着证据留档（两数字同录在案），再复位窗内计数开下一窗
        self.last_cpu_permille = if self.cpu_samples == 0 { u32::MAX } else { self.cpu_permille() };
        self.last_wakes = self.wakes;
        self.last_exempted = self.exempted;
        self.window_start_ms = None;
        self.cpu_permille_sum = 0;
        self.cpu_samples = 0;
        self.wakes = 0;
        self.exempted = 0;
        self.last
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_checks() -> CheckSet {
    let mut cs = CheckSet::new("F049-idlezero-ext");
    // 1) 事件源分类：输入与 IPC 常 armed，VSync 与光标可关。
    cs.add(
        "always_vs_optional_sources",
        Source::Input.always_armed() && Source::Ipc.always_armed() && !Source::Vsync.always_armed() && !Source::Caret.always_armed(),
        "",
    );
    // 2) 常 armed 源不可被关（关了就丢事件——请求被拒而不是静默接受）。
    let mut f = FenceSet::new();
    let refused = f.set(Source::Input, false);
    let ok_vsync = f.set(Source::Vsync, true);
    let disarm_vsync = f.set(Source::Vsync, false);
    cs.add("fence_refuses_disarm_always", !refused && f.is_armed(Source::Input) && ok_vsync && disarm_vsync && f.disarms(Source::Vsync) == 1, "");
    // 3) 未 armed 的源上报唤醒 = 栅栏漏检（返回 false，调用侧可计数）。
    cs.add("fence_detects_stray_wake", !f.note_wake(Source::Vsync) && f.wakes(Source::Vsync) == 0, "");
    cs.add("fence_counts_armed_wake", f.note_wake(Source::Input) && f.wakes(Source::Input) == 1, "");
    // 4) 可关源全关才可能零唤醒（静置判据前置）。
    cs.add("optional_sources_all_off", !f.any_optional_armed(), "");
    // 5) 光标闪烁：三条件全真才 armed，且能说清为什么没 armed。
    let mut caret = CaretArming::new();
    let a1 = caret.update(CaretState { caret_visible: true, window_visible: false, window_focused: true }, &mut f);
    let why1 = caret.why_not_armed();
    let a2 = caret.update(CaretState { caret_visible: true, window_visible: true, window_focused: true }, &mut f);
    cs.add(
        "caret_arming_three_conditions",
        !a1 && a2 && f.is_armed(Source::Caret) && why1 == Some("所属窗口不可见（最小化或被完全遮挡）") && caret.toggles == 1,
        "",
    );
    // 光标不可见 → 不同的原因文案（原因要具体到能修）
    caret.update(CaretState { caret_visible: false, window_visible: true, window_focused: true }, &mut f);
    cs.add("caret_reason_specific", caret.why_not_armed() == Some("光标不可见（无文本焦点或未显示插入符）"), "");
    // 6) 深睡：Y7000 记档为支持；不支持的机型走软路径并标注（不假装深睡）。
    let ds = DeepSleep::y7000();
    cs.add("deep_sleep_y7000_recorded", ds.supported && ds.path == SleepPath::MonitorMwait && ds.text().contains("Y7000"), "");
    let mut ds2 = DeepSleep::new(false);
    ds2.enter();
    ds2.exit(false);
    cs.add("deep_sleep_fallback_labeled", ds2.path == SleepPath::HaltWait && ds2.text().contains("已降级") && ds2.unexplained == 1, "");
    // 7) 常驻动画登记：登记者的唤醒豁免空转判据（不误杀正常应用）。
    let mut reg = AnimationRegistry::new();
    reg.register(AnimEntry { owner_id: 5, period_ms: 100, since_ms: 0 });
    cs.add("anim_register_exempts", reg.exempt(5) && !reg.exempt(6) && reg.exempted_wakes == 1, "");
    // 同 owner 重复登记不占第二槽
    reg.register(AnimEntry { owner_id: 5, period_ms: 50, since_ms: 1 });
    cs.add("anim_reregister_updates", reg.len() == 1, "");
    cs.add("anim_unregister", reg.unregister(5) && reg.len() == 0 && !reg.has(5), "");
    // 容量满被拒（零静默）
    let mut reg2 = AnimationRegistry::new();
    for i in 0..(ANIM_SLOTS + 2) {
        reg2.register(AnimEntry { owner_id: i as u32, period_ms: 16, since_ms: 0 });
    }
    cs.add("anim_full_rejected", reg2.len() == ANIM_SLOTS && reg2.rejected == 2, "");
    // 8) 60 秒静置窗：两项达标才 Pass（两数字同录在案）。
    let mut ia = IdleAudit::new();
    ia.open(0);
    for _ in 0..60 {
        ia.note_cpu(2); // 0.2% < 0.5%
    }
    for _ in 0..9 {
        ia.note_wake(false); // 9 < 10
    }
    let v_pass = ia.close(60_000);
    cs.add(
        "idle_60s_pass_both_numbers_on_record",
        v_pass == IdleVerdict::Pass && ia.rounds == 1 && ia.last_cpu_permille == 2 && ia.last_wakes == 9,
        "",
    );
    // 9) 窗未满不下结论（不拿半窗数据冒充结论）。
    let mut ia2 = IdleAudit::new();
    ia2.open(0);
    ia2.note_cpu(0);
    cs.add("idle_incomplete_window", ia2.close(1_000) == IdleVerdict::Incomplete && ia2.rounds == 0, "");
    // 10) CPU 超标 / 唤醒超标 / 双超标 三态各自可辨。
    let mut a = IdleAudit::new();
    a.open(0);
    for _ in 0..10 {
        a.note_cpu(9); // 0.9% > 0.5%
    }
    let v1 = a.close(60_000);
    let mut b = IdleAudit::new();
    b.open(0);
    b.note_cpu(1);
    for _ in 0..12 {
        b.note_wake(false);
    }
    let v2 = b.close(60_000);
    let mut c = IdleAudit::new();
    c.open(0);
    for _ in 0..5 {
        c.note_cpu(20);
    }
    for _ in 0..20 {
        c.note_wake(false);
    }
    let v3 = c.close(60_000);
    cs.add(
        "idle_three_failure_modes",
        v1 == IdleVerdict::CpuOver && v2 == IdleVerdict::WakeOver && v3 == IdleVerdict::BothOver && v1.text().contains("CPU") && v2.text().contains("唤醒"),
        "",
    );
    // 11) 豁免的唤醒不计入（常驻动画不算空转违规）。
    let mut d = IdleAudit::new();
    d.open(0);
    d.note_cpu(1);
    for _ in 0..30 {
        d.note_wake(true); // 全部豁免
    }
    let v4 = d.close(60_000);
    cs.add("idle_exempted_not_counted", v4 == IdleVerdict::Pass && d.last_exempted == 30 && d.last_wakes == 0, "");
    // 12) 零 CPU 样本不得判达标（没采样 ≠ 达标）。
    let mut e = IdleAudit::new();
    e.open(0);
    let v5 = e.close(60_000);
    cs.add("idle_no_cpu_sample_fails", v5 == IdleVerdict::CpuOver, "");
    // 13) 唤醒风暴报备通道（与主域 note_wake_storm → F042 同源）。
    let mut sink = DiagSink::new();
    sink.push("F049", 1, 1_000, DiagSev::Warn, 61, WAKE_REDLINE as u64, b"wake storm -> F042");
    cs.add("wake_storm_report_channel", sink.count(DiagSev::Warn) == 1, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fence_never_silently_disarms_always_on_sources() {
        let mut f = FenceSet::new();
        assert!(!f.set(Source::Input, false));
        assert!(!f.set(Source::Ipc, false));
        assert!(f.is_armed(Source::Input));
        assert!(f.is_armed(Source::Ipc));
    }

    #[test]
    fn caret_arming_reason_changes_with_condition() {
        let mut f = FenceSet::new();
        let mut c = CaretArming::new();
        c.update(CaretState { caret_visible: true, window_visible: true, window_focused: false }, &mut f);
        assert_eq!(c.why_not_armed(), Some("所属窗口无焦点"));
        c.update(CaretState { caret_visible: true, window_visible: true, window_focused: true }, &mut f);
        assert_eq!(c.why_not_armed(), None, "armed 时没有「为什么没 armed」");
    }

    #[test]
    fn deep_sleep_explained_wakes_do_not_count_as_unexplained() {
        let mut d = DeepSleep::y7000();
        d.enter();
        d.exit(true);
        assert_eq!(d.exits, 1);
        assert_eq!(d.unexplained, 0, "可解释的唤醒不该记不可解释");
    }

    #[test]
    fn idle_audit_resets_between_rounds_but_keeps_evidence() {
        let mut a = IdleAudit::new();
        a.open(0);
        for _ in 0..5 {
            a.note_cpu(4);
        }
        a.note_wake(false);
        assert_eq!(a.close(60_000), IdleVerdict::Pass);
        assert_eq!(a.cpu_samples, 0, "收窗后窗内计数复位，下一窗重新累积");
        // 但结论与证据留在案上（主册：两数字同录在案）
        assert_eq!(a.last_cpu_permille, 4);
        assert_eq!(a.last_wakes, 1);
        a.open(60_000);
        a.note_cpu(3);
        assert_eq!(a.cpu_permille(), 3, "新窗的均值不受上一窗影响");
    }

    #[test]
    fn idle_audit_no_cpu_sample_is_recorded_as_unknown_not_zero() {
        let mut a = IdleAudit::new();
        a.open(0);
        assert_eq!(a.close(60_000), IdleVerdict::CpuOver);
        assert_eq!(a.last_cpu_permille, u32::MAX, "零样本记为未知，不冒充 0%");
    }

    #[test]
    fn anim_registry_compacts_on_unregister() {
        let mut r = AnimationRegistry::new();
        r.register(AnimEntry { owner_id: 1, period_ms: 16, since_ms: 0 });
        r.register(AnimEntry { owner_id: 2, period_ms: 16, since_ms: 0 });
        r.register(AnimEntry { owner_id: 3, period_ms: 16, since_ms: 0 });
        assert!(r.unregister(2));
        assert_eq!(r.len(), 2);
        assert!(r.has(1) && r.has(3) && !r.has(2));
    }
}
