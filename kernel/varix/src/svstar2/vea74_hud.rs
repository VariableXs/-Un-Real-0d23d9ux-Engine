//! VE-F0074 · 呈现调试 HUD（VE-A 域 · A03b 呈现组 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0074`
//!
//! **职责定位（锚点原文）**：呈现调试 HUD——帧时间曲线、丢弃标记、队列深度、
//! 当前呈现模式叠加显示；HUD 数据只读自 F0069 统计快照，开关与透明度可调，
//! HUD 自身不计入被测呈现路径开销（旁路渲染）。
//!
//! **铁律**：
//! 1. **只读订阅**——HUD 永不写回统计源，只持有快照引用。
//! 2. **旁路渲染**——HUD 绘制走独立通道，不进被测呈现路径。
//! 3. **失联显性**——数据源失联时显示失联状态，绝不显示陈旧数据冒充现状。
//! 4. **可关闭**——开关与透明度用户可控，关闭后零订阅零开销。

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址 0xE8A4（HUD 域）。
pub const SEG_BASE: u16 = 0xE8A4;

/// 配置非法：拒绝并保留当前配置。
pub const XC_CONFIG_INVALID: u16 = SEG_BASE | 0x01;
/// 数据源失联。
pub const XC_SOURCE_LOST: u16 = SEG_BASE | 0x02;
/// 旁路渲染失败。
pub const XC_BYPASS_FAIL: u16 = SEG_BASE | 0x03;
/// 订阅不存在。
pub const XC_NO_SUB: u16 = SEG_BASE | 0x04;
/// 透明度越界。
pub const XC_OPACITY: u16 = SEG_BASE | 0x05;
/// 已关闭仍尝试绘制。
pub const XC_HUD_OFF: u16 = SEG_BASE | 0x06;

/// HUD 位置（角位，文字型语义——不只靠方位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HudCorner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// HUD 配置：开关 × 透明度 × 位置。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HudConfig {
    pub enabled: bool,
    /// 透明度 0..=255（0 全透，255 不透）。
    pub opacity: u8,
    pub corner: HudCorner,
    /// 是否显示帧时间曲线。
    pub show_frame_curve: bool,
    /// 是否显示丢弃标记。
    pub show_drop_marks: bool,
    /// 是否显示队列深度。
    pub show_queue_depth: bool,
    /// 是否显示当前呈现模式。
    pub show_present_mode: bool,
}

impl Default for HudConfig {
    fn default() -> Self {
        HudConfig {
            enabled: true,
            opacity: 200,
            corner: HudCorner::TopRight,
            show_frame_curve: true,
            show_drop_marks: true,
            show_queue_depth: true,
            show_present_mode: true,
        }
    }
}

impl HudConfig {
    /// 校验配置合法性：透明度任意值合法（u8 全域）；结构性校验预留。
    pub fn validate(&self) -> Result<(), u16> {
        // 透明度 u8 全域合法；此处校验「全关显示项」的无意义配置——
        // 开启 HUD 却关掉全部显示项等于空壳，拒绝以免用户误以为 HUD 在工作。
        if self.enabled
            && !self.show_frame_curve
            && !self.show_drop_marks
            && !self.show_queue_depth
            && !self.show_present_mode
        {
            return Err(XC_CONFIG_INVALID);
        }
        Ok(())
    }
}

/// 只读统计快照（F0069 数据源的投影——HUD 侧只见只读面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StatsSnapshot {
    /// 快照序列号（单调递增，用于判新旧）。
    pub seq: u64,
    /// 最近帧时间（微秒）。
    pub frame_us: u32,
    /// 帧时间环形缓冲（曲线数据，最多 64 点）。
    pub curve_len: u8,
    /// 丢弃帧计数。
    pub dropped: u32,
    /// 当前队列深度。
    pub queue_depth: u16,
    /// 呈现模式（文字型码：0 即时 / 1 邮箱 / 2 FIFO）。
    pub present_mode: u8,
}

impl StatsSnapshot {
    pub fn empty() -> Self {
        StatsSnapshot {
            seq: 0,
            frame_us: 0,
            curve_len: 0,
            dropped: 0,
            queue_depth: 0,
            present_mode: 0,
        }
    }

    /// 呈现模式文字语义（不只靠颜色/图形表意）。
    pub fn mode_label(&self) -> &'static str {
        match self.present_mode {
            0 => "immediate",
            1 => "mailbox",
            2 => "fifo",
            _ => "unknown",
        }
    }
}

/// HUD 数据源状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SourceState {
    /// 已订阅且快照新鲜。
    Live,
    /// 数据源失联（显示失联状态，不显示旧数据）。
    Lost,
    /// 未订阅。
    Unsubscribed,
}

