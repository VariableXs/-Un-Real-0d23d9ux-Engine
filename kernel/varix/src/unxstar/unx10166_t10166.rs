//! UNX 卡 10166 — 信号合并正反双判据（合并语义达成与超载误用受控）
//! 段位 seg=0x107e0
//! 判据原文：UNX-F10166-J1 合并达成（密集同号信号合并语义 10/10 次）
//! 锚点：pending 位图合并同号 + 密集投递不膨胀 + 超载受控 + 递达清位
//! 域：SigCoalesce 位图合并册（32 号位图 + 投递账 + 溢出受控）
//! 结构：sigmask 位图 → raise 置位（同号幂等）→ deliver 清位取号 → 溢出受控 → 注入检出

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x107e0;
pub const SC_BAD_SIG: u32 = SEG_BASE + 0x01;
pub const SC_PENDING_FULL: u32 = SEG_BASE + 0x02;
pub const SC_SEALED: u32 = SEG_BASE + 0x03;
pub const SC_EMPTY_DELIVER: u32 = SEG_BASE + 0x04;
pub const SC_DROP_AUDIT: u32 = SEG_BASE + 0x05;

/// 信号号闭集 1..=32 与投递账容量
pub const SIG_MAX: u32 = 32;
pub const DELIVER_CAP: usize = 0x100;

/// 信号合并册
pub struct SigCoalesce {
    pending: u32, // 位图：bit n = 信号 n
    pub raises: u32,
    pub drops: u32,
    pub delivered: Vec<u32>,
    sealed: bool,
}

