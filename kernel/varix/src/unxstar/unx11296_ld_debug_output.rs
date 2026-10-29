//! UNX-F11296 LD_DEBUG_OUTPUT 文件通道（0x1f6b0 段）
//!
//! 锚点：UNX-F11296-J1 LD_DEBUG_OUTPUT 文件按 pid 命名、只追加不截断，
//! 内容与 fd2 通道逐行一致。
//!
//! 纯功能模型：当 LD_DEBUG_OUTPUT 指向一个路径时，装载器不写 fd 2，而是
//! 打开「路径.pid」这一文件，把本进程的诊断整行追加进去。模型复刻三条
//! 语义：文件后缀必为本进程 pid；同一 pid 多次打开只能追加，任何截断请求
//! 一律拒（FO_EAPPEND）；不同 pid 各得一份文件，互不串扰。判定「内容一致」
//! 时把同一消息序列分别喂给文件通道与 fd2 镜像，逐行比对必须相同。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f6b0;
/// 输出基名为空或含 NUL
pub const FO_ENAME: u32 = SEG_BASE + 0x01;
/// pid 为 0，无法命名
pub const FO_EPID: u32 = SEG_BASE + 0x02;
/// 冻结后打开或写入
pub const FO_EFROZEN: u32 = SEG_BASE + 0x03;
/// 请求截断（文件通道只允许追加）
pub const FO_EAPPEND: u32 = SEG_BASE + 0x04;
/// 文件未打开即写入 / 关闭
pub const FO_ENOENT: u32 = SEG_BASE + 0x05;
/// 空诊断行
pub const FO_ELINE: u32 = SEG_BASE + 0x06;
/// 单行超过 200 字节
pub const FO_EOVER: u32 = SEG_BASE + 0x07;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;
pub const LINE_MAX: usize = 200;

fn fold_byte(h: u64, b: u8) -> u64 {
    (h ^ b as u64).wrapping_mul(FNV_PRIME)
}

fn fold_str(h: u64, s: &str) -> u64 {
    let mut x = h;
    let mut k = 0usize;
    let bytes = s.as_bytes();
    while k < bytes.len() {
        if let Some(b) = bytes.get(k) {
            x = fold_byte(x, *b);
        }
        k = k.wrapping_add(1);
    }
    x
}

fn fold_u32(h: u64, v: u32) -> u64 {
    let mut x = h;
    let mut s = 0u32;
    while s < 32 {
        x = fold_byte(x, ((v >> s) & 0xff) as u8);
        s = s.wrapping_add(8);
    }
    x
}

// ---------------------------------------------------------------- 单 pid 文件
/// 单个 pid 的诊断文件：名字、正文、累计字节与打开次数。
#[derive(Clone, Debug)]
pub struct PidFile {
    pub pid: u32,
    pub name: String,
    pub lines: Vec<String>,
    pub bytes: u32,
    pub opens: u32,
    pub appends: u32,
}

impl PidFile {
    fn new(pid: u32, name: String) -> Self {
        PidFile {
            pid,
            name,
            lines: Vec::new(),
            bytes: 0,
            opens: 0,
            appends: 0,
        }
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }
}

/// LD_DEBUG_OUTPUT 文件通道。
#[derive(Clone, Debug)]
pub struct FileChannel {
    base: String,
    files: Vec<PidFile>,
    current: Option<u32>,
    fd: u32,
    pub opens: u32,
    pub appends: u32,
    pub writes: u32,
    pub closes: u32,
    pub rejects: u32,
    pub lines_total: u32,
    pub bytes_total: u32,
    seal: Option<u64>,
}

impl FileChannel {
    pub fn new(base: &str) -> Self {
        FileChannel {
            base: base.to_string(),
            files: Vec::new(),
            current: None,
            fd: 0,
            opens: 0,
            appends: 0,
            writes: 0,
            closes: 0,
            rejects: 0,
            lines_total: 0,
            bytes_total: 0,
            seal: None,
        }
    }

    pub fn base(&self) -> &str {
        self.base.as_str()
    }

    pub fn fd(&self) -> u32 {
        self.fd
    }

    pub fn is_open(&self) -> bool {
        self.fd != 0
    }

    pub fn is_frozen(&self) -> bool {
        self.seal.is_some()
    }

    pub fn current_pid(&self) -> Option<u32> {
        self.current
    }

    pub fn files_len(&self) -> usize {
        self.files.len()
    }

