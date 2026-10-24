//! ve185_frameanno ↔ VE-F0185 帧标注系统
//!
//! 帧级人工标注：开发者在录制流上打标注（里程碑、异常说明、待查标记），标注随命令流存档
//! （F0175 联动）随流回放；标注检索（按文本或类型）；标注不含用户内容。
//!
//! 判据：随流存档、回放对齐、检索、类型化。

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 标注类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AnnotationType {
    /// 里程碑标记。
    Milestone,
    /// 异常说明。
    Anomaly,
    /// 待查标记。
    Todo,
}

/// 标注条目。
#[derive(Debug, Clone)]
pub struct Annotation {
    /// 标注序号。
    pub seq: u64,
    /// 帧号（时间点）。
    pub frame: u64,
    /// 标注类型。
    pub kind: AnnotationType,
    /// 标注文本（截断到 max_len）。
    pub text: String,
    /// 文本是否被截断。
    pub truncated: bool,
}

/// 标注系统错误码。
pub mod err {
    /// 文本超长（已截断，提示用户）。
    pub const E_TEXT_TRUNCATED: u16 = 0x7E01;
    /// 检索无结果。
    pub const E_NO_MATCH: u16 = 0x7E02;
    /// 存档失败。
    pub const E_ARCHIVE_FAIL: u16 = 0x7E03;
}

/// 帧标注系统。
pub struct FrameAnnotator {
    /// 标注存储（按帧号排序索引 + 全量列表）。
    annotations: Vec<Annotation>,
    /// 按帧号索引（帧 → 标注序号列表）。
    by_frame: BTreeMap<u64, Vec<u64>>,
    /// 按类型索引。
    by_type: BTreeMap<AnnotationType, Vec<u64>>,
    /// 文本最大长度。
    pub max_text_len: usize,
    /// 下一个序号。
    next_seq: u64,
}

impl FrameAnnotator {
    /// 新建标注系统。
    pub fn new() -> Self {
        Self {
            annotations: Vec::new(),
            by_frame: BTreeMap::new(),
            by_type: BTreeMap::new(),
            max_text_len: 256,
            next_seq: 0,
        }
    }

    /// 打标注（文本超长自动截断并提示）。
    pub fn annotate(
        &mut self,
        frame: u64,
        kind: AnnotationType,
        text: &str,
    ) -> Result<u64, u16> {
        // 文本截断处理
        let (truncated_text, was_truncated) = if text.len() > self.max_text_len {
            let mut t = text.chars().take(self.max_text_len).collect::<String>();
            t.push('…');
            (t, true)
        } else {
            (text.to_string(), false)
        };

        let seq = self.next_seq;
        self.next_seq += 1;

        let anno = Annotation {
            seq,
            frame,
            kind,
            text: truncated_text,
            truncated: was_truncated,
        };

        self.by_frame.entry(frame).or_default().push(seq);
        self.by_type.entry(kind).or_default().push(seq);
        self.annotations.push(anno);

        if was_truncated {
            Err(err::E_TEXT_TRUNCATED) // 提示但仍已存储
        } else {
            Ok(seq)
        }
    }

    /// 获取标注。
    pub fn get(&self, seq: u64) -> Option<&Annotation> {
        self.annotations.get(seq as usize)
    }

    /// 按帧号查询标注。
    pub fn by_frame(&self, frame: u64) -> Vec<&Annotation> {
        self.by_frame
            .get(&frame)
            .map(|seqs| seqs.iter().filter_map(|&s| self.get(s)).collect())
            .unwrap_or_default()
    }

    /// 按类型查询标注。
    pub fn by_kind(&self, kind: AnnotationType) -> Vec<&Annotation> {
        self.by_type
            .get(&kind)
            .map(|seqs| seqs.iter().filter_map(|&s| self.get(s)).collect())
            .unwrap_or_default()
    }

