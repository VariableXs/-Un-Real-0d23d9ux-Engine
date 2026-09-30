//! F039 深化批次三 · 交接会话面（compatstar2/deep2 · G-A-39）。
//!
//! 批次一深化覆盖 D3D 库词表/显存预估/独占全屏闸/交接参数打包，批次二
//! 覆盖 exe 识别/全屏降级链/回程四步账/分辨率安全交集；本批补齐交接
//! 会话全语义对齐的执行/边界/注入面：DXGI 模式枚举模型（定长 16 模式表
//! 按像素数降序冒泡定序——MS IDXGIOutput::GetDisplayModeList 枚举语义
//! 对拍）、全屏独占协商状态机（windowed→requesting→exclusive→lost→
//! restoring 五态，lost 强制出独占并记账，非法转换拒绝）、四步交接分段
//! 计时账（4+8+3+15s 四段预算，逐段实测入账与超段告警——主册 gamefront
//! STEP_BUDGETS_S 同源）、回滚帧率账（回程后采样 10 帧均值 vs 基线
//! 80fps，恢复判定 ≥95% 基线，定长 10 采样环）。
//!
//! 判据对账：主册 G-A-39 判据「双击游戏到 Windows 游戏画面全流程 ≤30s」
//! 分段对账 +【设计细节】「回来路径对称」的量化验收面。
//! 零堆纪律：定长模式表 + 定长采样环，无 alloc。

use crate::checks::CheckSet;

/// 模式表容量 16（DXGI 枚举定长口径——MS GetDisplayModeList 语义对拍）。
pub const MODE_CAP: usize = 16;
/// 交接分段预算（主册 gamefront STEP_BUDGETS_S 同源：4+8+3+15 = 30s）。
pub const STEP_BUDGETS_S: [u64; 4] = [4, 8, 3, 15];
/// 全程预算 30s（主册判据「全流程 ≤30s」）。
pub const HANDOFF_TOTAL_S: u64 = 30;
/// 回滚基线帧率 80fps（域内回程验收口径）。
pub const ROLLBACK_BASELINE_FPS: u32 = 80;
/// 恢复判定线 95% 基线（permille 950）。
pub const ROLLBACK_RECOVER_PERMILLE: u32 = 950;
/// 回滚采样环 10 帧。
pub const FPS_RING_CAP: usize = 10;

/// 一个显示模式（分辨率 × 刷新率）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DisplayMode {
    pub w: u32,
    pub h: u32,
    pub hz: u32,
}

impl DisplayMode {
    /// 像素数（降序定序主键）。
    pub fn pixels(&self) -> u64 {
        self.w as u64 * self.h as u64
    }
}

/// DXGI 模式枚举模型：定长 16，按像素数降序冒泡定序（同像素按刷新率
/// 降序——高分优先的枚举呈现序）。
pub struct ModeTable {
    pub modes: [DisplayMode; MODE_CAP],
    pub count: usize,
    /// 表满后的溢出拒绝数（显性化账面）。
    pub overflow: u32,
}

impl ModeTable {
    pub const fn new() -> Self {
        ModeTable { modes: [DisplayMode { w: 0, h: 0, hz: 0 }; MODE_CAP], count: 0, overflow: 0 }
    }

    /// 收录一个模式并保持降序；表满拒绝并计数。
    pub fn add(&mut self, m: DisplayMode) -> bool {
        if self.count >= MODE_CAP {
            self.overflow += 1;
            return false;
        }
        self.modes[self.count] = m;
        self.count += 1;
        self.sort_desc();
        true
    }

    /// 冒泡定序：像素数降序，同像素数按刷新率降序。
    pub fn sort_desc(&mut self) {
        for i in 0..self.count {
            for j in 0..self.count - 1 - i {
                let swap = self.modes[j].pixels() < self.modes[j + 1].pixels()
                    || (self.modes[j].pixels() == self.modes[j + 1].pixels()
                        && self.modes[j].hz < self.modes[j + 1].hz);
                if swap {
                    self.modes.swap(j, j + 1);
                }
            }
        }
    }
}

/// 全屏独占协商五态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FsnState {
    Windowed,
    Requesting,
    Exclusive,
    Lost,
    Restoring,
}

/// 协商事件。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FsnEvent {
    Request,
    Acquired,
    Failed,
    Lost,
    Restore,
    Resumed,
}

/// 全屏独占协商状态机：windowed→requesting→exclusive→lost→restoring。
/// lost 事件强制出独占并记账；非法转换拒绝并计数（不静默）。
pub struct FsnMachine {
    pub state: FsnState,
    /// lost 事件账（强制出独占的记账面）。
    pub lost_count: u32,
    /// 非法转换拒绝计数。
    pub illegal_count: u32,
}

impl FsnMachine {
    pub const fn new() -> Self {
        FsnMachine { state: FsnState::Windowed, lost_count: 0, illegal_count: 0 }
    }

