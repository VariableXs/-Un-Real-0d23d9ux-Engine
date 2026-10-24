//! ves317_s16docs — VE-F4077 · T04 翻译文档
//! 四类手册（流程五段各一册/键规范/术语库使用/机翻接入）+ 无障碍翻译专节 + 文档与实现一致性抽查（键名/状态机对拍，失配以代码为准修文档留痕）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x8FC0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum TdCode {
    Ok = 0,
    ManualMissing = SEG_BASE + 0x01,
    Drift = SEG_BASE + 0x02,
    A11ySectionMissing = SEG_BASE + 0x03,
    SampleNotFictional = SEG_BASE + 0x04,
}

/// 四类手册。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ManualKind { Pipeline = 0, KeySpec = 1, TermBook = 2, MtAccess = 3 }

impl ManualKind {
    pub const ALL: [ManualKind; 4] = [ManualKind::Pipeline, ManualKind::KeySpec, ManualKind::TermBook, ManualKind::MtAccess];
    pub fn name(self) -> &'static str {
        match self {
            ManualKind::Pipeline => "流程手册", ManualKind::KeySpec => "键规范手册",
            ManualKind::TermBook => "术语库手册", ManualKind::MtAccess => "机翻接入指南",
        }
    }
}

#[derive(Clone)]
pub struct Manual {
    pub kind: ManualKind,
    pub title: String,
    pub a11y_section: bool,
    pub sample_fictional: bool,
}

pub struct DocBook {
    pub manuals: Vec<Manual>,
    /// 一致性抽查留痕。
    pub audit_log: Vec<String>,
}

impl DocBook {
    pub fn new() -> Self { DocBook { manuals: Vec::new(), audit_log: Vec::new() } }

    pub fn add(&mut self, m: Manual) -> Result<(), TdCode> {
        if !m.sample_fictional { return Err(TdCode::SampleNotFictional); }
        for e in self.manuals.iter() {
            if e.kind == m.kind { return Err(TdCode::ManualMissing); }
        }
        self.manuals.push(m);
        Ok(())
    }

    pub fn missing(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        for k in ManualKind::ALL.iter() {
            if !self.manuals.iter().any(|m| m.kind == *k) { v.push(k.name()); }
        }
        v
    }

    /// 一致性抽查：文档键名样例与代码键名对拍。
    pub fn spot_check(&mut self, doc_keys: &[&str], code_keys: &[&str]) -> bool {
        let mut all_ok = true;
        for dk in doc_keys.iter() {
            if !code_keys.contains(dk) {
                all_ok = false;
                self.audit_log.push(String::from("文档漂移留痕: 以代码为准修正"));
            }
        }
        all_ok
    }

    /// 状态机抽查：文档描述的迁移与代码一致。
    pub fn spot_check_states(&mut self, doc_moves: &[(u8, u8)], code_moves: &[(u8, u8)]) -> bool {
        let mut ok = doc_moves.len() == code_moves.len();
        for d in doc_moves.iter() {
            if !code_moves.contains(d) { ok = false; }
        }
        if !ok {
            self.audit_log.push(String::from("状态机漂移留痕: 以代码为准修正"));
        }
        ok
    }

    /// 无障碍专节齐备：四类手册各含。
    pub fn a11y_complete(&self) -> Result<(), TdCode> {
        for k in ManualKind::ALL.iter() {
            match self.manuals.iter().find(|m| m.kind == *k) {
                None => return Err(TdCode::ManualMissing),
                Some(m) if !m.a11y_section => return Err(TdCode::A11ySectionMissing),
                _ => {}
            }
        }
        Ok(())
    }
}

pub fn run_ves317_checks() -> CheckSet {
    let mut set = CheckSet::new("ves317_s16docs");
    let mut db = DocBook::new();

    // 1) 示例非虚构拒。
    set.add("C4077-01", db.add(Manual { kind: ManualKind::Pipeline, title: String::from("流程"), a11y_section: true, sample_fictional: false }) == Err(TdCode::SampleNotFictional), "非虚构拒");

    // 2) 四类手册入库。
    for k in ManualKind::ALL.iter() {
        let _ = db.add(Manual { kind: *k, title: String::from(k.name()), a11y_section: true, sample_fictional: true });
    }
    set.add("C4077-02", db.missing().is_empty() && db.manuals.len() == 4, "四类手册齐");

    // 3) 手册缺类检出。
    let mut db2 = DocBook::new();
    let _ = db2.add(Manual { kind: ManualKind::KeySpec, title: String::from("键"), a11y_section: true, sample_fictional: true });
    set.add("C4077-03", db2.missing().len() == 3, "缺类检出");

    // 4) 一致性抽查：键名。
    let code = ["btn.ok", "btn.cancel", "menu.file"];
    set.add("C4077-04", db.spot_check(&["btn.ok", "menu.file"], &code), "键名对拍");

    // 5) 漂移留痕。
    let doc_bad = ["btn.ok", "btn.old_key"];
    set.add("C4077-05", !db.spot_check(&doc_bad, &code) && db.audit_log.len() == 1, "漂移留痕");

    // 6) 状态机抽查。
    let moves = [(0u8, 1u8), (1, 2), (2, 3)];
    set.add("C4077-06", db.spot_check_states(&moves, &moves), "状态机一致");
    let bad_moves = [(0, 1), (1, 5)];
    set.add("C4077-07", !db.spot_check_states(&bad_moves, &moves) && db.audit_log.len() == 2, "状态机漂移留痕");

    // 7) 无障碍专节齐备。
    set.add("C4077-08", db.a11y_complete().is_ok(), "无障碍专节");

    // 8) 专节缺检出。
    let mut db3 = DocBook::new();
    let _ = db3.add(Manual { kind: ManualKind::Pipeline, title: String::from("流程"), a11y_section: false, sample_fictional: true });
    set.add("C4077-09", db3.a11y_complete() == Err(TdCode::A11ySectionMissing), "专节缺检出");

    // 9) 手册名双射。
    let mut names: Vec<&str> = Vec::new();
    for k in ManualKind::ALL.iter() { names.push(k.name()); }
    let mut ndup = false;
    for i in 0..names.len() {
        for j in (i + 1)..names.len() {
            if names[i] == names[j] { ndup = true; }
        }
    }
    set.add("C4077-10", !ndup, "手册名双射");

    // 10) 段位自查。
    set.add("C4077-11", SEG_BASE == 0x8FC0, "段位 0x8FC0");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn docs_flow() {
        let mut db = DocBook::new();
        for k in ManualKind::ALL.iter() {
            let _ = db.add(Manual { kind: *k, title: String::new(), a11y_section: true, sample_fictional: true });
        }
        assert!(db.a11y_complete().is_ok());
    }
}
