//! unx0536_mapaudit —— UNX-F0536 映射账本守恒审计。
//! 锚点：主机 MMU 卷 · 映射账（map/unmap/remap 三流 + 守恒）。
//! 判据原文：映射账实相符（账面数 = 活映射数），重映射必先卸后挂
//!           （账不重项），卸不存在映射拒；三流守恒。

/// 单条映射。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mapping {
    pub va: u64,
    pub pa: u64,
    /// 权限：bit0=R bit1=W bit2=X。
    pub perms: u8,
}

/// 映射账本。
#[derive(Debug, Default)]
pub struct MapBook {
    pub ents: alloc::vec::Vec<Mapping>,
    /// 三流计数。
    pub n_map: u64,
    pub n_unmap: u64,
    pub n_remap: u64,
    /// 拒绝计数。
    pub n_reject: u64,
}

/// 权限合法性：至少 R，W 不带 R 拒，X 带写拒（W^X）。
pub fn perms_ok(p: u8) -> bool {
    let r = p & 1 != 0;
    let w = p & 2 != 0;
    let x = p & 4 != 0;
    if !r && !w && !x {
        return false; // 空权限拒
    }
    if w && !r {
        return false; // 写不带读拒
    }
    if x && w {
        return false; // W^X
    }
    true
}

impl MapBook {
    /// 挂映射：VA 唯一 + 权限合法。
    pub fn map(&mut self, va: u64, pa: u64, perms: u8) -> Result<(), ()> {
        if !perms_ok(perms) {
            self.n_reject += 1;
            return Err(());
        }
        if self.ents.iter().any(|m| m.va == va) {
            self.n_reject += 1;
            return Err(());
        }
        self.ents.push(Mapping { va, pa, perms });
        self.n_map += 1;
        Ok(())
    }

    /// 卸映射：存在才卸。
    pub fn unmap(&mut self, va: u64) -> Result<(), ()> {
        let before = self.ents.len();
        self.ents.retain(|m| m.va != va);
        if self.ents.len() == before {
            self.n_reject += 1;
            return Err(());
        }
        self.n_unmap += 1;
        Ok(())
    }

    /// 重映射 = 卸旧 + 挂新（原子语义：新权限非法则旧映射保留）。
    pub fn remap(&mut self, va: u64, pa: u64, perms: u8) -> Result<(), ()> {
        if !perms_ok(perms) || !self.ents.iter().any(|m| m.va == va) {
            self.n_reject += 1;
            return Err(());
        }
        self.unmap(va).ok();
        self.map(va, pa, perms).ok();
        self.n_remap += 1;
        Ok(())
    }

    /// 查映射。
    pub fn lookup(&self, va: u64) -> Option<&Mapping> {
        self.ents.iter().find(|m| m.va == va)
    }

    /// 账实守恒：账面数 = 活映射数，且 VA 无重项。
    pub fn reconcile(&self) -> bool {
        let unique = {
            let mut vas: alloc::vec::Vec<u64> = self.ents.iter().map(|m| m.va).collect();
            vas.sort_unstable();
            vas.dedup();
            vas.len()
        };
        unique == self.ents.len()
    }

    /// 三流恒等：挂流 = 活映射 + 卸流。
    pub fn flow_ok(&self) -> bool {
        self.n_map == self.ents.len() as u64 + self.n_unmap
    }
}

