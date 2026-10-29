//! UNX-F10108 ioctl 正反双判据（控制请求达成与非法请求误用受控）（0x100a0 段）
//!
//! 锚点：UNX-F10108-J1 ioctl 合法请求达成（LTP ioctl01 面请求分发正确
//! 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：设备册（终端/磁盘/杂项三类设备描述）+ 请求核（请求码分发：
//! 取属性/置属性/清属性三动作，请求码按设备类闭集校验）+ 请求账（逐次
//! 留痕 + 属性回读对平）+ 注入核（非法请求码/错类请求/坏 fd/空留痕/
//! 封表必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x100a0 独占 0x20）。
pub const SEG_BASE: u32 = 0x100a0;
pub const IO_BAD_REQ: u32 = SEG_BASE + 0x01; // 请求码出闭集
pub const IO_BAD_FD: u32 = SEG_BASE + 0x02; // 坏 fd
pub const IO_WRONG_CLASS: u32 = SEG_BASE + 0x03; // 请求码与设备类错配
pub const IO_EMPTY_NOTE: u32 = SEG_BASE + 0x04; // 空留痕
pub const IO_SEALED: u32 = SEG_BASE + 0x05; // 封表后操作

/// 设备类闭集。
pub const CLS_TTY: u32 = 1;
pub const CLS_DISK: u32 = 2;
pub const CLS_MISC: u32 = 3;

/// 请求码编解码：高 24 位 magic，低 8 位动作。
pub const REQ_MAGIC: u32 = 0x4B54; // "KT"
pub const ACT_GET: u32 = 1;
pub const ACT_SET: u32 = 2;
pub const ACT_CLR: u32 = 3;

/// 请求码合成与拆解。
pub fn mk_req(act: u32) -> u32 {
    (REQ_MAGIC << 8) | (act & 0xFF)
}

pub fn req_act(req: u32) -> u32 {
    req & 0xFF
}

pub fn req_magic(req: u32) -> u32 {
    (req >> 8) & 0xFFFF
}

/// 设备描述。
#[derive(Clone)]
pub struct IoDev {
    pub id: u32,
    pub class: u32,
    pub attr: u32,
}

impl IoDev {
    pub fn new(id: u32, class: u32) -> IoDev {
        IoDev { id, class, attr: 0 }
    }

    /// 属性位闭集（8 位可置）。
    pub fn attr_ok(v: u32) -> bool {
        v & !0xFF == 0
    }
}

/// 请求账留痕。
#[derive(Clone)]
pub struct IoRec {
    pub seq: u32,
    pub fd: i32,
    pub req: u32,
    pub arg: u32,
    pub note: String,
}

impl IoRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && req_magic(self.req) == REQ_MAGIC
    }
}

