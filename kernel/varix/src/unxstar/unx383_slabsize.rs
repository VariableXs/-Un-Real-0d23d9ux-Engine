//! unx383_slabsize —— UNX-F0383 尺寸档 slab 族（32/64/128/512）。
//! 锚点（CoRun Unxreal 主册 UNX-A1-B19）：UNX-F0383 · 尺寸档 slab 族（32/64/128/512） · 目标 260 行 · 已深化
//! 判据原文：四档 slab 分配归还正确，档位路由 100%（请求尺寸→恰档）
//! 组件：四档尺寸类（32/64/128/512 字节闭集）+ 档位路由器（请求尺寸→恰档映射，超额拒显性）
//!      + 档内自由表（归还回收 + 重用序确定性）+ slab 账（每档分配/归还/重用三计数 + 守恒）
//!      + 超档降级建议（>512B 走 bump/页路径的路由提示）
//! 判据落地：四档分配归还正确 + 档位路由 100%
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，确定性（自由表 FIFO 重用序）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const SS_VERSION: &str = "unx383_slabsize-v1";

/// 诊断段基址（0x4c90 段 · 本模块独占）。
pub const SS_SEG_BASE: u16 = 0x4c90;

/// 四档尺寸闭集（字节）。
pub const SS_CLASS_SIZES: [u64; 4] = [32, 64, 128, 512];

/// 尺寸档（四档闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SsClass {
    /// 32 字节档（小对象：句柄/游标/短键）。
    C32,
    /// 64 字节档（节点：树节点/链表元）。
    C64,
    /// 128 字节档（中对象：路径段/短描述符）。
    C128,
    /// 512 字节档（大对象：块头/扇区描述）。
    C512,
}

impl SsClass {
    /// 全量闭集。
    pub const ALL: [SsClass; 4] = [SsClass::C32, SsClass::C64, SsClass::C128, SsClass::C512];

    /// 档尺寸。
    pub const fn size(self) -> u64 {
        SS_CLASS_SIZES[self as usize]
    }

    /// 档名。
    pub const fn name(self) -> &'static str {
        match self {
            SsClass::C32 => "32B 档",
            SsClass::C64 => "64B 档",
            SsClass::C128 => "128B 档",
            SsClass::C512 => "512B 档",
        }
    }

    /// 档位路由（判据核心：请求尺寸 → 恰档；≤32→C32、≤64→C64、≤128→C128、≤512→C512、>512 拒）。
    pub const fn route(req: u64) -> Result<SsClass, SsCode> {
        if req == 0 {
            return Err(SsCode::ZeroReq);
        }
        if req <= 32 {
            return Ok(SsClass::C32);
        }
        if req <= 64 {
            return Ok(SsClass::C64);
        }
        if req <= 128 {
            return Ok(SsClass::C128);
        }
        if req <= 512 {
            return Ok(SsClass::C512);
        }
        Err(SsCode::OverClass)
    }
}

/// 诊断码（0x4c90 段十三码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SsCode {
    /// 0x4c90 零请求（零尺寸分配语义错误）。
    ZeroReq,
    /// 0x4c91 超档（>512B 走 bump/页路径，slab 族不管）。
    OverClass,
    /// 0x4c92 档池耗尽（该档自由表空且 slab 区间用尽）。
    ClassExhausted,
    /// 0x4c93 归还越权（块不属该档或重复归还）。
    ReturnBad,
    /// 0x4c94 档位路由破缺（路由结果尺寸 < 请求尺寸——100% 路由的硬保证被破坏）。
    RouteBreak,
    /// 0x4c95 slab 账破缺（三计数守恒恒等式失配）。
    LedgerBreak,
    /// 0x4c96 块对齐违例（块地址不按档尺寸对齐）。
    AlignBad,
    /// 0x4c97 双重归还（同块二次归还）。
    DoubleReturn,
    /// 0x4c98 档区间重叠（slab 区间与相邻档重叠）。
    SpanOverlap,
    /// 0x4c99 区间非法（HI ≤ LO）。
    SpanBad,
    /// 0x4c9a 重用序破缺（FIFO 序被绕过——确定性破坏）。
    ReuseOrder,
    /// 0x4c9b 路由账破缺（路由计数 ≢ 分配计数）。
    RouteLedger,
    /// 0x4c9c 段位守卫失败。
    SegmentGuard,
}

impl SsCode {
    /// 全量闭集。
    pub const ALL: [SsCode; 13] = [
        SsCode::ZeroReq,
        SsCode::OverClass,
        SsCode::ClassExhausted,
        SsCode::ReturnBad,
        SsCode::RouteBreak,
        SsCode::LedgerBreak,
        SsCode::AlignBad,
        SsCode::DoubleReturn,
        SsCode::SpanOverlap,
        SsCode::SpanBad,
        SsCode::ReuseOrder,
        SsCode::RouteLedger,
        SsCode::SegmentGuard,
    ];

