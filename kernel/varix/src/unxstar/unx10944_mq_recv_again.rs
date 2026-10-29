//! UNX-F10944 mq_receive EAGAIN/EMSGSIZE 矩阵（0x1d580 段）
//!
//! 纯功能模型：POSIX 消息队列 `mq_receive` 裁决门。
//! 两错误面：① 非阻塞模式下队列为空 → EAGAIN；② 接收缓冲小于
//! 队列单条上限 → EMSGSIZE。接收按优先级出队，队账逐条对平。
//! 反面千轮样本在合法缓冲与存续队列下零误报。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1d580;
/// 非阻塞空队
pub const RC_EAGAIN: u32 = SEG_BASE + 0x01;
/// 缓冲不足
pub const RC_EMSGSIZE: u32 = SEG_BASE + 0x02;
/// 坏描述符
pub const RC_EBADF: u32 = SEG_BASE + 0x03;
/// 门内对账失败
pub const RC_LEDGER: u32 = SEG_BASE + 0x04;

// ---------------------------------------------------------------- 常数
/// 队列单条上限
pub const RQ_MSG_MAX: usize = 64;
/// 队列容量
pub const RQ_CAP: usize = 8;
/// 非阻塞标志
pub const RCV_NONBLOCK: u32 = 0x1;
/// 反面验收轮数
pub const RC_ROUNDS: usize = 1000;

// ---------------------------------------------------------------- 数据结构
/// 队内消息（接收侧镜像）。
#[derive(Clone, Debug, PartialEq)]
pub struct RcvMsg {
    /// 优先级
    pub prio: u32,
    /// 载荷长度
    pub len: usize,
    /// 载荷首字节
    pub head: u8,
}

impl RcvMsg {
    /// FNV 指纹。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        h = (h ^ (self.prio as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ (self.len as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ (self.head as u64)).wrapping_mul(0x100000001b3);
        h
    }
}

/// 单轮接收日志。
#[derive(Clone, Debug, PartialEq)]
pub struct RcvLog {
    /// 轮次
    pub round: usize,
    /// 裁定码：0 成功
    pub verdict: u32,
}

/// mq_receive 裁决门。
#[derive(Clone, Debug)]
pub struct RcvGate {
    /// 队内消息（队首为最高优先级）
    pub q: Vec<RcvMsg>,
    /// 非阻塞位
    pub nonblock: bool,
    /// 轮次
    pub rounds: usize,
    /// 成功轮
    pub ok_hits: usize,
    /// EAGAIN 命中
    pub eagain_hits: usize,
    /// EMSGSIZE 命中
    pub emsgsize_hits: usize,
    /// EBADF 命中
    pub ebadf_hits: usize,
    /// 逐轮日志
    pub log: Vec<RcvLog>,
}

impl RcvGate {
    /// 建门。
    pub fn new(nonblock: bool) -> Self {
        RcvGate {
            q: Vec::new(),
            nonblock,
            rounds: 0,
            ok_hits: 0,
            eagain_hits: 0,
            emsgsize_hits: 0,
            ebadf_hits: 0,
            log: Vec::new(),
        }
    }

    /// 灌入一条消息（生产侧，供样本铺队）。
    pub fn feed(&mut self, prio: u32, len: usize, head: u8) -> bool {
        if self.q.len() >= RQ_CAP {
            return false;
        }
        let mut pos = self.q.len();
        let mut i = 0;
        while i < self.q.len() {
            if self.q[i].prio < prio {
                pos = i;
                break;
            }
            i += 1;
        }
        self.q.insert(pos, RcvMsg { prio, len, head });
        true
    }

