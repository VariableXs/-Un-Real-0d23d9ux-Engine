//! UNX-F10951 sem_wait EINTR/EAGAIN 矩阵（0x1d660 段）
//!
//! 纯功能模型：POSIX 信号量 `sem_wait`/`sem_trywait` 裁决门。
//! 两错误面：① 零值非阻塞（NOWAIT 类）→ EAGAIN；② 阻塞等待
//! 期间到达信号 → EINTR（信号被消费）。可减值时零误报。
//! 反面千轮样本在值充足且无信号时零误报。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1d660;
/// 零值非阻塞
pub const SW_EAGAIN: u32 = SEG_BASE + 0x01;
/// 阻塞中断
pub const SW_EINTR: u32 = SEG_BASE + 0x02;
/// 坏句柄
pub const SW_EBADF: u32 = SEG_BASE + 0x03;
/// 门内对账失败
pub const SW_LEDGER: u32 = SEG_BASE + 0x04;

// ---------------------------------------------------------------- 常数
/// 信号量值上限
pub const SW_VALUE_MAX: u32 = 0xffff;
/// 反面验收轮数
pub const SW_ROUNDS: usize = 1000;

// ---------------------------------------------------------------- 数据结构
/// 单轮日志。
#[derive(Clone, Debug, PartialEq)]
pub struct SwLog {
    /// 轮次
    pub round: usize,
    /// 裁定码：0 成功
    pub verdict: u32,
}

/// sem_wait 裁决门。
#[derive(Clone, Debug)]
pub struct WaitGate {
    /// 信号量当前值
    pub value: u32,
    /// 非阻塞位
    pub nonblock: bool,
    /// 挂起信号位图（位号即信号号）
    pub signals: u32,
    /// 消费的信号号账
    pub intr_log: Vec<u32>,
    /// 成功减值次数
    pub decs: usize,
    /// 成功加值次数
    pub incs: usize,
    /// 轮次
    pub rounds: usize,
    /// 成功轮
    pub ok_hits: usize,
    /// EAGAIN 命中
    pub eagain_hits: usize,
    /// EINTR 命中
    pub eintr_hits: usize,
    /// EBADF 命中
    pub ebadf_hits: usize,
    /// 逐轮日志
    pub log: Vec<SwLog>,
}

impl WaitGate {
    /// 建门。
    pub fn new(init: u32, nonblock: bool) -> Self {
        WaitGate {
            value: init,
            nonblock,
            signals: 0,
            intr_log: Vec::new(),
            decs: 0,
            incs: 0,
            rounds: 0,
            ok_hits: 0,
            eagain_hits: 0,
            eintr_hits: 0,
            ebadf_hits: 0,
            log: Vec::new(),
        }
    }

    /// 记账。
    fn book(&mut self, round: usize, verdict: u32) {
        self.rounds = self.rounds.wrapping_add(1);
        match verdict {
            0 => self.ok_hits = self.ok_hits.wrapping_add(1),
            x if x == SW_EAGAIN => self.eagain_hits = self.eagain_hits.wrapping_add(1),
            x if x == SW_EINTR => self.eintr_hits = self.eintr_hits.wrapping_add(1),
            x if x == SW_EBADF => self.ebadf_hits = self.ebadf_hits.wrapping_add(1),
            _ => {}
        }
        self.log.push(SwLog { round, verdict });
    }

    /// 派信号。
    pub fn raise(&mut self, sig: u32) {
        if sig < 32 {
            self.signals |= 1u32.wrapping_shl(sig);
        }
    }

