//! UNX-F10356 彩排问题清账（发现项闭环）（0x1b700 段）
//!
//! 锚点：UNX-F10356-J1 彩排问题清账——联排发现项逐条入册（号+发现场+
//! 描述），逐项走 修复→复测→销账 三跃迁闭环，跃迁不得跳步不得回头，
//! 修复必附处置注记，发现项全销账且册数达锚方算清账闭环。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：发现册（号+场+状态+双注记）+ 跃迁核（OPEN→FIXED→RETESTED→
//! CLOSED 单向三步）+ 注记核（修复必注）+ 闭环核（全销账达锚）+
//! 注入核（跳步销账/重复修复/未在册/重号四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b700 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b700;
pub const RC_ZERO: u32 = SEG_BASE + 0x01; // 空描述
pub const RC_STAGE: u32 = SEG_BASE + 0x02; // 发现场越界
pub const RC_CAP: u32 = SEG_BASE + 0x03; // 册超容
pub const RC_DUP: u32 = SEG_BASE + 0x04; // 发现项重号
pub const RC_NOFIND: u32 = SEG_BASE + 0x05; // 发现项未在册
pub const RC_STATE: u32 = SEG_BASE + 0x06; // 状态跃迁非法
pub const RC_NONOTE: u32 = SEG_BASE + 0x07; // 修复无注记
pub const RC_NOTREADY: u32 = SEG_BASE + 0x08; // 未闭环即封

/// 发现项状态（单向四态）。
pub const F_OPEN: u8 = 1;
pub const F_FIXED: u8 = 2;
pub const F_RETESTED: u8 = 3;
pub const F_CLOSED: u8 = 4;

/// 发现项数锚。
pub const FINDINGS_NEEDED: usize = 4;
/// 册定容。
pub const CAP: usize = 10;

/// 发现项：号 + 发现场 + 描述 + 状态 + 修复注记。
#[derive(Clone)]
pub struct Finding {
    pub fid: u32,
    pub found_at: u8,
    pub note: String,
    pub state: u8,
    pub fix_note: String,
}

/// 彩排清账册。
#[derive(Clone)]
pub struct RehearsalClear {
    pub items: Vec<Finding>,
    pub sealed: bool,
}

impl RehearsalClear {
    /// 空册。
    pub fn new() -> Self {
        RehearsalClear {
            items: Vec::new(),
            sealed: false,
        }
    }

    /// 找项。
    pub fn find(&self, fid: u32) -> Option<usize> {
        let mut i = 0;
        while i < self.items.len() {
            if self.items[i].fid == fid {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 录入发现项（OPEN 起）。
    pub fn record(&mut self, fid: u32, found_at: u8, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(RC_NOTREADY);
        }
        if note.is_empty() {
            return Err(RC_ZERO);
        }
        if found_at < STG_MIN || found_at > STG_MAX {
            return Err(RC_STAGE);
        }
        if self.items.len() >= CAP {
            return Err(RC_CAP);
        }
        if self.find(fid).is_some() {
            return Err(RC_DUP);
        }
        self.items.push(Finding {
            fid,
            found_at,
            note: note.to_string(),
            state: F_OPEN,
            fix_note: String::new(),
        });
        Ok(())
    }

    /// 修复：OPEN→FIXED，须附处置注记。
    pub fn fix(&mut self, fid: u32, fix_note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(RC_NOTREADY);
        }
        let idx = match self.find(fid) {
            Some(i) => i,
            None => return Err(RC_NOFIND),
        };
        if self.items[idx].state != F_OPEN {
            return Err(RC_STATE);
        }
        if fix_note.is_empty() {
            return Err(RC_NONOTE);
        }
        self.items[idx].state = F_FIXED;
        self.items[idx].fix_note = fix_note.to_string();
        Ok(())
    }

    /// 复测：FIXED→RETESTED。
    pub fn retest(&mut self, fid: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(RC_NOTREADY);
        }
        let idx = match self.find(fid) {
            Some(i) => i,
            None => return Err(RC_NOFIND),
        };
        if self.items[idx].state != F_FIXED {
            return Err(RC_STATE);
        }
        self.items[idx].state = F_RETESTED;
        Ok(())
    }

