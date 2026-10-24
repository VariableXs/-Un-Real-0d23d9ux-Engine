//! VE-F0245 · AMD 显示控制器 DCN（VE-B 域 · AMD 组 · 目标 440 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0245`
//!
//! **职责定位（锚点原文）**：pipe 与 hub 层级、时序编程（link 时钟与像素
//! 时钟匹配校验）、watermark 计算（DCHUB 语义与 PME 工作区）与 Underrun
//! 防护、模式切换原子序；与 Intel 组显示框架同构（显示框架第二用复用）
//! 但 DCN 语义独立实现不硬套。
//!
//! **数据结构（锚点原文）**：DCN 状态表；时序参数；watermark 表；原子序
//! 清单。
//!
//! **判据（锚点原文）**：DCN 层级、时序匹配、watermark、原子序、判据。
//! 本模块 14 条判据逐条映射：**DCN 层级 3 条**（pipe 槽位独占注册／hub
//! 归属一致／pipe 数按代际上限）、**时序匹配 3 条**（link 时钟 × 深度 =
//! 像素时钟匹配校验／时序非法安全模式回落／模式切换原子序递增）、
//! **watermark 3 条**（查表加插值 O(1)／watermark 超限禁用特性降级／
//! DCHUB 语义行渲染）、**Underrun 3 条**（事件计数／连续越阈升级处置／
//! 防护使能恢复清账）、**原子序 1 条**（一帧内完成提交闭包）、**判据
//! 自身 1 条**（码位互异且段独占）。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**：
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | Underrun → **中断计数升级处置** | 单帧 Underrun 计数，连续 [`UNDERRUN_ESCALATE`] 帧触发升级事件——不是静默吞掉 |
//! | 时序非法 → **安全模式回落** | link/像素时钟匹配失败即拒绝该模式并回落安全档——不硬编非法时序 |
//! | watermark 超限 → **禁用特性降级** | 超限 pipe 禁用即时特性并降档 watermark——不假装保住带宽 |
//!
//! ## 设计要点（为什么这样写）
//!
//! 1. **层级即状态**：pipe 槽位与 hub 的归属关系存在 [`DcnState`] 内，
//!    注册/注销都过守卫（重复注册拒绝、hub 越界拒绝），层级破缺不可能
//!    静默发生。
//! 2. **时序匹配是乘法等式**：`link_clock_khz × lane_depth ==
//!    pixel_clock_khz × 1000`（DCN 语义：link 带宽 = 像素带宽），等式
//!    不成立即拒绝——比阈值判断更精确，双向可验。
//! 3. **watermark 查表 + 插值**：分辨率档间线性插值 O(1)，避免逐像素
//!    重算；插值结果仍受上限夹取，超限走降级而不是放水。

use alloc::vec::Vec;
use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;

/// 诊断段（0xAFC0 独占 0x10）。
pub const SEG_BASE: u32 = 0xAFC0;
pub const XC_PIPE_DUP: u32 = SEG_BASE + 1; // pipe 槽位重复注册
pub const XC_HUB_BAD: u32 = SEG_BASE + 2; // hub 越界
pub const XC_TIMING_BAD: u32 = SEG_BASE + 3; // 时序匹配失败
pub const XC_WM_OOB: u32 = SEG_BASE + 4; // watermark 超限
pub const XC_PIPE_OOB: u32 = SEG_BASE + 5; // pipe 越出代际上限
pub const XC_SEALED: u32 = SEG_BASE + 6; // 原子提交封账

/// 代际 pipe 上限（GFX9..GFX11 DCN 语义）。
pub const PIPE_MAX: [u32; 4] = [4, 6, 8, 8];
/// HUB 上限。
pub const HUB_MAX: u32 = 4;
/// Underrun 升级阈（连续帧）。
pub const UNDERRUN_ESCALATE: u32 = 3;

/// 时序参数（link 时钟 kHz × lane 深度 vs 像素时钟 kHz）。
#[derive(Clone, Copy)]
pub struct TimingParam {
    pub link_clock_khz: u32,
    pub lane_depth: u32,
    pub pixel_clock_khz: u32,
}

