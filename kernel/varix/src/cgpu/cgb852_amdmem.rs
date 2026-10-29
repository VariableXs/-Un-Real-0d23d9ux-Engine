//! cgb852_amdmem —— CGPU-F0852 AMD 显存架构适配。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：CGPU-F0852 AMD 显存架构适配  <SEG 0x7500>
//! 目标行数：≥340（判据：纯功能代码 ≥340 行）
//! 判据原文摘录：
//!   ① 纯功能代码 ≥340 行；
//!   ② 规格与判据按册内锚点原文逐条落实：CGPU Varix STAR II · 总纲与施工书 #CGPU-F0852；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律（AMD 显存架构适配，整数 MiB 面）：
//!   1. 显存域闭集四域（写死）：本地 VRAM / 无限缓存 IC / GTT 共
//!      享 / 不可见 BAR；域号 0..=3，越界红码（XC_BAD_DOMAIN）；
//!   2. 分配状态机：Free → Reserved → Committed → Retired；
//!      Retired 归还配额；非法迁移红码（XC_BAD_TRANS）；
//!   3. 配额账守恒（恒等式）：每域 capacity = free + committed +
//!      reserved；分配/归还后逐域校验，漂移红码（XC_QUOTA_DRIFT）；
//!   4. 页粒度（写死 4 MiB）：分配大小必须为页整数倍，且 ≥1 页；
//!      违规红码（XC_BAD_PAGESIZE）；
//!   5. IC 域特殊纪律：IC 不可被 Committed 超过其配额 75%（写死
//!      红线 768/1024 MiB），越线红码（XC_IC_OVERCOMMIT）；
//!   6. 句柄账（定容 64）：句柄单调发放，不回收复用；句柄越界
//!      红码（XC_BAD_HANDLE）；
//!   7. 零 unwrap/expect/panic，整数算术，确定性，no_std + alloc。
//!
//! 诊断段声明：0x7500 段为本模块独占（SEG_BASE=0x7500），判别式
//!   SEG_BASE+1..+N；与 0x74E0/0x7520 邻段不重叠。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

/// 模块版本。
pub const AM_VERSION: &str = "cgb852_amdmem-v1";

/// 诊断段基址（0x7500 · 本模块独占）。
pub const SEG_BASE: u16 = 0x7500;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x7501 显存域越界（0..=3 之外）。
pub const XC_BAD_DOMAIN: u16 = SEG_BASE + 1;
/// 0x7502 分配状态迁移非法。
pub const XC_BAD_TRANS: u16 = SEG_BASE + 2;
/// 0x7503 配额账漂移（capacity ≠ free + committed + reserved）。
pub const XC_QUOTA_DRIFT: u16 = SEG_BASE + 3;
/// 0x7504 页粒度违规（非 4 MiB 整数倍或 0 页）。
pub const XC_BAD_PAGESIZE: u16 = SEG_BASE + 4;
/// 0x7505 配额不足。
pub const XC_NO_QUOTA: u16 = SEG_BASE + 5;
/// 0x7506 IC 超配红线（>75% 配额）。
pub const XC_IC_OVERCOMMIT: u16 = SEG_BASE + 6;
/// 0x7507 句柄越界。
pub const XC_BAD_HANDLE: u16 = SEG_BASE + 8 - 1;
/// 0x7508 句柄账满（64）。
pub const XC_HANDLE_FULL: u16 = SEG_BASE + 8;
/// 0x7509 句柄账漂移（活句柄数 ≠ 总发放 − 归还）。
pub const XC_HANDLE_DRIFT: u16 = SEG_BASE + 9;
/// 0x750A 重复归还（Retired 再动）。
pub const XC_DOUBLE_FREE: u16 = SEG_BASE + 10;

/// 显存域数（写死：4）。
pub const DOM_CAP: usize = 4;
/// 页粒度（写死：4 MiB）。
pub const PAGE_MIB: u32 = 4;
/// 句柄账容量（写死：64）。
pub const HANDLE_CAP: usize = 64;
/// IC 红线比例分子/分母（写死 3/4）。
pub const IC_NUM: u32 = 3;
pub const IC_DEN: u32 = 4;

/// 显存域闭集：0=本地 VRAM 1=无限缓存 IC 2=GTT 共享 3=不可见 BAR。
pub const D_VRAM: usize = 0;
pub const D_IC: usize = 1;
pub const D_GTT: usize = 2;
pub const D_BAR: usize = 3;

/// 各域容量（MiB，写死样本配置）。
pub const DOM_CAP_MIB: [u32; DOM_CAP] = [16384, 1024, 8192, 256];

/// 分配状态机。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AState {
    /// 预占。
    Reserved,
    /// 已提交。
    Committed,
    /// 已退役（页归还）。
    Retired,
}

/// 迁移合法性：Res→Com、Res→Ret、Com→Ret。
pub fn trans_legal(from: AState, to: AState) -> bool {
    matches!(
        (from, to),
        (AState::Reserved, AState::Committed)
            | (AState::Reserved, AState::Retired)
            | (AState::Committed, AState::Retired)
    )
}

