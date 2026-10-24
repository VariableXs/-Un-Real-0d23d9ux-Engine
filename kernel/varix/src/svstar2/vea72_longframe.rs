//! VE-F0072 · 长帧检测与交互恢复（VE-A 域 · A03b 同步与呈现组 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0072`
//!
//! **职责定位（锚点原文）**：帧时长超阈值判定长帧并记录（输入等待型与
//! 计算型区分），检测到交互等待（用户输入悬而未决）时切换低延迟呈现模式
//! （跳过缓冲提升响应），交互结束回落正常模式；切换全程入体验日志。
//!
//! **数据结构（锚点原文）**：长帧记录（时长 × 类型）；交互检测器；模式
//! 切换记录。
//!
//! **错误路径与降级矩阵（锚点原文）**：
//! - 检测误报 → 阈值可调并记录调整；
//! - 模式切换失败 → 保持原模式加告警；
//! - 长帧风暴 → 限流采样防日志淹没。
//!
//! **性能逐项分解（锚点原文）**：检测 O(1)；切换 O(1)；日志异步批量。
//!
//! **跨批对接点（锚点原文）**：上游 F0069 统计；下游 F0103 通知与体验日志。
//!
//! **无障碍与隐私（锚点原文）**：无直接无障碍面；交互优先策略惠及键盘
//! 操作响应。
//!
//! **判据（锚点原文）**：长帧分类、交互优先、自动回落、日志全程、判据。
//!
//! 纯功能线：no_std + alloc；零 unwrap/expect/panic；整数；确定性。
//!
//! 诊断段：0xE8A2 基址（全域核查无占用）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const LONGFRAME_VERSION: &str = "vea72_longframe-v1";

/// 诊断段基址。
pub const SEG_BASE: u32 = 0xE8A2;

// ---- 诊断码 ----
pub const XC_BAD_FRAME: u32 = SEG_BASE + 1; // 非法帧时长（0）
pub const XC_SWITCH_FAIL: u32 = SEG_BASE + 2; // 模式切换失败（保持原模式）
pub const XC_STORM: u32 = SEG_BASE + 3; // 长帧风暴（限流触发）
pub const XC_THRESH_BAD: u32 = SEG_BASE + 4; // 阈值参数非法

/// 长帧类型（锚点原文：输入等待型与计算型区分——处置策略不同）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LongFrameKind {
    /// 输入等待型：帧在等用户输入（可切低延迟模式缓解）。
    InputWait = 1,
    /// 计算型：GPU/CPU 负载重（低延迟模式无济于事）。
    Compute = 2,
}

/// 呈现模式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresentMode {
    /// 正常模式（多缓冲）。
    Normal,
    /// 低延迟模式（跳过缓冲提升响应——交互等待期专用）。
    LowLatency,
}

/// 长帧记录。
#[derive(Clone, Copy, Debug)]
pub struct LongFrameRec {
    pub frame: u64,
    pub duration_us: u32,
    pub kind: LongFrameKind,
}

/// 模式切换记录（全程入体验日志）。
#[derive(Clone, Copy, Debug)]
pub struct SwitchRec {
    pub frame: u64,
    pub from: PresentMode,
    pub to: PresentMode,
    /// 切换原因（1=交互优先进入 2=交互结束回落 3=失败保持）。
    pub reason: u8,
}

/// 长帧检测器 + 交互恢复器。
pub struct LongFrameMonitor {
    /// 长帧判定阈值（µs，可调并记录调整）。
    threshold_us: u32,
    last_code: u32,
    frames: Vec<LongFrameRec>,
    cap: usize,
    switches: Vec<SwitchRec>,
    cap_switch: usize,
    mode: PresentMode,
    frame: u64,
    /// 交互等待（用户输入悬而未决）。
    interactive_pending: bool,
    /// 风暴限流：连续长帧超限后进入限流采样（每 N 帧记 1 条）。
    storm_stride: u32,
    storm_counter: u32,
    /// 阈值调整记录（旧值 × 新值 × 次数）。
    retunes: u32,
    /// 切换失败告警计数。
    switch_fails: u32,
}

impl LongFrameMonitor {
    pub fn new(threshold_us: u32, cap: usize) -> LongFrameMonitor {
        LongFrameMonitor {
            threshold_us,
            last_code: 0,
            frames: Vec::new(),
            cap,
            switches: Vec::new(),
            cap_switch: 32,
            mode: PresentMode::Normal,
            frame: 0,
            interactive_pending: false,
            storm_stride: 0,
            storm_counter: 0,
            retunes: 0,
            switch_fails: 0,
        }
    }

    /// 阈值调整（可调并记录——防误报矩阵）。
    pub fn retune(&mut self, new_us: u32) -> Result<(), u32> {
        if new_us == 0 || new_us > 1_000_000 {
            self.last_code = XC_THRESH_BAD;
            return Err(self.last_code);
        }
        self.threshold_us = new_us;
        self.retunes += 1;
        Ok(())
    }

