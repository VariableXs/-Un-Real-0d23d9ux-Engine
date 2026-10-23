//! VE-F0133 · 窗口遮挡事件联动（VE-A 域 · A06 环境与系统集成组 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0133`
//!
//! **职责定位（锚点原文）**：窗口被遮挡时的渲染联动（完全遮挡→暂停该窗口渲染），
//! 遮挡判定的保守策略（判定不确定时继续渲染），遮挡恢复即时性。
//!
//! **铁律**：
//! 1. **遮挡暂停**——完全遮挡才暂停（部分遮挡照常渲染）。
//! 2. **保守判定**——判定不确定时继续渲染（宁可多渲不黑屏）。
//! 3. **即时恢复**——遮挡解除立即恢复，无延迟帧。
//! 4. **误判回退**——误判遮挡（实际可见却暂停）检出后回退并校准。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址 0xE8CB（窗口遮挡联动）。
pub const SEG_BASE: u16 = 0xE8CB;
/// 误判遮挡（可见却暂停——红线）。
pub const XC_FALSE_OCCLUDE: u16 = SEG_BASE | 0x01;
/// 判定漂移（遮挡判定结果抖动）。
pub const XC_JUDGE_DRIFT: u16 = SEG_BASE | 0x02;
/// 恢复迟缓（解除后未即时恢复）。
pub const XC_RESUME_LATE: u16 = SEG_BASE | 0x03;
/// 非法输入。
pub const XC_BAD_INPUT: u16 = SEG_BASE | 0x04;

/// 遮挡判定结果（三态——不确定是合法态，不是错误）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OccludeVerdict {
    /// 确认可见（0% 遮挡）。
    Visible,
    /// 确认完全遮挡（100%）。
    FullyOccluded,
    /// 不确定（部分遮挡/判定噪声）——保守走「继续渲染」。
    Uncertain,
}

/// 遮挡判定输入（单窗口）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OccludeProbe {
    /// 窗口标识。
    pub window: u32,
    /// 遮挡面积百分比 0..=100。
    pub occluded_pct: u8,
    /// 判定置信度 0..=100（低置信=不确定）。
    pub confidence: u8,
}

/// 窗口渲染状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RenderState {
    /// 渲染中。
    Rendering,
    /// 已暂停（完全遮挡）。
    Paused,
}

/// 单窗口遮挡联动记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WinRecord {
    /// 窗口标识。
    pub window: u32,
    /// 渲染状态。
    pub state: RenderState,
    /// 暂停开始时刻（微秒，0=未暂停）。
    pub paused_at: u64,
    /// 误判累计。
    pub false_hits: u32,
}

/// 窗口遮挡事件联动器。
///
/// **保守策略实现**：判定函数把「不确定」映射为 Visible（继续渲染）——
/// 只有 occluded_pct==100 且 confidence>=阈值 才暂停。误判可被 `report_visible()`
/// 检出并回退。
pub struct OccludeLinker {
    /// 窗口记录表。
    wins: Vec<WinRecord>,
    /// 窗口容量。
    cap: usize,
    /// 判定置信度阈值（低于此值=不确定=继续渲染）。
    conf_threshold: u8,
    /// 误判总累计。
    false_total: u32,
    /// 判定漂移累计（同一窗口短时判定翻转）。
    drift_hits: u32,
    /// 最近诊断码。
    last_code: u16,
    /// 暂停窗口数（当前）。
    paused_now: u32,
}

impl OccludeLinker {
    /// 建联动器（容量>0，置信阈值 1..=100）。
    pub fn new(cap: usize, conf_threshold: u8) -> Result<Self, u16> {
        if cap == 0 || conf_threshold == 0 || conf_threshold > 100 {
            return Err(XC_BAD_INPUT);
        }
        Ok(OccludeLinker {
            wins: Vec::new(),
            cap,
            conf_threshold,
            false_total: 0,
            drift_hits: 0,
            last_code: 0,
            paused_now: 0,
        })
    }

    /// 最近诊断码。
    pub fn last_code(&self) -> u16 {
        self.last_code
    }

    /// 注册窗口。
    pub fn register(&mut self, window: u32) -> Result<(), u16> {
        if self.wins.iter().any(|w| w.window == window) {
            self.last_code = XC_BAD_INPUT;
            return Err(XC_BAD_INPUT);
        }
        if self.wins.len() >= self.cap {
            self.last_code = XC_BAD_INPUT;
            return Err(XC_BAD_INPUT);
        }
        self.wins.push(WinRecord {
            window,
            state: RenderState::Rendering,
            paused_at: 0,
            false_hits: 0,
        });
        self.last_code = 0;
        Ok(())
    }

