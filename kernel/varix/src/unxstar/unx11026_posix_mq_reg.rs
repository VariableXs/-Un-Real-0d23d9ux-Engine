//! UNX-F11026 POSIX mq 挂号联签（mq_send/receive 挂点账）（0x1e340 段）
//!
//! 锚点：UNX-F11026-J1 收发挂点千次对平（10/10），通知注册账联动（10/10）。
//! 域：CoRun·UNX-C4-B32（五族挂号联签域·POSIX mq）。
//!
//! 结构：具名消息队列账（名 + 驻留表 + 通知注册位）+ 收发挂点（超容/空
//! 队必拒、字节入出账）+ 通知联动器（空转非瞬间注册生效、触发一次即注销）
//! + 联动断言（触发数 = 注册数）+ fold 指纹与冻结 + 注入核（空队收报/重
//! 复注册/超容发报/冻结后写四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1e340 独占 0x20）。
pub const SEG_BASE: u32 = 0x1e340;
pub const PN_EMPTY: u32 = SEG_BASE + 0x01; // 空队收报
pub const PN_DUPREG: u32 = SEG_BASE + 0x02; // 重复注册
pub const PN_FULL: u32 = SEG_BASE + 0x03; // 超容发报
pub const PN_NONAME: u32 = SEG_BASE + 0x04; // 无名队列
pub const PN_FROZEN: u32 = SEG_BASE + 0x05; // 冻结后写账
pub const PN_ZLEN: u32 = SEG_BASE + 0x06; // 零长报文

/// 队列驻留定容。
pub const MQ_CAP: usize = 6;
/// 千次收发轮数。
pub const PN_ROUNDS: u32 = 1000;
/// POSIX mq 账域标（fold 种子）。
pub const PMQ_TAG: u64 = 0x1e34_26;
/// 队列规模（10/10 对平面）。
pub const MQS_NEEDED: usize = 10;

/// 单个具名消息队列：名 + 驻留 + 收发计数 + 通知注册位与触发数。
#[derive(Clone, Debug)]
pub struct NamedMq {
    pub name: String,
    pub q: Vec<u32>,
    pub sent: u32,
    pub recvd: u32,
    pub notify_on: bool,
    pub fired: u32,
}

impl NamedMq {
    /// 驻留字节数。
    pub fn resident(&self) -> u32 {
        let mut n = 0u32;
        let mut k = 0usize;
        while k < self.q.len() {
            n = n.wrapping_add(self.q[k]);
            k += 1;
        }
        n
    }

    pub fn render(&self) -> String {
        format!("mq|{}|n{}|s{}|r{}", self.name, self.q.len(), self.sent, self.recvd)
    }
}

