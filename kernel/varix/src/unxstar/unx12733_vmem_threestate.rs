//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12733 · 内存族 Virtual* 对照回归（三态六弧） · 0x82140段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 矢量功能代码 ≥330 行；
//!   ② 判据原文：UNX-F12734-J1 disposition 五档对照回归绿，
//!      IOSB 交付语义维持；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - Virtual* 页面三态状态机（释放→保留→提交）六弧冻结表：
//!     保留/提交/卸交/还空(保留态)/还空(提交态)/改护——六弧逐条对照回归，
//!     越表弧（跳弧 Free→Commit、同态重弧）一律显性拒；
//!   - disposition 五档闭集（成功/挂起/重试/否决/故障）：每次 Virtual*
//!     操作恰结算一次，档位守恒可对账，五档全出现即对照回归绿；
//!   - IOSB 交付语义维持：状态+信息二元组一写一读、未读先写即拒、
//!     读空即拒、信息与档位自洽（成功=受影响页数，余档=0）；
//!   - 注入三族（跳弧/IOSB重写/五档重结算）各 10 轮必抓全抓；
//!   - LCG 种子写死 = SEG_BASE（0x82140）；no_std + alloc；
//!     零 std/零浮点/零 IO/零墙钟/零随机/零 unwrap/零裸下标。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（VX9 公式：0x80000 + 0x40 × (12733−12600) = 0x82140）。
pub const SEG_BASE: u32 = 0x82140;

// ── 冻结常量表 ─────────────────────────────────────────────────────
/// 场页数（虚拟内存对照场容量；冻结值）。
pub const ARENA_PAGES: usize = 12;
/// 页粒度（winnt.h PAGE granularity 0x1000，冻结）。
pub const PAGE_GRAN: u32 = 0x1000;
/// 六弧表弧数（三态状态机对照弧全集）。
pub const VM_ARC_COUNT: usize = 6;
/// 五档闭集档数（disposition 对照面）。
pub const DISP_TIERS: usize = 5;
/// MEM_COMMIT（winnt.h 0x1000，冻结）。
pub const MEM_COMMIT: u32 = 0x1000;
/// MEM_RESERVE（winnt.h 0x2000，冻结）。
pub const MEM_RESERVE: u32 = 0x2000;
/// MEM_DECOMMIT（winnt.h 0x4000，冻结）。
pub const MEM_DECOMMIT: u32 = 0x4000;
/// MEM_RELEASE（winnt.h 0x8000，冻结）。
pub const MEM_RELEASE: u32 = 0x8000;
/// PAGE_NOACCESS（winnt.h 0x01，改护闭集下界）。
pub const PROT_NONE: u32 = 0x01;
/// PAGE_READWRITE（winnt.h 0x04，改护闭集上界）。
pub const PROT_RW: u32 = 0x04;

/// 五档冻结状态值（NTSTATUS 惯例三段：成功/挂起 informational、
/// 重试 warning、否决/故障 error；对照语义用，值冻结）。
pub const DISP_STATUS: [u32; DISP_TIERS] = [
    0x0000_0000, // 成功 STATUS_SUCCESS
    0x0000_0103, // 挂起 STATUS_PENDING
    0x8000_0011, // 重试（警告段对照值）
    0xC000_0022, // 否决 STATUS_ACCESS_DENIED 对照值
    0xC000_0005, // 故障 STATUS_ACCESS_VIOLATION 对照值
];

// ── 诊断码闭集（段内 0x40 窗口，10 码） ────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VmDiag {
    /// 越表弧（跳弧 Free→Commit、同态重弧等）
    VmArcOffTable,
    /// 页号越场界
    VmPageIdxFault,
    /// IOSB 未读先写（交付语义破坏）
    VmIosbDoubleWrite,
    /// IOSB 读空（无交付可读）
    VmIosbNotDelivered,
    /// 五档外档位（档位闭集破坏）
    VmDispClosedOff,
    /// 场满（空闲页耗尽，容量耗尽面）
    VmArenaFull,
    /// 改护值越闭集（PROT_NONE~PROT_RW 之外）
    VmProtInvalid,
    /// 对账不守恒（Σ页账≠场账）
    VmLedgerBreak,
    /// 五档重结算（无在途工单或重复结算）
    VmSettleReplay,
    /// 粒度违约（尺寸非页对齐）
    VmGranBreak,
}

impl VmDiag {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            VmDiag::VmArcOffTable => "越表弧：按六弧表迁移",
            VmDiag::VmPageIdxFault => "页号越场界：0..12 之内重试",
            VmDiag::VmIosbDoubleWrite => "IOSB未读先写：先读后写",
            VmDiag::VmIosbNotDelivered => "IOSB读空：先操作再读",
            VmDiag::VmDispClosedOff => "五档外档位：档位闭集破坏",
            VmDiag::VmArenaFull => "场满：先还空再保留",
            VmDiag::VmProtInvalid => "改护值越闭集：0x01~0x04 之内",
            VmDiag::VmLedgerBreak => "对账不守恒：Σ页账≠场账",
            VmDiag::VmSettleReplay => "五档重结算：一单恰结算一次",
            VmDiag::VmGranBreak => "粒度违约：尺寸须页对齐",
        }
    }
    pub const ALL: [VmDiag; 10] = [
        VmDiag::VmArcOffTable,
        VmDiag::VmPageIdxFault,
        VmDiag::VmIosbDoubleWrite,
        VmDiag::VmIosbNotDelivered,
        VmDiag::VmDispClosedOff,
        VmDiag::VmArenaFull,
        VmDiag::VmProtInvalid,
        VmDiag::VmLedgerBreak,
        VmDiag::VmSettleReplay,
        VmDiag::VmGranBreak,
    ];
}

