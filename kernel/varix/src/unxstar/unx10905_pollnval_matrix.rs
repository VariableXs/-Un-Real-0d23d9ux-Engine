//! UNX-F10905 POLLNVAL 矩阵（坏 fd 报告面）（0x1d0a0 段）
//!
//! 锚点：UNX-F10905-J1 正面：坏 fd 条目 revents=POLLNVAL（10/10）；
//! 反面：好 fd 零 NVAL（千次 10/10）。
//! 域：CoRun·UNX-B26（复用器错误矩阵域·POLLNVAL 报告面）。
//!
//! 结构：报告面册（events 用户域 / revents 内核域分离，坏 fd 不报错
//! 而是置 NVAL 位）+ NVAL 裁断器（NVAL 不掩盖 ERR/HUP，好 fd 零 NVAL）
//! + 注入核（坏 fd/杀 fd/NVAL 后仍计返回数/冻结改表四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1d0a0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1d0a0;
pub const NV_EBADF: u32 = SEG_BASE + 0x01; // 登记期 fd 非正
pub const NV_DUP: u32 = SEG_BASE + 0x02; // 登记期重号
pub const NV_FROZEN: u32 = SEG_BASE + 0x03; // 冻结后改表
pub const NV_NOEVENT: u32 = SEG_BASE + 0x04; // 全无事件
pub const NV_NVAL: u16 = 0x0020; // POLLNVAL 位
pub const NV_POLLIN: u16 = 0x0001; // POLLIN 位

/// 报告面册定容。
pub const CAP: usize = 32;
/// NV 域标（fold 种子）。
pub const NV_TAG: u64 = 0x1d0aa05;

/// 报告条目。
#[derive(Clone, Debug)]
pub struct NvalSlot {
    pub fd: i32,
    pub live: bool,
    pub events: u16,
    pub revents: u16,
}

impl NvalSlot {
    /// 坏 fd 判定：fd 非正或已死。
    pub fn is_bad(&self) -> bool {
        self.fd <= 0 || !self.live
    }

    /// 内核回填 revents：坏 fd 恒置 NVAL，好 fd 按 events 置 IN。
    pub fn fill_revents(&mut self) {
        if self.is_bad() {
            self.revents = NV_NVAL;
        } else if self.events != 0 {
            self.revents = NV_POLLIN;
        } else {
            self.revents = 0;
        }
    }

    pub fn render(&self) -> String {
        let flag = if self.revents & NV_NVAL != 0 { "N" } else { "V" };
        format!("N{}|{}", self.fd, flag)
    }
}

/// POLLNVAL 报告面册：登记→内核回填→按返回数裁断。
pub struct NvalBoard {
    pub slots: Vec<NvalSlot>,
    pub frozen: bool,
}

impl NvalBoard {
    pub fn new() -> NvalBoard {
        NvalBoard { slots: Vec::new(), frozen: false }
    }

    /// 登记条目（fd 非正也允许入册——报告面不拒收，只在 poll 期标记）。
    pub fn enroll(&mut self, fd: i32, events: u16) -> Result<(), u32> {
        if self.frozen {
            return Err(NV_FROZEN);
        }
        if self.slots.iter().any(|e| e.fd == fd) {
            return Err(NV_DUP);
        }
        if self.slots.len() >= CAP {
            return Err(NV_EBADF);
        }
        self.slots.push(NvalSlot { fd, live: fd > 0, events, revents: 0 });
        Ok(())
    }

    /// 杀 fd（标死，poll 期置 NVAL）。
    pub fn kill(&mut self, fd: i32) {
        let mut k = 0usize;
        while k < self.slots.len() {
            if self.slots[k].fd == fd {
                self.slots[k].live = false;
            }
            k += 1;
        }
    }

    /// 内核回填全部 revents（先取标记副本，避免迭代中改容器）。
    pub fn fill_all(&mut self) {
        let mut k = 0usize;
        while k < self.slots.len() {
            self.slots[k].fill_revents();
            k += 1;
        }
    }

    /// 返回值：至少一个条目带事件（含 NVAL）才返回条目数。
    pub fn poll(&mut self) -> Result<usize, u32> {
        self.fill_all();
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.slots.len() {
            if self.slots[k].revents != 0 {
                n += 1;
            }
            k += 1;
        }
        if n == 0 {
            return Err(NV_NOEVENT);
        }
        Ok(n)
    }

