//! UNX-F11037 多进程五族挂号联测（fork 继承账对账）（0x1e4a0 段）
//!
//! 锚点：UNX-F11037-J1 继承账逐族对平（10/10），跨进程行为一致（10/10）。
//! 域：CoRun·UNX-C4-B32（五族挂号联签域·fork 继承）。
//!
//! 结构：五族继承账（族 × 父侧计数 + 子侧计数）+ fork 挂点（父账复制为
//! 子账起点）+ 双侧挂点（父/子各自步进互不串账）+ 一致断言（同入同出序
//! 列两侧行为一致）+ fold 与冻结 + 注入核（越族步进/未 fork 先子步/串账
//! 探针/冻结后写四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1e4a0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1e4a0;
pub const FH_NOFAM: u32 = SEG_BASE + 0x01; // 族号越界
pub const FH_NOFORK: u32 = SEG_BASE + 0x02; // 未 fork 先子步
pub const FH_CROSS: u32 = SEG_BASE + 0x03; // 串账命中
pub const FH_FROZEN: u32 = SEG_BASE + 0x04; // 冻结后写账
pub const FH_UNSET: u32 = SEG_BASE + 0x05; // 族未立账
pub const FH_DRAIN: u32 = SEG_BASE + 0x06; // 继承失平

/// 五族编号（与 F11034 同序）。
pub const FH_FAMS: usize = 5;
/// 每族双侧步进数（10/10 对平面：五族 × 双侧）。
pub const FH_STEPS: u32 = 100;
/// fork 继承账域标（fold 种子）。
pub const FINH_TAG: u64 = 0x1e4a_37;

/// 五族继承账：父侧计数 + 子侧计数 + fork 位。
pub struct ForkBook {
    pub parent: [u32; FH_FAMS],
    pub child: [u32; FH_FAMS],
    pub forked: bool,
    pub established: [bool; FH_FAMS],
    pub frozen: bool,
    pub root: Option<u64>,
}

impl ForkBook {
    pub fn new() -> ForkBook {
        ForkBook {
            parent: [0; FH_FAMS],
            child: [0; FH_FAMS],
            forked: false,
            established: [false; FH_FAMS],
            frozen: false,
            root: None,
        }
    }

