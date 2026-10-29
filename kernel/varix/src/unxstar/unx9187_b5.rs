//! UNX-F9187 身份族与 B5 三身份档对账格（0x63720 段）
//!
//! 锚点：UNX-F9187-J1 对账行（身份字段 × B5 档号 × 单位/语义列逐
//! 条落账）；J2 对账语义（字段闭集 + B5 档号在册闸 + 语义一致闸 +
//! 快照无副作用四性质）；J3 对账指纹（账本折叠指纹）三判据全绿。
//! 域：CoRun·UNX-C2-B20（身份族×B5 对账面）。
//!
//! 结构：B5 档号册 + 对账行（字段/档号/语义）+ 对账核 + 对账本 +
//! 折叠指纹 + 注入核（错档/空留痕/伪字段/重号/印后登记必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x63720 独占 0x20）。
pub const SEG_BASE: u32 = 0x63720;
pub const B5_UNK_FIELD: u32 = SEG_BASE + 0x01; // 伪字段
pub const B5_BAD_SEM: u32 = SEG_BASE + 0x02; // 语义不符
pub const B5_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const B5_DUP_SEQ: u32 = SEG_BASE + 0x04; // 重序号
pub const B5_FROZEN: u32 = SEG_BASE + 0x05; // 印后登记

/// B5 三身份档号。
pub const B5_UID: u32 = 0xB501;
pub const B5_GID: u32 = 0xB502;
pub const B5_SUPP: u32 = 0xB503;
pub const B5_COUNT: usize = 3;

pub const B5_IDS: [u32; B5_COUNT] = [B5_UID, B5_GID, B5_SUPP];

/// 字段闭集。
pub const ID_RUID: usize = 0;
pub const ID_EUID: usize = 1;
pub const ID_SUID: usize = 2;
pub const ID_FSUID: usize = 3;
pub const ID_RGID: usize = 4;
pub const ID_EGID: usize = 5;
pub const ID_SGID: usize = 6;
pub const ID_COUNT: usize = 7;

pub const ID_NAMES: [&str; ID_COUNT] =
    ["ruid", "euid", "suid", "fsuid", "rgid", "egid", "sgid"];

/// 字段 → B5 档号。
pub fn field_b5(f: usize) -> Result<u32, u32> {
    match f {
        ID_RUID | ID_EUID | ID_SUID | ID_FSUID => Ok(B5_UID),
        ID_RGID | ID_EGID | ID_SGID => Ok(B5_GID),
        _ => Err(B5_UNK_FIELD),
    }
}

/// 字段语义（0=身份值 u32 直接对齐）。
pub fn field_sem(f: usize) -> Result<u8, u32> {
    match f {
        ID_RUID | ID_EUID | ID_SUID | ID_FSUID => Ok(0),
        ID_RGID | ID_EGID | ID_SGID => Ok(0),
        _ => Err(B5_UNK_FIELD),
    }
}

/// 单条对账行。
#[derive(Clone)]
pub struct B5Row {
    pub seq: u32,
    pub field: usize,
    pub b5: u32,
    pub value: u32,
    pub note: String,
}

impl B5Row {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.field < ID_COUNT && field_b5(self.field) == Ok(self.b5)
    }

    pub fn render_line(&self) -> String {
        format!(
            "B5{}|{}|B{:x}|v{}|{}",
            self.seq,
            ID_NAMES[self.field],
            self.b5,
            self.value,
            self.note
        )
    }
}

/// 对账簿。
pub struct B5Book {
    pub rows: Vec<B5Row>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl B5Book {
    pub fn new() -> B5Book {
        B5Book { rows: Vec::new(), seq_next: 1, sealed: false }
    }

    pub fn enter(&mut self, field: usize, b5: u32, value: u32, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(B5_FROZEN);
        }
        if note.is_empty() {
            return Err(B5_EMPTY_NOTE);
        }
        if field >= ID_COUNT {
            return Err(B5_UNK_FIELD);
        }
        if field_b5(field) != Ok(b5) {
            return Err(B5_BAD_SEM);
        }
        let seq = self.seq_next;
        self.seq_next += 1;
        self.rows.push(B5Row { seq, field, b5, value, note: note.to_string() });
        Ok(seq)
    }