    /// 销账：RETESTED→CLOSED。
    pub fn close(&mut self, fid: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(RC_NOTREADY);
        }
        let idx = match self.find(fid) {
            Some(i) => i,
            None => return Err(RC_NOFIND),
        };
        if self.items[idx].state != F_RETESTED {
            return Err(RC_STATE);
        }
        self.items[idx].state = F_CLOSED;
        Ok(())
    }

    /// 未销账数。
    pub fn open_count(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.items.len() {
            if self.items[i].state != F_CLOSED {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 已销账数。
    pub fn closed_count(&self) -> usize {
        self.items.len() - self.open_count()
    }

    /// 状态小计。
    pub fn state_counts(&self) -> [usize; 4] {
        let mut c = [0usize; 4];
        let mut i = 0;
        while i < self.items.len() {
            let st = self.items[i].state;
            if st >= F_OPEN && st <= F_CLOSED {
                c[(st - 1) as usize] += 1;
            }
            i += 1;
        }
        c
    }

    /// 清账闭环：册数达锚且全部销账。
    pub fn clear_ok(&self) -> bool {
        self.items.len() >= FINDINGS_NEEDED && self.open_count() == 0
    }

    /// 封账：未闭环即拒。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(RC_NOTREADY);
        }
        if !self.clear_ok() {
            return Err(RC_NOTREADY);
        }
        self.sealed = true;
        Ok(())
    }

    /// 折叠指纹：号/场/状态/注记逐项叠算。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b7001b7;
        let mut i = 0;
        while i < self.items.len() {
            let f = &self.items[i];
            h = (h ^ f.fid).wrapping_mul(0x01000193);
            h = (h ^ (f.found_at as u32)).wrapping_mul(0x01000193);
            h = (h ^ (f.state as u32)).wrapping_mul(0x01000193);
            let bytes = f.fix_note.as_bytes();
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

/// 发现场边界（与联排六场对齐）。
pub const STG_MIN: u8 = 1;
pub const STG_MAX: u8 = 6;

/// 发现项样例。
pub fn finding_samples() -> [(u32, u8, &'static str, &'static str); 4] {
    [
        (9001, 2, "干跑缺超时保护", "补超时保护并回归"),
        (9002, 3, "正演走位漏对账", "走位表补对账步"),
        (9003, 4, "失绿预演未出告警", "告警钩子已挂"),
        (9004, 5, "清账预演漏复测位", "复测位已补"),
    ]
}

/// 注入核：四路坏账必抓。
/// 0: 跳步销账（OPEN 直销）；1: 重复修复；2: 未在册项操作；3: 重号录入。
pub fn inject_four() -> [bool; 4] {
    let mut b = RehearsalClear::new();
    let mut k = 0;
    while k < finding_samples().len() {
        let (fid, at, note, _) = finding_samples()[k];
        let _ = b.record(fid, at, note);
        k += 1;
    }
    let d0 = b.close(9001).err() == Some(RC_STATE);
    let d1 = {
        let _ = b.fix(9002, "修法一");
        let _ = b.fix(9002, "修法二");
        b.fix(9002, "修法三").err() == Some(RC_STATE)
    };
    let d2 = b.retest(9999).err() == Some(RC_NOFIND);
    let d3 = b.record(9001, 3, "重号").err() == Some(RC_DUP);
    [d0, d1, d2, d3]
}

/// 判据：run_unx10356_checks。
pub fn run_unx10356_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10356");
    // J1 正路径：四项录入全 OPEN。
    let mut b = RehearsalClear::new();
    let mut j1 = true;
    let mut k = 0;
    while k < finding_samples().len() {
        let (fid, at, note, _) = finding_samples()[k];
        if b.record(fid, at, note).is_err() {
            j1 = false;
        }
        k += 1;
    }
    s.add("J1-四项入册", j1 && b.state_counts()[0] == 4, "四发现项全 OPEN");
    // J2 边界：空描述拒。
    let mut b2 = RehearsalClear::new();
    let j2 = b2.record(9101, 2, "").err() == Some(RC_ZERO);
    s.add("J2-空述拒", j2, "空描述即 RC_ZERO");
    // J3 边界：发现场越界拒。
    let mut b3 = RehearsalClear::new();
    let j3 = b3.record(9102, 0, "越界").err() == Some(RC_STAGE)
        && b3.record(9103, 7, "越界").err() == Some(RC_STAGE);
    s.add("J3-越场拒", j3, "发现场不在 1..=6 即 RC_STAGE");
    // J4 注入：跳步销账必抓。
    let mut b4 = RehearsalClear::new();
    let _ = b4.record(9104, 2, "跳步样本");
    let j4 = b4.close(9104).err() == Some(RC_STATE);
    s.add("J4-跳步抓", j4, "OPEN 直销即 RC_STATE");
    // J5 注入：重复修复必抓。
    let mut b5 = RehearsalClear::new();
    let _ = b5.record(9105, 3, "重修样本");
    let _ = b5.fix(9105, "一修");
    let j5 = b5.fix(9105, "二修").err() == Some(RC_STATE);
    s.add("J5-重修抓", j5, "FIXED 再修即 RC_STATE");
    // J6 注入：未在册项操作必抓。
    let mut b6 = RehearsalClear::new();
    let j6 = b6.fix(4242, "空修").err() == Some(RC_NOFIND);
    s.add("J6-无项抓", j6, "未在册修复即 RC_NOFIND");
    // J7 边界：修复无注记拒。
    let mut b7 = RehearsalClear::new();
    let _ = b7.record(9107, 4, "无注样本");
    let j7 = b7.fix(9107, "").err() == Some(RC_NONOTE);
    s.add("J7-无注拒", j7, "修复空注记即 RC_NONOTE");
    // J8 注入：重号录入必抓。
    let mut b8 = RehearsalClear::new();
    let _ = b8.record(9108, 2, "原项");
    let j8 = b8.record(9108, 3, "重号").err() == Some(RC_DUP);
    s.add("J8-重号抓", j8, "发现项重号即 RC_DUP");
    // J9 正路径：四项全走三跃迁闭环。
    let mut b9 = b.clone();
    let mut ok9 = true;
    let mut k9 = 0;
    while k9 < finding_samples().len() {
        let (fid, _, _, fixnote) = finding_samples()[k9];
        if b9.fix(fid, fixnote).is_err()
            || b9.retest(fid).is_err()
            || b9.close(fid).is_err()
        {
            ok9 = false;
        }
        k9 += 1;
    }
    let j9 = ok9 && b9.clear_ok() && b9.closed_count() == 4;
    s.add("J9-闭环成", j9, "四项全走修→测→销");
    // J10 正路径：闭环后封账。
    let mut b10 = b9.clone();
    let j10 = b10.seal().is_ok() && b10.sealed;
    s.add("J10-封账成", j10, "清账闭环封账通过");
    // J11 边界：未闭环封账拒。
    let mut b11 = b.clone();
    let j11 = b11.seal().err() == Some(RC_NOTREADY) && b11.open_count() == 4;
    s.add("J11-缺封拒", j11, "全未销封账即 RC_NOTREADY");
    // J12 边界：封账后再修拒。
    let mut b12 = b9.clone();
    let _ = b12.seal();
    let j12 = b12.fix(9001, "封后修").err() == Some(RC_NOTREADY);
    s.add("J12-封后修拒", j12, "封账后修复即 RC_NOTREADY");
    // J13 动态：状态小计与指纹。
    let c = b9.state_counts();
    let j13 = c[3] == 4 && b9.fold() != 0;
    s.add_dyn(
        "J13-状态出数",
        j13,
        &format!(
            "OPEN {} FIXED {} RETESTED {} CLOSED {}，指纹 {:08x}",
            c[0],
            c[1],
            c[2],
            c[3],
            b9.fold()
        ),
    );
    // J14 注入：inject_four 四路必抓。
    let inj = inject_four();
    let j14 = inj[0] && inj[1] && inj[2] && inj[3];
    s.add("J14-注入抓", j14, "跳步/重修/无项/重号四路全检出");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_and_seal() {
        let mut b = RehearsalClear::new();
        let mut k = 0;
        while k < finding_samples().len() {
            let (fid, at, note, _) = finding_samples()[k];
            assert!(b.record(fid, at, note).is_ok());
            k += 1;
        }
        assert!(b.close(9001).err() == Some(RC_STATE));
        let mut k2 = 0;
        while k2 < finding_samples().len() {
            let (fid, _, _, fixnote) = finding_samples()[k2];
            assert!(b.fix(fid, fixnote).is_ok());
            assert!(b.retest(fid).is_ok());
            assert!(b.close(fid).is_ok());
            k2 += 1;
        }
        assert!(b.clear_ok());
        assert!(b.seal().is_ok());
        assert!(b.retest(9001).err() == Some(RC_NOTREADY));
        assert!(b.fold() != 0);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx10356_checks();
        assert!(s.all_passed());
    }
}