    /// 判定遮挡（三态映射到暂停/继续）。
    ///
    /// 返回该窗口本次判定后的渲染状态。
    pub fn judge(&mut self, probe: OccludeProbe, now_us: u64) -> Result<RenderState, u16> {
        if probe.occluded_pct > 100 || probe.confidence > 100 {
            self.last_code = XC_BAD_INPUT;
            return Err(XC_BAD_INPUT);
        }
        let idx = match self.wins.iter().position(|w| w.window == probe.window) {
            Some(i) => i,
            None => {
                self.last_code = XC_BAD_INPUT;
                return Err(XC_BAD_INPUT);
            }
        };
        let verdict = self.verdict_of(probe);
        let before = self.wins[idx].state;
        match verdict {
            OccludeVerdict::FullyOccluded => {
                self.wins[idx].state = RenderState::Paused;
                self.wins[idx].paused_at = now_us;
            }
            OccludeVerdict::Visible | OccludeVerdict::Uncertain => {
                // 保守：不确定继续渲染；可见即恢复（即时性——恢复不迟疑）。
                self.wins[idx].state = RenderState::Rendering;
                self.wins[idx].paused_at = 0;
            }
        }
        // 判定漂移检测：暂停↔渲染短时翻转计一次漂移（抖动信号）。
        if before != self.wins[idx].state {
            self.drift_hits += 1;
        }
        self.recount_paused();
        self.last_code = 0;
        Ok(self.wins[idx].state)
    }

    /// 三态判定（保守映射）。
    fn verdict_of(&self, probe: OccludeProbe) -> OccludeVerdict {
        if probe.occluded_pct == 0 {
            return OccludeVerdict::Visible;
        }
        if probe.occluded_pct == 100 && probe.confidence >= self.conf_threshold {
            return OccludeVerdict::FullyOccluded;
        }
        // 部分遮挡 / 置信不足 → 不确定 → 继续渲染。
        OccludeVerdict::Uncertain
    }

    /// 外部纠错：上报「实际可见」（误判检出口）。
    ///
    /// 若窗口正处于 Paused 而外部确认可见 = 误判红线：回退恢复 + 计数。
    pub fn report_visible(&mut self, window: u32, now_us: u64) -> Result<(), u16> {
        let idx = match self.wins.iter().position(|w| w.window == window) {
            Some(i) => i,
            None => {
                self.last_code = XC_BAD_INPUT;
                return Err(XC_BAD_INPUT);
            }
        };
        if self.wins[idx].state == RenderState::Paused {
            self.wins[idx].state = RenderState::Rendering;
            self.wins[idx].paused_at = 0;
            self.wins[idx].false_hits += 1;
            self.false_total += 1;
            self.recount_paused();
            self.last_code = XC_FALSE_OCCLUDE;
            Err(XC_FALSE_OCCLUDE)
        } else {
            let _ = now_us;
            self.last_code = 0;
            Ok(())
        }
    }

    /// 恢复迟缓检出：暂停解除后超过 `budget_us` 仍为 Paused 即报。
    pub fn check_resume_latency(&mut self, window: u32, now_us: u64, budget_us: u64) -> Result<(), u16> {
        let idx = match self.wins.iter().position(|w| w.window == window) {
            Some(i) => i,
            None => {
                self.last_code = XC_BAD_INPUT;
                return Err(XC_BAD_INPUT);
            }
        };
        let w = self.wins[idx];
        if w.state == RenderState::Paused && w.paused_at != 0 && now_us.saturating_sub(w.paused_at) > budget_us {
            // 迟缓信号（注意：完全遮挡期暂停是合法的，此检查只在
            // 「应恢复未恢复」场景由上层在解除事件后调用）。
            self.last_code = XC_RESUME_LATE;
            return Err(XC_RESUME_LATE);
        }
        self.last_code = 0;
        Ok(())
    }

    /// 窗口状态查询。
    pub fn state_of(&self, window: u32) -> Option<RenderState> {
        self.wins.iter().find(|w| w.window == window).map(|w| w.state)
    }

    /// 当前暂停窗口数。
    pub fn paused_now(&self) -> u32 {
        self.paused_now
    }

    /// 误判总累计。
    pub fn false_total(&self) -> u32 {
        self.false_total
    }

    /// 判定漂移累计。
    pub fn drift_hits(&self) -> u32 {
        self.drift_hits
    }

    /// 某窗口误判累计。
    pub fn false_hits_of(&self, window: u32) -> u32 {
        self.wins
            .iter()
            .find(|w| w.window == window)
            .map(|w| w.false_hits)
            .unwrap_or(0)
    }

    /// 重新统计暂停窗口数。
    fn recount_paused(&mut self) {
        self.paused_now = self
            .wins
            .iter()
            .filter(|w| w.state == RenderState::Paused)
            .count() as u32;
    }

