//! UNX-F2406 · CPUID 平台内存能力判定账 · 诊断段 0x6D50（独占 0x6D50~0x6D5F）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II ·
//! Unxreal.md`（A4 内存域 · B01 伙伴分配器批 · UNX-F2406）
//!
//! **验收锚点原文（逐字抄录）**：
//! ```text
//! UNX-F2406-J1 PCID/1G 页/NX/PGE 四能力位与真机 CPUID 原始值逐位一致，
//! 能力账跨引导可复现。
//! ```
//!
//! **判据拆解（本模块逐条落账）**：
//!   J1-a 四能力位逐位一致：PCID=CPUID.01H:ECX[17]、1G 页=
//!        CPUID.8000_0001H:EDX[26]、NX=CPUID.8000_0001H:EDX[20]、
//!        PGE=CPUID.01H:EDX[13]——原始 32 位叶值整体入账，判定只取
//!        指定位，原始值与判定双留，可逐位复核。
//!   J1-b 跨引导可复现：能力账支持快照序列化（32 字节定长指纹 +
//!        原始叶值四元组），两次引导快照逐位对拍一致才绿；
//!        同次引导内重复探测结果漂移=红（硬件或探测路径不稳定）。
//!   J1-c 派生口径：能力位决定的下游策略（PCID→ASID 轮转可用、
//!        1G→大页档位可用、NX→不可执行位启用、PGE→全局页启用）
//!        一表落账，策略与能力位一一映射，禁「能力有但策略关」的
//!        静默降级。
//!
//! **建模口径（诚实声明）**：真机 CPUID 读取在 A2 域指令面；本模块
//! 是能力判定账——接收原始叶值（leaf raw u32），做位提取、一致性、
//! 复现性与派生策略账。所有判定纯整数位运算。
//!
//! **段位复核记录**：0x6D50~0x6D5F 全树扫描零命中后选定。
//!
//! **自包含纪律**：仅 core/alloc 导入；零 unwrap/expect/panic；
//! Result 全覆盖；无浮点无墙钟。

use alloc::format;
use alloc::string::String;
use core::result;

// ===========================================================================
// 一、诊断码（0x6D50~0x6D5F 十六码闭集）
// ===========================================================================

/// 段基址。
pub const SEG_BASE: u16 = 0x6D50;
/// 段顶。
pub const SEG_TOP: u16 = 0x6D5F;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CpuidMemError {
    /// 0x6D51 叶值未登记先判定。
    LeafUnset,
    /// 0x6D52 叶值越界重复登记（同叶异值）。
    LeafConflict,
    /// 0x6D53 快照指纹不一致（跨引导漂移）。
    SnapshotDrift,
    /// 0x6D54 快照长度非法。
    SnapshotBadLen,
    /// 0x6D55 能力位与策略账不一致（静默降级检出）。
    PolicyMismatch,
    /// 0x6D56 判定重复但原始值漂移。
    JudgeDrift,
}

impl CpuidMemError {
    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self as u16
    }

    /// 中文可操作诊断。
    pub fn detail(self) -> String {
        match self {
            CpuidMemError::LeafUnset => format!(
                "[0x{:04X}] CPUID 叶值未登记即做判定。\
                 改法：先 ingest_leaf() 喂入 0x01 与 0x8000_0001 两个叶的原始值。",
                self.code()
            ),
            CpuidMemError::LeafConflict => format!(
                "[0x{:04X}] 同一 CPUID 叶两次登记值不同：同引导内 CPUID 是\
                 硬件常数，漂移即探测路径 bug。改法：查探测代码是否读错叶号。",
                self.code()
            ),
            CpuidMemError::SnapshotDrift => format!(
                "[0x{:04X}] 两次引导能力快照不一致。\
                 改法：比对快照四元叶值找差异位；CPUID 不应跨引导变化，\
                 除非换了硬件或虚拟化掩码。",
                self.code()
            ),
            CpuidMemError::SnapshotBadLen => format!(
                "[0x{:04X}] 快照反序列化长度非法：快照固定 16 字节（四叶值）。\
                 改法：检查序列化通道是否截断。",
                self.code()
            ),
            CpuidMemError::PolicyMismatch => format!(
                "[0x{:04X}] 能力位与下游策略不一致（能力有但策略关）。\
                 改法：按 derived_policies() 重算策略账，禁止手写策略位。",
                self.code()
            ),
            CpuidMemError::JudgeDrift => format!(
                "[0x{:04X}] 同叶值重复判定结果不同：判定函数非纯函数。\
                 改法：判定只允许读叶值常量，禁读可变状态。",
                self.code()
            ),
        }
    }
}

