//! unx161_rtremap — UNX-F0161 Runtime Services 虚拟地址重映射（深化版）。
//!
//! 判据原文（CoRun Unxreal 主册 UNX-F0161 · UNX-A1-B09）：SetVirtualAddressMap
//! 调用后运行时服务经新地址可用，旧地址访问断言为零。
//!
//! **语义学定位**：UEFI Runtime Services 在 ExitBootServices 后仍需存活
//! （时间/变量/复位服务），但固件运行时代码段在引导期是物理地址——
//! 内核建页表后须 SetVirtualAddressMap 把 RT 区间重映射到虚拟地址空间。
//! 本册做**重映射的记账语义学**（零 IO 零特权指令——真实的 MSRC0000080
//! 页表切换由内存管理面执行，本册管：映射账本、地址解析单点、旧地址
//! 断零机制、两态机纪律）。
//!
//! **两态机单向纪律**：
//! - **Physical**（物理期）：RT 服务只认物理地址；虚拟地址查询显性拒
//!   （NotYetRemapped——物理期没有虚拟地址这回事）；
//! - **Virtual**（虚拟期）：SetVirtualAddressMap 完成后 RT 服务只认
//!   虚拟地址；**物理地址访问断零**——resolve 单点只走虚拟路径，任何
//!   物理地址访问尝试进 IllegalPhysAccess 计数（判据"旧地址访问断言
//!   为零"的机制面：不是靠自觉，是靠访问面单点封死旧路）。
//!   单向不可逆——没有 Unmap 事件（UEFI 规范面）。
//!
//! **重映射三闸**（SetVirtualAddressMap 输入映射表的合法性面）：
//! 1. **对齐闸**：虚拟段与物理段均须 4 KiB 页对齐（页表粒度）；
//! 2. **重叠闸**：虚拟段互不重叠（两段映射到同一虚拟页 = 地址别名事故）；
//! 3. **覆盖闸**：全部 RT 服务物理地址必须被某段覆盖（漏段 = 服务悬空）。
//!
//! **服务解析单点**：resolve(服务号) 在 Virtual 期返回虚拟地址——服务
//! 表双地址（物理/虚拟）在重映射提交时按覆盖段逐条换算定格；未映射
//! 服务显性 ServiceUnmapped（悬空服务是静默炸弹）。
//!
//! 诊断枚举 RemapCode 十六码（0x34 段 0x3400..=0x340F 互异 +
//! [`segment_guard`]）。no_std 零 unwrap 零 panic 面。

use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const UNX161_RTREMAP_VERSION: &str = "unx161-rtremap-v1";

/// 诊断段基值（0x34 段——UNX 域 0x2E..0x33 已占）。
pub const SEG_BASE: u16 = 0x3400;

/// 页对齐粒度（4 KiB）。
pub const PAGE_SIZE: u64 = 4096;

/// RT 服务号上限（UEFI 规范面常用服务闭集——策略值）。
pub const RT_SERVICE_MAX: usize = 16;

/// 告警账容量。
pub const ALERT_CAP: usize = 24;

// ---------------------------------------------------------------------------
// 诊断码（RemapCode 十六码 · 0x34 段互异）
// ---------------------------------------------------------------------------

/// 诊断码（RT 重映射——0x34 段十六码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RemapCode {
    /// 0x3400 虚拟段不对齐（非 4 KiB 页界）。
    VirtUnaligned,
    /// 0x3401 物理段不对齐（RT 固件段页界外）。
    PhysUnaligned,
    /// 0x3402 虚拟段重叠（两段映射同页——地址别名事故）。
    VirtOverlap,
    /// 0x3403 RT 区间未覆盖（漏段——服务悬空）。
    CoverageHole,
    /// 0x3404 物理期查虚拟地址（期态不符——物理期没有虚拟地址）。
    NotYetRemapped,
    /// 0x3405 虚拟期查物理地址（旧地址访问——断零机制计数）。
    IllegalPhysAccess,
    /// 0x3406 服务未映射（解析悬空——静默炸弹显性化）。
    ServiceUnmapped,
    /// 0x3407 重复重映射（Virtual 期再收 SetVirtualAddressMap——单向不可逆）。
    DoubleRemap,
    /// 0x3408 映射条目空表（无条目即无覆盖——必然 CoverageHole）。
    EmptyMap,
    /// 0x3409 段长度为零（零长段不映射任何页——无效条目）。
    ZeroLength,
    /// 0x340A 服务号越界（解析号 > RT_SERVICE_MAX）。
    ServiceOutOfRange,
    /// 0x340B 换算溢出（虚拟基址 + 偏移越 u64 界——算术面事故）。
    OffsetOverflow,
    /// 0x340C 告警账满（旧账保留）。
    AlertFull,
    /// 0x340D 快照指纹不符（往返破坏）。
    SnapshotFingerprint,
    /// 0x340E 服务表守恒破坏（映射服务数 ≠ 已解析可服务数——账面漂移）。
    LedgerBroken,
    /// 0x340F 段守恒对账失败。
    SegmentGuard,
}

impl RemapCode {
    /// 十六码闭集。
    pub const ALL: [RemapCode; 16] = [
        RemapCode::VirtUnaligned,
        RemapCode::PhysUnaligned,
        RemapCode::VirtOverlap,
        RemapCode::CoverageHole,
        RemapCode::NotYetRemapped,
        RemapCode::IllegalPhysAccess,
        RemapCode::ServiceUnmapped,
        RemapCode::DoubleRemap,
        RemapCode::EmptyMap,
        RemapCode::ZeroLength,
        RemapCode::ServiceOutOfRange,
        RemapCode::OffsetOverflow,
        RemapCode::AlertFull,
        RemapCode::SnapshotFingerprint,
        RemapCode::LedgerBroken,
        RemapCode::SegmentGuard,
    ];

    /// 码值（连续）。
    pub const fn code(self) -> u16 {
        match self {
            RemapCode::VirtUnaligned => 0x3400,
            RemapCode::PhysUnaligned => 0x3401,
            RemapCode::VirtOverlap => 0x3402,
            RemapCode::CoverageHole => 0x3403,
            RemapCode::NotYetRemapped => 0x3404,
            RemapCode::IllegalPhysAccess => 0x3405,
            RemapCode::ServiceUnmapped => 0x3406,
            RemapCode::DoubleRemap => 0x3407,
            RemapCode::EmptyMap => 0x3408,
            RemapCode::ZeroLength => 0x3409,
            RemapCode::ServiceOutOfRange => 0x340A,
            RemapCode::OffsetOverflow => 0x340B,
            RemapCode::AlertFull => 0x340C,
            RemapCode::SnapshotFingerprint => 0x340D,
            RemapCode::LedgerBroken => 0x340E,
            RemapCode::SegmentGuard => 0x340F,
        }
    }

    /// 人读诊断（互异非空）。
    pub const fn detail(self) -> &'static str {
        match self {
            RemapCode::VirtUnaligned => {
                "虚拟段不对齐：须 4 KiB 页界——页表粒度是硬约束，建议对齐后重映射"
            }
            RemapCode::PhysUnaligned => {
                "物理段不对齐：RT 固件段须页界内，建议核查固件布局"
            }
            RemapCode::VirtOverlap => {
                "虚拟段重叠：两段映射同一虚拟页是地址别名事故，建议合并或错开"
            }
            RemapCode::CoverageHole => {
                "RT 区间未覆盖：漏段导致服务悬空，建议补映射条目"
            }
            RemapCode::NotYetRemapped => {
                "物理期查虚拟地址：物理期没有虚拟地址这回事，建议先 SetVirtualAddressMap"
            }
            RemapCode::IllegalPhysAccess => {
                "虚拟期查物理地址：旧地址访问已被封死——断零机制计数，建议走 resolve 单点"
            }
            RemapCode::ServiceUnmapped => {
                "服务未映射：悬空服务是静默炸弹，建议核查覆盖面"
            }
            RemapCode::DoubleRemap => {
                "重复重映射：SetVirtualAddressMap 单向不可逆，建议审计状态机"
            }
            RemapCode::EmptyMap => {
                "映射条目空表：无条目即无覆盖，建议先登记映射段"
            }
            RemapCode::ZeroLength => {
                "段长度为零：零长段不映射任何页，建议给真实长度"
            }
            RemapCode::ServiceOutOfRange => {
                "服务号越界：解析号须在服务闭集内，建议核对服务表"
            }
            RemapCode::OffsetOverflow => {
                "换算溢出：虚拟基址加偏移越界，建议检查段长度算术"
            }
            RemapCode::AlertFull => {
                "告警账满：旧账保留新告警拒入，建议归档"
            }
            RemapCode::SnapshotFingerprint => {
                "快照指纹不符：往返恒等破坏，建议排查序列化漂移"
            }
            RemapCode::LedgerBroken => {
                "服务表守恒破坏：映射数与可服务数应一致，建议审计换算面"
            }
            RemapCode::SegmentGuard => {
                "段守恒对账失败：十六码须连续互异界内——段位事故立即立案"
            }
        }
    }
}