    /// 读屏行。
    pub fn status_line(&self) -> &'static str {
        if self.paused_now > 0 {
            "occlude:partial-paused"
        } else {
            "occlude:all-rendering"
        }
    }
}

/// 自检：遮挡暂停、保守判定、即时恢复、误判回退、恢复迟缓。
pub fn run_vea133_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }

    fn probe(window: u32, pct: u8, conf: u8) -> OccludeProbe {
        OccludeProbe {
            window,
            occluded_pct: pct,
            confidence: conf,
        }
    }

    let mut set = CheckSet::new("vea133_occlude");

    // C133-01 合法建
    {
        let l = OccludeLinker::new(8, 80);
        chk(&mut set, "C133-01 合法建", l.is_ok(), "合法参数被拒");
    }

    // C133-02 参数守卫
    {
        let a = OccludeLinker::new(0, 80).err() == Some(XC_BAD_INPUT);
        let b = OccludeLinker::new(8, 0).err() == Some(XC_BAD_INPUT);
        let c = OccludeLinker::new(8, 101).err() == Some(XC_BAD_INPUT);
        chk(&mut set, "C133-02 参数守卫", a && b && c, "非法参数被放行");
    }

    // C133-03 完全遮挡 → 暂停
    {
        let mut l = OccludeLinker::new(8, 80).unwrap();
        let _ = l.register(1);
        let r = l.judge(probe(1, 100, 90), 1000);
        let ok = r == Ok(RenderState::Paused) && l.paused_now() == 1;
        chk(&mut set, "C133-03 遮挡暂停", ok, "完全遮挡未暂停");
    }

    // C133-04 保守判定：置信不足不暂停
    {
        let mut l = OccludeLinker::new(8, 80).unwrap();
        let _ = l.register(1);
        let r = l.judge(probe(1, 100, 50), 1000);
        let ok = r == Ok(RenderState::Rendering);
        chk(&mut set, "C133-04 保守判定", ok, "低置信误暂停");
    }

    // C133-05 部分遮挡不暂停（只有完全遮挡才停）
    {
        let mut l = OccludeLinker::new(8, 80).unwrap();
        let _ = l.register(1);
        let r = l.judge(probe(1, 60, 95), 1000);
        let ok = r == Ok(RenderState::Rendering);
        chk(&mut set, "C133-05 部分遮挡", ok, "部分遮挡被误停");
    }

    // C133-06 即时恢复：遮挡解除立即渲染
    {
        let mut l = OccludeLinker::new(8, 80).unwrap();
        let _ = l.register(1);
        let _ = l.judge(probe(1, 100, 90), 1000);
        let r = l.judge(probe(1, 0, 95), 2000);
        let ok = r == Ok(RenderState::Rendering) && l.paused_now() == 0;
        chk(&mut set, "C133-06 即时恢复", ok, "解除后未即时恢复");
    }

    // C133-07 误判回退：外部确认可见 → 回退+计数
    {
        let mut l = OccludeLinker::new(8, 80).unwrap();
        let _ = l.register(1);
        let _ = l.judge(probe(1, 100, 90), 1000);
        let r = l.report_visible(1, 2000);
        let ok = r.err() == Some(XC_FALSE_OCCLUDE)
            && l.false_total() == 1
            && l.state_of(1) == Some(RenderState::Rendering);
        chk(&mut set, "C133-07 误判回退", ok, "误判未回退");
    }

    // C133-08 判定漂移统计
    {
        let mut l = OccludeLinker::new(8, 80).unwrap();
        let _ = l.register(1);
        let _ = l.judge(probe(1, 100, 90), 1000);
        let _ = l.judge(probe(1, 0, 95), 1100);
        let _ = l.judge(probe(1, 100, 90), 1200);
        let ok = l.drift_hits() >= 2;
        chk(&mut set, "C133-08 判定漂移", ok, "翻转未统计");
    }

    // C133-09 恢复迟缓检出
    {
        let mut l = OccludeLinker::new(8, 80).unwrap();
        let _ = l.register(1);
        let _ = l.judge(probe(1, 100, 90), 1000);
        let r = l.check_resume_latency(1, 5000, 1000);
        let ok = r.err() == Some(XC_RESUME_LATE);
        chk(&mut set, "C133-09 恢复迟缓", ok, "迟缓未检出");
    }

    // C133-10 未注册窗口拒绝
    {
        let mut l = OccludeLinker::new(8, 80).unwrap();
        let r = l.judge(probe(99, 100, 90), 1000);
        let ok = r.err() == Some(XC_BAD_INPUT);
        chk(&mut set, "C133-10 注册守卫", ok, "未注册窗口被放行");
    }

    set
}