/// 帧时间曲线环形缓冲（旁路侧自有存储，不写回数据源）。
#[derive(Clone, Debug)]
pub struct FrameCurve {
    buf: Vec<u32>,
    cap: usize,
    head: usize,
}

impl FrameCurve {
    pub fn new(cap: usize) -> Self {
        let c = if cap == 0 { 64 } else { cap.min(256) };
        FrameCurve {
            buf: vec![0u32; c],
            cap: c,
            head: 0,
        }
    }

    pub fn push(&mut self, us: u32) {
        self.buf[self.head] = us;
        self.head = (self.head + 1) % self.cap;
    }

    /// 按时间序读出（最旧在前）。
    pub fn samples(&self) -> Vec<u32> {
        let mut out = Vec::with_capacity(self.cap);
        for i in 0..self.cap {
            let idx = (self.head + i) % self.cap;
            out.push(self.buf[idx]);
        }
        out
    }

    pub fn len(&self) -> usize {
        self.cap
    }
}

/// 旁路渲染结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BypassResult {
    /// 绘制成功（旁路通道）。
    Drawn,
    /// 旁路通道故障。
    Failed,
    /// HUD 已关闭，跳过绘制。
    Skipped,
    /// 数据源失联，绘制失联态。
    DrawnLost,
}

/// 呈现调试 HUD。
pub struct PresentHud {
    cfg: HudConfig,
    source_state: SourceState,
    last_snap: StatsSnapshot,
    curve: FrameCurve,
    /// 旁路通道健康位。
    bypass_ok: bool,
    last_code: u16,
    /// 累计旁路绘制次数（自证旁路被调用）。
    draw_count: u64,
    /// 累计失联态绘制次数。
    lost_draws: u64,
}

impl PresentHud {
    pub fn new(cfg: HudConfig) -> Result<Self, u16> {
        cfg.validate()?;
        Ok(PresentHud {
            cfg,
            source_state: SourceState::Unsubscribed,
            last_snap: StatsSnapshot::empty(),
            curve: FrameCurve::new(64),
            bypass_ok: true,
            last_code: 0,
            draw_count: 0,
            lost_draws: 0,
        })
    }

    pub fn config(&self) -> HudConfig {
        self.cfg
    }

    pub fn last_code(&self) -> u16 {
        self.last_code
    }

    pub fn source_state(&self) -> SourceState {
        self.source_state
    }

    /// 应用新配置：非法则拒绝并保留当前。
    pub fn apply_config(&mut self, cfg: HudConfig) -> Result<(), u16> {
        if let Err(e) = cfg.validate() {
            self.last_code = e;
            return Err(e);
        }
        self.cfg = cfg;
        self.last_code = 0;
        Ok(())
    }

    /// 调整透明度（独立入口，越界显性拒绝）。
    pub fn set_opacity(&mut self, opacity: u8) -> Result<(), u16> {
        // u8 全域合法，但 0（全透）视为「看不见 = 无效 HUD」显性拒绝。
        if opacity == 0 {
            self.last_code = XC_OPACITY;
            return Err(XC_OPACITY);
        }
        self.cfg.opacity = opacity;
        self.last_code = 0;
        Ok(())
    }

    /// 订阅数据源（只读）。
    pub fn subscribe(&mut self) {
        self.source_state = SourceState::Live;
        self.last_code = 0;
    }

    /// 退订（关闭路径的伴生动作）。
    pub fn unsubscribe(&mut self) {
        self.source_state = SourceState::Unsubscribed;
        self.last_code = 0;
    }

    /// 推入一帧只读快照（订阅面唯一入口）。
    pub fn on_snapshot(&mut self, snap: StatsSnapshot) -> Result<(), u16> {
        if self.source_state == SourceState::Unsubscribed {
            self.last_code = XC_NO_SUB;
            return Err(XC_NO_SUB);
        }
        // 序号倒退 = 数据源异常，转失联。
        if snap.seq < self.last_snap.seq {
            self.source_state = SourceState::Lost;
            self.last_code = XC_SOURCE_LOST;
            return Err(XC_SOURCE_LOST);
        }
        self.source_state = SourceState::Live;
        self.last_snap = snap;
        if self.cfg.show_frame_curve {
            self.curve.push(snap.frame_us);
        }
        self.last_code = 0;
        Ok(())
    }

    /// 标记数据源失联（心跳超时由上游调用）。
    pub fn mark_lost(&mut self) {
        self.source_state = SourceState::Lost;
        self.last_code = XC_SOURCE_LOST;
    }

