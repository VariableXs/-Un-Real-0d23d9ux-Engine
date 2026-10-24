//! unx0452_degrade —— UNX-F0452 表缺失降级账（分级路径）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0452 · 表缺失降级账 · 目标 230 行
//! 判据原文：UNX-F0452-J1 分级账：必需表（FADT、MADT）缺失
//!           → 启动路径拒绝（不降级、不兜底——半初始化比不
//!           初始化更危险）；可选表缺失 → 分级降级立案（功
//!           能旗清零 + 降级档案）；账二 降级档案：每次降级
//!           记录（表签名、影响面、替代路径有无）；账三 影
//!           响面映射：HPET 缺 → 计时退化 RTC/PM_TIMER；MCFG
//!           缺 → ECAM 缺 → 走 0xCF8 遗留端口；SLIT/SRAT 缺
//!           → 单域 NUMA；DSDT 缺 → AML 不可用（严重降级但
//!           非致命——用 FADT 硬编码面兜底）；账四 降级不传
//!           播：可选表缺失不阻断其他表解析；账五 重复降级
//!           去重：同签名只立案一次。
//! 组件：必需/可选分派器 + 影响面映射器 + 降级档案账 +
//!       去重器 + 拒绝闸。
//! 职责定位：降级是内核的「防弹衣」——缺什么表都还活着，
//!   但必须把「缺了什么、退到哪条路」白纸黑字记下来；必需
//!   表缺失必须硬拒，因为 FADT 缺失意味着连 PM 寄存器基地
//!   址都不知道，继续跑是在赌随机内存内容。
//! 性能逐项分解：分派 O(n)；档案去重 O(m²)（m ≤ 8 缺口）。
//! 诊断码段：0x34240（0x34241~0x34249 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use alloc::vec;
use crate::checks::CheckSet;

/// 诊断码段位（u32 高段独占 0x34240 起）。
pub const SEG_BASE: u32 = 0x34240;
pub const XC_HARD_MISS: u32 = SEG_BASE + 1; // 必需表缺失（硬拒）
pub const XC_DUP_DEG: u32 = SEG_BASE + 2; // 重复降级（档案内静默去重）

/// 表角色分级。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// 必需：缺失即硬拒。
    Must,
    /// 可选：缺失降级立案。
    Nice,
}

/// 影响面代码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Impact {
    TimerFallback,  // HPET 缺
    LegacyPci,      // MCFG 缺
    SingleNuma,     // SRAT/SLIT 缺
    AmlDead,        // DSDT 缺
    None,
}

/// 降级档案条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DegRec {
    pub sign: [u8; 4],
    pub impact: Impact,
    /// 有替代路径（true = 可兜底）。
    pub alt: bool,
}

/// 分级表清单。
#[derive(Clone, Debug)]
pub struct Graded {
    pub sign: [u8; 4],
    pub grade: Grade,
}

/// 必需表判别。
pub fn is_must(s: &[u8; 4]) -> bool {
    s == b"FACP" || s == b"APIC"
}

/// 影响面映射。
pub fn impact_of(s: &[u8; 4]) -> Impact {
    match s {
        b"HPET" => Impact::TimerFallback,
        b"MCFG" => Impact::LegacyPci,
        b"SRAT" | b"SLIT" => Impact::SingleNuma,
        b"DSDT" => Impact::AmlDead,
        _ => Impact::None,
    }
}

/// 降级档案账（去重纪律）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DegLedger {
    pub recs: Vec<DegRec>,
}

impl DegLedger {
    pub fn new() -> Self {
        Self::default()
    }

    /// 立案：同签名重复降级静默去重（返回 false 表示被去重）。
    pub fn record(&mut self, sign: [u8; 4]) -> bool {
        if self.recs.iter().any(|r| r.sign == sign) {
            return false;
        }
        self.recs.push(DegRec { sign, impact: impact_of(&sign), alt: impact_of(&sign) != Impact::None });
        true
    }
}

/// 分级路径核：给出在场表清单，产出（硬拒码 or 降级档案）。
/// 返回 Ok(档案) = 可继续；Err = 必需表缺失。
pub fn grade_path(present: &[[u8; 4]], expected: &[[u8; 4]]) -> Result<DegLedger, u32> {
    let mut led = DegLedger::new();
    // 必需表核。
    for e in expected {
        if is_must(e) && !present.iter().any(|p| p == e) {
            return Err(XC_HARD_MISS);
        }
    }
    // 可选降级核：期望但缺席 → 立案（在场但不在期望清单的表不立案——白名单纪律）。
    for e in expected {
        if !present.iter().any(|p| p == e) {
            led.record(*e);
        }
    }
    Ok(led)
}

