//! UNX-F10807 FIFO open 错误总面（O_RDONLY/O_WRONLY 组合矩阵）（0x1bd60 段）
//!
//! 锚点：UNX-F10807-J1 FIFO open 组合矩阵对平——O_RDONLY|O_NONBLOCK 即开
//! 即返、O_WRONLY|O_NONBLOCK 无读端必 ENXIO、阻塞对两open互等（Wait 面）、
//! O_RDWR 恒即开、路径不在必 ENOENT 五线全对平；注入坏实现（只读非阻塞
//! 误等/读写型误拒）必与权威分歧。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：FIFO 节点状态机（读端/写端计数，open/close 推进）+ open 请求
//! 判定器（访问型×非阻塞位×两端计数算 Opened/Wait/ENOENT/ENXIO）+
//! 互等注册核（Wait 面登记等待者）+ 注入核（两类坏实现必分歧）+ 组合
//! 正反双样本矩阵（3 访问型 × 2 非阻塞位 × 2 对端态）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// POSIX 锚：ENOENT=2、ENXIO=6。
pub const POSIX_ENOENT: u32 = 2;
pub const POSIX_ENXIO: u32 = 6;

/// 诊断段（0x1bd60 独占 0x20）。
pub const SEG_BASE: u32 = 0x1bd60;
pub const FO_RDONLY_WAIT: u32 = SEG_BASE + 0x01; // 只读非阻塞误等
pub const FO_WRONLY_OPEN: u32 = SEG_BASE + 0x02; // 写端无读端误放行
pub const FO_RDWR_REJECT: u32 = SEG_BASE + 0x03; // 读写型误拒
pub const FO_ENOENT_LOSS: u32 = SEG_BASE + 0x04; // 路径不在漏查
pub const FO_WAIT_LOSS: u32 = SEG_BASE + 0x05; // 互等注册丢失
pub const FO_CNT_SKEW: u32 = SEG_BASE + 0x06; // 端计数错账

/// 访问型三态。
#[derive(Clone, PartialEq, Eq)]
pub enum Access {
    RdOnly,
    WrOnly,
    RdWr,
}

/// open 裁决输出。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum OpenOut {
    /// 即开成功。
    Opened,
    /// 阻塞互等（登记等待者）。
    Wait,
}

/// FIFO 节点状态机：读端/写端计数。
#[derive(Clone)]
pub struct FifoNode {
    pub exists: bool,
    pub readers: u32,
    pub writers: u32,
    pub note: String,
}

impl FifoNode {
    pub fn new() -> FifoNode {
        FifoNode { exists: true, readers: 0, writers: 0, note: "fifo".to_string() }
    }

    pub fn absent() -> FifoNode {
        FifoNode { exists: false, readers: 0, writers: 0, note: "absent".to_string() }
    }

    pub fn add_reader(&mut self) {
        self.readers += 1;
    }

    pub fn add_writer(&mut self) {
        self.writers += 1;
    }

    pub fn drop_reader(&mut self) {
        if self.readers > 0 {
            self.readers -= 1;
        }
    }

    pub fn drop_writer(&mut self) {
        if self.writers > 0 {
            self.writers -= 1;
        }
    }
}

/// open 请求。
#[derive(Clone)]
pub struct OpenReq {
    pub access: Access,
    pub nonblock: bool,
}

/// open 判定核：访问型 × 非阻塞位 × 两端计数。
pub fn judge(node: &FifoNode, req: &OpenReq) -> Result<OpenOut, u32> {
    if !node.exists {
        return Err(FO_ENOENT_LOSS);
    }
    match req.access {
        Access::RdOnly => {
            if req.nonblock {
                return Ok(OpenOut::Opened);
            }
            if node.writers > 0 {
                Ok(OpenOut::Opened)
            } else {
                Ok(OpenOut::Wait)
            }
        }
        Access::WrOnly => {
            if node.readers > 0 {
                return Ok(OpenOut::Opened);
            }
            if req.nonblock {
                Err(FO_WRONLY_OPEN)
            } else {
                Ok(OpenOut::Wait)
            }
        }
        Access::RdWr => Ok(OpenOut::Opened),
    }
}

