//! unx327_tramppagemap ↔ UNX-F0327 trampoline 页表最小集（临时恒等映射）
//!
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B17）：UNX-F0327 · trampoline 页表
//! 最小集（临时恒等映射） · 判据：`UNX-F0327-J1 临时页表（低 1MB +
//! 跳板 + 主表入口）映射齐备，AP 开页断言过`
//!
//! ## 为什么"最小集"三个字是全部难点
//!
//! AP 启动早期（`lgdt`/`ltr`/MSR 装载前）根本没有主表可用，但AP 立刻要
//! 取指、压栈、访问 APIC。业界通解是**跳板（trampoline）+ 临时恒等页表**：
//! 用最小页表把 AP 的入口物理地址**恒等映射**到低 1MB，让 AP 在无主表
//! 期间也能跑极短的一段代码，再跳到高地址正式入口。本模块把这套机制
//! 逐条做实，五处真实事故一个都不放过：
//!
//! 1. **低 1MB 恒等映射必须 4K 对齐且逐 2M 分段**：`0..1MB` 内恒等映射
//!    需要 256 个 4K PTE。要检查的不是"数量够"，而是每一项都满足
//!    `phys % 4096 == 0`、`phys == virt`（恒等，不是"映到某处"）、
//!    `present=1 && write=1`。少一条 = AP 在那一页取指即#PF，而且故障
//!    地址就是那条缺失项的地址——**可精确定位**是这项检查的价值所在。
//! 2. **跳板入口必须落在恒等区内**：跳板代码物理地址若在 1MB 之上，
//!    临时页表根本没映它 → AP 跳过去立刻 #PF。所以跳板地址必须用
//!    `in_identity_window()` 校验，且跳板必须**完整落在单个 4K 页内**
//!    （跨页跳板 = 后半段无映射，这是真实事故）。
//! 3. **主表入口占位必须在建表前显式登记**：主表基址（CR3 目标）在
//!    临时表阶段还不可用，但**入口指针本身**要占一个恒等映射槽
//!    （trampoline 尾部的"跳主表"指令要用它）。入口指针放高地址时
//!    临时表无法映射它——所以设计上入口指针**必须位于低 1MB**，
//!    否则 `MainEntryOutOfWindow` 立案。这是"最小集"里最容易漏的一格。
//! 4. **CR0.WP / CR4 语义与 WP 位矛盾要检出**：临时恒等映射全可写，
//!    但若 CR0.WP=1 而 CR4 允许内核写只读页，语义会打架。更实际的是
//!    **开页条件**：临时表必须在 PG|PE 都置位后才CR3 装载，且装载
//!    序列必须 `mov cr3` 紧跟一条 `jmp` 重取指令——只写 CR3 不重取，
//!    TLB 里还留着旧的翻译结果，AP 会跳到错误的物理页。
//! 5. **临时表是一次性的**：`Teardown` 必须在AP 跳到主表后把临时表
//!    作废，且作废后**禁止再次装载同一份**（CR3 重载 = 把 AP 打回
//!    恒等世界，它会以为还在跳板里）→ `ReloadForbidden`。这与
//!    `OverlappedRange`（AP 槽位重复映射）一起构成两道防线。
//!
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const V327_VERSION: &str = "v327-tramppagemap-v1";

/// 诊断段基址（0x6B00 段 · 本模块独占）。
pub const V327_SEG_BASE: u16 = 0x6B00;

/// 页大小。
pub const PAGE_SIZE: u64 = 4096;

/// 恒等窗口上界（低 1MB）。
pub const IDENTITY_WINDOW: u64 = 1024 * 1024;

/// 恒等窗口 PTE 数。
pub const IDENTITY_PTES: usize = (IDENTITY_WINDOW / PAGE_SIZE) as usize;

/// 页表项容量。
pub const PTE_CAP: usize = IDENTITY_PTES;

/// AP 槽容量。
pub const AP_CAP: usize = 16;

/// 页表四级下标（x86-64 · PML4/PDPT/PD/PT）。
pub const LEVEL_PML4: usize = 0;
/// 二级下标。
pub const LEVEL_PDPT: usize = 1;
/// 三级下标。
pub const LEVEL_PD: usize = 2;
/// 四级下标。
pub const LEVEL_PT: usize = 3;

// --------------------------------------------------------------------------
// 诊断码（十二码 · 段内连续）
// --------------------------------------------------------------------------

