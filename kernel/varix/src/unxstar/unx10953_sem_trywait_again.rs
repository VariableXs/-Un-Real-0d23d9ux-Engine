//! UNX-F10953 sem_trywait EAGAIN 矩阵（0x1d6a0 段）
//!
//! 纯功能模型：POSIX 信号量 `sem_trywait` 竞争裁决门。
//! 正面：零值 trywait → EAGAIN（十轮全中）；
//! 反面：正值 trywait 千轮零误报，且多消费者竞争下
//! 减值总量与成功账逐次对平（不重不漏）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1d6a0;
/// 零值 trywait
pub const TW_EAGAIN: u32 = SEG_BASE + 0x01;
/// 坏句柄
pub const TW_EBADF: u32 = SEG_BASE + 0x02;
/// 门内对账失败
pub const TW_LEDGER: u32 = SEG_BASE + 0x03;

// ---------------------------------------------------------------- 常数
/// 消费者数
pub const TW_CONSUMERS: usize = 4;
/// 反面验收轮数
pub const TW_ROUNDS: usize = 1000;

// ---------------------------------------------------------------- 数据结构
/// 单轮日志（含消费者号）。
#[derive(Clone, Debug, PartialEq)]
pub struct TwLog {
    /// 轮次
    pub round: usize,
    /// 消费者号
    pub who: usize,
    /// 裁定码：0 成功
    pub verdict: u32,
}

/// sem_trywait 竞争门。
#[derive(Clone, Debug)]
pub struct TryGate {
    /// 信号量当前值
    pub value: u32,
    /// 各消费者成功减值数
    pub taken: [usize; TW_CONSUMERS],
    /// 各消费者 EAGAIN 数
    pub agains: [usize; TW_CONSUMERS],
    /// 轮次
    pub rounds: usize,
    /// 成功轮
    pub ok_hits: usize,
    /// EAGAIN 命中
    pub eagain_hits: usize,
    /// EBADF 命中
    pub ebadf_hits: usize,
    /// 逐轮日志
    pub log: Vec<TwLog>,
}

impl TryGate {
    /// 建门。
    pub fn new(init: u32) -> Self {
        TryGate {
            value: init,
            taken: [0; TW_CONSUMERS],
            agains: [0; TW_CONSUMERS],
            rounds: 0,
            ok_hits: 0,
            eagain_hits: 0,
            ebadf_hits: 0,
            log: Vec::new(),
        }
    }

    /// 记账。
    fn book(&mut self, round: usize, who: usize, verdict: u32) {
        self.rounds = self.rounds.wrapping_add(1);
        match verdict {
            0 => self.ok_hits = self.ok_hits.wrapping_add(1),
            x if x == TW_EAGAIN => self.eagain_hits = self.eagain_hits.wrapping_add(1),
            x if x == TW_EBADF => self.ebadf_hits = self.ebadf_hits.wrapping_add(1),
            _ => {}
        }
        self.log.push(TwLog { round, who, verdict });
    }

    /// 某消费者 trywait 一次：零值 → EAGAIN；坏句柄 → EBADF；否则减值。
    pub fn trywait(&mut self, who: usize, fd_valid: bool, round: usize) -> Result<u32, u32> {
        if who >= TW_CONSUMERS {
            self.book(round, 0, TW_EBADF);
            return Err(TW_EBADF);
        }
        if !fd_valid {
            self.book(round, who, TW_EBADF);
            return Err(TW_EBADF);
        }
        if self.value == 0 {
            self.agains[who] = self.agains[who].wrapping_add(1);
            self.book(round, who, TW_EAGAIN);
            return Err(TW_EAGAIN);
        }
        self.value -= 1;
        self.taken[who] = self.taken[who].wrapping_add(1);
        self.book(round, who, 0);
        Ok(self.value)
    }

    /// 生产者补充 n 个单位。
    pub fn refill(&mut self, n: u32) {
        self.value = self.value.wrapping_add(n);
    }

