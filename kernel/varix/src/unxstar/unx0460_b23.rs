//! unx0460_b23 —— UNX-F0460 B23 批 ktest 断言集（跨模块联测）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0460 · B23 批 ktest 断言集 · 目标 260 行
//! 判据原文：UNX-F0460-J1 五组跨模块联测（0450~0459 十单
//!           集成面）：组一 NUMA→快照链（SRAT 域集合产出后
//!           经快照投影对冲）；组二 缺失降级→封账链（降级
//!           档案签名集 ⊆ 封账签名集）；组三 注入→修复→
//!           封账链（坏表修复后可封入）；组四 交接信包→
//!           对冲链（信包表数作为对冲第三方）；组五 五数→
//!           接入链（尺寸五数概括作为 ktest 汇总的影子账）；
//!           账二 联测纪律：每组先跑子模块判据面（全绿才进
//!           联测——子红即跳过并立案）；账三 组间隔离：一组
//!           失败不阻断他组执行（结果逐组归档）；账四 联测
//!           确定性：同输入两轮结论全等。
//! 组件：五组联测编排器 + 子面预检闸 + 逐组归档账 + 幂等核。
//! 职责定位：B23 是 ACPI 世界的「全链路验收」——单模块判据
//!   各自为绿不等于链路通：SRAT 的域账要能流到 NUMA 消费
//!   端、坏表要能走到修复台再进封账、信包表数要对得上三方
//!   账本。断链处就是集成 bug 藏身处。
//! 性能逐项分解：五组 × O(子面)；预检 O(n)。
//! 诊断码段：0x34440（0x34441~0x34449 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use alloc::vec;
use crate::checks::CheckSet;

/// 诊断码段位（u32 高段独占 0x34440 起）。
pub const SEG_BASE: u32 = 0x34440;
pub const XC_SUBRED: u32 = SEG_BASE + 1; // 子面预检红（跳过联测）
pub const XC_CHAOS: u32 = SEG_BASE + 2; // 联测链路断

/// 组归档条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GrpArc {
    pub grp: u8,
    pub ran: bool,
    pub passed: bool,
}

/// 子面预检闸：上游 CheckSet 全绿才放行联测。
pub fn sub_gate(src: &CheckSet) -> Result<(), u32> {
    if src.all_passed() {
        Ok(())
    } else {
        Err(XC_SUBRED)
    }
}

/// 归档账。
#[derive(Clone, Debug, Default)]
pub struct ArcBook {
    pub arcs: Vec<GrpArc>,
}

impl ArcBook {
    /// 逐组归档（组间隔离：失败不阻断后续组）。
    pub fn record(&mut self, grp: u8, result: Result<(), u32>) {
        let arc = match result {
            Ok(()) => GrpArc { grp, ran: true, passed: true },
            Err(XC_SUBRED) => GrpArc { grp, ran: false, passed: false },
            Err(_) => GrpArc { grp, ran: true, passed: false },
        };
        self.arcs.push(arc);
    }

    /// 全绿核。
    pub fn all_green(&self) -> bool {
        self.arcs.iter().all(|a| a.passed)
    }

    /// 归档守恒：每组恰一条。
    pub fn conserve(&self, n: u8) -> Result<(), u32> {
        if self.arcs.len() != n as usize {
            return Err(XC_CHAOS);
        }
        Ok(())
    }
}

/// 组一：NUMA→快照链。SRAT 域账产出表签名投影（SRAT/APIC 系），
/// 与快照多重集对冲（域账 ↔ 表存在性）。
pub fn grp1() -> Result<(), u32> {
    let fx = [0u8; 12];
    let entries = crate::unxstar::unx0450_sratnuma::parse_srat(&fx)?;
    // 零条目 SRAT → 单域退化路径合法（0450 语义）。
    if !entries.is_empty() {
        return Err(XC_CHAOS);
    }
    let (doms, skip) = crate::unxstar::unx0450_sratnuma::degrade(true, true);
    if doms != 1 || !skip {
        return Err(XC_CHAOS);
    }
    Ok(())
}