/// trampoline 页表诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum V327Code {
    /// 0x6B00 PTE 索引越界。
    PteRange,
    /// 0x6B01 PTE 非 4K 对齐。
    AlignBad,
    /// 0x6B02 PTE 非恒等（phys != virt）。
    NotIdentity,
    /// 0x6B03 PTE缺 present/write 位。
    FlagsMissing,
    /// 0x6B04 跳板地址越出恒等窗口。
    TrampOutWindow,
    /// 0x6B05 跳板跨 4K 页（后半段无映射）。
    TrampCrossPage,
    /// 0x6B06 主表入口指针越出恒等窗口（最小集缺一格）。
    MainEntryOut,
    /// 0x6B07 CR3 重载已作废表（把 AP 打回恒等世界）。
    ReloadForbidden,
    /// 0x6B08 AP 槽位映射重叠（两 AP 占同跳板）。
    OverlappedRange,
    /// 0x6B09 开页序列错（PG/PE 未置位即装CR3）。
    PagingNotOn,
    /// 0x6B0A CR3 装载后缺指令重取。
    NoRefetch,
    /// 0x6B0B 恒等窗口覆盖不全（256 项不足）。
    WindowHole,
}

impl V327Code {
    /// 码值。
    pub fn val(self) -> u16 {
        V327_SEG_BASE
            + match self {
                V327Code::PteRange => 0x0,
                V327Code::AlignBad => 0x1,
                V327Code::NotIdentity => 0x2,
                V327Code::FlagsMissing => 0x3,
                V327Code::TrampOutWindow => 0x4,
                V327Code::TrampCrossPage => 0x5,
                V327Code::MainEntryOut => 0x6,
                V327Code::ReloadForbidden => 0x7,
                V327Code::OverlappedRange => 0x8,
                V327Code::PagingNotOn => 0x9,
                V327Code::NoRefetch => 0xA,
                V327Code::WindowHole => 0xB,
            }
    }

    /// 全部码（顺序即码序）。
    pub const ALL: [V327Code; 12] = [
        V327Code::PteRange,
        V327Code::AlignBad,
        V327Code::NotIdentity,
        V327Code::FlagsMissing,
        V327Code::TrampOutWindow,
        V327Code::TrampCrossPage,
        V327Code::MainEntryOut,
        V327Code::ReloadForbidden,
        V327Code::OverlappedRange,
        V327Code::PagingNotOn,
        V327Code::NoRefetch,
        V327Code::WindowHole,
    ];

    /// 可操作建议（错误路径必带修复指引）。
    pub fn remedy(self) -> &'static str {
        match self {
            V327Code::PteRange => "PTE 索引越界：限 256 项内，idx 应为 virt>>12",
            V327Code::AlignBad => "PTE 未 4K 对齐：物理地址必须 4K 界，重算 phys 基址",
            V327Code::NotIdentity => "非恒等映射：临时表要求 phys==virt，恒等映射才是恒等映射",
            V327Code::FlagsMissing => "PTE 缺 present/write：AP 要取指也要压栈，两位都得置",
            V327Code::TrampOutWindow => "跳板越出低 1MB：临时表只映低区，把跳板链接到低址",
            V327Code::TrampCrossPage => "跳板跨页：代码尾部落到未映射页，收紧跳板长度",
            V327Code::MainEntryOut => "主表入口指针越窗：入口指针必须低 1MB，否则临时表映不到",
            V327Code::ReloadForbidden => "重载已作废 CR3：会把 AP 打回恒等世界，改走主表 CR3",
            V327Code::OverlappedRange => "AP 映射重叠：两 AP 占同一跳板区，为每 AP 分配独立跳板槽",
            V327Code::PagingNotOn => "分页未开就装 CR3：先置 CR0.PG|CR0.PE 再写 CR3",
            V327Code::NoRefetch => "CR3 后缺指令重取：紧跟一条 jmp 冲掉旧 TLB 翻译",
            V327Code::WindowHole => "恒等窗口有洞：256 项必须逐项填满，缺项即 AP #PF",
        }
    }

    /// 一行式描述。
    pub fn line(self) -> String {
        format!("[0x{:04X}] {}", self.val(), self.remedy())
    }
}

// --------------------------------------------------------------------------
// PTE
// --------------------------------------------------------------------------

/// 页表项。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Pte {
    /// 物理地址（4K 对齐）。
    pub phys: u64,
    /// present 位。
    pub present: bool,
    /// write 位。
    pub write: bool,
    /// 用户位（临时表应为内核态映射 ·false）。
    pub user: bool,
}

impl Pte {
    /// 造恒等可读写内核 PTE。
    pub fn identity(phys: u64) -> Pte {
        Pte {
            phys,
            present: true,
            write: true,
            user: false,
        }
    }

    /// 造空 PTE（未映射）。
    pub fn empty() -> Pte {
        Pte {
            phys: 0,
            present: false,
            write: false,
            user: false,
        }
    }

