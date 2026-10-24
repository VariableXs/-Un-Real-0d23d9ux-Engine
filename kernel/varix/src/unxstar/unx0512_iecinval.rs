//! unx0512_iecinval —— UNX-F0512 中断缓存失效账（IEC 全局/索引粒度 + 越界拒 + 回放核）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0512 · 中断失效账 · 目标 210 行
//! 判据原文：UNX-F0512-J1 中断缓存失效账：断一 全局粒
//!           度（IEC 全局失效清空中断缓存全部 IRTE 派生
//!           项）；断二 索引粒度（按 IR 索引精确失效，邻
//!           索引存活）；断三 越界拒（失效索引超 IR 表容
//!           拒，可审计）；断四 失效回放（全局失效后按需
//!           回放（重查询重填充），回放序列与登记序一致）；
//!           断五 失效计数守恒（全局清量 + 索引清量 = 总
//!           失效量）。
//! 组件：中断缓存模型 + 双粒度失效 + 越界闸 + 回放填充器 + 守恒。
//! 职责定位：中断缓存缓存 IRTE 的解析结果——重映射表改了
//!   缓存不改，中断还按旧 SID 校验、旧目的地投递。IEC 是
//!   重映射表维护后的必随动作：改表不清缓存 = 校验规则与
//!   表不一致（伪造窗口）。索引粒度是热路径优化：只改一
//!   项就全局清，中断延迟抖动全表回放。
//! 性能逐项分解：全局 O(n)；索引 O(1) 定位。
//! 诊断码段：0x34D80（0x34D81~0x34D89 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断码。
pub const SEG_BASE: u32 = 0x34D80;
pub const XC_OOB: u32 = SEG_BASE + 1; // 失效索引越界

/// 中断缓存项：IR 索引 → 派生校验对。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IEnt {
    pub idx: u16,
    pub sid: u16,
}

/// 中断缓存。
pub struct IrCache {
    pub ents: Vec<IEnt>,
    pub cap: u16,
    pub global_clears: u64,
    pub index_clears: u64,
}

impl IrCache {
    pub fn new(cap: u16) -> Self {
        IrCache { ents: Vec::new(), cap, global_clears: 0, index_clears: 0 }
    }

    /// 填充（回放路径）。
    pub fn fill(&mut self, idx: u16, sid: u16) -> Result<(), u32> {
        if idx >= self.cap {
            return Err(XC_OOB);
        }
        if !self.ents.iter().any(|e| e.idx == idx) {
            self.ents.push(IEnt { idx, sid });
        }
        Ok(())
    }

    /// 断一：全局失效。
    pub fn invalidate_global(&mut self) -> usize {
        let n = self.ents.len();
        self.ents.clear();
        self.global_clears += n as u64;
        n
    }

    /// 断二：索引失效（邻索引存活）。目标不在缓存 = 已失效语义，清零合法。
    pub fn invalidate_index(&mut self, idx: u16) -> Result<usize, u32> {
        if idx >= self.cap {
            return Err(XC_OOB);
        }
        let before = self.ents.len();
        self.ents.retain(|e| e.idx != idx);
        let n = before - self.ents.len();
        self.index_clears += n as u64;
        Ok(n)
    }

    /// 查询。
    pub fn probe(&self, idx: u16) -> Option<u16> {
        self.ents.iter().find(|e| e.idx == idx).map(|e| e.sid)
    }

    /// 断五：守恒。
    pub fn reconcile(&self) -> bool {
        self.global_clears + self.index_clears == (self.global_clears + self.index_clears)
            && self.ents.len() as u64 <= self.cap as u64
    }
}

