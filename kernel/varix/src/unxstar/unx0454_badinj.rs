//! unx0454_badinj —— UNX-F0454 坏表注入演练（校验破坏检出）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0454 · 坏表注入演练 · 目标 240 行
//! 判据原文：UNX-F0454-J1 注入三类坏表并核检出率：坏一 校
//!           验和翻转（单字节 +delta）→ 必检出；坏二 签名破
//!           坏（4 字节中任一字节非可打印）→ 必检出；坏三
//!           长度域虚报（declared > 实际缓冲）→ 必检出；账
//!           二 检出器纪律：三类检查独立（一类坏只触发一类
//!           立案码，不误伤他类）；账三 修复路径：对坏一提
//!           供补值器重算 checksum 使表复生（一次性修复）；
//!           坏二坏三不可修复（结构性损坏，仅立案）；账四
//!           注入演练覆盖率：三类各注入 ≥ 1 例且全部检出；
//!           账五 立案不传播：单表坏不影响其余表检出结论。
//! 组件：三通道检出器 + 补值修复器 + 演练计数器 + 隔离核。
//! 职责定位：坏表注入是 ACPI 探测器的「疫苗试验」——不注射
//!   病原就不知道免疫系统灵不灵；固件产线每年都有 checksum
//!   忘写、签名打错的真坏表流出，检出器必须在启动早期拦截，
//!   而不是等到 AML 解释器在坏字节码里梦游。
//! 性能逐项分解：校验和 O(len)；签名核 O(4)；演练 O(k·len)。
//! 诊断码段：0x342C0（0x342C1~0x342C9 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断码段位（u32 高段独占 0x342C0 起）。
pub const SEG_BASE: u32 = 0x342C0;
pub const XC_CSUM: u32 = SEG_BASE + 1; // 校验和破坏
pub const XC_SIGN: u32 = SEG_BASE + 2; // 签名破坏
pub const XC_DECL: u32 = SEG_BASE + 3; // 长度虚报
pub const XC_UNFIX: u32 = SEG_BASE + 4; // 不可修复类

/// 表头最小长度（36 字节）。
pub const HDR_MIN: usize = 36;

/// 签名可打印核（复用 0451 语义）。
fn sign_ok(s: &[u8]) -> bool {
    s.len() >= 4 && s[..4].iter().all(|b| (0x20..=0x7E).contains(b))
}

/// 整表校验和（8 位和 ≡ 0）。
pub fn csum(raw: &[u8], len: usize) -> u8 {
    let mut s = 0u8;
    for b in raw.iter().take(len) {
        s = s.wrapping_add(*b);
    }
    s
}

/// 三通道检出器：依序判 校验和 → 签名 → 长度；独立立案码。
/// 检查顺序刻意先签名后校验和？——不：签名是「身份」，长度
/// 是「边界」，校验和是「内容」。内容核依赖边界（len），故
/// 顺序：签名 → 长度 → 校验和。
pub fn detect(raw: &[u8]) -> Result<(), u32> {
    if raw.len() < HDR_MIN {
        return Err(XC_DECL);
    }
    if !sign_ok(&raw[..4]) {
        return Err(XC_SIGN);
    }
    let declared = u32::from_le_bytes([raw[4], raw[5], raw[6], raw[7]]) as usize;
    if declared < HDR_MIN || declared > raw.len() {
        return Err(XC_DECL);
    }
    if csum(raw, declared) != 0 {
        return Err(XC_CSUM);
    }
    Ok(())
}

/// 补值修复器：重算 checksum（offset 9）使整表和归零。
/// 只对「仅校验和坏」的表有效；其他坏类返回不可修复。
pub fn repair_csum(raw: &mut [u8]) -> Result<(), u32> {
    if raw.len() < HDR_MIN {
        return Err(XC_DECL);
    }
    let declared = u32::from_le_bytes([raw[4], raw[5], raw[6], raw[7]]) as usize;
    if declared < HDR_MIN || declared > raw.len() {
        return Err(XC_UNFIX);
    }
    if !sign_ok(&raw[..4]) {
        return Err(XC_UNFIX);
    }
    // 先清 checksum 位再求和，得补值。
    raw[9] = 0;
    let s = csum(raw, declared);
    raw[9] = s.wrapping_neg();
    Ok(())
}

/// 演练计数器：注入集 →（注入数、检出数、漏检数）。
pub fn drill(cases: &[Vec<u8>]) -> (usize, usize, usize) {
    let inj = cases.len();
    let det = cases.iter().filter(|c| detect(c).is_err()).count();
    (inj, det, inj - det)
}

// ─────────────────────────── 判据面 ───────────────────────────