    /// 实际落盘文件名：基名 + "." + pid。
    pub fn name_for(&self, pid: u32) -> String {
        format!("{}.{}", self.base, pid)
    }

    fn index_of(&self, pid: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < self.files.len() {
            if let Some(f) = self.files.get(k) {
                if f.pid == pid {
                    return Some(k);
                }
            }
            k = k.wrapping_add(1);
        }
        None
    }

    pub fn file(&self, pid: u32) -> Option<&PidFile> {
        match self.index_of(pid) {
            Some(k) => self.files.get(k),
            None => None,
        }
    }

    pub fn current_lines(&self) -> &[String] {
        match self.current {
            Some(pid) => match self.index_of(pid) {
                Some(k) => match self.files.get(k) {
                    Some(f) => f.lines.as_slice(),
                    None => &[],
                },
                None => &[],
            },
            None => &[],
        }
    }

    /// 打开「基名.pid」：只允许追加；同一 pid 再开即追加打开。
    pub fn open(&mut self, pid: u32, append: bool) -> Result<u32, u32> {
        if self.seal.is_some() {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(FO_EFROZEN);
        }
        if pid == 0 {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(FO_EPID);
        }
        if self.base.is_empty() || self.base.contains('\0') {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(FO_ENAME);
        }
        if !append {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(FO_EAPPEND);
        }
        self.opens = self.opens.wrapping_add(1);
        if let Some(k) = self.index_of(pid) {
            if let Some(f) = self.files.get_mut(k) {
                f.opens = f.opens.wrapping_add(1);
                f.appends = f.appends.wrapping_add(1);
            }
            self.appends = self.appends.wrapping_add(1);
        } else {
            let name = self.name_for(pid);
            let mut f = PidFile::new(pid, name);
            f.opens = 1;
            self.files.push(f);
        }
        self.current = Some(pid);
        self.fd = 3;
        Ok(3)
    }

    /// 整行追加；未打开、空行、超长均拒。
    pub fn write(&mut self, msg: &str) -> Result<u32, u32> {
        if self.seal.is_some() {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(FO_EFROZEN);
        }
        if msg.is_empty() {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(FO_ELINE);
        }
        if msg.len() > LINE_MAX {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(FO_EOVER);
        }
        if self.fd == 0 {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(FO_ENOENT);
        }
        let pid = match self.current {
            Some(p) => p,
            None => {
                self.rejects = self.rejects.wrapping_add(1);
                return Err(FO_ENOENT);
            }
        };
        let k = match self.index_of(pid) {
            Some(k) => k,
            None => {
                self.rejects = self.rejects.wrapping_add(1);
                return Err(FO_ENOENT);
            }
        };
        let n = (msg.len() as u32).wrapping_add(1);
        if let Some(f) = self.files.get_mut(k) {
            f.lines.push(msg.to_string());
            f.bytes = f.bytes.wrapping_add(n);
        }
        self.writes = self.writes.wrapping_add(1);
        self.lines_total = self.lines_total.wrapping_add(1);
        self.bytes_total = self.bytes_total.wrapping_add(n);
        Ok(n)
    }

    pub fn close(&mut self) -> Result<(), u32> {
        if self.seal.is_some() {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(FO_EFROZEN);
        }
        if self.fd == 0 {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(FO_ENOENT);
        }
        self.fd = 0;
        self.closes = self.closes.wrapping_add(1);
        Ok(())
    }

    /// fd2 镜像：同一消息序列在 fd2 通道上的逐行形态。
    pub fn fd2_mirror(msgs: &[&str]) -> Vec<String> {
        let mut out = Vec::new();
        let mut k = 0usize;
        while k < msgs.len() {
            if let Some(m) = msgs.get(k) {
                out.push((*m).to_string());
            }
            k = k.wrapping_add(1);
        }
        out
    }

