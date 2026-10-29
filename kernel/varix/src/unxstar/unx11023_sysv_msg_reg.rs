//! UNX-F11023 SysV msg 挂号联签（msgsnd/msgrcv 挂点账对账）（0x1e2e0 段）
//!
//! 锚点：UNX-F11023-J1 收发挂点千次账对平（10/10），队列驻留账守恒（10/10）。
//! 域：CoRun·UNX-C4-B32（五族挂号联签域·SysV msg）。
//!
//! 结构：消息队列账（类型 + 长度驻留表）+ 发送挂点（超容必拒、字节入账）
//! + 接收挂点（按类型首匹配出队、字节出账）+ 驻留守恒（入字节 = 出字节 +
//! 驻留字节）+ fold 指纹与冻结 + 注入核（空队收报/超容发报/无此类型/冻结
//! 后写四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1e2e0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1e2e0;
pub const MQ_EMPTY: u32 = SEG_BASE + 0x01; // 空队收报
pub const MQ_NOVAR: u32 = SEG_BASE + 0x02; // 无此类型
pub const MQ_FULL: u32 = SEG_BASE + 0x03; // 超容发报
pub const MQ_ZLEN: u32 = SEG_BASE + 0x04; // 零长报文
pub const MQ_FROZEN: u32 = SEG_BASE + 0x05; // 冻结后写账
pub const MQ_DRAIN: u32 = SEG_BASE + 0x06; // 收发类型失配

/// 队列消息定容。
pub const QUEUE_CAP: usize = 8;
/// 千次收发轮数。
pub const MSG_ROUNDS: u32 = 1000;
/// msg 账域标（fold 种子）。
pub const MSG_TAG: u64 = 0x1e2e_23;
/// 类型空间。
pub const MTYPES: u32 = 4;

/// 驻留报文：类型 + 长度。
#[derive(Clone, Debug)]
pub struct Msg {
    pub mtype: u32,
    pub len: u32,
}

/// SysV msg 挂号账：发送入队、按类型接收出队、驻留字节守恒。
pub struct MsgBook {
    pub q: Vec<Msg>,
    pub sent: u32,
    pub recvd: u32,
    pub bytes_in: u32,
    pub bytes_out: u32,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl MsgBook {
    pub fn new() -> MsgBook {
        MsgBook {
            q: Vec::new(),
            sent: 0,
            recvd: 0,
            bytes_in: 0,
            bytes_out: 0,
            frozen: false,
            root: None,
        }
    }

    /// 发送挂点：零长必拒、超容必拒；字节入账。
    pub fn send(&mut self, mtype: u32, len: u32) -> Result<u32, u32> {
        if self.frozen {
            return Err(MQ_FROZEN);
        }
        if len == 0 {
            return Err(MQ_ZLEN);
        }
        if self.q.len() >= QUEUE_CAP {
            return Err(MQ_FULL);
        }
        self.q.push(Msg { mtype, len });
        self.sent = self.sent.wrapping_add(1);
        self.bytes_in = self.bytes_in.wrapping_add(len);
        Ok(self.bytes_in)
    }

    /// 接收挂点：按类型首匹配出队；空队与无此类型必拒。
    pub fn recv(&mut self, mtype: u32) -> Result<u32, u32> {
        if self.frozen {
            return Err(MQ_FROZEN);
        }
        if self.q.is_empty() {
            return Err(MQ_EMPTY);
        }
        let mut k = 0usize;
        while k < self.q.len() {
            if self.q[k].mtype == mtype {
                let len = self.q[k].len;
                self.q.remove(k);
                self.recvd = self.recvd.wrapping_add(1);
                self.bytes_out = self.bytes_out.wrapping_add(len);
                return Ok(len);
            }
            k += 1;
        }
        Err(MQ_NOVAR)
    }

    /// 驻留字节数。
    pub fn resident_bytes(&self) -> u32 {
        let mut n = 0u32;
        let mut k = 0usize;
        while k < self.q.len() {
            n = n.wrapping_add(self.q[k].len);
            k += 1;
        }
        n
    }

    /// 驻留守恒：入字节 = 出字节 + 驻留字节。
    pub fn conserved(&self) -> bool {
        self.bytes_in == self.bytes_out.wrapping_add(self.resident_bytes())
    }

    /// 千次对平：收发计数齐平且队列清空。
    pub fn balanced(&self, rounds: u32) -> bool {
        self.sent == rounds && self.recvd == rounds && self.q.is_empty()
    }

    /// 按类型统计驻留条数。
    pub fn resident_of(&self, mtype: u32) -> u32 {
        let mut n = 0u32;
        let mut k = 0usize;
        while k < self.q.len() {
            if self.q[k].mtype == mtype {
                n += 1;
            }
            k += 1;
        }
        n
    }

    /// 链式 fold 指纹（FNV 风格，序敏感，含驻留与计数）。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = MSG_TAG;
        let mut k = 0usize;
        while k < self.q.len() {
            h = (h ^ self.q[k].mtype as u64).wrapping_mul(0x100000001b3);
            h = (h ^ self.q[k].len as u64).wrapping_mul(0x100000001b3);
            k += 1;
        }
        h = (h ^ self.sent as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.recvd as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.bytes_in as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.bytes_out as u64).wrapping_mul(0x100000001b3);
        h
    }

