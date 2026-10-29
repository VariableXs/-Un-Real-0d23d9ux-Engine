//! UNX-F10352 参演角色清单（各面职责核对）（0x1b680 段）
//!
//! 锚点：UNX-F10352-J1 参演角色清单——pty/进程/基建三面各面职责逐条
//! 落人（面号+职类+执事人三元组入册），执事人全域不得重挂两职，必配
//! 职责位（面,职类）共六席缺一即清单不齐，六席齐且各面职责数达下限，
//! 清单才算齐备可封。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：角色册（三元组行）+ 执事人唯一核（一人一职不得重挂）+
//! 必配席位核（六席逐席对账）+ 面覆盖核（每面职责数达下限）+
//! 注入核（重挂/重席/坏面号/空执事人四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b680 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b680;
pub const RL_ZERO: u32 = SEG_BASE + 0x01; // 执事人空名
pub const RL_FACE: u32 = SEG_BASE + 0x02; // 面号越界
pub const RL_DUTY: u32 = SEG_BASE + 0x03; // 职类越界
pub const RL_CAP: u32 = SEG_BASE + 0x04; // 册超容
pub const RL_DUP: u32 = SEG_BASE + 0x05; // (面,职) 重席
pub const RL_HOLDER: u32 = SEG_BASE + 0x06; // 执事人重挂
pub const RL_NOTREADY: u32 = SEG_BASE + 0x07; // 席位不齐即封
pub const RL_SEALED: u32 = SEG_BASE + 0x08; // 封册后再录

/// 三面（与联签域同一面轴）。
pub const FACE_PTY: u8 = 0;
pub const FACE_PROC: u8 = 1;
pub const FACE_INFRA: u8 = 2;
pub const FACE_N: u8 = 3;

/// 职类（开卷/联签/审计/通知/闭账五类）。
pub const DUTY_CHARTER: u8 = 1;
pub const DUTY_SIGN: u8 = 2;
pub const DUTY_AUDIT: u8 = 3;
pub const DUTY_NOTIFY: u8 = 4;
pub const DUTY_CLOSE: u8 = 5;
pub const DUTY_N: u8 = 6;

/// 每面职责数下限。
pub const FACE_DUTY_MIN: usize = 2;
/// 册定容。
pub const CAP: usize = 12;

/// 必配六席：(面, 职类) 逐一对账。
pub const REQUIRED_SEATS: [(u8, u8); 6] = [
    (FACE_PTY, DUTY_CHARTER),
    (FACE_PTY, DUTY_SIGN),
    (FACE_PROC, DUTY_SIGN),
    (FACE_PROC, DUTY_AUDIT),
    (FACE_INFRA, DUTY_AUDIT),
    (FACE_INFRA, DUTY_CLOSE),
];

/// 角色行：面号 + 职类 + 执事人。
#[derive(Clone)]
pub struct RoleRow {
    pub face: u8,
    pub duty: u8,
    pub holder: String,
}

/// 参演角色清单。
#[derive(Clone)]
pub struct RoleList {
    pub rows: Vec<RoleRow>,
    pub sealed: bool,
}

impl RoleList {
    /// 空册。
    pub fn new() -> Self {
        RoleList {
            rows: Vec::new(),
            sealed: false,
        }
    }

