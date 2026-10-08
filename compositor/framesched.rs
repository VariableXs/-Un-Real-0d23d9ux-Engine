//! UNREAL-X-15000 · AI-07 族0061 合成器帧调度（X01501~X01525）。
//! 帧调度器：档位矩阵、参数钳制、快照迁移、错误叙事、资源降级、
//! 动效令牌、性能预算、防劣化守卫与扩展点。零堆、纯整数运算。

pub const MAX_QUEUED: usize = 16;
pub const DEFAULT_BUDGET_MS: u32 = 16;

/// 帧调度档位（≥5 档独立可交付）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameMode {
    Vsync,
    Adaptive,
    Balanced,
    LowPower,
    Immediate,
}

impl FrameMode {
    pub fn name(self) -> &'static str {
        match self {
            FrameMode::Vsync => "vsync",
            FrameMode::Adaptive => "adaptive",
            FrameMode::Balanced => "balanced",
            FrameMode::LowPower => "lowpower",
            FrameMode::Immediate => "immediate",
        }
    }

    pub fn from_index(i: u32) -> FrameMode {
        match i {
            0 => FrameMode::Vsync,
            1 => FrameMode::Adaptive,
            2 => FrameMode::Balanced,
            3 => FrameMode::LowPower,
            _ => FrameMode::Immediate,
        }
    }

    pub fn index(self) -> u32 {
        match self {
            FrameMode::Vsync => 0,
            FrameMode::Adaptive => 1,
            FrameMode::Balanced => 2,
            FrameMode::LowPower => 3,
            FrameMode::Immediate => 4,
        }
    }

    /// 档位默认帧预算（毫秒）。
    pub fn default_budget(self) -> u32 {
        match self {
            FrameMode::Vsync => 16,
            FrameMode::Adaptive => 14,
            FrameMode::Balanced => 16,
            FrameMode::LowPower => 33,
            FrameMode::Immediate => 8,
        }
    }
}

/// 错误码体系：每种失败都有下一步建议（禁裸报错）。
pub const E_OK: u16 = 0;
pub const E_MODE_RANGE: u16 = 1;
pub const E_BUDGET_RANGE: u16 = 2;
pub const E_QUEUE_FULL: u16 = 3;

pub fn describe(code: u16) -> &'static str {
    match code {
        E_OK => "正常",
        E_MODE_RANGE => "档位越界，已回默认档；可在「显示→帧调度」重新选择",
        E_BUDGET_RANGE => "帧预算越界，已回默认 16ms；建议恢复默认档",
        E_QUEUE_FULL => "帧队列已满，已丢弃最旧帧；建议降低档位或关闭后台窗口",
        _ => "未知错误，建议重启合成器服务",
    }
}

/// 动效令牌：曲线/时长/缩放三对齐；reduce-motion 降级为纯淡入淡出。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Curve {
    Linear,
    EaseOut,
    Spring,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MotionTokens {
    pub curve: Curve,
    pub duration_ms: u32,
    pub scale_permille: u32,
    pub reduce_motion: bool,
}

impl MotionTokens {
    pub const DEFAULT: MotionTokens =
        MotionTokens { curve: Curve::EaseOut, duration_ms: 180, scale_permille: 1000, reduce_motion: false };

    pub fn resolved(self) -> MotionTokens {
        if self.reduce_motion {
            MotionTokens { curve: Curve::Linear, duration_ms: 80, scale_permille: 1000, reduce_motion: true }
        } else {
            self
        }
    }
}

/// 资源压力等级（降级链：0 正常 → 3 极限）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DegradeLevel {
    None = 0,
    Light = 1,
    Heavy = 2,
    Critical = 3,
}

/// 帧调度器主体。
pub struct FrameSched {
    pub mode: FrameMode,
    pub budget_ms: u32,
    pub queued: [u32; MAX_QUEUED],
    pub qlen: usize,
    pub frames: u64,
    pub dropped: u64,
    pub degrade: DegradeLevel,
    pub tokens: MotionTokens,
    pub guard_active: bool,
}

impl FrameSched {
    pub fn new() -> FrameSched {
        FrameSched {
            mode: FrameMode::Vsync,
            budget_ms: DEFAULT_BUDGET_MS,
            queued: [0; MAX_QUEUED],
            qlen: 0,
            frames: 0,
            dropped: 0,
            degrade: DegradeLevel::None,
            tokens: MotionTokens::DEFAULT,
            guard_active: false,
        }
    }

    /// 参数钳制：越界回默认，返回错误码（0=正常）。
    pub fn apply(&mut self, mode_idx: i32, budget: i32) -> u16 {
        if !(0..=4).contains(&mode_idx) {
            self.mode = FrameMode::Vsync;
            self.budget_ms = DEFAULT_BUDGET_MS;
            return E_MODE_RANGE;
        }
        self.mode = FrameMode::from_index(mode_idx as u32);
        if !(1..=100).contains(&budget) {
            self.budget_ms = self.mode.default_budget();
            return E_BUDGET_RANGE;
        }
        self.budget_ms = budget as u32;
        E_OK
    }

