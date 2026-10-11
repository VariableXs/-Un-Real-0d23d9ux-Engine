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

use alloc::string::ToString;
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
const EXPECTED_CHECKS: usize = 18;

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

    // ================= 四、深化十区：消费面（DEEP） =================

    // 深-01：头部全字段审计——36 字节逐段恰值（creator 面判据侧独立
    // 读字节对拍）+ 与 SdtHeader 双路提取恒等 + revision 分级。
    let t_good = j_dsdt(b"VARIX1", false);
    let audit_ok = match header_audit(&t_good) {
        Some(a) => {
            let sig_ok = a.signature == *b"DSDT";
            let len_ok = a.length == 96;
            let rev_ok = a.revision == 2;
            let oem_ok = a.oem_id[0] == b'V' && a.oem_id[5] == b'1';
            // 判据侧独立读 creator 面（28..32 / 32..36——夹具未写应全零）。
            let cid = [t_good[28], t_good[29], t_good[30], t_good[31]];
            let crev = u32::from_le_bytes([
                t_good[32],
                t_good[33],
                t_good[34],
                t_good[35],
            ]);
            let creator_ok = a.creator_id == cid && a.creator_rev == crev;
            // 双路对拍：SdtHeader::parse 同字节源提取恒等。
            let cross_ok = match crate::acpi::SdtHeader::parse(&t_good) {
                Some(h) => a.matches_sdt_header(&h),
                None => false,
            };
            sig_ok
                && len_ok
                && rev_ok
                && oem_ok
                && creator_ok
                && cross_ok
                && a.revision_class() == 2
                && a.creator_printable()
        }
        None => false,
    };
    // 残表显性 None。
    let audit_none = header_audit(&t_good[..10]).is_none();
    s.add("U41-深-01-头部全字段审计", audit_ok && audit_none, "");

    // 深-02：指针双写四态账——五态恰值（判据侧独立构造各态夹具）。
    let pa_none = pointer_audit(&j_facp(2, 0, 0));
    let pa_x = pointer_audit(&j_facp(2, J_ADDR, 0));
    let pa_d = pointer_audit(&j_facp(1, 0, 0x0009_A000));
    let pa_eq = pointer_audit(&j_facp(2, J_ADDR, J_ADDR as u32));
    let pa_div = pointer_audit(&j_facp(2, 0x0000_9E00_0000, 0x0009_A000));
    let pa_ok = pa_none == PointerAudit::None
        && pa_x == PointerAudit::XOnly(J_ADDR)
        && pa_d == PointerAudit::DsdtOnly(0x0009_A000)
        && pa_eq == PointerAudit::BothEqual(J_ADDR)
        && pa_div == PointerAudit::BothDiverge {
            x: 0x0000_9E00_0000,
            d32: 0x0009_A000,
        }
        && pa_x.locatable()
        && !pa_none.locatable()
        && pa_div.name() == "both-diverge"
        && pa_eq.consistent()
        && !pa_div.consistent();
    s.add("U41-深-02-指针双写四态", pa_ok, "");

    // 深-03：校验和修复建议闭式——建议值写回后 checksum_ok 必真
    // （「建议即修复」对拍：闭式 c' = c - S 代入新和恒零判据侧独立验）
    // + 报告 kv 渲染行数闸（好/坏/截断三态行数 == 结构闭式）。
    let bad = j_dsdt(b"VARIX1", true);
    let repair_ok = match checksum_repair_suggestion(&bad, 96) {
        Some(sug) => {
            let mut fixed = bad.clone();
            fixed[9] = sug;
            crate::acpi::checksum_ok(&fixed, 96)
        }
        None => false,
    };
    // 好表建议 == 原 checksum（闭式对合法表自反恒等）。
    let t_good = j_dsdt(b"VARIX1", false);
    let mut gsum: u8 = 0;
    for b in t_good.iter() {
        gsum = gsum.wrapping_add(*b);
    }
    let repair_self = match checksum_repair_suggestion(&t_good, 96) {
        Some(sug) => sug == t_good[9] && gsum == 0,
        None => false,
    };
    // kv 行数闸三态：好链 8 行（4+4+0）、坏链 6 行（4+1+1）、截断 14 行
    // （4+1+8+1——10 push 显 8）。
    let f_ok = j_facp(2, J_ADDR, 0);
    let r_good = boot_collect(&f_ok, Some((J_ADDR, &j_dsdt(b"VARIX1", false))));
    let r_bad = boot_collect(&f_ok, Some((J_ADDR, &j_dsdt(b"VARIX1", true))));
    let kv_good = report_kv_lines(&r_good);
    let kv_bad = report_kv_lines(&r_bad);
    let mut r_full = DsdtReport::new();
    for _ in 0..(MAX_DIAGS + 2) {
        r_full.push(DsCode::PTR_ZERO);
    }
    let kv_full = report_kv_lines(&r_full);
    let kv_ok = kv_good.len() == report_kv_expected_len(&r_good)
        && kv_bad.len() == report_kv_expected_len(&r_bad)
        && kv_full.len() == report_kv_expected_len(&r_full)
        && kv_good.len() == 8
        && kv_bad.len() == 6
        && kv_full.len() == 14
        && kv_good[1] == "healthy=true"
        && kv_bad[1] == "healthy=false"
        && kv_full.last() == Some(&"diag.truncated=true".to_string());
    s.add("U41-深-03-修复闭式与渲染闸", repair_ok && repair_self && kv_ok, "");

    // 深-04：AML 载荷普查恰值——知名八码全命中 + 扩展前缀两字节原子
    // 步进 + 尾孤前缀终止 + 空载荷/短表平凡（载荷画像面）。
    {
        let mut t = vec![0u8; 36 + 6];
        t[36..42].copy_from_slice(&[0x08, 0x5B, 0x82, 0x14, 0xA4, 0x00]);
        let c1 = aml_census(&t);
        let mixed_ok = c1.ext_ops == 1 && c1.known_single == 4 && c1.aml_len == 6 && c1.bytes_scanned == 6;
        let mut t2 = vec![0u8; 37];
        t2[36] = AML_EXT_PREFIX;
        let c2 = aml_census(&t2);
        let lone_ok = c2.ext_ops == 1 && c2.known_single == 0 && c2.aml_len == 1;
        let mut t3 = vec![0u8; 36 + 8];
        t3[36..44].copy_from_slice(&AML_KNOWN_SINGLE);
        let c3 = aml_census(&t3);
        let all_ok = c3.known_single == 8 && c3.ext_ops == 0;
        let c4 = aml_census(&vec![0u8; 36]);
        let empty_ok = c4.ext_ops == 0 && c4.known_single == 0 && c4.aml_len == 0;
        let c5 = aml_census(&vec![0u8; 10]);
        let short_ok = c5.aml_len == 0 && c5.bytes_scanned == 0;
        s.add("U41-深-04-AML载荷普查", mixed_ok && lone_ok && all_ok && empty_ok && short_ok, "");
    }

    // 深-05：分段校验和链——块数恰值（64B=4 块/36B=3 块尾 4 字节）+
    // whole_sum 与单点 wrap 和恒等（结合律）+ 单字节翻转恰变一块账
    // （坏块定位面）+ 空表零块。
    {
        let t = j_dsdt(b"VARIX1", false);
        let ch = checksum_chain(&t);
        let mut whole: u8 = 0;
        for b in t.iter() {
            whole = whole.wrapping_add(*b);
        }
        let blocks_ok = ch.block_sums.len() == 6 && ch.whole_sum == whole;
        let t36 = vec![0x41u8; 36];
        let ch36 = checksum_chain(&t36);
        let tail_ok = ch36.block_sums.len() == 3 && ch36.block_sums[2] == 0x41u8.wrapping_mul(4);
        let mut bad = t.clone();
        let before = ch.block_sums[2];
        bad[40] ^= 0xFF;
        let chb = checksum_chain(&bad);
        let locate_ok = chb.block_sums.len() == 6
            && chb.block_sums[2] != before
            && chb.block_sums[0] == ch.block_sums[0]
            && chb.block_sums[1] == ch.block_sums[1]
            && chb.block_sums[3] == ch.block_sums[3]
            && chb.whole_sum != ch.whole_sum;
        let ch0 = checksum_chain(&[]);
        let empty_ok = ch0.block_sums.is_empty() && ch0.whole_sum == 0;
        s.add("U41-深-05-分段校验和链", blocks_ok && tail_ok && locate_ok && empty_ok, "");
    }

    // 深-06：表版本演进 diff——同表恒等 + 单字节变分叉恰值 + 尾差
    // （b 是 a 前缀 → 分叉在 b 尾）+ 双空平凡 + 头同尾全异。
    {
        let a = j_dsdt(b"VARIX1", false);
        let b = a.clone();
        let same = diff_tables(&a, &b);
        let same_ok = same.diverge_at.is_none() && same.same_prefix == a.len() && same.a_tail == 0 && same.b_tail == 0;
        let mut c = a.clone();
        c[30] ^= 0xFF;
        let d1 = diff_tables(&a, &c);
        let diverge_ok = d1.diverge_at == Some(30) && d1.same_prefix == 30 && d1.a_tail == a.len() - 30 && d1.b_tail == c.len() - 30;
        let d2 = diff_tables(&a, &a[..36]);
        let tail_ok = d2.same_prefix == 36 && d2.diverge_at == Some(36) && d2.a_tail == a.len() - 36 && d2.b_tail == 0;
        let d3 = diff_tables(&[], &[]);
        let empty_ok = d3.diverge_at.is_none() && d3.same_prefix == 0;
        let mut d4a = vec![0x41u8; 40];
        let mut d4b = vec![0x41u8; 40];
        d4a[39] = 0x00;
        d4b[39] = 0xFF;
        let d4 = diff_tables(&d4a, &d4b);
        let head_ok = d4.same_prefix == 39 && d4.diverge_at == Some(39) && d4.a_tail == 1 && d4.b_tail == 1;
        s.add("U41-深-06-表版本演进diff", same_ok && diverge_ok && tail_ok && empty_ok && head_ok, "");
    }

    // ================= 五、判据自检（META） =================

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
