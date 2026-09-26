//! F172 引导自检可视化压缩（secstar · G-G-02）——kinfo 自检从滚代码到四枚里程碑。
//!
//! 主册判据（验收标准第一句）：
//! **自检项数与文字版一致（9/9、11/11、10/10 对拍）；失败注入 → 红闪+显示实测；日志完整率 100%。**
//!
//! 功能定义（G-G-02）：内核自检输出（kinfo：self-test 9/9、11/11、10/10 实测面）
//! 从用户视野移除：屏幕只留进度环+四枚里程碑图标（内存/进程/存储/输入逐个
//! 点亮）；全部文字进后台日志（诊断中心 F120 可查）；零代码开机条款（C-2）
//! 的引导段落地。
//!
//! 【交互设计】自检屏：中央星徽+下方四枚 48px 图标横排（点亮动画 150ms 弹性
//! F124）；进度环 64px 底部；全部自检仍完整执行（一项不少——只藏显示不藏
//! 检测）；开发态按 D 键切文字全输出（F172/F174 联调通道）。
//! 【数据与存储】自检日志照旧全量入环（F188 一环）；里程碑点亮时刻打点
//! （F053 时间线节点）。
//! 【状态与异常】某项自检失败 → 对应图标红闪+文字行显示（失败时诚实破例
//! ——出问题才给细节）；四图标全绿后才进动画（不跳检）。
//! 【设计细节】四图标点亮顺序=依赖链（F053：内存→进程→存储→输入）；失败
//! 红闪 2Hz×3 次后常亮红+底部一行原因（120ms 内响应）；进度环与真实自检
//! 进度绑定（不骗人条款同 C-2 幕二）；文字模式切换不留残影（清屏协议）；
//! 用户态永不出现 D 键提示（开发态镜像才有——F172 与 F193 安全模式互斥设计）。
//!
//! 对拍口径：三套 kinfo 套件（9/11/10 项）+ 输入探针项为第四里程碑喂数；
//! 可视化层与文字版消费同一事件流——项数一致性由「同流双计数」对拍复核。
//!
//! 零堆纪律：定长环 + 定长状态表，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（主册数值，一处一事实）
// ---------------------------------------------------------------------------

/// kinfo 三套件项数（主册：9/9、11/11、10/10 实测面）。
pub const SUITE_MEM_ITEMS: usize = 9;
pub const SUITE_PROC_ITEMS: usize = 11;
pub const SUITE_STORE_ITEMS: usize = 10;
/// 输入探针项数（第四里程碑；探针面实测值，非主册钉死数值）。
pub const SUITE_INPUT_ITEMS: usize = 6;
/// 里程碑图标 48px 横排。
pub const ICON_SIZE_PX: u32 = 48;
/// 进度环 64px 底部。
pub const RING_SIZE_PX: u32 = 64;
/// 点亮动画 150ms 弹性（F124 总谱）。
pub const LIGHT_ANIM_MS: u64 = 150;
/// 失败红闪 2Hz（周期 500ms）×3 次后常亮红。
pub const FLASH_PERIOD_MS: u64 = 500;
pub const FLASH_COUNT: usize = 3;
/// 失败响应时限 120ms（事件到首帧红闪）。
pub const FAIL_RESPONSE_MS: u64 = 120;
/// 自检日志环容量（全量启动 9+11+10+6=36 事件，128 留诊断余量）。
pub const LOG_CAP: usize = 128;

// ---------------------------------------------------------------------------
// 事件与里程碑
// ---------------------------------------------------------------------------

/// 里程碑（依赖链序 = 枚举序：内存→进程→存储→输入）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Milestone {
    Memory = 0,
    Process = 1,
    Storage = 2,
    Input = 3,
}

pub const MILESTONE_N: usize = 4;

/// 套件事件（与文字版共用同一事件流——对拍同源）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SuiteEvent {
    /// 一项通过（milestone, 项号）。
    ItemPassed(Milestone, usize),
    /// 一项失败（milestone, 项号, 原因码）。
    ItemFailed(Milestone, usize, u32),
    /// 套件收尾（预期项数核对）。
    SuiteDone(Milestone, usize),
}