/// 判据入口：映射账本全性质核。
pub fn run_unx0536_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("UNX-F0536");

    // 族一：权限闸。
    cs.add("RX 合法", perms_ok(0b101), "读执行可");
    cs.add("RW 合法", perms_ok(0b011), "读写可");
    cs.add("R 合法", perms_ok(0b001), "纯读可");
    cs.add("空权限拒", !perms_ok(0), "零权限拒");
    cs.add("W 无 R 拒", !perms_ok(0b010), "写不带读拒");
    cs.add("W^X 拒", !perms_ok(0b110), "写执行互斥");

    // 族二：挂映射。
    let mut b = MapBook::default();
    cs.add("首挂收", b.map(0x1000, 0x9000, 0b101).is_ok(), "RX 页入账");
    cs.add("VA 重挂拒", b.map(0x1000, 0xA000, 0b011).is_err(), "同 VA 拒");
    cs.add("非法权限拒", b.map(0x2000, 0xB000, 0b110).is_err(), "W^X 拒");
    cs.add("账二活一", b.n_map == 1 && b.ents.len() == 1, "拒不入账");

    // 族三：查映射与账实核。
    let m = b.lookup(0x1000);
    cs.add("查得映射", m.map(|x| x.pa) == Some(0x9000), "VA→PA 对");
    cs.add("查空映射", b.lookup(0x3000).is_none(), "未挂查空");
    cs.add("账实核", b.reconcile() && b.flow_ok(), "双守恒");

    // 族四：卸映射。
    cs.add("卸存在", b.unmap(0x1000).is_ok(), "卸已挂");
    cs.add("卸不存在拒", b.unmap(0x1000).is_err(), "空卸拒");
    cs.add("卸后账空", b.ents.is_empty(), "账清");
    cs.add("流恒等", b.flow_ok(), "挂一卸一活零");

    // 族五：重映射原子性。
    b.map(0x4000, 0xC000, 0b011).ok();
    cs.add("重映射通", b.remap(0x4000, 0xD000, 0b101).is_ok(), "RW→RX 换 PA");
    cs.add("新 PA 生效", b.lookup(0x4000).map(|x| x.pa) == Some(0xD000), "换靶成功");
    cs.add("新权限生效", b.lookup(0x4000).map(|x| x.perms) == Some(0b101), "权限换档");
    cs.add("重映射计一", b.n_remap == 1, "一账");

    // 族六：重映射原子保旧（新权限非法不破旧映射）。
    let old_pa = b.lookup(0x4000).map(|x| x.pa);
    cs.add("非法重映射拒", b.remap(0x4000, 0xE000, 0b110).is_err(), "W^X 拒");
    cs.add("旧映射保留", b.lookup(0x4000).map(|x| x.pa) == old_pa, "原子保旧");
    cs.add("重映射计不进", b.n_remap == 1, "失败不记账");

    // 族七：批量压力守恒（挂 50 卸 25 重映射 10）。
    let mut b2 = MapBook::default();
    for i in 0..50u64 {
        b2.map(0x10_0000 + i * 0x1000, 0x20_0000 + i * 0x1000, 0b101).ok();
    }
    for i in 0..25u64 {
        b2.unmap(0x10_0000 + i * 0x1000).ok();
    }
    for i in 25..35u64 {
        b2.remap(0x10_0000 + i * 0x1000, 0x30_0000 + i * 0x1000, 0b011).ok();
    }
    cs.add("批量活 25", b2.ents.len() == 25, "50-25=25");
    cs.add("批量账实", b2.reconcile(), "VA 无重项");
    cs.add("批量流恒等", b2.flow_ok(), "挂 50 = 活 25 + 卸 25");
    cs.add("批量重映射十", b2.n_remap == 10, "十次换靶");

    // 族八：重复卸压单调拒账。
    let r0 = b2.n_reject;
    for _ in 0..3 {
        let _ = b2.unmap(0x10_0000);
    }
    cs.add("三卸三拒", b2.n_reject == r0 + 3, "重复空卸均拒");
    cs.add("拒账不破守恒", b2.reconcile() && b2.flow_ok(), "拒绝流不扰主流");

    // 族九：三权限组合遍历（R/RW/RX 全合法集挂卸往返）。
    let mut b3 = MapBook::default();
    let perms_set: [u8; 3] = [0b001, 0b011, 0b101];
    let mut all_map = true;
    for (i, p) in perms_set.iter().enumerate() {
        if b3.map(0x400_0000 + i as u64 * 0x1000, 0x500_0000 + i as u64 * 0x1000, *p).is_err() {
            all_map = false;
        }
    }
    cs.add("三权限全挂", all_map && b3.ents.len() == 3, "合法集全收");
    let mut all_unmap = true;
    for i in 0..3u64 {
        if b3.unmap(0x400_0000 + i * 0x1000).is_err() {
            all_unmap = false;
        }
    }
    cs.add("三权限全卸", all_unmap && b3.ents.is_empty(), "往返清账");
    cs.add("权限遍历流恒等", b3.flow_ok(), "挂三卸三活零");

    // 族十：重映射权限降级链（RX→R→RW 往返）。
    let mut b4 = MapBook::default();
    b4.map(0x600_0000, 0x700_0000, 0b101).ok();
    b4.remap(0x600_0000, 0x700_0000, 0b001).ok();
    cs.add("RX 降 R", b4.lookup(0x600_0000).map(|m| m.perms) == Some(0b001), "去执行位");
    b4.remap(0x600_0000, 0x700_0000, 0b011).ok();
    cs.add("R 升 RW", b4.lookup(0x600_0000).map(|m| m.perms) == Some(0b011), "加写位");
    cs.add("降升两账", b4.n_remap == 2, "两次换档");
    cs.add("链末守恒", b4.reconcile() && b4.flow_ok(), "往返账平");

    cs
}
