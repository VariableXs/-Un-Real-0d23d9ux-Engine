//! UNREAL-X AI-04 启动收官与遥测（族0031 启动遥测 · X00751~X00775 K 线落点）。
//! 内核启动遥测通道：no_std 安全的固定容量环形缓冲，记录启动各阶段耗时。
//! 与 code-analysis/core/src/ai04.rs 的 C 线遥测口径一致（事件名 ≤8 字节、
//! 时长 u32 毫秒、环形满载挤最旧）。



/// 遥测事件：阶段名 + 耗时（毫秒）。
pub struct Event {
    pub name: [u8; 8],
    pub name_len: usize,
    pub ms: u32,
}

/// 固定容量启动遥测环形缓冲。
pub struct BootTelemetry {
    buf: [Option<Event>; CAP],
    head: usize,
    pub count: usize,
    pub dropped: u32,
}

const CAP: usize = 16;

impl BootTelemetry {
    pub const fn new() -> Self {
        BootTelemetry { buf: [const { None }; CAP], head: 0, count: 0, dropped: 0 }
    }

    /// 记录一个阶段耗时；满载挤掉最旧并计 dropped。
    pub fn record(&mut self, name: &str, ms: u32) {
        let bytes = name.as_bytes();
        let n = bytes.len().min(8);
        let mut ev = Event { name: [0; 8], name_len: n, ms };
        ev.name[..n].copy_from_slice(&bytes[..n]);
        if self.count == CAP {
            self.dropped += 1;
        } else {
            self.count += 1;
        }
        self.buf[self.head] = Some(ev);
        self.head = (self.head + 1) % CAP;
    }

    /// 按时间序遍历（旧 → 新）。
    pub fn iter(&self) -> impl Iterator<Item = &Event> {
        let start = if self.count == CAP { self.head } else { 0 };
        (0..self.count).filter_map(move |i| self.buf[(start + i) % CAP].as_ref())
    }

    /// 全链路总时长。
    pub fn span_ms(&self) -> u32 {
        self.iter().map(|e| e.ms).fold(0u32, |a, b| a.saturating_add(b))
    }

    /// 最大单阶段耗时。
    pub fn worst_ms(&self) -> u32 {
        self.iter().map(|e| e.ms).max().unwrap_or(0)
    }

    /// 遥测摘要：(总时长, 最差阶段, 丢弃数)。串口打印由调用方负责。
    pub fn summary(&self) -> (u32, u32, u32) {
        (self.span_ms(), self.worst_ms(), self.dropped)
    }
}

// ---- 自检（host 上 cargo ktest / cargo test 运行）----

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn telemetry_ring_and_span() {
        let mut t = BootTelemetry::new();
        t.record("fw", 40);
        t.record("kernel", 120);
        t.record("shell", 200);
        assert_eq!(t.count, 3);
        assert_eq!(t.span_ms(), 360);
        assert_eq!(t.worst_ms(), 200);
        assert_eq!(t.iter().next().unwrap().name_len, 2);
        assert_eq!(&t.iter().next().unwrap().name[..2], b"fw");
        // 满载挤最旧
        for i in 0..CAP {
            t.record("fill", i as u32);
        }
        assert_eq!(t.count, CAP);
        assert_eq!(t.dropped, 3); // 前面已占 3 格，本轮挤掉 3 条旧事件
        t.record("overflow", 1);
        assert_eq!(t.count, CAP);
        assert_eq!(t.dropped, 4);
        // 名字截断到 8 字节
        t.record("verylongname", 2);
        let last = t.iter().last().unwrap();
        assert_eq!(last.name_len, 8);
        // X00751~X00775 全链路口径：总时长可累计、worst 可判
        assert!(t.span_ms() > 0 && t.worst_ms() > 0);
    }
}