/// 里程碑显示态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IconState {
    Pending,
    /// 点亮动画进行中（150ms 弹性）。
    Lighting(u64),
    Lit,
    /// 红闪中（已闪次数）。
    Flashing(u64, usize),
    /// 常亮红。
    FailedSolid,
}

/// 日志条目（事件 → 一行；定长不存字符串，存结构化字段供诊断中心渲染）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LogLine {
    pub ms: u64,
    pub milestone: u8,
    pub item: usize,
    pub passed: bool,
    pub reason: u32,
}

// ---------------------------------------------------------------------------
// 可视化器
// ---------------------------------------------------------------------------

/// 自检可视化器。引导期内核把 kinfo 套件事件喂进来；本层只管「藏显示不藏
/// 检测」——检测本体仍在套件内，这里零决策零跳检。
pub struct SelftestViz {
    /// 各里程碑预期项数（套件登记）。
    expected: [usize; MILESTONE_N],
    /// 各里程碑已完成项数（含失败）。
    executed: [usize; MILESTONE_N],
    /// 各里程碑通过项数。
    passed: [usize; MILESTONE_N],
    /// 套件是否收尾（SuiteDone 到过）。
    done: [bool; MILESTONE_N],
    /// 图标状态。
    icons: [IconState; MILESTONE_N],
    /// 原因行（失败诚实破例：底部一行）。
    fail_reason: u32,
    fail_milestone: u8,
    /// 日志环（全量入环——F188 一环）。
    log: [Option<LogLine>; LOG_CAP],
    log_head: usize,
    log_in: usize,
    log_out: usize,
    log_dropped: usize,
    /// 打点：里程碑点亮时刻（F053 时间线节点）。
    lit_at_ms: [Option<u64>; MILESTONE_N],
    /// 开发态文字模式（仅开发镜像可置位——用户态永不出现 D 键提示）。
    dev_text_mode: bool,
    d_hint_visible: bool,
    /// 清屏协议：文字模式切换后须无残影。
    screen_dirty: bool,
    /// 动画闸门（四图标全绿后才放行——不跳检）。
    anim_gate_open: bool,
}

impl SelftestViz {
    pub const fn new() -> Self {
        SelftestViz {
            expected: [SUITE_MEM_ITEMS, SUITE_PROC_ITEMS, SUITE_STORE_ITEMS, SUITE_INPUT_ITEMS],
            executed: [0; MILESTONE_N],
            passed: [0; MILESTONE_N],
            done: [false; MILESTONE_N],
            icons: [IconState::Pending; MILESTONE_N],
            fail_reason: 0,
            fail_milestone: 0,
            log: [None; LOG_CAP],
            log_head: 0,
            log_in: 0,
            log_out: 0,
            log_dropped: 0,
            lit_at_ms: [None; MILESTONE_N],
            dev_text_mode: false,
            d_hint_visible: false,
            screen_dirty: false,
            anim_gate_open: false,
        }
    }

    /// 喂一个套件事件。`ms` 为虚拟时基（引导期单调时钟）。
    pub fn feed(&mut self, ev: SuiteEvent, ms: u64) {
        // 事件 100% 入环（只藏显示不藏检测——文本层照旧全量）。
        match ev {
            SuiteEvent::ItemPassed(m, item) => {
                self.push_log(LogLine { ms, milestone: m as u8, item, passed: true, reason: 0 });
                let i = m as usize;
                self.executed[i] += 1;
                self.passed[i] += 1;
                // 依赖链前序全绿才点亮（点亮顺序=依赖链）。
                if self.passed[i] == self.expected[i] && self.predecessors_lit(i) {
                    self.icons[i] = IconState::Lighting(ms);
                    self.lit_at_ms[i] = Some(ms);
                }
            }
            SuiteEvent::ItemFailed(m, item, reason) => {
                self.push_log(LogLine { ms, milestone: m as u8, item, passed: false, reason });
                let i = m as usize;
                self.executed[i] += 1;
                // 失败响应 ≤120ms：即刻进红闪首帧（120ms 预算内）。
                self.icons[i] = IconState::Flashing(ms, 0);
                self.fail_reason = reason;
                self.fail_milestone = m as u8;
            }
            SuiteEvent::SuiteDone(m, n) => {
                self.push_log(LogLine { ms, milestone: m as u8, item: usize::MAX, passed: true, reason: 0 });
                let i = m as usize;
                self.done[i] = true;
                // 套件项数对拍锚：收尾计数必须等于登记预期（9/11/10/探针）。
                debug_assert_eq!(n, self.expected[i], "suite item count mismatch vs text version");
            }
        }
        self.maybe_open_gate();
    }