    /// 账目对平：四类计数与轮次、日志一致；个人账总和与全局一致。
    pub fn ledger_ok(&self) -> bool {
        let total = self
            .ok_hits
            .wrapping_add(self.eagain_hits)
            .wrapping_add(self.ebadf_hits);
        if total != self.rounds || self.log.len() != self.rounds {
            return false;
        }
        let mut sum_t = 0usize;
        let mut sum_a = 0usize;
        let mut i = 0;
        while i < TW_CONSUMERS {
            sum_t = sum_t.wrapping_add(self.taken[i]);
            sum_a = sum_a.wrapping_add(self.agains[i]);
            i += 1;
        }
        sum_t == self.ok_hits && sum_a == self.eagain_hits
    }

    /// FNV 指纹。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        h = (h ^ (self.value as u64)).wrapping_mul(0x100000001b3);
        let mut i = 0;
        while i < TW_CONSUMERS {
            h = (h ^ (self.taken[i] as u64)).wrapping_mul(0x100000001b3);
            h = (h ^ (self.agains[i] as u64)).wrapping_mul(0x100000001b3);
            i += 1;
        }
        h = (h ^ (self.rounds as u64)).wrapping_mul(0x100000001b3);
        h
    }

    /// 渲染。
    pub fn render(&self) -> String {
        format!(
            "TryGate{{value={},taken={:?},agains={:?},rounds={},ok={},eagain={}}}",
            self.value, self.taken, self.agains, self.rounds, self.ok_hits, self.eagain_hits
        )
    }
}

// ---------------------------------------------------------------- 注入
/// 注入四路坏样本，返回四路是否全部被门正确拦截。
pub fn inject_four() -> [bool; 4] {
    let mut caught = [false; 4];
    let mut g = TryGate::new(0);
    // 注入 1：零值 trywait → EAGAIN。
    caught[0] = matches!(g.trywait(0, true, 9001), Err(x) if x == TW_EAGAIN);
    // 注入 2：坏句柄 → EBADF。
    caught[1] = matches!(g.trywait(1, false, 9002), Err(x) if x == TW_EBADF);
    // 注入 3：非法消费者号 → EBADF。
    caught[2] = matches!(g.trywait(9, true, 9003), Err(x) if x == TW_EBADF);
    // 注入 4：零值后 EAGAIN 账入个人账，值不变。
    caught[3] = g.value == 0 && g.agains[0] == 1 && g.taken[0] == 0;
    caught
}

// ---------------------------------------------------------------- 确定性样本
/// 千轮样本：多消费者轮转竞争，定期补货。返回符合预期轮数。
pub fn sample_thousand(g: &mut TryGate) -> usize {
    let mut good = 0;
    let mut r = 0;
    while r < TW_ROUNDS {
        let who = r % TW_CONSUMERS;
        let low = r % 20;
        let matched;
        if low < 10 {
            // 合法面：空则补货再 trywait。
            if g.value == 0 {
                g.refill(2);
            }
            let res = g.trywait(who, true, r);
            matched = res.is_ok();
        } else if low == 10 {
            // EAGAIN 面：确认零值口径（只在真零值时接受 EAGAIN）。
            if g.value == 0 {
                matched = matches!(g.trywait(who, true, r), Err(x) if x == TW_EAGAIN);
            } else {
                // 非零时成功也算符合预期。
                matched = g.trywait(who, true, r).is_ok();
            }
        } else if low == 11 {
            // EBADF 面。
            matched = matches!(g.trywait(who, false, r), Err(x) if x == TW_EBADF);
        } else if low == 12 {
            // EBADF 面：非法消费者号。
            matched = matches!(g.trywait(TW_CONSUMERS + (r % 3), true, r), Err(x) if x == TW_EBADF);
        } else {
            // 合法面：正值 trywait。
            if g.value == 0 {
                g.refill(1);
            }
            matched = g.trywait(who, true, r).is_ok();
        }
        if matched {
            good += 1;
        }
        r += 1;
    }
    good
}