    /// 冻结：翻冻结位并落冻结根。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }
}

/// 类型确定性样本。
pub fn mq_type(no: u32) -> u32 {
    no % MTYPES + 1
}

/// 长度确定性样本。
pub fn mq_len(no: u32) -> u32 {
    (no % 13 + 5) as u32
}

/// 千次收发全链：逐轮按类型发送并按同类型接收（队列即时清空）。
pub fn build_msgbook() -> (MsgBook, bool) {
    let mut b = MsgBook::new();
    let mut ok = true;
    let mut i = 0u32;
    while i < MSG_ROUNDS {
        let t = mq_type(i);
        let l = mq_len(i);
        if b.send(t, l).is_err() {
            ok = false;
        }
        if b.recv(t).is_err() {
            ok = false;
        }
        i += 1;
    }
    let b_ok = ok && b.balanced(MSG_ROUNDS) && b.conserved();
    (b, b_ok)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 空队收报必拒
    let mut a = MsgBook::new();
    got[0] = a.recv(1).err() == Some(MQ_EMPTY);
    // 2) 无此类型必拒且不损账
    let mut b = MsgBook::new();
    let _ = b.send(1, 16);
    let before = b.fold();
    let miss = b.recv(2).err() == Some(MQ_NOVAR);
    got[1] = miss && b.fold() == before;
    // 3) 超容发报必拒：第 QUEUE_CAP+1 条必拒
    let mut c = MsgBook::new();
    let mut filled = true;
    let mut n = 0u32;
    while (n as usize) < QUEUE_CAP {
        if c.send(1, 8).is_err() {
            filled = false;
        }
        n += 1;
    }
    got[2] = filled && c.send(1, 8).err() == Some(MQ_FULL);
    // 4) 冻结后写账必抓
    let (mut d, _) = build_msgbook();
    d.freeze();
    got[3] = d.send(1, 8).err() == Some(MQ_FROZEN) && d.recv(1).err() == Some(MQ_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11023_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11023");

    // J1 收发挂点千次账对平
    let (mut book, green) = build_msgbook();
    s.add("J1-千次账对平", green && book.sent == MSG_ROUNDS && book.recvd == MSG_ROUNDS, "千轮收发计数齐平");

    // J2 队列驻留账守恒：入字节 = 出字节 + 驻留
    s.add("J2-驻留守恒", book.conserved() && book.q.is_empty() && book.bytes_in > 0, "字节账入出驻留三面守恒");

    // J3 冻结根：fold 稳定且与根一致
    let root = book.freeze();
    s.add("J3-冻结根", book.root == Some(root) && book.fold() == root && root != 0, "冻结根落账且指纹稳定");

    // J4 冻结后写账必抓
    let j4 = book.send(1, 8).err() == Some(MQ_FROZEN) && book.recv(1).err() == Some(MQ_FROZEN);
    s.add("J4-冻结拒写", j4, "冻结后收发均必拒");

    // J5 注入检出：空队收报 + 无此类型
    let inj = inject_four();
    s.add("J5-空队无型抓", inj[0] && inj[1], "空队收报与无此类型两路必抓");

    // J6 注入检出：超容发报 + 冻结后写
    s.add("J6-超容冻结检出", inj[2] && inj[3], "超容发报与冻结后写必抓");

    // J7 先进先出：同类型多条按入队序出队
    let j7 = {
        let mut c = MsgBook::new();
        let _ = c.send(3, 10);
        let _ = c.send(3, 20);
        let _ = c.send(3, 30);
        let first = c.recv(3);
        let resid = c.resident_of(3);
        first == Ok(10) && resid == 2 && c.conserved()
    };
    s.add("J7-先进先出", j7, "同类型按入队序出队");

    // J8 类型选择：不同类型互不误取
    let j8 = {
        let mut c = MsgBook::new();
        let _ = c.send(1, 12);
        let _ = c.send(2, 24);
        let got1 = c.recv(2);
        got1 == Ok(24) && c.resident_of(1) == 1 && c.resident_of(2) == 0
    };
    s.add("J8-类型选择", j8, "按类型取报不误取他型");

    // J9 字节账分面：入出与驻留逐面核对
    let j9 = {
        let mut c = MsgBook::new();
        let _ = c.send(1, 40);
        let _ = c.send(2, 60);
        let _ = c.recv(1);
        c.bytes_in == 100 && c.bytes_out == 40 && c.resident_bytes() == 60 && c.conserved()
    };
    s.add("J9-字节分面", j9, "入出驻留三面逐面核对");

    // J10 零长报文拒
    let j10 = MsgBook::new().send(1, 0).err() == Some(MQ_ZLEN);
    s.add("J10-零长拒", j10, "零长报文必拒");

    // J11 接收清空后空队：清空后再收必拒
    let j11 = {
        let mut c = MsgBook::new();
        let _ = c.send(1, 8);
        let _ = c.recv(1);
        c.recv(1).err() == Some(MQ_EMPTY)
    };
    s.add("J11-清空后拒", j11, "队列清空后接收必拒");

    // J12 混型驻留：多类型混驻守恒不断
    let j12 = {
        let mut c = MsgBook::new();
        let mut i = 0u32;
        let mut ok = true;
        while i < 6 {
            if c.send(mq_type(i), mq_len(i)).is_err() {
                ok = false;
            }
            i += 1;
        }
        let _ = c.recv(mq_type(0));
        let _ = c.recv(mq_type(3));
        ok && c.conserved() && c.q.len() == 4
    };
    s.add("J12-混型驻留", j12, "多类型混驻守恒不断");

    // J13 长度样本确定性：同轮同长
    let l3a = mq_len(3);
    let l3b = mq_len(3);
    let t1a = mq_type(1);
    let t1b = mq_type(1);
    s.add("J13-样本确定性", l3a == l3b && t1a == t1b, "类型与长度样本同入同出");

    // J14 驻留折叠敏感：驻留变化指纹必变
    let j14 = {
        let mut c = MsgBook::new();
        let _ = c.send(1, 8);
        let f1 = c.fold();
        let _ = c.send(2, 9);
        c.fold() != f1
    };
    s.add("J14-折叠敏感", j14, "驻留变化指纹必变");

    // J15 动态明细：千次对平与冻结根落账
    let fp = book.fold();
    s.add_dyn(
        "J15-千次明细",
        fp == root && book.bytes_in == book.bytes_out,
        &format!("root={:#x} in={} out={}", fp, book.bytes_in, book.bytes_out),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11023_msg_reg_flow() {
        let (mut b, built) = build_msgbook();
        assert!(built);
        assert!(b.recv(1).err() == Some(MQ_EMPTY));
        b.freeze();
        assert!(b.send(1, 8).err() == Some(MQ_FROZEN));
        assert!(b.root.is_some());
    }

    #[test]
    fn unx11023_checks_green() {
        let s = run_unx11023_checks();
        assert!(s.all_passed(), "UNX-F11023 判据未全绿");
    }
}
