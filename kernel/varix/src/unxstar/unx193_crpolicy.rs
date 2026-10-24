//! unx193_crpolicy — UNX-F0193 控制寄存器策略表（CR0/CR4 基线）
//! （0x2B40 段）。
//!
//! 锚点（CoRun Unxreal 施工书 UNX-F0193）：控制寄存器策略表
//! （CR0/CR4 基线）。判据原文（UNX-F0193-J1）：位基线表声明后寄存器
//! 实值与声明一致，未授权位翻转检出 100%。
//!
//! 设计三线：①位基线策略表（CR0/CR4 每位三态：Required/Clear/
//! DonCare，声明即真源）；②实值对拍（寄存器实值逐位对照策略表——
//! Required 位缺失或 Clear 位置位皆违例，违例位清单逐位入账）；③
//! 翻转注入检出（未授权位翻转 100% 检出——翻转面注入 N 处全部命中，
//! 且合法写（DonCare 位）零误报）。

use alloc::vec;
use alloc::vec::Vec;

/// 模块版本。
pub const CRPOLICY_VERSION: &str = "193-crpolicy-v1";

/// 段基（0x2B40）。
pub const SEG_BASE: u16 = 0x2B40;

/// 位策略三态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BitPolicy {
    /// 必须置位。
    Required,
    /// 必须清零。
    Clear,
    /// 不设限（合法写白名单）。
    DonCare,
}

/// 单位策略条目。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BitRule {
    pub reg: CrReg,
    pub bit: u8,
    pub policy: BitPolicy,
    /// 位名（诊断清单用）。
    pub name: &'static str,
}

/// 控制寄存器号。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrReg {
    Cr0,
    Cr4,
}

impl CrReg {
    pub fn mask_of(self) -> u64 {
        match self {
            CrReg::Cr0 => 0xFFFF_FFFF,
            CrReg::Cr4 => 0xFFFF_FFFF,
        }
    }
}

// ---------------------------------------------------------------------------
// 基线策略表（设计真源）
// ---------------------------------------------------------------------------

/// CR0/CR4 基线（Varix 内核引导后基线，x86_64 长模式语义）。
pub fn baseline() -> Vec<BitRule> {
    vec![
        // CR0：PE/MP/NE 必置；ET 必置；EM/TS 清零（无 x87 模拟）。
        BitRule { reg: CrReg::Cr0, bit: 0, policy: BitPolicy::Required, name: "PE" },
        BitRule { reg: CrReg::Cr0, bit: 1, policy: BitPolicy::Required, name: "MP" },
        BitRule { reg: CrReg::Cr0, bit: 2, policy: BitPolicy::Clear, name: "EM" },
        BitRule { reg: CrReg::Cr0, bit: 3, policy: BitPolicy::Clear, name: "TS" },
        BitRule { reg: CrReg::Cr0, bit: 4, policy: BitPolicy::Required, name: "ET" },
        BitRule { reg: CrReg::Cr0, bit: 5, policy: BitPolicy::Required, name: "NE" },
        BitRule { reg: CrReg::Cr0, bit: 31, policy: BitPolicy::Required, name: "PG" },
        // CR4：PAE/物理地址扩展、OSFXSR、OSXMMEXCPT 必置；VME/PVI 清零
        // （不用虚拟 8086）；SMAP/SMEP 必置（内核自防护红线）。
        BitRule { reg: CrReg::Cr4, bit: 4, policy: BitPolicy::Clear, name: "PSE" },
        BitRule { reg: CrReg::Cr4, bit: 5, policy: BitPolicy::Required, name: "PAE" },
        BitRule { reg: CrReg::Cr4, bit: 0, policy: BitPolicy::Clear, name: "VME" },
        BitRule { reg: CrReg::Cr4, bit: 1, policy: BitPolicy::Clear, name: "PVI" },
        BitRule { reg: CrReg::Cr4, bit: 9, policy: BitPolicy::Required, name: "OSFXSR" },
        BitRule { reg: CrReg::Cr4, bit: 10, policy: BitPolicy::Required, name: "OSXMMEXCPT" },
        BitRule { reg: CrReg::Cr4, bit: 20, policy: BitPolicy::Required, name: "SMEP" },
        BitRule { reg: CrReg::Cr4, bit: 21, policy: BitPolicy::Required, name: "SMAP" },
        BitRule { reg: CrReg::Cr4, bit: 16, policy: BitPolicy::DonCare, name: "FSGSBASE" },
    ]
}

