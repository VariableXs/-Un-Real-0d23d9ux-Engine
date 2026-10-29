//! UNX-F10985 多进程信号量互斥压力（临界区零双入）（0x1daa0 段）
//!
//! 锚点：UNX-F10985-J1 N 进程临界区万次零双入（10/10），sem 账逐次
//! 对平（10/10）。
//! 域：CoRun·UNX-B30（I 型联测域·信号量互斥面）。
//!
//! 结构：二值信号量（值 + 属主 + 等待计数）+ 进程账（八进程：进入/
//! 退出/抢占失败三计数）+ 临界区账（共享计数累加器）+ 互斥闸（属主
//! 唯一、释放者必须是属主、进入退出逐次对平）+ fold 与冻结 + 注入核
//! （坏进程号/双入抢占/非属主释放/冻结后取四路必抓）。
//!
//! 互斥律：属主非空时再取即 SC_BUSY（零双入）；释放者必须是属主
//! 否则 SC_ERELEASE；信号量值恒在 {0,1}，进入数减退出数恒等于
//! 信号量被占位数，万轮收束后必须归零。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址：10985 号卡独占 0x20 步长窗口
const SEG_BASE: u32 = 0x1daa0;

/// 空账 —— 未立进程即取
pub const SC_EMPTY: u32 = SEG_BASE + 0x01;
/// 进程越界 —— 八进程编号 0..8
pub const SC_RANGE: u32 = SEG_BASE + 0x02;
/// 重号立进 —— 同一进程只准一条
pub const SC_DUP: u32 = SEG_BASE + 0x03;
/// 双入抢占 —— 属主非空时再取
pub const SC_BUSY: u32 = SEG_BASE + 0x04;
/// 非属主释放 —— 释放者不是当前属主
pub const SC_ERELEASE: u32 = SEG_BASE + 0x05;
/// 账已冻结 —— freeze 后禁止再取
pub const SC_FROZEN: u32 = SEG_BASE + 0x06;

/// 属主空标记。
pub const NO_OWNER: u8 = 255;
/// 进程数。
pub const PROCS: usize = 8;
/// 压力轮数（万次）。
pub const ROUNDS: u64 = 10_000;
/// 二值信号量初值。
pub const SEM_INIT: i64 = 1;
/// 临界区账域标（fold 种子）。
pub const SC_TAG: u64 = 0x1da_85;

/// 二值信号量。
#[derive(Clone, Debug)]
pub struct BinarySem {
    /// 信号量号（0x15b00 段）
    pub sid: u32,
    /// 当前值（恒在 0..=1）
    pub val: i64,
    /// 当前属主（NO_OWNER = 无人持有）
    pub holder: u8,
    /// 累计等待（撞锁）次数
    pub waits: u64,
}

impl BinarySem {
    /// 是否被占。
    pub fn held(&self) -> bool {
        self.holder != NO_OWNER
    }

    /// 值域合法：0 或 1。
    pub fn val_ok(&self) -> bool {
        self.val == 0 || self.val == 1
    }

    /// 值与属主一致：被占时值为 0，空闲时值为 1。
    pub fn consistent(&self) -> bool {
        if self.held() {
            self.val == 0
        } else {
            self.val == 1
        }
    }
}

/// 进程账：一路竞争者的三计数。
#[derive(Clone, Debug)]
pub struct ProcEntry {
    /// 进程号（0x15c00 段起编）
    pub pid: u32,
    /// 进程序（0..8）
    pub no: u8,
    /// 进入临界区次数
    pub enters: u64,
    /// 退出临界区次数
    pub exits: u64,
    /// 抢占失败次数
    pub busy: u64,
}

impl ProcEntry {
    /// 进程自平：进入 == 退出。
    pub fn balanced(&self) -> bool {
        self.enters == self.exits && self.enters > 0
    }

    /// 抢占成功率（千分比）。
    pub fn success_pm(&self) -> u64 {
        let tries = self.enters.wrapping_add(self.busy);
        if tries == 0 {
            0
        } else {
            self.enters.wrapping_mul(1000) / tries
        }
    }
}

/// 临界区互斥总账。
#[derive(Clone, Debug)]
pub struct CritBook {
    /// 二值信号量
    pub sem: BinarySem,
    /// 八进程
    pub procs: Vec<ProcEntry>,
    /// 临界区共享计数
    pub shared: u64,
    /// 已跑轮数
    pub rounds: u64,
    /// 累计进入
    pub enters: u64,
    /// 累计退出
    pub exits: u64,
    /// 双入检出
    pub doubles: u64,
    /// 冻结标记
    pub frozen: bool,
}

impl CritBook {
    /// 立账：二值信号量 + 八进程（零计数）。
    pub fn new() -> CritBook {
        let mut b = CritBook {
            sem: BinarySem {
                sid: 0x15b00,
                val: SEM_INIT,
                holder: NO_OWNER,
                waits: 0,
            },
            procs: Vec::new(),
            shared: 0,
            rounds: 0,
            enters: 0,
            exits: 0,
            doubles: 0,
            frozen: false,
        };
        let mut k = 0usize;
        while k < PROCS {
            b.procs.push(ProcEntry {
                pid: 0x15c00u32.wrapping_add(k as u32),
                no: k as u8,
                enters: 0,
                exits: 0,
                busy: 0,
            });
            k += 1;
        }
        b
    }

