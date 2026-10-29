//! UNX-F10943 mq_send EINVAL/EBADF 矩阵（0x1d560 段）
//!
//! 纯功能模型：以描述符账为中心的 `mq_send` 有效性裁决。
//! 两错误面：① 坏描述符（未开/已关/只读开）→ EBADF；
//! ② 非法优先级（>= MQ_PRIO_MAX）→ EINVAL。
//! 与 F10942 分工：本单不看队满与超长，只看描述符账与优先级域。
//! 反面千轮样本只走合法描述符与合法优先级，零误报。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1d560;
/// 坏描述符（未开/已关/无写权）
pub const PR_EBADF: u32 = SEG_BASE + 0x01;
/// 非法优先级
pub const PR_EINVAL: u32 = SEG_BASE + 0x02;
/// 账内对平失败
pub const PR_LEDGER: u32 = SEG_BASE + 0x03;

// ---------------------------------------------------------------- 常数
/// POSIX 优先级上限（MQ_PRIO_MAX）
pub const PRIO_MAX: u32 = 32768;
/// 反面验收轮数
pub const PR_ROUNDS: usize = 1000;

// ---------------------------------------------------------------- 数据结构
/// 描述符账条目。
#[derive(Clone, Debug, PartialEq)]
pub struct FdEnt {
    /// 描述符号
    pub fd: u32,
    /// 是否开着
    pub open: bool,
    /// 是否可写（send 需要）
    pub writable: bool,
}

/// 单轮裁决日志。
#[derive(Clone, Debug, PartialEq)]
pub struct PrLog {
    /// 轮次
    pub round: usize,
    /// 目标描述符
    pub fd: u32,
    /// 裁定码：0 成功
    pub verdict: u32,
}

/// mq_send 描述符/优先级裁决台。
#[derive(Clone, Debug)]
pub struct PrioDesk {
    /// 描述符账
    pub fds: Vec<FdEnt>,
    /// 成功记账条数
    pub sent: usize,
    /// 轮次
    pub rounds: usize,
    /// 成功轮
    pub ok_hits: usize,
    /// EBADF 命中
    pub ebadf_hits: usize,
    /// EINVAL 命中
    pub einval_hits: usize,
    /// 逐轮日志
    pub log: Vec<PrLog>,
}

impl PrioDesk {
    /// 建台。
    pub fn new() -> Self {
        PrioDesk {
            fds: Vec::new(),
            sent: 0,
            rounds: 0,
            ok_hits: 0,
            ebadf_hits: 0,
            einval_hits: 0,
            log: Vec::new(),
        }
    }

    /// 开一条可写描述符，返回 fd 号。
    pub fn open_write(&mut self) -> u32 {
        let fd = self.fds.len() as u32;
        self.fds.push(FdEnt { fd, open: true, writable: true });
        fd
    }

    /// 开一条只读描述符。
    pub fn open_read(&mut self) -> u32 {
        let fd = self.fds.len() as u32;
        self.fds.push(FdEnt { fd, open: true, writable: false });
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
                return self.fds[i].open && self.fds[i].writable;
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
            x if x == PR_EBADF => self.ebadf_hits = self.ebadf_hits.wrapping_add(1),
            x if x == PR_EINVAL => self.einval_hits = self.einval_hits.wrapping_add(1),
            _ => {}
        }
        self.log.push(PrLog { round, fd, verdict });
    }

    /// 裁决一次 mq_send。判定序：① 描述符无效 → EBADF；
    /// ② 优先级越界 → EINVAL；③ 记账成功。
    pub fn mq_send(&mut self, fd: u32, prio: u32, round: usize) -> Result<usize, u32> {
        if !self.fd_ok(fd) {
            self.book(round, fd, PR_EBADF);
            return Err(PR_EBADF);
        }
        if prio >= PRIO_MAX {
            self.book(round, fd, PR_EINVAL);
            return Err(PR_EINVAL);
        }
        self.sent = self.sent.wrapping_add(1);
        self.book(round, fd, 0);
        Ok(self.sent)
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
            h = (h ^ (self.fds[i].writable as u64)).wrapping_mul(0x100000001b3);
            i += 1;
        }
        h = (h ^ (self.rounds as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ (self.sent as u64)).wrapping_mul(0x100000001b3);
        h
    }

    /// 渲染。
    pub fn render(&self) -> String {
        format!(
            "PrioDesk{{fds={},sent={},rounds={},ok={},ebadf={},einval={}}}",
            self.fds.len(), self.sent, self.rounds, self.ok_hits,
            self.ebadf_hits, self.einval_hits
        )
    }
}

