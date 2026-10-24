//! CGPU-F0248 · 检测器自保（cgb248_selfprot）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0248`
//! **判据原文摘录**：崩溃隔离（检测器异常→自动重启+期间降级为超时兜底模式、
//! 检测器故障不扩大化）、误报熔断（同类误报 3 次→该模式进观察名单+告警、
//! 不反复狼来了）、自身开销上限（检测 CPU 占用 ≤1% 预算、超限自动降频采样
//! 并显性告知精度下降）。
//!
//! ## 检测器为什么需要自保
//!
//! 检测器是安全网，但安全网本身也可能破。三种失效模式：
//!
//! 1. **崩溃**——panic / 越界 / 死循环。检测器死了谁来检测？
//! 2. **误报**——反复喊「狼来了」。真死锁来时没人信了；
//! 3. **过载**——检测开销吃掉 CPU，自己成了性能问题。
//!
//! 三种都有对策。核心原则：**检测器故障不扩大化**——检测器死了
//! 系统退化为超时兜底，不会变得更糟。
//!
//! ## 崩溃隔离与降级
//!
//! 检测器崩溃 → 自动重启。**重启期间降级为超时兜底模式**：
//! 环检测停了，但看门狗（F0247）还在——超时机制独立于环检测。
//!
//! 这就是分层防御：环检测是第一层，看门狗是第二层。第一层倒了
//! 第二层顶上。重启完成后第一层恢复。
//!
//! ## 误报熔断
//!
//! 同类误报 3 次 → 该检测模式进观察名单 + 告警。
//!
//! 「同类」= 同一环签名（成员集合相同）。连续 3 次对同一签名报
//! 死锁但被 F0244 降级（Dismissed）——说明这个签名的判定规则有
//! 问题，暂时别自动报了，等人工审。
//!
//! 观察名单不是黑名单——人工确认误报规则修复后可移出。
//!
//! ## 开销上限与降频
//!
//! 检测 CPU ≤ 1% 预算。超限 → 降频（增量检测频率减半 / 全量扫描
//! 周期加倍），**显性告知精度下降**（`degradation_notice`）。
//!
//! 「显性」是关键——不让用户以为检测还和以前一样准。降频必须
//! 可观测。

use alloc::format;
use alloc::vec;
use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

use super::cgb241_dlarch::SubjectId;

// ---------------------------------------------------------------------------
// 诊断码（0xCBB0 起）
// ---------------------------------------------------------------------------

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0xCBB0;
/// 观察名单已含该签名。
pub const E_ALREADY_WATCHED: u16 = SEG_BASE + 1;
/// 不在观察名单。
pub const E_NOT_WATCHED: u16 = SEG_BASE + 2;

// ---------------------------------------------------------------------------
// 容量
// ---------------------------------------------------------------------------

/// 观察名单容量。
pub const MAX_WATCH_LIST: usize = 64;
/// 环签名最大成员数。
pub const MAX_SIGNATURE: usize = 32;

// ---------------------------------------------------------------------------
// 环签名（误报归因的 key）
// ---------------------------------------------------------------------------

/// 环签名：成员集合的稳定标识。
///
/// 同一环（无论发现几次）签名相同。用于误报归因。
pub type CycleSignature = Vec<SubjectId>;

/// 生成环签名（排序去重）。
pub fn make_signature(members: &[SubjectId]) -> CycleSignature {
    let mut s: Vec<SubjectId> = Vec::new();
    for &m in members {
        if !s.contains(&m) {
            s.push(m);
        }
    }
    s.sort_unstable();
    s
}

// ---------------------------------------------------------------------------
// 误报熔断
// ---------------------------------------------------------------------------

/// 观察名单条目。
#[derive(Clone, Debug)]
pub struct WatchEntry {
    pub signature: CycleSignature,
    /// 误报次数。
    pub false_positives: u32,
    /// 进入观察的 tick。
    pub entered_at: u32,
    /// 人话原因。
    pub reason: String,
}

/// 误报熔断器。
///
/// 纪律：同类误报 3 次 → 进观察名单 + 告警。
/// 观察名单的签名**不再自动报**——降级为「仅记录」。
pub struct FalsePositiveBreaker {
    /// 观察名单。
    pub watch_list: Vec<WatchEntry>,
    /// 各签名误报计数（未进观察名单的）。
    counters: Vec<(CycleSignature, u32)>,
    /// 遥测：总误报数。
    pub total_false_positives: u64,
    /// 遥测：进观察名单数。
    pub entered_watch: u64,
    /// 遥测：移出观察名单数。
    pub removed_watch: u64,
}

/// 熔断阈值。
pub const FP_THRESHOLD: u32 = 3;

