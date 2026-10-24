//! unx0532_wptrap —— UNX-F0532 写保护陷阱分类账。
//! 锚点：主机 MMU 卷 · WP 违例四因（只读页 / 用户写内核 / CA 态 / 账本封写）。
//! 判据原文：违例必归类（四因互斥完备），放行仅限账面持写权者；
//!           陷阱账按因分桶守恒，重复违例单调。

/// 违例四因：互斥完备的 WP 陷阱分类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WpCause {
    /// 目标页只读（R/W 位清零）。
    ReadOnly,
    /// 用户态写内核页（U/S 位违反）。
    UserKern,
    /// 页处于不可写缓存态（账面 W^X 联动）。
    WxState,
    /// 账本封写（软件级单写者锁）。
    Sealed,
}

/// 单页写权账。
#[derive(Debug, Clone, Copy)]
pub struct WpPage {
    pub base: u64,
    /// R/W 位（1 = 可写）。
    pub rw: bool,
    /// U/S 位（1 = 用户可访）。
    pub us: bool,
    /// W^X 态：可执行页强制不可写。
    pub exec: bool,
    /// 封写位。
    pub seal: bool,
}

impl WpPage {
    pub fn new(base: u64, rw: bool, us: bool) -> Self {
        WpPage { base, rw, us, exec: false, seal: false }
    }

    /// 写尝试分类：Ok 放行，Err 返回互斥违例因。
    pub fn write(&self, user_mode: bool) -> Result<(), WpCause> {
        if !self.rw {
            return Err(WpCause::ReadOnly);
        }
        if user_mode && !self.us {
            return Err(WpCause::UserKern);
        }
        if self.exec {
            return Err(WpCause::WxState);
        }
        if self.seal {
            return Err(WpCause::Sealed);
        }
        Ok(())
    }
}

/// 陷阱账：四桶分计。
#[derive(Debug, Default)]
pub struct TrapBook {
    pub ro: u64,
    pub uk: u64,
    pub wx: u64,
    pub sealed: u64,
}

impl TrapBook {
    pub fn note(&mut self, c: WpCause) {
        match c {
            WpCause::ReadOnly => self.ro += 1,
            WpCause::UserKern => self.uk += 1,
            WpCause::WxState => self.wx += 1,
            WpCause::Sealed => self.sealed += 1,
        }
    }

    pub fn total(&self) -> u64 {
        self.ro + self.uk + self.wx + self.sealed
    }
}

/// 陷阱路由：页表查页 → 分类 → 入账。返回是否放行。
pub fn trap_route(pages: &alloc::vec::Vec<WpPage>, base: u64, user_mode: bool, book: &mut TrapBook) -> bool {
    match pages.iter().find(|p| p.base == base) {
        Some(p) => match p.write(user_mode) {
            Ok(()) => true,
            Err(c) => {
                book.note(c);
                false
            }
        },
        None => {
            book.note(WpCause::ReadOnly); // 未映射按只读缺页归桶
            false
        }
    }
}