// ---------------------------------------------------------------- 注入
/// 注入四路坏样本，返回四路是否全部被台正确拦截。
pub fn inject_four() -> [bool; 4] {
    let mut caught = [false; 4];
    let mut d = PrioDesk::new();
    let fdw = d.open_write();
    let fdr = d.open_read();
    // 注入 1：未开描述符 → EBADF。
    caught[0] = matches!(d.mq_send(999, 1, 9001), Err(x) if x == PR_EBADF);
    // 注入 2：只读描述符发消息 → EBADF。
    caught[1] = matches!(d.mq_send(fdr, 1, 9002), Err(x) if x == PR_EBADF);
    // 注入 3：关闭后再发 → EBADF。
    assert!(d.close(fdw));
    caught[2] = matches!(d.mq_send(fdw, 1, 9003), Err(x) if x == PR_EBADF);
    // 注入 4：优先级越界 → EINVAL（新开可写描述符）。
    let fd2 = d.open_write();
    caught[3] = matches!(d.mq_send(fd2, PRIO_MAX, 9004), Err(x) if x == PR_EINVAL);
    caught
}

// ---------------------------------------------------------------- 确定性样本
/// 千轮样本：合法描述符与合法优先级为主，三错误面轮转。
/// 返回符合预期轮数。
pub fn sample_thousand(d: &mut PrioDesk) -> usize {
    let fdw = d.open_write();
    let fdr = d.open_read();
    let mut good = 0;
    let mut r = 0;
    while r < PR_ROUNDS {
        let low = r % 10;
        let res;
        let expect_ok;
        if low < 6 {
            // 合法面：可写描述符 + 区间内优先级。
            res = d.mq_send(fdw, (r % 32) as u32, r);
            expect_ok = true;
        } else if low == 6 {
            // EBADF 面：未开号。
            res = d.mq_send(0x7f00 + (r % 8) as u32, 1, r);
            expect_ok = false;
        } else if low == 7 {
            // EBADF 面：只读描述符。
            res = d.mq_send(fdr, 1, r);
            expect_ok = false;
        } else if low == 8 {
            // EINVAL 面：优先级恰达上限。
            res = d.mq_send(fdw, PRIO_MAX + (r % 5) as u32, r);
            expect_ok = false;
        } else {
            // 合法面：优先级贴上限之下仍合法。
            res = d.mq_send(fdw, PRIO_MAX - 1, r);
            expect_ok = true;
        }
        let matched = match &res {
            Ok(_) => expect_ok,
            Err(x) => !expect_ok && (*x == PR_EBADF || *x == PR_EINVAL),
        };
        if matched {
            good += 1;
        }
        r += 1;
    }
    good
}

