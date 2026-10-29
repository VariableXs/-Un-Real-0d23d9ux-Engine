//! cgb833_product_tier — CGPU-F0833 Intel 产品线分档。
//!
//! 判据原文（验收行摘录）：Intel 产品线分档：设备 ID 归档（PCI ID 闭集
//! 表查档——未知 ID 即拒）；档位特征矩阵（四档各配 EU 数/带宽/媒体能力
//! 特征行）；档间单调校验（高档特征须 ≥ 低档，违序即表错）；分档快照守
//! 恒（登记数 = 各档计数之和）。判据：ID 归档、特征矩阵、单调校验、守
//! 恒快照。
//!
//! 诊断段 0x72A0（本单独占）。
//!
//! 设计纪律：no_std+alloc，禁 unwrap/expect/panic/浮点/std；失败路径
//! Result<_, u16> 闭集错误码；特征全整数；定容设备账；drill() 自例程返
//! 回 0 表示全过。

#![allow(dead_code)]

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const MOD_VERSION: &str = "W3-cgb833-v1";

/// 诊断段基（本单独占 0x72A0）。
pub const SEG_BASE: u16 = 0x72A0;

/// 设备 ID 未知（不在闭集表）。
pub const XC_DEV_UNKNOWN: u16 = SEG_BASE + 1;
/// 特征违序（高档特征 < 低档——矩阵错误）。
pub const XC_TIER_INVERT: u16 = SEG_BASE + 2;
/// 设备账满（定容设备登记耗尽）。
pub const XC_DEV_FULL: u16 = SEG_BASE + 3;
/// 设备重登（同 ID 二次登记）。
pub const XC_DEV_DUP: u16 = SEG_BASE + 4;
/// 快照漂移（登记数 ≠ 各档计数和）。
pub const XC_SNAPSHOT_DRIFT: u16 = SEG_BASE + 5;
/// 特征缺失（档特征未初始化即校验）。
pub const XC_TIER_UNSET: u16 = SEG_BASE + 6;

/// 档位数。
pub const TIER_N: usize = 4;
/// 设备账定容。
pub const DEV_CAP: usize = 32;

/// 档位闭集（判据"四档"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tier {
    /// Xe 高性能档。
    XeHpc,
    /// Iris Xe 档。
    IrisXe,
    /// UHD 基础档。
    Uhd,
    /// 老旧保留档。
    Legacy,
}

impl Tier {
    /// 档序号（单调序：0 高 → 3 低）。
    pub fn rank(self) -> usize {
        match self {
            Tier::XeHpc => 0,
            Tier::IrisXe => 1,
            Tier::Uhd => 2,
            Tier::Legacy => 3,
        }
    }
    /// 档名。
    pub fn name(self) -> &'static str {
        match self {
            Tier::XeHpc => "xe_hpc",
            Tier::IrisXe => "iris_xe",
            Tier::Uhd => "uhd",
            Tier::Legacy => "legacy",
        }
    }
}

/// 档特征行（判据"特征矩阵"——全整数）。
#[derive(Clone, Copy, Debug)]
pub struct TierFeat {
    /// EU 数。
    pub eus: u32,
    /// 内存带宽（GB/s 整数）。
    pub bandwidth_gbs: u32,
    /// 媒体能力位图（bit0 解码 H264、bit1 解码 HEVC、bit2 编码、bit3 去隔行）。
    pub media_bits: u32,
    /// 最大分辨率宽。
    pub max_w: u32,
    /// 最大分辨率高。
    pub max_h: u32,
}

/// 档特征矩阵（判据"档间单调校验"数据源；高档 → 低档递减）。
pub fn tier_feat(t: Tier) -> TierFeat {
    match t {
        Tier::XeHpc => TierFeat { eus: 512, bandwidth_gbs: 512, media_bits: 0b1111, max_w: 8192, max_h: 8192 },
        Tier::IrisXe => TierFeat { eus: 96, bandwidth_gbs: 64, media_bits: 0b1111, max_w: 7680, max_h: 4320 },
        Tier::Uhd => TierFeat { eus: 32, bandwidth_gbs: 32, media_bits: 0b0011, max_w: 3840, max_h: 2160 },
        Tier::Legacy => TierFeat { eus: 12, bandwidth_gbs: 16, media_bits: 0b0001, max_w: 1920, max_h: 1080 },
    }
}

