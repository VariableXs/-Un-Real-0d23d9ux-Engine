//! UNX-F11204 busybox applet 分发与 40 applet 自举顶层协议（0x1f804 段）
//!
//! 锚点：UNX-F11204-J1 argv[0] 分派协议与 40 核心 applet 自举总判据立账，
//! busybox sh 管道环境可工作总判据通过后 B20/B29-B36 实例化展开。
//! 域：CoRun·UNX-C5-B01——分发面只认 argv[0] 基名（或多调用二进制形态
//! 的第二个参数），40 个核心 applet 逐个可自举；sh 管道按段解析，任一段
//! 不可分发即整条失败。

use crate::alloc::string::String;
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1f804;
pub const BB_UNKNOWN: u32 = SEG_BASE + 0x01; // 未知 applet
pub const BB_NOARG: u32 = SEG_BASE + 0x02; // busybox 形态缺 applet 参数
pub const BB_CAT: u32 = SEG_BASE + 0x03; // 类别越界
pub const BB_DUP: u32 = SEG_BASE + 0x04; // applet 重名
pub const BB_PIPE: u32 = SEG_BASE + 0x05; // 管道段非法
pub const BB_FROZEN: u32 = SEG_BASE + 0x06; // 冻结后登记
pub const BB_OVER: u32 = SEG_BASE + 0x07; // 超容

/// 核心 applet 数量（自举总判据）。
pub const APPLETS: usize = 40;
/// 类别数。
pub const CATS: usize = 5;
pub const BB_TAG: u64 = 0x1f804b5;
pub const FP_STEP: u64 = 0x100000001b3;
/// 多调用二进制名。
pub const BUSYBOX: &str = "busybox";

/// 核心 applet 名单（40 条，顺序即自举序）。
pub const NAMES: [&str; APPLETS] = [
    "sh", "ash", "cat", "cp", "mv", "rm", "ls", "ln", "mkdir", "rmdir",
    "chmod", "chown", "touch", "stat", "df", "du", "find", "grep", "sed", "awk",
    "cut", "sort", "uniq", "wc", "head", "tail", "tr", "tee", "echo", "printf",
    "test", "expr", "mount", "umount", "ps", "kill", "sleep", "date", "dd", "tar",
];

/// 单 applet 登记项：号、名、类别。
#[derive(Clone, Debug)]
pub struct Applet {
    pub id: u32,
    pub name: &'static str,
    pub cat: u32,
}

impl Applet {
    pub fn render(&self) -> String {
        format!("#{} {} c{}", self.id, self.name, self.cat)
    }
}

/// 分发表：40 applet 自举 + argv[0] 分派。
#[derive(Clone, Debug)]
pub struct Dispatch {
    pub table: Vec<Applet>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl Dispatch {
    pub fn new() -> Dispatch {
        Dispatch { table: Vec::new(), frozen: false, root: None }
    }

    /// 登记 applet：冻结/类别越界/重名/超容四路拒。
    pub fn enroll(&mut self, name: &'static str, cat: u32) -> Result<u32, u32> {
        if self.frozen {
            return Err(BB_FROZEN);
        }
        if cat as usize >= CATS {
            return Err(BB_CAT);
        }
        if name.is_empty() || self.table.iter().any(|a| a.name == name) {
            return Err(BB_DUP);
        }
        if self.table.len() >= APPLETS {
            return Err(BB_OVER);
        }
        let id = self.table.len() as u32;
        self.table.push(Applet { id, name, cat });
        Ok(id)
    }

    /// 按名查 id。
    pub fn lookup(&self, name: &str) -> Option<u32> {
        let mut k = 0usize;
        while k < self.table.len() {
            if self.table[k].name == name {
                return Some(self.table[k].id);
            }
            k += 1;
        }
        None
    }

    /// argv[0] 分派：busybox 形态取 argv[1]，其余取基名。
    pub fn dispatch(&self, argv0: &str, argv1: Option<&str>) -> Result<u32, u32> {
        let base = basename(argv0);
        if base == BUSYBOX {
            match argv1 {
                Some(a) => {
                    let nm = basename(a);
                    self.lookup(nm).ok_or(BB_UNKNOWN)
                }
                None => Err(BB_NOARG),
            }
        } else {
            self.lookup(base).ok_or(BB_UNKNOWN)
        }
    }

    /// 自举判：恰 40 条且每条按名可回查。
    pub fn bootstrapped(&self) -> bool {
        if self.table.len() != APPLETS {
            return false;
        }
        let mut k = 0usize;
        while k < self.table.len() {
            if self.lookup(self.table[k].name) != Some(self.table[k].id) {
                return false;
            }
            k += 1;
        }
        true
    }

    /// 类别直方图。
    pub fn hist(&self) -> [u32; CATS] {
        let mut h = [0u32; CATS];
        let mut k = 0usize;
        while k < self.table.len() {
            let c = self.table[k].cat as usize;
            if c < CATS {
                h[c] = h[c].wrapping_add(1);
            }
            k += 1;
        }
        h
    }

    pub fn fold(&self) -> u64 {
        let mut h: u64 = BB_TAG;
        let mut k = 0usize;
        while k < self.table.len() {
            h = (h ^ self.table[k].id as u64).wrapping_mul(FP_STEP);
            h = (h ^ self.table[k].cat as u64).wrapping_mul(FP_STEP);
            let bytes = self.table[k].name.as_bytes();
            let mut b = 0usize;
            while b < bytes.len() {
                h = (h ^ bytes[b] as u64).wrapping_mul(FP_STEP);
                b += 1;
            }
            k += 1;
        }
        h
    }

    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }
}