    /// 录一行（面,职,人）。全部前置核通过才入册。
    pub fn add(&mut self, face: u8, duty: u8, holder: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(RL_SEALED);
        }
        if holder.is_empty() {
            return Err(RL_ZERO);
        }
        if face >= FACE_N {
            return Err(RL_FACE);
        }
        if duty < DUTY_CHARTER || duty >= DUTY_N {
            return Err(RL_DUTY);
        }
        if self.rows.len() >= CAP {
            return Err(RL_CAP);
        }
        // (面,职) 不得重席。
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].face == face && self.rows[i].duty == duty {
                return Err(RL_DUP);
            }
            i += 1;
        }
        // 执事人全域不得重挂两职。
        let mut j = 0;
        while j < self.rows.len() {
            if self.rows[j].holder == holder {
                return Err(RL_HOLDER);
            }
            j += 1;
        }
        self.rows.push(RoleRow {
            face,
            duty,
            holder: holder.to_string(),
        });
        Ok(())
    }

    /// 某面已录职类数。
    pub fn duty_count(&self, face: u8) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].face == face {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 某面职责覆盖（达下限）。
    pub fn face_covered(&self, face: u8) -> bool {
        self.duty_count(face) >= FACE_DUTY_MIN
    }

    /// 三面全覆盖。
    pub fn all_faces_covered(&self) -> bool {
        let mut f = 0;
        while f < FACE_N {
            if !self.face_covered(f) {
                return false;
            }
            f += 1;
        }
        true
    }

    /// 某席是否已有人。
    pub fn seat_filled(&self, face: u8, duty: u8) -> bool {
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].face == face && self.rows[i].duty == duty {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 必配席缺位数。
    pub fn missing_seats(&self) -> usize {
        let mut miss = 0;
        let mut k = 0;
        while k < REQUIRED_SEATS.len() {
            let (f, d) = REQUIRED_SEATS[k];
            if !self.seat_filled(f, d) {
                miss += 1;
            }
            k += 1;
        }
        miss
    }

    /// 清单齐备：必配六席全齐 + 三面覆盖全达下限。
    pub fn roster_ok(&self) -> bool {
        self.missing_seats() == 0 && self.all_faces_covered()
    }

    /// 某席执事人。
    pub fn holder_of(&self, face: u8, duty: u8) -> Option<String> {
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].face == face && self.rows[i].duty == duty {
                return Some(self.rows[i].holder.clone());
            }
            i += 1;
        }
        None
    }

    /// 封册：清单不齐即拒。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(RL_SEALED);
        }
        if !self.roster_ok() {
            return Err(RL_NOTREADY);
        }
        self.sealed = true;
        Ok(())
    }

    /// 折叠指纹：面/职/人逐行叠算。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b6801b6;
        let mut i = 0;
        while i < self.rows.len() {
            let r = &self.rows[i];
            h = (h ^ (r.face as u32)).wrapping_mul(0x01000193);
            h = (h ^ (r.duty as u32)).wrapping_mul(0x01000193);
            let bytes = r.holder.as_bytes();
            let mut j = 0;
            while j < bytes.len() {
                h = (h ^ (bytes[j] as u32)).wrapping_mul(0x01000193);
                j += 1;
            }
            i += 1;
        }
        h
    }
}

/// 名册样例：必配六席的标准执事人。
pub fn roster_names() -> Vec<(&'static str, &'static str)> {
    vec![
        ("pty-charter", "甲"),
        ("pty-sign", "乙"),
        ("proc-sign", "丙"),
        ("proc-audit", "丁"),
        ("infra-audit", "戊"),
        ("infra-close", "己"),
    ]
}

/// 注入核：四路坏行必抓。
/// 0: (面,职) 重席；1: 面号越界；2: 执事人重挂；3: 空执事人。
pub fn inject_four() -> [bool; 4] {
    let mut list = RoleList::new();
    let _ = list.add(FACE_PTY, DUTY_CHARTER, "甲");
    let _ = list.add(FACE_PTY, DUTY_SIGN, "乙");
    let _ = list.add(FACE_PROC, DUTY_SIGN, "丙");
    let _ = list.add(FACE_PROC, DUTY_AUDIT, "丁");
    let _ = list.add(FACE_INFRA, DUTY_AUDIT, "戊");
    let _ = list.add(FACE_INFRA, DUTY_CLOSE, "己");
    let d0 = list.add(FACE_PTY, DUTY_CHARTER, "庚").err() == Some(RL_DUP);
    let d1 = list.add(9, DUTY_SIGN, "辛").err() == Some(RL_FACE);
    let d2 = list.add(FACE_PROC, DUTY_NOTIFY, "丙").err() == Some(RL_HOLDER);
    let d3 = list.add(FACE_INFRA, DUTY_NOTIFY, "").err() == Some(RL_ZERO);
    [d0, d1, d2, d3]
}