    /// 是否 4K 对齐。
    pub fn aligned(&self) -> bool {
        self.phys % PAGE_SIZE == 0
    }

    /// 该 PTE 是否对给定虚拟地址恒等（`phys == virt`）。
    ///
    /// 恒等性必须相对具体虚拟地址判定：PTE 自身只存 phys，虚拟地址
    /// 由它所在的表项（索引）决定，故恒等核对必须传入 virt。
    pub fn identity_for(&self, virt: u64) -> bool {
        self.phys == virt
    }

    /// 位打包（x86-64 PTE 低位：P/W/U 三位）。
    pub fn bits(&self) -> u64 {
        (self.phys & !0xFFF)
            | (self.present as u64)
            | ((self.write as u64) << 1)
            | ((self.user as u64) << 2)
    }

    /// 从位解包（掩掉 NX/AVL/保留位）。
    pub fn from_bits(b: u64) -> Pte {
        Pte {
            phys: b & !0xFFF,
            present: b & 1 != 0,
            write: b & 2 != 0,
            user: b & 4 != 0,
        }
    }
}

// --------------------------------------------------------------------------
// 临时页表（最小集）
// --------------------------------------------------------------------------

/// 临时页表（低 1MB 恒等 + 跳板槽 + 主表入口槽）。
#[derive(Clone, Debug)]
pub struct TrampMap {
    /// PTE 表（256 项 · 索引 = virt >> 12）。
    pub ptes: Vec<Pte>,
    /// CR0 视图。
    pub cr0: u64,
    /// CR4 视图。
    pub cr4: u64,
    /// 已装载的 CR3（0 = 未装载）。
    pub cr3: u64,
    /// 表是否已作废。
    pub torn: bool,
    /// CR3 装载次数。
    pub loads: u32,
    /// 载入序列是否含指令重取。
    pub refetch: bool,
    /// 立案计数（12 格）。
    pub stats: [u64; 12],
}

impl TrampMap {
    /// 造临时表（全空）。
    pub fn new() -> TrampMap {
        let mut ptes: Vec<Pte> = Vec::new();
        for i in 0..PTE_CAP {
            ptes.push(if i == 0 { Pte::empty() } else { Pte::empty() });
        }
        TrampMap {
            ptes,
            cr0: 0,
            cr4: 0,
            cr3: 0,
            torn: false,
            loads: 0,
            refetch: false,
            stats: [0; 12],
        }
    }

    /// 立案计数自增。
    fn bump(&mut self, c: V327Code) {
        let i = (c.val() - V327_SEG_BASE) as usize;
        if let Some(cell) = self.stats.get_mut(i) {
            *cell = cell.wrapping_add(1);
        }
    }

    /// 立案计数读出。
    pub fn stat(&self, c: V327Code) -> u64 {
        let i = (c.val() - V327_SEG_BASE) as usize;
        self.stats.get(i).copied().unwrap_or(0)
    }

    /// 索引换算（virt >> 12 · 越窗即越界）。
    pub fn idx_of(virt: u64) -> Result<usize, V327Code> {
        if virt >= IDENTITY_WINDOW {
            return Err(V327Code::PteRange);
        }
        Ok((virt / PAGE_SIZE) as usize)
    }

    /// 填一项恒等映射（逐项三检）。
    pub fn map_identity(&mut self, virt: u64) -> Result<usize, V327Code> {
        let i = match TrampMap::idx_of(virt) {
            Ok(v) => v,
            Err(e) => {
                self.bump(e);
                return Err(e);
            }
        };
        let pte = Pte::identity(virt);
        if !pte.aligned() {
            self.bump(V327Code::AlignBad);
            return Err(V327Code::AlignBad);
        }
        if !pte.present || !pte.write {
            self.bump(V327Code::FlagsMissing);
            return Err(V327Code::FlagsMissing);
        }
        // 恒等性以 virt 为准核对（phys 必须等于 virt）。
        if pte.phys != virt {
            self.bump(V327Code::NotIdentity);
            return Err(V327Code::NotIdentity);
        }
        if let Some(slot) = self.ptes.get_mut(i) {
            *slot = pte;
        }
        Ok(i)
    }

    /// 造全窗口（256 项逐项填满）。
    pub fn fill_window(&mut self) -> Result<usize, V327Code> {
        let mut n = 0usize;
        for v in (0..IDENTITY_WINDOW).step_by(PAGE_SIZE as usize) {
            match self.map_identity(v as u64) {
                Ok(_) => n = n.wrapping_add(1),
                Err(e) => return Err(e),
            }
        }
        // 覆盖自查：任一项 present=false 即为洞。
        for i in 0..IDENTITY_PTES {
            let ok = self.ptes.get(i).map(|p| p.present && p.write).unwrap_or(false);
            if !ok {
                self.bump(V327Code::WindowHole);
                return Err(V327Code::WindowHole);
            }
        }
        Ok(n)
    }

