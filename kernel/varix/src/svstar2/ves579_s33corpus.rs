//! VE-F4345 · 一致性语料库（U08 测试与资产组）
//! 职责：一致性测试语料库——语料四族（违例样本库 / 判例文本库 / fuzz 病态
//! 库 / 多设备行为库）+ 语料治理三流程（标注 + 版本 + 去重）+ 语料共享（与
//! U07 性能资产、T08 语料互通复用——跨域参照不计用次）+ 标注缺失不可入池
//! + 哈希去重 + 语料污染来源审计隔离 + 无障碍违例样本族（域本色）。
//! 段位：0x3FE0（续表 idx579）。行数预算 340。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x3FE0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CpCode {
    /// 标注缺失（不可入池）。
    Unlabeled = SEG_BASE,
    /// 重复语料（哈希去重）。
    Duplicate = SEG_BASE + 0x01,
    /// 语料污染（来源审计隔离）。
    Contaminated = SEG_BASE + 0x02,
    /// 族号越界。
    FamilyRange = SEG_BASE + 0x03,
}

/// 四族。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Family {
    ViolationSamples,
    CaseTexts,
    FuzzPatho,
    DeviceBehavior,
}

/// 语料条目。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Corpus {
    pub family: Family,
    /// 内容哈希（去重键）。
    pub hash: u32,
    /// 标注完成（标注缺失不可入池）。
    pub labeled: bool,
    pub ver: u32,
    /// 来源域（0=本组 1=U07 2=T08——互通映射）。
    pub src: u8,
    /// 隔离标记（污染审计后）。
    pub quarantined: bool,
    /// 无障碍违例样本标记（域色——至少一族有）。
    pub a11y: bool,
}

/// 语料库台。
#[derive(Default)]
pub struct CorpusDesk {
    pub items: Vec<Corpus>,
    /// 互通映射：允许入池的来源域。
    pub allowed_src: Vec<u8>,
    pub quarantines: u32,
    pub dedup_hits: u32,
}

fn fnv(x: u32) -> u32 {
    let mut h: u32 = 0x811C9DC5;
    let mut v = x;
    for _ in 0..4 {
        h ^= v & 0xFF;
        h = h.wrapping_mul(0x01000193);
        v >>= 8;
    }
    h
}

impl CorpusDesk {
    pub fn new() -> Self {
        CorpusDesk { allowed_src: vec![0, 1, 2], ..Default::default() }
    }

    /// 语料入池：标注缺失拒收 + 哈希去重 + 来源审计（不在互通映射 = 污染隔离）。
    pub fn ingest(&mut self, family: Family, content: u32, labeled: bool, ver: u32, src: u8, a11y: bool) -> Result<(), CpCode> {
        if !labeled {
            return Err(CpCode::Unlabeled);
        }
        let hash = fnv(content);
        if self.items.iter().any(|c| c.hash == hash && !c.quarantined) {
            self.dedup_hits += 1;
            return Err(CpCode::Duplicate);
        }
        if !self.allowed_src.contains(&src) {
            // 来源不可信 = 污染嫌疑：隔离登记（不入主池——标记态保留审计）。
            self.items.push(Corpus { family, hash, labeled, ver, src, quarantined: true, a11y });
            self.quarantines += 1;
            return Err(CpCode::Contaminated);
        }
        self.items.push(Corpus { family, hash, labeled, ver, src, quarantined: false, a11y });
        Ok(())
    }

    /// 四族齐备断言（去隔离口径）。
    pub fn all_families(&self) -> bool {
        (0..4).all(|i| self.items.iter().any(|c| !c.quarantined && c.family as usize == i))
    }

    /// 无障碍违例样本族在册断言（域色）。
    pub fn a11y_family_present(&self) -> bool {
        self.items.iter().any(|c| !c.quarantined && c.a11y)
    }

    /// 版本演进：同语料内容更新版本（哈希不变——覆盖 ver）。
    pub fn bump_ver(&mut self, content: u32, new_ver: u32) -> Result<(), CpCode> {
        let hash = fnv(content);
        let c = self.items.iter_mut().find(|c| c.hash == hash && !c.quarantined).ok_or(CpCode::Unlabeled)?;
        c.ver = new_ver;
        Ok(())
    }

    /// 互通共享出库：主池语料按族导出（隔离件不出）。
    pub fn export_family(&self, f: Family) -> Vec<u32> {
        self.items
            .iter()
            .filter(|c| c.family == f && !c.quarantined)
            .map(|c| c.hash)
            .collect()
    }

