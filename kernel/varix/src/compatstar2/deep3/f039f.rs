//! F039 深化批次四 · 帧账与掉帧归因面（compatstar2/deep3 · G-A-39）。
//!
//! 批次一深化覆盖 D3D 库词表/显存预估/独占全屏闸/交接参数打包，批次二
//! 覆盖 exe 识别/全屏降级链/回程四步账/分辨率安全交集，批次三覆盖
//! DXGI 模式枚举/全屏协商状态机/交接分段计时/回滚帧率账；本批补齐
//! 【功能定义】「分流本身丝滑」全语义对齐的帧级量化面：帧耗时环形账
//! （定长 64，微秒——性能面板滚动读数的数据源）、抖动分级（hitch
//! <33ms / stutter 33-100ms / freeze >100ms 三级阈值分类计数——主册
//! 【设计细节】性能面板分级口径）、归因桶（gpu/cpu/io/input 四桶，每
//! 掉帧归一桶，占比 permille——掉帧根因可解释的账面底座）、恢复时间
//! 账（掉帧后回到 16.6ms 内的帧数计数，均值估算——「回来路径对称」
//! 的帧级对拍面）。
//!
//! 判据对账：主册 G-A-39【功能定义】帧率/丝滑量化 +【设计细节】交接
//! 预算分段对账的帧级延伸；三级阈值与四桶归因为域内量化口径（无 MS
//! 面——如实注明）。
//!
//! 零堆纪律：定长 64 环 + 定长 4 桶，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）

/// 帧耗时环形账容量 64（性能面板滚动读数窗口）。
pub const RING_CAP: usize = 64;
/// 目标帧间隔 16.6ms（60fps——主册【设计细节】丝滑基线）。
pub const TARGET_US: u32 = 16_667;
/// hitch 上界 33ms（轻微抖动——三级分类第一档）。
pub const HITCH_US: u32 = 33_333;
/// stutter 上界 100ms（重度卡顿——超过即 freeze）。
pub const STUTTER_US: u32 = 100_000;
/// 分级：达标（t ≤ 16.6ms）。
pub const GRADE_ON_TARGET: u8 = 0;
/// 分级：hitch（16.6ms < t < 33ms）。
pub const GRADE_HITCH: u8 = 1;
/// 分级：stutter（33ms ≤ t ≤ 100ms）。
pub const GRADE_STUTTER: u8 = 2;
/// 分级：freeze（t > 100ms）。
pub const GRADE_FREEZE: u8 = 3;
/// 归因桶：GPU。
pub const BUCKET_GPU: usize = 0;
/// 归因桶：CPU。
pub const BUCKET_CPU: usize = 1;
/// 归因桶：IO。
pub const BUCKET_IO: usize = 2;
/// 归因桶：Input。
pub const BUCKET_INPUT: usize = 3;
/// 归因桶数（四桶定长）。
pub const BUCKETS: usize = 4;

/// 三级阈值分类（hitch/stutter/freeze——边界值归属如实对拍主册口径：
/// 33ms 与 100ms 归 stutter，>100ms 归 freeze）。
pub fn classify(us: u32) -> u8 {
    if us > STUTTER_US {
        GRADE_FREEZE
    } else if us >= HITCH_US {
        GRADE_STUTTER
    } else if us > TARGET_US {
        GRADE_HITCH
    } else {
        GRADE_ON_TARGET
    }
}

// ---------------------------------------------------------------------------
// 帧账模型

/// 帧账模型：环形耗时账 + 三级计数 + 四桶归因 + 恢复时间账。
pub struct FrameAccount {
    ring: [u32; RING_CAP],
    head: usize,
    /// 已填充槽数（封顶 64——环形滚动不回退）。
    pub filled: usize,
    /// hitch 级掉帧计数。
    pub hitch_n: u32,
    /// stutter 级掉帧计数。
    pub stutter_n: u32,
    /// freeze 级掉帧计数。
    pub freeze_n: u32,
    /// 掉帧总数（三档合计——归因占比的分母）。
    pub drops_total: u32,
    buckets: [u32; BUCKETS],
    /// 恢复中标记（掉帧串收口用）。
    recovering: bool,
    /// 当前掉帧串长度（回到 16.6ms 内时收口入账）。
    run_len: u32,
    /// 恢复帧数累计（各掉帧串回 16.6ms 内所历帧数之和）。
    pub recovery_frames: u32,
    /// 恢复事件数（掉帧串数——均值估算法的分母）。
    pub recovery_events: u32,
}

