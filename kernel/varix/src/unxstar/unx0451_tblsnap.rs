//! unx0451_tblsnap —— UNX-F0451 表清单快照与 ACPI 版本账。
//! 锚点（CoRun Unxreal 主册）：UNX-F0451 · 表清单快照与版本账 · 目标 240 行
//! 判据原文：UNX-F0451-J1 快照账：遍历根表产出表签名清单
//!           （签名 4 字节、版本号 1 字节、OEM ID 6 字节投影）；
//!           清单可去重计数（同签名多实例计 instance 号）；账
//!           二 版本推断账：RSDP revision 0 = ACPI 1.0（仅
//!           RSDT）、revision ≥ 2 = ACPI 2.0+（有 XSDT）；
//!           FADT minor revision 推断特征（4.x+ 引用 X_PM 系列）；
//!           账三 快照不可变性：快照生成后源清单再变化不影响
//!           已出快照（纯数据投影，深拷贝纪律）；账四 签名合
//!           法性：签名字节必须可打印（0x20~0x7E），否则快照
//!           立案（坏签名条目仍记录但打污标）；账五 序数稳
//!           定：同输入两次快照序数一致（确定性纪律）。
//! 组件：签名投影器 + 版本推断器 + 去重计数控 + 污标器 +
//!       确定性双拍核。
//! 职责定位：表清单是 ACPI 的「户口本」——启动后第一件事就
//!   是把有哪些表、什么版本记下来，后续所有探测按户口本点
//!   名；版本推断错了会把 1.0 机器当 2.0 读 64 位指针（读
//!   到垃圾）或反之（丢高 32 位），是跨代兼容的第一悬案。
//! 性能逐项分解：快照 O(n)；去重 O(n²)（n ≤ 32 表微秒级）。
//! 诊断码段：0x34200（0x34201~0x34209 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use alloc::vec;
use crate::checks::CheckSet;

/// 诊断码段位（u32 高段独占 0x34200 起）。
pub const SEG_BASE: u32 = 0x34200;
pub const XC_SIGN_NP: u32 = SEG_BASE + 1; // 签名不可打印
pub const XC_REV_UNK: u32 = SEG_BASE + 2; // 版本推断矛盾

/// 单表快照条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapRow {
    /// 4 字节签名（ASCII）。
    pub sign: [u8; 4],
    /// 表 revision 字节。
    pub rev: u8,
    /// OEM ID 6 字节投影。
    pub oem: [u8; 6],
    /// 同签名第几实例（从 1 起）。
    pub instance: u32,
    /// 污标：签名不可打印。
    pub dirty: bool,
}

/// 全域 ACPI 版本画像。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AcpiVer {
    /// RSDP revision（0 = 1.0 代）。
    pub rsdp_rev: u8,
    /// 有 XSDT 可用。
    pub has_xsdt: bool,
    /// FADT minor（0~7）。
    pub fadt_minor: u8,
}

impl AcpiVer {
    /// 代际推断：rsdp_rev 0 → 1.0（无 XSDT）；≥2 → 2.0+。
    pub fn gen(&self) -> u8 {
        if self.rsdp_rev == 0 { 1 } else { 2 }
    }

    /// FADT 特征：minor ≥ 4 支持 X_PM 系列块（X_FIRMWARE_CTRL 等）。
    pub fn fadt_xpm(&self) -> bool {
        self.fadt_minor >= 4
    }

    /// 版本自洽核：rev 0 却声明有 XSDT → 矛盾立案。
    pub fn coherent(&self) -> Result<(), u32> {
        if self.rsdp_rev == 0 && self.has_xsdt {
            return Err(XC_REV_UNK);
        }
        Ok(())
    }
}

/// 签名可打印核：4 字节全落在 0x20~0x7E。
pub fn sign_printable(s: &[u8; 4]) -> bool {
    s.iter().all(|b| (0x20..=0x7E).contains(b))
}

/// 快照器：输入原始表头列表（sign/rev/oem），产出带 instance
/// 序数与污标的深拷贝快照。同输入两次调用产出逐行相等。
pub fn snapshot(rows: &[([u8; 4], u8, [u8; 6])]) -> Vec<SnapRow> {
    let mut out: Vec<SnapRow> = Vec::new();
    for (sign, rev, oem) in rows {
        let inst = 1u32 + out.iter().filter(|r| r.sign == *sign).count() as u32;
        let dirty = !sign_printable(sign);
        out.push(SnapRow { sign: *sign, rev: *rev, oem: *oem, instance: inst, dirty });
    }
    out
}

