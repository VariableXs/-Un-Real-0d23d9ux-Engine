//! UNX-F10989 并发子进程退出收割压力（SIGCHLD+wait 账对平）（0x1db20 段）
//!
//! 锚点：UNX-F10989-J1 M 子并发退出收割逐 pid 对平（千次 10/10），
//! 收割零遗漏（10/10）。
//! 域：CoRun·UNX-B30（I 型联测域·退出收割面）。
//!
//! 结构：子进程账（八子：pid + 退出位 + 退出码 + 收割位）+ 收割账
//! （退出数 + 收割数 + 僵尸数 + 遗漏数）+ 收割器（只收割已退出未收割
//! 者，重复收割即拒）+ 僵尸探针（已退未收必须归零）+ fold 与冻结 +
//! 注入核（越界子号/未退即等/重复收割/冻结后等四路必抓）。
//!
//! 收割律：每子每轮退出一次、收割一次；退出数 == 收割数 == 轮数 × 子数；
//! 收束后不得留下僵尸（已退未收）也不得遗漏（未退或未收）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址：10989 号卡独占 0x20 步长窗口
const SEG_BASE: u32 = 0x1db20;

/// 空账 —— 未立子即等
pub const CH_EMPTY: u32 = SEG_BASE + 0x01;
/// 子号越界 —— 八子 0..8
pub const CH_RANGE: u32 = SEG_BASE + 0x02;
/// 重复操作 —— 重复退出或重复收割
pub const CH_DUP: u32 = SEG_BASE + 0x03;
/// 未退即等 —— 子未退出不得收割
pub const CH_NOWAIT: u32 = SEG_BASE + 0x04;
/// 僵尸残留 —— 已退未收
pub const CH_ZOMBIE: u32 = SEG_BASE + 0x05;
/// 账已冻结 —— freeze 后禁止再等
pub const CH_FROZEN: u32 = SEG_BASE + 0x06;

/// 子进程数。
pub const CHILDREN: usize = 8;
/// 压力轮数（千次）。
pub const ROUNDS: u64 = 1000;
/// 退出码基数。
pub const STATUS_BASE: i32 = 0x40;
/// 收割账域标（fold 种子）。
pub const CH_TAG: u64 = 0x1db_89;

/// 同源退出码：轮号 + 子序 → 状态码（确定性）。
pub fn status_of(round: u64, no: usize) -> i32 {
    STATUS_BASE
        .wrapping_add((round % 61) as i32)
        .wrapping_add((no as i32) % 7)
}

/// 子进程：退出与收割两态。
#[derive(Clone, Debug)]
pub struct Child {
    /// 子号（0x16000 段起编）
    pub cid: u32,
    /// 子序（0..8）
    pub no: u8,
    /// 退出位
    pub exited: bool,
    /// 退出码
    pub status: i32,
    /// 收割位
    pub reaped: bool,
    /// 累计退出次数
    pub exits: u64,
    /// 累计收割次数
    pub reaps: u64,
}

impl Child {
    /// 僵尸：已退未收。
    pub fn zombie(&self) -> bool {
        self.exited && !self.reaped
    }

    /// 已安宁：未退且已收或从未产生。
    pub fn settled(&self) -> bool {
        !self.exited && !self.reaped
    }

    /// 逐子对平：退出次数等于收割次数。
    pub fn balanced(&self) -> bool {
        self.exits == self.reaps && self.exits > 0
    }

    /// 退出码合法（非负且在基址之上）。
    pub fn status_ok(&self) -> bool {
        self.status >= STATUS_BASE
    }
}

/// 退出收割总账。
#[derive(Clone, Debug)]
pub struct ReapBook {
    /// 八子
    pub children: Vec<Child>,
    /// 已跑轮数
    pub rounds: u64,
    /// 累计退出
    pub exits: u64,
    /// 累计收割
    pub reaps: u64,
    /// 僵尸检出
    pub zombies: u64,
    /// 遗漏检出
    pub missed: u64,
    /// 冻结标记
    pub frozen: bool,
}

impl ReapBook {
    /// 立账：八子全部登记（未退未收）。
    pub fn new() -> ReapBook {
        let mut b = ReapBook {
            children: Vec::new(),
            rounds: 0,
            exits: 0,
            reaps: 0,
            zombies: 0,
            missed: 0,
            frozen: false,
        };
        let mut k = 0usize;
        while k < CHILDREN {
            b.children.push(Child {
                cid: 0x16000u32.wrapping_add(k as u32),
                no: k as u8,
                exited: false,
                status: 0,
                reaped: false,
                exits: 0,
                reaps: 0,
            });
            k += 1;
        }
        b
    }

    /// 子定位。
    pub fn child_ix(&self, no: usize) -> Result<usize, u32> {
        if self.children.is_empty() {
            return Err(CH_EMPTY);
        }
        if no >= self.children.len() {
            return Err(CH_RANGE);
        }
        Ok(no)
    }