impl FrameAccount {
    pub const fn new() -> Self {
        FrameAccount {
            ring: [0; RING_CAP],
            head: 0,
            filled: 0,
            hitch_n: 0,
            stutter_n: 0,
            freeze_n: 0,
            drops_total: 0,
            buckets: [0; BUCKETS],
            recovering: false,
            run_len: 0,
            recovery_frames: 0,
            recovery_events: 0,
        }
    }

    fn open_or_extend(&mut self) {
        if self.recovering {
            self.run_len += 1;
        } else {
            self.recovering = true;
            self.run_len = 1;
        }
    }

    fn close_run(&mut self) {
        if self.recovering {
            self.recovery_frames += self.run_len;
            self.recovery_events += 1;
            self.recovering = false;
            self.run_len = 0;
        }
    }

    /// 入帧：环形账滚动 + 分级计数 + 恢复事件收口
    /// （回到 16.6ms 内 → 事件 +1，该串掉帧数入恢复账）。
    pub fn push(&mut self, us: u32) {
        self.ring[self.head] = us;
        self.head = (self.head + 1) % RING_CAP;
        if self.filled < RING_CAP {
            self.filled += 1;
        }
        match classify(us) {
            GRADE_HITCH => {
                self.hitch_n += 1;
                self.drops_total += 1;
                self.open_or_extend();
            }
            GRADE_STUTTER => {
                self.stutter_n += 1;
                self.drops_total += 1;
                self.open_or_extend();
            }
            GRADE_FREEZE => {
                self.freeze_n += 1;
                self.drops_total += 1;
                self.open_or_extend();
            }
            _ => self.close_run(),
        }
    }

    /// 掉帧归因入桶（每掉帧必归一桶——零静默；非法桶显性 Err）。
    pub fn attribute(&mut self, bucket: usize) -> Result<(), &'static str> {
        if bucket >= BUCKETS {
            return Err("bad-bucket");
        }
        self.buckets[bucket] += 1;
        Ok(())
    }

    /// 桶占比 permille（总掉帧为分母；无掉帧 → 0——不虚报）。
    pub fn permille(&self, bucket: usize) -> u32 {
        if self.drops_total == 0 {
            return 0;
        }
        self.buckets[bucket] * 1000 / self.drops_total
    }

    /// 归因审计：未落桶掉帧数（> 0 即有掉帧未归因——显性化暴露）。
    pub fn unattributed(&self) -> u32 {
        let sum: u32 = self.buckets.iter().sum();
        self.drops_total - sum.min(self.drops_total)
    }

    /// 最近一帧耗时读数（环形账）。
    pub fn latest(&self) -> u32 {
        if self.filled == 0 {
            return 0;
        }
        self.ring[(self.head + RING_CAP - 1) % RING_CAP]
    }

    /// 恢复均值 ×10 估算（整数域——恢复帧数×10 / 事件数）。
    pub fn recovery_mean_x10(&self) -> u32 {
        if self.recovery_events == 0 {
            return 0;
        }
        self.recovery_frames * 10 / self.recovery_events
    }
}

// ---------------------------------------------------------------------------
// 域自检（深化批次四）