    /// 码值。
    pub const fn val(self) -> u16 {
        SS_SEG_BASE + self as u16
    }

    /// 中文诊断。
    pub const fn text(self) -> &'static str {
        match self {
            SsCode::ZeroReq => "零请求：零尺寸分配按语义错误拒绝",
            SsCode::OverClass => "超档：请求超 512B，slab 族不管，路由到 bump/页路径",
            SsCode::ClassExhausted => "档池耗尽：该档自由表空且 slab 区间用尽",
            SsCode::ReturnBad => "归还越权：块不属任何档",
            SsCode::RouteBreak => "档位路由破缺：路由档尺寸小于请求尺寸",
            SsCode::LedgerBreak => "slab 账破缺：三计数守恒恒等式失配",
            SsCode::AlignBad => "块对齐违例：块地址不按档尺寸对齐",
            SsCode::DoubleReturn => "双重归还：同块二次归还",
            SsCode::SpanOverlap => "档区间重叠：slab 区间与相邻档重叠",
            SsCode::SpanBad => "区间非法：上界不大于下界",
            SsCode::ReuseOrder => "重用序破缺：FIFO 序被绕过，确定性破坏",
            SsCode::RouteLedger => "路由账破缺：路由计数与分配计数不一致",
            SsCode::SegmentGuard => "段位守卫失败：诊断码越出本模块段",
        }
    }

    /// 可操作建议。
    pub const fn advice(self) -> &'static str {
        match self {
            SsCode::ZeroReq => "调用方先判零长；占位语义请显式传 1",
            SsCode::OverClass => "走 unx382 bump 下水线或页分配器：slab 只管四档小对象",
            SsCode::ClassExhausted => "扩档区间或等归还：耗尽显性不静默降档",
            SsCode::ReturnBad => "归还前先查块属账：不属任何档的指针不应归还",
            SsCode::RouteBreak => "路由是硬保证：路由表被改，回溯 git 核 SS_CLASS_SIZES",
            SsCode::LedgerBreak => "分配/归还/重用三计数必须守恒：核对两路径记账",
            SsCode::AlignBad => "块地址必须按档尺寸对齐：slab 区间构造时先对齐",
            SsCode::DoubleReturn => "归还后置位在账：归还路径先查在用位",
            SsCode::SpanOverlap => "四档区间构造期核重叠：调用方先验",
            SsCode::SpanBad => "修正区间参数：HI > LO",
            SsCode::ReuseOrder => "自由表 FIFO 出队是确定性口径：禁止从中间取块",
            SsCode::RouteLedger => "每次分配恰一次路由记账：核对分配路径",
            SsCode::SegmentGuard => "诊断码分配越段：核对本模块段基址 0x4c90",
        }
    }
}

/// 单档 slab（区间 + 自由表 + 三计数）。
pub struct SsSlab {
    /// 档。
    pub class: SsClass,
    /// 区间下界。
    pub lo: u64,
    /// 区间上界（不含）。
    pub hi: u64,
    /// 区间内下一未切块（bump 式切分）。
    pub cut: u64,
    /// 自由表（FIFO 重用序：归还顺序入队，出队即最早归还块——确定性）。
    pub free: Vec<u64>,
    /// 在用位图（防双重归还）。
    pub live: Vec<u64>,
    /// 分配计数。
    pub allocs: u64,
    /// 归还计数。
    pub returns: u64,
    /// 重用计数（从自由表取的分配）。
    pub reuses: u64,
}

impl SsSlab {
    /// 建档（区间构造期对齐 + 对齐核）。
    pub fn new(class: SsClass, lo: u64, hi: u64) -> Result<Self, (SsCode, String)> {
        let sz = class.size();
        if hi <= lo {
            return Err((SsCode::SpanBad, SsCode::SpanBad.advice().to_string()));
        }
        if lo % sz != 0 || hi % sz != 0 {
            return Err((SsCode::AlignBad, SsCode::AlignBad.advice().to_string()));
        }
        Ok(SsSlab {
            class,
            lo,
            hi,
            cut: lo,
            free: Vec::new(),
            live: Vec::new(),
            allocs: 0,
            returns: 0,
            reuses: 0,
        })
    }

    /// 容量（块数）。
    pub const fn capacity(&self) -> u64 {
        (self.hi - self.lo) / self.class.size()
    }

    /// 块对齐核。
    const fn aligned(&self, addr: u64) -> bool {
        addr >= self.lo && addr < self.hi && (addr - self.lo) % self.class.size() == 0
    }

    /// 分配：优先自由表 FIFO 重用，否则 bump 切块。
    pub fn alloc(&mut self) -> Result<u64, SsCode> {
        // FIFO 重用（确定性口径：队列头出）。
        if !self.free.is_empty() {
            let addr = self.free.remove(0);
            self.live.push(addr);
            self.allocs += 1;
            self.reuses += 1;
            return Ok(addr);
        }
        // bump 切块。
        let sz = self.class.size();
        if self.cut + sz > self.hi {
            return Err(SsCode::ClassExhausted);
        }
        let addr = self.cut;
        self.cut += sz;
        self.live.push(addr);
        self.allocs += 1;
        Ok(addr)
    }

