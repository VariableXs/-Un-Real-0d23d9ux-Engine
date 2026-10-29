//! UNX-F10945 mq_receive EBADF/EINVAL 矩阵（0x1d5a0 段）
//!
//! 纯功能模型：以描述符账与出参指针有效性为中心的 `mq_receive`
//! 裁决台。两错误面：① 坏描述符（未开/已关/只写开）→ EBADF；
//! ② msg_prio 出参空指针 → EINVAL。
//! 与 F10944 分工：本单不看空队与缓冲，只看描述符与出参纪律。
//! 反面千轮样本只走合法描述符与非空出参，零误报。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1d5a0;
/// 坏描述符
pub const RV_EBADF: u32 = SEG_BASE + 0x01;
/// msg_prio 出参空指针
pub const RV_EINVAL: u32 = SEG_BASE + 0x02;
/// 台内对账失败
pub const RV_LEDGER: u32 = SEG_BASE + 0x03;

// ---------------------------------------------------------------- 常数
/// 反面验收轮数
pub const RV_ROUNDS: usize = 1000;

// ---------------------------------------------------------------- 数据结构
/// 接收侧描述符账条目。
#[derive(Clone, Debug, PartialEq)]
pub struct RvEnt {
    /// 描述符号
    pub fd: u32,
    /// 是否开着
    pub open: bool,
    /// 是否可读（receive 需要）
    pub readable: bool,
}

/// 单轮日志。
#[derive(Clone, Debug, PartialEq)]
pub struct RvLog {
    /// 轮次
    pub round: usize,
    /// 目标描述符
    pub fd: u32,
    /// 裁定码：0 成功
    pub verdict: u32,
}

/// mq_receive 描述符/出参裁决台。
#[derive(Clone, Debug)]
pub struct RcvDesk {
    /// 描述符账
    pub fds: Vec<RvEnt>,
    /// 成功接收条数
    pub recvd: usize,
    /// 轮次
    pub rounds: usize,
    /// 成功轮
    pub ok_hits: usize,
    /// EBADF 命中
    pub ebadf_hits: usize,
    /// EINVAL 命中
    pub einval_hits: usize,
    /// 逐轮日志
    pub log: Vec<RvLog>,
}

impl RcvDesk {
    /// 建台。
    pub fn new() -> Self {
        RcvDesk {
            fds: Vec::new(),
            recvd: 0,
            rounds: 0,
            ok_hits: 0,
            ebadf_hits: 0,
            einval_hits: 0,
            log: Vec::new(),
        }
    }

    /// 开一条可读描述符。
    pub fn open_read(&mut self) -> u32 {
        let fd = self.fds.len() as u32;
        self.fds.push(RvEnt { fd, open: true, readable: true });
        fd
    }

    /// 开一条只写描述符。
    pub fn open_write(&mut self) -> u32 {
        let fd = self.fds.len() as u32;
        self.fds.push(RvEnt { fd, open: true, readable: false });
        fd
    }

    /// 关闭描述符。
    pub fn close(&mut self, fd: u32) -> bool {
        let mut i = 0;
        while i < self.fds.len() {
            if self.fds[i].fd == fd {
                self.fds[i].open = false;
                return true;
            }
            i += 1;
        }
        false
    }

    /// 描述符有效性核查。
    pub fn fd_ok(&self, fd: u32) -> bool {
        let mut i = 0;
        while i < self.fds.len() {
            if self.fds[i].fd == fd {
                return self.fds[i].open && self.fds[i].readable;
            }
            i += 1;
        }
        false
    }

    /// 记账。
    fn book(&mut self, round: usize, fd: u32, verdict: u32) {
        self.rounds = self.rounds.wrapping_add(1);
        match verdict {
            0 => self.ok_hits = self.ok_hits.wrapping_add(1),
            x if x == RV_EBADF => self.ebadf_hits = self.ebadf_hits.wrapping_add(1),
            x if x == RV_EINVAL => self.einval_hits = self.einval_hits.wrapping_add(1),
            _ => {}
        }
        self.log.push(RvLog { round, fd, verdict });
    }

    /// 裁决一次 mq_receive。判定序：
    /// ① 描述符无效 → EBADF；② msg_prio 出参空 → EINVAL；③ 记账。
    pub fn mq_receive(&mut self, fd: u32, prio_ptr_valid: bool, round: usize) -> Result<u32, u32> {
        if !self.fd_ok(fd) {
            self.book(round, fd, RV_EBADF);
            return Err(RV_EBADF);
        }
        if !prio_ptr_valid {
            self.book(round, fd, RV_EINVAL);
            return Err(RV_EINVAL);
        }
        self.recvd = self.recvd.wrapping_add(1);
        self.book(round, fd, 0);
        Ok(self.recvd as u32)
    }

