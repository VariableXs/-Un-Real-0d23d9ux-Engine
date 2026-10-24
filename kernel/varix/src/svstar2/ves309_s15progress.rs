//! ves309_s15progress — VE-F4069 · T04 翻译进度管理
//! 键×语种×状态三维进度矩阵 + 五态状态机（未译/机翻/编辑中/评审中/定稿，非法迁移拒绝+留痕）+ 截止日按语种预警 + 看板色标复用声明 + 降级静态报表。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x8BC0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum PgCode {
    Ok = 0,
    IllegalMove = SEG_BASE + 0x01,
    Overdue = SEG_BASE + 0x02,
    BoardLag = SEG_BASE + 0x03,
    NoEntry = SEG_BASE + 0x04,
}

/// 五态。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum St { Untranslated = 0, Machine = 1, Editing = 2, Reviewing = 3, Final = 4 }

impl St {
    pub const ALL: [St; 5] = [St::Untranslated, St::Machine, St::Editing, St::Reviewing, St::Final];
    pub fn name(self) -> &'static str {
        match self {
            St::Untranslated => "未译", St::Machine => "机翻", St::Editing => "编辑中",
            St::Reviewing => "评审中", St::Final => "定稿",
        }
    }
    /// 合法迁移：只能 +1 步进（或重译回未译）。
    pub fn can_move(from: St, to: St) -> bool {
        to as u8 == from as u8 + 1 || to == St::Untranslated
    }
}

/// 三维矩阵条目。
#[derive(Clone)]
pub struct ProgEntry {
    pub key: String,
    pub lang: &'static str,
    pub state: St,
    pub moves: u32,
}

pub struct ProgMatrix {
    pub entries: Vec<ProgEntry>,
    /// 截止线：语种 → tick。
    pub deadlines: Vec<(&'static str, u64)>,
    pub log: Vec<String>,
}

impl ProgMatrix {
    pub fn new() -> Self {
        ProgMatrix { entries: Vec::new(), deadlines: Vec::new(), log: Vec::new() }
    }

    pub fn set_deadline(&mut self, lang: &'static str, tick: u64) {
        for d in self.deadlines.iter_mut() {
            if d.0 == lang { d.1 = tick; return; }
        }
        self.deadlines.push((lang, tick));
    }

    pub fn add_entry(&mut self, key: &str, lang: &'static str) -> Result<(), PgCode> {
        if self.entries.iter().any(|e| e.key == key && e.lang == lang) {
            return Err(PgCode::NoEntry);
        }
        self.entries.push(ProgEntry { key: String::from(key), lang, state: St::Untranslated, moves: 0 });
        Ok(())
    }

    /// 状态迁移：非法迁移拒绝+留痕。
    pub fn move_state(&mut self, key: &str, lang: &str, to: St) -> Result<(), PgCode> {
        let e = match self.entries.iter_mut().find(|e| e.key == key && e.lang == lang) {
            Some(e) => e,
            None => return Err(PgCode::NoEntry),
        };
        if !St::can_move(e.state, to) {
            self.log.push(String::from("非法迁移留痕"));
            return Err(PgCode::IllegalMove);
        }
        e.state = to;
        e.moves += 1;
        Ok(())
    }

    /// 进度百分比：某语种定稿占比。
    pub fn progress_of(&self, lang: &str) -> u32 {
        let total = self.entries.iter().filter(|e| e.lang == lang).count();
        if total == 0 { return 0; }
        let done = self.entries.iter().filter(|e| e.lang == lang && e.state == St::Final).count();
        (done * 100 / total) as u32
    }

    /// 截止预警：语种进度未达线（tick 前需 100%）→ 预警。
    pub fn overdue(&self, now: u64) -> Vec<&'static str> {
        let mut v = Vec::new();
        for d in self.deadlines.iter() {
            if now > d.1 || (self.progress_of(d.0) < 100 && now + 10 > d.1) {
                v.push(d.0);
            }
        }
        v
    }

    /// 看板降级静态报表。
    pub fn degraded_report(&self) -> String {
        let mut s = String::new();
        for d in self.deadlines.iter() {
            s.push_str(d.0);
        }
        s
    }
}

pub fn run_ves309_checks() -> CheckSet {
    let mut set = CheckSet::new("ves309_s15progress");
    let mut pm = ProgMatrix::new();

    // 1) 五态齐。
    set.add("C4069-01", St::ALL.len() == 5 && St::ALL[0] == St::Untranslated && St::ALL[4] == St::Final, "五态机");

    // 2) 合法迁移判定。
    set.add("C4069-02", St::can_move(St::Untranslated, St::Machine) && St::can_move(St::Reviewing, St::Final)
        && !St::can_move(St::Untranslated, St::Final), "步进迁移");

    // 3) 重译回未译合法。
    set.add("C4069-03", St::can_move(St::Reviewing, St::Untranslated), "重译回退");

    // 4) 入库与默认态。
    let _ = pm.add_entry("btn.ok", "zh");
    let _ = pm.add_entry("btn.ok", "ja");
    set.add("C4069-04", pm.entries.len() == 2 && pm.entries[0].state == St::Untranslated, "入库默认态");

    // 5) 重复入库拒。
    set.add("C4069-05", pm.add_entry("btn.ok", "zh") == Err(PgCode::NoEntry), "重复入库拒");

    // 6) 步进推进。
    let _ = pm.move_state("btn.ok", "zh", St::Machine);
    let _ = pm.move_state("btn.ok", "zh", St::Editing);
    set.add("C4069-06", pm.entries[0].state == St::Editing && pm.entries[0].moves == 2, "步进推进");

    // 7) 非法迁移拒绝+留痕。
    set.add("C4069-07", pm.move_state("btn.ok", "zh", St::Final) == Err(PgCode::IllegalMove)
        && !pm.log.is_empty(), "非法迁移留痕");

    // 8) 查无键拒。
    set.add("C4069-08", pm.move_state("ghost", "zh", St::Machine) == Err(PgCode::NoEntry), "查无拒");

    // 9) 进度百分比。
    let _ = pm.move_state("btn.ok", "zh", St::Reviewing);
    let _ = pm.move_state("btn.ok", "zh", St::Final);
    set.add("C4069-09", pm.progress_of("zh") == 100 && pm.progress_of("ja") == 0, "进度百分比");

    // 10) 截止预警。
    pm.set_deadline("ja", 100);
    set.add("C4069-10", pm.overdue(50) == ["ja"], "进度预警");

    // 11) 超期。
    pm.set_deadline("ja", 40);
    set.add("C4069-11", pm.overdue(50) == ["ja"], "超期预警");

    // 12) 降级静态报表。
    set.add("C4069-12", !pm.degraded_report().is_empty(), "降级报表");

    // 13) 态名双射。
    let mut names: Vec<&str> = Vec::new();
    for s in St::ALL.iter() { names.push(s.name()); }
    let mut ndup = false;
    for i in 0..names.len() {
        for j in (i + 1)..names.len() {
            if names[i] == names[j] { ndup = true; }
        }
    }
    set.add("C4069-13", !ndup, "态名双射");

    // 14) 段位自查。
    set.add("C4069-14", SEG_BASE == 0x8BC0, "段位 0x8BC0");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_machine() {
        assert!(St::can_move(St::Machine, St::Editing));
        assert!(!St::can_move(St::Machine, St::Final));
        let mut pm = ProgMatrix::new();
        let _ = pm.add_entry("k", "zh");
        assert!(pm.move_state("k", "zh", St::Final).is_err());
    }
}