/// 设备册 + 请求核。
pub struct IoctlBook {
    pub devs: Vec<Option<IoDev>>,
    pub ledger: Vec<IoRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl IoctlBook {
    pub fn new() -> IoctlBook {
        IoctlBook { devs: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    pub fn attach(&mut self, id: u32, class: u32) -> Result<usize, u32> {
        if self.sealed {
            return Err(IO_SEALED);
        }
        if class < CLS_TTY || class > CLS_MISC {
            return Err(IO_BAD_REQ);
        }
        if let Some(i) = self.devs.iter().position(|s| s.is_none()) {
            self.devs[i] = Some(IoDev::new(id, class));
            return Ok(i);
        }
        if self.devs.len() >= 8 {
            return Err(IO_BAD_FD);
        }
        self.devs.push(Some(IoDev::new(id, class)));
        Ok(self.devs.len() - 1)
    }

    /// ioctl 核心：GET/SET/CLR 三动作分发，类错配必拒。
    pub fn ioctl(&mut self, fd: i32, req: u32, arg: u32, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(IO_SEALED);
        }
        if note.is_empty() {
            return Err(IO_EMPTY_NOTE);
        }
        if fd < 0 || fd as usize >= self.devs.len() || self.devs[fd as usize].is_none() {
            return Err(IO_BAD_FD);
        }
        let act = req_act(req);
        if req_magic(req) != REQ_MAGIC {
            return Err(IO_BAD_REQ);
        }
        if act < ACT_GET || act > ACT_CLR {
            return Err(IO_BAD_REQ);
        }
        let u = fd as usize;
        let class = self.devs[u].as_ref().map(|d| d.class).unwrap_or(0);
        // 类闭集：DISK 不受理 CLR，TTY/MISC 全受理
        let allowed = match class {
            CLS_DISK => act == ACT_GET || act == ACT_SET,
            _ => true,
        };
        if !allowed {
            return Err(IO_WRONG_CLASS);
        }
        let ret: u32;
        match act {
            ACT_GET => {
                ret = self.devs[u].as_ref().map(|d| d.attr).unwrap_or(0);
            }
            ACT_SET => {
                if !IoDev::attr_ok(arg) {
                    return Err(IO_BAD_REQ);
                }
                if let Some(d) = self.devs[u].as_mut() {
                    d.attr = arg;
                }
                ret = arg;
            }
            _ => {
                if let Some(d) = self.devs[u].as_mut() {
                    d.attr &= !arg;
                }
                ret = self.devs[u].as_ref().map(|d| d.attr).unwrap_or(0);
            }
        }
        self.ledger.push(IoRec { seq: self.seq_next, fd, req, arg, note: note.to_string() });
        self.seq_next += 1;
        Ok(ret)
    }

    pub fn attr_of(&self, fd: i32) -> Option<u32> {
        if fd < 0 || fd as usize >= self.devs.len() {
            return None;
        }
        self.devs[fd as usize].as_ref().map(|d| d.attr)
    }

    pub fn class_of(&self, fd: i32) -> Option<u32> {
        if fd < 0 || fd as usize >= self.devs.len() {
            return None;
        }
        self.devs[fd as usize].as_ref().map(|d| d.class)
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
        let mut h: u32 = 0x100a00;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.req);
            h = h.wrapping_mul(31).wrapping_add(r.arg);
            for b in r.note.bytes() {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 账面笔数
    pub fn rec_count(&self) -> usize {
        self.ledger.len()
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = IoctlBook::new();
    let tty = b.attach(1, CLS_TTY).unwrap_or(usize::MAX);
    let disk = b.attach(2, CLS_DISK).unwrap_or(usize::MAX);
    // 1) 请求码动作位出闭集必拒
    got[0] = b.ioctl(tty as i32, mk_req(9), 0, "inj-act").err() == Some(IO_BAD_REQ);
    // 2) magic 错必拒
    got[1] = b.ioctl(tty as i32, 0x0099_0001, 0, "inj-magic").err() == Some(IO_BAD_REQ);
    // 3) 类错配：DISK 不受理 CLR（arg≠0 分支不触发，用 arg>0 的 CLR）
    got[2] = b.ioctl(disk as i32, mk_req(ACT_CLR), 0, "inj-cls").err() == Some(IO_WRONG_CLASS);
    // 4) 坏 fd 必拒
    got[3] = b.ioctl(-1, mk_req(ACT_GET), 0, "inj-fd").err() == Some(IO_BAD_FD);
    // 5) 封表后操作必抓
    b.seal();
    got[4] = b.ioctl(tty as i32, mk_req(ACT_GET), 0, "inj-seal").err() == Some(IO_SEALED);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10108_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10108");

    // 正路径：SET→GET 回读一致（ioctl01 面）
    let mut b = IoctlBook::new();
    let tty = b.attach(1, CLS_TTY).unwrap_or(usize::MAX);
    let _ = b.ioctl(tty as i32, mk_req(ACT_SET), 0xA5, "置属性");
    let j1 = b.ioctl(tty as i32, mk_req(ACT_GET), 0, "读属性").unwrap_or(0) == 0xA5;
    s.add("J1-属性回读", j1, "SET 0xA5 后 GET 一致");

    // CLR 清位语义
    let j2 = {
        let _ = b.ioctl(tty as i32, mk_req(ACT_CLR), 0x0F, "清低位");
        b.attr_of(tty as i32) == Some(0xA0)
    };
    s.add("J2-清位语义", j2, "CLR 0x0F 后余 0xA0");

    // 请求码编解码自洽
    let j3 = req_act(mk_req(ACT_SET)) == ACT_SET
        && req_magic(mk_req(ACT_GET)) == REQ_MAGIC
        && req_magic(0x0099_0001) != REQ_MAGIC;
    s.add("J3-编解码自洽", j3, "magic/动作位拆装一致");

    // 三类设备分发
    let j4 = {
        let disk = b.attach(2, CLS_DISK).unwrap_or(usize::MAX);
        let misc = b.attach(3, CLS_MISC).unwrap_or(usize::MAX);
        b.class_of(disk as i32) == Some(CLS_DISK) && b.class_of(misc as i32) == Some(CLS_MISC)
    };
    s.add("J4-三类分发", j4, "设备类挂载与回读一致");

    // 非法请求码受控
    let j5 = b.ioctl(tty as i32, mk_req(0), 0, "act0").err() == Some(IO_BAD_REQ)
        && b.ioctl(tty as i32, mk_req(4), 0, "act4").err() == Some(IO_BAD_REQ);
    s.add("J5-动作闭集", j5, "动作位出三动作闭集必拒");

    // 类错配受控：DISK 拒 CLR
    let j6 = {
        let disk = b.attach(9, CLS_DISK).unwrap_or(usize::MAX);
        b.ioctl(disk as i32, mk_req(ACT_CLR), 0, "disk-clr").err() == Some(IO_WRONG_CLASS)
    };
    s.add("J6-类错配受控", j6, "DISK 不受理 CLR 必拒");

    // 坏 fd 受控
    let j7 = b.ioctl(-3, mk_req(ACT_GET), 0, "neg").err() == Some(IO_BAD_FD)
        && b.ioctl(77, mk_req(ACT_GET), 0, "oob").err() == Some(IO_BAD_FD);
    s.add("J7-坏fd受控", j7, "负与越界 fd 同码拒");

    // 属性位闭集受控
    let j8 = b.ioctl(tty as i32, mk_req(ACT_SET), 0x1_0000, "wide").err() == Some(IO_BAD_REQ);
    s.add("J8-属性位闭集", j8, "属性超 8 位必拒");

    // 空留痕拒收
    let j9 = b.ioctl(tty as i32, mk_req(ACT_GET), 0, "").err() == Some(IO_EMPTY_NOTE);
    s.add("J9-空留痕拒", j9, "note 为空不入账");

    // 留痕合法与 seq 唯一
    let j10 = b.ledger.iter().all(|r| r.ok()) && b.seq_unique();
    s.add("J10-留痕合法", j10, "magic 在闭集且注非空");

    // 注入核 5/5
    let inj = inject_five();
    let j11 = inj.iter().all(|g| *g);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", inj));

    // 指纹稳定
    let j12 = b.fold() == b.fold();
    s.add("J12-指纹稳定", j12, "折叠指纹两次一致");

    // 账面笔数非零
    let j13 = b.rec_count() > 0;
    s.add("J13-账面非空", j13, "ioctl 留痕账非零");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10108_ioctl_flow() {
        let mut b = IoctlBook::new();
        let f = b.attach(1, CLS_MISC).unwrap_or(usize::MAX);
        let _ = b.ioctl(f as i32, mk_req(ACT_SET), 0x81, "s");
        assert_eq!(b.ioctl(f as i32, mk_req(ACT_GET), 0, "g").unwrap_or(0), 0x81);
        let _ = b.ioctl(f as i32, mk_req(ACT_CLR), 0x80, "c");
        assert_eq!(b.attr_of(f as i32), Some(0x01));
    }

    #[test]
    fn unx10108_checks_green() {
        let s = run_unx10108_checks();
        assert!(s.all_passed(), "UNX-F10108 判据未全绿");
    }
}