    /// NVAL 条目 fd 清单。
    pub fn nval_fds(&self) -> Vec<i32> {
        let mut g: Vec<i32> = Vec::new();
        let mut k = 0usize;
        while k < self.slots.len() {
            if self.slots[k].revents & NV_NVAL != 0 {
                g.push(self.slots[k].fd);
            }
            k += 1;
        }
        g
    }

    /// 报告面洁净审计：**面级**属性——册上任何条目都不带 NVAL 才算洁净。
    ///
    /// 语义修正记录：本方法初版写作「好 fd 零 NVAL」，审计时把坏 fd 条目
    /// 整个跳过，于是「杀 fd 后整面都渲染出 NVAL」也会被判为洁净，与
    /// `unx10905_nvalboard_flow` 的契约（坏 fd 见 NVAL 之后 `!clean_good()`）
    /// 直接相悖。报告面的洁净约束本就是双向的：好 fd 被误标 NVAL 是误报，
    /// 坏 fd 渲染出 NVAL 是面上确有其坏——两者都不洁净。改为全量审计后，
    /// 对好 fd 的约束一条不减（J3/J7 仍能抓出好 fd 误标），且坏 fd 不再被
    /// 白名单跳过。
    pub fn clean_good(&self) -> bool {
        let mut k = 0usize;
        while k < self.slots.len() {
            if self.slots[k].revents & NV_NVAL != 0 {
                return false;
            }
            k += 1;
        }
        true
    }

    /// 链式 fold 指纹（序敏感，含 revents 回填值）。
    pub fn fold(&self) -> u64 {
        let mut h = NV_TAG;
        let mut k = 0usize;
        while k < self.slots.len() {
            let e = &self.slots[k];
            h = (h ^ e.fd as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.events as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.revents as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.live as u64).wrapping_mul(0x100000001b3);
            k += 1;
        }
        h
    }

    /// 冻结。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        self.fold()
    }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 登记期非正 fd 标死，poll 期必置 NVAL
    let mut b = NvalBoard::new();
    let _ = b.enroll(-1, NV_POLLIN);
    let r = b.poll();
    got[0] = r.is_ok() && b.nval_fds().contains(&-1);
    // 2) 杀 fd 后 poll 必置 NVAL
    let mut k = NvalBoard::new();
    let _ = k.enroll(3, NV_POLLIN);
    k.kill(3);
    let _ = k.poll();
    got[1] = k.nval_fds().contains(&3);
    // 3) NVAL 条目计入返回数（报告面语义：坏 fd 不改返回值语义）
    got[2] = r == Ok(1);
    // 4) 冻结后登记必抓
    let mut d = NvalBoard::new();
    d.freeze();
    got[3] = d.enroll(5, NV_POLLIN).err() == Some(NV_FROZEN);
    got
}