    /// 推进状态机：合法迁移返回新态；非法转换 → Err 并计数。
    pub fn transition(&mut self, ev: FsnEvent) -> Result<FsnState, &'static str> {
        let next = match (self.state, ev) {
            (FsnState::Windowed, FsnEvent::Request) => FsnState::Requesting,
            (FsnState::Requesting, FsnEvent::Acquired) => FsnState::Exclusive,
            (FsnState::Requesting, FsnEvent::Failed) => FsnState::Windowed,
            // lost 强制出独占：唯一进入 Lost 态的路径（并记账）。
            (FsnState::Exclusive, FsnEvent::Lost) => {
                self.lost_count += 1;
                FsnState::Lost
            }
            (FsnState::Lost, FsnEvent::Restore) => FsnState::Restoring,
            (FsnState::Restoring, FsnEvent::Resumed) => FsnState::Windowed,
            (FsnState::Restoring, FsnEvent::Request) => FsnState::Requesting,
            _ => {
                self.illegal_count += 1;
                return Err("fsn-illegal-transition");
            }
        };
        self.state = next;
        Ok(next)
    }
}

/// 四步交接分段计时账：逐段实测入账与超段告警计数（4+8+3+15s 预算）。
pub struct HandoffTiming {
    pub measured_s: [u64; 4],
    /// 超段告警计数（不静默——超预算即告警入账）。
    pub over_budget: u32,
    pub recorded: usize,
}

impl HandoffTiming {
    pub const fn new() -> Self {
        HandoffTiming { measured_s: [0; 4], over_budget: 0, recorded: 0 }
    }

    /// 记录第 step 段实测耗时；超段告警计数。step ≥ 4 拒绝（显性化）。
    pub fn record(&mut self, step: usize, seconds: u64) -> bool {
        if step >= 4 || self.recorded >= 4 {
            return false;
        }
        self.measured_s[step] = seconds;
        if seconds > STEP_BUDGETS_S[step] {
            self.over_budget += 1;
        }
        self.recorded += 1;
        true
    }

    pub fn total_s(&self) -> u64 {
        self.measured_s.iter().sum()
    }

    /// 全程 ≤30s 判据（主册「双击游戏到 Windows 游戏画面 ≤30s」）。
    pub fn within_budget(&self) -> bool {
        self.total_s() <= HANDOFF_TOTAL_S
    }
}

/// 回滚帧率账：回程后采样 10 帧均值 vs 基线 80fps；恢复判定 ≥95% 基线。
pub struct FpsRing {
    pub samples: [u32; FPS_RING_CAP],
    pub count: usize,
    cursor: usize,
}

impl FpsRing {
    pub const fn new() -> Self {
        FpsRing { samples: [0; FPS_RING_CAP], count: 0, cursor: 0 }
    }

    /// 采样入环（满则替换最旧）。
    pub fn push(&mut self, fps: u32) {
        self.samples[self.cursor] = fps;
        self.cursor = (self.cursor + 1) % FPS_RING_CAP;
        if self.count < FPS_RING_CAP {
            self.count += 1;
        }
    }

    /// 均值（整数均值——账面口径；环满即最近 10 帧）。
    pub fn mean(&self) -> u32 {
        if self.count == 0 {
            return 0;
        }
        let sum: u64 = if self.count == FPS_RING_CAP {
            self.samples.iter().map(|&s| s as u64).sum()
        } else {
            self.samples[..self.count].iter().map(|&s| s as u64).sum()
        };
        (sum / self.count as u64) as u32
    }

    /// 恢复判定：均值 × 1000 ≥ 基线 × 950（≥95% 基线；76fps 为过线点）。
    pub fn recovered(&self) -> bool {
        self.mean() as u64 * 1000 >= ROLLBACK_BASELINE_FPS as u64 * ROLLBACK_RECOVER_PERMILLE as u64
    }
}