    /// 记账。
    fn book(&mut self, round: usize, verdict: u32) {
        self.rounds = self.rounds.wrapping_add(1);
        match verdict {
            0 => self.ok_hits = self.ok_hits.wrapping_add(1),
            x if x == RC_EAGAIN => self.eagain_hits = self.eagain_hits.wrapping_add(1),
            x if x == RC_EMSGSIZE => self.emsgsize_hits = self.emsgsize_hits.wrapping_add(1),
            x if x == RC_EBADF => self.ebadf_hits = self.ebadf_hits.wrapping_add(1),
            _ => {}
        }
        self.log.push(RcvLog { round, verdict });
    }

    /// 裁决一次 mq_receive。判定序：
    /// ① 坏描述符 → EBADF；② buf_len < RQ_MSG_MAX → EMSGSIZE；
    /// ③ 空队：非阻塞 → EAGAIN（阻塞位占位返回 0）；④ 出队。
    pub fn mq_receive(&mut self, fd_valid: bool, buf_len: usize, flags: u32, round: usize) -> Result<RcvMsg, u32> {
        if !fd_valid {
            self.book(round, RC_EBADF);
            return Err(RC_EBADF);
        }
        if buf_len < RQ_MSG_MAX {
            self.book(round, RC_EMSGSIZE);
            return Err(RC_EMSGSIZE);
        }
        if self.q.is_empty() {
            if self.nonblock || flags & RCV_NONBLOCK != 0 {
                self.book(round, RC_EAGAIN);
                return Err(RC_EAGAIN);
            }
            self.book(round, 0);
            return Err(0); // 阻塞占位：返回 Err(0) 表示挂起（模型约定）
        }
        let m = self.q.remove(0);
        self.book(round, 0);
        Ok(m)
    }

    /// 账目对平。
    pub fn ledger_ok(&self) -> bool {
        let total = self
            .ok_hits
            .wrapping_add(self.eagain_hits)
            .wrapping_add(self.emsgsize_hits)
            .wrapping_add(self.ebadf_hits);
        total == self.rounds && self.log.len() == self.rounds
    }

    /// FNV 指纹。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        let mut i = 0;
        while i < self.q.len() {
            h = (h ^ self.q[i].fold()).wrapping_mul(0x100000001b3);
            i += 1;
        }
        h = (h ^ (self.rounds as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ (self.ok_hits as u64)).wrapping_mul(0x100000001b3);
        h
    }

    /// 渲染。
    pub fn render(&self) -> String {
        format!(
            "RcvGate{{q={},rounds={},ok={},eagain={},emsgsize={},ebadf={}}}",
            self.q.len(), self.rounds, self.ok_hits,
            self.eagain_hits, self.emsgsize_hits, self.ebadf_hits
        )
    }
}

// ---------------------------------------------------------------- 注入
/// 注入四路坏样本，返回四路是否全部被门正确拦截。
pub fn inject_four() -> [bool; 4] {
    let mut caught = [false; 4];
    let mut g = RcvGate::new(true);
    // 注入 1：坏描述符 → EBADF。
    caught[0] = matches!(g.mq_receive(false, RQ_MSG_MAX, 0, 9001), Err(x) if x == RC_EBADF);
    // 注入 2：缓冲不足 → EMSGSIZE。
    caught[1] = matches!(g.mq_receive(true, RQ_MSG_MAX - 1, 0, 9002), Err(x) if x == RC_EMSGSIZE);
    // 注入 3：非阻塞空队 → EAGAIN。
    caught[2] = matches!(g.mq_receive(true, RQ_MSG_MAX, RCV_NONBLOCK, 9003), Err(x) if x == RC_EAGAIN);
    // 注入 4：缓冲不足优先于空队（判定序核销）。
    caught[3] = matches!(g.mq_receive(true, 8, RCV_NONBLOCK, 9004), Err(x) if x == RC_EMSGSIZE);
    caught
}

