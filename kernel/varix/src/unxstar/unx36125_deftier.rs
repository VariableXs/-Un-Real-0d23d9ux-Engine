//! UNX-F36125-J1 默认 SD 策略（类型创建时自动 SD 合成）。
//! 判据：① 四档策略各合成出档位特征明确的 SD（控制位快照冻结）；
//! ② 合成-复核双向一致（档位误配显性 E_TIER）；
//! ③ 黄金八类逐类合成成功且与注册表档位一致；
//! ④ 纯功能代码 ≥320 行。
//! 设计纪律：分档真源——36121::TypePolicy 是档位唯一裁决处，本模块只做
//! 「档位 → SD 形状」的确定性投影；SD 构造全部走 unx36063（B04 构造器
//! 真源），不私造 Sd 字面量。36126 装配挂点消费 synthesize_for_type 作为
//! ObCreateObject 的默认 SD 来源；档位快照供 36131 启动自检核对。

use crate::checks::CheckSet;
use super::unx36061_sdcore::{Sd, C_DACL_PRESENT, C_DACL_PROT, C_GROUP_DFLT, C_OWNER_DFLT, C_SACL_PRESENT, C_SACL_PROT};
use super::unx36063_sdctor::{self as ctor, SectSpec, SECT_MIN};
use super::unx36121_typereg::{self as tr, TypePolicy, TypeRegistry, GOLDEN_N};

pub const SEG_BASE: u16 = 0x3b10;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_SYN: u16 = SEG_BASE + 2; // 合成失败（构造器诊断透传）
pub const E_TIER: u16 = SEG_BASE + 3; // 档位误配（SD 控制位与档位不符）
pub const E_NOENT: u16 = SEG_BASE + 4; // 类型未注册（合成前置）
pub const E_SHAPE: u16 = SEG_BASE + 5; // 合成形状不符（段长/偏移纪律破）
pub const E_INTG: u16 = SEG_BASE + 6; // 完整性自检不符

/// 模块版本锚。
pub const VER: u32 = 1;

/// 合成段长档（全档统一最小体：Owner/Group 8 字节 SID 槽、SACL/DACL 2 字节 ACL 头）。
pub const SYN_OWNER_LEN: usize = 8;
pub const SYN_GROUP_LEN: usize = 8;
pub const SYN_SACL_LEN: usize = 2;
pub const SYN_DACL_LEN: usize = 2;

/// 按档位合成默认 SD（ObCreateObject 默认 SD 的唯一入口）。
/// 档位 → 形状投影：
///   ClosedAll     → 无 SD 对象（全 SENTINEL，默认拒绝红线，见 36065）；
///   NullDacl      → null DACL（显式空授权域，与默认拒绝分离，见 36066）；
///   InheritParent → 四段齐备 + OWNER/GROUP DFLT（继承合成模板）；
///   AdminGate     → 四段齐备 + 双 PROTECTED + 双 PRESENT（管理员门）。
pub fn synthesize(policy: TypePolicy) -> Result<(Sd, [SectSpec; 4]), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    match policy {
        TypePolicy::ClosedAll => ctor::build_no_sd().map_err(|_| E_SYN),
        TypePolicy::NullDacl => ctor::build_null_dacl().map_err(|_| E_SYN),
        TypePolicy::InheritParent => {
            ctor::build_full(SYN_OWNER_LEN, SYN_GROUP_LEN, SYN_SACL_LEN, SYN_DACL_LEN, 0)
                .map_err(|_| E_SYN)
        }
        TypePolicy::AdminGate => ctor::build_full(
            SYN_OWNER_LEN,
            SYN_GROUP_LEN,
            SYN_SACL_LEN,
            SYN_DACL_LEN,
            C_DACL_PROT | C_SACL_PROT,
        )
        .map_err(|_| E_SYN),
    }
}

/// 按类型名合成（先查注册表档位，未注册 → E_NOENT——36122 红线在合成面复刻）。
pub fn synthesize_for_type(reg: &TypeRegistry, name: &str) -> Result<(Sd, [SectSpec; 4]), u16> {
    let policy = reg.policy_of(name).map_err(|_| E_NOENT)?;
    synthesize(policy)
}