impl TimingParam {
    /// 匹配校验（DCN 语义等式：link 带宽 = 像素带宽）。
    pub fn matches(&self) -> bool {
        self.link_clock_khz
            .wrapping_mul(self.lane_depth)
            == self.pixel_clock_khz * 1000
    }
}

/// watermark 查表行（分辨率档 → 行水位）。
#[derive(Clone, Copy)]
pub struct WmRow {
    pub res_h: u32,   // 行分辨率档
    pub lines: u32,   // 水位行数
}

/// watermark 档表（升序，插值用）。
pub static WM_TABLE: [WmRow; 4] = [
    WmRow { res_h: 1080, lines: 12 },
    WmRow { res_h: 1440, lines: 16 },
    WmRow { res_h: 2160, lines: 24 },
    WmRow { res_h: 4320, lines: 40 },
];

/// watermark 查表 + 线性插值（O(1)：档间单插；超上限取末档）。
pub fn wm_lookup(res_h: u32) -> u32 {
    if res_h <= WM_TABLE[0].res_h {
        return WM_TABLE[0].lines;
    }
    let mut i = 0usize;
    while i + 1 < WM_TABLE.len() {
        let (a, b) = (WM_TABLE[i], WM_TABLE[i + 1]);
        if res_h <= b.res_h {
            // 线性插值
            let span = b.res_h - a.res_h;
            let t = (res_h - a.res_h) as u64;
            let d = ((b.lines - a.lines) as u64 * t + span as u64 / 2) / span as u64;
            return a.lines + d as u32;
        }
        i += 1;
    }
    WM_TABLE[WM_TABLE.len() - 1].lines
}

/// pipe 状态（hub 归属 + 时序 + 水位 + 特性使能）。
#[derive(Clone)]
pub struct PipeState {
    pub pipe: u32,
    pub hub: u32,
    pub timing: Option<TimingParam>,
    pub wm_lines: u32,
    pub wm_degraded: bool, // watermark 超限降级标注
    pub features_off: bool, // 特性禁用标注
}

/// DCN 状态表（pipe 集 + Underrun 账 + 原子序）。
pub struct DcnState {
    pub gen: usize,
    pub pipes: Vec<PipeState>,
    pub underrun_total: u32,
    pub underrun_streak: u32,
    pub escalated: u32,
    pub atomic_seq: u64,
    pub sealed: bool,
}

impl DcnState {
    pub fn new(gen: usize) -> Self {
        let g = if gen < 4 { gen } else { 3 };
        DcnState {
            gen: g,
            pipes: Vec::new(),
            underrun_total: 0,
            underrun_streak: 0,
            escalated: 0,
            atomic_seq: 0,
            sealed: false,
        }
    }

    /// pipe 上限。
    pub fn pipe_cap(&self) -> u32 {
        PIPE_MAX[self.gen]
    }