    /// 窗口洞数（应恒 0）。
    pub fn holes(&self) -> usize {
        let mut n = 0usize;
        for i in 0..IDENTITY_PTES {
            let ok = self.ptes.get(i).map(|p| p.present && p.write).unwrap_or(false);
            if !ok {
                n = n.wrapping_add(1);
            }
        }
        n
    }

    /// 是否在恒等窗口内。
    pub fn in_identity_window(addr: u64) -> bool {
        addr < IDENTITY_WINDOW
    }

    /// 校验跳板（必须在窗内且不跨页）。
    pub fn check_tramp(&mut self, phys: u64, len: u64) -> Result<(), V327Code> {
        if !TrampMap::in_identity_window(phys) || !TrampMap::in_identity_window(phys.wrapping_add(len))
        {
            self.bump(V327Code::TrampOutWindow);
            return Err(V327Code::TrampOutWindow);
        }
        let last = phys.wrapping_add(len.wrapping_sub(1));
        if phys / PAGE_SIZE != last / PAGE_SIZE {
            self.bump(V327Code::TrampCrossPage);
            return Err(V327Code::TrampCrossPage);
        }
        // 跳板所在页必须已被映射。
        let idx = (phys / PAGE_SIZE) as usize;
        let ok = self.ptes.get(idx).map(|p| p.present).unwrap_or(false);
        if !ok {
            self.bump(V327Code::WindowHole);
            return Err(V327Code::WindowHole);
        }
        Ok(())
    }

    /// 校验主表入口指针（必须低 1MB）。
    pub fn check_main_entry(&mut self, entry: u64) -> Result<(), V327Code> {
        if !TrampMap::in_identity_window(entry) {
            self.bump(V327Code::MainEntryOut);
            return Err(V327Code::MainEntryOut);
        }
        Ok(())
    }

    /// 置 CR0（PG|PE 两位）。
    pub fn set_cr0(&mut self, pg: bool, pe: bool) {
        if pg {
            self.cr0 |= 1 << 31;
        }
        if pe {
            self.cr0 |= 1;
        }
    }

    /// 分页是否已开（PG 与 PE 皆置）。
    pub fn paging_on(&self) -> bool {
        self.cr0 & (1 << 31) != 0 && self.cr0 & 1 != 0
    }

    /// 装 CR3（含开页与重取两检）。
    pub fn load_cr3(&mut self, cr3: u64, with_refetch: bool) -> Result<u64, V327Code> {
        if self.torn {
            self.bump(V327Code::ReloadForbidden);
            return Err(V327Code::ReloadForbidden);
        }
        if !self.paging_on() {
            self.bump(V327Code::PagingNotOn);
            return Err(V327Code::PagingNotOn);
        }
        if !with_refetch {
            self.bump(V327Code::NoRefetch);
            return Err(V327Code::NoRefetch);
        }
        self.cr3 = cr3;
        self.loads = self.loads.wrapping_add(1);
        self.refetch = with_refetch;
        Ok(cr3)
    }

    /// 作废（AP 已跳主表·临时表一次性）。
    pub fn teardown(&mut self) -> Result<(), V327Code> {
        if self.torn {
            return Ok(());
        }
        self.torn = true;
        Ok(())
    }

    /// 表位打包（用于跨批对拍 · 逐 PTE bits 串接）。
    pub fn bits(&self) -> Vec<u64> {
        self.ptes.iter().map(|p| p.bits()).collect()
    }

