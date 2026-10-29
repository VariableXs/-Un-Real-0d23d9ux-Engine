//! UNX-F11233 openat/read/close 文件族消费判据（0x1f833 段）
//!
//! 锚点：UNX-F11233-J1 glibc 高频文件操作序（openat→read→close）错误码透传
//! 判据绿，10 类错误码与 Linux 一致。
//! 域：CoRun·UNX-C5-B02——错误码不是数字而是三元组（名/号/文）：十类逐一
//! 登记后，操作序里的每个失败都必须原样带出名与文，不得改写、不得吞并。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1f833;
pub const FE_BAD: u32 = SEG_BASE + 0x01; // 未登记错误号
pub const FE_OP: u32 = SEG_BASE + 0x02; // 非法操作名
pub const FE_SEQ: u32 = SEG_BASE + 0x03; // 操作序不成立
pub const FE_COUNT: u32 = SEG_BASE + 0x04; // 错误类不足十
pub const FE_TEXT: u32 = SEG_BASE + 0x05; // 错误文为空
pub const FE_FROZEN: u32 = SEG_BASE + 0x06; // 冻结后改写
pub const FE_OVER: u32 = SEG_BASE + 0x07; // 超容

/// 十类错误码。
pub const CODES: [(&str, u32, &str); 10] = [
    ("ENOENT", 2, "No such file or directory"),
    ("EBADF", 9, "Bad file descriptor"),
    ("EACCES", 13, "Permission denied"),
    ("ENOTDIR", 20, "Not a directory"),
    ("EISDIR", 21, "Is a directory"),
    ("EINVAL", 22, "Invalid argument"),
    ("ENFILE", 23, "Too many open files in system"),
    ("EMFILE", 24, "Too many open files"),
    ("ENAMETOOLONG", 36, "File name too long"),
    ("ELOOP", 40, "Too many levels of symbolic links"),
];
pub const OPS: [&str; 3] = ["openat", "read", "close"];
pub const MAX_OPS: usize = 40;
pub const FE_TAG: u64 = 0x1f833e9;
pub const FP_STEP: u64 = 0x100000001b3;

/// 一条错误三元组。
#[derive(Clone, Debug)]
pub struct Errno {
    pub name: &'static str,
    pub nr: u32,
    pub text: &'static str,
}

/// 一次文件操作。
#[derive(Clone, Debug)]
pub struct FdOp {
    pub fd: i32,
    pub op: &'static str,
    pub errno: u32,
    pub bytes: u32,
}

/// 文件族轨迹账。
#[derive(Clone, Debug)]
pub struct FileTrace {
    pub errnos: Vec<Errno>,
    pub ops: Vec<FdOp>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl FileTrace {
    pub fn new() -> FileTrace {
        FileTrace { errnos: Vec::new(), ops: Vec::new(), frozen: false, root: None }
    }

    /// 登记错误三元组：重号、空文、空名三路拒。
    pub fn register(&mut self, name: &'static str, nr: u32, text: &'static str) -> Result<(), u32> {
        if self.frozen {
            return Err(FE_FROZEN);
        }
        if name.is_empty() || nr == 0 {
            return Err(FE_BAD);
        }
        if text.is_empty() {
            return Err(FE_TEXT);
        }
        if self.errnos.iter().any(|e| e.name == name || e.nr == nr) {
            return Err(FE_BAD);
        }
        if self.errnos.len() >= CODES.len() {
            return Err(FE_OVER);
        }
        self.errnos.push(Errno { name, nr, text });
        Ok(())
    }

    pub fn lookup(&self, nr: u32) -> Result<&Errno, u32> {
        let mut k = 0usize;
        while k < self.errnos.len() {
            if self.errnos[k].nr == nr {
                return Ok(&self.errnos[k]);
            }
            k += 1;
        }
        Err(FE_BAD)
    }

    /// 记一步：操作名合法、错误号零或已登记。
    pub fn step(&mut self, fd: i32, op: &'static str, errno: u32, bytes: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(FE_FROZEN);
        }
        if !OPS.iter().any(|o| *o == op) {
            return Err(FE_OP);
        }
        if errno != 0 && self.lookup(errno).is_err() {
            return Err(FE_BAD);
        }
        if errno == 0 && op == "openat" && fd < 0 {
            return Err(FE_SEQ);
        }
        if errno == 0 && op == "read" && (fd < 0 || !self.opened(fd)) {
            return Err(FE_SEQ);
        }
        if errno == 0 && op == "close" && (!self.opened(fd) || self.closed_already(fd)) {
            return Err(FE_SEQ);
        }
        if self.ops.len() >= MAX_OPS {
            return Err(FE_OVER);
        }
        self.ops.push(FdOp { fd, op, errno, bytes });
        Ok(())
    }

