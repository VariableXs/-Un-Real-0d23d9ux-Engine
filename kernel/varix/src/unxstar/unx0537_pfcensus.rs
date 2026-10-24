//! unx0537_pfcensus —— UNX-F0537 缺页原因聚类账。
//! 锚点：主机 MMU 卷 · 缺页五因聚类（present / RW / US / NX / 保key）。
//! 判据原文：每缺页必归类（五因互斥按位序裁定），聚类和 = 总缺页，
//!           热点页定位（同页重复缺页计数单调）。

/// 缺页五因：按裁定优先级排列（先到先裁）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PfCause {
    /// 页不在内存（present=0）。
    NotPresent,
    /// 写只读页（RW 违例）。
    WriteRo,
    /// 用户态越权（U/S 违例）。
    UserSup,
    /// 执行 NX 页。
    ExecNx,
    /// 保护键违例（软件模拟）。
    ProtKey,
}

/// 单页缺页账。
#[derive(Debug, Clone, Copy)]
pub struct PfPage {
    pub base: u64,
    pub present: bool,
    pub rw: bool,
    pub us: bool,
    pub nx: bool,
    /// 保护键（0 = 无键约束）。
    pub pkey: u16,
    /// 本页缺页次数。
    pub hits: u64,
}

impl PfPage {
    pub fn new(base: u64) -> Self {
        PfPage { base, present: true, rw: true, us: true, nx: false, pkey: 0, hits: 0 }
    }

    /// 访问裁定：Ok 放行，Err 缺页因（五因按优先级互斥裁定）。
    /// access: bit0=R bit1=W bit2=X；user 为用户态标志。
    pub fn access(&mut self, perm: u8, user: bool, key_allow: bool) -> Result<(), PfCause> {
        let w = perm & 2 != 0;
        let x = perm & 4 != 0;
        if !self.present {
            self.hits += 1;
            return Err(PfCause::NotPresent);
        }
        if w && !self.rw {
            self.hits += 1;
            return Err(PfCause::WriteRo);
        }
        if user && !self.us {
            self.hits += 1;
            return Err(PfCause::UserSup);
        }
        if x && self.nx {
            self.hits += 1;
            return Err(PfCause::ExecNx);
        }
        if self.pkey != 0 && !key_allow {
            self.hits += 1;
            return Err(PfCause::ProtKey);
        }
        Ok(())
    }
}

/// 聚类账：五桶分计 + 热点定位。
#[derive(Debug, Default)]
pub struct Census {
    pub c_not_present: u64,
    pub c_write_ro: u64,
    pub c_user_sup: u64,
    pub c_exec_nx: u64,
    pub c_prot_key: u64,
    /// 热点页基址（缺页最多者）。
    pub hot_base: Option<u64>,
    pub hot_hits: u64,
}

impl Census {
    pub fn note(&mut self, c: PfCause) {
        match c {
            PfCause::NotPresent => self.c_not_present += 1,
            PfCause::WriteRo => self.c_write_ro += 1,
            PfCause::UserSup => self.c_user_sup += 1,
            PfCause::ExecNx => self.c_exec_nx += 1,
            PfCause::ProtKey => self.c_prot_key += 1,
        }
    }

    pub fn total(&self) -> u64 {
        self.c_not_present + self.c_write_ro + self.c_user_sup + self.c_exec_nx + self.c_prot_key
    }

    /// 热点定位：遍历页账取 hits 最大者。
    pub fn locate_hot(&mut self, pages: &mut [PfPage]) {
        self.hot_base = None;
        self.hot_hits = 0;
        for p in pages.iter() {
            if p.hits > self.hot_hits {
                self.hot_hits = p.hits;
                self.hot_base = Some(p.base);
            }
        }
    }
}

/// 裁定路由：查页 → 访问 → 入聚类账。
pub fn route(pages: &mut alloc::vec::Vec<PfPage>, base: u64, perm: u8, user: bool, key_allow: bool, cen: &mut Census) -> bool {
    let idx = match pages.iter().position(|p| p.base == base) {
        Some(i) => i,
        None => {
            cen.note(PfCause::NotPresent);
            return false;
        }
    };
    match pages[idx].access(perm, user, key_allow) {
        Ok(()) => true,
        Err(c) => {
            cen.note(c);
            false
        }
    }
}

