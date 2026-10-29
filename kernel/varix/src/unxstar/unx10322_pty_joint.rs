//! UNX-F10322 pty 号面联签（终端族判据互认）（0x1b2c0 段）
//!
//! 锚点：UNX-F10322-J1 终端族互认——termios/winsize/ctty 三亚族判据
//! 逐条由甲乙双方独立裁定，双裁且同向才算互认，三族全平才算号面联签。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：族判据册（行=族号+键+题名+双裁 Option<bool>）+ 裁定核（单裁
//! 不算互认、异裁入冲突清单）+ 族锚（termios 三条/winsize 两条/ctty
//! 两条）+ 注入核（坏族号/重键/未裁即判平/异裁四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b2c0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b2c0;
pub const PJ_FAM: u32 = SEG_BASE + 0x01; // 族号越界
pub const PJ_DUP: u32 = SEG_BASE + 0x02; // 键重号
pub const PJ_EMPTY: u32 = SEG_BASE + 0x03; // 题名空
pub const PJ_CAP: u32 = SEG_BASE + 0x04; // 册超容
pub const PJ_NOROW: u32 = SEG_BASE + 0x05; // 未知判据行
pub const PJ_REJUDGE: u32 = SEG_BASE + 0x06; // 已裁再裁
pub const PJ_SEALED: u32 = SEG_BASE + 0x07; // 联签后再动

/// 终端三亚族。
pub const FAM_TERMIOS: u8 = 1;
pub const FAM_WINSIZE: u8 = 2;
pub const FAM_CTTY: u8 = 3;
pub const FAM_N: u8 = 4;

/// 各族判据需求锚。
pub const FAM_NEED: [usize; 3] = [3, 2, 2];
/// 册定容。
pub const CAP: usize = 12;

/// 族判据行。
#[derive(Clone)]
pub struct PtyRow {
    pub fam: u8,
    pub key: String,
    pub name: String,
    pub a_ok: Option<bool>,
    pub b_ok: Option<bool>,
}

impl PtyRow {
    /// 双裁齐备。
    pub fn both(&self) -> bool {
        self.a_ok.is_some() && self.b_ok.is_some()
    }

    /// 互认：双裁且同向。
    pub fn agree(&self) -> bool {
        self.a_ok == self.b_ok && self.both()
    }
}

/// pty 号面联签册。
pub struct PtyJoint {
    pub rows: Vec<PtyRow>,
    pub sealed: bool,
}

impl PtyJoint {
    pub fn new() -> PtyJoint {
        PtyJoint { rows: Vec::new(), sealed: false }
    }

