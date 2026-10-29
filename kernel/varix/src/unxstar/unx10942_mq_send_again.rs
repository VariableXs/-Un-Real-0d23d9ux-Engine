//! UNX-F10942 mq_send EAGAIN/EMSGSIZE 矩阵（0x1d540 段）
//!
//! 纯功能模型：POSIX 消息队列 `mq_send` 裁决门。
//! 两错误面：① 非阻塞模式下队列已满 → EAGAIN；② msg_len 超出
//! 队列单条上限 → EMSGSIZE。发送成功按优先级入账，队账逐条对平。
//! 反面千轮样本在限额内发送零误报。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1d540;
/// 非阻塞满队
pub const MS_EAGAIN: u32 = SEG_BASE + 0x01;
/// 消息超长
pub const MS_EMSGSIZE: u32 = SEG_BASE + 0x02;
/// 坏描述符
pub const MS_EBADF: u32 = SEG_BASE + 0x03;
/// 非法优先级
pub const MS_EINVAL: u32 = SEG_BASE + 0x04;
/// 门内对账失败
pub const MS_LEDGER: u32 = SEG_BASE + 0x05;

// ---------------------------------------------------------------- 常数
/// 队列容量（条）
pub const Q_CAP: usize = 8;
/// 单条消息上限
pub const MSG_MAX: usize = 64;
/// 非阻塞标志
pub const SND_NONBLOCK: u32 = 0x1;
/// 反面验收轮数
pub const MS_ROUNDS: usize = 1000;

// ---------------------------------------------------------------- 数据结构
/// 队内一条消息。
#[derive(Clone, Debug, PartialEq)]
pub struct MqMsg {
    /// 优先级（大者先出）
    pub prio: u32,
    /// 载荷长度
    pub len: usize,
    /// 载荷首字节（模型不存全量）
    pub head: u8,
}

impl MqMsg {
    /// FNV 指纹。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        h = (h ^ (self.prio as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ (self.len as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ (self.head as u64)).wrapping_mul(0x100000001b3);
        h
    }
}

/// 单轮发送日志。
#[derive(Clone, Debug, PartialEq)]
pub struct SendLog {
    /// 轮次
    pub round: usize,
    /// 裁定码：0 成功
    pub verdict: u32,
}

/// mq_send 裁决门。
#[derive(Clone, Debug)]
pub struct SendGate {
    /// 队内消息（按优先级序维护）
    pub q: Vec<MqMsg>,
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
    /// EINVAL 命中
    pub einval_hits: usize,
    /// 逐轮日志
    pub log: Vec<SendLog>,
}

impl SendGate {
    /// 建门。
    pub fn new(nonblock: bool) -> Self {
        SendGate {
            q: Vec::new(),
            nonblock,
            rounds: 0,
            ok_hits: 0,
            eagain_hits: 0,
            emsgsize_hits: 0,
            ebadf_hits: 0,
            einval_hits: 0,
            log: Vec::new(),
        }
    }

    /// 记账。
    fn book(&mut self, round: usize, verdict: u32) {
        self.rounds = self.rounds.wrapping_add(1);
        match verdict {
            0 => self.ok_hits = self.ok_hits.wrapping_add(1),
            x if x == MS_EAGAIN => self.eagain_hits = self.eagain_hits.wrapping_add(1),
            x if x == MS_EMSGSIZE => self.emsgsize_hits = self.emsgsize_hits.wrapping_add(1),
            x if x == MS_EBADF => self.ebadf_hits = self.ebadf_hits.wrapping_add(1),
            x if x == MS_EINVAL => self.einval_hits = self.einval_hits.wrapping_add(1),
            _ => {}
        }
        self.log.push(SendLog { round, verdict });
    }

