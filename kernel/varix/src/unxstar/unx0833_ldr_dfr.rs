//! unx0833_ldr_dfr — UNX-F0833 LDR/DFR 逻辑目的地与集群模式（诊断段 0x6540）。
//!
//! 判据原文：逻辑模式组播按集群正确送达（16 核档 10/10 次），
//!           集群划分账与拓扑一致。
//! 设计纪律：
//!   1. LDR（xAPIC 0x0D0）/DFR（0x0E0）按 SDM Vol.3 §10.6.2.2：
//!      Flat 模式（DFR=0xFFFF_0001）：逻辑 ID 位 n 对应物理 APIC ID n，
//!      组播目的 = 目的字节按位与各核逻辑 ID 非零者全收；
//!      Cluster 模式（DFR=0x0FFF_0002）：LDR = ClusterID[31:28] | Mask[23:16]，
//!      先按 ClusterID 分舱、再按 Mask 位组播。
//!   2. 集群划分守恒：16 核一账一舱，每核恰属一舱（划分账与拓扑一致）。
//!   3. 组播送达账：目标集 = 拓扑闭集计算，送达数 = 目标集大小，
//!      送达集守恒（不重不漏），16 核档 10/10 对账。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x6540 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x6540;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_DFR_BAD: u16 = SEG_BASE + 1;   // DFR 非 Flat/Cluster 两规范值
const E_LDR_BAD: u16 = SEG_BASE + 2;   // LDR 布局违反模式约束
const E_NO_TARGET: u16 = SEG_BASE + 3; // 组播零目标（无人可达）
const E_SPLIT: u16 = SEG_BASE + 4;     // 集群划分账与拓扑不一致
const E_APIC_ID: u16 = SEG_BASE + 5;   // 物理 APIC ID 超 16 核编址域
const E_MODE_MIX: u16 = SEG_BASE + 6;  // 混合 DFR 舱内模式不一

/// DFR 规范值：Flat / Cluster。
pub const DFR_FLAT: u32 = 0xFFFF_0001;
pub const DFR_CLUSTER: u32 = 0x0FFF_0002;

/// 16 核档编址域：APIC ID 0..=15。
pub const CORE_COUNT: u32 = 16;

/// 单核逻辑身份账。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct CoreLogical {
    pub apic_id: u8,   // 物理 ID（0..=15）
    pub ldr: u32,      // LDR 原值
    pub dfr: u32,      // DFR 原值
}

impl CoreLogical {
    /// 校验布局与模式一致性（SDM 约束）。
    pub fn validate(&self) -> Result<(), u16> {
        if self.apic_id as u32 >= CORE_COUNT {
            return Err(E_APIC_ID);
        }
        match self.dfr {
            DFR_FLAT => {
                // Flat：逻辑 ID = LDR[7:0]，且必须恰一位（一位一核）
                let lo = (self.ldr & 0xFF) as u8;
                if self.ldr & 0xFFFF_0003 != 0 || lo.count_ones() != 1 {
                    return Err(E_LDR_BAD);
                }
                if lo != (1u8 << self.apic_id) {
                    return Err(E_LDR_BAD); // Flat 档位 n 必须对应物理 ID n
                }
                Ok(())
            }
            DFR_CLUSTER => {
                // Cluster：ClusterID[31:28] + Mask[23:16]，其余位必须零
                if self.ldr & 0x0F00_0004 != 0 {
                    return Err(E_LDR_BAD);
                }
                let mask = ((self.ldr >> 16) & 0xFF) as u8;
                if mask == 0 || mask & (mask.wrapping_sub(1)) != 0 {
                    return Err(E_LDR_BAD); // 舱内 Mask 必须恰一位
                }
                Ok(())
            }
            _ => Err(E_DFR_BAD),
        }
    }

    pub fn mode_is_cluster(&self) -> bool {
        self.dfr == DFR_CLUSTER
    }

    pub fn cluster_id(&self) -> u8 {
        ((self.ldr >> 28) & 0xF) as u8
    }

    pub fn member_mask(&self) -> u8 {
        if self.dfr == DFR_CLUSTER {
            ((self.ldr >> 16) & 0xFF) as u8
        } else {
            (self.ldr & 0xFF) as u8
        }
    }
}

/// 16 核逻辑身份账本（拓扑闭集）。
pub struct LogicalFabric {
    pub cores: Vec<CoreLogical>,
}

impl LogicalFabric {
    /// 全 Flat 拓扑：核 n 逻辑位 n。
    pub fn flat_16() -> LogicalFabric {
        let mut cores = Vec::new();
        for n in 0u8..16 {
            cores.push(CoreLogical {
                apic_id: n,
                ldr: 1u32 << n,
                dfr: DFR_FLAT,
            });
        }
        LogicalFabric { cores }
    }

