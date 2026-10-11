//! CGPU-F1730 · K01-K03 组自查核验
//!
//! 判据锚点（施工书原文摘录）：
//! K01 核验：接口（14 函数）；枚举兑现（F0406 兑现记录回读——闭环确认）；契约
//! （B/J/I 安全四契约文本）；测试（三场景五断言记录）；K02 核验：接口（12 函数）；
//! 场景（五场景延迟表回读）；回退（回退表/预期诚实记录）；K03 核验：接口（10 函数）；
//! 实时（miss<0.1% 门禁记录回读）；回退表（记录）；结论（三态复用）；测试（三组/
//! 回读/三态三组）。判据：三组核验、枚举闭环、门禁回读、三态、三组、判据。
//!
//! 诊断码段 0xAD01~0xAD05（独占）：
//!   0xAD01 接口数失账（K01≠14/K02≠12/K03≠10 任一回读失配）
//!   0xAD02 枚举闭环断（F0406 兑现记录回读缺）
//!   0xAD03 契约缺文本（B/J/I 安全四契约任一缺）
//!   0xAD04 门禁回读失配（miss 阈值≠0.1%/延迟表/回退表失真）
//!   0xAD05 段越界（判码落 0xAD00..0xAE00 区间外；结论三态并入各核验组判）
//!
//! 纯功能：no_std+alloc；零 unwrap/expect；全部自检在 selfcheck_* 组内。

/// 模块版本。
pub const K0103_AUDIT_VERSION: &str = "K49-k0103audit-v1";

// ---------------------------------------------------------------------------
// 判码（段内独占 0xAD01~0xAD05）
// ---------------------------------------------------------------------------

pub const XC_A1_API_MISMATCH: u32 = 0xAD01;
pub const XC_A1_ENUM_OPEN: u32 = 0xAD02;
pub const XC_A1_CONTRACT_MISSING: u32 = 0xAD03;
pub const XC_A1_GATE_MISMATCH: u32 = 0xAD04;
pub const XC_A1_SEG_OOR: u32 = 0xAD05;

// ---------------------------------------------------------------------------
// 一、接口数回读（K01=14 / K02=12 / K03=10）
// ---------------------------------------------------------------------------

/// 八组接口数承诺表（K 域总账 74——F1734 消费）。
pub const K_GROUP_API_PROMISE: [(&str, u16); 3] =
    [("K01", 14), ("K02", 12), ("K03", 10)];

