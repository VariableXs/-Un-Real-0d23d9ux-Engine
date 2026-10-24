//! ves437_s23knowledge — VE-F4197 · T 域知识库与经验归档
//! 归档：T 域收官知识归档（终审模式/联测案例/走查发现/变更案例四类入家族知识库+与 F4179 自查档合并去重）
//! +检索索引；重复条→去重合并；索引断→重建；类缺→补档；
//! 归档含多语种走查方法沉淀（域本色）；案例脱敏。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1360;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KbCode {
    Ok = 0,
    ClassMissing = SEG_BASE + 0x01, // 0x1361
    IndexBroken = SEG_BASE + 0x02,  // 0x1362
    NotSanitized = SEG_BASE + 0x03, // 0x1363
}

/// 四类归档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArchiveClass { FinalMode = 0, JointCase = 1, WalkFinding = 2, ChangeCase = 3 }

impl ArchiveClass {
    pub const ALL: [ArchiveClass; 4] = [
        ArchiveClass::FinalMode, ArchiveClass::JointCase, ArchiveClass::WalkFinding, ArchiveClass::ChangeCase];
}

/// 知识条目（案例脱敏标记 + 多语种方法沉淀标记——域本色）。
pub struct KbEntry {
    pub key: String,
    pub class: ArchiveClass,
    pub sanitized: bool,
    pub ml_method: bool, // 多语种走查方法沉淀（域本色）
}

/// 知识归档册。
pub struct KnowledgeBook {
    pub entries: Vec<KbEntry>,
    pub index: Vec<String>,     // 检索索引（key 列表）
    pub merged_from_f4179: Vec<String>, // 与 F4179 自查档合并去重记录
}

impl KnowledgeBook {
    pub fn new() -> Self { KnowledgeBook { entries: Vec::new(), index: Vec::new(), merged_from_f4179: Vec::new() } }
    pub fn put(&mut self, key: &str, class: ArchiveClass, sanitized: bool, ml: bool) {
        let key = String::from(key);
        if !self.entries.iter().any(|e| e.key == key) {
            self.entries.push(KbEntry { key: key.clone(), class, sanitized, ml_method: ml });
        }
    }
    /// 四类齐备核验。
    pub fn classes_full(&self) -> Result<(), KbCode> {
        for c in ArchiveClass::ALL {
            if !self.entries.iter().any(|e| e.class == c) { return Err(KbCode::ClassMissing); }
        }
        Ok(())
    }
    /// 建索引（key 排序稳定）。
    pub fn build_index(&mut self) {
        let mut keys: Vec<String> = self.entries.iter().map(|e| e.key.clone()).collect();
        keys.sort();
        self.index = keys;
    }
    /// 索引完整性核验（断检→重建码）。
    pub fn index_intact(&self) -> Result<(), KbCode> {
        if self.index.len() != self.entries.len() { return Err(KbCode::IndexBroken); }
        let mut sorted: Vec<&String> = self.entries.iter().map(|e| &e.key).collect();
        sorted.sort();
        for (i, k) in sorted.iter().enumerate() {
            if *k != &self.index[i] { return Err(KbCode::IndexBroken); }
        }
        Ok(())
    }
    /// 检索：按 key 前缀。
    pub fn search(&self, prefix: &str) -> Vec<&str> {
        self.index.iter().filter(|k| k.starts_with(prefix)).map(|k| k.as_str()).collect()
    }
    /// 与 F4179 自查档合并去重：返回实际新增数。
    pub fn merge_dedup(&mut self, f4179_keys: &[&str], class: ArchiveClass) -> usize {
        let mut added = 0;
        for k in f4179_keys {
            if !self.entries.iter().any(|e| e.key == *k) {
                self.entries.push(KbEntry { key: String::from(*k), class, sanitized: true, ml_method: false });
                self.merged_from_f4179.push(String::from(*k));
                added += 1;
            }
        }
        added
    }
    /// 案例脱敏核验：全部条目须脱敏（红线）。
    pub fn all_sanitized(&self) -> bool { self.entries.iter().all(|e| e.sanitized) }
    /// 多语种方法沉淀在册（域本色）。
    pub fn ml_method_present(&self) -> bool { self.entries.iter().any(|e| e.ml_method) }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves437_checks() -> CheckSet {
    let mut set = CheckSet::new("ves437");
    let mut bk = KnowledgeBook::new();

    // C437-01 类缺→补档码。
    bk.put("mode-final-a11y", ArchiveClass::FinalMode, true, true);
    chk(&mut set, "C437-01", bk.classes_full() == Err(KbCode::ClassMissing), "类缺补档");

    // C437-02 四类齐→核验过。
    bk.put("joint-case-12", ArchiveClass::JointCase, true, false);
    bk.put("walk-finding-rtl", ArchiveClass::WalkFinding, true, true);
    bk.put("change-case-freeze", ArchiveClass::ChangeCase, true, false);
    chk(&mut set, "C437-02", bk.classes_full().is_ok() && bk.entries.len() == 4, "四类齐");

    // C437-03 建索引+完整核验。
    bk.build_index();
    chk(&mut set, "C437-03", bk.index_intact().is_ok() && bk.index.len() == 4, "索引完整");

    // C437-04 索引断检→重建码。
    bk.index.pop();
    chk(&mut set, "C437-04", bk.index_intact() == Err(KbCode::IndexBroken), "断检定位");

    // C437-05 重建索引复验过。
    bk.build_index();
    chk(&mut set, "C437-05", bk.index_intact().is_ok(), "重建复验");

    // C437-06 前缀检索。
    let hits = bk.search("walk-");
    chk(&mut set, "C437-06", hits == vec!["walk-finding-rtl"], "前缀检索");

    // C437-07 与 F4179 合并去重：新 2 旧 1 → 仅新增 2。
    let added = bk.merge_dedup(&["mode-final-a11y", "f4179-self-1", "f4179-self-2"], ArchiveClass::FinalMode);
    chk(&mut set, "C437-07", added == 2 && bk.entries.len() == 6, "合并去重");

    // C437-08 合并记录留痕。
    chk(&mut set, "C437-08", bk.merged_from_f4179 == vec!["f4179-self-1", "f4179-self-2"], "合并留痕");

    // C437-09 条目幂等（重 key 不重入）。
    bk.put("mode-final-a11y", ArchiveClass::FinalMode, true, true);
    chk(&mut set, "C437-09", bk.entries.len() == 6, "条目幂等");

    // C437-10 案例脱敏全真核验（红线）。
    chk(&mut set, "C437-10", bk.all_sanitized(), "脱敏全真");

    // C437-11 多语种方法沉淀在册（域本色）。
    chk(&mut set, "C437-11", bk.ml_method_present(), "方法沉淀在册");

    // C437-12 四类单源断言。
    chk(&mut set, "C437-12", ArchiveClass::ALL.len() == 4
        && ArchiveClass::ALL[3] == ArchiveClass::ChangeCase, "四类单源");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves437_smoke() {
        let set = run_ves437_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves437 red: {:?}", red);
    }
}