    pub fn fold(&self) -> u64 {
        let mut h = fold_str(FNV_OFF, &self.base);
        h = fold_byte(h, self.fd as u8);
        let cur = match self.current {
            Some(p) => p,
            None => 0,
        };
        h = fold_u32(h, cur);
        h = fold_u32(h, self.opens);
        h = fold_u32(h, self.appends);
        h = fold_u32(h, self.writes);
        h = fold_u32(h, self.closes);
        h = fold_u32(h, self.lines_total);
        h = fold_u32(h, self.bytes_total);
        let mut k = 0usize;
        while k < self.files.len() {
            if let Some(f) = self.files.get(k) {
                h = fold_u32(h, f.pid);
                h = fold_str(h, &f.name);
                h = fold_u32(h, f.bytes);
                h = fold_u32(h, f.opens);
                h = fold_u32(h, f.appends);
                let mut j = 0usize;
                while j < f.lines.len() {
                    if let Some(line) = f.lines.get(j) {
                        h = fold_str(h, line);
                        h = fold_byte(h, b'\n');
                    }
                    j = j.wrapping_add(1);
                }
            }
            k = k.wrapping_add(1);
        }
        h
    }

    /// 幂等冻结。
    pub fn freeze(&mut self) -> Option<u64> {
        if let Some(v) = self.seal {
            return Some(v);
        }
        let v = self.fold();
        self.seal = Some(v);
        Some(v)
    }
}

// ---------------------------------------------------------------- 基准案例
pub const PARENT_PID: u32 = 4242;
pub const CHILD_PID: u32 = 4243;
pub const DEBUG_BASE: &str = "/var/log/ld-debug";

pub static PARENT_MSGS: [&str; 6] = [
    "     4242:     bind: /usr/bin/probe [0]",
    "     4242:     bind: /lib/x86_64-linux-gnu/libc.so.6 [0]",
    "     4242:     init: /usr/bin/probe",
    "     4242:     init: /lib/x86_64-linux-gnu/libc.so.6",
    "     4242:     file=/usr/bin/probe;  generating link map",
    "     4242:     calling init: /lib/x86_64-linux-gnu/libc.so.6",
];

pub static CHILD_MSGS: [&str; 1] = ["     4243:     bind: /usr/bin/helper [0]"];

pub struct OutputCase {
    pub ch: FileChannel,
    pub seal: Option<u64>,
}

pub fn build_output_case() -> OutputCase {
    let mut ch = FileChannel::new(DEBUG_BASE);
    let _ = ch.open(PARENT_PID, true);
    let mut k = 0usize;
    while k < 4 {
        if let Some(m) = PARENT_MSGS.get(k) {
            let _ = ch.write(m);
        }
        k = k.wrapping_add(1);
    }
    let _ = ch.close();
    let _ = ch.open(PARENT_PID, true);
    while k < PARENT_MSGS.len() {
        if let Some(m) = PARENT_MSGS.get(k) {
            let _ = ch.write(m);
        }
        k = k.wrapping_add(1);
    }
    let _ = ch.close();
    let _ = ch.open(CHILD_PID, true);
    if let Some(m) = CHILD_MSGS.first() {
        let _ = ch.write(m);
    }
    let _ = ch.close();
    let seal = ch.freeze();
    OutputCase { ch, seal }
}

/// 四路对抗：零 pid、空基名、截断请求、未打开写入。
pub fn inject_four() -> [bool; 4] {
    let mut hits = [false; 4];

    let mut a = FileChannel::new(DEBUG_BASE);
    hits[0] = a.open(0, true) == Err(FO_EPID);

    let mut b = FileChannel::new("");
    hits[1] = b.open(PARENT_PID, true) == Err(FO_ENAME);

    let mut c = FileChannel::new(DEBUG_BASE);
    hits[2] = c.open(PARENT_PID, false) == Err(FO_EAPPEND);

    let mut d = FileChannel::new(DEBUG_BASE);
    hits[3] = d.write("orphan") == Err(FO_ENOENT);

    hits
}

// ---------------------------------------------------------------- 判据
pub fn long_line(n: usize) -> String {
    let mut s = String::new();
    let mut k = 0usize;
    while k < n {
        s.push('x');
        k = k.wrapping_add(1);
    }
    s
}

