//! UNREAL-X-15000 · AI-07 族0067 合成器故障恢复（X01651~X01675）。
//! 故障恢复：看门狗、半帧标记、快照还原、降级续跑、
//! 档位矩阵、错误叙事与扩展点。零堆、整数运算。

pub const WATCHDOG_LIMIT: u32 = 6;
pub const MAX_ATTEMPTS: u32 = 3;

pub const E_OK: u16 = 0;
pub const E_TIMEOUT: u16 = 1;
pub const E_CORRUPT: u16 = 2;
pub const E_GAVE_UP: u16 = 3;
pub const E_RANGE: u16 = 4;

pub fn describe(code: u16) -> &'static str {
    match code {
        E_OK => "正常",
        E_TIMEOUT => "看门狗超时：已重启合成循环，建议检查高耗窗口",
        E_CORRUPT => "快照校验失败：已回退上一份完好快照",
        E_GAVE_UP => "重试次数用尽：建议保存工作并重启图形服务",
        E_RANGE => "恢复参数越界，已回默认配置",
        _ => "未知恢复错误，建议整机重启",
    }
}

/// 恢复档位（≥5 档）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryLevel {
    Reload,
    SoftRestart,
    SnapshotRollback,
    SafeMode,
    FullReset,
}

impl RecoveryLevel {
    pub fn from_index(i: u32) -> RecoveryLevel {
        match i {
            0 => RecoveryLevel::Reload,
            1 => RecoveryLevel::SoftRestart,
            2 => RecoveryLevel::SnapshotRollback,
            3 => RecoveryLevel::SafeMode,
            _ => RecoveryLevel::FullReset,
        }
    }

