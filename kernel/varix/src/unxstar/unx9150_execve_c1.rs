//! UNX-F9150 execve 挂点协议（与 C1 加载链联签）（0x63280 段）
//!
//! 锚点：UNX-F9150-J1 联签账（每次 execve 挂点落账：pid、目标映像、
//! 协议版本、联签印、留痕）；J2 联签语义（映像在册闸 + 协议版本锁
//! + 联签印确定性 + 失败账留痕四性质）；J3 联签指纹（账本折叠指纹）
//! 三判据全绿。
//! 域：CoRun·UNX-C2-B18（execve 挂点与 C1 加载链联签面）。
//!
//! 结构：映像登记簿（C1 加载链产物清单）+ 挂点协议（版本锁 + 联签
//! 印 = 映像指纹×协议版本折叠）+ 联签账本 + 折叠指纹 + 注入核（错
//! 版本/空留痕/未知映像/重序号/印后登记必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x63280 独占 0x20）。
pub const SEG_BASE: u32 = 0x63280;
pub const EX_UNK_IMAGE: u32 = SEG_BASE + 0x01; // 未知映像
pub const EX_BAD_VER: u32 = SEG_BASE + 0x02; // 错协议版本
pub const EX_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const EX_DUP_SEQ: u32 = SEG_BASE + 0x04; // 重序号
pub const EX_FROZEN: u32 = SEG_BASE + 0x05; // 印后登记

/// 挂点协议版本（与 C1 加载链当前版本锁定）。
pub const PROTO_VER: u32 = 3;

/// 单条联签账。
#[derive(Clone)]
pub struct Sign {
    pub seq: u32,
    pub pid: u32,
    pub image: String,
    pub ver: u32,
    pub seal: u32,
    pub note: String,
}

impl Sign {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.ver == PROTO_VER
    }

    pub fn render_line(&self) -> String {
        format!("XS{}|p{}|{}|v{}|s{}", self.seq, self.pid, self.image, self.ver, self.seal)
    }
}