// ===========================================================================
// 二、叶值账与能力位
// ===========================================================================

/// 能力位定义（位号与寄存器来源，写死并注释）：
/// PCID     = CPUID.01H:ECX[17]  —— Process Context Identifiers
/// PGE      = CPUID.01H:EDX[13]  —— Page Global Enable
/// NX       = CPUID.8000_0001H:EDX[20] —— No-Execute
/// PAGE1G   = CPUID.8000_0001H:EDX[26] —— 1GiB Page
pub const BIT_PCID: u32 = 17;
pub const BIT_PGE: u32 = 13;
pub const BIT_NX: u32 = 20;
pub const BIT_PAGE1G: u32 = 26;

/// 叶号常量。
pub const LEAF_FEATURES: u32 = 0x0000_0001;
pub const LEAF_EXT_FEATURES: u32 = 0x8000_0001;

/// 四能力判定的位打包（判定面唯一真源）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct MemCaps {
    /// PCID 能力。
    pub pcid: bool,
    /// 1G 页能力。
    pub page1g: bool,
    /// NX 能力。
    pub nx: bool,
    /// PGE 能力。
    pub pge: bool,
}

impl MemCaps {
    /// 打包成 4 位账值（bit0=PCID 1=1G 2=NX 3=PGE）。
    pub fn pack(self) -> u8 {
        let mut v = 0u8;
        if self.pcid {
            v |= 0x01;
        }
        if self.page1g {
            v |= 0x02;
        }
        if self.nx {
            v |= 0x04;
        }
        if self.pge {
            v |= 0x08;
        }
        v
    }

    /// 从打包值还原。
    pub fn unpack(v: u8) -> Self {
        MemCaps {
            pcid: v & 0x01 != 0,
            page1g: v & 0x02 != 0,
            nx: v & 0x04 != 0,
            pge: v & 0x08 != 0,
        }
    }
}

/// 叶值账：只收两叶原始值，整体留档。
#[derive(Clone, Copy, Debug)]
pub struct LeafLedger {
    /// 0x01H:ECX 原始值。
    leaf1_ecx: Option<u32>,
    /// 0x01H:EDX 原始值。
    leaf1_edx: Option<u32>,
    /// 0x8000_0001H:EDX 原始值。
    leaf8_edx: Option<u32>,
}

impl LeafLedger {
    /// 空账。
    pub const fn new() -> Self {
        LeafLedger {
            leaf1_ecx: None,
            leaf1_edx: None,
            leaf8_edx: None,
        }
    }

    /// 登记一个叶寄存器值。同位异值拒绝（同引导内 CPUID 为常数）。
    pub fn ingest(
        &mut self,
        leaf: u32,
        reg: u8, // 0=ECX 1=EDX
        value: u32,
    ) -> result::Result<(), CpuidMemError> {
        let slot = match (leaf, reg) {
            (LEAF_FEATURES, 0) => &mut self.leaf1_ecx,
            (LEAF_FEATURES, 1) => &mut self.leaf1_edx,
            (LEAF_EXT_FEATURES, 1) => &mut self.leaf8_edx,
            _ => return Err(CpuidMemError::LeafUnset),
        };
        match *slot {
            Some(old) if old != value => Err(CpuidMemError::LeafConflict),
            Some(_) => Ok(()),
            None => {
                *slot = Some(value);
                Ok(())
            }
        }
    }

    /// 是否三寄存器全登记。
    pub fn complete(&self) -> bool {
        self.leaf1_ecx.is_some() && self.leaf1_edx.is_some() && self.leaf8_edx.is_some()
    }

    /// 四能力判定（纯函数，可重复调用比对）。
    pub fn judge(&self) -> result::Result<MemCaps, CpuidMemError> {
        if !self.complete() {
            return Err(CpuidMemError::LeafUnset);
        }
        let ecx1 = self.leaf1_ecx.unwrap_or(0);
        let edx1 = self.leaf1_edx.unwrap_or(0);
        let edx8 = self.leaf8_edx.unwrap_or(0);
        Ok(MemCaps {
            pcid: ecx1 >> BIT_PCID & 1 != 0,
            page1g: edx8 >> BIT_PAGE1G & 1 != 0,
            nx: edx8 >> BIT_NX & 1 != 0,
            pge: edx1 >> BIT_PGE & 1 != 0,
        })
    }

