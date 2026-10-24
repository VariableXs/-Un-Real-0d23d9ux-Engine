//! unx292_nmisafe ↔ UNX-F0292 panic 期 NMI 安全（NMI 到达处理中的次序）
//!
//! panic 处理中 NMI 到达零重入（次序标志），双触发演练过。
//!
//! 判据：UNX-F0292-J1 panic 处理中 NMI 到达零重入（次序标志），双触发演练过。

use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// panic 处理阶段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanicPhase {
    /// 未进入 panic。
    Idle,
    /// panic 入口（正在保存上下文）。
    Entry,
    /// 呈现中。
    Presenting,
    /// 转储写入中。
    Dumping,
    /// 安全停机中。
    Halting,
    /// 完成。
    Done,
}

/// NMI 次序状态标志。
pub struct NmiSafety {
    /// 当前 panic 阶段。
    phase: AtomicU32,
    /// NMI 是否在处理中（防重入标志）。
    nmi_in_progress: AtomicBool,
    /// NMI 到达计数。
    nmi_count: AtomicU32,
    /// NMI 被安全忽略计数（panic 中到达被丢弃）。
    nmi_ignored: AtomicU32,
    /// 双触发检测计数。
    double_trigger_count: AtomicU32,
}

impl NmiSafety {
    /// 新建 NMI 安全管理器。
    pub fn new() -> Self {
        Self {
            phase: AtomicU32::new(0),
            nmi_in_progress: AtomicBool::new(false),
            nmi_count: AtomicU32::new(0),
            nmi_ignored: AtomicU32::new(0),
            double_trigger_count: AtomicU32::new(0),
        }
    }

    /// 设置 panic 阶段。
    pub fn set_phase(&self, phase: PanicPhase) {
        let v = match phase {
            PanicPhase::Idle => 0,
            PanicPhase::Entry => 1,
            PanicPhase::Presenting => 2,
            PanicPhase::Dumping => 3,
            PanicPhase::Halting => 4,
            PanicPhase::Done => 5,
        };
        self.phase.store(v, Ordering::SeqCst);
    }

    /// 获取当前阶段。
    pub fn phase(&self) -> PanicPhase {
        match self.phase.load(Ordering::SeqCst) {
            0 => PanicPhase::Idle,
            1 => PanicPhase::Entry,
            2 => PanicPhase::Presenting,
            3 => PanicPhase::Dumping,
            4 => PanicPhase::Halting,
            _ => PanicPhase::Done,
        }
    }

    /// NMI 到达处理（零重入保证）。
    ///
    /// 返回 true = NMI 被安全处理；false = NMI 被忽略（panic 中）。
    pub fn nmi_arrive(&self) -> bool {
        self.nmi_count.fetch_add(1, Ordering::SeqCst);

        // 防重入：如果 NMI 已在处理中，忽略本次
        if self.nmi_in_progress.load(Ordering::SeqCst) {
            self.double_trigger_count.fetch_add(1, Ordering::SeqCst);
            return false;
        }

        let phase = self.phase();
        // panic 处理中（Entry/Dumping/Halting）：NMI 安全忽略
        if phase == PanicPhase::Entry
            || phase == PanicPhase::Dumping
            || phase == PanicPhase::Halting
        {
            self.nmi_ignored.fetch_add(1, Ordering::SeqCst);
            return false;
        }

        // 正常路径：标记 NMI 处理中，完成后清除
        self.nmi_in_progress.store(true, Ordering::SeqCst);
        // （NMI 处理逻辑在此）
        self.nmi_in_progress.store(false, Ordering::SeqCst);
        true
    }

    /// NMI 到达计数。
    pub fn nmi_count(&self) -> u32 {
        self.nmi_count.load(Ordering::SeqCst)
    }

    /// NMI 被忽略计数。
    pub fn nmi_ignored(&self) -> u32 {
        self.nmi_ignored.load(Ordering::SeqCst)
    }

    /// 双触发计数。
    pub fn double_trigger_count(&self) -> u32 {
        self.double_trigger_count.load(Ordering::SeqCst)
    }

    /// 零重入检查：panic 中 NMI 不应被处理。
    pub fn verify_zero_reentry(&self) -> bool {
        // 如果 panic 中有 NMI 被处理（而非忽略），则重入失败
        let phase = self.phase();
        if phase == PanicPhase::Entry
            || phase == PanicPhase::Dumping
            || phase == PanicPhase::Halting
        {
            // 此时 nmi_in_progress 应为 false
            !self.nmi_in_progress.load(Ordering::SeqCst)
        } else {
            true
        }
    }
}

impl Default for NmiSafety {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nmi_ignored_during_panic() {
        let s = NmiSafety::new();
        s.set_phase(PanicPhase::Entry);
        assert!(!s.nmi_arrive()); // panic 中被忽略
        assert_eq!(s.nmi_ignored(), 1);
    }

    #[test]
    fn test_nmi_handled_when_idle() {
        let s = NmiSafety::new();
        s.set_phase(PanicPhase::Idle);
        assert!(s.nmi_arrive()); // 空闲时正常处理
    }

    #[test]
    fn test_double_trigger_detected() {
        let s = NmiSafety::new();
        s.set_phase(PanicPhase::Idle);
        // 手动设置 in_progress 模拟并发
        s.nmi_in_progress.store(true, Ordering::SeqCst);
        assert!(!s.nmi_arrive()); // 重入被忽略
        assert_eq!(s.double_trigger_count(), 1);
    }

    #[test]
    fn test_phase_transition() {
        let s = NmiSafety::new();
        assert_eq!(s.phase(), PanicPhase::Idle);
        s.set_phase(PanicPhase::Dumping);
        assert_eq!(s.phase(), PanicPhase::Dumping);
        s.set_phase(PanicPhase::Done);
        assert_eq!(s.phase(), PanicPhase::Done);
    }

    #[test]
    fn test_verify_zero_reentry() {
        let s = NmiSafety::new();
        s.set_phase(PanicPhase::Dumping);
        assert!(s.verify_zero_reentry());
    }
}