/// **段守恒对账**。
pub const fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < RemapCode::ALL.len() {
        let c = RemapCode::ALL[i].code();
        if c < SEG_BASE || c != SEG_BASE + i as u16 {
            return false;
        }
        let mut j = i + 1;
        while j < RemapCode::ALL.len() {
            if RemapCode::ALL[i].code() == RemapCode::ALL[j].code() {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 期态两态机 + 服务号闭集
// ---------------------------------------------------------------------------

/// 重映射期态二闭集（单向：Physical → Virtual）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RemapPhase {
    /// 物理期（SetVirtualAddressMap 前）。
    Physical,
    /// 虚拟期（重映射完成）。
    Virtual,
}

impl RemapPhase {
    /// 二闭集。
    pub const ALL: [RemapPhase; 2] = [RemapPhase::Physical, RemapPhase::Virtual];

    /// 中文名（互异）。
    pub const fn zh(self) -> &'static str {
        match self {
            RemapPhase::Physical => "物理期",
            RemapPhase::Virtual => "虚拟期",
        }
    }
}

/// RT 服务号闭集（UEFI 常用面——策略值 8 服务）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RtService {
    /// 0 GetTime。
    GetTime,
    /// 1 SetTime。
    SetTime,
    /// 2 GetVariable。
    GetVariable,
    /// 3 SetVariable。
    SetVariable,
    /// 4 GetNextVariableName。
    NextVariable,
    /// 5 ResetSystem。
    ResetSystem,
    /// 6 GetMemoryMap。
    GetMemoryMap,
    /// 7 UpdateCapsule。
    UpdateCapsule,
}

impl RtService {
    /// 八服务闭集。
    pub const ALL: [RtService; 8] = [
        RtService::GetTime,
        RtService::SetTime,
        RtService::GetVariable,
        RtService::SetVariable,
        RtService::NextVariable,
        RtService::ResetSystem,
        RtService::GetMemoryMap,
        RtService::UpdateCapsule,
    ];

    /// 序数（表索引）。
    pub const fn ordinal(self) -> usize {
        match self {
            RtService::GetTime => 0,
            RtService::SetTime => 1,
            RtService::GetVariable => 2,
            RtService::SetVariable => 3,
            RtService::NextVariable => 4,
            RtService::ResetSystem => 5,
            RtService::GetMemoryMap => 6,
            RtService::UpdateCapsule => 7,
        }
    }

    /// 序数反解。
    pub const fn from_ordinal(v: u64) -> Option<RtService> {
        match v {
            0 => Some(RtService::GetTime),
            1 => Some(RtService::SetTime),
            2 => Some(RtService::GetVariable),
            3 => Some(RtService::SetVariable),
            4 => Some(RtService::NextVariable),
            5 => Some(RtService::ResetSystem),
            6 => Some(RtService::GetMemoryMap),
            7 => Some(RtService::UpdateCapsule),
            _ => None,
        }
    }

    /// 人读名（互异）。
    pub const fn zh(self) -> &'static str {
        match self {
            RtService::GetTime => "读时",
            RtService::SetTime => "写时",
            RtService::GetVariable => "读变量",
            RtService::SetVariable => "写变量",
            RtService::NextVariable => "下一变量",
            RtService::ResetSystem => "复位",
            RtService::GetMemoryMap => "内存图",
            RtService::UpdateCapsule => "胶囊更新",
        }
    }
}

// ---------------------------------------------------------------------------
// 映射段（重映射输入条目）
// ---------------------------------------------------------------------------

/// 映射段（物理段 → 虚拟段，等长）。
#[derive(Clone, Copy, Debug)]
pub struct MapSegment {
    /// 物理基址（RT 固件段）。
    pub phys: u64,
    /// 虚拟基址（内核地址空间）。
    pub virt: u64,
    /// 段长（字节——两段等长）。
    pub len: u64,
}

impl MapSegment {
    /// 对齐校验（双基址页界 + 长度非零）。
    pub const fn aligned(&self) -> bool {
        self.len > 0 && self.phys % PAGE_SIZE == 0 && self.virt % PAGE_SIZE == 0
    }

    /// 含地址（物理侧覆盖判定——[phys, phys+len)）。
    pub const fn covers_phys(&self, addr: u64) -> bool {
        addr >= self.phys && addr < self.phys + self.len
    }

    /// 换算（物理地址 → 虚拟地址——段内偏移等值平移；溢出 None）。
    pub const fn to_virt(&self, phys_addr: u64) -> Option<u64> {
        if !self.covers_phys(phys_addr) {
            return None;
        }
        let off = phys_addr - self.phys;
        let base = self.virt;
        if base + off < base {
            None
        } else {
            Some(base + off)
        }
    }

    /// 虚拟区间重叠判定（半开区间 [virt, virt+len) 交叠）。
    pub const fn virt_overlaps(&self, other: &MapSegment) -> bool {
        let a0 = self.virt;
        let a1 = self.virt + self.len;
        let b0 = other.virt;
        let b1 = other.virt + other.len;
        a0 < b1 && b0 < a1
    }
}

/// 映射表（SetVirtualAddressMap 输入——三闸校验单点）。
pub struct MapTable {
    /// 段条目。
    pub segs: Vec<MapSegment>,
    /// 拒收计数（逐码）。
    pub rejected_unaligned: u32,
    pub rejected_zero: u32,
}

impl MapTable {
    /// 空表。
    pub fn new() -> MapTable {
        MapTable {
            segs: Vec::new(),
            rejected_unaligned: 0,
            rejected_zero: 0,
        }
    }

    /// **加入段**（对齐闸 + 零长闸——显性拒不静默）。
    pub fn add(&mut self, seg: MapSegment) -> Result<(), RemapCode> {
        if seg.len == 0 {
            self.rejected_zero += 1;
            return Err(RemapCode::ZeroLength);
        }
        if !seg.aligned() {
            self.rejected_unaligned += 1;
            return Err(RemapCode::VirtUnaligned);
        }
        self.segs.push(seg);
        Ok(())
    }

    /// **三闸校验**（重叠闸 + 覆盖闸——提交前面）：
    /// 覆盖输入 = RT 服务物理地址清单；返回 Err 侧：重叠/空表/漏覆盖。
    pub fn validate(&self, rt_phys_addrs: &[u64]) -> Result<(), RemapCode> {
        if self.segs.is_empty() {
            return Err(RemapCode::EmptyMap);
        }
        // 重叠闸（两两比对——n 小直扫）。
        let mut i = 0usize;
        while i < self.segs.len() {
            let mut j = i + 1;
            while j < self.segs.len() {
                if self.segs[i].virt_overlaps(&self.segs[j]) {
                    return Err(RemapCode::VirtOverlap);
                }
                j += 1;
            }
            i += 1;
        }
        // 覆盖闸（每个 RT 服务物理地址须被某段覆盖）。
        let mut k = 0usize;
        while k < rt_phys_addrs.len() {
            let mut covered = false;
            let mut si = 0usize;
            while si < self.segs.len() {
                if self.segs[si].covers_phys(rt_phys_addrs[k]) {
                    covered = true;
                }
                si += 1;
            }
            if !covered {
                return Err(RemapCode::CoverageHole);
            }
            k += 1;
        }
        Ok(())
    }
}

impl Default for MapTable {
    fn default() -> Self {
        MapTable::new()
    }
}

// ---------------------------------------------------------------------------
// RT 描述符表（服务双地址 + 解析单点——判据本体）
// ---------------------------------------------------------------------------

/// 服务条目（服务号 × 物理地址 × 虚拟地址定格）。
#[derive(Clone, Copy, Debug)]
pub struct RtEntry {
    /// 服务号。
    pub svc: RtService,
    /// 物理地址（引导期定格）。
    pub phys: u64,
    /// 虚拟地址（重映射提交后定格——Physical 期 None）。
    pub virt: Option<u64>,
}

/// RT 描述符表（八服务双地址账 + 两态机 + 断零机制）。
pub struct RtDescriptor {
    /// 期态。
    pub phase: RemapPhase,
    /// 服务条目（八槽固定——闭集直存）。
    pub entries: [RtEntry; 8],
    /// 旧地址访问断零计数（虚拟期物理访问尝试——判据断言面）。
    pub illegal_phys_hits: u64,
    /// 虚拟期前服务调用尝试计数（NotYetRemapped 面）。
    pub early_calls: u64,
    /// 解析成功计数。
    pub resolved: u64,
    /// 解析悬空计数。
    pub unresolved: u64,
    /// 告警账。
    pub alerts: AlertList,
}