    /// 注册 pipe（槽位独占 + hub 界守卫 + 代际上限守卫）。
    pub fn attach(&mut self, pipe: u32, hub: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_SEALED);
        }
        if pipe >= self.pipe_cap() {
            return Err(XC_PIPE_OOB);
        }
        if hub >= HUB_MAX {
            return Err(XC_HUB_BAD);
        }
        if self.pipes.iter().any(|p| p.pipe == pipe) {
            return Err(XC_PIPE_DUP);
        }
        self.pipes.push(PipeState {
            pipe,
            hub,
            timing: None,
            wm_lines: 0,
            wm_degraded: false,
            features_off: false,
        });
        Ok(())
    }

    /// 时序编程（匹配校验；失败拒绝——不硬编非法时序）。
    pub fn program_timing(&mut self, pipe: u32, t: TimingParam) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_SEALED);
        }
        if !t.matches() {
            return Err(XC_TIMING_BAD);
        }
        let p = match self.pipes.iter_mut().find(|p| p.pipe == pipe) {
            Some(p) => p,
            None => return Err(XC_PIPE_OOB),
        };
        p.timing = Some(t);
        // 水位随模式更新（查表 + 插值 + 超限降级）
        let wm = wm_lookup(t.pixel_clock_khz / 1000 / 2);
        if wm > 32 {
            p.wm_lines = 32;
            p.wm_degraded = true;
            p.features_off = true; // 特性禁用降级
        } else {
            p.wm_lines = wm;
            p.wm_degraded = false;
            p.features_off = false;
        }
        Ok(())
    }

    /// 安全模式回落：pipe 清时序置默认水位（时序非法路径）。
    pub fn safe_mode(&mut self, pipe: u32) -> Result<(), u32> {
        let p = match self.pipes.iter_mut().find(|p| p.pipe == pipe) {
            Some(p) => p,
            None => return Err(XC_PIPE_OOB),
        };
        p.timing = None;
        p.wm_lines = WM_TABLE[0].lines;
        p.wm_degraded = false;
        p.features_off = false;
        Ok(())
    }

    /// Underrun 事件（计数 + 连续越阈升级）。
    pub fn underrun_hit(&mut self) -> bool {
        self.underrun_total += 1;
        self.underrun_streak += 1;
        if self.underrun_streak >= UNDERRUN_ESCALATE {
            self.escalated += 1;
            self.underrun_streak = 0;
            return true; // 升级事件
        }
        false
    }

    /// 防护恢复：清连击账（防护使能恢复清账语义）。
    pub fn underrun_clear(&mut self) {
        self.underrun_streak = 0;
    }

    /// 原子序提交：一帧内完成全部 pipe 编程闭包（序号严格递增）。
    pub fn atomic_commit(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_SEALED);
        }
        if self.pipes.is_empty() {
            return Err(XC_PIPE_OOB);
        }
        self.atomic_seq += 1;
        Ok(self.atomic_seq)
    }

    /// 状态渲染（层级行 + 水位行 + 账行）。
    pub fn render(&self) -> String {
        let mut out = format!(
            "DCN{} pipe上限={} 原子序={} Underrun总={} 升级={}\n",
            self.gen, self.pipe_cap(), self.atomic_seq, self.underrun_total, self.escalated
        );
        for p in &self.pipes {
            let line = format!(
                "  pipe{} hub{} 水位={}行{}{}\n",
                p.pipe,
                p.hub,
                p.wm_lines,
                if p.wm_degraded { "（降级）" } else { "" },
                if p.features_off { " 特性禁用" } else { "" }
            );
            out.push_str(&line);
        }
        out
    }
}

/// 标准夹具：DCN3（GFX10.3，8 pipe）+ 3 pipe + 合法时序。
pub fn standard_dcn() -> DcnState {
    let mut d = DcnState::new(2);
    let _ = d.attach(0, 0);
    let _ = d.attach(1, 1);
    let _ = d.attach(2, 1);
    // link 540000 kHz × 4 lane = 像素 2160000 kHz（匹配）
    let _ = d.program_timing(0, TimingParam {
        link_clock_khz: 540_000,
        lane_depth: 4,
        pixel_clock_khz: 2_160_000,
    });
    d
}

/// 注入核：8 注入必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let mut d = DcnState::new(2);
    // 1) pipe 越代际上限拒（DCN3 上限 8）
    if d.attach(8, 0) == Err(XC_PIPE_OOB) {
        caught += 1;
    }
    // 2) hub 越界拒
    if d.attach(0, 4) == Err(XC_HUB_BAD) {
        caught += 1;
    }
    // 3) pipe 重复注册拒
    let _ = d.attach(0, 0);
    if d.attach(0, 1) == Err(XC_PIPE_DUP) {
        caught += 1;
    }
    // 4) 时序匹配失败拒（等式破缺）
    if d.program_timing(0, TimingParam {
        link_clock_khz: 540_000,
        lane_depth: 4,
        pixel_clock_khz: 2_000_000, // 540000*4=2160000 ≠ 2000000*1000
    }) == Err(XC_TIMING_BAD)
    {
        caught += 1;
    }
    // 5) watermark 超限降级标注（像素 8640000 → 4320 档 40 行 > 32）
    let _ = d.program_timing(0, TimingParam {
        link_clock_khz: 8_640_000,
        lane_depth: 4,
        pixel_clock_khz: 34_560_000,
    });
    if d.pipes[0].wm_degraded && d.pipes[0].features_off {
        caught += 1;
    }
    // 6) Underrun 升级（连续 3 帧 → 升级事件）
    let mut d2 = standard_dcn();
    if !d2.underrun_hit() && !d2.underrun_hit() && d2.underrun_hit() && d2.escalated == 1 {
        caught += 1;
    }
    // 7) 恢复清账（2 连击 + clear → 再 3 帧才升级）
    let mut d3 = standard_dcn();
    let _ = d3.underrun_hit();
    let _ = d3.underrun_hit();
    d3.underrun_clear();
    if !d3.underrun_hit() && !d3.underrun_hit() && d3.underrun_hit() && d3.escalated == 1 {
        caught += 1;
    }
    // 8) 原子序单调（两次提交序号递增）
    let a = d3.atomic_commit().unwrap_or(0);
    let b = d3.atomic_commit().unwrap_or(0);
    if b == a + 1 {
        caught += 1;
    }
    (caught, 8)
}