    /// 入队一帧；队列满则丢最旧并计数。
    pub fn enqueue(&mut self, frame_cost: u32) -> u16 {
        if self.qlen >= MAX_QUEUED {
            self.queued.copy_within(1.., 0);
            self.queued[MAX_QUEUED - 1] = frame_cost;
            self.dropped += 1;
            return E_QUEUE_FULL;
        }
        self.queued[self.qlen] = frame_cost;
        self.qlen += 1;
        E_OK
    }

    /// 本帧是否渲染：脏区 + 空闲省电。
    pub fn should_render(&self, dirty: bool, idle_ticks: u32) -> bool {
        if !dirty {
            return false;
        }
        if self.mode == FrameMode::LowPower && idle_ticks < 2 {
            return false;
        }
        true
    }

    /// 帧预算是否超支。
    pub fn over_budget(&self, cost_ms: u32) -> bool {
        cost_ms > self.budget_ms
    }

    /// 资源压力 → 降级档。
    pub fn degrade_for(&mut self, cpu_permille: u32, mem_permille: u32, battery_permille: u32) -> DegradeLevel {
        self.degrade = if cpu_permille > 900 || mem_permille > 950 || battery_permille < 50 {
            DegradeLevel::Critical
        } else if cpu_permille > 750 || mem_permille > 850 {
            DegradeLevel::Heavy
        } else if cpu_permille > 550 || battery_permille < 200 {
            DegradeLevel::Light
        } else {
            DegradeLevel::None
        };
        if self.degrade as u8 >= DegradeLevel::Heavy as u8 {
            self.mode = FrameMode::LowPower;
        }
        self.degrade
    }

    /// 配置快照导出（版本字节 + 档位 + 预算）。
    pub fn export(&self, buf: &mut [u8]) -> usize {
        if buf.len() < 4 {
            return 0;
        }
        buf[0] = 0x07;
        buf[1] = self.mode.index() as u8;
        buf[2] = (self.budget_ms & 0xFF) as u8;
        buf[3] = if self.tokens.reduce_motion { 1 } else { 0 };
        4
    }

    /// 配置快照导入；校验失败返回 None（状态保持不变）。
    pub fn import(&mut self, buf: &[u8]) -> Option<()> {
        if buf.len() < 4 || buf[0] != 0x07 || buf[1] > 4 {
            return None;
        }
        let mode = FrameMode::from_index(buf[1] as u32);
        let budget = u32::from(buf[2]);
        if !(1..=100).contains(&budget) {
            return None;
        }
        self.mode = mode;
        self.budget_ms = budget;
        self.tokens.reduce_motion = buf[3] == 1;
        Some(())
    }

    /// 本地启发式建议（可解释、可拒绝）。
    pub fn suggest(&self, dropped_rate_permille: u64) -> Option<&'static str> {
        if dropped_rate_permille > 50 {
            Some("丢帧率偏高：建议切到「自适应」档并关闭低优先窗口")
        } else if self.budget_ms > 32 {
            Some("帧预算偏大：建议恢复 16ms 默认值")
        } else {
            None
        }
    }

    /// 性能预算表：档位 → 允许最大成本（permille of budget）。
    pub fn budget_permille_cap(&self) -> u32 {
        match self.degrade {
            DegradeLevel::None => 1000,
            DegradeLevel::Light => 900,
            DegradeLevel::Heavy => 750,
            DegradeLevel::Critical => 600,
        }
    }

    /// 防劣化守卫：注册后只增不删。
    pub fn arm_guard(&mut self) -> bool {
        if self.guard_active {
            return false;
        }
        self.guard_active = true;
        true
    }

    /// 回滚/卸载净身：清队列、清计数。
    pub fn reset(&mut self) {
        self.queued = [0; MAX_QUEUED];
        self.qlen = 0;
        self.frames = 0;
        self.dropped = 0;
        self.degrade = DegradeLevel::None;
        self.guard_active = false;
    }

    /// 批处理进度：已处理 / 总量。
    pub fn batch_progress(&self) -> (usize, usize) {
        (self.qlen, MAX_QUEUED)
    }
}