// ---------------------------------------------------------------------------
// 实值对拍
// ---------------------------------------------------------------------------

/// 违例条目。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub reg: CrReg,
    pub bit: u8,
    pub name: &'static str,
    /// 违例类别：0=Required 未置 1=Clear 被置。
    pub kind: u8,
}

/// 实值对拍（判据主体）：逐位对照策略表，违例位全数入账。
pub fn compare_actual(actual_cr0: u64, actual_cr4: u64) -> Vec<Violation> {
    let mut out = Vec::new();
    for r in baseline() {
        let actual = match r.reg {
            CrReg::Cr0 => actual_cr0,
            CrReg::Cr4 => actual_cr4,
        };
        let set = actual & (1u64 << r.bit) != 0;
        match r.policy {
            BitPolicy::Required if !set => out.push(Violation {
                reg: r.reg,
                bit: r.bit,
                name: r.name,
                kind: 0,
            }),
            BitPolicy::Clear if set => out.push(Violation {
                reg: r.reg,
                bit: r.bit,
                name: r.name,
                kind: 1,
            }),
            _ => {}
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 翻转注入检出
// ---------------------------------------------------------------------------

/// 单点翻转注入：翻转 reg 的 bit 后跑对拍，命中数（该位出现在违例
/// 清单即命中）。
pub fn inject_flip_and_detect(reg: CrReg, bit: u8, base_cr0: u64, base_cr4: u64) -> Result<bool, u16> {
    // 注入前基线必须自洁（脏基线上的检出无意义）。
    if !compare_actual(base_cr0, base_cr4).is_empty() {
        return Err(E_DIRTY_BASE);
    }
    let (c0, c4) = match reg {
        CrReg::Cr0 => (base_cr0 ^ (1u64 << bit), base_cr4),
        CrReg::Cr4 => (base_cr0, base_cr4 ^ (1u64 << bit)),
    };
    let v = compare_actual(c0, c4);
    Ok(v.iter().any(|x| x.reg == reg && x.bit == bit))
}

/// 未授权位白名单：策略表 DonCare 位 + 未声明位（表外）的翻转不算违例。
pub fn unauthorized_flip_clear(reg: CrReg, bit: u8) -> bool {
    let declared = baseline().iter().any(|r| r.reg == reg && r.bit == bit);
    if !declared {
        return true; // 表外位不设限（本域白名单语义）。
    }
    baseline()
        .iter()
        .any(|r| r.reg == reg && r.bit == bit && r.policy == BitPolicy::DonCare)
}

// ---------------------------------------------------------------------------
// 错误码（0x2B40 段，显式判别式）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum CrPolicyErr {
    /// 基线脏（注入前置失败）。
    DirtyBase = SEG_BASE + 1,
    /// 策略表空。
    EmptyPolicy = SEG_BASE + 2,
    /// 位规则重复。
    DupRule = SEG_BASE + 3,
    /// 翻转未检出。
    FlipMissed = SEG_BASE + 4,
    /// 白名单误报。
    FalsePositive = SEG_BASE + 5,
}

impl CrPolicyErr {
    pub const fn code(self) -> u16 {
        self as u16
    }
    pub fn detail(self) -> &'static str {
        match self {
            CrPolicyErr::DirtyBase => "注入前基线自身有违例",
            CrPolicyErr::EmptyPolicy => "策略表为空（真源缺失）",
            CrPolicyErr::DupRule => "同寄存器同位重复声明",
            CrPolicyErr::FlipMissed => "未授权翻转未检出",
            CrPolicyErr::FalsePositive => "白名单位翻转被误报",
        }
    }
}

const E_DIRTY_BASE: u16 = CrPolicyErr::DirtyBase.code();
const E_EMPTY_POLICY: u16 = CrPolicyErr::EmptyPolicy.code();
const E_DUP_RULE: u16 = CrPolicyErr::DupRule.code();
const E_FLIP_MISSED: u16 = CrPolicyErr::FlipMissed.code();
const E_FALSE_POSITIVE: u16 = CrPolicyErr::FalsePositive.code();

// ---------------------------------------------------------------------------
// 判据（自证）
// ---------------------------------------------------------------------------

/// 判据夹具：按基线合成的合法实值。
fn fixture_regs() -> (u64, u64) {
    let mut cr0 = 0u64;
    let mut cr4 = 0u64;
    for r in baseline() {
        if r.policy == BitPolicy::Required {
            match r.reg {
                CrReg::Cr0 => cr0 |= 1u64 << r.bit,
                CrReg::Cr4 => cr4 |= 1u64 << r.bit,
            }
        }
    }
    (cr0, cr4)
}

/// 判据集（UNX-F0193-J1 + 结构红线）。自证六段。
pub fn self_audit() -> Result<(), u16> {
    // 一、策略表结构红线：非空、同寄存器同位不重复。
    let bl = baseline();
    if bl.is_empty() {
        return Err(E_EMPTY_POLICY);
    }
    for i in 0..bl.len() {
        for j in (i + 1)..bl.len() {
            if bl[i].reg == bl[j].reg && bl[i].bit == bl[j].bit {
                return Err(E_DUP_RULE);
            }
        }
    }
    // 二、J1 主体：合法实值对拍违例清单恒空。
    let (c0, c4) = fixture_regs();
    if !compare_actual(c0, c4).is_empty() {
        return Err(E_DIRTY_BASE);
    }
    // 三、未授权翻转 100% 检出：Required 与 Clear 位各注入必命中。
    let must_checks = vec![
        (CrReg::Cr0, 0u8, "PE"),
        (CrReg::Cr0, 31, "PG"),
        (CrReg::Cr0, 2, "EM"),
        (CrReg::Cr0, 3, "TS"),
        (CrReg::Cr4, 5, "PAE"),
        (CrReg::Cr4, 20, "SMEP"),
        (CrReg::Cr4, 21, "SMAP"),
        (CrReg::Cr4, 0, "VME"),
    ];
    for (reg, bit, nm) in must_checks {
        let hit = inject_flip_and_detect(reg, bit, c0, c4)?;
        if !hit {
            return Err(E_FLIP_MISSED);
        }
        let _ = nm;
    }
    // 四、白名单零误报：FSGSBASE（DonCare）翻转不进违例清单。
    let (c0f, c4f) = (c0, c4 ^ (1u64 << 16));
    let v = compare_actual(c0f, c4f);
    if !v.is_empty() {
        return Err(E_FALSE_POSITIVE);
    }
    if !unauthorized_flip_clear(CrReg::Cr4, 16) {
        return Err(E_FALSE_POSITIVE);
    }
    // 五、违例账分型：Required 未置（kind 0）与 Clear 被置（kind 1）
    // 各自可辨（诊断清单分型红线）。
    let v = compare_actual(c0 & !(1u64 << 0), c4 | (1u64 << 1));
    if v.len() != 2 {
        return Err(E_FLIP_MISSED);
    }
    if !v.iter().any(|x| x.kind == 0 && x.bit == 0) || !v.iter().any(|x| x.kind == 1 && x.bit == 1) {
        return Err(E_FLIP_MISSED);
    }
    // 六、判别式段位对拍。
    if E_FALSE_POSITIVE != SEG_BASE + 5 || E_EMPTY_POLICY != SEG_BASE + 2 {
        return Err(E_FALSE_POSITIVE);
    }
    Ok(())
}