/// 域自检（深化批次三）。
pub fn run_f039e_checks() -> CheckSet {
    let mut cs = CheckSet::new("F039-gamefront-d3");
    // 1) 模式表降序：4K30 > 1440p144 > 1080p60（按像素数）。
    let mut mt = ModeTable::new();
    let _ = mt.add(DisplayMode { w: 1920, h: 1080, hz: 60 });
    let _ = mt.add(DisplayMode { w: 3840, h: 2160, hz: 30 });
    let _ = mt.add(DisplayMode { w: 2560, h: 1440, hz: 144 });
    cs.add(
        "mode_table_desc",
        mt.count == 3
            && mt.modes[0].pixels() == 3840u64 * 2160
            && mt.modes[1].pixels() == 2560u64 * 1440
            && mt.modes[2].pixels() == 1920u64 * 1080,
        "",
    );
    // 2) 表满 16 拒收第 17 个并计数（不静默）。
    let mut mf = ModeTable::new();
    for k in 0..17u32 {
        let _ = mf.add(DisplayMode { w: 640 + k, h: 480, hz: 60 });
    }
    cs.add("mode_table_overflow", mf.count == MODE_CAP && mf.overflow == 1, "");
    // 3) 全屏独占合法全走：windowed→requesting→exclusive→lost→restoring→windowed。
    let mut fs = FsnMachine::new();
    let walk = fs.transition(FsnEvent::Request) == Ok(FsnState::Requesting)
        && fs.transition(FsnEvent::Acquired) == Ok(FsnState::Exclusive)
        && fs.transition(FsnEvent::Lost) == Ok(FsnState::Lost)
        && fs.transition(FsnEvent::Restore) == Ok(FsnState::Restoring)
        && fs.transition(FsnEvent::Resumed) == Ok(FsnState::Windowed);
    cs.add("fsn_legal_walk", walk && fs.state == FsnState::Windowed && fs.lost_count == 1, "");
    // 4) 非法转换拒绝并计数：窗口态直接 Acquired、独占态再 Request。
    let mut fi = FsnMachine::new();
    let r1 = fi.transition(FsnEvent::Acquired);
    let _ = fi.transition(FsnEvent::Request);
    let _ = fi.transition(FsnEvent::Acquired);
    let r2 = fi.transition(FsnEvent::Request);
    let il = Err("fsn-illegal-transition");
    cs.add("fsn_illegal_rejected", r1 == il && r2 == il && fi.illegal_count == 2, "");
    // 5) lost 强制出独占并记账：独占态 Lost 后必经 Restore（再 Lost 非法）。
    let mut fl = FsnMachine::new();
    let _ = fl.transition(FsnEvent::Request);
    let _ = fl.transition(FsnEvent::Acquired);
    let _ = fl.transition(FsnEvent::Lost);
    let lost_again = fl.transition(FsnEvent::Lost);
    cs.add("fsn_lost_ledger", fl.state == FsnState::Lost && fl.lost_count == 1 && lost_again.is_err(), "");
    // 6) 四段恰按预算走：4+8+3+15 = 30s，零告警、全程达标。
    let mut ht = HandoffTiming::new();
    for (i, &b) in STEP_BUDGETS_S.iter().enumerate() {
        let _ = ht.record(i, b);
    }
    cs.add(
        "handoff_within_budget",
        ht.recorded == 4 && ht.over_budget == 0 && ht.total_s() == HANDOFF_TOTAL_S && ht.within_budget(),
        "",
    );
    // 7) 超段告警：保全 5s、冲刷 9s 各告警一次；合计 31s 超 30s 判据。
    let mut ho = HandoffTiming::new();
    let _ = ho.record(0, 5);
    let _ = ho.record(1, 9);
    let _ = ho.record(2, 3);
    let _ = ho.record(3, 14);
    cs.add("handoff_overtime_alarm", ho.over_budget == 2 && ho.total_s() == 31 && !ho.within_budget(), "");
    // 8) 采样环满后滚动：80×8 + 60×2 → 最近 10 帧均值 76（恰过恢复线）。
    let mut fr = FpsRing::new();
    for _ in 0..10 {
        fr.push(80);
    }
    fr.push(60);
    fr.push(60);
    cs.add("fps_ring_wrap_mean", fr.count == FPS_RING_CAP && fr.mean() == 76 && fr.recovered(), "");
    // 9) 恢复判定边界：均值 76 过线、75 不过线。
    let mut f76 = FpsRing::new();
    let mut f75 = FpsRing::new();
    for _ in 0..10 {
        f76.push(76);
        f75.push(75);
    }
    cs.add("fps_recover_boundary", f76.recovered() && !f75.recovered(), "");
    // 10) 未满环均值只计已采样帧（不把空槽当 0 拉低均值）。
    let mut fp = FpsRing::new();
    fp.push(90);
    fp.push(90);
    fp.push(90);
    cs.add("fps_partial_ring", fp.count == 3 && fp.mean() == 90 && fp.recovered(), "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_tie_broken_by_refresh() {
        let mut mt = ModeTable::new();
        let _ = mt.add(DisplayMode { w: 1920, h: 1080, hz: 60 });
        let _ = mt.add(DisplayMode { w: 1920, h: 1080, hz: 144 });
        assert_eq!(mt.modes[0].hz, 144, "同像素数按刷新率降序");
        assert_eq!(mt.modes[1].hz, 60);
    }

    #[test]
    fn fsn_restore_can_re_request() {
        let mut fs = FsnMachine::new();
        for ev in [FsnEvent::Request, FsnEvent::Acquired, FsnEvent::Lost, FsnEvent::Restore] {
            assert!(fs.transition(ev).is_ok());
        }
        assert!(fs.transition(FsnEvent::Request).is_ok(), "恢复态可再次请求独占");
        assert_eq!(fs.state, FsnState::Requesting);
    }

    #[test]
    fn deep3_checks_all_green() {
        let cs = run_f039e_checks();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