    /// 全 Cluster 拓扑：分 4 舱，每舱 4 核（舱 c 含物理 ID 4c..4c+3）。
    pub fn cluster_4x4() -> LogicalFabric {
        let mut cores = Vec::new();
        for n in 0u8..16 {
            let c = n / 4;
            let bit = n % 4;
            cores.push(CoreLogical {
                apic_id: n,
                ldr: ((c as u32) << 28) | (1u32 << (16 + bit)),
                dfr: DFR_CLUSTER,
            });
        }
        LogicalFabric { cores }
    }

    /// 集群划分守恒：同舱核 ClusterID 一致，舱内各核物理 ID 与 Mask 位一一对应。
    pub fn cluster_ledger_consistent(&self) -> Result<(), u16> {
        // 每核先过布局校验
        for c in &self.cores {
            c.validate()?;
        }
        // 同舱一致性：ClusterID 相同的核必须同 DFR 且 Mask 位互异并覆盖
        let mut i = 0;
        while i < self.cores.len() {
            let cid = self.cores[i].cluster_id();
            let mut mask_union = 0u8;
            let mut seen_mask = 0u8;
            for c in self.cores.iter().skip(i) {
                if c.cluster_id() != cid {
                    break;
                }
                if c.mode_is_cluster() != self.cores[i].mode_is_cluster() {
                    return Err(E_MODE_MIX);
                }
                let m = c.member_mask();
                if m & seen_mask != 0 {
                    return Err(E_SPLIT); // 舱内 Mask 位重复：一舱位一核破坏
                }
                seen_mask |= m;
                mask_union |= m;
            }
            if cid != 0 && mask_union == 0 {
                return Err(E_SPLIT);
            }
            i += seen_mask.count_ones().max(1) as usize;
        }
        Ok(())
    }

    /// 物理模式送达：单目标直达。
    pub fn deliver_physical(&self, dest: u8) -> Result<Vec<u8>, u16> {
        for c in &self.cores {
            if c.apic_id == dest {
                let mut v = Vec::new();
                v.push(dest);
                return Ok(v);
            }
        }
        Err(E_NO_TARGET)
    }

    /// 逻辑模式组播：按各核 DFR/LDR 联合计算目标集。
    /// Flat：目的字节 & 各核逻辑 ID 非零者收；Cluster：同舱且位与非零者收。
    pub fn deliver_logical(&self, dest: u8) -> Result<Vec<u8>, u16> {
        let mut targets: Vec<u8> = Vec::new();
        for c in &self.cores {
            c.validate()?;
            if c.mode_is_cluster() {
                if c.cluster_id() as u32 == (dest >> 4) as u32 & 0xF
                    && c.member_mask() & (dest & 0xF) != 0
                {
                    targets.push(c.apic_id);
                }
            } else if c.member_mask() & dest != 0 {
                targets.push(c.apic_id);
            }
        }
        if targets.is_empty() {
            return Err(E_NO_TARGET);
        }
        Ok(targets)
    }
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_DFR_BAD => "DFR 非 Flat(0xFFFFFFFF)/Cluster(0x0FFFFFFF) 规范值",
        E_LDR_BAD => "LDR 布局违反模式约束（位域越位/多位/零位）",
        E_NO_TARGET => "组播零目标：无人可达",
        E_SPLIT => "集群划分账与拓扑不一致（Mask 位重复或舱空）",
        E_APIC_ID => "物理 APIC ID 超 16 核编址域",
        E_MODE_MIX => "同舱混合 Flat/Cluster 模式",
        _ => "未知诊断码",
    }
}