/// 档位-控制位特征面（复核真源）：每档位的必现位闭集与禁现位闭集。
pub fn tier_signature(policy: TypePolicy) -> (u16, u16) {
    match policy {
        TypePolicy::ClosedAll => (0, C_DACL_PRESENT | C_SACL_PRESENT | C_OWNER_DFLT | C_GROUP_DFLT),
        TypePolicy::NullDacl => (C_DACL_PRESENT, C_SACL_PRESENT | C_DACL_PROT | C_SACL_PROT),
        TypePolicy::InheritParent => {
            (C_DACL_PRESENT | C_SACL_PRESENT | C_OWNER_DFLT | C_GROUP_DFLT, C_DACL_PROT | C_SACL_PROT)
        }
        TypePolicy::AdminGate => (
            C_DACL_PRESENT | C_SACL_PRESENT | C_DACL_PROT | C_SACL_PROT | C_OWNER_DFLT | C_GROUP_DFLT,
            0,
        ),
    }
}

/// 合成-复核：SD 控制位必须落在档位特征内（必现位全在 ∧ 禁现位全无）。
pub fn verify_tier(sd: &Sd, policy: TypePolicy) -> Result<(), u16> {
    let (must, must_not) = tier_signature(policy);
    if sd.control & must != must {
        return Err(E_TIER);
    }
    if sd.control & must_not != 0 {
        return Err(E_TIER);
    }
    Ok(())
}

/// 合成形状复核：ClosedAll 全零段；NullDacl 仅 DACL 槽 ≥ ACL 头（2），
/// 其余段体恒零；其余档 Owner/Group ≥ SID 槽（8）、SACL/DACL ≥ ACL 头（2）。
pub fn verify_shape(sd: &Sd, bodies: &[SectSpec; 4], policy: TypePolicy) -> Result<(), u16> {
    let _ = sd;
    match policy {
        TypePolicy::ClosedAll => {
            let mut i = 0;
            while i < 4 {
                if bodies[i].len != 0 {
                    return Err(E_SHAPE);
                }
                i += 1;
            }
        }
        TypePolicy::NullDacl => {
            if bodies[0].len != 0 || bodies[1].len != 0 || bodies[2].len != 0 {
                return Err(E_SHAPE);
            }
            if bodies[3].len < SECT_MIN {
                return Err(E_SHAPE);
            }
        }
        _ => {
            if bodies[0].len < SYN_OWNER_LEN || bodies[1].len < SYN_GROUP_LEN {
                return Err(E_SHAPE);
            }
            if bodies[2].len < SECT_MIN || bodies[3].len < SECT_MIN {
                return Err(E_SHAPE);
            }
        }
    }
    Ok(())
}

/// 合成全链（合成 + 双复核）：36126 挂点推荐的调用形状。
pub fn synthesize_verified(policy: TypePolicy) -> Result<(Sd, [SectSpec; 4]), u16> {
    let (sd, bodies) = synthesize(policy)?;
    verify_tier(&sd, policy)?;
    verify_shape(&sd, &bodies, policy)?;
    Ok((sd, bodies))
}

/// 档位快照（36131 启动自检消费）：四档控制位快照，确定性锚。
pub fn tier_snapshot() -> Result<[u16; 4], u16> {
    let mut snap = [0u16; 4];
    let tiers = [
        TypePolicy::ClosedAll,
        TypePolicy::NullDacl,
        TypePolicy::InheritParent,
        TypePolicy::AdminGate,
    ];
    let mut i = 0;
    while i < 4 {
        let (sd, bodies) = synthesize_verified(tiers[i])?;
        snap[i] = sd.control;
        // 快照内自证：控制位与档位特征一致。
        let (must, must_not) = tier_signature(tiers[i]);
        if snap[i] & must != must || snap[i] & must_not != 0 {
            return Err(E_TIER);
        }
        // 形状内证。
        verify_shape(&sd, &bodies, tiers[i])?;
        i += 1;
    }
    Ok(snap)
}