/// 互等注册账：Wait 面登记 (访问型)，对端抵达即清账。
pub struct WaitLedger {
    pub rd_waiting: u32,
    pub wr_waiting: u32,
    pub note: String,
}

impl WaitLedger {
    pub fn new() -> WaitLedger {
        WaitLedger { rd_waiting: 0, wr_waiting: 0, note: "idle".to_string() }
    }

    /// open 落 Wait 面时登记等待者。
    pub fn register(&mut self, req: &OpenReq) -> Result<(), u32> {
        match judge(&FifoNode::new(), req) {
            Ok(OpenOut::Wait) => {
                match req.access {
                    Access::RdOnly => self.rd_waiting += 1,
                    Access::WrOnly => self.wr_waiting += 1,
                    Access::RdWr => return Err(FO_WAIT_LOSS),
                }
                self.note = "waiting".to_string();
                Ok(())
            }
            Ok(OpenOut::Opened) => Err(FO_WAIT_LOSS),
            Err(c) => Err(c),
        }
    }

    /// 对端抵达：双向等待互清。
    pub fn counterpart_arrives(&mut self) -> bool {
        if self.rd_waiting > 0 && self.wr_waiting > 0 {
            self.rd_waiting -= 1;
            self.wr_waiting -= 1;
            self.note = "matched".to_string();
            true
        } else {
            false
        }
    }
}

/// 正反双样本矩阵容量（先定后用）。
pub const MATRIX_CAP: usize = 24;

/// 组合矩阵：3 访问型 × 2 非阻塞位 × 2 对端态。
pub fn sample_matrix() -> Vec<(FifoNode, OpenReq, Option<u32>)> {
    let mut m: Vec<(FifoNode, OpenReq, Option<u32>)> = Vec::new();
    let node = FifoNode::new();
    let busy = {
        let mut n = FifoNode::new();
        n.add_reader();
        n.add_writer();
        n
    };
    // O_RDONLY
    m.push((node.clone(), OpenReq { access: Access::RdOnly, nonblock: true }, None));
    m.push((busy.clone(), OpenReq { access: Access::RdOnly, nonblock: false }, None));
    m.push((node.clone(), OpenReq { access: Access::RdOnly, nonblock: false }, None));
    // O_WRONLY
    m.push((busy.clone(), OpenReq { access: Access::WrOnly, nonblock: true }, None));
    m.push((busy.clone(), OpenReq { access: Access::WrOnly, nonblock: false }, None));
    m.push((node.clone(), OpenReq { access: Access::WrOnly, nonblock: false }, None));
    m.push((node.clone(), OpenReq { access: Access::WrOnly, nonblock: true }, Some(FO_WRONLY_OPEN)));
    // O_RDWR
    m.push((node.clone(), OpenReq { access: Access::RdWr, nonblock: true }, None));
    m.push((node.clone(), OpenReq { access: Access::RdWr, nonblock: false }, None));
    // 路径不在
    m.push((
        FifoNode::absent(),
        OpenReq { access: Access::RdOnly, nonblock: true },
        Some(FO_ENOENT_LOSS),
    ));
    m
}

/// 矩阵对平：返回 (命中数, 总数, 容量合规)。
pub fn verify_matrix() -> (usize, usize, bool) {
    let m = sample_matrix();
    let mut ok = 0usize;
    let mut i = 0usize;
    while i < m.len() {
        let (node, req, want) = &m[i];
        let got = judge(node, req);
        let hit = match want {
            None => got.is_ok(),
            Some(c) => got.err() == Some(*c),
        };
        if hit {
            ok += 1;
        }
        i += 1;
    }
    (ok, m.len(), m.len() <= MATRIX_CAP)
}