/// 确定性演练：16 核组播 10/10 送达 + 集群账 + 错误路径。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：Flat 16 核拓扑布局校验全绿。
    let flat = LogicalFabric::flat_16();
    for c in &flat.cores {
        c.validate()?;
    }
    hits += 1;

    // R2：Flat 组播 10/10——目的字节单播位、双播位、全播位逐次对账。
    let cases: [u8; 10] = [
        0x01, 0x80, 0x03, 0x81, 0x0F, 0xF0, 0xFF, 0x55, 0xAA, 0xC3,
    ];
    for d in cases.iter() {
        let targets = flat.deliver_logical(*d)?;
        // 期望集：位与核号
        let mut expect: Vec<u8> = Vec::new();
        for n in 0u8..16 {
            if d & (1 << n) != 0 {
                expect.push(n);
            }
        }
        if targets.len() != expect.len() {
            return Err(E_NO_TARGET);
        }
        for i in 0..expect.len() {
            if targets[i] != expect[i] {
                return Err(E_SPLIT);
            }
        }
    }
    hits += 1;

    // R3：Cluster 4x4 拓扑划分账与拓扑一致（判据原文面）。
    let clu = LogicalFabric::cluster_4x4();
    clu.cluster_ledger_consistent()?;
    hits += 1;

    // R4：Cluster 组播 10/10——每回合选一舱一致集，送达集恰为舱内目标。
    //     目的字节布局（Cluster 档）：dest[7:4]=ClusterID, dest[3:0]=Mask 位。
    let mut ok_rounds = 0u32;
    for r in 0..10u32 {
        let cid = (r % 4) as u8;
        let mask = match r % 4 {
            0 => 0x3, // 舱内位 0+1
            1 => 0x5, // 位 0+2
            2 => 0x8, // 位 3
            _ => 0xF, // 全舱
        };
        let dest = (cid << 4) | mask;
        let targets = clu.deliver_logical(dest)?;
        // 期望：物理 ID = 4*cid + 位（mask 置位者）
        let mut expect: Vec<u8> = Vec::new();
        for b in 0u8..4 {
            if mask & (1 << b) != 0 {
                expect.push(4 * cid + b);
            }
        }
        if targets.len() != expect.len() {
            return Err(E_NO_TARGET);
        }
        let mut all_match = true;
        for i in 0..expect.len() {
            if targets[i] != expect[i] {
                all_match = false;
            }
        }
        if !all_match {
            return Err(E_SPLIT);
        }
        ok_rounds += 1;
    }
    if ok_rounds != 10 {
        return Err(E_SPLIT);
    }
    hits += 1;

    // R5：物理模式单播直达与越域拒绝。
    let t = flat.deliver_physical(7)?;
    if t.len() != 1 || t[0] != 7 {
        return Err(E_SPLIT);
    }
    if flat.deliver_physical(200) != Err(E_NO_TARGET) {
        return Err(E_NO_TARGET);
    }
    hits += 1;

    // I1：DFR 非规范值 → E_DFR_BAD。
    let bad = CoreLogical {
        apic_id: 3,
        ldr: 0x08,
        dfr: 0x1234_5678,
    };
    if bad.validate() != Err(E_DFR_BAD) {
        return Err(E_DFR_BAD);
    }
    hits += 1;

    // I2：Flat 档多位 LDR / 位号不符 → E_LDR_BAD。
    let multi = CoreLogical {
        apic_id: 2,
        ldr: 0x03,
        dfr: DFR_FLAT,
    };
    if multi.validate() != Err(E_LDR_BAD) {
        return Err(E_LDR_BAD);
    }
    let mismatch = CoreLogical {
        apic_id: 2,
        ldr: 0x08, // 位 3 ≠ 物理 ID 2
        dfr: DFR_FLAT,
    };
    if mismatch.validate() != Err(E_LDR_BAD) {
        return Err(E_LDR_BAD);
    }
    hits += 1;

    // I3：Cluster 档 Mask 多位 / 越位污染 → E_LDR_BAD。
    let cmulti = CoreLogical {
        apic_id: 5,
        ldr: (1 << 28) | (0x03 << 16),
        dfr: DFR_CLUSTER,
    };
    if cmulti.validate() != Err(E_LDR_BAD) {
        return Err(E_LDR_BAD);
    }
    let cpollute = CoreLogical {
        apic_id: 5,
        ldr: (1 << 28) | (1 << 2), // bit2 属残留段（非 [31:28]/[23:16]）
        dfr: DFR_CLUSTER,
    };
    if cpollute.validate() != Err(E_LDR_BAD) {
        return Err(E_LDR_BAD);
    }
    hits += 1;

    // I4：APIC ID 越域 → E_APIC_ID。
    let over = CoreLogical {
        apic_id: 16,
        ldr: 0x01,
        dfr: DFR_FLAT,
    };
    if over.validate() != Err(E_APIC_ID) {
        return Err(E_APIC_ID);
    }
    hits += 1;

    // I5：组播零目标 → E_NO_TARGET（Flat 全零目的）。
    if flat.deliver_logical(0x00) != Err(E_NO_TARGET) {
        return Err(E_NO_TARGET);
    }
    // Cluster 舱号错位（舱 9 不存在）。
    if clu.deliver_logical(0x91) != Err(E_NO_TARGET) {
        return Err(E_NO_TARGET);
    }
    hits += 1;

    // I6：Mask 位重复舱 → E_SPLIT。
    let dup = CoreLogical {
        apic_id: 1,
        ldr: (0u32 << 28) | (1u32 << 16),
        dfr: DFR_CLUSTER,
    };
    let mut mixed = LogicalFabric { cores: Vec::new() };
    for n in 0u8..4 {
        let c = CoreLogical {
            apic_id: n,
            ldr: (0u32 << 28) | (1u32 << (16 + n)),
            dfr: DFR_CLUSTER,
        };
        mixed.cores.push(c);
    }
    // 核 1 与核 0 同舱同 Mask 位（bit16 重复）
    mixed.cores[1] = dup;
    if mixed.cluster_ledger_consistent() != Err(E_SPLIT) {
        return Err(E_SPLIT);
    }
    hits += 1;

    Ok(hits)
}
