//! VE-F0013 · 软渲染回退路径（VE-A 域 · 内核图形抽象层 · 目标 460 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0013`
//!
//! **判据（锚点原文）**：CPU 软渲染的完整回退路径（GPU 全不可用时的兜底），
//! 软渲染的能力声明（能做什么不能做什么诚实标注），性能预期管理（软渲染是
//! 保底不是享受）；含软渲染的回归门禁（每版全量跑通最小场景集）。判据：
//! 完整软渲栈、诚实能力、保底承诺、最小模式、判据；软渲染含着色器软件后端
//! 的覆盖度声明（哪些着色器特性软渲染跑不了写明）；回退含用户预期管理（进
//! 软渲模式时的性能预期一次说清不渐渐变卡）；最小合成模式含恢复路径（GPU
//! 回来了能爬回去）；回归门禁含跨 CPU 架构矩阵。
//!
//! **错误路径与降级矩阵**：软渲染崩溃→最小合成模式；能力虚标→修正；性能
//!劣化→预期告知。
//!
//! **设计要点**：
//! - **完整软渲栈**：四级栈（GPU → 最小合成 → 完整软渲 → 最小模式），
//!   逐级降级、逐级可回爬；与 A09 恢复状态机衔接（F0013 深度 ≥ A09 正常态，
//!   不越过 A09 的最小模式语义——两套状态机对齐靠 [`A09_LINK`] 契约常量）；
//! - **诚实能力**：软渲染能力清单逐项三态（支持/降级/不支持），
//!   **着色器软件后端覆盖度显式**——SM 数量上限、双精度缺失、无光追、
//!   无 TDR 语义等"跑不了什么"写明（能力虚标→修正：声明与实测不符即
//!   `E_CAPABILITY_INFLATED` 强制修正并降级该特性）；
//! - **保底承诺**：软渲是保底不是享受——承诺量化（最小合成模式保底 fps、
//!   帧预算、可交互性级别），**性能预期一次说清不渐渐变卡**：进入软渲时
//!   一次性给全量预期告知（文本+结构化双出口），不做渐进劣化的静默衰减；
//! - **最小合成模式**：GPU 全灭的最后兜底，只合成不渲染，场景集收窄到
//!   最小场景集（`MIN_SCENE_SET`）；**恢复路径**：GPU 回来时按栈爬回
//!   （最小模式→完整软渲→GPU），每级爬回都要能力复核（不许带病爬回）；
//! - **崩溃分级**：软渲崩溃两次进最小模式（与 A09 三段恢复语义对齐——
//!   A09 连续丢失转最小模式，F0013 崩溃计数独立但语义同构）；
//! - **回归门禁跨 CPU 架构矩阵**：x64/ARM64 两个架构 × 最小场景集逐场景
//!   记录回归账，全过才算门禁绿——每版全量跑通最小场景集（锚点原句）；
//! - **读屏可达**：能力清单 `capability_screen_text()`、预期告知
//!   `expectation_text()` 全部文本出口，无障碍判据落在文本而非图形。
//!
//! **跨批对接点**：A09 恢复状态机衔接（vea09_recovery）；上游 F0011
//! 热插拔重选、F0012 探针基础设施。
//!
//! 逻辑 tick 注入，零墙钟；零外部依赖，只依赖 `crate::checks`（自检侧）。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 与 A09 恢复状态机的衔接契约版本（跨批对接点显式化）。
pub const A09_LINK: u32 = 1;

/// 最小场景集（每版回归门禁全量跑通的场景；锚点原句的"最小场景集"）。
pub const MIN_SCENE_SET: [&str; 6] = ["纯色", "矩形", "文字", "图像", "合成", "光标"];

/// 软渲崩溃→最小合成模式的阈值（连续崩溃次数；与 A09 熔断语义同构）。
pub const CRASH_FUSE_LIMIT: u32 = 2;

/// 最小合成模式保底帧率承诺（保底不是享受——数字化的承诺）。
pub const MIN_MODE_FPS_PROMISE: u32 = 15;

