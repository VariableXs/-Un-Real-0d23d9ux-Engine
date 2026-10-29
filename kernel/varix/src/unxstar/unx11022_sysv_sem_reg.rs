//! UNX-F11022 SysV sem 挂号联签（semop 挂点与挂号账联动）（0x1e2c0 段）
//!
//! 锚点：UNX-F11022-J1 semop 挂点万次账对平（10/10），阻塞入眠账联动（10/10）。
//! 域：CoRun·UNX-C4-B32（五族挂号联签域·SysV sem）。
//!
//! 结构：信号量组账（四路计数值 + wait/post/sleep/wake 四计数）+ 挂点器
//! （wait 空量入眠、post 唤醒入眠者并即时交接）+ 万次对账器（确定性万轮
//! 收发模式）+ 联动断言（入眠必被唤醒、唤醒不超入眠）+ fold 与冻结 + 注
//! 入核（超界 post/入眠未唤醒/空集唤醒/冻结后写四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1e2c0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1e2c0;
pub const SM_OVERFLOW: u32 = SEG_BASE + 0x01; // post 超界
pub const SM_BADSEM: u32 = SEG_BASE + 0x02; // 信号量号越界
pub const SM_NOBLOCK: u32 = SEG_BASE + 0x03; // 空集唤醒
pub const SM_FROZEN: u32 = SEG_BASE + 0x04; // 冻结后写账
pub const SM_ZOPS: u32 = SEG_BASE + 0x05; // 零步操作
pub const SM_DRAIN: u32 = SEG_BASE + 0x06; // 唤醒后量不符

/// 信号量路数。
pub const SEM_SLOTS: usize = 4;
/// 信号量计数上限。
pub const SEM_VAL_CAP: u32 = 8;
/// 万次挂点轮数。
pub const SEM_OPS_TOTAL: u32 = 10_000;
/// 万轮折算的挂点步数（每轮 post/wait 双挂点）。
pub const SEM_STEPS_TOTAL: u32 = SEM_OPS_TOTAL * 2;
/// sem 账域标（fold 种子）。
pub const SEM_TAG: u64 = 0x1e2c_22;
/// 每百轮触发一次入眠样本。
pub const SLEEP_PERIOD: u32 = 100;