/// 诊断码查表（闭集返回；未知码给中文兜底）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < VmDiag::ALL.len() {
        if let Some(c) = VmDiag::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x82140 段闭集内"
}

// ── 三态状态机 ─────────────────────────────────────────────────────
/// 页三态：释放 / 保留 / 提交（Virtual* 生命周期对照面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PageState {
    /// 释放：未占用
    Free,
    /// 保留：地址段已订，未给物理页
    Reserved,
    /// 提交：物理页已给
    Committed,
}

impl PageState {
    /// 三态中文名（对照回归文案面）。
    pub fn name(self) -> &'static str {
        match self {
            PageState::Free => "释放",
            PageState::Reserved => "保留",
            PageState::Committed => "提交",
        }
    }
}

/// 六弧冻结表：(弧名, 起点, 终点, Virtual* 语义)。
/// 出处：VirtualAlloc/VirtualFree/VirtualProtect 页态迁移规格——
/// 三态间合法迁移恰此六条（含改护自环弧），越表即违约。
pub const VM_ARCS: [(&'static str, PageState, PageState, &'static str); VM_ARC_COUNT] = [
    ("弧一·保留", PageState::Free, PageState::Reserved, "VirtualAlloc MEM_RESERVE"),
    ("弧二·提交", PageState::Reserved, PageState::Committed, "VirtualAlloc MEM_COMMIT"),
    ("弧三·卸交", PageState::Committed, PageState::Reserved, "VirtualFree MEM_DECOMMIT"),
    ("弧四·还空保留", PageState::Reserved, PageState::Free, "VirtualFree MEM_RELEASE"),
    ("弧五·还空提交", PageState::Committed, PageState::Free, "VirtualFree MEM_RELEASE"),
    ("弧六·改护", PageState::Committed, PageState::Committed, "VirtualProtect 自环"),
];

/// disposition 五档闭集（判据锚点：五档对照回归绿）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Disp {
    /// 成功：合法弧走通
    Settled,
    /// 挂起：场忙，工单在途待续
    Pending,
    /// 重试：重试窗内暂缓一次
    Retry,
    /// 否决：越表弧/状态拒
    Denied,
    /// 故障：页号越界/参数错
    Faulted,
}

impl Disp {
    /// 档位序号（守恒账下标）。
    pub fn tier(self) -> usize {
        match self {
            Disp::Settled => 0,
            Disp::Pending => 1,
            Disp::Retry => 2,
            Disp::Denied => 3,
            Disp::Faulted => 4,
        }
    }
    /// 档位中文名（对照文案面）。
    pub fn name(self) -> &'static str {
        match self {
            Disp::Settled => "成功",
            Disp::Pending => "挂起",
            Disp::Retry => "重试",
            Disp::Denied => "否决",
            Disp::Faulted => "故障",
        }
    }
    /// 冻结状态值（IOSB status 语义自洽）。
    pub fn status(self) -> u32 {
        match DISP_STATUS.get(self.tier()) {
            Some(v) => *v,
            None => 0xFFFF_FFFF,
        }
    }
}

/// IOSB：状态+信息二元组（一写一读、读后清、信息与档位自洽）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Iosb {
    /// 冻结状态值（DISP_STATUS 对应档位）
    pub status: u32,
    /// 信息：成功=受影响页数，余档=0
    pub information: u32,
}

/// 场页：三态 + 保护 + 账。
#[derive(Clone, Copy)]
pub struct VmPage {
    /// 当前态
    pub state: PageState,
    /// 当前保护（PROT 闭集）
    pub prot: u32,
    /// 累计走弧数（页账）
    pub touches: u32,
    /// 累计成功结算数
    pub settles: u32,
}

impl VmPage {
    /// 冻结初值（释放态、无保护、零账）。
    pub const fn frozen() -> VmPage {
        VmPage { state: PageState::Free, prot: 0, touches: 0, settles: 0 }
    }
}

/// 虚拟内存对照场：12 页 × 三态六弧 × 五档结算 × IOSB 交付。
#[derive(Clone, Copy)]
pub struct VmArena {
    pub pages: [VmPage; ARENA_PAGES],
    /// 场账（只记成功走弧）：Σ页账必须与此守恒
    pub ops: u32,
    /// 总账（全档记账）：Σ五档账必须与此守恒
    pub total: u32,
    /// 场忙窗（挂起档触发面）
    pub busy: bool,
    /// 重试窗（重试档触发面，单发自清）
    pub retry_window: bool,
    /// 在途工单（挂起页号；None=无在途）
    pub inflight: Option<usize>,
    /// IOSB 待读交付（None=已读清）
    pub iosb: Option<Iosb>,
    /// 五档守恒账
    pub disp_counts: [u32; DISP_TIERS],
}

impl VmArena {
    pub fn new() -> VmArena {
        VmArena {
            pages: [VmPage::frozen(); ARENA_PAGES],
            ops: 0,
            total: 0,
            busy: false,
            retry_window: false,
            inflight: None,
            iosb: None,
            disp_counts: [0u32; DISP_TIERS],
        }
    }

    /// 档位记账（五档账 +1 且总账 +1；每操作恰调一次）。
    fn record(&mut self, d: Disp) {
        let t = d.tier();
        if let Some(c) = self.disp_counts.get_mut(t) {
            *c = c.saturating_add(1);
        }
        self.total = self.total.saturating_add(1);
    }

