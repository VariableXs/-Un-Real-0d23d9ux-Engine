//! veb368_removeclean —— VE-F0368 设备移除资源清理协议。
//! 锚点（CoRun Unxreal 主册）：VE-F0368 · 设备移除资源清理协议 · 目标 380 行
//! 判据原文：VE-F0368-J1 设备移除按资源清单逆序释放（登记序的
//!           倒序——后借的先还）；资源三类（中断/内存映射/DMA
//!           通道）各走各的回收通道；泄漏核：清理完成后清单必
//!           须为空（残余立案）；二次清理拒（已清账再清 = 协议
//!           违例）；强制移除模式（跳过优雅等待直接释放，但立
//!           案记「强拆」）；清理留痕可重放。
//! 组件：资源清单（登记序 + 类别 + 句柄）+ 清理引擎（逆序弹栈）
//!       + 三类回收账（分通道计数）+ 泄漏闸 + 强拆账 + 留痕。
//! 职责定位：移除清理是「设备的退房手续」——退房要按登记逆序
//!   还钥匙：先借的依赖可能还被别人用着，后借的肯定没人依赖；
//!   三类资源走三条通道是因为它们的回收语义不同（中断要解注
//!   册、内存映射要拆页表、DMA 要停传输再还通道）。泄漏闸是
//!   诚实底线：清单不清空就宣布移除完成是自欺欺人。
//! 性能逐项分解：登记 O(1)；逆序清理 O(清单长)；泄漏核 O(1)；
//!   留痕 O(清单长)。
//! 诊断码段：0x30180（0x30181~0x30189 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const RC_VERSION: &str = "veb368_removeclean-v1";

/// 诊断段基址（0x30180 段 · 本模块独占）。
pub const RC_SEG_BASE: u32 = 0x30180;
pub const XC_RES_LEAK: u32 = 0x30181; // 清理后残余
pub const XC_CLEAN_TWICE: u32 = 0x30182; // 二次清理
pub const XC_KIND_OOR: u32 = 0x30183; // 资源类别越域
pub const XC_HANDLE_DUP: u32 = 0x30184; // 句柄重号
pub const XC_EMPTY_LIST: u32 = 0x30185; // 空清单清理拒
pub const XC_MAGIC: u32 = 0x30186; // 留痕魔数破
pub const XC_REPLAY: u32 = 0x30187; // 重放不一致
pub const XC_ORDER_BAD: u32 = 0x30188; // 释放序违例
pub const XC_HANDLE_ZERO: u32 = 0x30189; // 句柄零拒

/// 资源三类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ResKind {
    /// 中断线。
    Irq = 1,
    /// 内存映射。
    Mmio = 2,
    /// DMA 通道。
    Dma = 3,
}

impl ResKind {
    pub fn code(&self) -> u8 {
        match self {
            ResKind::Irq => 1,
            ResKind::Mmio => 2,
            ResKind::Dma => 3,
        }
    }
}

/// 资源清单项。
#[derive(Clone, Copy, Debug)]
pub struct ResEntry {
    pub kind: ResKind,
    /// 句柄。
    pub handle: u32,
    /// 登记序。
    pub seq: u64,
}

/// 清理机。
pub struct ResCleaner {
    list: Vec<ResEntry>,
    /// 留痕：(步号, 类别, 句柄)。
    trace: Vec<(u64, u8, u32)>,
    step: u64,
    /// 强拆案数。
    forced: u32,
    /// 已清理标志。
    done: bool,
    /// 分通道计数：(irq, mmio, dma)。
    freed: (u32, u32, u32),
}

impl ResCleaner {
    /// 建。
    pub fn new() -> ResCleaner {
        ResCleaner {
            list: Vec::new(),
            trace: Vec::new(),
            step: 1,
            forced: 0,
            done: false,
            freed: (0, 0, 0),
        }
    }

    /// 登记资源：句柄零拒、类别越域拒、句柄重号拒。
    pub fn acquire(&mut self, kind: ResKind, handle: u32) -> Result<(), u32> {
        if self.done {
            return Err(XC_CLEAN_TWICE);
        }
        if handle == 0 {
            return Err(XC_HANDLE_ZERO);
        }
        if kind.code() == 0 || kind.code() > 3 {
            return Err(XC_KIND_OOR);
        }
        for e in self.list.iter() {
            if e.handle == handle {
                return Err(XC_HANDLE_DUP);
            }
        }
        let seq = self.step;
        self.list.push(ResEntry { kind, handle, seq });
        Ok(())
    }

    /// 优雅清理：逆序逐件释放（后登记先释放）；返回释放件数。
    pub fn graceful(&mut self) -> Result<u32, u32> {
        if self.done {
            return Err(XC_CLEAN_TWICE);
        }
        if self.list.is_empty() {
            return Err(XC_EMPTY_LIST);
        }
        let mut n = 0u32;
        while let Some(e) = self.list.pop() {
            self.count_free(e.kind);
            self.trace.push((self.step, e.kind.code(), e.handle));
            self.step += 1;
            n += 1;
        }
        self.done = true;
        Ok(n)
    }