    /// 归还（属账核 + 双归核 + FIFO 入队）。
    pub fn ret(&mut self, addr: u64) -> Result<(), SsCode> {
        if !self.aligned(addr) {
            return Err(SsCode::ReturnBad);
        }
        if self.live.iter().any(|a| *a == addr) == false {
            return Err(SsCode::ReturnBad);
        }
        if self.free.contains(&addr) {
            return Err(SsCode::DoubleReturn);
        }
        self.live.retain(|a| *a != addr);
        self.free.push(addr);
        self.returns += 1;
        Ok(())
    }

    /// 三计数守恒（allocs ≡ 在用 + returns；reuses ≤ allocs）。
    pub fn conserved(&self) -> bool {
        self.allocs == (self.live.len() as u64) + self.returns && self.reuses <= self.allocs
    }
}

/// 四档 slab 族（路由器 + 档集 + 路由账）。
pub struct SlabFamily {
    /// 四档（按尺寸升序）。
    pub slabs: [SsSlab; 4],
    /// 路由计数（每档）。
    pub routes: [u64; 4],
    /// 诊断码。
    pub codes: Vec<SsCode>,
}

impl SlabFamily {
    /// 建族（四档区间构造期重叠核）。
    pub fn new(spans: [(u64, u64); 4]) -> Result<Self, (SsCode, String)> {
        let slabs = [
            SsSlab::new(SsClass::C32, spans[0].0, spans[0].1)?,
            SsSlab::new(SsClass::C64, spans[1].0, spans[1].1)?,
            SsSlab::new(SsClass::C128, spans[2].0, spans[2].1)?,
            SsSlab::new(SsClass::C512, spans[3].0, spans[3].1)?,
        ];
        // 重叠核。
        for i in 0..4 {
            for j in (i + 1)..4 {
                if slabs[i].lo < slabs[j].hi && slabs[j].lo < slabs[i].hi {
                    return Err((SsCode::SpanOverlap, SsCode::SpanOverlap.advice().to_string()));
                }
            }
        }
        Ok(SlabFamily { slabs, routes: [0; 4], codes: Vec::new() })
    }

    fn note(&mut self, c: SsCode) {
        if !self.codes.contains(&c) {
            self.codes.push(c);
        }
    }

    /// 分配（判据核心：档位路由 100%——路由结果必须覆盖请求）。
    pub fn alloc(&mut self, req: u64) -> Result<u64, (SsCode, String)> {
        let class = SsClass::route(req).map_err(|c| {
            self.note(c);
            (c, c.advice().to_string())
        })?;
        // 路由硬保证：档尺寸 ≥ 请求。
        if class.size() < req {
            self.note(SsCode::RouteBreak);
            return Err((SsCode::RouteBreak, SsCode::RouteBreak.advice().to_string()));
        }
        let idx = class as usize;
        let addr = self.slabs[idx].alloc().map_err(|c| {
            self.note(c);
            (c, c.advice().to_string())
        })?;
        self.routes[idx] += 1;
        Ok(addr)
    }

    /// 归还（自动定位属档——四档区间互斥所以路由唯一）。
    pub fn ret(&mut self, addr: u64) -> Result<(), (SsCode, String)> {
        for i in 0..4 {
            if self.slabs[i].lo <= addr && addr < self.slabs[i].hi {
                return self.slabs[i].ret(addr).map_err(|c| {
                    self.note(c);
                    (c, c.advice().to_string())
                });
            }
        }
        self.note(SsCode::ReturnBad);
        Err((SsCode::ReturnBad, SsCode::ReturnBad.advice().to_string()))
    }

    /// 族级守恒（四档守恒 + 路由账对账）。
    pub fn conserved(&self) -> bool {
        let total_allocs: u64 = self.slabs.iter().map(|s| s.allocs).sum();
        let total_routes: u64 = self.routes.iter().sum();
        self.slabs.iter().all(|s| s.conserved()) && total_allocs == total_routes
    }

    /// 摘要。
    pub fn digest(&self) -> String {
        let mut s = format!("slab 族：路由 {} 总分配 {} {}", self.routes.iter().sum::<u64>(), {
            let t: u64 = self.slabs.iter().map(|s| s.allocs).sum();
            t
        }, if self.conserved() { "守恒" } else { "破缺" });
        for sl in &self.slabs {
            s.push_str(&format!(
                "；{} 块容 {} 用 {} 归 {} 重 {}",
                sl.class.name(),
                sl.capacity(),
                sl.live.len(),
                sl.returns,
                sl.reuses
            ));
        }
        s
    }
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    SsCode::ALL.iter().all(|c| c.val() >= SS_SEG_BASE && c.val() < SS_SEG_BASE + 0x100)
}