/// 档间单调校验（判据"高档特征须 ≥ 低档"——逐档对逐项）。
pub fn monotonic_check() -> Result<(), u16> {
    let mut rows: Vec<TierFeat> = Vec::new();
    for r in 0..TIER_N {
        let t = match r {
            0 => Tier::XeHpc,
            1 => Tier::IrisXe,
            2 => Tier::Uhd,
            _ => Tier::Legacy,
        };
        rows.push(tier_feat(t));
    }
    for i in 0..rows.len() - 1 {
        let hi = rows[i];
        let lo = rows[i + 1];
        if hi.eus < lo.eus
            || hi.bandwidth_gbs < lo.bandwidth_gbs
            || hi.max_w < lo.max_w
            || hi.max_h < lo.max_h
            || (hi.media_bits & lo.media_bits) != lo.media_bits
        {
            return Err(XC_TIER_INVERT);
        }
    }
    Ok(())
}

/// PCI 设备 ID 闭集表（判据"ID 归档"）。
pub fn id_to_tier(dev_id: u16) -> Result<Tier, u16> {
    match dev_id {
        0x56A0 | 0x56A1 | 0x56A2 => Ok(Tier::XeHpc),
        0x46A0 | 0x46A6 | 0x46B3 => Ok(Tier::IrisXe),
        0x4692 | 0x4693 | 0x46D1 => Ok(Tier::Uhd),
        0x1606 | 0x1912 | 0x0166 => Ok(Tier::Legacy),
        _ => Err(XC_DEV_UNKNOWN),
    }
}

/// 分档登记条目。
#[derive(Clone, Debug)]
pub struct DevEntry {
    /// PCI 设备 ID。
    pub dev_id: u16,
    /// 归档。
    pub tier: Tier,
    /// 登记序。
    pub order: u32,
}

/// 分档器。
pub struct TierBook {
    /// 账名。
    pub name: String,
    /// 设备账（定容）。
    pub devs: Vec<DevEntry>,
    /// 各档计数（守恒项）。
    tier_cnt: [u64; TIER_N],
}

/// 守恒恒等式：devs.len() == Σ tier_cnt（四档计数之和）。
/// 每次登记后 verify() 复核。
impl TierBook {
    /// 构造。
    pub fn new(name: &str) -> Self {
        TierBook { name: name.to_string(), devs: Vec::new(), tier_cnt: [0; TIER_N] }
    }

    /// 归档登记一台设备（判据"未知 ID 即拒"）。
    pub fn register(&mut self, dev_id: u16) -> Result<Tier, u16> {
        if self.devs.len() >= DEV_CAP {
            return Err(XC_DEV_FULL);
        }
        let t = id_to_tier(dev_id)?;
        for d in self.devs.iter() {
            if d.dev_id == dev_id {
                return Err(XC_DEV_DUP);
            }
        }
        let order = self.devs.len() as u32;
        self.devs.push(DevEntry { dev_id, tier: t, order });
        self.tier_cnt[t.rank()] += 1;
        self.verify()?;
        Ok(t)
    }

    /// 档计数。
    pub fn count(&self, t: Tier) -> u64 {
        self.tier_cnt[t.rank()]
    }

    /// 查设备档。
    pub fn lookup(&self, dev_id: u16) -> Result<Tier, u16> {
        for d in self.devs.iter() {
            if d.dev_id == dev_id {
                return Ok(d.tier);
            }
        }
        Err(XC_DEV_UNKNOWN)
    }

    /// 特征查询（档 + 特征行一并出）。
    pub fn feat_of(&self, dev_id: u16) -> Result<(Tier, TierFeat), u16> {
        let t = self.lookup(dev_id)?;
        Ok((t, tier_feat(t)))
    }

    /// 媒体能力判定（位图闭集）。
    pub fn media_ok(&self, dev_id: u16, bit: u32) -> Result<bool, u16> {
        if bit > 3 {
            return Err(XC_TIER_UNSET);
        }
        let (_, f) = self.feat_of(dev_id)?;
        Ok(f.media_bits & (1 << bit) != 0)
    }

    /// 守恒校验（判据"分档快照守恒"）。
    pub fn verify(&self) -> Result<(), u16> {
        let mut sum = 0u64;
        for c in self.tier_cnt.iter() {
            sum += *c;
        }
        if sum != self.devs.len() as u64 {
            return Err(XC_SNAPSHOT_DRIFT);
        }
        // 复核各档计数与明细一致。
        let mut re = [0u64; TIER_N];
        for d in self.devs.iter() {
            re[d.tier.rank()] += 1;
        }
        for i in 0..TIER_N {
            if re[i] != self.tier_cnt[i] {
                return Err(XC_SNAPSHOT_DRIFT);
            }
        }
        Ok(())
    }

