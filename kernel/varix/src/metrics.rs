//! AURORA-1000 步骤 0028 · 全局可观测计数器。
//!
//! no_std 全局原子计数器：帧、事件、分配、中断、丢包等。
//! 可读、可累加、可清零；宿主单测覆盖基本语义。

use core::sync::atomic::{AtomicU64, Ordering};

/// 单项计数器：原子递增 / 读取 / 清零。
pub struct Counter {
    name: &'static str,
    v: AtomicU64,
}

impl Counter {
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            v: AtomicU64::new(0),
        }
    }

    pub const fn name(&self) -> &'static str {
        self.name
    }

    #[inline]
    pub fn add(&self, n: u64) {
        self.v.fetch_add(n, Ordering::Relaxed);
    }

    #[inline]
    pub fn inc(&self) {
        self.add(1);
    }

    #[inline]
    pub fn get(&self) -> u64 {
        self.v.load(Ordering::Relaxed)
    }

    /// 读后清零（快照语义）。
    pub fn take(&self) -> u64 {
        self.v.swap(0, Ordering::Relaxed)
    }

    pub fn reset(&self) {
        self.v.store(0, Ordering::Relaxed);
    }
}

/// 全局计数器登记表（静态分配，固定容量）。
/// 注意：必须是 `static` 而非 `const` —— const 会在每个使用点内联一份
/// 全新的零值副本，导致 add/get 操作不同的内存（计数永远为 0）。
pub static METRIC_FRAMES: Counter = Counter::new("frames");
pub static METRIC_EVENTS: Counter = Counter::new("events");
pub static METRIC_ALLOCS: Counter = Counter::new("allocs");
pub static METRIC_IRQS: Counter = Counter::new("irqs");
pub static METRIC_DROPS: Counter = Counter::new("drops");
pub static METRIC_HOTPLUG: Counter = Counter::new("hotplug");
pub static METRIC_FOCUS_SWITCH: Counter = Counter::new("focus_switch");
pub static METRIC_COMPOSE: Counter = Counter::new("compose");

/// 快照：登记表逐项取值（不动原始计数）。
pub struct Snapshot {
    pub entries: [(&'static str, u64); 8],
}

pub fn snapshot() -> Snapshot {
    Snapshot {
        entries: [
            (METRIC_FRAMES.name(), METRIC_FRAMES.get()),
            (METRIC_EVENTS.name(), METRIC_EVENTS.get()),
            (METRIC_ALLOCS.name(), METRIC_ALLOCS.get()),
            (METRIC_IRQS.name(), METRIC_IRQS.get()),
            (METRIC_DROPS.name(), METRIC_DROPS.get()),
            (METRIC_HOTPLUG.name(), METRIC_HOTPLUG.get()),
            (METRIC_FOCUS_SWITCH.name(), METRIC_FOCUS_SWITCH.get()),
            (METRIC_COMPOSE.name(), METRIC_COMPOSE.get()),
        ],
    }
}

/// 全表清零。
pub fn reset_all() {
    METRIC_FRAMES.reset();
    METRIC_EVENTS.reset();
    METRIC_ALLOCS.reset();
    METRIC_IRQS.reset();
    METRIC_DROPS.reset();
    METRIC_HOTPLUG.reset();
    METRIC_FOCUS_SWITCH.reset();
    METRIC_COMPOSE.reset();
}

/// 渲染为串口友好的 ASCII 文本（供 logger 输出，无分配）。
pub fn render(out: &mut [u8]) -> usize {
    let snap = snapshot();
    let mut pos = 0;
    for (name, val) in snap.entries.iter() {
        // "name=val "
        for &b in name.as_bytes() {
            if pos < out.len() {
                out[pos] = b;
                pos += 1;
            }
        }
        if pos < out.len() {
            out[pos] = b'=';
            pos += 1;
        }
        let mut digits = [0u8; 20];
        let mut n = *val;
        let mut d = 0;
        if n == 0 {
            digits[0] = b'0';
            d = 1;
        } else {
            while n > 0 && d < 20 {
                digits[d] = b'0' + (n % 10) as u8;
                n /= 10;
                d += 1;
            }
        }
        while d > 0 {
            d -= 1;
            if pos < out.len() {
                out[pos] = digits[d];
                pos += 1;
            }
        }
        if pos < out.len() {
            out[pos] = b' ';
            pos += 1;
        }
    }
    pos
}

pub fn run_metrics_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("metrics");
    let c = Counter::new("probe");
    c.inc();
    c.add(9);
    set.add("M002 inc+add", c.get() == 10, "counter accumulates");
    let t = c.take();
    set.add("M002 take-and-clear", t == 10 && c.get() == 0, "snapshot clears");
    METRIC_FRAMES.reset();
    METRIC_FRAMES.add(5);
    set.add("M002 global counter", METRIC_FRAMES.get() == 5, "global add");
    METRIC_FRAMES.reset();
    let mut buf = [0u8; 128];
    METRIC_EVENTS.reset();
    METRIC_EVENTS.add(42);
    let n = render(&mut buf);
    let s = core::str::from_utf8(&buf[..n]).unwrap_or("");
    let render_ok = s.contains("frames=0 ") && s.contains("events=42 ");
    set.add("M002 render", render_ok, "frames=0 events=42");
    reset_all();
    let snap = snapshot();
    let all_zero = snap.entries.iter().all(|(_, v)| *v == 0);
    set.add("M002 reset-all", all_zero, "snapshot all zero after reset");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 全局计数器是进程级共享状态；涉 global 的测试必须串行执行，
    /// 否则并行测试互用 reset_all() 会相互清零（偶发 flaky）。
    static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn counter_semantics() {
        let c = Counter::new("t");
        c.inc();
        c.add(41);
        assert_eq!(c.get(), 42);
        assert_eq!(c.take(), 42);
        assert_eq!(c.get(), 0);
        assert_eq!(c.name(), "t");
    }

    #[test]
    fn snapshot_and_render() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset_all();
        METRIC_IRQS.add(7);
        assert_eq!(METRIC_IRQS.get(), 7, "add after reset lost, direct get");
        let mut buf = [0u8; 256];
        let n = render(&mut buf);
        let s = core::str::from_utf8(&buf[..n]).unwrap();
        assert!(s.contains("irqs=7 "), "got: {s}");
        reset_all();
        assert!(snapshot().entries.iter().all(|(_, v)| *v == 0));
    }

    #[test]
    fn metrics_domain_checks() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let set = run_metrics_checks();
        let (passed, failed) = set.tally();
        assert_eq!(failed, 0, "metrics self-check must pass: {} passed", passed);
    }
}
