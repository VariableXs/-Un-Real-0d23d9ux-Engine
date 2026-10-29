//! UNX-F10946 mq_notify EBUSY/EINVAL 矩阵（0x1d5c0 段）
//!
//! 纯功能模型：POSIX 消息队列异步通知注册裁决板。
//! 两错误面：① 队列已有注册者时他人再注册 → EBUSY；
//! ② 通知载荷非法（既无信号又无线程，sigev 口径双零）→ EINVAL。
//! 反面千轮样本只走合法注册/注销循环，零误报。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1d5c0;
/// 重复注册
pub const NF_EBUSY: u32 = SEG_BASE + 0x01;
/// 通知载荷非法
pub const NF_EINVAL: u32 = SEG_BASE + 0x02;
/// 非注册者注销
pub const NF_EACCES: u32 = SEG_BASE + 0x03;
/// 坏描述符
pub const NF_EBADF: u32 = SEG_BASE + 0x04;
/// 板内对账失败
pub const NF_LEDGER: u32 = SEG_BASE + 0x05;

// ---------------------------------------------------------------- 常数
/// 反面验收轮数
pub const NF_ROUNDS: usize = 1000;

// ---------------------------------------------------------------- 数据结构
/// 一条通知注册。
#[derive(Clone, Debug, PartialEq)]
pub struct NotifyReg {
    /// 注册进程
    pub pid: u32,
    /// 通知信号号
    pub sig: u32,
    /// 通知线程号（0 表示无）
    pub tid: u32,
}

/// 单轮日志。
#[derive(Clone, Debug, PartialEq)]
pub struct NfLog {
    /// 轮次
    pub round: usize,
    /// 裁定码：0 成功
    pub verdict: u32,
}

/// mq_notify 裁决板（单队列口径）。
#[derive(Clone, Debug)]
pub struct NotifyBoard {
    /// 当前注册（None 表示无人注册）
    pub reg: Option<NotifyReg>,
    /// 描述符有效性
    pub fd_valid: bool,
    /// 通知触发次数
    pub fired: usize,
    /// 轮次
    pub rounds: usize,
    /// 成功轮
    pub ok_hits: usize,
    /// EBUSY 命中
    pub ebusy_hits: usize,
    /// EINVAL 命中
    pub einval_hits: usize,
    /// EACCES 命中
    pub eacces_hits: usize,
    /// EBADF 命中
    pub ebadf_hits: usize,
    /// 逐轮日志
    pub log: Vec<NfLog>,
}

impl NotifyBoard {
    /// 建板。
    pub fn new(fd_valid: bool) -> Self {
        NotifyBoard {
            reg: None,
            fd_valid,
            fired: 0,
            rounds: 0,
            ok_hits: 0,
            ebusy_hits: 0,
            einval_hits: 0,
            eacces_hits: 0,
            ebadf_hits: 0,
            log: Vec::new(),
        }
    }

    /// 记账。
    fn book(&mut self, round: usize, verdict: u32) {
        self.rounds = self.rounds.wrapping_add(1);
        match verdict {
            0 => self.ok_hits = self.ok_hits.wrapping_add(1),
            x if x == NF_EBUSY => self.ebusy_hits = self.ebusy_hits.wrapping_add(1),
            x if x == NF_EINVAL => self.einval_hits = self.einval_hits.wrapping_add(1),
            x if x == NF_EACCES => self.eacces_hits = self.eacces_hits.wrapping_add(1),
            x if x == NF_EBADF => self.ebadf_hits = self.ebadf_hits.wrapping_add(1),
            _ => {}
        }
        self.log.push(NfLog { round, verdict });
    }

    /// 裁决一次注册。判定序：
    /// ① 坏描述符 → EBADF；② 载荷双零 → EINVAL；
    /// ③ 已有注册且注册者不同 → EBUSY；④ 同进程重注册视为改挂（成功）。
    pub fn notify_register(&mut self, pid: u32, sig: u32, tid: u32, round: usize) -> Result<(), u32> {
        if !self.fd_valid {
            self.book(round, NF_EBADF);
            return Err(NF_EBADF);
        }
        if sig == 0 && tid == 0 {
            self.book(round, NF_EINVAL);
            return Err(NF_EINVAL);
        }
        match &self.reg {
            Some(r) if r.pid != pid => {
                self.book(round, NF_EBUSY);
                Err(NF_EBUSY)
            }
            _ => {
                self.reg = Some(NotifyReg { pid, sig, tid });
                self.book(round, 0);
                Ok(())
            }
        }
    }

    /// 注销：只有注册者本人可注销。
    pub fn notify_cancel(&mut self, pid: u32, round: usize) -> Result<(), u32> {
        if !self.fd_valid {
            self.book(round, NF_EBADF);
            return Err(NF_EBADF);
        }
        match &self.reg {
            None => {
                self.book(round, 0);
                Ok(())
            }
            Some(r) if r.pid == pid => {
                self.reg = None;
                self.book(round, 0);
                Ok(())
            }
            Some(_) => {
                self.book(round, NF_EACCES);
                Err(NF_EACCES)
            }
        }
    }

