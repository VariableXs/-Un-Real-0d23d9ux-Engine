//! UNREAL-X AI-01 · 族0004 恢复环境重生（X00076~X00100，内核侧落点）。
//!
//! 恢复环境快照/续作：固定槽位的启动快照（阶段 + 指纹），中断后可标记
//! 半成品并一键续作；恢复环境工具清单与降级档位。纯逻辑 + 固定数组，
//! no_std 兼容；非法输入一律钳制并给出可读原因计数。

use crate::bootchain::hash_bytes;
use crate::checks::CheckSet;

/// 快照槽位数。
pub const SNAPSHOT_SLOTS: usize = 4;
/// 恢复工具清单上限。
pub const TOOL_MAX: usize = 12;

/// 可恢复的启动阶段（与体检链路段对齐）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BootStage {
    Firmware = 0,
    Loader = 1,
    Kernel = 2,
    Init = 3,
    Session = 4,
}

impl BootStage {
    pub fn from_index(i: u32) -> BootStage {
        match i {
            0 => BootStage::Firmware,
            1 => BootStage::Loader,
            2 => BootStage::Kernel,
            3 => BootStage::Init,
            _ => BootStage::Session,
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }
}

/// 快照状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SnapState {
    Empty,
    Complete,
    /// 半成品：中断留下的续作点。
    HalfDone,
}

/// 一个启动快照槽。
#[derive(Clone, Copy, Debug)]
pub struct BootSnapshot {
    pub state: SnapState,
    pub stage: BootStage,
    /// 阶段内字节偏移（续作从这里接着走）。
    pub offset: u32,
    pub fingerprint: u32,
    pub generation: u32,
}

impl BootSnapshot {
    const fn empty() -> BootSnapshot {
        BootSnapshot {
            state: SnapState::Empty,
            stage: BootStage::Firmware,
            offset: 0,
            fingerprint: 0,
            generation: 0,
        }
    }
}

/// 恢复环境管理器。
#[derive(Clone, Copy, Debug)]
pub struct RecoveryEnv {
    pub slots: [BootSnapshot; SNAPSHOT_SLOTS],
    /// 恢复工具登记数。
    pub tools: u32,
    /// 降级档位 0=全量 1=精简 2=最小（资源紧张守护）。
    pub degrade: u8,
    pub clamped: u32,
    pub generation: u32,
}

impl RecoveryEnv {
    pub const fn new() -> RecoveryEnv {
        RecoveryEnv {
            slots: [BootSnapshot::empty(); SNAPSHOT_SLOTS],
            tools: 0,
            degrade: 0,
            clamped: 0,
            generation: 0,
        }
    }

    /// 打快照：写入最老槽（环形覆盖），generation 递增。
    pub fn snapshot(&mut self, stage: BootStage, offset: u32, complete: bool) -> usize {
        self.generation = self.generation.wrapping_add(1);
        let mut victim = 0usize;
        let mut oldest = u32::MAX;
        for (i, s) in self.slots.iter().enumerate() {
            if s.state == SnapState::Empty {
                victim = i;
                break;
            }
            if s.generation <= oldest {
                oldest = s.generation;
                victim = i;
            }
        }
        self.slots[victim] = BootSnapshot {
            state: if complete { SnapState::Complete } else { SnapState::HalfDone },
            stage,
            offset,
            fingerprint: hash_bytes(&[(stage as u8), (offset & 0xff) as u8, (offset >> 8) as u8])
                ^ self.generation,
            generation: self.generation,
        };
        victim
    }

    /// 一键续作：找最新 HalfDone 槽；无则 None。
    pub fn resume_point(&self) -> Option<(BootStage, u32)> {
        let mut best: Option<&BootSnapshot> = None;
        for s in &self.slots {
            if s.state == SnapState::HalfDone {
                match best {
                    Some(b) if b.generation >= s.generation => {}
                    _ => best = Some(s),
                }
            }
        }
        best.map(|s| (s.stage, s.offset))
    }

    /// 续作完成：把 HalfDone 升级为 Complete。
    pub fn finish_resume(&mut self) -> bool {
        let gen = self
            .slots
            .iter()
            .filter(|s| s.state == SnapState::HalfDone)
            .map(|s| s.generation)
            .max();
        match gen {
            Some(g) => {
                for s in self.slots.iter_mut() {
                    if s.state == SnapState::HalfDone && s.generation == g {
                        s.state = SnapState::Complete;
                    }
                }
                true
            }
            None => {
                self.clamped += 1;
                false
            }
        }
    }

    /// 回滚净身：清空全部槽位与续作点。
    pub fn wipe(&mut self) {
        self.slots = [BootSnapshot::empty(); SNAPSHOT_SLOTS];
    }