    /// IOSB 交付：未读先写即拒（语义一）。
    fn deliver(&mut self, d: Disp, info: u32) -> Result<(), VmDiag> {
        if self.iosb.is_some() {
            return Err(VmDiag::VmIosbDoubleWrite);
        }
        self.iosb = Some(Iosb { status: d.status(), information: info });
        Ok(())
    }

    /// IOSB 取读：读空即拒（语义二）；读后清（语义三）。
    pub fn read_iosb(&mut self) -> Result<Iosb, VmDiag> {
        match self.iosb {
            Some(b) => {
                self.iosb = None;
                Ok(b)
            }
            None => Err(VmDiag::VmIosbNotDelivered),
        }
    }

    /// 页号界检（越界记故障档）。
    fn page_ok(&self, idx: usize) -> Result<(), VmDiag> {
        if idx >= ARENA_PAGES {
            return Err(VmDiag::VmPageIdxFault);
        }
        Ok(())
    }

    /// 六弧迁移核：查弧表→验起点（越表/跳弧即否决档）→迁移+记账。
    /// 场账 ops 只记成功走弧；总账 total 全档都记（record 内）。
    fn walk(&mut self, arc: usize, idx: usize) -> Result<Disp, VmDiag> {
        let entry = match VM_ARCS.get(arc) {
            Some(e) => *e,
            None => return Err(VmDiag::VmArcOffTable),
        };
        let (_name, from, to, _sem) = entry;
        let cur = match self.pages.get(idx) {
            Some(p) => p.state,
            None => return Err(VmDiag::VmPageIdxFault),
        };
        if cur != from {
            // 越表弧：起点不符（跳弧/同态重弧/倒退）→否决档，不改态。
            self.record(Disp::Denied);
            self.deliver(Disp::Denied, 0)?;
            return Ok(Disp::Denied);
        }
        if let Some(p) = self.pages.get_mut(idx) {
            p.state = to;
            p.touches = p.touches.saturating_add(1);
            if to != PageState::Free {
                p.settles = p.settles.saturating_add(1);
            }
        }
        self.ops = self.ops.saturating_add(1);
        self.record(Disp::Settled);
        self.deliver(Disp::Settled, 1)?;
        Ok(Disp::Settled)
    }

    /// 弧一·保留：释放态页 + 页对齐尺寸 → 保留。
    pub fn reserve(&mut self, idx: usize, size: u32) -> Result<Disp, VmDiag> {
        self.page_ok(idx)?;
        if size == 0 || size % PAGE_GRAN != 0 {
            self.record(Disp::Faulted);
            self.deliver(Disp::Faulted, 0)?;
            return Err(VmDiag::VmGranBreak);
        }
        if self.busy {
            self.inflight = Some(idx);
            self.record(Disp::Pending);
            self.deliver(Disp::Pending, 0)?;
            return Ok(Disp::Pending);
        }
        if self.retry_window {
            self.retry_window = false;
            self.record(Disp::Retry);
            self.deliver(Disp::Retry, 0)?;
            return Ok(Disp::Retry);
        }
        self.walk(0, idx)
    }

    /// 弧二·提交：保留态页 → 提交（保护默认可写）。
    pub fn commit(&mut self, idx: usize) -> Result<Disp, VmDiag> {
        self.page_ok(idx)?;
        if self.busy {
            self.inflight = Some(idx);
            self.record(Disp::Pending);
            self.deliver(Disp::Pending, 0)?;
            return Ok(Disp::Pending);
        }
        let d = self.walk(1, idx)?;
        if d == Disp::Settled {
            if let Some(p) = self.pages.get_mut(idx) {
                p.prot = PROT_RW;
            }
        }
        Ok(d)
    }

    /// 弧三·卸交：提交态页 → 保留（保保护值）。
    pub fn decommit(&mut self, idx: usize) -> Result<Disp, VmDiag> {
        self.page_ok(idx)?;
        self.walk(2, idx)
    }

    /// 弧四/弧五·还空：保留/提交态页 → 释放（按当前态自动选弧）。
    /// 释放态还空无弧可走 → 否决档（不改态、不记场账）。
    pub fn release(&mut self, idx: usize) -> Result<Disp, VmDiag> {
        self.page_ok(idx)?;
        let cur = match self.pages.get(idx) {
            Some(p) => p.state,
            None => return Err(VmDiag::VmPageIdxFault),
        };
        let arc = match cur {
            PageState::Reserved => 3usize,
            PageState::Committed => 4usize,
            PageState::Free => {
                self.record(Disp::Denied);
                self.deliver(Disp::Denied, 0)?;
                return Ok(Disp::Denied);
            }
        };
        let d = self.walk(arc, idx)?;
        if d == Disp::Settled {
            if let Some(p) = self.pages.get_mut(idx) {
                p.prot = 0;
            }
        }
        Ok(d)
    }

    /// 弧六·改护：提交态页自环改护（保护闭集 0x01~0x04）。
    pub fn protect(&mut self, idx: usize, prot: u32) -> Result<Disp, VmDiag> {
        self.page_ok(idx)?;
        if prot < PROT_NONE || prot > PROT_RW {
            self.record(Disp::Faulted);
            self.deliver(Disp::Faulted, 0)?;
            return Err(VmDiag::VmProtInvalid);
        }
        let d = self.walk(5, idx)?;
        if d == Disp::Settled {
            if let Some(p) = self.pages.get_mut(idx) {
                p.prot = prot;
            }
        }
        Ok(d)
    }

