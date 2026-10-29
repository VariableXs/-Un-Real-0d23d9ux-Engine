//! UNX-F9151 execveat 语义档（AT_EMPTY_PATH/AT_SYMLINK_FOLLOW）（0x632A0 段）
//!
//! 锚点：UNX-F9151-J1 解析账（每次 execveat 解析落账：dirfd、path、
//! 旗标、解析结果、留痕）；J2 解析语义（旗标位闭集 + 空路径须
//! AT_EMPTY_PATH + 符号链须 AT_SYMLINK_FOLLOW + 解析唯一映射四性
//! 质）；J3 解析指纹（账本折叠指纹）三判据全绿。
//! 域：CoRun·UNX-C2-B18（execveat 旗标解析面）。
//!
//! 结构：fd 映像表（dirfd→映像名/符号链标记）+ 解析核（路径/空路
//! 径/符号链三路）+ 解析账本 + 折叠指纹 + 注入核（错旗标/空留痕/
//! 未知 fd/重序号/印后登记必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x632A0 独占 0x20）。
pub const SEG_BASE: u32 = 0x632A0;
pub const EV_UNK_FD: u32 = SEG_BASE + 0x01; // 未知 dirfd
pub const EV_BAD_FLAG: u32 = SEG_BASE + 0x02; // 非法旗标
pub const EV_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const EV_DUP_SEQ: u32 = SEG_BASE + 0x04; // 重序号
pub const EV_FROZEN: u32 = SEG_BASE + 0x05; // 印后登记

/// 旗标位闭集。
pub const A_EMPTY_PATH: u32 = 0x1000; // path="" 时用 dirfd 映像
pub const A_SYMLINK_FOLLOW: u32 = 0x2000; // 允许跟进符号链
pub const A_KNOWN_MASK: u32 = A_EMPTY_PATH | A_SYMLINK_FOLLOW;

/// 结果码。
pub const R_OK: u32 = 0;
pub const R_ENOENT: u32 = 1; // 空路径无 AT_EMPTY_PATH / 映像不在册
pub const R_EINVAL: u32 = 2; // 旗标出闭集
pub const R_ELOOP: u32 = 3; // 符号链未授权跟进
pub const R_COUNT: usize = 4;

pub const R_NAMES: [&str; R_COUNT] = ["OK", "ENOENT", "EINVAL", "ELOOP"];

/// fd 登记的映像。
#[derive(Clone)]
pub struct FdImg {
    pub dirfd: u32,
    pub image: String,
    pub symlink: bool,
}

/// 单条解析账。
#[derive(Clone)]
pub struct Resolve {
    pub seq: u32,
    pub dirfd: u32,
    pub path: String,
    pub flags: u32,
    pub code: u32,
    pub image: String,
    pub note: String,
}

impl Resolve {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && (self.code as usize) < R_COUNT && self.flags & !A_KNOWN_MASK == 0
    }

    pub fn render_line(&self) -> String {
        format!(
            "RS{}|fd{}|{}|f{:x}|{}|{}",
            self.seq,
            self.dirfd,
            self.path,
            self.flags,
            R_NAMES[self.code as usize],
            self.note
        )
    }
}