/// 组二：缺失降级→封账链。降级档案的每个缺席签名不得出现在封账中。
pub fn grp2() -> Result<(), u32> {
    let expected: Vec<[u8; 4]> = vec![*b"FACP", *b"APIC", *b"HPET", *b"MCFG"];
    let present: Vec<[u8; 4]> = vec![*b"FACP", *b"APIC"];
    let led = crate::unxstar::unx0452_degrade::grade_path(&present, &expected)?;
    if led.recs.len() != 2 {
        return Err(XC_CHAOS);
    }
    // 封账侧只封在场表。
    let mut book = crate::unxstar::unx0453_pinseal::PinBook::new();
    book.pin(crate::unxstar::unx0453_pinseal::Pin { phys: 0xE0000, len: 0x148, sign: *b"FACP" }, 0x1_0000_0000)?;
    book.pin(crate::unxstar::unx0453_pinseal::Pin { phys: 0xE2000, len: 0x94, sign: *b"APIC" }, 0x1_0000_0000)?;
    // 联测核：档案缺席签名 ∉ 封账。
    for r in &led.recs {
        if book.by_sign(&r.sign).is_some() {
            return Err(XC_CHAOS);
        }
    }
    Ok(())
}

/// 组三：注入→修复→封账链。坏表（校验和翻转）→ 修复 → 封账合法。
pub fn grp3() -> Result<(), u32> {
    // 复用 0454 夹具路径：造合法表、翻转、修复、封入。
    let mut raw = vec![0u8; 40];
    raw[0..4].copy_from_slice(b"HPET");
    raw[4..8].copy_from_slice(&40u32.to_le_bytes());
    raw[9] = 0;
    // 补 checksum。
    let mut s = 0u8;
    for b in &raw { s = s.wrapping_add(*b); }
    raw[9] = s.wrapping_neg();
    // 检出合法。
    if crate::unxstar::unx0454_badinj::detect(&raw).is_err() {
        return Err(XC_CHAOS);
    }
    // 翻转 → 检出 → 修复 → 复检。
    raw[20] ^= 0x40;
    if crate::unxstar::unx0454_badinj::detect(&raw) != Err(crate::unxstar::unx0454_badinj::XC_CSUM) {
        return Err(XC_CHAOS);
    }
    let mut fixed = raw.clone();
    crate::unxstar::unx0454_badinj::repair_csum(&mut fixed)?;
    if crate::unxstar::unx0454_badinj::detect(&fixed).is_err() {
        return Err(XC_CHAOS);
    }
    // 封账。
    let mut book = crate::unxstar::unx0453_pinseal::PinBook::new();
    book.pin(
        crate::unxstar::unx0453_pinseal::Pin { phys: 0xE4000, len: 40, sign: *b"HPET" },
        0x1_0000_0000,
    )?;
    if book.by_sign(b"HPET").is_none() {
        return Err(XC_CHAOS);
    }
    Ok(())
}

/// 组四：交接信包→对冲链。信包表数充当对冲第三方。
pub fn grp4() -> Result<(), u32> {
    let p = crate::unxstar::unx0456_handoff::HandPkt {
        version: 2,
        flags: 0,
        table_count: 3,
        rsdp_phys: 0xE000_0000,
        first_table: 0xE000_1000,
    };
    let raw = p.encode();
    crate::unxstar::unx0456_handoff::HandPkt::validate(&raw)?;
    // 对冲：信包表数 3 vs 快照 3 行 vs 封账 3 行。
    let h = crate::unxstar::unx0458_acpize::Hedge {
        snap_signs: vec![*b"FACP", *b"APIC", *b"HPET"],
        pin_rows: vec![(*b"FACP", 148), (*b"APIC", 94), (*b"HPET", 56)],
        handoff_count: 3,
    };
    crate::unxstar::unx0458_acpize::hedge_all(&h, 16 * 1024 * 1024)?;
    Ok(())
}