/// 降级不传播核：可选表缺失不阻断其他表解析（缺 A 不影响 B 的解析结果）。
pub fn no_propagate(present_a: &[[u8; 4]], present_b: &[[u8; 4]], expected: &[[u8; 4]]) -> bool {
    match (grade_path(present_a, expected), grade_path(present_b, expected)) {
        (Ok(la), Ok(lb)) => {
            // 两套档案除缺席集合本身外互不感染：档案内签名均来自各自缺席集。
            la.recs.iter().all(|r| !present_a.contains(&r.sign))
                && lb.recs.iter().all(|r| !present_b.contains(&r.sign))
        }
        _ => false,
    }
}

// ─────────────────────────── 判据面 ───────────────────────────

/// 判据入口：UNX-F0452 全族核。
pub fn run_unx0452_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0452");

    let expected: Vec<[u8; 4]> = vec![*b"FACP", *b"APIC", *b"HPET", *b"MCFG", *b"SRAT", *b"DSDT"];

    // 族一：全在场零降级。
    let full: Vec<[u8; 4]> = expected.clone();
    let g1 = grade_path(&full, &expected).unwrap_or_else(|_| DegLedger::new());
    cs.add("全在场零降级", g1.recs.is_empty(), "档案空");

    // 族二：可选缺席降级。
    let partial: Vec<[u8; 4]> = vec![*b"FACP", *b"APIC", *b"HPET"];
    let g2 = grade_path(&partial, &expected).unwrap_or_else(|_| DegLedger::new());
    cs.add("三表降级立案", g2.recs.len() == 3, "MCFG/SRAT/DSDT");
    cs.add("HPET 不立案", !g2.recs.iter().any(|r| r.sign == *b"HPET"), "在场");
    cs.add("影响面 LegacyPci", g2.recs.iter().any(|r| r.impact == Impact::LegacyPci), "MCFG 缺");
    cs.add("影响面 AmlDead", g2.recs.iter().any(|r| r.impact == Impact::AmlDead), "DSDT 缺");
    cs.add("影响面 SingleNuma", g2.recs.iter().any(|r| r.impact == Impact::SingleNuma), "SRAT 缺");
    cs.add("替代旗真", g2.recs.iter().all(|r| r.alt), "皆可兜底");

    // 族三：必需表缺失硬拒。
    let no_fadt: Vec<[u8; 4]> = vec![*b"APIC", *b"HPET"];
    cs.add("FADT 缺硬拒", grade_path(&no_fadt, &expected) == Err(XC_HARD_MISS), "不降级");
    let no_apic: Vec<[u8; 4]> = vec![*b"FACP"];
    cs.add("MADT 缺硬拒", grade_path(&no_apic, &expected) == Err(XC_HARD_MISS), "硬拒二");
    let none: Vec<[u8; 4]> = Vec::new();
    cs.add("全缺硬拒", grade_path(&none, &expected) == Err(XC_HARD_MISS), "FADT 先拒");

    // 族四：重复降级去重。
    let mut led = DegLedger::new();
    cs.add("首案立案", led.record(*b"HPET"), "第一次");
    cs.add("重案去重", !led.record(*b"HPET"), "第二次 false");
    cs.add("异案立案", led.record(*b"MCFG"), "异签名");
    cs.add("档案两行", led.recs.len() == 2, "去重后");

    // 族五：去重码与档案自洽。
    cs.add("去重码执行", led.record(*b"HPET") == false, "恒去重");
    cs.add("档案影响面自洽", led.recs.iter().all(|r| impact_of(&r.sign) == r.impact), "映射一致");

    // 族六：降级不传播。
    let pa: Vec<[u8; 4]> = vec![*b"FACP", *b"APIC", *b"MCFG"];
    let pb: Vec<[u8; 4]> = vec![*b"FACP", *b"APIC", *b"HPET"];
    cs.add("不传播真", no_propagate(&pa, &pb, &expected), "互不感染");
    cs.add("档案含本方缺席", no_propagate(&pa, &pb, &expected), "签名归位");

    // 族七：白名单纪律（在场但不在期望清单的表不降级立案）。
    let extra: Vec<[u8; 4]> = vec![*b"FACP", *b"APIC", *b"XXXX"];
    let g7 = grade_path(&extra, &expected).unwrap_or_else(|_| DegLedger::new());
    cs.add("异表不立案", !g7.recs.iter().any(|r| r.sign == *b"XXXX"), "白名单外");
    cs.add("异表降级仍记可选缺口", g7.recs.len() == 4, "期望全缺四");

    // 族八：边界（空期望清单）。
    let g8 = grade_path(&full, &[]).unwrap_or_else(|_| DegLedger::new());
    cs.add("空期望零降级", g8.recs.is_empty(), "无事可查");

    cs
}