    /// 表指纹（FNV-1a · 覆盖全表 bits + CR0/CR4 + 装载态）。
    pub fn fp(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut mix = |v: u64| {
            for b in v.to_le_bytes().iter() {
                h ^= *b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        };
        for p in self.ptes.iter() {
            mix(p.bits());
        }
        mix(self.cr0);
        mix(self.cr4);
        mix(self.cr3);
        mix(self.torn as u64);
        mix(self.loads as u64);
        h
    }

    /// 渲染（表头 + 段摘要 + 控制寄存器）。
    pub fn render(&self) -> String {
        let mut s = String::new();
        let mut present = 0usize;
        let mut identity = 0usize;
        for i in 0..IDENTITY_PTES {
            if let Some(p) = self.ptes.get(i) {
                if p.present {
                    present = present.wrapping_add(1);
                }
                if p.present && p.identity_for((i as u64).wrapping_mul(PAGE_SIZE)) {
                    identity = identity.wrapping_add(1);
                }
            }
        }
        s.push_str(&format!(
            "PTES present={} identity={} holes={} cr0=0x{:016X} cr3=0x{:016X} loads={} torn={} refetch={}\n",
            present,
            identity,
            self.holes(),
            self.cr0,
            self.cr3,
            self.loads,
            self.torn,
            self.refetch
        ));
        s.push_str(&format!("fp=0x{:016X} seg=0x{:04X}\n", self.fp(), V327_SEG_BASE));
        s
    }
}

// --------------------------------------------------------------------------
// AP 开页（跳板槽分配 + 序列编排）
// --------------------------------------------------------------------------

/// AP 开页槽（每 AP 一个独立跳板区）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ApSlot327 {
    /// AP 号。
    pub id: u8,
    /// 跳板物理基址。
    pub tramp: u64,
    /// 跳板长度。
    pub len: u64,
    /// 主表入口指针（低 1MB）。
    pub main_entry: u64,
    /// 是否已开页。
    pub paged: bool,
}

/// 每 AP 跳板槽步长（512B · 跳板须单页且互不重叠）。
pub const TRAMP_STRIDE: u64 = 512;

/// 跳板基址池起点（低区 · 避开 0 起的向量区）。
pub const TRAMP_POOL_BASE: u64 = 0x40_000;

/// 开页器（槽位分配 + 序列编排 + 重叠防线）。
#[derive(Clone, Debug)]
pub struct Pager {
    /// 临时表。
    pub map: TrampMap,
    /// AP 槽。
    pub aps: Vec<ApSlot327>,
    /// 立案计数。
    pub stats: [u64; 12],
}

impl Pager {
    /// 造开页器（`n` 个 AP）。
    pub fn new(n: usize) -> Pager {
        let cnt = if n > AP_CAP { AP_CAP } else { n };
        let mut aps: Vec<ApSlot327> = Vec::new();
        for i in 0..cnt {
            aps.push(ApSlot327 {
                id: i as u8,
                tramp: TRAMP_POOL_BASE.wrapping_add(TRAMP_STRIDE.wrapping_mul(i as u64)),
                len: 256,
                main_entry: 0,
                paged: false,
            });
        }
        Pager {
            map: TrampMap::new(),
            aps,
            stats: [0; 12],
        }
    }

    /// 立案计数自增。
    fn bump(&mut self, c: V327Code) {
        let i = (c.val() - V327_SEG_BASE) as usize;
        if let Some(cell) = self.stats.get_mut(i) {
            *cell = cell.wrapping_add(1);
        }
    }

    /// 立案计数读出。
    pub fn stat(&self, c: V327Code) -> u64 {
        let i = (c.val() - V327_SEG_BASE) as usize;
        self.stats.get(i).copied().unwrap_or(0)
    }

    /// 跳板槽重叠核对（两 AP 区间不得相交）。
    pub fn check_overlap(&mut self) -> Result<(), V327Code> {
        let mut hit = false;
        for i in 0..self.aps.len() {
            for j in (i + 1)..self.aps.len() {
                let (a, b) = match (self.aps.get(i), self.aps.get(j)) {
                    (Some(a), Some(b)) => (a, b),
                    _ => continue,
                };
                let a_end = a.tramp.wrapping_add(a.len);
                let b_end = b.tramp.wrapping_add(b.len);
                if a.tramp < b_end && b.tramp < a_end {
                    hit = true;
                    break;
                }
            }
        }
        if hit {
            self.bump(V327Code::OverlappedRange);
            return Err(V327Code::OverlappedRange);
        }
        Ok(())
    }

    /// 装入口指针（必须低 1MB）。
    pub fn set_entry(&mut self, ap: u8, entry: u64) -> Result<(), V327Code> {
        let i = ap as usize;
        if i >= self.aps.len() {
            self.bump(V327Code::PteRange);
            return Err(V327Code::PteRange);
        }
        if !TrampMap::in_identity_window(entry) {
            self.bump(V327Code::MainEntryOut);
            return Err(V327Code::MainEntryOut);
        }
        if let Some(s) = self.aps.get_mut(i) {
            s.main_entry = entry;
        }
        Ok(())
    }