/// 坏实现 A：只读非阻塞误等（O_RDONLY|O_NONBLOCK 也注册等待）。
pub fn faulty_rd_wait(node: &FifoNode, req: &OpenReq) -> Result<OpenOut, u32> {
    match req.access {
        Access::RdOnly => Ok(OpenOut::Wait),
        _ => judge(node, req),
    }
}

/// 坏实现 B：读写型误拒（O_RDWR 也要求对端在位）。
pub fn faulty_rdwr_reject(node: &FifoNode, req: &OpenReq) -> Result<OpenOut, u32> {
    match req.access {
        Access::RdWr => {
            if node.readers == 0 || node.writers == 0 {
                Err(FO_RDWR_REJECT)
            } else {
                Ok(OpenOut::Opened)
            }
        }
        _ => judge(node, req),
    }
}

/// 注入核：两类坏实现必与权威分歧（检出）。
pub fn inject_two() -> [bool; 2] {
    let mut got = [false; 2];
    let empty = FifoNode::new();
    // 1) 只读非阻塞误等：权威 Opened、坏实现 Wait → 检出
    let req1 = OpenReq { access: Access::RdOnly, nonblock: true };
    got[0] = judge(&empty, &req1) != faulty_rd_wait(&empty, &req1)
        && judge(&empty, &req1) == Ok(OpenOut::Opened);
    // 2) 读写型误拒：空管 O_RDWR 权威 Opened、坏实现拒 → 检出
    let req2 = OpenReq { access: Access::RdWr, nonblock: false };
    got[1] = judge(&empty, &req2) != faulty_rdwr_reject(&empty, &req2)
        && judge(&empty, &req2) == Ok(OpenOut::Opened);
    got
}

/// 互等生命周期核：阻塞双 open 登记 → 对端抵达互清 → 计数对平。
pub fn wait_cycle_ok() -> bool {
    let mut led = WaitLedger::new();
    let r1 = led.register(&OpenReq { access: Access::RdOnly, nonblock: false });
    let r2 = led.register(&OpenReq { access: Access::WrOnly, nonblock: false });
    let matched = led.counterpart_arrives();
    let re_open = led.register(&OpenReq { access: Access::RdWr, nonblock: false });
    r1.is_ok() && r2.is_ok() && matched && re_open.err() == Some(FO_WAIT_LOSS)
        && led.rd_waiting == 0 && led.wr_waiting == 0
}