/// POSIX mq 挂号账：注册具名队列 → 收发挂点 → 通知联动封账。
pub struct PmqBook {
    pub mqs: Vec<NamedMq>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl PmqBook {
    pub fn new() -> PmqBook {
        PmqBook { mqs: Vec::new(), frozen: false, root: None }
    }

    /// 注册具名队列：冻结、重名必拒。
    pub fn enroll(&mut self, name: &str) -> Result<(), u32> {
        if self.frozen {
            return Err(PN_FROZEN);
        }
        if self.mqs.iter().any(|x| x.name == name) {
            return Err(PN_DUPREG);
        }
        self.mqs.push(NamedMq {
            name: name.to_string(),
            q: Vec::new(),
            sent: 0,
            recvd: 0,
            notify_on: false,
            fired: 0,
        });
        Ok(())
    }

    /// 发送挂点：零长/超容必拒；空转非触发通知联动。
    pub fn send(&mut self, name: &str, len: u32) -> Result<u32, u32> {
        if self.frozen {
            return Err(PN_FROZEN);
        }
        if len == 0 {
            return Err(PN_ZLEN);
        }
        let mut k = 0usize;
        while k < self.mqs.len() {
            if self.mqs[k].name == name {
                if self.mqs[k].q.len() >= MQ_CAP {
                    return Err(PN_FULL);
                }
                let was_empty = self.mqs[k].q.is_empty();
                self.mqs[k].q.push(len);
                self.mqs[k].sent = self.mqs[k].sent.wrapping_add(1);
                if was_empty && self.mqs[k].notify_on {
                    self.mqs[k].notify_on = false;
                    self.mqs[k].fired = self.mqs[k].fired.wrapping_add(1);
                }
                return Ok(self.mqs[k].sent);
            }
            k += 1;
        }
        Err(PN_NONAME)
    }

    /// 接收挂点：空队必拒；出队字节出账。
    pub fn receive(&mut self, name: &str) -> Result<u32, u32> {
        if self.frozen {
            return Err(PN_FROZEN);
        }
        let mut k = 0usize;
        while k < self.mqs.len() {
            if self.mqs[k].name == name {
                if self.mqs[k].q.is_empty() {
                    return Err(PN_EMPTY);
                }
                let len = self.mqs[k].q.remove(0);
                self.mqs[k].recvd = self.mqs[k].recvd.wrapping_add(1);
                return Ok(len);
            }
            k += 1;
        }
        Err(PN_NONAME)
    }

    /// 通知注册：空队可注册，重复注册必拒。
    pub fn notify_reg(&mut self, name: &str) -> Result<(), u32> {
        if self.frozen {
            return Err(PN_FROZEN);
        }
        let mut k = 0usize;
        while k < self.mqs.len() {
            if self.mqs[k].name == name {
                if self.mqs[k].notify_on {
                    return Err(PN_DUPREG);
                }
                self.mqs[k].notify_on = true;
                return Ok(());
            }
            k += 1;
        }
        Err(PN_NONAME)
    }

    /// 联动断言：注册次数 = 触发数（每注册必触发且仅一次）。
    pub fn notify_linked(&self, regs: u32) -> bool {
        let mut fired = 0u32;
        let mut on = 0u32;
        let mut k = 0usize;
        while k < self.mqs.len() {
            fired = fired.wrapping_add(self.mqs[k].fired);
            if self.mqs[k].notify_on {
                on += 1;
            }
            k += 1;
        }
        fired == regs && on == 0
    }

    /// 按名查位次。
    pub fn find(&self, name: &str) -> Option<usize> {
        let mut k = 0usize;
        while k < self.mqs.len() {
            if self.mqs[k].name == name {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 链式 fold 指纹（FNV 风格，序敏感）。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = PMQ_TAG;
        let mut k = 0usize;
        while k < self.mqs.len() {
            let x = &self.mqs[k];
            h = (h ^ x.sent as u64).wrapping_mul(0x100000001b3);
            h = (h ^ x.recvd as u64).wrapping_mul(0x100000001b3);
            h = (h ^ x.fired as u64).wrapping_mul(0x100000001b3);
            h = (h ^ x.notify_on as u64).wrapping_mul(0x100000001b3);
            h = (h ^ x.resident() as u64).wrapping_mul(0x100000001b3);
            k += 1;
        }
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

/// 名确定性样本。
pub fn pmq_name(no: usize) -> String {
    format!("/mq{}", no)
}

/// 长度确定性样本。
pub fn pmq_len(no: u32) -> u32 {
    (no % 9 + 4) as u32
}

/// 千次对账全链：十队列注册；逐轮对每队 send+receive 成对；每队在整百
/// 轮先注册通知（空队生效）再 send 触发一次。
pub fn build_pmqbook() -> (PmqBook, u32, bool) {
    let mut b = PmqBook::new();
    let mut ok = true;
    let mut i = 0usize;
    while i < MQS_NEEDED {
        if b.enroll(&pmq_name(i)).is_err() {
            ok = false;
        }
        i += 1;
    }
    let mut regs = 0u32;
    let mut r = 0u32;
    while r < PN_ROUNDS {
        let k = (r % MQS_NEEDED as u32) as usize;
        let nm = pmq_name(k);
        if r % 100 == 99 {
            if b.notify_reg(&nm).is_err() {
                ok = false;
            }
            regs += 1;
        }
        if b.send(&nm, pmq_len(r)).is_err() {
            ok = false;
        }
        if b.receive(&nm).is_err() {
            ok = false;
        }
        r += 1;
    }
    let good = ok && b.notify_linked(regs);
    (b, regs, good)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 空队收报必拒
    let mut a = PmqBook::new();
    let _ = a.enroll("/e");
    got[0] = a.receive("/e").err() == Some(PN_EMPTY);
    // 2) 无名队列挂点必拒
    let mut b = PmqBook::new();
    got[1] = b.send("/none", 8).err() == Some(PN_NONAME)
        && b.notify_reg("/none").err() == Some(PN_NONAME);
    // 3) 重复注册必拒
    let mut c = PmqBook::new();
    let _ = c.enroll("/d");
    let _ = c.notify_reg("/d");
    got[2] = c.notify_reg("/d").err() == Some(PN_DUPREG);
    // 4) 冻结后写账必抓
    let (mut d, _, _) = build_pmqbook();
    d.freeze();
    got[3] = d.send(&pmq_name(0), 8).err() == Some(PN_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11026_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11026");

    // J1 收发挂点千次对平：十队千轮成对收发
    let (mut book, regs, green) = build_pmqbook();
    s.add("J1-千次对平", green && book.mqs.len() == 10, "十队千轮成对收发对平");

    // J2 通知注册账联动：注册即触发且触发后注销
    s.add("J2-通知联动", book.notify_linked(regs) && regs == PN_ROUNDS / 100, "注册数与触发数联动一致");

    // J3 冻结根：fold 稳定且与根一致
    let root = book.freeze();
    s.add("J3-冻结根", book.root == Some(root) && book.fold() == root && root != 0, "冻结根落账且指纹稳定");

    // J4 冻结后写账必抓
    let j4 = book.send(&pmq_name(0), 8).err() == Some(PN_FROZEN)
        && book.receive(&pmq_name(0)).err() == Some(PN_FROZEN);
    s.add("J4-冻结拒写", j4, "冻结后收发均必拒");

    // J5 注入检出：空队收报 + 无名队列
    let inj = inject_four();
    s.add("J5-空队无名抓", inj[0] && inj[1], "空队收报与无名队列两路必抓");

    // J6 注入检出：重复注册 + 冻结后写
    s.add("J6-重注冻结检出", inj[2] && inj[3], "重复注册与冻结后写必抓");

    // J7 通知仅空转非触发：驻留非空时注册不发
    let j7 = {
        let mut c = PmqBook::new();
        let _ = c.enroll("/n");
        let _ = c.send("/n", 8);
        let _ = c.notify_reg("/n");
        let mid = c.find("/n").map(|k| c.mqs[k].notify_on) == Some(true);
        let _ = c.receive("/n");
        let _ = c.send("/n", 8);
        let fired_after = c.find("/n").map(|k| c.mqs[k].fired) == Some(1);
        mid && fired_after
    };
    s.add("J7-空转非触发", j7, "非空驻留注册不触发清空后再入触发");

    // J8 超容发报必拒：第 MQ_CAP+1 条必拒
    let j8 = {
        let mut c = PmqBook::new();
        let _ = c.enroll("/f");
        let mut filled = true;
        let mut n = 0u32;
        while (n as usize) < MQ_CAP {
            if c.send("/f", 4).is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && c.send("/f", 4).err() == Some(PN_FULL)
    };
    s.add("J8-超容边界", j8, "超出定容必拒且原账不损");

    // J9 先进先出：驻留按入队序出队
    let j9 = {
        let mut c = PmqBook::new();
        let _ = c.enroll("/o");
        let _ = c.send("/o", 11);
        let _ = c.send("/o", 22);
        c.receive("/o") == Ok(11) && c.receive("/o") == Ok(22)
    };
    s.add("J9-先进先出", j9, "驻留按入队序出队");

    // J10 零长报文拒
    let j10 = {
        let mut c = PmqBook::new();
        let _ = c.enroll("/z");
        c.send("/z", 0).err() == Some(PN_ZLEN)
    };
    s.add("J10-零长拒", j10, "零长报文必拒");

    // J11 计数均衡：各队收发各千次
    let j11 = {
        let per = PN_ROUNDS / MQS_NEEDED as u32;
        let mut even = true;
        let mut k = 0usize;
        while k < book.mqs.len() {
            if book.mqs[k].sent != per || book.mqs[k].recvd != per {
                even = false;
            }
            k += 1;
        }
        even && per == 100
    };
    s.add("J11-计数均衡", j11, "十队各百次收发均衡");

    // J12 渲染面：队列渲染含名与计数
    let j12 = {
        let x = NamedMq {
            name: "/r".to_string(),
            q: Vec::new(),
            sent: 9,
            recvd: 7,
            notify_on: false,
            fired: 1,
        };
        let r = x.render();
        r.contains("/r") && r.contains("s9") && r.contains("r7")
    };
    s.add("J12-渲染明细", j12, "队列渲染含名计数");

    // J13 样本确定性：名与长度同入同出
    let n3a = pmq_name(3);
    let n3b = pmq_name(3);
    let l5a = pmq_len(5);
    let l5b = pmq_len(5);
    s.add("J13-样本确定性", n3a == n3b && l5a == l5b, "名与长度样本同入同出");

    // J14 触发敏感：触发前后指纹必变
    let j14 = {
        let mut c = PmqBook::new();
        let _ = c.enroll("/t");
        let _ = c.notify_reg("/t");
        let f0 = c.fold();
        let _ = c.send("/t", 6);
        c.fold() != f0
    };
    s.add("J14-触发敏感", j14, "通知触发指纹必变");

    // J15 动态明细：千次对平与冻结根落账
    let fp = book.fold();
    s.add_dyn(
        "J15-千次明细",
        fp == root && regs == 10,
        &format!("root={:#x} regs={}", fp, regs),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11026_pmq_flow() {
        let (mut b, regs, built) = build_pmqbook();
        assert!(built);
        assert!(regs == 10);
        assert!(b.receive(&pmq_name(0)).err() == Some(PN_EMPTY));
        b.freeze();
        assert!(b.send(&pmq_name(1), 8).err() == Some(PN_FROZEN));
        assert!(b.root.is_some());
    }

    #[test]
    fn unx11026_checks_green() {
        let s = run_unx11026_checks();
        assert!(s.all_passed(), "UNX-F11026 判据未全绿");
    }
}
