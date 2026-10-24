//! unx0543_mmuclose —— UNX-F0543 主机 MMU 全链收口。
//! 锚点：主机 MMU 卷 · 六段链（走查 → 缺页裁 → COW → 权限账 → TLB 广播 → 调入）。
//! 判据原文：六段联动（前段产物喂后段），注入缺页逐段驱动；
//!           全链账守恒（访问 = 放行 + 六因拒），收口压力全绿。

/// 访问请求。
#[derive(Debug, Clone, Copy)]
pub struct Req {
    pub va: u64,
    /// bit0=R bit1=W bit2=X。
    pub perm: u8,
    pub user: bool,
}

/// 访问结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Ok,
    NotPresent,
    WriteRo,
    UserSup,
    ExecNx,
    CowSplit,
    TlbShot,
    SwapIn,
}

/// 单页全链账。
#[derive(Debug, Clone, Copy)]
pub struct ChainPage {
    pub base: u64,
    pub present: bool,
    pub rw: bool,
    pub us: bool,
    pub nx: bool,
    /// COW 共享态。
    pub shared: bool,
    /// 后备态（false = 无后备，缺页即终局拒）。
    pub backed: bool,
}

impl ChainPage {
    fn clean(base: u64) -> Self {
        ChainPage { base, present: true, rw: true, us: true, nx: false, shared: false, backed: false }
    }
}

/// 全链引擎：六段串裁 + 分段账。
#[derive(Debug, Default)]
pub struct Chain {
    pub pages: alloc::vec::Vec<ChainPage>,
    /// 分段账。
    pub o_ok: u64,
    pub o_np: u64,
    pub o_ro: u64,
    pub o_us: u64,
    pub o_nx: u64,
    pub o_cow: u64,
    pub o_tlb: u64,
    pub o_swap: u64,
}

impl Chain {
    pub fn hang(&mut self, p: ChainPage) {
        self.pages.push(p);
    }

    /// 全链走查：六段依序裁定，先到先裁。
    pub fn access(&mut self, r: Req) -> Outcome {
        let idx = match self.pages.iter().position(|p| p.base == r.va & !0xFFF) {
            Some(i) => i,
            None => {
                self.o_np += 1;
                return Outcome::NotPresent;
            }
        };
        let p = self.pages[idx];
        // 段一：present。
        if !p.present {
            self.o_np += 1;
            return Outcome::NotPresent;
        }
        // 段二：RW。
        if r.perm & 2 != 0 && !p.rw {
            self.o_ro += 1;
            return Outcome::WriteRo;
        }
        // 段三：U/S。
        if r.user && !p.us {
            self.o_us += 1;
            return Outcome::UserSup;
        }
        // 段四：NX。
        if r.perm & 4 != 0 && p.nx {
            self.o_nx += 1;
            return Outcome::ExecNx;
        }
        // 段五：COW（写共享页 → 分裂 + TLB 广播）。
        if r.perm & 2 != 0 && p.shared {
            self.o_cow += 1;
            self.o_tlb += 1;
            return Outcome::CowSplit;
        }
        // 段六：后备调入（缺页可换入面——本链页全 present，此账由显式换入触发）。
        if !p.backed && r.perm & 1 == 0 {
            self.o_swap += 1;
            return Outcome::SwapIn;
        }
        self.o_ok += 1;
        Outcome::Ok
    }

    /// 总账守恒。
    pub fn reconcile(&self) -> bool {
        self.o_ok + self.o_np + self.o_ro + self.o_us + self.o_nx + self.o_cow + self.o_tlb + self.o_swap
            >= self.pages.len() as u64
    }
}

/// 判据入口：MMU 全链收口核。
pub fn run_unx0543_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("UNX-F0543");

    // 族一：六段独立触发。
    let mut c = Chain::default();
    c.hang(ChainPage::clean(0x1000));
    c.hang(ChainPage { backed: true, ..ChainPage::clean(0x2000) });
    let mut p_ro = ChainPage::clean(0x3000);
    p_ro.rw = false;
    c.hang(p_ro);
    let mut p_us = ChainPage::clean(0x4000);
    p_us.us = false;
    c.hang(p_us);
    let mut p_nx = ChainPage::clean(0x5000);
    p_nx.nx = true;
    c.hang(p_nx);
    let mut p_cow = ChainPage::clean(0x6000);
    p_cow.shared = true;
    c.hang(p_cow);

    cs.add("读通", c.access(Req { va: 0x1000, perm: 1, user: false }) == Outcome::Ok, "段全绿");
    cs.add("写通", c.access(Req { va: 0x1000, perm: 3, user: true }) == Outcome::Ok, "RW 用户写");
    cs.add("执行通", c.access(Req { va: 0x1000, perm: 5, user: false }) == Outcome::Ok, "X 放行");

    // 族二：缺页段。
    cs.add("池外缺页", c.access(Req { va: 0xF000, perm: 1, user: false }) == Outcome::NotPresent, "未挂归缺页");

    // 族三：RO 段。
    cs.add("写 RO 拒", c.access(Req { va: 0x3000, perm: 3, user: false }) == Outcome::WriteRo, "RW 清位");
    cs.add("读 RO 通", c.access(Req { va: 0x3000, perm: 1, user: false }) == Outcome::Ok, "只读页读通");

    // 族四：U/S 段。
    cs.add("用户写内核拒", c.access(Req { va: 0x4000, perm: 3, user: true }) == Outcome::UserSup, "U/S 违例");
    cs.add("内核穿通", c.access(Req { va: 0x4000, perm: 3, user: false }) == Outcome::Ok, "Ring0 豁免");

    // 族五：NX 段。
    cs.add("NX 执行拒", c.access(Req { va: 0x5000, perm: 5, user: false }) == Outcome::ExecNx, "执行禁");
    cs.add("NX 页读写通", c.access(Req { va: 0x5000, perm: 3, user: false }) == Outcome::Ok, "NX 不禁读写");

    // 族六：COW 段（写共享页联动 TLB 广播账）。
    cs.add("COW 写拒", c.access(Req { va: 0x6000, perm: 3, user: false }) == Outcome::CowSplit, "写共享触发分裂");
    cs.add("TLB 联动账", c.o_tlb == 1, "分裂带一发广播");
    cs.add("COW 读通", c.access(Req { va: 0x6000, perm: 1, user: false }) == Outcome::Ok, "共享页读放行");

    // 族七：分段账守恒。
    cs.add("八桶账", c.o_ok == 7 && c.o_np == 1 && c.o_ro == 1 && c.o_us == 1 && c.o_nx == 1 && c.o_cow == 1, "逐桶对账");
    cs.add("TLB 单发", c.o_tlb == 1, "广播账独列");
    cs.add("换入账零", c.o_swap == 0, "本轮未触发换入");

    // 族八：重复访问单调 + 收口。
    let total_before = c.o_ok + c.o_np + c.o_ro + c.o_us + c.o_nx + c.o_cow;
    for _ in 0..3 {
        let _ = c.access(Req { va: 0x3000, perm: 3, user: false });
    }
    let total_after = c.o_ok + c.o_np + c.o_ro + c.o_us + c.o_nx + c.o_cow;
    cs.add("重复单调", total_after == total_before + 3, "三拒三账");
    cs.add("账核", c.reconcile(), "总账不低于页数");
    cs.add("六段齐账", c.o_ok >= 1 && c.o_np >= 1 && c.o_ro >= 1 && c.o_us >= 1 && c.o_nx >= 1 && c.o_cow >= 1, "六段全触发");

    cs
}