    /// 登记恢复工具（钳制上限）。
    pub fn add_tool(&mut self) -> bool {
        if self.tools >= TOOL_MAX as u32 {
            self.clamped += 1;
            return false;
        }
        self.tools += 1;
        true
    }

    /// 资源降级：钳制到 0~2。
    pub fn set_degrade(&mut self, level: u8) {
        self.degrade = if level <= 2 {
            level
        } else {
            self.clamped += 1;
            2
        };
    }

    /// 降级后工具预算：全量=12 精简=6 最小=3。
    pub fn tool_budget(&self) -> u32 {
        match self.degrade {
            0 => TOOL_MAX as u32,
            1 => 6,
            _ => 3,
        }
    }
}

/// 族0004 内核侧域自检。
pub fn run_recovery_checks() -> CheckSet {
    let mut set = CheckSet::new("bootchain.recovery");
    let mut r = RecoveryEnv::new();
    set.add("snapshot fills slot 0", r.snapshot(BootStage::Kernel, 128, true) == 0 && r.slots[0].state == SnapState::Complete, "");
    set.add("halfdone snapshot resumable", {
        let mut q = RecoveryEnv::new();
        q.snapshot(BootStage::Init, 64, false);
        q.resume_point() == Some((BootStage::Init, 64))
    }, "");
    set.add("finish resume promotes", {
        let mut q = RecoveryEnv::new();
        q.snapshot(BootStage::Init, 64, false);
        q.finish_resume() && q.slots.iter().all(|s| s.state != SnapState::HalfDone)
    }, "");
    set.add("no resume point clamped", {
        let mut q = RecoveryEnv::new();
        !q.finish_resume() && q.clamped == 1
    }, "");
    set.add("ring overwrite oldest", {
        let mut q = RecoveryEnv::new();
        for i in 0..SNAPSHOT_SLOTS + 1 {
            q.snapshot(BootStage::Loader, i as u32, true);
        }
        q.slots.iter().all(|s| s.state == SnapState::Complete)
    }, "");
    set.add("tool cap clamped", {
        let mut q = RecoveryEnv::new();
        for _ in 0..TOOL_MAX + 2 {
            q.add_tool();
        }
        q.tools == TOOL_MAX as u32 && q.clamped >= 1
    }, "");
    set.add("degrade clamp", {
        let mut q = RecoveryEnv::new();
        q.set_degrade(9);
        q.degrade == 2 && q.tool_budget() == 3
    }, "");
    set.add("wipe clean", {
        let mut q = RecoveryEnv::new();
        q.snapshot(BootStage::Kernel, 1, true);
        q.wipe();
        q.slots.iter().all(|s| s.state == SnapState::Empty) && q.resume_point().is_none()
    }, "");
    set.add("stage index roundtrip", BootStage::from_index(3) == BootStage::Init && BootStage::from_index(99) == BootStage::Session, "");
    set.add("fingerprint differs by offset", {
        let mut q = RecoveryEnv::new();
        let a = q.snapshot(BootStage::Kernel, 1, true);
        let f1 = q.slots[a].fingerprint;
        q.wipe();
        let b = q.snapshot(BootStage::Kernel, 2, true);
        f1 != q.slots[b].fingerprint
    }, "");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn x00076_snapshot_resume_min_loop() {
        let mut r = RecoveryEnv::new();
        r.snapshot(BootStage::Kernel, 512, false);
        assert_eq!(r.resume_point(), Some((BootStage::Kernel, 512)));
        assert!(r.finish_resume());
        assert!(r.resume_point().is_none());
    }

    #[test]
    fn x00081_invalid_clamped_not_crash() {
        let mut r = RecoveryEnv::new();
        assert!(!r.finish_resume());
        r.set_degrade(200);
        assert_eq!(r.degrade, 2);
        assert!(r.clamped >= 2);
    }

    #[test]
    fn x00084_degrade_guard_budget() {
        let mut r = RecoveryEnv::new();
        assert_eq!(r.tool_budget(), 12);
        r.set_degrade(1);
        assert_eq!(r.tool_budget(), 6);
        r.set_degrade(2);
        assert_eq!(r.tool_budget(), 3);
    }

    #[test]
    fn x00090_wipe_leaves_no_residue() {
        let mut r = RecoveryEnv::new();
        r.snapshot(BootStage::Session, 9, false);
        r.wipe();
        assert!(r.slots.iter().all(|s| s.state == SnapState::Empty));
    }

    #[test]
    fn x00076_run_checks_pass() {
        assert!(run_recovery_checks().all_passed());
    }
}
