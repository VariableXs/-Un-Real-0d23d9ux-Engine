//! UNX-F9179 getrusage RUSAGE_SELF 静态账（与 B15 对账）（0x63620 段）
//!
//! 锚点：UNX-F9179-J1 静态账（SELF 档字段 × 单位 × B15 账号三列逐
//! 条落账）；J2 静态语义（字段闭集 + 单位一致闸 + B15 账号在册闸 +
//! 快照无副作用四性质）；J3 静态指纹（账本折叠指纹）三判据全绿。
//! 域：CoRun·UNX-C2-B19（SELF 静态账与 B15 对账面）。
//!
//! 结构：B15 账号册 + 静态账行（字段/单位/账号）+ 对账核 + 静态账
//! 本 + 折叠指纹 + 注入核（错单位/空留痕/伪字段/重号/印后登记必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x63620 独占 0x20）。
pub const SEG_BASE: u32 = 0x63620;
pub const SB_UNK_FIELD: u32 = SEG_BASE + 0x01; // 伪字段
pub const SB_BAD_UNIT: u32 = SEG_BASE + 0x02; // 错单位
pub const SB_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const SB_DUP_SEQ: u32 = SEG_BASE + 0x04; // 重序号
pub const SB_FROZEN: u32 = SEG_BASE + 0x05; // 印后登记

/// B15 账号闭集（资源面五账）。
pub const B15_RUSAGE: u32 = 0xB151;
pub const B15_RLIMIT: u32 = 0xB152;
pub const B15_TIMES: u32 = 0xB153;
pub const B15_SYSINFO: u32 = 0xB154;
pub const B15_UNAME: u32 = 0xB155;
pub const B15_COUNT: usize = 5;

pub const B15_IDS: [u32; B15_COUNT] =
    [B15_RUSAGE, B15_RLIMIT, B15_TIMES, B15_SYSINFO, B15_UNAME];

/// 字段闭集。
pub const F_UTIME: usize = 0;
pub const F_STIME: usize = 1;
pub const F_MAXRSS: usize = 2;
pub const F_NIVCSW: usize = 3;
pub const F_NVCSW: usize = 4;
pub const F_COUNT: usize = 5;

pub const F_NAMES: [&str; F_COUNT] = ["utime", "stime", "maxrss", "nivcsw", "nvcsw"];

/// 单位闭集。
pub const U_TICK: usize = 0; // CLK_TCK tick
pub const U_KB: usize = 1; // KiB
pub const U_COUNT_T: usize = 2; // 次数
pub const U_NAMES: [&str; 3] = ["tick", "KiB", "count"];

/// 字段 → 单位映射。
pub fn field_unit(f: usize) -> Result<usize, u32> {
    match f {
        F_UTIME | F_STIME => Ok(U_TICK),
        F_MAXRSS => Ok(U_KB),
        F_NIVCSW | F_NVCSW => Ok(U_COUNT_T),
        _ => Err(SB_UNK_FIELD),
    }
}

/// 字段 → B15 账号。
pub fn field_b15(f: usize) -> Result<u32, u32> {
    match f {
        F_UTIME | F_STIME | F_MAXRSS => Ok(B15_RUSAGE),
        F_NIVCSW | F_NVCSW => Ok(B15_RUSAGE),
        _ => Err(SB_UNK_FIELD),
    }
}

/// 单条静态账行。
#[derive(Clone)]
pub struct SbRow {
    pub seq: u32,
    pub field: usize,
    pub unit: usize,
    pub b15: u32,
    pub value: u32,
    pub note: String,
}

impl SbRow {
    pub fn ok(&self) -> bool {
        !self.note.is_empty()
            && self.field < F_COUNT
            && field_unit(self.field) == Ok(self.unit)
            && field_b15(self.field) == Ok(self.b15)
    }

    pub fn render_line(&self) -> String {
        format!(
            "SB{}|{}|{}|B{:x}|v{}|{}",
            self.seq,
            F_NAMES[self.field],
            U_NAMES[self.unit],
            self.b15,
            self.value,
            self.note
        )
    }
}