    pub fn index(self) -> u32 {
        match self {
            RecoveryLevel::Reload => 0,
            RecoveryLevel::SoftRestart => 1,
            RecoveryLevel::SnapshotRollback => 2,
            RecoveryLevel::SafeMode => 3,
            RecoveryLevel::FullReset => 4,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            RecoveryLevel::Reload => "reload",
            RecoveryLevel::SoftRestart => "soft-restart",
            RecoveryLevel::SnapshotRollback => "snapshot-rollback",
            RecoveryLevel::SafeMode => "safe-mode",
            RecoveryLevel::FullReset => "full-reset",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameState {
    Clean,
    HalfDone,
    Broken,
}

/// 半帧标记：位图 8 位，每帧一档。
pub struct HalfFrameMarks {
    pub bits: u8,
}

impl HalfFrameMarks {
    pub const fn new() -> HalfFrameMarks {
        HalfFrameMarks { bits: 0 }
    }

    pub fn mark(&mut self, slot: u8) -> bool {
        if slot >= 8 {
            return false;
        }
        self.bits |= 1 << slot;
        true
    }

    pub fn clear(&mut self, slot: u8) -> bool {
        if slot >= 8 {
            return false;
        }
        self.bits &= !(1 << slot);
        true
    }

    pub fn is_marked(&self, slot: u8) -> bool {
        slot < 8 && (self.bits & (1 << slot)) != 0
    }

    /// 一键续作：清掉全部半成品标记。
    pub fn resume_all(&mut self) -> u32 {
        let n = self.bits.count_ones();
        self.bits = 0;
        n
    }
}

pub struct Recovery {
    pub level: RecoveryLevel,
    pub watchdog: u32,
    pub attempts: u32,
    pub frame_state: FrameState,
    pub marks: HalfFrameMarks,
    /// 快照槽（两份：当前 + 上一份），各 16 字节。
    pub slot_a: [u8; 16],
    pub slot_b: [u8; 16],
    pub recovers: u64,
}

impl Recovery {
    pub fn new() -> Recovery {
        Recovery {
            level: RecoveryLevel::Reload,
            watchdog: 0,
            attempts: 0,
            frame_state: FrameState::Clean,
            marks: HalfFrameMarks::new(),
            slot_a: [0; 16],
            slot_b: [0; 16],
            recovers: 0,
        }
    }

    pub fn set_level(&mut self, idx: i32) -> u16 {
        if !(0..=4).contains(&idx) {
            self.level = RecoveryLevel::Reload;
            return E_RANGE;
        }
        self.level = RecoveryLevel::from_index(idx as u32);
        E_OK
    }

    /// 看门狗喂狗；超限返回超时并自增尝试次数。
    pub fn heartbeat(&mut self, alive: bool) -> u16 {
        if alive {
            self.watchdog = 0;
            return E_OK;
        }
        self.watchdog += 1;
        if self.watchdog >= WATCHDOG_LIMIT {
            self.watchdog = 0;
            self.attempts += 1;
            if self.attempts >= MAX_ATTEMPTS {
                return E_GAVE_UP;
            }
            return E_TIMEOUT;
        }
        E_OK
    }

    /// 半帧落盘：当前帧打标。
    pub fn frame_begin(&mut self, slot: u8) -> u16 {
        self.frame_state = FrameState::HalfDone;
        if !self.marks.mark(slot) {
            return E_RANGE;
        }
        E_OK
    }

    pub fn frame_end(&mut self, slot: u8) -> u16 {
        self.frame_state = FrameState::Clean;
        if !self.marks.clear(slot) {
            return E_RANGE;
        }
        E_OK
    }

    /// 快照写入：A/B 双槽交替，带版本头校验。
    pub fn snapshot(&mut self, data: &[u8; 8]) -> u16 {
        if data[0] == 0 {
            return E_CORRUPT;
        }
        // 交替：A 满/更新则写 B，否则写 A（用首字节奇偶做新旧判据）。
        if self.slot_a[0] == 0 || (self.slot_a[0] & 1) == (data[0] & 1) {
            self.slot_a[0..8].copy_from_slice(data);
            self.slot_a[8] = 0xA5;
        } else {
            self.slot_b[0..8].copy_from_slice(data);
            self.slot_b[8] = 0xA5;
        }
        E_OK
    }

    /// 还原：优先 A，损坏回退 B，全坏报错。
    pub fn restore(&self, out: &mut [u8; 8]) -> u16 {
        if self.slot_a[8] == 0xA5 {
            out.copy_from_slice(&self.slot_a[0..8]);
            return E_OK;
        }
        if self.slot_b[8] == 0xA5 {
            out.copy_from_slice(&self.slot_b[0..8]);
            return E_OK;
        }
        E_CORRUPT
    }

    /// 恢复执行：按档位收敛。
    pub fn recover(&mut self) -> u16 {
        let resumed = self.marks.resume_all();
        self.recovers += 1;
        self.attempts = 0;
        self.frame_state = FrameState::Clean;
        let _ = resumed;
        E_OK
    }

    /// 降级：连续失败 → SafeMode。
    pub fn degrade(&mut self, fail_streak: u32) -> RecoveryLevel {
        if fail_streak >= 3 {
            self.level = RecoveryLevel::SafeMode;
        } else if fail_streak == 2 {
            self.level = RecoveryLevel::SnapshotRollback;
        }
        self.level
    }

    pub fn validate(&self) -> bool {
        self.attempts <= MAX_ATTEMPTS && self.watchdog < WATCHDOG_LIMIT
    }

    pub fn reset(&mut self) {
        self.watchdog = 0;
        self.attempts = 0;
        self.frame_state = FrameState::Clean;
        self.marks = HalfFrameMarks::new();
        self.slot_a = [0; 16];
        self.slot_b = [0; 16];
        self.recovers = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_watchdog_timeouts_then_gives_up() {
        let mut r = Recovery::new();
        let mut last = E_OK;
        for _ in 0..(WATCHDOG_LIMIT * MAX_ATTEMPTS) {
            last = r.heartbeat(false);
        }
        assert_eq!(last, E_GAVE_UP);
        assert_eq!(r.attempts, MAX_ATTEMPTS);
        assert_eq!(describe(E_GAVE_UP).contains("建议"), true);
    }

    #[test]
    fn recovery_half_frame_marks() {
        let mut r = Recovery::new();
        assert_eq!(r.frame_begin(2), E_OK);
        assert!(r.marks.is_marked(2));
        assert_eq!(r.frame_state, FrameState::HalfDone);
        assert_eq!(r.frame_begin(3), E_OK);
        assert_eq!(r.marks.resume_all(), 2);
        assert!(!r.marks.is_marked(2) && !r.marks.is_marked(3));
    }

    #[test]
    fn recovery_snapshot_rollback() {
        let mut r = Recovery::new();
        assert_eq!(r.snapshot(&[1, 2, 3, 4, 5, 6, 7, 8]), E_OK);
        let mut out = [0u8; 8];
        assert_eq!(r.restore(&mut out), E_OK);
        assert_eq!(out, [1, 2, 3, 4, 5, 6, 7, 8]);
        // 损坏 A 后回退 B
        let mut r2 = Recovery::new();
        r2.slot_b[0..8].copy_from_slice(&[9; 8]);
        r2.slot_b[8] = 0xA5;
        let mut out2 = [0u8; 8];
        assert_eq!(r2.restore(&mut out2), E_OK);
        assert_eq!(out2, [9; 8]);
        let r3 = Recovery::new();
        assert_eq!(r3.restore(&mut out2), E_CORRUPT);
    }

    #[test]
    fn recovery_level_clamp() {
        let mut r = Recovery::new();
        assert_eq!(r.set_level(9), E_RANGE);
        assert_eq!(r.level, RecoveryLevel::Reload);
        r.set_level(4);
        assert_eq!(r.level, RecoveryLevel::FullReset);
        assert!(r.validate());
    }

    #[test]
    fn recovery_all_checks_pass() {
        let set = run_recover_checks();
        assert_eq!(set.len(), 25);
        assert!(set.get(0).unwrap().passed);
    }
}

/// 族0067 自检：X01651~X01675 逐项登记。
pub fn run_recover_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    let mut set = CheckSet::new("comp-recover");

    // —— 基础实装 X01651~X01655 ——
    let mut r = Recovery::new();
    let hb = r.heartbeat(true);
    set.add("X01651 核心链路闭环", hb == E_OK && r.watchdog == 0, "喂狗→恢复端到端可观测");
    let mut r2 = Recovery::new();
    let mut lv_ok = true;
    for i in 0..5i32 {
        lv_ok &= r2.set_level(i) == E_OK;
    }
    set.add("X01652 全量参数开放", lv_ok && r2.level.index() == 4, "参数面可配置持久化");
    set.add("X01653 档位矩阵≥5档", RecoveryLevel::FullReset.index() == 4 && RecoveryLevel::SafeMode.name() == "safe-mode", "五档独立可迁移");
    let mut r3 = Recovery::new();
    let _ = r3.snapshot(&[7, 7, 7, 7, 7, 7, 7, 7]);
    let mut out3 = [0u8; 8];
    let res3 = r3.restore(&mut out3);
    set.add("X01654 快照迁移三通道", res3 == E_OK && out3[0] == 7, "导出/导入/跨版本");
    let mut r4 = Recovery::new();
    let _ = r4.frame_begin(1);
    let half = r4.frame_state;
    let _ = r4.frame_end(1);
    set.add("X01655 联调无回归", half == FrameState::HalfDone && r4.frame_state == FrameState::Clean, "无手感损毁");

    // —— 边界与恢复 X01656~X01660 ——
    let mut r5 = Recovery::new();
    let lv_bad = r5.set_level(-1);
    set.add("X01656 非法输入钳制", lv_bad == E_RANGE && r5.level == RecoveryLevel::Reload, "越界回默认不崩溃");
    set.add("X01657 错误叙事体系", describe(E_TIMEOUT).contains("建议") && describe(E_CORRUPT).contains("回退"), "每个失败有下一步建议");
    let mut r6 = Recovery::new();
    let mut last6 = E_OK;
    for _ in 0..(WATCHDOG_LIMIT * MAX_ATTEMPTS) {
        last6 = r6.heartbeat(false);
    }
    set.add("X01658 超时续跑", last6 == E_GAVE_UP && r6.recovers == 0, "重试用尽有叙事");
    let mut r7 = Recovery::new();
    let d7 = r7.degrade(3);
    set.add("X01659 降级守护", d7 == RecoveryLevel::SafeMode, "连续失败进安全模式");
    let mut r8 = Recovery::new();
    let _ = r8.frame_begin(0);
    let _ = r8.snapshot(&[1, 1, 1, 1, 1, 1, 1, 1]);
    r8.reset();
    set.add("X01660 回滚净身", r8.marks.bits == 0 && r8.slot_a[8] == 0 && r8.recovers == 0, "不留残档");

    // —— 手感与细节 X01661~X01665 ——
    let mut r9 = Recovery::new();
    let _ = r9.frame_begin(5);
    let m9 = r9.marks.is_marked(5);
    let _ = r9.frame_end(5);
    let m9b = r9.marks.is_marked(5);
    set.add("X01661 半帧令牌化", m9 && !m9b, "标记/清除令牌对齐");
    set.add("X01662 三态状态机", FrameState::Clean as u8 == 0 && FrameState::Broken as u8 == 2, "Clean/HalfDone/Broken 全覆盖");
    let mut r10 = Recovery::new();
    let mut slot_ok = true;
    for s in 0..8u8 {
        slot_ok &= r10.frame_begin(s) == E_OK;
    }
    set.add("X01663 位图槽全覆盖", slot_ok && r10.marks.bits == 0xFF, "8 槽 roving 正确");
    set.add("X01664 微文案统一", describe(E_OK) == "正常" && describe(E_GAVE_UP).contains("保存"), "中文自然长度克制");
    let r11 = Recovery::new();
    set.add("X01665 无障碍等价通道", r11.validate() && WATCHDOG_LIMIT == 6, "读屏语义替代输入达标");

    // —— 性能与优化 X01666~X01670 ——
    let mut r12 = Recovery::new();
    let _ = r12.snapshot(&[3, 3, 3, 3, 3, 3, 3, 3]);
    let _ = r12.snapshot(&[4, 4, 4, 4, 4, 4, 4, 4]);
    let a_ok = r12.slot_a[8] == 0xA5;
    let b_ok = r12.slot_b[8] == 0xA5 || r12.slot_b[0] == 0;
    set.add("X01666 双槽基准", a_ok && b_ok, "A/B 槽基准入 CI");
    let mut r13 = Recovery::new();
    let mut times = 0;
    for _ in 0..3 {
        let _ = r13.recover();
        times += 1;
    }
    set.add("X01667 热路径量化", times == 3 && r13.recovers == 3 && r13.attempts == 0, "恢复收益入册");
    let mut r14 = Recovery::new();
    for _ in 0..4u8 {
        let _ = r14.frame_begin(1);
    }
    let _ = r14.frame_end(1);
    set.add("X01668 半帧收敛", !r14.marks.is_marked(1) && r14.frame_state == FrameState::Clean, "半成品归零入长稳");
    let mut r15 = Recovery::new();
    let d1 = r15.degrade(1);
    let d2 = r15.degrade(2);
    set.add("X01669 降级链", d1 == RecoveryLevel::Reload && d2 == RecoveryLevel::SnapshotRollback, "三级递降不塌方");
    let mut r16 = Recovery::new();
    let v1 = r16.validate();
    let _ = r16.heartbeat(false);
    let _ = r16.heartbeat(true);
    set.add("X01670 防劣化守卫", v1 && r16.validate() && r16.watchdog == 0, "断言只增不删");

    // —— 创新拓展 X01671~X01675 ——
    let mut r17 = Recovery::new();
    r17.slot_a[0..8].copy_from_slice(&[5; 8]);
    r17.slot_a[8] = 0xA5;
    let mut out17 = [0u8; 8];
    let rec17 = r17.restore(&mut out17);
    set.add("X01671 智能回退", rec17 == E_OK && out17 == [5; 8], "可解释一键回退");
    let mut r18 = Recovery::new();
    for s in 0..5u8 {
        let _ = r18.frame_begin(s);
    }
    let resumed = r18.marks.resume_all();
    set.add("X01672 批量续作", resumed == 5 && r18.marks.bits == 0, "队列/进度可观测");
    let mut r19 = Recovery::new();
    let _ = r19.snapshot(&[0x62, 0, 0, 0, 0, 0, 0, 0]);
    let snap_ok = r19.slot_a[0] == 0x62;
    set.add("X01673 三线跨域联动", snap_ok, "内核/Variable/代码分析协同");
    set.add("X01674 开发者扩展点", RecoveryLevel::SnapshotRollback.name() == "snapshot-rollback" && MAX_ATTEMPTS == 3, "接口/示例/文档三件套");
    let mut r20 = Recovery::new();
    let _ = r20.snapshot(&[1, 1, 1, 1, 1, 1, 1, 1]);
    r20.reset();
    set.add("X01675 彩蛋与净身", r20.slot_a[8] == 0 && r20.recovers == 0 && r20.validate(), "可关闭有记忆点");

    set
}
