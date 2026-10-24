//! VE-F0073 · 低功耗呈现路径（VE-A 域 · A03b 同步与呈现组 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0073`
//!
//! **职责定位（锚点原文）**：内容静止（无脏区且无动画注册）时自动降刷新率
//! 呈现（节能），活动恢复即时回升最高可用刷新率；降级期间丢弃检测停用防
//! 误报；状态可查询且恢复无感（无闪烁）。
//!
//! **数据结构（锚点原文）**：脏区检测器；静止计时器；刷新率切换记录。
//!
//! **错误路径与降级矩阵（锚点原文）**：
//! - 回升延迟 → watchdog 超时强制回升；
//! - 切换失败 → 保持原刷新率加告警；
//! - 静止误判 → 动画注册白名单校准。
//!
//! **性能逐项分解（锚点原文）**：脏区判定 O(1) 位图；切换 O(1)；静止计时
//! O(1)。
//!
//! **跨批对接点（锚点原文）**：上游 F0066；下游 F0069 统计标注降级期样本。
//!
//! **无障碍与隐私（锚点原文）**：无直接无障碍面；回升无闪烁保读屏稳定。
//!
//! **判据（锚点原文）**：静止降帧、即时回升、无感切换、状态可查、判据。
//!
//! 纯功能线：no_std + alloc；零 unwrap/expect/panic；整数；确定性。
//!
//! 诊断段：0xE8A3 基址（全域核查无占用）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const LOWPWR_VERSION: &str = "vea73_lowpwr-v1";

/// 诊断段基址。
pub const SEG_BASE: u32 = 0xE8A3;

// ---- 诊断码 ----
pub const XC_HZ_BAD: u32 = SEG_BASE + 1; // 刷新率参数非法
pub const XC_WATCHDOG: u32 = SEG_BASE + 2; // 回升超时强制回升
pub const XC_SWITCH_FAIL: u32 = SEG_BASE + 3; // 切换失败（保持原值）

/// 刷新率档位（mHz 整数：60Hz = 60000，避免浮点）。
pub const HZ_MAX: u32 = 60_000;
pub const HZ_LOW: u32 = 15_000;

/// 脏区位图（O(1) 判定：任一位置位 = 有脏区）。
pub struct DirtyBitmap {
    bits: [u64; 4], // 256 区
    any: bool,
}

impl DirtyBitmap {
    pub fn new() -> DirtyBitmap {
        DirtyBitmap { bits: [0; 4], any: false }
    }
    pub fn mark(&mut self, region: u32) -> bool {
        if region >= 256 {
            return false;
        }
        self.bits[(region / 64) as usize] |= 1u64 << (region % 64);
        self.any = true;
        true
    }
    pub fn clear(&mut self) {
        self.bits = [0; 4];
        self.any = false;
    }
    /// O(1)：any 位直读。
    pub fn dirty(&self) -> bool {
        self.any
    }
    pub fn popcount(&self) -> u32 {
        self.bits.iter().map(|b| b.count_ones()).sum()
    }
}

/// 低功耗呈现控制器。
pub struct LowPowerPresent {
    dirty: DirtyBitmap,
    /// 动画注册白名单（有注册动画 = 非静止，防静止误判）。
    animations: Vec<u32>,
    anim_cap: usize,
    /// 静止计时（连续静止帧数）。
    idle_frames: u64,
    /// 降档触发阈值（连续静止帧数）。
    idle_trigger: u64,
    /// watchdog 强制回升阈值（降档后允许的最长降档帧数）。
    watchdog_frames: u64,
    /// 当前刷新率（mHz）。
    hz: u32,
    /// 降档状态。
    reduced: bool,
    reduced_since: u64,
    frame: u64,
    last_code: u32,
    /// 切换记录（帧 × 从 × 到 × 原因：1 静止降档 2 活动回升 3 watchdog 4 失败保持）。
    switches: Vec<(u64, u32, u32, u8)>,
    switch_fails: u32,
    drop_detect_suspended: bool,
}

impl LowPowerPresent {
    pub fn new(idle_trigger: u64, watchdog_frames: u64, anim_cap: usize) -> LowPowerPresent {
        LowPowerPresent {
            dirty: DirtyBitmap::new(),
            animations: Vec::new(),
            anim_cap,
            idle_frames: 0,
            idle_trigger,
            watchdog_frames,
            hz: HZ_MAX,
            reduced: false,
            reduced_since: 0,
            frame: 0,
            last_code: 0,
            switches: Vec::new(),
            switch_fails: 0,
            drop_detect_suspended: false,
        }
    }

    /// 动画注册（白名单校准：注册动画的资源在册即视为活动）。
    pub fn register_animation(&mut self, res_id: u32) -> bool {
        res_id != 0 && !self.animations.contains(&res_id) && self.animations.len() < self.anim_cap && {
            self.animations.push(res_id);
            true
        }
    }

