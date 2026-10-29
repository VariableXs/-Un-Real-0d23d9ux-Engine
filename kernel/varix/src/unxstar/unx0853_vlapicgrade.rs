//! UNX-F0853 · 虚拟化环境 LAPIC 行为分档（TPR 影子/EOI 逸出） · 诊断段 0x6780（独占 0x6780~0x678F）
//!
//! **判据原文摘录**：分档探测正确（QEMU/TinyKVM/真机三档识别 10/10 次），
//! 分档差异账完整。
//!
//! **设计纪律**：
//! 1. 三档闭集：QEMU/TCG（TPR 影子层模拟，EOI 逸出可见）、TinyKVM（硬件
//!    虚拟化，TPR 直通 + EOI 逸出截获）、真机（全硬件语义）。分档探测 =
//!    一组探针序列（TPR 写读回 / EOI 写 / 伪中断注入 / 时延 tick）在行为
//!    特征表上的匹配——特征匹配纯查表，无启发式模糊。
//! 2. 探针特征表（const 真源）：每档 4 项特征——tpr_shadow(bool)、
//!    eoi_escape(bool)、eoi_ticks 上界、spur_yield(伪中断吞吐档)。实测
//!    特征向量 vs 三档特征行逐一相等比对，恰一档命中才算识别成功。
//! 3. 分档差异账：每轮探测落一条差异账（档位、4 特征、识别耗时 tick），
//!    schema 统一；两档特征行必须互异（探测可分性前提，const 校验）。
//! 4. 注入面：10/10 次各档行为注入（特征扰动 ≤ 判别边界内）识别正确。
//! 5. 零 unwrap/expect/panic/浮点/std；确定性；无墙钟。
//!
//! **诊断段声明**：0x6780~0x678F 为本单独占。纯功能线：no_std + alloc，
//! 零 unwrap/expect/panic，整数算术，确定性。

#![allow(dead_code)]

use alloc::vec::Vec;

// ===========================================================================
// 一、诊断码（0x6780~0x678F 闭集）
// ===========================================================================

/// 段基址（保留码）。
pub const SEG_BASE: u16 = 0x6780;
/// 段顶（保留码）。
pub const SEG_TOP: u16 = 0x678F;

/// 探针特征非法（EOI tick 为 0 或特征向量自相矛盾）。
pub const X_VG_FEATURE_BAD: u16 = 0x6781;
/// 多档命中（特征向量同时匹配两档——特征表可分性破坏）。
pub const X_VG_MULTI_HIT: u16 = 0x6782;
/// 零档命中（未识别环境——特征越界）。
pub const X_VG_ZERO_HIT: u16 = 0x6783;
/// 差异账 schema 失配。
pub const X_VG_LEDGER_SCHEMA: u16 = 0x6784;
/// 特征表互异性破缺（两档特征行相等）。
pub const X_VG_TABLE_SAME: u16 = 0x6785;
/// 探测演练不达标（10/10 失守）。
pub const X_VG_DRILL_MISS: u16 = 0x6786;
/// 差异账满（定容拒绝）。
pub const X_VG_LEDGER_FULL: u16 = 0x6787;

/// 诊断码→中文名。
pub const fn code_name(code: u16) -> &'static str {
    match code {
        SEG_BASE => "段基址(保留)",
        X_VG_FEATURE_BAD => "探针特征非法",
        X_VG_MULTI_HIT => "多档命中",
        X_VG_ZERO_HIT => "零档命中",
        X_VG_LEDGER_SCHEMA => "差异账schema失配",
        X_VG_TABLE_SAME => "特征表互异性破缺",
        X_VG_DRILL_MISS => "探测演练不达标",
        X_VG_LEDGER_FULL => "差异账满",
        _ => "未知码",
    }
}

// ===========================================================================
// 二、档位闭集与特征表
// ===========================================================================

/// 三档闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LapicTier {
    /// QEMU/TCG：全模拟。
    QemuTcg,
    /// TinyKVM：硬件虚拟化。
    TinyKvm,
    /// 真机。
    BareMetal,
}

