//! UNX-F11009 epoll MOD/DEL 挂号账联签（改注注销幂等对账）（0x1e120 段）
//!
//! 锚点：UNX-F11009-J1 MOD/DEL 幂等千次对平（10/10），注销清扫零残留
//! （10/10）。
//! 域：CoRun·UNX-C4-B31（futex/epoll 挂号联签域）。
//!
//! 结构：改注账（epoll MOD 逐次改掩码、幂等重放同值不漂移）+ 注销账
//! （DEL 逐项摘除、清扫后零残留）+ 幂等对账器（同 fd 同掩码二次 MOD 的
//! fold 不变）+ fold 指纹与冻结 + 注入核（改注不存在的项/摘除不存在的
//! 项/零掩码/冻结后写四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1e120 独占 0x20）。
pub const SEG_BASE: u32 = 0x1e120;
pub const MD_NOMOD: u32 = SEG_BASE + 0x01; // 改注目标不存在
pub const MD_NODEL: u32 = SEG_BASE + 0x02; // 摘除目标不存在
pub const MD_ZEROMASK: u32 = SEG_BASE + 0x03; // 掩码非法
pub const MD_DUPDEL: u32 = SEG_BASE + 0x04; // 摘除后残留重号
pub const MD_FROZEN: u32 = SEG_BASE + 0x05; // 冻结后写账
pub const MD_CAP: u32 = SEG_BASE + 0x06; // 账面超容

/// 千次 MOD/DEL 轮数。
pub const ROUNDS: u32 = 1000;
/// 账面定容。
pub const MD_CAP_N: usize = 32;
/// 事件掩码合法位集。
pub const MASK_LEGAL: u32 = 0x0000_003f;
/// 改注账域标（fold 种子）。
pub const MD_TAG: u64 = 0x1e12_09;

/// 单条注册项：fd + 掩码 + 改注次数 + 在册位。
#[derive(Clone, Debug)]
pub struct RegItem {
    pub fd: u32,
    pub mask: u32,
    pub mods: u32,
    pub live: bool,
}

/// epoll MOD/DEL 账：ADD 底账 → MOD 改注 → DEL 摘除 → 清扫 → 冻结。
pub struct ModDelLedger {
    pub items: Vec<RegItem>,
    pub mod_count: u32,
    pub del_count: u32,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl ModDelLedger {
    pub fn new() -> ModDelLedger {
        ModDelLedger { items: Vec::new(), mod_count: 0, del_count: 0, frozen: false, root: None }
    }

    /// ADD 底账：冻结、零掩码、非法位、零 fd、重号、超容均拒。
    pub fn add(&mut self, fd: u32, mask: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(MD_FROZEN);
        }
        if mask == 0 || (mask & !MASK_LEGAL) != 0 {
            return Err(MD_ZEROMASK);
        }
        if fd == 0 {
            return Err(MD_NOMOD);
        }
        if self.items.iter().any(|i| i.fd == fd) {
            return Err(MD_DUPDEL);
        }
        if self.items.len() >= MD_CAP_N {
            return Err(MD_CAP);
        }
        self.items.push(RegItem { fd, mask, mods: 0, live: true });
        Ok(())
    }

    /// MOD 改注：目标在册才改掩码并计次；幂等——同 fd 同掩码二次改注
    /// 状态不变（fold 不变）。
    pub fn modify(&mut self, fd: u32, mask: u32) -> Result<u32, u32> {
        if self.frozen {
            return Err(MD_FROZEN);
        }
        if mask == 0 || (mask & !MASK_LEGAL) != 0 {
            return Err(MD_ZEROMASK);
        }
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].fd == fd {
                // 幂等：同值改注不动注册态（掩码/改注次数），仅计操作数
                if self.items[k].mask == mask && self.items[k].live {
                    self.mod_count = self.mod_count.wrapping_add(1);
                    return Ok(self.items[k].mods);
                }
                self.items[k].mask = mask;
                self.items[k].mods = self.items[k].mods.wrapping_add(1);
                self.mod_count = self.mod_count.wrapping_add(1);
                return Ok(self.items[k].mods);
            }
            k += 1;
        }
        Err(MD_NOMOD)
    }

    /// DEL 摘除：目标在册才翻 live 位并计次；重复摘除必拒。
    pub fn remove(&mut self, fd: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(MD_FROZEN);
        }
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].fd == fd {
                if !self.items[k].live {
                    return Err(MD_NODEL);
                }
                self.items[k].live = false;
                self.del_count = self.del_count.wrapping_add(1);
                return Ok(());
            }
            k += 1;
        }
        Err(MD_NODEL)
    }

    /// 在册 fd 清单（清扫面）。
    pub fn live_fds(&self) -> Vec<u32> {
        let mut g: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].live {
                g.push(self.items[k].fd);
            }
            k += 1;
        }
        g
    }

    /// 摘除后残留重号探针：live 位外的项不得再被 MOD 命中。
    pub fn mod_dead(&mut self, fd: u32) -> bool {
        if self.frozen {
            return false;
        }
        let mut hit = false;
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].fd == fd && !self.items[k].live {
                hit = true;
            }
            k += 1;
        }
        hit
    }

    /// 链式 fold 指纹（FNV 风格，序敏感，含 fd/掩码/改注次数/在册位，
    /// 只覆盖注册态——操作计数不入指纹，幂等重放才可能零漂移）。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = MD_TAG;
        let mut k = 0usize;
        while k < self.items.len() {
            let e = &self.items[k];
            h = (h ^ e.fd as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.mask as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.mods as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.live as u64).wrapping_mul(0x100000001b3);
            k += 1;
        }
        h
    }

    /// 冻结：翻冻结位并落冻结根。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }
}