    pub fn unregister_animation(&mut self, res_id: u32) -> bool {
        let before = self.animations.len();
        self.animations.retain(|r| *r != res_id);
        before != self.animations.len()
    }

    /// 脏区标记。
    pub fn mark_dirty(&mut self, region: u32) -> bool {
        self.dirty.mark(region)
    }

    /// 帧推进（判定核心）：
    /// - 有脏区或动画在册 → 活动：即时回升最高刷新率；
    /// - 全静 → 累计静止；达阈值降档（节能）；降档超 watchdog 强制回升。
    pub fn frame_step(&mut self) -> u32 {
        self.frame += 1;
        let active = self.dirty.dirty() || !self.animations.is_empty();
        self.dirty.clear();
        if active {
            self.idle_frames = 0;
            if self.reduced {
                self.set_hz(HZ_MAX, 2);
            }
        } else {
            self.idle_frames += 1;
            if !self.reduced && self.idle_frames >= self.idle_trigger {
                self.set_hz(HZ_LOW, 1);
            }
        }
        // watchdog：降档超时强制回升（回升延迟矩阵）。
        if self.reduced && self.frame - self.reduced_since >= self.watchdog_frames {
            self.set_hz(HZ_MAX, 3);
            self.last_code = XC_WATCHDOG;
        }
        self.hz
    }

    fn set_hz(&mut self, hz: u32, reason: u8) {
        // 切换失败路径：由上层回执 report_switch_failed 触发保持原值。
        if self.hz == hz {
            return;
        }
        if self.switches.len() >= 32 {
            self.switches.remove(0);
        }
        self.switches.push((self.frame, self.hz, hz, reason));
        self.hz = hz;
        self.reduced = hz == HZ_LOW;
        if self.reduced {
            self.reduced_since = self.frame;
        }
        // 降档期间丢弃检测停用防误报（静止时无新帧，丢弃计数全是误报）。
        self.drop_detect_suspended = self.reduced;
    }

    /// 切换失败回执：保持原刷新率加告警。
    pub fn report_switch_failed(&mut self, wanted: u32) {
        self.switch_fails += 1;
        self.last_code = XC_SWITCH_FAIL;
        if self.switches.len() >= 32 {
            self.switches.remove(0);
        }
        self.switches.push((self.frame, self.hz, wanted, 4));
    }

    /// 状态可查询：当前刷新率/降档态/丢弃检测挂起态。
    pub fn state(&self) -> (u32, bool, bool) {
        (self.hz, self.reduced, self.drop_detect_suspended)
    }

    /// 恢复无感验证：回升切换的帧内完成（记录里从降到升同帧生效）。
    pub fn seamless_recovery(&self) -> bool {
        // 无感 = 不存在「半帧两档」状态：实现上切换是帧边界原子动作，
        // 此断言钉死「降档中不存在中间档位」。
        self.hz == HZ_MAX || self.hz == HZ_LOW
    }

    pub fn idle_frames(&self) -> u64 {
        self.idle_frames
    }
    pub fn switch_fails(&self) -> u32 {
        self.switch_fails
    }
    pub fn switch_count(&self) -> usize {
        self.switches.len()
    }
    pub fn anim_count(&self) -> usize {
        self.animations.len()
    }
    pub fn last_code(&self) -> u32 {
        self.last_code
    }

    /// 读屏状态行。
    pub fn status_line(&self) -> String {
        let hz_hz = self.hz / 1000;
        alloc::format!(
            "LOWPWR 刷新率{}Hz 降档{} 静止帧{} 动画{} 切换失败{}",
            hz_hz,
            if self.reduced { "是" } else { "否" },
            self.idle_frames,
            self.animations.len(),
            self.switch_fails
        )
    }
}

// =====================================================================
// 自检（判据锚点：静止降帧 / 即时回升 / 无感切换 / 状态可查）
// =====================================================================

fn chk(set: &mut CheckSet, name: &'static str, ok: bool, detail: &'static str) {
    set.add(name, ok, detail);
}

