//! unx36016_sidaudit — UNX-F36016 SID 调试/审计打印面（诊断账格式 v1）。
//!
//! 判据原文：
//!   J1 诊断格式输出 100 例与黄金样本一致；未知格式版本拒绝 5 例
//!      （前向兼容保守派）。
//! 设计纪律：
//!   1. 诊断格式 v1（确定性字节面，逐字段定长）：
//!      `SID v1 rev=<2hex> ia=<16hex> cnt=<2hex> sub=[<8hex>,…]
//!      rid=<8hex> fp=<16hex>`——同 SID 双渲染必须逐字节相等。
//!   2. 黄金样本校验器：对输出做**独立结构校验**（前缀/字段序/hex
//!      合法性/字段值与 SID 逐项对平）——校验器与渲染器双端口互证，
//!      渲染漂移即红。
//!   3. 版本保守派：version ≠ 1 一律拒（前向兼容显性化，不猜格式）。
//!   4. 纯功能线：no_std，零 unwrap/expect/panic，零堆分配，确定性。

pub const SEG_BASE: u16 = 0x7450;

// ── 诊断码闭集（0x7450~0x746F，独占）─────────────────────────
const E_VER: u16 = SEG_BASE + 1; // 未知格式版本（保守派拒）
const E_CAP: u16 = SEG_BASE + 2; // 输出缓冲不足
const E_FMT: u16 = SEG_BASE + 3; // 结构校验失败（渲染漂移）
const E_ANCHOR: u16 = SEG_BASE + 4; // 字段值与 SID 对平失配

use crate::checks::CheckSet;
use super::unx36001_sidcore::Sid;

/// 诊断格式版本锚。
pub const DIAG_VERSION: u8 = 1;
/// 输出缓冲容量（最长：头 7 + rev 2 + 5 + ia 16 + 5 + cnt 2 + 子权
/// 15×10 + rid 12 + fp 19 ≈ 205，取 232）。
pub const OUT_CAP: usize = 232;

/// 定长 hex 写入（无前导省略）。
fn push_hex(buf: &mut [u8], pos: &mut usize, v: u64, digits: usize) -> Result<(), u16> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    if *pos + digits > buf.len() {
        return Err(E_CAP);
    }
    let mut i = 0;
    while i < digits {
        let shift = ((digits - 1 - i) * 4) as u32;
        buf[*pos + i] = HEX[((v >> shift) & 0xF) as usize];
        i += 1;
    }
    *pos += digits;
    Ok(())
}

fn push_str(buf: &mut [u8], pos: &mut usize, s: &[u8]) -> Result<(), u16> {
    if *pos + s.len() > buf.len() {
        return Err(E_CAP);
    }
    let mut i = 0;
    while i < s.len() {
        buf[*pos + i] = s[i];
        i += 1;
    }
    *pos += s.len();
    Ok(())
}

/// 渲染：SID → 诊断账格式 v1（返回有效长度）。
pub fn render_v1(version: u8, s: &Sid, out: &mut [u8]) -> Result<usize, u16> {
    if version != DIAG_VERSION {
        return Err(E_VER);
    }
    let mut pos = 0usize;
    push_str(out, &mut pos, b"SID v1 rev=")?;
    push_hex(out, &mut pos, s.revision as u64, 2)?;
    push_str(out, &mut pos, b" ia=")?;
    push_hex(out, &mut pos, s.ia_value(), 16)?;
    push_str(out, &mut pos, b" cnt=")?;
    push_hex(out, &mut pos, s.sub_count as u64, 2)?;
    push_str(out, &mut pos, b" sub=[")?;
    let mut j = 0;
    while j < s.sub_count as usize {
        if j > 0 {
            push_str(out, &mut pos, b",")?;
        }
        push_hex(out, &mut pos, s.sub_authorities[j] as u64, 8)?;
        j += 1;
    }
    push_str(out, &mut pos, b"] rid=")?;
    push_hex(out, &mut pos, s.rid() as u64, 8)?;
    push_str(out, &mut pos, b" fp=")?;
    push_hex(out, &mut pos, s.fingerprint(), 16)?;
    Ok(pos)
}