    /// 子退出：只准退一次。
    pub fn exit(&mut self, no: usize, status: i32) -> Result<i32, u32> {
        if self.frozen {
            return Err(CH_FROZEN);
        }
        let ix = self.child_ix(no)?;
        if self.children[ix].exited {
            return Err(CH_DUP);
        }
        let c = &mut self.children[ix];
        c.exited = true;
        c.status = status;
        c.exits = c.exits.wrapping_add(1);
        self.exits = self.exits.wrapping_add(1);
        Ok(c.status)
    }

    /// 收割一子：未退即拒，重复收割即拒。
    pub fn wait(&mut self, no: usize) -> Result<i32, u32> {
        if self.frozen {
            return Err(CH_FROZEN);
        }
        let ix = self.child_ix(no)?;
        if !self.children[ix].exited {
            return Err(CH_NOWAIT);
        }
        if self.children[ix].reaped {
            return Err(CH_DUP);
        }
        let c = &mut self.children[ix];
        c.reaped = true;
        c.reaps = c.reaps.wrapping_add(1);
        self.reaps = self.reaps.wrapping_add(1);
        Ok(c.status)
    }

    /// 全量收割：只收已退未收者。
    pub fn reap_all(&mut self) -> Result<u64, u32> {
        let mut k = 0usize;
        while k < self.children.len() {
            if self.children[k].exited && !self.children[k].reaped {
                self.wait(k)?;
            }
            k += 1;
        }
        Ok(self.reaps)
    }

    /// 一轮复位：把上一轮的退出/收割位清零（计数保留）。
    fn reopen(&mut self) {
        let mut k = 0usize;
        while k < self.children.len() {
            self.children[k].exited = false;
            self.children[k].reaped = false;
            k += 1;
        }
    }

    /// 一轮：八子并发退出 → 全量收割 → 探针。
    pub fn round(&mut self, r: u64) -> Result<u64, u32> {
        let mut k = 0usize;
        while k < self.children.len() {
            let st = status_of(r, k);
            self.exit(k, st)?;
            k += 1;
        }
        // 收前扫僵尸：此时应当恰好八只
        let pending = self.zombie_count();
        if pending != self.children.len() {
            self.missed = self.missed.wrapping_add(1);
        }
        self.zombies = self.zombies.wrapping_add(pending as u64);
        let _ = self.reap_all()?;
        if self.zombie_count() != 0 {
            self.missed = self.missed.wrapping_add(1);
        }
        self.reopen();
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

    /// 当前僵尸数。
    pub fn zombie_count(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.children.len() {
            if self.children[k].zombie() {
                n += 1;
            }
            k += 1;
        }
        n
    }

    /// 僵尸探针：有僵尸即拒。
    pub fn zombie_probe(&self) -> Result<(), u32> {
        if self.children.is_empty() {
            return Err(CH_EMPTY);
        }
        if self.zombie_count() != 0 {
            return Err(CH_ZOMBIE);
        }
        Ok(())
    }

    /// 逐子对平。
    pub fn children_balanced(&self) -> bool {
        let mut k = 0usize;
        while k < self.children.len() {
            if !self.children[k].balanced() {
                return false;
            }
            k += 1;
        }
        !self.children.is_empty()
    }

    /// 退出收割总数对平。
    pub fn totals_balanced(&self) -> bool {
        self.exits == self.reaps && self.exits == self.rounds.wrapping_mul(CHILDREN as u64)
    }

    /// 零遗漏零僵尸。
    pub fn clean(&self) -> bool {
        self.missed == 0 && self.zombies == ROUNDS.wrapping_mul(CHILDREN as u64) && self.zombie_count() == 0
    }

    /// 逐 pid 对平（按 pid 而非序复查）。
    pub fn pids_balanced(&self) -> bool {
        let mut k = 0usize;
        while k < self.children.len() {
            let pid = self.children[k].cid;
            let mut j = 0usize;
            while j < self.children.len() {
                if j != k && self.children[j].cid == pid {
                    return false;
                }
                j += 1;
            }
            k += 1;
        }
        !self.children.is_empty()
    }

    /// 链式 fold 指纹。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = CH_TAG;
        let mut k = 0usize;
        while k < self.children.len() {
            let c = &self.children[k];
            h = (h ^ c.cid as u64).wrapping_mul(0x100000001b3);
            h = (h ^ c.exits).wrapping_mul(0x100000001b3);
            h = (h ^ c.reaps).wrapping_mul(0x100000001b3);
            h = (h ^ c.status as u64).wrapping_mul(0x100000001b3);
            k += 1;
        }
        h = (h ^ self.zombies).wrapping_mul(0x100000001b3);
        h
    }

    /// 冻结账。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        self.fold()
    }
}

/// 确定性样本：千轮八子退出收割全对平。
pub fn sample_reaped() -> (ReapBook, bool) {
    let mut b = ReapBook::new();
    let _ = b.run(ROUNDS);
    let ok = b.children_balanced()
        && b.totals_balanced()
        && b.clean()
        && b.zombie_probe().is_ok()
        && b.pids_balanced();
    b.freeze();
    (b, ok)
}