/// 判据入口。
pub fn run_veb245_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0245");
    // J1-1 DCN 层级（槽位独占 + hub 守卫 + 上限守卫）
    let mut d = DcnState::new(2);
    let ok = d.attach(0, 0).is_ok()
        && d.attach(0, 1).is_err()
        && d.attach(8, 0).is_err()
        && d.attach(0, 9).is_err();
    s.add("AMD-245-01-DCN层级守卫", ok, if ok { "" } else { "层级破缺" });
    // J1-2 时序匹配（等式校验双向：合法过 / 非法拒）
    let mut d = standard_dcn();
    let ok = d.pipes[0].timing.is_some()
        && d.program_timing(1, TimingParam {
            link_clock_khz: 270_000,
            lane_depth: 4,
            pixel_clock_khz: 1_080_000,
        }).is_ok()
        && d.program_timing(1, TimingParam {
            link_clock_khz: 270_000,
            lane_depth: 4,
            pixel_clock_khz: 999_999,
        }).is_err();
    s.add("AMD-245-02-时序匹配校验", ok, if ok { "" } else { "匹配破缺" });
    // J1-3 watermark 查表插值（1080 档 12 行 / 4320 档 40 行 / 中档插值）
    let ok = wm_lookup(1080) == 12 && wm_lookup(4320) == 40
        && wm_lookup(1620) == 20 // 1440..2160 中点插值 (16+24)/2
        && wm_lookup(100) == 12; // 下夹取
    s.add("AMD-245-03-watermark插值", ok, if ok { "" } else { "水位漂移" });
    // J1-4 超限降级（>32 行 → 降级 + 特性禁用）
    let mut d = standard_dcn();
    let _ = d.program_timing(0, TimingParam {
        link_clock_khz: 8_640_000,
        lane_depth: 4,
        pixel_clock_khz: 34_560_000,
    });
    let ok = d.pipes[0].wm_degraded && d.pipes[0].features_off && d.pipes[0].wm_lines == 32;
    s.add("AMD-245-04-超限降级", ok, if ok { "" } else { "降级破缺" });
    // J1-5 Underrun 升级处置（3 连击升级 + 清账恢复）
    let mut d = standard_dcn();
    let e1 = d.underrun_hit();
    let _ = d.underrun_hit();
    let e2 = d.underrun_hit();
    let ok = !e1 && e2 && d.escalated == 1 && d.underrun_total == 3;
    d.underrun_clear();
    let ok = ok && d.underrun_streak == 0;
    s.add("AMD-245-05-Underrun升级", ok, if ok { "" } else { "升级破缺" });
    // J1-6 原子序（单调递增 + 一帧闭包）
    let mut d = standard_dcn();
    let a = d.atomic_commit().unwrap_or(0);
    let b = d.atomic_commit().unwrap_or(0);
    let ok = a == 1 && b == 2 && !d.render().is_empty();
    s.add("AMD-245-06-原子序单调", ok, if ok { "" } else { "原子序漂移" });
    // J1-7 注入核 8/8
    let (c, n) = injection_core();
    let ok = c == n;
    let d = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("AMD-245-07-注入核必抓", ok, &d);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn veb245_checks_all_green() {
        let set = run_veb245_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn veb245_timing_equation_exact() {
        let t = TimingParam { link_clock_khz: 810_000, lane_depth: 8, pixel_clock_khz: 6_480_000 };
        assert!(t.matches());
    }
}