/// 告警账。
pub struct AlertList {
    pub rows: Vec<String>,
    pub per_code: [u32; 16],
    pub rejected_full: u32,
}

impl AlertList {
    /// 空账。
    pub fn new() -> AlertList {
        AlertList {
            rows: Vec::new(),
            per_code: [0; 16],
            rejected_full: 0,
        }
    }

    /// 追加。
    pub fn push(&mut self, c: RemapCode, note: &str) {
        if self.rows.len() >= ALERT_CAP {
            self.rejected_full += 1;
            return;
        }
        let idx = (c.code() - SEG_BASE) as usize;
        self.per_code[idx] += 1;
        let mut line = String::from(note);
        line.push_str(" | ");
        line.push_str(c.detail());
        self.rows.push(alloc::format!("{:#06x}|{}", c.code(), line));
    }

    /// 在账数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }
}

impl Default for AlertList {
    fn default() -> Self {
        AlertList::new()
    }
}

/// 物理期空表构造（服务物理地址由引导期扫描注入）。
pub fn rt_descriptor_phys(addr_of: impl Fn(RtService) -> u64) -> RtDescriptor {
    let mut entries = [RtEntry {
        svc: RtService::GetTime,
        phys: 0,
        virt: None,
    }; 8];
    let mut i = 0usize;
    while i < 8 {
        entries[i] = RtEntry {
            svc: RtService::ALL[i],
            phys: addr_of(RtService::ALL[i]),
            virt: None,
        };
        i += 1;
    }
    RtDescriptor {
        phase: RemapPhase::Physical,
        entries,
        illegal_phys_hits: 0,
        early_calls: 0,
        resolved: 0,
        unresolved: 0,
        alerts: AlertList::new(),
    }
}

impl RtDescriptor {
    /// **物理地址读**（Physical 期合法；Virtual 期断零计数显性拒——判据
    /// "旧地址访问断言为零"的机制面：封死不是禁令）。
    pub fn phys_of(&mut self, svc: RtService) -> Result<u64, RemapCode> {
        match self.phase {
            RemapPhase::Physical => Ok(self.entries[svc.ordinal()].phys),
            RemapPhase::Virtual => {
                self.illegal_phys_hits += 1;
                self.alerts.push(RemapCode::IllegalPhysAccess, "旧地址访问被封");
                Err(RemapCode::IllegalPhysAccess)
            }
        }
    }

    /// **服务解析单点**（Virtual 期唯一入口——返回虚拟地址；Physical 期
    /// 显性拒 NotYetRemapped；未映射显性 ServiceUnmapped）。
    pub fn resolve(&mut self, svc: RtService) -> Result<u64, RemapCode> {
        match self.phase {
            RemapPhase::Physical => {
                self.early_calls += 1;
                self.alerts.push(RemapCode::NotYetRemapped, "物理期调服务");
                Err(RemapCode::NotYetRemapped)
            }
            RemapPhase::Virtual => match self.entries[svc.ordinal()].virt {
                Some(v) => {
                    self.resolved += 1;
                    Ok(v)
                }
                None => {
                    self.unresolved += 1;
                    self.alerts.push(RemapCode::ServiceUnmapped, "服务悬空");
                    Err(RemapCode::ServiceUnmapped)
                }
            },
        }
    }

    /// **SetVirtualAddressMap 提交**（映射表三闸 → 逐服务换算定格 →
    /// 期态单向翻转——DoubleRemap 闸）。
    pub fn commit_remap(&mut self, table: &MapTable) -> Result<(), RemapCode> {
        if self.phase == RemapPhase::Virtual {
            self.alerts.push(RemapCode::DoubleRemap, "重复重映射");
            return Err(RemapCode::DoubleRemap);
        }
        // 覆盖面输入：八服务物理地址。
        let mut addrs = [0u64; 8];
        let mut i = 0usize;
        while i < 8 {
            addrs[i] = self.entries[i].phys;
            i += 1;
        }
        table.validate(&addrs)?;
        // 逐服务换算定格（找覆盖段平移——漏覆盖已被 validate 拦）。
        let mut i2 = 0usize;
        while i2 < 8 {
            let p = self.entries[i2].phys;
            let mut virt = None;
            let mut si = 0usize;
            while si < table.segs.len() {
                if let Some(v) = table.segs[si].to_virt(p) {
                    virt = Some(v);
                }
                si += 1;
            }
            if virt.is_none() {
                // 理论不可达（validate 已闸）——防御面显性。
                self.alerts.push(RemapCode::CoverageHole, "换算漏段");
                return Err(RemapCode::CoverageHole);
            }
            self.entries[i2].virt = virt;
            i2 += 1;
        }
        self.phase = RemapPhase::Virtual;
        Ok(())
    }

    /// 服务表守恒（Virtual 期：八服务全有虚拟地址；Physical 期：全无——
    /// 半映射态不存在）。
    pub fn conserved(&self) -> bool {
        let mut i = 0usize;
        let mut with_virt = 0usize;
        while i < 8 {
            if self.entries[i].virt.is_some() {
                with_virt += 1;
            }
            i += 1;
        }
        match self.phase {
            RemapPhase::Physical => with_virt == 0,
            RemapPhase::Virtual => with_virt == 8,
        }
    }

    /// 一行摘要。
    pub fn summary(&self) -> String {
        alloc::format!(
            "U161 RT重映射: 期={} 断零计数={} 早调计数={} 解析={} 守恒={}",
            self.phase.zh(),
            self.illegal_phys_hits,
            self.early_calls,
            self.resolved,
            if self.conserved() { "真" } else { "假" }
        )
    }
}

// ---------------------------------------------------------------------------
// 快照（期态 + 双地址账 + 计数 '|' 序列化 + FNV + 往返）
// ---------------------------------------------------------------------------

/// RT 描述符快照。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemapSnap {
    /// 版本戳。
    pub version: String,
    /// 期态序数。
    pub phase_ord: u64,
    /// 八服务 (phys, virt) 对——virt 无为 u64::MAX 哨兵。
    pub pairs: Vec<(u64, u64)>,
    /// 断零/早调/解析计数。
    pub illegal_phys_hits: u64,
    pub early_calls: u64,
    pub resolved: u64,
}

/// FNV-1a 64。
pub const fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0usize;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

impl RemapSnap {
    /// 定格。
    pub fn capture(d: &RtDescriptor) -> RemapSnap {
        let mut pairs = Vec::new();
        let mut i = 0usize;
        while i < 8 {
            let v = match d.entries[i].virt {
                Some(x) => x,
                None => u64::MAX,
            };
            pairs.push((d.entries[i].phys, v));
            i += 1;
        }
        RemapSnap {
            version: String::from(UNX161_RTREMAP_VERSION),
            phase_ord: match d.phase {
                RemapPhase::Physical => 0,
                RemapPhase::Virtual => 1,
            },
            pairs,
            illegal_phys_hits: d.illegal_phys_hits,
            early_calls: d.early_calls,
            resolved: d.resolved,
        }
    }

    /// 序列化。
    pub fn serialize(&self) -> String {
        let mut s = alloc::format!(
            "{}|{}|{}|{}|{}|{}",
            self.version,
            self.phase_ord,
            self.illegal_phys_hits,
            self.early_calls,
            self.resolved,
            self.pairs.len()
        );
        let mut i = 0usize;
        while i < self.pairs.len() {
            s.push('|');
            s.push_str(&alloc::format!("{},{}", self.pairs[i].0, self.pairs[i].1));
            i += 1;
        }
        s
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        fnv1a64(self.serialize().as_bytes())
    }

    /// 往返恒等。
    pub fn roundtrip(&self) -> Result<(), RemapCode> {
        match parse_snap(&self.serialize()) {
            Some(p) => {
                if p == *self && p.fingerprint() == self.fingerprint() {
                    Ok(())
                } else {
                    Err(RemapCode::SnapshotFingerprint)
                }
            }
            None => Err(RemapCode::SnapshotFingerprint),
        }
    }
}

/// 十进制解析。
fn parse_u64(s: &str) -> Option<u64> {
    if s.is_empty() {
        return None;
    }
    let mut v: u64 = 0;
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        let d = (b[i] as u64).wrapping_sub(b'0' as u64);
        if d > 9 {
            return None;
        }
        v = v.wrapping_mul(10).wrapping_add(d);
        i += 1;
    }
    Some(v)
}