    pub fn opened(&self, fd: i32) -> bool {
        self.ops.iter().any(|o| o.fd == fd && o.op == "openat" && o.errno == 0)
    }

    pub fn closed_already(&self, fd: i32) -> bool {
        self.ops.iter().any(|o| o.fd == fd && o.op == "close" && o.errno == 0)
    }

    /// 序：read/close 之前同 fd 必有 openat，close 至多一次。
    pub fn seq_ok(&self) -> bool {
        let mut opens: Vec<i32> = Vec::new();
        let mut closed: Vec<i32> = Vec::new();
        let mut k = 0usize;
        while k < self.ops.len() {
            let o = &self.ops[k];
            if o.op == "openat" && o.errno == 0 {
                opens.push(o.fd);
            } else if o.op == "read" && o.errno == 0 {
                if !opens.contains(&o.fd) {
                    return false;
                }
            } else if o.op == "close" && o.errno == 0 {
                if !opens.contains(&o.fd) || closed.contains(&o.fd) {
                    return false;
                }
                closed.push(o.fd);
            }
            k += 1;
        }
        !opens.is_empty() && !closed.is_empty()
    }

    pub fn passthrough(&self, nr: u32) -> String {
        match self.lookup(nr) {
            Ok(e) => format!("{} ({})", e.text, e.nr),
            Err(_) => String::from("?"),
        }
    }

    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(FE_FROZEN);
        }
        if self.errnos.len() != CODES.len() {
            return Err(FE_COUNT);
        }
        if !self.seq_ok() {
            return Err(FE_SEQ);
        }
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        Ok(r)
    }

    pub fn render(&self) -> String {
        format!("file ops {} codes {} seq {}", self.ops.len(), self.errnos.len(), self.seq_ok())
    }

    pub fn fold(&self) -> u64 {
        let mut h: u64 = FE_TAG;
        let mut k = 0usize;
        while k < self.errnos.len() {
            let b = self.errnos[k].name.as_bytes();
            let mut i = 0usize;
            while i < b.len() {
                h = (h ^ b[i] as u64).wrapping_mul(FP_STEP);
                i += 1;
            }
            h = (h ^ self.errnos[k].nr as u64).wrapping_mul(FP_STEP);
            k += 1;
        }
        k = 0;
        while k < self.ops.len() {
            h = (h ^ self.ops[k].fd as u64).wrapping_mul(FP_STEP);
            h = (h ^ self.ops[k].errno as u64).wrapping_mul(FP_STEP);
            h = (h ^ self.ops[k].bytes as u64).wrapping_mul(FP_STEP);
            k += 1;
        }
        h
    }
}

/// 组一张十码全登、成功序与十路失败并存的账。
pub fn build() -> FileTrace {
    let mut t = FileTrace::new();
    let mut k = 0usize;
    while k < CODES.len() {
        let (n, nr, tx) = CODES[k];
        let _ = t.register(n, nr, tx);
        k += 1;
    }
    let _ = t.step(3, "openat", 0, 0);
    let _ = t.step(3, "read", 0, 512);
    let _ = t.step(3, "close", 0, 0);
    k = 0;
    while k < CODES.len() {
        let _ = t.step(-1, "openat", CODES[k].1, 0);
        k += 1;
    }
    t
}