// ---------------------------------------------------------------- 确定性样本
/// 千轮样本：合法接收为主，三错误面轮转，定期补货防空。
/// 返回符合预期轮数。
pub fn sample_thousand(g: &mut RcvGate) -> usize {
    let mut good = 0;
    let mut r = 0;
    while r < RC_ROUNDS {
        let low = r % 10;
        let res;
        let expect_ok;
        if low < 6 {
            // 合法面：足额缓冲接收（空队先补一条，保证反面口径）。
            if g.q.is_empty() {
                let _ = g.feed(1, 8, 0x33);
            }
            res = g.mq_receive(true, RQ_MSG_MAX, 0, r);
            expect_ok = true;
        } else if low == 6 {
            // EAGAIN 面：非阻塞空队（先手动清空）。
            while !g.q.is_empty() {
                let _ = g.mq_receive(true, RQ_MSG_MAX, 0, r.wrapping_add(RC_ROUNDS));
            }
            res = g.mq_receive(true, RQ_MSG_MAX, RCV_NONBLOCK, r);
            expect_ok = false;
        } else if low == 7 {
            // EMSGSIZE 面。
            res = g.mq_receive(true, RQ_MSG_MAX - 1 - (r % 9), 0, r);
            expect_ok = false;
        } else if low == 8 {
            // EBADF 面。
            res = g.mq_receive(false, RQ_MSG_MAX, 0, r);
            expect_ok = false;
        } else {
            // 合法面：补货后接收。
            let _ = g.feed((r % 8) as u32, 16, (r % 251) as u8);
            res = g.mq_receive(true, RQ_MSG_MAX, 0, r);
            expect_ok = true;
        }
        let matched = match &res {
            Ok(_) => expect_ok,
            Err(x) => {
                !expect_ok && (*x == RC_EAGAIN || *x == RC_EMSGSIZE || *x == RC_EBADF)
            }
        };
        if matched {
            good += 1;
        }
        // 定期补货。
        if r % 32 == 0 {
            let _ = g.feed(1, 8, 0x5a);
        }
        r += 1;
    }
    good
}