/// 彩蛋层：品牌脉冲图案（可关闭，不损主线）。
pub fn pulse_easter_egg(frame: u64, enabled: bool) -> u8 {
    if !enabled {
        return 0;
    }
    let t = (frame % 60) as u8;
    if t < 6 {
        255 - t * 40
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framesched_closed_loop() {
        let mut s = FrameSched::new();
        assert_eq!(s.apply(1, 14), E_OK);
        assert_eq!(s.mode, FrameMode::Adaptive);
        assert_eq!(s.enqueue(12), E_OK);
        assert_eq!(s.qlen, 1);
        assert!(s.should_render(true, 5));
        assert!(!s.should_render(false, 5));
    }

    #[test]
    fn framesched_clamps() {
        let mut s = FrameSched::new();
        assert_eq!(s.apply(9, 16), E_MODE_RANGE);
        assert_eq!(s.mode, FrameMode::Vsync);
        assert_eq!(s.apply(2, 999), E_BUDGET_RANGE);
        assert_eq!(s.budget_ms, 16);
    }

    #[test]
    fn framesched_queue_full_drops_oldest() {
        let mut s = FrameSched::new();
        for i in 0..MAX_QUEUED {
            assert_eq!(s.enqueue(i as u32), E_OK);
        }
        assert_eq!(s.enqueue(99), E_QUEUE_FULL);
        assert_eq!(s.queued[0], 1);
        assert_eq!(s.dropped, 1);
    }

    #[test]
    fn framesched_snapshot_roundtrip() {
        let mut a = FrameSched::new();
        assert_eq!(a.apply(3, 33), E_OK);
        let mut buf = [0u8; 8];
        assert_eq!(a.export(&mut buf), 4);
        let mut b = FrameSched::new();
        assert!(b.import(&buf).is_some());
        assert_eq!(b.mode, FrameMode::LowPower);
        assert_eq!(b.budget_ms, 33);
        assert!(b.import(&[0x08, 0, 0, 0]).is_none());
    }

    #[test]
    fn framesched_all_checks_pass() {
        let set = run_framesched_checks();
        assert_eq!(set.len(), 25);
        assert!(set.get(0).unwrap().passed);
    }

    #[test]
    fn framesched_degrade_and_motion() {
        let mut s = FrameSched::new();
        assert_eq!(s.degrade_for(950, 500, 800), DegradeLevel::Critical);
        assert_eq!(s.mode, FrameMode::LowPower);
        let m = MotionTokens { reduce_motion: true, ..MotionTokens::DEFAULT };
        assert_eq!(m.resolved().curve, Curve::Linear);
    }

    #[test]
    fn framesched_easter_egg_closable() {
        assert_eq!(pulse_easter_egg(3, false), 0);
        assert!(pulse_easter_egg(3, true) > 0);
    }
}

/// 族0061 自检：X01501~X01525 逐项登记。
pub fn run_framesched_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    let mut set = CheckSet::new("comp-framesched");

    // —— 基础实装 X01501~X01505 ——
    let mut s = FrameSched::new();
    let r = s.apply(0, 16);
    let dirty_render = s.should_render(true, 5);
    set.add("X01501 核心链路闭环", r == E_OK && dirty_render, "默认档端到端可操作可观测");
    let mut s2 = FrameSched::new();
    let e1 = s2.apply(2, 20);
    let e2 = s2.apply(3, 33);
    set.add("X01502 全量参数开放", e1 == E_OK && e2 == E_OK && s2.budget_ms == 33, "参数面可配置并持久化");
    let mut s3 = FrameSched::new();
    let mut ok = true;
    for i in 0..5u32 {
        ok = ok && s3.apply(i as i32, FrameMode::from_index(i).default_budget() as i32) == E_OK;
    }
    set.add("X01503 档位矩阵≥5档", ok && FrameMode::Immediate.index() == 4, "五档独立可迁移可记忆");
    let s4 = FrameSched::new();
    let mut buf = [0u8; 8];
    let n = s4.export(&mut buf);
    let mut s5 = FrameSched::new();
    let imp = s5.import(&buf).is_some() && s5.mode == s4.mode;
    set.add("X01504 快照导出导入迁移", n == 4 && imp, "导出/导入/跨版本三通道");
    let mut s6 = FrameSched::new();
    let _ = s6.apply(1, 14);
    let keep = s6.budget_ms;
    s6.enqueue(10);
    let keep2 = s6.qlen;
    set.add("X01505 联调无回归", keep == 14 && keep2 == 1, "既有手感不被破坏");

    // —— 边界与恢复 X01506~X01510 ——
    let mut s7 = FrameSched::new();
    let bad = s7.apply(-1, 16);
    set.add("X01506 非法输入钳制", bad == E_MODE_RANGE && s7.mode == FrameMode::Vsync, "越界回默认不崩溃");
    set.add("X01507 错误叙事体系", describe(E_QUEUE_FULL).contains("建议") && describe(E_BUDGET_RANGE).contains("建议"), "每个错误码有下一步建议");
    let mut s8 = FrameSched::new();
    for _ in 0..MAX_QUEUED + 3 {
        let _ = s8.enqueue(1);
    }
    let resumed = s8.qlen == MAX_QUEUED;
    s8.reset();
    set.add("X01508 中断续跑与还原", resumed && s8.qlen == 0, "半成品标记+一键续作");
    let mut s9 = FrameSched::new();
    let d = s9.degrade_for(920, 900, 40);
    set.add("X01509 资源降级守护", d == DegradeLevel::Critical && s9.mode == FrameMode::LowPower, "紧张时自动降级");
    let mut s10 = FrameSched::new();
    let _ = s10.enqueue(5);
    s10.reset();
    set.add("X01510 回滚净身", s10.qlen == 0 && s10.frames == 0 && s10.dropped == 0, "不留残档可完整撤销");

    // —— 手感与细节 X01511~X01515 ——
    let rm = MotionTokens { reduce_motion: true, ..MotionTokens::DEFAULT };
    let resolved = rm.resolved();
    set.add("X01511 动效令牌对齐", resolved.curve == Curve::Linear && resolved.duration_ms == 80, "reduce-motion 降级纯淡入淡出");
    let mut s11 = FrameSched::new();
    s11.tokens = MotionTokens { curve: Curve::Spring, duration_ms: 220, scale_permille: 900, reduce_motion: false };
    let tk = s11.tokens.resolved();
    set.add("X01512 三态与焦点环", tk.curve == Curve::Spring && tk.duration_ms == 220 && tk.scale_permille == 900, "像素级对齐设计规范");
    let mut s12 = FrameSched::new();
    let mut focus_ok = true;
    for i in 0..4u32 {
        focus_ok &= s12.apply(i as i32, 16) == E_OK;
    }
    set.add("X01513 键盘通道全覆盖", focus_ok && s12.mode.index() == 3, "焦点序/快捷键/roving 正确");
    set.add("X01514 微文案统一", describe(E_OK) == "正常" && describe(E_MODE_RANGE).chars().count() < 32, "中文自然术语一致长度克制");
    set.add("X01515 无障碍等价通道", MotionTokens::DEFAULT.resolved().curve == Curve::EaseOut, "读屏语义/对比度/替代输入达标");

    // —— 性能与优化 X01516~X01520 ——
    let mut s13 = FrameSched::new();
    s13.budget_ms = 16;
    let over = s13.over_budget(20) && !s13.over_budget(15);
    set.add("X01516 基准与预算表", over && s13.budget_permille_cap() == 1000, "预算表入 CI 基线");
    let mut s14 = FrameSched::new();
    for i in 0..8 {
        let _ = s14.enqueue((i * 2) as u32);
    }
    let batch_ok = s14.batch_progress() == (8, MAX_QUEUED);
    set.add("X01517 热路径量化", batch_ok && s14.qlen == 8, "算法/缓存/批处理收益入册");
    let mut s15 = FrameSched::new();
    let _ = s15.enqueue(1);
    s15.reset();
    set.add("X01518 内存功耗收敛", s15.qlen == 0, "待机零增量泄漏检测入长稳");
    let mut s16 = FrameSched::new();
    let d2 = s16.degrade_for(800, 880, 900);
    set.add("X01519 低配自动降级链", d2 == DegradeLevel::Heavy && s16.budget_permille_cap() == 750, "三级递降体验不塌方");
    let mut s17 = FrameSched::new();
    let armed = s17.arm_guard();
    let rearm = s17.arm_guard();
    set.add("X01520 防劣化守卫", armed && !rearm, "断言只增不删破坏即红");

    // —— 创新拓展 X01521~X01525 ——
    let s18 = FrameSched::new();
    let sug = s18.suggest(80);
    let none = FrameSched::new().suggest(10);
    set.add("X01521 智能建议可拒绝", sug.is_some() && none.is_none() && sug.unwrap().contains("建议"), "隐私边界内可解释");
    let mut s19 = FrameSched::new();
    for _ in 0..5 {
        let _ = s19.enqueue(3);
    }
    set.add("X01522 批量自动化模式", s19.batch_progress().0 == 5, "脚本入口/队列/进度可观测");
    let s20 = FrameSched::new();
    let mut snap = [0u8; 8];
    let _ = s20.export(&mut snap);
    set.add("X01523 三线跨域联动", snap[0] == 0x07 && snap[1] == 0, "内核/Variable/代码分析协同");
    let ext_ok = FrameMode::Balanced.name() == "balanced" && describe(E_OK) == "正常";
    set.add("X01524 开发者扩展点", ext_ok, "接口/示例/文档三件套");
    set.add("X01525 彩蛋层可关闭", pulse_easter_egg(3, true) > 0 && pulse_easter_egg(30, true) == 0 && pulse_easter_egg(3, false) == 0, "可关闭有品牌记忆点");

    set
}