    /// 续在途工单：无在途即重结算拒；有在途则按其态选弧完成。
    pub fn resume(&mut self) -> Result<Disp, VmDiag> {
        let idx = match self.inflight {
            Some(i) => i,
            None => return Err(VmDiag::VmSettleReplay),
        };
        self.inflight = None;
        self.busy = false;
        let cur = match self.pages.get(idx) {
            Some(p) => p.state,
            None => return Err(VmDiag::VmPageIdxFault),
        };
        let arc = match cur {
            PageState::Free => 0usize,
            PageState::Reserved => 1usize,
            PageState::Committed => 5usize,
        };
        self.walk(arc, idx)
    }

    /// 全场还空（对账前清场用）；每次还空交付读回，避免未读先写断流。
    pub fn release_all(&mut self) -> u32 {
        let mut freed = 0u32;
        let mut i = 0usize;
        while i < ARENA_PAGES {
            let st = match self.pages.get(i) {
                Some(p) => p.state,
                None => PageState::Free,
            };
            match st {
                PageState::Free => {
                    freed += 1;
                }
                _ => {
                    if self.release(i) == Ok(Disp::Settled) {
                        let _ = self.read_iosb();
                        freed += 1;
                    }
                }
            }
            i += 1;
        }
        freed
    }

    /// 对账守恒：Σ页账 ≡ 场账（只记成功走弧）。
    pub fn conservation(&self) -> Result<(), VmDiag> {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < ARENA_PAGES {
            if let Some(p) = self.pages.get(i) {
                sum = sum.saturating_add(p.touches);
            }
            i += 1;
        }
        if sum != self.ops {
            return Err(VmDiag::VmLedgerBreak);
        }
        Ok(())
    }

    /// 五档守恒：Σ五档账 ≡ 总账（每操作恰记一档）。
    pub fn disp_conservation(&self) -> Result<(), VmDiag> {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < DISP_TIERS {
            if let Some(c) = self.disp_counts.get(i) {
                sum = sum.saturating_add(*c);
            }
            i += 1;
        }
        if sum != self.total {
            return Err(VmDiag::VmLedgerBreak);
        }
        Ok(())
    }

