//! UNX-F10357 wave-06 入场确认书（就绪双签）（0x1b720 段）
//!
//! 锚点：UNX-F10357-J1 入场确认书——五项就绪（脚本终版/判据移交/预案
//! 演练/时序落账/角色齐备）逐项核签，任一项核否即不得入场，核齐后须
//! 两面各签一笔（同面双签与三签均拒），五项全真双签在册方出确认书。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：就绪核账（五项各一判定）+ 复核核（一项一核不重核）+ 双签核
//! （面号在册且两面互异）+ 确认书核（五真双签出书）+ 注入核（未核齐
//! 先签/同面双签/核否强签/越界签四路必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b720 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b720;
pub const EC_ITEM: u32 = SEG_BASE + 0x01; // 就绪项号越界
pub const EC_RECHECK: u32 = SEG_BASE + 0x02; // 就绪项重复核
pub const EC_FACE: u32 = SEG_BASE + 0x03; // 签发面越界
pub const EC_NOTREADY: u32 = SEG_BASE + 0x04; // 五项未核齐
pub const EC_DUPSIGN: u32 = SEG_BASE + 0x05; // 同面双签
pub const EC_ONCE: u32 = SEG_BASE + 0x06; // 超额签发
pub const EC_SEALED: u32 = SEG_BASE + 0x07; // 封书后再动
pub const EC_FLAG: u32 = SEG_BASE + 0x08; // 就绪项核否

/// 五项就绪。
pub const R_SCRIPT: u8 = 1;
pub const R_CRITERIA: u8 = 2;
pub const R_CONTINGENCY: u8 = 3;
pub const R_SCHEDULE: u8 = 4;
pub const R_ROLES: u8 = 5;
pub const R_N: u8 = 5;

/// 签发面三面。
pub const FACE_N: u8 = 3;

/// 就绪项数锚（五项）。
pub const ITEMS_NEEDED: usize = 5;
/// 签发数锚（双签）。
pub const SIGNS_NEEDED: usize = 2;

/// 入场确认书。
#[derive(Clone)]
pub struct EntryConfirm {
    pub ready: [Option<bool>; R_N as usize],
    pub sign_a: Option<u8>,
    pub sign_b: Option<u8>,
    pub sealed: bool,
}

impl EntryConfirm {
    /// 空书。
    pub fn new() -> Self {
        EntryConfirm {
            ready: [None; R_N as usize],
            sign_a: None,
            sign_b: None,
            sealed: false,
        }
    }

    /// 核就绪项：一项一核。
    pub fn set_ready(&mut self, item: u8, ok: bool) -> Result<(), u32> {
        if self.sealed {
            return Err(EC_SEALED);
        }
        if item < R_SCRIPT || item > R_N {
            return Err(EC_ITEM);
        }
        if self.ready[(item - 1) as usize].is_some() {
            return Err(EC_RECHECK);
        }
        self.ready[(item - 1) as usize] = Some(ok);
        Ok(())
    }