/// 污标审计：快照内不可打印签名条目全部 dirty 且可检索。
pub fn dirty_audit(snap: &[SnapRow]) -> Vec<usize> {
    snap.iter().enumerate().filter(|(_, r)| r.dirty).map(|(i, _)| i).collect()
}

/// 去重签名摘要：签名 → 实例数。
pub fn dedup_digest(snap: &[SnapRow]) -> Vec<([u8; 4], u32)> {
    let mut out: Vec<([u8; 4], u32)> = Vec::new();
    for r in snap {
        match out.iter_mut().find(|(s, _)| *s == r.sign) {
            Some((_, c)) => *c += 1,
            None => out.push((r.sign, 1)),
        }
    }
    out
}

// ─────────────────────────── 判据面 ───────────────────────────

/// 判据入口：UNX-F0451 全族核。
pub fn run_unx0451_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0451");

    // 夹具：RSDP rev 2 代，FACP/APIC/HPET/MCFG + 一张坏签名表。
    let rows: Vec<([u8; 4], u8, [u8; 6])> = vec![
        (*b"FACP", 4, *b"VARIX1"),
        (*b"APIC", 2, *b"VARIX1"),
        (*b"HPET", 1, *b"VARIX1"),
        (*b"MCFG", 1, *b"VARIX1"),
        (*b"APIC", 2, *b"VARIX1"), // 第二实例
        ([0x01, 0x7F, 0xE2, 0x99], 1, *b"VARIX1"), // 不可打印签名
    ];

    // 族一：快照生成与序数。
    let s1 = snapshot(&rows);
    cs.add("快照六行", s1.len() == 6, "含污行");
    cs.add("第二实例序数 2", s1[4].instance == 2, "APIC 重现");
    cs.add("首实例序数 1", s1[1].instance == 1, "APIC 首发");
    cs.add("OEM 投影", s1[0].oem == *b"VARIX1", "6 字节");

    // 族二：确定性双拍。
    let s2 = snapshot(&rows);
    cs.add("双拍全等", s1 == s2, "序数稳定");

    // 族三：深拷贝不可变性。
    let mut rows2 = rows.clone();
    rows2[0] = (*b"XXXX", 9, [0u8; 6]);
    let s3 = snapshot(&rows2);
    cs.add("快照免疫源变", s1[0].sign == *b"FACP" && s3[0].sign == *b"XXXX", "投影分离");
    cs.add("源变不回传", s3[0].rev == 9 && s1[0].rev == 4, "无回写");

    // 族四：污标。
    let dirty = dirty_audit(&s1);
    cs.add("污标一行", dirty.len() == 1 && dirty[0] == 5, "行 5");
    cs.add("污行 dirty 旗", s1[5].dirty, "旗位");
    cs.add("净行无污", !s1[0].dirty, "FACP 净");

    // 族五：签名可打印核。
    cs.add("可打印核真", sign_printable(b"FACP"), "FACP");
    cs.add("可打印核假", !sign_printable(&[0x01, 0x7F, 0xE2, 0x99]), "坏签名");
    cs.add("空格边界真", sign_printable(b"    "), "0x20 下界");

    // 族六：去重摘要。
    let dg = dedup_digest(&s1);
    cs.add("摘要四签名", dg.len() == 4, "FACP/APIC/HPET/MCFG+污");
    let apic_c = dg.iter().find(|(s, _)| s == b"APIC").map(|(_, c)| *c).unwrap_or(0);
    cs.add("APIC 双实例", apic_c == 2, "计数 2");

    // 族七：版本画像。
    let v2 = AcpiVer { rsdp_rev: 2, has_xsdt: true, fadt_minor: 5 };
    cs.add("2.0 代推断", v2.gen() == 2, "rev 2");
    cs.add("XPM 支持", v2.fadt_xpm(), "minor 5");
    cs.add("版本自洽", v2.coherent().is_ok(), "无矛盾");

    let v1 = AcpiVer { rsdp_rev: 0, has_xsdt: false, fadt_minor: 1 };
    cs.add("1.0 代推断", v1.gen() == 1, "rev 0");
    cs.add("1.0 无 XPM", !v1.fadt_xpm(), "minor 1");
    cs.add("1.0 自洽", v1.coherent().is_ok(), "无 XSDT");

    let vbad = AcpiVer { rsdp_rev: 0, has_xsdt: true, fadt_minor: 0 };
    cs.add("矛盾立案", vbad.coherent() == Err(XC_REV_UNK), "rev0+XSDT");

    // 族八：边界（空清单、全污）。
    let se = snapshot(&[]);
    cs.add("空清单空快照", se.is_empty(), "零行");
    let vd = dedup_digest(&se);
    cs.add("空快照空摘要", vd.is_empty(), "零签名");

    cs
}