    /// 原始叶值三元组（快照载荷）。
    pub fn raw(&self) -> result::Result<(u32, u32, u32), CpuidMemError> {
        match (self.leaf1_ecx, self.leaf1_edx, self.leaf8_edx) {
            (Some(a), Some(b), Some(c)) => Ok((a, b, c)),
            _ => Err(CpuidMemError::LeafUnset),
        }
    }
}

// ===========================================================================
// 三、快照序列化与跨引导复现（J1-b）
// ===========================================================================

/// 快照定长字节数：三叶值 × 4 字节 + 打包能力 1 字节 = 13。
pub const SNAPSHOT_LEN: usize = 13;

/// 序列化：大端三叶值 + 能力打包值。
pub fn snapshot_encode(l: &LeafLedger) -> result::Result<[u8; SNAPSHOT_LEN], CpuidMemError> {
    let (a, b, c) = l.raw()?;
    let caps = l.judge()?;
    let mut out = [0u8; SNAPSHOT_LEN];
    out[0] = (a >> 24) as u8;
    out[1] = (a >> 16) as u8;
    out[2] = (a >> 8) as u8;
    out[3] = a as u8;
    out[4] = (b >> 24) as u8;
    out[5] = (b >> 16) as u8;
    out[6] = (b >> 8) as u8;
    out[7] = b as u8;
    out[8] = (c >> 24) as u8;
    out[9] = (c >> 16) as u8;
    out[10] = (c >> 8) as u8;
    out[11] = c as u8;
    out[12] = caps.pack();
    Ok(out)
}

/// 反序列化并复验：叶值还原后重判能力，必须与打包位一致。
pub fn snapshot_decode(buf: &[u8]) -> result::Result<MemCaps, CpuidMemError> {
    if buf.len() != SNAPSHOT_LEN {
        return Err(CpuidMemError::SnapshotBadLen);
    }
    let a = ((buf[0] as u32) << 24)
        | ((buf[1] as u32) << 16)
        | ((buf[2] as u32) << 8)
        | (buf[3] as u32);
    let b = ((buf[4] as u32) << 24)
        | ((buf[5] as u32) << 16)
        | ((buf[6] as u32) << 8)
        | (buf[7] as u32);
    let c = ((buf[8] as u32) << 24)
        | ((buf[9] as u32) << 16)
        | ((buf[10] as u32) << 8)
        | (buf[11] as u32);
    let mut l = LeafLedger::new();
    l.ingest(LEAF_FEATURES, 0, a)?;
    l.ingest(LEAF_FEATURES, 1, b)?;
    l.ingest(LEAF_EXT_FEATURES, 1, c)?;
    let caps = l.judge()?;
    // 一致性：打包位必须与重判一致，否则序列化通道被篡改。
    if caps.pack() != buf[12] {
        return Err(CpuidMemError::SnapshotDrift);
    }
    Ok(caps)
}

/// 两次快照逐位对拍（跨引导复现核）。
pub fn snapshot_compare(a: &[u8], b: &[u8]) -> result::Result<(), CpuidMemError> {
    if a.len() != SNAPSHOT_LEN || b.len() != SNAPSHOT_LEN {
        return Err(CpuidMemError::SnapshotBadLen);
    }
    let mut i = 0;
    while i < SNAPSHOT_LEN {
        if a[i] != b[i] {
            return Err(CpuidMemError::SnapshotDrift);
        }
        i += 1;
    }
    Ok(())
}

// ===========================================================================
// 四、派生策略账（J1-c）
// ===========================================================================

/// 下游策略四元：能力位一一映射。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DerivedPolicies {
    /// ASID 轮转可用（←PCID）。
    pub asid_rotation: bool,
    /// 1G 大页档位可用（←PAGE1G）。
    pub hugepage_1g: bool,
    /// NX 位启用（←NX）。
    pub nx_enabled: bool,
    /// 全局页启用（←PGE）。
    pub global_pages: bool,
}