/// 判据入口：缺页聚类全性质核。
pub fn run_unx0537_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("UNX-F0537");

    // 族一：五因逐一触发。
    let mut p_np = PfPage::new(0x1000);
    p_np.present = false;
    cs.add("缺页因", p_np.access(1, false, true) == Err(PfCause::NotPresent), "present 清");

    let mut p_ro = PfPage::new(0x2000);
    p_ro.rw = false;
    cs.add("写只读因", p_ro.access(3, false, true) == Err(PfCause::WriteRo), "RW 清位");

    let mut p_us = PfPage::new(0x3000);
    p_us.us = false;
    cs.add("越权因", p_us.access(1, true, true) == Err(PfCause::UserSup), "U/S 违例");

    let mut p_nx = PfPage::new(0x4000);
    p_nx.nx = true;
    cs.add("NX 因", p_nx.access(5, false, true) == Err(PfCause::ExecNx), "执行禁页");

    let mut p_pk = PfPage::new(0x5000);
    p_pk.pkey = 3;
    cs.add("保key因", p_pk.access(1, false, false) == Err(PfCause::ProtKey), "键不容");
    cs.add("键容放行", p_pk.access(1, false, true).is_ok(), "键容即通");

    // 族二：裁定优先级（present > RO > US > NX > pkey）。
    let mut p_all = PfPage::new(0x6000);
    p_all.present = false;
    p_all.rw = false;
    p_all.us = false;
    p_all.nx = true;
    p_all.pkey = 1;
    cs.add("缺页最优先", p_all.access(7, true, false) == Err(PfCause::NotPresent), "present 先裁");

    let mut p_ro_us = PfPage::new(0x7000);
    p_ro_us.rw = false;
    p_ro_us.us = false;
    p_ro_us.nx = true;
    p_ro_us.pkey = 1;
    cs.add("RO 压 US", p_ro_us.access(7, true, false) == Err(PfCause::WriteRo), "RW 次序第二");

    let mut p_us_nx = PfPage::new(0x8000);
    p_us_nx.us = false;
    p_us_nx.nx = true;
    cs.add("US 压 NX", p_us_nx.access(7, true, false) == Err(PfCause::UserSup), "U/S 第三");

    let mut p_nx_pk = PfPage::new(0x9000);
    p_nx_pk.nx = true;
    p_nx_pk.pkey = 1;
    cs.add("NX 压保key", p_nx_pk.access(5, false, false) == Err(PfCause::ExecNx), "NX 第四");

    // 族三：路由与聚类账。
    let mut pages = {
        let mut v = alloc::vec::Vec::new();
        v.push(p_ro);
        v.push(p_us);
        v.push(p_nx);
        v.push(p_pk);
        v.push(PfPage::new(0xA000));
        v
    };
    let mut cen = Census::default();
    cs.add("路由放行", route(&mut pages, 0xA000, 1, false, true, &mut cen), "干净页通");
    cs.add("路由 RO", !route(&mut pages, 0x2000, 3, false, true, &mut cen), "写只读拒");
    cs.add("路由 US", !route(&mut pages, 0x3000, 1, true, true, &mut cen), "越权拒");
    cs.add("路由 NX", !route(&mut pages, 0x4000, 5, false, true, &mut cen), "NX 拒");
    cs.add("路由 PK", !route(&mut pages, 0x5000, 1, false, false, &mut cen), "保key拒");
    cs.add("路由池外", !route(&mut pages, 0xB000, 1, false, true, &mut cen), "未挂归缺页");

    // 族四：聚类守恒（五桶和 = 总缺页）。
    cs.add("五桶各一", cen.c_write_ro == 1 && cen.c_user_sup == 1 && cen.c_exec_nx == 1 && cen.c_prot_key == 1, "四因各一");
    cs.add("缺页桶一", cen.c_not_present == 1, "池外一");
    cs.add("聚类和守恒", cen.total() == 5, "五桶和 = 5");

    // 族五：热点定位（同页重复缺页）。
    for _ in 0..4 {
        let _ = route(&mut pages, 0x2000, 3, false, true, &mut cen);
    }
    cen.locate_hot(&mut pages);
    cs.add("热点页定位", cen.hot_base == Some(0x2000), "0x2000 五缺");
    cs.add("热点计数", cen.hot_hits == 5, "1+4=5");
    cs.add("页账同步", pages.iter().find(|p| p.base == 0x2000).map(|p| p.hits) == Some(5), "页账一致");

    cs
}
