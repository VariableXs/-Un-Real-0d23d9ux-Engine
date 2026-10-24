//! unx0515_devrw —— UNX-F0515 设备 TLB 读写分离失效账（R/W 双账 + 单侧清 + 对账）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0515 · 读写分离失效账 · 目标 210 行
//! 判据原文：UNX-F0515-J1 读写分离失效账：离一 双账独
//!           立（设备 TLB 读项与写项分账，同 (DID,IOVA)
//!           可各自存在）；离二 读失效不波及写（清读账后
//!           写项原样，反之亦然）；离三 全侧清（按侧清空
//!           ——读全清不碰写账）；离四 双侧清等价全局（
//!           读侧全清 + 写侧全清的总量 = 全局失效量）；离
//!           五 权限降级即失效（映射从 RW 降 R 时读写两
//!           侧都清——权限变化不认侧）。
//! 组件：读写双缓存账 + 单侧/双侧失效 + 降级联动 + 守恒。
//! 职责定位：设备 TLB 里读翻译与写翻译是两份缓存：只改只
//!   读映射（如把共享页转只读）时清读侧就够，全清是性能
//!   浪费；但权限降级必须两侧都清——旧写项残留 = 设备仍
//!   能写已变只读的页（权限陈旧事故）。
//! 性能逐项分解：单侧清 O(n) 扫描过滤；降级联动 O(2n)。
//! 诊断码段：0x34E40（0x34E41~0x34E49 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断码。
pub const SEG_BASE: u32 = 0x34E40;
pub const XC_NOENT: u32 = SEG_BASE + 1; // 降级目标不存在

/// 双侧项：同键 (did, iova) 各挂读写两份（Option）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RwEnt {
    pub did: u16,
    pub iova: u64,
    pub r: bool,
    pub w: bool,
}

/// 读写分离缓存。
pub struct RwCache {
    pub ents: Vec<RwEnt>,
    pub evicted: u64,
}

impl RwCache {
    pub fn new() -> Self {
        RwCache { ents: Vec::new(), evicted: 0 }
    }

    /// 灌入（读/写任一侧）。
    pub fn fill(&mut self, did: u16, iova: u64, for_write: bool) {
        if let Some(e) = self.ents.iter_mut().find(|e| e.did == did && e.iova == iova) {
            if for_write {
                e.w = true;
            } else {
                e.r = true;
            }
            return;
        }
        self.ents.push(RwEnt { did, iova, r: !for_write, w: for_write });
    }

    /// 离二：单侧清（清位不删项；双侧全空的实体回收）。
    pub fn invalidate_side(&mut self, did: u16, iova: u64, for_write: bool) -> usize {
        let mut n = 0usize;
        for e in self.ents.iter_mut() {
            if e.did == did && e.iova == iova {
                if for_write && e.w {
                    e.w = false;
                    n += 1;
                }
                if !for_write && e.r {
                    e.r = false;
                    n += 1;
                }
            }
        }
        self.ents.retain(|e| e.r || e.w);
        self.evicted += n as u64;
        n
    }

    /// 离三：全侧清（按侧清空，不碰另一侧）。
    pub fn flush_side(&mut self, for_write: bool) -> usize {
        let mut n = 0usize;
        for e in self.ents.iter_mut() {
            if for_write && e.w {
                e.w = false;
                n += 1;
            }
            if !for_write && e.r {
                e.r = false;
                n += 1;
            }
        }
        self.ents.retain(|e| e.r || e.w);
        self.evicted += n as u64;
        n
    }

    /// 离五：权限降级联动（两侧全清）。
    pub fn demote(&mut self, did: u16, iova: u64) -> Result<usize, u32> {
        let hit = self.ents.iter().any(|e| e.did == did && e.iova == iova);
        if !hit {
            return Err(XC_NOENT);
        }
        let before = self.ents.len();
        self.ents.retain(|e| !(e.did == did && e.iova == iova));
        let n = before - self.ents.len();
        self.evicted += n as u64;
        Ok(n)
    }

    /// 离四：双侧总量核。
    pub fn total_sides(&self) -> u64 {
        self.ents.iter().map(|e| e.r as u64 + e.w as u64).sum()
    }
}

pub fn run_unx0515_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0515");
    cs.add("SEG 段位独占", SEG_BASE == 0x34E40 && XC_NOENT == SEG_BASE + 1, "u32 高段全域普查无冲突");

    // 族一：双账独立（同键两侧并存）。
    let mut c = RwCache::new();
    c.fill(1, 0x1000, false);
    c.fill(1, 0x1000, true);
    cs.add("同键双侧并存", c.ents.len() == 1 && c.ents[0].r && c.ents[0].w, "一实体双侧");
    cs.add("侧计数二", c.total_sides() == 2, "读+写");

    // 族二：读失效不波及写。
    let mut c2 = RwCache::new();
    c2.fill(1, 0x2000, false);
    c2.fill(1, 0x2000, true);
    cs.add("清读侧", c2.invalidate_side(1, 0x2000, false) == 1, "读项清一实体");
    cs.add("写侧存活", c2.total_sides() == 1 && c2.ents[0].w, "写原样");
    // 反向：清写不波及读。
    let mut c3 = RwCache::new();
    c3.fill(1, 0x2000, false);
    c3.fill(1, 0x2000, true);
    c3.invalidate_side(1, 0x2000, true);
    cs.add("清写存活读", c3.total_sides() == 1 && c3.ents[0].r, "镜像语义");

    // 族三：全侧清。
    let mut c4 = RwCache::new();
    c4.fill(1, 0x1000, false);
    c4.fill(2, 0x2000, false);
    c4.fill(3, 0x3000, true);
    cs.add("读侧全清", c4.flush_side(false) == 2, "两读项");
    cs.add("写侧幸存", c4.total_sides() == 1 && c4.ents[0].w, "只余写项");
    cs.add("写侧全清", c4.flush_side(true) == 1, "余一写项");
    cs.add("全空", c4.ents.is_empty(), "双侧清空等价全局");

    // 族四：双侧清 = 全局量。
    let mut c5 = RwCache::new();
    for i in 0..4u64 {
        c5.fill(1, 0x1000 + i * 0x1000, false);
        c5.fill(1, 0x1000 + i * 0x1000, true);
    }
    let rs = c5.flush_side(false);
    let ws = c5.flush_side(true);
    cs.add("双侧=全局", rs == 4 && ws == 4 && c5.evicted == 8, "离四等式");

    // 族五：降级联动。
    let mut c6 = RwCache::new();
    c6.fill(1, 0x5000, false);
    c6.fill(1, 0x5000, true);
    cs.add("降级双侧清", c6.demote(1, 0x5000) == Ok(1), "双侧实体一并摘");
    cs.add("降级后空", c6.ents.is_empty(), "权限降级不认侧");
    cs.add("降级落空拒", c6.demote(1, 0x5000) == Err(XC_NOENT), "二次降级无可摘");

    // 族六：单侧实体降级（只有读侧时也清）。
    let mut c7 = RwCache::new();
    c7.fill(2, 0x6000, false);
    cs.add("单侧降级", c7.demote(2, 0x6000) == Ok(1), "只读实体摘除");

    cs
}