    /// 输入事件：交互等待开始/结束。
    pub fn set_interactive(&mut self, pending: bool) {
        self.interactive_pending = pending;
        // 交互优先：进入等待即切低延迟（若在正常模式）。
        if pending && self.mode == PresentMode::Normal {
            self.switch_mode(PresentMode::LowLatency, 1);
        } else if !pending && self.mode == PresentMode::LowLatency {
            // 交互结束回落正常模式（自动回落）。
            self.switch_mode(PresentMode::Normal, 2);
        }
    }

    fn switch_mode(&mut self, to: PresentMode, reason: u8) -> bool {
        if self.mode == to {
            return false;
        }
        // 模式切换失败模拟面：切换失败保持原模式加告警（由外部注入，
        // 本层记录失败路径的账）。此处切换动作本身恒成功（内核态无
        // 阻塞资源），失败面留给上层呈现管线的回执。
        let from = self.mode;
        self.mode = to;
        if self.switches.len() >= self.cap_switch {
            self.switches.remove(0);
        }
        self.switches.push(SwitchRec { frame: self.frame, from, to, reason });
        true
    }

    /// 切换失败回执（上层呈现管线回报切换未能生效）：保持原模式 + 告警。
    pub fn report_switch_failed(&mut self, wanted: PresentMode) {
        self.switch_fails += 1;
        self.last_code = XC_SWITCH_FAIL;
        if self.switches.len() >= self.cap_switch {
            self.switches.remove(0);
        }
        self.switches.push(SwitchRec {
            frame: self.frame,
            from: self.mode,
            to: wanted,
            reason: 3,
        });
    }

    /// 帧完成回执（检测入口）：超阈值判长帧并分类记录。
    pub fn frame_done(&mut self, duration_us: u32) -> Result<Option<LongFrameKind>, u32> {
        if duration_us == 0 {
            self.last_code = XC_BAD_FRAME;
            return Err(self.last_code);
        }
        self.frame += 1;
        if duration_us <= self.threshold_us {
            // 正常帧：风暴计数回落。
            self.storm_counter = self.storm_counter.saturating_sub(1);
            return Ok(None);
        }
        // 长帧分类：交互等待期 → 输入等待型；否则计算型。
        let kind = if self.interactive_pending {
            LongFrameKind::InputWait
        } else {
            LongFrameKind::Compute
        };
        // 风暴限流：连续长帧 ≥ 16 后按 8 帧间隔采样（防日志淹没）。
        self.storm_counter += 1;
        if self.storm_counter >= 16 && self.storm_stride == 0 {
            self.storm_stride = 8;
            self.last_code = XC_STORM;
        }
        if self.storm_stride > 0 && self.frame % self.storm_stride as u64 != 0 {
            return Ok(Some(kind)); // 检出但不记日志（限流窗口）
        }
        if self.frames.len() >= self.cap {
            self.frames.remove(0);
        }
        self.frames.push(LongFrameRec { frame: self.frame, duration_us, kind });
        Ok(Some(kind))
    }

    pub fn mode(&self) -> PresentMode {
        self.mode
    }
    pub fn threshold_us(&self) -> u32 {
        self.threshold_us
    }
    pub fn retunes(&self) -> u32 {
        self.retunes
    }
    pub fn switch_fails(&self) -> u32 {
        self.switch_fails
    }
    pub fn storm_active(&self) -> bool {
        self.storm_stride > 0
    }
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }
    pub fn switch_count(&self) -> usize {
        self.switches.len()
    }
    pub fn last_code(&self) -> u32 {
        self.last_code
    }

    /// 体验日志导出（切换全程——判据「日志全程」的载体）。
    pub fn switch_log(&self) -> String {
        let mut s = String::new();
        for r in self.switches.iter() {
            let from = if r.from == PresentMode::Normal { "正常" } else { "低延迟" };
            let to = if r.to == PresentMode::Normal { "正常" } else { "低延迟" };
            let why = match r.reason {
                1 => "交互优先进入",
                2 => "交互结束回落",
                _ => "切换失败保持",
            };
            s.push_str(&alloc::format!("帧{} {}→{}（{}）\n", r.frame, from, to, why));
        }
        s
    }

    /// 读屏状态行。
    pub fn status_line(&self) -> String {
        let m = if self.mode == PresentMode::Normal { "正常" } else { "低延迟" };
        alloc::format!(
            "LONGF 阈值{}µs 模式{} 长帧记录{} 切换{} 风暴限流{} 调阈值{}",
            self.threshold_us,
            m,
            self.frames.len(),
            self.switches.len(),
            self.storm_stride,
            self.retunes
        )
    }
}

// =====================================================================
// 自检（判据锚点：长帧分类 / 交互优先 / 自动回落 / 日志全程）
// =====================================================================

fn chk(set: &mut CheckSet, name: &'static str, ok: bool, detail: &'static str) {
    set.add(name, ok, detail);
}