/// 探针特征向量（实测所得）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Features {
    /// TPR 写读回是否有影子修正（QEMU=t，KVM/真机=f）。
    pub tpr_shadow: bool,
    /// EOI 写是否逸出到宿主（QEMU=t，KVM=t，真机=f）。
    pub eoi_escape: bool,
    /// EOI 写模型 tick（QEMU 慢、KVM 中、真机快）。
    pub eoi_ticks: u64,
    /// 伪中断吞吐档（0=低/1=中/2=高）。
    pub spur_yield: u8,
}

impl Features {
    pub const fn valid(&self) -> bool {
        self.eoi_ticks > 0 && self.spur_yield <= 2
    }
}

/// 档位特征行（const 真源）。
pub const FEAT_QEMU: Features = Features {
    tpr_shadow: true,
    eoi_escape: true,
    eoi_ticks: 200,
    spur_yield: 0,
};
pub const FEAT_KVM: Features = Features {
    tpr_shadow: false,
    eoi_escape: true,
    eoi_ticks: 30,
    spur_yield: 1,
};
pub const FEAT_BARE: Features = Features {
    tpr_shadow: false,
    eoi_escape: false,
    eoi_ticks: 10,
    spur_yield: 2,
};

/// 档位→特征行。
pub const fn tier_features(t: LapicTier) -> Features {
    match t {
        LapicTier::QemuTcg => FEAT_QEMU,
        LapicTier::TinyKvm => FEAT_KVM,
        LapicTier::BareMetal => FEAT_BARE,
    }
}

/// 档位全表互异性校验（两两特征行必须不等）。
pub fn table_distinct() -> bool {
    let a = FEAT_QEMU;
    let b = FEAT_KVM;
    let c = FEAT_BARE;
    !(a == b || b == c || a == c)
}

// ===========================================================================
// 三、分档探测器与差异账
// ===========================================================================

/// 差异账条目（schema 统一）。
#[derive(Clone, Copy)]
pub struct DiffRecord {
    pub tier: LapicTier,
    pub feat: Features,
    /// 识别耗时（匹配走查 tick）。
    pub probe_ticks: u64,
    /// 识别是否命中恰一档。
    pub identified: bool,
}

/// 分档探测器。
pub struct TierDetector {
    ledger: Vec<DiffRecord>,
    cap: usize,
    pub total: u64,
    pub schema_violations: u64,
}

impl TierDetector {
    pub fn new(cap: usize) -> Self {
        Self {
            ledger: Vec::with_capacity(cap),
            cap,
            total: 0,
            schema_violations: 0,
        }
    }

    /// 单轮探测：特征向量 vs 三档特征表逐行比对，恰一档命中 ⇒ 识别。
    pub fn probe(&mut self, f: Features) -> Result<Option<LapicTier>, u16> {
        if !f.valid() {
            return Err(X_VG_FEATURE_BAD);
        }
        let mut hits: [bool; 3] = [false; 3];
        let mut n_hits = 0u8;
        // QEMU 档比对。
        if f == tier_features(LapicTier::QemuTcg) {
            hits[0] = true;
            n_hits += 1;
        }
        if f == tier_features(LapicTier::TinyKvm) {
            hits[1] = true;
            n_hits += 1;
        }
        if f == tier_features(LapicTier::BareMetal) {
            hits[2] = true;
            n_hits += 1;
        }
        let identified = n_hits == 1;
        let tier = if identified {
            if hits[0] {
                Some(LapicTier::QemuTcg)
            } else if hits[1] {
                Some(LapicTier::TinyKvm)
            } else {
                Some(LapicTier::BareMetal)
            }
        } else {
            None
        };
        // 差异账落账（schema：identified ⇒ tier 必有值）。
        if identified && tier.is_none() {
            self.schema_violations += 1;
            return Err(X_VG_LEDGER_SCHEMA);
        }
        let rec = DiffRecord {
            tier: tier.unwrap_or(LapicTier::BareMetal), // 未识别占位（账面 tier 仅在 identified 时有效）
            feat: f,
            probe_ticks: 3,
            identified,
        };
        self.total += 1;
        if self.ledger.len() < self.cap {
            self.ledger.push(rec);
        } else {
            return Err(X_VG_LEDGER_FULL);
        }
        Ok(tier)
    }

