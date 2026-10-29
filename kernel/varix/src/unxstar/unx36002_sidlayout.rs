//! unx36002_sidlayout — UNX-F36002 SID 内存布局与变长编码（字节序与对齐面）。
//!
//! 判据原文：
//!   J1 变长 SID 序列化→反序列化往返 100 例逐字节一致；非对齐缓冲注入
//!      10 例安全拒绝（不越读）。
//! 设计纪律：
//!   1. 布局锚（与 ADR-UNX-008 布局面一致）：byte0 修订号、byte1 子权限
//!      计数、byte2..8 IA 大端、其后逐个 u32 子权限小端（Windows SID
//!      磁盘序）。变长总长 = 8 + 4×计数。
//!   2. 解码输入缓冲一律按「最短完整长度」先行校验，任何越界读路径
//!      均在长度检查处提前拒绝——不越读一字节；对齐纪律落在 API 入口
//!      （decode_at 偏移视图必须 4 字节对齐），不依赖指针实际地址
//!      （栈数组对齐为 1，地址非确定，不得作为判据样本）。
//!   3. 纯功能线：no_std + alloc，零 unwrap/expect/panic，确定性。

pub const SEG_BASE: u16 = 0x7290;

// ── 诊断码闭集（0x7290~0x72AF，独占）─────────────────────────
const E_SHORT: u16 = SEG_BASE + 1; // 缓冲短于最短完整头
const E_REV: u16 = SEG_BASE + 2; // 解码修订号非 1
const E_CNT: u16 = SEG_BASE + 3; // 解码计数越界（0 或 >15）
const E_TAIL: u16 = SEG_BASE + 4; // 尾部剩余不足以容纳声明的子权
const E_RT: u16 = SEG_BASE + 5; // 往返不一致（编码→解码还原失配）
const E_OUT: u16 = SEG_BASE + 6; // 输出缓冲不足
const E_UNALIGN: u16 = SEG_BASE + 7; // 偏移视图起点未 4 字节对齐（入口拒）

use crate::checks::CheckSet;
use super::unx36001_sidcore::{Sid, MAX_SUBS};

/// 编码后总长（变长公式：8 + 4×计数；const 供数组定容用）。
pub const fn encoded_len(count: usize) -> usize {
    8 + 4 * count
}

/// 序列化：SID → 字节流（写满 out 的前缀，返回写入长度）。
/// 布局锚：rev(1B) cnt(1B) IA(6B 大端) 子权×cnt(各 4B 小端)。
pub fn encode(s: &Sid, out: &mut [u8]) -> Result<usize, u16> {
    let n = encoded_len(s.sub_count as usize);
    if out.len() < n {
        return Err(E_OUT);
    }
    out[0] = s.revision;
    out[1] = s.sub_count;
    let mut i = 0;
    while i < 6 {
        out[2 + i] = s.id_authority[i];
        i += 1;
    }
    let mut j = 0;
    while j < s.sub_count as usize {
        let v = s.sub_authorities[j].to_le_bytes();
        let base = 8 + j * 4;
        out[base] = v[0];
        out[base + 1] = v[1];
        out[base + 2] = v[2];
        out[base + 3] = v[3];
        j += 1;
    }
    Ok(n)
}

/// 反序列化：字节流前缀 → SID + 消费长度。
/// 长度校验先行：修订号/计数/尾部完整度逐级拒，绝不越读。
pub fn decode(buf: &[u8]) -> Result<(Sid, usize), u16> {
    if buf.len() < 8 {
        return Err(E_SHORT);
    }
    let rev = buf[0];
    let cnt = buf[1] as usize;
    if rev != 1 {
        return Err(E_REV);
    }
    if cnt == 0 || cnt > MAX_SUBS {
        return Err(E_CNT);
    }
    let need = encoded_len(cnt);
    if buf.len() < need {
        // 尾部不完整：比 E_SHORT 更精确的拒绝面（声明的子权读不满）。
        return Err(E_TAIL);
    }
    let mut ia = [0u8; 6];
    let mut i = 0;
    while i < 6 {
        ia[i] = buf[2 + i];
        i += 1;
    }
    let mut subs = [0u32; MAX_SUBS];
    let mut j = 0;
    while j < cnt {
        let base = 8 + j * 4;
        subs[j] = u32::from_le_bytes([buf[base], buf[base + 1], buf[base + 2], buf[base + 3]]);
        j += 1;
    }
    match Sid::new(rev, ia, &subs[..cnt]) {
        Ok(s) => Ok((s, need)),
        Err(_) => Err(E_CNT),
    }
}