    /// 裁决一次 mq_send。判定序：
    /// ① msg_len 超 MSG_MAX → EMSGSIZE；② 优先级越界（>= MQ_PRIO_MAX）→ EINVAL；
    /// ③ 满队：非阻塞 → EAGAIN（阻塞位返回 0 占位）；④ 按优先级入队。
    pub fn mq_send(&mut self, fd_valid: bool, len: usize, prio: u32, head: u8, flags: u32, round: usize) -> Result<usize, u32> {
        if !fd_valid {
            self.book(round, MS_EBADF);
            return Err(MS_EBADF);
        }
        if len > MSG_MAX {
            self.book(round, MS_EMSGSIZE);
            return Err(MS_EMSGSIZE);
        }
        if prio >= 32768 {
            self.book(round, MS_EINVAL);
            return Err(MS_EINVAL);
        }
        if self.q.len() >= Q_CAP {
            if self.nonblock || flags & SND_NONBLOCK != 0 {
                self.book(round, MS_EAGAIN);
                return Err(MS_EAGAIN);
            }
            self.book(round, 0);
            return Ok(0);
        }
        // 按优先级降序插入。
        let msg = MqMsg { prio, len, head };
        let mut pos = self.q.len();
        let mut i = 0;
        while i < self.q.len() {
            if self.q[i].prio < prio {
                pos = i;
                break;
            }
            i += 1;
        }
        self.q.insert(pos, msg);
        self.book(round, 0);
        Ok(self.q.len())
    }

    /// 取最高优先级消息（模拟消费，供账目滚动）。
    pub fn drain_one(&mut self) -> Option<MqMsg> {
        if self.q.is_empty() {
            return None;
        }
        Some(self.q.remove(0))
    }

    /// 账目对平。
    pub fn ledger_ok(&self) -> bool {
        let total = self
            .ok_hits
            .wrapping_add(self.eagain_hits)
            .wrapping_add(self.emsgsize_hits)
            .wrapping_add(self.ebadf_hits)
            .wrapping_add(self.einval_hits);
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
            "SendGate{{q={},rounds={},ok={},eagain={},emsgsize={},ebadf={},einval={}}}",
            self.q.len(), self.rounds, self.ok_hits,
            self.eagain_hits, self.emsgsize_hits, self.ebadf_hits, self.einval_hits
        )
    }
}

// ---------------------------------------------------------------- 注入
/// 注入四路坏样本，返回四路是否全部被门正确拦截。
pub fn inject_four() -> [bool; 4] {
    let mut caught = [false; 4];
    let mut g = SendGate::new(true);
    // 注入 1：坏描述符 → EBADF。
    caught[0] = matches!(g.mq_send(false, 8, 1, 0, 0, 9001), Err(x) if x == MS_EBADF);
    // 注入 2：超长 → EMSGSIZE。
    caught[1] = matches!(g.mq_send(true, MSG_MAX + 1, 1, 0, 0, 9002), Err(x) if x == MS_EMSGSIZE);
    // 注入 3：优先级越界 → EINVAL。
    caught[2] = matches!(g.mq_send(true, 8, 32768, 0, 0, 9003), Err(x) if x == MS_EINVAL);
    // 注入 4：灌满后非阻塞发送 → EAGAIN 且队不膨胀。
    let mut i = 0;
    while i < Q_CAP {
        let _ = g.mq_send(true, 4, 1, 0, 0, 9100 + i);
        i += 1;
    }
    let before = g.q.len();
    caught[3] = matches!(g.mq_send(true, 4, 1, 0, 0, 9200), Err(x) if x == MS_EAGAIN)
        && g.q.len() == before;
    caught
}