// ---------------------------------------------------------------- 判据
/// UNX-F10944 判据组。
pub fn run_unx10944_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10944");
    // J1 正面之一：非阻塞空队 → EAGAIN 十轮全中。
    let mut g1 = RcvGate::new(true);
    let mut c = 0;
    let mut k = 0;
    while k < 10 {
        if matches!(g1.mq_receive(true, RQ_MSG_MAX, RCV_NONBLOCK, k), Err(x) if x == RC_EAGAIN) {
            c += 1;
        }
        k += 1;
    }
    s.add("J1-正面EAGAIN", c == 10, "非阻塞空队十轮全部裁 EAGAIN");
    // J1 正面之二：缓冲小于 msgsize → EMSGSIZE 十轮全中。
    let mut g2 = RcvGate::new(true);
    let mut c = 0;
    let mut k = 0;
    while k < 10 {
        if matches!(g2.mq_receive(true, RQ_MSG_MAX - 1 - k, 0, k), Err(x) if x == RC_EMSGSIZE) {
            c += 1;
        }
        k += 1;
    }
    s.add("J1-正面EMSGSIZE", c == 10, "缓冲不足十轮全部裁 EMSGSIZE");
    // J2 反面：千轮合法接收零误报。
    let mut g3 = RcvGate::new(true);
    let mut i = 0;
    while i < 6 {
        let _ = g3.feed((i % 5) as u32, 16, (i * 7) as u8);
        i += 1;
    }
    let good = sample_thousand(&mut g3);
    s.add_dyn("J2-反面零误报", good == RC_ROUNDS, &format!("千轮样本符合预期 {} 轮", good));
    // J3 注入四路全捕获。
    let inj = inject_four();
    s.add(
        "J3-注入四路",
        inj[0] && inj[1] && inj[2] && inj[3],
        "EBADF/EMSGSIZE/EAGAIN/判定序四路坏样本全部拦截",
    );
    // J4 判定序：EMSGSIZE 先于 EAGAIN（空队且缓冲不足裁 EMSGSIZE）。
    let mut g4 = RcvGate::new(true);
    s.add(
        "J4-判定序",
        matches!(g4.mq_receive(true, 8, RCV_NONBLOCK, 1), Err(x) if x == RC_EMSGSIZE),
        "缓冲不足与空队同现按序裁 EMSGSIZE",
    );
    // J5 边界：恰好 RQ_MSG_MAX 合法。
    let mut g5 = RcvGate::new(true);
    let _ = g5.feed(1, 8, 0x41);
    s.add(
        "J5-缓冲边界",
        matches!(g5.mq_receive(true, RQ_MSG_MAX, 0, 1), Ok(_)),
        "恰好足额缓冲接收成功",
    );
    // J6 错误码分段。
    s.add(
        "J6-错误码分段",
        RC_EAGAIN > SEG_BASE && RC_LEDGER < SEG_BASE + 0x20,
        "四个错误码全部落在段基址页内",
    );
    // J7 错误码互异。
    let codes = [RC_EAGAIN, RC_EMSGSIZE, RC_EBADF, RC_LEDGER];
    let mut distinct = true;
    let mut a = 0;
    while a < codes.len() {
        let mut b = a + 1;
        while b < codes.len() {
            if codes[a] == codes[b] {
                distinct = false;
            }
            b += 1;
        }
        a += 1;
    }
    s.add("J7-错误码互异", distinct, "四个错误码两两不同");
    // J8 账目对平。
    s.add("J8-账目对平", g3.ledger_ok(), "千轮之后四类计数与轮次、日志逐项对平");
    // J9 优先级出队：队首最高。
    let mut g6 = RcvGate::new(true);
    let _ = g6.feed(1, 8, 1);
    let _ = g6.feed(7, 8, 2);
    let _ = g6.feed(4, 8, 3);
    let top = g6.mq_receive(true, RQ_MSG_MAX, 0, 1);
    s.add(
        "J9-优先级出队",
        matches!(&top, Ok(m) if m.prio == 7),
        "队首为最高优先级消息",
    );
    // J10 fold 稳定。
    let f = g3.fold();
    let clone = g3.clone();
    s.add("J10-fold稳定", clone.fold() == f, "克隆后指纹逐位一致");
    // J11 渲染非空。
    let txt = g3.render();
    s.add("J11-渲染非空", txt.len() > 10 && txt.contains("RcvGate"), "门级渲染含类型名与计数");
    // J12 阻塞占位：空队不带非阻塞位返回占位码 0。
    let mut g7 = RcvGate::new(false);
    let r12 = g7.mq_receive(true, RQ_MSG_MAX, 0, 1);
    s.add("J12-阻塞占位", matches!(r12, Err(0)), "阻塞位空队占位返回零码");
    // J13 动态：千轮明细。
    s.add_dyn(
        "J13-千轮明细",
        good == RC_ROUNDS,
        &format!("good={} gate={}", good, g3.render()),
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
    fn unx10944_mq_recv_again_flow() {
        // 流程：灌队 → 合法接收 → 空队 EAGAIN → 缓冲不足 → 坏 fd。
        let mut g = RcvGate::new(true);
        let _ = g.feed(2, 16, 0x41);
        let got = g.mq_receive(true, RQ_MSG_MAX, 0, 1);
        assert!(matches!(&got, Ok(m) if m.head == 0x41));
        assert!(matches!(g.mq_receive(true, RQ_MSG_MAX, RCV_NONBLOCK, 2), Err(x) if x == RC_EAGAIN));
        assert!(matches!(g.mq_receive(true, 8, 0, 3), Err(x) if x == RC_EMSGSIZE));
        assert!(matches!(g.mq_receive(false, RQ_MSG_MAX, 0, 4), Err(x) if x == RC_EBADF));
        assert!(g.ledger_ok());
        assert!(inject_four().iter().all(|x| *x));
    }

    #[test]
    fn unx10944_checks_green() {
        let s = run_unx10944_checks();
        assert!(s.all_passed(), "UNX-F10944 判据未全绿");
    }
}
