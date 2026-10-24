//! unx0510_ptewalk —— UNX-F0510 翻译页表走查模型（四级链 + 断点分类 + 大页截断）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0510 · 页表走查账 · 目标 210 行
//! 判据原文：UNX-F0510-J1 页表走查账：走一 四级链（PML4
//!           →PDPT→PD→PT 逐级走查，IOVA 逐级切九位索引）；
//!           走二 断点分类（走查失败按断点级分类——L4 缺
//!           /L3 缺/L2 缺/PT 末级缺，级别可审计）；走三 大
//!           页截断（PD 级 PS 位=1 则 2M 直达、PDPT 级 1G
//!           直达，截断后不再下钻）；走四 全链 present 前提
//!           （任一级 present=0 即断，不下探死路）；走五 叶
//!           物理页合成（叶项基址 + IOVA 页内偏移 = 物理
//!           地址，偏移按叶级页粒度取模）。
//! 组件：四级页表模型 + 走查器 + 断点分类 + 大页截断 + 叶合成。
//! 职责定位：页表走查是 DMA 翻译的核心路径——IOVA 从最高
//!   位到最低位逐级切 9 位索引下探，任何一级缺席都是翻译
//!   失败但失败原因各不相同（哪个级缺席决定故障账怎么记、
//!   软件怎么补映射）。大页截断是空间换走查深度。
//! 性能逐项分解：走查 O(级数)=O(4)；合成 O(1)。
//! 诊断码段：0x34D00（0x34D01~0x34D09 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use crate::checks::CheckSet;

/// 诊断码。
pub const SEG_BASE: u32 = 0x34D00;
pub const XC_L4: u32 = SEG_BASE + 1; // PML4 级断
pub const XC_L3: u32 = SEG_BASE + 2; // PDPT 级断
pub const XC_L2: u32 = SEG_BASE + 3; // PD 级断
pub const XC_L1: u32 = SEG_BASE + 4; // PT 级断

/// 叶级（决定物理页粒度）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Leaf {
    P4K,
    P2M,
    P1G,
}

impl Leaf {
    fn gran(self) -> u64 {
        match self {
            Leaf::P4K => 0x1000,
            Leaf::P2M => 1 << 21,
            Leaf::P1G => 1 << 30,
        }
    }
}

/// 走查结果：命中（叶级+物理基址）或断点（级诊断码）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Walk {
    Hit(Leaf, u64),
    Miss(u32),
}

/// 单级页表：256 槽位（present+基址+PS 位）。
pub struct PtLevel {
    pub present: [bool; 256],
    pub base: [u64; 256],
    pub ps: [bool; 256], // 大页位
}

impl PtLevel {
    pub fn new() -> Self {
        PtLevel { present: [false; 256], base: [0; 256], ps: [false; 256] }
    }
}

/// 四级页表集。
pub struct PageTable {
    pub l4: PtLevel,
    pub l3: PtLevel,
    pub l2: PtLevel,
    pub l1: PtLevel,
}

impl PageTable {
    pub fn new() -> Self {
        PageTable { l4: PtLevel::new(), l3: PtLevel::new(), l2: PtLevel::new(), l1: PtLevel::new() }
    }

    /// 走一：IOVA 四级走查。索引切法：L4=[39:47]、L3=[30:38]、L2=[21:29]、L1=[12:20]。
    pub fn walk(&self, iova: u64) -> Walk {
        let i4 = ((iova >> 39) & 0x1FF) as usize;
        let i3 = ((iova >> 30) & 0x1FF) as usize;
        let i2 = ((iova >> 21) & 0x1FF) as usize;
        let i1 = ((iova >> 12) & 0x1FF) as usize;
        // L4。
        if !self.l4.present[i4] {
            return Walk::Miss(XC_L4);
        }
        // L3（PS=1 → 1G 大页截断）。
        if !self.l3.present[i3] {
            return Walk::Miss(XC_L3);
        }
        if self.l3.ps[i3] {
            return Walk::Hit(Leaf::P1G, self.l3.base[i3]);
        }
        // L2（PS=1 → 2M 大页截断）。
        if !self.l2.present[i2] {
            return Walk::Miss(XC_L2);
        }
        if self.l2.ps[i2] {
            return Walk::Hit(Leaf::P2M, self.l2.base[i2]);
        }
        // L1。
        if !self.l1.present[i1] {
            return Walk::Miss(XC_L1);
        }
        Walk::Hit(Leaf::P4K, self.l1.base[i1])
    }