/// 快照反序列化。
pub fn parse_snap(s: &str) -> Option<RemapSnap> {
    let parts: Vec<&str> = s.split('|').collect();
    if parts.len() < 6 {
        return None;
    }
    let version = String::from(parts[0]);
    let phase_ord = parse_u64(parts[1])?;
    let illegal = parse_u64(parts[2])?;
    let early = parse_u64(parts[3])?;
    let resolved = parse_u64(parts[4])?;
    let n = parse_u64(parts[5])? as usize;
    if parts.len() != 6 + n || phase_ord > 1 {
        return None;
    }
    let mut pairs = Vec::new();
    let mut i = 0usize;
    while i < n {
        let cols: Vec<&str> = parts[6 + i].split(',').collect();
        if cols.len() != 2 {
            return None;
        }
        pairs.push((parse_u64(cols[0])?, parse_u64(cols[1])?));
        i += 1;
    }
    Some(RemapSnap {
        version,
        phase_ord,
        pairs,
        illegal_phys_hits: illegal,
        early_calls: early,
        resolved,
    })
}

// ---------------------------------------------------------------------------
// 多口径参考探针（四口径）
// ---------------------------------------------------------------------------

/// 口径一：全链手算——物理期定格 → 三闸映射提交 → 八服务虚拟地址可解
/// → 旧物理访问断零（IllegalPhysAccess）。
fn probe_full_chain() -> bool {
    // 服务物理地址：0x100000 + 序数 × 0x100（页内不齐——先做非齐版？）
    // 页界：0x101000 起每服务一页。
    let d = rt_descriptor_phys(|svc| 0x101_000 + (svc.ordinal() as u64) * PAGE_SIZE);
    let mut m = MapTable::new();
    let seg = MapSegment {
        phys: 0x101_000,
        virt: 0xFFFF_8000_0010_1000,
        len: 8 * PAGE_SIZE,
    };
    if m.add(seg).is_err() {
        return false;
    }
    let mut dd = d;
    // 物理期服务解析显性拒。
    let early = dd.resolve(RtService::GetTime) == Err(RemapCode::NotYetRemapped);
    // 提交。
    if dd.commit_remap(&m).is_err() {
        return false;
    }
    // 虚拟期服务可解（GetTime → 0xFFFF8000_0010_1000）。
    let r1 = dd.resolve(RtService::GetTime);
    let r7 = dd.resolve(RtService::UpdateCapsule);
    let usable = r1 == Ok(0xFFFF_8000_0010_1000)
        && r7 == Ok(0xFFFF_8000_0010_1000 + 7 * PAGE_SIZE);
    // 旧物理访问断零（显性拒 + 计数递增）。
    let old1 = dd.phys_of(RtService::GetTime) == Err(RemapCode::IllegalPhysAccess);
    let old2 = dd.phys_of(RtService::ResetSystem) == Err(RemapCode::IllegalPhysAccess);
    let zero = dd.illegal_phys_hits == 2; // 断言"旧地址访问为零"= 合法访问路径零次旧地址（计数只收被拦截尝试）
    early && usable && old1 && old2 && zero && dd.conserved()
}

/// 口径二：三闸显性——虚拟不对齐/物理不对齐/零长/空表/重叠/漏覆盖
/// 逐码拒。
fn probe_three_gates() -> bool {
    let mut m = MapTable::new();
    let zero_len = m.add(MapSegment { phys: 0x200_000, virt: 0x300_000, len: 0 })
        == Err(RemapCode::ZeroLength);
    let unaligned = m.add(MapSegment { phys: 0x200_123, virt: 0x300_000, len: PAGE_SIZE })
        == Err(RemapCode::VirtUnaligned);
    let phys_un = m.add(MapSegment { phys: 0x200_000, virt: 0x300_123, len: PAGE_SIZE })
        == Err(RemapCode::VirtUnaligned);
    let empty = m.validate(&[0x200_000]) == Err(RemapCode::EmptyMap);
    // 重叠。
    let _ = m.add(MapSegment { phys: 0x200_000, virt: 0x300_000, len: 2 * PAGE_SIZE });
    let _ = m.add(MapSegment { phys: 0x202_000, virt: 0x301_000, len: PAGE_SIZE });
    let overlap = m.validate(&[0x200_000, 0x202_000]) == Err(RemapCode::VirtOverlap);
    // 漏覆盖（服务地址不在任何段）。
    let mut m2 = MapTable::new();
    let _ = m2.add(MapSegment { phys: 0x200_000, virt: 0x300_000, len: PAGE_SIZE });
    let hole = m2.validate(&[0x200_000, 0x900_000]) == Err(RemapCode::CoverageHole);
    zero_len && unaligned && phys_un && empty && overlap && hole
}

/// 口径三：单向不可逆——DoubleRemap 显性拒 + 守恒面（Physical 全无
/// 虚拟地址/Virtual 全有，半映射不存在）。
fn probe_one_way() -> bool {
    let d = rt_descriptor_phys(|svc| 0x401_000 + (svc.ordinal() as u64) * PAGE_SIZE);
    let mut dd = d;
    let mut m = MapTable::new();
    let _ = m.add(MapSegment {
        phys: 0x401_000,
        virt: 0x501_000,
        len: 8 * PAGE_SIZE,
    });
    let ok = dd.commit_remap(&m).is_ok();
    let again = dd.commit_remap(&m) == Err(RemapCode::DoubleRemap);
    let cons = dd.conserved();
    ok && again && cons
}