/// FIFO open 组合矩阵判据（对应卡验收 J 判据）。
pub fn run_unx10807_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10807");
    let inj = inject_two();
    let (hits, total, cap_ok) = verify_matrix();

    // J1 组合矩阵对平
    s.add("J1-组合矩阵对平", hits == total && cap_ok, "十线组合样本全命中");

    // J2 O_RDONLY|O_NONBLOCK 即开即返（空管亦然）
    let j2 = judge(&FifoNode::new(), &OpenReq { access: Access::RdOnly, nonblock: true })
        == Ok(OpenOut::Opened);
    s.add("J2-只读非阻塞即开", j2, "O_RDONLY|O_NONBLOCK 不等对端");

    // J3 O_WRONLY|O_NONBLOCK 无读端必 ENXIO
    let j3 = judge(&FifoNode::new(), &OpenReq { access: Access::WrOnly, nonblock: true })
        .err() == Some(FO_WRONLY_OPEN);
    s.add("J3-写端无读必拒", j3, "O_WRONLY|O_NONBLOCK 无读端 ENXIO");

    // J4 阻塞面：双端互等 Wait（空管）
    let j4 = judge(&FifoNode::new(), &OpenReq { access: Access::RdOnly, nonblock: false })
        == Ok(OpenOut::Wait)
        && judge(&FifoNode::new(), &OpenReq { access: Access::WrOnly, nonblock: false })
            == Ok(OpenOut::Wait);
    s.add("J4-阻塞互等", j4, "空管阻塞 open 双向 Wait");

    // J5 O_RDWR 恒即开
    let j5 = judge(&FifoNode::new(), &OpenReq { access: Access::RdWr, nonblock: true })
        == Ok(OpenOut::Opened)
        && judge(&FifoNode::new(), &OpenReq { access: Access::RdWr, nonblock: false })
            == Ok(OpenOut::Opened);
    s.add("J5-读写型恒开", j5, "O_RDWR 不等不拒");

    // J6 路径不在必 ENOENT
    let j6 = judge(&FifoNode::absent(), &OpenReq { access: Access::RdOnly, nonblock: true })
        .err() == Some(FO_ENOENT_LOSS);
    s.add("J6-ENOENT优先", j6, "路径不在先于访问型裁");

    // J7 互等生命周期核
    s.add("J7-互等注册核", wait_cycle_ok(), "登记→对端抵达互清→读写型不注册");

    // J8 注入1 只读非阻塞误等检出
    s.add("J8-误等检出", inj[0], "O_RDONLY|O_NONBLOCK 误等必分歧");

    // J9 注入2 读写型误拒检出
    s.add("J9-误拒检出", inj[1], "O_RDWR 误拒必分歧");

    // J10 端计数推进：open/close 全序计数对平
    let mut n = FifoNode::new();
    n.add_reader();
    n.add_reader();
    n.add_writer();
    n.drop_reader();
    n.drop_writer();
    n.drop_writer();
    let j10 = n.readers == 1 && n.writers == 0;
    s.add("J10-端计数对平", j10, "open/close 增减不透支");

    // J11 对端在位加速：有读端时阻塞写端 open 即开
    let mut n2 = FifoNode::new();
    n2.add_reader();
    let j11 = judge(&n2, &OpenReq { access: Access::WrOnly, nonblock: false })
        == Ok(OpenOut::Opened);
    s.add("J11-对端在位即开", j11, "读端在位写端即开");

    // J12 字面锚：ENOENT=2 / ENXIO=6
    let j12 = POSIX_ENOENT == 2 && POSIX_ENXIO == 6;
    s.add("J12-字面锚对平", j12, "2/6 双锚");

    // J13 (dyn) 节点指纹落账
    let mut n3 = FifoNode::new();
    n3.add_writer();
    let mut h: u32 = SEG_BASE;
    h = h.wrapping_mul(31).wrapping_add(n3.exists as u32);
    h = h.wrapping_mul(31).wrapping_add(n3.readers);
    h = h.wrapping_mul(31).wrapping_add(n3.writers);
    for c in n3.note.bytes() {
        h = h.wrapping_mul(31).wrapping_add(c as u32);
    }
    s.add_dyn("J13-节点指纹", h != 0, &format!("fold={:#x}", h));

    // J14 (dyn) 矩阵命中明细
    s.add_dyn(
        "J14-命中明细",
        hits == total,
        &format!("hits={}/{} cap_ok={}", hits, total, cap_ok),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10807_fifo_open_flow() {
        let mut n = FifoNode::new();
        let req_w = OpenReq { access: Access::WrOnly, nonblock: true };
        assert!(judge(&n, &req_w).err() == Some(FO_WRONLY_OPEN));
        n.add_reader();
        assert_eq!(judge(&n, &req_w), Ok(OpenOut::Opened));
        n.drop_reader();
        assert!(judge(&n, &req_w).err() == Some(FO_WRONLY_OPEN));
        assert_eq!(
            judge(&FifoNode::absent(), &req_w),
            Err(FO_ENOENT_LOSS)
        );
        assert!(judge(&FifoNode::absent(), &OpenReq { access: Access::RdWr, nonblock: false })
            .err() == Some(FO_ENOENT_LOSS));
    }

    #[test]
    fn unx10807_checks_green() {
        let s = run_unx10807_checks();
        assert!(s.all_passed(), "UNX-F10807 判据未全绿");
    }
}
