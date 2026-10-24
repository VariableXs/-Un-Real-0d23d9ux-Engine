//! unx0357_apcaprep —— UNX-F0357 AP 启动能力位报告（拓扑面）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0357 · AP 能力位 · 目标 200 行
//! 判据原文：UNX-F0357-J1 每 AP 启动后上报能力位（x2APIC/SMAP/
//!           UMIP/RDRAND 四位闭集）；拓扑账要求：全 AP 位集与
//!           BSP 逐位对齐（异构位集 = 调度假设破，立案不放行）；
//!           能力位只读采样（位集构造后冻结）。
//! 组件：能力位（四位 u8 掩码）+ AP 报告行（核号 + 位集 + 采样
//!       签）+ 拓扑对账核（全 AP vs BSP 逐位）+ 冻结闸。
//! 职责定位：能力位对齐是「同构调度假设」的兑现——调度器假设
//!   所有核能跑同一套代码路径；一台 AP 缺 SMAP 位 = 用户态
//!   防护只在一半核上存在，这种不对称必须在启动时拦下，不能
//!   等第一次缺页异常时才炸。
//! 性能逐项分解：报告 O(核数)；对账 O(核数)；冻结 O(1)。
//! 诊断码段：0x20640（0x20641~0x20649 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const AR_VERSION: &str = "unx0357_apcaprep-v1";

/// 诊断段基址（0x20640 段 · 本模块独占）。
pub const AR_SEG_BASE: u32 = 0x20640;
pub const XC_CAP_MISMATCH: u32 = 0x20641; // 位集失配
pub const XC_CORE_OOR: u32 = 0x20642; // 核号越域
pub const XC_BIT_OOR: u32 = 0x20643; // 位越闭集
pub const XC_FROZEN: u32 = 0x20644; // 冻结后拒改
pub const XC_BSP_MISSING: u32 = 0x20645; // 无 BSP 基准
pub const XC_DUP_CORE: u32 = 0x20646; // 核重复上报
pub const XC_SIG_DRIFT: u32 = 0x20647; // 采样签漂
pub const XC_NONE_AP: u32 = 0x20648; // 无 AP 报告
pub const XC_ALL_ZERO: u32 = 0x20649; // 全零位集拒

/// 能力位掩码（四位闭集）。
pub const CAP_X2APIC: u8 = 0b0001;
pub const CAP_SMAP: u8 = 0b0010;
pub const CAP_UMIP: u8 = 0b0100;
pub const CAP_RDRAND: u8 = 0b1000;
/// 合法掩码闭集（四位）。
pub const CAP_MASK_ALL: u8 = 0b1111;