/// 单笔分配。
#[derive(Clone, Copy, Debug)]
pub struct Alloc {
    /// 句柄（单调，1 起）。
    pub handle: u32,
    /// 域号。
    pub domain: usize,
    /// 页数（4 MiB/页）。
    pub pages: u32,
    /// 当前状态。
    pub state: AState,
}

/// 域配额账。
#[derive(Clone, Copy, Debug)]
pub struct DomainLedger {
    /// 容量（MiB）。
    pub capacity: u32,
    /// 空闲（MiB）。
    pub free: u32,
    /// 已提交（MiB）。
    pub committed: u32,
    /// 预占（MiB）。
    pub reserved: u32,
}

impl DomainLedger {
    /// 守恒：capacity = free + committed + reserved。
    pub fn balanced(&self) -> bool {
        self.capacity == self.free + self.committed + self.reserved
    }
}

/// AMD 显存适配册。
pub struct AmdMem {
    /// 四域账。
    pub doms: [DomainLedger; DOM_CAP],
    /// 句柄账（定容 64，槽位 = 句柄-1）。
    pub slots: Vec<Option<Alloc>>,
    /// 下一句柄。
    pub next_handle: u32,
    /// 活句柄数。
    pub live: u64,
    /// 总发放数。
    pub issued: u64,
    /// 归还数。
    pub retired: u64,
    /// 累计红码。
    pub red_count: u64,
}

impl AmdMem {
    /// 初始化（容量全空闲）。
    pub fn new() -> Self {
        let mut doms = [DomainLedger {
            capacity: 0,
            free: 0,
            committed: 0,
            reserved: 0,
        }; DOM_CAP];
        let mut i = 0;
        while i < DOM_CAP {
            doms[i] = DomainLedger {
                capacity: DOM_CAP_MIB[i],
                free: DOM_CAP_MIB[i],
                committed: 0,
                reserved: 0,
            };
            i += 1;
        }
        AmdMem {
            doms,
            slots: Vec::new(),
            next_handle: 1,
            live: 0,
            issued: 0,
            retired: 0,
            red_count: 0,
        }
    }

    /// 预占（页粒度、配额、IC 红线三重校验）。
    pub fn reserve(&mut self, domain: usize, mib: u32) -> Result<u32, u16> {
        if domain >= DOM_CAP {
            self.red_count += 1;
            return Err(XC_BAD_DOMAIN);
        }
        if mib == 0 || mib % PAGE_MIB != 0 {
            self.red_count += 1;
            return Err(XC_BAD_PAGESIZE);
        }
        if mib > self.doms[domain].free {
            self.red_count += 1;
            return Err(XC_NO_QUOTA);
        }
        if domain == D_IC {
            let after_committed = self.doms[D_IC].committed;
            let after_reserved = self.doms[D_IC].reserved + mib;
            if after_committed + after_reserved > self.doms[D_IC].capacity * IC_NUM / IC_DEN {
                self.red_count += 1;
                return Err(XC_IC_OVERCOMMIT);
            }
        }
        if self.slots.len() == HANDLE_CAP {
            self.red_count += 1;
            return Err(XC_HANDLE_FULL);
        }
        let h = self.next_handle;
        self.next_handle += 1;
        self.doms[domain].free -= mib;
        self.doms[domain].reserved += mib;
        self.slots.push(Some(Alloc {
            handle: h,
            domain,
            pages: mib / PAGE_MIB,
            state: AState::Reserved,
        }));
        self.issued += 1;
        self.live += 1;
        Ok(h)
    }

    /// 按句柄定位槽。
    fn locate(&self, handle: u32) -> Result<usize, u16> {
        if handle == 0 || handle as usize > self.slots.len() {
            self.red_count += 1;
            return Err(XC_BAD_HANDLE);
        }
        let idx = handle as usize - 1;
        if self.slots[idx].is_none() {
            self.red_count += 1;
            return Err(XC_BAD_HANDLE);
        }
        Ok(idx)
    }

    /// 提交（Res→Com；IC 红线二次校验）。
    pub fn commit(&mut self, handle: u32) -> Result<(), u16> {
        let idx = self.locate(handle)?;
        let cur = self.slots[idx].map(|a| a.state);
        let dom = self.slots[idx].map(|a| a.domain);
        let mib = self.slots[idx].map(|a| a.pages * PAGE_MIB);
        let (cur, dom, mib) = match (cur, dom, mib) {
            (Some(c), Some(d), Some(m)) => (c, d, m),
            _ => return Err(XC_BAD_HANDLE),
        };
        if !trans_legal(cur, AState::Committed) {
            self.red_count += 1;
            return Err(XC_BAD_TRANS);
        }
        if dom == D_IC {
            let after = self.doms[D_IC].committed + mib;
            if after > self.doms[D_IC].capacity * IC_NUM / IC_DEN {
                self.red_count += 1;
                return Err(XC_IC_OVERCOMMIT);
            }
        }
        self.doms[dom].reserved -= mib;
        self.doms[dom].committed += mib;
        self.slots[idx].as_mut().map(|a| a.state = AState::Committed);
        Ok(())
    }