    /// 消息到达触发通知：仅当有人注册。
    pub fn on_arrival(&mut self) -> bool {
        if self.reg.is_some() {
            self.fired = self.fired.wrapping_add(1);
            true
        } else {
            false
        }
    }

    /// 账目对平。
    pub fn ledger_ok(&self) -> bool {
        let total = self
            .ok_hits
            .wrapping_add(self.ebusy_hits)
            .wrapping_add(self.einval_hits)
            .wrapping_add(self.eacces_hits)
            .wrapping_add(self.ebadf_hits);
        total == self.rounds && self.log.len() == self.rounds
    }

    /// FNV 指纹。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        match &self.reg {
            Some(r) => {
                h = (h ^ (r.pid as u64)).wrapping_mul(0x100000001b3);
                h = (h ^ (r.sig as u64)).wrapping_mul(0x100000001b3);
                h = (h ^ (r.tid as u64)).wrapping_mul(0x100000001b3);
            }
            None => {
                h = (h ^ 0xdead).wrapping_mul(0x100000001b3);
            }
        }
        h = (h ^ (self.fired as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ (self.rounds as u64)).wrapping_mul(0x100000001b3);
        h
    }

    /// 渲染。
    pub fn render(&self) -> String {
        let reg_txt = match &self.reg {
            Some(r) => format!("pid={}", r.pid),
            None => "none".to_string(),
        };
        format!(
            "NotifyBoard{{reg={},fd_valid={},fired={},rounds={},ok={},ebusy={},einval={}}}",
            reg_txt, self.fd_valid as u32, self.fired, self.rounds, self.ok_hits,
            self.ebusy_hits, self.einval_hits
        )
    }
}

// ---------------------------------------------------------------- 注入
/// 注入四路坏样本，返回四路是否全部被板正确拦截。
pub fn inject_four() -> [bool; 4] {
    let mut caught = [false; 4];
    let mut b = NotifyBoard::new(true);
    // 注入 1：坏描述符 → EBADF。
    caught[0] = matches!(NotifyBoard::new(false).notify_register(1, 14, 0, 9001), Err(x) if x == NF_EBADF);
    // 注入 2：载荷双零 → EINVAL。
    caught[1] = matches!(b.notify_register(1, 0, 0, 9002), Err(x) if x == NF_EINVAL);
    // 注入 3：他人重复注册 → EBUSY。
    let _ = b.notify_register(1, 14, 0, 9003);
    caught[2] = matches!(b.notify_register(2, 15, 0, 9004), Err(x) if x == NF_EBUSY);
    // 注入 4：非注册者注销 → EACCES。
    caught[3] = matches!(b.notify_cancel(3, 9005), Err(x) if x == NF_EACCES);
    caught
}

// ---------------------------------------------------------------- 确定性样本
/// 千轮样本：注册-触发-注销循环为主，三错误面轮转。
/// 返回符合预期轮数。
pub fn sample_thousand(b: &mut NotifyBoard) -> usize {
    let mut good = 0;
    let mut r = 0;
    while r < NF_ROUNDS {
        let low = r % 10;
        let matched;
        if low < 5 {
            // 合法面：注册（覆盖式）→ 触发 → 注销。
            let ok1 = b.notify_register(0x46, 14 + (r % 8) as u32, 0, r).is_ok();
            let fired = b.on_arrival();
            let ok2 = b.notify_cancel(0x46, r).is_ok();
            matched = ok1 && ok2 && fired;
        } else if low == 5 {
            // EBUSY 面。
            let _ = b.notify_register(0x46, 14, 0, r);
            let hit = matches!(b.notify_register(0x47, 15, 0, r), Err(x) if x == NF_EBUSY);
            let _ = b.notify_cancel(0x46, r);
            matched = hit;
        } else if low == 6 {
            // EINVAL 面。
            matched = matches!(b.notify_register(0x46, 0, 0, r), Err(x) if x == NF_EINVAL);
        } else if low == 7 {
            // EACCES 面。
            let _ = b.notify_register(0x46, 14, 0, r);
            let hit = matches!(b.notify_cancel(0x48, r), Err(x) if x == NF_EACCES);
            let _ = b.notify_cancel(0x46, r);
            matched = hit;
        } else if low == 8 {
            // EBADF 面（独立坏描述符板）。
            let mut bad = NotifyBoard::new(false);
            matched = matches!(bad.notify_register(1, 14, 0, r), Err(x) if x == NF_EBADF);
        } else {
            // 合法面：空队列到达不触发。
            let clean = b.reg.is_none();
            let fired = b.on_arrival();
            matched = clean && !fired;
        }
        if matched {
            good += 1;
        }
        r += 1;
    }
    good
}

