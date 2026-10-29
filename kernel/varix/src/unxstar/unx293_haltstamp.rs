//! unx293_haltstamp ↔ UNX-F0293 停机戳语义（下次引导识别上轮 panic）
//!
//! 停机戳（P-码/指纹/阶段）持久化，下次引导识别并入报告。
//!
//! 判据：UNX-F0293-J1 停机戳（P-码/指纹/阶段）持久化，下次引导识别并入报告。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 停机戳（三要素：P-码/指纹/阶段）。
#[derive(Debug, Clone)]
pub struct HaltStamp {
    /// P-码（panic 码）。
    pub p_code: u16,
    /// 崩溃指纹（简单哈希）。
    pub fingerprint: u32,
    /// 停机阶段（字符串标签）。
    pub phase: String,
    /// 逻辑时间戳。
    pub timestamp: u64,
    /// 是否已消费（下次引导读取后标记）。
    pub consumed: bool,
}

/// 停机戳持久化存储。
pub struct HaltStampStore {
    /// 存储表（模拟持久化存储，key = 时间戳）。
    stamps: Vec<HaltStamp>,
    /// 容量上限。
    pub max_entries: usize,
}

impl HaltStampStore {
    /// 新建存储。
    pub fn new() -> Self {
        Self {
            stamps: Vec::new(),
            max_entries: 64,
        }
    }

    /// 写入停机戳（持久化语义）。
    pub fn write(
        &mut self,
        p_code: u16,
        fingerprint: u32,
        phase: &str,
        timestamp: u64,
    ) -> Result<(), u16> {
        if self.stamps.len() >= self.max_entries {
            self.stamps.remove(0); // 覆盖最旧
        }
        self.stamps.push(HaltStamp {
            p_code,
            fingerprint,
            phase: phase.to_string(),
            timestamp,
            consumed: false,
        });
        Ok(())
    }

    /// 下次引导识别：读取未消费的停机戳。
    pub fn detect_last_panic(&mut self) -> Option<HaltStamp> {
        for stamp in self.stamps.iter_mut().rev() {
            if !stamp.consumed {
                stamp.consumed = true;
                return Some(stamp.clone());
            }
        }
        None
    }

    /// 获取所有未消费停机戳。
    pub fn unconsumed(&self) -> Vec<&HaltStamp> {
        self.stamps.iter().filter(|s| !s.consumed).collect()
    }

    /// 生成引导报告（并入上次 panic 信息）。
    pub fn boot_report(&self) -> String {
        let unconsumed = self.unconsumed();
        if unconsumed.is_empty() {
            return "上次引导无 panic 记录".to_string();
        }
        let mut out = String::from("检测到上轮 panic：\n");
        for s in &unconsumed {
            out.push_str(&alloc::format!(
                "  P-码=0x{:04X}, 指纹=0x{:08X}, 阶段={}, 时间={}\n",
                s.p_code,
                s.fingerprint,
                s.phase,
                s.timestamp
            ));
        }
        out
    }

    /// 停机戳总数。
    pub fn len(&self) -> usize {
        self.stamps.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.stamps.is_empty()
    }

    /// 清空（重置出厂）。
    pub fn clear(&mut self) {
        self.stamps.clear();
    }
}

impl Default for HaltStampStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_write_and_detect() {
        let mut s = HaltStampStore::new();
        s.write(0xF001, 0xDEAD_0001, "Dumping", 100).unwrap();
        let stamp = s.detect_last_panic().unwrap();
        assert_eq!(stamp.p_code, 0xF001);
        assert_eq!(stamp.fingerprint, 0xDEAD_0001);
        assert!(stamp.consumed);
    }

    #[test]
    fn test_detect_consumed_once() {
        let mut s = HaltStampStore::new();
        s.write(0xF001, 0xAAAA, "Entry", 100).unwrap();
        s.detect_last_panic().unwrap();
        // 第二次 detect 返回 None（已消费）
        assert!(s.detect_last_panic().is_none());
    }

    #[test]
    fn test_boot_report() {
        let mut s = HaltStampStore::new();
        s.write(0xF001, 0xBBBB, "Halting", 200).unwrap();
        let report = s.boot_report();
        assert!(report.contains("P-码=0x"));
        assert!(report.contains("Halting"));
    }

    #[test]
    fn test_overflow_evicts_oldest() {
        let mut s = HaltStampStore::new();
        s.max_entries = 3;
        for i in 0..5 {
            s.write(0xF001, i, "Phase", i as u64).unwrap();
        }
        assert_eq!(s.len(), 3); // 只保留最新 3 条
    }

    #[test]
    fn test_empty_report() {
        let s = HaltStampStore::new();
        assert!(s.boot_report().contains("无 panic"));
    }
}