/// 偏移视图入口：容器内偏移解码（对齐纪律在此强制）。
/// off 必须 4 字节对齐且在界内——非对齐注入 10 例在此确定性拒绝。
pub fn decode_at(buf: &[u8], off: usize) -> Result<(Sid, usize), u16> {
    if off % 4 != 0 {
        return Err(E_UNALIGN);
    }
    if off >= buf.len() {
        return Err(E_SHORT);
    }
    decode(&buf[off..])
}

/// 批量编码：多个 SID 连续落同一缓冲（变长布局的打包面），
/// 返回各 SID 的 (offset, len) 槽表——容器面（unx36015）的上游原语。
/// 缓冲不足显性拒；总长公式逐槽可复算。
pub fn encode_pack(sids: &[Sid], out: &mut [u8]) -> Result<usize, u16> {
    let mut need = 0usize;
    let mut i = 0;
    while i < sids.len() {
        need += encoded_len(sids[i].sub_count as usize);
        i += 1;
    }
    if out.len() < need {
        return Err(E_OUT);
    }
    let mut off = 0usize;
    let mut i = 0;
    while i < sids.len() {
        let n = encode(&sids[i], &mut out[off..])?;
        off += n;
        i += 1;
    }
    Ok(off)
}

/// 批量解码：对打包缓冲逐槽还原（槽长由解码自述，链式推进），
/// 返回消费总长——与 encode_pack 互为逆（打包往返面）。
pub fn decode_pack(buf: &[u8], count: usize) -> Result<(usize, usize), u16> {
    let mut off = 0usize;
    let mut i = 0;
    while i < count {
        let (_, used) = decode(&buf[off..])?;
        off += used;
        i += 1;
    }
    Ok((off, count))
}

/// 往返一致性：编码→解码还原图灵等价（逐字段）。
pub fn roundtrip_equal(a: &Sid, b: &Sid) -> bool {
    a.revision == b.revision
        && a.sub_count == b.sub_count
        && a.id_authority == b.id_authority
        && a.subs() == b.subs()
}

/// 诊断码释义。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_SHORT => "缓冲短于 8 字节最短头",
        E_REV => "解码修订号非 1",
        E_CNT => "解码子权限计数越界（0 或 >15）",
        E_TAIL => "尾部剩余不足容纳声明的子权限",
        E_RT => "序列化→反序列化往返失配",
        E_OUT => "输出缓冲不足",
        E_UNALIGN => "偏移视图起点未 4 字节对齐（decode_at 入口拒）",
        _ => "未知诊断码",
    }
}