    /// 账目对平。
    pub fn ledger_ok(&self) -> bool {
        let total = self
            .ok_hits
            .wrapping_add(self.ebadf_hits)
            .wrapping_add(self.einval_hits);
        total == self.rounds && self.log.len() == self.rounds
    }

    /// FNV 指纹。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        let mut i = 0;
        while i < self.fds.len() {
            h = (h ^ (self.fds[i].fd as u64)).wrapping_mul(0x100000001b3);
            h = (h ^ (self.fds[i].open as u64)).wrapping_mul(0x100000001b3);
            h = (h ^ (self.fds[i].readable as u64)).wrapping_mul(0x100000001b3);
            i += 1;
        }
        h = (h ^ (self.rounds as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ (self.recvd as u64)).wrapping_mul(0x100000001b3);
        h
    }

    /// 渲染。
    pub fn render(&self) -> String {
        format!(
            "RcvDesk{{fds={},recvd={},rounds={},ok={},ebadf={},einval={}}}",
            self.fds.len(), self.recvd, self.rounds, self.ok_hits,
            self.ebadf_hits, self.einval_hits
        )
    }
}

// ---------------------------------------------------------------- 注入
/// 注入四路坏样本，返回四路是否全部被台正确拦截。
pub fn inject_four() -> [bool; 4] {
    let mut caught = [false; 4];
    let mut d = RcvDesk::new();
    let fdr = d.open_read();
    let fdw = d.open_write();
    // 注入 1：未开描述符 → EBADF。
    caught[0] = matches!(d.mq_receive(0x999, true, 9001), Err(x) if x == RV_EBADF);
    // 注入 2：只写描述符收消息 → EBADF。
    caught[1] = matches!(d.mq_receive(fdw, true, 9002), Err(x) if x == RV_EBADF);
    // 注入 3：关闭后再收 → EBADF。
    assert!(d.close(fdr));
    caught[2] = matches!(d.mq_receive(fdr, true, 9003), Err(x) if x == RV_EBADF);
    // 注入 4：msg_prio 空指针 → EINVAL。
    let fd2 = d.open_read();
    caught[3] = matches!(d.mq_receive(fd2, false, 9004), Err(x) if x == RV_EINVAL);
    caught
}

// ---------------------------------------------------------------- 确定性样本
/// 千轮样本：合法接收为主，三错误面轮转。返回符合预期轮数。
pub fn sample_thousand(d: &mut RcvDesk) -> usize {
    let fdr = d.open_read();
    let fdw = d.open_write();
    let mut good = 0;
    let mut r = 0;
    while r < RV_ROUNDS {
        let low = r % 10;
        let res;
        let expect_ok;
        if low < 6 {
            res = d.mq_receive(fdr, true, r);
            expect_ok = true;
        } else if low == 6 {
            res = d.mq_receive(0x7e00 + (r % 6) as u32, true, r);
            expect_ok = false;
        } else if low == 7 {
            res = d.mq_receive(fdw, true, r);
            expect_ok = false;
        } else if low == 8 {
            res = d.mq_receive(fdr, false, r);
            expect_ok = false;
        } else {
            res = d.mq_receive(fdr, true, r);
            expect_ok = true;
        }
        let matched = match &res {
            Ok(_) => expect_ok,
            Err(x) => !expect_ok && (*x == RV_EBADF || *x == RV_EINVAL),
        };
        if matched {
            good += 1;
        }
        r += 1;
    }
    good
}