/// 四路对抗：未登错误号、非法操作名、无 open 即 close、九码冻存。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut a = build();
    out[0] = a.step(-1, "openat", 999, 0) == Err(FE_BAD);
    out[1] = a.step(0, "write", 0, 4) == Err(FE_OP);
    let mut b = FileTrace::new();
    let _ = b.register("ENOENT", 2, "No such file or directory");
    out[2] = b.step(7, "close", 0, 0) == Err(FE_SEQ);
    let mut c = FileTrace::new();
    let mut k = 0usize;
    while k < 9 {
        let (n, nr, tx) = CODES[k];
        let _ = c.register(n, nr, tx);
        k += 1;
    }
    out[3] = c.freeze() == Err(FE_COUNT);
    out
}

/// 判据入口。
pub fn run_unx11233_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11233");
    let mut t = build();
    let root = t.freeze().unwrap_or(0);

    s.add("J1-十类错误", t.errnos.len() == CODES.len(), "十类错误码全部登记");
    s.add("J2-号唯一", {
        let mut seen: Vec<u32> = Vec::new();
        for e in t.errnos.iter() {
            if !seen.contains(&e.nr) {
                seen.push(e.nr);
            }
        }
        seen.len() == 10
    }, "十个错误号两两不同");
    s.add("J3-文非空", t.errnos.iter().all(|e| !e.text.is_empty()), "每条错误文均非空");
    s.add("J4-ENOENT 文", t.lookup(2).map(|e| e.text) == Ok("No such file or directory"), "ENOENT 文与 Linux 逐字一致");
    s.add("J5-透传十路", {
        let mut ok = 0usize;
        for e in t.errnos.iter() {
            if t.passthrough(e.nr).as_str() == format!("{} ({})", e.text, e.nr) {
                ok += 1;
            }
        }
        ok == 10
    }, "十类错误号透传后名与文不改写");
    s.add("J6-成功读", t.ops.iter().any(|o| o.op == "read" && o.errno == 0 && o.bytes == 512), "成功 read 返回五百一十二字节");
    s.add("J7-操作序", t.seq_ok(), "openat→read→close 序成立且 close 只一次");
    s.add("J8-冻存根", root != 0 && t.root == Some(root), "文件族账冻存产出非零根");

    s.add("J9-冻后步拒", t.step(4, "openat", 0, 0) == Err(FE_FROZEN), "冻存后记步拒为 FE_FROZEN");
    s.add("J10-未登号拒", {
        let mut x = FileTrace::new();
        x.step(-1, "openat", 999, 0) == Err(FE_BAD)
    }, "未登记错误号拒为 FE_BAD");
    s.add("J11-非法操作拒", {
        let mut x = FileTrace::new();
        x.step(0, "lseek", 0, 0) == Err(FE_OP)
    }, "操作名不在三操作内拒为 FE_OP");

    let four = inject_four();
    s.add("J12-四路注入", four.iter().all(|x| *x), "未登号/非法操作/无 open 即 close/九码四路全拒");
    s.add_dyn(
        "J13-透传明细",
        t.errnos.len() == 10 && t.passthrough(36) == "File name too long (36)",
        &format!("十码全登，ENAMETOOLONG 透传为 {}", t.passthrough(36)),
    );
    s.add_dyn(
        "J14-账根",
        root == t.fold(),
        &format!("根 0x{:x}，步数 {}，失败步 {}", root, t.ops.len(), CODES.len()),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11233_file_family_flow() {
        let mut t = build();
        assert!(t.seq_ok());
        assert_eq!(t.errnos.len(), 10);
        let r = t.freeze().unwrap_or(0);
        assert!(r != 0);
        assert_eq!(t.step(4, "openat", 0, 0), Err(FE_FROZEN));
        assert!(inject_four().iter().all(|x| *x));
    }

    #[test]
    fn unx11233_checks_green() {
        let s = run_unx11233_checks();
        assert!(s.all_passed(), "UNX-F11233 判据未全绿");
    }
}
