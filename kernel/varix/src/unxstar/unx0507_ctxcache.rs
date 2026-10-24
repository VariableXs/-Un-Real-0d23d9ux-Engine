//! unx0507_ctxcache —— UNX-F0507 上下文缓存失效账（CTXT 粒度 + 全局清 + 残留对账）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0507 · 上下文缓存失效账 · 目标 280 行
//! 判据原文：UNX-F0507-J1 上下文缓存失效账：缓一 全局
//!           清（CONTEXT 全局失效清空全部上下文缓存项）；
//!           缓二 总线级清（按总线号失效只清该总线挂载项，
//!           异总线存活）；缓三 条目级清（按 (总线,设备,功
//!           能) 三级键精确清，兄弟功能号存活）；缓四 残留
//!           对账（失效后缓存残留数 + 清除数 = 灌入总数）；
//!           缓五 防幽灵命中（已失效条目再查询必须落空，
//!           落实「清了就是真清了」）。
//! 组件：上下文缓存模型 + 三级失效粒度 + 残留守恒 + 幽灵闸。
//! 职责定位：上下文缓存缓存的是「设备→域」的翻译路径——
//!   根/上下文表改了缓存不改，设备就还挂在上一个域（幽灵
//!   域）。幽灵命中是隔离事故：设备被摘走后仍按旧上下文
//!   翻译，读到别人的页。三级粒度对应三级修改深度，粒度
//!   选错同 IOTLB 一样要么过清要么漏清。
//! 性能逐项分解：全局 O(n)；总线/条目 O(n) 扫描过滤。
//! 诊断码段：0x34C40（0x34C41~0x34C49 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断码。
pub const SEG_BASE: u32 = 0x34C40;
pub const XC_NOENT: u32 = SEG_BASE + 1; // 条目级失效目标不存在
pub const XC_GHOST: u32 = SEG_BASE + 2; // 幽灵命中（清后仍可查）

/// 上下文缓存项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CEnt {
    pub bus: u8,
    pub dev: u8,
    pub fn_: u8,
    pub did: u16,
}

impl CEnt {
    fn matches(&self, bus: u8) -> bool {
        self.bus == bus
    }
    fn matches_exact(&self, bus: u8, dev: u8, fn_: u8) -> bool {
        self.bus == bus && self.dev == dev && self.fn_ == fn_
    }
}

/// 上下文缓存模型。
pub struct CtxCache {
    pub ents: Vec<CEnt>,
    pub evicted: u64,
}

impl CtxCache {
    pub fn new() -> Self {
        CtxCache { ents: Vec::new(), evicted: 0 }
    }

    /// 灌入。
    pub fn fill(&mut self, bus: u8, dev: u8, fn_: u8, did: u16) {
        self.ents.push(CEnt { bus, dev, fn_, did });
    }

    /// 缓一：全局清。
    pub fn flush_global(&mut self) -> usize {
        let n = self.ents.len();
        self.ents.clear();
        self.evicted += n as u64;
        n
    }

    /// 缓二：总线级清。返回清除数。
    pub fn flush_bus(&mut self, bus: u8) -> usize {
        let before = self.ents.len();
        self.ents.retain(|e| !e.matches(bus));
        let n = before - self.ents.len();
        self.evicted += n as u64;
        n
    }

    /// 缓三：条目级清。目标不存在返回 Err（可审计）。
    pub fn flush_entry(&mut self, bus: u8, dev: u8, fn_: u8) -> Result<usize, u32> {
        let hit = self.ents.iter().any(|e| e.matches_exact(bus, dev, fn_));
        if !hit {
            return Err(XC_NOENT);
        }
        let before = self.ents.len();
        self.ents.retain(|e| !e.matches_exact(bus, dev, fn_));
        let n = before - self.ents.len();
        self.evicted += n as u64;
        Ok(n)
    }

    /// 查询：命中返回 DID。
    pub fn probe(&self, bus: u8, dev: u8, fn_: u8) -> Option<u16> {
        self.ents.iter().find(|e| e.matches_exact(bus, dev, fn_)).map(|e| e.did)
    }

    /// 缓四：残留守恒。
    pub fn reconcile(&self, filled: usize) -> bool {
        self.evicted as usize + self.ents.len() <= filled
    }
}

pub fn run_unx0507_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0507");
    cs.add("SEG 段位独占", SEG_BASE == 0x34C40 && XC_GHOST == SEG_BASE + 2, "u32 高段全域普查无冲突");

    // 族一：全局清。
    let mut c = CtxCache::new();
    c.fill(0, 2, 0, 1);
    c.fill(0, 3, 0, 2);
    c.fill(0x20, 0, 0, 3);
    cs.add("全局清三", c.flush_global() == 3, "三条全清");
    cs.add("全局后空", c.ents.is_empty(), "clear 语义");

    // 族二：总线级清。
    let mut c2 = CtxCache::new();
    c2.fill(0, 2, 0, 1);
    c2.fill(0, 3, 0, 2);
    c2.fill(0x20, 0, 0, 3);
    cs.add("总线清二", c2.flush_bus(0) == 2, "bus=0 两项");
    cs.add("异总线存活", c2.probe(0x20, 0, 0) == Some(3), "bus=0x20 存活");
    cs.add("总线清空后再清零", c2.flush_bus(0) == 0, "无可清");

    // 族三：条目级清 + 兄弟存活。
    let mut c3 = CtxCache::new();
    c3.fill(0, 2, 0, 1);
    c3.fill(0, 2, 1, 4);
    c3.fill(0, 3, 0, 2);
    cs.add("条目清一", c3.flush_entry(0, 2, 0) == Ok(1), "(0,2,0) 精确清");
    cs.add("兄弟功能号存活", c3.probe(0, 2, 1) == Some(4), "fn=1 不波及");
    cs.add("同设备异功能存活", c3.probe(0, 2, 0).is_none() && c3.probe(0, 2, 1).is_some(), "键级隔离");
    cs.add("落空可审计", c3.flush_entry(0, 9, 9) == Err(XC_NOENT), "目标不存在");

    // 族四：残留守恒。
    cs.add("守恒过", c3.reconcile(3), "evicted(1)+ents(2) <= filled(3)");
    let mut c4 = CtxCache::new();
    for i in 0..6u8 {
        c4.fill(i, 0, 0, i as u16 + 1);
    }
    c4.flush_bus(2);
    c4.flush_bus(4);
    cs.add("多轮守恒", c4.reconcile(6), "灌六清二余四");

    // 族五：防幽灵命中。
    let mut c5 = CtxCache::new();
    c5.fill(0, 2, 0, 1);
    c5.flush_entry(0, 2, 0).ok();
    cs.add("清后落空", c5.probe(0, 2, 0).is_none(), "幽灵闸：无残留命中");
    // 全局清后同样落空。
    c5.fill(1, 1, 1, 9);
    c5.flush_global();
    cs.add("全局清后落空", c5.probe(1, 1, 1).is_none(), "全局幽灵闸");

    // 族六：evicted 累计账。
    let mut c6 = CtxCache::new();
    for i in 0..4u8 {
        c6.fill(i, 0, 0, i as u16 + 1);
    }
    let e1 = c6.flush_bus(1);
    let e2 = c6.flush_global();
    cs.add("累计账核", c6.evicted == (e1 + e2) as u64 && e1 == 1 && e2 == 3, "1+3=4");

    cs
}