    /// 按文本关键字检索（子串匹配）。
    pub fn search(&self, keyword: &str) -> Vec<&Annotation> {
        if keyword.is_empty() {
            return Vec::new();
        }
        self.annotations
            .iter()
            .filter(|a| a.text.contains(keyword))
            .collect()
    }

    /// 回放对齐：获取帧区间 [from, to] 内的所有标注。
    pub fn replay_align(&self, from_frame: u64, to_frame: u64) -> Vec<&Annotation> {
        self.annotations
            .iter()
            .filter(|a| a.frame >= from_frame && a.frame <= to_frame)
            .collect()
    }

    /// 标注总数。
    pub fn len(&self) -> usize {
        self.annotations.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.annotations.is_empty()
    }

    /// 全量标注列表（只读）。
    pub fn all(&self) -> &[Annotation] {
        &self.annotations
    }

    /// 构建随流存档数据（与 F0175 联动）。
    pub fn export_for_archive(&self) -> Vec<(u64, u8, &str)> {
        self.annotations
            .iter()
            .map(|a| {
                let kind_byte = match a.kind {
                    AnnotationType::Milestone => 0u8,
                    AnnotationType::Anomaly => 1,
                    AnnotationType::Todo => 2,
                };
                (a.frame, kind_byte, a.text.as_str())
            })
            .collect()
    }
}

impl Default for FrameAnnotator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_annotate_and_get() {
        let mut a = FrameAnnotator::new();
        let seq = a.annotate(100, AnnotationType::Milestone, "关键帧").unwrap();
        assert_eq!(a.len(), 1);
        assert_eq!(a.get(seq).unwrap().frame, 100);
    }

    #[test]
    fn test_text_truncation() {
        let mut a = FrameAnnotator::new();
        a.max_text_len = 5;
        let long_text = "a".repeat(20);
        let result = a.annotate(1, AnnotationType::Todo, &long_text);
        assert_eq!(result, Err(err::E_TEXT_TRUNCATED));
        // 仍然存储了
        assert_eq!(a.len(), 1);
        assert!(a.get(0).unwrap().truncated);
        assert!(a.get(0).unwrap().text.len() <= 7); // 5 + '…'
    }

    #[test]
    fn test_search_by_keyword() {
        let mut a = FrameAnnotator::new();
        a.annotate(1, AnnotationType::Anomaly, "GPU 超时").unwrap();
        a.annotate(2, AnnotationType::Anomaly, "CPU 过载").unwrap();
        a.annotate(3, AnnotationType::Todo, "检查 GPU 管线").unwrap();
        assert_eq!(a.search("GPU").len(), 2);
        assert_eq!(a.search("CPU").len(), 1);
        assert_eq!(a.search("none").len(), 0);
    }

    #[test]
    fn test_replay_align() {
        let mut a = FrameAnnotator::new();
        a.annotate(10, AnnotationType::Milestone, "a").unwrap();
        a.annotate(20, AnnotationType::Milestone, "b").unwrap();
        a.annotate(30, AnnotationType::Milestone, "c").unwrap();
        let aligned = a.replay_align(10, 20);
        assert_eq!(aligned.len(), 2);
    }

    #[test]
    fn test_type_index() {
        let mut a = FrameAnnotator::new();
        a.annotate(1, AnnotationType::Milestone, "m").unwrap();
        a.annotate(2, AnnotationType::Todo, "t").unwrap();
        assert_eq!(a.by_kind(AnnotationType::Milestone).len(), 1);
        assert_eq!(a.by_kind(AnnotationType::Todo).len(), 1);
    }

    #[test]
    fn test_export_for_archive() {
        let mut a = FrameAnnotator::new();
        a.annotate(5, AnnotationType::Anomaly, "anomaly!").unwrap();
        let data = a.export_for_archive();
        assert_eq!(data.len(), 1);
        assert_eq!(data[0].0, 5);
        assert_eq!(data[0].1, 1); // Anomaly = 1
    }
}