    /// 审计摘要。
    pub fn audit(&self) -> String {
        let mut s = String::new();
        s.push_str("cgb833[");
        s.push_str(&self.name);
        s.push_str("] devs=");
        s.push_str(&self.devs.len().to_string());
        for (i, n) in ["xe_hpc", "iris_xe", "uhd", "legacy"].iter().enumerate() {
            s.push(' ');
            s.push_str(n);
            s.push('=');
            s.push_str(&self.tier_cnt[i].to_string());
        }
        s
    }
}

/// 自例程：构造正/负样本走核心路径；全过返回 0，失败返回错误码。
pub fn drill() -> Result<u32, u16> {
    // 特征矩阵单调校验（表自检）。
    monotonic_check()?;

    // 负样本 1：未知设备 ID——应拒。
    let mut tb = TierBook::new("w3-drill-833");
    match tb.register(0xDEAD) {
        Err(code) if code == XC_DEV_UNKNOWN => {}
        Ok(_) => return Err(XC_DEV_UNKNOWN),
        Err(other) => return Err(other),
    }

    // 正样本：四档各登记若干。
    let hpc = [0x56A0u16, 0x56A1];
    let iris = [0x46A0u16, 0x46A6];
    let uhd = [0x4692u16, 0x4693, 0x46D1];
    let leg = [0x1606u16, 0x1912];
    for id in hpc {
        if tb.register(id)? != Tier::XeHpc {
            return Err(XC_TIER_INVERT);
        }
    }
    for id in iris {
        if tb.register(id)? != Tier::IrisXe {
            return Err(XC_TIER_INVERT);
        }
    }
    for id in uhd {
        if tb.register(id)? != Tier::Uhd {
            return Err(XC_TIER_INVERT);
        }
    }
    for id in leg {
        if tb.register(id)? != Tier::Legacy {
            return Err(XC_TIER_INVERT);
        }
    }
    if tb.devs.len() != 9 {
        return Err(XC_SNAPSHOT_DRIFT);
    }
    // 计数核对。
    if tb.count(Tier::XeHpc) != 2 || tb.count(Tier::IrisXe) != 2 || tb.count(Tier::Uhd) != 3 || tb.count(Tier::Legacy) != 2 {
        return Err(XC_SNAPSHOT_DRIFT);
    }

    // 负样本 2：重复登记——应拒。
    match tb.register(0x56A0) {
        Err(code) if code == XC_DEV_DUP => {}
        Ok(_) => return Err(XC_DEV_DUP),
        Err(other) => return Err(other),
    }

    // 特征查询：高档带宽 ≥ 低档（实例级单调抽查）。
    let (_, f0) = tb.feat_of(0x56A0)?;
    let (_, f1) = tb.feat_of(0x46A0)?;
    let (_, f2) = tb.feat_of(0x4692)?;
    let (_, f3) = tb.feat_of(0x1912)?;
    if f0.eus < f1.eus || f1.eus < f2.eus || f2.eus < f3.eus {
        return Err(XC_TIER_INVERT);
    }
    if f0.bandwidth_gbs < f1.bandwidth_gbs || f1.bandwidth_gbs < f2.bandwidth_gbs
        || f2.bandwidth_gbs < f3.bandwidth_gbs
    {
        return Err(XC_TIER_INVERT);
    }

    // 媒体位图判定：UHD 不支持编码（bit2），支持解码 H264（bit0）。
    if tb.media_ok(0x4692, 2)? {
        return Err(XC_TIER_INVERT);
    }
    if !tb.media_ok(0x4692, 0)? {
        return Err(XC_TIER_INVERT);
    }
    // 负样本 3：媒体位越界。
    match tb.media_ok(0x4692, 4) {
        Err(code) if code == XC_TIER_UNSET => {}
        Ok(_) => return Err(XC_TIER_UNSET),
        Err(other) => return Err(other),
    }
    // 负样本 4：查未登记设备档。
    match tb.lookup(0x46B3) {
        Err(code) if code == XC_DEV_UNKNOWN => {}
        Ok(_) => return Err(XC_DEV_UNKNOWN),
        Err(other) => return Err(other),
    }
    tb.verify()?;
    Ok(0)
}