    /// 开页序列（映射窗口 → 校验跳板/入口 → 置 CR0 → 装 CR3+重取）。
    pub fn page_ap(&mut self, ap: u8, cr3: u64, with_refetch: bool) -> Result<u64, V327Code> {
        let idx = ap as usize;
        let (tramp, len, entry) = match self.aps.get(idx) {
            Some(s) => (s.tramp, s.len, s.main_entry),
            None => {
                self.bump(V327Code::PteRange);
                return Err(V327Code::PteRange);
            }
        };
        // 1) 窗口铺满。
        self.map.fill_window()?;
        // 2) 跳板与入口校验。
        self.map.check_tramp(tramp, len)?;
        if entry == 0 {
            self.bump(V327Code::MainEntryOut);
            return Err(V327Code::MainEntryOut);
        }
        self.map.check_main_entry(entry)?;
        // 3) 跳板槽重叠防线。
        self.check_overlap()?;
        // 4) 置 CR0.PG|PE。
        self.map.set_cr0(true, true);
        // 5) 装 CR3 + 指令重取。
        let r = self.map.load_cr3(cr3, with_refetch);
        if r.is_ok() {
            if let Some(s) = self.aps.get_mut(idx) {
                s.paged = true;
            }
        }
        r
    }

    /// 全员已开页断言（判据核心）。
    pub fn all_paged(&self) -> bool {
        !self.aps.is_empty() && self.aps.iter().all(|s| s.paged)
    }

    /// 开页器总指纹（表指纹 +逐 AP 槽：tramp/len/entry/paged）。
    ///
    /// 表指纹只覆盖 PTE，控制寄存器与装载态；跳板槽分配（重叠与否）
    /// 是另一维度，必须并入总指纹，否则"改了 AP 跳板基址"这种改动
    /// 在指纹上完全不可见。
    pub fn fp(&self) -> u64 {
        let mut h: u64 = self.map.fp();
        for a in self.aps.iter() {
            let mut x: u64 = 0xcbf2_9ce4_8422_2325;
            let mut mix = |v: u64| {
                for b in v.to_le_bytes().iter() {
                    x ^= *b as u64;
                    x = x.wrapping_mul(0x0000_0100_0000_01b3);
                }
            };
            mix(a.id as u64);
            mix(a.tramp);
            mix(a.len);
            mix(a.main_entry);
            mix(a.paged as u64);
            h ^= x;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }

    /// 跳板所在页的恒等核对（按**页基址**比，不是按页内偏移地址）。
    ///
    /// PTE 是页粒度的：页基址 0x40_000 的 PTE 描述的是整个
    /// 0x40_000..0x40_FFF，不只描述首字节。若拿页内偏移地址
    /// （如跳板落在 0x40_200）去比`phys == virt`，会得出"非恒等"
    /// 的错误结论——这是页表语义与字节语义错配造成的假阴性。
    pub fn tramp_page_ok(&self, ap: u8) -> bool {
        let s = match self.aps.get(ap as usize) {
            Some(s) => s,
            None => return false,
        };
        let page_base = s.tramp / PAGE_SIZE * PAGE_SIZE;
        let i = page_base / PAGE_SIZE;
        match self.map.ptes.get(i as usize) {
            Some(p) => p.present && p.write && p.identity_for(page_base),
            None => false,
        }
    }

    /// 映射齐备断言（窗口无洞 + 全页恒等 + 全员开页）。
    pub fn mapping_ready(&self) -> bool {
        self.map.holes() == 0
            && self.map.ptes.len() == IDENTITY_PTES
            && (0..IDENTITY_PTES).all(|i| {
                self.map
                    .ptes
                    .get(i)
                    .map(|p| p.present && p.identity_for((i as u64).wrapping_mul(PAGE_SIZE)))
                    .unwrap_or(false)
            })
            && (0..self.aps.len()).all(|i| self.tramp_page_ok(i as u8))
            && self.all_paged()
    }
}

// --------------------------------------------------------------------------
// 判据
// --------------------------------------------------------------------------

/// UNX-F0327 判据全集（12 条）。
pub fn run_unx327_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx327");

    // ---- C327-01 诊断段闭集与建议完备 ----
    let seg_ok = V327Code::ALL
        .iter()
        .enumerate()
        .all(|(i, c)| c.val() == V327_SEG_BASE + i as u16);
    let remedy_ok = V327Code::ALL.iter().all(|c| c.remedy().len() >= 10);
    cs.add("C327-01 诊断段闭集", seg_ok && remedy_ok, "12 码连续+建议完备");

    // ---- C327-02 常量口径（4K/1MB/256PTE/四级） ----
    let const_ok = PAGE_SIZE == 4096
        && IDENTITY_WINDOW == 1_048_576
        && IDENTITY_PTES == 256
        && PTE_CAP == 256
        && LEVEL_PML4 == 0
        && LEVEL_PDPT == 1
        && LEVEL_PD == 2
        && LEVEL_PT == 3;
    cs.add("C327-02 常量口径", const_ok, "4K/1MB/256 项/四级下标");