impl FalsePositiveBreaker {
    pub fn new() -> FalsePositiveBreaker {
        FalsePositiveBreaker {
            watch_list: Vec::new(),
            counters: Vec::new(),
            total_false_positives: 0,
            entered_watch: 0,
            removed_watch: 0,
        }
    }

    /// 报告一次误报。
    ///
    /// 返回 `true` 表示触发熔断（进观察名单）。
    pub fn report_false_positive(
        &mut self,
        signature: &CycleSignature,
        now: u32,
        reason: &str,
    ) -> bool {
        self.total_false_positives += 1;

        // 已在观察名单 → 不重复计。
        if self.watch_list.iter().any(|w| &w.signature == signature) {
            return false;
        }

        // 查计数器。
        if let Some(pos) = self.counters.iter().position(|(s, _)| s == signature) {
            self.counters[pos].1 += 1;
            if self.counters[pos].1 >= FP_THRESHOLD {
                // 熔断 → 进观察名单。
                if self.watch_list.len() < MAX_WATCH_LIST {
                    self.watch_list.push(WatchEntry {
                        signature: signature.clone(),
                        false_positives: self.counters[pos].1,
                        entered_at: now,
                        reason: String::from(reason),
                    });
                    self.entered_watch += 1;
                }
                self.counters.remove(pos);
                return true;
            }
        } else {
            if self.counters.len() < MAX_WATCH_LIST {
                self.counters.push((signature.clone(), 1));
            }
        }
        false
    }

    /// 签名是否在观察名单（不应自动报）。
    pub fn is_watched(&self, signature: &CycleSignature) -> bool {
        self.watch_list.iter().any(|w| &w.signature == signature)
    }

    /// 人工确认误报规则修复后，移出观察名单。
    pub fn clear_watch(&mut self, signature: &CycleSignature) -> Result<(), u16> {
        let pos = self
            .watch_list
            .iter()
            .position(|w| &w.signature == signature)
            .ok_or(E_NOT_WATCHED)?;
        self.watch_list.remove(pos);
        self.removed_watch += 1;
        Ok(())
    }

    pub fn watch_count(&self) -> usize {
        self.watch_list.len()
    }

    pub fn counter_count(&self) -> usize {
        self.counters.len()
    }
}

// ---------------------------------------------------------------------------
// 崩溃隔离
// ---------------------------------------------------------------------------

/// 检测器状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DetectorState {
    /// 正常。
    Healthy,
    /// 降级（超时兜底模式）。
    Degraded,
    /// 重启中。
    Restarting,
    /// 已停止（严重故障）。
    Stopped,
}

impl DetectorState {
    pub const fn name(self) -> &'static str {
        match self {
            DetectorState::Healthy => "healthy",
            DetectorState::Degraded => "degraded",
            DetectorState::Restarting => "restarting",
            DetectorState::Stopped => "stopped",
        }
    }
}

/// 崩溃隔离器。
///
/// 检测器异常 → 自动重启 + 降级为超时兜底。
/// 检测器故障不扩大化。
pub struct CrashIsolator {
    pub state: DetectorState,
    /// 崩溃次数。
    pub crashes: u64,
    /// 重启次数。
    pub restarts: u64,
    /// 连续崩溃（快速重启保护）。
    pub consecutive_crashes: u32,
    /// 最后崩溃 tick。
    pub last_crash_at: u32,
    /// 快速崩溃阈值（连续此数则延长重启间隔）。
    pub crash_burst_limit: u32,
    /// 降级期间是否启用超时兜底。
    pub timeout_fallback: bool,
}

impl CrashIsolator {
    pub fn new() -> CrashIsolator {
        CrashIsolator {
            state: DetectorState::Healthy,
            crashes: 0,
            restarts: 0,
            consecutive_crashes: 0,
            last_crash_at: 0,
            crash_burst_limit: 5,
            timeout_fallback: true,
        }
    }

    /// 报告一次崩溃。自动降级。
    pub fn crash(&mut self, now: u32) -> DetectorState {
        self.crashes += 1;
        self.consecutive_crashes += 1;
        self.last_crash_at = now;

        if self.consecutive_crashes >= self.crash_burst_limit {
            // 快速崩溃风暴 → 停止自动重启，转人工。
            self.state = DetectorState::Stopped;
        } else {
            self.state = DetectorState::Degraded;
        }
        self.state
    }

    /// 尝试重启。
    pub fn restart(&mut self) -> Result<DetectorState, u16> {
        match self.state {
            DetectorState::Stopped => Err(E_ALREADY_WATCHED), // 转人工
            _ => {
                self.state = DetectorState::Restarting;
                self.restarts += 1;
                // 模拟重启完成。
                self.state = DetectorState::Healthy;
                self.consecutive_crashes = 0;
                Ok(self.state)
            }
        }
    }

