//! unx0531_nxguard —— UNX-F0531 NX 位执行拒守卫。
//! 锚点：主机 MMU 卷 · 执行禁页三态（页表 NX / 段限 / 账本封印）。
//! 判据原文：NX 置位页取指必拒（拒因账齐），NX 清位页放行；
//!           封印位单向（置后不可清），全拒绝账守恒。

/// 拒因分类：账面分类与总量守恒的基准轴。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rej {
    /// 页表 NX 位为 1：硬件级执行禁。
    TableNx,
    /// 页未 present：映射悬空。
    NotPresent,
    /// 账本封印：软件级永久执行禁。
    Sealed,
}

/// 单页执行守卫账：一张页框一个账位。
#[derive(Debug, Clone, Copy)]
pub struct XGuard {
    /// 线性页基址（4K 对齐）。
    pub base: u64,
    /// 页表 NX 位。
    pub nx: bool,
    /// present 位。
    pub present: bool,
    /// 软件封印（单向置位）。
    pub seal: bool,
}

impl XGuard {
    pub fn new(base: u64) -> Self {
        XGuard { base, nx: false, present: true, seal: false }
    }

    /// 取指请求：三态判定，命中拒因返回 Err。
    pub fn fetch(&self) -> Result<(), Rej> {
        if !self.present {
            return Err(Rej::NotPresent);
        }
        if self.seal {
            return Err(Rej::Sealed);
        }
        if self.nx {
            return Err(Rej::TableNx);
        }
        Ok(())
    }

    /// 封印：单向置位，已封印再封是幂等而非错误。
    pub fn seal_page(&mut self) {
        self.seal = true;
    }
}

/// 拒绝账：按拒因分桶计数。
#[derive(Debug, Default)]
pub struct RejBook {
    pub table_nx: u64,
    pub not_present: u64,
    pub sealed: u64,
}

impl RejBook {
    /// 记一笔拒因。
    pub fn note(&mut self, r: Rej) {
        match r {
            Rej::TableNx => self.table_nx += 1,
            Rej::NotPresent => self.not_present += 1,
            Rej::Sealed => self.sealed += 1,
        }
    }

    /// 三桶总和。
    pub fn total(&self) -> u64 {
        self.table_nx + self.not_present + self.sealed
    }
}

/// 守卫池：按页基址管理的守卫集合。
#[derive(Debug, Default)]
pub struct GuardPool {
    pub pages: alloc::vec::Vec<XGuard>,
}

impl GuardPool {
    /// 定容建池。
    pub fn with_capacity(n: usize) -> Self {
        GuardPool { pages: alloc::vec::Vec::with_capacity(n) }
    }

    /// 挂页。
    pub fn hang(&mut self, base: u64) -> Result<(), ()> {
        if self.pages.iter().any(|p| p.base == base) {
            return Err(()); // 重复挂页拒
        }
        if base & 0xFFF != 0 {
            return Err(()); // 非对齐拒
        }
        self.pages.push(XGuard::new(base));
        Ok(())
    }

    /// 按基址取守卫（可变）。
    pub fn at(&mut self, base: u64) -> Option<&mut XGuard> {
        self.pages.iter_mut().find(|p| p.base == base)
    }

    /// 取指路由：命中守卫走三态，未命中按缺页处理。
    pub fn fetch_route(&self, base: u64, book: &mut RejBook) -> bool {
        match self.pages.iter().find(|p| p.base == base) {
            Some(g) => match g.fetch() {
                Ok(()) => true,
                Err(r) => {
                    book.note(r);
                    false
                }
            },
            None => {
                book.note(Rej::NotPresent);
                false
            }
        }
    }
}