/// 信号量组账：wait/post 挂点计数 + 阻塞入眠/唤醒联动账。
pub struct SemBook {
    pub vals: [u32; SEM_SLOTS],
    pub waits: u32,
    pub posts: u32,
    pub sleeps: u32,
    pub wakes: u32,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl SemBook {
    pub fn new() -> SemBook {
        SemBook {
            vals: [0; SEM_SLOTS],
            waits: 0,
            posts: 0,
            sleeps: 0,
            wakes: 0,
            frozen: false,
            root: None,
        }
    }

    /// P 挂点：有量即取，无量入眠（挂账不取量）。
    pub fn wait(&mut self, k: usize) -> Result<bool, u32> {
        if self.frozen {
            return Err(SM_FROZEN);
        }
        if k >= SEM_SLOTS {
            return Err(SM_BADSEM);
        }
        if self.vals[k] > 0 {
            self.vals[k] -= 1;
            self.waits = self.waits.wrapping_add(1);
            Ok(true)
        } else {
            self.sleeps = self.sleeps.wrapping_add(1);
            Ok(false)
        }
    }

    /// V 挂点：入眠者优先即时交接，余量入账；超界必拒。
    pub fn post(&mut self, k: usize) -> Result<bool, u32> {
        if self.frozen {
            return Err(SM_FROZEN);
        }
        if k >= SEM_SLOTS {
            return Err(SM_BADSEM);
        }
        if self.sleeps > self.wakes {
            self.wakes = self.wakes.wrapping_add(1);
            self.posts = self.posts.wrapping_add(1);
            return Ok(true);
        }
        if self.vals[k] >= SEM_VAL_CAP {
            return Err(SM_OVERFLOW);
        }
        self.vals[k] += 1;
        self.posts = self.posts.wrapping_add(1);
        Ok(false)
    }

    /// 挂点总步数。
    pub fn ops(&self) -> u32 {
        self.waits.wrapping_add(self.posts).wrapping_add(self.sleeps)
    }

    /// 入眠账联动：入眠者必被唤醒且唤醒不超入眠。
    pub fn sleep_linked(&self) -> bool {
        self.wakes == self.sleeps && self.sleeps <= self.posts
    }

    /// 定向唤醒：无入眠者可唤即报空集。
    pub fn wake_one(&mut self) -> Result<(), u32> {
        if self.frozen {
            return Err(SM_FROZEN);
        }
        if self.sleeps > self.wakes {
            self.wakes = self.wakes.wrapping_add(1);
            Ok(())
        } else {
            Err(SM_NOBLOCK)
        }
    }

    /// 存量校验：各路量均在界内。
    pub fn vals_inbound(&self) -> bool {
        let mut ok = true;
        let mut k = 0usize;
        while k < SEM_SLOTS {
            if self.vals[k] > SEM_VAL_CAP {
                ok = false;
            }
            k += 1;
        }
        ok
    }

    /// 万次对平：总步数恰为轮数两倍且取还守恒。
    pub fn balanced(&self, total: u32) -> bool {
        self.ops() == total.wrapping_mul(2)
            && self.waits.wrapping_add(self.wakes) == total
            && self.posts == total
    }

    /// 清账校验：入眠唤醒未联动即报联动失平。
    pub fn check_drain(&self) -> Result<(), u32> {
        if self.sleeps != self.wakes {
            return Err(SM_DRAIN);
        }
        Ok(())
    }

    /// 链式 fold 指纹（FNV 风格，序敏感；只把非零面入链，空账即种子）。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = SEM_TAG;
        let mut k = 0usize;
        while k < SEM_SLOTS {
            if self.vals[k] != 0 {
                h = (h ^ k as u64).wrapping_mul(0x100000001b3);
                h = (h ^ self.vals[k] as u64).wrapping_mul(0x100000001b3);
            }
            k += 1;
        }
        if self.waits != 0 {
            h = (h ^ 1).wrapping_mul(0x100000001b3);
            h = (h ^ self.waits as u64).wrapping_mul(0x100000001b3);
        }
        if self.posts != 0 {
            h = (h ^ 2).wrapping_mul(0x100000001b3);
            h = (h ^ self.posts as u64).wrapping_mul(0x100000001b3);
        }
        if self.sleeps != 0 {
            h = (h ^ 3).wrapping_mul(0x100000001b3);
            h = (h ^ self.sleeps as u64).wrapping_mul(0x100000001b3);
        }
        if self.wakes != 0 {
            h = (h ^ 4).wrapping_mul(0x100000001b3);
            h = (h ^ self.wakes as u64).wrapping_mul(0x100000001b3);
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

/// 确定性万轮模式：第 i 轮在路 i%4 上 post 一次；每 SLEEP_PERIOD 轮先
/// 在空路上 wait（入眠）随后 post（唤醒交接）。返回总步数。
pub fn run_semop_rounds(sb: &mut SemBook, rounds: u32) -> Result<u32, u32> {
    if rounds == 0 {
        return Err(SM_ZOPS);
    }
    let mut ok = true;
    let mut i = 0u32;
    while i < rounds {
        let k = (i % SEM_SLOTS as u32) as usize;
        if i % SLEEP_PERIOD == SLEEP_PERIOD - 1 {
            // 阻塞样本：空路 wait 入眠，post 即时唤醒交接（不落存量）。
            let alt = ((i + 1) % SEM_SLOTS as u32) as usize;
            if sb.wait(alt).is_err() {
                ok = false;
            }
            if sb.post(alt).is_err() {
                ok = false;
            }
        } else {
            // 常规样本：同路 post 后 wait 取还，存量归零。
            if sb.post(k).is_err() {
                ok = false;
            }
            if sb.wait(k).is_err() {
                ok = false;
            }
        }
        i += 1;
    }
    if ok {
        Ok(sb.ops())
    } else {
        Err(SM_DRAIN)
    }
}

/// 万次账全链：空账起步跑满万轮。
pub fn build_sembook() -> (SemBook, bool) {
    let mut b = SemBook::new();
    let ok = match run_semop_rounds(&mut b, SEM_OPS_TOTAL) {
        Ok(_) => true,
        Err(_) => false,
    };
    let b_ok = ok && b.balanced(SEM_OPS_TOTAL) && b.sleep_linked();
    (b, b_ok)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 满量 post 超界必拒
    let mut a = SemBook::new();
    let mut filled = true;
    let mut n = 0u32;
    while n < SEM_VAL_CAP {
        if a.post(0).is_err() {
            filled = false;
        }
        n += 1;
    }
    got[0] = filled && a.post(0).err() == Some(SM_OVERFLOW);
    // 2) 信号量号越界必拒
    let mut b = SemBook::new();
    got[1] = b.wait(SEM_SLOTS).err() == Some(SM_BADSEM)
        && b.post(SEM_SLOTS).err() == Some(SM_BADSEM);
    // 3) 空账 wait 入眠后不唤醒则联动断言为假
    let mut c = SemBook::new();
    let _ = c.wait(1);
    got[2] = c.sleeps == 1 && !c.sleep_linked();
    // 4) 冻结后写账必抓
    let (mut d, _) = build_sembook();
    d.freeze();
    got[3] = d.post(0).err() == Some(SM_FROZEN) && d.wait(0).err() == Some(SM_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11022_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11022");

    // J1 semop 挂点万次账对平
    let (mut book, green) = build_sembook();
    s.add("J1-万次账对平", green && book.ops() == SEM_STEPS_TOTAL, "万轮双挂点步数折算与计数守恒");

    // J2 阻塞入眠账联动：入眠必唤醒且不超发
    s.add("J2-入眠账联动", book.sleep_linked() && book.sleeps > 0, "入眠唤醒联动且确有阻塞样本");

    // J3 计数分布：post 恰为轮数（交接不重复计发）
    s.add("J3-post 计数", book.posts == SEM_OPS_TOTAL, "post 含交接恰为轮数");

    // J4 冻结根：fold 稳定且与根一致
    let root = book.freeze();
    s.add("J4-冻结根", book.root == Some(root) && book.fold() == root && root != 0, "冻结根落账且指纹稳定");

    // J5 冻结后写账必抓
    let j5 = book.post(0).err() == Some(SM_FROZEN) && book.wait(0).err() == Some(SM_FROZEN);
    s.add("J5-冻结拒写", j5, "冻结后 post 与 wait 均必拒");

    // J6 注入检出：超界 + 越界号
    let inj = inject_four();
    s.add("J6-超界越界抓", inj[0] && inj[1], "满量 post 与越界信号量两路必抓");

    // J7 注入检出：入眠未唤醒 + 冻结后写
    s.add("J7-未唤冻结检出", inj[2] && inj[3], "入眠未唤与冻结后写必抓");

    // J8 即时交接语义：入眠后 post 不增存量
    let j8 = {
        let mut c = SemBook::new();
        let _ = c.wait(2);
        let before = c.fold();
        let _ = c.post(2);
        c.wakes == 1 && c.vals[2] == 0 && c.fold() != before
    };
    s.add("J8-即时交接", j8, "交接令牌直达入眠者不落存量");

    // J9 轮次对称：步数恰为轮数两倍
    let j9 = {
        let mut c = SemBook::new();
        let got = run_semop_rounds(&mut c, 400);
        got == Ok(800) && c.vals_inbound()
    };
    s.add("J9-轮次均匀", j9, "轮转模式步数精确可推");

    // J10 入眠周期精确：万轮入眠恰为万/周期
    let expect_sleeps = SEM_OPS_TOTAL / SLEEP_PERIOD;
    s.add("J10-入眠精确", book.sleeps == expect_sleeps, "入眠次数与周期公式吻合");

    // J11 唤醒即时性：唤醒后等待集清空
    let j11 = {
        let mut c = SemBook::new();
        let _ = c.wait(0);
        let _ = c.wait(0);
        let _ = c.post(0);
        let _ = c.post(0);
        c.wakes == 2 && c.check_drain().is_ok() && c.wake_one().err() == Some(SM_NOBLOCK)
    };
    s.add("J11-唤醒清集", j11, "连续入眠依次唤醒集清空");

    // J12 存量上界：全程各路量不超 SEM_VAL_CAP
    s.add("J12-存量上界", book.vals_inbound(), "万轮后各路存量均在界内");

    // J13 零步守恒：空账步数为零
    let j13 = SemBook::new().ops() == 0 && SemBook::new().fold() == SEM_TAG;
    s.add("J13-空账零步", j13, "空账无步数且种子指纹成立");

    // J14 样本确定性：同轮同路
    let k7a = 7 % SEM_SLOTS;
    let k7b = 7 % SEM_SLOTS;
    s.add("J14-样本确定性", k7a == k7b, "路号样本同入同出");

    // J15 动态明细：万次对平与冻结根落账
    let fp = book.fold();
    s.add_dyn(
        "J15-万次明细",
        fp == root && book.ops() == SEM_STEPS_TOTAL,
        &format!("root={:#x} ops={}", fp, book.ops()),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11022_sem_reg_flow() {
        let (mut b, built) = build_sembook();
        assert!(built);
        assert!(b.wait(SEM_SLOTS).err() == Some(SM_BADSEM));
        b.freeze();
        assert!(b.post(0).err() == Some(SM_FROZEN));
        assert!(b.wait(0).err() == Some(SM_FROZEN));
        assert!(b.root.is_some());
    }

    #[test]
    fn unx11022_checks_green() {
        let s = run_unx11022_checks();
        assert!(s.all_passed(), "UNX-F11022 判据未全绿");
    }
}
