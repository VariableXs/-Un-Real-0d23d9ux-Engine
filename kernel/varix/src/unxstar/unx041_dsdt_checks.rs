//! UNX-F0041 判据层：DSDT 采集与校验链（锚点判据 J1 逐项映射）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal.md#UNX-F0041`
//!
//! **判据 J1（定位/校验和与 OEM ID 落账/坏表诊断不阻塞）→ 判据族**：
//! PTR 3 / CHK 3 / RPT 3 / META 3 = 12 项。
//!
//! # 本层核心纪律：判据侧独立重算，不向被测问答案
//!
//! FACP/DSDT 夹具**判据侧独立构造**（与被测 tests 的 fixture 分开落
//! 笔）；校验和补字节判据侧独立求和取负；X_DSDT/32 位回退链判据侧
//! 手算；坏表翻转字节位置判据侧自定；码段判据 `!=` 防自判死
//! （0x39/0x43/0x44/0x50..0x5D 全排除）。

use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;

use super::unx041_dsdt::*;

// ---------------------------------------------------------------------------
// 判据侧独立参照
// ---------------------------------------------------------------------------

/// 判据侧独立写死的复用声明句。
const EXP_NON_BLOCKING: &str =
    "坏表时引导报告列出诊断项且不阻塞引导——诊断项是报告不是中止，引导继续";

/// 全部诊断码（判据侧点名）。
const ALL_CODES: [DsCode; 5] = [
    DsCode::PTR_ZERO,
    DsCode::SIGNATURE_MISMATCH,
    DsCode::CHECKSUM_BAD,
    DsCode::LENGTH_OUT_OF_BOUNDS,
    DsCode::OEM_ID_EMPTY,
];

/// 预期判据条数（写前先数实挂 s.add 个数）。
const EXPECTED_CHECKS: usize = 12;

// ---------------------------------------------------------------------------
// 判据侧独立夹具（与被测 tests 分开落笔）
// ---------------------------------------------------------------------------

/// 判据侧独立构造 FACP：revision `rev`、X_DSDT=64 位字段、DSDT=32 位字段。
fn j_facp(rev: u8, x_dsdt: u64, dsdt32: u32) -> Vec<u8> {
    let mut v = vec![0u8; 148];
    v[0..4].copy_from_slice(b"FACP");
    v[8] = rev;
    v[140..148].copy_from_slice(&x_dsdt.to_le_bytes());
    v[40..44].copy_from_slice(&dsdt32.to_le_bytes());
    v
}

/// 判据侧独立构造 DSDT 表：合法校验和（判据侧独立求和取负）+ 自定 OEM。
/// corrupt 翻转在合法化**之后**——翻转被校验和补偿则坏表永远合法（夹具反噬）。
fn j_dsdt(oem: &[u8; 6], corrupt: bool) -> Vec<u8> {
    let mut v = vec![0u8; 96];
    v[0..4].copy_from_slice(b"DSDT");
    let len = v.len() as u32;
    v[4..8].copy_from_slice(&len.to_le_bytes());
    v[8] = 2;
    v[10..16].copy_from_slice(oem);
    v[36] = 0x10; // AML Op 垫一个字节（真实感）
    let sum = v.iter().fold(0u8, |a, &b| a.wrapping_add(b));
    v[9] = sum.wrapping_neg();
    if corrupt {
        v[50] ^= 0x5A;
    }
    v
}

const J_ADDR: u64 = 0x0000_9E00_0000;

// ---------------------------------------------------------------------------
// 主判据
// ---------------------------------------------------------------------------

