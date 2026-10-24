//! unx0538_sharecred —— UNX-F0538 共享映射权限升降账。
//! 锚点：主机 MMU 卷 · 共享页权限升降（升权带锁 / 降权即时 / 单向降）。
//! 判据原文：升权须全员共识（任一持有者拒则整体拒），降权即时生效
//!           （无共识），权限只能降不能升（单向棘轮）。

/// 共享映射条目。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Share {
    pub holder: u8,
    /// 权限位：bit0=R bit1=W。
    pub perms: u8,
}

/// 权限棘轮账。
#[derive(Debug)]
pub struct Ratchet {
    pub va: u64,
    pub shares: alloc::vec::Vec<Share>,
    /// 历史最高权限（棘轮顶）。
    pub peak: u8,
    /// 升权账 / 降权账 / 拒账。
    pub n_up: u64,
    pub n_down: u64,
    pub n_reject: u64,
}

/// 权限合法集：R/W 双位，W 必带 R。
pub fn perm_valid(p: u8) -> bool {
    let r = p & 1 != 0;
    let w = p & 2 != 0;
    (r || w) && (!w || r)
}

impl Ratchet {
    pub fn new(va: u64, holders: &[u8]) -> Self {
        let shares = holders.iter().map(|h| Share { holder: *h, perms: 0b11 }).collect();
        Ratchet { va, shares, peak: 0b11, n_up: 0, n_down: 0, n_reject: 0 }
    }

    /// 查持有者权限。
    pub fn perm_of(&self, holder: u8) -> Option<u8> {
        self.shares.iter().find(|s| s.holder == holder).map(|s| s.perms)
    }

    /// 降权：单持有者即时降，无需共识。新权限必须 ≤ 旧权限。
    pub fn demote(&mut self, holder: u8, to: u8) -> Result<(), ()> {
        if !perm_valid(to) {
            self.n_reject += 1;
            return Err(());
        }
        let s = match self.shares.iter_mut().find(|s| s.holder == holder) {
            Some(s) => s,
            None => {
                self.n_reject += 1;
                return Err(());
            }
        };
        if (to & !s.perms) != 0 {
            // 含有旧权限没有的位 → 是升权，棘轮拒。
            self.n_reject += 1;
            return Err(());
        }
        s.perms = to;
        self.n_down += 1;
        Ok(())
    }

    /// 升权：须全员共识（每位持有者均允许升）。
    /// 共识模拟：传入选民闭包表——全 true 才升。
    pub fn promote(&mut self, to: u8, votes: &[bool]) -> Result<(), ()> {
        if !perm_valid(to) {
            self.n_reject += 1;
            return Err(());
        }
        if votes.len() != self.shares.len() || votes.iter().any(|v| !*v) {
            self.n_reject += 1;
            return Err(());
        }
        for s in self.shares.iter_mut() {
            s.perms = to;
        }
        if to > self.peak {
            self.peak = to;
        }
        self.n_up += 1;
        Ok(())
    }

    /// 全员同权核。
    pub fn uniform(&self) -> bool {
        let first = self.shares[0].perms;
        self.shares.iter().all(|s| s.perms == first)
    }
}

/// 判据入口：共享权限升降全性质核。
pub fn run_unx0538_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("UNX-F0538");

    // 族一：权限集合法性。
    cs.add("R 合法", perm_valid(0b01), "纯读");
    cs.add("RW 合法", perm_valid(0b11), "读写");
    cs.add("W 无 R 拒", !perm_valid(0b10), "孤立写拒");
    cs.add("零权限拒", !perm_valid(0), "无权拒");

    // 族二：初始共享态。
    let mut r = Ratchet::new(0x1000, &[1, 2, 3]);
    cs.add("三持有者", r.shares.len() == 3, "fork 三进程");
    cs.add("初始全 RW", r.uniform() && r.perm_of(1) == Some(0b11), "全员读写");
    cs.add("查无此人", r.perm_of(9).is_none(), "未挂查空");

    // 族三：降权即时（无共识）。
    cs.add("降 2 号为 R", r.demote(2, 0b01).is_ok(), "单方降权通");
    cs.add("2 号只读", r.perm_of(2) == Some(0b01), "即时生效");
    cs.add("他人不变", r.perm_of(1) == Some(0b11) && r.perm_of(3) == Some(0b11), "局部性");
    cs.add("降账一", r.n_down == 1, "一次降权");

    // 族四：棘轮拒升（demote 只能降）。
    cs.add("demote 升权拒", r.demote(2, 0b11).is_err(), "R→RW 是升权");
    cs.add("拒账一", r.n_reject == 1, "棘轮拒入账");
    cs.add("2 号仍只读", r.perm_of(2) == Some(0b01), "拒后不变");

    // 族五：升权须全员共识。
    cs.add("共识缺一拒", r.promote(0b01, &[true, true, false]).is_err(), "有反对票");
    cs.add("票数不符拒", r.promote(0b01, &[true, true]).is_err(), "票数少于持有者");
    cs.add("全票升通", r.promote(0b01, &[true, true, true]).is_ok(), "全员同意降");
    cs.add("升降后全 R", r.uniform() && r.perm_of(1) == Some(0b01), "统一为只读");
    cs.add("升账一", r.n_up == 1, "一次共识操作");

    // 族六：升权回 RW（共识 + 棘轮顶更新）。
    cs.add("共识升 RW", r.promote(0b11, &[true, true, true]).is_ok(), "全票恢复");
    cs.add("全员 RW", r.uniform(), "恢复读写");
    cs.add("棘轮顶不变", r.peak == 0b11, "顶已封");

    // 族七：查无此人降权拒。
    cs.add("降幽灵拒", r.demote(9, 0b01).is_err(), "未挂 holder 拒");
    cs.add("降权到非法拒", r.demote(1, 0b10).is_err(), "W 无 R 拒");

    // 族八：批量升降压力守恒。
    let mut r2 = Ratchet::new(0x2000, &[1, 2, 3, 4, 5, 6, 7, 8]);
    let mut downs = 0u64;
    for h in 1u8..=8 {
        if r2.demote(h, 0b01).is_ok() {
            downs += 1;
        }
    }
    cs.add("八降全收", downs == 8 && r2.uniform(), "全员降至 R");
    cs.add("账对齐", r2.n_down == 8 && r2.n_up == 0 && r2.n_reject == 0, "零拒零升");
    let ups = r2.promote(0b11, &[true; 8]);
    cs.add("八票升通", ups.is_ok(), "全员共识恢复");
    cs.add("终账", r2.n_up == 1 && r2.n_down == 8, "8 降 1 升");

    cs
}