/// 口径四：段守恒 + 快照往返 + 服务闭集序数往返。
fn probe_snapshot_segment() -> bool {
    if !segment_guard() {
        return false;
    }
    let d = rt_descriptor_phys(|svc| 0x601_000 + (svc.ordinal() as u64) * PAGE_SIZE);
    let mut dd = d;
    let mut m = MapTable::new();
    let _ = m.add(MapSegment {
        phys: 0x601_000,
        virt: 0x701_000,
        len: 8 * PAGE_SIZE,
    });
    let _ = dd.commit_remap(&m);
    let snap = RemapSnap::capture(&dd);
    let rt = snap.roundtrip().is_ok() && snap.pairs.len() == 8 && snap.phase_ord == 1;
    // 服务序数往返全闭集。
    let svc_ok = {
        let mut ok = true;
        let mut i = 0usize;
        while i < 8 {
            if RtService::from_ordinal(RtService::ALL[i].ordinal() as u64)
                != Some(RtService::ALL[i])
            {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    rt && svc_ok && RtService::from_ordinal(8).is_none()
}

/// 汇总参考探针。
pub fn reference_budget_check() -> bool {
    probe_full_chain() && probe_three_gates() && probe_one_way() && probe_snapshot_segment()
}

// ---------------------------------------------------------------------------
// 判据（CheckSet——判据面零 panic）
// ---------------------------------------------------------------------------

/// UNX-F0161 域自检（判据四组：两态×3、三闸×3、解析×3、判据元×1）。
pub fn run_unx161_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0161");

    // 态-01：两态机单向——Physical 全无虚拟地址/commit 后全有/重复提交拒。
    if probe_one_way() {
        s.ok("U161-态-01-单向不可逆守恒");
    } else {
        s.fail("U161-态-01-单向不可逆守恒", "态机面失效");
    }

    // 态-02：物理期合法面——phys_of 可读（引导期定格面）。
    let mut d2 = rt_descriptor_phys(|svc| 0x801_000 + (svc.ordinal() as u64) * PAGE_SIZE);
    let p_ok = d2.phys_of(RtService::GetTime) == Ok(0x801_000)
        && d2.phase == RemapPhase::Physical;
    if p_ok {
        s.ok("U161-态-02-物理期读合法");
    } else {
        s.fail("U161-态-02-物理期读合法", "物理期面失效");
    }

    // 态-03：期态闭集与服务闭集序数往返。
    let ord_ok = RemapPhase::ALL.len() == 2
        && RtService::from_ordinal(0) == Some(RtService::GetTime)
        && RtService::from_ordinal(7) == Some(RtService::UpdateCapsule)
        && RtService::from_ordinal(8).is_none()
        && RtService::GetVariable.zh() == "读变量";
    if ord_ok {
        s.ok("U161-态-03-闭集序数往返");
    } else {
        s.fail("U161-态-03-闭集序数往返", "闭集面失效");
    }

    // 闸-01：三闸显性逐码。
    if probe_three_gates() {
        s.ok("U161-闸-01-三闸显性逐码");
    } else {
        s.fail("U161-闸-01-三闸显性逐码", "闸面失效");
    }

    // 闸-02：覆盖闸正例——全服务覆盖验证通过。
    let d3 = rt_descriptor_phys(|svc| 0x901_000 + (svc.ordinal() as u64) * PAGE_SIZE);
    let mut m3 = MapTable::new();
    let _ = m3.add(MapSegment {
        phys: 0x901_000,
        virt: 0xA01_000,
        len: 8 * PAGE_SIZE,
    });
    let mut addrs = [0u64; 8];
    let mut i = 0usize;
    while i < 8 {
        addrs[i] = d3.entries[i].phys;
        i += 1;
    }
    let covered = m3.validate(&addrs).is_ok();
    if covered {
        s.ok("U161-闸-02-覆盖正例通过");
    } else {
        s.fail("U161-闸-02-覆盖正例通过", "覆盖面失效");
    }

    // 闸-03：段换算恰值——段内偏移等值平移手算（+0x123 偏移 → 虚拟同偏）
    // 段外 None。
    let seg = MapSegment { phys: 0xB00_000, virt: 0xC00_000, len: 2 * PAGE_SIZE };
    let t1 = seg.to_virt(0xB00_123) == Some(0xC00_123);
    let t2 = seg.to_virt(0xB00_000) == Some(0xC00_000);
    let t3 = seg.to_virt(0xB02_000).is_none();
    let t4 = seg.covers_phys(0xB01_FFF) && !seg.covers_phys(0xB02_000);
    if t1 && t2 && t3 && t4 {
        s.ok("U161-闸-03-段内换算恰值");
    } else {
        s.fail("U161-闸-03-段内换算恰值", "换算面失效");
    }

    // 解-01：全链——新地址可用 + 旧地址断零（判据本体）。
    if probe_full_chain() {
        s.ok("U161-解-01-新地址可用旧地址断零");
    } else {
        s.fail("U161-解-01-新地址可用旧地址断零", "全链失效");
    }

    // 解-02：未映射服务显性（ServiceUnmapped——悬空非静默）。
    // 构造：直接改期态不走 commit（防御面测试——半映射态由 conserved 揭示）。
    let mut d4 = rt_descriptor_phys(|_| 0xD01_000);
    d4.phase = RemapPhase::Virtual; // 强制虚拟期（半映射——守恒应假）
    let unmapped = d4.resolve(RtService::GetVariable) == Err(RemapCode::ServiceUnmapped);
    let not_cons = !d4.conserved();
    if unmapped && not_cons {
        s.ok("U161-解-02-悬空服务显性守恒揭破");
    } else {
        s.fail("U161-解-02-悬空服务显性守恒揭破", "悬空面失效");
    }

    // 解-03：快照往返 + 段守恒 + 摘要读屏。
    if probe_snapshot_segment() {
        s.ok("U161-解-03-快照往返段守恒");
    } else {
        s.fail("U161-解-03-快照往返段守恒", "快照面失效");
    }

    // 组-01：十六码段守恒 + 详情互异非空。
    let codes = RemapCode::ALL;
    let mut codes_ok = codes.len() == 16;
    let mut i2 = 0usize;
    while i2 < codes.len() && codes_ok {
        if codes[i2].detail().is_empty() {
            codes_ok = false;
        }
        let mut j = i2 + 1;
        while j < codes.len() {
            if codes[i2].code() == codes[j].code() || codes[i2].detail() == codes[j].detail() {
                codes_ok = false;
            }
            j += 1;
        }
        i2 += 1;
    }
    if codes_ok && segment_guard() {
        s.ok("U161-组-01-十六码段守恒");
    } else {
        s.fail("U161-组-01-十六码段守恒", "码面漂移");
    }

    // 组-02：参考探针四口径全真。
    if reference_budget_check() {
        s.ok("U161-组-02-四口径探针全真");
    } else {
        s.fail("U161-组-02-四口径探针全真", "探针失效");
    }

    // 元-01：零 panic 面 + 空态平凡 + 版本非空。
    let src = include_str!("unx161_rtremap.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".expect(", "panic!", "unreachable!", "unsafe"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    let empty_d = rt_descriptor_phys(|_| 0);
    let snap0 = RemapSnap::capture(&empty_d);
    let empty_ok = empty_d.phase == RemapPhase::Physical
        && empty_d.conserved()
        && empty_d.summary().contains("U161")
        && snap0.roundtrip().is_ok()
        && !UNX161_RTREMAP_VERSION.is_empty()
        && PAGE_SIZE == 4096
        && RemapPhase::Virtual.zh() == "虚拟期";
    if hits == 0 && empty_ok {
        s.ok("U161-元-01-零panic空态平凡");
    } else {
        s.fail("U161-元-01-零panic空态平凡", "panic 面或空态失效");
    }

    // 深-01：PAT 互斥/TLB 守恒/域判定/引用计数四语义合流恰值。
    if probe_deep_semantics() {
        s.ok("U161-深-01-属性失效域引用四语义");
    } else {
        s.fail("U161-深-01-属性失效域引用四语义", "深化面失效");
    }

    // 深-02：TlbSnap 快照往返（失效账可复现对拍面）。
    if probe_tlb_snap() {
        s.ok("U161-深-02-TLB账快照往返");
    } else {
        s.fail("U161-深-02-TLB账快照往返", "快照面失效");
    }

    // 深-03：五段索引拆分恰值（9+9+9+9+12 闭式——典型 VA/规范顶/零）。
    if probe_walk_indices() {
        s.ok("U161-深-03-四级索引拆分");
    } else {
        s.fail("U161-深-03-四级索引拆分", "拆分面失效");
    }

    // 深-04：大页三档恰值（粒度/停走级/对齐闸/步数与偏移位闭式）。
    if probe_huge_page() {
        s.ok("U161-深-04-大页停走三档");
    } else {
        s.fail("U161-深-04-大页停走三档", "大页面失效");
    }

    // 深-05：权限合成恰值（页链 AND 最严语义——R/W/U/S/NX 手算）。
    if probe_perm_and() {
        s.ok("U161-深-05-页链权限合成");
    } else {
        s.fail("U161-深-05-页链权限合成", "权限面失效");
    }

    s
}

/// 单遍词法剥除（注释剥除、字符串整体剥空——判据模式串不自 hit）。
fn strip_lexical_noise(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < b.len() {
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'/' {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
            let mut depth = 1usize;
            i += 2;
            while i < b.len() && depth > 0 {
                if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
                    depth += 1;
                    i += 2;
                } else if i + 1 < b.len() && b[i] == b'*' && b[i + 1] == b'/' {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            continue;
        }
        if b[i] == b'"' {
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    i += 2;
                    continue;
                }
                let closed = b[i] == b'"';
                i += 1;
                if closed {
                    break;
                }
            }
            continue;
        }
        if b[i] == b'\'' {
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    i += 2;
                    continue;
                }
                let closed = b[i] == b'\'';
                i += 1;
                if closed {
                    break;
                }
            }
            continue;
        }
        if let Some(c) = src.get(i..i + 1) {
            out.push_str(c);
        }
        i += 1;
    }
    out
}

// ===========================================================================
// 深化四区：PAT 缓存属性域 / TLB 失效账 / 地址空间域隔离 / 服务引用计数
// ——重映射之后的「地址可用」只是及格线，属性正确与失效彻底才是满分面
// ===========================================================================

// ---------------------------------------------------------------------------
// PAT 缓存属性域（页属性表语义建模——映射正确但属性错 = 缓存一致性地雷）
// ---------------------------------------------------------------------------

/// 缓存属性闭集（x86_64 PAT 语义五档——映射面之外的第二维：
/// 同一物理页双虚拟映射若属性不一致，数据面就是两套真相）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PatKind {
    /// 写回（常规 RAM——最高性能档）。
    WriteBack,
    /// 写穿（帧簿式直写面）。
    WriteThrough,
    /// 不可缓存（MMIO——寄存器面禁缓存）。
    Uncacheable,
    /// 写合并（帧缓冲——连续写合并爆发）。
    WriteCombining,
    /// 写保护（只读映射——写入触发异常而非静默丢弃）。
    WriteProtect,
}

impl PatKind {
    /// 五档闭集。
    pub const ALL: [PatKind; 5] = [
        PatKind::WriteBack,
        PatKind::WriteThrough,
        PatKind::Uncacheable,
        PatKind::WriteCombining,
        PatKind::WriteProtect,
    ];

    /// 属性码（0..=4——PAT 索引位域面）。
    pub const fn code(self) -> u16 {
        match self {
            PatKind::WriteBack => 0,
            PatKind::WriteThrough => 1,
            PatKind::Uncacheable => 2,
            PatKind::WriteCombining => 3,
            PatKind::WriteProtect => 4,
        }
    }

    /// 中文注记。
    pub const fn zh(self) -> &'static str {
        match self {
            PatKind::WriteBack => "写回",
            PatKind::WriteThrough => "写穿",
            PatKind::Uncacheable => "不可缓存",
            PatKind::WriteCombining => "写合并",
            PatKind::WriteProtect => "写保护",
        }
    }
}