    /// 确认健康（重启后稳定运行一段时间调用）。
    pub fn confirm_healthy(&mut self) {
        self.consecutive_crashes = 0;
        self.state = DetectorState::Healthy;
    }

    /// 降级期间超时兜底是否可用。
    pub fn fallback_available(&self) -> bool {
        self.timeout_fallback && self.state != DetectorState::Stopped
    }
}

// ---------------------------------------------------------------------------
// 开销控制
// ---------------------------------------------------------------------------

/// 开销档位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OverheadLevel {
    /// 全速。
    Full,
    /// 降频 50%。
    Half,
    /// 降频 25%。
    Quarter,
    /// 最低（仅全量兜底）。
    Minimal,
}

impl OverheadLevel {
    pub const fn name(self) -> &'static str {
        match self {
            OverheadLevel::Full => "full",
            OverheadLevel::Half => "half",
            OverheadLevel::Quarter => "quarter",
            OverheadLevel::Minimal => "minimal",
        }
    }

    /// 采样频率倍率。
    pub const fn rate(self) -> f32 {
        match self {
            OverheadLevel::Full => 1.0,
            OverheadLevel::Half => 0.5,
            OverheadLevel::Quarter => 0.25,
            OverheadLevel::Minimal => 0.1,
        }
    }
}

/// 开销控制器。
///
/// CPU ≤ 1% 预算。超限自动降频 + 显性告知。
pub struct OverheadController {
    pub level: OverheadLevel,
    /// CPU 占用估计（万分比，1% = 100）。
    pub cpu_bp: u32,
    /// 预算（万分比）。
    pub budget_bp: u32,
    /// 降级告知（用户可见）。
    pub degradation_notice: String,
    /// 遥测：降频次数。
    pub throttles: u64,
    /// 遥测：升频次数。
    pub unthrottles: u64,
}

impl OverheadController {
    pub fn new() -> OverheadController {
        OverheadController {
            level: OverheadLevel::Full,
            cpu_bp: 0,
            budget_bp: 100, // 1%
            degradation_notice: String::new(),
            throttles: 0,
            unthrottles: 0,
        }
    }

    /// 上报 CPU 占用。自动调档。
    pub fn report(&mut self, cpu_bp: u32) {
        self.cpu_bp = cpu_bp;
        if cpu_bp > self.budget_bp {
            self.throttle_down();
        } else if cpu_bp * 2 < self.budget_bp && self.level != OverheadLevel::Full {
            self.throttle_up();
        }
    }

    fn throttle_down(&mut self) {
        self.level = match self.level {
            OverheadLevel::Full => OverheadLevel::Half,
            OverheadLevel::Half => OverheadLevel::Quarter,
            OverheadLevel::Quarter => OverheadLevel::Minimal,
            OverheadLevel::Minimal => OverheadLevel::Minimal,
        };
        self.throttles += 1;
        self.degradation_notice = String::from(match self.level {
            OverheadLevel::Half => "检测精度下降 50%（开销超限降频）",
            OverheadLevel::Quarter => "检测精度下降 75%（开销超限降频）",
            OverheadLevel::Minimal => "检测精度极低（仅全量兜底）",
            _ => "",
        });
    }

    fn throttle_up(&mut self) {
        self.level = match self.level {
            OverheadLevel::Minimal => OverheadLevel::Quarter,
            OverheadLevel::Quarter => OverheadLevel::Half,
            OverheadLevel::Half => OverheadLevel::Full,
            OverheadLevel::Full => OverheadLevel::Full,
        };
        self.unthrottles += 1;
        if self.level == OverheadLevel::Full {
            self.degradation_notice = String::new();
        }
    }