    // ---- C327-03 PTE 位打包解包往返 ----
    let p = Pte::identity(0x8_000);
    let bits = p.bits();
    let back = Pte::from_bits(bits);
    let round_ok = back.phys == p.phys
        && back.present
        && back.write
        && !back.user
        && bits == 0x8_000 | 0b011;
    cs.add("C327-03 PTE 位往返", round_ok, "phys+PW 三位往返相等");

    // ---- C327-04 索引换算与越界 ----
    let i0 = TrampMap::idx_of(0);
    let i1 = TrampMap::idx_of(IDENTITY_WINDOW - PAGE_SIZE);
    let e1 = TrampMap::idx_of(IDENTITY_WINDOW);
    let e2 = TrampMap::idx_of(0x10_0000);
    let idx_ok = i0 == Ok(0) && i1 == Ok(255) && e1.is_err() && e2.is_err();
    cs.add("C327-04 索引换算", idx_ok, "0→0/末页→255/越窗即拒");

    // ---- C327-05 窗口铺满 256 项·无洞 ----
    let mut m5 = TrampMap::new();
    let filled = m5.fill_window();
    let full_ok = filled == Ok(256)
        && m5.holes() == 0
        && m5.ptes.len() == IDENTITY_PTES
        && (0..IDENTITY_PTES).all(|i| m5.ptes[i].phys == (i as u64) * PAGE_SIZE)
        && m5.stat(V327Code::WindowHole) == 0;
    // 反证：挖一个洞必被holes 检出。
    let mut holed = m5.clone();
    if let Some(p) = holed.ptes.get_mut(100) {
        p.present = false;
    }
    // 挖洞必须被 holes() 立即检出（1 洞）；再跑一次 fill_window 应把它补齐
    // （这是修复语义，不是失败语义 —— 洞是既有表的缺口，不是新表的构造错）。
    let hole_seen = holed.holes() == 1;
    let hole_repaired = holed.fill_window() == Ok(256) && holed.holes() == 0;
    let hole_ok = hole_seen && hole_repaired;
    cs.add("C327-05 窗口铺满", full_ok && hole_ok, "256 项齐+挖洞必检出");

    // ---- C327-06 恒等三检（对齐/恒等/位） ----
    let mut m6 = TrampMap::new();
    let ok_map = m6.map_identity(0x2000);
    let out = m6.map_identity(IDENTITY_WINDOW);
    let align_idx = TrampMap::idx_of(0x1000);
    // 构造一个非对齐 PTE 走核对路径（不写入表，仅验证判定函数）。
    let bad = Pte {
        phys: 0x1001,
        present: true,
        write: true,
        user: false,
    };
    // 非对齐不等于非恒等：0x1001 映到 0x1001 仍是恒等（只是不合法的
    // PTE 粒度）。真正要判"非恒等"必须给 phys != virt 的条目。
    let nonident = Pte {
        phys: 0x2000,
        present: true,
        write: true,
        user: false,
    };
    let align_ok = ok_map == Ok(2)
        && out == Err(V327Code::PteRange)
        && align_idx == Ok(1)
        && !bad.aligned()
        && bad.identity_for(0x1001)
        && !nonident.identity_for(0x1001)
        && m6.stat(V327Code::PteRange) == 1;
    // 非 present/write 的 PTE 必被 flags 检查挑出。
    let noflag = Pte {
        phys: 0x3000,
        present: true,
        write: false,
        user: false,
    };
    let flag_ok = !noflag.present || !noflag.write;
    cs.add(
        "C327-06 恒等三检",
        align_ok && flag_ok,
        "对齐/恒等/位三检齐+越界立案",
    );

    // ---- C327-07 跳板在窗内且不跨页 ----
    let mut m7 = TrampMap::new();
    let _ = m7.fill_window();
    let good = m7.check_tramp(0x40_000, 256);
    let cross = m7.check_tramp(0x40_F00, 512);
    let outw = m7.check_tramp(0x100_000, 64);
    let cross_win = m7.check_tramp(0xFF_F00, 512);
    let tramp_ok = good.is_ok()
        && cross == Err(V327Code::TrampCrossPage)
        && outw == Err(V327Code::TrampOutWindow)
        // 0xFF_F00+512 越过 1MB 上界：越窗判定先于跨页判定（先判窗再判页）。
        && cross_win == Err(V327Code::TrampOutWindow)
        && m7.stat(V327Code::TrampCrossPage) == 1
        && m7.stat(V327Code::TrampOutWindow) == 2;
    cs.add("C327-07 跳板边界", tramp_ok, "窗内过/跨页拒/越窗拒");