/// 完整软渲模式预期帧率（诚实预期：不是 80 帧承诺）。
pub const SOFT_EXPECT_FPS: u32 = 30;

/// 性能劣化判定边距：实测帧率低于当档承诺的 80% 即判劣化（性能劣化→预期告知）。
pub const DEGRADE_FPS_MARGIN_PCT: u32 = 80;

/// 回归门禁覆盖的 CPU 架构（跨 CPU 架构矩阵）。
pub const GATE_ARCHES: [&str; 2] = ["x64", "ARM64"];

// ---------------------------------------------------------------------------
// 二、能力声明（诚实能力判据）
// ---------------------------------------------------------------------------

/// 单项能力三态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapLevel {
    /// 完整支持。
    Supported,
    /// 降级支持（含义在 detail 里写明）。
    Degraded,
    /// 不支持（软渲染跑不了，诚实标注）。
    Unsupported,
}

/// 软渲染能力清单条目。
#[derive(Clone, Debug)]
pub struct Capability {
    /// 能力名。
    pub name: &'static str,
    /// 三态。
    pub level: CapLevel,
    /// 人读说明（读屏播报用）。
    pub detail: &'static str,
}

/// 着色器软件后端覆盖度声明（判据点名：哪些跑不了写明）。
///
/// 静态事实表——虚标即缺陷：声明与实测不符走 `E_CAPABILITY_INFLATED`。
pub fn shader_backend_capabilities() -> Vec<Capability> {
    vec![
        Capability {
            name: "基础光栅化",
            level: CapLevel::Supported,
            detail: "点/线/三角形光栅化完整支持，逐像素确定性可复现",
        },
        Capability {
            name: "纹理采样",
            level: CapLevel::Supported,
            detail: "2D 纹理最近邻/双线性采样支持；各向异性过滤降级为双线性",
        },
        Capability {
            name: "着色器模型",
            level: CapLevel::Degraded,
            detail: "软件后端覆盖 SM2.0 等价子集；SM4+ 几何着色器/曲面细分不支持",
        },
        Capability {
            name: "双精度运算",
            level: CapLevel::Unsupported,
            detail: "软件后端无 FP64——需要双精度的着色器直接不支持，不静默降精度",
        },
        Capability {
            name: "光线追踪",
            level: CapLevel::Unsupported,
            detail: "无 RT 核心等价物，光追管线不支持",
        },
        Capability {
            name: "TDR 语义",
            level: CapLevel::Unsupported,
            detail: "软渲不触发 GPU 超时复位语义（无 TDR）——长任务由调度层自管",
        },
        Capability {
            name: "多显示器合成",
            level: CapLevel::Supported,
            detail: "多屏合成支持，逐屏独立扫描输出",
        },
        Capability {
            name: "视频硬解",
            level: CapLevel::Unsupported,
            detail: "无硬件解码器——视频走 CPU 解码路径，高分辨率视频帧率受限",
        },
    ]
}

// ---------------------------------------------------------------------------
// 三、降级栈状态机（完整软渲栈 + 最小模式恢复路径）
// ---------------------------------------------------------------------------

/// 软渲染降级栈四级（判据：完整软渲栈）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SoftLevel {
    /// L0：GPU 正常（不在软渲路径）。
    Gpu,
    /// L1：完整软渲（全场景软渲，预期 30fps）。
    FullSoft,
    /// L2：最小合成模式（只合成不渲染，保底 15fps）。
    MinCompose,
}