    /// 隔离件审计放行（来源补审通过 → 解除隔离入主池）。
    pub fn audit_release(&mut self, hash: u32) -> Result<(), CpCode> {
        let c = self.items.iter_mut().find(|c| c.hash == hash && c.quarantined).ok_or(CpCode::Contaminated)?;
        c.quarantined = false;
        Ok(())
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves579_checks() -> CheckSet {
    let mut set = CheckSet::new("ves579");
    let mut cd = CorpusDesk::new();

    // C579-01 四族语料入池（标注齐备）。
    let fams = [Family::ViolationSamples, Family::CaseTexts, Family::FuzzPatho, Family::DeviceBehavior];
    let mut ok = true;
    for (i, f) in fams.iter().enumerate() {
        if cd.ingest(*f, 100 + i as u32, true, 1, 0, i == 0).is_err() {
            ok = false;
        }
    }
    chk(&mut set, "C579-01", ok && cd.all_families() && cd.a11y_family_present(), "四族入池含无障碍族");

    // C579-02 标注缺失拒收（不可入池）。
    chk(&mut set, "C579-02", cd.ingest(Family::CaseTexts, 500, false, 1, 0, false) == Err(CpCode::Unlabeled), "缺标注拒收");

    // C579-03 哈希去重（同内容拒绝计数）。
    chk(&mut set, "C579-03", cd.ingest(Family::CaseTexts, 101, true, 2, 0, false) == Err(CpCode::Duplicate)
        && cd.dedup_hits == 1, "哈希去重计数");

    // C579-04 互通来源入池（U07/T08 共享语料）。
    chk(&mut set, "C579-04", cd.ingest(Family::FuzzPatho, 300, true, 1, 1, false).is_ok()
        && cd.ingest(Family::DeviceBehavior, 301, true, 1, 2, false).is_ok(), "互通来源入池");

    // C579-05 非映射来源 = 污染隔离。
    chk(&mut set, "C579-05", cd.ingest(Family::CaseTexts, 400, true, 1, 9, false) == Err(CpCode::Contaminated)
        && cd.quarantines == 1, "污染隔离登记");

    // C579-06 隔离件不参与去重/出库。
    chk(&mut set, "C579-06", cd.export_family(Family::CaseTexts).len() == 1, "隔离件不出库");

    // C579-07 审计放行（隔离解除入主池）。
    let h400 = fnv(400);
    chk(&mut set, "C579-07", cd.audit_release(h400).is_ok()
        && cd.export_family(Family::CaseTexts).contains(&h400), "审计放行入主池");

    // C579-08 版本演进（哈希不变覆盖 ver）。
    chk(&mut set, "C579-08", cd.bump_ver(100, 3).is_ok()
        && cd.items.iter().find(|c| c.hash == fnv(100)).map(|c| c.ver) == Some(3), "版本演进覆盖");

    // C579-09 未知语料版本推进拒。
    chk(&mut set, "C579-09", cd.bump_ver(0x7777, 2).is_err(), "未知语料拒");

    // C579-10 出库按族精确（fuzz 族 2 件）。
    chk(&mut set, "C579-10", cd.export_family(Family::FuzzPatho).len() == 2, "按族出库精确");

    // C579-11 污染件阻断四族断言（隔离态下 fuzz 族仍有活件——不断言全灭）。
    let mut cd2 = CorpusDesk::new();
    let _ = cd2.ingest(Family::ViolationSamples, 1, true, 1, 9, true); // 全隔离
    chk(&mut set, "C579-11", !cd2.all_families() && !cd2.a11y_family_present(), "全隔离断言假");

    // C579-12 空库互通映射默认含本组（不臆测外源）。
    let cd3 = CorpusDesk::new();
    chk(&mut set, "C579-12", cd3.allowed_src == [0, 1, 2] && cd3.export_family(Family::FuzzPatho).is_empty(), "默认映射+空库安全");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_hardline() {
        let mut cd = CorpusDesk::new();
        assert_eq!(cd.ingest(Family::CaseTexts, 1, false, 1, 0, false), Err(CpCode::Unlabeled));
        assert!(cd.ingest(Family::CaseTexts, 1, true, 1, 0, false).is_ok());
    }

    #[test]
    fn dedup_by_hash() {
        let mut cd = CorpusDesk::new();
        assert!(cd.ingest(Family::FuzzPatho, 42, true, 1, 0, false).is_ok());
        assert_eq!(cd.ingest(Family::FuzzPatho, 42, true, 2, 0, false), Err(CpCode::Duplicate));
        assert_eq!(cd.dedup_hits, 1);
    }

    #[test]
    fn contamination_quarantine() {
        let mut cd = CorpusDesk::new();
        assert_eq!(cd.ingest(Family::CaseTexts, 7, true, 1, 9, false), Err(CpCode::Contaminated));
        assert_eq!(cd.quarantines, 1);
        assert!(cd.export_family(Family::CaseTexts).is_empty());
        assert!(cd.audit_release(fnv(7)).is_ok());
        assert_eq!(cd.export_family(Family::CaseTexts), [fnv(7)]);
    }
}