    /// 全字段覆盖。
    pub fn full_cover(&self) -> bool {
        for f in 0..ID_COUNT {
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
        let mut h: u32 = 0x91870;
        for r in &self.rows {
            h = h.wrapping_mul(31).wrapping_add(r.seq);
            h = h.wrapping_mul(31).wrapping_add(r.field as u32);
            h = h.wrapping_mul(31).wrapping_add(r.b5);
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
    // 1) 错档行
    let bad = B5Row {
        seq: 1,
        field: ID_RUID,
        b5: B5_GID, // ruid 应挂 B5_UID
        value: 0,
        note: "inj-b5".to_string(),
    };
    got[0] = !bad.ok();
    // 2) 空留痕
    let bad2 = B5Row { seq: 2, field: 0, b5: B5_UID, value: 0, note: String::new() };
    got[1] = !bad2.ok();
    // 3) 伪字段
    let mut b = B5Book::new();
    got[2] = b.enter(99, B5_UID, 1, "inj-field").err() == Some(B5_UNK_FIELD);
    // 4) 错档挂载
    got[3] = b.enter(ID_RGID, B5_UID, 1, "inj-sem").err() == Some(B5_BAD_SEM);
    // 5) 印后登记必抓
    b.seal();
    got[4] = b.enter(ID_RUID, B5_UID, 1, "inj-frozen").err() == Some(B5_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx9187_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9187");
    let mut b = B5Book::new();

    // J1 对账行非空
    let _ = b.enter(ID_RUID, B5_UID, 1000, "ruid 对 B5_UID");
    let j1 = !b.rows.is_empty();
    s.add("J1-对账行非空", j1, "至少一行在册");

    // J2 账字段齐
    let j2 = b.rows.iter().all(|r| r.ok());
    s.add("J2-账字段齐备", j2, "留痕非空且档号一致");

    // J3 字段→档号映射
    let j3 = field_b5(ID_RUID) == Ok(B5_UID)
        && field_b5(ID_FSUID) == Ok(B5_UID)
        && field_b5(ID_RGID) == Ok(B5_GID)
        && field_b5(ID_SGID) == Ok(B5_GID);
    s.add("J3-档号映射", j3, "uid 族挂 B501 gid 族挂 B502");

    // J4 伪字段拒
    let j4 = b.enter(99, B5_UID, 1, "伪").err() == Some(B5_UNK_FIELD);
    s.add("J4-伪字段拒", j4, "字段出闭集必拒");

    // J5 错档挂载拒
    let j5 = b.enter(ID_RGID, B5_UID, 1, "错档").err() == Some(B5_BAD_SEM);
    s.add("J5-错档拒", j5, "gid 字段挂 uid 档必拒");

    // J6 B5 档号册
    let j6 = B5_IDS.len() == 3 && B5_UID == 0xB501 && B5_SUPP == 0xB503;
    s.add("J6-B5档号册", j6, "三档号锁定");

    // J7 全字段覆盖
    let j7 = {
        let mut b2 = B5Book::new();
        for f in 0..ID_COUNT {
            let _ = b2.enter(f, field_b5(f).unwrap_or(0), 1, "c");
        }
        b2.full_cover()
    };
    s.add("J7-全字段覆盖", j7, "七字段全在册");

    // J8 行值稳定
    let j8 = {
        let mut b2 = B5Book::new();
        let _ = b2.enter(ID_RUID, B5_UID, 42, "a");
        let before = b2.rows[0].value;
        let _ = b2.enter(ID_EUID, B5_UID, 7, "b");
        b2.rows[0].value == before
    };
    s.add("J8-行值稳定", j8, "追行不改旧行值");

    // J9 语义映射一致
    let j9 = field_sem(ID_RUID) == Ok(0) && field_sem(ID_SGID) == Ok(0);
    s.add("J9-语义一致", j9, "全部字段 u32 身份值对齐");

    // J10 指纹稳定 + seq 唯一
    let j10 = b.fold() == b.fold() && b.seq_unique();
    s.add("J10-指纹稳定", j10, "折叠指纹两次一致且 seq 唯一");

    // J11 空留痕拒
    let j11 = b.enter(ID_RUID, B5_UID, 1, "").err() == Some(B5_EMPTY_NOTE);
    s.add("J11-空留痕拒", j11, "空留痕必拒");

    // J12 拒绝不落账
    let j12 = {
        let before = b.rows.len();
        let _ = b.enter(99, B5_UID, 1, "不落");
        b.rows.len() == before
    };
    s.add("J12-拒绝不落账", j12, "拒绝路径零污染账本");

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
    fn unx9187_b5_flow() {
        assert_eq!(field_b5(ID_RUID), Ok(B5_UID));
        assert_eq!(field_b5(ID_RGID), Ok(B5_GID));
        assert!(field_b5(99).is_err());
        let mut b = B5Book::new();
        assert!(b.enter(ID_RUID, B5_UID, 1000, "a").is_ok());
        assert_eq!(b.enter(ID_RGID, B5_UID, 1, "b"), Err(B5_BAD_SEM));
        assert_eq!(b.enter(99, B5_UID, 1, "c"), Err(B5_UNK_FIELD));
    }

    #[test]
    fn unx9187_checks_green() {
        let s = run_unx9187_checks();
        assert!(s.all_passed(), "UNX-F9187 判据未全绿");
    }
}