// ---------------------------------------------------------------- 确定性样本
/// 千轮样本：限额内发送为主，四错误面轮转，定期消费防满。
/// 返回符合预期轮数。
pub fn sample_thousand(g: &mut SendGate) -> usize {
    let mut good = 0;
    let mut r = 0;
    while r < MS_ROUNDS {
        let low = r % 10;
        let res;
        let expect_ok;
        if low < 5 {
            // 合法面：小消息低优先级；队满先消费一条腾位。
            if g.q.len() >= Q_CAP {
                let _ = g.drain_one();
            }
            res = g.mq_send(true, 4 + (r % 8), (r % 4) as u32, (r % 251) as u8, 0, r);
            expect_ok = true;
        } else if low == 5 {
            // EAGAIN 面：先灌满，再非阻塞发送。
            while g.q.len() < Q_CAP {
                let _ = g.mq_send(true, 4, 1, 0, 0, r);
            }
            res = g.mq_send(true, 4, 1, 0, SND_NONBLOCK, r);
            expect_ok = false;
        } else if low == 6 {
            // EMSGSIZE 面。
            res = g.mq_send(true, MSG_MAX + 1 + (r % 5), 1, 0, 0, r);
            expect_ok = false;
        } else if low == 7 {
            // EBADF 面。
            res = g.mq_send(false, 4, 1, 0, 0, r);
            expect_ok = false;
        } else if low == 8 {
            // EINVAL 面。
            res = g.mq_send(true, 4, 32768 + (r % 9) as u32, 0, 0, r);
            expect_ok = false;
        } else {
            // 合法面：中优先级消息；同样先腾位再发。
            if g.q.len() >= Q_CAP {
                let _ = g.drain_one();
            }
            res = g.mq_send(true, 8, 8, 0x42, 0, r);
            expect_ok = true;
        }
        let matched = match &res {
            Ok(_) => expect_ok,
            Err(x) => {
                !expect_ok
                    && (*x == MS_EAGAIN
                        || *x == MS_EMSGSIZE
                        || *x == MS_EBADF
                        || *x == MS_EINVAL)
            }
        };
        if matched {
            good += 1;
        }
        // 定期消费，防合法面枯竭。
        if r % 24 == 0 {
            let _ = g.drain_one();
        }
        r += 1;
    }
    good
}