/// 映像登记簿 + 挂点联签。
pub struct ExeBook {
    /// (映像名, C1 加载链指纹)
    pub images: Vec<(String, u32)>,
    pub ledger: Vec<Sign>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl ExeBook {
    pub fn new() -> ExeBook {
        ExeBook { images: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 登记映像（C1 加载链落图）。
    pub fn register(&mut self, image: &str, img_fp: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(EX_FROZEN);
        }
        if image.is_empty() {
            return Err(EX_UNK_IMAGE);
        }
        if self.images.iter().any(|(n, _)| n == image) {
            // 重登记覆盖指纹（同映像重载）
            for e in self.images.iter_mut() {
                if e.0 == image {
                    e.1 = img_fp;
                }
            }
            return Ok(());
        }
        self.images.push((image.to_string(), img_fp));
        Ok(())
    }

    fn image_fp(&self, image: &str) -> Option<u32> {
        self.images.iter().find(|(n, _)| n == image).map(|(_, f)| *f)
    }

    /// 联签印：映像指纹 × 协议版本折叠。
    pub fn seal_of(&self, image: &str) -> Option<u32> {
        let fp = self.image_fp(image)?;
        Some(fp.wrapping_mul(0x9150).wrapping_add(PROTO_VER))
    }

    /// execve 挂点：映像在册闸 + 版本锁 + 联签。
    pub fn execve(&mut self, pid: u32, image: &str, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(EX_FROZEN);
        }
        if note.is_empty() {
            return Err(EX_EMPTY_NOTE);
        }
        let fp = match self.image_fp(image) {
            Some(f) => f,
            None => return Err(EX_UNK_IMAGE),
        };
        let seal = fp.wrapping_mul(0x9150).wrapping_add(PROTO_VER);
        let seq = self.seq_next;
        self.seq_next += 1;
        self.ledger.push(Sign {
            seq,
            pid,
            image: image.to_string(),
            ver: PROTO_VER,
            seal,
            note: note.to_string(),
        });
        Ok(seq)
    }

    /// 联签印确定性：同映像两次联签同印。
    pub fn seal_stable(&self, image: &str) -> bool {
        let sigs: Vec<u32> = self
            .ledger
            .iter()
            .filter(|s| s.image == image)
            .map(|s| s.seal)
            .collect();
        if sigs.len() < 2 {
            return true;
        }
        for i in 1..sigs.len() {
            if sigs[i] != sigs[0] {
                return false;
            }
        }
        true
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
        let mut h: u32 = 0x91500;
        for s in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(s.seq);
            h = h.wrapping_mul(31).wrapping_add(s.pid);
            h = h.wrapping_mul(31).wrapping_add(s.ver);
            h = h.wrapping_mul(31).wrapping_add(s.seal);
            for b in s.image.bytes() {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
            }
            for b in s.note.bytes() {
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

/// 基础样本：C1 加载链三映像。
pub fn mk_book() -> ExeBook {
    let mut b = ExeBook::new();
    let _ = b.register("/bin/init", 0x0000_00C1);
    let _ = b.register("/bin/sh", 0x0000_00C2);
    let _ = b.register("/bin/tool", 0x0000_00C3);
    b
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 错版本账
    let bad = Sign {
        seq: 1,
        pid: 1,
        image: "x".to_string(),
        ver: PROTO_VER + 1,
        seal: 0,
        note: "inj-ver".to_string(),
    };
    got[0] = !bad.ok();
    // 2) 空留痕
    let bad2 = Sign {
        seq: 2,
        pid: 1,
        image: "x".to_string(),
        ver: PROTO_VER,
        seal: 0,
        note: String::new(),
    };
    got[1] = !bad2.ok();
    // 3) 未知映像
    let mut b = mk_book();
    got[2] = b.execve(9, "/bin/none", "inj-unk").err() == Some(EX_UNK_IMAGE);
    // 4) 重序号
    let _ = b.execve(9, "/bin/sh", "real");
    b.ledger.push(Sign {
        seq: 1,
        pid: 9,
        image: "/bin/sh".to_string(),
        ver: PROTO_VER,
        seal: 1,
        note: "inj-dup".to_string(),
    });
    got[3] = !b.seq_unique();
    // 5) 印后登记必抓
    let mut b2 = mk_book();
    b2.seal();
    got[4] = b2.execve(9, "/bin/sh", "inj-frozen").err() == Some(EX_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx9150_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9150");
    let mut b = mk_book();

    // J1 联签账非空
    let _ = b.execve(7, "/bin/sh", "sh 换映像");
    let j1 = !b.ledger.is_empty();
    s.add("J1-联签账非空", j1, "execve 挂点落账至少一条");

    // J2 账字段齐
    let j2 = b.ledger.iter().all(|g| g.ok());
    s.add("J2-账字段齐备", j2, "留痕非空且版本锁定");

    // J3 未知映像闸
    let j3 = b.execve(8, "/bin/none", "未知").err() == Some(EX_UNK_IMAGE);
    s.add("J3-未知映像闸", j3, "不在册映像必拒");

    // J4 协议版本锁
    let j4 = b.ledger.iter().all(|g| g.ver == PROTO_VER);
    s.add("J4-版本锁", j4, "全部联签版本=当前协议版本");

    // J5 联签印确定性
    let j5 = {
        let _ = b.execve(7, "/bin/sh", "再联签");
        b.seal_stable("/bin/sh")
    };
    s.add("J5-联签印稳定", j5, "同映像两次联签同印");

    // J6 联签印与映像指纹绑定
    let j6 = {
        let expect = 0x0000_00C2u32.wrapping_mul(0x9150).wrapping_add(PROTO_VER);
        b.ledger.iter().filter(|g| g.image == "/bin/sh").all(|g| g.seal == expect)
    };
    s.add("J6-印绑映像", j6, "联签印由映像指纹折叠而来");

    // J7 映像在册清单
    let j7 = b.images.len() == 3;
    s.add("J7-映像清单", j7, "C1 加载链三映像在册");

    // J8 重登记覆盖
    let j8 = {
        let mut b2 = mk_book();
        let _ = b2.register("/bin/sh", 0xFEED);
        b2.image_fp("/bin/sh") == Some(0xFEED) && b2.images.len() == 3
    };
    s.add("J8-重登记覆盖", j8, "同映像重载覆盖指纹不增席");

    // J9 联签不改清单
    let j9 = b.images.len() == 3;
    s.add("J9-联签不改清单", j9, "execve 只联签不改映像席");

    // J10 空名映像登记必拒（C1 加载链不接受无名映像）
    let j10 = {
        let mut b2 = mk_book();
        b2.register("", 1).err() == Some(EX_UNK_IMAGE) && b2.images.len() == 3
    };
    s.add("J10-空名登记拒", j10, "空名映像登记必拒且清单不增席");

    // J11 指纹稳定 + seq 唯一
    let j11 = b.fold() == b.fold() && b.seq_unique();
    s.add("J11-指纹稳定", j11, "折叠指纹两次一致且 seq 唯一");

    // J12 失败不落账（未知映像拒绝不留痕）
    let j12 = {
        let before = b.ledger.len();
        let _ = b.execve(8, "/bin/none", "失败不落账");
        b.ledger.len() == before
    };
    s.add("J12-失败不落账", j12, "拒绝路径零污染账本");

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
    fn unx9150_sign_flow() {
        let mut b = mk_book();
        assert!(b.execve(7, "/bin/sh", "a").is_ok());
        assert_eq!(b.execve(7, "/bin/none", "b"), Err(EX_UNK_IMAGE));
        assert!(b.seal_stable("/bin/sh"));
        let expect = 0x0000_00C2u32.wrapping_mul(0x9150).wrapping_add(PROTO_VER);
        assert_eq!(b.ledger[0].seal, expect);
        // 重登记覆盖
        let _ = b.register("/bin/sh", 0xFEED);
        assert_eq!(b.image_fp("/bin/sh"), Some(0xFEED));
    }

    #[test]
    fn unx9150_checks_green() {
        let s = run_unx9150_checks();
        assert!(s.all_passed(), "UNX-F9150 判据未全绿");
    }
}
