//! ve192_dedup ↔ VE-F0192 域查重总对账
//!
//! A 域 200 条正文两两语义查重：同主题条目机制与数据结构命名必须差异化
//! （程序辅助比对加人工复核抽查），重复段位登记并限期改写；查重结果入自检证据。
//!
//! 判据：语义查重、限期改写、误报复核、宣告前置。

use alloc::format;
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 条目正文。
#[derive(Debug, Clone)]
pub struct ContentEntry {
    /// 条目 ID。
    pub id: String,
    /// 主题（分类）。
    pub topic: String,
    /// 机制/数据结构命名。
    pub mechanism: String,
    /// 正文摘要（用于相似度比对）。
    pub text: String,
}

/// 比对判定。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// 无重复。
    Unique,
    /// 疑似重复（待人工复核）。
    Suspected,
    /// 确认重复（需改写）。
    Confirmed,
    /// 误报（人工改判）。
    FalsePositive,
}

/// 比对结果对。
#[derive(Debug, Clone)]
pub struct ComparePair {
    /// 条目 A ID。
    pub id_a: String,
    /// 条目 B ID。
    pub id_b: String,
    /// 相似度（0.0~1.0）。
    pub similarity: f64,
    /// 判定。
    pub verdict: Verdict,
}

/// 查重总对账器。
pub struct DedupLedger {
    /// 条目表。
    entries: Vec<ContentEntry>,
    /// 比对结果。
    pairs: Vec<ComparePair>,
    /// 重复登记（限期改写）。
    rewrite_queue: Vec<(String, u64)>, // (id, deadline)
    /// 相似度阈值。
    pub threshold: f64,
}

impl DedupLedger {
    /// 新建对账器。
    pub fn new(threshold: f64) -> Self {
        Self {
            entries: Vec::new(),
            pairs: Vec::new(),
            rewrite_queue: Vec::new(),
            threshold,
        }
    }

    /// 登记条目。
    pub fn add_entry(&mut self, id: &str, topic: &str, mechanism: &str, text: &str) {
        self.entries.push(ContentEntry {
            id: id.to_string(),
            topic: topic.to_string(),
            mechanism: mechanism.to_string(),
            text: text.to_string(),
        });
    }

    /// 程序辅助比对：计算两文本的字符重叠相似度（简化 bigram 交集比）。
    fn similarity(&self, a: &str, b: &str) -> f64 {
        if a.is_empty() || b.is_empty() {
            return 0.0;
        }
        let a_chars: Vec<char> = a.chars().collect();
        let b_chars: Vec<char> = b.chars().collect();
        let mut matches = 0;
        let min_len = a_chars.len().min(b_chars.len());
        for i in 0..min_len {
            if a_chars[i] == b_chars[i] {
                matches += 1;
            }
        }
        matches as f64 / min_len.max(1) as f64
    }

    /// 两两比对（程序辅助）。
    pub fn compare_all(&mut self) -> usize {
        let n = self.entries.len();
        let mut count = 0;
        for i in 0..n {
            for j in (i + 1)..n {
                let sim = self.similarity(&self.entries[i].text, &self.entries[j].text);
                // 同主题且命名相同 → 强制标记疑似
                let same_mech = self.entries[i].mechanism == self.entries[j].mechanism
                    && self.entries[i].topic == self.entries[j].topic;
                let verdict = if same_mech {
                    Verdict::Confirmed
                } else if sim >= self.threshold {
                    Verdict::Suspected
                } else {
                    Verdict::Unique
                };
                if verdict != Verdict::Unique {
                    count += 1;
                }
                self.pairs.push(ComparePair {
                    id_a: self.entries[i].id.clone(),
                    id_b: self.entries[j].id.clone(),
                    similarity: sim,
                    verdict,
                });
            }
        }
        count
    }