    fn pos(&self, fam: u8, key: &str) -> Option<usize> {
        let mut k = 0usize;
        while k < self.rows.len() {
            let r = &self.rows[k];
            if r.fam == fam && r.key == key {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    fn fam_rows(&self, fam: u8) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.rows.len() {
            if self.rows[k].fam == fam {
                n += 1;
            }
            k += 1;
        }
        n
    }

    /// 登记判据行。
    pub fn register(&mut self, fam: u8, key: &str, name: &str) -> Result<(), u32> {
        if fam == 0 || fam >= FAM_N {
            return Err(PJ_FAM);
        }
        if self.sealed {
            return Err(PJ_SEALED);
        }
        if self.pos(fam, key).is_some() {
            return Err(PJ_DUP);
        }
        if key.is_empty() || name.is_empty() {
            return Err(PJ_EMPTY);
        }
        if self.rows.len() >= CAP {
            return Err(PJ_CAP);
        }
        self.rows.push(PtyRow {
            fam,
            key: key.to_string(),
            name: name.to_string(),
            a_ok: None,
            b_ok: None,
        });
        Ok(())
    }

    /// 甲方裁定。
    pub fn a_judge(&mut self, fam: u8, key: &str, ok: bool) -> Result<(), u32> {
        if self.sealed {
            return Err(PJ_SEALED);
        }
        match self.pos(fam, key) {
            None => Err(PJ_NOROW),
            Some(p) => {
                if self.rows[p].a_ok.is_some() {
                    return Err(PJ_REJUDGE);
                }
                self.rows[p].a_ok = Some(ok);
                Ok(())
            }
        }
    }

    /// 乙方裁定。
    pub fn b_judge(&mut self, fam: u8, key: &str, ok: bool) -> Result<(), u32> {
        if self.sealed {
            return Err(PJ_SEALED);
        }
        match self.pos(fam, key) {
            None => Err(PJ_NOROW),
            Some(p) => {
                if self.rows[p].b_ok.is_some() {
                    return Err(PJ_REJUDGE);
                }
                self.rows[p].b_ok = Some(ok);
                Ok(())
            }
        }
    }

    /// 异裁清单（双裁但不同向）。
    pub fn conflicts(&self) -> Vec<(u8, String)> {
        let mut v: Vec<(u8, String)> = Vec::new();
        let mut k = 0usize;
        while k < self.rows.len() {
            let r = &self.rows[k];
            if r.both() && !r.agree() {
                v.push((r.fam, r.key.clone()));
            }
            k += 1;
        }
        v
    }

    /// 单裁未齐清单。
    pub fn pending(&self) -> Vec<(u8, String)> {
        let mut v: Vec<(u8, String)> = Vec::new();
        let mut k = 0usize;
        while k < self.rows.len() {
            if !self.rows[k].both() {
                v.push((self.rows[k].fam, self.rows[k].key.clone()));
            }
            k += 1;
        }
        v
    }

    /// 族覆盖：该族行数达锚且族内全互认。
    pub fn family_ok(&self, fam: u8) -> bool {
        if fam == 0 || fam >= FAM_N {
            return false;
        }
        let idx = (fam - 1) as usize;
        if self.fam_rows(fam) < FAM_NEED[idx] {
            return false;
        }
        let mut k = 0usize;
        while k < self.rows.len() {
            if self.rows[k].fam == fam && !self.rows[k].agree() {
                return false;
            }
            k += 1;
        }
        true
    }

    /// 号面联签过：三族全达锚全互认。
    pub fn mutual(&self) -> bool {
        let mut f = 1u8;
        while f < FAM_N {
            if !self.family_ok(f) {
                return false;
            }
            f += 1;
        }
        true
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 号面指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b2c00;
        h = h.wrapping_mul(31).wrapping_add(self.rows.len() as u32);
        let mut k = 0usize;
        while k < self.rows.len() {
            let r = &self.rows[k];
            h = h.wrapping_mul(31).wrapping_add(r.fam as u32);
            let kb = r.key.as_bytes();
            let mut i = 0usize;
            while i < kb.len() {
                h = h.wrapping_mul(31).wrapping_add(kb[i] as u32);
                i += 1;
            }
            h = h.wrapping_mul(31).wrapping_add(r.a_ok.unwrap_or(false) as u32 + 2);
            h = h.wrapping_mul(31).wrapping_add(r.b_ok.unwrap_or(false) as u32 + 2);
            k += 1;
        }
        h = h.wrapping_mul(31).wrapping_add(self.sealed as u32);
        h
    }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    let mut j = PtyJoint::new();
    // 1) 坏族号（0 与 4）必拒
    got[0] = j.register(0, "x", "y").err() == Some(PJ_FAM)
        && j.register(4, "x", "y").err() == Some(PJ_FAM);
    // 2) 同族重键必拒
    let _ = j.register(FAM_TERMIOS, "cfmakeraw", "原始终据");
    got[1] = j.register(FAM_TERMIOS, "cfmakeraw", "重键").err() == Some(PJ_DUP);
    // 3) 未裁即判互认必不成
    let _ = j.register(FAM_WINSIZE, "ws_col", "列宽判据");
    got[2] = !j.family_ok(FAM_WINSIZE);
    // 4) 异裁必入冲突清单
    let _ = j.register(FAM_CTTY, "ctty_set", "控制端判据");
    let _ = j.a_judge(FAM_CTTY, "ctty_set", true);
    let _ = j.b_judge(FAM_CTTY, "ctty_set", false);
    got[3] = j.conflicts().len() == 1 && !j.rows[2].agree();
    got
}

/// 终端族样例（族号, 键, 题名）。
pub fn pty_samples() -> Vec<(u8, &'static str, &'static str)> {
    let mut v: Vec<(u8, &'static str, &'static str)> = Vec::new();
    v.push((FAM_TERMIOS, "cfmakeraw", "原始终据"));
    v.push((FAM_TERMIOS, "iflag", "入规程判据"));
    v.push((FAM_TERMIOS, "oflag", "出规程判据"));
    v.push((FAM_WINSIZE, "ws_col", "列宽判据"));
    v.push((FAM_WINSIZE, "ws_row", "行高判据"));
    v.push((FAM_CTTY, "ctty_set", "控制端判据"));
    v.push((FAM_CTTY, "ctty_bg", "后台组判据"));
    v
}

/// 批内判据。
pub fn run_unx10322_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10322");

    // J1 正路径：三亚族登记双裁互认并封册
    let mut j = PtyJoint::new();
    let samples = pty_samples();
    let mut ok = true;
    let mut k = 0usize;
    while k < samples.len() {
        let (f, key, name) = (samples[k].0, samples[k].1, samples[k].2);
        if j.register(f, key, name).is_err() {
            ok = false;
        }
        k += 1;
    }
    k = 0;
    while k < samples.len() {
        let (f, key, _) = (samples[k].0, samples[k].1, samples[k].2);
        if j.a_judge(f, key, true).is_err() || j.b_judge(f, key, true).is_err() {
            ok = false;
        }
        k += 1;
    }
    s.add(
        "J1-终端族互认",
        ok && j.mutual() && j.conflicts().is_empty(),
        "三亚族七条判据双裁同向全互认",
    );

    // J2 族覆盖逐族达锚
    let j2 = j.family_ok(FAM_TERMIOS)
        && j.family_ok(FAM_WINSIZE)
        && j.family_ok(FAM_CTTY);
    s.add("J2-逐族达锚", j2, "3/2/2 三族行数达锚且族内全平");

    // J3 注入检出：坏族号 + 重键
    let inj = inject_four();
    s.add("J3-族号重键检出", inj[0] && inj[1], "坏族号与同族重键必拒");

    // J4 注入检出：未裁即平 + 异裁入单
    s.add("J4-未裁异裁检出", inj[2] && inj[3], "未裁不成互认且异裁必入冲突单");

    // J5 异裁阻断族互认
    let j5 = {
        let mut t = PtyJoint::new();
        let _ = t.register(FAM_TERMIOS, "a", "甲");
        let _ = t.a_judge(FAM_TERMIOS, "a", true);
        let _ = t.b_judge(FAM_TERMIOS, "a", false);
        !t.family_ok(FAM_TERMIOS) && !t.mutual()
    };
    s.add("J5-异裁阻断", j5, "任一条异裁该族即不平");

    // J6 未知行裁定必拒
    let j6 = PtyJoint::new().a_judge(FAM_TERMIOS, "无", true).err() == Some(PJ_NOROW);
    s.add("J6-未知行拒裁", j6, "未登记行不得裁定");

    // J7 重裁必拒
    let j7 = {
        let mut t = PtyJoint::new();
        let _ = t.register(FAM_TERMIOS, "a", "甲");
        let _ = t.a_judge(FAM_TERMIOS, "a", true);
        let _ = t.b_judge(FAM_TERMIOS, "a", false);
        let _ = t.b_judge(FAM_TERMIOS, "a", true);
        t.a_judge(FAM_TERMIOS, "a", false).err() == Some(PJ_REJUDGE)
            && t.b_judge(FAM_TERMIOS, "a", true).err() == Some(PJ_REJUDGE)
    };
    s.add("J7-重裁拒收", j7, "单侧二次裁定必拒");

    // J8 封册后登记与裁定均拒
    let j8 = {
        let mut t = PtyJoint::new();
        let _ = t.register(FAM_TERMIOS, "a", "甲");
        t.seal();
        t.register(FAM_WINSIZE, "b", "乙").err() == Some(PJ_SEALED)
            && t.a_judge(FAM_TERMIOS, "a", true).err() == Some(PJ_SEALED)
    };
    s.add("J8-封册拒动", j8, "封册后登记裁定均拒");

    // J9 负裁也可互认（双方同判否）
    let j9 = {
        let mut t = PtyJoint::new();
        let _ = t.register(FAM_CTTY, "x", "某判据");
        let _ = t.a_judge(FAM_CTTY, "x", false);
        let _ = t.b_judge(FAM_CTTY, "x", false);
        t.rows[0].agree()
    };
    s.add("J9-同否互认", j9, "双裁同为否亦算互认");

    // J10 容量边界
    let j10 = {
        let mut t = PtyJoint::new();
        let mut n = 0u8;
        let mut filled = true;
        while (n as usize) < CAP {
            let f = 1 + (n % 3);
            let key = format!("k{}", n);
            if t.register(f, &key, "填").is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && t.register(FAM_TERMIOS, "over", "超").err() == Some(PJ_CAP)
    };
    s.add("J10-容量边界", j10, "超容必拒且原册不损");

    // J11 指纹随裁定推进且封册后稳定
    let j11 = {
        let mut t = PtyJoint::new();
        let _ = t.register(FAM_TERMIOS, "a", "甲");
        let f0 = t.fold();
        let _ = t.a_judge(FAM_TERMIOS, "a", true);
        let f1 = t.fold();
        let _ = t.b_judge(FAM_TERMIOS, "a", true);
        let f2 = t.fold();
        t.seal();
        let f3 = t.fold();
        f0 != f1 && f1 != f2 && f2 != f3 && t.fold() == f3
    };
    s.add("J11-指纹推进", j11, "每裁一步指纹一步且封册后稳定");

    // J12 动态明细：互认行数
    let mut agree_n = 0usize;
    let mut q = 0usize;
    while q < j.rows.len() {
        if j.rows[q].agree() {
            agree_n += 1;
        }
        q += 1;
    }
    s.add_dyn(
        "J12-互认行数",
        agree_n == j.rows.len(),
        &format!("agree={}/{}", agree_n, j.rows.len()),
    );

    // J13 动态明细：号面指纹落账
    let fp = j.fold();
    s.add_dyn("J13-号面指纹", fp != 0, &format!("fold={:#x}", fp));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10322_pty_joint_flow() {
        let mut j = PtyJoint::new();
        assert!(j.register(0, "x", "y").err() == Some(PJ_FAM));
        assert!(j.register(FAM_TERMIOS, "a", "甲").is_ok());
        assert!(j.register(FAM_TERMIOS, "a", "重").err() == Some(PJ_DUP));
        assert!(j.a_judge(FAM_TERMIOS, "无", true).err() == Some(PJ_NOROW));
        assert!(j.a_judge(FAM_TERMIOS, "a", true).is_ok());
        assert!(j.a_judge(FAM_TERMIOS, "a", false).err() == Some(PJ_REJUDGE));
        assert!(j.b_judge(FAM_TERMIOS, "a", false).is_ok());
        assert!(j.rows[0].agree());
        j.seal();
        assert!(j.register(FAM_CTTY, "b", "乙").err() == Some(PJ_SEALED));
    }

    #[test]
    fn unx10322_checks_green() {
        let s = run_unx10322_checks();
        assert!(s.all_passed(), "UNX-F10322 判据未全绿");
    }
}