/// 判据入口：NX 执行拒守卫全性质核。
pub fn run_unx0531_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("UNX-F0531");

    // 族一：三态判定正反两面。
    let mut g_ok = XGuard::new(0x1000);
    cs.add("干净页放行", g_ok.fetch().is_ok(), "无 NX 无封印 present");

    g_ok.nx = true;
    cs.add("NX 置位拒", g_ok.fetch() == Err(Rej::TableNx), "页表级执行禁");

    let g_abs = XGuard { base: 0x2000, nx: false, present: false, seal: false };
    cs.add("缺页拒", g_abs.fetch() == Err(Rej::NotPresent), "present 清位优先");

    // 族二：封印单向性。
    let mut g_seal = XGuard::new(0x3000);
    g_seal.seal_page();
    cs.add("封印拒", g_seal.fetch() == Err(Rej::Sealed), "软件级禁执行");
    g_seal.seal_page();
    cs.add("封印幂等", g_seal.fetch() == Err(Rej::Sealed), "再封不错乱");

    // 族三：优先级序（缺页 > 封印 > NX）。
    let g_all = XGuard { base: 0x4000, nx: true, present: false, seal: true };
    cs.add("缺页最优先", g_all.fetch() == Err(Rej::NotPresent), "present 先裁");

    let g_nx_seal = XGuard { base: 0x5000, nx: true, present: true, seal: true };
    cs.add("封印压 NX", g_nx_seal.fetch() == Err(Rej::Sealed), "软件封印先于硬件位");

    // 族四：守卫池挂页闸。
    let mut pool = GuardPool::with_capacity(8);
    cs.add("对齐挂页", pool.hang(0x6000).is_ok(), "4K 对齐收");
    cs.add("非对齐拒", pool.hang(0x6001).is_err(), "低位非零拒");
    cs.add("重挂拒", pool.hang(0x6000).is_err(), "同基址重复拒");
    cs.add("池账", pool.pages.len() == 1, "只进一页");

    // 族五：取指路由与拒绝账。
    let mut pool2 = GuardPool::with_capacity(8);
    pool2.hang(0x7000).ok();
    pool2.hang(0x8000).ok();
    pool2.hang(0x9000).ok();
    pool2.at(0x7000).map(|g| g.nx = true);
    pool2.at(0x8000).map(|g| {
        g.present = false;
        g.seal_page();
    });
    let mut book = RejBook::default();
    cs.add("路由放行", pool2.fetch_route(0x9000, &mut book), "0x9000 干净放行");
    cs.add("路由 NX 拒", !pool2.fetch_route(0x7000, &mut book), "0x7000 执行禁");
    cs.add("路由缺页拒", !pool2.fetch_route(0x8000, &mut book), "0x8000 悬空");
    cs.add("路由未挂拒", !pool2.fetch_route(0xA000, &mut book), "池外页缺页");

    // 族六：拒绝账守恒（三桶和 = 总拒次）。
    cs.add("账三桶", book.table_nx == 1 && book.not_present == 2, "NX 一缺页二");
    cs.add("封印桶零", book.sealed == 0, "本轮未触发封印页");
    cs.add("账守恒", book.total() == 3, "三拒齐账");

    // 族七：封印后放行面清零（NX 清了也拒）。
    let mut pool3 = GuardPool::with_capacity(4);
    pool3.hang(0xB000).ok();
    pool3.at(0xB000).map(|g| {
        g.nx = true;
        g.seal_page();
        g.nx = false; // 试清 NX 绕封印
    });
    let mut book2 = RejBook::default();
    cs.add("封印堵绕行", !pool3.fetch_route(0xB000, &mut book2), "清 NX 无效");
    cs.add("绕行入封印桶", book2.sealed == 1, "拒因正确归桶");

    // 族八：重复路由幂等（账只增不倒退）。
    let t1 = book.total();
    let _ = pool2.fetch_route(0x7000, &mut book);
    let _ = pool2.fetch_route(0x7000, &mut book);
    cs.add("重复拒单调", book.total() == t1 + 2, "每次拒均入账");
    cs.add("账只增", book.table_nx == 3, "NX 桶累计三");

    cs
}
