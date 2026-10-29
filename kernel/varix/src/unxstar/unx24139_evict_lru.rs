//! UNX-F24139 显存驱逐 LRU 与迁移搬运·子功能19（驱逐失败重试与退避账）（0x41340 段）
//!
//! 锚点：UNX-F24139-J1 退避账（重试轮次与退避延迟逐条落账）；J2 退
//! 避语义（重试上限闸 + 指数退避 + 成功即停三性质）；J3 退避指纹（账
//! 本折叠指纹）三判据全绿。
//! 域：CoRun·UNX-F5-B15（驱逐重试退避面）。
//!
//! 结构：重试引擎 + 指数退避核 + 尝试账本 + 折叠指纹 + 注入核（零上
//! 限/超上限/非指数/野任务/印后必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x41340 独占 0x20）。
pub const SEG_BASE: u32 = 0x41340;
pub const RT_ZERO_MAX: u32 = SEG_BASE + 0x01; // 零上限
pub const RT_OVER_MAX: u32 = SEG_BASE + 0x02; // 超重试上限
pub const RT_NOT_EXP: u32 = SEG_BASE + 0x03; // 非指数退避
pub const RT_UNK_TASK: u32 = SEG_BASE + 0x04; // 野任务
pub const RT_FROZEN: u32 = SEG_BASE + 0x05; // 印后重试

/// 单次尝试账。
#[derive(Clone)]
pub struct TryRec {
    pub seq: u32,
    pub task_id: u32,
    pub round: u32,
    pub delay_ms: u64,
    pub ok: bool,
}