    /// 取最低挂起信号并消费；无信号返回 None。
    pub fn take_signal(&mut self) -> Option<u32> {
        let mut i = 0;
        while i < 32 {
            if self.signals & (1u32.wrapping_shl(i)) != 0 {
                self.signals &= !(1u32.wrapping_shl(i));
                self.intr_log.push(i);
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 挂起信号数。
    pub fn pending(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < 32 {
            if self.signals & (1u32.wrapping_shl(i)) != 0 {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// sem_wait：判定序 ① 坏句柄 → EBADF；② 值 > 0 → 减值成功；
    /// ③ 值 = 0：非阻塞 → EAGAIN；④ 有挂起信号 → EINTR（消费）；
    /// ⑤ 否则阻塞占位（返回 Ok(0) 不减值）。
    pub fn sem_wait(&mut self, fd_valid: bool, round: usize) -> Result<u32, u32> {
        if !fd_valid {
            self.book(round, SW_EBADF);
            return Err(SW_EBADF);
        }
        if self.value > 0 {
            self.value -= 1;
            self.decs = self.decs.wrapping_add(1);
            self.book(round, 0);
            return Ok(self.value);
        }
        if self.nonblock {
            self.book(round, SW_EAGAIN);
            return Err(SW_EAGAIN);
        }
        if self.pending() > 0 {
            let _ = self.take_signal();
            self.book(round, SW_EINTR);
            return Err(SW_EINTR);
        }
        self.book(round, 0);
        Ok(0)
    }

    /// sem_trywait：零值一律 EAGAIN（非阻塞语义）。
    pub fn sem_trywait(&mut self, fd_valid: bool, round: usize) -> Result<u32, u32> {
        if !fd_valid {
            self.book(round, SW_EBADF);
            return Err(SW_EBADF);
        }
        if self.value == 0 {
            self.book(round, SW_EAGAIN);
            return Err(SW_EAGAIN);
        }
        self.value -= 1;
        self.decs = self.decs.wrapping_add(1);
        self.book(round, 0);
        Ok(self.value)
    }

    /// sem_post：加值，越上限占位拒绝。
    pub fn sem_post(&mut self, fd_valid: bool, round: usize) -> Result<u32, u32> {
        if !fd_valid {
            self.book(round, SW_EBADF);
            return Err(SW_EBADF);
        }
        if self.value >= SW_VALUE_MAX {
            self.book(round, 0);
            return Ok(self.value);
        }
        self.value += 1;
        self.incs = self.incs.wrapping_add(1);
        self.book(round, 0);
        Ok(self.value)
    }

    /// 账目对平。
    pub fn ledger_ok(&self) -> bool {
        let total = self
            .ok_hits
            .wrapping_add(self.eagain_hits)
            .wrapping_add(self.eintr_hits)
            .wrapping_add(self.ebadf_hits);
        total == self.rounds && self.log.len() == self.rounds
    }

    /// FNV 指纹。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        h = (h ^ (self.value as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ (self.signals as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ (self.decs as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ (self.incs as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ (self.rounds as u64)).wrapping_mul(0x100000001b3);
        h
    }

    /// 渲染。
    pub fn render(&self) -> String {
        format!(
            "WaitGate{{value={},sigs={},decs={},incs={},rounds={},ok={},eagain={},eintr={}}}",
            self.value, self.pending(), self.decs, self.incs, self.rounds,
            self.ok_hits, self.eagain_hits, self.eintr_hits
        )
    }
}

// ---------------------------------------------------------------- 注入
/// 注入四路坏样本，返回四路是否全部被门正确拦截。
pub fn inject_four() -> [bool; 4] {
    let mut caught = [false; 4];
    let mut g = WaitGate::new(0, true);
    // 注入 1：坏句柄 → EBADF。
    caught[0] = matches!(g.sem_wait(false, 9001), Err(x) if x == SW_EBADF);
    // 注入 2：零值非阻塞 → EAGAIN。
    caught[1] = matches!(g.sem_wait(true, 9002), Err(x) if x == SW_EAGAIN);
    // 注入 3：零值 trywait → EAGAIN（阻塞门同样适用）。
    let mut g2 = WaitGate::new(0, false);
    caught[2] = matches!(g2.sem_trywait(true, 9003), Err(x) if x == SW_EAGAIN);
    // 注入 4：零值阻塞 + 挂起信号 → EINTR 且信号被消费。
    let mut g3 = WaitGate::new(0, false);
    g3.raise(9);
    let r4 = g3.sem_wait(true, 9004);
    caught[3] = matches!(r4, Err(x) if x == SW_EINTR) && g3.pending() == 0
        && matches!(g3.intr_log.last(), Some(&9u32));
    caught
}

// ---------------------------------------------------------------- 确定性样本
/// 千轮样本：可减值为主，四错误面轮转，post 回补。
/// 返回符合预期轮数。
pub fn sample_thousand(g: &mut WaitGate) -> usize {
    let mut good = 0;
    let mut r = 0;
    while r < SW_ROUNDS {
        let low = r % 10;
        let matched;
        if low < 5 {
            // 合法面：可减值（空则回补）。是否真正减值看减值账，
            // 而不是返回值：从 1 减到 0 时 Ok(0) 也是合法成功。
            if g.value == 0 {
                let _ = g.sem_post(true, r);
            }
            let before = g.decs;
            let res = g.sem_wait(true, r);
            matched = res.is_ok() && g.decs == before + 1;
        } else if low == 5 {
            // EAGAIN 面：先清值再非阻塞等。
            let saved = g.value;
            g.value = 0;
            let hit = matches!(g.sem_wait(true, r), Err(x) if x == SW_EAGAIN);
            g.value = saved;
            matched = hit;
        } else if low == 6 {
            // EINTR 面：清值 + 派信号 + 阻塞等（临时切阻塞）。
            let saved_nb = g.nonblock;
            let saved_v = g.value;
            g.value = 0;
            g.nonblock = false;
            g.raise((3 + (r % 5)) as u32);
            let hit = matches!(g.sem_wait(true, r), Err(x) if x == SW_EINTR);
            g.nonblock = saved_nb;
            g.value = saved_v;
            matched = hit;
        } else if low == 7 {
            // EBADF 面。
            matched = matches!(g.sem_wait(false, r), Err(x) if x == SW_EBADF);
        } else if low == 8 {
            // 合法面：trywait 可减值。
            if g.value == 0 {
                let _ = g.sem_post(true, r);
            }
            matched = g.sem_trywait(true, r).is_ok();
        } else {
            // 合法面：post 加值。
            let before = g.value;
            let res = g.sem_post(true, r);
            matched = matches!(res, Ok(v) if v == before + 1 || before == SW_VALUE_MAX);
        }
        if matched {
            good += 1;
        }
        r += 1;
    }
    good
}

// ---------------------------------------------------------------- 判据
/// UNX-F10951 判据组。
pub fn run_unx10951_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10951");
    // J1 正面之一：零值 NOWAIT 类 → EAGAIN 十轮全中。
    let mut g1 = WaitGate::new(0, true);
    let mut c = 0;
    let mut k = 0;
    while k < 10 {
        if matches!(g1.sem_wait(true, k), Err(x) if x == SW_EAGAIN) {
            c += 1;
        }
        k += 1;
    }
    s.add("J1-正面EAGAIN", c == 10, "零值非阻塞十轮全部裁 EAGAIN");
    // J1 正面之二：阻塞中断 → EINTR 十轮全中（信号逐次消费）。
    let mut g2 = WaitGate::new(0, false);
    let mut c = 0;
    let mut k = 0;
    while k < 10 {
        g2.raise(k as u32);
        if matches!(g2.sem_wait(true, k), Err(x) if x == SW_EINTR) {
            c += 1;
        }
        k += 1;
    }
    s.add(
        "J1-正面EINTR",
        c == 10 && g2.pending() == 0 && g2.intr_log.len() == 10,
        "阻塞中断十轮全部裁 EINTR 且信号逐一消费",
    );
    // J2 反面：千轮可减值零误报。
    let mut g3 = WaitGate::new(8, true);
    let good = sample_thousand(&mut g3);
    s.add_dyn("J2-反面零误报", good == SW_ROUNDS, &format!("千轮样本符合预期 {} 轮", good));
    // J3 注入四路全捕获。
    let inj = inject_four();
    s.add(
        "J3-注入四路",
        inj[0] && inj[1] && inj[2] && inj[3],
        "EBADF/EAGAIN/trywait/EINTR 四路坏样本全部拦截",
    );
    // J4 判定序：非阻塞 EAGAIN 先于信号 EINTR。
    let mut g4 = WaitGate::new(0, true);
    g4.raise(5);
    s.add(
        "J4-判定序",
        matches!(g4.sem_wait(true, 1), Err(x) if x == SW_EAGAIN) && g4.pending() == 1,
        "非阻塞优先且信号保留",
    );
    // J5 值域边界：post 到上限不再增长。
    let mut g5 = WaitGate::new(SW_VALUE_MAX, true);
    let r5 = g5.sem_post(true, 1);
    s.add(
        "J5-上限饱和",
        matches!(r5, Ok(v) if v == SW_VALUE_MAX) && g5.value == SW_VALUE_MAX,
        "上限值 post 不再增长",
    );
    // J6 错误码分段。
    s.add(
        "J6-错误码分段",
        SW_EAGAIN > SEG_BASE && SW_LEDGER < SEG_BASE + 0x20,
        "四个错误码全部落在段基址页内",
    );
    // J7 错误码互异。
    let codes = [SW_EAGAIN, SW_EINTR, SW_EBADF, SW_LEDGER];
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
    // J9 减值账与值回归：decs - incs 与初末值差一致。
    let drift = (g3.decs as i64).wrapping_sub(g3.incs as i64);
    s.add(
        "J9-值账回归",
        drift == 8 - g3.value as i64,
        "减值减加值之差与初末值差一致",
    );
    // J10 fold 稳定。
    let f = g3.fold();
    let clone = g3.clone();
    s.add("J10-fold稳定", clone.fold() == f, "克隆后指纹逐位一致");
    // J11 渲染非空。
    let txt = g3.render();
    s.add("J11-渲染非空", txt.len() > 10 && txt.contains("WaitGate"), "门级渲染含类型名与计数");
    // J12 trywait 不受 nonblock 位影响：零值恒 EAGAIN。
    let mut g6 = WaitGate::new(0, false);
    s.add(
        "J12-trywait口径",
        matches!(g6.sem_trywait(true, 1), Err(x) if x == SW_EAGAIN),
        "trywait 零值恒裁 EAGAIN",
    );
    // J13 动态：千轮明细。
    s.add_dyn(
        "J13-千轮明细",
        good == SW_ROUNDS,
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
    fn unx10951_sem_wait_matrix_flow() {
        // 流程：可减值成功 → 清零 EAGAIN → 派信号阻塞 EINTR → 消费 → 坏句柄。
        let mut g = WaitGate::new(2, false);
        assert_eq!(g.sem_wait(true, 1), Ok(1));
        g.value = 0;
        g.raise(2);
        assert!(matches!(g.sem_wait(true, 2), Err(x) if x == SW_EINTR));
        g.nonblock = true;
        assert!(matches!(g.sem_wait(true, 3), Err(x) if x == SW_EAGAIN));
        g.nonblock = false;
        g.raise(7);
        assert!(matches!(g.sem_wait(true, 4), Err(x) if x == SW_EINTR));
        assert!(g.pending() == 0);
        assert!(matches!(g.sem_wait(false, 5), Err(x) if x == SW_EBADF));
        assert!(g.ledger_ok());
        assert!(inject_four().iter().all(|x| *x));
    }

    #[test]
    fn unx10951_checks_green() {
        let s = run_unx10951_checks();
        assert!(s.all_passed(), "UNX-F10951 判据未全绿");
    }
}