    /// 五项是否全核。
    pub fn all_checked(&self) -> bool {
        let mut i = 0;
        while i < R_N as usize {
            if self.ready[i].is_none() {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 五项是否全真。
    pub fn all_ok(&self) -> bool {
        let mut i = 0;
        while i < R_N as usize {
            match self.ready[i] {
                Some(true) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 签发：核齐且全真方可，两面互异，以两笔为限。
    pub fn sign(&mut self, face: u8) -> Result<(), u32> {
        if self.sealed {
            return Err(EC_SEALED);
        }
        if face >= FACE_N {
            return Err(EC_FACE);
        }
        if !self.all_checked() {
            return Err(EC_NOTREADY);
        }
        if !self.all_ok() {
            return Err(EC_FLAG);
        }
        if self.sign_a.is_some() && self.sign_b.is_some() {
            return Err(EC_ONCE);
        }
        if self.sign_a == Some(face) {
            return Err(EC_DUPSIGN);
        }
        if self.sign_a.is_none() {
            self.sign_a = Some(face);
        } else {
            self.sign_b = Some(face);
        }
        Ok(())
    }

    /// 签发数。
    pub fn signed_count(&self) -> usize {
        let mut n = 0;
        if self.sign_a.is_some() {
            n += 1;
        }
        if self.sign_b.is_some() {
            n += 1;
        }
        n
    }

    /// 核否项清单。
    pub fn failed_items(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < R_N as usize {
            if self.ready[i] == Some(false) {
                out.push(i as u8 + 1);
            }
            i += 1;
        }
        out
    }

    /// 入场成立：五真双签。
    pub fn entry_ok(&self) -> bool {
        self.all_ok() && self.signed_count() == SIGNS_NEEDED
    }

    /// 确认书正文（未成立即空）。
    pub fn letter(&self) -> Option<String> {
        if !self.entry_ok() {
            return None;
        }
        Some(format!(
            "wave-06 入场确认书：五项就绪全真，签发面 {} 与 {} 双签在册",
            self.sign_a.unwrap_or(9),
            self.sign_b.unwrap_or(9)
        ))
    }

    /// 封书：未成立即拒。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(EC_SEALED);
        }
        if !self.entry_ok() {
            return Err(EC_NOTREADY);
        }
        self.sealed = true;
        Ok(())
    }

    /// 折叠指纹：五判定+双签逐位叠算。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b7201b7;
        let mut i = 0;
        while i < R_N as usize {
            let v = match self.ready[i] {
                Some(true) => 1u32,
                Some(false) => 2,
                None => 0,
            };
            h = (h ^ v).wrapping_mul(0x01000193);
            i += 1;
        }
        h = (h ^ self.sign_a.unwrap_or(9) as u32).wrapping_mul(0x01000193);
        h = (h ^ self.sign_b.unwrap_or(9) as u32).wrapping_mul(0x01000193);
        h
    }
}

/// 就绪项样例：五项全真。
pub fn ready_verdicts() -> [(u8, bool); 5] {
    [
        (R_SCRIPT, true),
        (R_CRITERIA, true),
        (R_CONTINGENCY, true),
        (R_SCHEDULE, true),
        (R_ROLES, true),
    ]
}

/// 注入核：四路坏签必抓。
/// 0: 未核齐先签；1: 同面双签；2: 核否强签；3: 越界签。
pub fn inject_four() -> [bool; 4] {
    let mut c = EntryConfirm::new();
    let mut k = 0;
    while k < ready_verdicts().len() {
        let (item, ok) = ready_verdicts()[k];
        let _ = c.set_ready(item, ok);
        k += 1;
    }
    let d0 = {
        let mut half = EntryConfirm::new();
        let _ = half.set_ready(R_SCRIPT, true);
        half.sign(0).err() == Some(EC_NOTREADY)
    };
    let d1 = {
        let mut twin = c.clone();
        let _ = twin.sign(1);
        twin.sign(1).err() == Some(EC_DUPSIGN)
    };
    let d2 = {
        let mut bad = EntryConfirm::new();
        let _ = bad.set_ready(R_SCRIPT, true);
        let _ = bad.set_ready(R_CRITERIA, true);
        let _ = bad.set_ready(R_CONTINGENCY, true);
        let _ = bad.set_ready(R_SCHEDULE, true);
        let _ = bad.set_ready(R_ROLES, false);
        bad.sign(0).err() == Some(EC_FLAG)
    };
    let d3 = c.sign(9).err() == Some(EC_FACE);
    [d0, d1, d2, d3]
}

/// 判据：run_unx10357_checks。
pub fn run_unx10357_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10357");
    // J1 正路径：五项全核全真。
    let mut c = EntryConfirm::new();
    let mut j1 = true;
    let mut k = 0;
    while k < ready_verdicts().len() {
        let (item, ok) = ready_verdicts()[k];
        if c.set_ready(item, ok).is_err() {
            j1 = false;
        }
        k += 1;
    }
    s.add("J1-五项核齐", j1 && c.all_checked() && c.all_ok(), "五就绪项全真");
    // J2 边界：就绪项号越界拒。
    let mut c2 = EntryConfirm::new();
    let j2 = c2.set_ready(0, true).err() == Some(EC_ITEM)
        && c2.set_ready(6, true).err() == Some(EC_ITEM);
    s.add("J2-越项拒", j2, "项号不在 1..=5 即 EC_ITEM");
    // J3 注入：重复核必抓。
    let mut c3 = EntryConfirm::new();
    let _ = c3.set_ready(R_SCRIPT, true);
    let j3 = c3.set_ready(R_SCRIPT, true).err() == Some(EC_RECHECK);
    s.add("J3-复核抓", j3, "一项两核即 EC_RECHECK");
    // J4 注入：未核齐先签必抓。
    let mut c4 = EntryConfirm::new();
    let _ = c4.set_ready(R_SCRIPT, true);
    let _ = c4.set_ready(R_CRITERIA, true);
    let j4 = c4.sign(0).err() == Some(EC_NOTREADY);
    s.add("J4-缺签抓", j4, "五项未核齐签发即 EC_NOTREADY");
    // J5 注入：核否强签必抓。
    let mut c5b = EntryConfirm::new();
    let _ = c5b.set_ready(R_SCRIPT, true);
    let _ = c5b.set_ready(R_CRITERIA, true);
    let _ = c5b.set_ready(R_CONTINGENCY, true);
    let _ = c5b.set_ready(R_SCHEDULE, true);
    let _ = c5b.set_ready(R_ROLES, false);
    let j5 = c5b.sign(2).err() == Some(EC_FLAG);
    s.add("J5-否签抓", j5, "有项核否签发即 EC_FLAG");
    // J6 注入：同面双签必抓。
    let mut c6 = c.clone();
    let _ = c6.sign(1);
    let j6 = c6.sign(1).err() == Some(EC_DUPSIGN);
    s.add("J6-同面抓", j6, "同面双签即 EC_DUPSIGN");
    // J7 注入：越界签必抓。
    let mut c7 = c.clone();
    let j7 = c7.sign(3).err() == Some(EC_FACE);
    s.add("J7-越面抓", j7, "面号越界即 EC_FACE");
    // J8 正路径：五真+双签成立出书。
    let mut c8 = c.clone();
    let _ = c8.sign(0);
    let _ = c8.sign(2);
    let letter = c8.letter();
    let j8 = c8.entry_ok() && letter.is_some();
    s.add("J8-确认书成", j8, "五真双签出确认书");
    // J9 正路径：成立后封书。
    let mut c9 = c8.clone();
    let j9 = c9.seal().is_ok() && c9.sealed;
    s.add("J9-封书成", j9, "确认书成立封书通过");
    // J10 边界：未成立封书拒。
    let mut c10 = c.clone();
    let _ = c10.sign(0);
    let j10 = c10.seal().err() == Some(EC_NOTREADY) && c10.signed_count() == 1;
    s.add("J10-缺封拒", j10, "单签封书即 EC_NOTREADY");
    // J11 注入：超额三签必抓。
    let mut c11 = c8.clone();
    let j11 = c11.sign(1).err() == Some(EC_ONCE);
    s.add("J11-三签抓", j11, "双签后三签即 EC_ONCE");
    // J12 注入：封书后再签拒。
    let mut c12 = c8.clone();
    let _ = c12.seal();
    let j12 = c12.sign(1).err() == Some(EC_SEALED);
    s.add("J12-封后签拒", j12, "封书后签发即 EC_SEALED");
    // J13 动态：核否清单与指纹。
    let fails = c5b.failed_items();
    let j13 = fails.len() == 1 && fails[0] == R_ROLES && c5b.fold() != 0;
    s.add_dyn(
        "J13-否项出数",
        j13,
        &format!("核否项 {:?}（角色项核否即拦），指纹 {:08x}", fails, c5b.fold()),
    );
    // J14 注入：inject_four 四路必抓。
    let inj = inject_four();
    let j14 = inj[0] && inj[1] && inj[2] && inj[3];
    s.add("J14-注入抓", j14, "缺签/同面/否签/越面四路全检出");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_and_seal() {
        let mut c = EntryConfirm::new();
        let mut k = 0;
        while k < ready_verdicts().len() {
            let (item, ok) = ready_verdicts()[k];
            assert!(c.set_ready(item, ok).is_ok());
            k += 1;
        }
        assert!(c.sign(0).is_ok());
        assert!(c.sign(0).err() == Some(EC_DUPSIGN));
        assert!(c.sign(2).is_ok());
        assert!(c.entry_ok());
        assert!(c.letter().is_some());
        assert!(c.seal().is_ok());
        assert!(c.sign(1).err() == Some(EC_SEALED));
        assert!(c.fold() != 0);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx10357_checks();
        assert!(s.all_passed());
    }
}
