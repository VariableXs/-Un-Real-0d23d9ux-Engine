//! unx291_pcode ↔ UNX-F0291 panic 码表（P-码系）与 E-码映射
//!
//! P-码表全量三要素，与 F0206 E-码映射覆盖 100%。
//!
//! 判据：UNX-F0291-J1 P-码表全量三要素，与 F0206 E-码映射覆盖 100%。

use alloc::format;
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// P-码条目三要素。
#[derive(Debug, Clone)]
pub struct PCodeEntry {
    /// P-码值。
    pub code: u16,
    /// 语义描述。
    pub semantics: String,
    /// 建议处置。
    pub remedy: String,
    /// 映射的 E-码（全局错误码）。
    pub e_code: u16,
}

/// P-码表。
pub struct PCodeTable {
    /// P-码表。
    entries: BTreeMap<u16, PCodeEntry>,
    /// E-码映射表（E-码 → P-码）。
    e_to_p: BTreeMap<u16, u16>,
}

impl PCodeTable {
    /// 新建空码表。
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
            e_to_p: BTreeMap::new(),
        }
    }

    /// 登记 P-码（三要素齐备）。
    pub fn register(
        &mut self,
        code: u16,
        semantics: &str,
        remedy: &str,
        e_code: u16,
    ) -> Result<(), u16> {
        if semantics.is_empty() || remedy.is_empty() {
            return Err(0x9101); // 三要素不齐
        }
        if self.entries.contains_key(&code) {
            return Err(0x9102); // 重复
        }
        self.entries.insert(
            code,
            PCodeEntry {
                code,
                semantics: semantics.to_string(),
                remedy: remedy.to_string(),
                e_code,
            },
        );
        self.e_to_p.insert(e_code, code);
        Ok(())
    }

    /// 查询 P-码。
    pub fn lookup_p(&self, code: u16) -> Option<&PCodeEntry> {
        self.entries.get(&code)
    }

    /// 通过 E-码查找 P-码。
    pub fn lookup_by_e(&self, e_code: u16) -> Option<&PCodeEntry> {
        self.e_to_p.get(&e_code).and_then(|&p| self.entries.get(&p))
    }

    /// 映射覆盖率检查（与 F0206 E-码集对比）。
    pub fn coverage_check(&self, e_codes: &[u16]) -> (usize, usize) {
        let covered = e_codes
            .iter()
            .filter(|e| self.e_to_p.contains_key(e))
            .count();
        (covered, e_codes.len())
    }

    /// 全量条目。
    pub fn all(&self) -> Vec<&PCodeEntry> {
        self.entries.values().collect()
    }

    /// 条目总数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 三要素完整性检查。
    pub fn verify_completeness(&self) -> Vec<String> {
        self.entries
            .values()
            .filter(|e| e.semantics.is_empty() || e.remedy.is_empty())
            .map(|e| alloc::format!("P-码 0x{:04X} 三要素不全", e.code))
            .collect()
    }
}

impl Default for PCodeTable {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_and_lookup() {
        let mut t = PCodeTable::new();
        t.register(0xF001, "栈耗尽", "扩大栈或减少递归", 0xE101).unwrap();
        assert_eq!(t.len(), 1);
        let e = t.lookup_p(0xF001).unwrap();
        assert_eq!(e.e_code, 0xE101);
    }

    #[test]
    fn test_lookup_by_e_code() {
        let mut t = PCodeTable::new();
        t.register(0xF001, "三重故障", "查页表", 0xE101).unwrap();
        let p = t.lookup_by_e(0xE101).unwrap();
        assert_eq!(p.code, 0xF001);
    }

    #[test]
    fn test_coverage_check() {
        let mut t = PCodeTable::new();
        t.register(0xF001, "s", "r", 0xE101).unwrap();
        t.register(0xF002, "s2", "r2", 0xE102).unwrap();
        let (covered, total) = t.coverage_check(&[0xE101, 0xE102, 0xE103]);
        assert_eq!(covered, 2);
        assert_eq!(total, 3);
    }

    #[test]
    fn test_three_elements_required() {
        let mut t = PCodeTable::new();
        assert_eq!(t.register(0xF001, "", "r", 0xE101), Err(0x9101));
        assert_eq!(t.register(0xF001, "s", "", 0xE101), Err(0x9101));
    }

    #[test]
    fn test_duplicate_rejected() {
        let mut t = PCodeTable::new();
        t.register(0xF001, "s", "r", 0xE101).unwrap();
        assert_eq!(t.register(0xF001, "s2", "r2", 0xE102), Err(0x9102));
    }
}