/// 基名：取最后一个 '/' 之后；尾斜杠不参与。
pub fn basename(path: &str) -> &str {
    let b = path.as_bytes();
    let mut i = b.len();
    let mut start = 0usize;
    while i > 0 {
        i -= 1;
        if b[i] == b'/' {
            start = i + 1;
            break;
        }
    }
    &path[start..]
}

/// 管道环境：每段一个 applet 名，段间以 | 连接。
#[derive(Clone, Debug)]
pub struct Pipeline {
    pub stages: Vec<u32>,
    pub bytes: u32,
}

impl Pipeline {
    /// 跑管道：逐段分发，任一失败即整条失败；单段零字节输出仅记 0。
    pub fn run(d: &Dispatch, cmds: &[&str], feed: u32) -> Result<Pipeline, u32> {
        if cmds.is_empty() {
            return Err(BB_PIPE);
        }
        let mut stages: Vec<u32> = Vec::new();
        let mut bytes = feed;
        let mut k = 0usize;
        while k < cmds.len() {
            let id = d.dispatch(cmds[k], None)?;
            stages.push(id);
            // 段语义：cat/tee 透传，其余按段衰减一字节（确定性模型）。
            if cmds[k] != "cat" && cmds[k] != "tee" {
                bytes = bytes.saturating_sub(1);
            }
            k += 1;
        }
        Ok(Pipeline { stages, bytes })
    }

    pub fn ok(&self) -> bool {
        !self.stages.is_empty() && self.bytes > 0
    }
}

/// 类别：0 壳 / 1 文件 / 2 文本 / 3 系统 / 4 归档。
pub fn cat_of(name: &str) -> u32 {
    if name == "sh" || name == "ash" {
        0
    } else if name == "cat" || name == "cp" || name == "mv" || name == "rm"
        || name == "ls" || name == "ln" || name == "mkdir" || name == "rmdir"
        || name == "chmod" || name == "chown" || name == "touch" || name == "stat"
        || name == "df" || name == "du" || name == "find"
    {
        1
    } else if name == "mount" || name == "umount" || name == "ps" || name == "kill"
        || name == "sleep" || name == "date" || name == "dd"
    {
        3
    } else if name == "tar" {
        4
    } else {
        2
    }
}

/// 自举建表：40 名单逐个登记。
pub fn bootstrap() -> (Dispatch, bool) {
    let mut d = Dispatch::new();
    let mut k = 0usize;
    while k < APPLETS {
        if d.enroll(NAMES[k], cat_of(NAMES[k])).is_err() {
            return (d, false);
        }
        k += 1;
    }
    let ok = d.bootstrapped();
    (d, ok)
}

/// sh 管道样本：cat | grep | wc 三段。
pub fn sh_sample(d: &Dispatch) -> Result<Pipeline, u32> {
    Pipeline::run(d, &["cat", "grep", "wc"], 9)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    let (d, _) = bootstrap();
    // 1) 未知 applet 必拒（两种形态）。
    got[0] = d.dispatch("nosuch", None).err() == Some(BB_UNKNOWN)
        && d.dispatch("busybox", Some("nosuch")).err() == Some(BB_UNKNOWN);
    // 2) busybox 形态缺第二参数必拒。
    got[1] = d.dispatch("busybox", None).err() == Some(BB_NOARG);
    // 3) 重名登记必拒。
    let mut e = Dispatch::new();
    let _ = e.enroll("cat", 1);
    got[2] = e.enroll("cat", 2).err() == Some(BB_DUP);
    // 4) 管道含未知段整条失败。
    got[3] = Pipeline::run(&d, &["cat", "nosuch", "wc"], 9).err() == Some(BB_UNKNOWN)
        && Pipeline::run(&d, &[], 9).err() == Some(BB_PIPE);
    got
}