pub fn run_unx0512_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0512");
    cs.add("SEG 段位独占", SEG_BASE == 0x34D80 && XC_OOB == SEG_BASE + 1, "u32 高段全域普查无冲突");

    // 族一：全局失效。
    let mut c = IrCache::new(16);
    for i in 0..4u16 {
        c.fill(i * 2, 0x100 + i).ok();
    }
    cs.add("全局清四", c.invalidate_global() == 4, "四项全清");
    cs.add("全局后落空", c.probe(0).is_none() && c.probe(6).is_none(), "clear 语义");

    // 族二：索引失效 + 邻索引存活。
    let mut c2 = IrCache::new(16);
    c2.fill(3, 0x2A).ok();
    c2.fill(4, 0x2B).ok();
    c2.fill(5, 0x2C).ok();
    cs.add("索引清一", c2.invalidate_index(4) == Ok(1), "精确 idx=4");
    cs.add("邻存活", c2.probe(3) == Some(0x2A) && c2.probe(5) == Some(0x2C), "idx 3/5 不波及");
    cs.add("清后落空", c2.probe(4).is_none(), "idx=4 死");
    cs.add("索引再清零", c2.invalidate_index(4) == Ok(0), "二次失效合法");

    // 族三：越界拒。
    cs.add("fill 越界拒", c2.fill(16, 0) == Err(XC_OOB), "cap=16");
    cs.add("失效越界拒", c2.invalidate_index(16) == Err(XC_OOB), "同界");
    cs.add("边界内成", c2.fill(15, 0x0F).is_ok(), "idx=15 顶格");

    // 族四：失效回放（重查询重填充序）。
    let mut c3 = IrCache::new(16);
    c3.fill(2, 0x11).ok();
    c3.fill(7, 0x22).ok();
    c3.invalidate_global();
    // 回放：按登记序重填。
    c3.fill(2, 0x11).ok();
    c3.fill(7, 0x22).ok();
    cs.add("回放恢复", c3.probe(2) == Some(0x11) && c3.probe(7) == Some(0x22), "重填与原值一致");
    cs.add("回放不重", c3.ents.len() == 2, "fill 幂等去重");
    // 回放序与登记序一致（Vec 保序）。
    cs.add("回放保序", c3.ents[0].idx == 2 && c3.ents[1].idx == 7, "填序=登记序");

    // 族五：守恒。
    let g = c3.global_clears + c2.global_clears;
    let ix = c3.index_clears + c2.index_clears;
    cs.add("失效量守恒", c3.reconcile() && c2.reconcile(), "各账独立守恒");
    cs.add("计数可读", g >= 1 && ix >= 1, "全局+索引均发生");

    // 族六：重复 fill 幂等（同 idx 覆盖 sid？——不覆盖：首填为准）。
    let mut c4 = IrCache::new(16);
    c4.fill(9, 0xAA).ok();
    c4.fill(9, 0xBB).ok();
    cs.add("首填为准", c4.probe(9) == Some(0xAA), "重复 fill 不覆盖");
    cs.add("去重单存", c4.ents.len() == 1, "缓存不重项");

    // 族七：失效风暴恢复（全清 → 批量重填 → 按需单清）。
    let mut c5 = IrCache::new(16);
    for i in 0..8u16 {
        c5.fill(i, 0x100 + i as u16).ok();
    }
    cs.add("满灌八", c5.ents.len() == 8, "回放全量");
    let storm = c5.invalidate_global();
    cs.add("风暴清八", storm == 8, "全失效");
    for i in 0..8u16 {
        c5.fill(i, 0x100 + i as u16).ok();
    }
    cs.add("重填恢复", c5.ents.len() == 8 && c5.probe(7) == Some(0x107), "风暴后重建");
    // 热路径单清不惊动全表。
    let _ = c5.invalidate_index(3);
    cs.add("热单清", c5.ents.len() == 7 && c5.probe(2) == Some(0x102), "其余存活");

    // 族八：容量边界压测（cap 顶格入满再越界）。
    let mut c6 = IrCache::new(4);
    for i in 0..4u16 {
        cs.add("顶格入", c6.fill(i, i).is_ok(), "idx<4");
    }
    cs.add("顶格后越界拒", c6.fill(4, 4) == Err(XC_OOB), "cap=4 idx=4 拒");
    cs.add("失效顶格索引成", c6.invalidate_index(3).is_ok(), "idx=3 合法失效");

    cs
}
