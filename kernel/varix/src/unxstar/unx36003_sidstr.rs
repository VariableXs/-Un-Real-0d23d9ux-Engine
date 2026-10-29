//! unx36003_sidstr — UNX-F36003 SID 字符串格式（S-R-IA-SA…-RID）双向解析。
//!
//! 判据原文：
//!   J1 标准格式串 ↔ 结构双向解析 200 例零差；畸形串注入 50 例
//!      （缺段/超长/非数字）全拒并返回参数错误。
//! 设计纪律：
//!   1. 格式锚：`S-<rev>-<IA>-<sub>…-<RID>`，rev/子权十进制、IA 十进制
//!      或 `0x` 十六进制；子权限 ≥1 且 ≤15；IA 为 48 位（≤
//!      0xFFFF_FFFF_FFFF）。
//!   2. 解析逐段推进：任何空段/非数字/溢出/缺段/超段即拒——畸形串
//!      永不产出半成品 SID。
//!   3. 渲染用定容 208 字节缓冲（最长 SID：4+20+15×11 ≈ 189），零堆
//!      分配；整数转字符自写（无 format! 依赖，字节级确定性）。
//!   4. 纯功能线：no_std，零 unwrap/expect/panic，确定性。

pub const SEG_BASE: u16 = 0x72B0;

// ── 诊断码闭集（0x72B0~0x72CF，独占）─────────────────────────
const E_NO_S: u16 = SEG_BASE + 1; // 串不以 "S-" 起始
const E_SEG: u16 = SEG_BASE + 2; // 空段（连续减号/首尾减号）
const E_NONDIG: u16 = SEG_BASE + 3; // 段内非数字字符
const E_IA: u16 = SEG_BASE + 4; // IA 溢出 48 位或非法十六进制
const E_VAL: u16 = SEG_BASE + 5; // 子权限溢出 u32
const E_REV: u16 = SEG_BASE + 6; // 修订号非 1
const E_SUBS: u16 = SEG_BASE + 7; // 子权限缺失（<1）或超段（>15）
const E_LONG: u16 = SEG_BASE + 8; // 渲染缓冲不足（串超 208 字节）
const E_RT: u16 = SEG_BASE + 9; // 往返失配（render→parse→render 不等）

use crate::checks::CheckSet;
use super::unx36001_sidcore::{Sid, MAX_SUBS};

/// IA 的 48 位上限。
pub const IA_MAX: u64 = 0xFFFF_FFFF_FFFF;
/// 渲染缓冲容量。
pub const RENDER_CAP: usize = 208;

/// 无符号整数写入游标（字节级确定性）。
struct Cursor {
    buf: [u8; RENDER_CAP],
    len: usize,
}

impl Cursor {
    fn new() -> Cursor {
        Cursor { buf: [0u8; RENDER_CAP], len: 0 }
    }
    fn push(&mut self, b: u8) -> Result<(), u16> {
        if self.len >= RENDER_CAP {
            return Err(E_LONG);
        }
        self.buf[self.len] = b;
        self.len += 1;
        Ok(())
    }
    fn push_u64(&mut self, mut v: u64) -> Result<(), u16> {
        // 无前导零十进制；v==0 输出 "0"。
        let mut tmp = [0u8; 20];
        let mut n = 0;
        if v == 0 {
            tmp[0] = b'0';
            n = 1;
        }
        while v > 0 {
            tmp[n] = b'0' + (v % 10) as u8;
            v /= 10;
            n += 1;
        }
        let mut i = n;
        while i > 0 {
            i -= 1;
            self.push(tmp[i])?;
        }
        Ok(())
    }
}

/// 渲染：SID → 标准格式串（定容缓冲，返回有效长度）。
pub fn render(s: &Sid, out: &mut [u8; RENDER_CAP]) -> Result<usize, u16> {
    let mut c = Cursor::new();
    c.push(b'S')?;
    c.push(b'-')?;
    c.push_u64(s.revision as u64)?;
    c.push(b'-')?;
    c.push_u64(s.ia_value())?;
    let mut j = 0;
    while j < s.sub_count as usize {
        c.push(b'-')?;
        c.push_u64(s.sub_authorities[j] as u64)?;
        j += 1;
    }
    if out.len() < c.len {
        return Err(E_LONG);
    }
    let mut i = 0;
    while i < c.len {
        out[i] = c.buf[i];
        i += 1;
    }
    Ok(c.len)
}