    /// 五档全出现判定（对照回归绿核心）。
    pub fn five_tiers_seen(&self) -> bool {
        let mut i = 0usize;
        while i < DISP_TIERS {
            match self.disp_counts.get(i) {
                Some(&c) if c > 0 => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 找一空页（场满即拒，容量耗尽面）。
    pub fn find_free(&self) -> Result<usize, VmDiag> {
        let mut i = 0usize;
        while i < ARENA_PAGES {
            if let Some(p) = self.pages.get(i) {
                if p.state == PageState::Free {
                    return Ok(i);
                }
            }
            i += 1;
        }
        Err(VmDiag::VmArenaFull)
    }
}

// ── 确定性 LCG（种子 = SEG_BASE） ─────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

// ── 黄金正演：六弧全走 + 五档全现 + IOSB 一写一读 ──────────────────
/// 黄金正演返回断言通过数（满绿 = 12）。
pub fn golden_run() -> Result<u32, VmDiag> {
    let mut a = VmArena::new();
    // 弧一：页0 释放→保留（页对齐尺寸）。
    if a.reserve(0, PAGE_GRAN) != Ok(Disp::Settled) {
        return Err(VmDiag::VmArcOffTable);
    }
    let _ = a.read_iosb()?;
    // 弧二：页0 保留→提交。
    if a.commit(0) != Ok(Disp::Settled) {
        return Err(VmDiag::VmArcOffTable);
    }
    let _ = a.read_iosb()?;
    // 弧六：页0 提交态改护（自环）。
    if a.protect(0, PROT_NONE) != Ok(Disp::Settled) {
        return Err(VmDiag::VmArcOffTable);
    }
    let _ = a.read_iosb()?;
    // 弧三：页0 提交→保留（卸交）。
    if a.decommit(0) != Ok(Disp::Settled) {
        return Err(VmDiag::VmArcOffTable);
    }
    let _ = a.read_iosb()?;
    // 弧四：页0 保留→释放。
    if a.release(0) != Ok(Disp::Settled) {
        return Err(VmDiag::VmArcOffTable);
    }
    let _ = a.read_iosb()?;
    // 弧五：页1 保留→提交→释放（还空提交态）。
    if a.reserve(1, PAGE_GRAN) != Ok(Disp::Settled) {
        return Err(VmDiag::VmArcOffTable);
    }
    let _ = a.read_iosb()?;
    if a.commit(1) != Ok(Disp::Settled) {
        return Err(VmDiag::VmArcOffTable);
    }
    let _ = a.read_iosb()?;
    if a.release(1) != Ok(Disp::Settled) {
        return Err(VmDiag::VmArcOffTable);
    }
    let _ = a.read_iosb()?;
    // 挂起档：场忙窗 → 挂起 → 续单成功。
    a.busy = true;
    if a.reserve(2, PAGE_GRAN) != Ok(Disp::Pending) {
        return Err(VmDiag::VmDispClosedOff);
    }
    let _ = a.read_iosb()?;
    if a.resume() != Ok(Disp::Settled) {
        return Err(VmDiag::VmSettleReplay);
    }
    let _ = a.read_iosb()?;
    // 重试档：重试窗单发 → 重试一次 → 重发成功。
    a.retry_window = true;
    if a.reserve(3, PAGE_GRAN) != Ok(Disp::Retry) {
        return Err(VmDiag::VmDispClosedOff);
    }
    let _ = a.read_iosb()?;
    if a.reserve(3, PAGE_GRAN) != Ok(Disp::Settled) {
        return Err(VmDiag::VmArcOffTable);
    }
    let _ = a.read_iosb()?;
    // 否决档：跳弧（释放态页直接提交），状态值自洽读回。
    if a.commit(0) != Ok(Disp::Denied) {
        return Err(VmDiag::VmDispClosedOff);
    }
    let denied_status = a.read_iosb()?.status;
    if denied_status != Disp::Denied.status() {
        return Err(VmDiag::VmDispClosedOff);
    }
    // 故障档：改护越闭集（页号在场内但参数违约），状态值自洽读回。
    if a.protect(0, 0x40).is_ok() {
        return Err(VmDiag::VmProtInvalid);
    }
    let faulted_status = a.read_iosb()?.status;
    if faulted_status != Disp::Faulted.status() {
        return Err(VmDiag::VmDispClosedOff);
    }
    // 页号越场界必抓（界检先于记账，不产生档位）。
    if a.commit(ARENA_PAGES + 1).is_ok() {
        return Err(VmDiag::VmPageIdxFault);
    }
    // 容量耗尽：清场→全保留→find_free 拒。
    let freed_all = a.release_all();
    if freed_all != ARENA_PAGES as u32 {
        return Err(VmDiag::VmArenaFull);
    }
    let mut i = 0usize;
    while i < ARENA_PAGES {
        if a.reserve(i, PAGE_GRAN) != Ok(Disp::Settled) {
            return Err(VmDiag::VmArenaFull);
        }
        let _ = a.read_iosb()?;
        i += 1;
    }
    if a.find_free().is_ok() {
        return Err(VmDiag::VmArenaFull);
    }
    // 对账双守恒 + 五档全现。
    a.conservation()?;
    a.disp_conservation()?;
    if !a.five_tiers_seen() {
        return Err(VmDiag::VmDispClosedOff);
    }
    let _ = a.release_all();
    Ok(run_asserts())
}

/// 对照断言组（12 条，全过 = 12）。
pub fn run_asserts() -> u32 {
    let mut pass = 0u32;
    // A1：六弧表弧数冻结为 6。
    if VM_ARCS.len() == VM_ARC_COUNT {
        pass += 1;
    }
    // A2：六弧语义字段全非空 + MEM_* 四旗冻结自洽（文案闭集前置）。
    let mut names_ok = true;
    let mut i = 0usize;
    while i < VM_ARCS.len() {
        if let Some((n, _f, _t, sem)) = VM_ARCS.get(i) {
            if n.is_empty() || sem.is_empty() {
                names_ok = false;
            }
        }
        i += 1;
    }
    let mem_ok = MEM_RESERVE != MEM_COMMIT
        && MEM_DECOMMIT != MEM_RELEASE
        && (MEM_COMMIT & MEM_RESERVE) == 0
        && PAGE_GRAN == 0x1000;
    if names_ok && mem_ok {
        pass += 1;
    }
    // A3：五档状态值全非零映射（挂起/重试/否决/故障 ≠ 成功 0）。
    let mut status_ok = Disp::Settled.status() == 0;
    let mut k = 1usize;
    while k < DISP_TIERS {
        if let Some(s) = DISP_STATUS.get(k) {
            if *s == 0 {
                status_ok = false;
            }
        }
        k += 1;
    }
    if status_ok {
        pass += 1;
    }
    // A4：跳弧必抓（释放态直接提交）。
    let mut a1 = VmArena::new();
    if a1.commit(0) == Ok(Disp::Denied) {
        pass += 1;
    }
    // A5：IOSB 读空必抓。
    let mut a2 = VmArena::new();
    if a2.read_iosb().is_err() {
        pass += 1;
    }
    // A6：IOSB 未读先写必抓（连续两操作不读）。
    let mut a3 = VmArena::new();
    let _ = a3.reserve(0, PAGE_GRAN);
    if a3.reserve(1, PAGE_GRAN).is_err() {
        pass += 1;
    }
    // A7：改护越闭集必抓。
    let mut a4 = VmArena::new();
    let _ = a4.reserve(0, PAGE_GRAN);
    let _ = a4.read_iosb();
    let _ = a4.commit(0);
    let _ = a4.read_iosb();
    if a4.protect(0, 0x40).is_err() {
        pass += 1;
    }
    // A8：五档重结算必抓（无在途续单）。
    let mut a5 = VmArena::new();
    if a5.resume().is_err() {
        pass += 1;
    }
    // A9：粒度违约必抓（非页对齐尺寸）。
    let mut a6 = VmArena::new();
    if a6.reserve(0, 0x1234).is_err() {
        pass += 1;
    }
    // A10：容量耗尽必抓。
    let mut a7 = VmArena::new();
    let mut full = true;
    let mut j = 0usize;
    while j < ARENA_PAGES {
        if a7.reserve(j, PAGE_GRAN).is_err() {
            full = false;
        }
        let _ = a7.read_iosb();
        j += 1;
    }
    if full && a7.find_free().is_err() {
        pass += 1;
    }
    // A11：页号越界必抓。
    if VmArena::new().commit(ARENA_PAGES).is_err() {
        pass += 1;
    }
    // A12：对账双守恒（页账/档账/总账）+ 页结算账非零。
    let mut a8 = VmArena::new();
    let _ = a8.reserve(0, PAGE_GRAN);
    let _ = a8.read_iosb();
    let _ = a8.commit(0);
    let _ = a8.read_iosb();
    let settles_ok = match a8.pages.get(0) {
        Some(p) => p.settles == 2 && p.state == PageState::Committed,
        None => false,
    };
    if a8.conservation().is_ok() && a8.disp_conservation().is_ok() && settles_ok {
        pass += 1;
    }
    pass
}

// ── 注入面：三族 × 10 轮全抓 ──────────────────────────────────────
/// 注入账本：三族各自 10 轮尝试/抓获。
#[derive(Clone, Copy)]
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    /// 全抓判定：三族抓获数 ≡ 尝试数。
    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < 3 {
            let t = match self.tries.get(i) {
                Some(v) => *v,
                None => 0,
            };
            let c = match self.caught.get(i) {
                Some(v) => *v,
                None => 0,
            };
            if c != t {
                return false;
            }
            i += 1;
        }
        true
    }
    /// 轮数守恒：每族恰 10 轮。
    pub fn ten_rounds_each(&self) -> bool {
        let mut i = 0usize;
        while i < 3 {
            match self.tries.get(i) {
                Some(&10) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }
}

/// 注入三族 × 10 轮：族0=跳弧（释放态直提/同态重提）、族1=IOSB未读先写、
/// 族2=五档重结算（无在途续单）。LCG 变参（种子=SEG_BASE），违约必抓。
pub fn run_injections(ledger: &mut InjectLedger) {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut round = 0u32;
    while round < 10 {
        // 族0：跳弧注入——对释放态页直接提交（弧表外迁移必抓）。
        if let Some(t0) = ledger.tries.get_mut(0) {
            *t0 += 1;
        }
        let mut a = VmArena::new();
        let page = rng.below(ARENA_PAGES as u64) as usize;
        let caught0 = a.commit(page) == Ok(Disp::Denied);
        if caught0 {
            let _ = a.read_iosb();
            if let Some(c0) = ledger.caught.get_mut(0) {
                *c0 += 1;
            }
        }
        // 族1：IOSB 未读先写注入——连发两操作不读交付必抓。
        if let Some(t1) = ledger.tries.get_mut(1) {
            *t1 += 1;
        }
        let mut b = VmArena::new();
        let p1 = rng.below(ARENA_PAGES as u64) as usize;
        let _ = b.reserve(p1, PAGE_GRAN);
        if b.reserve((p1 + 1) % ARENA_PAGES, PAGE_GRAN).is_err() {
            if let Some(c1) = ledger.caught.get_mut(1) {
                *c1 += 1;
            }
        }
        // 族2：五档重结算注入——无在途工单续单必抓。
        if let Some(t2) = ledger.tries.get_mut(2) {
            *t2 += 1;
        }
        let mut c = VmArena::new();
        if c.resume().is_err() {
            if let Some(c2) = ledger.caught.get_mut(2) {
                *c2 += 1;
            }
        }
        round += 1;
    }
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool） ──────────────────────
/// 六弧对照回归绿：黄金正演走通 + 弧表冻结。
fn six_arc_ok() -> bool {
    match golden_run() {
        Ok(n) => n == 12 && VM_ARCS.len() == VM_ARC_COUNT,
        Err(_) => false,
    }
}

/// disposition 五档对照回归绿：五档全现 + 双账守恒。
fn disp_five_ok() -> bool {
    match golden_run() {
        Ok(12) => true,
        _ => false,
    }
}

/// IOSB 交付语义维持：一写一读、读后清、状态与档位自洽。
fn iosb_ok() -> bool {
    let mut a = VmArena::new();
    if a.read_iosb().is_ok() {
        return false;
    }
    if a.reserve(0, PAGE_GRAN) != Ok(Disp::Settled) {
        return false;
    }
    let b = match a.read_iosb() {
        Ok(b) => b,
        Err(_) => return false,
    };
    if b.status != Disp::Settled.status() || b.information != 1 {
        return false;
    }
    // 读后清：再读必空。
    a.read_iosb().is_err()
}

/// 判定次序：保留先于提交、提交先于改护，乱序必否决。
fn order_ok() -> bool {
    let mut a = VmArena::new();
    // 未保留先改护 → 否决。
    if a.protect(0, PROT_NONE) != Ok(Disp::Denied) {
        return false;
    }
    let _ = a.read_iosb();
    // 未提交先卸交 → 否决。
    if a.decommit(0) != Ok(Disp::Denied) {
        return false;
    }
    let _ = a.read_iosb();
    // 正序：保留→提交→改护 全绿。
    a.reserve(0, PAGE_GRAN) == Ok(Disp::Settled)
        && {
            let _ = a.read_iosb();
            true
        }
        && a.commit(0) == Ok(Disp::Settled)
        && {
            let _ = a.read_iosb();
            true
        }
        && a.protect(0, PROT_NONE) == Ok(Disp::Settled)
}

/// 容量耗尽：12 页全保留后 find_free 必拒，逐页还空后可复用。
fn capacity_ok() -> bool {
    let mut a = VmArena::new();
    let mut i = 0usize;
    while i < ARENA_PAGES {
        if a.reserve(i, PAGE_GRAN) != Ok(Disp::Settled) {
            return false;
        }
        let _ = a.read_iosb();
        i += 1;
    }
    if a.find_free().is_ok() {
        return false;
    }
    a.release_all() == ARENA_PAGES as u32 && a.find_free().is_ok()
}

/// 双跑一致：黄金正演跑两遍结果逐位一致。
fn double_run_ok() -> bool {
    golden_run() == golden_run()
}

/// 对账一致/计数守恒：页账+档账+注入账三守恒。
fn conservation_ok() -> bool {
    let mut a = VmArena::new();
    let mut i = 0usize;
    while i < 4 {
        if a.reserve(i, PAGE_GRAN) != Ok(Disp::Settled) {
            return false;
        }
        let _ = a.read_iosb();
        if a.commit(i) != Ok(Disp::Settled) {
            return false;
        }
        let _ = a.read_iosb();
        i += 1;
    }
    if a.conservation().is_err() || a.disp_conservation().is_err() {
        return false;
    }
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    led.all_caught() && led.ten_rounds_each()
}

/// 文案闭集：10 码全查得非空中文，未知码给兜底。
fn text_closed_ok() -> bool {
    let mut i = 0usize;
    while i < VmDiag::ALL.len() {
        if let Some(c) = VmDiag::ALL.get(i) {
            let d = detail(c.val());
            if d.is_empty() || d == "未知诊断码：不在 0x82140 段闭集内" {
                return false;
            }
        }
        i += 1;
    }
    detail(SEG_BASE + 0x3F) == "未知诊断码：不在 0x82140 段闭集内"
}

/// 段位守卫：SEG_BASE 对齐 0x40 且落在 VX9 区间，诊断码全在段窗内。
fn segment_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x80000 || SEG_BASE > 0x82FC0 {
        return false;
    }
    let mut i = 0usize;
    while i < VmDiag::ALL.len() {
        if let Some(c) = VmDiag::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 收口只增不减：断言组重跑不劣化。
fn closing_ok() -> bool {
    let a = run_asserts();
    let b = run_asserts();
    b >= a && a == 12
}

// ── 检查名/静态明细闭集（match 查表，无裸下标） ───────────────────
/// 第 i 条检查的静态名（闭集 match 查表，越表给兜底）。
fn check_name(i: usize) -> &'static str {
    match i {
        0 => "J1-六弧对照回归绿",
        1 => "J1-disposition五档全现",
        2 => "J1-IOSB交付语义维持",
        3 => "J1-跳弧倒退显性拒",
        4 => "J1-判定次序先行",
        5 => "J1-容量耗尽必抓",
        6 => "J1-注入族一·跳弧10轮全抓",
        7 => "J1-注入族二·IOSB重写全抓",
        8 => "J1-注入族三·重结算全抓",
        9 => "J1-双跑一致",
        10 => "J1-对账一致·三账守恒",
        11 => "J1-文案闭集",
        12 => "J1-段位守卫0x82140",
        13 => "J1-收口只增不减",
        _ => "越表检查项",
    }
}

/// 第 i 条检查的静态明细兜底（第 9 条起走 s.add 静态明细）。
fn check_static(i: usize) -> &'static str {
    match i {
        0 => "三态六弧全走通·断言12/12",
        1 => "成功/挂起/重试/否决/故障全现",
        2 => "一写一读·读后清·状态自洽",
        3 => "越表弧一律否决不改态",
        4 => "保留→提交→改护乱序必拒",
        5 => "12页满拒·还空复用守恒",
        6 => "跳弧注入10/10",
        7 => "未读先写注入10/10",
        8 => "重结算注入10/10",
        9 => "两次黄金正演逐位一致",
        10 => "页账+档账+注入账三守恒",
        11 => "10码查表非空·未知码兜底",
        12 => "0x40对齐·VX9区间·段窗内",
        13 => "断言组重跑不劣化",
        _ => "明细缺表",
    }
}

/// extra_checks：14 条检查（正常流/边界/注入/判定次序/双跑/守恒/守卫/收口）。
pub fn extra_checks() -> Vec<(String, bool, String)> {
    let mut v: Vec<(String, bool, String)> = Vec::new();
    // 1：六弧对照回归绿（判据锚点主谓词）。
    let golden = golden_run();
    v.push((
        String::from(check_name(0)),
        six_arc_ok(),
        format!("asserts={:?}", golden),
    ));
    // 2：disposition 五档对照回归绿（判据锚点档位面）。
    let mut probe = VmArena::new();
    let _ = probe.reserve(0, PAGE_GRAN);
    let _ = probe.read_iosb();
    v.push((
        String::from(check_name(1)),
        disp_five_ok(),
        format!("five={:?}", probe.five_tiers_seen()),
    ));
    // 3：IOSB 交付语义维持（判据锚点交付面）。
    v.push((
        String::from(check_name(2)),
        iosb_ok(),
        String::from("一写一读·读后清·状态自洽"),
    ));
    // 4：跳弧倒退显性拒。
    v.push((String::from(check_name(3)), backward_reject_ok(), String::from(check_static(3))));
    // 5：判定次序先行。
    v.push((String::from(check_name(4)), order_ok(), String::from(check_static(4))));
    // 6：容量耗尽必抓。
    v.push((String::from(check_name(5)), capacity_ok(), String::from(check_static(5))));
    // 7~9：注入三族各 10 轮全抓。
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    let mut f = 0usize;
    while f < 3 {
        let t = match led.tries.get(f) {
            Some(x) => *x,
            None => 0,
        };
        let c = match led.caught.get(f) {
            Some(x) => *x,
            None => 0,
        };
        v.push((
            String::from(check_name(6 + f)),
            c == t && t == 10,
            format!("c/t={}/{}", c, t),
        ));
        f += 1;
    }
    // 10：双跑一致。
    v.push((String::from(check_name(9)), double_run_ok(), String::from(check_static(9))));
    // 11：对账一致/三账守恒。
    v.push((
        String::from(check_name(10)),
        conservation_ok(),
        String::from("页账+档账+注入账三守恒"),
    ));
    // 12：文案闭集。
    v.push((String::from(check_name(11)), text_closed_ok(), String::from(check_static(11))));
    // 13：段位守卫。
    v.push((String::from(check_name(12)), segment_guard_ok(), String::from(check_static(12))));
    // 14：收口只增不减。
    v.push((String::from(check_name(13)), closing_ok(), String::from(check_static(13))));
    v
}

/// 跳弧倒退显性拒谓词（第 4 条检查的判定核）。
fn backward_reject_ok() -> bool {
    let mut a = VmArena::new();
    // 释放态直接提交（跳弧）→ 否决。
    if a.commit(0) != Ok(Disp::Denied) {
        return false;
    }
    let _ = a.read_iosb();
    // 释放态直接卸交（倒退向弧）→ 否决。
    if a.decommit(0) != Ok(Disp::Denied) {
        return false;
    }
    let _ = a.read_iosb();
    // 提交后重复提交（同态重弧，弧二非自环）→ 否决。
    let _ = a.reserve(0, PAGE_GRAN);
    let _ = a.read_iosb();
    let _ = a.commit(0);
    let _ = a.read_iosb();
    let again = a.commit(0);
    let _ = a.read_iosb();
    again == Ok(Disp::Denied)
}

// ── 检查入口 ───────────────────────────────────────────────────────
/// VX9 检查入口：extra_checks 逐条进 set（动态槽只给最关键 8 条）。
pub fn run_unx12733_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12733 内存族 Virtual* 对照回归（三态六弧）");
    let items = extra_checks();
    let mut i = 0usize;
    while i < items.len() {
        if let Some((_name_dyn, passed, detail_dyn)) = items.get(i) {
            let name = check_name(i);
            if i < 8 {
                // 动态明细槽（≤8）：判据锚点与注入族优先。
                s.add_dyn(name, *passed, detail_dyn);
            } else {
                // 静态明细：不占动态槽，红项照记不丢。
                s.add(name, *passed, check_static(i));
            }
        }
        i += 1;
    }
    s
}

// ── 测试节（≥8 条 #[test]） ───────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn six_arcs_full_walk_green() {
        // 正常流：三态六弧按合法序全走通，黄金正演 12/12。
        assert_eq!(golden_run(), Ok(12));
        assert!(six_arc_ok());
    }