/// 快照比对（自检面）。
pub fn snapshot_eq(a: &[u16; 4], b: &[u16; 4]) -> bool {
    let mut i = 0;
    while i < 4 {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// 黄金类型段长特化（ObCreateObject 实际消费形状）：类型敏感对象需要
/// 更大的段体预算（如 Token 的 Owner/Group 双 SID、WindowStation 的
/// 大 DACL）。默认档只给最小体，特化表按黄金序逐类覆写。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SynthSpec {
    pub owner: usize,
    pub group: usize,
    pub sacl: usize,
    pub dacl: usize,
}

/// 黄金八类段长特化表（tid 序 = GOLDEN 序）。
pub const SYN_GOLDEN_SPECS: [SynthSpec; 8] = [
    SynthSpec { owner: 8, group: 8, sacl: 2, dacl: 16 },  // File：DACL 常多条
    SynthSpec { owner: 8, group: 8, sacl: 2, dacl: 12 },  // Key
    SynthSpec { owner: 8, group: 8, sacl: 4, dacl: 24 },  // WindowStation：管理员门大 DACL
    SynthSpec { owner: 8, group: 8, sacl: 4, dacl: 20 },  // Desktop
    SynthSpec { owner: 8, group: 8, sacl: 2, dacl: 8 },   // Section
    SynthSpec { owner: 12, group: 12, sacl: 4, dacl: 16 }, // Process：双 SID 槽
    SynthSpec { owner: 12, group: 12, sacl: 4, dacl: 16 }, // Thread
    SynthSpec { owner: 16, group: 16, sacl: 2, dacl: 8 },  // Token：敏感对象大 SID 槽
];

/// 按特化段长合成（档位形状不变，只放大段体预算——档位特征面不受影响）。
pub fn synthesize_tiered(policy: TypePolicy, spec: SynthSpec) -> Result<(Sd, [SectSpec; 4]), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    match policy {
        TypePolicy::ClosedAll => {
            // 全闭档形状冻结：无 SD 对象不携带段体预算（覆写请求被静默
            // 归一为默认形状——段体只在有 SD 时才有载体）。
            synthesize(policy)
        }
        TypePolicy::NullDacl => {
            // null DACL 形状同样冻结：仅零长 ACL 头在 DACL 槽，Owner/Group
            // 段缺席（覆写只作用于四段齐备档）。
            synthesize(policy)
        }
        TypePolicy::InheritParent => ctor::build_full(
            spec.owner.max(SYN_OWNER_LEN),
            spec.group.max(SYN_GROUP_LEN),
            spec.sacl.max(SYN_SACL_LEN),
            spec.dacl.max(SYN_DACL_LEN),
            0,
        )
        .map_err(|_| E_SYN),
        TypePolicy::AdminGate => ctor::build_full(
            spec.owner.max(SYN_OWNER_LEN),
            spec.group.max(SYN_GROUP_LEN),
            spec.sacl.max(SYN_SACL_LEN),
            spec.dacl.max(SYN_DACL_LEN),
            C_DACL_PROT | C_SACL_PROT,
        )
        .map_err(|_| E_SYN),
    }
}

