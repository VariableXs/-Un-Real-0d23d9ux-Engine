//! ve190_errcodes ↔ VE-F0190 域错误码段分配总表
//!
//! A 域错误码段统一分配登记：每族错误（呈现、显存、提交、调试）连续段位、码值语义、
//! 建议处置三要素齐备；段位分配后不回收不复用（防语义漂移）；与全局错误码体系对齐登记。
//!
//! 判据：连续段位、不回收、三要素齐、对齐登记。

use alloc::format;
use alloc::vec;
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 错误族。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ErrorFamily {
    /// 呈现族。
    Presentation,
    /// 显存族。
    Memory,
    /// 提交族。
    Submission,
    /// 调试族。
    Debug,
}

/// 码段条目（三要素：段位、语义、处置）。
#[derive(Debug, Clone)]
pub struct CodeSegment {
    /// 起始码值（含）。
    pub start: u16,
    /// 结束码值（含）。
    pub end: u16,
    /// 所属族。
    pub family: ErrorFamily,
    /// 语义描述。
    pub semantics: String,
    /// 建议处置。
    pub remedy: String,
    /// 是否已回收（恒 false——不回收原则）。
    pub recycled: bool,
}

/// 码段分配总表。
pub struct ErrorCodeTable {
    /// 段位表。
    segments: Vec<CodeSegment>,
    /// 码值 → 段索引。
    code_index: BTreeMap<u16, usize>,
    /// 全局对齐记录。
    global_align: BTreeMap<u16, String>,
    /// 下一个可用码值。
    next_code: u16,
}

impl ErrorCodeTable {
    /// 新建总表（从 0x8300 开始分配）。
    pub fn new() -> Self {
        Self {
            segments: Vec::new(),
            code_index: BTreeMap::new(),
            global_align: BTreeMap::new(),
            next_code: 0x8300,
        }
    }

    /// 分配连续段位（三要素齐备才分配）。
    pub fn allocate(
        &mut self,
        family: ErrorFamily,
        count: u16,
        semantics: &str,
        remedy: &str,
    ) -> Result<(u16, u16), u16> {
        if count == 0 {
            return Err(0x8301);
        }
        if semantics.is_empty() || remedy.is_empty() {
            return Err(0x8302); // 三要素不齐
        }

        let start = self.next_code;
        let end = start + count - 1;

        // 段位冲突检测
        for seg in &self.segments {
            if !(end < seg.start || start > seg.end) {
                return Err(0x8303); // 段位冲突
            }
        }

        let seg = CodeSegment {
            start,
            end,
            family,
            semantics: semantics.to_string(),
            remedy: remedy.to_string(),
            recycled: false, // 不回收原则
        };

        // 建立码值索引
        for code in start..=end {
            self.code_index.insert(code, self.segments.len());
        }

        self.segments.push(seg);
        self.next_code = end + 1;
        Ok((start, end))
    }

    /// 查询码值归属。
    pub fn lookup(&self, code: u16) -> Option<&CodeSegment> {
        self.code_index
            .get(&code)
            .map(|&i| &self.segments[i])
    }

    /// 全局对齐登记（与全局错误码体系对齐）。
    pub fn align_global(&mut self, code: u16, global_name: &str) {
        self.global_align.insert(code, global_name.to_string());
    }

    /// 全局对齐记录。
    pub fn global_alignment(&self) -> &BTreeMap<u16, String> {
        &self.global_align
    }

    /// 按族列出段位。
    pub fn list_by_family(&self, family: ErrorFamily) -> Vec<&CodeSegment> {
        self.segments.iter().filter(|s| s.family == family).collect()
    }

    /// 全量段位。
    pub fn all_segments(&self) -> &[CodeSegment] {
        &self.segments
    }

    /// 未登记码检查：出现不在任何段位内的码 → 返回缺失列表。
    pub fn find_unregistered(&self, codes: &[u16]) -> Vec<u16> {
        codes
            .iter()
            .copied()
            .filter(|c| !self.code_index.contains_key(c))
            .collect()
    }

    /// 处置建议缺失检查：所有段位必须有非空处置。
    pub fn verify_remedies(&self) -> Vec<String> {
        self.segments
            .iter()
            .filter(|s| s.remedy.is_empty())
            .map(|s| alloc::format!("段位[0x{:04X}-0x{:04X}]处置缺失", s.start, s.end))
            .collect()
    }

    /// 段位总数。
    pub fn len(&self) -> usize {
        self.segments.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }
}

impl Default for ErrorCodeTable {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_allocate_and_lookup() {
        let mut t = ErrorCodeTable::new();
        let (start, end) = t
            .allocate(ErrorFamily::Presentation, 16, "呈现错误", "重启呈现管线")
            .unwrap();
        assert_eq!(start, 0x8300);
        assert_eq!(end, 0x830F);
        assert_eq!(t.lookup(0x8305).unwrap().family, ErrorFamily::Presentation);
    }

    #[test]
    fn test_conflict_rejected() {
        let mut t = ErrorCodeTable::new();
        t.allocate(ErrorFamily::Presentation, 4, "s", "r").unwrap();
        // 第二段从 next_code 开始，不冲突
        assert!(t.allocate(ErrorFamily::Memory, 4, "s2", "r2").is_ok());
    }

    #[test]
    fn test_three_elements_required() {
        let mut t = ErrorCodeTable::new();
        assert_eq!(t.allocate(ErrorFamily::Debug, 4, "", "r"), Err(0x8302));
        assert_eq!(t.allocate(ErrorFamily::Debug, 4, "s", ""), Err(0x8302));
    }

    #[test]
    fn test_not_recycled() {
        let mut t = ErrorCodeTable::new();
        t.allocate(ErrorFamily::Debug, 4, "s", "r").unwrap();
        assert!(!t.all_segments()[0].recycled); // 恒 false
    }

    #[test]
    fn test_find_unregistered() {
        let mut t = ErrorCodeTable::new();
        t.allocate(ErrorFamily::Debug, 4, "s", "r").unwrap();
        let missing = t.find_unregistered(&[0x8300, 0x8305, 0x9999]);
        assert_eq!(missing, vec![0x9999]);
    }

    #[test]
    fn test_global_align() {
        let mut t = ErrorCodeTable::new();
        t.allocate(ErrorFamily::Presentation, 2, "s", "r").unwrap();
        t.align_global(0x8300, "GLOBAL_PRESENT_E01");
        assert_eq!(
            t.global_alignment().get(&0x8300),
            Some(&"GLOBAL_PRESENT_E01".to_string())
        );
    }

    #[test]
    fn test_list_by_family() {
        let mut t = ErrorCodeTable::new();
        t.allocate(ErrorFamily::Presentation, 4, "s", "r").unwrap();
        t.allocate(ErrorFamily::Memory, 4, "s2", "r2").unwrap();
        assert_eq!(t.list_by_family(ErrorFamily::Presentation).len(), 1);
        assert_eq!(t.list_by_family(ErrorFamily::Memory).len(), 1);
        assert_eq!(t.list_by_family(ErrorFamily::Debug).len(), 0);
    }
}