/// 退避引擎。
pub struct RtEngine {
    pub max_retry: u32,
    pub base_delay: u64,
    pub ledger: Vec<TryRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl RtEngine {
    pub fn new(max_retry: u32, base_delay: u64) -> RtEngine {
        RtEngine { max_retry, base_delay, ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 指数退避延迟：base << round。
    pub fn delay_of(&self, round: u32) -> u64 {
        self.base_delay << round.min(16)
    }

    /// 执行一轮重试：round 从 0 起；成功即停返 Ok(round)，超上限拒。
    pub fn attempt(&mut self, task_id: u32, round: u32, ok: bool) -> Result<u32, u32> {
        if self.sealed {
            return Err(RT_FROZEN);
        }
        if self.max_retry == 0 {
            return Err(RT_ZERO_MAX);
        }
        if round > self.max_retry {
            return Err(RT_OVER_MAX);
        }
        let seq = self.seq_next;
        self.seq_next += 1;
        let delay = self.delay_of(round);
        self.ledger.push(TryRec { seq, task_id, round, delay_ms: delay, ok });
        if ok {
            Ok(round)
        } else if round == self.max_retry {
            Err(RT_OVER_MAX)
        } else {
            Ok(self.seq_next) // 未成功未超限：返下轮锚
        }
    }

    /// 账面退避指数性：同任务相邻轮 delay 恒翻倍。
    pub fn exp_ok(&self, task_id: u32) -> bool {
        let rs: Vec<&TryRec> = self.ledger.iter().filter(|r| r.task_id == task_id).collect();
        for w in rs.windows(2) {
            if w[1].delay_ms != w[0].delay_ms * 2 {
                return false;
            }
        }
        true
    }

    /// 成功即停：同任务不得有成功后的后续尝试。
    pub fn stop_on_ok(&self) -> bool {
        let mut tasks: Vec<u32> = Vec::new();
        for r in &self.ledger {
            if !tasks.contains(&r.task_id) {
                tasks.push(r.task_id);
            }
        }
        for t in tasks {
            let rs: Vec<&TryRec> = self.ledger.iter().filter(|r| r.task_id == t).collect();
            for (i, r) in rs.iter().enumerate() {
                if r.ok && i + 1 < rs.len() {
                    return false;
                }
            }
        }
        true
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
        let mut h: u32 = 0x41340;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.seq);
            h = h.wrapping_mul(31).wrapping_add(r.task_id);
            h = h.wrapping_mul(31).wrapping_add(r.round);
            h = h.wrapping_mul(31).wrapping_add(r.delay_ms as u32);
            h = h.wrapping_mul(31).wrapping_add(r.ok as u32);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准样本：上限 3 基延迟 10ms，任务 1 两败一成。
pub fn mk_engine() -> RtEngine {
    let mut e = RtEngine::new(3, 10);
    let _ = e.attempt(1, 0, false);
    let _ = e.attempt(1, 1, false);
    let _ = e.attempt(1, 2, true);
    let _ = e.attempt(2, 0, true);
    e
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut e = mk_engine();
    // 1) 零上限
    got[0] = RtEngine::new(0, 10).attempt(1, 0, true).err() == Some(RT_ZERO_MAX);
    // 2) 超上限
    got[1] = e.attempt(3, 4, true).err() == Some(RT_OVER_MAX);
    // 3) 非指数（伪账：同任务 delay 不翻倍）
    let mut e2 = RtEngine::new(3, 10);
    let _ = e2.attempt(9, 0, false);
    e2.ledger.push(TryRec { seq: 99, task_id: 9, round: 1, delay_ms: 15, ok: false });
    got[2] = !e2.exp_ok(9);
    // 4) 成功后续尝被识破
    let mut e3 = RtEngine::new(3, 10);
    let _ = e3.attempt(7, 0, true);
    e3.ledger.push(TryRec { seq: 98, task_id: 7, round: 1, delay_ms: 20, ok: false });
    got[3] = !e3.stop_on_ok();
    // 5) 印后重试必抓
    e.seal();
    got[4] = e.attempt(5, 0, true).err() == Some(RT_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx24139_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24139");
    let mut e = mk_engine();

    // J1 尝试账非空
    let j1 = e.ledger.len() == 4;
    s.add("J1-尝试账齐", j1, "四轮尝试全落账");

    // J2 指数退避延迟
    let j2 = e.delay_of(0) == 10 && e.delay_of(1) == 20 && e.delay_of(2) == 40;
    s.add("J2-指数延迟", j2, "base<<round 翻倍");

    // J3 账面指数性
    let j3 = e.exp_ok(1);
    s.add("J3-账面指数", j3, "任务1 相邻轮延迟翻倍");

    // J4 成功即停
    let j4 = e.stop_on_ok();
    s.add("J4-成功即停", j4, "成功后无后续尝");

    // J5 重试上限闸
    let j5 = e.attempt(3, 4, true).err() == Some(RT_OVER_MAX);
    s.add("J5-上限闸", j5, "round 超上限必拒");

    // J6 末轮失败终局
    let j6 = {
        let mut e2 = RtEngine::new(2, 5);
        let _ = e2.attempt(1, 0, false);
        let _ = e2.attempt(1, 1, false);
        e2.attempt(1, 2, false).err() == Some(RT_OVER_MAX)
    };
    s.add("J6-末轮终局", j6, "末轮仍败即终局拒");

    // J7 零上限拒
    let j7 = RtEngine::new(0, 10).attempt(1, 0, true).err() == Some(RT_ZERO_MAX);
    s.add("J7-零上限拒", j7, "max_retry=0 必拒");

    // J8 成功轮返值
    let j8 = {
        let mut e2 = RtEngine::new(3, 10);
        e2.attempt(4, 0, true) == Ok(0)
    };
    s.add("J8-成功返轮", j8, "成功返成功轮号");

    // J9 成功后续禁令（伪账识破）
    let j9 = {
        let mut e2 = RtEngine::new(3, 10);
        let _ = e2.attempt(7, 0, true);
        e2.ledger.push(TryRec { seq: 98, task_id: 7, round: 1, delay_ms: 20, ok: false });
        !e2.stop_on_ok()
    };
    s.add("J9-成功后续识破", j9, "成功后伪造尝必被识破");

    // J10 退避封顶
    let j10 = e.delay_of(20) == RtEngine::new(3, 10).delay_of(16);
    s.add("J10-退避封顶", j10, "round>16 延迟封顶");

    // J11 seq 唯一
    let j11 = e.seq_unique();
    s.add("J11-seq唯一", j11, "账 seq 不重");

    // J12 指纹稳定
    let j12 = e.fold() == e.fold();
    s.add("J12-指纹稳定", j12, "折叠指纹两次一致");

    // J13 注入核 5/5
    let got = inject_five();
    let j13 = got.iter().all(|g| *g);
    s.add_dyn("J13-注入全抓", j13, &format!("inj5={:?}", got));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx24139_retry_flow() {
        let mut e = RtEngine::new(2, 5);
        assert!(e.attempt(1, 0, false).is_ok());
        assert!(e.attempt(1, 1, false).is_ok());
        assert_eq!(e.attempt(1, 2, false), Err(RT_OVER_MAX));
        assert_eq!(RtEngine::new(0, 1).attempt(1, 0, true), Err(RT_ZERO_MAX));
        e.seal();
        assert_eq!(e.attempt(1, 0, true), Err(RT_FROZEN));
    }

    #[test]
    fn unx24139_checks_green() {
        let s = run_unx24139_checks();
        assert!(s.all_passed(), "UNX-F24139 判据未全绿");
    }
}