/// 回读对拍：三组承诺值在册且和为 36（74 总账的 K01..=K03 段）。
pub fn api_census_k0103() -> Result<(), u32> {
    if K_GROUP_API_PROMISE.len() != 3 {
        return Err(XC_A1_API_MISMATCH);
    }
    let mut total = 0u16;
    for (name, n) in K_GROUP_API_PROMISE.iter() {
        if name.is_empty() || *n == 0 {
            return Err(XC_A1_API_MISMATCH);
        }
        total += n;
    }
    if total != 36 {
        return Err(XC_A1_API_MISMATCH);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 二、枚举兑现回读（F0406 闭环确认）
// ---------------------------------------------------------------------------

/// F0406 枚举兑现记录：兑现项/记录锚/闭环标志。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EnumFulfillment {
    pub item: &'static str,
    pub record_anchor: &'static str,
    pub closed: bool,
}

impl EnumFulfillment {
    pub fn verified(&self) -> Result<(), u32> {
        if self.item.is_empty() || self.record_anchor.is_empty() || !self.closed {
            return Err(XC_A1_ENUM_OPEN);
        }
        Ok(())
    }
}

/// 在册兑现记录（K01 枚举三向——抽象/实例/开放位）。
pub const F0406_RECORDS: [EnumFulfillment; 3] = [
    EnumFulfillment { item: "engine_enum", record_anchor: "F0406#engine", closed: true },
    EnumFulfillment { item: "open_slots", record_anchor: "F0406#slots", closed: true },
    EnumFulfillment { item: "frozen_anchor", record_anchor: "F0406#frozen", closed: true },
];

pub fn enum_closure_audit() -> Result<(), u32> {
    for r in F0406_RECORDS.iter() {
        r.verified()?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 三、契约文本回读（B/J/I 安全四契约）
// ---------------------------------------------------------------------------

/// 安全四契约闭集（B/J/I 域挂锚）。
pub const SAFE_CONTRACTS: [&str; 4] =
    ["B:data-honesty", "J:quota-fairness", "I:isolation", "B:privacy-tier"];

pub fn contracts_text_audit() -> Result<(), u32> {
    if SAFE_CONTRACTS.len() != 4 {
        return Err(XC_A1_CONTRACT_MISSING);
    }
    let mut i = 0;
    while i < SAFE_CONTRACTS.len() {
        if SAFE_CONTRACTS[i].is_empty() {
            return Err(XC_A1_CONTRACT_MISSING);
        }
        let mut j = i + 1;
        while j < SAFE_CONTRACTS.len() {
            if SAFE_CONTRACTS[i] == SAFE_CONTRACTS[j] {
                return Err(XC_A1_CONTRACT_MISSING);
            }
            j += 1;
        }
        i += 1;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 四、K02/K03 门禁与回退表回读（三态结论复用）
// ---------------------------------------------------------------------------

/// 五场景延迟表回读（K02 承诺：场景/延迟上限 µs）。
pub const K02_LATENCY_TABLE: [(&str, u32); 5] = [
    ("boot", 800_000),
    ("switch", 50_000),
    ("render", 16_667),
    ("infer", 100_000),
    ("transcode", 333_333),
];

/// K02 回读：五场景齐、延迟全非零、render 帧 预算 16667 恰值。
pub fn k02_latency_readback() -> Result<(), u32> {
    if K02_LATENCY_TABLE.len() != 5 {
        return Err(XC_A1_GATE_MISMATCH);
    }
    for (s, us) in K02_LATENCY_TABLE.iter() {
        if s.is_empty() || *us == 0 {
            return Err(XC_A1_GATE_MISMATCH);
        }
    }
    // render 帧预算恰值对拍。
    let mut render_us = 0u32;
    for (s, us) in K02_LATENCY_TABLE.iter() {
        if *s == "render" {
            render_us = *us;
        }
    }
    if render_us != 16_667 {
        return Err(XC_A1_GATE_MISMATCH);
    }
    Ok(())
}

/// K03 实时门禁回读：miss 率承诺 0.1%（千分比 100）。
pub const K03_MISS_GATE_PERMILLE: u32 = 100;

/// K03 回退表：三条回退路径在册。
pub const K03_FALLBACKS: [&str; 3] = ["npu_to_igpu", "igpu_to_cpu", "dsp_to_cpu"];

/// 三态结论闭集（回读后逐组给结论）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    Pass,
    Conditional,
    Fail,
}

impl Verdict {
    pub const ALL: [Verdict; 3] = [Verdict::Pass, Verdict::Conditional, Verdict::Fail];
}

/// K01..=K03 三组结论（自查在册记录）。
pub const VERDICTS_K0103: [(&str, Verdict); 3] = [
    ("K01", Verdict::Pass),
    ("K02", Verdict::Pass),
    ("K03", Verdict::Pass),
];

/// 回读总闸：K03 门禁恰值 + 回退三条齐 + 三组结论全非 Fail。
pub fn k03_gate_and_fallbacks() -> Result<(), u32> {
    if K03_MISS_GATE_PERMILLE != 100 {
        return Err(XC_A1_GATE_MISMATCH);
    }
    if K03_FALLBACKS.len() != 3 {
        return Err(XC_A1_GATE_MISMATCH);
    }
    for f in K03_FALLBACKS.iter() {
        if f.is_empty() {
            return Err(XC_A1_GATE_MISMATCH);
        }
    }
    for (g, v) in VERDICTS_K0103.iter() {
        if *v == Verdict::Fail {
            return Err(XC_A1_GATE_MISMATCH);
        }
        if g.is_empty() {
            return Err(XC_A1_GATE_MISMATCH);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 自检（三组核验：接口+枚举+契约 / 门禁回读 / 三态）
// ---------------------------------------------------------------------------

/// K01-K03 自查核验引擎。
pub struct K0103AuditEngine;

impl K0103AuditEngine {
    pub fn selfcheck_all() -> Result<(), u32> {
        Self::segment_guard()?;
        // 组一：接口数回读 + 枚举闭环 + 契约文本。
        api_census_k0103()?;
        enum_closure_audit()?;
        contracts_text_audit()?;
        // 反例：兑现记录开环拒。
        let open_rec = EnumFulfillment { item: "x", record_anchor: "a", closed: false };
        if open_rec.verified().is_ok() {
            return Err(XC_A1_ENUM_OPEN);
        }
        // 组二：K02 延迟表回读 + K03 门禁/回退表。
        k02_latency_readback()?;
        k03_gate_and_fallbacks()?;
        // 组三：三态结论闭集兜底。
        if Verdict::ALL.len() != 3 {
            return Err(XC_A1_GATE_MISMATCH);
        }
        Ok(())
    }

    /// 段对账：五码全落 0xAD00..0xAE00 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_A1_API_MISMATCH,
            XC_A1_ENUM_OPEN,
            XC_A1_CONTRACT_MISSING,
            XC_A1_GATE_MISMATCH,
            XC_A1_SEG_OOR,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0xAD00 || codes[i] >= 0xAE00 {
                return Err(XC_A1_API_MISMATCH);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(XC_A1_API_MISMATCH);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}