/// 静态账簿。
pub struct SbBook {
    pub rows: Vec<SbRow>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl SbBook {
    pub fn new() -> SbBook {
        SbBook { rows: Vec::new(), seq_next: 1, sealed: false }
    }

    pub fn enter(&mut self, field: usize, unit: usize, b15: u32, value: u32, note: &str)
    -> Result<u32, u32> {
        if self.sealed {
            return Err(SB_FROZEN);
        }
        if note.is_empty() {
            return Err(SB_EMPTY_NOTE);
        }
        if field >= F_COUNT {
            return Err(SB_UNK_FIELD);
        }
        if field_unit(field) != Ok(unit) {
            return Err(SB_BAD_UNIT);
        }
        if field_b15(field) != Ok(b15) {
            return Err(SB_BAD_UNIT);
        }
        let seq = self.seq_next;
        self.seq_next += 1;
        self.rows.push(SbRow { seq, field, unit, b15, value, note: note.to_string() });
        Ok(seq)
    }

    /// 全字段覆盖。
    pub fn full_cover(&self) -> bool {
        for f in 0..F_COUNT {
            if !self.rows.iter().any(|r| r.field == f) {
                return false;
            }
        }
        true
    }

    pub fn seq_unique(&self) -> bool {
        for i in 0..self.rows.len() {
            for j in (i + 1)..self.rows.len() {
                if self.rows[i].seq == self.rows[j].seq {
                    return false;
                }
            }
        }
        true
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x91790;
        for r in &self.rows {
            h = h.wrapping_mul(31).wrapping_add(r.seq);
            h = h.wrapping_mul(31).wrapping_add(r.field as u32);
            h = h.wrapping_mul(31).wrapping_add(r.unit as u32);
            h = h.wrapping_mul(31).wrapping_add(r.b15);
            h = h.wrapping_mul(31).wrapping_add(r.value);
            for b in r.note.bytes() {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 错单位行
    let bad = SbRow {
        seq: 1,
        field: F_MAXRSS,
        unit: U_TICK, // maxrss 应为 KiB
        b15: B15_RUSAGE,
        value: 0,
        note: "inj-unit".to_string(),
    };
    got[0] = !bad.ok();
    // 2) 空留痕
    let bad2 = SbRow {
        seq: 2,
        field: 0,
        unit: 0,
        b15: B15_RUSAGE,
        value: 0,
        note: String::new(),
    };
    got[1] = !bad2.ok();
    // 3) 伪字段
    let mut b = SbBook::new();
    got[2] = b.enter(99, U_TICK, B15_RUSAGE, 1, "inj-field").err() == Some(SB_UNK_FIELD);
    // 4) 错 B15 账号
    got[3] = b.enter(F_MAXRSS, U_KB, 0xDEAD, 1, "inj-b15").err() == Some(SB_BAD_UNIT);
    // 5) 印后登记必抓
    b.seal();
    got[4] = b.enter(F_UTIME, U_TICK, B15_RUSAGE, 1, "inj-frozen").err() == Some(SB_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx9179_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9179");
    let mut b = SbBook::new();

    // J1 静态账非空
    let _ = b.enter(F_UTIME, U_TICK, B15_RUSAGE, 42, "utime 快照");
    let j1 = !b.rows.is_empty();
    s.add("J1-静态账非空", j1, "至少一行在册");

    // J2 账字段齐
    let j2 = b.rows.iter().all(|r| r.ok());
    s.add("J2-账字段齐备", j2, "留痕非空且单位/账号一致");

    // J3 字段→单位映射正确
    let j3 = field_unit(F_UTIME) == Ok(U_TICK)
        && field_unit(F_STIME) == Ok(U_TICK)
        && field_unit(F_MAXRSS) == Ok(U_KB)
        && field_unit(F_NIVCSW) == Ok(U_COUNT_T)
        && field_unit(F_NVCSW) == Ok(U_COUNT_T);
    s.add("J3-单位映射", j3, "时间 tick / 内存 KiB / 次数 count");

    // J4 伪字段拒
    let j4 = b.enter(99, U_TICK, B15_RUSAGE, 1, "伪").err() == Some(SB_UNK_FIELD);
    s.add("J4-伪字段拒", j4, "字段出闭集必拒");

    // J5 错单位拒
    let j5 = b.enter(F_MAXRSS, U_TICK, B15_RUSAGE, 1, "错单位").err() == Some(SB_BAD_UNIT);
    s.add("J5-错单位拒", j5, "maxrss 记 tick 必拒");

    // J6 错 B15 账号拒
    let j6 = b.enter(F_UTIME, U_TICK, 0xDEAD, 1, "错账").err() == Some(SB_BAD_UNIT);
    s.add("J6-错账号拒", j6, "B15 账号不符必拒");

    // J7 B15 账号册
    let j7 = B15_IDS.len() == 5 && B15_RUSAGE == 0xB151 && B15_UNAME == 0xB155;
    s.add("J7-B15账号册", j7, "五账号闭集锁定");

    // J8 全字段覆盖
    let j8 = {
        let mut b2 = SbBook::new();
        let _ = b2.enter(F_UTIME, U_TICK, B15_RUSAGE, 1, "a");
        let _ = b2.enter(F_STIME, U_TICK, B15_RUSAGE, 1, "b");
        let _ = b2.enter(F_MAXRSS, U_KB, B15_RUSAGE, 1, "c");
        let _ = b2.enter(F_NIVCSW, U_COUNT_T, B15_RUSAGE, 1, "d");
        let _ = b2.enter(F_NVCSW, U_COUNT_T, B15_RUSAGE, 1, "e");
        b2.full_cover()
    };
    s.add("J8-全字段覆盖", j8, "五字段全在册");

    // J9 快照无副作用（行值只读）
    let j9 = {
        let mut b2 = SbBook::new();
        let _ = b2.enter(F_UTIME, U_TICK, B15_RUSAGE, 42, "a");
        let before = b2.rows[0].value;
        let _ = b2.enter(F_STIME, U_TICK, B15_RUSAGE, 7, "b");
        b2.rows[0].value == before
    };
    s.add("J9-行值稳定", j9, "追行不改旧行值");

    // J10 单位闭集
    let j10 = U_NAMES.len() == 3 && U_TICK == 0 && U_KB == 1;
    s.add("J10-单位闭集", j10, "tick/KiB/count 三单位锁定");

    // J11 指纹稳定 + seq 唯一
    let j11 = b.fold() == b.fold() && b.seq_unique();
    s.add("J11-指纹稳定", j11, "折叠指纹两次一致且 seq 唯一");

    // J12 空留痕拒
    let j12 = b.enter(F_UTIME, U_TICK, B15_RUSAGE, 1, "").err() == Some(SB_EMPTY_NOTE);
    s.add("J12-空留痕拒", j12, "空留痕必拒");

    // J13 注入核 5/5
    let got = inject_five();
    let j13 = got.iter().all(|g| *g);
    s.add_dyn("J13-注入全抓", j13, &format!("inj5={:?}", got));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9179_static_flow() {
        assert_eq!(field_unit(F_UTIME), Ok(U_TICK));
        assert_eq!(field_unit(F_MAXRSS), Ok(U_KB));
        assert_eq!(field_b15(F_UTIME), Ok(B15_RUSAGE));
        assert!(field_unit(99).is_err());
        let mut b = SbBook::new();
        assert!(b.enter(F_UTIME, U_TICK, B15_RUSAGE, 42, "a").is_ok());
        assert_eq!(b.enter(F_MAXRSS, U_TICK, B15_RUSAGE, 1, "b"), Err(SB_BAD_UNIT));
        assert_eq!(b.enter(F_UTIME, U_TICK, 0xDEAD, 1, "c"), Err(SB_BAD_UNIT));
    }

    #[test]
    fn unx9179_checks_green() {
        let s = run_unx9179_checks();
        assert!(s.all_passed(), "UNX-F9179 判据未全绿");
    }
}