/// 域自检（深化批次四）。
pub fn run_f039f_checks() -> CheckSet {
    let mut cs = CheckSet::new("F039-frameacct-d4");
    // 1) 环形账容量封顶 64：入 70 帧 filled == 64，最新读数 = 第 70 帧。
    let mut f = FrameAccount::new();
    for i in 1..=70u32 {
        f.push(i * 100);
    }
    cs.add("ring_cap_64", f.filled == RING_CAP && f.latest() == 7000, "");
    // 2) 三级阈值边界：16.6ms 达标 / 33ms、100ms 归 stutter / >100ms freeze。
    cs.add(
        "classify_boundaries",
        classify(16_667) == GRADE_ON_TARGET
            && classify(16_668) == GRADE_HITCH
            && classify(33_332) == GRADE_HITCH
            && classify(33_333) == GRADE_STUTTER
            && classify(100_000) == GRADE_STUTTER
            && classify(100_001) == GRADE_FREEZE,
        "",
    );
    // 3) 三档计数：hitch/stutter/freeze 各一 → 计数 1/1/1，掉帧总数 3。
    let mut f2 = FrameAccount::new();
    f2.push(20_000);
    f2.push(50_000);
    f2.push(150_000);
    cs.add(
        "grade_counters",
        f2.hitch_n == 1 && f2.stutter_n == 1 && f2.freeze_n == 1 && f2.drops_total == 3,
        "",
    );
    // 4) 归因占比 permille：补第 4 桶掉帧后 gpu 2/4 = 500‰、cpu/io 各 250‰。
    f2.push(40_000);
    let _ = f2.attribute(BUCKET_GPU);
    let _ = f2.attribute(BUCKET_GPU);
    let _ = f2.attribute(BUCKET_CPU);
    let _ = f2.attribute(BUCKET_IO);
    cs.add(
        "attribution_permille",
        f2.drops_total == 4
            && f2.permille(BUCKET_GPU) == 500 && f2.permille(BUCKET_CPU) == 250
            && f2.permille(BUCKET_IO) == 250 && f2.permille(BUCKET_INPUT) == 0,
        "",
    );
    // 5) 归因审计：全落桶 → unattributed == 0（零静默闭环）。
    cs.add("attribution_closed", f2.unattributed() == 0, "");
    // 6) 非法桶显性报错（不静默吞归因）。
    cs.add(
        "bad_bucket_explicit",
        matches!(f2.attribute(4), Err("bad-bucket")) && f2.unattributed() == 0,
        "",
    );
    // 7) 恢复时间账：掉帧串 [20000,25000] 后回达标 → 事件 1、恢复帧数 2。
    let mut f3 = FrameAccount::new();
    f3.push(20_000);
    f3.push(25_000);
    f3.push(16_667);
    cs.add("recovery_single_run", f3.recovery_events == 1 && f3.recovery_frames == 2, "");
    // 8) 两个独立掉帧串（2 帧 + 1 帧）→ 事件 2、恢复帧数累计 3、均值 ×10 = 15。
    f3.push(30_000);
    f3.push(16_667);
    cs.add(
        "recovery_two_runs_mean",
        f3.recovery_events == 2 && f3.recovery_frames == 3 && f3.recovery_mean_x10() == 15,
        "",
    );
    // 9) 达标帧序列不产生恢复事件（无掉帧无账——不虚报）。
    let mut f4 = FrameAccount::new();
    f4.push(16_667);
    f4.push(16_666);
    cs.add(
        "no_drop_no_event",
        f4.recovery_events == 0 && f4.recovery_frames == 0 && f4.drops_total == 0,
        "",
    );
    // 10) 无掉帧时占比读数恒 0（分母零保护）。
    cs.add("permille_zero_guard", f4.permille(BUCKET_GPU) == 0 && f4.latest() == 16_666, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_mean_real_values() {
        // 串1：2 帧恢复；串2：1 帧恢复 → 均值 1.5 帧（×10 = 15）。
        let mut f = FrameAccount::new();
        f.push(20_000);
        f.push(25_000);
        f.push(16_667);
        f.push(40_000);
        f.push(16_667);
        assert_eq!(f.recovery_events, 2);
        assert_eq!(f.recovery_frames, 3);
        assert_eq!(f.recovery_mean_x10(), 15, "3×10/2 = 15（1.5 帧/串）");
    }

    #[test]
    fn unattributed_exposed() {
        // 掉帧 3、归因 2 → 1 帧未归因必须显性可见。
        let mut f = FrameAccount::new();
        f.push(20_000);
        f.push(50_000);
        f.push(150_000);
        let _ = f.attribute(BUCKET_GPU);
        let _ = f.attribute(BUCKET_CPU);
        assert_eq!(f.unattributed(), 1, "未归因掉帧不允许静默");
        let _ = f.attribute(BUCKET_IO);
        assert_eq!(f.unattributed(), 0);
    }

    #[test]
    fn deep4_checks_all_green() {
        let cs = run_f039f_checks();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