/// 确定性演练：100 例往返逐字节一致 + 10 例注入全拒（判据 J1）。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：100 例变长往返——计数 1..=15 循环铺满、IA 与子权由 LCG 生成，
    // 编码→解码逐字段还原 + 重编码逐字节相等。
    let mut seed: u32 = 0x3602_0001;
    let mut n = 0usize;
    while n < 100 {
        let cnt = (n % 15) + 1;
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let ia_val = (0x0000_0005_0000_0000u64) | ((seed & 0xFFFF) as u64) << 8 | 0x21;
        let ia = Sid::ia_from(ia_val);
        let mut subs = [0u32; MAX_SUBS];
        let mut i = 0;
        while i < cnt {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            subs[i] = seed;
            i += 1;
        }
        let s = Sid::new(1, ia, &subs[..cnt])?;
        let mut buf = [0u8; encoded_len(MAX_SUBS)];
        let wlen = encode(&s, &mut buf)?;
        if wlen != encoded_len(cnt) {
            return Err(E_RT);
        }
        let (back, used) = decode(&buf[..wlen])?;
        if used != wlen || !roundtrip_equal(&s, &back) {
            return Err(E_RT);
        }
        // 逐字节一致面：重编码后前缀必须与首次编码完全相同。
        let mut buf2 = [0u8; encoded_len(MAX_SUBS)];
        let wlen2 = encode(&back, &mut buf2)?;
        if wlen2 != wlen || buf[..wlen] != buf2[..wlen2] {
            return Err(E_RT);
        }
        n += 1;
    }
    hits += 1;

    // R2：非对齐/短缺缓冲注入 10 例——全部确定性拒绝且不越读。
    // 族谱：短缺头（E_SHORT）3 例 + 尾部断缺（E_TAIL）3 例 +
    // 计数伪造（E_CNT）2 例 + 修订号伪造（E_REV）2 例 = 10 例。
    let s = Sid::new(1, Sid::ia_from(5), &[21, 100, 1000])?;
    let mut buf = [0u8; encoded_len(3)];
    let w = encode(&s, &mut buf)?;
    if w != encoded_len(3) {
        return Err(E_RT);
    }
    if decode(&buf[..7]).err() != Some(E_SHORT)
        && decode(&buf[..3]).err() != Some(E_SHORT)
        && decode(&buf[..1]).err() != Some(E_SHORT)
    {
        return Err(E_SHORT);
    }
    if decode(&buf[..encoded_len(3) - 1]).err() != Some(E_TAIL) {
        return Err(E_TAIL);
    }
    if decode(&buf[..9]).err() != Some(E_TAIL) {
        return Err(E_TAIL);
    }
    if decode(&buf[..10]).err() != Some(E_TAIL) {
        return Err(E_TAIL);
    }
    let mut bad_cnt = buf;
    bad_cnt[1] = 0;
    if decode(&bad_cnt).err() != Some(E_CNT) {
        return Err(E_CNT);
    }
    let mut bad_cnt2 = buf;
    bad_cnt2[1] = 16;
    if decode(&bad_cnt2).err() != Some(E_CNT) {
        return Err(E_CNT);
    }
    let mut bad_rev = buf;
    bad_rev[0] = 2;
    if decode(&bad_rev).err() != Some(E_REV) {
        return Err(E_REV);
    }
    let mut bad_rev2 = buf;
    bad_rev2[0] = 0;
    if decode(&bad_rev2).err() != Some(E_REV) {
        return Err(E_REV);
    }
    hits += 1;

    // R3：decode_at 对齐注入恰 10 例——[0,13] 内全部非 4 倍数偏移
    // （1,2,3,5,6,7,9,10,11,13）确定性拒绝，不依赖指针地址；
    // 对齐偏移 0 正常通过。
    let mut checked: u32 = 0;
    let mut off = 0usize;
    while off <= 13 {
        if off % 4 != 0 {
            if decode_at(&buf, off) != Err(E_UNALIGN) {
                return Err(E_UNALIGN);
            }
            checked += 1;
        }
        off += 1;
    }
    if checked != 10 {
        return Err(E_UNALIGN);
    }
    let (back0, used0) = decode_at(&buf, 0)?;
    if used0 != w || !roundtrip_equal(&s, &back0) {
        return Err(E_RT);
    }
    hits += 1;

    // R4：输出缓冲不足 + 变长公式守恒。
    let mut tiny = [0u8; 11];
    if encode(&s, &mut tiny).err() != Some(E_OUT) {
        return Err(E_OUT);
    }
    if encoded_len(MAX_SUBS) != 68 || encoded_len(1) != 12 {
        return Err(E_RT);
    }
    hits += 1;

    // R5：打包往返面——20 个 SID（计数 1..=3 循环）连续落同一缓冲，
    // decode_pack 链式还原消费总长与 encode_pack 返回值一致；
    // 缓冲不足显性拒。
    let mut pack = [0u8; 20 * encoded_len(3)];
    let mut sids = [match Sid::new(1, Sid::ia_from(5), &[1]) {
        Ok(v) => v,
        Err(_) => return Err(E_RT),
    }; 20];
    let mut i = 0;
    while i < 20 {
        let cnt = (i % 3) + 1;
        let mut subs = [0u32; MAX_SUBS];
        let mut j = 0;
        while j < cnt {
            subs[j] = (i * 10 + j) as u32 + 1;
            j += 1;
        }
        sids[i] = Sid::new(1, Sid::ia_from(5), &subs[..cnt])?;
        i += 1;
    }
    let packed = encode_pack(&sids, &mut pack)?;
    let (consumed, n) = decode_pack(&pack, 20)?;
    if packed != consumed || n != 20 {
        return Err(E_RT);
    }
    let mut small = [0u8; 30];
    if encode_pack(&sids, &mut small).err() != Some(E_OUT) {
        return Err(E_OUT);
    }
    hits += 1;

    Ok(hits)
}

// ── 判据聚合（CheckSet 面 · 进 unxstar_blocks 检查表）────────
/// 判据 J1 聚合入口：SEG 锚 + 确定性演练全绿（红时回填诊断码）。
pub fn run_unx36002_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36002_sidlayout");
    cs.add("SEG 段位独占", SEG_BASE == 0x7290, "0x7290~0x72AF 本模块独占");
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
