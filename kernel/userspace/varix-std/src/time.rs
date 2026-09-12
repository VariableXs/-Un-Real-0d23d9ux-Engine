//! F058 · 时钟包装。

use crate::abi::{CLOCK_MONOTONIC, CLOCK_REALTIME, SYS_CLOCK};
use crate::error::{invoke, Error, Result};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ClockId(pub u64);

impl ClockId {
    pub const REALTIME: ClockId = ClockId(CLOCK_REALTIME);
    pub const MONOTONIC: ClockId = ClockId(CLOCK_MONOTONIC);
}

/// 时间点，纳秒精度、秒/纳秒分离（与内核 `(sec, nsec)` 布局一致）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub struct Timespec {
    pub sec: u64,
    pub nsec: u64,
}

impl Timespec {
    pub const ZERO: Timespec = Timespec { sec: 0, nsec: 0 };

    pub const fn to_nanos(self) -> u64 {
        self.sec
            .saturating_mul(1_000_000_000)
            .saturating_add(self.nsec)
    }

    pub const fn from_nanos(ns: u64) -> Timespec {
        Timespec {
            sec: ns / 1_000_000_000,
            nsec: ns % 1_000_000_000,
        }
    }

    /// 两个时间点之差（只用于单调钟；真实钟会回退，必须调用方自己判断）。
    pub const fn saturating_sub(self, earlier: Timespec) -> Timespec {
        Timespec::from_nanos(self.to_nanos().saturating_sub(earlier.to_nanos()))
    }
}

/// `clock_gettime(id, tp)`。内核侧 `tp` 由 F054 校验，这里只保证拿到结构体。
pub fn clock_gettime(id: ClockId) -> Result<Timespec> {
    let mut out = [0u64; 2];
    unsafe {
        invoke(SYS_CLOCK, [id.0, out.as_mut_ptr() as u64, 0, 0, 0, 0])?;
    }
    if out[1] >= 1_000_000_000 {
        // 内核不该回一个非规格化的纳秒字段；与其让它悄悄溢出到下一秒，
        // 不如明确报错（用户态最后一次防线）。
        return Err(Error::Range);
    }
    Ok(Timespec {
        sec: out[0],
        nsec: out[1],
    })
}

/// 单调纳秒——最常用的形式。
pub fn monotonic_nanos() -> Result<u64> {
    Ok(clock_gettime(ClockId::MONOTONIC)?.to_nanos())
}

/// 墙钟纳秒。
pub fn realtime_nanos() -> Result<u64> {
    Ok(clock_gettime(ClockId::REALTIME)?.to_nanos())
}

/// F066/F068 · 如果内核提供 vDSO（`FEAT_VDSO`）且当前调用在导出表里，
/// 则这次 `clock_gettime` 可以在用户态完成，零陷入。
///
/// 本雏形只暴露判定：真正跳进 vDSO 页面需要解析内核写入的符号表，属于
/// 后续步骤；此处的价值是让调用方**现在就**能按能力位选择代码路径。
pub fn vdso_available(features: u64) -> bool {
    features & crate::abi::FEAT_VDSO != 0
}

/// 忙等（调试用；正式代码请用 `event_wait` 或 `yield_now`）。
pub fn spin_nanos(ns: u64) -> Result<()> {
    let start = monotonic_nanos()?;
    while monotonic_nanos()?.saturating_sub(start) < ns {
        core::hint::spin_loop();
    }
    Ok(())
}
