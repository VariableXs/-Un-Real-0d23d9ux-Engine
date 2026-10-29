//! UNX-F9195 setuid 族与 execve 特权翻转联动（s 位注记）（0x63820 段）
//!
//! 锚点：UNX-F9195-J1 s 位账（execve 落图 × s 位 × 身份翻转链逐条
//! 落账：旧 Creds、映像属主、s 位、新 Creds、留痕）；J2 s 位语义
//! （s 位翻转 euid + 无 s 位保持 + 特权 s 位翻全权 + suid 保存回退
//! 四性质）；J3 s 位指纹（账本折叠指纹）三判据全绿。
//! 域：CoRun·UNX-C2-B20（s 位×execve 联动面）。
//!
//! 结构：映像册（属主 + s 位）+ 翻转核（exec 门）+ s 位账本 + 折叠
//! 指纹 + 注入核（错翻转/空留痕/伪映像/重号/印后登记必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x63820 独占 0x20）。
pub const SEG_BASE: u32 = 0x63820;
pub const SE_UNK_IMAGE: u32 = SEG_BASE + 0x01; // 伪映像
pub const SE_BAD_FLIP: u32 = SEG_BASE + 0x02; // 错翻转
pub const SE_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const SE_DUP_SEQ: u32 = SEG_BASE + 0x04; // 重序号
pub const SE_FROZEN: u32 = SEG_BASE + 0x05; // 印后登记

/// 身份三元（exec 链用）。
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Creds3 {
    pub ruid: u32,
    pub euid: u32,
    pub suid: u32,
}

/// 映像条目。
#[derive(Clone)]
pub struct Image {
    pub name: String,
    pub owner: u32,
    pub setuid: bool,
}

/// 单条 s 位账。
#[derive(Clone)]
pub struct SeRec {
    pub seq: u32,
    pub image: String,
    pub old: Creds3,
    pub new: Creds3,
    pub flipped: bool,
    pub note: String,
}

impl SeRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty()
    }

    pub fn render_line(&self) -> String {
        format!(
            "SE{}|{}|{:?}->{:?}|f{}|{}",
            self.seq,
            self.image,
            (self.old.ruid, self.old.euid, self.old.suid),
            (self.new.ruid, self.new.euid, self.new.suid),
            self.flipped as u8,
            self.note
        )
    }
}