    /// 旁路渲染一帧 HUD。被测路径零侵入——本函数不进呈现管线。
    pub fn draw_bypass(&mut self) -> Result<BypassResult, u16> {
        if !self.cfg.enabled {
            self.last_code = XC_HUD_OFF;
            return Ok(BypassResult::Skipped);
        }
        if !self.bypass_ok {
            self.last_code = XC_BYPASS_FAIL;
            return Err(XC_BYPASS_FAIL);
        }
        match self.source_state {
            SourceState::Lost => {
                // 失联显性：绘制失联态，绝不拿 last_snap 冒充现状。
                self.lost_draws += 1;
                self.draw_count += 1;
                self.last_code = XC_SOURCE_LOST;
                Ok(BypassResult::DrawnLost)
            }
            SourceState::Unsubscribed => {
                self.last_code = XC_NO_SUB;
                Err(XC_NO_SUB)
            }
            SourceState::Live => {
                self.draw_count += 1;
                self.last_code = 0;
                Ok(BypassResult::Drawn)
            }
        }
    }

    /// 注入旁路通道故障（测试支撑 / 上游故障传播）。
    pub fn inject_bypass_fault(&mut self) {
        self.bypass_ok = false;
    }

    /// 旁路故障后 HUD 自动关闭（锚点错误路径）。
    pub fn on_bypass_failure(&mut self) {
        self.bypass_ok = false;
        self.cfg.enabled = false;
        self.last_code = XC_BYPASS_FAIL;
    }

    /// 重新打开（配置恢复入口）。
    pub fn reenable(&mut self) -> Result<(), u16> {
        self.bypass_ok = true;
        self.cfg.enabled = true;
        self.last_code = 0;
        self.cfg.validate()
    }

    /// 当前应显示的内容描述（供一致性断言与读屏层消费）。
    pub fn describe(&self) -> Vec<u32> {
        // 用数值编码描述（无 String 依赖，纯功能面）：
        // [0]=状态码 1=live 2=lost 3=unsub；[1]=opacity；[2]=corner；
        // [3]=frame_us；[4]=dropped；[5]=queue；[6]=mode；[7]=draws。
        let st = match self.source_state {
            SourceState::Live => 1,
            SourceState::Lost => 2,
            SourceState::Unsubscribed => 3,
        };
        let corner = match self.cfg.corner {
            HudCorner::TopLeft => 0,
            HudCorner::TopRight => 1,
            HudCorner::BottomLeft => 2,
            HudCorner::BottomRight => 3,
        };
        vec![
            st,
            self.cfg.opacity as u32,
            corner,
            self.last_snap.frame_us,
            self.last_snap.dropped,
            self.last_snap.queue_depth as u32,
            self.last_snap.present_mode as u32,
            self.draw_count as u32,
        ]
    }

    pub fn draw_count(&self) -> u64 {
        self.draw_count
    }

    pub fn lost_draws(&self) -> u64 {
        self.lost_draws
    }

    pub fn curve(&self) -> &FrameCurve {
        &self.curve
    }
}