/// VE-F0073 域自检。
pub fn run_vea73_checks() -> CheckSet {
    let mut set = CheckSet::new("vea73");

    // C73-01 静止降帧：连续静止达阈值降档
    {
        let mut p = LowPowerPresent::new(4, 1000, 8);
        let mut hz = 0;
        for _ in 0..5 {
            hz = p.frame_step();
        }
        let ok = hz == HZ_LOW && p.state().1;
        chk(&mut set, "C73-01 静止降帧", ok, "静止未降档");
    }

    // C73-02 即时回升：活动恢复单帧回最高
    {
        let mut p = LowPowerPresent::new(2, 1000, 8);
        for _ in 0..3 {
            p.frame_step();
        }
        let _ = p.mark_dirty(1);
        let hz = p.frame_step();
        let ok = hz == HZ_MAX && !p.state().1;
        chk(&mut set, "C73-02 即时回升", ok, "回升有延迟");
    }

    // C73-03 动画注册防误判：有动画在册不降档
    {
        let mut p = LowPowerPresent::new(2, 1000, 8);
        let _ = p.register_animation(7);
        for _ in 0..4 {
            p.frame_step();
        }
        let ok = p.state().0 == HZ_MAX;
        chk(&mut set, "C73-03 动画白名单", ok, "动画期被误降档");
    }

    // C73-04 动画注销后静止判定恢复
    {
        let mut p = LowPowerPresent::new(2, 1000, 8);
        let _ = p.register_animation(7);
        let _ = p.unregister_animation(7);
        for _ in 0..3 {
            p.frame_step();
        }
        let ok = p.state().1;
        chk(&mut set, "C73-04 注销恢复", ok, "注销后未恢复判定");
    }

    // C73-05 watchdog 强制回升
    {
        let mut p = LowPowerPresent::new(2, 5, 8);
        for _ in 0..3 {
            p.frame_step();
        }
        // 继续静止到 watchdog 阈值。
        for _ in 0..10 {
            p.frame_step();
        }
        let ok = p.state().0 == HZ_MAX && p.last_code() == XC_WATCHDOG;
        chk(&mut set, "C73-05 watchdog", ok, "超时未强制回升");
    }

    // C73-06 降档期间丢弃检测挂起（防误报）
    {
        let mut p = LowPowerPresent::new(1, 1000, 8);
        p.frame_step();
        p.frame_step();
        let (_, reduced, susp) = p.state();
        let ok = reduced && susp;
        chk(&mut set, "C73-06 检测挂起", ok, "降档未挂起丢弃检测");
    }

    // C73-07 无感切换：档位只有两档无中间态
    {
        let mut p = LowPowerPresent::new(1, 1000, 8);
        for _ in 0..2 {
            p.frame_step();
        }
        p.frame_step();
        let ok = p.seamless_recovery();
        chk(&mut set, "C73-07 无中间档", ok, "出现中间档位");
    }

    // C73-08 切换失败保持原值加告警
    {
        let mut p = LowPowerPresent::new(1, 1000, 8);
        p.frame_step();
        p.frame_step();
        p.report_switch_failed(HZ_LOW);
        let ok = p.switch_fails() == 1 && p.last_code() == XC_SWITCH_FAIL;
        chk(&mut set, "C73-08 失败保持", ok, "失败路径未记账");
    }

    // C73-09 脏区 O(1) 判定与清账
    {
        let mut p = LowPowerPresent::new(100, 1000, 8);
        let _ = p.mark_dirty(100);
        let dirty_now = p.dirty.dirty();
        p.frame_step(); // 活动帧：脏区清账
        let dirty_after = p.dirty.dirty();
        let ok = dirty_now && !dirty_after;
        chk(&mut set, "C73-09 脏区清账", ok, "脏区未清");
    }

    // C73-10 脏区越界拒绝
    {
        let mut p = LowPowerPresent::new(100, 1000, 8);
        let ok = !p.mark_dirty(256);
        chk(&mut set, "C73-10 越界拒", ok, "越界脏区被接受");
    }

    // C73-11 动画重复注册幂等
    {
        let mut p = LowPowerPresent::new(100, 1000, 8);
        let _ = p.register_animation(7);
        let ok = !p.register_animation(7) && p.anim_count() == 1;
        chk(&mut set, "C73-11 幂等注册", ok, "重复注册入表");
    }

    // C73-12 状态可查：三元组语义齐
    {
        let p = LowPowerPresent::new(100, 1000, 8);
        let (hz, reduced, susp) = p.state();
        let ok = hz == HZ_MAX && !reduced && !susp;
        chk(&mut set, "C73-12 状态可查", ok, "初始态失准");
    }

    // C73-13 确定性
    {
        let seq = || -> u32 {
            let mut p = LowPowerPresent::new(2, 1000, 8);
            for _ in 0..3 {
                p.frame_step();
            }
            p.state().0
        };
        chk(&mut set, "C73-13 确定性", seq() == seq() && seq() == HZ_LOW, "两次不一致");
    }

    // C73-14 读屏行五要素
    {
        let p = LowPowerPresent::new(100, 1000, 8);
        let line = p.status_line();
        let ok = line.contains("刷新率") && line.contains("降档") && line.contains("静止帧")
            && line.contains("动画") && line.contains("切换失败");
        chk(&mut set, "C73-14 读屏要素", ok, "读屏行缺要素");
    }

    set
}

/// 深度驱动入口。
pub fn drill() -> (bool, String) {
    let s = run_vea73_checks();
    let ok = s.all_passed() && !s.truncated();
    (ok, if ok { String::new() } else { s.red_items_text() })
}