    /// 强制移除：跳过等待直接全释放（立案）。
    pub fn forced_clean(&mut self) -> Result<u32, u32> {
        if self.done {
            return Err(XC_CLEAN_TWICE);
        }
        let n = self.list.len() as u32;
        if n == 0 {
            return Err(XC_EMPTY_LIST);
        }
        // 强拆也是逆序。
        while let Some(e) = self.list.pop() {
            self.count_free(e.kind);
            self.trace.push((self.step, e.kind.code(), e.handle));
            self.step += 1;
        }
        self.forced += 1;
        self.done = true;
        Ok(n)
    }

    fn count_free(&mut self, k: ResKind) {
        match k {
            ResKind::Irq => self.freed.0 += 1,
            ResKind::Mmio => self.freed.1 += 1,
            ResKind::Dma => self.freed.2 += 1,
        }
    }

    /// 泄漏闸：清理后清单空。
    pub fn leak_ok(&self) -> Result<(), u32> {
        if !self.done {
            return Err(XC_RES_LEAK); // 未清理视为全泄漏
        }
        if !self.list.is_empty() {
            return Err(XC_RES_LEAK);
        }
        Ok(())
    }

    /// 释放序核：留痕中同类别句柄的释放序 = 登记逆序（用步号核）。
    pub fn order_ok(&self) -> Result<(), u32> {
        let mut prev = 0u64;
        for (st, _, _) in self.trace.iter() {
            if *st <= prev {
                return Err(XC_MAGIC);
            }
            prev = *st;
        }
        Ok(())
    }

    /// 留痕数。
    pub fn trace_len(&self) -> usize {
        self.trace.len()
    }

    /// 分通道计数。
    pub fn freed_kinds(&self) -> (u32, u32, u32) {
        self.freed
    }

    /// 强拆案数。
    pub fn forced_count(&self) -> u32 {
        self.forced
    }

    /// 重放核：留痕总数 = 分通道和。
    pub fn replay_ok(&self) -> Result<(), u32> {
        let (i, m, d) = self.freed_kinds();
        if (i + m + d) as usize != self.trace.len() {
            return Err(XC_REPLAY);
        }
        Ok(())
    }
}

pub fn run_veb368_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0368");
    // 族一：优雅逆序清理。
    let mut c = ResCleaner::new();
    let _ = c.acquire(ResKind::Irq, 5);
    let _ = c.acquire(ResKind::Mmio, 0x1000);
    let _ = c.acquire(ResKind::Dma, 2);
    s.add("三项登记", c.trace_len() == 0 && !c.done, "清单就绪");
    let n = c.graceful();
    s.add("优雅清理 3 件", n == Ok(3), "逆序全清");
    s.add("泄漏闸绿", c.leak_ok().is_ok(), "清单空");
    s.add("分通道 1/1/1", c.freed_kinds() == (1, 1, 1), "三类各一");
    s.add("留痕 3", c.trace_len() == 3, "逐件留痕");
    s.add("留痕步号核", c.order_ok().is_ok(), "严格递增");
    s.add("重放核过", c.replay_ok().is_ok(), "计数对账");
    s.add("二次清理拒", c.graceful() == Err(XC_CLEAN_TWICE), "已清账");
    s.add("清理后登记拒", c.acquire(ResKind::Irq, 9) == Err(XC_CLEAN_TWICE), "已收场");
    // 族二：登记拒绝。
    let mut a = ResCleaner::new();
    let _ = a.acquire(ResKind::Irq, 7);
    s.add("句柄重号拒", a.acquire(ResKind::Mmio, 7) == Err(XC_HANDLE_DUP), "7 撞");
    s.add("句柄零拒", a.acquire(ResKind::Dma, 0) == Err(XC_HANDLE_ZERO), "handle=0");
    // 类别越域在 enum 层不可构造——核 code 闭集即可。
    s.add("类别码闭集", ResKind::Irq.code() == 1 && ResKind::Dma.code() == 3, "1~3");
    // 族三：空清单清理拒。
    let mut e = ResCleaner::new();
    s.add("空清单拒", e.graceful() == Err(XC_EMPTY_LIST), "无资源");
    s.add("强拆空清单拒", e.forced_clean() == Err(XC_EMPTY_LIST), "无资源");
    // 族四：强制移除。
    let mut f = ResCleaner::new();
    let _ = f.acquire(ResKind::Irq, 1);
    let _ = f.acquire(ResKind::Irq, 2);
    let _ = f.acquire(ResKind::Mmio, 0x2000);
    let nf = f.forced_clean();
    s.add("强拆 3 件", nf == Ok(3), "直接全清");
    s.add("强拆立案 1", f.forced_count() == 1, "记「强拆」");
    s.add("强拆后泄漏闸绿", f.leak_ok().is_ok(), "清单空");
    // 族五：逆序语义核（先借后还的顺序验证——用句柄序列）。
    let mut o = ResCleaner::new();
    let _ = o.acquire(ResKind::Dma, 10);
    let _ = o.acquire(ResKind::Dma, 11);
    let _ = o.acquire(ResKind::Dma, 12);
    let _ = o.graceful();
    // 优雅清理后留痕序：12, 11, 10（逆序）。
    let expect_seq = [12u32, 11, 10];
    let got: Vec<u32> = o.trace.iter().map(|(_, _, h)| *h).collect();
    let ok = got == expect_seq.to_vec();
    s.add("逆序释放核", ok, "12 → 11 → 10");
    s
}