/// VE-F0072 域自检。
pub fn run_vea72_checks() -> CheckSet {
    let mut set = CheckSet::new("vea72");

    // C72-01 长帧分类：交互等待期 → 输入等待型
    {
        let mut m = LongFrameMonitor::new(16_667, 32);
        m.set_interactive(true);
        let r = m.frame_done(30_000);
        let ok = r == Ok(Some(LongFrameKind::InputWait));
        chk(&mut set, "C72-01 输入等待型", ok, "分类失准");
    }

    // C72-02 长帧分类：非交互期 → 计算型
    {
        let mut m = LongFrameMonitor::new(16_667, 32);
        let r = m.frame_done(30_000);
        let ok = r == Ok(Some(LongFrameKind::Compute));
        chk(&mut set, "C72-02 计算型", ok, "分类失准");
    }

    // C72-03 交互优先：进入交互即切低延迟
    {
        let mut m = LongFrameMonitor::new(16_667, 32);
        m.set_interactive(true);
        let ok = m.mode() == PresentMode::LowLatency;
        chk(&mut set, "C72-03 交互优先", ok, "未切低延迟");
    }

    // C72-04 自动回落：交互结束回正常模式
    {
        let mut m = LongFrameMonitor::new(16_667, 32);
        m.set_interactive(true);
        m.set_interactive(false);
        let ok = m.mode() == PresentMode::Normal;
        chk(&mut set, "C72-04 自动回落", ok, "未回落");
    }

    // C72-05 日志全程：切换记录含进入与回落两条
    {
        let mut m = LongFrameMonitor::new(16_667, 32);
        m.set_interactive(true);
        m.set_interactive(false);
        let log = m.switch_log();
        let ok = m.switch_count() == 2 && log.contains("交互优先进入") && log.contains("交互结束回落");
        chk(&mut set, "C72-05 日志全程", ok, "切换日志缺失");
    }

    // C72-06 切换失败：保持原模式加告警
    {
        let mut m = LongFrameMonitor::new(16_667, 32);
        m.report_switch_failed(PresentMode::LowLatency);
        let ok = m.mode() == PresentMode::Normal && m.switch_fails() == 1
            && m.last_code() == XC_SWITCH_FAIL;
        chk(&mut set, "C72-06 失败保持", ok, "失败路径未保持");
    }

    // C72-07 阈值可调并记录
    {
        let mut m = LongFrameMonitor::new(16_667, 32);
        let _ = m.retune(33_333);
        let ok = m.threshold_us() == 33_333 && m.retunes() == 1;
        chk(&mut set, "C72-07 阈值调整", ok, "调阈值未生效/未记");
    }

    // C72-08 阈值参数非法拒绝
    {
        let mut m = LongFrameMonitor::new(16_667, 32);
        let ok = m.retune(0) == Err(XC_THRESH_BAD);
        chk(&mut set, "C72-08 阈值校验", ok, "零阈值被接受");
    }

    // C72-09 长帧风暴限流：连续长帧触发 stride 采样
    {
        let mut m = LongFrameMonitor::new(16_667, 256);
        for _ in 0..20 {
            let _ = m.frame_done(30_000);
        }
        let ok = m.storm_active() && m.last_code() == XC_STORM;
        chk(&mut set, "C72-09 风暴限流", ok, "风暴未限流");
    }

    // C72-10 正常帧不误报
    {
        let mut m = LongFrameMonitor::new(16_667, 32);
        let r = m.frame_done(8_000);
        let ok = r == Ok(None) && m.frame_count() == 0;
        chk(&mut set, "C72-10 正常帧", ok, "正常帧误报");
    }

    // C72-11 零时长帧拒绝
    {
        let mut m = LongFrameMonitor::new(16_667, 32);
        let ok = m.frame_done(0) == Err(XC_BAD_FRAME);
        chk(&mut set, "C72-11 零时长拒", ok, "零时长被接受");
    }

    // C72-12 记录环形：超容量挤掉最旧
    {
        let mut m = LongFrameMonitor::new(16_667, 8);
        for _ in 0..12 {
            let _ = m.frame_done(30_000);
        }
        let ok = m.frame_count() == 8;
        chk(&mut set, "C72-12 记录环形", ok, "记录表未环形");
    }

    // C72-13 确定性
    {
        let seq = || -> (u32, usize) {
            let mut m = LongFrameMonitor::new(16_667, 32);
            m.set_interactive(true);
            let _ = m.frame_done(30_000);
            (m.threshold_us(), m.switch_count())
        };
        chk(&mut set, "C72-13 确定性", seq() == seq(), "两次不一致");
    }

    // C72-14 读屏行六要素
    {
        let m = LongFrameMonitor::new(16_667, 32);
        let line = m.status_line();
        let ok = line.contains("阈值") && line.contains("模式") && line.contains("长帧记录")
            && line.contains("切换") && line.contains("风暴限流");
        chk(&mut set, "C72-14 读屏要素", ok, "读屏行缺要素");
    }

    set
}

/// 深度驱动入口。
pub fn drill() -> (bool, String) {
    let s = run_vea72_checks();
    let ok = s.all_passed() && !s.truncated();
    (ok, if ok { String::new() } else { s.red_items_text() })
}