/// unx041 域自检入口。
pub fn run_unx041_checks() -> CheckSet {
    let mut s = CheckSet::new("unx-dsdt");

    // ================= 一、指针定位（PTR） =================

    // PTR-1：X_DSDT 优先——revision 2 且 X_DSDT 非零 → 用 64 位指针。
    let f = j_facp(2, 0x0000_9E00_0000, 0x0009_A000);
    s.add(
        "U41-PTR-X_DSDT优先",
        locate_dsdt_ptr(&f) == Ok(0x0000_9E00_0000),
        "",
    );

    // PTR-2：回退链——revision 1（64 位字段不可信）→ 用 32 位 DSDT 字段。
    let f2 = j_facp(1, 0xFFFF_FFFF_FFFF, 0x0009_A000);
    s.add(
        "U41-PTR-32位回退",
        locate_dsdt_ptr(&f2) == Ok(0x0009_A000),
        "",
    );

    // PTR-3：双零指针 → PTR_ZERO（诊断不 panic）。
    let f3 = j_facp(2, 0, 0);
    s.add(
        "U41-PTR-双零诊断",
        locate_dsdt_ptr(&f3) == Err(DsCode::PTR_ZERO),
        "",
    );

    // ================= 二、校验和与 OEM ID 落账（CHK） =================

    // CHK-1：好表采集——校验和过 + OEM ID 逐字节提取落账。
    let t = j_dsdt(b"VARIX1", false);
    let chk1 = match collect(J_ADDR, &t) {
        Ok(info) => {
            info.checksum_ok
                && info.dsdt_addr == J_ADDR
                && info.oem_id[0] == b'V'
                && info.oem_id[5] == b'1'
                && info.length == 96
        }
        _ => false,
    };
    s.add("U41-CHK-好表采集落账", chk1, "");

    // CHK-2：坏表（判据侧翻转 50 偏移一字节）→ CHECKSUM_BAD，不 panic。
    let bad = j_dsdt(b"VARIX1", true);
    s.add(
        "U41-CHK-坏表校验和拒",
        collect(J_ADDR, &bad) == Err(DsCode::CHECKSUM_BAD),
        "",
    );

    // CHK-3：OEM ID 空（全空格）→ OEM_ID_EMPTY；签名错 → SIGNATURE_MISMATCH。
    let blank = j_dsdt(b"      ", false);
    let mut wrong_sig = j_dsdt(b"VARIX1", false);
    wrong_sig[0..4].copy_from_slice(b"XSDT");
    // 签名改后校验和也要重新合法化，保证命中的是签名判据而非校验和判据
    let sum = wrong_sig.iter().fold(0u8, |a, &b| a.wrapping_add(b));
    wrong_sig[9] = sum.wrapping_neg();
    s.add(
        "U41-CHK-OEM空与签名拒",
        collect(J_ADDR, &blank) == Err(DsCode::OEM_ID_EMPTY)
            && collect(J_ADDR, &wrong_sig) == Err(DsCode::SIGNATURE_MISMATCH),
        "",
    );

    // ================= 三、坏表诊断不阻塞（RPT） =================

    // RPT-1：boot_collect 好链——healthy 且 info 落账；坏链——诊断入报告不阻塞。
    let f_ok = j_facp(2, J_ADDR, 0);
    let r_good = boot_collect(&f_ok, Some((J_ADDR, &j_dsdt(b"VARIX1", false))));
    let r_bad = boot_collect(&f_ok, Some((J_ADDR, &j_dsdt(b"VARIX1", true))));
    let rpt1 = r_good.healthy && r_good.info.is_some() && r_good.diag_count == 0
        && !r_bad.healthy && r_bad.diag_count == 1 && r_bad.info.is_none();
    s.add("U41-RPT-好坏链报告对照", rpt1, "");

    // RPT-2：诊断容量截断如实标注——灌满 MAX_DIAGS+2 条后 truncated 且计数不虚。
    let mut r = DsdtReport::new();
    for _ in 0..(MAX_DIAGS + 2) {
        r.push(DsCode::PTR_ZERO);
    }
    let mut full = 0;
    for d in r.diags.iter() {
        if d.is_some() {
            full += 1;
        }
    }
    s.add(
        "U41-RPT-容量截断如实标注",
        r.diag_count == MAX_DIAGS + 2 && full == MAX_DIAGS && r.truncated,
        "",
    );

    // RPT-3：不阻塞引导语义声明逐字对拍（判据侧写死）。
    s.add(
        "U41-RPT-不阻塞语义对拍",
        NON_BLOCKING_NOTE == EXP_NON_BLOCKING,
        "",
    );

    // ================= 四、判据自检（META） =================

    // META-3：码段独占——全部 0x2Exx，且 != 0x39/0x43/0x44/0x50..0x5D（防自判死）。
    let section_ok = ALL_CODES.iter().all(|c| (c.code() >> 8) == 0x2E)
        && ALL_CODES.iter().all(|c| {
            let hi = c.code() >> 8;
            hi != 0x39 && hi != 0x43 && hi != 0x44 && !(0x50..=0x5D).contains(&hi)
        });
    s.add("U41-META-诊断码段独占", section_ok, "");

    // META-2：码两两互异 + 人话原因非空 + 判据容量无截断。
    let mut code_ok = true;
    for i in 0..ALL_CODES.len() {
        for j in 0..ALL_CODES.len() {
            if i != j && ALL_CODES[i].code() == ALL_CODES[j].code() {
                code_ok = false;
            }
        }
    }
    for c in ALL_CODES {
        if c.reason().is_empty() {
            code_ok = false;
        }
    }
    s.add("U41-META-码互异与无截断", code_ok && !s.truncated(), "");

    // META-1：判据条数对账（放末位：此时 len 应为 11，加自身恰 12）。
    s.add("U41-META-判据条数对账", s.len() + 1 == EXPECTED_CHECKS, "");

    s
}