/// execveat 解析簿。
pub struct EvBook {
    pub fds: Vec<FdImg>,
    pub ledger: Vec<Resolve>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl EvBook {
    pub fn new() -> EvBook {
        EvBook { fds: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    pub fn open(&mut self, dirfd: u32, image: &str, symlink: bool) -> Result<(), u32> {
        if self.sealed {
            return Err(EV_FROZEN);
        }
        if image.is_empty() {
            return Err(EV_UNK_FD);
        }
        if let Some(i) = self.fds.iter().position(|f| f.dirfd == dirfd) {
            self.fds[i].image = image.to_string();
            self.fds[i].symlink = symlink;
            return Ok(());
        }
        self.fds.push(FdImg { dirfd, image: image.to_string(), symlink });
        Ok(())
    }

    /// 解析核：execveat(dirfd, path, flags)。
    pub fn execveat(
        &mut self,
        dirfd: u32,
        path: &str,
        flags: u32,
        note: &str,
    ) -> Result<(u32, String), u32> {
        if self.sealed {
            return Err(EV_FROZEN);
        }
        if note.is_empty() {
            return Err(EV_EMPTY_NOTE);
        }
        if flags & !A_KNOWN_MASK != 0 {
            self.record(dirfd, path, flags, R_EINVAL, "", note);
            return Err(R_EINVAL);
        }
        // 路径解析三路
        if path.is_empty() {
            if flags & A_EMPTY_PATH == 0 {
                self.record(dirfd, path, flags, R_ENOENT, "", note);
                return Err(R_ENOENT);
            }
            let f = match self.fds.iter().find(|f| f.dirfd == dirfd) {
                Some(f) => f.clone(),
                None => {
                    self.record(dirfd, path, flags, R_ENOENT, "", note);
                    return Err(R_ENOENT);
                }
            };
            self.record(dirfd, path, flags, R_OK, &f.image, note);
            return Ok((dirfd, f.image));
        }
        // 非空路径：在册即映像（以路径名为映像键）；符号链须授权
        let want_sym = path.ends_with(".lnk");
        if want_sym && flags & A_SYMLINK_FOLLOW == 0 {
            self.record(dirfd, path, flags, R_ELOOP, "", note);
            return Err(R_ELOOP);
        }
        // 映像在册检查：路径名须与某 fd 的映像名一致（挂点简化：C1 落图名）
        let img = self.fds.iter().find(|f| f.image == path).map(|f| f.image.clone());
        match img {
            Some(im) => {
                self.record(dirfd, path, flags, R_OK, &im, note);
                Ok((dirfd, im))
            }
            None => {
                self.record(dirfd, path, flags, R_ENOENT, "", note);
                Err(R_ENOENT)
            }
        }
    }

    fn record(&mut self, dirfd: u32, path: &str, flags: u32, code: u32, image: &str, note: &str) {
        let seq = self.seq_next;
        self.seq_next += 1;
        self.ledger.push(Resolve {
            seq,
            dirfd,
            path: path.to_string(),
            flags,
            code,
            image: image.to_string(),
            note: note.to_string(),
        });
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
        let mut h: u32 = 0x91510;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.seq);
            h = h.wrapping_mul(31).wrapping_add(r.dirfd);
            h = h.wrapping_mul(31).wrapping_add(r.flags);
            h = h.wrapping_mul(31).wrapping_add(r.code);
            for b in r.path.bytes() {
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

/// 基础样本：fd3→/bin/sh（实链）、fd4→/bin/ld.so.lnk（符号链）、
/// 路径映像 /bin/tool 经 fd5 落图。
pub fn mk_book() -> EvBook {
    let mut b = EvBook::new();
    let _ = b.open(3, "/bin/sh", false);
    let _ = b.open(4, "/bin/ld.so.lnk", true);
    let _ = b.open(5, "/bin/tool", false);
    b
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 非法旗标
    let bad = Resolve {
        seq: 1,
        dirfd: 3,
        path: "".to_string(),
        flags: 0x4000,
        code: 0,
        image: String::new(),
        note: "inj-flag".to_string(),
    };
    got[0] = !bad.ok();
    // 2) 空留痕
    let bad2 = Resolve {
        seq: 2,
        dirfd: 3,
        path: "".to_string(),
        flags: 0,
        code: 0,
        image: String::new(),
        note: String::new(),
    };
    got[1] = !bad2.ok();
    // 3) 未知 fd 空路径
    let mut b = mk_book();
    got[2] = b.execveat(99, "", A_EMPTY_PATH, "inj-unk").err() == Some(R_ENOENT);
    // 4) 重序号
    let _ = b.execveat(3, "", A_EMPTY_PATH, "real");
    b.ledger.push(Resolve {
        seq: 1,
        dirfd: 3,
        path: "".to_string(),
        flags: 0,
        code: 0,
        image: String::new(),
        note: "inj-dup".to_string(),
    });
    got[3] = !b.seq_unique();
    // 5) 印后登记必抓
    let mut b2 = mk_book();
    b2.seal();
    got[4] = b2.execveat(3, "", A_EMPTY_PATH, "inj-frozen").err() == Some(EV_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx9151_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9151");
    let mut b = mk_book();

    // J1 解析账非空（成功 + 拒绝两式）
    let j1 = {
        let _ = b.execveat(3, "", A_EMPTY_PATH, "空路径用 fd3");
        let _ = b.execveat(3, "", 0, "空路径无旗标");
        b.ledger.iter().filter(|r| r.code == R_OK).count() >= 1
            && b.ledger.iter().filter(|r| r.code == R_ENOENT).count() >= 1
    };
    s.add("J1-解析账两式", j1, "成功与 ENOENT 账各至少一条");

    // J2 账字段齐
    let j2 = b.ledger.iter().all(|r| r.ok());
    s.add("J2-账字段齐备", j2, "留痕非空且码/旗标闭集");

    // J3 空路径须 AT_EMPTY_PATH
    let j3 = b.execveat(3, "", 0, "无旗标").err() == Some(R_ENOENT);
    s.add("J3-空路径须旗标", j3, "空路径无 AT_EMPTY_PATH 必 ENOENT");

    // J4 AT_EMPTY_PATH 正解析
    let j4 = b.execveat(3, "", A_EMPTY_PATH, "再试") == Ok((3, "/bin/sh".to_string()));
    s.add("J4-空路径正解析", j4, "fd3 空路径解析出 /bin/sh");

    // J5 旗标闭集
    let j5 = b.execveat(3, "", 0x4000, "野旗标").err() == Some(R_EINVAL);
    s.add("J5-旗标闭集", j5, "出闭集旗标必 EINVAL");

    // J6 符号链须授权
    let j6 = {
        let mut b2 = mk_book();
        let no = b2.execveat(4, "/bin/ld.so.lnk", 0, "不跟链").err() == Some(R_ELOOP);
        let yes = b2
            .execveat(4, "/bin/ld.so.lnk", A_SYMLINK_FOLLOW, "跟链")
            .is_ok();
        no && yes
    };
    s.add("J6-符号链授权", j6, "无 AT_SYMLINK_FOLLOW 必 ELOOP，有则放行");

    // J7 非空路径在册解析
    let j7 = {
        let mut b2 = mk_book();
        b2.execveat(5, "/bin/tool", 0, "路径解析").is_ok()
    };
    s.add("J7-路径解析", j7, "在册路径映像可解析");

    // J8 非空路径不在册 ENOENT
    let j8 = {
        let mut b2 = mk_book();
        b2.execveat(5, "/bin/none", 0, "不在册").err() == Some(R_ENOENT)
    };
    s.add("J8-不在册ENOENT", j8, "不在册路径必 ENOENT");

    // J9 解析唯一映射：同参数两次解析同映像
    let j9 = {
        let mut b2 = mk_book();
        let a = b2.execveat(3, "", A_EMPTY_PATH, "一");
        let bb = b2.execveat(3, "", A_EMPTY_PATH, "二");
        a == bb
    };
    s.add("J9-解析唯一", j9, "同参数解析结果一致");

    // J10 fd 重开覆盖
    let j10 = {
        let mut b2 = mk_book();
        let _ = b2.open(3, "/bin/newsh", false);
        b2.execveat(3, "", A_EMPTY_PATH, "覆盖后") == Ok((3, "/bin/newsh".to_string()))
            && b2.fds.len() == 3
    };
    s.add("J10-fd重开覆盖", j10, "同 fd 重开覆盖映像不增席");

    // J11 失败不落错账（拒绝也留痕但 code 正确）
    let j11 = {
        let before = b.ledger.len();
        let _ = b.execveat(9, "", A_EMPTY_PATH, "未知fd");
        b.ledger.len() == before + 1
            && b.ledger.last().unwrap().code == R_ENOENT
    };
    s.add("J11-拒绝留痕", j11, "拒绝路径留痕且码正确");

    // J12 指纹稳定 + seq 唯一
    let j12 = b.fold() == b.fold() && b.seq_unique();
    s.add("J12-指纹稳定", j12, "折叠指纹两次一致且 seq 唯一");

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
    fn unx9151_resolve_flow() {
        let mut b = mk_book();
        // 空路径
        assert_eq!(b.execveat(3, "", 0, "a"), Err(R_ENOENT));
        assert_eq!(b.execveat(3, "", A_EMPTY_PATH, "b"), Ok((3, "/bin/sh".to_string())));
        // 符号链
        assert_eq!(b.execveat(4, "/bin/ld.so.lnk", 0, "c"), Err(R_ELOOP));
        assert!(b.execveat(4, "/bin/ld.so.lnk", A_SYMLINK_FOLLOW, "d").is_ok());
        // 野旗标
        assert_eq!(b.execveat(3, "", 0x4000, "e"), Err(R_EINVAL));
        // 未知 fd
        assert_eq!(b.execveat(99, "", A_EMPTY_PATH, "f"), Err(R_ENOENT));
    }

    #[test]
    fn unx9151_checks_green() {
        let s = run_unx9151_checks();
        assert!(s.all_passed(), "UNX-F9151 判据未全绿");
    }
}
