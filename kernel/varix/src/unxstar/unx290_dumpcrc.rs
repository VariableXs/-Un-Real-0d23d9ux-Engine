//! unx290_dumpcrc ↔ UNX-F0290 转储三件套对账（F0123 深化：完整性核验闭环）
//!
//! 状态/栈/日志三件各自 CRC + 整块 CRC，对端核验口径冻结。
//!
//! 判据：UNX-F0290-J1 状态/栈/日志三件各自 CRC + 整块 CRC，对端核验口径冻结。

use alloc::vec;
use alloc::vec::Vec;

/// 简单 CRC32 实现（多项式 0xEDB88320）。
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFFFFFF;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB88320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

/// 转储三件套。
#[derive(Debug, Clone)]
pub struct DumpTriple {
    /// 状态转储字节。
    pub state: Vec<u8>,
    /// 栈转储字节。
    pub stack: Vec<u8>,
    /// 日志转储字节。
    pub log: Vec<u8>,
}

/// 三件套 CRC 对账记录。
#[derive(Debug, Clone)]
pub struct DumpCrcReport {
    /// 状态 CRC。
    pub state_crc: u32,
    /// 栈 CRC。
    pub stack_crc: u32,
    /// 日志 CRC。
    pub log_crc: u32,
    /// 整块 CRC（三件串联）。
    pub block_crc: u32,
    /// 对账是否通过。
    pub verified: bool,
}

/// 转储三件套对账器。
pub struct DumpCrcLedger {
    /// 对账记录。
    reports: Vec<DumpCrcReport>,
    /// 对账通过计数。
    pass_count: u64,
    /// 对账失败计数。
    fail_count: u64,
}

impl DumpCrcLedger {
    /// 新建对账器。
    pub fn new() -> Self {
        Self {
            reports: Vec::new(),
            pass_count: 0,
            fail_count: 0,
        }
    }

    /// 计算三件套 CRC 报告。
    pub fn compute(&self, dump: &DumpTriple) -> DumpCrcReport {
        let state_crc = crc32(&dump.state);
        let stack_crc = crc32(&dump.stack);
        let log_crc = crc32(&dump.log);

        // 整块 CRC：三件串联
        let mut combined = Vec::new();
        combined.extend_from_slice(&dump.state);
        combined.extend_from_slice(&dump.stack);
        combined.extend_from_slice(&dump.log);
        let block_crc = crc32(&combined);

        DumpCrcReport {
            state_crc,
            stack_crc,
            log_crc,
            block_crc,
            verified: true,
        }
    }

    /// 登记对账（对端核验：用对端 CRC 比对）。
    pub fn verify_and_record(
        &mut self,
        dump: &DumpTriple,
        remote: &DumpCrcReport,
    ) -> bool {
        let local = self.compute(dump);
        let verified = local.state_crc == remote.state_crc
            && local.stack_crc == remote.stack_crc
            && local.log_crc == remote.log_crc
            && local.block_crc == remote.block_crc;

        let report = DumpCrcReport {
            verified,
            ..local
        };
        self.reports.push(report);

        if verified {
            self.pass_count += 1;
        } else {
            self.fail_count += 1;
        }
        verified
    }

    /// 获取最近一次报告。
    pub fn latest(&self) -> Option<&DumpCrcReport> {
        self.reports.last()
    }

    /// 对账统计。
    pub fn stats(&self) -> (u64, u64) {
        (self.pass_count, self.fail_count)
    }

    /// 报告总数。
    pub fn report_count(&self) -> usize {
        self.reports.len()
    }
}

impl Default for DumpCrcLedger {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crc32_known_value() {
        // CRC32("123456789") = 0xCBF43926
        assert_eq!(crc32(b"123456789"), 0xCBF43926);
    }

    #[test]
    fn test_compute_report() {
        let mut l = DumpCrcLedger::new();
        let dump = DumpTriple {
            state: vec![1, 2, 3],
            stack: vec![4, 5, 6],
            log: vec![7, 8, 9],
        };
        let report = l.compute(&dump);
        assert!(report.verified);
        assert_ne!(report.state_crc, 0);
    }

    #[test]
    fn test_verify_matching() {
        let mut l = DumpCrcLedger::new();
        let dump = DumpTriple {
            state: vec![0xAA],
            stack: vec![0xBB],
            log: vec![0xCC],
        };
        let remote = l.compute(&dump);
        assert!(l.verify_and_record(&dump, &remote));
        assert_eq!(l.stats().0, 1);
    }

    #[test]
    fn test_verify_mismatch() {
        let mut l = DumpCrcLedger::new();
        let dump = DumpTriple {
            state: vec![0xAA],
            stack: vec![0xBB],
            log: vec![0xCC],
        };
        let mut remote = l.compute(&dump);
        remote.state_crc ^= 0xFF; // 篡改
        assert!(!l.verify_and_record(&dump, &remote));
        assert_eq!(l.stats().1, 1);
    }

    #[test]
    fn test_empty_dump() {
        let l = DumpCrcLedger::new();
        let dump = DumpTriple {
            state: vec![],
            stack: vec![],
            log: vec![],
        };
        let report = l.compute(&dump);
        assert!(report.verified);
    }
}