/// 十进制/十六进制段解析（checked 逐位累积，溢出即拒）。
/// `hex_ok` 为 true 时接受 `0x` 前缀（IA 专用）。
fn parse_num(seg: &[u8], hex_ok: bool, max: u64) -> Result<u64, u16> {
    if seg.is_empty() {
        return Err(E_SEG);
    }
    let mut val: u64 = 0;
    let mut i = 0;
    let mut radix: u64 = 10;
    if hex_ok && seg.len() >= 2 && seg[0] == b'0' && (seg[1] == b'x' || seg[1] == b'X') {
        radix = 16;
        i = 2;
        if seg.len() == 2 {
            return Err(E_IA);
        }
    }
    while i < seg.len() {
        let d = match seg[i] {
            b'0'..=b'9' => (seg[i] - b'0') as u64,
            b'a'..=b'f' if radix == 16 => (seg[i] - b'a' + 10) as u64,
            b'A'..=b'F' if radix == 16 => (seg[i] - b'A' + 10) as u64,
            _ => return Err(E_NONDIG),
        };
        val = match val.checked_mul(radix).and_then(|v| v.checked_add(d)) {
            Some(v) => v,
            None => return Err(E_IA),
        };
        if val > max {
            return Err(E_IA);
        }
        i += 1;
    }
    Ok(val)
}

/// 解析：标准格式串 → SID（逐段推进，畸形即拒）。
pub fn parse(buf: &[u8]) -> Result<Sid, u16> {
    if buf.len() < 2 || buf[0] != b'S' || buf[1] != b'-' {
        return Err(E_NO_S);
    }
    let mut segs: [&[u8]; 18] = [&[]; 18]; // rev + IA + ≤15 子权 + 哨兵
    let mut nseg = 0;
    let mut start = 2;
    let mut i = 2;
    // 逐段切分：减号为分隔，空段即拒。
    while i <= buf.len() {
        if i == buf.len() || buf[i] == b'-' {
            if i == start {
                return Err(E_SEG);
            }
            if nseg >= 18 {
                return Err(E_SUBS);
            }
            segs[nseg] = &buf[start..i];
            nseg += 1;
            start = i + 1;
        }
        i += 1;
    }
    if nseg < 3 {
        // 至少 rev + IA + 1 个子权限。
        return Err(E_SUBS);
    }
    let rev = parse_num(segs[0], false, 0xFF)? as u8;
    if rev != 1 {
        return Err(E_REV);
    }
    let ia_val = parse_num(segs[1], true, IA_MAX)?;
    let nsub = nseg - 2;
    if nsub > MAX_SUBS {
        return Err(E_SUBS);
    }
    let mut subs = [0u32; MAX_SUBS];
    let mut j = 0;
    while j < nsub {
        let v = parse_num(segs[2 + j], false, 0xFFFF_FFFF)?;
        subs[j] = v as u32;
        j += 1;
    }
    Sid::new(rev, Sid::ia_from(ia_val), &subs[..nsub])
}

/// 结构等价（与布局面同一口径）。
fn same(a: &Sid, b: &Sid) -> bool {
    a.revision == b.revision
        && a.ia_value() == b.ia_value()
        && a.sub_count == b.sub_count
        && a.subs() == b.subs()
}