/// 判据：run_unx10352_checks。
pub fn run_unx10352_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10352");
    // J1 正路径：必配六席逐席录齐。
    let mut list = RoleList::new();
    let mut j1 = true;
    let mut k = 0;
    while k < roster_names().len() {
        let seat = REQUIRED_SEATS[k];
        let who = roster_names()[k].1;
        if list.add(seat.0, seat.1, who).is_err() {
            j1 = false;
        }
        k += 1;
    }
    s.add("J1-六席录齐", j1 && list.rows.len() == 6, "必配六席逐席入册");
    // J2 边界：空执事人拒。
    let mut l2 = RoleList::new();
    let j2 = l2.add(FACE_PTY, DUTY_CHARTER, "").err() == Some(RL_ZERO);
    s.add("J2-空人拒", j2, "空执事人即 RL_ZERO");
    // J3 边界：坏面号拒。
    let mut l3 = RoleList::new();
    let j3 = l3.add(3, DUTY_SIGN, "乙").err() == Some(RL_FACE);
    s.add("J3-坏面拒", j3, "面号>=3 即 RL_FACE");
    // J4 边界：坏职类拒。
    let mut l4 = RoleList::new();
    let j4 = l4.add(FACE_PTY, 0, "乙").err() == Some(RL_DUTY)
        && l4.add(FACE_PTY, 6, "乙").err() == Some(RL_DUTY);
    s.add("J4-坏职拒", j4, "职类越界即 RL_DUTY");
    // J5 注入：(面,职) 重席必抓。
    let mut l5 = RoleList::new();
    let _ = l5.add(FACE_PTY, DUTY_SIGN, "乙");
    let j5 = l5.add(FACE_PTY, DUTY_SIGN, "丙").err() == Some(RL_DUP);
    s.add("J5-重席抓", j5, "同席重录即 RL_DUP");
    // J6 注入：执事人重挂必抓。
    let mut l6 = RoleList::new();
    let _ = l6.add(FACE_PTY, DUTY_SIGN, "乙");
    let j6 = l6.add(FACE_PROC, DUTY_AUDIT, "乙").err() == Some(RL_HOLDER);
    s.add("J6-重挂抓", j6, "一人两职即 RL_HOLDER");
    // J7 边界：缺席时清单不齐。
    let mut l7 = RoleList::new();
    let _ = l7.add(FACE_PTY, DUTY_CHARTER, "甲");
    let j7 = !l7.roster_ok() && l7.missing_seats() == 5;
    s.add("J7-缺席核", j7, "一席在册五席缺");
    // J8 正路径：六席齐则清单齐备且三面覆盖。
    let mut l8 = RoleList::new();
    let mut k8 = 0;
    while k8 < REQUIRED_SEATS.len() {
        let seat = REQUIRED_SEATS[k8];
        let who = roster_names()[k8].1;
        let _ = l8.add(seat.0, seat.1, who);
        k8 += 1;
    }
    let j8 = l8.roster_ok() && l8.all_faces_covered();
    s.add("J8-清单齐", j8, "六席齐三面覆盖");
    // J9 正路径：清单齐方可封册。
    let mut l9 = l8.clone();
    let j9 = l9.seal().is_ok() && l9.sealed;
    s.add("J9-封册成", j9, "齐备后封册通过");
    // J10 边界：清单不齐封册即拒。
    let mut l10 = l7.clone();
    let j10 = l10.seal().err() == Some(RL_NOTREADY);
    s.add("J10-缺封拒", j10, "缺席封册即 RL_NOTREADY");
    // J11 注入：封册后再录必抓。
    let mut l11 = l8.clone();
    let _ = l11.seal();
    let j11 = l11
        .add(FACE_PTY, DUTY_NOTIFY, "庚")
        .err()
        == Some(RL_SEALED);
    s.add("J11-封后录抓", j11, "封册后录入即 RL_SEALED");
    // J12 动态：在册行数与执事人总数。
    let n_rows = list.rows.len();
    let mut holders = 0;
    let mut tag_acc = String::new();
    let mut i = 0;
    while i < list.rows.len() {
        if !list.rows[i].holder.is_empty() {
            holders += 1;
        }
        if i < roster_names().len() {
            tag_acc.push_str(roster_names()[i].0);
            tag_acc.push(' ');
        }
        i += 1;
    }
    let j12 = n_rows == holders;
    s.add_dyn(
        "J12-人职对平",
        j12,
        &format!(
            "在册 {} 行，执事人 {} 人，席位[{}]一一对应",
            n_rows, holders, tag_acc
        ),
    );
    // J13 动态：面覆盖与折叠指纹。
    let fold = list.fold();
    let cov = format!(
        "{}/{}",
        list.duty_count(FACE_PTY) + list.duty_count(FACE_PROC) + list.duty_count(FACE_INFRA),
        list.rows.len()
    );
    let j13 = fold != 0 && list.all_faces_covered();
    s.add_dyn(
        "J13-面覆盖",
        j13,
        &format!("三面职责 {} 条全录，指纹 {:08x}", cov, fold),
    );
    // J14 注入：inject_four 四路必抓。
    let inj = inject_four();
    let j14 = inj[0] && inj[1] && inj[2] && inj[3];
    s.add("J14-注入抓", j14, "重席/坏面/重挂/空人四路全检出");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_and_seal() {
        let mut list = RoleList::new();
        let mut k = 0;
        while k < REQUIRED_SEATS.len() {
            let seat = REQUIRED_SEATS[k];
            let who = roster_names()[k].1;
            assert!(list.add(seat.0, seat.1, who).is_ok());
            k += 1;
        }
        assert_eq!(list.rows.len(), 6);
        assert!(list.roster_ok());
        assert_eq!(list.holder_of(FACE_INFRA, DUTY_CLOSE), Some("己".to_string()));
        assert!(list.seal().is_ok());
        assert!(list.add(FACE_PTY, DUTY_NOTIFY, "庚").err() == Some(RL_SEALED));
        assert!(list.fold() != 0);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx10352_checks();
        assert!(s.all_passed());
    }
}