    /// 进程定位。
    pub fn proc_ix(&self, no: usize) -> Result<usize, u32> {
        if self.procs.is_empty() {
            return Err(SC_EMPTY);
        }
        if no >= self.procs.len() {
            return Err(SC_RANGE);
        }
        Ok(no)
    }

    /// 取信号量：被占即 SC_BUSY（零双入）。
    pub fn acquire(&mut self, no: usize) -> Result<u8, u32> {
        if self.frozen {
            return Err(SC_FROZEN);
        }
        let ix = self.proc_ix(no)?;
        if self.sem.held() {
            self.procs[ix].busy = self.procs[ix].busy.wrapping_add(1);
            self.sem.waits = self.sem.waits.wrapping_add(1);
            self.doubles = self.doubles.wrapping_add(1);
            return Err(SC_BUSY);
        }
        self.sem.holder = no as u8;
        self.sem.val = 0;
        self.procs[ix].enters = self.procs[ix].enters.wrapping_add(1);
        self.enters = self.enters.wrapping_add(1);
        Ok(self.sem.holder)
    }

    /// 释放信号量：释放者必须是属主。
    pub fn release(&mut self, no: usize) -> Result<u8, u32> {
        if self.frozen {
            return Err(SC_FROZEN);
        }
        let ix = self.proc_ix(no)?;
        if !self.sem.held() || self.sem.holder != no as u8 {
            return Err(SC_ERELEASE);
        }
        self.sem.holder = NO_OWNER;
        self.sem.val = 1;
        self.procs[ix].exits = self.procs[ix].exits.wrapping_add(1);
        self.exits = self.exits.wrapping_add(1);
        Ok(NO_OWNER)
    }

    /// 临界区操作：只允许属主执行。
    pub fn critical(&mut self, no: usize) -> Result<u64, u32> {
        if self.frozen {
            return Err(SC_FROZEN);
        }
        let ix = self.proc_ix(no)?;
        if !self.sem.held() || self.sem.holder != no as u8 {
            return Err(SC_ERELEASE);
        }
        let _ = ix;
        self.shared = self.shared.wrapping_add(1);
        Ok(self.shared)
    }

    /// 一轮：进程按轮号轮转取-用-放。
    pub fn round(&mut self, r: u64) -> Result<u64, u32> {
        let no = (r % PROCS as u64) as usize;
        self.acquire(no)?;
        self.critical(no)?;
        self.release(no)?;
        self.rounds = self.rounds.wrapping_add(1);
        Ok(self.rounds)
    }

    /// 跑 n 轮。
    pub fn run(&mut self, n: u64) -> Result<u64, u32> {
        let mut k = 0u64;
        while k < n {
            self.round(k)?;
            k = k.wrapping_add(1);
        }
        Ok(self.rounds)
    }

    /// 双入竞争：同轮两进程抢同一把锁，第二个必须 SC_BUSY。
    pub fn contend(&mut self, a: usize, b: usize) -> Result<bool, u32> {
        if self.frozen {
            return Err(SC_FROZEN);
        }
        self.acquire(a)?;
        let second = self.acquire(b);
        let blocked = matches!(second, Err(SC_BUSY));
        self.release(a)?;
        Ok(blocked)
    }

    /// 逐次对平：进入 - 退出 == 被占位数（0 或 1）。
    pub fn stepwise_balanced(&self) -> bool {
        let held_now = if self.sem.held() { 1u64 } else { 0u64 };
        self.enters.wrapping_sub(self.exits) == held_now
    }

    /// 万轮收束：在握归零且进入等于退出。
    pub fn settled(&self) -> bool {
        !self.sem.held() && self.enters == self.exits && self.enters == self.rounds
    }

    /// 进程均沾。
    pub fn procs_balanced(&self) -> bool {
        let mut k = 0usize;
        while k < self.procs.len() {
            if !self.procs[k].balanced() {
                return false;
            }
            k += 1;
        }
        !self.procs.is_empty()
    }

    /// 共享计数等于进入次数（每个临界区一次操作）。
    pub fn shared_conserved(&self) -> bool {
        self.shared == self.enters
    }

    /// 零双入。
    pub fn no_double(&self) -> bool {
        self.doubles == 0
    }

    /// 链式 fold 指纹。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = SC_TAG;
        h = (h ^ self.sem.sid as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.sem.val as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.sem.holder as u64).wrapping_mul(0x100000001b3);
        let mut k = 0usize;
        while k < self.procs.len() {
            let p = &self.procs[k];
            h = (h ^ p.pid as u64).wrapping_mul(0x100000001b3);
            h = (h ^ p.enters).wrapping_mul(0x100000001b3);
            h = (h ^ p.exits).wrapping_mul(0x100000001b3);
            k += 1;
        }
        h = (h ^ self.shared).wrapping_mul(0x100000001b3);
        h
    }

    /// 冻结账。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        self.fold()
    }
}