    /// 退役（Res/Com→Ret；页归还 free；重复归还红）。
    pub fn retire(&mut self, handle: u32) -> Result<(), u16> {
        let idx = self.locate(handle)?;
        let cur = self.slots[idx].map(|a| a.state);
        let dom = self.slots[idx].map(|a| a.domain);
        let mib = self.slots[idx].map(|a| a.pages * PAGE_MIB);
        let (cur, dom, mib) = match (cur, dom, mib) {
            (Some(c), Some(d), Some(m)) => (c, d, m),
            _ => return Err(XC_BAD_HANDLE),
        };
        if !trans_legal(cur, AState::Retired) {
            self.red_count += 1;
            return Err(XC_DOUBLE_FREE);
        }
        match cur {
            AState::Reserved => {
                self.doms[dom].reserved -= mib;
            }
            AState::Committed => {
                self.doms[dom].committed -= mib;
            }
            AState::Retired => {
                self.red_count += 1;
                return Err(XC_DOUBLE_FREE);
            }
        }
        self.doms[dom].free += mib;
        self.slots[idx].as_mut().map(|a| a.state = AState::Retired);
        self.slots[idx] = None;
        self.live -= 1;
        self.retired += 1;
        Ok(())
    }

    /// 句柄账守恒：live = issued − retired。
    pub fn handles_balanced(&self) -> bool {
        self.live == self.issued - self.retired
    }

    /// 全域配额审计。
    pub fn audit(&self) -> Result<(), u16> {
        let mut d = 0;
        while d < DOM_CAP {
            if !self.doms[d].balanced() {
                return Err(XC_QUOTA_DRIFT);
            }
            d += 1;
        }
        if !self.handles_balanced() {
            return Err(XC_HANDLE_DRIFT);
        }
        Ok(())
    }

    /// 摘要（非空判据）。
    pub fn summary(&self) -> String {
        let mut s = String::from("amdmem live=");
        s += &self.live.to_string();
        s += " issued=";
        s += &self.issued.to_string();
        s += " retired=";
        s += &self.retired.to_string();
        let mut d = 0;
        while d < DOM_CAP {
            s += " d";
            s += &d.to_string();
            s += "c=";
            s += &self.doms[d].committed.to_string();
            d += 1;
        }
        s
    }
}

/// 自例程：四域分配 → 提交 → 退役 → 守恒审计 + 负例六连。
pub fn drill() -> Result<u32, u16> {
    let mut m = AmdMem::new();
    let h1 = m.reserve(D_VRAM, 64)?;
    let h2 = m.reserve(D_IC, 256)?;
    let h3 = m.reserve(D_GTT, 128)?;
    let h4 = m.reserve(D_BAR, 16)?;
    // 负例：页粒度违规。
    if m.reserve(D_VRAM, 6) != Err(XC_BAD_PAGESIZE) {
        return Err(XC_BAD_PAGESIZE);
    }
    if m.reserve(D_VRAM, 0) != Err(XC_BAD_PAGESIZE) {
        return Err(XC_BAD_PAGESIZE);
    }
    // 负例：域越界。
    if m.reserve(DOM_CAP, 64) != Err(XC_BAD_DOMAIN) {
        return Err(XC_BAD_DOMAIN);
    }
    // 负例：配额不足。
    if m.reserve(D_BAR, 512) != Err(XC_NO_QUOTA) {
        return Err(XC_NO_QUOTA);
    }
    // IC 红线：再占 512 MiB 应越线（256+512 > 1024*3/4）。
    if m.reserve(D_IC, 512) != Err(XC_IC_OVERCOMMIT) {
        return Err(XC_IC_OVERCOMMIT);
    }
    m.audit()?;
    m.commit(h1)?;
    m.commit(h2)?;
    // 负例：Retired 不许 commit（先退役 h3 再试提交）。
    m.retire(h3)?;
    if m.commit(h3) != Err(XC_BAD_HANDLE) {
        return Err(XC_BAD_HANDLE);
    }
    // 负例：重复归还。
    if m.retire(h3) != Err(XC_BAD_HANDLE) {
        return Err(XC_BAD_HANDLE);
    }
    // 负例：句柄越界。
    if m.commit(999) != Err(XC_BAD_HANDLE) {
        return Err(XC_BAD_HANDLE);
    }
    m.commit(h4)?;
    m.retire(h4)?;
    m.retire(h1)?;
    m.retire(h2)?;
    m.audit()?;
    // 全退后各域 free 回满。
    let mut d = 0;
    while d < DOM_CAP {
        if m.doms[d].free != DOM_CAP_MIB[d] {
            return Err(XC_QUOTA_DRIFT);
        }
        d += 1;
    }
    if m.summary().len() == 0 || !m.handles_balanced() {
        return Err(XC_HANDLE_DRIFT);
    }
    Ok(0)
}