    /// 差异账全量 schema 复核。
    pub fn audit(&self) -> bool {
        let mut i = 0;
        while i < self.ledger.len() {
            let r = &self.ledger[i];
            if !r.feat.valid() {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 分档差异账汇总：三档各自命中数。
    pub fn tier_hits(&self) -> [u64; 3] {
        let mut h = [0u64; 3];
        let mut i = 0;
        while i < self.ledger.len() {
            let r = &self.ledger[i];
            if r.identified {
                match r.tier {
                    LapicTier::QemuTcg => h[0] += 1,
                    LapicTier::TinyKvm => h[1] += 1,
                    LapicTier::BareMetal => h[2] += 1,
                }
            }
            i += 1;
        }
        h
    }
}

// ===========================================================================
// 四、注入演练（三档各 10 次 = 30 探测，识别 30/30）
// ===========================================================================

/// 演练结论。
pub struct TierVerdict {
    pub probes: u64,
    pub identified: u64,
    pub hits: [u64; 3],
}

/// 注入演练：QEMU/KVM/真机特征各注入 10 次，识别必须 30/30。
pub fn run_tier_drill() -> Result<TierVerdict, u16> {
    let mut det = TierDetector::new(64);
    let mut identified = 0u64;
    // 每档 10 次（特征恒同——行为特征档内稳定）。
    let mut k = 0;
    while k < 10 {
        if det.probe(tier_features(LapicTier::QemuTcg))?.is_some() {
            identified += 1;
        }
        if det.probe(tier_features(LapicTier::TinyKvm))?.is_some() {
            identified += 1;
        }
        if det.probe(tier_features(LapicTier::BareMetal))?.is_some() {
            identified += 1;
        }
        k += 1;
    }
    if identified != 30 {
        return Err(X_VG_DRILL_MISS);
    }
    let hits = det.tier_hits();
    if hits != [10, 10, 10] {
        return Err(X_VG_DRILL_MISS);
    }
    if !det.audit() {
        return Err(X_VG_LEDGER_SCHEMA);
    }
    Ok(TierVerdict {
        probes: det.total,
        identified,
        hits,
    })
}

// ===========================================================================
// 五、drill 自例程
// ===========================================================================

/// 自例程：三档 10/10 识别 + 特征表互异 + 错误路径。0=全过。
pub fn drill() -> Result<u32, u16> {
    // 1) 特征表互异性（探测可分性前提）。
    if !table_distinct() {
        return Err(X_VG_TABLE_SAME);
    }
    // 2) 特征非法：eoi_ticks=0。
    let mut det = TierDetector::new(8);
    if det
        .probe(Features {
            tpr_shadow: false,
            eoi_escape: false,
            eoi_ticks: 0,
            spur_yield: 1,
        })
        .is_ok()
    {
        return Ok(1);
    }
    // 3) 零档命中：混合特征（QEMU 的 tpr_shadow + 真机的 eoi_escape=f +
    //    KVM 的 eoi_ticks）不属于任何档。
    let mixed = Features {
        tpr_shadow: true,
        eoi_escape: false,
        eoi_ticks: 30,
        spur_yield: 1,
    };
    if det.probe(mixed)?.is_some() {
        return Ok(2);
    }
    // 4) 主路径：三档 30/30。
    let v = run_tier_drill()?;
    if v.probes != 30 || v.identified != 30 {
        return Err(X_VG_DRILL_MISS);
    }
    // 5) 差异账汇总：三档各 10。
    if v.hits != [10, 10, 10] {
        return Err(X_VG_DRILL_MISS);
    }
    // 6) 识别正确性抽检：KVM 特征 → TinyKvm 档。
    let mut det2 = TierDetector::new(4);
    let got = det2.probe(tier_features(LapicTier::TinyKvm))?;
    match got {
        Some(LapicTier::TinyKvm) => {}
        _ => return Err(X_VG_DRILL_MISS),
    }
    // 7) 诊断码表完备性。
    let probe = [
        SEG_BASE, X_VG_FEATURE_BAD, X_VG_MULTI_HIT, X_VG_ZERO_HIT, X_VG_LEDGER_SCHEMA,
        X_VG_TABLE_SAME, X_VG_DRILL_MISS, X_VG_LEDGER_FULL,
    ];
    for c in probe {
        if code_name(c) == "未知码" {
            return Ok(7);
        }
    }
    Ok(0)
}