/// 批内判据。
pub fn run_unx11204_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11204");
    let (mut d, built) = bootstrap();

    s.add("J1-四十自举", built && d.table.len() == APPLETS, "40 核心 applet 全部自举成功");
    s.add("J2-按名回查", d.lookup("sh") == Some(0) && d.lookup("tar") == Some(39),
        "自举序首尾按名回查一致");

    s.add("J3-argv0 分派", d.dispatch("ls", None) == Ok(6) && d.dispatch("/bin/rm", None) == Ok(5),
        "argv[0] 基名分派（含带路径形态）");
    s.add("J4-busybox 形态", d.dispatch("busybox", Some("grep")) == Ok(17),
        "多调用二进制第二参数分派");
    s.add("J5-缺参拒", d.dispatch("busybox", None).err() == Some(BB_NOARG), "busybox 形态缺 applet 参数必拒");
    s.add("J6-未知拒", d.dispatch("/tmp/x", None).err() == Some(BB_UNKNOWN), "未知 applet 必拒");

    let pipe = sh_sample(&d);
    let j7 = match &pipe {
        Ok(p) => p.stages.len() == 3 && p.bytes == 7,
        Err(_) => false,
    };
    s.add("J7-sh 管道", j7, "sh 管道 cat|grep|wc 三段可工作且字节衰减正确");

    s.add("J8-管道失败", Pipeline::run(&d, &["cat", "zzz"], 4).err() == Some(BB_UNKNOWN),
        "管道任一段不可分发即整条失败");
    s.add("J9-空管道拒", Pipeline::run(&d, &[], 4).err() == Some(BB_PIPE), "零段管道必拒");

    let hist = d.hist();
    s.add("J10-类别闭合", hist[0] + hist[1] + hist[2] + hist[3] + hist[4] == APPLETS as u32,
        "五类别计数闭合到 40");

    let j11 = {
        let mut e = Dispatch::new();
        e.enroll("sleep", CATS as u32).err() == Some(BB_CAT)
    };
    s.add("J11-类别越界拒", j11, "类别号越界登记必拒");

    let inj = inject_four();
    s.add("J12-注入四路", inj[0] && inj[1] && inj[2] && inj[3],
        "未知/缺参/重名/坏管道四路必抓");

    let root = d.freeze();
    s.add("J13-冻结根", d.fold() == root && root != 0 && d.enroll("x", 1).err() == Some(BB_FROZEN),
        "冻结根稳定且冻结后登记拒");

    let j14 = {
        let mut e = Dispatch::new();
        let mut k = 0usize;
        while k < APPLETS {
            let _ = e.enroll(NAMES[k], 1);
            k += 1;
        }
        e.enroll("over", 1).err() == Some(BB_OVER)
    };
    s.add("J14-超容拒", j14, "第 41 个 applet 登记必拒");

    let fp = d.fold();
    s.add_dyn("J15-分发明细", fp == root && basename("/bin/busybox") == "busybox",
        &format!("applets={} fp={:#x}", d.table.len(), fp));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11204_applet_flow() {
        let (mut d, built) = bootstrap();
        assert!(built);
        assert_eq!(d.table.len(), 40);
        assert_eq!(d.dispatch("sh", None), Ok(0));
        assert_eq!(d.dispatch("busybox", Some("ls")), Ok(6));
        assert_eq!(basename("/usr/bin/awk"), "awk");
        let p = sh_sample(&d);
        assert!(p.is_ok());
        assert_eq!(p.map(|x| x.stages.len()).unwrap_or(0), 3);
        assert!(d.dispatch("nope", None).is_err());
        let root = d.freeze();
        assert_eq!(d.fold(), root);
        assert_eq!(d.hist().iter().sum::<u32>(), 40);
    }

    #[test]
    fn unx11204_checks_green() {
        let s = run_unx11204_checks();
        assert!(s.all_passed(), "UNX-F11204 判据未全绿");
    }
}