/// 确定性演练：200 例往返零差 + 50 例畸形注入全拒（判据 J1）。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：200 例 render→parse→render 往返零差。
    let mut seed: u32 = 0x3603_0001;
    let mut n = 0usize;
    while n < 200 {
        let cnt = (n % 15) + 1;
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let ia_val = ((seed as u64) << 16) | 0x21;
        let mut subs = [0u32; MAX_SUBS];
        let mut i = 0;
        while i < cnt {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            subs[i] = seed;
            i += 1;
        }
        let s = Sid::new(1, Sid::ia_from(ia_val), &subs[..cnt])?;
        let mut buf1 = [0u8; RENDER_CAP];
        let l1 = render(&s, &mut buf1)?;
        let p = parse(&buf1[..l1])?;
        if !same(&s, &p) {
            return Err(E_RT);
        }
        let mut buf2 = [0u8; RENDER_CAP];
        let l2 = render(&p, &mut buf2)?;
        if l1 != l2 || buf1[..l1] != buf2[..l2] {
            return Err(E_RT);
        }
        n += 1;
    }
    hits += 1;

    // R2：畸形串注入恰 50 例全拒——缺段/空段/非数字/溢出/超段/修订号错。
    // 基形 10 族 × 5 变体 = 50，逐条断言 Err。
    let mut bads: [[u8; 48]; 50] = [[0u8; 48]; 50];
    let mut blens = [0usize; 50];
    let put = |idx: usize, src: &[u8], lens: &mut [usize; 50], dst: &mut [[u8; 48]; 50]| {
        let mut i = 0;
        while i < src.len() {
            dst[idx][i] = src[i];
            i += 1;
        }
        lens[idx] = src.len();
    };
    // 族1：缺 S 前缀（X-/s 小写/T-）。
    put(0, b"X-1-5-1000", &mut blens, &mut bads);
    put(1, b"s-1-5-1000", &mut blens, &mut bads);
    put(2, b"T-1-5-1000", &mut blens, &mut bads);
    put(3, b"S1-5-1000", &mut blens, &mut bads);
    put(4, b"1-5-1000", &mut blens, &mut bads);
    // 族2：空段。
    put(5, b"S-1--5-1000", &mut blens, &mut bads);
    put(6, b"S--5-1000", &mut blens, &mut bads);
    put(7, b"S-1-5-1000-", &mut blens, &mut bads);
    put(8, b"-S-1-5-1000", &mut blens, &mut bads);
    put(9, b"S-1-5--1000", &mut blens, &mut bads);
    // 族3：非数字。
    put(10, b"S-1-5-a", &mut blens, &mut bads);
    put(11, b"S-1-5-10x0", &mut blens, &mut bads);
    put(12, b"S-1-z-1000", &mut blens, &mut bads);
    put(13, b"S-1-5-10 00", &mut blens, &mut bads);
    put(14, b"S-1-5-0xG1", &mut blens, &mut bads);
    // 族4：子权限超段（16 个子权，>MAX_SUBS=15）。
    put(15, b"S-1-5-1-1-1-1-1-1-1-1-1-1-1-1-1-1-1-1-1", &mut blens, &mut bads);
    put(16, b"S-1-5-2-2-2-2-2-2-2-2-2-2-2-2-2-2-2-2-2", &mut blens, &mut bads);
    put(17, b"S-1-5-3-3-3-3-3-3-3-3-3-3-3-3-3-3-3-3-3", &mut blens, &mut bads);
    put(18, b"S-1-5-4-4-4-4-4-4-4-4-4-4-4-4-4-4-4-4-4", &mut blens, &mut bads);
    put(19, b"S-1-5-5-5-5-5-5-5-5-5-5-5-5-5-5-5-5-5-5", &mut blens, &mut bads);
    // 族5：IA 溢出（>48 位）。
    put(20, b"S-1-0x1000000000000-1000", &mut blens, &mut bads);
    put(21, b"S-1-0xFFFFFFFFFFFFFFFF-1000", &mut blens, &mut bads);
    put(22, b"S-1-281474976710656-1000", &mut blens, &mut bads);
    put(23, b"S-1-99999999999999999-1000", &mut blens, &mut bads);
    put(24, b"S-1-0xzzz-1000", &mut blens, &mut bads);
    // 族6：子权限溢出 u32。
    put(25, b"S-1-5-4294967296", &mut blens, &mut bads);
    put(26, b"S-1-5-99999999999", &mut blens, &mut bads);
    put(27, b"S-1-5-10000000000", &mut blens, &mut bads);
    put(28, b"S-1-5-4294967297", &mut blens, &mut bads);
    put(29, b"S-1-5-18446744073709551616", &mut blens, &mut bads);
    // 族7：修订号非 1。
    put(30, b"S-2-5-1000", &mut blens, &mut bads);
    put(31, b"S-0-5-1000", &mut blens, &mut bads);
    put(32, b"S-3-5-1000", &mut blens, &mut bads);
    put(33, b"S-256-5-1000", &mut blens, &mut bads);
    put(34, b"S-99-5-1000", &mut blens, &mut bads);
    // 族8：缺段（<3 段：rev+IA 但无子权 / 只有 rev）。
    put(35, b"S-1-5", &mut blens, &mut bads);
    put(36, b"S-1", &mut blens, &mut bads);
    put(37, b"S-", &mut blens, &mut bads);
    put(38, b"S", &mut blens, &mut bads);
    put(39, b"S-1-", &mut blens, &mut bads);
    // 族9：十六进制畸形（0x 空体/子权位带 0x）。
    put(40, b"S-1-0x-1000", &mut blens, &mut bads);
    put(41, b"S-1-0X-1000", &mut blens, &mut bads);
    put(42, b"S-1-5-0x10", &mut blens, &mut bads);
    put(43, b"S-1-5-1x", &mut blens, &mut bads);
    put(44, b"S-1-0x-1000-1000", &mut blens, &mut bads);
    // 族10：空串/超短杂形。
    put(45, b"", &mut blens, &mut bads);
    put(46, b"S1", &mut blens, &mut bads);
    put(47, b"X", &mut blens, &mut bads);
    put(48, b"S--", &mut blens, &mut bads);
    put(49, b"S-1-5-1000--2000", &mut blens, &mut bads);
    let mut b = 0usize;
    while b < 50 {
        if parse(&bads[b][..blens[b]]).is_ok() {
            return Err(E_NONDIG);
        }
        b += 1;
    }
    hits += 1;

    // R3：合法十六进制 IA 往返（0x 前缀渲染为十进制，解析等价）。
    let s = Sid::new(1, Sid::ia_from(0x0000_0005_0000_0021), &[1000])?;
    let mut buf = [0u8; RENDER_CAP];
    let l = render(&s, &mut buf)?;
    // 手工构造十六进制形式 S-1-0x500000021-1000 解析后等价。
    let hx = b"S-1-0x500000021-1000";
    let p = parse(hx)?;
    if !same(&s, &p) {
        return Err(E_RT);
    }
    if parse(&buf[..l]) != Ok(p) {
        return Err(E_RT);
    }
    hits += 1;

    // R4：已知锚样例——S-1-1-0（Everyone）与 S-1-5-32-544（Administrators）。
    let ev = parse(b"S-1-1-0")?;
    if ev.ia_value() != 1 || ev.rid() != 0 || ev.sub_count != 1 {
        return Err(E_RT);
    }
    let adm = parse(b"S-1-5-32-544")?;
    if adm.ia_value() != 5 || adm.subs() != [32u32, 544u32] {
        return Err(E_RT);
    }
    hits += 1;

    Ok(hits)
}

// ── 判据聚合（CheckSet 面 · 进 unxstar_blocks 检查表）────────
/// 判据 J1 聚合入口：SEG 锚 + 确定性演练全绿（红时回填诊断码）。
pub fn run_unx36003_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36003_sidstr");
    cs.add("SEG 段位独占", SEG_BASE == 0x72B0, "0x72B0~0x72CF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "演练段全对平、注入全拒"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs
}

/// 诊断码释义。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_NO_S => "串不以 \"S-\" 起始",
        E_SEG => "空段（连续减号/首尾减号）",
        E_NONDIG => "段内非数字字符",
        E_IA => "IA 溢出 48 位或非法十六进制",
        E_VAL => "子权限溢出 u32",
        E_REV => "修订号非 1",
        E_SUBS => "子权限缺失（<1）或超段（>15）",
        E_LONG => "渲染缓冲不足（串超 208 字节）",
        E_RT => "往返失配（render→parse→render 不等）",
        _ => "未知诊断码",
    }
}

#[cfg(test)]
mod tests {
    use super::drill;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(4));
    }
}