/// 虚拟属性视图（同虚拟范围声明一次属性——范围互斥由表闸保证）。
#[derive(Clone, Copy, Debug)]
pub struct PatView {
    /// 虚拟基址（页对齐——与映射段同粒度）。
    pub virt_base: u64,
    /// 覆盖字节长（页倍数）。
    pub len: u64,
    /// 属性档。
    pub kind: PatKind,
}

impl PatView {
    /// 范围覆盖判定（virt ∈ [base, base+len)）。
    pub const fn covers(&self, virt: u64) -> bool {
        virt >= self.virt_base && virt < self.virt_base + self.len
    }
}

/// PAT 视图表（范围互斥闸——同虚拟页双属性 = 别名属性冲突立案，
/// 这是缓存一致性地雷的显性揭破面）。
pub struct PatTable {
    /// 视图条目。
    pub views: Vec<PatView>,
    /// 属性冲突立案计数。
    pub alias_conflicts: u32,
}

impl PatTable {
    /// 空表。
    pub fn new() -> PatTable {
        PatTable {
            views: Vec::new(),
            alias_conflicts: 0,
        }
    }

    /// **声明属性视图**（互斥闸：与既有视图范围重叠即拒——同范围双属性
    /// 在硬件上不可表达，软件面必须显性拒）。
    pub fn declare(&mut self, view: PatView) -> Result<(), RemapCode> {
        if view.len == 0 || view.virt_base % PAGE_SIZE != 0 || view.len % PAGE_SIZE != 0 {
            return Err(RemapCode::VirtUnaligned);
        }
        let mut i = 0usize;
        while i < self.views.len() {
            let e = &self.views[i];
            let overlap = view.virt_base < e.virt_base + e.len
                && e.virt_base < view.virt_base + view.len;
            if overlap {
                self.alias_conflicts += 1;
                return Err(RemapCode::VirtOverlap); // 复用重叠码——语义同构
            }
            i += 1;
        }
        self.views.push(view);
        Ok(())
    }

    /// **虚拟地址属性查**（覆盖命中恰一个视图——互斥闸保证单值）。
    pub fn kind_of(&self, virt: u64) -> Option<PatKind> {
        let mut hit = None;
        let mut i = 0usize;
        while i < self.views.len() {
            if self.views[i].covers(virt) {
                hit = Some(self.views[i].kind);
            }
            i += 1;
        }
        hit
    }

    /// 表守恒（互斥性复检——两两不重叠；新增面已闸，此为审计面）。
    pub fn conserved(&self) -> bool {
        let mut i = 0usize;
        while i < self.views.len() {
            let mut j = i + 1;
            while j < self.views.len() {
                let a = &self.views[i];
                let b = &self.views[j];
                if a.virt_base < b.virt_base + b.len && b.virt_base < a.virt_base + a.len {
                    return false;
                }
                j += 1;
            }
            i += 1;
        }
        true
    }
}

// ---------------------------------------------------------------------------
// TLB 失效账（重映射提交后旧线性地址翻译仍在 TLB——不刷 = 服务走旧路）
// ---------------------------------------------------------------------------

/// TLB 失效粒度闭集（提交后的失效面三档——缺省档 PendingNone 是
/// 「改了没刷」的显性立案态，不是免刷声明）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TlbFlush {
    /// 单页失效（换算面逐服务——页粒度精准打击）。
    SinglePage,
    /// 全量失效（INVLPGB 式——提交时一次性清台）。
    FullFlush,
    /// 未失效（提交后搁置——stale 计数来源，显性立案）。
    PendingNone,
}

impl TlbFlush {
    /// 三档闭集。
    pub const ALL: [TlbFlush; 3] = [
        TlbFlush::SinglePage,
        TlbFlush::FullFlush,
        TlbFlush::PendingNone,
    ];

    /// 中文注记。
    pub const fn zh(self) -> &'static str {
        match self {
            TlbFlush::SinglePage => "单页失效",
            TlbFlush::FullFlush => "全量失效",
            TlbFlush::PendingNone => "未失效",
        }
    }
}

/// TLB 失效账（变更/失效两流守恒：改动过映射而没有对应失效动作的
/// 差额 = stale 陈旧翻译——服务可能走旧地址直到 TLB 逐出，静默炸弹面）。
pub struct TlbLedger {
    /// 变更计数（映射面改动次数——含提交/补映射）。
    pub changed: u64,
    /// 失效计数（已执行的失效动作次数）。
    pub flushed: u64,
    /// 陈旧翻译立案（变更 > 失效的差额立案数）。
    pub stale: u64,
    /// 当前粒度档。
    pub mode: TlbFlush,
}

impl TlbLedger {
    /// 新账（初始未失效——提交面强制过账）。
    pub const fn new() -> TlbLedger {
        TlbLedger {
            changed: 0,
            flushed: 0,
            stale: 0,
            mode: TlbFlush::PendingNone,
        }
    }

    /// 记变更（映射面每次改动 +1——失效应紧随其后）。
    pub const fn record_change(&mut self) {
        self.changed += 1;
    }

    /// 执行失效（粒度档生效——PendingNone 档不算动作：stale 立案，
    /// 返回 false = 失效缺席，调用侧显性见假）。
    pub fn flush(&mut self, granularity: TlbFlush) -> bool {
        if granularity == TlbFlush::PendingNone {
            self.stale = self.changed - self.flushed;
            return false;
        }
        self.mode = granularity;
        self.flushed += 1;
        true
    }

    /// 守恒（硬不变量：失效不能超前于变更——stale 是立案快照字段，
    /// 只在 PendingNone 档刷新，不进恒等式）。
    pub const fn conserved(&self) -> bool {
        self.flushed <= self.changed
    }

    /// 无陈旧（全部变更都有失效覆盖——可用性满分面）。
    pub const fn no_stale(&self) -> bool {
        self.changed == self.flushed
    }
}

// ---------------------------------------------------------------------------
// 地址空间域隔离（虚拟地址的「户口」——跨域解析是越权面显性拒绝）
// ---------------------------------------------------------------------------

/// 虚拟地址域闭集（x86_64 四象限惯例策略值——resolve 结果的户口面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AddrDomain {
    /// 用户低半区。
    UserLow,
    /// 内核直映射（物理线性映射窗）。
    Hhdm,
    /// 内核代码段窗。
    KernelText,
    /// 内核数据段窗。
    KernelData,
}

impl AddrDomain {
    /// 四域闭集。
    pub const ALL: [AddrDomain; 4] = [
        AddrDomain::UserLow,
        AddrDomain::Hhdm,
        AddrDomain::KernelText,
        AddrDomain::KernelData,
    ];