    #[test]
    fn five_dispositions_all_seen() {
        // 判据锚点：五档（成功/挂起/重试/否决/故障）全出现且守恒。
        assert!(disp_five_ok());
        let mut a = VmArena::new();
        let _ = a.reserve(0, PAGE_GRAN);
        let _ = a.read_iosb();
        let _ = a.commit(0);
        let _ = a.read_iosb();
        assert!(a.disp_conservation().is_ok());
    }

    #[test]
    fn iosb_delivery_semantics_held() {
        // 判据锚点：一写一读、读后清、状态与档位自洽、信息=受影响页数。
        assert!(iosb_ok());
        let mut a = VmArena::new();
        let _ = a.reserve(0, PAGE_GRAN);
        let b = a.read_iosb().unwrap_or(Iosb { status: 9, information: 9 });
        assert_eq!(b.status, Disp::Settled.status());
        assert_eq!(b.information, 1);
    }

    #[test]
    fn skip_arc_explicitly_rejected() {
        // 注入：跳弧（释放直提/释放卸交/同态重提）必否决且不改态。
        assert!(backward_reject_ok());
        let mut a = VmArena::new();
        let _ = a.commit(0);
        assert!(a.read_iosb().is_ok());
    }

    #[test]
    fn op_order_enforced() {
        // 判序：未保留先改护/未提交先卸交必否决；正序全绿。
        assert!(order_ok());
    }

