//! unx289_logfreeze ↔ UNX-F0289 panic 前日志冻结与快照接线（F0108 深化）
//!
//! panic 点日志缓冲原子冻结，快照 256 条与 F0108 判据复测一致。
//!
//! 判据：UNX-F0289-J1 panic 点日志缓冲原子冻结，快照 256 条与 F0108 判据复测一致。

use alloc::vec::Vec;
use alloc::format;
use alloc::collections::VecDeque;
use alloc::string::{String, ToString};

/// 日志级别。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    /// 紧急。
    Emerg,
    /// 警告。
    Warning,
    /// 信息。
    Info,
    /// 调试。
    Debug,
}

/// 日志条目。
#[derive(Debug, Clone)]
pub struct LogEntry {
    /// 逻辑时间戳。
    pub timestamp: u64,
    /// 级别。
    pub level: LogLevel,
    /// 消息文本。
    pub message: String,
}

/// 冻结快照。
#[derive(Debug, Clone)]
pub struct FrozenSnapshot {
    /// 冻结时的逻辑时间。
    pub freeze_time: u64,
    /// 快照条目（最多 256 条）。
    pub entries: Vec<LogEntry>,
    /// 快照 CRC（简单校验和）。
    pub crc: u32,
}

/// panic 前日志冻结器。
pub struct LogFreezer {
    /// 环形日志缓冲。
    buffer: VecDeque<LogEntry>,
    /// 缓冲容量（默认 256）。
    pub capacity: usize,
    /// 是否已冻结。
    frozen: bool,
    /// 冻结快照。
    snapshot: Option<FrozenSnapshot>,
    /// 逻辑时钟。
    clock: u64,
}

impl LogFreezer {
    /// 新建日志冻结器。
    pub fn new() -> Self {
        Self {
            buffer: VecDeque::with_capacity(256),
            capacity: 256,
            frozen: false,
            snapshot: None,
            clock: 0,
        }
    }

    /// 写入日志（冻结后拒绝写入）。
    pub fn log(&mut self, level: LogLevel, message: &str) -> Result<(), u16> {
        if self.frozen {
            return Err(0x8901); // 已冻结
        }
        self.clock += 1;
        if self.buffer.len() >= self.capacity {
            self.buffer.pop_front(); // 环形覆盖最旧
        }
        self.buffer.push_back(LogEntry {
            timestamp: self.clock,
            level,
            message: message.to_string(),
        });
        Ok(())
    }

    /// 原子冻结：抓取快照并拒绝后续写入。
    pub fn freeze(&mut self) -> Result<&FrozenSnapshot, u16> {
        if self.frozen {
            return self.snapshot.as_ref().ok_or(0x8902);
        }
        self.frozen = true;

        let entries: Vec<LogEntry> = self.buffer.iter().cloned().collect();
        // 计算 CRC（简单校验和）
        let mut crc: u32 = 0;
        for e in &entries {
            crc = crc.wrapping_add(e.timestamp as u32);
            crc = crc.wrapping_add(e.message.len() as u32);
        }

        let snapshot = FrozenSnapshot {
            freeze_time: self.clock,
            entries,
            crc,
        };
        self.snapshot = Some(snapshot);
        self.snapshot.as_ref().ok_or(0x8902)
    }

    /// 获取快照（冻结前返回 None）。
    pub fn snapshot(&self) -> Option<&FrozenSnapshot> {
        self.snapshot.as_ref()
    }

    /// 是否已冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 当前缓冲条目数。
    pub fn count(&self) -> usize {
        self.buffer.len()
    }

    /// 快照条目数（应为 256 或当前缓冲大小）。
    pub fn snapshot_count(&self) -> usize {
        self.snapshot.as_ref().map(|s| s.entries.len()).unwrap_or(0)
    }

    /// 验证快照完整性（CRC 核验）。
    pub fn verify_snapshot(&self) -> bool {
        match &self.snapshot {
            Some(s) => {
                let mut crc: u32 = 0;
                for e in &s.entries {
                    crc = crc.wrapping_add(e.timestamp as u32);
                    crc = crc.wrapping_add(e.message.len() as u32);
                }
                crc == s.crc
            }
            None => false,
        }
    }
}

impl Default for LogFreezer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_and_freeze() {
        let mut f = LogFreezer::new();
        for i in 0..10 {
            f.log(LogLevel::Info, alloc::format!("msg {}", i).as_str()).unwrap();
        }
        assert_eq!(f.count(), 10);
        let snap = f.freeze().unwrap();
        assert_eq!(snap.entries.len(), 10);
        assert!(f.is_frozen());
    }

    #[test]
    fn test_frozen_rejects_write() {
        let mut f = LogFreezer::new();
        f.log(LogLevel::Info, "a").unwrap();
        f.freeze().unwrap();
        assert_eq!(f.log(LogLevel::Info, "b"), Err(0x8901));
    }

    #[test]
    fn test_ring_overflow() {
        let mut f = LogFreezer::new();
        f.capacity = 5;
        for i in 0..10 {
            f.log(LogLevel::Info, alloc::format!("{}", i).as_str()).unwrap();
        }
        assert_eq!(f.count(), 5); // 环形覆盖
    }

    #[test]
    fn test_snapshot_256() {
        let mut f = LogFreezer::new();
        for i in 0..300 {
            f.log(LogLevel::Info, alloc::format!("{}", i).as_str()).unwrap();
        }
        let snap = f.freeze().unwrap();
        assert_eq!(snap.entries.len(), 256); // 容量上限
    }

    #[test]
    fn test_verify_snapshot() {
        let mut f = LogFreezer::new();
        f.log(LogLevel::Info, "test").unwrap();
        f.freeze().unwrap();
        assert!(f.verify_snapshot());
    }
}