/// 黄金样本校验器（独立结构校验，与渲染器双端口互证）：
/// 前缀 / 字段序 / hex 合法性 / 字段值逐项对平。
/// 格式：`SID v1 rev=XX ia=I(16) cnt=NN sub=[h(8),…] rid=R(8) fp=F(16)`
pub fn validate_v1(out: &[u8], s: &Sid) -> Result<(), u16> {
    let head: &[u8] = b"SID v1 rev=";
    if out.len() < head.len() + 2 || &out[..head.len()] != head {
        return Err(E_FMT);
    }
    let mut p = head.len();
    // rev=XX（2 hex）→ " ia="
    if out.len() < p + 2 + 4 || &out[p + 2..p + 2 + 4] != b" ia=" {
        return Err(E_FMT);
    }
    if hex_val(&out[p..p + 2]) != s.revision as u64 {
        return Err(E_ANCHOR);
    }
    p += 2 + 4;
    // ia（16 hex）→ " cnt="
    if out.len() < p + 16 + 5 || &out[p + 16..p + 16 + 5] != b" cnt=" {
        return Err(E_FMT);
    }
    if hex_val(&out[p..p + 16]) != s.ia_value() {
        return Err(E_ANCHOR);
    }
    p += 16 + 5;
    // cnt（2 hex）→ " sub=["
    if out.len() < p + 2 + 6 || &out[p + 2..p + 2 + 6] != b" sub=[" {
        return Err(E_FMT);
    }
    if hex_val(&out[p..p + 2]) != s.sub_count as u64 {
        return Err(E_ANCHOR);
    }
    p += 2 + 6;
    // 子权列表：8 hex × cnt，逗号分隔 → "]"
    let mut j = 0;
    while j < s.sub_count as usize {
        if j > 0 {
            if out.len() < p + 1 || out[p] != b',' {
                return Err(E_FMT);
            }
            p += 1;
        }
        if out.len() < p + 8 {
            return Err(E_FMT);
        }
        if hex_val(&out[p..p + 8]) != s.sub_authorities[j] as u64 {
            return Err(E_ANCHOR);
        }
        p += 8;
        j += 1;
    }
    if out.len() < p + 1 || out[p] != b']' {
        return Err(E_FMT);
    }
    p += 1;
    // → " rid=" + 8 hex
    if out.len() < p + 5 + 8 || &out[p..p + 5] != b" rid=" {
        return Err(E_FMT);
    }
    if hex_val(&out[p + 5..p + 5 + 8]) != s.rid() as u64 {
        return Err(E_ANCHOR);
    }
    p += 5 + 8;
    // → " fp=" + 16 hex（恰尽）
    if out.len() < p + 4 + 16 || &out[p..p + 4] != b" fp=" {
        return Err(E_FMT);
    }
    if hex_val(&out[p + 4..p + 4 + 16]) != s.fingerprint() {
        return Err(E_ANCHOR);
    }
    p += 4 + 16;
    if p != out.len() {
        return Err(E_FMT); // 尾部必须恰尽（无冗余字节）
    }
    Ok(())
}

/// hex 串 → 数值（校验器内部，非 hex 即 E_FMT）。
fn hex_val(b: &[u8]) -> u64 {
    let mut v: u64 = 0;
    let mut i = 0;
    while i < b.len() {
        let d = match b[i] {
            b'0'..=b'9' => (b[i] - b'0') as u64,
            b'a'..=b'f' => (b[i] - b'a' + 10) as u64,
            _ => return u64::MAX, // 哨兵：必不与合法字段值相等
        };
        v = (v << 4) | d;
        i += 1;
    }
    v
}

/// 版本白名单（前向兼容保守派）：当前仅 v1；表驱动便于未来显性扩档。
pub const VERSION_WHITELIST: [u8; 1] = [1];

/// 版本是否在白名单（渲染入口外的独立判定面）。
pub fn version_allowed(v: u8) -> bool {
    let mut i = 0;
    while i < VERSION_WHITELIST.len() {
        if VERSION_WHITELIST[i] == v {
            return true;
        }
        i += 1;
    }
    false
}

/// 诊断行长度理论界：最短（cnt=1）与最长（cnt=15）可解析预核。
/// 头 11 + rev 2 + " ia=" 4 + ia 16 + " cnt=" 5 + cnt 2 + " sub=[" 6
/// + 子权（cnt×8 + (cnt-1)×1）+ "]" 1 + " rid=" 5 + rid 8 + " fp=" 4 + fp 16。
pub fn len_bounds() -> (usize, usize) {
    // 固定部分：11+2+4+16+5+2+6+1+5+8+4+16 = 80
    // 子权部分：cnt=1 → 8；cnt=15 → 15×8+14 = 134
    // 最短 = 80+8 = 88；最长 = 80+134 = 214
    (88, 214)
}

/// 诊断码释义。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "未知格式版本（保守派拒）",
        E_CAP => "输出缓冲不足",
        E_FMT => "诊断账结构校验失败（渲染漂移）",
        E_ANCHOR => "字段值与 SID 对平失配",
        _ => "未知诊断码",
    }
}