// ---------------------------------------------------------------- 判据
/// UNX-F10946 判据组。
pub fn run_unx10946_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10946");
    // J1 正面之一：重复注册 → EBUSY 十轮全中。
    let mut b1 = NotifyBoard::new(true);
    let mut c = 0;
    let mut k = 0;
    while k < 10 {
        let _ = b1.notify_register(1, 14, 0, 100 + k);
        if matches!(b1.notify_register(2 + k as u32, 15, 0, k), Err(x) if x == NF_EBUSY) {
            c += 1;
        }
        k += 1;
    }
    s.add("J1-正面EBUSY", c == 10, "他人重复注册十轮全部裁 EBUSY");
    // J1 正面之二：非法通知 → EINVAL 十轮全中。
    let mut b2 = NotifyBoard::new(true);
    let mut c = 0;
    let mut k = 0;
    while k < 10 {
        let r = if k % 2 == 0 {
            b2.notify_register(1, 0, 0, k)
        } else {
            b2.notify_register(1, 0, 0, k)
        };
        if matches!(r, Err(x) if x == NF_EINVAL) {
            c += 1;
        }
        k += 1;
    }
    s.add("J1-正面EINVAL", c == 10, "双零载荷十轮全部裁 EINVAL");
    // J2 反面：千轮合法注册零误报。
    let mut b3 = NotifyBoard::new(true);
    let good = sample_thousand(&mut b3);
    s.add_dyn("J2-反面零误报", good == NF_ROUNDS, &format!("千轮样本符合预期 {} 轮", good));
    // J3 注入四路全捕获。
    let inj = inject_four();
    s.add(
        "J3-注入四路",
        inj[0] && inj[1] && inj[2] && inj[3],
        "EBADF/EINVAL/EBUSY/EACCES 四路坏样本全部拦截",
    );
    // J4 同进程重注册为改挂：不报 EBUSY 且载荷更新。
    let mut b4 = NotifyBoard::new(true);
    let _ = b4.notify_register(5, 14, 0, 1);
    let r4 = b4.notify_register(5, 20, 0, 2);
    let sig_now = match &b4.reg {
        Some(x) => x.sig,
        None => 0,
    };
    s.add(
        "J4-同进程改挂",
        r4.is_ok() && sig_now == 20,
        "同进程重注册更新载荷",
    );
    // J5 触发纪律：注册时到达触发，注销后不触发。
    let mut b5 = NotifyBoard::new(true);
    let _ = b5.notify_register(7, 14, 0, 1);
    let f1 = b5.on_arrival();
    let _ = b5.notify_cancel(7, 2);
    let f2 = b5.on_arrival();
    s.add("J5-触发纪律", f1 && !f2 && b5.fired == 1, "注册触发一次、注销后零触发");
    // J6 错误码分段。
    s.add(
        "J6-错误码分段",
        NF_EBUSY > SEG_BASE && NF_LEDGER < SEG_BASE + 0x20,
        "五个错误码全部落在段基址页内",
    );
    // J7 错误码互异。
    let codes = [NF_EBUSY, NF_EINVAL, NF_EACCES, NF_EBADF, NF_LEDGER];
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
    s.add("J7-错误码互异", distinct, "五个错误码两两不同");
    // J8 账目对平。
    s.add("J8-账目对平", b3.ledger_ok(), "千轮之后五类计数与轮次、日志逐项对平");
    // J9 fold 稳定。
    let f = b3.fold();
    let clone = b3.clone();
    s.add("J9-fold稳定", clone.fold() == f, "克隆后指纹逐位一致");
    // J10 渲染非空。
    let txt = b3.render();
    s.add("J10-渲染非空", txt.len() > 10 && txt.contains("NotifyBoard"), "板级渲染含类型名与计数");
    // J11 空注销幂等：无人注册时注销仍成功。
    let mut b6 = NotifyBoard::new(true);
    s.add("J11-空注销幂等", b6.notify_cancel(1, 1).is_ok() && b6.reg.is_none(), "无注册注销成功且状态不变");
    // J12 注册互斥窗口：EBUSY 后原注册者仍有效。
    let holder = match &b1.reg {
        Some(r) => r.pid,
        None => 0,
    };
    s.add("J12-原注册保持", holder == 1, "EBUSY 拒绝后原注册者未被顶替");
    // J13 动态：千轮明细。
    s.add_dyn(
        "J13-千轮明细",
        good == NF_ROUNDS,
        &format!("good={} board={}", good, b3.render()),
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
    fn unx10946_mq_notify_busy_flow() {
        // 流程：注册 → 触发 → 他人注册 EBUSY → 双零 EINVAL → 注销 → 幂等。
        let mut b = NotifyBoard::new(true);
        assert!(b.notify_register(1, 14, 0, 1).is_ok());
        assert!(b.on_arrival());
        assert!(matches!(b.notify_register(2, 15, 0, 2), Err(x) if x == NF_EBUSY));
        assert!(matches!(b.notify_register(1, 0, 0, 3), Err(x) if x == NF_EINVAL));
        assert!(b.notify_cancel(1, 4).is_ok());
        assert!(!b.on_arrival());
        assert!(b.notify_cancel(1, 5).is_ok());
        assert!(b.ledger_ok());
        assert!(inject_four().iter().all(|x| *x));
    }

    #[test]
    fn unx10946_checks_green() {
        let s = run_unx10946_checks();
        assert!(s.all_passed(), "UNX-F10946 判据未全绿");
    }
}