pub fn run_unx11296_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11296");
    let mut case = build_output_case();

    // J1 文件名带 pid
    let pname = case.ch.name_for(PARENT_PID);
    let cname = case.ch.name_for(CHILD_PID);
    let pfile = case.ch.file(PARENT_PID);
    let cfile = case.ch.file(CHILD_PID);
    s.add(
        "J1-文件名带pid",
        pname == "/var/log/ld-debug.4242"
            && cname == "/var/log/ld-debug.4243"
            && pfile.map(|f| f.name.as_str()) == Some("/var/log/ld-debug.4242")
            && cfile.map(|f| f.name.as_str()) == Some("/var/log/ld-debug.4243"),
        "两份文件分别以 .4242 / .4243 结尾",
    );

    // J2 文件通道总账
    s.add(
        "J2-文件通道总账",
        case.ch.files_len() == 2
            && case.ch.opens == 3
            && case.ch.appends == 1
            && case.ch.writes == 7
            && case.ch.closes == 3
            && case.ch.lines_total == 7
            && case.ch.fd() == 0,
        "2 文件 / 3 次打开 / 1 次追加 / 7 行",
    );

    // J3 追加不截断
    let six = match case.ch.file(PARENT_PID) {
        Some(f) => f.line_count() == 6 && f.opens == 2 && f.appends == 1,
        None => false,
    };
    let first = match case.ch.file(PARENT_PID) {
        Some(f) => f.lines.first().map(|x| x.as_str()),
        None => None,
    };
    let last = match case.ch.file(PARENT_PID) {
        Some(f) => f.lines.last().map(|x| x.as_str()),
        None => None,
    };
    s.add(
        "J3-追加不截断",
        six
            && first == Some("     4242:     bind: /usr/bin/probe [0]")
            && last == Some("     4242:     calling init: /lib/x86_64-linux-gnu/libc.so.6"),
        "重开同 pid 保留前四行并追加后两行",
    );

    // J4 与 fd2 内容一致
    let mirror = FileChannel::fd2_mirror(&PARENT_MSGS);
    let expect_bytes = PARENT_MSGS
        .iter()
        .fold(0u32, |a, m| a.wrapping_add(m.len() as u32).wrapping_add(1));
    let same = match case.ch.file(PARENT_PID) {
        Some(f) => f.lines == mirror && f.bytes == expect_bytes,
        None => false,
    };
    s.add(
        "J4-与fd2一致",
        same && expect_bytes > 0,
        "文件正文与 fd2 镜像逐行相同，字节数吻合",
    );

    // J5 空行拒
    let mut a = FileChannel::new(DEBUG_BASE);
    let _ = a.open(PARENT_PID, true);
    let empty = a.write("");
    s.add(
        "J5-空行拒",
        empty == Err(FO_ELINE)
            && a.lines_total == 0
            && a.rejects == 1
            && a.file(PARENT_PID).map(|f| f.line_count()) == Some(0),
        "空行不入文件，拒绝计数加一",
    );

    // J6 超长拒与边界
    let mut b = FileChannel::new(DEBUG_BASE);
    let _ = b.open(PARENT_PID, true);
    let over = b.write(long_line(LINE_MAX.wrapping_add(1)).as_str());
    let edge = b.write(long_line(LINE_MAX).as_str());
    s.add(
        "J6-超长拒",
        over == Err(FO_EOVER) && edge == Ok(LINE_MAX as u32 + 1) && b.lines_total == 1,
        "201 字节拒，200 字节收",
    );

    // J7 零 pid 拒
    let mut c = FileChannel::new(DEBUG_BASE);
    s.add(
        "J7-零pid拒",
        c.open(0, true) == Err(FO_EPID) && c.files_len() == 0 && c.fd() == 0,
        "pid 0 无法命名，拒且不建文件",
    );

    // J8 空基名拒
    let mut d = FileChannel::new("");
    s.add(
        "J8-空基名拒",
        d.open(PARENT_PID, true) == Err(FO_ENAME) && d.rejects == 1 && d.files_len() == 0,
        "空基名拒，不产生 .4242 残名",
    );

    // J9 截断请求拒
    let mut e = FileChannel::new(DEBUG_BASE);
    s.add(
        "J9-截断请求拒",
        e.open(PARENT_PID, false) == Err(FO_EAPPEND) && e.files_len() == 0 && e.fd() == 0,
        "只追加策略：截断打开一律拒",
    );

    // J10 未打开写入
    let mut f = FileChannel::new(DEBUG_BASE);
    s.add(
        "J10-未打开写入",
        f.write("orphan") == Err(FO_ENOENT) && f.writes == 0 && f.lines_total == 0,
        "无文件句柄不落账",
    );

    // J11 关闭与续写
    let mut g = FileChannel::new(DEBUG_BASE);
    let _ = g.open(PARENT_PID, true);
    let _ = g.write("     4242:     first");
    let closed = g.close();
    let twice = g.close();
    let orphan = g.write("     4242:     lost");
    let reopened = g.open(PARENT_PID, true);
    let again = g.write("     4242:     second");
    let gl = g.file(PARENT_PID).map(|x| x.line_count());
    s.add(
        "J11-关闭与续写",
        closed.is_ok()
            && twice == Err(FO_ENOENT)
            && orphan == Err(FO_ENOENT)
            && reopened == Ok(3)
            && again == Ok(22)
            && g.closes == 1
            && g.opens == 2
            && g.appends == 1
            && gl == Some(2),
        "关后写拒；重开追加，旧行保留",
    );

    // J12 不同 pid 隔离
    let parent_lines = match case.ch.file(PARENT_PID) {
        Some(f) => f.line_count(),
        None => 0,
    };
    let child_lines = match case.ch.file(CHILD_PID) {
        Some(f) => f.line_count(),
        None => 0,
    };
    let child_first = match case.ch.file(CHILD_PID) {
        Some(f) => f.lines.first().map(|x| x.as_str()),
        None => None,
    };
    s.add(
        "J12-pid隔离",
        parent_lines == 6 && child_lines == 1 && child_first == CHILD_MSGS.first().copied(),
        "父子进程各一份文件，行数互不影响",
    );

    // J13 冻结拒变更
    let before = case.ch.rejects;
    let late_open = case.ch.open(PARENT_PID, true);
    let late_write = case.ch.write("late");
    s.add(
        "J13-冻结拒变更",
        late_open == Err(FO_EFROZEN)
            && late_write == Err(FO_EFROZEN)
            && case.ch.rejects == before.wrapping_add(2)
            && case.ch.lines_total == 7,
        "冻结后打开与写入均拒，行数不变",
    );

    // J14 冻结幂等
    let again = case.ch.freeze();
    s.add(
        "J14-冻结幂等",
        again.is_some() && again == case.seal && case.ch.is_frozen(),
        "重复冻结取回同一指纹",
    );

    // J15 注入四拒
    let hits = inject_four();
    s.add(
        "J15-注入四拒",
        hits.iter().all(|h| *h),
        "零pid/空基名/截断/未开写四路全拒",
    );

    // J16 文件通道明细
    let parent_bytes = match case.ch.file(PARENT_PID) {
        Some(f) => f.bytes,
        None => 0,
    };
    s.add_dyn(
        "J16-文件通道明细",
        case.ch.files_len() == 2 && parent_bytes > 0,
        &format!(
            "base={} files={} open={} app={} writes={} lines={} bytes={} pbytes={}",
            case.ch.base(),
            case.ch.files_len(),
            case.ch.opens,
            case.ch.appends,
            case.ch.writes,
            case.ch.lines_total,
            case.ch.bytes_total,
            parent_bytes
        ),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11296_output_flow() {
        let mut case = build_output_case();
        assert_eq!(case.ch.files_len(), 2);
        assert_eq!(case.ch.opens, 3);
        assert_eq!(case.ch.appends, 1);
        assert_eq!(case.ch.writes, 7);
        assert_eq!(case.ch.lines_total, 7);
        assert_eq!(case.ch.name_for(PARENT_PID), "/var/log/ld-debug.4242");
        assert_eq!(case.ch.name_for(CHILD_PID), "/var/log/ld-debug.4243");
        assert!(case.ch.freeze() == case.seal);
        assert_eq!(case.ch.open(PARENT_PID, true).err(), Some(FO_EFROZEN));

        let parent = match case.ch.file(PARENT_PID) {
            Some(f) => f,
            None => return,
        };
        assert_eq!(parent.line_count(), 6);
        assert_eq!(parent.opens, 2);
        assert_eq!(parent.lines, FileChannel::fd2_mirror(&PARENT_MSGS));

        let mut a = FileChannel::new(DEBUG_BASE);
        assert_eq!(a.write("x"), Err(FO_ENOENT));
        assert_eq!(a.open(0, true), Err(FO_EPID));
        assert_eq!(a.open(PARENT_PID, false), Err(FO_EAPPEND));
        assert_eq!(a.open(PARENT_PID, true), Ok(3));
        assert_eq!(a.write("hello"), Ok(6));
        assert_eq!(a.close(), Ok(()));
        assert_eq!(a.close(), Err(FO_ENOENT));

        assert!(inject_four().iter().all(|h| *h));
    }

    #[test]
    fn unx11296_checks_green() {
        let s = run_unx11296_checks();
        assert!(s.all_passed(), "UNX-F11296 判据未全绿");
    }
}