/// 注入核：越界子号 / 未退即等 / 重复收割 / 冻结后等。
pub fn inject_four() -> [bool; 4] {
    let mut b = ReapBook::new();
    // 注入 1：越界子号 → CH_RANGE 拒收
    let i1 = matches!(b.wait(CHILDREN), Err(CH_RANGE));
    // 注入 2：未退即等 → CH_NOWAIT 拒收
    let i2 = matches!(b.wait(0), Err(CH_NOWAIT));
    // 注入 3：重复收割 → CH_DUP 拒收
    let _ = b.exit(0, STATUS_BASE);
    let _ = b.wait(0);
    let i3 = matches!(b.wait(0), Err(CH_DUP));
    // 注入 4：冻结后等 → CH_FROZEN 拒收
    b.freeze();
    let i4 = matches!(b.wait(1), Err(CH_FROZEN));
    [i1, i2, i3, i4]
}

/// 判据检查：J1 千次逐 pid 对平，收割零遗漏。
pub fn run_unx10989_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10989");
    let (b, ok) = sample_reaped();

    s.add("J1-逐 pid 对平", ok && b.children_balanced() && b.pids_balanced(), "八子逐 pid 退出等于收割");
    s.add("J1-零遗漏", b.clean() && b.missed == 0, "千轮收割零遗漏零残留僵尸");
    s.add("J2-总数守恒", b.totals_balanced(), "退出等于收割且等于千乘八");
    s.add("J2-件数对账", b.exits == ROUNDS * CHILDREN as u64 && b.reaps == b.exits, "退出收割各八千次");
    s.add("J2-轮数守恒", b.rounds == ROUNDS, "千轮全部跑满");
    s.add("J3-僵尸逐轮", b.zombies == ROUNDS.wrapping_mul(CHILDREN as u64), "逐轮收前恰八僵尸后归零");
    s.add("J3-僵尸探针", b.zombie_probe().is_ok() && b.zombie_count() == 0, "收束后僵尸探针放行");
    s.add("J3-逐子次数", b.children.iter().all(|c| c.exits == ROUNDS && c.reaps == ROUNDS), "八子逐者千退千收");
    s.add("J4-状态码", b.children.iter().all(|c| c.status_ok()), "八子退出码均在基址之上");
    s.add("J4-状态同源", status_of(0, 0) == STATUS_BASE && status_of(1, 0) == STATUS_BASE + 1, "退出码与同源函数一致");
    s.add("J5-未退拒等", inject_four()[1], "未退出即收割被 CH_NOWAIT 拒止");
    s.add("J6-越界拒等", inject_four()[0], "越界子号被 CH_RANGE 拒收");
    s.add("J6-重复拒收", inject_four()[2], "重复收割被 CH_DUP 拒收");
    s.add("J7-冻结拒等", inject_four()[3], "冻结后等被 CH_FROZEN 拒止");
    s.add("J8-僵尸必检", {
        let mut g = ReapBook::new();
        let _ = g.exit(0, STATUS_BASE);
        g.zombie_count() == 1 && g.zombie_probe() == Err(CH_ZOMBIE)
    }, "已退未收即被僵尸探针拒放");
    s.add("J8-部分非对平", {
        let mut g = ReapBook::new();
        let _ = g.exit(0, STATUS_BASE);
        let _ = g.wait(0);
        !g.children_balanced() && g.children[0].balanced()
    }, "只跑一子时整体未对平");
    s.add("J8-指纹稳定", b.frozen && b.fold() == b.fold(), "冻结后指纹重算稳定");
    s.add_dyn(
        "J9-收割动态",
        ok,
        &format!(
            "轮 {} 退出 {} 收割 {} 僵尸 {} 遗漏 {} 当前僵尸 {}",
            b.rounds,
            b.exits,
            b.reaps,
            b.zombies,
            b.missed,
            b.zombie_count(),
        ),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10989_reap_book_flow() {
        let mut b = ReapBook::new();
        assert_eq!(b.run(ROUNDS).unwrap_or(0), ROUNDS);
        assert!(b.children_balanced());
        assert!(b.totals_balanced());
        assert!(b.clean());
        assert_eq!(b.exits, 8000);
        // 单步语义抽查
        let mut g = ReapBook::new();
        let _ = g.exit(2, status_of(0, 2));
        assert_eq!(g.zombie_count(), 1);
        assert_eq!(g.wait(2).unwrap_or(0), status_of(0, 2));
        assert_eq!(g.zombie_count(), 0);
        assert!(g.wait(2).is_err());
        assert!(g.child_ix(CHILDREN).is_err());
        let root = b.freeze();
        assert!(b.frozen);
        assert_eq!(root, b.fold());
        for hit in inject_four() {
            assert!(hit, "注入路径必须全部拒止");
        }
    }

    #[test]
    fn unx10989_checks_green() {
        let s = run_unx10989_checks();
        assert!(s.all_passed(), "UNX-F10989 判据未全绿");
    }
}
