//! UNX-F10947 mq_getattr/mq_setattr 错误矩阵（0x1d5e0 段）
//!
//! 纯功能模型：POSIX 消息队列属性读写裁决台。
//! 两错误面：① 坏描述符 → EBADF；② setattr 携带非阻塞位以外的
//! 未知位 → EINVAL。getattr 回读与队列账逐项一致。
//! 反面千轮样本只走合法读改写，零误报。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1d5e0;
/// 坏描述符
pub const GA_EBADF: u32 = SEG_BASE + 0x01;
/// 属性位非法
pub const GA_EINVAL: u32 = SEG_BASE + 0x02;
/// 台内对账失败
pub const GA_LEDGER: u32 = SEG_BASE + 0x03;

// ---------------------------------------------------------------- 常数
/// 属主可置位：非阻塞
pub const ATTR_NONBLOCK: u32 = 0x1;
/// 属性合法掩码
pub const ATTR_MASK: u32 = 0x1;
/// 队列容量（getattr 展示位）
pub const ATTR_MAX_COUNT: usize = 8;
/// 单条上限（getattr 展示位）
pub const ATTR_MAX_MSG: usize = 64;
/// 反面验收轮数
pub const GA_ROUNDS: usize = 1000;

// ---------------------------------------------------------------- 数据结构
/// 属性快照。
#[derive(Clone, Debug, PartialEq)]
pub struct MqAttr {
    /// 标志位（当前仅非阻塞）
    pub flags: u32,
    /// 最大条数
    pub max_count: usize,
    /// 单条上限
    pub max_msg: usize,
    /// 当前条数
    pub cur: usize,
}

impl MqAttr {
    /// FNV 指纹。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        h = (h ^ (self.flags as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ (self.max_count as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ (self.max_msg as u64)).wrapping_mul(0x100000001b3);
        h = (h ^ (self.cur as u64)).wrapping_mul(0x100000001b3);
        h
    }

    /// 渲染。
    pub fn render(&self) -> String {
        format!(
            "MqAttr{{flags={:#x},max_count={},max_msg={},cur={}}}",
            self.flags, self.max_count, self.max_msg, self.cur
        )
    }
}

/// 描述符账条目。
#[derive(Clone, Debug, PartialEq)]
pub struct GaEnt {
    /// 描述符号
    pub fd: u32,
    /// 是否开着
    pub open: bool,
    /// 所属队列属性
    pub attr: MqAttr,
}

/// 单轮日志。
#[derive(Clone, Debug, PartialEq)]
pub struct GaLog {
    /// 轮次
    pub round: usize,
    /// 裁定码：0 成功
    pub verdict: u32,
}

/// 属性裁决台。
#[derive(Clone, Debug)]
pub struct AttrDesk {
    /// 描述符账
    pub fds: Vec<GaEnt>,
    /// 轮次
    pub rounds: usize,
    /// 成功轮
    pub ok_hits: usize,
    /// EBADF 命中
    pub ebadf_hits: usize,
    /// EINVAL 命中
    pub einval_hits: usize,
    /// 逐轮日志
    pub log: Vec<GaLog>,
}

impl AttrDesk {
    /// 建台：n 条描述符统一初始属性。
    pub fn new(n: usize, nonblock: bool) -> Self {
        let mut fds = Vec::new();
        let mut i = 0;
        while i < n {
            fds.push(GaEnt {
                fd: i as u32,
                open: true,
                attr: MqAttr {
                    flags: if nonblock { ATTR_NONBLOCK } else { 0 },
                    max_count: ATTR_MAX_COUNT,
                    max_msg: ATTR_MAX_MSG,
                    cur: 0,
                },
            });
            i += 1;
        }
        AttrDesk {
            fds,
            rounds: 0,
            ok_hits: 0,
            ebadf_hits: 0,
            einval_hits: 0,
            log: Vec::new(),
        }
    }