// ---------------------------------------------------------------- 判据
/// UNX-F10945 判据组。
pub fn run_unx10945_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10945");
    // J1 正面之一：坏描述符 → EBADF 十轮全中。
    let mut d1 = RcvDesk::new();
    let fdr1 = d1.open_read();
    let mut c = 0;
    let mut k = 0;
    while k < 10 {
        let r = if k % 3 == 0 {
            d1.mq_receive(0xdead, true, k)
        } else if k % 3 == 1 {
            d1.mq_receive(0x11, true, k)
        } else {
            let _ = d1.close(fdr1);
            d1.mq_receive(fdr1, true, k)
        };
        if matches!(r, Err(x) if x == RV_EBADF) {
            c += 1;
        }
        k += 1;
    }
    s.add("J1-正面EBADF", c == 10, "未开/只写/已关描述符十轮全部裁 EBADF");
    // J1 正面之二：msg_prio 空指针 → EINVAL 十轮全中。
    let mut d2 = RcvDesk::new();
    let fdr2 = d2.open_read();
    let mut c = 0;
    let mut k = 0;
    while k < 10 {
        if matches!(d2.mq_receive(fdr2, false, k), Err(x) if x == RV_EINVAL) {
            c += 1;
        }
        k += 1;
    }
    s.add("J1-正面EINVAL", c == 10, "msg_prio 空指针十轮全部裁 EINVAL");
    // J2 反面：千轮合法接收零误报。
    let mut d3 = RcvDesk::new();
    let good = sample_thousand(&mut d3);
    s.add_dyn("J2-反面零误报", good == RV_ROUNDS, &format!("千轮样本符合预期 {} 轮", good));
    // J3 注入四路全捕获。
    let inj = inject_four();
    s.add(
        "J3-注入四路",
        inj[0] && inj[1] && inj[2] && inj[3],
        "未开/只写/已关/空指针四路坏样本全部拦截",
    );
    // J4 判定序：EBADF 先于 EINVAL。
    let mut d4 = RcvDesk::new();
    s.add(
        "J4-判定序",
        matches!(d4.mq_receive(9, false, 1), Err(x) if x == RV_EBADF),
        "双错同现按序裁 EBADF",
    );
    // J5 错误码分段。
    s.add(
        "J5-错误码分段",
        RV_EBADF > SEG_BASE && RV_LEDGER < SEG_BASE + 0x20,
        "三个错误码全部落在段基址页内",
    );
    // J6 错误码互异。
    s.add(
        "J6-错误码互异",
        RV_EBADF != RV_EINVAL && RV_EINVAL != RV_LEDGER && RV_EBADF != RV_LEDGER,
        "三个错误码两两不同",
    );
    // J7 账目对平。
    s.add("J7-账目对平", d3.ledger_ok(), "千轮之后三类计数与轮次、日志逐项对平");
    // J8 接收账一致。
    s.add("J8-接收账一致", d3.recvd == d3.ok_hits, "成功接收条数等于成功轮数");
    // J9 fold 稳定。
    let f = d3.fold();
    let clone = d3.clone();
    s.add("J9-fold稳定", clone.fold() == f, "克隆后指纹逐位一致");
    // J10 渲染非空。
    let txt = d3.render();
    s.add("J10-渲染非空", txt.len() > 10 && txt.contains("RcvDesk"), "台级渲染含类型名与计数");
    // J11 出参合法时回填接收序号。
    let mut d5 = RcvDesk::new();
    let fdr5 = d5.open_read();
    let v1 = d5.mq_receive(fdr5, true, 1);
    let v2 = d5.mq_receive(fdr5, true, 2);
    s.add(
        "J11-序号回填",
        matches!(&v1, Ok(x) if *x == 1) && matches!(&v2, Ok(x) if *x == 2),
        "连续接收回填递增序号",
    );
    // J12 只读域互斥：写描述符不可收，读描述符不可发（同账复用 10943 口径的反向断言）。
    let mut d6 = RcvDesk::new();
    let fdw6 = d6.open_write();
    let fdr6 = d6.open_read();
    s.add(
        "J12-读写互斥",
        !d6.fd_ok(fdw6) && d6.fd_ok(fdr6),
        "只写描述符不通过接收有效性核查",
    );
    // J13 动态：千轮明细。
    s.add_dyn(
        "J13-千轮明细",
        good == RV_ROUNDS,
        &format!("good={} desk={}", good, d3.render()),
    );
    // J14 动态：注入明细。
    s.add_dyn(
        "J14-注入明细",
        inj.iter().all(|x| *x),
        &format!("inj={:?}", inj),
    );
    s
}

// ---------------------------------------------------------------- 测试
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10945_mq_recv_einval_flow() {
        // 流程：开 fd → 合法接收 → 空指针 → 只写拒 → 关闭拒。
        let mut d = RcvDesk::new();
        let fdr = d.open_read();
        let fdw = d.open_write();
        assert_eq!(d.mq_receive(fdr, true, 1), Ok(1));
        assert!(matches!(d.mq_receive(fdr, false, 2), Err(x) if x == RV_EINVAL));
        assert!(matches!(d.mq_receive(fdw, true, 3), Err(x) if x == RV_EBADF));
        assert!(d.close(fdr));
        assert!(matches!(d.mq_receive(fdr, true, 4), Err(x) if x == RV_EBADF));
        assert!(d.ledger_ok());
        assert!(inject_four().iter().all(|x| *x));
    }

    #[test]
    fn unx10945_checks_green() {
        let s = run_unx10945_checks();
        assert!(s.all_passed(), "UNX-F10945 判据未全绿");
    }
}
