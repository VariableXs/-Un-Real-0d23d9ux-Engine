//! unx0535_hugefld —— UNX-F0535 大页分裂与合并账。
//! 锚点：主机 MMU 卷 · 2M↔4K 双向（分裂 / 合并 / 碎片账）。
//! 判据原文：分裂一次出 512 页（2M = 512×4K），全 512 齐 (全 RW 且
//!           无驻留) 才可合并回收；碎片账守恒。

/// 4K 页槽位。
#[derive(Debug, Clone, Copy)]
pub struct Sub {
    /// 页内索引 0~511。
    pub idx: u16,
    /// present 位。
    pub present: bool,
    /// R/W 位。
    pub rw: bool,
}

/// 大页区域账：要么整体 2M，要么裂成 512×4K。
#[derive(Debug)]
pub struct HugeArea {
    pub base_2m: u64,
    /// None = 整体 2M 驻留；Some = 已分裂。
    pub subs: Option<alloc::vec::Vec<Sub>>,
    /// 区域可写（合并前提之一）。
    pub writable: bool,
}

impl HugeArea {
    pub fn new(base_2m: u64, writable: bool) -> Self {
        HugeArea { base_2m, subs: None, writable }
    }

    /// 是否整体态。
    pub fn is_huge(&self) -> bool {
        self.subs.is_none()
    }

    /// 分裂：2M → 512×4K（继承 RW 位）。
    pub fn split(&mut self) -> Result<usize, ()> {
        if self.subs.is_some() {
            return Err(()); // 已分裂拒重分
        }
        let mut v = alloc::vec::Vec::with_capacity(512);
        for i in 0..512u16 {
            v.push(Sub { idx: i, present: false, rw: self.writable });
        }
        self.subs = Some(v);
        Ok(512)
    }

    /// 挂 4K 子页。
    pub fn map_sub(&mut self, idx: u16) -> Result<(), ()> {
        match self.subs.as_mut() {
            Some(v) => {
                let s = v.iter_mut().find(|s| s.idx == idx).ok_or(())?;
                if s.present {
                    return Err(());
                }
                s.present = true;
                Ok(())
            }
            None => Err(()), // 整体态不可挂子页
        }
    }

    /// 卸 4K 子页。
    pub fn unmap_sub(&mut self, idx: u16) -> Result<(), ()> {
        match self.subs.as_mut() {
            Some(v) => {
                let s = v.iter_mut().find(|s| s.idx == idx).ok_or(())?;
                if !s.present {
                    return Err(());
                }
                s.present = false;
                Ok(())
            }
            None => Err(()),
        }
    }

    /// 合并资格：已分裂 && 512 全卸载 && RW 位一致。
    pub fn merge_eligible(&self) -> bool {
        match self.subs.as_ref() {
            Some(v) => v.iter().all(|s| !s.present) && v.iter().all(|s| s.rw == self.writable),
            None => false,
        }
    }

    /// 合并：512×4K → 2M。
    pub fn merge(&mut self) -> Result<(), ()> {
        if !self.merge_eligible() {
            return Err(());
        }
        self.subs = None;
        Ok(())
    }

    /// 驻留子页计数。
    pub fn resident(&self) -> usize {
        self.subs.as_ref().map(|v| v.iter().filter(|s| s.present).count()).unwrap_or(0)
    }
}

/// 碎片账：分裂/合并的总流水。
#[derive(Debug, Default)]
pub struct FragBook {
    pub splits: u64,
    pub merges: u64,
    pub sub_maps: u64,
    pub sub_unmaps: u64,
}

impl FragBook {
    pub fn reconcile(&self, area: &HugeArea) -> bool {
        // 已分裂态：挂-卸差 = 当前驻留；整体态：驻留 0。
        let flow = self.sub_maps - self.sub_unmaps;
        flow as usize == area.resident()
    }
}