/// 确定性演练：100 例渲染+校验互证 + 5 例未知版本全拒（判据 J1）。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：100 例——LCG 铺满计数档，渲染→双渲染逐字节相等→独立校验器
    // 全对平。
    let mut seed: u32 = 0x3610_0001;
    let mut n = 0;
    while n < 100 {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let cnt = (n % 15) + 1;
        let mut subs = [0u32; 15];
        let mut i = 0;
        while i < cnt {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            subs[i] = seed;
            i += 1;
        }
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let ia_val = ((seed & 0xFFFF) as u64) << 8 | 0x21;
        let s = Sid::new(1, Sid::ia_from(ia_val), &subs[..cnt])?;
        let mut buf1 = [0u8; OUT_CAP];
        let mut buf2 = [0u8; OUT_CAP];
        let l1 = render_v1(DIAG_VERSION, &s, &mut buf1)?;
        let l2 = render_v1(DIAG_VERSION, &s, &mut buf2)?;
        if l1 != l2 || buf1[..l1] != buf2[..l2] {
            return Err(E_FMT);
        }
        validate_v1(&buf1[..l1], &s)?;
        // 篡改面：末字节翻一位必红（校验器独立发现渲染行被改）。
        let mut tam = buf1;
        tam[l1 - 1] ^= 0x01;
        if validate_v1(&tam[..l1], &s).is_ok() {
            return Err(E_FMT);
        }
        n += 1;
    }
    hits += 1;

    // R2：未知格式版本拒绝 5 例——0/2/16/200/255 全拒（保守派锚）。
    let s = Sid::new(1, Sid::ia_from(5), &[21, 1000])?;
    let mut buf = [0u8; OUT_CAP];
    let bads = [0u8, 2, 16, 200, 255];
    let mut k = 0;
    while k < 5 {
        if render_v1(bads[k], &s, &mut buf) != Err(E_VER) {
            return Err(E_VER);
        }
        k += 1;
    }
    hits += 1;

    // R3：缓冲不足显性拒——16 字节小缓冲必然 E_CAP。
    let mut tiny = [0u8; 16];
    if render_v1(DIAG_VERSION, &s, &mut tiny) != Err(E_CAP) {
        return Err(E_CAP);
    }
    hits += 1;

    // R4：已知锚样本——S-1-5-32-544 的诊断行前缀与 rev/ia/cnt 字段值
    // 逐项对平（人工黄金样本首 32 字节）。
    let adm = Sid::new(1, Sid::ia_from(5), &[32, 544])?;
    let l = render_v1(DIAG_VERSION, &adm, &mut buf)?;
    let gold_prefix: &[u8] = b"SID v1 rev=01 ia=0000000000000005 cnt=02";
    if l < gold_prefix.len() || &buf[..gold_prefix.len()] != gold_prefix {
        return Err(E_ANCHOR);
    }
    validate_v1(&buf[..l], &adm)?;
    hits += 1;

    // R5：版本白名单与长度理论界——白名单 {1} 面（version_allowed 与
    // render_v1 双端口对平）；100 例渲染行长度全部落在理论界内；
    // 统计极值与界一致（cnt=1 最短 / cnt=15 最长）。
    if !version_allowed(1) || version_allowed(0) || version_allowed(2) {
        return Err(E_VER);
    }
    let (lo_b, hi_b) = len_bounds();
    let mut min_l = usize::MAX;
    let mut max_l = 0usize;
    let mut seed2: u32 = 0x3610_0002;
    let mut n = 0;
    while n < 100 {
        seed2 = seed2.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let cnt = (n % 15) + 1;
        let mut subs = [0u32; 15];
        let mut i = 0;
        while i < cnt {
            seed2 = seed2.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            subs[i] = seed2;
            i += 1;
        }
        seed2 = seed2.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let s2 = Sid::new(1, Sid::ia_from(5), &subs[..cnt])?;
        let mut b2 = [0u8; OUT_CAP];
        let l2 = render_v1(DIAG_VERSION, &s2, &mut b2)?;
        if l2 < lo_b || l2 > hi_b {
            return Err(E_FMT);
        }
        validate_v1(&b2[..l2], &s2)?;
        if l2 < min_l {
            min_l = l2;
        }
        if l2 > max_l {
            max_l = l2;
        }
        n += 1;
    }
    if min_l != lo_b || max_l != hi_b {
        return Err(E_FMT);
    }
    hits += 1;

    Ok(hits)
}

// ── 判据聚合（CheckSet 面 · 进 unxstar_blocks 检查表）────────
/// 判据 J1 聚合入口：SEG 锚 + 确定性演练全绿（红时回填诊断码）。
pub fn run_unx36016_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36016_sidaudit");
    cs.add("SEG 段位独占", SEG_BASE == 0x7450, "0x7450~0x746F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "演练段全对平、注入全拒"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs
}

#[cfg(test)]
mod tests {
    use super::drill;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(5));
    }
}