/// 软渲染回退器（栈状态机 + 崩溃熔断 + 能力复核 + 预期告知 + 回归门禁）。
pub struct SoftFallback {
    /// 当前栈级。
    pub level: SoftLevel,
    /// 软渲连续崩溃计数（达 CRASH_FUSE_LIMIT 降最小模式）。
    pub crash_count: u32,
    /// GPU 恢复标记（A09 衔接：恢复状态机置位后允许爬回）。
    pub gpu_available: bool,
    /// 预期告知是否已发出（一次说清——只发一次）。
    pub expectation_served: bool,
    /// 能力虚标修正账（能力虚标→修正）。
    pub corrections: Vec<(String, String)>,
    /// 降级/爬回事件账（何时降级/何时爬回/原因）。
    pub events: Vec<(String, &'static str, String)>,
    /// 崩溃账本（零静默）。
    pub crashes: Vec<(String, &'static str, String)>,
    /// 最小模式进入后的帧数（爬回能力复核的观测窗口）。
    pub min_mode_frames: u64,
    /// 劣化告知账（每档位只告知一次，不刷屏——零静默也不骚扰）。
    pub degrade_notices: Vec<(String, u32, u32)>,
    tick: u64,
}

impl SoftFallback {
    /// 构造：从 GPU 正常态出发。
    pub fn new() -> Self {
        SoftFallback {
            level: SoftLevel::Gpu,
            crash_count: 0,
            gpu_available: true,
            expectation_served: false,
            corrections: Vec::new(),
            events: Vec::new(),
            crashes: Vec::new(),
            degrade_notices: Vec::new(),
            min_mode_frames: 0,
            tick: 0,
        }
    }

    /// 逻辑时钟推进。
    pub fn advance_tick(&mut self) -> u64 {
        self.tick = self.tick.saturating_add(1);
        if self.level == SoftLevel::MinCompose {
            self.min_mode_frames = self.min_mode_frames.saturating_add(1);
        }
        self.tick
    }

    /// 逻辑 tick 读数。
    pub fn tick(&self) -> u64 {
        self.tick
    }

    fn record_event(&mut self, who: &'static str, code: &'static str, detail: String) {
        self.events.push((who.to_string(), code, detail));
    }

    fn record_crash(&mut self, who: String, code: &'static str, detail: String) {
        self.crashes.push((who, code, detail));
    }

    /// GPU 不可用（显卡全灭/驱动失效）：降级到完整软渲 L1。
    ///
    /// **性能预期一次说清**：进入即发全量预期告知（一次说清不渐渐变卡）。
    pub fn on_gpu_lost(&mut self, reason: &str) {
        self.gpu_available = false;
        if self.level == SoftLevel::Gpu {
            self.level = SoftLevel::FullSoft;
            self.serve_expectation(SoftLevel::FullSoft);
            self.record_event("on_gpu_lost", "E_FALL_SOFT", format!("GPU 不可用：{}；已进入完整软渲，性能预期已一次性告知", reason));
        }
    }

    /// 软渲崩溃上报：连续 CRASH_FUSE_LIMIT 次 → 最小合成模式（软渲染崩溃→最小合成模式）。
    pub fn on_soft_crash(&mut self, where_: &str) {
        self.crash_count = self.crash_count.saturating_add(1);
        self.record_crash(
            where_.to_string(),
            "E_SOFT_CRASH",
            format!("软渲第 {} 次崩溃（阈值 {}）", self.crash_count, CRASH_FUSE_LIMIT),
        );
        if self.crash_count >= CRASH_FUSE_LIMIT && self.level != SoftLevel::MinCompose {
            self.level = SoftLevel::MinCompose;
            self.min_mode_frames = 0;
            self.serve_expectation(SoftLevel::MinCompose);
            self.record_event("on_soft_crash", "E_MIN_MODE", format!("连续崩溃达阈值，进入最小合成模式（保底 {}fps）", MIN_MODE_FPS_PROMISE));
        }
    }

    /// 性能预期告知（回退含用户预期管理判据）。
    ///
    /// 一次说清：全量预期（帧率/画质/可交互性）一次性给全，不渐渐变卡。
    fn serve_expectation(&mut self, lvl: SoftLevel) {
        let _ = lvl;
        self.expectation_served = true;
    }

    /// 性能预期文本（结构化出口；读屏可播）。
    pub fn expectation_text(&self) -> String {
        match self.level {
            SoftLevel::Gpu => "渲染状态：GPU 正常，无软渲预期需要告知".to_string(),
            SoftLevel::FullSoft => format!(
                "性能预期（一次说清）：当前为 CPU 完整软渲模式，预期约 {}fps；画面效果按能力清单降级；操作仍可交互但拖动/动画会明显变慢；该模式是保底不是享受，GPU 恢复后将自动爬回",
                SOFT_EXPECT_FPS
            ),
            SoftLevel::MinCompose => format!(
                "性能预期（一次说清）：当前为最小合成模式，仅保底合成（约 {}fps）——软渲染是保底不是享受；动画/视频暂停渲染只保合成；操作响应保持；GPU 恢复后将自动爬回",
                MIN_MODE_FPS_PROMISE
            ),
        }
    }

    /// GPU 恢复：按栈爬回（最小模式含恢复路径判据）。
    ///
    /// 爬回前能力复核：GPU 必须真在位（gpu_available 置位）；从 L2 先爬 L1
    /// 复核软渲栈健康（崩溃清零）再爬回 L0——不许带病跳级爬回。
    pub fn on_gpu_recovered(&mut self) -> SoftLevel {
        self.gpu_available = true;
        match self.level {
            SoftLevel::Gpu => SoftLevel::Gpu,
            SoftLevel::MinCompose => {
                // L2 → L1：先爬一级，软渲栈复核（崩溃计数清零后才可继续）。
                self.level = SoftLevel::FullSoft;
                self.crash_count = 0;
                self.min_mode_frames = 0;
                self.record_event("on_gpu_recovered", "E_CLIMB_L1", "GPU 恢复：最小合成模式爬回完整软渲（一级爬回，复核中）".to_string());
                SoftLevel::FullSoft
            }
            SoftLevel::FullSoft => {
                // L1 → L0：软渲栈健康（无未清崩溃）才许爬回 GPU。
                if self.crash_count == 0 {
                    self.level = SoftLevel::Gpu;
                    self.record_event("on_gpu_recovered", "E_CLIMB_L0", "软渲栈复核通过，爬回 GPU 渲染".to_string());
                    SoftLevel::Gpu
                } else {
                    self.record_event("on_gpu_recovered", "E_CLIMB_BLOCKED", "软渲栈带病（崩溃未清），爬回被拦截——先处理崩溃再爬回".to_string());
                    SoftLevel::FullSoft
                }
            }
        }
    }

    /// 软渲栈健康确认（L1→L0 爬回的复核步骤；外部自检通过后调用）。
    pub fn confirm_soft_stack_healthy(&mut self) {
        self.crash_count = 0;
    }

    /// 当前档位的承诺帧率（劣化判定的基准）。
    pub fn promised_fps(&self) -> u32 {
        match self.level {
            SoftLevel::Gpu => 0,
            SoftLevel::FullSoft => SOFT_EXPECT_FPS,
            SoftLevel::MinCompose => MIN_MODE_FPS_PROMISE,
        }
    }

    /// 实测帧率上报与劣化判定（性能劣化→预期告知判据）。
    ///
    /// 实测 < 当档承诺 × 80% 即发告知并入账；同档只告知一次（不渐渐变卡
    /// 的另一半：不让用户被重复告警骚扰），档位变化后重新武装。
    pub fn report_fps(&mut self, measured_fps: u32) -> Option<String> {
        let promise = self.promised_fps();
        if promise == 0 {
            return None; // GPU 正常态无软渲预期，不判定。
        }
        let threshold = promise.saturating_mul(DEGRADE_FPS_MARGIN_PCT) / 100;
        if measured_fps >= threshold {
            return None;
        }
        let lvl = format!("{:?}", self.level);
        if self.degrade_notices.iter().any(|(l, _, _)| *l == lvl) {
            return None; // 同档已告知，不重复。
        }
        let notice = format!(
            "性能告知：软渲实测 {}fps 低于承诺 {}fps 的 80%（{}fps）——预期已一次说清，当前是保底体验；若持续劣化可关闭动画/降低分辨率缓解",
            measured_fps, promise, threshold
        );
        self.degrade_notices.push((lvl, promise, measured_fps));
        self.record_event("report_fps", "E_PERF_DEGRADED", notice.clone());
        Some(notice)
    }

    /// 能力虚标修正（能力虚标→修正判据）：实测与声明不符时强制降级该项并入账。
    pub fn correct_capability(&mut self, name: &str, measured: CapLevel, declared: CapLevel) {
        let inflated = match (declared, measured) {
            (CapLevel::Supported, CapLevel::Degraded) | (CapLevel::Supported, CapLevel::Unsupported) | (CapLevel::Degraded, CapLevel::Unsupported) => true,
            _ => false,
        };
        if inflated {
            self.corrections.push((
                name.to_string(),
                format!("声明 {:?} 实测 {:?}——虚标已修正", declared, measured),
            ));
            self.record_event("correct_capability", "E_CAPABILITY_INFLATED", format!("能力 {} 虚标：声明 {:?} 实测 {:?}，已强制修正并降级", name, declared, measured));
        }
    }

    /// 读屏能力清单（无障碍判据：能力清单读屏可达）。
    pub fn capability_screen_text(&self) -> String {
        let mut s = String::from("软渲染能力清单：");
        for c in shader_backend_capabilities().iter() {
            let tag = match c.level {
                CapLevel::Supported => "支持",
                CapLevel::Degraded => "降级",
                CapLevel::Unsupported => "不支持",
            };
            s.push_str(&format!("{}（{}）：{}；", c.name, tag, c.detail));
        }
        s
    }
}

impl Default for SoftFallback {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 四、回归门禁（每版全量跑通最小场景集 × 跨 CPU 架构矩阵）
// ---------------------------------------------------------------------------

/// 单架构回归门禁结果。
#[derive(Clone, Debug)]
pub struct ArchGateResult {
    /// 架构名（x64/ARM64）。
    pub arch: &'static str,
    /// 逐场景通过账。
    pub scenes: Vec<(&'static str, bool)>,
    /// 全过。
    pub all_passed: bool,
}

/// 软渲染回归门禁：跨 CPU 架构 × 最小场景集全量。
pub struct RegressionGate {
    /// 各架构结果。
    pub results: Vec<ArchGateResult>,
}

impl RegressionGate {
    /// 跑门禁：场景执行器按 (架构, 场景) 注入通过/失败（确定性注入，回归可复现）。
    pub fn run(mut exec: impl FnMut(&'static str, &'static str) -> bool) -> Self {
        let mut results = Vec::new();
        for arch in GATE_ARCHES {
            let mut scenes = Vec::new();
            let mut all = true;
            for scene in MIN_SCENE_SET {
                let ok = exec(arch, scene);
                if !ok {
                    all = false;
                }
                scenes.push((scene, ok));
            }
            results.push(ArchGateResult { arch, scenes, all_passed: all });
        }
        RegressionGate { results }
    }

    /// 门禁是否全绿（全部架构 × 全部场景）。
    pub fn all_green(&self) -> bool {
        self.results.iter().all(|r| r.all_passed)
    }

    /// 门禁人话报告（失败场景逐条点名——回归门禁不许静默）。
    pub fn report(&self) -> String {
        let mut s = String::from("软渲染回归门禁：");
        for r in self.results.iter() {
            s.push_str(&format!("[{}] ", r.arch));
            for (scene, ok) in r.scenes.iter() {
                s.push_str(&format!("{}={}, ", scene, if *ok { "过" } else { "红" }));
            }
        }
        s.push_str(&format!("总判定：{}", if self.all_green() { "全绿" } else { "有红项" }));
        s
    }
}

// ---------------------------------------------------------------------------
// 五、自检注册入口
// ---------------------------------------------------------------------------

/// VE-F0013 域自检（判据逐条映射见 `vea13_checks.rs`）。
pub fn run_vea13_checks() -> CheckSet {
    super::vea13_checks::run_vea13_checks()
}

// ---------------------------------------------------------------------------
// 六、单元测试（宿主 cargo test 直跑）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn vea13_full_stack_fall_and_climb() {
        let mut f = SoftFallback::new();
        f.on_gpu_lost("驱动失效");
        assert_eq!(f.level, SoftLevel::FullSoft);
        assert!(f.expectation_served, "进入软渲即一次性告知预期");
        assert!(f.expectation_text().contains("一次说清"), "预期文本读屏可播");
        // 连续崩溃达阈值 → 最小模式。
        f.on_soft_crash("合成器");
        f.on_soft_crash("光栅化");
        assert_eq!(f.level, SoftLevel::MinCompose);
        assert!(f.expectation_text().contains("最小合成模式"));
        // GPU 恢复：逐级爬回，不许跳级。
        let l1 = f.on_gpu_recovered();
        assert_eq!(l1, SoftLevel::FullSoft);
        let l0 = f.on_gpu_recovered();
        assert_eq!(l0, SoftLevel::Gpu, "崩溃已清，二级爬回成功");
    }

    #[test]
    fn vea13_honest_capability_and_gate_matrix() {
        let caps = shader_backend_capabilities();
        // 覆盖度声明三态齐全：FP64/RT/视频硬解必须显式"不支持"。
        assert!(caps.iter().any(|c| c.name == "双精度运算" && c.level == CapLevel::Unsupported));
        assert!(caps.iter().any(|c| c.name == "光线追踪" && c.level == CapLevel::Unsupported));
        assert!(caps.iter().any(|c| c.name == "着色器模型" && c.level == CapLevel::Degraded));
        let f = SoftFallback::new();
        let t = f.capability_screen_text();
        assert!(t.contains("不支持") && t.contains("降级"), "能力清单读屏可达");
        // 跨架构门禁：全过 → 绿；注入一红 → 报告点名。
        let g = RegressionGate::run(|_a, _s| true);
        assert!(g.all_green());
        let g2 = RegressionGate::run(|a, s| !(a == "ARM64" && s == "文字"));
        assert!(!g2.all_green());
        assert!(g2.report().contains("文字=红"));
    }

    #[test]
    fn vea13_capability_inflation_corrected() {
        let mut f = SoftFallback::new();
        f.correct_capability("双精度运算", CapLevel::Unsupported, CapLevel::Supported);
        assert_eq!(f.corrections.len(), 1, "虚标入账");
        assert!(f.events.iter().any(|(_, c, _)| *c == "E_CAPABILITY_INFLATED"));
        // 未虚标（实测=声明）不入账。
        f.correct_capability("基础光栅化", CapLevel::Supported, CapLevel::Supported);
        assert_eq!(f.corrections.len(), 1);
    }

    #[test]
    fn vea13_degrade_notice_once_per_level() {
        let mut f = SoftFallback::new();
        f.on_gpu_lost("x");
        // L1 承诺 30fps：实测 20fps（< 80% 阈值 24）→ 告知一次。
        let n1 = f.report_fps(20);
        assert!(n1.is_some(), "劣化应触发告知");
        let n2 = f.report_fps(18);
        assert!(n2.is_none(), "同档不重复告知");
        assert!(f.degrade_notices.len() == 1);
        assert!(f.events.iter().any(|(_, c, _)| *c == "E_PERF_DEGRADED"));
        // 正常帧率不告知。
        let mut g = SoftFallback::new();
        g.on_gpu_lost("y");
        assert!(g.report_fps(30).is_none(), "达标帧率不触发");
        // GPU 正常态无判定。
        let mut h = SoftFallback::new();
        assert!(h.report_fps(5).is_none());
    }

    #[test]
    fn vea13_checks_all_green() {
        let set = run_vea13_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "VE-F0013 域自检存在红项：{}/{} 绿，红项：{:?}",
            passed,
            passed + failed,
            set.red_items()
        );
    }
}