/// 判据入口：大页分裂合并全性质核。
pub fn run_unx0535_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("UNX-F0535");

    // 族一：分裂出 512 页。
    let mut a = HugeArea::new(0x20_0000, true);
    cs.add("初始整体态", a.is_huge(), "2M 驻留");
    cs.add("分裂出 512", a.split() == Ok(512), "2M = 512×4K");
    cs.add("重分拒", a.split().is_err(), "已分裂拒");
    cs.add("分裂态驻零", a.resident() == 0, "全空挂");

    // 族二：子页挂卸。
    cs.add("整体态拒挂", HugeArea::new(0x40_0000, true).map_sub(0).is_err(), "未分裂不可挂");
    a.map_sub(0).ok();
    a.map_sub(511).ok();
    cs.add("首尾挂上", a.resident() == 2, "idx 0 与 511");
    cs.add("重挂拒", a.map_sub(0).is_err(), "present 重叠拒");
    cs.add("卸载放行", a.unmap_sub(0).is_ok(), "卸 idx 0");
    cs.add("空卸拒", a.unmap_sub(0).is_err(), "已空再卸拒");

    // 族三：合并资格与执行。
    let mut book = FragBook { splits: 1, merges: 0, sub_maps: 2, sub_unmaps: 1, ..FragBook::default() };
    cs.add("驻留拒合并", a.merge().is_err(), "仍有 idx 511 驻留");
    a.unmap_sub(511).ok();
    book.sub_unmaps += 1;
    cs.add("全空够格", a.merge_eligible(), "512 全卸");
    cs.add("合并回整体", a.merge().is_ok(), "512→2M");
    cs.add("合并后驻零", a.is_huge() && a.resident() == 0, "整体态");
    cs.add("碎片账守恒", book.reconcile(&a), "挂卸差 = 驻留（整体态=0）");

    // 族四：RW 继承。
    let mut ro = HugeArea::new(0x60_0000, false);
    ro.split().ok();
    cs.add("RO 继承", ro.subs.as_ref().map(|v| v.iter().all(|s| !s.rw)).unwrap_or(false), "512 页全 RO");
    let mut rw = HugeArea::new(0x80_0000, true);
    rw.split().ok();
    cs.add("RW 继承", rw.subs.as_ref().map(|v| v.iter().all(|s| s.rw)).unwrap_or(false), "512 页全 RW");

    // 族五：合并守恒的驻留路径（分裂→挂→守恒核）。
    let mut b = HugeArea::new(0xA0_0000, true);
    let mut book2 = FragBook::default();
    b.split().ok();
    book2.splits += 1;
    for i in 0..10u16 {
        b.map_sub(i).ok();
        book2.sub_maps += 1;
    }
    cs.add("驻十", b.resident() == 10, "idx 0~9");
    cs.add("守恒十", book2.reconcile(&b), "挂十卸零差十");
    for i in 0..4u16 {
        b.unmap_sub(i).ok();
        book2.sub_unmaps += 1;
    }
    cs.add("卸四剩六", b.resident() == 6, "idx 4~9 驻留");
    cs.add("守恒六", book2.reconcile(&b), "10-4=6");
    cs.add("驻留拒合并", b.merge().is_err(), "六页在挂");

    // 族六：分裂-合并-再分裂往返。
    let mut c = HugeArea::new(0xC0_0000, true);
    c.split().ok();
    c.merge().ok();
    cs.add("再分裂可", c.split().is_ok(), "合并后恢复分裂能力");
    cs.add("往返驻零", c.resident() == 0, "往返干净");

    // 族七：RO 区合并资格（RW 一致性）。
    let mut ro2 = HugeArea::new(0xE0_0000, false);
    ro2.split().ok();
    cs.add("RO 全空够格", ro2.merge_eligible(), "512 全卸且 RW 一致");
    cs.add("RO 合并通", ro2.merge().is_ok(), "RO 区可合并");

    // 族八：大规模挂卸压力（512 全挂全卸）。
    let mut d = HugeArea::new(0x100_0000, true);
    d.split().ok();
    let mut n_map = 0u64;
    for i in 0..512u16 {
        if d.map_sub(i).is_ok() {
            n_map += 1;
        }
    }
    cs.add("512 全挂", n_map == 512 && d.resident() == 512, "满驻留");
    let mut n_unmap = 0u64;
    for i in 0..512u16 {
        if d.unmap_sub(i).is_ok() {
            n_unmap += 1;
        }
    }
    cs.add("512 全卸", n_unmap == 512, "清空");
    cs.add("压力后合并", d.merge().is_ok(), "全卸即够格");

    cs
}