/// 造一张合法样表（36+ 字节，校验和自洽）。
fn good_table(sign: &[u8; 4], extra: usize) -> Vec<u8> {
    let total = HDR_MIN + extra;
    let mut raw: Vec<u8> = Vec::new();
    raw.extend_from_slice(sign);
    raw.extend_from_slice(&(total as u32).to_le_bytes());
    raw.push(5); // revision
    raw.push(0); // checksum 位（后补）
    raw.extend_from_slice(b"VARIX1"); // OEM ID
    raw.extend_from_slice(b"VXPC"); // OEM Table ID
    raw.extend_from_slice(&0x2026_0101u32.to_le_bytes()); // OEM Rev
    raw.extend_from_slice(&0x1122_3344u32.to_le_bytes()); // Creator ID
    raw.extend_from_slice(&0x0000_0001u32.to_le_bytes()); // Creator Rev
    for i in 0..extra {
        raw.push((i % 251) as u8 + 1); // 净载荷非零
    }
    raw[9] = 0;
    let s = csum(&raw, total);
    raw[9] = s.wrapping_neg();
    raw
}

/// 判据入口：UNX-F0454 全族核。
pub fn run_unx0454_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0454");

    // 族一：合法表通过。
    let good = good_table(b"FACP", 24);
    cs.add("合法表过检", detect(&good).is_ok(), "基线");
    cs.add("校验和自洽", csum(&good, good.len()) == 0, "8 位和零");

    // 族二：坏一 校验和翻转检出。
    let mut c1 = good.clone();
    c1[20] = c1[20].wrapping_add(0x40); // 净载荷单字节 +delta
    cs.add("校验翻转检出", detect(&c1) == Err(XC_CSUM), "内容坏");
    cs.add("签名仍好", sign_ok(&c1[..4]), "身份未坏");
    let dl = u32::from_le_bytes([c1[4], c1[5], c1[6], c1[7]]) as usize;
    cs.add("长度仍好", dl == c1.len(), "边界未坏");

    // 族三：坏二 签名破坏检出。
    let mut c2 = good.clone();
    c2[2] = 0xE2; // 'C' → 不可打印
    cs.add("签名破坏检出", detect(&c2) == Err(XC_SIGN), "身份坏");
    cs.add("签名核独立", detect(&c2) != Err(XC_CSUM) || csum(&c2, dl) == 0, "不误报内容");

    // 族四：坏三 长度虚报检出。
    let mut c3 = good.clone();
    c3[4] = 0xFF; // declared 虚报
    c3[5] = 0xFF; c3[6] = 0xFF; c3[7] = 0x00;
    cs.add("长度虚报检出", detect(&c3) == Err(XC_DECL), "边界坏");
    let mut c4 = good_table(b"APIC", 0);
    c4.truncate(20); // 砍到 < 36
    cs.add("短表检出", detect(&c4) == Err(XC_DECL), "不足头长");

    // 族五：修复路径（坏一可复生）。
    let mut r1 = c1.clone();
    cs.add("补值修复成", repair_csum(&mut r1).is_ok(), "重算 checksum");
    cs.add("修复后过检", detect(&r1).is_ok(), "复生");
    cs.add("修复后和零", csum(&r1, r1.len()) == 0, "8 位和归零");
    cs.add("修复不改载荷", r1[20] == c1[20], "净载荷保真");

    // 族六：坏二坏三不可修复。
    let mut r2 = c2.clone();
    cs.add("签名坏不可修", repair_csum(&mut r2) == Err(XC_UNFIX), "结构性");
    let mut r3 = c3.clone();
    cs.add("虚报不可修", repair_csum(&mut r3) == Err(XC_UNFIX), "结构性二");

    // 族七：演练计数。
    let cases: Vec<Vec<u8>> = vec![c1.clone(), c2.clone(), c3.clone(), c4];
    let (inj, det, miss) = drill(&cases);
    cs.add("演练四例", inj == 4, "三类+短表");
    cs.add("全检出", det == 4, "检出率 100%");
    cs.add("零漏检", miss == 0, "免疫成立");

    // 族八：立案不传播。
    let g2 = good_table(b"APIC", 8);
    cs.add("邻表不受染", detect(&g2).is_ok(), "独立检");
    let cases2: Vec<Vec<u8>> = vec![g2.clone(), c1.clone()];
    let (i2, d2, _) = drill(&cases2);
    cs.add("混编二出一", i2 == 2 && d2 == 1, "好表不误杀");

    // 族九：多次翻转仍检出。
    let mut c5 = good.clone();
    c5[15] ^= 0xFF; c5[16] ^= 0xFF; c5[17] ^= 0xFF;
    cs.add("多字节翻转检出", detect(&c5) == Err(XC_CSUM), "和必非零");

    cs
}