/// 判据入口：WP 陷阱全性质核。
pub fn run_unx0532_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("UNX-F0532");

    // 族一：四因互斥正例（每因单独触发）。
    let p_ro = WpPage::new(0x1000, false, true);
    cs.add("只读因", p_ro.write(false) == Err(WpCause::ReadOnly), "R/W 清零");

    let p_uk = WpPage::new(0x2000, true, false);
    cs.add("用户写内核因", p_uk.write(true) == Err(WpCause::UserKern), "U/S 违反");

    let mut p_wx = WpPage::new(0x3000, true, true);
    p_wx.exec = true;
    cs.add("W^X 因", p_wx.write(false) == Err(WpCause::WxState), "可执行页禁写");

    let mut p_seal = WpPage::new(0x4000, true, true);
    p_seal.seal = true;
    cs.add("封写因", p_seal.write(false) == Err(WpCause::Sealed), "单写者锁");

    // 族二：用户态豁免（内核态穿 U/S）。
    cs.add("内核态穿 U/S", p_uk.write(false).is_ok(), "Ring0 越用户位合法");

    // 族三：优先级序（只读 > 用户内核 > W^X > 封写）。
    let p_all = WpPage { base: 0x5000, rw: false, us: false, exec: true, seal: true };
    cs.add("只读最优先", p_all.write(true) == Err(WpCause::ReadOnly), "R/W 先裁");

    let p_uk_wx = WpPage { base: 0x6000, rw: true, us: false, exec: true, seal: true };
    cs.add("UK 压 WX", p_uk_wx.write(true) == Err(WpCause::UserKern), "U/S 次序第二");

    let p_wx_seal = WpPage { base: 0x7000, rw: true, us: true, exec: true, seal: true };
    cs.add("WX 压封写", p_wx_seal.write(false) == Err(WpCause::WxState), "W^X 第三");

    // 族四：路由与四桶账。
    let pages = {
        let mut v = alloc::vec::Vec::new();
        v.push(p_ro);
        v.push(p_uk);
        v.push(p_wx);
        v.push(p_seal);
        v.push(WpPage::new(0x8000, true, true));
        v
    };
    let mut book = TrapBook::default();
    cs.add("路由放行", trap_route(&pages, 0x8000, false, &mut book), "干净页写通");
    cs.add("路由 RO", !trap_route(&pages, 0x1000, false, &mut book), "只读拒");
    cs.add("路由 UK", !trap_route(&pages, 0x2000, true, &mut book), "用户写内核拒");
    cs.add("路由 WX", !trap_route(&pages, 0x3000, false, &mut book), "W^X 拒");
    cs.add("路由 Seal", !trap_route(&pages, 0x4000, false, &mut book), "封写拒");
    cs.add("路由未映射", !trap_route(&pages, 0x9000, false, &mut book), "池外归 RO 桶");

    // 族五：账守恒与分桶核。
    cs.add("RO 桶二", book.ro == 2, "显式 RO + 未映射");
    cs.add("其余桶各一", book.uk == 1 && book.wx == 1 && book.sealed == 1, "三因各一");
    cs.add("账总守恒", book.total() == 5, "四桶和 = 总拒次");

    // 族六：重复违例单调（账只增）。
    let t1 = book.total();
    let _ = trap_route(&pages, 0x1000, false, &mut book);
    let _ = trap_route(&pages, 0x1000, false, &mut book);
    cs.add("重复拒单调", book.total() == t1 + 2, "两次再拒两账");
    cs.add("RO 桶累计", book.ro == 4, "桶内累计四");

    // 族七：封写解除后放行（解除是显式动作）。
    let mut p_rel = WpPage::new(0xA000, true, true);
    p_rel.seal = true;
    let pages2 = { let mut v = alloc::vec::Vec::new(); v.push(p_rel); v };
    let mut book2 = TrapBook::default();
    cs.add("封写先拒", !trap_route(&pages2, 0xA000, false, &mut book2), "未解封拒");
    // 解封（重建账面条目模拟显式解除）。
    let pages3 = { let mut v = alloc::vec::Vec::new(); v.push(WpPage::new(0xA000, true, true)); v };
    cs.add("解封放行", trap_route(&pages3, 0xA000, false, &mut book2), "重建账面通");

    // 族八：W^X 联动核（置执行位后原可写页翻拒）。
    let mut p_flip = WpPage::new(0xB000, true, true);
    let pages4 = { let mut v = alloc::vec::Vec::new(); v.push(p_flip); v };
    let mut book3 = TrapBook::default();
    cs.add("翻前可写", trap_route(&pages4, 0xB000, false, &mut book3), "初始通");
    p_flip.exec = true;
    let pages5 = { let mut v = alloc::vec::Vec::new(); v.push(p_flip); v };
    cs.add("翻后 W^X 拒", !trap_route(&pages5, 0xB000, false, &mut book3), "置执行位即禁写");
    cs.add("联动入账", book3.wx == 1, "W^X 桶收一");

    cs
}