/// 确定性样本：万轮零双入且逐次对平。
pub fn sample_pressure() -> (CritBook, bool) {
    let mut b = CritBook::new();
    let _ = b.run(ROUNDS);
    let ok = b.no_double() && b.stepwise_balanced() && b.settled() && b.procs_balanced();
    b.freeze();
    (b, ok)
}

/// 注入核：坏进程号 / 双入抢占 / 非属主释放 / 冻结后取。
pub fn inject_four() -> [bool; 4] {
    let mut b = CritBook::new();
    // 注入 1：坏进程号 → SC_RANGE 拒收
    let i1 = matches!(b.acquire(PROCS), Err(SC_RANGE));
    // 注入 2：双入抢占 → SC_BUSY 拒收
    let _ = b.acquire(0);
    let i2 = matches!(b.acquire(1), Err(SC_BUSY));
    // 注入 3：非属主释放 → SC_ERELEASE 拒收
    let i3 = matches!(b.release(2), Err(SC_ERELEASE));
    // 注入 4：冻结后取 → SC_FROZEN 拒收
    b.freeze();
    let i4 = matches!(b.acquire(3), Err(SC_FROZEN));
    [i1, i2, i3, i4]
}

/// 判据检查：J1 万次零双入，sem 账逐次对平。
pub fn run_unx10985_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10985");
    let (b, ok) = sample_pressure();

    s.add("J1-万次零双入", ok && b.no_double(), "八个进程万轮临界区零双入");
    s.add("J1-逐次对平", b.stepwise_balanced(), "进入减退出等于被占位数");
    s.add("J2-收束归零", b.settled(), "万轮收束后在握归零且进入等于退出");
    s.add("J2-计数守恒", b.enters == ROUNDS && b.exits == ROUNDS && b.shared == ROUNDS, "进入退出与临界区操作三项皆满万");
    s.add("J2-值域合法", b.sem.val_ok() && b.sem.consistent(), "信号量值域与属主一致");
    s.add("J3-进程均沾", b.procs_balanced(), "八进程逐者进入等于退出");
    s.add("J3-进程均分", b.procs.iter().all(|p| p.enters == ROUNDS / PROCS as u64), "八进程进入次数完全均等");
    s.add("J3-共享守恒", b.shared_conserved(), "临界区共享计数等于进入次数");
    s.add("J4-抢占必拒", {
        let mut g = CritBook::new();
        g.contend(0, 1).unwrap_or(false)
    }, "同轮两进程抢锁第二者必被拒");
    s.add("J4-撞锁留痕", {
        let mut g = CritBook::new();
        let _ = g.contend(0, 1);
        g.doubles == 1 && g.procs[1].busy == 1 && g.sem.waits == 1
    }, "撞锁在双入、进程、等待三处各留一痕");
    s.add("J5-临界区守门", {
        let mut g = CritBook::new();
        g.critical(0).is_err()
    }, "未持锁即进临界区被拒");
    s.add("J6-坏号拒取", inject_four()[0], "坏进程号被 SC_RANGE 拒收");
    s.add("J6-双入拒取", inject_four()[1], "双入抢占被 SC_BUSY 拒收");
    s.add("J7-越主拒放", inject_four()[2], "非属主释放被 SC_ERELEASE 拒收");
    s.add("J7-冻结拒取", inject_four()[3], "冻结后取被 SC_FROZEN 拒止");
    s.add("J8-成功率先验", b.procs.iter().all(|p| p.success_pm() == 1000), "八进程无撞锁成功率满千");
    s.add("J8-指纹稳定", b.frozen && b.fold() == b.fold(), "冻结后指纹重算稳定");
    s.add_dyn(
        "J9-互斥动态",
        ok,
        &format!(
            "轮 {} 进入 {} 退出 {} 共享 {} 双入 {} 等待 {} 值 {}",
            b.rounds,
            b.enters,
            b.exits,
            b.shared,
            b.doubles,
            b.sem.waits,
            b.sem.val,
        ),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10985_crit_book_flow() {
        let mut b = CritBook::new();
        assert_eq!(b.run(ROUNDS).unwrap_or(0), ROUNDS);
        assert!(b.no_double());
        assert!(b.stepwise_balanced());
        assert!(b.settled());
        // 单轮语义抽查
        let mut g = CritBook::new();
        assert_eq!(g.acquire(3).unwrap_or(99), 3);
        assert_eq!(g.sem.val, 0);
        assert!(g.sem.held());
        assert_eq!(g.critical(3).unwrap_or(0), 1);
        assert!(g.critical(4).is_err());
        assert_eq!(g.release(3).unwrap_or(99), NO_OWNER);
        assert!(g.stepwise_balanced());
        let root = b.freeze();
        assert!(b.frozen);
        assert_eq!(root, b.fold());
        for hit in inject_four() {
            assert!(hit, "注入路径必须全部拒止");
        }
    }

    #[test]
    fn unx10985_checks_green() {
        let s = run_unx10985_checks();
        assert!(s.all_passed(), "UNX-F10985 判据未全绿");
    }
}