/// FNV-1a 32 位采样签。
pub fn fnv32(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for &b in data {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// AP 能力报告行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ApCapRow {
    pub core: u32,
    /// 能力位集（低 4 位有效）。
    pub caps: u8,
    /// 采样签 = FNV32(core || caps)（防账改）。
    pub sig: u32,
}

impl ApCapRow {
    pub fn new(core: u32, caps: u8) -> Result<ApCapRow, u32> {
        if core > 15 {
            return Err(XC_CORE_OOR);
        }
        if caps & !CAP_MASK_ALL != 0 {
            return Err(XC_BIT_OOR);
        }
        if caps == 0 {
            return Err(XC_ALL_ZERO);
        }
        let sig = fnv32(&[core as u8, caps]);
        Ok(ApCapRow { core, caps, sig })
    }

    pub fn sig_ok(&self) -> bool {
        self.sig == fnv32(&[self.core as u8, self.caps])
    }
}

/// 拓扑能力账。
pub struct CapTopology {
    bsp: Option<ApCapRow>,
    aps: Vec<ApCapRow>,
    frozen: bool,
}

impl CapTopology {
    pub fn new() -> CapTopology {
        CapTopology { bsp: None, aps: Vec::new(), frozen: false }
    }

    /// 登记 BSP 基准（核号 0）。
    pub fn set_bsp(&mut self, row: ApCapRow) -> Result<(), u32> {
        if self.frozen {
            return Err(XC_FROZEN);
        }
        if row.core != 0 {
            return Err(XC_CORE_OOR);
        }
        self.bsp = Some(row);
        Ok(())
    }

    /// 登记 AP（核号 1~15；重复拒；冻结拒）。
    pub fn add_ap(&mut self, row: ApCapRow) -> Result<(), u32> {
        if self.frozen {
            return Err(XC_FROZEN);
        }
        if row.core == 0 || row.core > 15 {
            return Err(XC_CORE_OOR);
        }
        if self.aps.iter().any(|r| r.core == row.core) {
            return Err(XC_DUP_CORE);
        }
        self.aps.push(row);
        Ok(())
    }

    /// 冻结（采样窗关闭）。
    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    /// 拓扑对账：全 AP 位集与 BSP 逐位相等（异构立案）。
    pub fn align_check(&self) -> Result<(), u32> {
        let b = self.bsp.as_ref().ok_or(XC_BSP_MISSING)?;
        if self.aps.is_empty() {
            return Err(XC_NONE_AP);
        }
        for ap in self.aps.iter() {
            if !ap.sig_ok() || !b.sig_ok() {
                return Err(XC_SIG_DRIFT);
            }
            if ap.caps != b.caps {
                return Err(XC_CAP_MISMATCH);
            }
        }
        Ok(())
    }

    /// 失配核号清单。
    pub fn mismatch_cores(&self) -> Vec<u32> {
        match self.bsp.as_ref() {
            Some(b) => self.aps.iter().filter(|ap| ap.caps != b.caps).map(|ap| ap.core).collect(),
            None => Vec::new(),
        }
    }

    /// AP 数。
    pub fn ap_count(&self) -> u32 {
        self.aps.len() as u32
    }
}

pub fn run_unx0357_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0357_apcaprep");
    // 族一：行构造与签。
    let bsp = ApCapRow::new(0, CAP_X2APIC | CAP_SMAP | CAP_UMIP | CAP_RDRAND).unwrap_or(ApCapRow { core: 0, caps: 0xF, sig: 0 });
    cs.add("BSP 行", bsp.core == 0 && bsp.caps == 0xF, "全能力");
    cs.add("签复算", bsp.sig_ok(), "FNV32 一致");
    cs.add("位越域拒", ApCapRow::new(1, 0x10).is_err(), "BIT_OOR");
    cs.add("全零拒", ApCapRow::new(1, 0).is_err(), "ALL_ZERO");
    cs.add("核越域拒", ApCapRow::new(16, 0x1).is_err(), "CORE_OOR");
    // 族二：拓扑登记。
    let mut topo = CapTopology::new();
    cs.add("BSP 登记", topo.set_bsp(bsp).is_ok(), "核 0 基准");
    cs.add("BSP 重设覆盖", topo.set_bsp(bsp).is_ok(), "采样窗内允许");
    let ap1 = ApCapRow::new(1, 0xF).unwrap_or(bsp);
    let ap2 = ApCapRow::new(2, 0xF).unwrap_or(bsp);
    cs.add("AP 登记", topo.add_ap(ap1).is_ok() && topo.add_ap(ap2).is_ok(), "两 AP");
    cs.add("重号拒", topo.add_ap(ap1) == Err(XC_DUP_CORE), "DUP_CORE");
    cs.add("AP 数 2", topo.ap_count() == 2, "账面");
    // 族三：对齐核。
    cs.add("全对齐", topo.align_check().is_ok(), "8 核假设成立");
    let odd = ApCapRow::new(3, CAP_X2APIC | CAP_SMAP).unwrap_or(bsp); // 缺 UMIP/RDRAND
    let _ = topo.add_ap(odd);
    cs.add("异构立案", topo.align_check() == Err(XC_CAP_MISMATCH), "核 3 位集缺");
    cs.add("失配核号", topo.mismatch_cores().as_slice() == &[3u32], "定位核 3");
    // 族四：冻结。
    topo.freeze();
    cs.add("冻结拒改", topo.add_ap(ap1).is_err() && topo.set_bsp(bsp).is_err(), "采样窗闭");
    // 族五：无基准/无 AP 拒。
    let mut t2 = CapTopology::new();
    let _ = t2.add_ap(ap1);
    cs.add("无 BSP 拒", t2.align_check() == Err(XC_BSP_MISSING), "BSP_MISSING");
    let mut t3 = CapTopology::new();
    let _ = t3.set_bsp(bsp);
    cs.add("无 AP 拒", t3.align_check() == Err(XC_NONE_AP), "NONE_AP");
    // 族六：签漂检出。
    let mut bad = ap1;
    bad.caps ^= 1;
    cs.add("签漂检出", !bad.sig_ok(), "改位不签即破");
    cs.add("冻结闸拦非 BSP", topo.set_bsp(ApCapRow::new(5, 0xF).unwrap_or(bsp)).is_err(), "冻结先拦");
    cs
}