    #[test]
    fn arena_capacity_exhaustion() {
        // 边界：12 页全保留后场满必拒；逐页还空后可复用。
        assert!(capacity_ok());
        let mut a = VmArena::new();
        let mut i = 0usize;
        while i < ARENA_PAGES {
            assert_eq!(a.reserve(i, PAGE_GRAN), Ok(Disp::Settled));
            let _ = a.read_iosb();
            i += 1;
        }
        assert!(a.find_free().is_err());
    }

    #[test]
    fn granularity_and_prot_closed() {
        // 边界：非页对齐尺寸与改护越闭集必抓；故障交付读回后改护放行。
        let mut a = VmArena::new();
        assert!(a.reserve(0, 0x1234).is_err());
        let mut b = VmArena::new();
        let _ = b.reserve(0, PAGE_GRAN);
        let _ = b.read_iosb();
        let _ = b.commit(0);
        let _ = b.read_iosb();
        assert!(b.protect(0, 0x40).is_err());
        let _ = b.read_iosb();
        assert!(b.protect(0, PROT_NONE).is_ok());
    }

    #[test]
    fn injections_three_families_all_caught() {
        // 注入：跳弧/IOSB重写/重结算三族各 10 轮全抓。
        let mut led = InjectLedger::new();
        run_injections(&mut led);
        assert!(led.all_caught());
        assert!(led.ten_rounds_each());
    }

    #[test]
    fn double_run_identical() {
        // 双跑一致：两次黄金正演逐位一致。
        assert!(double_run_ok());
        assert_eq!(extra_checks().len(), 14);
    }

    #[test]
    fn conservation_triple_ledger() {
        // 守恒：页账+档账+注入账三账全守恒。
        assert!(conservation_ok());
        let mut a = VmArena::new();
        let _ = a.reserve(0, PAGE_GRAN);
        let _ = a.read_iosb();
        assert!(a.conservation().is_ok());
    }

    #[test]
    fn segment_and_text_closed() {
        // 段位守卫 + 文案闭集 + 未知码兜底。
        assert!(segment_guard_ok());
        assert!(text_closed_ok());
        assert_eq!(SEG_BASE, 0x82140);
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(detail(0xFFFFFFFF), "未知诊断码：不在 0x82140 段闭集内");
    }

    #[test]
    fn checks_all_green_in_set() {
        // 全绿：检查入口 14 条全过、无截断、动态槽不超 8。
        let s = run_unx12733_checks();
        assert_eq!(s.len(), 14);
        assert!(s.all_passed());
        assert!(!s.truncated());
        assert!(s.dyn_used() <= 8);
    }
}
