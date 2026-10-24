// CGPU-F0318 域级基线归档 · cgb318_baseline.rs
// AI-MIMO-1022 · CGPU-B 收口
// 判据：性能/内存/代码/测试四基线快照归档；基线可对比；
//       归档不可变（只增不改）；版本化标签；完整性 CRC。
#![allow(dead_code)]

extern crate alloc;
use alloc::vec::Vec;

use super::super::checks::CheckSet;

/// 基线类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BaselineKind {
    Performance,
    Memory,
    CodeSize,
    TestCoverage,
    ApiSurface,
}

/// 基线条目。
#[derive(Clone, Debug)]
pub struct BaselineEntry {
    pub id: u32,
    pub kind: BaselineKind,
    pub metric_name_id: u32,
    pub value_q8: u64,
    pub version: u32,
    pub crc32: u32,
    pub immutable: bool,
}

/// 对比结果。
#[derive(Clone, Copy, Debug)]
pub struct BaselineDiff {
    pub entry_a: u32,
    pub entry_b: u32,
    pub delta_pct_q8: i32,
    pub regressed: bool,
}

/// 归档库。
pub struct BaselineArchive {
    pub entries: Vec<BaselineEntry>,
    pub diffs: Vec<BaselineDiff>,
    pub tag: u32,
    pub sealed: bool,
}

impl BaselineArchive {
    pub fn new(tag: u32) -> Self {
        Self {
            entries: Vec::new(),
            diffs: Vec::new(),
            tag,
            sealed: false,
        }
    }

    fn crc32(data: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFFu32;
        for &b in data {
            crc ^= b as u32;
            for _ in 0..8 {
                if crc & 1 != 0 {
                    crc = (crc >> 1) ^ 0xEDB8_8320;
                } else {
                    crc >>= 1;
                }
            }
        }
        !crc
    }

    /// 归档（封存后拒绝）。
    pub fn archive(&mut self, kind: BaselineKind, metric_name_id: u32, value_q8: u64, version: u32) -> Option<u32> {
        if self.sealed {
            return None;
        }
        let id = self.entries.len() as u32 + 1;
        let crc = Self::crc32(&value_q8.to_le_bytes());
        self.entries.push(BaselineEntry {
            id,
            kind,
            metric_name_id,
            value_q8,
            version,
            crc32: crc,
            immutable: false,
        });
        Some(id)
    }

    /// 封存（全部 immutable）。
    pub fn seal(&mut self) {
        self.sealed = true;
        for e in self.entries.iter_mut() {
            e.immutable = true;
        }
    }

    /// 两版对比。
    pub fn compare(&mut self, a_id: u32, b_id: u32, threshold_pct_q8: i32) -> Option<BaselineDiff> {
        let a = self.entries.iter().find(|e| e.id == a_id)?;
        let b = self.entries.iter().find(|e| e.id == b_id)?;
        if a.kind != b.kind || a.metric_name_id != b.metric_name_id {
            return None;
        }
        let delta = if a.value_q8 == 0 {
            0
        } else {
            (((b.value_q8 as i64 - a.value_q8 as i64) * 256) / a.value_q8 as i64) as i32
        };
        let d = BaselineDiff {
            entry_a: a_id,
            entry_b: b_id,
            delta_pct_q8: delta,
            regressed: delta > threshold_pct_q8,
        };
        self.diffs.push(d);
        Some(d)
    }

    /// CRC 完整性校验。
    pub fn verify_integrity(&self) -> bool {
        self.entries.iter().all(|e| Self::crc32(&e.value_q8.to_le_bytes()) == e.crc32)
    }

    /// 类别覆盖。
    pub fn kind_coverage(&self) -> u32 {
        let kinds = [
            BaselineKind::Performance,
            BaselineKind::Memory,
            BaselineKind::CodeSize,
            BaselineKind::TestCoverage,
            BaselineKind::ApiSurface,
        ];
        kinds.iter().filter(|k| self.entries.iter().any(|e| e.kind == **k)).count() as u32
    }

    pub fn run_checks(&self) -> CheckSet {
        let mut cs = CheckSet::new("CGPU-F0318 域级基线归档");
        cs.add("条目 id 唯一", {
            let mut ids: Vec<u32> = self.entries.iter().map(|e| e.id).collect();
            ids.sort_unstable();
            ids.dedup();
            ids.len() == self.entries.len()
        }, "unique");
        cs.add("CRC 完整", self.verify_integrity(), "crc");
        cs.add("五类覆盖 ≥ 3", self.kind_coverage() >= 3, ">=3/5");
        cs.add("封存后不可变", {
            !self.sealed || self.entries.iter().all(|e| e.immutable)
        }, "sealed");
        cs.add("对比引用有效", {
            self.diffs.iter().all(|d| self.entries.iter().any(|e| e.id == d.entry_a) && self.entries.iter().any(|e| e.id == d.entry_b))
        }, "valid refs");
        cs.add("标签非零", self.tag > 0, "tag");
        cs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_seal_compare() {
        let mut a = BaselineArchive::new(1);
        let id1 = a.archive(BaselineKind::Performance, 1, 1000, 1).unwrap();
        let id2 = a.archive(BaselineKind::Performance, 1, 1500, 2).unwrap();
        a.seal();
        assert!(a.archive(BaselineKind::Performance, 1, 2000, 3).is_none());
        let d = a.compare(id1, id2, 256).unwrap();
        assert!(d.regressed); // +50% > 100%
    }

    #[test]
    fn integrity() {
        let mut a = BaselineArchive::new(1);
        a.archive(BaselineKind::Memory, 1, 42, 1);
        assert!(a.verify_integrity());
    }
}