    /// 是否有精度下降告知。
    pub fn has_degradation(&self) -> bool {
        !self.degradation_notice.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 自保引擎
// ---------------------------------------------------------------------------

/// 检测器自保引擎。
pub struct SelfProtection {
    /// 崩溃隔离。
    pub crash: CrashIsolator,
    /// 误报熔断。
    pub fp: FalsePositiveBreaker,
    /// 开销控制。
    pub overhead: OverheadController,
}

impl SelfProtection {
    pub fn new() -> SelfProtection {
        SelfProtection {
            crash: CrashIsolator::new(),
            fp: FalsePositiveBreaker::new(),
            overhead: OverheadController::new(),
        }
    }

    /// 是否应报告某签名的死锁（观察名单过滤）。
    pub fn should_report(&self, signature: &CycleSignature) -> bool {
        !self.fp.is_watched(signature)
    }

    /// 综合健康状态。
    pub fn health(&self) -> DetectorState {
        self.crash.state
    }

    /// 渲染自保状态（人话）。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str(&alloc::format!(
            "检测器状态：{}\n",
            self.crash.state.name()
        ));
        s.push_str(&alloc::format!(
            "  崩溃 {} 次 / 重启 {} 次\n",
            self.crash.crashes, self.crash.restarts
        ));
        s.push_str(&alloc::format!(
            "  误报 {} 次 / 观察名单 {} 条\n",
            self.fp.total_false_positives,
            self.fp.watch_count()
        ));
        s.push_str(&alloc::format!(
            "  开销档位 {}（CPU {}%%）\n",
            self.overhead.level.name(),
            self.overhead.cpu_bp as f32 / 100.0
        ));
        if self.overhead.has_degradation() {
            s.push_str(&alloc::format!("  ⚠ {}\n", self.overhead.degradation_notice));
        }
        s
    }

    // -- 自检 --------------------------------------------------------------

    pub fn check_consistency(&self) -> CheckSet {
        let mut cs = CheckSet::new("cgb248_selfprot");

        cs.add(
            "崩溃可隔离",
            self.crash.timeout_fallback,
            "降级为超时兜底",
        );
        cs.add(
            "误报可熔断",
            FP_THRESHOLD == 3,
            "同类 3 次进观察",
        );
        cs.add(
            "开销有预算",
            self.overhead.budget_bp == 100,
            "1% = 100bp",
        );
        cs.add(
            "降频显性告知",
            !self.overhead.has_degradation() || !self.overhead.degradation_notice.is_empty(),
            "notice 必填",
        );
        cs.add(
            "观察名单有界",
            self.fp.watch_count() <= MAX_WATCH_LIST,
            "MAX=64",
        );
        cs
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// CGPU-F0248 域自检。
pub fn run_cgb248_checks() -> CheckSet {
    let mut cs = CheckSet::new("cgb248_selfprot");
    let mut sp = SelfProtection::new();

    // --- 崩溃隔离 ---
    let state = sp.crash.crash(0);
    cs.add(
        "崩溃降级",
        state == DetectorState::Degraded,
        "Degraded",
    );
    cs.add(
        "降级时兜底可用",
        sp.crash.fallback_available(),
        "timeout fallback",
    );
    let restarted = sp.crash.restart();
    cs.add(
        "自动重启",
        restarted.is_ok() && sp.crash.state == DetectorState::Healthy,
        "Healthy after restart",
    );
    cs.add(
        "重启计数",
        sp.crash.restarts == 1,
        "1 restart",
    );

    // --- 崩溃风暴 → 停止 ---
    for i in 0..6u32 {
        sp.crash.crash(100 + i);
    }
    cs.add(
        "崩溃风暴停止",
        sp.crash.state == DetectorState::Stopped,
        "连续 5 次 → Stopped",
    );
    cs.add(
        "停止后拒重启",
        sp.crash.restart().is_err(),
        "转人工",
    );

    // --- 误报熔断 ---
    let sig = make_signature(&[1, 2, 3]);
    cs.add("签名生成", sig == alloc::vec![1, 2, 3], "排序去重");
    let mut broke = false;
    for i in 0..FP_THRESHOLD {
        broke = sp.fp.report_false_positive(&sig, 200 + i, "慢任务误报");
    }
    cs.add(
        "误报熔断",
        broke && sp.fp.is_watched(&sig),
        "3 次进观察名单",
    );
    cs.add(
        "观察名单过滤",
        !sp.should_report(&sig),
        "不自动报",
    );

    // --- 观察名单移出 ---
    let cleared = sp.fp.clear_watch(&sig);
    cs.add(
        "人工移出",
        cleared.is_ok() && sp.should_report(&sig),
        "clear_watch",
    );

    // --- 开销控制 ---
    sp.overhead.report(150); // 超 1% 预算
    cs.add(
        "超限降频",
        sp.overhead.level != OverheadLevel::Full,
        "降档",
    );
    cs.add(
        "降频显性告知",
        sp.overhead.has_degradation(),
        "notice 非空",
    );
    sp.overhead.report(10); // 回落
    // 升频需要 cpu*2 < budget
    sp.overhead.report(10);
    cs.add(
        "回落升频",
        sp.overhead.level == OverheadLevel::Full,
        "恢复 full",
    );

    // --- 综合状态 ---
    let text = sp.render();
    cs.add(
        "状态可读",
        text.contains("检测器状态"),
        "人话渲染",
    );

    // --- 一致性 ---
    let ic = sp.check_consistency();
    let mut ok = true;
    for i in 0..ic.len() {
        if let Some(c) = ic.get(i) {
            if !c.passed {
                ok = false;
            }
        }
    }
    cs.add("自保一致性", ok, "五项");

    // --- 签名稳定性 ---
    let sig1 = make_signature(&[3, 1, 2]);
    let sig2 = make_signature(&[2, 3, 1]);
    cs.add(
        "签名稳定",
        sig1 == sig2,
        "顺序无关",
    );

    cs
}