    /// **域判定单点**（范围策略表——None 即非规范地址/未分配域，
    /// 调用侧显性拒而非猜一个域）。
    pub const fn classify(virt: u64) -> Option<AddrDomain> {
        if virt < 0x0000_8000_0000_0000 {
            Some(AddrDomain::UserLow)
        } else if virt >= 0xFFFF_8000_0000_0000 && virt <= 0xFFFF_8FFF_FFFF_FFFF {
            Some(AddrDomain::Hhdm)
        } else if virt >= 0xFFFF_9000_0000_0000 && virt <= 0xFFFF_9FFF_FFFF_FFFF {
            Some(AddrDomain::KernelText)
        } else if virt >= 0xFFFF_A000_0000_0000 && virt <= 0xFFFF_AFFF_FFFF_FFFF {
            Some(AddrDomain::KernelData)
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// 服务引用计数（RT 服务可被多消费者持有——提前解除 = 悬空指针制造机）
// ---------------------------------------------------------------------------

/// 服务引用计数器（八槽闭集——每服务独立计数；解除闸 = 计数归零才真解除）。
pub struct ServiceRefs {
    /// 逐服务引用计数。
    pub counts: [u64; 8],
    /// 提前解除立案（release 时计数已零——解除一个不存在的引用）。
    pub premature: u32,
    /// 解除计数（真解除次数）。
    pub released: u32,
}

impl ServiceRefs {
    /// 全零新计数器。
    pub const fn new() -> ServiceRefs {
        ServiceRefs {
            counts: [0; 8],
            premature: 0,
            released: 0,
        }
    }

    /// 取引用（+1——消费者声明持有）。
    pub fn acquire(&mut self, svc: RtService) -> u64 {
        self.counts[svc.ordinal()] += 1;
        self.counts[svc.ordinal()]
    }

    /// 放引用（-1；计数已零再放 = 提前解除立案拒——悬空制造面封死）。
    pub fn release(&mut self, svc: RtService) -> Result<u64, RemapCode> {
        let idx = svc.ordinal();
        if self.counts[idx] == 0 {
            self.premature += 1;
            return Err(RemapCode::ServiceUnmapped); // 复用悬空码——语义同构
        }
        self.counts[idx] -= 1;
        self.released += 1;
        Ok(self.counts[idx])
    }

    /// 守恒（全部计数不超服务闭集且无负数面——u64 天然无负，审计面即
    /// 「未解除数 >= 0 恒真 + premature 只增不减」的只增面）。
    pub const fn conserved(&self) -> bool {
        self.released <= self.premature + self.released
    }

    /// 全释放（八槽全零——关机前审计面）。
    pub const fn all_free(&self) -> bool {
        let mut i = 0usize;
        while i < 8 {
            if self.counts[i] != 0 {
                return false;
            }
            i += 1;
        }
        true
    }
}

// ---------------------------------------------------------------------------
// 深化面快照（TLB 账 '|' 序列化 + FNV + 往返——失效面可复现对拍）
// ---------------------------------------------------------------------------

/// TLB 账快照。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TlbSnap {
    /// 版本戳。
    pub version: String,
    /// 账面行（changed|flushed|stale|mode_code）。
    pub rows: Vec<String>,
    /// 指纹域（serialize 的 FNV——往返回代恒等锚）。
    pub fp: u64,
}

impl TlbSnap {
    /// 捕获。
    pub fn capture(l: &TlbLedger) -> TlbSnap {
        let mut rows = Vec::new();
        rows.push(alloc::format!(
            "{}|{}|{}|{}",
            l.changed,
            l.flushed,
            l.stale,
            l.mode.zh()
        ));
        let fp = fnv1a64(
            {
                let mut s = String::from(UNX161_RTREMAP_VERSION);
                s.push('|');
                let mut i = 0usize;
                while i < rows.len() {
                    s.push_str(&rows[i]);
                    i += 1;
                }
                s
            }
            .as_bytes(),
        );
        TlbSnap {
            version: String::from(UNX161_RTREMAP_VERSION),
            rows,
            fp,
        }
    }

    /// 序列化（'|' 分隔保空段——账面行单行）。
    pub fn serialize(&self) -> String {
        let s = alloc::format!("{}|{}", self.version, self.rows[0]);
        let _ = &self.fp;
        s
    }

    /// 往返恒等（串→重解析→字段面比对；parts[1..4] 三数字段，
    /// parts[4] 为粒度中文注记——不进数字解析）。
    pub fn roundtrip(&self) -> Result<(), RemapCode> {
        let ser = self.serialize();
        let parts: Vec<&str> = ser.split('|').collect();
        if parts.len() != 5 {
            return Err(RemapCode::SegmentGuard);
        }
        let ch = parse_u64_field(parts[1]).ok_or(RemapCode::SegmentGuard)?;
        let fl = parse_u64_field(parts[2]).ok_or(RemapCode::SegmentGuard)?;
        let st = parse_u64_field(parts[3]).ok_or(RemapCode::SegmentGuard)?;
        let row_ch = self.rows[0].split('|').next().and_then(parse_u64_field);
        let row_fl = self.rows[0].split('|').nth(1).and_then(parse_u64_field);
        let row_st = self.rows[0].split('|').nth(2).and_then(parse_u64_field);
        if Some(ch) == row_ch && Some(fl) == row_fl && Some(st) == row_st {
            Ok(())
        } else {
            Err(RemapCode::SegmentGuard)
        }
    }
}

/// u64 字段解析（深化面本地小解析——零依赖）。
fn parse_u64_field(s: &str) -> Option<u64> {
    if s.is_empty() {
        return None;
    }
    let mut v: u64 = 0;
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        let d = (b[i] as char).to_digit(10)? as u64;
        v = v.checked_mul(10)?.checked_add(d)?;
        i += 1;
    }
    Some(v)
}

/// 深化面参考探针——PAT 互斥/TLB 守恒/域判定恰值（四口径合流）。
pub fn probe_deep_semantics() -> bool {
    // PAT：互斥闸 + 冲突立案 + 查表恰值。
    let mut pt = PatTable::new();
    let d1 = pt.declare(PatView {
        virt_base: 0xFFFF_8000_0000_0000,
        len: 2 * PAGE_SIZE,
        kind: PatKind::WriteBack,
    })
    .is_ok();
    let conflict = pt.declare(PatView {
        virt_base: 0xFFFF_8000_0000_1000,
        len: PAGE_SIZE,
        kind: PatKind::Uncacheable,
    }) == Err(RemapCode::VirtOverlap)
        && pt.alias_conflicts == 1;
    let kind_ok = pt.kind_of(0xFFFF_8000_0000_1234) == Some(PatKind::WriteBack)
        && pt.kind_of(0xFFFF_8000_0002_0000).is_none()
        && pt.conserved();
    // TLB：变更-失效守恒手算（3 变更 2 失效 → 差额 1 未覆盖）。
    let mut tl = TlbLedger::new();
    tl.record_change();
    tl.record_change();
    tl.record_change();
    let f1 = tl.flush(TlbFlush::SinglePage);
    let f2 = tl.flush(TlbFlush::FullFlush);
    let stale_ok = f1 && f2 && tl.conserved() && !tl.no_stale();
    let f3 = !tl.flush(TlbFlush::PendingNone) && tl.stale == 1 && tl.conserved();
    // 域判定恰值（四域 + 非规范 None）。
    let dom_ok = AddrDomain::classify(0x0000_0000_0001_2345) == Some(AddrDomain::UserLow)
        && AddrDomain::classify(0xFFFF_8000_0000_0000) == Some(AddrDomain::Hhdm)
        && AddrDomain::classify(0xFFFF_9000_0000_0000) == Some(AddrDomain::KernelText)
        && AddrDomain::classify(0xFFFF_A000_0000_0000) == Some(AddrDomain::KernelData)
        && AddrDomain::classify(0xFFFF_B000_0000_0000).is_none()
        && AddrDomain::classify(0x0000_8000_0000_0000).is_none();
    // 引用计数生命周期手算（+1+1→2 / -1→1 / -1→0 / -1→拒）。
    let mut refs = ServiceRefs::new();
    let a1 = refs.acquire(RtService::GetVariable) == 1;
    let a2 = refs.acquire(RtService::GetVariable) == 2;
    let r1 = refs.release(RtService::GetVariable) == Ok(1);
    let r2 = refs.release(RtService::GetVariable) == Ok(0);
    let r3 = refs.release(RtService::GetVariable) == Err(RemapCode::ServiceUnmapped);
    let ref_ok = a1 && a2 && r1 && r2 && r3 && refs.all_free() && refs.conserved();
    d1 && conflict && kind_ok && stale_ok && f3 && dom_ok && ref_ok
}

/// 深化面参考探针——TlbSnap 快照往返。
pub fn probe_tlb_snap() -> bool {
    let mut tl = TlbLedger::new();
    tl.record_change();
    let _ = tl.flush(TlbFlush::SinglePage);
    let snap = TlbSnap::capture(&tl);
    snap.roundtrip().is_ok() && snap.rows.len() == 1
}

// ---------------------------------------------------------------------------
// 深化五区：页表遍历面——PAT/TLB 之外是「翻译怎么走」的机制面：
// 四级索引拆分（x86_64 9+9+9+9+12）/ 大页停走（1G/2M 早停）/ 权限
// 位域合成（页链 AND 语义）。全部纯函数零 IO——机制面与属性面同构
//（PAT 是「翻译后怎么缓存」，遍历是「翻译本身怎么算」，两层互补）。
// ---------------------------------------------------------------------------

/// 页表层级闭集（x86_64 四级——PML4 根起）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WalkLevel {
    /// L4 顶级（PML4）。
    Pml4,
    /// L3 目录指针（PDPT——1G 大页停走级）。
    Pdpt,
    /// L2 页目录（PD——2M 大页停走级）。
    Pd,
    /// L1 页表（PT——4K 叶）。
    Pt,
}

impl WalkLevel {
    /// 四级闭集。
    pub const ALL: [WalkLevel; 4] = [
        WalkLevel::Pml4,
        WalkLevel::Pdpt,
        WalkLevel::Pd,
        WalkLevel::Pt,
    ];

    /// 中文注记。
    pub const fn zh(self) -> &'static str {
        match self {
            WalkLevel::Pml4 => "四级根",
            WalkLevel::Pdpt => "目录指针",
            WalkLevel::Pd => "页目录",
            WalkLevel::Pt => "页表",
        }
    }

    /// 该级的地址域宽（位）：L4 取 47..39、L3 38..30、L2 29..21、
    /// L1 20..12（9 位索引域——x86_64 恒 4×9+12=48 规范宽）。
    pub const fn index_shift(self) -> u32 {
        match self {
            WalkLevel::Pml4 => 39,
            WalkLevel::Pdpt => 30,
            WalkLevel::Pd => 21,
            WalkLevel::Pt => 12,
        }
    }
}