    /// 描述符查找。
    pub fn find(&self, fd: u32) -> Option<usize> {
        let mut i = 0;
        while i < self.fds.len() {
            if self.fds[i].fd == fd && self.fds[i].open {
                return Some(i);
            }
            i += 1;
        }
        None
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

    /// 记账。
    fn book(&mut self, round: usize, verdict: u32) {
        self.rounds = self.rounds.wrapping_add(1);
        match verdict {
            0 => self.ok_hits = self.ok_hits.wrapping_add(1),
            x if x == GA_EBADF => self.ebadf_hits = self.ebadf_hits.wrapping_add(1),
            x if x == GA_EINVAL => self.einval_hits = self.einval_hits.wrapping_add(1),
            _ => {}
        }
        self.log.push(GaLog { round, verdict });
    }

    /// mq_getattr：坏描述符 → EBADF，否则回读快照。
    pub fn getattr(&mut self, fd: u32, round: usize) -> Result<MqAttr, u32> {
        match self.find(fd) {
            None => {
                self.book(round, GA_EBADF);
                Err(GA_EBADF)
            }
            Some(ix) => {
                let a = self.fds[ix].attr.clone();
                self.book(round, 0);
                Ok(a)
            }
        }
    }

    /// mq_setattr：判定序 ① 坏描述符 → EBADF；② 未知位 → EINVAL；
    /// ③ 生效（仅 flags 可改）。
    pub fn setattr(&mut self, fd: u32, newflags: u32, round: usize) -> Result<u32, u32> {
        let ix = match self.find(fd) {
            Some(ix) => ix,
            None => {
                self.book(round, GA_EBADF);
                return Err(GA_EBADF);
            }
        };
        if newflags & !ATTR_MASK != 0 {
            self.book(round, GA_EINVAL);
            return Err(GA_EINVAL);
        }
        let old = self.fds[ix].attr.flags;
        self.fds[ix].attr.flags = newflags;
        self.book(round, 0);
        Ok(old)
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
            h = (h ^ self.fds[i].attr.fold()).wrapping_mul(0x100000001b3);
            i += 1;
        }
        h = (h ^ (self.rounds as u64)).wrapping_mul(0x100000001b3);
        h
    }

    /// 渲染。
    pub fn render(&self) -> String {
        format!(
            "AttrDesk{{fds={},rounds={},ok={},ebadf={},einval={}}}",
            self.fds.len(), self.rounds, self.ok_hits, self.ebadf_hits, self.einval_hits
        )
    }
}

// ---------------------------------------------------------------- 注入
/// 注入四路坏样本，返回四路是否全部被台正确拦截。
pub fn inject_four() -> [bool; 4] {
    let mut caught = [false; 4];
    let mut d = AttrDesk::new(2, false);
    // 注入 1：getattr 坏描述符 → EBADF。
    caught[0] = matches!(d.getattr(9, 9001), Err(x) if x == GA_EBADF);
    // 注入 2：setattr 坏描述符 → EBADF。
    caught[1] = matches!(d.setattr(9, ATTR_NONBLOCK, 9002), Err(x) if x == GA_EBADF);
    // 注入 3：未知位 → EINVAL。
    caught[2] = matches!(d.setattr(0, 0x4, 9003), Err(x) if x == GA_EINVAL);
    // 注入 4：关闭后再 getattr → EBADF。
    assert!(d.close(1));
    caught[3] = matches!(d.getattr(1, 9004), Err(x) if x == GA_EBADF);
    caught
}

// ---------------------------------------------------------------- 确定性样本
/// 千轮样本：合法读改写循环为主，三错误面轮转。返回符合预期轮数。
pub fn sample_thousand(d: &mut AttrDesk) -> usize {
    let mut good = 0;
    let mut r = 0;
    while r < GA_ROUNDS {
        let low = r % 10;
        let matched;
        if low < 4 {
            // 合法面：getattr 回读。
            let res = d.getattr(0, r);
            matched = match &res {
                Ok(a) => a.max_count == ATTR_MAX_COUNT && a.max_msg == ATTR_MAX_MSG,
                Err(_) => false,
            };
        } else if low < 7 {
            // 合法面：setattr 翻转非阻塞位再读回核验。
            let want = if r % 2 == 0 { ATTR_NONBLOCK } else { 0 };
            let res = d.setattr(0, want, r);
            let readback = d.getattr(0, r);
            matched = matches!(res, Ok(_)) && matches!(&readback, Ok(a) if a.flags == want);
        } else if low == 7 {
            // EBADF 面。
            matched = matches!(d.getattr(0x7f, r), Err(x) if x == GA_EBADF);
        } else if low == 8 {
            // EINVAL 面：未知位组合。
            matched = matches!(d.setattr(0, 0x10 + (r % 8) as u32, r), Err(x) if x == GA_EINVAL);
        } else {
            // 合法面：对 1 号描述符读（保持活跃）。
            matched = d.getattr(1, r).is_ok();
        }
        if matched {
            good += 1;
        }
        r += 1;
    }
    good
}