// ---------------------------------------------------------------- 判据
/// UNX-F10942 判据组。
pub fn run_unx10942_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10942");
    // J1 正面之一：非阻塞满队 → EAGAIN 十轮全中。
    let mut g1 = SendGate::new(true);
    let mut i = 0;
    while i < Q_CAP {
        let _ = g1.mq_send(true, 4, 1, 0, 0, 900 + i);
        i += 1;
    }
    let mut c = 0;
    let mut k = 0;
    while k < 10 {
        if matches!(g1.mq_send(true, 4, 1, 0, SND_NONBLOCK, k), Err(x) if x == MS_EAGAIN) {
            c += 1;
        }
        k += 1;
    }
    s.add("J1-正面EAGAIN", c == 10, "非阻塞满队十轮全部裁 EAGAIN");
    // J1 正面之二：msg_len 超 → EMSGSIZE 十轮全中。
    let mut g2 = SendGate::new(true);
    let mut c = 0;
    let mut k = 0;
    while k < 10 {
        if matches!(g2.mq_send(true, MSG_MAX + 1 + k, 1, 0, 0, k), Err(x) if x == MS_EMSGSIZE) {
            c += 1;
        }
        k += 1;
    }
    s.add("J1-正面EMSGSIZE", c == 10, "超长消息十轮全部裁 EMSGSIZE");
    // J2 反面：千轮限额内发送零误报。
    let mut g3 = SendGate::new(true);
    let good = sample_thousand(&mut g3);
    s.add_dyn("J2-反面零误报", good == MS_ROUNDS, &format!("千轮样本符合预期 {} 轮", good));
    // J3 注入四路全捕获。
    let inj = inject_four();
    s.add(
        "J3-注入四路",
        inj[0] && inj[1] && inj[2] && inj[3],
        "EBADF/EMSGSIZE/EINVAL/EAGAIN 四路坏样本全部拦截",
    );
    // J4 判定序：EMSGSIZE 先于 EAGAIN（满队且超长裁 EMSGSIZE）。
    let mut g4 = SendGate::new(true);
    let mut i = 0;
    while i < Q_CAP {
        let _ = g4.mq_send(true, 4, 1, 0, 0, i);
        i += 1;
    }
    s.add(
        "J4-判定序",
        matches!(g4.mq_send(true, MSG_MAX + 2, 1, 0, SND_NONBLOCK, 1), Err(x) if x == MS_EMSGSIZE),
        "超长与满队同现按序裁 EMSGSIZE",
    );
    // J5 错误码分段。
    s.add(
        "J5-错误码分段",
        MS_EAGAIN > SEG_BASE && MS_LEDGER < SEG_BASE + 0x20,
        "五个错误码全部落在段基址页内",
    );
    // J6 错误码互异。
    let codes = [MS_EAGAIN, MS_EMSGSIZE, MS_EBADF, MS_EINVAL, MS_LEDGER];
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
    s.add("J6-错误码互异", distinct, "五个错误码两两不同");
    // J7 账目对平。
    s.add("J7-账目对平", g3.ledger_ok(), "千轮之后五类计数与轮次、日志逐项对平");
    // J8 优先级序：高优先级先入队首。
    let mut g5 = SendGate::new(true);
    let _ = g5.mq_send(true, 4, 1, 1, 0, 1);
    let _ = g5.mq_send(true, 4, 9, 2, 0, 2);
    let _ = g5.mq_send(true, 4, 5, 3, 0, 3);
    let top = g5.q[0].prio;
    s.add("J8-优先级序", top == 9 && g5.q[1].prio == 5 && g5.q[2].prio == 1, "队首为最高优先级");
    // J9 fold 稳定。
    let f = g3.fold();
    let clone = g3.clone();
    s.add("J9-fold稳定", clone.fold() == f, "克隆后指纹逐位一致");
    // J10 消费回落。
    let n_before = g5.q.len();
    let drained = g5.drain_one();
    s.add(
        "J10-消费回落",
        drained.is_some() && g5.q.len() == n_before.saturating_sub(1),
        "消费后队账回落一条",
    );
    // J11 渲染非空。
    let txt = g3.render();
    s.add("J11-渲染非空", txt.len() > 10 && txt.contains("SendGate"), "门级渲染含类型名与计数");
    // J12 阻塞位占位：满队不带非阻塞位返回 0 且不入队。
    let mut g6 = SendGate::new(false);
    let mut i = 0;
    while i < Q_CAP {
        let _ = g6.mq_send(true, 4, 1, 0, 0, 900 + i);
        i += 1;
    }
    let r12 = g6.mq_send(true, 4, 1, 0, 0, 999);
    s.add(
        "J12-阻塞占位",
        matches!(r12, Ok(0)) && g6.q.len() == Q_CAP,
        "阻塞位满队占位返回零且队不膨胀",
    );
    // J13 动态：千轮明细。
    s.add_dyn(
        "J13-千轮明细",
        good == MS_ROUNDS,
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
    fn unx10942_mq_send_again_flow() {
        // 流程：合法发送 → 超长 → 坏 fd → 优先级越界 → 满队 EAGAIN。
        let mut g = SendGate::new(true);
        assert!(g.mq_send(true, 16, 2, 0x41, 0, 1).is_ok());
        assert!(matches!(g.mq_send(true, MSG_MAX + 1, 2, 0, 0, 2), Err(x) if x == MS_EMSGSIZE));
        assert!(matches!(g.mq_send(false, 8, 2, 0, 0, 3), Err(x) if x == MS_EBADF));
        assert!(matches!(g.mq_send(true, 8, 40000, 0, 0, 4), Err(x) if x == MS_EINVAL));
        let mut i = 0;
        while i < Q_CAP - 1 {
            let _ = g.mq_send(true, 4, 1, 0, 0, 100 + i);
            i += 1;
        }
        assert!(matches!(g.mq_send(true, 4, 1, 0, SND_NONBLOCK, 200), Err(x) if x == MS_EAGAIN));
        assert!(g.ledger_ok());
        assert!(inject_four().iter().all(|x| *x));
    }

    #[test]
    fn unx10942_checks_green() {
        let s = run_unx10942_checks();
        assert!(s.all_passed(), "UNX-F10942 判据未全绿");
    }
}