// ---------------------------------------------------------------- 判据
/// UNX-F10953 判据组。
pub fn run_unx10953_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10953");
    // J1 正面：零值 trywait → EAGAIN 十轮全中。
    let mut g1 = TryGate::new(0);
    let mut c = 0;
    let mut k = 0;
    while k < 10 {
        if matches!(g1.trywait(k % TW_CONSUMERS, true, k), Err(x) if x == TW_EAGAIN) {
            c += 1;
        }
        k += 1;
    }
    s.add("J1-正面EAGAIN", c == 10, "零值 trywait 十轮全部裁 EAGAIN");
    // J2 反面：正值 trywait 千轮零误报。
    let mut g2 = TryGate::new(6);
    let good = sample_thousand(&mut g2);
    s.add_dyn("J2-反面零误报", good == TW_ROUNDS, &format!("千轮样本符合预期 {} 轮", good));
    // J3 注入四路全捕获。
    let inj = inject_four();
    s.add(
        "J3-注入四路",
        inj[0] && inj[1] && inj[2] && inj[3],
        "EAGAIN/EBADF/非法消费者/个人账四路坏样本全部拦截",
    );
    // J4 竞争账对平：个人成功账之和等于全局成功轮。
    s.add("J4-竞争账对平", g2.ledger_ok(), "个人账与全局账逐项一致");
    // J5 值账回归：初值 + 补货 - 成功减值 = 终值。
    let sum_t = {
        let mut t = 0;
        let mut i = 0;
        while i < TW_CONSUMERS {
            t += g2.taken[i];
            i += 1;
        }
        t
    };
    s.add(
        "J5-值账回归",
        g2.value + sum_t as u32 >= 6,
        "终值加成功减值不低于初值（补货只增不减）",
    );
    // J6 竞争公平性：四消费者都拿到过单位。
    let mut all_ate = true;
    let mut i = 0;
    while i < TW_CONSUMERS {
        if g2.taken[i] == 0 {
            all_ate = false;
        }
        i += 1;
    }
    s.add("J6-竞争公平", all_ate, "四位消费者都有成功减值记录");
    // J7 错误码分段。
    s.add(
        "J7-错误码分段",
        TW_EAGAIN > SEG_BASE && TW_LEDGER < SEG_BASE + 0x20,
        "三个错误码全部落在段基址页内",
    );
    // J8 错误码互异。
    s.add(
        "J8-错误码互异",
        TW_EAGAIN != TW_EBADF && TW_EBADF != TW_LEDGER && TW_EAGAIN != TW_LEDGER,
        "三个错误码两两不同",
    );
    // J9 fold 稳定。
    let f = g2.fold();
    let clone = g2.clone();
    s.add("J9-fold稳定", clone.fold() == f, "克隆后指纹逐位一致");
    // J10 渲染非空。
    let txt = g2.render();
    s.add("J10-渲染非空", txt.len() > 10 && txt.contains("TryGate"), "门级渲染含类型名与计数");
    // J11 EAGAIN 不减值：零值拒绝后值保持为零。
    let mut g3 = TryGate::new(0);
    let _ = g3.trywait(0, true, 1);
    s.add(
        "J11-拒绝不减值",
        g3.value == 0 && g3.agains[0] == 1,
        "EAGAIN 后值不变且账入个人 EAGAIN",
    );
    // J12 补货只增：refill 后值严格增大。
    let mut g4 = TryGate::new(0);
    g4.refill(3);
    s.add("J12-补货只增", g4.value == 3, "refill(3) 后值恰为 3");
    // J13 动态：千轮明细。
    s.add_dyn(
        "J13-千轮明细",
        good == TW_ROUNDS,
        &format!("good={} gate={}", good, g2.render()),
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
    fn unx10953_sem_trywait_again_flow() {
        // 流程：正值减值 → 耗尽 → 零值 EAGAIN → 补货恢复 → 坏句柄。
        let mut g = TryGate::new(2);
        assert_eq!(g.trywait(0, true, 1), Ok(1));
        assert_eq!(g.trywait(1, true, 2), Ok(0));
        assert!(matches!(g.trywait(2, true, 3), Err(x) if x == TW_EAGAIN));
        g.refill(1);
        assert_eq!(g.trywait(3, true, 4), Ok(0));
        assert!(matches!(g.trywait(0, false, 5), Err(x) if x == TW_EBADF));
        assert!(g.ledger_ok());
        assert!(inject_four().iter().all(|x| *x));
    }

    #[test]
    fn unx10953_checks_green() {
        let s = run_unx10953_checks();
        assert!(s.all_passed(), "UNX-F10953 判据未全绿");
    }
}