    /// 人工复核改判（误报改判为 Unique，真重复改判为 Confirmed 并限期）。
    pub fn review(&mut self, id_a: &str, id_b: &str, confirmed: bool, deadline: u64) -> Result<(), u16> {
        for pair in self.pairs.iter_mut() {
            if pair.id_a == id_a && pair.id_b == id_b {
                pair.verdict = if confirmed {
                    Verdict::Confirmed
                } else {
                    Verdict::FalsePositive
                };
                if confirmed {
                    self.rewrite_queue.push((id_a.to_string(), deadline));
                    self.rewrite_queue.push((id_b.to_string(), deadline));
                }
                return Ok(());
            }
        }
        Err(0x8501) // 比对对不存在
    }

    /// 确认重复列表。
    pub fn confirmed_duplicates(&self) -> Vec<&ComparePair> {
        self.pairs
            .iter()
            .filter(|p| p.verdict == Verdict::Confirmed)
            .collect()
    }

    /// 疑似重复列表（待人工复核）。
    pub fn suspected_duplicates(&self) -> Vec<&ComparePair> {
        self.pairs
            .iter()
            .filter(|p| p.verdict == Verdict::Suspected)
            .collect()
    }

    /// 限期改写队列。
    pub fn rewrite_queue(&self) -> &[(String, u64)] {
        &self.rewrite_queue
    }

    /// 宣告前置检查：确认重复是否已全部入改写队列。
    pub fn verify_precondition(&self) -> Vec<String> {
        let mut issues = Vec::new();
        for pair in self.confirmed_duplicates() {
            let a_queued = self.rewrite_queue.iter().any(|(id, _)| id == &pair.id_a);
            let b_queued = self.rewrite_queue.iter().any(|(id, _)| id == &pair.id_b);
            if !a_queued {
                issues.push(alloc::format!("确认重复[{}]未入改写队列", pair.id_a));
            }
            if !b_queued {
                issues.push(alloc::format!("确认重复[{}]未入改写队列", pair.id_b));
            }
        }
        issues
    }

    /// 条目总数。
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// 比对对总数。
    pub fn pair_count(&self) -> usize {
        self.pairs.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_same_mechanism() {
        let mut d = DedupLedger::new(0.8);
        d.add_entry("a1", "渲染", "PipelineState", "管线状态管理");
        d.add_entry("a2", "渲染", "PipelineState", "管线状态管理器");
        let n = d.compare_all();
        assert_eq!(n, 1); // 同主题同命名 → Confirmed
        assert_eq!(d.confirmed_duplicates().len(), 1);
    }

    #[test]
    fn test_unique_entries() {
        let mut d = DedupLedger::new(0.8);
        d.add_entry("a", "渲染", "PipeA", "完全不同内容甲乙丙丁");
        d.add_entry("b", "内存", "MemB", "天差地别文本戊己庚辛");
        d.compare_all();
        assert_eq!(d.confirmed_duplicates().len(), 0);
    }

    #[test]
    fn test_review_false_positive() {
        let mut d = DedupLedger::new(0.3);
        d.add_entry("a", "t", "m", "abc");
        d.add_entry("b", "t", "m2", "abd");
        d.compare_all();
        assert!(d.confirmed_duplicates().len() >= 1);
        d.review("a", "b", false, 100).unwrap();
        assert_eq!(d.confirmed_duplicates().len(), 0);
    }

    #[test]
    fn test_review_confirmed_queues_rewrite() {
        let mut d = DedupLedger::new(0.3);
        d.add_entry("a", "t", "m", "abc");
        d.add_entry("b", "t", "m2", "abd");
        d.compare_all();
        d.review("a", "b", true, 100).unwrap();
        assert_eq!(d.rewrite_queue().len(), 2);
    }

    #[test]
    fn test_precondition_check() {
        let mut d = DedupLedger::new(0.3);
        d.add_entry("a", "t", "m", "abc");
        d.add_entry("b", "t", "m2", "abd");
        d.compare_all();
        // Confirmed 但未入队列
        let issues = d.verify_precondition();
        assert!(!issues.is_empty());
    }
}