// ---------------------------------------------------------------- 判据
/// UNX-F10943 判据组。
pub fn run_unx10943_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10943");
    // J1 正面之一：坏描述符 → EBADF 十轮全中。
    let mut d1 = PrioDesk::new();
    let _ = d1.open_write();
    let mut c = 0;
    let mut k = 0;
    while k < 10 {
        let r = if k % 3 == 0 {
            d1.mq_send(0xdead, 1, k)
        } else if k % 3 == 1 {
            d1.mq_send(1, 1, k)
        } else {
            // 描述符 0 关闭后引用。
            let _ = d1.close(0);
            d1.mq_send(0, 1, k)
        };
        if matches!(r, Err(x) if x == PR_EBADF) {
            c += 1;
        }
        k += 1;
    }
    s.add("J1-正面EBADF", c == 10, "未开/只读/已关描述符十轮全部裁 EBADF");
    // J1 正面之二：非法优先级 → EINVAL 十轮全中。
    let mut d2 = PrioDesk::new();
    let fd2 = d2.open_write();
    let mut c = 0;
    let mut k = 0;
    while k < 10 {
        if matches!(d2.mq_send(fd2, PRIO_MAX + k as u32, k), Err(x) if x == PR_EINVAL) {
            c += 1;
        }
        k += 1;
    }
    s.add("J1-正面EINVAL", c == 10, "优先级越界十轮全部裁 EINVAL");
    // J2 反面：千轮合法范围零误报。
    let mut d3 = PrioDesk::new();
    let good = sample_thousand(&mut d3);
    s.add_dyn("J2-反面零误报", good == PR_ROUNDS, &format!("千轮样本符合预期 {} 轮", good));
    // J3 注入四路全捕获。
    let inj = inject_four();
    s.add(
        "J3-注入四路",
        inj[0] && inj[1] && inj[2] && inj[3],
        "未开/只读/已关/优先级越界四路坏样本全部拦截",
    );
    // J4 判定序：EBADF 先于 EINVAL（坏描述符 + 坏优先级裁 EBADF）。
    let mut d4 = PrioDesk::new();
    s.add(
        "J4-判定序",
        matches!(d4.mq_send(9, PRIO_MAX, 1), Err(x) if x == PR_EBADF),
        "双错同现按序裁 EBADF",
    );
    // J5 优先级边界：上限之下合法。
    let mut d5 = PrioDesk::new();
    let fd5 = d5.open_write();
    s.add(
        "J5-优先级边界",
        matches!(d5.mq_send(fd5, PRIO_MAX - 1, 1), Ok(_)),
        "PRIO_MAX-1 合法",
    );
    // J6 错误码分段。
    s.add(
        "J6-错误码分段",
        PR_EBADF > SEG_BASE && PR_LEDGER < SEG_BASE + 0x20,
        "三个错误码全部落在段基址页内",
    );
    // J7 错误码互异。
    s.add(
        "J7-错误码互异",
        PR_EBADF != PR_EINVAL && PR_EINVAL != PR_LEDGER && PR_EBADF != PR_LEDGER,
        "三个错误码两两不同",
    );
    // J8 账目对平。
    s.add("J8-账目对平", d3.ledger_ok(), "千轮之后三类计数与轮次、日志逐项对平");
    // J9 发送账与成功轮一致。
    s.add("J9-发送账一致", d3.sent == d3.ok_hits, "成功记账条数等于成功轮数");
    // J10 fold 稳定。
    let f = d3.fold();
    let clone = d3.clone();
    s.add("J10-fold稳定", clone.fold() == f, "克隆后指纹逐位一致");
    // J11 渲染非空。
    let txt = d3.render();
    s.add("J11-渲染非空", txt.len() > 10 && txt.contains("PrioDesk"), "台级渲染含类型名与计数");
    // J12 关闭后再开：新描述符独立。
    let mut d6 = PrioDesk::new();
    let a6 = d6.open_write();
    let _ = d6.close(a6);
    let b6 = d6.open_write();
    s.add(
        "J12-重开独立",
        d6.fd_ok(b6) && a6 != b6,
        "关闭后重开得到独立可用描述符",
    );
    // J13 动态：千轮明细。
    s.add_dyn(
        "J13-千轮明细",
        good == PR_ROUNDS,
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
    fn unx10943_mq_send_einval_flow() {
        // 流程：开 fd → 合法发送 → 只读拒 → 关闭拒 → 优先级越界。
        let mut d = PrioDesk::new();
        let fdw = d.open_write();
        let fdr = d.open_read();
        assert_eq!(d.mq_send(fdw, 5, 1), Ok(1));
        assert!(matches!(d.mq_send(fdr, 5, 2), Err(x) if x == PR_EBADF));
        assert!(d.close(fdw));
        assert!(matches!(d.mq_send(fdw, 5, 3), Err(x) if x == PR_EBADF));
        let fd2 = d.open_write();
        assert!(matches!(d.mq_send(fd2, PRIO_MAX, 4), Err(x) if x == PR_EINVAL));
        assert!(matches!(d.mq_send(fd2, PRIO_MAX - 1, 5), Ok(_)));
        assert!(d.ledger_ok());
        assert!(inject_four().iter().all(|x| *x));
    }

    #[test]
    fn unx10943_checks_green() {
        let s = run_unx10943_checks();
        assert!(s.all_passed(), "UNX-F10943 判据未全绿");
    }
}