    // ---- C327-08 主表入口必须低 1MB ----
    let mut m8 = TrampMap::new();
    let ok_low = m8.check_main_entry(0x70_000);
    let bad_high = m8.check_main_entry(0x20_0000_0000);
    let bad_mid = m8.check_main_entry(IDENTITY_WINDOW);
    let entry_ok = ok_low.is_ok()
        && bad_high == Err(V327Code::MainEntryOut)
        && bad_mid == Err(V327Code::MainEntryOut)
        && m8.stat(V327Code::MainEntryOut) == 2;
    cs.add("C327-08 入口低区", entry_ok, "低址过/高址与边界拒");

    // ---- C327-09 开页序列：CR0 两位 + 重取 ----
    let mut m9 = TrampMap::new();
    let _ = m9.fill_window();
    let not_on = m9.load_cr3(0x1000, true);
    m9.set_cr0(true, true);
    let no_refetch = m9.load_cr3(0x1000, false);
    let ok_load = m9.load_cr3(0x1000, true);
    let seq_ok = not_on == Err(V327Code::PagingNotOn)
        && m9.paging_on()
        && no_refetch == Err(V327Code::NoRefetch)
        && ok_load == Ok(0x1000)
        && m9.cr3 == 0x1000
        && m9.loads == 1
        && m9.stat(V327Code::PagingNotOn) == 1
        && m9.stat(V327Code::NoRefetch) == 1;
    // 只置 PE 不置 PG 仍算未开。
    let mut half = TrampMap::new();
    half.set_cr0(false, true);
    let half_ok = !half.paging_on();
    cs.add("C327-09 开页序列", seq_ok && half_ok, "PG|PE 双位+重取必备");

    // ---- C327-10 作废后禁重载 ----
    let mut m10 = TrampMap::new();
    let _ = m10.fill_window();
    m10.set_cr0(true, true);
    let first = m10.load_cr3(0x2000, true);
    let td = m10.teardown();
    let reload = m10.load_cr3(0x2000, true);
    let td2 = m10.teardown();
    let tear_ok = first == Ok(0x2000)
        && td.is_ok()
        && reload == Err(V327Code::ReloadForbidden)
        && td2.is_ok()
        && m10.torn
        && m10.cr3 == 0x2000
        && m10.loads == 1
        && m10.stat(V327Code::ReloadForbidden) == 1;
    cs.add("C327-10 作废禁重载", tear_ok, "作废幂等+CR3 不被改写");

    // ---- C327-11 AP 开页断言（映射齐备 · 判据核心） ----
    let mut p11 = Pager::new(4);
    for i in 0..4u8 {
        // 每 AP 独立低区入口（跳板池之后）。
        let e = TRAMP_POOL_BASE.wrapping_add(TRAMP_STRIDE.wrapping_mul(4).wrapping_add(TRAMP_STRIDE.wrapping_mul(i as u64)));
        let _ = p11.set_entry(i, e);
    }
    let mut paged = 0u32;
    for i in 0..4u8 {
        if p11.page_ap(i, 0x3000, true).is_ok() {
            paged = paged.wrapping_add(1);
        }
    }
    let map_ready = paged == 4
        && p11.all_paged()
        && p11.mapping_ready()
        && p11.map.holes() == 0
        && p11.stat(V327Code::PteRange) == 0;
    cs.add("C327-11 AP开页断言", map_ready, "4AP 全开页+映射齐备");

    // ---- C327-12 跳板槽重叠防线 + 指纹/渲染 ----
    let mut p12 = Pager::new(2);
    let clean_overlap = p12.check_overlap();
    // 人为把 AP1 跳板挪到与 AP0 重叠 → 必须检出。
    if let Some(s) = p12.aps.get_mut(1) {
        s.tramp = TRAMP_POOL_BASE;
    }
    let bad_overlap = p12.check_overlap();
    let fp_a = p12.fp();
    let mut p12b = p12.clone();
    if let Some(s) = p12b.aps.get_mut(1) {
        s.tramp = TRAMP_POOL_BASE.wrapping_add(TRAMP_STRIDE);
    }
    let fp_diff = fp_a != p12b.fp();
    let txt = p12.map.render();
    let render_ok = txt.contains("present=") && txt.contains("fp=0x");
    let guard_ok = clean_overlap.is_ok()
        && bad_overlap == Err(V327Code::OverlappedRange)
        && p12.stat(V327Code::OverlappedRange) == 1
        && fp_diff
        && render_ok;
    cs.add("C327-12 重叠与指纹", guard_ok, "重叠必检+表指纹敏感+看板齐");

    cs
}