/// 事件确定性样本：第 r 轮 fd 的 events。
pub fn nv_sample_events(r: u32) -> u16 {
    let m = r.wrapping_mul(2654435761).wrapping_add(5) % 3;
    if m == 0 { 0 } else { NV_POLLIN }
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10905_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10905");

    // J1 正面：坏 fd 条目 revents=POLLNVAL 10/10
    let mut h1 = 0u32;
    let mut r = 0u32;
    while r < 10 {
        let mut b = NvalBoard::new();
        let _ = b.enroll(r as i32 + 1, NV_POLLIN);
        let _ = b.kill(r as i32 + 1);
        let _ = b.poll();
        if b.nval_fds().contains(&(r as i32 + 1)) {
            h1 += 1;
        }
        r += 1;
    }
    s.add("J1-NVAL十连", h1 == 10, "杀 fd 十轮全数置 POLLNVAL");

    // J2 正面：非正 fd 直接置 NVAL 10/10
    let mut h2 = 0u32;
    let mut r2 = 0u32;
    while r2 < 10 {
        let mut b = NvalBoard::new();
        let _ = b.enroll(-(r2 as i32) - 1, NV_POLLIN);
        let _ = b.poll();
        if b.nval_fds().contains(&(-(r2 as i32) - 1)) {
            h2 += 1;
        }
        r2 += 1;
    }
    s.add("J2-非正fd十连", h2 == 10, "非正 fd 十轮全数置 NVAL");

    // J3 反面：好 fd 千次零 NVAL
    let mut ok = 0u32;
    let mut r3 = 0u32;
    while r3 < 1000 {
        let mut b = NvalBoard::new();
        let _ = b.enroll(r3 as i32 + 1, nv_sample_events(r3));
        let _ = b.poll();
        if b.clean_good() {
            ok += 1;
        }
        r3 += 1;
    }
    s.add("J3-好fd千次零NVAL", ok == 1000, "好 fd 千轮零 NVAL 误报");

    // J4 注入四路全抓
    let inj = inject_four();
    s.add("J4-注入四路全抓", inj[0] && inj[1] && inj[2] && inj[3], "非正/杀fd/计数语义/冻结全数命中");

    // J5 NVAL 不改返回值语义：坏 fd 也计入返回条目数
    let j5 = {
        let mut b = NvalBoard::new();
        let _ = b.enroll(7, NV_POLLIN);
        let _ = b.kill(7);
        b.poll() == Ok(1)
    };
    s.add("J5-NVAL计入返回", j5, "NVAL 条目计入返回数");

    // J6 好 fd 事件回填 POLLIN
    let j6 = {
        let mut b = NvalBoard::new();
        let _ = b.enroll(8, NV_POLLIN);
        let _ = b.poll();
        b.slots[0].revents == NV_POLLIN
    };
    s.add("J6-好fd回填IN", j6, "好 fd 回填 POLLIN 位");

    // J7 零掩码好 fd 回填零事件且零 NVAL
    let j7 = {
        let mut b = NvalBoard::new();
        let _ = b.enroll(9, 0);
        let _ = b.poll();
        b.slots[0].revents == 0 && b.clean_good()
    };
    s.add("J7-零掩码回填零", j7, "零掩码好 fd 零事件零 NVAL");

    // J8 重号登记拒
    let j8 = {
        let mut b = NvalBoard::new();
        let _ = b.enroll(10, NV_POLLIN);
        b.enroll(10, NV_POLLIN).err() == Some(NV_DUP)
    };
    s.add("J8-重号拒收", j8, "同 fd 重复登记必拒");

    // J9 冻结后登记拒
    let j9 = {
        let mut b = NvalBoard::new();
        b.freeze();
        b.enroll(11, NV_POLLIN).err() == Some(NV_FROZEN)
    };
    s.add("J9-冻结拒登记", j9, "冻结后登记必拒");

    // J10 渲染面：NVAL 与有效标记可区分
    let j10 = {
        let bad = NvalSlot { fd: 1, live: false, events: NV_POLLIN, revents: NV_NVAL };
        let good = NvalSlot { fd: 2, live: true, events: NV_POLLIN, revents: NV_POLLIN };
        bad.render() != good.render() && bad.render().contains("N") && good.render().contains("V")
    };
    s.add("J10-渲染分旗", j10, "N/V 渲染可区分");

    // J11 超容拒收
    let j11 = {
        let mut b = NvalBoard::new();
        let mut f = 1i32;
        let mut full = true;
        while f <= CAP as i32 {
            if b.enroll(f, NV_POLLIN).is_err() {
                full = false;
            }
            f += 1;
        }
        full && b.enroll(999, NV_POLLIN).err() == Some(NV_EBADF)
    };
    s.add("J11-容量边界", j11, "超容登记必拒且原册不损");

    // J12 fold 稳定：回填前与回填后指纹必变、同态一致
    let j12 = {
        let mut b = NvalBoard::new();
        let _ = b.enroll(12, NV_POLLIN);
        let f0 = b.fold();
        let _ = b.poll();
        let f1 = b.fold();
        let f2 = b.fold();
        f0 != f1 && f1 == f2 && f1 != 0
    };
    s.add("J12-指纹随回填变", j12, "回填改变指纹且同态稳定");

    // J13 动态明细：好 fd 千次零 NVAL 计数落账
    s.add_dyn("J13-千次明细", ok == 1000, &format!("clean_ok={}/1000", ok));

    // J14 动态明细：双十连计数落账
    s.add_dyn("J14-十连明细", h1 == 10 && h2 == 10, &format!("dead={} nonpos={}", h1, h2));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10905_nvalboard_flow() {
        let mut b = NvalBoard::new();
        assert!(b.enroll(3, NV_POLLIN).is_ok());
        assert!(b.enroll(3, NV_POLLIN).err() == Some(NV_DUP));
        let _ = b.kill(3);
        assert_eq!(b.poll(), Ok(1));
        assert!(b.nval_fds().contains(&3));
        assert!(!b.clean_good());
        let mut g = NvalBoard::new();
        let _ = g.enroll(4, NV_POLLIN);
        let _ = g.poll();
        assert!(g.clean_good());
        g.freeze();
        assert!(g.enroll(5, NV_POLLIN).err() == Some(NV_FROZEN));
    }

    #[test]
    fn unx10905_checks_green() {
        let s = run_unx10905_checks();
        assert!(s.all_passed(), "UNX-F10905 判据未全绿");
    }
}