// ---------------------------------------------------------------- 判据
/// UNX-F10947 判据组。
pub fn run_unx10947_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10947");
    // J1 正面之一：getattr/setattr 坏描述符 → EBADF 十轮全中。
    let mut d1 = AttrDesk::new(1, false);
    let mut c = 0;
    let mut k = 0;
    while k < 10 {
        let r = if k % 2 == 0 {
            d1.getattr(0xdead, k).map(|a| a.flags)
        } else {
            d1.setattr(0xdead, ATTR_NONBLOCK, k).map(|v| v)
        };
        if matches!(r, Err(x) if x == GA_EBADF) {
            c += 1;
        }
        k += 1;
    }
    s.add("J1-正面EBADF", c == 10, "坏描述符读写十轮全部裁 EBADF");
    // J1 正面之二：attr 非法 → EINVAL 十轮全中。
    let mut d2 = AttrDesk::new(1, false);
    let mut c = 0;
    let mut k = 0;
    while k < 10 {
        if matches!(d2.setattr(0, 0x2 + k as u32, k), Err(x) if x == GA_EINVAL) {
            c += 1;
        }
        k += 1;
    }
    s.add("J1-正面EINVAL", c == 10, "未知属性位十轮全部裁 EINVAL");
    // J2 反面：千轮合法读改写零误报。
    let mut d3 = AttrDesk::new(2, false);
    let good = sample_thousand(&mut d3);
    s.add_dyn("J2-反面零误报", good == GA_ROUNDS, &format!("千轮样本符合预期 {} 轮", good));
    // J3 注入四路全捕获。
    let inj = inject_four();
    s.add(
        "J3-注入四路",
        inj[0] && inj[1] && inj[2] && inj[3],
        "EBADF/EINVAL/关闭/未知位四路坏样本全部拦截",
    );
    // J4 getattr 回读与账一致：setattr 后 flags 立即生效。
    let mut d4 = AttrDesk::new(1, false);
    let _ = d4.setattr(0, ATTR_NONBLOCK, 1);
    let a4 = d4.getattr(0, 2);
    s.add(
        "J4-回读一致",
        matches!(&a4, Ok(a) if a.flags == ATTR_NONBLOCK),
        "setattr 后 getattr 立即回读新值",
    );
    // J5 判定序：EBADF 先于 EINVAL。
    let mut d5 = AttrDesk::new(1, false);
    s.add(
        "J5-判定序",
        matches!(d5.setattr(9, 0xff, 1), Err(x) if x == GA_EBADF),
        "双错同现按序裁 EBADF",
    );
    // J6 错误码分段。
    s.add(
        "J6-错误码分段",
        GA_EBADF > SEG_BASE && GA_LEDGER < SEG_BASE + 0x20,
        "三个错误码全部落在段基址页内",
    );
    // J7 错误码互异。
    s.add(
        "J7-错误码互异",
        GA_EBADF != GA_EINVAL && GA_EINVAL != GA_LEDGER && GA_EBADF != GA_LEDGER,
        "三个错误码两两不同",
    );
    // J8 账目对平。
    s.add("J8-账目对平", d3.ledger_ok(), "千轮之后三类计数与轮次、日志逐项对平");
    // J9 fold 稳定。
    let f = d3.fold();
    let clone = d3.clone();
    s.add("J9-fold稳定", clone.fold() == f, "克隆后指纹逐位一致");
    // J10 渲染非空。
    let txt = d3.render();
    s.add("J10-渲染非空", txt.len() > 10 && txt.contains("AttrDesk"), "台级渲染含类型名与计数");
    // J11 属性快照指纹稳定：同状态两次 fold 一致。
    let mut d6 = AttrDesk::new(1, true);
    let f1 = d6.fds[0].attr.fold();
    let f2 = d6.fds[0].attr.fold();
    s.add("J11-快照指纹稳定", f1 == f2, "同属性两次指纹一致");
    // J12 关闭后账不破：close 只改 open 位。
    let mut d7 = AttrDesk::new(1, false);
    let flags_before = d7.fds[0].attr.flags;
    let _ = d7.close(0);
    s.add(
        "J12-关闭不破账",
        !d7.fds[0].open && d7.fds[0].attr.flags == flags_before,
        "close 仅改 open 位，属性原样",
    );
    // J13 动态：千轮明细。
    s.add_dyn(
        "J13-千轮明细",
        good == GA_ROUNDS,
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
    fn unx10947_mq_attr_matrix_flow() {
        // 流程：getattr → setattr 翻转 → 回读 → 未知位 EINVAL → 坏 fd EBADF。
        let mut d = AttrDesk::new(1, false);
        let a0 = d.getattr(0, 1);
        assert!(matches!(&a0, Ok(a) if a.flags == 0));
        assert_eq!(d.setattr(0, ATTR_NONBLOCK, 2), Ok(0));
        assert!(matches!(&d.getattr(0, 3), Ok(a) if a.flags == ATTR_NONBLOCK));
        assert!(matches!(d.setattr(0, 0x8, 4), Err(x) if x == GA_EINVAL));
        assert!(matches!(d.getattr(5, 5), Err(x) if x == GA_EBADF));
        assert!(d.ledger_ok());
        assert!(inject_four().iter().all(|x| *x));
    }

    #[test]
    fn unx10947_checks_green() {
        let s = run_unx10947_checks();
        assert!(s.all_passed(), "UNX-F10947 判据未全绿");
    }
}