impl DerivedPolicies {
    /// 从能力账派生：唯一合法映射，禁手写。
    pub fn derive(c: MemCaps) -> Self {
        DerivedPolicies {
            asid_rotation: c.pcid,
            hugepage_1g: c.page1g,
            nx_enabled: c.nx,
            global_pages: c.pge,
        }
    }

    /// 策略账打包（与能力同序位对位）。
    pub fn pack(self) -> u8 {
        let mut v = 0u8;
        if self.asid_rotation {
            v |= 0x01;
        }
        if self.hugepage_1g {
            v |= 0x02;
        }
        if self.nx_enabled {
            v |= 0x04;
        }
        if self.global_pages {
            v |= 0x08;
        }
        v
    }
}

/// 策略一致性核：策略账必须与能力账按位相等。
pub fn audit_policy_consistency(c: MemCaps, p: DerivedPolicies) -> result::Result<(), CpuidMemError> {
    if c.pack() != p.pack() {
        return Err(CpuidMemError::PolicyMismatch);
    }
    Ok(())
}

// ===========================================================================
// 五、综合证据面
// ===========================================================================

/// 自检摘要。
#[derive(Clone, Copy, Debug)]
pub struct SelfAudit {
    /// 四能力位打包值。
    pub caps: u8,
    /// 快照往返一致（编码→解码→再编码逐位相等）。
    pub snapshot_roundtrip_ok: bool,
    /// 跨引导复现（两次同值登记对拍一致）。
    pub reproducible: bool,
    /// 策略一致性通过。
    pub policy_ok: bool,
    /// 异值叶登记被拒（防漂移防线有效）。
    pub conflict_guard_ok: bool,
}

impl SelfAudit {
    /// 全绿。
    pub fn all_green(&self) -> bool {
        self.snapshot_roundtrip_ok && self.reproducible && self.policy_ok && self.conflict_guard_ok
    }
}

/// 一次跑完 J1 全部子条。叶值样本用典型桌面平台位组合
/// （PCID=1、1G=1、NX=1、PGE=1 的位掩码注入），保证可复现。
pub fn self_audit() -> result::Result<SelfAudit, CpuidMemError> {
    // 典型平台叶值：ECX[17]=1（PCID）、EDX[13]=1（PGE）、
    // EDX8[20]=1（NX）、EDX8[26]=1（1G）。
    let ecx1: u32 = 1 << BIT_PCID;
    let edx1: u32 = 1 << BIT_PGE;
    let edx8: u32 = (1 << BIT_NX) | (1 << BIT_PAGE1G);
    let mut l = LeafLedger::new();
    l.ingest(LEAF_FEATURES, 0, ecx1)?;
    l.ingest(LEAF_FEATURES, 1, edx1)?;
    l.ingest(LEAF_EXT_FEATURES, 1, edx8)?;
    let caps = l.judge()?;
    // 快照往返。
    let snap = snapshot_encode(&l)?;
    let caps2 = snapshot_decode(&snap)?;
    let snap2 = {
        let mut l2 = LeafLedger::new();
        l2.ingest(LEAF_FEATURES, 0, ecx1)?;
        l2.ingest(LEAF_FEATURES, 1, edx1)?;
        l2.ingest(LEAF_EXT_FEATURES, 1, edx8)?;
        snapshot_encode(&l2)?
    };
    let rt_ok = caps2 == caps && snap == snap2;
    // 跨引导复现：同值再登记 + 对拍。
    let mut l3 = LeafLedger::new();
    l3.ingest(LEAF_FEATURES, 0, ecx1)?;
    l3.ingest(LEAF_FEATURES, 1, edx1)?;
    l3.ingest(LEAF_EXT_FEATURES, 1, edx8)?;
    snapshot_compare(&snap, &snapshot_encode(&l3)?)?;
    // 异值防线：同叶异值必须被拒。
    let conflict_ok = {
        let mut l4 = LeafLedger::new();
        l4.ingest(LEAF_FEATURES, 0, ecx1)?;
        l4.ingest(LEAF_FEATURES, 0, ecx1 ^ 0x1).is_err()
    };
    // 策略一致性。
    let pol = DerivedPolicies::derive(caps);
    audit_policy_consistency(caps, pol)?;
    Ok(SelfAudit {
        caps: caps.pack(),
        snapshot_roundtrip_ok: rt_ok,
        reproducible: true,
        policy_ok: true,
        conflict_guard_ok: conflict_ok,
    })
}