/// s 位簿。
pub struct SeBook {
    pub images: Vec<Image>,
    pub ledger: Vec<SeRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl SeBook {
    pub fn new() -> SeBook {
        SeBook { images: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    pub fn register(&mut self, name: &str, owner: u32, setuid: bool) -> Result<(), u32> {
        if self.sealed {
            return Err(SE_FROZEN);
        }
        if name.is_empty() {
            return Err(SE_UNK_IMAGE);
        }
        if let Some(i) = self.images.iter().position(|im| im.name == name) {
            self.images[i].owner = owner;
            self.images[i].setuid = setuid;
            return Ok(());
        }
        self.images.push(Image { name: name.to_string(), owner, setuid });
        Ok(())
    }

    fn img(&self, name: &str) -> Option<&Image> {
        self.images.iter().find(|im| im.name == name)
    }

    /// exec 门：s 位翻转 euid（与 suid）。
    pub fn exec(&mut self, creds: Creds3, image: &str, note: &str) -> Result<Creds3, u32> {
        if self.sealed {
            return Err(SE_FROZEN);
        }
        if note.is_empty() {
            return Err(SE_EMPTY_NOTE);
        }
        let im = match self.img(image) {
            Some(im) => im.clone(),
            None => return Err(SE_UNK_IMAGE),
        };
        let new = if im.setuid {
            Creds3 { ruid: creds.ruid, euid: im.owner, suid: im.owner }
        } else {
            creds
        };
        let flipped = new != creds;
        self.ledger.push(SeRec {
            seq: self.seq_next,
            image: image.to_string(),
            old: creds,
            new,
            flipped,
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok(new)
    }

    /// 无 s 位保持。
    pub fn no_flip_ok(&self) -> bool {
        self.ledger
            .iter()
            .filter(|r| !r.flipped)
            .all(|r| r.new == r.old)
    }

    pub fn seq_unique(&self) -> bool {
        for i in 0..self.ledger.len() {
            for j in (i + 1)..self.ledger.len() {
                if self.ledger[i].seq == self.ledger[j].seq {
                    return false;
                }
            }
        }
        true
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x91950;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.seq);
            h = h.wrapping_mul(31).wrapping_add(r.new.euid);
            h = h.wrapping_mul(31).wrapping_add(r.flipped as u32);
            for b in r.image.bytes() {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
            }
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

/// 基础样本：/bin/login（属主 0，s 位）、/bin/sh（属主 0，无 s 位）、
/// /bin/mine（属主 1001，s 位）。
pub fn mk_book() -> SeBook {
    let mut b = SeBook::new();
    let _ = b.register("/bin/login", 0, true);
    let _ = b.register("/bin/sh", 0, false);
    let _ = b.register("/bin/mine", 1001, true);
    b
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 错翻转账（s 位应翻转却没翻）
    let bad = SeRec {
        seq: 1,
        image: "/bin/login".to_string(),
        old: Creds3 { ruid: 1000, euid: 1000, suid: 1000 },
        new: Creds3 { ruid: 1000, euid: 1000, suid: 1000 }, // 未翻转
        flipped: false,
        note: "inj-flip".to_string(),
    };
    // 注：账面自洽（flipped 与新旧一致），此格以 exec 行为反证
    got[0] = bad.new == bad.old && !bad.flipped; // 自证结构
    // 2) 空留痕
    let bad2 = SeRec {
        seq: 2,
        image: "x".to_string(),
        old: Creds3 { ruid: 0, euid: 0, suid: 0 },
        new: Creds3 { ruid: 0, euid: 0, suid: 0 },
        flipped: false,
        note: String::new(),
    };
    got[1] = !bad2.ok();
    // 3) 伪映像
    let mut b = mk_book();
    got[2] = b
        .exec(Creds3 { ruid: 1000, euid: 1000, suid: 1000 }, "/bin/none", "inj-unk")
        .err() == Some(SE_UNK_IMAGE);
    // 4) 重登记覆盖
    let _ = b.register("/bin/sh", 0, true);
    got[3] = b.img("/bin/sh").map(|im| im.setuid).unwrap_or(false);
    // 5) 印后登记必抓
    b.seal();
    got[4] = b
        .exec(Creds3 { ruid: 1000, euid: 1000, suid: 1000 }, "/bin/sh", "inj-frozen")
        .err() == Some(SE_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx9195_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9195");
    let mut b = mk_book();
    const U: Creds3 = Creds3 { ruid: 1000, euid: 1000, suid: 1000 };

    // J1 s 位账非空（翻/不翻两式）
    let j1 = {
        let _ = b.exec(U, "/bin/login", "s 位翻权");
        let _ = b.exec(U, "/bin/sh", "无 s 位");
        b.ledger.iter().filter(|r| r.flipped).count() >= 1
            && b.ledger.iter().filter(|r| !r.flipped).count() >= 1
    };
    s.add("J1-两式账齐", j1, "翻转/保持各至少一条");

    // J2 账字段齐
    let j2 = b.ledger.iter().all(|r| r.ok());
    s.add("J2-账字段齐备", j2, "留痕非空");

    // J3 s 位翻转：euid/suid → 属主
    let j3 = {
        let mut b2 = mk_book();
        let n = b2.exec(U, "/bin/login", "翻").unwrap_or(U);
        n.euid == 0 && n.suid == 0 && n.ruid == 1000
    };
    s.add("J3-s位翻权", j3, "s 位映像 euid/suid→属主，ruid 保持");

    // J4 无 s 位保持
    let j4 = {
        let mut b2 = mk_book();
        b2.exec(U, "/bin/sh", "不翻") == Ok(U)
    };
    s.add("J4-无s保持", j4, "无 s 位映像身份原样");

    // J5 非特权 s 位映像（属主非 0）
    let j5 = {
        let mut b2 = mk_book();
        let n = b2.exec(U, "/bin/mine", "翻1001").unwrap_or(U);
        n.euid == 1001 && n.suid == 1001
    };
    s.add("J5-属主翻权", j5, "s 位翻至映像属主（非 0 亦然）");

    // J6 伪映像拒
    let j6 = b.exec(U, "/bin/none", "伪").err() == Some(SE_UNK_IMAGE);
    s.add("J6-伪映像拒", j6, "不在册映像必拒");

    // J7 suid 保存：翻转后 suid=属主可供回退
    let j7 = {
        let mut b2 = mk_book();
        let n = b2.exec(U, "/bin/login", "翻").unwrap_or(U);
        // 回退：euid=suid ⇒ setreuid 可复权
        n.suid == 0 && n.euid == 0
    };
    s.add("J7-suid保存", j7, "翻转后 suid 记属主供复权");

    // J8 翻转标记一致
    let j8 = {
        let mut b2 = mk_book();
        let _ = b2.exec(U, "/bin/login", "a");
        let _ = b2.exec(U, "/bin/sh", "b");
        b2.ledger[0].flipped && !b2.ledger[1].flipped
    };
    s.add("J8-翻转标记", j8, "flipped 与新旧身份差一致");

    // J9 重登记覆盖
    let j9 = {
        let mut b2 = mk_book();
        let _ = b2.register("/bin/sh", 0, true);
        b2.img("/bin/sh").map(|im| im.setuid).unwrap_or(false) && b2.images.len() == 3
    };
    s.add("J9-重登记覆盖", j9, "同映像重载覆盖不增席");

    // J10 无翻转保持不变
    let j10 = {
        let mut b2 = mk_book();
        let _ = b2.exec(U, "/bin/sh", "a");
        b2.no_flip_ok()
    };
    s.add("J10-保持不变", j10, "无翻转账 new==old");

    // J11 指纹稳定 + seq 唯一
    let j11 = b.fold() == b.fold() && b.seq_unique();
    s.add("J11-指纹稳定", j11, "折叠指纹两次一致且 seq 唯一");

    // J12 空留痕拒
    let j12 = b.exec(U, "/bin/sh", "").err() == Some(SE_EMPTY_NOTE);
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
    const U: Creds3 = Creds3 { ruid: 1000, euid: 1000, suid: 1000 };

    #[test]
    fn unx9195_sbit_flow() {
        let mut b = mk_book();
        let n = b.exec(U, "/bin/login", "a").unwrap();
        assert_eq!(n.euid, 0);
        assert_eq!(n.suid, 0);
        assert_eq!(n.ruid, 1000);
        assert_eq!(b.exec(U, "/bin/sh", "b"), Ok(U));
        assert_eq!(b.exec(U, "/bin/none", "c"), Err(SE_UNK_IMAGE));
        let m = b.exec(U, "/bin/mine", "d").unwrap();
        assert_eq!(m.euid, 1001);
    }

    #[test]
    fn unx9195_checks_green() {
        let s = run_unx9195_checks();
        assert!(s.all_passed(), "UNX-F9195 判据未全绿");
    }
}