    /// 时间一拍：推进点亮动画与红闪状态机。
    pub fn tick(&mut self, ms: u64) {
        for i in 0..MILESTONE_N {
            match self.icons[i] {
                IconState::Lighting(t0) => {
                    if ms.saturating_sub(t0) >= LIGHT_ANIM_MS {
                        self.icons[i] = IconState::Lit;
                        self.maybe_open_gate();
                        // 依赖链解锁传播：前序补亮转 Lit 的当拍，排队中
                        // （项数已满但前序未亮）的后序立即进点亮动画——
                        // 「前序补亮后排队者随后亮起」（排队不吞事件）。
                        for j in (i + 1)..MILESTONE_N {
                            if self.icons[j] == IconState::Pending
                                && self.passed[j] == self.expected[j]
                                && self.expected[j] > 0
                                && self.predecessors_lit(j)
                            {
                                self.icons[j] = IconState::Lighting(ms);
                                self.lit_at_ms[j] = Some(ms);
                            }
                        }
                    }
                }
                IconState::Flashing(t0, n) => {
                    let elapsed = ms.saturating_sub(t0);
                    // 2Hz ×3 次后常亮红（3 个完整周期 = 1500ms）。
                    if elapsed >= FLASH_PERIOD_MS * FLASH_COUNT as u64 {
                        self.icons[i] = IconState::FailedSolid;
                    } else {
                        let flashes = (elapsed / (FLASH_PERIOD_MS / 2)) as usize; // 半周期计闪
                        if flashes != n {
                            self.icons[i] = IconState::Flashing(t0, flashes);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// 首帧红闪是否已在 120ms 预算内出现（失败响应时限）。
    pub fn fail_response_ok(&self, fail_ms: u64, first_flash_ms: u64) -> bool {
        // feed() 在失败事件当拍即置 Flashing —— 首帧延迟恒 0 ≤ 120ms；
        // 本钩子供渲染层对账（虚拟时基下响应延迟构造上为零）。
        first_flash_ms.saturating_sub(fail_ms) <= FAIL_RESPONSE_MS
    }

    /// 依赖链前序是否全 Lit（点亮顺序强制）。
    fn predecessors_lit(&self, i: usize) -> bool {
        (0..i).all(|p| self.icons[p] == IconState::Lit)
    }

    /// 四图标全绿 → 动画闸门开（不跳检）。
    fn maybe_open_gate(&mut self) {
        if !self.anim_gate_open
            && self.icons.iter().all(|s| *s == IconState::Lit)
            && self.done.iter().all(|d| *d)
        {
            self.anim_gate_open = true;
        }
    }

    pub fn anim_gate_open(&self) -> bool {
        self.anim_gate_open
    }

    /// 进度环绑定真实进度（已完成项/总项——不骗人条款）。
    pub fn progress_permille(&self) -> u32 {
        let total: usize = self.expected.iter().sum();
        let done: usize = self.executed.iter().sum();
        if total == 0 {
            return 0;
        }
        ((done * 1000) / total) as u32
    }

    /// 开发态文字模式（仅开发镜像调用；用户态无入口）。
    pub fn enable_dev_text(&mut self) {
        self.dev_text_mode = true;
        self.d_hint_visible = true; // 开发态镜像才可见
        self.screen_dirty = true; // 切换需清屏协议
    }

    /// 清屏协议执行（文字模式切换不留残影）。
    pub fn apply_clear_screen(&mut self) {
        self.screen_dirty = false;
    }

    pub fn dev_text_mode(&self) -> bool {
        self.dev_text_mode
    }
    /// 用户态 D 键提示恒不可见（F172 与 F193 安全模式互斥设计）。
    pub fn d_hint_visible(&self) -> bool {
        self.d_hint_visible && self.dev_text_mode
    }
    pub fn screen_dirty(&self) -> bool {
        self.screen_dirty
    }

    fn push_log(&mut self, line: LogLine) {
        if self.log_in - self.log_out >= LOG_CAP {
            // 环满：挤最旧（诊断中心未及时消费时的诚实口径——计数不撒谎）。
            self.log_out += 1;
            self.log_dropped += 1;
        }
        self.log[self.log_head] = Some(line);
        self.log_head = (self.log_head + 1) % LOG_CAP;
        self.log_in += 1;
    }

    /// 日志排空（诊断中心 F120 消费；完整率对账：in == out + dropped）。
    pub fn drain_log(&mut self, out: &mut [Option<LogLine>]) -> usize {
        let mut n = 0;
        let avail = self.log_in - self.log_out;
        for k in 0..avail.min(out.len()).min(LOG_CAP) {
            let idx = (self.log_head + LOG_CAP - avail + k) % LOG_CAP;
            out[n] = self.log[idx];
            n += 1;
        }
        self.log_out += n;
        n
    }

    pub fn log_dropped(&self) -> usize {
        self.log_dropped
    }
    pub fn log_in(&self) -> usize {
        self.log_in
    }
    pub fn log_out(&self) -> usize {
        self.log_out
    }

    pub fn icon_state(&self, m: Milestone) -> IconState {
        self.icons[m as usize]
    }
    pub fn fail_reason(&self) -> u32 {
        self.fail_reason
    }
    pub fn lit_at(&self, m: Milestone) -> Option<u64> {
        self.lit_at_ms[m as usize]
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 全量启动事件流（9/11/10/6 项全过——对拍基准）。
fn feed_full_green(v: &mut SelftestViz, ms: &mut u64) {
    let suites = [(Milestone::Memory, SUITE_MEM_ITEMS), (Milestone::Process, SUITE_PROC_ITEMS), (Milestone::Storage, SUITE_STORE_ITEMS), (Milestone::Input, SUITE_INPUT_ITEMS)];
    for (m, n) in suites {
        for item in 0..n {
            *ms += 40;
            v.feed(SuiteEvent::ItemPassed(m, item), *ms);
        }
        v.feed(SuiteEvent::SuiteDone(m, n), *ms);
        // 推进动画 150ms 让 Light→Lit。
        *ms += LIGHT_ANIM_MS;
        v.tick(*ms);
    }
}

/// 域自检。
#[inline(never)]
pub fn run_selftestviz_checks() -> CheckSet {
    let mut cs = CheckSet::new("F172-selftestviz");

    // 1) 套件项数对拍：9/9、11/11、10/10 逐项入流逐项出流（+输入探针项）。
    let mut v = SelftestViz::new();
    let mut ms = 0u64;
    feed_full_green(&mut v, &mut ms);
    let total_expected = SUITE_MEM_ITEMS + SUITE_PROC_ITEMS + SUITE_STORE_ITEMS + SUITE_INPUT_ITEMS;
    cs.add("suite_counts_9_11_10", v.progress_permille() == 1000 && total_expected == 36, "");

    // 2) 四图标全绿才开动画闸门（不跳检）。
    cs.add("gate_after_all_green", v.anim_gate_open(), "");

    // 3) 日志完整率 100%：入环 40 条（36 项+4 收尾），排空 40 条，零丢失。
    let mut sink: [Option<LogLine>; LOG_CAP] = [None; LOG_CAP];
    let n = v.drain_log(&mut sink);
    cs.add("log_complete_100pct", v.log_in() == 40 && n == 40 && v.log_dropped() == 0 && v.log_in() == v.log_out() + v.log_dropped(), "");

    // 4) 失败注入：红闪在 120ms 内响应（构造上事件当拍即闪）。
    let mut v2 = SelftestViz::new();
    let mut ms2 = 0u64;
    v2.feed(SuiteEvent::ItemPassed(Milestone::Memory, 0), ms2);
    ms2 += 40;
    v2.feed(SuiteEvent::ItemFailed(Milestone::Memory, 1, 0xE001), ms2);
    cs.add("fail_flash_within_120ms", v2.fail_response_ok(ms2, ms2) && v2.icon_state(Milestone::Memory) == IconState::Flashing(ms2, 0), "");

    // 5) 红闪 2Hz×3 次后常亮红（1500ms 转固）。
    v2.tick(ms2 + FLASH_PERIOD_MS * 3);
    cs.add("fail_solid_after_3_flashes", v2.icon_state(Milestone::Memory) == IconState::FailedSolid && v2.fail_reason() == 0xE001, "");

    // 6) 失败即闭闸：失败里程碑后续图标不点亮、闸门不开。
    cs.add("fail_blocks_gate", !v2.anim_gate_open(), "");

    // 7) 依赖链强制：前序未亮时后序套件全过也不亮（排队等前序）。
    let mut v3 = SelftestViz::new();
    let mut ms3 = 0u64;
    // 进程套件先跑完（内存还没跑）——按依赖链不得先亮。
    for item in 0..SUITE_PROC_ITEMS {
        ms3 += 10;
        v3.feed(SuiteEvent::ItemPassed(Milestone::Process, item), ms3);
    }
    cs.add(
        "dependency_order_enforced",
        v3.icon_state(Milestone::Process) == IconState::Pending && v3.icon_state(Milestone::Memory) == IconState::Pending,
        "",
    );

    // 8) 前序补亮后排队者随后亮起（依赖链解锁）。
    // 内存套件跑完 → 150ms 后 Memory 转 Lit；同拍 Process（已排队 11/11）
    // 解锁进点亮动画；再 150ms 后 Process 转 Lit——两段动画逐拍可查。
    for item in 0..SUITE_MEM_ITEMS {
        ms3 += 10;
        v3.feed(SuiteEvent::ItemPassed(Milestone::Memory, item), ms3);
    }
    ms3 += LIGHT_ANIM_MS;
    v3.tick(ms3); // Memory Lit；Process 解锁进 Lighting
    let mem_lit = v3.icon_state(Milestone::Memory) == IconState::Lit;
    ms3 += LIGHT_ANIM_MS;
    v3.tick(ms3); // Process Lit
    cs.add(
        "queued_lights_after_predecessor",
        mem_lit && v3.icon_state(Milestone::Process) == IconState::Lit,
        "",
    );

    // 9) 进度环绑定真实进度（半程 = 18/36 = 500‰——不骗人条款）。
    let mut v4 = SelftestViz::new();
    let mut ms4 = 0u64;
    for item in 0..SUITE_MEM_ITEMS {
        ms4 += 10;
        v4.feed(SuiteEvent::ItemPassed(Milestone::Memory, item), ms4);
    }
    // 进程套件跑到 9/11（总进度恰半程：9+9=18 / 36）。
    for item in 0..(SUITE_PROC_ITEMS - 2) {
        ms4 += 10;
        v4.feed(SuiteEvent::ItemPassed(Milestone::Process, item), ms4);
    }
    cs.add("progress_ring_bound_to_real", v4.progress_permille() == 500, "");

    // 10) 用户态 D 键提示恒不可见；开发态镜像才有。
    let mut v5 = SelftestViz::new();
    cs.add("d_hint_never_in_user_mode", !v5.d_hint_visible(), "");

    // 11) 文字模式切换需清屏协议（不留残影）。
    v5.enable_dev_text();
    cs.add("dev_text_needs_clear", v5.dev_text_mode() && v5.d_hint_visible() && v5.screen_dirty(), "");
    v5.apply_clear_screen();
    cs.add("clear_screen_protocol", !v5.screen_dirty(), "");

    // 12) 点亮动画 150ms：未满 150ms 不转 Lit。
    let mut v6 = SelftestViz::new();
    let mut ms6 = 0u64;
    for item in 0..SUITE_MEM_ITEMS {
        ms6 += 10;
        v6.feed(SuiteEvent::ItemPassed(Milestone::Memory, item), ms6);
    }
    let lit0 = v6.lit_at(Milestone::Memory).unwrap_or(0);
    v6.tick(lit0 + LIGHT_ANIM_MS - 1);
    let not_yet = v6.icon_state(Milestone::Memory) != IconState::Lit;
    v6.tick(lit0 + LIGHT_ANIM_MS);
    cs.add("light_anim_150ms", not_yet && v6.icon_state(Milestone::Memory) == IconState::Lit, "");

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_green_boot_passes_all_gates() {
        let mut v = SelftestViz::new();
        let mut ms = 0u64;
        feed_full_green(&mut v, &mut ms);
        assert!(v.anim_gate_open(), "四图标全绿必须开闸");
        for m in [Milestone::Memory, Milestone::Process, Milestone::Storage, Milestone::Input] {
            assert_eq!(v.icon_state(m), IconState::Lit);
        }
        // 里程碑点亮时刻打点在册（F053 时间线节点）。
        for m in [Milestone::Memory, Milestone::Process, Milestone::Storage, Milestone::Input] {
            assert!(v.lit_at(m).is_some(), "点亮时刻必须打点");
        }
    }

    #[test]
    fn failure_scenario_honest_display() {
        // 失败注入全链：失败 → 红闪 → 常亮红 + 原因行 + 闸门不开。
        let mut v = SelftestViz::new();
        let mut ms = 100u64;
        for item in 0..SUITE_MEM_ITEMS {
            if item == 4 {
                v.feed(SuiteEvent::ItemFailed(Milestone::Memory, item, 0xBEEF), ms);
            } else {
                v.feed(SuiteEvent::ItemPassed(Milestone::Memory, item), ms);
            }
            ms += 40;
        }
        v.feed(SuiteEvent::SuiteDone(Milestone::Memory, SUITE_MEM_ITEMS), ms);
        // 红闪推进到常亮。
        ms += FLASH_PERIOD_MS * 3;
        v.tick(ms);
        assert_eq!(v.icon_state(Milestone::Memory), IconState::FailedSolid);
        assert_eq!(v.fail_reason(), 0xBEEF);
        assert!(!v.anim_gate_open());
        // 失败项也在日志里（诚实破例——出问题才给细节）。
        let mut sink: [Option<LogLine>; LOG_CAP] = [None; LOG_CAP];
        let n = v.drain_log(&mut sink);
        let failed = (0..n).filter(|i| matches!(sink[*i], Some(l) if !l.passed)).count();
        assert_eq!(failed, 1, "失败项必须 1:1 入日志");
    }

    #[test]
    fn log_completeness_under_drain_pressure() {
        // 排空压力下完整率账目恒平：in == out + dropped。
        let mut v = SelftestViz::new();
        let mut ms = 0u64;
        feed_full_green(&mut v, &mut ms);
        let mut sink: [Option<LogLine>; LOG_CAP] = [None; LOG_CAP];
        let n1 = v.drain_log(&mut sink);
        assert_eq!(n1, v.log_in());
        assert_eq!(v.log_in(), v.log_out() + v.log_dropped());
        // 二次排空：零新增（不重复不遗漏）。
        let n2 = v.drain_log(&mut sink);
        assert_eq!(n2, 0);
    }

    #[test]
    fn suite_count_mismatch_panics_in_debug() {
        // 对拍锚：收尾计数与登记预期不符必须炸（debug 断言——只藏显示不藏检测）。
        let result = std::panic::catch_unwind(|| {
            let mut v = SelftestViz::new();
            v.feed(SuiteEvent::SuiteDone(Milestone::Memory, 8), 0); // 应为 9
        });
        assert!(result.is_err(), "项数对拍失败必须显性化");
    }
}