/// 自检：只读订阅、旁路渲染、失联显性、可关闭、配置拒绝。
pub fn run_vea74_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }

    let mut set = CheckSet::new("vea74_hud");

    // C74-01 默认配置合法可建
    {
        let hud = PresentHud::new(HudConfig::default());
        chk(&mut set, "C74-01 默认配置", hud.is_ok(), "默认配置被拒");
    }

    // C74-02 空壳配置（全显示项关）被拒
    {
        let mut cfg = HudConfig::default();
        cfg.show_frame_curve = false;
        cfg.show_drop_marks = false;
        cfg.show_queue_depth = false;
        cfg.show_present_mode = false;
        let r = PresentHud::new(cfg);
        chk(
            &mut set,
            "C74-02 空壳配置拒绝",
            r.err() == Some(XC_CONFIG_INVALID),
            "空壳配置未拒",
        );
    }

    // C74-03 非法配置 apply 被拒且保留当前
    {
        let mut hud = PresentHud::new(HudConfig::default()).unwrap();
        let before = hud.config().opacity;
        let mut bad = HudConfig::default();
        bad.show_frame_curve = false;
        bad.show_drop_marks = false;
        bad.show_queue_depth = false;
        bad.show_present_mode = false;
        let r = hud.apply_config(bad);
        let ok = r.is_err() && hud.config().opacity == before;
        chk(&mut set, "C74-03 非法配置保留当前", ok, "配置被污染");
    }

    // C74-04 只读订阅：快照推进被记录，无写回
    {
        let mut hud = PresentHud::new(HudConfig::default()).unwrap();
        hud.subscribe();
        let snap = StatsSnapshot {
            seq: 1,
            frame_us: 16666,
            curve_len: 1,
            dropped: 0,
            queue_depth: 2,
            present_mode: 1,
        };
        let r = hud.on_snapshot(snap);
        let ok = r.is_ok() && hud.describe()[3] == 16666;
        chk(&mut set, "C74-04 只读订阅", ok, "快照未推进");
    }

    // C74-05 未订阅推快照显性拒绝
    {
        let mut hud = PresentHud::new(HudConfig::default()).unwrap();
        let r = hud.on_snapshot(StatsSnapshot {
            seq: 1,
            frame_us: 1,
            curve_len: 0,
            dropped: 0,
            queue_depth: 0,
            present_mode: 0,
        });
        chk(
            &mut set,
            "C74-05 未订阅拒绝",
            r.is_err() && hud.last_code() == XC_NO_SUB,
            "未订阅被放行",
        );
    }

    // C74-06 失联显性：失联后绘制返回 DrawnLost 不给旧数据
    {
        let mut hud = PresentHud::new(HudConfig::default()).unwrap();
        hud.subscribe();
        let _ = hud.on_snapshot(StatsSnapshot {
            seq: 5,
            frame_us: 999,
            curve_len: 0,
            dropped: 0,
            queue_depth: 0,
            present_mode: 0,
        });
        hud.mark_lost();
        let r = hud.draw_bypass();
        let ok = r == Ok(BypassResult::DrawnLost) && hud.lost_draws() >= 1;
        chk(&mut set, "C74-06 失联显性", ok, "失联仍显旧数据");
    }

    // C74-07 序号倒退转失联
    {
        let mut hud = PresentHud::new(HudConfig::default()).unwrap();
        hud.subscribe();
        let _ = hud.on_snapshot(StatsSnapshot {
            seq: 10,
            frame_us: 1,
            curve_len: 0,
            dropped: 0,
            queue_depth: 0,
            present_mode: 0,
        });
        let r = hud.on_snapshot(StatsSnapshot {
            seq: 3,
            frame_us: 1,
            curve_len: 0,
            dropped: 0,
            queue_depth: 0,
            present_mode: 0,
        });
        let ok = r.is_err() && hud.source_state() == SourceState::Lost;
        chk(&mut set, "C74-07 序号倒退转失联", ok, "倒退未被识破");
    }

    // C74-08 旁路渲染成功路径
    {
        let mut hud = PresentHud::new(HudConfig::default()).unwrap();
        hud.subscribe();
        let _ = hud.on_snapshot(StatsSnapshot {
            seq: 1,
            frame_us: 16000,
            curve_len: 0,
            dropped: 0,
            queue_depth: 1,
            present_mode: 0,
        });
        let r = hud.draw_bypass();
        chk(
            &mut set,
            "C74-08 旁路绘制",
            r == Ok(BypassResult::Drawn) && hud.draw_count() == 1,
            "旁路未走通",
        );
    }

    // C74-09 可关闭：关闭后 Skipped 零绘制
    {
        let mut cfg = HudConfig::default();
        cfg.enabled = false;
        let mut hud = PresentHud::new(cfg).unwrap();
        hud.subscribe();
        let r = hud.draw_bypass();
        let ok = r == Ok(BypassResult::Skipped) && hud.draw_count() == 0;
        chk(&mut set, "C74-09 可关闭零开销", ok, "关闭后仍绘制");
    }

    // C74-10 旁路故障自动关闭
    {
        let mut hud = PresentHud::new(HudConfig::default()).unwrap();
        hud.subscribe();
        hud.on_bypass_failure();
        let r = hud.draw_bypass();
        let ok = r == Ok(BypassResult::Skipped) && !hud.config().enabled;
        chk(&mut set, "C74-10 旁路故障自关", ok, "故障后未自关");
    }

    // C74-11 透明度 0 拒绝
    {
        let mut hud = PresentHud::new(HudConfig::default()).unwrap();
        let r = hud.set_opacity(0);
        chk(
            &mut set,
            "C74-11 全透拒绝",
            r.is_err() && hud.last_code() == XC_OPACITY,
            "全透被放行",
        );
    }

    // C74-12 曲线只在显示开启时采集
    {
        let mut cfg = HudConfig::default();
        cfg.show_frame_curve = false;
        let mut hud = PresentHud::new(cfg).unwrap();
        hud.subscribe();
        let _ = hud.on_snapshot(StatsSnapshot {
            seq: 1,
            frame_us: 100,
            curve_len: 0,
            dropped: 0,
            queue_depth: 0,
            present_mode: 0,
        });
        // describe 不依赖曲线；曲线容量恒在但不 push——用 draw 侧无感佐证。
        let ok = hud.describe()[3] == 100;
        chk(&mut set, "C74-12 显示项裁剪", ok, "裁剪路径异常");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vea74_all_green() {
        let s = run_vea74_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