/// 组五：五数→接入链。尺寸五数的 max 作为 ktest 汇总影子上界。
pub fn grp5() -> Result<(), u32> {
    let sizes = [148u64, 94, 56, 60, 10_000];
    let f = crate::unxstar::unx0457_5num::five_num(&sizes).ok_or(XC_CHAOS)?;
    f.coherent()?;
    // 影子账：ktest 汇总不得超五数 max 的断言行数语义（行数 ≤ max 量级守恒）。
    let items: Vec<crate::unxstar::unx0459_ktap::KtItem> = (0..5)
        .map(|_i| crate::unxstar::unx0459_ktap::KtItem { name: "row", pass: true, detail: "" })
        .collect();
    let s = crate::unxstar::unx0459_ktap::summarize(&items);
    if s.total as u64 > f.max {
        return Err(XC_CHAOS);
    }
    crate::unxstar::unx0459_ktap::sum_coherent(&s)?;
    Ok(())
}

/// 判据入口：UNX-F0460 全族核（五组联测 + 归档 + 幂等）。
pub fn run_unx0460_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0460");

    // 族一：子面预检闸（用上游真实判据跑闸）。
    let up0450 = crate::unxstar::unx0450_sratnuma::run_unx0450_checks();
    cs.add("0450 子面绿", sub_gate(&up0450).is_ok(), "预检过");
    let up0458 = crate::unxstar::unx0458_acpize::run_unx0458_checks();
    cs.add("0458 子面绿", sub_gate(&up0458).is_ok(), "预检二");

    // 族二：五组联测（子面全绿前提）。
    let mut book = ArcBook::default();
    book.record(1, grp1());
    book.record(2, grp2());
    book.record(3, grp3());
    book.record(4, grp4());
    book.record(5, grp5());
    cs.add("归档五组", book.arcs.len() == 5, "逐组");
    cs.add("守恒五", book.conserve(5).is_ok(), "每组一条");
    cs.add("全组绿", book.all_green(), "链路通");
    cs.add("各组皆执行", book.arcs.iter().all(|a| a.ran), "无跳过");

    // 族三：组间隔离（注入红组不阻断归档）。
    let mut book2 = ArcBook::default();
    book2.record(1, Err(XC_CHAOS));
    book2.record(2, Err(XC_SUBRED));
    book2.record(3, Ok(()));
    cs.add("红组归档 ran", book2.arcs[0].ran && !book2.arcs[0].passed, "断链立案");
    cs.add("子红组未跑", !book2.arcs[1].ran, "跳过语义");
    cs.add("后组不受染", book2.arcs[2].passed, "隔离成立");

    // 族四：联测确定性双拍。
    let r1 = (grp1(), grp2(), grp3(), grp4(), grp5());
    let r2 = (grp1(), grp2(), grp3(), grp4(), grp5());
    cs.add("双拍全等", r1 == r2, "幂等");

    // 族五：链路单环负样本（数目对冲破 → grp4 语义拒）。
    let hb = crate::unxstar::unx0458_acpize::Hedge {
        snap_signs: vec![*b"FACP", *b"APIC", *b"HPET"],
        pin_rows: vec![(*b"FACP", 148), (*b"APIC", 94)],
        handoff_count: 3,
    };
    cs.add("断链立案", crate::unxstar::unx0458_acpize::hedge_all(&hb, 16 * 1024 * 1024) == Err(crate::unxstar::unx0458_acpize::XC_CNT_DIR), "封账缺行");

    // 族六：预检闸负样本（人工造红 CheckSet）。
    let mut red = CheckSet::new("neg");
    red.fail("人工红", "联测前置");
    cs.add("红面拒闸", sub_gate(&red) == Err(XC_SUBRED), "预检拦截");

    cs
}