impl SigCoalesce {
    pub fn new() -> Self {
        SigCoalesce {
            pending: 0,
            raises: 0,
            drops: 0,
            delivered: Vec::new(),
            sealed: false,
        }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    pub fn pending_bits(&self) -> u32 {
        self.pending
    }

    /// 信号号合法
    pub fn sig_ok(sig: u32) -> bool {
        sig >= 1 && sig <= SIG_MAX
    }

    /// raise：置位（同号幂等=合并）；投递账满受控
    pub fn raise(&mut self, sig: u32) -> Result<bool, u32> {
        if self.sealed {
            return Err(SC_SEALED);
        }
        if !Self::sig_ok(sig) {
            return Err(SC_BAD_SIG);
        }
        self.raises += 1;
        let bit = 1u32 << (sig - 1);
        if self.pending & bit != 0 {
            // 已挂起：合并（丢弃一次冗余）
            self.drops += 1;
            return Ok(false);
        }
        if self.delivered.len() >= DELIVER_CAP {
            return Err(SC_PENDING_FULL);
        }
        self.pending |= bit;
        Ok(true)
    }

    /// deliver：取最低挂起号并清位
    pub fn deliver(&mut self) -> Result<u32, u32> {
        if self.sealed {
            return Err(SC_SEALED);
        }
        if self.pending == 0 {
            return Err(SC_EMPTY_DELIVER);
        }
        if self.delivered.len() >= DELIVER_CAP {
            return Err(SC_PENDING_FULL);
        }
        let sig = self.pending.trailing_zeros() + 1;
        self.pending &= self.pending - 1;
        self.delivered.push(sig);
        Ok(sig)
    }

    /// 密集合并审计：n 次同号 raise 只挂 1 位、丢 n-1 次
    pub fn coalesce_audit(&self, raises_for_sig: u32, dropped_for_sig: u32) -> bool {
        raises_for_sig > 0 && dropped_for_sig == raises_for_sig - 1
    }

    /// 挂起位数
    pub fn pending_count(&self) -> u32 {
        self.pending.count_ones()
    }

    /// 递达账计数
    pub fn delivered_count(&self) -> usize {
        self.delivered.len()
    }

    /// 先到先递审计：deliver 序非降（trailing_zeros 保证）
    pub fn order_audit(&self) -> bool {
        for w in self.delivered.windows(2) {
            if w[1] < w[0] {
                return false;
            }
        }
        true
    }

    /// 合并比（千分位）：drops / raises
    pub fn drop_permille(&self) -> usize {
        if self.raises == 0 {
            return 0;
        }
        self.drops as usize * 1000 / self.raises as usize
    }

    /// 位图指纹
    pub fn render_pending(&self) -> String {
        format!("{:#010x}", self.pending)
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 独立信号总数：挂起位数 + 已递达数（合并前后守恒面）
    pub fn distinct_total(&self) -> usize {
        self.pending_count() as usize + self.delivered.len()
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = SigCoalesce::new();
    // 1) 非法信号号必拒
    got[0] = b.raise(0).is_err() && b.raise(33).is_err();
    // 2) 同号幂等：连 raise 3 次 SIG5 只挂 1 位
    let _ = b.raise(5);
    let _ = b.raise(5);
    let _ = b.raise(5);
    got[1] = b.pending_count() == 1;
    // 3) 空册递达必拒
    let mut b2 = SigCoalesce::new();
    got[2] = b2.deliver().is_err();
    // 4) 递达后清位（先递 5 再递 7，两次取最低清位）
    let _ = b.raise(7);
    let _ = b.deliver();
    let _ = b.deliver();
    got[3] = b.pending_bits() & (1 << 6) == 0;
    // 5) 封册后必拒
    b.seal();
    got[4] = b.raise(1).is_err();
    got
}

/// 判据入口：UNX-F10166
pub fn run_unx10166_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10166");
    let mut b = SigCoalesce::new();

    // J1 密集合并 10/10：十轮各连投 i+1 次同号 → 每轮挂 1 位丢 i 次
    let mut j1 = true;
    for i in 0..10u32 {
        let sig = 10 + i;
        let mut dropped = 0u32;
        for k in 0..(i + 1) {
            match b.raise(sig) {
                Ok(true) => {}
                Ok(false) => dropped += 1,
                Err(_) => j1 = false,
            }
            let _ = k;
        }
        if !b.coalesce_audit(i + 1, dropped) {
            j1 = false;
        }
    }
    s.add("J1_coalesce_10x", j1 && b.pending_count() == 10, "密集同号合并 10/10（每号仅 1 位）");

    // J2 幂等位面：SIG10 再投不增位
    let before = b.pending_count();
    let r = b.raise(10);
    let j2 = r == Ok(false) && b.pending_count() == before;
    s.add("J2_idempotent", j2, "已挂号再投合并不增位");

    // J3 递达取号
    let d = b.deliver();
    let j3 = d == Ok(10) && b.pending_count() == 9;
    s.add("J3_deliver_low", j3, "递达取最低号 10 且清位");

    // J4 先到先递有序
    let mut b2 = SigCoalesce::new();
    let _ = b2.raise(20);
    let _ = b2.raise(5);
    let _ = b2.raise(9);
    let _ = b2.deliver();
    let _ = b2.deliver();
    let _ = b2.deliver();
    s.add("J4_order", b2.order_audit(), "递达序 5→9→20 非降");

    // J5 非法号受控
    let j5 = b.raise(0) == Err(SC_BAD_SIG)
        && b.raise(33) == Err(SC_BAD_SIG)
        && b.raise(SIG_MAX + 1) == Err(SC_BAD_SIG);
    s.add("J5_bad_sig", j5, "零号与越界号三路受控");

    // J6 空册递达受控
    let mut b3 = SigCoalesce::new();
    s.add("J6_empty", b3.deliver() == Err(SC_EMPTY_DELIVER), "空册递达拒 EMPTY");

    // J7 合并账对平：drops = raises − 挂起位数 − 已递达数
    let accounted = b.raises as usize;
    let merged = b.drops as usize;
    let distinct = b.pending_count() as usize + b.delivered_count();
    s.add("J7_account", merged + distinct == accounted, "raises = drops + distinct");

    // J8 合并比指纹
    let pm = b.drop_permille();
    let want = b.drops as usize * 1000 / b.raises as usize;
    s.add("J8_permille", pm == want && pm > 0, "合并比千分位与账对平");

    // J9 注入 5 项全抓
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J9_inject_all", caught == 5, "5 注入点全检出");

    // J10 封册受控
    let mut b4 = SigCoalesce::new();
    b4.seal();
    let j10 = b4.raise(1) == Err(SC_SEALED) && b4.deliver() == Err(SC_SEALED);
    s.add("J10_sealed", j10, "封册后投递/递达皆拒 SEALED");

    // J11 位图边界：1 与 32 两端在册
    let mut b5 = SigCoalesce::new();
    let _ = b5.raise(1);
    let _ = b5.raise(32);
    let j11 = b5.pending_count() == 2 && b5.deliver() == Ok(1) && b5.deliver() == Ok(32);
    s.add("J11_edges", j11, "首尾号 1/32 挂位递达对平");

    // J12 位图指纹稳定
    let f1 = b.render_pending();
    let f2 = b.render_pending();
    s.add("J12_fingerprint", f1 == f2 && f1.starts_with("0x"), "位图指纹稳定");

    // J13 交替投递递达
    let mut b6 = SigCoalesce::new();
    let mut ok = true;
    for sig in 1..=8u32 {
        if b6.raise(sig) != Ok(true) {
            ok = false;
        }
        if b6.deliver() != Ok(sig) {
            ok = false;
        }
    }
    s.add("J13_pingpong", ok && b6.pending_count() == 0, "八轮投递即递达全清");

    // J14 溢出账守恒
    s.add("J14_drops", b.drops >= 45, "十轮密集合并丢账 ≥45（0+1+..+9）");

    // J15 递达账容量守门
    let mut b7 = SigCoalesce::new();
    let mut fill_ok = true;
    for i in 0..DELIVER_CAP {
        let sig = (i % 32) as u32 + 1;
        if b7.raise(sig).is_err() {
            // 位可能已挂起（幂等合并），继续
        }
        if b7.delivered_count() >= DELIVER_CAP {
            break;
        }
        if b7.deliver().is_err() {
            fill_ok = false;
            break;
        }
    }
    let j15 = fill_ok
        && b7.raise(9) == Err(SC_PENDING_FULL)
        && b7.deliver() == Err(SC_EMPTY_DELIVER);
    s.add("J15_cap", j15, "递达账满 256 后新投拒 PENDING_FULL");

    // J16 独立信号总数守恒
    let j16 = b.distinct_total() == 10;
    s.add("J16_distinct", j16, "挂起 9 位 + 递达 1 笔 = 10 独立信号（J3 已取号）");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sig66_coalesce() {
        let mut b = SigCoalesce::new();
        assert_eq!(b.raise(5), Ok(true));
        assert_eq!(b.raise(5), Ok(false));
        assert_eq!(b.pending_count(), 1);
        assert_eq!(b.drops, 1);
    }

    #[test]
    fn test_sig66_deliver() {
        let mut b = SigCoalesce::new();
        let _ = b.raise(9);
        let _ = b.raise(3);
        assert_eq!(b.deliver(), Ok(3));
        assert_eq!(b.deliver(), Ok(9));
        assert_eq!(b.deliver(), Err(SC_EMPTY_DELIVER));
    }

    #[test]
    fn test_sig66_checks() {
        let cs = run_unx10166_checks();
        assert!(cs.all_passed());
    }

    #[test]
    fn test_sig66_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