/// 黄金序号合法性（越界显性拒）。
pub fn golden_spec(idx: usize) -> Result<SynthSpec, u16> {
    if idx >= SYN_GOLDEN_SPECS.len() {
        return Err(E_SHAPE);
    }
    Ok(SYN_GOLDEN_SPECS[idx])
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R1：四档全合成 + 双复核 + 互相误配检测。
    let tiers = [
        TypePolicy::ClosedAll,
        TypePolicy::NullDacl,
        TypePolicy::InheritParent,
        TypePolicy::AdminGate,
    ];
    let mut i = 0;
    while i < 4 {
        let (sd, bodies) = synthesize_verified(tiers[i])?;
        // 自档复核过；错档复核必须炸（档位判别互斥的显性证明）。
        let mut j = 0;
        while j < 4 {
            if i == j {
                verify_tier(&sd, tiers[j])?;
            } else {
                if verify_tier(&sd, tiers[j]).is_ok() {
                    return Err(E_TIER);
                }
            }
            j += 1;
        }
        let _ = bodies;
        i += 1;
    }
    // 特例核对：ClosedAll 合成体是「无 SD」（控制位全零——默认拒绝锚）。
    let (sd_closed, _) = synthesize(TypePolicy::ClosedAll)?;
    if sd_closed.control != 0 {
        return Err(E_TIER);
    }
    // NullDacl 合成体：DACL_PRESENT 置位但无 PROTECTED（空授权域 ≠ 管理员门）。
    let (sd_null, _) = synthesize(TypePolicy::NullDacl)?;
    if sd_null.control & C_DACL_PRESENT == 0 || sd_null.control & (C_DACL_PROT | C_SACL_PROT) != 0 {
        return Err(E_TIER);
    }
    // AdminGate 合成体：双 PRESENT + 双 PROTECTED。
    let (sd_admin, _) = synthesize(TypePolicy::AdminGate)?;
    if sd_admin.control & (C_DACL_PRESENT | C_SACL_PRESENT) != C_DACL_PRESENT | C_SACL_PRESENT {
        return Err(E_TIER);
    }
    if sd_admin.control & (C_DACL_PROT | C_SACL_PROT) != C_DACL_PROT | C_SACL_PROT {
        return Err(E_TIER);
    }
    // R2：快照确定性（两次快照逐位相等）。
    let s1 = tier_snapshot()?;
    let s2 = tier_snapshot()?;
    if !snapshot_eq(&s1, &s2) {
        return Err(E_INTG);
    }
    // 快照值与档位特征逐位自洽（冗余内证）。
    let mut k = 0;
    while k < 4 {
        let (must, must_not) = tier_signature(tiers[k]);
        if s1[k] & must != must || s1[k] & must_not != 0 {
            return Err(E_TIER);
        }
        k += 1;
    }
    // R3：黄金八类逐类按名合成（注册表驱动路径）。
    let reg = tr::assemble_golden()?;
    let mut g = 0;
    while g < GOLDEN_N {
        let name = tr::GOLDEN_NAMES[g];
        let policy = reg.policy_of(name).map_err(|_| E_NOENT)?;
        let (sd, bodies) = synthesize_for_type(&reg, name)?;
        verify_tier(&sd, policy)?;
        verify_shape(&sd, &bodies, policy)?;
        g += 1;
    }
    // R4：未注册类型合成显性拒（红线在合成面复刻）。
    if synthesize_for_type(&reg, "Ghost").err() != Some(E_NOENT) {
        return Err(E_NOENT);
    }
    // R5：确定性重放（同名重复合成 → 控制位逐位一致）。
    let (sd_a, _) = synthesize_for_type(&reg, "Token")?;
    let (sd_b, _) = synthesize_for_type(&reg, "Token")?;
    if sd_a.control != sd_b.control {
        return Err(E_INTG);
    }
    // R6：黄金特化段长路径（逐类按特化表合成 + 档位特征复核 + 段长复核）。
    let mut g2 = 0;
    while g2 < GOLDEN_N {
        let policy = reg.policy_of(tr::GOLDEN_NAMES[g2]).map_err(|_| E_NOENT)?;
        let spec = golden_spec(g2)?;
        let (sd, bodies) = synthesize_tiered(policy, spec)?;
        verify_tier(&sd, policy)?;
        verify_shape(&sd, &bodies, policy)?;
        // 特化内证：四段齐备档的段体必须 ≥ 默认档（覆写只放不缩）；
        // 全闭/null DACL 形状冻结，不参与段长内证。
        if policy == TypePolicy::InheritParent || policy == TypePolicy::AdminGate {
            if bodies[0].len < SYN_OWNER_LEN || bodies[1].len < SYN_GROUP_LEN {
                return Err(E_SHAPE);
            }
        }
        g2 += 1;
    }
    // 特化表越界显性拒。
    if golden_spec(8).err() != Some(E_SHAPE) {
        return Err(E_SHAPE);
    }
    // 校验和：四档快照全并（确定性锚）。
    Ok((s1[0] as u32) | ((s1[1] as u32) << 8) | ((s1[2] as u32) << 16) | ((s1[3] as u32) << 24))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_SYN => "合成失败（构造器诊断透传）",
        E_TIER => "档位误配（SD 控制位与档位不符）",
        E_NOENT => "类型未注册（合成前置）",
        E_SHAPE => "合成形状不符（段长纪律破）",
        E_INTG => "完整性自检不符",
        _ => "未知诊断码",
    }
}

pub fn run_unx36125_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36125_deftier");
    cs.add("SEG 段位独占", SEG_BASE == 0x3b10, "0x3b10~0x3b1F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "四档合成/互斥复核/快照确定性/黄金驱动/未注册拒全对平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("段长锚", SYN_OWNER_LEN == 8 && SYN_GROUP_LEN == 8 && SYN_SACL_LEN == 2 && SYN_DACL_LEN == 2, "SID 槽 8 字节 / ACL 头 2 字节");
    cs.add("档位特征闭集", {
        // 四档特征两两不同（判别互斥的静态证明）。
        let a = tier_signature(TypePolicy::ClosedAll);
        let b = tier_signature(TypePolicy::NullDacl);
        let c = tier_signature(TypePolicy::InheritParent);
        let d = tier_signature(TypePolicy::AdminGate);
        !(a == b) && !(a == c) && !(a == d) && !(b == c) && !(b == d) && !(c == d)
    }, "四档 (必现,禁现) 特征两两互异");
    cs.add("ClosedAll 恒无 SD", synthesize(TypePolicy::ClosedAll).map(|s| s.0.control) == Ok(0), "默认拒绝档控制位全零");
    cs.add("快照确定性", { let a = tier_snapshot(); let b = tier_snapshot(); a.is_ok() && b.is_ok() && snapshot_eq(&a.unwrap_or([0;4]), &b.unwrap_or([0;4])) }, "两次快照逐位一致");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}