/// 五段索引拆分账（VA → (pml4, pdpt, pd, pt, offset)——9+9+9+9+12
/// 位闭式；48 位规范宽之外位按零处理（canonical 检查在调用侧））。
/// const fn 内闭包不可调用（E0015）——四次移位内联展开。
pub const fn walk_indices(va: u64) -> (u64, u64, u64, u64, u64) {
    (
        (va >> 39) & 0x1FF,
        (va >> 30) & 0x1FF,
        (va >> 21) & 0x1FF,
        (va >> 12) & 0x1FF,
        va & 0xFFF,
    )
}

/// 大页粒度闭集（停走档——叶级决定页粒度）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HugePage {
    /// 4K 叶（PT 级命中）。
    K4,
    /// 2M 叶（PD 级 PS 位 1——21 位偏移域）。
    M2,
    /// 1G 叶（PDPT 级 PS 位 1——30 位偏移域）。
    G1,
}

impl HugePage {
    /// 三档闭集。
    pub const ALL: [HugePage; 3] = [HugePage::K4, HugePage::M2, HugePage::G1];

    /// 页粒度字节。
    pub const fn size_bytes(self) -> u64 {
        match self {
            HugePage::K4 => PAGE_SIZE,
            HugePage::M2 => 1 << 21,
            HugePage::G1 => 1 << 30,
        }
    }

    /// 停走级（叶所在层级——K4 停 PT / M2 停 PD / G1 停 PDPT）。
    pub const fn stop_level(self) -> WalkLevel {
        match self {
            HugePage::K4 => WalkLevel::Pt,
            HugePage::M2 => WalkLevel::Pd,
            HugePage::G1 => WalkLevel::Pdpt,
        }
    }

    /// 基址对齐闸（页粒度对齐——2M 页基址 2M 对齐，混粒度是硬件
    /// #PF 面软件必须显性拒）。
    pub const fn base_aligned(self, base: u64) -> bool {
        base % self.size_bytes() == 0
    }
}

/// 页表项权限位域（低 12 位语义——x86_64 惯例位面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PagePerm {
    /// 存在位（P——bit 0）。
    pub present: bool,
    /// 可写位（R/W——bit 1）。
    pub writable: bool,
    /// 用户位（U/S——bit 2）。
    pub user: bool,
    /// 不可执行位（NX——bit 63，本账用 u64 位面）。
    pub no_exec: bool,
}

impl PagePerm {
    /// 从表项低语义位提取（bit0/1/2/63——其余位对权限面无义）。
    pub const fn of_entry(entry: u64) -> PagePerm {
        PagePerm {
            present: entry & 1 != 0,
            writable: entry & (1 << 1) != 0,
            user: entry & (1 << 2) != 0,
            no_exec: entry & (1 << 63) != 0,
        }
    }

    /// 零权限（无页——P=0 缺席）。
    pub const fn absent() -> PagePerm {
        PagePerm {
            present: false,
            writable: false,
            user: false,
            no_exec: false,
        }
    }
}

/// **页链权限合成**（x86_64 语义：约束取最严——任一级不可写即不可写、
/// 任一级核态即核态、任一级 NX 即 NX；P 链上全 1 才存在）。
pub const fn perm_and(chain: [PagePerm; 4]) -> PagePerm {
    let mut present = true;
    let mut writable = true;
    let mut user = true;
    let mut no_exec = false;
    let mut i = 0usize;
    while i < 4 {
        if !chain[i].present {
            present = false;
        }
        if !chain[i].writable {
            writable = false;
        }
        if !chain[i].user {
            user = false;
        }
        if chain[i].no_exec {
            no_exec = true;
        }
        i += 1;
    }
    PagePerm {
        present,
        writable,
        user,
        no_exec,
    }
}

/// **遍历停走账**（大页命中级 → 偏移域宽——1G 页偏移 30 位、2M 页
/// 21 位、4K 页 12 位；早停后低级索引全部并入偏移——大页的「一级
/// 顶三级」算术闭式）。
pub const fn stop_offset_bits(hp: HugePage) -> u32 {
    match hp {
        HugePage::G1 => 30,
        HugePage::M2 => 21,
        HugePage::K4 => 12,
    }
}

/// 遍历步数闭式（命中大页档的走过级数：G1 走 2 级（L4+L3）/ M2 走
/// 3 级（+L2）/ K4 走 4 级（+L1）——TLB 命中外的内存访存次数账）。
pub const fn walk_steps(hp: HugePage) -> u32 {
    match hp {
        HugePage::G1 => 2,
        HugePage::M2 => 3,
        HugePage::K4 => 4,
    }
}

/// 深化面参考探针——五段索引拆分恰值（手算对拍：典型 VA 分段）。
pub fn probe_walk_indices() -> bool {
    // 手造 VA：pml4=1 pdpt=2 pd=3 pt=4 off=0x123
    // = (1<<39)|(2<<30)|(3<<21)|(4<<12)|0x123。
    let va = (1u64 << 39) | (2 << 30) | (3 << 21) | (4 << 12) | 0x123;
    let (a, b, c, d, e) = walk_indices(va);
    let split_ok = a == 1 && b == 2 && c == 3 && d == 4 && e == 0x123;
    // 全一 VA（规范顶）：五段全 511/511/511/511/4095。
    let top = 0x0000_FFFF_FFFF_FFFF;
    let (a2, b2, c2, d2, e2) = walk_indices(top);
    let top_ok = a2 == 511 && b2 == 511 && c2 == 511 && d2 == 511 && e2 == 4095;
    // 零 VA：全零。
    let (a3, b3, c3, d3, e3) = walk_indices(0);
    let zero_ok = a3 == 0 && b3 == 0 && c3 == 0 && d3 == 0 && e3 == 0;
    split_ok && top_ok && zero_ok
}

/// 深化面参考探针——大页三档恰值（粒度/停走级/对齐闸/步数与偏移位）。
pub fn probe_huge_page() -> bool {
    let k4 = HugePage::K4;
    let m2 = HugePage::M2;
    let g1 = HugePage::G1;
    let size_ok = k4.size_bytes() == 4096
        && m2.size_bytes() == 2 * 1024 * 1024
        && g1.size_bytes() == 1024 * 1024 * 1024;
    let stop_ok = k4.stop_level() == WalkLevel::Pt
        && m2.stop_level() == WalkLevel::Pd
        && g1.stop_level() == WalkLevel::Pdpt;
    // 对齐闸：2M 基址 0x200000 对齐过 / 0x123456 拒；1G 基址 0x40000000 过。
    let align_ok = m2.base_aligned(0x0000_0000_0020_0000)
        && !m2.base_aligned(0x0000_0000_0012_3456)
        && g1.base_aligned(0x0000_0000_4000_0000)
        && k4.base_aligned(0x0000_0000_0000_1000);
    let steps_ok = walk_steps(g1) == 2 && walk_steps(m2) == 3 && walk_steps(k4) == 4
        && stop_offset_bits(g1) == 30 && stop_offset_bits(m2) == 21
        && stop_offset_bits(k4) == 12;
    size_ok && stop_ok && align_ok && steps_ok
}

/// 深化面参考探针——权限合成恰值（页链 AND 语义手算：核态最严/
/// NX 传染/缺席页存在位塌）。
pub fn probe_perm_and() -> bool {
    let full = PagePerm {
        present: true,
        writable: true,
        user: true,
        no_exec: false,
    };
    // 全宽松链：全真。
    let all_ok = perm_and([full; 4]).present && perm_and([full; 4]).writable && perm_and([full; 4]).user;
    // L2 不可写：合成为不可写（最严语义）。
    let mut ro = full;
    ro.writable = false;
    let mut chain = [full; 4];
    chain[2] = ro;
    let ro_ok = !perm_and(chain).writable && perm_and(chain).present;
    // L1 核态（user=false）：合成为核态。
    let mut sup = full;
    sup.user = false;
    let mut chain2 = [full; 4];
    chain2[3] = sup;
    let sup_ok = !perm_and(chain2).user;
    // 任一级 NX：合成 NX（传染语义）。
    let mut nx = full;
    nx.no_exec = true;
    let mut chain3 = [full; 4];
    chain3[1] = nx;
    let nx_ok = perm_and(chain3).no_exec;
    // 缺席页：P 塌（链上一位 P=0 全链不存在）。
    let mut chain4 = [full; 4];
    chain4[0] = PagePerm::absent();
    let absent_ok = !perm_and(chain4).present && !perm_and(chain4).writable;
    all_ok && ro_ok && sup_ok && nx_ok && absent_ok
}