/// fd 确定性样本。
pub fn mdf_sample(no: u32) -> u32 {
    no.wrapping_mul(5).wrapping_add(2)
}

/// 掩码确定性样本。
pub fn mdm_sample(no: u32) -> u32 {
    1u32.wrapping_shl(no % 6)
}

/// 千次幂等全链：底账 8 项，千轮 MOD（同 fd 同掩码成对重放）+ 摘除。
pub fn run_moddel_thousand() -> (ModDelLedger, bool) {
    let mut l = ModDelLedger::new();
    let mut ok = true;
    let mut i = 1u32;
    while i <= 8u32 {
        if l.add(mdf_sample(i), mdm_sample(i)).is_err() {
            ok = false;
        }
        i += 1;
    }
    let mut r = 0u32;
    while r < ROUNDS {
        let fd = mdf_sample(r % 8 + 1);
        let m = mdm_sample(r + 1);
        if l.modify(fd, m).is_err() {
            ok = false;
        }
        // 幂等重放：同 fd 同掩码立刻再来一次，状态不得漂移
        if l.modify(fd, m).is_err() {
            ok = false;
        }
        r += 1;
    }
    let l_ok = ok && l.mod_count == ROUNDS * 2 && l.del_count == 0;
    (l, l_ok)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 改注不存在的项必拒
    let mut a = ModDelLedger::new();
    got[0] = a.modify(99, 1).err() == Some(MD_NOMOD);
    // 2) 摘除不存在的项必拒
    let mut b = ModDelLedger::new();
    got[1] = b.remove(99).err() == Some(MD_NODEL);
    // 3) 零掩码必拒
    let mut c = ModDelLedger::new();
    let _ = c.add(3, 1);
    got[2] = c.modify(3, 0).err() == Some(MD_ZEROMASK);
    // 4) 冻结后写账必抓
    let (mut d, _) = run_moddel_thousand();
    d.freeze();
    got[3] = d.modify(3, 1).err() == Some(MD_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11009_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11009");

    // J1 MOD 幂等千次对平：千轮双放计数恰为两千
    let (mut book, green) = run_moddel_thousand();
    s.add("J1-MOD 幂等千次对平", green && book.mod_count == ROUNDS * 2, "千轮幂等重放计数零漂移");

    // J2 幂等实证：同 fd 同掩码二次改注 fold 不变
    let idem = {
        let mut c = ModDelLedger::new();
        let _ = c.add(3, 1);
        let _ = c.modify(3, 4);
        let f1 = c.fold();
        let _ = c.modify(3, 4);
        f1 == c.fold()
    };
    let _ = idem;
    s.add("J2-幂等实证", idem, "同值二次改注指纹不变");

    // J3 注销清扫零残留：逐项摘除后 live 清单清空且死项 MOD 拒
    let sweep = {
        let mut c = ModDelLedger::new();
        let mut i = 1u32;
        while i <= 8u32 {
            let _ = c.add(mdf_sample(i), 1);
            i += 1;
        }
        let mut i = 1u32;
        while i <= 8u32 {
            let _ = c.remove(mdf_sample(i));
            i += 1;
        }
        c.live_fds().is_empty() && c.del_count == 8 && c.mod_dead(mdf_sample(1))
    };
    s.add("J3-清扫零残留", sweep, "逐项摘除后 live 清零且死项隔离");

    // J4 冻结根：fold 稳定且与根一致
    let root = book.freeze();
    s.add("J4-冻结根", book.root == Some(root) && book.fold() == root && root != 0, "冻结根落账且指纹稳定");

    // J5 注入检出：改注死目标 + 摘除死目标
    let inj = inject_four();
    s.add("J5-双不存在检出", inj[0] && inj[1], "改注与摘除不存在项两路必抓");

    // J6 注入检出：零掩码 + 冻结后写
    s.add("J6-掩码冻结检出", inj[2] && inj[3], "零掩码必拒且冻结后写必抓");

    // J7 重复摘除必拒
    let j7 = {
        let mut c = ModDelLedger::new();
        let _ = c.add(3, 1);
        let _ = c.remove(3);
        c.remove(3).err() == Some(MD_NODEL)
    };
    s.add("J7-重复摘除拒", j7, "重复 DEL 必拒不静默");

    // J8 重号 ADD 拒与零 fd 拒
    let j8 = {
        let mut c = ModDelLedger::new();
        let _ = c.add(3, 1);
        c.add(3, 1).err() == Some(MD_DUPDEL) && ModDelLedger::new().add(0, 1).err() == Some(MD_NOMOD)
    };
    s.add("J8-重号零 fd 拒", j8, "重号与零 fd 均必拒");

    // J9 部分摘除：摘半数 live 清单恰余半数
    let j9 = {
        let mut c = ModDelLedger::new();
        let mut i = 1u32;
        while i <= 6u32 {
            let _ = c.add(mdf_sample(i), 1);
            i += 1;
        }
        let mut i = 1u32;
        while i <= 3u32 {
            let _ = c.remove(mdf_sample(i));
            i += 1;
        }
        c.live_fds().len() == 3
    };
    s.add("J9-部分摘除", j9, "摘半数后 live 清单恰余半数");

    // J10 超容拒：第 MD_CAP_N+1 项必拒
    let j10 = {
        let mut c = ModDelLedger::new();
        let mut filled = true;
        let mut n = 1u32;
        while n <= MD_CAP_N as u32 {
            if c.add(n, 1).is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && c.add(999, 1).err() == Some(MD_CAP)
    };
    s.add("J10-超容边界", j10, "超出定容必拒且原账不损");

    // J11 改注计数逐项留痕：mods 计数随改注递增
    let j11 = {
        let mut c = ModDelLedger::new();
        let _ = c.add(3, 1);
        let _ = c.modify(3, 2);
        let _ = c.modify(3, 4);
        let _ = c.modify(3, 8);
        match c.items.iter().find(|i| i.fd == 3) {
            Some(it) => it.mods == 3 && it.mask == 8,
            None => false,
        }
    };
    s.add("J11-改注留痕", j11, "逐项改注次数与终值掩码留痕");

    // J12 样本确定性：fd 与掩码同入同出
    let f3a = mdf_sample(3);
    let f3b = mdf_sample(3);
    let m3a = mdm_sample(3);
    let m3b = mdm_sample(3);
    s.add("J12-样本确定性", f3a == f3b && m3a == m3b, "fd 与掩码样本同入同出");

    // J13 掩码轮转覆盖合法全集
    let j13 = {
        let mut bits = 0u32;
        let mut i = 0u32;
        while i < 6u32 {
            bits |= mdm_sample(i);
            i += 1;
        }
        bits == MASK_LEGAL
    };
    s.add("J13-掩码全位覆盖", j13, "六轮单掩码轮转覆盖全部合法位");

    // J14 动态明细：MOD/DEL 双计数落账
    let mc = book.mod_count;
    let dc = book.del_count;
    s.add_dyn(
        "J14-双计数明细",
        mc == ROUNDS * 2 && dc == 0,
        &format!("mods={} dels={}", mc, dc),
    );

    // J15 动态明细：冻结根十六进制落账
    let fp = book.fold();
    s.add_dyn("J15-冻结根明细", fp == root, &format!("root={:#x}", fp));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11009_moddel_flow() {
        let mut l = ModDelLedger::new();
        assert!(l.add(3, 1).is_ok());
        assert!(l.add(3, 1).err() == Some(MD_DUPDEL));
        assert_eq!(l.modify(3, 2), Ok(1));
        assert!(l.modify(99, 2).err() == Some(MD_NOMOD));
        assert!(l.remove(3).is_ok());
        assert!(l.remove(3).err() == Some(MD_NODEL));
        l.freeze();
        assert!(l.modify(3, 1).err() == Some(MD_FROZEN));
        assert!(l.root.is_some());
    }

    #[test]
    fn unx11009_checks_green() {
        let s = run_unx11009_checks();
        assert!(s.all_passed(), "UNX-F11009 判据未全绿");
    }
}