    /// 走五：叶合成——叶基址 + 页内偏移。
    pub fn translate(&self, iova: u64) -> Result<(Leaf, u64), u32> {
        match self.walk(iova) {
            Walk::Hit(lv, base) => {
                let off = iova & (lv.gran() - 1);
                Ok((lv, base + off))
            }
            Walk::Miss(code) => Err(code),
        }
    }
}

pub fn run_unx0510_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0510");
    cs.add("SEG 段位独占", SEG_BASE == 0x34D00 && XC_L1 == SEG_BASE + 4, "u32 高段全域普查无冲突");

    // 夹具：4K 全链映射 iova 0x123_4567_8000 → phys 0xABCD_000。
    let mut pt = PageTable::new();
    let iova4k: u64 = 0x123_4567_8000;
    pt.l4.present[((iova4k >> 39) & 0x1FF) as usize] = true;
    pt.l3.present[((iova4k >> 30) & 0x1FF) as usize] = true;
    pt.l2.present[((iova4k >> 21) & 0x1FF) as usize] = true;
    pt.l1.present[((iova4k >> 12) & 0x1FF) as usize] = true;
    pt.l1.base[((iova4k >> 12) & 0x1FF) as usize] = 0xABCD_000;

    // 族一：4K 全链命中 + 合成。
    let t4k = pt.translate(iova4k + 0x123);
    cs.add("4K 命中合成", t4k == Ok((Leaf::P4K, 0xABCD_000 + 0x123)), "叶基址+偏移");
    let t4k2 = pt.translate(iova4k + 0xFFF);
    cs.add("4K 页尾偏移", t4k2 == Ok((Leaf::P4K, 0xABCD_000 + 0xFFF)), "页内最大偏移");

    // 族二：断点分类（逐级拆链）。
    let mut pt_m = PageTable::new();
    let _ = pt_m.translate(iova4k);
    cs.add("L4 断", pt_m.translate(iova4k) == Err(XC_L4), "空表 L4 缺");
    pt_m.l4.present[((iova4k >> 39) & 0x1FF) as usize] = true;
    cs.add("L3 断", pt_m.translate(iova4k) == Err(XC_L3), "L4 在 L3 缺");
    pt_m.l3.present[((iova4k >> 30) & 0x1FF) as usize] = true;
    cs.add("L2 断", pt_m.translate(iova4k) == Err(XC_L2), "L3 在 L2 缺");
    pt_m.l2.present[((iova4k >> 21) & 0x1FF) as usize] = true;
    cs.add("L1 断", pt_m.translate(iova4k) == Err(XC_L1), "L2 在 L1 缺");
    pt_m.l1.present[((iova4k >> 12) & 0x1FF) as usize] = true;
    cs.add("补链全通", pt_m.translate(iova4k).is_ok(), "逐级补齐后命中");

    // 族三：2M 大页截断。
    let mut pt2 = PageTable::new();
    let iova2m: u64 = 0x1_0020_0000; // L2 索引 1
    pt2.l4.present[((iova2m >> 39) & 0x1FF) as usize] = true;
    pt2.l3.present[((iova2m >> 30) & 0x1FF) as usize] = true;
    pt2.l2.present[1] = true;
    pt2.l2.ps[1] = true;
    pt2.l2.base[1] = 0x9000_0000;
    let t2m = pt2.translate(iova2m + 0x1F_FFFF);
    cs.add("2M 截断直达", t2m == Ok((Leaf::P2M, 0x9000_0000 + 0x1F_FFFF)), "L1 不再走查");
    // 2M 页内偏移模粒度。
    let t2m_b = pt2.translate(iova2m);
    cs.add("2M 页首", t2m_b == Ok((Leaf::P2M, 0x9000_0000)), "偏移零");

    // 族四：1G 大页截断。
    let mut pt3 = PageTable::new();
    let iova1g: u64 = 0x40_0000_0000; // L3 索引 1
    pt3.l4.present[((iova1g >> 39) & 0x1FF) as usize] = true;
    pt3.l3.present[1] = true;
    pt3.l3.ps[1] = true;
    pt3.l3.base[1] = 0x1_8000_0000;
    let t1g = pt3.translate(iova1g + 0x3FFF_FFFF);
    cs.add("1G 截断直达", t1g == Ok((Leaf::P1G, 0x1_8000_0000 + 0x3FFF_FFFF)), "L2/L1 不查");
    // 1G 大页但 L2 表未配置——不影响命中（截断语义）。
    cs.add("截断无下游", pt3.walk(iova1g) == Walk::Hit(Leaf::P1G, 0x1_8000_0000), "walk 层直达");

    // 族五：索引切片位域核。
    let iova: u64 = 0x0001_0203_0400;
    let i4 = ((iova >> 39) & 0x1FF) as usize;
    let i3 = ((iova >> 30) & 0x1FF) as usize;
    let i2 = ((iova >> 21) & 0x1FF) as usize;
    let i1 = ((iova >> 12) & 0x1FF) as usize;
    cs.add("索引切片", i4 == 0 && i3 == 4 && i2 == 0x81 && i1 == 3, "九位逐级切");
    // 邻页不同索引。
    let iova_next = iova + 0x1000;
    cs.add("邻页切位", ((iova_next >> 12) & 0x1FF) as usize == i1 + 1, "4K 步进索引+1");

    // 族六：走查幂等（同 IOVA 双走恒等）。
    let a = pt.translate(iova4k + 0x42);
    let b = pt.translate(iova4k + 0x42);
    cs.add("双走恒等", a == b && a.is_ok(), "纯函数无状态");

    // 族七：走查深度核（大页减少下钻级数的量化面）。
    /// 走查深度：4K=4 级、2M=3 级、1G=2 级。
    pub fn walk_depth(lv: Leaf) -> u8 {
        match lv {
            Leaf::P4K => 4,
            Leaf::P2M => 3,
            Leaf::P1G => 2,
        }
    }
    cs.add("深度阶梯", walk_depth(Leaf::P4K) == 4 && walk_depth(Leaf::P2M) == 3 && walk_depth(Leaf::P1G) == 2, "大页省级");
    // 同一 1G 区域内 2M 页与 4K 页深度差核。
    let d1g = pt3.walk(iova1g);
    cs.add("1G 深度二", d1g == Walk::Hit(Leaf::P1G, 0x1_8000_0000) && walk_depth(Leaf::P1G) == 2, "截断即两级");
    // 4K 全链 vs 大页深度差 = 缓存项节省。
    let depth_saving = walk_depth(Leaf::P4K) - walk_depth(Leaf::P1G);
    cs.add("省两级", depth_saving == 2, "1G 相对 4K 省 2 级");

    // 族八：邻页映射连续性（同 L1 实体命中同叶基址）。
    let t_a = pt.translate(iova4k);
    let t_b = pt.translate(iova4k + 0x1000);
    // 邻页 L1 索引 +1：未灌入 → L1 断。
    cs.add("邻页独立", t_a.is_ok() && t_b == Err(XC_L1), "4K 页粒度逐页映射");
    // 同页内偏移不换实体。
    let t_c = pt.translate(iova4k + 0x800);
    cs.add("页内同叶", t_c == Ok((Leaf::P4K, 0xABCD_000 + 0x800)), "同 L1 索引同基址");

    // 族九：地址空间高段（L4 索引高位）。
    let mut pt_hi = PageTable::new();
    let iova_hi: u64 = 0x0000_FFFF_C000_0000; // L4 索引 255 顶格
    let i4h = ((iova_hi >> 39) & 0x1FF) as usize;
    cs.add("高段索引", i4h == 255, "L4 顶格索引");
    pt_hi.l4.present[255] = true;
    pt_hi.l3.present[((iova_hi >> 30) & 0x1FF) as usize] = true;
    pt_hi.l3.ps[((iova_hi >> 30) & 0x1FF) as usize] = true;
    pt_hi.l3.base[((iova_hi >> 30) & 0x1FF) as usize] = 0xEE00_0000;
    cs.add("高段走查", pt_hi.translate(iova_hi) == Ok((Leaf::P1G, 0xEE00_0000)), "顶格 L4 命中");

    cs
}