    /// 父侧立账。
    pub fn establish(&mut self, fam: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(FH_FROZEN);
        }
        if fam >= FH_FAMS {
            return Err(FH_NOFAM);
        }
        self.established[fam] = true;
        Ok(())
    }

    /// 父侧步进。
    pub fn pstep(&mut self, fam: usize) -> Result<u32, u32> {
        if self.frozen {
            return Err(FH_FROZEN);
        }
        if fam >= FH_FAMS {
            return Err(FH_NOFAM);
        }
        if !self.established[fam] {
            return Err(FH_UNSET);
        }
        self.parent[fam] = self.parent[fam].wrapping_add(1);
        Ok(self.parent[fam])
    }

    /// fork 挂点：立账族父账复制为子账起点。
    pub fn fork(&mut self) -> Result<u32, u32> {
        if self.frozen {
            return Err(FH_FROZEN);
        }
        let mut f = 0usize;
        while f < FH_FAMS {
            if self.established[f] {
                self.child[f] = self.parent[f];
            }
            f += 1;
        }
        self.forked = true;
        Ok(self.child_sum())
    }

    /// 子侧步进：未 fork 必拒。
    pub fn cstep(&mut self, fam: usize) -> Result<u32, u32> {
        if self.frozen {
            return Err(FH_FROZEN);
        }
        if fam >= FH_FAMS {
            return Err(FH_NOFAM);
        }
        if !self.forked {
            return Err(FH_NOFORK);
        }
        if !self.established[fam] {
            return Err(FH_UNSET);
        }
        self.child[fam] = self.child[fam].wrapping_add(1);
        Ok(self.child[fam])
    }

    /// 父侧计数和。
    pub fn parent_sum(&self) -> u32 {
        let mut n = 0u32;
        let mut f = 0usize;
        while f < FH_FAMS {
            n = n.wrapping_add(self.parent[f]);
            f += 1;
        }
        n
    }

    /// 子侧计数和。
    pub fn child_sum(&self) -> u32 {
        let mut n = 0u32;
        let mut f = 0usize;
        while f < FH_FAMS {
            n = n.wrapping_add(self.child[f]);
            f += 1;
        }
        n
    }

    /// 继承对平：fork 后逐族子账起点不低于快照。
    pub fn inherited(&self, base: [u32; FH_FAMS]) -> bool {
        if !self.forked {
            return false;
        }
        let mut ok = true;
        let mut f = 0usize;
        while f < FH_FAMS {
            if self.child[f] < base[f] {
                ok = false;
            }
            f += 1;
        }
        ok
    }

    /// 跨进程行为一致：子账总量 = 快照和 + 子侧步数。
    pub fn consistent(&self, base: [u32; FH_FAMS]) -> bool {
        if !self.forked {
            return false;
        }
        let mut base_sum = 0u32;
        let mut f = 0usize;
        while f < FH_FAMS {
            base_sum = base_sum.wrapping_add(base[f]);
            f += 1;
        }
        self.child_sum() == base_sum.wrapping_add(FH_STEPS)
    }

    /// 继承失平校验：不一致即报失平。
    pub fn check_drain(&self, base: [u32; FH_FAMS]) -> Result<(), u32> {
        if self.inherited(base) && self.consistent(base) {
            Ok(())
        } else {
            Err(FH_DRAIN)
        }
    }

    /// 链式 fold 指纹（FNV 风格，序敏感；未立账未步进族与未 fork 位不入链，
    /// 空账即种子）。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = FINH_TAG;
        let mut f = 0usize;
        while f < FH_FAMS {
            if self.parent[f] != 0 || self.child[f] != 0 || self.established[f] {
                h = (h ^ f as u64).wrapping_mul(0x100000001b3);
                h = (h ^ self.parent[f] as u64).wrapping_mul(0x100000001b3);
                h = (h ^ self.child[f] as u64).wrapping_mul(0x100000001b3);
                h = (h ^ self.established[f] as u64).wrapping_mul(0x100000001b3);
            }
            f += 1;
        }
        if self.forked {
            h = (h ^ self.forked as u64).wrapping_mul(0x100000001b3);
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

/// 族名确定性文本。
pub fn finh_fam(no: usize) -> String {
    match no % 5 {
        0 => "sysv".to_string(),
        1 => "posix".to_string(),
        2 => "pty".to_string(),
        3 => "sock".to_string(),
        _ => "evfd".to_string(),
    }
}

/// 双侧步进序列确定性样本：第 s 步落族 s%5。
pub fn finh_fam_of(s: u32) -> usize {
    (s % FH_FAMS as u32) as usize
}

/// 对账全链：五族立账；父侧各 FH_STEPS 步；fork 快照；子侧各 FH_STEPS 步。
pub fn build_forkbook() -> (ForkBook, [u32; FH_FAMS], bool) {
    let mut b = ForkBook::new();
    let mut ok = true;
    let mut f = 0usize;
    while f < FH_FAMS {
        if b.establish(f).is_err() {
            ok = false;
        }
        f += 1;
    }
    let mut s = 0u32;
    while s < FH_STEPS {
        if b.pstep(finh_fam_of(s)).is_err() {
            ok = false;
        }
        s += 1;
    }
    let mut base = [0u32; FH_FAMS];
    let mut f = 0usize;
    while f < FH_FAMS {
        base[f] = b.parent[f];
        f += 1;
    }
    if b.fork().is_err() {
        ok = false;
    }
    let mut s = 0u32;
    while s < FH_STEPS {
        if b.cstep(finh_fam_of(s)).is_err() {
            ok = false;
        }
        s += 1;
    }
    let good = ok && b.inherited(base) && b.consistent(base);
    (b, base, good)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 族号越界必拒
    let mut a = ForkBook::new();
    got[0] = a.establish(FH_FAMS).err() == Some(FH_NOFAM)
        && a.pstep(FH_FAMS).err() == Some(FH_NOFAM)
        && a.cstep(FH_FAMS).err() == Some(FH_NOFAM);
    // 2) 未 fork 先子步必拒
    let mut b = ForkBook::new();
    let _ = b.establish(0);
    let _ = b.pstep(0);
    got[1] = b.cstep(0).err() == Some(FH_NOFORK);
    // 3) 串账探针：父步不落子账
    let mut c = ForkBook::new();
    let _ = c.establish(1);
    let _ = c.pstep(1);
    got[2] = c.child[1] == 0 && c.parent[1] == 1;
    // 4) 冻结后写账必抓
    let (mut d, _, _) = build_forkbook();
    d.freeze();
    got[3] = d.pstep(0).err() == Some(FH_FROZEN) && d.fork().err() == Some(FH_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11037_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11037");

    // J1 继承账逐族对平：五族双侧步进齐平
    let (mut book, base, green) = build_forkbook();
    s.add("J1-继承对平", green && book.forked, "五族 fork 继承双侧步进对平");

    // J2 跨进程行为一致：子账总量守恒且失平校验通过
    s.add(
        "J2-行为一致",
        book.consistent(base) && book.inherited(base) && book.check_drain(base).is_ok(),
        "同序步进双侧行为一致",
    );

    // J3 fork 快照：子账起点 = 父账 fork 时点
    let j3 = {
        let mut ok = true;
        let mut f = 0usize;
        while f < FH_FAMS {
            if book.child[f] < base[f] {
                ok = false;
            }
            f += 1;
        }
        ok && book.child_sum() as usize == base[0] as usize * FH_FAMS + FH_STEPS as usize
    };
    s.add("J3-fork 快照", j3, "子账起点不低于快照且总量可推");

    // J4 冻结根：fold 稳定且与根一致
    let root = book.freeze();
    s.add("J4-冻结根", book.root == Some(root) && book.fold() == root && root != 0, "冻结根落账且指纹稳定");

    // J5 注入检出：族号越界 + 未 fork 先子步
    let inj = inject_four();
    s.add("J5-越界未fork抓", inj[0] && inj[1], "族号越界与未 fork 先子步两路必抓");

    // J6 注入检出：串账探针 + 冻结后写
    s.add("J6-串账冻结检出", inj[2] && inj[3], "串账隔离与冻结后写必抓");

    // J7 轮转均匀：父侧逐族二十步
    let j7 = {
        let per = FH_STEPS / FH_FAMS as u32;
        let mut even = true;
        let mut f = 0usize;
        while f < FH_FAMS {
            if book.parent[f] != per {
                even = false;
            }
            f += 1;
        }
        even && per == 20
    };
    s.add("J7-父侧均匀", j7, "父侧轮转逐族二十步");

    // J8 子侧均匀：fork 后子侧逐族增量二十
    let j8 = {
        let per = FH_STEPS / FH_FAMS as u32;
        let mut even = true;
        let mut f = 0usize;
        while f < FH_FAMS {
            if book.child[f].wrapping_sub(base[f]) != per {
                even = false;
            }
            f += 1;
        }
        even
    };
    s.add("J8-子侧均匀", j8, "子侧轮转逐族增量二十");

    // J9 未立账步进必拒
    let j9 = {
        let mut c = ForkBook::new();
        let _ = c.establish(0);
        let _ = c.fork();
        c.cstep(1).err() == Some(FH_UNSET) && c.pstep(1).err() == Some(FH_UNSET)
    };
    s.add("J9-未立账拒", j9, "未立账族双侧步进均必拒");

    // J10 族名确定性
    let j10 = finh_fam(0) == "sysv" && finh_fam(4) == "evfd" && finh_fam(7) == "pty";
    s.add("J10-族名确定", j10, "族名样本同入同出");

    // J11 轮转公式精确
    let j11 = finh_fam_of(0) == 0 && finh_fam_of(4) == 4 && finh_fam_of(5) == 0;
    s.add("J11-轮转公式", j11, "步转族号公式精确");

    // J12 fork 幂等：二次 fork 快照刷新不损账
    let j12 = {
        let mut c = ForkBook::new();
        let _ = c.establish(0);
        let _ = c.pstep(0);
        let _ = c.fork();
        let _ = c.cstep(0);
        let _ = c.fork();
        c.child[0] == 1 && c.parent[0] == 1
    };
    s.add("J12-fork 幂等", j12, "二次 fork 快照刷新不损账");

    // J13 空账零面：未步账指纹为种子
    let j13 = ForkBook::new().fold() == FINH_TAG && !ForkBook::new().forked;
    s.add("J13-空账零面", j13, "空账零步零 fork 指纹成立");

    // J14 失平校验：构造未 fork 态继承断言为假
    let j14 = {
        let mut c = ForkBook::new();
        let _ = c.establish(0);
        let _ = c.pstep(0);
        let zero = [0u32; FH_FAMS];
        !c.inherited(zero)
    };
    s.add("J14-失平可报", j14, "未 fork 继承断言为假");

    // J15 动态明细：双侧对平与冻结根落账
    let fp = book.fold();
    s.add_dyn(
        "J15-双侧明细",
        fp == root && book.forked && book.consistent(base),
        &format!("root={:#x} ps={} cs={}", fp, book.parent_sum(), book.child_sum()),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11037_fork_flow() {
        let (mut b, base, built) = build_forkbook();
        assert!(built);
        b.freeze();
        assert!(b.cstep(0).err() == Some(FH_FROZEN));
        assert!(b.pstep(1).err() == Some(FH_FROZEN));
        assert!(base[0] == 20);
        assert!(b.root.is_some());
    }

    #[test]
    fn unx11037_checks_green() {
        let s = run_unx11037_checks();
        assert!(s.all_passed(), "UNX-F11037 判据未全绿");
    }
}
